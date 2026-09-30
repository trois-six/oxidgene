//! Tree detail page — Pedigree chart view.
//!
//! Shows the tree breadcrumb, search fields, the [`PedigreeChart`] as the
//! main view, a context menu for person actions (including search-or-create
//! flows for AddSpouse/AddParents/AddChild), and union editing.

use std::collections::HashMap;

use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::ApiClient;
use crate::components::breadcrumb::TreeBreadcrumb;
use crate::components::confirm_dialog::ConfirmDialog;
use crate::components::context_menu::{ContextMenu, PersonAction};
use crate::components::merge_dialog::MergeDialog;
use crate::components::pedigree_chart::{PedigreeChart, PedigreeData, SharedPedigree};
use crate::components::person_form::{PersonForm, PersonFormCreateContext};
use crate::components::print::PrintHeading;
use crate::components::search_person::SearchPerson;
use crate::components::topbar_search::TopbarSearch;
use crate::components::tree_cache::{fetch_tree_cached, use_tree_cache, use_view_state_cache};
use crate::components::union_form::UnionForm;
use crate::i18n::use_i18n;
use crate::prefs::PedigreeDefaults;
use crate::router::Route;
use crate::ui_observability::{UiPage, use_traced_resource, use_ui_load_trace};
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

/// Page rendered at `/trees/:tree_id?person=...`.
#[component]
pub fn TreeDetail(tree_id: String, person: Option<String>) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let nav = use_navigator();
    let load_trace = use_ui_load_trace(UiPage::Pedigree);

    // ── Global caches ──
    let tree_cache = use_tree_cache();
    let view_cache = use_view_state_cache();
    let pedigree_defaults = use_context::<Signal<Option<PedigreeDefaults>>>();

    // Reactive tree_id: a signal always in sync with the prop so resources re-run.
    let mut tree_id_parsed = use_signal(|| tree_id.parse::<Uuid>().ok());
    // Synchronously overwrite — write() updates in place for the current render.
    let new_parsed = tree_id.parse::<Uuid>().ok();
    let tree_changed = new_parsed != *tree_id_parsed.peek();
    if tree_changed {
        *tree_id_parsed.write() = new_parsed;
    }

    // ── Root person — from query param, view-state cache, or first person ──
    let initial_person = person
        .as_deref()
        .and_then(|p| p.parse::<Uuid>().ok())
        .or_else(|| {
            tree_id_parsed()
                .and_then(|tid| view_cache.get_untracked(tid))
                .and_then(|vs| vs.selected_root)
        });
    let mut selected_root = use_signal(move || initial_person);

    // Generation counter: incremented every time we navigate with a ?person param
    // so PedigreeChart re-centers even when the root person hasn't changed.
    // Start at 1 when a person param is present on mount, so centering triggers
    // even though prev_person_raw is initialized to the same value.
    let has_person_param = person.is_some();
    let mut center_gen = use_signal(move || if has_person_param { 1u32 } else { 0u32 });

    // Reset state when navigating to a different tree (component is reused by the router).
    let mut prev_tree_id = use_signal(|| tree_id.clone());
    if tree_id != *prev_tree_id.peek() {
        *prev_tree_id.write() = tree_id.clone();
        selected_root.set(None);
        center_gen += 1;
    }

    // Sync selected_root when navigating with a (possibly identical) person query param.
    // We compare the raw string to detect re-navigation to the same person.
    let person_raw = person.clone();
    let mut prev_person_raw = use_signal(move || person_raw);
    if person != prev_person_raw() {
        prev_person_raw.set(person.clone());
        if let Some(pid) = initial_person {
            selected_root.set(Some(pid));
        }
        center_gen += 1;
    }

    // ── Context menu state ──
    let mut context_menu_person = use_signal(|| None::<(Uuid, f64, f64)>);

    // ── Person edit modal ──
    let mut editing_person_id = use_signal(|| None::<Uuid>);
    let mut creating_person_ctx = use_signal(|| None::<PersonFormCreateContext>);

    // ── Union edit modal ──
    let mut editing_union_id = use_signal(|| None::<Uuid>);

    // ── Linking mode (search-or-create panel) ──
    let mut linking_mode = use_signal(|| None::<LinkingMode>);
    // The person "Merge with…" was chosen on, while the wizard is open.
    let mut merging = use_signal(|| None::<Uuid>);

    // ── Delete person confirmation ──
    let mut confirm_delete_person_id = use_signal(|| None::<Uuid>);
    let mut delete_person_error = use_signal(|| None::<String>);

    // ── Fetch tree details (cache-backed) ──
    let api_tree = api.clone();
    let mut tree_resource = use_traced_resource(load_trace.clone(), "tree", move || {
        let api = api_tree.clone();
        let _gen = tree_cache.generation();
        let tid = tree_id_parsed();
        async move {
            let Some(tid) = tid else {
                return Err(crate::api::ApiError::invalid_tree_id(&i18n));
            };
            fetch_tree_cached(&api, &tree_cache, tid).await
        }
    });

    // Fetch SOSA ancestor IDs from the family graph.
    // This set is used to display the green SOSA badge on ancestor cards,
    // even when jumping to a distant ancestor outside the pedigree window.
    let api_sosa = api.clone();
    let sosa_ancestors_resource =
        use_traced_resource(load_trace.clone(), "sosa_ancestors", move || {
            let api = api_sosa.clone();
            let tid = tree_id_parsed();
            let _gen = tree_cache.generation();
            // Read sosa_root_person_id reactively from tree_resource.
            let sosa_root = match &*tree_resource.read() {
                Some(Ok(tree)) => tree.sosa_root_person_id,
                _ => None,
            };
            async move {
                let (Some(tid), Some(sosa_id)) = (tid, sosa_root) else {
                    return std::collections::HashSet::new();
                };
                match api.get_ancestors(tid, sosa_id, None).await {
                    Ok(entries) => entries
                        .into_iter()
                        .map(|a| a.person_id)
                        .collect::<std::collections::HashSet<Uuid>>(),
                    Err(_) => std::collections::HashSet::new(),
                }
            }
        });

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
                return Err(crate::api::ApiError::invalid_tree_id(&i18n));
            };
            let ancestor_levels = vs
                .as_ref()
                .map(|view| view.ancestor_levels)
                .unwrap_or(defaults.ancestor_levels);
            let descendant_levels = vs
                .as_ref()
                .map(|view| view.descendant_levels)
                .unwrap_or(defaults.descendant_levels);

            // Resolve root person: selected > sosa_root from tree > first person.
            let root_id = if let Some(sel) = sel_root {
                Some(sel)
            } else {
                // Try sosa_root from tree settings — goes through the same
                // TreeCache as `tree_resource` instead of a raw `get_tree`
                // call, avoiding a duplicate `GET /trees/:id` request.
                let tree_root = match fetch_tree_cached(&api, &tree_cache, tid).await {
                    Ok(tree) => tree.sosa_root_person_id,
                    Err(_) => None,
                };
                if tree_root.is_some() {
                    tree_root
                } else {
                    // Fall back to first person in tree.
                    match api.list_persons(tid, Some(1), None).await {
                        Ok(list) => list.edges.first().map(|e| e.node.id),
                        Err(_) => None,
                    }
                }
            };

            let Some(root_id) = root_id else {
                // Empty tree — no persons at all.
                return Err(crate::api::ApiError::Api {
                    status: 404,
                    body: "No persons in tree".to_string(),
                });
            };

            api.get_pedigree(
                tid,
                root_id,
                ancestor_levels as u32,
                descendant_levels as u32,
            )
            .await
        }
    });

    // ── Fetch the portrait map for the tree (person_id → image URL) ──
    let api_photos = api.clone();
    let photos_resource = use_traced_resource(load_trace.clone(), "portraits", move || {
        let api = api_photos.clone();
        let tid = tree_id_parsed();
        let _gen = tree_cache.generation();
        let person_ids = match &*pedigree_resource.read() {
            Some(Ok(pedigree)) => pedigree.persons.keys().copied().collect::<Vec<_>>(),
            _ => Vec::new(),
        };
        async move {
            let Some(tid) = tid.filter(|_| !person_ids.is_empty()) else {
                return std::collections::HashMap::new();
            };
            api.portrait_map_for_ids(tid, &person_ids).await
        }
    });

    // Force resources to re-fetch when tree_id changes (component reused by router).
    if tree_changed {
        tree_resource.restart();
        pedigree_resource.restart();
    }

    // ── Build pedigree data from the fetched pedigree ──
    //
    // Assembled once per change and shared from there. Every person, name,
    // event, place and portrait the pedigree pulled in lives in here, and a
    // dozen handlers below read it; rebuilt inline it was rebuilt — and deep
    // copied once per handler — on every render, including the render that
    // merely opened a context menu.
    let pedigree_view = use_memo(move || {
        load_trace.measure("pedigree_data", || {
            let ped_data = pedigree_resource.read();
            let Some(Ok(pedigree)) = &*ped_data else {
                return (None, selected_root());
            };
            let mut pd = PedigreeData::from_pedigree(pedigree);
            if let Some(photos) = &*photos_resource.read() {
                pd.photos = photos.clone();
            }
            pd.self_person_id = match &*tree_resource.read() {
                Some(Ok(tree)) => tree.self_person_id,
                _ => None,
            };
            (Some(SharedPedigree::new(pd)), Some(pedigree.root_person_id))
        })
    });
    let (pedigree_data, root_person_id) = pedigree_view();

    // Context menu person name.
    let ctx_person_name: String = match context_menu_person() {
        Some((pid, _, _)) => match pedigree_data.as_ref() {
            Some(data) => resolve_name(pid, &data.names, &i18n),
            None => resolve_name(pid, &HashMap::new(), &i18n),
        },
        None => String::new(),
    };

    // Check if context menu person has a union (is a spouse in some family).
    let ctx_person_has_union: bool = match context_menu_person() {
        Some((pid, _, _)) => pedigree_data
            .as_ref()
            .and_then(|d| d.families_as_spouse.get(&pid))
            .is_some_and(|fids| !fids.is_empty()),
        None => false,
    };

    // Union list for context menu multi-union sub-list.
    let ctx_unions: Vec<(Uuid, String, String)> = match context_menu_person() {
        Some((pid, _, _)) => pedigree_data
            .as_ref()
            .map(|d| d.unions_for_person(pid, &i18n))
            .unwrap_or_default(),
        None => vec![],
    };

    // Relatives the chart does not draw around the person, to go to from
    // the action picker: spouses and children in an ancestor chart, parents
    // and spouses in a descendant one.
    let view = crate::prefs::use_pedigree_view();
    let ctx_go_to: Vec<(String, Vec<(Uuid, String)>)> =
        match (context_menu_person(), pedigree_data.as_ref()) {
            (Some((pid, _, _)), Some(data)) => {
                go_to_relatives(data, pid, root_person_id, view, &i18n)
            }
            _ => Vec::new(),
        };

    // ── Handlers ──

    // Context menu action handler.
    let pedigree_data_ctx = pedigree_data.clone();
    let on_context_action = move |action: PersonAction| {
        let Some((pid, _, _)) = context_menu_person() else {
            return;
        };
        context_menu_person.set(None);

        match action {
            PersonAction::Edit => {
                editing_person_id.set(Some(pid));
            }
            PersonAction::Merge => {
                merging.set(Some(pid));
            }
            PersonAction::AddParents => {
                linking_mode.set(Some(LinkingMode::Parents(pid)));
            }
            PersonAction::AddSpouse => {
                linking_mode.set(Some(LinkingMode::Spouse(pid)));
            }
            PersonAction::AddChild => {
                linking_mode.set(Some(LinkingMode::Child(pid)));
            }
            PersonAction::AddSibling => {
                linking_mode.set(Some(LinkingMode::Sibling(pid)));
            }
            PersonAction::EditUnion => {
                let family_id = pedigree_data_ctx
                    .as_ref()
                    .and_then(|data| data.families_as_spouse.get(&pid))
                    .and_then(|fids| fids.first().copied());
                if let Some(fid) = family_id {
                    editing_union_id.set(Some(fid));
                }
            }
            PersonAction::EditSpecificUnion(fid) => {
                editing_union_id.set(Some(fid));
            }
            PersonAction::Kinship => {
                linking_mode.set(Some(LinkingMode::Kinship(pid)));
            }
            PersonAction::GoTo(relative) => {
                selected_root.set(Some(relative));
            }
            PersonAction::Delete => {
                confirm_delete_person_id.set(Some(pid));
                delete_person_error.set(None);
            }
        }
    };

    // Delete person handler.
    let api_del_person = api.clone();
    let on_confirm_delete_person = move |_| {
        let api = api_del_person.clone();
        let Some(tid) = tree_id_parsed() else { return };
        let Some(pid) = confirm_delete_person_id() else {
            return;
        };
        spawn(async move {
            match api.delete_person(tid, pid).await {
                Ok(_) => {
                    confirm_delete_person_id.set(None);
                    delete_person_error.set(None);
                    if selected_root() == Some(pid) {
                        selected_root.set(None);
                    }
                    tree_cache.invalidate();
                }
                Err(e) => delete_person_error.set(Some(format!("{e}"))),
            }
        });
    };

    let pedigree_data_empty = pedigree_data.clone();

    // ── Linking mode handlers ──

    // AddSpouse: link existing person as spouse.
    let api_link_spouse = api.clone();
    let pedigree_data_spouse = pedigree_data.clone();
    let on_link_spouse = move |person_id: Uuid| {
        let api = api_link_spouse.clone();
        let Some(tid) = tree_id_parsed() else { return };
        let Some(LinkingMode::Spouse(for_pid)) = linking_mode() else {
            return;
        };
        // Find or create a family for this person.
        let existing_family_id = pedigree_data_spouse
            .as_ref()
            .and_then(|data| data.families_as_spouse.get(&for_pid))
            .and_then(|fids| fids.first().copied());
        spawn(async move {
            let fid = if let Some(fid) = existing_family_id {
                fid
            } else {
                let Ok(family) = api.create_family(tid).await else {
                    return;
                };
                let body = crate::api::AddSpouseBody {
                    person_id: for_pid,
                    role: oxidgene_core::SpouseRole::Partner,
                    sort_order: 0,
                };
                let _ = api.add_spouse(tid, family.id, &body).await;
                family.id
            };
            let body = crate::api::AddSpouseBody {
                person_id,
                role: oxidgene_core::SpouseRole::Partner,
                sort_order: 1,
            };
            let _ = api.add_spouse(tid, fid, &body).await;
            linking_mode.set(None);
            tree_cache.invalidate();
        });
    };

    // AddSpouse: create new person as spouse.
    let api_new_spouse = api.clone();
    let pedigree_data_new_spouse = pedigree_data.clone();
    let on_create_new_spouse = move |_| {
        let api = api_new_spouse.clone();
        let Some(tid) = tree_id_parsed() else { return };
        let Some(LinkingMode::Spouse(for_pid)) = linking_mode() else {
            return;
        };
        let existing_family_id = pedigree_data_new_spouse
            .as_ref()
            .and_then(|data| data.families_as_spouse.get(&for_pid))
            .and_then(|fids| fids.first().copied());
        spawn(async move {
            let fid = if let Some(fid) = existing_family_id {
                fid
            } else {
                let Ok(family) = api.create_family(tid).await else {
                    return;
                };
                let body = crate::api::AddSpouseBody {
                    person_id: for_pid,
                    role: oxidgene_core::SpouseRole::Partner,
                    sort_order: 0,
                };
                let _ = api.add_spouse(tid, family.id, &body).await;
                family.id
            };
            if let Ok(new_person) = api
                .create_person(
                    tid,
                    &crate::api::CreatePersonBody {
                        sex: oxidgene_core::Sex::Unknown,
                    },
                )
                .await
            {
                let body = crate::api::AddSpouseBody {
                    person_id: new_person.id,
                    role: oxidgene_core::SpouseRole::Partner,
                    sort_order: 1,
                };
                let _ = api.add_spouse(tid, fid, &body).await;
            }
            linking_mode.set(None);
            tree_cache.invalidate();
        });
    };

    // AddParents: link existing person as parent.
    let api_link_parent = api.clone();
    let pedigree_data_parent = pedigree_data.clone();
    let on_link_parent = move |person_id: Uuid| {
        let api = api_link_parent.clone();
        let Some(tid) = tree_id_parsed() else { return };
        let Some(LinkingMode::Parents(child_id)) = linking_mode() else {
            return;
        };
        // Find or create a family where child_id is a child.
        let existing_family_id = pedigree_data_parent
            .as_ref()
            .and_then(|data| data.families_as_child.get(&child_id))
            .and_then(|fids| fids.first().copied());
        spawn(async move {
            let fid = if let Some(fid) = existing_family_id {
                fid
            } else {
                let Ok(family) = api.create_family(tid).await else {
                    return;
                };
                let body = crate::api::AddChildBody {
                    person_id: child_id,
                    child_type: oxidgene_core::ChildType::Biological,
                    sort_order: 0,
                };
                let _ = api.add_child(tid, family.id, &body).await;
                family.id
            };
            let body = crate::api::AddSpouseBody {
                person_id,
                role: oxidgene_core::SpouseRole::Partner,
                sort_order: 0,
            };
            let _ = api.add_spouse(tid, fid, &body).await;
            linking_mode.set(None);
            tree_cache.invalidate();
        });
    };

    // AddParents: create new person as parent.
    let api_new_parent = api.clone();
    let pedigree_data_new_parent = pedigree_data.clone();
    let on_create_new_parent = move |_| {
        let api = api_new_parent.clone();
        let Some(tid) = tree_id_parsed() else { return };
        let Some(LinkingMode::Parents(child_id)) = linking_mode() else {
            return;
        };
        let existing_family_id = pedigree_data_new_parent
            .as_ref()
            .and_then(|data| data.families_as_child.get(&child_id))
            .and_then(|fids| fids.first().copied());
        spawn(async move {
            let fid = if let Some(fid) = existing_family_id {
                fid
            } else {
                let Ok(family) = api.create_family(tid).await else {
                    return;
                };
                let body = crate::api::AddChildBody {
                    person_id: child_id,
                    child_type: oxidgene_core::ChildType::Biological,
                    sort_order: 0,
                };
                let _ = api.add_child(tid, family.id, &body).await;
                family.id
            };
            if let Ok(new_person) = api
                .create_person(
                    tid,
                    &crate::api::CreatePersonBody {
                        sex: oxidgene_core::Sex::Unknown,
                    },
                )
                .await
            {
                let body = crate::api::AddSpouseBody {
                    person_id: new_person.id,
                    role: oxidgene_core::SpouseRole::Partner,
                    sort_order: 0,
                };
                let _ = api.add_spouse(tid, fid, &body).await;
            }
            linking_mode.set(None);
            tree_cache.invalidate();
        });
    };

    // AddChild: link existing person as child.
    let api_link_child = api.clone();
    let pedigree_data_child = pedigree_data.clone();
    let on_link_child = move |person_id: Uuid| {
        let api = api_link_child.clone();
        let Some(tid) = tree_id_parsed() else { return };
        let Some(LinkingMode::Child(parent_id)) = linking_mode() else {
            return;
        };
        let existing_family_id = pedigree_data_child
            .as_ref()
            .and_then(|data| data.families_as_spouse.get(&parent_id))
            .and_then(|fids| fids.first().copied());
        spawn(async move {
            let fid = if let Some(fid) = existing_family_id {
                fid
            } else {
                let Ok(family) = api.create_family(tid).await else {
                    return;
                };
                let body = crate::api::AddSpouseBody {
                    person_id: parent_id,
                    role: oxidgene_core::SpouseRole::Partner,
                    sort_order: 0,
                };
                let _ = api.add_spouse(tid, family.id, &body).await;
                family.id
            };
            let body = crate::api::AddChildBody {
                person_id,
                child_type: oxidgene_core::ChildType::Biological,
                sort_order: 0,
            };
            let _ = api.add_child(tid, fid, &body).await;
            linking_mode.set(None);
            tree_cache.invalidate();
        });
    };

    // AddChild: create new person as child.
    let api_new_child = api.clone();
    let pedigree_data_new_child = pedigree_data.clone();
    let on_create_new_child = move |_| {
        let api = api_new_child.clone();
        let Some(tid) = tree_id_parsed() else { return };
        let Some(LinkingMode::Child(parent_id)) = linking_mode() else {
            return;
        };
        let existing_family_id = pedigree_data_new_child
            .as_ref()
            .and_then(|data| data.families_as_spouse.get(&parent_id))
            .and_then(|fids| fids.first().copied());
        spawn(async move {
            let fid = if let Some(fid) = existing_family_id {
                fid
            } else {
                let Ok(family) = api.create_family(tid).await else {
                    return;
                };
                let body = crate::api::AddSpouseBody {
                    person_id: parent_id,
                    role: oxidgene_core::SpouseRole::Partner,
                    sort_order: 0,
                };
                let _ = api.add_spouse(tid, family.id, &body).await;
                family.id
            };
            if let Ok(new_person) = api
                .create_person(
                    tid,
                    &crate::api::CreatePersonBody {
                        sex: oxidgene_core::Sex::Unknown,
                    },
                )
                .await
            {
                let body = crate::api::AddChildBody {
                    person_id: new_person.id,
                    child_type: oxidgene_core::ChildType::Biological,
                    sort_order: 0,
                };
                let _ = api.add_child(tid, fid, &body).await;
            }
            linking_mode.set(None);
            tree_cache.invalidate();
        });
    };

    // AddSibling: link existing person as sibling (add them to the same parent family).
    let api_link_sibling = api.clone();
    let pedigree_data_sibling = pedigree_data.clone();
    let on_link_sibling = move |person_id: Uuid| {
        let api = api_link_sibling.clone();
        let Some(tid) = tree_id_parsed() else { return };
        let Some(LinkingMode::Sibling(for_pid)) = linking_mode() else {
            return;
        };
        // Find the parent family of the person we want to add a sibling to.
        let parent_family_id = pedigree_data_sibling
            .as_ref()
            .and_then(|data| data.families_as_child.get(&for_pid))
            .and_then(|fids| fids.first().copied());
        spawn(async move {
            let fid = if let Some(fid) = parent_family_id {
                fid
            } else {
                // No parent family exists yet — create one and add the original person as child.
                let Ok(family) = api.create_family(tid).await else {
                    return;
                };
                let body = crate::api::AddChildBody {
                    person_id: for_pid,
                    child_type: oxidgene_core::ChildType::Biological,
                    sort_order: 0,
                };
                let _ = api.add_child(tid, family.id, &body).await;
                family.id
            };
            let body = crate::api::AddChildBody {
                person_id,
                child_type: oxidgene_core::ChildType::Biological,
                sort_order: 1,
            };
            let _ = api.add_child(tid, fid, &body).await;
            linking_mode.set(None);
            tree_cache.invalidate();
        });
    };

    // AddSibling: create new person as sibling.
    let api_new_sibling = api.clone();
    let pedigree_data_new_sibling = pedigree_data.clone();
    let on_create_new_sibling = move |_| {
        let api = api_new_sibling.clone();
        let Some(tid) = tree_id_parsed() else { return };
        let Some(LinkingMode::Sibling(for_pid)) = linking_mode() else {
            return;
        };
        let parent_family_id = pedigree_data_new_sibling
            .as_ref()
            .and_then(|data| data.families_as_child.get(&for_pid))
            .and_then(|fids| fids.first().copied());
        spawn(async move {
            let fid = if let Some(fid) = parent_family_id {
                fid
            } else {
                let Ok(family) = api.create_family(tid).await else {
                    return;
                };
                let body = crate::api::AddChildBody {
                    person_id: for_pid,
                    child_type: oxidgene_core::ChildType::Biological,
                    sort_order: 0,
                };
                let _ = api.add_child(tid, family.id, &body).await;
                family.id
            };
            if let Ok(new_person) = api
                .create_person(
                    tid,
                    &crate::api::CreatePersonBody {
                        sex: oxidgene_core::Sex::Unknown,
                    },
                )
                .await
            {
                let body = crate::api::AddChildBody {
                    person_id: new_person.id,
                    child_type: oxidgene_core::ChildType::Biological,
                    sort_order: 1,
                };
                let _ = api.add_child(tid, fid, &body).await;
            }
            linking_mode.set(None);
            tree_cache.invalidate();
        });
    };

    // Merge: link existing person to merge with.
    // Kinship: open the relationship page between the two persons.
    let tree_id_kinship = tree_id.clone();
    let on_pick_kinship = move |other: Uuid| {
        let Some(LinkingMode::Kinship(from)) = linking_mode() else {
            return;
        };
        linking_mode.set(None);
        if other != from {
            nav.push(Route::Kinship {
                tree_id: tree_id_kinship.clone(),
                from: from.to_string(),
                to: other.to_string(),
            });
        }
    };
    // The persons most often asked about, offered before any search: the
    // user themself and the tree's SOSA root.
    let kinship_shortcuts: Vec<(Uuid, String)> = match (linking_mode(), &*tree_resource.read()) {
        (Some(LinkingMode::Kinship(from)), Some(Ok(tree))) => [
            (tree.self_person_id, "kinship.pick_self"),
            (tree.sosa_root_person_id, "kinship.pick_sosa_root"),
        ]
        .into_iter()
        .filter_map(|(id, key)| Some((id?, key)))
        .filter(|&(id, _)| id != from)
        .fold(Vec::new(), |mut picks, (id, key)| {
            if !picks.iter().any(|&(seen, _)| seen == id) {
                picks.push((id, i18n.t(key)));
            }
            picks
        }),
        _ => Vec::new(),
    };

    // Linking mode label for the panel header.
    let linking_label: Option<String> = linking_mode().map(|mode| match &mode {
        LinkingMode::Spouse(_) => i18n.t("linking.add_spouse"),
        LinkingMode::Parents(_) => i18n.t("linking.add_parent"),
        LinkingMode::Child(_) => i18n.t("linking.add_child"),
        LinkingMode::Sibling(_) => i18n.t("linking.add_sibling"),
        LinkingMode::Kinship(_) => i18n.t("context.kinship"),
    });

    // ── Render ──

    rsx! {
        div { class: "tree-detail-page",

        // ── Topbar: breadcrumb + search ──
        {
            let tree_name_str = {
                let guard = tree_resource.read();
                match &*guard {
                    Some(Ok(t)) => t.name.clone(),
                    _ => tree_id_parsed()
                        .and_then(|tid| tree_cache.tree(tid))
                        .map(|t| t.name)
                        .unwrap_or_default(),
                }
            };

            rsx! {
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
            }
        }

        // "Merge with…": the wizard, from the search for the other record.
        if let (Some(pid), Some(tid)) = (merging(), tree_id_parsed()) {
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
        if confirm_delete_person_id().is_some() {
            ConfirmDialog {
                title: i18n.t("confirm.delete_person.title"),
                message: i18n.t("confirm.delete_person.message"),
                confirm_label: i18n.t("common.delete"),
                confirm_class: "btn btn-danger",
                error: delete_person_error(),
                on_confirm: move |_| on_confirm_delete_person(()),
                on_cancel: move |_| {
                    confirm_delete_person_id.set(None);
                    delete_person_error.set(None);
                },
            }
        }

        // Context menu
        if let Some((_pid, x, y)) = context_menu_person() {
            ContextMenu {
                person_name: ctx_person_name.clone(),
                x: x,
                y: y,
                has_union: ctx_person_has_union,
                unions: ctx_unions.clone(),
                go_to: ctx_go_to.clone(),
                on_action: on_context_action,
                on_close: move |_| context_menu_person.set(None),
            }
        }

        // Person edit modal
        if let Some(edit_pid) = editing_person_id() {
            if let Some(tid) = tree_id_parsed() {
                PersonForm {
                    tree_id: tid,
                    person_id: Some(edit_pid),
                    on_close: move |_| editing_person_id.set(None),
                    on_saved: move |_| tree_cache.invalidate(),
                    // The edited person may be the chart's root, and no longer
                    // exists: centre the chart on the one they were merged into.
                    on_merged: {
                        let tree_id = tree_id.clone();
                        move |kept: Uuid| {
                            nav.replace(Route::TreeDetail {
                                tree_id: tree_id.clone(),
                                person: Some(kept.to_string()),
                            });
                        }
                    },
                }
            }
        }

        // Person create modal
        if let Some(ctx) = creating_person_ctx() {
            if let Some(tid) = tree_id_parsed() {
                PersonForm {
                    tree_id: tid,
                    create_context: ctx,
                    on_close: move |_| creating_person_ctx.set(None),
                    on_saved: move |_| tree_cache.invalidate(),
                }
            }
        }

        // Union edit modal
        if let Some(union_fid) = editing_union_id() {
            if let Some(tid) = tree_id_parsed() {
                UnionForm {
                    tree_id: tid,
                    family_id: union_fid,
                    on_close: move |_| editing_union_id.set(None),
                    on_saved: move |_| tree_cache.invalidate(),
                }
            }
        }

        // ── Pedigree chart (fills remaining space) ──
        div { class: "pedigree-card",

            // Chart
            if let (Some(data), Some(root_id)) = (pedigree_data.clone(), root_person_id) {
                PedigreeChart {
                    root_person_id: root_id,
                    data: data,
                    tree_id: tree_id.clone(),
                    sosa_root_person_id: {
                        let guard = tree_resource.read();
                        match &*guard {
                            Some(Ok(tree)) => tree.sosa_root_person_id,
                            _ => None,
                        }
                    },
                    sosa_ancestor_ids: {
                        let guard = sosa_ancestors_resource.read();
                        match &*guard {
                            Some(set) if !set.is_empty() => Some(set.clone()),
                            _ => None,
                        }
                    },
                    center_gen: center_gen(),
                    on_person_click: move |(pid, x, y)| {
                        context_menu_person.set(Some((pid, x, y)));
                    },
                    on_person_navigate: move |pid| {
                        selected_root.set(Some(pid));
                    },
                    on_empty_slot: move |(child_id, is_father)| {
                        let family_id = pedigree_data_empty
                            .as_ref()
                            .and_then(|data| data.families_as_child.get(&child_id))
                            .and_then(|fids| fids.first().copied());
                        let child_surname = pedigree_data_empty
                            .as_ref()
                            .and_then(|data| data.names.get(&child_id))
                            .and_then(|names| {
                                names
                                    .iter()
                                    .find(|name| name.is_primary)
                                    .or_else(|| names.first())
                            })
                            .and_then(|name| name.full_surname())
                            .filter(|surname| !surname.trim().is_empty());
                        creating_person_ctx.set(Some(PersonFormCreateContext::AddParent {
                            child_id,
                            family_id,
                            is_father,
                            child_surname,
                        }));
                    },
                    on_add_spouse_slot: move |person_id| {
                        linking_mode.set(Some(LinkingMode::Spouse(person_id)));
                    },
                    on_add_person: move |_| {
                        creating_person_ctx.set(Some(PersonFormCreateContext::Standalone));
                    },
                    on_profile_view: {
                        let tree_id = tree_id.clone();
                        move |pid: Uuid| {
                            nav.push(Route::PersonDetail {
                                tree_id: tree_id.clone(),
                                person_id: pid.to_string(),
                            });
                        }
                    },
                    on_couple_view: {
                        let tree_id = tree_id.clone();
                        move |family_id: Uuid| {
                            nav.push(Route::CoupleDetail {
                                tree_id: tree_id.clone(),
                                family_id: family_id.to_string(),
                            });
                        }
                    },
                    on_settings: {
                        let tree_id = tree_id.clone();
                        move |_| {
                            nav.push(Route::Settings {
                                tree_id: tree_id.clone(),
                            });
                        }
                    },
                    on_dictionary: {
                        let tree_id = tree_id.clone();
                        move |_| {
                            nav.push(Route::Dictionary {
                                tree_id: tree_id.clone(),
                            });
                        }
                    },
                }
            } else {
                // Loading or empty state
                {
                    let ped_data = pedigree_resource.read();
                    // Show empty-tree UI when pedigree loaded but tree has no persons,
                    // or when the cache API returned a 404 (no persons in tree).
                    let all_loaded = match &*ped_data {
                        Some(Ok(_)) => true, // Has data but no pedigree_data (shouldn't happen)
                        Some(Err(_)) => true, // Error = either no persons or network error
                        None => false,        // Still loading
                    };

                    if all_loaded {
                        rsx! {
                            div { class: "empty-tree-container",
                                button {
                                    class: "empty-tree-slot",
                                    title: "{i18n.t(\"tree.no_persons_hint\")}",
                                    onclick: move |_| {
                                        let api = api.clone();
                                        let Some(tid) = tree_id_parsed() else { return };
                                        spawn(async move {
                                            if let Ok(new_person) = api.create_person(tid, &crate::api::CreatePersonBody { sex: oxidgene_core::Sex::Unknown }).await {
                                                editing_person_id.set(Some(new_person.id));
                                                tree_cache.invalidate();
                                            }
                                        });
                                    },
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
                    } else {
                        rsx! {
                            div { class: "loading", {i18n.t("tree.loading_pedigree")} }
                        }
                    }
                }
            }
        }

        // ── Linking panel (search-or-create for AddSpouse/AddParents/AddChild) ──
        if let (Some(label), Some(tid)) = (linking_label, tree_id_parsed()) {
            div { class: "card linking-card",
                div { class: "section-header",
                    h2 { style: "font-size: 1.1rem;", "{label}" }
                    button {
                        class: "btn btn-outline btn-sm",
                        onclick: move |_| linking_mode.set(None),
                        {i18n.t("common.cancel")}
                    }
                }

                div { class: "linking-panel",
                    p { class: "linking-panel-title",
                        {i18n.t("linking.search_existing")}
                    }

                    // Determine which handler to use based on mode.
                    {
                        let mode = linking_mode();
                        match mode {
                            Some(LinkingMode::Spouse(_)) => rsx! {
                                SearchPerson {
                                    tree_id: tid,
                                    placeholder: i18n.t("linking.search_spouse"),
                                    on_select: on_link_spouse,
                                    on_cancel: move |_| linking_mode.set(None),
                                }
                                div { class: "linking-panel-or", {i18n.t("common.or_divider")} }
                                button {
                                    class: "btn btn-outline",
                                    onclick: on_create_new_spouse,
                                    {i18n.t("linking.create_spouse")}
                                }
                            },
                            Some(LinkingMode::Parents(_)) => rsx! {
                                SearchPerson {
                                    tree_id: tid,
                                    placeholder: i18n.t("linking.search_parent"),
                                    on_select: on_link_parent,
                                    on_cancel: move |_| linking_mode.set(None),
                                }
                                div { class: "linking-panel-or", {i18n.t("common.or_divider")} }
                                button {
                                    class: "btn btn-outline",
                                    onclick: on_create_new_parent,
                                    {i18n.t("linking.create_parent")}
                                }
                            },
                            Some(LinkingMode::Child(_)) => rsx! {
                                SearchPerson {
                                    tree_id: tid,
                                    placeholder: i18n.t("linking.search_child"),
                                    on_select: on_link_child,
                                    on_cancel: move |_| linking_mode.set(None),
                                }
                                div { class: "linking-panel-or", {i18n.t("common.or_divider")} }
                                button {
                                    class: "btn btn-outline",
                                    onclick: on_create_new_child,
                                    {i18n.t("linking.create_child")}
                                }
                            },
                            Some(LinkingMode::Sibling(_)) => rsx! {
                                SearchPerson {
                                    tree_id: tid,
                                    placeholder: i18n.t("linking.search_sibling"),
                                    on_select: on_link_sibling,
                                    on_cancel: move |_| linking_mode.set(None),
                                }
                                div { class: "linking-panel-or", {i18n.t("common.or_divider")} }
                                button {
                                    class: "btn btn-outline",
                                    onclick: on_create_new_sibling,
                                    {i18n.t("linking.create_sibling")}
                                }
                            },
                            Some(LinkingMode::Kinship(_)) => rsx! {
                                if !kinship_shortcuts.is_empty() {
                                    div { class: "linking-shortcuts",
                                        for (id, label) in kinship_shortcuts.iter().cloned() {
                                            {
                                                let mut pick = on_pick_kinship.clone();
                                                rsx! {
                                                    button {
                                                        class: "btn btn-outline btn-sm",
                                                        onclick: move |_| pick(id),
                                                        "{label}"
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                SearchPerson {
                                    tree_id: tid,
                                    placeholder: i18n.t("kinship.choose"),
                                    on_select: on_pick_kinship,
                                    on_cancel: move |_| linking_mode.set(None),
                                }
                            },
                            None => rsx! {},
                        }
                    }
                }
            }
        }

        } // close .tree-detail-page
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
        let dates = format_lifespan(data.qualified_birth_year(id), data.qualified_death_year(id));
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
