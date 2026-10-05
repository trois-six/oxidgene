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

mod error;
pub mod history;
pub mod inputs;
mod loaders;
pub mod mutation;
pub mod query;
mod scope;
mod tracing;
pub mod types;

use crate::media::MediaStore;
use crate::profile::ProfileService;
use crate::rest::state::LocalFileAccess;
use crate::service::archive::ArchivePortals;
use crate::service::purge::PurgeQueue;
use async_graphql::{EmptySubscription, Schema, http::GraphiQLSource};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::extract::State;
use axum::response::{Html, IntoResponse};
use oxidgene_db::repo::Connections;
use std::sync::Arc;

use mutation::MutationRoot;

use crate::workdir::WorkDir;
use query::QueryRoot;

use self::error::SafeErrors;
use self::tracing::Tracing;

const MAX_QUERY_DEPTH: usize = 16;
const MAX_QUERY_COMPLEXITY: usize = 1_000;
const MAX_RECURSIVE_DEPTH: usize = 32;

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
    build_schema_with_local_file_access(
        db.into(),
        profiles,
        purge,
        media,
        WorkDir::temporary(),
        Arc::new(ArchivePortals::native()),
        LocalFileAccess(false),
    )
}

/// Queries read through the reader ([`types::reader_from_ctx`]), mutations
/// write through the writer ([`types::db_from_ctx`]).
pub(crate) fn build_schema_with_local_file_access(
    db: Connections,
    profiles: Arc<ProfileService>,
    purge: PurgeQueue,
    media: Arc<dyn MediaStore>,
    work_dir: WorkDir,
    archives: Arc<ArchivePortals>,
    local_file_access: LocalFileAccess,
) -> OxidGeneSchema {
    Schema::build(QueryRoot, MutationRoot, EmptySubscription)
        .limit_depth(MAX_QUERY_DEPTH)
        .limit_complexity(MAX_QUERY_COMPLEXITY)
        .limit_recursive_depth(MAX_RECURSIVE_DEPTH)
        .extension(Tracing)
        .extension(SafeErrors)
        .data(db.writer)
        .data(types::Reader(db.reader))
        .data(profiles)
        .data(purge)
        .data(media)
        .data(work_dir)
        .data(archives)
        .data(local_file_access)
        .finish()
}

/// Axum handler for `POST /graphql`: the nested fields of the response
/// are read in batches ([`loaders`]).
pub async fn graphql_handler(
    State(schema): State<OxidGeneSchema>,
    req: GraphQLRequest,
) -> GraphQLResponse {
    loaders::execute(&schema, req.into_inner()).await.into()
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
