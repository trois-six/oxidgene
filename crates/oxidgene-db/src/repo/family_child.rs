//! Repository for `FamilyChild` junction table (create/delete only).

use oxidgene_core::enums::ChildType;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::FamilyChild;
use sea_orm::entity::prelude::*;
use sea_orm::{QueryFilter, Set};
use uuid::Uuid;

use crate::entities::family_child::{self, Column, Entity};
use crate::entities::sea_enums;

/// Repository for family–child membership.
pub struct FamilyChildRepo;

impl FamilyChildRepo {
    /// List children in a family.
    pub async fn list_by_family(
        db: &DatabaseConnection,
        family_id: Uuid,
    ) -> Result<Vec<FamilyChild>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::FamilyId.eq(family_id))
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// List children for multiple families.
    pub async fn list_by_families(
        db: &DatabaseConnection,
        family_ids: &[Uuid],
    ) -> Result<Vec<FamilyChild>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::FamilyId.is_in(family_ids.iter().copied()))
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// Create a family–child link.
    pub async fn create(
        db: &DatabaseConnection,
        id: Uuid,
        family_id: Uuid,
        person_id: Uuid,
        child_type: ChildType,
        sort_order: i32,
    ) -> Result<FamilyChild, OxidGeneError> {
        let model = family_child::ActiveModel {
            id: Set(id),
            family_id: Set(family_id),
            person_id: Set(person_id),
            child_type: Set(sea_enums::ChildType::from(child_type)),
            sort_order: Set(sort_order),
        };
        let result = model
            .insert(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(into_domain(result))
    }

    /// Hard-delete a family–child link.
    pub async fn delete(db: &DatabaseConnection, id: Uuid) -> Result<(), OxidGeneError> {
        let result = Entity::delete_by_id(id)
            .exec(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        if result.rows_affected == 0 {
            return Err(OxidGeneError::NotFound {
                entity: "FamilyChild",
                id,
            });
        }
        Ok(())
    }
}

fn into_domain(m: family_child::Model) -> FamilyChild {
    FamilyChild {
        id: m.id,
        family_id: m.family_id,
        person_id: m.person_id,
        child_type: m.child_type.into(),
        sort_order: m.sort_order,
    }
}
