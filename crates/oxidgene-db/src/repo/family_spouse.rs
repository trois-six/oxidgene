//! Repository for `FamilySpouse` junction table (create/delete only).

use oxidgene_core::enums::SpouseRole;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::FamilySpouse;
use sea_orm::entity::prelude::*;
use sea_orm::{ConnectionTrait, JoinType, QueryFilter, QuerySelect, Set};
use uuid::Uuid;

use crate::entities::family;
use crate::entities::family_spouse::{self, Column, Entity};
use crate::entities::sea_enums;

/// Repository for family–spouse membership.
pub struct FamilySpouseRepo;

impl FamilySpouseRepo {
    /// List spouses in a family.
    pub async fn list_by_family(
        db: &impl ConnectionTrait,
        family_id: Uuid,
    ) -> Result<Vec<FamilySpouse>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::FamilyId.eq(family_id))
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// List spouses for multiple families.
    pub async fn list_by_families(
        db: &impl ConnectionTrait,
        family_ids: &[Uuid],
    ) -> Result<Vec<FamilySpouse>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::FamilyId.is_in(family_ids.iter().copied()))
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// List spouse links for active families in one tree.
    pub async fn list_by_families_in_tree(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        family_ids: &[Uuid],
    ) -> Result<Vec<FamilySpouse>, OxidGeneError> {
        let models = Entity::find()
            .join(JoinType::InnerJoin, family_spouse::Relation::Family.def())
            .filter(family::Column::TreeId.eq(tree_id))
            .filter(family::Column::DeletedAt.is_null())
            .filter(Column::FamilyId.is_in(family_ids.iter().copied()))
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
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
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// List all family memberships for multiple spouses.
    pub async fn list_by_persons(
        db: &impl ConnectionTrait,
        person_ids: &[Uuid],
    ) -> Result<Vec<FamilySpouse>, OxidGeneError> {
        if person_ids.is_empty() {
            return Ok(Vec::new());
        }
        let models = Entity::find()
            .filter(Column::PersonId.is_in(person_ids.iter().copied()))
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// Create a family–spouse link.
    pub async fn create(
        db: &impl ConnectionTrait,
        id: Uuid,
        family_id: Uuid,
        person_id: Uuid,
        role: SpouseRole,
        sort_order: i32,
    ) -> Result<FamilySpouse, OxidGeneError> {
        let model = family_spouse::ActiveModel {
            id: Set(id),
            family_id: Set(family_id),
            person_id: Set(person_id),
            role: Set(sea_enums::SpouseRole::from(role)),
            sort_order: Set(sort_order),
        };
        let result = model
            .insert(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(into_domain(result))
    }

    /// Hard-delete a family–spouse link.
    pub async fn delete(db: &impl ConnectionTrait, id: Uuid) -> Result<(), OxidGeneError> {
        let result = Entity::delete_by_id(id)
            .exec(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        if result.rows_affected == 0 {
            return Err(OxidGeneError::NotFound {
                entity: "FamilySpouse",
                id,
            });
        }
        Ok(())
    }
}

fn into_domain(m: family_spouse::Model) -> FamilySpouse {
    FamilySpouse {
        id: m.id,
        family_id: m.family_id,
        person_id: m.person_id,
        role: m.role.into(),
        sort_order: m.sort_order,
    }
}
