//! The dictionary's family-name repairs — re-cutting a surname at its
//! particle, renaming it — as one workflow each for REST and GraphQL.
//!
//! See `docs/ui-dictionary.md` §7.1 and the family-name routes of
//! `docs/api.md`.

use oxidgene_core::error::OxidGeneError;
use oxidgene_db::repo::{DictionaryRepo, FamilyNameParticleUpdate, FamilyNameRename};
use oxidgene_db::sea_orm::DatabaseConnection;
use serde::Deserialize;
use uuid::Uuid;

use crate::profile::{ProfileService, invalidation};
use crate::service::history;
use crate::service::scope::{begin_tx, commit_tx};

/// A surname to re-cut at a particle.
#[derive(Debug, Deserialize)]
pub struct ParticleChange {
    /// The surname as the dictionary lists it.
    pub value: String,
    /// The particle it starts with ("de la", "van der").
    pub particle: String,
}

/// A surname to rename.
#[derive(Debug, Deserialize)]
pub struct FamilyNameChange {
    /// The surname as the dictionary lists it.
    pub value: String,
    /// What it becomes.
    pub new_value: String,
    /// The particle the new surname starts with, if any.
    pub particle: Option<String>,
}

/// Re-cut every occurrence of a surname of `tree_id` at a particle — the
/// bulk repair for an import that guessed wrong across a whole family.
///
/// A surname reaches every projection that embeds a display name, so the
/// affected set of this edit is unbounded in practice: the tree is rebuilt
/// eagerly once the change is committed, outside the transaction, as an
/// import does, rather than holding a write lock over every row of a large
/// tree. Nothing is rebuilt, or recorded, when nothing changed.
pub async fn set_particle(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    change: ParticleChange,
) -> Result<FamilyNameParticleUpdate, OxidGeneError> {
    let txn = begin_tx(db).await?;
    let update =
        DictionaryRepo::set_family_name_particle(&txn, tree_id, &change.value, &change.particle)
            .await?;
    if update.names_updated > 0 {
        history::family_name_change(tree_id, &update)
            .record(&txn)
            .await?;
    }
    commit_tx(txn).await?;
    if update.names_updated > 0 {
        profiles.rebuild_tree_full(db, tree_id).await?;
    }
    Ok(update)
}

/// Rename a surname of `tree_id` on every primary name carrying it (see
/// [`DictionaryRepo::rename_family_name`]), then refresh what shows it and
/// record the change, in one transaction.
///
/// A surname is shown by its carriers' own projections and search rows, and
/// by their relatives': a spouse's or child's family link and a child's
/// father and mother filters name it. Those persons are refreshed, and nobody
/// else, so the cost follows the name's family rather than the tree.
pub async fn rename(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    change: FamilyNameChange,
) -> Result<FamilyNameRename, OxidGeneError> {
    let txn = begin_tx(db).await?;
    let renamed = DictionaryRepo::rename_family_name(
        &txn,
        tree_id,
        &change.value,
        &change.new_value,
        change.particle.as_deref(),
    )
    .await?;
    if renamed.names_updated > 0 {
        let affected = invalidation::affected_persons_of_all(&txn, &renamed.person_ids).await?;
        profiles
            .invalidate_for_mutation(&txn, tree_id, &affected)
            .await?;
        history::family_name_rename_change(tree_id, &renamed)
            .record(&txn)
            .await?;
    }
    commit_tx(txn).await?;
    Ok(renamed)
}
