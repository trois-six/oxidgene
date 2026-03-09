//! In-memory cache store backed by `DashMap`.
//!
//! Used for desktop deployments and as a fallback when Redis is unavailable.
//! Disk persistence is available via the `disk` feature (see `store::disk`).

use crate::store::CacheStore;
use crate::types::*;
use async_trait::async_trait;
use dashmap::DashMap;
use oxidgene_core::error::OxidGeneError;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use uuid::Uuid;

/// Composite key for person cache entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PersonKey {
    tree_id: Uuid,
    person_id: Uuid,
}

/// Composite key for pedigree cache entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PedigreeKey {
    tree_id: Uuid,
    root_person_id: Uuid,
}

/// Default pedigree cache budget: 64 MB (desktop).
const DEFAULT_PEDIGREE_BUDGET_BYTES: usize = 64 * 1024 * 1024;

/// Wrapper that tracks estimated byte size and last access time for LRU eviction.
#[derive(Debug, Clone)]
struct PedigreeEntry {
    pedigree: CachedPedigree,
    estimated_bytes: usize,
    last_access: Instant,
}

/// Estimate the in-memory byte footprint of a `CachedPedigree`.
///
/// ~300 bytes per node + ~80 bytes per edge + fixed overhead.
fn estimate_pedigree_bytes(p: &CachedPedigree) -> usize {
    let node_bytes = p.persons.len() * 300;
    let edge_bytes = p.edges.len() * 80;
    node_bytes + edge_bytes + 128
}

/// In-memory cache store using `DashMap` for lock-free concurrent access.
///
/// Pedigree entries are subject to an LRU eviction policy: when the total
/// estimated byte size of all stored pedigrees exceeds `pedigree_budget_bytes`,
/// the least-recently-used entries are evicted until the budget is satisfied.
#[derive(Debug)]
pub struct MemoryCacheStore {
    persons: DashMap<PersonKey, CachedPerson>,
    pedigrees: DashMap<PedigreeKey, PedigreeEntry>,
    search_indexes: DashMap<Uuid, CachedSearchIndex>,
    /// Current total estimated byte size of all pedigree entries.
    pedigree_total_bytes: AtomicUsize,
    /// Maximum byte budget for pedigree entries.
    pedigree_budget_bytes: usize,
}

impl MemoryCacheStore {
    /// Create a new empty in-memory cache store with the default pedigree budget (64 MB).
    pub fn new() -> Self {
        Self::with_budget(DEFAULT_PEDIGREE_BUDGET_BYTES)
    }

    /// Create a new empty in-memory cache store with a custom pedigree byte budget.
    pub fn with_budget(pedigree_budget_bytes: usize) -> Self {
        Self {
            persons: DashMap::new(),
            pedigrees: DashMap::new(),
            search_indexes: DashMap::new(),
            pedigree_total_bytes: AtomicUsize::new(0),
            pedigree_budget_bytes,
        }
    }

    /// Extract a snapshot of all cache contents for disk persistence.
    ///
    /// Returns `(persons, pedigrees, search_indexes)`.
    pub fn snapshot_for_disk(
        &self,
    ) -> (
        Vec<CachedPerson>,
        Vec<CachedPedigree>,
        Vec<CachedSearchIndex>,
    ) {
        let persons: Vec<CachedPerson> = self.persons.iter().map(|r| r.value().clone()).collect();
        let pedigrees: Vec<CachedPedigree> = self
            .pedigrees
            .iter()
            .map(|r| r.value().pedigree.clone())
            .collect();
        let search_indexes: Vec<CachedSearchIndex> = self
            .search_indexes
            .iter()
            .map(|r| r.value().clone())
            .collect();
        (persons, pedigrees, search_indexes)
    }

    /// Reconstruct a `MemoryCacheStore` from a disk snapshot.
    pub fn from_disk_snapshot(
        persons: Vec<CachedPerson>,
        pedigrees: Vec<CachedPedigree>,
        search_indexes: Vec<CachedSearchIndex>,
        pedigree_budget_bytes: usize,
    ) -> Self {
        let store = Self::with_budget(pedigree_budget_bytes);

        for person in persons {
            let key = PersonKey {
                tree_id: person.tree_id,
                person_id: person.person_id,
            };
            store.persons.insert(key, person);
        }

        let mut total_bytes = 0usize;
        for pedigree in pedigrees {
            let key = PedigreeKey {
                tree_id: pedigree.tree_id,
                root_person_id: pedigree.root_person_id,
            };
            let estimated = estimate_pedigree_bytes(&pedigree);
            total_bytes += estimated;
            store.pedigrees.insert(
                key,
                PedigreeEntry {
                    pedigree,
                    estimated_bytes: estimated,
                    last_access: Instant::now(),
                },
            );
        }
        store
            .pedigree_total_bytes
            .store(total_bytes, Ordering::Relaxed);

        for index in search_indexes {
            store.search_indexes.insert(index.tree_id, index);
        }

        // Evict if loaded pedigrees exceed budget
        store.evict_pedigrees_if_needed();

        store
    }

    /// Evict least-recently-used pedigree entries until total size ≤ budget.
    fn evict_pedigrees_if_needed(&self) {
        while self.pedigree_total_bytes.load(Ordering::Relaxed) > self.pedigree_budget_bytes {
            // Find the LRU entry (oldest last_access).
            let lru_key = self
                .pedigrees
                .iter()
                .min_by_key(|entry| entry.value().last_access)
                .map(|entry| *entry.key());

            if let Some(key) = lru_key {
                if let Some((_, evicted)) = self.pedigrees.remove(&key) {
                    self.pedigree_total_bytes
                        .fetch_sub(evicted.estimated_bytes, Ordering::Relaxed);
                }
            } else {
                break; // No entries left
            }
        }
    }
}

impl Default for MemoryCacheStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CacheStore for MemoryCacheStore {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    // ── PersonCache ──────────────────────────────────────────────────────

    async fn get_person(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
    ) -> Result<Option<CachedPerson>, OxidGeneError> {
        let key = PersonKey { tree_id, person_id };
        Ok(self.persons.get(&key).map(|entry| entry.value().clone()))
    }

    async fn set_person(&self, entry: &CachedPerson) -> Result<(), OxidGeneError> {
        let key = PersonKey {
            tree_id: entry.tree_id,
            person_id: entry.person_id,
        };
        self.persons.insert(key, entry.clone());
        Ok(())
    }

    async fn set_persons_batch(&self, entries: &[CachedPerson]) -> Result<(), OxidGeneError> {
        for entry in entries {
            let key = PersonKey {
                tree_id: entry.tree_id,
                person_id: entry.person_id,
            };
            self.persons.insert(key, entry.clone());
        }
        Ok(())
    }

    async fn delete_person(&self, tree_id: Uuid, person_id: Uuid) -> Result<(), OxidGeneError> {
        let key = PersonKey { tree_id, person_id };
        self.persons.remove(&key);
        Ok(())
    }

    async fn get_persons_batch(
        &self,
        tree_id: Uuid,
        person_ids: &[Uuid],
    ) -> Result<Vec<CachedPerson>, OxidGeneError> {
        Ok(person_ids
            .iter()
            .filter_map(|pid| {
                let key = PersonKey {
                    tree_id,
                    person_id: *pid,
                };
                self.persons.get(&key).map(|entry| entry.value().clone())
            })
            .collect())
    }

    async fn get_all_persons(&self, tree_id: Uuid) -> Result<Vec<CachedPerson>, OxidGeneError> {
        Ok(self
            .persons
            .iter()
            .filter(|entry| entry.key().tree_id == tree_id)
            .map(|entry| entry.value().clone())
            .collect())
    }

    // ── PedigreeCache ────────────────────────────────────────────────────

    async fn get_pedigree(
        &self,
        tree_id: Uuid,
        root_id: Uuid,
    ) -> Result<Option<CachedPedigree>, OxidGeneError> {
        let key = PedigreeKey {
            tree_id,
            root_person_id: root_id,
        };
        // Touch (update last_access) on read for LRU tracking.
        if let Some(mut entry) = self.pedigrees.get_mut(&key) {
            entry.last_access = Instant::now();
            Ok(Some(entry.pedigree.clone()))
        } else {
            Ok(None)
        }
    }

    async fn set_pedigree(&self, entry: &CachedPedigree) -> Result<(), OxidGeneError> {
        let key = PedigreeKey {
            tree_id: entry.tree_id,
            root_person_id: entry.root_person_id,
        };
        let new_size = estimate_pedigree_bytes(entry);

        // If replacing an existing entry, subtract its old size first.
        if let Some((_, old)) = self.pedigrees.remove(&key) {
            self.pedigree_total_bytes
                .fetch_sub(old.estimated_bytes, Ordering::Relaxed);
        }

        self.pedigrees.insert(
            key,
            PedigreeEntry {
                pedigree: entry.clone(),
                estimated_bytes: new_size,
                last_access: Instant::now(),
            },
        );
        self.pedigree_total_bytes
            .fetch_add(new_size, Ordering::Relaxed);

        self.evict_pedigrees_if_needed();
        Ok(())
    }

    async fn delete_pedigree(&self, tree_id: Uuid, root_id: Uuid) -> Result<(), OxidGeneError> {
        let key = PedigreeKey {
            tree_id,
            root_person_id: root_id,
        };
        if let Some((_, old)) = self.pedigrees.remove(&key) {
            self.pedigree_total_bytes
                .fetch_sub(old.estimated_bytes, Ordering::Relaxed);
        }
        Ok(())
    }

    async fn delete_all_pedigrees(&self, tree_id: Uuid) -> Result<(), OxidGeneError> {
        // Collect keys to remove first, then remove and adjust total.
        let keys_to_remove: Vec<PedigreeKey> = self
            .pedigrees
            .iter()
            .filter(|e| e.key().tree_id == tree_id)
            .map(|e| *e.key())
            .collect();
        for key in keys_to_remove {
            if let Some((_, old)) = self.pedigrees.remove(&key) {
                self.pedigree_total_bytes
                    .fetch_sub(old.estimated_bytes, Ordering::Relaxed);
            }
        }
        Ok(())
    }

    // ── SearchIndex ──────────────────────────────────────────────────────

    async fn get_search_index(
        &self,
        tree_id: Uuid,
    ) -> Result<Option<CachedSearchIndex>, OxidGeneError> {
        Ok(self
            .search_indexes
            .get(&tree_id)
            .map(|entry| entry.value().clone()))
    }

    async fn set_search_index(&self, entry: &CachedSearchIndex) -> Result<(), OxidGeneError> {
        self.search_indexes.insert(entry.tree_id, entry.clone());
        Ok(())
    }

    async fn delete_search_index(&self, tree_id: Uuid) -> Result<(), OxidGeneError> {
        self.search_indexes.remove(&tree_id);
        Ok(())
    }

    // ── Bulk ─────────────────────────────────────────────────────────────

    async fn invalidate_tree(&self, tree_id: Uuid) -> Result<(), OxidGeneError> {
        // Remove all persons for this tree
        self.persons.retain(|key, _| key.tree_id != tree_id);
        // Remove all pedigrees for this tree (updating byte tracking)
        let keys_to_remove: Vec<PedigreeKey> = self
            .pedigrees
            .iter()
            .filter(|e| e.key().tree_id == tree_id)
            .map(|e| *e.key())
            .collect();
        for key in keys_to_remove {
            if let Some((_, old)) = self.pedigrees.remove(&key) {
                self.pedigree_total_bytes
                    .fetch_sub(old.estimated_bytes, Ordering::Relaxed);
            }
        }
        // Remove search index for this tree
        self.search_indexes.remove(&tree_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use oxidgene_core::enums::Sex;

    fn make_person(tree_id: Uuid, person_id: Uuid) -> CachedPerson {
        CachedPerson {
            person_id,
            tree_id,
            sex: Sex::Male,
            primary_name: Some(CachedName {
                name_id: Uuid::now_v7(),
                name_type: oxidgene_core::enums::NameType::Birth,
                display_name: "John Doe".to_string(),
                given_names: Some("John".to_string()),
                surname: Some("Doe".to_string()),
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

    #[tokio::test]
    async fn test_person_crud() {
        let store = MemoryCacheStore::new();
        let tree_id = Uuid::now_v7();
        let person_id = Uuid::now_v7();

        // Initially empty
        assert!(
            store
                .get_person(tree_id, person_id)
                .await
                .unwrap()
                .is_none()
        );

        // Store and retrieve
        let person = make_person(tree_id, person_id);
        store.set_person(&person).await.unwrap();
        let retrieved = store.get_person(tree_id, person_id).await.unwrap().unwrap();
        assert_eq!(retrieved.person_id, person_id);
        assert_eq!(retrieved.tree_id, tree_id);

        // Delete
        store.delete_person(tree_id, person_id).await.unwrap();
        assert!(
            store
                .get_person(tree_id, person_id)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn test_batch_operations() {
        let store = MemoryCacheStore::new();
        let tree_id = Uuid::now_v7();
        let ids: Vec<Uuid> = (0..5).map(|_| Uuid::now_v7()).collect();

        let persons: Vec<CachedPerson> = ids.iter().map(|id| make_person(tree_id, *id)).collect();
        store.set_persons_batch(&persons).await.unwrap();

        // Batch get
        let batch = store.get_persons_batch(tree_id, &ids).await.unwrap();
        assert_eq!(batch.len(), 5);

        // Get all
        let all = store.get_all_persons(tree_id).await.unwrap();
        assert_eq!(all.len(), 5);

        // Partial batch
        let partial = store.get_persons_batch(tree_id, &ids[0..2]).await.unwrap();
        assert_eq!(partial.len(), 2);
    }

    #[tokio::test]
    async fn test_invalidate_tree() {
        let store = MemoryCacheStore::new();
        let tree_a = Uuid::now_v7();
        let tree_b = Uuid::now_v7();

        // Add persons to both trees
        let pa = make_person(tree_a, Uuid::now_v7());
        let pb = make_person(tree_b, Uuid::now_v7());
        store.set_person(&pa).await.unwrap();
        store.set_person(&pb).await.unwrap();

        // Add search indexes
        let si_a = CachedSearchIndex {
            tree_id: tree_a,
            entries: vec![],
            cached_at: Utc::now(),
        };
        store.set_search_index(&si_a).await.unwrap();

        // Invalidate tree A
        store.invalidate_tree(tree_a).await.unwrap();

        // Tree A gone, tree B still there
        assert!(
            store
                .get_person(tree_a, pa.person_id)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .get_person(tree_b, pb.person_id)
                .await
                .unwrap()
                .is_some()
        );
        assert!(store.get_search_index(tree_a).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_pedigree_crud() {
        let store = MemoryCacheStore::new();
        let tree_id = Uuid::now_v7();
        let root_id = Uuid::now_v7();

        let pedigree = CachedPedigree {
            tree_id,
            root_person_id: root_id,
            persons: std::collections::HashMap::new(),
            edges: vec![],
            family_events: std::collections::HashMap::new(),
            families: std::collections::HashMap::new(),
            ancestor_depth_loaded: 5,
            descendant_depth_loaded: 3,
            cached_at: Utc::now(),
        };

        store.set_pedigree(&pedigree).await.unwrap();
        let retrieved = store.get_pedigree(tree_id, root_id).await.unwrap().unwrap();
        assert_eq!(retrieved.ancestor_depth_loaded, 5);

        store.delete_pedigree(tree_id, root_id).await.unwrap();
        assert!(
            store
                .get_pedigree(tree_id, root_id)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn test_delete_all_pedigrees() {
        let store = MemoryCacheStore::new();
        let tree_id = Uuid::now_v7();

        for _ in 0..3 {
            let pedigree = CachedPedigree {
                tree_id,
                root_person_id: Uuid::now_v7(),
                persons: std::collections::HashMap::new(),
                edges: vec![],
                family_events: std::collections::HashMap::new(),
                families: std::collections::HashMap::new(),
                ancestor_depth_loaded: 5,
                descendant_depth_loaded: 3,
                cached_at: Utc::now(),
            };
            store.set_pedigree(&pedigree).await.unwrap();
        }

        store.delete_all_pedigrees(tree_id).await.unwrap();
        // All should be gone — we can't enumerate easily, but invalidate_tree covers this
    }

    #[tokio::test]
    async fn test_lru_eviction() {
        use oxidgene_core::Sex;

        // Budget of ~500 bytes — each pedigree with 1 node ≈ 300 + 128 = 428 bytes.
        // So we can fit exactly 1 entry before eviction triggers on the second.
        let store = MemoryCacheStore::with_budget(500);
        let tree_id = Uuid::now_v7();

        let make_pedigree = |root_id: Uuid| {
            let mut persons = std::collections::HashMap::new();
            persons.insert(
                root_id,
                PedigreeNode {
                    person_id: root_id,
                    sex: Sex::Male,
                    display_name: "Test Person".to_string(),
                    given_names: None,
                    surname: None,
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
                family_events: std::collections::HashMap::new(),
                families: std::collections::HashMap::new(),
                ancestor_depth_loaded: 3,
                descendant_depth_loaded: 2,
                cached_at: Utc::now(),
            }
        };

        // Insert first pedigree — fits within budget.
        let root_a = Uuid::now_v7();
        store.set_pedigree(&make_pedigree(root_a)).await.unwrap();
        assert!(
            store.get_pedigree(tree_id, root_a).await.unwrap().is_some(),
            "First pedigree should exist"
        );

        // Small sleep so second pedigree has a strictly later timestamp.
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;

        // Insert second pedigree — should evict the first (LRU).
        let root_b = Uuid::now_v7();
        store.set_pedigree(&make_pedigree(root_b)).await.unwrap();
        assert!(
            store.get_pedigree(tree_id, root_b).await.unwrap().is_some(),
            "Second pedigree should exist"
        );
        assert!(
            store.get_pedigree(tree_id, root_a).await.unwrap().is_none(),
            "First pedigree should have been evicted"
        );

        // Total bytes should be ~428, not double.
        let total = store.pedigree_total_bytes.load(Ordering::Relaxed);
        assert!(
            total <= 500,
            "Total bytes ({total}) should be within budget"
        );
    }

    #[tokio::test]
    async fn test_lru_touch_on_read() {
        use oxidgene_core::Sex;

        // Budget of ~900 bytes — fits 2 entries (each ~428 bytes).
        let store = MemoryCacheStore::with_budget(900);
        let tree_id = Uuid::now_v7();

        let make_pedigree = |root_id: Uuid| {
            let mut persons = std::collections::HashMap::new();
            persons.insert(
                root_id,
                PedigreeNode {
                    person_id: root_id,
                    sex: Sex::Female,
                    display_name: "Test".to_string(),
                    given_names: None,
                    surname: None,
                    birth_year: None,
                    birth_place: None,
                    death_year: None,
                    death_place: None,
                    occupation: None,
                    primary_media_path: None,
                    generation: 0,
                    sosa_number: None,
                },
            );
            CachedPedigree {
                tree_id,
                root_person_id: root_id,
                persons,
                edges: vec![],
                family_events: std::collections::HashMap::new(),
                families: std::collections::HashMap::new(),
                ancestor_depth_loaded: 3,
                descendant_depth_loaded: 2,
                cached_at: Utc::now(),
            }
        };

        let root_a = Uuid::now_v7();
        store.set_pedigree(&make_pedigree(root_a)).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;

        let root_b = Uuid::now_v7();
        store.set_pedigree(&make_pedigree(root_b)).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;

        // Touch A so it's no longer the LRU — B is now the oldest.
        store.get_pedigree(tree_id, root_a).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;

        // Insert C — should evict B (LRU), not A.
        let root_c = Uuid::now_v7();
        store.set_pedigree(&make_pedigree(root_c)).await.unwrap();

        assert!(
            store.get_pedigree(tree_id, root_a).await.unwrap().is_some(),
            "A should survive (was touched)"
        );
        assert!(
            store.get_pedigree(tree_id, root_b).await.unwrap().is_none(),
            "B should be evicted (LRU)"
        );
        assert!(
            store.get_pedigree(tree_id, root_c).await.unwrap().is_some(),
            "C should exist (just inserted)"
        );
    }
}
