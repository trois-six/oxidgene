//! The tree's audit log: every write, newest first, each opening onto the
//! versions it produced. Shown in the Tools › History section of the tree
//! settings. See `docs/ui-settings.md` §11.

use dioxus::prelude::*;
use oxidgene_core::history::{
    AuditCategory, AuditEntry, AuditSubject, RecordSnapshot, RecordType, VersionChange,
};
use uuid::Uuid;

use crate::api::ApiClient;
use crate::components::confirm_dialog::ConfirmDialog;
use crate::components::empty_state::EmptyState;
use crate::components::history_diff::{
    HISTORY_STYLES, VersionDiff, describe_entry, entry_details, format_timestamp, snapshot_name,
};
use crate::components::paged_list::{PagedList, use_paged_list};
use crate::components::tree_cache::use_tree_cache;
use crate::i18n::{I18n, use_i18n};
use crate::router::Route;
use crate::ui_observability::{UiCommand, trace_ui_action};

/// The Tools › History section of the tree settings.
#[component]
pub fn AuditLogSection(tree_id: Uuid) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let mut category = use_signal(|| None::<AuditCategory>);
    let refresh = use_signal(|| 0u32);
    let entries = use_paged_list("audit_entries", move |cursor: Option<String>| {
        let api = api.clone();
        let category = category();
        let _tick = refresh();
        async move {
            api.list_audit(tree_id, category, cursor.as_deref())
                .await
                .map_err(|e| e.to_string())
        }
    });

    let filters: Vec<(Option<AuditCategory>, String)> =
        std::iter::once((None, i18n.t("history.category.all")))
            .chain(
                AuditCategory::ALL
                    .iter()
                    .map(|c| (Some(*c), i18n.t(&format!("history.category.{c}")))),
            )
            .collect();

    rsx! {
        style { {HISTORY_STYLES} }
        style { {AUDIT_LOG_STYLES} }
        div { class: "settings-section",
            span { class: "settings-section-eyebrow", {i18n.t("settings.tools")} }
            h2 { class: "settings-section-title", {i18n.t("settings.history")} }
            p { class: "settings-section-subtitle", {i18n.t("history.audit_desc")} }

            div { class: "al-filters", role: "group", aria_label: i18n.t("history.filter"),
                for (value, label) in filters {
                    button {
                        key: "{label}",
                        class: if category() == value { "al-filter active" } else { "al-filter" },
                        aria_pressed: if category() == value { "true" } else { "false" },
                        onclick: move |_| category.set(value),
                        "{label}"
                    }
                }
            }

            match &*entries.first.read() {
                None => rsx! { div { class: "loading", {i18n.t("common.loading")} } },
                Some(Err(error)) => rsx! {
                    div { class: "error-msg", {i18n.t_args("history.load_error", &[("error", error)])} }
                },
                Some(Ok(_)) => {
                    let loaded: Vec<AuditEntry> = entries.items();
                    rsx! {
                        if loaded.is_empty() {
                            EmptyState { class: "card", p { {i18n.t("history.no_entries")} } }
                        }
                        ol { class: "al-entries",
                            for entry in loaded {
                                AuditEntryRow { key: "{entry.id}", entry, refresh }
                            }
                        }
                        {load_more_button(&i18n, &entries)}
                    }
                }
            }
        }
    }
}

/// One line of the log, opening onto the states it replaced.
#[component]
fn AuditEntryRow(entry: AuditEntry, refresh: Signal<u32>) -> Element {
    let i18n = use_i18n();
    let mut open = use_signal(|| false);
    let tree_id = entry.tree_id.to_string();
    let label = entry.label.clone().unwrap_or_default();
    let subject_link = match (entry.subject, entry.subject_id) {
        (Some(AuditSubject::Person), Some(id)) => Some(Route::PersonHistory {
            tree_id: tree_id.clone(),
            person_id: id.to_string(),
        }),
        (Some(AuditSubject::Family), Some(id)) => Some(Route::CoupleDetail {
            tree_id: tree_id.clone(),
            family_id: id.to_string(),
        }),
        _ => None,
    };
    let category = entry.category;

    rsx! {
        li { class: "al-entry card",
            div { class: "al-entry-head",
                span { class: "al-time", {format_timestamp(&i18n, entry.occurred_at)} }
                span { class: "badge al-category al-category-{category}",
                    {i18n.t(&format!("history.category.{category}"))}
                }
                span { class: "al-what", {describe_entry(&i18n, &entry)} }
                if !label.is_empty() {
                    match subject_link {
                        Some(route) => rsx! { Link { to: route, class: "al-subject", "{label}" } },
                        None => rsx! { span { class: "al-subject", "{label}" } },
                    }
                }
                if entry.version_count > 0 {
                    button {
                        class: "btn btn-outline btn-sm al-toggle",
                        aria_expanded: if open() { "true" } else { "false" },
                        onclick: move |_| open.toggle(),
                        if open() {
                            {i18n.t("history.hide_changes")}
                        } else {
                            {i18n.t_plural("history.show_changes", entry.version_count as usize)}
                        }
                    }
                }
            }
            if let Some(details) = entry_details(&i18n, &entry) {
                div { class: "al-details", "{details}" }
            }
            if open() {
                EntryChanges { tree_id: entry.tree_id, entry_id: entry.id, refresh }
            }
        }
    }
}

/// The states one write replaced, each compared with the state it produced.
#[component]
fn EntryChanges(tree_id: Uuid, entry_id: Uuid, refresh: Signal<u32>) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let tree_cache = use_tree_cache();
    let mut restoring = use_signal(|| None::<VersionChange>);
    let mut restore_error = use_signal(|| None::<String>);

    let api_changes = api.clone();
    let changes = use_paged_list("audit_changes", move |cursor: Option<String>| {
        let api = api_changes.clone();
        async move {
            api.list_audit_changes(tree_id, entry_id, cursor.as_deref())
                .await
                .map_err(|e| e.to_string())
        }
    });

    let on_restore = {
        let api = api.clone();
        move |_| {
            let api = api.clone();
            let Some(change) = restoring() else { return };
            let (previous, version) = (change.previous, change.version);
            spawn(async move {
                let reverted = api.revert_record(
                    tree_id,
                    version.record_type,
                    version.record_id,
                    previous.version,
                );
                match trace_ui_action(UiCommand::Restore, reverted).await {
                    Ok(_) => {
                        restoring.set(None);
                        restore_error.set(None);
                        tree_cache.invalidate();
                        refresh += 1;
                    }
                    Err(error) => restore_error.set(Some(error.to_string())),
                }
            });
        }
    };

    rsx! {
        div { class: "al-changes",
            if let Some(change) = restoring() {
                ConfirmDialog {
                    title: i18n.t("history.undo_title"),
                    message: i18n.t_args(
                        "history.undo_message",
                        &[
                            ("record", &record_title(&i18n, &change)),
                            (
                                "version",
                                &change.previous.version.to_string(),
                            ),
                        ],
                    ),
                    confirm_label: i18n.t("history.restore"),
                    confirm_class: "btn btn-primary",
                    error: restore_error(),
                    on_confirm: on_restore,
                    on_cancel: move |_| {
                        restoring.set(None);
                        restore_error.set(None);
                    },
                }
            }
            match &*changes.first.read() {
                None => rsx! { div { class: "loading", {i18n.t("common.loading")} } },
                Some(Err(error)) => rsx! {
                    div { class: "error-msg", {i18n.t_args("history.load_error", &[("error", error)])} }
                },
                Some(Ok(_)) => {
                    let loaded: Vec<VersionChange> = changes.items();
                    rsx! {
                        for change in loaded {
                            div { class: "al-change", key: "{change.version.id}",
                                div { class: "al-change-head",
                                    span { class: "al-change-record", {record_title(&i18n, &change)} }
                                    if change.version.record_type == RecordType::Person {
                                        Link {
                                            class: "btn btn-outline btn-sm",
                                            to: Route::PersonHistory {
                                                tree_id: tree_id.to_string(),
                                                person_id: change.version.record_id.to_string(),
                                            },
                                            {i18n.t("history.open_person_history")}
                                        }
                                    }
                                    if !change.previous.deleted {
                                        button {
                                            class: "btn btn-outline btn-sm",
                                            onclick: {
                                                let change = change.clone();
                                                move |_| {
                                                    restore_error.set(None);
                                                    restoring.set(Some(change.clone()));
                                                }
                                            },
                                            {i18n.t("history.undo")}
                                        }
                                    }
                                }
                                VersionDiff {
                                    before: Some(change.previous.clone()),
                                    after: change.version.clone(),
                                }
                            }
                        }
                        {load_more_button(&i18n, &changes)}
                    }
                }
            }
        }
    }
}

/// The "Load more" button under a paged list, while a page is left.
pub(crate) fn load_more_button<T: Clone + 'static>(i18n: &I18n, list: &PagedList<T>) -> Element {
    if !list.has_more() {
        return rsx! {};
    }
    let list = list.clone();
    rsx! {
        button {
            class: "btn btn-outline btn-sm",
            disabled: list.loading_more(),
            onclick: move |_| list.load_more(),
            {i18n.t("history.load_more")}
        }
    }
}

/// « Personne — Alpha Fixture », « Lieu — Springfield ».
fn record_title(i18n: &I18n, change: &VersionChange) -> String {
    let version = &change.version;
    let kind = i18n.t(&format!("history.record.{}", version.record_type));
    // A deletion produced no state: the name is the one the record had.
    let snapshot = version
        .snapshot
        .as_ref()
        .or(change.previous.snapshot.as_ref());
    let name = match snapshot {
        Some(RecordSnapshot::Person(person)) => snapshot_name(person),
        Some(RecordSnapshot::Place(place)) => Some(place.name.clone()),
        Some(RecordSnapshot::Source(source)) => Some(source.title.clone()),
        Some(RecordSnapshot::Repository(repository)) => Some(repository.name.clone()),
        Some(RecordSnapshot::Tree(tree)) => Some(tree.name.clone()),
        None => None,
    };
    match name {
        Some(name) => format!("{kind} — {name}"),
        None => kind,
    }
}

const AUDIT_LOG_STYLES: &str = r#"
    .al-filters { display: flex; flex-wrap: wrap; gap: 6px; margin: 12px 0 16px; }
    .al-filter {
        padding: 4px 12px;
        border-radius: 14px;
        border: 1px solid var(--border);
        background: transparent;
        color: var(--text-secondary);
        font-size: 0.8rem;
        cursor: pointer;
        font-family: var(--font-sans);
    }
    .al-filter:hover { background: var(--bg-card-hover); color: var(--text-primary); }
    .al-filter.active { border-color: var(--orange); color: var(--orange); font-weight: 600; }
    .al-entries {
        list-style: none;
        margin: 0 0 12px;
        padding: 0;
        display: flex;
        flex-direction: column;
        gap: 8px;
    }
    .al-entry { padding: 12px 14px; display: flex; flex-direction: column; gap: 6px; }
    .al-entry-head {
        display: flex;
        align-items: center;
        flex-wrap: wrap;
        gap: 8px 12px;
    }
    .al-time { font-size: 0.78rem; color: var(--text-muted); min-width: 140px; }
    .al-what { font-size: 0.875rem; color: var(--text-primary); }
    .al-subject { font-size: 0.875rem; font-weight: 700; color: var(--orange); }
    .al-toggle { margin-inline-start: auto; }
    .al-details { font-size: 0.78rem; color: var(--text-secondary); }
    .al-category-data { color: var(--blue); border-color: var(--blue); }
    .al-category-settings { color: var(--text-primary); }
    .al-category-media { color: var(--pink); border-color: var(--pink); }
    .al-category-import, .al-category-export { color: var(--green); border-color: var(--green); }
    .al-category-history { color: var(--orange); border-color: var(--orange); }
    .al-changes {
        display: flex;
        flex-direction: column;
        gap: 18px;
        margin-top: 8px;
        padding-top: 12px;
        border-top: 1px solid var(--border);
    }
    .al-change { display: flex; flex-direction: column; gap: 10px; }
    .al-change-head { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; }
    .al-change-head a.btn { text-decoration: none; }
    .al-change-record { font-weight: 700; color: var(--text-primary); margin-inline-end: auto; }
"#;
