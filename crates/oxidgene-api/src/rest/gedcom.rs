//! REST handler for GEDCOM and GEDZIP export.
//!
//! Imports have no handler here: a genealogy file is imported only as a job
//! (see [`super::file_import`]).

use super::authorize::Viewer;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use oxidgene_core::OxidGeneError;
use uuid::Uuid;

use super::dto::{ExportGedcomQuery, ExportGedcomResponse};
use super::error::ApiError;
use super::state::AppState;
use crate::service::gedcom;
use crate::service::history;

/// GET /api/v1/gedcom/export-destinations
///
/// The programs a plain GEDCOM can be written for, as GraphQL's
/// `exportDestinations` lists them: the standard versions first, then the
/// programs by name.
pub async fn export_destinations() -> Json<Vec<gedcom::ExportDestination>> {
    Json(gedcom::export_destinations())
}

/// GET /api/v1/trees/:tree_id/gedcom/export
///
/// Export all entities in a tree as a GEDCOM string written for
/// `?destination=` (an export destination's id; standard GEDCOM 5.5.1 by
/// default), with what writing for it lost (`report`). Pass
/// `?format=gedzip` to instead receive a GEDZIP archive (`application/zip`):
/// the same records as standard GEDCOM 7.0, which GEDZIP requires, with the
/// media; it takes no destination. Pass `?include_notes_and_sources=false`
/// to leave the notes and the sources out, and
/// `?format=gedzip&include_media=false` to leave the media out of an
/// archive. `?scope=` (the target as JSON, URL-encoded) limits the export
/// to the persons it names and what they reach. `?leave_out_private=`,
/// `?leave_out_excluded=` and `?apply_visitor_rules=` leave persons out for
/// privacy, and an exporter who is not the owner never writes what their
/// access hides (docs/privacy.md §8). Either way the export is recorded in
/// the tree's audit log, with its privacy options, as GraphQL's
/// `exportGedcom` records it.
pub async fn export_gedcom_handler(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<ExportGedcomQuery>,
) -> Result<Response, ApiError> {
    let choices = query.choices()?;
    let scope = query.scope()?;
    let privacy = gedcom::viewer_privacy(&state.reader, &viewer, tree_id, query.privacy()).await?;
    if query.format.as_deref() != Some("gedzip") {
        let data = gedcom::export_gedcom(
            &viewer,
            &state.db,
            tree_id,
            choices,
            scope.as_ref(),
            privacy,
        )
        .await?;
        return Ok(Json(ExportGedcomResponse {
            gedcom: data.gedcom,
            warnings: data.warnings,
            report: data.report.unwrap_or_default(),
        })
        .into_response());
    }
    if query.destination.is_some() {
        return Err(ApiError(OxidGeneError::Validation(
            "a GEDZIP is standard GEDCOM 7.0 and takes no destination".to_string(),
        )));
    }

    let data =
        gedcom::load_and_export(&state.db, tree_id, choices, scope.as_ref(), true, privacy).await?;
    // Staged on disk, never whole in memory: an archive of scans is
    // gigabytes. The file goes when the response has been streamed.
    let staged = state
        .work_dir
        .staged_file("export-")
        .map_err(OxidGeneError::Io)?
        .into_temp_path();
    gedcom::write_gedzip_file(
        &*state.media,
        &state.work_dir,
        &staged,
        data.gedcom,
        data.media_files,
    )
    .await?;
    history::record_export(
        &viewer,
        &state.db,
        tree_id,
        "gedzip",
        None,
        gedcom::scope_details(scope.as_ref())?,
        privacy.details(),
    )
    .await?;

    let file = tokio::fs::File::open(&staged)
        .await
        .map_err(OxidGeneError::Io)?;
    let length = file.metadata().await.map_err(OxidGeneError::Io)?.len();
    let stream =
        futures_util::stream::try_unfold((file, staged), |(mut file, staged)| async move {
            use tokio::io::AsyncReadExt as _;
            let mut buffer = vec![0; 64 * 1024];
            let read = file.read(&mut buffer).await?;
            buffer.truncate(read);
            Ok::<_, std::io::Error>((read != 0).then_some((buffer, (file, staged))))
        });
    Ok((
        [
            (header::CONTENT_TYPE, "application/zip".to_string()),
            (header::CONTENT_LENGTH, length.to_string()),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"export.gdz\"".to_string(),
            ),
        ],
        axum::body::Body::from_stream(stream),
    )
        .into_response())
}
