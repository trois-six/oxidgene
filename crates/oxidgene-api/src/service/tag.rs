//! Tag writes and reads: the tree's vocabulary, and the tags put on records of
//! every kind (docs/tags.md). REST and GraphQL both go through here.
//!
//! A tag change is recorded in the audit log, never versioned, and refreshes
//! the projections of the persons whose profiles show it, in the transaction
//! that makes the change.

use std::collections::HashMap;

use oxidgene_core::collections::sorted_unique;
use oxidgene_core::enums::{Calendar, DateQualifier, EventType};
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::history::{AuditAction, AuditCategory, AuditDetails, AuditEntity, AuditSubject};
use oxidgene_core::types::{Citation, Connection, Edge, Event, FamilySpouse, RecordKind};
use oxidgene_db::entities::{event, record_tag};
use oxidgene_db::repo::{
    CitationRepo, DictionaryRepo, EventRepo, FamilySpouseRepo, MediaRepo, NoteRepo,
    PaginationParams, PersonUsageEntry, PlaceRepo, RecordTagRepo, RepositoryRepo, SourceRepo,
    TagRepo, TagUsage, checked_color, db_err, display_names,
};
use oxidgene_db::sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter,
};
use serde::Serialize;
use uuid::Uuid;

use crate::profile::{ProfileService, invalidation};
use crate::service::history::Change;
use crate::service::scope::{
    TreeResource, begin_tx, commit_tx, require_live_tree, require_tree_resource,
};

/// The vocabulary of `tree_id` with every tag's counts, sorted by folded label.
pub async fn list(db: &DatabaseConnection, tree_id: Uuid) -> Result<Vec<TagUsage>, OxidGeneError> {
    TagRepo::list(db, tree_id).await
}

/// The record kind named by a request, spelled as `RecordKind::as_str` spells
/// it. An unknown spelling is `not_found` (docs/api.md, Tags) on both
/// surfaces; the kind is not an id, so the error carries the nil one.
pub fn parse_kind(value: &str) -> Result<RecordKind, OxidGeneError> {
    value.parse().map_err(|_| OxidGeneError::NotFound {
        entity: "RecordKind",
        id: Uuid::nil(),
    })
}

/// Create the tag `label` in `tree_id`, or return the tag its label folds to.
/// The flag tells whether this call created it. `color`, when given, is the
/// colour of a new tag; an existing tag keeps its own.
pub async fn create(
    db: &DatabaseConnection,
    tree_id: Uuid,
    label: &str,
    color: Option<i16>,
) -> Result<(TagUsage, bool), OxidGeneError> {
    if let Some(color) = color {
        checked_color(color)?;
    }
    let txn = begin_tx(db).await?;
    require_live_tree(&txn, tree_id).await?;
    let (tag, created) = TagRepo::create(&txn, tree_id, label).await?;
    if created {
        if let Some(color) = color {
            TagRepo::update(&txn, tree_id, tag.id, None, Some(color)).await?;
        }
        Change::create(tree_id, AuditEntity::Tag, tag.id)
            .label(tag.label.clone())
            .record_unversioned(&txn)
            .await?;
    }
    let usage = usage_of(&txn, tree_id, tag.id).await?;
    commit_tx(txn).await?;
    Ok((usage, created))
}

/// Rename and/or recolour tag `id` of `tree_id`. A label folding to another
/// tag's is a conflict. Persons showing the tag are refreshed when it changed.
pub async fn update(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    id: Uuid,
    label: Option<&str>,
    color: Option<i16>,
) -> Result<TagUsage, OxidGeneError> {
    let txn = begin_tx(db).await?;
    require_live_tree(&txn, tree_id).await?;
    let before = TagRepo::get(&txn, tree_id, id).await?;
    let after = TagRepo::update(&txn, tree_id, id, label, color).await?;
    if after.label != before.label || after.color != before.color {
        refresh_carriers(&txn, profiles, tree_id, id).await?;
        let mut details = AuditDetails::default();
        if after.label != before.label {
            details.new_label = Some(after.label.clone());
        }
        Change::update(tree_id, AuditEntity::Tag, id)
            .label(before.label.clone())
            .details(details)
            .record_unversioned(&txn)
            .await?;
    }
    let usage = usage_of(&txn, tree_id, id).await?;
    commit_tx(txn).await?;
    Ok(usage)
}

/// Move every link of tag `from` onto tag `into` and delete `from`. Returns the
/// merged `into` with its counts.
pub async fn merge(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    from: Uuid,
    into: Uuid,
) -> Result<TagUsage, OxidGeneError> {
    let txn = begin_tx(db).await?;
    require_live_tree(&txn, tree_id).await?;
    let source = TagRepo::get(&txn, tree_id, from).await?;
    let target = TagRepo::get(&txn, tree_id, into).await?;
    let moved = TagRepo::merge(&txn, tree_id, from, into).await?;
    refresh_carriers(&txn, profiles, tree_id, into).await?;
    Change::delete(tree_id, AuditEntity::Tag, from)
        .label(source.label)
        .details(AuditDetails {
            count: Some(moved),
            other_label: Some(target.label),
            ..AuditDetails::default()
        })
        .record_unversioned(&txn)
        .await?;
    let usage = usage_of(&txn, tree_id, into).await?;
    commit_tx(txn).await?;
    Ok(usage)
}

/// Delete tag `id` of `tree_id` and every link to it. Returns the number of
/// links removed.
pub async fn delete(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    id: Uuid,
) -> Result<u64, OxidGeneError> {
    let txn = begin_tx(db).await?;
    require_live_tree(&txn, tree_id).await?;
    let tag = TagRepo::get(&txn, tree_id, id).await?;
    // Read before the links go: afterwards nothing says whose profile shows it.
    let carriers = carrier_persons(&txn, tree_id, id).await?;
    let removed = TagRepo::delete(&txn, tree_id, id).await?;
    profiles
        .invalidate_for_mutation(&txn, tree_id, &carriers)
        .await?;
    Change::delete(tree_id, AuditEntity::Tag, id)
        .label(tag.label)
        .details(AuditDetails {
            count: Some(removed),
            ..AuditDetails::default()
        })
        .record_unversioned(&txn)
        .await?;
    commit_tx(txn).await?;
    Ok(removed)
}

/// Put tag `tag_id` on record `record_id` of `kind`. Idempotent. A page of a
/// document is tagged on the document.
pub async fn tag_record(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    tag_id: Uuid,
    kind: RecordKind,
    record_id: Uuid,
) -> Result<(), OxidGeneError> {
    set_record_tag(db, profiles, tree_id, tag_id, kind, record_id, true).await
}

/// Take tag `tag_id` off record `record_id` of `kind`. Idempotent.
pub async fn untag_record(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    tag_id: Uuid,
    kind: RecordKind,
    record_id: Uuid,
) -> Result<(), OxidGeneError> {
    set_record_tag(db, profiles, tree_id, tag_id, kind, record_id, false).await
}

/// A live record carrying a tag, as the tag's list shows it: its id, its kind,
/// a label to show, and for an event, citation or note the person or family
/// it belongs to. For a person, its name parts and lifespan, as the
/// dictionary's usage lists show them. For an event, its own details; for a
/// citation, its source and page; for an event, citation or note, and for a
/// couple, the persons it shows.
#[derive(Debug, Clone, Serialize)]
pub struct TaggedRecord {
    pub id: Uuid,
    pub kind: RecordKind,
    /// The text the record is named by: a person's name is in `person`, an
    /// event's in `event`, a citation's source in `citation`. Empty when the
    /// record has no text to show (a note with blank text, a citation whose
    /// source is gone, a couple).
    pub label: String,
    pub person_id: Option<Uuid>,
    pub family_id: Option<Uuid>,
    pub person: Option<PersonUsageEntry>,
    /// For an event: its type, dates, place and description, without sources.
    pub event: Option<TaggedEvent>,
    /// For a citation: its source's title and its page, without the event.
    pub citation: Option<TaggedCitation>,
    /// For an event, citation or note: the person it belongs to, or the two
    /// spouses of the couple it belongs to. For a couple: its spouses, in
    /// their order. Empty when nothing owns the record; `None` for the other
    /// kinds.
    pub persons: Option<Vec<PersonUsageEntry>>,
}

/// An event as a tag's list shows it: what happened, when and where, and what
/// the event says about it. Its sources are not part of it.
#[derive(Debug, Clone, Serialize)]
pub struct TaggedEvent {
    pub event_type: EventType,
    pub date_value: Option<String>,
    pub date_qualifier: DateQualifier,
    pub date_value2: Option<String>,
    pub calendar: Calendar,
    pub place_name: Option<String>,
    pub description: Option<String>,
}

/// A citation as a tag's list shows it: the title of its source, and its page
/// when it has one. Both are `None` when the source is gone.
#[derive(Debug, Clone, Serialize)]
pub struct TaggedCitation {
    pub source_title: Option<String>,
    pub page: Option<String>,
}

/// One page of the live records of `kind` carrying tag `tag_id` of `tree_id`.
/// The reads behind the page are a fixed number of grouped queries, whatever
/// the number of records on it.
pub async fn tagged_records(
    db: &DatabaseConnection,
    tree_id: Uuid,
    tag_id: Uuid,
    kind: RecordKind,
    params: &PaginationParams,
) -> Result<Connection<TaggedRecord>, OxidGeneError> {
    TagRepo::get(db, tree_id, tag_id).await?;
    let page = RecordTagRepo::records_of_tag(db, tree_id, tag_id, kind, params).await?;
    let ids: Vec<Uuid> = page.edges.iter().map(|edge| edge.node).collect();
    let details = page_details(db, tree_id, kind, &ids).await?;
    let persons = persons_of(db, kind, &ids, &details.owners).await?;
    let edges = page
        .edges
        .into_iter()
        .map(|edge| Edge {
            node: tagged_node(kind, edge.node, &details, &persons),
            cursor: edge.cursor,
        })
        .collect();
    Ok(Connection {
        edges,
        page_info: page.page_info,
        total_count: page.total_count,
    })
}

/// What a page of records of one kind shows about each of them, read in a
/// fixed number of grouped queries.
struct PageDetails {
    labels: HashMap<Uuid, String>,
    owners: HashMap<Uuid, (AuditSubject, Uuid)>,
    events: HashMap<Uuid, TaggedEvent>,
    citations: HashMap<Uuid, TaggedCitation>,
}

/// The labels, owners and details of the page `ids` of records of `kind`. An
/// event or a citation is read once: its owner, its label and its details all
/// come from the same rows.
async fn page_details(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    kind: RecordKind,
    ids: &[Uuid],
) -> Result<PageDetails, OxidGeneError> {
    let event_rows = match kind {
        RecordKind::Event => EventRepo::get_many_untagged(db, ids).await?,
        _ => Vec::new(),
    };
    let citation_rows = match kind {
        RecordKind::Citation => CitationRepo::get_many_untagged(db, ids).await?,
        _ => Vec::new(),
    };
    let labels = labels_of(db, tree_id, kind, ids, &event_rows).await?;
    let owners = match kind {
        RecordKind::Person | RecordKind::Family => HashMap::new(),
        RecordKind::Event => event_owner_map(&event_rows),
        RecordKind::Citation => citation_owner_map(db, &citation_rows).await?,
        _ => subjects_of(db, kind, ids).await?,
    };
    let events = match kind {
        RecordKind::Event => event_details(db, event_rows).await?,
        _ => HashMap::new(),
    };
    let citations = match kind {
        RecordKind::Citation => citation_details(db, tree_id, citation_rows).await?,
        _ => HashMap::new(),
    };
    Ok(PageDetails {
        labels,
        owners,
        events,
        citations,
    })
}

/// The entry of record `id` of `kind` on a page, from its details.
fn tagged_node(
    kind: RecordKind,
    id: Uuid,
    details: &PageDetails,
    persons: &PagePersons,
) -> TaggedRecord {
    let (person_id, family_id) = match details.owners.get(&id) {
        Some((AuditSubject::Person, person)) => (Some(*person), None),
        Some((AuditSubject::Family, family)) => (None, Some(*family)),
        _ => (None, None),
    };
    let citation = details.citations.get(&id).cloned();
    let label = citation
        .as_ref()
        .and_then(|citation| citation.source_title.clone())
        .or_else(|| details.labels.get(&id).cloned())
        .unwrap_or_default();
    TaggedRecord {
        id,
        kind,
        person_id,
        family_id,
        person: match kind {
            RecordKind::Person => persons.entries.get(&id).cloned(),
            _ => None,
        },
        event: details.events.get(&id).cloned(),
        label,
        citation,
        persons: shows_persons(kind).then(|| persons.list_of(&id)),
    }
}

/// Whether a record of `kind` shows persons in the tag's list: a couple its
/// spouses, an event, citation or note its owner or owning couple.
fn shows_persons(kind: RecordKind) -> bool {
    matches!(
        kind,
        RecordKind::Family | RecordKind::Event | RecordKind::Citation | RecordKind::Note
    )
}

/// The persons a page of records shows, each resolved once.
struct PagePersons {
    /// The usage entry of every person named in `members`, or of every
    /// person record on the page.
    entries: HashMap<Uuid, PersonUsageEntry>,
    /// The persons each record shows, in their order.
    members: HashMap<Uuid, Vec<Uuid>>,
}

impl PagePersons {
    /// The entries of the persons a record shows, empty when it shows none.
    fn list_of(&self, id: &Uuid) -> Vec<PersonUsageEntry> {
        self.members
            .get(id)
            .into_iter()
            .flatten()
            .filter_map(|person| self.entries.get(person).cloned())
            .collect()
    }
}

/// The persons of a page of records of `kind`, read in a fixed number of
/// grouped queries: the persons themselves for a page of persons, else the
/// spouses of the couples the records show.
async fn persons_of(
    db: &impl ConnectionTrait,
    kind: RecordKind,
    ids: &[Uuid],
    owners: &HashMap<Uuid, (AuditSubject, Uuid)>,
) -> Result<PagePersons, OxidGeneError> {
    let families: Vec<Uuid> = match kind {
        RecordKind::Family => ids.to_vec(),
        _ => owners
            .values()
            .filter(|(subject, _)| matches!(subject, AuditSubject::Family))
            .map(|(_, id)| *id)
            .collect(),
    };
    let couples = couples_of(db, &families).await?;
    let members: HashMap<Uuid, Vec<Uuid>> = ids
        .iter()
        .filter_map(|id| {
            let owner = match kind {
                RecordKind::Family => Some((AuditSubject::Family, *id)),
                _ => owners.get(id).cloned(),
            };
            let shown = match owner {
                Some((AuditSubject::Person, person)) => vec![person],
                Some((AuditSubject::Family, family)) => {
                    couples.get(&family).cloned().unwrap_or_default()
                }
                _ => Vec::new(),
            };
            shows_persons(kind).then_some((*id, shown))
        })
        .collect();
    let wanted: Vec<Uuid> = match kind {
        RecordKind::Person => ids.to_vec(),
        _ => members.values().flatten().copied().collect(),
    };
    let entries = DictionaryRepo::resolve_person_usage_entries(db, &sorted_unique(wanted))
        .await?
        .into_iter()
        .map(|entry| (entry.person_id, entry))
        .collect();
    Ok(PagePersons { entries, members })
}

/// The spouses of each of `families`, in their order.
async fn couples_of(
    db: &impl ConnectionTrait,
    families: &[Uuid],
) -> Result<HashMap<Uuid, Vec<Uuid>>, OxidGeneError> {
    let mut spouses = FamilySpouseRepo::list_by_families(db, families).await?;
    spouses.sort_by_key(|spouse| spouse.sort_order);
    let mut couples: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for spouse in spouses {
        couples
            .entry(spouse.family_id)
            .or_default()
            .push(spouse.person_id);
    }
    Ok(couples)
}

/// The details of each of `events`, its place named.
async fn event_details(
    db: &impl ConnectionTrait,
    events: Vec<Event>,
) -> Result<HashMap<Uuid, TaggedEvent>, OxidGeneError> {
    let place_ids: Vec<Uuid> = events.iter().filter_map(|event| event.place_id).collect();
    let places: HashMap<Uuid, String> = PlaceRepo::get_many_untagged(db, &place_ids)
        .await?
        .into_iter()
        .map(|place| (place.id, place.name))
        .collect();
    Ok(events
        .into_iter()
        .map(|event| {
            let detail = TaggedEvent {
                event_type: event.event_type,
                date_value: event.date_value,
                date_qualifier: event.date_qualifier,
                date_value2: event.date_value2,
                calendar: event.calendar,
                place_name: event.place_id.and_then(|id| places.get(&id).cloned()),
                description: event.description.filter(|text| !text.trim().is_empty()),
            };
            (event.id, detail)
        })
        .collect())
}

/// The label to show for each of `ids`, all records of `kind`; `events` are
/// their event rows when `kind` is events. A record with no text to show is
/// absent from the map.
async fn labels_of(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    kind: RecordKind,
    ids: &[Uuid],
    events: &[Event],
) -> Result<HashMap<Uuid, String>, OxidGeneError> {
    let mut labels: HashMap<Uuid, String> = match kind {
        RecordKind::Person => display_names(db, ids).await?,
        RecordKind::Family => family_labels(db, ids).await?,
        RecordKind::Place => PlaceRepo::get_many_untagged(db, ids)
            .await?
            .into_iter()
            .map(|place| (place.id, place.name))
            .collect(),
        RecordKind::Source => SourceRepo::get_many(db, tree_id, ids)
            .await?
            .into_iter()
            .map(|source| (source.id, source.title))
            .collect(),
        RecordKind::Repository => RepositoryRepo::get_many_untagged(db, ids)
            .await?
            .into_iter()
            .map(|repository| (repository.id, repository.name))
            .collect(),
        RecordKind::Media => MediaRepo::get_many(db, ids)
            .await?
            .into_iter()
            .map(|media| (media.id, media.title.unwrap_or(media.file_name)))
            .collect(),
        RecordKind::Event => event_labels(events),
        // A citation is named by its source, read with its details.
        RecordKind::Citation => HashMap::new(),
        RecordKind::Note => note_labels(db, ids).await?,
    };
    labels.retain(|_, label| !label.trim().is_empty());
    Ok(labels)
}

/// A couple's label: its spouses' names, in their order, joined by a middle
/// dot. This is the one joined line the API writes; the UI shows the spouses
/// themselves and never this label.
async fn family_labels(
    db: &impl ConnectionTrait,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, String>, OxidGeneError> {
    let spouses = FamilySpouseRepo::list_by_families(db, ids).await?;
    let persons: Vec<Uuid> = spouses.iter().map(|spouse| spouse.person_id).collect();
    let names = display_names(db, &persons).await?;
    let mut by_family: HashMap<Uuid, Vec<&FamilySpouse>> = HashMap::new();
    for spouse in &spouses {
        by_family.entry(spouse.family_id).or_default().push(spouse);
    }
    let mut labels = HashMap::new();
    for (family_id, mut members) in by_family {
        members.sort_by_key(|spouse| spouse.sort_order);
        let parts: Vec<&str> = members
            .iter()
            .filter_map(|spouse| names.get(&spouse.person_id).map(String::as_str))
            .collect();
        labels.insert(family_id, parts.join(" \u{00b7} "));
    }
    Ok(labels)
}

/// An event's label: its type, as the wire spells it.
fn event_labels(events: &[Event]) -> HashMap<Uuid, String> {
    events
        .iter()
        .filter_map(|event| {
            let spelling = serde_json::to_value(event.event_type)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))?;
            Some((event.id, spelling))
        })
        .collect()
}
/// The longest start of a note's text that a tag's list shows, in characters.
const NOTE_EXCERPT_CHARS: usize = 100;

/// A note's label: the start of its text, its first line cut to
/// [`NOTE_EXCERPT_CHARS`] characters, with an ellipsis when cut.
async fn note_labels(
    db: &impl ConnectionTrait,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, String>, OxidGeneError> {
    let notes = NoteRepo::get_many_untagged(db, ids).await?;
    Ok(notes
        .into_iter()
        .filter_map(|note| note_excerpt(&note.text).map(|excerpt| (note.id, excerpt)))
        .collect())
}

/// The first non-empty line of `text`, cut to [`NOTE_EXCERPT_CHARS`]
/// characters, with an ellipsis when it was cut.
fn note_excerpt(text: &str) -> Option<String> {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    if line.chars().count() <= NOTE_EXCERPT_CHARS {
        return Some(line.to_owned());
    }
    let cut: String = line.chars().take(NOTE_EXCERPT_CHARS).collect();
    Some(format!("{cut}\u{2026}"))
}

/// The details of each of `citations`: the title of its source, which is
/// absent when the source is gone, and its page, when it has one.
async fn citation_details(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    citations: Vec<Citation>,
) -> Result<HashMap<Uuid, TaggedCitation>, OxidGeneError> {
    let source_ids: Vec<Uuid> = citations
        .iter()
        .map(|citation| citation.source_id)
        .collect();
    let titles: HashMap<Uuid, String> = SourceRepo::get_many(db, tree_id, &source_ids)
        .await?
        .into_iter()
        .map(|source| (source.id, source.title))
        .collect();
    Ok(citations
        .into_iter()
        .map(|citation| {
            let detail = TaggedCitation {
                source_title: titles
                    .get(&citation.source_id)
                    .filter(|title| !title.trim().is_empty())
                    .cloned(),
                page: citation
                    .page
                    .map(|page| page.trim().to_owned())
                    .filter(|page| !page.is_empty()),
            };
            (citation.id, detail)
        })
        .collect())
}

async fn set_record_tag(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    tag_id: Uuid,
    kind: RecordKind,
    record_id: Uuid,
    present: bool,
) -> Result<(), OxidGeneError> {
    let txn = begin_tx(db).await?;
    require_live_tree(&txn, tree_id).await?;
    let tag = TagRepo::get(&txn, tree_id, tag_id).await?;
    let record_id = tagged_record(&txn, tree_id, kind, record_id).await?;
    let changed = if present {
        RecordTagRepo::add(&txn, tree_id, tag.id, kind, record_id).await?
    } else {
        RecordTagRepo::remove(&txn, tree_id, tag.id, kind, record_id).await?
    };
    if changed {
        refresh_record_persons(&txn, profiles, tree_id, kind, record_id).await?;
        let action = if present {
            AuditAction::Create
        } else {
            AuditAction::Delete
        };
        let mut change = Change::new(tree_id, action, AuditEntity::RecordTag, tag.id)
            .category(AuditCategory::of_record_tag(kind));
        if let Some((subject, id)) = subject_of(&txn, kind, record_id).await? {
            change = change.about(subject, id);
        }
        change.record_unversioned(&txn).await?;
    }
    commit_tx(txn).await
}

/// Refresh the projections a link on record `record_id` of `kind` shows: the
/// person's own, or those of the persons an event belongs to.
async fn refresh_record_persons(
    db: &impl ConnectionTrait,
    profiles: &ProfileService,
    tree_id: Uuid,
    kind: RecordKind,
    record_id: Uuid,
) -> Result<(), OxidGeneError> {
    let affected = match kind {
        RecordKind::Person => vec![record_id],
        RecordKind::Event => {
            let event = EventRepo::get(db, record_id).await?;
            invalidation::affected_persons_for_event(db, event.person_id, event.family_id).await?
        }
        _ => return Ok(()),
    };
    profiles
        .invalidate_for_mutation(db, tree_id, &affected)
        .await
}

/// The record a tag is put on for a request about record `id` of `kind`: the
/// record itself, or for a page of a document, the document. Fails with
/// `NotFound` for a record of another tree or a deleted one.
async fn tagged_record(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    kind: RecordKind,
    id: Uuid,
) -> Result<Uuid, OxidGeneError> {
    let resource = match kind {
        RecordKind::Person => TreeResource::Person,
        RecordKind::Family => TreeResource::Family,
        RecordKind::Event => TreeResource::Event,
        RecordKind::Place => TreeResource::Place,
        RecordKind::Source => TreeResource::Source,
        RecordKind::Citation => TreeResource::Citation,
        RecordKind::Repository => TreeResource::Repository,
        RecordKind::Media => TreeResource::Media,
        RecordKind::Note => TreeResource::Note,
    };
    require_tree_resource(db, tree_id, resource, id).await?;
    if kind == RecordKind::Media {
        let media = MediaRepo::get(db, id).await?;
        return Ok(media.parent_media_id.unwrap_or(media.id));
    }
    Ok(id)
}

/// The record a tag change on `id` of `kind` is about in the audit log: the
/// record itself, or for an event, citation or note, the person or family
/// owning it, else the source, repository or media a note is about. `None`
/// when nothing owns it.
async fn subject_of(
    db: &impl ConnectionTrait,
    kind: RecordKind,
    id: Uuid,
) -> Result<Option<(AuditSubject, Uuid)>, OxidGeneError> {
    Ok(subjects_of(db, kind, &[id]).await?.remove(&id))
}

/// [`subject_of`] for each of `ids`, all of one kind, in a fixed number of
/// grouped reads. A record with no subject, or one that does not exist, is
/// absent from the map.
async fn subjects_of(
    db: &impl ConnectionTrait,
    kind: RecordKind,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, (AuditSubject, Uuid)>, OxidGeneError> {
    let own = |subject: AuditSubject| -> HashMap<Uuid, (AuditSubject, Uuid)> {
        ids.iter().map(|id| (*id, (subject, *id))).collect()
    };
    Ok(match kind {
        RecordKind::Person => own(AuditSubject::Person),
        RecordKind::Family => own(AuditSubject::Family),
        RecordKind::Place => own(AuditSubject::Place),
        RecordKind::Source => own(AuditSubject::Source),
        RecordKind::Repository => own(AuditSubject::Repository),
        RecordKind::Media => own(AuditSubject::Media),
        RecordKind::Event => event_owner_map(&EventRepo::get_many_untagged(db, ids).await?),
        RecordKind::Citation => {
            citation_owner_map(db, &CitationRepo::get_many_untagged(db, ids).await?).await?
        }
        RecordKind::Note => {
            let notes = NoteRepo::get_many_untagged(db, ids).await?;
            let event_ids: Vec<Uuid> = notes.iter().filter_map(|n| n.event_id).collect();
            let owners = event_owners(db, &event_ids).await?;
            notes
                .into_iter()
                .filter_map(|note| {
                    let event_owner = note
                        .event_id
                        .and_then(|event_id| owners.get(&event_id).cloned());
                    owner_subject(note.person_id, note.family_id, event_owner)
                        .or(note
                            .source_id
                            .map(|source_id| (AuditSubject::Source, source_id)))
                        .or(note
                            .repository_id
                            .map(|repository_id| (AuditSubject::Repository, repository_id)))
                        .or(note
                            .media_id
                            .map(|media_id| (AuditSubject::Media, media_id)))
                        .map(|subject| (note.id, subject))
                })
                .collect()
        }
    })
}

/// The person, else the family, of each of `events`.
fn event_owner_map(events: &[Event]) -> HashMap<Uuid, (AuditSubject, Uuid)> {
    events
        .iter()
        .filter_map(|event| {
            owner_subject(event.person_id, event.family_id, None).map(|owner| (event.id, owner))
        })
        .collect()
}

/// The person, else the family, of each live event among `ids`.
async fn event_owners(
    db: &impl ConnectionTrait,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, (AuditSubject, Uuid)>, OxidGeneError> {
    Ok(event_owner_map(
        &EventRepo::get_many_untagged(db, ids).await?,
    ))
}

/// The subject of each of `citations`: its person, else its family, else the
/// owning event's, else its source.
async fn citation_owner_map(
    db: &impl ConnectionTrait,
    citations: &[Citation],
) -> Result<HashMap<Uuid, (AuditSubject, Uuid)>, OxidGeneError> {
    let event_ids: Vec<Uuid> = citations.iter().filter_map(|c| c.event_id).collect();
    let owners = event_owners(db, &event_ids).await?;
    Ok(citations
        .iter()
        .map(|citation| {
            let event_owner = citation
                .event_id
                .and_then(|event_id| owners.get(&event_id).cloned());
            let subject = owner_subject(citation.person_id, citation.family_id, event_owner)
                .unwrap_or((AuditSubject::Source, citation.source_id));
            (citation.id, subject)
        })
        .collect())
}

/// The person of a record, else its family, else `fallback`.
fn owner_subject(
    person_id: Option<Uuid>,
    family_id: Option<Uuid>,
    fallback: Option<(AuditSubject, Uuid)>,
) -> Option<(AuditSubject, Uuid)> {
    person_id
        .map(|id| (AuditSubject::Person, id))
        .or(family_id.map(|id| (AuditSubject::Family, id)))
        .or(fallback)
}

/// The persons whose profiles show tag `tag_id`: the persons carrying it, and
/// those the events carrying it belong to.
async fn carrier_persons(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    tag_id: Uuid,
) -> Result<Vec<Uuid>, OxidGeneError> {
    let links = record_tag::Entity::find()
        .filter(record_tag::Column::TreeId.eq(tree_id))
        .filter(record_tag::Column::TagId.eq(tag_id))
        .all(db)
        .await
        .map_err(db_err)?;
    let mut persons: Vec<Uuid> = links.iter().filter_map(|link| link.person_id).collect();
    let event_ids: Vec<Uuid> = links.iter().filter_map(|link| link.event_id).collect();
    if !event_ids.is_empty() {
        let events = event::Entity::find()
            .filter(event::Column::Id.is_in(event_ids))
            .all(db)
            .await
            .map_err(db_err)?;
        let owners: Vec<(Option<Uuid>, Option<Uuid>)> = events
            .iter()
            .map(|event| (event.person_id, event.family_id))
            .collect();
        persons.extend(invalidation::affected_persons_for_events(db, &owners).await?);
    }
    Ok(sorted_unique(persons))
}

/// Refresh the projections of every person showing tag `tag_id`.
async fn refresh_carriers(
    db: &impl ConnectionTrait,
    profiles: &ProfileService,
    tree_id: Uuid,
    tag_id: Uuid,
) -> Result<(), OxidGeneError> {
    let persons = carrier_persons(db, tree_id, tag_id).await?;
    profiles
        .invalidate_for_mutation(db, tree_id, &persons)
        .await
}

/// The tag `id` of `tree_id` with its counts, read back after a write.
async fn usage_of(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    id: Uuid,
) -> Result<TagUsage, OxidGeneError> {
    TagRepo::list(db, tree_id)
        .await?
        .into_iter()
        .find(|usage| usage.tag.id == id)
        .ok_or(OxidGeneError::NotFound { entity: "Tag", id })
}
