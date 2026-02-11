//! Repository for `PersonAncestry` closure table (read queries).

use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::PersonAncestry;
use sea_orm::entity::prelude::*;
use sea_orm::{Order, QueryFilter, QueryOrder, Set};
use uuid::Uuid;

use crate::entities::person_ancestry::{self, Column, Entity};

/// Repository for person ancestry closure table operations.
pub struct PersonAncestryRepo;

impl PersonAncestryRepo {
    /// Get all ancestors of a person (optionally limited by max depth).
    pub async fn ancestors(
        db: &DatabaseConnection,
        descendant_id: Uuid,
        max_depth: Option<i32>,
    ) -> Result<Vec<PersonAncestry>, OxidGeneError> {
        let mut query = Entity::find()
            .filter(Column::DescendantId.eq(descendant_id))
            .filter(Column::Depth.gt(0)); // Exclude self-reference

        if let Some(max) = max_depth {
            query = query.filter(Column::Depth.lte(max));
        }

        let models = query
            .order_by(Column::Depth, Order::Asc)
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// Get all descendants of a person (optionally limited by max depth).
    pub async fn descendants(
        db: &DatabaseConnection,
        ancestor_id: Uuid,
        max_depth: Option<i32>,
    ) -> Result<Vec<PersonAncestry>, OxidGeneError> {
        let mut query = Entity::find()
            .filter(Column::AncestorId.eq(ancestor_id))
            .filter(Column::Depth.gt(0)); // Exclude self-reference

        if let Some(max) = max_depth {
            query = query.filter(Column::Depth.lte(max));
        }

        let models = query
            .order_by(Column::Depth, Order::Asc)
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// Insert a closure table entry (used internally when family relationships change).
    pub async fn create(
        db: &DatabaseConnection,
        id: Uuid,
        tree_id: Uuid,
        ancestor_id: Uuid,
        descendant_id: Uuid,
        depth: i32,
    ) -> Result<PersonAncestry, OxidGeneError> {
        let model = person_ancestry::ActiveModel {
            id: Set(id),
            tree_id: Set(tree_id),
            ancestor_id: Set(ancestor_id),
            descendant_id: Set(descendant_id),
            depth: Set(depth),
        };
        let result = model
            .insert(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(into_domain(result))
    }

    /// Delete all ancestry entries for a given descendant (used when re-parenting).
    pub async fn delete_by_descendant(
        db: &DatabaseConnection,
        descendant_id: Uuid,
    ) -> Result<u64, OxidGeneError> {
        let result = Entity::delete_many()
            .filter(Column::DescendantId.eq(descendant_id))
            .exec(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(result.rows_affected)
    }
}

fn into_domain(m: person_ancestry::Model) -> PersonAncestry {
    PersonAncestry {
        id: m.id,
        tree_id: m.tree_id,
        ancestor_id: m.ancestor_id,
        descendant_id: m.descendant_id,
        depth: m.depth,
    }
}
