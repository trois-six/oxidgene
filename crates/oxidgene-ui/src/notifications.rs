//! Notifications in the interface (docs/messaging.md §2.2, §6): the bell's
//! unread count and latest notifications, kept current by the long poll.
//!
//! Like the rest of the account UI, this exists only where the server signs
//! users in: the follower starts once `/auth/session` names a signed-in user
//! (`crate::account`), so the desktop and the `none` mode never request
//! anything. While the page is shown it keeps one `GET /me/updates` open,
//! which the server answers when something happens or after 25 seconds; a
//! hidden page asks nothing until it is shown again, and an error is
//! followed by a growing pause rather than a retry at once.

use dioxus::prelude::*;
use oxidgene_core::notification::NotificationKind;

use crate::api::notification::Notification;
use crate::api::{ApiClient, ApiError};
use crate::i18n::I18n;
use crate::router::Route;

/// How long the server holds a long poll, in seconds.
const WAIT_SECONDS: u32 = 25;

/// The longest pause after errors, in milliseconds.
const MAX_BACKOFF_MS: u32 = 60_000;

/// What the bell shows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Notifications {
    /// Whether the latest notifications were read once.
    pub loaded: bool,
    pub unread: u64,
    /// The latest notifications, newest first.
    pub latest: Vec<Notification>,
}

/// The bell's state in the Dioxus context, provided by
/// `crate::account::use_init_account`.
#[derive(Clone, Copy)]
pub struct NotificationsState(pub Signal<Notifications>);

/// The bell's state.
pub fn use_notifications() -> Signal<Notifications> {
    use_context::<NotificationsState>().0
}

/// Read the latest notifications into `state`.
pub async fn refresh(api: &ApiClient, mut state: Signal<Notifications>) -> Result<(), ApiError> {
    let page = api.notifications().await?;
    let unread = page.unread_count;
    state.set(Notifications {
        loaded: true,
        unread,
        latest: page.into_notifications(),
    });
    Ok(())
}

/// Keep `state` current for as long as the application runs, signed in.
pub(crate) async fn follow(api: ApiClient, state: Signal<Notifications>) {
    let mut cursor: Option<String> = None;
    let mut backoff = 1_000_u32;
    loop {
        until_shown().await;
        let first = cursor.is_none();
        if first && refresh(&api, state).await.is_err() {
            crate::utils::sleep_ms(backoff).await;
            backoff = (backoff * 2).min(MAX_BACKOFF_MS);
            continue;
        }
        match api.updates(cursor.as_deref(), WAIT_SECONDS).await {
            Ok(updates) => {
                backoff = 1_000;
                let happened = !updates.events.is_empty();
                cursor = Some(updates.cursor);
                if happened {
                    let _ = refresh(&api, state).await;
                } else if !first {
                    // An answer without events comes after the wait, or at
                    // once from a server holding too many: either way, a
                    // breath before asking again.
                    crate::utils::sleep_ms(1_000).await;
                }
            }
            // The session ended: the account UI says so, and nothing more is
            // asked until the page is loaded again.
            Err(error) if error.status() == Some(401) => return,
            Err(_) => {
                crate::utils::sleep_ms(backoff).await;
                backoff = (backoff * 2).min(MAX_BACKOFF_MS);
            }
        }
    }
}

/// Wait until the page is shown: a hidden tab keeps no request open.
async fn until_shown() {
    let _ = document::eval(
        "if (document.hidden) {
            await new Promise(resolve => {
                const shown = () => {
                    if (!document.hidden) {
                        document.removeEventListener('visibilitychange', shown);
                        resolve();
                    }
                };
                document.addEventListener('visibilitychange', shown);
            });
        }
        return true;",
    )
    .await;
}

/// The sentence of `notification` in the language of its reader, `reader`.
pub fn text(i18n: &I18n, notification: &Notification, reader: uuid::Uuid) -> String {
    if notification.tree_unavailable || notification.tree_name.is_none() {
        return i18n.t("notifications.no_longer_shared");
    }
    let tree = notification.tree_name.clone().unwrap_or_default();
    let actor = notification
        .actor_name
        .clone()
        .unwrap_or_else(|| i18n.t("notifications.someone"));
    let args = [("actor", actor.as_str()), ("tree", tree.as_str())];
    match notification.kind {
        NotificationKind::TreeShared => i18n.t_args("notifications.tree_shared", &args),
        NotificationKind::MembershipChanged => {
            i18n.t_args("notifications.membership_changed", &args)
        }
        NotificationKind::InvitationAccepted => {
            i18n.t_args("notifications.invitation_accepted", &args)
        }
        NotificationKind::MemberLeft => i18n.t_args("notifications.member_left", &args),
        NotificationKind::OwnershipTransferred if notification.subject_id == Some(reader) => {
            i18n.t_args("notifications.ownership_transferred_you", &args)
        }
        NotificationKind::OwnershipTransferred => {
            let owner = notification
                .subject_name
                .clone()
                .unwrap_or_else(|| i18n.t("notifications.someone"));
            i18n.t_args(
                "notifications.ownership_transferred",
                &[("owner", owner.as_str()), ("tree", tree.as_str())],
            )
        }
        NotificationKind::JobFinished => {
            i18n.t_args(job_key(notification.outcome.as_deref()), &args)
        }
        NotificationKind::MembershipEnded => i18n.t("notifications.no_longer_shared"),
    }
}

/// The sentence of a finished job, by how it ended.
fn job_key(outcome: Option<&str>) -> &'static str {
    match outcome {
        Some("import_completed") => "notifications.job.import_completed",
        Some("import_failed") => "notifications.job.import_failed",
        Some("export_completed") => "notifications.job.export_completed",
        Some("export_failed") => "notifications.job.export_failed",
        _ => "notifications.job.ended",
    }
}

/// Where opening `notification` leads: its tree while the reader sees it.
pub fn target(notification: &Notification) -> Route {
    match notification
        .tree_id
        .filter(|_| !notification.tree_unavailable)
    {
        Some(tree_id)
            if matches!(
                notification.kind,
                NotificationKind::InvitationAccepted | NotificationKind::MemberLeft
            ) =>
        {
            Route::Settings {
                tree_id: tree_id.to_string(),
            }
        }
        Some(tree_id) => Route::TreeDetail {
            tree_id: tree_id.to_string(),
            person: None,
        },
        None => Route::Home {},
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Language;

    fn notification(kind: NotificationKind) -> Notification {
        Notification {
            id: uuid::Uuid::now_v7(),
            kind,
            tree_id: Some(uuid::Uuid::now_v7()),
            tree_name: Some("Fictive tree".into()),
            tree_unavailable: false,
            actor_name: Some("Ada Fictive".into()),
            subject_id: None,
            subject_name: None,
            outcome: None,
            created_at: chrono::Utc::now(),
            read_at: None,
        }
    }

    #[test]
    fn every_kind_reads_as_a_sentence() {
        let i18n = I18n::new(Language::english());
        for kind in NotificationKind::ALL {
            let text = text(&i18n, &notification(kind), uuid::Uuid::nil());
            assert!(!text.contains("notifications."), "{kind:?}: {text}");
        }
        let reader = uuid::Uuid::now_v7();
        let mut owned = notification(NotificationKind::OwnershipTransferred);
        owned.subject_id = Some(reader);
        assert_eq!(
            text(&i18n, &owned, reader),
            "You are now the owner of Fictive tree"
        );
        let mut gone = notification(NotificationKind::TreeShared);
        gone.tree_unavailable = true;
        gone.tree_name = None;
        assert_eq!(
            text(&i18n, &gone, reader),
            i18n.t("notifications.no_longer_shared")
        );
        assert_eq!(target(&gone), Route::Home {});
    }
}
