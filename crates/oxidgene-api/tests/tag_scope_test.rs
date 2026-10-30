//! The statistics and the anomalies of a tree limited to a tag
//! (`docs/tags.md` §2.1 and §2.2). Each operation has a REST test and its
//! GraphQL twin with the same cases.
//!
//! All data is fictitious.

use crate::common;

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

    /// Put `tag` on a person over REST, expecting success.
    async fn tag_person(&self, tag: Uuid, person: &str) {
        let (status, body) = send(
            &self.app,
            Method::PUT,
            &self.path(&format!("/tags/{tag}/records/person/{person}")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    }

    async fn person_with_events(&self, events: &[(&str, &str, Option<&str>)]) -> String {
        let person = new_person(&self.app, &self.tree).await;
        for (kind, date, place) in events {
            let mut body = json!({
                "event_type": kind,
                "date_value": date,
                "person_id": person,
            });
            if let Some(place) = place {
                body["place_id"] = json!(place);
            }
            self.post("/events", body).await;
        }
        person
    }

    /// Joins two persons in a new union, returned by its family ID.
    async fn union(&self, husband: &str, wife: &str) -> String {
        let family = ok(&self.app, Method::POST, &self.path("/families"), None).await["id"]
            .as_str()
            .unwrap()
            .to_owned();
        for (person, role, order) in [(husband, "husband", 0), (wife, "wife", 1)] {
            ok(
                &self.app,
                Method::POST,
                &self.path(&format!("/families/{family}/spouses")),
                Some(json!({ "person_id": person, "role": role, "sort_order": order })),
            )
            .await;
        }
        family
    }

    /// Joins two persons in a new union that married in `year`.
    async fn marriage(&self, husband: &str, wife: &str, year: &str) {
        let family = self.union(husband, wife).await;
        self.post(
            "/events",
            json!({ "event_type": "marriage", "date_value": year, "family_id": family }),
        )
        .await;
    }

    async fn place(&self, name: &str) -> String {
        self.post("/places", json!({ "name": name })).await
    }
}

/// The fictitious tree every test reads. Ada carries the tag `Branch A`,
/// Bram is her husband and does not. Cora and Dion are another couple and
/// carry nothing; Ada and Dion both have a death before their birth.
struct Scenario {
    tag: Uuid,
    ada: String,
    dion: String,
}

async fn scenario(f: &Fixture) -> Scenario {
    let hollowmere = f.place("Hollowmere").await;
    let fernlake = f.place("Fernlake").await;
    let tag = f.create_tag("Branch A").await;
    let ada = f
        .person_with_events(&[
            ("birth", "1850", Some(hollowmere.as_str())),
            ("death", "1840", None),
        ])
        .await;
    f.tag_person(tag, &ada).await;
    let bram = f
        .person_with_events(&[
            ("birth", "1852", Some(fernlake.as_str())),
            ("death", "1900", None),
        ])
        .await;
    f.union(&ada, &bram).await;
    let cora = f
        .person_with_events(&[("birth", "1830", None), ("death", "1840", None)])
        .await;
    let dion = f
        .person_with_events(&[("birth", "1850", None), ("death", "1820", None)])
        .await;
    f.union(&cora, &dion).await;
    f.post("/sources", json!({ "title": "Parish register" }))
        .await;
    Scenario { tag, ada, dion }
}

/// Statistics of the tree, over REST, with an optional query string.
async fn rest_statistics(f: &Fixture, query: &str) -> Value {
    ok(
        &f.app,
        Method::GET,
        &f.path(&format!("/statistics{query}")),
        None,
    )
    .await
}

/// The rule `death_before_birth` of the anomalies, over REST, when it is listed.
fn death_before_birth(anomalies: &Value) -> Option<&Value> {
    anomalies["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|rule| rule["rule"] == "death_before_birth")
}

// ── REST ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn statistics_limited_to_a_tag_count_only_its_persons_over_rest() {
    let f = Fixture::new("Tag Scope Statistics REST").await;
    let scenario = scenario(&f).await;
    let tag = scenario.tag;

    let whole = rest_statistics(&f, "").await;
    assert_eq!(whole["persons"], 4);
    assert_eq!(whole["unions"], 2);
    assert_eq!(whole["places"], 2);
    assert_eq!(whole["sources"], 1);

    let limited = rest_statistics(&f, &format!("?tag={tag}")).await;
    assert_eq!(limited["persons"], 1);
    // Ada's union counts through her; Bram's own figures do not.
    assert_eq!(limited["unions"], 1);
    assert_eq!(limited["places"], 1);
    assert_eq!(limited["top_places"][0]["name"], "Hollowmere");
    assert_eq!(limited["top_places"][0]["count"], 1);
    // The sources are not about persons: they stay whole.
    assert_eq!(limited["sources"], 1);

    // Absent or with the tag left out, the statistics are the whole tree.
    let absent = rest_statistics(&f, "?approximate=false").await;
    assert_eq!(absent["persons"], 4);

    // A tag no person carries: the empty figures of persons, sources whole.
    let unused = f.create_tag("Unused").await;
    let empty = rest_statistics(&f, &format!("?tag={unused}")).await;
    assert_eq!(empty["persons"], 0);
    assert_eq!(empty["unions"], 0);
    assert_eq!(empty["places"], 0);
    assert_eq!(empty["sources"], 1);

    let (status, body) = send(
        &f.app,
        Method::GET,
        &f.path(&format!("/statistics?tag={}", Uuid::now_v7())),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

/// A union whose tagged spouse outlives the untagged one: the union ends at
/// the untagged spouse's death, as the whole tree reads it. Returns the tag.
async fn union_ended_by_an_untagged_spouse(f: &Fixture) -> Uuid {
    let tag = f.create_tag("Branch B").await;
    let ada = f
        .person_with_events(&[("birth", "1850", None), ("death", "1920", None)])
        .await;
    f.tag_person(tag, &ada).await;
    let bram = f
        .person_with_events(&[("birth", "1848", None), ("death", "1910", None)])
        .await;
    f.marriage(&ada, &bram, "1870").await;
    tag
}

#[tokio::test]
async fn a_union_ends_by_an_untagged_spouse_as_in_the_whole_tree_over_rest() {
    let f = Fixture::new("Tag Scope Union End REST").await;
    let tag = union_ended_by_an_untagged_spouse(&f).await;

    let whole = rest_statistics(&f, "").await;
    assert_eq!(whole["union_duration"].as_array().unwrap().len(), 1);
    assert_eq!(whole["union_duration"][0]["count"], 1);

    let limited = rest_statistics(&f, &format!("?tag={tag}")).await;
    assert_eq!(limited["persons"], 1);
    assert_eq!(limited["union_duration"], whole["union_duration"]);
}

#[tokio::test]
async fn anomalies_limited_to_a_tag_name_only_its_persons_over_rest() {
    let f = Fixture::new("Tag Scope Anomalies REST").await;
    let scenario = scenario(&f).await;

    let whole = ok(&f.app, Method::GET, &f.path("/anomalies"), None).await;
    assert_eq!(whole["persons"], 4);
    let rule = death_before_birth(&whole).expect("two deaths before births");
    assert_eq!(rule["count"], 2);

    let limited = ok(
        &f.app,
        Method::GET,
        &f.path(&format!("/anomalies?tag={}", scenario.tag)),
        None,
    )
    .await;
    // Persons examined stay the whole tree.
    assert_eq!(limited["persons"], 4);
    let rule = death_before_birth(&limited).expect("Ada's death is still listed");
    assert_eq!(rule["count"], 1);
    assert_eq!(
        rule["items"][0]["persons"][0]["person_id"],
        scenario.ada.as_str()
    );
    assert!(
        rule["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["persons"][0]["person_id"] != scenario.dion.as_str())
    );

    let unused = f.create_tag("Unused").await;
    let empty = ok(
        &f.app,
        Method::GET,
        &f.path(&format!("/anomalies?tag={unused}")),
        None,
    )
    .await;
    assert!(empty["rules"].as_array().unwrap().is_empty());

    let (status, body) = send(
        &f.app,
        Method::GET,
        &f.path(&format!("/anomalies?tag={}", Uuid::now_v7())),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

// ── GraphQL ──────────────────────────────────────────────────────────

const GQL_STATISTICS: &str = r#"query($tree: ID!, $tag: ID) {
    treeStatistics(treeId: $tree, tag: $tag) {
        persons unions places sources topPlaces { name count }
        unionDuration { year sum count }
    }
}"#;

const GQL_ANOMALIES: &str = r#"query($tree: ID!, $tag: ID) {
    treeAnomalies(treeId: $tree, tag: $tag) {
        persons
        rules { rule count items { persons { personId } } }
    }
}"#;

async fn gql_statistics(f: &Fixture, tag: Option<Uuid>) -> Value {
    gql_ok(
        &f.app,
        GQL_STATISTICS,
        json!({ "tree": f.tree, "tag": tag.map(|t| t.to_string()) }),
    )
    .await["treeStatistics"]
        .clone()
}

async fn gql_anomalies(f: &Fixture, tag: Option<Uuid>) -> Value {
    gql_ok(
        &f.app,
        GQL_ANOMALIES,
        json!({ "tree": f.tree, "tag": tag.map(|t| t.to_string()) }),
    )
    .await["treeAnomalies"]
        .clone()
}

#[tokio::test]
async fn statistics_limited_to_a_tag_count_only_its_persons_over_graphql() {
    let f = Fixture::new("Tag Scope Statistics GraphQL").await;
    let scenario = scenario(&f).await;

    let whole = gql_statistics(&f, None).await;
    assert_eq!(whole["persons"], 4);
    assert_eq!(whole["unions"], 2);
    assert_eq!(whole["places"], 2);

    let limited = gql_statistics(&f, Some(scenario.tag)).await;
    assert_eq!(limited["persons"], 1);
    assert_eq!(limited["unions"], 1);
    assert_eq!(limited["places"], 1);
    assert_eq!(limited["topPlaces"][0]["name"], "Hollowmere");
    assert_eq!(limited["topPlaces"][0]["count"], 1);
    assert_eq!(limited["sources"], 1);

    let unused = f.create_tag("Unused").await;
    let empty = gql_statistics(&f, Some(unused)).await;
    assert_eq!(empty["persons"], 0);
    assert_eq!(empty["sources"], 1);

    let resp = gql(&f, GQL_STATISTICS, Some(Uuid::now_v7())).await;
    assert!(
        resp["errors"].as_array().is_some_and(|e| !e.is_empty()),
        "an unknown tag should be rejected, as REST answers 404: {resp}"
    );
}

#[tokio::test]
async fn anomalies_limited_to_a_tag_name_only_its_persons_over_graphql() {
    let f = Fixture::new("Tag Scope Anomalies GraphQL").await;
    let scenario = scenario(&f).await;

    let whole = gql_anomalies(&f, None).await;
    assert_eq!(whole["persons"], 4);
    let rule = whole["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|rule| rule["rule"] == "death_before_birth")
        .expect("two deaths before births");
    assert_eq!(rule["count"], 2);

    let limited = gql_anomalies(&f, Some(scenario.tag)).await;
    assert_eq!(limited["persons"], 4);
    let rule = limited["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|rule| rule["rule"] == "death_before_birth")
        .expect("Ada's death is still listed");
    assert_eq!(rule["count"], 1);
    assert_eq!(
        rule["items"][0]["persons"][0]["personId"],
        scenario.ada.as_str()
    );

    let unused = f.create_tag("Unused").await;
    let empty = gql_anomalies(&f, Some(unused)).await;
    assert!(empty["rules"].as_array().unwrap().is_empty());

    let resp = gql(&f, GQL_ANOMALIES, Some(Uuid::now_v7())).await;
    assert!(
        resp["errors"].as_array().is_some_and(|e| !e.is_empty()),
        "an unknown tag should be rejected, as REST answers 404: {resp}"
    );
}

#[tokio::test]
async fn a_union_ends_by_an_untagged_spouse_as_in_the_whole_tree_over_graphql() {
    let f = Fixture::new("Tag Scope Union End GraphQL").await;
    let tag = union_ended_by_an_untagged_spouse(&f).await;

    let whole = gql_statistics(&f, None).await;
    assert_eq!(whole["unionDuration"].as_array().unwrap().len(), 1);
    assert_eq!(whole["unionDuration"][0]["count"], 1);

    let limited = gql_statistics(&f, Some(tag)).await;
    assert_eq!(limited["persons"], 1);
    assert_eq!(limited["unionDuration"], whole["unionDuration"]);
}

/// Runs `query` with `tag` as its `$tag` variable, returning the whole
/// response, errors included.
async fn gql(f: &Fixture, query: &str, tag: Option<Uuid>) -> Value {
    common::gql(
        &f.app,
        query,
        json!({ "tree": f.tree, "tag": tag.map(|t| t.to_string()) }),
    )
    .await
}
