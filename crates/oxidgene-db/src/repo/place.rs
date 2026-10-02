//! Repository for `Place` entities (CRUD, no soft delete, search filter).

use chrono::Utc;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::{Connection, Place};
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveModelTrait, ConnectionTrait, IntoActiveModel, QueryFilter, Set};
use uuid::Uuid;

use crate::entities::place::{self, ActiveModel, Column, Entity};
use crate::repo::batch::in_chunks;
use crate::repo::db_err;
use crate::repo::pagination::{PaginationParams, paginate};

/// Repository for place CRUD operations.
pub struct PlaceRepo;

/// What a place list may be narrowed to.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlaceFilter<'a> {
    /// Places whose name contains it.
    pub search: Option<&'a str>,
    /// Places named exactly so — trimmed, ignoring case: how an editor finds
    /// the place a typed name names without reading every place of the tree.
    pub name: Option<&'a str>,
    /// These places: how an editor names the places its events sit on.
    pub ids: Option<&'a [Uuid]>,
}

impl PlaceRepo {
    /// List places in a tree with optional search and pagination.
    pub async fn list(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        search: Option<&str>,
        params: &PaginationParams,
    ) -> Result<Connection<Place>, OxidGeneError> {
        let filter = PlaceFilter {
            search,
            ..PlaceFilter::default()
        };
        Self::list_filtered(db, tree_id, &filter, params).await
    }

    /// [`Self::list`] with every filter a place list takes.
    pub async fn list_filtered(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        filter: &PlaceFilter<'_>,
        params: &PaginationParams,
    ) -> Result<Connection<Place>, OxidGeneError> {
        let mut query = Entity::find().filter(Column::TreeId.eq(tree_id));
        if let Some(q) = filter.search {
            query = query.filter(Column::Name.contains(q));
        }
        if let Some(name) = filter.name {
            query = query.filter(crate::repo::lower_trim_eq(Column::Name, name));
        }
        if let Some(ids) = filter.ids {
            query = query.filter(Column::Id.is_in(ids.iter().copied()));
        }
        paginate(db, query, Column::Id, params, |m| (m.id, into_domain(m))).await
    }

    /// List all places in a tree without pagination.
    pub async fn list_all(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<Place>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .all(db)
            .await
            .map_err(db_err)?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// Get multiple places by ID.
    pub async fn get_many(
        db: &impl ConnectionTrait,
        ids: &[Uuid],
    ) -> Result<Vec<Place>, OxidGeneError> {
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

    /// Get a single place by ID.
    pub async fn get(db: &impl ConnectionTrait, id: Uuid) -> Result<Place, OxidGeneError> {
        Entity::find_by_id(id)
            .one(db)
            .await
            .map_err(db_err)?
            .map(into_domain)
            .ok_or(OxidGeneError::NotFound {
                entity: "Place",
                id,
            })
    }

    /// Create a new place.
    pub async fn create(
        db: &impl ConnectionTrait,
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
        let result = model.insert(db).await.map_err(db_err)?;
        Ok(into_domain(result))
    }

    /// Update a place.
    pub async fn update(
        db: &impl ConnectionTrait,
        id: Uuid,
        name: Option<String>,
        latitude: Option<Option<f64>>,
        longitude: Option<Option<f64>>,
    ) -> Result<Place, OxidGeneError> {
        let existing = Entity::find_by_id(id)
            .one(db)
            .await
            .map_err(db_err)?
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

        let result = active.update(db).await.map_err(db_err)?;
        Ok(into_domain(result))
    }

    /// Hard-delete a place.
    pub async fn delete(db: &impl ConnectionTrait, id: Uuid) -> Result<(), OxidGeneError> {
        let result = Entity::delete_by_id(id).exec(db).await.map_err(db_err)?;
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
