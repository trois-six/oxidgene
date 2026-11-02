//! The owner's exclusions from sharing (docs/privacy.md §6): creating,
//! listing and removing them, and keeping their union, `excluded_person`,
//! current. REST and GraphQL both go through here. Web build only.
//!
//! The union is rebuilt in the transaction of every exclusion written, and
//! otherwise whenever the tree's SOSA ancestry generation has moved since it
//! was built — a family link added or removed, a person or a family deleted
//! or restored — by the next read that needs it, on the writer, as an access
//! scope is (docs/authorization.md §6.5). A child added under an excluded
//! root is therefore excluded at the next read.

use std::collections::{HashMap, HashSet};

use chrono::Utc;
use oxidgene_core::OxidGeneError;
use oxidgene_core::access::ViewerContext;
use oxidgene_core::enums::ExclusionKind;
use oxidgene_core::history::{AuditEntity, AuditSubject};
use oxidgene_core::privacy::{ExclusionScope, Kinship};
use oxidgene_db::repo::{Exclusion, ExclusionRepo, display_names};
use oxidgene_db::sea_orm::{ConnectionTrait, DatabaseConnection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::service::history::Change;
use crate::service::scope::{TreeResource, begin_tx, commit_tx, require_tree_resource};
use crate::service::tag_bulk::{MAX_DEPTH, MIN_DEPTH};

/// An exclusion to create.
#[derive(Debug, Clone, Deserialize)]
pub struct NewExclusion {
    pub kind: ExclusionKind,
    /// The person excluded, or the root of the excluded branch.
    pub person_id: Uuid,
    /// The generations an ancestry or a descendancy walks, 1 to 30; all of
    /// them when absent.
    #[serde(default)]
    pub depth: Option<i32>,
    /// The root itself: `false` by default for ancestors, `true` for
    /// descendants.
    #[serde(default)]
    pub include_root: Option<bool>,
    /// The spouses of the descendants: `true` by default.
    #[serde(default)]
    pub include_spouses: Option<bool>,
}

/// An exclusion as its list shows it.
#[derive(Debug, Clone, Serialize)]
pub struct ExclusionView {
    pub id: Uuid,
    pub kind: ExclusionKind,
    pub person_id: Uuid,
    pub depth: Option<i32>,
    pub include_root: bool,
    pub include_spouses: bool,
    pub created_by: Option<Uuid>,
    pub created_at: chrono::DateTime<Utc>,
    /// The root's display name; none when they have none.
    pub person_name: Option<String>,
    /// Whether the root is deleted: the exclusion covers nobody until it is
    /// restored.
    pub person_deleted: bool,
    /// The persons it covers now.
    pub person_count: u64,
}

/// What `exclusion` names.
fn scope_of(exclusion: &Exclusion) -> ExclusionScope {
    ExclusionScope {
        kind: exclusion.kind,
        root: exclusion.person_id,
        depth: exclusion.depth.and_then(|depth| u32::try_from(depth).ok()),
        include_root: exclusion.include_root,
        include_spouses: exclusion.include_spouses,
    }
}

/// The live persons of `tree_id` and how their families link them.
async fn links(
    conn: &impl ConnectionTrait,
    tree_id: Uuid,
) -> Result<(Kinship, HashSet<Uuid>), OxidGeneError> {
    crate::service::access_scope::kinship(conn, tree_id).await
}

/// Rebuild the union of `tree_id` in the transaction `conn`, when it is
/// stale or `force`. The tree's state is locked first, so that a rebuild
/// running meanwhile either commits before this one reads, or waits for it.
async fn rebuild(
    conn: &impl ConnectionTrait,
    tree_id: Uuid,
    force: bool,
) -> Result<(), OxidGeneError> {
    let (generation, built) = ExclusionRepo::lock_generations(conn, tree_id).await?;
    if !force && built == Some(generation) {
        return Ok(());
    }
    let exclusions = ExclusionRepo::list(conn, tree_id).await?;
    let mut union = HashSet::new();
    if !exclusions.is_empty() {
        let (kin, live) = links(conn, tree_id).await?;
        for exclusion in &exclusions {
            union.extend(scope_of(exclusion).persons(&kin, &live));
            // A deleted root covers nobody until it is restored, but stays
            // excluded itself: its events, notes and citations outlive it
            // (docs/privacy.md §6.5).
            if !live.contains(&exclusion.person_id)
                && (exclusion.kind == ExclusionKind::Person || exclusion.include_root)
            {
                union.insert(exclusion.person_id);
            }
        }
    }
    ExclusionRepo::replace_union(conn, tree_id, &union, generation).await
}

/// The persons of `tree_id` excluded from sharing, the union rebuilt first
/// on the writer `db` when the tree's family links moved since it was
/// built.
pub async fn excluded_persons(
    db: &DatabaseConnection,
    tree_id: Uuid,
) -> Result<HashSet<Uuid>, OxidGeneError> {
    let (generation, built) = ExclusionRepo::generations(db, tree_id).await?;
    if built != Some(generation) {
        let txn = begin_tx(db).await?;
        rebuild(&txn, tree_id, false).await?;
        commit_tx(txn).await?;
    }
    ExclusionRepo::excluded(db, tree_id).await
}

/// The exclusions of `tree_id`, oldest first, each with its root and the
/// number of persons it covers now.
pub async fn list_exclusions(
    db: &DatabaseConnection,
    tree_id: Uuid,
) -> Result<Vec<ExclusionView>, OxidGeneError> {
    oxidgene_db::repo::TreeRepo::get(db, tree_id).await?;
    let exclusions = ExclusionRepo::list(db, tree_id).await?;
    if exclusions.is_empty() {
        return Ok(Vec::new());
    }
    let (kin, live) = links(db, tree_id).await?;
    let roots: Vec<Uuid> = exclusions.iter().map(|e| e.person_id).collect();
    let names: HashMap<Uuid, String> = display_names(db, &roots).await?.into_iter().collect();
    Ok(exclusions
        .into_iter()
        .map(|exclusion| {
            let person_count = scope_of(&exclusion).persons(&kin, &live).len() as u64;
            ExclusionView {
                person_name: names.get(&exclusion.person_id).cloned(),
                person_deleted: !live.contains(&exclusion.person_id),
                person_count,
                id: exclusion.id,
                kind: exclusion.kind,
                person_id: exclusion.person_id,
                depth: exclusion.depth,
                include_root: exclusion.include_root,
                include_spouses: exclusion.include_spouses,
                created_by: exclusion.created_by,
                created_at: exclusion.created_at,
            }
        })
        .collect())
}

/// Exclude what `new` names from sharing in `tree_id`, recorded in the
/// tree's history as the viewer's. A root that is not a live person of the
/// tree is not found; a depth out of 1 to 30 is a validation error.
pub async fn create_exclusion(
    viewer: &ViewerContext,
    db: &DatabaseConnection,
    tree_id: Uuid,
    new: NewExclusion,
) -> Result<ExclusionView, OxidGeneError> {
    if let Some(depth) = new.depth
        && !(MIN_DEPTH..=MAX_DEPTH).contains(&i64::from(depth))
    {
        return Err(OxidGeneError::Validation(format!(
            "an exclusion's depth is between {MIN_DEPTH} and {MAX_DEPTH} generations"
        )));
    }
    let exclusion = Exclusion {
        id: Uuid::now_v7(),
        tree_id,
        kind: new.kind,
        person_id: new.person_id,
        depth: new.depth.filter(|_| new.kind != ExclusionKind::Person),
        include_root: match new.kind {
            ExclusionKind::Person => true,
            ExclusionKind::Ancestors => new.include_root.unwrap_or(false),
            ExclusionKind::Descendants => new.include_root.unwrap_or(true),
        },
        include_spouses: new.kind == ExclusionKind::Descendants
            && new.include_spouses.unwrap_or(true),
        created_by: viewer.user().map(|identity| identity.user_id),
        created_at: Utc::now(),
    };
    let txn = begin_tx(db).await?;
    require_tree_resource(&txn, tree_id, TreeResource::Person, new.person_id).await?;
    ExclusionRepo::create(&txn, &exclusion).await?;
    rebuild(&txn, tree_id, true).await?;
    Change::create(viewer, tree_id, AuditEntity::PrivacyExclusion, exclusion.id)
        .about(AuditSubject::Person, exclusion.person_id)
        .record_unversioned(&txn)
        .await?;
    commit_tx(txn).await?;
    list_exclusions(db, tree_id)
        .await?
        .into_iter()
        .find(|view| view.id == exclusion.id)
        .ok_or(OxidGeneError::NotFound {
            entity: "Exclusion",
            id: exclusion.id,
        })
}

/// Remove exclusion `exclusion_id` of `tree_id`, recorded in the tree's
/// history as the viewer's.
pub async fn delete_exclusion(
    viewer: &ViewerContext,
    db: &DatabaseConnection,
    tree_id: Uuid,
    exclusion_id: Uuid,
) -> Result<(), OxidGeneError> {
    let txn = begin_tx(db).await?;
    let exclusion = ExclusionRepo::get(&txn, tree_id, exclusion_id).await?;
    ExclusionRepo::delete(&txn, tree_id, exclusion_id).await?;
    rebuild(&txn, tree_id, true).await?;
    Change::delete(viewer, tree_id, AuditEntity::PrivacyExclusion, exclusion_id)
        .about(AuditSubject::Person, exclusion.person_id)
        .record_unversioned(&txn)
        .await?;
    commit_tx(txn).await
}
