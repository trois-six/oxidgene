//! Repository for the `person_search_fts` search table (Sprint E.6).
//!
//! On SQLite the table is an FTS5 virtual table and matching uses `MATCH`
//! with per-word prefix queries (`"jean"* "dup"*`). On PostgreSQL the table
//! is a plain table and matching falls back to per-word `LIKE 'word%'`
//! conditions — prefix on both, so the same query gives the same answer
//! whichever backend is behind it.
//!
//! The named filters are substrings rather than prefixes, which is what lets
//! `surname=cruz` find "de la Cruz".
//!
//! All searchable columns (`surname`, `given_names`, `maiden_name`, and the
//! relatives' names) are pre-normalized (lowercase + accent-folded) by the
//! caller via [`oxidgene_core::search::normalize_for_search`]; queries are
//! normalized here, so both backends match identically.

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
    /// Precision of `birth_year` / `death_year` as the lowercase string form of
    /// `DateQualifier` (`exact`, `about`, `before`, …). Kept beside the year
    /// rather than folded into it so the UI can word it in its own language.
    pub birth_qualifier: String,
    pub death_qualifier: String,
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
    // ── Close relatives ──
    //
    // Denormalized onto the row so a result can name a spouse or the parents
    // without a second round trip. The `_display` columns are rendered as-is;
    // the normalized ones back the `spouse_*` / `father_*` / `mother_*`
    // filters, which are accent-folded exactly like the subject's own name.
    //
    // A person can have several spouses, so those columns hold every spouse
    // joined by [`RELATIVE_SEP`] — a separator no name contains, which also
    // stops a `LIKE '%…%'` from matching across two of them.
    /// Spouse display names, joined by [`RELATIVE_SEP`].
    pub spouse_names: String,
    /// Normalized spouse surnames, joined by [`RELATIVE_SEP`].
    pub spouse_surnames: String,
    /// Normalized spouse given names, joined by [`RELATIVE_SEP`].
    pub spouse_given_names: String,
    pub father_name: Option<String>,
    pub father_surname: Option<String>,
    pub father_given_names: Option<String>,
    pub mother_name: Option<String>,
    pub mother_surname: Option<String>,
    pub mother_given_names: Option<String>,
    /// Total children across every family where this person is a spouse.
    pub children_count: u32,
}

/// Joins several relatives into one column.
///
/// U+001F (unit separator) is a control character, so no name contains it and
/// no user can type it into a filter — a substring match therefore cannot span
/// two relatives.
pub const RELATIVE_SEP: char = '\u{1f}';

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
                       given_names_display, birth_place, date_sort, \
                       birth_qualifier, death_qualifier, \
                       spouse_names, spouse_surnames, spouse_given_names, \
                       father_name, father_surname, father_given_names, \
                       mother_name, mother_surname, mother_given_names, \
                       children_count";

/// Bind values per inserted row — one per column in [`COLUMNS`].
///
/// Derived rather than written out, so adding a column cannot leave the
/// placeholder list behind.
const COLUMN_COUNT: usize = 25;

// Keeps [`COLUMN_COUNT`] honest: counting the separators in [`COLUMNS`] must
// give the same answer, or the crate does not compile.
const _: () = {
    let bytes = COLUMNS.as_bytes();
    let mut i = 0;
    let mut columns = 1;
    while i < bytes.len() {
        if bytes[i] == b',' {
            columns += 1;
        }
        i += 1;
    }
    assert!(columns == COLUMN_COUNT);
};

/// Maximum rows per INSERT batch, kept well under the SQLite / PostgreSQL
/// parameter limits: 250 × [`COLUMN_COUNT`] bind values.
const INSERT_CHUNK: usize = 250;

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
                // Prefix, not substring: FTS5 above matches `"word"*`, and the
                // two backends have to answer the same question. Prefix is
                // also the indexable half of the choice, and the one a
                // typeahead wants.
                for word in words {
                    let param = push_value(&mut values, backend, format!("{word}%").into());
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

        // Relatives are matched on this row's own denormalized columns rather
        // than by joining back to `person_name`. That is what makes these
        // filters accent-folded like the subject's own name — SQL cannot fold
        // accents portably, so the folding has to happen where the row is
        // written — and it drops three correlated EXISTS subqueries.
        for (column, value) in [
            ("spouse_surnames", filters.spouse_surname.as_deref()),
            ("spouse_given_names", filters.spouse_given_names.as_deref()),
            ("father_surname", filters.father_surname.as_deref()),
            ("father_given_names", filters.father_given_names.as_deref()),
            ("mother_surname", filters.mother_surname.as_deref()),
            ("mother_given_names", filters.mother_given_names.as_deref()),
        ] {
            if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
                let param = push_value(
                    &mut values,
                    backend,
                    format!("%{}%", normalize_for_search(value.trim())).into(),
                );
                conditions.push(format!("COALESCE({column}, '') LIKE {param}"));
            }
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
            PersonSearchSort::Relevance => relevance_order(&mut values, backend, words, filters),
            PersonSearchSort::NameAsc => "surname, given_names".to_string(),
            PersonSearchSort::NameDesc => "surname DESC, given_names DESC".to_string(),
            PersonSearchSort::BirthAsc => {
                "date_sort IS NULL, date_sort, surname, given_names".to_string()
            }
            PersonSearchSort::BirthDesc => {
                "date_sort IS NULL, date_sort DESC, surname, given_names".to_string()
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
            let mut values: Vec<Value> = Vec::with_capacity(chunk.len() * COLUMN_COUNT);
            let mut rows = Vec::with_capacity(chunk.len());
            for entry in chunk {
                let base = values.len();
                let placeholders = (0..COLUMN_COUNT)
                    .map(|i| match backend {
                        DbBackend::Sqlite => "?".to_owned(),
                        _ => format!("${}", base + i + 1),
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                rows.push(format!("({placeholders})"));
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
                    Value::from(entry.birth_qualifier.clone()),
                    Value::from(entry.death_qualifier.clone()),
                    Value::from(entry.spouse_names.clone()),
                    Value::from(entry.spouse_surnames.clone()),
                    Value::from(entry.spouse_given_names.clone()),
                    Value::from(entry.father_name.clone()),
                    Value::from(entry.father_surname.clone()),
                    Value::from(entry.father_given_names.clone()),
                    Value::from(entry.mother_name.clone()),
                    Value::from(entry.mother_surname.clone()),
                    Value::from(entry.mother_given_names.clone()),
                    Value::from(entry.children_count.to_string()),
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
            birth_qualifier: get_string("birth_qualifier")?,
            death_qualifier: get_string("death_qualifier")?,
            spouse_names: get_string("spouse_names")?,
            spouse_surnames: get_string("spouse_surnames")?,
            spouse_given_names: get_string("spouse_given_names")?,
            father_name: get_opt("father_name")?,
            father_surname: get_opt("father_surname")?,
            father_given_names: get_opt("father_given_names")?,
            mother_name: get_opt("mother_name")?,
            mother_surname: get_opt("mother_surname")?,
            mother_given_names: get_opt("mother_given_names")?,
            children_count: get_opt("children_count")?
                .and_then(|n| n.parse().ok())
                .unwrap_or(0),
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

/// Ordering for [`PersonSearchSort::Relevance`]: what the searcher typed,
/// matched as a prefix, ranks above the same text found further in.
///
/// Deliberately not FTS5's `bm25()`. That only exists on SQLite and only when
/// the query carries a `MATCH`, so it would mean two rankings to keep in step
/// and no ranking at all for a search made of structured filters — which is
/// exactly what the search page sends. This expression is one code path, works
/// on both backends, and degrades to plain name order when there is nothing to
/// rank by.
fn relevance_order(
    values: &mut Vec<Value>,
    backend: DbBackend,
    words: &[String],
    filters: &PersonSearchFilters,
) -> String {
    let term = words.first().cloned().or_else(|| {
        [filters.surname.as_deref(), filters.given_names.as_deref()]
            .into_iter()
            .flatten()
            .map(str::trim)
            .find(|value| !value.is_empty())
            .map(normalize_for_search)
    });

    let Some(term) = term.filter(|term| !term.is_empty()) else {
        return "surname, given_names".to_string();
    };

    // Bound once per occurrence, not once per value: SQLite's `?` is
    // positional, so reusing one placeholder string in two spots would consume
    // two bind slots and shift every parameter after it.
    let prefix: Value = format!("{term}%").into();
    let surname_param = push_value(values, backend, prefix.clone());
    let given_names_param = push_value(values, backend, prefix);
    format!(
        "CASE WHEN surname LIKE {surname_param} THEN 0 \
              WHEN given_names LIKE {given_names_param} THEN 1 \
              ELSE 2 END, \
         surname, given_names"
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
