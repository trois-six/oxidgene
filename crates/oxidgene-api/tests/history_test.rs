//! Change history over REST and GraphQL: the audit log every write leaves,
//! the versions of persons, places, sources and tree settings, and restoring
//! an earlier version.
//!
//! All data is fictitious.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use oxidgene_api::service::history::record_baselines;
use oxidgene_api::{AppState, build_router};
use oxidgene_db::repo::{PersonNamePieces, PersonNameRepo, PersonRepo, TreeRepo};
use oxidgene_db::repo::{connect, run_migrations};
use sea_orm::DatabaseConnection;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

async fn setup() -> (DatabaseConnection, axum::Router) {
    let db = connect("sqlite::memory:")
        .await
        .expect("connect to in-memory SQLite");
    run_migrations(&db).await.expect("migrations");
    let state = AppState::new(db.clone(), std::env::temp_dir().join("oxidgene-test-media"));
    (db, build_router(state))
}

async fn send(
    app: &axum::Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let body = match body {
        Some(json) => Body::from(serde_json::to_vec(&json).unwrap()),
        None => Body::empty(),
    };
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(body)
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, json)
}

async fn ok(app: &axum::Router, method: Method, uri: &str, body: Option<Value>) -> Value {
    let (status, json) = send(app, method, uri, body).await;
    assert!(status.is_success(), "{uri}: {status} {json}");
    json
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

async fn versions(app: &axum::Router, tree: &str, record_type: &str, id: &str) -> Vec<Value> {
    let page = ok(
        app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/history/{record_type}/{id}?first=100"),
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

#[tokio::test]
async fn every_person_write_is_audited_and_versioned() {
    let (_, app) = setup().await;
    let tree = create_tree(&app).await;
    let person = create_person(&app, &tree, "Alpha", "Fixture").await;

    // Created without a name, then named: two versions, latest first.
    let history = versions(&app, &tree, "person", &person).await;
    assert_eq!(history.len(), 2);
    assert_eq!(history[0]["version"], 2);
    assert_eq!(history[0]["entry"]["action"], "create");
    assert_eq!(history[0]["entry"]["entity"], "person_name");
    assert_eq!(history[0]["snapshot"]["type"], "person");
    assert_eq!(history[0]["snapshot"]["names"][0]["given_names"], "Alpha");
    assert!(
        history[1]["snapshot"]["names"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    // The audit log reads newest first and labels the person by name.
    let entries = audit(&app, &tree, "").await;
    assert_eq!(entries[0]["entity"], "person_name");
    assert_eq!(entries[0]["subject"], "person");
    assert_eq!(entries[0]["subject_id"], person.as_str());
    assert_eq!(entries[0]["label"], "Alpha Fixture");
    assert_eq!(entries[0]["version_count"], 1);
    assert_eq!(entries.last().unwrap()["entity"], "tree");
    assert_eq!(entries.last().unwrap()["category"], "settings");

    // Filters: by category and by subject.
    let settings = audit(&app, &tree, "&category=settings").await;
    assert_eq!(settings.len(), 1);
    let about = audit(&app, &tree, &format!("&subject_id={person}")).await;
    assert_eq!(about.len(), 2);
}

#[tokio::test]
async fn an_edit_versions_only_the_records_it_changed() {
    let (_, app) = setup().await;
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
    let entry = {
        ok(
            &app,
            Method::PUT,
            &format!("/api/v1/trees/{tree}/persons/{father}/names/{name_id}"),
            Some(json!({ "given_names": "Delta" })),
        )
        .await;
        audit(&app, &tree, "").await[0].clone()
    };
    assert_eq!(entry["version_count"], 1);
    assert_eq!(
        versions(&app, &tree, "person", &child).await.len(),
        child_versions
    );

    // The entry's changes pair the new version with the one it replaced.
    let changes = ok(
        &app,
        Method::GET,
        &format!(
            "/api/v1/trees/{tree}/audit/{}/changes",
            entry["id"].as_str().unwrap()
        ),
        None,
    )
    .await;
    let change = &changes["edges"][0]["node"];
    assert_eq!(
        change["version"]["snapshot"]["names"][0]["given_names"],
        "Delta"
    );
    assert_eq!(
        change["previous"]["snapshot"]["names"][0]["given_names"],
        "Beta"
    );
}

#[tokio::test]
async fn reverting_a_person_restores_names_events_and_deletion() {
    let (_, app) = setup().await;
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
    ok(
        &app,
        Method::DELETE,
        &format!("/api/v1/trees/{tree}/persons/{person}"),
        None,
    )
    .await;
    let latest = &versions(&app, &tree, "person", &person).await[0];
    assert_eq!(latest["deleted"], true);

    // A deleted state is not something to restore.
    let (status, _) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/history/person/{person}/revert"),
        Some(json!({ "version": latest["version"] })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

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

    // The restore is itself the newest version, equal to the one restored.
    let history = versions(&app, &tree, "person", &person).await;
    let restored = history.iter().find(|v| v["version"] == with_birth).unwrap();
    assert_eq!(history[0]["deleted"], false);
    assert_eq!(history[0]["snapshot"], restored["snapshot"]);
}

#[tokio::test]
async fn reverting_a_spouse_restores_the_union() {
    let (_, app) = setup().await;
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

    // A marriage recorded on the family versions both spouses.
    ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/events"),
        Some(json!({ "event_type": "marriage", "family_id": family })),
    )
    .await;
    assert_eq!(audit(&app, &tree, "").await[0]["version_count"], 2);

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
}

#[tokio::test]
async fn settings_media_and_exports_are_audited() {
    let (_, app) = setup().await;
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

    // A media write is audited under media, and versions nothing.
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

    // An export leaves an entry naming its format.
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
}

#[tokio::test]
async fn imports_version_everything_they_bring() {
    let (_, app) = setup().await;
    let tree = create_tree(&app).await;
    let gedcom = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n1 CHAR UTF-8\n\
                  0 @I1@ INDI\n1 NAME Iota /Fixture/\n1 SEX F\n\
                  0 @I2@ INDI\n1 NAME Kappa /Fixture/\n1 SEX M\n0 TRLR\n";
    ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/gedcom/import"),
        Some(json!({ "gedcom": gedcom })),
    )
    .await;
    let imports = audit(&app, &tree, "&category=import").await;
    assert_eq!(imports.len(), 1);
    assert_eq!(imports[0]["details"]["format"], "gedcom");
    assert_eq!(imports[0]["details"]["count"], 2);
    assert_eq!(imports[0]["version_count"], 2);
}

#[tokio::test]
async fn existing_data_gets_one_baseline() {
    let (db, app) = setup().await;
    // Written behind the API's back, as data from before history existed.
    let tree_id = Uuid::now_v7();
    TreeRepo::create(&db, tree_id, "Legacy Fixture".into(), None)
        .await
        .unwrap();
    let person_id = Uuid::now_v7();
    PersonRepo::create(&db, person_id, tree_id, oxidgene_core::Sex::Female)
        .await
        .unwrap();
    PersonNameRepo::create(
        &db,
        Uuid::now_v7(),
        person_id,
        oxidgene_core::NameType::Birth,
        PersonNamePieces {
            given_names: Some("Lambda".into()),
            surname: Some("Fixture".into()),
            ..PersonNamePieces::default()
        },
        true,
        0,
    )
    .await
    .unwrap();

    assert_eq!(record_baselines(&db).await.unwrap(), 1);
    assert_eq!(record_baselines(&db).await.unwrap(), 0);

    let tree = tree_id.to_string();
    let entries = audit(&app, &tree, "").await;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["action"], "baseline");
    assert_eq!(entries[0]["category"], "history");
    let history = versions(&app, &tree, "person", &person_id.to_string()).await;
    assert_eq!(history.len(), 1);
    assert_eq!(history[0]["snapshot"]["sex"], "female");
}

#[tokio::test]
async fn graphql_mirrors_the_history_surface() {
    let (_, app) = setup().await;
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
                edges { node { version snapshot { recordType person { sex names { givenNames } } } } }
            }
            recordVersion(treeId: $t, recordType: PERSON, recordId: $p, version: 2) {
                snapshot { person { sex } }
            }
        }",
        json!({ "t": tree, "p": person }),
    )
    .await;
    assert_eq!(data["recordVersions"]["totalCount"], 3);
    let node = &data["recordVersions"]["edges"][0]["node"];
    assert_eq!(node["version"], 3);
    assert_eq!(node["snapshot"]["recordType"], "PERSON");
    assert_eq!(node["snapshot"]["person"]["sex"], "MALE");
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
                edges { node { version { version } previous { version } } }
            }
        }",
        json!({ "t": tree, "e": entry_id }),
    )
    .await;
    assert_eq!(data["auditEntry"]["action"], "UPDATE");
    let change = &data["auditEntryChanges"]["edges"][0]["node"];
    assert_eq!(change["version"]["version"], 3);
    assert_eq!(change["previous"]["version"], 2);

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

/// The home page's list: the persons a write versioned, newest first, less
/// those an import brought in and those deleted since — over REST and GraphQL
/// alike.
#[tokio::test]
async fn recently_modified_persons_follow_the_history() {
    let (_, app) = setup().await;
    let tree = create_tree(&app).await;
    // An import versions everyone it brings; nobody worked on them.
    let gedcom = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n1 CHAR UTF-8\n\
                  0 @I1@ INDI\n1 NAME Iota /Fixture/\n1 SEX F\n0 TRLR\n";
    ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/gedcom/import"),
        Some(json!({ "gedcom": gedcom })),
    )
    .await;
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
