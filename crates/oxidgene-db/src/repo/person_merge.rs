//! Moves everything that refers to one person onto another.
//!
//! This is the storage half of merging two records of the same individual:
//! every row naming the duplicate is re-pointed at the person that is kept,
//! and rows that would then say the same thing twice are dropped instead.
//! Deciding *whether* two persons may be merged is the caller's business; so
//! is soft-deleting the duplicate and refreshing projections afterwards.

use std::collections::HashSet;

use chrono::Utc;
use oxidgene_core::error::OxidGeneError;
use sea_orm::entity::prelude::*;
use sea_orm::sea_query::Expr;
use sea_orm::{ConnectionTrait, IntoActiveModel, QueryFilter, Set};
use uuid::Uuid;

use crate::entities::sea_enums;
use crate::entities::{
    citation, event, event_witness, family_child, family_spouse, media_link, note, person,
    person_name, tree, vignette,
};
use crate::repo::person_distinct::PersonDistinctRepo;

/// Re-points a duplicate person's rows at the person kept in its place.
pub struct PersonMergeRepo;

impl PersonMergeRepo {
    /// Move every row referring to `duplicate` onto `kept`.
    ///
    /// - **Names**: the kept person's primary name stays primary; the
    ///   duplicate's names follow as secondary names, except one identical to
    ///   a name the kept person already bears.
    /// - **Sex and portrait**: the kept person's win; the duplicate's fill
    ///   them only where the kept person has none.
    /// - **Events, notes, citations, identification boxes**: re-pointed.
    /// - **Family links, witnesses, media links**: re-pointed, except a link
    ///   the kept person already has — the same family in the same role, the
    ///   same event, the same media — which is dropped rather than doubled. A
    ///   witness row on the kept person's own event is dropped as well: nobody
    ///   witnesses their own birth.
    /// - **Tree roots** and **distinct-person confirmations** move with them.
    ///
    /// The duplicate itself is left in place, and is the caller's to delete.
    pub async fn absorb(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        kept: Uuid,
        duplicate: Uuid,
    ) -> Result<(), OxidGeneError> {
        Self::absorb_person_fields(db, kept, duplicate).await?;
        Self::absorb_names(db, kept, duplicate).await?;
        Self::absorb_family_links(db, kept, duplicate).await?;
        Self::absorb_witnesses(db, kept, duplicate).await?;
        Self::absorb_media_links(db, kept, duplicate).await?;

        repoint(
            db,
            event::Entity::update_many().filter(event::Column::PersonId.eq(duplicate)),
            event::Column::PersonId,
            kept,
        )
        .await?;
        repoint(
            db,
            note::Entity::update_many().filter(note::Column::PersonId.eq(duplicate)),
            note::Column::PersonId,
            kept,
        )
        .await?;
        repoint(
            db,
            citation::Entity::update_many().filter(citation::Column::PersonId.eq(duplicate)),
            citation::Column::PersonId,
            kept,
        )
        .await?;
        repoint(
            db,
            vignette::Entity::update_many().filter(vignette::Column::PersonId.eq(duplicate)),
            vignette::Column::PersonId,
            kept,
        )
        .await?;
        repoint(
            db,
            tree::Entity::update_many().filter(tree::Column::SosaRootPersonId.eq(duplicate)),
            tree::Column::SosaRootPersonId,
            kept,
        )
        .await?;
        repoint(
            db,
            tree::Entity::update_many().filter(tree::Column::SelfPersonId.eq(duplicate)),
            tree::Column::SelfPersonId,
            kept,
        )
        .await?;

        PersonDistinctRepo::transfer(db, tree_id, kept, duplicate).await
    }

    async fn absorb_person_fields(
        db: &impl ConnectionTrait,
        kept: Uuid,
        duplicate: Uuid,
    ) -> Result<(), OxidGeneError> {
        let (Some(kept_row), Some(duplicate_row)) = (
            person::Entity::find_by_id(kept)
                .one(db)
                .await
                .map_err(database)?,
            person::Entity::find_by_id(duplicate)
                .one(db)
                .await
                .map_err(database)?,
        ) else {
            return Err(OxidGeneError::NotFound {
                entity: "Person",
                id: kept,
            });
        };

        let takes_sex =
            kept_row.sex == sea_enums::Sex::Unknown && duplicate_row.sex != sea_enums::Sex::Unknown;
        let takes_portrait = kept_row.portrait_media_id.is_none()
            && kept_row.portrait_vignette_id.is_none()
            && (duplicate_row.portrait_media_id.is_some()
                || duplicate_row.portrait_vignette_id.is_some());
        if !takes_sex && !takes_portrait {
            return Ok(());
        }

        let mut active = kept_row.into_active_model();
        if takes_sex {
            active.sex = Set(duplicate_row.sex);
        }
        if takes_portrait {
            active.portrait_media_id = Set(duplicate_row.portrait_media_id);
            active.portrait_vignette_id = Set(duplicate_row.portrait_vignette_id);
        }
        active.updated_at = Set(Utc::now());
        active.update(db).await.map_err(database)?;
        Ok(())
    }

    async fn absorb_names(
        db: &impl ConnectionTrait,
        kept: Uuid,
        duplicate: Uuid,
    ) -> Result<(), OxidGeneError> {
        let kept_names = person_name::Entity::find()
            .filter(person_name::Column::PersonId.eq(kept))
            .all(db)
            .await
            .map_err(database)?;
        let mut duplicate_names = person_name::Entity::find()
            .filter(person_name::Column::PersonId.eq(duplicate))
            .all(db)
            .await
            .map_err(database)?;
        // Primary first, then in the duplicate's own order, so that when the
        // kept person has no name at all the duplicate's primary stays primary.
        duplicate_names.sort_by_key(|name| (!name.is_primary, name.sort_order));

        let mut borne: Vec<person_name::Model> = kept_names;
        let mut has_primary = borne.iter().any(|name| name.is_primary);
        let mut next_order = borne.iter().map(|name| name.sort_order).max().unwrap_or(-1) + 1;

        for name in duplicate_names {
            if borne.iter().any(|existing| same_name(existing, &name)) {
                person_name::Entity::delete_by_id(name.id)
                    .exec(db)
                    .await
                    .map_err(database)?;
                continue;
            }
            let primary = name.is_primary && !has_primary;
            has_primary |= primary;
            let mut active = name.clone().into_active_model();
            active.person_id = Set(kept);
            active.is_primary = Set(primary);
            active.sort_order = Set(next_order);
            active.updated_at = Set(Utc::now());
            let moved = active.update(db).await.map_err(database)?;
            next_order += 1;
            borne.push(moved);
        }
        Ok(())
    }

    async fn absorb_family_links(
        db: &impl ConnectionTrait,
        kept: Uuid,
        duplicate: Uuid,
    ) -> Result<(), OxidGeneError> {
        let kept_spouse_of: HashSet<Uuid> = family_spouse::Entity::find()
            .filter(family_spouse::Column::PersonId.eq(kept))
            .all(db)
            .await
            .map_err(database)?
            .into_iter()
            .map(|link| link.family_id)
            .collect();
        for link in family_spouse::Entity::find()
            .filter(family_spouse::Column::PersonId.eq(duplicate))
            .all(db)
            .await
            .map_err(database)?
        {
            if kept_spouse_of.contains(&link.family_id) {
                family_spouse::Entity::delete_by_id(link.id)
                    .exec(db)
                    .await
                    .map_err(database)?;
            } else {
                let mut active = link.into_active_model();
                active.person_id = Set(kept);
                active.update(db).await.map_err(database)?;
            }
        }

        let kept_child_of: HashSet<Uuid> = family_child::Entity::find()
            .filter(family_child::Column::PersonId.eq(kept))
            .all(db)
            .await
            .map_err(database)?
            .into_iter()
            .map(|link| link.family_id)
            .collect();
        for link in family_child::Entity::find()
            .filter(family_child::Column::PersonId.eq(duplicate))
            .all(db)
            .await
            .map_err(database)?
        {
            if kept_child_of.contains(&link.family_id) {
                family_child::Entity::delete_by_id(link.id)
                    .exec(db)
                    .await
                    .map_err(database)?;
            } else {
                let mut active = link.into_active_model();
                active.person_id = Set(kept);
                active.update(db).await.map_err(database)?;
            }
        }
        Ok(())
    }

    async fn absorb_witnesses(
        db: &impl ConnectionTrait,
        kept: Uuid,
        duplicate: Uuid,
    ) -> Result<(), OxidGeneError> {
        let mut witnessed: HashSet<Uuid> = event_witness::Entity::find()
            .filter(event_witness::Column::PersonId.eq(kept))
            .all(db)
            .await
            .map_err(database)?
            .into_iter()
            .map(|row| row.event_id)
            .collect();
        // Every event is the kept person's by now or will be: the duplicate's
        // own events move to them, so those count as theirs too.
        let own: HashSet<Uuid> = event::Entity::find()
            .filter(
                sea_orm::Condition::any()
                    .add(event::Column::PersonId.eq(kept))
                    .add(event::Column::PersonId.eq(duplicate)),
            )
            .all(db)
            .await
            .map_err(database)?
            .into_iter()
            .map(|row| row.id)
            .collect();

        for row in event_witness::Entity::find()
            .filter(event_witness::Column::PersonId.eq(duplicate))
            .all(db)
            .await
            .map_err(database)?
        {
            if witnessed.contains(&row.event_id) || own.contains(&row.event_id) {
                event_witness::Entity::delete_by_id(row.id)
                    .exec(db)
                    .await
                    .map_err(database)?;
            } else {
                witnessed.insert(row.event_id);
                let mut active = row.into_active_model();
                active.person_id = Set(kept);
                active.update(db).await.map_err(database)?;
            }
        }

        // The kept person may have witnessed an event of the duplicate's,
        // which is now their own.
        event_witness::Entity::delete_many()
            .filter(event_witness::Column::PersonId.eq(kept))
            .filter(event_witness::Column::EventId.is_in(own))
            .exec(db)
            .await
            .map_err(database)?;
        Ok(())
    }

    async fn absorb_media_links(
        db: &impl ConnectionTrait,
        kept: Uuid,
        duplicate: Uuid,
    ) -> Result<(), OxidGeneError> {
        let mut linked: HashSet<Uuid> = media_link::Entity::find()
            .filter(media_link::Column::PersonId.eq(kept))
            .all(db)
            .await
            .map_err(database)?
            .into_iter()
            .map(|link| link.media_id)
            .collect();
        for link in media_link::Entity::find()
            .filter(media_link::Column::PersonId.eq(duplicate))
            .all(db)
            .await
            .map_err(database)?
        {
            if linked.contains(&link.media_id) {
                media_link::Entity::delete_by_id(link.id)
                    .exec(db)
                    .await
                    .map_err(database)?;
            } else {
                linked.insert(link.media_id);
                let mut active = link.into_active_model();
                active.person_id = Set(Some(kept));
                active.update(db).await.map_err(database)?;
            }
        }
        Ok(())
    }
}

/// Whether two rows record the same name: every piece equal, type included.
fn same_name(a: &person_name::Model, b: &person_name::Model) -> bool {
    a.name_type == b.name_type
        && a.given_names == b.given_names
        && a.surname == b.surname
        && a.surname_prefix == b.surname_prefix
        && a.prefix == b.prefix
        && a.suffix == b.suffix
        && a.nickname == b.nickname
}

async fn repoint<E: EntityTrait>(
    db: &impl ConnectionTrait,
    update: sea_orm::UpdateMany<E>,
    column: E::Column,
    kept: Uuid,
) -> Result<(), OxidGeneError> {
    update
        .col_expr(column, Expr::value(kept))
        .exec(db)
        .await
        .map_err(database)?;
    Ok(())
}

fn database(error: DbErr) -> OxidGeneError {
    OxidGeneError::Database(error.to_string())
}
