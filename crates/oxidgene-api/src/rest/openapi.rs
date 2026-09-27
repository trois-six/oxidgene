//! Serves the REST API description generated from the Axum router at build time.

use std::sync::OnceLock;

use axum::http::header;
use axum::response::IntoResponse;

use crate::embedded;

static SPEC: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/openapi.json.br"));
static DECOMPRESSED: OnceLock<Vec<u8>> = OnceLock::new();

/// Return the OpenAPI 3.1 description for the REST surface.
pub async fn spec() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        DECOMPRESSED
            .get_or_init(|| embedded::decompress(SPEC))
            .as_slice(),
    )
}
