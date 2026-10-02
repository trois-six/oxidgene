//! Repository for `Repository` entities — the archives and libraries holding
//! sources (CRUD with soft delete) — and for the links saying which sources
//! each holds.

use std::collections::HashMap;

use chrono::Utc;
use oxidgene_core::enums::SourceMediaType;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::{Connection, Repository, SourceRepository};
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveModelTrait, ConnectionTrait, IntoActiveModel, QueryFilter, QueryOrder, Set};
use uuid::Uuid;

use crate::entities::repository::{self, ActiveModel, Column, Entity};
use crate::entities::{note, source, source_repository};
use crate::repo::batch::in_chunks;
use crate::repo::db_err;
use crate::repo::pagination::{PaginationParams, paginate};

/// A repository's own fields, as a create writes them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepositoryFields {
    pub name: String,
    pub address: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub website: Option<String>,
}

/// The fields a repository update changes: `None` keeps a field and
/// `Some(None)` clears an optional one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepositoryPatch {
    pub name: Option<String>,
    pub address: Option<Option<String>>,
    pub phone: Option<Option<String>>,
    pub email: Option<Option<String>>,
    pub website: Option<Option<String>>,
}

/// What a source's link to a repository says, as a create writes it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceRepositoryFields {
    pub call_number: Option<String>,
    pub media_type: Option<SourceMediaType>,
    pub sort_order: i32,
}

/// The fields a link update changes: `None` keeps a field and `Some(None)`
/// clears an optional one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceRepositoryPatch {
    pub repository_id: Option<Uuid>,
    pub call_number: Option<Option<String>>,
    pub media_type: Option<Option<SourceMediaType>>,
    pub sort_order: Option<i32>,
}

/// Repository for repository CRUD operations.
pub struct RepositoryRepo;

impl RepositoryRepo {
    /// The live repositories of a tree, a page at a time.
    pub async fn list(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        params: &PaginationParams,
    ) -> Result<Connection<Repository>, OxidGeneError> {
        let query = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::DeletedAt.is_null());
        paginate(db, query, Column::Id, params, |m| (m.id, into_domain(m))).await
    }

    /// Every live repository of a tree.
    pub async fn list_all(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<Repository>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::DeletedAt.is_null())
            .all(db)
            .await
            .map_err(db_err)?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// A live repository.
    pub async fn get(db: &impl ConnectionTrait, id: Uuid) -> Result<Repository, OxidGeneError> {
        find_live(db, id).await.map(into_domain)
    }

    /// Live repositories among `ids`.
    pub async fn get_many(
        db: &impl ConnectionTrait,
        ids: &[Uuid],
    ) -> Result<Vec<Repository>, OxidGeneError> {
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

    pub async fn create(
        db: &impl ConnectionTrait,
        id: Uuid,
        tree_id: Uuid,
        fields: RepositoryFields,
    ) -> Result<Repository, OxidGeneError> {
        let now = Utc::now();
        let model = repository::ActiveModel {
            id: Set(id),
            tree_id: Set(tree_id),
            name: Set(fields.name),
            address: Set(fields.address),
            phone: Set(fields.phone),
            email: Set(fields.email),
            website: Set(fields.website),
            created_at: Set(now),
            updated_at: Set(now),
            deleted_at: Set(None),
        };
        model.insert(db).await.map_err(db_err).map(into_domain)
    }

    pub async fn update(
        db: &impl ConnectionTrait,
        id: Uuid,
        patch: RepositoryPatch,
    ) -> Result<Repository, OxidGeneError> {
        let mut active: ActiveModel = find_live(db, id).await?.into_active_model();
        if let Some(name) = patch.name {
            active.name = Set(name);
        }
        if let Some(address) = patch.address {
            active.address = Set(address);
        }
        if let Some(phone) = patch.phone {
            active.phone = Set(phone);
        }
        if let Some(email) = patch.email {
            active.email = Set(email);
        }
        if let Some(website) = patch.website {
            active.website = Set(website);
        }
        active.updated_at = Set(Utc::now());
        active.update(db).await.map_err(db_err).map(into_domain)
    }

    /// Soft-delete a repository. Its links stay, unread while it is deleted:
    /// undeleting it brings them back.
    pub async fn delete(db: &impl ConnectionTrait, id: Uuid) -> Result<(), OxidGeneError> {
        let mut active: ActiveModel = find_live(db, id).await?.into_active_model();
        active.deleted_at = Set(Some(Utc::now()));
        active.update(db).await.map_err(db_err)?;
        Ok(())
    }

    /// Whether a live source holds `id` or a live note is about it.
    pub async fn is_used(db: &impl ConnectionTrait, id: Uuid) -> Result<bool, OxidGeneError> {
        let held = source_repository::Entity::find()
            .inner_join(source::Entity)
            .filter(source_repository::Column::RepositoryId.eq(id))
            .filter(source::Column::DeletedAt.is_null())
            .count(db)
            .await
            .map_err(db_err)?;
        if held > 0 {
            return Ok(true);
        }
        let noted = note::Entity::find()
            .filter(note::Column::RepositoryId.eq(id))
            .filter(note::Column::DeletedAt.is_null())
            .count(db)
            .await
            .map_err(db_err)?;
        Ok(noted > 0)
    }
}

/// Repository for the links between sources and repositories.
pub struct SourceRepositoryRepo;

impl SourceRepositoryRepo {
    /// The links of a source, in order.
    pub async fn list_by_source(
        db: &impl ConnectionTrait,
        source_id: Uuid,
    ) -> Result<Vec<SourceRepository>, OxidGeneError> {
        Self::list_by_sources(db, &[source_id]).await
    }

    /// The links of these sources to live repositories, each source's in
    /// order.
    pub async fn list_by_sources(
        db: &impl ConnectionTrait,
        source_ids: &[Uuid],
    ) -> Result<Vec<SourceRepository>, OxidGeneError> {
        in_chunks(source_ids, |chunk| async move {
            let models = source_repository::Entity::find()
                .inner_join(repository::Entity)
                .filter(source_repository::Column::SourceId.is_in(chunk))
                .filter(repository::Column::DeletedAt.is_null())
                .order_by_asc(source_repository::Column::SortOrder)
                .order_by_asc(source_repository::Column::Id)
                .all(db)
                .await
                .map_err(db_err)?;
            Ok(models.into_iter().map(link_into_domain).collect())
        })
        .await
    }

    /// The links to a repository, from live sources.
    pub async fn list_by_repository(
        db: &impl ConnectionTrait,
        repository_id: Uuid,
    ) -> Result<Vec<SourceRepository>, OxidGeneError> {
        Self::list_by_repositories(db, &[repository_id]).await
    }

    /// The links to these repositories from live sources, each repository's
    /// in order.
    pub async fn list_by_repositories(
        db: &impl ConnectionTrait,
        repository_ids: &[Uuid],
    ) -> Result<Vec<SourceRepository>, OxidGeneError> {
        in_chunks(repository_ids, |chunk| async move {
            let models = source_repository::Entity::find()
                .inner_join(source::Entity)
                .filter(source_repository::Column::RepositoryId.is_in(chunk))
                .filter(source::Column::DeletedAt.is_null())
                .order_by_asc(source_repository::Column::Id)
                .all(db)
                .await
                .map_err(db_err)?;
            Ok(models.into_iter().map(link_into_domain).collect())
        })
        .await
    }

    /// The names of the live repositories holding each of these sources, in
    /// the sources' order, each name once.
    pub async fn repository_names(
        db: &impl ConnectionTrait,
        source_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, Vec<String>>, OxidGeneError> {
        let links = Self::list_by_sources(db, source_ids).await?;
        let repository_ids: Vec<Uuid> = links.iter().map(|l| l.repository_id).collect();
        let names: HashMap<Uuid, String> = RepositoryRepo::get_many(db, &repository_ids)
            .await?
            .into_iter()
            .map(|r| (r.id, r.name))
            .collect();
        let mut by_source: HashMap<Uuid, Vec<String>> = HashMap::new();
        for link in links {
            let Some(name) = names.get(&link.repository_id) else {
                continue;
            };
            let held = by_source.entry(link.source_id).or_default();
            if !held.contains(name) {
                held.push(name.clone());
            }
        }
        Ok(by_source)
    }

    pub async fn get(
        db: &impl ConnectionTrait,
        id: Uuid,
    ) -> Result<SourceRepository, OxidGeneError> {
        source_repository::Entity::find_by_id(id)
            .one(db)
            .await
            .map_err(db_err)?
            .map(link_into_domain)
            .ok_or(OxidGeneError::NotFound {
                entity: "SourceRepository",
                id,
            })
    }

    pub async fn create(
        db: &impl ConnectionTrait,
        id: Uuid,
        source_id: Uuid,
        repository_id: Uuid,
        fields: SourceRepositoryFields,
    ) -> Result<SourceRepository, OxidGeneError> {
        source_repository::ActiveModel {
            id: Set(id),
            source_id: Set(source_id),
            repository_id: Set(repository_id),
            call_number: Set(fields.call_number),
            media_type: Set(fields.media_type.map(Into::into)),
            sort_order: Set(fields.sort_order),
        }
        .insert(db)
        .await
        .map_err(db_err)
        .map(link_into_domain)
    }

    pub async fn update(
        db: &impl ConnectionTrait,
        id: Uuid,
        patch: SourceRepositoryPatch,
    ) -> Result<SourceRepository, OxidGeneError> {
        let existing = source_repository::Entity::find_by_id(id)
            .one(db)
            .await
            .map_err(db_err)?
            .ok_or(OxidGeneError::NotFound {
                entity: "SourceRepository",
                id,
            })?;
        let mut active = existing.into_active_model();
        if let Some(repository_id) = patch.repository_id {
            active.repository_id = Set(repository_id);
        }
        if let Some(call_number) = patch.call_number {
            active.call_number = Set(call_number);
        }
        if let Some(media_type) = patch.media_type {
            active.media_type = Set(media_type.map(Into::into));
        }
        if let Some(sort_order) = patch.sort_order {
            active.sort_order = Set(sort_order);
        }
        active
            .update(db)
            .await
            .map_err(db_err)
            .map(link_into_domain)
    }

    pub async fn delete(db: &impl ConnectionTrait, id: Uuid) -> Result<(), OxidGeneError> {
        let result = source_repository::Entity::delete_by_id(id)
            .exec(db)
            .await
            .map_err(db_err)?;
        if result.rows_affected == 0 {
            return Err(OxidGeneError::NotFound {
                entity: "SourceRepository",
                id,
            });
        }
        Ok(())
    }
}

fn into_domain(m: repository::Model) -> Repository {
    Repository {
        id: m.id,
        tree_id: m.tree_id,
        name: m.name,
        address: m.address,
        phone: m.phone,
        email: m.email,
        website: m.website,
        created_at: m.created_at,
        updated_at: m.updated_at,
        deleted_at: m.deleted_at,
    }
}

fn link_into_domain(m: source_repository::Model) -> SourceRepository {
    SourceRepository {
        id: m.id,
        source_id: m.source_id,
        repository_id: m.repository_id,
        call_number: m.call_number,
        media_type: m.media_type.map(Into::into),
        sort_order: m.sort_order,
    }
}

/// Row `id`, unless it is missing or soft-deleted.
async fn find_live(
    db: &impl ConnectionTrait,
    id: Uuid,
) -> Result<<Entity as EntityTrait>::Model, OxidGeneError> {
    crate::repo::find_live::<Entity>(db, id, Column::DeletedAt, "Repository").await
}
