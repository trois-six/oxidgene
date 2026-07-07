//! REST handlers for GEDCOM import and export.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use oxidgene_core::OxidGeneError;
use uuid::Uuid;

use super::dto::{
    ExportGedcomQuery, ExportGedcomResponse, ImportGedcomRequest, ImportGedcomResponse,
};
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

    // Eagerly rebuild the entire cache for this tree after GEDCOM import
    state
        .cache
        .rebuild_tree_full(tree_id)
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
/// Export all entities in a tree as a GEDCOM 5.5.1 string. Pass
/// `?format=gedzip` to instead receive a GEDZIP archive (`application/zip`)
/// wrapping the same GEDCOM data.
pub async fn export_gedcom_handler(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<ExportGedcomQuery>,
) -> Result<Response, ApiError> {
    let data = gedcom::load_and_export(&state.db, tree_id)
        .await
        .map_err(ApiError::from)?;

    if query.format.as_deref() == Some("gedzip") {
        let bytes = oxidgene_gedcom::export::export_gedzip(&data.gedcom)
            .map_err(OxidGeneError::Gedcom)
            .map_err(ApiError::from)?;

        return Ok((
            [
                (header::CONTENT_TYPE, "application/zip"),
                (
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=\"export.gdz\"",
                ),
            ],
            bytes,
        )
            .into_response());
    }

    Ok(Json(ExportGedcomResponse {
        gedcom: data.gedcom,
        warnings: data.warnings,
    })
    .into_response())
}
