//! REST handler for the tree snapshot endpoint.
//!
//! Returns all data needed to render the pedigree chart in a single response:
//! persons, names, families, spouses, children, events, and places.

use axum::Json;
use axum::extract::{Path, State};
use oxidgene_core::types::{Event, FamilyChild, FamilySpouse, Person, PersonName, Place};
use oxidgene_db::repo::{
    EventRepo, FamilyChildRepo, FamilyRepo, FamilySpouseRepo, PersonNameRepo, PersonRepo, PlaceRepo,
};
use serde::Serialize;
use uuid::Uuid;

use super::error::ApiError;
use super::state::AppState;

/// Response for the tree snapshot endpoint.
#[derive(Debug, Serialize)]
pub struct TreeSnapshotResponse {
    pub persons: Vec<Person>,
    pub names: Vec<PersonName>,
    pub events: Vec<Event>,
    pub places: Vec<Place>,
    pub spouses: Vec<FamilySpouse>,
    pub children: Vec<FamilyChild>,
}

/// GET /api/v1/trees/:tree_id/snapshot
pub async fn tree_snapshot(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
) -> Result<Json<TreeSnapshotResponse>, ApiError> {
    // Fetch persons and families first (needed for bulk lookups).
    let (persons, families) = tokio::try_join!(
        PersonRepo::list_all(&state.db, tree_id),
        FamilyRepo::list_all(&state.db, tree_id),
    )
    .map_err(ApiError::from)?;

    let person_ids: Vec<Uuid> = persons.iter().map(|p| p.id).collect();
    let family_ids: Vec<Uuid> = families.iter().map(|f| f.id).collect();

    // Fetch names, events, places, spouses, and children in parallel.
    let (names, events, places, spouses, children) = tokio::try_join!(
        PersonNameRepo::list_by_persons(&state.db, &person_ids),
        EventRepo::list_all(&state.db, tree_id),
        PlaceRepo::list_all(&state.db, tree_id),
        FamilySpouseRepo::list_by_families(&state.db, &family_ids),
        FamilyChildRepo::list_by_families(&state.db, &family_ids),
    )
    .map_err(ApiError::from)?;

    Ok(Json(TreeSnapshotResponse {
        persons,
        names,
        events,
        places,
        spouses,
        children,
    }))
}
