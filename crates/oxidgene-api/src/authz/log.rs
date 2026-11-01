//! Recording the access log (docs/authorization.md §9.2): refused requests
//! here, sign-ins, sign-outs and administrator actions where they happen.
//!
//! An event is written on the writer in a task of its own: a refusal is
//! answered at once, and an import holding the SQLite writer delays the
//! record, never the answer. A failed write is logged and dropped; the log
//! is an audit aid, not a gate.

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

/// Record `event` without waiting for it.
pub fn record(db: &DatabaseConnection, event: NewAccessEvent) {
    let db = db.clone();
    tokio::spawn(async move {
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
