//! The leak suite (docs/privacy.md §11, docs/authorization.md §12): canary
//! records a viewer may not see must never reach them — not in a body, a
//! list, a count, a search match, a suggestion, a statistic, an export or a
//! chart — through any REST route or GraphQL query that reads the tree.
//!
//! The fixture is the populated tree of the guards (one record of every
//! kind, all hanging from the "Anchor" person) with its anchor excluded
//! from sharing, and a confidential note on a visible person. Every read
//! operation of both surfaces is called by each restricted viewer, first
//! with the identifiers of the anchor's own records, then with those of a
//! visible person, and its answer must hold none of the viewer's canaries.
//!
//! The viewers are an Editor, a Guest, a Guest limited to an access scope
//! and a Visitor of the tree's `site` visibility; a share link's holder
//! reads through the same Visitor access. Adding a kind of viewer is a line
//! of [`VIEWERS`] and its canaries in [`canaries`].

use std::collections::BTreeMap;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt as _;
use oxidgene_core::access::{InstanceRole, TreeRole};
use oxidgene_core::sharing::{ScopeDefinition, TreeVisibility};
use oxidgene_db::repo::{
    AccessScopeRepo, AppUserRepo, MembershipRepo, NewAppUser, SharedMembership, TreeChanges,
    TreeRepo,
};
use oxidgene_db::sea_orm::DatabaseConnection;
use serde_json::{Value, json};
use tower::ServiceExt as _;
use uuid::Uuid;

use crate::common::oidc::{MockProvider, TRUSTED_CLIENT};
use crate::common::populated::{NOTE_MARKER, PLACE_MARKER, Populated, populated_tree};
use crate::common::{app_on, ok, setup_db};
use crate::guards::surface::Spec;

/// A restricted viewer of the suite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Who {
    Guest,
    Editor,
    /// A Guest whose membership is limited to an access scope.
    ScopedGuest,
    /// A signed-in user without a membership, a Visitor of a `site` tree.
    Visitor,
}

/// Every restricted viewer the suite runs as.
const VIEWERS: [Who; 4] = [Who::Guest, Who::Editor, Who::ScopedGuest, Who::Visitor];

/// The membership role of `who`, none for a Visitor.
fn role(who: Who) -> Option<TreeRole> {
    match who {
        Who::Guest | Who::ScopedGuest => Some(TreeRole::Guest),
        Who::Editor => Some(TreeRole::Editor),
        Who::Visitor => None,
    }
}

/// A confidential note on a visible person: withheld from Guests.
const CONFIDENTIAL_MARKER: &str = "Wickerbrand sealed remark";

/// What `who` may not meet anywhere: the excluded anchor's name and
/// identifier, every record only they reach, for a viewer without
/// `read_confidential` the confidential note, and for a scoped viewer the
/// identifier of the anchor's spouse, outside the scope.
fn canaries(who: Who, fixture: &Fixture) -> Vec<String> {
    let mut found = vec![
        "Anchor".to_string(),
        fixture.anchor.clone(),
        NOTE_MARKER.to_string(),
        PLACE_MARKER.to_string(),
        "Fictitious register".to_string(),
        "Fictitious archive".to_string(),
        "Fictitious scan".to_string(),
    ];
    if who != Who::Editor {
        found.push(CONFIDENTIAL_MARKER.to_string());
    }
    if who == Who::ScopedGuest {
        found.push(fixture.tree.id("spouse_id").to_string());
    }
    found
}

struct Fixture {
    app: axum::Router,
    db: DatabaseConnection,
    tokens: BTreeMap<Who, String>,
    tree: Populated,
    /// The excluded anchor's identifier.
    anchor: String,
    /// The fixture's records with a visible person in the anchor's place.
    visible: Populated,
    /// The scoped Guest's access scope.
    scope: Uuid,
    _provider: MockProvider,
}

async fn user(db: &DatabaseConnection, provider: &MockProvider, subject: &str) -> Uuid {
    AppUserRepo::create(
        db,
        NewAppUser {
            issuer: provider.issuer().to_string(),
            subject: subject.to_string(),
            email: Some(format!("{subject}@example.invalid")),
            email_verified: true,
            provider_name: Some(format!("Fictive {subject}")),
            username: Some(subject.to_string()),
            locale: None,
            instance_role: InstanceRole::User,
            can_create_trees: false,
        },
    )
    .await
    .expect("creates the user")
    .id
}

async fn fixture() -> Fixture {
    let provider = MockProvider::start().await;
    let db = setup_db().await;
    // Built by the local owner, the tree's every record.
    let local = app_on(db.clone());
    let tree = populated_tree(&local, &db, "Leak Fixture", 1).await;
    let anchor = tree.id("person_id").to_string();
    let base = format!("/api/v1/trees/{}", tree.tree_id);
    ok(
        &local,
        Method::POST,
        &format!("{base}/exclusions"),
        Some(json!({ "kind": "person", "person_id": anchor })),
    )
    .await;
    let visible_person = tree.id("spouse_id").to_string();
    ok(
        &local,
        Method::POST,
        &format!("{base}/notes"),
        Some(json!({ "text": CONFIDENTIAL_MARKER, "person_id": visible_person, "confidential": true })),
    )
    .await;
    let (app, _) = provider.app(&db, &[]);
    let tree_id: Uuid = tree.tree_id.parse().unwrap();
    // Every signed-in user is a Visitor of the tree.
    TreeRepo::update(
        &db,
        tree_id,
        TreeChanges {
            visibility: Some(TreeVisibility::Site),
            ..TreeChanges::default()
        },
    )
    .await
    .expect("opens the tree to the site");
    // A scope away from the anchor's couple: an unrelated person and their
    // ancestors.
    let definition: ScopeDefinition = serde_json::from_value(
        json!({ "kind": "ancestors_of", "person_id": tree.id("other_person_id") }),
    )
    .unwrap();
    let scope = oxidgene_api::service::access_scope::create(&db, tree_id, &definition)
        .await
        .expect("creates the scope");
    let mut tokens = BTreeMap::new();
    for who in VIEWERS {
        let subject = format!("{who:?}").to_lowercase();
        let id = user(&db, &provider, &subject).await;
        if let Some(role) = role(who) {
            MembershipRepo::create_shared(
                &db,
                SharedMembership {
                    tree_id,
                    user_id: id,
                    role,
                    scope_id: (who == Who::ScopedGuest).then_some(scope.id),
                    expires_at: None,
                    created_by: None,
                },
            )
            .await
            .expect("shares the tree");
        }
        tokens.insert(who, provider.access_token(&subject, TRUSTED_CLIENT, 3600));
    }
    let mut visible = Populated {
        tree_id: tree.tree_id.clone(),
        ids: tree.ids.clone(),
    };
    for key in ["person_id", "root_person_id", "child_id", "record_id"] {
        visible.ids.insert(key, visible_person.clone());
    }
    visible.ids.insert("spouse_id", anchor.clone());
    Fixture {
        app,
        db,
        tokens,
        tree,
        anchor,
        visible,
        scope: scope.id,
        _provider: provider,
    }
}

impl Fixture {
    async fn send(
        &self,
        who: Who,
        request: axum::http::request::Builder,
        body: Body,
    ) -> (StatusCode, String) {
        let request = request
            .header(header::HOST, "genealogy.example.invalid")
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", self.tokens[&who]),
            );
        let response = self
            .app
            .clone()
            .oneshot(request.body(body).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    async fn rest(
        &self,
        who: Who,
        method: Method,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, String) {
        let mut request = Request::builder().method(method).uri(uri);
        let body = match body {
            Some(body) => {
                request = request.header(header::CONTENT_TYPE, "application/json");
                Body::from(body.to_string())
            }
            None => Body::empty(),
        };
        self.send(who, request, body).await
    }

    #[cfg(feature = "graphql")]
    async fn gql(&self, who: Who, query: &str) -> Value {
        let request = Request::builder()
            .method(Method::POST)
            .uri("/graphql")
            .header(header::CONTENT_TYPE, "application/json");
        let (_, body) = self
            .send(
                who,
                request,
                Body::from(json!({ "query": query }).to_string()),
            )
            .await;
        serde_json::from_str(&body).unwrap_or(Value::Null)
    }
}

/// The canaries `text` holds.
fn leaked(text: &str, canaries: &[String]) -> Vec<String> {
    canaries
        .iter()
        .filter(|canary| text.contains(canary.as_str()))
        .cloned()
        .collect()
}

/// POST routes that only read.
const READ_POSTS: &[&str] = &[
    "/api/v1/trees/{tree_id}/gallery-bundle",
    "/api/v1/trees/{tree_id}/image-data",
    "/api/v1/trees/{tree_id}/pedigrees",
    "/api/v1/trees/{tree_id}/portrait-images",
    "/api/v1/trees/{tree_id}/relation-labels",
];

#[tokio::test]
async fn no_rest_read_shows_a_viewer_what_they_may_not_see() {
    let f = fixture().await;
    let spec = Spec(
        serde_json::from_str(
            &f.rest(Who::Editor, Method::GET, "/api/v1/openapi.json", None)
                .await
                .1,
        )
        .unwrap(),
    );
    let paths = spec.0["paths"].as_object().unwrap().clone();
    let mut leaks = Vec::new();
    let mut checked = 0;
    for who in VIEWERS {
        let canaries = canaries(who, &f);
        for records in [&f.tree, &f.visible] {
            for (path, operations) in &paths {
                if !path.contains("{tree_id}") {
                    continue;
                }
                for (method, operation) in operations.as_object().unwrap() {
                    let read = method == "get"
                        || (method == "post" && READ_POSTS.contains(&path.as_str()));
                    if !read {
                        continue;
                    }
                    let Ok((uri, _)) = spec.fill(path, operation, &records.tree_id, records) else {
                        continue;
                    };
                    let body = (method == "post")
                        .then(|| spec.request_body(operation, records, path))
                        .flatten();
                    let method = Method::from_bytes(method.to_uppercase().as_bytes()).unwrap();
                    let (status, answer) = f.rest(who, method.clone(), &uri, body).await;
                    checked += 1;
                    if !status.is_success() {
                        continue;
                    }
                    let found = leaked(&answer, &canaries);
                    if !found.is_empty() {
                        leaks.push(format!("{who:?} {method} {path}: {found:?}"));
                    }
                }
            }
        }
    }
    assert!(checked > 100, "only {checked} reads checked");
    assert!(leaks.is_empty(), "reads that leaked:\n{}", leaks.join("\n"));
}

#[cfg(feature = "graphql")]
#[tokio::test]
async fn no_graphql_query_shows_a_viewer_what_they_may_not_see() {
    use crate::guards::introspection::{INTROSPECTION, Schema};

    let f = fixture().await;
    let schema = Schema(f.gql(Who::Editor, INTROSPECTION).await["data"]["__schema"].clone());
    let mut leaks = Vec::new();
    let mut checked = 0;
    for who in VIEWERS {
        let canaries = canaries(who, &f);
        for records in [&f.tree, &f.visible] {
            for field in schema.root_fields("queryType") {
                let name = field["name"].as_str().unwrap();
                let takes_tree = field["args"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|a| a["name"] == "treeId" || a["name"] == "treeIds");
                if !takes_tree {
                    continue;
                }
                let Ok((rendered, _)) = schema.all_arguments(&field, &records.tree_id, records)
                else {
                    continue;
                };
                let arguments = if rendered.is_empty() {
                    String::new()
                } else {
                    format!("({rendered})")
                };
                let query = format!(
                    "query {{ {name}{arguments}{} }}",
                    schema.selection(&field["type"], 0)
                );
                let response = f.gql(who, &query).await;
                checked += 1;
                let found = leaked(&response["data"].to_string(), &canaries);
                if !found.is_empty() {
                    leaks.push(format!("{who:?} {name}: {found:?}"));
                }
            }
        }
    }
    assert!(checked > 60, "only {checked} queries checked");
    assert!(
        leaks.is_empty(),
        "queries that leaked:\n{}",
        leaks.join("\n")
    );
}

#[tokio::test]
async fn counts_leave_the_hidden_persons_out() {
    let f = fixture().await;
    let base = format!("/api/v1/trees/{}", f.tree.tree_id);
    let total = |text: &str| -> u64 {
        serde_json::from_str::<Value>(text).unwrap()["total_count"]
            .as_u64()
            .unwrap()
    };
    let local = app_on(f.db.clone());
    let all = ok(
        &local,
        Method::GET,
        &format!("{base}/persons/search?q=a"),
        None,
    )
    .await;
    let all = all["total_count"].as_u64().unwrap();
    let all_profiles = ok(
        &local,
        Method::GET,
        &format!("{base}/profiles?first=1"),
        None,
    )
    .await;
    let all_profiles = all_profiles["total_count"].as_u64().unwrap();
    let all_growth = net_growth(
        &ok(
            &local,
            Method::GET,
            &format!("{base}/statistics/growth"),
            None,
        )
        .await
        .to_string(),
    );
    for who in VIEWERS {
        let (_, search) = f
            .rest(
                who,
                Method::GET,
                &format!("{base}/persons/search?q=a"),
                None,
            )
            .await;
        assert!(total(&search) < all, "{who:?} {search}");
        let (_, profiles) = f
            .rest(who, Method::GET, &format!("{base}/profiles?first=1"), None)
            .await;
        // The anchor and the persons the fixture excludes itself; a scoped
        // viewer's own scope; a Visitor's, the privacy rules hiding more.
        let profiles = total(&profiles);
        match who {
            Who::Guest | Who::Editor => assert_eq!(profiles, all_profiles - 2, "{who:?}"),
            Who::ScopedGuest => {
                let scope = AccessScopeRepo::persons(&f.db, f.scope).await.unwrap();
                let tree_id = f.tree.tree_id.parse().unwrap();
                let excluded = oxidgene_api::service::exclusion::excluded_persons(&f.db, tree_id)
                    .await
                    .unwrap();
                let shown = scope.difference(&excluded).count() as u64;
                assert!(shown > 0);
                assert_eq!(profiles, shown, "{who:?}");
            }
            Who::Visitor => assert!(profiles <= all_profiles - 2, "{who:?}"),
        }

        let (status, _) = f
            .rest(
                who,
                Method::POST,
                &format!("{base}/tags/target"),
                Some(json!({ "target": { "persons": [f.anchor] } })),
            )
            .await;
        // Guests may not tag at all; an Editor may not name a hidden person.
        let refused = match who {
            Who::Editor => StatusCode::NOT_FOUND,
            Who::Guest | Who::ScopedGuest | Who::Visitor => StatusCode::FORBIDDEN,
        };
        assert_eq!(status, refused, "{who:?} tags a hidden person");

        let (_, growth) = f
            .rest(who, Method::GET, &format!("{base}/statistics/growth"), None)
            .await;
        let growth = net_growth(&growth);
        match who {
            Who::Guest | Who::Editor => assert_eq!(growth, all_growth - 2, "{who:?}"),
            Who::ScopedGuest | Who::Visitor => assert!(growth <= all_growth - 2, "{who:?}"),
        }
    }
}

/// The persons a growth chart adds up to.
fn net_growth(text: &str) -> i64 {
    serde_json::from_str::<Value>(text).unwrap()["days"]
        .as_array()
        .unwrap()
        .iter()
        .map(|day| day["added"].as_i64().unwrap() - day["removed"].as_i64().unwrap())
        .sum()
}
