//! Repository for `person_distinct`: same-named persons confirmed to be
//! different people.
//!
//! A pair is one row, stored with the lower id first, so every read and write
//! goes through [`ordered`] and the answer never depends on which of the two
//! persons asked.

use std::collections::HashSet;

use chrono::Utc;
use oxidgene_core::collections::sorted_unique;
use oxidgene_core::error::OxidGeneError;
use sea_orm::entity::prelude::*;
use sea_orm::{Condition, ConnectionTrait, QueryFilter, Set};
use uuid::Uuid;

use crate::entities::person_distinct::{self, Column, Entity};
use crate::repo::db_err;

/// Repository for distinct-person confirmations.
pub struct PersonDistinctRepo;

impl PersonDistinctRepo {
    /// Record that `person_id` is a different person from each of `others`.
    ///
    /// Idempotent: a pair already recorded is left as it is, and `person_id`
    /// itself is ignored if it appears among `others`.
    pub async fn mark(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        person_id: Uuid,
        others: &[Uuid],
    ) -> Result<(), OxidGeneError> {
        let known = Self::distinct_from(db, person_id).await?;
        let missing = sorted_unique(
            others
                .iter()
                .copied()
                .filter(|other| *other != person_id && !known.contains(other)),
        );
        if missing.is_empty() {
            return Ok(());
        }

        let now = Utc::now();
        let rows = missing.into_iter().map(|other| {
            let (low, high) = ordered(person_id, other);
            person_distinct::ActiveModel {
                id: Set(Uuid::now_v7()),
                tree_id: Set(tree_id),
                person_id: Set(low),
                other_person_id: Set(high),
                created_at: Set(now),
            }
        });
        Entity::insert_many(rows).exec(db).await.map_err(db_err)?;
        Ok(())
    }

    /// Everyone `person_id` has been confirmed to be different from.
    pub async fn distinct_from(
        db: &impl ConnectionTrait,
        person_id: Uuid,
    ) -> Result<HashSet<Uuid>, OxidGeneError> {
        let rows = Entity::find()
            .filter(
                Condition::any()
                    .add(Column::PersonId.eq(person_id))
                    .add(Column::OtherPersonId.eq(person_id)),
            )
            .all(db)
            .await
            .map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|row| {
                if row.person_id == person_id {
                    row.other_person_id
                } else {
                    row.person_id
                }
            })
            .collect())
    }

    /// Every pair of a tree confirmed to be different people, lower id first.
    pub async fn pairs_in_tree(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<HashSet<(Uuid, Uuid)>, OxidGeneError> {
        let rows = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .all(db)
            .await
            .map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|row| ordered(row.person_id, row.other_person_id))
            .collect())
    }

    /// Hand every confirmation `duplicate` holds over to `kept`.
    ///
    /// Used when `duplicate` turns out to be `kept`: whoever the one was known
    /// to differ from, so does the other. The pair joining the two, if any, is
    /// dropped — it was a statement the merge has just overruled.
    pub async fn transfer(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        kept: Uuid,
        duplicate: Uuid,
    ) -> Result<(), OxidGeneError> {
        let inherited: Vec<Uuid> = Self::distinct_from(db, duplicate)
            .await?
            .into_iter()
            .filter(|other| *other != kept)
            .collect();
        Entity::delete_many()
            .filter(
                Condition::any()
                    .add(Column::PersonId.eq(duplicate))
                    .add(Column::OtherPersonId.eq(duplicate)),
            )
            .exec(db)
            .await
            .map_err(db_err)?;
        Self::mark(db, tree_id, kept, &inherited).await
    }
}

/// A pair as it is stored: lower id first.
fn ordered(a: Uuid, b: Uuid) -> (Uuid, Uuid) {
    if a < b { (a, b) } else { (b, a) }
}
