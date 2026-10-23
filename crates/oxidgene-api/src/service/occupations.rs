//! The dictionary's occupation rename, as one workflow for REST and GraphQL.
//!
//! See `docs/ui-dictionary.md` §10.1 and the occupation routes of
//! `docs/api.md`.

use oxidgene_core::error::OxidGeneError;
use oxidgene_db::repo::{DictionaryRepo, OccupationRename};
use oxidgene_db::sea_orm::DatabaseConnection;
use serde::Deserialize;
use uuid::Uuid;

use crate::profile::ProfileService;
use crate::service::history;
use crate::service::scope::{begin_tx, commit_tx};

/// An occupation to rename.
#[derive(Debug, Deserialize)]
pub struct OccupationChange {
    /// The occupation as the dictionary lists it.
    pub value: String,
    /// What it becomes.
    pub new_value: String,
}

/// Rename an occupation of `tree_id` on every event labelled with it (see
/// [`DictionaryRepo::rename_occupation`]), then refresh the persons holding
/// it and record the change, in one transaction.
///
/// An occupation shows on its holder's own profile and nowhere else, so only
/// those persons are refreshed.
pub async fn rename(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    change: OccupationChange,
) -> Result<OccupationRename, OxidGeneError> {
    let txn = begin_tx(db).await?;
    let holders =
        DictionaryRepo::persons_holding_occupations(&txn, tree_id, &[&change.value]).await?;
    let pending = history::occupation_change(tree_id, change.value.trim(), &holders)
        .prepare(&txn)
        .await?;
    let renamed =
        DictionaryRepo::rename_occupation(&txn, tree_id, &change.value, &change.new_value).await?;
    if renamed.events_updated > 0 {
        profiles
            .invalidate_for_mutation(&txn, tree_id, &renamed.person_ids)
            .await?;
        pending
            .details(history::occupation_rename_details(&renamed))
            .record(&txn)
            .await?;
    }
    commit_tx(txn).await?;
    Ok(renamed)
}
