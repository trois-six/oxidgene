//! Static reference content — occupation sheets and given-name meanings —
//! served read-only under `/api/v1/reference`. Not tied to any tree.
//!
//! Source JSON lives in `data/` (one file per language per data type),
//! gzip-compressed at build time (see `build.rs`) and decompressed once,
//! on first lookup, into an in-memory table (see `loader.rs`).

mod loader;

use std::collections::HashSet;

use serde::Serialize;

pub use loader::{
    GivenNameEntry, OccupationEntry, ReferenceLang, lookup_given_name, lookup_occupation,
};

pub const MAX_REFERENCE_TERMS: usize = 128;

#[derive(Debug, Clone, Serialize)]
pub struct GivenNameMatch {
    pub term: String,
    #[serde(flatten)]
    pub entry: GivenNameEntry,
}

pub fn lookup_given_names(lang: ReferenceLang, terms: &[String]) -> Vec<GivenNameMatch> {
    let mut seen = HashSet::new();
    terms
        .iter()
        .filter(|term| seen.insert((*term).clone()))
        .filter_map(|term| {
            lookup_given_name(lang, term).map(|entry| GivenNameMatch {
                term: term.clone(),
                entry,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_lookup_preserves_order_deduplicates_and_omits_unknown_terms() {
        let terms = ["Jean", "__unknown__", "Marie", "Jean"]
            .map(str::to_string)
            .to_vec();

        let matches = lookup_given_names(ReferenceLang::Fr, &terms);

        assert_eq!(
            matches
                .iter()
                .map(|result| result.term.as_str())
                .collect::<Vec<_>>(),
            ["Jean", "Marie"]
        );
    }
}
