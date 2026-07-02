//! `CacheStore` trait — abstraction over storage backends.

#[cfg(feature = "disk")]
pub mod disk;
pub mod memory;
#[cfg(feature = "redis")]
pub mod redis_store;

use crate::types::*;
use async_trait::async_trait;
use oxidgene_core::error::OxidGeneError;
use std::any::Any;
use uuid::Uuid;

/// Trait abstracting over the cache storage backend (Redis, DashMap, etc.).
///
/// All methods are async to support both in-memory and network-based backends.
/// Methods return `Result` to allow network backends (e.g., Redis) to propagate
/// connection errors.
#[async_trait]
pub trait CacheStore: Send + Sync {
    /// Downcast helper — required for disk persistence (downcasting `dyn CacheStore`
    /// back to `MemoryCacheStore` to call `snapshot_for_disk()`).
    fn as_any(&self) -> &dyn Any;

    /// Whether this store actually persists `CachedPerson` entries.
    ///
    /// The desktop `MemoryCacheStore` returns `false` (Sprint E.6): SQLite is
    /// local, so persons are built on demand with targeted queries instead of
    /// being cached. `CacheService` uses this flag to skip pointless cache
    /// round-trips and to fetch tree data once when building pedigrees.
    fn caches_persons(&self) -> bool {
        true
    }

    // ── PersonCache ──────────────────────────────────────────────────────

    /// Get a single cached person.
    async fn get_person(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
    ) -> Result<Option<CachedPerson>, OxidGeneError>;

    /// Store a single cached person.
    async fn set_person(&self, entry: &CachedPerson) -> Result<(), OxidGeneError>;

    /// Store multiple cached persons at once.
    async fn set_persons_batch(&self, entries: &[CachedPerson]) -> Result<(), OxidGeneError>;

    /// Remove a single cached person.
    async fn delete_person(&self, tree_id: Uuid, person_id: Uuid) -> Result<(), OxidGeneError>;

    /// Get multiple cached persons at once. Returns only the ones found.
    async fn get_persons_batch(
        &self,
        tree_id: Uuid,
        person_ids: &[Uuid],
    ) -> Result<Vec<CachedPerson>, OxidGeneError>;

    /// Get all cached persons for a tree.
    async fn get_all_persons(&self, tree_id: Uuid) -> Result<Vec<CachedPerson>, OxidGeneError>;

    // ── PedigreeCache ────────────────────────────────────────────────────

    /// Get a pedigree cache for a given root person.
    async fn get_pedigree(
        &self,
        tree_id: Uuid,
        root_id: Uuid,
    ) -> Result<Option<CachedPedigree>, OxidGeneError>;

    /// Store a pedigree cache.
    async fn set_pedigree(&self, entry: &CachedPedigree) -> Result<(), OxidGeneError>;

    /// Remove a single pedigree cache.
    async fn delete_pedigree(&self, tree_id: Uuid, root_id: Uuid) -> Result<(), OxidGeneError>;

    /// Remove all pedigree caches for a tree.
    async fn delete_all_pedigrees(&self, tree_id: Uuid) -> Result<(), OxidGeneError>;

    // ── Bulk ─────────────────────────────────────────────────────────────

    /// Remove all caches (person, pedigree) for a tree.
    ///
    /// The search index is not handled here — it lives in the database
    /// (`person_search_fts`) since Sprint E.6 and is invalidated by
    /// `CacheService` via `PersonSearchRepo`.
    async fn invalidate_tree(&self, tree_id: Uuid) -> Result<(), OxidGeneError>;
}
