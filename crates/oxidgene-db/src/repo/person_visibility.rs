//! The stored inputs of the contemporary rule, `person_visibility`
//! (docs/data-model.md §6.8, docs/privacy.md §4.5).
//!
//! The rows are derived data, written by the web build in the transaction of
//! each write that changes them; this module reads and replaces them, and
//! reads the family links a refresh walks. Deciding what they hold is
//! `oxidgene_core::privacy`'s.

use std::collections::HashMap;

use chrono::Utc;
use oxidgene_core::enums::Privacy;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::privacy::{Descent, PersonVisibility, VISIBILITY_RULE_VERSION};
use sea_orm::{ConnectionTrait, DbBackend, Statement, Value};
use uuid::Uuid;

use crate::repo::batch::in_chunks;
use crate::repo::db_err;

/// Rows one insert statement writes: eleven values each, well within what
/// either backend binds.
const INSERT_BATCH: usize = 500;

/// Reads and writes of `person_visibility`.
pub struct PersonVisibilityRepo;

/// One membership of a live person in a live family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Membership {
    pub family_id: Uuid,
    pub person_id: Uuid,
    /// A spouse of the family, else a child.
    pub spouse: bool,
}

/// Which column a membership read selects on.
#[derive(Debug, Clone, Copy)]
pub enum MembershipsOf {
    Persons,
    Families,
}

/// `n` placeholders for `backend`, numbered from `first` on PostgreSQL.
fn placeholders(backend: DbBackend, first: usize, n: usize) -> String {
    (first..first + n)
        .map(|i| match backend {
            DbBackend::Sqlite => "?".to_string(),
            _ => format!("${i}"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn privacy_text(privacy: Privacy) -> &'static str {
    match privacy {
        Privacy::Default => "default",
        Privacy::Public => "public",
        Privacy::Private => "private",
    }
}

fn privacy_of(text: &str) -> Privacy {
    match text {
        "public" => Privacy::Public,
        "private" => Privacy::Private,
        _ => Privacy::Default,
    }
}

/// A stored row, by person.
fn row_of(row: &sea_orm::QueryResult) -> Result<(Uuid, PersonVisibility), OxidGeneError> {
    let privacy: String = row.try_get("", "privacy").map_err(db_err)?;
    let descent: String = row.try_get("", "descent").map_err(db_err)?;
    Ok((
        row.try_get("", "person_id").map_err(db_err)?,
        PersonVisibility {
            privacy: privacy_of(&privacy),
            deceased: row.try_get("", "deceased").map_err(db_err)?,
            latest_birth_year: row.try_get("", "latest_birth_year").map_err(db_err)?,
            earliest_birth_year: row.try_get("", "earliest_birth_year").map_err(db_err)?,
            recorded_birth_year: row.try_get("", "recorded_birth_year").map_err(db_err)?,
            kin_birth_year: row.try_get("", "kin_birth_year").map_err(db_err)?,
            descent: Descent::decode(&descent).unwrap_or_default(),
        },
    ))
}

impl PersonVisibilityRepo {
    /// Whether the rows of `tree_id` exist and were all derived by the
    /// current rule: a tree without any, or holding one from an earlier
    /// version, is rebuilt whole.
    pub async fn is_materialized(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<bool, OxidGeneError> {
        let backend = db.get_database_backend();
        let any = |op: &str| {
            format!(
                "SELECT person_id FROM person_visibility \
                 WHERE tree_id = {} AND rule_version {op} {} LIMIT 1",
                placeholders(backend, 1, 1),
                placeholders(backend, 2, 1)
            )
        };
        let values = || [Value::from(tree_id), Value::from(VISIBILITY_RULE_VERSION)];
        for (op, wanted) in [("<>", false), ("=", true)] {
            let found = db
                .query_one_raw(Statement::from_sql_and_values(backend, any(op), values()))
                .await
                .map_err(db_err)?
                .is_some();
            if found != wanted {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Every stored row of `tree_id`, by person: what a viewer without
    /// `read_private` is redacted from.
    pub async fn list_tree(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<HashMap<Uuid, PersonVisibility>, OxidGeneError> {
        let backend = db.get_database_backend();
        let rows = db
            .query_all_raw(Statement::from_sql_and_values(
                backend,
                format!(
                    "SELECT person_id, privacy, deceased, latest_birth_year, earliest_birth_year, \
                     recorded_birth_year, kin_birth_year, descent \
                     FROM person_visibility WHERE tree_id = {}",
                    placeholders(backend, 1, 1)
                ),
                [Value::from(tree_id)],
            ))
            .await
            .map_err(db_err)?;
        rows.iter().map(row_of).collect()
    }

    /// The stored rows of `person_ids`, by person; a person without one is
    /// absent.
    pub async fn get_many(
        db: &impl ConnectionTrait,
        person_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, PersonVisibility>, OxidGeneError> {
        let backend = db.get_database_backend();
        let rows = in_chunks(person_ids, |chunk| async move {
            let sql = format!(
                "SELECT person_id, privacy, deceased, latest_birth_year, earliest_birth_year, \
                 recorded_birth_year, kin_birth_year, descent \
                 FROM person_visibility WHERE person_id IN ({})",
                placeholders(backend, 1, chunk.len())
            );
            db.query_all_raw(Statement::from_sql_and_values(
                backend,
                &sql,
                chunk.into_iter().map(Value::from),
            ))
            .await
            .map_err(db_err)
        })
        .await?;
        rows.iter().map(row_of).collect()
    }

    /// Write `rows` of `tree_id`, replacing those the persons had.
    pub async fn upsert(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        rows: &[(Uuid, PersonVisibility)],
    ) -> Result<(), OxidGeneError> {
        let backend = db.get_database_backend();
        let now = Utc::now();
        for batch in rows.chunks(INSERT_BATCH) {
            let tuples = (0..batch.len())
                .map(|i| format!("({})", placeholders(backend, 11 * i + 1, 11)))
                .collect::<Vec<_>>()
                .join(", ");
            let mut values = Vec::with_capacity(11 * batch.len());
            for (person_id, row) in batch {
                values.extend([
                    Value::from(*person_id),
                    Value::from(tree_id),
                    Value::from(privacy_text(row.privacy)),
                    Value::from(row.deceased),
                    Value::from(row.latest_birth_year),
                    Value::from(row.earliest_birth_year),
                    Value::from(row.recorded_birth_year),
                    Value::from(row.kin_birth_year),
                    Value::from(row.descent.encode()),
                    Value::from(VISIBILITY_RULE_VERSION),
                    Value::from(now),
                ]);
            }
            let sql = format!(
                "INSERT INTO person_visibility (person_id, tree_id, privacy, deceased, \
                 latest_birth_year, earliest_birth_year, recorded_birth_year, kin_birth_year, \
                 descent, rule_version, updated_at) VALUES {tuples} \
                 ON CONFLICT (person_id) DO UPDATE SET tree_id = excluded.tree_id, \
                 privacy = excluded.privacy, deceased = excluded.deceased, \
                 latest_birth_year = excluded.latest_birth_year, \
                 earliest_birth_year = excluded.earliest_birth_year, \
                 recorded_birth_year = excluded.recorded_birth_year, \
                 kin_birth_year = excluded.kin_birth_year, descent = excluded.descent, \
                 rule_version = excluded.rule_version, updated_at = excluded.updated_at"
            );
            db.execute_raw(Statement::from_sql_and_values(backend, sql, values))
                .await
                .map_err(db_err)?;
        }
        Ok(())
    }

    /// Replace every row of `tree_id` with `rows`.
    pub async fn replace_tree(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        rows: &[(Uuid, PersonVisibility)],
    ) -> Result<(), OxidGeneError> {
        Self::delete_tree(db, tree_id).await?;
        Self::upsert(db, tree_id, rows).await
    }

    /// Remove the rows of `person_ids`.
    pub async fn delete_persons(
        db: &impl ConnectionTrait,
        person_ids: &[Uuid],
    ) -> Result<(), OxidGeneError> {
        let backend = db.get_database_backend();
        in_chunks(person_ids, |chunk| async move {
            let sql = format!(
                "DELETE FROM person_visibility WHERE person_id IN ({})",
                placeholders(backend, 1, chunk.len())
            );
            db.execute_raw(Statement::from_sql_and_values(
                backend,
                sql,
                chunk.into_iter().map(Value::from),
            ))
            .await
            .map_err(db_err)?;
            Ok(Vec::<()>::new())
        })
        .await?;
        Ok(())
    }

    /// Remove every row of `tree_id`.
    pub async fn delete_tree(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<(), OxidGeneError> {
        let backend = db.get_database_backend();
        db.execute_raw(Statement::from_sql_and_values(
            backend,
            format!(
                "DELETE FROM person_visibility WHERE tree_id = {}",
                placeholders(backend, 1, 1)
            ),
            [Value::from(tree_id)],
        ))
        .await
        .map_err(db_err)?;
        Ok(())
    }

    /// The privacy of each live person of `person_ids`; a deleted or missing
    /// person is absent.
    pub async fn live_privacy(
        db: &impl ConnectionTrait,
        person_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, Privacy>, OxidGeneError> {
        let backend = db.get_database_backend();
        let rows = in_chunks(person_ids, |chunk| async move {
            let sql = format!(
                "SELECT id, privacy FROM person WHERE id IN ({}) AND deleted_at IS NULL",
                placeholders(backend, 1, chunk.len())
            );
            db.query_all_raw(Statement::from_sql_and_values(
                backend,
                &sql,
                chunk.into_iter().map(Value::from),
            ))
            .await
            .map_err(db_err)
        })
        .await?;
        rows.iter()
            .map(|row| {
                let privacy: String = row.try_get("", "privacy").map_err(db_err)?;
                Ok((row.try_get("", "id").map_err(db_err)?, privacy_of(&privacy)))
            })
            .collect()
    }

    /// The memberships of live persons in live families whose person (or
    /// family, as `of` says) is one of `ids`.
    pub async fn memberships(
        db: &impl ConnectionTrait,
        ids: &[Uuid],
        of: MembershipsOf,
    ) -> Result<Vec<Membership>, OxidGeneError> {
        let backend = db.get_database_backend();
        let column = match of {
            MembershipsOf::Persons => "person_id",
            MembershipsOf::Families => "family_id",
        };
        let rows = in_chunks(ids, |chunk| async move {
            let n = chunk.len();
            let sql = format!(
                "SELECT fs.family_id, fs.person_id, 1 AS spouse FROM family_spouse fs \
                 JOIN family f ON f.id = fs.family_id AND f.deleted_at IS NULL \
                 JOIN person p ON p.id = fs.person_id AND p.deleted_at IS NULL \
                 WHERE fs.{column} IN ({}) \
                 UNION ALL \
                 SELECT fc.family_id, fc.person_id, 0 AS spouse FROM family_child fc \
                 JOIN family f ON f.id = fc.family_id AND f.deleted_at IS NULL \
                 JOIN person p ON p.id = fc.person_id AND p.deleted_at IS NULL \
                 WHERE fc.{column} IN ({})",
                placeholders(backend, 1, n),
                placeholders(backend, n + 1, n)
            );
            let values = chunk.iter().chain(chunk.iter()).copied().map(Value::from);
            db.query_all_raw(Statement::from_sql_and_values(backend, &sql, values))
                .await
                .map_err(db_err)
        })
        .await?;
        rows.iter()
            .map(|row| {
                let spouse: i32 = row.try_get("", "spouse").map_err(db_err)?;
                Ok(Membership {
                    family_id: row.try_get("", "family_id").map_err(db_err)?,
                    person_id: row.try_get("", "person_id").map_err(db_err)?,
                    spouse: spouse == 1,
                })
            })
            .collect()
    }
}
