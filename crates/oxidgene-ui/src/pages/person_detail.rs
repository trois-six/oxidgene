//! Person detail page — shows names, events, notes, citations, and ancestry charts with full CRUD.

use std::collections::HashMap;

use dioxus::prelude::*;
use oxidgene_core::EventType;
use oxidgene_core::types::Event as DomainEvent;
use uuid::Uuid;

use crate::api::{
    ApiClient, CreateCitationBody, CreateEventBody, CreateNoteBody, CreatePersonNameBody,
    UpdateEventBody, UpdateNoteBody, UpdatePersonBody, UpdatePersonNameBody,
};
use crate::components::confirm_dialog::ConfirmDialog;
use crate::components::person_node::PersonNode;
use crate::i18n::use_i18n;
use crate::router::Route;
use crate::utils::{
    generation_label, opt_str, parse_confidence, parse_event_type, parse_name_type, parse_sex,
    resolve_name,
};
use oxidgene_core::Sex;

/// Indicates the origin of an event relative to the displayed person.
#[derive(Clone, Debug, PartialEq)]
enum EventOrigin {
    /// Event directly attached to this person (birth, death, occupation…).
    Individual,
    /// Event from a conjugal family (marriage, divorce…).
    ConjugalFamily,
    /// Event from a child (birth, death, baptism, burial of a child).
    ChildFamily,
    /// Event from the parental family (parent death, sibling birth…).
    ParentalFamily,
}

/// An event enriched with origin metadata for display purposes.
#[derive(Clone, Debug)]
struct EnrichedEvent {
    event: DomainEvent,
    origin: EventOrigin,
    /// Optional context label (e.g. spouse name, sibling name).
    context: Option<String>,
}

/// Page rendered at `/trees/:tree_id/persons/:person_id`.
#[component]
pub fn PersonDetail(tree_id: String, person_id: String) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let nav = use_navigator();
    let mut refresh = use_signal(|| 0u32);

    let tree_id_parsed = tree_id.parse::<Uuid>().ok();
    let person_id_parsed = person_id.parse::<Uuid>().ok();

    // Delete confirmation state.
    let mut confirm_delete = use_signal(|| false);
    let mut delete_error = use_signal(|| None::<String>);

    // Edit person sex state.
    let mut editing_sex = use_signal(|| false);
    let mut edit_sex_val = use_signal(|| "Unknown".to_string());
    let mut edit_sex_error = use_signal(|| None::<String>);

    // Add name form state.
    let mut show_name_form = use_signal(|| false);
    let mut name_form_type = use_signal(|| "Birth".to_string());
    let mut name_form_given = use_signal(String::new);
    let mut name_form_surname = use_signal(String::new);
    let mut name_form_prefix = use_signal(String::new);
    let mut name_form_suffix = use_signal(String::new);
    let mut name_form_nickname = use_signal(String::new);
    let mut name_form_primary = use_signal(|| true);
    let mut name_form_error = use_signal(|| None::<String>);

    // Edit name state: which name is being edited.
    let mut editing_name_id = use_signal(|| None::<Uuid>);
    let mut edit_name_type = use_signal(|| "Birth".to_string());
    let mut edit_name_given = use_signal(String::new);
    let mut edit_name_surname = use_signal(String::new);
    let mut edit_name_prefix = use_signal(String::new);
    let mut edit_name_suffix = use_signal(String::new);
    let mut edit_name_nickname = use_signal(String::new);
    let mut edit_name_primary = use_signal(|| false);
    let mut edit_name_error = use_signal(|| None::<String>);

    // Delete name confirmation.
    let mut confirm_delete_name_id = use_signal(|| None::<Uuid>);
    let mut delete_name_error = use_signal(|| None::<String>);

    // Add event form state.
    let mut show_event_form = use_signal(|| false);
    let mut event_form_type = use_signal(|| "Birth".to_string());
    let mut event_form_date = use_signal(String::new);
    let mut event_form_place_id = use_signal(String::new);
    let mut event_form_desc = use_signal(String::new);
    let mut event_form_error = use_signal(|| None::<String>);

    // Edit event state.
    let mut editing_event_id = use_signal(|| None::<Uuid>);
    let mut edit_event_type = use_signal(|| "Birth".to_string());
    let mut edit_event_date = use_signal(String::new);
    let mut edit_event_place_id = use_signal(String::new);
    let mut edit_event_desc = use_signal(String::new);
    let mut edit_event_error = use_signal(|| None::<String>);

    // Delete event confirmation.
    let mut confirm_delete_event_id = use_signal(|| None::<Uuid>);
    let mut delete_event_error = use_signal(|| None::<String>);

    // Add note form state.
    let mut show_note_form = use_signal(|| false);
    let mut note_form_text = use_signal(String::new);
    let mut note_form_error = use_signal(|| None::<String>);

    // Edit note state.
    let mut editing_note_id = use_signal(|| None::<Uuid>);
    let mut edit_note_text = use_signal(String::new);
    let mut edit_note_error = use_signal(|| None::<String>);

    // Delete note confirmation.
    let mut confirm_delete_note_id = use_signal(|| None::<Uuid>);
    let mut delete_note_error = use_signal(|| None::<String>);

    // Add citation form state.
    let mut show_citation_form = use_signal(|| false);
    let mut citation_form_source_id = use_signal(String::new);
    let mut citation_form_page = use_signal(String::new);
    let mut citation_form_confidence = use_signal(|| "Medium".to_string());
    let mut citation_form_text = use_signal(String::new);
    let mut citation_form_error = use_signal(|| None::<String>);

    // Delete citation confirmation.
    let mut confirm_delete_citation_id = use_signal(|| None::<Uuid>);
    let mut delete_citation_error = use_signal(|| None::<String>);

    // Ancestry chart toggle signals.
    let mut show_ancestors = use_signal(|| false);
    let mut show_descendants = use_signal(|| false);

    // ── Resources ────────────────────────────────────────────────────

    // Fetch person.
    let api_person = api.clone();
    let person_resource = use_resource(move || {
        let api = api_person.clone();
        let _tick = refresh();
        let tid = tree_id_parsed;
        let pid = person_id_parsed;
        async move {
            let (Some(tid), Some(pid)) = (tid, pid) else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: "Invalid IDs".to_string(),
                });
            };
            api.get_person(tid, pid).await
        }
    });

    // Fetch person names.
    let api_names = api.clone();
    let names_resource = use_resource(move || {
        let api = api_names.clone();
        let _tick = refresh();
        let tid = tree_id_parsed;
        let pid = person_id_parsed;
        async move {
            let (Some(tid), Some(pid)) = (tid, pid) else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: "Invalid IDs".to_string(),
                });
            };
            api.list_person_names(tid, pid).await
        }
    });

    // Fetch person events.
    let api_events = api.clone();
    let events_resource = use_resource(move || {
        let api = api_events.clone();
        let _tick = refresh();
        let tid = tree_id_parsed;
        let pid = person_id_parsed;
        async move {
            let (Some(tid), Some(pid)) = (tid, pid) else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: "Invalid IDs".to_string(),
                });
            };
            api.list_events(tid, Some(100), None, None, Some(pid), None)
                .await
        }
    });

    // Fetch places in tree (for place picker in events).
    let api_places = api.clone();
    let places_resource = use_resource(move || {
        let api = api_places.clone();
        let _tick = refresh();
        let tid = tree_id_parsed;
        async move {
            let Some(tid) = tid else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: "Invalid IDs".to_string(),
                });
            };
            api.list_places(tid, Some(100), None, None).await
        }
    });

    // Fetch notes for this person.
    let api_notes = api.clone();
    let notes_resource = use_resource(move || {
        let api = api_notes.clone();
        let _tick = refresh();
        let tid = tree_id_parsed;
        let pid = person_id_parsed;
        async move {
            let (Some(tid), Some(pid)) = (tid, pid) else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: "Invalid IDs".to_string(),
                });
            };
            api.list_notes(tid, Some(pid), None, None, None).await
        }
    });

    // Fetch sources in tree (for citation form picker).
    let api_sources = api.clone();
    let sources_resource = use_resource(move || {
        let api = api_sources.clone();
        let _tick = refresh();
        let tid = tree_id_parsed;
        async move {
            let Some(tid) = tid else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: "Invalid IDs".to_string(),
                });
            };
            api.list_sources(tid, Some(100), None).await
        }
    });

    // Fetch ancestors (lazy: only when toggled on).
    let api_ancestors = api.clone();
    let ancestors_resource = use_resource(move || {
        let api = api_ancestors.clone();
        let _tick = refresh();
        let tid = tree_id_parsed;
        let pid = person_id_parsed;
        let active = show_ancestors();
        async move {
            if !active {
                return Ok(vec![]);
            }
            let (Some(tid), Some(pid)) = (tid, pid) else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: "Invalid IDs".to_string(),
                });
            };
            api.get_ancestors(tid, pid, None).await
        }
    });

    // Fetch descendants (lazy: only when toggled on).
    let api_descendants = api.clone();
    let descendants_resource = use_resource(move || {
        let api = api_descendants.clone();
        let _tick = refresh();
        let tid = tree_id_parsed;
        let pid = person_id_parsed;
        let active = show_descendants();
        async move {
            if !active {
                return Ok(vec![]);
            }
            let (Some(tid), Some(pid)) = (tid, pid) else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: "Invalid IDs".to_string(),
                });
            };
            api.get_descendants(tid, pid, None).await
        }
    });

    // Fetch all persons in tree (for resolving IDs in ancestry charts).
    let api_all_persons = api.clone();
    let all_persons_resource = use_resource(move || {
        let api = api_all_persons.clone();
        let _tick = refresh();
        let tid = tree_id_parsed;
        let need = show_ancestors() || show_descendants();
        async move {
            if !need {
                return Ok(oxidgene_core::types::Connection::empty());
            }
            let Some(tid) = tid else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: "Invalid IDs".to_string(),
                });
            };
            api.list_persons(tid, Some(500), None).await
        }
    });

    // Fetch all person names in tree (for resolving names in ancestry charts).
    // We use the list_persons result to gather person IDs, then load names.
    // For simplicity, load names for each person found in the ancestry edges.
    // Build a name lookup for *all* persons in the tree — used by
    // family connections (parents, spouses, children, siblings) and
    // ancestry charts. Uses the snapshot endpoint (single cached HTTP
    // request) instead of the previous N+1 per-person approach.
    let api_all_names = api.clone();
    let all_names_resource = use_resource(move || {
        let api = api_all_names.clone();
        let _tick = refresh();
        let tid = tree_id_parsed;
        async move {
            let Some(tid) = tid else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: "Invalid IDs".to_string(),
                });
            };
            let snapshot = api.get_tree_snapshot(tid).await?;
            let mut name_map: HashMap<Uuid, Vec<oxidgene_core::types::PersonName>> = HashMap::new();
            for pn in snapshot.names {
                name_map.entry(pn.person_id).or_default().push(pn);
            }
            Ok(name_map)
        }
    });

    // Fetch tree snapshot for enriched events (cached — same request as above).
    let api_snap = api.clone();
    let snapshot_resource = use_resource(move || {
        let api = api_snap.clone();
        let _tick = refresh();
        let tid = tree_id_parsed;
        async move {
            let Some(tid) = tid else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: "Invalid tree ID".to_string(),
                });
            };
            api.get_tree_snapshot(tid).await
        }
    });

    // Fetch citations for this person.
    // Citations don't have a list-by-person endpoint, so we fetch all sources
    // and then list citations by source filtered to this person.
    // For now, we use a workaround: create citations with person_id set and
    // load them via the notes-style query pattern. But the REST API for citations
    // doesn't have a list endpoint. We'll track citations that were created
    // for this person by loading them via a helper resource.
    //
    // Since there's no list endpoint for citations, we'll handle citations
    // display through a dedicated resource that creates/deletes in-memory.
    // For the MVP, we show a create form and a list of person citations
    // stored in local state after creation.

    // Fetch tree info (for breadcrumb).
    let api_tree = api.clone();
    let tree_resource = use_resource(move || {
        let api = api_tree.clone();
        let _tick = refresh();
        let tid = tree_id_parsed;
        async move {
            let Some(tid) = tid else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: "Invalid tree ID".to_string(),
                });
            };
            api.get_tree(tid).await
        }
    });

    // Fetch families for family connections card.
    let api_families = api.clone();
    let families_resource = use_resource(move || {
        let api = api_families.clone();
        let _tick = refresh();
        let tid = tree_id_parsed;
        async move {
            let Some(tid) = tid else {
                return Ok::<_, crate::api::ApiError>((
                    Vec::<oxidgene_core::types::Family>::new(),
                    Vec::<(Uuid, oxidgene_core::types::FamilySpouse)>::new(),
                    Vec::<(Uuid, oxidgene_core::types::FamilyChild)>::new(),
                ));
            };
            let families = api
                .list_families(tid, Some(500), None)
                .await
                .unwrap_or_else(|_| oxidgene_core::types::Connection::empty());
            let mut all_spouses = Vec::new();
            let mut all_children = Vec::new();
            for edge in &families.edges {
                let fid = edge.node.id;
                if let Ok(spouses) = api.list_family_spouses(tid, fid).await {
                    for s in &spouses {
                        all_spouses.push((fid, s.clone()));
                    }
                }
                if let Ok(children) = api.list_family_children(tid, fid).await {
                    for c in &children {
                        all_children.push((fid, c.clone()));
                    }
                }
            }
            Ok((
                families
                    .edges
                    .into_iter()
                    .map(|e| e.node)
                    .collect::<Vec<_>>(),
                all_spouses,
                all_children,
            ))
        }
    });

    let tree_name_str = match &*tree_resource.read() {
        Some(Ok(tree)) => tree.name.clone(),
        _ => i18n.t("common.loading"),
    };

    // Derive display name from loaded names.
    let display_name = match &*names_resource.read() {
        Some(Ok(names)) => {
            let primary = names.iter().find(|n| n.is_primary).or(names.first());
            match primary {
                Some(name) => {
                    let dn = name.display_name();
                    if dn.is_empty() {
                        i18n.t("common.unnamed")
                    } else {
                        dn
                    }
                }
                None => i18n.t("common.unnamed"),
            }
        }
        _ => i18n.t("common.loading"),
    };

    // Helper: resolve place_id to place name.
    let place_name = |place_id: Uuid| -> String {
        let places_data = places_resource.read();
        match &*places_data {
            Some(Ok(conn)) => conn
                .edges
                .iter()
                .find(|e| e.node.id == place_id)
                .map(|e| e.node.name.clone())
                .unwrap_or_else(|| place_id.to_string()[..8].to_string()),
            _ => place_id.to_string()[..8].to_string(),
        }
    };

    // ── Handlers ─────────────────────────────────────────────────────

    // Delete person handler.
    let tree_id_nav = tree_id.clone();
    let api_del = api.clone();
    let on_confirm_delete = move |_| {
        let api = api_del.clone();
        let Some(tid) = tree_id_parsed else { return };
        let Some(pid) = person_id_parsed else { return };
        let tree_id_nav = tree_id_nav.clone();
        spawn(async move {
            match api.delete_person(tid, pid).await {
                Ok(_) => {
                    nav.push(Route::TreeDetail {
                        tree_id: tree_id_nav,
                        person: None,
                    });
                }
                Err(e) => {
                    delete_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    // Save sex handler.
    let api_sex = api.clone();
    let on_save_sex = move |_| {
        let api = api_sex.clone();
        let Some(tid) = tree_id_parsed else { return };
        let Some(pid) = person_id_parsed else { return };
        let sex_str = edit_sex_val();
        spawn(async move {
            let sex = parse_sex(&sex_str);
            let body = UpdatePersonBody { sex: Some(sex) };
            match api.update_person(tid, pid, &body).await {
                Ok(_) => {
                    editing_sex.set(false);
                    edit_sex_error.set(None);
                    refresh += 1;
                }
                Err(e) => {
                    edit_sex_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    // Create name handler.
    let api_create_name = api.clone();
    let on_create_name = move |_| {
        let api = api_create_name.clone();
        let Some(tid) = tree_id_parsed else { return };
        let Some(pid) = person_id_parsed else { return };
        let given = name_form_given().trim().to_string();
        let surname = name_form_surname().trim().to_string();
        let prefix = name_form_prefix().trim().to_string();
        let suffix = name_form_suffix().trim().to_string();
        let nickname = name_form_nickname().trim().to_string();
        let name_type_str = name_form_type();
        let is_primary = name_form_primary();
        spawn(async move {
            if given.is_empty() && surname.is_empty() {
                name_form_error.set(Some("Given names or surname is required".to_string()));
                return;
            }
            let body = CreatePersonNameBody {
                name_type: parse_name_type(&name_type_str),
                given_names: opt_str(&given),
                surname: opt_str(&surname),
                prefix: opt_str(&prefix),
                suffix: opt_str(&suffix),
                nickname: opt_str(&nickname),
                is_primary,
            };
            match api.create_person_name(tid, pid, &body).await {
                Ok(_) => {
                    show_name_form.set(false);
                    name_form_given.set(String::new());
                    name_form_surname.set(String::new());
                    name_form_prefix.set(String::new());
                    name_form_suffix.set(String::new());
                    name_form_nickname.set(String::new());
                    name_form_type.set("Birth".to_string());
                    name_form_primary.set(true);
                    name_form_error.set(None);
                    refresh += 1;
                }
                Err(e) => {
                    name_form_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    // Save name edit handler.
    let api_edit_name = api.clone();
    let on_save_name_edit = move |_| {
        let api = api_edit_name.clone();
        let Some(tid) = tree_id_parsed else { return };
        let Some(pid) = person_id_parsed else { return };
        let Some(nid) = editing_name_id() else { return };
        let given = edit_name_given().trim().to_string();
        let surname = edit_name_surname().trim().to_string();
        let prefix = edit_name_prefix().trim().to_string();
        let suffix = edit_name_suffix().trim().to_string();
        let nickname = edit_name_nickname().trim().to_string();
        let name_type_str = edit_name_type();
        let is_primary = edit_name_primary();
        spawn(async move {
            let body = UpdatePersonNameBody {
                name_type: Some(parse_name_type(&name_type_str)),
                given_names: Some(opt_str(&given)),
                surname: Some(opt_str(&surname)),
                prefix: Some(opt_str(&prefix)),
                suffix: Some(opt_str(&suffix)),
                nickname: Some(opt_str(&nickname)),
                is_primary: Some(is_primary),
            };
            match api.update_person_name(tid, pid, nid, &body).await {
                Ok(_) => {
                    editing_name_id.set(None);
                    edit_name_error.set(None);
                    refresh += 1;
                }
                Err(e) => {
                    edit_name_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    // Delete name handler.
    let api_del_name = api.clone();
    let on_confirm_delete_name = move |_| {
        let api = api_del_name.clone();
        let Some(tid) = tree_id_parsed else { return };
        let Some(pid) = person_id_parsed else { return };
        let Some(nid) = confirm_delete_name_id() else {
            return;
        };
        spawn(async move {
            match api.delete_person_name(tid, pid, nid).await {
                Ok(_) => {
                    confirm_delete_name_id.set(None);
                    delete_name_error.set(None);
                    refresh += 1;
                }
                Err(e) => {
                    delete_name_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    // Create event handler.
    let api_create_event = api.clone();
    let on_create_event = move |_| {
        let api = api_create_event.clone();
        let Some(tid) = tree_id_parsed else { return };
        let Some(pid) = person_id_parsed else { return };
        let event_type_str = event_form_type();
        let date = event_form_date().trim().to_string();
        let place_id_str = event_form_place_id();
        let desc = event_form_desc().trim().to_string();
        spawn(async move {
            let place_id = if place_id_str.is_empty() {
                None
            } else {
                place_id_str.parse::<Uuid>().ok()
            };
            let body = CreateEventBody {
                event_type: parse_event_type(&event_type_str),
                date_value: opt_str(&date),
                date_sort: None,
                place_id,
                person_id: Some(pid),
                family_id: None,
                description: opt_str(&desc),
            };
            match api.create_event(tid, &body).await {
                Ok(_) => {
                    show_event_form.set(false);
                    event_form_type.set("Birth".to_string());
                    event_form_date.set(String::new());
                    event_form_place_id.set(String::new());
                    event_form_desc.set(String::new());
                    event_form_error.set(None);
                    refresh += 1;
                }
                Err(e) => {
                    event_form_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    // Save event edit handler.
    let api_edit_event = api.clone();
    let on_save_event_edit = move |_| {
        let api = api_edit_event.clone();
        let Some(tid) = tree_id_parsed else { return };
        let Some(eid) = editing_event_id() else {
            return;
        };
        let event_type_str = edit_event_type();
        let date = edit_event_date().trim().to_string();
        let place_id_str = edit_event_place_id();
        let desc = edit_event_desc().trim().to_string();
        spawn(async move {
            let place_id = if place_id_str.is_empty() {
                None
            } else {
                place_id_str.parse::<Uuid>().ok()
            };
            let body = UpdateEventBody {
                event_type: Some(parse_event_type(&event_type_str)),
                date_value: Some(opt_str(&date)),
                place_id: Some(place_id),
                date_sort: None,
                description: Some(opt_str(&desc)),
            };
            match api.update_event(tid, eid, &body).await {
                Ok(_) => {
                    editing_event_id.set(None);
                    edit_event_error.set(None);
                    refresh += 1;
                }
                Err(e) => {
                    edit_event_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    // Delete event handler.
    let api_del_event = api.clone();
    let on_confirm_delete_event = move |_| {
        let api = api_del_event.clone();
        let Some(tid) = tree_id_parsed else { return };
        let Some(eid) = confirm_delete_event_id() else {
            return;
        };
        spawn(async move {
            match api.delete_event(tid, eid).await {
                Ok(_) => {
                    confirm_delete_event_id.set(None);
                    delete_event_error.set(None);
                    refresh += 1;
                }
                Err(e) => {
                    delete_event_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    // Create note handler.
    let api_create_note = api.clone();
    let on_create_note = move |_| {
        let api = api_create_note.clone();
        let Some(tid) = tree_id_parsed else { return };
        let Some(pid) = person_id_parsed else { return };
        let text = note_form_text().trim().to_string();
        spawn(async move {
            if text.is_empty() {
                note_form_error.set(Some("Note text is required".to_string()));
                return;
            }
            let body = CreateNoteBody {
                text,
                person_id: Some(pid),
                event_id: None,
                family_id: None,
                source_id: None,
            };
            match api.create_note(tid, &body).await {
                Ok(_) => {
                    show_note_form.set(false);
                    note_form_text.set(String::new());
                    note_form_error.set(None);
                    refresh += 1;
                }
                Err(e) => {
                    note_form_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    // Save note edit handler.
    let api_edit_note = api.clone();
    let on_save_note_edit = move |_| {
        let api = api_edit_note.clone();
        let Some(tid) = tree_id_parsed else { return };
        let Some(nid) = editing_note_id() else {
            return;
        };
        let text = edit_note_text().trim().to_string();
        spawn(async move {
            if text.is_empty() {
                edit_note_error.set(Some("Note text is required".to_string()));
                return;
            }
            let body = UpdateNoteBody { text: Some(text) };
            match api.update_note(tid, nid, &body).await {
                Ok(_) => {
                    editing_note_id.set(None);
                    edit_note_error.set(None);
                    refresh += 1;
                }
                Err(e) => {
                    edit_note_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    // Delete note handler.
    let api_del_note = api.clone();
    let on_confirm_delete_note = move |_| {
        let api = api_del_note.clone();
        let Some(tid) = tree_id_parsed else { return };
        let Some(nid) = confirm_delete_note_id() else {
            return;
        };
        spawn(async move {
            match api.delete_note(tid, nid).await {
                Ok(_) => {
                    confirm_delete_note_id.set(None);
                    delete_note_error.set(None);
                    refresh += 1;
                }
                Err(e) => {
                    delete_note_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    // Create citation handler.
    let api_create_citation = api.clone();
    let on_create_citation = move |_| {
        let api = api_create_citation.clone();
        let Some(tid) = tree_id_parsed else { return };
        let Some(pid) = person_id_parsed else { return };
        let source_id_str = citation_form_source_id();
        let page = citation_form_page().trim().to_string();
        let confidence_str = citation_form_confidence();
        let text = citation_form_text().trim().to_string();
        spawn(async move {
            let Ok(source_id) = source_id_str.parse::<Uuid>() else {
                citation_form_error.set(Some("Please select a source".to_string()));
                return;
            };
            let body = CreateCitationBody {
                source_id,
                person_id: Some(pid),
                event_id: None,
                family_id: None,
                page: opt_str(&page),
                confidence: parse_confidence(&confidence_str),
                text: opt_str(&text),
            };
            match api.create_citation(tid, &body).await {
                Ok(_) => {
                    show_citation_form.set(false);
                    citation_form_source_id.set(String::new());
                    citation_form_page.set(String::new());
                    citation_form_confidence.set("Medium".to_string());
                    citation_form_text.set(String::new());
                    citation_form_error.set(None);
                    refresh += 1;
                }
                Err(e) => {
                    citation_form_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    // Delete citation handler.
    let on_confirm_delete_citation = move |_| {
        let api = api.clone();
        let Some(tid) = tree_id_parsed else { return };
        let Some(cid) = confirm_delete_citation_id() else {
            return;
        };
        spawn(async move {
            match api.delete_citation(tid, cid).await {
                Ok(_) => {
                    confirm_delete_citation_id.set(None);
                    delete_citation_error.set(None);
                    refresh += 1;
                }
                Err(e) => {
                    delete_citation_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    // ── Render ────────────────────────────────────────────────────────

    // Build family connections for the current person.
    let family_connections = {
        let pid = person_id_parsed;
        match (&*families_resource.read(), pid) {
            (Some(Ok((_families, all_spouses, all_children))), Some(pid)) => {
                // Families where this person is a spouse
                let spouse_family_ids: Vec<Uuid> = all_spouses
                    .iter()
                    .filter(|(_fid, s)| s.person_id == pid)
                    .map(|(fid, _)| *fid)
                    .collect();

                // For each such family, find the other spouse(s) and children
                let mut partner_ids: Vec<Uuid> = Vec::new();
                let mut child_ids: Vec<Uuid> = Vec::new();
                for fid in &spouse_family_ids {
                    for (_f, s) in all_spouses.iter() {
                        if _f == fid && s.person_id != pid {
                            partner_ids.push(s.person_id);
                        }
                    }
                    for (_f, c) in all_children.iter() {
                        if _f == fid {
                            child_ids.push(c.person_id);
                        }
                    }
                }

                // Families where this person is a child → find parents
                let child_family_ids: Vec<Uuid> = all_children
                    .iter()
                    .filter(|(_fid, c)| c.person_id == pid)
                    .map(|(fid, _)| *fid)
                    .collect();

                let mut parent_ids: Vec<Uuid> = Vec::new();
                let mut sibling_ids: Vec<Uuid> = Vec::new();
                for fid in &child_family_ids {
                    for (_f, s) in all_spouses.iter() {
                        if _f == fid {
                            parent_ids.push(s.person_id);
                        }
                    }
                    for (_f, c) in all_children.iter() {
                        if _f == fid && c.person_id != pid {
                            sibling_ids.push(c.person_id);
                        }
                    }
                }

                Some((parent_ids, partner_ids, child_ids, sibling_ids))
            }
            _ => None,
        }
    };

    // Helper to resolve person name from all_names or names_resource
    let resolve_person_name = |pid: Uuid| -> String {
        let names_data = all_names_resource.read();
        if let Some(Ok(name_map)) = &*names_data {
            return resolve_name(pid, name_map);
        }
        i18n.t("common.unknown")
    };

    // ── Build enriched event list ───────────────────────────────────
    //
    // Combines three sources:
    //   1. Individual events (birth, death, occupation…)
    //   2. Conjugal family events (marriage, divorce…)
    //   3. Parental family events (parent death, sibling birth…)
    let enriched_events: Vec<EnrichedEvent> = {
        let snap_data = snapshot_resource.read();
        let fam_data = families_resource.read();
        let pid = person_id_parsed;

        match (&*snap_data, &*fam_data, pid) {
            (Some(Ok(snapshot)), Some(Ok((_families, all_spouses, all_children))), Some(pid)) => {
                // Index events by person_id and family_id.
                let mut events_by_person: HashMap<Uuid, Vec<&DomainEvent>> = HashMap::new();
                let mut events_by_family: HashMap<Uuid, Vec<&DomainEvent>> = HashMap::new();
                for e in snapshot.events.iter() {
                    if e.deleted_at.is_some() {
                        continue;
                    }
                    if let Some(epid) = e.person_id {
                        events_by_person.entry(epid).or_default().push(e);
                    }
                    if let Some(fid) = e.family_id {
                        events_by_family.entry(fid).or_default().push(e);
                    }
                }

                // Derive family IDs (same logic as family_connections).
                let spouse_family_ids: Vec<Uuid> = all_spouses
                    .iter()
                    .filter(|(_fid, s)| s.person_id == pid)
                    .map(|(fid, _)| *fid)
                    .collect();
                let child_family_ids: Vec<Uuid> = all_children
                    .iter()
                    .filter(|(_fid, c)| c.person_id == pid)
                    .map(|(fid, _)| *fid)
                    .collect();

                let mut result: Vec<EnrichedEvent> = Vec::new();
                let mut seen_ids: std::collections::HashSet<Uuid> =
                    std::collections::HashSet::new();

                // 1. Individual events.
                if let Some(person_events) = events_by_person.get(&pid) {
                    for &e in person_events {
                        if seen_ids.insert(e.id) {
                            result.push(EnrichedEvent {
                                event: e.clone(),
                                origin: EventOrigin::Individual,
                                context: None,
                            });
                        }
                    }
                }

                // 2. Conjugal family events (from families where person is spouse).
                for fid in &spouse_family_ids {
                    // Find partner name for context.
                    let partner_name = all_spouses
                        .iter()
                        .find(|(f, s)| f == fid && s.person_id != pid)
                        .map(|(_, s)| resolve_person_name(s.person_id));

                    if let Some(fam_events) = events_by_family.get(fid) {
                        for &e in fam_events {
                            if seen_ids.insert(e.id) {
                                result.push(EnrichedEvent {
                                    event: e.clone(),
                                    origin: EventOrigin::ConjugalFamily,
                                    context: partner_name.clone(),
                                });
                            }
                        }
                    }

                    // Major individual events of children (birth, death, baptism, burial).
                    for (f, c) in all_children.iter() {
                        if *f != *fid {
                            continue;
                        }
                        let child_name = resolve_person_name(c.person_id);
                        if let Some(child_events) = events_by_person.get(&c.person_id) {
                            for &e in child_events {
                                if (e.event_type == EventType::Birth
                                    || e.event_type == EventType::Death
                                    || e.event_type == EventType::Baptism
                                    || e.event_type == EventType::Burial)
                                    && seen_ids.insert(e.id)
                                {
                                    result.push(EnrichedEvent {
                                        event: e.clone(),
                                        origin: EventOrigin::ChildFamily,
                                        context: Some(child_name.clone()),
                                    });
                                }
                            }
                        }
                    }
                }

                // 3. Parental family events (from families where person is child).
                for fid in &child_family_ids {
                    // Family-level events of parental family.
                    if let Some(fam_events) = events_by_family.get(fid) {
                        for &e in fam_events {
                            if seen_ids.insert(e.id) {
                                result.push(EnrichedEvent {
                                    event: e.clone(),
                                    origin: EventOrigin::ParentalFamily,
                                    context: None,
                                });
                            }
                        }
                    }

                    // Major individual events of parents (death, burial).
                    for (f, s) in all_spouses.iter() {
                        if f != fid {
                            continue;
                        }
                        let parent_name = resolve_person_name(s.person_id);
                        if let Some(parent_events) = events_by_person.get(&s.person_id) {
                            for &e in parent_events {
                                if (e.event_type == EventType::Death
                                    || e.event_type == EventType::Burial)
                                    && seen_ids.insert(e.id)
                                {
                                    result.push(EnrichedEvent {
                                        event: e.clone(),
                                        origin: EventOrigin::ParentalFamily,
                                        context: Some(parent_name.clone()),
                                    });
                                }
                            }
                        }
                    }

                    // Major individual events of siblings (birth, death, baptism, burial).
                    for (f, c) in all_children.iter() {
                        if f != fid || c.person_id == pid {
                            continue;
                        }
                        let sib_name = resolve_person_name(c.person_id);
                        if let Some(sib_events) = events_by_person.get(&c.person_id) {
                            for &e in sib_events {
                                if (e.event_type == EventType::Birth
                                    || e.event_type == EventType::Death
                                    || e.event_type == EventType::Baptism
                                    || e.event_type == EventType::Burial)
                                    && seen_ids.insert(e.id)
                                {
                                    result.push(EnrichedEvent {
                                        event: e.clone(),
                                        origin: EventOrigin::ParentalFamily,
                                        context: Some(sib_name.clone()),
                                    });
                                }
                            }
                        }
                    }
                }

                // Sort by date.
                result.sort_by(|a, b| a.event.date_sort.cmp(&b.event.date_sort));
                result
            }
            _ => Vec::new(),
        }
    };

    rsx! {
        div { class: "page-content",
        // Breadcrumb
        div { class: "pd-breadcrumb",
            Link { to: Route::Home {}, {i18n.t("tree.breadcrumb_trees")} }
            span { class: "pd-breadcrumb-sep", " / " }
            Link {
                to: Route::TreeDetail { tree_id: tree_id.clone(), person: None },
                "{tree_name_str}"
            }
            span { class: "pd-breadcrumb-sep", " / " }
            span { class: "pd-breadcrumb-current", "{display_name}" }
        }

        // Action buttons
        div { class: "pd-action-bar",
            Link {
                to: Route::TreeDetail { tree_id: tree_id.clone(), person: Some(person_id.clone()) },
                class: "btn btn-outline",
                {i18n.t("person.view_in_tree")}
            }
        }

        // Delete person confirmation dialog
        if confirm_delete() {
            ConfirmDialog {
                title: i18n.t("confirm.delete_person.title"),
                message: i18n.t_args("confirm.delete_person.message_name", &[("name", &display_name)]),
                confirm_label: i18n.t("common.delete"),
                confirm_class: "btn btn-danger",
                error: delete_error(),
                on_confirm: move |_| on_confirm_delete(()),
                on_cancel: move |_| {
                    confirm_delete.set(false);
                    delete_error.set(None);
                },
            }
        }

        // Delete name confirmation dialog
        if confirm_delete_name_id().is_some() {
            ConfirmDialog {
                title: i18n.t("confirm.delete_name.title"),
                message: i18n.t("confirm.delete_name.message"),
                confirm_label: i18n.t("common.delete"),
                confirm_class: "btn btn-danger",
                error: delete_name_error(),
                on_confirm: move |_| on_confirm_delete_name(()),
                on_cancel: move |_| {
                    confirm_delete_name_id.set(None);
                    delete_name_error.set(None);
                },
            }
        }

        // Delete event confirmation dialog
        if confirm_delete_event_id().is_some() {
            ConfirmDialog {
                title: i18n.t("confirm.delete_event.title"),
                message: i18n.t("confirm.delete_event.message"),
                confirm_label: i18n.t("common.delete"),
                confirm_class: "btn btn-danger",
                error: delete_event_error(),
                on_confirm: move |_| on_confirm_delete_event(()),
                on_cancel: move |_| {
                    confirm_delete_event_id.set(None);
                    delete_event_error.set(None);
                },
            }
        }

        // Delete note confirmation dialog
        if confirm_delete_note_id().is_some() {
            ConfirmDialog {
                title: i18n.t("confirm.delete_note.title"),
                message: i18n.t("confirm.delete_note.message"),
                confirm_label: i18n.t("common.delete"),
                confirm_class: "btn btn-danger",
                error: delete_note_error(),
                on_confirm: move |_| on_confirm_delete_note(()),
                on_cancel: move |_| {
                    confirm_delete_note_id.set(None);
                    delete_note_error.set(None);
                },
            }
        }

        // Delete citation confirmation dialog
        if confirm_delete_citation_id().is_some() {
            ConfirmDialog {
                title: i18n.t("confirm.delete_citation.title"),
                message: i18n.t("confirm.delete_citation.message"),
                confirm_label: i18n.t("common.delete"),
                confirm_class: "btn btn-danger",
                error: delete_citation_error(),
                on_confirm: move |_| on_confirm_delete_citation(()),
                on_cancel: move |_| {
                    confirm_delete_citation_id.set(None);
                    delete_citation_error.set(None);
                },
            }
        }

        // Person header
        match &*person_resource.read() {
            Some(Ok(person)) => {
                let person_sex = person.sex;
                let sex_str = match person_sex {
                    Sex::Male => i18n.t("sex.male"),
                    Sex::Female => i18n.t("sex.female"),
                    Sex::Unknown => i18n.t("sex.unknown"),
                };
                rsx! {
                    div { class: "page-header",
                        div {
                            h1 { "{display_name}" }
                            div { style: "display: flex; align-items: center; gap: 8px; margin-top: 4px;",
                                if editing_sex() {
                                    select {
                                        value: "{edit_sex_val}",
                                        oninput: move |e: Event<FormData>| edit_sex_val.set(e.value()),
                                        option { value: "Unknown", {i18n.t("sex.unknown")} }
                                        option { value: "Male", {i18n.t("sex.male")} }
                                        option { value: "Female", {i18n.t("sex.female")} }
                                    }
                                    button {
                                        class: "btn btn-primary btn-sm",
                                        onclick: on_save_sex,
                                        {i18n.t("common.save")}
                                    }
                                    button {
                                        class: "btn btn-outline btn-sm",
                                        onclick: move |_| {
                                            editing_sex.set(false);
                                            edit_sex_error.set(None);
                                        },
                                        {i18n.t("common.cancel")}
                                    }
                                    if let Some(err) = edit_sex_error() {
                                        span { class: "error-msg", style: "margin: 0; padding: 4px 8px; font-size: 0.8rem;", "{err}" }
                                    }
                                } else {
                                    span { class: "badge", "{sex_str}" }
                                    button {
                                        class: "btn btn-outline btn-sm",
                                        onclick: move |_| {
                                            edit_sex_val.set(format!("{:?}", person_sex));
                                            editing_sex.set(true);
                                        },
                                        {i18n.t("person.edit_sex")}
                                    }
                                }
                                span { class: "text-muted", style: "font-size: 0.85rem;",
                                    "ID: "
                                    {person.id.to_string().chars().take(8).collect::<String>()}
                                    "..."
                                }
                            }
                        }
                        div { style: "display: flex; gap: 8px;",
                            button {
                                class: "btn btn-danger",
                                onclick: move |_| {
                                    confirm_delete.set(true);
                                    delete_error.set(None);
                                },
                                {i18n.t("common.delete")}
                            }
                            button {
                                class: "btn btn-outline",
                                onclick: move |_| refresh += 1,
                                {i18n.t("person.refresh")}
                            }
                        }
                    }
                }
            },
            Some(Err(e)) => rsx! {
                div { class: "error-msg", {i18n.t_args("person.load_error", &[("error", &e.to_string())])} }
            },
            None => rsx! {
                div { class: "loading", {i18n.t("person.loading")} }
            },
        }

        // ── Family connections section ──────────────────────────────
        if let Some((parent_ids, partner_ids, child_ids, sibling_ids)) = &family_connections {
            div { class: "card", style: "margin-bottom: 24px;",
                h2 { style: "font-size: 1.1rem; margin-bottom: 12px;", {i18n.t("person.family_connections")} }

                if !parent_ids.is_empty() {
                    div { class: "pd-fc-section",
                        h3 { class: "pd-fc-label", {i18n.t("person.parents")} }
                        for pid in parent_ids.iter() {
                            { let pid = *pid; let tid = tree_id.clone(); rsx! {
                                Link {
                                    to: Route::PersonDetail { tree_id: tid, person_id: pid.to_string() },
                                    class: "pd-fc-link",
                                    "{resolve_person_name(pid)}"
                                }
                            }}
                        }
                    }
                }

                if !partner_ids.is_empty() {
                    div { class: "pd-fc-section",
                        h3 { class: "pd-fc-label", {i18n.t("person.spouses_partners")} }
                        for pid in partner_ids.iter() {
                            { let pid = *pid; let tid = tree_id.clone(); rsx! {
                                Link {
                                    to: Route::PersonDetail { tree_id: tid, person_id: pid.to_string() },
                                    class: "pd-fc-link",
                                    "{resolve_person_name(pid)}"
                                }
                            }}
                        }
                    }
                }

                if !child_ids.is_empty() {
                    div { class: "pd-fc-section",
                        h3 { class: "pd-fc-label", {i18n.t("person.children")} }
                        for pid in child_ids.iter() {
                            { let pid = *pid; let tid = tree_id.clone(); rsx! {
                                Link {
                                    to: Route::PersonDetail { tree_id: tid, person_id: pid.to_string() },
                                    class: "pd-fc-link",
                                    "{resolve_person_name(pid)}"
                                }
                            }}
                        }
                    }
                }

                if !sibling_ids.is_empty() {
                    div { class: "pd-fc-section",
                        h3 { class: "pd-fc-label", {i18n.t("person.siblings")} }
                        for pid in sibling_ids.iter() {
                            { let pid = *pid; let tid = tree_id.clone(); rsx! {
                                Link {
                                    to: Route::PersonDetail { tree_id: tid, person_id: pid.to_string() },
                                    class: "pd-fc-link",
                                    "{resolve_person_name(pid)}"
                                }
                            }}
                        }
                    }
                }

                if parent_ids.is_empty() && partner_ids.is_empty() && child_ids.is_empty() && sibling_ids.is_empty() {
                    div { class: "empty-state",
                        p { {i18n.t("person.no_family_connections")} }
                    }
                }
            }
        }

        // ── Names section ────────────────────────────────────────────
        div { class: "card", style: "margin-bottom: 24px;",
            div { class: "section-header",
                h2 { style: "font-size: 1.1rem;", {i18n.t("person.names_section")} }
                button {
                    class: "btn btn-primary btn-sm",
                    onclick: move |_| show_name_form.toggle(),
                    if show_name_form() { {i18n.t("common.cancel")} } else { {i18n.t("person_form.add_name")} }
                }
            }

            // Add name form
            if show_name_form() {
                div { style: "margin-bottom: 16px; padding: 16px; background: var(--color-bg); border-radius: var(--radius);",
                    h3 { style: "margin-bottom: 12px; font-size: 0.95rem;", {i18n.t("person.new_name")} }

                    if let Some(err) = name_form_error() {
                        div { class: "error-msg", "{err}" }
                    }

                    div { class: "form-row",
                        div { class: "form-group",
                            label { {i18n.t("person_form.name_type")} }
                            select {
                                value: "{name_form_type}",
                                oninput: move |e: Event<FormData>| name_form_type.set(e.value()),
                                option { value: "Birth", {i18n.t("name_type_short.birth")} }
                                option { value: "Married", {i18n.t("name_type_short.married")} }
                                option { value: "AlsoKnownAs", {i18n.t("name_type_short.also_known_as")} }
                                option { value: "Maiden", {i18n.t("name_type_short.maiden")} }
                                option { value: "Religious", {i18n.t("name_type_short.religious")} }
                                option { value: "Other", {i18n.t("name_type_short.other")} }
                            }
                        }
                        div { class: "form-group",
                            label { {i18n.t("person_form.primary")} }
                            select {
                                value: if name_form_primary() { "true" } else { "false" },
                                oninput: move |e: Event<FormData>| name_form_primary.set(e.value() == "true"),
                                option { value: "true", {i18n.t("common.yes")} }
                                option { value: "false", {i18n.t("common.no")} }
                            }
                        }
                    }
                    div { class: "form-row",
                        div { class: "form-group",
                            label { {i18n.t("person_form.given_names")} }
                            input {
                                r#type: "text",
                                placeholder: "{i18n.t(\"person_form.given_placeholder\")}",
                                value: "{name_form_given}",
                                oninput: move |e: Event<FormData>| name_form_given.set(e.value()),
                            }
                        }
                        div { class: "form-group",
                            label { {i18n.t("person_form.surname")} }
                            input {
                                r#type: "text",
                                placeholder: "{i18n.t(\"person_form.surname_placeholder\")}",
                                value: "{name_form_surname}",
                                oninput: move |e: Event<FormData>| name_form_surname.set(e.value()),
                            }
                        }
                    }
                    div { class: "form-row",
                        div { class: "form-group",
                            label { {i18n.t("person_form.prefix")} }
                            input {
                                r#type: "text",
                                placeholder: "{i18n.t(\"person_form.prefix_placeholder\")}",
                                value: "{name_form_prefix}",
                                oninput: move |e: Event<FormData>| name_form_prefix.set(e.value()),
                            }
                        }
                        div { class: "form-group",
                            label { {i18n.t("person_form.suffix")} }
                            input {
                                r#type: "text",
                                placeholder: "{i18n.t(\"person_form.suffix_placeholder\")}",
                                value: "{name_form_suffix}",
                                oninput: move |e: Event<FormData>| name_form_suffix.set(e.value()),
                            }
                        }
                        div { class: "form-group",
                            label { {i18n.t("person_form.nickname")} }
                            input {
                                r#type: "text",
                                placeholder: "{i18n.t(\"person_form.nickname_placeholder\")}",
                                value: "{name_form_nickname}",
                                oninput: move |e: Event<FormData>| name_form_nickname.set(e.value()),
                            }
                        }
                    }
                    button { class: "btn btn-primary btn-sm", onclick: on_create_name, {i18n.t("person.create_name")} }
                }
            }

            match &*names_resource.read() {
                Some(Ok(names)) => rsx! {
                    if names.is_empty() {
                        div { class: "empty-state",
                            p { {i18n.t("person.no_names")} }
                        }
                    } else {
                        div { class: "table-wrapper",
                            table {
                                thead {
                                    tr {
                                        th { {i18n.t("person_form.type")} }
                                        th { {i18n.t("person_form.given_names")} }
                                        th { {i18n.t("person_form.surname")} }
                                        th { {i18n.t("person_form.primary")} }
                                        th { style: "width: 140px;", {i18n.t("person.actions")} }
                                    }
                                }
                                tbody {
                                    for name in names.iter() {
                                        {
                                            let nid = name.id;
                                            let is_editing = editing_name_id() == Some(nid);
                                            let nt = format!("{:?}", name.name_type);
                                            let gn = name.given_names.clone().unwrap_or_default();
                                            let sn = name.surname.clone().unwrap_or_default();
                                            let pfx = name.prefix.clone().unwrap_or_default();
                                            let sfx = name.suffix.clone().unwrap_or_default();
                                            let nick = name.nickname.clone().unwrap_or_default();
                                            let prim = name.is_primary;
                                            if is_editing {
                                                rsx! {
                                                    tr {
                                                        td { colspan: 5,
                                                            div { style: "padding: 8px; background: var(--color-bg); border-radius: var(--radius);",
                                                                if let Some(err) = edit_name_error() {
                                                                    div { class: "error-msg", "{err}" }
                                                                }
                                                                div { class: "form-row",
                                                                    div { class: "form-group",
                                                                        label { {i18n.t("person_form.name_type")} }
                                                                        select {
                                                                            value: "{edit_name_type}",
                                                                            oninput: move |e: Event<FormData>| edit_name_type.set(e.value()),
                                                                            option { value: "Birth", {i18n.t("name_type_short.birth")} }
                                                                            option { value: "Married", {i18n.t("name_type_short.married")} }
                                                                            option { value: "AlsoKnownAs", {i18n.t("name_type_short.also_known_as")} }
                                                                            option { value: "Maiden", {i18n.t("name_type_short.maiden")} }
                                                                            option { value: "Religious", {i18n.t("name_type_short.religious")} }
                                                                            option { value: "Other", {i18n.t("name_type_short.other")} }
                                                                        }
                                                                    }
                                                                    div { class: "form-group",
                                                                        label { {i18n.t("person_form.primary")} }
                                                                        select {
                                                                            value: if edit_name_primary() { "true" } else { "false" },
                                                                            oninput: move |e: Event<FormData>| edit_name_primary.set(e.value() == "true"),
                                                                            option { value: "true", {i18n.t("common.yes")} }
                                                                            option { value: "false", {i18n.t("common.no")} }
                                                                        }
                                                                    }
                                                                }
                                                                div { class: "form-row",
                                                                    div { class: "form-group",
                                                                        label { {i18n.t("person_form.given_names")} }
                                                                        input {
                                                                            r#type: "text",
                                                                            value: "{edit_name_given}",
                                                                            oninput: move |e: Event<FormData>| edit_name_given.set(e.value()),
                                                                        }
                                                                    }
                                                                    div { class: "form-group",
                                                                        label { {i18n.t("person_form.surname")} }
                                                                        input {
                                                                            r#type: "text",
                                                                            value: "{edit_name_surname}",
                                                                            oninput: move |e: Event<FormData>| edit_name_surname.set(e.value()),
                                                                        }
                                                                    }
                                                                }
                                                                div { class: "form-row",
                                                                    div { class: "form-group",
                                                                        label { {i18n.t("person_form.prefix")} }
                                                                        input {
                                                                            r#type: "text",
                                                                            value: "{edit_name_prefix}",
                                                                            oninput: move |e: Event<FormData>| edit_name_prefix.set(e.value()),
                                                                        }
                                                                    }
                                                                    div { class: "form-group",
                                                                        label { {i18n.t("person_form.suffix")} }
                                                                        input {
                                                                            r#type: "text",
                                                                            value: "{edit_name_suffix}",
                                                                            oninput: move |e: Event<FormData>| edit_name_suffix.set(e.value()),
                                                                        }
                                                                    }
                                                                    div { class: "form-group",
                                                                        label { {i18n.t("person_form.nickname")} }
                                                                        input {
                                                                            r#type: "text",
                                                                            value: "{edit_name_nickname}",
                                                                            oninput: move |e: Event<FormData>| edit_name_nickname.set(e.value()),
                                                                        }
                                                                    }
                                                                }
                                                                div { style: "display: flex; gap: 8px;",
                                                                    button {
                                                                        class: "btn btn-primary btn-sm",
                                                                        onclick: on_save_name_edit.clone(),
                                                                        {i18n.t("common.save")}
                                                                    }
                                                                    button {
                                                                        class: "btn btn-outline btn-sm",
                                                                        onclick: move |_| {
                                                                            editing_name_id.set(None);
                                                                            edit_name_error.set(None);
                                                                        },
                                                                        {i18n.t("common.cancel")}
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            } else {
                                                rsx! {
                                                    tr {
                                                        td {
                                                            span { class: "badge", {format!("{:?}", name.name_type)} }
                                                        }
                                                        td { {name.given_names.as_deref().unwrap_or("--")} }
                                                        td { {name.surname.as_deref().unwrap_or("--")} }
                                                        td {
                                                            if name.is_primary { {i18n.t("common.yes")} } else { {i18n.t("common.no")} }
                                                        }
                                                        td {
                                                            div { style: "display: flex; gap: 4px;",
                                                                button {
                                                                    class: "btn btn-outline btn-sm",
                                                                    onclick: move |_| {
                                                                        editing_name_id.set(Some(nid));
                                                                        edit_name_type.set(nt.clone());
                                                                        edit_name_given.set(gn.clone());
                                                                        edit_name_surname.set(sn.clone());
                                                                        edit_name_prefix.set(pfx.clone());
                                                                        edit_name_suffix.set(sfx.clone());
                                                                        edit_name_nickname.set(nick.clone());
                                                                        edit_name_primary.set(prim);
                                                                        edit_name_error.set(None);
                                                                    },
                                                                    {i18n.t("common.edit")}
                                                                }
                                                                button {
                                                                    class: "btn btn-danger btn-sm",
                                                                    onclick: move |_| {
                                                                        confirm_delete_name_id.set(Some(nid));
                                                                        delete_name_error.set(None);
                                                                    },
                                                                    {i18n.t("common.delete")}
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
                Some(Err(e)) => rsx! {
                    div { class: "error-msg", {i18n.t_args("person.load_names_error", &[("error", &e.to_string())])} }
                },
                None => rsx! {
                    div { class: "loading", {i18n.t("person.loading_names")} }
                },
            }
        }

        // ── Events section ───────────────────────────────────────────
        div { class: "card", style: "margin-bottom: 24px;",
            div { class: "section-header",
                h2 { style: "font-size: 1.1rem;", {i18n.t("person.events_section")} }
                button {
                    class: "btn btn-primary btn-sm",
                    onclick: move |_| show_event_form.toggle(),
                    if show_event_form() { {i18n.t("common.cancel")} } else { {i18n.t("person_form.add_event")} }
                }
            }

            // Add event form
            if show_event_form() {
                div { style: "margin-bottom: 16px; padding: 16px; background: var(--color-bg); border-radius: var(--radius);",
                    h3 { style: "margin-bottom: 12px; font-size: 0.95rem;", {i18n.t("person.new_event")} }

                    if let Some(err) = event_form_error() {
                        div { class: "error-msg", "{err}" }
                    }

                    div { class: "form-row",
                        div { class: "form-group",
                            label { {i18n.t("person.event_type")} }
                            {event_type_select("{event_form_type}", move |e: Event<FormData>| event_form_type.set(e.value()), &i18n)}
                        }
                        div { class: "form-group",
                            label { {i18n.t("person_form.date")} }
                            input {
                                r#type: "text",
                                placeholder: "{i18n.t(\"person_form.date_placeholder\")}",
                                value: "{event_form_date}",
                                oninput: move |e: Event<FormData>| event_form_date.set(e.value()),
                            }
                        }
                    }
                    div { class: "form-row",
                        div { class: "form-group",
                            label { {i18n.t("person_form.place")} }
                            {place_select_widget(&places_resource, "{event_form_place_id}", move |e: Event<FormData>| event_form_place_id.set(e.value()), &i18n)}
                        }
                        div { class: "form-group",
                            label { {i18n.t("person_form.description")} }
                            input {
                                r#type: "text",
                                placeholder: "{i18n.t(\"citation.optional_desc\")}",
                                value: "{event_form_desc}",
                                oninput: move |e: Event<FormData>| event_form_desc.set(e.value()),
                            }
                        }
                    }
                    button { class: "btn btn-primary btn-sm", onclick: on_create_event, {i18n.t("person.create_event")} }
                }
            }

            match &*events_resource.read() {
                Some(Ok(_conn)) => rsx! {
                    if enriched_events.is_empty() {
                        div { class: "empty-state",
                            p { {i18n.t("person.no_events")} }
                        }
                    } else {
                        div { class: "table-wrapper",
                            table {
                                thead {
                                    tr {
                                        th { {i18n.t("person_form.type")} }
                                        th { {i18n.t("person_form.date")} }
                                        th { {i18n.t("person_form.place")} }
                                        th { {i18n.t("person_form.description")} }
                                        th { {i18n.t("person.origin")} }
                                        th { style: "width: 140px;", {i18n.t("person.actions")} }
                                    }
                                }
                                tbody {
                                    for ee in enriched_events.iter() {
                                        {
                                            let event = &ee.event;
                                            let eid = event.id;
                                            let is_own = ee.origin == EventOrigin::Individual;
                                            let is_editing = is_own && editing_event_id() == Some(eid);
                                            let et = format!("{:?}", event.event_type);
                                            let dv = event.date_value.clone().unwrap_or_default();
                                            let desc = event.description.clone().unwrap_or_default();
                                            let pid_str = event.place_id.map(|p| p.to_string()).unwrap_or_default();
                                            let place_display = event.place_id.map(&place_name).unwrap_or_else(|| "--".to_string());

                                            // Origin label.
                                            let origin_label = match &ee.origin {
                                                EventOrigin::Individual => i18n.t("person.origin_individual"),
                                                EventOrigin::ConjugalFamily => i18n.t("person.origin_conjugal"),
                                                EventOrigin::ChildFamily => i18n.t("person.origin_child"),
                                                EventOrigin::ParentalFamily => i18n.t("person.origin_parental"),
                                            };
                                            let origin_display = if let Some(ref ctx) = ee.context {
                                                format!("{origin_label} ({ctx})")
                                            } else {
                                                origin_label
                                            };

                                            if is_editing {
                                                rsx! {
                                                    tr {
                                                        td { colspan: 6,
                                                            div { style: "padding: 8px; background: var(--color-bg); border-radius: var(--radius);",
                                                                if let Some(err) = edit_event_error() {
                                                                    div { class: "error-msg", "{err}" }
                                                                }
                                                                div { class: "form-row",
                                                                    div { class: "form-group",
                                                                        label { {i18n.t("person.event_type")} }
                                                                        {event_type_select("{edit_event_type}", move |e: Event<FormData>| edit_event_type.set(e.value()), &i18n)}
                                                                    }
                                                                    div { class: "form-group",
                                                                        label { {i18n.t("person_form.date")} }
                                                                        input {
                                                                            r#type: "text",
                                                                            value: "{edit_event_date}",
                                                                            oninput: move |e: Event<FormData>| edit_event_date.set(e.value()),
                                                                        }
                                                                    }
                                                                }
                                                                div { class: "form-row",
                                                                    div { class: "form-group",
                                                                        label { {i18n.t("person_form.place")} }
                                                                        {place_select_widget(&places_resource, "{edit_event_place_id}", move |e: Event<FormData>| edit_event_place_id.set(e.value()), &i18n)}
                                                                    }
                                                                    div { class: "form-group",
                                                                        label { {i18n.t("person_form.description")} }
                                                                        input {
                                                                            r#type: "text",
                                                                            value: "{edit_event_desc}",
                                                                            oninput: move |e: Event<FormData>| edit_event_desc.set(e.value()),
                                                                        }
                                                                    }
                                                                }
                                                                div { style: "display: flex; gap: 8px;",
                                                                    button {
                                                                        class: "btn btn-primary btn-sm",
                                                                        onclick: on_save_event_edit.clone(),
                                                                        {i18n.t("common.save")}
                                                                    }
                                                                    button {
                                                                        class: "btn btn-outline btn-sm",
                                                                        onclick: move |_| {
                                                                            editing_event_id.set(None);
                                                                            edit_event_error.set(None);
                                                                        },
                                                                        {i18n.t("common.cancel")}
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            } else {
                                                rsx! {
                                                    tr {
                                                        td {
                                                            span { class: "badge", {format!("{:?}", event.event_type)} }
                                                        }
                                                        td {
                                                            {event.date_value.as_deref().unwrap_or("--")}
                                                        }
                                                        td {
                                                            "{place_display}"
                                                        }
                                                        td { class: "text-muted",
                                                            {event.description.as_deref().unwrap_or("--")}
                                                        }
                                                        td {
                                                            span { class: "badge badge-origin",
                                                                "{origin_display}"
                                                            }
                                                        }
                                                        td {
                                                            if is_own {
                                                                div { style: "display: flex; gap: 4px;",
                                                                    button {
                                                                        class: "btn btn-outline btn-sm",
                                                                        onclick: move |_| {
                                                                            editing_event_id.set(Some(eid));
                                                                            edit_event_type.set(et.clone());
                                                                            edit_event_date.set(dv.clone());
                                                                            edit_event_place_id.set(pid_str.clone());
                                                                            edit_event_desc.set(desc.clone());
                                                                            edit_event_error.set(None);
                                                                        },
                                                                        {i18n.t("common.edit")}
                                                                    }
                                                                    button {
                                                                        class: "btn btn-danger btn-sm",
                                                                        onclick: move |_| {
                                                                            confirm_delete_event_id.set(Some(eid));
                                                                            delete_event_error.set(None);
                                                                        },
                                                                        {i18n.t("common.delete")}
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
                Some(Err(e)) => rsx! {
                    div { class: "error-msg", {i18n.t_args("person.load_events_error", &[("error", &e.to_string())])} }
                },
                None => rsx! {
                    div { class: "loading", {i18n.t("person.loading_events")} }
                },
            }
        }

        // ── Notes section ────────────────────────────────────────────
        div { class: "card", style: "margin-bottom: 24px;",
            div { class: "section-header",
                h2 { style: "font-size: 1.1rem;", {i18n.t("person.notes_section")} }
                button {
                    class: "btn btn-primary btn-sm",
                    onclick: move |_| show_note_form.toggle(),
                    if show_note_form() { {i18n.t("common.cancel")} } else { {i18n.t("person.add_note")} }
                }
            }

            // Add note form
            if show_note_form() {
                div { style: "margin-bottom: 16px; padding: 16px; background: var(--color-bg); border-radius: var(--radius);",
                    h3 { style: "margin-bottom: 12px; font-size: 0.95rem;", {i18n.t("person.new_note")} }

                    if let Some(err) = note_form_error() {
                        div { class: "error-msg", "{err}" }
                    }

                    div { class: "form-group",
                        label { {i18n.t("person.note_text_label")} }
                        textarea {
                            rows: 3,
                            placeholder: "{i18n.t(\"person_form.note_placeholder\")}",
                            value: "{note_form_text}",
                            oninput: move |e: Event<FormData>| note_form_text.set(e.value()),
                        }
                    }
                    button { class: "btn btn-primary btn-sm", onclick: on_create_note, {i18n.t("person.create_note")} }
                }
            }

            match &*notes_resource.read() {
                Some(Ok(notes)) => rsx! {
                    if notes.is_empty() {
                        div { class: "empty-state",
                            p { {i18n.t("person.no_notes")} }
                        }
                    } else {
                        for note in notes.iter() {
                            {
                                let note_id = note.id;
                                let note_text = note.text.clone();
                                let is_editing = editing_note_id() == Some(note_id);

                                if is_editing {
                                    rsx! {
                                        div {
                                            style: "margin-bottom: 12px; padding: 12px; background: var(--color-bg); border-radius: var(--radius);",
                                            if let Some(err) = edit_note_error() {
                                                div { class: "error-msg", "{err}" }
                                            }
                                            div { class: "form-group",
                                                textarea {
                                                    rows: 3,
                                                    value: "{edit_note_text}",
                                                    oninput: move |e: Event<FormData>| edit_note_text.set(e.value()),
                                                }
                                            }
                                            div { style: "display: flex; gap: 8px;",
                                                button {
                                                    class: "btn btn-primary btn-sm",
                                                    onclick: on_save_note_edit.clone(),
                                                    {i18n.t("common.save")}
                                                }
                                                button {
                                                    class: "btn btn-outline btn-sm",
                                                    onclick: move |_| {
                                                        editing_note_id.set(None);
                                                        edit_note_error.set(None);
                                                    },
                                                    {i18n.t("common.cancel")}
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    rsx! {
                                        div {
                                            style: "margin-bottom: 12px; padding: 12px; border: 1px solid var(--color-border); border-radius: var(--radius); display: flex; justify-content: space-between; align-items: flex-start;",
                                            p { style: "margin: 0; flex: 1; white-space: pre-wrap;", "{note.text}" }
                                            div { style: "display: flex; gap: 4px; margin-left: 12px;",
                                                button {
                                                    class: "btn btn-outline btn-sm",
                                                    onclick: move |_| {
                                                        editing_note_id.set(Some(note_id));
                                                        edit_note_text.set(note_text.clone());
                                                        edit_note_error.set(None);
                                                    },
                                                    {i18n.t("common.edit")}
                                                }
                                                button {
                                                    class: "btn btn-danger btn-sm",
                                                    onclick: move |_| {
                                                        confirm_delete_note_id.set(Some(note_id));
                                                        delete_note_error.set(None);
                                                    },
                                                    {i18n.t("common.delete")}
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
                Some(Err(e)) => rsx! {
                    div { class: "error-msg", {i18n.t_args("person.load_notes_error", &[("error", &e.to_string())])} }
                },
                None => rsx! {
                    div { class: "loading", {i18n.t("person.loading_notes")} }
                },
            }
        }

        // ── Citations section ────────────────────────────────────────
        div { class: "card",
            div { class: "section-header",
                h2 { style: "font-size: 1.1rem;", {i18n.t("person.citations_section")} }
                button {
                    class: "btn btn-primary btn-sm",
                    onclick: move |_| show_citation_form.toggle(),
                    if show_citation_form() { {i18n.t("common.cancel")} } else { {i18n.t("person.add_citation")} }
                }
            }

            // Add citation form
            if show_citation_form() {
                div { style: "margin-bottom: 16px; padding: 16px; background: var(--color-bg); border-radius: var(--radius);",
                    h3 { style: "margin-bottom: 12px; font-size: 0.95rem;", {i18n.t("person.new_citation")} }

                    if let Some(err) = citation_form_error() {
                        div { class: "error-msg", "{err}" }
                    }

                    div { class: "form-row",
                        div { class: "form-group",
                            label { {i18n.t("person.source")} }
                            {source_select_widget(&sources_resource, "{citation_form_source_id}", move |e: Event<FormData>| citation_form_source_id.set(e.value()), &i18n)}
                        }
                        div { class: "form-group",
                            label { {i18n.t("person.confidence")} }
                            select {
                                value: "{citation_form_confidence}",
                                oninput: move |e: Event<FormData>| citation_form_confidence.set(e.value()),
                                option { value: "VeryLow", {i18n.t("confidence.very_low")} }
                                option { value: "Low", {i18n.t("confidence.low")} }
                                option { value: "Medium", {i18n.t("confidence.medium")} }
                                option { value: "High", {i18n.t("confidence.high")} }
                                option { value: "VeryHigh", {i18n.t("confidence.very_high")} }
                            }
                        }
                    }
                    div { class: "form-row",
                        div { class: "form-group",
                            label { {i18n.t("person.page")} }
                            input {
                                r#type: "text",
                                placeholder: "{i18n.t(\"citation.page_placeholder\")}",
                                value: "{citation_form_page}",
                                oninput: move |e: Event<FormData>| citation_form_page.set(e.value()),
                            }
                        }
                        div { class: "form-group",
                            label { {i18n.t("person.citation_text_label")} }
                            input {
                                r#type: "text",
                                placeholder: "{i18n.t(\"citation.text\")}",
                                value: "{citation_form_text}",
                                oninput: move |e: Event<FormData>| citation_form_text.set(e.value()),
                            }
                        }
                    }
                    button { class: "btn btn-primary btn-sm", onclick: on_create_citation, {i18n.t("person.create_citation")} }
                }
            }

            // Citation list note: citations are created with person_id
            // but there's no list-by-person endpoint for citations in the REST API.
            // Users can manage citations after creation; a full list endpoint would
            // require backend changes. For now, we show a helpful message.
            div { class: "empty-state",
                p { class: "text-muted", {i18n.t("person.citation_hint")} }
            }
        }

        // ── Ancestors section ─────────────────────────────────────────
        div { class: "card", style: "margin-bottom: 24px;",
            div { class: "section-header",
                h2 { style: "font-size: 1.1rem;", {i18n.t("person.ancestors")} }
                button {
                    class: "btn btn-primary btn-sm",
                    onclick: move |_| show_ancestors.toggle(),
                    if show_ancestors() { {i18n.t("person.hide")} } else { {i18n.t("person.show_ancestors")} }
                }
            }

            if show_ancestors() {
                {render_ancestry_chart(
                    &ancestors_resource,
                    &all_persons_resource,
                    &all_names_resource,
                    person_id_parsed,
                    &tree_id,
                    true,
                    &i18n,
                )}
            }
        }

        // ── Descendants section ───────────────────────────────────────
        div { class: "card",
            div { class: "section-header",
                h2 { style: "font-size: 1.1rem;", {i18n.t("person.descendants")} }
                button {
                    class: "btn btn-primary btn-sm",
                    onclick: move |_| show_descendants.toggle(),
                    if show_descendants() { {i18n.t("person.hide")} } else { {i18n.t("person.show_descendants")} }
                }
            }

            if show_descendants() {
                {render_ancestry_chart(
                    &descendants_resource,
                    &all_persons_resource,
                    &all_names_resource,
                    person_id_parsed,
                    &tree_id,
                    false,
                    &i18n,
                )}
            }
        }
        } // .page-content
    }
}

// ── Widget helpers (remain local — they take Resource references) ─────

/// Renders an event type `<select>` widget.
fn event_type_select(
    value: &str,
    oninput: impl FnMut(Event<FormData>) + 'static,
    i18n: &crate::i18n::I18n,
) -> Element {
    rsx! {
        select {
            value: value,
            oninput: oninput,
            option { value: "Birth", {i18n.t("event.type.birth")} }
            option { value: "Death", {i18n.t("event.type.death")} }
            option { value: "Baptism", {i18n.t("event.type.baptism")} }
            option { value: "Burial", {i18n.t("event.type.burial")} }
            option { value: "Cremation", {i18n.t("event.type.cremation")} }
            option { value: "Graduation", {i18n.t("event.type.graduation")} }
            option { value: "Immigration", {i18n.t("event.type.immigration")} }
            option { value: "Emigration", {i18n.t("event.type.emigration")} }
            option { value: "Naturalization", {i18n.t("event.type.naturalization")} }
            option { value: "Census", {i18n.t("event.type.census")} }
            option { value: "Occupation", {i18n.t("event.type.occupation")} }
            option { value: "Residence", {i18n.t("event.type.residence")} }
            option { value: "Retirement", {i18n.t("event.type.retirement")} }
            option { value: "Will", {i18n.t("event.type.will")} }
            option { value: "Probate", {i18n.t("event.type.probate")} }
            option { value: "Marriage", {i18n.t("event.type.marriage")} }
            option { value: "Divorce", {i18n.t("event.type.divorce")} }
            option { value: "Annulment", {i18n.t("event.type.annulment")} }
            option { value: "Engagement", {i18n.t("event.type.engagement")} }
            option { value: "MarriageBann", {i18n.t("event.type.marriage_bann")} }
            option { value: "MarriageContract", {i18n.t("event.type.marriage_contract")} }
            option { value: "MarriageLicense", {i18n.t("event.type.marriage_license")} }
            option { value: "MarriageSettlement", {i18n.t("event.type.marriage_settlement")} }
            option { value: "Other", {i18n.t("event.type.other")} }
        }
    }
}

/// Renders a place picker `<select>` widget.
fn place_select_widget(
    places_resource: &Resource<
        Result<oxidgene_core::types::Connection<oxidgene_core::types::Place>, crate::api::ApiError>,
    >,
    value: &str,
    oninput: impl FnMut(Event<FormData>) + 'static,
    i18n: &crate::i18n::I18n,
) -> Element {
    let places_data = places_resource.read();
    let places: Vec<_> = match &*places_data {
        Some(Ok(conn)) => conn
            .edges
            .iter()
            .map(|e| (e.node.id, e.node.name.clone()))
            .collect(),
        _ => vec![],
    };
    rsx! {
        select {
            value: value,
            oninput: oninput,
            option { value: "", {i18n.t("person_form.no_place")} }
            for (pid, name) in places.iter() {
                option {
                    value: "{pid}",
                    "{name}"
                }
            }
        }
    }
}

/// Renders a source picker `<select>` widget.
fn source_select_widget(
    sources_resource: &Resource<
        Result<
            oxidgene_core::types::Connection<oxidgene_core::types::Source>,
            crate::api::ApiError,
        >,
    >,
    value: &str,
    oninput: impl FnMut(Event<FormData>) + 'static,
    i18n: &crate::i18n::I18n,
) -> Element {
    let sources_data = sources_resource.read();
    let sources: Vec<_> = match &*sources_data {
        Some(Ok(conn)) => conn
            .edges
            .iter()
            .map(|e| (e.node.id, e.node.title.clone()))
            .collect(),
        _ => vec![],
    };
    rsx! {
        select {
            value: value,
            oninput: oninput,
            option { value: "", {i18n.t("person.select_source")} }
            for (sid, title) in sources.iter() {
                option {
                    value: "{sid}",
                    "{title}"
                }
            }
        }
    }
}

// ── Helper: ancestry chart rendering using PersonNode ─────────────────

/// Renders the ancestry/descendant chart using [`PersonNode`] components.
fn render_ancestry_chart(
    edges_resource: &Resource<
        Result<Vec<oxidgene_core::types::PersonAncestry>, crate::api::ApiError>,
    >,
    all_persons_resource: &Resource<
        Result<
            oxidgene_core::types::Connection<oxidgene_core::types::Person>,
            crate::api::ApiError,
        >,
    >,
    all_names_resource: &Resource<
        Result<HashMap<Uuid, Vec<oxidgene_core::types::PersonName>>, crate::api::ApiError>,
    >,
    current_person_id: Option<Uuid>,
    tree_id: &str,
    is_ancestors: bool,
    i18n: &crate::i18n::I18n,
) -> Element {
    let edges_data = edges_resource.read();
    let persons_data = all_persons_resource.read();
    let names_data = all_names_resource.read();

    if edges_data.is_none() || persons_data.is_none() || names_data.is_none() {
        return rsx! {
            div { class: "loading", {i18n.t("person.loading_ancestry")} }
        };
    }

    let edges = match &*edges_data {
        Some(Ok(e)) => e,
        Some(Err(e)) => {
            return rsx! {
                div { class: "error-msg", {i18n.t_args("person.load_ancestry_error", &[("error", &e.to_string())])} }
            };
        }
        None => unreachable!(),
    };

    if edges.is_empty() {
        let label = if is_ancestors {
            i18n.t("person.no_ancestors_label")
        } else {
            i18n.t("person.no_descendants_label")
        };
        return rsx! {
            div { class: "empty-state",
                p { {i18n.t_args("person.no_ancestry_data", &[("label", &label)])} }
                p { class: "text-muted",
                    {i18n.t("person.ancestry_hint")}
                }
            }
        };
    }

    let person_sex: HashMap<Uuid, Sex> = match &*persons_data {
        Some(Ok(conn)) => conn.edges.iter().map(|e| (e.node.id, e.node.sex)).collect(),
        _ => HashMap::new(),
    };

    let name_map: HashMap<Uuid, Vec<oxidgene_core::types::PersonName>> = match &*names_data {
        Some(Ok(m)) => m.clone(),
        _ => HashMap::new(),
    };

    let mut by_depth: std::collections::BTreeMap<i32, Vec<Uuid>> =
        std::collections::BTreeMap::new();
    for edge in edges.iter() {
        let person_id = if is_ancestors {
            edge.ancestor_id
        } else {
            edge.descendant_id
        };
        by_depth.entry(edge.depth).or_default().push(person_id);
    }

    for persons in by_depth.values_mut() {
        persons.sort();
        persons.dedup();
    }

    let tree_id_owned = tree_id.to_string();

    rsx! {
        div { class: "chart-container",
            for (depth, person_ids) in by_depth.iter() {
                div { class: "depth-group",
                    div { class: "gen-label",
                        {generation_label(*depth, is_ancestors, i18n)}
                        " ({person_ids.len()})"
                    }
                    div { class: "depth-group-nodes",
                        for pid in person_ids.iter() {
                            {
                                let pid = *pid;
                                let name = resolve_name(pid, &name_map);
                                let sex = person_sex.get(&pid).cloned().unwrap_or(Sex::Unknown);
                                let is_current = current_person_id == Some(pid);
                                let tree_id_link = tree_id_owned.clone();
                                rsx! {
                                    PersonNode {
                                        name: name,
                                        sex: sex,
                                        is_current: is_current,
                                        tree_id: tree_id_link,
                                        person_id: pid.to_string(),
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
