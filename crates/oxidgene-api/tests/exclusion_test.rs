//! The exclusions from sharing (docs/privacy.md §6, §11 "Exclusions") on
//! both surfaces: each kind with its options, the union kept current as
//! the family links move, a merge keeping a person excluded, the history,
//! and the owner's export leaving them out.

use axum::Router;
use axum::http::{Method, StatusCode};
use oxidgene_db::sea_orm::DatabaseConnection;
use serde_json::{Value, json};

use crate::common::canary::{Canaries, EXCLUDED};
use crate::common::{app_on, gql, new_person, new_tree, ok, send, setup_db};

struct Fixture {
    app: Router,
    canaries: Canaries,
    _db: DatabaseConnection,
}

impl Fixture {
    async fn new() -> Self {
        let db = setup_db().await;
        let app = app_on(db.clone());
        let canaries = Canaries::new(&app, &db).await;
        Self {
            app,
            canaries,
            _db: db,
        }
    }

    fn uri(&self, path: &str) -> String {
        format!("/api/v1/trees/{}{path}", self.canaries.tree)
    }

    async fn exclude(&self, body: Value) -> (StatusCode, Value) {
        send(
            &self.app,
            Method::POST,
            &self.uri("/exclusions"),
            Some(body),
        )
        .await
    }

    async fn list(&self) -> Vec<Value> {
        ok(&self.app, Method::GET, &self.uri("/exclusions"), None)
            .await
            .as_array()
            .unwrap()
            .clone()
    }
}

#[tokio::test]
async fn each_kind_covers_its_branch_and_follows_new_links() {
    let f = Fixture::new().await;
    let corwin = f.canaries.person("Corwin").to_owned();
    let (status, person) = f
        .exclude(json!({"kind": "person", "person_id": f.canaries.person("Thistle")}))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{person}");
    assert_eq!(person["person_count"], 1);
    assert_eq!(person["include_root"], true);

    // Corwin and his descendants with their spouses: Harlan, Thistle,
    // Quillon; Edda, Corwin's own wife, is no descendant's spouse.
    let (_, descendants) = f
        .exclude(json!({"kind": "descendants", "person_id": corwin}))
        .await;
    assert_eq!(descendants["person_count"], 4, "{descendants}");
    let (_, ancestors) = f
        .exclude(json!({"kind": "ancestors", "person_id": corwin, "depth": 1}))
        .await;
    assert_eq!(ancestors["person_count"], 2, "{ancestors}");
    assert_eq!(ancestors["include_root"], false);

    // A child added under the excluded branch is covered at the next read.
    let child = new_person(&f.app, &f.canaries.tree).await;
    let family = ok(&f.app, Method::POST, &f.uri("/families"), Some(json!({}))).await;
    let family = family["id"].as_str().unwrap();
    for (path, body) in [
        (
            format!("/families/{family}/spouses"),
            json!({"person_id": f.canaries.person("Quillon"), "role": "husband", "sort_order": 0}),
        ),
        (
            format!("/families/{family}/children"),
            json!({"person_id": child, "child_type": "biological", "sort_order": 0}),
        ),
    ] {
        ok(&f.app, Method::POST, &f.uri(&path), Some(body)).await;
    }
    let list = f.list().await;
    let counts: Vec<&Value> = list.iter().map(|row| &row["person_count"]).collect();
    assert_eq!(counts, [1, 5, 2], "{list:?}");

    // Removed, and recorded in the history under `access`.
    let id = list[0]["id"].as_str().unwrap();
    let (status, _) = send(
        &f.app,
        Method::DELETE,
        &f.uri(&format!("/exclusions/{id}")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(f.list().await.len(), 2);
    let audit = ok(&f.app, Method::GET, &f.uri("/audit?category=access"), None).await;
    let entries = audit["edges"].as_array().unwrap();
    assert_eq!(entries.len(), 4, "{audit}");
    assert_eq!(entries[0]["node"]["action"], "delete");
    assert_eq!(entries[0]["node"]["entity"], "privacy_exclusion");
}

#[tokio::test]
async fn a_bad_depth_or_another_trees_person_is_refused() {
    let f = Fixture::new().await;
    let (status, _) = f
        .exclude(json!({"kind": "ancestors", "person_id": f.canaries.person("Corwin"), "depth": 0}))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let other = new_tree(&f.app, "Elsewhere").await;
    let stranger = new_person(&f.app, &other).await;
    let (status, _) = f
        .exclude(json!({"kind": "person", "person_id": stranger}))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send(
        &f.app,
        Method::DELETE,
        &f.uri(&format!("/exclusions/{stranger}")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_merge_keeps_the_person_excluded() {
    let f = Fixture::new().await;
    let duplicate = new_person(&f.app, &f.canaries.tree).await;
    f.exclude(json!({"kind": "person", "person_id": duplicate}))
        .await;
    let kept = f.canaries.person("Ashby").to_owned();
    ok(
        &f.app,
        Method::POST,
        &f.uri(&format!("/persons/{kept}/merge")),
        Some(json!({"duplicate_id": duplicate})),
    )
    .await;
    let list = f.list().await;
    assert_eq!(list[0]["person_id"], kept.as_str(), "{list:?}");
    assert_eq!(list[0]["person_count"], 1);
}

#[tokio::test]
async fn graphql_creates_lists_and_removes_them_as_rest_does() {
    let f = Fixture::new().await;
    let created = gql(
        &f.app,
        "mutation($tree: ID!, $person: ID!) { createExclusion(treeId: $tree, input: { kind: DESCENDANTS, personId: $person, includeSpouses: false }) { id personCount includeRoot } }",
        json!({"tree": f.canaries.tree, "person": f.canaries.person("Harlan")}),
    )
    .await;
    assert!(created.get("errors").is_none(), "{created}");
    let exclusion = &created["data"]["createExclusion"];
    assert_eq!(exclusion["personCount"], 2, "{created}");
    let listed = gql(
        &f.app,
        "query($tree: ID!) { exclusions(treeId: $tree) { id kind personCount } }",
        json!({"tree": f.canaries.tree}),
    )
    .await;
    assert_eq!(listed["data"]["exclusions"][0]["kind"], "DESCENDANTS");
    assert_eq!(f.list().await[0]["person_count"], 2);
    let removed = gql(
        &f.app,
        "mutation($tree: ID!, $id: ID!) { deleteExclusion(treeId: $tree, exclusionId: $id) }",
        json!({"tree": f.canaries.tree, "id": exclusion["id"]}),
    )
    .await;
    assert_eq!(removed["data"]["deleteExclusion"], true, "{removed}");
    assert!(f.list().await.is_empty());
}

#[tokio::test]
async fn the_owners_export_leaves_out_the_excluded_persons_without_initials() {
    let f = Fixture::new().await;
    f.exclude(json!({"kind": "person", "person_id": f.canaries.person("Thistle")}))
        .await;
    ok(
        &f.app,
        Method::PUT,
        &format!("/api/v1/trees/{}", f.canaries.tree),
        Some(json!({"hidden_display": "initials"})),
    )
    .await;
    let export = |query: &'static str| {
        let app = f.app.clone();
        let uri = f.uri(&format!("/gedcom/export?{query}"));
        async move {
            ok(&app, Method::GET, &uri, None).await["gedcom"]
                .as_str()
                .unwrap()
                .to_owned()
        }
    };
    let whole = export("").await;
    assert!(whole.contains(EXCLUDED), "the owner's export includes them");
    let gedcom = export("leave_out_excluded=true").await;
    assert!(!gedcom.contains(EXCLUDED), "{gedcom}");
    assert!(!gedcom.contains("T. /C./"), "{gedcom}");
    assert_eq!(gedcom.matches("1 RESN privacy").count(), 1, "{gedcom}");
    let response = gql(
        &f.app,
        "query($tree: ID!) { exportGedcom(treeId: $tree, leaveOutExcluded: true) { gedcom } }",
        json!({"tree": f.canaries.tree}),
    )
    .await;
    let gedcom = response["data"]["exportGedcom"]["gedcom"].as_str().unwrap();
    assert!(!gedcom.contains(EXCLUDED), "{gedcom}");
}

/// Two rebuilds of one tree's exclusions, or of one access scope, never
/// interleave: the first to read the state holds it until its transaction
/// ends, and the second reads it only then. On PostgreSQL, under READ
/// COMMITTED, the second would otherwise delete what the first committed,
/// and mark a union lacking a new exclusion current.
#[tokio::test]
async fn a_rebuild_holds_the_state_it_read_until_it_commits() {
    use std::time::Duration;

    use oxidgene_core::sharing::{ScopeDefinition, ScopeKind};
    use oxidgene_db::repo::{AccessScopeRepo, ExclusionRepo};
    use oxidgene_db::sea_orm::TransactionTrait;

    let f = Fixture::new().await;
    let tree: uuid::Uuid = f.canaries.tree.parse().unwrap();
    let scope = AccessScopeRepo::create(
        &f._db,
        tree,
        &ScopeDefinition {
            kind: ScopeKind::DescendantsOf,
            person_id: f.canaries.person("Corwin").parse().unwrap(),
            depth: None,
            include_self: true,
            include_spouses: false,
            collateral: false,
        },
    )
    .await
    .expect("creates a scope")
    .id;
    let first = f._db.begin().await.expect("begins");
    ExclusionRepo::lock_generations(&first, tree)
        .await
        .expect("reads the state");
    AccessScopeRepo::lock_generations(&first, scope)
        .await
        .expect("reads the scope");
    let db = f._db.clone();
    let second = tokio::spawn(async move {
        let txn = db.begin().await.expect("begins");
        ExclusionRepo::lock_generations(&txn, tree)
            .await
            .expect("reads the state");
        AccessScopeRepo::lock_generations(&txn, scope)
            .await
            .expect("reads the scope");
        txn.commit().await.expect("commits");
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !second.is_finished(),
        "the second rebuild read a held state"
    );
    first.commit().await.expect("commits");
    tokio::time::timeout(Duration::from_secs(10), second)
        .await
        .expect("the second rebuild goes on once the first commits")
        .expect("runs");
}
