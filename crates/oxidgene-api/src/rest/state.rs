//! Shared application state for Axum handlers.

use oxidgene_cache::CacheService;
use oxidgene_cache::store::memory::MemoryCacheStore;
use sea_orm::DatabaseConnection;
use std::sync::Arc;
use tracing::info;

/// Default pedigree LRU budget in bytes (64 MB).
const DEFAULT_PEDIGREE_BUDGET_BYTES: usize = 64 * 1024 * 1024;

/// Shared state available to all Axum handlers.
#[derive(Debug, Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub cache: Arc<CacheService>,
}

impl AppState {
    /// Create a new `AppState` with automatic backend detection.
    ///
    /// Reads environment variables to decide which cache backend to use:
    /// - `OXIDGENE_CACHE_BACKEND`: `"redis"` or `"memory"` (default: `"memory"`)
    /// - `OXIDGENE_REDIS_URL`: Redis connection URL (default: `redis://127.0.0.1:6379`)
    /// - `OXIDGENE_PEDIGREE_CACHE_MB`: pedigree LRU budget in MB (default: 64)
    ///
    /// Falls back to in-memory if Redis is requested but connection fails.
    pub fn new(db: DatabaseConnection) -> Self {
        let budget = pedigree_budget_bytes();
        let backend = std::env::var("OXIDGENE_CACHE_BACKEND").unwrap_or_else(|_| "memory".into());

        let store: Arc<dyn oxidgene_cache::CacheStore> = match backend.as_str() {
            #[cfg(feature = "redis")]
            "redis" => match create_redis_store() {
                Ok(store) => {
                    info!("Cache backend: Redis");
                    Arc::new(store)
                }
                Err(e) => {
                    tracing::warn!("Redis connection failed ({e}), falling back to in-memory");
                    Arc::new(MemoryCacheStore::with_budget(budget))
                }
            },
            #[cfg(not(feature = "redis"))]
            "redis" => {
                tracing::warn!(
                    "Redis backend requested but `redis` feature not enabled, using in-memory"
                );
                Arc::new(MemoryCacheStore::with_budget(budget))
            }
            _ => {
                info!(
                    budget_mb = budget / (1024 * 1024),
                    "Cache backend: in-memory"
                );
                Arc::new(MemoryCacheStore::with_budget(budget))
            }
        };

        let cache = Arc::new(CacheService::new(store, db.clone()));
        Self { db, cache }
    }

    /// Create a new `AppState` with a pre-built `MemoryCacheStore`.
    ///
    /// Used by the desktop binary to load the cache from disk before starting.
    pub fn with_memory_store(db: DatabaseConnection, store: MemoryCacheStore) -> Self {
        let cache = Arc::new(CacheService::new(Arc::new(store), db.clone()));
        Self { db, cache }
    }

    /// Create a new `AppState` with an explicit cache service (for testing or
    /// alternative backends).
    pub fn with_cache(db: DatabaseConnection, cache: Arc<CacheService>) -> Self {
        Self { db, cache }
    }
}

/// Read the pedigree LRU budget from the environment.
fn pedigree_budget_bytes() -> usize {
    std::env::var("OXIDGENE_PEDIGREE_CACHE_MB")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .map(|mb| mb * 1024 * 1024)
        .unwrap_or(DEFAULT_PEDIGREE_BUDGET_BYTES)
}

/// Create a Redis cache store from environment variables.
#[cfg(feature = "redis")]
fn create_redis_store() -> Result<oxidgene_cache::RedisCacheStore, String> {
    let url =
        std::env::var("OXIDGENE_REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".into());
    info!(url = %url, "Connecting to Redis");

    // Use tokio's current runtime to create the connection
    let rt = tokio::runtime::Handle::current();
    rt.block_on(async {
        oxidgene_cache::RedisCacheStore::new(&url)
            .await
            .map_err(|e| format!("{e}"))
    })
}
