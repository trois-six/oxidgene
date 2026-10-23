//! The tree's media library — the filtered media listing and the values its
//! filters offer — shared by REST and GraphQL, so both surfaces validate and
//! answer identically.

use chrono::NaiveDate;
use oxidgene_core::OxidGeneError;
use oxidgene_core::collections::sorted_unique;
use oxidgene_core::enums::{DocumentCategory, MediaFileKind};
use oxidgene_core::types::{Connection, Edge, Media, TagRef};
use oxidgene_db::repo::{MediaFilter, MediaLibraryRepo, PaginationParams};
use sea_orm::ConnectionTrait;
use serde::Serialize;
use uuid::Uuid;

/// The media list's filters as a client sends them. Blank text is no filter.
#[derive(Debug, Clone, Default)]
pub struct MediaListFilters {
    /// Tags by id: a document must carry all of them.
    pub tags: Vec<Uuid>,
    pub kind: Option<MediaFileKind>,
    pub category: Option<DocumentCategory>,
    pub name: Option<String>,
    pub linked_name: Option<String>,
    pub event_from: Option<i32>,
    pub event_to: Option<i32>,
    pub added_from: Option<NaiveDate>,
    pub added_to: Option<NaiveDate>,
}

impl MediaListFilters {
    /// Check the filters and turn them into the repository's.
    ///
    /// # Errors
    ///
    /// `Validation` when a range ends before it starts.
    pub fn into_filter(self) -> Result<MediaFilter, OxidGeneError> {
        if let (Some(from), Some(to)) = (self.event_from, self.event_to)
            && from > to
        {
            return Err(OxidGeneError::Validation(
                "event_from must not be after event_to".into(),
            ));
        }
        if let (Some(from), Some(to)) = (self.added_from, self.added_to)
            && from > to
        {
            return Err(OxidGeneError::Validation(
                "added_from must not be after added_to".into(),
            ));
        }
        let text = |value: Option<String>| {
            value
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        };
        Ok(MediaFilter {
            tags: sorted_unique(self.tags),
            kind: self.kind,
            category: self.category,
            name: text(self.name),
            linked_name: text(self.linked_name),
            event_from: self.event_from,
            event_to: self.event_to,
            added_from: self.added_from,
            added_to: self.added_to,
        })
    }
}

/// A listed document and how many records — persons, families, events and
/// sources — it is attached to.
#[derive(Debug, Clone, Serialize)]
pub struct MediaListItem {
    #[serde(flatten)]
    pub media: Media,
    pub usage_count: i64,
}

/// A page of the tree's documents matching `filters`, each with its usage
/// count, starting after the cursor of `params` or `offset` documents in.
/// Counting the page's usage is one grouped query, whatever its size.
pub async fn list(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    filters: MediaListFilters,
    params: &PaginationParams,
    offset: Option<u64>,
) -> Result<Connection<MediaListItem>, OxidGeneError> {
    let filter = filters.into_filter()?;
    let connection = MediaLibraryRepo::list(db, tree_id, &filter, params, offset).await?;
    let ids: Vec<Uuid> = connection.edges.iter().map(|edge| edge.node.id).collect();
    let usage = MediaLibraryRepo::usage_counts(db, &ids).await?;
    Ok(Connection {
        edges: connection
            .edges
            .into_iter()
            .map(|edge| Edge {
                cursor: edge.cursor,
                node: MediaListItem {
                    usage_count: usage.get(&edge.node.id).copied().unwrap_or(0),
                    media: edge.node,
                },
            })
            .collect(),
        page_info: connection.page_info,
        total_count: connection.total_count,
    })
}

/// A tag and how many documents carry it.
#[derive(Debug, Clone, Serialize)]
pub struct MediaTagFacet {
    #[serde(flatten)]
    pub tag: TagRef,
    pub count: i64,
}

/// A file kind and how many documents hold a page of it.
#[derive(Debug, Clone, Serialize)]
pub struct MediaKindFacet {
    pub kind: MediaFileKind,
    pub count: i64,
}

/// A document category and how many documents are filed under it.
#[derive(Debug, Clone, Serialize)]
pub struct MediaCategoryFacet {
    pub category: DocumentCategory,
    pub count: i64,
}

/// The values the media list's filters can take in a tree.
#[derive(Debug, Clone, Serialize)]
pub struct MediaFacets {
    pub tags: Vec<MediaTagFacet>,
    pub kinds: Vec<MediaKindFacet>,
    pub categories: Vec<MediaCategoryFacet>,
}

/// The tree's tags, file kinds and categories, each with its document count.
/// The kinds and categories count the whole library; the tags count the
/// documents carrying every tag of `with_tags`, so a selection of tags only
/// offers the tags that still narrow it. Never narrowed by the other filters.
pub async fn facets(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    with_tags: Vec<Uuid>,
) -> Result<MediaFacets, OxidGeneError> {
    let facets = MediaLibraryRepo::facets(db, tree_id, &sorted_unique(with_tags)).await?;
    Ok(MediaFacets {
        tags: facets
            .tags
            .into_iter()
            .map(|tag| MediaTagFacet {
                tag: tag.tag,
                count: tag.count,
            })
            .collect(),
        kinds: facets
            .kinds
            .into_iter()
            .map(|(kind, count)| MediaKindFacet { kind, count })
            .collect(),
        categories: facets
            .categories
            .into_iter()
            .map(|(category, count)| MediaCategoryFacet { category, count })
            .collect(),
    })
}
