//! Recovers the person↔media links that a Geneanet export cannot carry.
//!
//! A Geneanet GEDCOM/`.gw` export emits at most one `OBJE`/`#image` per
//! individual — the default portrait — as a URL that 403s for anyone not
//! logged in. Everything else is lost: the other photos on a person's page,
//! every group photo shared by several people, every scanned document.
//!
//! The media manager's API still has all of it, so we collect it separately
//! and join it back onto the tree by GeneWeb key.
//!
//! Run in two steps, because they carry very different weight:
//!
//! 1. [`manifest`] — roughly fifteen small JSON requests, thanks to a bulk
//!    endpoint that returns every link with its deposit inline. Cheap enough
//!    to re-run, and enough to see the whole mapping before committing to
//!    anything.
//! 2. [`fetch`] — one request per deposit for the original files. Hundreds of
//!    megabytes, so it is a separate, explicit decision.

pub mod browser;
pub mod client;
pub mod gedzip;
pub mod join;
pub mod key;
pub mod media;
pub mod model;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};

pub use client::{Client, Throttle};
pub use model::Manifest;

/// Reports how much of a manifest joins onto a `.gw` file, without touching
/// the network.
///
/// Worth running before [`gedzip`]: it says exactly which media will land on
/// which persons, and names everything that will not.
pub async fn check(gw_path: &Path, manifest_path: &Path, verbose: bool) -> Result<join::Join> {
    let manifest = read_manifest(manifest_path).await?;
    let database = read_gw(gw_path).await?;
    let index = join::PersonIndex::from_database(&database);
    let result = join::join(&manifest, &index);

    eprintln!(
        "{} persons in {}\n{} deposits / {} views in the manifest\n",
        index.person_count(),
        gw_path.display(),
        manifest.deposit_count,
        manifest.view_count,
    );
    eprintln!(
        "{} media attached to {} persons ({} attachments)\n\
         {} views linked to nobody on Geneanet\n\
         {} references could not be attached",
        result.view_count(),
        result.person_count(),
        result.attachments.len(),
        result.unlinked_view_count,
        result.unjoined.len(),
    );

    let mut by_reason: BTreeMap<&str, usize> = BTreeMap::new();
    for unjoined in &result.unjoined {
        *by_reason.entry(unjoined.reason.as_str()).or_default() += 1;
    }
    for (reason, count) in &by_reason {
        eprintln!("  {count} — {reason}");
    }

    if verbose {
        for unjoined in &result.unjoined {
            eprintln!(
                "  [{}] {} (deposit {}, view {})",
                unjoined.geneweb_ref.as_deref().unwrap_or("no key"),
                unjoined.name,
                unjoined.deposit_id,
                unjoined.view_id,
            );
        }
    }

    Ok(result)
}

/// Reads and parses a `.gw` export.
///
/// Bytes, not a string: a `.gw` file is ISO-8859-1 unless it opts into UTF-8
/// mid-file, so decoding here would mangle accented names.
async fn read_gw(path: &Path) -> Result<geneweb::database::GwDatabase> {
    let bytes = tokio::fs::read(path)
        .await
        .with_context(|| format!("reading {}", path.display()))?;
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into(),
    );

    let (database, errors) = geneweb::database::GwDatabase::read_lenient(&bytes, &name);

    if database.persons.is_empty() {
        bail!(
            "no person could be read from {} ({} parse error(s))",
            path.display(),
            errors.len()
        );
    }
    if !errors.is_empty() {
        eprintln!(
            "{} block(s) of {} could not be parsed and were skipped",
            errors.len(),
            path.display()
        );
    }

    Ok(database)
}

/// Builds a manifest from a collection gathered by the user's browser.
///
/// Offline: no cookie, no network. This is the path that works when Cloudflare
/// has decided the CLI looks automated — see [`browser`] for why using a real
/// browser is the honest answer rather than dressing this one up as one.
pub async fn manifest_from_browser(input: &Path, out: &Path) -> Result<Manifest> {
    let json = tokio::fs::read_to_string(input)
        .await
        .with_context(|| format!("reading {}", input.display()))?;
    let collection: model::BrowserCollection = serde_json::from_str(&json).with_context(|| {
        format!(
            "parsing {} — is it the file the browser script saved?",
            input.display()
        )
    })?;

    let (deposits, references) = collection.into_references();
    let manifest = Manifest::build("browser".to_string(), deposits, references);

    write_manifest(&manifest, out).await?;
    report(&manifest, out);

    Ok(manifest)
}

/// Prints what the collected manifest holds.
fn report(manifest: &Manifest, out: &Path) {
    eprintln!(
        "\n{} deposits / {} views\n{} views linked to {} distinct persons\n\
         {} views linked to nobody\n{} references without a GeneWeb key (persons outside the tree)\n\
         → {}",
        manifest.deposit_count,
        manifest.view_count,
        manifest.linked_view_count,
        manifest.person_count,
        manifest.view_count - manifest.linked_view_count,
        manifest.unjoinable_reference_count,
        out.display(),
    );
}

/// Pins each bulk-collected link to the view it belongs to.
///
/// A deposit with one page needs no work: the link can only be on that page.
/// A deposit with several is the awkward case — the bulk endpoint lists every
/// page without saying which — so its pages are probed one at a time, stopping
/// as soon as every link the bulk pass reported for that deposit is accounted
/// for. Links cluster on page 1 (the cover of a scanned dossier), so this
/// almost always costs a single request per deposit rather than one per page.
async fn locate(
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

    let pages: usize = deposits
        .iter()
        .filter(|d| multi_page.contains(&d.id))
        .map(|d| d.views.len())
        .sum();
    eprintln!(
        "Locating links inside {} multi-page deposit(s) ({pages} pages, probed until accounted \
         for)…",
        multi_page.len()
    );

    let mut located = single_page;
    let mut probes = 0;

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
            probes += 1;
            if !found.is_empty() {
                remaining = remaining.saturating_sub(found.len());
                located.insert((deposit_id, view.id), found);
            }
        }
    }

    eprintln!("  {probes} extra request(s).");

    Ok(located)
}

/// Builds a `.gdz` carrying the tree and every medium attached to a person.
///
/// This is the endpoint of the whole exercise: a single file holding the
/// genealogy *and* its photos, where a Geneanet export would have carried one
/// unusable URL per person.
pub async fn build_gedzip(
    client: Client,
    gw_path: &Path,
    manifest_path: &Path,
    local_media: Option<&Path>,
    multipage_originals: bool,
    out: &Path,
) -> Result<()> {
    let manifest = read_manifest(manifest_path).await?;
    let database = read_gw(gw_path).await?;
    let index = join::PersonIndex::from_database(&database);
    let joined = join::join(&manifest, &index);

    if joined.attachments.is_empty() {
        bail!(
            "nothing to attach: no reference in {} matched a person in {}. Are they from the \
             same tree?",
            manifest_path.display(),
            gw_path.display()
        );
    }

    let local = match local_media {
        Some(dir) => {
            let index = media::LocalIndex::build(dir)?;
            eprintln!(
                "{} local files indexed from {}",
                index.file_count(),
                dir.display()
            );
            Some(index)
        }
        None => None,
    };

    eprintln!(
        "{} media to attach to {} persons ({} links)\n",
        joined.view_count(),
        joined.person_count(),
        joined.attachments.len(),
    );

    let mut source = media::MediaSource::new(client, local, multipage_originals);
    gedzip::build(&database, &joined, &mut source, &manifest, out).await
}

/// Collects the full deposit → view → person mapping and writes it as JSON.
///
/// Cheap by construction. `/media/api/references` hands back every link with
/// its deposit inline, so the bulk of the work is a handful of paginated calls;
/// only links sitting inside a multi-page deposit need locating individually,
/// because that endpoint lists all of a deposit's pages without saying which
/// one the link is on.
///
/// On the reference tree this is ~15 requests where the naive per-view walk
/// took 618 — which matters beyond speed, since request volume is what gets a
/// client challenged.
pub async fn manifest(client: Client, out: &Path) -> Result<Manifest> {
    eprintln!("Listing deposits…");
    let deposits = client.list_deposits().await?;

    eprintln!("{} deposits. Fetching person links…", deposits.len());
    let entries = client.list_references().await?;
    eprintln!("{} links.", entries.len());

    let references = locate(&client, &deposits, entries).await?;
    let manifest = Manifest::build(client.base_url().to_string(), deposits, references);

    write_manifest(&manifest, out).await?;
    report(&manifest, out);

    Ok(manifest)
}

/// Downloads each deposit's original file and records where it landed.
///
/// Resumable: a deposit whose `local_file` already exists on disk is skipped,
/// so re-running after an interruption costs only the missing files.
pub async fn fetch(client: Client, manifest_path: &Path, media_dir: &Path) -> Result<()> {
    let mut manifest = read_manifest(manifest_path).await?;

    tokio::fs::create_dir_all(media_dir)
        .await
        .with_context(|| format!("creating {}", media_dir.display()))?;

    let total = manifest.deposits.len();
    let mut downloaded = 0;
    let mut skipped = 0;

    for index in 0..manifest.deposits.len() {
        let deposit_id = manifest.deposits[index].id;

        if let Some(existing) = &manifest.deposits[index].local_file
            && media_dir.join(existing).is_file()
        {
            skipped += 1;
            continue;
        }

        let (bytes, suggested) = client.download_deposit(deposit_id).await?;
        let filename = local_filename(deposit_id, suggested.as_deref());
        let path = media_dir.join(&filename);

        tokio::fs::write(&path, &bytes)
            .await
            .with_context(|| format!("writing {}", path.display()))?;

        manifest.deposits[index].local_file = Some(filename);
        downloaded += 1;

        if downloaded % 10 == 0 {
            eprintln!("  {}/{total} deposits", downloaded + skipped);
            // Checkpoint, so an interruption does not lose the mapping for
            // files already on disk.
            write_manifest(&manifest, manifest_path).await?;
        }
    }

    write_manifest(&manifest, manifest_path).await?;

    eprintln!(
        "\n{downloaded} downloaded, {skipped} already present\n→ {}",
        media_dir.display()
    );

    Ok(())
}

/// Names a downloaded file after its deposit id, keeping the original
/// extension.
///
/// The id leads so the file is traceable back to the manifest — Geneanet's own
/// suggested names collide (several `Photo.jpg`) and say nothing about origin.
fn local_filename(deposit_id: i64, suggested: Option<&str>) -> String {
    let extension = suggested
        .and_then(|name| Path::new(name).extension())
        .and_then(|ext| ext.to_str())
        .filter(|ext| ext.len() <= 8 && ext.chars().all(|c| c.is_ascii_alphanumeric()))
        .unwrap_or("bin");

    format!("{deposit_id}.{}", extension.to_ascii_lowercase())
}

async fn write_manifest(manifest: &Manifest, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("creating {}", parent.display()))?;
    }

    let json = serde_json::to_string_pretty(manifest).context("serialising the manifest")?;

    // Write beside the target then rename, so an interrupted checkpoint cannot
    // leave a half-written manifest behind.
    let temporary = temporary_path(path);
    tokio::fs::write(&temporary, json)
        .await
        .with_context(|| format!("writing {}", temporary.display()))?;
    tokio::fs::rename(&temporary, path)
        .await
        .with_context(|| format!("renaming {} into place", temporary.display()))?;

    Ok(())
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    path.with_file_name(name)
}

async fn read_manifest(path: &Path) -> Result<Manifest> {
    if !path.is_file() {
        bail!(
            "no manifest at {} — run `geneanet-media manifest` first",
            path.display()
        );
    }

    let json = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("reading {}", path.display()))?;

    serde_json::from_str(&json).with_context(|| format!("parsing {}", path.display()))
}

/// Builds a throttle from CLI values.
pub fn throttle(delay_ms: u64) -> Result<Throttle> {
    Ok(Throttle {
        delay: Duration::from_millis(delay_ms),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_original_extension_and_leads_with_the_deposit_id() {
        assert_eq!(local_filename(16053569, Some("Renée.jpg")), "16053569.jpg");
        assert_eq!(
            local_filename(43994698, Some("geneanet_05_08_2026.zip")),
            "43994698.zip"
        );
    }

    #[test]
    fn normalises_a_shouty_extension() {
        assert_eq!(local_filename(1, Some("PANTIN_002.JPG")), "1.jpg");
    }

    #[test]
    fn falls_back_when_the_suggested_name_is_unusable() {
        assert_eq!(local_filename(1, None), "1.bin");
        assert_eq!(local_filename(1, Some("no-extension")), "1.bin");
        // A path separator in an "extension" must never reach the filesystem.
        assert_eq!(local_filename(1, Some("x.jp/../../etc")), "1.bin");
    }

    #[test]
    fn temporary_path_sits_beside_the_target() {
        let path = Path::new("/tmp/media/manifest.json");

        assert_eq!(
            temporary_path(path),
            PathBuf::from("/tmp/media/manifest.json.tmp")
        );
    }
}
