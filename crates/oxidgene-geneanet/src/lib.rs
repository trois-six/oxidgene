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
pub mod client;
pub mod join;
pub mod key;
pub mod media;
pub mod model;
pub mod script;

use std::collections::BTreeMap;

use anyhow::{Result, bail};

pub use client::{Client, Throttle};
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

/// Pins each bulk-collected link to the view it belongs to.
///
/// A deposit with one page needs no work: the link can only be on that page.
/// A deposit with several is the awkward case — the bulk endpoint lists every
/// page without saying which — so its pages are probed one at a time, stopping
/// as soon as every link the bulk pass reported for that deposit is accounted
/// for. Links cluster on page 1 (the cover of a scanned dossier), so this
/// almost always costs a single request per deposit rather than one per page.
///
/// # Errors
///
/// Returns `Err` if a per-view probe fails.
pub async fn locate(
    client: &Client,
    deposits: &[model::Deposit],
    entries: Vec<model::ReferenceEntry>,
) -> Result<model::LocatedReferences> {
    let mut expected: BTreeMap<i64, usize> = BTreeMap::new();
    let mut single_page = model::LocatedReferences::new();
    let mut multi_page: Vec<i64> = Vec::new();

    for entry in entries {
        let deposit_id = entry.deposit.id;
        *expected.entry(deposit_id).or_default() += 1;

        match entry.deposit.views.as_slice() {
            [only] => {
                let view_id = only.id;
                single_page
                    .entry((deposit_id, view_id))
                    .or_default()
                    .push(entry.into_reference());
            }
            _ => {
                if !multi_page.contains(&deposit_id) {
                    multi_page.push(deposit_id);
                }
            }
        }
    }

    if multi_page.is_empty() {
        return Ok(single_page);
    }

    let mut located = single_page;

    for deposit_id in multi_page {
        let Some(deposit) = deposits.iter().find(|d| d.id == deposit_id) else {
            continue;
        };
        let mut remaining = expected.get(&deposit_id).copied().unwrap_or(0);

        for view in &deposit.views {
            if remaining == 0 {
                break;
            }
            let found = client.view_references(deposit_id, view.id).await?;
            if !found.is_empty() {
                remaining = remaining.saturating_sub(found.len());
                located.insert((deposit_id, view.id), found);
            }
        }
    }

    Ok(located)
}

/// Collects the full deposit → view → person mapping over HTTP.
///
/// Cheap by construction. `/media/api/references` hands back every link with
/// its deposit inline, so the bulk of the work is a handful of paginated calls;
/// only links sitting inside a multi-page deposit need locating individually.
///
/// On the reference tree this is ~19 requests where the naive per-view walk
/// took 618 — which matters beyond speed, since request volume is what gets a
/// client challenged by Cloudflare.
///
/// # Errors
///
/// Returns `Err` if any request fails, including the Cloudflare challenge the
/// client detects and names rather than mistaking for an expired cookie.
pub async fn collect_manifest(client: &Client) -> Result<Manifest> {
    let deposits = client.list_deposits().await?;
    let entries = client.list_references().await?;
    let references = locate(client, &deposits, entries).await?;

    Ok(Manifest::build(
        client.base_url().to_string(),
        deposits,
        references,
    ))
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
        client::DEFAULT_BASE_URL.to_string(),
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
