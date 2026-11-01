//! The owner's exclusions from sharing (docs/data-model.md §6.9,
//! docs/privacy.md §6): `privacy_exclusion`, the union of the persons they
//! cover in `excluded_person`, and `exclusion_state`, the SOSA ancestry
//! generation that union was built from.
//!
//! The union is derived data: the triggers that move a tree's
//! `sosa_state.generation` on every change to a family link or a deletion
//! (docs/data-model.md §4.8) make it stale, and the API rebuilds it at the
//! next read that needs it, as an access scope is (docs/authorization.md
//! §6.5). Deciding what it holds is `oxidgene_core::privacy`'s.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use oxidgene_core::enums::ExclusionKind;
use oxidgene_core::error::OxidGeneError;
use sea_orm::{ConnectionTrait, DbBackend, Statement, Value};
use uuid::Uuid;

use crate::repo::db_err;

/// Rows one insert statement writes: two values each.
const INSERT_BATCH: usize = 2_000;

/// One exclusion from sharing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exclusion {
    pub id: Uuid,
    pub tree_id: Uuid,
    pub kind: ExclusionKind,
    /// The excluded person, or the root of the excluded branch.
    pub person_id: Uuid,
    /// The generations walked; all of them when none.
    pub depth: Option<i32>,
    pub include_root: bool,
    pub include_spouses: bool,
    /// The user who created it; none on a server without accounts.
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

/// Reads and writes of the exclusions and their union.
pub struct ExclusionRepo;

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

const COLUMNS: &str =
    "id, tree_id, kind, person_id, depth, include_root, include_spouses, created_by, created_at";

fn exclusion_of(row: &sea_orm::QueryResult) -> Result<Exclusion, OxidGeneError> {
    let kind: String = row.try_get("", "kind").map_err(db_err)?;
    Ok(Exclusion {
        id: row.try_get("", "id").map_err(db_err)?,
        tree_id: row.try_get("", "tree_id").map_err(db_err)?,
        kind: ExclusionKind::parse(&kind)
            .ok_or_else(|| OxidGeneError::Database(format!("unknown exclusion kind {kind}")))?,
        person_id: row.try_get("", "person_id").map_err(db_err)?,
        depth: row.try_get("", "depth").map_err(db_err)?,
        include_root: row.try_get("", "include_root").map_err(db_err)?,
        include_spouses: row.try_get("", "include_spouses").map_err(db_err)?,
        created_by: row.try_get("", "created_by").map_err(db_err)?,
        created_at: row.try_get("", "created_at").map_err(db_err)?,
    })
}

impl ExclusionRepo {
    /// Every exclusion of `tree_id`, oldest first.
    pub async fn list(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<Exclusion>, OxidGeneError> {
        let backend = db.get_database_backend();
        let rows = db
            .query_all_raw(Statement::from_sql_and_values(
                backend,
                format!(
                    "SELECT {COLUMNS} FROM privacy_exclusion WHERE tree_id = {} ORDER BY id",
                    placeholders(backend, 1, 1)
                ),
                [Value::from(tree_id)],
            ))
            .await
            .map_err(db_err)?;
        rows.iter().map(exclusion_of).collect()
    }

    /// Exclusion `id` of `tree_id`; `NotFound` otherwise.
    pub async fn get(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        id: Uuid,
    ) -> Result<Exclusion, OxidGeneError> {
        let backend = db.get_database_backend();
        let row = db
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                format!(
                    "SELECT {COLUMNS} FROM privacy_exclusion WHERE id = {} AND tree_id = {}",
                    placeholders(backend, 1, 1),
                    placeholders(backend, 2, 1)
                ),
                [Value::from(id), Value::from(tree_id)],
            ))
            .await
            .map_err(db_err)?
            .ok_or(OxidGeneError::NotFound {
                entity: "Exclusion",
                id,
            })?;
        exclusion_of(&row)
    }

    /// Store `exclusion`.
    pub async fn create(
        db: &impl ConnectionTrait,
        exclusion: &Exclusion,
    ) -> Result<(), OxidGeneError> {
        let backend = db.get_database_backend();
        db.execute_raw(Statement::from_sql_and_values(
            backend,
            format!(
                "INSERT INTO privacy_exclusion ({COLUMNS}) VALUES ({})",
                placeholders(backend, 1, 9)
            ),
            [
                Value::from(exclusion.id),
                Value::from(exclusion.tree_id),
                Value::from(exclusion.kind.as_str()),
                Value::from(exclusion.person_id),
                Value::from(exclusion.depth),
                Value::from(exclusion.include_root),
                Value::from(exclusion.include_spouses),
                Value::from(exclusion.created_by),
                Value::from(exclusion.created_at),
            ],
        ))
        .await
        .map_err(db_err)?;
        Ok(())
    }

    /// Remove exclusion `id` of `tree_id`; `NotFound` when there is none.
    pub async fn delete(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        id: Uuid,
    ) -> Result<(), OxidGeneError> {
        let backend = db.get_database_backend();
        let result = db
            .execute_raw(Statement::from_sql_and_values(
                backend,
                format!(
                    "DELETE FROM privacy_exclusion WHERE id = {} AND tree_id = {}",
                    placeholders(backend, 1, 1),
                    placeholders(backend, 2, 1)
                ),
                [Value::from(id), Value::from(tree_id)],
            ))
            .await
            .map_err(db_err)?;
        if result.rows_affected() == 0 {
            return Err(OxidGeneError::NotFound {
                entity: "Exclusion",
                id,
            });
        }
        Ok(())
    }

    /// Root the exclusions of `from` at `to`: a merge keeps a person
    /// excluded when either record was (docs/privacy.md §6.5).
    pub async fn move_root(
        db: &impl ConnectionTrait,
        from: Uuid,
        to: Uuid,
    ) -> Result<(), OxidGeneError> {
        let backend = db.get_database_backend();
        let (to_slot, from_slot) = match backend {
            DbBackend::Sqlite => ("?", "?"),
            _ => ("$1", "$2"),
        };
        db.execute_raw(Statement::from_sql_and_values(
            backend,
            format!(
                "UPDATE privacy_exclusion SET person_id = {to_slot} WHERE person_id = {from_slot}"
            ),
            [Value::from(to), Value::from(from)],
        ))
        .await
        .map_err(db_err)?;
        Ok(())
    }

    /// The tree's current SOSA ancestry generation, and the one its union
    /// was built from (none before the first build): the union is current
    /// while they are equal. The ancestry generation is read before any
    /// family link a rebuild reads, so a change committed meanwhile leaves
    /// the union stale rather than marked current.
    pub async fn generations(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<(i64, Option<i64>), OxidGeneError> {
        let backend = db.get_database_backend();
        let row = db
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                format!(
                    "SELECT s.generation, e.built_generation FROM sosa_state s \
                     LEFT JOIN exclusion_state e ON e.tree_id = s.tree_id \
                     WHERE s.tree_id = {}",
                    placeholders(backend, 1, 1)
                ),
                [Value::from(tree_id)],
            ))
            .await
            .map_err(db_err)?
            .ok_or(OxidGeneError::NotFound {
                entity: "Tree",
                id: tree_id,
            })?;
        Ok((
            row.try_get("", "generation").map_err(db_err)?,
            row.try_get("", "built_generation").map_err(db_err)?,
        ))
    }

    /// Replace the union of `tree_id` with `persons`, current as of
    /// `generation`.
    pub async fn replace_union(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        persons: &HashSet<Uuid>,
        generation: i64,
    ) -> Result<(), OxidGeneError> {
        let backend = db.get_database_backend();
        db.execute_raw(Statement::from_sql_and_values(
            backend,
            format!(
                "DELETE FROM excluded_person WHERE tree_id = {}",
                placeholders(backend, 1, 1)
            ),
            [Value::from(tree_id)],
        ))
        .await
        .map_err(db_err)?;
        let persons: Vec<Uuid> = persons.iter().copied().collect();
        for batch in persons.chunks(INSERT_BATCH) {
            let tuples = (0..batch.len())
                .map(|i| format!("({})", placeholders(backend, 2 * i + 1, 2)))
                .collect::<Vec<_>>()
                .join(", ");
            let values = batch
                .iter()
                .flat_map(|person| [Value::from(tree_id), Value::from(*person)]);
            db.execute_raw(Statement::from_sql_and_values(
                backend,
                format!("INSERT INTO excluded_person (tree_id, person_id) VALUES {tuples}"),
                values,
            ))
            .await
            .map_err(db_err)?;
        }
        db.execute_raw(Statement::from_sql_and_values(
            backend,
            format!(
                "INSERT INTO exclusion_state (tree_id, built_generation) VALUES ({}) \
                 ON CONFLICT (tree_id) DO UPDATE SET built_generation = excluded.built_generation",
                placeholders(backend, 1, 2)
            ),
            [Value::from(tree_id), Value::from(generation)],
        ))
        .await
        .map_err(db_err)?;
        Ok(())
    }

    /// The persons of `tree_id` its exclusions cover, as last built.
    pub async fn excluded(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<HashSet<Uuid>, OxidGeneError> {
        let backend = db.get_database_backend();
        let rows = db
            .query_all_raw(Statement::from_sql_and_values(
                backend,
                format!(
                    "SELECT person_id FROM excluded_person WHERE tree_id = {}",
                    placeholders(backend, 1, 1)
                ),
                [Value::from(tree_id)],
            ))
            .await
            .map_err(db_err)?;
        rows.iter()
            .map(|row| row.try_get::<Uuid>("", "person_id").map_err(db_err))
            .collect()
    }
}
