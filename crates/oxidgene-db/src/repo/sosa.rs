//! The stored SOSA ancestry of each tree: its root and every ancestor of the
//! root, with their SOSA-Stradonitz numbers (docs/data-model.md §4.8).
//!
//! The rows are derived data. Triggers move `sosa_state.generation` whenever
//! a family link, a family's or a person's deletion, or the root changes; the
//! rows are current while `built_generation` equals it, and the API rebuilds
//! them otherwise. This module reads and replaces them; deciding what they
//! hold is the API's.

use std::collections::HashSet;

use crate::repo::batch::in_chunks;
use crate::repo::db_err;
use oxidgene_core::enums::SpouseRole;
use oxidgene_core::error::OxidGeneError;
use sea_orm::{ConnectionTrait, DbBackend, Statement, Value};
use uuid::Uuid;

/// Rows one insert statement writes: three values each, well within what
/// either backend binds.
const INSERT_BATCH: usize = 1_000;

/// One person of a tree's SOSA ancestry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SosaAncestor {
    pub person_id: Uuid,
    /// `None` for an ancestor reached only through a spouse who is neither
    /// husband nor wife, which the numbering cannot place.
    pub sosa_number: Option<u64>,
}

/// One parent of a person, as a SOSA ancestry walks up to them: a spouse of a
/// live family the person is a child of, themselves live.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParentLink {
    pub child_id: Uuid,
    pub parent_id: Uuid,
    pub role: SpouseRole,
}

/// Reads and writes of `sosa_ancestor` and `sosa_state`.
pub struct SosaRepo;

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

impl SosaRepo {
    /// Whether the stored ancestry of `tree_id` reflects its current family
    /// links, deletions and root.
    pub async fn is_current(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<bool, OxidGeneError> {
        let backend = db.get_database_backend();
        let sql = format!(
            "SELECT 1 AS current FROM sosa_state \
             WHERE tree_id = {} AND built_generation = generation",
            placeholders(backend, 1, 1)
        );
        let row = db
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                &sql,
                [Value::from(tree_id)],
            ))
            .await
            .map_err(db_err)?;
        Ok(row.is_some())
    }

    /// Start rebuilding the ancestry of `tree_id` in transaction `txn`: the
    /// generation the rebuild will be current for, or `None` when another
    /// rebuild made it current meanwhile. It is read before any family link,
    /// so a change committed after it moves the generation past it, and the
    /// rebuild never marks as current what it did not see. On PostgreSQL the
    /// state row stays locked until `txn` ends, which also keeps two rebuilds
    /// of one tree from interleaving; on SQLite the single writer does.
    pub async fn begin_rebuild(
        txn: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Option<i64>, OxidGeneError> {
        let backend = txn.get_database_backend();
        let lock = match backend {
            DbBackend::Sqlite => "",
            _ => " FOR UPDATE",
        };
        let sql = format!(
            "SELECT generation, built_generation FROM sosa_state WHERE tree_id = {}{lock}",
            placeholders(backend, 1, 1)
        );
        let row = txn
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                &sql,
                [Value::from(tree_id)],
            ))
            .await
            .map_err(db_err)?
            .ok_or(OxidGeneError::NotFound {
                entity: "Tree",
                id: tree_id,
            })?;
        let generation: i64 = row.try_get("", "generation").map_err(db_err)?;
        let built: Option<i64> = row.try_get("", "built_generation").map_err(db_err)?;
        Ok((built != Some(generation)).then_some(generation))
    }

    /// Replace the stored ancestry of `tree_id` with `ancestors`, current as
    /// of `generation`.
    pub async fn replace_tree(
        txn: &impl ConnectionTrait,
        tree_id: Uuid,
        ancestors: &[SosaAncestor],
        generation: i64,
    ) -> Result<(), OxidGeneError> {
        let backend = txn.get_database_backend();
        let one = placeholders(backend, 1, 1);
        txn.execute_raw(Statement::from_sql_and_values(
            backend,
            format!("DELETE FROM sosa_ancestor WHERE tree_id = {one}"),
            [Value::from(tree_id)],
        ))
        .await
        .map_err(db_err)?;

        for batch in ancestors.chunks(INSERT_BATCH) {
            let rows = (0..batch.len())
                .map(|i| format!("({})", placeholders(backend, 3 * i + 1, 3)))
                .collect::<Vec<_>>()
                .join(", ");
            let mut values = Vec::with_capacity(3 * batch.len());
            for ancestor in batch {
                let number = ancestor
                    .sosa_number
                    .map(|n| i64::try_from(n).map_err(|e| OxidGeneError::Database(e.to_string())))
                    .transpose()?;
                values.extend([
                    Value::from(ancestor.person_id),
                    Value::from(tree_id),
                    Value::from(number),
                ]);
            }
            txn.execute_raw(Statement::from_sql_and_values(
                backend,
                format!(
                    "INSERT INTO sosa_ancestor (person_id, tree_id, sosa_number) VALUES {rows}"
                ),
                values,
            ))
            .await
            .map_err(db_err)?;
        }

        let (tree, built) = match backend {
            DbBackend::Sqlite => ("?", "?"),
            _ => ("$2", "$1"),
        };
        txn.execute_raw(Statement::from_sql_and_values(
            backend,
            format!("UPDATE sosa_state SET built_generation = {built} WHERE tree_id = {tree}"),
            [Value::from(generation), Value::from(tree_id)],
        ))
        .await
        .map_err(db_err)?;
        Ok(())
    }

    /// Those of `person_ids` in the stored ancestry of `tree_id`.
    pub async fn among(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        person_ids: &[Uuid],
    ) -> Result<HashSet<Uuid>, OxidGeneError> {
        let backend = db.get_database_backend();
        let rows = in_chunks(person_ids, |chunk| async move {
            let sql = format!(
                "SELECT person_id FROM sosa_ancestor \
                 WHERE tree_id = {} AND person_id IN ({})",
                placeholders(backend, 1, 1),
                placeholders(backend, 2, chunk.len())
            );
            let values = std::iter::once(Value::from(tree_id))
                .chain(chunk.into_iter().map(Value::from))
                .collect::<Vec<_>>();
            db.query_all_raw(Statement::from_sql_and_values(backend, &sql, values))
                .await
                .map_err(db_err)
        })
        .await?;
        rows.iter()
            .map(|row| row.try_get::<Uuid>("", "person_id").map_err(db_err))
            .collect()
    }

    /// The stored SOSA number of `person_id` in `tree_id`, if they have one.
    pub async fn number(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        person_id: Uuid,
    ) -> Result<Option<u64>, OxidGeneError> {
        let backend = db.get_database_backend();
        let sql = format!(
            "SELECT sosa_number FROM sosa_ancestor WHERE person_id = {} AND tree_id = {}",
            placeholders(backend, 1, 1),
            placeholders(backend, 2, 1)
        );
        let row = db
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                &sql,
                [Value::from(person_id), Value::from(tree_id)],
            ))
            .await
            .map_err(db_err)?;
        let Some(row) = row else {
            return Ok(None);
        };
        row.try_get::<Option<i64>>("", "sosa_number")
            .map_err(db_err)?
            .map(|n| u64::try_from(n).map_err(|e| OxidGeneError::Database(e.to_string())))
            .transpose()
    }

    /// The parents of each of `child_ids`: the live spouses of the live
    /// families they are children of.
    pub async fn parent_links(
        db: &impl ConnectionTrait,
        child_ids: &[Uuid],
    ) -> Result<Vec<ParentLink>, OxidGeneError> {
        let backend = db.get_database_backend();
        let rows = in_chunks(child_ids, |chunk| async move {
            let sql = format!(
                "SELECT fc.person_id AS child_id, fs.person_id AS parent_id, fs.role \
                 FROM family_child fc \
                 JOIN family f ON f.id = fc.family_id AND f.deleted_at IS NULL \
                 JOIN family_spouse fs ON fs.family_id = fc.family_id \
                 JOIN person p ON p.id = fs.person_id AND p.deleted_at IS NULL \
                 WHERE fc.person_id IN ({})",
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
                let role: String = row.try_get("", "role").map_err(db_err)?;
                Ok(ParentLink {
                    child_id: row.try_get("", "child_id").map_err(db_err)?,
                    parent_id: row.try_get("", "parent_id").map_err(db_err)?,
                    role: match role.as_str() {
                        "husband" => SpouseRole::Husband,
                        "wife" => SpouseRole::Wife,
                        _ => SpouseRole::Partner,
                    },
                })
            })
            .collect()
    }
}
