//! Builds a GEDZIP from a `.gw` export, a media manifest and the media bytes.
//!
//! A GEDZIP is a ZIP holding a GEDCOM 7 file plus its media, where every
//! `OBJE`/`FILE` payload is a path inside the archive. That is what makes it
//! the right container here: the whole point of this pipeline is that the
//! photos travel *with* the genealogy instead of as URLs that only resolve for
//! a logged-in Geneanet account.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use ged_io::types::multimedia::{Multimedia, file::Reference, format::Format};

use oxidgene_geneanet::join::{Attachment, Join};
use oxidgene_geneanet::media::{MediaSource, Origin};

/// Directory the media live in, inside the archive.
const MEDIA_DIR: &str = "media";

/// Assembles the archive.
///
/// `join` decides what goes where; this only fetches bytes and writes them out.
pub async fn build(
    database: &geneweb::database::GwDatabase,
    join: &Join,
    source: &mut MediaSource,
    manifest: &oxidgene_geneanet::Manifest,
    out: &Path,
) -> Result<()> {
    let mut gedcom = database.to_gedcom();

    anyhow::ensure!(
        gedcom.individuals.len() == database.persons.len(),
        "the .gw conversion produced {} individuals for {} persons, so attaching media by \
         position would put photos on the wrong people",
        gedcom.individuals.len(),
        database.persons.len()
    );

    // Look deposits and views up by id rather than re-walking the manifest.
    let deposits: HashMap<i64, &oxidgene_geneanet::model::ManifestDeposit> =
        manifest.deposits.iter().map(|d| (d.id, d)).collect();

    let mut files: HashMap<String, Vec<u8>> = HashMap::new();
    let mut fetched = 0;
    let total = join.view_count();

    for attachment in &join.attachments {
        let path = archive_path(attachment);

        if !files.contains_key(&path) {
            let deposit = deposits.get(&attachment.deposit_id).with_context(|| {
                format!("deposit {} is not in the manifest", attachment.deposit_id)
            })?;
            let view = deposit
                .views
                .iter()
                .find(|v| v.id == attachment.view_id)
                .with_context(|| format!("view {} is not in the manifest", attachment.view_id))?;

            let (bytes, origin) = source.bytes(deposit, view).await?;
            files.insert(path.clone(), bytes);

            fetched += 1;
            if fetched % 25 == 0 || fetched == total {
                eprintln!("  {fetched}/{total} media");
            }
            if origin == Origin::Rendition {
                eprintln!(
                    "  note: deposit {} page {} came from a downsized rendition — Geneanet \
                     exposes no per-page original",
                    attachment.deposit_id,
                    attachment.page.unwrap_or(0)
                );
            }
        }

        gedcom.individuals[attachment.person]
            .multimedia
            .push(multimedia(attachment, &path));
    }

    let bytes = ged_io::gedzip::write_gedzip_with_media(&gedcom, &files)
        .map_err(|e| anyhow::anyhow!("building the GEDZIP: {e}"))?;

    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    tokio::fs::write(out, &bytes)
        .await
        .with_context(|| format!("writing {}", out.display()))?;

    let sources = source.sources();
    eprintln!(
        "\n{} media ({} matched locally, {} originals downloaded, {} renditions)\n\
         {} links across {} persons\n\
         → {} ({:.1} MB)",
        files.len(),
        sources.local,
        sources.original,
        sources.rendition,
        join.attachments.len(),
        join.person_count(),
        out.display(),
        bytes.len() as f64 / 1_048_576.0,
    );

    Ok(())
}

/// Path a medium takes inside the archive.
///
/// Keyed by deposit and view so a photo shared by several people is stored
/// once and pointed at by each of them.
fn archive_path(attachment: &Attachment) -> String {
    format!(
        "{MEDIA_DIR}/{}_{}.{}",
        attachment.deposit_id, attachment.view_id, attachment.extension
    )
}

fn multimedia(attachment: &Attachment, path: &str) -> Multimedia {
    Multimedia {
        file: Some(Reference {
            value: Some(path.to_string()),
            title: attachment.title.clone(),
            form: Some(Format {
                value: Some(attachment.extension.clone()),
                source_media_type: Some(media_type(&attachment.extension).to_string()),
            }),
            crop: None,
        }),
        title: attachment.title.clone(),
        ..Multimedia::default()
    }
}

/// Maps an extension to the media type GEDCOM 7 wants on `FORM`.
fn media_type(extension: &str) -> &'static str {
    match extension {
        "png" => "image/png",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        _ => "image/jpeg",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attachment(deposit_id: i64, view_id: i64, extension: &str) -> Attachment {
        Attachment {
            person: 0,
            deposit_id,
            view_id,
            page: Some(1),
            title: Some("a title".to_string()),
            extension: extension.to_string(),
        }
    }

    #[test]
    fn names_a_medium_after_its_deposit_and_view() {
        assert_eq!(
            archive_path(&attachment(111, 222, "jpg")),
            "media/111_222.jpg"
        );
    }

    #[test]
    fn a_shared_photo_resolves_to_one_path() {
        // Two people, one group photo: the same view must land on the same
        // archive entry so the file is stored once.
        let mut first = attachment(111, 222, "jpg");
        first.person = 3;
        let mut second = attachment(111, 222, "jpg");
        second.person = 9;

        assert_eq!(archive_path(&first), archive_path(&second));
    }

    #[test]
    fn different_pages_of_a_deposit_get_different_paths() {
        assert_ne!(
            archive_path(&attachment(111, 222, "jpg")),
            archive_path(&attachment(111, 333, "jpg"))
        );
    }

    #[test]
    fn maps_extensions_to_media_types() {
        assert_eq!(media_type("png"), "image/png");
        assert_eq!(media_type("pdf"), "application/pdf");
        assert_eq!(media_type("tiff"), "image/tiff");
        // Anything unrecognised is treated as a JPEG, which is what the vast
        // majority of deposits are.
        assert_eq!(media_type("jpg"), "image/jpeg");
        assert_eq!(media_type("weird"), "image/jpeg");
    }

    #[test]
    fn the_multimedia_record_points_at_the_archive_entry() {
        let attachment = attachment(111, 222, "png");
        let path = archive_path(&attachment);

        let record = multimedia(&attachment, &path);
        let file = record.file.expect("has a file reference");

        assert_eq!(file.value.as_deref(), Some("media/111_222.png"));
        assert_eq!(file.title.as_deref(), Some("a title"));
        assert_eq!(
            file.form.and_then(|f| f.source_media_type).as_deref(),
            Some("image/png")
        );
    }
}
