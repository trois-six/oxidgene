//! Repository for `FamilySpouse` junction table (create/delete only).

use oxidgene_core::enums::SpouseRole;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::FamilySpouse;
use sea_orm::entity::prelude::*;
use sea_orm::{ConnectionTrait, QueryFilter, Set};
use uuid::Uuid;

use crate::entities::family;
use crate::entities::family_spouse::{self, Column, Entity};
use crate::entities::sea_enums;
use crate::repo::batch::in_chunks;
use crate::repo::db_err;

/// Repository for family–spouse membership.
pub struct FamilySpouseRepo;

impl FamilySpouseRepo {
    /// Get one family–spouse link.
    pub async fn get(db: &impl ConnectionTrait, id: Uuid) -> Result<FamilySpouse, OxidGeneError> {
        Entity::find_by_id(id)
            .one(db)
            .await
            .map_err(db_err)?
            .map(into_domain)
            .ok_or(OxidGeneError::NotFound {
                entity: "FamilySpouse",
                id,
            })
    }

    /// List spouses in a family.
    pub async fn list_by_family(
        db: &impl ConnectionTrait,
        family_id: Uuid,
    ) -> Result<Vec<FamilySpouse>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::FamilyId.eq(family_id))
            .all(db)
            .await
            .map_err(db_err)?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// List spouses for multiple families.
    pub async fn list_by_families(
        db: &impl ConnectionTrait,
        family_ids: &[Uuid],
    ) -> Result<Vec<FamilySpouse>, OxidGeneError> {
        in_chunks(family_ids, |chunk| async move {
            let models = Entity::find()
                .filter(Column::FamilyId.is_in(chunk))
                .all(db)
                .await
                .map_err(db_err)?;
            Ok(models.into_iter().map(into_domain).collect())
        })
        .await
    }

    /// List the spouse links of these families of one tree.
    pub async fn list_by_families_in_tree(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        family_ids: &[Uuid],
    ) -> Result<Vec<FamilySpouse>, OxidGeneError> {
        in_chunks(family_ids, |chunk| async move {
            let models = Entity::find()
                .filter(Column::TreeId.eq(tree_id))
                .filter(Column::FamilyId.is_in(chunk))
                .all(db)
                .await
                .map_err(db_err)?;
            Ok(models.into_iter().map(into_domain).collect())
        })
        .await
    }

    /// List all family memberships where this person is a spouse.
    pub async fn list_by_person(
        db: &impl ConnectionTrait,
        person_id: Uuid,
    ) -> Result<Vec<FamilySpouse>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::PersonId.eq(person_id))
            .all(db)
            .await
            .map_err(db_err)?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// List all family memberships for multiple spouses.
    pub async fn list_by_persons(
        db: &impl ConnectionTrait,
        person_ids: &[Uuid],
    ) -> Result<Vec<FamilySpouse>, OxidGeneError> {
        in_chunks(person_ids, |chunk| async move {
            let models = Entity::find()
                .filter(Column::PersonId.is_in(chunk))
                .all(db)
                .await
                .map_err(db_err)?;
            Ok(models.into_iter().map(into_domain).collect())
        })
        .await
    }

    /// Create a family–spouse link. A person is a spouse of a family once,
    /// whatever the role: a second link is a conflict.
    pub async fn create(
        db: &impl ConnectionTrait,
        id: Uuid,
        family_id: Uuid,
        person_id: Uuid,
        role: SpouseRole,
        sort_order: i32,
    ) -> Result<FamilySpouse, OxidGeneError> {
        let already = Entity::find()
            .filter(Column::FamilyId.eq(family_id))
            .filter(Column::PersonId.eq(person_id))
            .one(db)
            .await
            .map_err(db_err)?;
        if already.is_some() {
            return Err(OxidGeneError::Conflict(
                "the person is already a spouse of the family".to_string(),
            ));
        }
        let tree_id =
            crate::repo::tree_of::<family::Entity>(db, family_id, family::Column::TreeId, "Family")
                .await?;
        let model = family_spouse::ActiveModel {
            id: Set(id),
            tree_id: Set(tree_id),
            family_id: Set(family_id),
            person_id: Set(person_id),
            role: Set(sea_enums::SpouseRole::from(role)),
            sort_order: Set(sort_order),
        };
        let result = model.insert(db).await.map_err(db_err)?;
        Ok(into_domain(result))
    }

    /// Hard-delete a family–spouse link.
    pub async fn delete(db: &impl ConnectionTrait, id: Uuid) -> Result<(), OxidGeneError> {
        let result = Entity::delete_by_id(id).exec(db).await.map_err(db_err)?;
        if result.rows_affected == 0 {
            return Err(OxidGeneError::NotFound {
                entity: "FamilySpouse",
                id,
            });
        }
        Ok(())
    }
}

pub(crate) fn into_domain(m: family_spouse::Model) -> FamilySpouse {
    FamilySpouse {
        id: m.id,
        family_id: m.family_id,
        person_id: m.person_id,
        role: m.role.into(),
        sort_order: m.sort_order,
    }
}
