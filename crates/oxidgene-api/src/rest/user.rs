//! REST handlers of other users as their viewers see them
//! (docs/authentication.md §6.7): a profile and its avatar. Web build only
//! (`auth` feature). There is no user directory: a user the viewer may not
//! see does not exist (`404`).

use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use uuid::Uuid;

use super::authorize::Viewer;
use super::error::ApiError;
use super::state::AppState;
use crate::service::account::{self, UserProfile};
use crate::service::avatar;

/// GET /api/v1/users/{user_id}
pub async fn get_user_profile(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(user_id): Path<Uuid>,
) -> Result<Json<UserProfile>, ApiError> {
    let user = account::visible_user(&state.reader, viewer.principal(), user_id).await?;
    Ok(Json(UserProfile::from(&user)))
}

/// Query of an avatar read.
#[derive(Debug, Deserialize)]
pub struct AvatarQuery {
    /// `256` (the default) or `64` pixels.
    pub size: Option<u32>,
    /// The avatar's `etag`, which a client may add so that a replaced
    /// avatar is a new address for its cache; not read.
    pub v: Option<String>,
}

/// GET /api/v1/users/{user_id}/avatar?size=256|64
///
/// The uploaded avatar as JPEG, to whoever may see the user; `404` for a
/// user with the generated avatar, which the frontend draws itself.
pub async fn get_user_avatar(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(user_id): Path<Uuid>,
    Query(query): Query<AvatarQuery>,
    request_headers: HeaderMap,
) -> Result<Response, ApiError> {
    let size = query.size.unwrap_or(256);
    let image = avatar::avatar_image(&state.reader, viewer.principal(), user_id, size).await?;
    let etag = format!("\"{}\"", image.etag);
    let mut headers = HeaderMap::new();
    headers.insert(header::ETAG, value(&etag));
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=86400"),
    );
    headers.insert(
        header::VARY,
        HeaderValue::from_static("Cookie, Authorization"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("sandbox"),
    );
    let matches = request_headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.split(',').any(|candidate| candidate.trim() == etag));
    if matches {
        return Ok((StatusCode::NOT_MODIFIED, headers).into_response());
    }
    // Read only once the client is known to lack them.
    let bytes = state.media.get(&image.key).await?;
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/jpeg"));
    Ok((headers, Body::from(bytes)).into_response())
}

fn value(text: &str) -> HeaderValue {
    HeaderValue::from_str(text).unwrap_or(HeaderValue::from_static("\"\""))
}
