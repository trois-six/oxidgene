//! A scoped export (docs/tags.md §2.3): a branch at a depth, the
//! persons carrying a tag, no pointer to a record left out, a round trip, the
//! media a GEDZIP packs, the scope in the audit entry, and the whole tree when
//! no scope is given. Each case runs over REST and over GraphQL.
//!
//! All data is fictitious.

mod common;

use std::collections::{HashMap, HashSet};
use std::io::Read;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use common::populated::{png, upload_page};
use common::{
    all_profiles, app_on, gql, gql_error_code, gql_ok, import_gedcom, new_tree, ok, send, setup_db,
    worker_on,
};
use http_body_util::BodyExt;
use oxidgene_api::service::gedcom::{ExportChoices, load_and_export};
use oxidgene_api::service::tag;
use oxidgene_api::service::tag_bulk::TagTargetSpec;
use oxidgene_db::entities::tag as tag_entity;
use oxidgene_db::sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

/// Two families: the first holds the root, his spouse and a child; the child
/// and an outside spouse make the second, and their child carries the tag
/// `Marked`. A stranger cites a source and a note of their own.
const FAMILY: &str = "0 HEAD\n\
1 GEDC\n\
2 VERS 5.5.1\n\
2 FORM LINEAGE-LINKED\n\
1 CHAR UTF-8\n\
0 @I1@ INDI\n\
1 NAME Alpha /Founder/\n\
1 SEX M\n\
1 BIRT\n\
2 DATE 1800\n\
2 PLAC Northfield, Shire\n\
1 SOUR @S1@\n\
2 PAGE p. 12\n\
1 FAMS @F1@\n\
0 @I2@ INDI\n\
1 NAME Beta /Consort/\n\
1 SEX F\n\
1 FAMS @F1@\n\
0 @I3@ INDI\n\
1 NAME Gamma /Founder/\n\
1 SEX M\n\
1 FAMC @F1@\n\
1 FAMS @F2@\n\
0 @I4@ INDI\n\
1 NAME Delta /Outsider/\n\
1 SEX F\n\
1 FAMS @F2@\n\
0 @I5@ INDI\n\
1 NAME Epsilon /Founder/\n\
1 SEX F\n\
1 FAMC @F2@\n\
1 _OXIDGENE_TAG {\"v\":1,\"label\":\"Marked\",\"color\":3}\n\
0 @I6@ INDI\n\
1 NAME Zeta /Stranger/\n\
1 SEX U\n\
1 SOUR @S2@\n\
1 NOTE @N1@\n\
0 @F1@ FAM\n\
1 HUSB @I1@\n\
1 WIFE @I2@\n\
1 CHIL @I3@\n\
1 MARR\n\
2 DATE 1820\n\
2 PLAC Northfield, Shire\n\
0 @F2@ FAM\n\
1 HUSB @I3@\n\
1 WIFE @I4@\n\
1 CHIL @I5@\n\
0 @S1@ SOUR\n\
1 TITL Parish register\n\
1 REPO @R1@\n\
0 @S2@ SOUR\n\
1 TITL Other ledger\n\
0 @R1@ REPO\n\
1 NAME County archive\n\
0 @N1@ NOTE Fictitious note\n\
0 TRLR\n";

/// Percent-encodes `text` for a query string value.
fn encode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

/// The names of the persons a GEDCOM file holds, in file order.
fn names(gedcom: &str) -> Vec<String> {
    gedcom
        .lines()
        .filter_map(|line| line.strip_prefix("1 NAME "))
        .map(str::to_owned)
        .collect()
}

/// The pointers a GEDCOM file writes that name no record it holds.
fn dangling_pointers(gedcom: &str) -> Vec<String> {
    let defined: HashSet<&str> = gedcom
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            (words.next() == Some("0")).then(|| words.next()).flatten()
        })
        .collect();
    gedcom
        .lines()
        .filter(|line| !line.starts_with("0 "))
        .filter_map(|line| line.split_whitespace().nth(2))
        .filter(|word| word.starts_with('@') && word.ends_with('@'))
        .filter(|word| !defined.contains(word))
        .map(str::to_owned)
        .collect()
}

/// The GEDCOM text of a scoped or whole export is checked for its records:
/// the persons and records a branch or a tag keeps, and none it leaves out.
fn assert_kept(gedcom: &str, kept: &[&str], left_out: &[&str]) {
    let names = names(gedcom);
    for name in kept {
        assert!(
            names.iter().any(|kept| kept.starts_with(name)),
            "{name} is kept: {names:?}"
        );
    }
    for name in left_out {
        assert!(
            !names.iter().any(|kept| kept.starts_with(name)),
            "{name} is left out: {names:?}"
        );
    }
    let dangling = dangling_pointers(gedcom);
    assert!(dangling.is_empty(), "dangling pointers: {dangling:?}");
}

/// The branch of the root: its descendants to `depth` generations, the root
/// included, as the REST scope JSON.
fn branch_rest(root: &str, depth: i64) -> Value {
    json!({ "descendants_of": root, "depth": depth, "include_self": true })
}

/// The same branch as GraphQL's scope input.
fn branch_gql(root: &str, depth: i64) -> Value {
    json!({ "descendantsOf": root, "depth": depth, "includeSelf": true })
}

struct Fixture {
    db: DatabaseConnection,
    app: Router,
    tree: String,
    people: HashMap<String, String>,
}

impl Fixture {
    async fn new(name: &str) -> Self {
        let db = setup_db().await;
        let app = app_on(db.clone());
        let tree = new_tree(&app, name).await;
        import_gedcom(&app, &db, &tree, FAMILY).await;
        let people = all_profiles(&app, &tree)
            .await
            .iter()
            .map(|profile| {
                (
                    profile["primary_name"]["given_names"]
                        .as_str()
                        .unwrap()
                        .to_owned(),
                    profile["person_id"].as_str().unwrap().to_owned(),
                )
            })
            .collect();
        Self {
            db,
            app,
            tree,
            people,
        }
    }

    fn tree_id(&self) -> Uuid {
        Uuid::parse_str(&self.tree).unwrap()
    }

    fn person(&self, given_names: &str) -> String {
        self.people[given_names].clone()
    }

    /// The id of the tree's tag labelled `label`.
    async fn tag_id(&self, label: &str) -> String {
        tag_entity::Entity::find()
            .filter(tag_entity::Column::TreeId.eq(self.tree_id()))
            .filter(tag_entity::Column::Label.eq(label))
            .one(&self.db)
            .await
            .expect("lists tags")
            .expect("the tag exists")
            .id
            .to_string()
    }

    /// A REST export of the tree, limited to `scope` when given.
    async fn export_rest(&self, scope: Option<&Value>) -> (StatusCode, Value) {
        let mut uri = format!("/api/v1/trees/{}/gedcom/export", self.tree);
        if let Some(scope) = scope {
            uri += &format!("?scope={}", encode(&scope.to_string()));
        }
        send(&self.app, Method::GET, &uri, None).await
    }

    /// A GraphQL export of the tree, limited to `scope` when given: the
    /// response.
    async fn export_gql(&self, scope: Option<&Value>) -> Value {
        gql(
            &self.app,
            "query($tree: ID!, $scope: TagTargetInput) { exportGedcom(treeId: $tree, scope: $scope) { gedcom } }",
            json!({ "tree": self.tree, "scope": scope }),
        )
        .await
    }

    /// The GEDCOM text of a successful REST export.
    async fn gedcom_rest(&self, scope: Option<&Value>) -> String {
        let (status, body) = self.export_rest(scope).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body["gedcom"].as_str().expect("gedcom text").to_owned()
    }

    /// The GEDCOM text of a successful GraphQL export.
    async fn gedcom_gql(&self, scope: Option<&Value>) -> String {
        let response = self.export_gql(scope).await;
        assert!(response.get("errors").is_none(), "{response}");
        response["data"]["exportGedcom"]["gedcom"]
            .as_str()
            .expect("gedcom text")
            .to_owned()
    }
}

#[tokio::test]
async fn a_branch_at_a_depth_keeps_the_branch_and_nothing_pointing_out_of_it() {
    let f = Fixture::new("Scope Branch").await;
    let root = f.person("Alpha");
    let gedcom = f.gedcom_rest(Some(&branch_rest(&root, 1))).await;
    assert_kept(
        &gedcom,
        &["Alpha", "Gamma"],
        &["Beta", "Delta", "Epsilon", "Zeta"],
    );
    assert!(
        gedcom.contains("Northfield"),
        "the marriage of the kept family"
    );
    assert!(
        gedcom.contains("Parish register"),
        "the source the root cites"
    );
    assert!(
        gedcom.contains("County archive"),
        "the repository of that source"
    );
    assert!(
        !gedcom.contains("Other ledger"),
        "a source only a stranger cites"
    );
    assert!(!gedcom.contains("Fictitious note"), "a note on a stranger");
}

#[tokio::test]
async fn a_branch_at_a_depth_keeps_the_branch_over_graphql_too() {
    let f = Fixture::new("Scope Branch GraphQL").await;
    let root = f.person("Alpha");
    let gedcom = f.gedcom_gql(Some(&branch_gql(&root, 1))).await;
    assert_kept(
        &gedcom,
        &["Alpha", "Gamma"],
        &["Beta", "Delta", "Epsilon", "Zeta"],
    );
}

#[tokio::test]
async fn a_tag_scope_keeps_the_tagged_persons_and_their_families() {
    let f = Fixture::new("Scope Tag").await;
    let tag = f.tag_id("Marked").await;
    let scope = json!({ "tag": tag });
    let gedcom = f.gedcom_rest(Some(&scope)).await;
    assert_kept(
        &gedcom,
        &["Epsilon"],
        &["Alpha", "Beta", "Gamma", "Delta", "Zeta"],
    );
    assert!(
        gedcom.contains("_OXIDGENE_TAG"),
        "the tag travels with its record"
    );
}

#[tokio::test]
async fn a_tag_scope_keeps_the_tagged_persons_over_graphql_too() {
    let f = Fixture::new("Scope Tag GraphQL").await;
    let tag = f.tag_id("Marked").await;
    let gedcom = f.gedcom_gql(Some(&json!({ "tag": tag }))).await;
    assert_kept(
        &gedcom,
        &["Epsilon"],
        &["Alpha", "Beta", "Gamma", "Delta", "Zeta"],
    );
}

#[tokio::test]
async fn an_export_without_a_scope_is_the_whole_tree() {
    let f = Fixture::new("Scope Whole").await;
    let rest = f.gedcom_rest(None).await;
    assert_kept(
        &rest,
        &["Alpha", "Beta", "Gamma", "Delta", "Epsilon", "Zeta"],
        &[],
    );
    assert!(rest.contains("Other ledger"));
    assert!(rest.contains("Fictitious note"));
    let graphql = f.gedcom_gql(None).await;
    assert_kept(
        &graphql,
        &["Alpha", "Beta", "Gamma", "Delta", "Epsilon", "Zeta"],
        &[],
    );
}

#[tokio::test]
async fn a_scope_holding_no_person_is_refused() {
    let f = Fixture::new("Scope Empty").await;
    let (status, body) = f.export_rest(Some(&json!({ "persons": [] }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let unused = tag::create(&f.db, f.tree_id(), "Unused", None)
        .await
        .unwrap()
        .0
        .tag
        .id;
    let (status, body) = f.export_rest(Some(&json!({ "tag": unused }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let response = f.export_gql(Some(&json!({ "tag": unused }))).await;
    assert_eq!(gql_error_code(&response), "VALIDATION_ERROR", "{response}");
}

#[tokio::test]
async fn a_scope_naming_an_unknown_tag_is_not_found() {
    let f = Fixture::new("Scope Unknown").await;
    let unknown = Uuid::nil();
    let (status, body) = f.export_rest(Some(&json!({ "tag": unknown }))).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let response = f.export_gql(Some(&json!({ "tag": unknown }))).await;
    assert_eq!(gql_error_code(&response), "NOT_FOUND", "{response}");
}

#[tokio::test]
async fn a_scoped_export_round_trips() {
    let f = Fixture::new("Scope Round Trip").await;
    let root = f.person("Alpha");
    let gedcom = f.gedcom_rest(Some(&branch_rest(&root, 1))).await;
    let other = Fixture::new("Scope Round Trip Target").await;
    let target = other.tree.clone();
    let summary = import_gedcom(&other.app, &other.db, &target, &gedcom).await;
    assert_eq!(summary["persons_count"], 2, "{summary}");
    assert_eq!(summary["families_count"], 2, "{summary}");
}

#[tokio::test]
async fn a_scoped_export_names_its_scope_in_the_audit_entry() {
    let f = Fixture::new("Scope Audit").await;
    let root = f.person("Alpha");
    f.gedcom_rest(Some(&branch_rest(&root, 1))).await;
    let audit = ok(
        &f.app,
        Method::GET,
        &format!("/api/v1/trees/{}/audit?first=50", f.tree),
        None,
    )
    .await;
    let entries = audit.to_string();
    assert!(entries.contains("descendants_of"), "{entries}");
    assert!(entries.contains(&root), "{entries}");
}

#[tokio::test]
async fn a_scoped_export_names_its_scope_in_the_audit_entry_over_graphql() {
    let f = Fixture::new("Scope Audit GraphQL").await;
    let root = f.person("Alpha");
    f.gedcom_gql(Some(&branch_gql(&root, 1))).await;
    let data = gql_ok(
        &f.app,
        "query($tree: ID!) {
            auditEntries(treeId: $tree, first: 50, category: EXPORT) {
                edges { node { details { format scope } } }
            }
        }",
        json!({ "tree": f.tree }),
    )
    .await;
    let edges = data["auditEntries"]["edges"]
        .as_array()
        .expect("lists the exports");
    assert_eq!(edges.len(), 1, "{data}");
    let details = &edges[0]["node"]["details"];
    assert_eq!(details["format"], "gedcom", "{data}");
    let scope = details["scope"].as_str().expect("names the scope");
    assert!(scope.contains("descendants_of"), "{scope}");
    assert!(scope.contains(&root), "{scope}");
}

#[tokio::test]
async fn a_whole_tree_export_has_no_scope_in_the_audit_entry_over_graphql() {
    let f = Fixture::new("Scope Audit Whole").await;
    f.gedcom_gql(None).await;
    let data = gql_ok(
        &f.app,
        "query($tree: ID!) {
            auditEntries(treeId: $tree, first: 50, category: EXPORT) {
                edges { node { details { scope } } }
            }
        }",
        json!({ "tree": f.tree }),
    )
    .await;
    let edges = data["auditEntries"]["edges"]
        .as_array()
        .expect("lists the exports");
    assert_eq!(edges.len(), 1, "{data}");
    assert!(edges[0]["node"]["details"]["scope"].is_null(), "{data}");
}

/// The GEDCOM text and the names of the media entries of a GEDZIP archive.
fn read_archive(bytes: &[u8]) -> (String, Vec<String>) {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("a GEDZIP archive");
    let (mut gedcom, mut media) = (String::new(), Vec::new());
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("reads an entry");
        let name = entry.name().expect("names the entry").into_owned();
        if name == "gedcom.ged" {
            entry.read_to_string(&mut gedcom).expect("reads the GEDCOM");
        } else {
            media.push(name);
        }
    }
    (gedcom, media)
}

/// The media a GEDZIP packs: each `kept` page and no `left_out` page, and
/// no other entry.
fn assert_packed(media: &[String], kept: &[&str], left_out: &[&str]) {
    for page in kept {
        assert!(
            media.iter().any(|name| name.contains(page)),
            "{page} is packed: {media:?}"
        );
    }
    for page in left_out {
        assert!(
            !media.iter().any(|name| name.contains(page)),
            "{page} is left out: {media:?}"
        );
    }
    assert_eq!(media.len(), kept.len(), "{media:?}");
}

/// The status and the body of `GET uri`.
async fn get_bytes(app: &Router, uri: &str) -> (StatusCode, Vec<u8>) {
    let request = Request::builder()
        .uri(uri)
        .body(Body::empty())
        .expect("a request");
    let response = app.clone().oneshot(request).await.expect("answers");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("reads the body")
        .to_bytes();
    (status, bytes.to_vec())
}

/// The archive at `url`, which must answer successfully.
async fn archive_at(app: &Router, url: &str) -> (String, Vec<String>) {
    let (status, bytes) = get_bytes(app, url).await;
    assert_eq!(status, StatusCode::OK, "{url}");
    read_archive(&bytes)
}

/// The archive of export job `job`, once completed, downloaded from the URL
/// REST's status reports.
async fn rest_archive(f: &Fixture, job: &str) -> (String, Vec<String>) {
    let status = ok(
        &f.app,
        Method::GET,
        &format!("/api/v1/trees/{}/export-jobs/{job}", f.tree),
        None,
    )
    .await;
    assert_eq!(status["phase"], "completed", "{status}");
    let url = status["download_url"].as_str().expect("a download URL");
    archive_at(&f.app, url).await
}

/// The archive of export job `job`, once completed, downloaded from the URL
/// GraphQL's status reports.
async fn gql_archive(f: &Fixture, job: &str) -> (String, Vec<String>) {
    let response = gql(
        &f.app,
        "query($tree: ID!, $job: ID!) { exportJobStatus(treeId: $tree, jobId: $job) { phase downloadUrl } }",
        json!({ "tree": f.tree, "job": job }),
    )
    .await;
    assert!(response.get("errors").is_none(), "{response}");
    let status = &response["data"]["exportJobStatus"];
    assert_eq!(status["phase"], "completed", "{response}");
    let url = status["downloadUrl"].as_str().expect("a download URL");
    archive_at(&f.app, url).await
}

/// Queues a GEDZIP export job over REST with `scope`, and returns its id.
async fn start_rest_job(f: &Fixture, scope: &Value) -> String {
    let scope = encode(&scope.to_string());
    let (status, started) = send(
        &f.app,
        Method::POST,
        &format!("/api/v1/trees/{}/export-jobs?scope={scope}", f.tree),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{started}");
    started["job_id"].as_str().expect("job id").to_owned()
}

/// Queues a GEDZIP export job over GraphQL with `scope`, and returns its id.
async fn start_gql_job(f: &Fixture, scope: &Value) -> String {
    let response = gql(
        &f.app,
        "mutation($tree: ID!, $scope: TagTargetInput) { startExportJob(treeId: $tree, scope: $scope) { jobId } }",
        json!({ "tree": f.tree, "scope": scope }),
    )
    .await;
    assert!(response.get("errors").is_none(), "{response}");
    response["data"]["startExportJob"]["jobId"]
        .as_str()
        .expect("job id")
        .to_owned()
}

#[tokio::test]
async fn an_export_job_with_a_scope_runs_over_rest() {
    let f = Fixture::new("Scope Job").await;
    let root = f.person("Alpha");
    let job = start_rest_job(&f, &branch_rest(&root, 1)).await;
    assert!(worker_on(&f.db).run_once().await.expect("runs the export"));
    let (gedcom, _) = rest_archive(&f, &job).await;
    assert_kept(
        &gedcom,
        &["Alpha", "Gamma"],
        &["Beta", "Delta", "Epsilon", "Zeta"],
    );
}

#[tokio::test]
async fn an_export_job_with_a_scope_runs_over_graphql() {
    let f = Fixture::new("Scope Job GraphQL").await;
    let tag = f.tag_id("Marked").await;
    let job = start_gql_job(&f, &json!({ "tag": tag })).await;
    assert!(worker_on(&f.db).run_once().await.expect("runs the export"));
    let (gedcom, _) = gql_archive(&f, &job).await;
    assert_kept(
        &gedcom,
        &["Epsilon"],
        &["Alpha", "Beta", "Gamma", "Delta", "Zeta"],
    );
}

/// Two documents, each with one page, the first linked to the root of the
/// branch and the second to a stranger outside it. Returns the pages' ids.
async fn media_of_branch(f: &Fixture) -> (String, String) {
    let (kept_document, kept_page) = f.post_document("Kept").await;
    let (left_out_document, left_out_page) = f.post_document("Left out").await;
    f.link(&kept_document, &f.person("Alpha")).await;
    f.link(&left_out_document, &f.person("Zeta")).await;
    (kept_page, left_out_page)
}

#[tokio::test]
async fn a_gedzip_over_rest_packs_only_the_media_of_the_scope() {
    let f = Fixture::new("Scope Media REST").await;
    let (kept_page, left_out_page) = media_of_branch(&f).await;
    let root = f.person("Alpha");

    let scope = encode(&branch_rest(&root, 1).to_string());
    let uri = format!(
        "/api/v1/trees/{}/gedcom/export?format=gedzip&scope={scope}",
        f.tree
    );
    let (status, bytes) = get_bytes(&f.app, &uri).await;
    assert_eq!(status, StatusCode::OK);
    let (gedcom, media) = read_archive(&bytes);
    assert_kept(&gedcom, &["Alpha", "Gamma"], &["Zeta"]);
    assert_packed(&media, &[&kept_page], &[&left_out_page]);

    // Without a scope, both documents' pages are packed.
    let uri = format!("/api/v1/trees/{}/gedcom/export?format=gedzip", f.tree);
    let (_, media) = archive_at(&f.app, &uri).await;
    assert_packed(&media, &[&kept_page, &left_out_page], &[]);
}

#[tokio::test]
async fn an_export_job_with_a_scope_packs_only_its_media_over_rest() {
    let f = Fixture::new("Scope Job Media REST").await;
    let (kept_page, left_out_page) = media_of_branch(&f).await;
    let root = f.person("Alpha");
    let job = start_rest_job(&f, &branch_rest(&root, 1)).await;
    assert!(worker_on(&f.db).run_once().await.expect("runs the export"));
    let (gedcom, media) = rest_archive(&f, &job).await;
    assert_kept(&gedcom, &["Alpha", "Gamma"], &["Zeta"]);
    assert_packed(&media, &[&kept_page], &[&left_out_page]);
}

#[tokio::test]
async fn an_export_job_with_a_scope_packs_only_its_media_over_graphql() {
    let f = Fixture::new("Scope Job Media GraphQL").await;
    let (kept_page, left_out_page) = media_of_branch(&f).await;
    let root = f.person("Alpha");
    let job = start_gql_job(&f, &branch_gql(&root, 1)).await;
    assert!(worker_on(&f.db).run_once().await.expect("runs the export"));
    let (gedcom, media) = gql_archive(&f, &job).await;
    assert_kept(&gedcom, &["Alpha", "Gamma"], &["Zeta"]);
    assert_packed(&media, &[&kept_page], &[&left_out_page]);
}

#[tokio::test]
async fn a_gedzip_packs_only_the_media_of_the_scope() {
    let f = Fixture::new("Scope Media").await;
    let (kept_document, kept_page) = f.post_document("Kept").await;
    let (left_out_document, _) = f.post_document("Left out").await;
    let root = f.person("Alpha");
    let stranger = f.person("Zeta");
    f.link(&kept_document, &root).await;
    f.link(&left_out_document, &stranger).await;

    let spec: TagTargetSpec = serde_json::from_value(branch_rest(&root, 1)).unwrap();
    let data = load_and_export(
        &f.db,
        f.tree_id(),
        ExportChoices::default(),
        Some(&spec),
        true,
    )
    .await
    .expect("exports the archive's records");
    assert_eq!(data.media_files.len(), 1, "{:?}", data.media_files);
    assert!(
        data.media_files[0].1.contains(&kept_page),
        "{:?}",
        data.media_files
    );
    let dangling = dangling_pointers(&data.gedcom);
    assert!(dangling.is_empty(), "dangling pointers: {dangling:?}");
}

/// The name the `SUBM` record of a GEDCOM file gives.
fn submitter_name(gedcom: &str) -> String {
    let mut lines = gedcom.lines();
    lines
        .find(|line| line.starts_with("0 @") && line.ends_with(" SUBM"))
        .expect("a submitter record");
    lines
        .next()
        .and_then(|line| line.strip_prefix("1 NAME "))
        .expect("the submitter's name")
        .to_owned()
}

#[tokio::test]
async fn a_scope_leaving_out_who_am_i_names_the_same_submitter() {
    let f = Fixture::new("Scope Submitter").await;
    // The tree's "Who am I?" is outside the branch of Alpha, and the tree
    // names no submitter, so the name falls back to that person.
    ok(
        &f.app,
        Method::PUT,
        &format!("/api/v1/trees/{}", f.tree),
        Some(json!({ "self_person_id": f.person("Zeta") })),
    )
    .await;
    let whole = f.gedcom_rest(None).await;
    let whole_name = submitter_name(&whole);
    assert_ne!(whole_name, "Not Provided", "{whole}");

    let root = f.person("Alpha");
    let scoped = f.gedcom_rest(Some(&branch_rest(&root, 1))).await;
    assert_kept(&scoped, &["Alpha", "Gamma"], &[]);
    assert!(!scoped.contains("1 NAME Zeta /Stranger/"), "{scoped}");
    assert_eq!(submitter_name(&scoped), whole_name, "{scoped}");

    let scoped = f.gedcom_gql(Some(&branch_gql(&root, 1))).await;
    assert!(!scoped.contains("1 NAME Zeta /Stranger/"), "{scoped}");
    assert_eq!(submitter_name(&scoped), whole_name, "{scoped}");
}

impl Fixture {
    /// A new document of the tree, with one page uploaded; the document's id
    /// and the page's.
    async fn post_document(&self, title: &str) -> (String, String) {
        let document = ok(
            &self.app,
            Method::POST,
            &format!("/api/v1/trees/{}/media/document", self.tree),
            Some(json!({ "title": title })),
        )
        .await;
        let document = document["id"].as_str().unwrap().to_owned();
        let page = upload_page(&self.app, &self.tree, &document, &png(16, 16)).await;
        (document, page["id"].as_str().unwrap().to_owned())
    }

    /// Link a document to a person.
    async fn link(&self, document: &str, person: &str) {
        ok(
            &self.app,
            Method::POST,
            &format!("/api/v1/trees/{}/media-links", self.tree),
            Some(json!({ "media_id": document, "person_id": person })),
        )
        .await;
    }
}
