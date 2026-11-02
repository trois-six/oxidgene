//! Authorization (docs/authorization.md): the one module that decides what
//! a request's viewer may do, for REST, GraphQL and MCP alike.
//!
//! - Every operation declares a [`Requirement`] in [`operations`]: the REST
//!   routes by method and template, the GraphQL root fields by name. A guard
//!   keeps both tables complete and the two surfaces' declarations equal.
//! - [`tree_access`] resolves the viewer's [`TreeAccess`] to a tree once per
//!   request, and [`authorize`] checks a permission against it.
//! - Refusals are answered `401`, `403` or `404` as §4.3 says and recorded
//!   in the access log ([`log`]).
//!
//! Every build compiles it: without the `auth` feature, the viewer is always
//! the local owner, who passes every requirement on every live tree, as the
//! desktop always did.

pub mod log;
pub mod operations;
pub mod records;
pub mod redaction;
#[cfg(feature = "auth")]
pub mod scoped;

use chrono::Utc;
use oxidgene_core::OxidGeneError;
use oxidgene_core::access::{Identity, JobActor};
use oxidgene_core::access::{Permission, Principal, TreeAccess, TreeRole, ViewerContext};
use oxidgene_db::repo::{AppUserRepo, UserStatus};
use oxidgene_db::repo::{MembershipRepo, ShareLinkRepo, TreeRepo};
use sea_orm::{ConnectionTrait, DatabaseConnection};
use uuid::Uuid;

use crate::service::scope::require_live_tree;

/// What an operation needs of its viewer (docs/authorization.md §3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Requirement {
    /// Nothing: operations that need no tree data — the API description, the
    /// reference content, the export destinations.
    Open,
    /// A signed-in user, or the local owner.
    SignedIn,
    /// The instance's tree creation right.
    TreeCreation,
    /// An instance administrator. The local owner passes, and the account
    /// operations answer it `404` as having no account.
    Administrator,
    /// `permission` on the tree the operation names.
    Tree(Permission),
    /// `permission` on each of the trees the operation lists: a tree the
    /// viewer may not read is left out, as a missing one is.
    Trees(Permission),
}

impl Requirement {
    /// The part of the requirement the viewer alone decides, before any
    /// tree is read.
    ///
    /// A bearer token's scopes bound it (§7.2): a permission on a tree takes
    /// the permission's scope, an administrator's operation
    /// `instance:admin`.
    pub fn check_viewer(self, viewer: &ViewerContext) -> Result<(), OxidGeneError> {
        let principal = viewer.principal();
        match self {
            Self::Open => Ok(()),
            // A `public` tree, or one a share link opens, has anonymous
            // Visitors (§6.3, §6.4): the tree decides.
            Self::Tree(permission) | Self::Trees(permission)
                if !viewer.token_grants(permission.oauth_scope()) =>
            {
                Err(OxidGeneError::Forbidden)
            }
            Self::Tree(_) | Self::Trees(_) => Ok(()),
            _ if matches!(principal, Principal::Anonymous) => Err(OxidGeneError::Unauthenticated),
            Self::TreeCreation if !viewer.may_create_trees() => Err(OxidGeneError::Forbidden),
            Self::Administrator
                if !viewer.is_local()
                    && (!viewer.is_admin() || !viewer.token_grants("instance:admin")) =>
            {
                Err(OxidGeneError::Forbidden)
            }
            _ => Ok(()),
        }
    }

    /// The permission refused when this requirement fails, for the access
    /// log.
    pub fn permission(self) -> Option<Permission> {
        match self {
            Self::Tree(permission) | Self::Trees(permission) => Some(permission),
            _ => None,
        }
    }

    /// Its stable name, for the access log and the specification.
    pub fn name(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::SignedIn => "signed_in",
            Self::TreeCreation => "tree_creation",
            Self::Administrator => "administrator",
            Self::Tree(permission) | Self::Trees(permission) => permission.as_str(),
        }
    }
}

/// The viewer's access to tree `tree_id`, resolved once per request
/// (§4.1): the local owner's on any live tree; otherwise, in order, the
/// viewer's live membership with its scope, then the grants of the share
/// links their session holds, then the tree's visibility — `public` makes
/// anyone a Visitor, `site` any signed-in user. Every grant a membership
/// outranks; among Visitors' grants, one without a scope wins.
///
/// A viewer who cannot see the tree at all — missing, deleted, or not
/// theirs — is answered as if it did not exist (`not_found`); an anonymous
/// one is asked to sign in (`unauthenticated`), whether the tree exists or
/// not.
pub async fn tree_access(
    db: &impl ConnectionTrait,
    viewer: &ViewerContext,
    tree_id: Uuid,
) -> Result<TreeAccess, OxidGeneError> {
    if let Some(access) = viewer.cached(tree_id) {
        return Ok(access);
    }
    let now = Utc::now();
    let access = match viewer.principal() {
        Principal::Local => {
            require_live_tree(db, tree_id).await?;
            TreeAccess::of_role(TreeRole::Owner)
        }
        principal => {
            let member = match principal.user() {
                Some(identity) => {
                    MembershipRepo::live_access(db, tree_id, identity.user_id, now).await?
                }
                None => None,
            };
            match member {
                // A Guest exports where the tree lets its Guests export
                // (Privacy §9): the setting is read for a Guest only.
                Some((TreeRole::Guest, scope_id)) => {
                    let tree = oxidgene_db::repo::TreeRepo::get(db, tree_id).await?;
                    TreeAccess::of_role(TreeRole::Guest)
                        .with_guest_export(tree.privacy.guests_may_export)
                        .scoped(scope_id)
                }
                Some((role, scope_id)) => TreeAccess::of_role(role).scoped(scope_id),
                None => match visitor_access(db, viewer, tree_id, now).await? {
                    Some(access) => access,
                    None if viewer.is_anonymous() => return Err(OxidGeneError::Unauthenticated),
                    None => return Err(not_found(tree_id)),
                },
            }
        }
    };
    viewer.remember(tree_id, access);
    Ok(access)
}

/// The Visitor access the viewer holds on tree `tree_id` without a
/// membership (§6.3, §6.4): from a live share link their session holds,
/// scoped as the link is, or from the tree's visibility, scoped by its
/// visitor scope; the one without a scope when both are. None on a
/// missing or deleted tree.
async fn visitor_access(
    db: &impl ConnectionTrait,
    viewer: &ViewerContext,
    tree_id: Uuid,
    now: chrono::DateTime<Utc>,
) -> Result<Option<TreeAccess>, OxidGeneError> {
    let Some((visibility, visitor_scope)) = TreeRepo::sharing(db, tree_id).await? else {
        return Ok(None);
    };
    let visitor = TreeAccess::of_role(TreeRole::Visitor);
    let mut best = visibility
        .admits(viewer.user().is_some())
        .then(|| visitor.scoped(visitor_scope));
    for link in ShareLinkRepo::live_of_tree(db, tree_id, viewer.share_links(), now).await? {
        let access = TreeAccess {
            share_link_id: Some(link.id),
            ..visitor.scoped(link.scope_id)
        };
        if best.is_none_or(|best| access.outranks(&best)) {
            best = Some(access);
        }
    }
    Ok(best)
}

/// The trees the viewer's session holds a live share link to: what the
/// tree list adds to a user's memberships (§6.4).
pub async fn link_trees(
    db: &impl ConnectionTrait,
    viewer: &ViewerContext,
) -> Result<Vec<Uuid>, OxidGeneError> {
    let mut trees: Vec<Uuid> = ShareLinkRepo::live_among(db, viewer.share_links(), Utc::now())
        .await?
        .into_iter()
        .map(|link| link.tree_id)
        .collect();
    trees.sort_unstable();
    trees.dedup();
    Ok(trees)
}

/// Check that the viewer holds `permission` on tree `tree_id`: `403
/// forbidden` for a viewer who sees the tree without it, or whose bearer
/// token lacks the permission's scope (§7.2).
///
/// Duplicating a tree also takes the instance's tree creation right.
pub async fn authorize(
    db: &impl ConnectionTrait,
    viewer: &ViewerContext,
    tree_id: Uuid,
    permission: Permission,
) -> Result<TreeAccess, OxidGeneError> {
    let access = tree_access(db, viewer, tree_id).await?;
    if !access.allows(permission)
        || !viewer.token_grants(permission.oauth_scope())
        || (permission == Permission::TreeDuplicate && !viewer.may_create_trees())
    {
        return Err(OxidGeneError::Forbidden);
    }
    Ok(access)
}

/// The trees of `tree_ids` the viewer holds `permission` on, in the order
/// given; the others are left out, as missing trees are. The local owner
/// holds every permission on every tree: the operation leaves out the
/// missing ones itself. A user's memberships are read in one query; the
/// trees they are no member of, and every tree of an anonymous viewer, as
/// a Visitor would see them (§6.3, §6.4).
pub async fn permitted_trees(
    db: &DatabaseConnection,
    viewer: &ViewerContext,
    tree_ids: &[Uuid],
    permission: Permission,
) -> Result<Vec<Uuid>, OxidGeneError> {
    if viewer.is_local() {
        return Ok(tree_ids.to_vec());
    }
    let now = Utc::now();
    let roles = match viewer.user() {
        Some(identity) => MembershipRepo::live_roles(db, identity.user_id, tree_ids, now).await?,
        None => std::collections::HashMap::new(),
    };
    let mut permitted = Vec::new();
    for &tree_id in tree_ids {
        let access = match roles.get(&tree_id) {
            Some(&(role, scope)) => Some(TreeAccess::of_role(role).scoped(scope)),
            None => visitor_access(db, viewer, tree_id, now).await?,
        };
        if let Some(access) = access {
            viewer.remember(tree_id, access);
            if access.allows(permission) {
                permitted.push(tree_id);
            }
        }
    }
    Ok(permitted)
}

/// Check `requirement` for the viewer, on tree `tree_id` when it names one.
pub async fn check(
    db: &DatabaseConnection,
    viewer: &ViewerContext,
    requirement: Requirement,
    tree_id: Option<Uuid>,
) -> Result<(), OxidGeneError> {
    requirement.check_viewer(viewer)?;
    if let (Requirement::Tree(permission), Some(tree_id)) = (requirement, tree_id) {
        authorize(db, viewer, tree_id, permission).await?;
    }
    Ok(())
}

/// The viewer a background job acts for: the actor recorded when it was
/// submitted (docs/authentication.md §5.5), read again from the user row
/// so that a deactivated or erased user acts no more. An actor without a
/// user is the local owner only when it says so.
///
/// # Errors
///
/// `actor_revoked` ([`OxidGeneError::Forbidden`]) for a user who may no
/// longer act, and for an actor who is neither a user nor the local owner.
pub async fn job_viewer(
    db: &impl ConnectionTrait,
    actor: &JobActor,
) -> Result<ViewerContext, OxidGeneError> {
    let Some(user_id) = actor.user_id else {
        return if actor.is_local() {
            Ok(ViewerContext::local())
        } else {
            Err(OxidGeneError::Forbidden)
        };
    };
    let user = match AppUserRepo::get(db, user_id).await {
        Ok(user) => user,
        Err(OxidGeneError::NotFound { .. }) => return Err(OxidGeneError::Forbidden),
        Err(error) => return Err(error),
    };
    if user.status != UserStatus::Active {
        return Err(OxidGeneError::Forbidden);
    }
    Ok(ViewerContext::new(Principal::User(Identity {
        issuer: actor.issuer.clone().unwrap_or_default(),
        subject: actor.subject.clone().unwrap_or_default(),
        user_id,
        client_id: actor.client_id.clone(),
        scopes: actor.scopes.clone(),
        session_id: actor.session.clone(),
        api_client_id: actor.api_client_id,
        instance_role: user.instance_role,
        can_create_trees: user.can_create_trees,
    })))
}

/// Check, before a job starts and again before it commits, that its actor
/// still holds `permission` on its tree: the role is read afresh, whatever
/// the request resolved earlier. A lost membership or permission is
/// `actor_revoked` ([`OxidGeneError::Forbidden`]), and so is a deleted tree
/// for a user, who may not tell it from a lost membership.
pub async fn recheck_job(
    db: &impl ConnectionTrait,
    actor: &JobActor,
    tree_id: Uuid,
    permission: Permission,
) -> Result<ViewerContext, OxidGeneError> {
    let viewer = job_viewer(db, actor).await?;
    match authorize(db, &viewer, tree_id, permission).await {
        Ok(_) => Ok(viewer),
        Err(OxidGeneError::NotFound { .. }) if !viewer.is_local() => Err(OxidGeneError::Forbidden),
        Err(error) => Err(error),
    }
}

/// The answer to a tree the viewer may not see.
pub fn not_found(tree_id: Uuid) -> OxidGeneError {
    OxidGeneError::NotFound {
        entity: "Tree",
        id: tree_id,
    }
}

/// Whether `error` is a refusal of the policy rather than a failure.
pub fn is_refusal(error: &OxidGeneError) -> bool {
    matches!(
        error,
        OxidGeneError::Unauthenticated | OxidGeneError::Forbidden | OxidGeneError::NotFound { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A job submitted by nobody acts for nobody: only an actor saying it is
    /// the local owner acts as the owner of every tree.
    #[tokio::test]
    async fn only_the_local_owners_jobs_act_as_the_local_owner() {
        let db = oxidgene_db::repo::connect("sqlite::memory:")
            .await
            .expect("opens a database");
        let local = job_viewer(&db, &ViewerContext::local().job_actor())
            .await
            .expect("the local owner acts");
        assert!(local.is_local());
        for actor in [ViewerContext::anonymous().job_actor(), JobActor::default()] {
            assert!(matches!(
                job_viewer(&db, &actor).await,
                Err(OxidGeneError::Forbidden)
            ));
        }
    }
}
