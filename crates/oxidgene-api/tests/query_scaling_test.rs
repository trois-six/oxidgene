//! Algorithmic complexity of the REST surface, measured in SQL statements.
//!
//! Each request runs against two trees built from the same family block, one
//! four times the size of the other. A request that runs more statements on
//! the larger one issues queries per person, family or event: an N+1 pattern
//! whose cost grows with the tree instead of staying fixed. Counting
//! statements is deterministic, unlike timing, so this runs in the normal
//! suite; the timing checks of the in-memory algorithms are the opt-in
//! `scaling_*` tests of each crate.
//!
//! SeaORM opens one `sea_orm.*` span per statement (its `tracing-spans`
//! feature); a thread-local subscriber counts them for the request under test.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::http::Method;
use common::{family_blocks_tree, ok, setup_app};
use serde_json::{Value, json};
use tracing::Subscriber;
use tracing::span::{Attributes, Id};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt as _};
use tracing_subscriber::util::SubscriberInitExt as _;

/// Counts the SQL statements SeaORM runs.
struct StatementCounter(Arc<AtomicUsize>);

impl<S: Subscriber> Layer<S> for StatementCounter {
    fn on_new_span(&self, attributes: &Attributes<'_>, _id: &Id, _ctx: Context<'_, S>) {
        let name = attributes.metadata().name();
        // Transaction boundaries are not statements against the data.
        if name.starts_with("sea_orm.") && !matches!(name, "sea_orm.begin" | "sea_orm.commit") {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// One request of the survey; `{tree}` and `{anchor}` are substituted.
struct Case {
    method: Method,
    uri: &'static str,
    body: Option<Value>,
}

fn get(uri: &'static str) -> Case {
    Case {
        method: Method::GET,
        uri,
        body: None,
    }
}

fn cases() -> Vec<Case> {
    vec![
        get("/api/v1/trees/{tree}/statistics?lang=en"),
        get("/api/v1/trees/{tree}/statistics/growth"),
        get("/api/v1/trees/{tree}/anomalies"),
        get("/api/v1/trees/{tree}/duplicates"),
        get("/api/v1/trees/{tree}/unlocated-places"),
        get("/api/v1/trees/{tree}/ancestry-completeness?generations=8"),
        get("/api/v1/trees/{tree}/persons/search?q=Anchor"),
        get("/api/v1/trees/{tree}/persons/recently-modified"),
        get("/api/v1/trees/{tree}/profiles"),
        get("/api/v1/trees/{tree}/profiles/{anchor}"),
        get("/api/v1/trees/{tree}/pedigree/{anchor}?ancestor_depth=5&descendant_depth=3"),
        get("/api/v1/trees/{tree}/persons/{anchor}/detail-bundle"),
        get("/api/v1/trees/{tree}/persons/{anchor}/ancestors"),
        get("/api/v1/trees/{tree}/persons/{anchor}/descendants"),
        get("/api/v1/trees/{tree}/persons/{anchor}/homonyms"),
        get("/api/v1/trees/{tree}/persons/sosa/2"),
        get("/api/v1/trees/{tree}/persons"),
        get("/api/v1/trees/{tree}/families"),
        get("/api/v1/trees/{tree}/events"),
        get("/api/v1/trees/{tree}/places"),
        get("/api/v1/trees/{tree}/sources"),
        get("/api/v1/trees/{tree}/dictionary/family-names"),
        get("/api/v1/trees/{tree}/dictionary/occupations"),
        get("/api/v1/trees/{tree}/dictionary/places"),
        get("/api/v1/trees/{tree}/dictionary/sources"),
        get("/api/v1/trees/{tree}/dictionary/sources/groups"),
        get("/api/v1/trees/{tree}/suggestions/family-names?q=A&lang=en"),
        get("/api/v1/trees/{tree}/suggestions/given-names?q=A&lang=en"),
        get("/api/v1/trees/{tree}/audit"),
        get("/api/v1/trees/{tree}/gedcom/export"),
        Case {
            method: Method::POST,
            uri: "/api/v1/trees/{tree}/profiles/rebuild",
            body: None,
        },
        Case {
            method: Method::PATCH,
            uri: "/api/v1/trees/{tree}/dictionary/family-names/rename",
            body: Some(json!({ "value": "Dunmore", "new_value": "Dunmoor" })),
        },
        Case {
            method: Method::POST,
            uri: "/api/v1/trees/{tree}/duplicate",
            body: Some(json!({ "name": "Copy" })),
        },
        Case {
            method: Method::PUT,
            uri: "/api/v1/trees/{tree}/persons/{anchor}",
            body: Some(json!({ "sex": "male" })),
        },
        Case {
            method: Method::POST,
            uri: "/api/v1/trees/{tree}/events",
            body: Some(json!({
                "event_type": "occupation",
                "person_id": "{anchor}",
                "description": "Thatcher",
            })),
        },
        // Last: the anchor is gone afterwards.
        Case {
            method: Method::DELETE,
            uri: "/api/v1/trees/{tree}/persons/{anchor}",
            body: None,
        },
    ]
}

/// The SQL statements `case` runs against `(tree, anchor)`.
async fn statements(
    app: &Router,
    counter: &AtomicUsize,
    case: &Case,
    ids: &(String, String),
) -> usize {
    let fill = |s: &str| s.replace("{tree}", &ids.0).replace("{anchor}", &ids.1);
    let body = case
        .body
        .as_ref()
        .map(|b| serde_json::from_str(&fill(&b.to_string())).unwrap());
    counter.store(0, Ordering::Relaxed);
    ok(app, case.method.clone(), &fill(case.uri), body).await;
    counter.load(Ordering::Relaxed)
}

#[tokio::test]
async fn no_request_issues_statements_per_record() {
    let counter = Arc::new(AtomicUsize::new(0));
    let _guard = tracing_subscriber::registry()
        .with(StatementCounter(Arc::clone(&counter)))
        .set_default();

    let app = setup_app().await;
    let small = family_blocks_tree(&app, 4).await;
    let large = family_blocks_tree(&app, 16).await;

    let mut grows = Vec::new();
    for case in cases() {
        let at_small = statements(&app, &counter, &case, &small).await;
        let at_large = statements(&app, &counter, &case, &large).await;
        assert!(
            at_small > 0,
            "{} {}: no statement counted",
            case.method,
            case.uri
        );
        // Fewer is fine: a bulk refresh past its threshold reads the whole
        // tree in a fixed number of statements instead of person by person.
        if at_large > at_small {
            grows.push(format!(
                "{} {}: {at_small} statements for 40 persons, {at_large} for 160",
                case.method, case.uri
            ));
        }
    }
    assert!(grows.is_empty(), "{}", grows.join("\n"));
}
