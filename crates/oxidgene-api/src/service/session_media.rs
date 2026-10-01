//! The media of a decoded Geneanet session, staged as private temporary files
//! until the import that reads them is queued.
//!
//! Decoding a saved session extracts its photos — the account's own media —
//! into `oxidgene-geneanet-*` temporary files, and the wizard hands their
//! paths back with the import. Each staged file is owned here, and goes:
//! - when the import is queued, which copies it into job storage;
//! - when the wizard releases it, closed or reset without importing;
//! - [`STAGED_MEDIA_TTL`] after it was staged, if neither came;
//! - at the next start, from the temporary directory, if the process ended
//!   first. That sweep also takes a job's leftover `oxidgene-job-*` scratch
//!   directory, a crashed export's copy of a tree's media.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use oxidgene_core::OxidGeneError;
use oxidgene_geneanet::session::{self, Session};
use tempfile::TempPath;

pub(crate) static LOADS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);

/// How long a staged medium waits for its import or its release.
pub(crate) const STAGED_MEDIA_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// How often staged media past [`STAGED_MEDIA_TTL`] are deleted.
const EXPIRY_PERIOD: Duration = Duration::from_secs(60 * 60);

/// The prefixes of the temporary files and directories the application
/// creates for its work, and sweeps at start.
const TEMPORARY_PREFIXES: [&str; 2] = ["oxidgene-geneanet-", "oxidgene-job-"];

/// A staged medium and when it was staged.
struct Staged {
    _file: TempPath,
    staged_at: Instant,
}

static STAGED_MEDIA: OnceLock<Mutex<HashMap<PathBuf, Staged>>> = OnceLock::new();

fn staged_media() -> &'static Mutex<HashMap<PathBuf, Staged>> {
    STAGED_MEDIA.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn decode(reader: impl Read + Seek) -> Result<Session, OxidGeneError> {
    let mut staged = Vec::new();
    let session = session::decode_with_media(reader, |entry| {
        let mut file = tempfile::Builder::new()
            .prefix("oxidgene-geneanet-")
            .tempfile()?;
        std::io::copy(entry, &mut file)?;
        file.flush()?;
        let path = file.into_temp_path();
        let handle = path.to_string_lossy().into_owned();
        staged.push(path);
        Ok(handle)
    })
    .map_err(|error| OxidGeneError::Validation(error.to_string()))?;

    let mut registry = staged_media()
        .lock()
        .map_err(|_| OxidGeneError::Internal("session media registry is unavailable".into()))?;
    let referenced: HashSet<&Path> = session.media.values().map(Path::new).collect();
    let staged_at = Instant::now();
    for path in staged {
        if referenced.contains::<Path>(&path) {
            registry.insert(
                path.to_path_buf(),
                Staged {
                    _file: path,
                    staged_at,
                },
            );
        }
    }
    Ok(session)
}

/// Delete the staged media among `paths`. A path this registry does not own
/// — a file the login window fetched, or anything else — is left alone.
pub(crate) fn remove_owned<'a>(paths: impl IntoIterator<Item = &'a str>) {
    let Ok(mut registry) = staged_media().lock() else {
        tracing::warn!("session media registry is unavailable during cleanup");
        return;
    };
    let owned: Vec<_> = paths
        .into_iter()
        .filter_map(|path| registry.remove(Path::new(path)))
        .collect();
    drop(registry);
    drop(owned);
}

/// Delete the media of `registry` staged more than [`STAGED_MEDIA_TTL`]
/// before `now`.
fn expire(registry: &Mutex<HashMap<PathBuf, Staged>>, now: Instant) {
    let Ok(mut registry) = registry.lock() else {
        return;
    };
    let expired: Vec<_> = registry
        .extract_if(|_, staged| now.duration_since(staged.staged_at) >= STAGED_MEDIA_TTL)
        .collect();
    drop(registry);
    drop(expired);
}

/// Start, once per process, what bounds the life of staged media: sweep the
/// temporary files earlier runs left, then expire staged media hourly.
///
/// Only the desktop backend stages media, so only it starts this. Without a
/// Tokio runtime nothing is started.
pub(crate) fn start_janitor() {
    static STARTED: OnceLock<()> = OnceLock::new();
    if STARTED.set(()).is_err() {
        return;
    }
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        return;
    };
    // Detached from whatever request first staged media: the sweep is a root
    // of its own, and the hourly expiry touches neither the database nor any
    // traced work.
    runtime.spawn(async {
        let sweep = tracing::info_span!(parent: None, "session_media.sweep");
        let _ = crate::service::blocking::spawn_in(sweep, || {
            sweep_leftovers(&std::env::temp_dir(), SystemTime::now());
        })
        .await;
        let mut ticks = tokio::time::interval(EXPIRY_PERIOD);
        loop {
            ticks.tick().await;
            expire(staged_media(), Instant::now());
        }
    });
}

/// Delete the application's temporary files and directories in `directory`
/// last modified more than [`STAGED_MEDIA_TTL`] before `now`.
///
/// Only that old: another process — a second instance, the MCP server — may
/// be using newer ones, and anything this old is past its own expiry.
fn sweep_leftovers(directory: &Path, now: SystemTime) -> usize {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let ours = name.to_str().is_some_and(|name| {
            TEMPORARY_PREFIXES
                .iter()
                .any(|prefix| name.starts_with(prefix))
        });
        if ours && is_stale(&entry, now) && remove_entry(&entry.path()) {
            removed += 1;
        }
    }
    if removed > 0 {
        tracing::info!(removed, "removed temporary files left by an earlier run");
    }
    removed
}

/// Whether `entry` was last modified more than [`STAGED_MEDIA_TTL`] before
/// `now`.
fn is_stale(entry: &std::fs::DirEntry, now: SystemTime) -> bool {
    entry
        .metadata()
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| now.duration_since(modified).ok())
        .is_some_and(|age| age >= STAGED_MEDIA_TTL)
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

    fn decode_one_medium() -> String {
        let archive = session::encode(&Session {
            collection: r#"{"deposits":[],"references":[],"view_references":{}}"#.into(),
            media: HashMap::from([(
                "https://example.invalid/media.jpg".to_string(),
                "aGVsbG8=".to_string(),
            )]),
            ..Default::default()
        })
        .unwrap();
        decode(std::io::Cursor::new(archive))
            .expect("stages media")
            .media
            .into_values()
            .next()
            .expect("has a path")
    }

    #[test]
    fn staged_media_are_private_and_removed_only_when_owned() {
        let path = decode_one_medium();
        assert_eq!(std::fs::read(&path).expect("reads staged media"), b"hello");

        remove_owned(["/tmp/not-owned-by-oxidgene"]);
        assert!(Path::new(&path).exists());

        remove_owned([path.as_str()]);
        assert!(!Path::new(&path).exists());
    }

    #[test]
    fn staged_media_expire_after_their_ttl() {
        let path = decode_one_medium();
        // Moved to a registry of its own, so that expiring it cannot reach
        // what a test running alongside has staged.
        let staged = staged_media()
            .lock()
            .unwrap()
            .remove(Path::new(&path))
            .expect("staged");
        let registry = Mutex::new(HashMap::from([(PathBuf::from(&path), staged)]));

        expire(&registry, Instant::now());
        assert!(
            Path::new(&path).exists(),
            "fresh media wait for their import"
        );

        expire(&registry, Instant::now() + STAGED_MEDIA_TTL);
        assert!(!Path::new(&path).exists(), "abandoned media expire");
    }

    #[test]
    fn the_startup_sweep_takes_only_our_old_temporary_files() {
        let directory = tempfile::tempdir().unwrap();
        let ours = directory.path().join("oxidgene-geneanet-fixture");
        let scratch = directory.path().join("oxidgene-job-fixture");
        let theirs = directory.path().join("unrelated-fixture");
        std::fs::write(&ours, b"photo").unwrap();
        std::fs::create_dir(&scratch).unwrap();
        std::fs::write(scratch.join("media"), b"photo").unwrap();
        std::fs::write(&theirs, b"other").unwrap();

        assert_eq!(sweep_leftovers(directory.path(), SystemTime::now()), 0);
        assert!(ours.exists() && scratch.exists());

        let later = SystemTime::now() + STAGED_MEDIA_TTL;
        assert_eq!(sweep_leftovers(directory.path(), later), 2);
        assert!(!ours.exists() && !scratch.exists());
        assert!(theirs.exists());
    }
}
