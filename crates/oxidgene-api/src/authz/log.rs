//! Recording the access log (docs/authorization.md §9.2): refused requests
//! here, sign-ins, sign-outs and administrator actions where they happen.
//!
//! An event is written on the writer in a task of its own: a refusal is
//! answered at once, and an import holding the SQLite writer delays the
//! record, never the answer. A failed write is logged and dropped; the log
//! is an audit aid, not a gate. So is an event met while 1,024 others
//! wait to be written: a flood of refused requests piles up neither tasks
//! nor memory, nor holds back the writes that matter.
//!
//! The events nobody answers for — a request refused to a visitor without
//! an account, a failed sign-in, a refused back-channel call — cost their
//! sender nothing, so they share a budget of sixty a minute: past
//! it they are dropped and counted, and a flood fills neither the table
//! nor the log.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use oxidgene_core::OxidGeneError;
use oxidgene_core::access::{Principal, ViewerContext};
use oxidgene_db::repo::{AccessEventRepo, NewAccessEvent};
use sea_orm::DatabaseConnection;
use uuid::Uuid;

use super::Requirement;
use crate::error_contract::classify;

/// The principal kind an event records for `viewer`.
pub fn principal_kind(viewer: &ViewerContext) -> &'static str {
    match viewer.principal() {
        Principal::Local => "local",
        // A browser holding share links' grants (docs/authorization.md §2).
        Principal::Anonymous if !viewer.share_links().is_empty() => "link",
        Principal::Anonymous => "anonymous",
        Principal::User(identity) if identity.api_client_id.is_some() => "api_client",
        Principal::User(_) => "user",
    }
}

/// An event of `kind` about `viewer`, with `outcome`.
pub fn event(viewer: &ViewerContext, kind: &'static str, outcome: &str) -> NewAccessEvent {
    let user = viewer.user();
    NewAccessEvent {
        kind,
        principal_kind: principal_kind(viewer),
        user_id: user.map(|identity| identity.user_id),
        api_client_id: user.and_then(|identity| identity.api_client_id),
        client_id: viewer.client_id().map(str::to_owned),
        outcome: outcome.to_string(),
        ..NewAccessEvent::default()
    }
}

/// The most events waiting to be written at once; one more is dropped.
const MAX_PENDING: usize = 1024;

/// The events waiting to be written.
static PENDING: AtomicUsize = AtomicUsize::new(0);

/// A place among the events waiting to be written, given back when
/// dropped.
struct Pending;

impl Drop for Pending {
    fn drop(&mut self) {
        PENDING.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Take a place among the events waiting to be written; none when all
/// [`MAX_PENDING`] are taken.
fn reserve() -> Option<Pending> {
    PENDING
        .try_update(Ordering::AcqRel, Ordering::Acquire, |pending| {
            (pending < MAX_PENDING).then_some(pending + 1)
        })
        .ok()
        .map(|_| Pending)
}

/// The most events without a user the process records in a minute: a
/// visitor's refused requests are few, and a flood's are all alike.
const ANONYMOUS_PER_MINUTE: u32 = 60;

/// The events without a user recorded and dropped in the current minute.
#[derive(Debug, Default)]
struct Budget {
    /// When the current minute ends; none before the first event.
    until: Option<Instant>,
    recorded: u32,
    dropped: u64,
}

impl Budget {
    /// Whether one more event is recorded at `now`; and, when a minute
    /// just ended, how many events it dropped.
    fn admit(&mut self, now: Instant) -> (bool, u64) {
        let mut dropped = 0;
        if self.until.is_none_or(|until| until <= now) {
            dropped = std::mem::take(&mut self.dropped);
            self.until = Some(now + Duration::from_secs(60));
            self.recorded = 0;
        }
        if self.recorded < ANONYMOUS_PER_MINUTE {
            self.recorded += 1;
            (true, dropped)
        } else {
            self.dropped += 1;
            (false, dropped)
        }
    }
}

static ANONYMOUS: Mutex<Budget> = Mutex::new(Budget {
    until: None,
    recorded: 0,
    dropped: 0,
});

/// Record `event`, which names no user, within the budget of such events
/// (sixty a minute). What a minute dropped is said once, when
/// the next one starts.
pub fn record_anonymous(db: &DatabaseConnection, event: NewAccessEvent) {
    let (admitted, dropped) = ANONYMOUS
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .admit(Instant::now());
    if dropped > 0 {
        tracing::warn!(
            dropped,
            "access events of requests without a user dropped: over the budget of a minute"
        );
    }
    if admitted {
        record(db, event);
    }
}

/// Record `event` without waiting for it.
pub fn record(db: &DatabaseConnection, event: NewAccessEvent) {
    let Some(pending) = reserve() else {
        tracing::warn!("access event dropped: too many waiting to be written");
        return;
    };
    let db = db.clone();
    tokio::spawn(async move {
        let _pending = pending;
        if let Err(error) = AccessEventRepo::insert(&db, event).await {
            tracing::warn!(
                error.kind = crate::error_contract::error_kind(&error),
                "access event not recorded"
            );
        }
    });
}

/// Record that the policy refused `route` to `viewer` with `error`: a
/// `401`, a `403`, or a `404` it chose. The local owner is never refused
/// but for a tree that does not exist, which the policy did not decide.
pub fn refusal(
    db: &DatabaseConnection,
    viewer: &ViewerContext,
    tree_id: Option<Uuid>,
    route: &str,
    requirement: Requirement,
    error: &OxidGeneError,
) {
    if viewer.is_local() || !super::is_refusal(error) {
        return;
    }
    let mut event = event(viewer, "request_refused", classify(error).code);
    event.tree_id = tree_id;
    event.route = Some(route.to_string());
    event.permission = Some(requirement.name());
    if viewer.user().is_some() {
        record(db, event);
    } else {
        record_anonymous(db, event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_more_than_the_bound_wait_to_be_written() {
        let held: Vec<Pending> = std::iter::from_fn(reserve).take(MAX_PENDING).collect();
        assert_eq!(held.len(), MAX_PENDING);
        assert!(reserve().is_none(), "one more than the bound");
        drop(held);
        assert!(reserve().is_some());
    }

    #[test]
    fn events_without_a_user_stay_within_their_budget_each_minute() {
        let mut budget = Budget::default();
        let start = Instant::now();
        let admitted = (0..ANONYMOUS_PER_MINUTE + 25)
            .filter(|_| budget.admit(start).0)
            .count();
        assert_eq!(admitted, ANONYMOUS_PER_MINUTE as usize);
        // Still the same minute: dropped, and nothing reported yet.
        assert_eq!(budget.admit(start + Duration::from_secs(59)), (false, 0));
        // The next minute admits again and reports what the last dropped.
        assert_eq!(budget.admit(start + Duration::from_secs(60)), (true, 26));
        assert_eq!(budget.admit(start + Duration::from_secs(61)), (true, 0));
    }

    #[test]
    fn a_browser_holding_share_links_is_logged_as_a_link_holder() {
        let holder = ViewerContext::anonymous().with_share_links(vec![Uuid::nil()]);
        assert_eq!(principal_kind(&holder), "link");
        assert_eq!(principal_kind(&ViewerContext::anonymous()), "anonymous");
        assert_eq!(principal_kind(&ViewerContext::local()), "local");
    }
}
