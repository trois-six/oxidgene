//! Waiting for a job to change: the long poll of the job status reads.
//!
//! A status read may ask to wait (`wait`, at most [`MAX_WAIT`]) until the
//! job's state differs from the `version` it last read (`since`). It then
//! answers as soon as the state changes, or when the wait runs out, with the
//! state as it is — REST and GraphQL alike, with no connection held open
//! beyond the one request.
//!
//! The waiting itself reads nothing: a waiter sleeps on a [`watch`] channel
//! of its job, and reads the job's state again only when the channel says
//! something changed. Whoever changes a job wakes it: the
//! worker of this process at each progress report, registration, cancellation
//! and end; and, under PostgreSQL, where the worker may run in another
//! process, a listener of the database's notifications, which the workers
//! send with each write to a job (see `listen`).
//!
//! At most [`MAX_WAITERS`] requests wait at once; past them a status read
//! answers at once, as one without `wait` does.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Duration;

use oxidgene_core::OxidGeneError;
use tokio::sync::{Semaphore, watch};
use uuid::Uuid;

/// The longest a status read waits for a change: below the timeouts of the
/// proxies a request may cross.
pub const MAX_WAIT: Duration = Duration::from_secs(25);

/// How many status reads may wait at once.
pub const MAX_WAITERS: usize = 256;

/// The channel of each job someone waits on.
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

/// Wake whoever waits on job `job_id`: its state may have changed.
pub(crate) fn announce(job_id: Uuid) {
    if let Some(channel) = channels().get(&job_id) {
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

/// A waiter's subscription to the changes of one job, taken before it reads
/// the job so that no change falls between the read and the wait.
struct Subscription {
    job_id: Uuid,
    changes: watch::Receiver<()>,
}

impl Subscription {
    fn new(job_id: Uuid) -> Self {
        let changes = channels()
            .entry(job_id)
            .or_insert_with(|| watch::channel(()).0)
            .subscribe();
        Self { job_id, changes }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        let mut channels = channels();
        // This subscription is the channel's last receiver.
        if channels
            .get(&self.job_id)
            .is_some_and(|channel| channel.receiver_count() <= 1)
        {
            channels.remove(&self.job_id);
        }
    }
}

/// How a status read waits.
#[derive(Debug, Clone, Default)]
pub struct JobWait {
    /// The `version` the caller last read; no wait without it.
    pub since: Option<String>,
    /// How long to wait at most, capped at [`MAX_WAIT`].
    pub wait: Duration,
}

/// A job status that can be waited on.
pub(crate) trait Versioned {
    /// What identifies this state: any visible change changes it.
    fn version(&self) -> &str;
    /// Whether the job has ended, so that it will not change again.
    fn has_ended(&self) -> bool;
}

/// Read job `job_id`'s status with `read`, waiting as `wait` says for it to
/// differ from `wait.since`; the status then.
pub(crate) async fn wait_for<T, F, Fut>(
    job_id: Uuid,
    wait: &JobWait,
    mut read: F,
) -> Result<T, OxidGeneError>
where
    T: Versioned,
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, OxidGeneError>>,
{
    let Some(since) = wait.since.as_deref() else {
        return read().await;
    };
    let wait = wait.wait.min(MAX_WAIT);
    if wait.is_zero() {
        return read().await;
    }
    // Past the bound, a read answers at once rather than queue.
    let Ok(_permit) = waiters().try_acquire() else {
        return read().await;
    };
    let deadline = tokio::time::Instant::now() + wait;
    let mut subscription = Subscription::new(job_id);
    loop {
        subscription.changes.borrow_and_update();
        let status = read().await?;
        if status.version() != since || status.has_ended() {
            return Ok(status);
        }
        tokio::select! {
            changed = subscription.changes.changed() => {
                if changed.is_err() {
                    return Ok(status);
                }
            }
            () = tokio::time::sleep_until(deadline) => return Ok(status),
        }
    }
}

/// Listen, under PostgreSQL, to the notifications the workers of every
/// process send as they write a job (see
/// [`oxidgene_db::repo::BackgroundJobRepo::announce`]), and wake its
/// waiters. Runs until the process ends; a lost connection is opened again,
/// and every waiter woken, since what it missed is unknown. Does nothing on
/// another database.
#[cfg(feature = "postgres")]
pub async fn listen(db: sea_orm::DatabaseConnection) {
    if db.get_database_backend() != sea_orm::DbBackend::Postgres {
        return;
    }
    let pool = db.get_postgres_connection_pool().clone();
    loop {
        if relay(&pool).await.is_err() {
            tracing::warn!(
                error = "job_notification_listener",
                "lost the job notifications; listening again"
            );
            announce_all();
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}

/// Wake the waiters of each job announced on `pool`'s database, and every
/// waiter each time it starts listening, until listening fails.
#[cfg(feature = "postgres")]
async fn relay(
    pool: &sea_orm::sqlx::PgPool,
) -> Result<std::convert::Infallible, sea_orm::sqlx::Error> {
    loop {
        let mut listener = sea_orm::sqlx::postgres::PgListener::connect_with(pool).await?;
        listener.listen(oxidgene_db::repo::JOB_CHANNEL).await?;
        // Whatever changed while nobody listened.
        announce_all();
        // `None` is a lost connection, which `recv` would reconnect behind
        // our back, dropping what was sent meanwhile: listen again instead,
        // and wake everybody.
        while let Some(notification) = listener.try_recv().await? {
            if let Ok(job_id) = notification.payload().parse() {
                announce(job_id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct State(String);

    impl Versioned for State {
        fn version(&self) -> &str {
            &self.0
        }
        fn has_ended(&self) -> bool {
            self.0 == "completed"
        }
    }

    fn waiting(since: &str, wait: Duration) -> JobWait {
        JobWait {
            since: Some(since.into()),
            wait,
        }
    }

    #[tokio::test]
    async fn a_change_answers_the_wait_at_once() {
        let job_id = Uuid::now_v7();
        let reads = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&reads);
        let waiter = tokio::spawn(async move {
            wait_for(job_id, &waiting("media.1.9", MAX_WAIT), || {
                let read = counted.fetch_add(1, Ordering::SeqCst);
                async move {
                    Ok(State(
                        if read == 0 { "media.1.9" } else { "media.2.9" }.into(),
                    ))
                }
            })
            .await
        });
        while reads.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
        announce(job_id);
        let status = tokio::time::timeout(Duration::from_secs(5), waiter)
            .await
            .expect("the change ends the wait")
            .unwrap()
            .unwrap();
        assert_eq!(status.0, "media.2.9");
        // It read once, then once more when woken: no polling in between.
        assert_eq!(reads.load(Ordering::SeqCst), 2);
        assert!(
            !channels().contains_key(&job_id),
            "the channel goes with its last waiter"
        );
    }

    #[tokio::test]
    async fn an_unchanged_job_answers_when_the_wait_runs_out() {
        let status = wait_for(
            Uuid::now_v7(),
            &waiting("media.1.9", Duration::from_millis(50)),
            || async { Ok(State("media.1.9".into())) },
        )
        .await
        .unwrap();
        assert_eq!(status.0, "media.1.9");
    }

    #[tokio::test]
    async fn a_read_without_a_version_waits_for_nothing() {
        let status = tokio::time::timeout(
            Duration::from_millis(500),
            wait_for(
                Uuid::now_v7(),
                &JobWait {
                    since: None,
                    wait: MAX_WAIT,
                },
                || async { Ok(State("media.1.9".into())) },
            ),
        )
        .await
        .expect("answers at once")
        .unwrap();
        assert_eq!(status.0, "media.1.9");
    }
}
