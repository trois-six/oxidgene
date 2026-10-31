//! What the person form holds and loads: the fields its footer Save writes,
//! the person's stored records, and the effects seeding the first from the
//! second.

use dioxus::prelude::*;
use oxidgene_core::types::{Event as CoreEvent, Note as CoreNote, PersonName, Place};
use oxidgene_core::{EventType, NameType, Privacy, Sex};
use uuid::Uuid;

use super::sections::VitalDraft;
use super::{
    NewPerson, NotesSource, PersonFormCreateContext, load_notes_source, override_for_stored,
};
use crate::api::{ApiClient, ApiError, PersonDetail};
use crate::components::date_input::DateParts;
use crate::ui_observability::use_ui_resource;

/// The fields the footer Save writes: the person's sex and privacy, its
/// birth name, its birth and death, and its own source.
#[derive(Clone, Copy)]
pub(super) struct HeldFields {
    pub sex: Signal<Sex>,
    pub privacy: Signal<Privacy>,
    /// The stored birth name, which the Save updates rather than creating
    /// another. Managed under Civil Status rather than through the list of
    /// further information: there is always exactly one birth name.
    pub birth_name_id: Signal<Option<Uuid>>,
    pub given: Signal<String>,
    pub surname: Signal<String>,
    /// `None` = trust automatic particle detection; `Some(p)` = the user
    /// took control, where an empty `p` means "no particle".
    pub particle_override: Signal<Option<String>>,
    pub birth: VitalDraft,
    pub death: VitalDraft,
    /// The person's own source; its notes are the list under Civil Status.
    pub source: Signal<String>,
    /// The source stored, which the Save reconciles against.
    pub source_stored: Signal<NotesSource>,
}

impl HeldFields {
    /// The fields of a new form. A parent being added starts with the sex
    /// the context implies, and a father with the child's surname.
    pub(super) fn use_new(context: Option<&PersonFormCreateContext>) -> Self {
        let initial_sex = match context {
            Some(PersonFormCreateContext::AddParent {
                is_father: true, ..
            }) => Sex::Male,
            Some(PersonFormCreateContext::AddParent { .. }) => Sex::Female,
            _ => Sex::Unknown,
        };
        let initial_surname = context
            .and_then(PersonFormCreateContext::initial_surname)
            .unwrap_or_default();
        Self {
            sex: use_signal(|| initial_sex),
            privacy: use_signal(|| Privacy::Default),
            birth_name_id: use_signal(|| None::<Uuid>),
            given: use_signal(String::new),
            surname: use_signal(|| initial_surname),
            particle_override: use_signal(|| None::<String>),
            birth: VitalDraft::use_new(),
            death: VitalDraft::use_new(),
            source: use_signal(String::new),
            source_stored: use_signal(NotesSource::default),
        }
    }

    /// What the form holds now.
    pub(super) fn held(&self) -> NewPerson {
        let vital = |draft: VitalDraft| {
            (
                (draft.parts)(),
                (draft.place_id)(),
                (draft.notes)(),
                (draft.source)(),
            )
        };
        NewPerson {
            sex: (self.sex)(),
            given: (self.given)().trim().to_string(),
            surname: (self.surname)().trim().to_string(),
            particle_override: (self.particle_override)(),
            birth: vital(self.birth),
            death: vital(self.death),
            source: (self.source)(),
        }
    }
}

/// What the form loads of the person it edits. A person being created has
/// nothing stored: its loads answer empty.
#[derive(Clone, Copy)]
pub(super) struct FormLoads {
    /// `None` while the person is being created.
    pub person: Resource<Result<Option<PersonDetail>, ApiError>>,
    pub names: Resource<Result<Vec<PersonName>, ApiError>>,
    pub events: Resource<Result<Vec<CoreEvent>, ApiError>>,
    /// The places the person's events sit on — the only places the form
    /// shows without being asked; suggestions come from the server.
    pub places: Resource<Result<Vec<Place>, ApiError>>,
    pub notes: Resource<Result<Vec<CoreNote>, ApiError>>,
    /// The notes and source of the birth, then of the death.
    pub vital_sources: Resource<(NotesSource, NotesSource)>,
    pub person_source: Resource<NotesSource>,
}

/// The person form's loads, read again whenever `refresh` changes.
pub(super) fn use_form_data(
    api: &ApiClient,
    tid: Uuid,
    pid: Uuid,
    is_create: bool,
    refresh: Signal<u32>,
) -> FormLoads {
    // Nothing is stored for a person being created: each load answers as
    // for a person without records, and none reaches the server.
    let stored = move || (!is_create).then_some(pid);

    let person = use_ui_resource("form_person", {
        let api = api.clone();
        move || {
            let (api, _tick) = (api.clone(), refresh());
            async move {
                match stored() {
                    Some(pid) => api.get_person(tid, pid).await.map(Some),
                    None => Ok(None),
                }
            }
        }
    });
    let names = use_ui_resource("form_names", {
        let api = api.clone();
        move || {
            let (api, _tick) = (api.clone(), refresh());
            async move {
                match stored() {
                    Some(pid) => api.list_person_names(tid, pid).await,
                    None => Ok(Vec::new()),
                }
            }
        }
    });
    let events = use_ui_resource("form_events", {
        let api = api.clone();
        move || {
            let (api, _tick) = (api.clone(), refresh());
            async move {
                match stored() {
                    Some(pid) => api.list_events(tid, Some(pid), None).await,
                    None => Ok(Vec::new()),
                }
            }
        }
    });
    let places = use_ui_resource("form_places", {
        let api = api.clone();
        move || {
            let api = api.clone();
            let ids = match &*events.read() {
                Some(Ok(events)) => oxidgene_core::collections::sorted_unique(
                    events.iter().filter_map(|event| event.place_id),
                ),
                _ => Vec::new(),
            };
            async move { api.places_by_ids(tid, &ids).await }
        }
    });
    let notes = use_ui_resource("form_notes", {
        let api = api.clone();
        move || {
            let (api, _tick) = (api.clone(), refresh());
            async move {
                match stored() {
                    Some(pid) => api.list_notes(tid, Some(pid), None, None, None, None).await,
                    None => Ok(Vec::new()),
                }
            }
        }
    });
    let vital_sources = use_ui_resource("form_vital_sources", {
        let api = api.clone();
        move || {
            let (api, _tick) = (api.clone(), refresh());
            async move {
                match stored() {
                    Some(pid) => load_vital_sources(&api, tid, pid).await,
                    None => Default::default(),
                }
            }
        }
    });
    let person_source = use_ui_resource("form_person_source", {
        let api = api.clone();
        move || {
            let (api, _tick) = (api.clone(), refresh());
            async move {
                match stored() {
                    Some(pid) => load_notes_source(&api, tid, Some(pid), None).await,
                    None => NotesSource::default(),
                }
            }
        }
    });
    FormLoads {
        person,
        names,
        events,
        places,
        notes,
        vital_sources,
        person_source,
    }
}

/// The notes and source of a person's birth, then of its death. The events
/// are read again here (the GET is cached) so the ids loaded against are
/// always the ones just seen, rather than a signal that may not have caught
/// up.
async fn load_vital_sources(api: &ApiClient, tid: Uuid, pid: Uuid) -> (NotesSource, NotesSource) {
    let events = api
        .list_events(tid, Some(pid), None)
        .await
        .unwrap_or_default();
    let of = |event_type: EventType| {
        events
            .iter()
            .find(|event| event.event_type == event_type)
            .map(|event| event.id)
    };
    let (birth, death) = (of(EventType::Birth), of(EventType::Death));
    let load = |event_id: Option<Uuid>| async move {
        match event_id {
            Some(eid) => load_notes_source(api, tid, Some(pid), Some(eid)).await,
            None => NotesSource::default(),
        }
    };
    (load(birth).await, load(death).await)
}

/// Fills the held fields from what is stored, once each.
///
/// Effects, not writes in the body: a write while rendering schedules a
/// second render, and the flags read with `peek` keep a reload (after a
/// save's `refresh`) from overwriting what the user typed since.
///
/// A person being created has nothing stored to seed from; its sources are
/// left as the user types them.
pub(super) fn use_seeded_fields(data: FormLoads, fields: HeldFields, is_create: bool) {
    let mut person_seeded = use_signal(|| false);
    use_effect(move || {
        if let Some(Ok(Some(person))) = &*data.person.read()
            && !*person_seeded.peek()
        {
            let HeldFields {
                mut sex,
                mut privacy,
                ..
            } = fields;
            sex.set(person.sex);
            privacy.set(person.privacy);
            person_seeded.set(true);
        }
    });

    let mut vitals_seeded = use_signal(|| false);
    use_effect(move || {
        if let Some(Ok(events)) = &*data.events.read()
            && !*vitals_seeded.peek()
        {
            seed_vitals(events, fields);
            vitals_seeded.set(true);
        }
    });

    let mut sources_seeded = use_signal(|| false);
    use_effect(move || {
        if let (Some((birth, death)), Some(person)) =
            (&*data.vital_sources.read(), &*data.person_source.read())
            && !is_create
            && !*sources_seeded.peek()
        {
            seed_sources(fields, [birth, death], person);
            sources_seeded.set(true);
        }
    });

    let mut name_seeded = use_signal(|| false);
    use_effect(move || {
        if let Some(Ok(names)) = &*data.names.read()
            && !*name_seeded.peek()
        {
            seed_birth_name(fields, names);
            name_seeded.set(true);
        }
    });
}

/// The stored birth and death: their ids, dates and places.
fn seed_vitals(events: &[CoreEvent], fields: HeldFields) {
    for ev in events {
        let draft = match ev.event_type {
            EventType::Birth => fields.birth,
            EventType::Death => fields.death,
            _ => continue,
        };
        let VitalDraft {
            mut event_id,
            mut parts,
            mut place_id,
            ..
        } = draft;
        event_id.set(Some(ev.id));
        parts.set(DateParts::from_fields(
            ev.calendar,
            ev.date_qualifier,
            ev.date_value.as_deref(),
            ev.date_value2.as_deref(),
        ));
        place_id.set(ev.place_id.map(|id| id.to_string()).unwrap_or_default());
    }
}

/// The stored notes and sources of the birth and death, and the person's
/// own source.
fn seed_sources(fields: HeldFields, vitals: [&NotesSource; 2], person: &NotesSource) {
    for (draft, stored) in [fields.birth, fields.death].into_iter().zip(vitals) {
        let VitalDraft {
            mut notes,
            mut source,
            stored: mut held,
            ..
        } = draft;
        notes.set(stored.notes.clone());
        source.set(stored.source_title.clone());
        held.set(stored.clone());
    }
    let HeldFields {
        mut source,
        mut source_stored,
        ..
    } = fields;
    source.set(person.source_title.clone());
    source_stored.set(person.clone());
}

/// The stored birth name: the primary birth name, else the primary name,
/// else a birth name.
fn seed_birth_name(fields: HeldFields, names: &[PersonName]) {
    let primary = names
        .iter()
        .find(|n| n.name_type == NameType::Birth && n.is_primary)
        .or_else(|| names.iter().find(|n| n.is_primary))
        .or_else(|| names.iter().find(|n| n.name_type == NameType::Birth));
    let Some(name) = primary else { return };
    let HeldFields {
        mut birth_name_id,
        mut given,
        mut surname,
        mut particle_override,
        ..
    } = fields;
    birth_name_id.set(Some(name.id));
    given.set(name.given_names.clone().unwrap_or_default());
    // Edited as one field, particle included — re-split on save.
    let full = name.full_surname().unwrap_or_default();
    particle_override.set(override_for_stored(&full, name.surname_prefix.as_deref()));
    surname.set(full);
}
