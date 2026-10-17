//! Repository for `gedcom_kept`: the GEDCOM structures OxidGene's model has
//! no place for, kept per entity (docs/api.md, "Kept GEDCOM").

use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::{GedcomKept, KeptEntityKind};
use sea_orm::sea_query::Expr;
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder};
use uuid::Uuid;

use crate::entities::gedcom_kept::{self, Column, Entity};
use crate::repo::db_err;

/// Rows inserted per statement. Each row binds six values, so a chunk stays
/// far inside SQLite's variable limit.
const INSERT_CHUNK: usize = 500;

/// Repository for kept GEDCOM rows.
pub struct GedcomKeptRepo;

impl GedcomKeptRepo {
    /// Insert `rows`, chunked to stay within the database's parameter limit.
    pub async fn insert_many(
        db: &impl ConnectionTrait,
        rows: &[GedcomKept],
    ) -> Result<(), OxidGeneError> {
        for chunk in rows.chunks(INSERT_CHUNK) {
            let models = chunk.iter().map(gedcom_kept::ActiveModel::from_domain);
            Entity::insert_many(models).exec(db).await.map_err(db_err)?;
        }
        Ok(())
    }

    /// Every kept row of a tree, in creation order.
    pub async fn list_by_tree(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<GedcomKept>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .order_by_asc(Column::CreatedAt)
            .order_by_asc(Column::Id)
            .all(db)
            .await
            .map_err(db_err)?;
        models
            .into_iter()
            .map(gedcom_kept::Model::into_domain)
            .collect()
    }

    /// Move every row of `(kind, from)` onto `(kind, to)`: the rows of a
    /// merged person follow the person kept in its place.
    pub async fn repoint(
        db: &impl ConnectionTrait,
        kind: KeptEntityKind,
        from: Uuid,
        to: Uuid,
    ) -> Result<(), OxidGeneError> {
        Entity::update_many()
            .col_expr(Column::EntityId, Expr::value(to))
            .filter(Column::EntityKind.eq(kind.as_str()))
            .filter(Column::EntityId.eq(from))
            .exec(db)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}
