//! The Backend-for-Frontend's routes (docs/authentication.md §4.2): the
//! server is the OAuth client, keeps the tokens, and gives the browser an
//! `HttpOnly` cookie. They sit outside `/api/v1`, are a transport rather
//! than product operations, and have no GraphQL twin. Every answer is
//! `Cache-Control: no-store`.

use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::Oidc;
use super::cookie;
use super::crypto::{self, csrf_token, digest, pkce_challenge};
use super::provider::{ProviderError, with_query};
use super::session::{NewSession, browser_family};
use super::store::{LoginTransaction, Session, now};
use super::token::Purpose;
use crate::AppState;
use crate::service::account::{self, interface_language};

/// How long a sign-in may take between `/auth/login` and its callback.
const LOGIN_LIFETIME: std::time::Duration = std::time::Duration::from_secs(600);
/// The longest `return_to` accepted.
const MAX_RETURN_TO: usize = 2048;
/// The back-channel logout event (OpenID Connect Back-Channel Logout 1.0).
const LOGOUT_EVENT: &str = "http://schemas.openid.net/event/backchannel-logout";

/// The `/auth/*` routes. `/auth/session` answers in every mode; the others
/// exist in `oidc` only.
pub(crate) fn routes(state: AppState) -> Router {
    let mut router = Router::new().route("/auth/session", get(session_state));
    if state.auth.oidc().is_some() {
        router = router
            .route("/auth/login", get(login))
            .route("/auth/callback", get(callback))
            .route("/auth/logout", post(logout))
            .route("/auth/logout-everywhere", post(logout_everywhere))
            .route("/auth/backchannel-logout", post(backchannel_logout))
            .route("/auth/share/{token}", get(open_share_link));
    }
    router
        .layer(axum::middleware::map_response(no_store))
        .with_state(state)
}

async fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn oidc(state: &AppState) -> &Oidc {
    state
        .auth
        .oidc()
        .expect("the oidc routes are mounted in oidc mode only")
}

/// A `303 See Other` to `location`, with `cookies` set.
fn redirect(location: &str, cookies: &[HeaderValue]) -> Response {
    let mut response = StatusCode::SEE_OTHER.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::LOCATION,
        HeaderValue::from_str(location).unwrap_or(HeaderValue::from_static("/")),
    );
    for cookie in cookies {
        headers.append(header::SET_COOKIE, cookie.clone());
    }
    response
}

/// The landing page of a failed sign-in.
fn failed(code: &'static str) -> Response {
    redirect(
        &format!("/?auth_error={code}"),
        &[cookie::clear(cookie::LOGIN)],
    )
}

/// `return_to` when it is a path of the application, else `/`: a single
/// leading `/`, so no scheme and no authority, no backslash, no control
/// character, at most 2 KiB. No open redirect.
pub(crate) fn safe_return_to(value: Option<&str>) -> String {
    match value {
        Some(path)
            if path.len() <= MAX_RETURN_TO
                && path.starts_with('/')
                && !path.starts_with("//")
                && !path.contains('\\')
                && !path.chars().any(char::is_control) =>
        {
            path.to_string()
        }
        _ => "/".to_string(),
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct LoginQuery {
    return_to: Option<String>,
    /// The interface language, passed to the provider as `ui_locales`.
    lang: Option<String>,
}

/// `GET /auth/login`: start a sign-in and send the browser to the provider.
///
/// A sign-in asked on another host than the public URL's — `127.0.0.1`
/// where it names `localhost` — is first sent to the same address there:
/// the binding cookie is set for the host that answers, and the callback
/// comes back to the public URL, whose browser would have none to show.
async fn login(
    State(state): State<AppState>,
    Query(query): Query<LoginQuery>,
    headers: HeaderMap,
) -> Response {
    let oidc = oidc(&state);
    if let Some(canonical) = canonical_login(&oidc.config, &headers, &query) {
        return redirect(&canonical, &[]);
    }
    let Ok(metadata) = oidc.provider.metadata().await else {
        log_failure("auth_unavailable", &oidc.config.client_id);
        return failed("auth_unavailable");
    };
    let state_value = crypto::random_token();
    let nonce = crypto::random_token();
    let verifier = crypto::random_token();
    let binding = crypto::random_token();
    let transaction = LoginTransaction {
        binding_hash: digest(&binding),
        nonce_hash: digest(&nonce),
        pkce_verifier: verifier.clone(),
        return_to: safe_return_to(query.return_to.as_deref()),
    };
    if oidc
        .store
        .put_login(&digest(&state_value), &transaction, LOGIN_LIFETIME)
        .await
        .is_err()
    {
        return failed("auth_unavailable");
    }
    let redirect_uri = oidc.config.redirect_uri();
    let challenge = pkce_challenge(&verifier);
    let mut params = vec![
        ("response_type", "code"),
        ("client_id", oidc.config.client_id.as_str()),
        ("redirect_uri", redirect_uri.as_str()),
        ("scope", oidc.config.scopes.as_str()),
        ("state", state_value.as_str()),
        ("nonce", nonce.as_str()),
        ("code_challenge", challenge.as_str()),
        ("code_challenge_method", "S256"),
    ];
    let language = query.lang.as_deref().and_then(interface_language);
    if let Some(language) = language {
        params.push(("ui_locales", language));
    }
    match with_query(&metadata.authorization_endpoint, &params) {
        Some(location) => redirect(&location, &[cookie::login(&binding)]),
        None => failed("auth_unavailable"),
    }
}

/// The same sign-in on the public URL's host, when `headers` name another:
/// `return_to` and `lang` carried over, the first one made safe.
fn canonical_login(
    config: &super::config::OidcConfig,
    headers: &HeaderMap,
    query: &LoginQuery,
) -> Option<String> {
    let host = headers.get(header::HOST)?.to_str().ok()?;
    let public_host = config
        .public_origin
        .split_once("://")
        .map_or(config.public_origin.as_str(), |(_, authority)| authority);
    if host.eq_ignore_ascii_case(public_host) {
        return None;
    }
    let return_to = safe_return_to(query.return_to.as_deref());
    let mut params = vec![("return_to", return_to.as_str())];
    if let Some(lang) = query.lang.as_deref() {
        params.push(("lang", lang));
    }
    with_query(&format!("{}/auth/login", config.public_url), &params)
}

#[derive(Debug, Deserialize)]
pub(crate) struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    iss: Option<String>,
    error: Option<String>,
}

/// Why a callback failed: the `auth_error` code and the logged reason.
struct Failure {
    code: &'static str,
    reason: &'static str,
}

fn login_failed(reason: &'static str) -> Failure {
    Failure {
        code: "login_failed",
        reason,
    }
}

/// `GET /auth/callback`: complete the sign-in (§4.3).
async fn callback(
    State(state): State<AppState>,
    Query(query): Query<CallbackQuery>,
    headers: HeaderMap,
) -> Response {
    let oidc = oidc(&state);
    match complete(&state, oidc, &query, &headers).await {
        Ok((cookie_value, return_to, user_id)) => {
            tracing::info!(
                auth = "login_succeeded",
                client_id = %oidc.config.client_id,
                "A user signed in"
            );
            access_event(
                &state,
                "login_succeeded",
                "ok",
                Some(user_id),
                &oidc.config.client_id,
            );
            redirect(
                &return_to,
                &[
                    cookie::session(&cookie_value, oidc.config.absolute_lifetime),
                    cookie::clear(cookie::LOGIN),
                ],
            )
        }
        Err(failure) => {
            log_failure(failure.reason, &oidc.config.client_id);
            access_event(
                &state,
                "login_failed",
                failure.reason,
                None,
                &oidc.config.client_id,
            );
            failed(failure.code)
        }
    }
}

/// Record a sign-in or sign-out in the access log (docs/authorization.md
/// §9.2): the user's id and the client, never an identity or an address.
fn access_event(
    state: &AppState,
    kind: &'static str,
    outcome: &str,
    user_id: Option<uuid::Uuid>,
    client_id: &str,
) {
    crate::authz::log::record(
        &state.db,
        oxidgene_db::repo::NewAccessEvent {
            kind,
            principal_kind: if user_id.is_some() {
                "user"
            } else {
                "anonymous"
            },
            user_id,
            client_id: Some(client_id.to_string()),
            outcome: outcome.to_string(),
            ..oxidgene_db::repo::NewAccessEvent::default()
        },
    );
}

fn log_failure(reason: &'static str, client_id: &str) {
    tracing::info!(auth = "login_failed", reason, client_id, "A sign-in failed");
}

const STORE_UNAVAILABLE: Failure = Failure {
    code: "auth_unavailable",
    reason: "store_unavailable",
};

const IDP_UNREACHABLE: Failure = Failure {
    code: "auth_unavailable",
    reason: "idp_unreachable",
};

/// The steps of a callback, to the new session's cookie value and where the
/// browser goes next.
async fn complete(
    state: &AppState,
    oidc: &Oidc,
    query: &CallbackQuery,
    headers: &HeaderMap,
) -> Result<(String, String, uuid::Uuid), Failure> {
    let transaction = bound_transaction(oidc, query, headers).await?;
    let signed = exchange(oidc, query, &transaction).await?;
    let claims = oidc.identity_claims(&signed.id_claims, Some(&signed.access.claims));
    let admission = account::sign_in(&state.db, &oidc.config.signup, &claims)
        .await
        .map_err(|_| login_failed("provisioning"))?
        .map_err(|refusal| Failure {
            code: refusal.code(),
            reason: refusal.code(),
        })?;
    let cookie_value = open_session(oidc, &admission, &signed, headers).await?;
    Ok((cookie_value, transaction.return_to, admission.user.id))
}

/// The sign-in transaction the callback's `state` names, consumed whatever
/// follows — a replay finds nothing — and bound to this browser's login
/// cookie: a code planted in a browser by another page has none.
async fn bound_transaction(
    oidc: &Oidc,
    query: &CallbackQuery,
    headers: &HeaderMap,
) -> Result<LoginTransaction, Failure> {
    let state_value = query.state.as_deref().ok_or(login_failed("no_state"))?;
    let transaction = oidc
        .store
        .take_login(&digest(state_value))
        .await
        .map_err(|_| STORE_UNAVAILABLE)?
        .ok_or(login_failed("unknown_state"))?;
    let bound = cookie::read(headers, cookie::LOGIN)
        .is_some_and(|binding| digest(&binding) == transaction.binding_hash);
    if !bound {
        return Err(login_failed("unbound"));
    }
    match query.error.as_deref() {
        Some("access_denied") => Err(Failure {
            code: "access_denied",
            reason: "provider_denied",
        }),
        Some(_) => Err(login_failed("provider_error")),
        None => Ok(transaction),
    }
}

/// The validated tokens of a sign-in.
struct Signed {
    tokens: super::provider::TokenResponse,
    id_claims: serde_json::Map<String, Value>,
    access: super::token::AccessIdentity,
}

/// Exchange the code and validate what the provider answers: the issuer of
/// the response (RFC 9207), the ID token with its `azp` and `nonce`, the
/// access token as every request will, both naming one subject.
async fn exchange(
    oidc: &Oidc,
    query: &CallbackQuery,
    transaction: &LoginTransaction,
) -> Result<Signed, Failure> {
    let metadata = oidc
        .provider
        .metadata()
        .await
        .map_err(|_| IDP_UNREACHABLE)?;
    let issuer_ok = match &query.iss {
        Some(iss) => *iss == oidc.config.issuer,
        None => !metadata.authorization_response_iss_parameter_supported,
    };
    if !issuer_ok {
        return Err(login_failed("issuer"));
    }
    let code = query.code.as_deref().ok_or(login_failed("no_code"))?;
    let tokens = oidc
        .provider
        .exchange_code(
            code,
            &transaction.pkce_verifier,
            &oidc.config.redirect_uri(),
        )
        .await
        .map_err(|error| match error {
            ProviderError::Unreachable => IDP_UNREACHABLE,
            ProviderError::InvalidGrant | ProviderError::Refused => login_failed("code_refused"),
        })?;
    let id_token = tokens
        .id_token
        .as_deref()
        .ok_or(login_failed("no_id_token"))?;
    let id_claims = oidc
        .validator
        .validate(&oidc.provider, id_token, Purpose::Id)
        .await
        .map_err(|failure| login_failed(failure.check()))?;
    let access = oidc
        .access_identity(&tokens.access_token)
        .await
        .map_err(|failure| login_failed(failure.check()))?;
    check_id_token(oidc, &id_claims, &access, transaction)?;
    Ok(Signed {
        tokens,
        id_claims,
        access,
    })
}

/// The ID token's `azp` and `nonce`, and the subject and client both tokens
/// name.
fn check_id_token(
    oidc: &Oidc,
    id_claims: &serde_json::Map<String, Value>,
    access: &super::token::AccessIdentity,
    transaction: &LoginTransaction,
) -> Result<(), Failure> {
    let text = |name: &str| id_claims.get(name).and_then(Value::as_str);
    if text("azp").is_some_and(|azp| azp != oidc.config.client_id) {
        return Err(login_failed("azp"));
    }
    if text("nonce").map(digest).as_deref() != Some(transaction.nonce_hash.as_str()) {
        return Err(login_failed("nonce"));
    }
    let other_client = access
        .client_id
        .as_ref()
        .is_some_and(|client| *client != oidc.config.client_id);
    if access.subject != text("sub").unwrap_or_default() || other_client {
        return Err(login_failed("client"));
    }
    Ok(())
}

/// Create the session of an admitted sign-in. No session fixation: whatever
/// session the browser held ends, and a role change ends every session of
/// the user.
async fn open_session(
    oidc: &Oidc,
    admission: &account::Admission,
    signed: &Signed,
    headers: &HeaderMap,
) -> Result<String, Failure> {
    if let Some(previous) = cookie::read(headers, cookie::SESSION)
        && let Ok(Some(session)) = oidc.store.get(&digest(&previous)).await
    {
        oidc.end_session(&session, "replaced")
            .await
            .map_err(|_| STORE_UNAVAILABLE)?;
    }
    if admission.role_changed {
        oidc.end_user_sessions(admission.user.id, "role_changed")
            .await
            .map_err(|_| STORE_UNAVAILABLE)?;
    }
    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|agent| agent.to_str().ok());
    let (cookie_value, _) = oidc
        .create_session(NewSession {
            user: &admission.user,
            access: &signed.access,
            tokens: &signed.tokens,
            sid: signed
                .id_claims
                .get("sid")
                .and_then(Value::as_str)
                .map(str::to_string),
            browser: browser_family(user_agent),
        })
        .await
        .map_err(|_| STORE_UNAVAILABLE)?;
    Ok(cookie_value)
}

/// What `/auth/session` answers.
#[derive(Debug, Serialize)]
pub(crate) struct SessionState {
    /// `oidc` or `none`.
    mode: &'static str,
    authenticated: bool,
    user: Option<SessionUser>,
    /// The web client the user signs in through; none in `none` mode.
    client_id: Option<String>,
    csrf_token: Option<String>,
    expires_at: Option<DateTime<Utc>>,
    idle_expires_at: Option<DateTime<Utc>>,
    /// The provider's account page, for the password and second factor.
    account_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct SessionUser {
    id: Uuid,
    display_name: Option<String>,
    email: Option<String>,
    email_verified: bool,
    locale: Option<String>,
    instance_role: &'static str,
    can_create_trees: bool,
}

fn timestamp(seconds: i64) -> Option<DateTime<Utc>> {
    DateTime::from_timestamp(seconds, 0)
}

/// `GET /auth/session`: always `200`, whether anyone is signed in or not.
async fn session_state(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let Some(oidc) = state.auth.oidc() else {
        return Json(SessionState {
            mode: "none",
            authenticated: false,
            user: None,
            client_id: None,
            csrf_token: None,
            expires_at: None,
            idle_expires_at: None,
            account_url: None,
        })
        .into_response();
    };
    let mut answer = SessionState {
        mode: "oidc",
        authenticated: false,
        user: None,
        client_id: Some(oidc.config.client_id.clone()),
        csrf_token: None,
        expires_at: None,
        idle_expires_at: None,
        account_url: oidc.config.account_url.clone(),
    };
    let Some(cookie_value) = cookie::read(&headers, cookie::SESSION) else {
        return Json(answer).into_response();
    };
    match oidc
        .resolve_session(&state.connections(), &cookie_value, None)
        .await
    {
        Ok(resolved) => {
            let user = &resolved.user;
            answer.authenticated = true;
            answer.csrf_token = Some(csrf_token(&cookie_value));
            answer.expires_at = timestamp(resolved.session.absolute_expires_at);
            answer.idle_expires_at = timestamp(resolved.session.idle_expires_at);
            answer.user = Some(SessionUser {
                id: user.id,
                display_name: user.shown_name().map(str::to_string),
                email: user.email.clone(),
                email_verified: user.email_verified,
                locale: user.locale.clone(),
                instance_role: user.instance_role.as_str(),
                can_create_trees: user.can_create_trees,
            });
            Json(answer).into_response()
        }
        Err(refusal) => {
            let mut response = Json(answer).into_response();
            if refusal.clears_session() {
                cookie::end_session(response.headers_mut());
            }
            response
        }
    }
}

/// The most share links one browser holds the grants of: opening another
/// drops the oldest.
const MAX_SHARE_GRANTS: usize = 50;

/// `GET /auth/share/{token}`: open a share link (docs/authorization.md
/// §6.3). The link's grant joins the browser's share grants — created if it
/// has none, whether anyone is signed in or not — and the browser is sent
/// to the tree, so the token leaves the address bar at once. A link that
/// opens nothing lands on the homepage with `auth_error=share_link_invalid`.
async fn open_share_link(
    State(state): State<AppState>,
    axum::extract::Path(token): axum::extract::Path<String>,
    headers: HeaderMap,
) -> Response {
    let oidc = oidc(&state);
    let principal = match cookie::read(&headers, cookie::SESSION) {
        Some(value) => oidc
            .resolve_session(&state.connections(), &value, None)
            .await
            .map_or(oxidgene_core::access::Principal::Anonymous, |resolved| {
                resolved.principal
            }),
        None => oxidgene_core::access::Principal::Anonymous,
    };
    let viewer = oxidgene_core::access::ViewerContext::new(principal);
    let link = match crate::service::share_link::redeem(&state.db, &viewer, &token).await {
        Ok(Some(link)) => link,
        Ok(None) => return redirect("/?auth_error=share_link_invalid", &[]),
        Err(_) => return redirect("/?auth_error=auth_unavailable", &[]),
    };
    let at = now();
    let lifetime = i64::try_from(oidc.config.absolute_lifetime.as_secs()).unwrap_or(i64::MAX / 2);
    let ends = link.expires_at.map_or(at + lifetime, |expires| {
        expires.timestamp().min(at + lifetime)
    });
    let held = match cookie::read(&headers, cookie::SHARE) {
        Some(value) => match oidc.store.get_grants(&digest(&value)).await {
            Ok(Some(grants)) => Some((value, grants)),
            _ => None,
        },
        None => None,
    };
    let (value, mut grants) = held.unwrap_or_else(|| {
        (
            crypto::random_token(),
            super::store::LinkGrants {
                links: Vec::new(),
                expires_at: at,
            },
        )
    });
    grants.links.retain(|id| *id != link.id);
    grants.links.push(link.id);
    if grants.links.len() > MAX_SHARE_GRANTS {
        grants.links.remove(0);
    }
    grants.expires_at = grants.expires_at.max(ends);
    if oidc
        .store
        .put_grants(&digest(&value), &grants)
        .await
        .is_err()
    {
        return redirect("/?auth_error=auth_unavailable", &[]);
    }
    let max_age =
        std::time::Duration::from_secs(u64::try_from(grants.expires_at - at).unwrap_or(0));
    redirect(
        &format!("/trees/{}", link.tree_id),
        &[cookie::share(&value, max_age)],
    )
}

#[derive(Debug, Serialize)]
pub(crate) struct LogoutAnswer {
    /// The provider's end-session address to navigate to, or `null`.
    logout_url: Option<String>,
}

/// The session a sign-out names, once its CSRF token and origin are
/// checked; `Ok(None)` when the cookie names none.
async fn signing_out(
    oidc: &Oidc,
    headers: &HeaderMap,
) -> Result<Option<Session>, super::session::Refusal> {
    let Some(cookie_value) = cookie::read(headers, cookie::SESSION) else {
        return Ok(None);
    };
    let Some(session) = oidc.store.get(&digest(&cookie_value)).await? else {
        return Ok(None);
    };
    oidc.check_write(headers, &session)?;
    Ok(Some(session))
}

async fn sign_out(state: &AppState, headers: &HeaderMap, everywhere: bool) -> Response {
    let oidc = oidc(state);
    let session = match signing_out(oidc, headers).await {
        Ok(session) => session,
        Err(refused) => return refused.into_response(),
    };
    let mut logout_url = None;
    if let Some(session) = session {
        let ended = if everywhere {
            oidc.end_user_sessions(session.user_id, "logout_everywhere")
                .await
        } else {
            oidc.end_session(&session, "logout").await
        };
        if ended.is_err() {
            return super::session::Refusal::unavailable().into_response();
        }
        access_event(
            state,
            if everywhere {
                "logout_everywhere"
            } else {
                "logout"
            },
            "ok",
            Some(session.user_id),
            &session.client_id,
        );
        if let Ok(metadata) = oidc.provider.metadata().await {
            logout_url = oidc.logout_url(&metadata, &session);
        }
    }
    // Signing out on a shared computer leaves no share link's access
    // behind either.
    if let Some(value) = cookie::read(headers, cookie::SHARE)
        && oidc.store.delete_grants(&digest(&value)).await.is_err()
    {
        return super::session::Refusal::unavailable().into_response();
    }
    let mut response = Json(LogoutAnswer { logout_url }).into_response();
    let cleared = response.headers_mut();
    cookie::end_session(cleared);
    cleared.append(header::SET_COOKIE, cookie::clear(cookie::SHARE));
    response
}

/// `POST /auth/logout`: end this session (§4.7).
async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    sign_out(&state, &headers, false).await
}

/// `POST /auth/logout-everywhere`: end every session of the user.
async fn logout_everywhere(State(state): State<AppState>, headers: HeaderMap) -> Response {
    sign_out(&state, &headers, true).await
}

#[derive(Debug, Deserialize)]
pub(crate) struct BackchannelForm {
    logout_token: String,
}

/// `POST /auth/backchannel-logout`: the provider ends a session (§4.7).
/// No cookie, no CSRF token: the logout token is the credential.
async fn backchannel_logout(
    State(state): State<AppState>,
    Form(form): Form<BackchannelForm>,
) -> Response {
    let oidc = oidc(&state);
    let status = match end_by_logout_token(&state, oidc, &form.logout_token).await {
        Ok(()) => StatusCode::OK,
        Err(status) => status,
    };
    access_event(
        &state,
        "backchannel_logout",
        if status == StatusCode::OK {
            "ended"
        } else {
            "refused"
        },
        None,
        &oidc.config.client_id,
    );
    tracing::info!(
        auth = "backchannel_logout",
        outcome = if status == StatusCode::OK { "ended" } else { "refused" },
        client_id = %oidc.config.client_id,
        "The provider asked to end a session"
    );
    status.into_response()
}

/// The claims of a valid, first-seen logout token: the back-channel event,
/// a `sid` or a `sub`, a `jti`, no `nonce`.
async fn logout_claims(
    oidc: &Oidc,
    token: &str,
) -> Result<serde_json::Map<String, Value>, StatusCode> {
    let claims = oidc
        .validator
        .validate(&oidc.provider, token, Purpose::Logout)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let has_event = claims
        .get("events")
        .and_then(Value::as_object)
        .is_some_and(|events| events.contains_key(LOGOUT_EVENT));
    let names_a_session = claims.contains_key("sid") || claims.contains_key("sub");
    let jti = claims.get("jti").and_then(Value::as_str);
    let (Some(jti), true, true, false) = (
        jti,
        has_event,
        names_a_session,
        claims.contains_key("nonce"),
    ) else {
        return Err(StatusCode::BAD_REQUEST);
    };
    // Not seen within its lifetime: a replayed token is refused.
    let lifetime = claims
        .get("exp")
        .and_then(Value::as_i64)
        .map_or(600, |exp| (exp - now()).clamp(60, 86_400));
    let ttl = std::time::Duration::from_secs(u64::try_from(lifetime + 60).unwrap_or(660));
    match oidc.store.first_seen(&digest(jti), ttl).await {
        Ok(true) => Ok(claims),
        Ok(false) => Err(StatusCode::BAD_REQUEST),
        Err(_) => Err(StatusCode::SERVICE_UNAVAILABLE),
    }
}

async fn end_by_logout_token(state: &AppState, oidc: &Oidc, token: &str) -> Result<(), StatusCode> {
    let claims = logout_claims(oidc, token).await?;
    let text = |name: &str| claims.get(name).and_then(Value::as_str);
    let (sid, sub) = (text("sid"), text("sub"));
    let unavailable = |_| StatusCode::SERVICE_UNAVAILABLE;
    if let Some(sid) = sid {
        let keys = oidc.store.sid_sessions(sid).await.map_err(unavailable)?;
        // A session signed in without a `sid` is not indexed under one: a
        // token naming its user too ends the user's sessions without one,
        // rather than nothing.
        if !keys.is_empty() || sub.is_none() {
            super::store::delete_all(oidc.store.as_ref(), keys)
                .await
                .map_err(unavailable)?;
            return Ok(());
        }
    }
    let issuer = text("iss").unwrap_or_default();
    let Some(user) = oxidgene_db::repo::AppUserRepo::find_by_identity(
        &state.reader,
        issuer,
        sub.unwrap_or_default(),
    )
    .await
    .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
    else {
        return Ok(());
    };
    if sid.is_some() {
        let keys: Vec<String> = oidc
            .store
            .user_sessions(user.id)
            .await
            .map_err(unavailable)?
            .into_iter()
            .filter(|session| session.sid.is_none())
            .map(|session| session.id_hash)
            .collect();
        super::store::delete_all(oidc.store.as_ref(), keys)
            .await
            .map_err(unavailable)?;
        return Ok(());
    }
    oidc.end_user_sessions(user.id, "backchannel_logout")
        .await
        .map_err(unavailable)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn return_to_is_a_path_of_the_application_or_the_root() {
        for kept in ["/", "/trees/abc?tab=persons", "/a:b"] {
            assert_eq!(safe_return_to(Some(kept)), kept);
        }
        for refused in [
            "https://elsewhere.example.invalid/",
            "//elsewhere.example.invalid",
            "/\\elsewhere.example.invalid",
            "relative",
            "/line\nbreak",
            "",
        ] {
            assert_eq!(safe_return_to(Some(refused)), "/", "{refused:?}");
        }
        assert_eq!(safe_return_to(Some(&format!("/{}", "a".repeat(2048)))), "/");
        assert_eq!(safe_return_to(None), "/");
    }
}
