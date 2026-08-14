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
use axum::http::StatusCode;
use oxidgene_core::OxidGeneError;
use uuid::Uuid;

use super::dto::{
    GeneanetImportRequest, GeneanetImportResponse, GeneanetPreviewRequest, GeneanetPreviewResponse,
    ImportGenewebQuery, IndexArchivesRequest, IndexArchivesResponse, IndexedArchive,
    InspectGenewebResponse,
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
        to_download: preview.to_download,
        group_photos: preview.group_photos,
        unlinked_views: preview.unlinked_views,
        outside_tree: preview.outside_tree,
        ambiguous: preview.ambiguous,
        outside_tree_names: preview.outside_tree_names,
        ambiguous_names: preview.ambiguous_names,
        mismatch: preview.mismatch,
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

    let summary = geneanet::import(
        &state.db,
        &*state.media,
        tree_id,
        &gw,
        &body.file_name,
        &body.collection,
        &body.deposit_sizes,
        &body.archive_paths,
        body.cookie.as_deref(),
    )
    .await
    .map_err(ApiError::from)?;

    // Eagerly rebuild every projection of this tree — same rationale as the
    // GEDCOM and GeneWeb import paths.
    state
        .profiles
        .rebuild_tree_full(&state.db, tree_id)
        .await
        .map_err(ApiError::from)?;

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
            skipped: summary.skipped,
            warnings: summary.warnings,
        }),
    ))
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
