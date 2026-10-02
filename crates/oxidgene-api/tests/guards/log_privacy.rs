//! Privacy of logs and traces: no genealogical content in any span or event
//! field, at any level.
//!
//! Drift it prevents: a `debug!("renamed {name}")`, a span field holding a
//! search query, a place or a note, or a SQL statement with its values
//! inlined — logs and traces leave the machine through OTLP and must carry
//! identifiers and counts only (docs/cross-cutting.md, privacy).
//!
//! The test installs a subscriber that keeps every field of every span and
//! event, at every level and from every crate, builds the populated fixture
//! (`common::populated`) and runs a survey of reads over REST and GraphQL —
//! searches by name included — then looks for the fixture's fictitious
//! markers in what was recorded.
//!
//! Fixing a failure: log the record's id, a count or a length instead of
//! its text; keep `db.statement` parameterised.

use axum::http::Method;

use crate::common::capture::global_capture;
use crate::common::populated::{NAME_MARKERS, populated_tree};
use crate::common::{app_on, ok, setup_db};

#[tokio::test]
async fn no_log_or_span_field_carries_genealogical_content() {
    let capture = global_capture();
    let db = setup_db().await;
    let app = app_on(db.clone());
    let tree = populated_tree(&app, &db, "Privacy", 1).await;
    let t = |path: &str| format!("/api/v1/trees/{}{path}", tree.tree_id);
    let person = tree.id("person_id");
    for uri in [
        t("/persons/search?q=Anchor"),
        t("/persons?search=Anchor"),
        t(&format!("/persons/{person}/detail-bundle")),
        t(&format!(
            "/pedigree/{person}?ancestor_depth=3&descendant_depth=2"
        )),
        t("/suggestions/given-names?q=Anc&lang=en"),
        t("/dictionary/places"),
        t("/dictionary/family-names"),
        t("/notes"),
        t("/statistics?lang=en"),
        t("/anomalies"),
        t("/duplicates"),
        t("/gedcom/export"),
    ] {
        ok(&app, Method::GET, &uri, None).await;
    }
    #[cfg(feature = "graphql")]
    crate::common::gql_ok(
        &app,
        "query($t: ID!) { searchPersons(treeId: $t, query: \"Anchor\") { totalCount } }",
        serde_json::json!({ "t": tree.tree_id }),
    )
    .await;

    let recorded = capture.take();
    assert!(
        recorded.iter().any(|c| c.field == "db.statement"),
        "the capture saw no SQL statement: it is not installed"
    );
    let leaks: Vec<String> = recorded
        .iter()
        .filter(|c| NAME_MARKERS.iter().any(|marker| c.value.contains(marker)))
        .map(|c| {
            let value: String = c.value.chars().take(160).collect();
            format!("{} {}: {value}", c.owner, c.field)
        })
        .collect();
    assert!(
        leaks.is_empty(),
        "fields carrying genealogical content:\n{}",
        leaks.join("\n")
    );
}
