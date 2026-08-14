//! Headless driver for the Geneanet media pipeline.
//!
//! The pipeline itself lives in [`oxidgene_geneanet`], shared with the API so
//! the import wizard and the CLI cannot drift apart on the join, the key
//! folding or the size matching. What stays here is what only a terminal
//! needs: reading and checkpointing files, printing reports, and the `.gdz`
//! writer — the app imports straight into a tree, so that container is a CLI
//! affordance rather than the product's path.
//!
//! Run in two steps, because they carry very different weight:
//!
//! 1. [`manifest`] — roughly nineteen small JSON requests, thanks to a bulk
//!    endpoint that returns every link with its deposit inline. Cheap enough
//!    to re-run, and enough to see the whole mapping before committing to
//!    anything.
//! 2. [`fetch`] — one request per deposit for the original files. Hundreds of
//!    megabytes, so it is a separate, explicit decision.

pub mod gedzip;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use oxidgene_geneanet::archive::LocalOriginals;
use oxidgene_geneanet::{join, media, script};

pub use oxidgene_geneanet::{Client, Manifest, Throttle};

/// Reports how much of a manifest joins onto a `.gw` file, without touching
/// the network.
///
/// Worth running before [`build_gedzip`]: it says exactly which media will land
/// on which persons, and names everything that will not.
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

    let (database, skipped) = oxidgene_geneanet::parse_gw(&bytes, &name)?;

    if skipped > 0 {
        eprintln!(
            "{skipped} block(s) of {} could not be parsed and were skipped",
            path.display()
        );
    }

    Ok(database)
}

/// Builds a manifest from a collection gathered by the user's browser.
///
/// Offline: no cookie, no network. This is the path that works when Cloudflare
/// has decided the CLI looks automated — see [`script`] for why using a real
/// browser is the honest answer rather than dressing this one up as one.
pub async fn manifest_from_browser(input: &Path, out: &Path) -> Result<Manifest> {
    let json = tokio::fs::read_to_string(input)
        .await
        .with_context(|| format!("reading {}", input.display()))?;

    let manifest = oxidgene_geneanet::manifest_from_collection(&json).with_context(|| {
        format!(
            "parsing {} — is it the file the browser script saved?",
            input.display()
        )
    })?;

    write_manifest(&manifest, out).await?;
    report(&manifest, out);

    Ok(manifest)
}

/// Prints the script and the instructions for running it in a real browser.
pub fn browser_script() {
    println!("{}\n", script::INSTRUCTIONS);
    println!("{}", script::collection_script());
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

/// Builds a `.gdz` carrying the tree and every medium attached to a person.
///
/// A headless convenience, not the app's path: OxidGene imports straight into
/// a tree and can export a `.gdz` from it afterwards, so producing one only to
/// re-import it would be a detour.
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
            let boxed: Box<dyn LocalOriginals + Send + Sync> = Box::new(index);
            Some(boxed)
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
/// Cheap by construction — ~19 requests where the naive per-view walk took 618,
/// which matters beyond speed, since request volume is what gets a client
/// challenged.
pub async fn manifest(client: Client, out: &Path) -> Result<Manifest> {
    eprintln!("Collecting the mapping…");
    let manifest = oxidgene_geneanet::collect_manifest(&client).await?;

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
