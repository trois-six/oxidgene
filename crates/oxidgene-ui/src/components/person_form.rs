//! Modal-based person edit form, Geneanet-style.
//!
//! Provides a unified form for editing a person's civil status (sex, names),
//! birth/death events, other events, and notes.  Opened as a modal overlay
//! from the pedigree chart context menu or other triggers.

use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::{
    ApiClient, CreateEventBody, CreateNoteBody, CreatePersonNameBody, UpdateEventBody,
    UpdatePersonBody, UpdatePersonNameBody,
};
use crate::utils::{opt_str, parse_event_type, parse_name_type, parse_sex};
use oxidgene_core::EventType;
use oxidgene_core::types::{Event as CoreEvent, Note as CoreNote};

// ── Props ────────────────────────────────────────────────────────────────

#[derive(Props, Clone, PartialEq)]
pub struct PersonFormProps {
    /// Tree ID.
    pub tree_id: Uuid,
    /// Person ID to edit.
    pub person_id: Uuid,
    /// Called when the form is closed (saved or cancelled).
    pub on_close: EventHandler<()>,
    /// Called when data is saved (so parent can refresh).
    pub on_saved: EventHandler<()>,
}

// ── Component ────────────────────────────────────────────────────────────

/// Modal person edit form.
#[component]
pub fn PersonForm(props: PersonFormProps) -> Element {
    let api = use_context::<ApiClient>();
    let mut refresh = use_signal(|| 0u32);

    let tid = props.tree_id;
    let pid = props.person_id;

    // ── Tab / section state ──
    let mut active_tab = use_signal(|| "civil");
    let mut save_error = use_signal(|| None::<String>);

    // ── Sex editing ──
    let mut sex_val = use_signal(|| "Unknown".to_string());
    let mut sex_loaded = use_signal(|| false);

    // ── Name form state (add) ──
    let mut show_name_form = use_signal(|| false);
    let mut name_form_type = use_signal(|| "Birth".to_string());
    let mut name_form_given = use_signal(String::new);
    let mut name_form_surname = use_signal(String::new);
    let mut name_form_prefix = use_signal(String::new);
    let mut name_form_suffix = use_signal(String::new);
    let mut name_form_nickname = use_signal(String::new);
    let mut name_form_primary = use_signal(|| true);
    let mut name_form_error = use_signal(|| None::<String>);

    // ── Name edit state ──
    let mut editing_name_id = use_signal(|| None::<Uuid>);
    let mut edit_name_type = use_signal(|| "Birth".to_string());
    let mut edit_name_given = use_signal(String::new);
    let mut edit_name_surname = use_signal(String::new);
    let mut edit_name_prefix = use_signal(String::new);
    let mut edit_name_suffix = use_signal(String::new);
    let mut edit_name_nickname = use_signal(String::new);
    let mut edit_name_primary = use_signal(|| false);
    let mut edit_name_error = use_signal(|| None::<String>);

    // ── Birth/Death event state ──
    let mut birth_date = use_signal(String::new);
    let mut birth_place_id = use_signal(String::new);
    let mut birth_desc = use_signal(String::new);
    let mut death_date = use_signal(String::new);
    let mut death_place_id = use_signal(String::new);
    let mut death_desc = use_signal(String::new);
    let mut birth_death_loaded = use_signal(|| false);
    let mut birth_event_id = use_signal(|| None::<Uuid>);
    let mut death_event_id = use_signal(|| None::<Uuid>);

    // ── Event add state ──
    let mut show_event_form = use_signal(|| false);
    let mut event_form_type = use_signal(|| "Baptism".to_string());
    let mut event_form_date = use_signal(String::new);
    let mut event_form_place_id = use_signal(String::new);
    let mut event_form_desc = use_signal(String::new);
    let mut event_form_error = use_signal(|| None::<String>);

    // ── Note add state ──
    let mut show_note_form = use_signal(|| false);
    let mut note_form_text = use_signal(String::new);
    let mut note_form_error = use_signal(|| None::<String>);

    // ── Resources ──

    // Person
    let api_person = api.clone();
    let person_resource = use_resource(move || {
        let api = api_person.clone();
        let _tick = refresh();
        async move { api.get_person(tid, pid).await }
    });

    // Names
    let api_names = api.clone();
    let names_resource = use_resource(move || {
        let api = api_names.clone();
        let _tick = refresh();
        async move { api.list_person_names(tid, pid).await }
    });

    // Events (for this person)
    let api_events = api.clone();
    let events_resource = use_resource(move || {
        let api = api_events.clone();
        let _tick = refresh();
        async move {
            api.list_events(tid, Some(100), None, None, Some(pid), None)
                .await
        }
    });

    // Places (for place picker)
    let api_places = api.clone();
    let places_resource = use_resource(move || {
        let api = api_places.clone();
        let _tick = refresh();
        async move { api.list_places(tid, Some(200), None, None).await }
    });

    // Notes
    let api_notes = api.clone();
    let notes_resource = use_resource(move || {
        let api = api_notes.clone();
        let _tick = refresh();
        async move { api.list_notes(tid, Some(pid), None, None, None).await }
    });

    // ── Populate sex from person data ──
    if !sex_loaded()
        && let Some(Ok(person)) = &*person_resource.read()
    {
        sex_val.set(format!("{:?}", person.sex));
        sex_loaded.set(true);
    }

    // ── Populate birth/death from events ──
    if !birth_death_loaded()
        && let Some(Ok(conn)) = &*events_resource.read()
    {
        for edge in &conn.edges {
            let ev = &edge.node;
            match ev.event_type {
                EventType::Birth => {
                    birth_event_id.set(Some(ev.id));
                    birth_date.set(ev.date_value.clone().unwrap_or_default());
                    birth_place_id.set(ev.place_id.map(|id| id.to_string()).unwrap_or_default());
                    birth_desc.set(ev.description.clone().unwrap_or_default());
                }
                EventType::Death => {
                    death_event_id.set(Some(ev.id));
                    death_date.set(ev.date_value.clone().unwrap_or_default());
                    death_place_id.set(ev.place_id.map(|id| id.to_string()).unwrap_or_default());
                    death_desc.set(ev.description.clone().unwrap_or_default());
                }
                _ => {}
            }
        }
        birth_death_loaded.set(true);
    }

    // ── Derived data ──

    let display_name: String = match &*names_resource.read() {
        Some(Ok(names)) => {
            let primary = names.iter().find(|n| n.is_primary).or(names.first());
            match primary {
                Some(n) => {
                    let dn = n.display_name();
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

    // Extract non-birth/death events.
    let other_events: Vec<CoreEvent> = match &*events_resource.read() {
        Some(Ok(conn)) => conn
            .edges
            .iter()
            .filter(|e| !matches!(e.node.event_type, EventType::Birth | EventType::Death))
            .map(|e| e.node.clone())
            .collect(),
        _ => vec![],
    };

    let notes_list: Vec<CoreNote> = match &*notes_resource.read() {
        Some(Ok(notes)) => notes.clone(),
        _ => vec![],
    };

    // Place name resolver.
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

    // Place options for selects.
    let place_options: Vec<(String, String)> = {
        let data = places_resource.read();
        match &*data {
            Some(Ok(conn)) => conn
                .edges
                .iter()
                .map(|e| (e.node.id.to_string(), e.node.name.clone()))
                .collect(),
            _ => vec![],
        }
    };

    // ── Handlers ──

    // Save sex.
    let api_save_sex = api.clone();
    let on_saved_sex = props.on_saved;
    let on_save_sex = move |_| {
        let api = api_save_sex.clone();
        let sex_str = sex_val();
        spawn(async move {
            let body = UpdatePersonBody {
                sex: Some(parse_sex(&sex_str)),
            };
            match api.update_person(tid, pid, &body).await {
                Ok(_) => {
                    save_error.set(None);
                    on_saved_sex.call(());
                    refresh += 1;
                }
                Err(e) => save_error.set(Some(format!("{e}"))),
            }
        });
    };

    // Save birth event.
    let api_save_birth = api.clone();
    let on_saved_birth = props.on_saved;
    let on_save_birth = move |_| {
        let api = api_save_birth.clone();
        let date = birth_date().trim().to_string();
        let place_str = birth_place_id();
        let desc = birth_desc().trim().to_string();
        let existing_id = birth_event_id();
        spawn(async move {
            let place_id = if place_str.is_empty() {
                None
            } else {
                place_str.parse::<Uuid>().ok()
            };
            if let Some(eid) = existing_id {
                // Update existing birth event.
                let body = UpdateEventBody {
                    event_type: Some(EventType::Birth),
                    date_value: Some(opt_str(&date)),
                    date_sort: None,
                    place_id: Some(place_id),
                    description: Some(opt_str(&desc)),
                };
                match api.update_event(tid, eid, &body).await {
                    Ok(_) => {
                        save_error.set(None);
                        on_saved_birth.call(());
                        refresh += 1;
                    }
                    Err(e) => save_error.set(Some(format!("{e}"))),
                }
            } else {
                // Create new birth event.
                let body = CreateEventBody {
                    event_type: EventType::Birth,
                    date_value: opt_str(&date),
                    date_sort: None,
                    place_id,
                    person_id: Some(pid),
                    family_id: None,
                    description: opt_str(&desc),
                };
                match api.create_event(tid, &body).await {
                    Ok(ev) => {
                        birth_event_id.set(Some(ev.id));
                        save_error.set(None);
                        on_saved_birth.call(());
                        refresh += 1;
                    }
                    Err(e) => save_error.set(Some(format!("{e}"))),
                }
            }
        });
    };

    // Save death event.
    let api_save_death = api.clone();
    let on_saved_death = props.on_saved;
    let on_save_death = move |_| {
        let api = api_save_death.clone();
        let date = death_date().trim().to_string();
        let place_str = death_place_id();
        let desc = death_desc().trim().to_string();
        let existing_id = death_event_id();
        spawn(async move {
            let place_id = if place_str.is_empty() {
                None
            } else {
                place_str.parse::<Uuid>().ok()
            };
            if let Some(eid) = existing_id {
                let body = UpdateEventBody {
                    event_type: Some(EventType::Death),
                    date_value: Some(opt_str(&date)),
                    date_sort: None,
                    place_id: Some(place_id),
                    description: Some(opt_str(&desc)),
                };
                match api.update_event(tid, eid, &body).await {
                    Ok(_) => {
                        save_error.set(None);
                        on_saved_death.call(());
                        refresh += 1;
                    }
                    Err(e) => save_error.set(Some(format!("{e}"))),
                }
            } else {
                let body = CreateEventBody {
                    event_type: EventType::Death,
                    date_value: opt_str(&date),
                    date_sort: None,
                    place_id,
                    person_id: Some(pid),
                    family_id: None,
                    description: opt_str(&desc),
                };
                match api.create_event(tid, &body).await {
                    Ok(ev) => {
                        death_event_id.set(Some(ev.id));
                        save_error.set(None);
                        on_saved_death.call(());
                        refresh += 1;
                    }
                    Err(e) => save_error.set(Some(format!("{e}"))),
                }
            }
        });
    };

    // Create name.
    let api_create_name = api.clone();
    let on_saved_name = props.on_saved;
    let on_create_name = move |_| {
        let api = api_create_name.clone();
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
                    on_saved_name.call(());
                    refresh += 1;
                }
                Err(e) => name_form_error.set(Some(format!("{e}"))),
            }
        });
    };

    // Save name edit — api and signals captured per use in rsx loop.
    let api_edit_name = api.clone();
    let on_saved_name_edit = props.on_saved;

    // Delete name — api and signals captured per use in rsx loop.
    let api_del_name = api.clone();
    let on_saved_name_del = props.on_saved;

    // Create other event.
    let api_create_event = api.clone();
    let on_saved_event = props.on_saved;
    let on_create_event = move |_| {
        let api = api_create_event.clone();
        let event_type_str = event_form_type();
        let date = event_form_date().trim().to_string();
        let place_str = event_form_place_id();
        let desc = event_form_desc().trim().to_string();
        spawn(async move {
            let place_id = if place_str.is_empty() {
                None
            } else {
                place_str.parse::<Uuid>().ok()
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
                    event_form_type.set("Baptism".to_string());
                    event_form_date.set(String::new());
                    event_form_place_id.set(String::new());
                    event_form_desc.set(String::new());
                    event_form_error.set(None);
                    on_saved_event.call(());
                    refresh += 1;
                }
                Err(e) => event_form_error.set(Some(format!("{e}"))),
            }
        });
    };

    // Delete event — api and signals captured per use in rsx loop.
    let api_del_event = api.clone();
    let on_saved_event_del = props.on_saved;

    // Create note.
    let api_create_note = api.clone();
    let on_saved_note = props.on_saved;
    let on_create_note = move |_| {
        let api = api_create_note.clone();
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
                    on_saved_note.call(());
                    refresh += 1;
                }
                Err(e) => note_form_error.set(Some(format!("{e}"))),
            }
        });
    };

    // Delete note — api and signals captured per use in rsx loop.
    let api_del_note = api.clone();
    let on_saved_note_del = props.on_saved;

    // ── Render ──

    rsx! {
        div { class: "modal-backdrop person-form-backdrop",
            onclick: move |_| props.on_close.call(()),

            div { class: "person-form-modal",
                // Stop click propagation so clicking inside doesn't close.
                onclick: move |evt| evt.stop_propagation(),

                // Header
                div { class: "person-form-header",
                    h2 { "{display_name}" }
                    button {
                        class: "person-form-close",
                        onclick: move |_| props.on_close.call(()),
                        "x"
                    }
                }

                if let Some(err) = save_error() {
                    div { class: "error-msg", style: "margin: 0 16px;", "{err}" }
                }

                // Tab bar
                div { class: "person-form-tabs",
                    button {
                        class: if active_tab() == "civil" { "person-form-tab active" } else { "person-form-tab" },
                        onclick: move |_| active_tab.set("civil"),
                        "Civil Status"
                    }
                    button {
                        class: if active_tab() == "birth" { "person-form-tab active" } else { "person-form-tab" },
                        onclick: move |_| active_tab.set("birth"),
                        "Birth"
                    }
                    button {
                        class: if active_tab() == "death" { "person-form-tab active" } else { "person-form-tab" },
                        onclick: move |_| active_tab.set("death"),
                        "Death"
                    }
                    button {
                        class: if active_tab() == "events" { "person-form-tab active" } else { "person-form-tab" },
                        onclick: move |_| active_tab.set("events"),
                        "Events ({other_events.len()})"
                    }
                    button {
                        class: if active_tab() == "notes" { "person-form-tab active" } else { "person-form-tab" },
                        onclick: move |_| active_tab.set("notes"),
                        "Notes ({notes_list.len()})"
                    }
                }

                // Tab content
                div { class: "person-form-body",
                    // ── Civil Status tab ──
                    if active_tab() == "civil" {
                        div { class: "person-form-section",
                            // Sex
                            div { class: "form-group",
                                label { "Sex" }
                                div { style: "display: flex; gap: 8px; align-items: center;",
                                    select {
                                        value: "{sex_val}",
                                        oninput: move |e: Event<FormData>| sex_val.set(e.value()),
                                        option { value: "Unknown", "Unknown" }
                                        option { value: "Male", "Male" }
                                        option { value: "Female", "Female" }
                                    }
                                    button {
                                        class: "btn btn-primary btn-sm",
                                        onclick: on_save_sex,
                                        "Save Sex"
                                    }
                                }
                            }

                            // Names
                            div { style: "margin-top: 16px;",
                                div { class: "section-header",
                                    h3 { style: "font-size: 0.95rem;", "Names" }
                                    button {
                                        class: "btn btn-primary btn-sm",
                                        onclick: move |_| show_name_form.toggle(),
                                        if show_name_form() { "Cancel" } else { "Add Name" }
                                    }
                                }

                                // Add name form
                                if show_name_form() {
                                    {render_name_form(
                                        &name_form_error,
                                        &mut name_form_type, &mut name_form_given, &mut name_form_surname,
                                        &mut name_form_prefix, &mut name_form_suffix, &mut name_form_nickname,
                                        &mut name_form_primary, on_create_name,
                                    )}
                                }

                                // Existing names
                                match &*names_resource.read() {
                                    Some(Ok(names)) => rsx! {
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
                                                        div { class: "person-form-item editing",
                                                            if let Some(err) = edit_name_error() {
                                                                div { class: "error-msg", "{err}" }
                                                            }
                                                            div { class: "form-row",
                                                                div { class: "form-group",
                                                                    label { "Type" }
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
                                                                    onclick: {
                                                                        let api = api_edit_name.clone();
                                                                        move |_| {
                                                                            let api = api.clone();
                                                                            let Some(nid) = editing_name_id() else {
                                                                                return;
                                                                            };
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
                                                                                        on_saved_name_edit.call(());
                                                                                        refresh += 1;
                                                                                    }
                                                                                    Err(e) => edit_name_error.set(Some(format!("{e}"))),
                                                                                }
                                                                            });
                                                                        }
                                                                    },
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
                                                } else {
                                                    rsx! {
                                                        div { class: "person-form-item",
                                                            div { class: "person-form-item-info",
                                                                span { class: "badge", "{nt}" }
                                                                strong {
                                                                    if !gn.is_empty() { "{gn} " }
                                                                    "{sn}"
                                                                }
                                                                if prim {
                                                                    span { class: "badge", style: "background: var(--color-primary); color: white;", "Primary" }
                                                                }
                                                            }
                                                            div { class: "person-form-item-actions",
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
                                                                    onclick: {
                                                                        let api = api_del_name.clone();
                                                                        move |_| {
                                                                            let api = api.clone();
                                                                            spawn(async move {
                                                                                match api.delete_person_name(tid, pid, nid).await {
                                                                                    Ok(_) => {
                                                                                        on_saved_name_del.call(());
                                                                                        refresh += 1;
                                                                                    }
                                                                                    Err(e) => save_error.set(Some(format!("{e}"))),
                                                                                }
                                                                            });
                                                                        }
                                                                    },
                                                                    "Delete"
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
                        }
                    }

                    // ── Birth tab ──
                    if active_tab() == "birth" {
                        div { class: "person-form-section",
                            h3 { style: "font-size: 0.95rem; margin-bottom: 12px;", "Birth" }
                            div { class: "form-group",
                                label { "Date" }
                                input {
                                    r#type: "text",
                                    placeholder: "e.g. 1 Jan 1900, ABT 1850, BET 1800 AND 1810",
                                    value: "{birth_date}",
                                    oninput: move |e: Event<FormData>| birth_date.set(e.value()),
                                }
                            }
                            div { class: "form-group",
                                label { "Place" }
                                select {
                                    value: "{birth_place_id}",
                                    oninput: move |e: Event<FormData>| birth_place_id.set(e.value()),
                                    option { value: "", "-- No place --" }
                                    for (pid, pname) in place_options.iter() {
                                        option { value: "{pid}", "{pname}" }
                                    }
                                }
                            }
                            div { class: "form-group",
                                label { "Description" }
                                input {
                                    r#type: "text",
                                    placeholder: "Optional description",
                                    value: "{birth_desc}",
                                    oninput: move |e: Event<FormData>| birth_desc.set(e.value()),
                                }
                            }
                            button {
                                class: "btn btn-primary",
                                onclick: on_save_birth,
                                if birth_event_id().is_some() { "Update Birth" } else { "Save Birth" }
                            }
                        }
                    }

                    // ── Death tab ──
                    if active_tab() == "death" {
                        div { class: "person-form-section",
                            h3 { style: "font-size: 0.95rem; margin-bottom: 12px;", "Death" }
                            div { class: "form-group",
                                label { "Date" }
                                input {
                                    r#type: "text",
                                    placeholder: "e.g. 15 Mar 1975",
                                    value: "{death_date}",
                                    oninput: move |e: Event<FormData>| death_date.set(e.value()),
                                }
                            }
                            div { class: "form-group",
                                label { "Place" }
                                select {
                                    value: "{death_place_id}",
                                    oninput: move |e: Event<FormData>| death_place_id.set(e.value()),
                                    option { value: "", "-- No place --" }
                                    for (pid, pname) in place_options.iter() {
                                        option { value: "{pid}", "{pname}" }
                                    }
                                }
                            }
                            div { class: "form-group",
                                label { "Description" }
                                input {
                                    r#type: "text",
                                    placeholder: "Optional description",
                                    value: "{death_desc}",
                                    oninput: move |e: Event<FormData>| death_desc.set(e.value()),
                                }
                            }
                            button {
                                class: "btn btn-primary",
                                onclick: on_save_death,
                                if death_event_id().is_some() { "Update Death" } else { "Save Death" }
                            }
                        }
                    }

                    // ── Events tab ──
                    if active_tab() == "events" {
                        div { class: "person-form-section",
                            div { class: "section-header",
                                h3 { style: "font-size: 0.95rem;", "Other Events" }
                                button {
                                    class: "btn btn-primary btn-sm",
                                    onclick: move |_| show_event_form.toggle(),
                                    if show_event_form() { "Cancel" } else { "Add Event" }
                                }
                            }

                            if show_event_form() {
                                div { style: "padding: 12px; background: var(--color-bg); border-radius: var(--radius); margin-bottom: 12px;",
                                    if let Some(err) = event_form_error() {
                                        div { class: "error-msg", "{err}" }
                                    }
                                    div { class: "form-row",
                                        div { class: "form-group",
                                            label { "Type" }
                                            select {
                                                value: "{event_form_type}",
                                                oninput: move |e: Event<FormData>| event_form_type.set(e.value()),
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
                                                option { value: "Other", "Other" }
                                            }
                                        }
                                        div { class: "form-group",
                                            label { "Date" }
                                            input {
                                                r#type: "text",
                                                placeholder: "e.g. 1 Jan 1900",
                                                value: "{event_form_date}",
                                                oninput: move |e: Event<FormData>| event_form_date.set(e.value()),
                                            }
                                        }
                                    }
                                    div { class: "form-row",
                                        div { class: "form-group",
                                            label { "Place" }
                                            select {
                                                value: "{event_form_place_id}",
                                                oninput: move |e: Event<FormData>| event_form_place_id.set(e.value()),
                                                option { value: "", "-- No place --" }
                                                for (pid, pname) in place_options.iter() {
                                                    option { value: "{pid}", "{pname}" }
                                                }
                                            }
                                        }
                                        div { class: "form-group",
                                            label { "Description" }
                                            input {
                                                r#type: "text",
                                                value: "{event_form_desc}",
                                                oninput: move |e: Event<FormData>| event_form_desc.set(e.value()),
                                            }
                                        }
                                    }
                                    button {
                                        class: "btn btn-primary btn-sm",
                                        onclick: on_create_event,
                                        "Create Event"
                                    }
                                }
                            }

                            if other_events.is_empty() {
                                div { class: "empty-state",
                                    p { "No other events recorded." }
                                }
                            } else {
                                for ev in other_events.iter() {
                                    {
                                        let eid = ev.id;
                                        let et = format!("{:?}", ev.event_type);
                                        let date = ev.date_value.clone().unwrap_or_default();
                                        let place = ev.place_id.map(&place_name).unwrap_or_default();
                                        let desc = ev.description.clone().unwrap_or_default();
                                        rsx! {
                                            div { class: "person-form-item",
                                                div { class: "person-form-item-info",
                                                    span { class: "badge", "{et}" }
                                                    if !date.is_empty() { span { "{date}" } }
                                                    if !place.is_empty() { span { class: "text-muted", "@ {place}" } }
                                                    if !desc.is_empty() { span { class: "text-muted", "— {desc}" } }
                                                }
                                                div { class: "person-form-item-actions",
                                                    button {
                                                        class: "btn btn-danger btn-sm",
                                                        onclick: {
                                                            let api = api_del_event.clone();
                                                            move |_| {
                                                                let api = api.clone();
                                                                spawn(async move {
                                                                    match api.delete_event(tid, eid).await {
                                                                        Ok(_) => {
                                                                            on_saved_event_del.call(());
                                                                            refresh += 1;
                                                                        }
                                                                        Err(e) => save_error.set(Some(format!("{e}"))),
                                                                    }
                                                                });
                                                            }
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

                    // ── Notes tab ──
                    if active_tab() == "notes" {
                        div { class: "person-form-section",
                            div { class: "section-header",
                                h3 { style: "font-size: 0.95rem;", "Notes" }
                                button {
                                    class: "btn btn-primary btn-sm",
                                    onclick: move |_| show_note_form.toggle(),
                                    if show_note_form() { "Cancel" } else { "Add Note" }
                                }
                            }

                            if show_note_form() {
                                div { style: "padding: 12px; background: var(--color-bg); border-radius: var(--radius); margin-bottom: 12px;",
                                    if let Some(err) = note_form_error() {
                                        div { class: "error-msg", "{err}" }
                                    }
                                    div { class: "form-group",
                                        textarea {
                                            rows: 4,
                                            placeholder: "Enter note text...",
                                            value: "{note_form_text}",
                                            oninput: move |e: Event<FormData>| note_form_text.set(e.value()),
                                        }
                                    }
                                    button {
                                        class: "btn btn-primary btn-sm",
                                        onclick: on_create_note,
                                        "Create Note"
                                    }
                                }
                            }

                            if notes_list.is_empty() {
                                div { class: "empty-state",
                                    p { "No notes recorded." }
                                }
                            } else {
                                for note in notes_list.iter() {
                                    {
                                        let nid = note.id;
                                        let text = note.text.clone();
                                        let preview = if text.len() > 120 {
                                            format!("{}...", &text[..120])
                                        } else {
                                            text
                                        };
                                        rsx! {
                                            div { class: "person-form-item",
                                                div { class: "person-form-item-info",
                                                    span { "{preview}" }
                                                }
                                                div { class: "person-form-item-actions",
                                                    button {
                                                        class: "btn btn-danger btn-sm",
                                                        onclick: {
                                                            let api = api_del_note.clone();
                                                            move |_| {
                                                                let api = api.clone();
                                                                spawn(async move {
                                                                    match api.delete_note(tid, nid).await {
                                                                        Ok(_) => {
                                                                            on_saved_note_del.call(());
                                                                            refresh += 1;
                                                                        }
                                                                        Err(e) => save_error.set(Some(format!("{e}"))),
                                                                    }
                                                                });
                                                            }
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
    }
}

// ── Helper: name form fields ──

#[allow(clippy::too_many_arguments)]
fn render_name_form(
    error: &Signal<Option<String>>,
    name_type_mut: &mut Signal<String>,
    given_mut: &mut Signal<String>,
    surname_mut: &mut Signal<String>,
    prefix_mut: &mut Signal<String>,
    suffix_mut: &mut Signal<String>,
    nickname_mut: &mut Signal<String>,
    primary_mut: &mut Signal<bool>,
    on_create: impl FnMut(Event<MouseData>) + 'static,
) -> Element {
    let mut name_type_sig = *name_type_mut;
    let mut given_sig = *given_mut;
    let mut surname_sig = *surname_mut;
    let mut prefix_sig = *prefix_mut;
    let mut suffix_sig = *suffix_mut;
    let mut nickname_sig = *nickname_mut;
    let mut primary_sig = *primary_mut;

    rsx! {
        div { style: "padding: 12px; background: var(--color-bg); border-radius: var(--radius); margin-bottom: 12px;",
            if let Some(err) = error() {
                div { class: "error-msg", "{err}" }
            }
            div { class: "form-row",
                div { class: "form-group",
                    label { "Name Type" }
                    select {
                        value: "{name_type_sig}",
                        oninput: move |e: Event<FormData>| name_type_sig.set(e.value()),
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
                        value: if primary_sig() { "true" } else { "false" },
                        oninput: move |e: Event<FormData>| primary_sig.set(e.value() == "true"),
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
                        value: "{given_sig}",
                        oninput: move |e: Event<FormData>| given_sig.set(e.value()),
                    }
                }
                div { class: "form-group",
                    label { "Surname" }
                    input {
                        r#type: "text",
                        placeholder: "e.g. Dupont",
                        value: "{surname_sig}",
                        oninput: move |e: Event<FormData>| surname_sig.set(e.value()),
                    }
                }
            }
            div { class: "form-row",
                div { class: "form-group",
                    label { "Prefix" }
                    input {
                        r#type: "text",
                        placeholder: "e.g. Dr.",
                        value: "{prefix_sig}",
                        oninput: move |e: Event<FormData>| prefix_sig.set(e.value()),
                    }
                }
                div { class: "form-group",
                    label { "Suffix" }
                    input {
                        r#type: "text",
                        placeholder: "e.g. Jr.",
                        value: "{suffix_sig}",
                        oninput: move |e: Event<FormData>| suffix_sig.set(e.value()),
                    }
                }
                div { class: "form-group",
                    label { "Nickname" }
                    input {
                        r#type: "text",
                        placeholder: "e.g. JP",
                        value: "{nickname_sig}",
                        oninput: move |e: Event<FormData>| nickname_sig.set(e.value()),
                    }
                }
            }
            button { class: "btn btn-primary btn-sm", onclick: on_create, "Create Name" }
        }
    }
}
