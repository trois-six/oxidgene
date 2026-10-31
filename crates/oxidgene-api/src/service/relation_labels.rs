//! Bounded labels for person and family media relations.

use std::collections::HashSet;

use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::{FamilySpouse, PersonName};
use oxidgene_db::repo::{FamilySpouseRepo, PersonNameRepo};
use sea_orm::ConnectionTrait;
use serde::Serialize;
use uuid::Uuid;

pub const MAX_RELATIONS_PER_REQUEST: usize = 1_024;

#[derive(Debug, Clone, Serialize)]
pub struct RelationLabels {
    pub names: Vec<PersonName>,
    pub spouses: Vec<FamilySpouse>,
}

/// Load only the names and spouse links needed to label media relations.
pub async fn load_relation_labels(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    person_ids: &[Uuid],
    family_ids: &[Uuid],
) -> Result<RelationLabels, OxidGeneError> {
    if person_ids.len() + family_ids.len() > MAX_RELATIONS_PER_REQUEST {
        return Err(OxidGeneError::Validation(format!(
            "at most {MAX_RELATIONS_PER_REQUEST} relations can be loaded at once"
        )));
    }

    load_labels(db, tree_id, person_ids, family_ids).await
}

/// [`load_relation_labels`] without its bound on the ids, for a read model
/// whose ids come from the database rather than from a request: the reads go
/// by bounded slices of ids all the same.
pub(crate) async fn load_labels(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    person_ids: &[Uuid],
    family_ids: &[Uuid],
) -> Result<RelationLabels, OxidGeneError> {
    let spouses = FamilySpouseRepo::list_by_families_in_tree(db, tree_id, family_ids).await?;
    let mut all_person_ids = person_ids.iter().copied().collect::<HashSet<_>>();
    all_person_ids.extend(spouses.iter().map(|spouse| spouse.person_id));
    let all_person_ids = all_person_ids.into_iter().collect::<Vec<_>>();
    let names = PersonNameRepo::list_by_persons_in_tree(db, tree_id, &all_person_ids).await?;

    Ok(RelationLabels { names, spouses })
}
