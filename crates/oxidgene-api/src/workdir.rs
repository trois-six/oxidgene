//! The one directory a process keeps its disposable working files in.
//!
//! Everything a request or a job writes only to read it back moments later
//! goes here, never to the system's temporary directory, which is often a
//! RAM-backed `tmpfs` that a large import would fill:
//!
//! - `jobs/` — a job's scratch directory: its staged source, a Geneanet
//!   import's archives and pages, an export's media and archive. Removed
//!   when the job ends, completed or failed.
//! - `staging/` — inputs between a request and the job it queues: an
//!   uploaded file, the media of a decoded Geneanet session, the pages the
//!   desktop's Geneanet window fetched, a media archive being streamed.
//!   Removed once read.
//!
//! The desktop's is its cache directory ([`crate::app_dirs::AppDirs::cache`]);
//! the server and the worker take `OXIDGENE_WORK_DIR`, the user's cache
//! directory by default, and the system's temporary directory only when
//! there is no user's cache directory either.
//!
//! What a crashed run left is swept once it is older than [`STALE_AFTER`]:
//! when a process starts, and at each worker maintenance pass.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tempfile::{NamedTempFile, TempDir};
use uuid::Uuid;

use crate::app_dirs::AppDirs;

/// How old a leftover must be before a sweep removes it: the longest a job
/// can run without its scratch being touched — the lease of the desktop's
/// single SQLite worker — which is also how long staged inputs wait for the
/// job that reads them. Anything younger may belong to another process
/// sharing the directory.
pub const STALE_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

const JOBS: &str = "jobs";
const STAGING: &str = "staging";

/// A process's working directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkDir {
    root: Arc<PathBuf>,
}

impl WorkDir {
    /// Working files under `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: Arc::new(root.into()),
        }
    }

    /// The working directory a server or worker uses unless told one: the
    /// user's cache directory, or [`Self::temporary`] without one.
    #[must_use]
    pub fn user_default() -> Self {
        AppDirs::resolve().map_or_else(Self::temporary, |dirs| Self::new(dirs.work()))
    }

    /// `oxidgene/` in the system's temporary directory: the last resort,
    /// and what an [`AppState`](crate::AppState) nobody configured uses, as
    /// tests do.
    #[must_use]
    pub fn temporary() -> Self {
        Self::new(std::env::temp_dir().join("oxidgene"))
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where jobs keep their scratch directories.
    #[must_use]
    pub fn jobs(&self) -> PathBuf {
        self.root.join(JOBS)
    }

    /// Where staged inputs wait.
    #[must_use]
    pub fn staging(&self) -> PathBuf {
        self.root.join(STAGING)
    }

    /// A scratch directory for job `job_id`, removed when dropped.
    pub fn job_scratch(&self, job_id: Uuid) -> std::io::Result<TempDir> {
        tempfile::Builder::new()
            .prefix(&format!("{job_id}-"))
            .tempdir_in(created(self.jobs())?)
    }

    /// A staged file named after `prefix`, removed when dropped.
    pub fn staged_file(&self, prefix: &str) -> std::io::Result<NamedTempFile> {
        tempfile::Builder::new()
            .prefix(prefix)
            .tempfile_in(created(self.staging())?)
    }

    /// A staged directory named after `prefix`, removed when dropped.
    pub fn staged_directory(&self, prefix: &str) -> std::io::Result<TempDir> {
        tempfile::Builder::new()
            .prefix(prefix)
            .tempdir_in(created(self.staging())?)
    }

    /// A staged file with no name, gone once closed.
    pub fn anonymous_file(&self) -> std::io::Result<std::fs::File> {
        tempfile::tempfile_in(created(self.staging())?)
    }

    /// Remove what earlier runs left under `jobs/` and `staging/`, last
    /// modified more than [`STALE_AFTER`] before `now`; how many entries
    /// went. Entries of this process are younger, or held open.
    pub fn sweep(&self, now: SystemTime) -> usize {
        let removed = [self.jobs(), self.staging()]
            .iter()
            .map(|directory| sweep_directory(directory, now))
            .sum();
        if removed > 0 {
            tracing::info!(removed, "removed working files left by an earlier run");
        }
        removed
    }
}

/// `directory`, created if it is missing.
fn created(directory: PathBuf) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(&directory)?;
    Ok(directory)
}

/// Remove the entries of `directory` last modified more than
/// [`STALE_AFTER`] before `now`.
fn sweep_directory(directory: &Path, now: SystemTime) -> usize {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| is_stale(entry, now) && remove_entry(&entry.path()))
        .count()
}

/// Whether `entry` was last modified more than [`STALE_AFTER`] before `now`.
fn is_stale(entry: &std::fs::DirEntry, now: SystemTime) -> bool {
    entry
        .metadata()
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| now.duration_since(modified).ok())
        .is_some_and(|age| age >= STALE_AFTER)
}

/// Remove the file or directory at `path`; whether it went.
fn remove_entry(path: &Path) -> bool {
    if path.is_dir() {
        std::fs::remove_dir_all(path).is_ok()
    } else {
        std::fs::remove_file(path).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scratch_and_staging_land_under_the_root_and_go_when_dropped() {
        let root = tempfile::tempdir().unwrap();
        let work = WorkDir::new(root.path());
        let job_id = Uuid::now_v7();

        let scratch = work.job_scratch(job_id).unwrap();
        assert_eq!(scratch.path().parent(), Some(work.jobs().as_path()));
        let staged = work.staged_file("upload-").unwrap();
        assert_eq!(staged.path().parent(), Some(work.staging().as_path()));
        let (scratch_path, staged_path) = (scratch.path().to_owned(), staged.path().to_owned());

        drop((scratch, staged));
        assert!(!scratch_path.exists());
        assert!(!staged_path.exists());
    }

    #[test]
    fn a_sweep_removes_only_what_is_stale() {
        let root = tempfile::tempdir().unwrap();
        let work = WorkDir::new(root.path());
        let left = work.job_scratch(Uuid::now_v7()).unwrap().keep();
        std::fs::write(left.join("artifact.gdz"), b"fixture").unwrap();
        let staged = work
            .staged_file("upload-")
            .unwrap()
            .into_temp_path()
            .keep()
            .unwrap();
        // Something next to the two directories is not this module's.
        let unrelated = root.path().join("WebKitCache");
        std::fs::create_dir(&unrelated).unwrap();

        assert_eq!(work.sweep(SystemTime::now()), 0, "fresh entries stay");
        assert!(left.exists() && staged.exists());

        let later = SystemTime::now() + STALE_AFTER + Duration::from_secs(1);
        assert_eq!(work.sweep(later), 2);
        assert!(!left.exists());
        assert!(!staged.exists());
        assert!(unrelated.exists());
    }

    #[test]
    fn a_missing_root_sweeps_nothing() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            WorkDir::new(root.path().join("absent")).sweep(SystemTime::now()),
            0
        );
    }
}
