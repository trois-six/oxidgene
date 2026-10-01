//! Pedigree assembly shared by REST, GraphQL and MCP: one pedigree, a batch
//! of them, and the delta of an expansion, all within one depth limit.
//!
//! A screen that draws one small pedigree per row — the search grid's result
//! cards — needs a pedigree for each of them at once. Asking one at a time puts
//! a request per row on the wire and a traced resource per row in the load
//! trace, which is both slower and unreadable. One bounded operation answers
//! for the whole page instead.

use std::sync::Arc;

use futures_util::{StreamExt as _, stream};
use oxidgene_core::OxidGeneError;
use oxidgene_core::projection::{Pedigree, PedigreeDelta, PedigreeDirection};
use serde::Serialize;
use uuid::Uuid;

use crate::profile::ProfileService;

/// Deepest a pedigree reaches in either direction, in generations: the range
/// the pedigree view offers, and what fits in an assistant's context. Every
/// surface enforces it, so no request can ask for a whole closure.
pub const MAX_PEDIGREE_DEPTH: u32 = 10;

/// A pedigree depth, as a request gave it: within `0..=MAX_PEDIGREE_DEPTH`
/// or a validation error. Taken as `i64` so a negative GraphQL `Int` is
/// refused rather than wrapped into a huge `u32`.
pub fn depth(value: i64) -> Result<u32, OxidGeneError> {
    u32::try_from(value)
        .ok()
        .filter(|depth| *depth <= MAX_PEDIGREE_DEPTH)
        .ok_or_else(|| {
            OxidGeneError::Validation(format!(
                "pedigree depths are between 0 and {MAX_PEDIGREE_DEPTH} generations"
            ))
        })
}

/// The pedigree of `root_person_id` in `tree_id`, `ancestor_depth`
/// generations up and `descendant_depth` down.
pub async fn pedigree(
    profiles: &ProfileService,
    tree_id: Uuid,
    root_person_id: Uuid,
    ancestor_depth: i64,
    descendant_depth: i64,
) -> Result<Pedigree, OxidGeneError> {
    let (ancestor_depth, descendant_depth) = (depth(ancestor_depth)?, depth(descendant_depth)?);
    profiles
        .get_or_build_pedigree(tree_id, root_person_id, ancestor_depth, descendant_depth)
        .await
}

/// A pedigree expansion: in `direction`, from `from_depth` to `to_depth`
/// generations, the opposite direction being loaded to `other_depth`.
pub struct Expansion {
    pub direction: PedigreeDirection,
    pub from_depth: i64,
    pub to_depth: i64,
    pub other_depth: i64,
}

/// The nodes and edges the pedigree of `root_person_id` gains in
/// `expansion`.
pub async fn expand_pedigree(
    profiles: &ProfileService,
    tree_id: Uuid,
    root_person_id: Uuid,
    expansion: Expansion,
) -> Result<PedigreeDelta, OxidGeneError> {
    let from = depth(expansion.from_depth)?;
    let to = depth(expansion.to_depth)?;
    let other = depth(expansion.other_depth)?;
    if to <= from {
        return Err(OxidGeneError::Validation(format!(
            "to_depth ({to}) must be greater than from_depth ({from})"
        )));
    }
    profiles
        .expand_pedigree(
            tree_id,
            root_person_id,
            expansion.direction,
            from,
            to,
            other,
        )
        .await
}

/// Small enough that a page of results always fits, and bounded like every
/// other batch operation so one request can never ask for a whole tree.
pub const MAX_PEDIGREES_PER_REQUEST: usize = 64;

/// How many pedigrees are assembled at once. They each hit the database, so
/// this bounds the connection pressure one request can create.
const ASSEMBLY_CONCURRENCY: usize = 8;

/// One requested pedigree, paired with the root it was asked for.
#[derive(Debug, Clone, Serialize)]
pub struct PedigreeEntry {
    pub root_person_id: Uuid,
    pub pedigree: Pedigree,
}

/// Assemble several pedigrees in one operation.
///
/// Request order is preserved. A root that cannot be assembled — deleted, or
/// belonging to another tree — is omitted rather than failing the batch: a grid
/// of twenty cards should not go blank because one of them is stale.
pub async fn load_pedigrees(
    profiles: &Arc<ProfileService>,
    tree_id: Uuid,
    root_person_ids: &[Uuid],
    ancestor_depth: i64,
    descendant_depth: i64,
) -> Result<Vec<PedigreeEntry>, OxidGeneError> {
    if root_person_ids.len() > MAX_PEDIGREES_PER_REQUEST {
        return Err(OxidGeneError::Validation(format!(
            "at most {MAX_PEDIGREES_PER_REQUEST} pedigrees can be loaded at once"
        )));
    }
    let (ancestor_depth, descendant_depth) = (depth(ancestor_depth)?, depth(descendant_depth)?);
    // The projections are the tree's, not the roots': checked once for the
    // batch rather than once per pedigree.
    profiles.ensure_tree_materialized(tree_id).await?;

    let assembled = stream::iter(root_person_ids.iter().copied().enumerate())
        .map(|(index, root_person_id)| {
            let profiles = Arc::clone(profiles);
            async move {
                let pedigree = profiles
                    .assemble_pedigree(tree_id, root_person_id, ancestor_depth, descendant_depth)
                    .await;
                (index, root_person_id, pedigree)
            }
        })
        .buffer_unordered(ASSEMBLY_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;

    let mut kept = assembled
        .into_iter()
        .filter_map(|(index, root_person_id, pedigree)| match pedigree {
            Ok(pedigree) => Some((
                index,
                PedigreeEntry {
                    root_person_id,
                    pedigree,
                },
            )),
            Err(error) => {
                tracing::warn!(%error, %root_person_id, "pedigree could not be assembled");
                None
            }
        })
        .collect::<Vec<_>>();
    kept.sort_by_key(|(index, _)| *index);
    Ok(kept.into_iter().map(|(_, entry)| entry).collect())
}
