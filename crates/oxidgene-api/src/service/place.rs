//! Place writes: the same steps whether REST or GraphQL asked.
//!
//! A place's name is what every event at it shows, so it may never be blank.
//! Each write rewrites the projections of the persons whose events it changes
//! and records the change, in one transaction.

use oxidgene_core::OxidGeneError;
use oxidgene_core::history::AuditEntity;
use oxidgene_core::types::Place;
use oxidgene_db::repo::PlaceRepo;
use oxidgene_db::sea_orm::DatabaseConnection;
use serde::Deserialize;
use uuid::Uuid;

use crate::profile::{ProfileService, invalidation};
use crate::service::history::Change;
use crate::service::patch::double_option;
use crate::service::scope::{TreeResource, begin_tx, commit_tx, require_tree_resource};

/// A place to create.
#[derive(Debug, Deserialize)]
pub struct NewPlace {
    pub name: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

/// The fields a place update changes: `None` keeps a field, and `Some(None)`
/// clears a coordinate.
#[derive(Debug, Default, Deserialize)]
pub struct PlacePatch {
    pub name: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub latitude: Option<Option<f64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub longitude: Option<Option<f64>>,
}

/// Create a place in `tree_id`. A blank name is refused.
pub async fn create_place(
    db: &DatabaseConnection,
    tree_id: Uuid,
    new: NewPlace,
) -> Result<Place, OxidGeneError> {
    require_name(&new.name)?;
    let id = Uuid::now_v7();
    let txn = begin_tx(db).await?;
    let pending = Change::create(tree_id, AuditEntity::Place, id)
        .place(id)
        .prepare(&txn)
        .await?;
    let place = PlaceRepo::create(&txn, id, tree_id, new.name, new.latitude, new.longitude).await?;
    pending.record(&txn).await?;
    commit_tx(txn).await?;
    Ok(place)
}

/// Update place `id` of `tree_id`. A blank name is refused.
pub async fn update_place(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    id: Uuid,
    patch: PlacePatch,
) -> Result<Place, OxidGeneError> {
    if let Some(name) = &patch.name {
        require_name(name)?;
    }
    let txn = begin_tx(db).await?;
    require_tree_resource(&txn, tree_id, TreeResource::Place, id).await?;
    let affected = invalidation::affected_persons_for_place(&txn, id).await?;
    let pending = Change::update(tree_id, AuditEntity::Place, id)
        .place(id)
        .prepare(&txn)
        .await?;
    let place = PlaceRepo::update(&txn, id, patch.name, patch.latitude, patch.longitude).await?;
    profiles
        .invalidate_for_mutation(&txn, tree_id, &affected)
        .await?;
    pending.record(&txn).await?;
    commit_tx(txn).await?;
    Ok(place)
}

/// Delete place `id` of `tree_id`; the events there lose their place.
pub async fn delete_place(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    id: Uuid,
) -> Result<(), OxidGeneError> {
    let txn = begin_tx(db).await?;
    require_tree_resource(&txn, tree_id, TreeResource::Place, id).await?;
    let affected = invalidation::affected_persons_for_place(&txn, id).await?;
    // The deleted place's events lose their place: their owners change too.
    let pending = Change::delete(tree_id, AuditEntity::Place, id)
        .place(id)
        .persons(affected.iter().copied())
        .prepare(&txn)
        .await?;
    PlaceRepo::delete(&txn, id).await?;
    profiles
        .invalidate_for_mutation(&txn, tree_id, &affected)
        .await?;
    pending.record(&txn).await?;
    commit_tx(txn).await
}

fn require_name(name: &str) -> Result<(), OxidGeneError> {
    if name.trim().is_empty() {
        return Err(OxidGeneError::Validation(
            "name must not be empty".to_string(),
        ));
    }
    Ok(())
}
