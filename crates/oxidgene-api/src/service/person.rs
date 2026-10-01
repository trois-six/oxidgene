//! Person writes and the reads that walk from one: the same steps whether
//! REST or GraphQL asked.
//!
//! Each write checks that the person belongs to the tree, rewrites the
//! projections it touches, and records the change, in one transaction.

use oxidgene_core::OxidGeneError;
use oxidgene_core::enums::{ChildType, Privacy, Sex, SpouseRole};
use oxidgene_core::history::AuditEntity;
use oxidgene_core::types::{AncestryLink, Person};
use oxidgene_db::repo::{
    AncestryRepo, FamilyChildRepo, FamilyRepo, FamilySpouseRepo, MAX_GENERATIONS, PersonRepo,
    TreeRepo,
};
use oxidgene_db::sea_orm::{ConnectionTrait, DatabaseConnection};
use serde::Deserialize;
use uuid::Uuid;

use crate::profile::{ProfileService, invalidation};
use crate::service::history::Change;
use crate::service::scope::{begin_tx, commit_tx};

/// The deepest an ancestor or descendant walk goes, in generations: the
/// traversal's own ceiling, so a corrupt cycle cannot recurse forever.
pub const MAX_ANCESTRY_DEPTH: i32 = MAX_GENERATIONS;

/// A person to create.
#[derive(Debug, Deserialize)]
pub struct NewPerson {
    pub sex: Sex,
}

/// The fields a person update changes; `None` keeps a field.
#[derive(Debug, Default, Deserialize)]
pub struct PersonPatch {
    pub sex: Option<Sex>,
    pub privacy: Option<Privacy>,
}

/// Which way an ancestry walk goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lineage {
    Ancestors,
    Descendants,
}

/// Create a person in `tree_id`, with the projection a new, unlinked person
/// has.
pub async fn create_person(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    new: NewPerson,
) -> Result<Person, OxidGeneError> {
    let id = Uuid::now_v7();
    let txn = begin_tx(db).await?;
    let person = PersonRepo::create(&txn, id, tree_id, new.sex).await?;
    profiles.rebuild_person(&txn, tree_id, id).await?;
    Change::create(tree_id, AuditEntity::Person, id)
        .person(id)
        .record(&txn)
        .await?;
    commit_tx(txn).await?;
    Ok(person)
}

/// Update person `id` of `tree_id`, and the projections of everyone whose
/// own shows them: spouses, children, parents.
pub async fn update_person(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    id: Uuid,
    patch: PersonPatch,
) -> Result<Person, OxidGeneError> {
    let txn = begin_tx(db).await?;
    PersonRepo::get_in_tree(&txn, tree_id, id).await?;
    let person = PersonRepo::update(&txn, id, patch.sex, patch.privacy).await?;
    let affected = invalidation::affected_persons(&txn, id).await?;
    profiles
        .invalidate_for_mutation(&txn, tree_id, &affected)
        .await?;
    Change::update(tree_id, AuditEntity::Person, id)
        .person(id)
        .record(&txn)
        .await?;
    commit_tx(txn).await?;
    Ok(person)
}

/// Delete person `id` of `tree_id` (a soft delete): their projection and
/// search row go, and the relatives that showed them are rewritten.
pub async fn delete_person(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    id: Uuid,
) -> Result<(), OxidGeneError> {
    let txn = begin_tx(db).await?;
    PersonRepo::get_in_tree(&txn, tree_id, id).await?;
    PersonRepo::delete(&txn, id).await?;
    profiles
        .invalidate_for_person_delete(&txn, tree_id, id)
        .await?;
    Change::delete(tree_id, AuditEntity::Person, id)
        .person(id)
        .record(&txn)
        .await?;
    commit_tx(txn).await
}

/// The ancestors or descendants of person `person_id` of `tree_id`, each at
/// its shortest distance, down to `max_depth` generations.
///
/// `max_depth` defaults to, and may not exceed, [`MAX_ANCESTRY_DEPTH`]; it
/// must be positive.
pub async fn lineage(
    db: &DatabaseConnection,
    tree_id: Uuid,
    person_id: Uuid,
    lineage: Lineage,
    max_depth: Option<i32>,
) -> Result<Vec<AncestryLink>, OxidGeneError> {
    let max_depth = max_depth.unwrap_or(MAX_ANCESTRY_DEPTH);
    if !(1..=MAX_ANCESTRY_DEPTH).contains(&max_depth) {
        return Err(OxidGeneError::Validation(format!(
            "max_depth must be between 1 and {MAX_ANCESTRY_DEPTH}"
        )));
    }
    PersonRepo::get_in_tree(db, tree_id, person_id).await?;
    match lineage {
        Lineage::Ancestors => AncestryRepo::ancestors(db, person_id, Some(max_depth)).await,
        Lineage::Descendants => AncestryRepo::descendants(db, person_id, Some(max_depth)).await,
    }
}

/// The person at SOSA number `number` of tree `tree_id`, walking down from its
/// SOSA root (root = 1, father = 2n, mother = 2n + 1).
///
/// `None` when the tree has no SOSA root, `number` is 0, or the chain breaks
/// before reaching it. Every person read is scoped to the tree, so a root
/// left over from another tree is no root at all.
pub async fn person_by_sosa(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    number: u64,
) -> Result<Option<Person>, OxidGeneError> {
    if number == 0 {
        return Ok(None);
    }
    let Some(root) = TreeRepo::get(db, tree_id).await?.sosa_root_person_id else {
        return Ok(None);
    };
    // Bits of `number` after the leading 1, MSB-first: each one selects the
    // father (0) or mother (1) edge for the next step down from `root`.
    //
    // One step is two small reads — the families the current person is a
    // child of, and that family's spouses — so a lookup costs a few dozen
    // indexed queries at most instead of loading the tree's whole family
    // structure.
    let msb = 63 - number.leading_zeros();
    let mut current = root;
    for i in (0..msb).rev() {
        let mother = (number >> i) & 1 == 1;
        match parent(db, current, mother).await? {
            Some(next) => current = next,
            None => return Ok(None),
        }
    }
    match PersonRepo::get_in_tree(db, tree_id, current).await {
        Ok(person) => Ok(Some(person)),
        Err(OxidGeneError::NotFound { .. }) => Ok(None),
        Err(error) => Err(error),
    }
}

/// The father, or the mother, of `person_id` in their birth family — or in
/// whichever family they belong to when they have no other.
async fn parent(
    db: &impl ConnectionTrait,
    person_id: Uuid,
    mother: bool,
) -> Result<Option<Uuid>, OxidGeneError> {
    let memberships = FamilyChildRepo::list_by_person(db, person_id).await?;
    let candidates: Vec<Uuid> = memberships.iter().map(|c| c.family_id).collect();
    let live = FamilyRepo::live_ids(db, &candidates).await?;
    let Some(family_id) = memberships
        .iter()
        .filter(|c| live.contains(&c.family_id))
        .min_by_key(|c| c.child_type != ChildType::Biological)
        .map(|c| c.family_id)
    else {
        return Ok(None);
    };
    let wanted = if mother {
        SpouseRole::Wife
    } else {
        SpouseRole::Husband
    };
    Ok(FamilySpouseRepo::list_by_families(db, &[family_id])
        .await?
        .into_iter()
        .rev()
        .find(|spouse| spouse.role == wanted)
        .map(|spouse| spouse.person_id))
}
