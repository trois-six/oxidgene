//! Repository for `Note` entities (CRUD with soft delete).
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
    /// List notes in a tree with optional entity filters and pagination.
    pub async fn list(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        filter: &NoteFilter,
        params: &PaginationParams,
    ) -> Result<Connection<Note>, OxidGeneError> {
        let mut query = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::DeletedAt.is_null());

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

    /// Get a single note by ID (excludes soft-deleted).
    pub async fn get(db: &impl ConnectionTrait, id: Uuid) -> Result<Note, OxidGeneError> {
        let note = find_live(db, id).await.map(into_domain)?;
        hydrate_one(db, note).await
    }

    /// The live notes among `ids`, one read per bounded slice of ids. Their tags
    /// are not filled: this serves the labels of many records at once. A
    /// missing or soft-deleted id is absent.
    pub async fn get_many_untagged(
        db: &impl ConnectionTrait,
        ids: &[Uuid],
    ) -> Result<Vec<Note>, OxidGeneError> {
        in_chunks(ids, |chunk| async move {
            let models = Entity::find()
                .filter(Column::Id.is_in(chunk))
                .filter(Column::DeletedAt.is_null())
                .all(db)
                .await
                .map_err(db_err)?;
            Ok(models.into_iter().map(into_domain).collect())
        })
        .await
    }

    /// The live notes among `ids`, one read per bounded slice of ids. A
    /// missing or soft-deleted id is absent.
    pub async fn get_many(
        db: &impl ConnectionTrait,
        ids: &[Uuid],
    ) -> Result<Vec<Note>, OxidGeneError> {
        let notes = Self::get_many_untagged(db, ids).await?;
        hydrate(db, notes).await
    }

    /// List all notes in a tree without pagination (excludes soft-deleted).
    pub async fn list_all(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<Note>, OxidGeneError> {
        let notes = Self::list_all_untagged(db, tree_id).await?;
        hydrate(db, notes).await
    }

    /// Every live note of a tree, without its tags: the profile projections
    /// only count notes per person.
    pub async fn list_all_untagged(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<Note>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::DeletedAt.is_null())
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
        let mut query = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::DeletedAt.is_null());

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

    /// List the live notes attached to any of the given persons in a tree.
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
        Self::list_live_by_untagged(db, tree_id, Column::PersonId, person_ids).await
    }

    /// List the live notes attached to any of the given events in a tree.
    pub async fn list_by_events(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        event_ids: &[Uuid],
    ) -> Result<Vec<Note>, OxidGeneError> {
        let notes = Self::list_live_by_untagged(db, tree_id, Column::EventId, event_ids).await?;
        hydrate(db, notes).await
    }

    /// The live notes of a tree whose `column` is one of `ids`, without tags.
    async fn list_live_by_untagged(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        column: Column,
        ids: &[Uuid],
    ) -> Result<Vec<Note>, OxidGeneError> {
        in_chunks(ids, |chunk| async move {
            let models = Entity::find()
                .filter(Column::TreeId.eq(tree_id))
                .filter(Column::DeletedAt.is_null())
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
        owner: &NoteFilter,
    ) -> Result<Note, OxidGeneError> {
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
            created_at: Set(now),
            updated_at: Set(now),
            deleted_at: Set(None),
        };
        let result = model.insert(db).await.map_err(db_err)?;
        hydrate_one(db, into_domain(result)).await
    }

    /// Update a note's text.
    pub async fn update(
        db: &impl ConnectionTrait,
        id: Uuid,
        text: Option<String>,
    ) -> Result<Note, OxidGeneError> {
        let existing = find_live(db, id).await?;

        let mut active: ActiveModel = existing.into_active_model();
        if let Some(text) = text {
            active.text = Set(sanitize_note_html(&text));
        }
        active.updated_at = Set(Utc::now());

        let result = active.update(db).await.map_err(db_err)?;
        hydrate_one(db, into_domain(result)).await
    }

    /// Soft-delete a note.
    pub async fn delete(db: &impl ConnectionTrait, id: Uuid) -> Result<(), OxidGeneError> {
        let existing = find_live(db, id).await?;

        let mut active: ActiveModel = existing.into_active_model();
        active.deleted_at = Set(Some(Utc::now()));
        active.update(db).await.map_err(db_err)?;
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
        tags: Vec::new(),
        created_at: m.created_at,
        updated_at: m.updated_at,
        deleted_at: m.deleted_at,
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

/// Row `id`, unless it is missing or soft-deleted.
async fn find_live(
    db: &impl ConnectionTrait,
    id: Uuid,
) -> Result<<Entity as EntityTrait>::Model, OxidGeneError> {
    crate::repo::find_live::<Entity>(db, id, Column::DeletedAt, "Note").await
}
