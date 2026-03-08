//! Disk persistence for `MemoryCacheStore`.
//!
//! Serializes cache contents to bincode files on disk for fast startup on desktop.
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

const PERSONS_FILE: &str = "persons.bin";
const PEDIGREES_FILE: &str = "pedigrees.bin";
const SEARCH_INDEXES_FILE: &str = "search_indexes.bin";
const METADATA_FILE: &str = "cache_metadata.json";

// ── Serializable snapshot types ──────────────────────────────────────────

/// All cached persons keyed by `(tree_id, person_id)`.
#[derive(Serialize, Deserialize)]
struct PersonsSnapshot {
    entries: Vec<CachedPerson>,
}

/// All cached pedigrees.
#[derive(Serialize, Deserialize)]
struct PedigreesSnapshot {
    entries: Vec<CachedPedigree>,
}

/// All cached search indexes.
#[derive(Serialize, Deserialize)]
struct SearchIndexesSnapshot {
    entries: Vec<CachedSearchIndex>,
}

/// Metadata about the persisted cache — used for staleness detection.
#[derive(Debug, Serialize, Deserialize)]
pub struct CacheMetadata {
    /// When the cache was last persisted to disk.
    pub persisted_at: DateTime<Utc>,
    /// Schema version for forward compatibility.
    pub schema_version: u32,
    /// Number of persons cached.
    pub person_count: usize,
    /// Number of pedigrees cached.
    pub pedigree_count: usize,
    /// Number of search indexes cached.
    pub search_index_count: usize,
    /// Optional: path to the SQLite DB at persistence time (for staleness check).
    pub db_path: Option<String>,
    /// Optional: last-modified timestamp of the SQLite DB file.
    pub db_modified_at: Option<DateTime<Utc>>,
}

/// Current schema version. Bump when snapshot format changes.
const SCHEMA_VERSION: u32 = 1;

// ── Public API ───────────────────────────────────────────────────────────

/// Resolve the platform-native cache directory for OxidGene.
///
/// Returns `None` if the platform has no standard cache directory.
pub fn default_cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("oxidgene"))
}

/// Persist all in-memory cache contents to disk as bincode files.
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

    let (persons, pedigrees, search_indexes) = store.snapshot_for_disk();

    let person_count = persons.len();
    let pedigree_count = pedigrees.len();
    let search_index_count = search_indexes.len();

    // Persist persons
    write_bincode_atomic(
        cache_dir,
        PERSONS_FILE,
        &PersonsSnapshot { entries: persons },
    )?;

    // Persist pedigrees
    write_bincode_atomic(
        cache_dir,
        PEDIGREES_FILE,
        &PedigreesSnapshot { entries: pedigrees },
    )?;

    // Persist search indexes
    write_bincode_atomic(
        cache_dir,
        SEARCH_INDEXES_FILE,
        &SearchIndexesSnapshot {
            entries: search_indexes,
        },
    )?;

    // Persist metadata (JSON for human readability)
    let db_modified_at = db_path
        .and_then(|p| fs::metadata(p).ok())
        .and_then(|m| m.modified().ok())
        .map(DateTime::<Utc>::from);

    let metadata = CacheMetadata {
        persisted_at: Utc::now(),
        schema_version: SCHEMA_VERSION,
        person_count,
        pedigree_count,
        search_index_count,
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

    info!(
        person_count,
        pedigree_count,
        search_index_count,
        cache_dir = %cache_dir.display(),
        "Cache persisted to disk"
    );

    Ok(())
}

/// Load cache contents from disk into a new `MemoryCacheStore`.
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

    // Load persons
    let persons: PersonsSnapshot = read_bincode(cache_dir, PERSONS_FILE)?;

    // Load pedigrees
    let pedigrees: PedigreesSnapshot = read_bincode(cache_dir, PEDIGREES_FILE)?;

    // Load search indexes
    let search_indexes: SearchIndexesSnapshot = read_bincode(cache_dir, SEARCH_INDEXES_FILE)?;

    let store = MemoryCacheStore::from_disk_snapshot(
        persons.entries,
        pedigrees.entries,
        search_indexes.entries,
        pedigree_budget_bytes,
    );

    info!(
        persons = metadata.person_count,
        pedigrees = metadata.pedigree_count,
        search_indexes = metadata.search_index_count,
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

/// Remove all persisted cache files.
pub fn clear_disk_cache(cache_dir: &Path) -> io::Result<()> {
    for name in [
        PERSONS_FILE,
        PEDIGREES_FILE,
        SEARCH_INDEXES_FILE,
        METADATA_FILE,
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
    let writer = BufWriter::new(file);
    bincode::serialize_into(writer, value).map_err(|e| {
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
    let reader = BufReader::new(file);
    bincode::deserialize_from(reader)
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
    use oxidgene_core::enums::{NameType, Sex};
    use std::collections::HashMap;
    use uuid::Uuid;

    fn make_person(tree_id: Uuid, person_id: Uuid) -> CachedPerson {
        CachedPerson {
            person_id,
            tree_id,
            sex: Sex::Male,
            primary_name: Some(CachedName {
                name_id: Uuid::now_v7(),
                name_type: NameType::Birth,
                display_name: "John Doe".into(),
                given_names: Some("John".into()),
                surname: Some("Doe".into()),
            }),
            other_names: vec![],
            birth: None,
            death: None,
            baptism: None,
            burial: None,
            occupation: None,
            other_events: vec![],
            families_as_spouse: vec![],
            family_as_child: None,
            primary_media: None,
            media_count: 0,
            citation_count: 0,
            note_count: 0,
            updated_at: Utc::now(),
            cached_at: Utc::now(),
        }
    }

    fn make_pedigree(tree_id: Uuid, root_id: Uuid) -> CachedPedigree {
        let mut persons = HashMap::new();
        persons.insert(
            root_id,
            PedigreeNode {
                person_id: root_id,
                display_name: "Root Person".into(),
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
            ancestor_depth_loaded: 4,
            descendant_depth_loaded: 2,
            cached_at: Utc::now(),
        }
    }

    fn make_search_index(tree_id: Uuid) -> CachedSearchIndex {
        CachedSearchIndex {
            tree_id,
            entries: vec![SearchEntry {
                person_id: Uuid::now_v7(),
                display_name: "John Doe".into(),
                sex: Sex::Male,
                surname_normalized: "doe".into(),
                given_names_normalized: "john".into(),
                maiden_name_normalized: None,
                birth_year: Some("1900".into()),
                birth_place: None,
                death_year: Some("1970".into()),
                date_sort: None,
            }],
            cached_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn persist_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path();

        let store = MemoryCacheStore::new();
        let tree_id = Uuid::now_v7();
        let p1 = Uuid::now_v7();
        let p2 = Uuid::now_v7();

        // Populate store
        store.set_person(&make_person(tree_id, p1)).await.unwrap();
        store.set_person(&make_person(tree_id, p2)).await.unwrap();
        store
            .set_pedigree(&make_pedigree(tree_id, p1))
            .await
            .unwrap();
        store
            .set_search_index(&make_search_index(tree_id))
            .await
            .unwrap();

        // Persist
        persist_to_disk(&store, cache_dir, None).unwrap();

        // Verify files exist
        assert!(cache_dir.join(PERSONS_FILE).exists());
        assert!(cache_dir.join(PEDIGREES_FILE).exists());
        assert!(cache_dir.join(SEARCH_INDEXES_FILE).exists());
        assert!(cache_dir.join(METADATA_FILE).exists());

        // Load into new store
        let loaded = load_from_disk(cache_dir, 64 * 1024 * 1024).unwrap();

        // Verify persons
        let loaded_p1 = loaded.get_person(tree_id, p1).await.unwrap();
        assert!(loaded_p1.is_some());
        assert_eq!(loaded_p1.unwrap().person_id, p1);

        let loaded_p2 = loaded.get_person(tree_id, p2).await.unwrap();
        assert!(loaded_p2.is_some());

        // Verify pedigree
        let loaded_ped = loaded.get_pedigree(tree_id, p1).await.unwrap();
        assert!(loaded_ped.is_some());
        assert_eq!(loaded_ped.unwrap().root_person_id, p1);

        // Verify search index
        let loaded_idx = loaded.get_search_index(tree_id).await.unwrap();
        assert!(loaded_idx.is_some());
        assert_eq!(loaded_idx.unwrap().entries.len(), 1);
    }

    #[test]
    fn load_from_nonexistent_returns_none() {
        let result = load_from_disk(Path::new("/nonexistent/path"), 64 * 1024 * 1024);
        assert!(result.is_none());
    }

    #[test]
    fn metadata_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path();

        let metadata = CacheMetadata {
            persisted_at: Utc::now(),
            schema_version: SCHEMA_VERSION,
            person_count: 42,
            pedigree_count: 3,
            search_index_count: 1,
            db_path: Some("/some/path/oxidgene.db".into()),
            db_modified_at: Some(Utc::now()),
        };

        let meta_path = cache_dir.join(METADATA_FILE);
        fs::write(&meta_path, serde_json::to_string_pretty(&metadata).unwrap()).unwrap();

        let loaded = load_metadata(cache_dir).unwrap();
        assert_eq!(loaded.schema_version, SCHEMA_VERSION);
        assert_eq!(loaded.person_count, 42);
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

        assert!(cache_dir.join(PERSONS_FILE).exists());
        clear_disk_cache(cache_dir).unwrap();
        assert!(!cache_dir.join(PERSONS_FILE).exists());
        assert!(!cache_dir.join(METADATA_FILE).exists());
    }
}
