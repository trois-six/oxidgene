//! REST handlers for vignettes — named rectangles cut out of a stored media
//! file, and the cropped images they stand for.

use super::authorize::Viewer;
use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE, ETAG, IF_NONE_MATCH};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use oxidgene_core::OxidGeneError;
use oxidgene_core::types::{RecordKind, Vignette};
use oxidgene_db::repo::{MediaRepo, VignetteRepo};
use uuid::Uuid;

use super::dto::VignetteListQuery;
use super::error::ApiError;
use super::state::AppState;
use crate::media::thumbnail::Rendition;
use crate::service::scope::{TreeResource, require_tree_resource};
use crate::service::vignette::{self, NewVignette, VignetteUpdate};

/// GET /api/v1/trees/:tree_id/media/:media_id/vignettes
pub async fn list_media_vignettes(
    State(state): State<AppState>,
    viewer: Viewer,
    Path((tree_id, media_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<Vignette>>, ApiError> {
    require_tree_resource(&state.reader, tree_id, TreeResource::Media, media_id).await?;
    let records = state.records(&viewer, tree_id).await?;
    records.require_id(RecordKind::Media, media_id)?;
    let vignettes = VignetteRepo::list_for_media(&state.reader, media_id).await?;
    Ok(Json(records.vignettes(vignettes)))
}

/// GET /api/v1/trees/:tree_id/vignettes?person_id=…&event_id=…
///
/// Exactly one filter is required: an unfiltered list of every crop in a tree
/// is not a view anything needs, and paginating one would be busywork.
pub async fn list_vignettes(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<VignetteListQuery>,
) -> Result<Json<Vec<Vignette>>, ApiError> {
    if let Some(person_id) = query.person_id {
        require_tree_resource(&state.reader, tree_id, TreeResource::Person, person_id).await?;
    }
    if let Some(event_id) = query.event_id {
        require_tree_resource(&state.reader, tree_id, TreeResource::Event, event_id).await?;
    }
    let vignettes = match (query.person_id, query.event_id) {
        (Some(person_id), None) => VignetteRepo::list_for_person(&state.reader, person_id).await,
        (None, Some(event_id)) => VignetteRepo::list_for_event(&state.reader, event_id).await,
        _ => {
            return Err(ApiError(OxidGeneError::Validation(
                "exactly one of person_id or event_id is required".into(),
            )));
        }
    }?;
    let records = state.records(&viewer, tree_id).await?;
    Ok(Json(records.vignettes(vignettes)))
}

/// POST /api/v1/trees/:tree_id/media/:media_id/vignettes
pub async fn create_vignette(
    State(state): State<AppState>,
    viewer: Viewer,
    Path((tree_id, media_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<NewVignette>,
) -> Result<(StatusCode, Json<Vignette>), ApiError> {
    let vignette = vignette::create_vignette(&viewer, &state.db, tree_id, media_id, body).await?;
    Ok((StatusCode::CREATED, Json(vignette)))
}

/// GET /api/v1/trees/:tree_id/vignettes/:vignette_id
pub async fn get_vignette(
    State(state): State<AppState>,
    viewer: Viewer,
    Path((tree_id, vignette_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vignette>, ApiError> {
    require_tree_resource(&state.reader, tree_id, TreeResource::Vignette, vignette_id).await?;
    let records = state.records(&viewer, tree_id).await?;
    let vignette = VignetteRepo::get(&state.reader, vignette_id).await?;
    Ok(Json(records.require_vignette(vignette)?))
}

/// PUT /api/v1/trees/:tree_id/vignettes/:vignette_id
pub async fn update_vignette(
    State(state): State<AppState>,
    viewer: Viewer,
    Path((tree_id, vignette_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<VignetteUpdate>,
) -> Result<Json<Vignette>, ApiError> {
    Ok(Json(
        vignette::update_vignette(
            &viewer,
            &state.db,
            &*state.media,
            tree_id,
            vignette_id,
            body,
        )
        .await?,
    ))
}

/// DELETE /api/v1/trees/:tree_id/vignettes/:vignette_id
pub async fn delete_vignette(
    State(state): State<AppState>,
    viewer: Viewer,
    Path((tree_id, vignette_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    vignette::delete_vignette(
        &viewer,
        &state.db,
        &state.profiles,
        &*state.media,
        tree_id,
        vignette_id,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/trees/:tree_id/vignettes/:vignette_id/image
///
/// The cropped region, as its own JPEG at thumbnail size.
///
/// Cut on its first view rather than when the vignette is drawn, and stored
/// then (`media::crops`): a vignette stays cheap enough to create freely —
/// eight entries on one register page cost eight rows until somebody looks
/// at them — and a second view reads a few kilobytes instead of decoding the
/// page again.
pub async fn vignette_image(
    State(state): State<AppState>,
    viewer: Viewer,
    Path((tree_id, vignette_id)): Path<(Uuid, Uuid)>,
    request_headers: HeaderMap,
) -> Result<Response, ApiError> {
    cropped_image(
        &state,
        &viewer,
        tree_id,
        vignette_id,
        &request_headers,
        Rendition::Thumbnail,
    )
    .await
}

/// GET /api/v1/trees/:tree_id/vignettes/:vignette_id/card
///
/// The cropped region at the size of a portrait on a card.
pub async fn vignette_card(
    State(state): State<AppState>,
    viewer: Viewer,
    Path((tree_id, vignette_id)): Path<(Uuid, Uuid)>,
    request_headers: HeaderMap,
) -> Result<Response, ApiError> {
    cropped_image(
        &state,
        &viewer,
        tree_id,
        vignette_id,
        &request_headers,
        Rendition::Card,
    )
    .await
}

/// Vignette `vignette_id` of tree `tree_id`, cut and sized for `rendition`.
async fn cropped_image(
    state: &AppState,
    viewer: &Viewer,
    tree_id: Uuid,
    vignette_id: Uuid,
    request_headers: &HeaderMap,
    rendition: Rendition,
) -> Result<Response, ApiError> {
    require_tree_resource(&state.reader, tree_id, TreeResource::Vignette, vignette_id).await?;
    let records = state.records(viewer, tree_id).await?;
    let vignette =
        records.require_vignette(VignetteRepo::get(&state.reader, vignette_id).await?)?;
    let media = MediaRepo::get(&state.reader, vignette.media_id).await?;

    if media.storage_key.is_none() {
        return Err(ApiError(OxidGeneError::NotFound {
            entity: "Media file",
            id: media.id,
        }));
    }
    // A PDF's thumbnail shows one image of its first page, not the page a
    // region was drawn on: crops are raster-only.
    if !crate::media::thumbnail::can_crop(&media.mime_type) {
        return Err(ApiError(OxidGeneError::Validation(format!(
            "cannot crop a {} — crops are raster-only",
            media.mime_type
        ))));
    }

    // The fingerprint names everything the crop depends on, hashed: a strong
    // validator that reveals nothing. It is compared only once the viewer
    // has been allowed to see the vignette, so one who lost access is
    // answered 404, never 304.
    let etag = crate::media::crops::fingerprint(&media, &vignette, rendition)
        .map(|fingerprint| format!("\"{fingerprint}\""));
    let mut headers = HeaderMap::new();
    if let Some(etag) = &etag {
        headers.insert(ETAG, super::media::header_value(etag));
    }
    // Revalidated on every use: the rectangle can move and the page's file
    // be replaced under the same address, and the old picture must not be
    // drawn meanwhile. A revalidation costs no cut and no read of the store.
    headers.insert(
        CACHE_CONTROL,
        super::media::header_value("private, no-cache"),
    );
    #[cfg(feature = "auth")]
    super::caching::vary_on_credentials(&mut headers);
    if let (Some(etag), Some(requested)) = (&etag, request_headers.get(IF_NONE_MATCH))
        && requested
            .to_str()
            .is_ok_and(|value| value.split(',').any(|candidate| candidate.trim() == etag))
    {
        return Ok((StatusCode::NOT_MODIFIED, headers).into_response());
    }

    let (cropped, written) =
        crate::media::crops::read_or_cut(&*state.media, &media, &vignette, rendition).await?;
    if let Some(written) = written {
        crate::media::crops::confirm(&state.reader, &*state.media, &[written]).await;
    }

    headers.insert(CONTENT_TYPE, super::media::header_value("image/jpeg"));
    headers.insert(
        CONTENT_LENGTH,
        super::media::header_value(&cropped.len().to_string()),
    );
    Ok((headers, Body::from(cropped)).into_response())
}
