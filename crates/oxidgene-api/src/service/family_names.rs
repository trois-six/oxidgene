//! The dictionary's family-name rename, as one workflow for REST and GraphQL.
//!
//! See `docs/ui-dictionary.md` §7.1 and the rename route of `docs/api.md`.

use oxidgene_core::error::OxidGeneError;
use oxidgene_db::repo::{DictionaryRepo, FamilyNameRename};
use oxidgene_db::sea_orm::ConnectionTrait;
use uuid::Uuid;

use crate::profile::{ProfileService, invalidation};
use crate::service::history;

/// Rename surname `value` to `new_value` on every primary name carrying it
/// (see [`DictionaryRepo::rename_family_name`]), then refresh what shows it
/// and record the change — all on `conn`, the caller's transaction.
///
/// A surname is shown by its carriers' own projections and search rows, and
/// by their relatives': a spouse's or child's family link and a child's
/// father and mother filters name it. Those persons are refreshed, and nobody
/// else, so the cost follows the name's family rather than the tree.
pub async fn rename(
    conn: &impl ConnectionTrait,
    profiles: &ProfileService,
    tree_id: Uuid,
    value: &str,
    new_value: &str,
    particle: Option<&str>,
) -> Result<FamilyNameRename, OxidGeneError> {
    let renamed =
        DictionaryRepo::rename_family_name(conn, tree_id, value, new_value, particle).await?;
    if renamed.names_updated == 0 {
        return Ok(renamed);
    }
    let affected = invalidation::affected_persons_of_all(conn, &renamed.person_ids).await?;
    profiles
        .invalidate_for_mutation(conn, tree_id, &affected)
        .await?;
    history::family_name_rename_change(tree_id, &renamed)
        .record(conn)
        .await?;
    Ok(renamed)
}
