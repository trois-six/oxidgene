//! Disk persistence for `MemoryCacheStore`.
//!
//! Serializes cached pedigrees to a bincode file on disk for fast startup on
//! desktop. Since Sprint E.6 only pedigrees are persisted: persons are rebuilt
//! from local SQLite on demand and search lives in the `person_search_fts`
//! table inside the database itself.
//!
//! Uses platform-native cache directories via `dirs::cache_dir()`:
//! - Linux:   `~/.cache/oxidgene/`
//! - macOS:   `~/Library/Caches/oxidgene/`
//! - Windows: `C:\Users\<user>\AppData\Local\oxidgene\`

use crate::store::memory::MemoryCacheStore;
use crate::types::*;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, BufReader, BufWriter};
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

// ── File names ───────────────────────────────────────────────────────────

const PEDIGREES_FILE: &str = "pedigrees.bin";
const METADATA_FILE: &str = "cache_metadata.json";

/// Legacy files from schema version 1 (persons + search were still persisted).
/// Removed on clear and ignored on load.
const LEGACY_FILES: [&str; 2] = ["persons.bin", "search_indexes.bin"];

// ── Serializable snapshot types ──────────────────────────────────────────

/// All cached pedigrees.
#[derive(Serialize, Deserialize)]
struct PedigreesSnapshot {
    entries: Vec<CachedPedigree>,
}

/// Metadata about the persisted cache — used for staleness detection.
#[derive(Debug, Serialize, Deserialize)]
pub struct CacheMetadata {
    /// When the cache was last persisted to disk.
    pub persisted_at: DateTime<Utc>,
    /// Schema version for forward compatibility.
    pub schema_version: u32,
    /// Number of pedigrees cached.
    pub pedigree_count: usize,
    /// Optional: path to the SQLite DB at persistence time (for staleness check).
    pub db_path: Option<String>,
    /// Optional: last-modified timestamp of the SQLite DB file.
    pub db_modified_at: Option<DateTime<Utc>>,
}

/// Current schema version. Bump when snapshot format changes.
/// v2 (Sprint E.6): only pedigrees are persisted.
const SCHEMA_VERSION: u32 = 2;

// ── Public API ───────────────────────────────────────────────────────────

/// Resolve the platform-native cache directory for OxidGene.
///
/// Returns `None` if the platform has no standard cache directory.
pub fn default_cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("oxidgene"))
}

/// Persist the in-memory pedigree cache to disk as a bincode file.
///
/// Creates the directory if it doesn't exist.
/// Files are written atomically (write to temp, then rename) to avoid corruption.
///
/// # Arguments
/// * `store` — the in-memory cache store to persist
/// * `cache_dir` — directory to write files into
/// * `db_path` — optional path to the SQLite DB (for staleness metadata)
pub fn persist_to_disk(
    store: &MemoryCacheStore,
    cache_dir: &Path,
    db_path: Option<&Path>,
) -> io::Result<()> {
    fs::create_dir_all(cache_dir)?;

    let pedigrees = store.snapshot_for_disk();
    let pedigree_count = pedigrees.len();

    write_bincode_atomic(
        cache_dir,
        PEDIGREES_FILE,
        &PedigreesSnapshot { entries: pedigrees },
    )?;

    // Persist metadata (JSON for human readability)
    let db_modified_at = db_path
        .and_then(|p| fs::metadata(p).ok())
        .and_then(|m| m.modified().ok())
        .map(DateTime::<Utc>::from);

    let metadata = CacheMetadata {
        persisted_at: Utc::now(),
        schema_version: SCHEMA_VERSION,
        pedigree_count,
        db_path: db_path.map(|p| p.to_string_lossy().into_owned()),
        db_modified_at,
    };

    let meta_path = cache_dir.join(METADATA_FILE);
    let meta_json = serde_json::to_string_pretty(&metadata).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Failed to serialize metadata: {e}"),
        )
    })?;
    fs::write(&meta_path, meta_json)?;

    // Remove leftover schema-v1 files so stale data doesn't linger on disk.
    for name in LEGACY_FILES {
        let path = cache_dir.join(name);
        if path.exists() {
            let _ = fs::remove_file(&path);
        }
    }

    info!(
        pedigree_count,
        cache_dir = %cache_dir.display(),
        "Cache persisted to disk"
    );

    Ok(())
}

/// Load cached pedigrees from disk into a new `MemoryCacheStore`.
///
/// Returns `None` if the cache directory doesn't exist or files are missing/corrupt.
/// On any deserialization error, logs a warning and returns `None` (triggering a fresh rebuild).
///
/// # Arguments
/// * `cache_dir` — directory containing persisted cache files
/// * `pedigree_budget_bytes` — LRU budget for the new store
pub fn load_from_disk(cache_dir: &Path, pedigree_budget_bytes: usize) -> Option<MemoryCacheStore> {
    if !cache_dir.exists() {
        debug!(cache_dir = %cache_dir.display(), "Cache directory does not exist, starting fresh");
        return None;
    }

    // Check metadata first
    let metadata = load_metadata(cache_dir)?;
    if metadata.schema_version != SCHEMA_VERSION {
        warn!(
            found = metadata.schema_version,
            expected = SCHEMA_VERSION,
            "Cache schema version mismatch, discarding"
        );
        return None;
    }

    let pedigrees: PedigreesSnapshot = read_bincode(cache_dir, PEDIGREES_FILE)?;

    let store = MemoryCacheStore::from_disk_snapshot(pedigrees.entries, pedigree_budget_bytes);

    info!(
        pedigrees = metadata.pedigree_count,
        cache_dir = %cache_dir.display(),
        "Cache loaded from disk"
    );

    Some(store)
}

/// Load just the metadata file (for staleness checks without loading the full cache).
pub fn load_metadata(cache_dir: &Path) -> Option<CacheMetadata> {
    let meta_path = cache_dir.join(METADATA_FILE);
    if !meta_path.exists() {
        return None;
    }
    let content = fs::read_to_string(&meta_path).ok()?;
    serde_json::from_str(&content)
        .map_err(|e| {
            warn!("Failed to parse cache metadata: {e}");
            e
        })
        .ok()
}

/// Check whether the persisted cache is stale relative to the SQLite DB.
///
/// Returns `true` if the cache should be discarded and rebuilt:
/// - DB file has been modified since the cache was persisted
/// - DB path has changed
/// - Metadata is missing or unreadable
pub fn is_cache_stale(cache_dir: &Path, db_path: &Path) -> bool {
    let Some(metadata) = load_metadata(cache_dir) else {
        return true; // No metadata → stale
    };

    // Check schema version
    if metadata.schema_version != SCHEMA_VERSION {
        return true;
    }

    // Check DB path matches
    let current_db = db_path.to_string_lossy().into_owned();
    if metadata.db_path.as_deref() != Some(&current_db) {
        warn!("Cache DB path mismatch: expected {current_db}");
        return true;
    }

    // Check DB modification time
    let db_modified = fs::metadata(db_path)
        .ok()
        .and_then(|m| m.modified().ok())
        .map(DateTime::<Utc>::from);

    match (db_modified, metadata.db_modified_at) {
        (Some(current), Some(cached)) => {
            if current > cached {
                info!("DB modified since cache was persisted (db={current}, cache={cached})");
                true
            } else {
                false
            }
        }
        // Can't compare → assume stale to be safe
        _ => true,
    }
}

/// Remove all persisted cache files (including legacy schema-v1 files).
pub fn clear_disk_cache(cache_dir: &Path) -> io::Result<()> {
    for name in [
        PEDIGREES_FILE,
        METADATA_FILE,
        LEGACY_FILES[0],
        LEGACY_FILES[1],
    ] {
        let path = cache_dir.join(name);
        if path.exists() {
            fs::remove_file(&path)?;
        }
    }
    info!(cache_dir = %cache_dir.display(), "Disk cache cleared");
    Ok(())
}

// ── Internal helpers ─────────────────────────────────────────────────────

/// Write a bincode-serialized value atomically (temp file → rename).
fn write_bincode_atomic<T: Serialize>(dir: &Path, filename: &str, value: &T) -> io::Result<()> {
    let final_path = dir.join(filename);
    let tmp_path = dir.join(format!("{filename}.tmp"));

    let file = fs::File::create(&tmp_path)?;
    let mut writer = BufWriter::new(file);
    bincode::serde::encode_into_std_write(value, &mut writer, bincode::config::standard())
        .map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Failed to serialize {filename}: {e}"),
            )
        })?;

    fs::rename(&tmp_path, &final_path)?;
    debug!(file = %final_path.display(), "Wrote cache file");
    Ok(())
}

/// Read and deserialize a bincode file, returning `None` on any error.
fn read_bincode<T: for<'de> Deserialize<'de>>(dir: &Path, filename: &str) -> Option<T> {
    let path = dir.join(filename);
    if !path.exists() {
        warn!(file = %path.display(), "Cache file missing");
        return None;
    }
    let file = fs::File::open(&path)
        .map_err(|e| warn!(file = %path.display(), "Failed to open cache file: {e}"))
        .ok()?;
    let mut reader = BufReader::new(file);
    bincode::serde::decode_from_std_read(&mut reader, bincode::config::standard())
        .map_err(|e| warn!(file = %path.display(), "Failed to deserialize cache file: {e}"))
        .ok()
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::CacheStore;
    use crate::store::memory::MemoryCacheStore;
    use chrono::Utc;
    use oxidgene_core::enums::Sex;
    use std::collections::HashMap;
    use uuid::Uuid;

    fn make_pedigree(tree_id: Uuid, root_id: Uuid) -> CachedPedigree {
        let mut persons = HashMap::new();
        persons.insert(
            root_id,
            PedigreeNode {
                person_id: root_id,
                display_name: "Root Person".into(),
                given_names: None,
                surname: None,
                sex: Sex::Male,
                birth_year: None,
                birth_place: None,
                death_year: None,
                death_place: None,
                occupation: None,
                primary_media_path: None,
                generation: 0,
                sosa_number: Some(1),
            },
        );
        CachedPedigree {
            tree_id,
            root_person_id: root_id,
            persons,
            edges: vec![],
            family_events: HashMap::new(),
            families: HashMap::new(),
            ancestor_depth_loaded: 4,
            descendant_depth_loaded: 2,
            cached_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn persist_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path();

        let store = MemoryCacheStore::new();
        let tree_id = Uuid::now_v7();
        let root_id = Uuid::now_v7();

        store
            .set_pedigree(&make_pedigree(tree_id, root_id))
            .await
            .unwrap();

        // Persist
        persist_to_disk(&store, cache_dir, None).unwrap();

        // Verify files exist
        assert!(cache_dir.join(PEDIGREES_FILE).exists());
        assert!(cache_dir.join(METADATA_FILE).exists());

        // Load into new store
        let loaded = load_from_disk(cache_dir, 64 * 1024 * 1024).unwrap();

        let loaded_ped = loaded.get_pedigree(tree_id, root_id).await.unwrap();
        assert!(loaded_ped.is_some());
        assert_eq!(loaded_ped.unwrap().root_person_id, root_id);
    }

    #[test]
    fn load_from_nonexistent_returns_none() {
        let result = load_from_disk(Path::new("/nonexistent/path"), 64 * 1024 * 1024);
        assert!(result.is_none());
    }

    #[test]
    fn legacy_schema_v1_is_discarded() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path();

        // Simulate a schema-v1 metadata file (persons + search still persisted).
        let meta = serde_json::json!({
            "persisted_at": Utc::now(),
            "schema_version": 1,
            "pedigree_count": 0,
            "db_path": null,
            "db_modified_at": null,
        });
        fs::write(cache_dir.join(METADATA_FILE), meta.to_string()).unwrap();

        assert!(load_from_disk(cache_dir, 64 * 1024 * 1024).is_none());
        assert!(is_cache_stale(cache_dir, Path::new("/some/db.sqlite")));
    }

    #[test]
    fn metadata_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path();

        let metadata = CacheMetadata {
            persisted_at: Utc::now(),
            schema_version: SCHEMA_VERSION,
            pedigree_count: 3,
            db_path: Some("/some/path/oxidgene.db".into()),
            db_modified_at: Some(Utc::now()),
        };

        let meta_path = cache_dir.join(METADATA_FILE);
        fs::write(&meta_path, serde_json::to_string_pretty(&metadata).unwrap()).unwrap();

        let loaded = load_metadata(cache_dir).unwrap();
        assert_eq!(loaded.schema_version, SCHEMA_VERSION);
        assert_eq!(loaded.pedigree_count, 3);
    }

    #[test]
    fn staleness_no_metadata_is_stale() {
        let dir = tempfile::tempdir().unwrap();
        assert!(is_cache_stale(dir.path(), Path::new("/some/db.sqlite")));
    }

    #[tokio::test]
    async fn clear_disk_cache_removes_files() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path();

        let store = MemoryCacheStore::new();
        persist_to_disk(&store, cache_dir, None).unwrap();

        // Leave a legacy v1 file around; clear must remove it too.
        fs::write(cache_dir.join("persons.bin"), b"legacy").unwrap();

        assert!(cache_dir.join(PEDIGREES_FILE).exists());
        clear_disk_cache(cache_dir).unwrap();
        assert!(!cache_dir.join(PEDIGREES_FILE).exists());
        assert!(!cache_dir.join(METADATA_FILE).exists());
        assert!(!cache_dir.join("persons.bin").exists());
    }
}
