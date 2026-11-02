//! The worker's mail sender (docs/messaging.md §3.3): it claims the due rows
//! of the outbox with a lease, renders and hands each to the relay, and
//! marks it sent; a failure is tried again after 1, 5 and 30 minutes, then
//! 2 and 12 hours, and marked failed after the sixth attempt with its
//! category. A permanent refusal — an SMTP 5xx reply — is failed at once.
//!
//! Idle, it reads nothing until there may be mail, as the job worker does
//! (docs/architecture.md §6.5): a mail queued in this process wakes it in
//! memory, one queued elsewhere through PostgreSQL's notifications; a
//! retry or a digest falls due at a time the outbox knows, and it wakes
//! then; and a minute after it went to sleep at the latest.
//!
//! Logs carry the kind, the outcome and the attempt, never an address.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use chrono::Utc;
use oxidgene_core::OxidGeneError;
use oxidgene_db::repo::{MailOutboxRepo, OutboxMail};
use oxidgene_db::sea_orm::DatabaseConnection;
use tokio::sync::Notify;

use super::relay::Relay;
use super::templates::{self, MailContext, Rendered};
use crate::service::job_queue::IDLE_RECHECK;

/// How long a worker holds a mail it is sending; another takes it over
/// once this has passed.
const LEASE: Duration = Duration::from_secs(5 * 60);

/// The waits before the second to the sixth attempt.
const RETRY_DELAYS: [Duration; 5] = [
    Duration::from_secs(60),
    Duration::from_secs(5 * 60),
    Duration::from_secs(30 * 60),
    Duration::from_secs(2 * 60 * 60),
    Duration::from_secs(12 * 60 * 60),
];

/// The attempts after which a mail is failed.
pub const MAX_ATTEMPTS: i32 = 6;

/// The pause after the outbox could not be read.
const ERROR_PAUSE: Duration = Duration::from_secs(5);

fn queued() -> &'static Notify {
    static QUEUED: OnceLock<Notify> = OnceLock::new();
    QUEUED.get_or_init(Notify::new)
}

/// Tell this process's sender a mail was queued and committed. A sender
/// busy at that moment finds the signal when it next goes idle.
pub fn wake() {
    queued().notify_one();
}

/// The sender of one process.
#[derive(Clone)]
pub struct MailSender {
    db: DatabaseConnection,
    relay: Arc<dyn Relay>,
    context: Arc<MailContext>,
}

impl std::fmt::Debug for MailSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MailSender")
    }
}

/// What happened to one mail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Sent,
    /// Tried again later.
    Retried,
    Failed,
    Dropped,
}

impl Outcome {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Sent => "sent",
            Self::Retried => "retried",
            Self::Failed => "failed",
            Self::Dropped => "dropped",
        }
    }
}

impl MailSender {
    #[must_use]
    pub fn new(db: DatabaseConnection, relay: Arc<dyn Relay>, context: MailContext) -> Self {
        Self {
            db,
            relay,
            context: Arc::new(context),
        }
    }

    /// Send mail until the process ends. Under PostgreSQL a task of its own
    /// listens for the mail other processes queue; dropping this future
    /// stops both.
    pub async fn run(self) {
        #[cfg(feature = "postgres")]
        let _listener = AbortOnDrop(tokio::spawn(listen(self.db.clone())));
        loop {
            match self.send_next().await {
                Ok(Some(_)) => {}
                Ok(None) => self.idle().await,
                Err(error) => {
                    tracing::warn!(
                        error.category = "mail_outbox_unavailable",
                        error.kind = crate::error_contract::error_kind(&error),
                        "could not read the mail outbox; trying again"
                    );
                    tokio::time::sleep(ERROR_PAUSE).await;
                }
            }
        }
    }

    /// Claim and handle the mail due longest, if any: what became of it.
    ///
    /// # Errors
    ///
    /// When the outbox cannot be read or written.
    pub async fn send_next(&self) -> Result<Option<Outcome>, OxidGeneError> {
        let lease = chrono::Duration::from_std(LEASE)
            .map_err(|error| OxidGeneError::Internal(error.to_string()))?;
        let Some(mail) = MailOutboxRepo::claim_next(&self.db, Utc::now(), lease).await? else {
            return Ok(None);
        };
        let outcome = self.handle(&mail).await?;
        tracing::info!(
            mail.kind = %mail.kind,
            mail.outcome = outcome.as_str(),
            mail.attempt = mail.attempts,
            "mail handled"
        );
        Ok(Some(outcome))
    }

    /// Render `mail` and hand it over, or drop it. A database failure while
    /// reading what the mail names is an error: the lease runs out and the
    /// mail is claimed again.
    async fn handle(&self, mail: &OutboxMail) -> Result<Outcome, OxidGeneError> {
        match templates::render(&self.db, &self.context, mail).await? {
            Rendered::Send(outgoing) => self.relay_one(mail, &outgoing).await,
            Rendered::Drop(reason) => {
                MailOutboxRepo::mark_dropped(&self.db, mail, Utc::now(), reason).await?;
                Ok(Outcome::Dropped)
            }
        }
    }

    /// Hand `outgoing`, rendered from `mail`, to the relay, and record how
    /// it went.
    async fn relay_one(
        &self,
        mail: &OutboxMail,
        outgoing: &super::relay::Outgoing,
    ) -> Result<Outcome, OxidGeneError> {
        match self.relay.send(outgoing).await {
            Ok(()) => {
                MailOutboxRepo::mark_sent(&self.db, mail, Utc::now()).await?;
                Ok(Outcome::Sent)
            }
            Err(failure) if failure.permanent || mail.attempts >= MAX_ATTEMPTS => {
                MailOutboxRepo::mark_failed(&self.db, mail, Utc::now(), failure.category).await?;
                tracing::warn!(
                    error.category = failure.category,
                    mail.kind = %mail.kind,
                    mail.attempt = mail.attempts,
                    "mail failed"
                );
                Ok(Outcome::Failed)
            }
            Err(failure) => {
                let delay = retry_delay(mail.attempts);
                let at = Utc::now() + chrono::Duration::from_std(delay).unwrap_or_default();
                MailOutboxRepo::retry_at(&self.db, mail, at, failure.category).await?;
                Ok(Outcome::Retried)
            }
        }
    }

    /// Sleep until there may be mail: one queued, a retry or a digest due,
    /// a lease run out, or [`IDLE_RECHECK`] at the latest.
    async fn idle(&self) {
        let now = tokio::time::Instant::now();
        let mut deadline = now + IDLE_RECHECK;
        match MailOutboxRepo::next_due(&self.db).await {
            Ok(Some(due)) => {
                let left = (due - Utc::now()).to_std().unwrap_or_default();
                // A second's grace, so that the row is due when looked at.
                deadline = deadline.min(now + left + Duration::from_secs(1));
            }
            Ok(None) => {}
            Err(error) => tracing::warn!(
                error.kind = crate::error_contract::error_kind(&error),
                "could not read when the next mail is due"
            ),
        }
        tokio::select! {
            () = queued().notified() => {}
            () = tokio::time::sleep_until(deadline) => {}
        }
    }
}

/// The wait after attempt `attempts` failed.
fn retry_delay(attempts: i32) -> Duration {
    let index = usize::try_from(attempts.max(1) - 1).unwrap_or(0);
    RETRY_DELAYS[index.min(RETRY_DELAYS.len() - 1)]
}

/// A task that ends when its handle is dropped.
#[cfg(feature = "postgres")]
struct AbortOnDrop(tokio::task::JoinHandle<()>);

#[cfg(feature = "postgres")]
impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Listen, under PostgreSQL, to the mail other processes queue (see
/// [`oxidgene_db::repo::MAIL_CHANNEL`]) and wake this process's sender for
/// each, and each time listening starts. Does nothing on another database.
#[cfg(feature = "postgres")]
async fn listen(db: DatabaseConnection) {
    if db.get_database_backend() != oxidgene_db::sea_orm::DbBackend::Postgres {
        return;
    }
    let pool = db.get_postgres_connection_pool().clone();
    loop {
        if relay_notifications(&pool).await.is_err() {
            tracing::warn!(
                error = "mail_queue_listener",
                "lost the mail queue notifications; listening again"
            );
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}

#[cfg(feature = "postgres")]
async fn relay_notifications(
    pool: &oxidgene_db::sea_orm::sqlx::PgPool,
) -> Result<std::convert::Infallible, oxidgene_db::sea_orm::sqlx::Error> {
    loop {
        let mut listener =
            oxidgene_db::sea_orm::sqlx::postgres::PgListener::connect_with(pool).await?;
        listener.listen(oxidgene_db::repo::MAIL_CHANNEL).await?;
        // Whatever was queued while nobody listened.
        wake();
        while listener.try_recv().await?.is_some() {
            wake();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_wait_one_five_thirty_minutes_then_two_and_twelve_hours() {
        let minutes: Vec<u64> = (1..=5).map(|n| retry_delay(n).as_secs() / 60).collect();
        assert_eq!(minutes, [1, 5, 30, 120, 720]);
        assert_eq!(retry_delay(9), RETRY_DELAYS[4]);
    }
}
