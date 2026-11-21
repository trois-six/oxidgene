//! GraphQL API layer: schema construction, Axum handlers, and module declarations.

/// A Relay connection type, its edge, and the conversion from the domain's
/// `Connection` of `$core` nodes — declared once for every paginated list.
macro_rules! connection {
    ($edge:ident, $connection:ident, $gql:ident, $core:ty) => {
        #[derive(Debug, Clone, async_graphql::SimpleObject)]
        pub struct $edge {
            pub cursor: String,
            pub node: $gql,
        }

        #[derive(Debug, Clone, async_graphql::SimpleObject)]
        pub struct $connection {
            pub edges: Vec<$edge>,
            pub page_info: $crate::graphql::types::GqlPageInfo,
            pub total_count: i64,
        }

        impl From<oxidgene_core::types::Connection<$core>> for $connection {
            fn from(c: oxidgene_core::types::Connection<$core>) -> Self {
                Self {
                    edges: c
                        .edges
                        .into_iter()
                        .map(|edge| $edge {
                            cursor: edge.cursor,
                            node: edge.node.into(),
                        })
                        .collect(),
                    page_info: $crate::graphql::types::GqlPageInfo {
                        has_next_page: c.page_info.has_next_page,
                        end_cursor: c.page_info.end_cursor,
                    },
                    total_count: c.total_count,
                }
            }
        }
    };
}

#[cfg(feature = "auth")]
pub mod account;
mod authorize;
mod error;
#[cfg(feature = "auth")]
pub mod exclusion;
pub mod history;
pub mod inputs;
mod loaders;
#[cfg(feature = "auth")]
pub mod member;
pub mod mutation;
#[cfg(feature = "auth")]
pub mod notification;
pub mod query;
mod scope;
#[cfg(feature = "auth")]
pub mod sharing;
mod tracing;
pub mod types;

use crate::media::MediaStore;
use crate::profile::ProfileService;
use crate::service::archive::ArchivePortals;
use crate::service::purge::PurgeQueue;
use async_graphql::{EmptySubscription, Schema, http::GraphiQLSource};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::extract::State;
use axum::response::{Html, IntoResponse};
use oxidgene_db::repo::Connections;
use std::sync::Arc;

use crate::workdir::WorkDir;

use self::authorize::Authorize;
use self::error::SafeErrors;
use self::tracing::Tracing;

const MAX_QUERY_DEPTH: usize = 16;
/// The most a query may weigh: one for each field it selects, and a
/// paginated field's selection once per [`ROWS_PER_UNIT`] rows of its page
/// ([`page_complexity`]).
const MAX_QUERY_COMPLEXITY: usize = 1_000;
const MAX_RECURSIVE_DEPTH: usize = 32;

/// How many rows of a page weigh as one more selection of its fields.
const ROWS_PER_UNIT: u64 = 10;

/// What a paginated field weighs: itself, and its selection once per ten
/// rows its page may return — `rows` as asked (`first` or `limit`), the
/// default page when it is not, at most the rows a page holds — and never
/// less than once.
///
/// Counted once, the fields of a page weighed the same for one row and for
/// a hundred, each of which resolves them again: a query a hundred rows
/// wide cost what a single record did. Every root field taking `first` or
/// `limit` declares this as its complexity. A page of ten rows or fewer
/// weighs what it always did, so nothing that was refused is accepted.
pub(crate) fn page_complexity<N: TryInto<u64>>(rows: Option<N>, child_complexity: usize) -> usize {
    use oxidgene_db::repo::PaginationParams;

    // The repositories' own default and bound on a page.
    let asked = rows.and_then(|rows| rows.try_into().ok());
    let page = PaginationParams {
        first: asked.unwrap_or(PaginationParams::default().first),
        after: None,
    };
    let rows = page.clamped_first().max(ROWS_PER_UNIT);
    let weight = u64::try_from(child_complexity)
        .unwrap_or(u64::MAX)
        .saturating_mul(rows)
        .div_ceil(ROWS_PER_UNIT);
    usize::try_from(weight)
        .unwrap_or(usize::MAX)
        .saturating_add(1)
}

/// The root query type: the tree queries and, in the web build, the
/// account queries.
#[derive(async_graphql::MergedObject, Default)]
#[graphql(name = "QueryRoot")]
pub struct QueryRoot(
    query::TreeQuery,
    #[cfg(feature = "auth")] account::AccountQuery,
    #[cfg(feature = "auth")] exclusion::ExclusionQuery,
    #[cfg(feature = "auth")] member::MemberQuery,
    #[cfg(feature = "auth")] notification::NotificationQuery,
    #[cfg(feature = "auth")] sharing::SharingQuery,
);

/// The root mutation type: the tree mutations and, in the web build, the
/// account mutations.
#[derive(async_graphql::MergedObject, Default)]
#[graphql(name = "MutationRoot")]
pub struct MutationRoot(
    mutation::TreeMutation,
    #[cfg(feature = "auth")] account::AccountMutation,
    #[cfg(feature = "auth")] exclusion::ExclusionMutation,
    #[cfg(feature = "auth")] member::MemberMutation,
    #[cfg(feature = "auth")] notification::NotificationMutation,
    #[cfg(feature = "auth")] sharing::SharingMutation,
);

/// The full GraphQL schema type.
pub type OxidGeneSchema = Schema<QueryRoot, MutationRoot, EmptySubscription>;

/// Build the async-graphql schema with the given database connections,
/// profile service, purge queue and media store.
///
/// Its requests run through [`graphql_handler`], which gives each the
/// batcher its nested fields read through: executed without it, a nested
/// field answers an error.
pub fn build_schema(
    db: impl Into<Connections>,
    profiles: Arc<ProfileService>,
    purge: PurgeQueue,
    media: Arc<dyn MediaStore>,
) -> OxidGeneSchema {
    build_schema_on(
        db.into(),
        profiles,
        purge,
        media,
        WorkDir::temporary(),
        Arc::new(ArchivePortals::native()),
        crate::limits::IntakeLimits::default(),
    )
}

/// Queries read through the reader ([`types::reader_from_ctx`]), mutations
/// write through the writer ([`types::db_from_ctx`]).
///
/// The schema has no operation exchanging a path of the server's own disk:
/// those are the desktop's, which serves REST alone (docs/api.md, Surfaces
/// and parity).
pub(crate) fn build_schema_on(
    db: Connections,
    profiles: Arc<ProfileService>,
    purge: PurgeQueue,
    media: Arc<dyn MediaStore>,
    work_dir: WorkDir,
    archives: Arc<ArchivePortals>,
    limits: crate::limits::IntakeLimits,
) -> OxidGeneSchema {
    Schema::build(
        QueryRoot::default(),
        MutationRoot::default(),
        EmptySubscription,
    )
    .limit_depth(MAX_QUERY_DEPTH)
    .limit_complexity(MAX_QUERY_COMPLEXITY)
    .limit_recursive_depth(MAX_RECURSIVE_DEPTH)
    .extension(Tracing)
    .extension(SafeErrors)
    .extension(Authorize)
    .data(db.writer)
    .data(types::Reader(db.reader))
    .data(profiles)
    .data(purge)
    .data(media)
    .data(work_dir)
    .data(archives)
    .data(limits)
    .finish()
}

/// Axum handler for `POST /graphql`: the nested fields of the response
/// are read in batches ([`loaders`]).
///
/// The request's viewer, built from the principal the authentication layer
/// left in the request's extensions — the local owner in a build without
/// authentication — and, in the web build, the server's authentication
/// become data of the GraphQL request: resolvers act for the same viewer
/// REST would.
pub async fn graphql_handler(
    State(schema): State<OxidGeneSchema>,
    #[cfg(feature = "auth")] auth: Option<axum::Extension<Arc<crate::auth::Auth>>>,
    extensions: axum::http::Extensions,
    req: GraphQLRequest,
) -> GraphQLResponse {
    let viewer = crate::rest::authorize::viewer_of(&extensions);
    let request = req.into_inner().data(viewer);
    #[cfg(feature = "auth")]
    let request = match auth {
        Some(axum::Extension(auth)) => request.data(auth),
        None => request,
    };
    loaders::execute(&schema, request).await.into()
}

/// Marks a router whose `GET /graphql` serves GraphiQL: add it as an
/// [`axum::Extension`] layer.
///
/// Off unless a deployment asks for it: the GraphiQL page loads its scripts
/// and styles from a public CDN, so serving it makes every visitor's browser
/// call a third party.
#[derive(Clone, Copy, Debug)]
pub struct GraphiQl;

/// Axum handler for `GET /graphql` — serves the GraphiQL playground where
/// [`GraphiQl`] enables it, and is an unknown route everywhere else.
pub async fn graphql_playground(
    enabled: Option<axum::Extension<GraphiQl>>,
) -> axum::response::Response {
    if enabled.is_none() {
        return crate::rest::error::unknown_route().await;
    }
    Html(GraphiQLSource::build().endpoint("/graphql").finish()).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_weighs_its_selection_once_per_ten_rows_and_never_less_than_once() {
        // Ten rows or fewer: the selection and the field, as before.
        for rows in [Some(1_u64), Some(10)] {
            assert_eq!(page_complexity(rows, 40), 41);
        }
        // The default page, and a full one.
        assert_eq!(page_complexity(None::<u64>, 40), 101);
        assert_eq!(page_complexity(Some(100_u64), 40), 401);
        // More than a page holds weighs as a full page; a negative count
        // as the default.
        assert_eq!(page_complexity(Some(5_000_usize), 40), 401);
        assert_eq!(page_complexity(Some(-3_i32), 40), 101);
        // However large the selection, the weight neither wraps nor
        // falls under the limit.
        assert!(page_complexity(Some(100_u64), usize::MAX) > MAX_QUERY_COMPLEXITY);
    }
}
