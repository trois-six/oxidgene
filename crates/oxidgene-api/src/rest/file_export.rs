//! Asynchronous GEDZIP export jobs and artifact downloads.

use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use oxidgene_core::OxidGeneError;
use oxidgene_db::repo::{
    BackgroundJobKind, BackgroundJobRepo, BackgroundJobStatus, NewBackgroundJob, TreeRepo,
};
use serde::Deserialize;
use uuid::Uuid;

use super::dto::{ExportJobStartedResponse, ExportJobStatusResponse, StartExportJobQuery};
use super::error::ApiError;
use super::state::AppState;

/// POST /api/v1/trees/:tree_id/export-jobs
pub async fn start(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<StartExportJobQuery>,
) -> Result<(StatusCode, Json<ExportJobStartedResponse>), ApiError> {
    TreeRepo::get(&state.db, tree_id).await?;
    let job_id = Uuid::now_v7();
    BackgroundJobRepo::create(
        &state.db,
        NewBackgroundJob {
            id: job_id,
            tree_id,
            kind: BackgroundJobKind::Export,
            format: "gedzip".into(),
            source_key: None,
            payload_json: None,
            original_filename: None,
            merge_occupations: query.merge_occupations.unwrap_or(false),
            merge_names: query.merge_names.unwrap_or(false),
        },
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
) -> Result<Json<ExportJobStatusResponse>, ApiError> {
    let job = export_job(&state, tree_id, job_id).await?;
    let result = job
        .result_json
        .as_deref()
        .map(serde_json::from_str::<ExportResult>)
        .transpose()
        .map_err(|error| ApiError(OxidGeneError::Internal(error.to_string())))?;
    let download_url = (job.status == BackgroundJobStatus::Completed.as_str())
        .then(|| format!("/api/v1/trees/{tree_id}/export-jobs/{job_id}/download"));
    Ok(Json(ExportJobStatusResponse {
        phase: job.phase,
        done: as_usize(job.done),
        total: as_usize(job.total),
        download_url,
        warnings: result.map_or_else(Vec::new, |result| result.warnings),
        error: job.error_code,
    }))
}

/// GET /api/v1/trees/:tree_id/export-jobs/:job_id/download
pub async fn download(
    State(state): State<AppState>,
    Path((tree_id, job_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    let job = export_job(&state, tree_id, job_id).await?;
    if job.status != BackgroundJobStatus::Completed.as_str() {
        return Err(ApiError(OxidGeneError::Validation(
            "export artifact is not ready".into(),
        )));
    }
    let artifact_key = job
        .artifact_key
        .as_deref()
        .ok_or_else(|| ApiError(OxidGeneError::Internal("export artifact is missing".into())))?;
    let stream = state.media.get_stream(artifact_key).await?;
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

async fn export_job(
    state: &AppState,
    tree_id: Uuid,
    job_id: Uuid,
) -> Result<oxidgene_db::repo::BackgroundJob, ApiError> {
    let job = BackgroundJobRepo::get_in_tree(&state.db, tree_id, job_id).await?;
    if job.kind == BackgroundJobKind::Export.as_str() {
        Ok(job)
    } else {
        Err(ApiError(OxidGeneError::NotFound {
            entity: "ExportJob",
            id: job_id,
        }))
    }
}

fn as_usize(value: i64) -> usize {
    usize::try_from(value).unwrap_or_default()
}

#[derive(Deserialize)]
struct ExportResult {
    warnings: Vec<String>,
}
