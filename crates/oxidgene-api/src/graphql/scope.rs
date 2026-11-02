//! What every resolver does with the identifiers it receives: parse them, and
//! check that the viewer may do what the root field declares on the tree
//! they are scoped to; and how a root resolver runs its body, from the heap.

use std::future::Future;
use std::sync::Arc;

use async_graphql::{Context, ID, Result};
use futures_util::future::BoxFuture;
use oxidgene_core::OxidGeneError;
use oxidgene_core::access::{Permission, ViewerContext};
use uuid::Uuid;

use super::types::{db_from_ctx, reader_from_ctx};
use crate::authz::{self, Requirement, operations};

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

/// The request's viewer, which the handler put in the request's data:
/// anonymous when there is none.
pub(crate) fn viewer_from_ctx<'a>(ctx: &'a Context<'_>) -> &'a Arc<ViewerContext> {
    static ANONYMOUS: std::sync::LazyLock<Arc<ViewerContext>> =
        std::sync::LazyLock::new(|| Arc::new(ViewerContext::anonymous()));
    ctx.data_opt::<Arc<ViewerContext>>().unwrap_or(&ANONYMOUS)
}

/// The tree `id` names, once the viewer is checked to hold what the root
/// field being resolved declares on it (docs/authorization.md §4).
///
/// GraphQL's counterpart of REST's authorization middleware
/// (`rest::authorize`): every tree-scoped root resolver starts here, so a
/// deleted or unknown tree, or one the viewer cannot see, answers
/// `NOT_FOUND` — never its data, never a write into it — and a permission
/// the viewer lacks answers `FORBIDDEN`. The requirement is the one the
/// field declares in [`operations`], looked up by the field's name, never
/// its alias.
pub(crate) async fn live_tree(ctx: &Context<'_>, id: &ID) -> Result<Uuid> {
    let tree_id = uuid(id)?;
    authorized(ctx, tree_id).await?;
    Ok(tree_id)
}

/// The tree `id` names as [`live_tree`] checks it, `None` where that answers
/// `NOT_FOUND`: a singular query's `null`.
pub(crate) async fn visible_tree(ctx: &Context<'_>, id: &ID) -> Result<Option<Uuid>> {
    let tree_id = uuid(id)?;
    match authorized(ctx, tree_id).await {
        Ok(()) => Ok(Some(tree_id)),
        Err(OxidGeneError::NotFound { .. }) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

async fn authorized(ctx: &Context<'_>, tree_id: Uuid) -> Result<(), OxidGeneError> {
    let field = ctx.field().name();
    let permission = match operations::graphql(field) {
        Some(Requirement::Tree(permission) | Requirement::Trees(permission)) => permission,
        Some(_) => Permission::Read,
        // Fail closed: the API contract guard keeps every field declared.
        None => {
            tracing::error!(
                graphql.field = field,
                "field without a declared requirement refused"
            );
            return Err(OxidGeneError::Forbidden);
        }
    };
    let viewer = viewer_from_ctx(ctx);
    let requirement = Requirement::Tree(permission);
    let checked = authz::check(reader_from_ctx(ctx), viewer, requirement, Some(tree_id)).await;
    #[cfg(feature = "auth")]
    let checked = match checked {
        Ok(()) => editor_write(ctx, field, permission, tree_id).await,
        refused => refused,
    };
    if let Err(error) = checked {
        authz::log::refusal(
            db_from_ctx(ctx),
            viewer,
            Some(tree_id),
            field,
            requirement,
            &error,
        );
        return Err(error);
    }
    Ok(())
}

/// Apply the rules on an Editor's mutations ([`authz::scoped`]), as REST's
/// middleware does: a scoped Editor's change to a record the whole tree
/// shares is `FORBIDDEN` (docs/authorization.md §3.3), and every person its
/// arguments name is checked against the owner's exclusions and the scope
/// (docs/privacy.md §6.4).
#[cfg(feature = "auth")]
async fn editor_write(
    ctx: &Context<'_>,
    field: &str,
    permission: Permission,
    tree_id: Uuid,
) -> Result<(), OxidGeneError> {
    let viewer = viewer_from_ctx(ctx);
    let Some(access) = viewer.cached(tree_id) else {
        return Ok(());
    };
    if !authz::scoped::applies(&access, permission) {
        return Ok(());
    }
    if authz::scoped::refuses_shared(&access) && authz::scoped::refuses_field(field) {
        return Err(OxidGeneError::Forbidden);
    }
    let mut ids = std::collections::HashSet::new();
    let arguments = ctx
        .field()
        .arguments()
        .map_err(|_| OxidGeneError::Validation("unreadable arguments".into()))?;
    for (_, value) in arguments {
        if let Ok(json) = value.into_json() {
            authz::scoped::ids_in_json(&json, &mut ids);
        }
    }
    authz::scoped::check_named(db_from_ctx(ctx), &access, tree_id, &ids).await
}

/// What the request's viewer sees of the records of tree `tree_id`
/// (docs/privacy.md §3.2): what every resolver reading them filters with,
/// nested fields included.
pub(crate) async fn records(
    ctx: &Context<'_>,
    tree_id: Uuid,
) -> Result<crate::authz::records::Records> {
    Ok(crate::authz::records::Records::of(
        reader_from_ctx(ctx),
        super::types::profiles_from_ctx(ctx),
        viewer_from_ctx(ctx),
        tree_id,
    )
    .await?)
}

/// [`records`] of the tree `tree_id` names: a nested field's, whose parent
/// carries it.
pub(crate) async fn records_of(
    ctx: &Context<'_>,
    tree_id: &ID,
) -> Result<crate::authz::records::Records> {
    records(ctx, uuid(tree_id)?).await
}

/// The tree a job's status or cancellation names. The local owner's is not
/// read: a running job answers from memory, without the database, and the
/// service checks the tree when it has to read. Anyone else's goes through
/// [`live_tree`].
pub(crate) async fn job_tree(ctx: &Context<'_>, id: &ID) -> Result<Uuid> {
    if viewer_from_ctx(ctx).is_local() {
        uuid(id)
    } else {
        live_tree(ctx, id).await
    }
}

/// Run a root resolver's body from the heap: every resolver of
/// [`TreeQuery`](super::query::TreeQuery) and
/// [`TreeMutation`](super::mutation::TreeMutation) is
/// `boxed(async move { … }).await`.
///
/// async-graphql dispatches a root field through one generated `match` over
/// every field of the root, and a debug build gives each arm's future its own
/// slot in the frame that polls it: unboxed, that frame grew with the sum of
/// all the resolvers' futures — the whole service call each one awaits — and
/// a mutation needed nearly 2 MiB of stack, all a tokio worker or a test
/// thread has. Boxed, each arm holds a pointer, and the stack holds only the
/// frames of the resolver that runs. The `stack` guard of the API tests keeps
/// it so.
pub(crate) fn boxed<'a, T>(body: impl Future<Output = T> + Send + 'a) -> BoxFuture<'a, T> {
    Box::pin(body)
}
