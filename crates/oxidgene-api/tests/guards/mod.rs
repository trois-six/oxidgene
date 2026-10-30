//! The API guards: functional tests that fail when the codebase drifts from
//! a rule of AGENTS.md rather than when one feature breaks.
//!
//! Each module documents what drift it prevents and how to fix a failure;
//! docs/development.md lists them with their tier and CI job.

mod api_contract;
mod cross_tree;
mod history;
#[cfg(feature = "graphql")]
mod introspection;
mod log_privacy;
mod pagination;
mod purge;
mod sql_plans;
mod stack;
mod surface;
