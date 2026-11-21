//! Snapshots of the desktop's database file, the check of its integrity,
//! and what the application answers while the file cannot be used
//! (docs/architecture.md §8.4).
//!
//! `oxidgene.db` is the only copy of the user's genealogy. Each time the
//! application starts, the file is checked (`PRAGMA quick_check`, `PRAGMA
//! foreign_key_check`), and once a day a consistent copy of it is then
//! written beside the application's state — `snapshots/oxidgene-<UTC
//! time>.db` — the three most recent kept; a copy is also taken before a
//! pending migration changes the file. A file that fails its check is not
//! copied — its copies would push the sound ones out — and the application
//! stops writing to it ([`DatabaseHealth`]). The media files are not
//! copied: they are written once and never changed, and the user's own
//! backup of the data directory covers them.
//!
//! The check and the copy run on a connection of their own, off the request
//! path: they read the last committed state under a read transaction, which
//! in write-ahead-log mode blocks neither the writer nor the readers.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, SystemTime};

use axum::http::{Method, StatusCode};
use axum::response::Response;
use chrono::{DateTime, NaiveDateTime, Utc};
use oxidgene_core::OxidGeneError;
use oxidgene_db::repo::{IntegrityReport, check_integrity, connect, db_err, snapshot_into};
use tracing::{error, info, warn};

use crate::access::refusal;
use crate::authz::Requirement;

/// The prefix and suffix of a snapshot's file name, around its UTC time.
const PREFIX: &str = "oxidgene-";
const SUFFIX: &str = ".db";
/// The time in a snapshot's name: sortable, and safe in a file name.
const TIME_FORMAT: &str = "%Y%m%dT%H%M%SZ";

/// Where the snapshots of a database file go, how many are kept and how
/// often one is taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotPolicy {
    pub directory: PathBuf,
    /// The most recent snapshots kept.
    pub keep: usize,
    /// No snapshot is taken while the most recent is younger.
    pub every: Duration,
}

impl SnapshotPolicy {
    /// The desktop's policy: the last three, at most one a day.
    #[must_use]
    pub fn daily(directory: PathBuf) -> Self {
        Self {
            directory,
            keep: 3,
            every: Duration::from_secs(24 * 60 * 60),
        }
    }
}

/// How an attempt to take a snapshot ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotOutcome {
    /// A snapshot was written there.
    Taken(PathBuf),
    /// The most recent snapshot is recent enough.
    NotDue,
    /// The disk lacks the room for a copy of the file.
    NoRoom,
    /// The file failed its integrity check and was not copied.
    Damaged(IntegrityReport),
}

/// Whether the database file can be used, shared between what finds out —
/// the start, the daily check — and the route guard that answers for it
/// (`rest::authorize`).
#[derive(Debug, Clone, Default)]
pub struct DatabaseHealth(Arc<AtomicU8>);

impl DatabaseHealth {
    const SOUND: u8 = 0;
    const DAMAGED: u8 = 1;
    const INCOMPATIBLE: u8 = 2;
    const CLOSED: u8 = 3;

    /// A file whose schema is not this build's.
    #[must_use]
    pub fn incompatible() -> Self {
        Self(Arc::new(AtomicU8::new(Self::INCOMPATIBLE)))
    }

    /// A file that failed its integrity check before a migration it needed:
    /// it was not migrated, and so not opened.
    #[must_use]
    pub fn closed() -> Self {
        Self(Arc::new(AtomicU8::new(Self::CLOSED)))
    }

    /// Record that the file failed its integrity check.
    pub fn mark_damaged(&self) {
        // A file that was never opened stays so.
        let _ = self.0.compare_exchange(
            Self::SOUND,
            Self::DAMAGED,
            Ordering::Relaxed,
            Ordering::Relaxed,
        );
    }

    /// Whether the file failed its integrity check.
    #[must_use]
    pub fn is_damaged(&self) -> bool {
        matches!(self.0.load(Ordering::Relaxed), Self::DAMAGED | Self::CLOSED)
    }

    /// Whether the file's schema is not this build's.
    #[must_use]
    pub fn is_incompatible(&self) -> bool {
        self.0.load(Ordering::Relaxed) == Self::INCOMPATIBLE
    }

    /// What the operation `method` needing `requirement` is answered
    /// instead of being served, if the file cannot serve it: `503
    /// database_incompatible` to everything of a file with another schema,
    /// and `503 database_damaged` to what changes a damaged one. A damaged
    /// file still answers what only reads — every `GET`, the reads sent as
    /// `POST`, an export — so that what it holds can be looked at and
    /// exported while a snapshot is restored; one that was not opened
    /// answers nothing.
    pub(crate) fn refusal(&self, method: &Method, requirement: Requirement) -> Option<Response> {
        if self.is_incompatible() {
            return Some(refusal(
                StatusCode::SERVICE_UNAVAILABLE,
                "database_incompatible",
                "The database was created by another version of OxidGene; \
                 recreate it and import the genealogy again",
            ));
        }
        let opened = self.0.load(Ordering::Relaxed) != Self::CLOSED;
        let reads = opened
            && (matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
                || reads_only(requirement));
        (self.is_damaged() && !reads).then(|| {
            refusal(
                StatusCode::SERVICE_UNAVAILABLE,
                "database_damaged",
                "The database file is damaged; changes are refused until a \
                 snapshot is restored",
            )
        })
    }
}

/// Whether an operation needing `requirement` changes nothing a tree
/// holds: one open to all, or needing a permission a token limited to
/// reading, to the history or to exports may use.
fn reads_only(requirement: Requirement) -> bool {
    match requirement {
        Requirement::Open => true,
        Requirement::Tree(permission) | Requirement::Trees(permission) => matches!(
            permission.oauth_scope(),
            "tree:read" | "history:read" | "export"
        ),
        Requirement::SignedIn | Requirement::TreeCreation | Requirement::Administrator => false,
    }
}

/// The snapshots of `directory`, oldest first, with the time each was
/// taken. Files that are not snapshots are left alone.
#[must_use]
pub fn snapshots(directory: &Path) -> Vec<(DateTime<Utc>, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut found: Vec<_> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let time = name.strip_prefix(PREFIX)?.strip_suffix(SUFFIX)?;
            let taken = NaiveDateTime::parse_from_str(time, TIME_FORMAT).ok()?;
            Some((taken.and_utc(), entry.path()))
        })
        .collect();
    found.sort();
    found
}

/// The free space of the file system holding `directory`, in bytes; `None`
/// where the platform gives no portable way to ask, and the copy's own
/// failure stands for the check.
#[cfg(unix)]
fn free_space(directory: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt as _;

    let path = std::ffi::CString::new(directory.as_os_str().as_bytes()).ok()?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: `path` is a NUL-terminated string that outlives the call, and
    // `stats` points to writable memory of the size `statvfs` fills; it is
    // read only after the call reported success.
    let filled = unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) } == 0;
    if !filled {
        return None;
    }
    // SAFETY: `statvfs` returned 0, so it initialised the structure.
    let stats = unsafe { stats.assume_init() };
    Some(widen(stats.f_bavail).saturating_mul(widen(stats.f_frsize)))
}

/// A `statvfs` count as `u64`, whatever its width on the platform: 32 bits
/// for `f_bavail` on macOS, 64 on Linux.
#[cfg(unix)]
fn widen<T: Into<u64>>(count: T) -> u64 {
    count.into()
}

#[cfg(not(unix))]
fn free_space(_directory: &Path) -> Option<u64> {
    None
}

/// The size of the database file with its write-ahead log: what a copy may
/// take at most.
fn database_size(database: &Path) -> u64 {
    let size = |path: &Path| std::fs::metadata(path).map_or(0, |metadata| metadata.len());
    let mut log = database.as_os_str().to_owned();
    log.push("-wal");
    size(database).saturating_add(size(Path::new(&log)))
}

/// Whether `free` bytes are room for a copy of `needed` bytes, with a
/// tenth to spare: a copy that fills the disk would fail the next write of
/// the application itself.
fn is_room(free: Option<u64>, needed: u64) -> bool {
    free.is_none_or(|free| free >= needed.saturating_add(needed / 10))
}

/// Check the integrity of the SQLite file `database`, then, if it is sound
/// and `policy` says a snapshot is due at `now`, take one, and delete the
/// snapshots beyond those it keeps.
///
/// # Errors
///
/// A database or file-system failure. A file that is damaged, a disk
/// without room and a snapshot that is not due are outcomes, not errors.
pub async fn snapshot_if_due(
    database: &Path,
    policy: &SnapshotPolicy,
    now: DateTime<Utc>,
) -> Result<SnapshotOutcome, OxidGeneError> {
    let due = snapshots(&policy.directory)
        .last()
        .is_none_or(|(taken, _)| {
            now.signed_duration_since(*taken)
                .to_std()
                .is_ok_and(|age| age >= policy.every)
        });
    checked_copy(database, policy, now, due).await
}

/// Take a snapshot of `database` now, whatever the age of the last one:
/// what a pending migration asks for before it changes the file.
///
/// # Errors
///
/// As [`snapshot_if_due`].
pub async fn take_snapshot(
    database: &Path,
    policy: &SnapshotPolicy,
    now: DateTime<Utc>,
) -> Result<SnapshotOutcome, OxidGeneError> {
    checked_copy(database, policy, now, true).await
}

/// Check `database`, then copy it if the copy is `due`, on a connection of
/// its own, apart from the application's writer.
async fn checked_copy(
    database: &Path,
    policy: &SnapshotPolicy,
    now: DateTime<Utc>,
    due: bool,
) -> Result<SnapshotOutcome, OxidGeneError> {
    let source = connect(&format!("sqlite://{}?mode=rw", database.display()))
        .await
        .map_err(db_err)?;
    let outcome = copy_checked(&source, database, policy, now, due).await;
    let _ = source.close().await;
    let outcome = outcome?;
    if matches!(outcome, SnapshotOutcome::Taken(_)) {
        for (_, old) in snapshots(&policy.directory).iter().rev().skip(policy.keep) {
            let _ = std::fs::remove_file(old);
        }
    }
    Ok(outcome)
}

/// Check the file behind `source`, then, if the copy is `due`, copy it
/// into `policy`'s directory under the name of `now`.
async fn copy_checked(
    source: &oxidgene_db::sea_orm::DatabaseConnection,
    database: &Path,
    policy: &SnapshotPolicy,
    now: DateTime<Utc>,
    due: bool,
) -> Result<SnapshotOutcome, OxidGeneError> {
    let report = check_integrity(source).await.map_err(db_err)?;
    if !report.is_sound() {
        return Ok(SnapshotOutcome::Damaged(report));
    }
    if !due {
        return Ok(SnapshotOutcome::NotDue);
    }
    std::fs::create_dir_all(&policy.directory)?;
    if !is_room(free_space(&policy.directory), database_size(database)) {
        return Ok(SnapshotOutcome::NoRoom);
    }
    let name = format!("{PREFIX}{}{SUFFIX}", now.format(TIME_FORMAT));
    let destination = policy.directory.join(&name);
    // Written under another name first: a copy cut short is never taken for
    // a snapshot.
    let partial = policy.directory.join(format!(".{name}.partial"));
    let _ = std::fs::remove_file(&partial);
    if let Err(failure) = snapshot_into(source, &partial).await {
        let _ = std::fs::remove_file(&partial);
        return Err(db_err(failure));
    }
    std::fs::rename(&partial, &destination)?;
    Ok(SnapshotOutcome::Taken(destination))
}

/// Check `database` and take the day's snapshot of it in the background,
/// and record in `health` a file that failed its check. Never fails its
/// caller: the outcome is logged.
pub fn spawn_daily_snapshot(database: PathBuf, policy: SnapshotPolicy, health: DatabaseHealth) {
    tokio::spawn(async move {
        let now = DateTime::<Utc>::from(SystemTime::now());
        log_outcome(snapshot_if_due(&database, &policy, now).await, &health);
    });
}

/// Log how a snapshot ended, and record a damaged file in `health`. No
/// path is logged: it holds the user's name.
pub fn log_outcome(outcome: Result<SnapshotOutcome, OxidGeneError>, health: &DatabaseHealth) {
    match outcome {
        Ok(SnapshotOutcome::Taken(_)) => info!("Database snapshot taken"),
        Ok(SnapshotOutcome::NotDue) => {}
        Ok(SnapshotOutcome::NoRoom) => log_no_room(),
        Ok(SnapshotOutcome::Damaged(report)) => {
            health.mark_damaged();
            log_damaged(report);
        }
        Err(failure) => log_failure(&failure),
    }
}

fn log_no_room() {
    warn!(
        error = "snapshot_no_room",
        "Database snapshot skipped: the disk lacks the room"
    );
}

/// The counts alone: the checks' messages name tables, rows and values.
fn log_damaged(report: IntegrityReport) {
    error!(
        error = "database_damaged",
        damaged = report.damaged,
        dangling = report.dangling,
        "The database file failed its integrity check: no snapshot was taken \
         and writes are refused"
    );
}

fn log_failure(failure: &OxidGeneError) {
    warn!(
        error = "snapshot_failed",
        error.kind = crate::error_contract::error_kind(failure),
        "Database snapshot failed"
    );
}

#[cfg(test)]
mod tests {
    use oxidgene_db::repo::{TreeRepo, run_migrations};
    use oxidgene_db::sea_orm::ConnectionTrait;
    use uuid::Uuid;

    use super::*;

    /// A migrated database file holding a tree, in a directory of its own.
    async fn database() -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().expect("a directory");
        let path = directory.path().join("oxidgene.db");
        let db = connect(&format!("sqlite://{}?mode=rwc", path.display()))
            .await
            .expect("connect");
        run_migrations(&db).await.expect("migrations");
        TreeRepo::create(&db, Uuid::now_v7(), "Fictional tree".into(), None)
            .await
            .expect("tree");
        db.close().await.expect("close");
        (directory, path)
    }

    fn at(day: u32, hour: u32) -> DateTime<Utc> {
        chrono::NaiveDate::from_ymd_opt(2026, 3, day)
            .and_then(|date| date.and_hms_opt(hour, 0, 0))
            .expect("a time")
            .and_utc()
    }

    /// One snapshot a day at most, the last three kept, each a database of
    /// its own that opens and holds the tree.
    #[tokio::test]
    async fn a_snapshot_is_taken_once_a_day_and_the_last_three_are_kept() {
        let (directory, database) = database().await;
        let policy = SnapshotPolicy::daily(directory.path().join("snapshots"));

        let first = snapshot_if_due(&database, &policy, at(1, 8))
            .await
            .expect("first");
        let SnapshotOutcome::Taken(copy) = first else {
            panic!("{first:?}");
        };
        assert_eq!(
            snapshot_if_due(&database, &policy, at(1, 20))
                .await
                .expect("same day"),
            SnapshotOutcome::NotDue
        );
        for day in 2..=5 {
            let taken = snapshot_if_due(&database, &policy, at(day, 9))
                .await
                .expect("a day later");
            assert!(matches!(taken, SnapshotOutcome::Taken(_)), "{taken:?}");
        }
        let kept: Vec<_> = snapshots(&policy.directory)
            .into_iter()
            .map(|(taken, _)| taken)
            .collect();
        assert_eq!(kept, [at(3, 9), at(4, 9), at(5, 9)]);
        assert!(!copy.exists(), "the oldest went");

        // Nothing else is left in the directory: no partial copy.
        assert_eq!(
            std::fs::read_dir(&policy.directory)
                .expect("directory")
                .count(),
            3
        );

        // A snapshot is a database: it opens, sound, with the tree.
        let (_, latest) = snapshots(&policy.directory).pop().expect("a snapshot");
        let restored = connect(&format!("sqlite://{}?mode=rw", latest.display()))
            .await
            .expect("the snapshot opens");
        assert!(check_integrity(&restored).await.expect("check").is_sound());
        let trees = TreeRepo::list(&restored, &oxidgene_db::repo::PaginationParams::default())
            .await
            .expect("trees");
        assert_eq!(trees.edges.len(), 1);
    }

    /// A pending migration's snapshot does not wait for the day to end.
    #[tokio::test]
    async fn a_snapshot_asked_for_is_taken_whatever_the_age_of_the_last() {
        let (directory, database) = database().await;
        let policy = SnapshotPolicy::daily(directory.path().join("snapshots"));
        for hour in [8, 9] {
            let taken = take_snapshot(&database, &policy, at(1, hour))
                .await
                .expect("taken");
            assert!(matches!(taken, SnapshotOutcome::Taken(_)), "{taken:?}");
        }
        assert_eq!(snapshots(&policy.directory).len(), 2);
    }

    /// A file whose rows name rows that do not exist fails the check: it is
    /// not copied, and the sound snapshots stay.
    #[tokio::test]
    async fn a_file_that_fails_its_check_is_not_copied() {
        let (directory, database) = database().await;
        let policy = SnapshotPolicy::daily(directory.path().join("snapshots"));
        take_snapshot(&database, &policy, at(1, 8))
            .await
            .expect("a sound copy");

        let db = connect(&format!("sqlite://{}?mode=rw", database.display()))
            .await
            .expect("connect");
        // As a tool that does not enforce the keys leaves it.
        db.execute_unprepared(&format!(
            "PRAGMA foreign_keys = OFF; \
             INSERT INTO place (id, tree_id, name, created_at, updated_at) \
             VALUES ('{}', '{}', 'Nowhere', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            Uuid::now_v7(),
            Uuid::now_v7()
        ))
        .await
        .expect("a place of no tree");
        db.close().await.expect("close");

        // Checked at every start, whether a snapshot is due or not.
        let outcome = snapshot_if_due(&database, &policy, at(1, 9))
            .await
            .expect("checked");
        assert!(
            matches!(outcome, SnapshotOutcome::Damaged(_)),
            "{outcome:?}"
        );
        let outcome = snapshot_if_due(&database, &policy, at(3, 8))
            .await
            .expect("checked");
        let SnapshotOutcome::Damaged(report) = outcome.clone() else {
            panic!("{outcome:?}");
        };
        assert_eq!((report.damaged, report.dangling), (0, 1));
        assert_eq!(
            snapshots(&policy.directory).len(),
            1,
            "the sound copy stays"
        );

        let health = DatabaseHealth::default();
        log_outcome(Ok(outcome), &health);
        assert!(health.is_damaged());
    }

    #[test]
    fn a_copy_wants_its_size_and_a_tenth_to_spare() {
        assert!(is_room(None, u64::MAX));
        assert!(is_room(Some(110), 100));
        assert!(!is_room(Some(109), 100));
        assert!(is_room(Some(0), 0));
    }

    #[test]
    fn only_snapshots_are_listed_oldest_first() {
        let directory = tempfile::tempdir().expect("a directory");
        for name in [
            "oxidgene-20260302T090000Z.db",
            "oxidgene-20260301T090000Z.db",
            ".oxidgene-20260303T090000Z.db.partial",
            "oxidgene-yesterday.db",
            "notes.txt",
        ] {
            std::fs::write(directory.path().join(name), b"").expect("a file");
        }
        let listed: Vec<_> = snapshots(directory.path())
            .into_iter()
            .map(|(taken, _)| taken)
            .collect();
        assert_eq!(listed, [at(1, 9), at(2, 9)]);
        assert!(snapshots(&directory.path().join("missing")).is_empty());
    }

    /// A sound file refuses nothing; a damaged one what changes it, and
    /// neither a `GET` nor a read sent as `POST` nor an export; one of
    /// another schema everything, and stays so.
    #[test]
    fn a_file_that_cannot_be_used_refuses_what_it_cannot_serve() {
        use oxidgene_core::access::Permission;

        let edit = Requirement::Tree(Permission::Edit);
        let read = Requirement::Tree(Permission::Read);
        let refused = |health: &DatabaseHealth, method: Method, requirement| {
            health
                .refusal(&method, requirement)
                .map(|response| response.status())
        };
        let unavailable = Some(StatusCode::SERVICE_UNAVAILABLE);

        let health = DatabaseHealth::default();
        assert_eq!(refused(&health, Method::POST, edit), None);

        health.mark_damaged();
        assert!(health.is_damaged());
        assert_eq!(refused(&health, Method::POST, edit), unavailable);
        assert_eq!(refused(&health, Method::DELETE, edit), unavailable);
        assert_eq!(
            refused(&health, Method::POST, Requirement::TreeCreation),
            unavailable
        );
        assert_eq!(refused(&health, Method::GET, edit), None);
        assert_eq!(refused(&health, Method::POST, read), None);
        assert_eq!(
            refused(&health, Method::POST, Requirement::Tree(Permission::Export)),
            None
        );
        assert_eq!(refused(&health, Method::POST, Requirement::Open), None);

        let incompatible = DatabaseHealth::incompatible();
        incompatible.mark_damaged();
        assert!(incompatible.is_incompatible());
        assert_eq!(refused(&incompatible, Method::GET, read), unavailable);
        assert_eq!(refused(&incompatible, Method::POST, edit), unavailable);

        // A damaged file that was not opened answers nothing either.
        let closed = DatabaseHealth::closed();
        assert!(closed.is_damaged());
        assert_eq!(refused(&closed, Method::GET, read), unavailable);
    }
}
