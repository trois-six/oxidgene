//! What every resolver does with the identifiers it receives: parse them, and
//! check that the tree they are scoped to is live.

use async_graphql::{Context, ID, Result};
use oxidgene_core::OxidGeneError;
use uuid::Uuid;

use super::types::db_from_ctx;
use crate::service::scope::require_live_tree;

/// Parse an identifier. A malformed one is a validation error, as it is on
/// REST.
pub(crate) fn uuid(id: impl AsRef<str>) -> Result<Uuid> {
    Uuid::parse_str(id.as_ref())
        .map_err(|_| OxidGeneError::Validation("malformed identifier".to_string()).into())
}

/// Parse an optional identifier.
pub(crate) fn opt_uuid(id: Option<impl AsRef<str>>) -> Result<Option<Uuid>> {
    id.map(uuid).transpose()
}

/// Parse a list of identifiers; any malformed one fails the lot.
pub(crate) fn uuids(ids: &[impl AsRef<str>]) -> Result<Vec<Uuid>> {
    ids.iter().map(uuid).collect()
}

/// The tree `id` names, which must exist and not be deleted.
///
/// GraphQL's counterpart of REST's tree guard (`rest::tree_guard`): a tree is
/// purged in the background a few seconds after it is deleted, and until then
/// its rows are still there. Every tree-scoped resolver starts here, so a
/// deleted or unknown tree answers `NOT_FOUND` — never its data, never a write
/// into it.
pub(crate) async fn live_tree(ctx: &Context<'_>, id: &ID) -> Result<Uuid> {
    let tree_id = uuid(id)?;
    require_live_tree(db_from_ctx(ctx), tree_id).await?;
    Ok(tree_id)
}
