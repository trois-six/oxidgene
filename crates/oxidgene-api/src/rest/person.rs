//! REST handlers for Person CRUD operations.

use std::collections::{HashMap, HashSet, VecDeque};

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use oxidgene_cache::invalidation;
use oxidgene_core::enums::SpouseRole;
use oxidgene_core::error::OxidGeneError;
use oxidgene_db::repo::{
    FamilyChildRepo, FamilyRepo, FamilySpouseRepo, PaginationParams, PersonAncestryRepo,
    PersonRepo, TreeRepo,
};
use sea_orm::DatabaseConnection;
use uuid::Uuid;

use super::dto::{
    AncestryQuery, CreatePersonRequest, PaginationQuery, PersonDetailResponse, PersonSearchQuery,
    UpdatePersonRequest,
};
use super::error::ApiError;
use super::state::AppState;

/// BFS from `sosa_root` through the ancestry graph to find the SOSA-Stradonitz
/// number of `person_id`. Loads all family data for the tree in two queries.
async fn compute_sosa_number(
    db: &DatabaseConnection,
    tree_id: Uuid,
    person_id: Uuid,
) -> Result<Option<u64>, OxidGeneError> {
    let tree = TreeRepo::get(db, tree_id).await?;
    let root = match tree.sosa_root_person_id {
        Some(r) => r,
        None => return Ok(None),
    };
    if person_id == root {
        return Ok(Some(1));
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

    let mut queue: VecDeque<(Uuid, u64)> = VecDeque::new();
    queue.push_back((root, 1));
    let mut visited: HashSet<Uuid> = HashSet::new();
    while let Some((current, sosa)) = queue.pop_front() {
        if !visited.insert(current) {
            continue;
        }
        if let Some(&family_id) = child_to_family.get(&current)
            && let Some(&(father, mother)) = family_parents.get(&family_id)
        {
            if let Some(fid) = father {
                if fid == person_id {
                    return Ok(Some(sosa * 2));
                }
                queue.push_back((fid, sosa * 2));
            }
            if let Some(mid) = mother {
                if mid == person_id {
                    return Ok(Some(sosa * 2 + 1));
                }
                queue.push_back((mid, sosa * 2 + 1));
            }
        }
    }
    Ok(None)
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
    let person = PersonRepo::create(&state.db, id, tree_id, body.sex)
        .await
        .map_err(ApiError::from)?;
    // Build cache for the new person (not linked to any family yet).
    state
        .cache
        .rebuild_person(tree_id, id)
        .await
        .map_err(ApiError)?;
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
    let person = PersonRepo::get(&state.db, person_id)
        .await
        .map_err(ApiError::from)?;
    let sosa_number = compute_sosa_number(&state.db, tree_id, person_id)
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
    let person = PersonRepo::update(&state.db, person_id, body.sex, body.privacy)
        .await
        .map_err(ApiError::from)?;
    let affected = invalidation::affected_persons(&state.db, person_id)
        .await
        .map_err(ApiError)?;
    state
        .cache
        .invalidate_for_mutation(tree_id, &affected)
        .await
        .map_err(ApiError)?;
    Ok(Json(serde_json::to_value(person).unwrap()))
}

/// DELETE /api/v1/trees/:tree_id/persons/:person_id
pub async fn delete_person(
    State(state): State<AppState>,
    Path((tree_id, person_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    PersonRepo::delete(&state.db, person_id)
        .await
        .map_err(ApiError::from)?;
    // Removes the person from cache + search table, rebuilds affected
    // relatives, and drops pedigrees.
    state
        .cache
        .invalidate_for_person_delete(tree_id, person_id)
        .await
        .map_err(ApiError)?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/trees/:tree_id/persons/:person_id/ancestors
pub async fn get_ancestors(
    State(state): State<AppState>,
    Path((_tree_id, person_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<AncestryQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let ancestors = PersonAncestryRepo::ancestors(&state.db, person_id, query.max_depth)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(ancestors).unwrap()))
}

/// GET /api/v1/trees/:tree_id/persons/:person_id/descendants
pub async fn get_descendants(
    State(state): State<AppState>,
    Path((_tree_id, person_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<AncestryQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let descendants = PersonAncestryRepo::descendants(&state.db, person_id, query.max_depth)
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
    let results = state
        .cache
        .search(tree_id, &q, limit, offset)
        .await
        .map_err(ApiError)?;
    Ok(Json(serde_json::to_value(results).unwrap()))
}
