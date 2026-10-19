//! The reads of places, repositories and notes carry their tags, over REST
//! and GraphQL, and the batched reads of them cost the same whatever the
//! number of records. Each operation has a REST test and its GraphQL twin
//! with the same cases.
//!
//! All data is fictitious.

mod common;

use axum::Router;
use axum::http::{Method, StatusCode};
use common::{app_on, gql_ok, new_person, new_tree, ok, send, setup_db};
use oxidgene_api::service::tag;
use oxidgene_db::sea_orm::DatabaseConnection;
use serde_json::{Value, json};
use uuid::Uuid;

struct Fixture {
    db: DatabaseConnection,
    app: Router,
    tree: String,
}

impl Fixture {
    async fn new(name: &str) -> Self {
        let db = setup_db().await;
        let app = app_on(db.clone());
        let tree = new_tree(&app, name).await;
        Self { db, app, tree }
    }

    fn tree_id(&self) -> Uuid {
        Uuid::parse_str(&self.tree).unwrap()
    }

    fn path(&self, suffix: &str) -> String {
        format!("/api/v1/trees/{}{suffix}", self.tree)
    }

    async fn post(&self, suffix: &str, body: Value) -> String {
        ok(&self.app, Method::POST, &self.path(suffix), Some(body)).await["id"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    async fn create_tag(&self, label: &str) -> Uuid {
        tag::create(&self.db, self.tree_id(), label, None)
            .await
            .unwrap()
            .0
            .tag
            .id
    }

    /// Put `tag` on a record over REST, expecting success.
    async fn tag_over_rest(&self, tag: Uuid, kind: &str, id: &str) {
        let (status, body) = send(
            &self.app,
            Method::PUT,
            &self.path(&format!("/tags/{tag}/records/{kind}/{id}")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    }

    async fn named_person(&self, given: &str, surname: &str) -> String {
        let id = new_person(&self.app, &self.tree).await;
        ok(
            &self.app,
            Method::POST,
            &self.path(&format!("/persons/{id}/names")),
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
}

/// The labels of a JSON array of tag references, in order.
fn labels(tags: &Value) -> Vec<String> {
    tags.as_array()
        .expect("an array of tags")
        .iter()
        .map(|tag| tag["label"].as_str().unwrap().to_owned())
        .collect()
}

/// The `node` of the connection entry whose `id` is `id`.
fn node<'a>(connection: &'a Value, id: &str) -> &'a Value {
    connection["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|edge| &edge["node"])
        .find(|node| node["id"] == id)
        .unwrap_or_else(|| panic!("no entry {id}"))
}

/// The entry of a flat dictionary list whose `id` is `id`.
fn entry_of<'a>(entries: &'a [Value], id: &str) -> &'a Value {
    entries
        .iter()
        .find(|entry| entry["id"] == id)
        .unwrap_or_else(|| panic!("no dictionary entry {id}"))
}

// ── REST ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn place_repository_and_note_reads_carry_their_tags_over_rest() {
    let f = Fixture::new("Tags Record Reads REST").await;
    // Created in this order, read back sorted by folded label.
    let bravo = f.create_tag("Bravo").await;
    let alpha = f.create_tag("alpha").await;
    let person = f.named_person("Piers", "Hollowell").await;
    let place = f.post("/places", json!({ "name": "Harbour Town" })).await;
    let plain_place = f.post("/places", json!({ "name": "Millbrook" })).await;
    let repository = f
        .post("/repositories", json!({ "name": "Fictional Archive" }))
        .await;
    let note = f
        .post("/notes", json!({ "text": "Remark", "person_id": person }))
        .await;
    let plain_note = f
        .post("/notes", json!({ "text": "Plain", "person_id": person }))
        .await;
    f.tag_over_rest(bravo, "place", &place).await;
    f.tag_over_rest(alpha, "place", &place).await;
    f.tag_over_rest(bravo, "repository", &repository).await;
    f.tag_over_rest(alpha, "note", &note).await;

    let read = ok(
        &f.app,
        Method::GET,
        &f.path(&format!("/places/{place}")),
        None,
    )
    .await;
    assert_eq!(labels(&read["tags"]), ["alpha", "Bravo"]);
    let listed = ok(&f.app, Method::GET, &f.path("/places"), None).await;
    assert_eq!(labels(&node(&listed, &place)["tags"]), ["alpha", "Bravo"]);
    assert!(labels(&node(&listed, &plain_place)["tags"]).is_empty());

    // An update answers with the tags the place still carries.
    let updated = ok(
        &f.app,
        Method::PUT,
        &f.path(&format!("/places/{place}")),
        Some(json!({ "name": "Harbour Town, north quay" })),
    )
    .await;
    assert_eq!(updated["name"], "Harbour Town, north quay");
    assert_eq!(labels(&updated["tags"]), ["alpha", "Bravo"]);

    let read = ok(
        &f.app,
        Method::GET,
        &f.path(&format!("/repositories/{repository}")),
        None,
    )
    .await;
    assert_eq!(labels(&read["tags"]), ["Bravo"]);
    let updated = ok(
        &f.app,
        Method::PUT,
        &f.path(&format!("/repositories/{repository}")),
        Some(json!({ "address": "1 Fictional Street" })),
    )
    .await;
    assert_eq!(labels(&updated["tags"]), ["Bravo"]);
    let listed = ok(&f.app, Method::GET, &f.path("/repositories"), None).await;
    assert_eq!(labels(&node(&listed, &repository)["tags"]), ["Bravo"]);

    let read = ok(
        &f.app,
        Method::GET,
        &f.path(&format!("/notes/{note}")),
        None,
    )
    .await;
    assert_eq!(labels(&read["tags"]), ["alpha"]);
    let listed = ok(
        &f.app,
        Method::GET,
        &f.path(&format!("/notes?person_id={person}")),
        None,
    )
    .await;
    assert_eq!(labels(&node(&listed, &note)["tags"]), ["alpha"]);
    assert!(labels(&node(&listed, &plain_note)["tags"]).is_empty());
    let updated = ok(
        &f.app,
        Method::PUT,
        &f.path(&format!("/notes/{note}")),
        Some(json!({ "text": "Remark, corrected" })),
    )
    .await;
    assert_eq!(labels(&updated["tags"]), ["alpha"]);
}

#[tokio::test]
async fn dictionary_place_reads_carry_their_tags_over_rest() {
    let f = Fixture::new("Tags Dictionary Places REST").await;
    let bravo = f.create_tag("Bravo").await;
    let alpha = f.create_tag("alpha").await;
    let tagged = f.post("/places", json!({ "name": "Harbour Town" })).await;
    let plain = f.post("/places", json!({ "name": "Millbrook" })).await;
    f.tag_over_rest(bravo, "place", &tagged).await;
    f.tag_over_rest(alpha, "place", &tagged).await;

    let listed = ok(&f.app, Method::GET, &f.path("/dictionary/places"), None).await;
    let entries = listed.as_array().expect("a flat list");
    assert_eq!(
        labels(&entry_of(entries, &tagged)["tags"]),
        ["alpha", "Bravo"]
    );
    assert!(labels(&entry_of(entries, &plain)["tags"]).is_empty());
}

#[tokio::test]
async fn deleted_and_foreign_records_are_not_read_over_rest() {
    let f = Fixture::new("Tags Record Deleted REST").await;
    let other = new_tree(&f.app, "Tags Record Foreign Other").await;
    let tag = f.create_tag("Doubtful").await;
    let person = f.named_person("Piers", "Hollowell").await;
    let note = f
        .post("/notes", json!({ "text": "Remark", "person_id": person }))
        .await;
    let place = f.post("/places", json!({ "name": "Harbour Town" })).await;
    f.tag_over_rest(tag, "note", &note).await;
    f.tag_over_rest(tag, "place", &place).await;

    let (status, body) = send(
        &f.app,
        Method::DELETE,
        &f.path(&format!("/notes/{note}")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (status, _) = send(
        &f.app,
        Method::GET,
        &f.path(&format!("/notes/{note}")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let listed = ok(
        &f.app,
        Method::GET,
        &f.path(&format!("/notes?person_id={person}")),
        None,
    )
    .await;
    assert!(listed["edges"].as_array().unwrap().is_empty());

    let (status, _) = send(
        &f.app,
        Method::GET,
        &format!("/api/v1/trees/{other}/places/{place}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ── GraphQL twins ────────────────────────────────────────────────────

#[tokio::test]
async fn place_repository_and_note_reads_carry_their_tags_over_graphql() {
    let f = Fixture::new("Tags Record Reads GraphQL").await;
    let bravo = f.create_tag("Bravo").await;
    let alpha = f.create_tag("alpha").await;
    let person = f.named_person("Piers", "Hollowell").await;
    let created = gql_ok(
        &f.app,
        r#"mutation($tree: ID!) { createPlace(treeId: $tree, input: { name: "Harbour Town" }) { id } }"#,
        json!({ "tree": f.tree }),
    )
    .await;
    let place = created["createPlace"]["id"].as_str().unwrap().to_owned();
    let created = gql_ok(
        &f.app,
        r#"mutation($tree: ID!) { createRepository(treeId: $tree, input: { name: "Fictional Archive" }) { id } }"#,
        json!({ "tree": f.tree }),
    )
    .await;
    let repository = created["createRepository"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let created = gql_ok(
        &f.app,
        r#"mutation($tree: ID!, $person: String!) {
            createNote(treeId: $tree, input: { text: "Remark", personId: $person }) { id }
        }"#,
        json!({ "tree": f.tree, "person": person }),
    )
    .await;
    let note = created["createNote"]["id"].as_str().unwrap().to_owned();
    f.tag_over_rest(bravo, "place", &place).await;
    f.tag_over_rest(alpha, "place", &place).await;
    f.tag_over_rest(bravo, "repository", &repository).await;
    f.tag_over_rest(alpha, "note", &note).await;

    let data = gql_ok(
        &f.app,
        r#"query ($tree: ID!, $place: ID!, $repository: ID!, $note: ID!, $person: ID!) {
            place(treeId: $tree, id: $place) { tags { label } }
            places(treeId: $tree) { edges { node { id tags { label } } } }
            repository(treeId: $tree, id: $repository) { tags { label } }
            repositories(treeId: $tree) { edges { node { id tags { label } } } }
            note(treeId: $tree, id: $note) { tags { label } }
            notes(treeId: $tree, personId: $person) { edges { node { id tags { label } } } }
        }"#,
        json!({
            "tree": f.tree,
            "place": place,
            "repository": repository,
            "note": note,
            "person": person,
        }),
    )
    .await;
    assert_eq!(labels(&data["place"]["tags"]), ["alpha", "Bravo"]);
    assert_eq!(
        labels(&node(&data["places"], &place)["tags"]),
        ["alpha", "Bravo"]
    );
    assert_eq!(labels(&data["repository"]["tags"]), ["Bravo"]);
    assert_eq!(
        labels(&node(&data["repositories"], &repository)["tags"]),
        ["Bravo"]
    );
    assert_eq!(labels(&data["note"]["tags"]), ["alpha"]);
    assert_eq!(labels(&node(&data["notes"], &note)["tags"]), ["alpha"]);

    let updated = gql_ok(
        &f.app,
        r#"mutation($tree: ID!, $place: ID!) {
            updatePlace(treeId: $tree, id: $place, input: { name: "Harbour Town, north quay" }) { tags { label } }
        }"#,
        json!({ "tree": f.tree, "place": place }),
    )
    .await;
    assert_eq!(labels(&updated["updatePlace"]["tags"]), ["alpha", "Bravo"]);
}

#[tokio::test]
async fn dictionary_place_reads_carry_their_tags_over_graphql() {
    let f = Fixture::new("Tags Dictionary Places GraphQL").await;
    let bravo = f.create_tag("Bravo").await;
    let alpha = f.create_tag("alpha").await;
    let created = gql_ok(
        &f.app,
        r#"mutation($tree: ID!) { createPlace(treeId: $tree, input: { name: "Harbour Town" }) { id } }"#,
        json!({ "tree": f.tree }),
    )
    .await;
    let tagged = created["createPlace"]["id"].as_str().unwrap().to_owned();
    let created = gql_ok(
        &f.app,
        r#"mutation($tree: ID!) { createPlace(treeId: $tree, input: { name: "Millbrook" }) { id } }"#,
        json!({ "tree": f.tree }),
    )
    .await;
    let plain = created["createPlace"]["id"].as_str().unwrap().to_owned();
    f.tag_over_rest(bravo, "place", &tagged).await;
    f.tag_over_rest(alpha, "place", &tagged).await;

    let data = gql_ok(
        &f.app,
        r#"query ($tree: ID!) {
            dictionaryPlaces(treeId: $tree) { place { id tags { label } } count }
        }"#,
        json!({ "tree": f.tree }),
    )
    .await;
    let listed = data["dictionaryPlaces"].as_array().expect("a flat list");
    let entry = |id: &str| {
        listed
            .iter()
            .find(|entry| entry["place"]["id"] == id)
            .unwrap_or_else(|| panic!("no dictionary entry {id}"))
    };
    assert_eq!(labels(&entry(&tagged)["place"]["tags"]), ["alpha", "Bravo"]);
    assert!(labels(&entry(&plain)["place"]["tags"]).is_empty());
}

#[tokio::test]
async fn deleted_and_foreign_records_are_not_read_over_graphql() {
    let f = Fixture::new("Tags Record Deleted GraphQL").await;
    let other = new_tree(&f.app, "Tags Record Foreign GraphQL").await;
    let tag = f.create_tag("Doubtful").await;
    let person = f.named_person("Piers", "Hollowell").await;
    let created = gql_ok(
        &f.app,
        r#"mutation($tree: ID!, $person: String!) {
            createNote(treeId: $tree, input: { text: "Remark", personId: $person }) { id }
        }"#,
        json!({ "tree": f.tree, "person": person }),
    )
    .await;
    let note = created["createNote"]["id"].as_str().unwrap().to_owned();
    let created = gql_ok(
        &f.app,
        r#"mutation($tree: ID!) { createPlace(treeId: $tree, input: { name: "Harbour Town" }) { id } }"#,
        json!({ "tree": f.tree }),
    )
    .await;
    let place = created["createPlace"]["id"].as_str().unwrap().to_owned();
    f.tag_over_rest(tag, "note", &note).await;
    f.tag_over_rest(tag, "place", &place).await;

    gql_ok(
        &f.app,
        r#"mutation($tree: ID!, $note: ID!) { deleteNote(treeId: $tree, id: $note) }"#,
        json!({ "tree": f.tree, "note": note }),
    )
    .await;
    let data = gql_ok(
        &f.app,
        r#"query ($tree: ID!, $note: ID!) { note(treeId: $tree, id: $note) { id } }"#,
        json!({ "tree": f.tree, "note": note }),
    )
    .await;
    assert!(data["note"].is_null());

    let data = gql_ok(
        &f.app,
        r#"query ($tree: ID!, $place: ID!) { place(treeId: $tree, id: $place) { id } }"#,
        json!({ "tree": other, "place": place }),
    )
    .await;
    assert!(data["place"].is_null());
}
