//! Share links (docs/authorization.md §6.3): a token that gives Visitor
//! access to a tree without an account — creating, listing, changing and
//! revoking them for the owner, and redeeming one for whoever opens it. REST
//! and GraphQL both go through here. Web build only.
//!
//! The token is 256 random bits, base64url, prefixed `ogs_` so that secret
//! scanners recognize it; only its SHA-256 is stored, which suffices for a
//! value of that entropy. It is returned once, in the link's URL
//! `{public_url}/auth/share/{token}`, and never readable again. A link
//! grants Visitor access only: a holder is not identified, so contemporaries
//! stay hidden (open decision 4).

use chrono::{DateTime, Utc};
use oxidgene_core::OxidGeneError;
use oxidgene_core::access::ViewerContext;
use oxidgene_core::history::{AuditAction, AuditEntity};
use oxidgene_core::sharing::ScopeDefinition;
use oxidgene_core::types::{Connection, Edge};
use oxidgene_db::repo::{
    NewShareLink, PaginationParams, ShareLink, ShareLinkChanges, ShareLinkRepo,
};
use oxidgene_db::sea_orm::{ConnectionTrait, DatabaseConnection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::authz;
use crate::service::access_scope::{self, ScopeView};
use crate::service::patch::double_option;
use crate::service::scope::{begin_tx, commit_tx};
use crate::service::sharing::{SharingEnv, future, record};
use crate::tree_cache::TreeCache;

/// The prefix of every share-link token.
pub const TOKEN_PREFIX: &str = "ogs_";
/// The longest label a link may carry.
pub const MAX_LABEL: usize = 100;

/// A share link as the owner's sharing section lists it: never its token.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ShareLinkView {
    pub id: Uuid,
    pub label: Option<String>,
    pub scope: Option<ScopeView>,
    pub expires_at: Option<DateTime<Utc>>,
    /// Whether the link has expired: it grants nothing any more.
    pub expired: bool,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub use_count: i64,
}

/// A link just created, with its URL: shown once.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CreatedShareLink {
    pub link: ShareLinkView,
    /// `{public_url}/auth/share/{token}`.
    pub url: String,
}

/// A link to create.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct NewShareLinkRequest {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub scope: Option<ScopeDefinition>,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
}

/// What a link update changes: `None` keeps a field, `Some(None)` clears
/// it.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ShareLinkPatch {
    #[serde(default, deserialize_with = "double_option")]
    pub label: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub scope: Option<Option<ScopeDefinition>>,
    #[serde(default, deserialize_with = "double_option")]
    pub expires_at: Option<Option<DateTime<Utc>>>,
}

/// `label`, trimmed; none when blank; refused when longer than
/// [`MAX_LABEL`].
fn checked_label(label: Option<String>) -> Result<Option<String>, OxidGeneError> {
    let label = label
        .map(|label| label.trim().to_string())
        .filter(|label| !label.is_empty());
    match label {
        Some(label) if label.chars().count() > MAX_LABEL || label.contains('\n') => Err(
            OxidGeneError::Validation(format!("a link's label is at most {MAX_LABEL} characters")),
        ),
        label => Ok(label),
    }
}

/// The views of `links`.
async fn views(
    conn: &impl ConnectionTrait,
    links: Vec<ShareLink>,
) -> Result<Vec<ShareLinkView>, OxidGeneError> {
    let scope_ids: Vec<Uuid> = links.iter().filter_map(|l| l.scope_id).collect();
    let scopes = access_scope::views(conn, &scope_ids).await?;
    let now = Utc::now();
    Ok(links
        .into_iter()
        .map(|link| ShareLinkView {
            scope: link.scope_id.and_then(|id| scopes.get(&id).cloned()),
            expired: !link.is_live(now),
            id: link.id,
            label: link.label,
            expires_at: link.expires_at,
            created_at: link.created_at,
            last_used_at: link.last_used_at,
            use_count: link.use_count,
        })
        .collect())
}

async fn one_view(
    conn: &impl ConnectionTrait,
    link: ShareLink,
) -> Result<ShareLinkView, OxidGeneError> {
    let id = link.id;
    views(conn, vec![link])
        .await?
        .into_iter()
        .next()
        .ok_or(OxidGeneError::NotFound {
            entity: "ShareLink",
            id,
        })
}

/// The links of `tree_id` not revoked, oldest first.
pub async fn list(
    db: &DatabaseConnection,
    tree_id: Uuid,
    params: &PaginationParams,
) -> Result<Connection<ShareLinkView>, OxidGeneError> {
    let page = ShareLinkRepo::list(db, tree_id, params).await?;
    let cursors: Vec<String> = page.edges.iter().map(|e| e.cursor.clone()).collect();
    let links: Vec<ShareLink> = page.edges.into_iter().map(|e| e.node).collect();
    Ok(Connection {
        edges: views(db, links)
            .await?
            .into_iter()
            .zip(cursors)
            .map(|(node, cursor)| Edge { cursor, node })
            .collect(),
        page_info: page.page_info,
        total_count: page.total_count,
    })
}

/// Create a link to `tree_id` as `request` says; its URL is returned once.
pub async fn create(
    viewer: &ViewerContext,
    db: &DatabaseConnection,
    env: &SharingEnv,
    tree_id: Uuid,
    request: NewShareLinkRequest,
) -> Result<CreatedShareLink, OxidGeneError> {
    let label = checked_label(request.label)?;
    future(request.expires_at)?;
    let token = format!("{TOKEN_PREFIX}{}", crate::auth::crypto::random_token());
    let txn = begin_tx(db).await?;
    let scope_id = access_scope::create_optional(&txn, tree_id, request.scope.as_ref()).await?;
    let link = ShareLinkRepo::create(
        &txn,
        NewShareLink {
            tree_id,
            token_hash: crate::auth::crypto::digest(&token),
            label,
            scope_id,
            expires_at: request.expires_at,
            created_by: viewer.user().map(|identity| identity.user_id),
        },
    )
    .await?;
    record(
        &txn,
        viewer,
        tree_id,
        AuditAction::Create,
        AuditEntity::ShareLink,
        link.id,
    )
    .await?;
    commit_tx(txn).await?;
    Ok(CreatedShareLink {
        link: one_view(db, link).await?,
        url: format!("{}/auth/share/{token}", env.public_url),
    })
}

/// Change link `link_id` of `tree_id` as `patch` says.
pub async fn update(
    viewer: &ViewerContext,
    db: &DatabaseConnection,
    cache: &TreeCache,
    tree_id: Uuid,
    link_id: Uuid,
    patch: ShareLinkPatch,
) -> Result<ShareLinkView, OxidGeneError> {
    let label = patch.label.map(checked_label).transpose()?;
    future(patch.expires_at.flatten())?;
    let txn = begin_tx(db).await?;
    let current = ShareLinkRepo::get(&txn, tree_id, link_id).await?;
    if current.revoked_at.is_some() {
        return Err(OxidGeneError::NotFound {
            entity: "ShareLink",
            id: link_id,
        });
    }
    let scope_id = access_scope::replacement(&txn, tree_id, patch.scope.as_ref()).await?;
    let link = ShareLinkRepo::update(
        &txn,
        tree_id,
        link_id,
        ShareLinkChanges {
            label,
            scope_id,
            expires_at: patch.expires_at,
        },
    )
    .await?;
    access_scope::drop_replaced(&txn, scope_id, current.scope_id).await?;
    record(
        &txn,
        viewer,
        tree_id,
        AuditAction::Update,
        AuditEntity::ShareLink,
        link_id,
    )
    .await?;
    commit_tx(txn).await?;
    // The viewers this access gave may no longer see what was kept for
    // them (docs/architecture.md §4.1).
    cache.forget_redactions(tree_id);
    one_view(db, link).await
}

/// Revoke link `link_id` of `tree_id`: every grant made from it ends at
/// the next request.
pub async fn revoke(
    viewer: &ViewerContext,
    db: &DatabaseConnection,
    cache: &TreeCache,
    tree_id: Uuid,
    link_id: Uuid,
) -> Result<(), OxidGeneError> {
    let txn = begin_tx(db).await?;
    ShareLinkRepo::revoke(&txn, tree_id, link_id, Utc::now()).await?;
    record(
        &txn,
        viewer,
        tree_id,
        AuditAction::Delete,
        AuditEntity::ShareLink,
        link_id,
    )
    .await?;
    commit_tx(txn).await?;
    // The viewers this access gave may no longer see what was kept for
    // them (docs/architecture.md §4.1).
    cache.forget_redactions(tree_id);
    Ok(())
}

/// The live link whose token is `token`, its use counted and recorded in
/// the access log: what opening the link redeems. None for a token that
/// opens nothing — unknown, revoked, expired, or on a deleted tree.
pub async fn redeem(
    db: &DatabaseConnection,
    viewer: &ViewerContext,
    token: &str,
) -> Result<Option<ShareLink>, OxidGeneError> {
    if !token.starts_with(TOKEN_PREFIX) {
        return Ok(None);
    }
    let now = Utc::now();
    let digest = crate::auth::crypto::digest(token);
    let Some(link) = ShareLinkRepo::live_by_token(db, &digest, now).await? else {
        let event = authz::log::event(viewer, "share_link_refused", "not_found");
        authz::log::record(db, event);
        return Ok(None);
    };
    ShareLinkRepo::record_use(db, link.id, now).await?;
    let mut event = authz::log::event(viewer, "share_link_redeemed", "ok");
    event.tree_id = Some(link.tree_id);
    event.share_link_id = Some(link.id);
    authz::log::record(db, event);
    Ok(Some(link))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_trimmed_bounded_and_blank_is_none() {
        assert_eq!(
            checked_label(Some("  Cousins  ".into()))
                .unwrap()
                .as_deref(),
            Some("Cousins")
        );
        assert_eq!(checked_label(Some("   ".into())).unwrap(), None);
        assert!(checked_label(Some("x".repeat(101))).is_err());
        assert!(checked_label(Some("two\nlines".into())).is_err());
    }
}
