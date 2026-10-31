//! Tags over the service, REST and GraphQL: the vocabulary's writes (folding,
//! rename, merge, delete), the links on every kind of record, the refresh of
//! the person projections that show them, and the reads that carry them.
//!
//! All data is fictitious.

use crate::common;

use axum::Router;
use axum::http::{Method, StatusCode};
use common::populated::{png, upload_page};
use common::{
    all_profiles, app_on, gql, gql_error_code, gql_ok, new_person, new_tree, ok, send, setup_db,
};
use oxidgene_api::profile::ProfileService;
use oxidgene_api::service::tag;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::RecordKind;
use oxidgene_db::repo::{RecordTagRepo, TagRepo};
use oxidgene_db::sea_orm::DatabaseConnection;
use serde_json::{Value, json};
use uuid::Uuid;

struct Fixture {
    db: DatabaseConnection,
    app: Router,
    profiles: ProfileService,
    tree: String,
}

impl Fixture {
    async fn new(name: &str) -> Self {
        let db = setup_db().await;
        let app = app_on(db.clone());
        let profiles = ProfileService::new(db.clone());
        let tree = new_tree(&app, name).await;
        Self {
            db,
            app,
            profiles,
            tree,
        }
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

    async fn create_tag(&self, label: &str) -> Uuid {
        tag::create(&self.db, self.tree_id(), label, None)
            .await
            .unwrap()
            .0
            .tag
            .id
    }

    async fn tag_person(&self, tag_id: Uuid, person: &str) {
        self.tag_record(tag_id, RecordKind::Person, person)
            .await
            .unwrap();
    }

    async fn tag_record(
        &self,
        tag_id: Uuid,
        kind: RecordKind,
        id: &str,
    ) -> Result<(), OxidGeneError> {
        tag::tag_record(
            &self.db,
            &self.profiles,
            self.tree_id(),
            tag_id,
            kind,
            Uuid::parse_str(id).unwrap(),
        )
        .await
    }

    async fn profile_tags(&self, person: &str) -> Vec<String> {
        all_profiles(&self.app, &self.tree)
            .await
            .into_iter()
            .find(|profile| profile["person_id"] == person)
            .map(|profile| labels(&profile["tags"]))
            .expect("the person has a profile")
    }

    async fn tags_of(&self, kind: RecordKind, id: &str) -> Vec<String> {
        RecordTagRepo::tags_of(&self.db, kind, &[Uuid::parse_str(id).unwrap()])
            .await
            .unwrap()
            .remove(&Uuid::parse_str(id).unwrap())
            .unwrap_or_default()
            .into_iter()
            .map(|tag| tag.label)
            .collect()
    }
}

/// The labels of a JSON array of tag references, in order; none for an
/// absent array, which is how a profile writes an empty one.
fn labels(tags: &Value) -> Vec<String> {
    if tags.is_null() {
        return Vec::new();
    }
    tags.as_array()
        .expect("an array of tags")
        .iter()
        .map(|tag| tag["label"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn create_folds_the_label_onto_the_existing_tag() {
    let f = Fixture::new("Tags Create").await;
    let (first, created) = tag::create(&f.db, f.tree_id(), "Lanterne", Some(3))
        .await
        .unwrap();
    assert!(created);
    assert_eq!(first.tag.color, 3);

    // Case and accents fold away; the existing tag keeps its own colour.
    let (again, created) = tag::create(&f.db, f.tree_id(), "  LANTÈRNE ", Some(7))
        .await
        .unwrap();
    assert!(!created);
    assert_eq!(again.tag.id, first.tag.id);
    assert_eq!(again.tag.color, 3);
    assert_eq!(tag::list(&f.db, f.tree_id()).await.unwrap().len(), 1);
}

#[tokio::test]
async fn rename_conflicts_with_another_tag_and_recolours() {
    let f = Fixture::new("Tags Rename").await;
    let alpha = f.create_tag("Alpha").await;
    let bravo = f.create_tag("Bravo").await;

    let err = tag::update(&f.db, &f.profiles, f.tree_id(), bravo, Some("ALPHA"), None)
        .await
        .unwrap_err();
    assert!(matches!(err, OxidGeneError::Conflict(_)), "{err:?}");

    let renamed = tag::update(
        &f.db,
        &f.profiles,
        f.tree_id(),
        bravo,
        Some("Bravo Two"),
        Some(5),
    )
    .await
    .unwrap();
    assert_eq!(renamed.tag.label, "Bravo Two");
    assert_eq!(renamed.tag.color, 5);
    assert_ne!(alpha, bravo);
}

#[tokio::test]
async fn merge_moves_links_once_and_refreshes_profiles() {
    let f = Fixture::new("Tags Merge").await;
    let alpha = f.create_tag("Alpha").await;
    let bravo = f.create_tag("Bravo").await;
    let p1 = f.named_person("Piers", "Hollowell").await;
    let p2 = f.named_person("Ada", "Marrow").await;
    let family = f.post("/families", json!({})).await;
    f.tag_person(alpha, &p1).await;
    f.tag_person(bravo, &p1).await;
    f.tag_person(alpha, &p2).await;
    f.tag_record(alpha, RecordKind::Family, &family)
        .await
        .unwrap();

    let merged = tag::merge(&f.db, &f.profiles, f.tree_id(), alpha, bravo)
        .await
        .unwrap();
    assert_eq!(merged.tag.id, bravo);
    assert_eq!(merged.count(RecordKind::Person), 2);
    assert_eq!(merged.count(RecordKind::Family), 1);

    // A person carrying both tags keeps one link to the survivor.
    assert_eq!(f.tags_of(RecordKind::Person, &p1).await, ["Bravo"]);
    assert_eq!(f.tags_of(RecordKind::Family, &family).await, ["Bravo"]);
    assert_eq!(f.profile_tags(&p1).await, ["Bravo"]);
    assert_eq!(f.profile_tags(&p2).await, ["Bravo"]);
    assert!(matches!(
        TagRepo::get(&f.db, f.tree_id(), alpha).await,
        Err(OxidGeneError::NotFound { .. })
    ));

    let err = tag::merge(&f.db, &f.profiles, f.tree_id(), bravo, bravo)
        .await
        .unwrap_err();
    assert!(matches!(err, OxidGeneError::Validation(_)), "{err:?}");
}

#[tokio::test]
async fn delete_counts_links_and_clears_profiles() {
    let f = Fixture::new("Tags Delete").await;
    let gamma = f.create_tag("Gamma").await;
    let p1 = f.named_person("Piers", "Hollowell").await;
    let p2 = f.named_person("Ada", "Marrow").await;
    let family = f.post("/families", json!({})).await;
    f.tag_person(gamma, &p1).await;
    f.tag_person(gamma, &p2).await;
    f.tag_record(gamma, RecordKind::Family, &family)
        .await
        .unwrap();
    assert_eq!(f.profile_tags(&p1).await, ["Gamma"]);

    let removed = tag::delete(&f.db, &f.profiles, f.tree_id(), gamma)
        .await
        .unwrap();
    assert_eq!(removed, 3);
    assert!(f.profile_tags(&p1).await.is_empty());
    assert!(f.profile_tags(&p2).await.is_empty());
    assert!(f.tags_of(RecordKind::Family, &family).await.is_empty());
    assert!(tag::list(&f.db, f.tree_id()).await.unwrap().is_empty());
}

#[tokio::test]
async fn rename_refreshes_the_profiles_showing_the_tag() {
    let f = Fixture::new("Tags Refresh").await;
    let delta = f.create_tag("Delta").await;
    let person = f.named_person("Piers", "Hollowell").await;
    let occupation = f
        .post(
            "/events",
            json!({ "event_type": "occupation", "person_id": person, "description": "Cooper" }),
        )
        .await;
    f.tag_person(delta, &person).await;
    f.tag_record(delta, RecordKind::Event, &occupation)
        .await
        .unwrap();

    tag::update(
        &f.db,
        &f.profiles,
        f.tree_id(),
        delta,
        Some("Delta Two"),
        None,
    )
    .await
    .unwrap();
    assert_eq!(f.profile_tags(&person).await, ["Delta Two"]);
    let profile = all_profiles(&f.app, &f.tree)
        .await
        .into_iter()
        .find(|profile| profile["person_id"] == person.as_str())
        .unwrap();
    assert_eq!(labels(&profile["other_events"][0]["tags"]), ["Delta Two"]);
}

#[tokio::test]
async fn tag_and_untag_every_kind_of_record() {
    let f = Fixture::new("Tags Kinds").await;
    let label = f.create_tag("Epsilon").await;
    let person = f.named_person("Piers", "Hollowell").await;
    let family = f.post("/families", json!({})).await;
    let event = f
        .post(
            "/events",
            json!({ "event_type": "occupation", "person_id": person }),
        )
        .await;
    let place = f.post("/places", json!({ "name": "Harbourfield" })).await;
    let source = f
        .post("/sources", json!({ "title": "Parish register" }))
        .await;
    let citation = f
        .post(
            "/citations",
            json!({ "source_id": source, "person_id": person }),
        )
        .await;
    let repository = f
        .post("/repositories", json!({ "name": "County archive" }))
        .await;
    let note = f
        .post(
            "/notes",
            json!({ "text": "Fictitious", "person_id": person }),
        )
        .await;
    let document = f.post("/media/document", json!({ "title": "Scan" })).await;
    let page = upload_page(&f.app, &f.tree, &document, &png(16, 16)).await;
    let page = page["id"].as_str().unwrap().to_owned();

    let cases = [
        (RecordKind::Person, person.clone(), person.clone()),
        (RecordKind::Family, family.clone(), family.clone()),
        (RecordKind::Event, event.clone(), event.clone()),
        (RecordKind::Place, place.clone(), place.clone()),
        (RecordKind::Source, source.clone(), source.clone()),
        (RecordKind::Citation, citation.clone(), citation.clone()),
        (
            RecordKind::Repository,
            repository.clone(),
            repository.clone(),
        ),
        // A page is tagged on its document.
        (RecordKind::Media, page.clone(), document.clone()),
        (RecordKind::Note, note.clone(), note.clone()),
    ];
    for (kind, target, read_back) in cases {
        f.tag_record(label, kind, &target).await.unwrap();
        // Idempotent: a second tagging adds no link.
        f.tag_record(label, kind, &target).await.unwrap();
        assert_eq!(f.tags_of(kind, &read_back).await, ["Epsilon"], "{kind:?}");
        let usage = tag::list(&f.db, f.tree_id()).await.unwrap();
        assert_eq!(usage[0].count(kind), 1, "{kind:?}");

        tag::untag_record(
            &f.db,
            &f.profiles,
            f.tree_id(),
            label,
            kind,
            Uuid::parse_str(&target).unwrap(),
        )
        .await
        .unwrap();
        tag::untag_record(
            &f.db,
            &f.profiles,
            f.tree_id(),
            label,
            kind,
            Uuid::parse_str(&target).unwrap(),
        )
        .await
        .unwrap();
        assert!(f.tags_of(kind, &read_back).await.is_empty(), "{kind:?}");
    }
}

#[tokio::test]
async fn a_page_is_tagged_on_its_document() {
    let f = Fixture::new("Tags Pages").await;
    let label = f.create_tag("Zeta").await;
    let document = f.post("/media/document", json!({ "title": "Scan" })).await;
    let page = upload_page(&f.app, &f.tree, &document, &png(16, 16)).await;
    let page = page["id"].as_str().unwrap().to_owned();

    f.tag_record(label, RecordKind::Media, &page).await.unwrap();
    assert_eq!(f.tags_of(RecordKind::Media, &document).await, ["Zeta"]);
    assert!(f.tags_of(RecordKind::Media, &page).await.is_empty());
}

#[tokio::test]
async fn records_of_another_tree_and_purged_records_are_not_found() {
    let f = Fixture::new("Tags Scope A").await;
    let other = new_tree(&f.app, "Tags Scope B").await;
    let label = f.create_tag("Eta").await;
    let person = f.named_person("Piers", "Hollowell").await;
    let foreign_person = new_person(&f.app, &other).await;

    let err = f
        .tag_record(label, RecordKind::Person, &foreign_person)
        .await
        .unwrap_err();
    assert!(matches!(err, OxidGeneError::NotFound { .. }), "{err:?}");

    let foreign_tag = tag::create(&f.db, Uuid::parse_str(&other).unwrap(), "Theta", None)
        .await
        .unwrap()
        .0
        .tag
        .id;
    let err = f
        .tag_record(foreign_tag, RecordKind::Person, &person)
        .await
        .unwrap_err();
    assert!(matches!(err, OxidGeneError::NotFound { .. }), "{err:?}");

    let (status, _) = send(
        &f.app,
        Method::DELETE,
        &f.path(&format!("/persons/{person}")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let err = f
        .tag_record(label, RecordKind::Person, &person)
        .await
        .unwrap_err();
    assert!(matches!(err, OxidGeneError::NotFound { .. }), "{err:?}");
}

#[tokio::test]
async fn tags_show_on_profile_and_search_rows_over_rest() {
    let f = Fixture::new("Tags REST").await;
    let ledger = f.create_tag("Ledger").await;
    let tagged = f.named_person("Piers", "Hollowell").await;
    let plain = f.named_person("Ada", "Hollowell").await;
    f.tag_person(ledger, &tagged).await;

    assert_eq!(f.profile_tags(&tagged).await, ["Ledger"]);
    assert!(f.profile_tags(&plain).await.is_empty());

    let search = ok(
        &f.app,
        Method::GET,
        &f.path("/persons/search?q=hollowell"),
        None,
    )
    .await;
    let row = |id: &str| {
        search["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["person_id"] == id)
            .unwrap_or_else(|| panic!("no search row for {id}"))
            .clone()
    };
    assert_eq!(labels(&row(&tagged)["tags"]), ["Ledger"]);
    assert!(row(&plain)["tags"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn tags_show_on_profile_and_search_rows_over_graphql() {
    let f = Fixture::new("Tags GraphQL").await;
    let ledger = f.create_tag("Ledger").await;
    let tagged = f.named_person("Piers", "Hollowell").await;
    f.tag_person(ledger, &tagged).await;

    let data = gql_ok(
        &f.app,
        r#"query ($tree: ID!, $person: ID!) {
            personProfile(treeId: $tree, personId: $person) { tags { label color } }
            searchPersons(treeId: $tree, query: "hollowell") { entries { personId tags { label } } }
        }"#,
        json!({ "tree": f.tree, "person": tagged }),
    )
    .await;
    assert_eq!(data["personProfile"]["tags"][0]["label"], json!("Ledger"));
    let row = data["searchPersons"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["personId"] == tagged.as_str())
        .expect("the tagged person is found");
    assert_eq!(row["tags"][0]["label"], json!("Ledger"));
}

// ── Every operation over REST and GraphQL ───────────────────────────
//
// Each operation has a REST test and its GraphQL twin with the same cases.

const CREATE_TAG: &str = r#"mutation($tree: ID!, $input: CreateTagInput!) {
    createTag(treeId: $tree, input: $input) { id label color counts { persons families } }
}"#;

const UPDATE_TAG: &str = r#"mutation($tree: ID!, $id: ID!, $input: UpdateTagInput!) {
    updateTag(treeId: $tree, id: $id, input: $input) { id label color }
}"#;

const DELETE_TAG: &str = r#"mutation($tree: ID!, $id: ID!) { deleteTag(treeId: $tree, id: $id) }"#;

const TAG_RECORD: &str = r#"mutation($tree: ID!, $tag: ID!, $kind: String!, $record: ID!) {
    tagRecord(treeId: $tree, tagId: $tag, kind: $kind, recordId: $record)
}"#;

const UNTAG_RECORD: &str = r#"mutation($tree: ID!, $tag: ID!, $kind: String!, $record: ID!) {
    untagRecord(treeId: $tree, tagId: $tag, kind: $kind, recordId: $record)
}"#;

/// The labels of a JSON array of objects with a `label` field, in order.
fn label_list(items: &Value) -> Vec<String> {
    items
        .as_array()
        .expect("an array")
        .iter()
        .map(|item| item["label"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn vocabulary_over_rest() {
    let f = Fixture::new("Tags REST Vocabulary").await;
    let (status, created) = send(
        &f.app,
        Method::POST,
        &f.path("/tags"),
        Some(json!({ "label": "Lanterne", "color": 3 })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["color"], 3);
    let lantern = created["id"].as_str().unwrap().to_owned();
    let bravo = f.post("/tags", json!({ "label": "Bravo" })).await;

    // The label folds onto the existing tag: `200`, and the tag keeps its colour.
    let (status, again) = send(
        &f.app,
        Method::POST,
        &f.path("/tags"),
        Some(json!({ "label": "  LANTÈRNE ", "color": 7 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["id"], lantern.as_str());
    assert_eq!(again["color"], 3);

    // Renaming onto another tag's label is a conflict; recolouring is not.
    let (status, body) = send(
        &f.app,
        Method::PUT,
        &f.path(&format!("/tags/{lantern}")),
        Some(json!({ "label": "bravo" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let (status, body) = send(
        &f.app,
        Method::PUT,
        &f.path(&format!("/tags/{lantern}")),
        Some(json!({ "color": 9 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        (body["label"].as_str(), body["color"].as_i64()),
        (Some("Lanterne"), Some(9))
    );

    for body in [
        json!({ "label": "   " }),
        json!({ "label": "Ok", "color": 12 }),
    ] {
        let (status, answer) = send(&f.app, Method::POST, &f.path("/tags"), Some(body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
    }

    let vocabulary = ok(&f.app, Method::GET, &f.path("/tags"), None).await;
    assert_eq!(label_list(&vocabulary), ["Bravo", "Lanterne"]);

    let (status, _) = send(
        &f.app,
        Method::DELETE,
        &f.path(&format!("/tags/{bravo}")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = send(
        &f.app,
        Method::PUT,
        &f.path(&format!("/tags/{bravo}")),
        Some(json!({ "label": "Gone" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn vocabulary_over_graphql() {
    let f = Fixture::new("Tags GraphQL Vocabulary").await;
    let created = gql_ok(
        &f.app,
        CREATE_TAG,
        json!({ "tree": f.tree, "input": { "label": "Lanterne", "color": 3 } }),
    )
    .await["createTag"]
        .clone();
    assert_eq!(created["color"], 3);
    let lantern = created["id"].as_str().unwrap().to_owned();
    let bravo = gql_ok(
        &f.app,
        CREATE_TAG,
        json!({ "tree": f.tree, "input": { "label": "Bravo" } }),
    )
    .await["createTag"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let again = gql_ok(
        &f.app,
        CREATE_TAG,
        json!({ "tree": f.tree, "input": { "label": "  LANTÈRNE ", "color": 7 } }),
    )
    .await["createTag"]
        .clone();
    assert_eq!(again["id"], lantern.as_str());
    assert_eq!(again["color"], 3);

    let response = gql(
        &f.app,
        UPDATE_TAG,
        json!({ "tree": f.tree, "id": lantern, "input": { "label": "bravo" } }),
    )
    .await;
    assert_eq!(gql_error_code(&response), "CONFLICT", "{response}");
    let recoloured = gql_ok(
        &f.app,
        UPDATE_TAG,
        json!({ "tree": f.tree, "id": lantern, "input": { "color": 9 } }),
    )
    .await["updateTag"]
        .clone();
    assert_eq!(
        (recoloured["label"].as_str(), recoloured["color"].as_i64()),
        (Some("Lanterne"), Some(9))
    );

    for input in [
        json!({ "label": "   " }),
        json!({ "label": "Ok", "color": 12 }),
    ] {
        let response = gql(
            &f.app,
            CREATE_TAG,
            json!({ "tree": f.tree, "input": input }),
        )
        .await;
        assert_eq!(gql_error_code(&response), "VALIDATION_ERROR", "{response}");
    }

    let tags = gql_ok(
        &f.app,
        r#"query($tree: ID!) { tags(treeId: $tree) { label counts { persons } } }"#,
        json!({ "tree": f.tree }),
    )
    .await["tags"]
        .clone();
    assert_eq!(label_list(&tags), ["Bravo", "Lanterne"]);

    gql_ok(&f.app, DELETE_TAG, json!({ "tree": f.tree, "id": bravo })).await;
    let response = gql(
        &f.app,
        UPDATE_TAG,
        json!({ "tree": f.tree, "id": bravo, "input": { "label": "Gone" } }),
    )
    .await;
    assert_eq!(gql_error_code(&response), "NOT_FOUND", "{response}");
}

#[tokio::test]
async fn tagging_is_idempotent_over_rest() {
    let f = Fixture::new("Tags REST Idempotent").await;
    let tag = f.post("/tags", json!({ "label": "Ledger" })).await;
    let person = f.named_person("Piers", "Hollowell").await;
    let path = f.path(&format!("/tags/{tag}/records/person/{person}"));
    for _ in 0..2 {
        let (status, body) = send(&f.app, Method::PUT, &path, None).await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    }
    let vocabulary = ok(&f.app, Method::GET, &f.path("/tags"), None).await;
    assert_eq!(vocabulary[0]["counts"]["persons"], 1);
    for _ in 0..2 {
        let (status, body) = send(&f.app, Method::DELETE, &path, None).await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    }
    let vocabulary = ok(&f.app, Method::GET, &f.path("/tags"), None).await;
    assert_eq!(vocabulary[0]["counts"]["persons"], 0);
}

#[tokio::test]
async fn tagging_is_idempotent_over_graphql() {
    let f = Fixture::new("Tags GraphQL Idempotent").await;
    let tag = f.post("/tags", json!({ "label": "Ledger" })).await;
    let person = f.named_person("Piers", "Hollowell").await;
    let variables = json!({ "tree": f.tree, "tag": tag, "kind": "person", "record": person });
    for _ in 0..2 {
        gql_ok(&f.app, TAG_RECORD, variables.clone()).await;
    }
    let counts = gql_ok(
        &f.app,
        r#"query($tree: ID!) { tags(treeId: $tree) { counts { persons } } }"#,
        json!({ "tree": f.tree }),
    )
    .await;
    assert_eq!(counts["tags"][0]["counts"]["persons"], 1);
    for _ in 0..2 {
        gql_ok(&f.app, UNTAG_RECORD, variables.clone()).await;
    }
    let counts = gql_ok(
        &f.app,
        r#"query($tree: ID!) { tags(treeId: $tree) { counts { persons } } }"#,
        json!({ "tree": f.tree }),
    )
    .await;
    assert_eq!(counts["tags"][0]["counts"]["persons"], 0);
}

#[tokio::test]
async fn merge_drops_the_links_the_target_already_has_over_rest() {
    let f = Fixture::new("Tags REST Merge").await;
    let alpha = f.post("/tags", json!({ "label": "Alpha" })).await;
    let bravo = f.post("/tags", json!({ "label": "Bravo" })).await;
    let both = f.named_person("Piers", "Hollowell").await;
    let only_alpha = f.named_person("Ada", "Hollowell").await;
    for (tag, person) in [(&alpha, &both), (&bravo, &both), (&alpha, &only_alpha)] {
        let (status, body) = send(
            &f.app,
            Method::PUT,
            &f.path(&format!("/tags/{tag}/records/person/{person}")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    }
    let (status, merged) = send(
        &f.app,
        Method::POST,
        &f.path(&format!("/tags/{alpha}/merge")),
        Some(json!({ "into": bravo })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{merged}");
    assert_eq!(merged["id"], bravo.as_str());
    assert_eq!(merged["counts"]["persons"], 2, "one link per person");
    assert_eq!(f.profile_tags(&both).await, ["Bravo"]);
    assert_eq!(f.profile_tags(&only_alpha).await, ["Bravo"]);
    let vocabulary = ok(&f.app, Method::GET, &f.path("/tags"), None).await;
    assert_eq!(label_list(&vocabulary), ["Bravo"]);
}

#[tokio::test]
async fn merge_drops_the_links_the_target_already_has_over_graphql() {
    let f = Fixture::new("Tags GraphQL Merge").await;
    let alpha = f.post("/tags", json!({ "label": "Alpha" })).await;
    let bravo = f.post("/tags", json!({ "label": "Bravo" })).await;
    let both = f.named_person("Piers", "Hollowell").await;
    let only_alpha = f.named_person("Ada", "Hollowell").await;
    for (tag, person) in [(&alpha, &both), (&bravo, &both), (&alpha, &only_alpha)] {
        gql_ok(
            &f.app,
            TAG_RECORD,
            json!({ "tree": f.tree, "tag": tag, "kind": "person", "record": person }),
        )
        .await;
    }
    let merged = gql_ok(
        &f.app,
        r#"mutation($tree: ID!, $id: ID!, $into: ID!) {
            mergeTag(treeId: $tree, id: $id, into: $into) { id counts { persons } }
        }"#,
        json!({ "tree": f.tree, "id": alpha, "into": bravo }),
    )
    .await["mergeTag"]
        .clone();
    assert_eq!(merged["id"], bravo.as_str());
    assert_eq!(merged["counts"]["persons"], 2, "one link per person");
    assert_eq!(f.profile_tags(&both).await, ["Bravo"]);
    assert_eq!(f.profile_tags(&only_alpha).await, ["Bravo"]);
    let tags = gql_ok(
        &f.app,
        r#"query($tree: ID!) { tags(treeId: $tree) { label } }"#,
        json!({ "tree": f.tree }),
    )
    .await["tags"]
        .clone();
    assert_eq!(label_list(&tags), ["Bravo"]);
}

#[tokio::test]
async fn deleting_a_tag_removes_its_links_over_rest() {
    let f = Fixture::new("Tags REST Delete").await;
    let tag = f.post("/tags", json!({ "label": "Doomed" })).await;
    let person = f.named_person("Piers", "Hollowell").await;
    f.tag_person(Uuid::parse_str(&tag).unwrap(), &person).await;
    assert_eq!(f.profile_tags(&person).await, ["Doomed"]);

    let (status, body) = send(
        &f.app,
        Method::DELETE,
        &f.path(&format!("/tags/{tag}")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert!(f.profile_tags(&person).await.is_empty());
    assert!(
        ok(&f.app, Method::GET, &f.path("/tags"), None)
            .await
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn deleting_a_tag_removes_its_links_over_graphql() {
    let f = Fixture::new("Tags GraphQL Delete").await;
    let tag = f.post("/tags", json!({ "label": "Doomed" })).await;
    let person = f.named_person("Piers", "Hollowell").await;
    f.tag_person(Uuid::parse_str(&tag).unwrap(), &person).await;
    assert_eq!(f.profile_tags(&person).await, ["Doomed"]);

    gql_ok(&f.app, DELETE_TAG, json!({ "tree": f.tree, "id": tag })).await;
    assert!(f.profile_tags(&person).await.is_empty());
    let tags = gql_ok(
        &f.app,
        r#"query($tree: ID!) { tags(treeId: $tree) { id } }"#,
        json!({ "tree": f.tree }),
    )
    .await;
    assert!(tags["tags"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn foreign_and_deleted_records_are_not_found_over_rest() {
    let f = Fixture::new("Tags REST Scope A").await;
    let other = new_tree(&f.app, "Tags REST Scope B").await;
    let tag = f.post("/tags", json!({ "label": "Eta" })).await;
    let person = f.named_person("Piers", "Hollowell").await;
    let foreign_person = new_person(&f.app, &other).await;
    let foreign_tag = ok(
        &f.app,
        Method::POST,
        &format!("/api/v1/trees/{other}/tags"),
        Some(json!({ "label": "Theta" })),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned();

    for (tag_id, record) in [(&tag, &foreign_person), (&foreign_tag, &person)] {
        let (status, body) = send(
            &f.app,
            Method::PUT,
            &f.path(&format!("/tags/{tag_id}/records/person/{record}")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    }
    let (status, _) = send(
        &f.app,
        Method::DELETE,
        &f.path(&format!("/persons/{person}")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = send(
        &f.app,
        Method::PUT,
        &f.path(&format!("/tags/{tag}/records/person/{person}")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

#[tokio::test]
async fn foreign_and_deleted_records_are_not_found_over_graphql() {
    let f = Fixture::new("Tags GraphQL Scope A").await;
    let other = new_tree(&f.app, "Tags GraphQL Scope B").await;
    let tag = f.post("/tags", json!({ "label": "Eta" })).await;
    let person = f.named_person("Piers", "Hollowell").await;
    let foreign_person = new_person(&f.app, &other).await;
    let foreign_tag = ok(
        &f.app,
        Method::POST,
        &format!("/api/v1/trees/{other}/tags"),
        Some(json!({ "label": "Theta" })),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned();

    for (tag_id, record) in [(&tag, &foreign_person), (&foreign_tag, &person)] {
        let response = gql(
            &f.app,
            TAG_RECORD,
            json!({ "tree": f.tree, "tag": tag_id, "kind": "person", "record": record }),
        )
        .await;
        assert_eq!(gql_error_code(&response), "NOT_FOUND", "{response}");
    }
    let (status, _) = send(
        &f.app,
        Method::DELETE,
        &f.path(&format!("/persons/{person}")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let response = gql(
        &f.app,
        TAG_RECORD,
        json!({ "tree": f.tree, "tag": tag, "kind": "person", "record": person }),
    )
    .await;
    assert_eq!(gql_error_code(&response), "NOT_FOUND", "{response}");
}

#[tokio::test]
async fn a_page_is_tagged_on_its_document_over_rest() {
    let f = Fixture::new("Tags REST Page").await;
    let tag = f.post("/tags", json!({ "label": "Scanned" })).await;
    let document = f.post("/media/document", json!({ "title": "Scan" })).await;
    let page = upload_page(&f.app, &f.tree, &document, &png(16, 16)).await;
    let page = page["id"].as_str().unwrap().to_owned();
    let (status, body) = send(
        &f.app,
        Method::PUT,
        &f.path(&format!("/tags/{tag}/records/media/{page}")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let media = ok(
        &f.app,
        Method::GET,
        &f.path(&format!("/media/{document}")),
        None,
    )
    .await;
    assert_eq!(label_list(&media["tags"]), ["Scanned"]);
}

#[tokio::test]
async fn a_page_is_tagged_on_its_document_over_graphql() {
    let f = Fixture::new("Tags GraphQL Page").await;
    let tag = f.post("/tags", json!({ "label": "Scanned" })).await;
    let document = f.post("/media/document", json!({ "title": "Scan" })).await;
    let page = upload_page(&f.app, &f.tree, &document, &png(16, 16)).await;
    let page = page["id"].as_str().unwrap().to_owned();
    gql_ok(
        &f.app,
        TAG_RECORD,
        json!({ "tree": f.tree, "tag": tag, "kind": "media", "record": page }),
    )
    .await;
    let media = gql_ok(
        &f.app,
        r#"query($tree: ID!, $id: ID!) { media(treeId: $tree, id: $id) { tags { label } } }"#,
        json!({ "tree": f.tree, "id": document }),
    )
    .await;
    assert_eq!(label_list(&media["media"]["tags"]), ["Scanned"]);
}

#[tokio::test]
async fn search_filters_by_tag_over_rest() {
    let f = Fixture::new("Tags REST Search").await;
    let ledger = f.post("/tags", json!({ "label": "Ledger" })).await;
    let tagged = f.named_person("Piers", "Hollowell").await;
    let plain = f.named_person("Ada", "Hollowell").await;
    f.tag_person(Uuid::parse_str(&ledger).unwrap(), &tagged)
        .await;

    let search = ok(
        &f.app,
        Method::GET,
        &f.path(&format!("/persons/search?q=hollowell&tag={ledger}")),
        None,
    )
    .await;
    let ids: Vec<&str> = search["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["person_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, [tagged.as_str()]);
    assert!(!ids.contains(&plain.as_str()));
}

#[tokio::test]
async fn search_filters_by_tag_over_graphql() {
    let f = Fixture::new("Tags GraphQL Search").await;
    let ledger = f.post("/tags", json!({ "label": "Ledger" })).await;
    let tagged = f.named_person("Piers", "Hollowell").await;
    let _plain = f.named_person("Ada", "Hollowell").await;
    f.tag_person(Uuid::parse_str(&ledger).unwrap(), &tagged)
        .await;

    let data = gql_ok(
        &f.app,
        r#"query($tree: ID!, $tags: [ID!]) {
            searchPersons(treeId: $tree, query: "hollowell", tags: $tags) { totalCount entries { personId } }
        }"#,
        json!({ "tree": f.tree, "tags": [ledger] }),
    )
    .await;
    assert_eq!(data["searchPersons"]["totalCount"], 1);
    assert_eq!(
        data["searchPersons"]["entries"][0]["personId"],
        tagged.as_str()
    );
}

#[tokio::test]
async fn an_unknown_record_kind_is_not_found_over_rest() {
    let f = Fixture::new("Tags REST Unknown Kind").await;
    let tag = f.post("/tags", json!({ "label": "Kappa" })).await;
    let person = f.named_person("Piers", "Hollowell").await;
    let records = format!("/tags/{tag}/records");

    for (method, suffix) in [
        (Method::PUT, format!("{records}/bogus/{person}")),
        (Method::DELETE, format!("{records}/bogus/{person}")),
        (Method::GET, format!("{records}?kind=bogus")),
    ] {
        let (status, body) = send(&f.app, method, &f.path(&suffix), None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
        assert_eq!(body["error"], "not_found");
    }
}

#[tokio::test]
async fn an_unknown_record_kind_is_not_found_over_graphql() {
    let f = Fixture::new("Tags GraphQL Unknown Kind").await;
    let tag = f.post("/tags", json!({ "label": "Kappa" })).await;
    let person = f.named_person("Piers", "Hollowell").await;
    let tagged_records = r#"query($tree: ID!, $tag: ID!, $kind: String!) {
        taggedRecords(treeId: $tree, tagId: $tag, kind: $kind) { totalCount }
    }"#;

    for (query, variables) in [
        (
            TAG_RECORD,
            json!({ "tree": f.tree, "tag": tag, "kind": "bogus", "record": person }),
        ),
        (
            UNTAG_RECORD,
            json!({ "tree": f.tree, "tag": tag, "kind": "bogus", "record": person }),
        ),
        (
            tagged_records,
            json!({ "tree": f.tree, "tag": tag, "kind": "bogus" }),
        ),
    ] {
        let response = gql(&f.app, query, variables).await;
        assert_eq!(gql_error_code(&response), "NOT_FOUND", "{response}");
    }
}

#[tokio::test]
async fn tagged_records_are_listed_with_labels_over_rest() {
    let f = Fixture::new("Tags REST Records").await;
    let tag = f.post("/tags", json!({ "label": "Ledger" })).await;
    let person = f.named_person("Piers", "Hollowell").await;
    let event = f
        .post(
            "/events",
            json!({ "event_type": "occupation", "person_id": person }),
        )
        .await;
    f.tag_person(Uuid::parse_str(&tag).unwrap(), &person).await;
    let (status, body) = send(
        &f.app,
        Method::PUT,
        &f.path(&format!("/tags/{tag}/records/event/{event}")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let people = ok(
        &f.app,
        Method::GET,
        &f.path(&format!("/tags/{tag}/records?kind=person")),
        None,
    )
    .await;
    assert_eq!(people["total_count"], 1);
    assert_eq!(people["edges"][0]["node"]["label"], "Piers Hollowell");
    assert_eq!(people["edges"][0]["node"]["kind"], "person");
    // A person carries her name parts, as the usage lists read them.
    assert_eq!(people["edges"][0]["node"]["person"]["given_names"], "Piers");
    assert_eq!(people["edges"][0]["node"]["person"]["surname"], "Hollowell");

    let events = ok(
        &f.app,
        Method::GET,
        &f.path(&format!("/tags/{tag}/records?kind=event")),
        None,
    )
    .await;
    assert_eq!(events["edges"][0]["node"]["id"], event.as_str());
    assert_eq!(events["edges"][0]["node"]["person_id"], person.as_str());
    assert!(events["edges"][0]["node"]["person"].is_null());
}

#[tokio::test]
async fn tagged_records_are_listed_with_labels_over_graphql() {
    let f = Fixture::new("Tags GraphQL Records").await;
    let tag = f.post("/tags", json!({ "label": "Ledger" })).await;
    let person = f.named_person("Piers", "Hollowell").await;
    f.tag_person(Uuid::parse_str(&tag).unwrap(), &person).await;

    let data = gql_ok(
        &f.app,
        r#"query($tree: ID!, $tag: ID!) {
            taggedRecords(treeId: $tree, tagId: $tag, kind: "person") {
                totalCount edges { node { id kind label personId familyId person { givenNames surname } } }
            }
        }"#,
        json!({ "tree": f.tree, "tag": tag }),
    )
    .await;
    let node = &data["taggedRecords"]["edges"][0]["node"];
    assert_eq!(data["taggedRecords"]["totalCount"], 1);
    assert_eq!(node["label"], "Piers Hollowell");
    assert_eq!(node["kind"], "PERSON");
    assert_eq!(node["id"], person.as_str());
    assert_eq!(node["person"]["givenNames"], "Piers");
    assert_eq!(node["person"]["surname"], "Hollowell");
}

#[tokio::test]
async fn merging_persons_keeps_both_persons_tags_over_rest() {
    let f = Fixture::new("Tags REST Person Merge").await;
    let alpha = f.post("/tags", json!({ "label": "Alpha" })).await;
    let bravo = f.post("/tags", json!({ "label": "Bravo" })).await;
    let kept = f.named_person("Piers", "Hollowell").await;
    let duplicate = f.named_person("Piers", "Hollowel").await;
    f.tag_person(Uuid::parse_str(&alpha).unwrap(), &kept).await;
    f.tag_person(Uuid::parse_str(&bravo).unwrap(), &duplicate)
        .await;

    let merged = ok(
        &f.app,
        Method::POST,
        &f.path(&format!("/persons/{kept}/merge")),
        Some(json!({ "duplicate_id": duplicate })),
    )
    .await;
    assert_eq!(merged["id"], kept.as_str());
    assert_eq!(f.profile_tags(&kept).await, ["Alpha", "Bravo"]);
}

#[tokio::test]
async fn merging_persons_keeps_both_persons_tags_over_graphql() {
    let f = Fixture::new("Tags GraphQL Person Merge").await;
    let alpha = f.post("/tags", json!({ "label": "Alpha" })).await;
    let bravo = f.post("/tags", json!({ "label": "Bravo" })).await;
    let kept = f.named_person("Piers", "Hollowell").await;
    let duplicate = f.named_person("Piers", "Hollowel").await;
    f.tag_person(Uuid::parse_str(&alpha).unwrap(), &kept).await;
    f.tag_person(Uuid::parse_str(&bravo).unwrap(), &duplicate)
        .await;

    gql_ok(
        &f.app,
        r#"mutation($tree: ID!, $person: ID!, $duplicate: ID!) {
            mergePersons(treeId: $tree, personId: $person, duplicateId: $duplicate) { id }
        }"#,
        json!({ "tree": f.tree, "person": kept, "duplicate": duplicate }),
    )
    .await;
    assert_eq!(f.profile_tags(&kept).await, ["Alpha", "Bravo"]);
}

#[tokio::test]
async fn citations_of_a_deleted_source_leave_the_counts_over_rest() {
    let f = Fixture::new("Tags REST Deleted Source").await;
    let tag = f.post("/tags", json!({ "label": "Sourced" })).await;
    let source = f
        .post("/sources", json!({ "title": "Fictional register" }))
        .await;
    let citation = f.post("/citations", json!({ "source_id": source })).await;
    let (status, body) = send(
        &f.app,
        Method::PUT,
        &f.path(&format!("/tags/{tag}/records/citation/{citation}")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let vocabulary = ok(&f.app, Method::GET, &f.path("/tags"), None).await;
    let entry = vocabulary
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == tag.as_str())
        .unwrap();
    assert_eq!(entry["counts"]["citations"], 1);

    let (status, body) = send(
        &f.app,
        Method::DELETE,
        &f.path(&format!("/sources/{source}")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let vocabulary = ok(&f.app, Method::GET, &f.path("/tags"), None).await;
    let entry = vocabulary
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == tag.as_str())
        .unwrap();
    assert_eq!(entry["counts"]["citations"], 0);
    let listed = ok(
        &f.app,
        Method::GET,
        &f.path(&format!("/tags/{tag}/records?kind=citation")),
        None,
    )
    .await;
    assert_eq!(listed["total_count"], 0);
    assert!(listed["edges"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn citations_of_a_deleted_source_leave_the_counts_over_graphql() {
    let f = Fixture::new("Tags GraphQL Deleted Source").await;
    let tag = f.post("/tags", json!({ "label": "Sourced" })).await;
    let source = gql_ok(
        &f.app,
        r#"mutation($tree: ID!) { createSource(treeId: $tree, input: { title: "Fictional register" }) { id } }"#,
        json!({ "tree": f.tree }),
    )
    .await["createSource"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let citation = gql_ok(
        &f.app,
        r#"mutation($tree: ID!, $source: String!) { createCitation(treeId: $tree, input: { sourceId: $source }) { id } }"#,
        json!({ "tree": f.tree, "source": source }),
    )
    .await["createCitation"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    gql_ok(
        &f.app,
        TAG_RECORD,
        json!({ "tree": f.tree, "tag": tag, "kind": "citation", "record": citation }),
    )
    .await;

    let counted = gql_ok(
        &f.app,
        r#"query($tree: ID!) { tags(treeId: $tree) { id counts { citations } } }"#,
        json!({ "tree": f.tree }),
    )
    .await;
    assert_eq!(counted["tags"][0]["counts"]["citations"], 1);

    gql_ok(
        &f.app,
        r#"mutation($tree: ID!, $id: ID!) { deleteSource(treeId: $tree, id: $id) }"#,
        json!({ "tree": f.tree, "id": source }),
    )
    .await;

    let counted = gql_ok(
        &f.app,
        r#"query($tree: ID!) { tags(treeId: $tree) { id counts { citations } } }"#,
        json!({ "tree": f.tree }),
    )
    .await;
    assert_eq!(counted["tags"][0]["counts"]["citations"], 0);
    let listed = gql_ok(
        &f.app,
        r#"query($tree: ID!, $tag: ID!) {
            taggedRecords(treeId: $tree, tagId: $tag, kind: "citation") { totalCount }
        }"#,
        json!({ "tree": f.tree, "tag": tag }),
    )
    .await;
    assert_eq!(listed["taggedRecords"]["totalCount"], 0);
}

/// Two persons and their marriage, an occupation of the first with a place,
/// a description and a source, a citation and a note on that occupation, and
/// a note on the marriage. Returns the ids the summary tests read.
struct SummaryFixture {
    f: Fixture,
    tag: String,
    husband: String,
    wife: String,
    family: String,
    occupation: String,
    marriage: String,
    citation: String,
    note: String,
}

impl SummaryFixture {
    async fn new(name: &str) -> Self {
        let f = Fixture::new(name).await;
        let tag = f.post("/tags", json!({ "label": "Ledger" })).await;
        let husband = f.named_person("Piers", "Hollowell").await;
        let wife = f.named_person("Ada", "Marrow").await;
        let family = f.post("/families", json!({})).await;
        couple(&f, &family, &husband, &wife).await;
        let place = f.post("/places", json!({ "name": "Paris" })).await;
        let occupation = f
            .post(
                "/events",
                json!({
                    "event_type": "occupation",
                    "person_id": husband,
                    "date_value": "1938",
                    "date_qualifier": "about",
                    "place_id": place,
                    "description": "Police inspector",
                }),
            )
            .await;
        let marriage = f
            .post(
                "/events",
                json!({ "event_type": "marriage", "family_id": family, "date_value": "1913" }),
            )
            .await;
        let source = f
            .post("/sources", json!({ "title": "Parish register" }))
            .await;
        let citation = f
            .post(
                "/citations",
                json!({ "source_id": source, "event_id": occupation, "page": "folio 12" }),
            )
            .await;
        let note = f
            .post(
                "/notes",
                json!({ "text": "First line of the remark\nSecond line", "event_id": occupation }),
            )
            .await;
        let marriage_note = f
            .post(
                "/notes",
                json!({ "text": "Marriage remark", "event_id": marriage }),
            )
            .await;
        let fixture = Self {
            f,
            tag,
            husband,
            wife,
            family,
            occupation,
            marriage,
            citation,
            note,
        };
        tag_summaries(&fixture, &marriage_note).await;
        fixture
    }

    /// The listed item of `kind` with `id` on the tag's first page.
    async fn node_over_rest(&self, kind: &str, id: &str) -> Value {
        let page = ok(
            &self.f.app,
            Method::GET,
            &self
                .f
                .path(&format!("/tags/{}/records?kind={kind}", self.tag)),
            None,
        )
        .await;
        page["edges"]
            .as_array()
            .unwrap()
            .iter()
            .map(|edge| edge["node"].clone())
            .find(|node| node["id"] == id)
            .unwrap_or_else(|| panic!("{kind} {id} is listed: {page}"))
    }
}

#[tokio::test]
async fn tagged_records_carry_their_summaries_over_rest() {
    let s = SummaryFixture::new("Tags REST Summaries").await;

    let occupation = s.node_over_rest("event", &s.occupation).await;
    assert_eq!(occupation["event"]["event_type"], "occupation");
    assert_eq!(occupation["event"]["date_value"], "1938");
    assert_eq!(occupation["event"]["date_qualifier"], "about");
    assert_eq!(occupation["event"]["calendar"], "gregorian");
    assert_eq!(occupation["event"]["place_name"], "Paris");
    assert_eq!(occupation["event"]["description"], "Police inspector");
    // The event's sources are not part of its summary.
    assert!(occupation["event"].get("citations").is_none());
    assert_eq!(occupation["persons"][0]["person_id"], s.husband.as_str());
    assert_eq!(occupation["persons"][0]["given_names"], "Piers");
    assert_eq!(occupation["persons"][0]["surname"], "Hollowell");
    assert_eq!(occupation["persons"].as_array().unwrap().len(), 1);
    assert!(occupation["person"].is_null());

    let marriage = s.node_over_rest("event", &s.marriage).await;
    assert_eq!(marriage["event"]["event_type"], "marriage");
    assert_eq!(marriage["event"]["date_value"], "1913");
    assert_eq!(marriage["event"]["date_qualifier"], "exact");
    assert!(marriage["event"]["place_name"].is_null());
    assert!(marriage["event"]["description"].is_null());
    // Both spouses of the couple, in their order.
    assert_eq!(marriage["persons"][0]["person_id"], s.husband.as_str());
    assert_eq!(marriage["persons"][1]["person_id"], s.wife.as_str());

    let citation = s.node_over_rest("citation", &s.citation).await;
    assert_eq!(citation["label"], "Parish register");
    assert_eq!(citation["citation"]["source_title"], "Parish register");
    assert_eq!(citation["citation"]["page"], "folio 12");
    assert_eq!(citation["persons"][0]["person_id"], s.husband.as_str());
    assert!(citation["event"].is_null());
    assert!(occupation["citation"].is_null());

    let note = s.node_over_rest("note", &s.note).await;
    assert_eq!(note["label"], "First line of the remark");
    assert_eq!(note["persons"][0]["person_id"], s.husband.as_str());
    assert!(note["citation"].is_null());

    let family = s.node_over_rest("family", &s.family).await;
    assert!(family["event"].is_null());
    assert_eq!(family["persons"][0]["person_id"], s.husband.as_str());
    assert_eq!(family["persons"][1]["surname"], "Marrow");
    assert_eq!(family["persons"][1]["given_names"], "Ada");

    let person = s.node_over_rest("person", &s.husband).await;
    assert!(person["persons"].is_null());
    assert!(person["event"].is_null());
    assert_eq!(person["person"]["given_names"], "Piers");
}

#[tokio::test]
async fn a_long_note_is_listed_by_its_first_line_cut_to_an_excerpt() {
    let f = Fixture::new("Tags Note Excerpt").await;
    let tag = f.post("/tags", json!({ "label": "Ledger" })).await;
    let long = "x".repeat(150);
    let note = f
        .post("/notes", json!({ "text": format!("  {long}\nsecond") }))
        .await;
    let (status, body) = send(
        &f.app,
        Method::PUT,
        &f.path(&format!("/tags/{tag}/records/note/{note}")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let page = ok(
        &f.app,
        Method::GET,
        &f.path(&format!("/tags/{tag}/records?kind=note")),
        None,
    )
    .await;
    let label = page["edges"][0]["node"]["label"].as_str().unwrap();
    assert_eq!(label, format!("{}\u{2026}", "x".repeat(100)));
    assert!(
        page["edges"][0]["node"]["persons"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn tagged_records_carry_their_summaries_over_graphql() {
    let s = SummaryFixture::new("Tags GraphQL Summaries").await;
    let data = gql_ok(
        &s.f.app,
        r#"query($tree: ID!, $tag: ID!) {
            events: taggedRecords(treeId: $tree, tagId: $tag, kind: "event") {
                edges { node { id label event { eventType dateValue dateQualifier dateValue2 calendar placeName description } persons { personId givenNames surname birthYear birthQualifier deathYear } } }
            }
            citations: taggedRecords(treeId: $tree, tagId: $tag, kind: "citation") {
                edges { node { id label citation { sourceTitle page } event { eventType } persons { personId givenNames surname } } }
            }
            notes: taggedRecords(treeId: $tree, tagId: $tag, kind: "note") {
                edges { node { id label persons { personId } } }
            }
            families: taggedRecords(treeId: $tree, tagId: $tag, kind: "family") {
                edges { node { id label event { eventType } persons { personId givenNames surname } } }
            }
            people: taggedRecords(treeId: $tree, tagId: $tag, kind: "person") {
                edges { node { id person { givenNames } persons { personId } event { eventType } } }
            }
        }"#,
        json!({ "tree": s.f.tree, "tag": s.tag }),
    )
    .await;
    let node = |connection: &str, id: &str| -> Value {
        data[connection]["edges"]
            .as_array()
            .unwrap()
            .iter()
            .map(|edge| edge["node"].clone())
            .find(|node| node["id"] == id)
            .unwrap_or_else(|| panic!("{connection} {id} is listed: {data}"))
    };

    let occupation = node("events", &s.occupation);
    assert_eq!(occupation["event"]["eventType"], "OCCUPATION");
    assert_eq!(occupation["event"]["dateValue"], "1938");
    assert_eq!(occupation["event"]["dateQualifier"], "ABOUT");
    assert_eq!(occupation["event"]["calendar"], "GREGORIAN");
    assert_eq!(occupation["event"]["placeName"], "Paris");
    assert_eq!(occupation["event"]["description"], "Police inspector");
    assert!(occupation["event"]["dateValue2"].is_null());
    assert_eq!(occupation["persons"][0]["personId"], s.husband.as_str());
    assert_eq!(occupation["persons"][0]["givenNames"], "Piers");
    assert_eq!(occupation["persons"][0]["surname"], "Hollowell");

    let marriage = node("events", &s.marriage);
    assert_eq!(marriage["event"]["eventType"], "MARRIAGE");
    assert!(marriage["event"]["placeName"].is_null());
    assert_eq!(marriage["persons"][0]["personId"], s.husband.as_str());
    assert_eq!(marriage["persons"][1]["personId"], s.wife.as_str());

    let citation = node("citations", &s.citation);
    assert_eq!(citation["label"], "Parish register");
    assert_eq!(citation["citation"]["sourceTitle"], "Parish register");
    assert_eq!(citation["citation"]["page"], "folio 12");
    assert_eq!(citation["persons"][0]["personId"], s.husband.as_str());

    let note = node("notes", &s.note);
    assert_eq!(note["label"], "First line of the remark");
    assert_eq!(note["persons"][0]["personId"], s.husband.as_str());

    let family = node("families", &s.family);
    assert!(family["event"].is_null());
    assert_eq!(family["persons"][0]["personId"], s.husband.as_str());
    assert_eq!(family["persons"][1]["surname"], "Marrow");

    let person = node("people", &s.husband);
    assert_eq!(person["person"]["givenNames"], "Piers");
    assert!(person["persons"].is_null());
    assert!(person["event"].is_null());
}

/// Makes `husband` and `wife` the spouses of `family`, in that order.
async fn couple(f: &Fixture, family: &str, husband: &str, wife: &str) {
    for (person, role, order) in [(husband, "husband", 0), (wife, "wife", 1)] {
        f.post(
            &format!("/families/{family}/spouses"),
            json!({ "person_id": person, "role": role, "sort_order": order }),
        )
        .await;
    }
}

/// Puts the tag of `fixture` on its husband, its events, its citation, its
/// notes and its couple.
async fn tag_summaries(fixture: &SummaryFixture, marriage_note: &str) {
    for (kind, id) in [
        ("person", fixture.husband.as_str()),
        ("event", fixture.occupation.as_str()),
        ("event", fixture.marriage.as_str()),
        ("citation", fixture.citation.as_str()),
        ("note", fixture.note.as_str()),
        ("note", marriage_note),
        ("family", fixture.family.as_str()),
    ] {
        let (status, body) = send(
            &fixture.f.app,
            Method::PUT,
            &fixture
                .f
                .path(&format!("/tags/{}/records/{kind}/{id}", fixture.tag)),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    }
}

#[tokio::test]
async fn a_note_with_blank_text_is_refused_so_no_list_shows_an_empty_note() {
    let f = Fixture::new("Tags Blank Note").await;
    let (status, body) = send(
        &f.app,
        Method::POST,
        &f.path("/notes"),
        Some(json!({ "text": "  \n \t" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}
