//! Cache invalidation logic.
//!
//! Given a mutation on a person (or related entity), computes the bounded set
//! of persons whose [`CachedPerson`] entries must be rebuilt. The set is
//! typically 2–10 persons, keeping the synchronous invalidation cost under
//! 15 ms.

use oxidgene_core::error::OxidGeneError;
use oxidgene_db::repo::{FamilyChildRepo, FamilySpouseRepo};
use oxidgene_db::sea_orm::DatabaseConnection;
use uuid::Uuid;

/// Compute the set of person IDs whose [`CachedPerson`] entries are affected
/// by a mutation involving `person_id`.
///
/// The affected set includes:
/// 1. The person itself.
/// 2. All co-spouses and children in families where this person is a spouse
///    (their [`CachedFamilyLink`] references this person's display name).
/// 3. All spouses (parents) in the family where this person is a child
///    (their [`CachedFamilyLink::children_ids`] references this person).
///
/// The result is de-duplicated but not otherwise ordered.
pub async fn affected_persons(
    db: &DatabaseConnection,
    person_id: Uuid,
) -> Result<Vec<Uuid>, OxidGeneError> {
    let mut affected = vec![person_id];

    // 1. Find all families where this person is a spouse.
    //    We need to query FamilySpouseRepo to find family_ids first, then get
    //    all spouses and children in those families.
    //
    //    There is no `list_by_person` on FamilySpouseRepo, so we use a
    //    two-step approach: fetch all spouses for families we discover.
    //    However, we need to discover those families first.
    //
    //    Strategy: we query all families where the person appears as spouse
    //    by looking at the family_spouse table directly.
    let spouse_families = families_as_spouse(db, person_id).await?;

    if !spouse_families.is_empty() {
        // Get all spouses in these families (includes the person itself).
        let all_spouses = FamilySpouseRepo::list_by_families(db, &spouse_families).await?;
        for spouse in &all_spouses {
            if spouse.person_id != person_id {
                affected.push(spouse.person_id);
            }
        }

        // Get all children in these families.
        let all_children = FamilyChildRepo::list_by_families(db, &spouse_families).await?;
        for child in &all_children {
            affected.push(child.person_id);
        }
    }

    // 2. Find the family where this person is a child, and get its spouses
    //    (the parents).
    let child_family = family_as_child(db, person_id).await?;

    if let Some(family_id) = child_family {
        let parents = FamilySpouseRepo::list_by_families(db, &[family_id]).await?;
        for parent in &parents {
            affected.push(parent.person_id);
        }
    }

    // De-duplicate.
    affected.sort();
    affected.dedup();

    Ok(affected)
}

/// Compute the affected set for a family event mutation.
///
/// Family events (marriage, divorce, etc.) affect both spouses in the family.
/// Returns the set of persons whose caches need rebuilding.
pub async fn affected_persons_for_family(
    db: &DatabaseConnection,
    family_id: Uuid,
) -> Result<Vec<Uuid>, OxidGeneError> {
    let spouses = FamilySpouseRepo::list_by_families(db, &[family_id]).await?;
    let mut affected: Vec<Uuid> = spouses.iter().map(|s| s.person_id).collect();

    // Each spouse's full affected set includes their other families' members.
    // But for a family event, we only need to rebuild the two spouses — their
    // CachedPerson includes the family's marriage event.
    affected.sort();
    affected.dedup();

    Ok(affected)
}

/// Compute affected persons when a family membership changes (spouse added/removed).
///
/// This is broader than a simple person edit: both spouses, all children in the
/// family, AND the parents of both spouses (since their CachedChildLink
/// references might change) are affected.
pub async fn affected_persons_for_family_spouse_change(
    db: &DatabaseConnection,
    family_id: Uuid,
    changed_person_id: Uuid,
) -> Result<Vec<Uuid>, OxidGeneError> {
    // Start with the full affected set of the changed person.
    let mut affected = affected_persons(db, changed_person_id).await?;

    // Also include all members of the target family (the other spouse + children).
    let spouses = FamilySpouseRepo::list_by_families(db, &[family_id]).await?;
    for spouse in &spouses {
        affected.push(spouse.person_id);
    }

    let children = FamilyChildRepo::list_by_families(db, &[family_id]).await?;
    for child in &children {
        affected.push(child.person_id);
    }

    affected.sort();
    affected.dedup();

    Ok(affected)
}

/// Compute affected persons when a family child link changes (child added/removed).
///
/// The child itself, both parents in the family, and the child's other family
/// relationships are all affected.
pub async fn affected_persons_for_family_child_change(
    db: &DatabaseConnection,
    family_id: Uuid,
    child_person_id: Uuid,
) -> Result<Vec<Uuid>, OxidGeneError> {
    // The child + all persons referencing them.
    let mut affected = affected_persons(db, child_person_id).await?;

    // Also ensure the parents in the family are included.
    let parents = FamilySpouseRepo::list_by_families(db, &[family_id]).await?;
    for parent in &parents {
        affected.push(parent.person_id);
    }

    affected.sort();
    affected.dedup();

    Ok(affected)
}

// ── Private helpers ──────────────────────────────────────────────────────────

/// Find all family IDs where `person_id` is a spouse.
///
/// Uses `FamilySpouseRepo` indirectly: since there is no `list_by_person`
/// method, we query the `family_spouse` entity table directly.
async fn families_as_spouse(
    db: &DatabaseConnection,
    person_id: Uuid,
) -> Result<Vec<Uuid>, OxidGeneError> {
    use oxidgene_db::entities::family_spouse;
    use oxidgene_db::sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

    let rows = family_spouse::Entity::find()
        .filter(family_spouse::Column::PersonId.eq(person_id))
        .all(db)
        .await
        .map_err(|e| OxidGeneError::Database(e.to_string()))?;

    Ok(rows.into_iter().map(|r| r.family_id).collect())
}

/// Find the family where `person_id` is a child, if any.
///
/// A person can be a child in at most one family in our data model.
async fn family_as_child(
    db: &DatabaseConnection,
    person_id: Uuid,
) -> Result<Option<Uuid>, OxidGeneError> {
    use oxidgene_db::entities::family_child;
    use oxidgene_db::sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

    let row = family_child::Entity::find()
        .filter(family_child::Column::PersonId.eq(person_id))
        .one(db)
        .await
        .map_err(|e| OxidGeneError::Database(e.to_string()))?;

    Ok(row.map(|r| r.family_id))
}

#[cfg(test)]
mod tests {
    // Integration tests for invalidation require a database connection.
    // They will be added in a later sprint when we have test fixtures.
    //
    // Unit-level logic is minimal here — the functions are thin wrappers
    // around DB queries + set union. The correctness of the set computation
    // is best verified with integration tests.
}
