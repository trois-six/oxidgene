//! The dictionary's bulk family-name edits over REST and GraphQL.
//!
//! Every scenario runs once per surface through the same helpers, so the two
//! stay strictly symmetric: same inputs, same outcome, same errors. Fixtures
//! are built over REST; only the edit under test changes surface.
//!
//! All data is fictitious.

mod common;

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
        let app = setup_app().await;
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
        let app = setup_app().await;
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

/// `PATCH …/family-names/rename` or `renameFamilyName`, answered in the REST
/// shape; `Err` as for [`set_particle`].
async fn rename(
    app: &axum::Router,
    surface: Surface,
    tree: &str,
    value: &str,
    new_value: &str,
    particle: Option<&str>,
) -> Result<Value, StatusCode> {
    match surface {
        Surface::Rest => {
            let mut body = json!({ "value": value, "new_value": new_value });
            if let Some(particle) = particle {
                body["particle"] = json!(particle);
            }
            let (status, body) = send(
                app,
                Method::PATCH,
                &format!("/api/v1/trees/{tree}/dictionary/family-names/rename"),
                Some(body),
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
            "mutation($t: ID!, $v: String!, $n: String!, $p: String) {
                renameFamilyName(treeId: $t, input: { value: $v, newValue: $n, particle: $p }) {
                    value newValue surnamePrefix surname namesUpdated personsUpdated merged
                }
            }",
            json!({ "t": tree, "v": value, "n": new_value, "p": particle }),
        )
        .await
        .map(|data| {
            let r = &data["renameFamilyName"];
            json!({
                "value": r["value"],
                "new_value": r["newValue"],
                "surname_prefix": r["surnamePrefix"],
                "surname": r["surname"],
                "names_updated": r["namesUpdated"],
                "persons_updated": r["personsUpdated"],
                "merged": r["merged"],
            })
        })
        .map_err(|_| StatusCode::BAD_REQUEST),
    }
}

async fn search_total(app: &axum::Router, tree: &str, filter: &str) -> i64 {
    ok(
        app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/persons/search?{filter}"),
        None,
    )
    .await["total_count"]
        .as_i64()
        .unwrap()
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

async fn link(app: &axum::Router, tree: &str, family: &str, what: &str, body: Value) {
    ok(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/families/{family}/{what}"),
        Some(body),
    )
    .await;
}

#[tokio::test]
async fn a_rename_merges_primary_names_and_refreshes_relatives_on_both_surfaces() {
    for surface in SURFACES {
        let app = setup_app().await;
        let tree = create_tree(&app).await;
        let father = create_person(&app, &tree, "Given_f", None, "Thornby").await;
        let mother = create_person(&app, &tree, "Given_m", None, "Ashcombe").await;
        let child = create_person(&app, &tree, "Given_c", None, "Thornby").await;
        let existing = create_person(&app, &tree, "Given_e", None, "WESTLEY").await;
        // Carries the name only as an alias, and keeps it.
        let alias = create_person(&app, &tree, "Given_a", None, "Ashcombe").await;
        add_name(
            &app,
            &tree,
            &alias,
            json!({
                "name_type": "also_known_as",
                "given_names": "Given_a",
                "surname": "Thornby",
                "is_primary": false,
            }),
        )
        .await;
        let family = ok(
            &app,
            Method::POST,
            &format!("/api/v1/trees/{tree}/families"),
            None,
        )
        .await["id"]
            .as_str()
            .unwrap()
            .to_string();
        link(
            &app,
            &tree,
            &family,
            "spouses",
            json!({ "person_id": father, "role": "husband" }),
        )
        .await;
        link(
            &app,
            &tree,
            &family,
            "spouses",
            json!({ "person_id": mother, "role": "wife" }),
        )
        .await;
        link(
            &app,
            &tree,
            &family,
            "children",
            json!({ "person_id": child, "child_type": "biological" }),
        )
        .await;

        let before = family_names(&app, &tree).await;
        let thornby = entry(&before, "Thornby").unwrap();
        assert_eq!(thornby["count"], 3, "{surface:?}");
        assert_eq!(thornby["primary_count"], 2, "{surface:?}");
        let father_versions = versions(&app, &tree, &father).await;

        let out = rename(&app, surface, &tree, "Thornby", "WESTLEY", None)
            .await
            .unwrap();
        assert_eq!(out["value"], "Thornby", "{surface:?}");
        assert_eq!(out["new_value"], "WESTLEY", "{surface:?}");
        assert_eq!(out["surname_prefix"], Value::Null, "{surface:?}");
        assert_eq!(out["surname"], "WESTLEY", "{surface:?}");
        assert_eq!(out["names_updated"], 2, "{surface:?}");
        assert_eq!(out["persons_updated"], 2, "{surface:?}");
        assert_eq!(out["merged"], true, "{surface:?}");

        // The dictionary shows the union; the alias keeps the old name.
        let after = family_names(&app, &tree).await;
        let westley = entry(&after, "WESTLEY").unwrap();
        assert_eq!(westley["count"], 3, "{surface:?}");
        assert_eq!(westley["primary_count"], 3, "{surface:?}");
        let thornby = entry(&after, "Thornby").unwrap();
        assert_eq!(thornby["count"], 1, "{surface:?}");
        assert_eq!(thornby["primary_count"], 0, "{surface:?}");
        assert_eq!(
            usage(&app, &tree, "Thornby").await,
            std::slice::from_ref(&alias),
            "{surface:?}"
        );
        let mut carriers = vec![father.clone(), child.clone(), existing];
        carriers.sort();
        assert_eq!(usage(&app, &tree, "WESTLEY").await, carriers, "{surface:?}");

        // Search rows: the carriers under the new name, the child under
        // their father's.
        assert_eq!(
            search_total(&app, &tree, "surname=WESTLEY").await,
            3,
            "{surface:?}"
        );
        assert_eq!(
            search_total(&app, &tree, "father_surname=WESTLEY").await,
            1,
            "{surface:?}"
        );
        assert_eq!(
            search_total(&app, &tree, "father_surname=Thornby").await,
            0,
            "{surface:?}"
        );

        // Projections: the person's own, and the relatives' family links.
        assert_eq!(
            profile(&app, &tree, &father).await["primary_name"]["surname"],
            "WESTLEY",
            "{surface:?}"
        );
        assert_eq!(
            profile(&app, &tree, &mother).await["families_as_spouse"][0]["spouse_surname"],
            "WESTLEY",
            "{surface:?}"
        );
        assert_eq!(
            profile(&app, &tree, &child).await["family_as_child"]["father_surname"],
            "WESTLEY",
            "{surface:?}"
        );

        // One bulk entry reading "Thornby → WESTLEY", one version per carrier.
        let entries = audit(&app, &tree).await;
        let renamed = &entries[0];
        assert_eq!(renamed["entity"], "family_name", "{surface:?}");
        assert_eq!(renamed["action"], "update", "{surface:?}");
        assert_eq!(renamed["label"], "Thornby", "{surface:?}");
        assert_eq!(renamed["details"]["count"], 2, "{surface:?}");
        assert_eq!(renamed["details"]["new_label"], "WESTLEY", "{surface:?}");
        assert_eq!(renamed["version_count"], 2, "{surface:?}");
        if let Surface::Graphql = surface {
            let data = graphql(
                &app,
                "query($t: ID!) {
                    auditEntries(treeId: $t, first: 1) {
                        edges { node { entity details { count newLabel } } }
                    }
                }",
                json!({ "t": tree }),
            )
            .await
            .unwrap();
            let node = &data["auditEntries"]["edges"][0]["node"];
            assert_eq!(node["entity"], "FAMILY_NAME");
            assert_eq!(node["details"]["newLabel"], "WESTLEY");
        }

        // Undo is per person: restoring the father leaves the child renamed.
        let history = versions(&app, &tree, &father).await;
        assert_eq!(history.len(), father_versions.len() + 1, "{surface:?}");
        assert_eq!(history[0]["entry"]["entity"], "family_name", "{surface:?}");
        ok(
            &app,
            Method::POST,
            &format!("/api/v1/trees/{tree}/history/person/{father}/revert"),
            Some(json!({ "version": father_versions[0]["version"] })),
        )
        .await;
        assert_eq!(
            profile(&app, &tree, &father).await["primary_name"]["surname"],
            "Thornby",
            "{surface:?}"
        );
        assert_eq!(
            profile(&app, &tree, &child).await["primary_name"]["surname"],
            "WESTLEY",
            "{surface:?}"
        );
    }
}

#[tokio::test]
async fn a_rename_adopts_the_cut_of_the_name_it_joins_on_both_surfaces() {
    for surface in SURFACES {
        let app = setup_app().await;
        let tree = create_tree(&app).await;
        create_person(&app, &tree, "Given_a", None, "Cruz de la").await;
        // Cut by hand where detection would not cut it.
        create_person(&app, &tree, "Given_b", Some("de"), "la Cruz").await;
        let bare = create_person(&app, &tree, "Given_c", None, "Cruz").await;

        let out = rename(&app, surface, &tree, "Cruz de la", "de la Cruz", None)
            .await
            .unwrap();
        assert_eq!(out["merged"], true, "{surface:?}");
        assert_eq!(out["surname_prefix"], "de", "{surface:?}");
        assert_eq!(out["surname"], "la Cruz", "{surface:?}");
        let entries = family_names(&app, &tree).await;
        let merged = entry(&entries, "de la Cruz").unwrap();
        assert_eq!(merged["count"], 2, "{surface:?}");
        assert_eq!(merged["sort_key"], "la cruz", "{surface:?}");
        assert_eq!(usage(&app, &tree, "Cruz").await, [bare], "{surface:?}");
    }
}

#[tokio::test]
async fn a_rename_rejects_what_it_cannot_apply_on_both_surfaces() {
    for surface in SURFACES {
        let app = setup_app().await;
        let tree = create_tree(&app).await;
        create_person(&app, &tree, "Given_a", None, "Thornby").await;
        let entries_before = audit(&app, &tree).await.len();

        for (value, new_value, particle) in [
            ("Thornby", "  ", None),
            ("", "WESTLEY", None),
            ("Thornby", "WESTLEY", Some("VON")),
        ] {
            assert_eq!(
                rename(&app, surface, &tree, value, new_value, particle).await,
                Err(StatusCode::BAD_REQUEST),
                "{surface:?}: {value:?} → {new_value:?} at {particle:?}"
            );
        }
        // Case-exact, and a rename to itself: nothing written, nothing recorded.
        let out = rename(&app, surface, &tree, "thornby", "WESTLEY", None)
            .await
            .unwrap();
        assert_eq!(out["names_updated"], 0, "{surface:?}");
        let out = rename(&app, surface, &tree, "Thornby", "Thornby", None)
            .await
            .unwrap();
        assert_eq!(out["names_updated"], 0, "{surface:?}");
        assert_eq!(out["merged"], false, "{surface:?}");
        assert_eq!(
            audit(&app, &tree).await.len(),
            entries_before,
            "{surface:?}"
        );
        assert_eq!(
            entry(&family_names(&app, &tree).await, "Thornby").unwrap()["count"],
            1
        );
    }
}

/// A person's versions, latest first.
async fn versions(app: &axum::Router, tree: &str, person: &str) -> Vec<Value> {
    ok(
        app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/history/person/{person}?first=100"),
        None,
    )
    .await["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|edge| edge["node"].clone())
        .collect()
}
