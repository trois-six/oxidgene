//! Person detail page — shows names, events, notes, citations, and ancestry charts with full CRUD.

use std::collections::HashMap;

use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::{
    ApiClient, CreateCitationBody, CreateEventBody, CreateNoteBody, CreatePersonNameBody,
    UpdateEventBody, UpdateNoteBody, UpdatePersonBody, UpdatePersonNameBody,
};
use crate::router::Route;
use oxidgene_core::{Confidence, EventType, NameType, Sex};

/// Page rendered at `/trees/:tree_id/persons/:person_id`.
#[component]
pub fn PersonDetail(tree_id: String, person_id: String) -> Element {
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
    // Since there's no "list all names in tree" endpoint, we'll build a lookup
    // from the persons + names loaded by ancestries.
    let api_all_names = api.clone();
    let all_names_resource = use_resource(move || {
        let api = api_all_names.clone();
        let _tick = refresh();
        let tid = tree_id_parsed;
        let need = show_ancestors() || show_descendants();
        async move {
            if !need {
                return Ok(HashMap::<Uuid, Vec<oxidgene_core::types::PersonName>>::new());
            }
            let Some(tid) = tid else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: "Invalid IDs".to_string(),
                });
            };
            // Load all persons first, then load names for each.
            let persons = api.list_persons(tid, Some(500), None).await?;
            let mut name_map = HashMap::new();
            for edge in &persons.edges {
                let pid = edge.node.id;
                match api.list_person_names(tid, pid).await {
                    Ok(names) => {
                        name_map.insert(pid, names);
                    }
                    Err(_) => {
                        // If we can't load names for a person, skip.
                        name_map.insert(pid, vec![]);
                    }
                }
            }
            Ok(name_map)
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

    // Derive display name from loaded names.
    let display_name = match &*names_resource.read() {
        Some(Ok(names)) => {
            let primary = names.iter().find(|n| n.is_primary).or(names.first());
            match primary {
                Some(name) => {
                    let dn = name.display_name();
                    if dn.is_empty() {
                        "Unnamed".to_string()
                    } else {
                        dn
                    }
                }
                None => "Unnamed".to_string(),
            }
        }
        _ => "Loading...".to_string(),
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

    rsx! {
        // Back navigation
        div { style: "margin-bottom: 16px;",
            Link {
                to: Route::TreeDetail { tree_id: tree_id.clone() },
                class: "back-link",
                "← Back to Tree"
            }
        }

        // Delete person confirmation dialog
        if confirm_delete() {
            div { class: "modal-backdrop",
                div { class: "modal-card",
                    h3 { "Delete Person" }
                    p { style: "margin: 12px 0;",
                        "Are you sure you want to delete "
                        strong { "{display_name}" }
                        "? This action cannot be undone."
                    }
                    if let Some(err) = delete_error() {
                        div { class: "error-msg", "{err}" }
                    }
                    div { class: "modal-actions",
                        button {
                            class: "btn btn-outline",
                            onclick: move |_| {
                                confirm_delete.set(false);
                                delete_error.set(None);
                            },
                            "Cancel"
                        }
                        button {
                            class: "btn btn-danger",
                            onclick: on_confirm_delete,
                            "Delete"
                        }
                    }
                }
            }
        }

        // Delete name confirmation dialog
        if confirm_delete_name_id().is_some() {
            div { class: "modal-backdrop",
                div { class: "modal-card",
                    h3 { "Delete Name" }
                    p { style: "margin: 12px 0;",
                        "Are you sure you want to delete this name?"
                    }
                    if let Some(err) = delete_name_error() {
                        div { class: "error-msg", "{err}" }
                    }
                    div { class: "modal-actions",
                        button {
                            class: "btn btn-outline",
                            onclick: move |_| {
                                confirm_delete_name_id.set(None);
                                delete_name_error.set(None);
                            },
                            "Cancel"
                        }
                        button {
                            class: "btn btn-danger",
                            onclick: on_confirm_delete_name,
                            "Delete"
                        }
                    }
                }
            }
        }

        // Delete event confirmation dialog
        if confirm_delete_event_id().is_some() {
            div { class: "modal-backdrop",
                div { class: "modal-card",
                    h3 { "Delete Event" }
                    p { style: "margin: 12px 0;",
                        "Are you sure you want to delete this event?"
                    }
                    if let Some(err) = delete_event_error() {
                        div { class: "error-msg", "{err}" }
                    }
                    div { class: "modal-actions",
                        button {
                            class: "btn btn-outline",
                            onclick: move |_| {
                                confirm_delete_event_id.set(None);
                                delete_event_error.set(None);
                            },
                            "Cancel"
                        }
                        button {
                            class: "btn btn-danger",
                            onclick: on_confirm_delete_event,
                            "Delete"
                        }
                    }
                }
            }
        }

        // Delete note confirmation dialog
        if confirm_delete_note_id().is_some() {
            div { class: "modal-backdrop",
                div { class: "modal-card",
                    h3 { "Delete Note" }
                    p { style: "margin: 12px 0;",
                        "Are you sure you want to delete this note?"
                    }
                    if let Some(err) = delete_note_error() {
                        div { class: "error-msg", "{err}" }
                    }
                    div { class: "modal-actions",
                        button {
                            class: "btn btn-outline",
                            onclick: move |_| {
                                confirm_delete_note_id.set(None);
                                delete_note_error.set(None);
                            },
                            "Cancel"
                        }
                        button {
                            class: "btn btn-danger",
                            onclick: on_confirm_delete_note,
                            "Delete"
                        }
                    }
                }
            }
        }

        // Delete citation confirmation dialog
        if confirm_delete_citation_id().is_some() {
            div { class: "modal-backdrop",
                div { class: "modal-card",
                    h3 { "Delete Citation" }
                    p { style: "margin: 12px 0;",
                        "Are you sure you want to delete this citation?"
                    }
                    if let Some(err) = delete_citation_error() {
                        div { class: "error-msg", "{err}" }
                    }
                    div { class: "modal-actions",
                        button {
                            class: "btn btn-outline",
                            onclick: move |_| {
                                confirm_delete_citation_id.set(None);
                                delete_citation_error.set(None);
                            },
                            "Cancel"
                        }
                        button {
                            class: "btn btn-danger",
                            onclick: on_confirm_delete_citation,
                            "Delete"
                        }
                    }
                }
            }
        }

        // Person header
        match &*person_resource.read() {
            Some(Ok(person)) => {
                let sex_str = format!("{:?}", person.sex);
                rsx! {
                    div { class: "page-header",
                        div {
                            h1 { "{display_name}" }
                            div { style: "display: flex; align-items: center; gap: 8px; margin-top: 4px;",
                                if editing_sex() {
                                    select {
                                        value: "{edit_sex_val}",
                                        oninput: move |e: Event<FormData>| edit_sex_val.set(e.value()),
                                        option { value: "Unknown", "Unknown" }
                                        option { value: "Male", "Male" }
                                        option { value: "Female", "Female" }
                                    }
                                    button {
                                        class: "btn btn-primary btn-sm",
                                        onclick: on_save_sex,
                                        "Save"
                                    }
                                    button {
                                        class: "btn btn-outline btn-sm",
                                        onclick: move |_| {
                                            editing_sex.set(false);
                                            edit_sex_error.set(None);
                                        },
                                        "Cancel"
                                    }
                                    if let Some(err) = edit_sex_error() {
                                        span { class: "error-msg", style: "margin: 0; padding: 4px 8px; font-size: 0.8rem;", "{err}" }
                                    }
                                } else {
                                    span { class: "badge", "{sex_str}" }
                                    button {
                                        class: "btn btn-outline btn-sm",
                                        onclick: move |_| {
                                            edit_sex_val.set(sex_str.clone());
                                            editing_sex.set(true);
                                        },
                                        "Edit Sex"
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
                                "Delete"
                            }
                            button {
                                class: "btn btn-outline",
                                onclick: move |_| refresh += 1,
                                "Refresh"
                            }
                        }
                    }
                }
            },
            Some(Err(e)) => rsx! {
                div { class: "error-msg", "Failed to load person: {e}" }
            },
            None => rsx! {
                div { class: "loading", "Loading person..." }
            },
        }

        // ── Names section ────────────────────────────────────────────
        div { class: "card", style: "margin-bottom: 24px;",
            div { class: "section-header",
                h2 { style: "font-size: 1.1rem;", "Names" }
                button {
                    class: "btn btn-primary btn-sm",
                    onclick: move |_| show_name_form.toggle(),
                    if show_name_form() { "Cancel" } else { "Add Name" }
                }
            }

            // Add name form
            if show_name_form() {
                div { style: "margin-bottom: 16px; padding: 16px; background: var(--color-bg); border-radius: var(--radius);",
                    h3 { style: "margin-bottom: 12px; font-size: 0.95rem;", "New Name" }

                    if let Some(err) = name_form_error() {
                        div { class: "error-msg", "{err}" }
                    }

                    div { class: "form-row",
                        div { class: "form-group",
                            label { "Name Type" }
                            select {
                                value: "{name_form_type}",
                                oninput: move |e: Event<FormData>| name_form_type.set(e.value()),
                                option { value: "Birth", "Birth" }
                                option { value: "Married", "Married" }
                                option { value: "AlsoKnownAs", "Also Known As" }
                                option { value: "Maiden", "Maiden" }
                                option { value: "Religious", "Religious" }
                                option { value: "Other", "Other" }
                            }
                        }
                        div { class: "form-group",
                            label { "Primary" }
                            select {
                                value: if name_form_primary() { "true" } else { "false" },
                                oninput: move |e: Event<FormData>| name_form_primary.set(e.value() == "true"),
                                option { value: "true", "Yes" }
                                option { value: "false", "No" }
                            }
                        }
                    }
                    div { class: "form-row",
                        div { class: "form-group",
                            label { "Given Names" }
                            input {
                                r#type: "text",
                                placeholder: "e.g. Jean-Pierre",
                                value: "{name_form_given}",
                                oninput: move |e: Event<FormData>| name_form_given.set(e.value()),
                            }
                        }
                        div { class: "form-group",
                            label { "Surname" }
                            input {
                                r#type: "text",
                                placeholder: "e.g. Dupont",
                                value: "{name_form_surname}",
                                oninput: move |e: Event<FormData>| name_form_surname.set(e.value()),
                            }
                        }
                    }
                    div { class: "form-row",
                        div { class: "form-group",
                            label { "Prefix" }
                            input {
                                r#type: "text",
                                placeholder: "e.g. Dr.",
                                value: "{name_form_prefix}",
                                oninput: move |e: Event<FormData>| name_form_prefix.set(e.value()),
                            }
                        }
                        div { class: "form-group",
                            label { "Suffix" }
                            input {
                                r#type: "text",
                                placeholder: "e.g. Jr.",
                                value: "{name_form_suffix}",
                                oninput: move |e: Event<FormData>| name_form_suffix.set(e.value()),
                            }
                        }
                        div { class: "form-group",
                            label { "Nickname" }
                            input {
                                r#type: "text",
                                placeholder: "e.g. JP",
                                value: "{name_form_nickname}",
                                oninput: move |e: Event<FormData>| name_form_nickname.set(e.value()),
                            }
                        }
                    }
                    button { class: "btn btn-primary btn-sm", onclick: on_create_name, "Create Name" }
                }
            }

            match &*names_resource.read() {
                Some(Ok(names)) => rsx! {
                    if names.is_empty() {
                        div { class: "empty-state",
                            p { "No names recorded." }
                        }
                    } else {
                        div { class: "table-wrapper",
                            table {
                                thead {
                                    tr {
                                        th { "Type" }
                                        th { "Given Names" }
                                        th { "Surname" }
                                        th { "Primary" }
                                        th { style: "width: 140px;", "Actions" }
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
                                                                        label { "Name Type" }
                                                                        select {
                                                                            value: "{edit_name_type}",
                                                                            oninput: move |e: Event<FormData>| edit_name_type.set(e.value()),
                                                                            option { value: "Birth", "Birth" }
                                                                            option { value: "Married", "Married" }
                                                                            option { value: "AlsoKnownAs", "Also Known As" }
                                                                            option { value: "Maiden", "Maiden" }
                                                                            option { value: "Religious", "Religious" }
                                                                            option { value: "Other", "Other" }
                                                                        }
                                                                    }
                                                                    div { class: "form-group",
                                                                        label { "Primary" }
                                                                        select {
                                                                            value: if edit_name_primary() { "true" } else { "false" },
                                                                            oninput: move |e: Event<FormData>| edit_name_primary.set(e.value() == "true"),
                                                                            option { value: "true", "Yes" }
                                                                            option { value: "false", "No" }
                                                                        }
                                                                    }
                                                                }
                                                                div { class: "form-row",
                                                                    div { class: "form-group",
                                                                        label { "Given Names" }
                                                                        input {
                                                                            r#type: "text",
                                                                            value: "{edit_name_given}",
                                                                            oninput: move |e: Event<FormData>| edit_name_given.set(e.value()),
                                                                        }
                                                                    }
                                                                    div { class: "form-group",
                                                                        label { "Surname" }
                                                                        input {
                                                                            r#type: "text",
                                                                            value: "{edit_name_surname}",
                                                                            oninput: move |e: Event<FormData>| edit_name_surname.set(e.value()),
                                                                        }
                                                                    }
                                                                }
                                                                div { class: "form-row",
                                                                    div { class: "form-group",
                                                                        label { "Prefix" }
                                                                        input {
                                                                            r#type: "text",
                                                                            value: "{edit_name_prefix}",
                                                                            oninput: move |e: Event<FormData>| edit_name_prefix.set(e.value()),
                                                                        }
                                                                    }
                                                                    div { class: "form-group",
                                                                        label { "Suffix" }
                                                                        input {
                                                                            r#type: "text",
                                                                            value: "{edit_name_suffix}",
                                                                            oninput: move |e: Event<FormData>| edit_name_suffix.set(e.value()),
                                                                        }
                                                                    }
                                                                    div { class: "form-group",
                                                                        label { "Nickname" }
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
                                                                        "Save"
                                                                    }
                                                                    button {
                                                                        class: "btn btn-outline btn-sm",
                                                                        onclick: move |_| {
                                                                            editing_name_id.set(None);
                                                                            edit_name_error.set(None);
                                                                        },
                                                                        "Cancel"
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
                                                            if name.is_primary { "Yes" } else { "No" }
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
                                                                    "Edit"
                                                                }
                                                                button {
                                                                    class: "btn btn-danger btn-sm",
                                                                    onclick: move |_| {
                                                                        confirm_delete_name_id.set(Some(nid));
                                                                        delete_name_error.set(None);
                                                                    },
                                                                    "Delete"
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
                    div { class: "error-msg", "Failed to load names: {e}" }
                },
                None => rsx! {
                    div { class: "loading", "Loading names..." }
                },
            }
        }

        // ── Events section ───────────────────────────────────────────
        div { class: "card", style: "margin-bottom: 24px;",
            div { class: "section-header",
                h2 { style: "font-size: 1.1rem;", "Events" }
                button {
                    class: "btn btn-primary btn-sm",
                    onclick: move |_| show_event_form.toggle(),
                    if show_event_form() { "Cancel" } else { "Add Event" }
                }
            }

            // Add event form
            if show_event_form() {
                div { style: "margin-bottom: 16px; padding: 16px; background: var(--color-bg); border-radius: var(--radius);",
                    h3 { style: "margin-bottom: 12px; font-size: 0.95rem;", "New Event" }

                    if let Some(err) = event_form_error() {
                        div { class: "error-msg", "{err}" }
                    }

                    div { class: "form-row",
                        div { class: "form-group",
                            label { "Event Type" }
                            {event_type_select("{event_form_type}", move |e: Event<FormData>| event_form_type.set(e.value()))}
                        }
                        div { class: "form-group",
                            label { "Date" }
                            input {
                                r#type: "text",
                                placeholder: "e.g. 1 JAN 1900",
                                value: "{event_form_date}",
                                oninput: move |e: Event<FormData>| event_form_date.set(e.value()),
                            }
                        }
                    }
                    div { class: "form-row",
                        div { class: "form-group",
                            label { "Place" }
                            {place_select_widget(&places_resource, "{event_form_place_id}", move |e: Event<FormData>| event_form_place_id.set(e.value()))}
                        }
                        div { class: "form-group",
                            label { "Description" }
                            input {
                                r#type: "text",
                                placeholder: "Optional description",
                                value: "{event_form_desc}",
                                oninput: move |e: Event<FormData>| event_form_desc.set(e.value()),
                            }
                        }
                    }
                    button { class: "btn btn-primary btn-sm", onclick: on_create_event, "Create Event" }
                }
            }

            match &*events_resource.read() {
                Some(Ok(conn)) => rsx! {
                    if conn.edges.is_empty() {
                        div { class: "empty-state",
                            p { "No events recorded." }
                        }
                    } else {
                        div { class: "table-wrapper",
                            table {
                                thead {
                                    tr {
                                        th { "Type" }
                                        th { "Date" }
                                        th { "Place" }
                                        th { "Description" }
                                        th { style: "width: 140px;", "Actions" }
                                    }
                                }
                                tbody {
                                    for edge in conn.edges.iter() {
                                        {
                                            let event = &edge.node;
                                            let eid = event.id;
                                            let is_editing = editing_event_id() == Some(eid);
                                            let et = format!("{:?}", event.event_type);
                                            let dv = event.date_value.clone().unwrap_or_default();
                                            let desc = event.description.clone().unwrap_or_default();
                                            let pid_str = event.place_id.map(|p| p.to_string()).unwrap_or_default();
                                            let place_display = event.place_id.map(&place_name).unwrap_or_else(|| "--".to_string());

                                            if is_editing {
                                                rsx! {
                                                    tr {
                                                        td { colspan: 5,
                                                            div { style: "padding: 8px; background: var(--color-bg); border-radius: var(--radius);",
                                                                if let Some(err) = edit_event_error() {
                                                                    div { class: "error-msg", "{err}" }
                                                                }
                                                                div { class: "form-row",
                                                                    div { class: "form-group",
                                                                        label { "Event Type" }
                                                                        {event_type_select("{edit_event_type}", move |e: Event<FormData>| edit_event_type.set(e.value()))}
                                                                    }
                                                                    div { class: "form-group",
                                                                        label { "Date" }
                                                                        input {
                                                                            r#type: "text",
                                                                            value: "{edit_event_date}",
                                                                            oninput: move |e: Event<FormData>| edit_event_date.set(e.value()),
                                                                        }
                                                                    }
                                                                }
                                                                div { class: "form-row",
                                                                    div { class: "form-group",
                                                                        label { "Place" }
                                                                        {place_select_widget(&places_resource, "{edit_event_place_id}", move |e: Event<FormData>| edit_event_place_id.set(e.value()))}
                                                                    }
                                                                    div { class: "form-group",
                                                                        label { "Description" }
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
                                                                        "Save"
                                                                    }
                                                                    button {
                                                                        class: "btn btn-outline btn-sm",
                                                                        onclick: move |_| {
                                                                            editing_event_id.set(None);
                                                                            edit_event_error.set(None);
                                                                        },
                                                                        "Cancel"
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
                                                                    "Edit"
                                                                }
                                                                button {
                                                                    class: "btn btn-danger btn-sm",
                                                                    onclick: move |_| {
                                                                        confirm_delete_event_id.set(Some(eid));
                                                                        delete_event_error.set(None);
                                                                    },
                                                                    "Delete"
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
                    div { class: "error-msg", "Failed to load events: {e}" }
                },
                None => rsx! {
                    div { class: "loading", "Loading events..." }
                },
            }
        }

        // ── Notes section ────────────────────────────────────────────
        div { class: "card", style: "margin-bottom: 24px;",
            div { class: "section-header",
                h2 { style: "font-size: 1.1rem;", "Notes" }
                button {
                    class: "btn btn-primary btn-sm",
                    onclick: move |_| show_note_form.toggle(),
                    if show_note_form() { "Cancel" } else { "Add Note" }
                }
            }

            // Add note form
            if show_note_form() {
                div { style: "margin-bottom: 16px; padding: 16px; background: var(--color-bg); border-radius: var(--radius);",
                    h3 { style: "margin-bottom: 12px; font-size: 0.95rem;", "New Note" }

                    if let Some(err) = note_form_error() {
                        div { class: "error-msg", "{err}" }
                    }

                    div { class: "form-group",
                        label { "Text" }
                        textarea {
                            rows: 3,
                            placeholder: "Enter note text...",
                            value: "{note_form_text}",
                            oninput: move |e: Event<FormData>| note_form_text.set(e.value()),
                        }
                    }
                    button { class: "btn btn-primary btn-sm", onclick: on_create_note, "Create Note" }
                }
            }

            match &*notes_resource.read() {
                Some(Ok(notes)) => rsx! {
                    if notes.is_empty() {
                        div { class: "empty-state",
                            p { "No notes recorded." }
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
                                                    "Save"
                                                }
                                                button {
                                                    class: "btn btn-outline btn-sm",
                                                    onclick: move |_| {
                                                        editing_note_id.set(None);
                                                        edit_note_error.set(None);
                                                    },
                                                    "Cancel"
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
                                                    "Edit"
                                                }
                                                button {
                                                    class: "btn btn-danger btn-sm",
                                                    onclick: move |_| {
                                                        confirm_delete_note_id.set(Some(note_id));
                                                        delete_note_error.set(None);
                                                    },
                                                    "Delete"
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
                    div { class: "error-msg", "Failed to load notes: {e}" }
                },
                None => rsx! {
                    div { class: "loading", "Loading notes..." }
                },
            }
        }

        // ── Citations section ────────────────────────────────────────
        div { class: "card",
            div { class: "section-header",
                h2 { style: "font-size: 1.1rem;", "Citations" }
                button {
                    class: "btn btn-primary btn-sm",
                    onclick: move |_| show_citation_form.toggle(),
                    if show_citation_form() { "Cancel" } else { "Add Citation" }
                }
            }

            // Add citation form
            if show_citation_form() {
                div { style: "margin-bottom: 16px; padding: 16px; background: var(--color-bg); border-radius: var(--radius);",
                    h3 { style: "margin-bottom: 12px; font-size: 0.95rem;", "New Citation" }

                    if let Some(err) = citation_form_error() {
                        div { class: "error-msg", "{err}" }
                    }

                    div { class: "form-row",
                        div { class: "form-group",
                            label { "Source" }
                            {source_select_widget(&sources_resource, "{citation_form_source_id}", move |e: Event<FormData>| citation_form_source_id.set(e.value()))}
                        }
                        div { class: "form-group",
                            label { "Confidence" }
                            select {
                                value: "{citation_form_confidence}",
                                oninput: move |e: Event<FormData>| citation_form_confidence.set(e.value()),
                                option { value: "VeryLow", "Very Low" }
                                option { value: "Low", "Low" }
                                option { value: "Medium", "Medium" }
                                option { value: "High", "High" }
                                option { value: "VeryHigh", "Very High" }
                            }
                        }
                    }
                    div { class: "form-row",
                        div { class: "form-group",
                            label { "Page" }
                            input {
                                r#type: "text",
                                placeholder: "e.g. p. 42",
                                value: "{citation_form_page}",
                                oninput: move |e: Event<FormData>| citation_form_page.set(e.value()),
                            }
                        }
                        div { class: "form-group",
                            label { "Text" }
                            input {
                                r#type: "text",
                                placeholder: "Citation text",
                                value: "{citation_form_text}",
                                oninput: move |e: Event<FormData>| citation_form_text.set(e.value()),
                            }
                        }
                    }
                    button { class: "btn btn-primary btn-sm", onclick: on_create_citation, "Create Citation" }
                }
            }

            // Citation list note: citations are created with person_id
            // but there's no list-by-person endpoint for citations in the REST API.
            // Users can manage citations after creation; a full list endpoint would
            // require backend changes. For now, we show a helpful message.
            div { class: "empty-state",
                p { class: "text-muted", "Citations are linked to this person via the form above. View source details on the tree page to see all citations." }
            }
        }

        // ── Ancestors section ─────────────────────────────────────────
        div { class: "card", style: "margin-bottom: 24px;",
            div { class: "section-header",
                h2 { style: "font-size: 1.1rem;", "Ancestors" }
                button {
                    class: "btn btn-primary btn-sm",
                    onclick: move |_| show_ancestors.toggle(),
                    if show_ancestors() { "Hide" } else { "Show Ancestors" }
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
                )}
            }
        }

        // ── Descendants section ───────────────────────────────────────
        div { class: "card",
            div { class: "section-header",
                h2 { style: "font-size: 1.1rem;", "Descendants" }
                button {
                    class: "btn btn-primary btn-sm",
                    onclick: move |_| show_descendants.toggle(),
                    if show_descendants() { "Hide" } else { "Show Descendants" }
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
                )}
            }
        }
    }
}

// ── Helper functions ─────────────────────────────────────────────────

fn parse_sex(s: &str) -> oxidgene_core::Sex {
    match s {
        "Male" => oxidgene_core::Sex::Male,
        "Female" => oxidgene_core::Sex::Female,
        _ => oxidgene_core::Sex::Unknown,
    }
}

fn parse_name_type(s: &str) -> NameType {
    match s {
        "Birth" => NameType::Birth,
        "Married" => NameType::Married,
        "AlsoKnownAs" => NameType::AlsoKnownAs,
        "Maiden" => NameType::Maiden,
        "Religious" => NameType::Religious,
        _ => NameType::Other,
    }
}

fn parse_event_type(s: &str) -> EventType {
    match s {
        "Birth" => EventType::Birth,
        "Death" => EventType::Death,
        "Baptism" => EventType::Baptism,
        "Burial" => EventType::Burial,
        "Cremation" => EventType::Cremation,
        "Graduation" => EventType::Graduation,
        "Immigration" => EventType::Immigration,
        "Emigration" => EventType::Emigration,
        "Naturalization" => EventType::Naturalization,
        "Census" => EventType::Census,
        "Occupation" => EventType::Occupation,
        "Residence" => EventType::Residence,
        "Retirement" => EventType::Retirement,
        "Will" => EventType::Will,
        "Probate" => EventType::Probate,
        "Marriage" => EventType::Marriage,
        "Divorce" => EventType::Divorce,
        "Annulment" => EventType::Annulment,
        "Engagement" => EventType::Engagement,
        "MarriageBann" => EventType::MarriageBann,
        "MarriageContract" => EventType::MarriageContract,
        "MarriageLicense" => EventType::MarriageLicense,
        "MarriageSettlement" => EventType::MarriageSettlement,
        _ => EventType::Other,
    }
}

fn parse_confidence(s: &str) -> Confidence {
    match s {
        "VeryLow" => Confidence::VeryLow,
        "Low" => Confidence::Low,
        "High" => Confidence::High,
        "VeryHigh" => Confidence::VeryHigh,
        _ => Confidence::Medium,
    }
}

fn opt_str(s: &str) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

/// Renders an event type `<select>` widget.
fn event_type_select(value: &str, oninput: impl FnMut(Event<FormData>) + 'static) -> Element {
    rsx! {
        select {
            value: value,
            oninput: oninput,
            option { value: "Birth", "Birth" }
            option { value: "Death", "Death" }
            option { value: "Baptism", "Baptism" }
            option { value: "Burial", "Burial" }
            option { value: "Cremation", "Cremation" }
            option { value: "Graduation", "Graduation" }
            option { value: "Immigration", "Immigration" }
            option { value: "Emigration", "Emigration" }
            option { value: "Naturalization", "Naturalization" }
            option { value: "Census", "Census" }
            option { value: "Occupation", "Occupation" }
            option { value: "Residence", "Residence" }
            option { value: "Retirement", "Retirement" }
            option { value: "Will", "Will" }
            option { value: "Probate", "Probate" }
            option { value: "Marriage", "Marriage" }
            option { value: "Divorce", "Divorce" }
            option { value: "Annulment", "Annulment" }
            option { value: "Engagement", "Engagement" }
            option { value: "MarriageBann", "Marriage Bann" }
            option { value: "MarriageContract", "Marriage Contract" }
            option { value: "MarriageLicense", "Marriage License" }
            option { value: "MarriageSettlement", "Marriage Settlement" }
            option { value: "Other", "Other" }
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
            option { value: "", "-- None --" }
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
            option { value: "", "-- Select Source --" }
            for (sid, title) in sources.iter() {
                option {
                    value: "{sid}",
                    "{title}"
                }
            }
        }
    }
}

/// Returns a short CSS class for the sex icon.
fn sex_icon_class(sex: &Sex) -> &'static str {
    match sex {
        Sex::Male => "male",
        Sex::Female => "female",
        Sex::Unknown => "",
    }
}

/// Returns a short symbol for sex.
fn sex_symbol(sex: &Sex) -> &'static str {
    match sex {
        Sex::Male => "M",
        Sex::Female => "F",
        Sex::Unknown => "?",
    }
}

/// Resolve a display name for a person from the name map.
fn resolve_name(
    person_id: Uuid,
    name_map: &HashMap<Uuid, Vec<oxidgene_core::types::PersonName>>,
) -> String {
    match name_map.get(&person_id) {
        Some(names) => {
            let primary = names.iter().find(|n| n.is_primary).or(names.first());
            match primary {
                Some(name) => {
                    let dn = name.display_name();
                    if dn.is_empty() {
                        "Unnamed".to_string()
                    } else {
                        dn
                    }
                }
                None => "Unnamed".to_string(),
            }
        }
        None => "Unnamed".to_string(),
    }
}

/// Renders the ancestry/descendant chart.
///
/// For ancestors (`is_ancestors=true`): edges have `ancestor_id` at depth N from
/// the current person (the `descendant_id`). We group by depth and display
/// generation labels (Parents, Grandparents, etc.).
///
/// For descendants (`is_ancestors=false`): edges have `descendant_id` at depth N
/// from the current person (the `ancestor_id`). Same grouping logic.
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
) -> Element {
    let edges_data = edges_resource.read();
    let persons_data = all_persons_resource.read();
    let names_data = all_names_resource.read();

    // Check if resources are still loading.
    if edges_data.is_none() || persons_data.is_none() || names_data.is_none() {
        return rsx! {
            div { class: "loading", "Loading ancestry data..." }
        };
    }

    let edges = match &*edges_data {
        Some(Ok(e)) => e,
        Some(Err(e)) => {
            return rsx! {
                div { class: "error-msg", "Failed to load ancestry: {e}" }
            };
        }
        None => unreachable!(),
    };

    if edges.is_empty() {
        let label = if is_ancestors {
            "ancestors"
        } else {
            "descendants"
        };
        return rsx! {
            div { class: "empty-state",
                p { "No {label} data available." }
                p { class: "text-muted",
                    "Ancestry data is populated during GEDCOM import. "
                    "Manual person creation does not build the ancestry closure table."
                }
            }
        };
    }

    // Build person sex lookup from all_persons_resource.
    let person_sex: HashMap<Uuid, Sex> = match &*persons_data {
        Some(Ok(conn)) => conn.edges.iter().map(|e| (e.node.id, e.node.sex)).collect(),
        _ => HashMap::new(),
    };

    // Build name lookup.
    let name_map: HashMap<Uuid, Vec<oxidgene_core::types::PersonName>> = match &*names_data {
        Some(Ok(m)) => m.clone(),
        _ => HashMap::new(),
    };

    // Group edges by depth, collecting the "other" person ID.
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

    // Deduplicate within each depth level.
    for persons in by_depth.values_mut() {
        persons.sort();
        persons.dedup();
    }

    let generation_label = |depth: i32, is_anc: bool| -> String {
        if is_anc {
            match depth {
                1 => "Parents".to_string(),
                2 => "Grandparents".to_string(),
                3 => "Great-Grandparents".to_string(),
                n => format!("{n}x Great-Grandparents"),
            }
        } else {
            match depth {
                1 => "Children".to_string(),
                2 => "Grandchildren".to_string(),
                3 => "Great-Grandchildren".to_string(),
                n => format!("{n}x Great-Grandchildren"),
            }
        }
    };

    let tree_id_owned = tree_id.to_string();

    rsx! {
        div { class: "chart-container",
            for (depth, person_ids) in by_depth.iter() {
                div { class: "depth-group",
                    div { class: "gen-label",
                        {generation_label(*depth, is_ancestors)}
                        " ({person_ids.len()})"
                    }
                    div { class: "depth-group-nodes",
                        for pid in person_ids.iter() {
                            {
                                let pid = *pid;
                                let name = resolve_name(pid, &name_map);
                                let sex = person_sex.get(&pid).cloned().unwrap_or(Sex::Unknown);
                                let is_current = current_person_id == Some(pid);
                                let node_class = if is_current { "tree-node current" } else { "tree-node" };
                                let icon_class = format!("sex-icon {}", sex_icon_class(&sex));
                                let symbol = sex_symbol(&sex);
                                let tree_id_link = tree_id_owned.clone();
                                rsx! {
                                    Link {
                                        to: Route::PersonDetail {
                                            tree_id: tree_id_link,
                                            person_id: pid.to_string(),
                                        },
                                        class: node_class,
                                        span { class: icon_class, "{symbol}" }
                                        "{name}"
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
