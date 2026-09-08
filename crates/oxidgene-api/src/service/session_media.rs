use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use oxidgene_core::OxidGeneError;
use oxidgene_geneanet::session::{self, Session};
use tempfile::TempPath;

pub(crate) static LOADS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);

static STAGED_MEDIA: OnceLock<Mutex<HashMap<PathBuf, TempPath>>> = OnceLock::new();

fn staged_media() -> &'static Mutex<HashMap<PathBuf, TempPath>> {
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
    for path in staged {
        if referenced.contains::<Path>(&path) {
            registry.insert(path.to_path_buf(), path);
        }
    }
    Ok(session)
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staged_media_are_private_and_removed_only_when_owned() {
        let archive = session::encode(&Session {
            collection: r#"{"deposits":[],"references":[],"view_references":{}}"#.into(),
            media: HashMap::from([(
                "https://example.invalid/media.jpg".to_string(),
                "aGVsbG8=".to_string(),
            )]),
            ..Default::default()
        })
        .unwrap();
        let paths = decode(std::io::Cursor::new(archive))
            .expect("stages media")
            .media;
        let path = paths.values().next().expect("has a path");
        assert_eq!(std::fs::read(path).expect("reads staged media"), b"hello");

        remove_owned(["/tmp/not-owned-by-oxidgene"]);
        assert!(Path::new(path).exists());

        remove_owned([path.as_str()]);
        assert!(!Path::new(path).exists());
    }
}
