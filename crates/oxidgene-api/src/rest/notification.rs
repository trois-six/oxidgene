//! REST handlers of the notifications (docs/messaging.md §2, §6, §11): the
//! signed-in user's notifications, their email preferences, the long poll
//! of their updates, and the one-click unsubscribe of the mail, which needs
//! no session. Web build only (`auth` feature), and absent from the `none`
//! mode, which has no accounts (`404`).

use std::time::Duration;

use axum::Json;
use axum::extract::{Query, State};
use oxidgene_db::repo::PaginationParams;
use serde::Deserialize;

use super::authorize::Viewer;
use super::dto::PaginationQuery;
use super::error::ApiError;
use super::state::AppState;
use crate::service::notification::{
    self, MarkRead, NotificationPage, NotificationPreferences, PreferencesPatch, UnreadCount,
    Unsubscribed,
};
use crate::service::updates::{self, Updates};

/// GET /api/v1/me/notifications?first=&after=
pub async fn list_notifications(
    State(state): State<AppState>,
    viewer: Viewer,
    Query(query): Query<PaginationQuery>,
) -> Result<Json<NotificationPage>, ApiError> {
    let params = PaginationParams {
        first: query.first.unwrap_or(20),
        after: query.after,
    };
    Ok(Json(
        notification::list(&viewer, &state.reader, &params).await?,
    ))
}

/// POST /api/v1/me/notifications/read: `{ ids? }`, every notification
/// without ids.
pub async fn mark_read(
    State(state): State<AppState>,
    viewer: Viewer,
    Json(body): Json<MarkRead>,
) -> Result<Json<UnreadCount>, ApiError> {
    Ok(Json(
        notification::mark_read(&viewer, &state.db, body).await?,
    ))
}

/// GET /api/v1/me/notification-preferences
pub async fn preferences(
    State(state): State<AppState>,
    viewer: Viewer,
) -> Result<Json<NotificationPreferences>, ApiError> {
    Ok(Json(
        notification::preferences(&viewer, &state.reader, state.auth.mailing()).await?,
    ))
}

/// PUT /api/v1/me/notification-preferences
pub async fn update_preferences(
    State(state): State<AppState>,
    viewer: Viewer,
    Json(body): Json<PreferencesPatch>,
) -> Result<Json<NotificationPreferences>, ApiError> {
    Ok(Json(
        notification::update_preferences(&viewer, &state.db, state.auth.mailing(), body).await?,
    ))
}

/// The long poll's query.
#[derive(Debug, Deserialize)]
pub struct UpdatesQuery {
    #[serde(default)]
    pub after: Option<String>,
    /// Seconds, at most 30; 25 when absent.
    #[serde(default)]
    pub wait: Option<u64>,
}

/// GET /api/v1/me/updates?after=&wait=
pub async fn updates(
    State(state): State<AppState>,
    viewer: Viewer,
    Query(query): Query<UpdatesQuery>,
) -> Result<Json<Updates>, ApiError> {
    let wait = query
        .wait
        .map_or(updates::DEFAULT_WAIT, Duration::from_secs);
    Ok(Json(
        updates::wait(&viewer, &state.reader, query.after.as_deref(), wait).await?,
    ))
}

/// The unsubscribe's query: the signed token of the mail's link.
#[derive(Debug, Deserialize)]
pub struct UnsubscribeQuery {
    pub token: String,
}

/// POST /api/v1/mail/unsubscribe?token=
///
/// The one-click unsubscribe of RFC 8058: a mail client posts
/// `List-Unsubscribe=One-Click` to the address the mail names, without a
/// session; the body is not read.
pub async fn unsubscribe(
    State(state): State<AppState>,
    Query(query): Query<UnsubscribeQuery>,
) -> Result<Json<Unsubscribed>, ApiError> {
    let keys = state.auth.unsubscribe_keys()?;
    Ok(Json(
        notification::unsubscribe(&state.db, &keys, &query.token).await?,
    ))
}
