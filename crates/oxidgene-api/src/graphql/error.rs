use std::sync::Arc;

use async_graphql::extensions::{Extension, ExtensionContext, ExtensionFactory, NextRequest};
use async_graphql::{ErrorExtensionValues, Response, ServerError};
use oxidgene_core::OxidGeneError;
use tracing::error;
use uuid::Uuid;

use crate::error_contract::{ErrorContract, classify, error_kind};
use crate::request_context::RequestContext;

pub struct SafeErrors;

impl ExtensionFactory for SafeErrors {
    fn create(&self) -> Arc<dyn Extension> {
        Arc::new(SafeErrorsExtension)
    }
}

struct SafeErrorsExtension;

#[async_trait::async_trait]
impl Extension for SafeErrorsExtension {
    async fn request(&self, ctx: &ExtensionContext<'_>, next: NextRequest<'_>) -> Response {
        let mut response = next.run(ctx).await;
        for error in &mut response.errors {
            sanitize(error);
        }
        response
    }
}

fn sanitize(error: &mut ServerError) {
    let contract = error
        .source::<OxidGeneError>()
        .map(classify)
        .unwrap_or_else(|| fallback_contract(error));
    let request_id = contract.unexpected.then(Uuid::now_v7);

    if let Some(request_id) = request_id {
        let request = RequestContext::current();
        error!(
            %request_id,
            error = contract.code,
            error.kind = cause_kind(error),
            http.request.method = request.as_ref().map(|request| request.method.as_str()),
            http.route = request.as_ref().and_then(RequestContext::route),
            "GraphQL request failed"
        );
    }

    error.message = contract.message.to_string();
    let extensions = error
        .extensions
        .get_or_insert_with(ErrorExtensionValues::default);
    extensions.set("code", contract.code.to_ascii_uppercase());
    if let Some(request_id) = request_id {
        extensions.set("requestId", request_id.to_string());
    }
}

/// The bounded category of an unexpected failure's cause: the domain error's
/// kind, or how a blocking task the resolver awaited ended.
fn cause_kind(error: &ServerError) -> &'static str {
    if let Some(error) = error.source::<OxidGeneError>() {
        error_kind(error)
    } else if let Some(join) = error.source::<tokio::task::JoinError>() {
        if join.is_panic() {
            "panic"
        } else {
            "cancelled"
        }
    } else {
        "unclassified"
    }
}

fn fallback_contract(error: &ServerError) -> ErrorContract {
    if error.source::<uuid::Error>().is_some() || error.source.is_none() {
        ErrorContract {
            code: "validation_error",
            message: "The request is invalid",
            unexpected: false,
        }
    } else {
        ErrorContract {
            code: "internal_error",
            message: "The request could not be completed",
            unexpected: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use async_graphql::{Error, Pos};

    use super::*;

    #[test]
    fn domain_errors_use_the_shared_public_contract() {
        let mut error = Error::from(OxidGeneError::Database("private SQL".to_string()))
            .into_server_error(Pos::default());

        sanitize(&mut error);

        assert_eq!(error.message, "The request could not be completed");
        let extensions = error.extensions.expect("error extensions");
        assert_eq!(extensions.get("code"), Some(&"DATABASE_ERROR".into()));
        assert!(extensions.get("requestId").is_some());
    }

    #[test]
    fn validation_errors_have_no_correlation_id() {
        let mut error = Error::from(OxidGeneError::Validation("private value".to_string()))
            .into_server_error(Pos::default());

        sanitize(&mut error);

        assert_eq!(error.message, "The request is invalid");
        let extensions = error.extensions.expect("error extensions");
        assert_eq!(extensions.get("code"), Some(&"VALIDATION_ERROR".into()));
        assert!(extensions.get("requestId").is_none());
    }

    #[tokio::test]
    async fn unexpected_failures_are_logged_with_a_bounded_cause_kind() {
        let database = Error::from(OxidGeneError::Database(
            "Execution Error: error returned from database: (code: 5) database is locked"
                .to_string(),
        ))
        .into_server_error(Pos::default());
        assert_eq!(cause_kind(&database), "busy");

        let panicked = tokio::spawn(async { panic!("private payload") })
            .await
            .expect_err("the task panics");
        let panicked = Error::from(panicked).into_server_error(Pos::default());
        assert_eq!(cause_kind(&panicked), "panic");

        let other = Error::new("private").into_server_error(Pos::default());
        assert_eq!(cause_kind(&other), "unclassified");
    }
}
