//! Repository for `Person` entities (CRUD with soft delete, search filter).

use chrono::Utc;
use oxidgene_core::enums::Sex;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::{Connection, Person};
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveModelTrait, IntoActiveModel, QueryFilter, Set};
use uuid::Uuid;

use crate::entities::person::{self, ActiveModel, Column, Entity};
use crate::entities::sea_enums;
use crate::repo::pagination::{PaginationParams, paginate};

/// Repository for person CRUD operations.
pub struct PersonRepo;

impl PersonRepo {
    /// List persons in a tree with pagination (excludes soft-deleted).
    pub async fn list(
        db: &DatabaseConnection,
        tree_id: Uuid,
        params: &PaginationParams,
    ) -> Result<Connection<Person>, OxidGeneError> {
        let query = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::DeletedAt.is_null());
        paginate(db, query, Column::Id, params, |m| (m.id, into_domain(m))).await
    }

    /// List all persons in a tree without pagination (excludes soft-deleted).
    pub async fn list_all(
        db: &DatabaseConnection,
        tree_id: Uuid,
    ) -> Result<Vec<Person>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::DeletedAt.is_null())
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// Get a single person by ID (excludes soft-deleted).
    pub async fn get(db: &DatabaseConnection, id: Uuid) -> Result<Person, OxidGeneError> {
        Entity::find_by_id(id)
            .filter(Column::DeletedAt.is_null())
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .map(into_domain)
            .ok_or(OxidGeneError::NotFound {
                entity: "Person",
                id,
            })
    }

    /// Create a new person.
    pub async fn create(
        db: &DatabaseConnection,
        id: Uuid,
        tree_id: Uuid,
        sex: Sex,
    ) -> Result<Person, OxidGeneError> {
        let now = Utc::now();
        let model = person::ActiveModel {
            id: Set(id),
            tree_id: Set(tree_id),
            sex: Set(sea_enums::Sex::from(sex)),
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

    /// Update a person's sex.
    pub async fn update(
        db: &DatabaseConnection,
        id: Uuid,
        sex: Option<Sex>,
    ) -> Result<Person, OxidGeneError> {
        let existing = Entity::find_by_id(id)
            .filter(Column::DeletedAt.is_null())
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .ok_or(OxidGeneError::NotFound {
                entity: "Person",
                id,
            })?;

        let mut active: ActiveModel = existing.into_active_model();
        if let Some(sex) = sex {
            active.sex = Set(sea_enums::Sex::from(sex));
        }
        active.updated_at = Set(Utc::now());

        let result = active
            .update(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(into_domain(result))
    }

    /// Soft-delete a person.
    pub async fn delete(db: &DatabaseConnection, id: Uuid) -> Result<(), OxidGeneError> {
        let existing = Entity::find_by_id(id)
            .filter(Column::DeletedAt.is_null())
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .ok_or(OxidGeneError::NotFound {
                entity: "Person",
                id,
            })?;

        let mut active: ActiveModel = existing.into_active_model();
        active.deleted_at = Set(Some(Utc::now()));
        active
            .update(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(())
    }
}

fn into_domain(m: person::Model) -> Person {
    Person {
        id: m.id,
        tree_id: m.tree_id,
        sex: m.sex.into(),
        created_at: m.created_at,
        updated_at: m.updated_at,
        deleted_at: m.deleted_at,
    }
}
