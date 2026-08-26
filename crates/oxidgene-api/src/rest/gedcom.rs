//! REST handlers for GEDCOM import and export.

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use oxidgene_core::OxidGeneError;
use uuid::Uuid;

use super::dto::{ExportGedcomQuery, ExportGedcomResponse, ImportGedcomRequest, ImportResponse};
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
) -> Result<(StatusCode, Json<ImportResponse>), ApiError> {
    let summary = gedcom::import_and_persist(&state.db, tree_id, &body.gedcom)
        .await
        .map_err(ApiError::from)?;

    // Eagerly rebuild every projection of this tree after a GEDCOM import
    state
        .profiles
        .rebuild_tree_full(&state.db, tree_id)
        .await
        .map_err(ApiError::from)?;

    let response = ImportResponse {
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

/// POST /api/v1/trees/:tree_id/gedzip/import
///
/// Import a GEDZIP archive (`.gdz`) into the given tree: the genealogy from
/// the `gedcom.ged` it wraps, plus every media file it carries.
///
/// The body is the **raw archive**, not JSON — a ZIP is bytes, and base64 in a
/// JSON envelope would inflate a photo album by a third for nothing.
pub async fn import_gedzip_handler(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    body: Bytes,
) -> Result<(StatusCode, Json<ImportResponse>), ApiError> {
    let summary = gedcom::import_gedzip_and_persist(&state.db, &*state.media, tree_id, &body)
        .await
        .map_err(ApiError::from)?;

    // Eagerly rebuild every projection of this tree — same rationale as the
    // plain GEDCOM path above.
    state
        .profiles
        .rebuild_tree_full(&state.db, tree_id)
        .await
        .map_err(ApiError::from)?;

    let response = ImportResponse {
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
/// wrapping the same GEDCOM data. Pass `?merge_occupations=true` to collapse
/// each person's multiple `OCCU` tags back into one, comma-separated. Pass
/// `?merge_names=true` to collapse each person's non-primary names into the
/// primary name's `SURN` tag, comma-separated.
pub async fn export_gedcom_handler(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<ExportGedcomQuery>,
) -> Result<Response, ApiError> {
    let for_archive = query.format.as_deref() == Some("gedzip");
    let data = gedcom::load_and_export(
        &state.db,
        tree_id,
        query.merge_occupations.unwrap_or(false),
        query.merge_names.unwrap_or(false),
        for_archive,
    )
    .await
    .map_err(ApiError::from)?;

    if for_archive {
        // The whole reason to choose this format over `.ged`. A medium whose
        // bytes have gone missing from the store is skipped rather than
        // fatal: the rest of the archive is still a correct export, and
        // refusing to produce one over a single absent file would be worse
        // than producing one whose `FILE` names it.
        let mut files = Vec::with_capacity(data.media_files.len());
        for (key, path) in &data.media_files {
            match state.media.get(key).await {
                Ok(bytes) => files.push((path.clone(), bytes)),
                Err(_) => tracing::warn!(
                    error = "media_store_read",
                    "media absent from the store; not packed"
                ),
            }
        }

        let bytes = oxidgene_gedcom::export::export_gedzip(&data.gedcom, &files)
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
