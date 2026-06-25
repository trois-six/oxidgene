//! Modal-based person edit form — single scrollable body with section dividers.
//!
//! Sections: Civil Status · Birth · Death · Privacy · Additional Fields ·
//!           Other Events · Notes.
//! A single footer Save button persists sex + privacy + birth event + death event
//! (including qualifier, calendar, witnesses) and closes the modal.
//! Name, event, and note CRUD use inline per-item saves.

use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::{
    AddChildBody, AddSpouseBody, ApiClient, CreateEventBody, CreateNoteBody, CreatePersonBody,
    CreatePersonNameBody, UpdateEventBody, UpdatePersonBody, UpdatePersonNameBody,
};
use crate::i18n::use_i18n;
use crate::utils::{
    opt_str, parse_calendar, parse_date_qualifier, parse_event_type, parse_name_type,
    parse_privacy, parse_sex,
};
use oxidgene_core::types::{Event as CoreEvent, Note as CoreNote};
use oxidgene_core::{Calendar, ChildType, DateQualifier, EventType, SpouseRole};

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

    // ── Name CRUD state ──
    let mut show_name_form = use_signal(move || is_create); // visible by default in create mode
    let mut name_form_type = use_signal(|| "Birth".to_string());
    let mut name_form_given = use_signal(String::new);
    let mut name_form_surname = use_signal(String::new);
    let mut name_form_prefix = use_signal(String::new);
    let mut name_form_suffix = use_signal(String::new);
    let mut name_form_nickname = use_signal(String::new);
    let mut name_form_primary = use_signal(|| true);
    let mut name_form_error = use_signal(|| None::<String>);

    let mut editing_name_id = use_signal(|| None::<Uuid>);
    let mut edit_name_type = use_signal(|| "Birth".to_string());
    let mut edit_name_given = use_signal(String::new);
    let mut edit_name_surname = use_signal(String::new);
    let mut edit_name_prefix = use_signal(String::new);
    let mut edit_name_suffix = use_signal(String::new);
    let mut edit_name_nickname = use_signal(String::new);
    let mut edit_name_primary = use_signal(|| false);
    let mut edit_name_error = use_signal(|| None::<String>);

    // ── Birth state ──
    let mut birth_date = use_signal(String::new);
    let mut birth_qualifier = use_signal(|| "Exact".to_string());
    let mut birth_date2 = use_signal(String::new);
    let mut birth_place_id = use_signal(String::new);
    let mut birth_note = use_signal(String::new);
    let mut birth_calendar = use_signal(|| "Gregorian".to_string());
    let mut birth_witnesses = use_signal(|| Vec::<String>::new());
    let mut birth_event_id = use_signal(|| None::<Uuid>);

    // ── Death state ──
    let mut death_date = use_signal(String::new);
    let mut death_qualifier = use_signal(|| "Exact".to_string());
    let mut death_date2 = use_signal(String::new);
    let mut death_place_id = use_signal(String::new);
    let mut death_note = use_signal(String::new);
    let mut death_calendar = use_signal(|| "Gregorian".to_string());
    let mut death_witnesses = use_signal(|| Vec::<String>::new());
    let mut death_event_id = use_signal(|| None::<Uuid>);

    let mut birth_death_loaded = use_signal(|| false);

    // ── Additional fields panel ──
    let mut show_additional = use_signal(|| false);

    // ── Other event CRUD state ──
    let mut show_event_form = use_signal(|| false);
    let mut event_form_type = use_signal(|| "Baptism".to_string());
    let mut event_form_date = use_signal(String::new);
    let mut event_form_place_id = use_signal(String::new);
    let mut event_form_note = use_signal(String::new);
    let mut event_form_cause = use_signal(String::new);
    let mut event_form_error = use_signal(|| None::<String>);

    // ── Note CRUD state ──
    let mut show_note_form = use_signal(|| false);
    let mut note_form_text = use_signal(String::new);
    let mut note_form_error = use_signal(|| None::<String>);

    // ── UI state ──
    let mut saving = use_signal(|| false);
    let mut save_error = use_signal(|| None::<String>);
    let mut has_changes = use_signal(|| false);
    let mut show_discard_confirm = use_signal(|| false);
    let mut show_delete_confirm = use_signal(|| false);
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
        async move { api.list_places(tid, Some(200), None, None).await }
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
                    birth_date.set(ev.date_value.clone().unwrap_or_default());
                    birth_qualifier.set(format!("{:?}", ev.date_qualifier));
                    birth_date2.set(ev.date_value2.clone().unwrap_or_default());
                    birth_place_id.set(ev.place_id.map(|id| id.to_string()).unwrap_or_default());
                    birth_note.set(ev.description.clone().unwrap_or_default());
                    birth_calendar.set(format!("{:?}", ev.calendar));
                    birth_witnesses.set(ev.witnesses.clone());
                }
                EventType::Death => {
                    death_event_id.set(Some(ev.id));
                    death_date.set(ev.date_value.clone().unwrap_or_default());
                    death_qualifier.set(format!("{:?}", ev.date_qualifier));
                    death_date2.set(ev.date_value2.clone().unwrap_or_default());
                    death_place_id.set(ev.place_id.map(|id| id.to_string()).unwrap_or_default());
                    death_note.set(ev.description.clone().unwrap_or_default());
                    death_calendar.set(format!("{:?}", ev.calendar));
                    death_witnesses.set(ev.witnesses.clone());
                }
                _ => {}
            }
        }
        birth_death_loaded.set(true);
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
            .filter(|e| !matches!(e.node.event_type, EventType::Birth | EventType::Death))
            .map(|e| e.node.clone())
            .collect(),
        _ => vec![],
    };

    let notes_list: Vec<CoreNote> = match &*notes_resource.read() {
        Some(Ok(notes)) => notes.clone(),
        _ => vec![],
    };

    let place_name = |place_id: Uuid| -> String {
        let data = places_resource.read();
        match &*data {
            Some(Ok(conn)) => conn
                .edges
                .iter()
                .find(|e| e.node.id == place_id)
                .map(|e| e.node.name.clone())
                .unwrap_or_else(|| place_id.to_string()[..8].to_string()),
            _ => place_id.to_string()[..8].to_string(),
        }
    };

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

    // Whether qualifier needs a second date input.
    let birth_needs_date2 = matches!(
        parse_date_qualifier(&birth_qualifier()),
        DateQualifier::Or | DateQualifier::Between
    );
    let death_needs_date2 = matches!(
        parse_date_qualifier(&death_qualifier()),
        DateQualifier::Or | DateQualifier::Between
    );

    // ── Handlers ──

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
                name_form_error.set(Some(i18n.t("person_form.given_or_surname_required")));
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

    let api_edit_name = api.clone();
    let on_saved_name_edit = props.on_saved;
    let api_del_name = api.clone();
    let on_saved_name_del = props.on_saved;

    let api_create_event = api.clone();
    let on_saved_event = props.on_saved;
    let on_create_event = move |_| {
        let api = api_create_event.clone();
        let event_type_str = event_form_type();
        let date = event_form_date().trim().to_string();
        let place_str = event_form_place_id();
        let note = event_form_note().trim().to_string();
        let cause = event_form_cause().trim().to_string();
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
                date_qualifier: DateQualifier::default(),
                date_value2: None,
                calendar: Calendar::default(),
                witnesses: vec![],
                cause: opt_str(&cause),
                place_id,
                person_id: Some(pid),
                family_id: None,
                description: opt_str(&note),
            };
            match api.create_event(tid, &body).await {
                Ok(_) => {
                    show_event_form.set(false);
                    event_form_type.set("Baptism".to_string());
                    event_form_date.set(String::new());
                    event_form_place_id.set(String::new());
                    event_form_note.set(String::new());
                    event_form_cause.set(String::new());
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
            // Name form values (used in create mode)
            let nm_type = name_form_type();
            let nm_given = name_form_given().trim().to_string();
            let nm_surname = name_form_surname().trim().to_string();
            let nm_prefix = name_form_prefix().trim().to_string();
            let nm_suffix = name_form_suffix().trim().to_string();
            let nm_nickname = name_form_nickname().trim().to_string();
            // Event form values
            let birth_eid = birth_event_id();
            let death_eid = death_event_id();
            let b_date = birth_date().trim().to_string();
            let b_qual = birth_qualifier();
            let b_date2 = birth_date2().trim().to_string();
            let b_place = birth_place_id();
            let b_note = birth_note().trim().to_string();
            let b_cal = birth_calendar();
            let b_witnesses = birth_witnesses();
            let d_date = death_date().trim().to_string();
            let d_qual = death_qualifier();
            let d_date2 = death_date2().trim().to_string();
            let d_place = death_place_id();
            let d_note = death_note().trim().to_string();
            let d_cal = death_calendar();
            let d_witnesses = death_witnesses();
            spawn(async move {
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

                    // 2. Create name if any field is filled.
                    if !nm_given.is_empty() || !nm_surname.is_empty() {
                        let body = CreatePersonNameBody {
                            name_type: parse_name_type(&nm_type),
                            given_names: opt_str(&nm_given),
                            surname: opt_str(&nm_surname),
                            prefix: opt_str(&nm_prefix),
                            suffix: opt_str(&nm_suffix),
                            nickname: opt_str(&nm_nickname),
                            is_primary: true,
                        };
                        if let Err(e) = api.create_person_name(tid, new_pid, &body).await {
                            save_error.set(Some(format!("{e}")));
                            saving.set(false);
                            return;
                        }
                    }

                    // 3. Birth event.
                    let b_place_id = if b_place.is_empty() {
                        None
                    } else {
                        b_place.parse::<Uuid>().ok()
                    };
                    if !b_date.is_empty() || b_place_id.is_some() {
                        let body = CreateEventBody {
                            event_type: EventType::Birth,
                            date_value: opt_str(&b_date),
                            date_sort: None,
                            date_qualifier: parse_date_qualifier(&b_qual),
                            date_value2: opt_str(&b_date2),
                            calendar: parse_calendar(&b_cal),
                            witnesses: b_witnesses,
                            cause: None,
                            place_id: b_place_id,
                            person_id: Some(new_pid),
                            family_id: None,
                            description: opt_str(&b_note),
                        };
                        if let Err(e) = api.create_event(tid, &body).await {
                            save_error.set(Some(format!("{e}")));
                            saving.set(false);
                            return;
                        }
                    }

                    // 4. Death event.
                    let d_place_id = if d_place.is_empty() {
                        None
                    } else {
                        d_place.parse::<Uuid>().ok()
                    };
                    if !d_date.is_empty() || d_place_id.is_some() {
                        let body = CreateEventBody {
                            event_type: EventType::Death,
                            date_value: opt_str(&d_date),
                            date_sort: None,
                            date_qualifier: parse_date_qualifier(&d_qual),
                            date_value2: opt_str(&d_date2),
                            calendar: parse_calendar(&d_cal),
                            witnesses: d_witnesses,
                            cause: None,
                            place_id: d_place_id,
                            person_id: Some(new_pid),
                            family_id: None,
                            description: opt_str(&d_note),
                        };
                        if let Err(e) = api.create_event(tid, &body).await {
                            save_error.set(Some(format!("{e}")));
                            saving.set(false);
                            return;
                        }
                    }

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

                    // 2. Birth event.
                    let b_place_id = if b_place.is_empty() {
                        None
                    } else {
                        b_place.parse::<Uuid>().ok()
                    };
                    let b_qualifier_enum = parse_date_qualifier(&b_qual);
                    let b_calendar_enum = parse_calendar(&b_cal);
                    if let Some(eid) = birth_eid {
                        let body = UpdateEventBody {
                            event_type: Some(EventType::Birth),
                            date_value: Some(opt_str(&b_date)),
                            date_sort: None,
                            date_qualifier: Some(b_qualifier_enum),
                            date_value2: Some(opt_str(&b_date2)),
                            calendar: Some(b_calendar_enum),
                            witnesses: Some(b_witnesses),
                            cause: None,
                            place_id: Some(b_place_id),
                            description: Some(opt_str(&b_note)),
                        };
                        if let Err(e) = api.update_event(tid, eid, &body).await {
                            save_error.set(Some(format!("{e}")));
                            saving.set(false);
                            return;
                        }
                    } else if !b_date.is_empty() || b_place_id.is_some() {
                        let body = CreateEventBody {
                            event_type: EventType::Birth,
                            date_value: opt_str(&b_date),
                            date_sort: None,
                            date_qualifier: b_qualifier_enum,
                            date_value2: opt_str(&b_date2),
                            calendar: b_calendar_enum,
                            witnesses: b_witnesses,
                            cause: None,
                            place_id: b_place_id,
                            person_id: Some(pid),
                            family_id: None,
                            description: opt_str(&b_note),
                        };
                        if let Err(e) = api.create_event(tid, &body).await {
                            save_error.set(Some(format!("{e}")));
                            saving.set(false);
                            return;
                        }
                    }

                    // 3. Death event.
                    let d_place_id = if d_place.is_empty() {
                        None
                    } else {
                        d_place.parse::<Uuid>().ok()
                    };
                    let d_qualifier_enum = parse_date_qualifier(&d_qual);
                    let d_calendar_enum = parse_calendar(&d_cal);
                    if let Some(eid) = death_eid {
                        let body = UpdateEventBody {
                            event_type: Some(EventType::Death),
                            date_value: Some(opt_str(&d_date)),
                            date_sort: None,
                            date_qualifier: Some(d_qualifier_enum),
                            date_value2: Some(opt_str(&d_date2)),
                            calendar: Some(d_calendar_enum),
                            witnesses: Some(d_witnesses),
                            cause: None,
                            place_id: Some(d_place_id),
                            description: Some(opt_str(&d_note)),
                        };
                        if let Err(e) = api.update_event(tid, eid, &body).await {
                            save_error.set(Some(format!("{e}")));
                            saving.set(false);
                            return;
                        }
                    } else if !d_date.is_empty() || d_place_id.is_some() {
                        let body = CreateEventBody {
                            event_type: EventType::Death,
                            date_value: opt_str(&d_date),
                            date_sort: None,
                            date_qualifier: d_qualifier_enum,
                            date_value2: opt_str(&d_date2),
                            calendar: d_calendar_enum,
                            witnesses: d_witnesses,
                            cause: None,
                            place_id: d_place_id,
                            person_id: Some(pid),
                            family_id: None,
                            description: opt_str(&d_note),
                        };
                        if let Err(e) = api.create_event(tid, &body).await {
                            save_error.set(Some(format!("{e}")));
                            saving.set(false);
                            return;
                        }
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
                    div { class: "person-form-section",
                        div { class: "pf-section-title", {i18n.t("person_form.tab_civil")} }

                        div { class: "form-group",
                            label { {i18n.t("person_form.sex")} }
                            div { class: "pf-gender-group",
                                {
                                    let gender_opts = [
                                        ("Male",    i18n.t("sex.male")),
                                        ("Female",  i18n.t("sex.female")),
                                        ("Unknown", i18n.t("sex.unknown")),
                                    ];
                                    rsx! {
                                        for (val, label) in gender_opts {
                                            {
                                                let v = val;
                                                let is_active = sex_val() == v;
                                                rsx! {
                                                    button {
                                                        class: if is_active { "pf-gender-btn active" } else { "pf-gender-btn" },
                                                        r#type: "button",
                                                        onclick: move |_| { sex_val.set(v.to_string()); has_changes.set(true); },
                                                        "{label}"
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        div { style: "margin-top: 16px;",
                            div { class: "section-header",
                                h3 { style: "font-size: 0.9rem;", {i18n.t("person_form.tab_names")} }
                                if !is_create {
                                    button {
                                        class: "btn btn-primary btn-sm",
                                        onclick: move |_| show_name_form.toggle(),
                                        if show_name_form() { {i18n.t("common.cancel")} } else { {i18n.t("person_form.add_name")} }
                                    }
                                }
                            }

                            if show_name_form() {
                                {render_name_form(
                                    &i18n,
                                    &name_form_error,
                                    is_create,
                                    &mut name_form_type, &mut name_form_given, &mut name_form_surname,
                                    &mut name_form_prefix, &mut name_form_suffix, &mut name_form_nickname,
                                    &mut name_form_primary, on_create_name,
                                )}
                            }

                            match &*names_resource.read() {
                                Some(Ok(names)) => rsx! {
                                    for name in names.iter() {
                                        {
                                            let nid = name.id;
                                            let is_editing = editing_name_id() == Some(nid);
                                            let nt = format!("{:?}", name.name_type);
                                            let nt_label = format!("{}", name.name_type);
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
                                                                label { {i18n.t("person_form.name_type")} }
                                                                select {
                                                                    value: "{edit_name_type}",
                                                                    oninput: move |e: Event<FormData>| edit_name_type.set(e.value()),
                                                                    option { value: "Birth", {i18n.t("name_type.birth")} }
                                                                    option { value: "Married", {i18n.t("name_type.married")} }
                                                                    option { value: "AlsoKnownAs", {i18n.t("name_type.also_known_as")} }
                                                                    option { value: "Maiden", {i18n.t("name_type.maiden")} }
                                                                    option { value: "Religious", {i18n.t("name_type.religious")} }
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
                                                                class: "btn btn-primary btn-sm",
                                                                onclick: {
                                                                    let api = api_edit_name.clone();
                                                                    move |_| {
                                                                        let api = api.clone();
                                                                        let Some(nid) = editing_name_id() else { return; };
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
                                                                {i18n.t("common.save")}
                                                            }
                                                            button {
                                                                class: "btn btn-outline btn-sm",
                                                                onclick: move |_| { editing_name_id.set(None); edit_name_error.set(None); },
                                                                {i18n.t("common.cancel")}
                                                            }
                                                        }
                                                    }
                                                }
                                            } else {
                                                rsx! {
                                                    div { class: "person-form-item",
                                                        div { class: "person-form-item-info",
                                                            span { class: "badge", "{nt_label}" }
                                                            strong {
                                                                if !gn.is_empty() { "{gn} " }
                                                                "{sn}"
                                                            }
                                                            if prim {
                                                                span { class: "badge", style: "background: var(--color-primary); color: white;", {i18n.t("person_form.primary")} }
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
                                                                {i18n.t("common.edit")}
                                                            }
                                                            button {
                                                                class: "btn btn-danger btn-sm",
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
                                },
                                Some(Err(e)) => rsx! { div { class: "error-msg", "Failed to load names: {e}" } },
                                None => rsx! { div { class: "loading", {i18n.t("person_form.loading_names")} } },
                            }
                        }
                    }

                    // ── Birth ──
                    div { class: "person-form-section",
                        div { class: "pf-section-title", {i18n.t("person_form.birth")} }
                        div { class: "form-group",
                            label { {i18n.t("person_form.date")} }
                            div { class: "pf-date-row",
                                select {
                                    class: "pf-date-qualifier-select",
                                    value: "{birth_qualifier}",
                                    oninput: move |e: Event<FormData>| { birth_qualifier.set(e.value()); has_changes.set(true); },
                                    {qualifier_options(&i18n)}
                                }
                                input {
                                    class: "pf-date-input",
                                    r#type: "text",
                                    placeholder: "{i18n.t(\"person_form.date_placeholder\")}",
                                    value: "{birth_date}",
                                    oninput: move |e: Event<FormData>| { birth_date.set(e.value()); has_changes.set(true); },
                                }
                                if birth_needs_date2 {
                                    span { class: "pf-date-separator",
                                        if birth_qualifier() == "Between" { {i18n.t("person_form.date2_label_between")} } else { {i18n.t("person_form.date2_label_or")} }
                                    }
                                    input {
                                        class: "pf-date-input",
                                        r#type: "text",
                                        placeholder: "{i18n.t(\"person_form.date_placeholder\")}",
                                        value: "{birth_date2}",
                                        oninput: move |e: Event<FormData>| { birth_date2.set(e.value()); has_changes.set(true); },
                                    }
                                }
                            }
                        }
                        div { class: "form-row",
                            div { class: "form-group",
                                label { {i18n.t("person_form.place")} }
                                select {
                                    value: "{birth_place_id}",
                                    oninput: move |e: Event<FormData>| { birth_place_id.set(e.value()); has_changes.set(true); },
                                    option { value: "", {i18n.t("person_form.no_place")} }
                                    for (pid_opt, pname) in place_options.iter() {
                                        option { value: "{pid_opt}", "{pname}" }
                                    }
                                }
                            }
                            div { class: "form-group",
                                label { {i18n.t("person_form.note")} }
                                input {
                                    r#type: "text",
                                    value: "{birth_note}",
                                    oninput: move |e: Event<FormData>| { birth_note.set(e.value()); has_changes.set(true); },
                                }
                            }
                        }
                    }

                    // ── Death ──
                    div { class: "person-form-section",
                        div { class: "pf-section-title", {i18n.t("person_form.death")} }
                        div { class: "form-group",
                            label { {i18n.t("person_form.date")} }
                            div { class: "pf-date-row",
                                select {
                                    class: "pf-date-qualifier-select",
                                    value: "{death_qualifier}",
                                    oninput: move |e: Event<FormData>| { death_qualifier.set(e.value()); has_changes.set(true); },
                                    {qualifier_options(&i18n)}
                                }
                                input {
                                    class: "pf-date-input",
                                    r#type: "text",
                                    placeholder: "{i18n.t(\"person_form.death_date_placeholder\")}",
                                    value: "{death_date}",
                                    oninput: move |e: Event<FormData>| { death_date.set(e.value()); has_changes.set(true); },
                                }
                                if death_needs_date2 {
                                    span { class: "pf-date-separator",
                                        if death_qualifier() == "Between" { {i18n.t("person_form.date2_label_between")} } else { {i18n.t("person_form.date2_label_or")} }
                                    }
                                    input {
                                        class: "pf-date-input",
                                        r#type: "text",
                                        placeholder: "{i18n.t(\"person_form.date_placeholder\")}",
                                        value: "{death_date2}",
                                        oninput: move |e: Event<FormData>| { death_date2.set(e.value()); has_changes.set(true); },
                                    }
                                }
                            }
                        }
                        div { class: "form-row",
                            div { class: "form-group",
                                label { {i18n.t("person_form.place")} }
                                select {
                                    value: "{death_place_id}",
                                    oninput: move |e: Event<FormData>| { death_place_id.set(e.value()); has_changes.set(true); },
                                    option { value: "", {i18n.t("person_form.no_place")} }
                                    for (pid_opt, pname) in place_options.iter() {
                                        option { value: "{pid_opt}", "{pname}" }
                                    }
                                }
                            }
                            div { class: "form-group",
                                label { {i18n.t("person_form.note")} }
                                input {
                                    r#type: "text",
                                    value: "{death_note}",
                                    oninput: move |e: Event<FormData>| { death_note.set(e.value()); has_changes.set(true); },
                                }
                            }
                        }
                    }

                    // ── Privacy ──
                    div { class: "person-form-section",
                        div { class: "pf-section-title", {i18n.t("person_form.privacy")} }
                        div { class: "pf-gender-group",
                            {
                                let privacy_opts = [
                                    ("Default", i18n.t("privacy.default")),
                                    ("Public",  i18n.t("privacy.public")),
                                    ("Private", i18n.t("privacy.private")),
                                ];
                                rsx! {
                                    for (val, label) in privacy_opts {
                                        {
                                            let v = val;
                                            let is_active = privacy_val() == v;
                                            rsx! {
                                                button {
                                                    class: if is_active { "pf-gender-btn active" } else { "pf-gender-btn" },
                                                    r#type: "button",
                                                    onclick: move |_| { privacy_val.set(v.to_string()); has_changes.set(true); },
                                                    "{label}"
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // ── Additional Fields (collapsible) ──
                    div { class: "person-form-section",
                        div { class: "pf-collapsible-header",
                            div { class: "pf-section-title has-action",
                                {i18n.t("person_form.additional_fields_show")}
                            }
                            button {
                                class: "pf-collapsible-toggle",
                                r#type: "button",
                                onclick: move |_| show_additional.toggle(),
                                if show_additional() { "\u{2212}" } else { "+" }
                            }
                        }

                        if show_additional() {
                            div { class: "pf-additional-body",
                                div { class: "pf-additional-group",
                                    div { class: "pf-additional-group-title", {i18n.t("person_form.birth")} }
                                    div { class: "form-group",
                                        label { {i18n.t("person_form.calendar")} }
                                        select {
                                            value: "{birth_calendar}",
                                            oninput: move |e: Event<FormData>| { birth_calendar.set(e.value()); has_changes.set(true); },
                                            {calendar_options(&i18n)}
                                        }
                                    }
                                    div { class: "form-group",
                                        label { {i18n.t("person_form.witnesses")} }
                                        {render_witnesses(&i18n, &birth_witnesses, &mut has_changes)}
                                        button {
                                            class: "btn btn-outline btn-sm",
                                            r#type: "button",
                                            onclick: move |_| {
                                                let mut ws = birth_witnesses();
                                                ws.push(String::new());
                                                birth_witnesses.set(ws);
                                                has_changes.set(true);
                                            },
                                            {i18n.t("person_form.add_witness")}
                                        }
                                    }
                                }

                                div { class: "pf-additional-group",
                                    div { class: "pf-additional-group-title", {i18n.t("person_form.death")} }
                                    div { class: "form-group",
                                        label { {i18n.t("person_form.calendar")} }
                                        select {
                                            value: "{death_calendar}",
                                            oninput: move |e: Event<FormData>| { death_calendar.set(e.value()); has_changes.set(true); },
                                            {calendar_options(&i18n)}
                                        }
                                    }
                                    div { class: "form-group",
                                        label { {i18n.t("person_form.witnesses")} }
                                        {render_witnesses(&i18n, &death_witnesses, &mut has_changes)}
                                        button {
                                            class: "btn btn-outline btn-sm",
                                            r#type: "button",
                                            onclick: move |_| {
                                                let mut ws = death_witnesses();
                                                ws.push(String::new());
                                                death_witnesses.set(ws);
                                                has_changes.set(true);
                                            },
                                            {i18n.t("person_form.add_witness")}
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // ── Other Events (edit mode only) ──
                    if !is_create { div { class: "person-form-section",
                        div { class: "section-header",
                            div { class: "pf-section-title has-action", {i18n.t("person_form.other_events")} }
                            button {
                                class: "btn btn-primary btn-sm",
                                onclick: move |_| show_event_form.toggle(),
                                if show_event_form() { {i18n.t("common.cancel")} } else { {i18n.t("person_form.add_event")} }
                            }
                        }

                        if show_event_form() {
                            div { style: "padding: 12px; background: var(--color-bg); border-radius: var(--radius); margin-bottom: 12px;",
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
                                        select {
                                            value: "{event_form_place_id}",
                                            oninput: move |e: Event<FormData>| event_form_place_id.set(e.value()),
                                            option { value: "", {i18n.t("person_form.no_place")} }
                                            for (pid_opt, pname) in place_options.iter() {
                                                option { value: "{pid_opt}", "{pname}" }
                                            }
                                        }
                                    }
                                    div { class: "form-group",
                                        label { {i18n.t("person_form.cause")} }
                                        input {
                                            r#type: "text",
                                            value: "{event_form_cause}",
                                            oninput: move |e: Event<FormData>| event_form_cause.set(e.value()),
                                        }
                                    }
                                }
                                div { class: "form-group",
                                    label { {i18n.t("person_form.note")} }
                                    input {
                                        r#type: "text",
                                        value: "{event_form_note}",
                                        oninput: move |e: Event<FormData>| event_form_note.set(e.value()),
                                    }
                                }
                                button {
                                    class: "btn btn-primary btn-sm",
                                    onclick: on_create_event,
                                    {i18n.t("person.create_event")}
                                }
                            }
                        }

                        if other_events.is_empty() {
                            div { class: "empty-state", p { {i18n.t("person_form.no_other_events")} } }
                        } else {
                            for ev in other_events.iter() {
                                {
                                    let eid = ev.id;
                                    let et = format!("{}", ev.event_type);
                                    let date = ev.date_value.clone().unwrap_or_default();
                                    let place = ev.place_id.map(&place_name).unwrap_or_default();
                                    rsx! {
                                        div { class: "person-form-item",
                                            div { class: "person-form-item-info",
                                                span { class: "badge", "{et}" }
                                                if !date.is_empty() { span { "{date}" } }
                                                if !place.is_empty() { span { class: "text-muted", "@ {place}" } }
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
                                    }
                                }
                            }
                        }
                    } } // end Other Events if !is_create

                    // ── Notes (edit mode only) ──
                    if !is_create { div { class: "person-form-section",
                        div { class: "section-header",
                            div { class: "pf-section-title has-action", {i18n.t("person_form.notes")} }
                            button {
                                class: "btn btn-primary btn-sm",
                                onclick: move |_| show_note_form.toggle(),
                                if show_note_form() { {i18n.t("common.cancel")} } else { {i18n.t("person_form.add_note")} }
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
                                        placeholder: "{i18n.t(\"person_form.note_placeholder\")}",
                                        value: "{note_form_text}",
                                        oninput: move |e: Event<FormData>| note_form_text.set(e.value()),
                                    }
                                }
                                button {
                                    class: "btn btn-primary btn-sm",
                                    onclick: on_create_note,
                                    {i18n.t("person.create_note")}
                                }
                            }
                        }

                        if notes_list.is_empty() {
                            div { class: "empty-state", p { {i18n.t("person_form.no_notes")} } }
                        } else {
                            for note in notes_list.iter() {
                                {
                                    let nid = note.id;
                                    let text = note.text.clone();
                                    let preview = if text.len() > 120 { format!("{}…", &text[..120]) } else { text };
                                    rsx! {
                                        div { class: "person-form-item",
                                            div { class: "person-form-item-info", span { "{preview}" } }
                                            div { class: "person-form-item-actions",
                                                button {
                                                    class: "btn btn-danger btn-sm",
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
                    } } // end Notes if !is_create

                    // ── Delete Person (edit mode only) ──
                    if !is_create && !is_embedded { div { class: "pf-delete-section",
                        if show_delete_confirm() {
                            div { class: "pf-delete-confirm",
                                p { class: "pf-delete-confirm-name",
                                    {format!("{} {}?", i18n.t("person_form.delete_confirm_title"), display_name)}
                                }
                                p { class: "pf-delete-confirm-message",
                                    {i18n.t("person_form.delete_confirm_message")}
                                }
                                if let Some(err) = delete_error() {
                                    div { class: "error-msg", "{err}" }
                                }
                                div { class: "pf-delete-confirm-actions",
                                    button {
                                        class: "btn btn-outline btn-sm",
                                        r#type: "button",
                                        disabled: deleting(),
                                        onclick: move |_| { show_delete_confirm.set(false); delete_error.set(None); },
                                        {i18n.t("common.cancel")}
                                    }
                                    button {
                                        class: "btn btn-danger btn-sm",
                                        r#type: "button",
                                        disabled: deleting(),
                                        onclick: on_confirm_delete,
                                        if deleting() { {i18n.t("person_form.deleting")} } else { {i18n.t("person_form.delete_confirm_button")} }
                                    }
                                }
                            }
                        } else {
                            hr { class: "pf-delete-divider" }
                            button {
                                class: "pf-delete-person-btn",
                                r#type: "button",
                                onclick: move |_| show_delete_confirm.set(true),
                                {i18n.t("person_form.delete_person")}
                            }
                        }
                    } } // end Delete if !is_create && !is_embedded
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
        div { class: "modal-backdrop person-form-backdrop",
            onclick: try_close,

            div {
                class: "person-form-modal",
                onclick: move |evt| evt.stop_propagation(),
                onkeydown: move |e: Event<KeyboardData>| {
                    match e.key() {
                        Key::Escape => {
                            if has_changes() { show_discard_confirm.set(true); }
                            else { props.on_close.call(()); }
                        }
                        Key::Enter => {
                            document::eval(
                                "var a=document.activeElement;\
                                if(a&&a.tagName==='INPUT'&&a.type!=='button'&&a.type!=='submit'){\
                                    var m=a.closest('.person-form-modal');\
                                    if(!m)return;\
                                    var fs=[...m.querySelectorAll('input:not([type=button]):not([type=submit]),select,textarea')];\
                                    var i=fs.indexOf(a);\
                                    if(i>=0&&i<fs.length-1)fs[i+1].focus();\
                                }"
                            );
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

fn qualifier_options(i18n: &crate::i18n::I18n) -> Element {
    let i18n = *i18n;
    rsx! {
        option { value: "Exact",   {i18n.t("date_qualifier.exact")} }
        option { value: "About",   {i18n.t("date_qualifier.about")} }
        option { value: "Perhaps", {i18n.t("date_qualifier.perhaps")} }
        option { value: "Before",  {i18n.t("date_qualifier.before")} }
        option { value: "After",   {i18n.t("date_qualifier.after")} }
        option { value: "Or",      {i18n.t("date_qualifier.or")} }
        option { value: "Between", {i18n.t("date_qualifier.between")} }
    }
}

fn calendar_options(i18n: &crate::i18n::I18n) -> Element {
    let i18n = *i18n;
    rsx! {
        option { value: "Gregorian",         {i18n.t("calendar.gregorian")} }
        option { value: "Julian",            {i18n.t("calendar.julian")} }
        option { value: "Hebrew",            {i18n.t("calendar.hebrew")} }
        option { value: "FrenchRepublican",  {i18n.t("calendar.french_republican")} }
    }
}

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
        optgroup { label: "{i18n.t(\"person_form.legal\")}",
            option { value: "Will",    {i18n.t("event.type.will")} }
            option { value: "Probate", {i18n.t("event.type.probate")} }
        }
        optgroup { label: "{i18n.t(\"person_form.other_events\")}",
            option { value: "Adoption", {i18n.t("event.type.adoption")} }
            option { value: "Other",    {i18n.t("event.type.other")} }
        }
    }
}

// ── Witnesses widget ──────────────────────────────────────────────────────

fn render_witnesses(
    i18n: &crate::i18n::I18n,
    witnesses: &Signal<Vec<String>>,
    has_changes: &mut Signal<bool>,
) -> Element {
    let _i18n = *i18n;
    let mut witnesses_sig = *witnesses;
    let mut changes_sig = *has_changes;
    let count = witnesses_sig().len();

    rsx! {
        for i in 0..count {
            div { class: "pf-witness-row",
                input {
                    r#type: "text",
                    placeholder: "Witness name",
                    value: "{witnesses_sig()[i]}",
                    oninput: move |e: Event<FormData>| {
                        let mut ws = witnesses_sig();
                        ws[i] = e.value();
                        witnesses_sig.set(ws);
                        changes_sig.set(true);
                    },
                }
                button {
                    class: "pf-witness-remove",
                    r#type: "button",
                    onclick: move |_| {
                        let mut ws = witnesses_sig();
                        ws.remove(i);
                        witnesses_sig.set(ws);
                        changes_sig.set(true);
                    },
                    "\u{00D7}"
                }
            }
        }
    }
}

// ── Name form helper ──────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn render_name_form(
    i18n: &crate::i18n::I18n,
    error: &Signal<Option<String>>,
    hide_create_btn: bool,
    name_type_mut: &mut Signal<String>,
    given_mut: &mut Signal<String>,
    surname_mut: &mut Signal<String>,
    prefix_mut: &mut Signal<String>,
    suffix_mut: &mut Signal<String>,
    nickname_mut: &mut Signal<String>,
    primary_mut: &mut Signal<bool>,
    on_create: impl FnMut(Event<MouseData>) + 'static,
) -> Element {
    let i18n = *i18n;
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
                    label { {i18n.t("person_form.name_type")} }
                    select {
                        value: "{name_type_sig}",
                        oninput: move |e: Event<FormData>| name_type_sig.set(e.value()),
                        option { value: "Birth",      {i18n.t("name_type.birth")} }
                        option { value: "Married",    {i18n.t("name_type.married")} }
                        option { value: "AlsoKnownAs",{i18n.t("name_type.also_known_as")} }
                        option { value: "Maiden",     {i18n.t("name_type.maiden")} }
                        option { value: "Religious",  {i18n.t("name_type.religious")} }
                        option { value: "Other",      {i18n.t("name_type.other")} }
                    }
                }
                div { class: "form-group",
                    label { {i18n.t("person_form.primary")} }
                    select {
                        value: if primary_sig() { "true" } else { "false" },
                        oninput: move |e: Event<FormData>| primary_sig.set(e.value() == "true"),
                        option { value: "true",  {i18n.t("common.yes")} }
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
                        value: "{given_sig}",
                        oninput: move |e: Event<FormData>| given_sig.set(e.value()),
                    }
                }
                div { class: "form-group",
                    label { {i18n.t("person_form.surname")} }
                    input {
                        r#type: "text",
                        placeholder: "{i18n.t(\"person_form.surname_placeholder\")}",
                        value: "{surname_sig}",
                        oninput: move |e: Event<FormData>| surname_sig.set(e.value().to_uppercase()),
                    }
                }
            }
            div { class: "form-row",
                div { class: "form-group",
                    label { {i18n.t("person_form.prefix")} }
                    input { r#type: "text", placeholder: "{i18n.t(\"person_form.prefix_placeholder\")}", value: "{prefix_sig}", oninput: move |e: Event<FormData>| prefix_sig.set(e.value()) }
                }
                div { class: "form-group",
                    label { {i18n.t("person_form.suffix")} }
                    input { r#type: "text", placeholder: "{i18n.t(\"person_form.suffix_placeholder\")}", value: "{suffix_sig}", oninput: move |e: Event<FormData>| suffix_sig.set(e.value()) }
                }
                div { class: "form-group",
                    label { {i18n.t("person_form.nickname")} }
                    input { r#type: "text", placeholder: "{i18n.t(\"person_form.nickname_placeholder\")}", value: "{nickname_sig}", oninput: move |e: Event<FormData>| nickname_sig.set(e.value()) }
                }
            }
            if !hide_create_btn {
                button { class: "btn btn-primary btn-sm", onclick: on_create, {i18n.t("person.create_name")} }
            }
        }
    }
}
