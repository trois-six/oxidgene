//! Recording the access log (docs/authorization.md §9.2): refused requests
//! here, sign-ins, sign-outs and administrator actions where they happen.
//!
//! An event is written on the writer in a task of its own: a refusal is
//! answered at once, and an import holding the SQLite writer delays the
//! record, never the answer. A failed write is logged and dropped; the log
//! is an audit aid, not a gate. So is an event met while 1,024 others
//! wait to be written: a flood of refused requests piles up neither tasks
//! nor memory, nor holds back the writes that matter.

use std::sync::atomic::{AtomicUsize, Ordering};

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
    record(db, event);
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
    fn a_browser_holding_share_links_is_logged_as_a_link_holder() {
        let holder = ViewerContext::anonymous().with_share_links(vec![Uuid::nil()]);
        assert_eq!(principal_kind(&holder), "link");
        assert_eq!(principal_kind(&ViewerContext::anonymous()), "anonymous");
        assert_eq!(principal_kind(&ViewerContext::local()), "local");
    }
}
