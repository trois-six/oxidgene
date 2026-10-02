//! Pedigree assembly shared by REST, GraphQL and MCP: one pedigree, a batch
//! of them, and the delta of an expansion, all within one depth limit.
//!
//! A screen that draws one small pedigree per row — the search grid's result
//! cards — needs a pedigree for each of them at once. Asking one at a time puts
//! a request per row on the wire and a traced resource per row in the load
//! trace, which is both slower and unreadable. One bounded operation answers
//! for the whole page instead.

use std::sync::Arc;

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

/// The person a tree's pedigree is drawn around when nobody was chosen: its
/// SOSA root while that is a live person of the tree, else its first person.
/// A tree without anyone has no pedigree (`not_found`).
pub async fn default_root(
    db: &impl sea_orm::ConnectionTrait,
    tree_id: Uuid,
) -> Result<Uuid, OxidGeneError> {
    use oxidgene_db::repo::{PaginationParams, PersonRepo, TreeRepo};

    if let Some(root) = TreeRepo::get(db, tree_id).await?.sosa_root_person_id {
        match PersonRepo::get_in_tree(db, tree_id, root).await {
            Ok(_) => return Ok(root),
            Err(OxidGeneError::NotFound { .. }) => {}
            Err(error) => return Err(error),
        }
    }
    let first = PaginationParams {
        first: 1,
        after: None,
    };
    PersonRepo::list_filtered(db, tree_id, None, &first)
        .await?
        .edges
        .first()
        .map(|edge| edge.node.id)
        .ok_or(OxidGeneError::NotFound {
            entity: "Person",
            id: tree_id,
        })
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

/// One requested pedigree, paired with the root it was asked for.
#[derive(Debug, Clone, Serialize)]
pub struct PedigreeEntry {
    pub root_person_id: Uuid,
    pub pedigree: Pedigree,
}

/// Assemble several pedigrees in one operation, sharing their reads: the
/// batch costs the statements of one pedigree, whatever its number of roots.
///
/// Request order is preserved, a root asked twice answered twice. A root
/// that is not a live person of the tree — deleted, unknown, or belonging to
/// another tree — is omitted rather than failing the batch: a grid of twenty
/// cards should not go blank because one of them is stale.
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

    let pedigrees = profiles
        .assemble_pedigrees(tree_id, root_person_ids, ancestor_depth, descendant_depth)
        .await?;
    let entries: Vec<PedigreeEntry> = root_person_ids
        .iter()
        .filter_map(|&root_person_id| {
            Some(PedigreeEntry {
                root_person_id,
                pedigree: pedigrees.get(&root_person_id)?.clone(),
            })
        })
        .collect();
    let omitted = root_person_ids.len() - entries.len();
    if omitted > 0 {
        // A person's ID identifies a person: only the count is logged.
        tracing::warn!(
            omitted,
            "pedigree roots that are not live persons of the tree were left out"
        );
    }
    Ok(entries)
}
