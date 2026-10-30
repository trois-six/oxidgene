//! The repository guards, one module per rule.
//!
//! One binary for the crate: Cargo builds every `tests/*.rs` file it finds
//! as an executable of its own, each linking the crate and its dependencies
//! again. The manifest turns that discovery off, so a file of `tests/` runs
//! only once declared below; the `test_targets` guard keeps it so.

mod continuous_listeners;
mod dead_css;
mod html_injection;
mod layering;
mod lint_discipline;
mod menu_buttons;
mod migration_policy;
mod select_style;
mod specs;
mod test_targets;
mod ui_tracing;
mod vendored_pdfjs;
