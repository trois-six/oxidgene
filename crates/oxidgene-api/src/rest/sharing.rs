//! REST handlers of sharing (docs/authorization.md §6, §10): sharing with
//! users and invitations by email, share links, and the preview of an
//! access scope. Web build only (`auth` feature), and absent from the
//! `none` mode, which has no accounts (`404`).

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use oxidgene_core::types::Connection;
use oxidgene_db::repo::PaginationParams;
use uuid::Uuid;

use super::authorize::Viewer;
use super::dto::PaginationQuery;
use super::error::ApiError;
use super::state::AppState;
use crate::service::access_scope::{self, ScopePreview, ScopePreviewRequest};
use crate::service::membership::MemberView;
use crate::service::share_link::{
    self, CreatedShareLink, NewShareLinkRequest, ShareLinkPatch, ShareLinkView,
};
use crate::service::sharing::{
    self, InvitationView, MyInvitation, ShareCandidate, ShareOutcome, ShareRequest,
};

fn params(query: &PaginationQuery) -> PaginationParams {
    PaginationParams {
        first: query.first.unwrap_or(25),
        after: query.after.clone(),
    }
}

/// POST /api/v1/trees/{tree_id}/members: a membership (`201`) or, for an
/// address no account has, an invitation (`202`).
pub async fn share_tree(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<ShareRequest>,
) -> Result<(StatusCode, Json<ShareOutcome>), ApiError> {
    let env = state.auth.sharing_env()?;
    let outcome = sharing::share(&viewer, &state.db, &env, tree_id, body).await?;
    let status = if outcome.member.is_some() {
        StatusCode::CREATED
    } else {
        StatusCode::ACCEPTED
    };
    Ok((status, Json(outcome)))
}

/// GET /api/v1/trees/{tree_id}/share-candidates
pub async fn share_candidates(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(tree_id): Path<Uuid>,
) -> Result<Json<Vec<ShareCandidate>>, ApiError> {
    state.auth.sharing_env()?;
    Ok(Json(
        sharing::share_candidates(&viewer, &state.reader, tree_id).await?,
    ))
}

/// GET /api/v1/trees/{tree_id}/invitations
pub async fn list_invitations(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<PaginationQuery>,
) -> Result<Json<Connection<InvitationView>>, ApiError> {
    let env = state.auth.sharing_env()?;
    Ok(Json(
        sharing::list_invitations(&viewer, &state.reader, &env, tree_id, &params(&query)).await?,
    ))
}

/// POST /api/v1/trees/{tree_id}/invitations/{invitation_id}/resend
pub async fn resend_invitation(
    State(state): State<AppState>,
    viewer: Viewer,
    Path((tree_id, invitation_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<InvitationView>, ApiError> {
    let env = state.auth.sharing_env()?;
    Ok(Json(
        sharing::resend_invitation(&viewer, &state.db, &env, tree_id, invitation_id).await?,
    ))
}

/// DELETE /api/v1/trees/{tree_id}/invitations/{invitation_id}
pub async fn revoke_invitation(
    State(state): State<AppState>,
    viewer: Viewer,
    Path((tree_id, invitation_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    state.auth.sharing_env()?;
    sharing::revoke_invitation(&viewer, &state.db, tree_id, invitation_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/me/invitations
pub async fn my_invitations(
    State(state): State<AppState>,
    viewer: Viewer,
) -> Result<Json<Vec<MyInvitation>>, ApiError> {
    state.auth.sharing_env()?;
    Ok(Json(sharing::my_invitations(&viewer, &state.reader).await?))
}

/// POST /api/v1/me/invitations/{invitation_id}/accept
pub async fn accept_invitation(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(invitation_id): Path<Uuid>,
) -> Result<Json<AcceptedInvitation>, ApiError> {
    let env = state.auth.sharing_env()?;
    let member =
        sharing::accept_invitation(&viewer, &state.db, env.mailing(), invitation_id).await?;
    Ok(Json(AcceptedInvitation { member }))
}

/// What accepting an invitation answers: the membership it became.
#[derive(Debug, serde::Serialize)]
pub struct AcceptedInvitation {
    pub member: MemberView,
}

/// POST /api/v1/me/invitations/{invitation_id}/decline
pub async fn decline_invitation(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(invitation_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state.auth.sharing_env()?;
    sharing::decline_invitation(&viewer, &state.db, invitation_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/trees/{tree_id}/share-links
pub async fn list_share_links(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<PaginationQuery>,
) -> Result<Json<Connection<ShareLinkView>>, ApiError> {
    state.auth.sharing_env()?;
    Ok(Json(
        share_link::list(&state.reader, tree_id, &params(&query)).await?,
    ))
}

/// POST /api/v1/trees/{tree_id}/share-links: `201` with the link and its
/// URL, shown once.
pub async fn create_share_link(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<NewShareLinkRequest>,
) -> Result<(StatusCode, Json<CreatedShareLink>), ApiError> {
    let env = state.auth.sharing_env()?;
    let created = share_link::create(&viewer, &state.db, &env, tree_id, body).await?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// PUT /api/v1/trees/{tree_id}/share-links/{link_id}
pub async fn update_share_link(
    State(state): State<AppState>,
    viewer: Viewer,
    Path((tree_id, link_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<ShareLinkPatch>,
) -> Result<Json<ShareLinkView>, ApiError> {
    state.auth.sharing_env()?;
    Ok(Json(
        share_link::update(&viewer, &state.db, tree_id, link_id, body).await?,
    ))
}

/// DELETE /api/v1/trees/{tree_id}/share-links/{link_id}
pub async fn revoke_share_link(
    State(state): State<AppState>,
    viewer: Viewer,
    Path((tree_id, link_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    state.auth.sharing_env()?;
    share_link::revoke(&viewer, &state.db, tree_id, link_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/v1/trees/{tree_id}/access-scopes/preview
pub async fn preview_access_scope(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<ScopePreviewRequest>,
) -> Result<Json<ScopePreview>, ApiError> {
    state.auth.sharing_env()?;
    Ok(Json(
        access_scope::preview(&state.db, &state.profiles, tree_id, &body).await?,
    ))
}

/// GET /api/v1/trees/{tree_id}/visitor-scope: the scope Visitors are
/// limited to, `null` for the whole tree.
pub async fn visitor_scope(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
) -> Result<Json<Option<access_scope::ScopeView>>, ApiError> {
    state.auth.sharing_env()?;
    Ok(Json(
        access_scope::visitor_scope(&state.reader, tree_id).await?,
    ))
}
