//! The rules the schema holds (docs/data-model.md, Integrity constraints),
//! as REST and GraphQL report them and as an import corrects for them.
//!
//! A write that would break one is refused before it reaches the database,
//! with the same code on both surfaces; a file that says so is imported
//! with a warning. All data is fictitious.

use crate::common;

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};

use common::{gql, gql_error_code, gql_ok, new_person, new_tree, ok, send, setup_app};

/// A new record at `path` of `tree`, created from `body`; its id.
async fn create(app: &axum::Router, tree: &str, path: &str, body: Value) -> String {
    ok(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/{path}"),
        Some(body),
    )
    .await["id"]
        .as_str()
        .expect("an id")
        .to_owned()
}

/// `POST body` at `path` of `tree`: the status and the error code.
async fn refused(app: &axum::Router, tree: &str, path: &str, body: Value) -> (StatusCode, String) {
    let (status, answer) = send(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/{path}"),
        Some(body),
    )
    .await;
    (
        status,
        answer["error"].as_str().unwrap_or_default().to_owned(),
    )
}

#[tokio::test]
async fn a_note_a_citation_and_a_media_link_name_exactly_one_record() {
    let app = setup_app().await;
    let tree = new_tree(&app, "Fictional tree").await;
    let person = new_person(&app, &tree).await;
    let event = create(
        &app,
        &tree,
        "events",
        json!({ "event_type": "birth", "person_id": person }),
    )
    .await;
    let source = create(
        &app,
        &tree,
        "sources",
        json!({ "title": "Sample register" }),
    )
    .await;
    let media = gql_ok(
        &app,
        "mutation($t: ID!) { createMediaDocument(treeId: $t, title: \"Sample\") { id } }",
        json!({ "t": tree }),
    )
    .await["createMediaDocument"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let invalid = (StatusCode::BAD_REQUEST, "validation_error".to_owned());
    for (path, none, several) in [
        (
            "notes",
            json!({ "text": "Sample" }),
            json!({ "text": "Sample", "person_id": person, "event_id": event }),
        ),
        (
            "citations",
            json!({ "source_id": source }),
            json!({ "source_id": source, "person_id": person, "event_id": event }),
        ),
        (
            "media-links",
            json!({ "media_id": media }),
            json!({ "media_id": media, "person_id": person, "source_id": source }),
        ),
    ] {
        assert_eq!(refused(&app, &tree, path, none).await, invalid, "{path}");
        assert_eq!(refused(&app, &tree, path, several).await, invalid, "{path}");
    }

    for (mutation, none, several) in [
        (
            "createNote",
            r#"{ text: "Sample" }"#.to_owned(),
            format!(r#"{{ text: "Sample", personId: "{person}", eventId: "{event}" }}"#),
        ),
        (
            "createCitation",
            format!(r#"{{ sourceId: "{source}" }}"#),
            format!(r#"{{ sourceId: "{source}", personId: "{person}", eventId: "{event}" }}"#),
        ),
        (
            "createMediaLink",
            format!(r#"{{ mediaId: "{media}" }}"#),
            format!(r#"{{ mediaId: "{media}", personId: "{person}", sourceId: "{source}" }}"#),
        ),
    ] {
        for input in [none, several] {
            let response = gql(
                &app,
                &format!(r#"mutation {{ {mutation}(treeId: "{tree}", input: {input}) {{ id }} }}"#),
                json!({}),
            )
            .await;
            assert_eq!(gql_error_code(&response), "VALIDATION_ERROR", "{mutation}");
        }
    }

    // One record each: accepted, and nothing of the refused writes stayed.
    create(
        &app,
        &tree,
        "notes",
        json!({ "text": "Sample", "event_id": event }),
    )
    .await;
    create(
        &app,
        &tree,
        "citations",
        json!({ "source_id": source, "event_id": event }),
    )
    .await;
    create(
        &app,
        &tree,
        "media-links",
        json!({ "media_id": media, "person_id": person }),
    )
    .await;
    for path in ["notes", "citations"] {
        let page = ok(
            &app,
            Method::GET,
            &format!("/api/v1/trees/{tree}/{path}"),
            None,
        )
        .await;
        assert_eq!(page["edges"].as_array().map(Vec::len), Some(1), "{path}");
    }
}

#[tokio::test]
async fn a_person_joins_a_family_and_witnesses_an_event_once() {
    let app = setup_app().await;
    let tree = new_tree(&app, "Fictional tree").await;
    let spouse = new_person(&app, &tree).await;
    let child = new_person(&app, &tree).await;
    let witness = new_person(&app, &tree).await;
    let family = create(&app, &tree, "families", json!({})).await;
    let event = create(
        &app,
        &tree,
        "events",
        json!({ "event_type": "baptism", "person_id": child }),
    )
    .await;

    let spouses = format!("families/{family}/spouses");
    let children = format!("families/{family}/children");
    let witnesses = format!("events/{event}/witnesses");
    create(
        &app,
        &tree,
        &spouses,
        json!({ "person_id": spouse, "role": "husband" }),
    )
    .await;
    create(
        &app,
        &tree,
        &children,
        json!({ "person_id": child, "child_type": "biological" }),
    )
    .await;
    create(
        &app,
        &tree,
        &witnesses,
        json!({ "person_id": witness, "relation": "Godfather" }),
    )
    .await;
    // The same person in another relation is a second witness.
    create(
        &app,
        &tree,
        &witnesses,
        json!({ "person_id": witness, "relation": "Witness" }),
    )
    .await;
    create(&app, &tree, &witnesses, json!({ "person_id": witness })).await;

    let conflict = (StatusCode::CONFLICT, "conflict".to_owned());
    for (path, body) in [
        // Whatever the role.
        (&spouses, json!({ "person_id": spouse, "role": "partner" })),
        (
            &children,
            json!({ "person_id": child, "child_type": "adopted" }),
        ),
        (
            &witnesses,
            json!({ "person_id": witness, "relation": "Godfather" }),
        ),
        (&witnesses, json!({ "person_id": witness })),
    ] {
        assert_eq!(refused(&app, &tree, path, body).await, conflict, "{path}");
    }

    for mutation in [
        format!(
            r#"addSpouse(treeId: "{tree}", familyId: "{family}", input: {{ personId: "{spouse}", role: WIFE }}) {{ id }}"#
        ),
        format!(
            r#"addChild(treeId: "{tree}", familyId: "{family}", input: {{ personId: "{child}", childType: FOSTER }}) {{ id }}"#
        ),
        format!(
            r#"addEventWitness(treeId: "{tree}", eventId: "{event}", input: {{ personId: "{witness}", relation: "Witness" }}) {{ id }}"#
        ),
    ] {
        let response = gql(&app, &format!("mutation {{ {mutation} }}"), json!({})).await;
        assert_eq!(gql_error_code(&response), "CONFLICT", "{mutation}");
    }

    let listed = |path: String| {
        let (app, tree) = (app.clone(), tree.clone());
        async move {
            ok(
                &app,
                Method::GET,
                &format!("/api/v1/trees/{tree}/{path}"),
                None,
            )
            .await
            .as_array()
            .map(Vec::len)
        }
    };
    assert_eq!(listed(spouses).await, Some(1));
    assert_eq!(listed(children).await, Some(1));
    assert_eq!(listed(witnesses).await, Some(3));
}

/// The names of `person`, as `(given names, is_primary)` in id order.
async fn names(app: &axum::Router, tree: &str, person: &str) -> Vec<(String, bool)> {
    let mut names: Vec<(String, String, bool)> = ok(
        app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/persons/{person}/names"),
        None,
    )
    .await
    .as_array()
    .expect("names")
    .iter()
    .map(|name| {
        (
            name["id"].as_str().unwrap().to_owned(),
            name["given_names"].as_str().unwrap().to_owned(),
            name["is_primary"].as_bool().unwrap(),
        )
    })
    .collect();
    names.sort();
    names
        .into_iter()
        .map(|(_, given, primary)| (given, primary))
        .collect()
}

#[tokio::test]
async fn a_new_primary_name_takes_the_place_of_the_previous_one() {
    let app = setup_app().await;
    let tree = new_tree(&app, "Fictional tree").await;
    let person = new_person(&app, &tree).await;
    let path = format!("persons/{person}/names");
    let name = |given: &str| {
        json!({
            "name_type": "birth",
            "given_names": given,
            "surname": "Sample",
            "is_primary": true,
        })
    };

    let first = create(&app, &tree, &path, name("First")).await;
    create(&app, &tree, &path, name("Second")).await;
    assert_eq!(
        names(&app, &tree, &person).await,
        [("First".to_owned(), false), ("Second".to_owned(), true)]
    );

    // GraphQL, creating.
    gql_ok(
        &app,
        &format!(
            r#"mutation {{ addPersonName(treeId: "{tree}", personId: "{person}", input: {{ nameType: BIRTH, givenNames: "Third", surname: "Sample", isPrimary: true }}) {{ id }} }}"#
        ),
        json!({}),
    )
    .await;
    assert_eq!(
        names(&app, &tree, &person).await,
        [
            ("First".to_owned(), false),
            ("Second".to_owned(), false),
            ("Third".to_owned(), true)
        ]
    );

    // Updating, on both: the name made primary is the only one.
    ok(
        &app,
        Method::PUT,
        &format!("/api/v1/trees/{tree}/persons/{person}/names/{first}"),
        Some(json!({ "is_primary": true })),
    )
    .await;
    assert_eq!(
        names(&app, &tree, &person).await,
        [
            ("First".to_owned(), true),
            ("Second".to_owned(), false),
            ("Third".to_owned(), false)
        ]
    );
    let second = ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/{path}"),
        None,
    )
    .await
    .as_array()
    .unwrap()
    .iter()
    .find(|name| name["given_names"] == "Second")
    .map(|name| name["id"].as_str().unwrap().to_owned())
    .unwrap();
    gql_ok(
        &app,
        &format!(
            r#"mutation {{ updatePersonName(treeId: "{tree}", personId: "{person}", id: "{second}", input: {{ isPrimary: true }}) {{ id }} }}"#
        ),
        json!({}),
    )
    .await;
    assert_eq!(
        names(&app, &tree, &person).await,
        [
            ("First".to_owned(), false),
            ("Second".to_owned(), true),
            ("Third".to_owned(), false)
        ]
    );
}

/// A file that names one person twice as a child of a family, as both
/// spouses of another, and twice as a witness in one relation: the links are
/// kept once and the import says so, rather than failing on the repeated
/// row.
#[tokio::test]
async fn an_import_repeating_links_completes_with_warnings() {
    const GEDCOM: &str = "\
0 HEAD
1 GEDC
2 VERS 7.0
0 @I1@ INDI
1 NAME Arthur /Ashdown/
1 SEX M
1 FAMS @F1@
1 FAMS @F2@
0 @I2@ INDI
1 NAME Alice /Birchley/
1 SEX F
1 FAMS @F1@
0 @I3@ INDI
1 NAME Cyril /Ashdown/
1 SEX M
1 FAMC @F1@
1 BAPM
2 DATE 4 MAR 1851
2 ASSO @I1@
3 ROLE GODP
2 ASSO @I1@
3 ROLE GODP
0 @F1@ FAM
1 HUSB @I1@
1 WIFE @I2@
1 CHIL @I3@
1 CHIL @I3@
0 @F2@ FAM
1 HUSB @I1@
1 WIFE @I1@
0 TRLR
";
    let db = common::setup_db().await;
    let app = common::app_on(db.clone());
    let tree = new_tree(&app, "Fictional tree").await;

    let summary = common::import_gedcom(&app, &db, &tree, GEDCOM).await;

    assert_eq!(summary["persons_count"], 3, "{summary}");
    assert_eq!(summary["families_count"], 2, "{summary}");
    let families = ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/families"),
        None,
    )
    .await;
    let mut links = [0, 0];
    for family in families["edges"].as_array().expect("families") {
        let family = family["node"]["id"].as_str().unwrap();
        for (count, path) in links.iter_mut().zip(["spouses", "children"]) {
            *count += ok(
                &app,
                Method::GET,
                &format!("/api/v1/trees/{tree}/families/{family}/{path}"),
                None,
            )
            .await
            .as_array()
            .map_or(0, Vec::len);
        }
    }
    assert_eq!(links, [3, 1], "two spouses and one, and one child");
    let warnings: Vec<&str> = summary["warnings"]
        .as_array()
        .expect("warnings")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    for rule in ["spouse link(s) repeated", "child link(s) repeated"] {
        assert!(
            warnings.iter().any(|warning| warning.contains(rule)),
            "no warning for `{rule}`: {warnings:?}"
        );
    }
}
