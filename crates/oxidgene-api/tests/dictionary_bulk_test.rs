//! The dictionary's bulk edits of occupations, places and repositories over
//! REST and GraphQL.
//!
//! Every scenario runs once per surface through the same helpers, so the two
//! stay strictly symmetric: same inputs, same outcome, same errors. Fixtures
//! are built over REST; only the edit under test changes surface.
//!
//! All data is fictitious.

use crate::common;

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};

use common::{ok, send, setup_app};

#[derive(Debug, Clone, Copy)]
enum Surface {
    Rest,
    Graphql,
}

const SURFACES: [Surface; 2] = [Surface::Rest, Surface::Graphql];

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

/// A REST call: its body, or its status when it failed.
async fn rest(
    app: &axum::Router,
    method: Method,
    uri: &str,
    body: Value,
) -> Result<Value, StatusCode> {
    let (status, body) = send(app, method, uri, Some(body)).await;
    if status.is_success() {
        Ok(body)
    } else {
        Err(status)
    }
}

async fn create_tree(app: &axum::Router) -> String {
    ok(
        app,
        Method::POST,
        "/api/v1/trees",
        Some(json!({ "name": "Bulk Edit Fixture" })),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string()
}

async fn create_person(app: &axum::Router, tree: &str, given: &str) -> String {
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
    ok(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/persons/{id}/names"),
        Some(json!({
            "name_type": "birth",
            "given_names": given,
            "surname": "Fictive",
            "is_primary": true,
        })),
    )
    .await;
    id
}

async fn create_event(app: &axum::Router, tree: &str, body: Value) -> String {
    ok(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/events"),
        Some(body),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string()
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

async fn profile(app: &axum::Router, tree: &str, person: &str) -> Value {
    ok(
        app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/profiles/{person}"),
        None,
    )
    .await
}

// ── Occupations ─────────────────────────────────────────────────────────

async fn occupations(app: &axum::Router, tree: &str) -> Vec<(String, i64)> {
    ok(
        app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/dictionary/occupations"),
        None,
    )
    .await
    .as_array()
    .unwrap()
    .iter()
    .map(|e| {
        (
            e["value"].as_str().unwrap().to_string(),
            e["count"].as_i64().unwrap(),
        )
    })
    .collect()
}

/// `PATCH …/occupations/rename` or `renameOccupation`, answered in the REST
/// shape. `Err` carries the status (400 for GraphQL's validation errors).
async fn rename_occupation(
    app: &axum::Router,
    surface: Surface,
    tree: &str,
    value: &str,
    new_value: &str,
) -> Result<Value, StatusCode> {
    match surface {
        Surface::Rest => {
            rest(
                app,
                Method::PATCH,
                &format!("/api/v1/trees/{tree}/dictionary/occupations/rename"),
                json!({ "value": value, "new_value": new_value }),
            )
            .await
        }
        Surface::Graphql => graphql(
            app,
            "mutation($t: ID!, $v: String!, $n: String!) {
                renameOccupation(treeId: $t, input: { value: $v, newValue: $n }) {
                    value newValue eventsUpdated personsUpdated merged
                }
            }",
            json!({ "t": tree, "v": value, "n": new_value }),
        )
        .await
        .map(|data| {
            let r = &data["renameOccupation"];
            json!({
                "value": r["value"],
                "new_value": r["newValue"],
                "events_updated": r["eventsUpdated"],
                "persons_updated": r["personsUpdated"],
                "merged": r["merged"],
            })
        })
        .map_err(|_| StatusCode::BAD_REQUEST),
    }
}

#[tokio::test]
async fn an_occupation_rename_merges_and_refreshes_its_holders_on_both_surfaces() {
    for surface in SURFACES {
        let app = setup_app().await;
        let tree = create_tree(&app).await;
        let alpha = create_person(&app, &tree, "Given_a").await;
        let beta = create_person(&app, &tree, "Given_b").await;
        let gamma = create_person(&app, &tree, "Given_c").await;
        for (person, label) in [
            (&alpha, "Weaver"),
            // Stored with stray spaces: listed, and renamed, as "Weaver".
            (&beta, " Weaver "),
            (&gamma, "Hand weaver"),
        ] {
            create_event(
                &app,
                &tree,
                json!({ "event_type": "occupation", "person_id": person, "description": label }),
            )
            .await;
        }
        // Another event type with the same text is no occupation.
        create_event(
            &app,
            &tree,
            json!({ "event_type": "residence", "person_id": alpha, "description": "Weaver" }),
        )
        .await;

        let out = rename_occupation(&app, surface, &tree, "Weaver", "Hand weaver")
            .await
            .unwrap();
        assert_eq!(out["events_updated"], 2, "{surface:?}: {out}");
        assert_eq!(out["persons_updated"], 2, "{surface:?}");
        assert_eq!(out["merged"], true, "{surface:?}");
        assert_eq!(
            occupations(&app, &tree).await,
            [("Hand weaver".to_string(), 3)],
            "{surface:?}"
        );
        // The holder's profile shows the new label at once.
        let shown = profile(&app, &tree, &alpha).await.to_string();
        assert!(shown.contains("Hand weaver"), "{surface:?}: {shown}");

        let entry = &audit(&app, &tree).await[0];
        assert_eq!(entry["entity"], "occupation", "{surface:?}: {entry}");
        assert_eq!(entry["label"], "Weaver", "{surface:?}");
        assert_eq!(entry["details"]["count"], 2, "{surface:?}");
        assert_eq!(entry["details"]["new_label"], "Hand weaver", "{surface:?}");

        // A label nobody holds: nothing written, nothing recorded.
        let before = audit(&app, &tree).await.len();
        let out = rename_occupation(&app, surface, &tree, "Weaver", "Spinner")
            .await
            .unwrap();
        assert_eq!(out["events_updated"], 0, "{surface:?}");
        assert_eq!(audit(&app, &tree).await.len(), before, "{surface:?}");
    }
}

#[tokio::test]
async fn an_occupation_rename_rejects_a_blank_label_on_both_surfaces() {
    for surface in SURFACES {
        let app = setup_app().await;
        let tree = create_tree(&app).await;
        for (value, new_value) in [("Weaver", "  "), ("", "Spinner")] {
            assert_eq!(
                rename_occupation(&app, surface, &tree, value, new_value).await,
                Err(StatusCode::BAD_REQUEST),
                "{surface:?}: {value:?} → {new_value:?}"
            );
        }
    }
}

// ── Places and repositories ─────────────────────────────────────────────

async fn create(app: &axum::Router, tree: &str, what: &str, body: Value) -> String {
    ok(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/{what}"),
        Some(body),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string()
}

async fn tag(app: &axum::Router, tree: &str, label: &str, kind: &str, record: &str) -> String {
    let tag = create(app, tree, "tags", json!({ "label": label })).await;
    let (status, body) = send(
        app,
        Method::PUT,
        &format!("/api/v1/trees/{tree}/tags/{tag}/records/{kind}/{record}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    tag
}

fn labels(record: &Value) -> Vec<String> {
    record["tags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tag| tag["label"].as_str().unwrap().to_string())
        .collect()
}

/// `POST …/{what}/{id}/merge` or `merge{Kind}`, answered in the REST shape.
async fn merge(
    app: &axum::Router,
    surface: Surface,
    tree: &str,
    kind: &str,
    id: &str,
    input: Value,
) -> Result<Value, StatusCode> {
    let (path, mutation, input_type, fields) = match kind {
        "place" => (
            "places",
            "mergePlace",
            "MergePlaceInput",
            "id name latitude longitude",
        ),
        _ => (
            "repositories",
            "mergeRepository",
            "MergeRepositoryInput",
            "id name address phone email website",
        ),
    };
    match surface {
        Surface::Rest => {
            rest(
                app,
                Method::POST,
                &format!("/api/v1/trees/{tree}/{path}/{id}/merge"),
                input,
            )
            .await
        }
        Surface::Graphql => {
            // The REST body in GraphQL's camel case: every field is one word.
            let query = format!(
                "mutation($t: ID!, $id: ID!, $input: {input_type}!) {{
                    {mutation}(treeId: $t, id: $id, input: $input) {{ {fields} }}
                }}"
            );
            graphql(app, &query, json!({ "t": tree, "id": id, "input": input }))
                .await
                .map(|data| data[mutation].clone())
                .map_err(|errors| {
                    let not_found = errors.to_string().contains("NOT_FOUND");
                    if not_found {
                        StatusCode::NOT_FOUND
                    } else {
                        StatusCode::BAD_REQUEST
                    }
                })
        }
    }
}

#[tokio::test]
async fn a_place_merge_moves_events_media_and_tags_on_both_surfaces() {
    for surface in SURFACES {
        let app = setup_app().await;
        let tree = create_tree(&app).await;
        let kept = create(
            &app,
            &tree,
            "places",
            json!({ "name": "Northfield", "latitude": 47.1, "longitude": -1.5 }),
        )
        .await;
        let absorbed = create(
            &app,
            &tree,
            "places",
            json!({ "name": "NORTHFIELD, Region Alpha" }),
        )
        .await;
        let alpha = create_person(&app, &tree, "Given_a").await;
        let beta = create_person(&app, &tree, "Given_b").await;
        create_event(
            &app,
            &tree,
            json!({ "event_type": "birth", "person_id": alpha, "place_id": kept }),
        )
        .await;
        let death = create_event(
            &app,
            &tree,
            json!({ "event_type": "death", "person_id": beta, "place_id": absorbed }),
        )
        .await;
        let scan = create(
            &app,
            &tree,
            "media/document",
            json!({ "title": "Fictitious scan" }),
        )
        .await;
        ok(
            &app,
            Method::PUT,
            &format!("/api/v1/trees/{tree}/media/{scan}"),
            Some(json!({ "place_id": absorbed })),
        )
        .await;
        tag(&app, &tree, "Kept tag", "place", &kept).await;
        tag(&app, &tree, "Absorbed tag", "place", &absorbed).await;

        // The name of one, the coordinates of the other.
        let place = merge(
            &app,
            surface,
            &tree,
            "place",
            &absorbed,
            json!({ "into": kept, "name": "Northfield, Region Alpha", "latitude": 47.1, "longitude": -1.5 }),
        )
        .await
        .unwrap();
        assert_eq!(place["id"], kept.as_str(), "{surface:?}");
        assert_eq!(place["name"], "Northfield, Region Alpha", "{surface:?}");
        assert_eq!(place["latitude"], 47.1, "{surface:?}");

        let places = ok(
            &app,
            Method::GET,
            &format!("/api/v1/trees/{tree}/places"),
            None,
        )
        .await;
        assert_eq!(places["total_count"], 1, "{surface:?}: {places}");
        let event = ok(
            &app,
            Method::GET,
            &format!("/api/v1/trees/{tree}/events/{death}"),
            None,
        )
        .await;
        assert_eq!(event["place_id"], kept.as_str(), "{surface:?}");
        let media = ok(
            &app,
            Method::GET,
            &format!("/api/v1/trees/{tree}/media/{scan}"),
            None,
        )
        .await;
        assert_eq!(media["place_id"], kept.as_str(), "{surface:?}");
        let place = ok(
            &app,
            Method::GET,
            &format!("/api/v1/trees/{tree}/places/{kept}"),
            None,
        )
        .await;
        assert_eq!(labels(&place), ["Absorbed tag", "Kept tag"], "{surface:?}");
        // Both persons' profiles show the kept place under its new name.
        for person in [&alpha, &beta] {
            let shown = profile(&app, &tree, person).await.to_string();
            assert!(
                shown.contains("Northfield, Region Alpha"),
                "{surface:?}: {shown}"
            );
        }

        let entry = &audit(&app, &tree).await[0];
        assert_eq!(entry["action"], "merge", "{surface:?}: {entry}");
        assert_eq!(entry["entity"], "place", "{surface:?}");
        assert_eq!(
            entry["details"]["other_label"], "NORTHFIELD, Region Alpha",
            "{surface:?}"
        );
    }
}

#[tokio::test]
async fn a_place_merge_rejects_what_it_cannot_apply_on_both_surfaces() {
    for surface in SURFACES {
        let app = setup_app().await;
        let tree = create_tree(&app).await;
        let kept = create(&app, &tree, "places", json!({ "name": "Westbrook" })).await;
        let absorbed = create(&app, &tree, "places", json!({ "name": "Westbrook" })).await;
        let other_tree = create_tree(&app).await;
        let elsewhere = create(&app, &other_tree, "places", json!({ "name": "Westbrook" })).await;
        for (into, name, status) in [
            (&kept, "  ", StatusCode::BAD_REQUEST),
            (&absorbed, "Westbrook", StatusCode::BAD_REQUEST),
            (&elsewhere, "Westbrook", StatusCode::NOT_FOUND),
        ] {
            assert_eq!(
                merge(
                    &app,
                    surface,
                    &tree,
                    "place",
                    &absorbed,
                    json!({ "into": into, "name": name })
                )
                .await,
                Err(status),
                "{surface:?}: into {into} as {name:?}"
            );
        }
        let places = ok(
            &app,
            Method::GET,
            &format!("/api/v1/trees/{tree}/places"),
            None,
        )
        .await;
        assert_eq!(places["total_count"], 2, "{surface:?}: nothing merged");
    }
}

#[tokio::test]
async fn a_repository_merge_moves_sources_notes_and_tags_on_both_surfaces() {
    for surface in SURFACES {
        let app = setup_app().await;
        let tree = create_tree(&app).await;
        let kept = create(
            &app,
            &tree,
            "repositories",
            json!({ "name": "City Archives", "address": "1 Fictional Street" }),
        )
        .await;
        let absorbed = create(
            &app,
            &tree,
            "repositories",
            json!({ "name": "City archives", "phone": "000 000" }),
        )
        .await;
        let register = create(
            &app,
            &tree,
            "sources",
            json!({ "title": "Fictitious register" }),
        )
        .await;
        let ledger = create(
            &app,
            &tree,
            "sources",
            json!({ "title": "Fictitious ledger" }),
        )
        .await;
        for (source, repository, call_number) in [
            (&register, &absorbed, "A 1"),
            // Held at both under the same call number: one link remains.
            (&ledger, &kept, "B 2"),
            (&ledger, &absorbed, "B 2"),
        ] {
            ok(
                &app,
                Method::POST,
                &format!("/api/v1/trees/{tree}/sources/{source}/repositories"),
                Some(json!({ "repository_id": repository, "call_number": call_number })),
            )
            .await;
        }
        create(
            &app,
            &tree,
            "notes",
            json!({ "text": "Fictitious opening hours", "repository_id": absorbed }),
        )
        .await;
        tag(&app, &tree, "Absorbed tag", "repository", &absorbed).await;

        let repository = merge(
            &app,
            surface,
            &tree,
            "repository",
            &absorbed,
            json!({
                "into": kept,
                "name": "City Archives",
                "address": "1 Fictional Street",
                "phone": "000 000",
            }),
        )
        .await
        .unwrap();
        assert_eq!(repository["id"], kept.as_str(), "{surface:?}");
        assert_eq!(repository["phone"], "000 000", "{surface:?}");
        assert_eq!(repository["address"], "1 Fictional Street", "{surface:?}");

        let listed = ok(
            &app,
            Method::GET,
            &format!("/api/v1/trees/{tree}/repositories"),
            None,
        )
        .await;
        assert_eq!(listed["total_count"], 1, "{surface:?}: {listed}");
        let held = ok(
            &app,
            Method::GET,
            &format!("/api/v1/trees/{tree}/repositories/{kept}/sources"),
            None,
        )
        .await;
        let mut held: Vec<(String, String)> = held
            .as_array()
            .unwrap()
            .iter()
            .map(|link| {
                (
                    link["source"]["title"].as_str().unwrap().to_string(),
                    link["call_number"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        held.sort();
        assert_eq!(
            held,
            [
                ("Fictitious ledger".to_string(), "B 2".to_string()),
                ("Fictitious register".to_string(), "A 1".to_string()),
            ],
            "{surface:?}"
        );
        let notes = ok(
            &app,
            Method::GET,
            &format!("/api/v1/trees/{tree}/notes?repository_id={kept}"),
            None,
        )
        .await;
        assert_eq!(
            notes["edges"][0]["node"]["text"], "Fictitious opening hours",
            "{surface:?}: {notes}"
        );
        let repository = ok(
            &app,
            Method::GET,
            &format!("/api/v1/trees/{tree}/repositories/{kept}"),
            None,
        )
        .await;
        assert_eq!(labels(&repository), ["Absorbed tag"], "{surface:?}");

        let entry = &audit(&app, &tree).await[0];
        assert_eq!(entry["action"], "merge", "{surface:?}: {entry}");
        assert_eq!(entry["entity"], "repository", "{surface:?}");
        assert_eq!(
            entry["details"]["other_label"], "City archives",
            "{surface:?}"
        );
    }
}

#[tokio::test]
async fn a_repository_merge_rejects_what_it_cannot_apply_on_both_surfaces() {
    for surface in SURFACES {
        let app = setup_app().await;
        let tree = create_tree(&app).await;
        let kept = create(
            &app,
            &tree,
            "repositories",
            json!({ "name": "Parish office" }),
        )
        .await;
        let absorbed = create(
            &app,
            &tree,
            "repositories",
            json!({ "name": "Parish office" }),
        )
        .await;
        for (into, name, status) in [
            (&kept, "  ", StatusCode::BAD_REQUEST),
            (&absorbed, "Parish office", StatusCode::BAD_REQUEST),
        ] {
            assert_eq!(
                merge(
                    &app,
                    surface,
                    &tree,
                    "repository",
                    &absorbed,
                    json!({ "into": into, "name": name })
                )
                .await,
                Err(status),
                "{surface:?}: into {into} as {name:?}"
            );
        }
    }
}
