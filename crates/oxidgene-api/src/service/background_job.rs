//! Durable import and export job execution shared by server and desktop workers.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use oxidgene_core::OxidGeneError;
use oxidgene_db::repo::{BackgroundJob, BackgroundJobRepo};
use oxidgene_gedcom::export::GedzipFileWriter;
use sea_orm::{DatabaseConnection, DbBackend, TransactionTrait};
use serde::Serialize;
use uuid::Uuid;

use super::gedcom;
use crate::media::MediaStore;
use crate::media::store::job_blob_key;
use crate::profile::ProfileService;

pub const DEFAULT_LEASE_DURATION: Duration = Duration::from_secs(30);
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone)]
pub struct BackgroundJobWorker {
    db: DatabaseConnection,
    profiles: Arc<ProfileService>,
    media: Arc<dyn MediaStore>,
    worker_id: String,
    lease_duration: Duration,
    poll_interval: Duration,
}

impl std::fmt::Debug for BackgroundJobWorker {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BackgroundJobWorker")
            .field("worker_id", &self.worker_id)
            .field("lease_duration", &self.lease_duration)
            .field("poll_interval", &self.poll_interval)
            .finish_non_exhaustive()
    }
}

impl BackgroundJobWorker {
    #[must_use]
    pub fn new(
        db: DatabaseConnection,
        profiles: Arc<ProfileService>,
        media: Arc<dyn MediaStore>,
        worker_id: impl Into<String>,
    ) -> Self {
        let lease_duration = if db.get_database_backend() == DbBackend::Sqlite {
            Duration::from_secs(24 * 60 * 60)
        } else {
            DEFAULT_LEASE_DURATION
        };
        Self {
            db,
            profiles,
            media,
            worker_id: worker_id.into(),
            lease_duration,
            poll_interval: DEFAULT_POLL_INTERVAL,
        }
    }

    /// Claim and execute at most one job. Returns whether work was claimed.
    pub async fn run_once(&self) -> Result<bool, OxidGeneError> {
        let Some(job) = BackgroundJobRepo::claim_next(
            &self.db,
            &self.worker_id,
            chrono::Duration::from_std(self.lease_duration)
                .map_err(|error| OxidGeneError::Internal(error.to_string()))?,
        )
        .await?
        else {
            return Ok(false);
        };

        if let Err(error) = self.execute(&job).await {
            let code = match error {
                OxidGeneError::Gedcom(_) | OxidGeneError::Validation(_) => "invalid_job_input",
                _ => "job_failed",
            };
            tracing::error!(job_id = %job.id, %error, "background job failed");
            let _ = BackgroundJobRepo::fail(&self.db, job.id, &self.worker_id, code).await?;
        }
        Ok(true)
    }

    /// Run until the process is shut down.
    pub async fn run(self) {
        loop {
            match self.run_once().await {
                Ok(true) => {}
                Ok(false) => tokio::time::sleep(self.poll_interval).await,
                Err(error) => {
                    tracing::error!(%error, "background job worker iteration failed");
                    tokio::time::sleep(self.poll_interval).await;
                }
            }
        }
    }

    async fn execute(&self, job: &BackgroundJob) -> Result<(), OxidGeneError> {
        match job.kind.as_str() {
            "import" => self.execute_import(job).await,
            "export" => self.execute_export(job).await,
            _ => Err(OxidGeneError::Validation("unknown job kind".into())),
        }
    }

    async fn execute_import(&self, job: &BackgroundJob) -> Result<(), OxidGeneError> {
        let source_key = job
            .source_key
            .as_deref()
            .ok_or_else(|| OxidGeneError::Validation("import job has no source".into()))?;
        if job.phase == "projections" {
            let summary = import_summary(job)?;
            self.finish_import(job, source_key, summary).await?;
            return Ok(());
        }
        let scratch = ScratchDirectory::new(job.id).await?;
        let source = scratch.path().join(format!("source.{}", job.format));
        self.progress(job.id, "staging", 0, 0).await?;
        self.media.get_to_file(source_key, &source).await?;

        let progress = Arc::new(gedcom::FileImportProgress::default());
        let import = async {
            let parsed = match job.format.as_str() {
                "gedcom" => {
                    progress.enter(gedcom::FileImportPhase::Parsing);
                    let source = tokio::fs::read_to_string(&source).await?;
                    oxidgene_gedcom::import::import_gedcom(&source, job.tree_id)
                        .map_err(OxidGeneError::Gedcom)?
                }
                "gedzip" => {
                    gedcom::prepare_gedzip_file(&*self.media, job.tree_id, &source, &progress)
                        .await?
                }
                "geneweb" => {
                    progress.enter(gedcom::FileImportPhase::Parsing);
                    let source = tokio::fs::read(&source).await?;
                    let origin = safe_origin_file(job.original_filename.as_deref());
                    oxidgene_gedcom::geneweb::import_geneweb(&source, &origin, job.tree_id)
                        .map_err(OxidGeneError::Gedcom)?
                }
                _ => return Err(OxidGeneError::Validation("unknown import format".into())),
            };
            progress.enter(gedcom::FileImportPhase::Database);
            let transaction = self
                .db
                .begin()
                .await
                .map_err(|error| OxidGeneError::Database(error.to_string()))?;
            let summary = gedcom::persist_import_result_in(&transaction, parsed).await?;
            let result_json = serde_json::to_string(&summary)
                .map_err(|error| OxidGeneError::Internal(error.to_string()))?;
            if !BackgroundJobRepo::checkpoint_import_persisted(
                &transaction,
                job.id,
                &self.worker_id,
                result_json,
                chrono::Duration::from_std(self.lease_duration)
                    .map_err(|error| OxidGeneError::Internal(error.to_string()))?,
            )
            .await?
            {
                return Err(OxidGeneError::Internal("background job lease lost".into()));
            }
            transaction
                .commit()
                .await
                .map_err(|error| OxidGeneError::Database(error.to_string()))?;
            Ok(summary)
        };
        tokio::pin!(import);
        let period = self.lease_duration / 3;
        let mut heartbeat = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
        let summary = loop {
            tokio::select! {
                result = &mut import => break result?,
                _ = heartbeat.tick() => {
                    let (phase, done, total, _, _) = progress.read();
                    self.progress(job.id, import_phase(phase), as_i64(done), as_i64(total)).await?;
                }
            }
        };

        self.finish_import(job, source_key, summary).await?;
        Ok(())
    }

    async fn finish_import(
        &self,
        job: &BackgroundJob,
        source_key: &str,
        summary: gedcom::ImportSummary,
    ) -> Result<(), OxidGeneError> {
        self.profiles
            .rebuild_tree_full(&self.db, job.tree_id)
            .await?;
        let result = serde_json::to_string(&summary)
            .map_err(|error| OxidGeneError::Internal(error.to_string()))?;
        if !BackgroundJobRepo::complete(&self.db, job.id, &self.worker_id, None, Some(result))
            .await?
        {
            return Err(OxidGeneError::Internal("background job lease lost".into()));
        }
        self.media.delete(source_key).await?;
        Ok(())
    }

    async fn execute_export(&self, job: &BackgroundJob) -> Result<(), OxidGeneError> {
        if job.format != "gedzip" {
            return Err(OxidGeneError::Validation("unknown export format".into()));
        }
        let scratch = ScratchDirectory::new(job.id).await?;
        self.progress(job.id, "loading", 0, 0).await?;
        let data = self
            .with_heartbeat(
                job.id,
                "loading",
                gedcom::load_and_export(
                    &self.db,
                    job.tree_id,
                    job.merge_occupations,
                    job.merge_names,
                    true,
                ),
            )
            .await?;

        let media_root = scratch.path().join("media");
        tokio::fs::create_dir_all(&media_root).await?;
        let total = as_i64(data.media_files.len());
        let mut staged_media = Vec::with_capacity(data.media_files.len());
        for (index, (key, archive_path)) in data.media_files.iter().enumerate() {
            let local_path = media_root.join(index.to_string());
            match self.media.get_to_file(key, &local_path).await {
                Ok(()) => staged_media.push((archive_path.clone(), local_path)),
                Err(error) => tracing::warn!(
                    job_id = %job.id,
                    %error,
                    "media absent from the store; not packed"
                ),
            }
            self.progress(job.id, "media", as_i64(index + 1), total)
                .await?;
        }

        let artifact_path = scratch.path().join("artifact.gdz");
        let gedcom = data.gedcom;
        let archive_path = artifact_path.clone();
        let archive_task = tokio::task::spawn_blocking(move || {
            let mut writer =
                GedzipFileWriter::create(&archive_path, &gedcom).map_err(OxidGeneError::Gedcom)?;
            for (entry_path, local_path) in staged_media {
                let bytes = std::fs::read(local_path)?;
                writer
                    .add_media_file(&entry_path, &bytes)
                    .map_err(OxidGeneError::Gedcom)?;
            }
            writer.finish().map_err(OxidGeneError::Gedcom)
        });
        self.with_heartbeat(job.id, "packaging", async {
            archive_task
                .await
                .map_err(|error| OxidGeneError::Internal(error.to_string()))?
        })
        .await?;

        let artifact_key = job_blob_key(job.id, "artifact", "gdz")?;
        self.with_heartbeat(job.id, "publishing", async {
            self.media.put_file(&artifact_key, &artifact_path).await
        })
        .await?;
        let result = serde_json::to_string(&ExportJobResult {
            warnings: data.warnings,
        })
        .map_err(|error| OxidGeneError::Internal(error.to_string()))?;
        if !BackgroundJobRepo::complete(
            &self.db,
            job.id,
            &self.worker_id,
            Some(artifact_key),
            Some(result),
        )
        .await?
        {
            return Err(OxidGeneError::Internal("background job lease lost".into()));
        }
        Ok(())
    }

    async fn with_heartbeat<T, F>(
        &self,
        job_id: Uuid,
        phase: &str,
        future: F,
    ) -> Result<T, OxidGeneError>
    where
        F: std::future::Future<Output = Result<T, OxidGeneError>>,
    {
        tokio::pin!(future);
        let period = self.lease_duration / 3;
        let mut heartbeat = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
        loop {
            tokio::select! {
                result = &mut future => return result,
                _ = heartbeat.tick() => self.progress(job_id, phase, 0, 0).await?,
            }
        }
    }

    async fn progress(
        &self,
        job_id: Uuid,
        phase: &str,
        done: i64,
        total: i64,
    ) -> Result<(), OxidGeneError> {
        let renewed = BackgroundJobRepo::progress(
            &self.db,
            job_id,
            &self.worker_id,
            phase,
            done,
            total,
            chrono::Duration::from_std(self.lease_duration)
                .map_err(|error| OxidGeneError::Internal(error.to_string()))?,
        )
        .await?;
        if renewed {
            Ok(())
        } else {
            Err(OxidGeneError::Internal("background job lease lost".into()))
        }
    }
}

struct ScratchDirectory(PathBuf);

impl ScratchDirectory {
    async fn new(job_id: Uuid) -> Result<Self, OxidGeneError> {
        let path = std::env::temp_dir()
            .join("oxidgene-jobs")
            .join(job_id.to_string());
        if tokio::fs::try_exists(&path).await? {
            tokio::fs::remove_dir_all(&path).await?;
        }
        tokio::fs::create_dir_all(&path).await?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ScratchDirectory {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(%error, "could not remove background job scratch directory");
        }
    }
}

fn safe_origin_file(filename: Option<&str>) -> String {
    filename
        .and_then(|name| Path::new(name).file_name())
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("import.gw")
        .to_string()
}

const fn import_phase(phase: gedcom::FileImportPhase) -> &'static str {
    match phase {
        gedcom::FileImportPhase::Starting => "starting",
        gedcom::FileImportPhase::Parsing => "parsing",
        gedcom::FileImportPhase::Media => "media",
        gedcom::FileImportPhase::Database => "database",
        gedcom::FileImportPhase::Projections => "projections",
        gedcom::FileImportPhase::Completed => "completed",
        gedcom::FileImportPhase::Failed => "failed",
    }
}

fn as_i64(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn import_summary(job: &BackgroundJob) -> Result<gedcom::ImportSummary, OxidGeneError> {
    let result = job
        .result_json
        .as_deref()
        .ok_or_else(|| OxidGeneError::Internal("persisted import has no result".into()))?;
    serde_json::from_str(result).map_err(|error| OxidGeneError::Internal(error.to_string()))
}

#[derive(Serialize)]
struct ExportJobResult {
    warnings: Vec<String>,
}
