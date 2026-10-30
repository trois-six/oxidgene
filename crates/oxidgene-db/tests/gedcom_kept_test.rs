//! Integration tests for the kept GEDCOM repository (`gedcom_kept`).
//!
//! All tests run against a fresh database (`common::setup_db`).

use chrono::Utc;
use oxidgene_core::enums::Sex;
use oxidgene_core::types::{GedcomKept, KeptEntityKind};
use oxidgene_db::repo::{GedcomKeptRepo, PersonRepo, TreeRepo};
use sea_orm::DatabaseConnection;

use crate::common::setup_db;
use uuid::Uuid;

async fn create_tree(db: &DatabaseConnection) -> Uuid {
    let id = Uuid::now_v7();
    TreeRepo::create(db, id, "Test Tree".into(), None)
        .await
        .expect("create tree");
    id
}

async fn create_person(db: &DatabaseConnection, tree_id: Uuid) -> Uuid {
    let id = Uuid::now_v7();
    PersonRepo::create(db, id, tree_id, Sex::Unknown)
        .await
        .expect("create person");
    id
}

fn kept(tree_id: Uuid, kind: KeptEntityKind, entity_id: Uuid, nodes: &str) -> GedcomKept {
    GedcomKept {
        id: Uuid::now_v7(),
        tree_id,
        entity_kind: kind,
        entity_id,
        nodes: nodes.to_string(),
        created_at: Utc::now(),
    }
}

#[tokio::test]
async fn kept_rows_are_inserted_listed_and_scoped_to_their_tree() {
    let db = setup_db().await;
    let tree_id = create_tree(&db).await;
    let other_tree_id = create_tree(&db).await;
    let person_id = create_person(&db, tree_id).await;
    let family_id = Uuid::now_v7();

    let rows = vec![
        kept(
            tree_id,
            KeptEntityKind::Person,
            person_id,
            r#"[{"tag":"REFN","value":"A-1"}]"#,
        ),
        kept(tree_id, KeptEntityKind::Family, family_id, "[]"),
        kept(tree_id, KeptEntityKind::Record, Uuid::now_v7(), "[]"),
    ];
    GedcomKeptRepo::insert_many(&db, &rows)
        .await
        .expect("insert kept rows");
    GedcomKeptRepo::insert_many(
        &db,
        &[kept(
            other_tree_id,
            KeptEntityKind::Submitter,
            other_tree_id,
            "[]",
        )],
    )
    .await
    .expect("insert a row of another tree");

    let listed = GedcomKeptRepo::list_by_tree(&db, tree_id)
        .await
        .expect("list kept rows");
    assert_eq!(listed.len(), 3);
    let person_row = listed
        .iter()
        .find(|row| row.entity_kind == KeptEntityKind::Person)
        .expect("the person's row");
    assert_eq!(person_row.entity_id, person_id);
    assert_eq!(person_row.nodes, r#"[{"tag":"REFN","value":"A-1"}]"#);
    assert_eq!(person_row.tree_id, tree_id);
    assert!(
        listed
            .iter()
            .any(|row| row.entity_kind == KeptEntityKind::Record)
    );

    let other = GedcomKeptRepo::list_by_tree(&db, other_tree_id)
        .await
        .expect("list the other tree");
    assert_eq!(other.len(), 1);
    assert_eq!(other[0].entity_kind, KeptEntityKind::Submitter);
}

#[tokio::test]
async fn insert_many_takes_more_rows_than_one_chunk() {
    let db = setup_db().await;
    let tree_id = create_tree(&db).await;
    let rows: Vec<GedcomKept> = (0..1_200)
        .map(|_| kept(tree_id, KeptEntityKind::Media, Uuid::now_v7(), "[]"))
        .collect();

    GedcomKeptRepo::insert_many(&db, &rows)
        .await
        .expect("insert a large batch");

    let listed = GedcomKeptRepo::list_by_tree(&db, tree_id)
        .await
        .expect("list kept rows");
    assert_eq!(listed.len(), 1_200);
}

#[tokio::test]
async fn repoint_moves_only_the_rows_of_that_kind_and_entity() {
    let db = setup_db().await;
    let tree_id = create_tree(&db).await;
    let duplicate = create_person(&db, tree_id).await;
    let kept_person = create_person(&db, tree_id).await;
    let bystander = create_person(&db, tree_id).await;

    GedcomKeptRepo::insert_many(
        &db,
        &[
            kept(tree_id, KeptEntityKind::Person, duplicate, "[1]"),
            kept(tree_id, KeptEntityKind::Person, duplicate, "[2]"),
            kept(tree_id, KeptEntityKind::Person, bystander, "[3]"),
            // Same id, other kind: not a person's row.
            kept(tree_id, KeptEntityKind::Family, duplicate, "[4]"),
        ],
    )
    .await
    .expect("insert kept rows");

    GedcomKeptRepo::repoint(&db, KeptEntityKind::Person, duplicate, kept_person)
        .await
        .expect("repoint");

    let listed = GedcomKeptRepo::list_by_tree(&db, tree_id)
        .await
        .expect("list kept rows");
    let on = |kind: KeptEntityKind, id: Uuid| {
        listed
            .iter()
            .filter(|row| row.entity_kind == kind && row.entity_id == id)
            .count()
    };
    assert_eq!(on(KeptEntityKind::Person, duplicate), 0);
    assert_eq!(on(KeptEntityKind::Person, kept_person), 2);
    assert_eq!(on(KeptEntityKind::Person, bystander), 1);
    assert_eq!(on(KeptEntityKind::Family, duplicate), 1);
}

#[tokio::test]
async fn purging_a_tree_takes_its_kept_rows_with_it() {
    let db = setup_db().await;
    let tree_id = create_tree(&db).await;
    let person_id = create_person(&db, tree_id).await;
    GedcomKeptRepo::insert_many(
        &db,
        &[kept(tree_id, KeptEntityKind::Person, person_id, "[]")],
    )
    .await
    .expect("insert kept row");

    TreeRepo::soft_delete(&db, tree_id)
        .await
        .expect("soft delete");
    TreeRepo::purge(&db, tree_id).await.expect("purge");

    assert!(
        GedcomKeptRepo::list_by_tree(&db, tree_id)
            .await
            .expect("list kept rows")
            .is_empty()
    );
}
