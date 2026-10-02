//! The merge wizard (`docs/ui-merge.md`): find the record describing the same
//! person, compare the two and choose the record kept and what to take from
//! the other, then confirm. One dialog for every "Merge with…": the
//! pedigree's action picker, the person page and the duplicates tool.

use std::collections::{HashMap, HashSet};

use dioxus::prelude::*;
use oxidgene_core::projection::PersonProfile;
use oxidgene_core::types::{Event as DomainEvent, PersonName, join_surname_particle};
use oxidgene_core::{DateQualifier, EventType, Sex};
use uuid::Uuid;

use crate::api::{ApiClient, CroppedSource, MediaWithLink, MergeChoices, PersonDetailBundle};
use crate::components::date_input::format_event_date;
use crate::components::modal::Modal;
use crate::components::search_person::{
    PersonSearchSummary, SearchPerson, render_person_search_summary,
};
use crate::i18n::{I18n, use_i18n};
use crate::ui_observability::{UiCommand, trace_ui_action, use_ui_resource};
use crate::utils::event_type_label_key;

/// Where the wizard stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Select,
    Compare,
    Confirm,
}

impl Step {
    fn number(self) -> usize {
        match self {
            Self::Select => 1,
            Self::Compare => 2,
            Self::Confirm => 3,
        }
    }

    fn label_key(self) -> &'static str {
        match self {
            Self::Select => "merge.step_select",
            Self::Compare => "merge.step_compare",
            Self::Confirm => "merge.step_confirm",
        }
    }
}

/// A row of the comparison whose value the user picks from either record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Field {
    Surname,
    GivenNames,
    Sex,
    /// The events of a type a person has once.
    Event(EventType),
}

/// Event types a person has once: a second one is the same event recorded
/// twice, so the comparison keeps one record's, never both.
const UNIQUE_EVENTS: [EventType; 5] = [
    EventType::Birth,
    EventType::Baptism,
    EventType::Death,
    EventType::Burial,
    EventType::Cremation,
];

fn is_unique(event_type: EventType) -> bool {
    UNIQUE_EVENTS.contains(&event_type)
}

/// A record as the wizard reads it: its projection, for the comparison, and
/// its detail bundle, for its names, own events and media.
#[derive(Clone)]
struct Record {
    profile: PersonProfile,
    bundle: PersonDetailBundle,
}

impl Record {
    fn id(&self) -> Uuid {
        self.profile.person_id
    }

    fn name(&self) -> String {
        self.profile
            .primary_name
            .as_ref()
            .map(|n| n.display_name.clone())
            .unwrap_or_default()
    }

    fn primary_name(&self) -> Option<&PersonName> {
        let own = || {
            self.bundle
                .names
                .iter()
                .filter(|n| n.person_id == self.id())
        };
        PersonName::primary(own())
    }

    /// The surname of the primary name, particle included.
    fn surname(&self) -> String {
        self.primary_name()
            .and_then(|n| {
                let root = n.surname.as_deref()?;
                Some(join_surname_particle(n.surname_prefix.as_deref(), root))
            })
            .unwrap_or_default()
    }

    fn given_names(&self) -> String {
        self.primary_name()
            .and_then(|n| n.given_names.clone())
            .unwrap_or_default()
    }

    /// The events that are the person's own, not a union's.
    fn own_events(&self) -> Vec<&DomainEvent> {
        self.bundle
            .events
            .iter()
            .filter(|e| e.person_id == Some(self.id()) && e.deleted_at.is_none())
            .collect()
    }

    fn events_of(&self, event_type: EventType) -> Vec<&DomainEvent> {
        self.own_events()
            .into_iter()
            .filter(|e| e.event_type == event_type)
            .collect()
    }

    /// The media linked to the person directly, not through a union.
    fn own_media(&self) -> Vec<&MediaWithLink> {
        self.bundle
            .profile_media
            .iter()
            .filter(|m| m.family_id.is_none())
            .map(|m| &m.tile)
            .collect()
    }

    fn place_name(&self, event: &DomainEvent) -> Option<String> {
        let id = event.place_id?;
        self.bundle
            .places
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.name.clone())
    }
}

/// How much an event says: the pieces of its date, whether that date is
/// exact, and whether it has a place.
fn richness(event: &DomainEvent) -> usize {
    let date_pieces = event.date_value.as_deref().map_or(0, |d| {
        d.split([' ', '-', '/']).filter(|p| !p.is_empty()).count()
    });
    2 * date_pieces
        + usize::from(event.date_qualifier == DateQualifier::Exact)
        + usize::from(event.place_id.is_some())
}

fn richest(events: &[&DomainEvent]) -> usize {
    events.iter().map(|e| richness(e)).max().unwrap_or(0)
}

/// The rows the user picks from, in table order: the name, the sex, then
/// each unique event type either record has.
fn fields(kept: &Record, absorbed: &Record) -> Vec<Field> {
    let mut fields = vec![Field::Surname, Field::GivenNames, Field::Sex];
    fields.extend(
        UNIQUE_EVENTS
            .into_iter()
            .filter(|t| !kept.events_of(*t).is_empty() || !absorbed.events_of(*t).is_empty())
            .map(Field::Event),
    );
    fields
}

/// Whether the row offers a choice: the absorbed record has something the
/// kept one could take instead. A sex only one record knows is not a choice
/// — the known one wins.
fn decidable(field: Field, kept: &Record, absorbed: &Record) -> bool {
    match field {
        Field::Surname => !absorbed.surname().is_empty() && absorbed.surname() != kept.surname(),
        Field::GivenNames => {
            !absorbed.given_names().is_empty() && absorbed.given_names() != kept.given_names()
        }
        Field::Sex => {
            let (a, b) = (kept.profile.sex, absorbed.profile.sex);
            a != b && a != Sex::Unknown && b != Sex::Unknown
        }
        Field::Event(t) => !absorbed.events_of(t).is_empty(),
    }
}

/// Whether the absorbed record's value is the one picked by default: when
/// the kept record has none, or, for an event, says less.
fn absorbed_by_default(field: Field, kept: &Record, absorbed: &Record) -> bool {
    match field {
        Field::Surname => kept.surname().is_empty(),
        Field::GivenNames => kept.given_names().is_empty(),
        Field::Sex => false,
        Field::Event(t) => richest(&absorbed.events_of(t)) > richest(&kept.events_of(t)),
    }
}

/// Whether the kept record's events already hold `event`: one of the same
/// type, date and place.
fn already_recorded(kept: &[&DomainEvent], event: &DomainEvent) -> bool {
    kept.iter().any(|mine| {
        mine.event_type == event.event_type
            && mine.date_value == event.date_value
            && mine.place_id == event.place_id
    })
}

/// The absorbed record's repeatable events: the unique ones are picked in
/// the comparison instead.
fn repeatable(events: Vec<&DomainEvent>) -> Vec<&DomainEvent> {
    events
        .into_iter()
        .filter(|e| !is_unique(e.event_type))
        .collect()
}

/// The absorbed repeatable events the kept ones lack, taken by default.
fn events_lacking(kept: &[&DomainEvent], absorbed: &[&DomainEvent]) -> HashSet<Uuid> {
    absorbed
        .iter()
        .filter(|e| !is_unique(e.event_type) && !already_recorded(kept, e))
        .map(|e| e.id)
        .collect()
}

/// What the user picks, all of it reset whenever the record kept changes.
#[derive(Clone, Default, PartialEq)]
struct Picks {
    /// The rows whose value comes from the absorbed record.
    from_absorbed: HashSet<Field>,
    /// The absorbed record's repeatable events taken.
    events: HashSet<Uuid>,
    /// The absorbed record's direct media links taken.
    media: HashSet<Uuid>,
}

/// The defaults: whatever the kept record lacks, and the richer of two
/// unique events.
fn default_picks(kept: &Record, absorbed: &Record) -> Picks {
    let kept_media: HashSet<Uuid> = kept.own_media().iter().map(|m| m.media.id).collect();
    Picks {
        from_absorbed: fields(kept, absorbed)
            .into_iter()
            .filter(|f| decidable(*f, kept, absorbed) && absorbed_by_default(*f, kept, absorbed))
            .collect(),
        events: events_lacking(&kept.own_events(), &absorbed.own_events()),
        media: absorbed
            .own_media()
            .into_iter()
            .filter(|m| !kept_media.contains(&m.media.id))
            .map(|m| m.link_id)
            .collect(),
    }
}

/// The merge request the picks make.
fn merge_choices(kept: &Record, absorbed: &Record, picks: &Picks) -> MergeChoices {
    let from_absorbed = |f: Field| picks.from_absorbed.contains(&f);
    let absorbed_left = absorbed.own_events().into_iter().filter(|e| {
        if is_unique(e.event_type) {
            !from_absorbed(Field::Event(e.event_type))
        } else {
            !picks.events.contains(&e.id)
        }
    });
    let kept_replaced = kept
        .own_events()
        .into_iter()
        .filter(|e| is_unique(e.event_type) && from_absorbed(Field::Event(e.event_type)));
    MergeChoices {
        left_out_events: absorbed_left.chain(kept_replaced).map(|e| e.id).collect(),
        left_out_media_links: absorbed
            .own_media()
            .iter()
            .map(|m| m.link_id)
            .filter(|id| !picks.media.contains(id))
            .collect(),
        surname_from_duplicate: from_absorbed(Field::Surname),
        given_names_from_duplicate: from_absorbed(Field::GivenNames),
        sex_from_duplicate: from_absorbed(Field::Sex),
    }
}

/// An event as its date and place.
fn event_facts(i18n: &I18n, record: &Record, event: &DomainEvent) -> String {
    [
        Some(format_event_date(i18n, event)),
        record.place_name(event),
        event.description.clone(),
    ]
    .into_iter()
    .flatten()
    .filter(|s| !s.is_empty())
    .collect::<Vec<_>>()
    .join(" \u{2014} ")
}

/// An event as a line: its type, then its facts.
fn event_line(i18n: &I18n, record: &Record, event: &DomainEvent) -> String {
    let facts = event_facts(i18n, record, event);
    let kind = i18n.t(event_type_label_key(event.event_type));
    if facts.is_empty() {
        kind
    } else {
        format!("{kind} \u{2014} {facts}")
    }
}

fn media_line(tile: &MediaWithLink) -> String {
    tile.media
        .title
        .clone()
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| tile.media.file_name.clone())
}

fn sex_label(i18n: &I18n, sex: Sex) -> String {
    i18n.t(match sex {
        Sex::Male => "sex.male",
        Sex::Female => "sex.female",
        Sex::Unknown => "sex.unknown",
    })
}

fn field_label(i18n: &I18n, field: Field) -> String {
    match field {
        Field::Surname => i18n.t("merge.field.surname"),
        Field::GivenNames => i18n.t("merge.field.given_names"),
        Field::Sex => i18n.t("merge.field.sex"),
        Field::Event(t) => i18n.t(event_type_label_key(t)),
    }
}

fn field_value(i18n: &I18n, field: Field, record: &Record) -> String {
    match field {
        Field::Surname => record.surname(),
        Field::GivenNames => record.given_names(),
        Field::Sex => sex_label(i18n, record.profile.sex),
        Field::Event(t) => record
            .events_of(t)
            .into_iter()
            .map(|e| {
                let facts = event_facts(i18n, record, e);
                if facts.is_empty() {
                    i18n.t("merge.no_details")
                } else {
                    facts
                }
            })
            .collect::<Vec<_>>()
            .join("; "),
    }
}

/// The rows the merge does not choose from, for reference: the relatives,
/// who all follow the record kept.
fn relative_rows(
    i18n: &I18n,
    a: &PersonProfile,
    b: &PersonProfile,
) -> Vec<(String, String, String)> {
    let row = |key: &str, of: &dyn Fn(&PersonProfile) -> String| {
        (i18n.t(&format!("merge.field.{key}")), of(a), of(b))
    };
    let parent = |p: &PersonProfile, father: bool| {
        p.family_as_child
            .as_ref()
            .and_then(|l| {
                if father {
                    l.father_display_name.clone()
                } else {
                    l.mother_display_name.clone()
                }
            })
            .unwrap_or_default()
    };
    vec![
        row("father", &|p| parent(p, true)),
        row("mother", &|p| parent(p, false)),
        row("spouses", &|p| {
            p.families_as_spouse
                .iter()
                .filter_map(|f| f.spouse_display_name.clone())
                .collect::<Vec<_>>()
                .join(", ")
        }),
        row("children", &|p| {
            p.families_as_spouse
                .iter()
                .map(|f| f.children_count)
                .sum::<u32>()
                .to_string()
        }),
    ]
}

/// The primary name the merged record will bear.
fn merged_name(kept: &Record, absorbed: &Record, picks: &Picks) -> String {
    let pick = |field: Field, of: fn(&Record) -> String| {
        if picks.from_absorbed.contains(&field) {
            of(absorbed)
        } else {
            of(kept)
        }
    };
    let given = pick(Field::GivenNames, Record::given_names);
    let surname = pick(Field::Surname, Record::surname);
    format!("{given} {surname}").trim().to_string()
}

/// The records to compare, loaded together.
async fn load_records(api: ApiClient, tree_id: Uuid, ids: [Uuid; 2]) -> Option<[Record; 2]> {
    let load = |id: Uuid| {
        let api = api.clone();
        async move {
            let profile = api.get_person_profile(tree_id, id).await.ok()?;
            let bundle = api.get_person_detail_bundle(tree_id, id).await.ok()?;
            Some(Record { profile, bundle })
        }
    };
    let (a, b) = futures_util::join!(load(ids[0]), load(ids[1]));
    Some([a?, b?])
}

/// The merge wizard. `other_id` given, it opens on the comparison; otherwise
/// on the search for the record to merge with `person_id`. `on_distinct`, when
/// set, offers to record the two as different people instead.
#[component]
pub fn MergeDialog(
    tree_id: Uuid,
    person_id: Uuid,
    #[props(default)] other_id: Option<Uuid>,
    on_close: EventHandler<()>,
    /// Called with the kept person once the merge is written.
    on_merged: EventHandler<Uuid>,
    #[props(default)] on_distinct: Option<EventHandler<()>>,
) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let mut other = use_signal(|| other_id);
    let mut step = use_signal(|| {
        if other_id.is_some() {
            Step::Compare
        } else {
            Step::Select
        }
    });
    let kept_is_source = use_signal(|| true);
    let mut picks = use_signal(Picks::default);
    // The absorbed record the picks were filled for.
    let mut defaults_for = use_signal(|| None::<Uuid>);
    let busy = use_signal(|| false);
    let error = use_signal(|| None::<String>);

    let api_records = api.clone();
    let records = use_ui_resource("merge_records", move || {
        let api = api_records.clone();
        let other = other();
        async move { load_records(api, tree_id, [person_id, other?]).await }
    });
    let api_portraits = api.clone();
    let portraits = use_ui_resource("merge_portraits", move || {
        let api = api_portraits.clone();
        let ids: Vec<Uuid> = [Some(person_id), other()].into_iter().flatten().collect();
        async move { api.portrait_map_for_ids(tree_id, &ids).await }
    });
    let portraits = portraits.read().clone().unwrap_or_default();

    let loaded = records.read().clone().flatten();
    let pair = loaded.as_ref().map(|[source, other]| {
        if kept_is_source() {
            (source.clone(), other.clone())
        } else {
            (other.clone(), source.clone())
        }
    });
    // Each record kept starts from its own defaults: what it lacks.
    if let Some((kept, absorbed)) = &pair
        && *defaults_for.peek() != Some(absorbed.id())
    {
        defaults_for.set(Some(absorbed.id()));
        picks.set(default_picks(kept, absorbed));
    }

    let current = step();
    let body = match (current, &pair) {
        (Step::Select, _) => rsx! {
            p { class: "tools-intro", {i18n.t("merge.select_hint")} }
            SearchPerson {
                tree_id,
                placeholder: i18n.t("merge.search"),
                on_select: move |id: Uuid| {
                    if id != person_id {
                        other.set(Some(id));
                        step.set(Step::Compare);
                    }
                },
            }
        },
        (_, None) => rsx! {
            p { class: "stats-loading", {i18n.t("common.loading")} }
        },
        (Step::Compare, Some((kept, absorbed))) => render_compare(CompareView {
            i18n,
            kept,
            absorbed,
            portraits: &portraits,
            kept_is_source,
            picks,
        }),
        (Step::Confirm, Some((kept, absorbed))) => {
            render_confirm(&i18n, kept, absorbed, &picks.read())
        }
    };
    let actions = render_actions(ActionsView {
        i18n,
        api,
        tree_id,
        step,
        searched: other_id.is_none(),
        request: pair.as_ref().map(|(kept, absorbed)| {
            (
                kept.id(),
                absorbed.id(),
                merge_choices(kept, absorbed, &picks.read()),
            )
        }),
        busy,
        error,
        on_close,
        on_merged,
        on_distinct,
    });

    rsx! {
        // Spread over three steps: a stray press on the backdrop must not
        // throw them away.
        Modal {
            class: "modal-card merge-card",
            label: i18n.t("merge.title"),
            busy: busy(),
            close_on_backdrop: false,
            on_close,
            h3 { {i18n.t("merge.title")} }
            p { class: "merge-step",
                {
                    i18n.t_args(
                        "merge.step",
                        &[
                            ("n", &current.number().to_string()),
                            ("label", &i18n.t(current.label_key())),
                        ],
                    )
                }
            }
            {body}
            if let Some(message) = error() {
                div { class: "error-msg", "{message}" }
            }
            {actions}
        }
    }
}

/// What step 2 draws.
struct CompareView<'a> {
    i18n: I18n,
    kept: &'a Record,
    absorbed: &'a Record,
    portraits: &'a HashMap<Uuid, CroppedSource>,
    kept_is_source: Signal<bool>,
    picks: Signal<Picks>,
}

/// Step 2: the two records side by side, the one kept, the value of each
/// row, and what to take from the other.
fn render_compare(view: CompareView<'_>) -> Element {
    let CompareView {
        i18n,
        kept,
        absorbed,
        portraits,
        kept_is_source,
        picks,
    } = view;
    // The source record first, whichever is kept.
    let (first, second) = if kept_is_source() {
        (kept, absorbed)
    } else {
        (absorbed, kept)
    };
    let relatives = relative_rows(&i18n, &first.profile, &second.profile);
    rsx! {
        {render_kept_choice(&i18n, [first, second], portraits, kept_is_source)}
        table { class: "stats-table merge-compare-table",
            tbody {
                for field in fields(kept, absorbed) {
                    {render_field_row(&i18n, field, [first, second], kept, absorbed, picks)}
                }
                for (k, (label, a, b)) in relatives.into_iter().enumerate() {
                    tr {
                        key: "relative-{k}",
                        class: if a != b { "merge-differs" } else { "" },
                        th { "{label}" }
                        td { "{a}" }
                        td { "{b}" }
                    }
                }
            }
        }
        p { class: "stats-note", {i18n.t("merge.pick_hint")} }
        {render_events(&i18n, kept, absorbed, picks)}
        {render_media(&i18n, absorbed, kept, picks)}
        p { class: "stats-note",
            {i18n.t_args("merge.moves_rest", &[("name", &absorbed.name())])}
        }
    }
}

/// The two cards, each with its "Keep this record" radio.
fn render_kept_choice(
    i18n: &I18n,
    records: [&Record; 2],
    portraits: &HashMap<Uuid, CroppedSource>,
    mut kept_is_source: Signal<bool>,
) -> Element {
    rsx! {
        div { class: "merge-persons",
            for (index, record) in records.into_iter().enumerate() {
                label { key: "{record.id()}", class: "merge-person",
                    input {
                        r#type: "radio",
                        name: "merge-kept",
                        checked: (index == 0) == kept_is_source(),
                        onchange: move |_| kept_is_source.set(index == 0),
                    }
                    span { class: "merge-person-keep", {i18n.t("merge.keep_this")} }
                    div { class: "search-person-result merge-person-card",
                        {
                            render_person_search_summary(
                                &PersonSearchSummary::from(record.profile.clone()),
                                portraits.get(&record.id()).cloned(),
                                i18n,
                            )
                        }
                    }
                }
            }
        }
    }
}

/// One row the user picks from: each record's value, with a radio when the
/// row is a choice.
fn render_field_row(
    i18n: &I18n,
    field: Field,
    columns: [&Record; 2],
    kept: &Record,
    absorbed: &Record,
    mut picks: Signal<Picks>,
) -> Element {
    let values = columns.map(|record| field_value(i18n, field, record));
    let choice = decidable(field, kept, absorbed);
    let absorbed_picked = picks.read().from_absorbed.contains(&field);
    let group = format!("merge-field-{field:?}");
    let differs = values[0] != values[1];
    rsx! {
        tr {
            key: "{group}",
            class: if differs { "merge-differs" } else { "" },
            th { {field_label(i18n, field)} }
            for (record, value) in columns.into_iter().zip(values) {
                td { key: "{record.id()}",
                    if choice {
                        {
                            let is_absorbed = record.id() == absorbed.id();
                            rsx! {
                                label { class: "merge-pick",
                                    input {
                                        r#type: "radio",
                                        name: "{group}",
                                        checked: absorbed_picked == is_absorbed,
                                        onchange: move |_| {
                                            let mut picks = picks.write();
                                            if is_absorbed {
                                                picks.from_absorbed.insert(field);
                                            } else {
                                                picks.from_absorbed.remove(&field);
                                            }
                                        },
                                    }
                                    span { if value.is_empty() { "\u{2014}" } else { "{value}" } }
                                }
                            }
                        }
                    } else {
                        "{value}"
                    }
                }
            }
        }
    }
}

/// One checkbox of a list: an item of the absorbed record, taken or left.
fn render_list_item(
    id: Uuid,
    line: String,
    already: bool,
    taken: bool,
    on_toggle: impl FnMut(bool) + 'static,
    i18n: &I18n,
) -> Element {
    let mut on_toggle = on_toggle;
    rsx! {
        label { key: "{id}", class: "merge-list-item",
            input {
                r#type: "checkbox",
                checked: taken,
                onchange: move |e: Event<FormData>| on_toggle(e.checked()),
            }
            span { "{line}" }
            if already {
                span { class: "merge-already", {i18n.t("merge.already")} }
            }
        }
    }
}

/// The absorbed record's repeatable events, each to take or leave.
fn render_events(
    i18n: &I18n,
    kept: &Record,
    absorbed: &Record,
    mut picks: Signal<Picks>,
) -> Element {
    let events = repeatable(absorbed.own_events());
    let kept_events = kept.own_events();
    let taken = picks.read().events.clone();
    rsx! {
        h4 { class: "merge-list-title",
            {i18n.t_args("merge.events_title", &[("name", &absorbed.name())])}
        }
        p { class: "stats-note", {i18n.t("merge.events_hint")} }
        if events.is_empty() {
            p { class: "stats-empty", {i18n.t("merge.none")} }
        }
        div { class: "merge-list",
            for event in events {
                {
                    let id = event.id;
                    render_list_item(
                        id,
                        event_line(i18n, absorbed, event),
                        already_recorded(&kept_events, event),
                        taken.contains(&id),
                        move |on| {
                            let mut picks = picks.write();
                            if on { picks.events.insert(id) } else { picks.events.remove(&id) };
                        },
                        i18n,
                    )
                }
            }
        }
    }
}

/// The absorbed record's own media, each to attach or leave.
fn render_media(
    i18n: &I18n,
    absorbed: &Record,
    kept: &Record,
    mut picks: Signal<Picks>,
) -> Element {
    let media = absorbed.own_media();
    let kept_media: HashSet<Uuid> = kept.own_media().iter().map(|m| m.media.id).collect();
    let taken = picks.read().media.clone();
    rsx! {
        h4 { class: "merge-list-title",
            {i18n.t_args("merge.media_title", &[("name", &absorbed.name())])}
        }
        p { class: "stats-note", {i18n.t("merge.media_hint")} }
        if media.is_empty() {
            p { class: "stats-empty", {i18n.t("merge.none")} }
        }
        div { class: "merge-list",
            for tile in media {
                {
                    let id = tile.link_id;
                    render_list_item(
                        id,
                        media_line(tile),
                        kept_media.contains(&tile.media.id),
                        taken.contains(&id),
                        move |on| {
                            let mut picks = picks.write();
                            if on { picks.media.insert(id) } else { picks.media.remove(&id) };
                        },
                        i18n,
                    )
                }
            }
        }
    }
}

/// Counts of events and media, as one phrase.
fn counts(i18n: &I18n, events: usize, media: usize) -> String {
    format!(
        "{}, {}",
        i18n.t_plural("merge.events_count", events),
        i18n.t_plural("merge.media_count", media)
    )
}

/// Step 3: what the merge will do.
fn render_confirm(i18n: &I18n, kept: &Record, absorbed: &Record, picks: &Picks) -> Element {
    let choices = merge_choices(kept, absorbed, picks);
    let left_out: HashSet<Uuid> = choices.left_out_events.iter().copied().collect();
    let events_taken = absorbed
        .own_events()
        .iter()
        .filter(|e| !left_out.contains(&e.id))
        .count();
    let media_left = choices.left_out_media_links.len();
    let media_taken = absorbed.own_media().len() - media_left;
    let absorbed_name = absorbed.name();
    rsx! {
        ul { class: "merge-summary",
            li { {i18n.t_args("merge.summary_kept", &[("name", &kept.name())])} }
            li {
                {
                    i18n.t_args(
                        "merge.summary_name",
                        &[("name", &merged_name(kept, absorbed, picks))],
                    )
                }
            }
            li {
                {
                    i18n.t_args(
                        "merge.summary_taken",
                        &[
                            ("name", &absorbed_name),
                            ("items", &counts(i18n, events_taken, media_taken)),
                        ],
                    )
                }
            }
            li {
                {
                    i18n.t_args(
                        "merge.summary_left",
                        &[("items", &counts(i18n, left_out.len(), media_left))],
                    )
                }
            }
        }
        p { class: "homonym-warning",
            {
                i18n.t_args(
                    "merge.warning",
                    &[("name", &absorbed_name), ("kept", &kept.name())],
                )
            }
        }
    }
}

/// What the footer acts on.
struct ActionsView {
    i18n: I18n,
    api: ApiClient,
    tree_id: Uuid,
    step: Signal<Step>,
    /// The other record was searched for, so step 2 can go back to step 1.
    searched: bool,
    /// The kept record, the absorbed one, and the merge the picks make.
    request: Option<(Uuid, Uuid, MergeChoices)>,
    busy: Signal<bool>,
    error: Signal<Option<String>>,
    on_close: EventHandler<()>,
    on_merged: EventHandler<Uuid>,
    on_distinct: Option<EventHandler<()>>,
}

/// The footer: back, cancel, and the step's own action.
fn render_actions(view: ActionsView) -> Element {
    let ActionsView {
        i18n,
        api,
        tree_id,
        mut step,
        searched,
        request,
        mut busy,
        mut error,
        on_close,
        on_merged,
        on_distinct,
    } = view;
    let current = step();
    let back = match current {
        Step::Confirm => Some(Step::Compare),
        Step::Compare if searched => Some(Step::Select),
        _ => None,
    };
    let ids = request
        .as_ref()
        .map(|(kept, absorbed, _)| (*kept, *absorbed));
    let api_distinct = api.clone();
    let merge = move |_| {
        let Some((kept, absorbed, choices)) = request.clone() else {
            return;
        };
        let api = api.clone();
        spawn(async move {
            busy.set(true);
            error.set(None);
            let merged = api.merge_persons(tree_id, kept, absorbed, &choices);
            match trace_ui_action(UiCommand::Merge, merged).await {
                Ok(_) => on_merged.call(kept),
                Err(_) => error.set(Some(i18n.t("homonym.merge_failed"))),
            }
            busy.set(false);
        });
    };
    let distinct = move |_| {
        let (Some((kept, absorbed)), Some(on_distinct)) = (ids, on_distinct) else {
            return;
        };
        let api = api_distinct.clone();
        spawn(async move {
            busy.set(true);
            error.set(None);
            let apart = [kept];
            let marked = api.mark_persons_distinct(tree_id, absorbed, &apart);
            match trace_ui_action(UiCommand::MarkDistinct, marked).await {
                Ok(()) => on_distinct.call(()),
                Err(_) => error.set(Some(i18n.t("homonym.distinct_failed"))),
            }
            busy.set(false);
        });
    };
    rsx! {
        div { class: "modal-actions",
            if current == Step::Compare && on_distinct.is_some() {
                button {
                    class: "btn btn-outline",
                    disabled: busy(),
                    onclick: distinct,
                    {i18n.t("tools.duplicates.not_duplicates")}
                }
            }
            if let Some(target) = back {
                button {
                    class: "btn btn-outline",
                    disabled: busy(),
                    onclick: move |_| step.set(target),
                    {i18n.t("common.back")}
                }
            }
            button {
                class: "btn btn-outline",
                disabled: busy(),
                onclick: move |_| on_close.call(()),
                {i18n.t("common.cancel")}
            }
            if current == Step::Compare {
                button {
                    class: "btn btn-primary",
                    disabled: busy() || ids.is_none(),
                    onclick: move |_| step.set(Step::Confirm),
                    {i18n.t("merge.next")}
                }
            }
            if current == Step::Confirm {
                button {
                    class: "btn btn-danger",
                    disabled: busy(),
                    onclick: merge,
                    if busy() {
                        span { class: "btn-spinner" }
                    }
                    {i18n.t("homonym.merge")}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use oxidgene_core::Calendar;

    use super::*;

    fn event(event_type: EventType, date: &str, place: Option<Uuid>) -> DomainEvent {
        let now = chrono::Utc::now();
        DomainEvent {
            id: Uuid::now_v7(),
            tree_id: Uuid::nil(),
            event_type,
            date_value: Some(date.to_string()),
            date_sort: None,
            date_qualifier: DateQualifier::Exact,
            date_value2: None,
            calendar: Calendar::Gregorian,
            cause: None,
            age: None,
            agency: None,
            spouse_ages: Vec::new(),
            place_id: place,
            person_id: None,
            family_id: None,
            description: None,
            created_at: now,
            updated_at: now,
            deleted_at: None,
        }
    }

    /// A full date with its place says more than a year alone: of two
    /// births, the richer one is picked by default.
    #[test]
    fn the_richer_of_two_unique_events_says_more() {
        let full = event(EventType::Birth, "12 MAR 1842", Some(Uuid::now_v7()));
        let year = event(EventType::Birth, "1842", None);
        assert!(richness(&full) > richness(&year));
        assert_eq!(richest(&[]), 0);
    }

    /// A repeatable event is the same only with the same date and place;
    /// otherwise it is taken. Unique events are never in the list: the
    /// comparison picks them.
    #[test]
    fn what_the_kept_record_lacks_is_taken_by_default() {
        let place = Some(Uuid::now_v7());
        let kept = [event(EventType::Occupation, "1870", place)];
        let absorbed = [
            event(EventType::Occupation, "1870", place),
            event(EventType::Occupation, "1880", place),
            event(EventType::Residence, "1890", None),
            event(EventType::Death, "1918", None),
        ];
        let kept: Vec<&DomainEvent> = kept.iter().collect();
        let absorbed: Vec<&DomainEvent> = absorbed.iter().collect();
        let taken = events_lacking(&kept, &absorbed);
        assert_eq!(taken, HashSet::from([absorbed[1].id, absorbed[2].id]));
        assert_eq!(repeatable(absorbed).len(), 3);
    }
}
