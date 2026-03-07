//! Tree detail page — Geneanet-style pedigree chart view.
//!
//! Shows the tree breadcrumb, search fields, the [`PedigreeChart`] as the
//! main view, a context menu for person actions (including search-or-create
//! flows for AddSpouse/AddParents/AddChild), and union editing.

use std::collections::HashMap;

use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::ApiClient;
use crate::components::confirm_dialog::ConfirmDialog;
use crate::components::context_menu::{ContextMenu, PersonAction};
use crate::components::pedigree_chart::{PedigreeChart, PedigreeData};
use crate::components::person_form::PersonForm;
use crate::components::search_person::SearchPerson;
use crate::components::tree_cache::{
    fetch_snapshot_cached, fetch_tree_cached, use_tree_cache, use_view_state_cache,
};
use crate::components::union_form::UnionForm;
use crate::i18n::use_i18n;
use crate::router::Route;
use crate::utils::resolve_name;

/// Isolated search bar — lives in its own component so signal updates on
/// each keystroke only re-render this small widget, not the entire TreeDetail.
#[component]
fn TopbarSearch(tree_id: String) -> Element {
    let i18n = use_i18n();
    let nav = use_navigator();
    let mut search_last = use_signal(String::new);
    let mut search_first = use_signal(String::new);

    let do_search = {
        let tree_id = tree_id.clone();
        move || {
            if !search_last().trim().is_empty() || !search_first().trim().is_empty() {
                nav.push(Route::SearchResults {
                    tree_id: tree_id.clone(),
                    last: if search_last().is_empty() {
                        None
                    } else {
                        Some(search_last())
                    },
                    first: if search_first().is_empty() {
                        None
                    } else {
                        Some(search_first())
                    },
                });
            }
        }
    };

    let do_search2 = do_search.clone();
    let do_search3 = do_search.clone();
    let on_search_enter = move |e: Event<KeyboardData>| {
        if e.key() == Key::Enter {
            do_search();
        }
    };
    let on_search_enter2 = move |e: Event<KeyboardData>| {
        if e.key() == Key::Enter {
            do_search2();
        }
    };
    let on_search_btn = move |_| {
        do_search3();
    };

    rsx! {
        div { class: "td-search-group",
            input {
                r#type: "text",
                class: "td-search-input",
                placeholder: "{i18n.t(\"tree.search_last\")}",
                value: "{search_last}",
                oninput: move |e: Event<FormData>| search_last.set(e.value()),
                onkeydown: on_search_enter,
            }
            input {
                r#type: "text",
                class: "td-search-input",
                placeholder: "{i18n.t(\"tree.search_first\")}",
                value: "{search_first}",
                oninput: move |e: Event<FormData>| search_first.set(e.value()),
                onkeydown: on_search_enter2,
            }
            button {
                class: "td-search-btn",
                title: "{i18n.t(\"tree.search\")}",
                onclick: on_search_btn,
                svg {
                    width: "14",
                    height: "14",
                    fill: "none",
                    "viewBox": "0 0 24 24",
                    stroke: "currentColor",
                    "strokeWidth": "2.5",
                    circle { cx: "11", cy: "11", r: "8" }
                    line { x1: "21", y1: "21", x2: "16.65", y2: "16.65" }
                }
            }
        }
    }
}

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
    /// Merging person with another (search target).
    Merge(Uuid),
}

/// Page rendered at `/trees/:tree_id?person=...`.
#[component]
pub fn TreeDetail(tree_id: String, person: Option<String>) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let nav = use_navigator();

    // ── Global caches ──
    let tree_cache = use_tree_cache();
    let view_cache = use_view_state_cache();

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
    let mut center_gen = use_signal(|| 0u32);

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

    // ── Union edit modal ──
    let mut editing_union_id = use_signal(|| None::<Uuid>);

    // ── Linking mode (search-or-create panel) ──
    let mut linking_mode = use_signal(|| None::<LinkingMode>);

    // ── Delete person confirmation ──
    let mut confirm_delete_person_id = use_signal(|| None::<Uuid>);
    let mut delete_person_error = use_signal(|| None::<String>);

    // ── Fetch tree details (cache-backed) ──
    let api_tree = api.clone();
    let mut tree_resource = use_resource(move || {
        let api = api_tree.clone();
        let _gen = tree_cache.generation();
        let tid = tree_id_parsed();
        async move {
            let Some(tid) = tid else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: "Invalid tree ID".to_string(),
                });
            };
            fetch_tree_cached(&api, &tree_cache, tid).await
        }
    });

    // ── Fetch tree snapshot (cache-backed) ──
    let api_snapshot = api.clone();
    let mut snapshot_resource = use_resource(move || {
        let api = api_snapshot.clone();
        let _gen = tree_cache.generation();
        let tid = tree_id_parsed();
        async move {
            let Some(tid) = tid else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: "Invalid tree ID".to_string(),
                });
            };
            fetch_snapshot_cached(&api, &tree_cache, tid).await
        }
    });

    // Force resources to re-fetch when tree_id changes (component reused by router).
    if tree_changed {
        tree_resource.restart();
        snapshot_resource.restart();
    }

    // ── Build pedigree data from snapshot ──
    let pedigree_data: Option<PedigreeData> = {
        let snap_data = snapshot_resource.read();
        match &*snap_data {
            Some(Ok(snap)) => {
                let mut name_map: HashMap<Uuid, Vec<oxidgene_core::types::PersonName>> =
                    HashMap::new();
                for name in &snap.names {
                    name_map
                        .entry(name.person_id)
                        .or_default()
                        .push(name.clone());
                }
                let places: HashMap<Uuid, oxidgene_core::types::Place> =
                    snap.places.iter().map(|p| (p.id, p.clone())).collect();
                Some(PedigreeData::build(
                    &snap.persons,
                    name_map,
                    &snap.spouses,
                    &snap.children,
                    snap.events.clone(),
                    places,
                ))
            }
            _ => None,
        }
    };

    // Determine root person: selected or first person.
    let root_person_id: Option<Uuid> = {
        if let Some(sel) = selected_root() {
            Some(sel)
        } else {
            let snap_data = snapshot_resource.read();
            match &*snap_data {
                Some(Ok(snap)) => snap.persons.first().map(|p| p.id),
                _ => None,
            }
        }
    };

    // Build name_map for context menu lookups.
    let name_map: HashMap<Uuid, Vec<oxidgene_core::types::PersonName>> = {
        let snap_data = snapshot_resource.read();
        match &*snap_data {
            Some(Ok(snap)) => {
                let mut map = HashMap::new();
                for name in &snap.names {
                    map.entry(name.person_id)
                        .or_insert_with(Vec::new)
                        .push(name.clone());
                }
                map
            }
            _ => HashMap::new(),
        }
    };

    // Context menu person name.
    let ctx_person_name: String = {
        let ctx = context_menu_person();
        match ctx {
            Some((pid, _, _)) => resolve_name(pid, &name_map),
            None => String::new(),
        }
    };

    // Check if context menu person has a union (is a spouse in some family).
    let ctx_person_has_union: bool = {
        let ctx = context_menu_person();
        match ctx {
            Some((pid, _, _)) => pedigree_data
                .as_ref()
                .and_then(|d| d.families_as_spouse.get(&pid))
                .is_some_and(|fids| !fids.is_empty()),
            None => false,
        }
    };

    // Union list for context menu multi-union sub-list.
    let ctx_unions: Vec<(Uuid, String, String)> = {
        let ctx = context_menu_person();
        match ctx {
            Some((pid, _, _)) => pedigree_data
                .as_ref()
                .map(|d| d.unions_for_person(pid))
                .unwrap_or_default(),
            None => vec![],
        }
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
                linking_mode.set(Some(LinkingMode::Merge(pid)));
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

    // Empty slot handler — add a parent for a child.
    let api_empty = api.clone();
    let pedigree_data_empty = pedigree_data.clone();
    let on_empty_slot = move |(child_id, is_father): (Uuid, bool)| {
        let api = api_empty.clone();
        let Some(tid) = tree_id_parsed() else { return };

        let family_id = pedigree_data_empty
            .as_ref()
            .and_then(|data| data.families_as_child.get(&child_id))
            .and_then(|fids| fids.first().copied());

        spawn(async move {
            let sex = if is_father {
                oxidgene_core::Sex::Male
            } else {
                oxidgene_core::Sex::Female
            };
            let Ok(new_person) = api
                .create_person(tid, &crate::api::CreatePersonBody { sex })
                .await
            else {
                return;
            };

            let fid = if let Some(fid) = family_id {
                fid
            } else {
                let Ok(family) = api.create_family(tid).await else {
                    return;
                };
                let child_body = crate::api::AddChildBody {
                    person_id: child_id,
                    child_type: oxidgene_core::ChildType::Biological,
                    sort_order: 0,
                };
                let _ = api.add_child(tid, family.id, &child_body).await;
                family.id
            };

            let role = if is_father {
                oxidgene_core::SpouseRole::Husband
            } else {
                oxidgene_core::SpouseRole::Wife
            };
            let spouse_body = crate::api::AddSpouseBody {
                person_id: new_person.id,
                role,
                sort_order: 0,
            };
            let _ = api.add_spouse(tid, fid, &spouse_body).await;
            tree_cache.invalidate();
        });
    };

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
    let on_link_merge = move |_target_id: Uuid| {
        // TODO: Implement merge UI — should show a modal to choose which
        // events/info/sources to keep from each person, then delete one.
        // For now, just close the linking panel.
        linking_mode.set(None);
    };

    // Linking mode label for the panel header.
    let linking_label: Option<String> = linking_mode().map(|mode| match &mode {
        LinkingMode::Spouse(_) => i18n.t("linking.add_spouse"),
        LinkingMode::Parents(_) => i18n.t("linking.add_parent"),
        LinkingMode::Child(_) => i18n.t("linking.add_child"),
        LinkingMode::Sibling(_) => i18n.t("linking.add_sibling"),
        LinkingMode::Merge(_) => i18n.t("linking.merge"),
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
                    Some(Err(_)) => "Error".to_string(),
                    None => "Loading…".to_string(),
                }
            };

            rsx! {
                div { class: "td-topbar",
                    nav { class: "td-bc",
                        Link { to: Route::Home {}, class: "td-bc-logo",
                            img {
                                src: crate::components::layout::LOGO_PNG_B64,
                                alt: "OxidGene",
                                class: "td-bc-logo-img",
                            }
                        }
                        span { class: "td-bc-current", "{tree_name_str}" }
                    }
                    if root_person_id.is_some() {
                        TopbarSearch { tree_id: tree_id.clone() }
                    }
                }
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
                on_action: on_context_action,
                on_close: move |_| context_menu_person.set(None),
            }
        }

        // Person edit modal
        if let Some(edit_pid) = editing_person_id() {
            if let Some(tid) = tree_id_parsed() {
                PersonForm {
                    tree_id: tid,
                    person_id: edit_pid,
                    on_close: move |_| editing_person_id.set(None),
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
                    center_gen: center_gen(),
                    on_person_click: move |(pid, x, y)| {
                        context_menu_person.set(Some((pid, x, y)));
                    },
                    on_person_navigate: move |pid| {
                        selected_root.set(Some(pid));
                    },
                    on_empty_slot: move |(child_id, is_father)| {
                        on_empty_slot((child_id, is_father));
                    },
                    on_add_person: move |_| {
                        // Create a new person and open edit form
                        let api = api.clone();
                        let Some(tid) = tree_id_parsed() else { return };
                        spawn(async move {
                            if let Ok(new_person) = api.create_person(tid, &crate::api::CreatePersonBody { sex: oxidgene_core::Sex::Unknown }).await {
                                editing_person_id.set(Some(new_person.id));
                                tree_cache.invalidate();
                            }
                        });
                    },
                    on_profile_view: move |pid: Uuid| {
                        nav.push(Route::PersonDetail {
                            tree_id: tree_id.clone(),
                            person_id: pid.to_string(),
                        });
                    },
                }
            } else {
                // Loading or empty state
                {
                    let snap_data = snapshot_resource.read();
                    let all_loaded = snap_data.is_some();

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
                            Some(LinkingMode::Merge(_)) => rsx! {
                                p { style: "font-size: 0.85rem; color: var(--text-secondary); margin-bottom: 8px;",
                                    {i18n.t("linking.merge_hint")}
                                }
                                SearchPerson {
                                    tree_id: tid,
                                    placeholder: i18n.t("linking.search_merge"),
                                    on_select: on_link_merge,
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
