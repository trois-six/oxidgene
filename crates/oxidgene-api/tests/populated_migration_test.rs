//! The migrations on a populated database, on SQLite and, under `just
//! test-postgres`, on PostgreSQL (docs/development.md §2.7).
//!
//! The migration test of `oxidgene-db` runs the schema up and down on an
//! empty database, where no constraint, trigger or foreign-key action ever
//! sees a row. Here every table of the schema holds at least one row,
//! written the way the application writes it — the records, their
//! projections and search rows through the API and an import, the accounts,
//! sharing, notifications, mail, access log and the derived rows only the
//! web build's reads would build through their repositories — then the
//! migrator runs
//! again, which must find nothing to apply and change nothing, and the
//! schema is taken down over those rows and brought up again.
//!
//! The tables are read from the schema itself, not listed here: a table a
//! later change adds fails this test until it is given a row below. Under
//! SQLite the storage tables of the full-text index are its own, filled as
//! it sees fit, and are left out.
//!
//! All data is fictitious.

use std::collections::HashSet;

use chrono::{Duration, Utc};
use oxidgene_core::access::{InstanceRole, TreeRole};
use oxidgene_core::enums::ExclusionKind;
use oxidgene_core::privacy::PersonVisibility;
use oxidgene_core::sharing::{ScopeDefinition, ScopeKind};
use oxidgene_db::repo::{
    AccessEventRepo, AccessScopeRepo, AppUserRepo, Exclusion, ExclusionRepo, InvitationRepo,
    MailOutboxRepo, MembershipRepo, NewAccessEvent, NewAppUser, NewInvitation, NewMail,
    NewNotification, NewShareLink, NotificationRepo, PersonDistinctRepo, PersonVisibilityRepo,
    ShareLinkRepo, SosaAncestor, SosaRepo, rollback_migrations, run_migrations,
};
use oxidgene_db::sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use uuid::Uuid;

use crate::common::populated::populated_tree;
use crate::common::{app_on, import_gedcom, new_tree, setup_db};

/// A marriage giving both spouses' ages, and a tag of no standard, which an
/// import keeps as it read it.
const AGES_AND_KEPT: &str = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n2 FORM LINEAGE-LINKED\n1 CHAR UTF-8\n\
0 @I1@ INDI\n1 NAME Alder /Fernwood/\n1 SEX M\n1 _FIXTURE kept as read\n\
0 @I2@ INDI\n1 NAME Briar /Hollowell/\n1 SEX F\n\
0 @F1@ FAM\n1 HUSB @I1@\n1 WIFE @I2@\n1 MARR\n2 DATE 1901\n2 HUSB\n3 AGE 25y\n2 WIFE\n3 AGE 22y\n\
0 TRLR\n";

/// The tables of the schema `db` is connected to, by name, but the
/// migrator's own and, under SQLite, the storage of a virtual table.
async fn schema_tables(db: &DatabaseConnection) -> Vec<String> {
    let backend = db.get_database_backend();
    let sql = match backend {
        DbBackend::Sqlite => {
            "SELECT name FROM sqlite_master AS t WHERE type = 'table' \
             AND name NOT LIKE 'sqlite_%' AND name <> 'seaql_migrations' \
             AND NOT EXISTS (SELECT 1 FROM sqlite_master AS v \
                 WHERE v.type = 'table' AND v.sql LIKE 'CREATE VIRTUAL TABLE%' \
                 AND t.name LIKE v.name || '_%') \
             ORDER BY name"
        }
        _ => {
            "SELECT table_name AS name FROM information_schema.tables \
             WHERE table_schema = current_schema() AND table_type = 'BASE TABLE' \
             AND table_name <> 'seaql_migrations' ORDER BY table_name"
        }
    };
    db.query_all_raw(Statement::from_string(backend, sql))
        .await
        .expect("the schema's tables")
        .iter()
        .map(|row| row.try_get::<String>("", "name").expect("a name"))
        .collect()
}

/// Two persons of tree `tree_id`.
async fn two_persons(db: &DatabaseConnection, tree_id: Uuid) -> (Uuid, Uuid) {
    let backend = db.get_database_backend();
    let placeholder = if backend == DbBackend::Sqlite {
        "?"
    } else {
        "$1"
    };
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            backend,
            format!("SELECT id FROM person WHERE tree_id = {placeholder} ORDER BY id LIMIT 2"),
            [tree_id.into()],
        ))
        .await
        .expect("the tree's persons");
    let id = |index: usize| rows[index].try_get::<Uuid>("", "id").expect("an id");
    (id(0), id(1))
}

/// The derived rows of a tree no request of the populated tree builds
/// without the web build's accounts, written as their rebuilds write them:
/// a SOSA ancestor, a visibility row, an exclusion and its union, and two
/// persons known to be different.
async fn fill_derived(db: &DatabaseConnection, tree_id: Uuid) {
    let (person, other) = two_persons(db, tree_id).await;
    SosaRepo::replace_tree(
        db,
        tree_id,
        &[SosaAncestor {
            person_id: person,
            sosa_number: Some(1),
        }],
        1,
    )
    .await
    .expect("a SOSA ancestor");
    PersonVisibilityRepo::upsert(db, tree_id, &[(person, PersonVisibility::default())])
        .await
        .expect("a visibility row");
    ExclusionRepo::create(
        db,
        &Exclusion {
            id: Uuid::now_v7(),
            tree_id,
            kind: ExclusionKind::Person,
            person_id: other,
            depth: None,
            include_root: true,
            include_spouses: false,
            created_by: None,
            created_at: Utc::now(),
        },
    )
    .await
    .expect("an exclusion");
    ExclusionRepo::replace_union(db, tree_id, &HashSet::from([other]), 1)
        .await
        .expect("the exclusions' union");
    PersonDistinctRepo::mark(db, tree_id, person, &[other])
        .await
        .expect("two persons known to be different");
}

/// How many rows `table` holds.
async fn count(db: &DatabaseConnection, table: &str) -> i64 {
    let backend = db.get_database_backend();
    db.query_one_raw(Statement::from_string(
        backend,
        format!("SELECT COUNT(*) AS n FROM {table}"),
    ))
    .await
    .unwrap_or_else(|error| panic!("count {table}: {error}"))
    .expect("a count")
    .try_get::<i64>("", "n")
    .expect("a number")
}

/// Every table with its row count.
async fn counts(db: &DatabaseConnection) -> Vec<(String, i64)> {
    let mut counted = Vec::new();
    for table in schema_tables(db).await {
        let rows = count(db, &table).await;
        counted.push((table, rows));
    }
    counted
}

/// The rows no API request of the populated tree writes: an account and
/// what hangs on it — a membership, an invitation, a share link with its
/// scope, a notification and its preference, a queued mail — and an entry
/// of the access log.
async fn fill_accounts(db: &DatabaseConnection, tree_id: Uuid, person_id: Uuid) {
    let user = AppUserRepo::create(
        db,
        NewAppUser {
            issuer: "https://idp.example.invalid".into(),
            subject: "fixture-subject".into(),
            email: Some("member@example.invalid".into()),
            email_verified: true,
            provider_name: None,
            username: Some("fixture".into()),
            locale: Some("en".into()),
            instance_role: InstanceRole::User,
            can_create_trees: true,
        },
    )
    .await
    .expect("a user");
    MembershipRepo::create(db, tree_id, user.id, TreeRole::Editor, None)
        .await
        .expect("a membership");
    let scope = AccessScopeRepo::create(
        db,
        tree_id,
        &ScopeDefinition {
            kind: ScopeKind::AncestorsOf,
            person_id,
            depth: Some(2),
            include_self: true,
            include_spouses: false,
            collateral: false,
        },
    )
    .await
    .expect("a scope");
    AccessScopeRepo::replace_persons(db, scope.id, &HashSet::from([person_id]), 1)
        .await
        .expect("the scope's persons");
    InvitationRepo::create(
        db,
        NewInvitation {
            tree_id,
            email: "invitee@example.invalid".into(),
            role: TreeRole::Guest,
            scope_id: Some(scope.id),
            membership_expires_at: None,
            expires_at: Utc::now() + Duration::days(7),
            created_by: Some(user.id),
        },
    )
    .await
    .expect("an invitation");
    let link = ShareLinkRepo::create(
        db,
        NewShareLink {
            tree_id,
            token_hash: "0".repeat(64),
            label: Some("Fixture link".into()),
            scope_id: Some(scope.id),
            expires_at: None,
            created_by: Some(user.id),
        },
    )
    .await
    .expect("a share link");
    NotificationRepo::insert(
        db,
        NewNotification {
            user_id: user.id,
            kind: "job_finished",
            tree_id: Some(tree_id),
            actor_user_id: None,
            subject_id: None,
        },
    )
    .await
    .expect("a notification");
    NotificationRepo::set_preference(db, user.id, "job_finished", "off")
        .await
        .expect("a preference");
    MailOutboxRepo::enqueue(
        db,
        NewMail {
            user_id: Some(user.id),
            to_address: "member@example.invalid".into(),
            kind: "invitation",
            language: "en".into(),
            params: serde_json::json!({}),
            due: Utc::now(),
        },
    )
    .await
    .expect("a mail");
    AccessEventRepo::insert(
        db,
        NewAccessEvent {
            kind: "membership_expired",
            principal_kind: "user",
            user_id: Some(user.id),
            share_link_id: Some(link.id),
            tree_id: Some(tree_id),
            outcome: TreeRole::Editor.as_str().to_string(),
            ..NewAccessEvent::default()
        },
    )
    .await
    .expect("an access event");
}

/// Every table holds a row; the migrator finds nothing left to apply on
/// them and leaves them as they are; the schema comes down over them and
/// up again.
#[tokio::test]
async fn the_migrations_run_over_a_database_where_every_table_holds_a_row() {
    let db = setup_db().await;
    let app = app_on(db.clone());
    let tree = populated_tree(&app, &db, "Populated", 2).await;
    let tree_id: Uuid = tree.tree_id.parse().expect("a tree id");
    let person_id: Uuid = tree.id("person_id").parse().expect("a person id");
    fill_accounts(&db, tree_id, person_id).await;
    fill_derived(&db, tree_id).await;
    let ages = new_tree(&app, "Ages").await;
    import_gedcom(&app, &db, &ages, AGES_AND_KEPT).await;

    let before = counts(&db).await;
    let empty: Vec<&str> = before
        .iter()
        .filter(|(_, rows)| *rows == 0)
        .map(|(table, _)| table.as_str())
        .collect();
    assert!(
        empty.is_empty(),
        "tables without a row, to be given one in this test: {empty:?}"
    );

    run_migrations(&db).await.expect("nothing left to apply");
    assert_eq!(counts(&db).await, before, "the migrator changed rows");

    rollback_migrations(&db)
        .await
        .expect("the schema comes down over its rows");
    assert!(
        schema_tables(&db).await.is_empty(),
        "tables left after the migrations were rolled back"
    );
    run_migrations(&db)
        .await
        .expect("the schema comes up again");
    let after = counts(&db).await;
    assert_eq!(
        after.iter().map(|(table, _)| table).collect::<Vec<_>>(),
        before.iter().map(|(table, _)| table).collect::<Vec<_>>(),
        "the schema came back with other tables"
    );
}
