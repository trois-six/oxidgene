//! The authorization of every REST request (docs/authorization.md §4): one
//! middleware on every route, after routing, that
//!
//! - builds the request's [`ViewerContext`] from the principal the
//!   authentication layer resolved — the local owner on the desktop and in
//!   the `none` mode;
//! - finds the route's declared [`Requirement`]
//!   ([`crate::authz::operations`]) and checks it, the tree the path names
//!   included, answering `401`, `403` or `404` before any handler runs;
//! - hands the viewer to the handler ([`Viewer`]).
//!
//! The tree the path names must also exist and not be deleted: tree
//! deletion is asynchronous, and a deleted tree's rows remain until the
//! background purge removes them, so without this check its children would
//! stay readable meanwhile, and a request naming a tree that never existed
//! would get a `200` with empty results rather than a `404`. One indexed
//! read per request: the membership's, the tree's liveness joined in, or
//! the tree's own for the local owner.
//!
//! Two requests of the local owner skip that read: polling the status of a
//! job this process knows in memory — it runs it, or holds a cancellation
//! for it — and cancelling such a job. The job holds the SQLite writer for
//! its whole write transaction, which on an in-memory database is the only
//! connection, readers included; the status and cancel handlers answer from
//! memory precisely so that they do not wait for it. That answer carries
//! only the job's own phase and counters; once the job ends, the poll goes
//! through the read again, and a tree deleted meanwhile answers `404`. A
//! signed-in user's poll always reads their membership.

use std::sync::Arc;

use axum::extract::{FromRequestParts, MatchedPath, Request, State};
use axum::http::Method;
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use oxidgene_core::OxidGeneError;
use oxidgene_core::access::ViewerContext;
use oxidgene_db::repo::BackgroundJobKind;
use uuid::Uuid;

use super::error::ApiError;
use super::state::AppState;
use crate::authz::{self, Requirement, operations};
use crate::service::background_job::live_state;

/// The viewer of a REST request, which the authorization middleware hands
/// every handler. A request it did not see is anonymous: absent means no
/// credential, never full access.
#[derive(Debug, Clone)]
pub struct Viewer(pub Arc<ViewerContext>);

impl std::ops::Deref for Viewer {
    type Target = ViewerContext;

    fn deref(&self) -> &ViewerContext {
        &self.0
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Viewer {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(parts
            .extensions
            .get::<Self>()
            .cloned()
            .unwrap_or_else(|| Self(Arc::new(ViewerContext::anonymous()))))
    }
}

/// The viewer of the request whose extensions are `extensions`: the
/// principal the authentication layer left there, anonymous without one;
/// the local owner in a build without authentication.
pub fn viewer_of(extensions: &axum::http::Extensions) -> Arc<ViewerContext> {
    #[cfg(feature = "auth")]
    {
        let principal = extensions
            .get::<oxidgene_core::access::Principal>()
            .cloned()
            .unwrap_or(oxidgene_core::access::Principal::Anonymous);
        let links = extensions
            .get::<crate::auth::layer::ShareGrants>()
            .map(|grants| grants.0.clone())
            .unwrap_or_default();
        Arc::new(ViewerContext::new(principal).with_share_links(links))
    }
    #[cfg(not(feature = "auth"))]
    {
        let _ = extensions;
        Arc::new(ViewerContext::local())
    }
}

/// Check the route's requirement for the request's viewer, then run the
/// handler with the viewer in hand.
pub async fn authorize(
    State(state): State<AppState>,
    matched: Option<MatchedPath>,
    request: Request,
    next: Next,
) -> Response {
    let viewer = viewer_of(request.extensions());
    let route = matched.as_ref().map_or("", MatchedPath::as_str);
    let Some(requirement) = operations::rest(request.method().as_str(), route) else {
        // Fail closed: the API contract guard keeps every route declared.
        tracing::error!(
            http.route = route,
            "route without a declared requirement refused"
        );
        return ApiError(OxidGeneError::Forbidden).into_response();
    };
    let tree_id = match tree_in_path(route, request.uri().path()) {
        Ok(tree_id) => tree_id,
        Err(error) => return ApiError(error).into_response(),
    };
    let skips_read = viewer.is_local()
        && tree_id.is_some_and(|tree_id| {
            is_live_job_request(request.method(), tree_id, request.uri().path())
        });
    if !skips_read && let Err(error) = check(&state, &viewer, requirement, tree_id).await {
        authz::log::refusal(&state.db, &viewer, tree_id, route, requirement, &error);
        return ApiError(error).into_response();
    }
    #[cfg(feature = "auth")]
    let request = match scoped_write(&state, &viewer, requirement, tree_id, route, request).await {
        Ok(request) => request,
        Err(error) => {
            authz::log::refusal(&state.db, &viewer, tree_id, route, requirement, &error);
            return ApiError(error).into_response();
        }
    };
    let mut request = request;
    request.extensions_mut().insert(Viewer(viewer));
    next.run(request).await
}

/// The largest JSON body a scoped viewer's write is read for the persons
/// it names; a larger one is refused.
#[cfg(feature = "auth")]
const SCOPED_BODY_LIMIT: usize = 2 * 1024 * 1024;

/// Apply a scoped Editor's rules to their write (docs/authorization.md
/// §3.3, [`authz::scoped`]): a change to a record the whole tree shares is
/// `403`, and every person the path or the JSON body names must be in the
/// scope. The body is read here and handed on whole.
#[cfg(feature = "auth")]
async fn scoped_write(
    state: &AppState,
    viewer: &ViewerContext,
    requirement: Requirement,
    tree_id: Option<Uuid>,
    route: &str,
    request: Request,
) -> Result<Request, OxidGeneError> {
    let (Requirement::Tree(permission), Some(tree_id)) = (requirement, tree_id) else {
        return Ok(request);
    };
    let Some(access) = viewer.cached(tree_id) else {
        return Ok(request);
    };
    if !authz::scoped::applies(&access, permission) {
        return Ok(request);
    }
    if authz::scoped::refuses_route(route) {
        return Err(OxidGeneError::Forbidden);
    }
    let mut ids = std::collections::HashSet::new();
    authz::scoped::ids_in_path(request.uri().path(), &mut ids);
    let json = request
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("application/json"));
    let (parts, body) = request.into_parts();
    let bytes = if json {
        axum::body::to_bytes(body, SCOPED_BODY_LIMIT)
            .await
            .map_err(|_| OxidGeneError::Validation("the request body is too large".into()))?
    } else {
        // Only JSON bodies name persons; a file upload names none.
        let request = Request::from_parts(parts, body);
        authz::scoped::check_named(&state.db, &access, tree_id, &ids).await?;
        return Ok(request);
    };
    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) {
        authz::scoped::ids_in_json(&value, &mut ids);
    }
    authz::scoped::check_named(&state.db, &access, tree_id, &ids).await?;
    Ok(Request::from_parts(parts, axum::body::Body::from(bytes)))
}

async fn check(
    state: &AppState,
    viewer: &ViewerContext,
    requirement: Requirement,
    tree_id: Option<Uuid>,
) -> Result<(), OxidGeneError> {
    match authz::check(&state.reader, viewer, requirement, tree_id).await {
        // A database failure is not the client's fault: let the handler run
        // and report the real error rather than masking it, as a refusal
        // would, provided the viewer is the local owner, who may do
        // anything. Anyone else is refused.
        Err(OxidGeneError::Database(_)) if viewer.is_local() => Ok(()),
        result => result,
    }
}

/// The tree id in `path`, where route template `route` has `{tree_id}`.
fn tree_in_path(route: &str, path: &str) -> Result<Option<Uuid>, OxidGeneError> {
    let Some(index) = route.split('/').position(|segment| segment == "{tree_id}") else {
        return Ok(None);
    };
    path.split('/')
        .nth(index)
        .and_then(|segment| Uuid::parse_str(segment).ok())
        .map(Some)
        .ok_or_else(|| OxidGeneError::Validation("malformed identifier".to_string()))
}

/// Whether `method` on `path` reads the status of a job of tree `tree_id`
/// this process knows in memory, `GET …/import-jobs/{id}` or
/// `GET …/export-jobs/{id}`, or cancels it, the same paths followed by
/// `/cancel` with `POST`.
fn is_live_job_request(method: &Method, tree_id: Uuid, path: &str) -> bool {
    let tree = tree_id.to_string();
    let Some(rest) = path
        .split_once(&format!("/trees/{tree}/"))
        .map(|(_, rest)| rest)
    else {
        return false;
    };
    let mut rest = rest.split('/');
    let kind = match rest.next() {
        Some("import-jobs") => BackgroundJobKind::Import,
        Some("export-jobs") => BackgroundJobKind::Export,
        _ => return false,
    };
    let Some(Ok(job_id)) = rest.next().map(Uuid::parse_str) else {
        return false;
    };
    let request = match (method, rest.next()) {
        (&Method::GET, None) => true,
        (&Method::POST, Some("cancel")) => rest.next().is_none(),
        _ => false,
    };
    request && live_state(tree_id, job_id, kind).is_some()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use oxidgene_db::repo::{BackgroundJobKind, TreeRepo, run_migrations};
    use sea_orm::{ConnectOptions, Database, TransactionTrait};
    use tower::ServiceExt;
    use uuid::Uuid;

    use super::tree_in_path;
    use crate::service::background_job::LiveJobGuard;
    use crate::{AppState, build_router};

    #[test]
    fn the_tree_is_read_where_the_template_names_it() {
        let tree = Uuid::now_v7();
        assert_eq!(
            tree_in_path(
                "/api/v1/trees/{tree_id}/persons",
                &format!("/api/v1/trees/{tree}/persons")
            )
            .unwrap(),
            Some(tree)
        );
        assert_eq!(
            tree_in_path("/api/v1/trees", "/api/v1/trees").unwrap(),
            None
        );
        assert!(tree_in_path("/api/v1/trees/{tree_id}", "/api/v1/trees/not-a-uuid").is_err());
    }

    #[tokio::test]
    async fn a_running_job_status_does_not_wait_for_the_database() {
        let mut options = ConnectOptions::new("sqlite::memory:");
        options.max_connections(1);
        let db = Database::connect(options).await.expect("connects");
        run_migrations(&db).await.expect("migrates");
        let tree_id = Uuid::now_v7();
        TreeRepo::create(&db, tree_id, "Guard Fixture".into(), None)
            .await
            .expect("creates tree");
        let media_root = tempfile::tempdir().expect("creates media root");
        let app = build_router(AppState::new(db.clone(), media_root.path()));

        for (segment, kind) in [
            ("import-jobs", BackgroundJobKind::Import),
            ("export-jobs", BackgroundJobKind::Export),
        ] {
            let job_id = Uuid::now_v7();
            let _live_job = LiveJobGuard::for_test(job_id, tree_id, kind);
            // As an import holds it for its whole write transaction.
            let held = db.begin().await.expect("holds the only connection");
            let request = Request::get(format!("/api/v1/trees/{tree_id}/{segment}/{job_id}"))
                .body(Body::empty())
                .unwrap();
            let response =
                tokio::time::timeout(Duration::from_secs(2), app.clone().oneshot(request))
                    .await
                    .expect("the status poll does not wait for the connection")
                    .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{segment}");
            held.rollback().await.unwrap();
        }
    }

    /// The JSON body of `request` sent to `app`, which must answer `200`
    /// within `limit` although the running job holds the only connection.
    async fn answered_within(
        app: &axum::Router,
        request: Request<Body>,
        limit: Duration,
    ) -> serde_json::Value {
        use http_body_util::BodyExt as _;
        let response = tokio::time::timeout(limit, app.clone().oneshot(request))
            .await
            .expect("the request does not wait for the connection")
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).expect("a JSON body")
    }

    #[tokio::test]
    async fn a_running_job_is_cancelled_and_waited_on_without_the_database() {
        let mut options = ConnectOptions::new("sqlite::memory:");
        options.max_connections(1);
        let db = Database::connect(options).await.expect("connects");
        run_migrations(&db).await.expect("migrates");
        let tree_id = Uuid::now_v7();
        TreeRepo::create(&db, tree_id, "Guard Fixture".into(), None)
            .await
            .expect("creates tree");
        let media_root = tempfile::tempdir().expect("creates media root");
        let app = build_router(AppState::new(db.clone(), media_root.path()));

        for (segment, kind) in [
            ("import-jobs", BackgroundJobKind::Import),
            ("export-jobs", BackgroundJobKind::Export),
        ] {
            let job_id = Uuid::now_v7();
            let live_job = LiveJobGuard::for_test(job_id, tree_id, kind);
            // As an import holds it for its whole write transaction.
            let held = db.begin().await.expect("holds the only connection");
            let job = format!("/api/v1/trees/{tree_id}/{segment}/{job_id}");

            let cancelled = answered_within(
                &app,
                Request::post(format!("{job}/cancel"))
                    .body(Body::empty())
                    .unwrap(),
                Duration::from_secs(2),
            )
            .await;
            assert_eq!(cancelled["phase"], "cancelling", "{segment}");
            assert!(live_job.cancel_token().is_cancelled());
            // A wait for a change reads memory too, then answers when its
            // time runs out.
            let version = cancelled["version"].as_str().unwrap();
            let waited = answered_within(
                &app,
                Request::get(format!("{job}?since={version}&wait=1"))
                    .body(Body::empty())
                    .unwrap(),
                Duration::from_secs(4),
            )
            .await;
            assert_eq!(waited["version"], cancelled["version"]);
            held.rollback().await.unwrap();
        }
    }

    #[cfg(feature = "graphql")]
    #[tokio::test]
    async fn a_running_job_is_cancelled_over_graphql_without_the_database() {
        let mut options = ConnectOptions::new("sqlite::memory:");
        options.max_connections(1);
        let db = Database::connect(options).await.expect("connects");
        run_migrations(&db).await.expect("migrates");
        let tree_id = Uuid::now_v7();
        TreeRepo::create(&db, tree_id, "Guard Fixture".into(), None)
            .await
            .expect("creates tree");
        let media_root = tempfile::tempdir().expect("creates media root");
        let app = build_router(AppState::new(db.clone(), media_root.path()));

        for (field, kind) in [
            ("cancelImportJob", BackgroundJobKind::Import),
            ("cancelExportJob", BackgroundJobKind::Export),
        ] {
            let job_id = Uuid::now_v7();
            let live_job = LiveJobGuard::for_test(job_id, tree_id, kind);
            let held = db.begin().await.expect("holds the only connection");
            let query = format!(
                r#"mutation {{ {field}(treeId: "{tree_id}", jobId: "{job_id}") {{ phase }} }}"#
            );
            let response = answered_within(
                &app,
                Request::post("/graphql")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "query": query }).to_string(),
                    ))
                    .unwrap(),
                Duration::from_secs(2),
            )
            .await;
            assert_eq!(response["data"][field]["phase"], "cancelling", "{response}");
            assert!(live_job.cancel_token().is_cancelled());
            held.rollback().await.unwrap();
        }
    }
}
