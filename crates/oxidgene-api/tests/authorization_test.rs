//! Authorization of the web build (docs/authorization.md §12): the role ×
//! operation matrix on both surfaces, generated from the declared
//! requirements; memberships, ownership and the tree list; audit authors;
//! the access log; and a job whose actor loses the tree.
//!
//! Every viewer calls every REST route and every GraphQL root field:
//! anonymous, a signed-in outsider, an instance administrator who is no
//! member, the holder of a share link, the tree's guest, editor and owner,
//! and a guest and an editor limited to a scope; then, the tree made
//! `public`, the anonymous and the outsider again, now Visitors. A refusal
//! must be the exact `401`, `403` or `404` the policy owes that viewer;
//! anything else must not be a refusal. API clients join the matrix with
//! phase 6.

use std::collections::BTreeMap;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt as _;
use oxidgene_api::authz::Requirement;
use oxidgene_api::authz::operations;
use oxidgene_core::access::{InstanceRole, Permission, TreeRole};
use oxidgene_db::repo::{AppUserRepo, MembershipRepo, NewAppUser};
use oxidgene_db::sea_orm::DatabaseConnection;
use serde_json::{Value, json};
use tower::ServiceExt as _;
use uuid::Uuid;

use crate::common::oidc::{MockProvider, TRUSTED_CLIENT};
use crate::common::setup_db;

/// A viewer of the matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Who {
    Anonymous,
    Outsider,
    Admin,
    /// An anonymous browser holding a share link to the tree.
    LinkHolder,
    Guest,
    Editor,
    /// A guest limited to a scope.
    ScopedGuest,
    /// An editor limited to a scope (docs/authorization.md §3.3).
    ScopedEditor,
    Owner,
}

const VIEWERS: [Who; 9] = [
    Who::Anonymous,
    Who::Outsider,
    Who::Admin,
    Who::LinkHolder,
    Who::Guest,
    Who::Editor,
    Who::ScopedGuest,
    Who::ScopedEditor,
    Who::Owner,
];

/// The viewers of a `public` tree who are no member: Visitors.
const PUBLIC_VIEWERS: [Who; 2] = [Who::Anonymous, Who::Outsider];

/// What the policy owes a viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    /// Anything but a refusal: the handler ran.
    Allowed,
    Unauthenticated,
    Forbidden,
    NotFound,
}

fn role(who: Who, public: bool) -> Option<TreeRole> {
    match who {
        Who::Guest | Who::ScopedGuest => Some(TreeRole::Guest),
        Who::Editor | Who::ScopedEditor => Some(TreeRole::Editor),
        Who::Owner => Some(TreeRole::Owner),
        Who::LinkHolder => Some(TreeRole::Visitor),
        Who::Anonymous | Who::Outsider if public => Some(TreeRole::Visitor),
        _ => None,
    }
}

/// Whether `who` has no account.
fn anonymous(who: Who) -> bool {
    matches!(who, Who::Anonymous | Who::LinkHolder)
}

/// The outcome `requirement` owes `who` on the fixture tree, `public` or
/// private. Only the owner may create trees, only the administrator
/// administers.
fn expected(requirement: Requirement, who: Who, public: bool) -> Outcome {
    match requirement {
        Requirement::Open | Requirement::Trees(_) => return Outcome::Allowed,
        Requirement::Tree(permission) if anonymous(who) => {
            return match role(who, public) {
                None => Outcome::Unauthenticated,
                Some(role) if role.permissions().contains(permission) => Outcome::Allowed,
                Some(_) => Outcome::Forbidden,
            };
        }
        _ if anonymous(who) => return Outcome::Unauthenticated,
        _ => {}
    }
    match requirement {
        Requirement::Open | Requirement::SignedIn | Requirement::Trees(_) => Outcome::Allowed,
        Requirement::TreeCreation if who == Who::Owner => Outcome::Allowed,
        Requirement::Administrator if who == Who::Admin => Outcome::Allowed,
        Requirement::TreeCreation | Requirement::Administrator => Outcome::Forbidden,
        Requirement::Tree(permission) => match role(who, public) {
            None => Outcome::NotFound,
            Some(role) if role.permissions().contains(permission) => Outcome::Allowed,
            Some(_) => Outcome::Forbidden,
        },
    }
}

/// Operations the generic matrix cannot call as a permitted viewer without
/// ending the fixture: deleting the tree, erasing one's account. Their
/// refusals are still checked.
fn destructive(operation: &str) -> bool {
    matches!(
        operation,
        "DELETE /api/v1/trees/{tree_id}"
            | "DELETE /api/v1/me"
            | "Mutation.deleteTree"
            | "Mutation.deleteMyAccount"
    )
}

/// What a member's own service answers beyond the declared requirement:
/// removing another member takes `members_manage` (§10); the members
/// operations, which a Visitor's `read` passes, need an account; a scoped
/// Editor may not change a record the whole tree shares (§3.3).
fn narrowed(operation: &str, who: Who, requirement: Requirement, public: bool) -> Option<Outcome> {
    let removes = matches!(
        operation,
        "DELETE /api/v1/trees/{tree_id}/members/{user_id}" | "Mutation.removeTreeMember"
    );
    let members = matches!(
        operation,
        "GET /api/v1/trees/{tree_id}/members"
            | "PUT /api/v1/trees/{tree_id}/members/{user_id}"
            | "DELETE /api/v1/trees/{tree_id}/members/{user_id}"
            | "Query.treeMembers"
            | "Mutation.updateTreeMember"
            | "Mutation.removeTreeMember"
    );
    if members && anonymous(who) {
        return Some(Outcome::Unauthenticated);
    }
    let writes = matches!(
        requirement,
        Requirement::Tree(
            Permission::Create
                | Permission::Edit
                | Permission::PrivacyEdit
                | Permission::Delete
                | Permission::Merge
                | Permission::HistoryRestore
        )
    );
    if who == Who::ScopedEditor && writes {
        let shared = match operation.split_once(' ') {
            Some((_, route)) => oxidgene_api::authz::scoped::refuses_route(route),
            None => operation
                .split_once('.')
                .is_some_and(|(_, field)| oxidgene_api::authz::scoped::refuses_field(field)),
        };
        if shared {
            return Some(Outcome::Forbidden);
        }
    }
    // Anyone signed in who sees the tree but is not its owner.
    let sees = matches!(
        who,
        Who::Guest | Who::Editor | Who::ScopedGuest | Who::ScopedEditor
    ) || (public && who == Who::Outsider);
    (removes && sees).then_some(Outcome::Forbidden)
}

struct Fixture {
    app: axum::Router,
    db: DatabaseConnection,
    tokens: BTreeMap<Who, String>,
    /// The share-link cookie of the link holder.
    share_cookie: String,
    tree_id: Uuid,
    users: BTreeMap<Who, Uuid>,
}

async fn user(
    db: &DatabaseConnection,
    provider: &MockProvider,
    subject: &str,
    admin: bool,
    creates: bool,
) -> Uuid {
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
            instance_role: if admin {
                InstanceRole::Admin
            } else {
                InstanceRole::User
            },
            can_create_trees: creates,
        },
    )
    .await
    .expect("creates the user")
    .id
}

async fn fixture() -> (Fixture, MockProvider) {
    let provider = MockProvider::start().await;
    let db = setup_db().await;
    let (app, _) = provider.app(&db, &[]);
    let mut tokens = BTreeMap::new();
    let mut users = BTreeMap::new();
    for (who, subject) in [
        (Who::Outsider, "outsider"),
        (Who::Admin, "admin"),
        (Who::Guest, "guest"),
        (Who::Editor, "editor"),
        (Who::ScopedGuest, "scoped-guest"),
        (Who::ScopedEditor, "scoped-editor"),
        (Who::Owner, "owner"),
    ] {
        let id = user(
            &db,
            &provider,
            subject,
            who == Who::Admin,
            who == Who::Owner,
        )
        .await;
        users.insert(who, id);
        tokens.insert(who, provider.access_token(subject, TRUSTED_CLIENT, 3600));
    }
    let mut fixture = Fixture {
        app,
        db,
        tokens,
        share_cookie: String::new(),
        tree_id: Uuid::nil(),
        users,
    };
    let (status, tree) = fixture
        .rest(
            Who::Owner,
            Method::POST,
            "/api/v1/trees",
            Some(json!({ "name": "Matrix Fixture" })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{tree}");
    fixture.tree_id = tree["id"].as_str().unwrap().parse().unwrap();
    for (who, role) in [
        (Who::Guest, TreeRole::Guest),
        (Who::Editor, TreeRole::Editor),
    ] {
        MembershipRepo::create(
            &fixture.db,
            fixture.tree_id,
            fixture.users[&who],
            role,
            None,
        )
        .await
        .expect("shares the tree");
    }
    fixture.share_scoped().await;
    fixture.open_share_link().await;
    (fixture, provider)
}

impl Fixture {
    /// Share the tree with the scoped guest and editor, limited to the
    /// descendants of a person of their own.
    async fn share_scoped(&self) {
        let (status, person) = self
            .rest(
                Who::Owner,
                Method::POST,
                &format!("/api/v1/trees/{}/persons", self.tree_id),
                Some(json!({ "sex": "unknown" })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{person}");
        for (username, role) in [("scoped-guest", "guest"), ("scoped-editor", "editor")] {
            let (status, shared) = self
                .rest(
                    Who::Owner,
                    Method::POST,
                    &format!("/api/v1/trees/{}/members", self.tree_id),
                    Some(json!({
                        "username": username,
                        "role": role,
                        "scope": { "kind": "descendants_of", "person_id": person["id"] },
                    })),
                )
                .await;
            assert_eq!(status, StatusCode::CREATED, "{shared}");
        }
    }

    /// Create a share link and open it anonymously: the link holder's
    /// cookie.
    async fn open_share_link(&mut self) {
        let (status, link) = self
            .rest(
                Who::Owner,
                Method::POST,
                &format!("/api/v1/trees/{}/share-links", self.tree_id),
                Some(json!({ "label": "Matrix" })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{link}");
        let path = link["url"]
            .as_str()
            .unwrap()
            .strip_prefix(crate::common::oidc::PUBLIC_URL)
            .unwrap()
            .to_owned();
        let opened = self
            .app
            .clone()
            .oneshot(
                Request::get(path)
                    .header(header::HOST, "genealogy.example.invalid")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let cookie = opened.headers()[header::SET_COOKIE].to_str().unwrap();
        self.share_cookie = cookie.split(';').next().unwrap().to_owned();
    }

    /// Make the fixture tree `public`: anyone is a Visitor of it.
    async fn make_public(&self) {
        let (status, tree) = self
            .rest(
                Who::Owner,
                Method::PUT,
                &format!("/api/v1/trees/{}", self.tree_id),
                Some(json!({ "visibility": "public" })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{tree}");
    }

    async fn send(
        &self,
        who: Who,
        request: axum::http::request::Builder,
        body: Body,
    ) -> (StatusCode, Value) {
        let mut request = request.header(header::HOST, "genealogy.example.invalid");
        if let Some(token) = self.tokens.get(&who) {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        if who == Who::LinkHolder {
            request = request.header(header::COOKIE, &self.share_cookie);
        }
        let response = self
            .app
            .clone()
            .oneshot(request.body(body).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn rest(
        &self,
        who: Who,
        method: Method,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
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

    async fn gql(&self, who: Who, query: &str) -> Value {
        let request = Request::builder()
            .method(Method::POST)
            .uri("/graphql")
            .header(header::CONTENT_TYPE, "application/json");
        self.send(
            who,
            request,
            Body::from(json!({ "query": query }).to_string()),
        )
        .await
        .1
    }
}

/// `route` with its parameters filled: the fixture tree, values its
/// handlers parse, a fresh identifier for everything else.
fn concrete(route: &str, tree_id: Uuid) -> String {
    route
        .split('/')
        .map(|segment| match segment {
            "{tree_id}" => tree_id.to_string(),
            "{lang}" => "en".to_string(),
            "{record_type}" | "{kind}" => "person".to_string(),
            "{field}" => "occupation".to_string(),
            "{number}" | "{version}" => "1".to_string(),
            s if s.starts_with('{') => Uuid::now_v7().to_string(),
            s => s.to_string(),
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn rest_outcome(status: StatusCode) -> Outcome {
    match status {
        StatusCode::UNAUTHORIZED => Outcome::Unauthenticated,
        StatusCode::FORBIDDEN => Outcome::Forbidden,
        StatusCode::NOT_FOUND => Outcome::NotFound,
        _ => Outcome::Allowed,
    }
}

/// Whether `actual` is what `expected` asks: a permitted viewer may meet a
/// `404` of the record the operation names, never a refusal of its own.
fn matches(expected: Outcome, actual: Outcome) -> bool {
    match expected {
        Outcome::Allowed => matches!(actual, Outcome::Allowed | Outcome::NotFound),
        refusal => actual == refusal,
    }
}

#[tokio::test]
async fn every_rest_route_answers_each_viewer_as_its_requirement_says() {
    let (fixture, _provider) = fixture().await;
    rest_matrix(&fixture, &VIEWERS, false, 1500).await;
}

#[tokio::test]
async fn every_rest_route_answers_the_visitors_of_a_public_tree() {
    let (fixture, _provider) = fixture().await;
    fixture.make_public().await;
    rest_matrix(&fixture, &PUBLIC_VIEWERS, true, 300).await;
}

/// Call every REST route as each of `viewers`, the tree `public` or not,
/// and check the outcome; at least `least` calls.
async fn rest_matrix(fixture: &Fixture, viewers: &[Who], public: bool, least: usize) {
    let mut failures = Vec::new();
    let mut calls = 0;
    // Refusals first: a permitted viewer's call must not change what the
    // refused ones meet.
    for refusals_only in [true, false] {
        for (method, route, requirement) in operations::rest_operations() {
            let operation = format!("{method} /api/v1{route}");
            for &who in viewers {
                let wanted = narrowed(&operation, who, requirement, public)
                    .unwrap_or_else(|| expected(requirement, who, public));
                if refusals_only == (wanted == Outcome::Allowed)
                    || (wanted == Outcome::Allowed && destructive(&operation))
                {
                    continue;
                }
                let uri = concrete(&format!("/api/v1{route}"), fixture.tree_id);
                let body = matches!(method, "POST" | "PUT" | "PATCH").then(|| json!({}));
                let method = Method::from_bytes(method.as_bytes()).unwrap();
                let (status, answer) = fixture.rest(who, method, &uri, body).await;
                calls += 1;
                if !matches(wanted, rest_outcome(status)) {
                    failures.push(format!(
                        "{operation} as {who:?}: {status} {answer}, wanted {wanted:?}"
                    ));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {calls}:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert!(calls > least, "{calls} calls");
}

// ── GraphQL documents from introspection ─────────────────────────────────

const TYPE_REF: &str =
    "kind name ofType { kind name ofType { kind name ofType { kind name ofType { kind name } } } }";

struct Schema {
    types: BTreeMap<String, Value>,
}

impl Schema {
    /// A literal of input type `ty`, with every required field.
    fn literal(&self, ty: &Value, name: &str, tree_id: Uuid) -> String {
        match ty["kind"].as_str().unwrap() {
            "NON_NULL" => self.literal(&ty["ofType"], name, tree_id),
            "LIST" => "[]".to_string(),
            "ENUM" => {
                let ty = &self.types[ty["name"].as_str().unwrap()];
                ty["enumValues"][0]["name"].as_str().unwrap().to_string()
            }
            "INPUT_OBJECT" => {
                let ty = &self.types[ty["name"].as_str().unwrap()];
                let fields: Vec<String> = ty["inputFields"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|field| required(field))
                    .map(|field| {
                        let name = field["name"].as_str().unwrap();
                        format!("{name}: {}", self.literal(&field["type"], name, tree_id))
                    })
                    .collect();
                format!("{{ {} }}", fields.join(", "))
            }
            _ => match ty["name"].as_str().unwrap() {
                "ID" if name == "treeId" => format!("\"{tree_id}\""),
                "ID" => format!("\"{}\"", Uuid::now_v7()),
                "Int" => "1".to_string(),
                "Float" => "1.0".to_string(),
                "Boolean" => "false".to_string(),
                "DateTime" => "\"2000-01-01T00:00:00Z\"".to_string(),
                "NaiveDate" => "\"2000-01-01\"".to_string(),
                "UUID" => format!("\"{}\"", Uuid::now_v7()),
                _ => "\"x\"".to_string(),
            },
        }
    }

    /// The selection a field of type `ty` needs.
    fn selection(&self, ty: &Value) -> &'static str {
        match ty["kind"].as_str().unwrap() {
            "NON_NULL" | "LIST" => self.selection(&ty["ofType"]),
            "OBJECT" | "INTERFACE" | "UNION" => " { __typename }",
            _ => "",
        }
    }

    /// The document calling root field `field` of `root`.
    fn document(&self, root: &str, field: &Value, tree_id: Uuid) -> String {
        let name = field["name"].as_str().unwrap();
        let args: Vec<String> = field["args"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|arg| required(arg))
            .map(|arg| {
                let arg_name = arg["name"].as_str().unwrap();
                // The tree operations name their tree `id`.
                let arg_name_for_value =
                    if matches!(name, "tree" | "updateTree" | "deleteTree") && arg_name == "id" {
                        "treeId"
                    } else {
                        arg_name
                    };
                format!(
                    "{arg_name}: {}",
                    self.literal(&arg["type"], arg_name_for_value, tree_id)
                )
            })
            .collect();
        let args = if args.is_empty() {
            String::new()
        } else {
            format!("({})", args.join(", "))
        };
        let keyword = if root == "Mutation" {
            "mutation"
        } else {
            "query"
        };
        format!(
            "{keyword} {{ {name}{args}{} }}",
            self.selection(&field["type"])
        )
    }
}

fn required(input: &Value) -> bool {
    input["type"]["kind"] == "NON_NULL" && input["defaultValue"].is_null()
}

fn gql_outcome(answer: &Value, field: &str) -> Outcome {
    match answer["errors"][0]["extensions"]["code"].as_str() {
        Some("UNAUTHENTICATED") => Outcome::Unauthenticated,
        Some("FORBIDDEN") => Outcome::Forbidden,
        Some("NOT_FOUND") => Outcome::NotFound,
        // A singular query answers `null` where REST answers `404`.
        None if answer["data"][field].is_null() && answer["errors"].is_null() => Outcome::NotFound,
        _ => Outcome::Allowed,
    }
}

#[tokio::test]
async fn every_graphql_root_field_answers_each_viewer_as_its_requirement_says() {
    let (fixture, _provider) = fixture().await;
    graphql_matrix(&fixture, &VIEWERS, false, 1500).await;
}

#[tokio::test]
async fn every_graphql_root_field_answers_the_visitors_of_a_public_tree() {
    let (fixture, _provider) = fixture().await;
    fixture.make_public().await;
    graphql_matrix(&fixture, &PUBLIC_VIEWERS, true, 300).await;
}

/// Call every GraphQL root field as each of `viewers`, the tree `public` or
/// not, and check the outcome; at least `least` calls.
async fn graphql_matrix(fixture: &Fixture, viewers: &[Who], public: bool, least: usize) {
    let introspection = fixture
        .gql(
            Who::Owner,
            &format!(
                "{{ __schema {{ queryType {{ name }} mutationType {{ name }} types {{ name kind \
                 fields {{ name args {{ name defaultValue type {{ {TYPE_REF} }} }} type {{ {TYPE_REF} }} }} \
                 inputFields {{ name defaultValue type {{ {TYPE_REF} }} }} enumValues {{ name }} }} }} }}"
            ),
        )
        .await;
    let schema = &introspection["data"]["__schema"];
    let types: BTreeMap<String, Value> = schema["types"]
        .as_array()
        .expect("introspection answers")
        .iter()
        .map(|ty| (ty["name"].as_str().unwrap().to_string(), ty.clone()))
        .collect();
    let schema_types = Schema { types };
    let mut failures = Vec::new();
    let mut calls = 0;
    let roots = [
        ("Query", schema["queryType"]["name"].as_str().unwrap()),
        ("Mutation", schema["mutationType"]["name"].as_str().unwrap()),
    ];
    for refusals_only in [true, false] {
        for (root, type_name) in roots {
            for field in schema_types.types[type_name]["fields"].as_array().unwrap() {
                let name = field["name"].as_str().unwrap();
                let operation = format!("{root}.{name}");
                let requirement = operations::graphql(name)
                    .unwrap_or_else(|| panic!("{operation} declares no requirement"));
                let document = schema_types.document(root, field, fixture.tree_id);
                for &who in viewers {
                    let wanted = narrowed(&operation, who, requirement, public)
                        .unwrap_or_else(|| expected(requirement, who, public));
                    if refusals_only == (wanted == Outcome::Allowed)
                        || (wanted == Outcome::Allowed && destructive(&operation))
                    {
                        continue;
                    }
                    let answer = fixture.gql(who, &document).await;
                    calls += 1;
                    if !matches(wanted, gql_outcome(&answer, name)) {
                        failures.push(format!(
                            "{operation} as {who:?}: {answer}, wanted {wanted:?}\n  {document}"
                        ));
                    }
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {calls}:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert!(calls > least, "{calls} calls");
}

// ── Memberships and ownership ───────────────────────────────────────────

#[tokio::test]
async fn the_tree_list_is_the_viewers_memberships_with_their_access() {
    let (fixture, _provider) = fixture().await;
    let (status, list) = fixture
        .rest(Who::Guest, Method::GET, "/api/v1/trees", None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["total_count"], 1, "{list}");
    let node = &list["edges"][0]["node"];
    assert_eq!(node["viewer_access"]["role"], "guest");
    let permissions = node["viewer_access"]["permissions"].as_array().unwrap();
    assert!(permissions.contains(&json!("read")));
    assert!(!permissions.contains(&json!("edit")));
    let (_, list) = fixture
        .rest(Who::Outsider, Method::GET, "/api/v1/trees", None)
        .await;
    assert_eq!(list["total_count"], 0, "{list}");
    let (_, list) = fixture
        .rest(Who::Admin, Method::GET, "/api/v1/trees", None)
        .await;
    assert_eq!(
        list["total_count"], 0,
        "an administrator reads no tree's content"
    );

    let tree = fixture.tree_id;
    let answer = fixture
        .gql(
            Who::Editor,
            &format!(
                r#"{{ tree(id: "{tree}") {{ viewerAccess {{ role permissions scoped }} }} }}"#
            ),
        )
        .await;
    assert_eq!(
        answer["data"]["tree"]["viewerAccess"]["role"], "editor",
        "{answer}"
    );
    let (_, one) = fixture
        .rest(
            Who::Owner,
            Method::GET,
            &format!("/api/v1/trees/{tree}"),
            None,
        )
        .await;
    assert_eq!(one["viewer_access"]["role"], "owner", "{one}");
}

#[tokio::test]
async fn members_leave_the_owner_hands_over_and_cannot_leave() {
    let (fixture, _provider) = fixture().await;
    let tree = fixture.tree_id;
    let members = format!("/api/v1/trees/{tree}/members");
    let (status, list) = fixture.rest(Who::Owner, Method::GET, &members, None).await;
    assert_eq!(status, StatusCode::OK);
    // The owner, the guest, the editor and the two scoped members.
    assert_eq!(list["total_count"], 5, "{list}");
    // The members page by cursor, as every collection does.
    let (_, first) = fixture
        .rest(Who::Owner, Method::GET, &format!("{members}?first=2"), None)
        .await;
    assert_eq!(first["edges"].as_array().unwrap().len(), 2);
    assert_eq!(first["page_info"]["has_next_page"], true);
    let after = first["page_info"]["end_cursor"].as_str().unwrap();
    let (_, next) = fixture
        .rest(
            Who::Owner,
            Method::GET,
            &format!("{members}?first=2&after={after}"),
            None,
        )
        .await;
    assert_eq!(next["edges"].as_array().unwrap().len(), 2, "{next}");
    let (_, own) = fixture.rest(Who::Guest, Method::GET, &members, None).await;
    assert_eq!(own["total_count"], 1, "a member reads their own row: {own}");
    assert_eq!(own["edges"][0]["node"]["role"], "guest");

    // The owner promotes the guest; the guest cannot change roles.
    let guest = fixture.users[&Who::Guest];
    let (status, _) = fixture
        .rest(
            Who::Guest,
            Method::PUT,
            &format!("{members}/{guest}"),
            Some(json!({ "role": "editor" })),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, member) = fixture
        .rest(
            Who::Owner,
            Method::PUT,
            &format!("{members}/{guest}"),
            Some(json!({ "role": "editor" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{member}");
    assert_eq!(member["role"], "editor");

    // The owner cannot leave; a member can.
    let owner = fixture.users[&Who::Owner];
    let (status, _) = fixture
        .rest(
            Who::Owner,
            Method::DELETE,
            &format!("{members}/{owner}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = fixture
        .rest(
            Who::Guest,
            Method::DELETE,
            &format!("{members}/{guest}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = fixture
        .rest(
            Who::Guest,
            Method::GET,
            &format!("/api/v1/trees/{tree}"),
            None,
        )
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a former member sees nothing"
    );

    // The owner hands the tree to the editor over GraphQL and becomes one.
    let editor = fixture.users[&Who::Editor];
    let answer = fixture
        .gql(
            Who::Owner,
            &format!(
                r#"mutation {{ transferTreeOwnership(treeId: "{tree}", userId: "{editor}") {{ role }} }}"#
            ),
        )
        .await;
    assert_eq!(
        answer["data"]["transferTreeOwnership"]["role"], "owner",
        "{answer}"
    );
    assert_eq!(
        MembershipRepo::live_role(&fixture.db, tree, owner, chrono::Utc::now())
            .await
            .unwrap(),
        Some(TreeRole::Editor)
    );
    let (status, _) = fixture
        .rest(
            Who::Owner,
            Method::PUT,
            &format!("/api/v1/trees/{tree}"),
            Some(json!({ "name": "Renamed" })),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "the former owner manages no settings"
    );

    // Every change is in the tree's history, under `access`, by its author.
    let (_, audit) = fixture
        .rest(
            Who::Editor,
            Method::GET,
            &format!("/api/v1/trees/{tree}/audit?category=access"),
            None,
        )
        .await;
    let entries = audit["edges"].as_array().unwrap();
    // The test's three changes, newest first, after the fixture's shares.
    assert_eq!(entries.len(), 6, "{audit}");
    assert!(
        entries
            .iter()
            .take(3)
            .all(|e| e["node"]["entity"] == "membership")
    );
    assert_eq!(
        entries[0]["node"]["author"]["user_id"],
        json!(owner.to_string())
    );
    assert_eq!(entries[0]["node"]["author"]["kind"], "user");
    assert_eq!(entries[0]["node"]["author"]["client_id"], TRUSTED_CLIENT);
}

#[tokio::test]
async fn an_owner_cannot_erase_their_account_and_an_administrator_assigns_ownerless_trees() {
    let (fixture, _provider) = fixture().await;
    let (status, _) = fixture
        .rest(Who::Owner, Method::DELETE, "/api/v1/me", None)
        .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "an owner transfers or deletes first"
    );

    // A tree the `none` mode created has no owner: only an administrator
    // sees it, and gives it one.
    let ownerless = Uuid::now_v7();
    oxidgene_db::repo::TreeRepo::create(&fixture.db, ownerless, "Ownerless".into(), None)
        .await
        .unwrap();
    let (_, trees) = fixture
        .rest(Who::Admin, Method::GET, "/api/v1/admin/trees", None)
        .await;
    let listed = trees["edges"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["node"]["id"] == json!(ownerless.to_string()))
        .expect("listed");
    assert!(listed["node"]["owner_id"].is_null());
    let outsider = fixture.users[&Who::Outsider];
    let uri = format!("/api/v1/admin/trees/{ownerless}/owner");
    let (status, _) = fixture
        .rest(
            Who::Outsider,
            Method::PUT,
            &uri,
            Some(json!({ "user_id": outsider })),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, owner) = fixture
        .rest(
            Who::Admin,
            Method::PUT,
            &uri,
            Some(json!({ "user_id": outsider })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{owner}");
    let (status, _) = fixture
        .rest(
            Who::Admin,
            Method::PUT,
            &uri,
            Some(json!({ "user_id": outsider })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "the tree has an owner now");
    let (status, _) = fixture
        .rest(
            Who::Outsider,
            Method::GET,
            &format!("/api/v1/trees/{ownerless}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
}

// ── Audit authors and the access log ────────────────────────────────────

#[tokio::test]
async fn every_write_names_its_author_and_a_refusal_is_logged() {
    let (fixture, _provider) = fixture().await;
    let tree = fixture.tree_id;
    let (status, person) = fixture
        .rest(
            Who::Editor,
            Method::POST,
            &format!("/api/v1/trees/{tree}/persons"),
            Some(json!({ "sex": "unknown" })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{person}");
    let answer = fixture
        .gql(
            Who::Editor,
            &format!(r#"{{ auditEntries(treeId: "{tree}", first: 1) {{ edges {{ node {{ entity author {{ kind userId displayName clientId }} }} }} }} }}"#),
        )
        .await;
    let author = &answer["data"]["auditEntries"]["edges"][0]["node"]["author"];
    assert_eq!(author["kind"], "USER", "{answer}");
    assert_eq!(
        author["userId"],
        json!(fixture.users[&Who::Editor].to_string())
    );
    assert_eq!(author["displayName"], "Fictive editor");
    assert_eq!(author["clientId"], TRUSTED_CLIENT);

    // A guest's write is refused, and the owner reads the refusal in the
    // tree's access log; the guest may not.
    let (status, _) = fixture
        .rest(
            Who::Guest,
            Method::POST,
            &format!("/api/v1/trees/{tree}/persons"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let log = format!("/api/v1/trees/{tree}/access-log");
    // The log is written off the request path, and the fixture's sharing
    // steps log events of their own: wait for the refusal itself.
    let refusal = |events: &Value| {
        events["edges"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|edge| edge["node"].clone())
            .find(|node| node["kind"] == "request_refused")
    };
    let mut events = Value::Null;
    for _ in 0..250 {
        events = fixture.rest(Who::Owner, Method::GET, &log, None).await.1;
        if refusal(&events).is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let event = &refusal(&events).unwrap_or_else(|| panic!("no refusal logged: {events}"));
    assert_eq!(event["kind"], "request_refused", "{events}");
    assert_eq!(event["outcome"], "forbidden");
    assert_eq!(event["permission"], "create");
    assert_eq!(event["route"], "/api/v1/trees/{tree_id}/persons");
    assert_eq!(
        event["user_id"],
        json!(fixture.users[&Who::Guest].to_string())
    );
    let (status, _) = fixture.rest(Who::Guest, Method::GET, &log, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let printed = events.to_string();
    assert!(
        !printed.contains("example.invalid") && !printed.contains("Fictive"),
        "{printed}"
    );
}

#[tokio::test]
async fn a_job_whose_actor_lost_the_tree_fails_as_actor_revoked() {
    let (fixture, _provider) = fixture().await;
    let tree = fixture.tree_id;
    // The editor starts an export, then loses their membership before a
    // worker runs it.
    let (status, started) = fixture
        .rest(
            Who::Editor,
            Method::POST,
            &format!("/api/v1/trees/{tree}/export-jobs"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{started}");
    let job_id: Uuid = started["job_id"].as_str().unwrap().parse().unwrap();
    let job = oxidgene_db::repo::BackgroundJobRepo::get_in_tree(&fixture.db, tree, job_id)
        .await
        .unwrap();
    assert_eq!(job.actor_user_id, Some(fixture.users[&Who::Editor]));
    assert_eq!(job.actor_client_id.as_deref(), Some(TRUSTED_CLIENT));
    MembershipRepo::delete(&fixture.db, tree, fixture.users[&Who::Editor])
        .await
        .unwrap();
    let worker = crate::common::worker_on(&fixture.db);
    assert!(worker.run_once().await.unwrap());
    let job = oxidgene_db::repo::BackgroundJobRepo::get_in_tree(&fixture.db, tree, job_id)
        .await
        .unwrap();
    assert_eq!(job.status, "failed");
    assert_eq!(job.error_code.as_deref(), Some("actor_revoked"));
}

#[test]
fn the_requirements_cover_the_permissions_of_the_matrix() {
    // Every permission the operations check exists in some role's set, and
    // only the owner manages the tree.
    for (_, _, requirement) in operations::rest_operations() {
        if let Requirement::Tree(permission) = requirement {
            assert!(
                TreeRole::Owner.permissions().contains(permission)
                    || permission == Permission::Leave
            );
        }
    }
    for permission in [
        Permission::SettingsManage,
        Permission::MembersManage,
        Permission::TreeDelete,
        Permission::OwnershipTransfer,
        Permission::Import,
        Permission::AccessLogRead,
    ] {
        assert!(
            !TreeRole::Editor.permissions().contains(permission),
            "{permission:?}"
        );
    }
}
