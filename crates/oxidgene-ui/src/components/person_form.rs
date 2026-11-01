//! Modal-based person edit form — single scrollable body with section dividers.
//!
//! Sections: Civil Status · Birth · Death · Privacy · Other Events · Notes.
//! A single footer Save button persists sex + privacy + birth event + death event
//! and closes the modal. Name, event, and note CRUD use inline per-item saves.
//!
//! The form shows the same sections whether it creates a person or edits
//! one. A person being created has no id yet for an occupation, a piece of
//! information, a note, an event or a medium to hang off, so asking to add
//! one creates the person first, from what the form already holds, and goes
//! on as the edit form of that person with the requested part open
//! ([`PersonForm`] swaps one for the other).
//!
//! Every date in the form — birth, death, professions, other events — is edited
//! through the one [`DateInput`] widget, so calendar and qualifier sit with the
//! date they qualify rather than in a panel of their own. Witnesses likewise
//! live in the event's own block.
//!
//! The sections that save on their own — professions, further information,
//! notes, other events — and the birth and death sections are in the
//! `sections` submodule; this module keeps the form itself, its footer Save, and the
//! building blocks the couple form shares.

mod sections;
mod state;

use std::collections::HashMap;

use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::{
    AddChildBody, AddSpouseBody, ApiClient, ApiError, CreateCitationBody, CreateEventBody,
    CreateNoteBody, CreatePersonBody, CreatePersonNameBody, CreateSourceBody, SuggestionField,
    UpdateCitationBody, UpdateEventBody, UpdateNoteBody, UpdatePersonBody, UpdatePersonNameBody,
};
use crate::components::date_input::{DateInput, DateParts, format_event_date};
use crate::components::homonym_picker::{HomonymDecision, HomonymDialog};
use crate::components::media_gallery::{MediaGallery, MediaOwner};
use crate::components::modal::Modal;
use crate::components::parent_suggestions::ParentSuggestions;
use crate::components::place_input::{render_place_input, resolve_place};
use crate::components::suggest_input::ValueInput;
use crate::i18n::use_i18n;
use crate::permissions::{IfCan, Permission};
use crate::ui_observability::use_ui_resource;
use crate::utils::{
    event_type_label_key, event_type_value, name_type_label_key, name_type_value, opt_str,
    parse_name_type, resolve_name,
};
use oxidgene_core::types::Event as CoreEvent;
use oxidgene_core::types::SpouseAge;
use oxidgene_core::types::{split_surname_at_head, split_surname_particle};
use oxidgene_core::{ChildType, Confidence, EventType, NameType, Privacy, Sex, SpouseRole};

use sections::{
    FormScope, InformationBlock, OtherEventsSection, PersonNotesBlock, ProfessionsBlock,
    vital_section,
};
use state::{FormLoads, HeldFields, use_form_data, use_seeded_fields};

// ── Props ────────────────────────────────────────────────────────────────

/// Context that determines which relationship is wired on create-mode save.
#[derive(Debug, Clone, PartialEq)]
pub enum PersonFormCreateContext {
    Standalone,
    AddParent {
        child_id: Uuid,
        family_id: Option<Uuid>,
        is_father: bool,
        child_surname: Option<String>,
    },
}

/// Where a parent created or picked from the form goes: the child, the
/// family of the child's parents if there is one, and which parent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ParentLink {
    pub child_id: Uuid,
    pub family_id: Option<Uuid>,
    pub is_father: bool,
}

impl ParentLink {
    /// The parent `context` adds, if it adds one.
    pub(crate) fn of(context: &PersonFormCreateContext) -> Option<Self> {
        match *context {
            PersonFormCreateContext::AddParent {
                child_id,
                family_id,
                is_father,
                ..
            } => Some(Self {
                child_id,
                family_id,
                is_father,
            }),
            PersonFormCreateContext::Standalone => None,
        }
    }

    /// Make `person_id` the child's father or mother, founding the parents'
    /// family first when there is none. The error is the reader's.
    pub(crate) async fn link(
        self,
        api: &ApiClient,
        tid: Uuid,
        person_id: Uuid,
        i18n: &crate::i18n::I18n,
    ) -> Result<(), String> {
        let fid = match self.family_id {
            Some(fid) => fid,
            None => {
                let Ok(family) = api.create_family(tid).await else {
                    return Err(i18n.t("person_form.create_failed"));
                };
                let child_body = AddChildBody {
                    person_id: self.child_id,
                    child_type: ChildType::Biological,
                    sort_order: 0,
                };
                if api.add_child(tid, family.id, &child_body).await.is_err() {
                    // The family would stay empty, and the new parent the
                    // spouse of no child's parents: drop it, best effort.
                    let _ = api.delete_family(tid, family.id).await;
                    return Err(i18n.t("person_form.create_failed"));
                }
                family.id
            }
        };
        let spouse_body = AddSpouseBody {
            person_id,
            role: if self.is_father {
                SpouseRole::Husband
            } else {
                SpouseRole::Wife
            },
            sort_order: 0,
        };
        api.add_spouse(tid, fid, &spouse_body)
            .await
            .map(drop)
            .map_err(|e| e.message(i18n))
    }
}

impl PersonFormCreateContext {
    fn initial_surname(&self) -> Option<String> {
        match self {
            Self::AddParent {
                is_father: true,
                child_surname: Some(child_surname),
                ..
            } => {
                let child_surname = child_surname.trim();
                (!child_surname.is_empty()).then(|| child_surname.to_string())
            }
            _ => None,
        }
    }
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
    /// Called with the kept person's ID when the saved person was merged into
    /// a homonym — the record this form edited no longer exists, so a page
    /// showing it has somewhere else to go. The form closes either way.
    #[props(default)]
    pub on_merged: Option<EventHandler<Uuid>>,
}

/// A saved person whose name others of the tree already bear, waiting for the
/// user to say whether they are one of them.
#[derive(Clone, PartialEq)]
struct HomonymCheck {
    person_id: Uuid,
    name: String,
    homonyms: Vec<oxidgene_core::projection::SearchEntry>,
}

/// What the user asked to add to a person the create form had not created
/// yet. The form creates the person, then opens this part of the edit form.
#[derive(Debug, Clone, Copy, PartialEq)]
enum FormOpening {
    Profession,
    Information,
    Note,
    Event,
    Media,
}

// ── Component ────────────────────────────────────────────────────────────

/// The person form. In create mode it holds the form until the person
/// exists — on Create, or as soon as the user adds something that needs the
/// person's id — then the edit form of the created person takes its place.
#[component]
pub fn PersonForm(props: PersonFormProps) -> Element {
    let mut created = use_signal(|| None::<(Uuid, FormOpening)>);
    // The edit form of the created person is a new body, with its own state
    // and its own loads. Dioxus compares keys between siblings only: the
    // body is therefore a child, keyed, not the root this component
    // returns, whose key is never read. As the root, both arms render the
    // same template — one component — and a release build, which merges
    // identical templates, kept the create body with the edit props.
    rsx! {
        {
            match created() {
                Some((person_id, opening)) => rsx! {
                    PersonFormBody {
                        key: "{person_id}",
                        form: PersonFormProps {
                            person_id: Some(person_id),
                            create_context: None,
                            ..props
                        },
                        opening,
                    }
                },
                None => rsx! {
                    PersonFormBody {
                        // No person yet: the nil id, which none has.
                        key: "{Uuid::nil()}",
                        form: props,
                        on_created: move |created_as: (Uuid, FormOpening)| created.set(Some(created_as)),
                    }
                },
            }
        }
    }
}

/// Which of the form's own sections are open. Every section opens with the
/// form; folding one is a way to get it out of the way, not a step to go
/// through before the fields are reachable.
#[derive(Clone, Copy)]
struct SectionFolds {
    civil: Signal<bool>,
    birth: Signal<bool>,
    death: Signal<bool>,
    privacy: Signal<bool>,
    media: Signal<bool>,
}

impl SectionFolds {
    fn use_new() -> Self {
        Self {
            civil: use_signal(|| true),
            birth: use_signal(|| true),
            death: use_signal(|| true),
            privacy: use_signal(|| true),
            media: use_signal(|| true),
        }
    }
}

/// Where the form says how its footer Save went.
#[derive(Clone, Copy)]
struct SaveState {
    saving: Signal<bool>,
    error: Signal<Option<String>>,
    /// A saved person whose name others of the tree bear, waiting for the
    /// user's answer before the form closes.
    homonym_check: Signal<Option<HomonymCheck>>,
}

#[component]
fn PersonFormBody(
    form: PersonFormProps,
    /// The part to open on mount: what the user asked to add while creating.
    opening: Option<FormOpening>,
    /// Create mode: the person was created to receive `FormOpening`.
    on_created: Option<EventHandler<(Uuid, FormOpening)>>,
) -> Element {
    let props = form;
    let api = use_context::<ApiClient>();
    let i18n = use_i18n();
    let refresh = use_signal(|| 0u32);
    let is_create = props.create_context.is_some();
    let (tid, pid) = (props.tree_id, props.person_id.unwrap_or_default());

    let fields = HeldFields::use_new(props.create_context.as_ref());
    let data = use_form_data(&api, tid, pid, is_create, refresh);
    use_seeded_fields(data, fields, is_create);

    let save = SaveState {
        saving: use_signal(|| false),
        error: use_signal(|| None::<String>),
        homonym_check: use_signal(|| None::<HomonymCheck>),
    };
    let folds = SectionFolds::use_new();
    let has_changes = use_signal(|| false);
    let mut show_discard_confirm = use_signal(|| false);

    // Create mode: what the user adds needs the person's id, so the person
    // is created first, from what the form holds, then the edit form opens
    // with that part (see `PersonForm`).
    let promote = use_callback({
        let (api, context, on_saved) = (api.clone(), props.create_context.clone(), props.on_saved);
        move |opening: FormOpening| {
            let (Some(context), Some(on_created)) = (context.clone(), on_created) else {
                return;
            };
            let new = fields.held();
            spawn(promote_person(
                api.clone(),
                i18n,
                tid,
                context,
                new,
                save,
                move |person_id| {
                    on_saved.call(());
                    on_created.call((person_id, opening));
                },
            ));
        }
    });

    let scope = FormScope {
        i18n,
        tid,
        pid,
        is_create,
        refresh,
        save_error: save.error,
        on_saved: props.on_saved,
        promote,
        open_event_notes: use_signal(|| None::<Uuid>),
    };

    let mut try_close = move |()| {
        if has_changes() {
            show_discard_confirm.set(true);
        } else {
            props.on_close.call(());
        }
    };

    let display_name = display_name(&i18n, is_create, &data.names);
    let body = form_body(&props, scope, fields, data, folds, opening, has_changes);
    let on_save = footer_save(api.clone(), &props, scope, fields, save);
    let footer = render_footer(scope, save, props.embedded, try_close, on_save);
    let homonym_dialog = render_homonym_dialog(&props, save.homonym_check);

    if props.embedded {
        return rsx! {
            div { class: "pf-embedded", {body} {footer} }
            {homonym_dialog}
        };
    }

    rsx! {
        Modal {
            class: "person-form-modal",
            label: display_name.clone(),
            on_close: try_close,
            on_key: move |e: KeyboardEvent| {
                if e.key() == Key::Enter {
                    document::eval(&focus_next_field_js("person-form-modal"));
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
                button { class: "person-form-close", onclick: move |_| try_close(()), "\u{00D7}" }
            }

            {body}
            {footer}
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

        {homonym_dialog}
    }
}

/// The name the form's title shows.
fn display_name(
    i18n: &crate::i18n::I18n,
    is_create: bool,
    names: &Resource<Result<Vec<oxidgene_core::types::PersonName>, ApiError>>,
) -> String {
    if is_create {
        return i18n.t("person_form.new_person");
    }
    match &*names.read() {
        Some(Ok(names)) => oxidgene_core::types::PersonName::primary(names)
            .map(oxidgene_core::types::PersonName::display_name)
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| i18n.t("common.unnamed")),
        _ => i18n.t("common.loading"),
    }
}

/// Every event of the person, as `(id, label)` pairs — what the media
/// gallery offers when asking which event a file or a crop documents.
fn event_choices(
    i18n: &crate::i18n::I18n,
    events: &Resource<Result<Vec<CoreEvent>, ApiError>>,
) -> Vec<(Uuid, String)> {
    let Some(Ok(events)) = &*events.read() else {
        return Vec::new();
    };
    events
        .iter()
        .map(|ev| {
            let kind = i18n.t(event_type_label_key(ev.event_type));
            let date = format_event_date(i18n, ev);
            let label = if date.is_empty() {
                kind
            } else {
                format!("{kind} — {date}")
            };
            (ev.id, label)
        })
        .collect()
}

/// Creates the person the create form holds, then hands its id to `created`.
async fn promote_person(
    api: ApiClient,
    i18n: crate::i18n::I18n,
    tid: Uuid,
    context: PersonFormCreateContext,
    new: NewPerson,
    save: SaveState,
    created: impl FnOnce(Uuid),
) {
    let SaveState {
        mut saving,
        mut error,
        ..
    } = save;
    if let Some(key) = new.invalid() {
        error.set(Some(i18n.t(key)));
        return;
    }
    saving.set(true);
    error.set(None);
    match create_person(&api, tid, &i18n, &context, &new).await {
        Ok(person_id) => created(person_id),
        Err(message) => {
            error.set(Some(message));
            saving.set(false);
        }
    }
}

/// The footer button: creates the person, or saves what the edit form
/// holds; then asks about the person's homonyms, if any, or closes.
fn footer_save(
    api: ApiClient,
    props: &PersonFormProps,
    scope: FormScope,
    fields: HeldFields,
    save: SaveState,
) -> impl FnMut(Event<MouseData>) + 'static {
    let (on_saved, on_close) = (props.on_saved, props.on_close);
    let context = props.create_context.clone();
    move |_| {
        let api = api.clone();
        let context = context.clone();
        let new = fields.held();
        let privacy = (fields.privacy)();
        let SaveState {
            mut saving,
            mut error,
            mut homonym_check,
        } = save;
        let FormScope { i18n, tid, pid, .. } = scope;
        spawn(async move {
            if let Some(key) = new.invalid() {
                error.set(Some(i18n.t(key)));
                return;
            }
            saving.set(true);
            error.set(None);
            let saved = match &context {
                Some(context) => create_person(&api, tid, &i18n, context, &new).await,
                None => save_person_edits(&api, tid, pid, &i18n, &new, privacy, fields)
                    .await
                    .map(|()| pid),
            };
            saving.set(false);
            let saved_pid = match saved {
                Ok(saved_pid) => saved_pid,
                Err(message) => {
                    error.set(Some(message));
                    return;
                }
            };
            // A name somebody else in the tree already bears raises the
            // question before the form closes. A failed lookup is not a
            // failed save, so it closes as it always did.
            if let Ok(homonyms) = api.person_homonyms(tid, saved_pid).await
                && !homonyms.is_empty()
            {
                homonym_check.set(Some(HomonymCheck {
                    person_id: saved_pid,
                    name: format!("{} {}", new.given, new.surname),
                    homonyms,
                }));
                return;
            }
            on_saved.call(());
            on_close.call(());
        });
    }
}

/// The form's scrollable body: every section, then the deletion.
fn form_body(
    props: &PersonFormProps,
    scope: FormScope,
    fields: HeldFields,
    data: FormLoads,
    folds: SectionFolds,
    opening: Option<FormOpening>,
    mut has_changes: Signal<bool>,
) -> Element {
    let i18n = scope.i18n;
    let place_options = sections::place_options(&data.places);
    // Adding a parent: the persons of the tree this one may already be.
    let parent_link = props.create_context.as_ref().and_then(ParentLink::of);
    let (on_saved, on_close) = (props.on_saved, props.on_close);
    rsx! {
        div { class: "person-form-body",
            if let Some(link) = parent_link {
                ParentSuggestions {
                    tree_id: scope.tid,
                    link,
                    surname: fields.surname,
                    given_names: fields.given,
                    on_linked: move |()| {
                        on_saved.call(());
                        on_close.call(());
                    },
                }
            }

            {civil_status_section(scope, fields, data, folds.civil, opening, has_changes)}
            {vital_section(scope, i18n.t("person_form.birth"), folds.birth, fields.birth, &place_options, has_changes)}
            {vital_section(scope, i18n.t("person_form.death"), folds.death, fields.death, &place_options, has_changes)}

            // ── Privacy ── to whom may change it (`privacy_edit`).
            IfCan { permission: Permission::PrivacyEdit,
                FormSection { title: i18n.t("person_form.privacy"), open: folds.privacy,
                    {render_choice_group(
                        &[
                            (Privacy::Default, i18n.t("privacy.default")),
                            (Privacy::Public, i18n.t("privacy.public")),
                            (Privacy::Private, i18n.t("privacy.private")),
                        ],
                        fields.privacy,
                        move |_| has_changes.set(true),
                    )}
                }
            }

            OtherEventsSection {
                scope,
                events: data.events,
                places: data.places,
                open: opening == Some(FormOpening::Event),
            }

            {media_section(scope, data, folds.media)}

            // ── Delete Person (edit mode only) ──
            // No section header: the button says what it does, and a
            // heading repeating it above would be the same word twice.
            if !scope.is_create && !props.embedded {
                PersonDeletion { scope, display_name: display_name(&i18n, false, &data.names), on_close }
            }
        }
    }
}

/// The Civil Status section: the birth name, the sex, the professions, the
/// further information and the notes, then the person's own source.
fn civil_status_section(
    scope: FormScope,
    fields: HeldFields,
    data: FormLoads,
    open: Signal<bool>,
    opening: Option<FormOpening>,
    mut has_changes: Signal<bool>,
) -> Element {
    let FormScope { i18n, tid, .. } = scope;
    rsx! {
        FormSection { title: i18n.t("person_form.tab_civil"), open,
            // Birth name + given names — mandatory, always visible.
            div { class: "form-row",
                div { class: "form-group",
                    label { {i18n.t("name_type.birth")} " *" }
                    ValueInput {
                        value: fields.surname,
                        tree_id: tid,
                        field: SuggestionField::FamilyNames,
                        placeholder: i18n.t("person_form.surname_placeholder"),
                        surname: true,
                        on_change: move |()| has_changes.set(true),
                    }
                    // Surface the split the save path will apply, and
                    // let the user correct it — detection is a guess.
                    {render_particle_row(&i18n, &(fields.surname)(), fields.particle_override)}
                }
                div { class: "form-group",
                    label { {i18n.t("person_form.given_names")} " *" }
                    ValueInput {
                        value: fields.given,
                        tree_id: tid,
                        field: SuggestionField::GivenNames,
                        placeholder: i18n.t("person_form.given_placeholder"),
                        on_change: move |()| has_changes.set(true),
                    }
                }
            }

            div { class: "form-group",
                label { {i18n.t("person_form.sex")} }
                {render_choice_group(
                    &[
                        (Sex::Male, i18n.t("sex.male")),
                        (Sex::Female, i18n.t("sex.female")),
                        (Sex::Unknown, i18n.t("sex.unknown")),
                    ],
                    fields.sex,
                    move |_| has_changes.set(true),
                )}
            }

            ProfessionsBlock {
                scope,
                events: data.events,
                places: data.places,
                open: opening == Some(FormOpening::Profession),
            }

            InformationBlock {
                scope,
                names: data.names,
                birth_name_id: (fields.birth_name_id)(),
                birth_surname: fields.surname,
                birth_particle_override: fields.particle_override,
                open: opening == Some(FormOpening::Information),
            }

            PersonNotesBlock {
                scope,
                notes: data.notes,
                open: opening == Some(FormOpening::Note),
                // Where the civil-status information itself
                // came from; saved with the footer button.
                div { class: "form-group pf-subblock",
                    label { {i18n.t("person_form.source")} }
                    ValueInput {
                        value: fields.source,
                        tree_id: tid,
                        field: SuggestionField::Sources,
                        placeholder: i18n.t("person_form.source_placeholder"),
                        on_change: move |()| has_changes.set(true),
                    }
                }
            }
        }
    }
}

/// The Media section. A person being created has no id for a medium to
/// hang off yet: its button creates the person, and the gallery of the edit
/// form takes over.
fn media_section(scope: FormScope, data: FormLoads, open: Signal<bool>) -> Element {
    let i18n = scope.i18n;
    if scope.is_create {
        return rsx! {
            FormSection {
                title: i18n.t("media.section"),
                open,
                action: render_add_creating(i18n.t("person_form.add_media"), scope.promote, FormOpening::Media),
                div { class: "pf-empty-item", p { {i18n.t("person_form.no_media")} } }
            }
        };
    }
    let on_saved = scope.on_saved;
    rsx! {
        FormSection {
            title: i18n.t("media.section"),
            open,
            MediaGallery {
                tree_id: scope.tid,
                owner: MediaOwner::Person(scope.pid),
                events: event_choices(&i18n, &data.events),
                // Choosing a portrait writes immediately and is not part of
                // this form's save, so a user who picks one and then closes
                // without saving must still see the card behind the modal
                // change. Every host treats `on_saved` as "refresh", never
                // as "close", so this is safe to call.
                on_changed: move |()| on_saved.call(()),
            }
        }
    }
}

/// The deletion of the person edited, behind its own confirmation.
#[component]
fn PersonDeletion(scope: FormScope, display_name: String, on_close: EventHandler<()>) -> Element {
    let api = use_context::<ApiClient>();
    let i18n = scope.i18n;
    let mut deleting = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let on_confirm = move |_| {
        let api = api.clone();
        spawn(async move {
            deleting.set(true);
            error.set(None);
            match api.delete_person(scope.tid, scope.pid).await {
                Ok(_) => {
                    scope.on_saved.call(());
                    on_close.call(());
                }
                Err(e) => {
                    error.set(Some(e.message(&i18n)));
                    deleting.set(false);
                }
            }
        });
    };
    rsx! {
        DeleteSection {
            button_label: i18n.t("person_form.delete_person"),
            title: format!("{} {}?", i18n.t("person_form.delete_confirm_title"), display_name),
            message: i18n.t("person_form.delete_confirm_message"),
            confirm_label: i18n.t("person_form.delete_confirm_button"),
            busy_label: i18n.t("person_form.deleting"),
            deleting: deleting(),
            error: error(),
            on_confirm,
        }
    }
}

/// The fixed footer: the Save error, Cancel, and Save or Create.
fn render_footer(
    scope: FormScope,
    save: SaveState,
    embedded: bool,
    mut try_close: impl FnMut(()) + 'static,
    on_save: impl FnMut(Event<MouseData>) + 'static,
) -> Element {
    let i18n = scope.i18n;
    let saving = (save.saving)();
    let label = if saving {
        i18n.t("common.saving")
    } else if scope.is_create {
        i18n.t("person_form.btn_create")
    } else {
        i18n.t("common.save")
    };
    rsx! {
        div { class: "pf-footer",
            if let Some(err) = (save.error)() {
                div { class: "error-msg", "{err}" }
            }
            div { class: "pf-footer-right",
                if !embedded {
                    button {
                        class: "btn btn-outline",
                        r#type: "button",
                        onclick: move |_| try_close(()),
                        {i18n.t("common.cancel")}
                    }
                }
                button {
                    class: "btn btn-primary",
                    r#type: "button",
                    disabled: saving,
                    onclick: on_save,
                    "{label}"
                }
            }
        }
    }
}

/// The homonym question a Save raised, if any.
fn render_homonym_dialog(
    props: &PersonFormProps,
    mut homonym_check: Signal<Option<HomonymCheck>>,
) -> Element {
    let Some(check) = homonym_check() else {
        return rsx! {};
    };
    let (on_saved, on_close, on_merged) = (props.on_saved, props.on_close, props.on_merged);
    rsx! {
        HomonymDialog {
            tree_id: props.tree_id,
            person_id: check.person_id,
            person_name: check.name,
            homonyms: check.homonyms,
            on_later: move |()| {
                homonym_check.set(None);
                on_saved.call(());
                on_close.call(());
            },
            on_decided: move |decision: HomonymDecision| {
                homonym_check.set(None);
                on_saved.call(());
                if let (HomonymDecision::Merged(kept), Some(on_merged)) = (decision, on_merged) {
                    on_merged.call(kept);
                }
                on_close.call(());
            },
        }
    }
}

// ── Shared option builders ────────────────────────────────────────────────

/// The event types the person form offers, by group, in picker order.
/// Birth, death and the union's events have fields of their own.
const EVENT_TYPE_GROUPS: [(&str, &[EventType]); 6] = [
    (
        "person_form.sacraments",
        &[
            EventType::Baptism,
            EventType::Confirmation,
            EventType::FirstCommunion,
            EventType::BarBatMitzvah,
            EventType::BasMitzvah,
            EventType::Burial,
            EventType::Cremation,
        ],
    ),
    (
        "person_form.civil",
        &[
            EventType::Census,
            EventType::Graduation,
            EventType::Immigration,
            EventType::Emigration,
            EventType::Naturalization,
            EventType::Occupation,
            EventType::Residence,
            EventType::Retirement,
            EventType::MilitaryService,
        ],
    ),
    (
        "person_form.geneweb",
        &[
            EventType::Blessing,
            EventType::Ordination,
            EventType::Christening,
            EventType::AdultChristening,
            EventType::Accomplishment,
            EventType::Acquisition,
            EventType::Membership,
            EventType::ChangeName,
            EventType::Circumcision,
            EventType::Award,
            EventType::MilitaryDischarge,
            EventType::Degree,
            EventType::Distinction,
            EventType::Election,
            EventType::Excommunication,
            EventType::Funeral,
            EventType::Hospitalization,
            EventType::Illness,
            EventType::PassengerList,
            EventType::MilitaryDistinction,
            EventType::MilitaryPromotion,
            EventType::MilitaryMobilization,
            EventType::PropertySale,
            EventType::Endowment,
            EventType::LdsDotation,
            EventType::SealingChild,
            EventType::SealingSpouse,
            EventType::SealingParent,
            EventType::FamilyLinkLds,
            EventType::NoMarriage,
            EventType::LdsBaptism,
            EventType::LdsConfirmation,
            EventType::LdsInitiatory,
            EventType::NoMention,
        ],
    ),
    ("person_form.legal", &[EventType::Will, EventType::Probate]),
    (
        "person_form.attributes",
        &[
            EventType::CasteName,
            EventType::PhysicalDescription,
            EventType::Education,
            EventType::NationalId,
            EventType::NationalOrigin,
            EventType::ChildrenCount,
            EventType::MarriagesCount,
            EventType::Property,
            EventType::Religion,
            EventType::SocialSecurityNumber,
            EventType::NobilityTitle,
            EventType::Fact,
        ],
    ),
    (
        "person_form.other_events",
        &[EventType::Adoption, EventType::Other],
    ),
];

fn event_type_options(i18n: &crate::i18n::I18n) -> Element {
    let i18n = *i18n;
    rsx! {
        for (group, types) in EVENT_TYPE_GROUPS {
            optgroup { label: i18n.t(group),
                for et in types.iter().copied() {
                    option { value: event_type_value(et), {i18n.t(event_type_label_key(et))} }
                }
            }
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

/// The add button of a part the person must exist for, in create mode: it
/// creates the person, then opens that part of the edit form.
fn render_add_creating(
    label: String,
    promote: Callback<FormOpening>,
    opening: FormOpening,
) -> Element {
    rsx! {
        button {
            class: "pf-add-btn",
            r#type: "button",
            onclick: move |_| promote.call(opening),
            "{label}"
        }
    }
}

/// The cause of an event and, when `age` is given — an event of a person —
/// the age the record gives: the two rarer fields kept in the open, beside
/// the place. A family event records each spouse's age instead.
pub(crate) fn render_cause_age_fields(
    i18n: &crate::i18n::I18n,
    mut cause: Signal<String>,
    age: Option<Signal<String>>,
) -> Element {
    rsx! {
        div { class: "form-group",
            label { {i18n.t("person_form.cause")} }
            input {
                r#type: "text",
                value: "{cause}",
                oninput: move |e: Event<FormData>| cause.set(e.value()),
            }
        }
        if let Some(mut age) = age {
            div { class: "form-group",
                label { {i18n.t("person_form.age")} }
                input {
                    r#type: "text",
                    value: "{age}",
                    placeholder: i18n.t("person_form.age_placeholder"),
                    oninput: move |e: Event<FormData>| age.set(e.value()),
                }
            }
        }
    }
}

/// The responsible agency of an event — one of the rare fields kept behind
/// [`MoreDetails`].
pub(crate) fn render_agency_field(i18n: &crate::i18n::I18n, mut agency: Signal<String>) -> Element {
    rsx! {
        div { class: "form-group",
            label { {i18n.t("person_form.agency")} }
            input {
                r#type: "text",
                value: "{agency}",
                oninput: move |e: Event<FormData>| agency.set(e.value()),
            }
        }
    }
}

/// The rare fields of a form, behind a "More details" link so that the
/// fields everybody fills stay few. Callers open it from the start when one
/// of those fields already holds something, so nothing stored is hidden.
#[component]
pub fn MoreDetails(open: Signal<bool>, children: Element) -> Element {
    let i18n = use_i18n();
    let mut open = open;
    rsx! {
        div { class: "pf-more",
            button {
                class: "pf-more-toggle",
                r#type: "button",
                "aria-expanded": open().to_string(),
                onclick: move |_| open.toggle(),
                span { class: if open() { "pf-chevron is-open" } else { "pf-chevron" } }
                if open() {
                    {i18n.t("person_form.fewer_details")}
                } else {
                    {i18n.t("person_form.more_details")}
                }
            }
            if open() {
                div { class: "pf-more-body", {children} }
            }
        }
    }
}

/// A row of mutually exclusive buttons bound to one signal (sex, privacy) —
/// a radio group that reads as a segmented control. `on_change` receives the
/// value the click replaced, so a write that fails can put it back.
pub fn render_choice_group<T: Copy + PartialEq + 'static>(
    options: &[(T, String)],
    mut selected: Signal<T>,
    on_change: impl FnMut(T) + Clone + 'static,
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
                            onclick: move |_| {
                                let previous = *selected.peek();
                                selected.set(value);
                                on_change(previous);
                            },
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

/// An event's rarer fields — its cause, the age the record gives and the
/// responsible agency — as a create writes them.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct EventExtras {
    pub cause: Option<String>,
    pub age: Option<String>,
    pub agency: Option<String>,
    /// For a family event, the age it gives for each spouse.
    pub spouse_ages: Vec<SpouseAge>,
}

/// [`EventExtras`] as an update writes them: `None` leaves a field alone and
/// `Some(None)` clears it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct EventExtrasPatch {
    pub cause: Option<Option<String>>,
    pub age: Option<Option<String>>,
    pub agency: Option<Option<String>>,
    /// Replaces a family event's spouse ages when set.
    pub spouse_ages: Option<Vec<SpouseAge>>,
}

impl EventExtras {
    /// What a form's cause, age and agency fields say, the age in the
    /// canonical form the server stores — or the translation key of the
    /// message saying the age is not one.
    pub(crate) fn from_form(cause: &str, age: &str, agency: &str) -> Result<Self, &'static str> {
        let age =
            oxidgene_core::types::age::normalize(age).map_err(|_| "person_form.age_invalid")?;
        Ok(Self {
            cause: opt_str(cause.trim()),
            age,
            agency: opt_str(agency.trim()),
            spouse_ages: Vec::new(),
        })
    }

    /// Every field written, a blank one cleared: what saving a form that
    /// shows all three means. Spouse ages are written when `spouses` names
    /// the spouses the form showed them for.
    pub(crate) fn into_patch(self, spouses: bool) -> EventExtrasPatch {
        EventExtrasPatch {
            cause: Some(self.cause),
            age: Some(self.age),
            agency: Some(self.agency),
            spouse_ages: spouses.then_some(self.spouse_ages),
        }
    }
}

/// The ages typed for each of `spouses` (person, name), in canonical form,
/// the blank ones left out — or the key of the message saying one is not an
/// age.
pub(crate) fn spouse_ages_from_form(
    spouses: &[(Uuid, String)],
    typed: &HashMap<Uuid, String>,
) -> Result<Vec<SpouseAge>, &'static str> {
    let mut ages = Vec::new();
    for (person_id, _) in spouses {
        let text = typed.get(person_id).map(String::as_str).unwrap_or_default();
        if let Some(age) =
            oxidgene_core::types::age::normalize(text).map_err(|_| "person_form.age_invalid")?
        {
            ages.push(SpouseAge {
                person_id: *person_id,
                age,
            });
        }
    }
    Ok(ages)
}

/// The ages a family event gives, by spouse, as a form edits them.
pub(crate) fn typed_spouse_ages(ages: &[SpouseAge]) -> HashMap<Uuid, String> {
    ages.iter().map(|a| (a.person_id, a.age.clone())).collect()
}

/// One "Age of {name}" field per spouse of a family event.
pub(crate) fn render_spouse_age_fields(
    i18n: &crate::i18n::I18n,
    spouses: &[(Uuid, String)],
    mut ages: Signal<HashMap<Uuid, String>>,
) -> Element {
    let spouses = spouses.to_vec();
    rsx! {
        for (person_id, name) in spouses {
            div { key: "{person_id}", class: "form-group",
                label { {i18n.t_args("person_form.age_of", &[("name", &name)])} }
                input {
                    r#type: "text",
                    value: ages.read().get(&person_id).cloned().unwrap_or_default(),
                    placeholder: i18n.t("person_form.age_placeholder"),
                    oninput: move |e: Event<FormData>| {
                        ages.write().insert(person_id, e.value());
                    },
                }
            }
        }
    }
}

pub(crate) fn create_event_body(
    event_type: EventType,
    parts: &DateParts,
    place_id: Option<Uuid>,
    owner: EventOwner,
    description: Option<String>,
    extras: EventExtras,
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
        cause: extras.cause,
        age: extras.age,
        agency: extras.agency,
        spouse_ages: extras.spouse_ages,
        place_id,
        person_id,
        family_id,
        description,
    }
}

/// `description` and each of the `extras`: `None` leaves the stored value
/// alone; `Some(value)` writes it (with `Some(None)` clearing it), matching
/// the DTO's own contract.
pub(crate) fn update_event_body(
    event_type: Option<EventType>,
    parts: &DateParts,
    place_id: Option<Uuid>,
    description: Option<Option<String>>,
    extras: EventExtrasPatch,
) -> UpdateEventBody {
    UpdateEventBody {
        event_type,
        date_value: Some(parts.date_value()),
        date_qualifier: Some(parts.stored_qualifier()),
        date_value2: Some(parts.date_value2()),
        calendar: Some(parts.calendar),
        cause: extras.cause,
        age: extras.age,
        agency: extras.agency,
        spouse_ages: extras.spouse_ages,
        place_id: Some(place_id),
        description,
    }
}

/// What the create form holds for the person it creates. Each event is
/// its date, place, notes and source.
#[derive(Clone)]
struct NewPerson {
    sex: Sex,
    given: String,
    surname: String,
    particle_override: Option<String>,
    birth: (DateParts, String, String, String),
    death: (DateParts, String, String, String),
    source: String,
}

impl NewPerson {
    /// The translation key of what keeps the person from being created: the
    /// birth name and given names are required, and the dates must be valid.
    fn invalid(&self) -> Option<&'static str> {
        if self.given.is_empty() || self.surname.is_empty() {
            return Some("person_form.birth_identity_required");
        }
        self.birth.0.validate().or_else(|| self.death.0.validate())
    }
}

/// Creates the person the create form holds: the person, its birth name, its
/// birth and death with their notes and source, its own source, and the link
/// the form was opened for. Returns the new person's id.
async fn create_person(
    api: &ApiClient,
    tree_id: Uuid,
    i18n: &crate::i18n::I18n,
    context: &PersonFormCreateContext,
    new: &NewPerson,
) -> Result<Uuid, String> {
    let person = api
        .create_person(tree_id, &CreatePersonBody { sex: new.sex })
        .await
        .map_err(|_| i18n.t("person_form.create_failed"))?;
    let person_id = person.id;

    let split = resolve_particle(&new.surname, new.particle_override.as_deref());
    let name = CreatePersonNameBody {
        name_type: NameType::Birth,
        given_names: Some(new.given.clone()),
        surname: Some(split.root),
        surname_prefix: split.particle,
        prefix: None,
        suffix: None,
        nickname: None,
        is_primary: true,
        sort_order: 0,
    };
    api.create_person_name(tree_id, person_id, &name)
        .await
        .map_err(|e| e.message(i18n))?;

    for (event_type, (parts, place, notes, source)) in [
        (EventType::Birth, &new.birth),
        (EventType::Death, &new.death),
    ] {
        let event = VitalEvent {
            event_type,
            existing_id: None,
            parts,
            place,
            notes,
            source,
            current: &NotesSource::default(),
        };
        save_vital_event(api, tree_id, person_id, i18n.0.reference_code(), event)
            .await
            .map_err(|e| e.message(i18n))?;
    }

    let _ = save_notes_source(
        api,
        tree_id,
        Some(person_id),
        None,
        "",
        &new.source,
        &NotesSource::default(),
    )
    .await;

    if let Some(link) = ParentLink::of(context) {
        link.link(api, tree_id, person_id, i18n).await?;
    }
    Ok(person_id)
}

/// Saves what the edit form holds over the stored person: its sex and
/// privacy, its birth name, its birth and death with their notes and source,
/// then its own source. The error is the reader's.
async fn save_person_edits(
    api: &ApiClient,
    tid: Uuid,
    pid: Uuid,
    i18n: &crate::i18n::I18n,
    held: &NewPerson,
    privacy: Privacy,
    targets: HeldFields,
) -> Result<(), String> {
    let failed = |e: ApiError| e.message(i18n);

    // 1. Sex and privacy.
    let person_body = UpdatePersonBody {
        sex: Some(held.sex),
        privacy: Some(privacy),
    };
    api.update_person(tid, pid, &person_body)
        .await
        .map_err(failed)?;

    // 2. Birth name (surname + given names).
    let split = resolve_particle(&held.surname, held.particle_override.as_deref());
    let birth_name_id = *targets.birth_name_id.peek();
    if let Some(name_id) = birth_name_id {
        let name_body = UpdatePersonNameBody {
            name_type: None,
            given_names: Some(opt_str(&held.given)),
            surname: Some(opt_str(&split.root)),
            surname_prefix: Some(split.particle),
            prefix: None,
            suffix: None,
            nickname: None,
            is_primary: None,
            sort_order: None,
        };
        api.update_person_name(tid, pid, name_id, &name_body)
            .await
            .map_err(failed)?;
    } else {
        let name_body = CreatePersonNameBody {
            name_type: NameType::Birth,
            given_names: Some(held.given.clone()),
            surname: Some(split.root),
            surname_prefix: split.particle,
            prefix: None,
            suffix: None,
            nickname: None,
            is_primary: true,
            sort_order: 0,
        };
        api.create_person_name(tid, pid, &name_body)
            .await
            .map_err(failed)?;
    }

    // 3. Birth and death, each with its notes and source.
    for (event_type, (parts, place, notes, source), draft) in [
        (EventType::Birth, &held.birth, targets.birth),
        (EventType::Death, &held.death, targets.death),
    ] {
        let current = draft.stored.peek().clone();
        let event = VitalEvent {
            event_type,
            existing_id: *draft.event_id.peek(),
            parts,
            place,
            notes,
            source,
            current: &current,
        };
        let stored = save_vital_event(api, tid, pid, i18n.0.reference_code(), event)
            .await
            .map_err(failed)?;
        if let Some(stored) = stored {
            let mut target = draft.stored;
            target.set(stored);
        }
    }

    // 4. The person's own source. Only the source travels with the person:
    //    its notes are the list under Civil Status, each row saved on its
    //    own, so this save must not touch them — and keeps them out of the
    //    state it reconciles against.
    let without_notes = |ns: NotesSource| NotesSource {
        notes: String::new(),
        note_id: None,
        ..ns
    };
    let current = without_notes(targets.source_stored.peek().clone());
    if let Ok(stored) =
        save_notes_source(api, tid, Some(pid), None, "", &held.source, &current).await
    {
        let mut source_stored = targets.source_stored;
        source_stored.set(without_notes(stored));
    }
    Ok(())
}

struct VitalEvent<'a> {
    event_type: EventType,
    existing_id: Option<Uuid>,
    parts: &'a DateParts,
    place: &'a str,
    notes: &'a str,
    source: &'a str,
    current: &'a NotesSource,
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
async fn save_vital_event(
    api: &ApiClient,
    tree_id: Uuid,
    person_id: Uuid,
    lang: &str,
    event: VitalEvent<'_>,
) -> Result<Option<NotesSource>, ApiError> {
    let VitalEvent {
        event_type,
        existing_id,
        parts,
        place,
        notes,
        source,
        current,
    } = event;
    let place_id = resolve_place(api, tree_id, place, lang).await?;
    let event_id = match existing_id {
        Some(eid) => {
            api.update_event(
                tree_id,
                eid,
                &update_event_body(
                    Some(event_type),
                    parts,
                    place_id,
                    None,
                    EventExtrasPatch::default(),
                ),
            )
            .await?;
            Some(eid)
        }
        None if !parts.is_empty() || place_id.is_some() => Some(
            api.create_event(
                tree_id,
                &create_event_body(
                    event_type,
                    parts,
                    place_id,
                    EventOwner::Person(person_id),
                    None,
                    EventExtras::default(),
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

/// An event's witnesses, with the names of the persons they are, loaded
/// in one request.
async fn load_witnesses(
    api: &ApiClient,
    tree_id: Uuid,
    event_id: Uuid,
) -> (
    Vec<oxidgene_core::types::EventWitness>,
    std::collections::HashMap<Uuid, Vec<oxidgene_core::types::PersonName>>,
) {
    let witnesses = api
        .list_event_witnesses(tree_id, event_id)
        .await
        .unwrap_or_default();
    let person_ids: Vec<Uuid> = witnesses.iter().map(|w| w.person_id).collect();
    let names = api
        .person_names(tree_id, &person_ids)
        .await
        .unwrap_or_default();
    (witnesses, names)
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
    let mut error = use_signal(|| None::<String>);

    let api_list = api.clone();
    let witnesses_resource = use_ui_resource("event_witnesses", move || {
        let api = api_list.clone();
        let _tick = refresh_tick();
        async move {
            let Some(event_id) = event_id else {
                return Default::default();
            };
            load_witnesses(&api, tree_id, event_id).await
        }
    });

    let Some(event_id) = event_id else {
        return rsx! {
            p { class: "text-muted", {i18n.t("person_form.witnesses_save_first")} }
        };
    };
    let (witnesses, names) = witnesses_resource.read().clone().unwrap_or_default();
    let entries: Vec<_> = witnesses
        .into_iter()
        .map(|w| {
            let name = resolve_name(w.person_id, &names, &i18n);
            (w, name)
        })
        .collect();

    rsx! {
        if let Some(message) = error() {
            div { class: "error-msg", role: "alert", "{message}" }
        }
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
                                    let removed = api.remove_event_witness(tree_id, event_id, witness_id).await;
                                    error.set(removed.err().map(|e| e.message(&i18n)));
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
                                let added = api.add_event_witness(tree_id, event_id, &body).await;
                                error.set(added.err().map(|e| e.message(&i18n)));
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
// cannot hold sourceless notes. Citation text describes the source evidence,
// independently of the editable notes.

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
    /// How reliable the citation says the source is; `None` when not
    /// assessed, or when there is no citation.
    confidence: Option<Confidence>,
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
    // The server finds the tree's source of that title rather than the form
    // reading every source of the tree on each save.
    if let Some(existing) = api.find_source(tree_id, title).await? {
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
                agency: None,
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
        .list_notes(tree_id, person_filter, event_filter, None, None, None)
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

    let source_id = citation.as_ref().map(|c| c.source_id);
    let confidence = citation.as_ref().and_then(|c| c.confidence);
    let source_title = match source_id {
        Some(sid) => api
            .get_source(tree_id, sid)
            .await
            .map(|s| s.title)
            .unwrap_or_default(),
        None => String::new(),
    };
    NotesSource {
        notes: note.as_ref().map(|n| n.text.clone()).unwrap_or_default(),
        source_title,
        source_id,
        note_id: note.map(|n| n.id),
        citation_id: citation.map(|c| c.id),
        confidence,
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
                        ..UpdateNoteBody::default()
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
                    media_id: None,
                    repository_id: None,
                    text: notes.to_string(),
                    person_id: owner,
                    event_id,
                    family_id: None,
                    source_id: None,
                    confidential: false,
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
        confidence: current.confidence,
    };

    // Only touch the sources when the typed title actually changed, so an
    // unrelated save never creates a `Source` row as a side effect.
    if source_title.trim() == current.source_title.trim() {
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
                    text: None,
                },
            )
            .await?;
        }
        (Some(cid), None) => {
            api.delete_citation(tree_id, cid).await?;
            saved.citation_id = None;
            saved.confidence = None;
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
                        confidence: None,
                        text: None,
                    },
                )
                .await?
                .id,
            );
        }
        (None, None) => {}
    }

    if let Some(previous) = current.source_id
        && Some(previous) != source_id
    {
        let_go_source(api, tree_id, previous).await;
    }

    Ok(saved)
}

/// Deletes source `previous` of tree `tree_id`, which a citation was just
/// repointed away from, if nothing uses it any more: free-text entry mints
/// one `Source` per distinct title, so a corrected typo would otherwise
/// leave its row in the tree, and in the source dictionary, forever. The
/// server keeps any source still referenced, so this only ever collects
/// what nothing points at. A scoped Editor deletes no source
/// (docs/authorization.md §3.3): the form leaves it in place.
async fn let_go_source(api: &ApiClient, tree_id: Uuid, previous: Uuid) {
    if crate::permissions::deletes_shared_records(api.cached_viewer_access(tree_id).as_ref()) {
        let _ = api.delete_source_if_unused(tree_id, previous).await;
    }
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
/// It suggests the tree's source titles from a debounced query rather than
/// a `<datalist>` of them all: an imported tree has thousands of sources, and
/// re-diffing that many `<option>` nodes on every keystroke made the field
/// unusable.
pub(crate) fn render_notes_source_fields(
    i18n: &crate::i18n::I18n,
    tree_id: Uuid,
    mut notes: Signal<String>,
    source_title: Signal<String>,
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
            ValueInput {
                value: source_title,
                tree_id,
                field: SuggestionField::Sources,
                placeholder: i18n.t("person_form.source_placeholder"),
                on_change: move |()| on_edit_source(),
            }
        }
    }
}

/// The witnesses block of a saved event's editor: the persons of the tree
/// the record names beside the event's subject — witnesses, godparents, the
/// informant — each with the role it gives them. Their own writes, like the
/// evidence: a witness is linked the moment it is chosen. An occupation has
/// none, being a state rather than an act anybody witnessed.
fn render_event_witnesses(i18n: &crate::i18n::I18n, tree_id: Uuid, event: &CoreEvent) -> Element {
    if event.event_type == EventType::Occupation {
        return rsx! {};
    }
    rsx! {
        div { class: "pf-ns-block",
            label { class: "pf-ns-label", {i18n.t("person_form.witnesses")} }
            EventWitnesses { tree_id, event_id: Some(event.id) }
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
    /// A family event's spouses (person, name): one age field each.
    #[props(default)]
    spouses: Vec<(Uuid, String)>,
    on_saved: EventHandler<()>,
) -> Element {
    let api = use_context::<ApiClient>();
    let i18n = use_i18n();

    let event_id = event.id;
    let is_occupation = event.event_type == EventType::Occupation;

    let mut description = use_signal(|| event.description.clone().unwrap_or_default());
    let cause = use_signal(|| event.cause.clone().unwrap_or_default());
    // A family event states each spouse's age, not its own.
    let age = use_signal(|| event.age.clone().unwrap_or_default());
    let age = event.family_id.is_none().then_some(age);
    let spouse_ages = use_signal(|| typed_spouse_ages(&event.spouse_ages));
    let family_spouses = spouses.clone();
    let edits_spouse_ages = event.family_id.is_some() && !spouses.is_empty();
    let agency = use_signal(|| event.agency.clone().unwrap_or_default());
    // Open from the start when it holds something, so nothing stored hides.
    let mut more = use_signal(|| event.agency.is_some());
    let mut reliability = use_signal(|| None::<Confidence>);
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
    let resource = use_ui_resource("event_notes_source", move || {
        let api = api_load.clone();
        async move { load_notes_source(&api, tree_id, person_id, Some(event_id)).await }
    });

    use_effect(move || {
        if let Some(ns) = &*resource.read()
            && loaded.peek().is_none()
        {
            notes.set(ns.notes.clone());
            source_title.set(ns.source_title.clone());
            reliability.set(ns.confidence);
            if ns.confidence.is_some() {
                more.set(true);
            }
            loaded.set(Some(ns.clone()));
        }
    });

    let api_save = api.clone();
    let on_save = move |_| {
        let api = api_save.clone();
        let Some(current) = loaded() else { return };
        let notes_val = notes();
        let source = source_title();
        let desc = description().trim().to_string();
        let extras = EventExtras::from_form(
            &cause(),
            &age.map(|age| age()).unwrap_or_default(),
            &agency(),
        )
        .and_then(|mut extras| {
            extras.spouse_ages = spouse_ages_from_form(&family_spouses, &spouse_ages.read())?;
            Ok(extras)
        });
        let confidence = reliability();
        let date = parts();
        let place = place_id();
        spawn(async move {
            if let Some(key) = date.validate() {
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
            saving.set(true);
            error.set(None);
            let edit = EventEdit {
                date,
                place,
                description: desc,
                extras: extras.into_patch(edits_spouse_ages),
                notes: notes_val,
                source,
                confidence,
            };
            let target = (person_id, event_id);
            match save_event_edit(
                &api,
                tree_id,
                target,
                i18n.0.reference_code(),
                edit,
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
                Err(e) => error.set(Some(e.message(&i18n))),
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
                    if is_occupation {
                        ValueInput {
                            value: description,
                            tree_id,
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
                    DateInput { parts, i18n, on_change: move |()| {} }
                }
                div { class: "form-row",
                    {render_place_input(&i18n, tree_id, place_id, &place_options, || {})}
                    {render_cause_age_fields(&i18n, cause, age)}
                }
                if edits_spouse_ages {
                    div { class: "form-row",
                        {render_spouse_age_fields(&i18n, &spouses, spouse_ages)}
                    }
                }
                {render_notes_source_fields(&i18n, tree_id, notes, source_title, || {})}
                MoreDetails { open: more,
                    {render_agency_field(&i18n, agency)}
                    // A reliability qualifies a citation, so it needs a source.
                    if !source_title().trim().is_empty() {
                        {render_reliability_field(&i18n, reliability)}
                    }
                }
                div { class: "pf-ns-actions",
                    button {
                        class: "pf-confirm-btn",
                        r#type: "button",
                        disabled: saving(),
                        onclick: on_save,
                        if saving() { {i18n.t("common.saving")} } else { {i18n.t("common.save")} }
                    }
                }

                {render_event_witnesses(&i18n, tree_id, &event)}

                // ── Evidence ──
                // The documents that prove this event, hung off the event
                // itself rather than off the person. The same scan may be a
                // person's photograph *and* an event's proof, and they are two
                // different claims: only the second belongs beside a citation.
                //
                // Outside the Save button above, because these are their own
                // writes — a file is uploaded and attached the moment it is
                // chosen, and making it wait for a form save would mean an
                // upload silently lost by a Cancel.
                div { class: "pf-ns-block",
                    label { class: "pf-ns-label", {i18n.t("media.evidence")} }
                    p { class: "pf-ns-hint", {i18n.t("media.evidence_hint")} }
                    MediaGallery {
                        tree_id,
                        owner: MediaOwner::Event(event_id),
                    }
                }
            }
        }
    }
}

/// What the event editor saves: the event's own fields, then its notes,
/// source and the source's reliability.
struct EventEdit {
    date: DateParts,
    place: String,
    description: String,
    extras: EventExtrasPatch,
    notes: String,
    source: String,
    confidence: Option<Confidence>,
}

/// Writes `edit` to the event `target` names (its person, if any, and its
/// id), reconciling notes and source against `current`; the notes and
/// source state now stored.
async fn save_event_edit(
    api: &ApiClient,
    tree_id: Uuid,
    (person_id, event_id): (Option<Uuid>, Uuid),
    lang: &str,
    edit: EventEdit,
    current: &NotesSource,
) -> Result<NotesSource, ApiError> {
    let place_id = resolve_place(api, tree_id, &edit.place, lang).await?;
    let body = update_event_body(
        None,
        &edit.date,
        place_id,
        Some(opt_str(&edit.description)),
        edit.extras,
    );
    api.update_event(tree_id, event_id, &body).await?;
    let stored = save_notes_source(
        api,
        tree_id,
        person_id,
        Some(event_id),
        &edit.notes,
        &edit.source,
        current,
    )
    .await?;
    save_reliability(api, tree_id, stored, edit.confidence).await
}

/// Writes the reliability chosen for the citation `stored` holds, when it
/// differs from the stored one; the state now stored.
async fn save_reliability(
    api: &ApiClient,
    tree_id: Uuid,
    mut stored: NotesSource,
    confidence: Option<Confidence>,
) -> Result<NotesSource, ApiError> {
    let Some(citation_id) = stored.citation_id else {
        return Ok(stored);
    };
    if stored.confidence != confidence {
        api.update_citation(
            tree_id,
            citation_id,
            &UpdateCitationBody {
                source_id: None,
                page: None,
                confidence: Some(confidence),
                text: None,
            },
        )
        .await?;
        stored.confidence = confidence;
    }
    Ok(stored)
}

/// How reliable the cited source is, "not assessed" included.
fn render_reliability_field(
    i18n: &crate::i18n::I18n,
    mut reliability: Signal<Option<Confidence>>,
) -> Element {
    let selected = reliability().map(|c| c.to_string()).unwrap_or_default();
    rsx! {
        div { class: "form-group",
            label { {i18n.t("person_form.reliability")} }
            select {
                value: "{selected}",
                oninput: move |e: Event<FormData>| {
                    let value = e.value();
                    reliability.set(
                        crate::utils::CONFIDENCE_LEVELS
                            .into_iter()
                            .find(|c| c.to_string() == value),
                    );
                },
                option { value: "", selected: selected.is_empty(), {i18n.t("confidence.not_assessed")} }
                for level in crate::utils::CONFIDENCE_LEVELS {
                    option {
                        value: "{level}",
                        selected: selected == level.to_string(),
                        {i18n.t(crate::utils::confidence_key(level))}
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

/// The picker values of the two pieces that are no type of name.
const PREFIX_CHOICE: &str = "Prefixe";
const SUFFIX_CHOICE: &str = "Suffixe";

/// The information picker's entries, value and label key, in its order: the
/// name types an information may carry, and the prefix and suffix pieces.
fn information_choices() -> [(&'static str, &'static str); 10] {
    let name_type = |nt| (name_type_value(nt), name_type_label_key(nt));
    [
        name_type(NameType::GivenName),
        name_type(NameType::Married),
        name_type(NameType::Alias),
        name_type(NameType::Byname),
        name_type(NameType::Maiden),
        name_type(NameType::Religious),
        (PREFIX_CHOICE, "name_type.prefixe"),
        (SUFFIX_CHOICE, "name_type.suffixe"),
        name_type(NameType::Other),
        name_type(NameType::Sobriquet),
    ]
}

fn info_piece(info_type: &str) -> InfoPiece {
    if info_type == PREFIX_CHOICE {
        return InfoPiece::Prefix;
    }
    if info_type == SUFFIX_CHOICE {
        return InfoPiece::Suffix;
    }
    match parse_name_type(info_type) {
        NameType::GivenName => InfoPiece::Given,
        NameType::Sobriquet | NameType::Byname => InfoPiece::Nickname,
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
    tree_id: Uuid,
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
                        for (value, label) in information_choices() {
                            option { value, {i18n.t(label)} }
                        }
                    }
                }
                div { class: "form-group",
                    label { "{value_label}" }
                    match piece {
                        InfoPiece::Given => rsx! {
                            ValueInput { value: value_sig, tree_id, field: SuggestionField::GivenNames }
                        },
                        InfoPiece::Surname => rsx! {
                            ValueInput { value: value_sig, tree_id, field: SuggestionField::FamilyNames }
                        },
                        _ => rsx! {
                            input {
                                r#type: "text",
                                value: "{value_sig}",
                                oninput: move |e: Event<FormData>| value_sig.set(e.value()),
                            }
                        },
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

    /// A failed child link while founding the parents' family is reported,
    /// and the empty family it leaves is deleted, instead of the parent
    /// being made the spouse of a family with no child.
    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn a_parent_whose_child_link_fails_is_not_linked() {
        let (listener, address) = crate::api::test_server::listen().await;
        let api = ApiClient::new(&address);
        let i18n = crate::i18n::I18n::new(crate::i18n::Language::english());
        let (tree, family) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let family_json = format!(
            r#"{{"id":"{family}","tree_id":"{tree}","created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z","deleted_at":null}}"#
        );
        let link = ParentLink {
            child_id: Uuid::from_u128(3),
            family_id: None,
            is_father: true,
        };
        let responses = [
            (200, family_json.as_str()),
            (500, r#"{"error":"internal"}"#),
            (204, ""),
        ];
        let (result, requests) = tokio::join!(
            link.link(&api, tree, Uuid::from_u128(4), &i18n),
            crate::api::test_server::serve_json(&listener, &responses)
        );
        assert_eq!(result, Err(i18n.t("person_form.create_failed")));
        assert_eq!(
            requests,
            [
                format!("POST /api/v1/trees/{tree}/families HTTP/1.1"),
                format!("POST /api/v1/trees/{tree}/families/{family}/children HTTP/1.1"),
                format!("DELETE /api/v1/trees/{tree}/families/{family} HTTP/1.1"),
            ]
        );
    }

    /// Saving an event from its editor writes the cause it shows — set,
    /// changed or cleared — while the shortcuts that do not show one leave
    /// the stored cause alone.
    #[test]
    fn an_event_update_carries_the_cause_only_when_the_form_edits_it() {
        let parts = DateParts::default();
        let body = |cause| {
            serde_json::to_value(update_event_body(
                None,
                &parts,
                None,
                None,
                EventExtrasPatch {
                    cause,
                    ..Default::default()
                },
            ))
            .expect("serializes")
        };
        assert_eq!(body(Some(Some("Fever".into())))["cause"], "Fever");
        assert!(body(Some(None))["cause"].is_null());
        assert!(
            body(Some(None))
                .as_object()
                .expect("object")
                .contains_key("cause")
        );
        assert!(
            !body(None)
                .as_object()
                .expect("object")
                .contains_key("cause")
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn citation_evidence_is_not_loaded_or_saved_as_a_note() {
        use serde_json::json;
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

        for has_note in [false, true] {
            let tree = Uuid::now_v7();
            let person = Uuid::now_v7();
            let source = Uuid::now_v7();
            let citation = Uuid::now_v7();
            let note = Uuid::now_v7();
            let now = chrono::Utc::now();
            let notes = if has_note {
                vec![json!({"cursor": note, "node": {
                    "id": note, "tree_id": tree, "person_id": person,
                    "text": "Editable note", "created_at": now, "updated_at": now
                }})]
            } else {
                vec![]
            };
            let responses = [
                (
                    "/notes",
                    json!({"edges": notes, "total_count": notes.len(),
                    "page_info": {"has_next_page": false, "end_cursor": null}}),
                ),
                (
                    "/citations",
                    json!({"edges": [{"cursor": citation, "node": {
                    "id": citation, "source_id": source, "person_id": person,
                    "confidence": null, "text": "Imported source evidence",
                    "created_at": now, "updated_at": now
                }}], "total_count": 1,
                    "page_info": {"has_next_page": false, "end_cursor": null}}),
                ),
                (
                    "/sources/",
                    json!({"id": source, "tree_id": tree,
                    "title": "Fictional source", "created_at": now, "updated_at": now}),
                ),
            ];
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let api = ApiClient::new(&format!("http://{}", listener.local_addr().unwrap()));
            let server = async {
                for (path, body) in responses {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let mut request = String::new();
                    let mut reader = tokio::io::BufReader::new(&mut socket);
                    while !request.ends_with("\r\n\r\n") {
                        assert!(reader.read_line(&mut request).await.unwrap() > 0);
                    }
                    assert!(request.starts_with("GET "));
                    assert!(request.lines().next().unwrap().contains(path));
                    let body = body.to_string();
                    socket.write_all(format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()
                    ).as_bytes()).await.unwrap();
                }
            };
            let (loaded, ()) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                tokio::join!(load_notes_source(&api, tree, Some(person), None), server)
            })
            .await
            .expect("notes, citations and source must all be requested");
            assert_eq!(loaded.notes, if has_note { "Editable note" } else { "" });
            assert_eq!(loaded.note_id, has_note.then_some(note));
            assert_eq!(loaded.citation_id, Some(citation));
            assert_eq!(loaded.source_title, "Fictional source");
            let saved = tokio::time::timeout(
                std::time::Duration::from_secs(1),
                save_notes_source(
                    &api,
                    tree,
                    Some(person),
                    None,
                    &loaded.notes,
                    &loaded.source_title,
                    &loaded,
                ),
            )
            .await
            .expect("an unchanged save must not send mutations")
            .unwrap();
            assert_eq!(saved, loaded);
        }
    }

    #[test]
    fn father_inherits_the_child_surname() {
        let context = PersonFormCreateContext::AddParent {
            child_id: Uuid::nil(),
            family_id: None,
            is_father: true,
            child_surname: Some(" de la Cruz ".to_string()),
        };

        assert_eq!(context.initial_surname().as_deref(), Some("de la Cruz"));
    }

    #[test]
    fn mother_does_not_inherit_the_child_surname() {
        let context = PersonFormCreateContext::AddParent {
            child_id: Uuid::nil(),
            family_id: None,
            is_father: false,
            child_surname: Some("DUPONT".to_string()),
        };

        assert_eq!(context.initial_surname(), None);
    }

    #[test]
    fn father_does_not_inherit_an_empty_child_surname() {
        let context = PersonFormCreateContext::AddParent {
            child_id: Uuid::nil(),
            family_id: None,
            is_father: true,
            child_surname: Some("  ".to_string()),
        };

        assert_eq!(context.initial_surname(), None);
    }

    #[test]
    fn prefix_and_suffix_reach_their_own_pieces() {
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
