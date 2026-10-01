//! REST handlers for Family CRUD operations.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use oxidgene_core::OxidGeneError;
use oxidgene_core::types::{Connection, Family};
use oxidgene_db::repo::{FamilyRepo, PaginationParams};
use uuid::Uuid;

use super::dto::PaginationQuery;
use super::error::ApiError;
use super::state::AppState;
use crate::service::family::{self, FamilyPatch};
use crate::service::scope::{TreeResource, require_tree_resource};

/// GET /api/v1/trees/:tree_id/families
pub async fn list_families(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<PaginationQuery>,
) -> Result<Json<Connection<Family>>, ApiError> {
    let params = PaginationParams {
        first: query.first.unwrap_or(25),
        after: query.after,
    };
    Ok(Json(
        FamilyRepo::list(&state.reader, tree_id, &params).await?,
    ))
}

/// POST /api/v1/trees/:tree_id/families
pub async fn create_family(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
) -> Result<(StatusCode, Json<Family>), ApiError> {
    let family = family::create_family(&state.db, tree_id).await?;
    Ok((StatusCode::CREATED, Json(family)))
}

/// GET /api/v1/trees/:tree_id/families/:family_id
pub async fn get_family(
    State(state): State<AppState>,
    Path((tree_id, family_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Family>, ApiError> {
    require_tree_resource(&state.reader, tree_id, TreeResource::Family, family_id).await?;
    Ok(Json(FamilyRepo::get(&state.reader, family_id).await?))
}

/// PUT /api/v1/trees/:tree_id/families/:family_id
///
/// The body is optional: this route began as a bare "touch `updated_at`"
/// and is still called with no body at all, which an extractor expecting
/// JSON would refuse. A body that is there must be a valid update.
pub async fn update_family(
    State(state): State<AppState>,
    Path((tree_id, family_id)): Path<(Uuid, Uuid)>,
    body: axum::body::Bytes,
) -> Result<Json<Family>, ApiError> {
    let patch = if body.iter().all(u8::is_ascii_whitespace) {
        FamilyPatch::default()
    } else {
        serde_json::from_slice(&body)
            .map_err(|_| OxidGeneError::Validation("malformed family update".to_string()))?
    };
    Ok(Json(
        family::update_family(&state.db, tree_id, family_id, patch).await?,
    ))
}

/// DELETE /api/v1/trees/:tree_id/families/:family_id
pub async fn delete_family(
    State(state): State<AppState>,
    Path((tree_id, family_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    family::delete_family(&state.db, &state.profiles, tree_id, family_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/trees/:tree_id/families/:family_id/detail-bundle
///
/// Everything the couple page draws in one answer: the family, its spouses,
/// each spouse's person bundle, the family's and the spouses' notes, and the
/// family's own media.
pub async fn get_couple_detail_bundle(
    State(state): State<AppState>,
    Path((tree_id, family_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<crate::service::couple_detail::CoupleDetailBundle>, ApiError> {
    Ok(Json(
        crate::service::couple_detail::load_couple_detail_bundle(&state.reader, tree_id, family_id)
            .await?,
    ))
}
