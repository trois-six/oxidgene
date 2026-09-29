//! The sections of a person's profile, shared by the person page and the
//! couple page.
//!
//! [`build_profile`] turns one person's detail bundle into everything the
//! sections draw, once per load; the `*_section` functions render one card
//! each from it. The person page stacks every section of one profile. The
//! couple page lays the same sections of two profiles side by side, leaves the
//! union they share out of both columns, and draws it once across the two.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::NaiveDate;
use dioxus::prelude::*;
use oxidgene_core::Sex;
use oxidgene_core::enums::{Calendar, DateQualifier, EventType, SpouseRole};
use oxidgene_core::projection::Pedigree;
use oxidgene_core::types::{
    Event as DomainEvent, Note, Person, PersonName, QualifiedYear, Tree, Vignette,
};
use uuid::Uuid;

use crate::api::{
    ApiClient, ApiError, CroppedSource, GalleryBundle, MediaWithLink, PersonDetailBundle,
};
use crate::components::cropped_image::CroppedImage;
use crate::components::date_input::{DateKind, DatePhrase, event_date_phrase, format_event_date};
use crate::components::document_form::DocumentForm;
use crate::components::media_gallery::{MediaEventLinkOption, MediaGallery, MediaOwner};
use crate::components::pedigree_chart::SharedPedigree;
use crate::components::reference_tooltip::{GivenNamesHover, OccupationsHover};
use crate::components::tree_cache::{fetch_tree_cached, use_tree_cache};
use crate::i18n::I18n;
use crate::router::Route;
use crate::ui_observability::{UiLoadTrace, use_traced_resource};
use crate::utils::{event_type_label_key, note_html_for_display, opt_str, resolve_name};

// ── Derived model ────────────────────────────────────────────────────────

/// Indicates the origin of an event relative to the displayed person.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum EventOrigin {
    /// Event directly attached to this person (birth, death, occupation…).
    Individual,
    /// Event from a conjugal family (marriage, divorce…).
    ConjugalFamily,
    /// Event from a child (birth, death, baptism, burial of a child).
    ChildFamily,
    /// Event from the parental family (parent death, sibling birth…).
    ParentalFamily,
}

/// An event enriched with origin metadata for display purposes.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EnrichedEvent {
    pub event: DomainEvent,
    pub origin: EventOrigin,
    /// Optional context label (e.g. spouse name, sibling name).
    pub context: Option<String>,
    /// The person's own union the event belongs to: that family's own events
    /// and its children's. The couple page draws these once, for both spouses.
    pub union_id: Option<Uuid>,
}

/// One of a person's own unions: partner(s), their role in it,
/// marriage/divorce info, and the children born into it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct UnionGroup {
    pub family_id: Uuid,
    /// The union's earliest dated event, which orders the unions.
    pub sort_date: Option<NaiveDate>,
    pub partner_ids: Vec<Uuid>,
    pub role: SpouseRole,
    pub marriage_date: Option<DatePhrase>,
    pub marriage_place: Option<String>,
    pub divorce_date: Option<DatePhrase>,
    pub child_ids: Vec<Uuid>,
}

/// A group of half-siblings sharing one parent with this person, born from
/// that parent's union with someone other than this person's other parent.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SiblingGroup {
    pub common_parent_id: Uuid,
    pub other_parent_id: Option<Uuid>,
    pub child_ids: Vec<Uuid>,
}

/// The family narrative: parents, own unions, full siblings, half-siblings.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct FamilyNarrative {
    pub parent_ids: Vec<Uuid>,
    pub unions: Vec<UnionGroup>,
    pub full_sibling_ids: Vec<Uuid>,
    pub half_sibling_groups: Vec<SiblingGroup>,
}

/// One clause of the birth/death vitals sentence — kept structured (rather
/// than a flat formatted string) so the date/age can be rendered in bold.
pub(crate) enum VitalClause {
    Event {
        event_type: EventType,
        date: DatePhrase,
        place: Option<String>,
    },
    Age(AgeSpan),
    Occupation(Vec<String>),
}

/// The header's name, split so only the given name itself becomes hoverable
/// while the rendered text stays identical to `display_name`.
#[derive(Default)]
pub(crate) struct HeaderName {
    pub display_name: String,
    pub prefix: Option<String>,
    pub given: Option<String>,
    pub rest: String,
    /// Alternate names shown under the header name, e.g. "(Given Surname)".
    pub alt_names: Vec<String>,
}

/// Everything the profile sections of one person draw, derived once per load.
pub(crate) struct Profile {
    pub person_id: Uuid,
    pub person: Option<Person>,
    pub sosa_number: Option<u64>,
    pub name: HeaderName,
    pub vitals: Vec<VitalClause>,
    pub family: FamilyNarrative,
    /// Chronological: individual, conjugal, child and parental events.
    pub events: Vec<EnrichedEvent>,
    /// Every name in the bundle, keyed by person.
    pub names: HashMap<Uuid, Vec<PersonName>>,
    pub places: HashMap<Uuid, String>,
    pub sexes: HashMap<Uuid, Sex>,
    /// "birth-death" suffixes, matching the pedigree cards.
    pub lifespans: HashMap<Uuid, String>,
    /// "Source title — page", one entry per citation, keyed by event.
    pub citations_by_event: HashMap<Uuid, Vec<String>>,
    /// The documents proving each event, keyed by the event they document.
    pub evidence_by_event: HashMap<Uuid, Vec<MediaWithLink>>,
    pub bundle: Arc<PersonDetailBundle>,
}

impl Profile {
    pub fn name_of(&self, person_id: Uuid, i18n: &I18n) -> String {
        resolve_name(person_id, &self.names, i18n)
    }

    pub fn place_name(&self, place_id: Uuid) -> String {
        self.places.get(&place_id).cloned().unwrap_or_default()
    }

    /// The media the person's own gallery shows: attached to them directly,
    /// or, when `with_unions` is set, to one of their couples too.
    pub fn profile_tiles(&self, with_unions: bool) -> Vec<MediaWithLink> {
        self.bundle
            .profile_media
            .iter()
            .filter(|item| with_unions || item.family_id.is_none())
            .map(|item| item.tile.clone())
            .collect()
    }

    pub fn union_family_ids(&self) -> Vec<Uuid> {
        self.family.unions.iter().map(|u| u.family_id).collect()
    }

    /// The person's couples: their unions with a known partner, earliest
    /// first. A union recorded without a partner is not a couple.
    pub fn couples(&self) -> impl Iterator<Item = &UnionGroup> {
        self.family
            .unions
            .iter()
            .filter(|union| !union.partner_ids.is_empty())
    }

    /// The couple the couple view opens on for this person, if they have one.
    pub fn default_couple_id(&self) -> Option<Uuid> {
        self.couples().next().map(|union| union.family_id)
    }
}

/// A union's earliest dated event — its marriage, usually.
pub(crate) fn union_sort_date<'a>(
    events: impl IntoIterator<Item = &'a DomainEvent>,
) -> Option<NaiveDate> {
    events.into_iter().filter_map(|event| event.date_sort).min()
}

/// Orders unions by their earliest dated event. Undated unions follow, in
/// the order they were recorded.
pub(crate) fn sort_unions_chronologically<T>(
    unions: &mut [T],
    sort_date: impl Fn(&T) -> Option<NaiveDate>,
) {
    unions.sort_by_key(|union| {
        let date = sort_date(union);
        (date.is_none(), date)
    });
}

/// Which spouse of a couple the couple view draws on the left and which on
/// the right: the husband always on the left, the wife on the right. A
/// partner is placed by sex, and two spouses the rule cannot tell apart keep
/// their recorded order. A lone spouse keeps their side, the other one empty.
pub(crate) fn couple_sides(
    spouses: &[oxidgene_core::types::FamilySpouse],
    sex_of: impl Fn(Uuid) -> Sex,
) -> (Option<Uuid>, Option<Uuid>) {
    let side = |spouse: &oxidgene_core::types::FamilySpouse| match spouse.role {
        SpouseRole::Husband => 0,
        SpouseRole::Wife => 2,
        SpouseRole::Partner => match sex_of(spouse.person_id) {
            Sex::Male => 0,
            Sex::Unknown => 1,
            Sex::Female => 2,
        },
    };
    let mut ordered: Vec<_> = spouses.iter().collect();
    ordered.sort_by_key(|spouse| (side(spouse), spouse.sort_order));
    match ordered.as_slice() {
        [] => (None, None),
        [only] if side(only) == 2 => (None, Some(only.person_id)),
        [only] => (Some(only.person_id), None),
        [left, right, ..] => (Some(left.person_id), Some(right.person_id)),
    }
}

/// A [`Profile`] shared by pointer between a page and the sections it renders.
#[derive(Clone)]
pub(crate) struct SharedProfile(Arc<Profile>);

impl SharedProfile {
    pub fn new(profile: Profile) -> Self {
        Self(Arc::new(profile))
    }
}

impl PartialEq for SharedProfile {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl std::ops::Deref for SharedProfile {
    type Target = Profile;

    fn deref(&self) -> &Profile {
        &self.0
    }
}

/// A family's marriage date/place and divorce date, earliest first.
fn union_marriage_divorce(
    events: Option<&Vec<&DomainEvent>>,
    places: &HashMap<Uuid, String>,
    i18n: &I18n,
) -> (Option<DatePhrase>, Option<String>, Option<DatePhrase>) {
    let mut marriage_date = None;
    let mut marriage_place = None;
    let mut divorce_date = None;
    if let Some(events) = events {
        let mut sorted: Vec<&DomainEvent> = events.to_vec();
        sorted.sort_by_key(|e| e.date_sort);
        for e in sorted {
            match e.event_type {
                EventType::Marriage if marriage_date.is_none() => {
                    marriage_date = Some(event_date_phrase(i18n, e)).filter(|d| !d.is_empty());
                    marriage_place = e
                        .place_id
                        .map(|id| places.get(&id).cloned().unwrap_or_default());
                }
                EventType::Divorce if divorce_date.is_none() => {
                    divorce_date = Some(event_date_phrase(i18n, e)).filter(|d| !d.is_empty());
                }
                _ => {}
            }
        }
    }
    (marriage_date, marriage_place, divorce_date)
}

/// A date as a sentence carries it: « le 7 mars 1799 », « en an VII »,
/// « vers 1799 ».
fn date_in_sentence(i18n: &I18n, date: &DatePhrase) -> String {
    match date.kind {
        DateKind::Day => i18n.t_args("person.family.on_date", &[("date", &date.text)]),
        DateKind::Period => i18n.t_args("date.in", &[("date", &date.text)]),
        DateKind::Qualified => date.text.clone(),
    }
}

/// The vitals key matching both the displayed event and the person's sex.
fn vitals_event_key(event_type: EventType, has_date: bool, sex: Sex) -> String {
    let event = match event_type {
        EventType::Birth => "born",
        EventType::Baptism => "baptized",
        EventType::Death => "died",
        EventType::Burial => "buried",
        _ => unreachable!("only birth/death vital events have a header label"),
    };
    let date = if has_date { "prefix" } else { "no_date" };
    let base = format!("person.vitals.{event}_{date}");

    match sex {
        Sex::Male => format!("{base}_male"),
        Sex::Female => format!("{base}_female"),
        Sex::Unknown => base,
    }
}

/// Derive everything the sections of `person_id`'s profile draw.
///
/// `pedigree` supplies the parents when the bundle knows of none, which is
/// how a parent reached only through the ancestry projection still shows.
pub(crate) fn build_profile(
    bundle: Arc<PersonDetailBundle>,
    person_id: Uuid,
    pedigree: Option<&Pedigree>,
    i18n: &I18n,
) -> Profile {
    let detail = &*bundle;

    let mut names: HashMap<Uuid, Vec<PersonName>> = HashMap::new();
    for pn in &detail.names {
        names.entry(pn.person_id).or_default().push(pn.clone());
    }
    let places: HashMap<Uuid, String> = detail
        .places
        .iter()
        .map(|place| (place.id, place.name.clone()))
        .collect();
    let resolve = |pid: Uuid| resolve_name(pid, &names, i18n);

    // Index events by person and by family.
    let mut events_by_person: HashMap<Uuid, Vec<&DomainEvent>> = HashMap::new();
    let mut events_by_family: HashMap<Uuid, Vec<&DomainEvent>> = HashMap::new();
    for e in detail.events.iter().filter(|e| e.deleted_at.is_none()) {
        if let Some(epid) = e.person_id {
            events_by_person.entry(epid).or_default().push(e);
        }
        if let Some(fid) = e.family_id {
            events_by_family.entry(fid).or_default().push(e);
        }
    }

    // Tree-wide sex + lifespan lookups, used to decorate every person
    // mentioned in the family narrative (sex glyph + "birth-death" suffix,
    // matching the format shown on the pedigree cards).
    let sexes: HashMap<Uuid, Sex> = detail.persons.iter().map(|p| (p.id, p.sex)).collect();
    // Years carry their qualifier so the narrative hedges the same way the
    // pedigree cards do — "ca 1849" in both places.
    let mut birth_years: HashMap<Uuid, QualifiedYear> = HashMap::new();
    let mut death_years: HashMap<Uuid, QualifiedYear> = HashMap::new();
    for e in &detail.events {
        let (Some(pid), Some(year)) = (e.person_id, e.qualified_year()) else {
            continue;
        };
        match e.event_type {
            EventType::Birth => {
                birth_years.entry(pid).or_insert(year);
            }
            EventType::Death => {
                death_years.entry(pid).or_insert(year);
            }
            _ => {}
        }
    }
    let lifespans: HashMap<Uuid, String> = sexes
        .keys()
        .filter_map(|pid| {
            let lifespan = crate::components::pedigree_chart::format_lifespan(
                birth_years.get(pid).copied(),
                death_years.get(pid).copied(),
            );
            (!lifespan.is_empty()).then_some((*pid, lifespan))
        })
        .collect();

    let spouse_family_ids: Vec<Uuid> = detail
        .spouses
        .iter()
        .filter(|s| s.person_id == person_id)
        .map(|s| s.family_id)
        .collect();
    let child_family_ids: Vec<Uuid> = detail
        .children
        .iter()
        .filter(|c| c.person_id == person_id)
        .map(|c| c.family_id)
        .collect();

    // ── Family narrative ──
    let mut unions: Vec<UnionGroup> = spouse_family_ids
        .iter()
        .map(|fid| {
            let role = detail
                .spouses
                .iter()
                .find(|s| s.family_id == *fid && s.person_id == person_id)
                .map(|s| s.role)
                .unwrap_or(SpouseRole::Partner);
            let partner_ids = detail
                .spouses
                .iter()
                .filter(|s| s.family_id == *fid && s.person_id != person_id)
                .map(|s| s.person_id)
                .collect();
            let child_ids = detail
                .children
                .iter()
                .filter(|c| c.family_id == *fid)
                .map(|c| c.person_id)
                .collect();
            let (marriage_date, marriage_place, divorce_date) =
                union_marriage_divorce(events_by_family.get(fid), &places, i18n);
            UnionGroup {
                family_id: *fid,
                sort_date: union_sort_date(
                    events_by_family.get(fid).into_iter().flatten().copied(),
                ),
                partner_ids,
                role,
                marriage_date,
                marriage_place,
                divorce_date,
                child_ids,
            }
        })
        .collect();
    sort_unions_chronologically(&mut unions, |union| union.sort_date);

    let mut parent_ids: Vec<Uuid> = Vec::new();
    let mut full_sibling_ids: Vec<Uuid> = Vec::new();
    for fid in &child_family_ids {
        for s in detail.spouses.iter().filter(|s| s.family_id == *fid) {
            parent_ids.push(s.person_id);
        }
        for c in detail.children.iter() {
            if c.family_id == *fid && c.person_id != person_id {
                full_sibling_ids.push(c.person_id);
            }
        }
    }
    if parent_ids.is_empty()
        && let Some(pedigree) = pedigree
    {
        let mut pedigree_parent_ids = pedigree
            .edges
            .iter()
            .filter(|edge| edge.child_id == person_id)
            .map(|edge| edge.parent_id)
            .collect::<Vec<_>>();
        pedigree_parent_ids.sort_by_key(|parent_id| {
            pedigree
                .persons
                .get(parent_id)
                .map(|person| match person.sex {
                    Sex::Male => 0,
                    Sex::Female => 1,
                    Sex::Unknown => 2,
                })
                .unwrap_or(2)
        });
        pedigree_parent_ids.dedup();
        parent_ids = pedigree_parent_ids;
    }

    // Half-siblings: each parent's *other* unions.
    let mut half_sibling_groups: Vec<SiblingGroup> = Vec::new();
    for parent_id in parent_ids.iter().filter(|_| !child_family_ids.is_empty()) {
        let other_family_ids: Vec<Uuid> = detail
            .spouses
            .iter()
            .filter(|s| s.person_id == *parent_id && !child_family_ids.contains(&s.family_id))
            .map(|s| s.family_id)
            .collect();
        for fid in &other_family_ids {
            let other_parent_id = detail
                .spouses
                .iter()
                .find(|s| s.family_id == *fid && s.person_id != *parent_id)
                .map(|s| s.person_id);
            let child_ids: Vec<Uuid> = detail
                .children
                .iter()
                .filter(|c| c.family_id == *fid)
                .map(|c| c.person_id)
                .collect();
            if !child_ids.is_empty() {
                half_sibling_groups.push(SiblingGroup {
                    common_parent_id: *parent_id,
                    other_parent_id,
                    child_ids,
                });
            }
        }
    }

    // ── Enriched event list ──
    //
    // Combines three sources:
    //   1. Individual events (birth, death, occupation…)
    //   2. Conjugal family events (marriage, divorce…) and the children's
    //   3. Parental family events (parent death, sibling birth…)
    let is_life_event = |e: &DomainEvent| {
        matches!(
            e.event_type,
            EventType::Birth | EventType::Death | EventType::Baptism | EventType::Burial
        )
    };
    let mut events: Vec<EnrichedEvent> = Vec::new();
    let mut seen_ids: HashSet<Uuid> = HashSet::new();

    for &e in events_by_person.get(&person_id).into_iter().flatten() {
        if seen_ids.insert(e.id) {
            events.push(EnrichedEvent {
                event: e.clone(),
                origin: EventOrigin::Individual,
                context: None,
                union_id: None,
            });
        }
    }

    for fid in &spouse_family_ids {
        let partner_name = detail
            .spouses
            .iter()
            .find(|s| s.family_id == *fid && s.person_id != person_id)
            .map(|s| resolve(s.person_id));
        for &e in events_by_family.get(fid).into_iter().flatten() {
            if seen_ids.insert(e.id) {
                events.push(EnrichedEvent {
                    event: e.clone(),
                    origin: EventOrigin::ConjugalFamily,
                    context: partner_name.clone(),
                    union_id: Some(*fid),
                });
            }
        }
        for c in detail.children.iter().filter(|c| c.family_id == *fid) {
            let child_name = resolve(c.person_id);
            for &e in events_by_person.get(&c.person_id).into_iter().flatten() {
                if is_life_event(e) && seen_ids.insert(e.id) {
                    events.push(EnrichedEvent {
                        event: e.clone(),
                        origin: EventOrigin::ChildFamily,
                        context: Some(child_name.clone()),
                        union_id: Some(*fid),
                    });
                }
            }
        }
    }

    for fid in &child_family_ids {
        for &e in events_by_family.get(fid).into_iter().flatten() {
            if seen_ids.insert(e.id) {
                events.push(EnrichedEvent {
                    event: e.clone(),
                    origin: EventOrigin::ParentalFamily,
                    context: None,
                    union_id: None,
                });
            }
        }
        // Major individual events of parents (death, burial).
        for s in detail.spouses.iter().filter(|s| s.family_id == *fid) {
            let parent_name = resolve(s.person_id);
            for &e in events_by_person.get(&s.person_id).into_iter().flatten() {
                if matches!(e.event_type, EventType::Death | EventType::Burial)
                    && seen_ids.insert(e.id)
                {
                    events.push(EnrichedEvent {
                        event: e.clone(),
                        origin: EventOrigin::ParentalFamily,
                        context: Some(parent_name.clone()),
                        union_id: None,
                    });
                }
            }
        }
        // Major individual events of siblings (birth, death, baptism, burial).
        for c in detail
            .children
            .iter()
            .filter(|c| c.family_id == *fid && c.person_id != person_id)
        {
            let sib_name = resolve(c.person_id);
            for &e in events_by_person.get(&c.person_id).into_iter().flatten() {
                if is_life_event(e) && seen_ids.insert(e.id) {
                    events.push(EnrichedEvent {
                        event: e.clone(),
                        origin: EventOrigin::ParentalFamily,
                        context: Some(sib_name.clone()),
                        union_id: None,
                    });
                }
            }
        }
    }
    events.sort_by_key(|a| a.event.date_sort);

    // ── Citations and evidence per event ──
    let mut citations_by_event: HashMap<Uuid, Vec<String>> = HashMap::new();
    let source_by_id: HashMap<Uuid, &oxidgene_core::types::Source> =
        detail.sources.iter().map(|s| (s.id, s)).collect();
    for citation in &detail.citations {
        let (Some(eid), Some(source)) = (citation.event_id, source_by_id.get(&citation.source_id))
        else {
            continue;
        };
        let text = match &citation.page {
            Some(page) if !page.is_empty() => format!("{} \u{2014} {page}", source.title),
            _ => source.title.clone(),
        };
        citations_by_event.entry(eid).or_default().push(text);
    }
    let mut evidence_by_event: HashMap<Uuid, Vec<MediaWithLink>> = HashMap::new();
    for item in &detail.event_media {
        evidence_by_event
            .entry(item.event_id)
            .or_default()
            .push(MediaWithLink {
                link_id: item.link_id,
                sort_order: item.sort_order,
                media: item.media.clone(),
            });
    }

    let person = detail.persons.iter().find(|p| p.id == person_id).cloned();
    let own_names = names.get(&person_id).cloned().unwrap_or_default();
    let own_events: Vec<&DomainEvent> = events_by_person
        .get(&person_id)
        .cloned()
        .unwrap_or_default();
    let name = header_name(&own_names, i18n);
    let vitals = vital_clauses(&own_events, &places, i18n);

    Profile {
        person_id,
        person,
        sosa_number: detail.sosa_number,
        name,
        vitals,
        family: FamilyNarrative {
            parent_ids,
            unions,
            full_sibling_ids,
            half_sibling_groups,
        },
        events,
        names,
        places,
        sexes,
        lifespans,
        citations_by_event,
        evidence_by_event,
        bundle,
    }
}

fn header_name(own_names: &[PersonName], i18n: &I18n) -> HeaderName {
    let primary = own_names
        .iter()
        .find(|n| n.is_primary)
        .or(own_names.first());
    let Some(primary) = primary else {
        return HeaderName {
            display_name: i18n.t("common.unnamed"),
            ..HeaderName::default()
        };
    };
    let display_name = match primary.display_name() {
        dn if dn.is_empty() => i18n.t("common.unnamed"),
        dn => dn,
    };
    // Particle included, so the header still matches `display_name` part
    // for part.
    let rest = [primary.full_surname().as_deref(), primary.suffix.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");

    // Excludes whichever name was picked as display_name above, and
    // de-duplicates identical given/surname combinations. Keyed on the full
    // surname: "Cruz" and "de la Cruz" are different alternates and must not
    // dedup against each other.
    let mut seen: HashSet<(String, String)> = HashSet::new();
    seen.insert((
        primary.given_names.clone().unwrap_or_default(),
        primary.full_surname().unwrap_or_default(),
    ));
    let alt_names = own_names
        .iter()
        .filter(|n| n.id != primary.id)
        .filter_map(|n| {
            let key = (
                n.given_names.clone().unwrap_or_default(),
                n.full_surname().unwrap_or_default(),
            );
            if !seen.insert(key) {
                return None;
            }
            let dn = n.display_name();
            if dn.is_empty() { None } else { Some(dn) }
        })
        .collect();

    HeaderName {
        display_name,
        prefix: primary.prefix.clone(),
        given: primary.given_names.clone().filter(|s| !s.is_empty()),
        rest,
        alt_names,
    }
}

/// Birth/death vitals clauses shown under the header name, e.g.
/// "Born on **10 December 1700** in Paris — **43 years old**."
fn vital_clauses(
    own_events: &[&DomainEvent],
    places: &HashMap<Uuid, String>,
    i18n: &I18n,
) -> Vec<VitalClause> {
    let place_name = |id: Uuid| places.get(&id).cloned().unwrap_or_default();
    // Prefer the birth, but skip a dateless stub in favour of a dated
    // baptism — the register entry is very often the sacrament, and the
    // header should say "vers 1620" rather than a bare "Né le". Same
    // resolution as the pedigree card and its side panel.
    let dated_or_first = |preferred: EventType, fallback: EventType| {
        let of_type = |t: EventType| own_events.iter().copied().find(move |e| e.event_type == t);
        let dated = |e: &&DomainEvent| e.date_value.is_some() || e.date_sort.is_some();
        of_type(preferred)
            .filter(dated)
            .or_else(|| of_type(fallback).filter(dated))
            .or_else(|| of_type(preferred))
            .or_else(|| of_type(fallback))
    };
    let birth = dated_or_first(EventType::Birth, EventType::Baptism);
    let death = dated_or_first(EventType::Death, EventType::Burial);

    let mut clauses = Vec::new();
    if let Some(b) = birth {
        let (date, place) = (event_date_phrase(i18n, b), b.place_id.map(place_name));
        // A birth event carrying neither a date nor a place says nothing;
        // rendering it produced the dangling "Né(e) le ".
        if !date.is_empty() || place.is_some() {
            clauses.push(VitalClause::Event {
                event_type: b.event_type,
                date,
                place,
            });
        }
    }
    if let Some(d) = death {
        clauses.push(VitalClause::Event {
            event_type: d.event_type,
            date: event_date_phrase(i18n, d),
            place: d.place_id.map(place_name),
        });
    }
    if let Some(birth_date) = birth.and_then(|e| e.date_sort) {
        // Only fall back to "today" when the person has no death event at
        // all (still alive). If a death event exists but its date is
        // unrecorded, the age at death is unknown — don't guess it as the
        // current date, which would wildly inflate the age shown.
        let end_date = match death {
            Some(d) => d.date_sort,
            None => Some(chrono::Local::now().date_naive()),
        };
        if let Some(end_date) = end_date {
            clauses.push(VitalClause::Age(age_span(birth_date, end_date)));
        }
    }

    // Occupation(s), shown on its own line below the birth/death vitals. A
    // person can have several OCCU events (career changes); list them all
    // rather than picking just one.
    let occupations: Vec<String> = own_events
        .iter()
        .filter(|e| e.event_type == EventType::Occupation)
        .filter_map(|e| e.description.clone())
        .filter(|title| !title.is_empty())
        .collect();
    if !occupations.is_empty() {
        clauses.push(VitalClause::Occupation(occupations));
    }

    clauses
}

/// The events a person's media may be linked to from their gallery: their
/// own and those of their conjugal families, never the derived parental and
/// child events shown only for narrative context in the timeline.
pub(crate) fn media_event_links<'a>(
    events: impl IntoIterator<Item = &'a EnrichedEvent>,
    i18n: &I18n,
) -> Vec<MediaEventLinkOption> {
    events
        .into_iter()
        .filter(|entry| {
            matches!(
                entry.origin,
                EventOrigin::Individual | EventOrigin::ConjugalFamily
            )
        })
        .map(|entry| {
            let event = &entry.event;
            let date = if event.calendar == Calendar::Gregorian
                && event.date_qualifier == DateQualifier::Exact
            {
                event
                    .date_value
                    .as_deref()
                    .and_then(|value| NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
                    .map(|value| value.format("%d/%m/%Y").to_string())
            } else {
                None
            }
            .or_else(|| opt_str(&format_event_date(i18n, event)));
            MediaEventLinkOption {
                event_id: event.id,
                label: i18n.t(event_type_label_key(event.event_type)),
                date,
                date_sort: event.date_sort,
            }
        })
        .collect()
}

// ── Age ──────────────────────────────────────────────────────────────────

/// Whole years between two dates, matching the usual "age" definition
/// (doesn't count the current year until the birthday has passed).
fn age_in_years(birth: NaiveDate, end: NaiveDate) -> i32 {
    use chrono::Datelike;
    let mut age = end.year() - birth.year();
    if (end.month(), end.day()) < (birth.month(), birth.day()) {
        age -= 1;
    }
    age.max(0)
}

/// Whole months between two dates (doesn't count the current month until the
/// day-of-month has passed) — used by `age_span` to pick a display unit.
fn months_between(birth: NaiveDate, end: NaiveDate) -> i32 {
    use chrono::Datelike;
    let mut months = (end.year() - birth.year()) * 12 + end.month() as i32 - birth.month() as i32;
    if end.day() < birth.day() {
        months -= 1;
    }
    months.max(0)
}

/// A person's age at `end`, expressed in the coarsest unit that keeps it
/// meaningful: days for infants under one month old, months for children
/// under one year old, years otherwise.
pub(crate) enum AgeSpan {
    Days(i32),
    Months(i32),
    Years(i32),
}

fn age_span(birth: NaiveDate, end: NaiveDate) -> AgeSpan {
    let months = months_between(birth, end);
    if months < 1 {
        AgeSpan::Days((end - birth).num_days().max(0) as i32)
    } else if months < 12 {
        AgeSpan::Months(months)
    } else {
        AgeSpan::Years(age_in_years(birth, end))
    }
}

// ── Shared page resources ────────────────────────────────────────────────

/// The tree, for the breadcrumb and the identity badges (cache-backed).
pub(crate) fn use_tree_resource(
    load_trace: UiLoadTrace,
    api: ApiClient,
    tree_id: Signal<Option<Uuid>>,
    refresh: Signal<u32>,
    i18n: I18n,
) -> Resource<Result<Tree, ApiError>> {
    let tree_cache = use_tree_cache();
    use_traced_resource(load_trace, "tree", move || {
        let api = api.clone();
        let _tick = refresh();
        let _gen = tree_cache.generation();
        let tid = tree_id();
        async move {
            let Some(tid) = tid else {
                return Err(ApiError::Api {
                    status: 400,
                    body: i18n.t("common.invalid_tree_id"),
                });
            };
            fetch_tree_cached(&api, &tree_cache, tid).await
        }
    })
}

/// The SOSA root's ancestors (same query as the tree view), used to show the
/// green SOSA mark in the family narrative.
pub(crate) fn use_sosa_ancestors(
    load_trace: UiLoadTrace,
    api: ApiClient,
    tree_id: Signal<Option<Uuid>>,
    tree_resource: Resource<Result<Tree, ApiError>>,
) -> Resource<HashSet<Uuid>> {
    let tree_cache = use_tree_cache();
    use_traced_resource(load_trace, "sosa_ancestors", move || {
        let api = api.clone();
        let tid = tree_id();
        let _gen = tree_cache.generation();
        let sosa_root = match &*tree_resource.read() {
            Some(Ok(tree)) => tree.sosa_root_person_id,
            _ => None,
        };
        async move {
            let (Some(tid), Some(sosa_id)) = (tid, sosa_root) else {
                return HashSet::new();
            };
            match api.get_ancestors(tid, sosa_id, None).await {
                Ok(entries) => entries.into_iter().map(|a| a.person_id).collect(),
                Err(_) => HashSet::new(),
            }
        }
    })
}

/// The Ancestors section's small static pedigree window (self + parents +
/// grandparents).
pub(crate) fn use_ancestor_pedigree(
    load_trace: UiLoadTrace,
    api: ApiClient,
    tree_id: Signal<Option<Uuid>>,
    person_id: ReadSignal<Option<Uuid>>,
    i18n: I18n,
) -> Resource<Result<Option<Pedigree>, ApiError>> {
    use_traced_resource(load_trace, "ancestor_pedigree", move || {
        let api = api.clone();
        let tid = tree_id();
        let pid = person_id();
        async move {
            let (Some(tid), Some(pid)) = (tid, pid) else {
                return Err(ApiError::Api {
                    status: 400,
                    body: i18n.t("common.invalid_ids"),
                });
            };
            api.get_pedigree(tid, pid, 2, 0).await.map(Some)
        }
    })
}

/// The Ancestors section's pedigree fragment, assembled once per change. It
/// carries a portrait picture per person, so rebuilding it inline meant
/// copying those on every render of the page.
pub(crate) fn use_mini_pedigree(
    pedigree: Resource<Result<Option<Pedigree>, ApiError>>,
    photos: Resource<HashMap<Uuid, CroppedSource>>,
) -> Memo<Option<(Uuid, SharedPedigree)>> {
    use_memo(move || {
        let cached = pedigree.read();
        let Some(Ok(Some(cached))) = &*cached else {
            return None;
        };
        let mut data = crate::ui_observability::measure_ui("pedigree_data", || {
            crate::components::pedigree_chart::PedigreeData::from_pedigree(cached)
        });
        if let Some(photos) = &*photos.read() {
            data.photos = photos
                .iter()
                .filter(|(id, _)| cached.persons.contains_key(id))
                .map(|(id, photo)| (*id, photo.clone()))
                .collect();
        }
        Some((cached.root_person_id, SharedPedigree::new(data)))
    })
}

// ── Sections ─────────────────────────────────────────────────────────────

/// The desktop reloads on its own; the web build offers a manual refresh.
pub(crate) const SHOW_MANUAL_REFRESH: bool = cfg!(target_arch = "wasm32");

/// The header's manual reload button, shown when [`SHOW_MANUAL_REFRESH`].
pub(crate) fn refresh_button(i18n: &I18n, mut on_refresh: impl FnMut() + 'static) -> Element {
    rsx! {
        button {
            class: "btn btn-outline pd-header-action-btn",
            title: i18n.t("person.refresh"),
            aria_label: i18n.t("person.refresh"),
            onclick: move |_| on_refresh(),
            svg {
                class: "pd-header-action-icon",
                width: "16", height: "16", fill: "none", "viewBox": "0 0 24 24",
                stroke: "currentColor", "strokeWidth": "2",
                path { d: "M20 11a8 8 0 1 0-2.34 5.66" }
                path { d: "M20 4v7h-7" }
            }
            span { class: "pd-header-action-label", {i18n.t("person.refresh")} }
        }
    }
}

/// What every section needs beyond the profile it draws.
pub(crate) struct SectionContext<'a> {
    pub i18n: I18n,
    pub tree_id: Uuid,
    pub sosa_ancestors: &'a HashSet<Uuid>,
    /// Bumped when a gallery changes what is attached, so the page reloads.
    pub media_revision: Signal<u32>,
}

/// Renders "[SOSA mark] [sex glyph] Name [years]", linked to the person's own
/// page — used throughout the family narrative.
pub(crate) fn person_chip(ctx: &SectionContext, profile: &Profile, pid: Uuid) -> Element {
    let name = profile.name_of(pid, &ctx.i18n);
    let sex = profile.sexes.get(&pid).copied().unwrap_or(Sex::Unknown);
    let (sex_glyph, sex_class) = match sex {
        Sex::Male => ("\u{2642}", "pd-sex-glyph male"),
        Sex::Female => ("\u{2640}", "pd-sex-glyph female"),
        Sex::Unknown => ("?", "pd-sex-glyph"),
    };
    let lifespan = profile.lifespans.get(&pid).cloned().unwrap_or_default();
    let is_sosa = ctx.sosa_ancestors.contains(&pid);
    let tree_id = ctx.tree_id.to_string();
    rsx! {
        span { class: "pd-person-chip",
            span { class: "pd-person-identity",
                if is_sosa {
                    svg { class: "pd-sosa-mark", "viewBox": "0 0 10 10", width: "10", height: "10",
                        circle { cx: "5", cy: "5", r: "5", fill: "var(--pn-sosa)" }
                        circle { cx: "5", cy: "5", r: "3", fill: "var(--white)" }
                        circle { cx: "5", cy: "5", r: "1.8", fill: "var(--pn-sosa)" }
                    }
                }
                span { class: sex_class, "{sex_glyph}" }
                Link {
                    to: Route::PersonDetail { tree_id, person_id: pid.to_string() },
                    class: "pd-person-link",
                    "{name}"
                }
            }
            if !lifespan.is_empty() {
                span { class: "pd-person-years", "{lifespan}" }
            }
        }
    }
}

/// The identity card: avatar, names, vitals, identity badges and `actions`.
pub(crate) fn header_section(
    ctx: &SectionContext,
    profile: &Profile,
    photo: Option<CroppedSource>,
    is_self: bool,
    on_self_badge: EventHandler<()>,
    actions: Element,
) -> Element {
    let i18n = ctx.i18n;
    let Some(person) = profile.person.as_ref() else {
        return rsx! {};
    };
    let person_sex = person.sex;
    let sex_symbol = match person_sex {
        Sex::Male => "\u{2642}",
        Sex::Female => "\u{2640}",
        Sex::Unknown => "?",
    };
    let avatar = photo.unwrap_or_else(|| CroppedSource::silhouette(person_sex));
    let name = &profile.name;
    rsx! {
        div { class: "card page-header",
            div { class: "pd-header-left",
                CroppedImage {
                    class: "pd-avatar",
                    image: avatar,
                    alt: String::new(),
                    fallback: CroppedSource::silhouette(person_sex),
                }
                div { class: "pd-header-main",
                    div { class: "pd-header-top",
                        h1 {
                            if let Some(given) = name.given.clone() {
                                if let Some(prefix) = &name.prefix {
                                    "{prefix} "
                                }
                                GivenNamesHover { given_names: given }
                                if !name.rest.is_empty() {
                                    " {name.rest}"
                                }
                            } else {
                                "{name.display_name}"
                            }
                        }
                    }
                    if !name.alt_names.is_empty() {
                        p { class: "pd-alt-names",
                            for n in name.alt_names.iter() {
                                span { key: "{n}", "({n})" }
                            }
                        }
                    }
                    if !profile.vitals.is_empty() {
                        p { class: "pd-vitals",
                            span { class: "pd-sex-mark", "{sex_symbol}" }
                            for (i, clause) in profile.vitals.iter().enumerate() {
                                if i > 0 {
                                    match clause {
                                        VitalClause::Event {
                                            event_type: EventType::Death | EventType::Burial,
                                            ..
                                        }
                                        | VitalClause::Occupation(_) => rsx! { br {} },
                                        _ => rsx! { " \u{2014} " },
                                    }
                                }
                                {vital_clause(clause, person_sex, &i18n)}
                            }
                        }
                    }
                }
            }
            div { class: "pd-header-actions",
                div { class: "pd-header-sosa",
                    if let Some(sosa) = profile.sosa_number {
                        span { class: "badge pd-sosa-badge", "SOSA {sosa}" }
                    }
                    if is_self {
                        button {
                            class: "badge pd-self-badge",
                            title: i18n.t("person.self_badge_settings"),
                            onclick: move |_| on_self_badge.call(()),
                            {i18n.t("person.self_badge")}
                        }
                    }
                }
                div { class: "pd-header-buttons", {actions} }
            }
        }
    }
}

fn vital_clause(clause: &VitalClause, sex: Sex, i18n: &I18n) -> Element {
    match clause {
        VitalClause::Event {
            event_type,
            date,
            place,
        } => {
            let place_clause = place
                .as_ref()
                .map(|p| {
                    format!(
                        " {}",
                        i18n.t_args("person.vitals.in_place", &[("place", p)])
                    )
                })
                .unwrap_or_default();
            // « Né le 8 déc. 1776 », but « Né en 1776 » and « Né vers 1776 »:
            // only a day takes the label that ends in « le ».
            let on_day = date.kind == DateKind::Day;
            let label = i18n.t(&vitals_event_key(
                *event_type,
                on_day && !date.is_empty(),
                sex,
            ));
            let date = match date.kind {
                DateKind::Period => i18n.t_args("date.in", &[("date", &date.text)]),
                _ => date.text.clone(),
            };
            if date.is_empty() {
                rsx! {
                    b { "{label}" }
                    "{place_clause}"
                }
            } else {
                rsx! {
                    "{label} "
                    b { "{date}" }
                    "{place_clause}"
                }
            }
        }
        VitalClause::Age(age) => {
            let (key, n) = match age {
                AgeSpan::Days(n) => ("person.vitals.age_days", *n),
                AgeSpan::Months(n) => ("person.vitals.age_months", *n),
                AgeSpan::Years(n) => ("person.vitals.age", *n),
            };
            let label = i18n
                .t_plural(key, n as usize)
                .replace("{n}", &n.to_string());
            rsx! { b { "{label}" } }
        }
        VitalClause::Occupation(titles) => rsx! {
            OccupationsHover { titles: titles.clone() }
        },
    }
}

/// A notes card, rendered only when there is something to show.
pub(crate) fn notes_section(
    i18n: &I18n,
    title_key: &str,
    notes: Option<&Result<Vec<Note>, ApiError>>,
) -> Element {
    match notes {
        Some(Ok(notes)) if !notes.is_empty() => rsx! {
            div { class: "card pd-section",
                h2 { style: "font-size: 1.1rem; margin-bottom: 12px;", {i18n.t(title_key)} }
                div {
                    for note in notes.iter() {
                        div {
                            key: "{note.id}",
                            style: "margin-bottom: 12px; padding: 12px; border: 1px solid var(--color-border); border-radius: var(--radius);",
                            // Note bodies carry markup — GEDCOM and GeneWeb
                            // both put some in — and are sanitized server-side
                            // on write by `oxidgene_db::html::sanitize_note_html`,
                            // so nothing executable can reach here. That same
                            // pass stores line breaks as `\n`, which only shows
                            // as a break once turned back into `<br>`.
                            div {
                                class: "note-html",
                                dangerous_inner_html: note_html_for_display(&note.text),
                            }
                        }
                    }
                }
            }
        },
        Some(Err(e)) => rsx! {
            div { class: "error-msg", {i18n.t_args("person.load_notes_error", &[("error", &e.to_string())])} }
        },
        _ => rsx! {},
    }
}

/// A read-only media card with a compact `+` to add a document.
///
/// The gallery stays read-only — restructuring what is attached belongs to
/// the edit modals — but a document can be added from here, including before
/// the first one.
#[component]
pub(crate) fn ProfileMediaCard(
    tree_id: Uuid,
    owner: MediaOwner,
    title: String,
    /// Couples whose media this gallery also shows.
    #[props(default)]
    related_family_ids: Vec<Uuid>,
    /// Events a media may be linked to, from the tile's menu and the upload.
    #[props(default)]
    event_links: Vec<MediaEventLinkOption>,
    #[props(default)] preloaded_tiles: Option<Vec<MediaWithLink>>,
    #[props(default)] preloaded_bundle: Option<Arc<GalleryBundle>>,
    #[props(default)] preloaded_portrait: Option<(Option<Uuid>, Option<Uuid>)>,
    #[props(default)] preloaded_vignettes: Option<Vec<Vignette>>,
    revision: u32,
    on_changed: EventHandler<()>,
) -> Element {
    let i18n = crate::i18n::use_i18n();
    let mut document_form_open = use_signal(|| false);
    let upload_events = event_links
        .iter()
        .map(|link| (link.event_id, link.label.clone()))
        .collect::<Vec<_>>();
    rsx! {
        div { class: "card pd-section",
            div { class: "pd-media-header",
                h2 { style: "font-size: 1.1rem;", "{title}" }
                div { class: "media-drop media-upload-icon",
                    button {
                        class: "media-upload-icon-btn",
                        r#type: "button",
                        title: i18n.t("media.new_document"),
                        onclick: move |_| document_form_open.set(true),
                        span { class: "media-upload-icon-glyph", "+" }
                    }
                }
            }
            MediaGallery {
                tree_id,
                owner,
                related_family_ids,
                profile_event_links: event_links,
                read_only: true,
                preloaded_tiles,
                preloaded_bundle,
                preloaded_portrait,
                preloaded_vignettes,
                external_revision: revision,
                on_changed: move |()| on_changed.call(()),
            }
        }
        if document_form_open() {
            DocumentForm {
                tree_id,
                owner,
                // The profile's own events, so a certificate can be filed as
                // evidence for the event it proves while it is being added
                // rather than in a second pass.
                events: upload_events,
                on_created: move |()| on_changed.call(()),
                on_close: move |()| document_form_open.set(false),
            }
        }
    }
}

/// The family narrative: parents, unions and their children, siblings.
///
/// `except_union` leaves one union out — the couple page draws the couple's
/// own union once, across both spouses' columns.
pub(crate) fn family_section(
    ctx: &SectionContext,
    profile: &Profile,
    except_union: Option<Uuid>,
) -> Element {
    let i18n = ctx.i18n;
    let family = &profile.family;
    let person_sex = profile.person.as_ref().map(|person| person.sex);
    let unions: Vec<&UnionGroup> = family
        .unions
        .iter()
        .filter(|u| Some(u.family_id) != except_union)
        .collect();
    let parent_ids = &family.parent_ids;
    let is_empty = parent_ids.is_empty()
        && unions.is_empty()
        && family.full_sibling_ids.is_empty()
        && family.half_sibling_groups.is_empty();

    rsx! {
        div { class: "card pd-family-card pd-section",
            h2 { style: "font-size: 1.1rem; margin-bottom: 12px;", {i18n.t("person.family_connections")} }

            if !parent_ids.is_empty() {
                p { class: "pd-family-prose",
                    {
                        let key = match (parent_ids.len() >= 2, person_sex) {
                            (true, Some(Sex::Male)) => "person.family.son_of_two",
                            (true, Some(Sex::Female)) => "person.family.daughter_of_two",
                            (true, _) => "person.family.child_of_two",
                            (false, Some(Sex::Male)) => "person.family.son_of_one",
                            (false, Some(Sex::Female)) => "person.family.daughter_of_one",
                            (false, _) => "person.family.child_of_one",
                        };
                        let template = i18n.t(key);
                        if parent_ids.len() >= 2 {
                            let (pre, rest) =
                                template.split_once("{p1}").unwrap_or((template.as_str(), ""));
                            let (mid, post) = rest.split_once("{p2}").unwrap_or((rest, ""));
                            rsx! {
                                "{pre}"
                                {person_chip(ctx, profile, parent_ids[0])}
                                "{mid}"
                                {person_chip(ctx, profile, parent_ids[1])}
                                "{post}"
                            }
                        } else {
                            let (pre, post) =
                                template.split_once("{p1}").unwrap_or((template.as_str(), ""));
                            rsx! {
                                "{pre}"
                                {person_chip(ctx, profile, parent_ids[0])}
                                "{post}"
                            }
                        }
                    }
                }
            }

            for union in unions.iter() {
                div { key: "{union.family_id}", class: "pd-union",
                    p { class: "pd-union-line",
                        {union_line(ctx, profile, union, true)}
                    }
                    {children_list(ctx, profile, &union.child_ids)}
                }
            }

            if !family.full_sibling_ids.is_empty() {
                div { class: "pd-fc-section",
                    h3 { class: "pd-fc-label", {i18n.t("person.siblings")} }
                    {children_list(ctx, profile, &family.full_sibling_ids)}
                }
            }

            if !family.half_sibling_groups.is_empty() {
                div { class: "pd-fc-section",
                    h3 { class: "pd-fc-label", {i18n.t("person.half_siblings")} }
                    for (idx, group) in family.half_sibling_groups.iter().enumerate() {
                        div { key: "{idx}", class: "pd-sib-group",
                            p { class: "pd-sib-group-head",
                                {
                                    let side_template = i18n.t("person.family.side_of");
                                    let (side_pre, side_post) = side_template
                                        .split_once("{parent}")
                                        .unwrap_or((side_template.as_str(), ""));
                                    let with_template = i18n.t("person.family.with_person");
                                    let (with_pre, with_post) = with_template
                                        .split_once("{partner}")
                                        .unwrap_or((with_template.as_str(), ""));
                                    let unknown_label = i18n.t("person.family.unknown_person");
                                    rsx! {
                                        "{side_pre}"
                                        {person_chip(ctx, profile, group.common_parent_id)}
                                        "{side_post}, {with_pre}"
                                        if let Some(pid) = group.other_parent_id {
                                            {person_chip(ctx, profile, pid)}
                                        } else {
                                            "{unknown_label}"
                                        }
                                        "{with_post}"
                                    }
                                }
                            }
                            {children_list(ctx, profile, &group.child_ids)}
                        }
                    }
                }
            }

            if is_empty {
                div { class: "empty-state",
                    p { {i18n.t("person.no_family_connections")} }
                }
            }
        }
    }
}

/// "Married on <date> in <place>, with <partner>, divorced on <date>, and
/// had:" — the partner clause only when `with_partner` is set.
pub(crate) fn union_line(
    ctx: &SectionContext,
    profile: &Profile,
    union: &UnionGroup,
    with_partner: bool,
) -> Element {
    let i18n = ctx.i18n;
    // Everything up to "with" is plain text; the partner name(s) need real
    // links, so the "with {partner}" template is split around its
    // placeholder instead of substituted.
    let mut prefix = if union.role == SpouseRole::Partner {
        i18n.t("person.family.in_relationship")
    } else {
        i18n.t("person.family.married")
    };
    if let Some(date) = &union.marriage_date {
        prefix.push(' ');
        prefix.push_str(&date_in_sentence(&i18n, date));
    }
    if let Some(place) = &union.marriage_place {
        prefix.push(' ');
        prefix.push_str(&i18n.t_args("person.family.in_place", &[("place", place)]));
    }
    let with_template = i18n.t("person.family.with_person");
    let (with_pre, with_post) = with_template
        .split_once("{partner}")
        .unwrap_or((with_template.as_str(), ""));
    let mut suffix = String::new();
    if with_partner {
        prefix.push_str(", ");
        prefix.push_str(with_pre);
        suffix.push_str(with_post);
    }
    if let Some(ddate) = &union.divorce_date {
        suffix.push_str(", ");
        suffix.push_str(&i18n.t("person.family.divorced"));
        suffix.push(' ');
        suffix.push_str(&date_in_sentence(&i18n, ddate));
    }
    if union.child_ids.is_empty() {
        suffix.push('.');
    } else {
        suffix.push_str(", ");
        suffix.push_str(&i18n.t("person.family.and_had"));
    }
    let and_word = i18n.t("common.and");
    rsx! {
        "{prefix}"
        if with_partner {
            for (i, pid) in union.partner_ids.iter().enumerate() {
                if i > 0 {
                    " {and_word} "
                }
                {person_chip(ctx, profile, *pid)}
            }
        }
        "{suffix}"
    }
}

/// A list of person chips, one per line.
pub(crate) fn children_list(ctx: &SectionContext, profile: &Profile, ids: &[Uuid]) -> Element {
    if ids.is_empty() {
        return rsx! {};
    }
    rsx! {
        ul { class: "pd-children",
            for id in ids.iter() {
                li { key: "{id}", {person_chip(ctx, profile, *id)} }
            }
        }
    }
}

/// A timeline card listing `events` with their sources and evidence.
pub(crate) fn timeline_section(
    ctx: &SectionContext,
    profile: &Profile,
    title: String,
    events: &[&EnrichedEvent],
) -> Element {
    let i18n = ctx.i18n;
    let tree_id = ctx.tree_id;
    let mut media_revision = ctx.media_revision;
    rsx! {
        div { class: "card pd-section",
            div { class: "section-header",
                h2 { style: "font-size: 1.1rem;", "{title}" }
            }
            if events.is_empty() {
                div { class: "empty-state",
                    p { {i18n.t("person.no_events")} }
                }
            } else {
                ul { class: "pd-timeline",
                    for ee in events.iter() {
                        {
                            let event = &ee.event;
                            let eid = event.id;
                            let event_type_label = i18n.t(event_type_label_key(event.event_type));
                            let desc = event.description.clone().unwrap_or_default();
                            let place_display = event.place_id.map(|id| profile.place_name(id));

                            let origin_label = match &ee.origin {
                                EventOrigin::Individual => i18n.t("person.origin_individual"),
                                EventOrigin::ConjugalFamily => i18n.t("person.origin_conjugal"),
                                EventOrigin::ChildFamily => i18n.t("person.origin_child"),
                                EventOrigin::ParentalFamily => i18n.t("person.origin_parental"),
                            };
                            let origin_display = match &ee.context {
                                Some(context) => format!("{origin_label} ({context})"),
                                None => origin_label,
                            };
                            let is_direct = matches!(
                                ee.origin,
                                EventOrigin::Individual | EventOrigin::ConjugalFamily
                            );
                            let li_class = if is_direct { "pd-ev-direct" } else { "" };
                            let event_sources = profile.citations_by_event.get(&eid);
                            let evidence = profile
                                .evidence_by_event
                                .get(&eid)
                                .filter(|rows| !rows.is_empty())
                                .cloned();

                            rsx! {
                                li { key: "{eid}", class: "{li_class}",
                                    span { class: "pd-ev-date",
                                        {opt_str(&format_event_date(&i18n, event)).unwrap_or_else(|| "--".to_string())}
                                    }
                                    div { class: "pd-ev-body",
                                        div { class: "pd-ev-row",
                                            div {
                                                span { class: "badge", "{event_type_label}" }
                                                if let Some(place) = &place_display {
                                                    " \u{2014} {place}"
                                                }
                                                if !desc.is_empty() {
                                                    span { class: "text-muted", " \u{2014} {desc}" }
                                                }
                                            }
                                        }
                                        div { class: "pd-ev-origin", "{origin_display}" }
                                        if let Some(sources) = event_sources {
                                            div { class: "pd-ev-sources",
                                                {i18n.t("person.sources_section")}
                                                ": {sources.join(\"; \")}"
                                            }
                                        }
                                        // The documents that prove this event use the
                                        // very same gallery, viewer and context menu as
                                        // the profile's main media section.
                                        if let Some(tiles) = evidence {
                                            div { class: "pd-ev-evidence",
                                                MediaGallery {
                                                    tree_id,
                                                    owner: MediaOwner::Event(eid),
                                                    read_only: true,
                                                    compact: true,
                                                    preloaded_tiles: Some(tiles),
                                                    preloaded_bundle: Some(Arc::clone(&profile.bundle.gallery)),
                                                    on_changed: move |()| media_revision += 1,
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

/// The events card while its profile is loading or failed to.
pub(crate) fn timeline_placeholder(i18n: &I18n, error: Option<&str>) -> Element {
    rsx! {
        div { class: "card pd-section",
            div { class: "section-header",
                h2 { style: "font-size: 1.1rem;", {i18n.t("person.events_section")} }
            }
            match error {
                Some(error) => rsx! {
                    div { class: "error-msg", {i18n.t_args("person.load_events_error", &[("error", error)])} }
                },
                None => rsx! {
                    div { class: "loading", {i18n.t("person.loading_events")} }
                },
            }
        }
    }
}

/// The Ancestors card: a small static (no pan/zoom/drag) pedigree fragment.
pub(crate) fn ancestors_section(
    i18n: &I18n,
    pedigree_resource: &Resource<Result<Option<Pedigree>, ApiError>>,
    mini_pedigree: Option<(Uuid, SharedPedigree)>,
    on_navigate: EventHandler<Uuid>,
) -> Element {
    // The assembled fragment decides what to draw; the resource is consulted
    // only to tell "still loading" apart from "failed".
    let body = match mini_pedigree {
        Some((root_person_id, data)) => rsx! {
            crate::components::pedigree_chart::MiniPedigree {
                root_person_id,
                data,
                ancestor_levels: 2,
                descendant_levels: 0,
                on_person_navigate: on_navigate,
            }
        },
        None => match &*pedigree_resource.read() {
            Some(Err(e)) => rsx! {
                div { class: "error-msg", {i18n.t_args("person.load_ancestry_error", &[("error", &e.to_string())])} }
            },
            _ => rsx! {
                div { class: "loading", {i18n.t("person.loading_ancestry")} }
            },
        },
    };
    rsx! {
        div { class: "card pd-section",
            div { class: "section-header",
                h2 { style: "font-size: 1.1rem;", {i18n.t("person.ancestors")} }
            }
            {body}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Language;
    use oxidgene_core::types::FamilySpouse;

    fn spouse(person_id: Uuid, role: SpouseRole, sort_order: i32) -> FamilySpouse {
        FamilySpouse {
            id: Uuid::now_v7(),
            family_id: Uuid::nil(),
            person_id,
            role,
            sort_order,
        }
    }

    #[test]
    fn the_husband_is_always_on_the_left() {
        let (husband, wife) = (Uuid::now_v7(), Uuid::now_v7());
        let spouses = [
            spouse(wife, SpouseRole::Wife, 0),
            spouse(husband, SpouseRole::Husband, 1),
        ];

        assert_eq!(
            couple_sides(&spouses, |_| Sex::Unknown),
            (Some(husband), Some(wife))
        );
    }

    #[test]
    fn partners_are_placed_by_sex_then_recorded_order() {
        let (man, woman) = (Uuid::now_v7(), Uuid::now_v7());
        let sex_of = |id: Uuid| if id == man { Sex::Male } else { Sex::Female };
        let partners = [
            spouse(woman, SpouseRole::Partner, 0),
            spouse(man, SpouseRole::Partner, 1),
        ];
        assert_eq!(couple_sides(&partners, sex_of), (Some(man), Some(woman)));

        let (first, second) = (Uuid::now_v7(), Uuid::now_v7());
        let alike = [
            spouse(second, SpouseRole::Partner, 1),
            spouse(first, SpouseRole::Partner, 0),
        ];
        assert_eq!(
            couple_sides(&alike, |_| Sex::Female),
            (Some(first), Some(second))
        );
    }

    #[test]
    fn a_lone_spouse_keeps_their_side() {
        let wife = Uuid::now_v7();
        assert_eq!(
            couple_sides(&[spouse(wife, SpouseRole::Wife, 0)], |_| Sex::Female),
            (None, Some(wife))
        );
        let husband = Uuid::now_v7();
        assert_eq!(
            couple_sides(&[spouse(husband, SpouseRole::Husband, 0)], |_| Sex::Male),
            (Some(husband), None)
        );
    }

    #[test]
    fn unions_run_from_the_earliest_and_undated_ones_follow_in_order() {
        let date = |y| NaiveDate::from_ymd_opt(y, 1, 1);
        let mut unions = vec![
            ("undated_a", None),
            ("later", date(1880)),
            ("undated_b", None),
            ("earlier", date(1865)),
        ];
        sort_unions_chronologically(&mut unions, |union| union.1);

        let order: Vec<_> = unions.iter().map(|union| union.0).collect();
        assert_eq!(order, ["earlier", "later", "undated_a", "undated_b"]);
    }

    #[test]
    fn fallback_events_keep_their_own_gendered_label() {
        let fr = I18n(Language::Fr);

        assert_eq!(
            fr.t(&vitals_event_key(EventType::Baptism, true, Sex::Male)),
            "Baptisé le"
        );
        assert_eq!(
            fr.t(&vitals_event_key(EventType::Baptism, true, Sex::Female)),
            "Baptisée le"
        );
        assert_eq!(
            fr.t(&vitals_event_key(EventType::Burial, true, Sex::Female)),
            "Inhumée le"
        );
        assert_eq!(
            fr.t(&vitals_event_key(EventType::Burial, false, Sex::Unknown)),
            "Inhumé(e)"
        );
    }
}
