//! REST handlers for the Geneanet import wizard.
//!
//! Four calls, one per step that needs the server. Step 3 — signing in and
//! collecting the person↔photo mapping — has no endpoint at all: it happens
//! inside the WebView the user authenticated in, because that is the only
//! place a Geneanet session exists. What arrives here is its output.
//!
//! Two of these take **filesystem paths** rather than uploads, which is only
//! sound because the wizard's archive steps are desktop-only: there the server
//! runs in-process and reads the very files the user picked. The web build
//! never calls them — it has no WebView to sign in with either, so the whole
//! photo half of the flow is out of reach and the tab says so.

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use oxidgene_core::OxidGeneError;
use uuid::Uuid;

use oxidgene_geneanet::session;

use super::dto::{
    DecodeSessionResponse, EncodeSessionRequest, GeneanetImportRequest, GeneanetImportResponse,
    GeneanetPlanResponse, GeneanetPreviewRequest, GeneanetPreviewResponse, ImportGenewebQuery,
    ImportProgressResponse, IndexArchivesRequest, IndexArchivesResponse, IndexedArchive,
    InspectGenewebResponse, NeededMedia,
};
use super::error::ApiError;
use super::state::AppState;
use crate::service::geneanet;

/// Default `origin_file` when the client sends no `?filename=`.
const DEFAULT_ORIGIN_FILE: &str = "import.gw";

/// POST /api/v1/geneweb/inspect
///
/// Parse a `.gw` file and report what it holds, writing nothing.
///
/// The body is the **raw file content**, for the same reason
/// [`super::geneweb::import_geneweb_handler`] takes one: `.gw` is ISO-8859-1
/// unless it opts into UTF-8, so decoding it upstream would mangle the accented
/// names the join key is built from.
///
/// Not scoped to a tree: this runs before the user has chosen one, and it is
/// what tells them whether they picked the right export.
pub async fn inspect_geneweb_handler(
    Query(query): Query<ImportGenewebQuery>,
    body: Bytes,
) -> Result<Json<InspectGenewebResponse>, ApiError> {
    let file_name = query.filename.as_deref().unwrap_or(DEFAULT_ORIGIN_FILE);

    let inspection = geneanet::inspect_gw(&body, file_name).map_err(ApiError::from)?;

    Ok(Json(InspectGenewebResponse {
        person_count: inspection.person_count,
        family_count: inspection.family_count,
        skipped_blocks: inspection.skipped_blocks,
    }))
}

/// POST /api/v1/geneanet/archives
///
/// Index the central directory of each named data archive, extracting nothing.
///
/// An archive that cannot be read is reported in its own row rather than
/// failing the request: users add several at once and one corrupt ZIP is no
/// reason to discard the four that opened.
pub async fn index_archives_handler(
    Json(body): Json<IndexArchivesRequest>,
) -> Result<Json<IndexArchivesResponse>, ApiError> {
    let (set, reports) = geneanet::index_archives(&body.paths);

    Ok(Json(IndexArchivesResponse {
        file_count: <_ as oxidgene_geneanet::archive::LocalOriginals>::file_count(&set),
        archives: reports
            .into_iter()
            .map(|report| IndexedArchive {
                path: report.path,
                file_name: report.file_name,
                file_count: report.file_count,
                image_count: report.image_count,
                error: report.error,
            })
            .collect(),
    }))
}

/// POST /api/v1/geneanet/preview
///
/// Join the collected mapping onto the `.gw` and report what an import would
/// do. No network access and no writes — this is the moment the user finds out
/// whether the two halves belong to each other, *before* anything is written.
pub async fn preview_handler(
    Json(body): Json<GeneanetPreviewRequest>,
) -> Result<Json<GeneanetPreviewResponse>, ApiError> {
    let gw = decode_gw(&body.gw_base64)?;
    let (archives, _) = geneanet::index_archives(&body.archive_paths);

    let preview = geneanet::preview(
        &gw,
        &body.file_name,
        &body.collection,
        &body.deposit_sizes,
        &archives,
    )
    .map_err(ApiError::from)?;

    Ok(Json(GeneanetPreviewResponse {
        person_count: preview.person_count,
        photo_count: preview.photo_count,
        persons_with_photo: preview.persons_with_photo,
        attachment_count: preview.attachment_count,
        in_archives: preview.in_archives,
        to_match: preview.to_match,
        to_download: preview.to_download,
        group_photos: preview.group_photos,
        unlinked_views: preview.unlinked_views,
        documents: preview.documents,
        document_pages: preview.document_pages,
        unlinked_names: preview.unlinked_names,
        outside_tree: preview.outside_tree,
        ambiguous: preview.ambiguous,
        unlinked_names_sample: preview.unlinked_names_sample,
        outside_tree_names: preview.outside_tree_names,
        ambiguous_names: preview.ambiguous_names,
        mismatch: preview.mismatch,
    }))
}

/// POST /api/v1/geneanet/session/encode
///
/// Turn what the login window collected into the file the wizard offers to
/// save. Step 3 is the only part of the import that talks to Geneanet, and the
/// expensive half of it is one `HEAD` per deposit — several hundred on a real
/// account — so its result is worth keeping rather than re-collecting on every
/// run.
pub async fn encode_session_handler(
    Json(body): Json<EncodeSessionRequest>,
) -> Result<Response, ApiError> {
    // The wizard holds paths; the archive holds bytes. Read them here rather
    // than making the UI carry several hundred pictures through itself.
    let media = body
        .media
        .iter()
        .filter_map(|(url, path)| {
            use base64::Engine as _;
            let bytes = std::fs::read(path).ok()?;
            Some((
                url.clone(),
                base64::engine::general_purpose::STANDARD.encode(bytes),
            ))
        })
        .collect();

    let archive = session::encode(&session::Session {
        collection: body.collection,
        deposit_sizes: body.deposit_sizes,
        account: body.account,
        media,
    })
    .map_err(|e| ApiError::from(OxidGeneError::Validation(e.to_string())))?;

    // The bytes themselves: the wizard writes them straight to the file the
    // user chose, and wrapping an archive in JSON would only base64 it again.
    Ok((
        [
            (header::CONTENT_TYPE, "application/zip"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"geneanet-session.zip\"",
            ),
        ],
        archive,
    )
        .into_response())
}

/// POST /api/v1/geneanet/session/decode
///
/// Read a saved session back, refusing anything that is not one. The body is
/// the file itself.
pub async fn decode_session_handler(body: Bytes) -> Result<Json<DecodeSessionResponse>, ApiError> {
    // Bytes, not text: the file is an archive now, and a bare JSON one is
    // still told apart by content rather than by extension.
    let restored = session::decode(&body)
        .map_err(|e| ApiError::from(OxidGeneError::Validation(e.to_string())))?;

    // Reported rather than inferred from the sizes, which only cover the
    // single-page deposits.
    let photo_count = oxidgene_geneanet::manifest_from_collection(&restored.collection)
        .map(|manifest| manifest.view_count)
        .unwrap_or(0);

    // Written out rather than handed back as bytes, so a loaded session looks
    // exactly like a freshly gathered one: the wizard holds paths either way,
    // and an air-gapped import reads them from disk like any other.
    let media = stage_media(&restored.media)?;

    Ok(Json(DecodeSessionResponse {
        collection: restored.collection,
        deposit_sizes: restored.deposit_sizes,
        account: restored.account,
        photo_count,
        media,
    }))
}

/// POST /api/v1/geneanet/plan
///
/// Say which media the server cannot produce on its own, so the login window
/// can fetch them. Takes the same inputs as the preview.
///
/// This exists because the server never reaches Geneanet: every direct request
/// is challenged whatever the cookie, so the bytes come from the window the
/// user signed in to and are handed back with the import.
pub async fn plan_handler(
    Json(body): Json<GeneanetPreviewRequest>,
) -> Result<Json<GeneanetPlanResponse>, ApiError> {
    let gw = decode_gw(&body.gw_base64)?;
    let (archives, _) = geneanet::index_archives(&body.archive_paths);

    let needed = geneanet::plan(
        &gw,
        &body.file_name,
        &body.collection,
        &body.deposit_sizes,
        &archives,
    )
    .map_err(ApiError::from)?;

    Ok(Json(GeneanetPlanResponse {
        needed: needed
            .into_iter()
            .map(|item| NeededMedia {
                deposit_id: item.deposit_id,
                view_id: item.view_id,
                page: item.page,
                url: item.url,
                original: item.original,
            })
            .collect(),
    }))
}

/// POST /api/v1/trees/:tree_id/geneanet/import
///
/// Import the tree and attach every photo that joins onto it.
///
/// A photo that cannot be fetched is reported in `skipped` and the run
/// continues: by the time media are being written the people are already in
/// the database, and losing one scan is not a reason to throw away ten
/// thousand persons.
pub async fn import_handler(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<GeneanetImportRequest>,
) -> Result<(StatusCode, Json<GeneanetImportResponse>), ApiError> {
    let gw = decode_gw(&body.gw_base64)?;

    // Registered before the run so the first poll finds it, and removed after
    // however the run ends — a progress entry outliving its import would be a
    // slow leak of one map entry per import.
    let progress = std::sync::Arc::new(geneanet::ImportProgress::default());
    if let Some(id) = body.progress_id
        && let Ok(mut running) = state.imports.lock()
    {
        running.insert(id, std::sync::Arc::clone(&progress));
    }

    let summary = geneanet::import(
        &state.db,
        &*state.media,
        tree_id,
        &gw,
        &body.file_name,
        &body.collection,
        &body.deposit_sizes,
        &body.archive_paths,
        &body.fetched,
        &progress,
    )
    .await;

    progress.enter(geneanet::ImportPhase::Finishing);
    let summary = summary.map_err(|e| {
        forget_progress(&state, body.progress_id);
        ApiError::from(e)
    })?;

    // Eagerly rebuild every projection of this tree — same rationale as the
    // GEDCOM and GeneWeb import paths.
    state
        .profiles
        .rebuild_tree_full(&state.db, tree_id)
        .await
        .map_err(ApiError::from)?;

    forget_progress(&state, body.progress_id);

    Ok((
        StatusCode::CREATED,
        Json(GeneanetImportResponse {
            persons_count: summary.persons_count,
            families_count: summary.families_count,
            events_count: summary.events_count,
            sources_count: summary.sources_count,
            places_count: summary.places_count,
            notes_count: summary.notes_count,
            media_count: summary.media_count,
            links_count: summary.links_count,
            portraits_count: summary.portraits_count,
            isolated_count: summary.isolated_count,
            vignettes_count: summary.vignettes_count,
            skipped: summary.skipped,
            warnings: summary.warnings,
        }),
    ))
}

/// GET /api/v1/geneanet/import/{progress_id}
///
/// How far a running import has got. An import holds its own request open for
/// minutes, so this is the only way to say anything while it runs.
///
/// A run that has finished — or was never started — reports nothing rather
/// than 404ing, because the wizard polls right up to the moment the import
/// returns and a race there should not surface as an error.
pub async fn import_progress_handler(
    State(state): State<AppState>,
    Path(progress_id): Path<Uuid>,
) -> Json<Option<ImportProgressResponse>> {
    let running = state
        .imports
        .lock()
        .ok()
        .and_then(|running| running.get(&progress_id).cloned());

    Json(running.map(|progress| {
        let (phase, done, total) = progress.read();
        ImportProgressResponse { phase, done, total }
    }))
}

/// Drops a finished run's progress entry.
fn forget_progress(state: &AppState, progress_id: Option<Uuid>) {
    if let Some(id) = progress_id
        && let Ok(mut running) = state.imports.lock()
    {
        running.remove(&id);
    }
}

/// Writes a loaded session's media to disk and returns where each landed.
///
/// The import reads media from paths, so a session loaded from a file has to
/// look like one gathered live. Under the OS temp directory: these exist only
/// until the import has read them.
fn stage_media(
    media: &std::collections::HashMap<String, String>,
) -> Result<std::collections::HashMap<String, String>, ApiError> {
    use base64::Engine as _;

    if media.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    let directory = std::env::temp_dir().join(format!("oxidgene-geneanet-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&directory).map_err(|e| {
        ApiError::from(OxidGeneError::Internal(format!(
            "creating {}: {e}",
            directory.display()
        )))
    })?;

    Ok(media
        .iter()
        .enumerate()
        .filter_map(|(index, (url, encoded))| {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .ok()?;
            let path = directory.join(format!("{index:05}"));
            std::fs::write(&path, bytes).ok()?;
            Some((url.clone(), path.display().to_string()))
        })
        .collect())
}

/// Decodes the base64 the JSON bodies carry the `.gw` in.
///
/// JSON cannot hold the raw bytes and the raw bytes are what the reader needs,
/// so the two calls that bundle a `.gw` with other fields encode it. The two
/// that send nothing else take it as a raw body instead.
fn decode_gw(encoded: &str) -> Result<Vec<u8>, ApiError> {
    use base64::Engine as _;

    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| {
            ApiError::from(OxidGeneError::Validation(format!(
                "the .gw payload is not valid base64: {e}"
            )))
        })
}
