//! REST handler for GEDCOM and GEDZIP export.
//!
//! Imports have no handler here: a genealogy file is imported only as a job
//! (see [`super::file_import`]).

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use oxidgene_core::OxidGeneError;
use tracing::Instrument as _;
use uuid::Uuid;

use super::dto::{ExportGedcomQuery, ExportGedcomResponse};
use super::error::ApiError;
use super::state::AppState;
use crate::service::gedcom;
use crate::service::history;

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
