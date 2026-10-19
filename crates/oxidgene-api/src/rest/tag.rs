//! REST handlers for the tag vocabulary and for tagging records (docs/api.md, Tags).
//! Each one is a thin adapter over `service::tag`.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use oxidgene_core::types::{Connection, RecordKind};
use oxidgene_db::repo::{PaginationParams, TagUsage};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::error::ApiError;
use super::state::AppState;
use crate::service::tag::{self, TaggedRecord};
use crate::service::tag_bulk::{self, BulkAction, TagTargetSpec, TargetCounts};

/// A tag of the vocabulary with its counts per record kind.
#[derive(Debug, Serialize)]
pub struct TagView {
    pub id: Uuid,
    pub label: String,
    pub color: i16,
    pub counts: TagCounts,
}

/// The live records of each kind carrying a tag.
#[derive(Debug, Serialize)]
pub struct TagCounts {
    pub persons: i64,
    pub families: i64,
    pub events: i64,
    pub places: i64,
    pub sources: i64,
    pub citations: i64,
    pub repositories: i64,
    pub media: i64,
    pub notes: i64,
}

impl From<TagUsage> for TagView {
    fn from(usage: TagUsage) -> Self {
        Self {
            id: usage.tag.id,
            label: usage.tag.label.clone(),
            color: usage.tag.color,
            counts: TagCounts {
                persons: usage.count(RecordKind::Person),
                families: usage.count(RecordKind::Family),
                events: usage.count(RecordKind::Event),
                places: usage.count(RecordKind::Place),
                sources: usage.count(RecordKind::Source),
                citations: usage.count(RecordKind::Citation),
                repositories: usage.count(RecordKind::Repository),
                media: usage.count(RecordKind::Media),
                notes: usage.count(RecordKind::Note),
            },
        }
    }
}

/// Body of `POST /trees/:tree_id/tags`.
#[derive(Debug, Deserialize)]
pub struct CreateTagRequest {
    pub label: String,
    pub color: Option<i16>,
}

/// Body of `PUT /trees/:tree_id/tags/:tag_id`.
#[derive(Debug, Deserialize)]
pub struct UpdateTagRequest {
    pub label: Option<String>,
    pub color: Option<i16>,
}

/// Body of `POST /trees/:tree_id/tags/:tag_id/merge`.
#[derive(Debug, Deserialize)]
pub struct MergeTagRequest {
    pub into: Uuid,
}

/// Query of `GET /trees/:tree_id/tags/:tag_id/records`. The kind is read as
/// text so that an unknown one is `not_found`, not an extractor error.
#[derive(Debug, Deserialize)]
pub struct TaggedRecordsQuery {
    pub kind: String,
    pub first: Option<u64>,
    pub after: Option<String>,
}

/// GET /api/v1/trees/:tree_id/tags
///
/// The whole vocabulary, answered in one response (docs/api.md, Tags).
pub async fn list_tags(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
) -> Result<Json<Vec<TagView>>, ApiError> {
    let tags = tag::list(&state.reader, tree_id).await?;
    Ok(Json(tags.into_iter().map(TagView::from).collect()))
}

/// POST /api/v1/trees/:tree_id/tags
///
/// Creates the tag, or returns the existing one its label folds to (`200`).
pub async fn create_tag(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<CreateTagRequest>,
) -> Result<(StatusCode, Json<TagView>), ApiError> {
    let (usage, created) = tag::create(&state.db, tree_id, &body.label, body.color).await?;
    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(usage.into())))
}

/// PUT /api/v1/trees/:tree_id/tags/:tag_id
pub async fn update_tag(
    State(state): State<AppState>,
    Path((tree_id, tag_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateTagRequest>,
) -> Result<Json<TagView>, ApiError> {
    let usage = tag::update(
        &state.db,
        &state.profiles,
        tree_id,
        tag_id,
        body.label.as_deref(),
        body.color,
    )
    .await?;
    Ok(Json(usage.into()))
}

/// POST /api/v1/trees/:tree_id/tags/:tag_id/merge
///
/// Moves every link of the tag to `into` and deletes it; returns `into`.
pub async fn merge_tag(
    State(state): State<AppState>,
    Path((tree_id, tag_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<MergeTagRequest>,
) -> Result<Json<TagView>, ApiError> {
    let usage = tag::merge(&state.db, &state.profiles, tree_id, tag_id, body.into).await?;
    Ok(Json(usage.into()))
}

/// DELETE /api/v1/trees/:tree_id/tags/:tag_id
pub async fn delete_tag(
    State(state): State<AppState>,
    Path((tree_id, tag_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    tag::delete(&state.db, &state.profiles, tree_id, tag_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/trees/:tree_id/tags/:tag_id/records?kind=...
pub async fn list_tagged_records(
    State(state): State<AppState>,
    Path((tree_id, tag_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<TaggedRecordsQuery>,
) -> Result<Json<Connection<TaggedRecord>>, ApiError> {
    let kind = tag::parse_kind(&query.kind)?;
    let params = PaginationParams {
        first: query.first.unwrap_or(25),
        after: query.after,
    };
    let page = tag::tagged_records(&state.reader, tree_id, tag_id, kind, &params).await?;
    Ok(Json(page))
}

/// PUT /api/v1/trees/:tree_id/tags/:tag_id/records/:kind/:record_id
///
/// Idempotent: `204` whether or not the record already carried the tag.
pub async fn tag_record(
    State(state): State<AppState>,
    Path((tree_id, tag_id, kind, record_id)): Path<(Uuid, Uuid, String, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let kind = tag::parse_kind(&kind)?;
    tag::tag_record(&state.db, &state.profiles, tree_id, tag_id, kind, record_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /api/v1/trees/:tree_id/tags/:tag_id/records/:kind/:record_id
///
/// Idempotent: `204` also when the record did not carry the tag.
pub async fn untag_record(
    State(state): State<AppState>,
    Path((tree_id, tag_id, kind, record_id)): Path<(Uuid, Uuid, String, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let kind = tag::parse_kind(&kind)?;
    tag::untag_record(&state.db, &state.profiles, tree_id, tag_id, kind, record_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Body of `POST /trees/:tree_id/tags/:tag_id/bulk`. The action is read as
/// text so that an unknown one is `validation_error`, not an extractor error.
#[derive(Debug, Deserialize)]
pub struct BulkTagRequest {
    pub action: String,
    pub target: TagTargetSpec,
}

/// Body of `POST /trees/:tree_id/tags/target`.
#[derive(Debug, Deserialize)]
pub struct TagTargetRequest {
    pub target: TagTargetSpec,
}

/// The persons a bulk change changed.
#[derive(Debug, Serialize)]
pub struct BulkTagCount {
    pub count: u64,
}

/// POST /api/v1/trees/:tree_id/tags/:tag_id/bulk
///
/// Adds the tag to, or removes it from, every person of the target, in one
/// transaction and one audit entry (docs/api.md, Tags).
pub async fn bulk_tag(
    State(state): State<AppState>,
    Path((tree_id, tag_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<BulkTagRequest>,
) -> Result<Json<BulkTagCount>, ApiError> {
    let action = body.action.parse::<BulkAction>()?;
    let count = tag_bulk::bulk_tag(
        &state.db,
        &state.profiles,
        tree_id,
        tag_id,
        action,
        &body.target,
    )
    .await?;
    Ok(Json(BulkTagCount { count }))
}

/// POST /api/v1/trees/:tree_id/tags/target
///
/// The size of the target and, per tag, how many of its persons carry it.
/// Writes nothing (docs/api.md, Tags).
pub async fn tag_target(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<TagTargetRequest>,
) -> Result<Json<TargetCounts>, ApiError> {
    let counts = tag_bulk::target_counts(&state.reader, tree_id, &body.target).await?;
    Ok(Json(counts))
}
