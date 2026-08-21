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
use oxidgene_core::projection::{PROJECTION_SCHEMA_VERSION, PersonProfile};
use sea_orm::sea_query::OnConflict;
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait, QueryFilter, Set};
use uuid::Uuid;

use crate::entities::person_denorm::{ActiveModel, Column, Entity, Model};

/// Maximum rows per INSERT batch (5 bind values per row, well under the
/// SQLite / PostgreSQL parameter limits).
const INSERT_CHUNK: usize = 500;

/// Matches only rows this build wrote, so one written by an older one reads as
/// *absent* rather than as data.
///
/// Every field added to `PersonProfile` carries `#[serde(default)]`, so an old
/// payload deserializes cleanly and comes back looking complete — nothing can
/// tell it apart from a person who genuinely has nothing recorded. Checking the
/// version in SQL instead makes a stale row simply not match: `get` returns
/// `None`, `get_many` omits it, `count_current` does not count it, and the
/// callers that already rebuild a projection they could not find rebuild these
/// too. No second code path, and no way to forget one.
fn is_current() -> sea_orm::sea_query::SimpleExpr {
    Column::SchemaVersion.eq(PROJECTION_SCHEMA_VERSION)
}

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
            .filter(is_current())
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
                .filter(is_current())
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
    ///
    /// Deliberately *not* filtered by version, unlike [`Self::get`] and
    /// [`Self::get_many`]: this returns a whole tree's worth of people, and
    /// silently dropping the stale ones would answer "who is in this tree" with
    /// a short list. Its one caller checks [`Self::count_current`] first and
    /// rebuilds the tree when it comes back zero, so by the time this runs
    /// there is nothing stale left to filter.
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
                        // `SchemaVersion` belongs in this list: overwriting a
                        // stale row's payload while leaving its old version
                        // behind would rebuild it forever, once per read.
                        .update_columns([
                            Column::TreeId,
                            Column::Payload,
                            Column::SchemaVersion,
                            Column::UpdatedAt,
                        ])
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

    /// Count the *usable* projections of a tree — rows this build can read.
    ///
    /// This is what "has the tree been materialized" means, and it answers no
    /// both for a tree nobody has built and for one whose rows an older build
    /// wrote. Callers rebuild on zero, so a schema bump heals a tree on its
    /// first read rather than serving defaults until somebody re-imports.
    pub async fn count_current(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<u64, OxidGeneError> {
        Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .filter(is_current())
            .count(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))
    }

    /// Count every projection row of a tree, current or stale.
    ///
    /// Only for reporting on what is physically stored; use
    /// [`Self::count_current`] to decide whether a tree needs building.
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
        // Stamped from the constant, never from the row being replaced: this
        // payload was just built by *this* binary, whatever wrote the last one.
        schema_version: Set(PROJECTION_SCHEMA_VERSION),
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
