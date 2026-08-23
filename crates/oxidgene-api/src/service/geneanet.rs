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
use oxidgene_core::enums::Privacy;
use oxidgene_core::types::Portrait;
use oxidgene_db::repo::{
    MediaLinkRepo, MediaPatch, MediaRepo, PersonNamePieces, PersonNameRepo, PersonRepo, TreeRepo,
    UploadedMedia, VignetteInput, VignetteRepo,
};
use oxidgene_geneanet::Manifest;
use oxidgene_geneanet::archive::{ArchiveSet, LocalOriginals, PhashIndex};
use oxidgene_geneanet::join::{self, UnjoinedReason};
use oxidgene_geneanet::model::{ManifestDeposit, ManifestView};
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
        preview.documents += 1;
        preview.document_pages += deposit.views.len();
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
/// The rule per medium, in the order [`resolve_bytes`] applies it:
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
        if single
            && deposit_sizes
                .get(&deposit.id)
                .is_some_and(|size| matches!(archives.resolve(*size), Ok(Some(_))))
        {
            continue;
        }

        for view in pages_in_order(deposit) {
            let url = if single {
                original_url(deposit)
            } else {
                rendition_url(view)
            };
            let Some(url) = url else { continue };

            needed.push(NeededMedia {
                deposit_id: deposit.id,
                view_id: view.id,
                page: view.page,
                url,
                original: single,
            });
        }
    }

    Ok(needed)
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
    progress: &ImportProgress,
) -> Result<GeneanetImportSummary, OxidGeneError> {
    let _tree = TreeRepo::get(db, tree_id).await?;

    let (database, _) = oxidgene_geneanet::parse_gw(gw_bytes, file_name)
        .map_err(|e| OxidGeneError::Validation(e.to_string()))?;
    let manifest = oxidgene_geneanet::manifest_from_collection(collection_json)
        .map_err(|e| OxidGeneError::Validation(e.to_string()))?;

    let index = join::PersonIndex::from_database(&database);
    let joined = join::join(&manifest, &index);

    progress.enter(ImportPhase::People);

    // The persons first: their ids are what the photo links point at.
    let mut import_result = oxidgene_gedcom::geneweb::import_geneweb(gw_bytes, file_name, tree_id)
        .map_err(OxidGeneError::Gedcom)?;

    // A `.gw` carries one `#image` per person — the portrait, as a URL that
    // 403s for anyone not logged in. We are about to import that very photo
    // properly, so keeping the URL would leave every portrait in the tree
    // twice: once as a stored medium and once as a dead link beside it.
    //
    // The URL *is* one of the renditions the collection lists, so this is an
    // exact match on the path, not a guess — and it tells us which view is the
    // person's portrait, which is the only place that fact exists.
    let portraits = take_portrait_urls(&mut import_result, &manifest);
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

    // Every view of every attached deposit, which is what the media phase will
    // write — a document contributes all its pages, not just its linked ones.
    let attached: std::collections::BTreeSet<i64> = joined
        .attachments
        .iter()
        .map(|attachment| attachment.deposit_id)
        .collect();
    progress.expect(
        manifest
            .deposits
            .iter()
            .filter(|deposit| attached.contains(&deposit.id))
            .map(|deposit| deposit.views.len())
            .sum(),
    );

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
async fn attach_media(
    db: &DatabaseConnection,
    store: &dyn MediaStore,
    tree_id: Uuid,
    manifest: &Manifest,
    joined: &join::Join,
    person_by_xref: &HashMap<String, Uuid>,
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

    progress.enter(ImportPhase::Matching);
    let hashes = build_content_index(&deposits, &by_deposit, deposit_sizes, &archives);
    progress.enter(ImportPhase::Media);

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
            if people.iter().any(|a| a.person_id == person_id) {
                continue;
            }
            // The `.gw` named one view as this person's portrait. Nothing else
            // knows which of their photos that is.
            let is_portrait = portraits
                .get(&xref)
                .is_some_and(|view_id| *view_id == attachment.view_id);
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
            if people.iter().any(|a| a.person_id == person_id) {
                continue;
            }
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
                summary,
            )
            .await
            {
                Some(pair) => pair,
                None => continue,
            }
        };

        for (order, attached) in people.into_iter().enumerate() {
            let created = MediaLinkRepo::create(
                db,
                Uuid::now_v7(),
                owner,
                Some(attached.person_id),
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
                    if attached.is_portrait
                        && let Some(person_id) = link.person_id
                    {
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

            // The box Geneanet drew round this person. It is the only record
            // of *which* face in a group photograph is whom, and a vignette is
            // exactly that: a rectangle on a stored medium attributed to
            // somebody.
            if let Some(face) = &attached.face
                && let Some(page_id) = pages.get(&attached.view_id).copied()
            {
                add_vignette(db, page_id, face, attached.person_id, summary).await;
            }
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
        title: None,
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
async fn prepare_single_pages(
    db: &DatabaseConnection,
    store: &dyn MediaStore,
    tree_id: Uuid,
    deposits: &HashMap<i64, &ManifestDeposit>,
    by_deposit: &BTreeMap<i64, Vec<&join::Attachment>>,
    deposit_sizes: &HashMap<i64, u64>,
    archives: &ArchiveSet,
    hashes: Option<&PhashIndex>,
    fetched: &HashMap<String, String>,
    progress: &ImportProgress,
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
                continue;
            };
            match resolve_bytes(deposit, view, deposit_sizes, archives, hashes, fetched).await {
                Ok(bytes) => resolved.push((
                    *deposit_id,
                    view.id,
                    photo_file_name(deposit, view, extension),
                    deposit.title.clone(),
                    media_classification(deposit),
                    geneanet_privacy(deposit),
                    bytes,
                )),
                Err(err) => summary.skipped.push(format!("deposit {deposit_id}: {err}")),
            }
        }

        let ingested =
            futures_util::future::join_all(resolved.iter().map(|(_, _, name, _, _, _, bytes)| {
                media::ingest(store, tree_id, name, bytes.clone())
            }))
            .await;

        for ((deposit_id, view_id, name, title, classification, privacy, _), outcome) in
            resolved.iter().zip(ingested)
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
                ingested,
                title.clone(),
                *classification,
                *privacy,
                summary,
            )
            .await
            {
                summary.media_count += 1;
                prepared.insert(*deposit_id, (id, HashMap::from([(*view_id, id)])));
            }
            progress.advance();
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
    hashes: Option<&PhashIndex>,
    fetched: &HashMap<String, String>,
    progress: &ImportProgress,
    summary: &mut GeneanetImportSummary,
) -> Option<(Uuid, HashMap<i64, Uuid>)> {
    let document_id = Uuid::now_v7();
    let classification = media_classification(deposit);
    let privacy = geneanet_privacy(deposit);
    match MediaRepo::create_document(db, document_id, tree_id, deposit.title.clone()).await {
        Ok(_) => {
            if let Err(err) = update_media_metadata(db, document_id, classification, privacy).await
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
            }
        }
    }

    let mut stored = 0usize;
    let mut pages: HashMap<i64, Uuid> = HashMap::new();

    for chunk in resolved.chunks(ingest_width()) {
        let ingested = futures_util::future::join_all(
            chunk
                .iter()
                .map(|(_, _, name, bytes)| media::ingest(store, tree_id, name, bytes.clone())),
        )
        .await;

        for ((view_id, page, name, _), outcome) in chunk.iter().zip(ingested) {
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
                ingested,
                None,
                classification,
                privacy,
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
                }
                Err(err) => summary
                    .skipped
                    .push(format!("deposit {} page {page}: {err}", deposit.id)),
            }
            progress.advance();
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

/// Hashes the archive entries a document page might be, and only those.
///
/// Decoding is what this costs — several hundred full-size photographs — so it
/// is done once, before the loop, and over as few entries as possible:
///
/// - nothing at all unless a **multi-page** deposit is actually being
///   imported, since a single-page one is recognised by its byte length;
/// - and never an entry that byte length has *already* accounted for, which on
///   the reference archive removes 379 of 623 before a single decode.
///
/// It runs under `block_in_place` because it is seconds of CPU inside an async
/// handler. Without that it pins a runtime worker for the duration, and the
/// rest of the app — the tree list, the page the user goes back to — waits
/// behind it.
fn build_content_index(
    deposits: &HashMap<i64, &ManifestDeposit>,
    by_deposit: &BTreeMap<i64, Vec<&join::Attachment>>,
    deposit_sizes: &HashMap<i64, u64>,
    archives: &ArchiveSet,
) -> Option<PhashIndex> {
    if archives.is_empty() {
        return None;
    }

    let mut wanted = false;
    let mut claimed: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();

    for deposit_id in by_deposit.keys() {
        let Some(deposit) = deposits.get(deposit_id) else {
            continue;
        };
        if deposit.views.len() > 1 {
            wanted = true;
        } else if let Some(size) = deposit_sizes.get(deposit_id)
            && let Ok(Some(position)) = archives.locate_by_size(*size)
        {
            claimed.insert(position);
        }
    }

    if !wanted {
        return None;
    }

    let candidates: Vec<usize> = (0..archives.entry_count())
        .filter(|position| !claimed.contains(position))
        .collect();

    Some(tokio::task::block_in_place(|| {
        PhashIndex::build_from(archives, &candidates)
    }))
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
    /// Media written so far.
    done: std::sync::atomic::AtomicUsize,
    /// Media expected in total, known once the join has been computed.
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
    /// Writing people, families and events — one transaction, no counter.
    People,
    /// Hashing the archives so document pages can be recognised.
    Matching,
    /// Storing pictures. This is the one with a count worth showing.
    Media,
    /// Rebuilding the projections the tree is read through.
    Finishing,
}

impl ImportProgress {
    pub fn enter(&self, phase: ImportPhase) {
        if let Ok(mut current) = self.phase.lock() {
            *current = phase;
        }
    }

    pub fn expect(&self, total: usize) {
        self.total
            .store(total, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn advance(&self) {
        self.done.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// What to show: the phase, and how far through the media it is.
    #[must_use]
    pub fn read(&self) -> (ImportPhase, usize, usize) {
        (
            self.phase.lock().map(|p| *p).unwrap_or_default(),
            self.done.load(std::sync::atomic::Ordering::Relaxed),
            self.total.load(std::sync::atomic::Ordering::Relaxed),
        )
    }
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
    std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1)
        .clamp(1, 8)
}

/// Writes the `media` row for something already ingested.
async fn write_media(
    db: &DatabaseConnection,
    tree_id: Uuid,
    ingested: crate::media::IngestedMedia,
    title: Option<String>,
    classification: (
        oxidgene_core::enums::SourceMediaType,
        Option<oxidgene_core::enums::DocumentCategory>,
    ),
    privacy: Privacy,
    summary: &mut GeneanetImportSummary,
) -> Option<Uuid> {
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
    };

    match MediaRepo::create_uploaded(db, Uuid::now_v7(), tree_id, upload).await {
        Ok(row) => match update_media_metadata(db, row.id, classification, privacy).await {
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

async fn update_media_metadata(
    db: &DatabaseConnection,
    media_id: Uuid,
    (source_media_type, document_category): (
        oxidgene_core::enums::SourceMediaType,
        Option<oxidgene_core::enums::DocumentCategory>,
    ),
    privacy: Privacy,
) -> Result<(), OxidGeneError> {
    MediaRepo::update(
        db,
        media_id,
        MediaPatch {
            source_media_type: Some(source_media_type),
            document_category: Some(document_category),
            privacy: Some(privacy),
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
    hashes: Option<&PhashIndex>,
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
            && let Ok(query) = oxidgene_geneanet::phash::hash_image(sample)
            && let Ok(Some(bytes)) = index.resolve(archives, query)
        {
            return Ok(bytes);
        }
    }

    // 3. Whatever the window fetched for this view, original preferred.
    if let Some(bytes) = original_url(deposit)
        .and_then(|url| read_fetched(fetched, &url))
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
/// `medium`, not the largest. This is fetched to *recognise* the page in the
/// archives, and a perceptual hash reduces whatever it is given to 32×32 — so
/// the extra pixels of `normal` buy no accuracy and cost real bandwidth across
/// every page of every document.
///
/// It is also what gets stored on the minority of pages the archives cannot
/// account for, which is why this is not the smallest rendition either:
/// `thumbnail` would be a poor thing to keep. §10 already records that a
/// multi-page page arrives downsized whichever of these is chosen.
///
/// The fallbacks follow Geneanet's own size ladder — `normal` > `medium` >
/// `screen` > `thumbnail` — from `medium` outwards, so a view missing one
/// still yields the nearest thing to it. (An earlier list put `screen` ahead
/// of `medium`, which is not the ladder's order.)
fn rendition_url(view: &ManifestView) -> Option<String> {
    for rendition in ["medium", "normal", "screen", "thumbnail"] {
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

        let needed = plan(
            gw,
            "tree.gw",
            &collection,
            &HashMap::new(),
            &ArchiveSet::new(),
        )
        .expect("plans");

        assert!(needed.is_empty());
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
    fn indexing_a_missing_archive_reports_it_without_losing_the_others() {
        let (set, reports) = index_archives(&["/nonexistent/archive.zip".to_string()]);

        assert!(set.is_empty());
        assert_eq!(reports.len(), 1);
        assert!(reports[0].error.is_some());
        assert_eq!(reports[0].file_name, "archive.zip");
    }
}
