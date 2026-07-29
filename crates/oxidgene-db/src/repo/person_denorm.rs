//! Repository for the `person_denorm` materialized person projection.
//!
//! Each row stores one [`PersonProfile`] as JSON. Writes happen right after a
//! mutation, for the bounded affected set computed by the API's invalidation
//! module; reads serve the person detail page, the person list and every
//! pedigree node.
//!
//! This table (together with `person_search_fts`) is what replaced the
//! `oxidgene-cache` crate: the projection is durable, survives restarts, and
//! is identical on desktop (SQLite) and web (PostgreSQL).

use oxidgene_core::error::OxidGeneError;
use oxidgene_core::projection::PersonProfile;
use sea_orm::sea_query::OnConflict;
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait, QueryFilter, Set};
use uuid::Uuid;

use crate::entities::person_denorm::{ActiveModel, Column, Entity, Model};

/// Maximum rows per INSERT batch (4 bind values per row, well under the
/// SQLite / PostgreSQL parameter limits).
const INSERT_CHUNK: usize = 500;

/// Repository for the denormalized person projection table.
pub struct PersonDenormRepo;

impl PersonDenormRepo {
    /// Read one person's projection, if it has been materialized.
    pub async fn get(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        person_id: Uuid,
    ) -> Result<Option<PersonProfile>, OxidGeneError> {
        let model = Entity::find_by_id(person_id)
            .filter(Column::TreeId.eq(tree_id))
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        model.map(decode).transpose()
    }

    /// Read the projections for a bounded set of persons.
    ///
    /// Missing rows are simply absent from the result — callers rebuild them.
    pub async fn get_many(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        person_ids: &[Uuid],
    ) -> Result<Vec<PersonProfile>, OxidGeneError> {
        if person_ids.is_empty() {
            return Ok(vec![]);
        }
        let mut out = Vec::with_capacity(person_ids.len());
        for chunk in person_ids.chunks(INSERT_CHUNK) {
            let models = Entity::find()
                .filter(Column::TreeId.eq(tree_id))
                .filter(Column::PersonId.is_in(chunk.iter().copied()))
                .all(db)
                .await
                .map_err(|e| OxidGeneError::Database(e.to_string()))?;
            for model in models {
                out.push(decode(model)?);
            }
        }
        Ok(out)
    }

    /// Read every projection of a tree.
    pub async fn list_tree(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<PersonProfile>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        models.into_iter().map(decode).collect()
    }

    /// Insert or replace the projections for a bounded set of persons.
    pub async fn upsert(
        db: &impl ConnectionTrait,
        entries: &[PersonProfile],
    ) -> Result<(), OxidGeneError> {
        if entries.is_empty() {
            return Ok(());
        }
        for chunk in entries.chunks(INSERT_CHUNK) {
            let models: Vec<ActiveModel> = chunk
                .iter()
                .map(encode)
                .collect::<Result<Vec<_>, OxidGeneError>>()?;
            Entity::insert_many(models)
                .on_conflict(
                    OnConflict::column(Column::PersonId)
                        .update_columns([Column::TreeId, Column::Payload, Column::UpdatedAt])
                        .to_owned(),
                )
                .exec(db)
                .await
                .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        }
        Ok(())
    }

    /// Replace every projection of a tree (full rebuild / GEDCOM import).
    pub async fn replace_tree(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        entries: &[PersonProfile],
    ) -> Result<(), OxidGeneError> {
        Self::delete_tree(db, tree_id).await?;
        Self::upsert(db, entries).await
    }

    /// Drop one person's projection.
    ///
    /// The `ON DELETE CASCADE` on `person_id` already covers a hard person
    /// delete; this is for soft deletes, where the `person` row survives.
    pub async fn delete_person(
        db: &impl ConnectionTrait,
        person_id: Uuid,
    ) -> Result<(), OxidGeneError> {
        Entity::delete_by_id(person_id)
            .exec(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(())
    }

    /// Drop every projection of a tree.
    pub async fn delete_tree(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<(), OxidGeneError> {
        Entity::delete_many()
            .filter(Column::TreeId.eq(tree_id))
            .exec(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(())
    }

    /// Count the materialized projections of a tree (used to detect a tree
    /// that has never been built, e.g. right after the migration).
    pub async fn count_tree(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<u64, OxidGeneError> {
        Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .count(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))
    }
}

/// Serialize a projection into its storage row.
fn encode(person: &PersonProfile) -> Result<ActiveModel, OxidGeneError> {
    let payload = serde_json::to_string(person)
        .map_err(|e| OxidGeneError::Database(format!("person_denorm encode: {e}")))?;
    Ok(ActiveModel {
        person_id: Set(person.person_id),
        tree_id: Set(person.tree_id),
        payload: Set(payload),
        updated_at: Set(person.built_at.into()),
    })
}

/// Deserialize a storage row back into a projection.
fn decode(model: Model) -> Result<PersonProfile, OxidGeneError> {
    serde_json::from_str(&model.payload).map_err(|e| {
        OxidGeneError::Database(format!(
            "person_denorm decode for person {}: {e}",
            model.person_id
        ))
    })
}
