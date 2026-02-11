//! Repository for `Note` entities (CRUD with soft delete).

use chrono::Utc;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::Note;
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveModelTrait, IntoActiveModel, QueryFilter, Set};
use uuid::Uuid;

use crate::entities::note::{self, ActiveModel, Column, Entity};

/// Repository for note CRUD operations.
pub struct NoteRepo;

impl NoteRepo {
    /// Get a single note by ID (excludes soft-deleted).
    pub async fn get(db: &DatabaseConnection, id: Uuid) -> Result<Note, OxidGeneError> {
        Entity::find_by_id(id)
            .filter(Column::DeletedAt.is_null())
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .map(into_domain)
            .ok_or(OxidGeneError::NotFound { entity: "Note", id })
    }

    /// List notes for a specific entity (person, event, family, or source) in a tree.
    pub async fn list_by_entity(
        db: &DatabaseConnection,
        tree_id: Uuid,
        person_id: Option<Uuid>,
        event_id: Option<Uuid>,
        family_id: Option<Uuid>,
        source_id: Option<Uuid>,
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
        if let Some(sid) = source_id {
            query = query.filter(Column::SourceId.eq(sid));
        }

        let models = query
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// Create a new note.
    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        db: &DatabaseConnection,
        id: Uuid,
        tree_id: Uuid,
        text: String,
        person_id: Option<Uuid>,
        event_id: Option<Uuid>,
        family_id: Option<Uuid>,
        source_id: Option<Uuid>,
    ) -> Result<Note, OxidGeneError> {
        let now = Utc::now();
        let model = note::ActiveModel {
            id: Set(id),
            tree_id: Set(tree_id),
            text: Set(text),
            person_id: Set(person_id),
            event_id: Set(event_id),
            family_id: Set(family_id),
            source_id: Set(source_id),
            created_at: Set(now),
            updated_at: Set(now),
            deleted_at: Set(None),
        };
        let result = model
            .insert(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(into_domain(result))
    }

    /// Update a note's text.
    pub async fn update(
        db: &DatabaseConnection,
        id: Uuid,
        text: Option<String>,
    ) -> Result<Note, OxidGeneError> {
        let existing = Entity::find_by_id(id)
            .filter(Column::DeletedAt.is_null())
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .ok_or(OxidGeneError::NotFound { entity: "Note", id })?;

        let mut active: ActiveModel = existing.into_active_model();
        if let Some(text) = text {
            active.text = Set(text);
        }
        active.updated_at = Set(Utc::now());

        let result = active
            .update(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(into_domain(result))
    }

    /// Soft-delete a note.
    pub async fn delete(db: &DatabaseConnection, id: Uuid) -> Result<(), OxidGeneError> {
        let existing = Entity::find_by_id(id)
            .filter(Column::DeletedAt.is_null())
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .ok_or(OxidGeneError::NotFound { entity: "Note", id })?;

        let mut active: ActiveModel = existing.into_active_model();
        active.deleted_at = Set(Some(Utc::now()));
        active
            .update(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
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
        created_at: m.created_at,
        updated_at: m.updated_at,
        deleted_at: m.deleted_at,
    }
}
