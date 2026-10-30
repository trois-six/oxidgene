//! The opt-in harnesses of `oxidgene-geneanet`, run on real Geneanet data by
//! `just geneanet-harness`.
//!
//! One binary for the crate: Cargo builds every `tests/*.rs` file it finds
//! as an executable of its own, each linking the crate and its dependencies
//! again. The manifest turns that discovery off, so a file of `tests/` runs
//! only once declared below; the `test_targets` guard keeps it so.

mod phash_cost;
mod phash_real_session;
mod phash_separation;
mod unkeyed_references;
