//! Tree detail page — Pedigree chart view.
//!
//! Shows the tree breadcrumb, search fields, the [`PedigreeChart`] as the
//! main view, a context menu for person actions (including search-or-create
//! flows for AddSpouse/AddParents/AddChild), and union editing.

use dioxus::prelude::*;
use oxidgene_core::projection::Pedigree;
use oxidgene_core::types::Tree;
use oxidgene_core::{ChildType, SpouseRole};
use uuid::Uuid;

use crate::api::{AddChildBody, AddSpouseBody, ApiClient, ApiError, CreatePersonBody};
use crate::components::breadcrumb::TreeBreadcrumb;
use crate::components::confirm_dialog::ConfirmDialog;
use crate::components::context_menu::{ContextMenu, PersonAction};
use crate::components::merge_dialog::MergeDialog;
use crate::components::pedigree_chart::{PedigreeChart, PedigreeData, Portraits, SharedPedigree};
use crate::components::person_form::{PersonForm, PersonFormCreateContext};
use crate::components::print::PrintHeading;
use crate::components::search_person::SearchPerson;
use crate::components::topbar_search::TopbarSearch;
use crate::components::tree_cache::{
    TreeCache, fetch_tree_cached, use_tree_cache, use_view_state_cache,
};
use crate::components::union_form::UnionForm;
use crate::i18n::{I18n, use_i18n};
use crate::nav_history::use_history_subject;
use crate::prefs::PedigreeDefaults;
use crate::router::{
    Route, couple_route, pedigree_route, person_route, push_tree_route, replace_tree_route,
};
use crate::shared::Shared;
use crate::ui_observability::{UiLoadTrace, UiPage, use_traced_resource, use_ui_load_trace};
use crate::utils::resolve_name;

/// Describes which linking flow is active.
#[derive(Debug, Clone, PartialEq)]
enum LinkingMode {
    /// Adding a spouse for the given person.
    Spouse(Uuid),
    /// Adding parents for the given person (child_id).
    Parents(Uuid),
    /// Adding a child for the given person (parent_id).
    Child(Uuid),
    /// Adding a sibling for the given person.
    Sibling(Uuid),
    /// Choosing whom to trace the given person's relationship to.
    Kinship(Uuid),
}

/// How a person belongs to a family, with their place in its order.
#[derive(Debug, Clone, Copy)]
enum Membership {
    Spouse(i32),
    Child(i32),
}

impl Membership {
    /// Adds `person_id` to the family `fid`.
    async fn join(self, api: &ApiClient, tid: Uuid, fid: Uuid, person_id: Uuid) {
        let _ = match self {
            Membership::Spouse(sort_order) => {
                let body = AddSpouseBody {
                    person_id,
                    role: SpouseRole::Partner,
                    sort_order,
                };
                api.add_spouse(tid, fid, &body).await.map(drop)
            }
            Membership::Child(sort_order) => {
                let body = AddChildBody {
                    person_id,
                    child_type: ChildType::Biological,
                    sort_order,
                };
                api.add_child(tid, fid, &body).await.map(drop)
            }
        };
    }
}

/// A relative being added to a person: the family they join, and how each
/// of the two belongs to it.
#[derive(Debug, Clone, Copy)]
struct Relation {
    /// The person the relative is added to.
    anchor: Uuid,
    /// How the person belongs to the family — the one they are already in,
    /// or a new one.
    anchor_joins: Membership,
    /// How the relative joins it.
    relative_joins: Membership,
}

impl LinkingMode {
    /// The relative this flow adds, or `None` for the kinship pick.
    fn relation(&self) -> Option<Relation> {
        let (anchor, anchor_joins, relative_joins) = match *self {
            LinkingMode::Spouse(pid) => (pid, Membership::Spouse(0), Membership::Spouse(1)),
            LinkingMode::Parents(pid) => (pid, Membership::Child(0), Membership::Spouse(0)),
            LinkingMode::Child(pid) => (pid, Membership::Spouse(0), Membership::Child(0)),
            LinkingMode::Sibling(pid) => (pid, Membership::Child(0), Membership::Child(1)),
            LinkingMode::Kinship(_) => return None,
        };
        Some(Relation {
            anchor,
            anchor_joins,
            relative_joins,
        })
    }

    /// The panel's title, search placeholder and create button, by i18n key.
    fn label_keys(&self) -> (&'static str, &'static str, &'static str) {
        match self {
            LinkingMode::Spouse(_) => (
                "linking.add_spouse",
                "linking.search_spouse",
                "linking.create_spouse",
            ),
            LinkingMode::Parents(_) => (
                "linking.add_parent",
                "linking.search_parent",
                "linking.create_parent",
            ),
            LinkingMode::Child(_) => (
                "linking.add_child",
                "linking.search_child",
                "linking.create_child",
            ),
            LinkingMode::Sibling(_) => (
                "linking.add_sibling",
                "linking.search_sibling",
                "linking.create_sibling",
            ),
            LinkingMode::Kinship(_) => ("context.kinship", "kinship.choose", ""),
        }
    }
}

impl Relation {
    /// The family the anchor already belongs to in the right way, if any.
    fn existing_family(&self, data: Option<&SharedPedigree>) -> Option<Uuid> {
        let families = match self.anchor_joins {
            Membership::Spouse(_) => &data?.families_as_spouse,
            Membership::Child(_) => &data?.families_as_child,
        };
        families.get(&self.anchor)?.first().copied()
    }

    /// Adds the relative — `relative`, or a new person — to the anchor's
    /// family, founding it first when there is none. `None` when the family
    /// could not be created.
    async fn add(
        self,
        api: &ApiClient,
        tid: Uuid,
        family: Option<Uuid>,
        relative: Option<Uuid>,
    ) -> Option<()> {
        let fid = match family {
            Some(fid) => fid,
            None => {
                let family = api.create_family(tid).await.ok()?;
                self.anchor_joins
                    .join(api, tid, family.id, self.anchor)
                    .await;
                family.id
            }
        };
        let relative = match relative {
            Some(relative) => Some(relative),
            None => new_person(api, tid).await,
        };
        if let Some(relative) = relative {
            self.relative_joins.join(api, tid, fid, relative).await;
        }
        Some(())
    }
}

/// Creates a blank person, handing back their id.
async fn new_person(api: &ApiClient, tid: Uuid) -> Option<Uuid> {
    api.create_person(
        tid,
        &CreatePersonBody {
            sex: oxidgene_core::Sex::Unknown,
        },
    )
    .await
    .ok()
    .map(|person| person.id)
}

/// The dialogs and panels the page opens over the chart.
#[derive(Clone, Copy)]
struct Overlays {
    context_menu: Signal<Option<(Uuid, f64, f64)>>,
    editing_person: Signal<Option<Uuid>>,
    creating_person: Signal<Option<PersonFormCreateContext>>,
    editing_union: Signal<Option<Uuid>>,
    linking: Signal<Option<LinkingMode>>,
    /// The person "Merge with…" was chosen on, while the wizard is open.
    merging: Signal<Option<Uuid>>,
    confirm_delete: Signal<Option<Uuid>>,
    delete_error: Signal<Option<String>>,
}

fn use_overlays() -> Overlays {
    Overlays {
        context_menu: use_signal(|| None),
        editing_person: use_signal(|| None),
        creating_person: use_signal(|| None),
        editing_union: use_signal(|| None),
        linking: use_signal(|| None),
        merging: use_signal(|| None),
        confirm_delete: use_signal(|| None),
        delete_error: use_signal(|| None),
    }
}

impl Overlays {
    /// Runs a context menu action on the person the menu was opened on.
    fn act(
        mut self,
        action: PersonAction,
        data: Option<&SharedPedigree>,
        mut selected_root: Signal<Option<Uuid>>,
    ) {
        let Some((pid, _, _)) = (self.context_menu)() else {
            return;
        };
        self.context_menu.set(None);
        match action {
            PersonAction::Edit => self.editing_person.set(Some(pid)),
            PersonAction::Merge => self.merging.set(Some(pid)),
            PersonAction::AddParents => self.linking.set(Some(LinkingMode::Parents(pid))),
            PersonAction::AddSpouse => self.linking.set(Some(LinkingMode::Spouse(pid))),
            PersonAction::AddChild => self.linking.set(Some(LinkingMode::Child(pid))),
            PersonAction::AddSibling => self.linking.set(Some(LinkingMode::Sibling(pid))),
            PersonAction::EditUnion => {
                let family_id = data
                    .and_then(|data| data.families_as_spouse.get(&pid))
                    .and_then(|fids| fids.first().copied());
                if family_id.is_some() {
                    self.editing_union.set(family_id);
                }
            }
            PersonAction::EditSpecificUnion(fid) => self.editing_union.set(Some(fid)),
            PersonAction::Kinship => self.linking.set(Some(LinkingMode::Kinship(pid))),
            PersonAction::GoTo(relative) => selected_root.set(Some(relative)),
            PersonAction::Delete => {
                self.confirm_delete.set(Some(pid));
                self.delete_error.set(None);
            }
        }
    }
}

/// What the page's writes need: the API, the tree, and the cache to
/// invalidate once they land.
#[derive(Clone)]
struct TreeWrites {
    api: ApiClient,
    tree_id: Signal<Option<Uuid>>,
    tree_cache: TreeCache,
    overlays: Overlays,
    selected_root: Signal<Option<Uuid>>,
}

impl TreeWrites {
    /// Adds a relative in the open linking flow: `relative`, or a new person.
    fn link(&self, relative: Option<Uuid>, data: Option<&SharedPedigree>) {
        let Some(tid) = (self.tree_id)() else { return };
        let Some(relation) = (self.overlays.linking)().and_then(|mode| mode.relation()) else {
            return;
        };
        let family = relation.existing_family(data);
        let (api, tree_cache, mut linking) =
            (self.api.clone(), self.tree_cache, self.overlays.linking);
        spawn(async move {
            if relation.add(&api, tid, family, relative).await.is_some() {
                linking.set(None);
                tree_cache.invalidate();
            }
        });
    }

    /// Deletes the person whose deletion was confirmed.
    fn delete_confirmed(&self) {
        let Some(tid) = (self.tree_id)() else { return };
        let Some(pid) = (self.overlays.confirm_delete)() else {
            return;
        };
        let Self {
            api,
            tree_cache,
            overlays,
            mut selected_root,
            ..
        } = self.clone();
        let Overlays {
            mut confirm_delete,
            mut delete_error,
            ..
        } = overlays;
        spawn(async move {
            match api.delete_person(tid, pid).await {
                Ok(_) => {
                    confirm_delete.set(None);
                    delete_error.set(None);
                    if selected_root() == Some(pid) {
                        selected_root.set(None);
                    }
                    tree_cache.invalidate();
                }
                Err(e) => delete_error.set(Some(format!("{e}"))),
            }
        });
    }

    /// Creates the tree's first person and opens them for editing.
    fn add_first_person(&self) {
        let Some(tid) = (self.tree_id)() else { return };
        let (api, tree_cache, mut editing) = (
            self.api.clone(),
            self.tree_cache,
            self.overlays.editing_person,
        );
        spawn(async move {
            if let Some(person) = new_person(&api, tid).await {
                editing.set(Some(person));
                tree_cache.invalidate();
            }
        });
    }
}

/// Which person the chart is drawn around, kept in step with the route.
#[derive(Clone, Copy)]
struct RootSelection {
    tree_id: Signal<Option<Uuid>>,
    selected_root: Signal<Option<Uuid>>,
    /// Incremented every time the route names a person, so the chart
    /// re-centres even when the root has not changed.
    center_gen: Signal<u32>,
    /// Whether this render moved to another tree.
    tree_changed: bool,
}

fn use_root_selection(tree_id: &str, person: Option<&String>) -> RootSelection {
    let view_cache = use_view_state_cache();

    // Reactive tree_id: a signal always in sync with the prop so resources re-run.
    let mut tree_id_parsed = use_signal(|| tree_id.parse::<Uuid>().ok());
    // Synchronously overwrite — write() updates in place for the current render.
    let new_parsed = tree_id.parse::<Uuid>().ok();
    let tree_changed = new_parsed != *tree_id_parsed.peek();
    if tree_changed {
        *tree_id_parsed.write() = new_parsed;
    }

    // Root person — from query param, view-state cache, or first person.
    let saved_root = tree_id_parsed()
        .and_then(|tid| view_cache.get_untracked(tid))
        .and_then(|vs| vs.selected_root);
    let named = person.and_then(|p| p.parse::<Uuid>().ok());
    let initial_person = named.or(saved_root);
    let mut selected_root = use_signal(move || initial_person);

    // Start at 1 when the route names a person on mount, so centering
    // triggers even though prev_person_raw is initialized to the same value
    // — unless it names the root the saved view was framed on, as going
    // back to the pedigree does: that view is reopened as it was left.
    let recenter_on_mount = named.is_some() && named != saved_root;
    let mut center_gen = use_signal(move || u32::from(recenter_on_mount));

    // Reset state when navigating to a different tree (component is reused by the router).
    let mut prev_tree_id = use_signal(|| tree_id.to_string());
    if tree_id != *prev_tree_id.peek() {
        *prev_tree_id.write() = tree_id.to_string();
        selected_root.set(None);
        center_gen += 1;
    }

    // Sync selected_root when navigating with a (possibly identical) person query param.
    // We compare the raw string to detect re-navigation to the same person.
    let person_raw = person.cloned();
    let mut prev_person_raw = use_signal(|| person_raw.clone());
    // The route as the page last rewrote it itself (see `use_root_in_route`).
    let mut rewritten = use_signal(|| None::<Option<String>>);
    if person_raw != prev_person_raw() {
        prev_person_raw.set(person_raw.clone());
        let written_here = std::mem::take(&mut *rewritten.write()) == Some(person_raw);
        if !written_here {
            if initial_person.is_some() {
                selected_root.set(initial_person);
            }
            center_gen += 1;
        }
    }
    use_root_in_route(tree_id_parsed, selected_root, prev_person_raw, rewritten);

    RootSelection {
        tree_id: tree_id_parsed,
        selected_root,
        center_gen,
        tree_changed,
    }
}

/// Keeps the route naming the person the chart is drawn around, whichever
/// way they became its root — a click on a card, a relative gone to, a
/// merge — so that the history entry reopens the chart on them. The route
/// is replaced rather than pushed: moving about the chart is not a new
/// page. The page notes the route it wrote, so that reading it back does
/// not re-centre a chart the click has already moved.
fn use_root_in_route(
    tree_id: Signal<Option<Uuid>>,
    selected_root: Signal<Option<Uuid>>,
    route_person: Signal<Option<String>>,
    mut rewritten: Signal<Option<Option<String>>>,
) {
    let nav = navigator();
    use_effect(move || {
        let named = route_person();
        let Some(tid) = tree_id() else {
            return;
        };
        // Without a root of its own the chart is on the tree's default one,
        // and the route names nobody — not a person deleted since.
        let root = selected_root();
        let root_raw = root.map(|root| root.to_string());
        if named == root_raw {
            return;
        }
        rewritten.set(Some(root_raw));
        nav.replace(pedigree_route(tid.to_string(), root));
    });
}

/// The pedigree around the selected person, else around the tree's default
/// root — its SOSA root, else its first person — which the server chooses,
/// so the chart waits on nothing else; `levels` generations up and down. A
/// tree without anyone answers `404`, and the page offers its first person.
async fn load_pedigree(
    api: &ApiClient,
    tid: Uuid,
    selected: Option<Uuid>,
    (ancestor_levels, descendant_levels): (usize, usize),
) -> Result<Pedigree, ApiError> {
    let (up, down) = (ancestor_levels as u32, descendant_levels as u32);
    match selected {
        Some(root_id) => api.get_pedigree(tid, root_id, up, down).await,
        None => api.get_default_pedigree(tid, up, down).await,
    }
}

/// Page rendered at `/trees/:tree_id?person=...`.
#[component]
pub fn TreeDetail(tree_id: String, person: Option<String>) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let load_trace = use_ui_load_trace(UiPage::Pedigree);

    // ── Global caches ──
    let tree_cache = use_tree_cache();
    let view_cache = use_view_state_cache();
    let pedigree_defaults = use_context::<Signal<Option<PedigreeDefaults>>>();

    let RootSelection {
        tree_id: tree_id_parsed,
        mut selected_root,
        center_gen,
        tree_changed,
    } = use_root_selection(&tree_id, person.as_ref());
    let overlays = use_overlays();
    let Overlays {
        mut context_menu,
        mut creating_person,
        mut linking,
        ..
    } = overlays;

    // ── Fetch tree details (cache-backed) ──
    let api_tree = api.clone();
    let mut tree_resource = use_traced_resource(load_trace.clone(), "tree", move || {
        let api = api_tree.clone();
        let _gen = tree_cache.generation();
        let tid = tree_id_parsed();
        async move {
            let Some(tid) = tid else {
                return Err(ApiError::invalid_tree_id(&i18n));
            };
            fetch_tree_cached(&api, &tree_cache, tid).await
        }
    });
    let tree = match &*tree_resource.read() {
        Some(Ok(tree)) => Some(tree.clone()),
        _ => None,
    };

    // ── Fetch pedigree from the API ──
    let api_pedigree = api.clone();
    let mut pedigree_resource = use_traced_resource(load_trace.clone(), "pedigree", move || {
        let api = api_pedigree.clone();
        let _gen = tree_cache.generation();
        let tid = tree_id_parsed();
        let sel_root = selected_root();
        let _depth_gen = view_cache.depth_generation();
        let vs = tid.and_then(|t| view_cache.get_untracked(t));
        let defaults = pedigree_defaults();
        async move {
            let Some(defaults) = defaults else {
                return std::future::pending().await;
            };
            let Some(tid) = tid else {
                return Err(ApiError::invalid_tree_id(&i18n));
            };
            let levels = vs.as_ref().map_or(
                (defaults.ancestor_levels, defaults.descendant_levels),
                |view| (view.ancestor_levels, view.descendant_levels),
            );
            load_pedigree(&api, tid, sel_root, levels).await
        }
    });

    // ── The portraits of the chart's people, whose sources the pedigree
    // carries — only for a view that draws them ──
    let view_pref = try_use_context::<Signal<crate::components::pedigree_view::PedigreeView>>();
    let api_photos = api.clone();
    let photos_resource = use_traced_resource(load_trace.clone(), "portraits", move || {
        let api = api_photos.clone();
        let tid = tree_id_parsed();
        let draws_portraits = view_pref.is_none_or(|view| view.read().draws_portraits());
        let refs = match &*pedigree_resource.read() {
            Some(Ok(pedigree)) if draws_portraits => pedigree
                .persons
                .values()
                .filter_map(|node| Some((node.person_id, node.portrait.clone()?)))
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        };
        async move {
            let Some(tid) = tid.filter(|_| !refs.is_empty()) else {
                return Portraits::default();
            };
            Shared::new(api.portraits_from_refs(tid, &refs).await)
        }
    });
    // The SOSA root's ancestors in the window, as the server marked them.
    let sosa_ancestors = use_memo(move || match &*pedigree_resource.read() {
        Some(Ok(pedigree)) => {
            let marked: std::collections::HashSet<Uuid> = pedigree
                .persons
                .values()
                .filter(|node| node.sosa_ancestor)
                .map(|node| node.person_id)
                .collect();
            (!marked.is_empty()).then(|| Shared::new(marked))
        }
        _ => None,
    });

    // Force resources to re-fetch when tree_id changes (component reused by router).
    if tree_changed {
        tree_resource.restart();
        pedigree_resource.restart();
    }

    let pedigree_view =
        use_pedigree_view(load_trace, pedigree_resource, tree_resource, selected_root);
    let (pedigree_data, root_person_id) = pedigree_view();

    let writes = TreeWrites {
        api: api.clone(),
        tree_id: tree_id_parsed,
        tree_cache,
        overlays,
        selected_root,
    };
    let view = crate::prefs::use_pedigree_view();
    let tree_name_str = tree_cache
        .loaded_or_cached(tree_id_parsed(), tree.as_ref())
        .map(|t| t.name)
        .unwrap_or_default();
    // The history names the chart after the person it is drawn around.
    let root_name = match (pedigree_data.as_ref(), root_person_id) {
        (Some(data), Some(root)) => Some(resolve_name(root, &data.names, &i18n)),
        _ => Some(tree_name_str.clone()).filter(|name| !name.is_empty()),
    };
    use_history_subject(root_name);

    // ── Render ──

    rsx! {
        div { class: "tree-detail-page",

        // ── Topbar: breadcrumb + search ──
        div { class: "td-topbar",
            TreeBreadcrumb {
                tree_name: tree_name_str.clone(),
                linked: false,
                span { class: "td-bc-current", {i18n.t("pedigree.breadcrumb")} }
            }
            if root_person_id.is_some() {
                TopbarSearch { tree_id: tree_id.clone() }
            }
            PrintHeading {
                tree_name: tree_name_str.clone(),
                title: i18n.t("pedigree.breadcrumb"),
            }
        }

        {overlay_dialogs(&i18n, &writes, &tree_id)}

        // Context menu
        if let (Some((pid, x, y)), Some(data)) = (context_menu(), pedigree_data.as_ref()) {
            ContextMenu {
                person_name: resolve_name(pid, &data.names, &i18n),
                x,
                y,
                has_union: data.families_as_spouse.get(&pid).is_some_and(|fids| !fids.is_empty()),
                unions: data.unions_for_person(pid, &i18n),
                // Relatives the chart does not draw around the person, to go
                // to from the action picker: spouses and children in an
                // ancestor chart, parents and spouses in a descendant one.
                go_to: go_to_relatives(data, pid, root_person_id, view, &i18n),
                on_action: {
                    let data = pedigree_data.clone();
                    move |action| overlays.act(action, data.as_ref(), selected_root)
                },
                on_close: move |_| context_menu.set(None),
            }
        }

        // ── Pedigree chart (fills remaining space) ──
        div { class: "pedigree-card",
            match (pedigree_data.clone(), root_person_id) {
                (Some(data), Some(root_id)) => rsx! {
                    PedigreeChart {
                        root_person_id: root_id,
                        data: data.clone(),
                        tree_id: tree_id.clone(),
                        sosa_root_person_id: tree.as_ref().and_then(|tree| tree.sosa_root_person_id),
                        sosa_ancestor_ids: sosa_ancestors(),
                        portraits: photos_resource.read().clone(),
                        center_gen: center_gen(),
                        on_person_click: move |(pid, x, y)| {
                            context_menu.set(Some((pid, x, y)));
                        },
                        on_person_navigate: move |pid| {
                            selected_root.set(Some(pid));
                        },
                        on_empty_slot: move |(child_id, is_father)| {
                            creating_person.set(Some(add_parent_context(&data, child_id, is_father)));
                        },
                        on_add_spouse_slot: move |person_id| {
                            linking.set(Some(LinkingMode::Spouse(person_id)));
                        },
                        on_add_person: move |_| {
                            creating_person.set(Some(PersonFormCreateContext::Standalone));
                        },
                        on_profile_view: push_tree_route(&tree_id, person_route),
                        on_couple_view: push_tree_route(&tree_id, couple_route),
                    }
                },
                // Show the empty-tree UI once the pedigree has loaded without
                // one: an error means no persons, or a network failure.
                _ if pedigree_resource.read().is_some() => empty_tree(&i18n, writes.clone()),
                _ => rsx! {
                    div { class: "loading", {i18n.t("tree.loading_pedigree")} }
                },
            }
        }

        // ── Linking panel (search-or-create for AddSpouse/AddParents/AddChild) ──
        if let (Some(mode), Some(tid)) = (linking(), tree_id_parsed()) {
            {linking_panel(&i18n, tid, &mode, &writes, pedigree_data.clone(), tree.as_ref(), &tree_id)}
        }

        } // close .tree-detail-page
    }
}

/// The dialogs opened over the chart: merging, deleting, and the person and
/// union forms.
fn overlay_dialogs(i18n: &I18n, writes: &TreeWrites, tree_id: &str) -> Element {
    let Some(tid) = (writes.tree_id)() else {
        return rsx! {};
    };
    let tree_cache = writes.tree_cache;
    let mut selected_root = writes.selected_root;
    let Overlays {
        mut editing_person,
        mut creating_person,
        mut editing_union,
        mut merging,
        mut confirm_delete,
        mut delete_error,
        ..
    } = writes.overlays;
    let delete = writes.clone();
    rsx! {
        // "Merge with…": the wizard, from the search for the other record.
        if let Some(pid) = merging() {
            MergeDialog {
                tree_id: tid,
                person_id: pid,
                on_close: move |_| merging.set(None),
                on_merged: move |kept: Uuid| {
                    merging.set(None);
                    if selected_root() == Some(pid) && kept != pid {
                        selected_root.set(Some(kept));
                    }
                    tree_cache.invalidate();
                },
            }
        }

        // Delete person confirmation
        if confirm_delete().is_some() {
            ConfirmDialog {
                title: i18n.t("confirm.delete_person.title"),
                message: i18n.t("confirm.delete_person.message"),
                confirm_label: i18n.t("common.delete"),
                confirm_class: "btn btn-danger",
                error: delete_error(),
                on_confirm: move |_| delete.delete_confirmed(),
                on_cancel: move |_| {
                    confirm_delete.set(None);
                    delete_error.set(None);
                },
            }
        }

        // Person edit modal
        if let Some(edit_pid) = editing_person() {
            PersonForm {
                tree_id: tid,
                person_id: Some(edit_pid),
                on_close: move |_| editing_person.set(None),
                on_saved: move |_| tree_cache.invalidate(),
                // The edited person may be the chart's root, and no longer
                // exists: centre the chart on the one they were merged into.
                on_merged: replace_tree_route(tree_id, |tree_id, kept: Uuid| {
                    pedigree_route(tree_id, Some(kept))
                }),
            }
        }

        // Person create modal
        if let Some(ctx) = creating_person() {
            PersonForm {
                tree_id: tid,
                create_context: ctx,
                on_close: move |_| creating_person.set(None),
                on_saved: move |_| tree_cache.invalidate(),
            }
        }

        // Union edit modal
        if let Some(union_fid) = editing_union() {
            UnionForm {
                tree_id: tid,
                family_id: union_fid,
                on_close: move |_| editing_union.set(None),
                on_saved: move |_| tree_cache.invalidate(),
            }
        }
    }
}

/// The chart's data, assembled from the pedigree and the tree, and the root
/// it is drawn around. The portraits stay out of it (see [`Portraits`]).
fn use_pedigree_view(
    load_trace: UiLoadTrace,
    pedigree_resource: Resource<Result<Pedigree, ApiError>>,
    tree_resource: Resource<Result<Tree, ApiError>>,
    selected_root: Signal<Option<Uuid>>,
) -> Memo<(Option<SharedPedigree>, Option<Uuid>)> {
    // ── Build pedigree data from the fetched pedigree ──
    //
    // Assembled once per change and shared from there. Every person, name,
    // event and place the pedigree pulled in lives in here, and a
    // dozen handlers below read it; rebuilt inline it was rebuilt — and deep
    // copied once per handler — on every render, including the render that
    // merely opened a context menu.
    use_memo(move || {
        load_trace.measure("pedigree_data", || {
            let ped_data = pedigree_resource.read();
            let Some(Ok(pedigree)) = &*ped_data else {
                return (None, selected_root());
            };
            let mut pd = PedigreeData::from_pedigree(pedigree);
            pd.self_person_id = match &*tree_resource.read() {
                Some(Ok(tree)) => tree.self_person_id,
                _ => None,
            };
            (Some(SharedPedigree::new(pd)), Some(pedigree.root_person_id))
        })
    })
}

/// Creating a parent from an empty slot of the chart: the child's family,
/// if they have one, and their surname to start from.
fn add_parent_context(
    data: &PedigreeData,
    child_id: Uuid,
    is_father: bool,
) -> PersonFormCreateContext {
    let family_id = data
        .families_as_child
        .get(&child_id)
        .and_then(|fids| fids.first().copied());
    let child_surname = data
        .names
        .get(&child_id)
        .and_then(oxidgene_core::types::PersonName::primary)
        .and_then(|name| name.full_surname())
        .filter(|surname| !surname.trim().is_empty());
    PersonFormCreateContext::AddParent {
        child_id,
        family_id,
        is_father,
        child_surname,
    }
}

/// A tree with no one yet: the button creating its first person.
fn empty_tree(i18n: &I18n, writes: TreeWrites) -> Element {
    rsx! {
        div { class: "empty-tree-container",
            button {
                class: "empty-tree-slot",
                title: "{i18n.t(\"tree.no_persons_hint\")}",
                onclick: move |_| writes.add_first_person(),
                svg {
                    width: "32",
                    height: "32",
                    fill: "none",
                    "viewBox": "0 0 24 24",
                    stroke: "currentColor",
                    "strokeWidth": "1.5",
                    line { x1: "12", y1: "5", x2: "12", y2: "19" }
                    line { x1: "5", y1: "12", x2: "19", y2: "12" }
                }
                span { {i18n.t("tree.add_first_person")} }
            }
        }
    }
}

/// The persons most often asked about, offered before any search: the user
/// themself and the tree's SOSA root — neither being `from`, and each once.
fn kinship_shortcuts(from: Uuid, tree: Option<&Tree>, i18n: &I18n) -> Vec<(Uuid, String)> {
    let Some(tree) = tree else {
        return Vec::new();
    };
    let mut picks: Vec<(Uuid, String)> = Vec::new();
    for (id, key) in [
        (tree.self_person_id, "kinship.pick_self"),
        (tree.sosa_root_person_id, "kinship.pick_sosa_root"),
    ] {
        if let Some(id) = id.filter(|&id| id != from && !picks.iter().any(|&(seen, _)| seen == id))
        {
            picks.push((id, i18n.t(key)));
        }
    }
    picks
}

/// The search-or-create panel of a linking flow, or the kinship pick.
fn linking_panel(
    i18n: &I18n,
    tid: Uuid,
    mode: &LinkingMode,
    writes: &TreeWrites,
    data: Option<SharedPedigree>,
    tree: Option<&Tree>,
    tree_id: &str,
) -> Element {
    let mut linking = writes.overlays.linking;
    let (title_key, search_key, create_key) = mode.label_keys();
    // A tree that does not suggest its persons only offers a new one.
    let suggest = writes.tree_cache.suggest_persons();
    let body = match *mode {
        LinkingMode::Kinship(from) => {
            let (nav, tree_id) = (dioxus::router::navigator(), tree_id.to_string());
            let pick = EventHandler::new(move |other: Uuid| {
                linking.set(None);
                if other != from {
                    nav.push(Route::Kinship {
                        tree_id: tree_id.clone(),
                        from: from.to_string(),
                        to: other.to_string(),
                    });
                }
            });
            let shortcuts = kinship_shortcuts(from, tree, i18n);
            rsx! {
                if !shortcuts.is_empty() {
                    div { class: "linking-shortcuts",
                        for (id, label) in shortcuts {
                            button {
                                class: "btn btn-outline btn-sm",
                                onclick: move |_| pick.call(id),
                                "{label}"
                            }
                        }
                    }
                }
                SearchPerson {
                    tree_id: tid,
                    placeholder: i18n.t(search_key),
                    on_select: pick,
                    on_cancel: move |_| linking.set(None),
                }
            }
        }
        _ => {
            let (select, create) = (writes.clone(), writes.clone());
            let (select_data, create_data) = (data.clone(), data);
            rsx! {
                if suggest {
                    SearchPerson {
                        tree_id: tid,
                        placeholder: i18n.t(search_key),
                        on_select: move |person_id| select.link(Some(person_id), select_data.as_ref()),
                        on_cancel: move |_| linking.set(None),
                    }
                    div { class: "linking-panel-or", {i18n.t("common.or_divider")} }
                }
                button {
                    class: "btn btn-outline",
                    onclick: move |_| create.link(None, create_data.as_ref()),
                    {i18n.t(create_key)}
                }
            }
        }
    };
    rsx! {
        div { class: "card linking-card",
            div { class: "section-header",
                h2 { class: "section-title", {i18n.t(title_key)} }
                button {
                    class: "btn btn-outline btn-sm",
                    onclick: move |_| linking.set(None),
                    {i18n.t("common.cancel")}
                }
            }
            div { class: "linking-panel",
                if suggest || matches!(mode, LinkingMode::Kinship(_)) {
                    p { class: "linking-panel-title",
                        {i18n.t("linking.search_existing")}
                    }
                }
                {body}
            }
        }
    }
}

/// The relatives `view` lists to go to from `pid`'s action picker, grouped
/// under their heading, each named with their lifespan. The chart's focus
/// is left out — it is already there — and so are kinds with nobody.
fn go_to_relatives(
    data: &PedigreeData,
    pid: Uuid,
    focus: Option<Uuid>,
    view: crate::components::pedigree_view::PedigreeView,
    i18n: &crate::i18n::I18n,
) -> Vec<(String, Vec<(Uuid, String)>)> {
    use crate::components::pedigree_chart::format_lifespan;
    use crate::components::pedigree_view::Relatives;

    let label = |id: Uuid| {
        let name = data.display_name(id, i18n);
        let dates = format_lifespan(
            i18n.dates(),
            data.qualified_birth_year(id),
            data.qualified_death_year(id),
        );
        if dates.is_empty() {
            name
        } else {
            format!("{name}  {dates}")
        }
    };
    view.relatives_to_reach()
        .iter()
        .filter_map(|kind| {
            let people = match kind {
                Relatives::Parents => {
                    let (father, mother) = data.parents_of(pid);
                    [father, mother].into_iter().flatten().collect()
                }
                Relatives::Spouses => data.spouses_of(pid),
                Relatives::Children => data.children_of(pid),
            };
            let people: Vec<Uuid> = people.into_iter().filter(|id| Some(*id) != focus).collect();
            (!people.is_empty()).then(|| {
                (
                    i18n.t(kind.heading_key()),
                    people.into_iter().map(|id| (id, label(id))).collect(),
                )
            })
        })
        .collect()
}
