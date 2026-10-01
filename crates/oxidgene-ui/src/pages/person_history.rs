//! Person history page: every recorded version of a person, compared side by
//! side, with the restore of an earlier one. See `docs/ui-person-history.md`.

use dioxus::prelude::*;
use oxidgene_core::history::{RecordSnapshot, RecordType, RecordVersion};
use uuid::Uuid;

use crate::api::ApiClient;
use crate::components::confirm_dialog::ConfirmDialog;
use crate::components::history_diff::{
    HISTORY_STYLES, VersionDiff, describe_entry, entry_details, format_timestamp, snapshot_name,
};
use crate::components::tree_cache::{use_track_current_person, use_tree_cache};
use crate::components::tree_page::{ToolPageFrame, use_tree_page};
use crate::i18n::use_i18n;
use crate::router::Route;
use crate::ui_observability::{
    UiCommand, UiPage, trace_ui_action, use_traced_resource, use_ui_load_trace,
};

/// Page rendered at `/trees/:tree_id/persons/:person_id/history`.
#[component]
pub fn PersonHistory(tree_id: String, person_id: String) -> Element {
    let load_trace = use_ui_load_trace(UiPage::PersonHistory);
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let tree_cache = use_tree_cache();

    // Kept in step with the props: the router reuses this component when
    // navigating from one person's history to another's.
    let parsed = (tree_id.parse::<Uuid>().ok(), person_id.parse::<Uuid>().ok());
    let ids = crate::utils::use_synced(parsed);
    use_track_current_person(parsed.0, parsed.1);

    let mut refresh = use_signal(|| 0u32);
    // Pages loaded after the first, and where the next one starts.
    let mut more = use_signal(Vec::<RecordVersion>::new);
    let mut next_cursor = use_signal(|| None::<String>);
    let mut loading_more = use_signal(|| false);
    // The version shown, and the one it is compared with; `None` follows the
    // latest and its predecessor.
    let mut selected = use_signal(|| None::<i32>);
    let mut compare_with = use_signal(|| None::<i32>);
    let mut confirm_restore = use_signal(|| false);
    let mut restore_error = use_signal(|| None::<String>);

    let page = use_tree_page(&tree_id);

    let api_first = api.clone();
    let first_page = use_traced_resource(load_trace.clone(), "versions", move || {
        let api = api_first.clone();
        let (tid, pid) = ids();
        let _tick = refresh();
        async move {
            let (Some(tid), Some(pid)) = (tid, pid) else {
                return Err(i18n.t("common.invalid_ids"));
            };
            api.list_versions(tid, RecordType::Person, pid, None)
                .await
                .map_err(|e| e.to_string())
        }
    });

    // A new first page starts the list over.
    use_effect(move || {
        if let Some(Ok(page)) = &*first_page.read() {
            more.set(Vec::new());
            next_cursor.set(page_end(&page.page_info));
        }
    });

    let versions = use_memo(move || match &*first_page.read() {
        Some(Ok(page)) => page
            .edges
            .iter()
            .map(|edge| edge.node.clone())
            .chain(more.read().iter().cloned())
            .collect::<Vec<RecordVersion>>(),
        _ => Vec::new(),
    });
    let latest = versions.read().first().map(|v| v.version);
    let shown_number = selected().or(latest);
    let shown = shown_number.and_then(|n| versions.read().iter().find(|v| v.version == n).cloned());
    let before_number = compare_with()
        .or_else(|| shown_number.map(|n| n - 1))
        .filter(|n| *n >= 1);

    // The version compared against, fetched when it lies beyond the pages
    // loaded so far.
    let api_before = api.clone();
    let before_resource = use_traced_resource(load_trace, "compared_version", move || {
        let api = api_before.clone();
        let (tid, pid) = ids();
        let loaded = versions();
        let latest = loaded.first().map(|v| v.version);
        let wanted = compare_with().or_else(|| selected().or(latest).map(|n| n - 1));
        async move { version_numbered(&api, (tid, pid), &loaded, wanted.filter(|n| *n >= 1)?).await }
    });
    let before = before_resource
        .read()
        .clone()
        .flatten()
        .filter(|v| Some(v.version) == before_number);

    let person_name = person_name_in(&versions.read()).unwrap_or_else(|| i18n.t("common.unnamed"));
    let is_deleted = versions.read().first().is_some_and(|v| v.deleted);

    let load_more = {
        let api = api.clone();
        move |_| {
            let api = api.clone();
            let (Some(tid), Some(pid)) = ids() else {
                return;
            };
            let Some(cursor) = next_cursor() else { return };
            loading_more.set(true);
            spawn(async move {
                let page = api
                    .list_versions(tid, RecordType::Person, pid, Some(&cursor))
                    .await;
                if let Ok(page) = page {
                    more.write()
                        .extend(page.edges.into_iter().map(|edge| edge.node));
                    next_cursor.set(page_end(&page.page_info));
                }
                loading_more.set(false);
            });
        }
    };

    let on_restore = {
        let api = api.clone();
        move |_| {
            let api = api.clone();
            let (Some(tid), Some(pid)) = ids() else {
                return;
            };
            let Some(version) = shown_number else { return };
            spawn(async move {
                let reverted = api.revert_record(tid, RecordType::Person, pid, version);
                match trace_ui_action(UiCommand::Restore, reverted).await {
                    Ok(_) => {
                        confirm_restore.set(false);
                        restore_error.set(None);
                        selected.set(None);
                        compare_with.set(None);
                        tree_cache.invalidate();
                        refresh += 1;
                    }
                    Err(error) => restore_error.set(Some(error.to_string())),
                }
            });
        }
    };

    let can_restore = shown
        .as_ref()
        .is_some_and(|v| Some(v.version) != latest && !v.deleted);
    let older_versions: Vec<i32> = versions
        .read()
        .iter()
        .map(|v| v.version)
        .filter(|n| shown_number.is_some_and(|shown| *n < shown))
        .collect();

    rsx! {
        style { {HISTORY_STYLES} }
        style { {PERSON_HISTORY_STYLES} }
        ToolPageFrame {
            tree_id: tree_id.clone(),
            tree_name: page.name(),
            title: i18n.t("history.breadcrumb"),
            print_title: format!("{person_name} / {}", i18n.t("history.breadcrumb")),
            crumbs: rsx! {
                if is_deleted {
                    span { class: "td-bc-link", "{person_name}" }
                } else {
                    Link {
                        to: Route::PersonDetail { tree_id: tree_id.clone(), person_id: person_id.clone() },
                        class: "td-bc-link",
                        "{person_name}"
                    }
                }
                span { class: "td-bc-sep", "/" }
            },
            selected_person_id: if is_deleted { None } else { ids().1 },
            content_class: "ph-content",
            if confirm_restore() {
                ConfirmDialog {
                    title: i18n.t("history.restore_title"),
                    message: i18n.t_args(
                        "history.restore_message",
                        &[
                            ("name", &person_name),
                            ("version", &shown_number.unwrap_or_default().to_string()),
                        ],
                    ),
                    confirm_label: i18n.t("history.restore"),
                    confirm_class: "btn btn-primary",
                    error: restore_error(),
                    on_confirm: on_restore,
                    on_cancel: move |_| {
                        confirm_restore.set(false);
                        restore_error.set(None);
                    },
                }
            }

            div { class: "ph-header",
                h1 { class: "ph-title", {i18n.t_args("history.title", &[("name", &person_name)])} }
                if is_deleted {
                    span { class: "badge ph-deleted-badge", {i18n.t("history.person_deleted")} }
                }
            }

            match &*first_page.read() {
                None => rsx! { div { class: "loading", {i18n.t("common.loading")} } },
                Some(Err(error)) => rsx! {
                    div { class: "error-msg", {i18n.t_args("history.load_error", &[("error", error)])} }
                },
                Some(Ok(_)) if versions.read().is_empty() => rsx! {
                    div { class: "card empty-state",
                        p { {i18n.t("history.no_versions")} }
                    }
                },
                Some(Ok(_)) => rsx! {
                    div { class: "ph-layout",
                        VersionTimeline {
                            versions: versions(),
                            shown: shown_number,
                            has_more: next_cursor().is_some(),
                            loading_more: loading_more(),
                            on_pick: move |number| {
                                selected.set(Some(number));
                                compare_with.set(None);
                            },
                            on_more: load_more,
                        }
                        // Comparison of the selected version.
                        if let Some(shown) = shown.clone() {
                            VersionComparison {
                                shown,
                                before,
                                before_number,
                                older_versions: older_versions.clone(),
                                compare_with,
                                can_restore,
                                on_restore: move |_| {
                                    restore_error.set(None);
                                    confirm_restore.set(true);
                                },
                            }
                        }
                    }
                },
            }
        }
    }
}

/// The person's name, as the latest version naming them has it.
fn person_name_in(versions: &[RecordVersion]) -> Option<String> {
    versions.iter().find_map(|v| match &v.snapshot {
        RecordSnapshot::Person(person) => snapshot_name(person),
        _ => None,
    })
}

/// Where the next page of versions starts, if there is one.
fn page_end(page_info: &oxidgene_core::types::PageInfo) -> Option<String> {
    page_info
        .has_next_page
        .then(|| page_info.end_cursor.clone())
        .flatten()
}

/// Version `wanted` of the person: from the pages `loaded`, else fetched.
async fn version_numbered(
    api: &ApiClient,
    (tid, pid): (Option<Uuid>, Option<Uuid>),
    loaded: &[RecordVersion],
    wanted: i32,
) -> Option<RecordVersion> {
    if let Some(found) = loaded.iter().find(|v| v.version == wanted) {
        return Some(found.clone());
    }
    api.get_version(tid?, RecordType::Person, pid?, wanted)
        .await
        .ok()
}

/// The versions, latest first, the one `shown` highlighted, and the button
/// loading older ones when there are.
#[component]
fn VersionTimeline(
    versions: Vec<RecordVersion>,
    shown: Option<i32>,
    has_more: bool,
    loading_more: bool,
    on_pick: EventHandler<i32>,
    on_more: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    rsx! {
        ol { class: "ph-versions",
            for version in versions.iter() {
                li { key: "{version.id}",
                    button {
                        class: if Some(version.version) == shown { "ph-version active" } else { "ph-version" },
                        onclick: {
                            let number = version.version;
                            move |_| on_pick.call(number)
                        },
                        span { class: "ph-version-number",
                            {i18n.t_args("history.version_n", &[("version", &version.version.to_string())])}
                        }
                        span { class: "ph-version-date", {format_timestamp(&i18n, version.created_at)} }
                        span { class: "ph-version-what", {describe_entry(&i18n, &version.entry)} }
                        if let Some(details) = entry_details(&i18n, &version.entry) {
                            span { class: "ph-version-details", "{details}" }
                        }
                    }
                }
            }
            if has_more {
                li {
                    button {
                        class: "btn btn-outline btn-sm ph-more",
                        disabled: loading_more,
                        onclick: move |_| on_more.call(()),
                        {i18n.t("history.load_more")}
                    }
                }
            }
        }
    }
}

/// The version `shown` against `before`, the version chosen among the older
/// ones, with the changes alone or everything, and the restore button when
/// it can be restored.
#[component]
fn VersionComparison(
    shown: RecordVersion,
    before: Option<RecordVersion>,
    before_number: Option<i32>,
    older_versions: Vec<i32>,
    compare_with: Signal<Option<i32>>,
    can_restore: bool,
    on_restore: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let mut changes_only = use_signal(|| true);
    rsx! {
        div { class: "card ph-diff",
            div { class: "ph-toolbar",
                label { class: "ph-compare",
                    span { {i18n.t("history.compare_with")} }
                    select {
                        value: before_number.map(|n| n.to_string()).unwrap_or_default(),
                        onchange: move |event| {
                            compare_with.set(event.value().parse().ok());
                        },
                        if older_versions.is_empty() {
                            option { value: "", {i18n.t("history.no_previous")} }
                        }
                        for number in older_versions.iter() {
                            option {
                                value: "{number}",
                                {i18n.t_args("history.version_n", &[("version", &number.to_string())])}
                            }
                        }
                    }
                }
                label { class: "ph-toggle",
                    input {
                        r#type: "checkbox",
                        checked: changes_only(),
                        onchange: move |event| changes_only.set(event.checked()),
                    }
                    span { {i18n.t("history.changes_only")} }
                }
                if can_restore {
                    button {
                        class: "btn btn-primary btn-sm",
                        onclick: move |_| on_restore.call(()),
                        {i18n.t("history.restore")}
                    }
                }
            }
            VersionDiff {
                before,
                after: shown,
                changes_only: changes_only(),
            }
        }
    }
}

const PERSON_HISTORY_STYLES: &str = r#"
    .ph-content { display: flex; flex-direction: column; gap: 16px; }
    .ph-header { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; }
    .ph-title {
        font-family: var(--font-heading);
        font-size: 1.4rem;
        color: var(--text-primary);
        margin: 0;
    }
    .ph-deleted-badge { color: var(--danger-text); border-color: var(--danger); }
    .ph-layout {
        display: grid;
        grid-template-columns: minmax(220px, 280px) 1fr;
        gap: 16px;
        align-items: start;
    }
    .ph-versions {
        list-style: none;
        margin: 0;
        padding: 0;
        display: flex;
        flex-direction: column;
        gap: 6px;
    }
    .ph-version {
        width: 100%;
        display: flex;
        flex-direction: column;
        gap: 2px;
        text-align: start;
        padding: 10px 12px;
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        color: var(--text-primary);
        cursor: pointer;
        font-family: var(--font-sans);
    }
    .ph-version:hover { background: var(--bg-card-hover); }
    .ph-version.active { border-color: var(--orange); box-shadow: var(--shadow-sm); }
    .ph-version-number { font-weight: 700; font-size: 0.85rem; }
    .ph-version-date { font-size: 0.75rem; color: var(--text-muted); }
    .ph-version-what { font-size: 0.8rem; color: var(--text-secondary); }
    .ph-version-details { font-size: 0.75rem; color: var(--text-muted); }
    .ph-more { align-self: flex-start; }
    .ph-diff { display: flex; flex-direction: column; gap: 14px; min-width: 0; }
    .ph-toolbar {
        display: flex;
        align-items: center;
        gap: 16px;
        flex-wrap: wrap;
    }
    .ph-toolbar .btn { margin-inline-start: auto; }
    .ph-compare, .ph-toggle {
        display: inline-flex;
        align-items: center;
        gap: 8px;
        font-size: 0.85rem;
        color: var(--text-secondary);
        white-space: nowrap;
    }

    @media (max-width: 768px) {
        .ph-layout { grid-template-columns: 1fr; }
        .ph-versions { flex-direction: row; overflow-x: auto; }
        .ph-versions li { flex: 0 0 200px; }
    }
"#;
