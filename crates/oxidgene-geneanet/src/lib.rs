//! Recovers the person↔media links that a Geneanet export cannot carry.
//!
//! A Geneanet GEDCOM/`.gw` export emits at most one `OBJE`/`#image` per
//! individual — the default portrait — as a URL that 403s for anyone not
//! logged in. Everything else is lost: the other photos on a person's page,
//! every group photo shared by several people, every scanned document.
//!
//! The media manager's API still has all of it, so we collect it separately
//! and join it back onto the tree by GeneWeb key. See
//! `docs/specifications/geneanet-media-import.md` for why this is the only
//! surface that knows about it.
//!
//! This crate is the platform-independent half, shared by the CLI (headless
//! `geneanet-media` subcommands) and the API (the import wizard's steps). It
//! deliberately holds no UI, no database and no `.gdz` writer: the CLI keeps
//! the archive builder, and the API keeps the persistence.

pub mod archive;
pub mod join;
pub mod key;
pub mod model;
pub mod phash;
pub mod script;
pub mod session;

use anyhow::{Result, bail};

/// The host the media manager and its API live on.
///
/// A browser collection carries no host of its own, and the manager it was
/// gathered from only exists on this one.
pub const DEFAULT_BASE_URL: &str = "https://www.geneanet.org";

pub use model::Manifest;

/// Parses a `.gw` export from its raw bytes.
///
/// Bytes, not a string: a `.gw` file is ISO-8859-1 unless it opts into UTF-8
/// mid-file, so decoding before handing it over would mangle accented names —
/// and the accents are part of the join key.
///
/// Returns the database alongside the number of blocks that could not be read,
/// which the wizard reports rather than treating as a failure: a real export
/// routinely carries a handful, and skipping them loses those blocks only.
///
/// # Errors
///
/// Returns `Err` if not a single person could be read, which is what a `.ged`
/// handed to this function looks like.
pub fn parse_gw(bytes: &[u8], name: &str) -> Result<(geneweb::database::GwDatabase, usize)> {
    let (database, errors) = geneweb::database::GwDatabase::read_lenient(bytes, name);

    if database.persons.is_empty() {
        bail!(
            "no person could be read from {name} ({} parse error(s))",
            errors.len()
        );
    }

    Ok((database, errors.len()))
}

/// Builds a manifest from a collection gathered inside a real browser.
///
/// Offline: no cookie, no network. This is the path the desktop wizard takes —
/// the requests are issued by the WebView the user signed in to, so Cloudflare
/// sees a browser because it *is* one. See [`script`].
///
/// # Errors
///
/// Returns `Err` if the JSON is not the shape [`script::COLLECTION`] emits.
pub fn manifest_from_collection(json: &str) -> Result<Manifest> {
    let collection: model::BrowserCollection = serde_json::from_str(json)?;
    let (deposits, references) = collection.into_references();

    // The browser collection carries no host of its own, and the media manager
    // it was gathered from only exists on the one host.
    Ok(Manifest::build(
        DEFAULT_BASE_URL.to_string(),
        deposits,
        references,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gedcom_is_rejected_as_holding_no_person() {
        // What a user who picked the wrong export hands us. The wizard turns
        // this into "this looks like a GEDCOM file".
        let gedcom = b"0 HEAD\n1 SOUR OxidGene\n0 @I1@ INDI\n1 NAME Test /BRANCH_A/\n0 TRLR\n";

        assert!(parse_gw(gedcom, "tree.ged").is_err());
    }

    #[test]
    fn a_minimal_gw_parses_and_reports_no_skipped_blocks() {
        let gw = b"encoding: utf-8\n\nfam BRANCH_A person_a.0 + BRANCH_B person_b.0\n";

        let (database, errors) = parse_gw(gw, "tree.gw").expect("parses");

        assert!(!database.persons.is_empty());
        assert_eq!(errors, 0);
    }
}
