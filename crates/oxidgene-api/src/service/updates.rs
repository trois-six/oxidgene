//! Delivery without polling (docs/messaging.md §6): `GET /me/updates`, the
//! long poll a signed-in page keeps open while it is shown.
//!
//! A request names the cursor it last read (`after`) and how long it may
//! wait (`wait`, at most [`MAX_WAIT`]). It answers at once when events newer
//! than the cursor exist; otherwise it holds the request until one arrives
//! or the wait runs out, then answers with what it has and the next cursor.
//! Without a cursor it answers at once with the current one, the start of
//! the page's watch.
//!
//! The waiting reads nothing: a waiter sleeps on a [`watch`] channel of its
//! user, and reads again only when the channel says something changed.
//! Whoever notifies a user wakes it, once their transaction has committed
//! (`service::notification::Notices::publish`); under PostgreSQL, where the
//! worker and other server replicas notify from other processes, a
//! listener of the database's notifications relays theirs ([`listen`]), as
//! for the job status reads (docs/architecture.md §6.5). An idle request
//! costs one read when it starts and none while it waits.
//!
//! Events are typed and carry ids, nothing else: today `notification`; the
//! messaging events of §6 join them with conversations.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Duration;

use oxidgene_core::OxidGeneError;
use oxidgene_core::access::ViewerContext;
use oxidgene_db::repo::{NotificationRepo, PaginationParams, encode_cursor};
use oxidgene_db::sea_orm::ConnectionTrait;
use serde::Serialize;
use tokio::sync::{Semaphore, watch};
use uuid::Uuid;

use crate::service::account::require_user;

/// The longest a request waits.
pub const MAX_WAIT: Duration = Duration::from_secs(30);

/// The wait of a request that names none.
pub const DEFAULT_WAIT: Duration = Duration::from_secs(25);

/// How many requests may wait at once in one process; past them a request
/// answers at once, as one without a cursor does, and the page waits before
/// it asks again.
pub const MAX_WAITERS: usize = 10_000;

/// The most events one answer carries; the next request takes the rest.
const MAX_EVENTS: u64 = 100;

/// One event: what happened, by type and id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpdateEvent {
    /// `notification`.
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub id: Uuid,
}

/// What a long poll answers: the events after the cursor, and the cursor
/// to ask from next.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Updates {
    pub events: Vec<UpdateEvent>,
    pub cursor: String,
}

/// The channel of each user someone waits on.
fn channels() -> MutexGuard<'static, HashMap<Uuid, watch::Sender<()>>> {
    static CHANNELS: OnceLock<Mutex<HashMap<Uuid, watch::Sender<()>>>> = OnceLock::new();
    CHANNELS
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

fn waiters() -> &'static Semaphore {
    static WAITERS: Semaphore = Semaphore::const_new(MAX_WAITERS);
    &WAITERS
}

/// Wake whoever waits on the updates of `user_id`.
pub fn announce(user_id: Uuid) {
    if let Some(channel) = channels().get(&user_id) {
        channel.send_replace(());
    }
}

/// Wake every waiter: what changed is unknown, after a lost notification.
#[cfg(feature = "postgres")]
fn announce_all() {
    for channel in channels().values() {
        channel.send_replace(());
    }
}

/// A waiter's subscription to its user's changes, taken before it reads so
/// that no change falls between the read and the wait.
struct Subscription {
    user_id: Uuid,
    changes: watch::Receiver<()>,
}

impl Subscription {
    fn new(user_id: Uuid) -> Self {
        let changes = channels()
            .entry(user_id)
            .or_insert_with(|| watch::channel(()).0)
            .subscribe();
        Self { user_id, changes }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        let mut channels = channels();
        if channels
            .get(&self.user_id)
            .is_some_and(|channel| channel.receiver_count() <= 1)
        {
            channels.remove(&self.user_id);
        }
    }
}

/// The cursor of notification `id`: opaque to the client.
fn cursor_of(id: Option<Uuid>) -> String {
    encode_cursor(&id.unwrap_or(Uuid::nil()))
}

/// The notification a cursor stands after.
fn after_of(cursor: &str) -> Result<Uuid, OxidGeneError> {
    PaginationParams {
        first: 1,
        after: Some(cursor.to_string()),
    }
    .decode_cursor()?
    .ok_or_else(|| OxidGeneError::Validation("invalid cursor".into()))
}

/// The updates of the signed-in user after `after`, waiting up to `wait`
/// for one.
pub async fn wait(
    viewer: &ViewerContext,
    db: &impl ConnectionTrait,
    after: Option<&str>,
    wait: Duration,
) -> Result<Updates, OxidGeneError> {
    let user_id = require_user(viewer.principal())?.user_id;
    let Some(after) = after.filter(|cursor| !cursor.is_empty()) else {
        let newest = NotificationRepo::newest(db, user_id).await?;
        return Ok(Updates {
            events: Vec::new(),
            cursor: cursor_of(newest),
        });
    };
    let after = after_of(after)?;
    let wait = wait.min(MAX_WAIT);
    let permit = if wait.is_zero() {
        None
    } else {
        waiters().try_acquire().ok()
    };
    let deadline = tokio::time::Instant::now() + wait;
    let mut subscription = permit.as_ref().map(|_| Subscription::new(user_id));
    loop {
        if let Some(subscription) = &mut subscription {
            subscription.changes.borrow_and_update();
        }
        let events = NotificationRepo::after(db, user_id, after, MAX_EVENTS).await?;
        if let Some(last) = events.last() {
            return Ok(Updates {
                cursor: cursor_of(Some(last.id)),
                events: events
                    .iter()
                    .map(|notification| UpdateEvent {
                        kind: "notification",
                        id: notification.id,
                    })
                    .collect(),
            });
        }
        let unchanged = Updates {
            events: Vec::new(),
            cursor: cursor_of(Some(after)),
        };
        let Some(subscription) = &mut subscription else {
            return Ok(unchanged);
        };
        tokio::select! {
            changed = subscription.changes.changed() => {
                if changed.is_err() {
                    return Ok(unchanged);
                }
            }
            () = tokio::time::sleep_until(deadline) => return Ok(unchanged),
        }
    }
}

/// Listen, under PostgreSQL, to the notifications every process announces
/// (see [`oxidgene_db::repo::UPDATES_CHANNEL`]) and wake their users'
/// waiters. Runs until the process ends; a lost connection is opened again,
/// and every waiter woken, since what it missed is unknown. Does nothing on
/// another database.
#[cfg(feature = "postgres")]
pub async fn listen(db: oxidgene_db::sea_orm::DatabaseConnection) {
    if db.get_database_backend() != oxidgene_db::sea_orm::DbBackend::Postgres {
        return;
    }
    let pool = db.get_postgres_connection_pool().clone();
    loop {
        if relay(&pool).await.is_err() {
            tracing::warn!(
                error = "updates_listener",
                "lost the update notifications; listening again"
            );
            announce_all();
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}

#[cfg(feature = "postgres")]
async fn relay(
    pool: &oxidgene_db::sea_orm::sqlx::PgPool,
) -> Result<std::convert::Infallible, oxidgene_db::sea_orm::sqlx::Error> {
    loop {
        let mut listener =
            oxidgene_db::sea_orm::sqlx::postgres::PgListener::connect_with(pool).await?;
        listener.listen(oxidgene_db::repo::UPDATES_CHANNEL).await?;
        announce_all();
        while let Some(notification) = listener.try_recv().await? {
            if let Ok(user_id) = notification.payload().parse() {
                announce(user_id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A notification another process writes — a worker, another replica —
    /// wakes this process's long poll through PostgreSQL's notifications,
    /// with no in-process signal. Needs an empty disposable PostgreSQL
    /// database, which the test leaves empty.
    #[cfg(feature = "postgres")]
    #[tokio::test]
    #[ignore = "needs an empty disposable PostgreSQL database: run by `just test-postgres`"]
    async fn on_postgres_a_notification_written_elsewhere_wakes_the_long_poll() {
        use oxidgene_core::access::{Identity, InstanceRole, Principal};
        use oxidgene_db::repo::{AppUserRepo, NewAppUser, NewNotification};
        use oxidgene_db::sea_orm::{ConnectionTrait, DbBackend, Statement};

        let url = std::env::var("OXIDGENE_TEST_DATABASE_URL")
            .expect("set OXIDGENE_TEST_DATABASE_URL to an empty disposable PostgreSQL database");
        let db = oxidgene_db::repo::connect(&url).await.expect("connects");
        oxidgene_db::repo::run_migrations(&db)
            .await
            .expect("migrates");
        let listener = tokio::spawn(listen(db.clone()));
        let user = AppUserRepo::create(
            &db,
            NewAppUser {
                issuer: "https://idp.example.invalid/".into(),
                subject: format!("subject-{}", Uuid::now_v7()),
                email: None,
                email_verified: false,
                provider_name: Some("Fictive waiter".into()),
                username: None,
                locale: None,
                instance_role: InstanceRole::User,
                can_create_trees: false,
            },
        )
        .await
        .expect("creates the user");
        let viewer = ViewerContext::new(Principal::User(Identity {
            issuer: user.issuer.clone().unwrap_or_default(),
            subject: user.subject.clone().unwrap_or_default(),
            user_id: user.id,
            client_id: None,
            scopes: Vec::new(),
            session_id: None,
            api_client_id: None,
            instance_role: InstanceRole::User,
            can_create_trees: false,
        }));
        // Let the listener open its connection.
        tokio::time::sleep(Duration::from_millis(500)).await;
        let cursor = cursor_of(None);
        let waiting = tokio::spawn({
            let db = db.clone();
            async move { wait(&viewer, &db, Some(&cursor), MAX_WAIT).await }
        });
        tokio::time::sleep(Duration::from_millis(200)).await;
        // As another process writes it: the row and its `pg_notify`, no
        // in-process announcement.
        let started = std::time::Instant::now();
        NotificationRepo::insert(
            &db,
            NewNotification {
                user_id: user.id,
                kind: "job_finished",
                tree_id: None,
                actor_user_id: None,
                subject_id: None,
            },
        )
        .await
        .expect("notifies");
        let updates = tokio::time::timeout(Duration::from_secs(10), waiting)
            .await
            .expect("the notification ends the wait")
            .unwrap()
            .expect("reads");
        assert_eq!(updates.events.len(), 1);
        assert!(started.elapsed() < Duration::from_secs(5));
        listener.abort();
        db.execute_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "DELETE FROM app_user WHERE id = $1",
            [user.id.into()],
        ))
        .await
        .expect("leaves the database empty");
    }

    #[test]
    fn a_cursor_reads_back() {
        let id = Uuid::now_v7();
        assert_eq!(after_of(&cursor_of(Some(id))).unwrap(), id);
        assert_eq!(after_of(&cursor_of(None)).unwrap(), Uuid::nil());
        assert!(after_of("not a cursor").is_err());
    }
}
