//! Repository for `Family` entities (CRUD).

use chrono::Utc;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::{Connection, Family, KeptEntityKind, RecordKind};
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveModelTrait, ConnectionTrait, IntoActiveModel, QueryFilter, QuerySelect, Set};
use uuid::Uuid;

use crate::entities::event;
use crate::entities::family::{self, ActiveModel, Column, Entity};
use crate::repo::batch::in_chunks;
use crate::repo::pagination::{PaginationParams, paginate};
use crate::repo::record_tag::RecordTagRepo;
use crate::repo::{GedcomKeptRepo, VignetteRepo, db_err};

/// Repository for family CRUD operations.
pub struct FamilyRepo;

impl FamilyRepo {
    /// List families in a tree with pagination.
    pub async fn list(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        params: &PaginationParams,
    ) -> Result<Connection<Family>, OxidGeneError> {
        let query = Entity::find().filter(Column::TreeId.eq(tree_id));
        let mut page = paginate(db, query, Column::Id, params, |m| (m.id, into_domain(m))).await?;
        let mut families: Vec<&mut Family> =
            page.edges.iter_mut().map(|edge| &mut edge.node).collect();
        hydrate_tags(db, &mut families).await?;
        Ok(page)
    }

    /// How many families each of `tree_ids` holds; a tree without any is
    /// absent.
    pub async fn count_by_trees(
        db: &impl ConnectionTrait,
        tree_ids: &[Uuid],
    ) -> Result<Vec<(Uuid, i64)>, OxidGeneError> {
        in_chunks(tree_ids, |chunk| async move {
            Entity::find()
                .select_only()
                .column(Column::TreeId)
                .column_as(Column::Id.count(), "count")
                .filter(Column::TreeId.is_in(chunk))
                .group_by(Column::TreeId)
                .into_tuple()
                .all(db)
                .await
                .map_err(db_err)
        })
        .await
    }

    /// The families among `ids`, in no particular order.
    pub async fn get_many(
        db: &impl ConnectionTrait,
        ids: &[Uuid],
    ) -> Result<Vec<Family>, OxidGeneError> {
        in_chunks(ids, |chunk| async move {
            let models = Entity::find()
                .filter(Column::Id.is_in(chunk))
                .all(db)
                .await
                .map_err(db_err)?;
            let mut families: Vec<Family> = models.into_iter().map(into_domain).collect();
            hydrate_all_tags(db, &mut families).await?;
            Ok(families)
        })
        .await
    }

    /// List all families in a tree without pagination.
    pub async fn list_all(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<Family>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .all(db)
            .await
            .map_err(db_err)?;
        let mut families: Vec<Family> = models.into_iter().map(into_domain).collect();
        hydrate_all_tags(db, &mut families).await?;
        Ok(families)
    }

    /// Get a single family by ID.
    pub async fn get(db: &impl ConnectionTrait, id: Uuid) -> Result<Family, OxidGeneError> {
        let mut family = find_row(db, id).await.map(into_domain)?;
        hydrate_tags(db, &mut [&mut family]).await?;
        Ok(family)
    }

    /// Create a new family.
    pub async fn create(
        db: &impl ConnectionTrait,
        id: Uuid,
        tree_id: Uuid,
    ) -> Result<Family, OxidGeneError> {
        let now = Utc::now();
        let model = family::ActiveModel {
            id: Set(id),
            tree_id: Set(tree_id),
            privacy: Set(oxidgene_core::enums::Privacy::default().into()),
            created_at: Set(now),
            updated_at: Set(now),
        };
        let result = model.insert(db).await.map_err(db_err)?;
        Ok(into_domain(result))
    }

    /// Update a family: its privacy, and `updated_at` either way.
    pub async fn update(
        db: &impl ConnectionTrait,
        id: Uuid,
        privacy: Option<oxidgene_core::enums::Privacy>,
    ) -> Result<Family, OxidGeneError> {
        let existing = find_row(db, id).await?;

        let mut active: ActiveModel = existing.into_active_model();
        if let Some(privacy) = privacy {
            active.privacy = Set(privacy.into());
        }
        active.updated_at = Set(Utc::now());

        let result = active.update(db).await.map_err(db_err)?;
        let mut family = into_domain(result);
        hydrate_tags(db, &mut [&mut family]).await?;
        Ok(family)
    }

    /// Delete a family, and with it every row that is its alone
    /// (docs/data-model.md, Deletion).
    ///
    /// The schema's cascades take its spouse and child links with the
    /// spouses' ages, its own events with what hangs off them, its notes,
    /// citations, media links and tags; an individual event naming it as
    /// its related family stops doing so. The identification boxes standing
    /// as evidence for one of its events lose that attribution, and its kept
    /// GEDCOM goes, first, in the same transaction. Its members stay.
    pub async fn delete(db: &impl ConnectionTrait, id: Uuid) -> Result<(), OxidGeneError> {
        find_row(db, id).await?;
        let events: Vec<Uuid> = event::Entity::find()
            .select_only()
            .column(event::Column::Id)
            .filter(event::Column::FamilyId.eq(id))
            .into_tuple()
            .all(db)
            .await
            .map_err(db_err)?;
        VignetteRepo::detach_events(db, &events).await?;
        GedcomKeptRepo::delete_for(db, KeptEntityKind::Family, &[id]).await?;
        Entity::delete_by_id(id).exec(db).await.map_err(db_err)?;
        Ok(())
    }
}

fn into_domain(m: family::Model) -> Family {
    Family {
        id: m.id,
        tree_id: m.tree_id,
        privacy: m.privacy.into(),
        tags: Vec::new(),
        created_at: m.created_at,
        updated_at: m.updated_at,
    }
}

/// Row `id`; `NotFound` when it is missing.
async fn find_row(
    db: &impl ConnectionTrait,
    id: Uuid,
) -> Result<<Entity as EntityTrait>::Model, OxidGeneError> {
    crate::repo::find_row::<Entity>(db, id, "Family").await
}

/// Fill each couple's tags from one grouped read.
async fn hydrate_tags(
    db: &impl ConnectionTrait,
    families: &mut [&mut Family],
) -> Result<(), OxidGeneError> {
    let ids: Vec<Uuid> = families.iter().map(|family| family.id).collect();
    let mut tags = RecordTagRepo::tags_of(db, RecordKind::Family, &ids).await?;
    for family in families {
        family.tags = tags.remove(&family.id).unwrap_or_default();
    }
    Ok(())
}

/// [`hydrate_tags`] for a list of couples.
async fn hydrate_all_tags(
    db: &impl ConnectionTrait,
    families: &mut [Family],
) -> Result<(), OxidGeneError> {
    let mut refs: Vec<&mut Family> = families.iter_mut().collect();
    hydrate_tags(db, &mut refs).await
}
