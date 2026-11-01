//! The authentication layer (docs/authentication.md §4.8): one middleware in
//! front of the REST routes, GraphQL and the direct media reads, so that the
//! three see the same [`Principal`].
//!
//! - `Authorization: Bearer` is validated as an access token; the cookie is
//!   then ignored and no CSRF token is needed.
//! - Otherwise the `__Host-oxidgene` cookie names a session, whose access
//!   token is refreshed and validated; a state-changing request must carry
//!   its CSRF token and, when it sends one, the public origin.
//! - Otherwise the request is anonymous.
//!
//! An invalid, expired or revoked credential is refused here, before any
//! handler (`401`). A request without one goes on as anonymous: each
//! operation's requirement decides what it may do (docs/authorization.md
//! §4.3), REST's in `rest::authorize`, GraphQL's per root field.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{HeaderMap, Method, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use super::Auth;
use super::cookie;
use super::identity::Principal;
use super::session::Refusal;
use crate::AppState;

/// Resolve the request's principal and hand it on, or refuse it.
pub(crate) async fn authenticate(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let auth = Arc::clone(&state.auth);
    // Borrowing the request itself across an await would make this future
    // `!Send`: its body is not `Sync`.
    let (head, body) = request.into_parts();
    let principal = principal(&state, &auth, &head.headers, &head.method).await;
    let mut request = Request::from_parts(head, body);
    let principal = match principal {
        Ok(principal) => principal,
        Err(refusal) => return refusal.into_response(),
    };
    if let Some(client_id) = principal.client_id() {
        tracing::Span::current().record("oxidgene.client_id", client_id);
    }
    request.extensions_mut().insert(principal);
    request.extensions_mut().insert(auth);
    next.run(request).await
}

/// The principal of `request`.
async fn principal(
    state: &AppState,
    auth: &Auth,
    headers: &HeaderMap,
    method: &Method,
) -> Result<Principal, Refusal> {
    let Some(oidc) = auth.oidc() else {
        return Ok(Principal::Local);
    };
    if let Some(token) = bearer(headers) {
        return oidc.bearer_principal(&state.reader, token).await;
    }
    let Some(cookie) = cookie::read(headers, cookie::SESSION) else {
        return Ok(Principal::Anonymous);
    };
    let writes = !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS);
    oidc.resolve_session(&state.connections(), &cookie, writes.then_some(headers))
        .await
        .map(|resolved| resolved.principal)
}

/// The token of an `Authorization: Bearer` header.
fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|token| !token.is_empty())
}
