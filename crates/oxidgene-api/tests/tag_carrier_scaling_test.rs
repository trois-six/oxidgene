//! The persons whose profiles a tag change refreshes are found in a fixed number
//! of batched reads, whatever the number of events carrying the tag. A page of
//! the records a tag is on, of each kind that names an owner, is read in a fixed
//! number of batched reads too.
//!
//! Two trees hold the same three persons: a family of two spouses and one
//! individual. In the small tree the tag sits on one event of each owner; in
//! the large one on thirty. The carrier set is the same in both, so a rename,
//! a recolour, a merge and a delete must each issue the same number of SQL
//! statements. Counting statements is deterministic, so this runs in the
//! normal suite.
//!
//! All data is fictitious.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::http::{Method, StatusCode};
use common::{app_on, new_person, new_tree, ok, send, setup_db};
use oxidgene_api::profile::ProfileService;
use oxidgene_api::service::tag;
use oxidgene_core::types::RecordKind;
use oxidgene_db::repo::PaginationParams;
use oxidgene_db::sea_orm::DatabaseConnection;
use serde_json::{Value, json};
use tracing::Subscriber;
use tracing::span::{Attributes, Id};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt as _};
use tracing_subscriber::util::SubscriberInitExt as _;
use uuid::Uuid;

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

/// Creates tag `label` in `tree` and returns its id.
async fn create_tag(app: &Router, tree: &str, label: &str) -> String {
    ok(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/tags"),
        Some(json!({ "label": label })),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// Adds person `id` of `tree` to the family of spouses `family` as `role`.
async fn add_spouse(app: &Router, tree: &str, family: &str, person: &str, role: &str) {
    ok(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/families/{family}/spouses"),
        Some(json!({ "person_id": person, "role": role, "sort_order": 0 })),
    )
    .await;
}

/// Creates an event of `tree` with `body` and returns its id.
async fn create_event(app: &Router, tree: &str, body: Value) -> String {
    ok(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/events"),
        Some(body),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// Creates the resource at `suffix` of `tree` from `body` and returns its id.
async fn create_record(app: &Router, tree: &str, suffix: &str, body: Value) -> String {
    ok(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/{suffix}"),
        Some(body),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// Puts tag `tag_id` on every record of `kind` (its wire spelling) in `records`.
async fn put_tag_on(app: &Router, tree: &str, tag_id: &str, kind: &str, records: &[String]) {
    for record in records {
        let (status, body) = send(
            app,
            Method::PUT,
            &format!("/api/v1/trees/{tree}/tags/{tag_id}/records/{kind}/{record}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    }
}

/// A tree with one family of two spouses and one individual, each owner with
/// `events` events, all carrying tags `Ledger` and `Journal`. Returns the tree
/// and the ids of `(Ledger, Journal)`.
async fn tagged_tree(app: &Router, label: &str, events: usize) -> (String, Uuid, Uuid) {
    let tree = new_tree(app, label).await;
    let tag_ids = [
        create_tag(app, &tree, "Ledger").await,
        create_tag(app, &tree, "Journal").await,
    ];

    let individual = new_person(app, &tree).await;
    let husband = new_person(app, &tree).await;
    let wife = new_person(app, &tree).await;
    let family = ok(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/families"),
        Some(json!({})),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    add_spouse(app, &tree, &family, &husband, "husband").await;
    add_spouse(app, &tree, &family, &wife, "wife").await;

    let mut owned = Vec::new();
    for _ in 0..events {
        owned.push(
            create_event(
                app,
                &tree,
                json!({ "event_type": "occupation", "person_id": individual }),
            )
            .await,
        );
        owned.push(
            create_event(
                app,
                &tree,
                json!({ "event_type": "marriage", "family_id": family }),
            )
            .await,
        );
    }
    for tag_id in &tag_ids {
        put_tag_on(app, &tree, tag_id, "event", &owned).await;
    }
    let [ledger, journal] = tag_ids.map(|id| Uuid::parse_str(&id).unwrap());
    (tree, ledger, journal)
}

/// Cites `event` of `tree` from `source` and writes a note on it; returns the
/// ids of `(citation, note)`.
async fn cite_and_note(app: &Router, tree: &str, source: &str, event: &str) -> (String, String) {
    let citation = create_record(
        app,
        tree,
        "citations",
        json!({ "source_id": source, "event_id": event, "text": "Entry" }),
    )
    .await;
    let note = create_record(
        app,
        tree,
        "notes",
        json!({ "text": "Remark", "event_id": event }),
    )
    .await;
    (citation, note)
}

/// Creates an occupation of `individual` and a marriage of `family` in `tree`,
/// each cited and noted by [`cite_and_note`]. Returns the two event ids and
/// pushes the citation and note ids on `records`.
async fn owner_events_with_records(
    app: &Router,
    tree: &str,
    source: &str,
    individual: &str,
    family: &str,
    records: (&mut Vec<String>, &mut Vec<String>),
) -> [String; 2] {
    let (citations, notes) = records;
    let occupation = create_event(
        app,
        tree,
        json!({ "event_type": "occupation", "person_id": individual }),
    )
    .await;
    let marriage = create_event(
        app,
        tree,
        json!({ "event_type": "marriage", "family_id": family }),
    )
    .await;
    for event in [&occupation, &marriage] {
        let (citation, note) = cite_and_note(app, tree, source, event).await;
        citations.push(citation);
        notes.push(note);
    }
    [occupation, marriage]
}

/// A tree like [`tagged_tree`] with one source, and on each of its events a
/// citation and a note, all carrying tag `Ledger`. Returns the tree and the id
/// of `Ledger`.
async fn tagged_records_tree(app: &Router, label: &str, events: usize) -> (String, Uuid) {
    let tree = new_tree(app, label).await;
    let tag = create_tag(app, &tree, "Ledger").await;
    let source = create_record(app, &tree, "sources", json!({ "title": "Register" })).await;

    let individual = new_person(app, &tree).await;
    let husband = new_person(app, &tree).await;
    let wife = new_person(app, &tree).await;
    let family = create_record(app, &tree, "families", json!({})).await;
    add_spouse(app, &tree, &family, &husband, "husband").await;
    add_spouse(app, &tree, &family, &wife, "wife").await;

    let mut owned = Vec::new();
    let mut citations = Vec::new();
    let mut notes = Vec::new();
    for _ in 0..events {
        let pair = owner_events_with_records(
            app,
            &tree,
            &source,
            &individual,
            &family,
            (&mut citations, &mut notes),
        )
        .await;
        owned.extend(pair);
    }
    put_tag_on(app, &tree, &tag, "event", &owned).await;
    put_tag_on(app, &tree, &tag, "citation", &citations).await;
    put_tag_on(app, &tree, &tag, "note", &notes).await;
    (tree, Uuid::parse_str(&tag).unwrap())
}

/// The statements `change` runs, counted by `counter`.
async fn statements<F, Fut>(counter: &AtomicUsize, change: F) -> usize
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    counter.store(0, Ordering::Relaxed);
    change().await;
    counter.load(Ordering::Relaxed)
}

#[tokio::test]
async fn a_tag_on_many_events_costs_what_it_does_on_few() {
    let counter = Arc::new(AtomicUsize::new(0));
    let _guard = tracing_subscriber::registry()
        .with(StatementCounter(Arc::clone(&counter)))
        .set_default();

    let db: DatabaseConnection = setup_db().await;
    let app = app_on(db.clone());
    let profiles = ProfileService::new(db.clone());

    let (small, small_tag, small_journal) = tagged_tree(&app, "Tags Scaling Few", 1).await;
    let (large, large_tag, large_journal) = tagged_tree(&app, "Tags Scaling Many", 30).await;
    let small_id = Uuid::parse_str(&small).unwrap();
    let large_id = Uuid::parse_str(&large).unwrap();

    let rename_small = statements(&counter, || async {
        tag::update(&db, &profiles, small_id, small_tag, Some("Renamed"), None)
            .await
            .unwrap();
    })
    .await;
    let rename_large = statements(&counter, || async {
        tag::update(&db, &profiles, large_id, large_tag, Some("Renamed"), None)
            .await
            .unwrap();
    })
    .await;
    assert!(rename_small > 0, "no statement counted");
    assert_eq!(
        rename_large, rename_small,
        "renaming a tag on 30 events per owner costs {rename_large} statements, \
         renaming it on one costs {rename_small}"
    );

    let recolour_small = statements(&counter, || async {
        tag::update(&db, &profiles, small_id, small_tag, None, Some(3))
            .await
            .unwrap();
    })
    .await;
    let recolour_large = statements(&counter, || async {
        tag::update(&db, &profiles, large_id, large_tag, None, Some(3))
            .await
            .unwrap();
    })
    .await;
    assert_eq!(
        recolour_large, recolour_small,
        "recolouring a tag on 30 events per owner costs {recolour_large} statements, \
         recolouring it on one costs {recolour_small}"
    );

    let merge_small = statements(&counter, || async {
        tag::merge(&db, &profiles, small_id, small_journal, small_tag)
            .await
            .unwrap();
    })
    .await;
    let merge_large = statements(&counter, || async {
        tag::merge(&db, &profiles, large_id, large_journal, large_tag)
            .await
            .unwrap();
    })
    .await;
    assert_eq!(
        merge_large, merge_small,
        "merging a tag carried by 30 events per owner costs {merge_large} statements, \
         merging one carried by a single event costs {merge_small}"
    );

    let delete_small = statements(&counter, || async {
        tag::delete(&db, &profiles, small_id, small_tag)
            .await
            .unwrap();
    })
    .await;
    let delete_large = statements(&counter, || async {
        tag::delete(&db, &profiles, large_id, large_tag)
            .await
            .unwrap();
    })
    .await;
    assert_eq!(
        delete_large, delete_small,
        "deleting a tag on 30 events per owner costs {delete_large} statements, \
         deleting it on one costs {delete_small}"
    );
}

#[tokio::test]
async fn a_page_of_tagged_events_citations_and_notes_costs_the_same_on_many_as_on_few() {
    let counter = Arc::new(AtomicUsize::new(0));
    let _guard = tracing_subscriber::registry()
        .with(StatementCounter(Arc::clone(&counter)))
        .set_default();

    let db: DatabaseConnection = setup_db().await;
    let app = app_on(db.clone());

    let (small, small_tag) = tagged_records_tree(&app, "Tags Page Few", 1).await;
    let (large, large_tag) = tagged_records_tree(&app, "Tags Page Many", 30).await;
    let small_id = Uuid::parse_str(&small).unwrap();
    let large_id = Uuid::parse_str(&large).unwrap();
    let params = PaginationParams {
        first: 100,
        after: None,
    };

    for kind in [RecordKind::Event, RecordKind::Citation, RecordKind::Note] {
        let few = statements(&counter, || async {
            let page = tag::tagged_records(&db, small_id, small_tag, kind, &params)
                .await
                .unwrap();
            assert_eq!(page.edges.len(), 2, "{kind:?} on one event per owner");
            for edge in &page.edges {
                assert!(
                    edge.node.person_id.is_some() || edge.node.family_id.is_some(),
                    "{kind:?} {} has no owner",
                    edge.node.id
                );
            }
        })
        .await;
        let many = statements(&counter, || async {
            let page = tag::tagged_records(&db, large_id, large_tag, kind, &params)
                .await
                .unwrap();
            assert_eq!(page.edges.len(), 60, "{kind:?} on thirty events per owner");
            for edge in &page.edges {
                assert!(
                    edge.node.person_id.is_some() || edge.node.family_id.is_some(),
                    "{kind:?} {} has no owner",
                    edge.node.id
                );
            }
        })
        .await;
        assert!(few > 0, "no statement counted for {kind:?}");
        assert_eq!(
            many, few,
            "listing {kind:?} records on 30 events per owner costs {many} statements, \
             on one event per owner costs {few}"
        );
    }
}
