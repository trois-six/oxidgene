//! Repository for `Citation` entities (CRUD, no soft delete).

use chrono::Utc;
use oxidgene_core::enums::Confidence;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::{Citation, Connection, RecordKind};
use sea_orm::entity::prelude::*;
use sea_orm::{
    ActiveModelTrait, Condition, ConnectionTrait, IntoActiveModel, JoinType, QueryFilter,
    QuerySelect, Set,
};
use uuid::Uuid;

use crate::entities::citation::{self, ActiveModel, Column, Entity};
use crate::entities::sea_enums;
use crate::entities::source;
use crate::repo::batch::in_chunks;
use crate::repo::db_err;
use crate::repo::pagination::{PaginationParams, paginate};
use crate::repo::record_tag::RecordTagRepo;

/// Optional entity filters for listing citations.
#[derive(Debug, Clone, Default)]
pub struct CitationFilter {
    pub person_id: Option<Uuid>,
    pub event_id: Option<Uuid>,
    pub family_id: Option<Uuid>,
    pub source_id: Option<Uuid>,
}

/// Repository for citation operations.
pub struct CitationRepo;

impl CitationRepo {
    /// List citations in a tree with optional entity filters and pagination.
    pub async fn list(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        filter: &CitationFilter,
        params: &PaginationParams,
    ) -> Result<Connection<Citation>, OxidGeneError> {
        let mut query = Entity::find()
            .join(JoinType::InnerJoin, citation::Relation::Source.def())
            .filter(source::Column::TreeId.eq(tree_id))
            .filter(source::Column::DeletedAt.is_null());

        if let Some(person_id) = filter.person_id {
            query = query.filter(Column::PersonId.eq(person_id));
        }
        if let Some(event_id) = filter.event_id {
            query = query.filter(Column::EventId.eq(event_id));
        }
        if let Some(family_id) = filter.family_id {
            query = query.filter(Column::FamilyId.eq(family_id));
        }
        if let Some(source_id) = filter.source_id {
            query = query.filter(Column::SourceId.eq(source_id));
        }

        let mut page = paginate(db, query, Column::Id, params, |model| {
            (model.id, into_domain(model))
        })
        .await?;
        let citations = page.edges.iter().map(|edge| edge.node.clone()).collect();
        for (edge, citation) in page.edges.iter_mut().zip(hydrate(db, citations).await?) {
            edge.node = citation;
        }
        Ok(page)
    }

    /// List all citations belonging to sources in one tree.
    pub async fn list_all(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<Citation>, OxidGeneError> {
        let models = Entity::find()
            .join(JoinType::InnerJoin, citation::Relation::Source.def())
            .filter(source::Column::TreeId.eq(tree_id))
            .filter(source::Column::DeletedAt.is_null())
            .all(db)
            .await
            .map_err(db_err)?;
        hydrate(db, models.into_iter().map(into_domain).collect()).await
    }

    /// List citations directly linked to any of the given persons.
    pub async fn list_by_persons(
        db: &impl ConnectionTrait,
        person_ids: &[Uuid],
    ) -> Result<Vec<Citation>, OxidGeneError> {
        let citations = in_chunks(person_ids, |chunk| async move {
            let models = Entity::find()
                .filter(Column::PersonId.is_in(chunk))
                .all(db)
                .await
                .map_err(db_err)?;
            Ok(models.into_iter().map(into_domain).collect())
        })
        .await?;
        hydrate(db, citations).await
    }

    /// Citations directly attached to a person or one of the supplied events.
    pub async fn list_for_person_events(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        person_id: Uuid,
        event_ids: &[Uuid],
    ) -> Result<Vec<Citation>, OxidGeneError> {
        let mut targets = Condition::any().add(Column::PersonId.eq(person_id));
        if !event_ids.is_empty() {
            targets = targets.add(Column::EventId.is_in(event_ids.iter().copied()));
        }
        Self::list_live(db, tree_id, targets).await
    }

    /// Citations of tree `tree_id` attached directly to any of `person_ids`,
    /// from live sources.
    pub async fn list_for_persons(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        person_ids: &[Uuid],
    ) -> Result<Vec<Citation>, OxidGeneError> {
        in_chunks(person_ids, |chunk| {
            Self::list_live(
                db,
                tree_id,
                Condition::all().add(Column::PersonId.is_in(chunk)),
            )
        })
        .await
    }

    /// Citations of tree `tree_id` attached to any of `event_ids`, from live
    /// sources.
    pub async fn list_for_events(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        event_ids: &[Uuid],
    ) -> Result<Vec<Citation>, OxidGeneError> {
        in_chunks(event_ids, |chunk| {
            Self::list_live(
                db,
                tree_id,
                Condition::all().add(Column::EventId.is_in(chunk)),
            )
        })
        .await
    }

    /// The citations of tree `tree_id` matching `targets` whose source is live.
    async fn list_live(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        targets: Condition,
    ) -> Result<Vec<Citation>, OxidGeneError> {
        let models = Entity::find()
            .join(JoinType::InnerJoin, citation::Relation::Source.def())
            .filter(source::Column::TreeId.eq(tree_id))
            .filter(source::Column::DeletedAt.is_null())
            .filter(targets)
            .all(db)
            .await
            .map_err(db_err)?;
        hydrate(db, models.into_iter().map(into_domain).collect()).await
    }

    /// List citations for multiple sources.
    pub async fn list_by_sources(
        db: &impl ConnectionTrait,
        source_ids: &[Uuid],
    ) -> Result<Vec<Citation>, OxidGeneError> {
        let citations = in_chunks(source_ids, |chunk| async move {
            let models = Entity::find()
                .filter(Column::SourceId.is_in(chunk))
                .all(db)
                .await
                .map_err(db_err)?;
            Ok(models.into_iter().map(into_domain).collect())
        })
        .await?;
        hydrate(db, citations).await
    }

    /// Get a single citation by ID.
    pub async fn get(db: &impl ConnectionTrait, id: Uuid) -> Result<Citation, OxidGeneError> {
        let citation = Entity::find_by_id(id)
            .one(db)
            .await
            .map_err(db_err)?
            .map(into_domain)
            .ok_or(OxidGeneError::NotFound {
                entity: "Citation",
                id,
            })?;
        hydrate_one(db, citation).await
    }

    /// The citations among `ids`, one read per bounded slice of ids. Their
    /// tags are not filled: this serves the labels and owners of many records
    /// at once. A missing id is absent.
    pub async fn get_many_untagged(
        db: &impl ConnectionTrait,
        ids: &[Uuid],
    ) -> Result<Vec<Citation>, OxidGeneError> {
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

    /// Create a new citation.
    #[expect(
        clippy::too_many_arguments,
        reason = "one parameter per column the statement writes, already validated by the calling service"
    )]
    pub async fn create(
        db: &impl ConnectionTrait,
        id: Uuid,
        source_id: Uuid,
        person_id: Option<Uuid>,
        event_id: Option<Uuid>,
        family_id: Option<Uuid>,
        page: Option<String>,
        confidence: Option<Confidence>,
        text: Option<String>,
    ) -> Result<Citation, OxidGeneError> {
        let now = Utc::now();
        let model = citation::ActiveModel {
            id: Set(id),
            source_id: Set(source_id),
            person_id: Set(person_id),
            event_id: Set(event_id),
            family_id: Set(family_id),
            page: Set(page),
            confidence: Set(confidence.map(sea_enums::Confidence::from)),
            text: Set(text),
            created_at: Set(now),
            updated_at: Set(now),
        };
        let result = model.insert(db).await.map_err(db_err)?;
        hydrate_one(db, into_domain(result)).await
    }

    /// Update a citation.
    /// Updates a citation in place. `source_id` can be repointed at another
    /// source: a citation carries which record backs a fact, and correcting
    /// that record is an edit of the same citation — deleting and recreating
    /// it instead would strand the row every reference to it points at.
    pub async fn update(
        db: &impl ConnectionTrait,
        id: Uuid,
        source_id: Option<Uuid>,
        page: Option<Option<String>>,
        confidence: Option<Option<Confidence>>,
        text: Option<Option<String>>,
    ) -> Result<Citation, OxidGeneError> {
        let existing = Entity::find_by_id(id)
            .one(db)
            .await
            .map_err(db_err)?
            .ok_or(OxidGeneError::NotFound {
                entity: "Citation",
                id,
            })?;

        let mut active: ActiveModel = existing.into_active_model();
        if let Some(source_id) = source_id {
            active.source_id = Set(source_id);
        }
        if let Some(page) = page {
            active.page = Set(page);
        }
        if let Some(confidence) = confidence {
            active.confidence = Set(confidence.map(sea_enums::Confidence::from));
        }
        if let Some(text) = text {
            active.text = Set(text);
        }
        active.updated_at = Set(Utc::now());

        let result = active.update(db).await.map_err(db_err)?;
        hydrate_one(db, into_domain(result)).await
    }

    /// Hard-delete a citation.
    pub async fn delete(db: &impl ConnectionTrait, id: Uuid) -> Result<(), OxidGeneError> {
        let result = Entity::delete_by_id(id).exec(db).await.map_err(db_err)?;
        if result.rows_affected == 0 {
            return Err(OxidGeneError::NotFound {
                entity: "Citation",
                id,
            });
        }
        Ok(())
    }
}

/// Fill the tags of each citation from one grouped read.
async fn hydrate(
    db: &impl ConnectionTrait,
    mut citations: Vec<Citation>,
) -> Result<Vec<Citation>, OxidGeneError> {
    let ids: Vec<Uuid> = citations.iter().map(|citation| citation.id).collect();
    let mut tags = RecordTagRepo::tags_of(db, RecordKind::Citation, &ids).await?;
    for citation in &mut citations {
        citation.tags = tags.remove(&citation.id).unwrap_or_default();
    }
    Ok(citations)
}

/// [`hydrate`] for one citation.
async fn hydrate_one(
    db: &impl ConnectionTrait,
    citation: Citation,
) -> Result<Citation, OxidGeneError> {
    let mut citations = hydrate(db, vec![citation]).await?;
    Ok(citations.remove(0))
}

fn into_domain(m: citation::Model) -> Citation {
    Citation {
        id: m.id,
        source_id: m.source_id,
        person_id: m.person_id,
        event_id: m.event_id,
        family_id: m.family_id,
        page: m.page,
        confidence: m.confidence.map(Into::into),
        text: m.text,
        tags: Vec::new(),
        created_at: m.created_at,
        updated_at: m.updated_at,
    }
}
