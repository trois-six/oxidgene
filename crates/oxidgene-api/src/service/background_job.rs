//! Durable import and export job execution shared by server and desktop workers.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use oxidgene_core::OxidGeneError;
use oxidgene_db::repo::{
    BackgroundJob, BackgroundJobKind, BackgroundJobRepo, BackgroundJobStatus, NewBackgroundJob,
    TreeRepo, db_err,
};
use oxidgene_gedcom::export::GedzipFileWriter;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, TransactionTrait};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tracing::Instrument as _;
use uuid::Uuid;

use super::{gedcom, geneanet, history};
use crate::error_contract::error_kind;
use crate::media::MediaStore;
use crate::media::store::{job_blob_key, job_input_blob_key};
use crate::profile::ProfileService;

pub const DEFAULT_LEASE_DURATION: Duration = Duration::from_secs(30);
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// How long a completed export's artifact waits for its download. A
/// download removes it at once; this bounds the ones never downloaded — the
/// UI downloads as soon as the job completes, so only an export whose window
/// was closed meanwhile waits this long.
pub const EXPORT_ARTIFACT_TTL: Duration = Duration::from_secs(60 * 60);

/// How long an ended job's row stays, with the result its status reports.
pub const ENDED_JOB_RETENTION: Duration = Duration::from_secs(24 * 60 * 60);

/// How old an entry under `jobs/` that no job needs must be before it is
/// swept: a job's inputs are stored before its row is created, and staging a
/// Geneanet import's archives can take a while.
const ORPHAN_GRACE: Duration = Duration::from_secs(24 * 60 * 60);

/// How often a worker expires artifacts, prunes ended jobs and sweeps
/// orphaned job objects; it also does so when it starts.
const MAINTENANCE_PERIOD: Duration = Duration::from_secs(60 * 60);

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

pub(crate) struct LiveJobGuard {
    job_id: Uuid,
}

impl LiveJobGuard {
    /// Registers a running job of `kind` in `tree_id` at its start.
    #[cfg(test)]
    pub(crate) fn for_test(job_id: Uuid, tree_id: Uuid, kind: BackgroundJobKind) -> Self {
        if let Ok(mut jobs) = live_jobs().lock() {
            jobs.insert(
                job_id,
                LiveJob {
                    tree_id,
                    kind: kind.as_str().to_string(),
                    progress: LiveJobProgress {
                        phase: "staging".to_string(),
                        done: 0,
                        total: 0,
                    },
                },
            );
        }
        Self { job_id }
    }

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
                // Console logs carry no span context, so the job's bounded
                // dimensions ride on the event itself.
                tracing::error!(
                    error.category = code,
                    error.kind = error_kind(&error),
                    job.kind = %job.kind,
                    job.format = %job.format,
                    "background job failed"
                );
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
        let mut next_maintenance = tokio::time::Instant::now();
        loop {
            if tokio::time::Instant::now() >= next_maintenance {
                self.maintain(chrono::Utc::now()).await;
                next_maintenance = tokio::time::Instant::now() + MAINTENANCE_PERIOD;
            }
            match self.run_once().await {
                Ok(true) => {}
                Ok(false) => tokio::time::sleep(self.poll_interval).await,
                Err(error) => {
                    tracing::error!(
                        error.category = "worker_iteration_failed",
                        error.kind = error_kind(&error),
                        "background job worker iteration failed"
                    );
                    tokio::time::sleep(self.poll_interval).await;
                }
            }
        }
    }

    /// Bound what ended jobs leave behind, as of `now`: delete the
    /// artifacts of exports completed more than [`EXPORT_ARTIFACT_TTL`] ago,
    /// the rows (and any objects) of jobs ended more than
    /// [`ENDED_JOB_RETENTION`] ago, and the objects under `jobs/` that no job
    /// needs any more, once older than a day. Failures are logged; the next
    /// pass retries.
    pub async fn maintain(&self, now: chrono::DateTime<chrono::Utc>) {
        let media = &*self.media;
        log_maintenance_failure(
            "export_artifact_expiry",
            expire_artifacts(&self.db, media, now).await,
        );
        log_maintenance_failure(
            "ended_job_pruning",
            prune_ended_jobs(&self.db, media, now).await,
        );
        log_maintenance_failure(
            "job_object_sweep",
            sweep_orphaned_job_objects(&self.db, media, now).await,
        );
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
        let summary = self
            .with_progress(job.id, self.import_file(job, &source, &progress), || {
                let (phase, done, total, _, _) = progress.read();
                (import_phase(phase), done, total)
            })
            .await?;
        self.finish_import(job, source_key, summary).await
    }

    /// Reads the staged `source` and writes its rows in one transaction,
    /// with the checkpoint a retry resumes from.
    async fn import_file(
        &self,
        job: &BackgroundJob,
        source: &Path,
        progress: &gedcom::FileImportProgress,
    ) -> Result<gedcom::ImportSummary, OxidGeneError> {
        let parsed = self.parse_import(job, source, progress).await?;
        progress.enter(gedcom::FileImportPhase::Database);
        let transaction = self.db.begin().await.map_err(db_err)?;
        let summary = gedcom::persist_import_result_in(&transaction, parsed).await?;
        self.checkpoint_import(&transaction, job.id, &summary)
            .await?;
        transaction.commit().await.map_err(db_err)?;
        Ok(summary)
    }

    /// Reads the staged `source` of a GEDCOM, GEDZIP or GeneWeb import.
    async fn parse_import(
        &self,
        job: &BackgroundJob,
        source: &Path,
        progress: &gedcom::FileImportProgress,
    ) -> Result<oxidgene_gedcom::ImportResult, OxidGeneError> {
        match job.format.as_str() {
            "gedcom" => {
                progress.enter(gedcom::FileImportPhase::Parsing);
                let source = tokio::fs::read(source).await?;
                tracing::info_span!("import.parse", import.format = "gedcom")
                    .in_scope(|| oxidgene_gedcom::import::import_gedcom_bytes(&source, job.tree_id))
                    .map_err(OxidGeneError::Gedcom)
            }
            "gedzip" => {
                gedcom::prepare_gedzip_file(&*self.media, job.tree_id, source, progress).await
            }
            "geneweb" => {
                progress.enter(gedcom::FileImportPhase::Parsing);
                let source = tokio::fs::read(source).await?;
                let origin = safe_origin_file(job.original_filename.as_deref());
                tracing::info_span!("import.parse", import.format = "geneweb")
                    .in_scope(|| {
                        oxidgene_gedcom::geneweb::import_geneweb(&source, &origin, job.tree_id)
                    })
                    .map_err(OxidGeneError::Gedcom)
            }
            _ => Err(OxidGeneError::Validation("unknown import format".into())),
        }
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

        let (archive_paths, fetched) = self.stage_geneanet_inputs(scratch.path(), &payload).await?;

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
        let summary = self
            .with_progress(job.id, import, || {
                let (phase, done, total) = progress.read();
                (geneanet_phase(phase), done, total)
            })
            .await?;

        self.checkpoint_import(&self.db, job.id, &summary).await?;
        self.finish_geneanet_import(job, summary).await
    }

    /// Downloads a Geneanet import's archives and fetched pages next to its
    /// source, returning the archives' paths and each fetched URL's path.
    async fn stage_geneanet_inputs(
        &self,
        scratch: &Path,
        payload: &GeneanetJobPayload,
    ) -> Result<(Vec<String>, HashMap<String, String>), OxidGeneError> {
        let archive_root = scratch.join("archives");
        tokio::fs::create_dir_all(&archive_root).await?;
        let mut archive_paths = Vec::with_capacity(payload.archives.len());
        for (index, input) in payload.archives.iter().enumerate() {
            let path = archive_root.join(format!("{index}-{}", input.file_name));
            self.media.get_to_file(&input.key, &path).await?;
            archive_paths.push(path.to_string_lossy().into_owned());
        }

        let fetched_root = scratch.join("fetched");
        tokio::fs::create_dir_all(&fetched_root).await?;
        let mut fetched = HashMap::with_capacity(payload.fetched.len());
        for (index, input) in payload.fetched.iter().enumerate() {
            let path = fetched_root.join(index.to_string());
            self.media.get_to_file(&input.key, &path).await?;
            fetched.insert(input.url.clone(), path.to_string_lossy().into_owned());
        }
        Ok((archive_paths, fetched))
    }

    /// Records on `conn` that an import's rows are persisted, with its
    /// summary, so that a retry resumes at the projections.
    async fn checkpoint_import(
        &self,
        conn: &impl ConnectionTrait,
        job_id: Uuid,
        summary: &impl Serialize,
    ) -> Result<(), OxidGeneError> {
        let result_json = serde_json::to_string(summary)
            .map_err(|error| OxidGeneError::Internal(error.to_string()))?;
        if !BackgroundJobRepo::checkpoint_import_persisted(
            conn,
            job_id,
            &self.worker_id,
            result_json,
            chrono::Duration::from_std(self.lease_duration)
                .map_err(|error| OxidGeneError::Internal(error.to_string()))?,
        )
        .await?
        {
            return Err(OxidGeneError::Internal("background job lease lost".into()));
        }
        Ok(())
    }

    async fn finish_geneanet_import(
        &self,
        job: &BackgroundJob,
        summary: geneanet::GeneanetImportSummary,
    ) -> Result<(), OxidGeneError> {
        self.progress(job.id, "projections", 0, 0).await?;
        self.complete_import(job, summary.persons_count, &summary)
            .await?;
        self.cleanup_import_inputs(job).await;
        Ok(())
    }

    /// Rebuilds the tree's projections after an import of `persons_count`
    /// persons, records it in the history and completes the job with its
    /// summary.
    async fn complete_import(
        &self,
        job: &BackgroundJob,
        persons_count: usize,
        summary: &impl Serialize,
    ) -> Result<(), OxidGeneError> {
        self.profiles
            .rebuild_tree_full_transactional(&self.db, job.tree_id)
            .instrument(tracing::info_span!("import.projections"))
            .await?;
        history::record_import(
            &self.db,
            job.tree_id,
            &job.format,
            job.original_filename.clone(),
            persons_count,
        )
        .await?;
        let result = serde_json::to_string(summary)
            .map_err(|error| OxidGeneError::Internal(error.to_string()))?;
        if !BackgroundJobRepo::complete(&self.db, job.id, &self.worker_id, None, Some(result))
            .await?
        {
            return Err(OxidGeneError::Internal("background job lease lost".into()));
        }
        remove_live_job(job.id);
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
                tracing::warn!(
                    job_id = %job.id,
                    %key,
                    error.kind = error_kind(&error),
                    "could not delete job input"
                );
            }
        }
    }

    async fn finish_import(
        &self,
        job: &BackgroundJob,
        source_key: &str,
        summary: gedcom::ImportSummary,
    ) -> Result<(), OxidGeneError> {
        self.complete_import(job, summary.persons_count, &summary)
            .await?;
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

        let staged_media = self
            .stage_export_media(job.id, scratch.path(), &data.media_files)
            .await?;
        let artifact_path = scratch.path().join("artifact.gdz");
        self.package_gedzip(job.id, data.gedcom, staged_media, &artifact_path)
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
        history::record_export(&self.db, job.tree_id, &job.format, None).await?;
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

    /// Copies an export's media from the store into `scratch`; each staged
    /// file's archive path, MIME type and local path. A medium absent from
    /// the store is left out with a warning.
    async fn stage_export_media(
        &self,
        job_id: Uuid,
        scratch: &Path,
        media_files: &[(String, String, String)],
    ) -> Result<Vec<(String, String, std::path::PathBuf)>, OxidGeneError> {
        let media_root = scratch.join("media");
        tokio::fs::create_dir_all(&media_root).await?;
        let total = as_i64(media_files.len());
        let media_span = tracing::info_span!(
            "export.media",
            export.format = "gedzip",
            export.media.count = total,
        );
        let mut staged_media = Vec::with_capacity(media_files.len());
        async {
            for (index, (key, archive_path, mime_type)) in media_files.iter().enumerate() {
                let local_path = media_root.join(index.to_string());
                match self.media.get_to_file(key, &local_path).await {
                    Ok(()) => {
                        staged_media.push((archive_path.clone(), mime_type.clone(), local_path));
                    }
                    Err(error) => tracing::warn!(
                        job_id = %job_id,
                        error.kind = error_kind(&error),
                        "media absent from the store; not packed"
                    ),
                }
                self.progress(job_id, "media", as_i64(index + 1), total)
                    .await?;
            }
            Ok::<(), OxidGeneError>(())
        }
        .instrument(media_span)
        .await?;
        Ok(staged_media)
    }

    /// Writes the GEDZIP at `artifact_path` from `gedcom` and the staged
    /// media.
    async fn package_gedzip(
        &self,
        job_id: Uuid,
        gedcom: String,
        staged_media: Vec<(String, String, std::path::PathBuf)>,
        artifact_path: &Path,
    ) -> Result<(), OxidGeneError> {
        let archive_path = artifact_path.to_path_buf();
        // The packaging span is opened here so the blocking thread runs
        // inside it rather than in a trace of its own.
        let package_span = tracing::info_span!("export.package", export.format = "gedzip");
        let archive_task = crate::service::blocking::spawn_in(package_span.clone(), move || {
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
        self.with_heartbeat(job_id, "packaging", async {
            archive_task
                .await
                .map_err(|error| OxidGeneError::Internal(error.to_string()))?
        })
        .instrument(package_span)
        .await
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
        self.with_progress(job_id, future, || (phase, 0, 0)).await
    }

    /// Drives `future` to its end, reporting the phase, units done and units
    /// expected that `report` reads at every progress period.
    async fn with_progress<'p, T, F>(
        &self,
        job_id: Uuid,
        future: F,
        report: impl Fn() -> (&'p str, usize, usize),
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
                _ = heartbeat.tick() => {
                    let (phase, done, total) = report();
                    self.progress(job_id, phase, as_i64(done), as_i64(total)).await?;
                }
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

/// Log a failed maintenance pass under `code`; it is retried by the next.
fn log_maintenance_failure(code: &'static str, result: Result<(), OxidGeneError>) {
    if result.is_err() {
        tracing::warn!(
            error = code,
            "background job maintenance failed; the next pass retries"
        );
    }
}

/// `now` less `age`.
fn before(now: chrono::DateTime<chrono::Utc>, age: Duration) -> chrono::DateTime<chrono::Utc> {
    now - chrono::Duration::from_std(age).unwrap_or(chrono::TimeDelta::MAX)
}

/// Delete the artifacts of the exports completed before `now` less
/// [`EXPORT_ARTIFACT_TTL`].
async fn expire_artifacts(
    db: &DatabaseConnection,
    media: &dyn MediaStore,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), OxidGeneError> {
    let cutoff = before(now, EXPORT_ARTIFACT_TTL);
    for (job_id, key) in BackgroundJobRepo::artifacts_finished_before(db, cutoff).await? {
        release_export_artifact(db, media, job_id, &key).await?;
    }
    Ok(())
}

/// Delete the rows and the objects of the jobs ended before `now` less
/// [`ENDED_JOB_RETENTION`]. Objects first, so that an interrupted pass
/// leaves rows to find again rather than objects nothing points at.
async fn prune_ended_jobs(
    db: &DatabaseConnection,
    media: &dyn MediaStore,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), OxidGeneError> {
    let ended = BackgroundJobRepo::ended_before(db, before(now, ENDED_JOB_RETENTION)).await?;
    for job_id in &ended {
        media.delete_job(*job_id).await?;
    }
    BackgroundJobRepo::delete_ended(db, &ended).await?;
    Ok(())
}

/// Delete the objects under `jobs/` that no job needs — its row gone, or
/// ended without an artifact to download — once they are older than
/// [`ORPHAN_GRACE`], measured by the job id's own time stamp. A crash
/// between storing a job's inputs and creating its row, or between ending
/// it and removing them, leaves such objects.
async fn sweep_orphaned_job_objects(
    db: &DatabaseConnection,
    media: &dyn MediaStore,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), OxidGeneError> {
    let cutoff = before(now, ORPHAN_GRACE);
    let old: Vec<Uuid> = media
        .job_ids()
        .await?
        .into_iter()
        .filter(|job_id| created_before(*job_id, cutoff))
        .collect();
    let needed = BackgroundJobRepo::holding_objects(db, &old).await?;
    for job_id in old.into_iter().filter(|job_id| !needed.contains(job_id)) {
        media.delete_job(job_id).await?;
    }
    Ok(())
}

/// Whether the UUID v7 `id` was minted before `cutoff`; an id without a
/// time stamp never is.
fn created_before(id: Uuid, cutoff: chrono::DateTime<chrono::Utc>) -> bool {
    id.get_timestamp()
        .and_then(|stamp| {
            let (seconds, nanos) = stamp.to_unix();
            chrono::DateTime::from_timestamp(i64::try_from(seconds).ok()?, nanos)
        })
        .is_some_and(|minted| minted < cutoff)
}

/// Delete export `job_id`'s artifact `key`: forget it first, so that no
/// status offers a download of a deleted file, then remove the job's objects.
/// A crash in between leaves objects the orphan sweep removes.
pub(crate) async fn release_export_artifact(
    db: &DatabaseConnection,
    media: &dyn MediaStore,
    job_id: Uuid,
    key: &str,
) -> Result<(), OxidGeneError> {
    if BackgroundJobRepo::clear_artifact(db, job_id, key).await? {
        media.delete_job(job_id).await?;
    }
    Ok(())
}

/// `stream`, the artifact `key` of export `job_id`, deleting the artifact
/// once the stream has been read to its end.
///
/// An export is downloaded once, as soon as it completes, and left in store
/// it is a full copy of the tree and its media. Only a download read to the
/// end releases it: a broken one leaves the artifact for a retry, until
/// [`EXPORT_ARTIFACT_TTL`]. The deletion runs on its own task so that the
/// response's last chunk never waits for the database.
pub(crate) fn release_when_read(
    stream: crate::media::store::BlobStream,
    db: DatabaseConnection,
    media: Arc<dyn MediaStore>,
    job_id: Uuid,
    key: String,
) -> crate::media::store::BlobStream {
    use futures_util::StreamExt as _;

    let on_end = futures_util::stream::once(async move {
        // Spawned, but still part of the download's trace: the span is
        // opened here, under the response body's span.
        let span = tracing::info_span!("export.release");
        tokio::spawn(
            async move {
                if release_export_artifact(&db, &*media, job_id, &key)
                    .await
                    .is_err()
                {
                    tracing::warn!(
                        error = "export_artifact_release",
                        "could not delete a downloaded export; it expires later"
                    );
                }
            }
            .instrument(span),
        );
        None
    })
    .filter_map(std::future::ready);
    Box::pin(stream.chain(on_end))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectOptions, Database};

    #[test]
    fn geneanet_jobs_require_an_explicit_fidelity() {
        let mut payload = serde_json::json!({
            "collection": "{}", "deposit_sizes": {}, "archives": [], "fetched": []
        });
        assert!(serde_json::from_value::<GeneanetJobPayload>(payload.clone()).is_err());
        for fidelity in [
            geneanet::MediaFidelity::Originals,
            geneanet::MediaFidelity::Renditions,
        ] {
            payload["media_fidelity"] = serde_json::to_value(fidelity).unwrap();
            let decoded: GeneanetJobPayload = serde_json::from_value(payload.clone()).unwrap();
            assert_eq!(decoded.media_fidelity, fidelity);
        }
    }

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
    media_fidelity: geneanet::MediaFidelity,
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

// ── Job status, as both surfaces report it ─────────────────────────────

/// Where an export job stands.
#[derive(Debug, Clone, Serialize)]
pub struct ExportJobStatus {
    pub phase: String,
    pub done: i64,
    pub total: i64,
    /// Where to download the archive, once it is complete.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_url: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// A stable error code, once the job has failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Where an import job stands, with its receipt once it has completed.
#[derive(Debug, Clone, Serialize)]
pub struct ImportJobStatus {
    pub phase: String,
    pub done: i64,
    pub total: i64,
    /// The receipt of a completed file import.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<gedcom::ImportSummary>,
    /// The receipt of a completed Geneanet import.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geneanet_result: Option<geneanet::GeneanetImportSummary>,
    /// A stable error code, once the job has failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Queue a GEDZIP export of tree `tree_id`; the job's id.
///
/// A tree runs one job at a time: while another is queued or running, this
/// is a [`OxidGeneError::Conflict`].
pub async fn start_export_job(
    db: &DatabaseConnection,
    tree_id: Uuid,
    merge_occupations: bool,
    merge_names: bool,
) -> Result<Uuid, OxidGeneError> {
    TreeRepo::get(db, tree_id).await?;
    let job_id = Uuid::now_v7();
    BackgroundJobRepo::create(
        db,
        NewBackgroundJob {
            id: job_id,
            tree_id,
            kind: BackgroundJobKind::Export,
            format: "gedzip".into(),
            source_key: None,
            payload_json: None,
            original_filename: None,
            merge_occupations,
            merge_names,
        },
    )
    .await?;
    Ok(job_id)
}

/// Where export job `job_id` of tree `tree_id` stands.
pub async fn export_job_status(
    db: &DatabaseConnection,
    tree_id: Uuid,
    job_id: Uuid,
) -> Result<ExportJobStatus, OxidGeneError> {
    if let Some(progress) = live_job_progress(tree_id, job_id, BackgroundJobKind::Export) {
        return Ok(ExportJobStatus {
            phase: progress.phase,
            done: count(progress.done),
            total: count(progress.total),
            download_url: None,
            warnings: Vec::new(),
            error: None,
        });
    }
    let job = job_of_kind(db, tree_id, job_id, BackgroundJobKind::Export).await?;
    let warnings = receipt::<ExportReceipt>(job.result_json.as_deref())?
        .map_or_else(Vec::new, |receipt| receipt.warnings);
    // A downloaded or expired artifact is gone, and so is its link.
    let download_url = (job.status == BackgroundJobStatus::Completed.as_str()
        && job.artifact_key.is_some())
    .then(|| format!("/api/v1/trees/{tree_id}/export-jobs/{job_id}/download"));
    Ok(ExportJobStatus {
        phase: job.phase,
        done: count(job.done),
        total: count(job.total),
        download_url,
        warnings,
        error: job.error_code,
    })
}

/// Where import job `job_id` of tree `tree_id` stands.
pub async fn import_job_status(
    db: &DatabaseConnection,
    tree_id: Uuid,
    job_id: Uuid,
) -> Result<ImportJobStatus, OxidGeneError> {
    if let Some(progress) = live_job_progress(tree_id, job_id, BackgroundJobKind::Import) {
        return Ok(ImportJobStatus {
            phase: progress.phase,
            done: count(progress.done),
            total: count(progress.total),
            result: None,
            geneanet_result: None,
            error: None,
        });
    }
    let job = job_of_kind(db, tree_id, job_id, BackgroundJobKind::Import).await?;
    let serialized = job.result_json.as_deref();
    let (result, geneanet_result) = if job.format == "geneanet" {
        (None, receipt(serialized)?)
    } else {
        (receipt(serialized)?, None)
    };
    Ok(ImportJobStatus {
        phase: job.phase,
        done: count(job.done),
        total: count(job.total),
        result,
        geneanet_result,
        error: job.error_code,
    })
}

/// The export artifact of completed job `job_id` of tree `tree_id`: its
/// storage key. An artifact released by its first complete download, or
/// expired, is not found.
pub async fn export_artifact(
    db: &DatabaseConnection,
    tree_id: Uuid,
    job_id: Uuid,
) -> Result<String, OxidGeneError> {
    let job = job_of_kind(db, tree_id, job_id, BackgroundJobKind::Export).await?;
    if job.status != BackgroundJobStatus::Completed.as_str() {
        return Err(OxidGeneError::Validation(
            "export artifact is not ready".into(),
        ));
    }
    job.artifact_key.ok_or(OxidGeneError::NotFound {
        entity: "ExportArtifact",
        id: job_id,
    })
}

/// Job `job_id` of tree `tree_id`, which must be of `kind` and belong to a
/// live tree.
///
/// The status reads answer a job this process is running from memory before
/// coming here, so that a poll never waits on the database — on SQLite the
/// running job may hold its only connection. The tree is checked here, once
/// the answer has to come from the database anyway.
async fn job_of_kind(
    db: &DatabaseConnection,
    tree_id: Uuid,
    job_id: Uuid,
    kind: BackgroundJobKind,
) -> Result<BackgroundJob, OxidGeneError> {
    crate::service::scope::require_live_tree(db, tree_id).await?;
    let job = BackgroundJobRepo::get_in_tree(db, tree_id, job_id).await?;
    if job.kind != kind.as_str() {
        return Err(OxidGeneError::NotFound {
            entity: match kind {
                BackgroundJobKind::Export => "ExportJob",
                BackgroundJobKind::Import => "ImportJob",
            },
            id: job_id,
        });
    }
    Ok(job)
}

/// What a completed export job records about itself.
#[derive(Deserialize)]
struct ExportReceipt {
    warnings: Vec<String>,
}

/// A job's stored receipt, read back.
fn receipt<T: DeserializeOwned>(serialized: Option<&str>) -> Result<Option<T>, OxidGeneError> {
    serialized
        .map(serde_json::from_str)
        .transpose()
        .map_err(|error| OxidGeneError::Internal(error.to_string()))
}

/// A progress count as reported: never negative.
fn count(value: i64) -> i64 {
    value.max(0)
}
