//! The integration tests of `oxidgene-gedcom`: GEDCOM import, export and
//! the conformance round trip.
//!
//! One binary for the crate: Cargo builds every `tests/*.rs` file it finds
//! as an executable of its own, each linking the crate and its dependencies
//! again. The manifest turns that discovery off, so a file of `tests/` runs
//! only once declared below; the `test_targets` guard keeps it so.

mod conformance_round_trip;
mod export_fidelity;
mod gedcom_test;
