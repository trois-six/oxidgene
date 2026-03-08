//! OxidGene server-side cache layer.
//!
//! Provides three caches for instant page rendering:
//! - **PersonCache** — per-person denormalized profile
//! - **PedigreeCache** — windowed tree display
//! - **SearchIndex** — per-tree normalized search index
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
