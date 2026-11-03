//! The media of a decoded Geneanet session, staged as private files until
//! the import that reads them is queued.
//!
//! Decoding a saved session extracts its photos — the account's own media —
//! into `geneanet-*` files staged in the working directory
//! ([`crate::workdir`]), and the wizard hands their paths back with the
//! import. Each staged file is owned here, and goes:
//! - when the import is queued, which copies it into job storage;
//! - when the wizard releases it, closed or reset without importing;
//! - [`STAGED_MEDIA_TTL`] after it was staged, if neither came;
//! - once stale, swept from the working directory, if the process ended
//!   first.
//!
//! The web build exchanges no path: its wizard [`read`]s a session, keeping
//! nothing, then sends the same file with the import, whose media are
//! [`extract`]ed and copied into job storage within that request.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use oxidgene_core::OxidGeneError;
use oxidgene_geneanet::archive::ArchiveSet;
use oxidgene_geneanet::session::{self, Session};
use tempfile::TempPath;

use crate::workdir::WorkDir;

/// How long a staged medium waits for its import or its release.
pub(crate) const STAGED_MEDIA_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// How often staged media past [`STAGED_MEDIA_TTL`] are deleted.
const EXPIRY_PERIOD: Duration = Duration::from_secs(60 * 60);

/// A staged medium and when it was staged.
struct Staged {
    _file: TempPath,
    staged_at: Instant,
}

static STAGED_MEDIA: OnceLock<Mutex<HashMap<PathBuf, Staged>>> = OnceLock::new();

fn staged_media() -> &'static Mutex<HashMap<PathBuf, Staged>> {
    STAGED_MEDIA.get_or_init(|| Mutex::new(HashMap::new()))
}

/// A decoded session and the private files its media were extracted to,
/// each deleted when its handle drops. The session's `media` map names those
/// files.
pub(crate) struct Extracted {
    pub(crate) session: Session,
    pub(crate) files: Vec<TempPath>,
}

/// Decode a saved session, extracting each of its media to a `geneanet-*`
/// file of the working directory. Nothing outlives the returned value:
/// [`decode`] hands the files to the registry, and a session import
/// copies them into job storage.
///
/// # Errors
///
/// `Validation` for anything that is not a session.
pub(crate) fn extract(
    reader: impl Read + Seek,
    work_dir: &WorkDir,
) -> Result<Extracted, OxidGeneError> {
    let mut staged = Vec::new();
    let session = session::decode_with_media(reader, |entry| {
        let mut file = work_dir.staged_file("geneanet-")?;
        std::io::copy(entry, &mut file)?;
        file.flush()?;
        let path = file.into_temp_path();
        let handle = path.to_string_lossy().into_owned();
        staged.push(path);
        Ok(handle)
    })
    .map_err(|error| OxidGeneError::Validation(error.to_string()))?;
    check_deposit_sizes(&session)?;
    let referenced: HashSet<&Path> = session.media.values().map(Path::new).collect();
    let files = staged
        .into_iter()
        .filter(|path| referenced.contains::<Path>(path))
        .collect();
    Ok(Extracted { session, files })
}

/// Decode a saved session for the desktop wizard: its media are staged
/// ([`extract`]) and owned here until the import or the wizard lets them go.
pub(crate) fn decode(
    reader: impl Read + Seek,
    work_dir: &WorkDir,
) -> Result<Session, OxidGeneError> {
    let Extracted { session, files } = extract(reader, work_dir)?;
    let mut registry = staged_media()
        .lock()
        .map_err(|_| OxidGeneError::Internal("session media registry is unavailable".into()))?;
    let staged_at = Instant::now();
    for path in files {
        registry.insert(
            path.to_path_buf(),
            Staged {
                _file: path,
                staged_at,
            },
        );
    }
    Ok(session)
}

/// What a saved session holds, its media checked as [`extract`] checks them
/// but kept nowhere: the session's `media` map names each medium's URL with
/// an empty handle. What the web wizard reads before importing the same file
/// again with [`crate::service::background_job::stage_geneanet_session_import`].
///
/// # Errors
///
/// `Validation` for anything that is not a session.
pub(crate) fn read(reader: impl Read + Seek) -> Result<Session, OxidGeneError> {
    let session = session::decode_with_media(reader, |entry| {
        std::io::copy(entry, &mut std::io::sink())?;
        Ok(String::new())
    })
    .map_err(|error| OxidGeneError::Validation(error.to_string()))?;
    check_deposit_sizes(&session)?;
    Ok(session)
}

/// A byte length past the signed range is no file's: GraphQL could only
/// carry it wrapped negative.
fn check_deposit_sizes(session: &Session) -> Result<(), OxidGeneError> {
    if session
        .deposit_sizes
        .values()
        .any(|size| i64::try_from(*size).is_err())
    {
        return Err(OxidGeneError::Validation(
            "a deposit size of the session is out of range".to_string(),
        ));
    }
    Ok(())
}

/// The archive of `session`, each of its media read from the path the
/// wizard holds — staged by [`decode`] or fetched by the login window — and
/// streamed into it, with the originals the data archives hold.
///
/// Those originals are what the login window never fetches: the import reads
/// a single-page deposit's original from the archive entry of its exact byte
/// length. A file that left them out would bring the web build, which takes
/// no archive, every medium but the photos. So each deposit whose size names
/// an entry of `archive_paths`, and whose original the session does not hold
/// already, has that entry written under its original's URL, as if the
/// window had fetched it. Reading hundreds of pictures and deflating them is
/// blocking work: it runs on the blocking pool, off the async workers the
/// desktop shares with every page load.
///
/// # Errors
///
/// `Validation` for a session that cannot be encoded, such as one naming a
/// medium that cannot be read, or whose collection is not one.
pub(crate) async fn encode(
    mut session: Session,
    archive_paths: Vec<String>,
) -> Result<Vec<u8>, OxidGeneError> {
    let span = tracing::info_span!(
        "geneanet.session.encode",
        session.media.count = session.media.len(),
        archive.count = archive_paths.len()
    );
    super::blocking::run(span, move || {
        let (archives, _) = super::geneanet::index_archives(&archive_paths);
        let entries = archived_originals(&session, &archives)?;
        session.media.extend(
            entries
                .iter()
                .map(|(handle, (url, _))| (url.clone(), handle.clone())),
        );
        session::encode_with_media(&session, |handle| match entries.get(handle) {
            Some((_, position)) => archives
                .read_at(*position)
                .map(|bytes| Box::new(std::io::Cursor::new(bytes)) as Box<dyn Read>)
                .map_err(std::io::Error::other),
            None => open_staged_medium(handle),
        })
        .map_err(|error| OxidGeneError::Validation(error.to_string()))
    })
    .await?
}

/// The originals of `session`'s single-page deposits that `archives` hold and
/// the session does not, by the handle [`encode`] names each with: a NUL
/// first, which no path holds, then the entry's position. Each maps to the
/// original's URL and the entry's position.
fn archived_originals(
    session: &Session,
    archives: &ArchiveSet,
) -> Result<HashMap<String, (String, usize)>, OxidGeneError> {
    if archives.is_empty() {
        return Ok(HashMap::new());
    }
    let manifest = oxidgene_geneanet::manifest_from_collection(&session.collection)
        .map_err(|error| OxidGeneError::Validation(error.to_string()))?;
    let mut entries = HashMap::new();
    for deposit in &manifest.deposits {
        let url = &deposit.original;
        if deposit.views.len() != 1 || url.is_empty() || session.media.contains_key(url) {
            continue;
        }
        let Some(size) = session.deposit_sizes.get(&deposit.id) else {
            continue;
        };
        if let Ok(Some(position)) = archives.locate_by_size(*size) {
            entries.insert(format!("\0archive:{position}"), (url.clone(), position));
        }
    }
    Ok(entries)
}

/// The medium the wizard holds at `path`, opened for
/// `session::encode_with_media`.
fn open_staged_medium(path: &str) -> std::io::Result<Box<dyn Read>> {
    Ok(Box::new(std::fs::File::open(path)?))
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

/// Start, once per process, what bounds the life of staged media: expire
/// them hourly. What an earlier run left is the working directory's sweep's
/// (see [`WorkDir::sweep`]).
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
    // The hourly expiry touches neither the database nor any traced work.
    runtime.spawn(async {
        let mut ticks = tokio::time::interval(EXPIRY_PERIOD);
        loop {
            ticks.tick().await;
            expire(staged_media(), Instant::now());
        }
    });
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
        decode(std::io::Cursor::new(archive), &WorkDir::temporary())
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
}
