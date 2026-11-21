//! Repository for `Note` entities (CRUD).
//!
//! Note bodies are rendered as HTML, so every write here runs the text through
//! [`sanitize_note_html`] — this is the choke point both REST and GraphQL go
//! through. Bulk imports do not come this way and sanitize on their own; see
//! [`crate::html`].

use chrono::Utc;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::{Connection, Note, RecordKind};
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveModelTrait, ConnectionTrait, IntoActiveModel, QueryFilter, Set};
use uuid::Uuid;

use crate::entities::note::{self, ActiveModel, Column, Entity};
use crate::html::sanitize_note_html;
use crate::repo::batch::in_chunks;
use crate::repo::db_err;
use crate::repo::pagination::{PaginationParams, paginate};
use crate::repo::record_tag::RecordTagRepo;
use crate::repo::redaction::LeaveOut;

/// Optional entity filters for listing notes.
#[derive(Debug, Clone, Default)]
pub struct NoteFilter {
    pub person_id: Option<Uuid>,
    pub event_id: Option<Uuid>,
    pub family_id: Option<Uuid>,
    pub source_id: Option<Uuid>,
    pub media_id: Option<Uuid>,
    pub repository_id: Option<Uuid>,
}

/// Repository for note CRUD operations.
pub struct NoteRepo;

impl NoteRepo {
    /// List notes in a tree with optional entity filters and pagination,
    /// without those `leave_out` names (`LeaveOut::NONE` for the owner), nor the
    /// confidential ones when it says so.
    pub async fn list_visible(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        filter: &NoteFilter,
        params: &PaginationParams,
        leave_out: LeaveOut,
    ) -> Result<Connection<Note>, OxidGeneError> {
        let mut query = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .filter(leave_out.keeping(oxidgene_core::types::RecordKind::Note, Column::Id));
        if leave_out.confidential {
            query = query.filter(Column::Confidential.eq(false));
        }

        if let Some(person_id) = filter.person_id {
            query = query.filter(Column::PersonId.eq(person_id));
        }
        if let Some(event_id) = filter.event_id {
            query = query.filter(Column::EventId.eq(event_id));
        }
        if let Some(family_id) = filter.family_id {
            query = query.filter(Column::FamilyId.eq(family_id));
        }
        if let Some(source_id) = filter.source_id {
            query = query.filter(Column::SourceId.eq(source_id));
        }
        if let Some(media_id) = filter.media_id {
            query = query.filter(Column::MediaId.eq(media_id));
        }
        if let Some(repository_id) = filter.repository_id {
            query = query.filter(Column::RepositoryId.eq(repository_id));
        }

        let mut page = paginate(db, query, Column::Id, params, |model| {
            (model.id, into_domain(model))
        })
        .await?;
        let notes = page.edges.iter().map(|edge| edge.node.clone()).collect();
        for (edge, note) in page.edges.iter_mut().zip(hydrate(db, notes).await?) {
            edge.node = note;
        }
        Ok(page)
    }

    /// Get a single note by ID.
    pub async fn get(db: &impl ConnectionTrait, id: Uuid) -> Result<Note, OxidGeneError> {
        let note = find_row(db, id).await.map(into_domain)?;
        hydrate_one(db, note).await
    }

    /// The notes among `ids`, one read per bounded slice of ids. Their tags
    /// are not filled: this serves the labels of many records at once. A
    /// missing id is absent.
    pub async fn get_many_untagged(
        db: &impl ConnectionTrait,
        ids: &[Uuid],
    ) -> Result<Vec<Note>, OxidGeneError> {
        in_chunks(ids, |chunk| async move {
            let models = Entity::find()
                .filter(Column::Id.is_in(chunk))
                .all(db)
                .await
                .map_err(db_err)?;
            Ok(models.into_iter().map(into_domain).collect())
        })
        .await
    }

    /// The notes among `ids`, one read per bounded slice of ids. A
    /// missing id is absent.
    pub async fn get_many(
        db: &impl ConnectionTrait,
        ids: &[Uuid],
    ) -> Result<Vec<Note>, OxidGeneError> {
        let notes = Self::get_many_untagged(db, ids).await?;
        hydrate(db, notes).await
    }

    /// List all notes in a tree without pagination.
    pub async fn list_all(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<Note>, OxidGeneError> {
        let notes = Self::list_all_untagged(db, tree_id).await?;
        hydrate(db, notes).await
    }

    /// Every note of a tree, without its tags: the profile projections
    /// only count notes per person.
    pub async fn list_all_untagged(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<Note>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .all(db)
            .await
            .map_err(db_err)?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// List notes for a specific entity (person, event, family, source, or
    /// media) in a tree.
    pub async fn list_by_entity(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        person_id: Option<Uuid>,
        event_id: Option<Uuid>,
        family_id: Option<Uuid>,
        source_id: Option<Uuid>,
        media_id: Option<Uuid>,
    ) -> Result<Vec<Note>, OxidGeneError> {
        let mut query = Entity::find().filter(Column::TreeId.eq(tree_id));

        if let Some(pid) = person_id {
            query = query.filter(Column::PersonId.eq(pid));
        }
        if let Some(eid) = event_id {
            query = query.filter(Column::EventId.eq(eid));
        }
        if let Some(fid) = family_id {
            query = query.filter(Column::FamilyId.eq(fid));
        }
        if let Some(mid) = media_id {
            query = query.filter(Column::MediaId.eq(mid));
        }
        if let Some(sid) = source_id {
            query = query.filter(Column::SourceId.eq(sid));
        }

        let models = query.all(db).await.map_err(db_err)?;
        hydrate(db, models.into_iter().map(into_domain).collect()).await
    }

    /// List the notes attached to any of the given persons in a tree.
    pub async fn list_by_persons(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        person_ids: &[Uuid],
    ) -> Result<Vec<Note>, OxidGeneError> {
        let notes = Self::list_by_persons_untagged(db, tree_id, person_ids).await?;
        hydrate(db, notes).await
    }

    /// [`Self::list_by_persons`] without the tags, for the profile
    /// projections, which count these notes and never show their tags.
    pub async fn list_by_persons_untagged(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        person_ids: &[Uuid],
    ) -> Result<Vec<Note>, OxidGeneError> {
        Self::list_by_untagged(db, tree_id, Column::PersonId, person_ids).await
    }

    /// How many notes of a tree are attached to each of the given
    /// persons; a person without any is absent. One grouped read per chunk
    /// of persons, which reads no note text: the profile projections show
    /// the count and never the notes.
    pub async fn count_by_persons(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        person_ids: &[Uuid],
    ) -> Result<std::collections::HashMap<Uuid, u32>, OxidGeneError> {
        use sea_orm::QuerySelect as _;
        let counts: Vec<(Option<Uuid>, i64)> = in_chunks(person_ids, |chunk| async move {
            Entity::find()
                .select_only()
                .column(Column::PersonId)
                .column_as(Column::Id.count(), "notes")
                .filter(Column::TreeId.eq(tree_id))
                .filter(Column::PersonId.is_in(chunk))
                .group_by(Column::PersonId)
                .into_tuple()
                .all(db)
                .await
                .map_err(db_err)
        })
        .await?;
        Ok(counts
            .into_iter()
            .filter_map(|(person, count)| Some((person?, u32::try_from(count).ok()?)))
            .collect())
    }

    /// List the notes attached to any of the given events in a tree.
    pub async fn list_by_events(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        event_ids: &[Uuid],
    ) -> Result<Vec<Note>, OxidGeneError> {
        let notes = Self::list_by_untagged(db, tree_id, Column::EventId, event_ids).await?;
        hydrate(db, notes).await
    }

    /// List the notes attached to any of the given media in a tree.
    pub async fn list_by_medias(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        media_ids: &[Uuid],
    ) -> Result<Vec<Note>, OxidGeneError> {
        let notes = Self::list_by_untagged(db, tree_id, Column::MediaId, media_ids).await?;
        hydrate(db, notes).await
    }

    /// The notes of a tree whose `column` is one of `ids`, without tags.
    async fn list_by_untagged(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        column: Column,
        ids: &[Uuid],
    ) -> Result<Vec<Note>, OxidGeneError> {
        in_chunks(ids, |chunk| async move {
            let models = Entity::find()
                .filter(Column::TreeId.eq(tree_id))
                .filter(column.is_in(chunk))
                .all(db)
                .await
                .map_err(db_err)?;
            Ok(models.into_iter().map(into_domain).collect())
        })
        .await
    }

    /// Create a new note about what `owner` names — each record it sets.
    pub async fn create(
        db: &impl ConnectionTrait,
        id: Uuid,
        tree_id: Uuid,
        text: String,
        confidential: bool,
        owner: &NoteFilter,
    ) -> Result<Note, OxidGeneError> {
        crate::repo::require_one_target(
            "note",
            "person_id, event_id, family_id, source_id, media_id and repository_id",
            &[
                owner.person_id,
                owner.event_id,
                owner.family_id,
                owner.source_id,
                owner.media_id,
                owner.repository_id,
            ],
        )?;
        let now = Utc::now();
        let model = note::ActiveModel {
            id: Set(id),
            tree_id: Set(tree_id),
            text: Set(sanitize_note_html(&text)),
            person_id: Set(owner.person_id),
            event_id: Set(owner.event_id),
            family_id: Set(owner.family_id),
            source_id: Set(owner.source_id),
            media_id: Set(owner.media_id),
            repository_id: Set(owner.repository_id),
            shared_group: Set(None),
            confidential: Set(confidential),
            created_at: Set(now),
            updated_at: Set(now),
        };
        let result = model.insert(db).await.map_err(db_err)?;
        hydrate_one(db, into_domain(result)).await
    }

    /// Update a note's text.
    pub async fn update(
        db: &impl ConnectionTrait,
        id: Uuid,
        text: Option<String>,
        confidential: Option<bool>,
    ) -> Result<Note, OxidGeneError> {
        let existing = find_row(db, id).await?;

        let mut active: ActiveModel = existing.into_active_model();
        if let Some(text) = text {
            active.text = Set(sanitize_note_html(&text));
        }
        if let Some(confidential) = confidential {
            active.confidential = Set(confidential);
        }
        active.updated_at = Set(Utc::now());

        let result = active.update(db).await.map_err(db_err)?;
        hydrate_one(db, into_domain(result)).await
    }

    /// Delete a note; its tags go with it (cascade).
    pub async fn delete(db: &impl ConnectionTrait, id: Uuid) -> Result<(), OxidGeneError> {
        find_row(db, id).await?;
        Entity::delete_by_id(id).exec(db).await.map_err(db_err)?;
        Ok(())
    }
}

fn into_domain(m: note::Model) -> Note {
    Note {
        id: m.id,
        tree_id: m.tree_id,
        text: m.text,
        person_id: m.person_id,
        event_id: m.event_id,
        family_id: m.family_id,
        source_id: m.source_id,
        media_id: m.media_id,
        repository_id: m.repository_id,
        shared_group: m.shared_group,
        confidential: m.confidential,
        tags: Vec::new(),
        created_at: m.created_at,
        updated_at: m.updated_at,
    }
}

/// Fill the tags of each note from one grouped read.
async fn hydrate(
    db: &impl ConnectionTrait,
    mut notes: Vec<Note>,
) -> Result<Vec<Note>, OxidGeneError> {
    let ids: Vec<Uuid> = notes.iter().map(|note| note.id).collect();
    let mut tags = RecordTagRepo::tags_of(db, RecordKind::Note, &ids).await?;
    for note in &mut notes {
        note.tags = tags.remove(&note.id).unwrap_or_default();
    }
    Ok(notes)
}

/// [`hydrate`] for one note.
async fn hydrate_one(db: &impl ConnectionTrait, note: Note) -> Result<Note, OxidGeneError> {
    let mut notes = hydrate(db, vec![note]).await?;
    Ok(notes.remove(0))
}

/// Row `id`; `NotFound` when it is missing.
async fn find_row(
    db: &impl ConnectionTrait,
    id: Uuid,
) -> Result<<Entity as EntityTrait>::Model, OxidGeneError> {
    crate::repo::find_row::<Entity>(db, id, "Note").await
}
