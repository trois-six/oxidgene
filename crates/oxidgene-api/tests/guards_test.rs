//! The API guards: functional tests that fail when the codebase drifts from
//! a rule of AGENTS.md rather than when one feature breaks.
//!
//! One test binary for all of them, so they cost one link in CI. Each module
//! documents what drift it prevents and how to fix a failure;
//! docs/development.md lists them with their tier and CI job.

mod common;
mod guards;
