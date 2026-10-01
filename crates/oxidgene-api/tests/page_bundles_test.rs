//! What the pages read in one answer, on REST and GraphQL alike: portraits
//! and SOSA marks riding with pedigrees and search rows, the default
//! pedigree, the couple bundle, the home page's batched recent persons, the
//! dictionary's inline source list and the base map's cache validator.
//!
//! The tree is the fictitious family-blocks fixture: block 0's root,
//! "Anchor", is the SOSA root, with parents, grandparents, two siblings and
//! a wife.

mod common;

use axum::http::{Method, StatusCode, header};
use common::{all_profiles, family_blocks_tree, gql_ok, ok, send, setup_db};
use serde_json::{Value, json};

/// A person of the fixture by given name; their id.
fn person(profiles: &[Value], given: &str) -> String {
    profiles
        .iter()
        .find(|p| p["primary_name"]["given_names"] == given)
        .unwrap_or_else(|| panic!("no {given}"))["person_id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// A remote picture as `person_id`'s portrait; its page id.
async fn remote_portrait(app: &axum::Router, tree: &str, person_id: &str) -> String {
    let document = ok(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/media/document"),
        Some(json!({ "title": "Portrait" })),
    )
    .await;
    let page = ok(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/media"),
        Some(json!({
            "document_id": document["id"],
            "file_name": "portrait.jpg",
            "mime_type": "image/jpeg",
            "file_path": "https://archives.example.invalid/portrait.jpg",
            "file_size": 0
        })),
    )
    .await;
    let page_id = page["id"].as_str().unwrap().to_string();
    ok(
        app,
        Method::PUT,
        &format!("/api/v1/trees/{tree}/persons/{person_id}/portrait"),
        Some(json!({ "media_id": page_id })),
    )
    .await;
    page_id
}

struct Fixture {
    app: axum::Router,
    tree: String,
    anchor: String,
    profiles: Vec<Value>,
}

async fn fixture() -> Fixture {
    let db = setup_db().await;
    let app = common::app_on(db.clone());
    let (tree, anchor) = family_blocks_tree(&app, &db, 2).await;
    let profiles = all_profiles(&app, &tree).await;
    Fixture {
        app,
        tree,
        anchor,
        profiles,
    }
}

const PORTRAIT_URL: &str = "https://archives.example.invalid/portrait.jpg";

#[tokio::test]
async fn pedigree_nodes_carry_their_portrait_and_sosa_mark() {
    let f = fixture().await;
    assert_eq!(person(&f.profiles, "Anchor"), f.anchor);
    remote_portrait(&f.app, &f.tree, &f.anchor).await;

    let pedigree = ok(
        &f.app,
        Method::GET,
        &format!(
            "/api/v1/trees/{}/pedigree/{}?ancestor_depth=2&descendant_depth=1",
            f.tree, f.anchor
        ),
        None,
    )
    .await;
    let nodes = pedigree["persons"].as_object().unwrap();
    let anchor = &nodes[&f.anchor];
    assert_eq!(anchor["sosa_ancestor"], true);
    assert_eq!(anchor["portrait"]["source"]["kind"], "remote");
    assert_eq!(anchor["portrait"]["source"]["url"], PORTRAIT_URL);
    let ancestors = nodes
        .values()
        .filter(|node| node["sosa_ancestor"] == true)
        .count();
    // The anchor, two parents, four grandparents.
    assert_eq!(ancestors, 7, "{pedigree}");
    assert!(
        nodes
            .values()
            .filter(|node| node["person_id"] != f.anchor.as_str())
            .all(|node| node.get("portrait").is_none()),
        "{pedigree}"
    );

    let data = gql_ok(
        &f.app,
        r#"query($t: ID!, $r: ID!) { pedigree(treeId: $t, rootPersonId: $r, ancestorDepth: 2, descendantDepth: 1) {
            nodes { personId sosaAncestor portrait { source { kind url } } } } }"#,
        json!({ "t": f.tree, "r": f.anchor }),
    )
    .await;
    let nodes = data["pedigree"]["nodes"].as_array().unwrap();
    let anchor = nodes
        .iter()
        .find(|n| n["personId"] == f.anchor.as_str())
        .unwrap();
    assert_eq!(anchor["sosaAncestor"], true);
    assert_eq!(anchor["portrait"]["source"]["url"], PORTRAIT_URL);
    assert_eq!(
        nodes.iter().filter(|n| n["sosaAncestor"] == true).count(),
        7
    );
}

#[tokio::test]
async fn the_default_pedigree_is_drawn_around_the_sosa_root() {
    let f = fixture().await;
    let pedigree = ok(
        &f.app,
        Method::GET,
        &format!(
            "/api/v1/trees/{}/pedigree?ancestor_depth=1&descendant_depth=0",
            f.tree
        ),
        None,
    )
    .await;
    assert_eq!(pedigree["root_person_id"], f.anchor);
    let data = gql_ok(
        &f.app,
        r#"query($t: ID!) { pedigree(treeId: $t, ancestorDepth: 1, descendantDepth: 0) { rootPersonId } }"#,
        json!({ "t": f.tree }),
    )
    .await;
    assert_eq!(data["pedigree"]["rootPersonId"], f.anchor);

    // A tree without anyone has no pedigree.
    let empty = common::new_tree(&f.app, "Empty").await;
    let (status, body) = send(
        &f.app,
        Method::GET,
        &format!("/api/v1/trees/{empty}/pedigree?ancestor_depth=1&descendant_depth=0"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let response = common::gql(
        &f.app,
        r#"query($t: ID!) { pedigree(treeId: $t, ancestorDepth: 1, descendantDepth: 0) { rootPersonId } }"#,
        json!({ "t": empty }),
    )
    .await;
    assert_eq!(common::gql_error_code(&response), "NOT_FOUND");
}

#[tokio::test]
async fn search_rows_carry_their_portrait() {
    let f = fixture().await;
    remote_portrait(&f.app, &f.tree, &f.anchor).await;
    let found = ok(
        &f.app,
        Method::GET,
        &format!("/api/v1/trees/{}/persons/search?q=Anchor", f.tree),
        None,
    )
    .await;
    let row = &found["entries"][0];
    assert_eq!(row["person_id"], f.anchor);
    assert_eq!(row["portrait"]["source"]["url"], PORTRAIT_URL);
    let data = gql_ok(
        &f.app,
        r#"query($t: ID!) { searchPersons(treeId: $t, query: "Anchor") { entries { personId portrait { source { url } } } } }"#,
        json!({ "t": f.tree }),
    )
    .await;
    assert_eq!(
        data["searchPersons"]["entries"][0]["portrait"]["source"]["url"],
        PORTRAIT_URL
    );
}

#[tokio::test]
async fn a_person_bundle_carries_the_portrait_and_the_sosa_marks() {
    let f = fixture().await;
    remote_portrait(&f.app, &f.tree, &f.anchor).await;
    let bundle = ok(
        &f.app,
        Method::GET,
        &format!(
            "/api/v1/trees/{}/persons/{}/detail-bundle",
            f.tree, f.anchor
        ),
        None,
    )
    .await;
    assert_eq!(bundle["portrait"]["source"]["url"], PORTRAIT_URL);
    // The anchor and both parents are in the bundle and marked; the
    // siblings and the wife are in it and are not.
    let marked = bundle["sosa_ancestor_ids"].as_array().unwrap();
    assert_eq!(marked.len(), 3, "{marked:?}");
    assert!(marked.contains(&json!(f.anchor)));
    assert!(bundle["persons"].as_array().unwrap().len() > 3);

    let data = gql_ok(
        &f.app,
        r#"query($t: ID!, $p: ID!) { personDetailBundle(treeId: $t, personId: $p) { sosaAncestorIds portrait { source { url } } } }"#,
        json!({ "t": f.tree, "p": f.anchor }),
    )
    .await;
    assert_eq!(
        data["personDetailBundle"]["sosaAncestorIds"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        data["personDetailBundle"]["portrait"]["source"]["url"],
        PORTRAIT_URL
    );
}

#[tokio::test]
async fn a_couple_bundle_holds_the_family_its_spouses_notes_and_media() {
    let f = fixture().await;
    let anchor_profile = f
        .profiles
        .iter()
        .find(|p| p["person_id"] == f.anchor.as_str())
        .unwrap();
    let family = anchor_profile["families_as_spouse"][0]["family_id"]
        .as_str()
        .unwrap()
        .to_string();
    for (owner, text) in [
        ("family_id", "About the couple"),
        ("person_id", "About him"),
    ] {
        let id = if owner == "family_id" {
            &family
        } else {
            &f.anchor
        };
        ok(
            &f.app,
            Method::POST,
            &format!("/api/v1/trees/{}/notes", f.tree),
            Some(json!({ owner: id, "text": text })),
        )
        .await;
    }
    let document = ok(
        &f.app,
        Method::POST,
        &format!("/api/v1/trees/{}/media/document", f.tree),
        Some(json!({ "title": "Wedding" })),
    )
    .await;
    ok(
        &f.app,
        Method::POST,
        &format!("/api/v1/trees/{}/media-links", f.tree),
        Some(json!({ "media_id": document["id"], "family_id": family })),
    )
    .await;

    let bundle = ok(
        &f.app,
        Method::GET,
        &format!("/api/v1/trees/{}/families/{family}/detail-bundle", f.tree),
        None,
    )
    .await;
    assert_eq!(bundle["family"]["id"], family);
    let spouses = bundle["spouses"].as_array().unwrap();
    assert_eq!(spouses.len(), 2);
    let persons = bundle["persons"].as_array().unwrap();
    assert_eq!(persons.len(), 2);
    for (spouse, person) in spouses.iter().zip(persons) {
        assert!(
            person["persons"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["id"] == spouse["person_id"])
        );
    }
    let notes: Vec<&str> = bundle["notes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|note| note["text"].as_str().unwrap())
        .collect();
    assert_eq!(notes, ["About the couple", "About him"]);
    assert_eq!(bundle["media"][0]["id"], document["id"]);
    assert_eq!(bundle["gallery"]["media"][0]["media_id"], document["id"]);

    let data = gql_ok(
        &f.app,
        r#"query($t: ID!, $f: ID!) { coupleDetailBundle(treeId: $t, familyId: $f) {
            family { id } spouses { personId } persons { sosaNumber } notes { text } media { media { id } } } }"#,
        json!({ "t": f.tree, "f": family }),
    )
    .await;
    let couple = &data["coupleDetailBundle"];
    assert_eq!(couple["family"]["id"], family);
    assert_eq!(couple["persons"].as_array().unwrap().len(), 2);
    assert_eq!(couple["notes"].as_array().unwrap().len(), 2);
    assert_eq!(couple["media"][0]["media"]["id"], document["id"]);

    // A family of another tree, or none, is not found on either surface.
    let missing = uuid::Uuid::now_v7();
    let (status, _) = send(
        &f.app,
        Method::GET,
        &format!("/api/v1/trees/{}/families/{missing}/detail-bundle", f.tree),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let response = common::gql(
        &f.app,
        r#"query($t: ID!, $f: ID!) { coupleDetailBundle(treeId: $t, familyId: $f) { family { id } } }"#,
        json!({ "t": f.tree, "f": missing }),
    )
    .await;
    assert_eq!(common::gql_error_code(&response), "NOT_FOUND");
}

#[tokio::test]
async fn the_recent_persons_of_several_trees_come_in_one_answer() {
    let f = fixture().await;
    let other = common::new_tree(&f.app, "Other").await;
    let someone = common::new_person(&f.app, &other).await;
    let gone = common::new_tree(&f.app, "Gone").await;
    ok(
        &f.app,
        Method::DELETE,
        &format!("/api/v1/trees/{gone}"),
        None,
    )
    .await;
    ok(
        &f.app,
        Method::PUT,
        &format!("/api/v1/trees/{}/persons/{}", f.tree, f.anchor),
        Some(json!({ "sex": "male" })),
    )
    .await;

    let batch = ok(
        &f.app,
        Method::GET,
        &format!(
            "/api/v1/trees/recent-persons?tree_ids={},{gone},{other}&limit=3",
            f.tree
        ),
        None,
    )
    .await;
    let trees = batch.as_array().unwrap();
    assert_eq!(trees.len(), 2, "the deleted tree is left out: {batch}");
    assert_eq!(trees[0]["tree_id"], f.tree);
    assert_eq!(trees[0]["persons"][0]["person_id"], f.anchor);
    assert_eq!(trees[1]["tree_id"], other);
    assert_eq!(trees[1]["persons"][0]["person_id"], someone);

    let (status, _) = send(
        &f.app,
        Method::GET,
        "/api/v1/trees/recent-persons?tree_ids=not-an-id",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let data = gql_ok(
        &f.app,
        r#"query($ids: [ID!]!) { recentPersonsOfTrees(treeIds: $ids, limit: 3) { treeId persons { personId } } }"#,
        json!({ "ids": [f.tree, gone, other] }),
    )
    .await;
    let trees = data["recentPersonsOfTrees"].as_array().unwrap();
    assert_eq!(trees.len(), 2);
    assert_eq!(trees[0]["persons"][0]["personId"], f.anchor);
}

#[tokio::test]
async fn the_last_source_level_comes_with_its_sources() {
    let f = fixture().await;
    let level = ok(
        &f.app,
        Method::GET,
        &format!("/api/v1/trees/{}/dictionary/sources/groups", f.tree),
        None,
    )
    .await;
    assert!(level["groups"].as_array().unwrap().is_empty(), "{level}");
    let titles: Vec<&str> = level["sources"]
        .as_array()
        .expect("the sources come with the last level")
        .iter()
        .map(|entry| entry["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Parish register 0", "Parish register 1"]);
    // The same entries as the plain list, their repositories included.
    assert!(level["sources"][0]["repositories"].is_array(), "{level}");

    let data = gql_ok(
        &f.app,
        r#"query($t: ID!) { dictionarySourceDrill(treeId: $t) { groups { label } sources { count repositories source { title } } } }"#,
        json!({ "t": f.tree }),
    )
    .await;
    let sources = data["dictionarySourceDrill"]["sources"].as_array().unwrap();
    assert_eq!(sources.len(), 2);
    assert!(sources[0]["repositories"].is_array(), "{data}");
}

#[tokio::test]
async fn the_base_map_is_cacheable_and_revalidated_without_a_body() {
    let db = setup_db().await;
    let app = common::app_on(db);
    let request = |etag: Option<&str>| {
        let mut builder = axum::http::Request::builder().uri("/api/v1/reference/basemap");
        if let Some(etag) = etag {
            builder = builder.header(header::IF_NONE_MATCH, etag);
        }
        builder.body(axum::body::Body::empty()).unwrap()
    };
    use tower::ServiceExt as _;
    let response = app.clone().oneshot(request(None)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "public, max-age=604800, immutable"
    );
    let etag = response.headers()[header::ETAG]
        .to_str()
        .unwrap()
        .to_string();
    let response = app.oneshot(request(Some(&etag))).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
}
