//! Batched pedigree assembly shared by REST and GraphQL.
//!
//! A screen that draws one small pedigree per row — the search grid's result
//! cards — needs a pedigree for each of them at once. Asking one at a time puts
//! a request per row on the wire and a traced resource per row in the load
//! trace, which is both slower and unreadable. One bounded operation answers
//! for the whole page instead.

use std::sync::Arc;

use futures_util::{StreamExt as _, stream};
use oxidgene_core::OxidGeneError;
use oxidgene_core::projection::Pedigree;
use serde::Serialize;
use uuid::Uuid;

use crate::profile::ProfileService;

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
    ancestor_depth: u32,
    descendant_depth: u32,
) -> Result<Vec<PedigreeEntry>, OxidGeneError> {
    if root_person_ids.len() > MAX_PEDIGREES_PER_REQUEST {
        return Err(OxidGeneError::Validation(format!(
            "at most {MAX_PEDIGREES_PER_REQUEST} pedigrees can be loaded at once"
        )));
    }

    let assembled = stream::iter(root_person_ids.iter().copied().enumerate())
        .map(|(index, root_person_id)| {
            let profiles = Arc::clone(profiles);
            async move {
                let pedigree = profiles
                    .get_or_build_pedigree(
                        tree_id,
                        root_person_id,
                        ancestor_depth,
                        descendant_depth,
                    )
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
