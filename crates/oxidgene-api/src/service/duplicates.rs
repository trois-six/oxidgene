//! Same-named persons: recording that two records are different people, and
//! merging two records that turn out to be one.
//!
//! Finding the candidates is a read of the search projection
//! ([`ProfileService::homonyms`]). What lives here are the two answers a user
//! can give about them, shared by REST and GraphQL.

use std::collections::HashSet;

use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::Person;
use oxidgene_db::repo::{
    AncestryRepo, FamilySpouseRepo, PersonDistinctRepo, PersonMergeRepo, PersonRepo,
};
use sea_orm::ConnectionTrait;
use uuid::Uuid;

use crate::profile::invalidation;
use crate::profile::service::{ProfileService, SEARCH_MAX_LIMIT};

/// Record that `person_id` is a different person from each of `others`.
///
/// Every person must belong to `tree_id`. Recording a pair twice is a no-op,
/// so answering the same question again changes nothing.
///
/// # Errors
///
/// `NotFound` if any person is missing from the tree; `Validation` if
/// `person_id` is among `others`, or `others` is empty or longer than
/// [`SEARCH_MAX_LIMIT`] — the most homonyms a single answer can concern.
pub async fn mark_distinct(
    conn: &impl ConnectionTrait,
    tree_id: Uuid,
    person_id: Uuid,
    others: &[Uuid],
) -> Result<(), OxidGeneError> {
    if others.is_empty() || others.len() > SEARCH_MAX_LIMIT {
        return Err(OxidGeneError::Validation(format!(
            "between 1 and {SEARCH_MAX_LIMIT} other persons are required"
        )));
    }
    if others.contains(&person_id) {
        return Err(OxidGeneError::Validation(
            "a person cannot be distinct from themselves".to_string(),
        ));
    }
    PersonRepo::get_in_tree(conn, tree_id, person_id).await?;
    for other in others {
        PersonRepo::get_in_tree(conn, tree_id, *other).await?;
    }
    PersonDistinctRepo::mark(conn, tree_id, person_id, others).await
}

/// Merge `duplicate` into `kept`: one individual recorded twice becomes one
/// record, the kept one, and the duplicate is soft-deleted.
///
/// Everything the duplicate carried moves to the kept person — see
/// [`PersonMergeRepo::absorb`] for what is re-pointed and what is dropped as
/// already present. Projections of both people's relatives are rebuilt in the
/// same transaction, and the duplicate's are removed.
///
/// # Errors
///
/// `NotFound` if either person is missing from the tree. `Validation` if they
/// are the same person, spouses of the same union, or one is an ancestor of
/// the other: each of those would leave a person married to, or descended
/// from, themselves.
pub async fn merge_persons(
    conn: &impl ConnectionTrait,
    profiles: &ProfileService,
    tree_id: Uuid,
    kept: Uuid,
    duplicate: Uuid,
) -> Result<Person, OxidGeneError> {
    if kept == duplicate {
        return Err(OxidGeneError::Validation(
            "a person cannot be merged with themselves".to_string(),
        ));
    }
    PersonRepo::get_in_tree(conn, tree_id, kept).await?;
    PersonRepo::get_in_tree(conn, tree_id, duplicate).await?;

    let (kept_unions, duplicate_unions) = tokio::try_join!(
        FamilySpouseRepo::list_by_person(conn, kept),
        FamilySpouseRepo::list_by_person(conn, duplicate),
    )?;
    let kept_unions: HashSet<Uuid> = kept_unions.iter().map(|link| link.family_id).collect();
    if duplicate_unions
        .iter()
        .any(|link| kept_unions.contains(&link.family_id))
    {
        return Err(OxidGeneError::Validation(
            "spouses of the same union cannot be merged".to_string(),
        ));
    }

    let (ancestors, descendants) = tokio::try_join!(
        AncestryRepo::ancestors(conn, duplicate, None),
        AncestryRepo::descendants(conn, duplicate, None),
    )?;
    if ancestors
        .iter()
        .chain(descendants.iter())
        .any(|link| link.person_id == kept)
    {
        return Err(OxidGeneError::Validation(
            "a person cannot be merged with their own ancestor or descendant".to_string(),
        ));
    }

    // Both relative sets are read while the family links still say who the
    // duplicate's relatives are.
    let (kept_affected, duplicate_affected) = tokio::try_join!(
        invalidation::affected_persons(conn, kept),
        invalidation::affected_persons(conn, duplicate),
    )?;

    PersonMergeRepo::absorb(conn, tree_id, kept, duplicate).await?;
    PersonRepo::delete(conn, duplicate).await?;
    profiles
        .invalidate_for_person_delete(conn, tree_id, duplicate)
        .await?;

    let mut affected: Vec<Uuid> = kept_affected
        .into_iter()
        .chain(duplicate_affected)
        .filter(|id| *id != duplicate)
        .collect();
    affected.sort();
    affected.dedup();
    profiles
        .invalidate_for_mutation(conn, tree_id, &affected)
        .await?;

    PersonRepo::get(conn, kept).await
}
