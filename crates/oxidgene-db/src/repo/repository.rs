//! Repository for `Repository` entities — the archives and libraries holding
//! sources (CRUD) — and for the links saying which sources
//! each holds.

use std::collections::HashMap;

use chrono::Utc;
use oxidgene_core::enums::SourceMediaType;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::{Connection, KeptEntityKind, RecordKind, Repository, SourceRepository};
use sea_orm::entity::prelude::*;
use sea_orm::{
    ActiveModelTrait, ConnectionTrait, IntoActiveModel, QueryFilter, QueryOrder, QuerySelect, Set,
};
use uuid::Uuid;

use crate::entities::repository::{self, ActiveModel, Column, Entity};
use crate::entities::{note, source_repository};
use crate::repo::batch::in_chunks;
use crate::repo::pagination::{PaginationParams, paginate};
use crate::repo::record_tag::RecordTagRepo;
use crate::repo::redaction::LeaveOut;
use crate::repo::{GedcomKeptRepo, db_err};

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
    /// The repositories of a tree, a page at a time, without those
    /// `leave_out` names (`LeaveOut::NONE` for the owner).
    pub async fn list_visible(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        params: &PaginationParams,
        leave_out: LeaveOut,
    ) -> Result<Connection<Repository>, OxidGeneError> {
        let query = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .filter(leave_out.keeping(oxidgene_core::types::RecordKind::Repository, Column::Id));
        let mut page = paginate(db, query, Column::Id, params, |m| (m.id, into_domain(m))).await?;
        let repositories = page.edges.iter().map(|edge| edge.node.clone()).collect();
        for (edge, repository) in page.edges.iter_mut().zip(hydrate(db, repositories).await?) {
            edge.node = repository;
        }
        Ok(page)
    }

    /// Every repository of a tree.
    pub async fn list_all(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<Repository>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .all(db)
            .await
            .map_err(db_err)?;
        hydrate(db, models.into_iter().map(into_domain).collect()).await
    }

    /// A repository.
    pub async fn get(db: &impl ConnectionTrait, id: Uuid) -> Result<Repository, OxidGeneError> {
        let repository = find_row(db, id).await.map(into_domain)?;
        hydrate_one(db, repository).await
    }

    /// The repositories among `ids`, one read per bounded slice of ids.
    /// Their tags are not filled: this serves the labels of many records at once.
    pub async fn get_many_untagged(
        db: &impl ConnectionTrait,
        ids: &[Uuid],
    ) -> Result<Vec<Repository>, OxidGeneError> {
        in_chunks(ids, |chunk| async move {
            let models = Entity::find()
                .filter(Column::Id.is_in(chunk))
                .all(db)
                .await
                .map_err(db_err)?;
            Ok(models.into_iter().map(into_domain).collect())
        })
        .await
    }

    /// The repositories among `ids`.
    pub async fn get_many(
        db: &impl ConnectionTrait,
        ids: &[Uuid],
    ) -> Result<Vec<Repository>, OxidGeneError> {
        let repositories = Self::get_many_untagged(db, ids).await?;
        hydrate(db, repositories).await
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
        };
        let repository = model.insert(db).await.map_err(db_err).map(into_domain)?;
        hydrate_one(db, repository).await
    }

    pub async fn update(
        db: &impl ConnectionTrait,
        id: Uuid,
        patch: RepositoryPatch,
    ) -> Result<Repository, OxidGeneError> {
        let mut active: ActiveModel = find_row(db, id).await?.into_active_model();
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
        let repository = active.update(db).await.map_err(db_err).map(into_domain)?;
        hydrate_one(db, repository).await
    }

    /// Delete a repository, and with it every row that is its alone
    /// (docs/data-model.md, Deletion): the schema's cascades take the links
    /// of the sources held there, its notes and tags, and its kept GEDCOM
    /// goes first, in the same transaction.
    pub async fn delete(db: &impl ConnectionTrait, id: Uuid) -> Result<(), OxidGeneError> {
        find_row(db, id).await?;
        GedcomKeptRepo::delete_for(db, KeptEntityKind::Repository, &[id]).await?;
        Entity::delete_by_id(id).exec(db).await.map_err(db_err)?;
        Ok(())
    }

    /// Fold repository `absorbed` into repository `kept`, both of `tree_id`:
    /// the sources it holds become held at `kept` — a link `kept` already has,
    /// same source, call number and medium, is not doubled — its notes and
    /// tags move to `kept`, and `absorbed` is deleted. `kept`'s own fields
    /// are left as they are.
    pub async fn absorb(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        kept: Uuid,
        absorbed: Uuid,
    ) -> Result<(), OxidGeneError> {
        let links = source_repository::Entity::find()
            .filter(source_repository::Column::RepositoryId.is_in([kept, absorbed]))
            .all(db)
            .await
            .map_err(db_err)?;
        let held: Vec<_> = links
            .iter()
            .filter(|link| link.repository_id == kept)
            .map(|link| (link.source_id, link.call_number.clone(), link.media_type))
            .collect();
        let (doubled, moved): (Vec<_>, Vec<_>) = links
            .iter()
            .filter(|link| link.repository_id == absorbed)
            .partition(|link| {
                held.contains(&(link.source_id, link.call_number.clone(), link.media_type))
            });
        let ids = |links: &[&source_repository::Model]| -> Vec<Uuid> {
            links.iter().map(|link| link.id).collect()
        };
        source_repository::Entity::delete_many()
            .filter(source_repository::Column::Id.is_in(ids(&doubled)))
            .exec(db)
            .await
            .map_err(db_err)?;
        source_repository::Entity::update_many()
            .col_expr(source_repository::Column::RepositoryId, Expr::value(kept))
            .filter(source_repository::Column::Id.is_in(ids(&moved)))
            .exec(db)
            .await
            .map_err(db_err)?;
        note::Entity::update_many()
            .col_expr(note::Column::RepositoryId, Expr::value(kept))
            .filter(note::Column::RepositoryId.eq(absorbed))
            .exec(db)
            .await
            .map_err(db_err)?;
        RecordTagRepo::move_tags(db, tree_id, RecordKind::Repository, absorbed, kept).await?;
        Self::delete(db, absorbed).await
    }

    /// The sources linked to repository `id`, in ascending
    /// order: the sources a merge of it rewrites, whose state the history
    /// keeps before it does.
    pub async fn linked_sources(
        db: &impl ConnectionTrait,
        id: Uuid,
    ) -> Result<Vec<Uuid>, OxidGeneError> {
        let mut sources: Vec<Uuid> = source_repository::Entity::find()
            .select_only()
            .column(source_repository::Column::SourceId)
            .filter(source_repository::Column::RepositoryId.eq(id))
            .into_tuple()
            .all(db)
            .await
            .map_err(db_err)?;
        sources.sort_unstable();
        sources.dedup();
        Ok(sources)
    }

    /// Whether a source holds `id` or a note is about it.
    pub async fn is_used(db: &impl ConnectionTrait, id: Uuid) -> Result<bool, OxidGeneError> {
        let held = source_repository::Entity::find()
            .filter(source_repository::Column::RepositoryId.eq(id))
            .count(db)
            .await
            .map_err(db_err)?;
        if held > 0 {
            return Ok(true);
        }
        let noted = note::Entity::find()
            .filter(note::Column::RepositoryId.eq(id))
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

    /// The links of these sources, each source's in order.
    pub async fn list_by_sources(
        db: &impl ConnectionTrait,
        source_ids: &[Uuid],
    ) -> Result<Vec<SourceRepository>, OxidGeneError> {
        in_chunks(source_ids, |chunk| async move {
            let models = source_repository::Entity::find()
                .filter(source_repository::Column::SourceId.is_in(chunk))
                .order_by_asc(source_repository::Column::SortOrder)
                .order_by_asc(source_repository::Column::Id)
                .all(db)
                .await
                .map_err(db_err)?;
            Ok(models.into_iter().map(link_into_domain).collect())
        })
        .await
    }

    /// The links to a repository.
    pub async fn list_by_repository(
        db: &impl ConnectionTrait,
        repository_id: Uuid,
    ) -> Result<Vec<SourceRepository>, OxidGeneError> {
        Self::list_by_repositories(db, &[repository_id]).await
    }

    /// The links to these repositories, each repository's in order.
    pub async fn list_by_repositories(
        db: &impl ConnectionTrait,
        repository_ids: &[Uuid],
    ) -> Result<Vec<SourceRepository>, OxidGeneError> {
        in_chunks(repository_ids, |chunk| async move {
            let models = source_repository::Entity::find()
                .filter(source_repository::Column::RepositoryId.is_in(chunk))
                .order_by_asc(source_repository::Column::Id)
                .all(db)
                .await
                .map_err(db_err)?;
            Ok(models.into_iter().map(link_into_domain).collect())
        })
        .await
    }

    /// The names of the repositories holding each of these sources, in
    /// the sources' order, each name once.
    pub async fn repository_names(
        db: &impl ConnectionTrait,
        source_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, Vec<String>>, OxidGeneError> {
        let links = Self::list_by_sources(db, source_ids).await?;
        let repository_ids: Vec<Uuid> = links.iter().map(|l| l.repository_id).collect();
        let names: HashMap<Uuid, String> = RepositoryRepo::get_many_untagged(db, &repository_ids)
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
        let tree_id = crate::repo::tree_of::<crate::entities::source::Entity>(
            db,
            source_id,
            crate::entities::source::Column::TreeId,
            "Source",
        )
        .await?;
        source_repository::ActiveModel {
            id: Set(id),
            tree_id: Set(tree_id),
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
        tags: Vec::new(),
        created_at: m.created_at,
        updated_at: m.updated_at,
    }
}

/// Fill the tags of each repository from one grouped read.
async fn hydrate(
    db: &impl ConnectionTrait,
    mut repositories: Vec<Repository>,
) -> Result<Vec<Repository>, OxidGeneError> {
    let ids: Vec<Uuid> = repositories
        .iter()
        .map(|repository| repository.id)
        .collect();
    let mut tags = RecordTagRepo::tags_of(db, RecordKind::Repository, &ids).await?;
    for repository in &mut repositories {
        repository.tags = tags.remove(&repository.id).unwrap_or_default();
    }
    Ok(repositories)
}

/// [`hydrate`] for one repository.
async fn hydrate_one(
    db: &impl ConnectionTrait,
    repository: Repository,
) -> Result<Repository, OxidGeneError> {
    let mut repositories = hydrate(db, vec![repository]).await?;
    Ok(repositories.remove(0))
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

/// Row `id`; `NotFound` when it is missing.
async fn find_row(
    db: &impl ConnectionTrait,
    id: Uuid,
) -> Result<<Entity as EntityTrait>::Model, OxidGeneError> {
    crate::repo::find_row::<Entity>(db, id, "Repository").await
}
