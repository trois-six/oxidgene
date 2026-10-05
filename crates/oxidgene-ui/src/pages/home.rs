//! Home / landing page — tree dashboard.

use std::collections::HashMap;

use chrono::Utc;
use dioxus::prelude::*;
use oxidgene_core::Sex;
use oxidgene_core::projection::SearchEntry;
use uuid::Uuid;

use crate::api::{ApiClient, CreateTreeBody, DuplicateTreeBody, UpdateTreeBody};
use crate::components::confirm_dialog::ConfirmDialog;
use crate::components::context_menu::ContextMenuSurface;
use crate::components::empty_state::EmptyState;
use crate::components::import_modal::ImportModal;
use crate::components::modal::Modal;
use crate::components::search_person::{PersonSearchSummary, render_person_search_summary};
use crate::components::tree_cache::use_tree_cache;
use crate::components::view_toggle::ViewToggle;
use crate::i18n::use_i18n;
use crate::router::Route;
use crate::ui_observability::{
    UiCommand, UiPage, trace_ui_action, use_traced_resource, use_ui_load_trace, use_ui_resource,
};

/// How many recently modified persons a tree card lists.
const RECENT_PERSONS: usize = 5;

/// Dashboard shown at `/`.
#[component]
pub fn Home() -> Element {
    let api = use_context::<ApiClient>();
    let load_trace = use_ui_load_trace(UiPage::Home);
    let mut refresh_counter = use_signal(|| 0u32);
    // Mutating a tree from here must also reach the shared `TreeCache`: it is
    // keyed by tree id alone, so a snapshot loaded before the change survives
    // it, and the tree pages would go on rendering stale metadata (the old
    // name in the breadcrumb) from that snapshot.
    let tree_cache = use_tree_cache();

    let api_res = api.clone();
    let trees_resource = use_traced_resource(load_trace.clone(), "trees", move || {
        let api = api_res.clone();
        let _tick = refresh_counter();
        async move { api.list_trees(Some(100), None).await }
    });
    // Every card's recent persons, in one request once the list is in.
    let api_recent = api.clone();
    let recent_resource = use_traced_resource(load_trace.clone(), "recent_persons", move || {
        let api = api_recent.clone();
        let tree_ids: Vec<Uuid> = trees_resource
            .read()
            .as_ref()
            .and_then(|result| result.as_ref().ok())
            .map(|connection| connection.edges.iter().map(|edge| edge.node.id).collect())
            .unwrap_or_default();
        async move {
            api.recent_persons_of_trees(&tree_ids, RECENT_PERSONS)
                .await
                .ok()
        }
    });
    use_context_provider(|| RecentPersonsBatch(recent_resource));
    let imports_active = use_memo(move || {
        trees_resource
            .read()
            .as_ref()
            .and_then(|result| result.as_ref().ok())
            .is_some_and(|connection| {
                connection
                    .edges
                    .iter()
                    .any(|edge| edge.node.import_in_progress)
            })
    });
    let api_poll = api.clone();
    use_effect(move || {
        if imports_active() {
            spawn(poll_imports(
                api_poll.clone(),
                imports_active,
                refresh_counter,
            ));
        }
    });

    let mut show_create = use_signal(|| false);
    // Delete confirmation state: the tree and its name.
    let mut confirm_delete = use_signal(|| None::<(Uuid, String)>);
    let mut delete_error = use_signal(|| None::<String>);
    // Set while the delete request is in flight, so the dialog can show a
    // spinner instead of sitting there looking hung.
    let mut deleting = use_signal(|| false);
    // Import state: which tree the modal is importing into, and its name for
    // the modal's title. The modal owns everything else about the run.
    let mut importing = use_signal(|| None::<(Uuid, String)>);
    // Errors from the card actions that have nowhere of their own to show one.
    let action_error = use_signal(|| None::<String>);
    let mut renaming = use_signal(|| None::<(Uuid, String)>);
    let duplicating_tree_id = use_signal(|| None::<Uuid>);
    // Tree card whose action menu is open, with the fixed menu's viewport
    // coordinates. Only one card menu may be open at a time.
    let open_menu = use_signal(|| None::<(String, f64, f64)>);
    let search_query = use_signal(String::new);
    let sort_mode = use_signal(|| "recent".to_string());
    let list_view = use_signal(|| false);

    let i18n = use_i18n();

    let api_del = api.clone();
    let on_confirm_delete = move |_| {
        let api = api_del.clone();
        // Guard against a second click landing before the first request
        // returns — the dialog stays open for the whole round-trip.
        let Some((id, _)) = confirm_delete().filter(|_| !deleting()) else {
            return;
        };
        deleting.set(true);
        spawn(async move {
            let result = trace_ui_action(UiCommand::DeleteTree, api.delete_tree(id)).await;
            deleting.set(false);
            match result {
                Ok(_) => {
                    confirm_delete.set(None);
                    delete_error.set(None);
                    tree_cache.forget(id);
                    refresh_counter += 1;
                }
                Err(e) => delete_error.set(Some(format!("{e}"))),
            }
        });
    };
    let api_dup = api.clone();
    let on_duplicate = move |(tid, name): (Uuid, String)| {
        let name = format!("{name}{}", i18n.t("home.duplicate_suffix"));
        spawn(duplicate_tree(
            api_dup.clone(),
            tid,
            name,
            duplicating_tree_id,
            refresh_counter,
            action_error,
        ));
    };

    rsx! {
        // Fixed background gear decorations
        div { class: "gear-bg gear-1" }
        div { class: "gear-bg gear-2" }

        div { class: "home-page",
            div { class: "home-main",
                HomeHeader {}
                HomeToolbar { search_query, sort_mode, list_view, show_create }

                // ── Trees grid ───────────────────────────────────────
                match &*trees_resource.read() {
                    Some(Ok(conn)) => rsx! {
                        TreesGrid {
                            trees: conn.edges.iter().map(|edge| TreeSummary::of(&edge.node)).collect::<Vec<_>>(),
                            query: search_query(),
                            sort: sort_mode(),
                            list: list_view(),
                            open_menu,
                            duplicating: duplicating_tree_id(),
                            on_create: move |_| show_create.set(true),
                            on_rename: move |tree| renaming.set(Some(tree)),
                            on_duplicate,
                            on_delete: move |tree| {
                                confirm_delete.set(Some(tree));
                                delete_error.set(None);
                            },
                            // Opens the import modal rather than a native
                            // file picker: a Geneanet import needs three
                            // inputs and a login, none of which a file
                            // dialog can ask for.
                            on_import: move |tree| importing.set(Some(tree)),
                        }
                    },
                    Some(Err(_)) => rsx! {
                        div { class: "error-msg", {i18n.t("home.load_error")} }
                    },
                    None => rsx! {
                        div { class: "loading", {i18n.t("home.loading")} }
                    },
                }
            }
        }

        if show_create() {
            CreateTreeModal {
                on_close: move |_| show_create.set(false),
                on_created: move |_| {
                    show_create.set(false);
                    refresh_counter += 1;
                },
            }
        }

        // ── Delete confirmation ───────────────────────────────────────
        if let Some((_, name)) = confirm_delete() {
            ConfirmDialog {
                title: i18n.t("confirm.delete_tree.title"),
                message: i18n.t_args("confirm.delete_tree.message_name", &[("name", &name)]),
                confirm_label: if deleting() { i18n.t("common.deleting") } else { i18n.t("common.delete") },
                confirm_class: "btn btn-danger",
                error: delete_error(),
                busy: deleting(),
                on_confirm: on_confirm_delete,
                on_cancel: move |_| {
                    confirm_delete.set(None);
                    delete_error.set(None);
                },
            }
        }

        if let Some((tid, name)) = renaming() {
            RenameTreeModal {
                tree_id: tid,
                name,
                on_close: move |_| renaming.set(None),
                on_renamed: move |_| {
                    renaming.set(None);
                    refresh_counter += 1;
                },
            }
        }

        // ── Duplicate blocking overlay ──
        if duplicating_tree_id().is_some() {
            div { class: "import-overlay",
                div { class: "spinner" }
                div { class: "import-overlay-text", {i18n.t("common.duplicating")} }
            }
        }

        // ── Import modal ────────────────────────────────────────────
        if let Some((tid, name)) = importing() {
            ImportModal {
                tree_id: tid,
                tree_name: name,
                on_close: move |_| importing.set(None),
                // The modal stays open on its result screen; what Home has to
                // do is forget the stale snapshot and re-read the list, since
                // an import changes the tree's counts and its updated_at.
                on_imported: move |_| {
                    tree_cache.invalidate();
                    refresh_counter += 1;
                },
            }
        }

        // ── Card action feedback ──
        if let Some(err) = action_error() {
            div { class: "home-import-banner error-msg", "{err}" }
        }

        style { {HOME_STYLES} }
    }
}

/// Re-reads the tree list every second while an import runs.
async fn poll_imports(
    api: ApiClient,
    imports_active: Memo<bool>,
    mut refresh_counter: Signal<u32>,
) {
    while imports_active() {
        crate::utils::sleep_ms(1_000).await;
        api.invalidate_tree_list();
        refresh_counter += 1;
    }
}

/// Duplicates tree `tid` as `name`, blocking the page meanwhile.
async fn duplicate_tree(
    api: ApiClient,
    tid: Uuid,
    name: String,
    mut duplicating: Signal<Option<Uuid>>,
    mut refresh_counter: Signal<u32>,
    mut action_error: Signal<Option<String>>,
) {
    duplicating.set(Some(tid));
    let body = DuplicateTreeBody { name };
    let result = trace_ui_action(UiCommand::DuplicateTree, api.duplicate_tree(tid, &body)).await;
    duplicating.set(None);
    match result {
        Ok(_) => refresh_counter += 1,
        Err(e) => action_error.set(Some(format!("{e}"))),
    }
}

/// What a tree card shows of a tree.
#[derive(Clone, PartialEq)]
struct TreeSummary {
    id: Uuid,
    name: String,
    description: String,
    updated_at: chrono::DateTime<Utc>,
    importing: bool,
}

impl TreeSummary {
    fn of(tree: &crate::api::TreeListItem) -> Self {
        Self {
            id: tree.id,
            name: tree.name.clone(),
            description: tree.description.clone().unwrap_or_default(),
            updated_at: tree.updated_at,
            importing: tree.import_in_progress,
        }
    }
}

/// The page title and the link to the application settings.
#[component]
fn HomeHeader() -> Element {
    let i18n = use_i18n();
    rsx! {
        div { class: "home-page-header",
            div { class: "home-page-header-row",
                h1 {
                    {i18n.t("home.title_prefix")}
                    span { class: "home-title-accent", {i18n.t("home.title_accent")} }
                }
                Link {
                    to: Route::AppSettings {},
                    class: "home-settings-btn",
                    title: "{i18n.t(\"app_settings.title\")}",
                    "aria-label": "{i18n.t(\"app_settings.title\")}",
                    svg {
                        width: "20",
                        height: "20",
                        fill: "none",
                        "viewBox": "0 0 24 24",
                        stroke: "currentColor",
                        "strokeWidth": "1.8",
                        // Gear icon
                        path { d: "M12 15a3 3 0 100-6 3 3 0 000 6z" }
                        path { d: "M19.4 15a1.65 1.65 0 00.33 1.82l.06.06a2 2 0 01-2.83 2.83l-.06-.06a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 01-4 0v-.09A1.65 1.65 0 009 19.4a1.65 1.65 0 00-1.82.33l-.06.06a2 2 0 01-2.83-2.83l.06-.06A1.65 1.65 0 004.68 15a1.65 1.65 0 00-1.51-1H3a2 2 0 010-4h.09A1.65 1.65 0 004.6 9a1.65 1.65 0 00-.33-1.82l-.06-.06a2 2 0 012.83-2.83l.06.06A1.65 1.65 0 009 4.68a1.65 1.65 0 001-1.51V3a2 2 0 014 0v.09a1.65 1.65 0 001 1.51 1.65 1.65 0 001.82-.33l.06-.06a2 2 0 012.83 2.83l-.06.06A1.65 1.65 0 0019.32 9a1.65 1.65 0 001.51 1H21a2 2 0 010 4h-.09a1.65 1.65 0 00-1.51 1z" }
                    }
                }
            }
            p { class: "home-subtitle",
                {i18n.t("home.subtitle")}
            }
        }
    }
}

/// Search, sort, the list / grid switch and the new-tree button.
#[component]
fn HomeToolbar(
    search_query: Signal<String>,
    sort_mode: Signal<String>,
    list_view: Signal<bool>,
    show_create: Signal<bool>,
) -> Element {
    let i18n = use_i18n();
    rsx! {
        div { class: "home-toolbar",
            div { class: "home-search-box",
                svg {
                    class: "home-search-icon",
                    width: "15",
                    height: "15",
                    fill: "none",
                    "viewBox": "0 0 24 24",
                    stroke: "currentColor",
                    "strokeWidth": "2",
                    circle { cx: "11", cy: "11", r: "8" }
                    path { d: "M21 21l-4.35-4.35" }
                }
                input {
                    r#type: "text",
                    class: "home-search-input",
                    placeholder: i18n.t("home.search_placeholder"),
                    value: "{search_query}",
                    oninput: move |e: Event<FormData>| search_query.set(e.value()),
                }
            }
            select {
                class: "home-sort-select",
                value: "{sort_mode}",
                onchange: move |e: Event<FormData>| sort_mode.set(e.value()),
                option { value: "recent", {i18n.t("home.sort_recent")} }
                option { value: "name", {i18n.t("home.sort_name")} }
                option { value: "name_desc", {i18n.t("home.sort_name_desc")} }
            }
            ViewToggle {
                list: list_view(),
                list_label: i18n.t("home.view_list"),
                grid_label: i18n.t("home.view_grid"),
                on_change: move |list| list_view.set(list),
            }
            button {
                class: "home-btn-new home-btn-new-toolbar",
                title: "{i18n.t(\"home.new_tree\")}",
                "aria-label": "{i18n.t(\"home.new_tree\")}",
                onclick: move |_| show_create.set(true),
                svg {
                    width: "13",
                    height: "13",
                    fill: "none",
                    "viewBox": "0 0 24 24",
                    stroke: "currentColor",
                    "strokeWidth": "2.5",
                    path { d: "M12 5v14M5 12h14" }
                }
                span { class: "home-btn-new-label", {i18n.t("home.new_tree")} }
            }
        }
    }
}

/// The trees matching `query` in their name or description, sorted by name
/// (`name`, or `name_desc` from Z to A) or else by last change, newest
/// first.
fn visible_trees(trees: &[TreeSummary], query: &str, sort: &str) -> Vec<TreeSummary> {
    let query = query.to_lowercase();
    let mut visible: Vec<TreeSummary> = trees
        .iter()
        .filter(|tree| {
            query.is_empty()
                || tree.name.to_lowercase().contains(&query)
                || tree.description.to_lowercase().contains(&query)
        })
        .cloned()
        .collect();
    match sort {
        "name" => visible.sort_by_cached_key(|tree| tree.name.to_lowercase()),
        "name_desc" => {
            visible.sort_by_cached_key(|tree| std::cmp::Reverse(tree.name.to_lowercase()));
        }
        _ => visible.sort_by_key(|tree| std::cmp::Reverse(tree.updated_at)),
    }
    visible
}

/// The tree cards and the add card; an invitation when there is no tree,
/// a note when none matches the search.
#[component]
fn TreesGrid(
    trees: Vec<TreeSummary>,
    query: String,
    sort: String,
    /// One card per row rather than as many as fit.
    list: bool,
    open_menu: Signal<Option<(String, f64, f64)>>,
    duplicating: Option<Uuid>,
    on_create: EventHandler<()>,
    on_rename: EventHandler<(Uuid, String)>,
    on_duplicate: EventHandler<(Uuid, String)>,
    on_delete: EventHandler<(Uuid, String)>,
    on_import: EventHandler<(Uuid, String)>,
) -> Element {
    let i18n = use_i18n();
    let visible = visible_trees(&trees, &query, &sort);
    if trees.is_empty() {
        return rsx! {
            EmptyState {
                class: "home-empty",
                icon: rsx! { "🌳" },
                title: i18n.t("home.no_trees"),
                action: rsx! {
                    button {
                        class: "home-btn-new",
                        onclick: move |_| on_create.call(()),
                        {i18n.t("home.new_tree")}
                    }
                },
                p { {i18n.t("home.no_trees_hint")} }
            }
        };
    }
    if visible.is_empty() {
        return rsx! {
            EmptyState {
                class: "home-empty",
                icon: rsx! {
                    svg {
                        width: "48",
                        height: "48",
                        fill: "none",
                        "viewBox": "0 0 24 24",
                        stroke: "var(--text-muted)",
                        "strokeWidth": "1.5",
                        circle { cx: "11", cy: "11", r: "8" }
                        path { d: "M21 21l-4.35-4.35" }
                    }
                },
                title: i18n.t("home.no_search_results"),
                p { {i18n.t("home.no_search_results_hint")} }
            }
        };
    }
    rsx! {
        div { class: if list { "trees-grid trees-list" } else { "trees-grid" },
            for tree in visible {
                TreeCard {
                    key: "{tree.id}",
                    name: tree.name.clone(),
                    description: tree.description.clone(),
                    updated_at: tree.updated_at,
                    tree_id: tree.id.to_string(),
                    duplicating: duplicating == Some(tree.id),
                    importing: tree.importing,
                    open_menu,
                    on_rename: {
                        let tree = (tree.id, tree.name.clone());
                        move |_| on_rename.call(tree.clone())
                    },
                    on_duplicate: {
                        let tree = (tree.id, tree.name.clone());
                        move |_| on_duplicate.call(tree.clone())
                    },
                    on_delete: {
                        let tree = (tree.id, tree.name.clone());
                        move |_| on_delete.call(tree.clone())
                    },
                    on_import: {
                        let tree = (tree.id, tree.name.clone());
                        move |_| on_import.call(tree.clone())
                    },
                }
            }

            // Add-new card
            div {
                class: "tree-card tree-card-add",
                onclick: move |_| on_create.call(()),
                div { class: "tree-card-add-icon", "＋" }
                div { class: "tree-card-add-text", {i18n.t("home.add_card_title")} }
                div { class: "tree-card-add-sub",
                    {i18n.t("home.add_card_subtitle")}
                }
            }
        }
    }
}

/// The modal creating a tree from a name and a description.
#[component]
fn CreateTreeModal(on_close: EventHandler<()>, on_created: EventHandler<()>) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let mut new_name = use_signal(String::new);
    let mut new_desc = use_signal(String::new);
    let mut form_error = use_signal(|| None::<String>);
    let on_create = move |_| {
        let api = api.clone();
        let name = new_name().trim().to_string();
        let desc = new_desc().trim().to_string();
        if name.is_empty() {
            form_error.set(Some(i18n.t("tree.form.name_required")));
            return;
        }
        spawn(async move {
            let body = CreateTreeBody {
                name,
                description: Some(desc).filter(|desc| !desc.is_empty()),
            };
            match api.create_tree(&body).await {
                Ok(_) => on_created.call(()),
                Err(e) => form_error.set(Some(format!("{e}"))),
            }
        });
    };
    rsx! {
        Modal {
            class: "home-create-modal",
            label: i18n.t("home.new_tree"),
            on_close,

            div { class: "home-create-modal-header",
                h2 { {i18n.t("home.new_tree")} }
                button {
                    class: "person-form-close",
                    onclick: move |_| on_close.call(()),
                    "✕"
                }
            }

            div { class: "home-create-modal-body",
                if let Some(err) = form_error() {
                    div { class: "error-msg", "{err}" }
                }
                div { class: "form-group",
                    label { {i18n.t("tree.form.name_label")} }
                    input {
                        r#type: "text",
                        placeholder: i18n.t("tree.form.name_placeholder"),
                        value: "{new_name}",
                        oninput: move |e: Event<FormData>| new_name.set(e.value()),
                    }
                }
                div { class: "form-group",
                    label { {i18n.t("tree.form.description_label")} }
                    textarea {
                        rows: 3,
                        placeholder: i18n.t("tree.form.description_placeholder"),
                        value: "{new_desc}",
                        oninput: move |e: Event<FormData>| new_desc.set(e.value()),
                    }
                }
                div { class: "modal-actions",
                    button {
                        class: "btn btn-outline",
                        onclick: move |_| on_close.call(()),
                        {i18n.t("common.cancel")}
                    }
                    button {
                        class: "btn btn-primary",
                        onclick: on_create,
                        {i18n.t("common.create")}
                    }
                }
            }
        }
    }
}

/// The modal renaming tree `tree_id`, now called `name`.
#[component]
fn RenameTreeModal(
    tree_id: Uuid,
    name: String,
    on_close: EventHandler<()>,
    on_renamed: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let tree_cache = use_tree_cache();
    let mut rename_name = use_signal(|| name.clone());
    let mut rename_error = use_signal(|| None::<String>);
    let on_save = move |_| {
        let api = api.clone();
        let name = rename_name().trim().to_string();
        if name.is_empty() {
            rename_error.set(Some(i18n.t("tree.form.name_required").to_string()));
            return;
        }
        spawn(async move {
            let body = UpdateTreeBody {
                name: Some(name),
                ..Default::default()
            };
            match api.update_tree(tree_id, &body).await {
                Ok(tree) => {
                    tree_cache.refresh_tree(tree_id, tree);
                    on_renamed.call(());
                }
                Err(e) => rename_error.set(Some(format!("{e}"))),
            }
        });
    };
    rsx! {
        Modal {
            class: "home-create-modal",
            label: i18n.t("home.rename_tree"),
            on_close,

            div { class: "home-create-modal-header",
                h2 { {i18n.t("home.rename_tree")} }
                button {
                    class: "person-form-close",
                    onclick: move |_| on_close.call(()),
                    "\u{2715}"
                }
            }

            div { class: "home-create-modal-body",
                if let Some(err) = rename_error() {
                    div { class: "error-msg", "{err}" }
                }
                div { class: "form-group",
                    label { {i18n.t("tree.form.name_label")} }
                    input {
                        r#type: "text",
                        placeholder: i18n.t("tree.form.name_placeholder"),
                        value: "{rename_name}",
                        oninput: move |e: Event<FormData>| rename_name.set(e.value()),
                    }
                }
                div { class: "modal-actions",
                    button {
                        class: "btn btn-outline",
                        onclick: move |_| on_close.call(()),
                        {i18n.t("common.cancel")}
                    }
                    button {
                        class: "btn btn-primary",
                        onclick: on_save,
                        {i18n.t("common.save")}
                    }
                }
            }
        }
    }
}

/// How long ago a tree changed, `diff` ago: today, yesterday, in days,
/// weeks, then months.
fn modified_ago(i18n: &crate::i18n::I18n, diff: chrono::TimeDelta) -> String {
    let days = diff.num_days();
    let with = |key: &str, n: i64| i18n.t_args(key, &[("count", &n.to_string())]);
    match days {
        0 => i18n.t("home.modified_today"),
        1 => i18n.t("home.modified_1day"),
        ..30 => with("home.modified_days", days),
        30..365 if diff.num_weeks() == 1 => with("home.modified_weeks_one", 1),
        30..365 => with("home.modified_weeks_other", diff.num_weeks()),
        _ if days / 30 == 1 => with("home.modified_months_one", 1),
        _ => with("home.modified_months_other", days / 30),
    }
}

/// Individual tree card in the grid.
#[component]
fn TreeCard(
    name: String,
    description: String,
    updated_at: chrono::DateTime<Utc>,
    tree_id: String,
    duplicating: bool,
    importing: bool,
    /// Id of the card whose dropdown is open, owned by [`Home`] so the
    /// click-outside backdrop can be rendered outside the animated grid.
    open_menu: Signal<Option<(String, f64, f64)>>,
    on_rename: EventHandler<()>,
    on_duplicate: EventHandler<()>,
    on_delete: EventHandler<()>,
    on_import: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let nav = use_navigator();
    let mut open_menu = open_menu;
    let menu_position = (!importing)
        .then(|| {
            open_menu
                .read()
                .as_ref()
                .and_then(|(id, x, y)| (id == &tree_id).then_some((*x, *y)))
        })
        .flatten();
    let menu_open = menu_position.is_some();
    let toggle_id = tree_id.clone();

    let diff = Utc::now().signed_duration_since(updated_at);
    let time_ago = modified_ago(&i18n, diff);

    let is_recent = diff.num_hours() < 24;

    // Lift the card above the backdrop while its dropdown is open.
    let card_class = if importing {
        "tree-card is-importing"
    } else if menu_open {
        "tree-card tree-card-menu-open"
    } else {
        "tree-card"
    };

    // The whole card is a click target for the tree view; the Open link stays
    // as the keyboard-reachable control for the same destination.
    let card_route = Route::TreeDetail {
        tree_id: tree_id.clone(),
        person: None,
    };

    rsx! {
        div {
            class: "{card_class}",
            onclick: move |_| {
                if !importing {
                    nav.push(card_route.clone());
                }
            },
            // ── Card body ──────────────────────────────────────────
            div { class: "tree-card-body",
                div { class: "tree-card-header",
                    div { class: "tree-card-name", "{name}" }
                    // Three-dot menu
                    if !importing {
                        div { class: "tree-card-menu-wrapper",
                            button {
                                class: "tree-card-menu-btn",
                                title: i18n.t("home.tree_actions"),
                                "aria-label": i18n.t("home.tree_actions"),
                                "aria-haspopup": "menu",
                                "aria-expanded": menu_open,
                                onclick: move |e: Event<MouseData>| {
                                    e.stop_propagation();
                                    if menu_open {
                                        open_menu.set(None);
                                    } else {
                                        let point = e.client_coordinates();
                                        open_menu.set(Some((
                                            toggle_id.clone(),
                                            point.x,
                                            point.y + 18.0,
                                        )));
                                    }
                                },
                                "⋮"
                            }
                            if let Some((x, y)) = menu_position {
                            ContextMenuSurface {
                                x,
                                y,
                                menu_class: "context-menu-anchor-right".to_string(),
                                on_close: move |_| open_menu.set(None),
                                Link {
                                    to: Route::TreeDetail { tree_id: tree_id.clone(), person: None },
                                    class: "context-menu-item",
                                    onclick: move |_| open_menu.set(None),
                                    {i18n.t("common.open")}
                                }
                                button {
                                    class: "context-menu-item",
                                    onclick: move |e: Event<MouseData>| {
                                        e.stop_propagation();
                                        open_menu.set(None);
                                        on_rename.call(());
                                    },
                                    {i18n.t("common.rename")}
                                }
                                button {
                                    class: "context-menu-item",
                                    disabled: duplicating,
                                    onclick: move |e: Event<MouseData>| {
                                        e.stop_propagation();
                                        open_menu.set(None);
                                        on_duplicate.call(());
                                    },
                                    if duplicating { {i18n.t("common.duplicating")} } else { {i18n.t("common.duplicate")} }
                                }
                                button {
                                    class: "context-menu-item",
                                    onclick: move |e: Event<MouseData>| {
                                        e.stop_propagation();
                                        open_menu.set(None);
                                        on_import.call(());
                                    },
                                    {i18n.t("common.import")}
                                }
                                Link {
                                    to: Route::Settings { tree_id: tree_id.clone() },
                                    class: "context-menu-item",
                                    onclick: move |_| open_menu.set(None),
                                    {i18n.t("common.settings")}
                                }
                                button {
                                    class: "context-menu-item context-menu-danger",
                                    onclick: move |e: Event<MouseData>| {
                                        e.stop_propagation();
                                        open_menu.set(None);
                                        on_delete.call(());
                                    },
                                    {i18n.t("common.delete")}
                                }
                            }
                        }
                    }
                    }
                }
                if !description.is_empty() {
                    div { class: "tree-card-desc", "{description}" }
                }
                // An importing tree is still being written: its persons are
                // read once the import is over, when this block mounts.
                if let (false, Ok(id)) = (importing, Uuid::parse_str(&tree_id)) {
                    RecentPersons { tree_id: id }
                }
                div { class: "tree-card-footer",
                    div { class: "tree-card-footer-left",
                        span { class: "tree-last-update", "{time_ago}" }
                        if is_recent {
                            span { class: "tree-badge-recent", {i18n.t("home.badge_recent")} }
                        }
                    }
                    if !importing {
                        Link {
                            to: Route::TreeDetail { tree_id: tree_id.clone(), person: None },
                            class: "btn-open",
                            onclick: move |e: Event<MouseData>| e.stop_propagation(),
                            {i18n.t("common.open")}
                        }
                    }
                }
            }
            if importing {
                div {
                    class: "tree-card-import-overlay",
                    role: "status",
                    "aria-live": "polite",
                    div { class: "spinner" }
                    div { class: "tree-card-import-title", {i18n.t("home.import_in_progress")} }
                }
            }
        }
    }
}

/// Every card's recent persons, read once by the home page; `None` while on
/// its way or when it failed.
#[derive(Clone, Copy)]
struct RecentPersonsBatch(Resource<Option<HashMap<Uuid, Vec<SearchEntry>>>>);

/// The persons of a tree card's tree modified most recently.
///
/// Each is the shared search row, drawn statically — the card, not the row,
/// reacts to hover — and opens the tree view centred on that person instead
/// of letting the click reach the card, which opens it on the root.
#[component]
fn RecentPersons(tree_id: Uuid) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();

    let RecentPersonsBatch(batch) = use_context::<RecentPersonsBatch>();
    // The home page reads every card's rows in one request.
    let rows = use_memo(move || {
        let batch = batch.read();
        let batch = batch.as_ref()?.as_ref()?;
        Some(batch.get(&tree_id).cloned().unwrap_or_default())
    });
    // Pictures follow the rows rather than holding them back.
    let portraits = use_ui_resource("home_recent_portraits", move || {
        let api = api.clone();
        let entries = rows().unwrap_or_default();
        async move { api.entry_portraits(tree_id, &entries).await }
    });

    // Nothing while loading or when the read failed: the card works without.
    let Some(entries) = rows() else {
        return rsx! {};
    };
    let portraits = portraits.read().clone().unwrap_or_default();
    let tree_id = tree_id.to_string();

    rsx! {
        div { class: "tree-card-persons",
            div { class: "tree-card-persons-title", {i18n.t("home.recent_persons")} }
            if entries.is_empty() {
                div { class: "tree-card-persons-empty", {i18n.t("home.no_recent_persons")} }
            }
            for entry in entries.iter() {{
                let summary = PersonSearchSummary::from(entry);
                let sex_class = match summary.sex() {
                    Sex::Male => "male",
                    Sex::Female => "female",
                    Sex::Unknown => "",
                };
                rsx! {
                    Link {
                        key: "{entry.person_id}",
                        to: Route::TreeDetail {
                            tree_id: tree_id.clone(),
                            person: Some(entry.person_id.to_string()),
                        },
                        class: "search-person-result tree-card-person {sex_class}",
                        onclick: move |e: Event<MouseData>| e.stop_propagation(),
                        {render_person_search_summary(
                            &summary,
                            portraits.get(&entry.person_id).cloned(),
                            &i18n,
                        )}
                    }
                }
            }}
        }
    }
}

const HOME_STYLES: &str = r#"
    /* ── Gear background decorations ────────────────────────────── */

    /* Static on purpose. The gears used to turn — three infinite
       rotations — and a WebView repaints the whole layer on every frame for
       that: the idle home page held a CPU core at about 90% on desktop. */
    .gear-bg {
        position: fixed;
        border-radius: 50%;
        border: 2px solid var(--border);
        opacity: 0.12;
        pointer-events: none;
        z-index: 0;
    }

    .gear-bg::before {
        content: '';
        position: absolute;
        inset: -12px;
        border-radius: 50%;
        border: 2px dashed var(--text-muted);
        opacity: 0.5;
    }

    .gear-1 {
        width: 320px;
        height: 320px;
        top: -80px;
        right: -80px;
    }

    .gear-2 {
        width: 200px;
        height: 200px;
        bottom: 80px;
        left: -60px;
    }

    /* ── Home page wrapper ───────────────────────────────────────── */

    .home-page {
        flex: 1;
        overflow-y: auto;
        position: relative;
        z-index: 1;
    }

    .home-main {
        max-width: 1200px;
        margin: 0 auto;
        padding: var(--space-24) var(--space-12) var(--space-40);
        width: 100%;
    }

    /* ── Page header ─────────────────────────────────────────────── */

    .home-page-header {
        margin-bottom: var(--space-20);
        animation: home-fade-up 0.6s ease backwards;
    }

    .home-page-header h1 {
        font-family: var(--font-heading);
        font-size: var(--text-200);
        font-weight: 700;
        color: var(--text-primary);
        letter-spacing: 0.03em;
    }

    .home-page-header-row {
        display: flex;
        align-items: center;
        justify-content: space-between;
    }

    .home-settings-btn {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 36px;
        height: 36px;
        border-radius: var(--radius);
        color: var(--text-muted);
        background: var(--bg-card);
        border: 1px solid var(--border);
        text-decoration: none;
        transition: color 0.2s, border-color 0.2s, background 0.2s;
        flex-shrink: 0;
    }

    .home-settings-btn:hover {
        color: var(--orange);
        border-color: var(--orange);
        background: var(--bg-card-hover);
    }

    .home-title-accent {
        background: linear-gradient(135deg, var(--orange) 0%, var(--orange-light) 100%);
        -webkit-background-clip: text;
        -webkit-text-fill-color: transparent;
        background-clip: text;
    }

    .home-subtitle {
        margin-top: var(--space-3);
        color: var(--text-secondary);
        font-size: var(--text-95);
        font-weight: 300;
    }

    /* ── Toolbar ───────────────────────────────────────────────────── */

    .home-toolbar {
        display: flex;
        align-items: center;
        gap: var(--space-6);
        margin-bottom: var(--space-16);
        flex-wrap: wrap;
        animation: home-fade-up 0.6s 0.08s ease backwards;
    }

    .home-search-box {
        display: flex;
        align-items: center;
        gap: var(--space-4);
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        padding: var(--space-4) var(--space-6);
        flex: 3;
        min-width: 0;
        transition: border-color 0.2s;
    }

    .home-search-box:focus-within {
        border-color: var(--orange);
    }

    .home-search-icon {
        color: var(--text-muted);
        flex-shrink: 0;
    }

    .home-search-input {
        background: none;
        border: none;
        outline: none;
        color: var(--text-primary);
        font-size: var(--text-85);
        font-family: var(--font-sans);
        width: 100%;
    }

    .home-search-input::placeholder {
        color: var(--text-muted);
    }

    .home-sort-select {
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        padding: var(--space-4) var(--space-6);
        color: var(--text-secondary);
        font-size: var(--text-80);
        font-family: var(--font-sans);
        cursor: pointer;
        transition: border-color 0.2s;
        flex: 1;
        min-width: 0;
    }

    .home-sort-select:focus {
        outline: none;
        border-color: var(--orange);
    }

    .home-btn-new {
        display: inline-flex;
        align-items: center;
        gap: var(--space-4);
        background: linear-gradient(135deg, var(--orange) 0%, var(--orange-light) 100%);
        border: none;
        border-radius: var(--radius);
        padding: var(--space-4) var(--space-9);
        color: var(--white);
        font-family: var(--font-heading);
        font-size: var(--text-80);
        font-weight: 600;
        letter-spacing: 0.05em;
        cursor: pointer;
        transition: opacity 0.2s, transform 0.15s;
        box-shadow: 0 2px 12px color-mix(in srgb, var(--orange) 35%, transparent);
        text-decoration: none;
        white-space: nowrap;
        flex-shrink: 0;
    }

    .home-btn-new:hover {
        opacity: 0.9;
        transform: translateY(-1px);
    }

    /* ── Trees grid ──────────────────────────────────────────────── */

    .trees-grid {
        display: grid;
        grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
        gap: var(--space-12);
        animation: home-fade-up 0.6s 0.15s ease backwards;
    }
    .trees-grid.trees-list { grid-template-columns: minmax(0, 1fr); }

    /* ── Tree card ───────────────────────────────────────────────── */

    .tree-card {
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: var(--radius-lg);
        min-width: 0;
        cursor: pointer;
        /* Lifted by `top`, not `transform`: a transformed card becomes the
           containing block of its `position: fixed` menu, which then opens
           offset by the card's position — for the whole of the lift's
           transition, and for good in a WebView that keeps that layout. */
        transition: top 0.25s, border-color 0.25s, box-shadow 0.25s, background 0.25s;
        position: relative;
        top: 0;
        display: flex;
        flex-direction: column;
    }

    .tree-card:hover {
        top: -4px;
        border-color: var(--orange);
        box-shadow: 0 8px 40px color-mix(in srgb, var(--orange) 18%, transparent), 0 2px 12px color-mix(in srgb, var(--shadow-black) 50%, transparent);
        background: var(--bg-card-hover);
    }

    .tree-card.is-importing,
    .tree-card.is-importing:hover {
        top: 0;
        border-color: var(--border);
        box-shadow: none;
        background: var(--bg-card);
        cursor: wait;
    }

    .tree-card-import-overlay {
        position: absolute;
        inset: 0;
        z-index: 2;
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: var(--space-6);
        border-radius: inherit;
        background: color-mix(in srgb, var(--bg-card) 88%, transparent);
        backdrop-filter: blur(2px);
        color: var(--text-primary);
        text-align: center;
        padding: var(--space-12);
    }

    .tree-card-import-overlay .spinner {
        --spinner-size: 28px;
    }

    .tree-card-import-title {
        font-family: var(--font-heading);
        font-size: var(--text-95);
        font-weight: 600;
    }

    .tree-card-body {
        padding: var(--space-10) var(--space-11) var(--space-11);
        flex: 1;
        min-width: 0;
        display: flex;
        flex-direction: column;
    }

    .tree-card-header {
        display: flex;
        align-items: flex-start;
        justify-content: space-between;
        gap: var(--space-4);
        margin-bottom: var(--space-2);
    }

    .tree-card-name {
        font-family: var(--font-heading);
        font-size: var(--text-110);
        font-weight: 600;
        color: var(--text-primary);
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }

    .tree-card-menu-wrapper {
        position: relative;
        flex-shrink: 0;
    }

    .tree-card-menu-btn {
        background: none;
        border: none;
        color: var(--text-muted);
        font-size: var(--text-120);
        cursor: pointer;
        padding: var(--space-1) var(--space-3);
        border-radius: var(--radius-sm);
        line-height: 1;
        transition: background 0.15s, color 0.15s;
    }

    .tree-card-menu-btn:hover {
        background: var(--bg-card-hover);
        color: var(--text-primary);
    }

    /* The card whose menu is open stays still and above its neighbours. */
    .tree-card-menu-open,
    .tree-card-menu-open:hover {
        top: 0;
        z-index: 320;
    }

    .tree-card-desc {
        font-size: var(--text-80);
        color: var(--text-secondary);
        margin-bottom: var(--space-6);
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }

    /* ── Recently modified persons ─────────────────────────────────
       The shared search rows, static: the card is what reacts to hover,
       and a row only says it leads somewhere by lighting its name. */

    /* Grows, so the footer sits at the bottom of a card stretched to the
       height of a neighbour listing more persons. */
    .tree-card-persons {
        flex: 1;
        min-width: 0;
        margin-top: var(--space-4);
    }

    .tree-card-persons-title {
        font-size: var(--text-70);
        text-transform: uppercase;
        letter-spacing: 0.06em;
        color: var(--text-muted);
        margin-bottom: var(--space-3);
    }

    .tree-card-persons-empty {
        font-size: var(--text-80);
        color: var(--text-muted);
    }

    .search-person-result.tree-card-person:hover {
        background: none;
    }

    /* One line per person, so five fit a card: a small portrait, the name
       and the years; the relatives and the birthplace are left to the
       search results, where telling namesakes apart matters. */
    .search-person-result.tree-card-person {
        padding: 3px var(--space-3);
    }

    .tree-card-person .sp-result-rel,
    .tree-card-person .sp-result-meta {
        display: none;
    }

    .tree-card-person .sp-result-photo,
    .tree-card-person .sp-result-portrait {
        width: 24px;
        height: 24px;
    }

    .tree-card-person .sp-result-info {
        display: flex;
        align-items: baseline;
        gap: var(--space-4);
    }

    .tree-card-person .sp-result-name {
        flex: 1;
        min-width: 0;
    }

    .tree-card-person .sp-result-dates {
        flex-shrink: 0;
        margin-top: 0;
    }

    .tree-card-person:hover .sp-surname,
    .tree-card-person:hover .sp-given {
        color: var(--orange);
    }

    .tree-card-footer {
        display: flex;
        align-items: center;
        justify-content: space-between;
        border-top: 1px solid var(--border);
        padding-top: var(--space-8);
        margin-top: var(--space-6);
    }

    .tree-card-footer-left {
        display: flex;
        align-items: center;
        gap: var(--space-4);
    }

    .tree-last-update {
        font-size: var(--text-75);
        color: var(--text-muted);
    }

    .tree-badge-recent {
        font-size: var(--text-65);
        font-weight: 600;
        color: var(--green-light);
        background: color-mix(in srgb, var(--green-accent) 12%, transparent);
        border: 1px solid color-mix(in srgb, var(--green-accent) 30%, transparent);
        border-radius: var(--radius);
        padding: var(--space-1) var(--space-4);
        text-transform: uppercase;
        letter-spacing: 0.04em;
    }

    .btn-open {
        background: linear-gradient(135deg, var(--orange), var(--orange-light));
        border: none;
        border-radius: var(--radius);
        padding: var(--space-3) var(--space-8);
        color: var(--white);
        font-family: var(--font-heading);
        font-size: var(--text-70);
        font-weight: 600;
        letter-spacing: 0.05em;
        cursor: pointer;
        transition: opacity 0.2s;
        text-decoration: none;
        display: inline-block;
    }

    .btn-open:hover {
        opacity: 0.85;
    }

    /* ── Add-new card ─────────────────────────────────────────────── */

    .tree-card-add {
        border-style: dashed;
        border-color: var(--border);
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        min-height: 280px;
        gap: var(--space-8);
        background: transparent;
    }

    .tree-card-add:hover {
        border-color: var(--green);
        background: color-mix(in srgb, var(--green-accent) 4%, transparent);
        box-shadow: 0 0 30px color-mix(in srgb, var(--green-accent) 8%, transparent);
        transform: translateY(-2px);
    }

    .tree-card-add-icon {
        width: 56px;
        height: 56px;
        border-radius: 50%;
        background: color-mix(in srgb, var(--green-accent) 10%, transparent);
        border: 1px solid var(--green);
        display: flex;
        align-items: center;
        justify-content: center;
        font-size: 1.6rem;
        color: var(--green-light);
        transition: background 0.2s;
    }

    .tree-card-add:hover .tree-card-add-icon {
        background: color-mix(in srgb, var(--green-accent) 20%, transparent);
    }

    .tree-card-add-text {
        font-family: var(--font-heading);
        font-size: var(--text-90);
        color: var(--green-light);
        letter-spacing: 0.04em;
    }

    .tree-card-add-sub {
        font-size: var(--text-80);
        color: var(--text-muted);
        text-align: center;
        padding: 0 var(--space-12);
    }

    /* ── Empty state ─────────────────────────────────────────────── */

    /* The shared EmptyState, roomier and in the page's heading face. */
    .home-empty {
        padding: var(--space-40) var(--space-16);
        gap: var(--space-8);
    }

    .home-empty h3 {
        font-family: var(--font-heading);
        font-size: var(--text-120);
    }

    .home-empty p {
        font-size: var(--text-90);
    }

    /* ── Create tree modal ───────────────────────────────────────── */

    .home-create-modal {
        background: var(--bg-panel);
        border: 1px solid var(--border);
        border-radius: var(--radius-lg);
        min-width: 360px;
        max-width: 480px;
        width: 95vw;
        box-shadow: var(--shadow-lg);
    }

    .home-create-modal-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: var(--space-10) var(--space-12);
        border-bottom: 1px solid var(--border);
    }

    .home-create-modal-header h2 {
        font-family: var(--font-heading);
        font-size: var(--text-110);
        font-weight: 600;
        color: var(--text-primary);
        margin: 0;
    }

    .home-create-modal-body {
        padding: var(--space-12);
    }

    /* ── Animations ──────────────────────────────────────────────── */

    /* Users of this animation must set `fill-mode: backwards`, never `both`.
       `backwards` covers the start delay and then stops applying once the
       animation ends; `both` keeps it in effect forever, so the animated
       `transform` lingers and turns the element into a containing block for
       its `position: fixed` descendants — which silently shrinks full-screen
       overlays (dropdown backdrops, modals) down to that element's box. */
    @keyframes home-fade-up {
        from { opacity: 0; transform: translateY(20px); }
        to   { opacity: 1; transform: translateY(0); }
    }

    /* ── Import banner ────────────────────────────────────────────── */

    .home-import-banner {
        position: fixed;
        bottom: 1.5rem;
        left: 50%;
        transform: translateX(-50%);
        z-index: 1000;
        padding: var(--space-6) var(--space-12);
        border-radius: var(--radius);
        font-size: var(--text-85);
        max-width: 600px;
        box-shadow: var(--shadow-md);
    }

    /* ── Responsive ──────────────────────────────────────────────── */

    @media (max-width: 640px) {
        .home-main { padding: var(--space-16) var(--space-8) var(--space-32); }
        .home-search-box { flex: 1 0 100%; }
        .home-sort-select {
            flex: 1 1 0;
            height: 38px;
        }
        .home-btn-new-toolbar {
            width: 38px;
            height: 38px;
            flex: 0 0 38px;
            justify-content: center;
            padding: 0;
        }
        .home-btn-new-toolbar svg {
            width: 18px;
            height: 18px;
        }
        .home-btn-new-toolbar .home-btn-new-label { display: none; }
        .trees-grid { grid-template-columns: minmax(0, 1fr); }
        .home-page-header h1 { font-size: var(--text-150); }
    }
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(name: &str, hours_ago: i64) -> TreeSummary {
        TreeSummary {
            id: Uuid::now_v7(),
            name: name.to_string(),
            description: String::new(),
            updated_at: Utc::now() - chrono::Duration::hours(hours_ago),
            importing: false,
        }
    }

    fn names(trees: Vec<TreeSummary>) -> Vec<String> {
        trees.into_iter().map(|tree| tree.name).collect()
    }

    /// The three orders of the sort menu, the name ones ignoring case.
    #[test]
    fn trees_sort_by_recency_or_by_name_either_way() {
        let trees = [tree("beta", 1), tree("Alpha", 3), tree("gamma", 2)];
        assert_eq!(
            names(visible_trees(&trees, "", "recent")),
            ["beta", "gamma", "Alpha"]
        );
        assert_eq!(
            names(visible_trees(&trees, "", "name")),
            ["Alpha", "beta", "gamma"]
        );
        assert_eq!(
            names(visible_trees(&trees, "", "name_desc")),
            ["gamma", "beta", "Alpha"]
        );
        assert_eq!(names(visible_trees(&trees, "AL", "name_desc")), ["Alpha"]);
    }
}
