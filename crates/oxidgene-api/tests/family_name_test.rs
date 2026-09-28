//! The dictionary's bulk family-name edits over REST and GraphQL.
//!
//! Every scenario runs once per surface through the same helpers, so the two
//! stay strictly symmetric: same inputs, same outcome, same errors. Fixtures
//! are built over REST; only the edit under test changes surface.
//!
//! All data is fictitious.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use oxidgene_api::{AppState, build_router};
use oxidgene_db::repo::{connect, run_migrations};
use serde_json::{Value, json};
use tower::ServiceExt;

#[derive(Debug, Clone, Copy)]
enum Surface {
    Rest,
    Graphql,
}

const SURFACES: [Surface; 2] = [Surface::Rest, Surface::Graphql];

async fn setup() -> axum::Router {
    let db = connect("sqlite::memory:")
        .await
        .expect("connect to in-memory SQLite");
    run_migrations(&db).await.expect("migrations");
    build_router(AppState::new(
        db,
        std::env::temp_dir().join("oxidgene-test-media"),
    ))
}

async fn send(
    app: &axum::Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let body = match body {
        Some(json) => Body::from(serde_json::to_vec(&json).unwrap()),
        None => Body::empty(),
    };
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(body)
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, json)
}

async fn ok(app: &axum::Router, method: Method, uri: &str, body: Option<Value>) -> Value {
    let (status, json) = send(app, method, uri, body).await;
    assert!(status.is_success(), "{uri}: {status} {json}");
    json
}

/// A GraphQL call: its data, or its errors.
async fn graphql(app: &axum::Router, query: &str, variables: Value) -> Result<Value, Value> {
    let body = json!({ "query": query, "variables": variables });
    let (status, json) = send(app, Method::POST, "/graphql", Some(body)).await;
    assert_eq!(status, StatusCode::OK);
    match json.get("errors") {
        Some(errors) => Err(errors.clone()),
        None => Ok(json["data"].clone()),
    }
}

async fn create_tree(app: &axum::Router) -> String {
    ok(
        app,
        Method::POST,
        "/api/v1/trees",
        Some(json!({ "name": "Family Name Fixture" })),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// A person whose name is already split into particle and root, as an import
/// would have stored it.
async fn create_person(
    app: &axum::Router,
    tree: &str,
    given: &str,
    particle: Option<&str>,
    root: &str,
) -> String {
    let id = ok(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/persons"),
        Some(json!({ "sex": "unknown" })),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    add_name(
        app,
        tree,
        &id,
        json!({
            "name_type": "birth",
            "given_names": given,
            "surname_prefix": particle,
            "surname": root,
            "is_primary": true,
        }),
    )
    .await;
    id
}

async fn add_name(app: &axum::Router, tree: &str, person: &str, name: Value) {
    ok(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/persons/{person}/names"),
        Some(name),
    )
    .await;
}

async fn family_names(app: &axum::Router, tree: &str) -> Vec<Value> {
    ok(
        app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/dictionary/family-names"),
        None,
    )
    .await
    .as_array()
    .unwrap()
    .clone()
}

fn entry<'a>(entries: &'a [Value], value: &str) -> Option<&'a Value> {
    entries.iter().find(|e| e["value"] == value)
}

async fn usage(app: &axum::Router, tree: &str, value: &str) -> Vec<String> {
    let mut ids: Vec<String> = ok(
        app,
        Method::GET,
        &format!(
            "/api/v1/trees/{tree}/dictionary/family-names/usage?value={}",
            value.replace(' ', "%20")
        ),
        None,
    )
    .await
    .as_array()
    .unwrap()
    .iter()
    .map(|p| p["person_id"].as_str().unwrap().to_string())
    .collect();
    ids.sort();
    ids
}

async fn audit(app: &axum::Router, tree: &str) -> Vec<Value> {
    ok(
        app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/audit?first=100"),
        None,
    )
    .await["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|edge| edge["node"].clone())
        .collect()
}

/// `PATCH …/family-names/particle` or `setFamilyNameParticle`, answered in
/// the REST shape. `Err` carries the status (400 for GraphQL's validation
/// errors, which surface as `errors`).
async fn set_particle(
    app: &axum::Router,
    surface: Surface,
    tree: &str,
    value: &str,
    particle: &str,
) -> Result<Value, StatusCode> {
    match surface {
        Surface::Rest => {
            let (status, body) = send(
                app,
                Method::PATCH,
                &format!("/api/v1/trees/{tree}/dictionary/family-names/particle"),
                Some(json!({ "value": value, "particle": particle })),
            )
            .await;
            if status.is_success() {
                Ok(body)
            } else {
                Err(status)
            }
        }
        Surface::Graphql => graphql(
            app,
            "mutation($t: ID!, $v: String!, $p: String!) {
                setFamilyNameParticle(treeId: $t, input: { value: $v, particle: $p }) {
                    value surnamePrefix surname namesUpdated personsUpdated
                }
            }",
            json!({ "t": tree, "v": value, "p": particle }),
        )
        .await
        .map(|data| {
            let u = &data["setFamilyNameParticle"];
            json!({
                "value": u["value"],
                "surname_prefix": u["surnamePrefix"],
                "surname": u["surname"],
                "names_updated": u["namesUpdated"],
                "persons_updated": u["personsUpdated"],
            })
        })
        .map_err(|_| StatusCode::BAD_REQUEST),
    }
}

#[tokio::test]
async fn a_particle_recut_reaches_every_carrier_on_both_surfaces() {
    for surface in SURFACES {
        let app = setup().await;
        let tree = create_tree(&app).await;
        // Two persons an import filed under a particle they do not have, and
        // a genuine particle next door that must be left alone.
        let a = create_person(&app, &tree, "Given_a", Some("LE"), "BRANCH").await;
        let b = create_person(&app, &tree, "Given_b", Some("LE"), "BRANCH").await;
        let cruz = create_person(&app, &tree, "Given_c", Some("de la"), "Cruz").await;

        let out = set_particle(&app, surface, &tree, "LE BRANCH", "")
            .await
            .unwrap();
        assert_eq!(out["value"], "LE BRANCH", "{surface:?}");
        assert_eq!(out["surname_prefix"], Value::Null, "{surface:?}");
        assert_eq!(out["surname"], "LE BRANCH", "{surface:?}");
        assert_eq!(out["names_updated"], 2, "{surface:?}");
        assert_eq!(out["persons_updated"], 2, "{surface:?}");

        // Same text, filed under the whole name now.
        let entries = family_names(&app, &tree).await;
        let branch = entry(&entries, "LE BRANCH").unwrap();
        assert_eq!(branch["count"], 2, "{surface:?}");
        assert_eq!(branch["sort_key"], "le branch", "{surface:?}");
        assert_eq!(
            entry(&entries, "de la Cruz").unwrap()["sort_key"],
            "cruz",
            "{surface:?}"
        );
        let mut both = vec![a.clone(), b.clone()];
        both.sort();
        assert_eq!(usage(&app, &tree, "LE BRANCH").await, both, "{surface:?}");

        // One bulk entry, one version per carrier.
        let entries = audit(&app, &tree).await;
        let recut = &entries[0];
        assert_eq!(recut["entity"], "family_name", "{surface:?}");
        assert_eq!(recut["action"], "update", "{surface:?}");
        assert_eq!(recut["label"], "LE BRANCH", "{surface:?}");
        assert_eq!(recut["details"]["count"], 2, "{surface:?}");
        assert_eq!(recut["version_count"], 2, "{surface:?}");

        // Applying the same cut again writes nothing, and records nothing.
        let again = set_particle(&app, surface, &tree, "LE BRANCH", "")
            .await
            .unwrap();
        assert_eq!(again["names_updated"], 0, "{surface:?}");
        assert_eq!(audit(&app, &tree).await.len(), entries.len(), "{surface:?}");

        // Narrowing a particle that went too far: the usage list still finds
        // the person although detection would cut the name elsewhere.
        let out = set_particle(&app, surface, &tree, "de la Cruz", "de")
            .await
            .unwrap();
        assert_eq!(out["surname_prefix"], "de", "{surface:?}");
        assert_eq!(out["surname"], "la Cruz", "{surface:?}");
        assert_eq!(
            usage(&app, &tree, "de la Cruz").await,
            [cruz],
            "{surface:?}"
        );
    }
}

#[tokio::test]
async fn a_particle_recut_rejects_what_it_cannot_cut_on_both_surfaces() {
    for surface in SURFACES {
        let app = setup().await;
        let tree = create_tree(&app).await;
        create_person(&app, &tree, "Given_a", None, "Thornby").await;

        // Not at the head: accepting it would invent a word.
        assert_eq!(
            set_particle(&app, surface, &tree, "Thornby", "von").await,
            Err(StatusCode::BAD_REQUEST),
            "{surface:?}"
        );
        // Swallowing the whole name.
        assert_eq!(
            set_particle(&app, surface, &tree, "Thornby", "Thornby").await,
            Err(StatusCode::BAD_REQUEST),
            "{surface:?}"
        );
        // No name at all.
        assert_eq!(
            set_particle(&app, surface, &tree, "  ", "").await,
            Err(StatusCode::BAD_REQUEST),
            "{surface:?}"
        );
        let entries = family_names(&app, &tree).await;
        assert_eq!(entry(&entries, "Thornby").unwrap()["sort_key"], "thornby");
    }
}
