//! Deletion of the records inside a tree (docs/data-model.md, Deletion): a
//! record goes for real, with every row that is its alone, in the
//! transaction of its audit entry, and nothing is left naming it but its
//! history, which keeps its last state and re-creates it on a restore.
//!
//! The rows naming a record are found from the schema itself — every column
//! of every table, on SQLite and PostgreSQL alike — so a table added later is
//! covered without touching this file.

use axum::Router;
use axum::http::Method;
use oxidgene_db::sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, Value};
use serde_json::{Value as Json, json};
use uuid::Uuid;

use crate::common::populated::{Populated, populated_tree};
use crate::common::{app_on, gql_ok, new_person, new_tree, ok, setup_db};

/// The columns that may still name a deleted record: the change history's,
/// which has to outlive what it records, and the root of a sharing
/// exclusion, kept by id until a restore brings the root back.
const KEPT_REFERENCES: &[(&str, &str)] = &[
    ("audit_entry", "entity_id"),
    ("audit_entry", "subject_id"),
    ("record_version", "record_id"),
    ("privacy_exclusion", "person_id"),
];

/// How a column can hold a record's id, and so how to look it up.
#[derive(Clone, Copy)]
enum Holds {
    /// SQLite: as a UUID or as its text, either way.
    Either,
    /// PostgreSQL: a `uuid` column.
    Uuid,
    /// PostgreSQL: a text column — the search table's ids.
    Text,
}

async fn query(
    db: &DatabaseConnection,
    sql: &str,
    values: Vec<Value>,
) -> Vec<oxidgene_db::sea_orm::QueryResult> {
    db.query_all_raw(Statement::from_sql_and_values(
        db.get_database_backend(),
        sql,
        values,
    ))
    .await
    .unwrap_or_else(|error| panic!("{sql}: {error}"))
}

/// Every column of every table that can hold an id.
async fn id_columns(db: &DatabaseConnection) -> Vec<(String, String, Holds)> {
    let mut columns = Vec::new();
    if db.get_database_backend() == DbBackend::Sqlite {
        let tables = query(
            db,
            "SELECT name FROM sqlite_master WHERE type = 'table' \
             AND name NOT LIKE 'sqlite_%' AND name <> 'seaql_migrations'",
            vec![],
        )
        .await;
        for table in tables {
            let table: String = table.try_get("", "name").unwrap();
            for column in query(
                db,
                "SELECT name FROM pragma_table_info(?)",
                vec![table.clone().into()],
            )
            .await
            {
                let column: String = column.try_get("", "name").unwrap();
                columns.push((table.clone(), column, Holds::Either));
            }
        }
    } else {
        for row in query(
            db,
            "SELECT table_name::text AS t, column_name::text AS c, data_type::text AS d \
             FROM information_schema.columns WHERE table_schema = current_schema()",
            vec![],
        )
        .await
        {
            let table: String = row.try_get("", "t").unwrap();
            let column: String = row.try_get("", "c").unwrap();
            let kind: String = row.try_get("", "d").unwrap();
            let holds = match kind.as_str() {
                "uuid" => Holds::Uuid,
                "text" | "character varying" => Holds::Text,
                _ => continue,
            };
            columns.push((table, column, holds));
        }
    }
    columns
}

/// Every `table.column` holding `id`, less the [`KEPT_REFERENCES`], with how
/// many rows.
async fn rows_naming(db: &DatabaseConnection, id: Uuid) -> Vec<String> {
    let mut found = Vec::new();
    for (table, column, holds) in id_columns(db).await {
        if KEPT_REFERENCES.contains(&(table.as_str(), column.as_str())) {
            continue;
        }
        let (condition, values) = match holds {
            Holds::Either => ("IN (?, ?)", vec![id.into(), id.to_string().into()]),
            Holds::Uuid => ("= $1", vec![id.into()]),
            Holds::Text => ("= $1", vec![id.to_string().into()]),
        };
        let sql = format!("SELECT COUNT(*) AS n FROM \"{table}\" WHERE \"{column}\" {condition}");
        let n: i64 = query(db, &sql, values).await[0].try_get("", "n").unwrap();
        if n > 0 {
            found.push(format!("{table}.{column}: {n} rows"));
        }
    }
    found
}

/// Fails when a row names `id` outside the history, and on SQLite when any
/// foreign key names a row that is gone — what the desktop's integrity check
/// treats as damage.
async fn assert_gone(db: &DatabaseConnection, what: &str, id: &str) {
    let id: Uuid = id.parse().unwrap();
    let left = rows_naming(db, id).await;
    assert!(
        left.is_empty(),
        "{what} {id} is still named:\n{}",
        left.join("\n")
    );
    if db.get_database_backend() == DbBackend::Sqlite {
        let dangling = query(db, "PRAGMA foreign_key_check", vec![]).await;
        assert!(
            dangling.is_empty(),
            "{} dangling references",
            dangling.len()
        );
    }
}

async fn scalar(db: &DatabaseConnection, sql: &str, id: &str) -> i64 {
    let id: Uuid = id.parse().unwrap();
    query(db, sql, vec![id.into()]).await[0]
        .try_get("", "n")
        .unwrap()
}

/// The ids of the rows of `table` whose `column` is `id`.
async fn ids_where(db: &DatabaseConnection, table: &str, column: &str, id: &str) -> Vec<String> {
    let id: Uuid = id.parse().unwrap();
    let sql = format!("SELECT id FROM {table} WHERE {column} = $1");
    query(db, &sql, vec![id.into()])
        .await
        .iter()
        .map(|row| row.try_get::<Uuid>("", "id").unwrap().to_string())
        .collect()
}

/// A record's versions, latest first.
async fn versions(app: &Router, tree: &str, record_type: &str, id: &str) -> Vec<Json> {
    let page = ok(
        app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/history/{record_type}/{id}?first=100"),
        None,
    )
    .await;
    page["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|edge| edge["node"].clone())
        .collect()
}

/// Where a deletion is asked.
#[derive(Clone, Copy, Debug)]
enum Surface {
    Rest,
    Graphql,
}

/// Delete record `id`, of the REST collection `collection`, through
/// `surface`.
async fn delete(app: &Router, surface: Surface, tree: &str, collection: &str, id: &str) {
    match surface {
        Surface::Rest => {
            ok(
                app,
                Method::DELETE,
                &format!("/api/v1/trees/{tree}/{collection}/{id}"),
                None,
            )
            .await;
        }
        Surface::Graphql => {
            let mutation = match collection {
                "persons" => "deletePerson",
                "families" => "deleteFamily",
                "events" => "deleteEvent",
                "sources" => "deleteSource",
                "repositories" => "deleteRepository",
                "notes" => "deleteNote",
                other => panic!("no mutation for {other}"),
            };
            let data = gql_ok(
                app,
                &format!("mutation($t: ID!, $i: ID!) {{ {mutation}(treeId: $t, id: $i) }}"),
                json!({ "t": tree, "i": id }),
            )
            .await;
            assert_eq!(data[mutation], true, "{mutation}: {data}");
        }
    }
}

/// Every kind of record deleted on both surfaces, each leaving no row behind
/// but its history: a note, a repository holding a source, the source and
/// the citation of it, an event with a place and a witness, a family, and
/// the person at the centre of it all, with names, events, notes, citations,
/// media links, an identification box, a tag and the SOSA root.
#[tokio::test]
async fn every_record_kind_deletes_on_both_surfaces_and_leaves_nothing() {
    for surface in [Surface::Rest, Surface::Graphql] {
        let db = setup_db().await;
        let app = app_on(db.clone());
        let tree = populated_tree(&app, &db, &format!("Deleted {surface:?}"), 1).await;
        let t = &tree.tree_id;
        let id = |name: &str| tree.id(name).to_owned();

        delete(&app, surface, t, "notes", &id("note_id")).await;
        assert_gone(&db, "note", &id("note_id")).await;

        delete(&app, surface, t, "repositories", &id("repository_id")).await;
        assert_gone(&db, "repository", &id("repository_id")).await;
        assert_gone(&db, "repository link", &id("source_link_id")).await;

        delete(&app, surface, t, "sources", &id("source_id")).await;
        assert_gone(&db, "source", &id("source_id")).await;
        assert_gone(&db, "citation", &id("citation_id")).await;

        delete(&app, surface, t, "events", &id("event_id")).await;
        assert_gone(&db, "event", &id("event_id")).await;
        assert_gone(&db, "witness", &id("witness_id")).await;
        // The place it was at stays.
        ok(
            &app,
            Method::GET,
            &format!("/api/v1/trees/{t}/places/{}", id("place_id")),
            None,
        )
        .await;

        delete_person_and_check(&app, &db, surface, &tree).await;
    }
}

/// The family, then its spouse, the anchor of the populated tree.
async fn delete_person_and_check(
    app: &Router,
    db: &DatabaseConnection,
    surface: Surface,
    tree: &Populated,
) {
    let t = &tree.tree_id;
    let person = tree.id("person_id");
    let names = ids_where(db, "person_name", "person_id", person).await;
    let events = ids_where(db, "event", "person_id", person).await;
    assert!(!names.is_empty(), "the fixture's anchor");
    delete_family_and_check(app, db, surface, tree).await;

    delete(app, surface, t, "persons", person).await;
    assert_gone(db, "person", person).await;
    for row in names.iter().chain(&events) {
        assert_gone(db, "name or own event", row).await;
    }
    assert_gone(db, "media link", tree.id("media_link_id")).await;
    check_what_stays(app, db, tree).await;
    check_last_state(app, t, person).await;
}

/// The anchor's family, with its events; its members stay.
async fn delete_family_and_check(
    app: &Router,
    db: &DatabaseConnection,
    surface: Surface,
    tree: &Populated,
) {
    let t = &tree.tree_id;
    let family = tree.id("family_id");
    let family_events = ids_where(db, "event", "family_id", family).await;
    delete(app, surface, t, "families", family).await;
    assert_gone(db, "family", family).await;
    for event in &family_events {
        assert_gone(db, "family event", event).await;
    }
    let spouse = tree.id("spouse_id");
    ok(
        app,
        Method::GET,
        &format!("/api/v1/trees/{t}/persons/{spouse}"),
        None,
    )
    .await;
}

/// What a person's deletion leaves: the identification box on its page,
/// unattributed, the document in the library, the family they were a child
/// of; the tree's SOSA root cleared, its former value versioned with the
/// tree's settings.
async fn check_what_stays(app: &Router, db: &DatabaseConnection, tree: &Populated) {
    let t = &tree.tree_id;
    let unattributed = scalar(
        db,
        "SELECT COUNT(*) AS n FROM vignette WHERE id = $1 AND person_id IS NULL",
        tree.id("vignette_id"),
    )
    .await;
    assert_eq!(unattributed, 1, "the identification box");
    let media = tree.id("media_id");
    ok(
        app,
        Method::GET,
        &format!("/api/v1/trees/{t}/media/{media}"),
        None,
    )
    .await;
    let families = scalar(
        db,
        "SELECT COUNT(*) AS n FROM family WHERE id = $1",
        tree.id("child_family_id"),
    )
    .await;
    assert_eq!(families, 1, "the family they were a child of");
    let settings = ok(app, Method::GET, &format!("/api/v1/trees/{t}"), None).await;
    assert!(settings["sosa_root_person_id"].is_null(), "{settings}");
    let tree_versions = versions(app, t, "tree", t).await;
    assert_eq!(
        tree_versions[1]["snapshot"]["sosa_root_person_id"],
        tree.id("person_id")
    );
}

/// The history alone holds a deleted person's last state.
async fn check_last_state(app: &Router, tree: &str, person: &str) {
    let history = versions(app, tree, "person", person).await;
    assert_eq!(history[0]["current"], true);
    assert_eq!(history[0]["deleted"], true);
    assert!(history[0]["snapshot"].is_null());
    assert_eq!(history[0]["entry"]["action"], "delete");
    assert_eq!(history[1]["deleted"], false);
    assert_eq!(history[1]["snapshot"]["names"][0]["given_names"], "Anchor");
}

/// A person deleted from a family they were alone in leaves it, empty: a
/// family stays until it is deleted itself.
#[tokio::test]
async fn a_family_left_with_nobody_stays() {
    let db = setup_db().await;
    let app = app_on(db.clone());
    let tree = new_tree(&app, "Lonely").await;
    let person = new_person(&app, &tree).await;
    let family = ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/families"),
        None,
    )
    .await;
    let family = family["id"].as_str().unwrap().to_owned();
    ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/families/{family}/spouses"),
        Some(json!({ "person_id": person, "role": "husband" })),
    )
    .await;

    ok(
        &app,
        Method::DELETE,
        &format!("/api/v1/trees/{tree}/persons/{person}"),
        None,
    )
    .await;
    assert_gone(&db, "person", &person).await;
    ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/families/{family}"),
        None,
    )
    .await;
    let spouses = ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/families/{family}/spouses"),
        None,
    )
    .await;
    assert_eq!(spouses, json!([]));

    // A restore links them back to it.
    ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/history/person/{person}/revert"),
        Some(json!({ "version": 2 })),
    )
    .await;
    let spouses = ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/families/{family}/spouses"),
        None,
    )
    .await;
    assert_eq!(spouses[0]["person_id"], person);
}

/// A deleted source and a deleted repository come back from their history,
/// on both surfaces, with their notes and the link between them.
#[tokio::test]
async fn a_deleted_source_and_repository_are_restored_from_their_history() {
    let db = setup_db().await;
    let app = app_on(db.clone());
    let tree = new_tree(&app, "Archives").await;
    let base = format!("/api/v1/trees/{tree}");
    let source = ok(
        &app,
        Method::POST,
        &format!("{base}/sources"),
        Some(json!({ "title": "Fictitious register" })),
    )
    .await;
    let source = source["id"].as_str().unwrap().to_owned();
    let repository = ok(
        &app,
        Method::POST,
        &format!("{base}/repositories"),
        Some(json!({ "name": "Fictitious archive" })),
    )
    .await;
    let repository = repository["id"].as_str().unwrap().to_owned();
    ok(
        &app,
        Method::POST,
        &format!("{base}/sources/{source}/repositories"),
        Some(json!({ "repository_id": repository, "call_number": "F-7" })),
    )
    .await;
    for (owner, id) in [("source_id", &source), ("repository_id", &repository)] {
        ok(
            &app,
            Method::POST,
            &format!("{base}/notes"),
            Some(json!({ "text": "Fictitious note", owner: id })),
        )
        .await;
    }

    // The repository goes first, which changes the source: its link went.
    ok(
        &app,
        Method::DELETE,
        &format!("{base}/repositories/{repository}"),
        None,
    )
    .await;
    assert_gone(&db, "repository", &repository).await;
    ok(
        &app,
        Method::DELETE,
        &format!("{base}/sources/{source}"),
        None,
    )
    .await;
    assert_gone(&db, "source", &source).await;

    // Back through GraphQL the repository, then through REST the source as
    // it was before the repository went.
    let history = versions(&app, &tree, "repository", &repository).await;
    assert_eq!(history[0]["deleted"], true);
    let last = history[1]["version"].as_i64().unwrap();
    gql_ok(
        &app,
        "mutation($t: ID!, $r: ID!, $v: Int!) {
            revertRecord(treeId: $t, recordType: REPOSITORY, recordId: $r, version: $v) { id }
        }",
        json!({ "t": tree, "r": repository, "v": last }),
    )
    .await;
    let history = versions(&app, &tree, "source", &source).await;
    assert_eq!(history[0]["deleted"], true);
    let linked = history
        .iter()
        .find(|v| {
            v["snapshot"]["repositories"]
                .as_array()
                .is_some_and(|l| !l.is_empty())
        })
        .expect("the source's state with its link");
    ok(
        &app,
        Method::POST,
        &format!("{base}/history/source/{source}/revert"),
        Some(json!({ "version": linked["version"] })),
    )
    .await;

    let restored = ok(&app, Method::GET, &format!("{base}/sources/{source}"), None).await;
    assert_eq!(restored["title"], "Fictitious register");
    let links = ok(
        &app,
        Method::GET,
        &format!("{base}/sources/{source}/repositories"),
        None,
    )
    .await;
    assert_eq!(links[0]["repository_id"], repository);
    assert_eq!(links[0]["call_number"], "F-7");
    for owner in [("source_id", &source), ("repository_id", &repository)] {
        let notes = ok(
            &app,
            Method::GET,
            &format!("{base}/notes?{}={}", owner.0, owner.1),
            None,
        )
        .await;
        assert_eq!(
            notes["edges"].as_array().unwrap().len(),
            1,
            "the notes of {owner:?}"
        );
    }
    // The restores recorded the absence they ended.
    let history = versions(&app, &tree, "source", &source).await;
    assert_eq!(history[0]["deleted"], false);
    assert_eq!(history[1]["deleted"], true);
}
