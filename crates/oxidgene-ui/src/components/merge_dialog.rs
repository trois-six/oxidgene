//! The merge wizard (`docs/ui-merge.md`): find the record describing the same
//! person, compare the two and choose the record kept and what to take from
//! the other, then confirm. One dialog for every "Merge with…": the
//! pedigree's action picker, the person page and the duplicates tool.

use std::collections::{HashMap, HashSet};

use dioxus::prelude::*;
use oxidgene_core::projection::{PersonProfile, ProfileEvent};
use oxidgene_core::types::Event as DomainEvent;
use oxidgene_core::{EventType, Sex};
use uuid::Uuid;

use crate::api::{ApiClient, CroppedSource, MediaWithLink, PersonDetailBundle};
use crate::components::date_input::{format_date, format_event_date};
use crate::components::search_person::{
    PersonSearchSummary, SearchPerson, render_person_search_summary,
};
use crate::i18n::{I18n, use_i18n};
use crate::ui_observability::use_ui_resource;
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

/// A record as the wizard reads it: its projection, for the comparison, and
/// its detail bundle, for its own events and media.
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

    /// The events that are the person's own, not a union's.
    fn own_events(&self) -> Vec<&DomainEvent> {
        self.bundle
            .events
            .iter()
            .filter(|e| e.person_id == Some(self.id()) && e.deleted_at.is_none())
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

/// Event types a person has once: a second one is the same event recorded
/// twice, whatever its date.
const UNIQUE_EVENTS: [EventType; 5] = [
    EventType::Birth,
    EventType::Baptism,
    EventType::Death,
    EventType::Burial,
    EventType::Cremation,
];

/// Whether the kept record's events already hold `event`: an event of a
/// unique type, or one of the same type, date and place.
fn already_recorded(kept: &[&DomainEvent], event: &DomainEvent) -> bool {
    kept.iter().any(|mine| {
        mine.event_type == event.event_type
            && (UNIQUE_EVENTS.contains(&event.event_type)
                || (mine.date_value == event.date_value && mine.place_id == event.place_id))
    })
}

/// The absorbed events the kept ones lack, taken by default.
fn events_lacking(kept: &[&DomainEvent], absorbed: &[&DomainEvent]) -> HashSet<Uuid> {
    absorbed
        .iter()
        .filter(|e| !already_recorded(kept, e))
        .map(|e| e.id)
        .collect()
}

/// The events and media taken by default: everything the kept record lacks.
fn default_taken(kept: &Record, absorbed: &Record) -> (HashSet<Uuid>, HashSet<Uuid>) {
    let kept_media: HashSet<Uuid> = kept.own_media().iter().map(|m| m.media.id).collect();
    let events = events_lacking(&kept.own_events(), &absorbed.own_events());
    let media = absorbed
        .own_media()
        .into_iter()
        .filter(|m| !kept_media.contains(&m.media.id))
        .map(|m| m.link_id)
        .collect();
    (events, media)
}

/// An event as a line: its type, date, place and description.
fn event_line(i18n: &I18n, record: &Record, event: &DomainEvent) -> String {
    let mut parts = vec![i18n.t(event_type_label_key(event.event_type))];
    parts.extend(
        [
            Some(format_event_date(i18n, event)),
            record.place_name(event).map(|p| format!("@ {p}")),
            event.description.clone(),
        ]
        .into_iter()
        .flatten()
        .filter(|s| !s.is_empty()),
    );
    parts.join(" \u{2014} ")
}

fn media_line(tile: &MediaWithLink) -> String {
    tile.media
        .title
        .clone()
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| tile.media.file_name.clone())
}

fn profile_date(i18n: &I18n, event: Option<&ProfileEvent>) -> String {
    event
        .map(|e| {
            format_date(
                i18n,
                e.calendar,
                e.date_qualifier,
                e.date_value.as_deref(),
                e.date_value2.as_deref(),
            )
        })
        .unwrap_or_default()
}

fn sex_label(i18n: &I18n, sex: Sex) -> String {
    i18n.t(match sex {
        Sex::Male => "sex.male",
        Sex::Female => "sex.female",
        Sex::Unknown => "sex.unknown",
    })
}

/// One row of the comparison per field: label, then each record's value.
fn compare_rows(
    i18n: &I18n,
    a: &PersonProfile,
    b: &PersonProfile,
) -> Vec<(String, String, String)> {
    let field = |key: &str, of: &dyn Fn(&PersonProfile) -> String| {
        (i18n.t(&format!("merge.field.{key}")), of(a), of(b))
    };
    let name = |p: &PersonProfile, surname: bool| {
        p.primary_name
            .as_ref()
            .and_then(|n| {
                if surname {
                    n.surname.clone()
                } else {
                    n.given_names.clone()
                }
            })
            .unwrap_or_default()
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
        field("surname", &|p| name(p, true)),
        field("given_names", &|p| name(p, false)),
        field("sex", &|p| sex_label(i18n, p.sex)),
        field("birth", &|p| profile_date(i18n, p.birth_or_baptism())),
        field("birth_place", &|p| {
            p.birth_or_baptism()
                .and_then(|e| e.place_name.clone())
                .unwrap_or_default()
        }),
        field("death", &|p| profile_date(i18n, p.death_or_burial())),
        field("father", &|p| parent(p, true)),
        field("mother", &|p| parent(p, false)),
        field("spouses", &|p| {
            p.families_as_spouse
                .iter()
                .filter_map(|f| f.spouse_display_name.clone())
                .collect::<Vec<_>>()
                .join(", ")
        }),
        field("children", &|p| {
            p.families_as_spouse
                .iter()
                .map(|f| f.children_count)
                .sum::<u32>()
                .to_string()
        }),
    ]
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
    let mut taken_events = use_signal(HashSet::<Uuid>::new);
    let mut taken_media = use_signal(HashSet::<Uuid>::new);
    // The absorbed record the taken sets were filled for.
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
        let (events, media) = default_taken(kept, absorbed);
        taken_events.set(events);
        taken_media.set(media);
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
        (_, None) => rsx! { p { class: "stats-loading", {i18n.t("common.loading")} } },
        (Step::Compare, Some((kept, absorbed))) => render_compare(CompareView {
            i18n,
            kept,
            absorbed,
            portraits: &portraits,
            kept_is_source,
            taken_events,
            taken_media,
        }),
        (Step::Confirm, Some((kept, absorbed))) => {
            render_confirm(&i18n, kept, absorbed, &taken_events(), &taken_media())
        }
    };
    let actions = render_actions(ActionsView {
        i18n,
        api,
        tree_id,
        step,
        searched: other_id.is_none(),
        pair: pair.clone(),
        taken_events,
        taken_media,
        busy,
        error,
        on_close,
        on_merged,
        on_distinct,
    });

    rsx! {
        div { class: "modal-backdrop",
            div {
                class: "modal-card merge-card",
                role: "dialog",
                "aria-modal": "true",
                onclick: move |e| e.stop_propagation(),
                h3 { {i18n.t("merge.title")} }
                p { class: "merge-step",
                    {i18n.t_args("merge.step", &[
                        ("n", &current.number().to_string()),
                        ("label", &i18n.t(current.label_key())),
                    ])}
                }
                {body}
                if let Some(message) = error() {
                    div { class: "error-msg", "{message}" }
                }
                {actions}
            }
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
    taken_events: Signal<HashSet<Uuid>>,
    taken_media: Signal<HashSet<Uuid>>,
}

/// Step 2: the two records side by side, the one kept, and what to take
/// from the other.
fn render_compare(view: CompareView<'_>) -> Element {
    let CompareView {
        i18n,
        kept,
        absorbed,
        portraits,
        mut kept_is_source,
        taken_events,
        taken_media,
    } = view;
    // The source record first, whichever is kept.
    let (first, second) = if kept_is_source() {
        (kept, absorbed)
    } else {
        (absorbed, kept)
    };
    let rows = compare_rows(&i18n, &first.profile, &second.profile);
    rsx! {
        div { class: "merge-persons",
            for (index, record) in [first, second].into_iter().enumerate() {
                label { key: "{record.id()}", class: "merge-person",
                    input {
                        r#type: "radio",
                        name: "merge-kept",
                        checked: (index == 0) == kept_is_source(),
                        onchange: move |_| kept_is_source.set(index == 0),
                    }
                    span { class: "merge-person-keep", {i18n.t("merge.keep_this")} }
                    div { class: "search-person-result merge-person-card",
                        {render_person_search_summary(
                            &PersonSearchSummary::from(record.profile.clone()),
                            portraits.get(&record.id()).cloned(),
                            &i18n,
                        )}
                    }
                }
            }
        }
        table { class: "stats-table merge-compare-table",
            tbody {
                for (k, (label, a, b)) in rows.into_iter().enumerate() {
                    tr { key: "{k}", class: if a != b { "merge-differs" } else { "" },
                        th { "{label}" }
                        td { "{a}" }
                        td { "{b}" }
                    }
                }
            }
        }
        {render_events(&i18n, kept, absorbed, taken_events)}
        {render_media(&i18n, absorbed, kept, taken_media)}
        p { class: "stats-note", {i18n.t_args("merge.moves_rest", &[("name", &absorbed.name())])} }
    }
}

/// The absorbed record's own events, each to take or leave.
fn render_events(
    i18n: &I18n,
    kept: &Record,
    absorbed: &Record,
    mut taken: Signal<HashSet<Uuid>>,
) -> Element {
    let events = absorbed.own_events();
    let kept_events = kept.own_events();
    rsx! {
        h4 { class: "merge-list-title", {i18n.t_args("merge.events_title", &[("name", &absorbed.name())])} }
        p { class: "stats-note", {i18n.t("merge.events_hint")} }
        if events.is_empty() {
            p { class: "stats-empty", {i18n.t("merge.none")} }
        }
        div { class: "merge-list",
            for event in events {
                {
                    let id = event.id;
                    let already = already_recorded(&kept_events, event);
                    rsx! {
                        label { key: "{id}", class: "merge-list-item",
                            input {
                                r#type: "checkbox",
                                checked: taken().contains(&id),
                                onchange: move |e: Event<FormData>| {
                                    let mut set = taken.write();
                                    if e.checked() { set.insert(id); } else { set.remove(&id); }
                                },
                            }
                            span { {event_line(i18n, absorbed, event)} }
                            if already {
                                span { class: "merge-already", {i18n.t("merge.already")} }
                            }
                        }
                    }
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
    mut taken: Signal<HashSet<Uuid>>,
) -> Element {
    let media = absorbed.own_media();
    let kept_media: HashSet<Uuid> = kept.own_media().iter().map(|m| m.media.id).collect();
    rsx! {
        h4 { class: "merge-list-title", {i18n.t_args("merge.media_title", &[("name", &absorbed.name())])} }
        p { class: "stats-note", {i18n.t("merge.media_hint")} }
        if media.is_empty() {
            p { class: "stats-empty", {i18n.t("merge.none")} }
        }
        div { class: "merge-list",
            for tile in media {
                {
                    let id = tile.link_id;
                    let already = kept_media.contains(&tile.media.id);
                    rsx! {
                        label { key: "{id}", class: "merge-list-item",
                            input {
                                r#type: "checkbox",
                                checked: taken().contains(&id),
                                onchange: move |e: Event<FormData>| {
                                    let mut set = taken.write();
                                    if e.checked() { set.insert(id); } else { set.remove(&id); }
                                },
                            }
                            span { {media_line(tile)} }
                            if already {
                                span { class: "merge-already", {i18n.t("merge.already")} }
                            }
                        }
                    }
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
fn render_confirm(
    i18n: &I18n,
    kept: &Record,
    absorbed: &Record,
    taken_events: &HashSet<Uuid>,
    taken_media: &HashSet<Uuid>,
) -> Element {
    let events = absorbed.own_events();
    let media = absorbed.own_media();
    let events_taken = events
        .iter()
        .filter(|e| taken_events.contains(&e.id))
        .count();
    let media_taken = media
        .iter()
        .filter(|m| taken_media.contains(&m.link_id))
        .count();
    let absorbed_name = absorbed.name();
    rsx! {
        ul { class: "merge-summary",
            li { {i18n.t_args("merge.summary_kept", &[("name", &kept.name())])} }
            li {
                {i18n.t_args("merge.summary_taken", &[
                    ("name", &absorbed_name),
                    ("items", &counts(i18n, events_taken, media_taken)),
                ])}
            }
            li {
                {i18n.t_args("merge.summary_left", &[
                    ("items", &counts(i18n, events.len() - events_taken, media.len() - media_taken)),
                ])}
            }
        }
        p { class: "homonym-warning",
            {i18n.t_args("merge.warning", &[("name", &absorbed_name), ("kept", &kept.name())])}
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
    pair: Option<(Record, Record)>,
    taken_events: Signal<HashSet<Uuid>>,
    taken_media: Signal<HashSet<Uuid>>,
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
        pair,
        taken_events,
        taken_media,
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
    let ids = pair
        .as_ref()
        .map(|(kept, absorbed)| (kept.id(), absorbed.id()));
    let left_out = pair.as_ref().map(|(_, absorbed)| {
        let events: Vec<Uuid> = absorbed
            .own_events()
            .iter()
            .map(|e| e.id)
            .filter(|id| !taken_events().contains(id))
            .collect();
        let links: Vec<Uuid> = absorbed
            .own_media()
            .iter()
            .map(|m| m.link_id)
            .filter(|id| !taken_media().contains(id))
            .collect();
        (events, links)
    });
    let api_distinct = api.clone();
    let merge = move |_| {
        let (Some((kept, absorbed)), Some((events, links))) = (ids, left_out.clone()) else {
            return;
        };
        let api = api.clone();
        spawn(async move {
            busy.set(true);
            error.set(None);
            match api
                .merge_persons(tree_id, kept, absorbed, &events, &links)
                .await
            {
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
            match api.mark_persons_distinct(tree_id, absorbed, &[kept]).await {
                Ok(()) => on_distinct.call(()),
                Err(_) => error.set(Some(i18n.t("homonym.distinct_failed"))),
            }
            busy.set(false);
        });
    };
    rsx! {
        div { class: "modal-actions",
            if current == Step::Compare && on_distinct.is_some() {
                button { class: "btn btn-outline", disabled: busy(), onclick: distinct,
                    {i18n.t("tools.duplicates.not_duplicates")}
                }
            }
            if let Some(target) = back {
                button { class: "btn btn-outline", disabled: busy(), onclick: move |_| step.set(target),
                    {i18n.t("common.back")}
                }
            }
            button { class: "btn btn-outline", disabled: busy(), onclick: move |_| on_close.call(()),
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
                button { class: "btn btn-danger", disabled: busy(), onclick: merge,
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
    use oxidgene_core::{Calendar, DateQualifier};

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
            place_id: place,
            person_id: None,
            family_id: None,
            description: None,
            created_at: now,
            updated_at: now,
            deleted_at: None,
        }
    }

    /// A person is born once: the other record's birth, whatever its date,
    /// is the same event recorded twice and is left out by default.
    #[test]
    fn a_second_birth_is_not_taken_by_default() {
        let kept = [event(EventType::Birth, "1842-03-12", None)];
        let absorbed = [event(EventType::Birth, "1842", None)];
        let kept: Vec<&DomainEvent> = kept.iter().collect();
        let absorbed: Vec<&DomainEvent> = absorbed.iter().collect();
        assert!(already_recorded(&kept, absorbed[0]));
        assert!(events_lacking(&kept, &absorbed).is_empty());
    }

    /// A repeatable event is the same only with the same date and place;
    /// otherwise it is taken, as is any type the kept record lacks.
    #[test]
    fn what_the_kept_record_lacks_is_taken_by_default() {
        let place = Some(Uuid::now_v7());
        let kept = [event(EventType::Occupation, "1870", place)];
        let absorbed = [
            event(EventType::Occupation, "1870", place),
            event(EventType::Occupation, "1880", place),
            event(EventType::Death, "1918", None),
        ];
        let kept: Vec<&DomainEvent> = kept.iter().collect();
        let absorbed: Vec<&DomainEvent> = absorbed.iter().collect();
        let taken = events_lacking(&kept, &absorbed);
        assert_eq!(taken, HashSet::from([absorbed[1].id, absorbed[2].id]));
    }
}
