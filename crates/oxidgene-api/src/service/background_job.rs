//! Durable import and export job execution shared by server and desktop workers.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use oxidgene_core::OxidGeneError;
use oxidgene_db::repo::{
    BackgroundJob, BackgroundJobKind, BackgroundJobRepo, NewBackgroundJob, TreeRepo,
};
use oxidgene_gedcom::export::GedzipFileWriter;
use sea_orm::{DatabaseConnection, DbBackend, TransactionTrait};
use serde::{Deserialize, Serialize};
use tracing::Instrument as _;
use uuid::Uuid;

use super::{gedcom, geneanet};
use crate::media::MediaStore;
use crate::media::store::{job_blob_key, job_input_blob_key};
use crate::profile::ProfileService;

pub const DEFAULT_LEASE_DURATION: Duration = Duration::from_secs(30);
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LiveJobProgress {
    pub phase: String,
    pub done: i64,
    pub total: i64,
}

#[derive(Debug)]
struct LiveJob {
    tree_id: Uuid,
    kind: String,
    progress: LiveJobProgress,
}

static LIVE_JOBS: OnceLock<Mutex<HashMap<Uuid, LiveJob>>> = OnceLock::new();

fn live_jobs() -> &'static Mutex<HashMap<Uuid, LiveJob>> {
    LIVE_JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn remove_live_job(job_id: Uuid) {
    if let Ok(mut jobs) = live_jobs().lock() {
        jobs.remove(&job_id);
    }
}

pub(crate) fn live_job_progress(
    tree_id: Uuid,
    job_id: Uuid,
    kind: BackgroundJobKind,
) -> Option<LiveJobProgress> {
    live_jobs().lock().ok().and_then(|jobs| {
        let job = jobs.get(&job_id)?;
        (job.tree_id == tree_id && job.kind == kind.as_str()).then(|| job.progress.clone())
    })
}

struct LiveJobGuard {
    job_id: Uuid,
}

impl LiveJobGuard {
    fn new(job: &BackgroundJob) -> Self {
        if let Ok(mut jobs) = live_jobs().lock() {
            jobs.insert(
                job.id,
                LiveJob {
                    tree_id: job.tree_id,
                    kind: job.kind.clone(),
                    progress: LiveJobProgress {
                        phase: job.phase.clone(),
                        done: job.done,
                        total: job.total,
                    },
                },
            );
        }
        Self { job_id: job.id }
    }
}

impl Drop for LiveJobGuard {
    fn drop(&mut self) {
        remove_live_job(self.job_id);
    }
}

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
        let _live_job =
            (self.db.get_database_backend() == DbBackend::Sqlite).then(|| LiveJobGuard::new(&job));

        let span = tracing::info_span!(
            "background_job.process",
            otel.kind = "consumer",
            job.kind = %job.kind,
            job.format = %job.format,
        );
        #[cfg(feature = "telemetry-context")]
        oxidgene_observability::set_parent_from_trace_context(
            &span,
            job.trace_parent.as_deref(),
            job.trace_state.as_deref(),
        );
        async {
            if let Err(error) = self.execute(&job).await {
                let code = match error {
                    OxidGeneError::Gedcom(_) | OxidGeneError::Validation(_) => "invalid_job_input",
                    _ => "job_failed",
                };
                tracing::error!(error.category = code, "background job failed");
                if BackgroundJobRepo::fail(&self.db, job.id, &self.worker_id, code).await? {
                    remove_live_job(job.id);
                    self.cleanup_import_inputs(&job).await;
                }
            }
            Ok::<(), OxidGeneError>(())
        }
        .instrument(span)
        .await?;
        Ok(true)
    }

    /// Run until the process is shut down.
    pub async fn run(self) {
        loop {
            match self.run_once().await {
                Ok(true) => {}
                Ok(false) => tokio::time::sleep(self.poll_interval).await,
                Err(_) => {
                    tracing::error!(
                        error.category = "worker_iteration_failed",
                        "background job worker iteration failed"
                    );
                    tokio::time::sleep(self.poll_interval).await;
                }
            }
        }
    }

    #[tracing::instrument(
        name = "background_job.execute",
        skip_all,
        fields(job.kind = %job.kind, job.format = %job.format)
    )]
    async fn execute(&self, job: &BackgroundJob) -> Result<(), OxidGeneError> {
        match job.kind.as_str() {
            "import" => self.execute_import(job).await,
            "export" => self.execute_export(job).await,
            _ => Err(OxidGeneError::Validation("unknown job kind".into())),
        }
    }

    #[tracing::instrument(
        name = "import.job",
        skip_all,
        fields(import.format = %job.format)
    )]
    async fn execute_import(&self, job: &BackgroundJob) -> Result<(), OxidGeneError> {
        if job.format == "geneanet" {
            return self.execute_geneanet_import(job).await;
        }
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
                    tracing::info_span!("import.parse", import.format = "gedcom")
                        .in_scope(|| oxidgene_gedcom::import::import_gedcom(&source, job.tree_id))
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
                    tracing::info_span!("import.parse", import.format = "geneweb")
                        .in_scope(|| {
                            oxidgene_gedcom::geneweb::import_geneweb(&source, &origin, job.tree_id)
                        })
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
        let period = self.progress_period();
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

    async fn execute_geneanet_import(&self, job: &BackgroundJob) -> Result<(), OxidGeneError> {
        let source_key = job
            .source_key
            .as_deref()
            .ok_or_else(|| OxidGeneError::Validation("import job has no source".into()))?;
        let payload = geneanet_payload(job)?;
        if job.phase == "projections" {
            let summary = geneanet_summary(job)?;
            self.finish_geneanet_import(job, summary).await?;
            return Ok(());
        }

        let scratch = ScratchDirectory::new(job.id).await?;
        self.progress(job.id, "staging", 0, 0).await?;
        let source = scratch.path().join("source.gw");
        self.media.get_to_file(source_key, &source).await?;

        let archive_root = scratch.path().join("archives");
        tokio::fs::create_dir_all(&archive_root).await?;
        let mut archive_paths = Vec::with_capacity(payload.archives.len());
        for (index, input) in payload.archives.iter().enumerate() {
            let path = archive_root.join(format!("{index}-{}", input.file_name));
            self.media.get_to_file(&input.key, &path).await?;
            archive_paths.push(path.to_string_lossy().into_owned());
        }

        let fetched_root = scratch.path().join("fetched");
        tokio::fs::create_dir_all(&fetched_root).await?;
        let mut fetched = HashMap::with_capacity(payload.fetched.len());
        for (index, input) in payload.fetched.iter().enumerate() {
            let path = fetched_root.join(index.to_string());
            self.media.get_to_file(&input.key, &path).await?;
            fetched.insert(input.url.clone(), path.to_string_lossy().into_owned());
        }

        let gw = tokio::fs::read(source).await?;
        let origin_file = safe_origin_file(job.original_filename.as_deref());
        let progress = Arc::new(geneanet::ImportProgress::default());
        let import = geneanet::import(
            &self.db,
            &*self.media,
            job.tree_id,
            &gw,
            &origin_file,
            &payload.collection,
            &payload.deposit_sizes,
            &archive_paths,
            &fetched,
            payload.media_fidelity,
            &progress,
        );
        tokio::pin!(import);
        let period = self.progress_period();
        let mut heartbeat = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
        let summary = loop {
            tokio::select! {
                result = &mut import => break result?,
                _ = heartbeat.tick() => {
                    let (phase, done, total) = progress.read();
                    self.progress(job.id, geneanet_phase(phase), as_i64(done), as_i64(total)).await?;
                }
            }
        };

        let result_json = serde_json::to_string(&summary)
            .map_err(|error| OxidGeneError::Internal(error.to_string()))?;
        if !BackgroundJobRepo::checkpoint_import_persisted(
            &self.db,
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
        self.finish_geneanet_import(job, summary).await
    }

    async fn finish_geneanet_import(
        &self,
        job: &BackgroundJob,
        summary: geneanet::GeneanetImportSummary,
    ) -> Result<(), OxidGeneError> {
        self.profiles
            .rebuild_tree_full_transactional(&self.db, job.tree_id)
            .instrument(tracing::info_span!("import.projections"))
            .await?;
        let result = serde_json::to_string(&summary)
            .map_err(|error| OxidGeneError::Internal(error.to_string()))?;
        if !BackgroundJobRepo::complete(&self.db, job.id, &self.worker_id, None, Some(result))
            .await?
        {
            return Err(OxidGeneError::Internal("background job lease lost".into()));
        }
        remove_live_job(job.id);
        self.cleanup_import_inputs(job).await;
        Ok(())
    }

    async fn cleanup_import_inputs(&self, job: &BackgroundJob) {
        let mut keys = job
            .source_key
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        let payload = (job.format == "geneanet")
            .then(|| geneanet_payload(job).ok())
            .flatten();
        if let Some(payload) = &payload {
            keys.extend(payload.archives.iter().map(|input| input.key.as_str()));
            keys.extend(payload.fetched.iter().map(|input| input.key.as_str()));
        }
        for key in keys {
            if let Err(error) = self.media.delete(key).await {
                tracing::warn!(job_id = %job.id, %key, %error, "could not delete job input");
            }
        }
    }

    async fn finish_import(
        &self,
        job: &BackgroundJob,
        source_key: &str,
        summary: gedcom::ImportSummary,
    ) -> Result<(), OxidGeneError> {
        self.profiles
            .rebuild_tree_full_transactional(&self.db, job.tree_id)
            .instrument(tracing::info_span!("import.projections"))
            .await?;
        let result = serde_json::to_string(&summary)
            .map_err(|error| OxidGeneError::Internal(error.to_string()))?;
        if !BackgroundJobRepo::complete(&self.db, job.id, &self.worker_id, None, Some(result))
            .await?
        {
            return Err(OxidGeneError::Internal("background job lease lost".into()));
        }
        remove_live_job(job.id);
        self.media.delete(source_key).await?;
        Ok(())
    }

    #[tracing::instrument(
        name = "export.job",
        skip_all,
        fields(export.format = %job.format)
    )]
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
        let media_span = tracing::info_span!(
            "export.media",
            export.format = "gedzip",
            export.media.count = total,
        );
        let mut staged_media = Vec::with_capacity(data.media_files.len());
        async {
            for (index, (key, archive_path, mime_type)) in data.media_files.iter().enumerate() {
                let local_path = media_root.join(index.to_string());
                match self.media.get_to_file(key, &local_path).await {
                    Ok(()) => {
                        staged_media.push((archive_path.clone(), mime_type.clone(), local_path));
                    }
                    Err(error) => tracing::warn!(
                        job_id = %job.id,
                        %error,
                        "media absent from the store; not packed"
                    ),
                }
                self.progress(job.id, "media", as_i64(index + 1), total)
                    .await?;
            }
            Ok::<(), OxidGeneError>(())
        }
        .instrument(media_span)
        .await?;

        let artifact_path = scratch.path().join("artifact.gdz");
        let gedcom = data.gedcom;
        let archive_path = artifact_path.clone();
        let archive_task = tokio::task::spawn_blocking(move || {
            let mut writer =
                GedzipFileWriter::create(&archive_path, &gedcom).map_err(OxidGeneError::Gedcom)?;
            for (entry_path, mime_type, local_path) in staged_media {
                let bytes = std::fs::read(local_path)?;
                writer
                    .add_media_file(&entry_path, &mime_type, &bytes)
                    .map_err(OxidGeneError::Gedcom)?;
            }
            writer.finish().map_err(OxidGeneError::Gedcom)
        });
        self.with_heartbeat(job.id, "packaging", async {
            archive_task
                .await
                .map_err(|error| OxidGeneError::Internal(error.to_string()))?
        })
        .instrument(tracing::info_span!(
            "export.package",
            export.format = "gedzip"
        ))
        .await?;

        let artifact_key = job_blob_key(job.id, "artifact", "gdz")?;
        self.with_heartbeat(job.id, "publishing", async {
            self.media.put_file(&artifact_key, &artifact_path).await
        })
        .instrument(tracing::info_span!(
            "export.publish",
            export.format = "gedzip"
        ))
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
        remove_live_job(job.id);
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
        let period = self.progress_period();
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
        if self.db.get_database_backend() == DbBackend::Sqlite {
            if let Ok(mut jobs) = live_jobs().lock()
                && let Some(job) = jobs.get_mut(&job_id)
            {
                job.progress = LiveJobProgress {
                    phase: phase.to_string(),
                    done,
                    total,
                };
            }
            return Ok(());
        }

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

    fn progress_period(&self) -> Duration {
        progress_period(self.poll_interval, self.lease_duration)
    }
}

fn progress_period(poll_interval: Duration, lease_duration: Duration) -> Duration {
    poll_interval.min(lease_duration / 3)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectOptions, Database};

    #[test]
    fn sqlite_progress_is_published_independently_of_its_long_lease() {
        assert_eq!(
            progress_period(DEFAULT_POLL_INTERVAL, Duration::from_secs(24 * 60 * 60)),
            Duration::from_secs(1)
        );
    }

    #[test]
    fn progress_renews_a_short_lease_before_it_expires() {
        assert_eq!(
            progress_period(Duration::from_secs(10), Duration::from_secs(6)),
            Duration::from_secs(2)
        );
    }

    #[tokio::test]
    async fn sqlite_progress_does_not_wait_for_the_only_pool_connection() {
        let mut options = ConnectOptions::new("sqlite::memory:");
        options.max_connections(1);
        let db = Database::connect(options).await.expect("connects");
        let profiles = Arc::new(ProfileService::new(db.clone()));
        let media_root = tempfile::tempdir().expect("creates media root");
        let media: Arc<dyn MediaStore> =
            Arc::new(crate::media::store::FsStore::new(media_root.path()));
        let worker = BackgroundJobWorker::new(db.clone(), profiles, media, "test");
        let job_id = Uuid::now_v7();
        let tree_id = Uuid::now_v7();
        let job = BackgroundJob {
            id: job_id,
            tree_id,
            active_tree_id: Some(tree_id),
            kind: BackgroundJobKind::Import.as_str().to_string(),
            format: "geneanet".to_string(),
            status: "running".to_string(),
            phase: "queued".to_string(),
            source_key: None,
            artifact_key: None,
            payload_json: None,
            original_filename: None,
            merge_occupations: false,
            merge_names: false,
            done: 0,
            total: 0,
            attempt: 1,
            lease_owner: Some("test".to_string()),
            lease_until: None,
            cancel_requested: false,
            result_json: None,
            error_code: None,
            trace_parent: None,
            trace_state: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            started_at: Some(chrono::Utc::now()),
            finished_at: None,
        };
        let _live_job = LiveJobGuard::new(&job);
        let _transaction = db.begin().await.expect("holds only connection");

        tokio::time::timeout(
            Duration::from_millis(100),
            worker.progress(job_id, "people", 100, 250),
        )
        .await
        .expect("progress does not wait for the pool")
        .expect("progress succeeds");

        assert_eq!(
            live_job_progress(tree_id, job_id, BackgroundJobKind::Import),
            Some(LiveJobProgress {
                phase: "people".to_string(),
                done: 100,
                total: 250,
            })
        );
    }
}

struct ScratchDirectory(tempfile::TempDir);

impl ScratchDirectory {
    async fn new(job_id: Uuid) -> Result<Self, OxidGeneError> {
        tempfile::Builder::new()
            .prefix(&format!("oxidgene-job-{job_id}-"))
            .tempdir()
            .map(Self)
            .map_err(OxidGeneError::Io)
    }

    fn path(&self) -> &Path {
        self.0.path()
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

const fn geneanet_phase(phase: geneanet::ImportPhase) -> &'static str {
    match phase {
        geneanet::ImportPhase::Starting => "starting",
        geneanet::ImportPhase::People => "people",
        geneanet::ImportPhase::Matching => "matching",
        geneanet::ImportPhase::Media => "media",
        geneanet::ImportPhase::Finishing => "projections",
    }
}

fn import_summary(job: &BackgroundJob) -> Result<gedcom::ImportSummary, OxidGeneError> {
    let result = job
        .result_json
        .as_deref()
        .ok_or_else(|| OxidGeneError::Internal("persisted import has no result".into()))?;
    serde_json::from_str(result).map_err(|error| OxidGeneError::Internal(error.to_string()))
}

fn geneanet_payload(job: &BackgroundJob) -> Result<GeneanetJobPayload, OxidGeneError> {
    let payload = job
        .payload_json
        .as_deref()
        .ok_or_else(|| OxidGeneError::Validation("Geneanet import job has no payload".into()))?;
    serde_json::from_str(payload).map_err(|error| OxidGeneError::Validation(error.to_string()))
}

fn geneanet_summary(job: &BackgroundJob) -> Result<geneanet::GeneanetImportSummary, OxidGeneError> {
    let result = job
        .result_json
        .as_deref()
        .ok_or_else(|| OxidGeneError::Internal("persisted import has no result".into()))?;
    serde_json::from_str(result).map_err(|error| OxidGeneError::Internal(error.to_string()))
}

#[derive(Debug, Deserialize, Serialize)]
struct GeneanetJobPayload {
    collection: String,
    deposit_sizes: HashMap<i64, u64>,
    archives: Vec<GeneanetArchiveInput>,
    fetched: Vec<GeneanetFetchedInput>,
    /// Absent in a job staged before the wizard offered the choice; those were
    /// all archive-and-original runs, so they must not decode as the new
    /// default.
    #[serde(default = "originals_fidelity")]
    media_fidelity: geneanet::MediaFidelity,
}

/// What a payload with no `media_fidelity` meant when it was written.
const fn originals_fidelity() -> geneanet::MediaFidelity {
    geneanet::MediaFidelity::Originals
}

#[derive(Debug, Deserialize, Serialize)]
struct GeneanetArchiveInput {
    key: String,
    file_name: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct GeneanetFetchedInput {
    url: String,
    key: String,
}

#[allow(clippy::too_many_arguments)]
pub async fn stage_geneanet_import(
    db: &DatabaseConnection,
    media: &dyn MediaStore,
    tree_id: Uuid,
    gw: &[u8],
    file_name: String,
    collection: String,
    deposit_sizes: HashMap<i64, u64>,
    archive_paths: &[String],
    fetched_paths: &HashMap<String, String>,
    media_fidelity: geneanet::MediaFidelity,
) -> Result<Uuid, OxidGeneError> {
    TreeRepo::get(db, tree_id).await?;
    let job_id = Uuid::now_v7();
    let scratch = ScratchDirectory::new(job_id).await?;
    let source_path = scratch.path().join("source.gw");
    tokio::fs::write(&source_path, gw).await?;

    let source_key = job_blob_key(job_id, "source", "gw")?;
    let mut staged_keys = Vec::with_capacity(1 + archive_paths.len() + fetched_paths.len());
    let staging = async {
        media.put_file(&source_key, &source_path).await?;
        staged_keys.push(source_key.clone());

        let mut next_input = 0usize;
        // Nothing is staged for a run that will not open them — a data archive
        // is gigabytes, and copying it into job storage to be ignored is the
        // most expensive way to do nothing.
        let archive_paths: &[String] = if media_fidelity.uses_archives() {
            archive_paths
        } else {
            &[]
        };
        let mut archives = Vec::with_capacity(archive_paths.len());
        for path in archive_paths {
            let key = job_input_blob_key(job_id, next_input);
            next_input += 1;
            media.put_file(&key, Path::new(path)).await?;
            staged_keys.push(key.clone());
            archives.push(GeneanetArchiveInput {
                key,
                file_name: safe_origin_file(Some(path)),
            });
        }

        let mut fetched_entries: Vec<_> = fetched_paths.iter().collect();
        fetched_entries.sort_by_key(|(url, _)| *url);
        let mut fetched = Vec::with_capacity(fetched_entries.len());
        for (url, path) in fetched_entries {
            let key = job_input_blob_key(job_id, next_input);
            next_input += 1;
            media.put_file(&key, Path::new(path)).await?;
            staged_keys.push(key.clone());
            fetched.push(GeneanetFetchedInput {
                url: url.clone(),
                key,
            });
        }

        let payload_json = serde_json::to_string(&GeneanetJobPayload {
            collection,
            deposit_sizes,
            archives,
            fetched,
            media_fidelity,
        })
        .map_err(|error| OxidGeneError::Internal(error.to_string()))?;
        BackgroundJobRepo::create(
            db,
            NewBackgroundJob {
                id: job_id,
                tree_id,
                kind: BackgroundJobKind::Import,
                format: "geneanet".to_string(),
                source_key: Some(source_key),
                payload_json: Some(payload_json),
                original_filename: Some(file_name),
                merge_occupations: false,
                merge_names: false,
            },
        )
        .await?;
        Ok::<(), OxidGeneError>(())
    }
    .await;

    crate::service::session_media::remove_owned(fetched_paths.values().map(String::as_str));
    if let Err(error) = staging {
        for key in staged_keys {
            let _ = media.delete(&key).await;
        }
        return Err(error);
    }
    Ok(job_id)
}

#[derive(Serialize)]
struct ExportJobResult {
    warnings: Vec<String>,
}
