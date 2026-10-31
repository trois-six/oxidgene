//! The tree cache (docs/architecture.md §4.1) never answers with data older
//! than what the database holds: every write that changes a tree's places
//! is seen by the next statistics, anomalies and list of places not located,
//! whether this process made it or another one did — another API process
//! sharing the database, which a second application state over the same
//! database stands for, here and on PostgreSQL under `just test-postgres`.

use crate::common;

use std::sync::Arc;

use axum::Router;
use axum::http::Method;
use common::{new_person, new_tree, offline, ok, setup_db, test_media_root};
use oxidgene_api::tree_cache::CacheSettings;
use oxidgene_api::{AppState, build_router};
use oxidgene_db::sea_orm::DatabaseConnection;
use serde_json::{Value, json};

/// Two API processes over one database: their states, each with its own
/// cache, and their routers.
fn two_processes(db: &DatabaseConnection) -> [(AppState, Router); 2] {
    [(), ()].map(|()| {
        let state = offline(AppState::new(db.clone(), test_media_root()))
            .with_cache(CacheSettings::default());
        let router = build_router(state.clone());
        (state, router)
    })
}

/// A place named `name` in `tree`, used by a birth of a new person; its id.
async fn used_place(app: &Router, tree: &str, name: &str) -> String {
    let place = ok(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/places"),
        Some(json!({ "name": name })),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let person = new_person(app, tree).await;
    ok(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/events"),
        Some(json!({
            "event_type": "birth",
            "date_value": "1900",
            "person_id": person,
            "place_id": place,
        })),
    )
    .await;
    place
}

/// The names of the places of `tree` not located, as `app` answers.
async fn unlocated(app: &Router, tree: &str) -> Vec<String> {
    let mut names: Vec<String> = ok(
        app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/unlocated-places"),
        None,
    )
    .await
    .as_array()
    .unwrap()
    .iter()
    .map(|p| p["name"].as_str().unwrap().to_owned())
    .collect();
    names.sort();
    names
}

async fn statistics(app: &Router, tree: &str) -> Value {
    ok(
        app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/statistics?lang=en"),
        None,
    )
    .await
}

/// Creating, renaming, locating by coordinates, merging and deleting places
/// change what the next reads locate, in the process that wrote and in the
/// other one, though both keep the dictionary's readings of the labels.
#[tokio::test]
async fn every_place_write_is_seen_by_the_next_reads_of_either_process() {
    let db = setup_db().await;
    let [(state, app), (other_state, other)] = two_processes(&db);
    let tree = new_tree(&app, "Cached places").await;
    let farm = used_place(&app, &tree, "Farm-A").await;
    let hamlet = used_place(&app, &tree, "Hamlet-B").await;
    for reader in [&app, &other] {
        assert_eq!(unlocated(reader, &tree).await, ["Farm-A", "Hamlet-B"]);
        assert_eq!(statistics(reader, &tree).await["unlocated_places"], 2);
    }
    let tree_id: uuid::Uuid = tree.parse().unwrap();
    assert!(state.profiles.cache().cached_trees().contains(&tree_id));
    assert!(
        other_state
            .profiles
            .cache()
            .cached_trees()
            .contains(&tree_id)
    );

    // Renamed to a dictionary label in one process: located in both.
    ok(
        &other,
        Method::PUT,
        &format!("/api/v1/trees/{tree}/places/{farm}"),
        Some(json!({ "name": "Brest, 29019, Finistère, Bretagne, France" })),
    )
    .await;
    for reader in [&app, &other] {
        assert_eq!(unlocated(reader, &tree).await, ["Hamlet-B"]);
        let stats = statistics(reader, &tree).await;
        assert_eq!(stats["unlocated_places"], 1);
        assert_eq!(stats["births_by_country"][0]["label"], "France");
    }

    // Given coordinates of its own: located, its label unknown as before.
    ok(
        &app,
        Method::PUT,
        &format!("/api/v1/trees/{tree}/places/{hamlet}"),
        Some(json!({ "latitude": 47.0, "longitude": -2.0 })),
    )
    .await;
    for reader in [&app, &other] {
        assert!(unlocated(reader, &tree).await.is_empty());
    }

    // A new place, then merged into the located one.
    let new = used_place(&other, &tree, "Field-C").await;
    for reader in [&app, &other] {
        assert_eq!(unlocated(reader, &tree).await, ["Field-C"]);
    }
    ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/places/{new}/merge"),
        Some(json!({
            "into": farm,
            "name": "Brest, 29019, Finistère, Bretagne, France",
        })),
    )
    .await;
    for reader in [&app, &other] {
        assert!(unlocated(reader, &tree).await.is_empty());
        assert_eq!(statistics(reader, &tree).await["places"], 2);
    }
}

/// A tree read by the desktop drops the previous tree's data, and its
/// memory is handed back.
#[tokio::test]
async fn the_desktop_keeps_one_tree_at_a_time() {
    let db = setup_db().await;
    let state = offline(AppState::new(db.clone(), test_media_root()))
        .with_cache(CacheSettings::one_tree(std::time::Duration::from_secs(300)));
    let app = build_router(state.clone());
    let (a, b) = (
        new_tree(&app, "Tree A").await,
        new_tree(&app, "Tree B").await,
    );
    used_place(&app, &a, "Farm-A").await;
    used_place(&app, &b, "Farm-B").await;
    let cache = Arc::clone(state.profiles.cache());

    unlocated(&app, &a).await;
    assert_eq!(cache.cached_trees(), [a.parse::<uuid::Uuid>().unwrap()]);
    let releases = cache.releases();
    statistics(&app, &b).await;
    assert_eq!(cache.cached_trees(), [b.parse::<uuid::Uuid>().unwrap()]);
    assert_eq!(cache.releases(), releases + 1);
}
