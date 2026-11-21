//! Streamed uploads and asynchronous genealogy file imports.

use super::authorize::Viewer;
use std::path::Path as FilePath;

use axum::Json;
use axum::body::{Body, BodyDataStream, Bytes};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use futures_util::StreamExt;
use oxidgene_core::OxidGeneError;
use oxidgene_db::repo::TreeRepo;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use super::dto::{
    FileImportFormat, FileImportStartedResponse, JobStatusQuery, StartFileImportQuery,
};
use super::error::ApiError;
use super::state::AppState;
use crate::limits::IntakeLimits;
use crate::service::background_job::{self, ImportJobStatus};
use crate::workdir::WorkDir;

/// An upload staged in the working directory, removed when dropped.
struct TemporaryUpload(tempfile::NamedTempFile);

impl TemporaryUpload {
    fn new(work_dir: &WorkDir) -> Result<Self, std::io::Error> {
        work_dir.staged_file("import-").map(Self)
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
    viewer: Viewer,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<StartFileImportQuery>,
    body: Body,
) -> Result<(StatusCode, Json<FileImportStartedResponse>), ApiError> {
    TreeRepo::get(&state.db, tree_id).await?;
    // The slot is taken once the file arrives — a request that sends
    // nothing waits on its own connection, and a waiting upload holds no
    // spool file — and held until the source is in job storage.
    let (first, rest) = crate::service::intake::first_chunk(body).await?;
    let _intake = crate::service::intake::slot(&viewer).await?;

    let upload = TemporaryUpload::new(&state.work_dir).map_err(OxidGeneError::Io)?;
    let within = Bound {
        limits: state.limits,
        format: query.format,
    };
    stream_to_file(
        first,
        rest,
        upload.reopen().map_err(OxidGeneError::Io)?,
        within,
    )
    .await?;
    let job_id = background_job::stage_import(
        &viewer,
        &state.db,
        &*state.media,
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

/// GET /api/v1/trees/:tree_id/import-jobs/:job_id
///
/// With `since` and `wait`, answers once the job's state differs from
/// version `since`, or after `wait` seconds.
pub async fn status(
    State(state): State<AppState>,
    Path((tree_id, job_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<JobStatusQuery>,
) -> Result<Json<ImportJobStatus>, ApiError> {
    Ok(Json(
        background_job::wait_import_job_status(&state.reader, tree_id, job_id, &query.wait())
            .await?,
    ))
}

/// POST /api/v1/trees/:tree_id/import-jobs/:job_id/cancel
///
/// Stop the import and undo it: at once when it is queued; when it runs, at
/// its next batch of rows or medium, rolling back everything it wrote.
/// Answers where it stands then.
pub async fn cancel(
    State(state): State<AppState>,
    Path((tree_id, job_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ImportJobStatus>, ApiError> {
    Ok(Json(
        background_job::cancel_import_job(&state.db, &state.reader, &*state.media, tree_id, job_id)
            .await?,
    ))
}

/// What an import source is spooled within: a genealogy text — a GEDCOM,
/// a GeneWeb file — within the bound on the text the parser reads whole;
/// a GEDZIP, which is read from disk entry by entry, within what its
/// tree's quota can hold, and without a bound where there is no quota.
#[derive(Clone, Copy)]
struct Bound {
    limits: IntakeLimits,
    format: FileImportFormat,
}

impl Bound {
    fn check(self, received: u64) -> Result<(), OxidGeneError> {
        match self.format {
            FileImportFormat::Gedcom | FileImportFormat::Geneweb => {
                self.limits.check_genealogy(received)
            }
            FileImportFormat::Gedzip => self.limits.check_archive(received),
        }
    }
}

/// Write an upload — its `first` bytes and the `rest` of its body — to
/// `file`, refusing it once it passes `within`.
async fn stream_to_file(
    first: Bytes,
    mut rest: BodyDataStream,
    file: std::fs::File,
    within: Bound,
) -> Result<(), OxidGeneError> {
    let mut file = tokio::fs::File::from_std(file);
    let mut received = 0u64;
    let mut next = Some(Ok(first));
    while let Some(chunk) = next {
        let chunk = chunk.map_err(|error| {
            OxidGeneError::Validation(format!("upload could not be read: {error}"))
        })?;
        received = received.saturating_add(chunk.len() as u64);
        within.check(received)?;
        file.write_all(&chunk).await?;
        next = rest.next().await;
    }
    file.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A genealogy text is bounded by what the parser reads whole; an
    /// archive by what its tree can hold, or not at all on the desktop.
    #[test]
    fn an_import_source_is_bounded_by_what_it_is() {
        let megabyte = 1024 * 1024;
        let server = IntakeLimits::default();
        let bound = |limits, format| Bound { limits, format };
        for text in [FileImportFormat::Gedcom, FileImportFormat::Geneweb] {
            assert!(bound(server, text).check(350 * megabyte).is_ok());
            assert!(matches!(
                bound(server, text).check(350 * megabyte + 1),
                Err(OxidGeneError::TooLarge(_))
            ));
            assert!(
                bound(IntakeLimits::local(), text)
                    .check(351 * megabyte)
                    .is_err()
            );
        }
        // A gigabyte of archive, the old ceiling, and several more.
        let archive = FileImportFormat::Gedzip;
        assert!(bound(server, archive).check(8 * 1024 * megabyte).is_ok());
        assert!(matches!(
            bound(server, archive).check(11 * 1024 * megabyte),
            Err(OxidGeneError::TooLarge(_))
        ));
        assert!(
            bound(IntakeLimits::local(), archive)
                .check(u64::MAX)
                .is_ok()
        );
    }

    #[test]
    fn dropping_an_upload_removes_its_private_file() {
        let root = tempfile::tempdir().expect("a working directory");
        let work_dir = WorkDir::new(root.path());
        let upload = TemporaryUpload::new(&work_dir).expect("creates private upload");
        assert!(upload.path().starts_with(work_dir.staging()));
        std::fs::write(upload.path(), b"partial").expect("writes upload");
        let path = upload.path().to_path_buf();

        drop(upload);

        assert!(!path.exists());
    }
}
