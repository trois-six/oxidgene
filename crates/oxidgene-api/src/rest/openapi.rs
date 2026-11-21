//! Serves the REST API description generated from the Axum router at build
//! time: the document of the routes this backend mounts. The operations
//! exchanging paths of the server's own disk are described where they
//! exist, on a backend with local file access, and nowhere else.

use std::sync::OnceLock;

use axum::extract::State;
use axum::http::header;
use axum::response::IntoResponse;

use super::state::AppState;
use crate::embedded;

static SPEC: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/openapi.json.br"));
static LOCAL_FILES_SPEC: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/openapi-local-files.json.br"));
static DECOMPRESSED: OnceLock<Vec<u8>> = OnceLock::new();
static LOCAL_FILES_DECOMPRESSED: OnceLock<Vec<u8>> = OnceLock::new();

/// Return the OpenAPI 3.1 description for the REST surface.
pub async fn spec(State(state): State<AppState>) -> impl IntoResponse {
    let document = if state.local_file_access.is_granted() {
        LOCAL_FILES_DECOMPRESSED.get_or_init(|| embedded::decompress(LOCAL_FILES_SPEC))
    } else {
        DECOMPRESSED.get_or_init(|| embedded::decompress(SPEC))
    };
    (
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        document.as_slice(),
    )
}
