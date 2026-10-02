//! Change history over REST and GraphQL: the audit log every write leaves,
//! the states writes replace — never a copy of the live record — and
//! restoring an earlier one.
//!
//! Every scenario ends by checking the history's invariants against the
//! database itself: see [`assert_no_duplicated_state`].
//!
//! All data is fictitious.

mod common;

use axum::http::{Method, StatusCode};
use oxidgene_core::history::{RecordSnapshot, RecordType};
use oxidgene_db::repo::{PersonRepo, SnapshotRepo, TreeRepo};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde_json::{Value, json};
use uuid::Uuid;

use common::{app_on, family_blocks_gedcom, ok, send, setup_db};

async fn setup() -> (DatabaseConnection, axum::Router) {
    let db = setup_db().await;
    (db.clone(), app_on(db))
}

async fn graphql(app: &axum::Router, query: &str, variables: Value) -> Value {
    let body = json!({ "query": query, "variables": variables });
    let (status, json) = send(app, Method::POST, "/graphql", Some(body)).await;
    assert_eq!(status, StatusCode::OK);
    if let Some(errors) = json.get("errors") {
        panic!("GraphQL errors: {errors}");
    }
    json["data"].clone()
}

async fn create_tree(app: &axum::Router) -> String {
    let tree = ok(
        app,
        Method::POST,
        "/api/v1/trees",
        Some(json!({ "name": "History Fixture" })),
    )
    .await;
    tree["id"].as_str().unwrap().to_string()
}

async fn create_person(app: &axum::Router, tree: &str, given: &str, surname: &str) -> String {
    let person = ok(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/persons"),
        Some(json!({ "sex": "unknown" })),
    )
    .await;
    let id = person["id"].as_str().unwrap().to_string();
    ok(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/persons/{id}/names"),
        Some(json!({
            "name_type": "birth",
            "given_names": given,
            "surname": surname,
            "is_primary": true,
        })),
    )
    .await;
    id
}

/// A record's versions over REST, latest first — checked against the same
/// read over GraphQL, so both surfaces present the history alike.
async fn versions(app: &axum::Router, tree: &str, record_type: &str, id: &str) -> Vec<Value> {
    let page = ok(
        app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/history/{record_type}/{id}?first=100"),
        None,
    )
    .await;
    let rest: Vec<Value> = page["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|edge| edge["node"].clone())
        .collect();
    let data = graphql(
        app,
        "query($t: ID!, $r: GqlRecordType!, $id: ID!) {
            recordVersions(treeId: $t, recordType: $r, recordId: $id, first: 100) {
                totalCount
                edges { node { id version current deleted entry { id } snapshot { recordType } } }
            }
        }",
        json!({ "t": tree, "r": record_type.to_uppercase(), "id": id }),
    )
    .await;
    let gql = &data["recordVersions"];
    assert_eq!(gql["totalCount"], page["total_count"]);
    let gql: Vec<&Value> = gql["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|edge| &edge["node"])
        .collect();
    assert_eq!(gql.len(), rest.len());
    for (rest, gql) in rest.iter().zip(gql) {
        assert_eq!(rest["id"], gql["id"]);
        assert_eq!(rest["version"], gql["version"]);
        assert_eq!(rest["current"], gql["current"]);
        assert_eq!(rest["deleted"], gql["deleted"]);
        assert_eq!(rest["entry"]["id"], gql["entry"]["id"]);
        assert_eq!(rest["snapshot"].is_null(), gql["snapshot"].is_null());
    }
    rest
}

async fn audit(app: &axum::Router, tree: &str, query: &str) -> Vec<Value> {
    let page = ok(
        app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/audit?first=100{query}"),
        None,
    )
    .await;
    page["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|edge| edge["node"].clone())
        .collect()
}

/// The changes of an audit entry, over REST.
async fn changes(app: &axum::Router, tree: &str, entry: &Value) -> Vec<Value> {
    let page = ok(
        app,
        Method::GET,
        &format!(
            "/api/v1/trees/{tree}/audit/{}/changes",
            entry["id"].as_str().unwrap()
        ),
        None,
    )
    .await;
    page["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|edge| edge["node"].clone())
        .collect()
}

async fn scalar(db: &DatabaseConnection, sql: &str) -> i64 {
    db.query_one_raw(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .unwrap()
        .unwrap()
        .try_get::<i64>("", "n")
        .unwrap()
}

/// How many states the history stores, and the bytes of their JSON.
async fn stored(db: &DatabaseConnection) -> (i64, i64) {
    (
        scalar(db, "SELECT COUNT(*) AS n FROM record_version").await,
        scalar(
            db,
            "SELECT COALESCE(SUM(LENGTH(snapshot) + LENGTH(labels)), 0) AS n FROM record_version",
        )
        .await,
    )
}

/// The history's invariants, read from the database:
///
/// - the latest stored state of a record that is live and not deleted is
///   never a copy of it, and never a deletion marker;
/// - a deletion marker is only ever a record's latest state, and the record
///   is then soft-deleted;
/// - a deleted state stores no snapshot.
async fn assert_no_duplicated_state(db: &DatabaseConnection) {
    let rows = db
        .query_all_raw(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT v.tree_id, v.record_type, v.record_id, v.deleted, v.snapshot, \
                    v.version = (SELECT MAX(l.version) FROM record_version l \
                                 WHERE l.record_type = v.record_type \
                                 AND l.record_id = v.record_id) AS latest \
             FROM record_version v"
                .to_string(),
        ))
        .await
        .unwrap();
    for row in rows {
        let tree_id: Uuid = row.try_get("", "tree_id").unwrap();
        let record_type: RecordType = row
            .try_get::<String>("", "record_type")
            .unwrap()
            .parse()
            .unwrap();
        let record_id: Uuid = row.try_get("", "record_id").unwrap();
        let deleted: bool = row.try_get("", "deleted").unwrap();
        let snapshot: Option<String> = row.try_get("", "snapshot").unwrap();
        let latest: bool = row.try_get("", "latest").unwrap();
        if deleted {
            assert!(snapshot.is_none(), "a deleted state stores no snapshot");
            continue;
        }
        if !latest && snapshot.is_none() {
            panic!("a deletion marker of {record_id} is not its latest state");
        }
        if !latest {
            continue;
        }
        let live = SnapshotRepo::build(db, tree_id, record_type, &[record_id])
            .await
            .unwrap()
            .pop();
        match (live, snapshot) {
            (Some(live), Some(stored)) if !live.deleted => {
                let stored: RecordSnapshot = serde_json::from_str(&stored).unwrap();
                assert_ne!(stored, live.snapshot, "{record_id}'s latest state is live");
            }
            (Some(live), None) => {
                assert!(
                    live.deleted,
                    "a marker of {record_id}, which is not deleted"
                );
            }
            (None, None) => panic!("a marker of {record_id}, which is gone"),
            _ => {}
        }
    }
}

#[tokio::test]
async fn every_person_write_is_audited_and_stores_the_state_it_replaced() {
    let (db, app) = setup().await;
    let tree = create_tree(&app).await;
    let person = create_person(&app, &tree, "Alpha", "Fixture").await;

    // Created without a name, then named: the unnamed state is stored, and
    // the named one is the live record.
    let history = versions(&app, &tree, "person", &person).await;
    assert_eq!(history.len(), 2);
    assert_eq!(history[0]["version"], 2);
    assert_eq!(history[0]["current"], true);
    assert_eq!(history[0]["id"], person.as_str());
    assert_eq!(history[0]["entry"]["action"], "create");
    assert_eq!(history[0]["entry"]["entity"], "person_name");
    assert_eq!(history[0]["snapshot"]["type"], "person");
    assert_eq!(history[0]["snapshot"]["names"][0]["given_names"], "Alpha");
    assert_eq!(history[1]["current"], false);
    assert!(
        history[1]["snapshot"]["names"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    // The first state came from the write that created the person.
    assert_eq!(history[1]["entry"]["entity"], "person");
    assert_eq!(history[1]["entry"]["action"], "create");

    // The audit log reads newest first and labels the person by name.
    let entries = audit(&app, &tree, "").await;
    assert_eq!(entries[0]["entity"], "person_name");
    assert_eq!(entries[0]["subject"], "person");
    assert_eq!(entries[0]["subject_id"], person.as_str());
    assert_eq!(entries[0]["label"], "Alpha Fixture");
    assert_eq!(entries[0]["version_count"], 1);
    // Creating a record replaced nothing.
    assert_eq!(entries[1]["entity"], "person");
    assert_eq!(entries[1]["version_count"], 0);
    assert_eq!(entries.last().unwrap()["entity"], "tree");
    assert_eq!(entries.last().unwrap()["category"], "settings");

    // Filters: by category and by subject.
    let settings = audit(&app, &tree, "&category=settings").await;
    assert_eq!(settings.len(), 1);
    let about = audit(&app, &tree, &format!("&subject_id={person}")).await;
    assert_eq!(about.len(), 2);
    assert_no_duplicated_state(&db).await;
}

#[tokio::test]
async fn an_edit_stores_only_the_records_it_changed() {
    let (db, app) = setup().await;
    let tree = create_tree(&app).await;
    let father = create_person(&app, &tree, "Beta", "Fixture").await;
    let child = create_person(&app, &tree, "Gamma", "Fixture").await;
    let family = ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/families"),
        None,
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/families/{family}/spouses"),
        Some(json!({ "person_id": father, "role": "husband" })),
    )
    .await;
    ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/families/{family}/children"),
        Some(json!({ "person_id": child, "child_type": "biological" })),
    )
    .await;
    let child_versions = versions(&app, &tree, "person", &child).await.len();

    // Renaming the father reaches the child's projection, not their record.
    let name_id = versions(&app, &tree, "person", &father).await[0]["snapshot"]["names"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    ok(
        &app,
        Method::PUT,
        &format!("/api/v1/trees/{tree}/persons/{father}/names/{name_id}"),
        Some(json!({ "given_names": "Delta" })),
    )
    .await;
    let entry = audit(&app, &tree, "").await[0].clone();
    assert_eq!(entry["version_count"], 1);
    assert_eq!(
        versions(&app, &tree, "person", &child).await.len(),
        child_versions
    );

    // The entry's change pairs the state it replaced with the one it
    // produced — the live record, as nothing changed the father since.
    let change = &changes(&app, &tree, &entry).await[0];
    assert_eq!(change["version"]["current"], true);
    assert_eq!(
        change["version"]["snapshot"]["names"][0]["given_names"],
        "Delta"
    );
    assert_eq!(change["previous"]["current"], false);
    assert_eq!(
        change["previous"]["snapshot"]["names"][0]["given_names"],
        "Beta"
    );
    assert_no_duplicated_state(&db).await;
}

/// An imported tree stores no state at all; each edit then stores exactly
/// the state it replaced, and the live record is the current version. Run
/// with `--no-capture` to see what the history weighs.
#[tokio::test]
async fn an_import_stores_nothing_and_edits_store_only_prior_states() {
    let (db, app) = setup().await;
    let tree = create_tree(&app).await;
    common::import_gedcom(&app, &db, &tree, &family_blocks_gedcom(20)).await;
    let imports = audit(&app, &tree, "&category=import").await;
    assert_eq!(imports.len(), 1);
    assert_eq!(imports[0]["details"]["format"], "gedcom");
    assert_eq!(imports[0]["details"]["count"], 200);
    assert_eq!(imports[0]["version_count"], 0);
    assert_eq!(stored(&db).await, (0, 0));
    println!(
        "after importing 200 persons: {:?} (versions, bytes)",
        stored(&db).await
    );

    // A record nobody changed has one version: the live one, from the import.
    let profiles = ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/profiles"),
        None,
    )
    .await;
    let anchor = profiles
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["primary_name"]["given_names"] == "Anchor")
        .unwrap()["person_id"]
        .as_str()
        .unwrap()
        .to_string();
    let history = versions(&app, &tree, "person", &anchor).await;
    assert_eq!(history.len(), 1);
    assert_eq!(history[0]["version"], 1);
    assert_eq!(history[0]["current"], true);
    assert_eq!(history[0]["entry"]["id"], imports[0]["id"]);

    // The first edit stores exactly the state it replaced.
    let name_id = history[0]["snapshot"]["names"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let imported = history[0]["snapshot"].clone();
    for (n, given) in ["Anchor_1", "Anchor_2", "Anchor_3"].iter().enumerate() {
        ok(
            &app,
            Method::PUT,
            &format!("/api/v1/trees/{tree}/persons/{anchor}/names/{name_id}"),
            Some(json!({ "given_names": given })),
        )
        .await;
        assert_eq!(stored(&db).await.0, n as i64 + 1);
        assert_no_duplicated_state(&db).await;
    }

    // Three edits, three prior states, and the live record on top.
    let history = versions(&app, &tree, "person", &anchor).await;
    let given: Vec<&str> = history
        .iter()
        .map(|v| v["snapshot"]["names"][0]["given_names"].as_str().unwrap())
        .collect();
    assert_eq!(given, ["Anchor_3", "Anchor_2", "Anchor_1", "Anchor"]);
    assert_eq!(history[3]["snapshot"], imported);
    assert_eq!(history[3]["entry"]["id"], imports[0]["id"]);
    assert_eq!(history[2]["entry"]["action"], "update");
    println!(
        "after three edits: {:?} (versions, bytes)",
        stored(&db).await
    );

    // Any two versions compare, the current one included: version 2 against
    // version 4 over GraphQL reads each side as stored and as live.
    let data = graphql(
        &app,
        "query($t: ID!, $p: ID!) {
            old: recordVersion(treeId: $t, recordType: PERSON, recordId: $p, version: 2) {
                current snapshot { person { names { givenNames } } }
            }
            now: recordVersion(treeId: $t, recordType: PERSON, recordId: $p, version: 4) {
                current snapshot { person { names { givenNames } } }
            }
        }",
        json!({ "t": tree, "p": anchor }),
    )
    .await;
    assert_eq!(data["old"]["current"], false);
    assert_eq!(
        data["old"]["snapshot"]["person"]["names"][0]["givenNames"],
        "Anchor_1"
    );
    assert_eq!(data["now"]["current"], true);
    assert_eq!(
        data["now"]["snapshot"]["person"]["names"][0]["givenNames"],
        "Anchor_3"
    );
    // A number past the current one is not found, on both surfaces.
    let (status, _) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/history/person/{anchor}/5"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let response = common::gql(
        &app,
        "query($t: ID!, $p: ID!) {
            recordVersion(treeId: $t, recordType: PERSON, recordId: $p, version: 5) { version }
        }",
        json!({ "t": tree, "p": anchor }),
    )
    .await;
    assert!(response.get("errors").is_some(), "{response}");
}

/// A family edit stores the state of every spouse and child it changes,
/// as they were before it.
#[tokio::test]
async fn a_family_edit_stores_the_prior_states_of_the_persons_it_links() {
    let (db, app) = setup().await;
    let tree = create_tree(&app).await;
    common::import_gedcom(&app, &db, &tree, &family_blocks_gedcom(1)).await;
    let profiles = ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/profiles"),
        None,
    )
    .await;
    let anchor = profiles
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["primary_name"]["given_names"] == "Anchor")
        .unwrap();
    // The anchor's own union: him and his wife, no child yet.
    let union = &anchor["families_as_spouse"][0];
    let family = union["family_id"].as_str().unwrap().to_string();
    let husband = anchor["person_id"].as_str().unwrap().to_string();
    let wife = union["spouse_id"].as_str().unwrap().to_string();
    let child = create_person(&app, &tree, "Epsilon", "Fixture").await;
    let before = stored(&db).await.0;

    ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/families/{family}/children"),
        Some(json!({ "person_id": child, "child_type": "biological" })),
    )
    .await;
    let entry = audit(&app, &tree, "").await[0].clone();
    assert_eq!(entry["entity"], "family_child");
    assert_eq!(entry["version_count"], 3);
    assert_eq!(stored(&db).await.0, before + 3);

    // Each change: the union or the parents without the child, beside the
    // live record with it.
    let changes = changes(&app, &tree, &entry).await;
    let mut changed: Vec<&str> = changes
        .iter()
        .map(|c| c["version"]["record_id"].as_str().unwrap())
        .collect();
    changed.sort();
    let mut expected = vec![husband.as_str(), wife.as_str(), child.as_str()];
    expected.sort();
    assert_eq!(changed, expected);
    for change in &changes {
        assert_eq!(change["version"]["current"], true);
        let id = change["version"]["record_id"].as_str().unwrap();
        let (before, after) = (
            &change["previous"]["snapshot"],
            &change["version"]["snapshot"],
        );
        if id == child {
            assert!(before["parents"].as_array().unwrap().is_empty());
            assert_eq!(after["parents"][0]["family_id"], family.as_str());
        } else {
            let union = |s: &Value| {
                s["unions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|u| u["family_id"] == family.as_str())
                    .unwrap()["children"]
                    .as_array()
                    .unwrap()
                    .len()
            };
            assert_eq!(union(before), 0);
            assert_eq!(union(after), 1);
        }
    }
    assert_no_duplicated_state(&db).await;
}

/// A deletion stores a marker reading the soft-deleted rows; restoring the
/// state before it undeletes the person, and the deleted state stays
/// readable between them.
#[tokio::test]
async fn a_deletion_stores_a_marker_and_a_restore_undeletes() {
    let (db, app) = setup().await;
    let tree = create_tree(&app).await;
    let person = create_person(&app, &tree, "Zeta", "Fixture").await;
    // UUIDs are stored as 16-byte blobs.
    let person_hex = person.replace('-', "");
    ok(
        &app,
        Method::DELETE,
        &format!("/api/v1/trees/{tree}/persons/{person}"),
        None,
    )
    .await;

    // Stored: the unnamed state, then the marker — no snapshot, no labels.
    let entry = audit(&app, &tree, "").await[0].clone();
    assert_eq!(entry["action"], "delete");
    assert_eq!(entry["version_count"], 1);
    let marker = scalar(
        &db,
        &format!(
            "SELECT COUNT(*) AS n FROM record_version WHERE lower(hex(record_id)) = '{person_hex}' \
             AND version = 2 AND NOT deleted AND snapshot IS NULL AND labels IS NULL"
        ),
    )
    .await;
    assert_eq!(marker, 1);
    assert_no_duplicated_state(&db).await;

    // Presented, the marker reads as the named state the rows still hold,
    // and the current version is the deletion, without content.
    let history = versions(&app, &tree, "person", &person).await;
    assert_eq!(history.len(), 3);
    assert_eq!(history[0]["current"], true);
    assert_eq!(history[0]["deleted"], true);
    assert!(history[0]["snapshot"].is_null());
    assert_eq!(history[0]["entry"]["action"], "delete");
    assert_eq!(history[1]["deleted"], false);
    assert_eq!(history[1]["snapshot"]["names"][0]["given_names"], "Zeta");

    // The deletion's change: the named state, beside the deletion.
    let change = &changes(&app, &tree, &entry).await[0];
    assert_eq!(change["previous"]["version"], 2);
    assert_eq!(change["version"]["deleted"], true);

    // A deleted state is not something to restore, on either surface.
    let (status, _) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/history/person/{person}/revert"),
        Some(json!({ "version": 3 })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let response = common::gql(
        &app,
        "mutation($t: ID!, $p: ID!) {
            revertRecord(treeId: $t, recordType: PERSON, recordId: $p, version: 3) { id }
        }",
        json!({ "t": tree, "p": person }),
    )
    .await;
    assert!(response.get("errors").is_some(), "{response}");

    // Restoring the state the deletion replaced undeletes the person.
    let data = graphql(
        &app,
        "mutation($t: ID!, $p: ID!) {
            revertRecord(treeId: $t, recordType: PERSON, recordId: $p, version: 2) {
                action details { version } versionCount
            }
        }",
        json!({ "t": tree, "p": person }),
    )
    .await;
    assert_eq!(data["revertRecord"]["action"], "REVERT");
    assert_eq!(data["revertRecord"]["details"]["version"], 2);
    assert_eq!(data["revertRecord"]["versionCount"], 1);
    let profile = ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/profiles/{person}"),
        None,
    )
    .await;
    assert_eq!(profile["primary_name"]["given_names"], "Zeta");

    // The marker now holds its state, and the deleted state sits between it
    // and the live record.
    let filled = scalar(
        &db,
        &format!(
            "SELECT COUNT(*) AS n FROM record_version WHERE lower(hex(record_id)) = '{person_hex}' \
             AND version = 2 AND snapshot IS NOT NULL"
        ),
    )
    .await;
    assert_eq!(filled, 1);
    let history = versions(&app, &tree, "person", &person).await;
    assert_eq!(history.len(), 4);
    assert_eq!(history[0]["current"], true);
    assert_eq!(history[0]["deleted"], false);
    assert_eq!(history[0]["entry"]["action"], "revert");
    assert_eq!(history[1]["deleted"], true);
    assert!(history[1]["snapshot"].is_null());
    assert_eq!(history[1]["entry"]["action"], "delete");
    assert_eq!(history[0]["snapshot"], history[2]["snapshot"]);
    assert_no_duplicated_state(&db).await;

    // Deleted again then restored to the first, unnamed state: across the
    // deletion and the undelete, every earlier state still reads.
    ok(
        &app,
        Method::DELETE,
        &format!("/api/v1/trees/{tree}/persons/{person}"),
        None,
    )
    .await;
    ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/history/person/{person}/revert"),
        Some(json!({ "version": 1 })),
    )
    .await;
    let history = versions(&app, &tree, "person", &person).await;
    let shape: Vec<(bool, bool)> = history
        .iter()
        .map(|v| (v["deleted"].as_bool().unwrap(), v["snapshot"].is_null()))
        .collect();
    assert_eq!(
        shape,
        [
            (false, false), // 6: live, unnamed again
            (true, true),   // 5: deleted
            (false, false), // 4: named, before the second deletion
            (true, true),   // 3: deleted
            (false, false), // 2: named
            (false, false), // 1: unnamed
        ]
    );
    assert!(
        history[0]["snapshot"]["names"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_no_duplicated_state(&db).await;
}

#[tokio::test]
async fn reverting_a_person_restores_names_events_and_deletion() {
    let (db, app) = setup().await;
    let tree = create_tree(&app).await;
    let person = create_person(&app, &tree, "Epsilon", "Fixture").await;
    let place = ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/places"),
        Some(json!({ "name": "Springfield, Fictland" })),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/events"),
        Some(json!({
            "event_type": "birth",
            "date_value": "1 JAN 1900",
            "place_id": place,
            "person_id": person,
        })),
    )
    .await;
    let with_birth = versions(&app, &tree, "person", &person).await[0]["version"]
        .as_i64()
        .unwrap();

    // Rename, then delete the place and the person.
    let name_id = versions(&app, &tree, "person", &person).await[0]["snapshot"]["names"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    ok(
        &app,
        Method::PUT,
        &format!("/api/v1/trees/{tree}/persons/{person}/names/{name_id}"),
        Some(json!({ "given_names": "Zeta" })),
    )
    .await;
    ok(
        &app,
        Method::DELETE,
        &format!("/api/v1/trees/{tree}/places/{place}"),
        None,
    )
    .await;
    // A hard-deleted place keeps its last state as a real snapshot.
    let place_history = versions(&app, &tree, "place", &place).await;
    assert_eq!(place_history[0]["deleted"], true);
    assert_eq!(
        place_history[1]["snapshot"]["name"],
        "Springfield, Fictland"
    );
    ok(
        &app,
        Method::DELETE,
        &format!("/api/v1/trees/{tree}/persons/{person}"),
        None,
    )
    .await;
    let latest = &versions(&app, &tree, "person", &person).await[0];
    assert_eq!(latest["deleted"], true);

    let entry = ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/history/person/{person}/revert"),
        Some(json!({ "version": with_birth })),
    )
    .await;
    assert_eq!(entry["action"], "revert");
    assert_eq!(entry["category"], "history");
    assert_eq!(entry["details"]["version"], with_birth);

    // The person is back, under their old name, born where they were born.
    ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/persons/{person}"),
        None,
    )
    .await;
    let profile = ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/profiles/{person}"),
        None,
    )
    .await;
    assert_eq!(profile["primary_name"]["given_names"], "Epsilon");
    let restored_place = ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/places/{place}"),
        None,
    )
    .await;
    assert_eq!(restored_place["name"], "Springfield, Fictland");
    // The re-created place's history shows the gap it spent deleted.
    let place_history = versions(&app, &tree, "place", &place).await;
    assert_eq!(place_history[0]["current"], true);
    assert_eq!(place_history[0]["deleted"], false);
    assert_eq!(place_history[1]["deleted"], true);

    // The live record equals the version restored.
    let history = versions(&app, &tree, "person", &person).await;
    let restored = history.iter().find(|v| v["version"] == with_birth).unwrap();
    assert_eq!(history[0]["current"], true);
    assert_eq!(history[0]["deleted"], false);
    assert_eq!(history[0]["snapshot"], restored["snapshot"]);
    assert_no_duplicated_state(&db).await;
}

/// An event's age and agency and a source's agency are part of the versioned
/// state: a version holds them and a revert brings them back.
#[tokio::test]
async fn event_ages_and_agencies_are_versioned() {
    let (_db, app) = setup().await;
    let tree = create_tree(&app).await;
    let person = create_person(&app, &tree, "Eta", "Fixture").await;
    let event = ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/events"),
        Some(json!({
            "event_type": "death",
            "person_id": person,
            "age": "34y",
            "agency": "Parish of Northwick",
        })),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let history = versions(&app, &tree, "person", &person).await;
    let with_age = history[0]["version"].as_i64().unwrap();
    let snapshot_event = &history[0]["snapshot"]["events"][0];
    assert_eq!(snapshot_event["age"], "34y");
    assert_eq!(snapshot_event["agency"], "Parish of Northwick");

    let uri = format!("/api/v1/trees/{tree}/events/{event}");
    ok(
        &app,
        Method::PUT,
        &uri,
        Some(json!({ "age": null, "agency": null })),
    )
    .await;
    assert!(
        versions(&app, &tree, "person", &person).await[0]["snapshot"]["events"][0]["age"].is_null()
    );
    ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/history/person/{person}/revert"),
        Some(json!({ "version": with_age })),
    )
    .await;
    let restored = ok(&app, Method::GET, &uri, None).await;
    assert_eq!(restored["age"], "34y");
    assert_eq!(restored["agency"], "Parish of Northwick");

    let source = ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/sources"),
        Some(json!({ "title": "Register", "agency": "Sample archives" })),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let source_history = versions(&app, &tree, "source", &source).await;
    assert_eq!(source_history[0]["snapshot"]["agency"], "Sample archives");
}

/// A family event's spouse ages are part of each spouse's versioned union,
/// and a revert brings them back.
#[tokio::test]
async fn spouse_ages_are_versioned() {
    let (_db, app) = setup().await;
    let tree = create_tree(&app).await;
    let husband = create_person(&app, &tree, "Theta", "Fixture").await;
    let wife = create_person(&app, &tree, "Iota", "Fixture").await;
    let family = ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/families"),
        None,
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    for (person, role) in [(&husband, "husband"), (&wife, "wife")] {
        ok(
            &app,
            Method::POST,
            &format!("/api/v1/trees/{tree}/families/{family}/spouses"),
            Some(json!({ "person_id": person, "role": role })),
        )
        .await;
    }
    let event = ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/events"),
        Some(json!({
            "event_type": "marriage",
            "family_id": family,
            "spouse_ages": [{ "person_id": husband, "age": "30y" }],
        })),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let history = versions(&app, &tree, "person", &husband).await;
    let with_ages = history[0]["version"].as_i64().unwrap();
    let ages = &history[0]["snapshot"]["unions"][0]["events"][0]["spouse_ages"];
    assert_eq!(ages[0]["person_id"], husband.as_str());
    assert_eq!(ages[0]["age"], "30y");

    let uri = format!("/api/v1/trees/{tree}/events/{event}");
    ok(&app, Method::PUT, &uri, Some(json!({ "spouse_ages": [] }))).await;
    ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/history/person/{husband}/revert"),
        Some(json!({ "version": with_ages })),
    )
    .await;
    let restored = ok(&app, Method::GET, &uri, None).await;
    assert_eq!(
        restored["spouse_ages"],
        json!([{ "person_id": husband, "age": "30y" }])
    );
}

#[tokio::test]
async fn reverting_a_spouse_restores_the_union() {
    let (db, app) = setup().await;
    let tree = create_tree(&app).await;
    let husband = create_person(&app, &tree, "Eta", "Fixture").await;
    let wife = create_person(&app, &tree, "Theta", "Sample").await;
    let family = ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/families"),
        None,
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    for (person, role) in [(&husband, "husband"), (&wife, "wife")] {
        ok(
            &app,
            Method::POST,
            &format!("/api/v1/trees/{tree}/families/{family}/spouses"),
            Some(json!({ "person_id": person, "role": role })),
        )
        .await;
    }
    let married = versions(&app, &tree, "person", &husband).await[0]["version"]
        .as_i64()
        .unwrap();
    // The spouse is named by ID; their name travels in the labels.
    assert!(
        versions(&app, &tree, "person", &husband).await[0]["labels"]
            .as_array()
            .unwrap()
            .iter()
            .any(|label| label["label"] == "Theta Sample")
    );

    // A marriage recorded on the family stores both spouses' prior states.
    ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/events"),
        Some(json!({ "event_type": "marriage", "family_id": family })),
    )
    .await;
    let marriage = audit(&app, &tree, "").await[0].clone();
    assert_eq!(marriage["version_count"], 2);
    assert_eq!(marriage["subject"], "family");
    assert_eq!(marriage["details"]["event_type"], "marriage");

    // Deleting the family, then restoring the husband's married state,
    // brings back the family and both spouse links, without the marriage.
    ok(
        &app,
        Method::DELETE,
        &format!("/api/v1/trees/{tree}/families/{family}"),
        None,
    )
    .await;
    ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/history/person/{husband}/revert"),
        Some(json!({ "version": married })),
    )
    .await;
    let spouses = ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/families/{family}/spouses"),
        None,
    )
    .await;
    assert_eq!(spouses.as_array().unwrap().len(), 2);
    let events = ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/events?family_id={family}"),
        None,
    )
    .await;
    assert_eq!(events["total_count"], 0);
    let profile = ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/profiles/{wife}"),
        None,
    )
    .await;
    assert_eq!(
        profile["families_as_spouse"][0]["family_id"],
        family.as_str()
    );
    // The restore changed the wife too: her state before it is stored.
    let restore = audit(&app, &tree, "").await[0].clone();
    assert_eq!(restore["version_count"], 2);
    assert_no_duplicated_state(&db).await;
}

#[tokio::test]
async fn settings_media_and_exports_are_audited() {
    let (db, app) = setup().await;
    let tree = create_tree(&app).await;
    ok(
        &app,
        Method::PUT,
        &format!("/api/v1/trees/{tree}"),
        Some(json!({ "name": "Renamed Fixture" })),
    )
    .await;
    let tree_versions = versions(&app, &tree, "tree", &tree).await;
    assert_eq!(tree_versions.len(), 2);
    assert_eq!(tree_versions[0]["snapshot"]["name"], "Renamed Fixture");
    assert_eq!(tree_versions[1]["snapshot"]["name"], "History Fixture");

    // Restoring the settings brings the first name back.
    ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/history/tree/{tree}/revert"),
        Some(json!({ "version": 1 })),
    )
    .await;
    let restored = ok(&app, Method::GET, &format!("/api/v1/trees/{tree}"), None).await;
    assert_eq!(restored["name"], "History Fixture");

    // A media write is audited under media, and stores nothing.
    let document = ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/media/document"),
        Some(json!({ "title": "Fixture document" })),
    )
    .await;
    let media = audit(&app, &tree, "&category=media").await;
    assert_eq!(media.len(), 1);
    assert_eq!(media[0]["subject_id"], document["id"]);
    assert_eq!(media[0]["label"], "Fixture document");
    assert_eq!(media[0]["version_count"], 0);

    // An export leaves an entry naming its format, and stores nothing.
    ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/gedcom/export"),
        None,
    )
    .await;
    let exports = audit(&app, &tree, "&category=export").await;
    assert_eq!(exports.len(), 1);
    assert_eq!(exports[0]["details"]["format"], "gedcom");
    assert_eq!(exports[0]["subject"], "tree");
    assert_eq!(exports[0]["version_count"], 0);
    assert_no_duplicated_state(&db).await;
}

/// The row count the planner statistics record for `table`, if any.
async fn analyzed_rows(db: &DatabaseConnection, table: &str) -> Option<i64> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT stat FROM sqlite_stat1 WHERE tbl = ? AND idx IS NOT NULL",
            [table.into()],
        ))
        .await
        .unwrap()?;
    let stat: String = row.try_get("", "stat").unwrap();
    stat.split_whitespace().next()?.parse().ok()
}

/// The projections are an import's last bulk write — its history entry
/// stores nothing — so the planner statistics are gathered after them.
#[tokio::test]
async fn statistics_include_what_an_import_writes() {
    let (db, app) = setup().await;
    let tree = create_tree(&app).await;
    common::import_gedcom(&app, &db, &tree, &family_blocks_gedcom(2)).await;
    for table in ["person", "person_denorm"] {
        let rows = scalar(&db, &format!("SELECT COUNT(*) AS n FROM {table}")).await;
        assert_eq!(rows, 20, "{table}");
        assert_eq!(analyzed_rows(&db, table).await, Some(rows), "{table}");
    }
}

#[tokio::test]
async fn graphql_mirrors_the_history_surface() {
    let (db, app) = setup().await;
    let tree = create_tree(&app).await;
    let person = create_person(&app, &tree, "Mu", "Fixture").await;

    // A GraphQL write is audited like a REST one.
    graphql(
        &app,
        "mutation($t: ID!, $p: ID!) { updatePerson(treeId: $t, id: $p, input: { sex: MALE }) { id } }",
        json!({ "t": tree, "p": person }),
    )
    .await;

    let data = graphql(
        &app,
        "query($t: ID!, $s: ID!) {
            auditEntries(treeId: $t, subjectId: $s) {
                totalCount
                edges { node { id action entity category label versionCount } }
            }
        }",
        json!({ "t": tree, "s": person }),
    )
    .await;
    let entries = &data["auditEntries"];
    assert_eq!(entries["totalCount"], 3);
    let latest = &entries["edges"][0]["node"];
    assert_eq!(latest["action"], "UPDATE");
    assert_eq!(latest["entity"], "PERSON");
    assert_eq!(latest["category"], "DATA");
    assert_eq!(latest["label"], "Mu Fixture");
    assert_eq!(latest["versionCount"], 1);

    let data = graphql(
        &app,
        "query($t: ID!, $p: ID!) {
            recordVersions(treeId: $t, recordType: PERSON, recordId: $p) {
                totalCount
                edges { node { version current entry { action } snapshot { recordType person { sex names { givenNames } } } } }
            }
            recordVersion(treeId: $t, recordType: PERSON, recordId: $p, version: 2) {
                current snapshot { person { sex } }
            }
        }",
        json!({ "t": tree, "p": person }),
    )
    .await;
    assert_eq!(data["recordVersions"]["totalCount"], 3);
    let node = &data["recordVersions"]["edges"][0]["node"];
    assert_eq!(node["version"], 3);
    assert_eq!(node["current"], true);
    assert_eq!(node["entry"]["action"], "UPDATE");
    assert_eq!(node["snapshot"]["recordType"], "PERSON");
    assert_eq!(node["snapshot"]["person"]["sex"], "MALE");
    assert_eq!(data["recordVersion"]["current"], false);
    assert_eq!(
        data["recordVersion"]["snapshot"]["person"]["sex"],
        "UNKNOWN"
    );

    let entry_id = latest["id"].as_str().unwrap();
    let data = graphql(
        &app,
        "query($t: ID!, $e: ID!) {
            auditEntry(treeId: $t, id: $e) { action }
            auditEntryChanges(treeId: $t, entryId: $e) {
                edges { node { version { version current } previous { version current } } }
            }
        }",
        json!({ "t": tree, "e": entry_id }),
    )
    .await;
    assert_eq!(data["auditEntry"]["action"], "UPDATE");
    let change = &data["auditEntryChanges"]["edges"][0]["node"];
    assert_eq!(change["version"]["version"], 3);
    assert_eq!(change["version"]["current"], true);
    assert_eq!(change["previous"]["version"], 2);
    assert_eq!(change["previous"]["current"], false);

    let data = graphql(
        &app,
        "mutation($t: ID!, $p: ID!) {
            revertRecord(treeId: $t, recordType: PERSON, recordId: $p, version: 2) {
                action category details { version }
            }
        }",
        json!({ "t": tree, "p": person }),
    )
    .await;
    assert_eq!(data["revertRecord"]["action"], "REVERT");
    assert_eq!(data["revertRecord"]["category"], "HISTORY");
    assert_eq!(data["revertRecord"]["details"]["version"], 2);
    let person_now = ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/persons/{person}"),
        None,
    )
    .await;
    assert_eq!(person_now["sex"], "unknown");
    assert_no_duplicated_state(&db).await;
}

async fn recently_modified(app: &axum::Router, tree: &str, query: &str) -> Vec<Value> {
    ok(
        app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/persons/recently-modified{query}"),
        None,
    )
    .await
    .as_array()
    .unwrap()
    .clone()
}

/// The home page's list: the persons the latest writes were about, newest
/// first, less those an import brought in and those deleted since — over
/// REST and GraphQL alike.
#[tokio::test]
async fn recently_modified_persons_follow_the_audit_log() {
    let (db, app) = setup().await;
    let tree = create_tree(&app).await;
    // An import is about the tree; nobody worked on its persons.
    let gedcom = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n1 CHAR UTF-8\n\
                  0 @I1@ INDI\n1 NAME Iota /Fixture/\n1 SEX F\n0 TRLR\n";
    common::import_gedcom(&app, &db, &tree, gedcom).await;
    assert!(recently_modified(&app, &tree, "").await.is_empty());

    let first = create_person(&app, &tree, "Nu", "Fixture").await;
    let second = create_person(&app, &tree, "Xi", "Fixture").await;
    let third = create_person(&app, &tree, "Omicron", "Fixture").await;
    // An event is a change to its person: the first comes back on top.
    ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/events"),
        Some(json!({
            "event_type": "birth",
            "date_value": "1 JAN 1900",
            "person_id": first,
        })),
    )
    .await;
    ok(
        &app,
        Method::DELETE,
        &format!("/api/v1/trees/{tree}/persons/{third}"),
        None,
    )
    .await;

    let rows = recently_modified(&app, &tree, "").await;
    let ids: Vec<&str> = rows
        .iter()
        .map(|row| row["person_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, [first.as_str(), second.as_str()]);
    // Rows are search entries, with what the search row draws.
    assert_eq!(rows[0]["given_names"], "Nu");
    assert_eq!(rows[0]["surname"], "Fixture");
    assert_eq!(rows[0]["birth_year"], "1900");

    let limited = recently_modified(&app, &tree, "?limit=1").await;
    assert_eq!(limited.len(), 1);
    assert_eq!(limited[0]["person_id"], first.as_str());

    let data = graphql(
        &app,
        "query($t: ID!) {
            all: recentlyModifiedPersons(treeId: $t) { personId givenNames }
            one: recentlyModifiedPersons(treeId: $t, limit: 1) { personId }
        }",
        json!({ "t": tree }),
    )
    .await;
    let gql_ids: Vec<&str> = data["all"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["personId"].as_str().unwrap())
        .collect();
    assert_eq!(gql_ids, [first.as_str(), second.as_str()]);
    assert_eq!(data["all"][0]["givenNames"], "Nu");
    assert_eq!(data["one"].as_array().unwrap().len(), 1);

    // A tree that does not exist is not found, on both surfaces.
    let missing = Uuid::now_v7();
    let (status, _) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{missing}/persons/recently-modified"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let body = json!({
        "query": "query($t: ID!) { recentlyModifiedPersons(treeId: $t) { personId } }",
        "variables": { "t": missing.to_string() },
    });
    let (_, json) = send(&app, Method::POST, "/graphql", Some(body)).await;
    assert!(json.get("errors").is_some(), "{json}");
}

/// The totals of a growth response: persons added, persons removed, and its
/// imports as `(format, persons)`.
fn growth_totals(growth: &Value) -> (i64, i64, Vec<(String, i64)>) {
    let days = growth["days"].as_array().unwrap();
    let sum = |field: &str| -> i64 { days.iter().map(|d| d[field].as_i64().unwrap()).sum() };
    let imports = growth["imports"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["format"].as_str().unwrap().to_string(),
                i["persons"].as_i64().unwrap(),
            )
        })
        .collect();
    (sum("added"), sum("removed"), imports)
}

/// The Growth tab's data: persons created, imported, deleted, merged and
/// restored, filed by day, with the imports to mark — over REST and GraphQL
/// alike.
#[tokio::test]
async fn growth_counts_the_persons_over_time() {
    let (db, app) = setup().await;
    let tree = create_tree(&app).await;
    let gedcom = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n1 CHAR UTF-8\n\
                  0 @I1@ INDI\n1 NAME Iota /Fixture/\n1 SEX F\n\
                  0 @I2@ INDI\n1 NAME Kappa /Fixture/\n1 SEX M\n\
                  0 @I3@ INDI\n1 NAME Lambda /Fixture/\n1 SEX M\n0 TRLR\n";
    common::import_gedcom(&app, &db, &tree, gedcom).await;
    // Three by hand: one merged into another, one deleted then restored.
    let kept = create_person(&app, &tree, "Nu", "Fixture").await;
    let duplicate = create_person(&app, &tree, "Nu", "Fixture").await;
    ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/persons/{kept}/merge"),
        Some(json!({ "duplicate_id": duplicate })),
    )
    .await;
    let restored = create_person(&app, &tree, "Pi", "Fixture").await;
    let before = versions(&app, &tree, "person", &restored).await[0]["version"].clone();
    ok(
        &app,
        Method::DELETE,
        &format!("/api/v1/trees/{tree}/persons/{restored}"),
        None,
    )
    .await;
    ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/history/person/{restored}/revert"),
        Some(json!({ "version": before })),
    )
    .await;
    assert_no_duplicated_state(&db).await;

    let rest = ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/statistics/growth"),
        None,
    )
    .await;
    // Added: three imported, three created, one restored. Removed: the
    // merged duplicate and the deletion the restore undid. Five remain.
    assert_eq!(
        growth_totals(&rest),
        (7, 2, vec![("gedcom".to_string(), 3)])
    );
    // Days are dates, oldest first.
    let dates: Vec<&str> = rest["days"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["date"].as_str().unwrap())
        .collect();
    assert!(dates.windows(2).all(|w| w[0] < w[1]), "{dates:?}");
    assert_eq!(dates[0].len(), "2026-01-01".len());

    let data = graphql(
        &app,
        "query($t: ID!) {
            treeGrowth(treeId: $t) {
                days { date added removed }
                imports { occurredAt format fileName persons }
            }
        }",
        json!({ "t": tree }),
    )
    .await;
    let growth = &data["treeGrowth"];
    assert_eq!(growth_totals(growth), growth_totals(&rest));
    assert_eq!(growth["days"], rest["days"]);

    // Data written behind the API's back has no history: its person counts
    // once, and no import is marked.
    let legacy = Uuid::now_v7();
    TreeRepo::create(&db, legacy, "Legacy Fixture".into(), None)
        .await
        .unwrap();
    PersonRepo::create(&db, Uuid::now_v7(), legacy, oxidgene_core::Sex::Male)
        .await
        .unwrap();
    let rest = ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{legacy}/statistics/growth"),
        None,
    )
    .await;
    assert_eq!(growth_totals(&rest), (1, 0, Vec::new()));

    // An empty tree has no day; an unknown one is not found, on both
    // surfaces.
    let empty = create_tree(&app).await;
    let rest = ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{empty}/statistics/growth"),
        None,
    )
    .await;
    assert_eq!(rest, json!({ "days": [], "imports": [] }));
    let missing = Uuid::now_v7();
    let (status, _) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{missing}/statistics/growth"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let body = json!({
        "query": "query($t: ID!) { treeGrowth(treeId: $t) { days { date } } }",
        "variables": { "t": missing.to_string() },
    });
    let (_, json) = send(&app, Method::POST, "/graphql", Some(body)).await;
    assert!(json.get("errors").is_some(), "{json}");
}
