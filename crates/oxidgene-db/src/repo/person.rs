//! Repository for `Person` entities (CRUD with soft delete).
//!
//! Free-text person search lives in [`crate::repo::PersonSearchRepo`]
//! (the `person_search_fts` table) since Sprint E.6.

use chrono::Utc;
use oxidgene_core::enums::{Privacy, Sex};
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::{Connection, Person, Portrait};
use sea_orm::entity::prelude::*;
use sea_orm::{
    ActiveModelTrait, Condition, ConnectionTrait, IntoActiveModel, JoinType, QueryFilter,
    QuerySelect, Set,
};
use uuid::Uuid;

use crate::entities::person::{self, ActiveModel, Column, Entity};
use crate::entities::person_name;
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
    #[doc(hidden)]
    #[serde(skip)]
    pub thumbnail_key: Option<String>,
    #[doc(hidden)]
    #[serde(skip)]
    pub storage_key: Option<String>,
    #[doc(hidden)]
    #[serde(skip)]
    pub mime_type: String,
    #[doc(hidden)]
    #[serde(skip)]
    pub crop: Option<(i32, i32, i32, i32)>,
    /// The pixel size of the image the crop was measured against, when it is
    /// known. Absent for a remote file nobody has measured yet, which is why
    /// it is two options and not a pair.
    #[doc(hidden)]
    #[serde(skip)]
    pub source_size: (Option<i32>, Option<i32>),
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

    /// List persons in a tree, optionally matching any stored name.
    pub async fn list_filtered(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        search: Option<&str>,
        params: &PaginationParams,
    ) -> Result<Connection<Person>, OxidGeneError> {
        let mut query = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::DeletedAt.is_null());
        if let Some(search) = search.map(str::trim).filter(|value| !value.is_empty()) {
            query = query
                .join(JoinType::InnerJoin, person::Relation::PersonName.def())
                .filter(
                    Condition::any()
                        .add(person_name::Column::GivenNames.contains(search))
                        .add(person_name::Column::Surname.contains(search))
                        .add(person_name::Column::Nickname.contains(search)),
                )
                .distinct();
        }
        paginate(db, query, Column::Id, params, |model| {
            (model.id, into_domain(model))
        })
        .await
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

    /// Get a person only when it belongs to the requested tree.
    pub async fn get_in_tree(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        id: Uuid,
    ) -> Result<Person, OxidGeneError> {
        Entity::find_by_id(id)
            .filter(Column::TreeId.eq(tree_id))
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

    /// Every person's portrait in a tree, with enough to draw it.
    ///
    /// `has_thumbnail` says whether we hold rasterised bytes for the media, so
    /// a caller knows to use our thumbnail rather than the producer's path —
    /// which is not a URL anything can load.
    pub async fn list_portraits(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<PortraitRow>, OxidGeneError> {
        Self::list_portraits_matching(db, tree_id, None).await
    }

    /// Resolve portraits only for the supplied people.
    pub async fn list_portraits_for(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        person_ids: &[Uuid],
    ) -> Result<Vec<PortraitRow>, OxidGeneError> {
        if person_ids.is_empty() {
            return Ok(Vec::new());
        }
        Self::list_portraits_matching(db, tree_id, Some(person_ids)).await
    }

    async fn list_portraits_matching(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        person_ids: Option<&[Uuid]>,
    ) -> Result<Vec<PortraitRow>, OxidGeneError> {
        use sea_orm::{DbBackend, Statement, Value};

        let backend = db.get_database_backend();
        let placeholder = if matches!(backend, DbBackend::Sqlite) {
            "?"
        } else {
            "$1"
        };
        let mut values: Vec<Value> = vec![tree_id.into()];
        let person_filter = person_ids
            .map(|ids| {
                let placeholders = ids
                    .iter()
                    .enumerate()
                    .map(|(index, id)| {
                        values.push((*id).into());
                        if matches!(backend, DbBackend::Sqlite) {
                            "?".to_string()
                        } else {
                            format!("${}", index + 2)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("AND p.id IN ({placeholders})")
            })
            .unwrap_or_default();
        // Two questions in one query.
        //
        // First, what represents each person: the portrait they chose, or —
        // when they have chosen none — their first linked photograph. That
        // fallback lets imported photographs represent people even when the
        // source does not specify a portrait choice.
        //
        // Only a medium that can actually be drawn qualifies as a fallback: one
        // we have rasterised, or a remote URL we recorded. A PDF or a record
        // naming a file nobody uploaded is not somebody's portrait by default.
        //
        // Either way the answer is then resolved to a page: what a reader
        // chooses is a tile, and a tile is a document. A document holds no
        // pixels, so reporting one would hand the caller an id whose bytes do
        // not exist — a silhouette where a photograph was chosen. The same
        // rule as the fallback's, applied once at the end so a chosen portrait
        // and an inferred one cannot disagree.
        //
        // Second, where to fetch it: a vignette resolves through the media it
        // crops, so one query answers both shapes and the caller never asks
        // twice.
        let sql = format!(
            r#"
                WITH chosen AS (
                    SELECT p.id AS person_id,
                           p.portrait_vignette_id,
                           COALESCE(p.portrait_media_id, CASE
                             -- Only when nothing was chosen at all. A crop is a
                             -- choice, and filling the media column beside it
                             -- would report both, which is not a state the
                             -- model holds.
                             WHEN p.portrait_vignette_id IS NULL THEN (
                               -- A link names a document, and a document holds
                               -- no pixels: it is its first drawable page that
                               -- can represent somebody. A link that names a
                               -- page already points at the pixels and is used
                               -- as it stands. Resolving each candidate first
                               -- and filtering afterwards keeps the link order
                               -- authoritative — picking a link and then
                               -- discovering it resolves to nothing would skip
                               -- the person's other photographs.
                               SELECT candidate.drawable
                               FROM (
                                 SELECT COALESCE(
                                          (SELECT page.id
                                           FROM media page
                                           WHERE page.parent_media_id = mm.id
                                             AND page.deleted_at IS NULL
                                             AND (page.thumbnail_key IS NOT NULL
                                                  OR page.file_path LIKE 'http%')
                                           ORDER BY page.page_index, page.id
                                           LIMIT 1),
                                          CASE
                                            WHEN mm.parent_media_id IS NOT NULL
                                                 AND (mm.thumbnail_key IS NOT NULL
                                                      OR mm.file_path LIKE 'http%')
                                            THEN mm.id
                                          END
                                        ) AS drawable,
                                        ml.sort_order AS sort_order,
                                        ml.id AS link_id
                                 FROM media_link ml
                                 INNER JOIN media mm
                                     ON mm.id = ml.media_id AND mm.deleted_at IS NULL
                                 WHERE ml.person_id = p.id
                               ) AS candidate
                               WHERE candidate.drawable IS NOT NULL
                               ORDER BY candidate.sort_order, candidate.link_id
                               LIMIT 1
                             )
                           END) AS portrait_media_id
                    FROM person p
                    WHERE p.tree_id = {placeholder}
                      AND p.deleted_at IS NULL
                                            {person_filter}
                ),
                resolved AS (
                    SELECT c.person_id,
                           c.portrait_vignette_id,
                           COALESCE(
                             (SELECT page.id
                              FROM media page
                              WHERE page.parent_media_id = c.portrait_media_id
                                AND page.deleted_at IS NULL
                                AND (page.thumbnail_key IS NOT NULL
                                     OR page.file_path LIKE 'http%')
                              ORDER BY page.page_index, page.id
                              LIMIT 1),
                             c.portrait_media_id
                           ) AS portrait_media_id
                    FROM chosen c
                )
                SELECT r.person_id,
                       r.portrait_media_id,
                       r.portrait_vignette_id,
                       COALESCE(m.file_path, vm.file_path) AS file_path,
                                             COALESCE(m.thumbnail_key, vm.thumbnail_key) AS thumbnail_key,
                                             COALESCE(m.storage_key, vm.storage_key) AS storage_key,
                                             COALESCE(m.mime_type, vm.mime_type) AS mime_type,
                                             COALESCE(m.width, vm.width) AS source_width,
                                             COALESCE(m.height, vm.height) AS source_height,
                                             v.x AS crop_x,
                                             v.y AS crop_y,
                                             v.width AS crop_width,
                                             v.height AS crop_height
                FROM resolved r
                LEFT JOIN media m ON m.id = r.portrait_media_id AND m.deleted_at IS NULL
                LEFT JOIN vignette v ON v.id = r.portrait_vignette_id
                LEFT JOIN media vm ON vm.id = v.media_id AND vm.deleted_at IS NULL
                WHERE r.portrait_media_id IS NOT NULL
                   OR r.portrait_vignette_id IS NOT NULL
            "#
        );
        let stmt = Statement::from_sql_and_values(backend, &sql, values);
        let results = db
            .query_all_raw(stmt)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;

        let mut rows = Vec::with_capacity(results.len());
        for row in results {
            let get = |name: &str| row.try_get::<Option<Uuid>>("", name);
            let thumbnail_key = row
                .try_get::<Option<String>>("", "thumbnail_key")
                .map_err(|e| OxidGeneError::Database(e.to_string()))?;
            let crop_x = row
                .try_get::<Option<i32>>("", "crop_x")
                .map_err(|e| OxidGeneError::Database(e.to_string()))?;
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
                has_thumbnail: thumbnail_key.is_some(),
                thumbnail_key,
                storage_key: row
                    .try_get::<Option<String>>("", "storage_key")
                    .map_err(|e| OxidGeneError::Database(e.to_string()))?
                    .filter(|key| !key.is_empty()),
                mime_type: row
                    .try_get::<Option<String>>("", "mime_type")
                    .map_err(|e| OxidGeneError::Database(e.to_string()))?
                    .unwrap_or_default(),
                crop: match crop_x {
                    Some(x) => Some((
                        x,
                        row.try_get("", "crop_y")
                            .map_err(|e| OxidGeneError::Database(e.to_string()))?,
                        row.try_get("", "crop_width")
                            .map_err(|e| OxidGeneError::Database(e.to_string()))?,
                        row.try_get("", "crop_height")
                            .map_err(|e| OxidGeneError::Database(e.to_string()))?,
                    )),
                    None => None,
                },
                source_size: (
                    row.try_get::<Option<i32>>("", "source_width")
                        .map_err(|e| OxidGeneError::Database(e.to_string()))?,
                    row.try_get::<Option<i32>>("", "source_height")
                        .map_err(|e| OxidGeneError::Database(e.to_string()))?,
                ),
            });
        }
        Ok(rows)
    }

    /// Set or clear the portrait, writing both columns from a single value.
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
