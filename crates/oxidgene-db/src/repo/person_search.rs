//! Repository for the `person_search_fts` search table (Sprint E.6).
//!
//! On SQLite the table is an FTS5 virtual table and matching uses `MATCH`
//! with per-word prefix queries (`"jean"* "dup"*`). On PostgreSQL the table
//! is a plain table and matching falls back to per-word `LIKE` conditions.
//!
//! All searchable columns (`surname`, `given_names`, `maiden_name`) are
//! pre-normalized (lowercase + accent-folded) by the caller via
//! [`oxidgene_core::search::normalize_for_search`]; queries are normalized
//! here, so both backends match identically.

use oxidgene_core::error::OxidGeneError;
use oxidgene_core::search::normalize_for_search;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, Value};
use uuid::Uuid;

/// A row of the `person_search_fts` table.
///
/// Doubles as the write model (built from cache data) and the search hit
/// returned by [`PersonSearchRepo::search`].
#[derive(Debug, Clone)]
pub struct PersonSearchEntry {
    pub person_id: Uuid,
    pub tree_id: Uuid,
    /// Normalized primary surname (lowercase, accent-folded).
    pub surname: String,
    /// Normalized given names (lowercase, accent-folded).
    pub given_names: String,
    /// Normalized maiden name, if any.
    pub maiden_name: Option<String>,
    pub birth_year: Option<String>,
    pub death_year: Option<String>,
    /// Sex as its lowercase string form (`male` / `female` / `unknown`).
    pub sex: String,
    /// Display name with original casing, for rendering results.
    pub display_name: String,
    pub birth_place: Option<String>,
    /// ISO date (`YYYY-MM-DD`) used for sorting, if known.
    pub date_sort: Option<String>,
}

/// Paginated search hits plus the total match count.
#[derive(Debug, Clone)]
pub struct PersonSearchPage {
    pub entries: Vec<PersonSearchEntry>,
    pub total_count: u64,
}

const COLUMNS: &str = "person_id, tree_id, surname, given_names, maiden_name, \
                       birth_year, death_year, sex, display_name, birth_place, date_sort";

/// Maximum rows per INSERT batch (11 bind values per row, well under the
/// SQLite / PostgreSQL parameter limits).
const INSERT_CHUNK: usize = 500;

/// Repository for the DB-native person search table.
pub struct PersonSearchRepo;

impl PersonSearchRepo {
    /// Replace all search rows for a tree (used on full cache rebuild /
    /// GEDCOM import).
    pub async fn replace_tree(
        db: &DatabaseConnection,
        tree_id: Uuid,
        entries: &[PersonSearchEntry],
    ) -> Result<(), OxidGeneError> {
        Self::delete_tree(db, tree_id).await?;
        Self::insert_batch(db, entries).await
    }

    /// Insert or update search rows for a bounded set of persons (used after
    /// person / name / event mutations).
    pub async fn upsert(
        db: &DatabaseConnection,
        entries: &[PersonSearchEntry],
    ) -> Result<(), OxidGeneError> {
        if entries.is_empty() {
            return Ok(());
        }
        let ids: Vec<Uuid> = entries.iter().map(|e| e.person_id).collect();
        Self::delete_persons(db, &ids).await?;
        Self::insert_batch(db, entries).await
    }

    /// Remove the search row for a single person.
    pub async fn delete_person(
        db: &DatabaseConnection,
        person_id: Uuid,
    ) -> Result<(), OxidGeneError> {
        Self::delete_persons(db, &[person_id]).await
    }

    /// Remove all search rows for a tree.
    pub async fn delete_tree(db: &DatabaseConnection, tree_id: Uuid) -> Result<(), OxidGeneError> {
        let backend = db.get_database_backend();
        let sql = match backend {
            DbBackend::Sqlite => "DELETE FROM person_search_fts WHERE tree_id = ?",
            _ => "DELETE FROM person_search_fts WHERE tree_id = $1",
        };
        db.execute(Statement::from_sql_and_values(
            backend,
            sql,
            [Value::from(tree_id.to_string())],
        ))
        .await
        .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(())
    }

    /// Count the search rows for a tree (used to detect a cold index).
    pub async fn count_tree(db: &DatabaseConnection, tree_id: Uuid) -> Result<u64, OxidGeneError> {
        let backend = db.get_database_backend();
        let sql = match backend {
            DbBackend::Sqlite => "SELECT COUNT(*) AS cnt FROM person_search_fts WHERE tree_id = ?",
            _ => "SELECT COUNT(*) AS cnt FROM person_search_fts WHERE tree_id = $1",
        };
        let row = db
            .query_one(Statement::from_sql_and_values(
                backend,
                sql,
                [Value::from(tree_id.to_string())],
            ))
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        let count: i64 = row
            .map(|r| r.try_get("", "cnt"))
            .transpose()
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .unwrap_or(0);
        Ok(count.max(0) as u64)
    }

    /// Search persons in a tree.
    ///
    /// The raw `query` is normalized (lowercase + accent folding) and split
    /// into words; every word must match. On SQLite each word is an FTS5
    /// prefix query (`"word"*`); on PostgreSQL each word is a `LIKE '%word%'`
    /// condition across the searchable columns. An empty query returns all
    /// persons in the tree (browse mode), sorted by name.
    pub async fn search(
        db: &DatabaseConnection,
        tree_id: Uuid,
        query: &str,
        limit: u64,
        offset: u64,
    ) -> Result<PersonSearchPage, OxidGeneError> {
        let backend = db.get_database_backend();
        let words: Vec<String> = normalize_for_search(query)
            .split_whitespace()
            .map(str::to_owned)
            .collect();

        let stmt = if words.is_empty() {
            Self::browse_statement(backend, tree_id, limit, offset)
        } else {
            match backend {
                DbBackend::Sqlite => Self::fts_statement(tree_id, &words, limit, offset),
                _ => Self::like_statement(backend, tree_id, &words, limit, offset),
            }
        };

        let rows = db
            .query_all(stmt)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;

        let mut total_count: u64 = 0;
        let mut entries = Vec::with_capacity(rows.len());
        for row in rows {
            let total: i64 = row
                .try_get("", "total_count")
                .map_err(|e| OxidGeneError::Database(e.to_string()))?;
            total_count = total.max(0) as u64;
            entries.push(Self::row_to_entry(&row)?);
        }

        Ok(PersonSearchPage {
            entries,
            total_count,
        })
    }

    // ── Statement builders ──────────────────────────────────────────────

    fn browse_statement(backend: DbBackend, tree_id: Uuid, limit: u64, offset: u64) -> Statement {
        let sql = match backend {
            DbBackend::Sqlite => format!(
                "SELECT {COLUMNS}, COUNT(*) OVER () AS total_count \
                 FROM person_search_fts WHERE tree_id = ? \
                 ORDER BY surname, given_names LIMIT ? OFFSET ?"
            ),
            _ => format!(
                "SELECT {COLUMNS}, COUNT(*) OVER () AS total_count \
                 FROM person_search_fts WHERE tree_id = $1 \
                 ORDER BY surname, given_names LIMIT $2 OFFSET $3"
            ),
        };
        Statement::from_sql_and_values(
            backend,
            sql,
            [
                Value::from(tree_id.to_string()),
                Value::from(limit as i64),
                Value::from(offset as i64),
            ],
        )
    }

    /// SQLite FTS5: match every word as a prefix query across the indexed
    /// columns (surname, given_names, maiden_name, birth_year, death_year).
    fn fts_statement(tree_id: Uuid, words: &[String], limit: u64, offset: u64) -> Statement {
        let match_expr = words
            .iter()
            .map(|w| format!("\"{}\"*", w.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" ");
        let sql = format!(
            "SELECT {COLUMNS}, COUNT(*) OVER () AS total_count \
             FROM person_search_fts \
             WHERE person_search_fts MATCH ? AND tree_id = ? \
             ORDER BY surname, given_names LIMIT ? OFFSET ?"
        );
        Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [
                Value::from(match_expr),
                Value::from(tree_id.to_string()),
                Value::from(limit as i64),
                Value::from(offset as i64),
            ],
        )
    }

    /// PostgreSQL fallback: every word must appear (substring) in one of the
    /// searchable columns.
    fn like_statement(
        backend: DbBackend,
        tree_id: Uuid,
        words: &[String],
        limit: u64,
        offset: u64,
    ) -> Statement {
        let mut values: Vec<Value> = vec![Value::from(tree_id.to_string())];
        let mut conditions = Vec::with_capacity(words.len());
        for word in words {
            let idx = values.len() + 1;
            conditions.push(format!(
                "(surname LIKE ${idx} OR given_names LIKE ${idx} \
                 OR COALESCE(maiden_name, '') LIKE ${idx} \
                 OR COALESCE(birth_year, '') LIKE ${idx} \
                 OR COALESCE(death_year, '') LIKE ${idx})"
            ));
            values.push(Value::from(format!("%{word}%")));
        }
        let limit_idx = values.len() + 1;
        let offset_idx = values.len() + 2;
        values.push(Value::from(limit as i64));
        values.push(Value::from(offset as i64));

        let sql = format!(
            "SELECT {COLUMNS}, COUNT(*) OVER () AS total_count \
             FROM person_search_fts \
             WHERE tree_id = $1 AND {} \
             ORDER BY surname, given_names LIMIT ${limit_idx} OFFSET ${offset_idx}",
            conditions.join(" AND ")
        );
        Statement::from_sql_and_values(backend, sql, values)
    }

    // ── Internals ───────────────────────────────────────────────────────

    async fn delete_persons(
        db: &DatabaseConnection,
        person_ids: &[Uuid],
    ) -> Result<(), OxidGeneError> {
        if person_ids.is_empty() {
            return Ok(());
        }
        let backend = db.get_database_backend();
        let placeholders: Vec<String> = (0..person_ids.len())
            .map(|i| match backend {
                DbBackend::Sqlite => "?".to_owned(),
                _ => format!("${}", i + 1),
            })
            .collect();
        let sql = format!(
            "DELETE FROM person_search_fts WHERE person_id IN ({})",
            placeholders.join(", ")
        );
        let values: Vec<Value> = person_ids
            .iter()
            .map(|id| Value::from(id.to_string()))
            .collect();
        db.execute(Statement::from_sql_and_values(backend, sql, values))
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(())
    }

    async fn insert_batch(
        db: &DatabaseConnection,
        entries: &[PersonSearchEntry],
    ) -> Result<(), OxidGeneError> {
        if entries.is_empty() {
            return Ok(());
        }
        let backend = db.get_database_backend();

        for chunk in entries.chunks(INSERT_CHUNK) {
            let mut values: Vec<Value> = Vec::with_capacity(chunk.len() * 11);
            let mut rows = Vec::with_capacity(chunk.len());
            for entry in chunk {
                let base = values.len();
                let row = match backend {
                    DbBackend::Sqlite => "(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)".to_owned(),
                    _ => format!(
                        "(${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${})",
                        base + 1,
                        base + 2,
                        base + 3,
                        base + 4,
                        base + 5,
                        base + 6,
                        base + 7,
                        base + 8,
                        base + 9,
                        base + 10,
                        base + 11
                    ),
                };
                rows.push(row);
                values.extend([
                    Value::from(entry.person_id.to_string()),
                    Value::from(entry.tree_id.to_string()),
                    Value::from(entry.surname.clone()),
                    Value::from(entry.given_names.clone()),
                    Value::from(entry.maiden_name.clone()),
                    Value::from(entry.birth_year.clone()),
                    Value::from(entry.death_year.clone()),
                    Value::from(entry.sex.clone()),
                    Value::from(entry.display_name.clone()),
                    Value::from(entry.birth_place.clone()),
                    Value::from(entry.date_sort.clone()),
                ]);
            }
            let sql = format!(
                "INSERT INTO person_search_fts ({COLUMNS}) VALUES {}",
                rows.join(", ")
            );
            db.execute(Statement::from_sql_and_values(backend, sql, values))
                .await
                .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        }
        Ok(())
    }

    fn row_to_entry(row: &sea_orm::QueryResult) -> Result<PersonSearchEntry, OxidGeneError> {
        let get_string = |col: &str| -> Result<String, OxidGeneError> {
            row.try_get::<String>("", col)
                .map_err(|e| OxidGeneError::Database(e.to_string()))
        };
        let get_opt = |col: &str| -> Result<Option<String>, OxidGeneError> {
            row.try_get::<Option<String>>("", col)
                .map_err(|e| OxidGeneError::Database(e.to_string()))
        };
        let parse_uuid = |s: String| -> Result<Uuid, OxidGeneError> {
            Uuid::parse_str(&s).map_err(|e| OxidGeneError::Database(e.to_string()))
        };

        Ok(PersonSearchEntry {
            person_id: parse_uuid(get_string("person_id")?)?,
            tree_id: parse_uuid(get_string("tree_id")?)?,
            surname: get_string("surname")?,
            given_names: get_string("given_names")?,
            maiden_name: get_opt("maiden_name")?,
            birth_year: get_opt("birth_year")?,
            death_year: get_opt("death_year")?,
            sex: get_string("sex")?,
            display_name: get_string("display_name")?,
            birth_place: get_opt("birth_place")?,
            date_sort: get_opt("date_sort")?,
        })
    }
}
