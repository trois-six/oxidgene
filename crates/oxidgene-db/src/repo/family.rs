//! Repository for `Family` entities (CRUD with soft delete).

use chrono::Utc;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::{Connection, Family};
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveModelTrait, ConnectionTrait, IntoActiveModel, QueryFilter, QuerySelect, Set};
use uuid::Uuid;

use crate::entities::family::{self, ActiveModel, Column, Entity};
use crate::repo::batch::in_chunks;
use crate::repo::db_err;
use crate::repo::pagination::{PaginationParams, paginate};

/// Repository for family CRUD operations.
pub struct FamilyRepo;

impl FamilyRepo {
    /// List families in a tree with pagination (excludes soft-deleted).
    pub async fn list(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        params: &PaginationParams,
    ) -> Result<Connection<Family>, OxidGeneError> {
        let query = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::DeletedAt.is_null());
        paginate(db, query, Column::Id, params, |m| (m.id, into_domain(m))).await
    }

    /// How many live families each of `tree_ids` holds; a tree without any is
    /// absent.
    pub async fn count_by_trees(
        db: &impl ConnectionTrait,
        tree_ids: &[Uuid],
    ) -> Result<Vec<(Uuid, i64)>, OxidGeneError> {
        in_chunks(tree_ids, |chunk| async move {
            Entity::find()
                .select_only()
                .column(Column::TreeId)
                .column_as(Column::Id.count(), "count")
                .filter(Column::TreeId.is_in(chunk))
                .filter(Column::DeletedAt.is_null())
                .group_by(Column::TreeId)
                .into_tuple()
                .all(db)
                .await
                .map_err(db_err)
        })
        .await
    }

    /// The live families among `ids`, in no particular order.
    pub async fn get_many(
        db: &impl ConnectionTrait,
        ids: &[Uuid],
    ) -> Result<Vec<Family>, OxidGeneError> {
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

    /// List all families in a tree without pagination (excludes soft-deleted).
    pub async fn list_all(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<Family>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::DeletedAt.is_null())
            .all(db)
            .await
            .map_err(db_err)?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// Narrow a set of family IDs to the ones that still exist.
    ///
    /// Deleting a family is a soft delete, and the `family_spouse` /
    /// `family_child` rows survive it. Anything that reaches families *through*
    /// those memberships must therefore check here, or it will keep reporting
    /// a family the user has deleted.
    pub async fn live_ids(
        db: &impl ConnectionTrait,
        ids: &[Uuid],
    ) -> Result<Vec<Uuid>, OxidGeneError> {
        in_chunks(ids, |chunk| async move {
            let models = Entity::find()
                .filter(Column::Id.is_in(chunk))
                .filter(Column::DeletedAt.is_null())
                .all(db)
                .await
                .map_err(db_err)?;
            Ok(models.into_iter().map(|m| m.id).collect())
        })
        .await
    }

    /// Get a single family by ID (excludes soft-deleted).
    pub async fn get(db: &impl ConnectionTrait, id: Uuid) -> Result<Family, OxidGeneError> {
        find_live(db, id).await.map(into_domain)
    }

    /// Create a new family.
    pub async fn create(
        db: &impl ConnectionTrait,
        id: Uuid,
        tree_id: Uuid,
    ) -> Result<Family, OxidGeneError> {
        let now = Utc::now();
        let model = family::ActiveModel {
            id: Set(id),
            tree_id: Set(tree_id),
            privacy: Set(oxidgene_core::enums::Privacy::default().into()),
            created_at: Set(now),
            updated_at: Set(now),
            deleted_at: Set(None),
        };
        let result = model.insert(db).await.map_err(db_err)?;
        Ok(into_domain(result))
    }

    /// Update a family: its privacy, and `updated_at` either way.
    pub async fn update(
        db: &impl ConnectionTrait,
        id: Uuid,
        privacy: Option<oxidgene_core::enums::Privacy>,
    ) -> Result<Family, OxidGeneError> {
        let existing = find_live(db, id).await?;

        let mut active: ActiveModel = existing.into_active_model();
        if let Some(privacy) = privacy {
            active.privacy = Set(privacy.into());
        }
        active.updated_at = Set(Utc::now());

        let result = active.update(db).await.map_err(db_err)?;
        Ok(into_domain(result))
    }

    /// Soft-delete a family.
    pub async fn delete(db: &impl ConnectionTrait, id: Uuid) -> Result<(), OxidGeneError> {
        let existing = find_live(db, id).await?;

        let mut active: ActiveModel = existing.into_active_model();
        active.deleted_at = Set(Some(Utc::now()));
        active.update(db).await.map_err(db_err)?;
        Ok(())
    }
}

fn into_domain(m: family::Model) -> Family {
    Family {
        id: m.id,
        tree_id: m.tree_id,
        privacy: m.privacy.into(),
        created_at: m.created_at,
        updated_at: m.updated_at,
        deleted_at: m.deleted_at,
    }
}

/// Row `id`, unless it is missing or soft-deleted.
async fn find_live(
    db: &impl ConnectionTrait,
    id: Uuid,
) -> Result<<Entity as EntityTrait>::Model, OxidGeneError> {
    crate::repo::find_live::<Entity>(db, id, Column::DeletedAt, "Family").await
}
