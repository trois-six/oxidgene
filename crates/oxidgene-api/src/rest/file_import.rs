//! Streamed uploads and asynchronous genealogy file imports.

use std::path::{Path as FilePath, PathBuf};

use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use futures_util::StreamExt;
use oxidgene_core::OxidGeneError;
use oxidgene_db::repo::{BackgroundJobKind, BackgroundJobRepo, NewBackgroundJob, TreeRepo};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use super::dto::{
    FileImportStartedResponse, FileImportStatusResponse, ImportResponse, StartFileImportQuery,
};
use super::error::ApiError;
use super::state::AppState;
use crate::media::store::job_blob_key;
use crate::service::gedcom;

pub const FILE_IMPORT_BODY_LIMIT: usize = 1024 * 1024 * 1024;

struct TemporaryUpload(PathBuf);

impl TemporaryUpload {
    fn new(job_id: Uuid) -> Self {
        Self(temporary_path(job_id))
    }

    fn path(&self) -> &FilePath {
        &self.0
    }
}

impl Drop for TemporaryUpload {
    fn drop(&mut self) {
        match std::fs::remove_file(&self.0) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => tracing::warn!(%error, "could not remove temporary import file"),
        }
    }
}

/// Remove uploads left by a process that could not run destructors.
pub fn cleanup_orphaned_uploads() -> Result<(), std::io::Error> {
    cleanup_directory(&temporary_root())
}

fn cleanup_directory(path: &FilePath) -> Result<(), std::io::Error> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
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

    let job_id = Uuid::now_v7();
    let upload = TemporaryUpload::new(job_id);
    if let Some(parent) = upload.path().parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(OxidGeneError::Io)?;
    }
    stream_to_file(body, upload.path()).await?;
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
) -> Result<Json<FileImportStatusResponse>, ApiError> {
    let job = BackgroundJobRepo::get_in_tree(&state.db, tree_id, job_id).await?;
    if job.kind != BackgroundJobKind::Import.as_str() {
        return Err(ApiError(OxidGeneError::NotFound {
            entity: "ImportJob",
            id: job_id,
        }));
    }
    let result = job
        .result_json
        .as_deref()
        .map(serde_json::from_str::<gedcom::ImportSummary>)
        .transpose()
        .map_err(|error| ApiError(OxidGeneError::Internal(error.to_string())))?;
    Ok(Json(FileImportStatusResponse {
        phase: job.phase,
        done: as_usize(job.done),
        total: as_usize(job.total),
        result: result.map(import_response),
        error: job.error_code,
    }))
}

async fn stream_to_file(body: Body, path: &FilePath) -> Result<(), OxidGeneError> {
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .await?;
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

fn temporary_path(job_id: Uuid) -> PathBuf {
    temporary_root().join(job_id.to_string())
}

fn temporary_root() -> PathBuf {
    std::env::temp_dir().join("oxidgene-imports")
}

fn import_response(summary: gedcom::ImportSummary) -> ImportResponse {
    ImportResponse {
        persons_count: summary.persons_count,
        families_count: summary.families_count,
        events_count: summary.events_count,
        sources_count: summary.sources_count,
        media_count: summary.media_count,
        places_count: summary.places_count,
        notes_count: summary.notes_count,
        warnings: summary.warnings,
    }
}

fn as_usize(value: i64) -> usize {
    usize::try_from(value).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temporary_names_depend_only_on_the_operation_id() {
        let first = Uuid::now_v7();
        let second = Uuid::now_v7();

        assert_eq!(
            temporary_path(first).file_name().unwrap().to_str().unwrap(),
            first.to_string()
        );
        assert_ne!(temporary_path(first), temporary_path(second));
    }

    #[test]
    fn dropping_an_upload_removes_its_partial_file() {
        let upload = TemporaryUpload::new(Uuid::now_v7());
        std::fs::create_dir_all(upload.path().parent().unwrap()).unwrap();
        std::fs::write(upload.path(), b"partial").unwrap();
        let path = upload.path().to_path_buf();

        drop(upload);

        assert!(!path.exists());
    }

    #[test]
    fn startup_cleanup_removes_an_orphaned_directory() {
        let root = std::env::temp_dir().join(format!("oxidgene-cleanup-test-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(Uuid::now_v7().to_string()), b"orphaned").unwrap();

        cleanup_directory(&root).unwrap();

        assert!(!root.exists());
    }
}
