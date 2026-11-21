//! The PostgreSQL notifications of this process: one connection, of its
//! own, listening to every channel the application notifies on.
//!
//! Four things are woken by a notification another process sends: the job
//! status reads waiting on a job ([`super::job_watch`]), the idle job worker
//! ([`super::job_queue`]), the long polls waiting on a user's updates
//! (`service::updates`) and the idle mail sender (`mail::sender`). Each
//! used to listen on a connection it took from the pool and kept for good,
//! two per process out of a pool sized for requests. One listener serves
//! them all on a connection opened beside the pool, so that every pooled
//! connection is a request's.
//!
//! It listens to all the channels whatever this process runs: a
//! notification for a part that is not running here wakes nobody, which
//! costs less than tracking who subscribed. A lost connection is opened
//! again, and every part woken each time listening starts, since what was
//! notified meanwhile is unknown.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use sea_orm::sqlx::postgres::{PgConnectOptions, PgListener, PgPoolOptions};
use sea_orm::{DatabaseConnection, DbBackend};

/// What a channel's notification wakes: called with the notification's
/// payload, or with `None` each time listening starts.
type Wake = fn(Option<&str>);

/// The channels listened to, each with what it wakes.
fn channels() -> Vec<(&'static str, Wake)> {
    let mut channels = vec![
        (
            oxidgene_db::repo::JOB_CHANNEL,
            super::job_watch::notified as Wake,
        ),
        (oxidgene_db::repo::QUEUE_CHANNEL, wake_worker as Wake),
    ];
    #[cfg(feature = "auth")]
    channels.extend([
        (
            oxidgene_db::repo::UPDATES_CHANNEL,
            super::updates::notified as Wake,
        ),
        (oxidgene_db::repo::MAIL_CHANNEL, wake_mail_sender as Wake),
    ]);
    channels
}

/// A job was queued by another process, or listening just started.
fn wake_worker(_: Option<&str>) {
    super::job_queue::wake();
}

/// A mail was queued by another process, or listening just started.
#[cfg(feature = "auth")]
fn wake_mail_sender(_: Option<&str>) {
    crate::mail::sender::wake();
}

/// Whether this process's listener was started.
static STARTED: AtomicBool = AtomicBool::new(false);

/// Start this process's listener on the database of `db`, unless it runs
/// already or `db` is not PostgreSQL. It runs until the process ends.
///
/// A process works on one database: the listener is the first one's.
pub(crate) fn start(db: &DatabaseConnection) {
    if db.get_database_backend() != DbBackend::Postgres || STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    let options = db.get_postgres_connection_pool().connect_options();
    tokio::spawn(async move {
        loop {
            if relay(&options).await.is_err() {
                tracing::warn!(
                    error = "notification_listener",
                    "lost the database notifications; listening again"
                );
                wake_all();
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    });
}

/// Wake every part: what was notified is unknown.
fn wake_all() {
    for (_, wake) in channels() {
        wake(None);
    }
}

/// Wake what each notification of the database is for, and every part
/// each time listening starts, until listening fails.
async fn relay(
    options: &PgConnectOptions,
) -> Result<std::convert::Infallible, sea_orm::sqlx::Error> {
    // A pool of one, without the timeouts of a pool that serves requests:
    // the listener's connection, opened beside the application's pool.
    let own = PgPoolOptions::new()
        .max_connections(1)
        .max_lifetime(None)
        .idle_timeout(None)
        .connect_with(options.clone())
        .await?;
    let channels = channels();
    loop {
        let mut listener = PgListener::connect_with(&own).await?;
        listener
            .listen_all(channels.iter().map(|(channel, _)| *channel))
            .await?;
        // Whatever was notified while nobody listened.
        wake_all();
        // `None` is a lost connection, which `recv` would reconnect behind
        // our back, dropping what was sent meanwhile: listen again instead,
        // and wake everybody.
        while let Some(notification) = listener.try_recv().await? {
            if let Some((_, wake)) = channels
                .iter()
                .find(|(channel, _)| *channel == notification.channel())
            {
                wake(Some(notification.payload()));
            }
        }
    }
}
