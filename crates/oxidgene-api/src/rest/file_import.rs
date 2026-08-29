//! Streamed uploads and asynchronous genealogy file imports.

use std::path::{Path as FilePath, PathBuf};
use std::sync::Arc;

use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use futures_util::StreamExt;
use oxidgene_core::OxidGeneError;
use oxidgene_db::repo::TreeRepo;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use super::dto::{
    FileImportFormat, FileImportStartedResponse, FileImportStatusResponse, ImportResponse,
    StartFileImportQuery,
};
use super::error::ApiError;
use super::state::AppState;
use crate::service::{gedcom, geneweb};

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

    let progress = Arc::new(gedcom::FileImportProgress::default());
    state
        .file_imports
        .lock()
        .map_err(|_| {
            ApiError(OxidGeneError::Internal(
                "import registry lock poisoned".into(),
            ))
        })?
        .insert(job_id, (tree_id, Arc::clone(&progress)));

    tokio::spawn(run(
        state,
        tree_id,
        job_id,
        upload,
        query.format,
        query.filename,
        progress,
    ));

    Ok((
        StatusCode::ACCEPTED,
        Json(FileImportStartedResponse { job_id }),
    ))
}

/// GET /api/v1/trees/:tree_id/import-jobs/:job_id
pub async fn status(
    State(state): State<AppState>,
    Path((tree_id, job_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<FileImportStatusResponse>, ApiError> {
    let progress = state
        .file_imports
        .lock()
        .map_err(|_| {
            ApiError(OxidGeneError::Internal(
                "import registry lock poisoned".into(),
            ))
        })?
        .get(&job_id)
        .filter(|(owner_tree_id, _)| *owner_tree_id == tree_id)
        .map(|(_, progress)| Arc::clone(progress))
        .ok_or(ApiError(OxidGeneError::NotFound {
            entity: "ImportJob",
            id: job_id,
        }))?;
    let (phase, done, total, result, error) = progress.read();
    Ok(Json(FileImportStatusResponse {
        phase,
        done,
        total,
        result: result.map(import_response),
        error,
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

async fn run(
    state: AppState,
    tree_id: Uuid,
    job_id: Uuid,
    upload: TemporaryUpload,
    format: FileImportFormat,
    filename: Option<String>,
    progress: Arc<gedcom::FileImportProgress>,
) {
    let path = upload.path();
    let result = match format {
        FileImportFormat::Gedcom => {
            gedcom::import_file_and_persist(&state.db, tree_id, path, &progress).await
        }
        FileImportFormat::Gedzip => {
            gedcom::import_gedzip_file_and_persist(
                &state.db,
                &*state.media,
                tree_id,
                path,
                &progress,
            )
            .await
        }
        FileImportFormat::Geneweb => {
            let origin = safe_origin_file(filename.as_deref());
            geneweb::import_file_and_persist(&state.db, tree_id, path, &origin, &progress).await
        }
    };

    let result = match result {
        Ok(summary) => {
            progress.enter(gedcom::FileImportPhase::Projections);
            state
                .profiles
                .rebuild_tree_full(&state.db, tree_id)
                .await
                .map(|_| summary)
        }
        Err(error) => Err(error),
    };

    match result {
        Ok(summary) => progress.complete(summary),
        Err(error) => {
            let code = match error {
                OxidGeneError::Gedcom(_) | OxidGeneError::Validation(_) => "invalid_import_file",
                _ => "import_failed",
            };
            tracing::error!(%job_id, %error, "asynchronous file import failed");
            progress.fail(code);
        }
    }
}

fn temporary_path(job_id: Uuid) -> PathBuf {
    temporary_root().join(job_id.to_string())
}

fn temporary_root() -> PathBuf {
    std::env::temp_dir().join("oxidgene-imports")
}

fn safe_origin_file(filename: Option<&str>) -> String {
    filename
        .and_then(|name| FilePath::new(name).file_name())
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("import.gw")
        .to_string()
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
    fn geneweb_origin_is_metadata_not_a_path() {
        assert_eq!(safe_origin_file(Some("../../same-name.gw")), "same-name.gw");
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
