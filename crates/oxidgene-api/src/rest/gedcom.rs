//! REST handlers for GEDCOM import and export.

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use oxidgene_core::OxidGeneError;
use tracing::Instrument as _;
use uuid::Uuid;

use super::dto::{ExportGedcomQuery, ExportGedcomResponse, ImportGedcomRequest};
use super::error::ApiError;
use super::state::AppState;
use crate::service::gedcom::{self, ImportSummary};
use crate::service::history;

/// POST /api/v1/trees/:tree_id/gedcom/import
///
/// Import a GEDCOM string into the given tree, persisting all extracted entities.
pub async fn import_gedcom_handler(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<ImportGedcomRequest>,
) -> Result<(StatusCode, Json<ImportSummary>), ApiError> {
    let summary = gedcom::import_and_persist(&state.db, tree_id, &body.gedcom).await?;
    gedcom::finish_import(
        &state.db,
        &state.profiles,
        tree_id,
        "gedcom",
        None,
        &summary,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(summary)))
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
) -> Result<(StatusCode, Json<ImportSummary>), ApiError> {
    let summary =
        gedcom::import_gedzip_and_persist(&state.db, &*state.media, tree_id, &body).await?;
    gedcom::finish_import(
        &state.db,
        &state.profiles,
        tree_id,
        "gedzip",
        None,
        &summary,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(summary)))
}

/// GET /api/v1/trees/:tree_id/gedcom/export
///
/// Export all entities in a tree as a GEDCOM 5.5.1 string. Pass
/// `?format=gedzip` to instead receive a GEDZIP archive (`application/zip`)
/// wrapping the same GEDCOM data. Pass `?merge_occupations=true` to collapse
/// each person's multiple `OCCU` tags back into one, comma-separated. Pass
/// `?merge_names=true` to collapse each person's non-primary names into the
/// primary name's `SURN` tag, comma-separated. Either way the export is
/// recorded in the tree's audit log, as GraphQL's `exportGedcom` records it.
pub async fn export_gedcom_handler(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<ExportGedcomQuery>,
) -> Result<Response, ApiError> {
    let merge_occupations = query.merge_occupations.unwrap_or(false);
    let merge_names = query.merge_names.unwrap_or(false);
    if query.format.as_deref() != Some("gedzip") {
        let data =
            gedcom::export_gedcom(&state.db, tree_id, merge_occupations, merge_names).await?;
        return Ok(Json(ExportGedcomResponse {
            gedcom: data.gedcom,
            warnings: data.warnings,
        })
        .into_response());
    }

    let data =
        gedcom::load_and_export(&state.db, tree_id, merge_occupations, merge_names, true).await?;
    // The whole reason to choose this format over `.ged`. A medium whose
    // bytes have gone missing from the store is skipped rather than fatal:
    // the rest of the archive is still a correct export, and refusing to
    // produce one over a single absent file would be worse than producing
    // one whose `FILE` names it.
    let media_span = tracing::info_span!(
        "export.media",
        export.format = "gedzip",
        export.media.count = data.media_files.len(),
    );
    let mut files = Vec::with_capacity(data.media_files.len());
    async {
        for (key, path, mime_type) in &data.media_files {
            match state.media.get(key).await {
                Ok(bytes) => files.push((path.clone(), mime_type.clone(), bytes)),
                Err(_) => tracing::warn!(
                    error = "media_store_read",
                    "media absent from the store; not packed"
                ),
            }
        }
    }
    .instrument(media_span)
    .await;

    let bytes = tracing::info_span!("export.package", export.format = "gedzip")
        .in_scope(|| oxidgene_gedcom::export::export_gedzip(&data.gedcom, &files))
        .map_err(OxidGeneError::Gedcom)?;
    history::record_export(&state.db, tree_id, "gedzip", None).await?;

    Ok((
        [
            (header::CONTENT_TYPE, "application/zip"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"export.gdz\"",
            ),
        ],
        bytes,
    )
        .into_response())
}
