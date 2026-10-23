//! Tags put on records: adding and removing one, reading the tags of many
//! records of one kind, and paging the live records a tag is put on.

use std::collections::HashMap;

use oxidgene_core::collections::sorted_unique;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::search::fold_words;
use oxidgene_core::types::{Connection, Edge, PageInfo, RecordKind, TagRef};
use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{Alias, Expr, ExprTrait, OnConflict, Query, SelectStatement};
use sea_orm::{ConnectionTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Set};
use uuid::Uuid;

use crate::entities::{record_tag, tag};
use crate::repo::batch::in_chunks;
use crate::repo::db_err;
use crate::repo::pagination::{PaginationParams, encode_cursor};

/// Rows written per `INSERT` or `DELETE` of a bulk change: nine bound values
/// each, well inside what one statement may bind.
const BULK_INSERT_ROWS: usize = 1_000;

/// Repository for the links between tags and records.
pub struct RecordTagRepo;

impl RecordTagRepo {
    /// Put `tag_id` on the record `record_id` of `kind`. Returns whether the
    /// link was added: adding a tag a record already carries changes nothing.
    pub async fn add(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        tag_id: Uuid,
        kind: RecordKind,
        record_id: Uuid,
    ) -> Result<bool, OxidGeneError> {
        // The schema keeps the tag in the link's tree; the record is checked
        // here, since no key can name its tree for every kind.
        if !record_in_tree(db, kind, tree_id, record_id).await? {
            return Err(OxidGeneError::NotFound {
                entity: kind.as_str(),
                id: record_id,
            });
        }
        let column = column_of(kind);
        let mut model = record_tag::ActiveModel {
            id: Set(Uuid::now_v7()),
            tree_id: Set(tree_id),
            tag_id: Set(tag_id),
            created_at: Set(chrono::Utc::now()),
            ..Default::default()
        };
        set_record(&mut model, kind, record_id);
        // The conflict target is the partial unique index of the kind. `ON
        // CONFLICT DO NOTHING` keeps a concurrent duplicate from failing the
        // statement, which would abort a PostgreSQL transaction.
        let inserted = record_tag::Entity::insert(model)
            .on_conflict(
                OnConflict::columns([record_tag::Column::TagId, column])
                    .target_and_where(Expr::col(column).is_not_null())
                    .do_nothing()
                    .to_owned(),
            )
            .exec_without_returning(db)
            .await
            .map_err(db_err)?;
        Ok(inserted == 1)
    }

    /// Take `tag_id` off the record `record_id` of `kind`. Returns whether a
    /// link was removed.
    pub async fn remove(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        tag_id: Uuid,
        kind: RecordKind,
        record_id: Uuid,
    ) -> Result<bool, OxidGeneError> {
        let result = record_tag::Entity::delete_many()
            .filter(record_tag::Column::TreeId.eq(tree_id))
            .filter(record_tag::Column::TagId.eq(tag_id))
            .filter(column_of(kind).eq(record_id))
            .exec(db)
            .await
            .map_err(db_err)?;
        Ok(result.rows_affected > 0)
    }

    /// Move the tags of record `from` onto record `to`, both of `kind`, when a
    /// merge folds one into the other: `to` keeps its own tags and gains the
    /// others, each once, and `from` is left with none.
    pub async fn move_tags(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        kind: RecordKind,
        from: Uuid,
        to: Uuid,
    ) -> Result<(), OxidGeneError> {
        let tags = Self::tags_of(db, kind, &[from]).await?;
        for tag in tags.get(&from).into_iter().flatten() {
            Self::add(db, tree_id, tag.id, kind, to).await?;
        }
        record_tag::Entity::delete_many()
            .filter(record_tag::Column::TreeId.eq(tree_id))
            .filter(column_of(kind).eq(from))
            .exec(db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    /// Put `tag_id` on each person of `person_ids` that does not carry it yet.
    /// Returns the persons whose link was inserted by this call, in no
    /// particular order. The insert itself decides who was changed: a person
    /// that already carries the tag, or gains it from a concurrent writer
    /// before this insert reaches its row, is left out, so the count is exact.
    pub async fn add_persons(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        tag_id: Uuid,
        person_ids: &[Uuid],
    ) -> Result<Vec<Uuid>, OxidGeneError> {
        let person_ids = sorted_unique(person_ids.iter().copied());
        let mut added: Vec<Uuid> = Vec::with_capacity(person_ids.len());
        // Chunked so that one statement binds at most a few thousand values.
        for chunk in person_ids.chunks(BULK_INSERT_ROWS) {
            let models = chunk.iter().map(|person_id| {
                let mut model = record_tag::ActiveModel {
                    id: Set(Uuid::now_v7()),
                    tree_id: Set(tree_id),
                    tag_id: Set(tag_id),
                    created_at: Set(chrono::Utc::now()),
                    ..Default::default()
                };
                set_record(&mut model, RecordKind::Person, *person_id);
                model
            });
            // `ON CONFLICT DO NOTHING` keeps a concurrent duplicate from
            // failing the statement, as in `add`. `RETURNING` names only the
            // rows this statement inserted.
            let inserted = record_tag::Entity::insert_many(models)
                .on_conflict(
                    OnConflict::columns([record_tag::Column::TagId, record_tag::Column::PersonId])
                        .target_and_where(Expr::col(record_tag::Column::PersonId).is_not_null())
                        .do_nothing()
                        .to_owned(),
                )
                .exec_with_returning(db)
                .await
                .map_err(db_err)?;
            added.extend(inserted.into_iter().filter_map(|link| link.person_id));
        }
        Ok(added)
    }

    /// Take `tag_id` off each person of `person_ids` that carries it. Returns
    /// the persons whose link was removed, in no particular order. The
    /// `DELETE` names the rows it removed through `RETURNING`, so a person
    /// that a concurrent writer already untagged is left out, as in
    /// `add_persons`.
    pub async fn remove_persons(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        tag_id: Uuid,
        person_ids: &[Uuid],
    ) -> Result<Vec<Uuid>, OxidGeneError> {
        let person_ids = sorted_unique(person_ids.iter().copied());
        let mut removed: Vec<Uuid> = Vec::with_capacity(person_ids.len());
        // Chunked so that one statement binds at most a few thousand values.
        for chunk in person_ids.chunks(BULK_INSERT_ROWS) {
            let deleted = record_tag::Entity::delete_many()
                .filter(record_tag::Column::TreeId.eq(tree_id))
                .filter(record_tag::Column::TagId.eq(tag_id))
                .filter(record_tag::Column::PersonId.is_in(chunk.to_vec()))
                .exec_with_returning(db)
                .await
                .map_err(db_err)?;
            removed.extend(deleted.into_iter().filter_map(|link| link.person_id));
        }
        Ok(removed)
    }

    /// For each tag carried by at least one of `person_ids` in `tree_id`, how
    /// many of them carry it. A person carries a tag at most once.
    pub async fn person_tag_counts(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        person_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, i64>, OxidGeneError> {
        let rows: Vec<(Uuid,)> = in_chunks(person_ids, |chunk| async move {
            record_tag::Entity::find()
                .select_only()
                .column(record_tag::Column::TagId)
                .filter(record_tag::Column::TreeId.eq(tree_id))
                .filter(record_tag::Column::PersonId.is_in(chunk))
                .into_tuple::<(Uuid,)>()
                .all(db)
                .await
                .map_err(db_err)
        })
        .await?;
        let mut counts: HashMap<Uuid, i64> = HashMap::new();
        for (tag_id,) in rows {
            *counts.entry(tag_id).or_default() += 1;
        }
        Ok(counts)
    }

    /// The persons of `tree_id` carrying `tag_id`, by id, deleted ones
    /// included: the caller keeps the live ones.
    pub async fn persons_with_tag(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        tag_id: Uuid,
    ) -> Result<Vec<Uuid>, OxidGeneError> {
        let rows: Vec<(Option<Uuid>,)> = record_tag::Entity::find()
            .select_only()
            .column(record_tag::Column::PersonId)
            .filter(record_tag::Column::TreeId.eq(tree_id))
            .filter(record_tag::Column::TagId.eq(tag_id))
            .filter(record_tag::Column::PersonId.is_not_null())
            .into_tuple::<(Option<Uuid>,)>()
            .all(db)
            .await
            .map_err(db_err)?;
        Ok(rows.into_iter().filter_map(|(id,)| id).collect())
    }

    /// The tags of each of `record_ids`, all of one kind, sorted by folded
    /// label. A record without tags is absent from the map. One grouped query
    /// per bounded slice of ids, whatever their number.
    pub async fn tags_of(
        db: &impl ConnectionTrait,
        kind: RecordKind,
        record_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, Vec<TagRef>>, OxidGeneError> {
        let column = column_of(kind);
        let rows: Vec<(record_tag::Model, Option<tag::Model>)> =
            in_chunks(record_ids, |chunk| async move {
                record_tag::Entity::find()
                    .filter(column.is_in(chunk))
                    .find_also_related(tag::Entity)
                    .all(db)
                    .await
                    .map_err(db_err)
            })
            .await?;
        let mut tags: HashMap<Uuid, Vec<TagRef>> = HashMap::new();
        for (link, tag) in rows {
            let (Some(record_id), Some(tag)) = (record_id_of(&link, kind), tag) else {
                continue;
            };
            tags.entry(record_id).or_default().push(TagRef {
                id: tag.id,
                label: tag.label,
                color: tag.color,
            });
        }
        for list in tags.values_mut() {
            list.sort_by_cached_key(|tag| (fold_words(&tag.label), tag.id));
        }
        Ok(tags)
    }

    /// One page of the live records of `kind` that carry `tag_id` in `tree_id`,
    /// by id, after the cursor in `params`. `total_count` counts them all.
    pub async fn records_of_tag(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        tag_id: Uuid,
        kind: RecordKind,
        params: &PaginationParams,
    ) -> Result<Connection<Uuid>, OxidGeneError> {
        let column = column_of(kind);
        let limit = params.clamped_first();
        let after = params.decode_cursor()?;
        let base = || {
            let mut query = record_tag::Entity::find()
                .select_only()
                .column(column)
                .filter(record_tag::Column::TreeId.eq(tree_id))
                .filter(record_tag::Column::TagId.eq(tag_id))
                .filter(column.is_not_null());
            if let Some(live) = live_records(kind, tree_id) {
                query = query.filter(column.in_subquery(live));
            }
            query
        };

        let total_count = base().count(db).await.map_err(db_err)? as i64;
        let mut query = base();
        if let Some(after) = after {
            query = query.filter(column.gt(after));
        }
        let mut ids: Vec<Uuid> = query
            .order_by_asc(column)
            .limit(limit + 1)
            .into_tuple::<(Option<Uuid>,)>()
            .all(db)
            .await
            .map_err(db_err)?
            .into_iter()
            .filter_map(|(id,)| id)
            .collect();
        let has_next_page = ids.len() as u64 > limit;
        ids.truncate(limit as usize);
        let edges: Vec<Edge<Uuid>> = ids
            .into_iter()
            .map(|id| Edge {
                cursor: encode_cursor(&id),
                node: id,
            })
            .collect();
        Ok(Connection {
            page_info: PageInfo {
                has_next_page,
                end_cursor: edges.last().map(|edge| edge.cursor.clone()),
            },
            edges,
            total_count,
        })
    }
}

/// The `record_tag` column that names a record of `kind`.
pub(crate) fn column_of(kind: RecordKind) -> record_tag::Column {
    match kind {
        RecordKind::Person => record_tag::Column::PersonId,
        RecordKind::Family => record_tag::Column::FamilyId,
        RecordKind::Event => record_tag::Column::EventId,
        RecordKind::Place => record_tag::Column::PlaceId,
        RecordKind::Source => record_tag::Column::SourceId,
        RecordKind::Citation => record_tag::Column::CitationId,
        RecordKind::Repository => record_tag::Column::RepositoryId,
        RecordKind::Media => record_tag::Column::MediaId,
        RecordKind::Note => record_tag::Column::NoteId,
    }
}

/// The record a link names, when it names one of `kind`.
pub(crate) fn record_id_of(link: &record_tag::Model, kind: RecordKind) -> Option<Uuid> {
    match kind {
        RecordKind::Person => link.person_id,
        RecordKind::Family => link.family_id,
        RecordKind::Event => link.event_id,
        RecordKind::Place => link.place_id,
        RecordKind::Source => link.source_id,
        RecordKind::Citation => link.citation_id,
        RecordKind::Repository => link.repository_id,
        RecordKind::Media => link.media_id,
        RecordKind::Note => link.note_id,
    }
}

/// Set the column of `kind` on a new link.
fn set_record(model: &mut record_tag::ActiveModel, kind: RecordKind, record_id: Uuid) {
    let value = Set(Some(record_id));
    match kind {
        RecordKind::Person => model.person_id = value,
        RecordKind::Family => model.family_id = value,
        RecordKind::Event => model.event_id = value,
        RecordKind::Place => model.place_id = value,
        RecordKind::Source => model.source_id = value,
        RecordKind::Citation => model.citation_id = value,
        RecordKind::Repository => model.repository_id = value,
        RecordKind::Media => model.media_id = value,
        RecordKind::Note => model.note_id = value,
    }
}

/// Whether the record `record_id` of `kind` exists in `tree_id`, deleted or
/// not. A citation has no tree of its own: it belongs to the tree of its
/// source.
async fn record_in_tree(
    db: &impl ConnectionTrait,
    kind: RecordKind,
    tree_id: Uuid,
    record_id: Uuid,
) -> Result<bool, OxidGeneError> {
    let mut query = Query::select();
    query.column(Alias::new("id"));
    match kind {
        RecordKind::Citation => {
            let sources = Query::select()
                .column(Alias::new("id"))
                .from(Alias::new("source"))
                .and_where(Expr::col(Alias::new("tree_id")).eq(tree_id))
                .to_owned();
            query
                .from(Alias::new("citation"))
                .and_where(Expr::col(Alias::new("source_id")).in_subquery(sources));
        }
        _ => {
            query
                .from(Alias::new(kind.as_str()))
                .and_where(Expr::col(Alias::new("tree_id")).eq(tree_id));
        }
    }
    query.and_where(Expr::col(Alias::new("id")).eq(record_id));
    let found = db.query_one(&query).await.map_err(db_err)?;
    Ok(found.is_some())
}

/// The ids of the live records of `kind` in `tree_id`, as a subquery, or
/// `None` when the kind has no soft-deleted state to exclude: a place is
/// removed outright, and its links go with it. A citation has no state of its
/// own: it is live while its source is, since a source's soft delete leaves
/// its citations in place.
pub(crate) fn live_records(kind: RecordKind, tree_id: Uuid) -> Option<SelectStatement> {
    let table = match kind {
        RecordKind::Person
        | RecordKind::Family
        | RecordKind::Event
        | RecordKind::Source
        | RecordKind::Repository
        | RecordKind::Media
        | RecordKind::Note => kind.as_str(),
        RecordKind::Citation => return Some(live_citations(tree_id)),
        RecordKind::Place => return None,
    };
    Some(
        Query::select()
            .column(Alias::new("id"))
            .from(Alias::new(table))
            .and_where(Expr::col(Alias::new("tree_id")).eq(tree_id))
            .and_where(Expr::col(Alias::new("deleted_at")).is_null())
            .to_owned(),
    )
}

/// The ids of the citations of `tree_id` whose source is live.
fn live_citations(tree_id: Uuid) -> SelectStatement {
    Query::select()
        .column((Alias::new("citation"), Alias::new("id")))
        .from(Alias::new("citation"))
        .inner_join(
            Alias::new("source"),
            Expr::col((Alias::new("citation"), Alias::new("source_id")))
                .equals((Alias::new("source"), Alias::new("id"))),
        )
        .and_where(Expr::col((Alias::new("source"), Alias::new("tree_id"))).eq(tree_id))
        .and_where(Expr::col((Alias::new("source"), Alias::new("deleted_at"))).is_null())
        .to_owned()
}
