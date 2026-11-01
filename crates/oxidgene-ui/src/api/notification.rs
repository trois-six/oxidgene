//! Client calls of the notifications (docs/messaging.md §2, §6): the
//! signed-in user's notifications and preferences, the long poll of their
//! updates, and a mail's unsubscribe. Web build in `oidc` mode only; nothing
//! here is cached.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{ApiClient, ApiError};

/// A notification: `GET /api/v1/me/notifications`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Notification {
    pub id: Uuid,
    /// `tree_shared`, `membership_changed`, `membership_ended`,
    /// `invitation_accepted`, `ownership_transferred`, `member_left`,
    /// `job_finished`.
    pub kind: String,
    #[serde(default)]
    pub tree_id: Option<Uuid>,
    #[serde(default)]
    pub tree_name: Option<String>,
    /// About a tree the reader no longer sees.
    #[serde(default)]
    pub tree_unavailable: bool,
    #[serde(default)]
    pub actor_name: Option<String>,
    #[serde(default)]
    pub subject_id: Option<Uuid>,
    #[serde(default)]
    pub subject_name: Option<String>,
    /// How a job ended, for `job_finished`.
    #[serde(default)]
    pub outcome: Option<String>,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub read_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct NotificationEdge {
    node: Notification,
}

/// The latest notifications and how many are unread.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct NotificationPage {
    edges: Vec<NotificationEdge>,
    pub unread_count: u64,
}

impl NotificationPage {
    pub fn into_notifications(self) -> Vec<Notification> {
        self.edges.into_iter().map(|edge| edge.node).collect()
    }
}

/// What the long poll answers: whether anything happened, and where to ask
/// from next.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Updates {
    pub events: Vec<serde_json::Value>,
    pub cursor: String,
}

/// One kind's email preference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KindPreference {
    pub kind: String,
    /// `off`, `immediate` or `daily`.
    pub email: String,
}

/// The notification preferences: `GET /api/v1/me/notification-preferences`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct NotificationPreferences {
    /// Whether this server mails at all: without, the email preferences
    /// are hidden.
    pub email_available: bool,
    pub email_enabled: bool,
    pub kinds: Vec<KindPreference>,
}

/// A change to the preferences.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PreferencesPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub kinds: Vec<KindPreference>,
}

#[derive(Serialize)]
struct MarkRead<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    ids: Option<&'a [Uuid]>,
}

/// How many notifications are left unread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct UnreadCount {
    pub unread_count: u64,
}

/// What an unsubscribe turned off: a kind, or `digest`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Unsubscribed {
    pub kind: String,
}

/// How many notifications the bell's panel lists.
pub const PANEL_SIZE: u64 = 10;

/// `value` encoded for a query string.
fn query_value(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

impl ApiClient {
    /// The latest notifications, newest first, and the unread count.
    pub async fn notifications(&self) -> Result<NotificationPage, ApiError> {
        self.get_fresh(&format!("/api/v1/me/notifications?first={PANEL_SIZE}"))
            .await
    }

    /// Mark `ids` read, every notification for `None`.
    pub async fn mark_notifications_read(
        &self,
        ids: Option<&[Uuid]>,
    ) -> Result<UnreadCount, ApiError> {
        self.post("/api/v1/me/notifications/read", &MarkRead { ids })
            .await
    }

    /// The long poll: what happened after `cursor`, waiting up to `wait`
    /// seconds for it; without a cursor, the current one at once.
    pub async fn updates(&self, cursor: Option<&str>, wait: u32) -> Result<Updates, ApiError> {
        let path = match cursor {
            Some(cursor) => format!(
                "/api/v1/me/updates?after={}&wait={wait}",
                query_value(cursor)
            ),
            None => "/api/v1/me/updates".to_string(),
        };
        self.get_fresh(&path).await
    }

    pub async fn notification_preferences(&self) -> Result<NotificationPreferences, ApiError> {
        self.get_fresh("/api/v1/me/notification-preferences").await
    }

    pub async fn update_notification_preferences(
        &self,
        patch: &PreferencesPatch,
    ) -> Result<NotificationPreferences, ApiError> {
        self.put("/api/v1/me/notification-preferences", patch).await
    }

    /// A mail's unsubscribe, with the token of its link.
    pub async fn unsubscribe(&self, token: &str) -> Result<Unsubscribed, ApiError> {
        self.post(
            &format!("/api/v1/mail/unsubscribe?token={}", query_value(token)),
            &serde_json::json!({}),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cursor_travels_in_the_query_intact() {
        assert_eq!(query_value("AZaz09-_.~"), "AZaz09-_.~");
        assert_eq!(query_value("a+b/c="), "a%2Bb%2Fc%3D");
    }
}
