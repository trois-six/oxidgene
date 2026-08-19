//! Repository for `Person` entities (CRUD with soft delete).
//!
//! Free-text person search lives in [`crate::repo::PersonSearchRepo`]
//! (the `person_search_fts` table) since Sprint E.6.

use chrono::Utc;
use oxidgene_core::enums::{Privacy, Sex};
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::{Connection, Person, Portrait};
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveModelTrait, ConnectionTrait, IntoActiveModel, QueryFilter, Set};
use uuid::Uuid;

use crate::entities::person::{self, ActiveModel, Column, Entity};
use crate::entities::sea_enums;
use crate::repo::pagination::{PaginationParams, paginate};

/// Repository for person CRUD operations.
pub struct PersonRepo;

/// One person's portrait, flat, with what a caller needs to draw it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PortraitRow {
    pub person_id: Uuid,
    pub media_id: Option<Uuid>,
    pub vignette_id: Option<Uuid>,
    /// The producer's own path. Only useful when it is an `http(s)` URL — a
    /// remote media we recorded and never fetched.
    pub file_path: String,
    pub has_thumbnail: bool,
}

impl PersonRepo {
    /// List persons in a tree with pagination (excludes soft-deleted).
    pub async fn list(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        params: &PaginationParams,
    ) -> Result<Connection<Person>, OxidGeneError> {
        let query = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::DeletedAt.is_null());
        paginate(db, query, Column::Id, params, |m| (m.id, into_domain(m))).await
    }

    /// List all persons in a tree without pagination (excludes soft-deleted).
    pub async fn list_all(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<Person>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::DeletedAt.is_null())
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// Get multiple persons by ID (excludes soft-deleted).
    pub async fn get_many(
        db: &impl ConnectionTrait,
        ids: &[Uuid],
    ) -> Result<Vec<Person>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::Id.is_in(ids.iter().copied()))
            .filter(Column::DeletedAt.is_null())
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// Get a single person by ID (excludes soft-deleted).
    pub async fn get(db: &impl ConnectionTrait, id: Uuid) -> Result<Person, OxidGeneError> {
        Entity::find_by_id(id)
            .filter(Column::DeletedAt.is_null())
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .map(into_domain)
            .ok_or(OxidGeneError::NotFound {
                entity: "Person",
                id,
            })
    }

    /// Create a new person.
    pub async fn create(
        db: &impl ConnectionTrait,
        id: Uuid,
        tree_id: Uuid,
        sex: Sex,
    ) -> Result<Person, OxidGeneError> {
        let now = Utc::now();
        let model = person::ActiveModel {
            id: Set(id),
            tree_id: Set(tree_id),
            sex: Set(sea_enums::Sex::from(sex)),
            privacy: Set(sea_enums::Privacy::from(Privacy::default())),
            portrait_media_id: Set(None),
            portrait_vignette_id: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
            deleted_at: Set(None),
        };
        let result = model
            .insert(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(into_domain(result))
    }

    /// Update a person's sex and/or privacy.
    pub async fn update(
        db: &impl ConnectionTrait,
        id: Uuid,
        sex: Option<Sex>,
        privacy: Option<Privacy>,
    ) -> Result<Person, OxidGeneError> {
        let existing = Entity::find_by_id(id)
            .filter(Column::DeletedAt.is_null())
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .ok_or(OxidGeneError::NotFound {
                entity: "Person",
                id,
            })?;

        let mut active: ActiveModel = existing.into_active_model();
        if let Some(sex) = sex {
            active.sex = Set(sea_enums::Sex::from(sex));
        }
        if let Some(privacy) = privacy {
            active.privacy = Set(sea_enums::Privacy::from(privacy));
        }
        active.updated_at = Set(Utc::now());

        let result = active
            .update(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(into_domain(result))
    }

    /// Soft-delete a person.
    pub async fn delete(db: &impl ConnectionTrait, id: Uuid) -> Result<(), OxidGeneError> {
        let existing = Entity::find_by_id(id)
            .filter(Column::DeletedAt.is_null())
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .ok_or(OxidGeneError::NotFound {
                entity: "Person",
                id,
            })?;

        let mut active: ActiveModel = existing.into_active_model();
        active.deleted_at = Set(Some(Utc::now()));
        active
            .update(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(())
    }

    /// Set — or clear — which image represents a person.
    ///
    /// One row, one write. The two columns are written together from a single
    /// [`Portrait`], so "media *and* vignette" is not a state this can produce,
    /// and no caller has to clear the other one first.
    /// Every person's portrait in a tree, with enough to draw it.
    ///
    /// `has_thumbnail` says whether we hold rasterised bytes for the media, so
    /// a caller knows to use our thumbnail rather than the producer's path —
    /// which is not a URL anything can load.
    pub async fn list_portraits(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<PortraitRow>, OxidGeneError> {
        use sea_orm::{DbBackend, Statement};

        let backend = db.get_database_backend();
        let placeholder = if matches!(backend, DbBackend::Sqlite) {
            "?"
        } else {
            "$1"
        };
        // Two questions in one query.
        //
        // First, what represents each person: the portrait they chose, or —
        // when they have chosen none — their first linked photograph. That
        // fallback is not a nicety: no import sets a portrait, because neither
        // GEDCOM nor a `.gw` says which of somebody's pictures represents
        // them, so without it a freshly imported tree draws silhouettes for
        // everyone who has photographs.
        //
        // Only a medium that can actually be drawn qualifies as a fallback: one
        // we have rasterised, or a remote URL we recorded. A PDF or a record
        // naming a file nobody uploaded is not somebody's portrait by default.
        //
        // Second, where to fetch it: a vignette resolves through the media it
        // crops, so one query answers both shapes and the caller never asks
        // twice.
        let sql = format!(
            r#"
                WITH resolved AS (
                    SELECT p.id AS person_id,
                           p.portrait_vignette_id,
                           COALESCE(p.portrait_media_id, CASE
                             -- Only when nothing was chosen at all. A crop is a
                             -- choice, and filling the media column beside it
                             -- would report both, which is not a state the
                             -- model holds.
                             WHEN p.portrait_vignette_id IS NULL THEN (
                               SELECT ml.media_id
                               FROM media_link ml
                               INNER JOIN media mm
                                   ON mm.id = ml.media_id AND mm.deleted_at IS NULL
                               WHERE ml.person_id = p.id
                                 AND mm.parent_media_id IS NULL
                                 AND (mm.thumbnail_key IS NOT NULL
                                      OR mm.file_path LIKE 'http%')
                               ORDER BY ml.sort_order, ml.id
                               LIMIT 1
                             )
                           END) AS portrait_media_id
                    FROM person p
                    WHERE p.tree_id = {placeholder}
                      AND p.deleted_at IS NULL
                )
                SELECT r.person_id,
                       r.portrait_media_id,
                       r.portrait_vignette_id,
                       COALESCE(m.file_path, vm.file_path) AS file_path,
                       COALESCE(m.thumbnail_key, vm.thumbnail_key) AS thumbnail_key
                FROM resolved r
                LEFT JOIN media m ON m.id = r.portrait_media_id AND m.deleted_at IS NULL
                LEFT JOIN vignette v ON v.id = r.portrait_vignette_id
                LEFT JOIN media vm ON vm.id = v.media_id AND vm.deleted_at IS NULL
                WHERE r.portrait_media_id IS NOT NULL
                   OR r.portrait_vignette_id IS NOT NULL
            "#
        );
        let stmt = Statement::from_sql_and_values(backend, &sql, vec![tree_id.into()]);
        let results = db
            .query_all(stmt)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;

        let mut rows = Vec::with_capacity(results.len());
        for row in results {
            let get = |name: &str| row.try_get::<Option<Uuid>>("", name);
            rows.push(PortraitRow {
                person_id: row
                    .try_get("", "person_id")
                    .map_err(|e| OxidGeneError::Database(e.to_string()))?,
                media_id: get("portrait_media_id")
                    .map_err(|e| OxidGeneError::Database(e.to_string()))?,
                vignette_id: get("portrait_vignette_id")
                    .map_err(|e| OxidGeneError::Database(e.to_string()))?,
                file_path: row
                    .try_get::<Option<String>>("", "file_path")
                    .map_err(|e| OxidGeneError::Database(e.to_string()))?
                    .unwrap_or_default(),
                has_thumbnail: row
                    .try_get::<Option<String>>("", "thumbnail_key")
                    .map_err(|e| OxidGeneError::Database(e.to_string()))?
                    .is_some(),
            });
        }
        Ok(rows)
    }

    pub async fn set_portrait(
        db: &impl ConnectionTrait,
        person_id: Uuid,
        portrait: Portrait,
    ) -> Result<Person, OxidGeneError> {
        let person = Entity::find_by_id(person_id)
            .filter(Column::DeletedAt.is_null())
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .ok_or(OxidGeneError::NotFound {
                entity: "Person",
                id: person_id,
            })?;

        let (media_id, vignette_id) = portrait.to_columns();
        let mut active: person::ActiveModel = person.into();
        active.portrait_media_id = Set(media_id);
        active.portrait_vignette_id = Set(vignette_id);
        active.updated_at = Set(Utc::now());
        let result = active
            .update(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(into_domain(result))
    }

    /// Forget any portrait pointing at a media, or at a crop of one.
    ///
    /// Called when a media is deleted: the pointer is not a foreign key — SQLite
    /// cannot add one through `ALTER TABLE` — so nothing else would clear it, and
    /// a card would go on asking for bytes that are gone.
    pub async fn clear_portraits_for_media(
        db: &impl ConnectionTrait,
        media_id: Uuid,
        vignette_ids: &[Uuid],
    ) -> Result<(), OxidGeneError> {
        Entity::update_many()
            .col_expr(
                Column::PortraitMediaId,
                sea_orm::sea_query::Expr::value(None::<Uuid>),
            )
            .filter(Column::PortraitMediaId.eq(media_id))
            .exec(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        if !vignette_ids.is_empty() {
            Entity::update_many()
                .col_expr(
                    Column::PortraitVignetteId,
                    sea_orm::sea_query::Expr::value(None::<Uuid>),
                )
                .filter(Column::PortraitVignetteId.is_in(vignette_ids.to_vec()))
                .exec(db)
                .await
                .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        }
        Ok(())
    }
}

fn into_domain(m: person::Model) -> Person {
    Person {
        id: m.id,
        tree_id: m.tree_id,
        sex: m.sex.into(),
        privacy: m.privacy.into(),
        portrait_media_id: m.portrait_media_id,
        portrait_vignette_id: m.portrait_vignette_id,
        created_at: m.created_at,
        updated_at: m.updated_at,
        deleted_at: m.deleted_at,
    }
}
