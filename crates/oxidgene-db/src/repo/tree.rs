//! Repository for `Tree` entities.
//!
//! Deleting a tree happens in two stages. [`TreeRepo::soft_delete`] flips
//! `deleted_at` — one row, instant, and the tree disappears from [`TreeRepo::list`]
//! straight away. [`TreeRepo::purge`] then does the real cascade in the
//! background, because SQLite resolves `ON DELETE CASCADE` one row at a time
//! and a large cascade must not block the request.

use chrono::Utc;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::{Connection, Tree};
use sea_orm::entity::prelude::*;
use sea_orm::sea_query::Expr;
use sea_orm::{ActiveModelTrait, ConnectionTrait, IntoActiveModel, QueryFilter, Set};
use uuid::Uuid;

use crate::entities::tree::{self, ActiveModel, Column, Entity};
use crate::repo::db_err;
use crate::repo::pagination::{PaginationParams, paginate};

/// Repository for tree CRUD operations.
pub struct TreeRepo;

/// What [`TreeRepo::update`] changes; `None` keeps a field. A doubled
/// `Option` clears the field with `Some(None)`.
#[derive(Debug, Default)]
pub struct TreeChanges {
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    pub sosa_root_person_id: Option<Option<Uuid>>,
    pub self_person_id: Option<Option<Uuid>>,
    pub default_privacy: Option<oxidgene_core::enums::TreeDefaultPrivacy>,
    pub entry_suggestions: Option<bool>,
    pub date_format: Option<oxidgene_core::enums::DateDisplayFormat>,
    pub date_symbols: Option<bool>,
    pub date_circa: Option<bool>,
    pub date_calendar: Option<oxidgene_core::enums::Calendar>,
    pub submitter_name: Option<Option<String>>,
    pub submitter_email: Option<Option<String>>,
    pub submitter_address: Option<Option<String>>,
}

impl TreeChanges {
    /// Set every field these changes name on `active`.
    fn apply(self, active: &mut ActiveModel) {
        set(&mut active.name, self.name);
        set(&mut active.description, self.description);
        set(&mut active.sosa_root_person_id, self.sosa_root_person_id);
        set(&mut active.self_person_id, self.self_person_id);
        set(
            &mut active.default_privacy,
            self.default_privacy.map(Into::into),
        );
        set(&mut active.entry_suggestions, self.entry_suggestions);
        set(&mut active.date_format, self.date_format.map(Into::into));
        set(&mut active.date_symbols, self.date_symbols);
        set(&mut active.date_circa, self.date_circa);
        set(
            &mut active.date_calendar,
            self.date_calendar.map(Into::into),
        );
        set(&mut active.submitter_name, self.submitter_name);
        set(&mut active.submitter_email, self.submitter_email);
        set(&mut active.submitter_address, self.submitter_address);
    }
}

/// Set `field` to `value`, unless there is none.
fn set<T: Into<sea_orm::Value>>(field: &mut sea_orm::ActiveValue<T>, value: Option<T>) {
    if let Some(value) = value {
        *field = Set(value);
    }
}

impl TreeRepo {
    /// List trees with cursor-based pagination (excludes soft-deleted).
    pub async fn list(
        db: &impl ConnectionTrait,
        params: &PaginationParams,
    ) -> Result<Connection<Tree>, OxidGeneError> {
        let query = Entity::find().filter(Column::DeletedAt.is_null());
        paginate(db, query, Column::Id, params, |m| (m.id, into_domain(m))).await
    }

    /// Get a single tree by ID (excludes soft-deleted).
    pub async fn get(db: &impl ConnectionTrait, id: Uuid) -> Result<Tree, OxidGeneError> {
        find_live(db, id).await.map(into_domain)
    }

    /// Create a new tree.
    pub async fn create(
        db: &impl ConnectionTrait,
        id: Uuid,
        name: String,
        description: Option<String>,
    ) -> Result<Tree, OxidGeneError> {
        let now = Utc::now();
        let model = tree::ActiveModel {
            id: Set(id),
            name: Set(name),
            description: Set(description),
            sosa_root_person_id: Set(None),
            self_person_id: Set(None),
            default_privacy: Set(oxidgene_core::enums::TreeDefaultPrivacy::default().into()),
            entry_suggestions: Set(true),
            date_format: Set(oxidgene_core::enums::DateDisplayFormat::default().into()),
            date_symbols: Set(false),
            date_circa: Set(false),
            date_calendar: Set(oxidgene_core::enums::Calendar::default().into()),
            submitter_name: Set(None),
            submitter_email: Set(None),
            submitter_address: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
            deleted_at: Set(None),
        };
        let result = model.insert(db).await.map_err(db_err)?;
        Ok(into_domain(result))
    }

    /// Update an existing tree: every field of `changes` left `None` is kept.
    pub async fn update(
        db: &impl ConnectionTrait,
        id: Uuid,
        changes: TreeChanges,
    ) -> Result<Tree, OxidGeneError> {
        let existing = find_live(db, id).await?;

        let mut active: ActiveModel = existing.into_active_model();
        changes.apply(&mut active);
        active.updated_at = Set(Utc::now());

        let result = active.update(db).await.map_err(db_err)?;
        Ok(into_domain(result))
    }

    /// Give tree `id` the submitter an import read, each field only where the
    /// tree's own is empty: an import never overwrites a setting.
    pub async fn fill_submitter(
        db: &impl ConnectionTrait,
        id: Uuid,
        name: Option<String>,
        email: Option<String>,
        address: Option<String>,
    ) -> Result<(), OxidGeneError> {
        let existing = find_live(db, id).await?;
        let mut active: ActiveModel = existing.clone().into_active_model();
        for (stored, imported, column) in [
            (existing.submitter_name, name, Column::SubmitterName),
            (existing.submitter_email, email, Column::SubmitterEmail),
            (
                existing.submitter_address,
                address,
                Column::SubmitterAddress,
            ),
        ] {
            if stored.is_none()
                && let Some(value) = imported
            {
                active.set(column, Some(value).into());
            }
        }
        if active.is_changed() {
            active.update(db).await.map_err(db_err)?;
        }
        Ok(())
    }

    /// Mark a tree as deleted, without touching the rows it owns.
    ///
    /// This is what a delete request does: it is a single-row UPDATE, so it
    /// returns in about a millisecond however large the tree is. [`list_purgeable`]
    /// then finds the tree again and [`purge`] does the expensive part in the
    /// background. The tree is already invisible to [`list`] and [`get`].
    pub async fn soft_delete(db: &impl ConnectionTrait, id: Uuid) -> Result<(), OxidGeneError> {
        let result = Entity::update_many()
            .col_expr(Column::DeletedAt, Expr::value(Utc::now()))
            .filter(Column::Id.eq(id))
            .filter(Column::DeletedAt.is_null())
            .exec(db)
            .await
            .map_err(db_err)?;
        if result.rows_affected == 0 {
            return Err(OxidGeneError::NotFound { entity: "Tree", id });
        }
        Ok(())
    }

    /// IDs of every soft-deleted tree still holding data.
    ///
    /// This *is* the purge queue: because the flag lives in the database, a
    /// purge interrupted by a crash or a quit is simply picked up again at the
    /// next start. No separate job table is needed.
    pub async fn list_purgeable(db: &impl ConnectionTrait) -> Result<Vec<Uuid>, OxidGeneError> {
        Entity::find()
            .filter(Column::DeletedAt.is_not_null())
            .all(db)
            .await
            .map(|models| models.into_iter().map(|m| m.id).collect())
            .map_err(db_err)
    }

    /// Hard-delete a tree. Cascades via `ON DELETE CASCADE` foreign keys to
    /// every entity scoped to this tree (person, event, family, place,
    /// source, media, note, ...) — a tree's data is never shared with
    /// another tree, so nothing outside it is affected.
    ///
    /// Unlike the other methods this deliberately does *not* filter on
    /// `deleted_at`: it is called precisely on trees that were soft-deleted.
    ///
    /// Expensive — SQLite resolves the cascade one row at a time, which on a
    /// 10k-person tree costs seconds. Call it from the purge worker, never
    /// from a request handler. Deleting an already-purged tree is not an
    /// error, so a re-run after a crash is harmless.
    pub async fn purge(db: &impl ConnectionTrait, id: Uuid) -> Result<(), OxidGeneError> {
        Entity::delete_by_id(id).exec(db).await.map_err(db_err)?;
        Ok(())
    }
}

fn into_domain(m: tree::Model) -> Tree {
    Tree {
        id: m.id,
        name: m.name,
        description: m.description,
        sosa_root_person_id: m.sosa_root_person_id,
        self_person_id: m.self_person_id,
        default_privacy: m.default_privacy.into(),
        entry_suggestions: m.entry_suggestions,
        date_format: m.date_format.into(),
        date_symbols: m.date_symbols,
        date_circa: m.date_circa,
        date_calendar: m.date_calendar.into(),
        submitter_name: m.submitter_name,
        submitter_email: m.submitter_email,
        submitter_address: m.submitter_address,
        created_at: m.created_at,
        updated_at: m.updated_at,
        deleted_at: m.deleted_at,
    }
}

/// Row `id`, unless it is missing or soft-deleted.
async fn find_live(
    db: &impl ConnectionTrait,
    id: Uuid,
) -> Result<<Entity as EntityTrait>::Model, OxidGeneError> {
    crate::repo::find_live::<Entity>(db, id, Column::DeletedAt, "Tree").await
}
