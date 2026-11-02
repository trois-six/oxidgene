//! The export's *Leave out contemporary and private persons* option
//! (docs/privacy.md §8, docs/ui-settings.md §10), on REST and GraphQL, for a
//! `.ged` and a GEDZIP, in each display of hidden persons, against the
//! canary family: no canary's name or detail in the file, the placeholders
//! marked `RESN`, the rest unchanged, the option named in the audit entry,
//! and the file importable again.

use axum::Router;
use axum::http::{Method, StatusCode};
use oxidgene_db::sea_orm::DatabaseConnection;
use serde_json::{Value, json};

use crate::common::canary::{
    CANARIES, CONTEMPORARY, CONTEMPORARY_NOTE, CONTEMPORARY_PLACE, Canaries, EXCLUDED, OUTSIDE,
    PRIVATE,
};
use crate::common::{app_on, gql, import_gedcom, new_tree, ok, send, setup_db, worker_on};

struct Fixture {
    db: DatabaseConnection,
    app: Router,
    canaries: Canaries,
}

impl Fixture {
    async fn new() -> Self {
        let db = setup_db().await;
        let app = app_on(db.clone());
        let canaries = Canaries::new(&app, &db).await;
        Self { db, app, canaries }
    }

    fn tree(&self) -> &str {
        &self.canaries.tree
    }

    async fn display(&self, mode: &str) {
        ok(
            &self.app,
            Method::PUT,
            &format!("/api/v1/trees/{}", self.tree()),
            Some(json!({"hidden_display": mode})),
        )
        .await;
    }

    async fn rest(&self, query: &str) -> String {
        let (status, body) = send(
            &self.app,
            Method::GET,
            &format!("/api/v1/trees/{}/gedcom/export?{query}", self.tree()),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body["gedcom"].as_str().unwrap().to_owned()
    }

    async fn graphql(&self, leave_out_private: bool) -> String {
        let response = gql(
            &self.app,
            "query($tree: ID!, $private: Boolean) { exportGedcom(treeId: $tree, leaveOutPrivate: $private) { gedcom } }",
            json!({"tree": self.tree(), "private": leave_out_private}),
        )
        .await;
        assert!(response.get("errors").is_none(), "{response}");
        response["data"]["exportGedcom"]["gedcom"]
            .as_str()
            .unwrap()
            .to_owned()
    }
}

fn individuals(gedcom: &str) -> usize {
    gedcom
        .lines()
        .filter(|line| line.starts_with("0 @") && line.ends_with(" INDI"))
        .count()
}

/// What leaving out the contemporary and private persons must give: no
/// trace of those two canaries, the excluded and outside ones kept (only
/// their own options leave them out), and the rest of the family whole.
fn assert_left_out(gedcom: &str) {
    for absent in [
        PRIVATE,
        CONTEMPORARY,
        CONTEMPORARY_PLACE,
        CONTEMPORARY_NOTE,
        "Lamplighter",
    ] {
        assert!(!gedcom.contains(absent), "{absent} leaked:\n{gedcom}");
    }
    for present in [EXCLUDED, OUTSIDE, "Ashby", "Harlan"] {
        assert!(gedcom.contains(present), "{present} is kept:\n{gedcom}");
    }
}

#[tokio::test]
async fn an_export_without_the_option_writes_every_canary() {
    let f = Fixture::new().await;
    let gedcom = f.rest("").await;
    for canary in CANARIES.iter().chain(&[CONTEMPORARY_PLACE]) {
        assert!(gedcom.contains(canary), "{canary}");
    }
    assert!(!gedcom.contains("RESN privacy"), "{gedcom}");
}

#[tokio::test]
async fn placeholders_keep_the_shape_on_both_surfaces() {
    let f = Fixture::new().await;
    for gedcom in [
        f.rest("leave_out_private=true").await,
        f.graphql(true).await,
    ] {
        assert_left_out(&gedcom);
        assert_eq!(individuals(&gedcom), 9, "{gedcom}");
        assert_eq!(gedcom.matches("1 RESN privacy").count(), 2, "{gedcom}");
    }
}

#[tokio::test]
async fn initials_and_hidden_follow_the_trees_display() {
    let f = Fixture::new().await;
    f.display("initials").await;
    let gedcom = f.rest("leave_out_private=true").await;
    assert_left_out(&gedcom);
    assert!(gedcom.contains("1 NAME Q. /C./"), "{gedcom}");
    assert!(gedcom.contains("1 NAME B. /C./"), "{gedcom}");

    f.display("hidden").await;
    let gedcom = f.rest("leave_out_private=true").await;
    assert_left_out(&gedcom);
    assert_eq!(individuals(&gedcom), 7, "{gedcom}");
    assert!(!gedcom.contains("RESN privacy"), "{gedcom}");
}

#[tokio::test]
async fn the_contemporary_rule_and_the_default_privacy_decide_who_is_left_out() {
    let f = Fixture::new().await;
    let settings = |body: Value| {
        let app = f.app.clone();
        let uri = format!("/api/v1/trees/{}", f.tree());
        async move { ok(&app, Method::PUT, &uri, Some(body)).await }
    };
    // The rule off: the contemporary canary is written in full.
    settings(json!({"contemporary_rule": false})).await;
    let gedcom = f.rest("leave_out_private=true").await;
    assert!(gedcom.contains(CONTEMPORARY), "{gedcom}");
    assert!(!gedcom.contains(PRIVATE), "{gedcom}");
    // A private tree: every record nobody classified is left out too.
    settings(json!({"contemporary_rule": true, "default_privacy": "private"})).await;
    let gedcom = f.rest("leave_out_private=true").await;
    for canary in [CONTEMPORARY, PRIVATE, OUTSIDE, "Ashby"] {
        assert!(!gedcom.contains(canary), "{canary}:\n{gedcom}");
    }
    // A threshold out of range changes nothing.
    let (status, _) = send(
        &f.app,
        Method::PUT,
        &format!("/api/v1/trees/{}", f.tree()),
        Some(json!({"contemporary_years": 40})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_gedzip_job_leaves_them_out_and_says_so_in_the_history() {
    let f = Fixture::new().await;
    let (status, started) = send(
        &f.app,
        Method::POST,
        &format!(
            "/api/v1/trees/{}/export-jobs?leave_out_private=true",
            f.tree()
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{started}");
    let job = started["job_id"].as_str().unwrap();
    assert!(worker_on(&f.db).run_once().await.expect("runs the export"));
    let status = ok(
        &f.app,
        Method::GET,
        &format!("/api/v1/trees/{}/export-jobs/{job}", f.tree()),
        None,
    )
    .await;
    assert_eq!(status["phase"], "completed", "{status}");
    let url = status["download_url"].as_str().unwrap();
    let request = axum::http::Request::builder()
        .uri(url)
        .body(axum::body::Body::empty())
        .unwrap();
    let response = tower::ServiceExt::oneshot(f.app.clone(), request)
        .await
        .unwrap();
    let bytes = http_body_util::BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).unwrap();
    let mut gedcom = String::new();
    std::io::Read::read_to_string(&mut archive.by_name("gedcom.ged").unwrap(), &mut gedcom)
        .unwrap();
    assert_left_out(&gedcom);
    // GEDCOM 7 discourages PRIVACY.
    assert!(gedcom.contains("RESN CONFIDENTIAL"), "{gedcom}");
    assert!(!gedcom.contains("RESN PRIVACY"), "{gedcom}");

    let audit = ok(
        &f.app,
        Method::GET,
        &format!("/api/v1/trees/{}/audit?category=export", f.tree()),
        None,
    )
    .await;
    assert_eq!(
        audit["edges"][0]["node"]["details"]["privacy"], "leave_out_private",
        "{audit}"
    );
}

#[tokio::test]
async fn a_redacted_export_imports_again_with_its_placeholders_private() {
    let f = Fixture::new().await;
    let gedcom = f.rest("leave_out_private=true").await;
    let copy = new_tree(&f.app, "Copy").await;
    let summary = import_gedcom(&f.app, &f.db, &copy, &gedcom).await;
    assert_eq!(summary["persons_count"], 9, "{summary}");
    let persons = ok(
        &f.app,
        Method::GET,
        &format!("/api/v1/trees/{copy}/persons?first=50"),
        None,
    )
    .await;
    let private = persons["edges"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|edge| edge["node"]["privacy"] == "private")
        .count();
    assert_eq!(private, 2, "{persons}");
}

/// A scan linked only to a record left out — the contemporary canary's
/// birth, a private family — goes with it (docs/privacy.md §8): neither
/// appears in a file leaving out contemporary and private persons, on REST
/// or GraphQL, though both are written without the option.
#[tokio::test]
async fn the_media_of_a_left_out_record_go_with_it() {
    let f = Fixture::new().await;
    let tree = f.tree();
    let profiles = crate::common::all_profiles(&f.app, tree).await;
    let given = |name: &str| {
        profiles
            .iter()
            .find(|p| p["primary_name"]["given_names"] == name)
            .unwrap_or_else(|| panic!("no {name}"))
    };
    let birth = given("Quillon")["birth"]["event_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let family = given("Ashby")["families_as_spouse"][0]["family_id"]
        .as_str()
        .unwrap()
        .to_owned();
    ok(
        &f.app,
        Method::PUT,
        &format!("/api/v1/trees/{tree}/families/{family}"),
        Some(json!({ "privacy": "private" })),
    )
    .await;
    for (title, link) in [
        ("Canarybirthscan", json!({ "event_id": birth })),
        ("Canaryfamilyscan", json!({ "family_id": family })),
    ] {
        let document = ok(
            &f.app,
            Method::POST,
            &format!("/api/v1/trees/{tree}/media/document"),
            Some(json!({ "title": title })),
        )
        .await;
        let document = document["id"].as_str().unwrap().to_owned();
        crate::common::populated::upload_page(
            &f.app,
            tree,
            &document,
            &crate::common::populated::png(32, 24),
        )
        .await;
        let mut link = link;
        link["media_id"] = json!(document);
        ok(
            &f.app,
            Method::POST,
            &format!("/api/v1/trees/{tree}/media-links"),
            Some(link),
        )
        .await;
    }
    let whole = f.rest("").await;
    for gedcom in [
        f.rest("leave_out_private=true").await,
        f.graphql(true).await,
    ] {
        for title in ["Canarybirthscan", "Canaryfamilyscan"] {
            assert!(whole.contains(title), "{title} is written:\n{whole}");
            assert!(!gedcom.contains(title), "{title} leaked:\n{gedcom}");
        }
    }
}
