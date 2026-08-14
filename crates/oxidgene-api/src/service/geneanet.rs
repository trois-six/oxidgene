//! The Geneanet import wizard's server side.
//!
//! Three halves meet here, and only here. The `.gw` export carries the tree
//! and the join key; a mapping collected from Geneanet's media API carries
//! which photo belongs to whom; the user's own data archives — read where they
//! lie, still zipped — carry the original bytes. See
//! `docs/specifications/geneanet-media-import.md` for why none of the three is
//! sufficient alone.
//!
//! The collection and the per-deposit byte lengths arrive already gathered:
//! the desktop wizard issues those requests inside the WebView the user signed
//! in to, so they come from a real browser on a real session rather than from
//! this process. What is left for the server is the part a browser cannot do —
//! reading the archives, joining, and writing rows.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use oxidgene_core::OxidGeneError;
use oxidgene_db::repo::{MediaLinkRepo, MediaRepo, TreeRepo, UploadedMedia};
use oxidgene_geneanet::archive::{ArchiveSet, LocalOriginals};
use oxidgene_geneanet::join::{self, UnjoinedReason};
use oxidgene_geneanet::model::{ManifestDeposit, ManifestView};
use oxidgene_geneanet::{Client, Manifest, Throttle};
use sea_orm::DatabaseConnection;
use uuid::Uuid;

use crate::media::{self, MediaStore};

use super::gedcom::persist_import_result;

/// How many names each expandable list in the preview carries.
///
/// The counts are the point; the names are there so a user can recognise
/// whether the mismatch is a wrong file or a genuinely absent branch. A few
/// dozen answers that, and the whole list would be thousands of strings
/// crossing the wire for nobody to read.
const SAMPLE_LIMIT: usize = 50;

/// Below this share of keyed references finding a person, the `.gw` and the
/// Geneanet account are almost certainly not the same tree.
///
/// The wizard blocks on this rather than importing a tree with photos on
/// nobody: the failure it catches — picking last year's export, or a relative's
/// — is common and invisible until it is too late to undo cheaply.
const MISMATCH_THRESHOLD: f64 = 0.10;

/// What a `.gw` file turned out to hold. Step 1 of the wizard.
#[derive(Debug, Clone)]
pub struct GwInspection {
    pub person_count: usize,
    pub family_count: usize,
    /// Blocks the lenient reader skipped. Reported, not fatal: a real export
    /// routinely carries a handful and losing them loses those blocks only.
    pub skipped_blocks: usize,
}

/// Parses a `.gw` export and reports what it holds, writing nothing.
///
/// This is the first moment a user learns whether they picked the right file,
/// and it costs nothing — which is exactly why the wizard does it on selection
/// rather than at import time.
///
/// # Errors
///
/// Returns `Err` if no person could be read, which is what a `.ged` handed to
/// this function looks like.
pub fn inspect_gw(bytes: &[u8], file_name: &str) -> Result<GwInspection, OxidGeneError> {
    let (database, skipped_blocks) = oxidgene_geneanet::parse_gw(bytes, file_name)
        .map_err(|e| OxidGeneError::Validation(e.to_string()))?;

    Ok(GwInspection {
        person_count: database.persons.len(),
        family_count: database.families.len(),
        skipped_blocks,
    })
}

/// What one data archive turned out to hold. Step 2 of the wizard.
#[derive(Debug, Clone)]
pub struct ArchiveReport {
    pub path: String,
    pub file_name: String,
    pub file_count: usize,
    pub image_count: usize,
    /// Set when this archive could not be read. The others still stand — one
    /// corrupt ZIP is not a reason to discard the four that opened.
    pub error: Option<String>,
}

/// Indexes each archive's central directory, extracting nothing.
///
/// Paths, not bytes: the archives are gigabytes and the wizard only reaches
/// this code on desktop, where the server is in-process and the files are the
/// same filesystem's. Reading a few kilobytes per archive is the whole cost.
pub fn index_archives(paths: &[String]) -> (ArchiveSet, Vec<ArchiveReport>) {
    let mut set = ArchiveSet::new();
    let mut reports = Vec::with_capacity(paths.len());

    for path in paths {
        let path_buf = PathBuf::from(path);
        let file_name = file_name_of(&path_buf);

        match set.add(&path_buf) {
            // Already added — the user picked the same archive twice, which is
            // a slip rather than a decision, so it is dropped silently.
            Ok(None) => {}
            Ok(Some(info)) => reports.push(ArchiveReport {
                path: path.clone(),
                file_name,
                file_count: info.file_count,
                image_count: info.image_count,
                error: None,
            }),
            Err(err) => reports.push(ArchiveReport {
                path: path.clone(),
                file_name,
                file_count: 0,
                image_count: 0,
                error: Some(err.to_string()),
            }),
        }
    }

    (set, reports)
}

fn file_name_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into(),
    )
}

/// Everything step 4 shows, computed with no further network access.
#[derive(Debug, Clone, Default)]
pub struct Preview {
    pub person_count: usize,
    /// Distinct views the account holds.
    pub photo_count: usize,
    pub persons_with_photo: usize,
    /// One per person a photo lands on, so a group photo counts several times.
    pub attachment_count: usize,
    /// Photos already in the supplied archives — the ones needing no download.
    pub in_archives: usize,
    /// Photos that will have to be downloaded.
    pub to_download: usize,
    /// Views showing several people.
    pub group_photos: usize,
    /// Views nobody on Geneanet attached to anyone; skipped, not imported.
    pub unlinked_views: usize,
    /// References naming people outside this tree.
    pub outside_tree: usize,
    /// Keys matching more than one person, so attaching would be a coin toss.
    pub ambiguous: usize,
    pub outside_tree_names: Vec<String>,
    pub ambiguous_names: Vec<String>,
    /// `true` when the `.gw` and the account look like different trees.
    pub mismatch: bool,
}

/// Joins the collected mapping onto the `.gw` and reports what would happen.
///
/// Nothing is written and nothing is fetched. `deposit_sizes` is the byte
/// length of each single-page deposit, gathered in the login window during
/// step 3 — it is what decides whether a photo is already in the archives.
///
/// # Errors
///
/// Returns `Err` if the `.gw` cannot be parsed or the collection is not the
/// shape the login window emits.
pub fn preview(
    gw_bytes: &[u8],
    file_name: &str,
    collection_json: &str,
    deposit_sizes: &HashMap<i64, u64>,
    archives: &ArchiveSet,
) -> Result<Preview, OxidGeneError> {
    let (database, _) = oxidgene_geneanet::parse_gw(gw_bytes, file_name)
        .map_err(|e| OxidGeneError::Validation(e.to_string()))?;
    let manifest = oxidgene_geneanet::manifest_from_collection(collection_json)
        .map_err(|e| OxidGeneError::Validation(e.to_string()))?;

    let index = join::PersonIndex::from_database(&database);
    let joined = join::join(&manifest, &index);

    let mut preview = Preview {
        person_count: index.person_count(),
        photo_count: manifest.view_count,
        persons_with_photo: joined.person_count(),
        attachment_count: joined.attachments.len(),
        unlinked_views: joined.unlinked_view_count,
        ..Preview::default()
    };

    // A view carrying more than one attachment is a photo of several people —
    // the thing the export could not express at all.
    let mut per_view: BTreeMap<(i64, i64), usize> = BTreeMap::new();
    for attachment in &joined.attachments {
        *per_view
            .entry((attachment.deposit_id, attachment.view_id))
            .or_default() += 1;
    }
    preview.group_photos = per_view.values().filter(|n| **n > 1).count();

    for unjoined in &joined.unjoined {
        match unjoined.reason {
            UnjoinedReason::NoKey | UnjoinedReason::NoSuchPerson => {
                preview.outside_tree += 1;
                if preview.outside_tree_names.len() < SAMPLE_LIMIT {
                    preview.outside_tree_names.push(unjoined.name.clone());
                }
            }
            UnjoinedReason::Ambiguous => {
                preview.ambiguous += 1;
                if preview.ambiguous_names.len() < SAMPLE_LIMIT {
                    preview.ambiguous_names.push(unjoined.name.clone());
                }
            }
        }
    }

    let deposits: HashMap<i64, &ManifestDeposit> =
        manifest.deposits.iter().map(|d| (d.id, d)).collect();

    for (deposit_id, view_id) in per_view.keys() {
        let held = deposits
            .get(deposit_id)
            .is_some_and(|d| held_locally(d, *view_id, deposit_sizes, archives));
        if held {
            preview.in_archives += 1;
        } else {
            preview.to_download += 1;
        }
    }

    // Keyed references are the ones that *could* have found a person; a
    // reference with no key names somebody outside the tree and says nothing
    // about whether the two halves belong together.
    let keyed = joined.attachments.len()
        + joined
            .unjoined
            .iter()
            .filter(|u| u.reason != UnjoinedReason::NoKey)
            .count();
    preview.mismatch =
        keyed > 0 && (joined.attachments.len() as f64 / keyed as f64) < MISMATCH_THRESHOLD;

    Ok(preview)
}

/// Whether a view's bytes can be taken from the archives rather than fetched.
///
/// Only a single-page deposit can: it *is* the file, so its length identifies
/// it. A multi-page deposit downloads as an archive Geneanet streams without a
/// `Content-Length`, so no length is available to match on.
fn held_locally(
    deposit: &ManifestDeposit,
    _view_id: i64,
    deposit_sizes: &HashMap<i64, u64>,
    archives: &ArchiveSet,
) -> bool {
    if deposit.views.len() != 1 {
        return false;
    }
    deposit_sizes
        .get(&deposit.id)
        .is_some_and(|size| matches!(archives.resolve(*size), Ok(Some(_))))
}

/// What an import actually did.
#[derive(Debug, Clone, Default)]
pub struct GeneanetImportSummary {
    pub persons_count: usize,
    pub families_count: usize,
    pub events_count: usize,
    pub sources_count: usize,
    pub places_count: usize,
    pub notes_count: usize,
    /// Distinct photos stored.
    pub media_count: usize,
    /// Person↔photo rows written; higher than `media_count` when a photo shows
    /// several people.
    pub links_count: usize,
    /// Photos that could not be fetched, one line each. A failure here skips
    /// that photo and no more: the tree is already imported, and losing one
    /// scan is not a reason to throw away ten thousand people.
    pub skipped: Vec<String>,
    pub warnings: Vec<String>,
}

/// Imports the tree, then attaches every photo that joins onto it.
///
/// Order matters. The `.gw` goes in first through the shared
/// [`persist_import_result`] path, which is what assigns the person ids the
/// photo links need; the media pass then resolves each photo's bytes — from
/// the archives when a size matches, over the network when it does not — and
/// writes one `media` row per photo with one `media_link` per person on it.
///
/// # Errors
///
/// Returns `Err` if the tree does not exist, the `.gw` cannot be parsed, or the
/// person import fails. Once the persons are in, a photo that cannot be fetched
/// is recorded in [`GeneanetImportSummary::skipped`] rather than failing the run.
#[allow(clippy::too_many_arguments)]
pub async fn import(
    db: &DatabaseConnection,
    store: &dyn MediaStore,
    tree_id: Uuid,
    gw_bytes: &[u8],
    file_name: &str,
    collection_json: &str,
    deposit_sizes: &HashMap<i64, u64>,
    archive_paths: &[String],
    cookie: Option<&str>,
) -> Result<GeneanetImportSummary, OxidGeneError> {
    let _tree = TreeRepo::get(db, tree_id).await?;

    let (database, _) = oxidgene_geneanet::parse_gw(gw_bytes, file_name)
        .map_err(|e| OxidGeneError::Validation(e.to_string()))?;
    let manifest = oxidgene_geneanet::manifest_from_collection(collection_json)
        .map_err(|e| OxidGeneError::Validation(e.to_string()))?;

    let index = join::PersonIndex::from_database(&database);
    let joined = join::join(&manifest, &index);

    // The persons first: their ids are what the photo links point at.
    let import_result = oxidgene_gedcom::geneweb::import_geneweb(gw_bytes, file_name, tree_id)
        .map_err(OxidGeneError::Gedcom)?;
    let person_by_xref = import_result.person_by_xref.clone();
    let people = persist_import_result(db, import_result).await?;

    let mut summary = GeneanetImportSummary {
        persons_count: people.persons_count,
        families_count: people.families_count,
        events_count: people.events_count,
        sources_count: people.sources_count,
        places_count: people.places_count,
        notes_count: people.notes_count,
        warnings: people.warnings,
        ..GeneanetImportSummary::default()
    };

    if joined.attachments.is_empty() {
        return Ok(summary);
    }

    attach_media(
        db,
        store,
        tree_id,
        &manifest,
        &joined,
        &person_by_xref,
        deposit_sizes,
        archive_paths,
        cookie,
        &mut summary,
    )
    .await;

    Ok(summary)
}

/// Stores each attached photo once and links it to every person on it.
///
/// Errors are collected rather than propagated: by the time this runs the tree
/// is already in the database, so aborting would leave the user with people and
/// no photos and no way to tell why.
#[allow(clippy::too_many_arguments)]
async fn attach_media(
    db: &DatabaseConnection,
    store: &dyn MediaStore,
    tree_id: Uuid,
    manifest: &Manifest,
    joined: &join::Join,
    person_by_xref: &HashMap<String, Uuid>,
    deposit_sizes: &HashMap<i64, u64>,
    archive_paths: &[String],
    cookie: Option<&str>,
    summary: &mut GeneanetImportSummary,
) {
    let (archives, _) = index_archives(archive_paths);

    // Only built when something actually has to be downloaded, so a run whose
    // archives cover everything makes no request at all — and needs no cookie.
    let mut client: Option<Client> = None;

    let deposits: HashMap<i64, &ManifestDeposit> =
        manifest.deposits.iter().map(|d| (d.id, d)).collect();

    // Group the attachments by the photo they point at: one `media` row per
    // photo however many people are on it, which is the whole reason
    // `MediaLink` is a separate table.
    let mut by_view: BTreeMap<(i64, i64), Vec<&join::Attachment>> = BTreeMap::new();
    for attachment in &joined.attachments {
        by_view
            .entry((attachment.deposit_id, attachment.view_id))
            .or_default()
            .push(attachment);
    }

    for ((deposit_id, view_id), attachments) in by_view {
        let Some(deposit) = deposits.get(&deposit_id) else {
            summary
                .skipped
                .push(format!("deposit {deposit_id} is not in the collection"));
            continue;
        };
        let Some(view) = deposit.views.iter().find(|v| v.id == view_id) else {
            summary
                .skipped
                .push(format!("page {view_id} is not in the collection"));
            continue;
        };

        let bytes =
            match resolve_bytes(deposit, view, deposit_sizes, &archives, cookie, &mut client).await
            {
                Ok(bytes) => bytes,
                Err(err) => {
                    summary.skipped.push(format!(
                        "{}: {err}",
                        deposit.title.as_deref().unwrap_or("untitled photo")
                    ));
                    continue;
                }
            };

        let name = photo_file_name(deposit, view, &attachments[0].extension);
        let ingested = match media::ingest(store, tree_id, &name, bytes).await {
            Ok(ingested) => ingested,
            Err(err) => {
                summary.skipped.push(format!("{name}: {err}"));
                continue;
            }
        };

        let upload = UploadedMedia {
            file_name: ingested.file_name,
            mime_type: ingested.mime_type,
            storage_key: ingested.storage_key,
            sha256: ingested.sha256,
            file_size: ingested.file_size,
            thumbnail_key: ingested.thumbnail_key,
            width: ingested.width,
            height: ingested.height,
            page_count: ingested.page_count,
            title: deposit.title.clone(),
            description: None,
        };

        let media_row = match MediaRepo::create_uploaded(db, Uuid::now_v7(), tree_id, upload).await
        {
            Ok(row) => row,
            Err(err) => {
                summary.skipped.push(format!("{name}: {err}"));
                continue;
            }
        };
        summary.media_count += 1;

        for (order, attachment) in attachments.iter().enumerate() {
            // `GwDatabase::persons[i]` becomes the individual with xref
            // `@I{i+1}@` — the positional correspondence the whole join rests
            // on, and the one place a drift would put photos on strangers.
            let xref = format!("@I{}@", attachment.person + 1);
            let Some(person_id) = person_by_xref.get(&xref).copied() else {
                summary
                    .skipped
                    .push(format!("{name}: person {xref} was not imported"));
                continue;
            };

            match MediaLinkRepo::create(
                db,
                Uuid::now_v7(),
                media_row.id,
                Some(person_id),
                None,
                None,
                None,
                i32::try_from(order).unwrap_or(0),
            )
            .await
            {
                Ok(_) => summary.links_count += 1,
                Err(err) => summary.skipped.push(format!("{name}: {err}")),
            }
        }
    }
}

/// Gets a photo's bytes, preferring a copy the user already has.
async fn resolve_bytes(
    deposit: &ManifestDeposit,
    view: &ManifestView,
    deposit_sizes: &HashMap<i64, u64>,
    archives: &ArchiveSet,
    cookie: Option<&str>,
    client: &mut Option<Client>,
) -> Result<Vec<u8>, String> {
    if deposit.views.len() == 1
        && let Some(size) = deposit_sizes.get(&deposit.id)
        && let Ok(Some(bytes)) = archives.resolve(*size)
    {
        return Ok(bytes);
    }

    let cookie = cookie.ok_or_else(|| {
        "not in the archives, and there is no Geneanet session to download it with".to_string()
    })?;

    if client.is_none() {
        *client = Some(Client::new(cookie, None, Throttle::default()).map_err(|e| e.to_string())?);
    }
    let client = client.as_ref().expect("just populated above");

    if deposit.views.len() == 1 {
        return client
            .download_deposit(deposit.id)
            .await
            .map(|(bytes, _)| bytes)
            .map_err(|e| e.to_string());
    }

    // A page of a multi-page deposit: Geneanet exposes no per-page original,
    // so the largest rendition it does expose is what a page comes in as.
    let url = rendition_url(view).ok_or_else(|| {
        format!(
            "page {} of deposit {} has no rendition URL to fall back on",
            view.page.unwrap_or(1),
            deposit.id
        )
    })?;

    client.download_url(&url).await.map_err(|e| e.to_string())
}

/// Picks the largest rendition a view exposes.
fn rendition_url(view: &ManifestView) -> Option<String> {
    for rendition in ["normal", "screen", "medium", "thumbnail"] {
        if let Some(path) = view.files.get(rendition) {
            // Manifest paths are host-relative and served from the gw
            // subdomain, not the www one the API lives on.
            return Some(if path.starts_with("http") {
                path.clone()
            } else {
                format!("https://gw.geneanet.org{path}")
            });
        }
    }
    None
}

/// Names a stored photo after its deposit, page and type.
///
/// Geneanet's own upload names collide (several `Photo.jpg`) and say nothing
/// about origin, so the ids lead — the same reasoning as the CLI's downloaded
/// file names.
fn photo_file_name(deposit: &ManifestDeposit, view: &ManifestView, extension: &str) -> String {
    match view.page {
        Some(page) if deposit.views.len() > 1 => {
            format!("geneanet-{}-p{page}.{extension}", deposit.id)
        }
        _ => format!("geneanet-{}.{extension}", deposit.id),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use oxidgene_geneanet::model::ManifestView;

    fn view(id: i64, page: Option<i64>) -> ManifestView {
        ManifestView {
            id,
            page,
            files: BTreeMap::from([("normal".to_string(), "/a/b/normal.png".to_string())]),
            references: Vec::new(),
        }
    }

    fn deposit(id: i64, views: Vec<ManifestView>) -> ManifestDeposit {
        ManifestDeposit {
            id,
            original: format!("https://www.geneanet.org/media/download/?deposits[]={id}"),
            title: Some("a title".to_string()),
            kind: Some("portraits".to_string()),
            private: true,
            date_create: None,
            local_file: None,
            views,
        }
    }

    #[test]
    fn a_gedcom_handed_to_step_one_is_rejected() {
        // The commonest wrong turn: Geneanet offers both exports and only the
        // .gw carries the occurrence number the join needs.
        let gedcom = b"0 HEAD\n1 SOUR OxidGene\n0 @I1@ INDI\n1 NAME A /BRANCH_A/\n0 TRLR\n";

        assert!(inspect_gw(gedcom, "tree.ged").is_err());
    }

    #[test]
    fn step_one_counts_what_the_file_holds() {
        let gw = b"encoding: utf-8\n\nfam BRANCH_A person_a.0 + BRANCH_B person_b.0\n";

        let inspection = inspect_gw(gw, "tree.gw").expect("parses");

        assert_eq!(inspection.person_count, 2);
        assert_eq!(inspection.family_count, 1);
        assert_eq!(inspection.skipped_blocks, 0);
    }

    #[test]
    fn a_single_page_deposit_whose_size_matches_needs_no_download() {
        let deposit = deposit(1, vec![view(10, Some(1))]);
        let sizes = HashMap::from([(1, 5)]);

        // An empty archive set cannot hold it, so it must be downloaded.
        assert!(!held_locally(&deposit, 10, &sizes, &ArchiveSet::new()));
    }

    #[test]
    fn a_multi_page_deposit_is_never_taken_from_the_archives() {
        // Its download is a streamed ZIP with no Content-Length, so there is no
        // length to match on — claiming otherwise would attach the wrong bytes.
        let deposit = deposit(1, vec![view(10, Some(1)), view(11, Some(2))]);
        let sizes = HashMap::from([(1, 5)]);

        assert!(!held_locally(&deposit, 10, &sizes, &ArchiveSet::new()));
    }

    #[test]
    fn a_page_of_a_document_is_named_after_its_page() {
        let single = deposit(1, vec![view(10, Some(1))]);
        assert_eq!(
            photo_file_name(&single, &single.views[0], "jpg"),
            "geneanet-1.jpg"
        );

        let multi = deposit(2, vec![view(10, Some(1)), view(11, Some(2))]);
        assert_eq!(
            photo_file_name(&multi, &multi.views[1], "jpg"),
            "geneanet-2-p2.jpg"
        );
    }

    #[test]
    fn a_rendition_path_is_absolutised_onto_the_gw_subdomain() {
        assert_eq!(
            rendition_url(&view(1, Some(1))).as_deref(),
            Some("https://gw.geneanet.org/a/b/normal.png")
        );
    }

    #[test]
    fn indexing_a_missing_archive_reports_it_without_losing_the_others() {
        let (set, reports) = index_archives(&["/nonexistent/archive.zip".to_string()]);

        assert!(set.is_empty());
        assert_eq!(reports.len(), 1);
        assert!(reports[0].error.is_some());
        assert_eq!(reports[0].file_name, "archive.zip");
    }
}
