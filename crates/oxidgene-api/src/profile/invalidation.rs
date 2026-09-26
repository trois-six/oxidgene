//! Affected-set computation.
//!
//! Given a mutation on a person (or a related entity), computes the bounded set
//! of persons whose projections must be rewritten — the person themselves, plus
//! everyone whose projection embeds their display name. The set is typically
//! 2–10 persons, keeping the synchronous refresh under 15 ms.
//!
//! This is the piece that survived the removal of the cache layer unchanged:
//! "who is affected by this change" is a domain question, not a caching one.

use oxidgene_core::error::OxidGeneError;
use oxidgene_db::repo::{FamilyChildRepo, FamilySpouseRepo};
use oxidgene_db::sea_orm::ConnectionTrait;
use uuid::Uuid;

/// Compute the set of person IDs whose [`PersonProfile`] entries are affected
/// by a mutation involving `person_id`.
///
/// The affected set includes:
/// 1. The person itself.
/// 2. All co-spouses and children in families where this person is a spouse
///    (their [`ProfileFamilyLink`] references this person's display name).
/// 3. All spouses (parents) of every family where this person is a child —
///    biological and adoptive alike — since their
///    [`ProfileFamilyLink::children_ids`] references this person.
///
/// The result is de-duplicated but not otherwise ordered.
pub async fn affected_persons(
    db: &impl ConnectionTrait,
    person_id: Uuid,
) -> Result<Vec<Uuid>, OxidGeneError> {
    let (as_spouse, as_child) = tokio::try_join!(
        FamilySpouseRepo::list_by_person(db, person_id),
        FamilyChildRepo::list_by_person(db, person_id),
    )?;
    let spouse_families: Vec<Uuid> = as_spouse.iter().map(|s| s.family_id).collect();
    let mut families = spouse_families.clone();
    families.extend(as_child.iter().map(|c| c.family_id));

    let (spouses, children) = tokio::try_join!(
        FamilySpouseRepo::list_by_families(db, &families),
        FamilyChildRepo::list_by_families(db, &spouse_families),
    )?;

    let mut affected = vec![person_id];
    affected.extend(spouses.iter().map(|s| s.person_id));
    affected.extend(children.iter().map(|c| c.person_id));
    affected.sort();
    affected.dedup();

    Ok(affected)
}

/// Compute the affected set for a family event mutation.
///
/// Family events (marriage, divorce, etc.) affect both spouses in the family.
/// Returns the set of persons whose projections need rewriting.
pub async fn affected_persons_for_family(
    db: &impl ConnectionTrait,
    family_id: Uuid,
) -> Result<Vec<Uuid>, OxidGeneError> {
    let spouses = FamilySpouseRepo::list_by_families(db, &[family_id]).await?;
    let mut affected: Vec<Uuid> = spouses.iter().map(|s| s.person_id).collect();

    // Each spouse's full affected set includes their other families' members.
    // But for a family event, we only need to rebuild the two spouses — their
    // PersonProfile includes the family's marriage event.
    affected.sort();
    affected.dedup();

    Ok(affected)
}

/// Compute the affected set for deleting a family.
///
/// Wider than [`affected_persons_for_family`], which is right for a family
/// *event*: changing a marriage date alters nothing anyone else's projection
/// records. Removing the family does. Its children lose their
/// [`oxidgene_core::projection::ProfileChildLink`], which carries their
/// parents' names, and its spouses lose each other — so every member has to be
/// rebuilt, not just the two spouses.
///
/// Callers must compute this *before* the delete, while the links still exist.
pub async fn affected_persons_for_family_delete(
    db: &impl ConnectionTrait,
    family_id: Uuid,
) -> Result<Vec<Uuid>, OxidGeneError> {
    let mut affected = affected_persons_for_family(db, family_id).await?;
    let children = FamilyChildRepo::list_by_families(db, &[family_id]).await?;
    affected.extend(children.iter().map(|child| child.person_id));

    affected.sort();
    affected.dedup();

    Ok(affected)
}

/// Compute the persons whose individual or family events reference a place.
pub async fn affected_persons_for_place(
    db: &impl ConnectionTrait,
    place_id: Uuid,
) -> Result<Vec<Uuid>, OxidGeneError> {
    use oxidgene_db::entities::event;
    use oxidgene_db::sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

    let events = event::Entity::find()
        .filter(event::Column::PlaceId.eq(place_id))
        .filter(event::Column::DeletedAt.is_null())
        .all(db)
        .await
        .map_err(|error| OxidGeneError::Database(error.to_string()))?;

    let mut affected: Vec<Uuid> = events.iter().filter_map(|event| event.person_id).collect();
    let family_ids: Vec<Uuid> = events.iter().filter_map(|event| event.family_id).collect();
    if !family_ids.is_empty() {
        affected.extend(
            FamilySpouseRepo::list_by_families(db, &family_ids)
                .await?
                .into_iter()
                .map(|spouse| spouse.person_id),
        );
    }
    affected.sort();
    affected.dedup();
    Ok(affected)
}

/// Compute affected persons when a family membership changes (spouse added/removed).
///
/// This is broader than a simple person edit: both spouses, all children in the
/// family, AND the parents of both spouses (since their ProfileChildLink
/// references might change) are affected.
pub async fn affected_persons_for_family_spouse_change(
    db: &impl ConnectionTrait,
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
    db: &impl ConnectionTrait,
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
