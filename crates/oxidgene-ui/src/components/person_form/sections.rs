//! The person form's sections that save on their own: professions, further
//! information, notes and other events, each a component holding the state
//! of its add and edit sub-forms, and the birth and death sections, which
//! ride the footer Save.
//!
//! [`FormScope`] is what every section shares with the form: the person,
//! how a write that landed is announced, and the create form's way of
//! creating the person before a part that needs its id.

use dioxus::prelude::*;
use oxidgene_core::EventType;
use oxidgene_core::types::{Event as CoreEvent, Note as CoreNote, PersonName, Place};
use uuid::Uuid;

use super::{
    EventEditor, EventExtras, EventOwner, EventWitnesses, FormOpening, FormSection, MoreDetails,
    NotesSource, build_information_body, create_event_body, event_type_options,
    override_for_stored, render_add_creating, render_add_toggle, render_agency_field,
    render_cause_age_fields, render_information_form, render_notes_source_fields,
    render_particle_row, resolve_particle, save_notes_source,
};
use crate::api::{
    ApiClient, ApiError, CreateNoteBody, SuggestionField, UpdateNoteBody, UpdatePersonNameBody,
};
use crate::components::date_input::{DateInput, DateParts, format_event_date};
use crate::components::place_input::{render_place_input, resolve_place};
use crate::components::suggest_input::ValueInput;
use crate::i18n::I18n;
use crate::utils::{
    NAME_TYPES, event_type_label_key, event_type_value, name_type_label_key, name_type_value,
    opt_str, parse_event_type, parse_name_type,
};

/// What every section of the person form shares.
#[derive(Clone, Copy, PartialEq)]
pub(super) struct FormScope {
    pub i18n: I18n,
    pub tid: Uuid,
    /// The person edited; the nil id while it is being created.
    pub pid: Uuid,
    pub is_create: bool,
    /// Bumped once a write lands, to reload the form's lists.
    pub refresh: Signal<u32>,
    /// The footer's error, where a row's failed deletion is said.
    pub save_error: Signal<Option<String>>,
    pub on_saved: EventHandler<()>,
    /// Create mode: creates the person, then opens this part of its edit
    /// form.
    pub promote: Callback<FormOpening>,
    /// Which profession or other event has its editor open — at most one
    /// across both lists.
    pub open_event_notes: Signal<Option<Uuid>>,
}

impl FormScope {
    /// A write landed: the host refreshes, the form reloads its lists.
    fn landed(mut self) {
        self.on_saved.call(());
        self.refresh += 1;
    }

    /// The add button of a part: it toggles the part's sub-form, or, while
    /// the person is being created, creates it first.
    fn add_button(self, label: String, show: Signal<bool>, opening: FormOpening) -> Element {
        if self.is_create {
            render_add_creating(label, self.promote, opening)
        } else {
            render_add_toggle(label, self.i18n.t("common.cancel"), show)
        }
    }

    /// Deletes one of the person's events, saying in the footer why it
    /// failed.
    fn delete_event(self, api: ApiClient, event_id: Uuid) -> impl FnMut(Event<MouseData>) {
        move |_| {
            let api = api.clone();
            let mut save_error = self.save_error;
            spawn(async move {
                match api.delete_event(self.tid, event_id).await {
                    Ok(_) => self.landed(),
                    Err(e) => save_error.set(Some(e.message(&self.i18n))),
                }
            });
        }
    }
}

/// The places the person's events sit on, as `(id, name)` options.
pub(super) fn place_options(
    places: &Resource<Result<Vec<Place>, ApiError>>,
) -> Vec<(String, String)> {
    match &*places.read() {
        Some(Ok(places)) => places
            .iter()
            .map(|p| (p.id.to_string(), p.name.clone()))
            .collect(),
        _ => Vec::new(),
    }
}

/// The name of `place_id` among `options`. An unknown or not-yet-loaded
/// place resolves to nothing rather than to a slice of its UUID: callers
/// hide the place when this is empty, and a bare "019fccf3" told the reader
/// less than showing no place at all.
fn place_name(options: &[(String, String)], place_id: Uuid) -> String {
    let id = place_id.to_string();
    options
        .iter()
        .find(|(option, _)| *option == id)
        .map(|(_, name)| name.clone())
        .unwrap_or_default()
}

/// The person's loaded events `keep` retains.
fn owner_events(
    events: &Resource<Result<Vec<CoreEvent>, ApiError>>,
    keep: impl Fn(EventType) -> bool,
) -> Vec<CoreEvent> {
    match &*events.read() {
        Some(Ok(events)) => events
            .iter()
            .filter(|e| keep(e.event_type))
            .cloned()
            .collect(),
        _ => Vec::new(),
    }
}

// ── Birth and death ──────────────────────────────────────────────────────

/// A birth or death as the form holds it, saved with the footer button.
#[derive(Clone, Copy)]
pub(super) struct VitalDraft {
    /// Calendar, qualifier and the day/month/year triplet, as the
    /// `DateInput` widget edits them.
    pub parts: Signal<DateParts>,
    pub place_id: Signal<String>,
    /// The stored event, once there is one.
    pub event_id: Signal<Option<Uuid>>,
    pub notes: Signal<String>,
    pub source: Signal<String>,
    /// The notes and source stored, which a save reconciles against.
    pub stored: Signal<NotesSource>,
}

impl VitalDraft {
    pub(super) fn use_new() -> Self {
        Self {
            parts: use_signal(DateParts::default),
            place_id: use_signal(String::new),
            event_id: use_signal(|| None::<Uuid>),
            notes: use_signal(String::new),
            source: use_signal(String::new),
            stored: use_signal(NotesSource::default),
        }
    }
}

/// The Birth or the Death section.
pub(super) fn vital_section(
    scope: FormScope,
    title: String,
    open: Signal<bool>,
    draft: VitalDraft,
    place_options: &[(String, String)],
    mut has_changes: Signal<bool>,
) -> Element {
    let i18n = scope.i18n;
    rsx! {
        FormSection { title, open,
            div { class: "form-group",
                label { {i18n.t("person_form.date")} }
                DateInput {
                    parts: draft.parts,
                    i18n,
                    on_change: move |()| has_changes.set(true),
                }
            }
            {render_place_input(&i18n, scope.tid, draft.place_id, place_options, move || has_changes.set(true))}
            {render_notes_source_fields(&i18n, scope.tid, draft.notes, draft.source, move || has_changes.set(true))}
            div { class: "form-group",
                label { {i18n.t("person_form.witnesses")} }
                EventWitnesses { tree_id: scope.tid, event_id: (draft.event_id)() }
            }
        }
    }
}

// ── Professions ──────────────────────────────────────────────────────────

/// The fields of a profession being added.
#[derive(Clone, Copy)]
struct ProfessionDraft {
    label: Signal<String>,
    parts: Signal<DateParts>,
    place_id: Signal<String>,
    notes: Signal<String>,
    source: Signal<String>,
    error: Signal<Option<String>>,
}

impl ProfessionDraft {
    fn use_new() -> Self {
        Self {
            label: use_signal(String::new),
            parts: use_signal(DateParts::default),
            place_id: use_signal(String::new),
            notes: use_signal(String::new),
            source: use_signal(String::new),
            error: use_signal(|| None::<String>),
        }
    }

    fn clear(mut self) {
        self.label.set(String::new());
        self.parts.set(DateParts::default());
        self.place_id.set(String::new());
        self.notes.set(String::new());
        self.source.set(String::new());
        self.error.set(None);
    }

    /// Creates the profession, with its notes and source.
    fn create(self, scope: FormScope, api: ApiClient, mut show: Signal<bool>) {
        let FormScope { i18n, tid, pid, .. } = scope;
        let mut error = self.error;
        let label = (self.label)().trim().to_string();
        let parts = (self.parts)();
        let place_str = (self.place_id)();
        let notes = (self.notes)().trim().to_string();
        let source = (self.source)();
        spawn(async move {
            if label.is_empty() {
                error.set(Some(i18n.t("person_form.profession_required")));
                return;
            }
            if let Some(key) = parts.validate() {
                error.set(Some(i18n.t(key)));
                return;
            }
            let place_id = match resolve_place(&api, tid, &place_str, i18n.0.reference_code()).await
            {
                Ok(place_id) => place_id,
                Err(e) => {
                    error.set(Some(e.message(&i18n)));
                    return;
                }
            };
            let body = create_event_body(
                EventType::Occupation,
                &parts,
                place_id,
                EventOwner::Person(pid),
                opt_str(&label),
                EventExtras::default(),
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
                    show.set(false);
                    self.clear();
                    scope.landed();
                }
                Err(e) => error.set(Some(e.message(&i18n))),
            }
        });
    }
}

/// The person's professions under Civil Status: the list, and the form
/// adding one.
#[component]
pub(super) fn ProfessionsBlock(
    scope: FormScope,
    events: Resource<Result<Vec<CoreEvent>, ApiError>>,
    places: Resource<Result<Vec<Place>, ApiError>>,
    /// Open the add form on mount: what the user asked for while creating.
    open: bool,
) -> Element {
    let api = use_context::<ApiClient>();
    let show = use_signal(|| open);
    let draft = ProfessionDraft::use_new();
    let FormScope { i18n, tid, pid, .. } = scope;
    let mut open_event_notes = scope.open_event_notes;
    let professions = owner_events(&events, |t| t == EventType::Occupation);
    let place_options = place_options(&places);
    let on_create = {
        let api = api.clone();
        move |_| draft.create(scope, api.clone(), show)
    };

    rsx! {
        div { class: "pf-subblock",
            div { class: "pf-block-label",
                {i18n.t("person_form.professions")}
                {scope.add_button(i18n.t("person_form.add_profession"), show, FormOpening::Profession)}
            }

            if show() {
                div { class: "pf-subform",
                    if let Some(err) = (draft.error)() {
                        div { class: "error-msg", "{err}" }
                    }
                    div { class: "form-group",
                        label { {i18n.t("person_form.profession")} }
                        ValueInput {
                            value: draft.label,
                            tree_id: tid,
                            field: SuggestionField::Occupations,
                        }
                    }
                    div { class: "form-group",
                        label { {i18n.t("person_form.date")} }
                        DateInput {
                            parts: draft.parts,
                            i18n,
                            on_change: move |()| {},
                        }
                    }
                    {render_place_input(&i18n, tid, draft.place_id, &place_options, || {})}
                    {render_notes_source_fields(&i18n, tid, draft.notes, draft.source, || {})}
                    button {
                        class: "pf-confirm-btn",
                        r#type: "button",
                        onclick: on_create,
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
                        let place = ev.place_id.map(|id| place_name(&place_options, id)).unwrap_or_default();
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
                                        onclick: scope.delete_event(api.clone(), eid),
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
                                    on_saved: move |_| { open_event_notes.set(None); scope.landed(); },
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

// ── Further information (names) ──────────────────────────────────────────

/// A stored name being edited in place.
#[derive(Clone, Copy)]
struct NameEdit {
    id: Signal<Option<Uuid>>,
    name_type: Signal<String>,
    given: Signal<String>,
    surname: Signal<String>,
    /// `None` trusts particle detection, `Some(p)` is the user's call
    /// (empty = "no particle here").
    particle_override: Signal<Option<String>>,
    prefix: Signal<String>,
    suffix: Signal<String>,
    nickname: Signal<String>,
    primary: Signal<bool>,
    error: Signal<Option<String>>,
}

impl NameEdit {
    fn use_new() -> Self {
        Self {
            id: use_signal(|| None::<Uuid>),
            name_type: use_signal(|| "Birth".to_string()),
            given: use_signal(String::new),
            surname: use_signal(String::new),
            particle_override: use_signal(|| None::<String>),
            prefix: use_signal(String::new),
            suffix: use_signal(String::new),
            nickname: use_signal(String::new),
            primary: use_signal(|| false),
            error: use_signal(|| None::<String>),
        }
    }

    /// Opens the editor on `name`.
    fn start(mut self, name: &PersonName) {
        // The picker value, not the Debug spelling: they diverge for the AKA
        // refinements ("Byname" vs "Surnom"), and feeding Debug back through
        // parse_name_type silently downgraded the entry to Other on save.
        self.name_type
            .set(name_type_value(name.name_type).to_string());
        self.given.set(name.given_names.clone().unwrap_or_default());
        // Edited as one field, particle included — re-split on save.
        let surname = name.full_surname().unwrap_or_default();
        self.particle_override.set(override_for_stored(
            &surname,
            opt_str(name.surname_prefix.as_deref().unwrap_or_default()).as_deref(),
        ));
        self.surname.set(surname);
        self.prefix.set(name.prefix.clone().unwrap_or_default());
        self.suffix.set(name.suffix.clone().unwrap_or_default());
        self.nickname.set(name.nickname.clone().unwrap_or_default());
        self.primary.set(name.is_primary);
        self.error.set(None);
        self.id.set(Some(name.id));
    }

    fn close(mut self) {
        self.id.set(None);
        self.error.set(None);
    }

    /// Saves the edited name.
    fn save(self, scope: FormScope, api: ApiClient) {
        let FormScope { i18n, tid, pid, .. } = scope;
        let Some(nid) = (self.id)() else { return };
        let given = (self.given)().trim().to_string();
        let surname = (self.surname)().trim().to_string();
        let particle_override = (self.particle_override)();
        let prefix = (self.prefix)().trim().to_string();
        let suffix = (self.suffix)().trim().to_string();
        let nickname = (self.nickname)().trim().to_string();
        let name_type = parse_name_type(&(self.name_type)());
        let is_primary = (self.primary)();
        let mut error = self.error;
        spawn(async move {
            // The surname is entered whole, particle included — detected
            // here, and overridable, exactly as the main name is.
            let split = resolve_particle(&surname, particle_override.as_deref());
            let body = UpdatePersonNameBody {
                name_type: Some(name_type),
                given_names: Some(opt_str(&given)),
                surname: Some(opt_str(&split.root)),
                surname_prefix: Some(split.particle),
                prefix: Some(opt_str(&prefix)),
                suffix: Some(opt_str(&suffix)),
                nickname: Some(opt_str(&nickname)),
                is_primary: Some(is_primary),
                sort_order: None,
            };
            match api.update_person_name(tid, pid, nid, &body).await {
                Ok(_) => {
                    self.close();
                    scope.landed();
                }
                Err(e) => error.set(Some(e.message(&i18n))),
            }
        });
    }
}

/// The further information under Civil Status: the person's names other
/// than the birth name, and the form adding one.
///
/// "Type d'information" reuses the existing NameType variants — Alias,
/// Surnom and Sobriquet are UI-only vocabulary for AlsoKnownAs (GEDCOM AKA),
/// not new name types (see parse_name_type). Each entry is just a single
/// value: a name-like string for every type except Sobriquet, which fills
/// the nickname piece instead.
#[component]
pub(super) fn InformationBlock(
    scope: FormScope,
    names: Resource<Result<Vec<PersonName>, ApiError>>,
    /// The birth name, edited under Civil Status and left out of the list.
    birth_name_id: Option<Uuid>,
    /// The birth surname and its particle override, which a new entry's
    /// surname starts from.
    birth_surname: Signal<String>,
    birth_particle_override: Signal<Option<String>>,
    /// Open the add form on mount: what the user asked for while creating.
    open: bool,
) -> Element {
    let api = use_context::<ApiClient>();
    let FormScope { i18n, tid, pid, .. } = scope;
    let mut show = use_signal(|| open);
    let mut form_type = use_signal(|| "Married".to_string());
    let mut form_value = use_signal(String::new);
    let mut form_particle_override = use_signal(|| None::<String>);
    let mut form_error = use_signal(|| None::<String>);
    let edit = NameEdit::use_new();

    let on_create = {
        let api = api.clone();
        move |_| {
            let api = api.clone();
            let value = form_value().trim().to_string();
            let info_type = form_type();
            let birth_surname = birth_surname().trim().to_string();
            let particle_override = form_particle_override();
            let birth_override = birth_particle_override();
            spawn(async move {
                if value.is_empty() {
                    form_error.set(Some(i18n.t("person_form.information_value_required")));
                    return;
                }
                let body = build_information_body(
                    &info_type,
                    &value,
                    &birth_surname,
                    particle_override.as_deref(),
                    birth_override.as_deref(),
                );
                match api.create_person_name(tid, pid, &body).await {
                    Ok(_) => {
                        show.set(false);
                        form_value.set(String::new());
                        form_particle_override.set(None);
                        form_type.set("Married".to_string());
                        form_error.set(None);
                        scope.landed();
                    }
                    Err(e) => form_error.set(Some(e.message(&i18n))),
                }
            });
        }
    };

    let list = match &*names.read() {
        Some(Ok(names)) => {
            let additional: Vec<PersonName> = names
                .iter()
                .filter(|n| Some(n.id) != birth_name_id)
                .cloned()
                .collect();
            rsx! {
                if additional.is_empty() {
                    div { class: "pf-empty-item", p { {i18n.t("person_form.no_information")} } }
                }
                for name in additional {
                    if (edit.id)() == Some(name.id) {
                        {name_editor(scope, api.clone(), edit)}
                    } else {
                        {name_row(scope, api.clone(), edit, name)}
                    }
                }
            }
        }
        Some(Err(e)) => rsx! {
            div { class: "error-msg", {i18n.t_args("person_form.load_names_error", &[("error", &e.message(&i18n))])} }
        },
        None => rsx! {
            div { class: "loading", {i18n.t("person_form.loading_names")} }
        },
    };

    rsx! {
        div { class: "pf-subblock",
            div { class: "pf-block-label",
                {i18n.t("person_form.tab_more_information")}
                {scope.add_button(i18n.t("person_form.add_information"), show, FormOpening::Information)}
            }

            if show() {
                {render_information_form(
                    &i18n,
                    tid,
                    form_error,
                    form_type,
                    form_value,
                    form_particle_override,
                    on_create,
                )}
            }

            {list}
        }
    }
}

/// A stored name, collapsed: its type, its pieces and its actions.
fn name_row(scope: FormScope, api: ApiClient, edit: NameEdit, name: PersonName) -> Element {
    let FormScope { i18n, tid, pid, .. } = scope;
    let mut save_error = scope.save_error;
    let nid = name.id;
    let type_label = i18n.t(name_type_label_key(name.name_type));
    // The collapsed row has to show whichever piece the entry actually
    // fills: an information may be a prefix or a suffix alone, and rendering
    // only given names and surname left such a row blank.
    let surname = name.full_surname().unwrap_or_default();
    let display = [
        name.prefix.as_deref().unwrap_or_default(),
        name.given_names.as_deref().unwrap_or_default(),
        surname.as_str(),
        name.suffix.as_deref().unwrap_or_default(),
    ]
    .into_iter()
    .filter(|piece| !piece.is_empty())
    .collect::<Vec<_>>()
    .join(" ");
    let nickname = name.nickname.clone().unwrap_or_default();
    let primary = name.is_primary;
    rsx! {
        div { class: "person-form-item pf-compact-item",
            div { class: "person-form-item-info",
                span { class: "badge", "{type_label}" }
                if !display.is_empty() {
                    strong { "{display}" }
                }
                if !nickname.is_empty() {
                    span { class: "text-muted", "\u{201C}{nickname}\u{201D}" }
                }
                if primary {
                    span { class: "badge badge-primary", {i18n.t("person_form.primary")} }
                }
            }
            div { class: "person-form-item-actions",
                button {
                    class: "pf-row-btn",
                    r#type: "button",
                    onclick: move |_| edit.start(&name),
                    {i18n.t("common.edit")}
                }
                button {
                    class: "pf-row-btn is-danger",
                    r#type: "button",
                    onclick: move |_| {
                        let api = api.clone();
                        spawn(async move {
                            match api.delete_person_name(tid, pid, nid).await {
                                Ok(_) => scope.landed(),
                                Err(e) => save_error.set(Some(e.message(&i18n))),
                            }
                        });
                    },
                    {i18n.t("common.delete")}
                }
            }
        }
    }
}

/// The editor of a stored name, in place of its row.
fn name_editor(scope: FormScope, api: ApiClient, edit: NameEdit) -> Element {
    let FormScope { i18n, tid, .. } = scope;
    let NameEdit {
        mut name_type,
        given,
        surname,
        particle_override,
        mut prefix,
        mut suffix,
        mut nickname,
        mut primary,
        error,
        ..
    } = edit;
    rsx! {
        div { class: "person-form-item editing",
            if let Some(err) = error() {
                div { class: "error-msg", "{err}" }
            }
            div { class: "form-row",
                div { class: "form-group",
                    label { {i18n.t("person_form.name_type")} }
                    select {
                        value: "{name_type}",
                        oninput: move |e: Event<FormData>| name_type.set(e.value()),
                        for nt in NAME_TYPES.iter().copied() {
                            option { value: name_type_value(nt), {i18n.t(name_type_label_key(nt))} }
                        }
                    }
                }
                div { class: "form-group",
                    label { {i18n.t("person_form.primary")} }
                    select {
                        value: if primary() { "true" } else { "false" },
                        oninput: move |e: Event<FormData>| primary.set(e.value() == "true"),
                        option { value: "true", {i18n.t("common.yes")} }
                        option { value: "false", {i18n.t("common.no")} }
                    }
                }
            }
            div { class: "form-row",
                div { class: "form-group",
                    label { {i18n.t("person_form.given_names")} }
                    ValueInput { value: given, tree_id: tid, field: SuggestionField::GivenNames }
                }
                div { class: "form-group",
                    label { {i18n.t("person_form.surname")} }
                    ValueInput { value: surname, tree_id: tid, field: SuggestionField::FamilyNames, surname: true }
                    {render_particle_row(&i18n, &surname(), particle_override)}
                }
            }
            div { class: "form-row",
                div { class: "form-group",
                    label { {i18n.t("person_form.prefix")} }
                    input { r#type: "text", value: "{prefix}", oninput: move |e: Event<FormData>| prefix.set(e.value()) }
                }
                div { class: "form-group",
                    label { {i18n.t("person_form.suffix")} }
                    input { r#type: "text", value: "{suffix}", oninput: move |e: Event<FormData>| suffix.set(e.value()) }
                }
                div { class: "form-group",
                    label { {i18n.t("person_form.nickname")} }
                    input { r#type: "text", value: "{nickname}", oninput: move |e: Event<FormData>| nickname.set(e.value()) }
                }
            }
            div { class: "inline-row",
                button {
                    class: "pf-confirm-btn",
                    r#type: "button",
                    onclick: move |_| edit.save(scope, api.clone()),
                    {i18n.t("common.save")}
                }
                button {
                    class: "pf-row-btn",
                    r#type: "button",
                    onclick: move |_| edit.close(),
                    {i18n.t("common.cancel")}
                }
            }
        }
    }
}

// ── Notes ────────────────────────────────────────────────────────────────

/// The person's own notes under Civil Status — the ones tied to a single
/// event live in that event's own panel — then `children`.
#[component]
pub(super) fn PersonNotesBlock(
    scope: FormScope,
    notes: Resource<Result<Vec<CoreNote>, ApiError>>,
    /// Open the add form on mount: what the user asked for while creating.
    open: bool,
    children: Element,
) -> Element {
    let api = use_context::<ApiClient>();
    let FormScope { i18n, tid, pid, .. } = scope;
    let mut save_error = scope.save_error;
    let mut show = use_signal(|| open);
    let mut form_text = use_signal(String::new);
    let mut form_error = use_signal(|| None::<String>);
    let mut editing = use_signal(|| None::<Uuid>);
    let mut edit_text = use_signal(String::new);
    let mut edit_error = use_signal(|| None::<String>);

    // Person-scoped notes only: a note stamped with an event belongs to that
    // event's own panel, not to the person's list. (Older rows carry both
    // ids, which is why this filters rather than trusting the query.)
    let list: Vec<CoreNote> = match &*notes.read() {
        Some(Ok(notes)) => notes
            .iter()
            .filter(|n| n.event_id.is_none())
            .cloned()
            .collect(),
        _ => Vec::new(),
    };

    let on_create = {
        let api = api.clone();
        move |_| {
            let api = api.clone();
            let text = form_text().trim().to_string();
            spawn(async move {
                if text.is_empty() {
                    form_error.set(Some(i18n.t("person_form.note_required")));
                    return;
                }
                let body = CreateNoteBody {
                    media_id: None,
                    repository_id: None,
                    text,
                    person_id: Some(pid),
                    event_id: None,
                    family_id: None,
                    source_id: None,
                };
                match api.create_note(tid, &body).await {
                    Ok(_) => {
                        show.set(false);
                        form_text.set(String::new());
                        form_error.set(None);
                        scope.landed();
                    }
                    Err(e) => form_error.set(Some(e.message(&i18n))),
                }
            });
        }
    };

    rsx! {
        div { class: "pf-subblock",
            div { class: "pf-block-label",
                {i18n.t("person_form.notes")}
                {scope.add_button(i18n.t("person_form.add_note"), show, FormOpening::Note)}
            }

            if show() {
                div { class: "pf-subform",
                    if let Some(err) = form_error() {
                        div { class: "error-msg", "{err}" }
                    }
                    div { class: "form-group",
                        textarea {
                            rows: 4,
                            placeholder: "{i18n.t(\"person_form.note_placeholder\")}",
                            value: "{form_text}",
                            oninput: move |e: Event<FormData>| form_text.set(e.value()),
                        }
                    }
                    button {
                        class: "pf-confirm-btn",
                        r#type: "button",
                        onclick: on_create,
                        {i18n.t("person.create_note")}
                    }
                }
            }

            if list.is_empty() {
                div { class: "pf-empty-item", p { {i18n.t("person_form.no_notes")} } }
            } else {
                for note in list.iter() {
                    {
                        let nid = note.id;
                        let text = note.text.clone();
                        let preview = crate::utils::html_to_preview(&note.text, 120);
                        if editing() == Some(nid) {
                            rsx! {
                                div { class: "person-form-item editing",
                                    if let Some(err) = edit_error() {
                                        div { class: "error-msg", "{err}" }
                                    }
                                    div { class: "form-group",
                                        textarea {
                                            rows: 4,
                                            value: "{edit_text}",
                                            oninput: move |e: Event<FormData>| edit_text.set(e.value()),
                                        }
                                    }
                                    div { class: "inline-row",
                                        button {
                                            class: "pf-confirm-btn",
                                            r#type: "button",
                                            onclick: {
                                                let api = api.clone();
                                                move |_| {
                                                    let api = api.clone();
                                                    let text = edit_text().trim().to_string();
                                                    spawn(async move {
                                                        if text.is_empty() {
                                                            edit_error.set(Some(i18n.t("person_form.note_required")));
                                                            return;
                                                        }
                                                        match api.update_note(tid, nid, &UpdateNoteBody { text: Some(text) }).await {
                                                            Ok(_) => {
                                                                editing.set(None);
                                                                edit_error.set(None);
                                                                scope.landed();
                                                            }
                                                            Err(e) => edit_error.set(Some(e.message(&i18n))),
                                                        }
                                                    });
                                                }
                                            },
                                            {i18n.t("common.save")}
                                        }
                                        button {
                                            class: "pf-row-btn",
                                            r#type: "button",
                                            onclick: move |_| { editing.set(None); edit_error.set(None); },
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
                                                edit_text.set(text.clone());
                                                editing.set(Some(nid));
                                                edit_error.set(None);
                                            },
                                            {i18n.t("common.edit")}
                                        }
                                        button {
                                            class: "pf-row-btn is-danger",
                                            r#type: "button",
                                            onclick: {
                                                let api = api.clone();
                                                move |_| {
                                                    let api = api.clone();
                                                    spawn(async move {
                                                        match api.delete_note(tid, nid).await {
                                                            Ok(_) => scope.landed(),
                                                            Err(e) => save_error.set(Some(e.message(&i18n))),
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

            {children}
        }
    }
}

// ── Other events ─────────────────────────────────────────────────────────

/// The fields of an event being added.
#[derive(Clone, Copy)]
struct EventDraft {
    event_type: Signal<EventType>,
    parts: Signal<DateParts>,
    place_id: Signal<String>,
    description: Signal<String>,
    cause: Signal<String>,
    age: Signal<String>,
    agency: Signal<String>,
    more: Signal<bool>,
    notes: Signal<String>,
    source: Signal<String>,
    error: Signal<Option<String>>,
}

impl EventDraft {
    fn use_new() -> Self {
        Self {
            event_type: use_signal(|| EventType::Baptism),
            parts: use_signal(DateParts::default),
            place_id: use_signal(String::new),
            description: use_signal(String::new),
            cause: use_signal(String::new),
            age: use_signal(String::new),
            agency: use_signal(String::new),
            more: use_signal(|| false),
            notes: use_signal(String::new),
            source: use_signal(String::new),
            error: use_signal(|| None::<String>),
        }
    }

    fn clear(mut self) {
        self.event_type.set(EventType::Baptism);
        self.parts.set(DateParts::default());
        self.place_id.set(String::new());
        self.description.set(String::new());
        self.cause.set(String::new());
        self.age.set(String::new());
        self.agency.set(String::new());
        self.more.set(false);
        self.notes.set(String::new());
        self.source.set(String::new());
        self.error.set(None);
    }

    /// Creates the event, with its notes and source.
    fn create(self, scope: FormScope, api: ApiClient, mut show: Signal<bool>) {
        let FormScope { i18n, tid, pid, .. } = scope;
        let mut error = self.error;
        let event_type = (self.event_type)();
        let parts = (self.parts)();
        let place_str = (self.place_id)();
        let desc = (self.description)().trim().to_string();
        let extras = EventExtras::from_form(&(self.cause)(), &(self.age)(), &(self.agency)());
        let notes = (self.notes)().trim().to_string();
        let source = (self.source)();
        spawn(async move {
            if let Some(key) = parts.validate() {
                error.set(Some(i18n.t(key)));
                return;
            }
            let extras = match extras {
                Ok(extras) => extras,
                Err(key) => {
                    error.set(Some(i18n.t(key)));
                    return;
                }
            };
            let place_id = match resolve_place(&api, tid, &place_str, i18n.0.reference_code()).await
            {
                Ok(place_id) => place_id,
                Err(e) => {
                    error.set(Some(e.message(&i18n)));
                    return;
                }
            };
            let body = create_event_body(
                event_type,
                &parts,
                place_id,
                EventOwner::Person(pid),
                opt_str(&desc),
                extras,
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
                    show.set(false);
                    self.clear();
                    scope.landed();
                }
                Err(e) => error.set(Some(e.message(&i18n))),
            }
        });
    }
}

/// The Other Events section: every event of the person but its birth,
/// death and professions, and the form adding one.
#[component]
pub(super) fn OtherEventsSection(
    scope: FormScope,
    events: Resource<Result<Vec<CoreEvent>, ApiError>>,
    places: Resource<Result<Vec<Place>, ApiError>>,
    /// Open the add form on mount: what the user asked for while creating.
    open: bool,
) -> Element {
    let api = use_context::<ApiClient>();
    let FormScope { i18n, tid, pid, .. } = scope;
    let mut open_event_notes = scope.open_event_notes;
    let section_open = use_signal(|| true);
    let show = use_signal(|| open);
    let draft = EventDraft::use_new();
    let EventDraft {
        mut event_type,
        mut description,
        ..
    } = draft;
    let other_events = owner_events(&events, |t| {
        !matches!(
            t,
            EventType::Birth | EventType::Death | EventType::Occupation
        )
    });
    let place_options = place_options(&places);
    let on_create = {
        let api = api.clone();
        move |_| draft.create(scope, api.clone(), show)
    };

    rsx! {
        FormSection {
            title: i18n.t("person_form.other_events"),
            open: section_open,
            action: scope.add_button(i18n.t("person_form.add_event"), show, FormOpening::Event),
            if show() {
                div { class: "pf-subform",
                    if let Some(err) = (draft.error)() {
                        div { class: "error-msg", "{err}" }
                    }
                    div { class: "form-row",
                        div { class: "form-group",
                            label { {i18n.t("person_form.type")} }
                            select {
                                value: event_type_value(event_type()),
                                oninput: move |e: Event<FormData>| event_type.set(parse_event_type(&e.value())),
                                {event_type_options(&i18n)}
                            }
                        }
                    }
                    // The event's own value — a profession, a title, a
                    // residence. Carried by GEDCOM on the tag itself, so it
                    // round-trips as such.
                    div { class: "form-group",
                        label { {i18n.t("person_form.description")} }
                        if event_type() == EventType::Occupation {
                            ValueInput {
                                value: description,
                                tree_id: tid,
                                field: SuggestionField::Occupations,
                            }
                        } else {
                            input {
                                r#type: "text",
                                value: "{description}",
                                oninput: move |e: Event<FormData>| description.set(e.value()),
                            }
                        }
                    }
                    div { class: "form-group",
                        label { {i18n.t("person_form.date")} }
                        DateInput {
                            parts: draft.parts,
                            i18n,
                            on_change: move |()| {},
                        }
                    }
                    div { class: "form-row",
                        {render_place_input(&i18n, tid, draft.place_id, &place_options, || {})}
                        {render_cause_age_fields(&i18n, draft.cause, Some(draft.age))}
                    }
                    {render_notes_source_fields(&i18n, tid, draft.notes, draft.source, || {})}
                    MoreDetails { open: draft.more,
                        {render_agency_field(&i18n, draft.agency)}
                    }
                    button {
                        class: "pf-confirm-btn",
                        r#type: "button",
                        onclick: on_create,
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
                        let place = ev.place_id.map(|id| place_name(&place_options, id)).unwrap_or_default();
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
                                        onclick: scope.delete_event(api.clone(), eid),
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
                                    on_saved: move |_| { open_event_notes.set(None); scope.landed(); },
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_place_resolves_to_its_name_or_to_nothing() {
        let known = Uuid::from_u128(1);
        let options = vec![(known.to_string(), "Placeville".to_string())];
        assert_eq!(place_name(&options, known), "Placeville");
        assert_eq!(place_name(&options, Uuid::from_u128(2)), "");
    }
}
