//! OxidGene server-side cache layer.
//!
//! Provides caches for instant page rendering:
//! - **PersonCache** — per-person denormalized profile (Redis / web only;
//!   on desktop persons are rebuilt on demand from local SQLite)
//! - **PedigreeCache** — windowed tree display (all backends)
//!
//! Search is DB-native since Sprint E.6: it goes through the
//! `person_search_fts` table (SQLite FTS5 virtual table / plain PostgreSQL
//! table), maintained by `CacheService` on every mutation.
//!
//! See `docs/specifications/caching.md` for the full architecture.

pub mod builder;
pub mod invalidation;
pub mod service;
pub mod store;
pub mod types;

pub use service::CacheService;
pub use store::CacheStore;
pub use store::memory::MemoryCacheStore;
#[cfg(feature = "redis")]
pub use store::redis_store::RedisCacheStore;
pub use types::*;
