//! REST handlers for GEDCOM import and export.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use uuid::Uuid;

use super::dto::{ExportGedcomResponse, ImportGedcomRequest, ImportGedcomResponse};
use super::error::ApiError;
use super::state::AppState;
use crate::service::gedcom;

/// POST /api/v1/trees/:tree_id/import
///
/// Import a GEDCOM string into the given tree, persisting all extracted entities.
pub async fn import_gedcom_handler(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<ImportGedcomRequest>,
) -> Result<(StatusCode, Json<ImportGedcomResponse>), ApiError> {
    let summary = gedcom::import_and_persist(&state.db, tree_id, &body.gedcom)
        .await
        .map_err(ApiError::from)?;

    let response = ImportGedcomResponse {
        persons_count: summary.persons_count,
        families_count: summary.families_count,
        events_count: summary.events_count,
        sources_count: summary.sources_count,
        media_count: summary.media_count,
        places_count: summary.places_count,
        notes_count: summary.notes_count,
        warnings: summary.warnings,
    };

    Ok((StatusCode::CREATED, Json(response)))
}

/// GET /api/v1/trees/:tree_id/export
///
/// Export all entities in a tree as a GEDCOM 5.5.1 string.
pub async fn export_gedcom_handler(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
) -> Result<Json<ExportGedcomResponse>, ApiError> {
    let data = gedcom::load_and_export(&state.db, tree_id)
        .await
        .map_err(ApiError::from)?;

    Ok(Json(ExportGedcomResponse {
        gedcom: data.gedcom,
        warnings: data.warnings,
    }))
}
