//! What the person form holds and loads: the fields its footer Save writes,
//! the person's stored records, and the effects seeding the first from the
//! second.

use dioxus::prelude::*;
use oxidgene_core::types::{Event as CoreEvent, Note as CoreNote, PersonName, Place};
use oxidgene_core::{EventType, NameType, Privacy, Sex};
use uuid::Uuid;

use super::sections::VitalDraft;
use super::{
    NewPerson, NotesSource, PersonFormCreateContext, Written, load_notes_source,
    override_for_stored, without_notes,
};
use crate::api::{ApiClient, ApiError, PersonDetail};
use crate::components::date_input::DateParts;
use crate::i18n::I18n;
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
    /// Create mode: the person an earlier Create created before a later
    /// step failed, which the next one goes on with.
    pub created: Signal<Option<Uuid>>,
    /// Create mode: the person is linked as the parent the form adds.
    pub linked: Signal<bool>,
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
            created: use_signal(|| None::<Uuid>),
            linked: use_signal(|| false),
        }
    }

    /// What is stored of the records the footer Save writes, as a save
    /// starts from.
    pub(super) fn written(&self) -> Written {
        let vital = |draft: VitalDraft| (*draft.event_id.peek(), draft.stored.peek().clone());
        Written {
            person_id: *self.created.peek(),
            birth_name_id: *self.birth_name_id.peek(),
            vitals: [vital(self.birth), vital(self.death)],
            source: without_notes(self.source_stored.peek().clone()),
            linked: *self.linked.peek(),
        }
    }

    /// Keeps what a save wrote, whether it went through or failed halfway:
    /// the next save updates those records rather than creating them again.
    pub(super) fn record(&self, written: Written) {
        let Self {
            mut created,
            mut birth_name_id,
            mut source_stored,
            mut linked,
            ..
        } = *self;
        let [birth, death] = written.vitals;
        for (draft, (id, stored)) in [(self.birth, birth), (self.death, death)] {
            let VitalDraft {
                mut event_id,
                stored: mut held,
                ..
            } = draft;
            event_id.set(id);
            held.set(stored);
        }
        created.set(written.person_id);
        birth_name_id.set(written.birth_name_id);
        source_stored.set(written.source);
        linked.set(written.linked);
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
            privacy: (self.privacy)(),
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
    pub vital_sources: Resource<Result<(NotesSource, NotesSource), ApiError>>,
    pub person_source: Resource<Result<NotesSource, ApiError>>,
}

impl FormLoads {
    /// Why a load the footer Save depends on failed, if one did: the Save
    /// is then refused, as it would write defaults over what is stored.
    pub(super) fn save_blocker(&self, i18n: &I18n) -> Option<String> {
        fn error<T>(resource: &Resource<Result<T, ApiError>>, i18n: &I18n) -> Option<String> {
            match &*resource.read() {
                Some(Err(e)) => Some(e.message(i18n)),
                _ => None,
            }
        }
        error(&self.person, i18n)
            .or_else(|| error(&self.names, i18n))
            .or_else(|| error(&self.events, i18n))
            .or_else(|| error(&self.vital_sources, i18n))
            .or_else(|| error(&self.person_source, i18n))
    }
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
                    None => Ok(Default::default()),
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
                    None => Ok(NotesSource::default()),
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
async fn load_vital_sources(
    api: &ApiClient,
    tid: Uuid,
    pid: Uuid,
) -> Result<(NotesSource, NotesSource), ApiError> {
    let events = api.list_events(tid, Some(pid), None).await?;
    let [birth, death] = vital_ids(&events);
    let load = |event_id: Option<Uuid>| async move {
        match event_id {
            Some(eid) => load_notes_source(api, tid, Some(pid), Some(eid)).await,
            None => Ok(NotesSource::default()),
        }
    };
    Ok((load(birth).await?, load(death).await?))
}

/// The birth and the death the form edits among `events`: the first of
/// each, the same one for its date, its place, its notes and its source.
/// Any other is listed with the other events.
pub(super) fn vital_ids(events: &[CoreEvent]) -> [Option<Uuid>; 2] {
    [EventType::Birth, EventType::Death].map(|event_type| {
        events
            .iter()
            .find(|event| event.event_type == event_type)
            .map(|event| event.id)
    })
}

/// Fills the held fields from what is stored, once each.
///
/// Effects, not writes in the body: a write while rendering schedules a
/// second render, and the flags read with `peek` keep a reload (after a
/// save's `refresh`) from overwriting what the user typed since.
///
/// A person being created has nothing stored to seed from; its sources are
/// left as the user types them.
///
/// Returns whether every field the footer Save writes holds what is stored:
/// until then, the Save would write the fields' defaults — sex unknown,
/// privacy following the tree — over the person.
pub(super) fn use_seeded_fields(
    data: FormLoads,
    fields: HeldFields,
    is_create: bool,
) -> Memo<bool> {
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
        if let (Some(Ok((birth, death))), Some(Ok(person))) =
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

    use_memo(move || {
        is_create || (person_seeded() && vitals_seeded() && name_seeded() && sources_seeded())
    })
}

/// The stored birth and death: their ids, dates and places — the first of
/// each, as [`vital_ids`] picks them.
fn seed_vitals(events: &[CoreEvent], fields: HeldFields) {
    for (draft, id) in [fields.birth, fields.death]
        .into_iter()
        .zip(vital_ids(events))
    {
        let Some(ev) = id.and_then(|id| events.iter().find(|event| event.id == id)) else {
            continue;
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
