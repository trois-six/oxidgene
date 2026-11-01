//! REST handlers of the signed-in user's own account (docs/authentication.md
//! §6.6, §6.7): the profile, the avatar, the sessions, the account data and
//! the account's deletion. Web build only (`auth` feature).

use axum::Json;
use axum::extract::{Multipart, Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use oxidgene_core::OxidGeneError;
use uuid::Uuid;

use super::authorize::Viewer;
use super::error::ApiError;
use super::state::AppState;
use crate::service::account::{self, Me, ProfilePatch, SessionView};
use crate::service::avatar::{self, Crop, MAX_AVATAR_BYTES};

/// The body limit of an avatar upload: the image's 5 MiB and the form
/// around it.
pub const AVATAR_BODY_LIMIT: usize = MAX_AVATAR_BYTES + 64 * 1024;

/// A response that clears the session cookie: the request's own session
/// ended.
fn signed_out(mut response: Response) -> Response {
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_static(
            "__Host-oxidgene=; Path=/; Secure; HttpOnly; SameSite=Strict; Max-Age=0",
        ),
    );
    response
}

/// GET /api/v1/me
pub async fn get_me(State(state): State<AppState>, viewer: Viewer) -> Result<Json<Me>, ApiError> {
    let user = account::me(&state.reader, viewer.principal()).await?;
    Ok(Json(Me::from(&user)))
}

/// PUT /api/v1/me
pub async fn update_me(
    State(state): State<AppState>,
    viewer: Viewer,
    Json(body): Json<ProfilePatch>,
) -> Result<Json<Me>, ApiError> {
    let user = account::update_profile(&state.db, viewer.principal(), body).await?;
    Ok(Json(Me::from(&user)))
}

/// DELETE /api/v1/me
///
/// Erase the account; the browser is signed out.
pub async fn delete_me(
    State(state): State<AppState>,
    viewer: Viewer,
) -> Result<Response, ApiError> {
    account::delete_my_account(&state.db, &state.auth, &state.media, viewer.principal()).await?;
    Ok(signed_out(StatusCode::NO_CONTENT.into_response()))
}

/// GET /api/v1/me/export
///
/// The account data as a JSON download.
pub async fn export_me(
    State(state): State<AppState>,
    viewer: Viewer,
) -> Result<Response, ApiError> {
    let data = account::account_data(&state.reader, viewer.principal()).await?;
    let mut response = Json(data).into_response();
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=\"oxidgene-account.json\""),
    );
    Ok(response)
}

/// PUT /api/v1/me/avatar
///
/// Multipart: `file`, the image, and optionally the crop square `x`, `y`
/// and `size`, in pixels of the upright image; without them the centred
/// largest square.
pub async fn upload_avatar(
    State(state): State<AppState>,
    viewer: Viewer,
    multipart: Multipart,
) -> Result<Json<Me>, ApiError> {
    let (bytes, crop) = read_avatar_form(multipart).await?;
    let user = avatar::set_avatar(&state.db, &state.media, viewer.principal(), bytes, crop).await?;
    Ok(Json(Me::from(&user)))
}

/// DELETE /api/v1/me/avatar
pub async fn delete_avatar(
    State(state): State<AppState>,
    viewer: Viewer,
) -> Result<StatusCode, ApiError> {
    avatar::delete_avatar(&state.db, &state.media, viewer.principal()).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/me/sessions
pub async fn list_sessions(
    State(state): State<AppState>,
    viewer: Viewer,
) -> Result<Json<Vec<SessionView>>, ApiError> {
    Ok(Json(
        account::my_sessions(&state.auth, viewer.principal()).await?,
    ))
}

/// DELETE /api/v1/me/sessions/{session_id}
///
/// Ending the request's own session signs the browser out.
pub async fn end_session(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(session_id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let current = account::end_my_session(&state.auth, viewer.principal(), session_id).await?;
    let response = StatusCode::NO_CONTENT.into_response();
    Ok(if current {
        signed_out(response)
    } else {
        response
    })
}

/// The image and the crop of an avatar upload.
async fn read_avatar_form(mut multipart: Multipart) -> Result<(Vec<u8>, Option<Crop>), ApiError> {
    let malformed = || ApiError(OxidGeneError::Validation("malformed avatar upload".into()));
    let mut file = None;
    let (mut x, mut y, mut size) = (None, None, None);
    while let Some(field) = multipart.next_field().await.map_err(|_| malformed())? {
        let name = field.name().unwrap_or_default().to_string();
        if name == "file" {
            file = Some(field.bytes().await.map_err(|_| malformed())?.to_vec());
            continue;
        }
        let slot = match name.as_str() {
            "x" => &mut x,
            "y" => &mut y,
            "size" => &mut size,
            _ => continue,
        };
        let text = field.text().await.map_err(|_| malformed())?;
        *slot = Some(text.trim().parse::<u32>().map_err(|_| malformed())?);
    }
    let file = file.ok_or_else(malformed)?;
    let crop = match (x, y, size) {
        (Some(x), Some(y), Some(size)) => Some(Crop { x, y, size }),
        (None, None, None) => None,
        _ => return Err(malformed()),
    };
    Ok((file, crop))
}
