//! Repository for `Source` entities (CRUD with soft delete).

use chrono::Utc;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::{Connection, Source};
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveModelTrait, ConnectionTrait, IntoActiveModel, QueryFilter, Set};
use uuid::Uuid;

use crate::entities::source::{self, ActiveModel, Column, Entity};
use crate::entities::{citation, media_link, note, source_repository};
use crate::repo::batch::in_chunks;
use crate::repo::db_err;
use crate::repo::pagination::{PaginationParams, paginate};

/// Repository for source CRUD operations.
pub struct SourceRepo;

impl SourceRepo {
    /// List sources in a tree with pagination (excludes soft-deleted).
    pub async fn list(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        params: &PaginationParams,
    ) -> Result<Connection<Source>, OxidGeneError> {
        Self::list_titled(db, tree_id, None, params).await
    }

    /// [`Self::list`], narrowed to the sources titled `title` — trimmed,
    /// ignoring case — when one is given: how an editor finds the source a
    /// typed title names without reading every source of the tree.
    pub async fn list_titled(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        title: Option<&str>,
        params: &PaginationParams,
    ) -> Result<Connection<Source>, OxidGeneError> {
        let mut query = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::DeletedAt.is_null());
        if let Some(title) = title {
            query = query.filter(crate::repo::lower_trim_eq(Column::Title, title));
        }
        paginate(db, query, Column::Id, params, |m| (m.id, into_domain(m))).await
    }

    /// How many live sources a tree has.
    pub async fn count_in_tree(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<u64, OxidGeneError> {
        Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::DeletedAt.is_null())
            .count(db)
            .await
            .map_err(db_err)
    }

    /// List all sources in a tree without pagination (excludes soft-deleted).
    pub async fn list_all(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<Source>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::DeletedAt.is_null())
            .all(db)
            .await
            .map_err(db_err)?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// Get a single source by ID (excludes soft-deleted).
    pub async fn get(db: &impl ConnectionTrait, id: Uuid) -> Result<Source, OxidGeneError> {
        find_live(db, id).await.map(into_domain)
    }

    /// Get multiple sources from one tree (excludes soft-deleted).
    pub async fn get_many(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        ids: &[Uuid],
    ) -> Result<Vec<Source>, OxidGeneError> {
        in_chunks(ids, |chunk| async move {
            let models = Entity::find()
                .filter(Column::TreeId.eq(tree_id))
                .filter(Column::Id.is_in(chunk))
                .filter(Column::DeletedAt.is_null())
                .all(db)
                .await
                .map_err(db_err)?;
            Ok(models.into_iter().map(into_domain).collect())
        })
        .await
    }

    /// Get multiple sources by ID, whatever their tree (excludes
    /// soft-deleted): for ids read from records of one tree already, such as
    /// a repository's links.
    pub async fn get_many_by_id(
        db: &impl ConnectionTrait,
        ids: &[Uuid],
    ) -> Result<Vec<Source>, OxidGeneError> {
        in_chunks(ids, |chunk| async move {
            let models = Entity::find()
                .filter(Column::Id.is_in(chunk))
                .filter(Column::DeletedAt.is_null())
                .all(db)
                .await
                .map_err(db_err)?;
            Ok(models.into_iter().map(into_domain).collect())
        })
        .await
    }

    /// Create a new source.
    #[expect(
        clippy::too_many_arguments,
        reason = "one parameter per column the statement writes, already validated by the calling service"
    )]
    pub async fn create(
        db: &impl ConnectionTrait,
        id: Uuid,
        tree_id: Uuid,
        title: String,
        author: Option<String>,
        publisher: Option<String>,
        abbreviation: Option<String>,
        agency: Option<String>,
    ) -> Result<Source, OxidGeneError> {
        let now = Utc::now();
        let model = source::ActiveModel {
            id: Set(id),
            tree_id: Set(tree_id),
            title: Set(title),
            author: Set(author),
            publisher: Set(publisher),
            abbreviation: Set(abbreviation),
            agency: Set(agency),
            created_at: Set(now),
            updated_at: Set(now),
            deleted_at: Set(None),
        };
        let result = model.insert(db).await.map_err(db_err)?;
        Ok(into_domain(result))
    }

    /// Update an existing source.
    pub async fn update(
        db: &impl ConnectionTrait,
        id: Uuid,
        title: Option<String>,
        author: Option<Option<String>>,
        publisher: Option<Option<String>>,
        abbreviation: Option<Option<String>>,
        agency: Option<Option<String>>,
    ) -> Result<Source, OxidGeneError> {
        let existing = find_live(db, id).await?;

        let mut active: ActiveModel = existing.into_active_model();
        if let Some(title) = title {
            active.title = Set(title);
        }
        if let Some(author) = author {
            active.author = Set(author);
        }
        if let Some(publisher) = publisher {
            active.publisher = Set(publisher);
        }
        if let Some(abbreviation) = abbreviation {
            active.abbreviation = Set(abbreviation);
        }
        if let Some(agency) = agency {
            active.agency = Set(agency);
        }
        active.updated_at = Set(Utc::now());

        let result = active.update(db).await.map_err(db_err)?;
        Ok(into_domain(result))
    }

    /// Soft-delete a source.
    pub async fn delete(db: &impl ConnectionTrait, id: Uuid) -> Result<(), OxidGeneError> {
        let existing = find_live(db, id).await?;

        let mut active: ActiveModel = existing.into_active_model();
        active.deleted_at = Set(Some(Utc::now()));
        active.update(db).await.map_err(db_err)?;
        Ok(())
    }

    /// Soft-deletes a source only if nothing points at it any more, and
    /// reports whether it did.
    ///
    /// Free-text source entry mints a `Source` per distinct title, so a typo
    /// corrected on the next save would otherwise leave its row in the tree —
    /// and in the source dictionary — forever. A source is "unused" only when
    /// no citation, note, media link *and* repository link reference it; a
    /// source that is still cited, or was catalogued at a repository, is left
    /// alone, so this can never take out a source the user is relying on.
    ///
    /// Returns `Ok(false)` for a source that is still referenced, and for one
    /// that is already gone — the caller asked for it to be absent, and it is.
    pub async fn delete_if_unused(
        db: &impl ConnectionTrait,
        id: Uuid,
    ) -> Result<bool, OxidGeneError> {
        let referenced = |count: u64| count > 0;

        let cited = citation::Entity::find()
            .filter(citation::Column::SourceId.eq(id))
            .count(db)
            .await
            .map_err(db_err)?;
        if referenced(cited) {
            return Ok(false);
        }

        // Notes and media links carry the source optionally, so only rows
        // that actually name it count.
        let noted = note::Entity::find()
            .filter(note::Column::SourceId.eq(id))
            .filter(note::Column::DeletedAt.is_null())
            .count(db)
            .await
            .map_err(db_err)?;
        if referenced(noted) {
            return Ok(false);
        }

        let linked = media_link::Entity::find()
            .filter(media_link::Column::SourceId.eq(id))
            .count(db)
            .await
            .map_err(db_err)?;
        if referenced(linked) {
            return Ok(false);
        }

        let held = source_repository::Entity::find()
            .filter(source_repository::Column::SourceId.eq(id))
            .count(db)
            .await
            .map_err(db_err)?;
        if referenced(held) {
            return Ok(false);
        }

        match Self::delete(db, id).await {
            Ok(()) => Ok(true),
            Err(OxidGeneError::NotFound { .. }) => Ok(false),
            Err(e) => Err(e),
        }
    }
}

fn into_domain(m: source::Model) -> Source {
    Source {
        id: m.id,
        tree_id: m.tree_id,
        title: m.title,
        author: m.author,
        publisher: m.publisher,
        abbreviation: m.abbreviation,
        agency: m.agency,
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
    crate::repo::find_live::<Entity>(db, id, Column::DeletedAt, "Source").await
}
