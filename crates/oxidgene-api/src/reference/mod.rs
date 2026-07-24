//! Static reference content — occupation sheets and given-name meanings —
//! served read-only under `/api/v1/reference`. Not tied to any tree.
//!
//! Source JSON lives in `data/` (one file per language per data type),
//! gzip-compressed at build time (see `build.rs`) and decompressed once,
//! on first lookup, into an in-memory table (see `loader.rs`).

mod loader;

pub use loader::{
    GivenNameEntry, OccupationEntry, ReferenceLang, lookup_given_name, lookup_occupation,
};
