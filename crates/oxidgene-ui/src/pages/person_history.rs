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
use crate::components::tree_cache::{fetch_tree_cached, use_track_current_person, use_tree_cache};
use crate::components::tree_icon_sidebar::{TreeIconSidebar, TreeSidebarView};
use crate::i18n::use_i18n;
use crate::router::Route;

/// Page rendered at `/trees/:tree_id/persons/:person_id/history`.
#[component]
pub fn PersonHistory(tree_id: String, person_id: String) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let nav = use_navigator();
    let tree_cache = use_tree_cache();

    // Kept in step with the props: the router reuses this component when
    // navigating from one person's history to another's.
    let mut ids = use_signal(|| (tree_id.parse::<Uuid>().ok(), person_id.parse::<Uuid>().ok()));
    let parsed = (tree_id.parse::<Uuid>().ok(), person_id.parse::<Uuid>().ok());
    if parsed != *ids.peek() {
        ids.set(parsed);
    }
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
    let mut changes_only = use_signal(|| true);
    let mut confirm_restore = use_signal(|| false);
    let mut restore_error = use_signal(|| None::<String>);

    let api_tree = api.clone();
    let tree_resource = use_resource(move || {
        let api = api_tree.clone();
        let (tid, _) = ids();
        let _generation = tree_cache.generation();
        async move { Some(fetch_tree_cached(&api, &tree_cache, tid?).await) }
    });

    let api_first = api.clone();
    let first_page = use_resource(move || {
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
            next_cursor.set(
                page.page_info
                    .has_next_page
                    .then(|| page.page_info.end_cursor.clone())
                    .flatten(),
            );
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
    let before_resource = use_resource(move || {
        let api = api_before.clone();
        let (tid, pid) = ids();
        let loaded = versions();
        let latest = loaded.first().map(|v| v.version);
        let wanted = compare_with().or_else(|| selected().or(latest).map(|n| n - 1));
        async move {
            let wanted = wanted.filter(|n| *n >= 1)?;
            if let Some(found) = loaded.iter().find(|v| v.version == wanted) {
                return Some(found.clone());
            }
            api.get_version(tid?, RecordType::Person, pid?, wanted)
                .await
                .ok()
        }
    });
    let before = before_resource
        .read()
        .clone()
        .flatten()
        .filter(|v| Some(v.version) == before_number);

    let tree_name = match &*tree_resource.read() {
        Some(Some(Ok(tree))) => tree.name.clone(),
        _ => ids()
            .0
            .and_then(|tid| tree_cache.tree(tid))
            .map(|tree| tree.name)
            .unwrap_or_default(),
    };
    let person_name = versions
        .read()
        .iter()
        .find_map(|v| match &v.snapshot {
            RecordSnapshot::Person(person) => snapshot_name(person),
            _ => None,
        })
        .unwrap_or_else(|| i18n.t("common.unnamed"));
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
                if let Ok(page) = api
                    .list_versions(tid, RecordType::Person, pid, Some(&cursor))
                    .await
                {
                    more.write()
                        .extend(page.edges.into_iter().map(|edge| edge.node));
                    next_cursor.set(
                        page.page_info
                            .has_next_page
                            .then_some(page.page_info.end_cursor)
                            .flatten(),
                    );
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
                match api
                    .revert_record(tid, RecordType::Person, pid, version)
                    .await
                {
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
        div { class: "sub-page",
            div { class: "td-topbar",
                nav { class: "td-bc",
                    Link { to: Route::Home {}, class: "td-bc-logo",
                        img {
                            src: crate::components::layout::LOGO_PNG_B64,
                            alt: "OxidGene",
                            class: "td-bc-logo-img",
                        }
                    }
                    if !tree_name.is_empty() {
                        Link {
                            to: Route::TreeDetail { tree_id: tree_id.clone(), person: None },
                            class: "td-bc-link",
                            "{tree_name}"
                        }
                        span { class: "td-bc-sep", "/" }
                    }
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
                    span { class: "td-bc-current", {i18n.t("history.breadcrumb")} }
                }
            }

            div { class: "pd-page-shell",
            TreeIconSidebar {
                active_view: TreeSidebarView::None,
                selected_person_id: if is_deleted { None } else { ids().1 },
                show_middle_separator: false,
                show_add_person: false,
                on_profile_view: {
                    let tree_id = tree_id.clone();
                    move |pid: Option<Uuid>| {
                        if let Some(pid) = pid {
                            nav.push(Route::PersonDetail {
                                tree_id: tree_id.clone(),
                                person_id: pid.to_string(),
                            });
                        }
                    }
                },
                on_pedigree_view: {
                    let tree_id = tree_id.clone();
                    move |pid: Option<Uuid>| {
                        nav.push(Route::TreeDetail {
                            tree_id: tree_id.clone(),
                            person: pid.map(|pid| pid.to_string()),
                        });
                    }
                },
                on_add_person: move |_| {},
                on_dictionary: {
                    let tree_id = tree_id.clone();
                    move |_| {
                        nav.push(Route::Dictionary { tree_id: tree_id.clone() });
                    }
                },
                on_settings: {
                    let tree_id = tree_id.clone();
                    move |_| {
                        nav.push(Route::Settings { tree_id: tree_id.clone() });
                    }
                },
            }

            div { class: "sub-page-content ph-content",
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
                            // Timeline of versions, latest first.
                            ol { class: "ph-versions",
                                for version in versions.read().iter() {
                                    li { key: "{version.id}",
                                        button {
                                            class: if Some(version.version) == shown_number { "ph-version active" } else { "ph-version" },
                                            onclick: {
                                                let number = version.version;
                                                move |_| {
                                                    selected.set(Some(number));
                                                    compare_with.set(None);
                                                }
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
                                if next_cursor().is_some() {
                                    li {
                                        button {
                                            class: "btn btn-outline btn-sm ph-more",
                                            disabled: loading_more(),
                                            onclick: load_more,
                                            {i18n.t("history.load_more")}
                                        }
                                    }
                                }
                            }

                            // Comparison of the selected version.
                            if let Some(shown) = shown.clone() {
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
                                                onclick: move |_| {
                                                    restore_error.set(None);
                                                    confirm_restore.set(true);
                                                },
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
                    },
                }
            }
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
