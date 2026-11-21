//! Write transactions take the tree first: what a write reads before it
//! writes — the state its history stores — is the committed state, whatever
//! another writer of the tree was doing meanwhile. All data is fictitious.

use std::time::Duration;

use axum::http::Method;
use oxidgene_core::Sex;
use oxidgene_db::repo::PersonRepo;
use sea_orm::{DbBackend, TransactionTrait};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::common::{app_on, new_person, new_tree, ok, setup_db};

/// The stored and current versions of person `id`, latest first.
async fn versions(app: &axum::Router, tree: &str, id: &str) -> Vec<Value> {
    ok(
        app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/history/person/{id}?first=100"),
        None,
    )
    .await["edges"]
        .as_array()
        .expect("versions")
        .iter()
        .map(|edge| edge["node"].clone())
        .collect()
}

/// Two writers of one person on PostgreSQL, the second starting while the
/// first has written and not committed. The second waits for the first, and
/// the state its history stores as replaced is the one the first committed:
/// had it read before waiting, it would store the state before both.
///
/// SQLite has a single writing connection, which the first writer holds:
/// there is nothing to interleave, and the test is PostgreSQL's alone.
#[tokio::test]
async fn a_write_waiting_for_another_stores_the_committed_state_as_replaced() {
    let db = setup_db().await;
    if db.get_database_backend() != DbBackend::Postgres {
        return;
    }
    let app = app_on(db.clone());
    let tree = new_tree(&app, "Fictional tree").await;
    let person = new_person(&app, &tree).await;
    let person_id = Uuid::parse_str(&person).unwrap();

    // The first writer: a write of the tree, not yet committed.
    let first = db.begin().await.expect("begin");
    PersonRepo::update(&first, person_id, Some(Sex::Male), None)
        .await
        .expect("the first writer writes");

    // The second writer: the service, over REST.
    let second = tokio::spawn({
        let (app, tree, person) = (app.clone(), tree.clone(), person.clone());
        async move {
            ok(
                &app,
                Method::PUT,
                &format!("/api/v1/trees/{tree}/persons/{person}"),
                Some(json!({ "sex": "female" })),
            )
            .await
        }
    });
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        !second.is_finished(),
        "the second writer waits for the first"
    );
    first.commit().await.expect("commit");
    second.await.expect("the second writer goes on");

    let history = versions(&app, &tree, &person).await;
    assert_eq!(history[0]["snapshot"]["sex"], "female", "{history:?}");
    assert_eq!(
        history[1]["snapshot"]["sex"], "male",
        "the state the second write replaced is the committed one: {history:?}"
    );
}
