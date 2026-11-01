//! GraphQL twins of the notifications (docs/messaging.md §2, §6, §11): the
//! signed-in user's notifications and email preferences, the long poll of
//! their updates, and the one-click unsubscribe of the mail, calling the
//! same services as REST. Web build only, and absent from the `none` mode.

use std::time::Duration;

use async_graphql::{Context, Enum, ID, InputObject, Object, Result, SimpleObject};
use chrono::{DateTime, Utc};
use oxidgene_db::repo::PaginationParams;

use super::account::auth_from_ctx;
use super::scope::{boxed, uuids, viewer_from_ctx};
use super::types::{GqlPageInfo, db_from_ctx, reader_from_ctx};
use crate::service::notification::{
    self, EmailPreference, KindPreference, MarkRead, NotificationKind, NotificationPage,
    NotificationPreferences, NotificationView, PreferencesPatch,
};
use crate::service::updates::{self, UpdateEvent, Updates};

fn id(uuid: uuid::Uuid) -> ID {
    ID(uuid.to_string())
}

/// What a notification is about: REST's `kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum GqlNotificationKind {
    TreeShared,
    MembershipChanged,
    MembershipEnded,
    InvitationAccepted,
    OwnershipTransferred,
    MemberLeft,
    JobFinished,
}

impl From<NotificationKind> for GqlNotificationKind {
    fn from(kind: NotificationKind) -> Self {
        match kind {
            NotificationKind::TreeShared => Self::TreeShared,
            NotificationKind::MembershipChanged => Self::MembershipChanged,
            NotificationKind::MembershipEnded => Self::MembershipEnded,
            NotificationKind::InvitationAccepted => Self::InvitationAccepted,
            NotificationKind::OwnershipTransferred => Self::OwnershipTransferred,
            NotificationKind::MemberLeft => Self::MemberLeft,
            NotificationKind::JobFinished => Self::JobFinished,
        }
    }
}

impl From<GqlNotificationKind> for NotificationKind {
    fn from(kind: GqlNotificationKind) -> Self {
        match kind {
            GqlNotificationKind::TreeShared => Self::TreeShared,
            GqlNotificationKind::MembershipChanged => Self::MembershipChanged,
            GqlNotificationKind::MembershipEnded => Self::MembershipEnded,
            GqlNotificationKind::InvitationAccepted => Self::InvitationAccepted,
            GqlNotificationKind::OwnershipTransferred => Self::OwnershipTransferred,
            GqlNotificationKind::MemberLeft => Self::MemberLeft,
            GqlNotificationKind::JobFinished => Self::JobFinished,
        }
    }
}

/// How a kind is mailed: REST's `email`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum GqlEmailPreference {
    Off,
    Immediate,
    Daily,
}

impl From<EmailPreference> for GqlEmailPreference {
    fn from(preference: EmailPreference) -> Self {
        match preference {
            EmailPreference::Off => Self::Off,
            EmailPreference::Immediate => Self::Immediate,
            EmailPreference::Daily => Self::Daily,
        }
    }
}

impl From<GqlEmailPreference> for EmailPreference {
    fn from(preference: GqlEmailPreference) -> Self {
        match preference {
            GqlEmailPreference::Off => Self::Off,
            GqlEmailPreference::Immediate => Self::Immediate,
            GqlEmailPreference::Daily => Self::Daily,
        }
    }
}

/// A notification: REST `GET /me/notifications`.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlNotification {
    pub id: ID,
    pub kind: GqlNotificationKind,
    pub tree_id: Option<ID>,
    pub tree_name: Option<String>,
    pub tree_unavailable: bool,
    pub actor_id: Option<ID>,
    pub actor_name: Option<String>,
    pub subject_id: Option<ID>,
    pub subject_name: Option<String>,
    pub outcome: Option<String>,
    pub count: i32,
    pub created_at: DateTime<Utc>,
    pub read_at: Option<DateTime<Utc>>,
}

impl From<NotificationView> for GqlNotification {
    fn from(view: NotificationView) -> Self {
        Self {
            id: id(view.id),
            kind: view.kind.into(),
            tree_id: view.tree_id.map(id),
            tree_name: view.tree_name,
            tree_unavailable: view.tree_unavailable,
            actor_id: view.actor_id.map(id),
            actor_name: view.actor_name,
            subject_id: view.subject_id.map(id),
            subject_name: view.subject_name,
            outcome: view.outcome,
            count: view.count,
            created_at: view.created_at,
            read_at: view.read_at,
        }
    }
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlNotificationEdge {
    pub cursor: String,
    pub node: GqlNotification,
}

/// A page of notifications, newest first, and how many are unread.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlNotificationConnection {
    pub edges: Vec<GqlNotificationEdge>,
    pub page_info: GqlPageInfo,
    pub total_count: i64,
    pub unread_count: i64,
}

impl From<NotificationPage> for GqlNotificationConnection {
    fn from(page: NotificationPage) -> Self {
        Self {
            edges: page
                .page
                .edges
                .into_iter()
                .map(|edge| GqlNotificationEdge {
                    cursor: edge.cursor,
                    node: edge.node.into(),
                })
                .collect(),
            page_info: GqlPageInfo {
                has_next_page: page.page.page_info.has_next_page,
                end_cursor: page.page.page_info.end_cursor,
            },
            total_count: page.page.total_count,
            unread_count: i64::try_from(page.unread_count).unwrap_or(i64::MAX),
        }
    }
}

/// One kind's email preference.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlKindPreference {
    pub kind: GqlNotificationKind,
    pub email: GqlEmailPreference,
}

/// The preferences: REST `GET /me/notification-preferences`.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlNotificationPreferences {
    pub email_available: bool,
    pub email_enabled: bool,
    pub kinds: Vec<GqlKindPreference>,
}

impl From<NotificationPreferences> for GqlNotificationPreferences {
    fn from(preferences: NotificationPreferences) -> Self {
        Self {
            email_available: preferences.email_available,
            email_enabled: preferences.email_enabled,
            kinds: preferences
                .kinds
                .into_iter()
                .map(|preference| GqlKindPreference {
                    kind: preference.kind.into(),
                    email: preference.email.into(),
                })
                .collect(),
        }
    }
}

/// One kind's preference to set.
#[derive(Debug, InputObject)]
pub struct KindPreferenceInput {
    pub kind: GqlNotificationKind,
    pub email: GqlEmailPreference,
}

/// A change to the preferences: REST `PUT /me/notification-preferences`.
#[derive(Debug, InputObject)]
pub struct NotificationPreferencesInput {
    pub email_enabled: Option<bool>,
    #[graphql(default)]
    pub kinds: Vec<KindPreferenceInput>,
}

/// One event of the long poll: its type and id.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlUpdateEvent {
    /// `notification`.
    #[graphql(name = "type")]
    pub kind: String,
    pub id: ID,
}

impl From<UpdateEvent> for GqlUpdateEvent {
    fn from(event: UpdateEvent) -> Self {
        Self {
            kind: event.kind.to_string(),
            id: id(event.id),
        }
    }
}

/// What the long poll answers: REST `GET /me/updates`.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlUpdates {
    pub events: Vec<GqlUpdateEvent>,
    pub cursor: String,
}

impl From<Updates> for GqlUpdates {
    fn from(updates: Updates) -> Self {
        Self {
            events: updates.events.into_iter().map(Into::into).collect(),
            cursor: updates.cursor,
        }
    }
}

/// The notification queries.
#[derive(Default)]
pub struct NotificationQuery;

#[Object]
impl NotificationQuery {
    /// The signed-in user's notifications, newest first.
    async fn notifications(
        &self,
        ctx: &Context<'_>,
        first: Option<i32>,
        after: Option<String>,
    ) -> Result<GqlNotificationConnection> {
        boxed(async move {
            let params = PaginationParams {
                first: first.map_or(20, |first| u64::try_from(first).unwrap_or(0)),
                after,
            };
            let page =
                notification::list(viewer_from_ctx(ctx), reader_from_ctx(ctx), &params).await?;
            Ok(page.into())
        })
        .await
    }

    /// The signed-in user's notification preferences.
    async fn notification_preferences(
        &self,
        ctx: &Context<'_>,
    ) -> Result<GqlNotificationPreferences> {
        boxed(async move {
            let preferences = notification::preferences(
                viewer_from_ctx(ctx),
                reader_from_ctx(ctx),
                auth_from_ctx(ctx).mailing(),
            )
            .await?;
            Ok(preferences.into())
        })
        .await
    }

    /// The long poll: the events after `after`, waiting up to `wait`
    /// seconds (at most 30, 25 by default) for one.
    async fn updates(
        &self,
        ctx: &Context<'_>,
        after: Option<String>,
        wait: Option<i32>,
    ) -> Result<GqlUpdates> {
        boxed(async move {
            let wait = wait.map_or(updates::DEFAULT_WAIT, |seconds| {
                Duration::from_secs(u64::try_from(seconds).unwrap_or(0))
            });
            let updates = updates::wait(
                viewer_from_ctx(ctx),
                reader_from_ctx(ctx),
                after.as_deref(),
                wait,
            )
            .await?;
            Ok(updates.into())
        })
        .await
    }
}

/// The notification mutations.
#[derive(Default)]
pub struct NotificationMutation;

#[Object]
impl NotificationMutation {
    /// Mark the notifications `ids` read, every one without ids; how many
    /// are left unread.
    async fn mark_notifications_read(
        &self,
        ctx: &Context<'_>,
        ids: Option<Vec<ID>>,
    ) -> Result<i64> {
        boxed(async move {
            let ids = ids.map(|ids| uuids(&ids)).transpose()?;
            let left =
                notification::mark_read(viewer_from_ctx(ctx), db_from_ctx(ctx), MarkRead { ids })
                    .await?;
            Ok(i64::try_from(left.unread_count).unwrap_or(i64::MAX))
        })
        .await
    }

    /// Change the signed-in user's notification preferences.
    async fn update_notification_preferences(
        &self,
        ctx: &Context<'_>,
        input: NotificationPreferencesInput,
    ) -> Result<GqlNotificationPreferences> {
        boxed(async move {
            let patch = PreferencesPatch {
                email_enabled: input.email_enabled,
                kinds: input
                    .kinds
                    .into_iter()
                    .map(|preference| KindPreference {
                        kind: preference.kind.into(),
                        email: preference.email.into(),
                    })
                    .collect(),
            };
            let preferences = notification::update_preferences(
                viewer_from_ctx(ctx),
                db_from_ctx(ctx),
                auth_from_ctx(ctx).mailing(),
                patch,
            )
            .await?;
            Ok(preferences.into())
        })
        .await
    }

    /// A mail's one-click unsubscribe, without a session: what the signed
    /// `token` names is no longer mailed. The kind, or `digest`.
    async fn unsubscribe(&self, ctx: &Context<'_>, token: String) -> Result<String> {
        boxed(async move {
            let keys = auth_from_ctx(ctx).unsubscribe_keys()?;
            let done = notification::unsubscribe(db_from_ctx(ctx), &keys, &token).await?;
            Ok(done.kind)
        })
        .await
    }
}
