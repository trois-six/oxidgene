//! Modal-based person edit form — single scrollable body with section dividers.
//!
//! Sections: Civil Status · Birth · Death · Privacy · Other Events · Notes.
//! A single footer Save button persists sex + privacy + birth event + death event
//! and closes the modal. Name, event, and note CRUD use inline per-item saves.
//!
//! Every date in the form — birth, death, professions, other events — is edited
//! through the one [`DateInput`] widget, so calendar and qualifier sit with the
//! date they qualify rather than in a panel of their own. Witnesses likewise
//! live in the event's own block.

use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::{
    AddChildBody, AddSpouseBody, ApiClient, ApiError, CreateCitationBody, CreateEventBody,
    CreateNoteBody, CreatePersonBody, CreatePersonNameBody, CreateSourceBody, UpdateCitationBody,
    UpdateEventBody, UpdateNoteBody, UpdatePersonBody, UpdatePersonNameBody,
};
use crate::components::date_input::{DateInput, DateParts, format_event_date};
use crate::i18n::use_i18n;
use crate::utils::{
    event_type_label_key, name_type_label_key, name_type_value, opt_str, parse_event_type,
    parse_name_type, parse_place_id, parse_privacy, parse_sex,
};
use oxidgene_core::types::{Event as CoreEvent, Note as CoreNote};
use oxidgene_core::types::{split_surname_at_head, split_surname_particle};
use oxidgene_core::{ChildType, Confidence, EventType, NameType, SpouseRole};

// ── Props ────────────────────────────────────────────────────────────────

/// Context that determines which relationship is wired on create-mode save.
#[derive(Debug, Clone, PartialEq)]
pub enum PersonFormCreateContext {
    Standalone,
    AddParent {
        child_id: Uuid,
        family_id: Option<Uuid>,
        is_father: bool,
    },
}

#[derive(Props, Clone, PartialEq)]
pub struct PersonFormProps {
    pub tree_id: Uuid,
    /// Edit mode: Some(person_uuid). Absent/None in create mode.
    #[props(default)]
    pub person_id: Option<Uuid>,
    /// If Some, the form opens in create mode.
    #[props(default)]
    pub create_context: Option<PersonFormCreateContext>,
    /// When true, renders just the body + a single Save button — no backdrop,
    /// header, Cancel button, or delete section. Used to embed a person's
    /// fields inside another modal (e.g. the couple edit modal).
    #[props(default)]
    pub embedded: bool,
    pub on_close: EventHandler<()>,
    pub on_saved: EventHandler<()>,
}

// ── Component ────────────────────────────────────────────────────────────

#[component]
pub fn PersonForm(props: PersonFormProps) -> Element {
    let api = use_context::<ApiClient>();
    let i18n = use_i18n();
    let mut refresh = use_signal(|| 0u32);

    let is_create = props.create_context.is_some();
    let is_embedded = props.embedded;
    let tid = props.tree_id;
    let pid = props.person_id.unwrap_or_default();

    // ── Sex & Privacy ──
    let mut sex_val = use_signal(|| "Unknown".to_string());
    let mut sex_loaded = use_signal(|| false);
    let mut privacy_val = use_signal(|| "Default".to_string());
    let mut privacy_loaded = use_signal(|| false);

    // ── Birth identity (mandatory surname + given names) ──
    // Managed directly under Civil Status rather than through the generic
    // name-CRUD list below — there is always exactly one Birth/primary name.
    let mut birth_name_id = use_signal(|| None::<Uuid>);
    let mut birth_identity_loaded = use_signal(|| false);
    let mut birth_given = use_signal(String::new);
    let mut birth_surname = use_signal(String::new);
    // `None` = trust automatic particle detection; `Some(p)` = the user took
    // control, where an empty `p` means "no particle".
    let mut birth_particle_override = use_signal(|| None::<String>);

    // ── Additional information (name) CRUD state ──
    //
    // "Type d'information" reuses the existing NameType variants — Alias,
    // Surnom and Sobriquet are UI-only vocabulary for AlsoKnownAs (GEDCOM
    // AKA), not new name types (see parse_name_type). Each entry is just a
    // single value: a name-like string for every type except Sobriquet,
    // which fills the nickname piece instead.
    let mut show_name_form = use_signal(|| false);
    let mut name_form_type = use_signal(|| "Married".to_string());
    let mut name_form_value = use_signal(String::new);
    let mut name_form_particle_override = use_signal(|| None::<String>);
    let mut name_form_error = use_signal(|| None::<String>);

    let mut editing_name_id = use_signal(|| None::<Uuid>);
    let mut edit_name_type = use_signal(|| "Birth".to_string());
    let mut edit_name_given = use_signal(String::new);
    let mut edit_name_surname = use_signal(String::new);
    // Same contract as `birth_particle_override`: `None` trusts detection,
    // `Some(p)` is the user's call (empty = "no particle here").
    let mut edit_name_particle_override = use_signal(|| None::<String>);
    let mut edit_name_prefix = use_signal(String::new);
    let mut edit_name_suffix = use_signal(String::new);
    let mut edit_name_nickname = use_signal(String::new);
    let mut edit_name_primary = use_signal(|| false);
    let mut edit_name_error = use_signal(|| None::<String>);

    // ── Birth state ──
    // Calendar, qualifier and the day/month/year triplet all live in DateParts,
    // which is what the DateInput widget edits.
    let mut birth_parts = use_signal(DateParts::default);
    let mut birth_place_id = use_signal(String::new);
    let mut birth_event_id = use_signal(|| None::<Uuid>);

    // ── Death state ──
    let mut death_parts = use_signal(DateParts::default);
    let mut death_place_id = use_signal(String::new);
    let mut death_event_id = use_signal(|| None::<Uuid>);

    let mut birth_death_loaded = use_signal(|| false);

    // ── Notes + source (see NotesSource) ──
    // Birth and death ride the footer Save like the rest of their section;
    // the person-level pair only exposes a source, since the person's notes
    // are the list under Civil Status.
    let mut birth_notes = use_signal(String::new);
    let mut birth_source = use_signal(String::new);
    let mut birth_ns = use_signal(NotesSource::default);
    let mut death_notes = use_signal(String::new);
    let mut death_source = use_signal(String::new);
    let mut death_ns = use_signal(NotesSource::default);
    let mut bd_ns_loaded = use_signal(|| false);
    let mut person_source = use_signal(String::new);
    let mut person_ns = use_signal(NotesSource::default);
    let mut person_ns_loaded = use_signal(|| false);
    // Which event row in the profession / other-event lists has its notes
    // panel open — at most one at a time.
    let mut open_event_notes = use_signal(|| None::<Uuid>);

    // ── Profession(s) CRUD state ──
    let mut show_profession_form = use_signal(|| false);
    let mut profession_form_label = use_signal(String::new);
    let mut profession_form_parts = use_signal(DateParts::default);
    let mut profession_form_place_id = use_signal(String::new);
    let mut profession_form_notes = use_signal(String::new);
    let mut profession_form_source = use_signal(String::new);
    let mut profession_form_error = use_signal(|| None::<String>);

    // ── Other event CRUD state ──
    let mut show_event_form = use_signal(|| false);
    let mut event_form_type = use_signal(|| "Baptism".to_string());
    let mut event_form_parts = use_signal(DateParts::default);
    let mut event_form_place_id = use_signal(String::new);
    let mut event_form_description = use_signal(String::new);
    let mut event_form_cause = use_signal(String::new);
    let mut event_form_notes = use_signal(String::new);
    let mut event_form_source = use_signal(String::new);
    let mut event_form_error = use_signal(|| None::<String>);

    // ── Note CRUD state ──
    let mut show_note_form = use_signal(|| false);
    let mut note_form_text = use_signal(String::new);
    let mut note_form_error = use_signal(|| None::<String>);
    let mut editing_note_id = use_signal(|| None::<Uuid>);
    let mut edit_note_text = use_signal(String::new);
    let mut edit_note_error = use_signal(|| None::<String>);

    // ── Section fold state ──
    // Every section opens with the form; folding one is a way to get it out of
    // the way, not a step to go through before the fields are reachable.
    let open_civil = use_signal(|| true);
    let open_birth = use_signal(|| true);
    let open_death = use_signal(|| true);
    let open_privacy = use_signal(|| true);
    let open_events = use_signal(|| true);

    // ── UI state ──
    let mut saving = use_signal(|| false);
    let mut save_error = use_signal(|| None::<String>);
    let mut has_changes = use_signal(|| false);
    let mut show_discard_confirm = use_signal(|| false);
    let mut delete_error = use_signal(|| None::<String>);
    let mut deleting = use_signal(|| false);

    // ── Resources ──

    let api_person = api.clone();
    let person_resource = use_resource(move || {
        let api = api_person.clone();
        let _tick = refresh();
        async move {
            if is_create {
                return Err(crate::api::ApiError::Api {
                    status: 0,
                    body: String::new(),
                });
            }
            api.get_person(tid, pid).await
        }
    });

    let api_names = api.clone();
    let names_resource = use_resource(move || {
        let api = api_names.clone();
        let _tick = refresh();
        async move {
            if is_create {
                return Ok(vec![]);
            }
            api.list_person_names(tid, pid).await
        }
    });

    let api_events = api.clone();
    let events_resource = use_resource(move || {
        let api = api_events.clone();
        let _tick = refresh();
        async move {
            if is_create {
                return Err(crate::api::ApiError::Api {
                    status: 0,
                    body: String::new(),
                });
            }
            api.list_events(tid, Some(100), None, None, Some(pid), None)
                .await
        }
    });

    let api_places = api.clone();
    let places_resource = use_resource(move || {
        let api = api_places.clone();
        let _tick = refresh();
        // Every page: an event may sit on any place in the tree, and a place
        // missing from this list has no name to show.
        async move { api.list_all_places(tid).await }
    });

    let api_notes = api.clone();
    let notes_resource = use_resource(move || {
        let api = api_notes.clone();
        let _tick = refresh();
        async move {
            if is_create {
                return Err(crate::api::ApiError::Api {
                    status: 0,
                    body: String::new(),
                });
            }
            api.list_notes(tid, Some(pid), None, None, None).await
        }
    });

    // Birth and death notes/source in one resource: it re-reads the events
    // itself (the GET is cached) so the ids it loads against are always the
    // ones it just saw, rather than a signal that may not have caught up.
    let api_bd_ns = api.clone();
    let bd_ns_resource = use_resource(move || {
        let api = api_bd_ns.clone();
        let _tick = refresh();
        async move {
            if is_create {
                return (NotesSource::default(), NotesSource::default());
            }
            let (mut birth_eid, mut death_eid) = (None, None);
            if let Ok(conn) = api
                .list_events(tid, Some(100), None, None, Some(pid), None)
                .await
            {
                for edge in &conn.edges {
                    match edge.node.event_type {
                        EventType::Birth => birth_eid = Some(edge.node.id),
                        EventType::Death => death_eid = Some(edge.node.id),
                        _ => {}
                    }
                }
            }
            let birth = match birth_eid {
                Some(eid) => load_notes_source(&api, tid, Some(pid), Some(eid)).await,
                None => NotesSource::default(),
            };
            let death = match death_eid {
                Some(eid) => load_notes_source(&api, tid, Some(pid), Some(eid)).await,
                None => NotesSource::default(),
            };
            (birth, death)
        }
    });

    let api_person_ns = api.clone();
    let person_ns_resource = use_resource(move || {
        let api = api_person_ns.clone();
        let _tick = refresh();
        async move {
            if is_create {
                return NotesSource::default();
            }
            load_notes_source(&api, tid, Some(pid), None).await
        }
    });

    // ── Populate sex + privacy (once) ──

    // Create mode: pre-fill sex from context (once).
    if is_create && !sex_loaded() {
        if let Some(PersonFormCreateContext::AddParent { is_father, .. }) = &props.create_context {
            sex_val.set(if *is_father {
                "Male".to_string()
            } else {
                "Female".to_string()
            });
        }
        sex_loaded.set(true);
        privacy_loaded.set(true);
    }

    if !sex_loaded()
        && let Some(Ok(person)) = &*person_resource.read()
    {
        sex_val.set(format!("{:?}", person.sex));
        sex_loaded.set(true);
    }
    if !privacy_loaded()
        && let Some(Ok(person)) = &*person_resource.read()
    {
        privacy_val.set(format!("{:?}", person.privacy));
        privacy_loaded.set(true);
    }

    // ── Populate birth/death (once) ──
    if !birth_death_loaded()
        && let Some(Ok(conn)) = &*events_resource.read()
    {
        for edge in &conn.edges {
            let ev = &edge.node;
            match ev.event_type {
                EventType::Birth => {
                    birth_event_id.set(Some(ev.id));
                    birth_parts.set(DateParts::from_fields(
                        ev.calendar,
                        ev.date_qualifier,
                        ev.date_value.as_deref(),
                        ev.date_value2.as_deref(),
                    ));
                    birth_place_id.set(ev.place_id.map(|id| id.to_string()).unwrap_or_default());
                }
                EventType::Death => {
                    death_event_id.set(Some(ev.id));
                    death_parts.set(DateParts::from_fields(
                        ev.calendar,
                        ev.date_qualifier,
                        ev.date_value.as_deref(),
                        ev.date_value2.as_deref(),
                    ));
                    death_place_id.set(ev.place_id.map(|id| id.to_string()).unwrap_or_default());
                }
                _ => {}
            }
        }
        birth_death_loaded.set(true);
    }

    // ── Populate notes + source (once) ──
    if !is_create
        && !bd_ns_loaded()
        && let Some((birth, death)) = &*bd_ns_resource.read()
    {
        birth_notes.set(birth.notes.clone());
        birth_source.set(birth.source_title.clone());
        birth_ns.set(birth.clone());
        death_notes.set(death.notes.clone());
        death_source.set(death.source_title.clone());
        death_ns.set(death.clone());
        bd_ns_loaded.set(true);
    }

    if !is_create
        && !person_ns_loaded()
        && let Some(ns) = &*person_ns_resource.read()
    {
        person_source.set(ns.source_title.clone());
        person_ns.set(ns.clone());
        person_ns_loaded.set(true);
    }

    // ── Populate birth identity (once) ──
    if !is_create
        && !birth_identity_loaded()
        && let Some(Ok(names)) = &*names_resource.read()
    {
        let primary = names
            .iter()
            .find(|n| n.name_type == NameType::Birth && n.is_primary)
            .or_else(|| names.iter().find(|n| n.is_primary))
            .or_else(|| names.iter().find(|n| n.name_type == NameType::Birth));
        if let Some(n) = primary {
            birth_name_id.set(Some(n.id));
            birth_given.set(n.given_names.clone().unwrap_or_default());
            // Edited as one field, particle included — re-split on save.
            let full = n.full_surname().unwrap_or_default();
            birth_particle_override.set(override_for_stored(&full, n.surname_prefix.as_deref()));
            birth_surname.set(full);
        }
        birth_identity_loaded.set(true);
    }

    // ── Derived ──

    let display_name: String = if is_create {
        i18n.t("person_form.new_person")
    } else {
        match &*names_resource.read() {
            Some(Ok(names)) => {
                let primary = names.iter().find(|n| n.is_primary).or(names.first());
                match primary {
                    Some(n) => {
                        let dn = n.display_name();
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
        }
    };

    let other_events: Vec<CoreEvent> = match &*events_resource.read() {
        Some(Ok(conn)) => conn
            .edges
            .iter()
            .filter(|e| {
                !matches!(
                    e.node.event_type,
                    EventType::Birth | EventType::Death | EventType::Occupation
                )
            })
            .map(|e| e.node.clone())
            .collect(),
        _ => vec![],
    };

    let professions: Vec<CoreEvent> = match &*events_resource.read() {
        Some(Ok(conn)) => conn
            .edges
            .iter()
            .filter(|e| e.node.event_type == EventType::Occupation)
            .map(|e| e.node.clone())
            .collect(),
        _ => vec![],
    };

    // Person-scoped notes only: a note stamped with an event belongs to that
    // event's own panel, not to the person's list. (Older rows carry both
    // ids, which is why this filters rather than trusting the query.)
    let notes_list: Vec<CoreNote> = match &*notes_resource.read() {
        Some(Ok(notes)) => notes
            .iter()
            .filter(|n| n.event_id.is_none())
            .cloned()
            .collect(),
        _ => vec![],
    };

    // An unknown or not-yet-loaded place resolves to nothing rather than to a
    // slice of its UUID: callers hide the place when this is empty, and a bare
    // "019fccf3" told the reader less than showing no place at all.
    let place_name = |place_id: Uuid| -> String {
        let data = places_resource.read();
        match &*data {
            Some(Ok(places)) => places
                .iter()
                .find(|p| p.id == place_id)
                .map(|p| p.name.clone())
                .unwrap_or_default(),
            _ => String::new(),
        }
    };

    let place_options: Vec<(String, String)> = {
        let data = places_resource.read();
        match &*data {
            Some(Ok(places)) => places
                .iter()
                .map(|p| (p.id.to_string(), p.name.clone()))
                .collect(),
            _ => vec![],
        }
    };

    // ── Handlers ──

    let api_create_name = api.clone();
    let on_saved_name = props.on_saved;
    let on_create_name = move |_| {
        let api = api_create_name.clone();
        let value = name_form_value().trim().to_string();
        let info_type_str = name_form_type();
        let birth_surname_val = birth_surname().trim().to_string();
        let particle_override = name_form_particle_override();
        let birth_override = birth_particle_override();
        spawn(async move {
            if value.is_empty() {
                name_form_error.set(Some(i18n.t("person_form.information_value_required")));
                return;
            }
            let body = build_information_body(
                &info_type_str,
                &value,
                &birth_surname_val,
                particle_override.as_deref(),
                birth_override.as_deref(),
            );
            match api.create_person_name(tid, pid, &body).await {
                Ok(_) => {
                    show_name_form.set(false);
                    name_form_value.set(String::new());
                    name_form_particle_override.set(None);
                    name_form_type.set("Married".to_string());
                    name_form_error.set(None);
                    on_saved_name.call(());
                    refresh += 1;
                }
                Err(e) => name_form_error.set(Some(format!("{e}"))),
            }
        });
    };

    let api_edit_name = api.clone();
    let on_saved_name_edit = props.on_saved;
    let api_del_name = api.clone();
    let on_saved_name_del = props.on_saved;

    let api_create_event = api.clone();
    let on_saved_event = props.on_saved;
    let on_create_event = move |_| {
        let api = api_create_event.clone();
        let event_type_str = event_form_type();
        let parts = event_form_parts();
        let place_str = event_form_place_id();
        let desc = event_form_description().trim().to_string();
        let cause = event_form_cause().trim().to_string();
        let notes = event_form_notes().trim().to_string();
        let source = event_form_source();
        spawn(async move {
            if let Some(key) = parts.validate() {
                event_form_error.set(Some(i18n.t(key)));
                return;
            }
            let body = create_event_body(
                parse_event_type(&event_type_str),
                &parts,
                &place_str,
                EventOwner::Person(pid),
                opt_str(&desc),
                opt_str(&cause),
            );
            match api.create_event(tid, &body).await {
                Ok(new_event) => {
                    let _ = save_notes_source(
                        &api,
                        tid,
                        Some(pid),
                        Some(new_event.id),
                        &notes,
                        &source,
                        &NotesSource::default(),
                    )
                    .await;
                    show_event_form.set(false);
                    event_form_type.set("Baptism".to_string());
                    event_form_parts.set(DateParts::default());
                    event_form_place_id.set(String::new());
                    event_form_description.set(String::new());
                    event_form_cause.set(String::new());
                    event_form_notes.set(String::new());
                    event_form_source.set(String::new());
                    event_form_error.set(None);
                    on_saved_event.call(());
                    refresh += 1;
                }
                Err(e) => event_form_error.set(Some(format!("{e}"))),
            }
        });
    };

    let api_del_event = api.clone();
    let on_saved_event_del = props.on_saved;

    let api_create_profession = api.clone();
    let on_saved_profession = props.on_saved;
    let on_create_profession = move |_| {
        let api = api_create_profession.clone();
        let label = profession_form_label().trim().to_string();
        let parts = profession_form_parts();
        let place_str = profession_form_place_id();
        let notes = profession_form_notes().trim().to_string();
        let source = profession_form_source();
        spawn(async move {
            if label.is_empty() {
                profession_form_error.set(Some(i18n.t("person_form.profession_required")));
                return;
            }
            if let Some(key) = parts.validate() {
                profession_form_error.set(Some(i18n.t(key)));
                return;
            }
            let body = create_event_body(
                EventType::Occupation,
                &parts,
                &place_str,
                EventOwner::Person(pid),
                opt_str(&label),
                None,
            );
            match api.create_event(tid, &body).await {
                Ok(new_event) => {
                    let _ = save_notes_source(
                        &api,
                        tid,
                        Some(pid),
                        Some(new_event.id),
                        &notes,
                        &source,
                        &NotesSource::default(),
                    )
                    .await;
                    show_profession_form.set(false);
                    profession_form_label.set(String::new());
                    profession_form_parts.set(DateParts::default());
                    profession_form_place_id.set(String::new());
                    profession_form_notes.set(String::new());
                    profession_form_source.set(String::new());
                    profession_form_error.set(None);
                    on_saved_profession.call(());
                    refresh += 1;
                }
                Err(e) => profession_form_error.set(Some(format!("{e}"))),
            }
        });
    };

    let api_del_profession = api.clone();
    let on_saved_profession_del = props.on_saved;

    let api_create_note = api.clone();
    let on_saved_note = props.on_saved;
    let on_create_note = move |_| {
        let api = api_create_note.clone();
        let text = note_form_text().trim().to_string();
        spawn(async move {
            if text.is_empty() {
                note_form_error.set(Some(i18n.t("person_form.note_required")));
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

    let api_del_note = api.clone();
    let on_saved_note_del = props.on_saved;

    let api_edit_note = api.clone();
    let on_saved_note_edit = props.on_saved;

    // ── Unified footer Save / Create ──
    let api_save = api.clone();
    let on_save = {
        let on_saved = props.on_saved;
        let on_close = props.on_close;
        let create_ctx = props.create_context.clone();
        move |_| {
            let api = api_save.clone();
            let ctx = create_ctx.clone();
            let sex_str = sex_val();
            let privacy_str = privacy_val();
            // Birth identity (mandatory surname + given names)
            let bn_given = birth_given().trim().to_string();
            let bn_surname = birth_surname().trim().to_string();
            let bn_particle_override = birth_particle_override();
            let bn_id = birth_name_id();
            // Event form values
            let birth_eid = birth_event_id();
            let death_eid = death_event_id();
            let b_parts = birth_parts();
            let b_place = birth_place_id();
            let d_parts = death_parts();
            let d_place = death_place_id();
            // Notes + source for both events and for the person.
            let b_notes = birth_notes();
            let b_source = birth_source();
            let b_ns = birth_ns();
            let d_notes = death_notes();
            let d_source = death_source();
            let d_ns = death_ns();
            let p_source = person_source();
            // Only the source travels with the person: its notes are the
            // list under Civil Status, each row managed on its own, so this
            // save must not touch them.
            let p_ns = NotesSource {
                notes: String::new(),
                note_id: None,
                citation_holds_notes: false,
                ..person_ns()
            };
            spawn(async move {
                if bn_given.is_empty() || bn_surname.is_empty() {
                    save_error.set(Some(i18n.t("person_form.birth_identity_required")));
                    return;
                }
                if let Some(key) = b_parts.validate().or_else(|| d_parts.validate()) {
                    save_error.set(Some(i18n.t(key)));
                    return;
                }

                saving.set(true);
                save_error.set(None);

                if let Some(context) = ctx {
                    // ── Create mode ──

                    // 1. Create person with sex.
                    let Ok(new_person) = api
                        .create_person(
                            tid,
                            &CreatePersonBody {
                                sex: parse_sex(&sex_str),
                            },
                        )
                        .await
                    else {
                        save_error.set(Some(i18n.t("person_form.create_failed")));
                        saving.set(false);
                        return;
                    };
                    let new_pid = new_person.id;

                    // 2. Create the mandatory birth name.
                    let bn_split = resolve_particle(&bn_surname, bn_particle_override.as_deref());
                    let (bn_particle, bn_root) = (bn_split.particle, bn_split.root);
                    let body = CreatePersonNameBody {
                        name_type: NameType::Birth,
                        given_names: Some(bn_given.clone()),
                        surname: Some(bn_root),
                        surname_prefix: bn_particle,
                        prefix: None,
                        suffix: None,
                        nickname: None,
                        is_primary: true,
                        sort_order: 0,
                    };
                    if let Err(e) = api.create_person_name(tid, new_pid, &body).await {
                        save_error.set(Some(format!("{e}")));
                        saving.set(false);
                        return;
                    }

                    // 3. Birth and death events, each with its notes + source.
                    for (event_type, parts, place, notes, source) in [
                        (EventType::Birth, &b_parts, &b_place, &b_notes, &b_source),
                        (EventType::Death, &d_parts, &d_place, &d_notes, &d_source),
                    ] {
                        if let Err(e) = save_vital_event(
                            &api,
                            tid,
                            new_pid,
                            event_type,
                            None,
                            parts,
                            place,
                            notes,
                            source,
                            &NotesSource::default(),
                        )
                        .await
                        {
                            save_error.set(Some(format!("{e}")));
                            saving.set(false);
                            return;
                        }
                    }

                    // 4. Person-level source.
                    let _ = save_notes_source(
                        &api,
                        tid,
                        Some(new_pid),
                        None,
                        "",
                        &p_source,
                        &NotesSource::default(),
                    )
                    .await;

                    // 5. Wire relationship.
                    match context {
                        PersonFormCreateContext::AddParent {
                            child_id,
                            family_id,
                            is_father,
                        } => {
                            let fid = if let Some(fid) = family_id {
                                fid
                            } else {
                                let Ok(family) = api.create_family(tid).await else {
                                    save_error.set(Some(i18n.t("person_form.create_failed")));
                                    saving.set(false);
                                    return;
                                };
                                let child_body = AddChildBody {
                                    person_id: child_id,
                                    child_type: ChildType::Biological,
                                    sort_order: 0,
                                };
                                let _ = api.add_child(tid, family.id, &child_body).await;
                                family.id
                            };
                            let role = if is_father {
                                SpouseRole::Husband
                            } else {
                                SpouseRole::Wife
                            };
                            let spouse_body = AddSpouseBody {
                                person_id: new_pid,
                                role,
                                sort_order: 0,
                            };
                            if let Err(e) = api.add_spouse(tid, fid, &spouse_body).await {
                                save_error.set(Some(format!("{e}")));
                                saving.set(false);
                                return;
                            }
                        }
                        PersonFormCreateContext::Standalone => {}
                    }
                } else {
                    // ── Edit mode ──

                    // 1. Update person sex + privacy.
                    let person_body = UpdatePersonBody {
                        sex: Some(parse_sex(&sex_str)),
                        privacy: Some(parse_privacy(&privacy_str)),
                    };
                    if let Err(e) = api.update_person(tid, pid, &person_body).await {
                        save_error.set(Some(format!("{e}")));
                        saving.set(false);
                        return;
                    }

                    // 1b. Birth name (surname + given names).
                    let bn_split = resolve_particle(&bn_surname, bn_particle_override.as_deref());
                    let (bn_particle, bn_root) = (bn_split.particle, bn_split.root);
                    if let Some(bnid) = bn_id {
                        let name_body = UpdatePersonNameBody {
                            name_type: None,
                            given_names: Some(opt_str(&bn_given)),
                            surname: Some(opt_str(&bn_root)),
                            surname_prefix: Some(bn_particle.clone()),
                            prefix: None,
                            suffix: None,
                            nickname: None,
                            is_primary: None,
                            sort_order: None,
                        };
                        if let Err(e) = api.update_person_name(tid, pid, bnid, &name_body).await {
                            save_error.set(Some(format!("{e}")));
                            saving.set(false);
                            return;
                        }
                    } else {
                        let name_body = CreatePersonNameBody {
                            name_type: NameType::Birth,
                            given_names: Some(bn_given.clone()),
                            surname: Some(bn_root),
                            surname_prefix: bn_particle,
                            prefix: None,
                            suffix: None,
                            nickname: None,
                            is_primary: true,
                            sort_order: 0,
                        };
                        if let Err(e) = api.create_person_name(tid, pid, &name_body).await {
                            save_error.set(Some(format!("{e}")));
                            saving.set(false);
                            return;
                        }
                    }

                    // 2. Birth and death events, each with its notes + source.
                    //    The stored state comes back so that pressing Save
                    //    again reconciles against those rows rather than
                    //    creating a second set.
                    for (event_type, existing, parts, place, notes, source, ns, mut target) in [
                        (
                            EventType::Birth,
                            birth_eid,
                            &b_parts,
                            &b_place,
                            &b_notes,
                            &b_source,
                            &b_ns,
                            birth_ns,
                        ),
                        (
                            EventType::Death,
                            death_eid,
                            &d_parts,
                            &d_place,
                            &d_notes,
                            &d_source,
                            &d_ns,
                            death_ns,
                        ),
                    ] {
                        match save_vital_event(
                            &api, tid, pid, event_type, existing, parts, place, notes, source, ns,
                        )
                        .await
                        {
                            Ok(Some(stored)) => target.set(stored),
                            Ok(None) => {}
                            Err(e) => {
                                save_error.set(Some(format!("{e}")));
                                saving.set(false);
                                return;
                            }
                        }
                    }

                    // 3. Person-level source (its notes are the list under
                    //    Civil Status, saved row by row).
                    if let Ok(stored) =
                        save_notes_source(&api, tid, Some(pid), None, "", &p_source, &p_ns).await
                    {
                        // The person's notes are the list above, not this
                        // pair — keep them out of the state it reconciles
                        // against, exactly as `p_ns` was built.
                        person_ns.set(NotesSource {
                            notes: String::new(),
                            note_id: None,
                            ..stored
                        });
                    }
                }

                saving.set(false);
                on_saved.call(());
                on_close.call(());
            });
        }
    };

    let try_close = move |_| {
        if has_changes() {
            show_discard_confirm.set(true);
        } else {
            props.on_close.call(());
        }
    };

    let api_delete = api.clone();
    let on_confirm_delete = {
        let on_saved = props.on_saved;
        let on_close = props.on_close;
        move |_| {
            let api = api_delete.clone();
            spawn(async move {
                deleting.set(true);
                delete_error.set(None);
                match api.delete_person(tid, pid).await {
                    Ok(_) => {
                        on_saved.call(());
                        on_close.call(());
                    }
                    Err(e) => {
                        delete_error.set(Some(format!("{e}")));
                        deleting.set(false);
                    }
                }
            });
        }
    };

    // ── Render ──

    let body = rsx! {
                // ── Scrollable body ──
                div { class: "person-form-body",

                    // ── Civil Status ──
                    FormSection { title: i18n.t("person_form.tab_civil"), open: open_civil,

                        // Birth name + given names — mandatory, always visible.
                        div { class: "form-row",
                            div { class: "form-group",
                                label { {i18n.t("name_type.birth")} " *" }
                                input {
                                    r#type: "text",
                                    placeholder: "{i18n.t(\"person_form.surname_placeholder\")}",
                                    value: "{birth_surname}",
                                    oninput: move |e: Event<FormData>| { birth_surname.set(e.value().to_uppercase()); has_changes.set(true); },
                                }
                                // Surface the split the save path will apply, and
                                // let the user correct it — detection is a guess.
                                {render_particle_row(&i18n, &birth_surname(), birth_particle_override)}
                            }
                            div { class: "form-group",
                                label { {i18n.t("person_form.given_names")} " *" }
                                input {
                                    r#type: "text",
                                    placeholder: "{i18n.t(\"person_form.given_placeholder\")}",
                                    value: "{birth_given}",
                                    oninput: move |e: Event<FormData>| { birth_given.set(e.value()); has_changes.set(true); },
                                }
                            }
                        }

                        div { class: "form-group",
                            label { {i18n.t("person_form.sex")} }
                            {render_choice_group(
                                &[
                                    ("Male",    i18n.t("sex.male")),
                                    ("Female",  i18n.t("sex.female")),
                                    ("Unknown", i18n.t("sex.unknown")),
                                ],
                                sex_val,
                                move || has_changes.set(true),
                            )}
                        }

                        // ── Profession(s) (edit mode only) ──
                        if !is_create {
                            div { class: "pf-subblock",
                                div { class: "pf-block-label",
                                    {i18n.t("person_form.professions")}
                                    {render_add_toggle(
                                        i18n.t("person_form.add_profession"),
                                        i18n.t("common.cancel"),
                                        show_profession_form,
                                    )}
                                }

                                if show_profession_form() {
                                    div { class: "pf-subform",
                                        if let Some(err) = profession_form_error() {
                                            div { class: "error-msg", "{err}" }
                                        }
                                        div { class: "form-group",
                                            label { {i18n.t("person_form.profession")} }
                                            input {
                                                r#type: "text",
                                                value: "{profession_form_label}",
                                                oninput: move |e: Event<FormData>| profession_form_label.set(e.value()),
                                            }
                                        }
                                        div { class: "form-group",
                                            label { {i18n.t("person_form.date")} }
                                            DateInput {
                                                parts: profession_form_parts,
                                                i18n,
                                                on_change: move |()| {},
                                            }
                                        }
                                        {render_place_select(&i18n, profession_form_place_id, &place_options, || {})}
                                        {render_notes_source_fields(&i18n, profession_form_notes, profession_form_source, || {})}
                                        button {
                                            class: "pf-confirm-btn",
                                            r#type: "button",
                                            onclick: on_create_profession,
                                            {i18n.t("person.create_profession")}
                                        }
                                    }
                                }

                                if professions.is_empty() {
                                    div { class: "pf-empty-item", p { {i18n.t("person_form.no_professions")} } }
                                } else {
                                    for ev in professions.iter() {
                                        {
                                            let eid = ev.id;
                                            let label = ev.description.clone().unwrap_or_default();
                                            let date = format_event_date(&i18n, ev);
                                            let place = ev.place_id.map(&place_name).unwrap_or_default();
                                            let notes_open = open_event_notes() == Some(eid);
                                            rsx! {
                                                div {
                                                    class: if notes_open { "person-form-item pf-compact-item pf-ns-open" } else { "person-form-item pf-compact-item" },
                                                    div { class: "person-form-item-info",
                                                        "{label}"
                                                        if !date.is_empty() { span { " \u{2014} {date}" } }
                                                        if !place.is_empty() { span { class: "text-muted", " @ {place}" } }
                                                    }
                                                    div { class: "person-form-item-actions",
                                                        button {
                                                            class: if notes_open { "pf-row-btn is-active" } else { "pf-row-btn" },
                                                            r#type: "button",
                                                            onclick: move |_| open_event_notes.set(if notes_open { None } else { Some(eid) }),
                                                            {i18n.t("common.edit")}
                                                        }
                                                        button {
                                                            class: "pf-row-btn is-danger",
                                                            r#type: "button",
                                                            onclick: {
                                                                let api = api_del_profession.clone();
                                                                move |_| {
                                                                    let api = api.clone();
                                                                    spawn(async move {
                                                                        match api.delete_event(tid, eid).await {
                                                                            Ok(_) => { on_saved_profession_del.call(()); refresh += 1; }
                                                                            Err(e) => save_error.set(Some(format!("{e}"))),
                                                                        }
                                                                    });
                                                                }
                                                            },
                                                            {i18n.t("common.delete")}
                                                        }
                                                    }
                                                }
                                                if notes_open {
                                                    EventEditor {
                                                        tree_id: tid,
                                                        person_id: Some(pid),
                                                        event: ev.clone(),
                                                        description_label: i18n.t("person_form.profession"),
                                                        place_options: place_options.clone(),
                                                        on_saved: move |_| { on_saved_profession_del.call(()); refresh += 1; },
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // ── Additional information (edit mode only) ──
                        if !is_create {
                        div { class: "pf-subblock",
                            div { class: "pf-block-label",
                                {i18n.t("person_form.tab_more_information")}
                                {render_add_toggle(
                                    i18n.t("person_form.add_information"),
                                    i18n.t("common.cancel"),
                                    show_name_form,
                                )}
                            }

                            if show_name_form() {
                                {render_information_form(
                                    &i18n,
                                    name_form_error,
                                    name_form_type,
                                    name_form_value,
                                    name_form_particle_override,
                                    on_create_name,
                                )}
                            }

                            match &*names_resource.read() {
                                Some(Ok(names)) => {
                                    let has_additional =
                                        names.iter().any(|n| Some(n.id) != birth_name_id());
                                    rsx! {
                                    if !has_additional {
                                        div { class: "pf-empty-item", p { {i18n.t("person_form.no_information")} } }
                                    }
                                    for name in names.iter().filter(|n| Some(n.id) != birth_name_id()) {
                                        {
                                            let nid = name.id;
                                            let is_editing = editing_name_id() == Some(nid);
                                            // The picker value, not the Debug spelling: they diverge
                                            // for the AKA refinements ("Byname" vs "Surnom"), and
                                            // feeding Debug back through parse_name_type silently
                                            // downgraded the entry to Other on save.
                                            let nt = name_type_value(name.name_type).to_string();
                                            let nt_label = i18n.t(name_type_label_key(name.name_type));
                                            let gn = name.given_names.clone().unwrap_or_default();
                                            let spfx = name.surname_prefix.clone().unwrap_or_default();
                                            // Edited and displayed as one field, particle
                                            // included — re-split on save.
                                            let sn_display = name.full_surname().unwrap_or_default();
                                            let pfx = name.prefix.clone().unwrap_or_default();
                                            let sfx = name.suffix.clone().unwrap_or_default();
                                            let nick = name.nickname.clone().unwrap_or_default();
                                            let prim = name.is_primary;
                                            // The collapsed row has to show whichever piece the
                                            // entry actually fills: an information may be a prefix
                                            // or a suffix alone, and rendering only given names and
                                            // surname left such a row blank.
                                            let display = [&pfx, &gn, &sn_display, &sfx]
                                                .iter()
                                                .filter(|p| !p.is_empty())
                                                .map(|p| p.as_str())
                                                .collect::<Vec<_>>()
                                                .join(" ");
                                            if is_editing {
                                                rsx! {
                                                    div { class: "person-form-item editing",
                                                        if let Some(err) = edit_name_error() {
                                                            div { class: "error-msg", "{err}" }
                                                        }
                                                        div { class: "form-row",
                                                            div { class: "form-group",
                                                                label { {i18n.t("person_form.name_type")} }
                                                                select {
                                                                    value: "{edit_name_type}",
                                                                    oninput: move |e: Event<FormData>| edit_name_type.set(e.value()),
                                                                    option { value: "Birth", {i18n.t("name_type.birth")} }
                                                                    option { value: "Married", {i18n.t("name_type.married")} }
                                                                    option { value: "AlsoKnownAs", {i18n.t("name_type.also_known_as")} }
                                                                    option { value: "Maiden", {i18n.t("name_type.maiden")} }
                                                                    option { value: "Religious", {i18n.t("name_type.religious")} }
                                                                    option { value: "Prenom", {i18n.t("name_type.prenom")} }
                                                                    option { value: "Alias", {i18n.t("name_type.alias")} }
                                                                    option { value: "Surnom", {i18n.t("name_type.surnom")} }
                                                                    option { value: "Sobriquet", {i18n.t("name_type.sobriquet")} }
                                                                    option { value: "Other", {i18n.t("name_type.other")} }
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
                                                                input { r#type: "text", value: "{edit_name_given}", oninput: move |e: Event<FormData>| edit_name_given.set(e.value()) }
                                                            }
                                                            div { class: "form-group",
                                                                label { {i18n.t("person_form.surname")} }
                                                                input { r#type: "text", value: "{edit_name_surname}", oninput: move |e: Event<FormData>| edit_name_surname.set(e.value().to_uppercase()) }
                                                                {render_particle_row(&i18n, &edit_name_surname(), edit_name_particle_override)}
                                                            }
                                                        }
                                                        div { class: "form-row",
                                                            div { class: "form-group",
                                                                label { {i18n.t("person_form.prefix")} }
                                                                input { r#type: "text", value: "{edit_name_prefix}", oninput: move |e: Event<FormData>| edit_name_prefix.set(e.value()) }
                                                            }
                                                            div { class: "form-group",
                                                                label { {i18n.t("person_form.suffix")} }
                                                                input { r#type: "text", value: "{edit_name_suffix}", oninput: move |e: Event<FormData>| edit_name_suffix.set(e.value()) }
                                                            }
                                                            div { class: "form-group",
                                                                label { {i18n.t("person_form.nickname")} }
                                                                input { r#type: "text", value: "{edit_name_nickname}", oninput: move |e: Event<FormData>| edit_name_nickname.set(e.value()) }
                                                            }
                                                        }
                                                        div { style: "display:flex;gap:8px;",
                                                            button {
                                                                class: "pf-confirm-btn",
                                                                r#type: "button",
                                                                onclick: {
                                                                    let api = api_edit_name.clone();
                                                                    move |_| {
                                                                        let api = api.clone();
                                                                        let Some(nid) = editing_name_id() else { return; };
                                                                        let given = edit_name_given().trim().to_string();
                                                                        let surname = edit_name_surname().trim().to_string();
                                                                        let particle_override = edit_name_particle_override();
                                                                        let prefix = edit_name_prefix().trim().to_string();
                                                                        let suffix = edit_name_suffix().trim().to_string();
                                                                        let nickname = edit_name_nickname().trim().to_string();
                                                                        let name_type_str = edit_name_type();
                                                                        let is_primary = edit_name_primary();
                                                                        spawn(async move {
                                                                            // The surname is entered whole, particle
                                                                            // included — detected here, and overridable,
                                                                            // exactly as the main name is.
                                                                            let split = resolve_particle(
                                                                                &surname,
                                                                                particle_override.as_deref(),
                                                                            );
                                                                            let (particle, root) =
                                                                                (split.particle, split.root);
                                                                            let body = UpdatePersonNameBody {
                                                                                name_type: Some(parse_name_type(&name_type_str)),
                                                                                given_names: Some(opt_str(&given)),
                                                                                surname: Some(opt_str(&root)),
                                                                                surname_prefix: Some(particle),
                                                                                prefix: Some(opt_str(&prefix)),
                                                                                suffix: Some(opt_str(&suffix)),
                                                                                nickname: Some(opt_str(&nickname)),
                                                                                is_primary: Some(is_primary),
                                                                                sort_order: None,
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
                                                                {i18n.t("common.save")}
                                                            }
                                                            button {
                                                                class: "pf-row-btn",
                                                                r#type: "button",
                                                                onclick: move |_| { editing_name_id.set(None); edit_name_error.set(None); },
                                                                {i18n.t("common.cancel")}
                                                            }
                                                        }
                                                    }
                                                }
                                            } else {
                                                rsx! {
                                                    div { class: "person-form-item pf-compact-item",
                                                        div { class: "person-form-item-info",
                                                            span { class: "badge", "{nt_label}" }
                                                            if !display.is_empty() {
                                                                strong { "{display}" }
                                                            }
                                                            if !nick.is_empty() {
                                                                span { class: "text-muted", "\u{201C}{nick}\u{201D}" }
                                                            }
                                                            if prim {
                                                                span { class: "badge badge-primary", {i18n.t("person_form.primary")} }
                                                            }
                                                        }
                                                        div { class: "person-form-item-actions",
                                                            button {
                                                                class: "pf-row-btn",
                                                                r#type: "button",
                                                                onclick: move |_| {
                                                                    editing_name_id.set(Some(nid));
                                                                    edit_name_type.set(nt.clone());
                                                                    edit_name_given.set(gn.clone());
                                                                    edit_name_surname.set(sn_display.clone());
                                                                    edit_name_particle_override.set(
                                                                        override_for_stored(&sn_display, opt_str(&spfx).as_deref()),
                                                                    );
                                                                    edit_name_prefix.set(pfx.clone());
                                                                    edit_name_suffix.set(sfx.clone());
                                                                    edit_name_nickname.set(nick.clone());
                                                                    edit_name_primary.set(prim);
                                                                    edit_name_error.set(None);
                                                                },
                                                                {i18n.t("common.edit")}
                                                            }
                                                            button {
                                                                class: "pf-row-btn is-danger",
                                                                r#type: "button",
                                                                onclick: {
                                                                    let api = api_del_name.clone();
                                                                    move |_| {
                                                                        let api = api.clone();
                                                                        spawn(async move {
                                                                            match api.delete_person_name(tid, pid, nid).await {
                                                                                Ok(_) => { on_saved_name_del.call(()); refresh += 1; }
                                                                                Err(e) => save_error.set(Some(format!("{e}"))),
                                                                            }
                                                                        });
                                                                    }
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
                                Some(Err(e)) => rsx! { div { class: "error-msg", "Failed to load names: {e}" } },
                                None => rsx! { div { class: "loading", {i18n.t("person_form.loading_names")} } },
                            }
                        }
                        } // end Additional information if !is_create

                        // ── Notes (edit mode only) ──
                        // Notes about the person as such — the ones tied to a
                        // single event live in that event's own panel.
                        if !is_create {
                            div { class: "pf-subblock",
                                div { class: "pf-block-label",
                                    {i18n.t("person_form.notes")}
                                    {render_add_toggle(
                                        i18n.t("person_form.add_note"),
                                        i18n.t("common.cancel"),
                                        show_note_form,
                                    )}
                                }

                                if show_note_form() {
                                    div { class: "pf-subform",
                                        if let Some(err) = note_form_error() {
                                            div { class: "error-msg", "{err}" }
                                        }
                                        div { class: "form-group",
                                            textarea {
                                                rows: 4,
                                                placeholder: "{i18n.t(\"person_form.note_placeholder\")}",
                                                value: "{note_form_text}",
                                                oninput: move |e: Event<FormData>| note_form_text.set(e.value()),
                                            }
                                        }
                                        button {
                                            class: "pf-confirm-btn",
                                            r#type: "button",
                                            onclick: on_create_note,
                                            {i18n.t("person.create_note")}
                                        }
                                    }
                                }

                                if notes_list.is_empty() {
                                    div { class: "pf-empty-item", p { {i18n.t("person_form.no_notes")} } }
                                } else {
                                    for note in notes_list.iter() {
                                        {
                                            let nid = note.id;
                                            let text = note.text.clone();
                                            let preview = crate::utils::html_to_preview(&note.text, 120);
                                            if editing_note_id() == Some(nid) {
                                                rsx! {
                                                    div { class: "person-form-item editing",
                                                        if let Some(err) = edit_note_error() {
                                                            div { class: "error-msg", "{err}" }
                                                        }
                                                        div { class: "form-group",
                                                            textarea {
                                                                rows: 4,
                                                                value: "{edit_note_text}",
                                                                oninput: move |e: Event<FormData>| edit_note_text.set(e.value()),
                                                            }
                                                        }
                                                        div { style: "display:flex;gap:8px;",
                                                            button {
                                                                class: "pf-confirm-btn",
                                                                r#type: "button",
                                                                onclick: {
                                                                    let api = api_edit_note.clone();
                                                                    move |_| {
                                                                        let api = api.clone();
                                                                        let text = edit_note_text().trim().to_string();
                                                                        spawn(async move {
                                                                            if text.is_empty() {
                                                                                edit_note_error.set(Some(i18n.t("person_form.note_required")));
                                                                                return;
                                                                            }
                                                                            match api.update_note(tid, nid, &UpdateNoteBody { text: Some(text) }).await {
                                                                                Ok(_) => {
                                                                                    editing_note_id.set(None);
                                                                                    edit_note_error.set(None);
                                                                                    on_saved_note_edit.call(());
                                                                                    refresh += 1;
                                                                                }
                                                                                Err(e) => edit_note_error.set(Some(format!("{e}"))),
                                                                            }
                                                                        });
                                                                    }
                                                                },
                                                                {i18n.t("common.save")}
                                                            }
                                                            button {
                                                                class: "pf-row-btn",
                                                                r#type: "button",
                                                                onclick: move |_| { editing_note_id.set(None); edit_note_error.set(None); },
                                                                {i18n.t("common.cancel")}
                                                            }
                                                        }
                                                    }
                                                }
                                            } else {
                                                rsx! {
                                                    div { class: "person-form-item pf-compact-item",
                                                        div { class: "person-form-item-info", span { "{preview}" } }
                                                        div { class: "person-form-item-actions",
                                                            button {
                                                                class: "pf-row-btn",
                                                                r#type: "button",
                                                                onclick: move |_| {
                                                                    // The stored body is the plain-text
                                                                    // form (breaks folded to \n), so it
                                                                    // goes straight back in the textarea.
                                                                    edit_note_text.set(text.clone());
                                                                    editing_note_id.set(Some(nid));
                                                                    edit_note_error.set(None);
                                                                },
                                                                {i18n.t("common.edit")}
                                                            }
                                                            button {
                                                                class: "pf-row-btn is-danger",
                                                                r#type: "button",
                                                                onclick: {
                                                                    let api = api_del_note.clone();
                                                                    move |_| {
                                                                        let api = api.clone();
                                                                        spawn(async move {
                                                                            match api.delete_note(tid, nid).await {
                                                                                Ok(_) => { on_saved_note_del.call(()); refresh += 1; }
                                                                                Err(e) => save_error.set(Some(format!("{e}"))),
                                                                            }
                                                                        });
                                                                    }
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

                                // Where the civil-status information itself
                                // came from; saved with the footer button.
                                div { class: "form-group pf-subblock",
                                    label { {i18n.t("person_form.source")} }
                                    input {
                                        r#type: "text",
                                        placeholder: "{i18n.t(\"person_form.source_placeholder\")}",
                                        value: "{person_source}",
                                        oninput: move |e: Event<FormData>| { person_source.set(e.value()); has_changes.set(true); },
                                    }
                                }
                            }
                        }
                    }

                    // ── Birth ──
                    FormSection { title: i18n.t("person_form.birth"), open: open_birth,
                        div { class: "form-group",
                            label { {i18n.t("person_form.date")} }
                            DateInput {
                                parts: birth_parts,
                                i18n,
                                on_change: move |()| has_changes.set(true),
                            }
                        }
                        {render_place_select(&i18n, birth_place_id, &place_options, move || has_changes.set(true))}
                        {render_notes_source_fields(&i18n, birth_notes, birth_source, move || has_changes.set(true))}
                        div { class: "form-group",
                            label { {i18n.t("person_form.witnesses")} }
                            EventWitnesses { tree_id: tid, event_id: birth_event_id() }
                        }
                    }

                    // ── Death ──
                    FormSection { title: i18n.t("person_form.death"), open: open_death,
                        div { class: "form-group",
                            label { {i18n.t("person_form.date")} }
                            DateInput {
                                parts: death_parts,
                                i18n,
                                on_change: move |()| has_changes.set(true),
                            }
                        }
                        {render_place_select(&i18n, death_place_id, &place_options, move || has_changes.set(true))}
                        {render_notes_source_fields(&i18n, death_notes, death_source, move || has_changes.set(true))}
                        div { class: "form-group",
                            label { {i18n.t("person_form.witnesses")} }
                            EventWitnesses { tree_id: tid, event_id: death_event_id() }
                        }
                    }

                    // ── Privacy ──
                    FormSection { title: i18n.t("person_form.privacy"), open: open_privacy,
                        {render_choice_group(
                            &[
                                ("Default", i18n.t("privacy.default")),
                                ("Public",  i18n.t("privacy.public")),
                                ("Private", i18n.t("privacy.private")),
                            ],
                            privacy_val,
                            move || has_changes.set(true),
                        )}
                    }

                    // ── Other Events (edit mode only) ──
                    if !is_create {
                        FormSection {
                            title: i18n.t("person_form.other_events"),
                            open: open_events,
                            action: render_add_toggle(
                                i18n.t("person_form.add_event"),
                                i18n.t("common.cancel"),
                                show_event_form,
                            ),
                        if show_event_form() {
                            div { class: "pf-subform",
                                if let Some(err) = event_form_error() {
                                    div { class: "error-msg", "{err}" }
                                }
                                div { class: "form-row",
                                    div { class: "form-group",
                                        label { {i18n.t("person_form.type")} }
                                        select {
                                            value: "{event_form_type}",
                                            oninput: move |e: Event<FormData>| event_form_type.set(e.value()),
                                            {event_type_options(&i18n)}
                                        }
                                    }
                                }
                                // The event's own value — a profession, a title,
                                // a residence. Carried by GEDCOM on the tag
                                // itself, so it round-trips as such.
                                div { class: "form-group",
                                    label { {i18n.t("person_form.description")} }
                                    input {
                                        r#type: "text",
                                        value: "{event_form_description}",
                                        oninput: move |e: Event<FormData>| event_form_description.set(e.value()),
                                    }
                                }
                                div { class: "form-group",
                                    label { {i18n.t("person_form.date")} }
                                    DateInput {
                                        parts: event_form_parts,
                                        i18n,
                                        on_change: move |()| {},
                                    }
                                }
                                div { class: "form-row",
                                    {render_place_select(&i18n, event_form_place_id, &place_options, || {})}
                                    div { class: "form-group",
                                        label { {i18n.t("person_form.cause")} }
                                        input {
                                            r#type: "text",
                                            value: "{event_form_cause}",
                                            oninput: move |e: Event<FormData>| event_form_cause.set(e.value()),
                                        }
                                    }
                                }
                                {render_notes_source_fields(&i18n, event_form_notes, event_form_source, || {})}
                                button {
                                    class: "pf-confirm-btn",
                                    r#type: "button",
                                    onclick: on_create_event,
                                    {i18n.t("person.create_event")}
                                }
                            }
                        }

                        if other_events.is_empty() {
                            div { class: "pf-empty-item", p { {i18n.t("person_form.no_other_events")} } }
                        } else {
                            for ev in other_events.iter() {
                                {
                                    let eid = ev.id;
                                    let et = i18n.t(event_type_label_key(ev.event_type));
                                    // The event's own value (a `TITL`, `RESI`, ... payload
                                    // on import) — without it the row shows only its type.
                                    let desc = ev.description.clone().unwrap_or_default();
                                    let date = format_event_date(&i18n, ev);
                                    let place = ev.place_id.map(&place_name).unwrap_or_default();
                                    let notes_open = open_event_notes() == Some(eid);
                                    rsx! {
                                        div {
                                            class: if notes_open { "person-form-item pf-ns-open" } else { "person-form-item" },
                                            div { class: "person-form-item-info",
                                                span { class: "badge", "{et}" }
                                                if !desc.is_empty() { span { "{desc}" } }
                                                if !date.is_empty() { span { class: "text-muted", "{date}" } }
                                                if !place.is_empty() { span { class: "text-muted", "@ {place}" } }
                                            }
                                            div { class: "person-form-item-actions",
                                                button {
                                                    class: if notes_open { "pf-row-btn is-active" } else { "pf-row-btn" },
                                                    r#type: "button",
                                                    onclick: move |_| open_event_notes.set(if notes_open { None } else { Some(eid) }),
                                                    {i18n.t("common.edit")}
                                                }
                                                button {
                                                    class: "pf-row-btn is-danger",
                                                    r#type: "button",
                                                    onclick: {
                                                        let api = api_del_event.clone();
                                                        move |_| {
                                                            let api = api.clone();
                                                            spawn(async move {
                                                                match api.delete_event(tid, eid).await {
                                                                    Ok(_) => { on_saved_event_del.call(()); refresh += 1; }
                                                                    Err(e) => save_error.set(Some(format!("{e}"))),
                                                                }
                                                            });
                                                        }
                                                    },
                                                    {i18n.t("common.delete")}
                                                }
                                            }
                                        }
                                        if notes_open {
                                            EventEditor {
                                                tree_id: tid,
                                                person_id: Some(pid),
                                                event: ev.clone(),
                                                description_label: i18n.t("person_form.description"),
                                                place_options: place_options.clone(),
                                                on_saved: move |_| { on_saved_event_del.call(()); refresh += 1; },
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        }
                    } // end Other Events if !is_create

                    // ── Delete Person (edit mode only) ──
                    // No section header: the button says what it does, and a
                    // heading repeating it above would be the same word twice.
                    if !is_create && !is_embedded {
                        DeleteSection {
                            button_label: i18n.t("person_form.delete_person"),
                            title: format!("{} {}?", i18n.t("person_form.delete_confirm_title"), display_name),
                            message: i18n.t("person_form.delete_confirm_message"),
                            confirm_label: i18n.t("person_form.delete_confirm_button"),
                            busy_label: i18n.t("person_form.deleting"),
                            deleting: deleting(),
                            error: delete_error(),
                            on_confirm: on_confirm_delete,
                        }
                    }
                }
    };

    // ── Fixed footer ──
    let footer = rsx! {
        div { class: "pf-footer",
            if let Some(err) = save_error() {
                div { class: "error-msg", "{err}" }
            }
            div { class: "pf-footer-right",
                if !is_embedded {
                    button {
                        class: "btn btn-outline",
                        r#type: "button",
                        onclick: try_close,
                        {i18n.t("common.cancel")}
                    }
                }
                button {
                    class: "btn btn-primary",
                    r#type: "button",
                    disabled: saving(),
                    onclick: on_save,
                    if saving() { {i18n.t("common.saving")} }
                    else if is_create { {i18n.t("person_form.btn_create")} }
                    else { {i18n.t("common.save")} }
                }
            }
        }
    };

    if is_embedded {
        return rsx! {
            div { class: "pf-embedded", {body} {footer} }
        };
    }

    rsx! {
        div { class: "modal-backdrop",
            // Dismiss on press (not click): a click fires on the common ancestor of
            // mousedown/mouseup, so selecting text then releasing outside would close.
            onmousedown: try_close,

            div {
                class: "person-form-modal",
                onmousedown: move |evt: Event<MouseData>| evt.stop_propagation(),
                onkeydown: move |e: Event<KeyboardData>| {
                    match e.key() {
                        Key::Escape => {
                            if has_changes() { show_discard_confirm.set(true); }
                            else { props.on_close.call(()); }
                        }
                        Key::Enter => {
                            document::eval(&focus_next_field_js("person-form-modal"));
                        }
                        _ => {}
                    }
                },

                // ── Fixed header ──
                div { class: "person-form-header",
                    div {
                        h2 { "{display_name}" }
                        span { class: "pf-subtitle",
                            if is_create { {i18n.t("person_form.subtitle_create")} } else { {i18n.t("person_form.subtitle_edit")} }
                        }
                    }
                    button { class: "person-form-close", onclick: try_close, "\u{00D7}" }
                }

                {body}
                {footer}
            }
        }

        if show_discard_confirm() {
            crate::components::confirm_dialog::ConfirmDialog {
                title: "{i18n.t(\"person_form.discard_title\")}",
                message: i18n.t("person_form.discard_message"),
                confirm_label: "{i18n.t(\"person_form.discard_confirm\")}",
                confirm_class: "btn btn-danger",
                error: None,
                on_confirm: move |_| { show_discard_confirm.set(false); props.on_close.call(()); },
                on_cancel: move |_| { show_discard_confirm.set(false); },
            }
        }
    }
}

// ── Shared option builders ────────────────────────────────────────────────

fn event_type_options(i18n: &crate::i18n::I18n) -> Element {
    let i18n = *i18n;
    rsx! {
        optgroup { label: "{i18n.t(\"person_form.sacraments\")}",
            option { value: "Baptism",        {i18n.t("event.type.baptism")} }
            option { value: "Confirmation",   {i18n.t("event.type.confirmation")} }
            option { value: "FirstCommunion", {i18n.t("event.type.first_communion")} }
            option { value: "BarBatMitzvah",  {i18n.t("event.type.bar_bat_mitzvah")} }
            option { value: "Burial",         {i18n.t("event.type.burial")} }
            option { value: "Cremation",      {i18n.t("event.type.cremation")} }
        }
        optgroup { label: "{i18n.t(\"person_form.civil\")}",
            option { value: "Census",          {i18n.t("event.type.census")} }
            option { value: "Graduation",      {i18n.t("event.type.graduation")} }
            option { value: "Immigration",     {i18n.t("event.type.immigration")} }
            option { value: "Emigration",      {i18n.t("event.type.emigration")} }
            option { value: "Naturalization",  {i18n.t("event.type.naturalization")} }
            option { value: "Occupation",      {i18n.t("event.type.occupation")} }
            option { value: "Residence",       {i18n.t("event.type.residence")} }
            option { value: "Retirement",      {i18n.t("event.type.retirement")} }
            option { value: "MilitaryService", {i18n.t("event.type.military_service")} }
        }
        optgroup { label: "{i18n.t(\"person_form.geneweb\")}",
            option { value: "Blessing", {i18n.t("event.type.blessing")} }
            option { value: "Ordination", {i18n.t("event.type.ordination")} }
            option { value: "Christening", {i18n.t("event.type.christening")} }
            option { value: "AdultChristening", {i18n.t("event.type.adult_christening")} }
            option { value: "Accomplishment", {i18n.t("event.type.accomplishment")} }
            option { value: "Acquisition", {i18n.t("event.type.acquisition")} }
            option { value: "Membership", {i18n.t("event.type.membership")} }
            option { value: "ChangeName", {i18n.t("event.type.change_name")} }
            option { value: "Circumcision", {i18n.t("event.type.circumcision")} }
            option { value: "Award", {i18n.t("event.type.award")} }
            option { value: "MilitaryDischarge", {i18n.t("event.type.military_discharge")} }
            option { value: "Degree", {i18n.t("event.type.degree")} }
            option { value: "Distinction", {i18n.t("event.type.distinction")} }
            option { value: "Election", {i18n.t("event.type.election")} }
            option { value: "Excommunication", {i18n.t("event.type.excommunication")} }
            option { value: "Funeral", {i18n.t("event.type.funeral")} }
            option { value: "Hospitalization", {i18n.t("event.type.hospitalization")} }
            option { value: "Illness", {i18n.t("event.type.illness")} }
            option { value: "PassengerList", {i18n.t("event.type.passenger_list")} }
            option { value: "MilitaryDistinction", {i18n.t("event.type.military_distinction")} }
            option { value: "MilitaryPromotion", {i18n.t("event.type.military_promotion")} }
            option { value: "MilitaryMobilization", {i18n.t("event.type.military_mobilization")} }
            option { value: "PropertySale", {i18n.t("event.type.property_sale")} }
            option { value: "Endowment", {i18n.t("event.type.endowment")} }
            option { value: "LdsDotation", {i18n.t("event.type.lds_dotation")} }
            option { value: "SealingChild", {i18n.t("event.type.sealing_child")} }
            option { value: "SealingSpouse", {i18n.t("event.type.sealing_spouse")} }
            option { value: "SealingParent", {i18n.t("event.type.sealing_parent")} }
            option { value: "FamilyLinkLds", {i18n.t("event.type.family_link_lds")} }
            option { value: "NoMarriage", {i18n.t("event.type.no_marriage")} }
            option { value: "LdsBaptism", {i18n.t("event.type.lds_baptism")} }
            option { value: "LdsConfirmation", {i18n.t("event.type.lds_confirmation")} }
            option { value: "NoMention", {i18n.t("event.type.no_mention")} }
        }
        optgroup { label: "{i18n.t(\"person_form.legal\")}",
            option { value: "Will",    {i18n.t("event.type.will")} }
            option { value: "Probate", {i18n.t("event.type.probate")} }
        }
        optgroup { label: "{i18n.t(\"person_form.attributes\")}",
            option { value: "CasteName",            {i18n.t("event.type.caste_name")} }
            option { value: "PhysicalDescription",  {i18n.t("event.type.physical_description")} }
            option { value: "Education",            {i18n.t("event.type.education")} }
            option { value: "NationalId",           {i18n.t("event.type.national_id")} }
            option { value: "NationalOrigin",       {i18n.t("event.type.national_origin")} }
            option { value: "ChildrenCount",        {i18n.t("event.type.children_count")} }
            option { value: "MarriagesCount",       {i18n.t("event.type.marriages_count")} }
            option { value: "Property",             {i18n.t("event.type.property")} }
            option { value: "Religion",             {i18n.t("event.type.religion")} }
            option { value: "SocialSecurityNumber", {i18n.t("event.type.social_security_number")} }
            option { value: "NobilityTitle",        {i18n.t("event.type.nobility_title")} }
            option { value: "Fact",                 {i18n.t("event.type.fact")} }
        }
        optgroup { label: "{i18n.t(\"person_form.other_events\")}",
            option { value: "Adoption", {i18n.t("event.type.adoption")} }
            option { value: "Other",    {i18n.t("event.type.other")} }
        }
    }
}

// ── Shared form building blocks ───────────────────────────────────────────
//
// The person modal and the couple modal are built out of the same parts —
// collapsible sections, place pickers, event bodies, a delete confirmation.
// They live here because `person_form` owns the `pf-` styles they render
// against, and `union_form` uses them through this module.

/// One collapsible block: a header row that toggles it, then its body.
///
/// `action` rides on the right of the header (add an event, add a child) and
/// is only shown while the section is open — a closed section offers nothing
/// to add to.
#[component]
pub fn FormSection(
    title: String,
    open: Signal<bool>,
    #[props(default)] action: Option<Element>,
    children: Element,
) -> Element {
    let mut open = open;
    rsx! {
        div { class: "pf-section",
            div { class: "pf-section-head",
                button {
                    class: "pf-section-toggle",
                    r#type: "button",
                    onclick: move |_| open.toggle(),
                    span { class: if open() { "pf-chevron is-open" } else { "pf-chevron" } }
                    "{title}"
                }
                if open() && let Some(action) = action {
                    {action}
                }
            }
            if open() {
                div { class: "pf-section-body", {children} }
            }
        }
    }
}

/// The "add / cancel" button that opens a sub-form, in the two states it has.
pub(crate) fn render_add_toggle(
    label: String,
    cancel_label: String,
    mut open: Signal<bool>,
) -> Element {
    rsx! {
        button {
            class: if open() { "pf-add-btn is-open" } else { "pf-add-btn" },
            r#type: "button",
            onclick: move |_| open.toggle(),
            if open() { "{cancel_label}" } else { "{label}" }
        }
    }
}

/// A labelled place picker over the tree's places, with a "no place" entry.
pub(crate) fn render_place_select(
    i18n: &crate::i18n::I18n,
    mut selected: Signal<String>,
    options: &[(String, String)],
    mut on_change: impl FnMut() + 'static,
) -> Element {
    let i18n = *i18n;
    let options = options.to_vec();
    let current = selected();
    rsx! {
        div { class: "form-group",
            label { {i18n.t("person_form.place")} }
            select {
                oninput: move |e: Event<FormData>| { selected.set(e.value()); on_change(); },
                // `selected` on the options, not `value` on the select: the
                // list is built by a loop and lands after the element's own
                // attributes, so a `value` set on a select with no options yet
                // selects nothing — an event would open on "no place" whatever
                // place it actually carries.
                option { value: "", selected: current.is_empty(), {i18n.t("person_form.no_place")} }
                for (place_id , place_name) in options.iter() {
                    option { value: "{place_id}", selected: *place_id == current, "{place_name}" }
                }
            }
        }
    }
}

/// A row of mutually exclusive buttons bound to one string signal (sex,
/// privacy) — a radio group that reads as a segmented control.
pub(crate) fn render_choice_group(
    options: &[(&'static str, String)],
    mut selected: Signal<String>,
    on_change: impl FnMut() + Clone + 'static,
) -> Element {
    let options = options.to_vec();
    rsx! {
        div { class: "pf-gender-group",
            for (value , label) in options {
                {
                    let mut on_change = on_change.clone();
                    rsx! {
                        button {
                            class: if selected() == value { "pf-gender-btn active" } else { "pf-gender-btn" },
                            r#type: "button",
                            onclick: move |_| { selected.set(value.to_string()); on_change(); },
                            "{label}"
                        }
                    }
                }
            }
        }
    }
}

/// The destructive action at the foot of a modal: one button that expands
/// into its own confirmation, rather than a dialog stacked on a dialog.
#[component]
pub fn DeleteSection(
    button_label: String,
    title: String,
    message: String,
    confirm_label: String,
    busy_label: String,
    deleting: bool,
    error: Option<String>,
    on_confirm: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let mut confirming = use_signal(|| false);
    rsx! {
        div { class: "pf-delete-section",
            if confirming() {
                div { class: "pf-delete-confirm",
                    p { class: "pf-delete-confirm-name", "{title}" }
                    p { class: "pf-delete-confirm-message", "{message}" }
                    if let Some(err) = error {
                        div { class: "error-msg", "{err}" }
                    }
                    div { class: "pf-delete-confirm-actions",
                        button {
                            class: "btn btn-outline btn-sm",
                            r#type: "button",
                            disabled: deleting,
                            onclick: move |_| confirming.set(false),
                            {i18n.t("common.cancel")}
                        }
                        button {
                            class: "btn btn-danger btn-sm",
                            r#type: "button",
                            disabled: deleting,
                            onclick: move |_| on_confirm.call(()),
                            if deleting { "{busy_label}" } else { "{confirm_label}" }
                        }
                    }
                }
            } else {
                button {
                    class: "pf-delete-person-btn",
                    r#type: "button",
                    onclick: move |_| confirming.set(true),
                    "{button_label}"
                }
            }
        }
    }
}

/// Enter advances to the next field instead of doing nothing.
///
/// Scoped to `modal_class` so a modal embedded in another one (the person
/// fields inside the couple modal) still walks its own field list.
pub(crate) fn focus_next_field_js(modal_class: &str) -> String {
    format!(
        "var a=document.activeElement;\
        if(a&&a.tagName==='INPUT'&&a.type!=='button'&&a.type!=='submit'){{\
            var m=a.closest('.{modal_class}');\
            if(!m)return;\
            var fs=[...m.querySelectorAll('input:not([type=button]):not([type=submit]),select,textarea')];\
            var i=fs.indexOf(a);\
            if(i>=0&&i<fs.length-1)fs[i+1].focus();\
        }}"
    )
}

// ── Event request bodies ──────────────────────────────────────────────────
//
// Every event this app writes carries the same five date columns, read off
// one `DateParts`. Spelling them out per call site meant a dozen near-copies
// that could — and did — drift from one another.
//
// The qualifier comes from `stored_qualifier`, not the raw field: "from an
// age" is an entry mode that resolves to the `About` year it implies.

/// What an event hangs off: a person, or the family (a marriage and its kin).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum EventOwner {
    Person(Uuid),
    Family(Uuid),
}

pub(crate) fn create_event_body(
    event_type: EventType,
    parts: &DateParts,
    place: &str,
    owner: EventOwner,
    description: Option<String>,
    cause: Option<String>,
) -> CreateEventBody {
    let (person_id, family_id) = match owner {
        EventOwner::Person(pid) => (Some(pid), None),
        EventOwner::Family(fid) => (None, Some(fid)),
    };
    CreateEventBody {
        event_type,
        date_value: parts.date_value(),
        date_qualifier: parts.stored_qualifier(),
        date_value2: parts.date_value2(),
        calendar: parts.calendar,
        cause,
        place_id: parse_place_id(place),
        person_id,
        family_id,
        description,
    }
}

/// `description: None` leaves the stored value alone; `Some(value)` writes it
/// (with `Some(None)` clearing it), matching the DTO's own contract.
pub(crate) fn update_event_body(
    event_type: Option<EventType>,
    parts: &DateParts,
    place: &str,
    description: Option<Option<String>>,
) -> UpdateEventBody {
    UpdateEventBody {
        event_type,
        date_value: Some(parts.date_value()),
        date_qualifier: Some(parts.stored_qualifier()),
        date_value2: Some(parts.date_value2()),
        calendar: Some(parts.calendar),
        cause: None,
        place_id: Some(parse_place_id(place)),
        description,
    }
}

/// Writes a person's birth or death — the two events the modal owns outright
/// — together with the notes and source hanging off it.
///
/// Creates the event when the person has none recorded and there is now
/// something to record, updates it when there is one, and leaves an empty
/// section alone: notes and a source need an event to hang off, so a person
/// with no birth at all has nothing to attach them to.
///
/// Returns the notes/source state now stored, which the caller must adopt as
/// its new `current` — saving twice against a stale one would take the
/// "nothing there yet" branch again and leave a duplicate row behind.
#[allow(clippy::too_many_arguments)]
async fn save_vital_event(
    api: &ApiClient,
    tree_id: Uuid,
    person_id: Uuid,
    event_type: EventType,
    existing_id: Option<Uuid>,
    parts: &DateParts,
    place: &str,
    notes: &str,
    source: &str,
    current: &NotesSource,
) -> Result<Option<NotesSource>, ApiError> {
    let event_id = match existing_id {
        Some(eid) => {
            api.update_event(
                tree_id,
                eid,
                &update_event_body(Some(event_type), parts, place, None),
            )
            .await?;
            Some(eid)
        }
        None if !parts.is_empty() || parse_place_id(place).is_some() => Some(
            api.create_event(
                tree_id,
                &create_event_body(
                    event_type,
                    parts,
                    place,
                    EventOwner::Person(person_id),
                    None,
                    None,
                ),
            )
            .await?
            .id,
        ),
        None => None,
    };

    match event_id {
        Some(eid) => save_notes_source(
            api,
            tree_id,
            Some(person_id),
            Some(eid),
            notes,
            source,
            current,
        )
        .await
        .map(Some),
        None => Ok(None),
    }
}

// ── Witnesses widget ──────────────────────────────────────────────────────

/// Resolves each witness's display name via its primary `PersonName`.
async fn resolve_witness_names(
    api: &ApiClient,
    tree_id: Uuid,
    witnesses: Vec<oxidgene_core::types::EventWitness>,
) -> Vec<(oxidgene_core::types::EventWitness, String)> {
    let mut out = Vec::with_capacity(witnesses.len());
    for w in witnesses {
        let name = match api.list_person_names(tree_id, w.person_id).await {
            Ok(names) => names
                .iter()
                .find(|n| n.is_primary)
                .or(names.first())
                .map(|n| {
                    format!(
                        "{} {}",
                        n.given_names.as_deref().unwrap_or(""),
                        n.full_surname().unwrap_or_default()
                    )
                    .trim()
                    .to_string()
                })
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "?".to_string()),
            Err(_) => "?".to_string(),
        };
        out.push((w, name));
    }
    out
}

/// Witness list + add/remove editor for an event. Each add/remove is
/// persisted immediately via the API (mirrors the inline per-item saves
/// used elsewhere in this form), so it's only usable once the event has
/// been saved (`event_id.is_some()`).
///
/// A component rather than a plain render helper because it owns hooks while
/// both of its guards move under it: `event_id` only arrives once the events
/// resource resolves, and the whole widget sits inside a collapsible section.
/// Rendered inline, either transition changed how many hooks its caller
/// registered between two renders — a hook-order mismatch. Its own scope
/// keeps that count stable.
#[component]
fn EventWitnesses(tree_id: Uuid, event_id: Option<Uuid>) -> Element {
    let api = use_context::<ApiClient>();
    let i18n = use_i18n();

    let mut adding = use_signal(|| false);
    let mut relation_input = use_signal(String::new);
    let mut refresh_tick = use_signal(|| 0u32);

    let api_list = api.clone();
    let witnesses_resource = use_resource(move || {
        let api = api_list.clone();
        let _tick = refresh_tick();
        async move {
            let Some(event_id) = event_id else {
                return Vec::new();
            };
            let witnesses = api
                .list_event_witnesses(tree_id, event_id)
                .await
                .unwrap_or_default();
            resolve_witness_names(&api, tree_id, witnesses).await
        }
    });

    let Some(event_id) = event_id else {
        return rsx! {
            p { class: "text-muted", {i18n.t("person_form.witnesses_save_first")} }
        };
    };
    let entries = witnesses_resource.read().clone().unwrap_or_default();

    rsx! {
        div { class: "pf-witness-list",
            for (w , name) in entries {
                div { class: "pf-witness-row",
                    span { class: "pf-witness-name", "{name}" }
                    if let Some(rel) = &w.relation {
                        span { class: "pf-witness-relation", " ({rel})" }
                    }
                    button {
                        class: "pf-witness-remove",
                        r#type: "button",
                        onclick: {
                            let api = api.clone();
                            let witness_id = w.id;
                            move |_| {
                                let api = api.clone();
                                spawn(async move {
                                    let _ = api.remove_event_witness(tree_id, event_id, witness_id).await;
                                    refresh_tick.set(refresh_tick() + 1);
                                });
                            }
                        },
                        "\u{00D7}"
                    }
                }
            }
        }
        if adding() {
            div { class: "pf-witness-add",
                input {
                    r#type: "text",
                    placeholder: i18n.t("person_form.witness_relation_placeholder"),
                    value: "{relation_input}",
                    oninput: move |e: Event<FormData>| relation_input.set(e.value()),
                }
                crate::components::search_person::SearchPerson {
                    tree_id,
                    placeholder: i18n.t("person_form.search_witness"),
                    on_select: {
                        let api = api.clone();
                        move |person_id: Uuid| {
                            let api = api.clone();
                            let relation = opt_str(&relation_input());
                            spawn(async move {
                                let body = crate::api::AddEventWitnessBody {
                                    person_id,
                                    relation,
                                    sort_order: 0,
                                };
                                let _ = api.add_event_witness(tree_id, event_id, &body).await;
                                refresh_tick.set(refresh_tick() + 1);
                            });
                            adding.set(false);
                            relation_input.set(String::new());
                        }
                    },
                    on_cancel: move |_| adding.set(false),
                }
            }
        } else {
            button {
                class: "pf-add-btn",
                r#type: "button",
                onclick: move |_| adding.set(true),
                {i18n.t("person_form.add_witness")}
            }
        }
    }
}

// ── Notes & source widget ─────────────────────────────────────────────────
//
// Every event (birth, death, profession, other) — and the person itself —
// can carry free notes plus the source the information came from.
//
// The two are stored separately on purpose: notes go in a `Note` row and the
// source in a `Citation` row. A `Citation` always needs a `source_id`, so it
// cannot hold sourceless notes; and folding the notes into `Citation.text`
// (which is what the profession form used to do) means they vanish the
// moment the source is cleared.

/// The notes + source pair attached to one target, together with the row ids
/// needed to update them in place on the next save.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct NotesSource {
    notes: String,
    /// The source as the user types it — a plain line of text, empty for
    /// none. `Source` rows are an implementation detail behind it: the title
    /// is matched against the tree's existing sources on save and a new one
    /// is created when nothing matches.
    source_title: String,
    /// The `Source` the citation currently points at, if any.
    source_id: Option<Uuid>,
    note_id: Option<Uuid>,
    citation_id: Option<Uuid>,
    /// A pre-existing citation whose `text` still holds the notes, from
    /// before they were split out into their own `Note`.
    citation_holds_notes: bool,
}

/// Resolves a typed source title to a `Source` id, creating the source when
/// the tree has none by that title. Empty input means "no source".
///
/// Matching is case-insensitive on the trimmed title, so re-typing a source
/// with different capitalisation reuses the existing row instead of
/// littering the dictionary with near-duplicates.
async fn resolve_source(
    api: &ApiClient,
    tree_id: Uuid,
    title: &str,
) -> Result<Option<Uuid>, ApiError> {
    let title = title.trim();
    if title.is_empty() {
        return Ok(None);
    }
    let needle = title.to_lowercase();
    if let Some(existing) = api
        .list_all_sources(tree_id)
        .await
        .unwrap_or_default()
        .into_iter()
        .find(|s| s.title.trim().to_lowercase() == needle)
    {
        return Ok(Some(existing.id));
    }
    let created = api
        .create_source(
            tree_id,
            &CreateSourceBody {
                title: title.to_string(),
                author: None,
                publisher: None,
                abbreviation: None,
                repository_name: None,
            },
        )
        .await?;
    Ok(Some(created.id))
}

/// Loads the notes + source attached to an event (`event_id = Some`) or
/// directly to the person (`event_id = None`).
///
/// Only ever surfaces the first of each: this editor exposes one notes field
/// and one source picker per target, so extra rows (imported, or added
/// through another surface) are left untouched rather than silently merged.
async fn load_notes_source(
    api: &ApiClient,
    tree_id: Uuid,
    person_id: Option<Uuid>,
    event_id: Option<Uuid>,
) -> NotesSource {
    let (person_filter, event_filter) = match event_id {
        Some(eid) => (None, Some(eid)),
        None => (person_id, None),
    };
    // A person-scoped query also returns whatever hangs off that person's
    // events, so the person-level target has to drop those itself.
    let person_level_only = event_id.is_none();

    let note = api
        .list_notes(tree_id, person_filter, event_filter, None, None)
        .await
        .unwrap_or_default()
        .into_iter()
        .find(|n| !person_level_only || n.event_id.is_none());
    let citation = api
        .list_citations(tree_id, person_filter, event_filter, None, None)
        .await
        .unwrap_or_default()
        .into_iter()
        .find(|c| !person_level_only || c.event_id.is_none());

    let citation_text = citation.as_ref().and_then(|c| c.text.clone());
    let source_id = citation.as_ref().map(|c| c.source_id);
    let source_title = match source_id {
        Some(sid) => api
            .get_source(tree_id, sid)
            .await
            .map(|s| s.title)
            .unwrap_or_default(),
        None => String::new(),
    };
    NotesSource {
        notes: note
            .as_ref()
            .map(|n| n.text.clone())
            .or_else(|| citation_text.clone())
            .unwrap_or_default(),
        source_title,
        source_id,
        citation_holds_notes: note.is_none() && citation_text.is_some(),
        note_id: note.map(|n| n.id),
        citation_id: citation.map(|c| c.id),
    }
}

/// Persists an edited notes + source pair against its target, reconciling
/// with what `load_notes_source` read (`current`).
///
/// Edits the `Note` and `Citation` rows in place wherever they already
/// exist — a row is only created when there is none and only deleted when
/// its field is cleared. Returns the state that is now stored, which the
/// caller must keep as its new `current`: saving twice against a stale
/// `current` would take the "nothing there yet" branch a second time and
/// leave a duplicate row behind.
pub(crate) async fn save_notes_source(
    api: &ApiClient,
    tree_id: Uuid,
    person_id: Option<Uuid>,
    event_id: Option<Uuid>,
    notes: &str,
    source_title: &str,
    current: &NotesSource,
) -> Result<NotesSource, ApiError> {
    // An event-scoped row is reachable through its event, and stamping the
    // person on it too would pull it into the person's own note list.
    let owner = if event_id.is_some() { None } else { person_id };
    let notes = notes.trim();

    let note_id = match (current.note_id, notes.is_empty()) {
        (Some(nid), true) => {
            api.delete_note(tree_id, nid).await?;
            None
        }
        (Some(nid), false) => {
            if notes != current.notes.trim() {
                api.update_note(
                    tree_id,
                    nid,
                    &UpdateNoteBody {
                        text: Some(notes.to_string()),
                    },
                )
                .await?;
            }
            Some(nid)
        }
        (None, false) => Some(
            api.create_note(
                tree_id,
                &CreateNoteBody {
                    text: notes.to_string(),
                    person_id: owner,
                    event_id,
                    family_id: None,
                    source_id: None,
                },
            )
            .await?
            .id,
        ),
        (None, true) => None,
    };

    let mut saved = NotesSource {
        notes: notes.to_string(),
        source_title: current.source_title.trim().to_string(),
        source_id: current.source_id,
        note_id,
        citation_id: current.citation_id,
        citation_holds_notes: false,
    };

    // Only touch the sources when the typed title actually changed, so an
    // unrelated save never creates a `Source` row as a side effect.
    if source_title.trim() == current.source_title.trim() {
        if current.citation_holds_notes
            && let Some(cid) = current.citation_id
        {
            // The notes now live in their own Note row; clearing the legacy
            // copy stops it coming back the next time the citation is read.
            api.update_citation(
                tree_id,
                cid,
                &UpdateCitationBody {
                    source_id: None,
                    page: None,
                    confidence: None,
                    text: Some(None),
                },
            )
            .await?;
        }
        return Ok(saved);
    }

    let source_id = resolve_source(api, tree_id, source_title).await?;
    saved.source_title = source_title.trim().to_string();
    saved.source_id = source_id;

    match (current.citation_id, source_id) {
        // Repointed at another source: the citation is the same statement
        // about the same fact, so it is edited rather than replaced.
        (Some(cid), Some(sid)) => {
            api.update_citation(
                tree_id,
                cid,
                &UpdateCitationBody {
                    source_id: Some(sid),
                    page: None,
                    confidence: None,
                    text: Some(None),
                },
            )
            .await?;
        }
        (Some(cid), None) => {
            api.delete_citation(tree_id, cid).await?;
            saved.citation_id = None;
        }
        (None, Some(sid)) => {
            saved.citation_id = Some(
                api.create_citation(
                    tree_id,
                    &CreateCitationBody {
                        source_id: sid,
                        person_id: owner,
                        event_id,
                        family_id: None,
                        page: None,
                        confidence: Confidence::Medium,
                        text: None,
                    },
                )
                .await?
                .id,
            );
        }
        (None, None) => {}
    }

    // The source that was just let go may now be catalogued but unused:
    // free-text entry mints one `Source` per distinct title, so a corrected
    // typo would otherwise leave its row in the tree, and in the source
    // dictionary, forever. The server keeps any source still referenced, so
    // this only ever collects what nothing points at.
    if let Some(previous) = current.source_id
        && Some(previous) != source_id
    {
        let _ = api.delete_source_if_unused(tree_id, previous).await;
    }

    Ok(saved)
}

/// The notes textarea + source field themselves, bound to caller-owned
/// signals. Who saves them differs by target — birth and death ride the
/// modal's footer Save, the per-event rows save themselves — so this only
/// renders the fields.
///
/// The source is a plain text line, not a picker: a source is typed the way
/// it is read off the record ("AD44 — Vigneux-de-Bretagne — N — 1913"), and
/// requiring it to exist first would put a detour in the middle of entering
/// an event. The title is reconciled against the tree's `Source` rows on
/// save.
///
/// Deliberately no `<datalist>` of existing titles: an imported tree has
/// thousands of sources, and re-diffing that many `<option>` nodes on every
/// keystroke made the field unusable. Completion belongs on a debounced
/// prefix query (`dictionary_sources`), not a list of everything.
pub(crate) fn render_notes_source_fields(
    i18n: &crate::i18n::I18n,
    mut notes: Signal<String>,
    mut source_title: Signal<String>,
    on_edit: impl FnMut() + Clone + 'static,
) -> Element {
    let i18n = *i18n;
    let mut on_edit_notes = on_edit.clone();
    let mut on_edit_source = on_edit;
    rsx! {
        div { class: "form-group",
            label { {i18n.t("person_form.notes")} }
            textarea {
                rows: 3,
                value: "{notes}",
                oninput: move |e: Event<FormData>| { notes.set(e.value()); on_edit_notes(); },
            }
        }
        div { class: "form-group",
            label { {i18n.t("person_form.source")} }
            input {
                r#type: "text",
                placeholder: "{i18n.t(\"person_form.source_placeholder\")}",
                value: "{source_title}",
                oninput: move |e: Event<FormData>| { source_title.set(e.value()); on_edit_source(); },
            }
        }
    }
}

/// Full editor for an already-saved event: its description, date, place, notes
/// and source, with its own Save button — the surrounding lists (professions,
/// other events) have no footer of their own.
///
/// The description is the event's own value: for a profession it is the trade
/// itself, and for a GEDCOM attribute (`TITL`, `RESI`, `EDUC`, ...) it is the
/// tag's value. `description_label` names it accordingly.
///
/// Mounted only while its row is expanded, so a long list of events costs
/// nothing until one is opened.
#[component]
pub fn EventEditor(
    tree_id: Uuid,
    /// The person the event hangs off, when it has one. Family events (a
    /// marriage and its kin) pass `None`: an event's notes and source are
    /// reached through the event itself, never through a person.
    person_id: Option<Uuid>,
    event: CoreEvent,
    description_label: String,
    place_options: Vec<(String, String)>,
    on_saved: EventHandler<()>,
) -> Element {
    let api = use_context::<ApiClient>();
    let i18n = use_i18n();

    let event_id = event.id;

    let mut description = use_signal(|| event.description.clone().unwrap_or_default());
    let parts = use_signal(|| {
        DateParts::from_fields(
            event.calendar,
            event.date_qualifier,
            event.date_value.as_deref(),
            event.date_value2.as_deref(),
        )
    });
    let place_id = use_signal(|| event.place_id.map(|id| id.to_string()).unwrap_or_default());

    let mut notes = use_signal(String::new);
    let mut source_title = use_signal(String::new);
    let mut loaded = use_signal(|| None::<NotesSource>);
    let mut saving = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    let api_load = api.clone();
    let resource = use_resource(move || {
        let api = api_load.clone();
        async move { load_notes_source(&api, tree_id, person_id, Some(event_id)).await }
    });

    if loaded().is_none()
        && let Some(ns) = &*resource.read()
    {
        notes.set(ns.notes.clone());
        source_title.set(ns.source_title.clone());
        loaded.set(Some(ns.clone()));
    }

    let api_save = api.clone();
    let on_save = move |_| {
        let api = api_save.clone();
        let Some(current) = loaded() else { return };
        let notes_val = notes();
        let source = source_title();
        let desc = description().trim().to_string();
        let date = parts();
        let place = place_id();
        spawn(async move {
            if let Some(key) = date.validate() {
                error.set(Some(i18n.t(key)));
                return;
            }
            saving.set(true);
            error.set(None);

            let body = update_event_body(None, &date, &place, Some(opt_str(&desc)));
            if let Err(e) = api.update_event(tree_id, event_id, &body).await {
                error.set(Some(format!("{e}")));
                saving.set(false);
                return;
            }

            match save_notes_source(
                &api,
                tree_id,
                person_id,
                Some(event_id),
                &notes_val,
                &source,
                &current,
            )
            .await
            {
                // Adopt the state that was just written, so pressing Save
                // again reconciles against those rows instead of creating a
                // second set.
                Ok(stored) => {
                    loaded.set(Some(stored));
                    on_saved.call(());
                }
                Err(e) => error.set(Some(format!("{e}"))),
            }
            saving.set(false);
        });
    };

    rsx! {
        div { class: "pf-ns-body",
            if let Some(err) = error() {
                div { class: "error-msg", "{err}" }
            }
            if loaded().is_none() {
                div { class: "loading", {i18n.t("common.loading")} }
            } else {
                div { class: "form-group",
                    label { "{description_label}" }
                    input {
                        r#type: "text",
                        value: "{description}",
                        oninput: move |e: Event<FormData>| description.set(e.value()),
                    }
                }
                div { class: "form-group",
                    label { {i18n.t("person_form.date")} }
                    DateInput { parts, i18n, on_change: move |()| {} }
                }
                {render_place_select(&i18n, place_id, &place_options, || {})}
                {render_notes_source_fields(&i18n, notes, source_title, || {})}
                div { class: "pf-ns-actions",
                    button {
                        class: "pf-confirm-btn",
                        r#type: "button",
                        disabled: saving(),
                        onclick: on_save,
                        if saving() { {i18n.t("common.saving")} } else { {i18n.t("common.save")} }
                    }
                }
            }
        }
    }
}

// ── Surname particle row ──────────────────────────────────────────────────

/// How a surname field splits into particle + root.
struct ParticleSplit {
    particle: Option<String>,
    root: String,
    /// The typed particle is not at the head of the surname, so it was not
    /// applied.
    rejected: bool,
}

/// Resolves the particle/root split for a surname field, honouring an override.
///
/// `override_particle` is `None` while the automatic detection is trusted, and
/// `Some(p)` once the user has taken control — where `p` may be empty, meaning
/// "this name has no particle at all".
///
/// The override can only *cut* the field, never add to it: these forms keep a
/// single surname input whose text is the complete surname, so a particle that
/// is absent from it would inject a word the user never typed — and clearing
/// the particle afterwards would not remove it, since by then the word has
/// become part of the surname. Such a particle is reported instead of applied.
fn resolve_particle(raw: &str, override_particle: Option<&str>) -> ParticleSplit {
    let Some(p) = override_particle else {
        let (particle, root) = split_surname_particle(raw);
        return ParticleSplit {
            particle,
            root,
            rejected: false,
        };
    };
    match split_surname_at_head(raw, p) {
        Some((particle, root)) => ParticleSplit {
            particle,
            root,
            rejected: false,
        },
        None => ParticleSplit {
            particle: None,
            root: raw.trim().to_string(),
            rejected: true,
        },
    }
}

/// Seeds the override signal so a stored particle that detection disagrees with
/// is not silently "corrected" the next time the form is saved.
///
/// Without this, a name deliberately stored with no particle ("Da Silva" filed
/// under D) would be re-split on the next save of an unrelated field.
fn override_for_stored(full_surname: &str, stored: Option<&str>) -> Option<String> {
    let stored = stored.unwrap_or("").trim().to_string();
    let (detected, _) = split_surname_particle(full_surname);
    if detected.unwrap_or_default() == stored {
        None
    } else {
        Some(stored)
    }
}

/// Whether the particle row has anything to say about this surname.
///
/// A single-word surname with no particle in it — the overwhelmingly common
/// case — gets no row at all: there is nothing to cut, and since
/// [`split_surname_at_head`] refuses a particle that is not already at the head
/// of the field, the editor could not do anything there either. Announcing
/// "no particle" under every ordinary name is noise.
///
/// One word is not the same as no particle: "d'Aubigné" is a single token that
/// does carry one, which is why this asks detection rather than counting words
/// alone. An open override always keeps the row, so it cannot vanish from under
/// the user mid-edit.
fn particle_row_is_useful(raw: &str, particle: Option<&str>, override_active: bool) -> bool {
    if raw.trim().is_empty() {
        return false;
    }
    override_active || particle.is_some() || raw.split_whitespace().count() > 1
}

/// Renders the detected particle under a surname field, with a way to correct
/// it — the detection is a guess over a fixed word list, so it has to be
/// overridable: someone actually surnamed "Le", or a "Da Silva" that should
/// file under D, needs to opt out, and an unusual particle needs declaring.
fn render_particle_row(
    i18n: &crate::i18n::I18n,
    raw: &str,
    mut override_sig: Signal<Option<String>>,
) -> Element {
    let i18n = *i18n;
    let raw = raw.trim().to_string();
    let current = override_sig();
    let split = resolve_particle(&raw, current.as_deref());
    let (particle, root, rejected) = (split.particle, split.root, split.rejected);

    if !particle_row_is_useful(&raw, particle.as_deref(), current.is_some()) {
        return rsx! {};
    }

    // A particle the surname does not contain is reported rather than applied,
    // so the field never gains a word the user did not type.
    let summary = if rejected {
        i18n.t_args(
            "person_form.particle_not_in_surname",
            &[
                ("particle", current.as_deref().unwrap_or("")),
                ("surname", &root),
            ],
        )
    } else {
        match &particle {
            Some(p) => i18n.t_args(
                "person_form.particle_detected",
                &[("particle", p), ("surname", &root)],
            ),
            None => i18n.t_args("person_form.particle_none", &[("surname", &root)]),
        }
    };

    rsx! {
        div { class: "particle-row",
            if current.is_some() {
                label { class: "particle-label", {i18n.t("person_form.particle")} }
                input {
                    class: "particle-input",
                    r#type: "text",
                    placeholder: "{i18n.t(\"person_form.particle_placeholder\")}",
                    value: "{current.clone().unwrap_or_default()}",
                    oninput: move |e: Event<FormData>| override_sig.set(Some(e.value())),
                }
                button {
                    r#type: "button",
                    class: "particle-btn",
                    onclick: move |_| override_sig.set(None),
                    {i18n.t("person_form.particle_auto")}
                }
                span { class: if rejected { "field-hint field-hint-warn" } else { "field-hint" }, "{summary}" }
            } else {
                span { class: "field-hint", "{summary}" }
                button {
                    r#type: "button",
                    class: "particle-btn",
                    // Seed the editor with the current guess so correcting it is
                    // an edit rather than a retype.
                    onclick: move |_| override_sig.set(Some(particle.clone().unwrap_or_default())),
                    {i18n.t("person_form.particle_change")}
                }
            }
        }
    }
}

/// The name piece an information type writes to.
///
/// The picker mixes two axes: most entries name a *type* of name (Married,
/// Maiden, Alias…) and fill the surname, while Prenom/Sobriquet/Prefixe/
/// Suffixe name a *piece* (GEDCOM `GIVN`/`NICK`/`NPFX`/`NSFX`) of one. This
/// maps each entry onto the piece it fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InfoPiece {
    Given,
    Surname,
    Nickname,
    Prefix,
    Suffix,
}

fn info_piece(info_type: &str) -> InfoPiece {
    match info_type {
        "Prenom" => InfoPiece::Given,
        "Sobriquet" | "Surnom" => InfoPiece::Nickname,
        "Prefixe" => InfoPiece::Prefix,
        "Suffixe" => InfoPiece::Suffix,
        _ => InfoPiece::Surname,
    }
}

/// Builds the request body for one "additional information" entry.
///
/// Surnames are split into particle + root here rather than server-side, so
/// the user can see and correct the split before saving (see
/// `render_information_form`). `birth_surname` pairs a standalone given name
/// with the surname the person already carries, since a given name is never
/// recorded on its own.
fn build_information_body(
    info_type: &str,
    value: &str,
    birth_surname: &str,
    particle_override: Option<&str>,
    birth_particle_override: Option<&str>,
) -> CreatePersonNameBody {
    let piece = info_piece(info_type);

    // Whichever string ends up in the surname slot gets split — with the
    // override belonging to whichever field that string came from.
    let (raw_surname, override_particle) = match piece {
        InfoPiece::Surname => (value, particle_override),
        InfoPiece::Given => (birth_surname, birth_particle_override),
        _ => ("", None),
    };
    let split = resolve_particle(raw_surname, override_particle);
    let (surname_prefix, surname_root) = (split.particle, split.root);

    CreatePersonNameBody {
        name_type: parse_name_type(info_type),
        given_names: (piece == InfoPiece::Given)
            .then(|| opt_str(value))
            .flatten(),
        surname: opt_str(&surname_root),
        surname_prefix,
        prefix: (piece == InfoPiece::Prefix)
            .then(|| opt_str(value))
            .flatten(),
        suffix: (piece == InfoPiece::Suffix)
            .then(|| opt_str(value))
            .flatten(),
        nickname: (piece == InfoPiece::Nickname)
            .then(|| opt_str(value))
            .flatten(),
        is_primary: false,
        sort_order: 0,
    }
}

fn render_information_form(
    i18n: &crate::i18n::I18n,
    error: Signal<Option<String>>,
    mut info_type_sig: Signal<String>,
    mut value_sig: Signal<String>,
    particle_override: Signal<Option<String>>,
    on_create: impl FnMut(Event<MouseData>) + 'static,
) -> Element {
    let i18n = *i18n;
    let piece = info_piece(&info_type_sig());
    let value_label = match piece {
        InfoPiece::Nickname => i18n.t("person_form.nickname"),
        InfoPiece::Given => i18n.t("person_form.given_names"),
        InfoPiece::Prefix => i18n.t("person_form.prefix"),
        InfoPiece::Suffix => i18n.t("person_form.suffix"),
        InfoPiece::Surname => i18n.t("person_form.name_value"),
    };

    rsx! {
        div { class: "pf-subform",
            if let Some(err) = error() {
                div { class: "error-msg", "{err}" }
            }
            div { class: "form-row",
                div { class: "form-group",
                    label { {i18n.t("person_form.information_type")} }
                    select {
                        value: "{info_type_sig}",
                        oninput: move |e: Event<FormData>| info_type_sig.set(e.value()),
                        option { value: "Prenom",     {i18n.t("name_type.prenom")} }
                        option { value: "Married",    {i18n.t("name_type.married")} }
                        option { value: "Alias",      {i18n.t("name_type.alias")} }
                        option { value: "Surnom",     {i18n.t("name_type.surnom")} }
                        option { value: "Maiden",     {i18n.t("name_type.maiden")} }
                        option { value: "Religious",  {i18n.t("name_type.religious")} }
                        option { value: "Prefixe",    {i18n.t("name_type.prefixe")} }
                        option { value: "Suffixe",    {i18n.t("name_type.suffixe")} }
                        option { value: "Other",      {i18n.t("name_type.other")} }
                        option { value: "Sobriquet",  {i18n.t("name_type.sobriquet")} }
                    }
                }
                div { class: "form-group",
                    label { "{value_label}" }
                    input {
                        r#type: "text",
                        value: "{value_sig}",
                        oninput: move |e: Event<FormData>| value_sig.set(e.value()),
                    }
                    if piece == InfoPiece::Surname {
                        {render_particle_row(&i18n, &value_sig(), particle_override)}
                    }
                }
            }
            button { class: "pf-confirm-btn", r#type: "button", onclick: on_create, {i18n.t("person.create_information")} }
        }
    }
}

#[cfg(test)]
mod information_form_tests {
    use super::*;

    #[test]
    fn prefix_and_suffix_reach_their_own_pieces() {
        // These two information types had no picker entry at all, so NPFX and
        // NSFX were unreachable from the add form.
        let body = build_information_body("Prefixe", "Dr.", "DUPONT", None, None);
        assert_eq!(body.prefix.as_deref(), Some("Dr."));
        assert_eq!(body.suffix, None);
        assert_eq!(body.surname, None);

        let body = build_information_body("Suffixe", "Jr.", "DUPONT", None, None);
        assert_eq!(body.suffix.as_deref(), Some("Jr."));
        assert_eq!(body.prefix, None);
    }

    #[test]
    fn each_information_type_keeps_its_own_name_type() {
        // Previously every one of these collapsed onto AlsoKnownAs, which made
        // the user's pick unrecoverable once saved.
        for (picked, expected) in [
            ("Alias", NameType::Alias),
            ("Surnom", NameType::Byname),
            ("Sobriquet", NameType::Sobriquet),
            ("Prenom", NameType::GivenName),
            ("Married", NameType::Married),
        ] {
            let body = build_information_body(picked, "X", "DUPONT", None, None);
            assert_eq!(body.name_type, expected, "for {picked}");
        }
    }

    #[test]
    fn surname_entries_are_split_into_particle_and_root() {
        let body = build_information_body("Married", "de la Cruz", "DUPONT", None, None);
        assert_eq!(body.surname.as_deref(), Some("Cruz"));
        assert_eq!(body.surname_prefix.as_deref(), Some("de la"));
    }

    #[test]
    fn a_standalone_given_name_pairs_with_the_split_birth_surname() {
        let body = build_information_body("Prenom", "Baptiste", "van der Berg", None, None);
        assert_eq!(body.given_names.as_deref(), Some("Baptiste"));
        assert_eq!(body.surname.as_deref(), Some("Berg"));
        assert_eq!(body.surname_prefix.as_deref(), Some("van der"));
    }

    /// The reported bug: on a plain surname, typing a particle that is not in
    /// the field used to inject it — after which clearing the particle field
    /// could not remove it, because the word had become part of the surname.
    #[test]
    fn the_particle_row_stays_hidden_for_an_ordinary_surname() {
        // The common case: one word, no particle. Nothing to cut, and the
        // editor could not cut anything either — so say nothing.
        assert!(!particle_row_is_useful("DUPONT", None, false));
        assert!(!particle_row_is_useful("", None, false));
        assert!(!particle_row_is_useful("   ", None, false));
    }

    #[test]
    fn the_particle_row_appears_when_it_has_something_to_offer() {
        // A detected particle, even inside a single token.
        assert!(particle_row_is_useful("d'Aubigné", Some("d'"), false));
        assert!(particle_row_is_useful("de la Cruz", Some("de la"), false));
        // Several words but no particle: the user may still want to declare an
        // unusual one, so the affordance stays.
        assert!(particle_row_is_useful("MARTIN DUPONT", None, false));
        // And an open editor is never yanked away mid-edit.
        assert!(particle_row_is_useful("DUPONT", None, true));
    }

    #[test]
    fn a_particle_absent_from_the_surname_is_never_injected() {
        let split = resolve_particle("DUPONT", Some("de"));
        assert_eq!(split.particle, None, "must not invent a particle");
        assert_eq!(split.root, "DUPONT", "the surname must be left untouched");
        assert!(split.rejected, "and the user must be told why");

        // Which makes it reversible: clearing returns the original name.
        let split = resolve_particle("DUPONT", Some(""));
        assert_eq!(split.particle, None);
        assert_eq!(split.root, "DUPONT");
        assert!(!split.rejected);
    }

    #[test]
    fn an_override_only_moves_the_cut_within_the_field() {
        // Narrowing a guess is a cut, so it applies.
        let split = resolve_particle("de la Cruz", Some("de"));
        assert_eq!(split.particle.as_deref(), Some("de"));
        assert_eq!(split.root, "la Cruz");
        assert!(!split.rejected);

        // Widening past what the field holds is not.
        let split = resolve_particle("Cruz", Some("de la"));
        assert!(split.rejected);
        assert_eq!(split.root, "Cruz");
    }

    #[test]
    fn an_override_wins_over_detection() {
        // "Da Silva" detects "Da" as a particle; the user says it is not one.
        let body = build_information_body("Married", "Da Silva", "DUPONT", Some(""), None);
        assert_eq!(body.surname.as_deref(), Some("Da Silva"));
        assert_eq!(body.surname_prefix, None);

        // Or narrows a guess that went too far.
        let body = build_information_body("Married", "de la Cruz", "DUPONT", Some("de"), None);
        assert_eq!(body.surname_prefix.as_deref(), Some("de"));
        assert_eq!(body.surname.as_deref(), Some("la Cruz"));
    }

    #[test]
    fn a_given_name_entry_uses_the_birth_field_override() {
        // The surname comes from the birth field, so it is that field's
        // override that applies — not the information form's own.
        let body =
            build_information_body("Prenom", "Baptiste", "Da Silva", Some("de la"), Some(""));
        assert_eq!(body.surname.as_deref(), Some("Da Silva"));
        assert_eq!(body.surname_prefix, None);
    }

    #[test]
    fn stored_particles_are_only_overridden_when_detection_disagrees() {
        // Detection already agrees: stay on auto, so later edits keep tracking it.
        assert_eq!(override_for_stored("de la Cruz", Some("de la")), None);
        assert_eq!(override_for_stored("Dupont", None), None);
        // Including for a name whose leading article detection now leaves alone.
        assert_eq!(override_for_stored("Le Branch", None), None);

        // A stored "no particle" that detection would split must be pinned,
        // otherwise saving an unrelated field would silently re-split it.
        assert_eq!(override_for_stored("Da Silva", None), Some(String::new()));
        // As must a stored particle narrower than the guess.
        assert_eq!(
            override_for_stored("de la Cruz", Some("de")),
            Some("de".to_string())
        );
    }

    #[test]
    fn a_nickname_fills_only_the_nickname_piece() {
        let body = build_information_body("Sobriquet", "Titi", "DUPONT", None, None);
        assert_eq!(body.nickname.as_deref(), Some("Titi"));
        assert_eq!(body.surname, None);
        assert_eq!(body.given_names, None);
    }
}
