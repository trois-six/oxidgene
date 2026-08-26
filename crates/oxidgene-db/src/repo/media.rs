//! Repository for `Media` entities (CRUD with soft delete).

use chrono::{DateTime, Utc};
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::{Connection, Media};
use sea_orm::entity::prelude::*;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, IntoActiveModel, QueryFilter,
    QueryOrder, Set,
};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::entities::media::{self, ActiveModel, Column, Entity};
use crate::entities::{media_link, media_tag, note, person, vignette};
use crate::repo::MediaTagRepo;
use crate::repo::pagination::{PaginationParams, paginate};

/// A file whose bytes are already in the media store, ready to be recorded.
#[derive(Debug, Clone)]
pub struct UploadedMedia {
    pub file_name: String,
    pub mime_type: String,
    pub storage_key: String,
    pub sha256: String,
    pub file_size: i64,
    pub thumbnail_key: Option<String>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub page_count: i32,
    pub title: Option<String>,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Fields a caller may change on a media record.
///
/// Every field is `Some`-means-change; the nested `Option` on the nullable
/// ones is what tells "clear this" from "leave it alone".
#[derive(Debug, Clone, Default)]
pub struct MediaPatch {
    pub title: Option<Option<String>>,
    pub description: Option<Option<String>>,
    pub date_value: Option<Option<String>>,
    pub date_value2: Option<Option<String>>,
    pub date_qualifier: Option<oxidgene_core::DateQualifier>,
    pub calendar: Option<oxidgene_core::Calendar>,
    pub place_id: Option<Option<Uuid>>,
    /// Where the file is. Only ever set for a media whose bytes we do *not*
    /// hold — a remote URL, or a GEDCOM record naming a file nobody uploaded.
    pub file_path: Option<String>,
    pub mime_type: Option<String>,
    pub privacy: Option<oxidgene_core::enums::Privacy>,
    pub source_media_type: Option<oxidgene_core::enums::SourceMediaType>,
    pub document_category: Option<Option<oxidgene_core::enums::DocumentCategory>>,
    /// Derived by the caller, never sent by a client. See [`MediaRepo::update`].
    pub date_sort: Option<Option<chrono::NaiveDate>>,
}

/// Store objects which became unreachable after a definitive deletion.
#[derive(Debug, Clone, Default)]
pub struct MediaPurge {
    pub storage_keys: Vec<String>,
}

/// Repository for media CRUD operations.
pub struct MediaRepo;

impl MediaRepo {
    /// List media in a tree with pagination (excludes soft-deleted).
    pub async fn list(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        params: &PaginationParams,
    ) -> Result<Connection<Media>, OxidGeneError> {
        // A document's pages are media too; listing them beside the document
        // they belong to would show a register nine times.
        let query = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::ParentMediaId.is_null())
            .filter(Column::DeletedAt.is_null());
        let mut connection =
            paginate(db, query, Column::Id, params, |m| (m.id, into_domain(m))).await?;
        let mut media: Vec<Media> = connection
            .edges
            .iter()
            .map(|edge| edge.node.clone())
            .collect();
        hydrate_tags(db, &mut media).await?;
        for (edge, media) in connection.edges.iter_mut().zip(media) {
            edge.node = media;
        }
        Ok(connection)
    }

    /// List all media in a tree without pagination (excludes soft-deleted).
    pub async fn list_all(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<Media>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::DeletedAt.is_null())
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        let mut media: Vec<Media> = models.into_iter().map(into_domain).collect();
        hydrate_tags(db, &mut media).await?;
        Ok(media)
    }

    /// Get multiple media items by ID (excludes soft-deleted).
    pub async fn get_many(
        db: &impl ConnectionTrait,
        ids: &[Uuid],
    ) -> Result<Vec<Media>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::Id.is_in(ids.iter().copied()))
            .filter(Column::DeletedAt.is_null())
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        let mut media: Vec<Media> = models.into_iter().map(into_domain).collect();
        hydrate_tags(db, &mut media).await?;
        Ok(media)
    }

    /// Get a single media by ID (excludes soft-deleted).
    pub async fn get(db: &impl ConnectionTrait, id: Uuid) -> Result<Media, OxidGeneError> {
        let mut media = Entity::find_by_id(id)
            .filter(Column::DeletedAt.is_null())
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .ok_or(OxidGeneError::NotFound {
                entity: "Media",
                id,
            })
            .map(into_domain)?;
        hydrate_tags(db, std::slice::from_mut(&mut media)).await?;
        Ok(media)
    }

    /// Create a media record that names a file without holding its bytes.
    ///
    /// This is the GEDCOM-import and metadata-only path: `file_path` is
    /// whatever the source said, and `storage_key` stays null until the file
    /// itself arrives. Use [`MediaRepo::create_uploaded`] when there are bytes.
    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        db: &impl ConnectionTrait,
        id: Uuid,
        tree_id: Uuid,
        file_name: String,
        mime_type: String,
        file_path: String,
        file_size: i64,
        title: Option<String>,
        description: Option<String>,
    ) -> Result<Media, OxidGeneError> {
        let now = Utc::now();
        let model = media::ActiveModel {
            id: Set(id),
            tree_id: Set(tree_id),
            file_name: Set(file_name),
            mime_type: Set(mime_type),
            file_path: Set(file_path),
            storage_key: Set(None),
            sha256: Set(None),
            thumbnail_key: Set(None),
            width: Set(None),
            height: Set(None),
            page_count: Set(1),
            parent_media_id: Set(None),
            page_index: Set(0),
            is_document: Set(false),
            file_size: Set(file_size),
            privacy: Set(oxidgene_core::enums::Privacy::default().into()),
            source_media_type: Set(oxidgene_core::enums::SourceMediaType::default().into()),
            document_category: Set(None),
            tags: Set("[]".to_string()),
            title: Set(title),
            description: Set(description),
            date_value: Set(None),
            date_sort: Set(None),
            date_qualifier: Set(oxidgene_core::DateQualifier::default().into()),
            date_value2: Set(None),
            calendar: Set(oxidgene_core::Calendar::default().into()),
            place_id: Set(None),
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

    /// Everything a media row records about a file whose bytes we hold.
    ///
    /// Grouped into a struct because passing eleven positional arguments —
    /// four of them `Option<i32>` — is a call site nobody can read.
    pub async fn create_uploaded(
        db: &impl ConnectionTrait,
        id: Uuid,
        tree_id: Uuid,
        upload: UploadedMedia,
    ) -> Result<Media, OxidGeneError> {
        let model = media::ActiveModel {
            id: Set(id),
            tree_id: Set(tree_id),
            // An uploaded file has no foreign path to preserve, so `file_path`
            // carries the name a GEDCOM export should write out.
            file_path: Set(upload.file_name.clone()),
            file_name: Set(upload.file_name),
            mime_type: Set(upload.mime_type),
            storage_key: Set(Some(upload.storage_key)),
            sha256: Set(Some(upload.sha256)),
            thumbnail_key: Set(upload.thumbnail_key),
            width: Set(upload.width),
            height: Set(upload.height),
            page_count: Set(upload.page_count),
            parent_media_id: Set(None),
            page_index: Set(0),
            is_document: Set(false),
            file_size: Set(upload.file_size),
            privacy: Set(oxidgene_core::enums::Privacy::default().into()),
            source_media_type: Set(oxidgene_core::enums::SourceMediaType::default().into()),
            document_category: Set(None),
            tags: Set("[]".to_string()),
            title: Set(upload.title),
            description: Set(upload.description),
            date_value: Set(None),
            date_sort: Set(None),
            date_qualifier: Set(oxidgene_core::DateQualifier::default().into()),
            date_value2: Set(None),
            calendar: Set(oxidgene_core::Calendar::default().into()),
            place_id: Set(None),
            created_at: Set(upload.created_at),
            updated_at: Set(Utc::now()),
            deleted_at: Set(None),
        };
        let result = model
            .insert(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(into_domain(result))
    }

    /// Attach stored bytes to a record that had none.
    ///
    /// The path a GEDCOM import left in `file_path` is kept: it is what the
    /// export has to write back, and now it also documents where the file came
    /// from before we had a copy.
    pub async fn attach_file(
        db: &impl ConnectionTrait,
        id: Uuid,
        upload: UploadedMedia,
    ) -> Result<Media, OxidGeneError> {
        let existing = Entity::find_by_id(id)
            .filter(Column::DeletedAt.is_null())
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .ok_or(OxidGeneError::NotFound {
                entity: "Media",
                id,
            })?;

        let mut active: ActiveModel = existing.into_active_model();
        active.mime_type = Set(upload.mime_type);
        active.storage_key = Set(Some(upload.storage_key));
        active.sha256 = Set(Some(upload.sha256));
        active.thumbnail_key = Set(upload.thumbnail_key);
        active.width = Set(upload.width);
        active.height = Set(upload.height);
        active.page_count = Set(upload.page_count);
        active.file_size = Set(upload.file_size);
        active.updated_at = Set(Utc::now());

        let result = active
            .update(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(into_domain(result))
    }

    /// Create an empty multi-page document.
    ///
    /// A document is a media with no bytes: the pages carry those. It exists
    /// before its first page is uploaded, which is what lets a user say "this
    /// is a register" and then add scans to it.
    pub async fn create_document(
        db: &impl ConnectionTrait,
        id: Uuid,
        tree_id: Uuid,
        title: Option<String>,
        created_at: DateTime<Utc>,
    ) -> Result<Media, OxidGeneError> {
        let name = title
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .unwrap_or("document")
            .to_string();
        let model = media::ActiveModel {
            id: Set(id),
            tree_id: Set(tree_id),
            file_name: Set(name.clone()),
            // Not a real MIME type of anything — the document has no bytes.
            // It names what the row is so a client branching on `mime_type`
            // alone does not mistake it for an image it can render.
            mime_type: Set("application/x-oxidgene-document".to_string()),
            file_path: Set(name),
            storage_key: Set(None),
            sha256: Set(None),
            thumbnail_key: Set(None),
            width: Set(None),
            height: Set(None),
            page_count: Set(0),
            parent_media_id: Set(None),
            page_index: Set(0),
            is_document: Set(true),
            file_size: Set(0),
            privacy: Set(oxidgene_core::enums::Privacy::default().into()),
            source_media_type: Set(oxidgene_core::enums::SourceMediaType::default().into()),
            document_category: Set(None),
            tags: Set("[]".to_string()),
            title: Set(title),
            description: Set(None),
            date_value: Set(None),
            date_sort: Set(None),
            date_qualifier: Set(oxidgene_core::DateQualifier::default().into()),
            date_value2: Set(None),
            calendar: Set(oxidgene_core::Calendar::default().into()),
            place_id: Set(None),
            created_at: Set(created_at),
            updated_at: Set(Utc::now()),
            deleted_at: Set(None),
        };
        let result = model
            .insert(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(into_domain(result))
    }

    /// The pages of a document, in order.
    pub async fn list_pages(
        db: &impl ConnectionTrait,
        document_id: Uuid,
    ) -> Result<Vec<Media>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::ParentMediaId.eq(document_id))
            .filter(Column::DeletedAt.is_null())
            .order_by_asc(Column::PageIndex)
            // A UUID v7 tie-break keeps two pages added in the same breath in
            // the order they arrived.
            .order_by_asc(Column::Id)
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// Make an uploaded media the next page of a document.
    ///
    /// Appends: the page index is the count of pages already there, so pages
    /// arrive in upload order without the caller tracking a counter.
    pub async fn append_page(
        db: &impl ConnectionTrait,
        document_id: Uuid,
        media_id: Uuid,
    ) -> Result<Media, OxidGeneError> {
        let document = Self::get(db, document_id).await?;
        if !document.is_document {
            return Err(OxidGeneError::Validation(
                "that media is not a multi-page document".into(),
            ));
        }
        if media_id == document_id {
            return Err(OxidGeneError::Validation(
                "a document cannot be a page of itself".into(),
            ));
        }
        let existing = Self::list_pages(db, document_id).await?;

        let page = Entity::find_by_id(media_id)
            .filter(Column::DeletedAt.is_null())
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .ok_or(OxidGeneError::NotFound {
                entity: "Media",
                id: media_id,
            })?;
        // One level only. A document holding documents is a shape nothing in
        // the viewer or the exporter knows how to walk.
        if page.is_document {
            return Err(OxidGeneError::Validation(
                "a document cannot be a page of another document".into(),
            ));
        }

        let mut active: ActiveModel = page.into_active_model();
        active.parent_media_id = Set(Some(document_id));
        active.page_index = Set(existing.len() as i32);
        active.updated_at = Set(Utc::now());
        let result = active
            .update(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;

        Self::refresh_page_count(db, document_id).await?;
        Ok(into_domain(result))
    }

    /// Set the order of a document's pages, by id.
    ///
    /// Takes the whole list rather than a move-one-page operation: reordering
    /// is a drag of the whole strip, and applying it as a sequence of single
    /// moves would let a failure halfway leave the pages in an order nobody
    /// asked for.
    pub async fn reorder_pages(
        db: &impl ConnectionTrait,
        document_id: Uuid,
        ordered: &[Uuid],
    ) -> Result<Vec<Media>, OxidGeneError> {
        let current = Self::list_pages(db, document_id).await?;
        let known: std::collections::HashSet<Uuid> = current.iter().map(|p| p.id).collect();
        if ordered.len() != current.len() || !ordered.iter().all(|id| known.contains(id)) {
            return Err(OxidGeneError::Validation(
                "the page order must list exactly this document's pages, once each".into(),
            ));
        }

        for (index, page_id) in ordered.iter().enumerate() {
            Entity::update_many()
                .col_expr(
                    Column::PageIndex,
                    sea_orm::sea_query::Expr::value(index as i32),
                )
                .filter(Column::Id.eq(*page_id))
                .exec(db)
                .await
                .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        }
        Self::list_pages(db, document_id).await
    }

    /// Detach a page from its document, leaving it as an ordinary media.
    pub async fn detach_page(
        db: &impl ConnectionTrait,
        page_id: Uuid,
    ) -> Result<Media, OxidGeneError> {
        let page = Entity::find_by_id(page_id)
            .filter(Column::DeletedAt.is_null())
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .ok_or(OxidGeneError::NotFound {
                entity: "Media",
                id: page_id,
            })?;
        let parent = page.parent_media_id;

        let mut active: ActiveModel = page.into_active_model();
        active.parent_media_id = Set(None);
        active.page_index = Set(0);
        active.updated_at = Set(Utc::now());
        let result = active
            .update(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;

        // Close the gap the removed page left, or page 5 of a 4-page document
        // is a number the viewer has to special-case.
        if let Some(parent) = parent {
            let remaining: Vec<Uuid> = Self::list_pages(db, parent)
                .await?
                .into_iter()
                .map(|p| p.id)
                .collect();
            Self::reorder_pages(db, parent, &remaining).await?;
            Self::refresh_page_count(db, parent).await?;
        }
        Ok(into_domain(result))
    }

    /// Recompute a document's `page_count` from the pages it actually has.
    ///
    /// Derived rather than incremented: an increment is one missed call away
    /// from a document that claims nine pages and shows eight.
    pub async fn refresh_page_count(
        db: &impl ConnectionTrait,
        document_id: Uuid,
    ) -> Result<(), OxidGeneError> {
        let count = Self::list_pages(db, document_id).await?.len() as i32;
        Entity::update_many()
            .col_expr(Column::PageCount, sea_orm::sea_query::Expr::value(count))
            .filter(Column::Id.eq(document_id))
            .exec(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(())
    }

    /// Find a tree's media record for a given content digest, if one exists.
    ///
    /// Lets an import that re-runs over an archive skip files it has already
    /// ingested, without comparing bytes it would have to fetch first.
    pub async fn find_by_sha256(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        sha256: &str,
    ) -> Result<Option<Media>, OxidGeneError> {
        let model = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::Sha256.eq(sha256))
            .filter(Column::DeletedAt.is_null())
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(model.map(into_domain))
    }

    /// Apply a patch to a media record.
    ///
    /// `date_sort` is not in the patch and never comes from a client: it is
    /// the normalized Gregorian date `calendar` + `date_value` imply, and
    /// converting a Julian or Republican date needs `ged_io`, which a WASM
    /// frontend cannot reach. The caller derives it and passes the result —
    /// exactly as the event write path does.
    pub async fn update(
        db: &impl ConnectionTrait,
        id: Uuid,
        patch: MediaPatch,
    ) -> Result<Media, OxidGeneError> {
        let existing = Entity::find_by_id(id)
            .filter(Column::DeletedAt.is_null())
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .ok_or(OxidGeneError::NotFound {
                entity: "Media",
                id,
            })?;

        let mut active: ActiveModel = existing.into_active_model();
        if let Some(title) = patch.title {
            active.title = Set(title);
        }
        if let Some(description) = patch.description {
            active.description = Set(description);
        }
        if let Some(date_value) = patch.date_value {
            active.date_value = Set(date_value);
        }
        if let Some(date_value2) = patch.date_value2 {
            active.date_value2 = Set(date_value2);
        }
        if let Some(qualifier) = patch.date_qualifier {
            active.date_qualifier = Set(qualifier.into());
        }
        if let Some(calendar) = patch.calendar {
            active.calendar = Set(calendar.into());
        }
        if let Some(place_id) = patch.place_id {
            active.place_id = Set(place_id);
        }
        if let Some(privacy) = patch.privacy {
            active.privacy = Set(privacy.into());
        }
        if let Some(source_media_type) = patch.source_media_type {
            active.source_media_type = Set(source_media_type.into());
        }
        if let Some(category) = patch.document_category {
            active.document_category = Set(category.map(|c| c.as_str().to_string()));
            // Choosing a category answers the GEDCOM question too. Setting
            // both explicitly in one request keeps the caller's medium; it is
            // only the unstated one that follows the category, so that a user
            // who classified a scan as a census return does not silently
            // export it as `OTHER`.
            if patch.source_media_type.is_none()
                && let Some(category) = category
            {
                active.source_media_type = Set(category.implied_medium().into());
            }
        }
        if let Some(file_path) = patch.file_path {
            // The name shown under a tile follows the path when the path is
            // all we have: a record repointed at a new URL should not keep
            // captioning itself with the old file's name.
            let derived_name = file_path
                .split(['?', '#'])
                .next()
                .unwrap_or(&file_path)
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(&file_path)
                .trim()
                .to_string();
            if !derived_name.is_empty() {
                active.file_name = Set(derived_name);
            }
            active.file_path = Set(file_path);
        }
        if let Some(mime_type) = patch.mime_type {
            active.mime_type = Set(mime_type);
        }
        if let Some(date_sort) = patch.date_sort {
            active.date_sort = Set(date_sort);
        }
        active.updated_at = Set(Utc::now());

        let result = active
            .update(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        let mut media = into_domain(result);
        hydrate_tags(db, std::slice::from_mut(&mut media)).await?;
        Ok(media)
    }

    /// Soft-delete a media record.
    pub async fn delete(db: &impl ConnectionTrait, id: Uuid) -> Result<(), OxidGeneError> {
        let existing = Entity::find_by_id(id)
            .filter(Column::DeletedAt.is_null())
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .ok_or(OxidGeneError::NotFound {
                entity: "Media",
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

    /// Delete a media record, every page it owns and every associated row.
    ///
    /// The returned keys are no longer used by another active media row; the
    /// API layer removes those objects from its configured media store after
    /// its transaction commits.
    pub async fn purge(db: &impl ConnectionTrait, id: Uuid) -> Result<MediaPurge, OxidGeneError> {
        let root = Entity::find_by_id(id)
            .filter(Column::DeletedAt.is_null())
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .ok_or(OxidGeneError::NotFound {
                entity: "Media",
                id,
            })?;
        let pages = if root.is_document {
            Entity::find()
                .filter(Column::ParentMediaId.eq(id))
                .filter(Column::DeletedAt.is_null())
                .all(db)
                .await
                .map_err(|e| OxidGeneError::Database(e.to_string()))?
        } else {
            Vec::new()
        };
        let page_ids: Vec<Uuid> = pages.iter().map(|page| page.id).collect();
        let mut media_ids = page_ids.clone();
        media_ids.push(id);

        let vignette_ids: Vec<Uuid> = vignette::Entity::find()
            .filter(vignette::Column::MediaId.is_in(media_ids.iter().copied()))
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .into_iter()
            .map(|vignette| vignette.id)
            .collect();

        // Content-addressed files can be shared. Preserve a key as long as a
        // different active media record still points at it.
        let candidate_keys: HashSet<String> = std::iter::once(&root)
            .chain(pages.iter())
            .flat_map(|media| {
                [media.storage_key.as_ref(), media.thumbnail_key.as_ref()]
                    .into_iter()
                    .flatten()
                    .cloned()
            })
            .collect();
        let retained_keys: HashSet<String> = Entity::find()
            .filter(Column::DeletedAt.is_null())
            .filter(Column::Id.is_not_in(media_ids.iter().copied()))
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .into_iter()
            .flat_map(|media| {
                [media.storage_key, media.thumbnail_key]
                    .into_iter()
                    .flatten()
            })
            .collect();

        media_tag::Entity::delete_many()
            .filter(media_tag::Column::MediaId.is_in(media_ids.iter().copied()))
            .exec(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        media_link::Entity::delete_many()
            .filter(media_link::Column::MediaId.is_in(media_ids.iter().copied()))
            .exec(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        note::Entity::delete_many()
            .filter(note::Column::MediaId.is_in(media_ids.iter().copied()))
            .exec(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;

        if !vignette_ids.is_empty() {
            person::Entity::update_many()
                .col_expr(
                    person::Column::PortraitVignetteId,
                    sea_orm::sea_query::Expr::value(Option::<Uuid>::None),
                )
                .filter(person::Column::PortraitVignetteId.is_in(vignette_ids.iter().copied()))
                .exec(db)
                .await
                .map_err(|e| OxidGeneError::Database(e.to_string()))?;
            vignette::Entity::delete_many()
                .filter(vignette::Column::Id.is_in(vignette_ids))
                .exec(db)
                .await
                .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        }
        person::Entity::update_many()
            .col_expr(
                person::Column::PortraitMediaId,
                sea_orm::sea_query::Expr::value(Option::<Uuid>::None),
            )
            .filter(person::Column::PortraitMediaId.is_in(media_ids.iter().copied()))
            .exec(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;

        if !page_ids.is_empty() {
            Entity::delete_many()
                .filter(Column::Id.is_in(page_ids))
                .exec(db)
                .await
                .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        }
        Entity::delete_by_id(id)
            .exec(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;

        Ok(MediaPurge {
            storage_keys: candidate_keys.difference(&retained_keys).cloned().collect(),
        })
    }

    /// Whether the gallery link being removed is the media's sole external
    /// reference. Another link, crop, portrait or parent document returns
    /// `false`, leaving the media eligible for neither a confirmation nor a
    /// conditional purge.
    pub async fn can_purge_if_unreferenced_elsewhere(
        db: &impl ConnectionTrait,
        id: Uuid,
        allowed_link_id: Uuid,
    ) -> Result<bool, OxidGeneError> {
        let media = Entity::find_by_id(id)
            .filter(Column::DeletedAt.is_null())
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .ok_or(OxidGeneError::NotFound {
                entity: "Media",
                id,
            })?;
        if media.parent_media_id.is_some() {
            return Ok(false);
        }
        let pages = if media.is_document {
            Entity::find()
                .filter(Column::ParentMediaId.eq(id))
                .filter(Column::DeletedAt.is_null())
                .all(db)
                .await
                .map_err(|e| OxidGeneError::Database(e.to_string()))?
        } else {
            Vec::new()
        };
        let mut media_ids: Vec<Uuid> = pages.iter().map(|page| page.id).collect();
        media_ids.push(id);

        let has_other_link = media_link::Entity::find()
            .filter(media_link::Column::MediaId.is_in(media_ids.iter().copied()))
            .filter(media_link::Column::Id.ne(allowed_link_id))
            .count(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            > 0;
        let has_vignette = vignette::Entity::find()
            .filter(vignette::Column::MediaId.is_in(media_ids.iter().copied()))
            .count(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            > 0;
        let has_portrait = person::Entity::find()
            .filter(person::Column::PortraitMediaId.is_in(media_ids.iter().copied()))
            .count(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            > 0;

        if has_other_link || has_vignette || has_portrait {
            return Ok(false);
        }
        Ok(true)
    }

    /// Purge only when [`Self::can_purge_if_unreferenced_elsewhere`] allows
    /// it. The eligibility is re-evaluated at deletion time because a second
    /// reference may have appeared after the UI asked whether to confirm.
    pub async fn purge_if_unreferenced_elsewhere(
        db: &impl ConnectionTrait,
        id: Uuid,
        allowed_link_id: Uuid,
    ) -> Result<Option<MediaPurge>, OxidGeneError> {
        if !Self::can_purge_if_unreferenced_elsewhere(db, id, allowed_link_id).await? {
            return Ok(None);
        }
        Self::purge(db, id).await.map(Some)
    }
}

pub(crate) fn into_domain(m: media::Model) -> Media {
    Media {
        id: m.id,
        tree_id: m.tree_id,
        file_name: m.file_name,
        mime_type: m.mime_type,
        file_path: m.file_path,
        storage_key: m.storage_key,
        sha256: m.sha256,
        thumbnail_key: m.thumbnail_key,
        width: m.width,
        height: m.height,
        page_count: m.page_count,
        parent_media_id: m.parent_media_id,
        page_index: m.page_index,
        is_document: m.is_document,
        file_size: m.file_size,
        title: m.title,
        description: m.description,
        date_value: m.date_value,
        date_sort: m.date_sort,
        date_qualifier: m.date_qualifier.into(),
        privacy: m.privacy.into(),
        source_media_type: m.source_media_type.into(),
        // A value the enum does not know is a row written by something older
        // than this column; treated as unclassified rather than guessed at.
        document_category: m
            .document_category
            .as_deref()
            .and_then(oxidgene_core::enums::DocumentCategory::parse),
        tags: serde_json::from_str(&m.tags).unwrap_or_default(),
        date_value2: m.date_value2,
        calendar: m.calendar.into(),
        place_id: m.place_id,
        created_at: m.created_at,
        updated_at: m.updated_at,
        deleted_at: m.deleted_at,
    }
}

/// Replace the legacy JSON value with the canonical rows from `media_tag`.
async fn hydrate_tags(db: &impl ConnectionTrait, media: &mut [Media]) -> Result<(), OxidGeneError> {
    let media_ids: Vec<Uuid> = media.iter().map(|item| item.id).collect();
    let tag_rows = MediaTagRepo::list_for_media_ids(db, &media_ids).await?;
    let mut tags_by_media: HashMap<Uuid, Vec<String>> = HashMap::new();
    for row in tag_rows {
        tags_by_media.entry(row.media_id).or_default().push(row.tag);
    }
    for item in media {
        item.tags = tags_by_media.remove(&item.id).unwrap_or_default();
    }
    Ok(())
}
