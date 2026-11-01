//! The requirement every GraphQL root field declares, checked before its
//! resolver runs (docs/authorization.md §4.3).
//!
//! The viewer's part is checked here, for every root field by its name —
//! never its alias — in [`operations`]: a field the anonymous viewer may not
//! use answers `UNAUTHENTICATED`, one needing the tree creation right or the
//! administrator role answers `FORBIDDEN` without it, and a field nobody
//! declared is refused. The tree a field names is checked by its resolver,
//! which knows its arguments ([`super::scope::live_tree`]).

use std::sync::Arc;

use async_graphql::extensions::{
    Extension, ExtensionContext, ExtensionFactory, NextResolve, ResolveInfo,
};
use async_graphql::{PathSegment, Pos, ServerResult, Value};
use oxidgene_core::OxidGeneError;
use oxidgene_core::access::ViewerContext;
use sea_orm::DatabaseConnection;

use crate::authz::{self, operations};

pub struct Authorize;

impl ExtensionFactory for Authorize {
    fn create(&self) -> Arc<dyn Extension> {
        Arc::new(AuthorizeExtension)
    }
}

struct AuthorizeExtension;

#[async_trait::async_trait]
impl Extension for AuthorizeExtension {
    async fn resolve(
        &self,
        ctx: &ExtensionContext<'_>,
        info: ResolveInfo<'_>,
        next: NextResolve<'_>,
    ) -> ServerResult<Option<Value>> {
        let root = info.path_node.parent.is_none();
        if !root || info.is_for_introspection || info.name.starts_with("__") {
            return next.run(ctx, info).await;
        }
        let anonymous;
        let viewer = match ctx.data_opt::<Arc<ViewerContext>>() {
            Some(viewer) => viewer,
            None => {
                anonymous = Arc::new(ViewerContext::anonymous());
                &anonymous
            }
        };
        let checked = match operations::graphql(info.name) {
            Some(requirement) => requirement.check_viewer(viewer).inspect_err(|error| {
                if let Some(db) = ctx.data_opt::<DatabaseConnection>() {
                    authz::log::refusal(db, viewer, None, info.name, requirement, error);
                }
            }),
            None => {
                tracing::error!(
                    graphql.field = info.name,
                    "field without a declared requirement refused"
                );
                Err(OxidGeneError::Forbidden)
            }
        };
        if let Err(error) = checked {
            let key = info.alias.unwrap_or(info.name).to_string();
            let mut error = async_graphql::Error::from(error).into_server_error(Pos::default());
            error.path = vec![PathSegment::Field(key)];
            return Err(error);
        }
        next.run(ctx, info).await
    }
}
