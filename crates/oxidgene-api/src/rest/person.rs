//! REST handlers for Person CRUD operations.

use std::collections::HashMap;

use crate::profile::invalidation;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use oxidgene_core::enums::SpouseRole;
use oxidgene_core::error::OxidGeneError;
use oxidgene_db::repo::{
    AncestryRepo, FamilyChildRepo, FamilyRepo, FamilySpouseRepo, PaginationParams, PersonRepo,
    TreeRepo,
};
use sea_orm::DatabaseConnection;
use uuid::Uuid;

use super::dto::{
    AncestryQuery, CreatePersonRequest, PaginationQuery, PersonDetailResponse, PersonSearchQuery,
    PortraitImagesRequest, UpdatePersonRequest,
};
use super::error::ApiError;
use super::state::{AppState, begin_tx, commit_tx};

/// BFS from `sosa_root` through the ancestry graph to find the SOSA-Stradonitz
/// number of `person_id`. Loads all family data for the tree in two queries.
/// Walks down from the tree's SOSA root to find the person at SOSA number
/// `number` (root = 1, father = 2n, mother = 2n+1). Returns `Ok(None)` if
/// the tree has no SOSA root configured, `number` is 0, or the chain breaks
/// before reaching `number` (a missing parent along the path).
pub(crate) async fn resolve_sosa_number(
    db: &DatabaseConnection,
    tree_id: Uuid,
    number: u64,
) -> Result<Option<oxidgene_core::types::Person>, OxidGeneError> {
    if number == 0 {
        return Ok(None);
    }
    let tree = TreeRepo::get(db, tree_id).await?;
    let Some(root) = tree.sosa_root_person_id else {
        return Ok(None);
    };
    if number == 1 {
        return PersonRepo::get(db, root).await.map(Some);
    }

    let families = FamilyRepo::list_all(db, tree_id).await?;
    if families.is_empty() {
        return Ok(None);
    }
    let family_ids: Vec<Uuid> = families.iter().map(|f| f.id).collect();
    let spouses = FamilySpouseRepo::list_by_families(db, &family_ids).await?;
    let children = FamilyChildRepo::list_by_families(db, &family_ids).await?;

    let child_to_family: HashMap<Uuid, Uuid> = children
        .iter()
        .map(|c| (c.person_id, c.family_id))
        .collect();
    let mut family_parents: HashMap<Uuid, (Option<Uuid>, Option<Uuid>)> = HashMap::new();
    for s in &spouses {
        let e = family_parents.entry(s.family_id).or_default();
        match s.role {
            SpouseRole::Husband => e.0 = Some(s.person_id),
            SpouseRole::Wife => e.1 = Some(s.person_id),
            SpouseRole::Partner => {}
        }
    }

    // Bits of `number` after the leading 1, MSB-first: each one selects the
    // father (0) or mother (1) edge for the next step down from `root`.
    let msb = 63 - number.leading_zeros();
    let mut current = root;
    for i in (0..msb).rev() {
        let bit = (number >> i) & 1;
        let Some(&family_id) = child_to_family.get(&current) else {
            return Ok(None);
        };
        let Some(&(father, mother)) = family_parents.get(&family_id) else {
            return Ok(None);
        };
        current = match (bit, father, mother) {
            (0, Some(f), _) => f,
            (1, _, Some(m)) => m,
            _ => return Ok(None),
        };
    }
    PersonRepo::get(db, current).await.map(Some)
}

/// GET /api/v1/trees/:tree_id/persons
pub async fn list_persons(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<PaginationQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let params = PaginationParams {
        first: query.first.unwrap_or(25),
        after: query.after,
    };
    let connection = PersonRepo::list(&state.db, tree_id, &params)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(connection).unwrap()))
}

/// POST /api/v1/trees/:tree_id/persons
pub async fn create_person(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<CreatePersonRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let id = Uuid::now_v7();
    let txn = begin_tx(&state.db).await.map_err(ApiError)?;
    let person = PersonRepo::create(&txn, id, tree_id, body.sex)
        .await
        .map_err(ApiError::from)?;
    // Build the projection for the new person (not linked to any family yet).
    state
        .profiles
        .rebuild_person(&txn, tree_id, id)
        .await
        .map_err(ApiError)?;
    commit_tx(txn).await.map_err(ApiError)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(person).unwrap()),
    ))
}

/// GET /api/v1/trees/:tree_id/persons/:person_id
pub async fn get_person(
    State(state): State<AppState>,
    Path((tree_id, person_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let person = PersonRepo::get_in_tree(&state.db, tree_id, person_id)
        .await
        .map_err(ApiError::from)?;
    let sosa_number =
        crate::service::person_detail::compute_sosa_number(&state.db, tree_id, person_id)
            .await
            .map_err(ApiError::from)?;
    Ok(Json(
        serde_json::to_value(PersonDetailResponse {
            person,
            sosa_number,
        })
        .unwrap(),
    ))
}

/// PUT /api/v1/trees/:tree_id/persons/:person_id
pub async fn update_person(
    State(state): State<AppState>,
    Path((tree_id, person_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdatePersonRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let txn = begin_tx(&state.db).await.map_err(ApiError)?;
    PersonRepo::get_in_tree(&txn, tree_id, person_id)
        .await
        .map_err(ApiError::from)?;
    let person = PersonRepo::update(&txn, person_id, body.sex, body.privacy)
        .await
        .map_err(ApiError::from)?;
    let affected = invalidation::affected_persons(&txn, person_id)
        .await
        .map_err(ApiError)?;
    state
        .profiles
        .invalidate_for_mutation(&txn, tree_id, &affected)
        .await
        .map_err(ApiError)?;
    commit_tx(txn).await.map_err(ApiError)?;
    Ok(Json(serde_json::to_value(person).unwrap()))
}

/// DELETE /api/v1/trees/:tree_id/persons/:person_id
pub async fn delete_person(
    State(state): State<AppState>,
    Path((tree_id, person_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let txn = begin_tx(&state.db).await.map_err(ApiError)?;
    PersonRepo::get_in_tree(&txn, tree_id, person_id)
        .await
        .map_err(ApiError::from)?;
    PersonRepo::delete(&txn, person_id)
        .await
        .map_err(ApiError::from)?;
    // Drops the person's projection + search row and refreshes the relatives
    // that referenced them.
    state
        .profiles
        .invalidate_for_person_delete(&txn, tree_id, person_id)
        .await
        .map_err(ApiError)?;
    commit_tx(txn).await.map_err(ApiError)?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/trees/:tree_id/persons/:person_id/ancestors
pub async fn get_ancestors(
    State(state): State<AppState>,
    Path((tree_id, person_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<AncestryQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    PersonRepo::get_in_tree(&state.db, tree_id, person_id)
        .await
        .map_err(ApiError::from)?;
    let ancestors = AncestryRepo::ancestors(&state.db, person_id, query.max_depth)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(ancestors).unwrap()))
}

/// GET /api/v1/trees/:tree_id/persons/:person_id/descendants
pub async fn get_descendants(
    State(state): State<AppState>,
    Path((tree_id, person_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<AncestryQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    PersonRepo::get_in_tree(&state.db, tree_id, person_id)
        .await
        .map_err(ApiError::from)?;
    let descendants = AncestryRepo::descendants(&state.db, person_id, query.max_depth)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(descendants).unwrap()))
}

/// GET /api/v1/trees/:tree_id/persons/search?q=...&limit=...&offset=...
///
/// Server-side free-text person search (Sprint E.6): accent-folded
/// multi-word matching against the `person_search_fts` table (SQLite FTS5
/// virtual table / plain PostgreSQL table). Returns a `SearchResult` with
/// display-ready entries and a total count. An empty or missing `q` lists
/// all persons sorted by name (browse mode).
pub async fn search_persons(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<PersonSearchQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let q = query.q.unwrap_or_default();
    let limit = query.limit.unwrap_or(25).min(100);
    let offset = query.offset.unwrap_or(0);
    let filters = oxidgene_db::repo::PersonSearchFilters {
        sex: query.sex,
        surname: query.surname,
        given_names: query.given_names,
        occupation: query.occupation,
        spouse_surname: query.spouse_surname,
        spouse_given_names: query.spouse_given_names,
        father_surname: query.father_surname,
        father_given_names: query.father_given_names,
        mother_surname: query.mother_surname,
        mother_given_names: query.mother_given_names,
        birth_from: query.birth_from,
        birth_to: query.birth_to,
        death_from: query.death_from,
        death_to: query.death_to,
        place: query.place,
        event_type: query.event_type,
        event_from: query.event_from,
        event_to: query.event_to,
        has_media: query.has_media,
    };
    let results = state
        .profiles
        .search_filtered(tree_id, &q, &filters, query.sort.into(), limit, offset)
        .await
        .map_err(ApiError)?;
    Ok(Json(serde_json::to_value(results).unwrap()))
}

/// GET /api/v1/trees/:tree_id/persons/sosa/:number
///
/// Resolves a SOSA-Stradonitz number to a person, walking down from the
/// tree's configured SOSA root. 404 if the tree has no SOSA root configured
/// or no person exists at that number.
pub async fn get_person_by_sosa(
    State(state): State<AppState>,
    Path((tree_id, number)): Path<(Uuid, u64)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let person = resolve_sosa_number(&state.db, tree_id, number)
        .await
        .map_err(ApiError::from)?
        .ok_or(ApiError(OxidGeneError::NotFound {
            entity: "Person (by SOSA number)",
            id: tree_id,
        }))?;
    Ok(Json(
        serde_json::to_value(PersonDetailResponse {
            person,
            sosa_number: Some(number),
        })
        .unwrap(),
    ))
}

/// PUT /api/v1/trees/:tree_id/persons/:person_id/portrait
///
/// Choose what represents a person: a whole media, a region of one — a face in
/// a group photograph — or nothing.
///
/// One write on the person. "At most one portrait" is a property of that row
/// rather than an invariant spanning the media links, so nothing has to be
/// cleared first and no failure between two statements can leave two.
pub async fn set_person_portrait(
    State(state): State<AppState>,
    Path((tree_id, person_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<super::dto::SetPortraitRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let portrait = body
        .portrait()
        .map_err(|e| ApiError(OxidGeneError::Validation(e)))?;
    let person = PersonRepo::set_portrait(&state.db, person_id, portrait)
        .await
        .map_err(ApiError::from)?;

    // The portrait is embedded in `person_denorm`, so the projection has to be
    // rebuilt or the tree keeps drawing the old one.
    state
        .profiles
        .rebuild_person(&state.db, tree_id, person_id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(person).unwrap()))
}

/// GET /api/v1/trees/:tree_id/portraits
///
/// Every person's portrait in one request, as (person, media, vignette).
///
/// A pedigree draws a hundred cards and a profile page draws one avatar, both
/// from the same answer. Before the portrait moved onto the person this was
/// read out of the tree-wide media-link list — every link in the tree shipped
/// so that a few of them could be recognised as portraits.
pub async fn list_portraits(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let rows = PersonRepo::list_portraits(&state.db, tree_id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(rows).unwrap()))
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
) -> Result<Json<Vec<crate::service::portrait::PortraitImage>>, ApiError> {
    let images = crate::service::portrait::load_portrait_images(
        &state.db,
        &state.media,
        tree_id,
        &body.person_ids,
    )
    .await
    .map_err(ApiError::from)?;
    Ok(Json(images))
}
