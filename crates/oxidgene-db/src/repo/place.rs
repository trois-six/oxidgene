//! Repository for `Place` entities (CRUD, no soft delete, search filter).

use chrono::Utc;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::{Connection, Place};
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveModelTrait, IntoActiveModel, QueryFilter, Set};
use uuid::Uuid;

use crate::entities::place::{self, ActiveModel, Column, Entity};
use crate::repo::pagination::{PaginationParams, paginate};

/// Repository for place CRUD operations.
pub struct PlaceRepo;

impl PlaceRepo {
    /// List places in a tree with optional search and pagination.
    pub async fn list(
        db: &DatabaseConnection,
        tree_id: Uuid,
        search: Option<&str>,
        params: &PaginationParams,
    ) -> Result<Connection<Place>, OxidGeneError> {
        let mut query = Entity::find().filter(Column::TreeId.eq(tree_id));

        if let Some(q) = search {
            query = query.filter(Column::Name.contains(q));
        }

        paginate(db, query, Column::Id, params, |m| (m.id, into_domain(m))).await
    }

    /// Get a single place by ID.
    pub async fn get(db: &DatabaseConnection, id: Uuid) -> Result<Place, OxidGeneError> {
        Entity::find_by_id(id)
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .map(into_domain)
            .ok_or(OxidGeneError::NotFound {
                entity: "Place",
                id,
            })
    }

    /// Create a new place.
    pub async fn create(
        db: &DatabaseConnection,
        id: Uuid,
        tree_id: Uuid,
        name: String,
        latitude: Option<f64>,
        longitude: Option<f64>,
    ) -> Result<Place, OxidGeneError> {
        let now = Utc::now();
        let model = place::ActiveModel {
            id: Set(id),
            tree_id: Set(tree_id),
            name: Set(name),
            latitude: Set(latitude),
            longitude: Set(longitude),
            created_at: Set(now),
            updated_at: Set(now),
        };
        let result = model
            .insert(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(into_domain(result))
    }

    /// Update a place.
    pub async fn update(
        db: &DatabaseConnection,
        id: Uuid,
        name: Option<String>,
        latitude: Option<Option<f64>>,
        longitude: Option<Option<f64>>,
    ) -> Result<Place, OxidGeneError> {
        let existing = Entity::find_by_id(id)
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .ok_or(OxidGeneError::NotFound {
                entity: "Place",
                id,
            })?;

        let mut active: ActiveModel = existing.into_active_model();
        if let Some(name) = name {
            active.name = Set(name);
        }
        if let Some(latitude) = latitude {
            active.latitude = Set(latitude);
        }
        if let Some(longitude) = longitude {
            active.longitude = Set(longitude);
        }
        active.updated_at = Set(Utc::now());

        let result = active
            .update(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(into_domain(result))
    }

    /// Hard-delete a place.
    pub async fn delete(db: &DatabaseConnection, id: Uuid) -> Result<(), OxidGeneError> {
        let result = Entity::delete_by_id(id)
            .exec(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        if result.rows_affected == 0 {
            return Err(OxidGeneError::NotFound {
                entity: "Place",
                id,
            });
        }
        Ok(())
    }
}

fn into_domain(m: place::Model) -> Place {
    Place {
        id: m.id,
        tree_id: m.tree_id,
        name: m.name,
        latitude: m.latitude,
        longitude: m.longitude,
        created_at: m.created_at,
        updated_at: m.updated_at,
    }
}
