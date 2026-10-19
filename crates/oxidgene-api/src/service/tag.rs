//! Tag writes and reads: the tree's vocabulary, and the tags put on records of
//! every kind (docs/tags.md). REST and GraphQL both go through here.
//!
//! A tag change is recorded in the audit log, never versioned, and refreshes
//! the projections of the persons whose profiles show it, in the transaction
//! that makes the change.

use std::collections::HashMap;

use oxidgene_core::collections::sorted_unique;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::history::{AuditAction, AuditCategory, AuditDetails, AuditEntity, AuditSubject};
use oxidgene_core::types::{Connection, Edge, FamilySpouse, RecordKind};
use oxidgene_db::entities::{event, record_tag};
use oxidgene_db::repo::{
    CitationRepo, EventRepo, FamilySpouseRepo, MediaRepo, NoteRepo, PaginationParams, PlaceRepo,
    RecordTagRepo, RepositoryRepo, SourceRepo, TagRepo, TagUsage, checked_color, db_err,
    display_names,
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
/// it belongs to.
#[derive(Debug, Clone, Serialize)]
pub struct TaggedRecord {
    pub id: Uuid,
    pub kind: RecordKind,
    pub label: String,
    pub person_id: Option<Uuid>,
    pub family_id: Option<Uuid>,
}

/// One page of the live records of `kind` carrying tag `tag_id` of `tree_id`.
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
    let labels = labels_of(db, tree_id, kind, &ids).await?;
    let mut edges = Vec::with_capacity(page.edges.len());
    for edge in page.edges {
        let (person_id, family_id) = match kind {
            RecordKind::Person | RecordKind::Family => (None, None),
            _ => match subject_of(db, kind, edge.node).await? {
                Some((AuditSubject::Person, id)) => (Some(id), None),
                Some((AuditSubject::Family, id)) => (None, Some(id)),
                _ => (None, None),
            },
        };
        edges.push(Edge {
            cursor: edge.cursor,
            node: TaggedRecord {
                id: edge.node,
                kind,
                label: labels
                    .get(&edge.node)
                    .cloned()
                    .unwrap_or_else(|| edge.node.to_string()),
                person_id,
                family_id,
            },
        });
    }
    Ok(Connection {
        edges,
        page_info: page.page_info,
        total_count: page.total_count,
    })
}

/// The label to show for each of `ids`, all records of `kind`. A record with
/// no text to show is absent from the map.
async fn labels_of(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    kind: RecordKind,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, String>, OxidGeneError> {
    let mut labels: HashMap<Uuid, String> = match kind {
        RecordKind::Person => display_names(db, ids).await?,
        RecordKind::Family => family_labels(db, ids).await?,
        RecordKind::Place => PlaceRepo::get_many(db, ids)
            .await?
            .into_iter()
            .map(|place| (place.id, place.name))
            .collect(),
        RecordKind::Source => SourceRepo::get_many(db, tree_id, ids)
            .await?
            .into_iter()
            .map(|source| (source.id, source.title))
            .collect(),
        RecordKind::Repository => RepositoryRepo::get_many(db, ids)
            .await?
            .into_iter()
            .map(|repository| (repository.id, repository.name))
            .collect(),
        RecordKind::Media => MediaRepo::get_many(db, ids)
            .await?
            .into_iter()
            .map(|media| (media.id, media.title.unwrap_or(media.file_name)))
            .collect(),
        RecordKind::Event => event_labels(db, ids).await?,
        RecordKind::Citation => citation_labels(db, ids).await?,
        RecordKind::Note => note_labels(db, ids).await?,
    };
    labels.retain(|_, label| !label.trim().is_empty());
    Ok(labels)
}

/// A couple's label: its spouses' names, in their order.
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
async fn event_labels(
    db: &impl ConnectionTrait,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, String>, OxidGeneError> {
    let mut labels = HashMap::new();
    for id in ids {
        let event = EventRepo::get(db, *id).await?;
        let spelling = serde_json::to_value(event.event_type)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned));
        if let Some(spelling) = spelling {
            labels.insert(*id, spelling);
        }
    }
    Ok(labels)
}

/// A note's label: its text.
async fn note_labels(
    db: &impl ConnectionTrait,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, String>, OxidGeneError> {
    let mut labels = HashMap::new();
    for id in ids {
        labels.insert(*id, NoteRepo::get(db, *id).await?.text);
    }
    Ok(labels)
}

/// A citation's label: its text, when it has one.
async fn citation_labels(
    db: &impl ConnectionTrait,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, String>, OxidGeneError> {
    let mut labels = HashMap::new();
    for id in ids {
        if let Some(text) = CitationRepo::get(db, *id).await?.text {
            labels.insert(*id, text);
        }
    }
    Ok(labels)
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
/// owning it. `None` when nothing owns it.
async fn subject_of(
    db: &impl ConnectionTrait,
    kind: RecordKind,
    id: Uuid,
) -> Result<Option<(AuditSubject, Uuid)>, OxidGeneError> {
    Ok(match kind {
        RecordKind::Person => Some((AuditSubject::Person, id)),
        RecordKind::Family => Some((AuditSubject::Family, id)),
        RecordKind::Place => Some((AuditSubject::Place, id)),
        RecordKind::Source => Some((AuditSubject::Source, id)),
        RecordKind::Repository => Some((AuditSubject::Repository, id)),
        RecordKind::Media => Some((AuditSubject::Media, id)),
        RecordKind::Event => {
            let event = EventRepo::get(db, id).await?;
            owner_subject(event.person_id, event.family_id, None)
        }
        RecordKind::Citation => {
            let citation = CitationRepo::get(db, id).await?;
            let event_owner = match citation.event_id {
                Some(event_id) => {
                    let event = EventRepo::get(db, event_id).await?;
                    owner_subject(event.person_id, event.family_id, None)
                }
                None => None,
            };
            owner_subject(citation.person_id, citation.family_id, event_owner)
                .or(Some((AuditSubject::Source, citation.source_id)))
        }
        RecordKind::Note => {
            let note = NoteRepo::get(db, id).await?;
            let event_owner = match note.event_id {
                Some(event_id) => {
                    let event = EventRepo::get(db, event_id).await?;
                    owner_subject(event.person_id, event.family_id, None)
                }
                None => None,
            };
            owner_subject(note.person_id, note.family_id, event_owner).or(note
                .source_id
                .map(|source_id| (AuditSubject::Source, source_id)))
        }
    })
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
        for event in events {
            persons.extend(
                invalidation::affected_persons_for_event(db, event.person_id, event.family_id)
                    .await?,
            );
        }
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
