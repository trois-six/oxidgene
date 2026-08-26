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

use oxidgene_core::enums::{EventType, Sex};
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::search::normalize_for_search;
use sea_orm::{ConnectionTrait, DbBackend, Statement, Value};
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
    /// Original-cased surname, for rendering without re-splitting `display_name`.
    pub surname_display: String,
    /// Original-cased given names, for rendering without re-splitting `display_name`.
    pub given_names_display: String,
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

/// Structured filters applied before search pagination.
#[derive(Debug, Clone, Default)]
pub struct PersonSearchFilters {
    pub sex: Option<Sex>,
    pub surname: Option<String>,
    pub given_names: Option<String>,
    pub occupation: Option<String>,
    pub spouse_surname: Option<String>,
    pub spouse_given_names: Option<String>,
    pub father_surname: Option<String>,
    pub father_given_names: Option<String>,
    pub mother_surname: Option<String>,
    pub mother_given_names: Option<String>,
    pub birth_from: Option<i32>,
    pub birth_to: Option<i32>,
    pub death_from: Option<i32>,
    pub death_to: Option<i32>,
    pub place: Option<String>,
    pub event_type: Option<EventType>,
    pub event_from: Option<i32>,
    pub event_to: Option<i32>,
    pub has_media: bool,
}

/// Stable server-side ordering for person search.
#[derive(Debug, Clone, Copy, Default)]
pub enum PersonSearchSort {
    #[default]
    Relevance,
    NameAsc,
    NameDesc,
    BirthAsc,
    BirthDesc,
}

const COLUMNS: &str = "person_id, tree_id, surname, given_names, maiden_name, \
                       birth_year, death_year, sex, display_name, surname_display, \
                       given_names_display, birth_place, date_sort";

/// Maximum rows per INSERT batch (13 bind values per row, well under the
/// SQLite / PostgreSQL parameter limits).
const INSERT_CHUNK: usize = 500;

/// Repository for the DB-native person search table.
pub struct PersonSearchRepo;

impl PersonSearchRepo {
    /// Replace all search rows for a tree (used on full cache rebuild /
    /// GEDCOM import).
    pub async fn replace_tree(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        entries: &[PersonSearchEntry],
    ) -> Result<(), OxidGeneError> {
        Self::delete_tree(db, tree_id).await?;
        Self::insert_batch(db, entries).await
    }

    /// Insert or update search rows for a bounded set of persons (used after
    /// person / name / event mutations).
    pub async fn upsert(
        db: &impl ConnectionTrait,
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
        db: &impl ConnectionTrait,
        person_id: Uuid,
    ) -> Result<(), OxidGeneError> {
        Self::delete_persons(db, &[person_id]).await
    }

    /// Remove all search rows for a tree.
    pub async fn delete_tree(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<(), OxidGeneError> {
        let backend = db.get_database_backend();
        let sql = match backend {
            DbBackend::Sqlite => "DELETE FROM person_search_fts WHERE tree_id = ?",
            _ => "DELETE FROM person_search_fts WHERE tree_id = $1",
        };
        db.execute_raw(Statement::from_sql_and_values(
            backend,
            sql,
            [Value::from(tree_id.to_string())],
        ))
        .await
        .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(())
    }

    /// Count the search rows for a tree (used to detect a cold index).
    pub async fn count_tree(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<u64, OxidGeneError> {
        let backend = db.get_database_backend();
        let sql = match backend {
            DbBackend::Sqlite => "SELECT COUNT(*) AS cnt FROM person_search_fts WHERE tree_id = ?",
            _ => "SELECT COUNT(*) AS cnt FROM person_search_fts WHERE tree_id = $1",
        };
        let row = db
            .query_one_raw(Statement::from_sql_and_values(
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
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        query: &str,
        limit: u64,
        offset: u64,
    ) -> Result<PersonSearchPage, OxidGeneError> {
        Self::search_filtered(
            db,
            tree_id,
            query,
            &PersonSearchFilters::default(),
            PersonSearchSort::Relevance,
            limit,
            offset,
        )
        .await
    }

    /// Search persons with filters, sorting, and pagination applied in SQL.
    #[allow(clippy::too_many_arguments)]
    pub async fn search_filtered(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        query: &str,
        filters: &PersonSearchFilters,
        sort: PersonSearchSort,
        limit: u64,
        offset: u64,
    ) -> Result<PersonSearchPage, OxidGeneError> {
        let backend = db.get_database_backend();
        let words: Vec<String> = normalize_for_search(query)
            .split_whitespace()
            .map(str::to_owned)
            .collect();

        let stmt = Self::filtered_statement(backend, tree_id, &words, filters, sort, limit, offset);

        let rows = db
            .query_all_raw(stmt)
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

    #[allow(clippy::too_many_arguments)]
    fn filtered_statement(
        backend: DbBackend,
        tree_id: Uuid,
        words: &[String],
        filters: &PersonSearchFilters,
        sort: PersonSearchSort,
        limit: u64,
        offset: u64,
    ) -> Statement {
        let mut values = Vec::new();
        let mut conditions = Vec::new();

        conditions.push(format!(
            "tree_id = {}",
            push_value(&mut values, backend, tree_id.to_string().into())
        ));

        if !words.is_empty() {
            if backend == DbBackend::Sqlite {
                let match_expr = words
                    .iter()
                    .map(|word| format!("\"{}\"*", word.replace('"', "\"\"")))
                    .collect::<Vec<_>>()
                    .join(" ");
                conditions.push(format!(
                    "person_search_fts MATCH {}",
                    push_value(&mut values, backend, match_expr.into())
                ));
            } else {
                for word in words {
                    let param = push_value(&mut values, backend, format!("%{word}%").into());
                    conditions.push(format!(
                        "(surname LIKE {param} OR given_names LIKE {param} OR \
                         COALESCE(maiden_name, '') LIKE {param} OR \
                         COALESCE(birth_year, '') LIKE {param} OR \
                         COALESCE(death_year, '') LIKE {param})"
                    ));
                }
            }
        }

        if let Some(sex) = filters.sex {
            let param = push_value(&mut values, backend, sex.to_string().into());
            conditions.push(format!("sex = {param}"));
        }
        for (column, value) in [
            ("surname", filters.surname.as_deref()),
            ("given_names", filters.given_names.as_deref()),
        ] {
            if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
                let param = push_value(
                    &mut values,
                    backend,
                    format!("%{}%", normalize_for_search(value.trim())).into(),
                );
                conditions.push(format!("{column} LIKE {param}"));
            }
        }
        for (column, operator, year) in [
            ("birth_year", ">=", filters.birth_from),
            ("birth_year", "<=", filters.birth_to),
            ("death_year", ">=", filters.death_from),
            ("death_year", "<=", filters.death_to),
        ] {
            if let Some(year) = year {
                let param = push_value(&mut values, backend, i64::from(year).into());
                conditions.push(format!("CAST({column} AS INTEGER) {operator} {param}"));
            }
        }

        if filters
            .place
            .as_ref()
            .is_some_and(|place| !place.trim().is_empty())
            || filters.event_type.is_some()
            || filters.event_from.is_some()
            || filters.event_to.is_some()
        {
            let event_person_id = uuid_as_text(backend, "e.person_id");
            let spouse_person_id = uuid_as_text(backend, "fs.person_id");
            let mut event_conditions = vec![
                "e.deleted_at IS NULL".to_string(),
                format!(
                    "({event_person_id} = person_search_fts.person_id OR EXISTS (\
                    SELECT 1 FROM family_spouse fs WHERE fs.family_id = e.family_id \
                    AND {spouse_person_id} = person_search_fts.person_id))"
                ),
            ];
            if let Some(place) = filters
                .place
                .as_ref()
                .filter(|place| !place.trim().is_empty())
            {
                let param = push_value(
                    &mut values,
                    backend,
                    format!("%{}%", place.trim().to_lowercase()).into(),
                );
                event_conditions.push(format!("LOWER(p.name) LIKE {param}"));
            }
            if let Some(event_type) = filters.event_type {
                let param = push_value(&mut values, backend, event_type.to_string().into());
                event_conditions.push(format!("e.event_type = {param}"));
            }
            if let Some(year) = filters.event_from {
                let param = push_value(&mut values, backend, format!("{year:04}-01-01").into());
                event_conditions.push(format!("CAST(e.date_sort AS TEXT) >= {param}"));
            }
            if let Some(year) = filters.event_to {
                let param = push_value(&mut values, backend, format!("{year:04}-12-31").into());
                event_conditions.push(format!("CAST(e.date_sort AS TEXT) <= {param}"));
            }
            conditions.push(format!(
                "EXISTS (SELECT 1 FROM event e LEFT JOIN place p ON p.id = e.place_id WHERE {})",
                event_conditions.join(" AND ")
            ));
        }

        if let Some(occupation) = filters
            .occupation
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            let param = push_value(
                &mut values,
                backend,
                format!("%{}%", occupation.trim().to_lowercase()).into(),
            );
            let occupation_person_id = uuid_as_text(backend, "oe.person_id");
            conditions.push(format!(
                "EXISTS (SELECT 1 FROM event oe WHERE oe.deleted_at IS NULL \
                 AND oe.event_type = 'occupation' \
                 AND {occupation_person_id} = person_search_fts.person_id \
                 AND LOWER(COALESCE(oe.description, '')) LIKE {param})"
            ));
        }

        if has_name_filter(&filters.spouse_surname, &filters.spouse_given_names) {
            let person_relation = format!(
                "{} = person_search_fts.person_id",
                uuid_as_text(backend, "self_fs.person_id")
            );
            conditions.push(related_name_condition(
                &mut values,
                backend,
                "family_spouse self_fs JOIN family_spouse related_fs \
                 ON related_fs.family_id = self_fs.family_id AND related_fs.person_id <> self_fs.person_id \
                 JOIN person_name related_name ON related_name.person_id = related_fs.person_id",
                &person_relation,
                filters.spouse_surname.as_deref(),
                filters.spouse_given_names.as_deref(),
            ));
        }
        if has_name_filter(&filters.father_surname, &filters.father_given_names) {
            let person_relation = format!(
                "{} = person_search_fts.person_id AND parent.sex = 'male'",
                uuid_as_text(backend, "child_link.person_id")
            );
            conditions.push(related_name_condition(
                &mut values,
                backend,
                "family_child child_link JOIN family_spouse parent_link \
                 ON parent_link.family_id = child_link.family_id \
                 JOIN person parent ON parent.id = parent_link.person_id \
                 JOIN person_name related_name ON related_name.person_id = parent.id",
                &person_relation,
                filters.father_surname.as_deref(),
                filters.father_given_names.as_deref(),
            ));
        }
        if has_name_filter(&filters.mother_surname, &filters.mother_given_names) {
            let person_relation = format!(
                "{} = person_search_fts.person_id AND parent.sex = 'female'",
                uuid_as_text(backend, "child_link.person_id")
            );
            conditions.push(related_name_condition(
                &mut values,
                backend,
                "family_child child_link JOIN family_spouse parent_link \
                 ON parent_link.family_id = child_link.family_id \
                 JOIN person parent ON parent.id = parent_link.person_id \
                 JOIN person_name related_name ON related_name.person_id = parent.id",
                &person_relation,
                filters.mother_surname.as_deref(),
                filters.mother_given_names.as_deref(),
            ));
        }

        if filters.has_media {
            let media_person_id = uuid_as_text(backend, "ml.person_id");
            conditions.push(format!(
                "EXISTS (SELECT 1 FROM media_link ml JOIN media m ON m.id = ml.media_id \
                 WHERE {media_person_id} = person_search_fts.person_id \
                 AND m.deleted_at IS NULL)"
            ));
        }

        let order = match sort {
            PersonSearchSort::Relevance | PersonSearchSort::NameAsc => "surname, given_names",
            PersonSearchSort::NameDesc => "surname DESC, given_names DESC",
            PersonSearchSort::BirthAsc => "date_sort IS NULL, date_sort, surname, given_names",
            PersonSearchSort::BirthDesc => {
                "date_sort IS NULL, date_sort DESC, surname, given_names"
            }
        };
        let limit_param = push_value(&mut values, backend, (limit as i64).into());
        let offset_param = push_value(&mut values, backend, (offset as i64).into());
        let sql = format!(
            "SELECT {COLUMNS}, COUNT(*) OVER () AS total_count FROM person_search_fts \
             WHERE {} ORDER BY {order} LIMIT {limit_param} OFFSET {offset_param}",
            conditions.join(" AND ")
        );
        Statement::from_sql_and_values(backend, sql, values)
    }

    // ── Internals ───────────────────────────────────────────────────────

    async fn delete_persons(
        db: &impl ConnectionTrait,
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
        db.execute_raw(Statement::from_sql_and_values(backend, sql, values))
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(())
    }

    async fn insert_batch(
        db: &impl ConnectionTrait,
        entries: &[PersonSearchEntry],
    ) -> Result<(), OxidGeneError> {
        if entries.is_empty() {
            return Ok(());
        }
        let backend = db.get_database_backend();

        for chunk in entries.chunks(INSERT_CHUNK) {
            let mut values: Vec<Value> = Vec::with_capacity(chunk.len() * 13);
            let mut rows = Vec::with_capacity(chunk.len());
            for entry in chunk {
                let base = values.len();
                let row = match backend {
                    DbBackend::Sqlite => "(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)".to_owned(),
                    _ => format!(
                        "(${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${})",
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
                        base + 11,
                        base + 12,
                        base + 13
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
                    Value::from(entry.surname_display.clone()),
                    Value::from(entry.given_names_display.clone()),
                    Value::from(entry.birth_place.clone()),
                    Value::from(entry.date_sort.clone()),
                ]);
            }
            let sql = format!(
                "INSERT INTO person_search_fts ({COLUMNS}) VALUES {}",
                rows.join(", ")
            );
            db.execute_raw(Statement::from_sql_and_values(backend, sql, values))
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
            surname_display: get_string("surname_display")?,
            given_names_display: get_string("given_names_display")?,
            birth_place: get_opt("birth_place")?,
            date_sort: get_opt("date_sort")?,
        })
    }
}

fn push_value(values: &mut Vec<Value>, backend: DbBackend, value: Value) -> String {
    values.push(value);
    match backend {
        DbBackend::Postgres => format!("${}", values.len()),
        _ => "?".to_string(),
    }
}

fn has_name_filter(surname: &Option<String>, given_names: &Option<String>) -> bool {
    [surname, given_names]
        .into_iter()
        .flatten()
        .any(|value| !value.trim().is_empty())
}

fn related_name_condition(
    values: &mut Vec<Value>,
    backend: DbBackend,
    joins: &str,
    relation: &str,
    surname: Option<&str>,
    given_names: Option<&str>,
) -> String {
    let mut conditions = vec![
        relation.to_string(),
        "related_name.is_primary = TRUE".to_string(),
    ];
    for (column, value) in [("surname", surname), ("given_names", given_names)] {
        if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
            let param = push_value(
                values,
                backend,
                format!("%{}%", value.trim().to_lowercase()).into(),
            );
            conditions.push(format!(
                "LOWER(COALESCE(related_name.{column}, '')) LIKE {param}"
            ));
        }
    }
    format!(
        "EXISTS (SELECT 1 FROM {joins} WHERE {})",
        conditions.join(" AND ")
    )
}

fn uuid_as_text(backend: DbBackend, column: &str) -> String {
    match backend {
        DbBackend::Sqlite => format!(
            "LOWER(SUBSTR(HEX({column}), 1, 8) || '-' || \
             SUBSTR(HEX({column}), 9, 4) || '-' || \
             SUBSTR(HEX({column}), 13, 4) || '-' || \
             SUBSTR(HEX({column}), 17, 4) || '-' || \
             SUBSTR(HEX({column}), 21, 12))"
        ),
        _ => format!("CAST({column} AS TEXT)"),
    }
}
