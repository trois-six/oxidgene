//! Ancestor / descendant traversal.
//!
//! Recursive queries report each ancestor at the shortest distance when
//! pedigree implex makes them reachable by several paths.
//!
//! Both back-ends support `WITH RECURSIVE` (SQLite since 3.8.3), and the
//! parent relation is read straight from the family links:
//! a person's parents are the spouses of the family in which they are a child.

use crate::repo::db_err;
use oxidgene_core::enums::SpouseRole;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::AncestryLink;
use sea_orm::{ConnectionTrait, DbBackend, Statement, Value};
use uuid::Uuid;

/// Hard ceiling on recursion depth when the caller does not set one.
///
/// The CTE walks `depth` upwards without ever revisiting a (person, depth)
/// pair, so a cycle in the family links — which the schema does not prevent,
/// and which corrupt imports do produce — would otherwise recurse forever.
const MAX_GENERATIONS: i32 = 64;

/// Maximum SOSA depth representable by the signed 64-bit integers shared by
/// SQLite and PostgreSQL. The largest number at depth 62 is `i64::MAX`.
const MAX_SOSA_GENERATIONS: i32 = 62;

/// Ancestor and descendant traversal over the family links.
pub struct AncestryRepo;

/// One membership of a person in a family, as [`AncestryRepo::family_links`]
/// reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FamilyLink {
    pub family_id: Uuid,
    pub person_id: Uuid,
    /// `Some` for a spouse, `None` for a child.
    pub spouse_role: Option<SpouseRole>,
    pub sort_order: i32,
}

impl AncestryRepo {
    /// Return one deterministic SOSA-Stradonitz number for `person_id`
    /// relative to `root_person_id`.
    ///
    /// The recursive query keeps the parent role so it can propagate `2n` for
    /// fathers and `2n + 1` for mothers. When pedigree implex reaches the same
    /// person through several paths, the shortest path and then lowest SOSA
    /// number wins.
    #[tracing::instrument(name = "pedigree.sosa_number", skip_all)]
    pub async fn sosa_number(
        db: &impl ConnectionTrait,
        root_person_id: Uuid,
        person_id: Uuid,
    ) -> Result<Option<u64>, OxidGeneError> {
        let backend = db.get_database_backend();
        let (root, target, limit) = match backend {
            DbBackend::Sqlite => ("?1", "?2", "?3"),
            _ => ("$1", "$2", "$3"),
        };
        let sql = format!(
            "WITH RECURSIVE step(person_id, sosa, depth) AS ( \
                 SELECT {root}, CAST(1 AS BIGINT), 0 \
                 UNION \
                 SELECT fs.person_id, \
                        CASE fs.role \
                            WHEN 'husband' THEN step.sosa * 2 \
                            WHEN 'wife' THEN step.sosa * 2 + 1 \
                        END, \
                        step.depth + 1 \
                 FROM step \
                 JOIN family_child fc ON fc.person_id = step.person_id \
                 JOIN family_spouse fs ON fs.family_id = fc.family_id \
                 WHERE step.depth < {limit} \
                   AND fs.role IN ('husband', 'wife') \
             ) \
             SELECT sosa \
             FROM step \
             WHERE person_id = {target} \
             ORDER BY depth, sosa \
             LIMIT 1"
        );

        let row = db
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                &sql,
                [
                    Value::from(root_person_id),
                    Value::from(person_id),
                    Value::from(MAX_SOSA_GENERATIONS),
                ],
            ))
            .await
            .map_err(db_err)?;
        row.map(|row| {
            let number = row.try_get::<i64>("", "sosa").map_err(db_err)?;
            u64::try_from(number).map_err(|e| OxidGeneError::Database(e.to_string()))
        })
        .transpose()
    }

    /// Every ancestor of `person_id`, each at its shortest distance.
    ///
    /// Walks child → family → spouses. `max_depth` counts generations, so 1 is
    /// the parents; `None` falls back to [`MAX_GENERATIONS`]. The person
    /// themself is never included.
    #[tracing::instrument(name = "pedigree.ancestors", skip_all, fields(max_depth))]
    pub async fn ancestors(
        db: &impl ConnectionTrait,
        person_id: Uuid,
        max_depth: Option<i32>,
    ) -> Result<Vec<AncestryLink>, OxidGeneError> {
        // Step upwards: from a person, to the families they are a child of,
        // to the spouses of those families.
        Self::walk(
            db,
            person_id,
            max_depth,
            "JOIN family_child  fc ON fc.person_id = step.person_id \
             JOIN family_spouse fs ON fs.family_id = fc.family_id",
            "fs.person_id",
        )
        .await
    }

    /// Every descendant of `person_id`, each at its shortest distance.
    ///
    /// Walks spouse → family → children, the mirror of [`ancestors`].
    #[tracing::instrument(name = "pedigree.descendants", skip_all, fields(max_depth))]
    pub async fn descendants(
        db: &impl ConnectionTrait,
        person_id: Uuid,
        max_depth: Option<i32>,
    ) -> Result<Vec<AncestryLink>, OxidGeneError> {
        Self::walk(
            db,
            person_id,
            max_depth,
            "JOIN family_spouse fs ON fs.person_id = step.person_id \
             JOIN family_child  fc ON fc.family_id = fs.family_id",
            "fc.person_id",
        )
        .await
    }

    /// Every spouse and child membership of a tree's active families, less
    /// the persons that were deleted.
    ///
    /// Soft-deleting a person keeps their family rows, so the join on
    /// `person` is what stops a deleted person from still linking two others.
    #[tracing::instrument(name = "pedigree.family_links", skip_all)]
    pub async fn family_links(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<FamilyLink>, OxidGeneError> {
        let backend = db.get_database_backend();
        let tree = match backend {
            DbBackend::Sqlite => "?1",
            _ => "$1",
        };
        let sql = format!(
            "SELECT fs.family_id, fs.person_id, fs.role, fs.sort_order \
             FROM family_spouse fs \
             JOIN family f ON f.id = fs.family_id \
             JOIN person p ON p.id = fs.person_id \
             WHERE f.tree_id = {tree} AND f.deleted_at IS NULL AND p.deleted_at IS NULL \
             UNION ALL \
             SELECT fc.family_id, fc.person_id, NULL, fc.sort_order \
             FROM family_child fc \
             JOIN family f ON f.id = fc.family_id \
             JOIN person p ON p.id = fc.person_id \
             WHERE f.tree_id = {tree} AND f.deleted_at IS NULL AND p.deleted_at IS NULL"
        );
        let rows = db
            .query_all_raw(Statement::from_sql_and_values(
                backend,
                &sql,
                [Value::from(tree_id)],
            ))
            .await
            .map_err(db_err)?;

        rows.iter()
            .map(|row| {
                let role = row.try_get::<Option<String>>("", "role").map_err(db_err)?;
                Ok(FamilyLink {
                    family_id: row.try_get("", "family_id").map_err(db_err)?,
                    person_id: row.try_get("", "person_id").map_err(db_err)?,
                    spouse_role: role.map(|role| match role.as_str() {
                        "husband" => SpouseRole::Husband,
                        "wife" => SpouseRole::Wife,
                        _ => SpouseRole::Partner,
                    }),
                    sort_order: row.try_get("", "sort_order").map_err(db_err)?,
                })
            })
            .collect()
    }

    /// Shared recursive walk; the two directions differ only in how one step
    /// joins through the family tables and which column it yields.
    async fn walk(
        db: &impl ConnectionTrait,
        person_id: Uuid,
        max_depth: Option<i32>,
        joins: &str,
        next_person: &str,
    ) -> Result<Vec<AncestryLink>, OxidGeneError> {
        let depth_limit = max_depth.unwrap_or(MAX_GENERATIONS).min(MAX_GENERATIONS);
        if depth_limit < 1 {
            return Ok(vec![]);
        }

        let backend = db.get_database_backend();
        let (root, limit) = match backend {
            DbBackend::Sqlite => ("?", "?"),
            _ => ("$1", "$2"),
        };

        // `UNION` (not UNION ALL) keeps the walk finite over the diamond
        // shapes that pedigree implex produces: a (person, depth) pair reached
        // by two different paths is only expanded once. MIN(depth) then
        // reports each person at their closest generation.
        let sql = format!(
            "WITH RECURSIVE step(person_id, depth) AS ( \
                 SELECT {root}, 0 \
                 UNION \
                 SELECT {next_person}, step.depth + 1 \
                 FROM step {joins} \
                 WHERE step.depth < {limit} \
             ) \
             SELECT person_id, MIN(depth) AS depth \
             FROM step \
             WHERE depth > 0 \
             GROUP BY person_id \
             ORDER BY depth, person_id"
        );

        let rows = db
            .query_all_raw(Statement::from_sql_and_values(
                backend,
                &sql,
                [Value::from(person_id), Value::from(depth_limit)],
            ))
            .await
            .map_err(db_err)?;

        rows.iter()
            .map(|row| {
                Ok(AncestryLink {
                    person_id: row.try_get::<Uuid>("", "person_id").map_err(db_err)?,
                    depth: row.try_get::<i32>("", "depth").map_err(db_err)?,
                })
            })
            .collect()
    }
}
