use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use base64::Engine as _;
use oxidgene_core::OxidGeneError;
use tempfile::TempPath;

static STAGED_MEDIA: OnceLock<Mutex<HashMap<PathBuf, TempPath>>> = OnceLock::new();

fn staged_media() -> &'static Mutex<HashMap<PathBuf, TempPath>> {
    STAGED_MEDIA.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn stage(
    media: &HashMap<String, String>,
) -> Result<HashMap<String, String>, OxidGeneError> {
    let mut paths = HashMap::with_capacity(media.len());
    let mut staged = Vec::with_capacity(media.len());

    for (url, encoded) in media {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|error| {
                OxidGeneError::Validation(format!("invalid session media: {error}"))
            })?;
        let mut file = tempfile::Builder::new()
            .prefix("oxidgene-geneanet-")
            .tempfile()?;
        file.write_all(&bytes)?;
        file.flush()?;
        let path = file.into_temp_path();
        paths.insert(url.clone(), path.to_string_lossy().into_owned());
        staged.push(path);
    }

    let mut registry = staged_media()
        .lock()
        .map_err(|_| OxidGeneError::Internal("session media registry is unavailable".into()))?;
    for path in staged {
        registry.insert(path.to_path_buf(), path);
    }
    Ok(paths)
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
        let paths = stage(&HashMap::from([(
            "https://example.invalid/media.jpg".to_string(),
            "aGVsbG8=".to_string(),
        )]))
        .expect("stages media");
        let path = paths.values().next().expect("has a path");
        assert_eq!(std::fs::read(path).expect("reads staged media"), b"hello");

        remove_owned(["/tmp/not-owned-by-oxidgene"]);
        assert!(Path::new(path).exists());

        remove_owned([path.as_str()]);
        assert!(!Path::new(path).exists());
    }
}
