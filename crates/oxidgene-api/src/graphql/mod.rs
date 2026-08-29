//! GraphQL API layer: schema construction, Axum handlers, and module declarations.

mod error;
pub mod inputs;
pub mod mutation;
pub mod query;
pub mod types;

use crate::media::MediaStore;
use crate::profile::ProfileService;
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
    Schema::build(QueryRoot, MutationRoot, EmptySubscription)
        .extension(SafeErrors)
        .data(db)
        .data(profiles)
        .data(purge)
        .data(media)
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
