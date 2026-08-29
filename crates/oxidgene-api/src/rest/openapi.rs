//! Serves the REST API description generated from the Axum router at build time.

use axum::http::header;
use axum::response::IntoResponse;

const SPEC: &str = include_str!(concat!(env!("OUT_DIR"), "/openapi.json"));

/// Return the OpenAPI 3.1 description for the REST surface.
pub async fn spec() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        SPEC,
    )
}
