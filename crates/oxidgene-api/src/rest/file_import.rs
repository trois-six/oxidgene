//! Streamed uploads and asynchronous genealogy file imports.

use std::path::Path as FilePath;

use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use futures_util::StreamExt;
use oxidgene_core::OxidGeneError;
use oxidgene_db::repo::{BackgroundJobKind, BackgroundJobRepo, NewBackgroundJob, TreeRepo};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use super::dto::{FileImportStartedResponse, StartFileImportQuery};
use super::error::ApiError;
use super::state::AppState;
use crate::media::store::job_blob_key;
use crate::service::background_job::{self, ImportJobStatus};

pub const FILE_IMPORT_BODY_LIMIT: usize = 1024 * 1024 * 1024;

struct TemporaryUpload(tempfile::NamedTempFile);

impl TemporaryUpload {
    fn new() -> Result<Self, std::io::Error> {
        tempfile::Builder::new()
            .prefix("oxidgene-import-")
            .tempfile()
            .map(Self)
    }

    fn path(&self) -> &FilePath {
        self.0.path()
    }

    fn reopen(&self) -> Result<std::fs::File, std::io::Error> {
        self.0.reopen()
    }
}

/// POST /api/v1/trees/:tree_id/import-jobs
pub async fn start(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<StartFileImportQuery>,
    body: Body,
) -> Result<(StatusCode, Json<FileImportStartedResponse>), ApiError> {
    TreeRepo::get(&state.db, tree_id).await?;
    // Before the body is read: a waiting upload holds a connection, not a
    // spool file. The slot is held until the source is in job storage.
    let _intake = crate::service::intake::slot().await?;

    let job_id = Uuid::now_v7();
    let upload = TemporaryUpload::new().map_err(OxidGeneError::Io)?;
    stream_to_file(body, upload.reopen().map_err(OxidGeneError::Io)?).await?;
    stage_import(
        &state.db,
        &*state.media,
        job_id,
        tree_id,
        query.format.as_str(),
        query.filename,
        upload.path(),
    )
    .await?;

    Ok((
        StatusCode::ACCEPTED,
        Json(FileImportStartedResponse { job_id }),
    ))
}

async fn stage_import(
    db: &impl sea_orm::ConnectionTrait,
    media: &dyn crate::media::MediaStore,
    job_id: Uuid,
    tree_id: Uuid,
    format: &str,
    filename: Option<String>,
    path: &FilePath,
) -> Result<(), OxidGeneError> {
    let source_key = job_blob_key(job_id, "source", format)?;
    media.put_file(&source_key, path).await?;
    let created = BackgroundJobRepo::create(
        db,
        NewBackgroundJob {
            id: job_id,
            tree_id,
            kind: BackgroundJobKind::Import,
            format: format.to_string(),
            source_key: Some(source_key.clone()),
            payload_json: None,
            original_filename: filename,
            merge_occupations: false,
            merge_names: false,
        },
    )
    .await;
    if let Err(error) = created {
        let _ = media.delete(&source_key).await;
        return Err(error);
    }
    Ok(())
}

/// GET /api/v1/trees/:tree_id/import-jobs/:job_id
pub async fn status(
    State(state): State<AppState>,
    Path((tree_id, job_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ImportJobStatus>, ApiError> {
    Ok(Json(
        background_job::import_job_status(&state.db, tree_id, job_id).await?,
    ))
}

async fn stream_to_file(body: Body, file: std::fs::File) -> Result<(), OxidGeneError> {
    let mut file = tokio::fs::File::from_std(file);
    let mut received = 0usize;
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| {
            OxidGeneError::Validation(format!("upload could not be read: {error}"))
        })?;
        received = received
            .checked_add(chunk.len())
            .ok_or_else(|| OxidGeneError::Validation("uploaded file is too large".into()))?;
        if received > FILE_IMPORT_BODY_LIMIT {
            return Err(OxidGeneError::Validation(format!(
                "uploaded file exceeds the {FILE_IMPORT_BODY_LIMIT}-byte limit"
            )));
        }
        file.write_all(&chunk).await?;
    }
    if received == 0 {
        return Err(OxidGeneError::Validation("uploaded file is empty".into()));
    }
    file.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropping_an_upload_removes_its_private_file() {
        let upload = TemporaryUpload::new().expect("creates private upload");
        std::fs::write(upload.path(), b"partial").expect("writes upload");
        let path = upload.path().to_path_buf();

        drop(upload);

        assert!(!path.exists());
    }
}
