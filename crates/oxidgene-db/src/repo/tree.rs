//! Repository for `Tree` entities.
//!
//! Deleting a tree happens in two stages. [`TreeRepo::soft_delete`] flips
//! `deleted_at` — one row, instant, and the tree disappears from [`TreeRepo::list`]
//! straight away. [`TreeRepo::purge`] then does the real cascade in the
//! background, because SQLite resolves `ON DELETE CASCADE` one row at a time
//! and that costs seconds on a tree of any size. Doing both inside the request
//! is what used to freeze the UI.

use chrono::Utc;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::{Connection, Tree};
use sea_orm::entity::prelude::*;
use sea_orm::sea_query::Expr;
use sea_orm::{ActiveModelTrait, ConnectionTrait, IntoActiveModel, QueryFilter, Set};
use uuid::Uuid;

use crate::entities::tree::{self, ActiveModel, Column, Entity};
use crate::repo::pagination::{PaginationParams, paginate};

/// Repository for tree CRUD operations.
pub struct TreeRepo;

impl TreeRepo {
    /// List trees with cursor-based pagination (excludes soft-deleted).
    pub async fn list(
        db: &impl ConnectionTrait,
        params: &PaginationParams,
    ) -> Result<Connection<Tree>, OxidGeneError> {
        let query = Entity::find().filter(Column::DeletedAt.is_null());
        paginate(db, query, Column::Id, params, |m| (m.id, into_domain(m))).await
    }

    /// Get a single tree by ID (excludes soft-deleted).
    pub async fn get(db: &impl ConnectionTrait, id: Uuid) -> Result<Tree, OxidGeneError> {
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
        db: &impl ConnectionTrait,
        id: Uuid,
        name: String,
        description: Option<String>,
    ) -> Result<Tree, OxidGeneError> {
        let now = Utc::now();
        let model = tree::ActiveModel {
            id: Set(id),
            name: Set(name),
            description: Set(description),
            sosa_root_person_id: Set(None),
            default_privacy: Set(oxidgene_core::enums::TreeDefaultPrivacy::default().into()),
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
        db: &impl ConnectionTrait,
        id: Uuid,
        name: Option<String>,
        description: Option<Option<String>>,
        sosa_root_person_id: Option<Option<Uuid>>,
        default_privacy: Option<oxidgene_core::enums::TreeDefaultPrivacy>,
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
        if let Some(sosa_root) = sosa_root_person_id {
            active.sosa_root_person_id = Set(sosa_root);
        }
        if let Some(default_privacy) = default_privacy {
            active.default_privacy = Set(default_privacy.into());
        }
        active.updated_at = Set(Utc::now());

        let result = active
            .update(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(into_domain(result))
    }

    /// Mark a tree as deleted, without touching the rows it owns.
    ///
    /// This is what a delete request does: it is a single-row UPDATE, so it
    /// returns in about a millisecond however large the tree is. [`list_purgeable`]
    /// then finds the tree again and [`purge`] does the expensive part in the
    /// background. The tree is already invisible to [`list`] and [`get`].
    pub async fn soft_delete(db: &impl ConnectionTrait, id: Uuid) -> Result<(), OxidGeneError> {
        let result = Entity::update_many()
            .col_expr(Column::DeletedAt, Expr::value(Utc::now()))
            .filter(Column::Id.eq(id))
            .filter(Column::DeletedAt.is_null())
            .exec(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        if result.rows_affected == 0 {
            return Err(OxidGeneError::NotFound { entity: "Tree", id });
        }
        Ok(())
    }

    /// IDs of every soft-deleted tree still holding data.
    ///
    /// This *is* the purge queue: because the flag lives in the database, a
    /// purge interrupted by a crash or a quit is simply picked up again at the
    /// next start. No separate job table is needed.
    pub async fn list_purgeable(db: &impl ConnectionTrait) -> Result<Vec<Uuid>, OxidGeneError> {
        Entity::find()
            .filter(Column::DeletedAt.is_not_null())
            .all(db)
            .await
            .map(|models| models.into_iter().map(|m| m.id).collect())
            .map_err(|e| OxidGeneError::Database(e.to_string()))
    }

    /// Hard-delete a tree. Cascades via `ON DELETE CASCADE` foreign keys to
    /// every entity scoped to this tree (person, event, family, place,
    /// source, media, note, ...) — a tree's data is never shared with
    /// another tree, so nothing outside it is affected.
    ///
    /// Unlike the other methods this deliberately does *not* filter on
    /// `deleted_at`: it is called precisely on trees that were soft-deleted.
    ///
    /// Expensive — SQLite resolves the cascade one row at a time, which on a
    /// 10k-person tree costs seconds. Call it from the purge worker, never
    /// from a request handler. Deleting an already-purged tree is not an
    /// error, so a re-run after a crash is harmless.
    pub async fn purge(db: &impl ConnectionTrait, id: Uuid) -> Result<(), OxidGeneError> {
        Entity::delete_by_id(id)
            .exec(db)
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
        sosa_root_person_id: m.sosa_root_person_id,
        default_privacy: m.default_privacy.into(),
        created_at: m.created_at,
        updated_at: m.updated_at,
        deleted_at: m.deleted_at,
    }
}
