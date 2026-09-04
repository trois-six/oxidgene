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

use chrono::{DateTime, NaiveDate, Utc};
use futures_util::stream::{FuturesUnordered, StreamExt};
use oxidgene_core::OxidGeneError;
use oxidgene_core::enums::{EventType, Privacy};
use oxidgene_core::types::Portrait;
use oxidgene_db::repo::{
    MediaLinkRepo, MediaPatch, MediaRepo, NoteRepo, PersonNamePieces, PersonNameRepo, PersonRepo,
    PlaceRepo, TreeRepo, UploadedMedia, VignetteInput, VignetteRepo,
};
use oxidgene_geneanet::Manifest;
use oxidgene_geneanet::archive::{ArchiveSet, ContentIndex, LocalOriginals};
use oxidgene_geneanet::join::{self, UnjoinedReason};
use oxidgene_geneanet::model::{GeneanetEvent, GeneanetTranscript, ManifestDeposit, ManifestView};
use sea_orm::DatabaseConnection;
use uuid::Uuid;

use crate::media::{self, MediaStore};

use super::gedcom::persist_import_result_with_progress;

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

/// Which bytes an import is asked to keep for each medium.
///
/// The two are not variations on one pipeline; they need different things from
/// the user. Originals need the data archives — a separate Geneanet request,
/// an email, and several gigabytes of ZIP — and reach for the network only for
/// what those archives cannot account for. Renditions need nothing but the
/// login: every page is taken from Geneanet's own largest per-page variant.
///
/// `normal` is provenance-unknown (see
/// `docs/specifications/geneanet-media-import.md` §4): sometimes the uploaded
/// bytes, more often a re-encoding, and always a JPEG where the deposit was a
/// PDF. That is the trade this makes, and it is the default because it is the
/// only path a user with no data archive can take at all.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaFidelity {
    /// Geneanet's `normal` rendition of every page. No archives, no byte-length
    /// match, no perceptual match — one fetch per view and nothing to reconcile.
    #[default]
    Renditions,
    /// The uploaded files: from the data archives wherever a byte length or a
    /// content match lands, downloaded from the deposit otherwise.
    Originals,
}

impl MediaFidelity {
    /// Whether the archives take part in this import at all.
    ///
    /// Also what says whether they are worth staging: a caller that sends
    /// archive paths with `Renditions` would otherwise have gigabytes copied
    /// into job storage for a run that never opens them.
    #[must_use]
    pub const fn uses_archives(self) -> bool {
        matches!(self, Self::Originals)
    }
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
    /// Photos found in the archives by exact byte length. Nothing is fetched
    /// for these at all.
    pub in_archives: usize,
    /// Document pages recognised in the archives *by content*.
    ///
    /// A page has no byte length to match on — its deposit downloads as a ZIP
    /// that Geneanet streams without a `Content-Length`, and there is no
    /// per-page original URL — so a small rendition of each is fetched, hashed
    /// and matched against the archives. The page that gets stored is the
    /// archive's original wherever that match lands, which on the reference
    /// archive is 97 % of them. Counting these as downloads would call the
    /// mechanism that *avoids* downloading a download.
    pub to_match: usize,
    /// Media with no archive candidate at all, which really are downloaded.
    pub to_download: usize,
    /// Views showing several people.
    pub group_photos: usize,
    /// Views in deposits nobody attached to anyone; skipped, not imported.
    ///
    /// Counted per *deposit*, not per view: a page of a document whose cover
    /// is linked is imported with it, so it is not skipped and must not be
    /// reported as though it were.
    pub unlinked_views: usize,
    /// Multi-page deposits that will be imported as documents.
    pub documents: usize,
    /// Pages those documents hold, every one of which is imported.
    pub document_pages: usize,
    /// References Geneanet recorded no tree person for at all — named on the
    /// medium, never linked to somebody in the GeneWeb tree.
    pub unlinked_names: usize,
    /// References carrying a key that matches nobody in the `.gw`. Different
    /// thing entirely: Geneanet *did* link these to a person, and the export
    /// does not have them — usually a partial export, or one taken before they
    /// were added.
    pub outside_tree: usize,
    /// Keys matching more than one person, so attaching would be a coin toss.
    pub ambiguous: usize,
    pub unlinked_names_sample: Vec<String>,
    pub outside_tree_names: Vec<String>,
    pub ambiguous_names: Vec<String>,
    /// `true` when the `.gw` and the account look like different trees.
    pub mismatch: bool,
}

/// Joins the collected mapping onto the `.gw` and reports what would happen.
///
/// Nothing is written and nothing is fetched. `deposit_sizes` is the byte
/// length of each single-page deposit, gathered in the login window during
/// step 3 — it is what decides whether a photo is already in the archives, and
/// [`MediaFidelity::Renditions`] therefore ignores it along with the archives.
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
    fidelity: MediaFidelity,
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
            // Two very different situations, and telling a user they are the
            // same sends them looking for people who were never linked.
            UnjoinedReason::NoKey => {
                preview.unlinked_names += 1;
                if preview.unlinked_names_sample.len() < SAMPLE_LIMIT {
                    preview.unlinked_names_sample.push(unjoined.name.clone());
                }
            }
            UnjoinedReason::NoSuchPerson => {
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

    // A deposit with any attached page is imported in full — a document comes
    // in whole, or a scanned dossier arrives as its cover page alone. So what
    // is counted here is deposits, and what is skipped is only the deposits
    // nobody attached to anybody at all.
    let attached: std::collections::BTreeSet<i64> = joined
        .attachments
        .iter()
        .map(|attachment| attachment.deposit_id)
        .collect();

    for deposit in &manifest.deposits {
        if !attached.contains(&deposit.id) {
            preview.unlinked_views += deposit.views.len();
            continue;
        }

        // A document is still a document whichever bytes are kept for its
        // pages, and every one of them is imported either way.
        if deposit.views.len() > 1 {
            preview.documents += 1;
            preview.document_pages += deposit.views.len();
        }

        // Renditions are fetched for every page and matched against nothing,
        // so there is one download per view and no local hit to report.
        if !fidelity.uses_archives() {
            preview.to_download += deposit.views.len();
            continue;
        }

        if deposit.views.len() == 1 {
            if held_locally(deposit, deposit.views[0].id, deposit_sizes, archives) {
                preview.in_archives += 1;
            } else {
                preview.to_download += 1;
            }
            continue;
        }

        // Every page of a document is fetched, because a page has no byte
        // length to match an archive entry against — it is recognised from its
        // rendition instead, and that rendition has to be retrieved first.
        preview.to_match += deposit.views.len();
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

/// One medium the server cannot produce on its own.
#[derive(Debug, Clone)]
pub struct NeededMedia {
    pub deposit_id: i64,
    pub view_id: i64,
    pub page: Option<i64>,
    /// Where the login window should fetch it from.
    pub url: String,
    /// `true` when this is the deposit's original rather than a rendition —
    /// exact bytes, but only available for a single-page deposit.
    pub original: bool,
}

/// Lists what the login window has to fetch before an import can run.
///
/// The server never reaches Geneanet: every direct request is challenged
/// whatever the cookie, so anything it cannot find locally has to come through
/// the window the user signed in to. This is the question it asks first.
///
/// Under [`MediaFidelity::Renditions`] the rule is one line: every page of
/// every attached deposit, as its `normal` rendition. Nothing is matched, so
/// nothing smaller is fetched to match with.
///
/// Under [`MediaFidelity::Originals`] the rule per medium, in the order
/// [`resolve_bytes`] applies it:
///
/// - resolvable from the archives by **exact byte length** → nothing needed
/// - otherwise, a **single-page** deposit → its original, which is exact
/// - otherwise, a **page of a document** → its largest rendition, which is
///   what a perceptual match against the archives runs on and, failing that,
///   what gets stored (downsized — Geneanet exposes no per-page original)
///
/// # Errors
///
/// Returns `Err` if the `.gw` cannot be parsed or the collection is not the
/// shape the login window emits.
pub fn plan(
    gw_bytes: &[u8],
    file_name: &str,
    collection_json: &str,
    deposit_sizes: &HashMap<i64, u64>,
    archives: &ArchiveSet,
    fidelity: MediaFidelity,
) -> Result<Vec<NeededMedia>, OxidGeneError> {
    let (database, _) = oxidgene_geneanet::parse_gw(gw_bytes, file_name)
        .map_err(|e| OxidGeneError::Validation(e.to_string()))?;
    let manifest = oxidgene_geneanet::manifest_from_collection(collection_json)
        .map_err(|e| OxidGeneError::Validation(e.to_string()))?;

    let index = join::PersonIndex::from_database(&database);
    let joined = join::join(&manifest, &index);

    // Deposits with at least one attached page. A document comes in whole, so
    // one linked page pulls in all of its siblings.
    let attached: std::collections::BTreeSet<i64> = joined
        .attachments
        .iter()
        .map(|attachment| attachment.deposit_id)
        .collect();

    let mut needed = Vec::new();
    for deposit in &manifest.deposits {
        if !attached.contains(&deposit.id) {
            continue;
        }

        let single = deposit.views.len() == 1;
        let original = fidelity.uses_archives() && single;
        if original
            && deposit_sizes
                .get(&deposit.id)
                .is_some_and(|size| matches!(archives.resolve(*size), Ok(Some(_))))
        {
            continue;
        }

        for view in pages_in_order(deposit) {
            let urls = if !fidelity.uses_archives() {
                // The one rendition that will be stored. The smaller sample
                // `rendition_urls` adds exists only to hash against an
                // archive, and there is no archive here.
                stored_rendition_url(view).into_iter().collect()
            } else if single {
                original_url(deposit).into_iter().collect()
            } else {
                rendition_urls(view)
            };
            for url in urls {
                needed.push(NeededMedia {
                    deposit_id: deposit.id,
                    view_id: view.id,
                    page: view.page,
                    url,
                    original,
                });
            }
        }
    }

    Ok(needed)
}

/// What an import actually did.
#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
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
    /// Links marked as the person's profile photo, from the `.gw`'s `#image`.
    pub portraits_count: usize,
    /// People created for Geneanet identifications marked "hors de l'arbre".
    pub isolated_count: usize,
    /// Regions drawn round a person on a picture, from Geneanet's own boxes.
    pub vignettes_count: usize,
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
/// `fidelity` decides whether the archives are consulted at all;
/// [`MediaFidelity::Renditions`] ignores `archive_paths` and `deposit_sizes`
/// and stores the `normal` rendition the login window fetched for each page.
///
/// # Errors
///
/// `fetched` carries the bytes the login window retrieved, keyed by URL — no
/// direct request to Geneanet succeeds, so nothing here reaches the network.
///
/// # Errors
///
/// Returns `Err` if the tree does not exist, the `.gw` cannot be parsed, or the
/// person import fails. Once the persons are in, a photo that cannot be fetched
/// is recorded in [`GeneanetImportSummary::skipped`] rather than failing the run.
#[allow(clippy::too_many_arguments)]
#[tracing::instrument(name = "import.geneanet", skip_all)]
pub async fn import(
    db: &DatabaseConnection,
    store: &dyn MediaStore,
    tree_id: Uuid,
    gw_bytes: &[u8],
    file_name: &str,
    collection_json: &str,
    deposit_sizes: &HashMap<i64, u64>,
    archive_paths: &[String],
    fetched: &HashMap<String, String>,
    fidelity: MediaFidelity,
    progress: &ImportProgress,
) -> Result<GeneanetImportSummary, OxidGeneError> {
    let _tree = TreeRepo::get(db, tree_id).await?;

    // Renditions keep what Geneanet re-encoded, so the archives take no part:
    // dropping the paths here is what turns off the byte-length match, the
    // perceptual index, and the archive reads all three depend on — rather
    // than each of them asking again further down.
    let archive_paths: &[String] = if fidelity.uses_archives() {
        archive_paths
    } else {
        &[]
    };

    let (mut import_result, manifest, joined) =
        tracing::info_span!("import.parse", import.format = "geneanet").in_scope(|| {
            let (database, _) = oxidgene_geneanet::parse_gw(gw_bytes, file_name)
                .map_err(|error| OxidGeneError::Validation(error.to_string()))?;
            let manifest = oxidgene_geneanet::manifest_from_collection(collection_json)
                .map_err(|error| OxidGeneError::Validation(error.to_string()))?;
            let index = join::PersonIndex::from_database(&database);
            let joined = join::join(&manifest, &index);
            let import_result =
                oxidgene_gedcom::geneweb::import_geneweb(gw_bytes, file_name, tree_id)
                    .map_err(OxidGeneError::Gedcom)?;
            Ok::<_, OxidGeneError>((import_result, manifest, joined))
        })?;

    // A `.gw` carries one `#image` per person — the portrait, as a URL that
    // 403s for anyone not logged in. We are about to import that very photo
    // properly, so keeping the URL would leave every portrait in the tree
    // twice: once as a stored medium and once as a dead link beside it.
    //
    // The URL *is* one of the renditions the collection lists, so this is an
    // exact match on the path, not a guess — and it tells us which view is the
    // person's portrait, which is the only place that fact exists.
    let event_matcher = GeneanetEventMatcher::from_import(&import_result);
    let portraits = take_portrait_urls(&mut import_result, &manifest);
    let person_by_xref = import_result.person_by_xref.clone();
    progress.begin(ImportPhase::People, persisted_entity_count(&import_result));
    let people = persist_import_result_with_progress(db, import_result, |inserted| {
        progress.advance_by(inserted);
    })
    .await?;

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

    // Geneanet lets an owner identify somebody on a photograph who is not in
    // the GeneWeb tree — its manager labels them "hors de l'arbre". Those
    // identifications name a real person and carry real media, so rather than
    // dropping them the import creates each one as an isolated `Person`: no
    // events, no family links, just a name and their photographs.
    let isolated = create_isolated_people(db, tree_id, &joined, &mut summary).await;

    attach_media(
        db,
        store,
        tree_id,
        &manifest,
        &joined,
        &person_by_xref,
        &event_matcher,
        &isolated,
        deposit_sizes,
        archive_paths,
        fetched,
        &portraits,
        progress,
        &mut summary,
    )
    .await;

    Ok(summary)
}

/// Writes every attached medium and links it to the people on it.
///
/// Two shapes come out of this, because Geneanet has two:
///
/// - A **single-page deposit** is one photograph. One `media` row, one
///   `media_link` per person on it — a group photo is stored once and linked
///   several times, which is what `MediaLink` was always for.
/// - A **multi-page deposit is a document, and it comes in whole.** Links on
///   Geneanet attach to *pages*, and a user who scans a 144-page dossier
///   attaches the cover — so importing only linked pages would import a cover
///   and discard 143 pages of a naturalisation file. Measured on the reference
///   account, that is every one of the 235 "unlinked" views. So the deposit
///   becomes one document (`is_document`) with every page beneath it in page
///   order, and the people any of its pages named are linked to the document.
///
/// Errors are collected rather than propagated: by the time this runs the tree
/// is already in the database, so aborting would leave the user with people and
/// no photos and no way to tell why.
#[allow(clippy::too_many_arguments)]
#[tracing::instrument(name = "import.media", skip_all, fields(import.format = "geneanet"))]
async fn attach_media(
    db: &DatabaseConnection,
    store: &dyn MediaStore,
    tree_id: Uuid,
    manifest: &Manifest,
    joined: &join::Join,
    person_by_xref: &HashMap<String, Uuid>,
    event_matcher: &GeneanetEventMatcher,
    // `isolated`: folded name → the person created for an out-of-tree
    // identification.
    isolated: &HashMap<String, Uuid>,
    deposit_sizes: &HashMap<i64, u64>,
    archive_paths: &[String],
    fetched: &HashMap<String, String>,
    // `portraits`: person xref → the view the `.gw` named as their portrait.
    portraits: &HashMap<String, i64>,
    progress: &ImportProgress,
    summary: &mut GeneanetImportSummary,
) {
    let (archives, _) = index_archives(archive_paths);

    let deposits: HashMap<i64, &ManifestDeposit> =
        manifest.deposits.iter().map(|d| (d.id, d)).collect();

    // Group by *deposit*, not by view: whether a page is imported at all now
    // depends on what the rest of its deposit is.
    let mut by_deposit: BTreeMap<i64, Vec<&join::Attachment>> = BTreeMap::new();
    for attachment in &joined.attachments {
        by_deposit
            .entry(attachment.deposit_id)
            .or_default()
            .push(attachment);
    }
    // Every view of every attached deposit, which is what the media phase will
    // write — a document contributes all its pages, not just its linked ones.
    let media_total: usize = by_deposit
        .keys()
        .filter_map(|deposit_id| deposits.get(deposit_id))
        .map(|deposit| deposit.views.len())
        .sum();

    progress.begin(ImportPhase::Matching, 0);
    let hashes = build_content_index(&deposits, &by_deposit, deposit_sizes, &archives, fetched);
    progress.begin(ImportPhase::Media, media_total);
    let mut places = match PlaceRepo::list_all(db, tree_id).await {
        Ok(places) => places
            .into_iter()
            .map(|place| (place_key(&place.name), place.id))
            .collect(),
        Err(err) => {
            summary
                .warnings
                .push(format!("could not list places: {err}"));
            HashMap::new()
        }
    };

    // Every one-page deposit, decoded and stored ahead of the loop below.
    //
    // These are the bulk of an import — 378 of 386 deposits on the reference
    // account — and each is a full-size decode and a thumbnail. Done one at a
    // time inside the loop they used a single core; batched here they use the
    // machine. The loop then only has links left to write, which is ordering
    // work and stays sequential.
    let prepared = prepare_single_pages(
        db,
        store,
        tree_id,
        &deposits,
        &by_deposit,
        deposit_sizes,
        &archives,
        hashes.as_ref(),
        fetched,
        progress,
        &mut places,
        summary,
    )
    .await;

    for (deposit_id, attachments) in by_deposit {
        let Some(deposit) = deposits.get(&deposit_id) else {
            summary
                .skipped
                .push(format!("deposit {deposit_id} is not in the collection"));
            continue;
        };

        // Everyone any page of this deposit named, in first-seen order: who
        // they are, whether this deposit holds their portrait, and where on
        // the picture they were boxed.
        let mut people: Vec<Attached> = Vec::new();
        let mut event_references: Vec<(Uuid, GeneanetEvent)> = Vec::new();
        for attachment in &attachments {
            // `GwDatabase::persons[i]` becomes the individual with xref
            // `@I{i+1}@` — the positional correspondence the whole join rests
            // on, and the one place a drift would put photos on strangers.
            let xref = format!("@I{}@", attachment.person + 1);
            let Some(person_id) = person_by_xref.get(&xref).copied() else {
                summary.skipped.push(format!(
                    "deposit {deposit_id}: person {xref} was not imported"
                ));
                continue;
            };
            // The `.gw` named one view as this person's portrait. Nothing else
            // knows which of their photos that is.
            let is_portrait = portraits
                .get(&xref)
                .is_some_and(|view_id| *view_id == attachment.view_id);
            if let Some(event) = attachment.event.clone() {
                event_references.push((person_id, event));
            }
            people.push(Attached {
                person_id,
                is_portrait,
                view_id: attachment.view_id,
                face: attachment.face.clone(),
            });
        }

        // The identifications Geneanet marks "hors de l'arbre" name people we
        // created above; their media attach exactly like anyone else's.
        for unjoined in &joined.unjoined {
            if unjoined.deposit_id != deposit_id || unjoined.reason != UnjoinedReason::NoKey {
                continue;
            }
            let (Some(lastname), Some(firstname)) = (&unjoined.lastname, &unjoined.firstname)
            else {
                continue;
            };
            let key = oxidgene_geneanet::key::geneanet_key(lastname, firstname, 0);
            let Some(person_id) = isolated.get(&key).copied() else {
                continue;
            };
            people.push(Attached {
                person_id,
                is_portrait: false,
                view_id: unjoined.view_id,
                face: unjoined.face.clone(),
            });
        }

        // `owner` is what a person links to — the photograph, or the document
        // as a whole. `pages` is where each view's bytes actually landed,
        // which is where an identification box belongs: on the page somebody
        // was boxed on, not on the document that contains it.
        let (owner, pages) = if deposit.views.len() == 1 {
            match prepared.get(&deposit_id) {
                Some(pair) => pair.clone(),
                None => continue,
            }
        } else {
            match document(
                db,
                store,
                tree_id,
                deposit,
                deposit_sizes,
                &archives,
                hashes.as_ref(),
                fetched,
                progress,
                &mut places,
                summary,
            )
            .await
            {
                Some(pair) => pair,
                None => continue,
            }
        };

        // Several people in a group photograph may each point at the shared
        // family event. The link belongs to the file, so persist it once.
        let mut linked_event_ids = std::collections::HashSet::new();
        for (person_id, event) in event_references {
            let Some(event_id) = event_matcher.resolve(person_id, &event) else {
                continue;
            };
            if !linked_event_ids.insert(event_id) {
                continue;
            }
            if let Err(err) = MediaLinkRepo::create(
                db,
                Uuid::now_v7(),
                owner,
                None,
                Some(event_id),
                None,
                None,
                0,
            )
            .await
            {
                summary.skipped.push(format!(
                    "deposit {deposit_id}: could not link Geneanet event {}: {err}",
                    event.id
                ));
            }
        }

        for (order, (person_id, is_portrait)) in linked_people(&people).into_iter().enumerate() {
            let created = MediaLinkRepo::create(
                db,
                Uuid::now_v7(),
                owner,
                Some(person_id),
                None,
                None,
                None,
                i32::try_from(order).unwrap_or(0),
            )
            .await;

            match created {
                Ok(link) => {
                    summary.links_count += 1;
                    // The portrait is a property of the person, so this
                    // writes the person rather than the link — one row, and
                    // "at most one portrait" needs no clearing pass.
                    if is_portrait && let Some(person_id) = link.person_id {
                        match PersonRepo::set_portrait(
                            db,
                            person_id,
                            Portrait::Media(link.media_id),
                        )
                        .await
                        {
                            Ok(_) => summary.portraits_count += 1,
                            Err(err) => {
                                summary.skipped.push(format!("deposit {deposit_id}: {err}"))
                            }
                        }
                    }
                }
                Err(err) => summary.skipped.push(format!("deposit {deposit_id}: {err}")),
            }
        }

        // A person links to the document once, but every box remains on the
        // exact page where Geneanet drew it. The same person may therefore
        // have several identifications across a multi-page document.
        for (page_id, face, person_id) in page_identifications(&people, &pages) {
            add_vignette(db, page_id, face, person_id, summary).await;
        }
    }
}

/// One person attached to a deposit, and what we know about their place on it.
struct Attached {
    person_id: Uuid,
    /// The `.gw` named this view as their portrait.
    is_portrait: bool,
    /// The page they were identified on.
    view_id: i64,
    face: Option<oxidgene_geneanet::model::FacePosition>,
}

fn linked_people(identifications: &[Attached]) -> Vec<(Uuid, bool)> {
    let mut people: Vec<(Uuid, bool)> = Vec::new();
    for identification in identifications {
        if let Some((_, is_portrait)) = people
            .iter_mut()
            .find(|(person_id, _)| *person_id == identification.person_id)
        {
            *is_portrait |= identification.is_portrait;
        } else {
            people.push((identification.person_id, identification.is_portrait));
        }
    }
    people
}

fn page_identifications<'a>(
    identifications: &'a [Attached],
    pages: &HashMap<i64, Uuid>,
) -> Vec<(Uuid, &'a oxidgene_geneanet::model::FacePosition, Uuid)> {
    identifications
        .iter()
        .filter_map(|identification| {
            Some((
                *pages.get(&identification.view_id)?,
                identification.face.as_ref()?,
                identification.person_id,
            ))
        })
        .collect()
}

/// Imported events indexed by the people whose media may document them.
/// Family events are indexed under both spouses because Geneanet puts its
/// marriage reference on each person while OxidGene stores one family event.
struct GeneanetEventMatcher {
    candidates_by_person: HashMap<Uuid, Vec<ImportedEvent>>,
}

struct ImportedEvent {
    id: Uuid,
    event_type: EventType,
    date: Option<NaiveDate>,
    place: Option<String>,
}

impl GeneanetEventMatcher {
    fn from_import(result: &oxidgene_gedcom::ImportResult) -> Self {
        let places: HashMap<Uuid, String> = result
            .places
            .iter()
            .map(|place| (place.id, place_key(&place.name)))
            .collect();
        let mut candidates_by_person: HashMap<Uuid, Vec<ImportedEvent>> = HashMap::new();
        let mut candidates_by_family: HashMap<Uuid, Vec<ImportedEvent>> = HashMap::new();

        for event in &result.events {
            let candidate = ImportedEvent {
                id: event.id,
                event_type: event.event_type,
                date: event.date_sort,
                place: event.place_id.and_then(|id| places.get(&id).cloned()),
            };
            if let Some(person_id) = event.person_id {
                candidates_by_person
                    .entry(person_id)
                    .or_default()
                    .push(candidate);
            } else if let Some(family_id) = event.family_id {
                candidates_by_family
                    .entry(family_id)
                    .or_default()
                    .push(candidate);
            }
        }

        for spouse in &result.family_spouses {
            if let Some(events) = candidates_by_family.get(&spouse.family_id) {
                candidates_by_person
                    .entry(spouse.person_id)
                    .or_default()
                    .extend(events.iter().map(|event| ImportedEvent {
                        id: event.id,
                        event_type: event.event_type,
                        date: event.date,
                        place: event.place.clone(),
                    }));
            }
        }

        Self {
            candidates_by_person,
        }
    }

    fn resolve(&self, person_id: Uuid, source: &GeneanetEvent) -> Option<Uuid> {
        let event_type = geneanet_event_type(source.name.as_deref())?;
        let date = source
            .date
            .as_deref()
            .and_then(|value| NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())?;
        let place = source.location.as_deref().map(geneanet_place_key);
        let matches: Vec<&ImportedEvent> = self
            .candidates_by_person
            .get(&person_id)?
            .iter()
            .filter(|candidate| {
                candidate.event_type == event_type
                    && candidate.date == Some(date)
                    && place
                        .as_ref()
                        .is_none_or(|place| candidate.place.as_ref() == Some(place))
            })
            .collect();
        match matches.as_slice() {
            [candidate] => Some(candidate.id),
            _ => None,
        }
    }
}

fn geneanet_event_type(name: Option<&str>) -> Option<EventType> {
    let name = name?.strip_prefix("gw_event_")?;
    Some(match name {
        "birth" => EventType::Birth,
        "death" => EventType::Death,
        "baptism" => EventType::Baptism,
        "confirmation" => EventType::Confirmation,
        "first_communion" => EventType::FirstCommunion,
        "bar_mitzvah" | "bat_mitzvah" => EventType::BarBatMitzvah,
        "military_service" => EventType::MilitaryService,
        "burial" => EventType::Burial,
        "cremation" => EventType::Cremation,
        "graduation" => EventType::Graduation,
        "immigration" => EventType::Immigration,
        "emigration" => EventType::Emigration,
        "naturalization" => EventType::Naturalization,
        "census" => EventType::Census,
        "occupation" => EventType::Occupation,
        "residence" => EventType::Residence,
        "retirement" => EventType::Retirement,
        "will" => EventType::Will,
        "probate" => EventType::Probate,
        "adoption" => EventType::Adoption,
        "caste_name" => EventType::CasteName,
        "physical_description" => EventType::PhysicalDescription,
        "education" => EventType::Education,
        "national_id" => EventType::NationalId,
        "national_origin" => EventType::NationalOrigin,
        "children_count" => EventType::ChildrenCount,
        "marriages_count" => EventType::MarriagesCount,
        "property" => EventType::Property,
        "religion" => EventType::Religion,
        "social_security_number" => EventType::SocialSecurityNumber,
        "nobility_title" => EventType::NobilityTitle,
        "fact" => EventType::Fact,
        "marriage" => EventType::Marriage,
        "divorce" => EventType::Divorce,
        "annulment" => EventType::Annulment,
        "engagement" => EventType::Engagement,
        "marriage_bann" => EventType::MarriageBann,
        "marriage_contract" => EventType::MarriageContract,
        "marriage_license" => EventType::MarriageLicense,
        "marriage_settlement" => EventType::MarriageSettlement,
        "civil_union" => EventType::CivilUnion,
        "separation" => EventType::Separation,
        "divorce_filed" => EventType::DivorceFiled,
        "blessing" => EventType::Blessing,
        "ordination" => EventType::Ordination,
        "christening" => EventType::Christening,
        "adult_christening" => EventType::AdultChristening,
        "accomplishment" => EventType::Accomplishment,
        "acquisition" => EventType::Acquisition,
        "membership" => EventType::Membership,
        "change_name" => EventType::ChangeName,
        "circumcision" => EventType::Circumcision,
        "award" => EventType::Award,
        "military_discharge" => EventType::MilitaryDischarge,
        "degree" => EventType::Degree,
        "distinction" => EventType::Distinction,
        "election" => EventType::Election,
        "excommunication" => EventType::Excommunication,
        "funeral" => EventType::Funeral,
        "hospitalization" => EventType::Hospitalization,
        "illness" => EventType::Illness,
        "passenger_list" => EventType::PassengerList,
        "military_distinction" => EventType::MilitaryDistinction,
        "military_promotion" => EventType::MilitaryPromotion,
        "military_mobilization" => EventType::MilitaryMobilization,
        "property_sale" => EventType::PropertySale,
        "endl" => EventType::Endowment,
        "dotationlds" => EventType::LdsDotation,
        "slgc" => EventType::SealingChild,
        "slgs" => EventType::SealingSpouse,
        "scellent_parent_lds" => EventType::SealingParent,
        "family_link_lds" => EventType::FamilyLinkLds,
        "unmarried" => EventType::NoMarriage,
        "nomen" => EventType::NoMention,
        "bapl" => EventType::LdsBaptism,
        "conl" => EventType::LdsConfirmation,
        _ => return None,
    })
}

fn geneanet_place_key(value: &str) -> String {
    place_key(
        &value
            .replace("&#39;", "'")
            .replace("&#x27;", "'")
            .replace("&amp;", "&"),
    )
}

/// Records Geneanet's identification box as a vignette on the stored picture.
///
/// Geneanet gives percentages, which is fortunate: they survive whichever
/// rendition or original ended up being stored. The conversion needs the
/// picture's own dimensions, so a medium we could not decode — a PDF page —
/// simply has no vignette, rather than one at a guessed size.
async fn add_vignette(
    db: &DatabaseConnection,
    media_id: Uuid,
    face: &oxidgene_geneanet::model::FacePosition,
    person_id: Uuid,
    summary: &mut GeneanetImportSummary,
) {
    let Ok(media) = MediaRepo::get(db, media_id).await else {
        return;
    };
    let (Some(width), Some(height)) = (media.width, media.height) else {
        return;
    };
    let Some((x, y, w, h)) = face.to_pixels(width, height) else {
        return;
    };

    let input = VignetteInput {
        media_id,
        page: 0,
        x,
        y,
        width: w,
        height: h,
        person_id: Some(person_id),
        event_id: None,
    };

    match VignetteRepo::create(db, Uuid::now_v7(), input).await {
        Ok(_) => summary.vignettes_count += 1,
        Err(err) => summary.skipped.push(format!("identification box: {err}")),
    }
}

/// Stores every one-page deposit, several at a time.
///
/// Decoding and thumbnailing is what an import spends its minutes on, and each
/// deposit is independent of the others — so they are resolved and ingested in
/// batches the width of the machine, and only the database writes are
/// sequential.
///
/// Batched rather than all at once because every medium in flight holds a
/// full-size decoded image: a few hundred scans read at once would be
/// gigabytes.
///
/// Returns `deposit id → (media id, view id → media id)`, the same shape
/// [`document`] returns, so the caller does not care which kind it was.
#[allow(clippy::too_many_arguments)]
#[tracing::instrument(name = "import.media.prepare", skip_all)]
async fn prepare_single_pages(
    db: &DatabaseConnection,
    store: &dyn MediaStore,
    tree_id: Uuid,
    deposits: &HashMap<i64, &ManifestDeposit>,
    by_deposit: &BTreeMap<i64, Vec<&join::Attachment>>,
    deposit_sizes: &HashMap<i64, u64>,
    archives: &ArchiveSet,
    hashes: Option<&ContentIndex>,
    fetched: &HashMap<String, String>,
    progress: &ImportProgress,
    places: &mut HashMap<String, Uuid>,
    summary: &mut GeneanetImportSummary,
) -> HashMap<i64, (Uuid, HashMap<i64, Uuid>)> {
    let single: Vec<(i64, &ManifestDeposit, &str)> = by_deposit
        .iter()
        .filter_map(|(deposit_id, attachments)| {
            let deposit = deposits.get(deposit_id)?;
            (deposit.views.len() == 1).then(|| {
                let extension = attachments.first().map_or("jpg", |a| a.extension.as_str());
                (*deposit_id, *deposit, extension)
            })
        })
        .collect();

    let mut prepared = HashMap::with_capacity(single.len());

    for batch in single.chunks(ingest_width()) {
        let mut resolved = Vec::with_capacity(batch.len());
        for (deposit_id, deposit, extension) in batch {
            let Some(view) = deposit.views.first() else {
                progress.advance();
                continue;
            };
            match resolve_bytes(deposit, view, deposit_sizes, archives, hashes, fetched).await {
                Ok(bytes) => {
                    let metadata = media_metadata(db, tree_id, deposit, places, summary).await;
                    resolved.push((
                        *deposit_id,
                        view.id,
                        photo_file_name(deposit, view, extension),
                        deposit.title.clone(),
                        media_classification(deposit),
                        geneanet_privacy(deposit),
                        geneanet_created_at(deposit),
                        metadata,
                        bytes,
                    ));
                }
                Err(err) => {
                    summary.skipped.push(format!("deposit {deposit_id}: {err}"));
                    progress.advance();
                }
            }
        }

        let mut pending = FuturesUnordered::new();
        for (index, (_, _, name, _, _, _, _, _, bytes)) in resolved.iter().enumerate() {
            pending.push(async move {
                let outcome = media::ingest(store, tree_id, name, bytes.clone()).await;
                (index, outcome)
            });
        }
        let mut ingested = Vec::with_capacity(resolved.len());
        while let Some(outcome) = pending.next().await {
            progress.advance();
            ingested.push(outcome);
        }
        ingested.sort_unstable_by_key(|(index, _)| *index);

        for (
            (deposit_id, view_id, name, title, classification, privacy, created_at, metadata, _),
            (_, outcome),
        ) in resolved.iter().zip(ingested)
        {
            let ingested = match outcome {
                Ok(ingested) => ingested,
                Err(err) => {
                    summary.skipped.push(format!("{name}: {err}"));
                    continue;
                }
            };

            if let Some(id) = write_media(
                db,
                tree_id,
                MediaWrite {
                    ingested,
                    title: title.clone(),
                    classification: *classification,
                    privacy: *privacy,
                    created_at: *created_at,
                    metadata,
                },
                summary,
            )
            .await
            {
                summary.media_count += 1;
                import_transcript(
                    db,
                    tree_id,
                    id,
                    *deposit_id,
                    *view_id,
                    deposits
                        .get(deposit_id)
                        .and_then(|deposit| deposit.views.first())
                        .and_then(|view| view.last_transcript.as_ref()),
                    summary,
                )
                .await;
                prepared.insert(*deposit_id, (id, HashMap::from([(*view_id, id)])));
            }
        }
    }

    prepared
}

/// Stores a multi-page deposit as a document with every page beneath it.
///
/// Page order is the deposit's own, not the order the pages happen to arrive
/// in: `append_page` indexes by how many pages are already there, so the pages
/// are sorted by their Geneanet page number before any of them is written.
#[allow(clippy::too_many_arguments)]
async fn document(
    db: &DatabaseConnection,
    store: &dyn MediaStore,
    tree_id: Uuid,
    deposit: &ManifestDeposit,
    deposit_sizes: &HashMap<i64, u64>,
    archives: &ArchiveSet,
    hashes: Option<&ContentIndex>,
    fetched: &HashMap<String, String>,
    progress: &ImportProgress,
    places: &mut HashMap<String, Uuid>,
    summary: &mut GeneanetImportSummary,
) -> Option<(Uuid, HashMap<i64, Uuid>)> {
    let document_id = Uuid::now_v7();
    let classification = media_classification(deposit);
    let privacy = geneanet_privacy(deposit);
    let created_at = geneanet_created_at(deposit);
    let metadata = media_metadata(db, tree_id, deposit, places, summary).await;
    match MediaRepo::create_document(db, document_id, tree_id, deposit.title.clone(), created_at)
        .await
    {
        Ok(_) => {
            if let Err(err) =
                update_media_metadata(db, document_id, classification, privacy, &metadata).await
            {
                summary
                    .skipped
                    .push(format!("deposit {}: {err}", deposit.id));
                return None;
            }
        }
        Err(err) => {
            summary
                .skipped
                .push(format!("deposit {}: {err}", deposit.id));
            return None;
        }
    }
    summary.media_count += 1;

    // Resolve every page first, then ingest them together. Decoding and
    // thumbnailing is what an import spends its minutes on — a 144-page
    // dossier is 144 full-size decodes — and each page is independent of the
    // others, so one at a time wastes every core but one.
    //
    // The *writes* stay sequential and in page order: `append_page` indexes by
    // how many pages are already there, so racing them would shuffle the
    // document.
    let mut resolved: Vec<(i64, i64, String, Vec<u8>)> = Vec::new();
    for view in pages_in_order(deposit) {
        let page = view.page.unwrap_or(0);
        match resolve_bytes(deposit, view, deposit_sizes, archives, hashes, fetched).await {
            Ok(bytes) => {
                resolved.push((view.id, page, photo_file_name(deposit, view, "jpg"), bytes))
            }
            Err(err) => {
                // A page that cannot be fetched leaves a gap, and the pages
                // after it move up one — so the number is recorded, or the
                // document would silently claim to be complete.
                summary
                    .skipped
                    .push(format!("deposit {} page {page}: {err}", deposit.id));
                progress.advance();
            }
        }
    }

    let mut stored = 0usize;
    let mut pages: HashMap<i64, Uuid> = HashMap::new();

    for chunk in resolved.chunks(ingest_width()) {
        let mut pending = FuturesUnordered::new();
        for (index, (_, _, name, bytes)) in chunk.iter().enumerate() {
            pending.push(async move {
                let outcome = media::ingest(store, tree_id, name, bytes.clone()).await;
                (index, outcome)
            });
        }
        let mut ingested = Vec::with_capacity(chunk.len());
        while let Some(outcome) = pending.next().await {
            progress.advance();
            ingested.push(outcome);
        }
        ingested.sort_unstable_by_key(|(index, _)| *index);

        for ((view_id, page, name, _), (_, outcome)) in chunk.iter().zip(ingested) {
            let ingested = match outcome {
                Ok(ingested) => ingested,
                Err(err) => {
                    summary.skipped.push(format!("{name}: {err}"));
                    continue;
                }
            };

            let Some(page_id) = write_media(
                db,
                tree_id,
                MediaWrite {
                    ingested,
                    title: None,
                    classification,
                    privacy,
                    created_at,
                    metadata: &metadata,
                },
                summary,
            )
            .await
            else {
                continue;
            };

            match MediaRepo::append_page(db, document_id, page_id).await {
                Ok(_) => {
                    stored += 1;
                    summary.media_count += 1;
                    pages.insert(*view_id, page_id);
                    import_transcript(
                        db,
                        tree_id,
                        page_id,
                        deposit.id,
                        *view_id,
                        deposit
                            .views
                            .iter()
                            .find(|view| view.id == *view_id)
                            .and_then(|view| view.last_transcript.as_ref()),
                        summary,
                    )
                    .await;
                }
                Err(err) => summary
                    .skipped
                    .push(format!("deposit {} page {page}: {err}", deposit.id)),
            }
        }
    }

    if stored == 0 {
        summary.skipped.push(format!(
            "deposit {}: none of its {} pages could be fetched",
            deposit.id,
            deposit.views.len()
        ));
    }

    Some((document_id, pages))
}

/// Creates a `Person` for each identification Geneanet marks as outside the
/// tree, so their photographs have somewhere to hang.
///
/// These are the references that carry a name but no GeneWeb key. Geneanet's
/// media manager labels them "hors de l'arbre", and that is a deliberate
/// statement by the owner: the person on the photograph is *not* the person of
/// that name in their tree. Matching them onto a namesake would therefore be
/// wrong, and is why [`oxidgene_geneanet::join`] refuses to.
///
/// But they are real people with real photographs, and dropping them loses
/// both. So each distinct name becomes an isolated person — no events, no
/// family links — and the media attach to them normally. On the reference
/// account that is 19 people carrying 33 identifications.
///
/// Keyed by folded name so the same person named on six photographs is created
/// once. Sex is unknown: Geneanet's identification records a name and nothing
/// else about them.
#[tracing::instrument(name = "import.identities", skip_all)]
async fn create_isolated_people(
    db: &DatabaseConnection,
    tree_id: Uuid,
    joined: &join::Join,
    summary: &mut GeneanetImportSummary,
) -> HashMap<String, Uuid> {
    let mut created: HashMap<String, Uuid> = HashMap::new();

    for unjoined in &joined.unjoined {
        if unjoined.reason != UnjoinedReason::NoKey {
            continue;
        }
        let (Some(lastname), Some(firstname)) = (&unjoined.lastname, &unjoined.firstname) else {
            continue;
        };

        let key = oxidgene_geneanet::key::geneanet_key(lastname, firstname, 0);
        if created.contains_key(&key) {
            continue;
        }

        let person_id = Uuid::now_v7();
        if let Err(err) =
            PersonRepo::create(db, person_id, tree_id, oxidgene_core::Sex::Unknown).await
        {
            summary.skipped.push(format!("{}: {err}", unjoined.name));
            continue;
        }

        let pieces = PersonNamePieces {
            given_names: Some(firstname.clone()),
            surname: Some(lastname.clone()),
            ..PersonNamePieces::default()
        };
        if let Err(err) = PersonNameRepo::create(
            db,
            Uuid::now_v7(),
            person_id,
            oxidgene_core::NameType::Birth,
            pieces,
            true,
            0,
        )
        .await
        {
            summary.skipped.push(format!("{}: {err}", unjoined.name));
            continue;
        }

        created.insert(key, person_id);
        summary.isolated_count += 1;
    }

    created
}

/// Drops the `.gw`'s portrait URLs and says which view each named.
///
/// GeneWeb records a person's portrait as `#image <url>`, which the importer
/// turns into a *remote* medium — a row pointing at a URL we never fetch. For
/// a Geneanet import that is exactly the photo we are importing properly, so
/// the remote row is removed here and the stored one takes its place, marked
/// as the person's profile photo.
///
/// The match is on the rendition path with its cache-busting query stripped,
/// against every rendition the collection lists — so it holds whichever size
/// the export happened to reference, and it is an equality, not a heuristic.
///
/// Returns `person xref → view id`.
fn take_portrait_urls(
    result: &mut oxidgene_gedcom::ImportResult,
    manifest: &Manifest,
) -> HashMap<String, i64> {
    // Every rendition of every view, so whichever one the `.gw` names is found.
    let mut view_of: HashMap<String, i64> = HashMap::new();
    let mut known: std::collections::BTreeSet<i64> = std::collections::BTreeSet::new();
    for deposit in &manifest.deposits {
        for view in &deposit.views {
            known.insert(view.id);
            for url in view.files.values() {
                view_of.insert(strip_query(url).to_string(), view.id);
            }
        }
    }

    // Which media rows are portraits we are about to replace.
    let mut replaced: HashMap<Uuid, i64> = HashMap::new();
    for medium in &result.media {
        let path = strip_query(&medium.file_path);
        // Host-relative in the manifest, absolute in the export.
        let tail = path.find("/public/").map_or(path, |at| &path[at..]);
        // Measured on a real export: 163 of 225 portraits name a rendition
        // path outright, and the other 62 name the *original* file
        // (`130436018.png` rather than `medium.jpg`). Both carry the view id in
        // the same place, so the id is the fallback and together they account
        // for every portrait.
        if let Some(view_id) = view_of
            .get(tail)
            .copied()
            .or_else(|| view_id_in_path(tail).filter(|id| known.contains(id)))
        {
            replaced.insert(medium.id, view_id);
        }
    }

    if replaced.is_empty() {
        return HashMap::new();
    }

    // The link is what says whose portrait it was.
    let mut portraits: HashMap<String, i64> = HashMap::new();
    let person_xref: HashMap<Uuid, String> = result
        .person_by_xref
        .iter()
        .map(|(xref, id)| (*id, xref.clone()))
        .collect();

    for link in &result.media_links {
        if let Some(view_id) = replaced.get(&link.media_id)
            && let Some(person_id) = link.person_id
            && let Some(xref) = person_xref.get(&person_id)
        {
            portraits.insert(xref.clone(), *view_id);
        }
    }

    result
        .media
        .retain(|medium| !replaced.contains_key(&medium.id));
    result
        .media_links
        .retain(|link| !replaced.contains_key(&link.media_id));

    portraits
}

/// A URL without its `?t=…` cache buster.
fn strip_query(url: &str) -> &str {
    url.split('?').next().unwrap_or(url)
}

/// The view id a CDN path carries.
///
/// Geneanet shards these as `…/deposits[/private]/<xx>/<yy>/<viewId>/…`, so
/// the id is the long numeric segment that follows a two-character one. That
/// shape is what distinguishes it from the original filename further along the
/// path, which is also numeric on some deposits.
fn view_id_in_path(path: &str) -> Option<i64> {
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    segments.iter().enumerate().find_map(|(index, segment)| {
        let follows_a_shard = index > 0 && segments[index - 1].len() == 2;
        let looks_like_an_id = segment.len() >= 6 && segment.chars().all(|c| c.is_ascii_digit());
        (follows_a_shard && looks_like_an_id)
            .then(|| segment.parse::<i64>().ok())
            .flatten()
    })
}

/// The distinct dimensions of every rendition this run could query the index
/// with.
///
/// Only multi-page deposits contribute: a single-page one is recognised by its
/// byte length and never reaches the perceptual matcher.
///
/// A rendition that is absent or does not decode contributes nothing. It can
/// never be hashed — [`oxidgene_geneanet::archive::image_dimensions`] and
/// `phash::hash_image` open the same decoder, so failing one means failing the
/// other — therefore it can never be matched, and admitting it as a target of
/// unknown size would widen the filter to accept every entry in the archive.
/// One scanned dossier among the pages used to cost a full-resolution decode
/// of the entire archive that way.
fn hashable_target_dimensions(
    deposits: &HashMap<i64, &ManifestDeposit>,
    by_deposit: &BTreeMap<i64, Vec<&join::Attachment>>,
    fetched: &HashMap<String, String>,
) -> std::collections::BTreeSet<(u32, u32)> {
    let mut dimensions = std::collections::BTreeSet::new();

    for deposit_id in by_deposit.keys() {
        let Some(deposit) = deposits.get(deposit_id) else {
            continue;
        };
        if deposit.views.len() <= 1 {
            continue;
        }
        for view in &deposit.views {
            let Some(bytes) = rendition_url(view).and_then(|url| read_fetched(fetched, &url))
            else {
                continue;
            };
            if let Some(found) = oxidgene_geneanet::archive::image_dimensions(&bytes) {
                dimensions.insert(found);
            }
        }
    }

    dimensions
}

/// Hashes the archive entries a document page might be, and only those.
///
/// Decoding is what this costs — several hundred full-size photographs — so it
/// is done once, before the loop, and over as few entries as possible:
///
/// - nothing at all unless some page of a **multi-page** deposit has a
///   rendition this run can actually hash, since that rendition is the only
///   thing the index is ever queried with and a single-page deposit is
///   recognised by its byte length instead;
/// - never an entry that byte length has *already* accounted for, which on
///   the reference archive removes 379 of 623 before a single decode;
/// - and never an entry whose aspect ratio matches no target, which only the
///   header is read to establish.
///
/// It runs under `block_in_place` because it is seconds of CPU inside an async
/// handler. Without that it pins a runtime worker for the duration, and the
/// rest of the app — the tree list, the page the user goes back to — waits
/// behind it.
#[tracing::instrument(name = "import.media.index", skip_all)]
fn build_content_index(
    deposits: &HashMap<i64, &ManifestDeposit>,
    by_deposit: &BTreeMap<i64, Vec<&join::Attachment>>,
    deposit_sizes: &HashMap<i64, u64>,
    archives: &ArchiveSet,
    fetched: &HashMap<String, String>,
) -> Option<ContentIndex> {
    if archives.is_empty() {
        return None;
    }

    let target_dimensions = hashable_target_dimensions(deposits, by_deposit, fetched);

    // Nothing hashable to match against: every rendition this import could
    // query the index with is absent or undecodable, so the index would be
    // built and never asked a question.
    if target_dimensions.is_empty() {
        return None;
    }

    let mut claimed: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
    for deposit_id in by_deposit.keys() {
        let Some(deposit) = deposits.get(deposit_id) else {
            continue;
        };
        if deposit.views.len() <= 1
            && let Some(size) = deposit_sizes.get(deposit_id)
            && let Ok(Some(position)) = archives.locate_by_size(*size)
        {
            claimed.insert(position);
        }
    }

    let candidates: Vec<usize> = (0..archives.entry_count())
        .filter(|position| !claimed.contains(position))
        .collect();
    let target_dimensions: Vec<_> = target_dimensions.into_iter().collect();

    // The renditions the run will look up. They are what decides where the
    // coarse tier stops: an entry falls to the fine tier precisely because no
    // page claimed it cheaply.
    let queries = hashable_renditions(deposits, by_deposit, fetched);

    let index = tokio::task::block_in_place(|| {
        ContentIndex::build(archives, &candidates, &target_dimensions, &queries)
    });
    let (coarse, fine) = index.hashed_counts();
    tracing::info!(
        candidates = candidates.len(),
        target_dimensions = target_dimensions.len(),
        filtered = index.filtered_count(),
        hashed_coarse = coarse,
        hashed_full = fine,
        undecodable = index.undecodable_count(),
        "built Geneanet archive perceptual index"
    );
    Some(index)
}

/// The rendition bytes of every page that could be looked up by content.
///
/// Same selection as [`hashable_target_dimensions`], carrying the bytes rather
/// than their shape.
fn hashable_renditions(
    deposits: &HashMap<i64, &ManifestDeposit>,
    by_deposit: &BTreeMap<i64, Vec<&join::Attachment>>,
    fetched: &HashMap<String, String>,
) -> Vec<Vec<u8>> {
    let mut renditions = Vec::new();

    for deposit_id in by_deposit.keys() {
        let Some(deposit) = deposits.get(deposit_id) else {
            continue;
        };
        if deposit.views.len() <= 1 {
            continue;
        }
        for view in &deposit.views {
            if let Some(bytes) = rendition_url(view).and_then(|url| read_fetched(fetched, &url)) {
                renditions.push(bytes);
            }
        }
    }

    renditions
}

/// A deposit's pages in the order they should be read.
///
/// Sorted by Geneanet's own page number rather than left in whatever order the
/// collection produced, because that order is what the reader sees:
/// `append_page` indexes each page by how many are already there, `list_pages`
/// reads back by that index, and the gallery renders that. So the sort here is
/// the only thing standing between a scanned dossier and a shuffled one — the
/// order pages are *fetched* in is irrelevant and must not leak into it.
///
/// A view with no page number sorts last, keeping the numbered ones in
/// sequence rather than interleaving an unknown among them.
fn pages_in_order(deposit: &ManifestDeposit) -> Vec<&ManifestView> {
    let mut pages: Vec<&ManifestView> = deposit.views.iter().collect();
    pages.sort_by_key(|view| (view.page.unwrap_or(i64::MAX), view.id));
    pages
}

/// Where an import records how far it has got.
///
/// An import writes ten thousand people and several hundred pictures, and the
/// pictures are minutes of decoding. Without this the wizard shows a bar that
/// cannot move and a user cannot tell a long import from a hung one — which is
/// exactly the report that prompted it.
///
/// Shared rather than returned because the run holds the request open for its
/// whole duration; the wizard asks a second endpoint how it is going.
#[derive(Debug, Default)]
pub struct ImportProgress {
    /// Units completed in the current phase.
    done: std::sync::atomic::AtomicUsize,
    /// Units expected in the current phase, or zero when it is not measurable.
    total: std::sync::atomic::AtomicUsize,
    /// What the run is doing, for a line above the bar.
    phase: std::sync::Mutex<ImportPhase>,
}

/// The stages an import passes through, in order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportPhase {
    #[default]
    Starting,
    /// Writing people, families and events in measured database batches.
    People,
    /// Hashing the archives so document pages can be recognised.
    Matching,
    /// Storing pictures, counted per processed medium.
    Media,
    /// Rebuilding the projections the tree is read through.
    Finishing,
}

impl ImportProgress {
    pub fn begin(&self, phase: ImportPhase, total: usize) {
        if let Ok(mut current) = self.phase.lock() {
            *current = phase;
        }
        self.done.store(0, std::sync::atomic::Ordering::Relaxed);
        self.total
            .store(total, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn advance(&self) {
        self.advance_by(1);
    }

    pub fn advance_by(&self, amount: usize) {
        self.done
            .fetch_add(amount, std::sync::atomic::Ordering::Relaxed);
    }

    /// What to show: the phase and its current measured progress.
    #[must_use]
    pub fn read(&self) -> (ImportPhase, usize, usize) {
        (
            self.phase.lock().map(|p| *p).unwrap_or_default(),
            self.done.load(std::sync::atomic::Ordering::Relaxed),
            self.total.load(std::sync::atomic::Ordering::Relaxed),
        )
    }
}

fn persisted_entity_count(result: &oxidgene_gedcom::ImportResult) -> usize {
    result.places.len()
        + result.sources.len()
        + result.media.len()
        + result.persons.len()
        + result.person_names.len()
        + result.families.len()
        + result.family_spouses.len()
        + result.family_children.len()
        + result.events.len()
        + result.event_witnesses.len()
        + result.citations.len()
        + result.media_links.len()
        + result.vignettes.len()
        + result.notes.len()
}

/// How many media to decode at once.
///
/// Decoding and thumbnailing is CPU-bound and each medium is independent, so
/// this is the machine's parallelism — capped, because every one in flight
/// holds a full-size decoded image in memory and a scanned page is tens of
/// megabytes decoded.
///
/// Shared with the GEDZIP importer, which ingests the same way from a
/// different source and has no reason to pick a different width.
pub(crate) fn ingest_width() -> usize {
    oxidgene_core::resources::cpu_worker_limit().min(8)
}

struct MediaWrite<'a> {
    ingested: crate::media::IngestedMedia,
    title: Option<String>,
    classification: (
        oxidgene_core::enums::SourceMediaType,
        Option<oxidgene_core::enums::DocumentCategory>,
    ),
    privacy: Privacy,
    created_at: DateTime<Utc>,
    metadata: &'a MediaMetadata,
}

/// Writes the `media` row for something already ingested.
async fn write_media(
    db: &DatabaseConnection,
    tree_id: Uuid,
    media: MediaWrite<'_>,
    summary: &mut GeneanetImportSummary,
) -> Option<Uuid> {
    let MediaWrite {
        ingested,
        title,
        classification,
        privacy,
        created_at,
        metadata,
    } = media;
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
        title,
        description: None,
        created_at,
    };

    match MediaRepo::create_uploaded(db, Uuid::now_v7(), tree_id, upload).await {
        Ok(row) => match update_media_metadata(db, row.id, classification, privacy, metadata).await
        {
            Ok(_) => Some(row.id),
            Err(err) => {
                summary.skipped.push(format!("{err}"));
                None
            }
        },
        Err(err) => {
            summary.skipped.push(format!("{err}"));
            None
        }
    }
}

async fn import_transcript(
    db: &DatabaseConnection,
    tree_id: Uuid,
    media_id: Uuid,
    deposit_id: i64,
    view_id: i64,
    transcript: Option<&GeneanetTranscript>,
    summary: &mut GeneanetImportSummary,
) {
    let Some(transcript) = transcript else {
        return;
    };
    let content = transcript.content.trim();
    if content.is_empty() {
        return;
    }

    match NoteRepo::create(
        db,
        Uuid::now_v7(),
        tree_id,
        content.to_string(),
        None,
        None,
        None,
        None,
        Some(media_id),
    )
    .await
    {
        Ok(_) => summary.notes_count += 1,
        Err(error) => tracing::warn!(
            deposit_id,
            view_id,
            transcript_id = transcript.id,
            %error,
            "could not import Geneanet transcript"
        ),
    }
}

/// Returns Geneanet's source creation timestamp when it is an RFC 3339 date.
///
/// Collection data is intentionally lenient: a missing or malformed value
/// must not discard a medium, so it keeps the normal OxidGene creation time.
fn geneanet_created_at(deposit: &ManifestDeposit) -> DateTime<Utc> {
    deposit
        .date_create
        .as_deref()
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))
        .unwrap_or_else(Utc::now)
}

/// Date and place attached to a Geneanet deposit, in OxidGene's normal model.
#[derive(Debug, Clone)]
struct MediaMetadata {
    date: oxidgene_gedcom::date::ImportedDate,
    place_id: Option<Uuid>,
}

async fn media_metadata(
    db: &DatabaseConnection,
    tree_id: Uuid,
    deposit: &ManifestDeposit,
    places: &mut HashMap<String, Uuid>,
    summary: &mut GeneanetImportSummary,
) -> MediaMetadata {
    let place_id = match deposit
        .location
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        Some(name) => {
            let key = place_key(name);
            if let Some(id) = places.get(&key).copied() {
                Some(id)
            } else {
                match PlaceRepo::create(db, Uuid::now_v7(), tree_id, name.to_string(), None, None)
                    .await
                {
                    Ok(place) => {
                        places.insert(key, place.id);
                        Some(place.id)
                    }
                    Err(err) => {
                        summary
                            .warnings
                            .push(format!("deposit {} place {name:?}: {err}", deposit.id));
                        None
                    }
                }
            }
        }
        None => None,
    };

    MediaMetadata {
        date: geneanet_media_date(deposit),
        place_id,
    }
}

fn place_key(name: &str) -> String {
    name.trim().to_lowercase()
}

/// Converts Geneanet's ISO-like partial date to the GEDCOM phrase the rest of
/// OxidGene parses, validates and sorts. Geneanet exposes no calendar, so the
/// source is Gregorian; the resulting fields remain editable with the normal
/// calendar-aware media date widget.
fn geneanet_media_date(deposit: &ManifestDeposit) -> oxidgene_gedcom::date::ImportedDate {
    let Some(raw) = deposit
        .date
        .as_deref()
        .map(str::trim)
        .filter(|date| !date.is_empty())
    else {
        return oxidgene_gedcom::date::ImportedDate::default();
    };

    let parts: Vec<&str> = raw.split('-').collect();
    let normalized = match parts.as_slice() {
        [year, month, day] if year.len() == 4 => match (month.parse::<u32>(), day.parse::<u32>()) {
            (Ok(0), Ok(0)) => (*year).to_string(),
            (Ok(month), Ok(0)) if (1..=12).contains(&month) => {
                format!("{} {year}", month_name(month))
            }
            (Ok(month), Ok(day)) if (1..=12).contains(&month) && (1..=31).contains(&day) => {
                format!("{day} {} {year}", month_name(month))
            }
            _ => raw.to_string(),
        },
        _ => raw.to_string(),
    };
    oxidgene_gedcom::date::parse(&normalized)
}

fn month_name(month: u32) -> &'static str {
    [
        "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
    ][(month - 1) as usize]
}

async fn update_media_metadata(
    db: &DatabaseConnection,
    media_id: Uuid,
    (source_media_type, document_category): (
        oxidgene_core::enums::SourceMediaType,
        Option<oxidgene_core::enums::DocumentCategory>,
    ),
    privacy: Privacy,
    metadata: &MediaMetadata,
) -> Result<(), OxidGeneError> {
    MediaRepo::update(
        db,
        media_id,
        MediaPatch {
            source_media_type: Some(source_media_type),
            document_category: Some(document_category),
            privacy: Some(privacy),
            date_value: Some(metadata.date.value.clone()),
            date_value2: Some(metadata.date.value2.clone()),
            date_qualifier: Some(metadata.date.qualifier),
            calendar: Some(metadata.date.calendar),
            date_sort: Some(metadata.date.sort),
            place_id: Some(metadata.place_id),
            ..MediaPatch::default()
        },
    )
    .await
    .map(|_| ())
}

/// Preserve the category Geneanet assigned to the record and its GEDCOM
/// physical medium. A label already in GEDCOM's vocabulary is a medium;
/// Geneanet-only labels use the richer document category instead.
fn media_classification(
    deposit: &ManifestDeposit,
) -> (
    oxidgene_core::enums::SourceMediaType,
    Option<oxidgene_core::enums::DocumentCategory>,
) {
    use oxidgene_core::enums::{DocumentCategory, SourceMediaType};

    let kind = deposit.kind.as_deref().unwrap_or_default().trim();
    if let Some(source_media_type) = SourceMediaType::parse(kind) {
        return (source_media_type, None);
    }

    let category = match kind.to_ascii_lowercase().as_str() {
        "portrait" | "portraits" => Some(DocumentCategory::Portrait),
        "photo_groupe" | "photo de groupe" | "photos de groupe" => {
            Some(DocumentCategory::GroupPhoto)
        }
        "document_familial" | "document familial" => Some(DocumentCategory::FamilyDocument),
        "acte_etat_civil" | "état civil" | "etat civil" => Some(DocumentCategory::CivilRecord),
        "registre_paroissial" | "registre paroissial" => Some(DocumentCategory::ParishRecord),
        "acte_notarie" | "archive notariée" | "archive notariale" => {
            Some(DocumentCategory::NotarialArchive)
        }
        "archive_militaire" | "archive militaire" => Some(DocumentCategory::MilitaryArchive),
        "recensement" | "recensements" => Some(DocumentCategory::Census),
        "blason" | "blasons" => Some(DocumentCategory::CoatOfArms),
        "tombe" | "tombeau" | "pierre tombale" => Some(DocumentCategory::Grave),
        "autres" | "other" | "" => Some(DocumentCategory::Other),
        // Geneanet's API is undocumented. Keep an unexpected value visibly
        // unclassified rather than claiming a medium it did not state.
        _ => Some(DocumentCategory::Other),
    };

    let source_media_type =
        category.map_or(SourceMediaType::Other, DocumentCategory::implied_medium);
    (source_media_type, category)
}

/// Geneanet's flag is an explicit visibility choice, not a request to follow
/// the tree default, so both values map to explicit OxidGene variants.
fn geneanet_privacy(deposit: &ManifestDeposit) -> Privacy {
    if deposit.private {
        Privacy::Private
    } else {
        Privacy::Public
    }
}

/// Gets a medium's bytes, preferring a copy the user already has.
///
/// Three sources, most exact first:
///
/// 1. **Exact byte length** — only a single-page deposit states one, and it
///    identifies the file outright.
/// 2. **The same picture, recognised by content** — a page of a multi-page
///    deposit has no length to match on, so its rendition is hashed against
///    the archive. `fetched` carries that rendition, because the window is the
///    only thing that can retrieve it (see below).
/// 3. **The bytes themselves**, when the window had to fetch the original.
///
/// Nothing here reaches the network. Every direct request to Geneanet is
/// challenged by Cloudflare whatever the cookie, so the bytes arrive already
/// fetched by the login window and are handed in through `fetched`, keyed by
/// URL. A medium that none of the three can supply is reported and skipped.
async fn resolve_bytes(
    deposit: &ManifestDeposit,
    view: &ManifestView,
    deposit_sizes: &HashMap<i64, u64>,
    archives: &ArchiveSet,
    hashes: Option<&ContentIndex>,
    fetched: &HashMap<String, String>,
) -> Result<Vec<u8>, String> {
    // 1. A single-page deposit *is* the file, so its length names it.
    if deposit.views.len() == 1
        && let Some(size) = deposit_sizes.get(&deposit.id)
        && let Ok(Some(bytes)) = archives.resolve(*size)
    {
        return Ok(bytes);
    }

    let rendition = rendition_url(view);
    let stored_rendition = stored_rendition_url(view);

    // 2. Recognise it in the archive from its rendition.
    let sample = rendition
        .as_ref()
        .and_then(|url| read_fetched(fetched, url));

    if let Some(sample) = &sample
        && !archives.is_empty()
    {
        // A rendition we cannot decode is not a failure — it just cannot be
        // matched, and the fetched bytes below still stand.
        if let Some(index) = hashes
            && let Ok(Some(bytes)) = index.resolve(archives, sample)
        {
            return Ok(bytes);
        }
    }

    // 3. Whatever the window fetched for this view, original preferred.
    if let Some(bytes) = original_url(deposit)
        .and_then(|url| read_fetched(fetched, &url))
        .or_else(|| {
            stored_rendition
                .as_ref()
                .and_then(|url| read_fetched(fetched, url))
        })
        .or(sample)
    {
        return Ok(bytes);
    }

    Err("not in the archives, and the Geneanet window fetched no copy of it".to_string())
}

/// Reads a medium the login window wrote to disk.
///
/// A path that will not read is not an error worth stopping for: that medium
/// is reported as skipped and the rest of the import stands.
fn read_fetched(fetched: &HashMap<String, String>, url: &str) -> Option<Vec<u8>> {
    std::fs::read(fetched.get(url)?).ok()
}

/// The URL a deposit's original is downloaded from.
///
/// Deposit-level on purpose: it is one file when the deposit has a single page
/// and a ZIP of every page when it has several, which is why a page of a
/// multi-page deposit is recognised by content instead.
fn original_url(deposit: &ManifestDeposit) -> Option<String> {
    (!deposit.original.is_empty()).then(|| deposit.original.clone())
}

/// Picks the rendition to fetch for a page.
///
/// `medium` keeps pHash decoding cheap: the hash reduces its input to 32x32,
/// while decoding a `normal` page can involve millions of pixels. A separate
/// `normal` request supplies the bytes retained when archive matching fails.
fn rendition_url(view: &ManifestView) -> Option<String> {
    rendition_url_in_order(view, ["medium", "normal", "screen", "thumbnail"])
}

/// Picks the best per-page rendition to retain when no archive entry matches.
fn stored_rendition_url(view: &ManifestView) -> Option<String> {
    rendition_url_in_order(view, ["normal", "medium", "screen", "thumbnail"])
}

/// Returns the distinct pHash sample and stored fallback URLs for one page.
fn rendition_urls(view: &ManifestView) -> Vec<String> {
    let mut urls = Vec::new();
    for url in [rendition_url(view), stored_rendition_url(view)]
        .into_iter()
        .flatten()
    {
        if !urls.contains(&url) {
            urls.push(url);
        }
    }
    urls
}

fn rendition_url_in_order(view: &ManifestView, order: [&str; 4]) -> Option<String> {
    for rendition in order {
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
            last_transcript: None,
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
            date: None,
            location: None,
            local_file: None,
            views,
        }
    }

    /// A view whose only rendition is `url`, so a test can point each page at
    /// a file of its own. An absolute URL is used verbatim.
    fn view_with_rendition(id: i64, url: &str) -> ManifestView {
        ManifestView {
            id,
            page: Some(id),
            files: BTreeMap::from([("normal".to_string(), url.to_string())]),
            last_transcript: None,
            references: Vec::new(),
        }
    }

    fn png_bytes(width: u32, height: u32) -> Vec<u8> {
        let image = image::RgbImage::from_fn(width, height, |x, y| {
            let v = ((x * 7 + y * 13) % 251) as u8;
            image::Rgb([v, v, v])
        });
        let mut buffer = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image)
            .write_to(&mut buffer, image::ImageFormat::Png)
            .expect("encodes");
        buffer.into_inner()
    }

    #[test]
    fn an_undecodable_rendition_does_not_erase_the_readable_targets() {
        // A multi-page deposit whose pages include a scanned document the
        // image decoder cannot read. That page can never be matched by content
        // whatever we do, and letting it stand for "any dimensions" turned the
        // aspect-ratio filter off for the whole archive — several hundred
        // full-resolution decodes for nothing.
        let dir = tempfile::tempdir().expect("scratch directory");
        let readable = dir.path().join("page1.png");
        std::fs::write(&readable, png_bytes(40, 30)).expect("writes the readable page");
        let undecodable = dir.path().join("page2.bin");
        std::fs::write(&undecodable, b"not an image at all").expect("writes the broken page");

        let readable_url = "http://archive.invalid/page1.png";
        let undecodable_url = "http://archive.invalid/page2.bin";
        let scanned = deposit(
            7,
            vec![
                view_with_rendition(1, readable_url),
                view_with_rendition(2, undecodable_url),
            ],
        );

        let deposits = HashMap::from([(7i64, &scanned)]);
        let by_deposit = BTreeMap::from([(7i64, Vec::new())]);
        let fetched = HashMap::from([
            (
                readable_url.to_string(),
                readable.to_string_lossy().into_owned(),
            ),
            (
                undecodable_url.to_string(),
                undecodable.to_string_lossy().into_owned(),
            ),
        ]);

        let dimensions = hashable_target_dimensions(&deposits, &by_deposit, &fetched);

        assert_eq!(dimensions.iter().copied().collect::<Vec<_>>(), [(40, 30)]);
    }

    #[test]
    fn a_run_with_no_hashable_rendition_has_no_targets() {
        // And therefore builds no index: there would be nothing to query it
        // with.
        let dir = tempfile::tempdir().expect("scratch directory");
        let undecodable = dir.path().join("page1.bin");
        std::fs::write(&undecodable, b"not an image at all").expect("writes the broken page");

        let url = "http://archive.invalid/page1.bin";
        let scanned = deposit(
            7,
            vec![view_with_rendition(1, url), view_with_rendition(2, url)],
        );

        let deposits = HashMap::from([(7i64, &scanned)]);
        let by_deposit = BTreeMap::from([(7i64, Vec::new())]);
        let fetched =
            HashMap::from([(url.to_string(), undecodable.to_string_lossy().into_owned())]);

        assert!(hashable_target_dimensions(&deposits, &by_deposit, &fetched).is_empty());
    }

    #[test]
    fn a_single_page_deposit_is_never_a_perceptual_target() {
        // It is recognised by its exact byte length instead, so hashing
        // anything on its behalf would be wasted work.
        let dir = tempfile::tempdir().expect("scratch directory");
        let page = dir.path().join("only.png");
        std::fs::write(&page, png_bytes(40, 30)).expect("writes the page");

        let url = "http://archive.invalid/only.png";
        let portrait = deposit(7, vec![view_with_rendition(1, url)]);

        let deposits = HashMap::from([(7i64, &portrait)]);
        let by_deposit = BTreeMap::from([(7i64, Vec::new())]);
        let fetched = HashMap::from([(url.to_string(), page.to_string_lossy().into_owned())]);

        assert!(hashable_target_dimensions(&deposits, &by_deposit, &fetched).is_empty());
    }

    #[test]
    fn one_person_keeps_identifications_on_several_document_pages() {
        let person_id = Uuid::now_v7();
        let first_page_id = Uuid::now_v7();
        let second_page_id = Uuid::now_v7();
        let identifications = vec![
            Attached {
                person_id,
                is_portrait: false,
                view_id: 222,
                face: Some(oxidgene_geneanet::model::FacePosition {
                    x1: 10.0,
                    y1: 20.0,
                    x2: 30.0,
                    y2: 40.0,
                }),
            },
            Attached {
                person_id,
                is_portrait: true,
                view_id: 223,
                face: Some(oxidgene_geneanet::model::FacePosition {
                    x1: 50.0,
                    y1: 60.0,
                    x2: 70.0,
                    y2: 80.0,
                }),
            },
        ];
        let pages = HashMap::from([(222, first_page_id), (223, second_page_id)]);

        assert_eq!(linked_people(&identifications), vec![(person_id, true)]);
        let targets = page_identifications(&identifications, &pages);
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].0, first_page_id);
        assert_eq!(targets[0].2, person_id);
        assert_eq!(targets[1].0, second_page_id);
        assert_eq!(targets[1].2, person_id);
    }

    #[tokio::test]
    async fn transcript_becomes_a_note_on_the_given_media_record() {
        let db = oxidgene_db::repo::connect("sqlite::memory:")
            .await
            .expect("connects");
        oxidgene_db::repo::run_migrations(&db)
            .await
            .expect("migrates");
        let tree_id = Uuid::now_v7();
        TreeRepo::create(&db, tree_id, "Sample tree".to_string(), None)
            .await
            .expect("creates tree");
        let media_id = Uuid::now_v7();
        MediaRepo::create_document(&db, media_id, tree_id, None, Utc::now())
            .await
            .expect("creates media");
        let mut summary = GeneanetImportSummary::default();

        import_transcript(
            &db,
            tree_id,
            media_id,
            111,
            222,
            Some(&GeneanetTranscript {
                id: 333,
                content: "  Page transcript  ".to_string(),
            }),
            &mut summary,
        )
        .await;
        import_transcript(
            &db,
            tree_id,
            media_id,
            111,
            223,
            Some(&GeneanetTranscript {
                id: 334,
                content: String::new(),
            }),
            &mut summary,
        )
        .await;

        let notes = NoteRepo::list_by_entity(&db, tree_id, None, None, None, None, Some(media_id))
            .await
            .expect("lists notes");
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].media_id, Some(media_id));
        assert_eq!(notes[0].text, "Page transcript");
        assert_eq!(summary.notes_count, 1);
    }

    #[test]
    fn import_progress_is_determinate_and_resets_between_phases() {
        let progress = ImportProgress::default();

        progress.begin(ImportPhase::People, 250);
        progress.advance_by(100);
        assert_eq!(progress.read(), (ImportPhase::People, 100, 250));

        progress.advance_by(150);
        assert_eq!(progress.read(), (ImportPhase::People, 250, 250));

        progress.begin(ImportPhase::Matching, 0);
        assert_eq!(progress.read(), (ImportPhase::Matching, 0, 0));

        progress.begin(ImportPhase::Media, 12);
        progress.advance();
        assert_eq!(progress.read(), (ImportPhase::Media, 1, 12));
    }

    #[test]
    fn geneanet_metadata_keeps_its_physical_medium_and_privacy() {
        let portrait = deposit(1, vec![view(10, Some(1))]);
        assert_eq!(
            media_classification(&portrait),
            (
                oxidgene_core::enums::SourceMediaType::Photo,
                Some(oxidgene_core::enums::DocumentCategory::Portrait),
            )
        );

        let mut census = deposit(2, vec![view(20, Some(1))]);
        census.kind = Some("Archive notariée".to_string());
        assert_eq!(
            media_classification(&census),
            (
                oxidgene_core::enums::SourceMediaType::Manuscript,
                Some(oxidgene_core::enums::DocumentCategory::NotarialArchive),
            )
        );

        let mut unknown = deposit(3, vec![view(30, Some(1))]);
        unknown.kind = Some("autres".to_string());
        assert_eq!(
            media_classification(&unknown).0,
            oxidgene_core::enums::SourceMediaType::Other,
        );
        assert_eq!(
            media_classification(&unknown).1,
            Some(oxidgene_core::enums::DocumentCategory::Other),
        );

        let mut gedcom = deposit(4, vec![view(40, Some(1))]);
        gedcom.kind = Some("video".to_string());
        assert_eq!(
            media_classification(&gedcom),
            (oxidgene_core::enums::SourceMediaType::Video, None),
        );

        assert_eq!(geneanet_privacy(&portrait), Privacy::Private);
        let mut public = deposit(5, vec![view(50, Some(1))]);
        public.private = false;
        assert_eq!(geneanet_privacy(&public), Privacy::Public);
    }

    #[test]
    fn geneanet_creation_date_is_preserved_in_utc() {
        let mut deposit = deposit(1, vec![view(10, Some(1))]);
        deposit.date_create = Some("2023-08-17T19:22:56+02:00".to_string());

        assert_eq!(
            geneanet_created_at(&deposit).to_rfc3339(),
            "2023-08-17T17:22:56+00:00"
        );
    }

    #[test]
    fn geneanet_historical_dates_use_the_media_date_model() {
        let mut deposit = deposit(1, vec![view(10, Some(1))]);
        deposit.date = Some("1924-00-00".to_string());
        let date = geneanet_media_date(&deposit);

        assert_eq!(date.calendar, oxidgene_core::Calendar::Gregorian);
        assert_eq!(date.qualifier, oxidgene_core::DateQualifier::Exact);
        assert_eq!(date.value.as_deref(), Some("1924"));
        assert_eq!(date.sort, chrono::NaiveDate::from_ymd_opt(1924, 1, 1));

        deposit.date = Some("1946-09-03".to_string());
        let date = geneanet_media_date(&deposit);
        assert_eq!(date.value.as_deref(), Some("3 SEP 1946"));
        assert_eq!(date.sort, chrono::NaiveDate::from_ymd_opt(1946, 9, 3));
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

    fn collection_with(deposits: &str) -> String {
        format!(r#"{{"deposits":[{deposits}],"references":[],"view_references":{{}}}}"#)
    }

    #[test]
    fn a_cache_buster_does_not_stop_a_portrait_matching() {
        // The `.gw` writes `…/medium.jpg?t=1785419513` and the collection
        // writes the same path with a different `t`. Comparing them whole
        // would never match, and every portrait would stay a dead URL beside
        // the photo we imported.
        assert_eq!(
            strip_query("/public/img/media/deposits/private/f2/29/9224/h/medium.jpg?t=178"),
            "/public/img/media/deposits/private/f2/29/9224/h/medium.jpg"
        );
        assert_eq!(strip_query("/a/b.jpg"), "/a/b.jpg");
    }

    #[test]
    fn a_view_id_is_read_out_of_a_cdn_path() {
        // The fallback for the portraits that name the original file rather
        // than a rendition — 62 of 225 on a real export.
        assert_eq!(
            view_id_in_path("/public/img/media/deposits/private/70/96/92668479/hash/130436018.png"),
            Some(92668479)
        );
        // Public deposits have no `private` segment, and the shape still holds.
        assert_eq!(
            view_id_in_path("/public/img/media/deposits/18/eb/92241319/129860128.png"),
            Some(92241319)
        );
    }

    #[test]
    fn a_filename_is_not_mistaken_for_a_view_id() {
        // `130436018.png` is numeric and long, and it is not the id. What
        // separates them is the two-character shard directory before the id.
        assert_eq!(view_id_in_path("/public/img/1234567890.png"), None);
        assert_eq!(view_id_in_path("/a/b/c.jpg"), None);
    }

    #[test]
    fn a_portrait_url_is_matched_whichever_rendition_the_export_named() {
        // A `.gw` references whichever size Geneanet gave it — `medium` on one
        // account, `normal` on another — so all four are candidates.
        let deposit = deposit(1, vec![view(10, Some(1))]);
        let renditions: Vec<&String> = deposit.views[0].files.values().collect();

        assert!(
            !renditions.is_empty(),
            "a view lists its renditions, and each is a way the export could name it"
        );
    }

    #[test]
    fn a_document_with_a_linked_page_counts_all_its_pages_not_one() {
        // What the preview promises has to be what the import does. It brings
        // a document in whole, so counting only the linked page would tell the
        // user 9 photos where 244 pages are coming.
        let single = deposit(1, vec![view(10, Some(1))]);
        let multi = deposit(
            2,
            vec![view(20, Some(1)), view(21, Some(2)), view(22, Some(3))],
        );

        assert_eq!(single.views.len(), 1);
        assert_eq!(pages_in_order(&multi).len(), 3);
    }

    #[test]
    fn the_plan_asks_for_every_page_of_an_attached_document() {
        // A page has no byte length to match an archive entry against, so each
        // one's rendition has to be fetched before it can be recognised.
        let multi = deposit(2, vec![view(20, Some(1)), view(21, Some(2))]);
        let urls: Vec<Option<String>> = pages_in_order(&multi)
            .iter()
            .map(|view| rendition_url(view))
            .collect();

        assert_eq!(urls.len(), 2);
        assert!(urls.iter().all(Option::is_some));
    }

    #[test]
    fn the_plan_asks_for_nothing_when_no_deposit_is_attached() {
        // Nothing joined, so nothing is imported, so the login window is not
        // asked to fetch a single byte.
        let gw = b"encoding: utf-8\n\nfam BRANCH_A person_a.0 + BRANCH_B person_b.0\n";
        let collection = collection_with(
            r#"{"id":1,"title":"t","type":"portraits","private":true,
                "views":[{"id":10,"page":1,"files":{"normal":"/n.jpg"}}]}"#,
        );

        for fidelity in [MediaFidelity::Renditions, MediaFidelity::Originals] {
            let needed = plan(
                gw,
                "tree.gw",
                &collection,
                &HashMap::new(),
                &ArchiveSet::new(),
                fidelity,
            )
            .expect("plans");

            assert!(
                needed.is_empty(),
                "{fidelity:?} planned an unattached deposit"
            );
        }
    }

    /// A `.gw` and a collection whose one deposit is attached to a person of
    /// it, so a plan and a preview both have something to say about them.
    ///
    /// `deposit` is spliced in verbatim: the caller decides whether it is a
    /// single photograph or a document. The link is placed through
    /// `view_references`, which pins it to a page — the bulk endpoint cannot,
    /// which is why the collection carries both shapes.
    fn attached_case(deposit: &str) -> (&'static [u8], String) {
        let gw: &[u8] = b"encoding: utf-8\n\nfam BRANCH_A person_a.0 + BRANCH_B person_b.0\n";
        let collection = format!(
            r#"{{"deposits":[{deposit}],
                 "references":[],
                 "view_references":{{"1:10":[
                   {{"lastname":"BRANCH_A","firstname":"person_a",
                     "reference_extra_geneweb":{{"ref":"branch a|person a|"}}}}
                 ]}}}}"#
        );
        (gw, collection)
    }

    #[test]
    fn the_shared_attached_case_really_does_attach() {
        // Every assertion below rests on this: a case whose reference joined
        // nobody would make an empty plan look like a correct one.
        let (gw, collection) = attached_case(
            r#"{"id":1,"title":"t","type":"portraits","private":true,
                "views":[{"id":10,"page":1,"files":{"normal":"/n1.jpg"}}]}"#,
        );

        let stats = preview(
            gw,
            "tree.gw",
            &collection,
            &HashMap::new(),
            &ArchiveSet::new(),
            MediaFidelity::Renditions,
        )
        .expect("previews");

        assert_eq!(stats.attachment_count, 1);
        assert_eq!(stats.unlinked_views, 0);
    }

    #[test]
    fn renditions_ask_for_one_normal_per_page_and_never_an_original() {
        // The whole point of the mode: no archive to match against, so no
        // original download and no smaller sample to hash — one fetch per
        // view, and it is the one whose bytes get stored.
        let (gw, collection) = attached_case(
            r#"{"id":1,"title":"t","type":"portraits","private":true,
                "views":[{"id":10,"page":1,"files":{"normal":"/n1.jpg","medium":"/m1.jpg"}},
                         {"id":11,"page":2,"files":{"normal":"/n2.jpg","medium":"/m2.jpg"}}]}"#,
        );

        let needed = plan(
            gw,
            "tree.gw",
            &collection,
            &HashMap::new(),
            &ArchiveSet::new(),
            MediaFidelity::Renditions,
        )
        .expect("plans");

        let urls: Vec<&str> = needed.iter().map(|item| item.url.as_str()).collect();
        assert_eq!(
            urls,
            [
                "https://gw.geneanet.org/n1.jpg",
                "https://gw.geneanet.org/n2.jpg"
            ]
        );
        assert!(needed.iter().all(|item| !item.original));
    }

    #[test]
    fn renditions_ignore_a_size_that_would_have_matched_an_archive() {
        // Originals skip a deposit whose length names an archive entry.
        // Renditions have no archive in play at all, so the same deposit is
        // still fetched — and as its rendition, not its original.
        let (gw, collection) = attached_case(
            r#"{"id":1,"title":"t","type":"portraits","private":true,
                "views":[{"id":10,"page":1,"files":{"normal":"/n1.jpg"}}]}"#,
        );

        let needed = plan(
            gw,
            "tree.gw",
            &collection,
            &HashMap::from([(1i64, 4096u64)]),
            &ArchiveSet::new(),
            MediaFidelity::Renditions,
        )
        .expect("plans");

        assert_eq!(needed.len(), 1);
        assert_eq!(needed[0].url, "https://gw.geneanet.org/n1.jpg");
    }

    #[test]
    fn originals_still_ask_for_the_deposit_download_of_a_single_page() {
        let (gw, collection) = attached_case(
            r#"{"id":1,"title":"t","type":"portraits","private":true,
                "views":[{"id":10,"page":1,"files":{"normal":"/n1.jpg"}}]}"#,
        );

        let needed = plan(
            gw,
            "tree.gw",
            &collection,
            &HashMap::new(),
            &ArchiveSet::new(),
            MediaFidelity::Originals,
        )
        .expect("plans");

        assert_eq!(needed.len(), 1);
        assert!(needed[0].url.contains("deposits[]=1"));
        assert!(needed[0].original);
    }

    #[test]
    fn a_renditions_preview_counts_every_page_as_a_download_and_matches_none() {
        // Nothing is held locally and nothing is recognised by content, so
        // reporting either would promise work this mode does not do — while
        // the deposit is still a document with both its pages.
        let (gw, collection) = attached_case(
            r#"{"id":1,"title":"t","type":"documents","private":true,
                "views":[{"id":10,"page":1,"files":{"normal":"/n1.jpg"}},
                         {"id":11,"page":2,"files":{"normal":"/n2.jpg"}}]}"#,
        );

        let stats = preview(
            gw,
            "tree.gw",
            &collection,
            &HashMap::new(),
            &ArchiveSet::new(),
            MediaFidelity::Renditions,
        )
        .expect("previews");

        assert_eq!(stats.to_download, 2);
        assert_eq!(stats.to_match, 0);
        assert_eq!(stats.in_archives, 0);
        assert_eq!(stats.documents, 1);
        assert_eq!(stats.document_pages, 2);
    }

    #[test]
    fn a_plan_entry_names_where_the_window_should_fetch_from() {
        // The two shapes: a single-page deposit yields its exact original, a
        // document's page yields the rendition a content match runs on.
        let single = deposit(1, vec![view(10, Some(1))]);
        let multi = deposit(2, vec![view(20, Some(1)), view(21, Some(2))]);

        assert!(original_url(&single).is_some_and(|u| u.contains("deposits[]=1")));
        assert!(
            rendition_url(&multi.views[1])
                .is_some_and(|u| u.starts_with("https://gw.geneanet.org"))
        );
    }

    #[test]
    fn a_documents_pages_are_ordered_by_their_page_number_not_their_arrival() {
        // The order the collection happens to list views in, or the order the
        // window fetches them in, must not reach the reader. `append_page`
        // indexes by how many pages are already there, so whatever order this
        // returns is the order the gallery shows.
        let shuffled = deposit(
            1,
            vec![view(30, Some(3)), view(10, Some(1)), view(20, Some(2))],
        );

        let pages: Vec<i64> = pages_in_order(&shuffled)
            .iter()
            .map(|view| view.page.unwrap_or(0))
            .collect();

        assert_eq!(pages, vec![1, 2, 3]);
    }

    #[test]
    fn a_page_with_no_number_sorts_last_rather_than_among_the_numbered() {
        let odd = deposit(
            1,
            vec![view(30, None), view(10, Some(1)), view(20, Some(2))],
        );

        let ids: Vec<i64> = pages_in_order(&odd).iter().map(|view| view.id).collect();

        assert_eq!(ids, vec![10, 20, 30]);
    }

    fn geneanet_event(name: &str, date: &str, location: Option<&str>) -> GeneanetEvent {
        GeneanetEvent {
            id: 1,
            name: Some(name.to_string()),
            kind: None,
            date: Some(date.to_string()),
            location: location.map(str::to_string),
        }
    }

    fn event_matcher(person_id: Uuid, candidates: Vec<ImportedEvent>) -> GeneanetEventMatcher {
        GeneanetEventMatcher {
            candidates_by_person: HashMap::from([(person_id, candidates)]),
        }
    }

    #[test]
    fn geneanet_event_names_cover_individual_and_family_vocabularies() {
        assert_eq!(
            geneanet_event_type(Some("gw_event_burial")),
            Some(EventType::Burial)
        );
        assert_eq!(
            geneanet_event_type(Some("gw_event_civil_union")),
            Some(EventType::CivilUnion)
        );
        assert_eq!(
            geneanet_event_type(Some("gw_event_military_promotion")),
            Some(EventType::MilitaryPromotion)
        );
        assert_eq!(geneanet_event_type(Some("gw_event_future")), None);
    }

    #[test]
    fn a_geneanet_event_link_requires_one_exact_imported_event() {
        let person_id = Uuid::now_v7();
        let event_id = Uuid::now_v7();
        let matcher = event_matcher(
            person_id,
            vec![ImportedEvent {
                id: event_id,
                event_type: EventType::Marriage,
                date: NaiveDate::from_ymd_opt(1912, 6, 15),
                place: Some(place_key("Paris, France")),
            }],
        );

        assert_eq!(
            matcher.resolve(
                person_id,
                &geneanet_event("gw_event_marriage", "1912-06-15", Some("Paris, France")),
            ),
            Some(event_id)
        );
        assert_eq!(
            matcher.resolve(
                person_id,
                &geneanet_event("gw_event_marriage", "1912-06-15", Some("Lyon, France")),
            ),
            None
        );
    }

    #[test]
    fn an_ambiguous_geneanet_event_link_is_not_imported() {
        let person_id = Uuid::now_v7();
        let candidates = (0..2)
            .map(|_| ImportedEvent {
                id: Uuid::now_v7(),
                event_type: EventType::Death,
                date: NaiveDate::from_ymd_opt(1944, 8, 20),
                place: None,
            })
            .collect();
        let matcher = event_matcher(person_id, candidates);

        assert_eq!(
            matcher.resolve(
                person_id,
                &geneanet_event("gw_event_death", "1944-08-20", None),
            ),
            None
        );
    }

    #[test]
    fn pages_sharing_a_number_keep_a_stable_order() {
        // Two views claiming the same page would otherwise sort
        // unpredictably, and a document that reorders itself between imports
        // is worse than one that is merely wrong once.
        let clash = deposit(1, vec![view(30, Some(1)), view(10, Some(1))]);

        let ids: Vec<i64> = pages_in_order(&clash).iter().map(|view| view.id).collect();

        assert_eq!(ids, vec![10, 30]);
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
    fn phash_and_storage_use_separate_page_renditions() {
        let mut page = view(1, Some(1));
        page.files
            .insert("medium".to_string(), "/a/b/medium.png".to_string());

        assert_eq!(
            rendition_url(&page).as_deref(),
            Some("https://gw.geneanet.org/a/b/medium.png")
        );
        assert_eq!(
            stored_rendition_url(&page).as_deref(),
            Some("https://gw.geneanet.org/a/b/normal.png")
        );
        assert_eq!(
            rendition_urls(&page),
            vec![
                "https://gw.geneanet.org/a/b/medium.png",
                "https://gw.geneanet.org/a/b/normal.png"
            ]
        );
    }

    #[test]
    fn a_missing_phash_rendition_does_not_duplicate_the_stored_fallback() {
        let page = view(1, Some(1));

        assert_eq!(
            rendition_url(&page).as_deref(),
            Some("https://gw.geneanet.org/a/b/normal.png")
        );
        assert_eq!(rendition_urls(&page).len(), 1);
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
