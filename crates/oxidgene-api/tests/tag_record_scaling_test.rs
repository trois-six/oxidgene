//! The batched reads of places, repositories and notes cost the same whatever
//! the number of records they return, the place dictionary included. Counting
//! statements is deterministic, so this runs in the normal suite, where
//! nextest gives each test a process of its own: the statement counter is a
//! tracing layer, and other tests running alongside it in one process would
//! skew the callsite interest it relies on.
//!
//! All data is fictitious.

use crate::common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::http::{Method, StatusCode};
use common::{app_on, new_person, new_tree, ok, send, setup_db};
use oxidgene_api::service::tag;
use oxidgene_core::types::RecordKind;
use oxidgene_db::repo::{DictionaryRepo, NoteRepo, PaginationParams, PlaceRepo, RepositoryRepo};
use oxidgene_db::sea_orm::DatabaseConnection;
use serde_json::json;
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

/// A tree with `count` places, repositories and notes, each tagged `Ledger`,
/// a tag of that tree. Returns the tree and the tag ids.
async fn tagged_tree(app: &Router, label: &str, count: usize) -> (Uuid, Uuid) {
    let tree = new_tree(app, label).await;
    let tag_id = ok(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/tags"),
        Some(json!({ "label": "Ledger" })),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let person = new_person(app, &tree).await;
    for index in 0..count {
        let place = ok(
            app,
            Method::POST,
            &format!("/api/v1/trees/{tree}/places"),
            Some(json!({ "name": format!("Place {index}") })),
        )
        .await["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let repository = ok(
            app,
            Method::POST,
            &format!("/api/v1/trees/{tree}/repositories"),
            Some(json!({ "name": format!("Repository {index}") })),
        )
        .await["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let note = ok(
            app,
            Method::POST,
            &format!("/api/v1/trees/{tree}/notes"),
            Some(json!({ "text": format!("Note {index}"), "person_id": person })),
        )
        .await["id"]
            .as_str()
            .unwrap()
            .to_owned();
        for (kind, id) in [("place", place), ("repository", repository), ("note", note)] {
            let (status, body) = send(
                app,
                Method::PUT,
                &format!("/api/v1/trees/{tree}/tags/{tag_id}/records/{kind}/{id}"),
                None,
            )
            .await;
            assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
        }
    }
    (
        Uuid::parse_str(&tree).unwrap(),
        Uuid::parse_str(&tag_id).unwrap(),
    )
}

/// The statements `read` runs, counted by `counter`.
async fn statements<F, Fut>(counter: &AtomicUsize, read: F) -> usize
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    counter.store(0, Ordering::Relaxed);
    read().await;
    counter.load(Ordering::Relaxed)
}

#[tokio::test]
async fn reading_tagged_places_repositories_and_notes_costs_the_same_on_many_as_on_few() {
    let counter = Arc::new(AtomicUsize::new(0));
    let _guard = tracing_subscriber::registry()
        .with(StatementCounter(Arc::clone(&counter)))
        .set_default();

    let db: DatabaseConnection = setup_db().await;
    let app = app_on(db.clone());
    let (small, _) = tagged_tree(&app, "Tags Record Scaling Few", 1).await;
    let (large, large_tag) = tagged_tree(&app, "Tags Record Scaling Many", 30).await;
    let params = PaginationParams {
        first: 100,
        after: None,
    };

    let few = statements(&counter, || async {
        let page = PlaceRepo::list(&db, small, None, &params).await.unwrap();
        assert_eq!(page.edges.len(), 1);
        assert_eq!(page.edges[0].node.tags.len(), 1);
    })
    .await;
    let many = statements(&counter, || async {
        let page = PlaceRepo::list(&db, large, None, &params).await.unwrap();
        assert_eq!(page.edges.len(), 30);
        assert!(page.edges.iter().all(|edge| edge.node.tags.len() == 1));
    })
    .await;
    assert!(few > 0, "no statement counted for places");
    assert_eq!(
        many, few,
        "listing 30 tagged places costs {many}, one costs {few}"
    );

    let few = statements(&counter, || async {
        let page = RepositoryRepo::list(&db, small, &params).await.unwrap();
        assert!(page.edges.iter().all(|edge| edge.node.tags.len() == 1));
    })
    .await;
    let many = statements(&counter, || async {
        let page = RepositoryRepo::list(&db, large, &params).await.unwrap();
        assert_eq!(page.edges.len(), 30);
        assert!(page.edges.iter().all(|edge| edge.node.tags.len() == 1));
    })
    .await;
    assert_eq!(
        many, few,
        "listing 30 tagged repositories costs {many}, one costs {few}"
    );

    let few = statements(&counter, || async {
        let notes = NoteRepo::list_all(&db, small).await.unwrap();
        assert!(notes.iter().all(|note| note.tags.len() == 1));
    })
    .await;
    let many = statements(&counter, || async {
        let notes = NoteRepo::list_all(&db, large).await.unwrap();
        assert_eq!(notes.len(), 30);
        assert!(notes.iter().all(|note| note.tags.len() == 1));
    })
    .await;
    assert_eq!(
        many, few,
        "listing 30 tagged notes costs {many}, one costs {few}"
    );

    let few = statements(&counter, || async {
        let entries = DictionaryRepo::places_with_usage(&db, small).await.unwrap();
        assert!(entries.iter().all(|(place, _)| place.tags.len() == 1));
    })
    .await;
    let many = statements(&counter, || async {
        let entries = DictionaryRepo::places_with_usage(&db, large).await.unwrap();
        assert_eq!(entries.len(), 30);
        assert!(entries.iter().all(|(place, _)| place.tags.len() == 1));
    })
    .await;
    assert_eq!(
        many, few,
        "the place dictionary of 30 tagged places costs {many}, of one costs {few}"
    );

    // The labels of a tagged list read the names without the tags.
    let listed = tag::tagged_records(&db, large, large_tag, RecordKind::Place, &params)
        .await
        .unwrap();
    assert_eq!(listed.edges.len(), 30);
}

/// Creates a record of `path` in `tree`, with `body`, and returns its id.
async fn created(app: &Router, tree: &str, path: &str, body: serde_json::Value) -> String {
    ok(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/{path}"),
        Some(body),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// Puts tag `tag_id` on the record `kind` `id` of `tree`.
async fn tag_on(app: &Router, tree: &str, tag_id: &str, kind: &str, id: &str) {
    let (status, body) = send(
        app,
        Method::PUT,
        &format!("/api/v1/trees/{tree}/tags/{tag_id}/records/{kind}/{id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
}

/// Makes a couple of `husband` and `wife` in `tree`, and returns its id.
async fn couple_of(app: &Router, tree: &str, husband: &str, wife: &str) -> String {
    let family = created(app, tree, "families", json!({})).await;
    for (order, (member, role)) in [(husband, "husband"), (wife, "wife")]
        .into_iter()
        .enumerate()
    {
        ok(
            app,
            Method::POST,
            &format!("/api/v1/trees/{tree}/families/{family}/spouses"),
            Some(json!({ "person_id": member, "role": role, "sort_order": order })),
        )
        .await;
    }
    family
}

/// Creates the owned records of the `index`th owner of a tree: a person with
/// a spouse, an occupation of theirs with its place, the couple, a citation of
/// the occupation and a note of the person, each tagged `tag_id`.
async fn owned_records_at(app: &Router, tree: &str, tag_id: &str, source: &str, index: usize) {
    let person = new_person(app, tree).await;
    let spouse = new_person(app, tree).await;
    let place = created(
        app,
        tree,
        "places",
        json!({ "name": format!("Place {index}") }),
    )
    .await;
    let event = created(
        app,
        tree,
        "events",
        json!({
            "event_type": "occupation",
            "person_id": person,
            "place_id": place,
            "date_value": "1900",
            "description": format!("Clerk {index}"),
        }),
    )
    .await;
    let family = couple_of(app, tree, &person, &spouse).await;
    let citation = created(
        app,
        tree,
        "citations",
        json!({ "source_id": source, "event_id": event, "page": format!("folio {index}") }),
    )
    .await;
    let note = created(
        app,
        tree,
        "notes",
        json!({ "text": format!("Note {index}"), "person_id": person }),
    )
    .await;
    tag_on(app, tree, tag_id, "event", &event).await;
    tag_on(app, tree, tag_id, "family", &family).await;
    tag_on(app, tree, tag_id, "citation", &citation).await;
    tag_on(app, tree, tag_id, "note", &note).await;
}

/// A tree with `count` owned records of each kind the tag's list reads
/// through the persons they show: an occupation event with its place, a couple
/// with its two spouses, a citation of its event and a note of its person, each
/// tagged `Ledger`, a tag of that tree. Returns the tree and the tag ids.
async fn owned_records_tree(app: &Router, label: &str, count: usize) -> (Uuid, Uuid) {
    let tree = new_tree(app, label).await;
    let tag_id = created(app, &tree, "tags", json!({ "label": "Ledger" })).await;
    let source = created(app, &tree, "sources", json!({ "title": "Register" })).await;
    for index in 0..count {
        owned_records_at(app, &tree, &tag_id, &source, index).await;
    }
    (
        Uuid::parse_str(&tree).unwrap(),
        Uuid::parse_str(&tag_id).unwrap(),
    )
}

#[tokio::test]
async fn reading_tagged_events_couples_citations_and_notes_costs_the_same_on_many_as_on_few() {
    let counter = Arc::new(AtomicUsize::new(0));
    let _guard = tracing_subscriber::registry()
        .with(StatementCounter(Arc::clone(&counter)))
        .set_default();

    let db: DatabaseConnection = setup_db().await;
    let app = app_on(db.clone());
    let (small, small_tag) = owned_records_tree(&app, "Tags Owned Scaling Few", 1).await;
    let (large, large_tag) = owned_records_tree(&app, "Tags Owned Scaling Many", 30).await;
    let params = PaginationParams {
        first: 100,
        after: None,
    };

    for kind in [
        RecordKind::Event,
        RecordKind::Family,
        RecordKind::Citation,
        RecordKind::Note,
    ] {
        let few = statements(&counter, || async {
            let page = tag::tagged_records(&db, small, small_tag, kind, &params)
                .await
                .unwrap();
            assert_eq!(page.edges.len(), 1, "{kind:?}");
            assert!(
                page.edges[0]
                    .node
                    .persons
                    .as_ref()
                    .is_some_and(|persons| !persons.is_empty()),
                "{kind:?} shows its persons"
            );
        })
        .await;
        let many = statements(&counter, || async {
            let page = tag::tagged_records(&db, large, large_tag, kind, &params)
                .await
                .unwrap();
            assert_eq!(page.edges.len(), 30, "{kind:?}");
            assert!(
                page.edges.iter().all(|edge| edge
                    .node
                    .persons
                    .as_ref()
                    .is_some_and(|persons| !persons.is_empty())),
                "{kind:?} shows its persons"
            );
        })
        .await;
        assert!(few > 0, "no statement counted for {kind:?}");
        assert_eq!(
            many, few,
            "listing 30 tagged {kind:?} records costs {many}, one costs {few}"
        );
    }
}
