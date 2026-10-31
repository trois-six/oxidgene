//! Durable import and export job execution shared by server and desktop workers.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Duration;

use oxidgene_core::OxidGeneError;
use oxidgene_db::repo::{
    BackgroundJob, BackgroundJobKind, BackgroundJobRepo, BackgroundJobStatus, NewBackgroundJob,
    TreeRepo, db_err,
};
use oxidgene_gedcom::export::GedzipFileWriter;
use sea_orm::{DatabaseConnection, DatabaseTransaction, DbBackend, TransactionTrait};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tracing::Instrument as _;
use uuid::Uuid;

use super::job_watch::{self, JobWait, Versioned};
use super::tag_bulk::{self, TagTargetSpec};
use super::{gedcom, geneanet, history};
use crate::error_contract::error_kind;
use crate::media::MediaStore;
use crate::media::job_objects::{self, RecordingStore};
use crate::media::store::{job_blob_key, job_input_blob_key};
use crate::profile::ProfileService;
use crate::workdir::WorkDir;

pub const DEFAULT_LEASE_DURATION: Duration = Duration::from_secs(30);
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// How long a completed export's artifact can be downloaded, from the
/// export's completion and however often it is downloaded meanwhile: long
/// enough to download it again when a save went wrong, short enough that a
/// full copy of the tree does not linger in store. Past it, the status
/// offers no download, the download route answers 404, and the next
/// maintenance pass deletes the artifact.
pub const EXPORT_ARTIFACT_TTL: Duration = Duration::from_secs(60 * 60);

/// How long an ended job's row stays, with the result its status reports.
pub const ENDED_JOB_RETENTION: Duration = Duration::from_secs(24 * 60 * 60);

/// How old an entry under `jobs/` that no job needs must be before it is
/// swept: a job's inputs are stored before its row is created, and staging a
/// Geneanet import's archives can take a while.
const ORPHAN_GRACE: Duration = Duration::from_secs(24 * 60 * 60);

/// How often a worker expires artifacts, prunes ended jobs and sweeps
/// orphaned job objects; it also does so when it starts. A tenth of
/// [`EXPORT_ARTIFACT_TTL`], so an expired artifact stays in store a few
/// minutes at most after nobody can download it any more.
const MAINTENANCE_PERIOD: Duration = Duration::from_secs(6 * 60);

/// How many times a job is claimed before it is failed unrun.
///
/// A job is claimed again after its worker stopped mid-run: a restart, an
/// expired lease. Most such stops are the host's doing, but one that the job
/// itself causes — a file that crashes the process, which a release build,
/// compiled to abort on panic, cannot catch — would otherwise be retried at
/// every start, forever, taking every job queued behind it down with it.
pub const MAX_ATTEMPTS: i32 = 3;

// ── The jobs of this process ───────────────────────────────────────────
//
// A worker registers the job it runs here, with its progress and its
// cancellation token, so that status reads, the tree guard and cancellation
// requests answer from memory. Under SQLite this is what keeps them from
// waiting: the running import holds the only writer for its whole
// transaction, and on an in-memory database the only connection. A
// cancellation requested under SQLite for a job this process does not run
// yet — queued, or claimed but not registered — is a mark the worker turns
// into a database write once the writer is free, or into the token of the
// job when it registers it (see `docs/architecture.md` §6).

/// A request that a running job stop, shared by its worker and whoever asks
/// it to stop. The job reads it between two units of work.
#[derive(Debug, Clone, Default)]
pub(crate) struct CancelToken {
    cancelled: Arc<AtomicBool>,
    /// In tests, a stop arriving at a known unit of work.
    #[cfg(test)]
    planned: Option<Arc<PlannedStop>>,
}

/// In tests, a stop a job makes at the `left`-th check of its progress
/// phase `phase` (a variant's name: `Database`, `People`, `Media`): a
/// cancellation, or with `fails` a database failure.
#[cfg(test)]
#[derive(Debug)]
pub(crate) struct PlannedStop {
    phase: String,
    left: std::sync::atomic::AtomicUsize,
    fails: bool,
}

impl CancelToken {
    /// Ask the job to stop.
    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    /// Whether the job was asked to stop.
    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Note a check made in progress phase `phase`, stopping the job when it
    /// is the one a test planned (see [`plan_stop`]).
    #[cfg(test)]
    pub(crate) fn reach(&self, phase: &str) -> Result<(), OxidGeneError> {
        if let Some(planned) = &self.planned
            && planned.phase == phase
            && planned
                .left
                .try_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                    left.checked_sub(1)
                })
                == Ok(1)
        {
            if planned.fails {
                return Err(OxidGeneError::Database("a planned failure".into()));
            }
            self.cancel();
        }
        Ok(())
    }

    /// `Err` once the job was asked to stop: what stops its work at the next
    /// unit, rolling its transaction back.
    pub(crate) fn check(&self) -> Result<(), OxidGeneError> {
        if self.is_cancelled() {
            Err(OxidGeneError::Conflict("the job was cancelled".into()))
        } else {
            Ok(())
        }
    }
}

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
    cancel: CancelToken,
}

/// A cancellation asked, under SQLite, of a job this process was not
/// running: written by the worker once the writer is free.
#[derive(Debug)]
struct CancelMark {
    tree_id: Uuid,
    kind: String,
    /// The job was queued when cancelled, so it ends unrun.
    queued: bool,
}

#[derive(Debug, Default)]
struct Registry {
    running: HashMap<Uuid, LiveJob>,
    marked: HashMap<Uuid, CancelMark>,
    /// In tests, the stops planned for jobs not registered yet.
    #[cfg(test)]
    planned: HashMap<Uuid, PlannedStop>,
}

/// Stop job `job_id`, once a worker of this process runs it, at the `nth`
/// check of its progress phase `phase`: cancel it, or with `fails` fail it
/// as a database error would.
#[cfg(test)]
pub(crate) fn plan_stop(job_id: Uuid, phase: &str, nth: usize, fails: bool) {
    registry().planned.insert(
        job_id,
        PlannedStop {
            phase: phase.to_string(),
            left: std::sync::atomic::AtomicUsize::new(nth),
            fails,
        },
    );
}

/// `job`, with the stop a test planned for it, if any.
#[cfg(test)]
fn with_planned_stop(job_id: Uuid, mut job: LiveJob) -> LiveJob {
    job.cancel.planned = registry().planned.remove(&job_id).map(Arc::new);
    job
}

static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();

fn registry() -> MutexGuard<'static, Registry> {
    REGISTRY
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// Forget job `job_id` as running here: its state is the database's again.
fn remove_live_job(job_id: Uuid) {
    registry().running.remove(&job_id);
    job_watch::announce(job_id);
}

/// What this process knows of a job without the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LiveState {
    /// It runs here: its progress, and whether it was asked to stop.
    Running {
        progress: LiveJobProgress,
        cancelling: bool,
    },
    /// Its cancellation waits for the writer: it was queued, and ends unrun.
    Cancelled,
    /// Its cancellation waits for its worker to register it.
    Cancelling,
}

impl LiveState {
    /// The phase a status read reports.
    fn phase(&self) -> String {
        match self {
            Self::Running {
                cancelling: true, ..
            }
            | Self::Cancelling => PHASE_CANCELLING.to_string(),
            Self::Running { progress, .. } => progress.phase.clone(),
            Self::Cancelled => BackgroundJobStatus::Cancelled.as_str().to_string(),
        }
    }

    fn counts(&self) -> (i64, i64) {
        match self {
            Self::Running { progress, .. } => (count(progress.done), count(progress.total)),
            Self::Cancelled | Self::Cancelling => (0, 0),
        }
    }
}

/// The phase a status reports while a running job stops at a cancellation
/// and undoes what it wrote.
const PHASE_CANCELLING: &str = "cancelling";

/// What this process knows of job `job_id` of `kind` in `tree_id` without
/// reading the database, if anything.
pub(crate) fn live_state(
    tree_id: Uuid,
    job_id: Uuid,
    kind: BackgroundJobKind,
) -> Option<LiveState> {
    let registry = registry();
    if let Some(job) = registry.running.get(&job_id) {
        return (job.tree_id == tree_id && job.kind == kind.as_str()).then(|| LiveState::Running {
            progress: job.progress.clone(),
            cancelling: job.cancel.is_cancelled(),
        });
    }
    let mark = registry.marked.get(&job_id)?;
    let state = if mark.queued {
        LiveState::Cancelled
    } else {
        LiveState::Cancelling
    };
    (mark.tree_id == tree_id && mark.kind == kind.as_str()).then_some(state)
}

/// How the active job `job_id`, which the database says has
/// `cancel_requested`, stands for the tree list: `None` once it is
/// cancelled although the database does not say so yet, otherwise whether
/// it is being cancelled.
pub(crate) fn active_job_cancelling(job_id: Uuid, cancel_requested: bool) -> Option<bool> {
    let registry = registry();
    if let Some(job) = registry.running.get(&job_id) {
        return Some(cancel_requested || job.cancel.is_cancelled());
    }
    match registry.marked.get(&job_id) {
        Some(mark) if mark.queued => None,
        Some(_) => Some(true),
        None => Some(cancel_requested),
    }
}

/// Ask job `job_id` of `kind` in `tree_id`, if this process runs it, to
/// stop; otherwise, with `mark`, note the request for the worker of this
/// process. Whether the job runs here.
fn cancel_here(tree_id: Uuid, job_id: Uuid, kind: BackgroundJobKind, mark: Option<bool>) -> bool {
    let (runs_here, changed) = {
        let mut registry = registry();
        match registry.running.get(&job_id) {
            Some(job) if job.tree_id == tree_id && job.kind == kind.as_str() => {
                job.cancel.cancel();
                (true, true)
            }
            Some(_) => (false, false),
            None => match mark {
                Some(queued) => {
                    registry.marked.insert(
                        job_id,
                        CancelMark {
                            tree_id,
                            kind: kind.as_str().to_string(),
                            queued,
                        },
                    );
                    (false, true)
                }
                None => (false, false),
            },
        }
    };
    if changed {
        job_watch::announce(job_id);
    }
    runs_here
}

/// Ask job `job_id`, running here, to stop: its worker read the request in
/// the database.
fn cancel_running_job(job_id: Uuid) {
    let found = registry()
        .running
        .get(&job_id)
        .map(|job| job.cancel.cancel())
        .is_some();
    if found {
        job_watch::announce(job_id);
    }
}

pub(crate) struct LiveJobGuard {
    job_id: Uuid,
    cancel: CancelToken,
}

impl LiveJobGuard {
    /// Registers a running job of `kind` in `tree_id` at its start.
    #[cfg(test)]
    pub(crate) fn for_test(job_id: Uuid, tree_id: Uuid, kind: BackgroundJobKind) -> Self {
        Self::register(
            job_id,
            LiveJob {
                tree_id,
                kind: kind.as_str().to_string(),
                progress: LiveJobProgress {
                    phase: "staging".to_string(),
                    done: 0,
                    total: 0,
                },
                cancel: CancelToken::default(),
            },
        )
    }

    fn new(job: &BackgroundJob) -> Self {
        Self::register(
            job.id,
            LiveJob {
                tree_id: job.tree_id,
                kind: job.kind.clone(),
                progress: LiveJobProgress {
                    phase: job.phase.clone(),
                    done: job.done,
                    total: job.total,
                },
                cancel: CancelToken::default(),
            },
        )
    }

    /// Register `job` as running here, its token already cancelled when a
    /// cancellation was marked for it before it got here.
    fn register(job_id: Uuid, job: LiveJob) -> Self {
        #[cfg(test)]
        let job = with_planned_stop(job_id, job);
        let cancel = job.cancel.clone();
        {
            let mut registry = registry();
            if registry.marked.remove(&job_id).is_some() {
                cancel.cancel();
            }
            registry.running.insert(job_id, job);
        }
        job_watch::announce(job_id);
        Self { job_id, cancel }
    }

    /// The job's cancellation token.
    pub(crate) fn cancel_token(&self) -> CancelToken {
        self.cancel.clone()
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
    work_dir: WorkDir,
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
            work_dir: WorkDir::temporary(),
        }
    }

    /// Keep job scratch under `work_dir` (see [`crate::workdir`]).
    #[must_use]
    pub fn with_work_dir(mut self, work_dir: WorkDir) -> Self {
        self.work_dir = work_dir;
        self
    }

    /// A scratch directory for job `job_id`, removed when dropped: when
    /// the job ends, completed or failed.
    fn scratch(&self, job_id: Uuid) -> Result<tempfile::TempDir, OxidGeneError> {
        self.work_dir.job_scratch(job_id).map_err(OxidGeneError::Io)
    }

    /// Claim and execute at most one job. Returns whether work was claimed.
    pub async fn run_once(&self) -> Result<bool, OxidGeneError> {
        self.write_marked_cancellations().await;
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
        let live_job = LiveJobGuard::new(&job);
        let cancel = live_job.cancel_token();
        if job.cancel_requested {
            cancel.cancel();
        }
        if job.attempt > MAX_ATTEMPTS {
            self.fail_exhausted(&job).await?;
            return Ok(true);
        }

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
            let Err(error) = Box::pin(self.execute(&job, &cancel)).await else {
                return Ok(());
            };
            if cancel.is_cancelled() {
                tracing::info!(
                    job.kind = %job.kind,
                    job.format = %job.format,
                    "background job cancelled"
                );
                return self.end(&job, Ending::Cancelled).await;
            }
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
            self.end(&job, Ending::Failed(code)).await
        }
        .instrument(span)
        .await?;
        drop(live_job);
        // A job is the most memory the process holds; what it freed goes
        // back now rather than sitting in the allocator until the next one.
        let _ = super::blocking::spawn(crate::memory::release_free_memory).await;
        Ok(true)
    }

    /// Fail `job`, claimed more than [`MAX_ATTEMPTS`] times, without running
    /// it again.
    async fn fail_exhausted(&self, job: &BackgroundJob) -> Result<(), OxidGeneError> {
        tracing::error!(
            error.category = "job_attempts_exhausted",
            job.kind = %job.kind,
            job.format = %job.format,
            job.attempt = job.attempt,
            "background job stopped too many times; failed without running it again"
        );
        self.end(job, Ending::Failed("job_attempts_exhausted"))
            .await
    }

    /// End `job`, stopped before it completed, as `ending` says, while this
    /// worker still holds it: first take back the media it stored — its
    /// rows are rolled back already, and the tree is still the job's — then
    /// record how it ended, then remove what it keeps under `jobs/`. A pass
    /// interrupted halfway leaves the job running, to be claimed again, or
    /// objects the maintenance sweep finds.
    async fn end(&self, job: &BackgroundJob, ending: Ending) -> Result<(), OxidGeneError> {
        let held = BackgroundJobRepo::held_by(&self.db, job.id, &self.worker_id).await?;
        let discarded = held && self.discard_stored_objects(job.id).await;
        let ended = match ending {
            Ending::Cancelled => {
                BackgroundJobRepo::cancel(&self.db, job.id, &self.worker_id).await?
            }
            Ending::Failed(code) => {
                BackgroundJobRepo::fail(&self.db, job.id, &self.worker_id, code).await?
            }
        };
        if ended {
            remove_live_job(job.id);
            if discarded {
                self.delete_job_objects(job.id).await;
            }
        }
        Ok(())
    }

    /// Delete the media objects job `job_id` stored that no row names (see
    /// [`job_objects`]). Whether it went through; a failure is logged and
    /// left to the maintenance sweep.
    async fn discard_stored_objects(&self, job_id: Uuid) -> bool {
        match job_objects::discard_unreferenced(&self.db, &*self.media, job_id).await {
            Ok(_) => true,
            Err(error) => {
                tracing::warn!(
                    job_id = %job_id,
                    error.kind = error_kind(&error),
                    "could not take back the media of an unfinished import"
                );
                false
            }
        }
    }

    /// Delete everything job `job_id` keeps under `jobs/`: its inputs, and
    /// the artifact of an export that did not complete.
    async fn delete_job_objects(&self, job_id: Uuid) {
        if let Err(error) = self.media.delete_job(job_id).await {
            tracing::warn!(
                job_id = %job_id,
                error.kind = error_kind(&error),
                "could not delete job objects"
            );
        }
    }

    /// Write the cancellations requested of jobs this process did not run
    /// (see [`cancel_job`]), now that this worker, between two jobs, leaves
    /// the writer free: a queued job ends cancelled, unrun, and its inputs
    /// go. A mark naming a job of another database is left alone.
    async fn write_marked_cancellations(&self) {
        let marked: Vec<Uuid> = registry().marked.keys().copied().collect();
        for job_id in marked {
            match self.write_cancellation(job_id).await {
                Ok(true) => {
                    registry().marked.remove(&job_id);
                }
                Ok(false) => {}
                Err(error) => tracing::warn!(
                    job_id = %job_id,
                    error.kind = error_kind(&error),
                    "could not record a job cancellation; retrying"
                ),
            }
        }
    }

    /// Write the cancellation marked for job `job_id`; `false` when this
    /// database has no such job.
    async fn write_cancellation(&self, job_id: Uuid) -> Result<bool, OxidGeneError> {
        let Some(job) = BackgroundJobRepo::find(&self.db, job_id).await? else {
            return Ok(false);
        };
        if job.status == BackgroundJobStatus::Queued.as_str() {
            if BackgroundJobRepo::cancel_queued(&self.db, job_id).await? {
                self.delete_job_objects(job_id).await;
            }
        } else if job.status == BackgroundJobStatus::Running.as_str() {
            BackgroundJobRepo::request_cancel(&self.db, job_id).await?;
        }
        job_watch::announce(job_id);
        Ok(true)
    }

    /// Run until the process is shut down.
    ///
    /// The loop runs on a task of its own, restarted after a pause if it
    /// panics: one job's failure must not end every job after it. Dropping
    /// this future stops the loop.
    pub async fn run(self) {
        let pause = self.poll_interval;
        supervise(move || self.clone().run_loop(), pause).await;
    }

    async fn run_loop(self) {
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
    /// [`ENDED_JOB_RETENTION`] ago, the objects under `jobs/` that no job
    /// needs any more, once older than a day, with the media an unfinished
    /// import stored and did not take back, and the working files a crashed
    /// run left (see [`WorkDir::sweep`]). Failures are logged; the next pass
    /// retries.
    pub async fn maintain(&self, now: chrono::DateTime<chrono::Utc>) {
        let work_dir = self.work_dir.clone();
        let swept = tokio::task::spawn_blocking(move || work_dir.sweep(now.into())).await;
        log_maintenance_failure(
            "working_file_sweep",
            swept
                .map(drop)
                .map_err(|error| OxidGeneError::Internal(error.to_string())),
        );
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
    async fn execute(
        &self,
        job: &BackgroundJob,
        cancel: &CancelToken,
    ) -> Result<(), OxidGeneError> {
        match job.kind.as_str() {
            "import" => Box::pin(self.execute_import(job, cancel)).await,
            "export" => Box::pin(self.execute_export(job, cancel)).await,
            _ => Err(OxidGeneError::Validation("unknown job kind".into())),
        }
    }

    /// Run import `job` all or nothing: its rows, their projections, its
    /// history entry and its completion commit in one transaction, and the
    /// media it stores are taken back unless that transaction commits.
    #[tracing::instrument(
        name = "import.job",
        skip_all,
        fields(import.format = %job.format)
    )]
    async fn execute_import(
        &self,
        job: &BackgroundJob,
        cancel: &CancelToken,
    ) -> Result<(), OxidGeneError> {
        let source_key = job
            .source_key
            .as_deref()
            .ok_or_else(|| OxidGeneError::Validation("import job has no source".into()))?;
        // An earlier attempt that stopped with the process committed
        // nothing: what it stored goes before this one starts.
        job_objects::discard_unreferenced(&self.db, &*self.media, job.id).await?;
        if job.format == "geneanet" {
            return Box::pin(self.execute_geneanet_import(job, source_key, cancel)).await;
        }
        let scratch = self.scratch(job.id)?;
        let source = scratch.path().join(format!("source.{}", job.format));
        self.progress(job.id, "staging", 0, 0).await?;
        self.media.get_to_file(source_key, &source).await?;
        cancel.check()?;

        let progress = Arc::new(gedcom::FileImportProgress::cancellable(cancel.clone()));
        let report = || {
            let (phase, done, total, _, _) = progress.read();
            (import_phase(phase), done, total)
        };
        let store = RecordingStore::new(&*self.media, job.id);
        let parsed = self
            .with_progress(
                job.id,
                Box::pin(self.parse_import(job, &store, &source, &progress)),
                report,
            )
            .await?;
        progress.check_cancelled()?;
        let transaction = self.db.begin().await.map_err(db_err)?;
        let written = self
            .with_progress(
                job.id,
                Box::pin(self.write_file_import(&transaction, job.tree_id, parsed, &progress)),
                report,
            )
            .await;
        let (transaction, summary) = rolled_back_on_error(transaction, written).await?;
        self.complete_import(transaction, job, summary.persons_count, &summary)
            .await?;
        self.release_job_objects(job.id).await;
        Ok(())
    }

    /// Writes the rows of `parsed` and the projections of tree `tree_id` on
    /// `transaction`, stopping at a cancellation between two batches.
    async fn write_file_import(
        &self,
        transaction: &DatabaseTransaction,
        tree_id: Uuid,
        parsed: oxidgene_gedcom::ImportResult,
        progress: &gedcom::FileImportProgress,
    ) -> Result<gedcom::ImportSummary, OxidGeneError> {
        progress.begin(
            gedcom::FileImportPhase::Database,
            gedcom::persisted_entity_count(&parsed),
        );
        let summary = gedcom::persist_import_result_in(transaction, parsed, &mut |inserted| {
            progress.advance_by(inserted);
            progress.check_cancelled()
        })
        .await?;
        progress.check_cancelled()?;
        progress.enter(gedcom::FileImportPhase::Projections);
        self.profiles
            .rebuild_tree_full(transaction, tree_id)
            .instrument(tracing::info_span!("import.projections"))
            .await?;
        progress.check_cancelled()?;
        Ok(summary)
    }

    /// Reads the staged `source` of a GEDCOM, GEDZIP or GeneWeb import,
    /// storing a GEDZIP's media in `store`.
    async fn parse_import(
        &self,
        job: &BackgroundJob,
        store: &dyn MediaStore,
        source: &Path,
        progress: &gedcom::FileImportProgress,
    ) -> Result<oxidgene_gedcom::ImportResult, OxidGeneError> {
        match job.format.as_str() {
            "gedcom" => {
                progress.enter(gedcom::FileImportPhase::Parsing);
                let source = tokio::fs::read(source).await?;
                let tree_id = job.tree_id;
                parse_off_thread("gedcom", move || {
                    oxidgene_gedcom::import::import_gedcom_bytes(&source, tree_id)
                })
                .await
            }
            "gedzip" => gedcom::prepare_gedzip_file(store, job.tree_id, source, progress).await,
            "geneweb" => {
                progress.enter(gedcom::FileImportPhase::Parsing);
                let source = tokio::fs::read(source).await?;
                let origin = safe_origin_file(job.original_filename.as_deref());
                let tree_id = job.tree_id;
                parse_off_thread("geneweb", move || {
                    oxidgene_gedcom::geneweb::import_geneweb(&source, &origin, tree_id)
                })
                .await
            }
            // A reader that panics, standing in for one a crafted file
            // crashes.
            #[cfg(test)]
            "test-panic" => {
                parse_off_thread(
                    "test-panic",
                    || -> Result<oxidgene_gedcom::ImportResult, String> {
                        panic!("a reader crashed")
                    },
                )
                .await
            }
            _ => Err(OxidGeneError::Validation("unknown import format".into())),
        }
    }

    async fn execute_geneanet_import(
        &self,
        job: &BackgroundJob,
        source_key: &str,
        cancel: &CancelToken,
    ) -> Result<(), OxidGeneError> {
        let payload = geneanet_payload(job)?;
        let scratch = self.scratch(job.id)?;
        self.progress(job.id, "staging", 0, 0).await?;
        let source = scratch.path().join("source.gw");
        self.media.get_to_file(source_key, &source).await?;

        let (archive_paths, fetched) = self
            .stage_geneanet_inputs(scratch.path(), &payload, cancel)
            .await?;

        let gw = tokio::fs::read(source).await?;
        let origin_file = safe_origin_file(job.original_filename.as_deref());
        let progress = Arc::new(geneanet::ImportProgress::cancellable(cancel.clone()));
        let store = RecordingStore::new(&*self.media, job.id);
        let transaction = self.db.begin().await.map_err(db_err)?;
        let import = async {
            let summary = geneanet::import(
                &transaction,
                &store,
                job.tree_id,
                &gw,
                &origin_file,
                &payload.collection,
                &payload.deposit_sizes,
                &archive_paths,
                &fetched,
                payload.media_fidelity,
                &progress,
            )
            .await?;
            progress.check_cancelled()?;
            self.profiles
                .rebuild_tree_full(&transaction, job.tree_id)
                .instrument(tracing::info_span!("import.projections"))
                .await?;
            progress.check_cancelled()?;
            Ok(summary)
        };
        let written = self
            .with_progress(job.id, Box::pin(import), || {
                let (phase, done, total) = progress.read();
                (geneanet_phase(phase), done, total)
            })
            .await;
        let (transaction, summary) = rolled_back_on_error(transaction, written).await?;
        self.complete_import(transaction, job, summary.receipt.persons_count, &summary)
            .await?;
        self.release_job_objects(job.id).await;
        Ok(())
    }

    /// Downloads a Geneanet import's archives and fetched pages next to its
    /// source, returning the archives' paths and each fetched URL's path.
    async fn stage_geneanet_inputs(
        &self,
        scratch: &Path,
        payload: &GeneanetJobPayload,
        cancel: &CancelToken,
    ) -> Result<(Vec<String>, HashMap<String, String>), OxidGeneError> {
        let archive_root = scratch.join("archives");
        tokio::fs::create_dir_all(&archive_root).await?;
        let mut archive_paths = Vec::with_capacity(payload.archives.len());
        for (index, input) in payload.archives.iter().enumerate() {
            cancel.check()?;
            let path = archive_root.join(format!("{index}-{}", input.file_name));
            self.media.get_to_file(&input.key, &path).await?;
            archive_paths.push(path.to_string_lossy().into_owned());
        }

        let fetched_root = scratch.join("fetched");
        tokio::fs::create_dir_all(&fetched_root).await?;
        let mut fetched = HashMap::with_capacity(payload.fetched.len());
        for (index, input) in payload.fetched.iter().enumerate() {
            cancel.check()?;
            let path = fetched_root.join(index.to_string());
            self.media.get_to_file(&input.key, &path).await?;
            fetched.insert(input.url.clone(), path.to_string_lossy().into_owned());
        }
        Ok((archive_paths, fetched))
    }

    /// Records the import of `persons_count` persons in the tree's history
    /// and completes the job with its summary, on the import's
    /// `transaction`, then commits it: the import's rows, its history entry
    /// and its completion become visible together, or not at all.
    async fn complete_import(
        &self,
        transaction: DatabaseTransaction,
        job: &BackgroundJob,
        persons_count: usize,
        summary: &impl Serialize,
    ) -> Result<(), OxidGeneError> {
        history::record_import(
            &transaction,
            job.tree_id,
            &job.format,
            job.original_filename.clone(),
            persons_count,
        )
        .await?;
        let result = serde_json::to_string(summary)
            .map_err(|error| OxidGeneError::Internal(error.to_string()))?;
        if !BackgroundJobRepo::complete(&transaction, job.id, &self.worker_id, None, Some(result))
            .await?
        {
            // Dropped, the transaction rolls back.
            return Err(OxidGeneError::Internal("background job lease lost".into()));
        }
        transaction.commit().await.map_err(db_err)?;
        remove_live_job(job.id);
        Ok(())
    }

    /// Remove what completed import `job_id` leaves in store beyond its
    /// media: its inputs, its records of what it stored, and any file it
    /// stored that no row ended up naming.
    async fn release_job_objects(&self, job_id: Uuid) {
        if self.discard_stored_objects(job_id).await {
            self.delete_job_objects(job_id).await;
        }
    }

    #[tracing::instrument(
        name = "export.job",
        skip_all,
        fields(export.format = %job.format)
    )]
    async fn execute_export(
        &self,
        job: &BackgroundJob,
        cancel: &CancelToken,
    ) -> Result<(), OxidGeneError> {
        if job.format != "gedzip" {
            return Err(OxidGeneError::Validation("unknown export format".into()));
        }
        let scratch = self.scratch(job.id)?;
        self.progress(job.id, "loading", 0, 0).await?;
        let scope = scope_of(job)?;
        let data = self
            .with_heartbeat(
                job.id,
                "loading",
                gedcom::load_and_export(
                    &self.db,
                    job.tree_id,
                    choices_of(job),
                    scope.as_ref(),
                    true,
                ),
            )
            .await?;
        cancel.check()?;

        let (artifact_key, size) = self
            .publish_gedzip(
                job.id,
                scratch.path(),
                data.gedcom,
                &data.media_files,
                cancel,
            )
            .await?;
        cancel.check()?;
        let receipt = ExportReceipt {
            warnings: data.warnings,
            size_bytes: Some(size),
        };
        self.complete_export(job, artifact_key, &receipt, scope.as_ref())
            .await
    }

    /// Packs `gedcom` and the media of `media_files` into export `job_id`'s
    /// GEDZIP in `scratch`, then stores it: its key and size.
    async fn publish_gedzip(
        &self,
        job_id: Uuid,
        scratch: &Path,
        gedcom: String,
        media_files: &[(String, String)],
        cancel: &CancelToken,
    ) -> Result<(String, i64), OxidGeneError> {
        let staged_media = self
            .stage_export_media(job_id, scratch, media_files, cancel)
            .await?;
        let artifact_path = scratch.join("artifact.gdz");
        self.package_gedzip(job_id, gedcom, staged_media, &artifact_path, cancel)
            .await?;
        cancel.check()?;
        let artifact_key = job_blob_key(job_id, "artifact", "gdz")?;
        let stored = self
            .with_heartbeat(job_id, "publishing", async {
                self.media.put_file(&artifact_key, &artifact_path).await
            })
            .instrument(tracing::info_span!(
                "export.publish",
                export.format = "gedzip"
            ))
            .await?;
        Ok((artifact_key, stored.size))
    }

    /// Records export `job` in its tree's history and completes it with
    /// `artifact_key` and `receipt`, both in one transaction.
    async fn complete_export(
        &self,
        job: &BackgroundJob,
        artifact_key: String,
        receipt: &ExportReceipt,
        scope: Option<&TagTargetSpec>,
    ) -> Result<(), OxidGeneError> {
        let result = serde_json::to_string(receipt)
            .map_err(|error| OxidGeneError::Internal(error.to_string()))?;
        let transaction = self.db.begin().await.map_err(db_err)?;
        history::record_export(
            &transaction,
            job.tree_id,
            &job.format,
            None,
            gedcom::scope_details(scope)?,
        )
        .await?;
        if !BackgroundJobRepo::complete(
            &transaction,
            job.id,
            &self.worker_id,
            Some(artifact_key),
            Some(result),
        )
        .await?
        {
            return Err(OxidGeneError::Internal("background job lease lost".into()));
        }
        transaction.commit().await.map_err(db_err)?;
        remove_live_job(job.id);
        Ok(())
    }

    /// Copies an export's media from the store into `scratch`; each staged
    /// file's archive path and local path. A medium absent from
    /// the store is left out with a warning.
    async fn stage_export_media(
        &self,
        job_id: Uuid,
        scratch: &Path,
        media_files: &[(String, String)],
        cancel: &CancelToken,
    ) -> Result<Vec<(String, std::path::PathBuf)>, OxidGeneError> {
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
            for (index, (key, archive_path)) in media_files.iter().enumerate() {
                cancel.check()?;
                let local_path = media_root.join(index.to_string());
                match self.media.get_to_file(key, &local_path).await {
                    Ok(()) => {
                        staged_media.push((archive_path.clone(), local_path));
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
    /// media, stopping at a cancellation between two media.
    async fn package_gedzip(
        &self,
        job_id: Uuid,
        gedcom: String,
        staged_media: Vec<(String, std::path::PathBuf)>,
        artifact_path: &Path,
        cancel: &CancelToken,
    ) -> Result<(), OxidGeneError> {
        let archive_path = artifact_path.to_path_buf();
        let cancel = cancel.clone();
        // The packaging span is opened here so the blocking thread runs
        // inside it rather than in a trace of its own.
        let package_span = tracing::info_span!("export.package", export.format = "gedzip");
        let archive_task = crate::service::blocking::spawn_in(package_span.clone(), move || {
            let mut writer =
                GedzipFileWriter::create(&archive_path, &gedcom).map_err(OxidGeneError::Gedcom)?;
            for (entry_path, local_path) in staged_media {
                cancel.check()?;
                let bytes = std::fs::read(local_path)?;
                writer
                    .add_media_file(&entry_path, &bytes)
                    .map_err(OxidGeneError::Gedcom)?;
            }
            writer.finish().map(drop).map_err(OxidGeneError::Gedcom)
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
        if let Some(job) = registry().running.get_mut(&job_id) {
            job.progress = LiveJobProgress {
                phase: phase.to_string(),
                done,
                total,
            };
        }
        job_watch::announce(job_id);
        // Under SQLite the running import holds the writer: the job's
        // progress, liveness and cancellation live in this process only,
        // and its long lease is not renewed (see `docs/architecture.md` §6).
        if self.db.get_database_backend() == DbBackend::Sqlite {
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
        match renewed {
            // Asked to stop through the database, by a server of another
            // process.
            Some(true) => {
                cancel_running_job(job_id);
                Ok(())
            }
            Some(false) => Ok(()),
            None => Err(OxidGeneError::Internal("background job lease lost".into())),
        }
    }

    fn progress_period(&self) -> Duration {
        progress_period(self.poll_interval, self.lease_duration)
    }
}

/// Run `parse`, a file reader, on the blocking pool under an `import.parse`
/// span of `format`.
///
/// Off the async workers, so a large file does not stall them, and behind a
/// task boundary: a reader that panics on a crafted file fails this job as
/// unreadable input instead of taking the worker loop down with it. (A
/// release build aborts on panic; [`MAX_ATTEMPTS`] covers that case.)
pub(crate) async fn parse_off_thread<F>(
    format: &'static str,
    parse: F,
) -> Result<oxidgene_gedcom::ImportResult, OxidGeneError>
where
    F: FnOnce() -> Result<oxidgene_gedcom::ImportResult, String> + Send + 'static,
{
    let span = tracing::info_span!("import.parse", import.format = format);
    match crate::service::blocking::spawn_in(span, parse).await {
        Ok(parsed) => parsed.map_err(OxidGeneError::Gedcom),
        Err(error) if error.is_panic() => {
            tracing::error!(
                error.category = "import_reader_panicked",
                import.format = format,
                "the file reader crashed; the import fails as unreadable input"
            );
            Err(OxidGeneError::Gedcom("the file could not be read".into()))
        }
        Err(_) => Err(OxidGeneError::Internal(
            "import parsing was cancelled".into(),
        )),
    }
}

/// Run the future `make` returns until it ends, starting a fresh one, after
/// `pause`, each time one panics.
///
/// The future runs on a task of its own, which is what lets its panic be
/// seen; the task is aborted when this future is dropped.
async fn supervise<F, Fut>(make: F, pause: Duration)
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    struct AbortOnDrop(tokio::task::JoinHandle<()>);
    impl Drop for AbortOnDrop {
        fn drop(&mut self) {
            self.0.abort();
        }
    }

    loop {
        let mut task = AbortOnDrop(tokio::spawn(make()));
        match (&mut task.0).await {
            Err(error) if error.is_panic() => {
                tracing::error!(
                    error.category = "worker_panicked",
                    "background job worker panicked; restarting it"
                );
                tokio::time::sleep(pause).await;
            }
            _ => return,
        }
    }
}

/// How a job that did not complete ends.
#[derive(Debug, Clone, Copy)]
enum Ending {
    /// It stopped at a cancellation request.
    Cancelled,
    /// It failed, with this stable error code.
    Failed(&'static str),
}

/// `outcome`, with `transaction` to go on with, or its error once
/// `transaction` is rolled back.
async fn rolled_back_on_error<T>(
    transaction: DatabaseTransaction,
    outcome: Result<T, OxidGeneError>,
) -> Result<(DatabaseTransaction, T), OxidGeneError> {
    match outcome {
        Ok(value) => Ok((transaction, value)),
        Err(error) => {
            if let Err(rollback) = transaction.rollback().await {
                // Dropped, the connection rolls back before its next use.
                tracing::warn!(
                    error.kind = error_kind(&db_err(rollback)),
                    "could not roll back an import"
                );
            }
            Err(error)
        }
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

/// Delete everything job `job_id` keeps in `media`: the media files it
/// stored that no row names (an import stopped before it could take them
/// back), then whatever lies under its `jobs/` prefix.
async fn delete_job_objects(
    db: &DatabaseConnection,
    media: &dyn MediaStore,
    job_id: Uuid,
) -> Result<(), OxidGeneError> {
    job_objects::discard_unreferenced(db, media, job_id).await?;
    media.delete_job(job_id).await
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
        delete_job_objects(db, media, *job_id).await?;
    }
    BackgroundJobRepo::delete_ended(db, &ended).await?;
    Ok(())
}

/// Delete the objects under `jobs/` that no job needs — its row gone, or
/// ended without an artifact to download — once they are older than
/// [`ORPHAN_GRACE`], measured by the job id's own time stamp, with the media
/// files they record an unfinished import stored. A crash between storing a
/// job's inputs and creating its row, or between ending it and removing
/// them, leaves such objects.
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
        delete_job_objects(db, media, job_id).await?;
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
async fn release_export_artifact(
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

/// When the artifact of an export finished at `finished_at` stops being
/// downloadable.
fn artifact_expiry(finished_at: chrono::DateTime<chrono::Utc>) -> chrono::DateTime<chrono::Utc> {
    finished_at + chrono::Duration::from_std(EXPORT_ARTIFACT_TTL).unwrap_or(chrono::TimeDelta::MAX)
}

/// Until when completed export `job` can be downloaded, or `None` when it
/// cannot (any more): no artifact, or one past [`EXPORT_ARTIFACT_TTL`] that
/// the maintenance pass has not deleted yet.
fn downloadable_until(job: &BackgroundJob) -> Option<chrono::DateTime<chrono::Utc>> {
    if job.status != BackgroundJobStatus::Completed.as_str() || job.artifact_key.is_none() {
        return None;
    }
    let expires_at = artifact_expiry(job.finished_at?);
    (chrono::Utc::now() < expires_at).then_some(expires_at)
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

    /// A temporary media store and an import job of `format` queued in
    /// `db`, its source stored; the store's directory and the job's id.
    async fn queued_import(
        db: &DatabaseConnection,
        format: &str,
    ) -> (tempfile::TempDir, Arc<dyn MediaStore>, Uuid) {
        let root = tempfile::tempdir().expect("creates media root");
        let media: Arc<dyn MediaStore> = Arc::new(crate::media::store::FsStore::new(root.path()));
        let tree_id = Uuid::now_v7();
        TreeRepo::create(db, tree_id, "Worker".into(), None)
            .await
            .expect("creates tree");
        let job_id = Uuid::now_v7();
        let source = tempfile::NamedTempFile::new().expect("creates source");
        std::fs::write(source.path(), b"0 HEAD\n0 TRLR\n").expect("writes source");
        let source_key = job_blob_key(job_id, "source", "ged").expect("job key");
        media
            .put_file(&source_key, source.path())
            .await
            .expect("stores source");
        BackgroundJobRepo::create(
            db,
            NewBackgroundJob {
                id: job_id,
                tree_id,
                kind: BackgroundJobKind::Import,
                format: format.to_string(),
                source_key: Some(source_key),
                payload_json: None,
                original_filename: None,
                include_notes_and_sources: true,
                include_media: true,
            },
        )
        .await
        .expect("queues job");
        (root, media, job_id)
    }

    async fn migrated_db() -> DatabaseConnection {
        let db = oxidgene_db::repo::connect("sqlite::memory:")
            .await
            .expect("connects");
        oxidgene_db::repo::run_migrations(&db)
            .await
            .expect("migrates");
        db
    }

    async fn job(db: &DatabaseConnection, job_id: Uuid) -> BackgroundJob {
        use sea_orm::EntityTrait as _;
        oxidgene_db::entities::background_job::Entity::find_by_id(job_id)
            .one(db)
            .await
            .expect("reads job")
            .expect("job exists")
    }

    #[tokio::test]
    async fn a_reader_that_panics_fails_its_job_once_and_for_all() {
        let db = migrated_db().await;
        let (_root, media, job_id) = queued_import(&db, "test-panic").await;
        let worker = BackgroundJobWorker::new(
            db.clone(),
            Arc::new(ProfileService::new(db.clone())),
            media,
            "test",
        );

        assert!(worker.run_once().await.expect("the iteration survives"));

        let failed = job(&db, job_id).await;
        assert_eq!(failed.status, "failed");
        assert_eq!(failed.error_code.as_deref(), Some("invalid_job_input"));
        // Not queued again: the next iteration finds nothing to do.
        assert!(!worker.run_once().await.expect("polls"));
    }

    #[tokio::test]
    async fn a_job_stopped_too_often_is_failed_without_running() {
        let db = migrated_db().await;
        let (_root, media, job_id) = queued_import(&db, "gedcom").await;
        // Each start claims it and dies mid-run, as a process aborting on a
        // crafted file would; the next start puts it back in the queue.
        for _ in 0..MAX_ATTEMPTS {
            BackgroundJobRepo::claim_next(&db, "crashed", chrono::Duration::hours(1))
                .await
                .expect("claims")
                .expect("a queued job");
            BackgroundJobRepo::requeue_running(&db)
                .await
                .expect("requeues");
        }
        let worker = BackgroundJobWorker::new(
            db.clone(),
            Arc::new(ProfileService::new(db.clone())),
            media,
            "test",
        );

        assert!(worker.run_once().await.expect("claims the job"));

        let failed = job(&db, job_id).await;
        assert_eq!(failed.status, "failed");
        assert_eq!(failed.error_code.as_deref(), Some("job_attempts_exhausted"));
        assert_eq!(failed.phase, "failed");
    }

    #[tokio::test]
    async fn a_panicking_loop_is_restarted() {
        let starts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = Arc::clone(&starts);
        tokio::time::timeout(
            Duration::from_secs(5),
            supervise(
                move || {
                    let start = counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    async move {
                        assert!(start > 0, "the first run panics");
                    }
                },
                Duration::from_millis(1),
            ),
        )
        .await
        .expect("the second run ends the supervision");
        assert_eq!(starts.load(std::sync::atomic::Ordering::SeqCst), 2);
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
            include_notes_and_sources: true,
            include_media: true,
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
            live_state(tree_id, job_id, BackgroundJobKind::Import),
            Some(LiveState::Running {
                progress: LiveJobProgress {
                    phase: "people".to_string(),
                    done: 100,
                    total: 250,
                },
                cancelling: false,
            })
        );
    }

    /// Under PostgreSQL a job's worker may run in another process: a
    /// cancellation reaches it through the database, and its progress
    /// wakes a waiting status read through a notification.
    ///
    /// `OXIDGENE_TEST_DATABASE_URL` must name an empty disposable database,
    /// which the test leaves empty.
    #[cfg(feature = "postgres")]
    #[tokio::test]
    #[ignore = "needs an empty disposable PostgreSQL database: run by `just test-postgres`"]
    async fn on_postgres_cancellation_and_progress_cross_processes() {
        let url = std::env::var("OXIDGENE_TEST_DATABASE_URL")
            .expect("set OXIDGENE_TEST_DATABASE_URL to an empty disposable PostgreSQL database");
        let db = oxidgene_db::repo::connect(&url).await.expect("connects");
        oxidgene_db::repo::run_migrations(&db)
            .await
            .expect("migrates");
        let listener = tokio::spawn(job_watch::listen(db.clone()));
        let root = tempfile::tempdir().expect("creates media root");
        let media: Arc<dyn MediaStore> = Arc::new(crate::media::store::FsStore::new(root.path()));
        let (tree_id, job_id) = queued_gedcom(&db, &*media, &families_gedcom(1)).await;
        // Another process's worker claims it.
        let lease = chrono::Duration::minutes(5);
        BackgroundJobRepo::claim_next(&db, "elsewhere", lease)
            .await
            .expect("claims")
            .expect("the queued job");
        let since = import_job_status(&db, tree_id, job_id)
            .await
            .expect("reads")
            .version;
        // Let the listener open its connection.
        tokio::time::sleep(Duration::from_millis(500)).await;

        let waiting = tokio::spawn({
            let db = db.clone();
            async move {
                let wait = JobWait {
                    since: Some(since),
                    wait: job_watch::MAX_WAIT,
                };
                wait_import_job_status(&db, tree_id, job_id, &wait).await
            }
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        BackgroundJobRepo::progress(&db, job_id, "elsewhere", "database", 1, 2, lease)
            .await
            .expect("reports progress");
        let woken = tokio::time::timeout(Duration::from_secs(10), waiting)
            .await
            .expect("the notification ends the wait")
            .unwrap()
            .expect("reads");
        assert_eq!((woken.phase.as_str(), woken.done), ("database", 1));

        let status = cancel_import_job(&db, &db, &*media, tree_id, job_id)
            .await
            .expect("cancels");
        assert_eq!(status.phase, "cancelling");
        assert_eq!(
            BackgroundJobRepo::progress(&db, job_id, "elsewhere", "database", 2, 2, lease)
                .await
                .expect("reports progress"),
            Some(true),
            "the worker reads the request with its progress"
        );

        listener.abort();
        oxidgene_db::repo::rollback_migrations(&db)
            .await
            .expect("leaves the database empty");
    }

    // ── Cancellation and all-or-nothing imports ──────────────────────────

    fn worker_over(db: &DatabaseConnection, media: &Arc<dyn MediaStore>) -> BackgroundJobWorker {
        BackgroundJobWorker::new(
            db.clone(),
            Arc::new(ProfileService::new(db.clone())),
            Arc::clone(media),
            "test",
        )
    }

    /// GEDCOM text of `families` couples, each with a child.
    fn families_gedcom(families: usize) -> String {
        let mut text = String::from("0 HEAD\n1 GEDC\n2 VERS 5.5.1\n1 CHAR UTF-8\n");
        for family in 0..families {
            for (role, sex) in [("H", "M"), ("W", "F"), ("C", "M")] {
                text.push_str(&format!(
                    "0 @I{role}{family}@ INDI\n1 NAME Person_{role} /BRANCH_{family}/\n1 SEX {sex}\n"
                ));
            }
            text.push_str(&format!(
                "0 @F{family}@ FAM\n1 HUSB @IH{family}@\n1 WIFE @IW{family}@\n1 CHIL @IC{family}@\n"
            ));
        }
        text.push_str("0 TRLR\n");
        text
    }

    /// A new tree of `db`, and an import of GEDCOM `text` queued into it,
    /// its source in `media`: the tree's id and the job's.
    async fn queued_gedcom(
        db: &DatabaseConnection,
        media: &dyn MediaStore,
        text: &str,
    ) -> (Uuid, Uuid) {
        let tree_id = Uuid::now_v7();
        TreeRepo::create(db, tree_id, "Cancellation".into(), None)
            .await
            .expect("creates tree");
        let source = tempfile::NamedTempFile::new().expect("creates source");
        std::fs::write(source.path(), text).expect("writes source");
        let job_id = Uuid::now_v7();
        stage_import(db, media, job_id, tree_id, "gedcom", None, source.path())
            .await
            .expect("queues the import");
        (tree_id, job_id)
    }

    /// How many rows tree `tree_id` holds of what an import writes: persons,
    /// media, projections and audit entries.
    async fn tree_rows(db: &DatabaseConnection, tree_id: Uuid) -> [u64; 4] {
        use oxidgene_db::entities::{audit_entry, media, person, person_denorm};
        use sea_orm::{ColumnTrait as _, EntityTrait as _, PaginatorTrait as _, QueryFilter as _};
        [
            person::Entity::find()
                .filter(person::Column::TreeId.eq(tree_id))
                .count(db)
                .await
                .expect("counts persons"),
            media::Entity::find()
                .filter(media::Column::TreeId.eq(tree_id))
                .count(db)
                .await
                .expect("counts media"),
            person_denorm::Entity::find()
                .filter(person_denorm::Column::TreeId.eq(tree_id))
                .count(db)
                .await
                .expect("counts projections"),
            audit_entry::Entity::find()
                .filter(audit_entry::Column::TreeId.eq(tree_id))
                .count(db)
                .await
                .expect("counts audit entries"),
        ]
    }

    /// Every file under `dir`, recursively.
    fn files_under(dir: &Path) -> Vec<std::path::PathBuf> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        entries
            .flatten()
            .flat_map(|entry| {
                let path = entry.path();
                if path.is_dir() {
                    files_under(&path)
                } else {
                    vec![path]
                }
            })
            .collect()
    }

    #[tokio::test]
    async fn a_queued_job_is_cancelled_at_once_and_never_runs() {
        let db = migrated_db().await;
        let root = tempfile::tempdir().expect("creates media root");
        let media: Arc<dyn MediaStore> = Arc::new(crate::media::store::FsStore::new(root.path()));
        let (tree_id, job_id) = queued_gedcom(&db, &*media, &families_gedcom(2)).await;

        let status = cancel_import_job(&db, &db, &*media, tree_id, job_id)
            .await
            .expect("cancels");
        assert_eq!(status.phase, "cancelled");
        assert_eq!(
            crate::service::tree::active_import(&db, tree_id)
                .await
                .expect("reads the active import"),
            None,
            "a cancelled job no longer holds its tree"
        );

        // The worker writes the cancellation, then finds nothing to run.
        let worker = worker_over(&db, &media);
        assert!(!worker.run_once().await.expect("polls"));
        let cancelled = job(&db, job_id).await;
        assert_eq!(
            (cancelled.status.as_str(), cancelled.phase.as_str()),
            ("cancelled", "cancelled")
        );
        assert!(cancelled.active_tree_id.is_none());
        assert!(
            files_under(&root.path().join("jobs")).is_empty(),
            "its source is gone"
        );
        assert_eq!(tree_rows(&db, tree_id).await, [0; 4]);
        assert_eq!(
            import_job_status(&db, tree_id, job_id)
                .await
                .expect("reads")
                .phase,
            "cancelled"
        );
    }

    #[tokio::test]
    async fn an_import_cancelled_between_two_batches_of_persons_leaves_the_tree_as_it_was() {
        let db = migrated_db().await;
        let root = tempfile::tempdir().expect("creates media root");
        let media: Arc<dyn MediaStore> = Arc::new(crate::media::store::FsStore::new(root.path()));
        // Three batches of persons.
        let (tree_id, job_id) = queued_gedcom(&db, &*media, &families_gedcom(400)).await;
        plan_stop(job_id, "Database", 2, false);

        assert!(worker_over(&db, &media).run_once().await.expect("runs"));

        let cancelled = job(&db, job_id).await;
        assert_eq!(
            (cancelled.status.as_str(), cancelled.error_code.as_deref()),
            ("cancelled", None)
        );
        assert_eq!(
            tree_rows(&db, tree_id).await,
            [0; 4],
            "nothing of it is left"
        );
        assert!(files_under(root.path()).is_empty());

        // The same file imports whole when nobody stops it.
        let (tree_id, _) = queued_gedcom(&db, &*media, &families_gedcom(400)).await;
        assert!(worker_over(&db, &media).run_once().await.expect("runs"));
        assert_eq!(tree_rows(&db, tree_id).await, [1200, 0, 1200, 1]);
    }

    /// A Geneanet import queued into a new tree of `db`: a couple, and a
    /// document of two pages naming one of them, its pages fetched into
    /// `scratch`. The tree's id and the job's.
    async fn queued_geneanet(
        db: &DatabaseConnection,
        media: &dyn MediaStore,
        scratch: &Path,
    ) -> (Uuid, Uuid) {
        let tree_id = Uuid::now_v7();
        TreeRepo::create(db, tree_id, "Geneanet".into(), None)
            .await
            .expect("creates tree");
        let mut fetched = HashMap::new();
        for page in 1..=2u32 {
            let path = scratch.join(format!("page{page}.png"));
            let mut bytes = std::io::Cursor::new(Vec::new());
            image::RgbImage::from_pixel(40 + page, 30, image::Rgb([page as u8 * 60, 90, 120]))
                .write_to(&mut bytes, image::ImageFormat::Png)
                .expect("encodes a page");
            std::fs::write(&path, bytes.into_inner()).expect("writes a page");
            fetched.insert(
                format!("https://example.invalid/deposit/page{page}.png"),
                path.to_string_lossy().into_owned(),
            );
        }
        let collection = serde_json::json!({
            "deposits": [{
                "id": 1, "title": "t", "type": "civil", "private": true,
                "views": [
                    {"id": 10, "page": 1, "files": {"normal": "https://example.invalid/deposit/page1.png"}},
                    {"id": 11, "page": 2, "files": {"normal": "https://example.invalid/deposit/page2.png"}},
                ],
            }],
            "references": [],
            "view_references": {"1:10": [{
                "lastname": "BRANCH_A", "firstname": "person_a",
                "reference_extra_geneweb": {"ref": "branch a|person a|"},
            }]},
        })
        .to_string();
        let job_id = stage_geneanet_import(
            db,
            media,
            &WorkDir::temporary(),
            tree_id,
            b"encoding: utf-8\n\nfam BRANCH_A person_a.0 3/4/1900 + BRANCH_B person_b.0\n",
            "family.gw".into(),
            collection,
            HashMap::new(),
            &[],
            &fetched,
            geneanet::MediaFidelity::Renditions,
        )
        .await
        .expect("queues the import");
        (tree_id, job_id)
    }

    #[tokio::test]
    async fn a_geneanet_import_cancelled_in_its_media_takes_back_what_it_stored() {
        let db = migrated_db().await;
        let root = tempfile::tempdir().expect("creates media root");
        let scratch = tempfile::tempdir().expect("creates scratch");
        let media: Arc<dyn MediaStore> = Arc::new(crate::media::store::FsStore::new(root.path()));
        let (tree_id, job_id) = queued_geneanet(&db, &*media, scratch.path()).await;
        // Past the document's first page, stored and written; at its second,
        // stored too.
        plan_stop(job_id, "Media", 3, false);

        assert!(worker_over(&db, &media).run_once().await.expect("runs"));

        assert_eq!(job(&db, job_id).await.status, "cancelled");
        assert_eq!(tree_rows(&db, tree_id).await, [0; 4], "no row is left");
        assert!(
            files_under(root.path()).is_empty(),
            "every object it stored is deleted: {:?}",
            files_under(root.path())
        );
    }

    #[tokio::test]
    async fn a_geneanet_import_failing_in_its_media_rolls_back_whole() {
        let db = migrated_db().await;
        let root = tempfile::tempdir().expect("creates media root");
        let scratch = tempfile::tempdir().expect("creates scratch");
        let media: Arc<dyn MediaStore> = Arc::new(crate::media::store::FsStore::new(root.path()));
        let (tree_id, job_id) = queued_geneanet(&db, &*media, scratch.path()).await;
        plan_stop(job_id, "Media", 3, true);

        assert!(worker_over(&db, &media).run_once().await.expect("runs"));

        let failed = job(&db, job_id).await;
        assert_eq!(
            (failed.status.as_str(), failed.error_code.as_deref()),
            ("failed", Some("job_failed"))
        );
        assert_eq!(tree_rows(&db, tree_id).await, [0; 4]);
        assert!(
            files_under(root.path()).is_empty(),
            "{:?}",
            files_under(root.path())
        );
    }

    #[tokio::test]
    async fn a_completed_geneanet_import_keeps_its_media_and_no_records() {
        let db = migrated_db().await;
        let root = tempfile::tempdir().expect("creates media root");
        let scratch = tempfile::tempdir().expect("creates scratch");
        let media: Arc<dyn MediaStore> = Arc::new(crate::media::store::FsStore::new(root.path()));
        let (tree_id, job_id) = queued_geneanet(&db, &*media, scratch.path()).await;

        assert!(worker_over(&db, &media).run_once().await.expect("runs"));

        assert_eq!(job(&db, job_id).await.status, "completed");
        let [persons, media_rows, _, audit] = tree_rows(&db, tree_id).await;
        assert_eq!(
            (persons, media_rows, audit),
            (2, 3, 1),
            "a document and its pages"
        );
        assert!(
            files_under(&root.path().join("jobs")).is_empty(),
            "no record is left"
        );
        // Every stored file is named by a row: the pages and their thumbnails.
        use sea_orm::{ColumnTrait as _, EntityTrait as _, QueryFilter as _};
        let mut named: Vec<String> = oxidgene_db::entities::media::Entity::find()
            .filter(oxidgene_db::entities::media::Column::TreeId.eq(tree_id))
            .all(&db)
            .await
            .expect("lists media")
            .into_iter()
            .flat_map(|row| [row.storage_key, row.thumbnail_key])
            .flatten()
            .collect();
        named.sort();
        named.dedup();
        let mut stored: Vec<String> = files_under(root.path())
            .iter()
            .map(|path| {
                path.strip_prefix(root.path())
                    .expect("under the root")
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        stored.sort();
        assert!(named.len() >= 2, "both pages are stored");
        assert_eq!(stored, named);
    }

    #[tokio::test]
    async fn an_earlier_attempts_media_go_before_the_import_runs_again() {
        let db = migrated_db().await;
        let root = tempfile::tempdir().expect("creates media root");
        let media: Arc<dyn MediaStore> = Arc::new(crate::media::store::FsStore::new(root.path()));
        let (tree_id, job_id) = queued_gedcom(&db, &*media, &families_gedcom(1)).await;
        // A file an attempt stored and recorded before its process stopped.
        let recording = RecordingStore::new(&*media, job_id);
        let orphan = recording
            .put(tree_id, "png", b"an attempt's page")
            .await
            .expect("stores");

        assert!(worker_over(&db, &media).run_once().await.expect("runs"));

        assert_eq!(job(&db, job_id).await.status, "completed");
        assert!(!media.exists(&orphan.key).await, "nothing names it");
        assert!(files_under(root.path()).is_empty());
    }

    #[tokio::test]
    async fn a_cancellation_reaches_a_running_job_in_memory_on_sqlite() {
        let mut options = ConnectOptions::new("sqlite::memory:");
        options.max_connections(1);
        let db = Database::connect(options).await.expect("connects");
        let media_root = tempfile::tempdir().expect("creates media root");
        let media: Arc<dyn MediaStore> =
            Arc::new(crate::media::store::FsStore::new(media_root.path()));
        let (job_id, tree_id) = (Uuid::now_v7(), Uuid::now_v7());
        let live_job = LiveJobGuard::for_test(job_id, tree_id, BackgroundJobKind::Import);
        // As the running import holds it for its whole transaction.
        let _transaction = db.begin().await.expect("holds the only connection");

        let status = tokio::time::timeout(
            Duration::from_millis(500),
            cancel_import_job(&db, &db, &*media, tree_id, job_id),
        )
        .await
        .expect("the request waits for nothing")
        .expect("cancels");

        assert_eq!(status.phase, "cancelling");
        assert!(live_job.cancel_token().is_cancelled());
        assert_eq!(
            crate::service::background_job::active_job_cancelling(job_id, false),
            Some(true)
        );
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

fn geneanet_payload(job: &BackgroundJob) -> Result<GeneanetJobPayload, OxidGeneError> {
    let payload = job
        .payload_json
        .as_deref()
        .ok_or_else(|| OxidGeneError::Validation("Geneanet import job has no payload".into()))?;
    serde_json::from_str(payload).map_err(|error| OxidGeneError::Validation(error.to_string()))
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

/// Put an uploaded genealogy file into job storage and queue its import job.
///
/// The staged source is deleted again when the job row cannot be written, so
/// a failed start leaves nothing behind.
pub async fn stage_import(
    db: &impl sea_orm::ConnectionTrait,
    media: &dyn MediaStore,
    job_id: Uuid,
    tree_id: Uuid,
    format: &str,
    filename: Option<String>,
    path: &Path,
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
            include_notes_and_sources: true,
            include_media: true,
        },
    )
    .await;
    if let Err(error) = created {
        let _ = media.delete(&source_key).await;
        return Err(error);
    }
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "the Geneanet import threads borrowed state of differently-owned sources (store, archives, manifest, indexes) through each step"
)]
pub async fn stage_geneanet_import(
    db: &DatabaseConnection,
    media: &dyn MediaStore,
    work_dir: &WorkDir,
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
    let scratch = work_dir.job_scratch(job_id)?;
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
                include_notes_and_sources: true,
                include_media: true,
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

// ── Job status, as both surfaces report it ─────────────────────────────

/// Where an export job stands.
#[derive(Debug, Clone, Serialize)]
pub struct ExportJobStatus {
    pub phase: String,
    /// Identifies this state, to wait for the next one with: see
    /// [`job_watch`].
    pub version: String,
    pub done: i64,
    pub total: i64,
    /// Where to download the archive, once it is complete and until
    /// `expires_at`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_url: Option<String>,
    /// When the archive stops being downloadable: [`EXPORT_ARTIFACT_TTL`]
    /// after the export completed, whatever the downloads meanwhile. Set
    /// exactly when `download_url` is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    /// The archive's size in bytes, once the export has completed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<i64>,
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
    /// Identifies this state, to wait for the next one with: see
    /// [`job_watch`].
    pub version: String,
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

/// What a GEDZIP export holds. It has no merge: a GEDZIP is the complete
/// export, and the merges serve the importers of a plain GEDCOM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArchiveChoices {
    /// Write the notes, and the sources with their citations and
    /// repositories.
    pub include_notes_and_sources: bool,
    /// Pack the media, and write their records, links and identifications.
    pub include_media: bool,
}

impl ArchiveChoices {
    /// The choices a request's options make, REST's and GraphQL's alike: an
    /// option left out keeps the complete export.
    #[must_use]
    pub fn from_options(
        include_notes_and_sources: Option<bool>,
        include_media: Option<bool>,
    ) -> Self {
        Self {
            include_notes_and_sources: include_notes_and_sources.unwrap_or(true),
            include_media: include_media.unwrap_or(true),
        }
    }
}

/// Queue a GEDZIP export of tree `tree_id` made as `choices` say; the job's
/// id. The job keeps the choices until its worker runs it.
///
/// A tree runs one job at a time: while another is queued or running, this
/// is a [`OxidGeneError::Conflict`].
pub async fn start_export_job(
    db: &DatabaseConnection,
    tree_id: Uuid,
    choices: ArchiveChoices,
    scope: Option<&TagTargetSpec>,
) -> Result<Uuid, OxidGeneError> {
    TreeRepo::get(db, tree_id).await?;
    if let Some(spec) = scope {
        // Refused now rather than by the worker: a scope holding no person.
        tag_bulk::export_persons(db, tree_id, spec).await?;
    }
    let job_id = Uuid::now_v7();
    let ArchiveChoices {
        include_notes_and_sources,
        include_media,
    } = choices;
    BackgroundJobRepo::create(
        db,
        NewBackgroundJob {
            id: job_id,
            tree_id,
            kind: BackgroundJobKind::Export,
            format: "gedzip".into(),
            source_key: None,
            payload_json: scope
                .map(serde_json::to_string)
                .transpose()
                .map_err(|error| OxidGeneError::Internal(error.to_string()))?,
            original_filename: None,
            include_notes_and_sources,
            include_media,
        },
    )
    .await?;
    Ok(job_id)
}

/// The scope export job `job` was queued with, `None` for the whole tree.
fn scope_of(job: &BackgroundJob) -> Result<Option<TagTargetSpec>, OxidGeneError> {
    job.payload_json
        .as_deref()
        .map(|text| {
            serde_json::from_str(text).map_err(|error| OxidGeneError::Internal(error.to_string()))
        })
        .transpose()
}

/// The choices export job `job` was queued with.
fn choices_of(job: &BackgroundJob) -> gedcom::ExportChoices {
    gedcom::ExportChoices {
        include_notes_and_sources: job.include_notes_and_sources,
        include_media: job.include_media,
        ..gedcom::ExportChoices::default()
    }
}

/// Where export job `job_id` of tree `tree_id` stands.
pub async fn export_job_status(
    db: &DatabaseConnection,
    tree_id: Uuid,
    job_id: Uuid,
) -> Result<ExportJobStatus, OxidGeneError> {
    if let Some(live) = live_state(tree_id, job_id, BackgroundJobKind::Export) {
        let (done, total) = live.counts();
        let phase = live.phase();
        return Ok(ExportJobStatus {
            version: version_of(&phase, done, total),
            phase,
            done,
            total,
            download_url: None,
            expires_at: None,
            size_bytes: None,
            warnings: Vec::new(),
            error: None,
        });
    }
    let job = job_of_kind(db, tree_id, job_id, BackgroundJobKind::Export).await?;
    let (warnings, size_bytes) = receipt::<ExportReceipt>(job.result_json.as_deref())?
        .map_or((Vec::new(), None), |receipt| {
            (receipt.warnings, receipt.size_bytes)
        });
    // An expired artifact is gone, or about to be, and so is its link.
    let expires_at = downloadable_until(&job);
    let download_url =
        expires_at.map(|_| format!("/api/v1/trees/{tree_id}/export-jobs/{job_id}/download"));
    let (phase, done, total) = (reported_phase(&job), count(job.done), count(job.total));
    Ok(ExportJobStatus {
        version: version_of(&phase, done, total),
        phase,
        done,
        total,
        download_url,
        expires_at,
        size_bytes,
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
    if let Some(live) = live_state(tree_id, job_id, BackgroundJobKind::Import) {
        let (done, total) = live.counts();
        let phase = live.phase();
        return Ok(ImportJobStatus {
            version: version_of(&phase, done, total),
            phase,
            done,
            total,
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
    let (phase, done, total) = (reported_phase(&job), count(job.done), count(job.total));
    Ok(ImportJobStatus {
        version: version_of(&phase, done, total),
        phase,
        done,
        total,
        result,
        geneanet_result,
        error: job.error_code,
    })
}

/// The phase a status read reports for `job`, read from the database:
/// `cancelling` once a running job was asked to stop.
fn reported_phase(job: &BackgroundJob) -> String {
    if job.cancel_requested && job.status == BackgroundJobStatus::Running.as_str() {
        PHASE_CANCELLING.to_string()
    } else {
        job.phase.clone()
    }
}

/// Cancel import job `job_id` of tree `tree_id`: a queued job ends at once,
/// a running one stops at its next unit of work and rolls back. Where it
/// stands then.
pub async fn cancel_import_job(
    writer: &DatabaseConnection,
    reader: &DatabaseConnection,
    media: &dyn MediaStore,
    tree_id: Uuid,
    job_id: Uuid,
) -> Result<ImportJobStatus, OxidGeneError> {
    cancel_job(
        writer,
        reader,
        media,
        tree_id,
        job_id,
        BackgroundJobKind::Import,
    )
    .await?;
    import_job_status(reader, tree_id, job_id).await
}

/// Cancel export job `job_id` of tree `tree_id`: a queued job ends at once,
/// a running one stops at its next medium. Where it stands then.
pub async fn cancel_export_job(
    writer: &DatabaseConnection,
    reader: &DatabaseConnection,
    media: &dyn MediaStore,
    tree_id: Uuid,
    job_id: Uuid,
) -> Result<ExportJobStatus, OxidGeneError> {
    cancel_job(
        writer,
        reader,
        media,
        tree_id,
        job_id,
        BackgroundJobKind::Export,
    )
    .await?;
    export_job_status(reader, tree_id, job_id).await
}

/// Ask job `job_id` of `kind` in tree `tree_id` to stop.
///
/// A queued job ends at once, cancelled, unrun. A running job stops at its
/// next unit of work — a batch of rows, a medium — rolls back everything it
/// wrote and ends cancelled; its status reads `cancelling` meanwhile. A job
/// that has ended already is left as it is.
///
/// The request waits for nothing the job holds. A job this process runs
/// gets the request in memory. Under SQLite, whose only writer the running
/// import holds, a request for any other job is noted in memory too, and
/// the worker writes it once the writer is free — the statuses and the tree
/// list report it meanwhile. Under PostgreSQL the request goes to the
/// database, where the worker of another process reads it with its next
/// progress report.
async fn cancel_job(
    writer: &DatabaseConnection,
    reader: &DatabaseConnection,
    media: &dyn MediaStore,
    tree_id: Uuid,
    job_id: Uuid,
    kind: BackgroundJobKind,
) -> Result<(), OxidGeneError> {
    if cancel_here(tree_id, job_id, kind, None) {
        return Ok(());
    }
    let job = job_of_kind(reader, tree_id, job_id, kind).await?;
    let queued = job.status == BackgroundJobStatus::Queued.as_str();
    if !queued && job.status != BackgroundJobStatus::Running.as_str() {
        return Ok(());
    }
    if writer.get_database_backend() == DbBackend::Sqlite {
        cancel_here(tree_id, job_id, kind, Some(queued));
        return Ok(());
    }
    if queued && BackgroundJobRepo::cancel_queued(writer, job_id).await? {
        if let Err(error) = media.delete_job(job_id).await {
            tracing::warn!(
                job_id = %job_id,
                error.kind = error_kind(&error),
                "could not delete the inputs of a cancelled job"
            );
        }
    } else {
        // Running, or claimed since it was read.
        BackgroundJobRepo::request_cancel(writer, job_id).await?;
    }
    job_watch::announce(job_id);
    Ok(())
}

/// Where import job `job_id` of tree `tree_id` stands, once it differs from
/// what `wait` last read or `wait`'s time has run out (see [`job_watch`]).
pub async fn wait_import_job_status(
    db: &DatabaseConnection,
    tree_id: Uuid,
    job_id: Uuid,
    wait: &JobWait,
) -> Result<ImportJobStatus, OxidGeneError> {
    job_watch::wait_for(job_id, wait, || import_job_status(db, tree_id, job_id)).await
}

/// Where export job `job_id` of tree `tree_id` stands, once it differs from
/// what `wait` last read or `wait`'s time has run out (see [`job_watch`]).
pub async fn wait_export_job_status(
    db: &DatabaseConnection,
    tree_id: Uuid,
    job_id: Uuid,
    wait: &JobWait,
) -> Result<ExportJobStatus, OxidGeneError> {
    job_watch::wait_for(job_id, wait, || export_job_status(db, tree_id, job_id)).await
}

/// What identifies a job's visible state: its phase and counts. Everything
/// else a status carries changes only with its phase.
fn version_of(phase: &str, done: i64, total: i64) -> String {
    format!("{phase}.{done}.{total}")
}

/// Whether a job in `phase` has ended.
fn has_ended(phase: &str) -> bool {
    [
        BackgroundJobStatus::Completed,
        BackgroundJobStatus::Failed,
        BackgroundJobStatus::Cancelled,
    ]
    .iter()
    .any(|status| status.as_str() == phase)
}

impl Versioned for ImportJobStatus {
    fn version(&self) -> &str {
        &self.version
    }

    fn has_ended(&self) -> bool {
        has_ended(&self.phase)
    }
}

impl Versioned for ExportJobStatus {
    fn version(&self) -> &str {
        &self.version
    }

    fn has_ended(&self) -> bool {
        has_ended(&self.phase)
    }
}

/// A completed export of a tree whose archive can still be downloaded.
#[derive(Debug, Clone, Serialize)]
pub struct DownloadableExport {
    pub job_id: Uuid,
    /// The archive's format: `gedzip`, the only one exports run as jobs.
    pub format: String,
    /// Where to download the archive, until `expires_at`.
    pub download_url: String,
    /// When the archive stops being downloadable: [`EXPORT_ARTIFACT_TTL`]
    /// after the export completed.
    pub expires_at: chrono::DateTime<chrono::Utc>,
    /// The archive's size in bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<i64>,
    pub include_notes_and_sources: bool,
    pub include_media: bool,
}

/// Tree `tree_id`'s most recent completed export whose archive can still be
/// downloaded: not past [`EXPORT_ARTIFACT_TTL`], and still in `media`.
/// `None` when there is none, which is no error: a page asks on load whether
/// there is one to offer again.
pub async fn downloadable_export(
    db: &DatabaseConnection,
    media: &dyn MediaStore,
    tree_id: Uuid,
) -> Result<Option<DownloadableExport>, OxidGeneError> {
    crate::service::scope::require_live_tree(db, tree_id).await?;
    let since = chrono::Utc::now()
        - chrono::Duration::from_std(EXPORT_ARTIFACT_TTL).unwrap_or(chrono::TimeDelta::MAX);
    let Some(job) = BackgroundJobRepo::latest_artifact_in_tree(db, tree_id, since).await? else {
        return Ok(None);
    };
    let (Some(expires_at), Some(key)) = (downloadable_until(&job), job.artifact_key.as_deref())
    else {
        return Ok(None);
    };
    if !media.exists(key).await {
        return Ok(None);
    }
    let size_bytes = receipt::<ExportReceipt>(job.result_json.as_deref())?
        .and_then(|receipt| receipt.size_bytes);
    Ok(Some(DownloadableExport {
        job_id: job.id,
        download_url: format!("/api/v1/trees/{tree_id}/export-jobs/{}/download", job.id),
        format: job.format,
        expires_at,
        size_bytes,
        include_notes_and_sources: job.include_notes_and_sources,
        include_media: job.include_media,
    }))
}

/// The export artifact of completed job `job_id` of tree `tree_id`: its
/// storage key. It can be downloaded any number of times until
/// [`EXPORT_ARTIFACT_TTL`] after the export completed; past it, it is not
/// found, whether or not the maintenance pass has deleted it yet.
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
    let not_found = OxidGeneError::NotFound {
        entity: "ExportArtifact",
        id: job_id,
    };
    if downloadable_until(&job).is_none() {
        return Err(not_found);
    }
    job.artifact_key.ok_or(not_found)
}

/// Job `job_id` of tree `tree_id`, which must be of `kind` and belong to a
/// live tree.
///
/// The status reads answer a job this process is running from memory before
/// coming here, so that a poll never waits on the database — the running job
/// holds the SQLite writer, which on an in-memory database is also the only
/// reader. The tree is checked here, once
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
#[derive(Deserialize, Serialize)]
struct ExportReceipt {
    warnings: Vec<String>,
    /// The archive's size in bytes.
    #[serde(default)]
    size_bytes: Option<i64>,
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
