//! REST handlers for Place CRUD operations.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use oxidgene_core::types::{Connection, Place};
use oxidgene_db::repo::{PaginationParams, PlaceFilter, PlaceRepo};
use uuid::Uuid;

use super::dto::PlaceListQuery;
use super::error::ApiError;
use super::state::AppState;
use crate::service::place::{self, NewPlace, PlacePatch};
use crate::service::scope::{TreeResource, require_tree_resource};

/// GET /api/v1/trees/:tree_id/places
pub async fn list_places(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<PlaceListQuery>,
) -> Result<Json<Connection<Place>>, ApiError> {
    let params = PaginationParams {
        first: query.first.unwrap_or(25),
        after: query.after,
    };
    let ids = query
        .ids
        .as_deref()
        .map(super::dto::comma_separated_ids)
        .transpose()?;
    let filter = PlaceFilter {
        search: query.search.as_deref(),
        name: query.name.as_deref(),
        ids: ids.as_deref(),
    };
    let places = PlaceRepo::list_filtered(&state.reader, tree_id, &filter, &params).await?;
    Ok(Json(places))
}

/// POST /api/v1/trees/:tree_id/places
pub async fn create_place(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<NewPlace>,
) -> Result<(StatusCode, Json<Place>), ApiError> {
    let place = place::create_place(&state.db, tree_id, body).await?;
    Ok((StatusCode::CREATED, Json(place)))
}

/// GET /api/v1/trees/:tree_id/places/:place_id
pub async fn get_place(
    State(state): State<AppState>,
    Path((tree_id, place_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Place>, ApiError> {
    require_tree_resource(&state.reader, tree_id, TreeResource::Place, place_id).await?;
    Ok(Json(PlaceRepo::get(&state.reader, place_id).await?))
}

/// PUT /api/v1/trees/:tree_id/places/:place_id
pub async fn update_place(
    State(state): State<AppState>,
    Path((tree_id, place_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<PlacePatch>,
) -> Result<Json<Place>, ApiError> {
    let place = place::update_place(&state.db, &state.profiles, tree_id, place_id, body).await?;
    Ok(Json(place))
}

/// DELETE /api/v1/trees/:tree_id/places/:place_id
pub async fn delete_place(
    State(state): State<AppState>,
    Path((tree_id, place_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    place::delete_place(&state.db, &state.profiles, tree_id, place_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
