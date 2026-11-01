//! REST handler for the entry forms' value suggestions.

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use uuid::Uuid;

use crate::service::suggestions::{self, NameScope, SuggestionField, ValueSuggestion};

use super::authorize::Viewer;
use super::error::ApiError;
use super::state::AppState;

#[derive(Debug, Deserialize)]
pub struct ValueSuggestionQuery {
    q: String,
    lang: String,
    limit: Option<usize>,
    #[serde(flatten)]
    scope: NameScope,
}

/// GET /api/v1/trees/:tree_id/suggestions/:field?q=...&lang=...&limit=...&surname=...&given_names=...
pub async fn suggest(
    State(state): State<AppState>,
    viewer: Viewer,
    Path((tree_id, field)): Path<(Uuid, SuggestionField)>,
    Query(query): Query<ValueSuggestionQuery>,
) -> Result<Json<Vec<ValueSuggestion>>, ApiError> {
    let seen = viewer.redaction(&state, tree_id).await?;
    Ok(Json(
        suggestions::suggest(
            &state.reader,
            &seen,
            tree_id,
            field,
            &query.lang,
            &query.q,
            query.limit,
            &query.scope,
        )
        .await?,
    ))
}
