//! Waking an idle background job worker when there is work for it, rather
//! than having it poll the database.
//!
//! A worker with nothing to claim sleeps until one of these:
//!
//! - *a job is queued, or a cancellation it must write is noted, in this
//!   process* — the API that enqueues signals `wake` once the job row is
//!   committed; a SQLite deployment, whose single process runs both the API
//!   and the worker, needs nothing else;
//! - *a job is queued by another process (PostgreSQL)* — the enqueue sends
//!   `pg_notify('oxidgene_job_queue', '')` in the transaction that inserts
//!   the row, so it leaves when the row is visible, and each worker listens
//!   on that channel with one connection (`listen`); after a lost
//!   connection it listens again and looks for work, since what it missed is
//!   unknown;
//! - *a lease expires* — a job whose worker stopped becomes claimable when its
//!   lease runs out, a time the database knows, and the worker wakes then;
//! - *its maintenance falls due*, every six minutes;
//! - *the safety net*: [`IDLE_RECHECK`] after it went to sleep, whatever
//!   else happened.

use std::sync::OnceLock;
use std::time::Duration;

use tokio::sync::Notify;

/// The longest an idle worker sleeps without looking for work.
///
/// Nothing is expected to need it: a job queued here wakes the worker in
/// memory, one queued elsewhere by notification, and a lease that expires at
/// its expiry. It bounds how long a job can wait on a path that misses its
/// signal all the same, such as a connection pooler that drops notifications
/// (one in transaction mode does not carry `LISTEN`). A minute keeps that
/// worst case short for somebody waiting on an import, at two statements a
/// minute while idle instead of one a second.
pub const IDLE_RECHECK: Duration = Duration::from_secs(60);

fn queued() -> &'static Notify {
    static QUEUED: OnceLock<Notify> = OnceLock::new();
    QUEUED.get_or_init(Notify::new)
}

/// Tell the worker of this process there may be work: a job was queued, or a
/// cancellation it must write was noted.
///
/// A worker busy at that moment finds the signal waiting when it next goes
/// idle, and looks once more before it sleeps: no signal is lost between its
/// last look and its sleep.
pub(crate) fn wake() {
    queued().notify_one();
}

/// Sleep until `wake` is called or `deadline` passes.
pub(crate) async fn sleep_until(deadline: tokio::time::Instant) {
    tokio::select! {
        () = queued().notified() => {}
        () = tokio::time::sleep_until(deadline) => {}
    }
}

/// Listen, under PostgreSQL, to the jobs other processes queue (see
/// [`oxidgene_db::repo::QUEUE_CHANNEL`]) and wake this process's worker for
/// each. Runs until dropped; a lost connection is opened again, and the
/// worker woken each time listening starts, for what was queued meanwhile.
/// Does nothing on another database.
#[cfg(feature = "postgres")]
pub(crate) async fn listen(db: sea_orm::DatabaseConnection) {
    if db.get_database_backend() != sea_orm::DbBackend::Postgres {
        return;
    }
    let pool = db.get_postgres_connection_pool().clone();
    loop {
        if relay(&pool).await.is_err() {
            tracing::warn!(
                error = "job_queue_listener",
                "lost the job queue notifications; listening again"
            );
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}

/// Wake the worker for each job queued on `pool`'s database, and each time it
/// starts listening, until listening fails.
#[cfg(feature = "postgres")]
async fn relay(
    pool: &sea_orm::sqlx::PgPool,
) -> Result<std::convert::Infallible, sea_orm::sqlx::Error> {
    loop {
        let mut listener = sea_orm::sqlx::postgres::PgListener::connect_with(pool).await?;
        listener.listen(oxidgene_db::repo::QUEUE_CHANNEL).await?;
        // Whatever was queued while nobody listened.
        wake();
        // `None` is a lost connection, which `recv` would reconnect behind
        // our back, dropping what was sent meanwhile: listen again instead,
        // then look.
        while listener.try_recv().await?.is_some() {
            wake();
        }
    }
}
