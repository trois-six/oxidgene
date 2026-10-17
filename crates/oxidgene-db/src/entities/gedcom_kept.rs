//! `gedcom_kept` table entity: GEDCOM structures the model has no place for.

use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::{GedcomKept, KeptEntityKind};
use sea_orm::Set;
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "gedcom_kept")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub tree_id: Uuid,
    pub entity_kind: String,
    pub entity_id: Uuid,
    pub nodes: String,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

impl Model {
    /// The domain row. A kind the code does not know is a corrupt row.
    pub fn into_domain(self) -> Result<GedcomKept, OxidGeneError> {
        let entity_kind = KeptEntityKind::parse(&self.entity_kind).ok_or_else(|| {
            OxidGeneError::Internal(format!(
                "gedcom_kept row {} has an unknown entity kind `{}`",
                self.id, self.entity_kind
            ))
        })?;
        Ok(GedcomKept {
            id: self.id,
            tree_id: self.tree_id,
            entity_kind,
            entity_id: self.entity_id,
            nodes: self.nodes,
            created_at: self.created_at,
        })
    }
}

impl ActiveModel {
    /// The active model that inserts `row`.
    pub fn from_domain(row: &GedcomKept) -> Self {
        Self {
            id: Set(row.id),
            tree_id: Set(row.tree_id),
            entity_kind: Set(row.entity_kind.as_str().to_string()),
            entity_id: Set(row.entity_id),
            nodes: Set(row.nodes.clone()),
            created_at: Set(row.created_at),
        }
    }
}
