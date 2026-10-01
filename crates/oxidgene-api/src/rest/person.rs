//! REST handlers for Person CRUD operations.

use crate::profile::service::SEARCH_DEFAULT_LIMIT;
use crate::service::duplicates;
use crate::service::history;
use crate::service::kinship;
use crate::service::person::{self, Lineage, NewPerson, PersonPatch};
use crate::service::portrait::{self, PortraitChoice, PortraitImage};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::projection::{SearchEntry, SearchResult};
use oxidgene_core::types::{AncestryLink, Connection, Kinship, Person};
use oxidgene_db::repo::{PaginationParams, PersonRepo, PersonSearchFilters, PortraitRow};
use uuid::Uuid;

use super::dto::{
    AncestryQuery, MarkPersonsDistinctRequest, MergePersonRequest, PersonDetailResponse,
    PersonListQuery, PersonSearchQuery, PortraitImagesRequest, RecentlyModifiedQuery,
};
use super::error::ApiError;
use super::state::AppState;
use crate::service::scope::{begin_tx, commit_tx};

/// GET /api/v1/trees/:tree_id/persons
///
/// `search` keeps the persons with a name — given names, surname or
/// nickname — containing it.
pub async fn list_persons(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<PersonListQuery>,
) -> Result<Json<Connection<Person>>, ApiError> {
    let params = PaginationParams {
        first: query.first.unwrap_or(25),
        after: query.after,
    };
    let persons =
        PersonRepo::list_filtered(&state.reader, tree_id, query.search.as_deref(), &params).await?;
    Ok(Json(persons))
}

/// POST /api/v1/trees/:tree_id/persons
pub async fn create_person(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<NewPerson>,
) -> Result<(StatusCode, Json<Person>), ApiError> {
    let person = person::create_person(&state.db, &state.profiles, tree_id, body).await?;
    Ok((StatusCode::CREATED, Json(person)))
}

/// GET /api/v1/trees/:tree_id/persons/:person_id
pub async fn get_person(
    State(state): State<AppState>,
    Path((tree_id, person_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<PersonDetailResponse>, ApiError> {
    let person = PersonRepo::get_in_tree(&state.reader, tree_id, person_id).await?;
    let sosa_number =
        crate::service::person_detail::compute_sosa_number(&state.reader, tree_id, person_id)
            .await?;
    Ok(Json(PersonDetailResponse {
        person,
        sosa_number,
    }))
}

/// PUT /api/v1/trees/:tree_id/persons/:person_id
pub async fn update_person(
    State(state): State<AppState>,
    Path((tree_id, person_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<PersonPatch>,
) -> Result<Json<Person>, ApiError> {
    let person =
        person::update_person(&state.db, &state.profiles, tree_id, person_id, body).await?;
    Ok(Json(person))
}

/// DELETE /api/v1/trees/:tree_id/persons/:person_id
pub async fn delete_person(
    State(state): State<AppState>,
    Path((tree_id, person_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    person::delete_person(&state.db, &state.profiles, tree_id, person_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/trees/:tree_id/persons/:person_id/homonyms
///
/// The other persons of the tree bearing the same folded primary surname and
/// given names, as search entries, less those already confirmed to be
/// somebody else.
pub async fn list_homonyms(
    State(state): State<AppState>,
    Path((tree_id, person_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<SearchEntry>>, ApiError> {
    PersonRepo::get_in_tree(&state.reader, tree_id, person_id).await?;
    Ok(Json(state.profiles.homonyms(tree_id, person_id).await?))
}

/// POST /api/v1/trees/:tree_id/persons/:person_id/distinct
///
/// Record that the person differs from each of `person_ids`, so those pairs
/// stop being offered as homonyms.
pub async fn mark_persons_distinct(
    State(state): State<AppState>,
    Path((tree_id, person_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<MarkPersonsDistinctRequest>,
) -> Result<StatusCode, ApiError> {
    let txn = begin_tx(&state.db).await?;
    duplicates::mark_distinct(&txn, tree_id, person_id, &body.person_ids).await?;
    commit_tx(txn).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/v1/trees/:tree_id/persons/:person_id/merge
///
/// Merge `duplicate_id` into the path's person, which is kept; the duplicate
/// is soft-deleted. Returns the kept person.
pub async fn merge_persons(
    State(state): State<AppState>,
    Path((tree_id, person_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<MergePersonRequest>,
) -> Result<Json<Person>, ApiError> {
    let txn = begin_tx(&state.db).await?;
    let person = duplicates::merge_persons(
        &txn,
        &state.profiles,
        tree_id,
        person_id,
        body.duplicate_id,
        &body.choices.into(),
    )
    .await?;
    commit_tx(txn).await?;
    Ok(Json(person))
}

/// GET /api/v1/trees/:tree_id/persons/:person_id/ancestors
pub async fn get_ancestors(
    State(state): State<AppState>,
    Path((tree_id, person_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<AncestryQuery>,
) -> Result<Json<Vec<AncestryLink>>, ApiError> {
    let ancestors = person::lineage(
        &state.reader,
        tree_id,
        person_id,
        Lineage::Ancestors,
        query.max_depth,
    )
    .await?;
    Ok(Json(ancestors))
}

/// GET /api/v1/trees/:tree_id/persons/:person_id/descendants
pub async fn get_descendants(
    State(state): State<AppState>,
    Path((tree_id, person_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<AncestryQuery>,
) -> Result<Json<Vec<AncestryLink>>, ApiError> {
    let descendants = person::lineage(
        &state.reader,
        tree_id,
        person_id,
        Lineage::Descendants,
        query.max_depth,
    )
    .await?;
    Ok(Json(descendants))
}

/// GET /api/v1/trees/:tree_id/persons/:person_id/kinship/:other_person_id
///
/// Every way found to go from the person to the other one: their blood
/// relationships, or the shortest paths through unions when they share no
/// ancestor.
pub async fn get_kinship(
    State(state): State<AppState>,
    Path((tree_id, person_id, other_person_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<Kinship>, ApiError> {
    let kinship = kinship::find_kinship(
        &state.reader,
        &state.profiles,
        tree_id,
        person_id,
        other_person_id,
    )
    .await?;
    Ok(Json(kinship))
}

/// GET /api/v1/trees/:tree_id/persons/search?q=...&limit=...&offset=...
///
/// Server-side free-text person search: accent-folded multi-word matching
/// against the `person_search_fts` table. Returns a `SearchResult` with
/// display-ready entries and a total count. An empty or missing `q` lists
/// all persons sorted by name (browse mode).
pub async fn search_persons(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<PersonSearchQuery>,
    Query(filters): Query<PersonSearchFilters>,
) -> Result<Json<SearchResult>, ApiError> {
    let results = state
        .profiles
        .search_filtered(
            tree_id,
            query.q.as_deref().unwrap_or_default(),
            &filters,
            query.sort,
            query.limit.unwrap_or(SEARCH_DEFAULT_LIMIT),
            query.offset.unwrap_or(0),
        )
        .await?;
    Ok(Json(results))
}

/// GET /api/v1/trees/:tree_id/persons/recently-modified
///
/// The persons modified most recently, newest first, as search entries.
pub async fn list_recently_modified(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<RecentlyModifiedQuery>,
) -> Result<Json<Vec<SearchEntry>>, ApiError> {
    let persons = history::recently_modified_persons(
        &state.reader,
        &state.profiles,
        tree_id,
        query.limit.unwrap_or(history::RECENT_PERSONS_DEFAULT_LIMIT),
    )
    .await?;
    Ok(Json(persons))
}

/// GET /api/v1/trees/:tree_id/persons/sosa/:number
///
/// Resolves a SOSA-Stradonitz number to a person, walking down from the
/// tree's configured SOSA root. 404 if the tree has no SOSA root configured
/// or no person exists at that number.
pub async fn get_person_by_sosa(
    State(state): State<AppState>,
    Path((tree_id, number)): Path<(Uuid, u64)>,
) -> Result<Json<PersonDetailResponse>, ApiError> {
    let person = person::person_by_sosa(&state.reader, tree_id, number)
        .await?
        .ok_or(ApiError(OxidGeneError::NotFound {
            entity: "Person (by SOSA number)",
            id: tree_id,
        }))?;
    Ok(Json(PersonDetailResponse {
        person,
        sosa_number: Some(number),
    }))
}

/// PUT /api/v1/trees/:tree_id/persons/:person_id/portrait
///
/// Choose what represents a person: a whole media, a region of one — a face in
/// a group photograph — or nothing.
pub async fn set_person_portrait(
    State(state): State<AppState>,
    Path((tree_id, person_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<PortraitChoice>,
) -> Result<Json<Person>, ApiError> {
    let person =
        portrait::set_person_portrait(&state.db, &state.profiles, tree_id, person_id, body).await?;
    Ok(Json(person))
}

/// GET /api/v1/trees/:tree_id/portraits
///
/// Every person's portrait in one request, as (person, media, vignette): a
/// pedigree draws a hundred cards and a profile page one avatar, both from
/// the same answer.
pub async fn list_portraits(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
) -> Result<Json<Vec<PortraitRow>>, ApiError> {
    Ok(Json(
        PersonRepo::list_portraits(&state.reader, tree_id).await?,
    ))
}

/// POST /api/v1/trees/:tree_id/portrait-images
///
/// Resolve a bounded set of portraits and return sources ready for image
/// elements. Locally-held images are embedded as data URLs, while remote
/// portraits retain their original URL.
pub async fn load_portrait_images(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<PortraitImagesRequest>,
) -> Result<Json<Vec<PortraitImage>>, ApiError> {
    let images = portrait::load_portrait_images(&state.reader, tree_id, &body.person_ids).await?;
    Ok(Json(images))
}
