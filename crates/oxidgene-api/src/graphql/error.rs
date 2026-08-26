use std::sync::Arc;

use async_graphql::extensions::{Extension, ExtensionContext, ExtensionFactory, NextRequest};
use async_graphql::{ErrorExtensionValues, Response, ServerError};
use oxidgene_core::OxidGeneError;
use tracing::error;
use uuid::Uuid;

use crate::error_contract::{ErrorContract, classify};

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
        error!(%request_id, error = contract.code, "GraphQL request failed");
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
}
