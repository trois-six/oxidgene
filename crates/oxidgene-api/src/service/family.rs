//! Family writes — the family itself, its spouses and its children: the same
//! steps whether REST or GraphQL asked.
//!
//! A spouse or child link is addressed through its family and must belong to
//! it: a link of another family — in this tree or another — is not found.
//! Each write rewrites the projections of the persons whose family the link
//! changes, and records the change against that family, in one transaction.

use oxidgene_core::OxidGeneError;
use oxidgene_core::enums::{ChildType, Privacy, SpouseRole};
use oxidgene_core::history::AuditEntity;
use oxidgene_core::types::{Family, FamilyChild, FamilySpouse};
use oxidgene_db::repo::{FamilyChildRepo, FamilyRepo, FamilySpouseRepo};
use oxidgene_db::sea_orm::DatabaseConnection;
use serde::Deserialize;
use uuid::Uuid;

use crate::profile::{ProfileService, invalidation};
use crate::service::history::Change;
use crate::service::scope::{TreeResource, begin_tx, commit_tx, require_tree_resource};

/// The fields a family update changes; `None` keeps a field. An update
/// touches `updated_at` whatever it carries.
#[derive(Debug, Default, Deserialize)]
pub struct FamilyPatch {
    pub privacy: Option<Privacy>,
}

/// A spouse to add to a family.
#[derive(Debug, Deserialize)]
pub struct NewSpouse {
    pub person_id: Uuid,
    pub role: SpouseRole,
    #[serde(default)]
    pub sort_order: i32,
}

/// A child to add to a family.
#[derive(Debug, Deserialize)]
pub struct NewChild {
    pub person_id: Uuid,
    pub child_type: ChildType,
    #[serde(default)]
    pub sort_order: i32,
}

/// Create an empty family in `tree_id`. Nobody's projection shows it yet.
pub async fn create_family(
    db: &DatabaseConnection,
    tree_id: Uuid,
) -> Result<Family, OxidGeneError> {
    let id = Uuid::now_v7();
    let txn = begin_tx(db).await?;
    let pending = Change::create(tree_id, AuditEntity::Family, id)
        .family(id)
        .prepare(&txn)
        .await?;
    let family = FamilyRepo::create(&txn, id, tree_id).await?;
    pending.record(&txn).await?;
    commit_tx(txn).await?;
    Ok(family)
}

/// Update family `family_id` of `tree_id`.
pub async fn update_family(
    db: &DatabaseConnection,
    tree_id: Uuid,
    family_id: Uuid,
    patch: FamilyPatch,
) -> Result<Family, OxidGeneError> {
    let txn = begin_tx(db).await?;
    require_tree_resource(&txn, tree_id, TreeResource::Family, family_id).await?;
    let pending = Change::update(tree_id, AuditEntity::Family, family_id)
        .family(family_id)
        .prepare(&txn)
        .await?;
    let family = FamilyRepo::update(&txn, family_id, patch.privacy).await?;
    pending.record(&txn).await?;
    commit_tx(txn).await?;
    Ok(family)
}

/// Delete family `family_id` of `tree_id` (a soft delete), rewriting the
/// projections of its members.
pub async fn delete_family(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    family_id: Uuid,
) -> Result<(), OxidGeneError> {
    let txn = begin_tx(db).await?;
    require_tree_resource(&txn, tree_id, TreeResource::Family, family_id).await?;
    // Read while the links still exist.
    let affected = invalidation::affected_persons_for_family_delete(&txn, family_id).await?;
    let pending = Change::delete(tree_id, AuditEntity::Family, family_id)
        .family(family_id)
        .persons(affected.iter().copied())
        .prepare(&txn)
        .await?;
    FamilyRepo::delete(&txn, family_id).await?;
    profiles
        .invalidate_for_mutation(&txn, tree_id, &affected)
        .await?;
    pending.record(&txn).await?;
    commit_tx(txn).await
}

/// The spouses of family `family_id` of `tree_id`.
pub async fn list_spouses(
    db: &DatabaseConnection,
    tree_id: Uuid,
    family_id: Uuid,
) -> Result<Vec<FamilySpouse>, OxidGeneError> {
    require_tree_resource(db, tree_id, TreeResource::Family, family_id).await?;
    FamilySpouseRepo::list_by_family(db, family_id).await
}

/// The children of family `family_id` of `tree_id`.
pub async fn list_children(
    db: &DatabaseConnection,
    tree_id: Uuid,
    family_id: Uuid,
) -> Result<Vec<FamilyChild>, OxidGeneError> {
    require_tree_resource(db, tree_id, TreeResource::Family, family_id).await?;
    FamilyChildRepo::list_by_family(db, family_id).await
}

/// Add a spouse to family `family_id` of `tree_id`.
pub async fn add_spouse(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    family_id: Uuid,
    new: NewSpouse,
) -> Result<FamilySpouse, OxidGeneError> {
    let id = Uuid::now_v7();
    let txn = begin_tx(db).await?;
    require_tree_resource(&txn, tree_id, TreeResource::Family, family_id).await?;
    require_tree_resource(&txn, tree_id, TreeResource::Person, new.person_id).await?;
    // The new spouse is named: the family's spouses are read before the link.
    let pending = Change::create(tree_id, AuditEntity::FamilySpouse, id)
        .person(new.person_id)
        .family(family_id)
        .prepare(&txn)
        .await?;
    let spouse =
        FamilySpouseRepo::create(&txn, id, family_id, new.person_id, new.role, new.sort_order)
            .await?;
    let affected =
        invalidation::affected_persons_for_family_spouse_change(&txn, family_id, new.person_id)
            .await?;
    profiles
        .invalidate_for_mutation(&txn, tree_id, &affected)
        .await?;
    pending.record(&txn).await?;
    commit_tx(txn).await?;
    Ok(spouse)
}

/// Remove spouse link `link_id` from family `family_id` of `tree_id`.
pub async fn remove_spouse(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    family_id: Uuid,
    link_id: Uuid,
) -> Result<(), OxidGeneError> {
    let txn = begin_tx(db).await?;
    require_tree_resource(&txn, tree_id, TreeResource::Family, family_id).await?;
    let link = FamilySpouseRepo::get(&txn, link_id).await?;
    if link.family_id != family_id {
        return Err(OxidGeneError::NotFound {
            entity: "FamilySpouse",
            id: link_id,
        });
    }
    // Read while the link still exists.
    let affected =
        invalidation::affected_persons_for_family_spouse_change(&txn, family_id, link.person_id)
            .await?;
    let pending = Change::delete(tree_id, AuditEntity::FamilySpouse, link_id)
        .person(link.person_id)
        .family(family_id)
        .prepare(&txn)
        .await?;
    FamilySpouseRepo::delete(&txn, link_id).await?;
    profiles
        .invalidate_for_mutation(&txn, tree_id, &affected)
        .await?;
    pending.record(&txn).await?;
    commit_tx(txn).await
}

/// Add a child to family `family_id` of `tree_id`.
pub async fn add_child(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    family_id: Uuid,
    new: NewChild,
) -> Result<FamilyChild, OxidGeneError> {
    let id = Uuid::now_v7();
    let txn = begin_tx(db).await?;
    require_tree_resource(&txn, tree_id, TreeResource::Family, family_id).await?;
    require_tree_resource(&txn, tree_id, TreeResource::Person, new.person_id).await?;
    let pending = Change::create(tree_id, AuditEntity::FamilyChild, id)
        .person(new.person_id)
        .family(family_id)
        .prepare(&txn)
        .await?;
    let child = FamilyChildRepo::create(
        &txn,
        id,
        family_id,
        new.person_id,
        new.child_type,
        new.sort_order,
    )
    .await?;
    let affected =
        invalidation::affected_persons_for_family_child_change(&txn, family_id, new.person_id)
            .await?;
    profiles
        .invalidate_for_mutation(&txn, tree_id, &affected)
        .await?;
    pending.record(&txn).await?;
    commit_tx(txn).await?;
    Ok(child)
}

/// Remove child link `link_id` from family `family_id` of `tree_id`.
pub async fn remove_child(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    family_id: Uuid,
    link_id: Uuid,
) -> Result<(), OxidGeneError> {
    let txn = begin_tx(db).await?;
    require_tree_resource(&txn, tree_id, TreeResource::Family, family_id).await?;
    let link = FamilyChildRepo::get(&txn, link_id).await?;
    if link.family_id != family_id {
        return Err(OxidGeneError::NotFound {
            entity: "FamilyChild",
            id: link_id,
        });
    }
    // Read while the link still exists.
    let affected =
        invalidation::affected_persons_for_family_child_change(&txn, family_id, link.person_id)
            .await?;
    let pending = Change::delete(tree_id, AuditEntity::FamilyChild, link_id)
        .person(link.person_id)
        .family(family_id)
        .prepare(&txn)
        .await?;
    FamilyChildRepo::delete(&txn, link_id).await?;
    profiles
        .invalidate_for_mutation(&txn, tree_id, &affected)
        .await?;
    pending.record(&txn).await?;
    commit_tx(txn).await
}
