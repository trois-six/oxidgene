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
//! Both trees carry a note, a citation, a media link and a portrait crop on
//! every person, so the reads that gather them are measured too. The same
//! survey runs over GraphQL, whose nested fields must cost one query per
//! relation for a whole page rather than one per record; and the batch reads
//! (pedigrees at three depths, portraits, image data, gallery bundles,
//! relation labels) are measured with 4 and with 64 ids, which must cost the
//! same.
//!
//! SeaORM opens one `sea_orm.*` span per statement (its `tracing-spans`
//! feature); a thread-local subscriber counts them for the request under test.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::http::Method;
use common::populated::{png, upload_page};
use common::{all_profiles, app_on, family_blocks_tree, ok, setup_db};
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

/// A note, a citation, a media link and a portrait crop on every person of
/// `tree`, all on one source and one scanned page.
async fn enrich(app: &Router, tree: &str) {
    let t = |path: &str| format!("/api/v1/trees/{tree}{path}");
    let source = ok(
        app,
        Method::POST,
        &t("/sources"),
        Some(json!({ "title": "Register" })),
    )
    .await;
    let document = ok(
        app,
        Method::POST,
        &t("/media/document"),
        Some(json!({ "title": "Scan" })),
    )
    .await;
    let document = document["id"].as_str().unwrap();
    let page = upload_page(app, tree, document, &png(64, 48)).await;
    for profile in all_profiles(app, tree).await {
        let person = profile["person_id"].as_str().unwrap();
        let note = json!({ "text": "Fictitious note", "person_id": person });
        ok(app, Method::POST, &t("/notes"), Some(note)).await;
        let citation = json!({ "source_id": source["id"], "person_id": person });
        ok(app, Method::POST, &t("/citations"), Some(citation)).await;
        let link = json!({ "media_id": document, "person_id": person });
        ok(app, Method::POST, &t("/media-links"), Some(link)).await;
        let crop = json!({ "x": 2, "y": 2, "width": 20, "height": 20, "person_id": person });
        let vignette = ok(
            app,
            Method::POST,
            &t(&format!(
                "/media/{}/vignettes",
                page["id"].as_str().unwrap()
            )),
            Some(crop),
        )
        .await;
        let portrait = json!({ "vignette_id": vignette["id"] });
        ok(
            app,
            Method::PUT,
            &t(&format!("/persons/{person}/portrait")),
            Some(portrait),
        )
        .await;
    }
}

/// A small and a large enriched tree in one router: `(tree, anchor)` each.
async fn trees(
    app: &Router,
    db: &oxidgene_db::sea_orm::DatabaseConnection,
) -> [(String, String); 2] {
    let small = family_blocks_tree(app, db, 4).await;
    let large = family_blocks_tree(app, db, 16).await;
    enrich(app, &small.0).await;
    enrich(app, &large.0).await;
    [small, large]
}

#[tokio::test]
async fn no_request_issues_statements_per_record() {
    let counter = Arc::new(AtomicUsize::new(0));
    let _guard = tracing_subscriber::registry()
        .with(StatementCounter(Arc::clone(&counter)))
        .set_default();

    let db = setup_db().await;
    let app = app_on(db.clone());
    let [small, large] = trees(&app, &db).await;

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

/// The ids of the first `n` persons, families and portrait crops of `tree`.
async fn ids(app: &Router, tree: &str, n: usize) -> (Vec<String>, Vec<String>, Vec<String>) {
    let profiles = all_profiles(app, tree).await;
    let take = |key: &str| -> Vec<String> {
        profiles
            .iter()
            .filter_map(|p| p.pointer(key).and_then(Value::as_str).map(str::to_string))
            .take(n)
            .collect()
    };
    let mut families: Vec<String> = take("/families_as_spouse/0/family_id");
    families.dedup();
    (
        take("/person_id"),
        families,
        take("/primary_media/vignette_id"),
    )
}

#[tokio::test]
async fn a_batch_of_64_costs_what_a_batch_of_4_does() {
    let counter = Arc::new(AtomicUsize::new(0));
    let _guard = tracing_subscriber::registry()
        .with(StatementCounter(Arc::clone(&counter)))
        .set_default();
    let db = setup_db().await;
    let app = app_on(db.clone());
    let [_, (tree, _)] = trees(&app, &db).await;

    let mut grows = Vec::new();
    let mut counts = Vec::new();
    for n in [4, 64] {
        let (persons, families, crops) = ids(&app, &tree, n).await;
        assert!(
            persons.len() == n && crops.len() == n,
            "the tree holds {n} portraits"
        );
        let sources: Vec<Value> = crops
            .iter()
            .map(|id| json!({ "kind": "crop", "vignette_id": id }))
            .collect();
        // The pedigrees at three depths: the roots of a batch share its walks
        // and reads, so its cost may follow the depth, never the roots.
        let pedigrees = [(2, 1), (5, 3), (10, 10)].map(|(up, down)| {
            let body = json!({
                "root_person_ids": persons, "ancestor_depth": up, "descendant_depth": down
            });
            (format!("/pedigrees at {up}/{down} generations"), body)
        });
        let others = [
            ("/portrait-images", json!({ "person_ids": persons })),
            ("/image-data", json!({ "sources": sources })),
            (
                "/gallery-bundle",
                json!({ "media_ids": [], "vignette_ids": crops }),
            ),
            (
                "/relation-labels",
                json!({ "person_ids": persons, "family_ids": families }),
            ),
        ];
        let batches = pedigrees.into_iter().chain(
            others
                .into_iter()
                .map(|(path, body)| (path.to_string(), body)),
        );
        let mut row = Vec::new();
        for (label, body) in batches {
            counter.store(0, Ordering::Relaxed);
            let path = label.split(' ').next().unwrap_or_default();
            ok(
                &app,
                Method::POST,
                &format!("/api/v1/trees/{tree}{path}"),
                Some(body),
            )
            .await;
            row.push((label, counter.load(Ordering::Relaxed)));
        }
        counts.push(row);
    }
    for ((label, four), (_, sixty_four)) in counts[0].iter().zip(&counts[1]) {
        if sixty_four > four {
            grows.push(format!(
                "POST {label}: {four} statements for 4 ids, {sixty_four} for 64"
            ));
        }
    }
    assert!(grows.is_empty(), "{}", grows.join("\n"));
}

/// The GraphQL survey: the tree-wide reads and the connections with their
/// nested lists, `$tree` and `$anchor` bound.
#[cfg(feature = "graphql")]
const GRAPHQL_CASES: &[&str] = &[
    "query($tree: ID!) { persons(treeId: $tree, first: 100) { edges { node { id names { surname } primaryName { surname } events { eventType place { id } } families { id } citations { id } media { id } notes { id } } } } }",
    "query($tree: ID!) { families(treeId: $tree, first: 100) { edges { node { id spouses { id person { id names { surname } } } children { id person { id } } events { id } } } } }",
    "query($tree: ID!) { events(treeId: $tree, first: 100) { edges { node { id place { id } person { id } family { id } citations { id } media { id } notes { id } witnesses { personId person { id } } } } } }",
    "query($tree: ID!) { sources(treeId: $tree, first: 100) { edges { node { id citations { id } repositories { id repository { id } source { id } } } } } }",
    "query { trees(first: 100) { edges { node { id personCount familyCount } } } }",
    "query($tree: ID!) { citations(treeId: $tree, first: 100) { edges { node { id } } } }",
    "query($tree: ID!) { notes(treeId: $tree, first: 100) { edges { node { id } } } }",
    "query($tree: ID!) { personProfiles(treeId: $tree, first: 100) { edges { node { personId } } } }",
    "query($tree: ID!) { treeMediaLinks(treeId: $tree) { __typename } }",
    "query($tree: ID!) { portraits(treeId: $tree) { __typename } }",
    "query($tree: ID!) { tree(id: $tree) { personCount familyCount } }",
    "query($tree: ID!) { treeStatistics(treeId: $tree, language: \"en\") { __typename } }",
    "query($tree: ID!) { treeAnomalies(treeId: $tree) { __typename } }",
    "query($tree: ID!) { potentialDuplicates(treeId: $tree) { __typename } }",
    "query($tree: ID!) { searchPersons(treeId: $tree, query: \"Anchor\") { totalCount } }",
    "query($tree: ID!, $anchor: ID!) { personDetailBundle(treeId: $tree, personId: $anchor) { __typename } }",
    "query($tree: ID!, $anchor: ID!) { pedigree(treeId: $tree, rootPersonId: $anchor, ancestorDepth: 5, descendantDepth: 3) { __typename } }",
    "query($tree: ID!, $anchor: ID!) { ancestors(treeId: $tree, personId: $anchor) { __typename } }",
    "query($tree: ID!) { dictionaryFamilyNames(treeId: $tree) { __typename } }",
    "query($tree: ID!) { dictionaryPlaces(treeId: $tree) { __typename } }",
];

#[cfg(feature = "graphql")]
#[tokio::test]
async fn no_graphql_query_issues_statements_per_record() {
    let counter = Arc::new(AtomicUsize::new(0));
    let _guard = tracing_subscriber::registry()
        .with(StatementCounter(Arc::clone(&counter)))
        .set_default();
    let db = setup_db().await;
    let app = app_on(db.clone());
    let [small, large] = trees(&app, &db).await;

    let mut grows = Vec::new();
    for query in GRAPHQL_CASES {
        let mut counts = Vec::new();
        for (tree, anchor) in [&small, &large] {
            counter.store(0, Ordering::Relaxed);
            let response =
                common::gql(&app, query, json!({ "tree": tree, "anchor": anchor })).await;
            assert!(response.get("errors").is_none(), "{query}: {response}");
            counts.push(counter.load(Ordering::Relaxed));
        }
        assert!(counts[0] > 0, "{query}: no statement counted");
        // A connection's page is up to 100 records: the larger tree fills
        // it, where the small one holds 40 persons and 16 families. Their
        // nested fields are read per relation, never per record.
        if counts[1] > counts[0] {
            grows.push(format!(
                "{query}: {} statements for 40 persons, {} for 160",
                counts[0], counts[1]
            ));
        }
    }
    assert!(grows.is_empty(), "{}", grows.join("\n"));
}
