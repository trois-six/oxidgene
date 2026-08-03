//! Middleware rejecting requests scoped to a tree that no longer exists.
//!
//! Tree deletion is asynchronous: the request flags `deleted_at` and a
//! background worker removes the rows a few seconds later (see
//! [`crate::service::purge`]). Without this guard the tree's children stay
//! readable for that window — `GET /api/v1/trees/{id}/persons` would still
//! answer for a tree the client was just told was deleted.
//!
//! It also closes a gap that predates that change: the tree-scoped handlers
//! take `tree_id` from the path but most never check it, so a request naming a
//! tree that never existed got a `200` with empty results rather than a `404`.
//!
//! One indexed primary-key lookup per tree-scoped request, applied in one
//! place instead of repeated across fifteen handlers.

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use oxidgene_db::repo::TreeRepo;
use uuid::Uuid;

use super::error::ApiError;
use super::state::AppState;

/// Reject the request when its path names a tree that is missing or deleted.
///
/// Runs inside the `/api/v1/trees` nest, so the path it sees is either `/`
/// (list and create, which name no tree) or `/{tree_id}/...`. Anything whose
/// first segment is not a UUID is passed through untouched — routing will
/// produce its own `404`.
pub async fn require_live_tree(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let first_segment = req.uri().path().trim_start_matches('/');
    let first_segment = first_segment.split('/').next().unwrap_or_default();

    if let Ok(tree_id) = Uuid::parse_str(first_segment) {
        // `get` already filters on `deleted_at`, so a soft-deleted tree is a
        // NotFound here just as it is in the tree list. Reusing `ApiError`
        // keeps the body identical to the one the handlers produce.
        match TreeRepo::get(&state.db, tree_id).await {
            Ok(_) => {}
            Err(e @ oxidgene_core::OxidGeneError::NotFound { .. }) => {
                return ApiError(e).into_response();
            }
            // A database failure is not the client's fault — let the handler
            // run and report the real error rather than masking it as a 404.
            Err(_) => {}
        }
    }

    next.run(req).await
}
