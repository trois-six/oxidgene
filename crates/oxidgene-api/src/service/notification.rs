//! In-app notifications and their email preferences (docs/messaging.md §2),
//! REST and GraphQL both going through here. Web build only.
//!
//! - A notification is a row for one user holding the ids of what it is
//!   about, written by the change that causes it in that change's
//!   transaction ([`Notices`]); its reader's page learns of it through the
//!   long poll (`service::updates`), woken once the transaction commits.
//! - By email, each kind is *off*, *immediate* or in a *daily digest*, as
//!   the recipient chose; the defaults are `tree_shared` and
//!   `invitation_accepted` immediately, the rest off, and one switch turns
//!   all email off. Without an SMTP relay nothing is mailed and the
//!   preferences are hidden (§3.5).
//! - The list resolves the tree and the people a notification names under
//!   its reader's current access: a tree they no longer see is named by
//!   nobody.
//! - Notifications are kept 90 days.

use std::collections::HashMap;

use chrono::{DateTime, Duration, NaiveTime, Utc};
use oxidgene_core::OxidGeneError;
use oxidgene_core::access::ViewerContext;
use oxidgene_core::types::{Connection, Edge};
use oxidgene_db::repo::{
    AppUserRepo, MailOutboxRepo, NewMail, NewNotification, Notification, NotificationRepo,
    PaginationParams, TreeRepo, UserStatus,
};
use oxidgene_db::sea_orm::{ConnectionTrait, DatabaseConnection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::mail::templates;
use crate::service::account::require_user;

/// How long a notification is kept.
pub const RETENTION_DAYS: i64 = 90;

/// The hour of the day (UTC) the daily digests are sent at.
pub const DIGEST_HOUR_UTC: u32 = 6;

/// The kinds live in the core, which the interface wording them shares.
pub use oxidgene_core::notification::NotificationKind;

/// The email preference of `kind` for a user who never changed it (§2.3).
pub const fn default_email(kind: NotificationKind) -> EmailPreference {
    match kind {
        NotificationKind::TreeShared | NotificationKind::InvitationAccepted => {
            EmailPreference::Immediate
        }
        _ => EmailPreference::Off,
    }
}

/// How a user wants one kind of notification by email.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmailPreference {
    Off,
    Immediate,
    Daily,
}

impl EmailPreference {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Immediate => "immediate",
            Self::Daily => "daily",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [Self::Off, Self::Immediate, Self::Daily]
            .into_iter()
            .find(|preference| preference.as_str() == value)
    }
}

/// The preference row of the switch turning all email off.
const ALL_EMAIL: &str = "all";

/// The kind of the daily digest's outbox row, and of its unsubscribe.
const DIGEST: &str = "digest";

/// Whether the deployment mails: an SMTP relay is configured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mailing(pub bool);

// ── Creating notifications ──────────────────────────────────────────────

/// The notifications a change writes in its transaction, and whom to wake
/// once it commits.
#[derive(Debug)]
#[must_use = "publish the notices once their transaction commits"]
pub struct Notices {
    mailing: Mailing,
    users: Vec<Uuid>,
    mailed: bool,
}

impl Notices {
    pub fn new(mailing: Mailing) -> Self {
        Self {
            mailing,
            users: Vec::new(),
            mailed: false,
        }
    }

    /// The notices of `notes` — each a kind, its reader and what it names —
    /// written in `txn`.
    pub async fn write(
        txn: &impl ConnectionTrait,
        mailing: Mailing,
        notes: impl IntoIterator<Item = (NotificationKind, Uuid, About)>,
    ) -> Result<Self, OxidGeneError> {
        let mut notices = Self::new(mailing);
        for (kind, user_id, about) in notes {
            notices.add(txn, kind, user_id, about).await?;
        }
        Ok(notices)
    }

    /// Notify `user_id` of `kind`, in `txn`, and queue its mail as they
    /// want it.
    pub async fn add(
        &mut self,
        txn: &impl ConnectionTrait,
        kind: NotificationKind,
        user_id: Uuid,
        about: About,
    ) -> Result<(), OxidGeneError> {
        let notification = NotificationRepo::insert(
            txn,
            NewNotification {
                user_id,
                kind: kind.as_str(),
                tree_id: about.tree_id,
                actor_user_id: about.actor_user_id,
                subject_id: about.subject_id,
            },
        )
        .await?;
        self.users.push(user_id);
        if self.mailing.0 {
            self.mailed |= queue_mail(txn, kind, &notification).await?;
        }
        Ok(())
    }

    /// Wake the readers' long polls and the mail sender: the transaction
    /// committed.
    pub fn publish(self) {
        for user_id in self.users {
            crate::service::updates::announce(user_id);
        }
        if self.mailed {
            crate::mail::wake();
        }
    }
}

/// What a notification names.
#[derive(Debug, Clone, Copy, Default)]
pub struct About {
    pub tree_id: Option<Uuid>,
    pub actor_user_id: Option<Uuid>,
    pub subject_id: Option<Uuid>,
}

impl About {
    /// Tree `tree_id`, changed by `viewer`.
    pub fn tree(tree_id: Uuid, viewer: &ViewerContext) -> Self {
        Self {
            tree_id: Some(tree_id),
            actor_user_id: viewer.user().map(|identity| identity.user_id),
            subject_id: None,
        }
    }

    #[must_use]
    pub fn subject(mut self, subject_id: Uuid) -> Self {
        self.subject_id = Some(subject_id);
        self
    }
}

/// The address and language `user_id` is mailed at: an active account
/// with a verified address.
async fn mail_recipient(
    db: &impl ConnectionTrait,
    user_id: Uuid,
) -> Result<Option<(String, String)>, OxidGeneError> {
    let user = AppUserRepo::get(db, user_id).await?;
    if user.status != UserStatus::Active || !user.email_verified {
        return Ok(None);
    }
    Ok(user
        .email
        .map(|email| (email, templates::language_of(user.locale.as_deref()))))
}

/// Queue the mail of `notification` as its reader wants it: one now, or
/// the daily digest it joins. Whether a row was written.
async fn queue_mail(
    db: &impl ConnectionTrait,
    kind: NotificationKind,
    notification: &Notification,
) -> Result<bool, OxidGeneError> {
    let user_id = notification.user_id;
    let preference = email_preference(db, user_id, kind).await?;
    if preference == EmailPreference::Off {
        return Ok(false);
    }
    let Some((address, language)) = mail_recipient(db, user_id).await? else {
        return Ok(false);
    };
    let (mail_kind, params, due) = match preference {
        EmailPreference::Immediate => (
            kind.as_str(),
            serde_json::json!({ "notification_id": notification.id }),
            Utc::now(),
        ),
        _ => {
            if MailOutboxRepo::pending_of_user(db, user_id, DIGEST)
                .await?
                .is_some()
            {
                // Today's digest gathers it.
                return Ok(false);
            }
            (
                DIGEST,
                serde_json::json!({ "since": notification.created_at.to_rfc3339() }),
                next_digest(notification.created_at),
            )
        }
    };
    MailOutboxRepo::enqueue(
        db,
        NewMail {
            user_id: Some(user_id),
            to_address: address,
            kind: mail_kind,
            language,
            params,
            due,
        },
    )
    .await?;
    Ok(true)
}

/// When the digest opened at `at` is sent: the next [`DIGEST_HOUR_UTC`].
pub fn next_digest(at: DateTime<Utc>) -> DateTime<Utc> {
    let hour = NaiveTime::from_hms_opt(DIGEST_HOUR_UTC, 0, 0).unwrap_or_default();
    let today = at.date_naive().and_time(hour).and_utc();
    if today > at {
        today
    } else {
        today + Duration::days(1)
    }
}

// ── Preferences ─────────────────────────────────────────────────────────

/// The email preferences `user_id` changed, as a map, the switch included.
async fn changed(
    db: &impl ConnectionTrait,
    user_id: Uuid,
) -> Result<HashMap<String, String>, OxidGeneError> {
    Ok(NotificationRepo::preferences(db, user_id)
        .await?
        .into_iter()
        .collect())
}

/// How `user_id` wants `kind` by email now: off for everything when they
/// turned all email off.
pub async fn email_preference(
    db: &impl ConnectionTrait,
    user_id: Uuid,
    kind: NotificationKind,
) -> Result<EmailPreference, OxidGeneError> {
    let changed = changed(db, user_id).await?;
    Ok(effective(&changed, kind))
}

fn effective(changed: &HashMap<String, String>, kind: NotificationKind) -> EmailPreference {
    if changed.get(ALL_EMAIL).map(String::as_str) == Some(EmailPreference::Off.as_str()) {
        return EmailPreference::Off;
    }
    changed
        .get(kind.as_str())
        .and_then(|value| EmailPreference::parse(value))
        .unwrap_or_else(|| default_email(kind))
}

/// One kind's preference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KindPreference {
    pub kind: NotificationKind,
    pub email: EmailPreference,
}

/// A user's notification preferences: in the application always; by
/// email as each kind says, unless all email is off.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NotificationPreferences {
    /// Whether the deployment mails at all: without, the email preferences
    /// are hidden.
    pub email_available: bool,
    /// The switch turning all email off.
    pub email_enabled: bool,
    pub kinds: Vec<KindPreference>,
}

/// A change to the preferences: the switch, and the kinds named.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PreferencesPatch {
    #[serde(default)]
    pub email_enabled: Option<bool>,
    #[serde(default)]
    pub kinds: Vec<KindPreference>,
}

async fn preferences_of(
    db: &impl ConnectionTrait,
    mailing: Mailing,
    user_id: Uuid,
) -> Result<NotificationPreferences, OxidGeneError> {
    let changed = changed(db, user_id).await?;
    let email_enabled = changed.get(ALL_EMAIL).map(String::as_str) != Some("off");
    Ok(NotificationPreferences {
        email_available: mailing.0,
        email_enabled,
        kinds: NotificationKind::ALL
            .into_iter()
            .map(|kind| KindPreference {
                kind,
                email: changed
                    .get(kind.as_str())
                    .and_then(|value| EmailPreference::parse(value))
                    .unwrap_or_else(|| default_email(kind)),
            })
            .collect(),
    })
}

/// The signed-in user's preferences.
pub async fn preferences(
    viewer: &ViewerContext,
    db: &DatabaseConnection,
    mailing: Mailing,
) -> Result<NotificationPreferences, OxidGeneError> {
    let identity = require_user(viewer.principal())?;
    preferences_of(db, mailing, identity.user_id).await
}

/// Change the signed-in user's preferences as `patch` says.
pub async fn update_preferences(
    viewer: &ViewerContext,
    db: &DatabaseConnection,
    mailing: Mailing,
    patch: PreferencesPatch,
) -> Result<NotificationPreferences, OxidGeneError> {
    let identity = require_user(viewer.principal())?;
    let txn = crate::service::scope::begin_tx(db).await?;
    if let Some(enabled) = patch.email_enabled {
        let value = if enabled { "on" } else { "off" };
        NotificationRepo::set_preference(&txn, identity.user_id, ALL_EMAIL, value).await?;
    }
    for preference in &patch.kinds {
        NotificationRepo::set_preference(
            &txn,
            identity.user_id,
            preference.kind.as_str(),
            preference.email.as_str(),
        )
        .await?;
    }
    crate::service::scope::commit_tx(txn).await?;
    preferences_of(db, mailing, identity.user_id).await
}

/// What a one-click unsubscribe turned off: a kind, or the digest's kinds.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Unsubscribed {
    /// The kind, or `digest`.
    pub kind: String,
}

/// Turn off by email what `token` names (§2.3), without a session: one
/// kind, or every kind its user wanted daily for a digest's token. An
/// invalid token, or one of an erased user, is `validation`.
pub async fn unsubscribe(
    db: &DatabaseConnection,
    keys: &crate::mail::UnsubscribeKeys,
    token: &str,
) -> Result<Unsubscribed, OxidGeneError> {
    let invalid = || OxidGeneError::Validation("invalid unsubscribe token".into());
    let unsubscription = keys.open(token).ok_or_else(invalid)?;
    let user = AppUserRepo::get(db, unsubscription.user_id)
        .await
        .map_err(|_| invalid())?;
    if user.status == UserStatus::Erased {
        return Err(invalid());
    }
    let kinds: Vec<NotificationKind> = if unsubscription.kind == DIGEST {
        let changed = changed(db, user.id).await?;
        NotificationKind::ALL
            .into_iter()
            .filter(|kind| effective(&changed, *kind) == EmailPreference::Daily)
            .collect()
    } else {
        vec![NotificationKind::parse(&unsubscription.kind).ok_or_else(invalid)?]
    };
    let txn = crate::service::scope::begin_tx(db).await?;
    for kind in kinds {
        NotificationRepo::set_preference(&txn, user.id, kind.as_str(), "off").await?;
    }
    crate::service::scope::commit_tx(txn).await?;
    tracing::info!(
        category = "mail_unsubscribed",
        mail.kind = %unsubscription.kind,
        "A recipient unsubscribed"
    );
    Ok(Unsubscribed {
        kind: unsubscription.kind,
    })
}

// ── Reading ─────────────────────────────────────────────────────────────

/// A notification as its reader sees it, what it names resolved under
/// their access now.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NotificationView {
    pub id: Uuid,
    pub kind: NotificationKind,
    /// The tree it is about, while the reader sees it.
    pub tree_id: Option<Uuid>,
    pub tree_name: Option<String>,
    /// Whether it is about a tree the reader no longer sees.
    pub tree_unavailable: bool,
    pub actor_id: Option<Uuid>,
    pub actor_name: Option<String>,
    pub subject_id: Option<Uuid>,
    /// The user it is about, for `ownership_transferred`.
    pub subject_name: Option<String>,
    /// How the job ended, for `job_finished`: `import_completed`,
    /// `import_failed`, `export_completed`, `export_failed` or `ended`.
    pub outcome: Option<String>,
    pub count: i32,
    pub created_at: DateTime<Utc>,
    pub read_at: Option<DateTime<Utc>>,
}

/// A page of notifications, newest first, and how many are unread.
#[derive(Debug, Clone, Serialize)]
pub struct NotificationPage {
    #[serde(flatten)]
    pub page: Connection<NotificationView>,
    pub unread_count: u64,
}

/// The signed-in user's notifications, newest first.
pub async fn list(
    viewer: &ViewerContext,
    db: &DatabaseConnection,
    params: &PaginationParams,
) -> Result<NotificationPage, OxidGeneError> {
    let identity = require_user(viewer.principal())?;
    let page = NotificationRepo::list(db, identity.user_id, params).await?;
    let unread_count = NotificationRepo::unread_count(db, identity.user_id).await?;
    let mut people: Vec<Uuid> = page
        .edges
        .iter()
        .flat_map(|edge| [edge.node.actor_user_id, edge.node.subject_id])
        .flatten()
        .collect();
    people.sort_unstable();
    people.dedup();
    let names = AppUserRepo::shown_names(db, &people).await?;
    let mut trees: HashMap<Uuid, Option<String>> = HashMap::new();
    let mut edges = Vec::with_capacity(page.edges.len());
    for edge in page.edges {
        let notification = edge.node;
        let tree_name = match notification.tree_id {
            Some(tree_id) => match trees.get(&tree_id) {
                Some(name) => name.clone(),
                None => {
                    let name = seen_tree(viewer, db, tree_id).await;
                    trees.insert(tree_id, name.clone());
                    name
                }
            },
            None => None,
        };
        edges.push(Edge {
            cursor: edge.cursor,
            node: view(db, notification, tree_name, &names).await?,
        });
    }
    Ok(NotificationPage {
        page: Connection {
            edges,
            page_info: page.page_info,
            total_count: page.total_count,
        },
        unread_count,
    })
}

/// The name of tree `tree_id`, if `viewer` may read it now.
async fn seen_tree(
    viewer: &ViewerContext,
    db: &DatabaseConnection,
    tree_id: Uuid,
) -> Option<String> {
    crate::authz::authorize(db, viewer, tree_id, oxidgene_core::access::Permission::Read)
        .await
        .ok()?;
    TreeRepo::get(db, tree_id).await.ok().map(|tree| tree.name)
}

async fn view(
    db: &DatabaseConnection,
    notification: Notification,
    tree_name: Option<String>,
    names: &HashMap<Uuid, Option<String>>,
) -> Result<NotificationView, OxidGeneError> {
    let kind = NotificationKind::parse(&notification.kind)
        .ok_or_else(|| OxidGeneError::Internal("a notification of an unknown kind".into()))?;
    let ended = kind == NotificationKind::MembershipEnded;
    let visible = tree_name.is_some() && !ended;
    let name = |id: Option<Uuid>| id.and_then(|id| names.get(&id).cloned().flatten());
    let outcome = if kind == NotificationKind::JobFinished {
        Some(
            templates::job_outcome(db, notification.subject_id)
                .await?
                .to_string(),
        )
    } else {
        None
    };
    Ok(NotificationView {
        id: notification.id,
        kind,
        tree_id: notification.tree_id.filter(|_| visible),
        tree_name: tree_name.filter(|_| visible),
        tree_unavailable: notification.tree_id.is_some() && !visible,
        actor_id: notification.actor_user_id,
        actor_name: name(notification.actor_user_id),
        subject_id: notification.subject_id,
        subject_name: (kind == NotificationKind::OwnershipTransferred)
            .then(|| name(notification.subject_id))
            .flatten(),
        outcome,
        count: notification.count,
        created_at: notification.created_at,
        read_at: notification.read_at,
    })
}

/// Which notifications to mark read: those named, or all.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct MarkRead {
    #[serde(default)]
    pub ids: Option<Vec<Uuid>>,
}

/// How many notifications are left unread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct UnreadCount {
    pub unread_count: u64,
}

/// Mark the signed-in user's notifications `request` names read — all of
/// them without ids; how many are left unread.
pub async fn mark_read(
    viewer: &ViewerContext,
    db: &DatabaseConnection,
    request: MarkRead,
) -> Result<UnreadCount, OxidGeneError> {
    let identity = require_user(viewer.principal())?;
    NotificationRepo::mark_read(db, identity.user_id, request.ids.as_deref(), Utc::now()).await?;
    Ok(UnreadCount {
        unread_count: NotificationRepo::unread_count(db, identity.user_id).await?,
    })
}

/// Delete the notifications older than [`RETENTION_DAYS`] and the mail
/// that ended more than seven days ago, with its address: the worker's
/// maintenance pass.
pub async fn prune(db: &DatabaseConnection, now: DateTime<Utc>) -> Result<(), OxidGeneError> {
    NotificationRepo::delete_before(db, now - Duration::days(RETENTION_DAYS)).await?;
    MailOutboxRepo::delete_ended_before(db, now - Duration::days(7)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_reads_back_and_has_its_default() {
        for kind in NotificationKind::ALL {
            assert_eq!(NotificationKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(
            default_email(NotificationKind::TreeShared),
            EmailPreference::Immediate
        );
        assert_eq!(
            default_email(NotificationKind::InvitationAccepted),
            EmailPreference::Immediate
        );
        assert_eq!(
            default_email(NotificationKind::JobFinished),
            EmailPreference::Off
        );
    }

    #[test]
    fn the_switch_turns_every_kind_off() {
        let mut changed = HashMap::new();
        changed.insert("job_finished".to_string(), "daily".to_string());
        assert_eq!(
            effective(&changed, NotificationKind::JobFinished),
            EmailPreference::Daily
        );
        assert_eq!(
            effective(&changed, NotificationKind::TreeShared),
            EmailPreference::Immediate
        );
        changed.insert(ALL_EMAIL.to_string(), "off".to_string());
        for kind in NotificationKind::ALL {
            assert_eq!(effective(&changed, kind), EmailPreference::Off);
        }
    }

    #[test]
    fn a_digest_leaves_at_the_next_six_o_clock_utc() {
        let at = |text: &str| {
            DateTime::parse_from_rfc3339(text)
                .unwrap()
                .with_timezone(&Utc)
        };
        assert_eq!(
            next_digest(at("2026-11-01T05:59:00Z")),
            at("2026-11-01T06:00:00Z")
        );
        assert_eq!(
            next_digest(at("2026-11-01T06:00:00Z")),
            at("2026-11-02T06:00:00Z")
        );
        assert_eq!(
            next_digest(at("2026-11-01T22:10:00Z")),
            at("2026-11-02T06:00:00Z")
        );
    }
}
