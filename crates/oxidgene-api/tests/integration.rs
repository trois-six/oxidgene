//! The integration tests of `oxidgene-api`: REST and GraphQL scenarios
//! against an in-memory SQLite database, the MCP server, the services, the
//! statement counts and the API guards.
//!
//! One binary for the crate: Cargo builds every `tests/*.rs` file it finds
//! as an executable of its own, each linking the crate and its dependencies
//! again. The manifest turns that discovery off, so a file of `tests/` runs
//! only once declared below; the `test_targets` guard keeps it so.

mod access_test;
mod algorithm_scaling_test;
mod archive_target_test;
mod common;
mod consistency_test;
mod dictionary_bulk_test;
mod dictionary_exact_title_test;
mod entry_lookup_test;
mod export_scope_test;
mod family_name_descent_test;
mod family_name_test;
mod gedcom_place_test;
mod gedcom_tag_test;
mod geneanet_session_test;
mod graphql_test;
mod guards;
mod history_test;
mod import_memory_test;
mod kinship_test;
mod mcp_test;
mod media_test;
mod media_validation_test;
mod page_bundles_test;
mod profile_service_test;
mod query_scaling_test;
mod real_import_test;
mod rest_test;
mod tag_bulk_test;
mod tag_carrier_scaling_test;
mod tag_reads_test;
mod tag_record_reads_test;
mod tag_record_scaling_test;
mod tag_scope_test;
mod tag_test;
mod trace_continuity_test;
