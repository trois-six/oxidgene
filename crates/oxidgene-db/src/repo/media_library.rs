//! A tree's whole media library: its filtered listing, how much each
//! document is used, and the values its filters offer.
//!
//! Backs the Dictionary's Media tab (see `docs/ui-dictionary.md`). The listed
//! rows are documents — a page is reached through its document, never listed
//! beside it.
//!
//! # How the filters combine
//!
//! Every filter narrows the same set of document ids, so they combine with
//! AND. The structural ones (tag, file kind, category, date added) are SQL
//! conditions on the id query. The text ones are matched accent-folded, which
//! SQL cannot do portably, so they are resolved to id sets here — the media
//! names from the tree's own rows, the linked names through the
//! pre-normalized `person_search_fts` columns — and intersected in Rust. The
//! event-date filter is an id set too, gathered by two grouped queries. Only
//! ids travel until the requested page is known; the page's rows are then
//! loaded in one query.

use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{Days, NaiveDate};
use oxidgene_core::enums::{DocumentCategory, MediaFileKind};
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::search::normalize_for_search;
use oxidgene_core::types::{Connection, Edge, Media, PageInfo};
use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{Func, Query, SimpleExpr};
use sea_orm::{
    Condition, ConnectionTrait, DbBackend, EntityTrait, JoinType, QueryFilter, QuerySelect,
    Statement,
};
use uuid::Uuid;

use crate::entities::media::{self, Column, Entity};
use crate::entities::{event, family_spouse, media_link, media_tag, vignette};
use crate::repo::MediaRepo;
use crate::repo::batch::in_chunks;
use crate::repo::pagination::{PaginationParams, encode_cursor};

/// What a library listing is narrowed to. Every field is optional, and the
/// set fields combine with AND; the default lists every document.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaFilter {
    /// Documents carrying every one of these tags, each given as its
    /// normalized key (`media_tag.normalized_tag`).
    pub tags: Vec<String>,
    /// Documents with at least one live page of this kind.
    pub kind: Option<MediaFileKind>,
    pub category: Option<DocumentCategory>,
    /// Substring of the document's title or file name, or of one of its
    /// pages' file names; case- and accent-insensitive.
    pub name: Option<String>,
    /// Substring of the name of a person connected to the document; case-
    /// and accent-insensitive. See [`MediaLibraryRepo::connected_to_named`].
    pub linked_name: Option<String>,
    /// Earliest year, inclusive, of an event the document is linked to.
    pub event_from: Option<i32>,
    /// Latest year, inclusive, of an event the document is linked to.
    pub event_to: Option<i32>,
    /// Earliest day, inclusive and in UTC, the document was added.
    pub added_from: Option<NaiveDate>,
    /// Latest day, inclusive and in UTC, the document was added.
    pub added_to: Option<NaiveDate>,
}

/// One tag of a tree and how many documents carry it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaTagCount {
    /// The spelling most of those documents carry.
    pub tag: String,
    /// The key the tag is matched on.
    pub normalized: String,
    pub count: i64,
}

/// The values a tree's library filters can take, each with its number of
/// documents. Only values the tree holds are listed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaFacets {
    /// Sorted alphabetically, ignoring case and accents.
    pub tags: Vec<MediaTagCount>,
    /// In [`MediaFileKind::all`] order. A document of several kinds counts
    /// once in each.
    pub kinds: Vec<(MediaFileKind, i64)>,
    /// In [`DocumentCategory::all`] order.
    pub categories: Vec<(DocumentCategory, i64)>,
}

/// Repository for the tree-wide media library.
pub struct MediaLibraryRepo;

impl MediaLibraryRepo {
    /// A page of the tree's documents matching `filter`, in creation order,
    /// with `total_count` counted under the same filter.
    pub async fn list(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        filter: &MediaFilter,
        params: &PaginationParams,
    ) -> Result<Connection<Media>, OxidGeneError> {
        let limit = params.clamped_first() as usize;
        let after = params.decode_cursor()?;

        let mut ids = Self::structural_ids(db, tree_id, filter).await?;
        if !ids.is_empty() && (filter.event_from.is_some() || filter.event_to.is_some()) {
            let linked =
                Self::linked_to_events_in(db, tree_id, filter.event_from, filter.event_to).await?;
            ids.retain(|id| linked.contains(id));
        }
        if !ids.is_empty()
            && let Some(name) = filter.name.as_deref()
        {
            let named = Self::named(db, tree_id, name).await?;
            ids.retain(|id| named.contains(id));
        }
        if !ids.is_empty()
            && let Some(name) = filter.linked_name.as_deref()
        {
            let connected = Self::connected_to_named(db, tree_id, name).await?;
            ids.retain(|id| connected.contains(id));
        }
        ids.sort_unstable();

        let start = after.map_or(0, |after| ids.partition_point(|id| *id <= after));
        let end = ids.len().min(start + limit);
        let page_ids = &ids[start..end];
        let mut media = MediaRepo::get_many(db, page_ids).await?;
        media.sort_by_key(|item| item.id);
        let edges: Vec<Edge<Media>> = media
            .into_iter()
            .map(|node| Edge {
                cursor: encode_cursor(&node.id),
                node,
            })
            .collect();
        Ok(Connection {
            page_info: PageInfo {
                has_next_page: end < ids.len(),
                end_cursor: edges.last().map(|edge| edge.cursor.clone()),
            },
            edges,
            total_count: ids.len() as i64,
        })
    }

    /// How many distinct records — persons, families, events and sources —
    /// each document is attached to, through a link on the document itself
    /// or on one of its pages. A document nothing links to is absent.
    ///
    /// One grouped query per bounded slice of ids, whatever their number.
    pub async fn usage_counts(
        db: &impl ConnectionTrait,
        document_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, i64>, OxidGeneError> {
        // A link names exactly one target, so the first non-null id is it;
        // v7 ids are unique across tables, so counting them distinctly
        // counts the records.
        let target = || {
            Func::count_distinct(Func::coalesce([
                Expr::col((media_link::Entity, media_link::Column::PersonId)),
                Expr::col((media_link::Entity, media_link::Column::FamilyId)),
                Expr::col((media_link::Entity, media_link::Column::EventId)),
                Expr::col((media_link::Entity, media_link::Column::SourceId)),
            ]))
        };
        let rows: Vec<(Uuid, i64)> = in_chunks(document_ids, |chunk| async move {
            media_link::Entity::find()
                .select_only()
                .column_as(document_of_row(), "document_id")
                .column_as(Expr::expr(target()), "uses")
                .join(JoinType::InnerJoin, media_link::Relation::Media.def())
                .filter(Column::DeletedAt.is_null())
                .filter(
                    Condition::any()
                        .add(Column::Id.is_in(chunk.clone()))
                        .add(Column::ParentMediaId.is_in(chunk)),
                )
                .group_by(document_of_row())
                .into_tuple()
                .all(db)
                .await
                .map_err(db_error)
        })
        .await?;
        Ok(rows.into_iter().collect())
    }

    /// The tags, file kinds and categories the tree's live documents carry,
    /// each counted in the database — one row per value comes back, not one
    /// per document. The tags are counted among the documents carrying every
    /// tag of `with_tags` (normalized keys), so a tag cloud narrows to the
    /// tags that can still be added to a selection; the kinds and categories
    /// always count the whole library.
    pub async fn facets(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        with_tags: &[String],
    ) -> Result<MediaFacets, OxidGeneError> {
        Ok(MediaFacets {
            tags: Self::tag_counts(db, tree_id, with_tags).await?,
            kinds: Self::kind_counts(db, tree_id).await?,
            categories: Self::category_counts(db, tree_id).await?,
        })
    }

    /// Every tag used in a tree, with how many live documents carry it,
    /// counting only the documents that also carry every tag of `with_tags`.
    ///
    /// A tag is identified by its normalized key and displayed in the
    /// spelling most documents carry, ties going to the first in code-point
    /// order so the choice is stable.
    async fn tag_counts(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        with_tags: &[String],
    ) -> Result<Vec<MediaTagCount>, OxidGeneError> {
        let mut query = media_tag::Entity::find();
        for tag in with_tags {
            query = query.filter(media_tag::Column::MediaId.in_subquery(tagged(tag)));
        }
        let rows: Vec<(String, String, i64)> = query
            .select_only()
            .column(media_tag::Column::NormalizedTag)
            .column(media_tag::Column::Tag)
            .column_as(media_tag::Column::MediaId.count(), "uses")
            .join(JoinType::InnerJoin, media_tag::Relation::Media.def())
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::ParentMediaId.is_null())
            .filter(Column::DeletedAt.is_null())
            .group_by(media_tag::Column::NormalizedTag)
            .group_by(media_tag::Column::Tag)
            .into_tuple()
            .all(db)
            .await
            .map_err(db_error)?;

        // normalized key -> (documents, best spelling, its documents)
        let mut tags: BTreeMap<String, (i64, String, i64)> = BTreeMap::new();
        for (normalized, spelling, uses) in rows {
            let entry = tags
                .entry(normalized)
                .or_insert_with(|| (0, spelling.clone(), 0));
            entry.0 += uses;
            if uses > entry.2 || (uses == entry.2 && spelling < entry.1) {
                entry.1 = spelling;
                entry.2 = uses;
            }
        }
        let mut counts: Vec<MediaTagCount> = tags
            .into_iter()
            .map(|(normalized, (count, tag, _))| MediaTagCount {
                tag,
                normalized,
                count,
            })
            .collect();
        counts.sort_by_cached_key(|entry| {
            (normalize_for_search(&entry.tag), entry.normalized.clone())
        });
        Ok(counts)
    }

    async fn kind_counts(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<(MediaFileKind, i64)>, OxidGeneError> {
        let rows: Vec<(String, i64)> = Entity::find()
            .select_only()
            .column_as(Expr::cust(kind_case()), "kind")
            .column_as(
                Expr::expr(Func::count_distinct(Expr::col(Column::ParentMediaId))),
                "uses",
            )
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::DeletedAt.is_null())
            .filter(Column::ParentMediaId.in_subquery(live_documents(tree_id)))
            // By alias: both backends accept it, and repeating the CASE would
            // make PostgreSQL compare two expressions it cannot prove equal.
            .group_by(Expr::cust("kind"))
            .into_tuple()
            .all(db)
            .await
            .map_err(db_error)?;
        let counts: HashMap<MediaFileKind, i64> = rows
            .into_iter()
            .filter_map(|(kind, uses)| Some((MediaFileKind::parse(&kind)?, uses)))
            .collect();
        Ok(MediaFileKind::all()
            .iter()
            .filter_map(|kind| Some((*kind, *counts.get(kind)?)))
            .collect())
    }

    async fn category_counts(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<(DocumentCategory, i64)>, OxidGeneError> {
        let rows: Vec<(String, i64)> = Entity::find()
            .select_only()
            .column(Column::DocumentCategory)
            .column_as(Column::Id.count(), "uses")
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::ParentMediaId.is_null())
            .filter(Column::DeletedAt.is_null())
            .filter(Column::DocumentCategory.is_not_null())
            .group_by(Column::DocumentCategory)
            .into_tuple()
            .all(db)
            .await
            .map_err(db_error)?;
        let counts: HashMap<DocumentCategory, i64> = rows
            .into_iter()
            .filter_map(|(category, uses)| Some((DocumentCategory::parse(&category)?, uses)))
            .collect();
        Ok(DocumentCategory::all()
            .iter()
            .filter_map(|category| Some((*category, *counts.get(category)?)))
            .collect())
    }

    /// The ids of the tree's live documents passing the filters SQL can
    /// express directly.
    async fn structural_ids(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        filter: &MediaFilter,
    ) -> Result<Vec<Uuid>, OxidGeneError> {
        let mut query = Entity::find()
            .select_only()
            .column(Column::Id)
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::ParentMediaId.is_null())
            .filter(Column::DeletedAt.is_null());
        for tag in &filter.tags {
            query = query.filter(Column::Id.in_subquery(tagged(tag)));
        }
        if let Some(kind) = filter.kind {
            query = query.filter(
                Column::Id.in_subquery(
                    Query::select()
                        .column(Column::ParentMediaId)
                        .from(Entity)
                        .and_where(Column::ParentMediaId.is_not_null())
                        .and_where(Column::DeletedAt.is_null())
                        .and_where(Expr::cust(kind_predicate(kind)))
                        .to_owned(),
                ),
            );
        }
        if let Some(category) = filter.category {
            query = query.filter(Column::DocumentCategory.eq(category.as_str()));
        }
        if let Some(from) = filter.added_from {
            query = query
                .filter(Column::CreatedAt.gte(from.and_time(chrono::NaiveTime::MIN).and_utc()));
        }
        if let Some(to) = filter.added_to {
            // The whole last day: strictly before the next one's midnight.
            let next = to
                .checked_add_days(Days::new(1))
                .ok_or_else(|| OxidGeneError::Validation(format!("date out of range: {to}")))?;
            query =
                query.filter(Column::CreatedAt.lt(next.and_time(chrono::NaiveTime::MIN).and_utc()));
        }
        query.into_tuple().all(db).await.map_err(db_error)
    }

    /// Documents linked — themselves or through a page, by a media link or a
    /// crop — to a live event dated within the years, inclusive. An event is
    /// placed by its `date_sort`, the same key the person search filters
    /// events on; an undated event matches no range.
    async fn linked_to_events_in(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        from: Option<i32>,
        to: Option<i32>,
    ) -> Result<HashSet<Uuid>, OxidGeneError> {
        let mut dated = Condition::all().add(event::Column::DeletedAt.is_null());
        if let Some(year) = from {
            let day = NaiveDate::from_ymd_opt(year, 1, 1)
                .ok_or_else(|| OxidGeneError::Validation(format!("year out of range: {year}")))?;
            dated = dated.add(event::Column::DateSort.gte(day));
        }
        if let Some(year) = to {
            let day = NaiveDate::from_ymd_opt(year, 12, 31)
                .ok_or_else(|| OxidGeneError::Validation(format!("year out of range: {year}")))?;
            dated = dated.add(event::Column::DateSort.lte(day));
        }
        let mut documents: HashSet<Uuid> = media_link::Entity::find()
            .select_only()
            .column_as(document_of_row(), "document_id")
            .distinct()
            .join(JoinType::InnerJoin, media_link::Relation::Media.def())
            .join(JoinType::InnerJoin, media_link::Relation::Event.def())
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::DeletedAt.is_null())
            .filter(dated.clone())
            .into_tuple::<Uuid>()
            .all(db)
            .await
            .map_err(db_error)?
            .into_iter()
            .collect();
        documents.extend(
            vignette::Entity::find()
                .select_only()
                .column_as(document_of_row(), "document_id")
                .distinct()
                .join(JoinType::InnerJoin, vignette::Relation::Media.def())
                .join(JoinType::InnerJoin, vignette::Relation::Event.def())
                .filter(Column::TreeId.eq(tree_id))
                .filter(Column::DeletedAt.is_null())
                .filter(dated)
                .into_tuple::<Uuid>()
                .all(db)
                .await
                .map_err(db_error)?,
        );
        Ok(documents)
    }

    /// Documents whose title or file name, or one of whose pages' file
    /// names, contains `text`, compared accent-folded.
    ///
    /// Folded here rather than in SQL, which cannot fold accents portably;
    /// what travels is four short columns per row of the tree.
    async fn named(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        text: &str,
    ) -> Result<HashSet<Uuid>, OxidGeneError> {
        let needle = normalize_for_search(text.trim());
        let rows: Vec<(Uuid, Option<Uuid>, Option<String>, String)> = Entity::find()
            .select_only()
            .columns([
                Column::Id,
                Column::ParentMediaId,
                Column::Title,
                Column::FileName,
            ])
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::DeletedAt.is_null())
            .into_tuple()
            .all(db)
            .await
            .map_err(db_error)?;
        Ok(rows
            .into_iter()
            .filter(|(_, _, title, file_name)| {
                title
                    .as_deref()
                    .is_some_and(|title| normalize_for_search(title).contains(&needle))
                    || normalize_for_search(file_name).contains(&needle)
            })
            .map(|(id, parent, _, _)| parent.unwrap_or(id))
            .collect())
    }

    /// Documents connected to a person whose name contains `text`.
    ///
    /// A person is matched on their primary given names and surname, read in
    /// either order, or their maiden name — the accent-folded columns the
    /// person search matches its named filters on. A document is connected
    /// to them through a media link (on it or one of its pages) to the
    /// person, to a family they are a spouse in, or to an event of theirs or
    /// of such a family, and through a crop identifying them.
    async fn connected_to_named(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        text: &str,
    ) -> Result<HashSet<Uuid>, OxidGeneError> {
        let persons = named_persons(db, tree_id, text).await?;
        if persons.is_empty() {
            return Ok(HashSet::new());
        }
        let families: Vec<Uuid> = in_chunks(&persons, |chunk| async move {
            family_spouse::Entity::find()
                .select_only()
                .column(family_spouse::Column::FamilyId)
                .distinct()
                .filter(family_spouse::Column::PersonId.is_in(chunk))
                .into_tuple()
                .all(db)
                .await
                .map_err(db_error)
        })
        .await?;
        let mut events: Vec<Uuid> = in_chunks(&persons, |chunk| async move {
            event_ids(db, event::Column::PersonId, chunk).await
        })
        .await?;
        events.extend(
            in_chunks(&families, |chunk| async move {
                event_ids(db, event::Column::FamilyId, chunk).await
            })
            .await?,
        );

        let mut documents = HashSet::new();
        for (column, ids) in [
            (media_link::Column::PersonId, &persons),
            (media_link::Column::FamilyId, &families),
            (media_link::Column::EventId, &events),
        ] {
            documents.extend(
                in_chunks(ids, |chunk| async move {
                    media_link::Entity::find()
                        .select_only()
                        .column_as(document_of_row(), "document_id")
                        .distinct()
                        .join(JoinType::InnerJoin, media_link::Relation::Media.def())
                        .filter(Column::TreeId.eq(tree_id))
                        .filter(Column::DeletedAt.is_null())
                        .filter(column.is_in(chunk))
                        .into_tuple::<Uuid>()
                        .all(db)
                        .await
                        .map_err(db_error)
                })
                .await?,
            );
        }
        documents.extend(
            in_chunks(&persons, |chunk| async move {
                vignette::Entity::find()
                    .select_only()
                    .column_as(document_of_row(), "document_id")
                    .distinct()
                    .join(JoinType::InnerJoin, vignette::Relation::Media.def())
                    .filter(Column::TreeId.eq(tree_id))
                    .filter(Column::DeletedAt.is_null())
                    .filter(vignette::Column::PersonId.is_in(chunk))
                    .into_tuple::<Uuid>()
                    .all(db)
                    .await
                    .map_err(db_error)
            })
            .await?,
        );
        Ok(documents)
    }
}

/// The document a joined `media` row stands for: its parent when it is a
/// page, itself otherwise.
fn document_of_row() -> SimpleExpr {
    Expr::expr(Func::coalesce([
        Expr::col((media::Entity, Column::ParentMediaId)),
        Expr::col((media::Entity, Column::Id)),
    ]))
}

/// The tree's live documents, as a subquery of ids.
fn live_documents(tree_id: Uuid) -> sea_orm::sea_query::SelectStatement {
    Query::select()
        .column(Column::Id)
        .from(Entity)
        .and_where(Column::TreeId.eq(tree_id))
        .and_where(Column::ParentMediaId.is_null())
        .and_where(Column::DeletedAt.is_null())
        .to_owned()
}

/// The SQL condition on a `media` row's `mime_type` that makes it `kind`.
///
/// Literal SQL, and no user text in it: the patterns are constants.
fn kind_predicate(kind: MediaFileKind) -> &'static str {
    match kind {
        MediaFileKind::Image => "LOWER(mime_type) LIKE 'image/%'",
        MediaFileKind::Pdf => "LOWER(mime_type) = 'application/pdf'",
        MediaFileKind::Video => "LOWER(mime_type) LIKE 'video/%'",
        MediaFileKind::Audio => "LOWER(mime_type) LIKE 'audio/%'",
        MediaFileKind::Other => {
            "NOT (LOWER(mime_type) LIKE 'image/%' OR LOWER(mime_type) = 'application/pdf' \
             OR LOWER(mime_type) LIKE 'video/%' OR LOWER(mime_type) LIKE 'audio/%')"
        }
    }
}

/// A CASE expression naming a `media` row's kind, built from
/// [`kind_predicate`] so the count and the filter cannot disagree.
fn kind_case() -> String {
    let arms: String = MediaFileKind::all()
        .iter()
        .filter(|kind| **kind != MediaFileKind::Other)
        .map(|kind| format!("WHEN {} THEN '{}' ", kind_predicate(*kind), kind.as_str()))
        .collect();
    format!("CASE {arms}ELSE '{}' END", MediaFileKind::Other.as_str())
}

/// The live events carrying one of `ids` in `column`.
async fn event_ids(
    db: &impl ConnectionTrait,
    column: event::Column,
    ids: Vec<Uuid>,
) -> Result<Vec<Uuid>, OxidGeneError> {
    event::Entity::find()
        .select_only()
        .column(event::Column::Id)
        .filter(event::Column::DeletedAt.is_null())
        .filter(column.is_in(ids))
        .into_tuple()
        .all(db)
        .await
        .map_err(db_error)
}

/// The persons of the tree whose primary name — given names and surname in
/// either order — or maiden name contains `text`, accent-folded.
async fn named_persons(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    text: &str,
) -> Result<Vec<Uuid>, OxidGeneError> {
    let pattern = format!("%{}%", escape_like(&normalize_for_search(text.trim())));
    let backend = db.get_database_backend();
    let placeholders: Vec<String> = (1..=4)
        .map(|index| match backend {
            DbBackend::Postgres => format!("${index}"),
            _ => "?".to_string(),
        })
        .collect();
    let sql = format!(
        "SELECT person_id FROM person_search_fts WHERE tree_id = {} AND (\
         (given_names || ' ' || surname) LIKE {} ESCAPE '\\' \
         OR (surname || ' ' || given_names) LIKE {} ESCAPE '\\' \
         OR COALESCE(maiden_name, '') LIKE {} ESCAPE '\\')",
        placeholders[0], placeholders[1], placeholders[2], placeholders[3],
    );
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            backend,
            sql,
            [
                tree_id.to_string().into(),
                pattern.clone().into(),
                pattern.clone().into(),
                pattern.into(),
            ],
        ))
        .await
        .map_err(db_error)?;
    rows.into_iter()
        .map(|row| {
            let id: String = row.try_get("", "person_id").map_err(db_error)?;
            Uuid::parse_str(&id).map_err(|error| OxidGeneError::Database(error.to_string()))
        })
        .collect()
}

/// Escape `LIKE`'s wildcards so user text matches literally, under
/// `ESCAPE '\'`.
fn escape_like(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The ids of the media carrying the tag of normalized key `tag`.
fn tagged(tag: &str) -> sea_orm::sea_query::SelectStatement {
    Query::select()
        .column(media_tag::Column::MediaId)
        .from(media_tag::Entity)
        .and_where(media_tag::Column::NormalizedTag.eq(tag))
        .to_owned()
}

fn db_error(error: DbErr) -> OxidGeneError {
    OxidGeneError::Database(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn like_wildcards_in_user_text_match_literally() {
        assert_eq!(escape_like("100%_a\\b"), "100\\%\\_a\\\\b");
    }

    #[test]
    fn every_kind_but_other_has_an_arm_and_other_is_the_fallback() {
        let case = kind_case();
        for kind in MediaFileKind::all() {
            assert!(case.contains(&format!("'{}'", kind.as_str())), "{case}");
        }
        assert!(case.ends_with("ELSE 'other' END"), "{case}");
    }
}
