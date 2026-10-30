//! The integration tests of `oxidgene-db`: repositories and migrations
//! against a real database.
//!
//! One binary for the crate: Cargo builds every `tests/*.rs` file it finds
//! as an executable of its own, each linking the crate and its dependencies
//! again. The manifest turns that discovery off, so a file of `tests/` runs
//! only once declared below; the `test_targets` guard keeps it so.

mod gedcom_kept_test;
mod media_invariants_test;
mod migration_test;
mod person_search_test;
mod repo_test;
mod tag_test;
