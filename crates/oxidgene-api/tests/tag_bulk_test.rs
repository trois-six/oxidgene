//! Bulk tagging of persons over REST and GraphQL (docs/api.md, Tags; docs/data-model.md, Audit log): the
//! add and remove of a tag on a target, the target's counts, each kind of
//! target and its depth, the bounds and refusals, and the one audit entry a
//! bulk change writes.
//!
//! Each case runs over both surfaces with the same assertions. All data is
//! fictitious.

use crate::common;

use axum::Router;
use axum::http::{Method, StatusCode};
use common::{
    all_profiles, app_on, gql, gql_error_code, gql_ok, new_person, new_tree, ok, send, setup_db,
};
use serde_json::{Value, json};
use uuid::Uuid;

struct Fixture {
    app: Router,
    tree: String,
}

impl Fixture {
    async fn new(name: &str) -> Self {
        let app = app_on(setup_db().await);
        let tree = new_tree(&app, name).await;
        Self { app, tree }
    }

    fn path(&self, suffix: &str) -> String {
        format!("/api/v1/trees/{}{suffix}", self.tree)
    }

    async fn person(&self, given: &str, surname: &str) -> String {
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

    async fn tag(&self, label: &str) -> String {
        ok(
            &self.app,
            Method::POST,
            &self.path("/tags"),
            Some(json!({ "label": label })),
        )
        .await["id"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    /// A family with the given spouses `(person, role)` and children.
    async fn family(&self, spouses: &[(&str, &str)], children: &[&str]) -> String {
        let family = ok(&self.app, Method::POST, &self.path("/families"), None).await["id"]
            .as_str()
            .unwrap()
            .to_owned();
        for (index, (person, role)) in spouses.iter().enumerate() {
            ok(
                &self.app,
                Method::POST,
                &self.path(&format!("/families/{family}/spouses")),
                Some(json!({ "person_id": person, "role": role, "sort_order": index })),
            )
            .await;
        }
        for person in children {
            ok(
                &self.app,
                Method::POST,
                &self.path(&format!("/families/{family}/children")),
                Some(json!({ "person_id": person, "child_type": "biological", "sort_order": 0 })),
            )
            .await;
        }
        family
    }

    async fn bulk_rest(&self, tag: &str, action: &str, target: Value) -> (StatusCode, Value) {
        send(
            &self.app,
            Method::POST,
            &self.path(&format!("/tags/{tag}/bulk")),
            Some(json!({ "action": action, "target": target })),
        )
        .await
    }

    async fn target_rest(&self, target: Value) -> (StatusCode, Value) {
        send(
            &self.app,
            Method::POST,
            &self.path("/tags/target"),
            Some(json!({ "target": target })),
        )
        .await
    }

    async fn bulk_gql(&self, tag: &str, action: &str, target: Value) -> Value {
        gql(
            &self.app,
            BULK_TAG,
            json!({ "tree": self.tree, "tag": tag, "action": action, "target": target }),
        )
        .await
    }

    async fn target_gql(&self, target: Value) -> Value {
        gql(
            &self.app,
            TAG_TARGET,
            json!({ "tree": self.tree, "target": target }),
        )
        .await
    }

    /// The audit entries about the tag `tag`, as the audit log lists them.
    async fn tag_audit(&self, tag: &str) -> Vec<Value> {
        let page = ok(&self.app, Method::GET, &self.path("/audit?first=100"), None).await;
        page["edges"]
            .as_array()
            .unwrap()
            .iter()
            .map(|edge| edge["node"].clone())
            .filter(|entry| entry["entity"] == "record_tag" && entry["entity_id"] == tag)
            .collect()
    }

    async fn carries(&self, person: &str, tag_label: &str) -> bool {
        all_profiles(&self.app, &self.tree)
            .await
            .into_iter()
            .find(|profile| profile["person_id"] == person)
            .map(|profile| {
                profile["tags"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|tag| tag["label"] == tag_label)
            })
            .expect("the person has a profile")
    }
}

const BULK_TAG: &str = r#"mutation($tree: ID!, $tag: ID!, $action: GqlBulkTagAction!, $target: TagTargetInput!) {
    bulkTag(treeId: $tree, tagId: $tag, action: $action, target: $target) { count }
}"#;

const TAG_TARGET: &str = r#"query($tree: ID!, $target: TagTargetInput!) {
    tagTarget(treeId: $tree, target: $target) { total tags { tagId count } }
}"#;

/// The `count` of each audit entry, sorted.
fn audit_counts(audit: &[Value]) -> Vec<i64> {
    let mut counts: Vec<i64> = audit
        .iter()
        .map(|entry| entry["details"]["count"].as_i64().unwrap())
        .collect();
    counts.sort_unstable();
    counts
}

fn count_of(tags: &Value, tag: &str) -> i64 {
    tags.as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["tag_id"] == tag || entry["tagId"] == tag)
        .map(|entry| entry["count"].as_i64().unwrap())
        .unwrap_or(0)
}

#[tokio::test]
async fn bulk_add_and_remove_over_rest() {
    let f = Fixture::new("Bulk REST").await;
    let ledger = f.tag("Ledger").await;
    let ada = f.person("Ada", "Quillfeather").await;
    let bo = f.person("Bo", "Quillfeather").await;
    let cy = f.person("Cy", "Quillfeather").await;

    let (status, body) = f
        .bulk_rest(&ledger, "add", json!({ "persons": [ada, bo] }))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["count"], 2);
    // Adding again changes nothing and counts nothing.
    let (_, again) = f
        .bulk_rest(&ledger, "add", json!({ "persons": [ada, bo] }))
        .await;
    assert_eq!(again["count"], 0);
    assert!(f.carries(&ada, "Ledger").await);
    assert!(!f.carries(&cy, "Ledger").await);

    let (_, removed) = f
        .bulk_rest(&ledger, "remove", json!({ "persons": [bo, cy] }))
        .await;
    assert_eq!(removed["count"], 1);
    assert!(!f.carries(&bo, "Ledger").await);

    // One audit entry per bulk change that changed something: the add, then the remove.
    let audit = f.tag_audit(&ledger).await;
    assert_eq!(audit_counts(&audit), [1, 2], "{audit:?}");
}

#[tokio::test]
async fn bulk_add_and_remove_over_graphql() {
    let f = Fixture::new("Bulk GraphQL").await;
    let ledger = f.tag("Ledger").await;
    let ada = f.person("Ada", "Quillfeather").await;
    let bo = f.person("Bo", "Quillfeather").await;

    let data = gql_ok(&f.app, BULK_TAG, json!({ "tree": f.tree, "tag": ledger, "action": "ADD", "target": { "persons": [ada, bo] } })).await;
    assert_eq!(data["bulkTag"]["count"], 2);
    let data = gql_ok(&f.app, BULK_TAG, json!({ "tree": f.tree, "tag": ledger, "action": "ADD", "target": { "persons": [ada, bo] } })).await;
    assert_eq!(data["bulkTag"]["count"], 0);
    assert!(f.carries(&ada, "Ledger").await);

    let data = gql_ok(&f.app, BULK_TAG, json!({ "tree": f.tree, "tag": ledger, "action": "REMOVE", "target": { "persons": [ada] } })).await;
    assert_eq!(data["bulkTag"]["count"], 1);
    assert!(!f.carries(&ada, "Ledger").await);

    let audit = f.tag_audit(&ledger).await;
    assert_eq!(audit_counts(&audit), [1, 2], "{audit:?}");
}

#[tokio::test]
async fn target_counts_over_rest_and_graphql() {
    let f = Fixture::new("Target Counts").await;
    let ledger = f.tag("Ledger").await;
    let lantern = f.tag("Lantern").await;
    let ada = f.person("Ada", "Quillfeather").await;
    let bo = f.person("Bo", "Quillfeather").await;
    let cy = f.person("Cy", "Quillfeather").await;
    f.bulk_rest(&ledger, "add", json!({ "persons": [ada, bo] }))
        .await;
    f.bulk_rest(&lantern, "add", json!({ "persons": [bo, cy] }))
        .await;

    let target = json!({ "persons": [ada, bo, cy] });
    let (status, body) = f.target_rest(target.clone()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["total"], 3);
    assert_eq!(count_of(&body["tags"], &ledger), 2);
    assert_eq!(count_of(&body["tags"], &lantern), 2);

    let data = f.target_gql(target).await;
    assert_eq!(data["data"]["tagTarget"]["total"], 3, "{data}");
    assert_eq!(count_of(&data["data"]["tagTarget"]["tags"], &ledger), 2);
}

#[tokio::test]
async fn branch_targets_over_rest_and_graphql() {
    let f = Fixture::new("Branches").await;
    let root = f.person("Alder", "Quillfeather").await;
    let root_spouse = f.person("Briar", "Quillfeather").await;
    let child = f.person("Cass", "Quillfeather").await;
    let child_spouse = f.person("Dane", "Quillfeather").await;
    let grandchild = f.person("Eve", "Quillfeather").await;
    f.family(&[(&root, "husband"), (&root_spouse, "wife")], &[&child])
        .await;
    f.family(
        &[(&child, "husband"), (&child_spouse, "wife")],
        &[&grandchild],
    )
    .await;

    let cases = [
        (json!({ "descendants_of": root }), 2),
        (json!({ "descendants_of": root, "depth": 1 }), 1),
        (
            json!({ "descendants_of": root, "include_spouses": true }),
            3,
        ),
        (
            json!({ "descendants_of": root, "include_self": true, "include_spouses": true }),
            4,
        ),
        (json!({ "ancestors_of": grandchild, "depth": 1 }), 2),
        (json!({ "ancestors_of": grandchild }), 4),
        (
            json!({ "ancestors_of": grandchild, "include_self": true }),
            5,
        ),
    ];
    for (target, expected) in cases {
        let (status, body) = f.target_rest(target.clone()).await;
        assert_eq!(status, StatusCode::OK, "{target}: {body}");
        assert_eq!(body["total"], expected, "REST {target}");
        let data = f.target_gql(camel(&target)).await;
        assert_eq!(
            data["data"]["tagTarget"]["total"], expected,
            "GraphQL {target}: {data}"
        );
    }

    // A depth outside 1 to 30 is refused on both surfaces.
    for depth in [0, 31] {
        let target = json!({ "descendants_of": root, "depth": depth });
        let (status, body) = f.target_rest(target.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["error"], "validation_error");
        let data = f.target_gql(camel(&target)).await;
        assert_eq!(gql_error_code(&data), "VALIDATION_ERROR", "{data}");
    }
}

/// The GraphQL spelling of a REST target: its keys in camelCase.
fn camel(target: &Value) -> Value {
    let mut out = serde_json::Map::new();
    for (key, value) in target.as_object().unwrap() {
        let mut name = String::new();
        let mut upper = false;
        for ch in key.chars() {
            if ch == '_' {
                upper = true;
            } else if upper {
                name.extend(ch.to_uppercase());
                upper = false;
            } else {
                name.push(ch);
            }
        }
        out.insert(name, value.clone());
    }
    Value::Object(out)
}

#[tokio::test]
async fn ancestors_and_descendants_count_a_person_once() {
    let f = Fixture::new("Both Sides").await;
    let ledger = f.tag("Ledger").await;
    let root = f.person("Alder", "Quillfeather").await;
    let father = f.person("Brook", "Quillfeather").await;
    let mother = f.person("Cedar", "Quillfeather").await;
    let grandfather = f.person("Dune", "Quillfeather").await;
    let grandmother = f.person("Elm", "Quillfeather").await;
    let spouse = f.person("Fern", "Quillfeather").await;
    let daughter = f.person("Gale", "Quillfeather").await;
    // The grandfather of the root is also the spouse of the root's daughter:
    // a person on both sides of the branch.
    f.family(&[(&father, "husband"), (&mother, "wife")], &[&root])
        .await;
    f.family(
        &[(&grandfather, "husband"), (&grandmother, "wife")],
        &[&mother],
    )
    .await;
    f.family(&[(&root, "husband"), (&spouse, "wife")], &[&daughter])
        .await;
    f.family(&[(&daughter, "wife"), (&grandfather, "husband")], &[])
        .await;

    // Ancestors (father, mother, grandfather, grandmother), the root and the
    // daughter, the grandfather once although he is her spouse too.
    let cases = [
        (
            json!({ "ancestors_and_descendants_of": root, "include_self": true, "include_spouses": true }),
            6,
        ),
        (
            json!({ "ancestors_and_descendants_of": root, "include_spouses": true }),
            5,
        ),
        (
            json!({ "ancestors_and_descendants_of": root, "depth": 1, "include_self": true, "include_spouses": true }),
            5,
        ),
    ];
    for (target, expected) in cases {
        let (status, body) = f.target_rest(target.clone()).await;
        assert_eq!(status, StatusCode::OK, "{target}: {body}");
        assert_eq!(body["total"], expected, "REST {target}");
        let data = f.target_gql(camel(&target)).await;
        assert_eq!(
            data["data"]["tagTarget"]["total"], expected,
            "GraphQL {target}: {data}"
        );
    }

    // The count of the tag on the whole branch is one per person, and the
    // bulk change writes one audit entry counting the same persons.
    let whole = json!({ "ancestors_and_descendants_of": root, "include_self": true, "include_spouses": true });
    let (status, body) = f.bulk_rest(&ledger, "add", whole.clone()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["count"], 6);
    assert_eq!(audit_counts(&f.tag_audit(&ledger).await), [6]);
    assert!(f.carries(&grandfather, "Ledger").await);

    let (_, counts) = f.target_rest(whole).await;
    assert_eq!(counts["total"], 6);
    assert_eq!(count_of(&counts["tags"], &ledger), 6);

    // A root that is not a person of the tree is not_found.
    let (status, body) = f
        .target_rest(json!({ "ancestors_and_descendants_of": Uuid::now_v7().to_string() }))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

/// A soft-deleted family cuts the walks through it, on both sides: its
/// children are not descendants and the parents it holds are not ancestors,
/// as the live family links say.
#[tokio::test]
async fn branches_do_not_walk_through_a_deleted_family() {
    let f = Fixture::new("Deleted Family").await;
    let gil = f.person("Gil", "Quillfeather").await;
    let hana = f.person("Hana", "Quillfeather").await;
    let pim = f.person("Pim", "Quillfeather").await;
    let quinn = f.person("Quinn", "Quillfeather").await;
    let rue = f.person("Rue", "Quillfeather").await;
    let sol = f.person("Sol", "Quillfeather").await;
    let tove = f.person("Tove", "Quillfeather").await;
    f.family(&[(&gil, "husband"), (&hana, "wife")], &[&pim])
        .await;
    // The middle family: Pim and Quinn, whose child Rue is the link to Tove.
    let middle = f
        .family(&[(&pim, "husband"), (&quinn, "wife")], &[&rue])
        .await;
    f.family(&[(&rue, "husband"), (&sol, "wife")], &[&tove])
        .await;

    let descendants = json!({ "descendants_of": gil });
    let ancestors = json!({ "ancestors_of": tove });
    for (target, expected) in [(&descendants, 3), (&ancestors, 6)] {
        let (status, body) = f.target_rest(target.clone()).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["total"], expected, "REST before the delete {target}");
        let data = f.target_gql(camel(target)).await;
        assert_eq!(
            data["data"]["tagTarget"]["total"], expected,
            "GraphQL before the delete {target}: {data}"
        );
    }

    ok(
        &f.app,
        Method::DELETE,
        &f.path(&format!("/families/{middle}")),
        None,
    )
    .await;

    // Pim is reached from Gil, but not the children of the deleted family;
    // Tove reaches Rue and Sol, not the parents of the deleted family.
    let cases = [(&descendants, 1), (&ancestors, 2)];
    for (target, expected) in cases {
        let (status, body) = f.target_rest(target.clone()).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["total"], expected, "REST after the delete {target}");
        let data = f.target_gql(camel(target)).await;
        assert_eq!(
            data["data"]["tagTarget"]["total"], expected,
            "GraphQL after the delete {target}: {data}"
        );
    }
}

#[tokio::test]
async fn search_target_covers_every_page_over_rest_and_graphql() {
    let f = Fixture::new("Search Target").await;
    let ledger = f.tag("Ledger").await;
    // More matches than one default page of search results holds.
    let mut matches = Vec::new();
    for index in 0..30 {
        matches.push(f.person(&format!("Given {index}"), "Thornwhistle").await);
    }
    f.person("Other", "Elsewhere").await;

    let target = json!({ "search": { "q": "thornwhistle" } });
    let (status, body) = f.target_rest(target.clone()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["total"], 30);

    let (status, body) = f.bulk_rest(&ledger, "add", target.clone()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["count"], 30);
    assert!(f.carries(&matches[29], "Ledger").await);

    let data = gql_ok(&f.app, BULK_TAG, json!({ "tree": f.tree, "tag": ledger, "action": "REMOVE", "target": { "search": { "q": "thornwhistle" } } })).await;
    assert_eq!(data["bulkTag"]["count"], 30);
    assert!(!f.carries(&matches[0], "Ledger").await);
}

#[tokio::test]
async fn malformed_and_oversized_targets_are_refused_over_rest_and_graphql() {
    let f = Fixture::new("Bounds").await;
    let ledger = f.tag("Ledger").await;
    let person = f.person("Ada", "Quillfeather").await;
    let many: Vec<String> = (0..1_001).map(|_| Uuid::now_v7().to_string()).collect();

    let refused = [
        json!({ "persons": many }),
        json!({}),
        json!({ "persons": [person], "descendants_of": person }),
    ];
    for target in refused {
        let (status, body) = f.bulk_rest(&ledger, "add", target.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["error"], "validation_error");
        let data = f.bulk_gql(&ledger, "ADD", camel(&target)).await;
        assert_eq!(gql_error_code(&data), "VALIDATION_ERROR", "{data}");
    }

    let (status, body) = send(
        &f.app,
        Method::POST,
        &f.path(&format!("/tags/{ledger}/bulk")),
        Some(json!({ "action": "toggle", "target": { "persons": [person] } })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "validation_error");
}

#[tokio::test]
async fn persons_of_another_or_deleted_tree_are_not_found() {
    let f = Fixture::new("Refusals").await;
    let ledger = f.tag("Ledger").await;
    let deleted = f.person("Ada", "Quillfeather").await;
    ok(
        &f.app,
        Method::DELETE,
        &f.path(&format!("/persons/{deleted}")),
        None,
    )
    .await;

    let other = new_tree(&f.app, "Other tree").await;
    let foreign = new_person(&f.app, &other).await;

    for person in [deleted, foreign] {
        let (status, body) = f
            .bulk_rest(&ledger, "add", json!({ "persons": [person] }))
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
        assert_eq!(body["error"], "not_found");
        let data = f
            .bulk_gql(&ledger, "ADD", json!({ "persons": [person] }))
            .await;
        assert_eq!(gql_error_code(&data), "NOT_FOUND", "{data}");
    }
}

/// Past the target bound, end to end on both surfaces: 10,001 persons, each
/// created over REST. Kept opt-in because creating them dominates the run
/// time; the cutoff arithmetic itself runs in the default suite as unit tests
/// of `service::tag_bulk` (`search_limit`, `enforce_bound`).
#[tokio::test]
#[ignore = "creates 10,001 persons: run by `just tag-bounds`"]
async fn a_target_past_ten_thousand_persons_is_refused() {
    let f = Fixture::new("Ten Thousand").await;
    let ledger = f.tag("Ledger").await;
    for index in 0..10_001 {
        f.person(&format!("Given {index}"), "Blackmarrow").await;
    }
    let (status, body) = f
        .bulk_rest(&ledger, "add", json!({ "search": { "q": "blackmarrow" } }))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "validation_error");
    let data = f
        .bulk_gql(&ledger, "ADD", json!({ "search": { "q": "blackmarrow" } }))
        .await;
    assert_eq!(gql_error_code(&data), "VALIDATION_ERROR", "{data}");
}
