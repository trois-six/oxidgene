//! Reading the access log (docs/authorization.md §9.2): an owner reads
//! their tree's events, an administrator the instance's. Web build only
//! (`auth` feature); the `none` mode records nothing and answers `404`, as
//! the desktop does.

use oxidgene_core::OxidGeneError;
use oxidgene_core::access::{Principal, ViewerContext};
use oxidgene_core::types::Connection;
use oxidgene_db::repo::{AccessEvent, AccessEventRepo, PaginationParams};
use sea_orm::DatabaseConnection;
use uuid::Uuid;

use crate::service::account::require_admin;

/// The events of tree `tree_id`, newest first. The operation's requirement,
/// `access_log_read`, is checked before.
pub async fn tree_events(
    viewer: &ViewerContext,
    db: &DatabaseConnection,
    tree_id: Uuid,
    params: &PaginationParams,
) -> Result<Connection<AccessEvent>, OxidGeneError> {
    if !matches!(viewer.principal(), Principal::User(_)) {
        return Err(OxidGeneError::NotFound {
            entity: "AccessLog",
            id: tree_id,
        });
    }
    AccessEventRepo::list(db, Some(tree_id), params).await
}

/// Every event of the instance, newest first, for an administrator.
pub async fn instance_events(
    viewer: &ViewerContext,
    db: &DatabaseConnection,
    params: &PaginationParams,
) -> Result<Connection<AccessEvent>, OxidGeneError> {
    require_admin(viewer.principal())?;
    AccessEventRepo::list(db, None, params).await
}
