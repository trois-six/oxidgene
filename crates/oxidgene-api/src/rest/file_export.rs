//! Asynchronous GEDZIP export jobs and artifact downloads.

use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use uuid::Uuid;

use super::dto::{ExportJobStartedResponse, StartExportJobQuery};
use super::error::ApiError;
use super::state::AppState;
use crate::service::background_job::{self, ExportJobStatus};

/// POST /api/v1/trees/:tree_id/export-jobs
pub async fn start(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<StartExportJobQuery>,
) -> Result<(StatusCode, Json<ExportJobStartedResponse>), ApiError> {
    let job_id = background_job::start_export_job(
        &state.db,
        tree_id,
        query.merge_occupations.unwrap_or(false),
        query.merge_names.unwrap_or(false),
    )
    .await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(ExportJobStartedResponse { job_id }),
    ))
}

/// GET /api/v1/trees/:tree_id/export-jobs/:job_id
pub async fn status(
    State(state): State<AppState>,
    Path((tree_id, job_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ExportJobStatus>, ApiError> {
    Ok(Json(
        background_job::export_job_status(&state.db, tree_id, job_id).await?,
    ))
}

/// GET /api/v1/trees/:tree_id/export-jobs/:job_id/download
pub async fn download(
    State(state): State<AppState>,
    Path((tree_id, job_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    let artifact_key = background_job::export_artifact(&state.db, tree_id, job_id).await?;
    // Kept for its hour whatever the downloads: a save that went wrong can
    // be downloaded again.
    let stream = state.media.get_stream(&artifact_key).await?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/zip"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"export.gdz\"",
            ),
        ],
        Body::from_stream(stream),
    )
        .into_response())
}
