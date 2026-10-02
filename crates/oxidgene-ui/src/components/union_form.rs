//! Modal-based couple/family edit form (spec §16).
//!
//! Body is divided into: Union (events, date/place/note shorthand),
//! Children (with staged detach, applied on Save), Person 1 / Person 2
//! (collapsible, embedding the full person edit fields). Footer holds
//! Delete couple (removes the union only — persons remain in the tree)
//! plus Cancel / Save.

use std::collections::{HashMap, HashSet};

use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::{AddChildBody, ApiClient, ApiError};
use crate::components::date_input::{DateInput, DateParts, format_event_date};
use crate::components::media_gallery::{MediaGallery, MediaOwner};
use crate::components::modal::Modal;
use crate::components::person_form::{
    DeleteSection, EventEditor, EventExtras, EventExtrasPatch, EventOwner, FormSection,
    NotesSource, PersonForm, create_event_body, focus_next_field_js, render_add_toggle,
    render_choice_group, render_notes_source_fields, render_spouse_age_fields, save_notes_source,
    spouse_ages_from_form, typed_spouse_ages, update_event_body,
};
use crate::components::place_input::{render_place_input, resolve_place};
use crate::components::search_person::SearchPerson;
use crate::i18n::{I18n, use_i18n};
use crate::ui_observability::use_ui_resource;
use crate::utils::{
    child_type_label_key, event_type_label_key, opt_str, parse_privacy, resolve_name,
};
use oxidgene_core::types::SpouseAge;
use oxidgene_core::types::{
    Connection, Event as StoredEvent, Family, FamilyChild, FamilySpouse, PersonName,
};
use oxidgene_core::{ChildType, EventType};

// ── Props ────────────────────────────────────────────────────────────────

#[derive(Props, Clone, PartialEq)]
pub struct UnionFormProps {
    /// Tree ID.
    pub tree_id: Uuid,
    /// Family ID to edit.
    pub family_id: Uuid,
    /// Called when the form is closed.
    pub on_close: EventHandler<()>,
    /// Called when data is saved (so parent can refresh).
    pub on_saved: EventHandler<()>,
}

// ── Component ────────────────────────────────────────────────────────────

/// Modal couple/family edit form.
#[component]
pub fn UnionForm(props: UnionFormProps) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let refresh = use_signal(|| 0u32);

    let tid = props.tree_id;
    let fid = props.family_id;
    // A couple has a privacy of its own: a living pair's marriage is a fact
    // about two living people, and withholding both their person records does
    // not withhold the union that names them.
    let privacy_val = use_signal(|| "Default".to_string());
    let open_privacy = use_signal(|| true);

    // ── State ──
    let save_error = use_signal(|| None::<String>);

    // Which union event row is expanded into its full editor.
    let open_union_event = use_signal(|| None::<Uuid>);
    let show_add_union_event = use_signal(|| false);
    let new_union_event = use_new_union_event();

    // Add child linking mode.
    let show_add_child = use_signal(|| false);

    // Section fold state. The union's own blocks open with the form; the two
    // person blocks stay closed, since each mounts a whole PersonForm with its
    // own fetches — opening both by default would load the couple twice over.
    let open_union = use_signal(|| true);
    let open_children = use_signal(|| true);
    let open_media = use_signal(|| true);
    let show_person1 = use_signal(|| false);
    let show_person2 = use_signal(|| false);

    // Staged child detach (applied on Save).
    let pending_detach = use_signal(HashSet::<Uuid>::new);
    let confirm_detach_id = use_signal(|| None::<Uuid>);

    // Delete couple state (the confirmation itself lives in DeleteSection).
    let delete_error = use_signal(|| None::<String>);
    let deleting = use_signal(|| false);
    let saving = use_signal(|| false);

    // ── Resources ──

    // Seeded once from the stored row: re-seeding on every render would fight
    // the user's own clicks.
    let family_resource = use_ui_resource("family", {
        let api = api.clone();
        move || {
            let api = api.clone();
            async move { api.get_family(tid, fid).await.ok() }
        }
    });
    use_seed_privacy(family_resource, privacy_val);

    let spouses_resource = use_ui_resource("family_spouses", {
        let api = api.clone();
        move || {
            let api = api.clone();
            let _tick = refresh();
            async move { api.list_family_spouses(tid, fid).await }
        }
    });
    let children_resource = use_ui_resource("family_children", {
        let api = api.clone();
        move || {
            let api = api.clone();
            let _tick = refresh();
            async move { api.list_family_children(tid, fid).await }
        }
    });
    let events_resource = use_ui_resource("family_events", {
        let api = api.clone();
        move || {
            let api = api.clone();
            let _tick = refresh();
            async move {
                api.list_events(tid, Some(100), None, None, None, Some(fid))
                    .await
            }
        }
    });
    let places_resource = use_ui_resource("places", {
        let api = api.clone();
        move || {
            let api = api.clone();
            let _tick = refresh();
            // Every page: an event may sit on any place in the tree, and a
            // place missing from this list has no name to show.
            async move { api.list_all_places(tid).await }
        }
    });
    // Names only for the spouses and children shown in this modal, in one
    // request once both lists are in.
    let names_resource = use_ui_resource("family_names", {
        let api = api.clone();
        move || {
            let api = api.clone();
            let members = family_members(&spouses_resource.read(), &children_resource.read());
            async move {
                match members {
                    Some(ids) => api.person_names(tid, &ids).await,
                    None => Ok(HashMap::new()),
                }
            }
        }
    });
    let marriage = use_marriage_draft(events_resource);

    // ── Derived data ──

    let union_events: Vec<StoredEvent> = match &*events_resource.read() {
        Some(Ok(conn)) => conn
            .edges
            .iter()
            .filter(|e| is_union_event(e.node.event_type))
            .map(|e| e.node.clone())
            .collect(),
        _ => vec![],
    };
    let place_options: Vec<(String, String)> = match &*places_resource.read() {
        Some(Ok(places)) => places
            .iter()
            .map(|p| (p.id.to_string(), p.name.clone()))
            .collect(),
        _ => vec![],
    };
    let names: HashMap<Uuid, Vec<PersonName>> = match &*names_resource.read() {
        Some(Ok(map)) => map.clone(),
        _ => HashMap::new(),
    };
    // Spouses sorted by sort_order — drives the header title and Person 1/2 blocks.
    let spouses: Vec<FamilySpouse> = match &*spouses_resource.read() {
        Some(Ok(spouses)) => {
            let mut v = spouses.clone();
            v.sort_by_key(|s| s.sort_order);
            v
        }
        _ => vec![],
    };
    let couple_title = couple_title(&spouses, &names, &i18n);

    let scope = FormScope {
        api: api.clone(),
        i18n,
        tid,
        fid,
        writes: Writes {
            save_error,
            refresh,
            on_saved: props.on_saved,
        },
    };
    let spouse_names: Vec<(Uuid, String)> = spouses
        .iter()
        .map(|s| (s.person_id, resolve_name(s.person_id, &names, &i18n)))
        .collect();
    let events_section = UnionEventsSection {
        scope: scope.clone(),
        spouses: spouse_names,
        events: &union_events,
        place_options: &place_options,
        marriage,
        new_event: new_union_event,
        show_add: show_add_union_event,
        open_event: open_union_event,
    };
    let children_section = ChildrenSection {
        scope: scope.clone(),
        names: &names,
        show_add: show_add_child,
        pending_detach,
        confirm_detach: confirm_detach_id,
    };
    let union_event_choices = union_event_choices(&union_events, &i18n);

    // ── Render ──

    rsx! {
        Modal {
            class: "union-form-modal",
            label: couple_title.clone(),
            on_close: props.on_close,
            on_key: move |e: KeyboardEvent| {
                if e.key() == Key::Enter {
                    document::eval(&focus_next_field_js("union-form-modal"));
                }
            },

            // Header
            div { class: "union-form-header",
                div {
                    h2 { "{couple_title}" }
                    span { class: "pf-subtitle", {i18n.t("union_form.subtitle_edit")} }
                }
                div { class: "uf-header-actions",
                    button {
                        class: "person-form-close",
                        onclick: move |_| props.on_close.call(()),
                        "x"
                    }
                }
            }

            if let Some(err) = save_error() {
                div { class: "error-msg", style: "margin: 0 16px;", "{err}" }
            }

            div { class: "union-form-body",
                // ── Person 1 / Person 2 blocks ──
                // Each mounts a whole PersonForm with its own fetches, so
                // both stay closed by default — opening them eagerly would
                // load the couple twice over.
                {spouse_section(&scope, spouses.first(), "union_form.person1", show_person1, &names)}
                {spouse_section(&scope, spouses.get(1), "union_form.person2", show_person2, &names)}

                // ── Union block ──
                FormSection {
                    title: i18n.t("union_form.events"),
                    open: open_union,
                    action: render_add_toggle(
                        i18n.t("union_form.add_event"),
                        i18n.t("common.cancel"),
                        show_add_union_event,
                    ),
                    {events_section.render()}
                }

                // ── Children block ──
                FormSection {
                    title: i18n.t("union_form.children"),
                    open: open_children,
                    action: render_add_toggle(
                        i18n.t("union_form.add_child"),
                        i18n.t("common.cancel"),
                        show_add_child,
                    ),
                    {children_section.render(&children_resource.read())}
                }

                // ── Privacy ──
                FormSection { title: i18n.t("union_form.privacy"), open: open_privacy,
                    {privacy_choice(&scope, privacy_val)}
                    p { class: "pf-ns-hint", {i18n.t("privacy.not_enforced_yet")} }
                }

                // ── Media ──
                // In the body and above the delete button, exactly where
                // a person's documents are: a couple's papers are the same
                // kind of thing as a person's, and reaching them through a
                // separate button in the header made them look like a
                // different feature.
                FormSection { title: i18n.t("media.section"), open: open_media,
                    MediaGallery {
                        tree_id: tid,
                        owner: MediaOwner::Family(fid),
                        events: union_event_choices,
                        // Media writes land immediately and are not part of
                        // this form's save, so the card behind the modal
                        // must refresh even if the user then cancels.
                        // Every host treats `on_saved` as "refresh".
                        on_changed: move |()| props.on_saved.call(()),
                    }
                }

                // ── Delete couple ──
                // No section header and no rule above it, as in person_form:
                // the button already says what it does.
                DeleteSection {
                    button_label: i18n.t("union_form.delete_couple"),
                    title: i18n.t("union_form.delete_confirm_title"),
                    message: i18n.t("union_form.delete_confirm_message"),
                    confirm_label: i18n.t("union_form.delete_confirm_button"),
                    busy_label: i18n.t("union_form.deleting"),
                    deleting: deleting(),
                    error: delete_error(),
                    on_confirm: scope.delete_couple(deleting, delete_error, props.on_close),
                }
            }

            // ── Fixed footer ──
            div { class: "uf-footer",
                div { class: "uf-footer-right",
                    button {
                        class: "btn btn-outline",
                        r#type: "button",
                        onclick: move |_| props.on_close.call(()),
                        {i18n.t("common.cancel")}
                    }
                    button {
                        class: "btn btn-primary",
                        r#type: "button",
                        disabled: saving(),
                        onclick: scope.apply_detachments(children_resource, pending_detach, saving, props.on_close),
                        if saving() { {i18n.t("common.saving")} } else { {i18n.t("common.save")} }
                    }
                }
            }
        }
    }
}

/// Seeds the privacy choice once from the stored family.
fn use_seed_privacy(family: Resource<Option<Family>>, mut privacy: Signal<String>) {
    let mut loaded = use_signal(|| false);
    if !loaded()
        && let Some(Some(family)) = &*family.read_unchecked()
    {
        privacy.set(format!("{:?}", family.privacy));
        loaded.set(true);
    }
}

/// The links to delete to detach the children staged by person: the family's
/// child endpoint names the link between family and child, not the person.
fn links_to_detach(children: &[FamilyChild], staged_persons: &HashSet<Uuid>) -> Vec<Uuid> {
    children
        .iter()
        .filter(|child| staged_persons.contains(&child.person_id))
        .map(|child| child.id)
        .collect()
}

/// The family's spouses and children, once both lists are loaded.
fn family_members(
    spouses: &Option<Result<Vec<FamilySpouse>, ApiError>>,
    children: &Option<Result<Vec<FamilyChild>, ApiError>>,
) -> Option<Vec<Uuid>> {
    let (Some(spouses), Some(children)) = (spouses, children) else {
        return None;
    };
    let spouses = spouses.iter().flatten().map(|spouse| spouse.person_id);
    let children = children.iter().flatten().map(|child| child.person_id);
    Some(spouses.chain(children).collect())
}

/// The modal's title: the spouses' names, or a generic title without them.
fn couple_title(
    spouses: &[FamilySpouse],
    names: &HashMap<Uuid, Vec<PersonName>>,
    i18n: &I18n,
) -> String {
    match spouses {
        [] => i18n.t("union_form.title"),
        [only] => resolve_name(only.person_id, names, i18n),
        [first, second, ..] => format!(
            "{} & {}",
            resolve_name(first.person_id, names, i18n),
            resolve_name(second.person_id, names, i18n)
        ),
    }
}

/// Whether an event starts a couple: the one the date shorthand edits.
fn is_marriage_like(event_type: EventType) -> bool {
    matches!(
        event_type,
        EventType::Marriage
            | EventType::Engagement
            | EventType::MarriageBann
            | EventType::MarriageContract
            | EventType::MarriageLicense
            | EventType::MarriageSettlement
    )
}

/// The union events offered when adding one, core ones first.
const CORE_UNION_EVENTS: [(&str, &str); 11] = [
    ("Marriage", "event.type.marriage"),
    ("Divorce", "event.type.divorce"),
    ("Annulment", "event.type.annulment"),
    ("Engagement", "event.type.engagement"),
    ("MarriageBann", "event.type.marriage_bann"),
    ("MarriageContract", "event.type.marriage_contract"),
    ("MarriageLicense", "event.type.marriage_license"),
    ("MarriageSettlement", "event.type.marriage_settlement"),
    ("CivilUnion", "event.type.civil_union"),
    ("Separation", "event.type.separation"),
    ("DivorceFiled", "event.type.divorce_filed"),
];
const OPTIONAL_UNION_EVENTS: [(&str, &str); 7] = [
    ("Residence", "event.type.residence"),
    ("Census", "event.type.census"),
    ("Emigration", "event.type.emigration"),
    ("Immigration", "event.type.immigration"),
    ("Will", "event.type.will"),
    ("Probate", "event.type.probate"),
    ("Other", "event.type.other"),
];

/// Whether an event is one this form lists as the union's.
fn is_union_event(event_type: EventType) -> bool {
    matches!(
        event_type,
        EventType::Marriage
            | EventType::Divorce
            | EventType::Annulment
            | EventType::Engagement
            | EventType::MarriageBann
            | EventType::MarriageContract
            | EventType::MarriageLicense
            | EventType::MarriageSettlement
            | EventType::CivilUnion
            | EventType::Separation
            | EventType::DivorceFiled
            | EventType::Residence
            | EventType::Census
            | EventType::Emigration
            | EventType::Immigration
            | EventType::Will
            | EventType::Probate
            | EventType::Other
    )
}

/// The union's events as (id, label) pairs — what the media gallery offers
/// when asking which event a certificate documents.
fn union_event_choices(events: &[StoredEvent], i18n: &I18n) -> Vec<(Uuid, String)> {
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

/// What every write in the form does once it lands: clear or show the
/// error, tell the host, and reload the form's lists.
#[derive(Clone, Copy)]
struct Writes {
    save_error: Signal<Option<String>>,
    refresh: Signal<u32>,
    on_saved: EventHandler<()>,
}

impl Writes {
    /// Settles a write, handing back its value when it landed.
    fn settle<T>(mut self, result: Result<T, String>) -> Option<T> {
        match result {
            Ok(value) => {
                self.save_error.set(None);
                self.on_saved.call(());
                self.refresh += 1;
                Some(value)
            }
            Err(error) => {
                self.save_error.set(Some(error));
                None
            }
        }
    }
}

/// The family the form edits, and how its writes land.
#[derive(Clone)]
struct FormScope {
    api: ApiClient,
    i18n: I18n,
    tid: Uuid,
    fid: Uuid,
    writes: Writes,
}

impl FormScope {
    /// Validates an event's date and resolves its place.
    async fn event_place(&self, parts: &DateParts, place: &str) -> Result<Option<Uuid>, String> {
        if let Some(key) = parts.validate() {
            return Err(self.i18n.t(key));
        }
        resolve_place(&self.api, self.tid, place, self.i18n.0.code())
            .await
            .map_err(|e| e.to_string())
    }

    /// Links an existing person as a child of the couple.
    fn add_child(&self, mut show_add_child: Signal<bool>) -> impl FnMut(Uuid) + 'static {
        let scope = self.clone();
        move |person_id: Uuid| {
            let scope = scope.clone();
            spawn(async move {
                let body = AddChildBody {
                    person_id,
                    child_type: ChildType::Biological,
                    sort_order: 0,
                };
                let added = scope.api.add_child(scope.tid, scope.fid, &body).await;
                if scope
                    .writes
                    .settle(added.map_err(|e| e.to_string()))
                    .is_some()
                {
                    show_add_child.set(false);
                }
            });
        }
    }

    /// Removes one of the union's events.
    fn delete_event(&self, event_id: Uuid) -> impl FnMut(Event<MouseData>) + 'static {
        let scope = self.clone();
        move |_| {
            let scope = scope.clone();
            spawn(async move {
                let deleted = scope.api.delete_event(scope.tid, event_id).await;
                scope.writes.settle(deleted.map_err(|e| e.to_string()));
            });
        }
    }

    /// Applies the staged child detachments, then closes.
    fn apply_detachments(
        &self,
        children: Resource<Result<Vec<FamilyChild>, ApiError>>,
        pending_detach: Signal<HashSet<Uuid>>,
        mut saving: Signal<bool>,
        on_close: EventHandler<()>,
    ) -> impl FnMut(Event<MouseData>) + 'static {
        let scope = self.clone();
        move |_| {
            let scope = scope.clone();
            let mut save_error = scope.writes.save_error;
            let to_detach = match &*children.read() {
                Some(Ok(children)) => links_to_detach(children, &pending_detach.read()),
                _ => Vec::new(),
            };
            spawn(async move {
                saving.set(true);
                for link_id in to_detach {
                    if let Err(e) = scope.api.remove_child(scope.tid, scope.fid, link_id).await {
                        save_error.set(Some(format!("{e}")));
                        saving.set(false);
                        return;
                    }
                }
                saving.set(false);
                scope.writes.on_saved.call(());
                on_close.call(());
            });
        }
    }

    /// Deletes the couple — the union only, the persons remain in the tree.
    fn delete_couple(
        &self,
        mut deleting: Signal<bool>,
        mut delete_error: Signal<Option<String>>,
        on_close: EventHandler<()>,
    ) -> impl FnMut(()) + 'static {
        let scope = self.clone();
        move |()| {
            let scope = scope.clone();
            spawn(async move {
                deleting.set(true);
                delete_error.set(None);
                match scope.api.delete_family(scope.tid, scope.fid).await {
                    Ok(_) => {
                        scope.writes.on_saved.call(());
                        on_close.call(());
                    }
                    Err(e) => {
                        delete_error.set(Some(format!("{e}")));
                        deleting.set(false);
                    }
                }
            });
        }
    }
}

/// One spouse's block, embedding their whole person form.
fn spouse_section(
    scope: &FormScope,
    spouse: Option<&FamilySpouse>,
    key: &str,
    open: Signal<bool>,
    names: &HashMap<Uuid, Vec<PersonName>>,
) -> Element {
    let Some(spouse_id) = spouse.map(|spouse| spouse.person_id) else {
        return rsx! {};
    };
    let i18n = scope.i18n;
    let name = resolve_name(spouse_id, names, &i18n);
    let mut refresh = scope.writes.refresh;
    rsx! {
        FormSection { title: i18n.t_args(key, &[("name", &name)]), open,
            PersonForm {
                tree_id: scope.tid,
                person_id: Some(spouse_id),
                embedded: true,
                on_close: move |_| {},
                on_saved: move |_| refresh += 1,
            }
        }
    }
}

/// The couple's privacy choice, saved as soon as it changes.
fn privacy_choice(scope: &FormScope, privacy: Signal<String>) -> Element {
    let i18n = scope.i18n;
    let (api, tid, fid) = (scope.api.clone(), scope.tid, scope.fid);
    render_choice_group(
        &[
            ("Default", i18n.t("privacy.default")),
            ("Public", i18n.t("privacy.public")),
            ("Private", i18n.t("privacy.private")),
        ],
        privacy,
        move || {
            let api = api.clone();
            let privacy = parse_privacy(&privacy());
            spawn(async move {
                let _ = api.update_family_privacy(tid, fid, privacy).await;
            });
        },
    )
}

/// The marriage shorthand's fields, mapped to the couple's first
/// marriage-like event.
#[derive(Clone, Copy)]
struct MarriageDraft {
    parts: Signal<DateParts>,
    place_id: Signal<String>,
    desc: Signal<String>,
    /// The age typed for each spouse.
    ages: Signal<HashMap<Uuid, String>>,
    event_id: Signal<Option<Uuid>>,
}

/// The marriage shorthand, seeded once from the couple's events.
fn use_marriage_draft(
    events: Resource<Result<Connection<StoredEvent>, ApiError>>,
) -> MarriageDraft {
    let mut draft = MarriageDraft {
        parts: use_signal(DateParts::default),
        place_id: use_signal(String::new),
        desc: use_signal(String::new),
        ages: use_signal(HashMap::new),
        event_id: use_signal(|| None::<Uuid>),
    };
    let mut loaded = use_signal(|| false);
    if !loaded()
        && let Some(Ok(conn)) = &*events.read()
    {
        if let Some(ev) = conn
            .edges
            .iter()
            .map(|edge| &edge.node)
            .find(|ev| is_marriage_like(ev.event_type))
        {
            draft.event_id.set(Some(ev.id));
            draft.parts.set(DateParts::from_fields(
                ev.calendar,
                ev.date_qualifier,
                ev.date_value.as_deref(),
                ev.date_value2.as_deref(),
            ));
            draft
                .place_id
                .set(ev.place_id.map(|id| id.to_string()).unwrap_or_default());
            draft.desc.set(ev.description.clone().unwrap_or_default());
            draft.ages.set(typed_spouse_ages(&ev.spouse_ages));
        }
        loaded.set(true);
    }
    draft
}

impl MarriageDraft {
    /// Saves the shorthand: updates the marriage event, or creates one.
    fn save(
        self,
        scope: &FormScope,
        spouses: &[(Uuid, String)],
    ) -> impl FnMut(Event<MouseData>) + 'static {
        let scope = scope.clone();
        let spouses = spouses.to_vec();
        let mut event_id = self.event_id;
        move |_| {
            let scope = scope.clone();
            let parts = (self.parts)();
            let place = (self.place_id)();
            let desc = (self.desc)().trim().to_string();
            let ages =
                spouse_ages_from_form(&spouses, &self.ages.read()).map_err(|key| scope.i18n.t(key));
            let existing_id = event_id();
            spawn(async move {
                let saved = match ages {
                    Ok(ages) => {
                        save_marriage(&scope, existing_id, &parts, &place, &desc, ages).await
                    }
                    Err(error) => Err(error),
                };
                if let Some(Some(created)) = scope.writes.settle(saved) {
                    event_id.set(Some(created));
                }
            });
        }
    }
}

/// Writes the marriage shorthand, handing back the event it created, if any.
async fn save_marriage(
    scope: &FormScope,
    existing_id: Option<Uuid>,
    parts: &DateParts,
    place: &str,
    desc: &str,
    spouse_ages: Vec<SpouseAge>,
) -> Result<Option<Uuid>, String> {
    let place_id = scope.event_place(parts, place).await?;
    let saved = match existing_id {
        Some(eid) => {
            let body = update_event_body(
                Some(EventType::Marriage),
                parts,
                place_id,
                Some(opt_str(desc)),
                EventExtrasPatch {
                    spouse_ages: Some(spouse_ages),
                    ..Default::default()
                },
            );
            scope
                .api
                .update_event(scope.tid, eid, &body)
                .await
                .map(|_| None)
        }
        None => {
            let body = create_event_body(
                EventType::Marriage,
                parts,
                place_id,
                EventOwner::Family(scope.fid),
                opt_str(desc),
                EventExtras {
                    spouse_ages,
                    ..Default::default()
                },
            );
            scope
                .api
                .create_event(scope.tid, &body)
                .await
                .map(|ev| Some(ev.id))
        }
    };
    saved.map_err(|e| e.to_string())
}

/// The fields of a union event being added.
#[derive(Clone, Copy)]
struct NewUnionEvent {
    kind: Signal<String>,
    parts: Signal<DateParts>,
    place: Signal<String>,
    desc: Signal<String>,
    notes: Signal<String>,
    source: Signal<String>,
}

fn use_new_union_event() -> NewUnionEvent {
    NewUnionEvent {
        kind: use_signal(|| "Marriage".to_string()),
        parts: use_signal(DateParts::default),
        place: use_signal(String::new),
        desc: use_signal(String::new),
        notes: use_signal(String::new),
        source: use_signal(String::new),
    }
}

impl NewUnionEvent {
    /// Empties the fields for the next event, keeping its kind.
    fn clear(mut self) {
        self.parts.set(DateParts::default());
        self.place.set(String::new());
        self.desc.set(String::new());
        self.notes.set(String::new());
        self.source.set(String::new());
    }

    /// Creates the event, with its notes and source.
    fn create(
        self,
        scope: &FormScope,
        mut show_add: Signal<bool>,
    ) -> impl FnMut(Event<MouseData>) + 'static {
        let scope = scope.clone();
        move |_| {
            let scope = scope.clone();
            let kind = crate::utils::parse_event_type(&(self.kind)());
            let parts = (self.parts)();
            let place = (self.place)();
            let desc = (self.desc)().trim().to_string();
            let notes = (self.notes)().trim().to_string();
            let source = (self.source)();
            spawn(async move {
                let created = create_union_event(&scope, kind, &parts, &place, &desc).await;
                let Some(event_id) = scope.writes.settle(created) else {
                    return;
                };
                // Family events carry no person: their notes and source are
                // reached through the event itself.
                let _ = save_notes_source(
                    &scope.api,
                    scope.tid,
                    None,
                    Some(event_id),
                    &notes,
                    &source,
                    &NotesSource::default(),
                )
                .await;
                show_add.set(false);
                self.clear();
            });
        }
    }
}

/// Creates one union event, handing back its id.
async fn create_union_event(
    scope: &FormScope,
    kind: EventType,
    parts: &DateParts,
    place: &str,
    desc: &str,
) -> Result<Uuid, String> {
    let place_id = scope.event_place(parts, place).await?;
    let body = create_event_body(
        kind,
        parts,
        place_id,
        EventOwner::Family(scope.fid),
        opt_str(desc),
        EventExtras::default(),
    );
    scope
        .api
        .create_event(scope.tid, &body)
        .await
        .map(|ev| ev.id)
        .map_err(|e| e.to_string())
}

/// The union block: the marriage shorthand, the other events, and the form
/// adding one.
struct UnionEventsSection<'a> {
    scope: FormScope,
    /// The couple (person, name), whose ages a family event may give.
    spouses: Vec<(Uuid, String)>,
    events: &'a [StoredEvent],
    place_options: &'a [(String, String)],
    marriage: MarriageDraft,
    new_event: NewUnionEvent,
    show_add: Signal<bool>,
    open_event: Signal<Option<Uuid>>,
}

impl UnionEventsSection<'_> {
    fn render(&self) -> Element {
        let i18n = self.scope.i18n;
        let marriage_id = (self.marriage.event_id)();
        rsx! {
            if self.events.is_empty() && marriage_id.is_none() {
                div { class: "empty-state",
                    p { {i18n.t("union_form.no_events")} }
                }
            }
            // Primary union date/place/note shorthand (mapped to the marriage event).
            if marriage_id.is_some() || self.events.is_empty() {
                {self.marriage_shorthand()}
            }
            // Other union events (not the primary one)
            for evt in self.events.iter().filter(|evt| Some(evt.id) != marriage_id) {
                {self.event_row(evt)}
            }
            if (self.show_add)() {
                {self.add_form()}
            }
        }
    }

    fn marriage_shorthand(&self) -> Element {
        let i18n = self.scope.i18n;
        let MarriageDraft {
            parts,
            place_id,
            mut desc,
            ages,
            event_id,
        } = self.marriage;
        let save_key = if event_id().is_some() {
            "union_form.update_marriage"
        } else {
            "union_form.save_marriage"
        };
        rsx! {
            div { class: "pf-subform",
                div { class: "form-group",
                    label { {i18n.t("person_form.date")} }
                    DateInput { parts, i18n, on_change: move |()| {} }
                }
                {render_place_input(&i18n, place_id, self.place_options, || {})}
                div { class: "form-group",
                    label { {i18n.t("person_form.description")} }
                    input {
                        r#type: "text",
                        value: "{desc}",
                        oninput: move |e: Event<FormData>| desc.set(e.value()),
                    }
                }
                div { class: "form-row",
                    {render_spouse_age_fields(&i18n, &self.spouses, ages)}
                }
                button {
                    class: "pf-confirm-btn",
                    r#type: "button",
                    onclick: self.marriage.save(&self.scope, &self.spouses),
                    {i18n.t(save_key)}
                }
            }
        }
    }

    /// The name of an event's place, if it has one.
    fn place_name(&self, evt: &StoredEvent) -> String {
        let Some(id) = evt.place_id.map(|id| id.to_string()) else {
            return String::new();
        };
        self.place_options
            .iter()
            .find(|(pid, _)| *pid == id)
            .map(|(_, name)| name.clone())
            .unwrap_or_default()
    }

    fn event_row(&self, evt: &StoredEvent) -> Element {
        let i18n = self.scope.i18n;
        let eid = evt.id;
        let et = i18n.t(event_type_label_key(evt.event_type));
        let date = format_event_date(&i18n, evt);
        let desc = evt.description.clone().unwrap_or_default();
        // What the row says without opening it: a residence often has only
        // its place.
        let place = self.place_name(evt);
        let mut open_event = self.open_event;
        let open = open_event() == Some(eid);
        let writes = self.scope.writes;
        rsx! {
            div {
                class: if open { "person-form-item pf-ns-open" } else { "person-form-item" },
                div { class: "person-form-item-info",
                    span { class: "badge", "{et}" }
                    if !desc.is_empty() { span { "{desc}" } }
                    if !date.is_empty() { span { class: "text-muted", "{date}" } }
                    if !place.is_empty() { span { class: "text-muted", "@ {place}" } }
                }
                div { class: "person-form-item-actions",
                    button {
                        class: if open { "pf-row-btn is-active" } else { "pf-row-btn" },
                        r#type: "button",
                        onclick: move |_| open_event.set((!open).then_some(eid)),
                        {i18n.t("common.edit")}
                    }
                    button {
                        class: "pf-row-btn is-danger",
                        r#type: "button",
                        onclick: self.scope.delete_event(eid),
                        {i18n.t("common.remove")}
                    }
                }
            }
            if open {
                EventEditor {
                    tree_id: self.scope.tid,
                    person_id: None,
                    event: evt.clone(),
                    description_label: i18n.t("person_form.description"),
                    place_options: self.place_options.to_vec(),
                    spouses: self.spouses.clone(),
                    // Saved, the event folds back into its row.
                    on_saved: move |_| {
                        open_event.set(None);
                        writes.settle(Ok::<_, String>(()));
                    },
                }
            }
        }
    }

    fn add_form(&self) -> Element {
        let i18n = self.scope.i18n;
        let NewUnionEvent {
            mut kind,
            parts,
            place,
            mut desc,
            notes,
            source,
        } = self.new_event;
        rsx! {
            div { class: "pf-subform",
                div { class: "form-row",
                    div { class: "form-group",
                        label { {i18n.t("person_form.type")} }
                        select {
                            value: "{kind}",
                            oninput: move |e: Event<FormData>| kind.set(e.value()),
                            optgroup { label: "{i18n.t(\"union_form.core_events\")}",
                                for (value, key) in CORE_UNION_EVENTS {
                                    option { value, {i18n.t(key)} }
                                }
                            }
                            optgroup { label: "{i18n.t(\"union_form.optional_events\")}",
                                for (value, key) in OPTIONAL_UNION_EVENTS {
                                    option { value, {i18n.t(key)} }
                                }
                            }
                        }
                    }
                    div { class: "form-group",
                        label { {i18n.t("person_form.date")} }
                        DateInput { parts, i18n, on_change: move |()| {} }
                    }
                }
                div { class: "form-row",
                    {render_place_input(&i18n, place, self.place_options, || {})}
                    div { class: "form-group",
                        label { {i18n.t("person_form.description")} }
                        input {
                            r#type: "text",
                            value: "{desc}",
                            oninput: move |e: Event<FormData>| desc.set(e.value()),
                        }
                    }
                }
                {render_notes_source_fields(&i18n, self.scope.tid, notes, source, || {})}
                button {
                    class: "pf-confirm-btn",
                    r#type: "button",
                    onclick: self.new_event.create(&self.scope, self.show_add),
                    {i18n.t("person.create_event")}
                }
            }
        }
    }
}

/// The children block: linking one, and detaching them on Save.
struct ChildrenSection<'a> {
    scope: FormScope,
    names: &'a HashMap<Uuid, Vec<PersonName>>,
    show_add: Signal<bool>,
    pending_detach: Signal<HashSet<Uuid>>,
    confirm_detach: Signal<Option<Uuid>>,
}

impl ChildrenSection<'_> {
    fn render(&self, children: &Option<Result<Vec<FamilyChild>, ApiError>>) -> Element {
        let i18n = self.scope.i18n;
        let mut show_add = self.show_add;
        rsx! {
            if show_add() {
                div { class: "linking-panel",
                    p { class: "linking-panel-title", {i18n.t("union_form.link_or_create")} }
                    SearchPerson {
                        tree_id: self.scope.tid,
                        placeholder: i18n.t("union_form.search_child"),
                        on_select: self.scope.add_child(show_add),
                        on_cancel: move |_| show_add.set(false),
                    }
                }
            }
            match children {
                Some(Ok(children)) if children.is_empty() => rsx! {
                    div { class: "empty-state",
                        p { {i18n.t("union_form.no_children")} }
                    }
                },
                Some(Ok(children)) => rsx! {
                    for child in children.iter() {
                        {self.child_row(child)}
                    }
                },
                Some(Err(e)) => rsx! {
                    div { class: "error-msg", {i18n.t_args("union_form.load_children_error", &[("error", &e.to_string())])} }
                },
                None => rsx! {
                    div { class: "loading", {i18n.t("union_form.loading_children")} }
                },
            }
        }
    }

    /// A child's row, or the confirmation of their detachment.
    fn child_row(&self, child: &FamilyChild) -> Element {
        let i18n = self.scope.i18n;
        let cid = child.person_id;
        let name = resolve_name(cid, self.names, &i18n);
        let mut pending_detach = self.pending_detach;
        let mut confirm_detach = self.confirm_detach;
        if confirm_detach() == Some(cid) {
            return rsx! {
                div { class: "uf-child-detach-confirm",
                    p { {i18n.t_args("union_form.detach_confirm_title", &[("name", &name)])} }
                    p { {i18n.t_args("union_form.detach_confirm_message", &[("name", &name)])} }
                    div { class: "pf-delete-confirm-actions",
                        button {
                            class: "btn btn-outline btn-sm",
                            r#type: "button",
                            onclick: move |_| confirm_detach.set(None),
                            {i18n.t("common.cancel")}
                        }
                        button {
                            class: "btn btn-danger btn-sm",
                            r#type: "button",
                            onclick: move |_| {
                                pending_detach.write().insert(cid);
                                confirm_detach.set(None);
                            },
                            {i18n.t("union_form.detach_confirm_button")}
                        }
                    }
                }
            };
        }
        let ct = i18n.t(child_type_label_key(child.child_type));
        let is_pending = pending_detach().contains(&cid);
        rsx! {
            div { class: if is_pending { "uf-child-row pending-detach" } else { "uf-child-row" },
                div { class: "uf-child-avatar", "\u{1F464}" }
                div { class: "uf-child-info",
                    span { class: "badge", "{ct}" }
                    strong { "{name}" }
                }
                if is_pending {
                    button {
                        class: "btn btn-outline btn-sm",
                        r#type: "button",
                        onclick: move |_| {
                            pending_detach.write().remove(&cid);
                        },
                        {i18n.t("union_form.undo_detach")}
                    }
                } else {
                    button {
                        class: "btn btn-danger btn-sm",
                        r#type: "button",
                        onclick: move |_| confirm_detach.set(Some(cid)),
                        {i18n.t("union_form.detach_button")}
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn child(link: u128, person: u128) -> FamilyChild {
        serde_json::from_value(serde_json::json!({
            "id": Uuid::from_u128(link),
            "family_id": Uuid::from_u128(99),
            "person_id": Uuid::from_u128(person),
            "child_type": "biological",
            "sort_order": 0,
        }))
        .unwrap()
    }

    /// A detachment is staged by person, but the endpoint deletes a
    /// family-child link: sending the person's id answered 404.
    #[test]
    fn a_detachment_deletes_the_childs_link_not_the_person() {
        let children = [child(1, 10), child(2, 20), child(3, 30)];
        let staged = HashSet::from([Uuid::from_u128(20), Uuid::from_u128(30)]);

        assert_eq!(
            links_to_detach(&children, &staged),
            [Uuid::from_u128(2), Uuid::from_u128(3)]
        );
        assert!(links_to_detach(&children, &HashSet::new()).is_empty());
    }
}
