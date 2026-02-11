//! Repository for `Tree` entities (full CRUD with soft delete).

use chrono::Utc;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::{Connection, Tree};
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveModelTrait, IntoActiveModel, QueryFilter, Set};
use uuid::Uuid;

use crate::entities::tree::{self, ActiveModel, Column, Entity};
use crate::repo::pagination::{PaginationParams, paginate};

/// Repository for tree CRUD operations.
pub struct TreeRepo;

impl TreeRepo {
    /// List trees with cursor-based pagination (excludes soft-deleted).
    pub async fn list(
        db: &DatabaseConnection,
        params: &PaginationParams,
    ) -> Result<Connection<Tree>, OxidGeneError> {
        let query = Entity::find().filter(Column::DeletedAt.is_null());
        paginate(db, query, Column::Id, params, |m| (m.id, into_domain(m))).await
    }

    /// Get a single tree by ID (excludes soft-deleted).
    pub async fn get(db: &DatabaseConnection, id: Uuid) -> Result<Tree, OxidGeneError> {
        Entity::find_by_id(id)
            .filter(Column::DeletedAt.is_null())
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .map(into_domain)
            .ok_or(OxidGeneError::NotFound { entity: "Tree", id })
    }

    /// Create a new tree.
    pub async fn create(
        db: &DatabaseConnection,
        id: Uuid,
        name: String,
        description: Option<String>,
    ) -> Result<Tree, OxidGeneError> {
        let now = Utc::now();
        let model = tree::ActiveModel {
            id: Set(id),
            name: Set(name),
            description: Set(description),
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

    /// Update an existing tree.
    pub async fn update(
        db: &DatabaseConnection,
        id: Uuid,
        name: Option<String>,
        description: Option<Option<String>>,
    ) -> Result<Tree, OxidGeneError> {
        let existing = Entity::find_by_id(id)
            .filter(Column::DeletedAt.is_null())
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .ok_or(OxidGeneError::NotFound { entity: "Tree", id })?;

        let mut active: ActiveModel = existing.into_active_model();
        if let Some(name) = name {
            active.name = Set(name);
        }
        if let Some(description) = description {
            active.description = Set(description);
        }
        active.updated_at = Set(Utc::now());

        let result = active
            .update(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(into_domain(result))
    }

    /// Soft-delete a tree.
    pub async fn delete(db: &DatabaseConnection, id: Uuid) -> Result<(), OxidGeneError> {
        let existing = Entity::find_by_id(id)
            .filter(Column::DeletedAt.is_null())
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .ok_or(OxidGeneError::NotFound { entity: "Tree", id })?;

        let mut active: ActiveModel = existing.into_active_model();
        active.deleted_at = Set(Some(Utc::now()));
        active
            .update(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(())
    }
}

fn into_domain(m: tree::Model) -> Tree {
    Tree {
        id: m.id,
        name: m.name,
        description: m.description,
        created_at: m.created_at,
        updated_at: m.updated_at,
        deleted_at: m.deleted_at,
    }
}
