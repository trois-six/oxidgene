//! The reads of events, sources and citations carry their tags, over REST and
//! GraphQL, and a tag change on an event refreshes the profiles that show it.
//! Each operation has a REST test and its GraphQL twin with the same cases.
//!
//! All data is fictitious.

mod common;

use axum::Router;
use axum::http::{Method, StatusCode};
use common::{all_profiles, app_on, gql_ok, new_person, new_tree, ok, send, setup_db};
use oxidgene_api::profile::ProfileService;
use oxidgene_api::service::tag;
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

    async fn profile(&self, person: &str) -> Value {
        all_profiles(&self.app, &self.tree)
            .await
            .into_iter()
            .find(|profile| profile["person_id"] == person)
            .expect("the person has a profile")
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

// ── REST ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn event_source_and_citation_reads_carry_their_tags_over_rest() {
    let f = Fixture::new("Tags Reads REST").await;
    // Created in this order, read back sorted by folded label.
    let bravo = f.create_tag("Bravo").await;
    let alpha = f.create_tag("alpha").await;
    let person = f.named_person("Piers", "Hollowell").await;
    let event = f
        .post(
            "/events",
            json!({ "event_type": "occupation", "person_id": person, "description": "Cooper" }),
        )
        .await;
    let plain = f
        .post(
            "/events",
            json!({ "event_type": "occupation", "person_id": person }),
        )
        .await;
    let source = f
        .post("/sources", json!({ "title": "Parish register" }))
        .await;
    let citation = f
        .post(
            "/citations",
            json!({ "source_id": source, "person_id": person, "page": "f. 12" }),
        )
        .await;
    f.tag_over_rest(bravo, "event", &event).await;
    f.tag_over_rest(alpha, "event", &event).await;
    f.tag_over_rest(bravo, "source", &source).await;
    f.tag_over_rest(alpha, "citation", &citation).await;

    let read = ok(
        &f.app,
        Method::GET,
        &f.path(&format!("/events/{event}")),
        None,
    )
    .await;
    assert_eq!(labels(&read["tags"]), ["alpha", "Bravo"]);
    let listed = ok(
        &f.app,
        Method::GET,
        &f.path(&format!("/events?person_id={person}")),
        None,
    )
    .await;
    assert_eq!(labels(&node(&listed, &event)["tags"]), ["alpha", "Bravo"]);
    assert!(labels(&node(&listed, &plain)["tags"]).is_empty());

    // An update answers with the tags the record still carries.
    let updated = ok(
        &f.app,
        Method::PUT,
        &f.path(&format!("/events/{event}")),
        Some(json!({ "description": "Master cooper" })),
    )
    .await;
    assert_eq!(updated["description"], "Master cooper");
    assert_eq!(labels(&updated["tags"]), ["alpha", "Bravo"]);

    let read = ok(
        &f.app,
        Method::GET,
        &f.path(&format!("/sources/{source}")),
        None,
    )
    .await;
    assert_eq!(labels(&read["tags"]), ["Bravo"]);
    let renamed = ok(
        &f.app,
        Method::PUT,
        &f.path(&format!("/sources/{source}")),
        Some(json!({ "title": "Parish register, copy" })),
    )
    .await;
    assert_eq!(labels(&renamed["tags"]), ["Bravo"]);
    let listed = ok(&f.app, Method::GET, &f.path("/sources"), None).await;
    assert_eq!(labels(&node(&listed, &source)["tags"]), ["Bravo"]);

    let listed = ok(&f.app, Method::GET, &f.path("/citations"), None).await;
    assert_eq!(labels(&node(&listed, &citation)["tags"]), ["alpha"]);
}

#[tokio::test]
async fn a_family_event_tag_refreshes_both_spouse_profiles_over_rest() {
    let f = Fixture::new("Tags Family Event REST").await;
    let ledger = f.create_tag("Ledger").await;
    let husband = f.named_person("Piers", "Hollowell").await;
    let wife = f.named_person("Ada", "Marrow").await;
    let family = f.post("/families", json!({})).await;
    for (person, role) in [(&husband, "husband"), (&wife, "wife")] {
        ok(
            &f.app,
            Method::POST,
            &f.path(&format!("/families/{family}/spouses")),
            Some(json!({ "person_id": person, "role": role, "sort_order": 0 })),
        )
        .await;
    }
    let marriage = f
        .post(
            "/events",
            json!({ "event_type": "marriage", "family_id": family }),
        )
        .await;
    f.tag_over_rest(ledger, "event", &marriage).await;

    for spouse in [&husband, &wife] {
        let profile = f.profile(spouse).await;
        let link = &profile["families_as_spouse"][0];
        assert_eq!(labels(&link["marriage"]["tags"]), ["Ledger"], "{spouse}");
    }

    tag::update(
        &f.db,
        &f.profiles,
        f.tree_id(),
        ledger,
        Some("Ledger two"),
        None,
    )
    .await
    .unwrap();
    for spouse in [&husband, &wife] {
        let profile = f.profile(spouse).await;
        let link = &profile["families_as_spouse"][0];
        assert_eq!(
            labels(&link["marriage"]["tags"]),
            ["Ledger two"],
            "{spouse}"
        );
    }

    let (status, body) = send(
        &f.app,
        Method::DELETE,
        &f.path(&format!("/tags/{ledger}/records/event/{marriage}")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    for spouse in [&husband, &wife] {
        let profile = f.profile(spouse).await;
        let link = &profile["families_as_spouse"][0];
        assert!(labels(&link["marriage"]["tags"]).is_empty(), "{spouse}");
    }
}

#[tokio::test]
async fn deleted_records_are_not_read_over_rest() {
    let f = Fixture::new("Tags Deleted REST").await;
    let tag = f.create_tag("Doubtful").await;
    let person = f.named_person("Piers", "Hollowell").await;
    let event = f
        .post(
            "/events",
            json!({ "event_type": "occupation", "person_id": person }),
        )
        .await;
    let source = f
        .post("/sources", json!({ "title": "Fictional register" }))
        .await;
    f.tag_over_rest(tag, "event", &event).await;
    f.tag_over_rest(tag, "source", &source).await;

    let (status, body) = send(
        &f.app,
        Method::DELETE,
        &f.path(&format!("/events/{event}")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (status, _) = send(
        &f.app,
        Method::GET,
        &f.path(&format!("/events/{event}")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let listed = ok(
        &f.app,
        Method::GET,
        &f.path(&format!("/events?person_id={person}")),
        None,
    )
    .await;
    assert!(listed["edges"].as_array().unwrap().is_empty());

    let (status, body) = send(
        &f.app,
        Method::DELETE,
        &f.path(&format!("/sources/{source}")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (status, _) = send(
        &f.app,
        Method::GET,
        &f.path(&format!("/sources/{source}")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn records_of_another_tree_are_not_read_over_rest() {
    let f = Fixture::new("Tags Foreign REST").await;
    let other = new_tree(&f.app, "Tags Foreign Other").await;
    let tag = f.create_tag("Foreign").await;
    let person = f.named_person("Piers", "Hollowell").await;
    let event = f
        .post(
            "/events",
            json!({ "event_type": "occupation", "person_id": person }),
        )
        .await;
    f.tag_over_rest(tag, "event", &event).await;

    let (status, _) = send(
        &f.app,
        Method::GET,
        &format!("/api/v1/trees/{other}/events/{event}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ── GraphQL twins ────────────────────────────────────────────────────

const EVENT_TAGS: &str = r#"query ($tree: ID!, $id: ID!) {
    event(treeId: $tree, id: $id) { tags { label } citations { tags { label } } }
}"#;

#[tokio::test]
async fn event_source_and_citation_reads_carry_their_tags_over_graphql() {
    let f = Fixture::new("Tags Reads GraphQL").await;
    let bravo = f.create_tag("Bravo").await;
    let alpha = f.create_tag("alpha").await;
    let person = f.named_person("Piers", "Hollowell").await;
    let created = gql_ok(
        &f.app,
        r#"mutation($tree: ID!, $person: String!) {
            createEvent(treeId: $tree, input: { eventType: OCCUPATION, personId: $person, description: "Cooper" }) { id }
        }"#,
        json!({ "tree": f.tree, "person": person }),
    )
    .await;
    let event = created["createEvent"]["id"].as_str().unwrap().to_owned();
    let created = gql_ok(
        &f.app,
        r#"mutation($tree: ID!) { createSource(treeId: $tree, input: { title: "Parish register" }) { id } }"#,
        json!({ "tree": f.tree }),
    )
    .await;
    let source = created["createSource"]["id"].as_str().unwrap().to_owned();
    let created = gql_ok(
        &f.app,
        r#"mutation($tree: ID!, $source: String!, $person: String!) {
            createCitation(treeId: $tree, input: { sourceId: $source, personId: $person }) { id }
        }"#,
        json!({ "tree": f.tree, "source": source, "person": person }),
    )
    .await;
    let citation = created["createCitation"]["id"].as_str().unwrap().to_owned();
    f.tag_over_rest(bravo, "event", &event).await;
    f.tag_over_rest(alpha, "event", &event).await;
    f.tag_over_rest(bravo, "source", &source).await;
    f.tag_over_rest(alpha, "citation", &citation).await;

    let data = gql_ok(&f.app, EVENT_TAGS, json!({ "tree": f.tree, "id": event })).await;
    assert_eq!(labels(&data["event"]["tags"]), ["alpha", "Bravo"]);
    let data = gql_ok(
        &f.app,
        r#"query ($tree: ID!, $id: ID!, $person: ID!) {
            events(treeId: $tree, personId: $person) { edges { node { id tags { label } } } }
            source(treeId: $tree, id: $id) { tags { label } citations { tags { label } } }
            sources(treeId: $tree) { edges { node { id tags { label } } } }
            citations(treeId: $tree) { edges { node { id tags { label } } } }
        }"#,
        json!({ "tree": f.tree, "id": source, "person": person }),
    )
    .await;
    assert_eq!(
        labels(&node(&data["events"], &event)["tags"]),
        ["alpha", "Bravo"]
    );
    assert_eq!(labels(&data["source"]["tags"]), ["Bravo"]);
    assert_eq!(labels(&data["source"]["citations"][0]["tags"]), ["alpha"]);
    assert_eq!(labels(&node(&data["sources"], &source)["tags"]), ["Bravo"]);
    assert_eq!(
        labels(&node(&data["citations"], &citation)["tags"]),
        ["alpha"]
    );
}

#[tokio::test]
async fn deleted_records_are_not_read_over_graphql() {
    let f = Fixture::new("Tags Deleted GraphQL").await;
    let tag = f.create_tag("Doubtful").await;
    let person = f.named_person("Piers", "Hollowell").await;
    let created = gql_ok(
        &f.app,
        r#"mutation($tree: ID!, $person: String!) {
            createEvent(treeId: $tree, input: { eventType: OCCUPATION, personId: $person }) { id }
        }"#,
        json!({ "tree": f.tree, "person": person }),
    )
    .await;
    let event = created["createEvent"]["id"].as_str().unwrap().to_owned();
    f.tag_over_rest(tag, "event", &event).await;
    gql_ok(
        &f.app,
        r#"mutation($tree: ID!, $id: ID!) { deleteEvent(treeId: $tree, id: $id) }"#,
        json!({ "tree": f.tree, "id": event }),
    )
    .await;
    let data = gql_ok(&f.app, EVENT_TAGS, json!({ "tree": f.tree, "id": event })).await;
    assert!(data["event"].is_null());
    let data = gql_ok(
        &f.app,
        r#"query ($tree: ID!, $person: ID!) {
            events(treeId: $tree, personId: $person) { totalCount edges { node { id } } }
        }"#,
        json!({ "tree": f.tree, "person": person }),
    )
    .await;
    assert!(data["events"]["edges"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn a_family_event_tag_shows_on_the_spouse_profile_over_graphql() {
    let f = Fixture::new("Tags Family Event GraphQL").await;
    let ledger = f.create_tag("Ledger").await;
    let husband = f.named_person("Piers", "Hollowell").await;
    let wife = f.named_person("Ada", "Marrow").await;
    let family = f.post("/families", json!({})).await;
    for (person, role) in [(&husband, "husband"), (&wife, "wife")] {
        ok(
            &f.app,
            Method::POST,
            &f.path(&format!("/families/{family}/spouses")),
            Some(json!({ "person_id": person, "role": role, "sort_order": 0 })),
        )
        .await;
    }
    let marriage = f
        .post(
            "/events",
            json!({ "event_type": "marriage", "family_id": family }),
        )
        .await;
    f.tag_over_rest(ledger, "event", &marriage).await;

    let data = gql_ok(
        &f.app,
        r#"query ($tree: ID!, $person: ID!) {
            personProfile(treeId: $tree, personId: $person) {
                familiesAsSpouse { marriage { tags { label } } }
            }
        }"#,
        json!({ "tree": f.tree, "person": wife }),
    )
    .await;
    assert_eq!(
        labels(&data["personProfile"]["familiesAsSpouse"][0]["marriage"]["tags"]),
        ["Ledger"]
    );
}

/// The entry of a flat list of dictionary sources whose `id` is `id`.
fn entry_of<'a>(entries: &'a [Value], id: &str) -> &'a Value {
    entries
        .iter()
        .find(|entry| entry["id"] == id)
        .unwrap_or_else(|| panic!("no dictionary entry {id}"))
}

#[tokio::test]
async fn dictionary_source_reads_carry_their_tags_over_rest() {
    let f = Fixture::new("Tags Dictionary REST").await;
    let bravo = f.create_tag("Bravo").await;
    let alpha = f.create_tag("alpha").await;
    let person = f.named_person("Piers", "Hollowell").await;
    let tagged = f
        .post("/sources", json!({ "title": "Parish register" }))
        .await;
    let plain = f
        .post("/sources", json!({ "title": "Fictional register" }))
        .await;
    f.post(
        "/citations",
        json!({ "source_id": tagged, "person_id": person, "page": "f. 12" }),
    )
    .await;
    f.tag_over_rest(bravo, "source", &tagged).await;
    f.tag_over_rest(alpha, "source", &tagged).await;

    let listed = ok(&f.app, Method::GET, &f.path("/dictionary/sources"), None).await;
    let entries = listed.as_array().expect("a flat list");
    assert_eq!(
        labels(&entry_of(entries, &tagged)["tags"]),
        ["alpha", "Bravo"]
    );
    assert!(labels(&entry_of(entries, &plain)["tags"]).is_empty());
    assert_eq!(entry_of(entries, &tagged)["count"], 1);

    let prefixed = ok(
        &f.app,
        Method::GET,
        &f.path("/dictionary/sources?prefix=parish"),
        None,
    )
    .await;
    let prefixed = prefixed.as_array().expect("a flat list");
    assert_eq!(prefixed.len(), 1);
    assert_eq!(labels(&prefixed[0]["tags"]), ["alpha", "Bravo"]);

    // The drill-down's final flat list carries the tags too.
    let drill = ok(
        &f.app,
        Method::GET,
        &f.path("/dictionary/sources/groups?prefix=parish"),
        None,
    )
    .await;
    let sources = drill["sources"].as_array().expect("the final flat list");
    assert_eq!(
        labels(&entry_of(sources, &tagged)["tags"]),
        ["alpha", "Bravo"]
    );
}

#[tokio::test]
async fn dictionary_source_reads_carry_their_tags_over_graphql() {
    let f = Fixture::new("Tags Dictionary GraphQL").await;
    let bravo = f.create_tag("Bravo").await;
    let alpha = f.create_tag("alpha").await;
    let created = gql_ok(
        &f.app,
        r#"mutation($tree: ID!) { createSource(treeId: $tree, input: { title: "Parish register" }) { id } }"#,
        json!({ "tree": f.tree }),
    )
    .await;
    let tagged = created["createSource"]["id"].as_str().unwrap().to_owned();
    let created = gql_ok(
        &f.app,
        r#"mutation($tree: ID!) { createSource(treeId: $tree, input: { title: "Fictional register" }) { id } }"#,
        json!({ "tree": f.tree }),
    )
    .await;
    let plain = created["createSource"]["id"].as_str().unwrap().to_owned();
    f.tag_over_rest(bravo, "source", &tagged).await;
    f.tag_over_rest(alpha, "source", &tagged).await;

    let data = gql_ok(
        &f.app,
        r#"query ($tree: ID!, $prefix: String!) {
            dictionarySources(treeId: $tree) { source { id tags { label } } count }
            dictionarySourceDrill(treeId: $tree, prefix: $prefix) {
                sources { source { id tags { label } } }
            }
        }"#,
        json!({ "tree": f.tree, "prefix": "parish" }),
    )
    .await;
    let listed = data["dictionarySources"].as_array().expect("a flat list");
    let entry = |id: &str| {
        listed
            .iter()
            .find(|entry| entry["source"]["id"] == id)
            .unwrap_or_else(|| panic!("no dictionary entry {id}"))
    };
    assert_eq!(
        labels(&entry(&tagged)["source"]["tags"]),
        ["alpha", "Bravo"]
    );
    assert!(labels(&entry(&plain)["source"]["tags"]).is_empty());

    let drilled = data["dictionarySourceDrill"]["sources"]
        .as_array()
        .expect("the final flat list");
    let drilled_entry = drilled
        .iter()
        .find(|entry| entry["source"]["id"] == tagged.as_str())
        .expect("the tagged source is in the drill-down");
    assert_eq!(labels(&drilled_entry["source"]["tags"]), ["alpha", "Bravo"]);
}
