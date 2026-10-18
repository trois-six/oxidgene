//! A tree's tag vocabulary: creating, renaming, recolouring, merging and
//! deleting tags, and counting the live records each one is put on.

use std::collections::HashMap;

use chrono::Utc;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::search::fold_words;
use oxidgene_core::types::{RecordKind, TAG_COLOR_SLOTS, TAG_LABEL_MAX};
use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{Expr, OnConflict, Query};
use sea_orm::{ConnectionTrait, EntityTrait, QueryFilter, QuerySelect, Set};
use uuid::Uuid;

use crate::entities::{record_tag, tag};
use crate::repo::db_err;
use crate::repo::record_tag::{column_of, live_records};

/// A tag of the vocabulary and the number of live records of each kind that
/// carry it, in [`RecordKind::ALL`] order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagUsage {
    pub tag: tag::Model,
    pub counts: Vec<(RecordKind, i64)>,
}

impl TagUsage {
    /// The number of live records of `kind` carrying the tag.
    pub fn count(&self, kind: RecordKind) -> i64 {
        self.counts
            .iter()
            .find(|(k, _)| *k == kind)
            .map_or(0, |(_, count)| *count)
    }
}

/// Repository for the tags of a tree.
pub struct TagRepo;

impl TagRepo {
    /// Every tag of the tree with its per-kind counts, sorted by folded label.
    pub async fn list(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<TagUsage>, OxidGeneError> {
        let mut tags = tag::Entity::find()
            .filter(tag::Column::TreeId.eq(tree_id))
            .all(db)
            .await
            .map_err(db_err)?;
        tags.sort_by_cached_key(|tag| (fold_words(&tag.label), tag.id));

        let mut counts: HashMap<(Uuid, RecordKind), i64> = HashMap::new();
        for kind in RecordKind::ALL {
            let mut query = record_tag::Entity::find()
                .select_only()
                .column(record_tag::Column::TagId)
                .column_as(record_tag::Column::Id.count(), "uses")
                .filter(record_tag::Column::TreeId.eq(tree_id))
                .filter(column_of(kind).is_not_null());
            if let Some(live) = live_records(kind, tree_id) {
                query = query.filter(column_of(kind).in_subquery(live));
            }
            let rows: Vec<(Uuid, i64)> = query
                .group_by(record_tag::Column::TagId)
                .into_tuple()
                .all(db)
                .await
                .map_err(db_err)?;
            for (tag_id, uses) in rows {
                counts.insert((tag_id, kind), uses);
            }
        }

        Ok(tags
            .into_iter()
            .map(|tag| {
                let counts = RecordKind::ALL
                    .into_iter()
                    .map(|kind| (kind, counts.get(&(tag.id, kind)).copied().unwrap_or(0)))
                    .collect();
                TagUsage { tag, counts }
            })
            .collect())
    }

    /// The tag `id` of the tree, or `NotFound`.
    pub async fn get(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        id: Uuid,
    ) -> Result<tag::Model, OxidGeneError> {
        tag::Entity::find_by_id(id)
            .filter(tag::Column::TreeId.eq(tree_id))
            .one(db)
            .await
            .map_err(db_err)?
            .ok_or(OxidGeneError::NotFound { entity: "Tag", id })
    }

    /// The tag of the tree whose label folds to `label`'s, if there is one.
    pub async fn find_by_label(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        label: &str,
    ) -> Result<Option<tag::Model>, OxidGeneError> {
        Self::find_by_normalized(db, tree_id, &fold_words(label.trim())).await
    }

    async fn find_by_normalized(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        normalized: &str,
    ) -> Result<Option<tag::Model>, OxidGeneError> {
        tag::Entity::find()
            .filter(tag::Column::TreeId.eq(tree_id))
            .filter(tag::Column::NormalizedLabel.eq(normalized))
            .one(db)
            .await
            .map_err(db_err)
    }

    /// The tag for `label`, creating it when no tag folds to the same text.
    /// Returns the tag and whether this call created it. A new tag takes the
    /// colour the fewest of the tree's tags use, the lowest slot on a tie.
    pub async fn create(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        label: &str,
    ) -> Result<(tag::Model, bool), OxidGeneError> {
        let label = checked_label(label)?;
        let normalized = fold_words(&label);
        if let Some(existing) = Self::find_by_normalized(db, tree_id, &normalized).await? {
            return Ok((existing, false));
        }
        let color = Self::least_used_color(db, tree_id).await?;
        let now = Utc::now();
        let model = tag::ActiveModel {
            id: Set(Uuid::now_v7()),
            tree_id: Set(tree_id),
            normalized_label: Set(normalized.clone()),
            label: Set(label),
            color: Set(color),
            created_at: Set(now),
            updated_at: Set(now),
        };
        // `ON CONFLICT DO NOTHING` rather than a caught unique violation: a
        // failed statement would abort a PostgreSQL transaction, and the
        // re-read below must still run inside it.
        let inserted = tag::Entity::insert(model)
            .on_conflict(
                OnConflict::columns([tag::Column::TreeId, tag::Column::NormalizedLabel])
                    .do_nothing()
                    .to_owned(),
            )
            .exec_without_returning(db)
            .await
            .map_err(db_err)?;
        // Zero rows inserted means a concurrent writer created the same tag
        // between the lookup and the insert: take theirs.
        let tag = Self::find_by_normalized(db, tree_id, &normalized)
            .await?
            .ok_or_else(|| {
                OxidGeneError::Internal("a tag vanished during its own creation".into())
            })?;
        Ok((tag, inserted == 1))
    }

    /// Rename and/or recolour a tag. A label folding to another tag's is a
    /// conflict: joining two tags is a [`Self::merge`].
    pub async fn update(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        id: Uuid,
        label: Option<&str>,
        color: Option<i16>,
    ) -> Result<tag::Model, OxidGeneError> {
        let current = Self::get(db, tree_id, id).await?;
        let mut active: tag::ActiveModel = current.into();
        if let Some(label) = label {
            let label = checked_label(label)?;
            let normalized = fold_words(&label);
            if let Some(other) = Self::find_by_normalized(db, tree_id, &normalized).await?
                && other.id != id
            {
                return Err(OxidGeneError::Conflict(
                    "a tag with this label already exists".into(),
                ));
            }
            active.normalized_label = Set(normalized);
            active.label = Set(label);
        }
        if let Some(color) = color {
            active.color = Set(checked_color(color)?);
        }
        active.updated_at = Set(Utc::now());
        active.update(db).await.map_err(db_err)
    }

    /// Move every link of tag `from` onto tag `into`, dropping the links
    /// `into` already has, then delete `from`. Returns the number of links
    /// moved.
    pub async fn merge(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        from: Uuid,
        into: Uuid,
    ) -> Result<u64, OxidGeneError> {
        if from == into {
            return Err(OxidGeneError::Validation(
                "a tag cannot be merged into itself".into(),
            ));
        }
        Self::get(db, tree_id, from).await?;
        Self::get(db, tree_id, into).await?;

        for kind in RecordKind::ALL {
            let column = column_of(kind);
            let already_on_target = Query::select()
                .column(column)
                .from(record_tag::Entity)
                .and_where(record_tag::Column::TagId.eq(into))
                .and_where(column.is_not_null())
                .to_owned();
            record_tag::Entity::delete_many()
                .filter(record_tag::Column::TagId.eq(from))
                .filter(column.in_subquery(already_on_target))
                .exec(db)
                .await
                .map_err(db_err)?;
        }
        let moved = record_tag::Entity::update_many()
            .col_expr(record_tag::Column::TagId, Expr::value(into))
            .filter(record_tag::Column::TagId.eq(from))
            .exec(db)
            .await
            .map_err(db_err)?
            .rows_affected;
        tag::Entity::delete_by_id(from)
            .exec(db)
            .await
            .map_err(db_err)?;
        Ok(moved)
    }

    /// Delete a tag and every link to it. Returns the number of links removed.
    pub async fn delete(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        id: Uuid,
    ) -> Result<u64, OxidGeneError> {
        Self::get(db, tree_id, id).await?;
        let removed = record_tag::Entity::delete_many()
            .filter(record_tag::Column::TagId.eq(id))
            .exec(db)
            .await
            .map_err(db_err)?
            .rows_affected;
        tag::Entity::delete_by_id(id)
            .exec(db)
            .await
            .map_err(db_err)?;
        Ok(removed)
    }

    /// The palette slot the fewest of the tree's tags use; the lowest on a tie.
    async fn least_used_color(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<i16, OxidGeneError> {
        let colors: Vec<i16> = tag::Entity::find()
            .select_only()
            .column(tag::Column::Color)
            .filter(tag::Column::TreeId.eq(tree_id))
            .into_tuple()
            .all(db)
            .await
            .map_err(db_err)?;
        let mut uses = vec![0usize; TAG_COLOR_SLOTS as usize];
        for color in colors {
            if let Ok(slot) = usize::try_from(color)
                && let Some(count) = uses.get_mut(slot)
            {
                *count += 1;
            }
        }
        let slot = (0..TAG_COLOR_SLOTS as usize)
            .min_by_key(|slot| uses[*slot])
            .unwrap_or(0);
        Ok(slot as i16)
    }
}

/// `label` trimmed, when it is 1 to [`TAG_LABEL_MAX`] characters long.
pub fn checked_label(label: &str) -> Result<String, OxidGeneError> {
    let trimmed = label.trim();
    let length = trimmed.chars().count();
    if length == 0 || length > TAG_LABEL_MAX {
        return Err(OxidGeneError::Validation(format!(
            "a tag label is 1 to {TAG_LABEL_MAX} characters long"
        )));
    }
    Ok(trimmed.to_string())
}

/// `color` when it is a palette slot.
pub fn checked_color(color: i16) -> Result<i16, OxidGeneError> {
    if (0..TAG_COLOR_SLOTS).contains(&color) {
        Ok(color)
    } else {
        Err(OxidGeneError::Validation(format!(
            "a tag colour is between 0 and {}",
            TAG_COLOR_SLOTS - 1
        )))
    }
}
