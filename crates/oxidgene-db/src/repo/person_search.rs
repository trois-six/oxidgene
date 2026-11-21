//! Repository for the person search: the typed `person_search` rows and
//! the words of `person_search_token` (docs/data-model.md §4.3).
//!
//! Every name a search matches is folded in Rust
//! ([`oxidgene_core::search::fold_words`]) before it is written, and every
//! query the same way before it is matched, so the database compares bytes
//! only. A person's words — the folded words of their own names and birth
//! and death years, and those of their close relatives' names — are rows of
//! `person_search_token`, keyed `(tree_id, token, person_id)`, each with the
//! set of fields ([`SearchField`]) that hold it. A typed word matches the
//! words it begins: `token >= 'abc' AND token < 'abd'`, one range of that
//! key. A search drives from those ranges, one per word, intersected, and
//! reads the rows of the persons they find by primary key; a search without
//! a word reads the tree's rows through their own indexes.
//!
//! The statements are built once, with SeaQuery, and are the same on
//! SQLite and PostgreSQL: the backend only decides how SeaORM writes their
//! placeholders, and nothing here branches on it.

use std::collections::BTreeMap;

use chrono::{Datelike as _, NaiveDate};
use oxidgene_core::enums::{EventType, Sex};
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::search::fold_words;
use sea_orm::sea_query::{Expr, ExprTrait, Func, LikeExpr, Query, SelectStatement, UnionType};
use sea_orm::{
    ColumnTrait, Condition, ConnectionTrait, DbBackend, EntityTrait, FromQueryResult as _, Order,
    QueryFilter, QueryOrder, QuerySelect, QueryTrait as _, Statement,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::person_search::{self as row, Column as RowColumn};
use crate::entities::person_search_token::{self as token, Column as TokenColumn};
use crate::entities::{event, family_spouse, media, media_link, place, record_tag, sea_enums};
use crate::repo::batch::MAX_BOUND_IDS;
use crate::repo::{db_err, escape_like};

/// A row of `person_search`: what a search returns.
pub type PersonSearchRow = row::Model;

/// What [`PersonSearchRepo`] writes for one person: their row, and the
/// folded names of their close relatives, which are written as words only.
#[derive(Debug, Clone)]
pub struct PersonSearchEntry {
    pub row: PersonSearchRow,
    pub relatives: RelativeNames,
}

/// The folded names of a person's close relatives, which the `spouse_*`,
/// `father_*` and `mother_*` filters match.
#[derive(Debug, Clone, Default)]
pub struct RelativeNames {
    /// One per spouse.
    pub spouse_surnames: Vec<String>,
    /// One per spouse.
    pub spouse_given_names: Vec<String>,
    pub father_surname: Option<String>,
    pub father_given_names: Option<String>,
    pub mother_surname: Option<String>,
    pub mother_given_names: Option<String>,
}

/// Joins several relatives' display names into the one `spouse_names`
/// column.
///
/// U+001F (unit separator) is a control character, so no name contains it
/// and the column splits back into the names it joined.
pub const RELATIVE_SEP: char = '\u{1f}';

/// A field of a person whose words a search matches: the bit it sets in
/// a word's `fields`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum SearchField {
    Surname = 1,
    GivenNames = 1 << 1,
    MaidenName = 1 << 2,
    BirthYear = 1 << 3,
    DeathYear = 1 << 4,
    SpouseSurname = 1 << 5,
    SpouseGivenNames = 1 << 6,
    FatherSurname = 1 << 7,
    FatherGivenNames = 1 << 8,
    MotherSurname = 1 << 9,
    MotherGivenNames = 1 << 10,
}

impl SearchField {
    /// The fields of the person themselves, which the free text matches:
    /// a search for a name returns that person, not everyone related to
    /// someone of that name.
    const OWN: i32 = Self::Surname as i32
        | Self::GivenNames as i32
        | Self::MaidenName as i32
        | Self::BirthYear as i32
        | Self::DeathYear as i32;
}

/// The two statements of a search page: what
/// [`PersonSearchRepo::search_statements`] builds.
#[derive(Debug, Clone)]
pub struct SearchStatements {
    /// The number of matches, as `total`.
    pub count: Statement,
    /// The page of matching rows, in order; with words to match, each row
    /// also carries the number of matches, as `total`.
    pub page: Statement,
    /// Whether [`Self::page`] counts the matches, so that [`Self::count`]
    /// runs only for a page past the last match.
    pub page_counts: bool,
}

/// Paginated search hits plus the total match count.
#[derive(Debug, Clone)]
pub struct PersonSearchPage {
    pub entries: Vec<PersonSearchRow>,
    pub total_count: u64,
}

/// Structured filters applied before search pagination.
///
/// Deserializable so that every API surface that receives the filters as
/// named fields — REST query strings, and anything else speaking JSON — reads
/// them straight into this one type instead of restating the field list. An
/// absent field is no constraint.
///
/// A name filter matches as the free text does: every word typed must begin
/// a word of that name, case and accents ignored, in any order — `cruz`
/// finds "de la Cruz", `sophie` finds "Anne-Sophie".
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct PersonSearchFilters {
    pub sex: Option<Sex>,
    /// Words beginning words of the primary surname, case- and
    /// accent-insensitive.
    pub surname: Option<String>,
    /// Words beginning words of the primary given names, case- and
    /// accent-insensitive.
    pub given_names: Option<String>,
    /// Substring of an occupation label.
    pub occupation: Option<String>,
    /// Words beginning words of a spouse's surname.
    pub spouse_surname: Option<String>,
    /// Words beginning words of a spouse's given names.
    pub spouse_given_names: Option<String>,
    /// Words beginning words of the father's surname.
    pub father_surname: Option<String>,
    /// Words beginning words of the father's given names.
    pub father_given_names: Option<String>,
    /// Words beginning words of the mother's surname.
    pub mother_surname: Option<String>,
    /// Words beginning words of the mother's given names.
    pub mother_given_names: Option<String>,
    /// Earliest birth year, inclusive; a birth may fall back to the baptism.
    pub birth_from: Option<i32>,
    /// Latest birth year, inclusive.
    pub birth_to: Option<i32>,
    /// Earliest death year, inclusive; a death may fall back to the burial.
    pub death_from: Option<i32>,
    /// Latest death year, inclusive.
    pub death_to: Option<i32>,
    /// Substring of the name of a place where the person has an event.
    pub place: Option<String>,
    /// Only persons with an event of this type.
    pub event_type: Option<EventType>,
    /// Earliest year of that event, inclusive.
    pub event_from: Option<i32>,
    /// Latest year of that event, inclusive.
    pub event_to: Option<i32>,
    /// Only persons with at least one attached media.
    pub has_media: bool,
    /// Only persons carrying every one of these tags, by id.
    pub tags: Vec<Uuid>,
    /// The rules hiding persons from the viewer — the owner's exclusions,
    /// the access scope (docs/authorization.md §6.5) and the privacy rules —
    /// whose persons are left out of the matches and the count
    /// (docs/privacy.md §7). Set by the server, never read from a request.
    #[serde(skip)]
    #[cfg_attr(feature = "schema", schemars(skip))]
    pub unseen: oxidgene_core::redaction::HidingRules,
}

/// Stable server-side ordering for person search.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum PersonSearchSort {
    #[default]
    Relevance,
    NameAsc,
    NameDesc,
    BirthAsc,
    BirthDesc,
}

/// Rows per `INSERT` of search rows: 250 × 19 bound values, well under
/// SQLite's and PostgreSQL's limits.
const ROW_CHUNK: usize = 250;

/// Words per `INSERT` of words: 2,000 × 4 bound values.
const TOKEN_CHUNK: usize = 2_000;

/// Repository for the person search's rows and words.
pub struct PersonSearchRepo;

impl PersonSearchRepo {
    /// Replace all search rows for a tree.
    pub async fn replace_tree(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        entries: &[PersonSearchEntry],
    ) -> Result<(), OxidGeneError> {
        Self::delete_tree(db, tree_id).await?;
        Self::insert(db, entries).await
    }

    /// Insert or replace the search rows of a set of persons (used after
    /// person / name / event mutations, and slice by slice by a rebuild).
    pub async fn upsert(
        db: &impl ConnectionTrait,
        entries: &[PersonSearchEntry],
    ) -> Result<(), OxidGeneError> {
        if entries.is_empty() {
            return Ok(());
        }
        let ids: Vec<Uuid> = entries.iter().map(|e| e.row.person_id).collect();
        Self::delete_persons(db, &ids).await?;
        Self::insert(db, entries).await
    }

    /// Remove the search row of a single person, and their words.
    pub async fn delete_person(
        db: &impl ConnectionTrait,
        person_id: Uuid,
    ) -> Result<(), OxidGeneError> {
        Self::delete_persons(db, &[person_id]).await
    }

    /// Remove all search rows of a tree, and their words: one range of
    /// each table's tree index.
    pub async fn delete_tree(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<(), OxidGeneError> {
        token::Entity::delete_many()
            .filter(TokenColumn::TreeId.eq(tree_id))
            .exec(db)
            .await
            .map_err(db_err)?;
        row::Entity::delete_many()
            .filter(RowColumn::TreeId.eq(tree_id))
            .exec(db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    /// Whether a tree has any search row (a cold index has none). Every tree
    /// read asks it, so it is one index probe.
    pub async fn has_tree(db: &impl ConnectionTrait, tree_id: Uuid) -> Result<bool, OxidGeneError> {
        row::Entity::find()
            .select_only()
            .column(RowColumn::PersonId)
            .filter(RowColumn::TreeId.eq(tree_id))
            .limit(1)
            .into_tuple::<Uuid>()
            .one(db)
            .await
            .map(|found| found.is_some())
            .map_err(db_err)
    }

    /// Count the search rows for a tree, from its name index alone.
    pub async fn count_tree(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<u64, OxidGeneError> {
        count(db, Condition::all().add(RowColumn::TreeId.eq(tree_id))).await
    }

    /// Search persons in a tree by free text alone, ranked by relevance.
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

    /// Search persons with filters, sorting and pagination applied in SQL.
    ///
    /// Every word of `query`, folded, must begin a word of the person's own
    /// names or years; an empty query with no filter lists the whole tree
    /// (browse mode). The matches are counted first, then the page is read:
    /// a page past the last match costs no second statement and still
    /// reports how many there are.
    pub async fn search_filtered(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        query: &str,
        filters: &PersonSearchFilters,
        sort: PersonSearchSort,
        limit: u64,
        offset: u64,
    ) -> Result<PersonSearchPage, OxidGeneError> {
        let statements = Self::search_statements(
            db.get_database_backend(),
            tree_id,
            query,
            filters,
            sort,
            limit,
            offset,
        );
        let count = |statement| async move {
            match db.query_one_raw(statement).await.map_err(db_err)? {
                Some(row) => total_of(&row),
                None => Ok(0),
            }
        };
        let mut total_count = None;
        if !statements.page_counts {
            let total = count(statements.count.clone()).await?;
            if total <= offset || limit == 0 {
                return Ok(PersonSearchPage {
                    entries: Vec::new(),
                    total_count: total,
                });
            }
            total_count = Some(total);
        }
        let rows = db.query_all_raw(statements.page).await.map_err(db_err)?;
        let entries: Vec<PersonSearchRow> = rows
            .iter()
            .map(|row| PersonSearchRow::from_query_result(row, "").map_err(db_err))
            .collect::<Result<_, _>>()?;
        let total_count = match (total_count, rows.first()) {
            (Some(total), _) => total,
            (None, Some(row)) => total_of(row)?,
            // A page past the last match brings no row to read the count
            // from: the matches are counted on their own.
            (None, None) if offset > 0 => count(statements.count).await?,
            (None, None) => 0,
        };
        Ok(PersonSearchPage {
            entries,
            total_count,
        })
    }

    /// The statements [`Self::search_filtered`] runs for the same
    /// arguments: the count of the matches, and the page. The backend only
    /// renders their placeholders; the plan checks read them
    /// (docs/development.md §2.8).
    ///
    /// A search with words reads every match to put them in order, so its
    /// page counts them as it goes (`COUNT(*) OVER ()`) and needs no second
    /// statement. A search without reads a page of an index in order and
    /// counts from the index alone: a window over its page would read the
    /// whole tree.
    pub fn search_statements(
        backend: DbBackend,
        tree_id: Uuid,
        query: &str,
        filters: &PersonSearchFilters,
        sort: PersonSearchSort,
        limit: u64,
        offset: u64,
    ) -> SearchStatements {
        let words = words_of(query);
        let page_counts = !matched_words(&words, filters).is_empty();
        let conditions = filtered_conditions(tree_id, &words, filters);
        let mut page = row::Entity::find();
        if page_counts {
            page = page.expr_as(Expr::cust("COUNT(*) OVER ()"), "total");
        }
        SearchStatements {
            count: counted(conditions.clone()).build(backend),
            page: sorted(page.filter(conditions), &words, filters, sort)
                .limit(limit)
                // More than SQL can skip is past every match.
                .offset(bounded(offset))
                .build(backend),
            page_counts,
        }
    }

    /// Other persons of the tree bearing the same name as `person_id`.
    ///
    /// Same name means the same primary surname *and* the same primary given
    /// names once folded (lowercase, accents removed) — what the search row
    /// already stores. A person missing either half has no homonym: half a
    /// name says too little to ask whether two records are one individual.
    /// Ordered by birth, then name, so the likeliest candidates of a long list
    /// come first.
    pub async fn homonyms(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        person_id: Uuid,
        limit: u64,
    ) -> Result<Vec<PersonSearchRow>, OxidGeneError> {
        let Some((surname, given_names)) = row::Entity::find_by_id(person_id)
            .select_only()
            .columns([RowColumn::SurnameKey, RowColumn::GivenKey])
            .into_tuple::<(String, String)>()
            .one(db)
            .await
            .map_err(db_err)?
        else {
            return Ok(Vec::new());
        };
        if surname.trim().is_empty() || given_names.trim().is_empty() {
            return Ok(Vec::new());
        }
        row::Entity::find()
            .filter(RowColumn::TreeId.eq(tree_id))
            .filter(RowColumn::SurnameKey.eq(surname))
            .filter(RowColumn::GivenKey.eq(given_names))
            .filter(RowColumn::PersonId.ne(person_id))
            .order_by(Expr::col(RowColumn::DateSort).is_null(), Order::Asc)
            .order_by_asc(RowColumn::DateSort)
            .order_by_asc(RowColumn::DisplayName)
            .limit(limit)
            .all(db)
            .await
            .map_err(db_err)
    }

    /// The primary `(person, surname, given names)` of each person the
    /// `surname` and `given_names` filters of [`Self::search_filtered`]
    /// find, as written: one per person, matched exactly as those filters
    /// match.
    pub async fn primary_names(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        surname: Option<&str>,
        given_names: Option<&str>,
    ) -> Result<Vec<(Uuid, String, String)>, OxidGeneError> {
        let filters = PersonSearchFilters {
            surname: surname.map(str::to_owned),
            given_names: given_names.map(str::to_owned),
            ..PersonSearchFilters::default()
        };
        row::Entity::find()
            .select_only()
            .columns([
                RowColumn::PersonId,
                RowColumn::SurnameDisplay,
                RowColumn::GivenNamesDisplay,
            ])
            .filter(filtered_conditions(tree_id, &[], &filters))
            .into_tuple()
            .all(db)
            .await
            .map_err(db_err)
    }

    /// The ids of every person `search_filtered` would return for the same
    /// arguments, unpaged, up to `limit` of them. A caller asking for `limit`
    /// + 1 learns whether the search is larger than it may take.
    pub async fn search_ids(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        query: &str,
        filters: &PersonSearchFilters,
        limit: u64,
    ) -> Result<Vec<Uuid>, OxidGeneError> {
        row::Entity::find()
            .select_only()
            .column(RowColumn::PersonId)
            .filter(filtered_conditions(tree_id, &words_of(query), filters))
            .limit(limit)
            .into_tuple()
            .all(db)
            .await
            .map_err(db_err)
    }

    // ── Internals ───────────────────────────────────────────────────────

    /// Delete the rows of `person_ids` and their words, a set with no upper
    /// bound: a rename reaches every carrier and their relatives. One
    /// statement per table and [`MAX_BOUND_IDS`] of them, under any
    /// backend's bound-variable limit. The words go first, through their
    /// person index; the foreign key would cascade to them, but only where
    /// SQLite enforces foreign keys.
    async fn delete_persons(
        db: &impl ConnectionTrait,
        person_ids: &[Uuid],
    ) -> Result<(), OxidGeneError> {
        for chunk in person_ids.chunks(MAX_BOUND_IDS) {
            token::Entity::delete_many()
                .filter(TokenColumn::PersonId.is_in(chunk.iter().copied()))
                .exec(db)
                .await
                .map_err(db_err)?;
            row::Entity::delete_many()
                .filter(RowColumn::PersonId.is_in(chunk.iter().copied()))
                .exec(db)
                .await
                .map_err(db_err)?;
        }
        Ok(())
    }

    /// Insert the rows of `entries`, then their words.
    async fn insert(
        db: &impl ConnectionTrait,
        entries: &[PersonSearchEntry],
    ) -> Result<(), OxidGeneError> {
        for chunk in entries.chunks(ROW_CHUNK) {
            let mut insert = Query::insert();
            insert.into_table(row::Entity).columns([
                RowColumn::PersonId,
                RowColumn::TreeId,
                RowColumn::SurnameKey,
                RowColumn::GivenKey,
                RowColumn::MaidenKey,
                RowColumn::Sex,
                RowColumn::BirthYear,
                RowColumn::BirthQualifier,
                RowColumn::DeathYear,
                RowColumn::DeathQualifier,
                RowColumn::DateSort,
                RowColumn::DisplayName,
                RowColumn::SurnameDisplay,
                RowColumn::GivenNamesDisplay,
                RowColumn::BirthPlace,
                RowColumn::SpouseNames,
                RowColumn::FatherName,
                RowColumn::MotherName,
                RowColumn::ChildrenCount,
            ]);
            for entry in chunk {
                let r = &entry.row;
                insert.values_panic([
                    r.person_id.into(),
                    r.tree_id.into(),
                    r.surname_key.clone().into(),
                    r.given_key.clone().into(),
                    r.maiden_key.clone().into(),
                    Expr::val(r.sex),
                    r.birth_year.into(),
                    Expr::val(r.birth_qualifier),
                    r.death_year.into(),
                    Expr::val(r.death_qualifier),
                    r.date_sort.into(),
                    r.display_name.clone().into(),
                    r.surname_display.clone().into(),
                    r.given_names_display.clone().into(),
                    r.birth_place.clone().into(),
                    r.spouse_names.clone().into(),
                    r.father_name.clone().into(),
                    r.mother_name.clone().into(),
                    r.children_count.into(),
                ]);
            }
            db.execute(&insert).await.map_err(db_err)?;
        }
        let tokens: Vec<(Uuid, Uuid, String, i32)> = entries
            .iter()
            .flat_map(|entry| {
                words_of_entry(entry)
                    .into_iter()
                    .map(|(word, fields)| (entry.row.tree_id, entry.row.person_id, word, fields))
            })
            .collect();
        for chunk in tokens.chunks(TOKEN_CHUNK) {
            let mut insert = Query::insert();
            insert.into_table(token::Entity).columns([
                TokenColumn::TreeId,
                TokenColumn::Token,
                TokenColumn::PersonId,
                TokenColumn::Fields,
            ]);
            for (tree_id, person_id, word, fields) in chunk {
                insert.values_panic([
                    (*tree_id).into(),
                    word.clone().into(),
                    (*person_id).into(),
                    (*fields).into(),
                ]);
            }
            db.execute(&insert).await.map_err(db_err)?;
        }
        Ok(())
    }
}

/// The folded words of `text`.
fn words_of(text: &str) -> Vec<String> {
    fold_words(text)
        .split_whitespace()
        .map(str::to_owned)
        .collect()
}

/// The words of a year: as it is shown, four digits at least.
fn year_words(year: Option<i32>) -> Vec<String> {
    year.map(|year| words_of(&format!("{year:04}")))
        .unwrap_or_default()
}

/// Every word of `entry`, once each, with the fields that hold it.
fn words_of_entry(entry: &PersonSearchEntry) -> BTreeMap<String, i32> {
    let (row, relatives) = (&entry.row, &entry.relatives);
    let mut words: BTreeMap<String, i32> = BTreeMap::new();
    let mut add = |field: SearchField, found: Vec<String>| {
        for word in found {
            *words.entry(word).or_default() |= field as i32;
        }
    };
    let text = |text: Option<&String>| text.map(|text| words_of(text)).unwrap_or_default();
    add(SearchField::Surname, words_of(&row.surname_key));
    add(SearchField::GivenNames, words_of(&row.given_key));
    add(SearchField::MaidenName, text(row.maiden_key.as_ref()));
    add(SearchField::BirthYear, year_words(row.birth_year));
    add(SearchField::DeathYear, year_words(row.death_year));
    for surname in &relatives.spouse_surnames {
        add(SearchField::SpouseSurname, words_of(surname));
    }
    for given_names in &relatives.spouse_given_names {
        add(SearchField::SpouseGivenNames, words_of(given_names));
    }
    add(
        SearchField::FatherSurname,
        text(relatives.father_surname.as_ref()),
    );
    add(
        SearchField::FatherGivenNames,
        text(relatives.father_given_names.as_ref()),
    );
    add(
        SearchField::MotherSurname,
        text(relatives.mother_surname.as_ref()),
    );
    add(
        SearchField::MotherGivenNames,
        text(relatives.mother_given_names.as_ref()),
    );
    words
}

/// The least text greater than every text `prefix` begins, in code point
/// order (UTF-8's byte order): `prefix` with its last character moved to
/// the next one. `None` when nothing is greater: no upper bound.
fn prefix_end(prefix: &str) -> Option<String> {
    let mut chars: Vec<char> = prefix.chars().collect();
    while let Some(last) = chars.pop() {
        let next = match last {
            '\u{d7ff}' => Some('\u{e000}'),
            last => char::from_u32(u32::from(last) + 1),
        };
        if let Some(next) = next {
            chars.push(next);
            return Some(chars.into_iter().collect());
        }
    }
    None
}

/// The condition that `column` begins with `prefix`, as a range of it.
fn begins(column: impl ExprTrait + Clone, prefix: &str) -> Condition {
    let mut range = Condition::all().add(column.clone().gte(prefix));
    if let Some(end) = prefix_end(prefix) {
        range = range.add(column.lt(end));
    }
    range
}

/// The persons of the tree one of whose `fields` holds a word `word`
/// begins: one range of the words' key.
fn persons_with_word(tree_id: Uuid, word: &str, fields: i32) -> SelectStatement {
    Query::select()
        .column(TokenColumn::PersonId)
        .from(token::Entity)
        .and_where(TokenColumn::TreeId.eq(tree_id))
        .cond_where(begins(Expr::col(TokenColumn::Token), word))
        .and_where(Expr::col(TokenColumn::Fields).bit_and(fields).ne(0))
        .to_owned()
}

/// Every word a search matches, with the fields it is matched in: the free
/// text's in the person's own fields, each name filter's in its own.
fn matched_words(words: &[String], filters: &PersonSearchFilters) -> Vec<(String, i32)> {
    let mut matched: Vec<(String, i32)> = words
        .iter()
        .map(|word| (word.clone(), SearchField::OWN))
        .collect();
    for (value, field) in [
        (&filters.surname, SearchField::Surname),
        (&filters.given_names, SearchField::GivenNames),
        (&filters.spouse_surname, SearchField::SpouseSurname),
        (&filters.spouse_given_names, SearchField::SpouseGivenNames),
        (&filters.father_surname, SearchField::FatherSurname),
        (&filters.father_given_names, SearchField::FatherGivenNames),
        (&filters.mother_surname, SearchField::MotherSurname),
        (&filters.mother_given_names, SearchField::MotherGivenNames),
    ] {
        if let Some(value) = value {
            matched.extend(words_of(value).into_iter().map(|word| (word, field as i32)));
        }
    }
    matched
}

/// The `WHERE` of a search: the rows of the tree its words find, and its
/// other filters.
///
/// With words to match, the persons are those every word's range finds,
/// intersected, and their rows are read by primary key; those ranges are
/// already the tree's, so the rows are not filtered by tree again, which
/// would offer the planner the tree's whole name index instead. Without,
/// the tree's rows are read through their own indexes.
fn filtered_conditions(
    tree_id: Uuid,
    words: &[String],
    filters: &PersonSearchFilters,
) -> Condition {
    let mut conditions = Condition::all();
    let mut matched = matched_words(words, filters).into_iter();
    match matched.next() {
        Some((word, fields)) => {
            let mut persons = persons_with_word(tree_id, &word, fields);
            for (word, fields) in matched {
                persons.union(
                    UnionType::Intersect,
                    persons_with_word(tree_id, &word, fields),
                );
            }
            conditions = conditions.add(RowColumn::PersonId.in_subquery(persons));
        }
        None => conditions = conditions.add(RowColumn::TreeId.eq(tree_id)),
    }
    if let Some(sex) = filters.sex {
        conditions = conditions.add(RowColumn::Sex.eq(sea_enums::Sex::from(sex)));
    }
    for (column, from, to) in [
        (RowColumn::BirthYear, filters.birth_from, filters.birth_to),
        (RowColumn::DeathYear, filters.death_from, filters.death_to),
    ] {
        if let Some(from) = from {
            conditions = conditions.add(column.gte(from));
        }
        if let Some(to) = to {
            conditions = conditions.add(column.lte(to));
        }
    }
    conditions = add_event_filters(conditions, tree_id, filters);
    conditions = add_media_and_tag_filters(conditions, tree_id, filters);
    if !filters.unseen.hide_nothing() {
        conditions = conditions.add(
            crate::repo::Unseen::new(tree_id, filters.unseen)
                .condition_on(Expr::col((row::Entity, RowColumn::PersonId))),
        );
    }
    conditions
}

/// The value, unless it is missing or only whitespace.
fn non_blank(value: Option<&str>) -> Option<&str> {
    value.filter(|value| !value.trim().is_empty())
}

/// A `LIKE … ESCAPE '\'` of the trimmed value, lowercased, anywhere in the
/// lowercased `text`: `%` and `_` in it match themselves.
fn lowercase_contains(text: Expr, value: &str) -> Expr {
    let pattern = format!("%{}%", escape_like(&value.trim().to_lowercase()));
    Expr::expr(Func::lower(text)).like(LikeExpr::new(pattern).escape('\\'))
}

/// January 1st of `year`, or the nearest date chrono holds.
fn first_day(year: i32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, 1, 1).unwrap_or(if year > 0 {
        NaiveDate::MAX.with_day(1).unwrap_or(NaiveDate::MAX)
    } else {
        NaiveDate::MIN
    })
}

/// December 31st of `year`, or the nearest date chrono holds.
fn last_day(year: i32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, 12, 31).unwrap_or(if year > 0 {
        NaiveDate::MAX
    } else {
        NaiveDate::MIN
    })
}

/// The filters on the person's events: one event (their own or a family's
/// they are a spouse in) matching place, type and years together, and an
/// occupation. Each is a tree-scoped `person_id IN (…)`: the matching ids
/// selected once from the tree's own events.
fn add_event_filters(
    mut conditions: Condition,
    tree_id: Uuid,
    filters: &PersonSearchFilters,
) -> Condition {
    let place = non_blank(filters.place.as_deref());
    if place.is_some()
        || filters.event_type.is_some()
        || filters.event_from.is_some()
        || filters.event_to.is_some()
    {
        let mut own = events_matching(tree_id, filters, place);
        own.column((event::Entity, event::Column::PersonId));
        let mut family = events_matching(tree_id, filters, place);
        family
            .column((family_spouse::Entity, family_spouse::Column::PersonId))
            .inner_join(
                family_spouse::Entity,
                Expr::col((family_spouse::Entity, family_spouse::Column::FamilyId))
                    .equals((event::Entity, event::Column::FamilyId)),
            );
        own.union(UnionType::Distinct, family);
        conditions = conditions.add(RowColumn::PersonId.in_subquery(own));
    }

    if let Some(occupation) = non_blank(filters.occupation.as_deref()) {
        let occupations = Query::select()
            .column(event::Column::PersonId)
            .from(event::Entity)
            .and_where(event::Column::TreeId.eq(tree_id))
            .and_where(event::Column::EventType.eq(sea_enums::EventType::Occupation))
            .and_where(lowercase_contains(
                Func::coalesce([Expr::col(event::Column::Description), Expr::val("")]).into(),
                occupation,
            ))
            .to_owned();
        conditions = conditions.add(RowColumn::PersonId.in_subquery(occupations));
    }
    conditions
}

/// The tree's events matching place, type and years, without the
/// column selected.
fn events_matching(
    tree_id: Uuid,
    filters: &PersonSearchFilters,
    place: Option<&str>,
) -> SelectStatement {
    let mut events = Query::select();
    events
        .from(event::Entity)
        .and_where(Expr::col((event::Entity, event::Column::TreeId)).eq(tree_id));
    if let Some(place) = place {
        events
            .inner_join(
                place::Entity,
                Expr::col((place::Entity, place::Column::Id))
                    .equals((event::Entity, event::Column::PlaceId)),
            )
            .and_where(lowercase_contains(
                Expr::col((place::Entity, place::Column::Name)),
                place,
            ));
    }
    if let Some(event_type) = filters.event_type {
        events.and_where(
            Expr::col((event::Entity, event::Column::EventType))
                .eq(sea_enums::EventType::from(event_type)),
        );
    }
    if let Some(year) = filters.event_from {
        events.and_where(Expr::col((event::Entity, event::Column::DateSort)).gte(first_day(year)));
    }
    if let Some(year) = filters.event_to {
        events.and_where(Expr::col((event::Entity, event::Column::DateSort)).lte(last_day(year)));
    }
    events
}

/// The filters on attached media and on tags: tree-scoped
/// `person_id IN (…)` each, one per tag.
fn add_media_and_tag_filters(
    mut conditions: Condition,
    tree_id: Uuid,
    filters: &PersonSearchFilters,
) -> Condition {
    if filters.has_media {
        let with_media = Query::select()
            .column((media_link::Entity, media_link::Column::PersonId))
            .from(media_link::Entity)
            .inner_join(
                media::Entity,
                Expr::col((media::Entity, media::Column::Id))
                    .equals((media_link::Entity, media_link::Column::MediaId)),
            )
            .and_where(Expr::col((media::Entity, media::Column::TreeId)).eq(tree_id))
            .to_owned();
        conditions = conditions.add(RowColumn::PersonId.in_subquery(with_media));
    }
    for tag_id in &filters.tags {
        let tagged = Query::select()
            .column(record_tag::Column::PersonId)
            .from(record_tag::Entity)
            .and_where(record_tag::Column::TreeId.eq(tree_id))
            .and_where(record_tag::Column::TagId.eq(*tag_id))
            .and_where(record_tag::Column::PersonId.is_not_null())
            .to_owned();
        conditions = conditions.add(RowColumn::PersonId.in_subquery(tagged));
    }
    conditions
}

/// An offset SQL takes: one past `i64::MAX`, which PostgreSQL would read as
/// negative, is past every match anyway.
fn bounded(offset: u64) -> u64 {
    Ord::min(offset, i64::MAX.unsigned_abs())
}

/// The count, as `total`, of the rows `conditions` keep.
fn counted(conditions: Condition) -> sea_orm::Select<row::Entity> {
    row::Entity::find()
        .select_only()
        .expr_as(Func::count(Expr::col(RowColumn::PersonId)), "total")
        .filter(conditions)
}

/// The `total` of a counting row.
fn total_of(row: &sea_orm::QueryResult) -> Result<u64, OxidGeneError> {
    let total: i64 = row.try_get("", "total").map_err(db_err)?;
    Ok(u64::try_from(total).unwrap_or(0))
}

/// How many rows `conditions` keep.
async fn count(db: &impl ConnectionTrait, conditions: Condition) -> Result<u64, OxidGeneError> {
    let statement = counted(conditions).build(db.get_database_backend());
    match db.query_one_raw(statement).await.map_err(db_err)? {
        Some(row) => total_of(&row),
        None => Ok(0),
    }
}

/// `select` in the order `sort` asks for, the person last: one order for
/// every page.
fn sorted(
    select: sea_orm::Select<row::Entity>,
    words: &[String],
    filters: &PersonSearchFilters,
    sort: PersonSearchSort,
) -> sea_orm::Select<row::Entity> {
    let by_name = |select: sea_orm::Select<row::Entity>, order: Order| {
        select
            .order_by(RowColumn::SurnameKey, order.clone())
            .order_by(RowColumn::GivenKey, order.clone())
            .order_by(RowColumn::PersonId, order)
    };
    let by_birth = |select: sea_orm::Select<row::Entity>, order: Order| {
        let select = select
            .order_by(Expr::col(RowColumn::DateSort).is_null(), Order::Asc)
            .order_by(RowColumn::DateSort, order);
        by_name(select, Order::Asc)
    };
    match sort {
        PersonSearchSort::Relevance => match relevance_term(words, filters) {
            Some(term) => {
                let rank: Expr = Expr::case(begins(Expr::col(RowColumn::SurnameKey), &term), 0)
                    .case(begins(Expr::col(RowColumn::GivenKey), &term), 1)
                    .finally(2)
                    .into();
                by_name(select.order_by(rank, Order::Asc), Order::Asc)
            }
            None => by_name(select, Order::Asc),
        },
        PersonSearchSort::NameAsc => by_name(select, Order::Asc),
        PersonSearchSort::NameDesc => by_name(select, Order::Desc),
        PersonSearchSort::BirthAsc => by_birth(select, Order::Asc),
        PersonSearchSort::BirthDesc => by_birth(select, Order::Desc),
    }
}

/// What [`PersonSearchSort::Relevance`] ranks by: the first word of the
/// free text, or else the `surname` or the `given_names` filter, folded.
///
/// A hit whose surname begins with it ranks first, then one whose given
/// names do, then every other match; each rank in name order. With nothing
/// to rank by, relevance is the name order. One expression on both
/// backends, ranking a search made of structured filters as well as one of
/// free text.
fn relevance_term(words: &[String], filters: &PersonSearchFilters) -> Option<String> {
    words
        .first()
        .cloned()
        .or_else(|| {
            [filters.surname.as_deref(), filters.given_names.as_deref()]
                .into_iter()
                .flatten()
                .map(fold_words)
                .find(|term| !term.is_empty())
        })
        .filter(|term| !term.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_prefix_ends_at_its_last_character_moved_on() {
        assert_eq!(prefix_end("abc").as_deref(), Some("abd"));
        assert_eq!(prefix_end("az").as_deref(), Some("a{"));
        assert_eq!(prefix_end("é").as_deref(), Some("ê"));
        assert_eq!(prefix_end("a\u{d7ff}").as_deref(), Some("a\u{e000}"));
        assert_eq!(prefix_end("a\u{10ffff}").as_deref(), Some("b"));
        assert_eq!(prefix_end("\u{10ffff}"), None);
        assert_eq!(prefix_end(""), None);
    }

    #[test]
    fn a_word_is_kept_once_with_every_field_holding_it() {
        let entry = PersonSearchEntry {
            row: PersonSearchRow {
                person_id: Uuid::nil(),
                tree_id: Uuid::nil(),
                surname_key: "branch".into(),
                given_key: "anne marie".into(),
                maiden_key: None,
                sex: sea_enums::Sex::Female,
                birth_year: Some(850),
                birth_qualifier: sea_enums::DateQualifier::Exact,
                death_year: None,
                death_qualifier: sea_enums::DateQualifier::Exact,
                date_sort: None,
                display_name: "Anne-Marie BRANCH".into(),
                surname_display: "BRANCH".into(),
                given_names_display: "Anne-Marie".into(),
                birth_place: None,
                spouse_names: String::new(),
                father_name: None,
                mother_name: None,
                children_count: 0,
            },
            relatives: RelativeNames {
                father_surname: Some("branch".into()),
                spouse_given_names: vec!["paul".into(), "marie".into()],
                ..RelativeNames::default()
            },
        };
        let words = words_of_entry(&entry);
        assert_eq!(
            words.into_iter().collect::<Vec<_>>(),
            [
                ("0850".to_owned(), SearchField::BirthYear as i32),
                ("anne".to_owned(), SearchField::GivenNames as i32),
                (
                    "branch".to_owned(),
                    SearchField::Surname as i32 | SearchField::FatherSurname as i32
                ),
                (
                    "marie".to_owned(),
                    SearchField::GivenNames as i32 | SearchField::SpouseGivenNames as i32
                ),
                ("paul".to_owned(), SearchField::SpouseGivenNames as i32),
            ]
        );
    }

    #[test]
    fn blank_filters_add_no_condition() {
        let filters = PersonSearchFilters {
            place: Some("  ".into()),
            occupation: Some(" ".into()),
            surname: Some(" ".into()),
            mother_surname: Some("\t".into()),
            ..PersonSearchFilters::default()
        };
        let sql = |backend| {
            row::Entity::find()
                .select_only()
                .column(RowColumn::PersonId)
                .filter(filtered_conditions(Uuid::nil(), &[], &filters))
                .build(backend)
                .sql
        };
        assert_eq!(
            sql(DbBackend::Sqlite),
            r#"SELECT "person_search"."person_id" FROM "person_search" WHERE "person_search"."tree_id" = ?"#
        );
        assert_eq!(
            sql(DbBackend::Postgres),
            r#"SELECT "person_search"."person_id" FROM "person_search" WHERE "person_search"."tree_id" = $1"#
        );
    }

    #[test]
    fn every_word_is_a_range_of_the_words_of_its_fields() {
        let filters = PersonSearchFilters {
            surname: Some("Dé La".into()),
            ..PersonSearchFilters::default()
        };
        let statement = row::Entity::find()
            .select_only()
            .column(RowColumn::PersonId)
            .filter(filtered_conditions(
                Uuid::nil(),
                &words_of("Anne"),
                &filters,
            ))
            .build(DbBackend::Sqlite);
        let word = r#"SELECT "person_id" FROM "person_search_token" WHERE "person_search_token"."tree_id" = ? AND "token" >= ? AND "token" < ? AND ("fields" & ?) <> ?"#;
        assert_eq!(
            statement.sql,
            format!(
                r#"SELECT "person_search"."person_id" FROM "person_search" WHERE "person_search"."person_id" IN ({word} INTERSECT {word} INTERSECT {word})"#
            )
        );
        let values = statement.values.map(|values| values.0).unwrap_or_default();
        let bound: Vec<String> = values.iter().map(|value| format!("{value:?}")).collect();
        let expected = |word: &str, end: &str, fields: i32| {
            [
                format!("{:?}", sea_orm::Value::from(Uuid::nil())),
                format!("{:?}", sea_orm::Value::from(word)),
                format!("{:?}", sea_orm::Value::from(end)),
                format!("{:?}", sea_orm::Value::from(fields)),
                format!("{:?}", sea_orm::Value::from(0)),
            ]
        };
        let surname = SearchField::Surname as i32;
        assert_eq!(
            bound,
            [
                expected("anne", "annf", SearchField::OWN),
                expected("de", "df", surname),
                expected("la", "lb", surname),
            ]
            .concat()
        );
    }
}
