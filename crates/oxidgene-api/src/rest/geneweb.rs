//! REST handler for GeneWeb `.gw` import.

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use uuid::Uuid;

use super::dto::{ImportGenewebQuery, ImportResponse};
use super::error::ApiError;
use super::state::AppState;
use crate::service::geneweb;

/// Default `origin_file` when the client sends no `?filename=`.
const DEFAULT_ORIGIN_FILE: &str = "import.gw";

/// POST /api/v1/trees/:tree_id/geneweb/import
///
/// Import a GeneWeb `.gw` file into the given tree, persisting all extracted
/// entities.
///
/// The body is the **raw file content**, not JSON: `.gw` is ISO-8859-1 unless
/// the file opts into UTF-8 with an `encoding:` directive, so decoding it into
/// a JSON string upstream would mangle accented names. Send it as
/// `application/octet-stream` and pass the original file name as
/// `?filename=` — GeneWeb records it on every family and it is echoed back in
/// parse warnings.
///
/// There is no matching export: `.gw` is a read-only format in OxidGene.
pub async fn import_geneweb_handler(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<ImportGenewebQuery>,
    body: Bytes,
) -> Result<(StatusCode, Json<ImportResponse>), ApiError> {
    let origin_file = query.filename.as_deref().unwrap_or(DEFAULT_ORIGIN_FILE);

    let summary = geneweb::import_and_persist(&state.db, tree_id, &body, origin_file)
        .await
        .map_err(ApiError::from)?;

    // Eagerly rebuild every projection of this tree after an import — same
    // rationale as the GEDCOM path (see `rest::gedcom::import_gedcom_handler`).
    state
        .profiles
        .rebuild_tree_full(&state.db, tree_id)
        .await
        .map_err(ApiError::from)?;

    let response = ImportResponse {
        persons_count: summary.persons_count,
        families_count: summary.families_count,
        events_count: summary.events_count,
        sources_count: summary.sources_count,
        media_count: summary.media_count,
        places_count: summary.places_count,
        notes_count: summary.notes_count,
        warnings: summary.warnings,
    };

    Ok((StatusCode::CREATED, Json(response)))
}
