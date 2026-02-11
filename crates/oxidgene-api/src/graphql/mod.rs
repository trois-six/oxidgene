//! GraphQL API layer: schema construction, Axum handlers, and module declarations.

pub mod inputs;
pub mod mutation;
pub mod query;
pub mod types;

use async_graphql::{EmptySubscription, Schema, http::GraphiQLSource};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::extract::State;
use axum::response::{Html, IntoResponse};
use sea_orm::DatabaseConnection;

use mutation::MutationRoot;
use query::QueryRoot;

/// The full GraphQL schema type.
pub type OxidGeneSchema = Schema<QueryRoot, MutationRoot, EmptySubscription>;

/// Build the async-graphql schema with the given database connection.
pub fn build_schema(db: DatabaseConnection) -> OxidGeneSchema {
    Schema::build(QueryRoot, MutationRoot, EmptySubscription)
        .data(db)
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
