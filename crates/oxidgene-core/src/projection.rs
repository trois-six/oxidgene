//! Denormalized read projections.
//!
//! These are the read-side shapes assembled from the normalized entities:
//! a full person profile, a windowed pedigree, and search results. They are
//! the wire format of the `/api/v1/trees/{id}/profiles` and `/pedigree`
//! endpoints, so both the backend (which builds and persists them) and the
//! frontend (which renders them) depend on this module.
//!
//! The person profile is materialized into the `person_denorm` table on every
//! mutation — see `docs/data-model.md`.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::enums::{Calendar, ChildType, DateQualifier, NameType, Sex, SpouseRole};
use crate::types::{PortraitRef, TagRef};

/// The shape of a stored [`PersonProfile`] payload.
///
/// Stored beside the row in `person_denorm.schema_version`, and compared on
/// every read: a row written by an older build is treated as *absent*, so the
/// paths that already rebuild a missing projection rebuild a stale one too.
///
/// **Raise this whenever a change alters what a payload means.** Adding a field
/// is the usual case, and it is exactly the one that needs it: new fields carry
/// `#[serde(default)]` so old rows keep deserializing, which means they come
/// back looking complete rather than empty. Without a bump the feature is
/// simply invisible on every existing install — that is what happened when
/// `date_qualifier` arrived and every card went on drawing bare years.
///
/// A bump costs one lazy rebuild per tree on first read. Not bumping costs a
/// silent wrong answer, so when in doubt, bump.
///
/// 4: couple-event media imported from Geneanet are filed under the family
/// rather than the spouses, which changes each spouse's `media_count` and
/// fallback primary media.
///
/// 5: the search rows are folded with `search::fold_words` — every script
/// unaccented, `æ` as `ae`, `ß` as `ss`, punctuation as a word break — so
/// every tree's rows are rebuilt to meet queries folded the same way.
///
/// 7: the primary media is a page that can represent somebody
/// (`Media::can_portray`): a picture we hold or a remote one, never a PDF,
/// as the portrait endpoints already decided.
///
/// 8: the profile and each of its events carry the tags put on them
/// (`PersonProfile::tags`, `ProfileEvent::tags`), which were not projected.
///
/// 9: a pedigree node carries its person's tags (`PedigreeNode::tags`), which
/// the events panel shows beside the selected person.
///
/// 10: the payload is compact. Empty values are left out (`None`, an empty
/// list, a zero count, the default qualifier, calendar and child type); a
/// family link no longer copies its marriage out of its `events`, nor its
/// children's count out of `children_ids`, and the profile no longer copies
/// the occupation out of `other_events`; and the stored row no longer repeats
/// its own `person_id`, `tree_id` and build time (see [`PersonProfile`]).
pub const PROJECTION_SCHEMA_VERSION: i32 = 10;

/// Whether `value` is its type's default, for `skip_serializing_if`: a field
/// left at its default is left out of a payload, and `#[serde(default)]`
/// reads it back the same.
fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

// ─── Person profile ─────────────────────────────────────────────────────────

/// A fully denormalized person profile, containing everything needed
/// to display a person card, detail page, or edit modal in a single read.
///
/// The payload is compact: a field holding nothing — `None`, an empty list, a
/// zero count, a default qualifier, calendar or child type — is left out, and
/// reads back as that value. Nothing is stored twice within one profile: a
/// union's marriage is one of its `events` ([`ProfileFamilyLink::marriage`]),
/// its children's count the length of `children_ids`, the occupation one of
/// `other_events` ([`Self::occupation`]).
///
/// The `person_denorm` row stores the payload without `person_id`, `tree_id`
/// and `built_at`, which are its own `person_id`, `tree_id` and `updated_at`
/// columns; reading the row puts them back. Every other surface — REST,
/// GraphQL, MCP — carries them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonProfile {
    // Core identity. Defaulted only so a stored row, which leaves them to its
    // columns, deserializes: see the type's documentation.
    #[serde(default)]
    pub person_id: Uuid,
    #[serde(default)]
    pub tree_id: Uuid,
    pub sex: Sex,

    // Names (denormalized from PersonName)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_name: Option<ProfileName>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub other_names: Vec<ProfileName>,

    // Key life events (denormalized from Event + Place)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub birth: Option<ProfileEvent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub death: Option<ProfileEvent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baptism: Option<ProfileEvent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub burial: Option<ProfileEvent>,
    /// Every other event of the person's own, occupations included.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub other_events: Vec<ProfileEvent>,

    // Family links
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub families_as_spouse: Vec<ProfileFamilyLink>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family_as_child: Option<ProfileChildLink>,

    // Attached media / sources / notes (counts + primary)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_media: Option<ProfileMediaRef>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub media_count: u32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub citation_count: u32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub note_count: u32,

    /// Tags put on the person, sorted by folded label.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<TagRef>,

    // Metadata
    pub updated_at: DateTime<Utc>,
    /// When this projection was last rebuilt from the normalized tables.
    #[serde(default)]
    pub built_at: DateTime<Utc>,
}

impl PersonProfile {
    /// The occupation a card names: the description of the person's last
    /// occupation event, when it has one.
    pub fn occupation(&self) -> Option<&str> {
        self.other_events
            .iter()
            .rfind(|event| event.event_type == crate::enums::EventType::Occupation)
            .and_then(|event| event.description.as_deref())
    }

    /// The event a card should date this person's life *from*: the birth, or
    /// the baptism when no birth was recorded.
    ///
    /// Mirrors GeneWeb's `Gutil.get_birth_death_date`. A parish register very
    /// often holds a baptism and no birth, and "1620" from the baptism is far
    /// more use on a card than an empty line — the baptism's own qualifier
    /// still says how firm it is.
    ///
    /// Note what this deliberately does *not* copy from GeneWeb: there, the
    /// fallback sets a single `approx` flag covering *both* ends of the life,
    /// so a person whose birth came from a baptism gets "ca" stamped on their
    /// death year too — which is how a death recorded as "between 1691 and
    /// 1693" ends up displayed as "ca 1691". Each event keeps its own
    /// precision here.
    /// The fallback triggers on a missing **date**, not a missing event. A
    /// birth recorded with no date at all is extremely common — the register
    /// entry is the baptism, and the birth is an empty stub someone created to
    /// hang a source on. Testing `self.birth.is_none()` would keep that stub
    /// and draw a blank year while a perfectly good "vers 1620" sat unused on
    /// the baptism. GeneWeb tests the date for the same reason
    /// (`Date.od_of_cdate (get_birth p)`).
    pub fn birth_or_baptism(&self) -> Option<&ProfileEvent> {
        pick_dated(self.birth.as_ref(), self.baptism.as_ref())
    }

    /// The event a card should date this person's life *to*: the death, or the
    /// burial when the death carries no date. See [`Self::birth_or_baptism`].
    pub fn death_or_burial(&self) -> Option<&ProfileEvent> {
        pick_dated(self.death.as_ref(), self.burial.as_ref())
    }
}

/// `preferred` when it carries a date, otherwise `fallback` when *it* does,
/// otherwise whichever exists — so a dateless stub still identifies the event
/// for anything that only needs to know it happened.
fn pick_dated<'a>(
    preferred: Option<&'a ProfileEvent>,
    fallback: Option<&'a ProfileEvent>,
) -> Option<&'a ProfileEvent> {
    let dated = |e: &&ProfileEvent| e.date_value.is_some() || e.date_sort.is_some();
    preferred
        .filter(dated)
        .or_else(|| fallback.filter(dated))
        .or(preferred)
        .or(fallback)
}

/// A person name, pre-computed for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileName {
    pub name_id: Uuid,
    pub name_type: NameType,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub given_names: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surname: Option<String>,
}

/// An event with its place name denormalized.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileEvent {
    pub event_id: Uuid,
    pub event_type: crate::enums::EventType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_sort: Option<NaiveDate>,
    /// How precise the date is; `Exact` when absent.
    #[serde(default, skip_serializing_if = "is_default")]
    pub date_qualifier: DateQualifier,
    /// The far end of an `Or`/`Between` range. Without it a "between 11 Nov
    /// 1691 and 20 Aug 1693" reads as a bare "between 1691" — the qualifier
    /// promises a second date the projection could not carry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_value2: Option<String>,
    /// Calendar the date was recorded in, so a Republican or Hebrew date is
    /// not silently re-read as Gregorian; Gregorian when absent.
    #[serde(default, skip_serializing_if = "is_default")]
    pub calendar: Calendar,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The age the record gives for the profile's person at this event —
    /// the event's own for an individual event, this spouse's for a family
    /// one — in canonical GEDCOM form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub age: Option<String>,
    /// Tags put on the event, sorted by folded label.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<TagRef>,
}

/// A family in which this person is a spouse, with the other spouse's info.
///
/// Each spouse's profile carries the union's events, marriage included: the
/// `age` of a family event is the profile's own person's, so the two copies
/// differ, and a profile stays one primary-key read.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileFamilyLink {
    pub family_id: Uuid,
    pub role: SpouseRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spouse_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spouse_display_name: Option<String>,
    /// The spouse's primary surname and given names, kept split so search rows
    /// can filter on one or the other without re-parsing `display_name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spouse_surname: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spouse_given_names: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spouse_sex: Option<Sex>,
    /// All family events (marriage, divorce, annulment, etc.)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<ProfileEvent>,
    /// The union's children, in birth order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children_ids: Vec<Uuid>,
}

impl ProfileFamilyLink {
    /// The union's marriage, the first of its `events` that is one.
    pub fn marriage(&self) -> Option<&ProfileEvent> {
        self.events
            .iter()
            .find(|event| event.event_type == crate::enums::EventType::Marriage)
    }
}

/// The family in which this person is a child, with parent info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileChildLink {
    pub family_id: Uuid,
    #[serde(default, skip_serializing_if = "is_default")]
    pub child_type: ChildType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub father_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub father_display_name: Option<String>,
    /// Split primary name, for the same reason as [`ProfileFamilyLink`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub father_surname: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub father_given_names: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mother_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mother_display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mother_surname: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mother_given_names: Option<String>,
}

/// A reference to a media item (portrait / primary photo).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileMediaRef {
    pub media_id: Uuid,
    /// Set when the portrait is a region of that media rather than the whole
    /// of it — a face in a group photograph. The cropped image is served from
    /// the vignette, not the media.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vignette_id: Option<Uuid>,
    pub file_path: String,
    pub mime_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

// ─── Pedigree ───────────────────────────────────────────────────────────────

/// A windowed pedigree view for a given root person, containing only
/// the persons and edges visible at the requested depth levels.
///
/// Built on demand by walking the family links and joining the reached persons
/// against the `person_denorm` projections — it is never stored. Like a
/// profile, it leaves out a field holding nothing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pedigree {
    pub tree_id: Uuid,
    pub root_person_id: Uuid,
    pub persons: HashMap<Uuid, PedigreeNode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<PedigreeEdge>,
    /// Family events keyed by family_id (marriage, divorce, annulment, etc.)
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub family_events: HashMap<Uuid, Vec<ProfileEvent>>,
    /// Family units with spouse and child membership (covers childless couples
    /// that produce no PedigreeEdge entries).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub families: HashMap<Uuid, PedigreeFamily>,
    pub ancestor_depth_loaded: u32,
    pub descendant_depth_loaded: u32,
    pub built_at: DateTime<Utc>,
}

/// A person node in the pedigree chart, optimized for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PedigreeNode {
    pub person_id: Uuid,
    pub sex: Sex,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub given_names: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surname: Option<String>,
    /// The complete birth event, retaining date precision, calendar and place.
    ///
    /// Falls back to baptism when there is no birth, the way GeneWeb's
    /// `get_birth_death_date` does — see [`PersonProfile::birth_or_baptism`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub birth: Option<ProfileEvent>,
    /// The whole death event, falling back to burial. See [`Self::birth`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub death: Option<ProfileEvent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occupation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_media_path: Option<String>,
    pub generation: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sosa_number: Option<u64>,
    /// Where the card's portrait is drawn from, when the person has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub portrait: Option<PortraitRef>,
    /// Whether the person is the tree's SOSA root or one of its ancestors,
    /// whatever the window: the chart marks them without loading the whole
    /// ancestry.
    #[serde(default, skip_serializing_if = "is_default")]
    pub sosa_ancestor: bool,
    /// The tags put on the person, in the vocabulary's order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<TagRef>,
}

/// A parent-child edge in the pedigree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PedigreeEdge {
    pub parent_id: Uuid,
    pub child_id: Uuid,
    pub family_id: Uuid,
    pub edge_type: ChildType,
}

/// A family unit in the pedigree, capturing spouse and child relationships
/// independently of parent→child edges (which miss childless couples).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PedigreeFamily {
    pub family_id: Uuid,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spouse_ids: Vec<Uuid>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children_ids: Vec<Uuid>,
    /// The children of the family who have no node of their own.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<PedigreeFamilyMember>,
}

/// A child of one of the window's families who has no node of their own:
/// an ancestor's brother or sister, a child of an ancestor's other union, the
/// root's own sibling, a child below the deepest generation loaded.
///
/// It carries what a chart draws for such a person and nothing more: their
/// name, sex and lifespan, on the card of one of the root's siblings and as
/// the context of their rows in the events panel, and the rows themselves —
/// their birth and death. A pedigree of ten generations holds thousands of
/// them, and their whole events were most of its weight.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PedigreeFamilyMember {
    pub person_id: Uuid,
    pub sex: Sex,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub given_names: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surname: Option<String>,
    /// The birth, else the baptism, as [`PersonProfile::birth_or_baptism`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub birth: Option<PedigreeMemberEvent>,
    /// The death, else the burial, as [`PersonProfile::death_or_burial`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub death: Option<PedigreeMemberEvent>,
}

impl PedigreeFamilyMember {
    /// The member `person` makes.
    pub fn of(person: &PersonProfile) -> Self {
        let name = person.primary_name.as_ref();
        Self {
            person_id: person.person_id,
            sex: person.sex,
            given_names: name.and_then(|n| n.given_names.clone()),
            surname: name.and_then(|n| n.surname.clone()),
            birth: person.birth_or_baptism().map(PedigreeMemberEvent::of),
            death: person.death_or_burial().map(PedigreeMemberEvent::of),
        }
    }
}

/// A member's birth or death as the events panel lists it: its kind, its
/// whole date — precision, far end and calendar, as a node's — the line
/// under it, and its tags. No ids: the panel only shows the row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PedigreeMemberEvent {
    pub event_type: crate::enums::EventType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_sort: Option<NaiveDate>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub date_qualifier: DateQualifier,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_value2: Option<String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub calendar: Calendar,
    /// The line the panel writes under the event: its description, else its
    /// place's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<TagRef>,
}

impl PedigreeMemberEvent {
    /// What the panel shows of `event`.
    pub fn of(event: &ProfileEvent) -> Self {
        Self {
            event_type: event.event_type,
            date_value: event.date_value.clone(),
            date_sort: event.date_sort,
            date_qualifier: event.date_qualifier,
            date_value2: event.date_value2.clone(),
            calendar: event.calendar,
            place: event
                .description
                .clone()
                .or_else(|| event.place_name.clone()),
            tags: event.tags.clone(),
        }
    }
}

/// The result of an incremental pedigree expansion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PedigreeDelta {
    pub new_nodes: Vec<PedigreeNode>,
    pub new_edges: Vec<PedigreeEdge>,
    pub ancestor_depth_loaded: u32,
    pub descendant_depth_loaded: u32,
}

/// Direction for pedigree expansion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PedigreeDirection {
    Ancestors,
    Descendants,
}

// ─── Search ─────────────────────────────────────────────────────────────────
//
// Since Sprint E.6 the search index lives in the database itself (SQLite FTS5
// virtual table / plain PostgreSQL table `person_search_fts`). These types
// remain as the API wire shape for search results.

/// A single search result entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchEntry {
    pub person_id: Uuid,
    pub sex: Sex,
    // Searchable fields (lowercased, accent-folded)
    pub surname_normalized: String,
    pub given_names_normalized: String,
    pub maiden_name_normalized: Option<String>,
    // Display fields (original casing) — always populated, so callers never
    // need to guess the surname/given-name split by parsing `display_name`.
    pub surname: String,
    pub given_names: String,
    pub display_name: String,
    // Key dates for result display. The year stays bare and the precision
    // rides beside it, as it does on a `ProfileEvent`: a year shown alone must
    // keep its precision, and only the UI knows how to say "ca" in the
    // reader's language.
    pub birth_year: Option<String>,
    #[serde(default)]
    pub birth_qualifier: DateQualifier,
    pub birth_place: Option<String>,
    pub death_year: Option<String>,
    #[serde(default)]
    pub death_qualifier: DateQualifier,
    // Close relatives, so a result reads as a person rather than as a name:
    // "married to X", or "child of X and Y" when there is no spouse.
    #[serde(default)]
    pub spouse_names: Vec<String>,
    #[serde(default)]
    pub father_name: Option<String>,
    #[serde(default)]
    pub mother_name: Option<String>,
    #[serde(default)]
    pub children_count: u32,
    // For sorting / filtering
    pub date_sort: Option<NaiveDate>,
    /// Where the row's portrait is drawn from, when the person has one.
    /// Filled where a row leaves the API, not in the search index.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub portrait: Option<PortraitRef>,
    /// Tags put on the person, sorted by folded label. Read from the
    /// projection, or from the tag tables for a search row, which the search
    /// index does not hold.
    #[serde(default)]
    pub tags: Vec<TagRef>,
}

/// Paginated search results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub entries: Vec<SearchEntry>,
    pub total_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::EventType;

    fn event(event_type: EventType, description: Option<&str>) -> ProfileEvent {
        ProfileEvent {
            event_id: Uuid::now_v7(),
            event_type,
            date_value: None,
            date_sort: None,
            date_qualifier: DateQualifier::Exact,
            date_value2: None,
            calendar: Calendar::Gregorian,
            place_name: None,
            place_id: None,
            description: description.map(str::to_owned),
            age: None,
            tags: Vec::new(),
        }
    }

    #[test]
    fn an_empty_event_is_only_its_identity() {
        let event = event(EventType::Birth, None);
        let json = serde_json::to_value(&event).unwrap();
        let mut keys: Vec<&str> = json
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["event_id", "event_type"]);
        let back: ProfileEvent = serde_json::from_value(json).unwrap();
        assert_eq!(back.date_qualifier, DateQualifier::Exact);
        assert_eq!(back.calendar, Calendar::Gregorian);
        assert!(back.tags.is_empty());
    }

    #[test]
    fn a_union_finds_its_marriage_among_its_events() {
        let marriage = event(EventType::Marriage, None);
        let link = ProfileFamilyLink {
            family_id: Uuid::now_v7(),
            role: SpouseRole::Wife,
            spouse_id: None,
            spouse_display_name: None,
            spouse_surname: None,
            spouse_given_names: None,
            spouse_sex: None,
            events: vec![event(EventType::MarriageBann, None), marriage.clone()],
            children_ids: Vec::new(),
        };
        assert_eq!(link.marriage().map(|e| e.event_id), Some(marriage.event_id));
        let json = serde_json::to_string(&link).unwrap();
        assert_eq!(json.matches("\"event_type\":\"marriage\"").count(), 1);
    }

    #[test]
    fn the_occupation_is_the_last_occupation_event() {
        let mut events = vec![
            event(EventType::Occupation, Some("Weaver")),
            event(EventType::Residence, Some("Elsewhere")),
            event(EventType::Occupation, Some("Miller")),
        ];
        let profile = |other_events: Vec<ProfileEvent>| PersonProfile {
            person_id: Uuid::now_v7(),
            tree_id: Uuid::now_v7(),
            sex: Sex::Unknown,
            primary_name: None,
            other_names: Vec::new(),
            birth: None,
            death: None,
            baptism: None,
            burial: None,
            other_events,
            families_as_spouse: Vec::new(),
            family_as_child: None,
            primary_media: None,
            media_count: 0,
            citation_count: 0,
            note_count: 0,
            tags: Vec::new(),
            updated_at: Utc::now(),
            built_at: Utc::now(),
        };
        assert_eq!(profile(events.clone()).occupation(), Some("Miller"));
        events.truncate(2);
        assert_eq!(profile(events).occupation(), Some("Weaver"));
        assert_eq!(profile(Vec::new()).occupation(), None);
    }

    #[test]
    fn a_member_event_keeps_the_date_and_the_line_under_it() {
        let mut baptism = event(EventType::Baptism, None);
        baptism.date_value = Some("ABT 1702".into());
        baptism.date_qualifier = DateQualifier::About;
        baptism.place_name = Some("Fictland".into());
        baptism.place_id = Some(Uuid::now_v7());
        baptism.age = Some("2d".into());
        let member = PedigreeMemberEvent::of(&baptism);
        assert_eq!(member.event_type, EventType::Baptism);
        assert_eq!(member.date_qualifier, DateQualifier::About);
        assert_eq!(member.place.as_deref(), Some("Fictland"));
        let json = serde_json::to_value(&member).unwrap();
        let mut keys: Vec<&str> = json
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            ["date_qualifier", "date_value", "event_type", "place"]
        );

        // A description is the line the panel writes, before the place.
        baptism.description = Some("At the chapel".into());
        assert_eq!(
            PedigreeMemberEvent::of(&baptism).place.as_deref(),
            Some("At the chapel")
        );
    }
}
