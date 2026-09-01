//! GraphQL API layer: schema construction, Axum handlers, and module declarations.

mod error;
pub mod inputs;
pub mod mutation;
pub mod query;
mod tracing;
pub mod types;

use crate::media::MediaStore;
use crate::profile::ProfileService;
use crate::rest::state::LocalFileAccess;
use crate::service::purge::PurgeQueue;
use async_graphql::{EmptySubscription, Schema, http::GraphiQLSource};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::extract::State;
use axum::response::{Html, IntoResponse};
use sea_orm::DatabaseConnection;
use std::sync::Arc;

use mutation::MutationRoot;
use query::QueryRoot;

use self::error::SafeErrors;
use self::tracing::Tracing;

const MAX_QUERY_DEPTH: usize = 16;
const MAX_QUERY_COMPLEXITY: usize = 1_000;
const MAX_RECURSIVE_DEPTH: usize = 32;

/// The full GraphQL schema type.
pub type OxidGeneSchema = Schema<QueryRoot, MutationRoot, EmptySubscription>;

/// Build the async-graphql schema with the given database connection, profile
/// service, purge queue and media store.
pub fn build_schema(
    db: DatabaseConnection,
    profiles: Arc<ProfileService>,
    purge: PurgeQueue,
    media: Arc<dyn MediaStore>,
) -> OxidGeneSchema {
    build_schema_with_local_file_access(db, profiles, purge, media, LocalFileAccess(false))
}

pub(crate) fn build_schema_with_local_file_access(
    db: DatabaseConnection,
    profiles: Arc<ProfileService>,
    purge: PurgeQueue,
    media: Arc<dyn MediaStore>,
    local_file_access: LocalFileAccess,
) -> OxidGeneSchema {
    Schema::build(QueryRoot, MutationRoot, EmptySubscription)
        .limit_depth(MAX_QUERY_DEPTH)
        .limit_complexity(MAX_QUERY_COMPLEXITY)
        .limit_recursive_depth(MAX_RECURSIVE_DEPTH)
        .extension(Tracing)
        .extension(SafeErrors)
        .data(db)
        .data(profiles)
        .data(purge)
        .data(media)
        .data(local_file_access)
        .finish()
}

/// Axum handler for `POST /graphql`.
pub async fn graphql_handler(
    State(schema): State<OxidGeneSchema>,
    req: GraphQLRequest,
) -> GraphQLResponse {
    schema.execute(req.into_inner()).await.into()
}

/// Axum handler for `GET /graphql` — serves GraphiQL playground.
pub async fn graphql_playground() -> impl IntoResponse {
    Html(GraphiQLSource::build().endpoint("/graphql").finish())
}
