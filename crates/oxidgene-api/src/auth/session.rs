//! Sessions of the `oidc` mode (docs/authentication.md §4.4–§4.7, §5): from
//! a cookie or a bearer token to the request's [`Principal`], refreshing the
//! session's access token when it nears its expiry, once per session at a
//! time, and ending sessions.
//!
//! A request is never authorized from the cookie alone: the session's access
//! token is validated at every request, as a bearer token would be.

use std::time::{Duration, Instant};

use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use oxidgene_core::OxidGeneError;
use oxidgene_db::repo::{AppUser, AppUserRepo, Connections, UserStatus};
use sea_orm::DatabaseConnection;
use serde_json::{Map, Value};
use uuid::Uuid;

use super::Oidc;
use super::cookie;
use super::crypto::{self, csrf_token, digest};
use super::identity::{Identity, Principal};
use super::provider::{Metadata, ProviderError, TokenResponse, with_query};
use super::store::{Session, StoreUnavailable, delete_all, now};
use super::token::{self, AccessIdentity, Purpose, TokenFailure};
use crate::rest::error::ErrorBody;
use crate::service::account::{self, IdentityClaims};

/// How long before its expiry an access token is refreshed: within its last
/// minute, and no earlier — some providers refuse a refresh token before.
const REFRESH_MARGIN: i64 = 60;
/// How long the refresh lock lasts at most: longer than the worst refresh —
/// discovery, the token endpoint and the key set, each bounded by the
/// provider's timeout — so that it never lapses while its holder still
/// refreshes, letting another refresh with the same, rotated, token.
const REFRESH_LOCK: Duration = Duration::from_secs(45);
/// How long a request waits for another's refresh.
const REFRESH_WAIT: Duration = Duration::from_secs(10);
/// How often a session's last use is written back: the idle limit is kept
/// to the minute without a store write per request.
const TOUCH_EVERY: i64 = 60;
/// The header a cookie-authenticated write carries its CSRF token in.
pub(crate) const CSRF_HEADER: &str = "x-oxidgene-csrf";

/// A refused credential: the answer the request gets instead of a handler's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Refusal {
    status: StatusCode,
    code: &'static str,
    /// The session cookie is cleared: its session is gone.
    clear_session: bool,
    /// The credential was a bearer token: `WWW-Authenticate` says why.
    bearer: bool,
}

impl Refusal {
    pub(crate) fn unauthenticated() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthenticated",
            clear_session: false,
            bearer: false,
        }
    }

    /// The session is gone: `401`, and the cookie cleared.
    fn ended() -> Self {
        Self {
            clear_session: true,
            ..Self::unauthenticated()
        }
    }

    fn forbidden() -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            code: "forbidden",
            clear_session: false,
            bearer: false,
        }
    }

    pub(crate) fn unavailable() -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "auth_unavailable",
            clear_session: false,
            bearer: false,
        }
    }

    fn bearer(self) -> Self {
        Self {
            bearer: true,
            ..self
        }
    }

    pub(crate) fn clears_session(&self) -> bool {
        self.clear_session
    }
}

impl From<StoreUnavailable> for Refusal {
    fn from(_: StoreUnavailable) -> Self {
        Self::unavailable()
    }
}

impl IntoResponse for Refusal {
    fn into_response(self) -> Response {
        let message = match self.code {
            "unauthenticated" => "Authentication is required",
            "forbidden" => "The request is not allowed",
            _ => "The identity provider is unavailable",
        };
        let body = ErrorBody {
            error: self.code.to_string(),
            message: message.to_string(),
            request_id: None,
        };
        let mut response = (self.status, axum::Json(body)).into_response();
        let headers = response.headers_mut();
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        if self.clear_session {
            cookie::end_session(headers);
        }
        if self.bearer && self.status == StatusCode::UNAUTHORIZED {
            headers.insert(
                header::WWW_AUTHENTICATE,
                HeaderValue::from_static("Bearer error=\"invalid_token\""),
            );
        }
        response
    }
}

/// A session resolved for a request: the principal, the session itself and
/// its user.
#[derive(Debug)]
pub(crate) struct ResolvedSession {
    pub principal: Principal,
    pub session: Session,
    pub user: AppUser,
}

/// What a sign-in hands to [`Oidc::create_session`].
pub(crate) struct NewSession<'a> {
    pub user: &'a AppUser,
    pub access: &'a AccessIdentity,
    pub tokens: &'a TokenResponse,
    pub sid: Option<String>,
    pub browser: &'static str,
}

impl Oidc {
    /// The identity an access token carries: a JWT validated here, or an
    /// opaque token introspected when the deployment allows it.
    pub(crate) async fn access_identity(
        &self,
        token: &str,
    ) -> Result<AccessIdentity, TokenFailure> {
        if token::is_jwt(token) {
            let claims = self
                .validator
                .validate(&self.provider, token, Purpose::Access)
                .await?;
            AccessIdentity::from_claims(claims)
        } else if self.config.introspection {
            self.introspection
                .identity(&self.validator, &self.provider, token)
                .await
        } else {
            Err(TokenFailure::Malformed)
        }
    }

    /// The principal a bearer token names (§5.1): a user signed in through
    /// a trusted client. Any other client's token is refused.
    pub(crate) async fn bearer_principal(
        &self,
        db: &DatabaseConnection,
        token: &str,
    ) -> Result<Principal, Refusal> {
        let identity = match self.access_identity(token).await {
            Ok(identity) => identity,
            Err(TokenFailure::Unavailable) => return Err(Refusal::unavailable()),
            Err(failure) => {
                log_token_invalid(failure.check(), None);
                return Err(Refusal::unauthenticated().bearer());
            }
        };
        let trusted = identity
            .client_id
            .as_ref()
            .is_some_and(|client| self.config.trusted_clients.contains(client));
        if !trusted {
            log_token_invalid("client", identity.client_id.as_deref());
            return Err(Refusal::unauthenticated().bearer());
        }
        let user = AppUserRepo::find_by_identity(db, &identity.issuer, &identity.subject)
            .await
            .map_err(|_| Refusal::unavailable())?
            .filter(|user| user.status == UserStatus::Active)
            .ok_or_else(|| Refusal::unauthenticated().bearer())?;
        Ok(Principal::User(Identity {
            issuer: identity.issuer,
            subject: identity.subject,
            user_id: user.id,
            client_id: identity.client_id,
            scopes: identity.scopes,
            session_id: None,
            api_client_id: None,
            instance_role: user.instance_role,
            can_create_trees: user.can_create_trees,
        }))
    }

    /// The session the cookie value `cookie` names, its access token fresh
    /// and valid, with its user. `writes` holds the headers of a
    /// state-changing request, whose CSRF token and origin are checked.
    pub(crate) async fn resolve_session(
        &self,
        db: &Connections,
        cookie: &str,
        writes: Option<&HeaderMap>,
    ) -> Result<ResolvedSession, Refusal> {
        let session = self.live_session(cookie).await?;
        if let Some(headers) = writes {
            self.check_write(headers, &session)?;
        }
        let session = self.fresh(&db.writer, session).await?;
        let identity = self.session_identity(&session).await?;
        let at = now();
        // A user gone ends the session; a database that does not answer
        // does not: the session is still good once it answers again.
        let user = match AppUserRepo::get(&db.reader, session.user_id).await {
            Ok(user) => user,
            Err(OxidGeneError::NotFound { .. }) => return Err(Refusal::ended()),
            Err(_) => return Err(Refusal::unavailable()),
        };
        if user.status != UserStatus::Active {
            self.end_user_sessions(user.id, "deactivated").await?;
            return Err(Refusal::ended());
        }
        if at - session.last_used_at >= TOUCH_EVERY {
            // Written back from a fresh read: the session read above may
            // predate a parallel request's refresh, whose rotated tokens
            // writing it back would undo.
            if let Some(mut current) = self.store.get(&session.id_hash).await? {
                current.last_used_at = at;
                current.idle_expires_at =
                    (at + seconds(self.config.idle_timeout)).min(current.absolute_expires_at);
                self.store.put(&current).await?;
            }
        }
        let principal = Principal::User(Identity {
            issuer: identity.issuer,
            subject: identity.subject,
            user_id: user.id,
            client_id: identity
                .client_id
                .or_else(|| Some(session.client_id.clone())),
            scopes: identity.scopes,
            session_id: Some(session.id_hash.clone()),
            api_client_id: None,
            instance_role: user.instance_role,
            can_create_trees: user.can_create_trees,
        });
        Ok(ResolvedSession {
            principal,
            session,
            user,
        })
    }

    /// The unexpired session the cookie value `cookie` names; a lapsed one
    /// ends.
    async fn live_session(&self, cookie: &str) -> Result<Session, Refusal> {
        let session = self
            .store
            .get(&digest(cookie))
            .await?
            .ok_or_else(Refusal::ended)?;
        let at = now();
        if session.expires_at() > at {
            return Ok(session);
        }
        let cause = if session.absolute_expires_at <= at {
            "absolute"
        } else {
            "idle"
        };
        self.end_session(&session, cause).await?;
        Err(Refusal::ended())
    }

    /// The identity `session`'s access token carries, validated as a bearer
    /// token would be; an invalid one ends the session.
    async fn session_identity(&self, session: &Session) -> Result<AccessIdentity, Refusal> {
        let access = self.open(session, &session.access_token)?;
        let failed = match self.access_identity(&access).await {
            Ok(identity)
                if identity.issuer == session.issuer && identity.subject == session.subject =>
            {
                return Ok(identity);
            }
            Ok(_) => "subject",
            Err(TokenFailure::Unavailable) => return Err(Refusal::unavailable()),
            Err(failure) => failure.check(),
        };
        log_token_invalid(failed, Some(&session.client_id));
        self.end_session(session, "token_invalid").await?;
        Err(Refusal::ended())
    }

    /// The three CSRF defences' two the server checks (§4.4): the CSRF
    /// header, compared in constant time, and the origin.
    pub(crate) fn check_write(
        &self,
        headers: &HeaderMap,
        session: &Session,
    ) -> Result<(), Refusal> {
        // The session was found by its cookie's digest, so its CSRF digest
        // is that cookie's; the header must match it.
        let given = headers
            .get(CSRF_HEADER)
            .and_then(|value| value.to_str().ok())
            .map(digest);
        let token_ok = given.is_some_and(|given| {
            crate::access::constant_time_eq(given.as_bytes(), session.csrf_hash.as_bytes())
        });
        let origin_ok = headers
            .get(header::ORIGIN)
            .is_none_or(|origin| origin.as_bytes() == self.config.public_origin.as_bytes());
        if token_ok && origin_ok {
            Ok(())
        } else {
            tracing::info!(
                auth = "csrf_refused",
                client_id = %session.client_id,
                "A write without its CSRF token or from another origin was refused"
            );
            Err(Refusal::forbidden())
        }
    }

    fn open(&self, session: &Session, sealed: &str) -> Result<String, Refusal> {
        self.sealer
            .open(sealed, &session.id_hash)
            .ok_or_else(Refusal::ended)
    }

    /// `session` with an access token valid for at least a minute more,
    /// refreshed when needed. Concurrent requests of one session wait for a
    /// single refresh: a refresh token rotated twice is a reuse, which the
    /// provider answers by revoking the whole family.
    async fn fresh(&self, db: &DatabaseConnection, session: Session) -> Result<Session, Refusal> {
        let lock = format!("refresh:{}", session.id_hash);
        let deadline = Instant::now() + REFRESH_WAIT;
        let mut session = session;
        loop {
            if session.access_expires_at - now() >= REFRESH_MARGIN {
                return Ok(session);
            }
            if session.refresh_token.is_none() {
                return self.unrefreshed(session, "no_refresh_token").await;
            }
            if let Some(holder) = self.store.lock(&lock, REFRESH_LOCK).await? {
                let result = self.refresh_locked(db, &session.id_hash).await;
                self.store.unlock(&lock, &holder).await?;
                return result;
            }
            if Instant::now() >= deadline {
                return self.unrefreshed(session, "refresh_wait").await;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
            session = self
                .store
                .get(&session.id_hash)
                .await?
                .ok_or_else(Refusal::ended)?;
        }
    }

    /// Refresh the session `id_hash`, its lock held. It is read again first:
    /// another request may have just refreshed it.
    async fn refresh_locked(
        &self,
        db: &DatabaseConnection,
        id_hash: &str,
    ) -> Result<Session, Refusal> {
        match self.store.get(id_hash).await? {
            Some(current) if current.access_expires_at - now() >= REFRESH_MARGIN => Ok(current),
            Some(current) => self.refresh(db, current).await,
            None => Err(Refusal::ended()),
        }
    }

    /// A session that could not be refreshed: usable while its access token
    /// lasts, unavailable after when the provider is to blame, ended when it
    /// cannot be refreshed at all.
    async fn unrefreshed(
        &self,
        session: Session,
        reason: &'static str,
    ) -> Result<Session, Refusal> {
        if session.access_expires_at > now() {
            return Ok(session);
        }
        if reason == "no_refresh_token" {
            self.end_session(&session, "expired").await?;
            return Err(Refusal::ended());
        }
        Err(Refusal::unavailable())
    }

    /// Refresh `session`'s tokens, holding its lock.
    async fn refresh(
        &self,
        db: &DatabaseConnection,
        mut session: Session,
    ) -> Result<Session, Refusal> {
        let sealed = session.refresh_token.clone().unwrap_or_default();
        let refresh_token = self.open(&session, &sealed)?;
        let tokens = match self.provider.refresh(&refresh_token).await {
            Ok(tokens) => tokens,
            Err(error) => return self.refresh_failed(session, error).await,
        };
        let access = match self.access_identity(&tokens.access_token).await {
            Ok(access) => access,
            Err(TokenFailure::Unavailable) => return Err(Refusal::unavailable()),
            Err(failure) => {
                log_token_invalid(failure.check(), Some(&session.client_id));
                self.end_session(&session, "token_invalid").await?;
                return Err(Refusal::ended());
            }
        };
        if let Some(id_token) = &tokens.id_token
            && self.refresh_claims(db, &session, &access, id_token).await?
        {
            return Err(Refusal::ended());
        }
        session.access_token = self.sealer.seal(&tokens.access_token, &session.id_hash);
        session.access_expires_at = access.expires_at;
        session.scopes = access.scopes;
        // The rotated refresh token replaces the old one; a provider that
        // does not rotate leaves it.
        if let Some(refresh_token) = &tokens.refresh_token {
            session.refresh_token = Some(self.sealer.seal(refresh_token, &session.id_hash));
        }
        if let Some(id_token) = &tokens.id_token {
            session.id_token = Some(self.sealer.seal(id_token, &session.id_hash));
        }
        self.store.put(&session).await?;
        Ok(session)
    }

    /// A refresh the provider did not grant: an unreachable provider leaves
    /// the session as it is, a refusal ends it.
    async fn refresh_failed(
        &self,
        session: Session,
        error: ProviderError,
    ) -> Result<Session, Refusal> {
        if error == ProviderError::Unreachable {
            super::provider::log_unreachable("token");
            return self.unrefreshed(session, "unreachable").await;
        }
        log_refresh_failed(error, &session.client_id);
        self.end_session(&session, "refresh_refused").await?;
        Err(Refusal::ended())
    }

    /// Bring the user's claims up to date from a refreshed ID token. `true`
    /// when that changed their instance role, which ends their sessions.
    async fn refresh_claims(
        &self,
        db: &DatabaseConnection,
        session: &Session,
        access: &AccessIdentity,
        id_token: &str,
    ) -> Result<bool, Refusal> {
        let Ok(claims) = self
            .validator
            .validate(&self.provider, id_token, Purpose::Id)
            .await
        else {
            // The access token is valid; a stale profile is no reason to
            // end the session.
            return Ok(false);
        };
        let identity = self.identity_claims(&claims, Some(&access.claims));
        if identity.subject != session.subject {
            return Ok(false);
        }
        let Ok(user) = AppUserRepo::get(db, session.user_id).await else {
            return Ok(false);
        };
        let admission = account::refresh_claims(db, &self.config.signup, &user, &identity)
            .await
            .map_err(|_| Refusal::unavailable())?;
        if admission.role_changed {
            self.end_user_sessions(user.id, "role_changed").await?;
        }
        Ok(admission.role_changed)
    }

    /// The claims an ID token gives of its user, the roles claim read from
    /// it or else from the access token.
    pub(crate) fn identity_claims(
        &self,
        id: &Map<String, Value>,
        access: Option<&Map<String, Value>>,
    ) -> IdentityClaims {
        let text = |name: &str| id.get(name).and_then(Value::as_str).map(str::to_string);
        let roles = self.config.roles_claim.as_ref().map(|path| {
            token::claim_values(id, path)
                .or_else(|| access.and_then(|access| token::claim_values(access, path)))
                .unwrap_or_default()
        });
        IdentityClaims {
            issuer: text("iss").unwrap_or_default(),
            subject: text("sub").unwrap_or_default(),
            email: text("email"),
            email_verified: id.get("email_verified").and_then(|value| match value {
                Value::Bool(verified) => Some(*verified),
                // Some providers send the boolean as a string.
                Value::String(verified) => Some(verified == "true"),
                _ => None,
            }),
            name: text("name"),
            preferred_username: text("preferred_username"),
            locale: text("locale"),
            roles,
        }
    }

    /// Store a new session for a sign-in; the cookie value is returned, the
    /// store keeping only its digest.
    pub(crate) async fn create_session(
        &self,
        new: NewSession<'_>,
    ) -> Result<(String, Session), StoreUnavailable> {
        let cookie = crypto::random_token();
        let id_hash = digest(&cookie);
        let at = now();
        let absolute = at + seconds(self.config.absolute_lifetime);
        let seal = |token: &str| self.sealer.seal(token, &id_hash);
        let session = Session {
            handle: Uuid::now_v7(),
            csrf_hash: digest(&csrf_token(&cookie)),
            user_id: new.user.id,
            issuer: new.access.issuer.clone(),
            subject: new.access.subject.clone(),
            client_id: new
                .access
                .client_id
                .clone()
                .unwrap_or_else(|| self.config.client_id.clone()),
            sid: new.sid,
            access_token: seal(&new.tokens.access_token),
            access_expires_at: new.access.expires_at,
            scopes: new.access.scopes.clone(),
            refresh_token: new.tokens.refresh_token.as_deref().map(seal),
            id_token: new.tokens.id_token.as_deref().map(seal),
            browser: new.browser.to_string(),
            created_at: at,
            last_used_at: at,
            idle_expires_at: (at + seconds(self.config.idle_timeout)).min(absolute),
            absolute_expires_at: absolute,
            id_hash,
        };
        self.store.put(&session).await?;
        Ok((cookie, session))
    }

    /// End `session`, logging why.
    pub(crate) async fn end_session(
        &self,
        session: &Session,
        cause: &'static str,
    ) -> Result<(), StoreUnavailable> {
        self.store.delete(&session.id_hash).await?;
        tracing::info!(
            auth = "session_ended",
            cause,
            client_id = %session.client_id,
            "A session ended"
        );
        Ok(())
    }

    /// End every session of `user_id`.
    pub(crate) async fn end_user_sessions(
        &self,
        user_id: Uuid,
        cause: &'static str,
    ) -> Result<(), StoreUnavailable> {
        let sessions = self.store.user_sessions(user_id).await?;
        let ended = delete_all(
            self.store.as_ref(),
            sessions.into_iter().map(|session| session.id_hash),
        )
        .await?;
        if ended > 0 {
            tracing::info!(
                auth = "session_ended",
                cause,
                sessions = ended,
                "A user's sessions ended"
            );
        }
        Ok(())
    }

    /// The provider's end-session address for `session` (RP-initiated
    /// logout), when it has one.
    pub(crate) fn logout_url(&self, metadata: &Metadata, session: &Session) -> Option<String> {
        let endpoint = metadata.end_session_endpoint.as_deref()?;
        let post_logout = format!("{}/", self.config.public_url);
        let id_token = session
            .id_token
            .as_deref()
            .and_then(|sealed| self.sealer.open(sealed, &session.id_hash));
        let mut params = vec![
            ("client_id", self.config.client_id.as_str()),
            ("post_logout_redirect_uri", post_logout.as_str()),
        ];
        if let Some(id_token) = id_token.as_deref() {
            params.insert(0, ("id_token_hint", id_token));
        }
        with_query(endpoint, &params)
    }
}

fn seconds(duration: Duration) -> i64 {
    i64::try_from(duration.as_secs()).unwrap_or(i64::MAX / 2)
}

/// Log a refresh the provider refused.
fn log_refresh_failed(error: ProviderError, client_id: &str) {
    let reason = if error == ProviderError::InvalidGrant {
        "invalid_grant"
    } else {
        "refused"
    };
    tracing::info!(
        auth = "refresh_failed",
        reason,
        client_id,
        "The provider refused to refresh a session"
    );
}

/// Log a token that failed validation: the check it failed and the client,
/// never the token or its subject.
pub(crate) fn log_token_invalid(check: &'static str, client_id: Option<&str>) {
    tracing::info!(
        auth = "token_invalid",
        check,
        client_id = client_id.unwrap_or("unknown"),
        "A token failed validation"
    );
}

/// The browser family a user agent names, for the session list: never the
/// user agent itself.
pub(crate) fn browser_family(user_agent: Option<&str>) -> &'static str {
    let Some(agent) = user_agent else {
        return "other";
    };
    if agent.contains("Edg/") {
        "edge"
    } else if agent.contains("OPR/") || agent.contains("Opera") {
        "opera"
    } else if agent.contains("Firefox/") {
        "firefox"
    } else if agent.contains("Chrome/") || agent.contains("Chromium/") {
        "chrome"
    } else if agent.contains("Safari/") {
        "safari"
    } else {
        "other"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browsers_are_told_apart_by_their_tokens() {
        let edge = "Mozilla/5.0 AppleWebKit/537.36 Chrome/120.0 Safari/537.36 Edg/120.0";
        assert_eq!(browser_family(Some(edge)), "edge");
        let chrome = "Mozilla/5.0 AppleWebKit/537.36 Chrome/120.0 Safari/537.36";
        assert_eq!(browser_family(Some(chrome)), "chrome");
        let safari = "Mozilla/5.0 AppleWebKit/605.1.15 Version/17.0 Safari/605.1.15";
        assert_eq!(browser_family(Some(safari)), "safari");
        let firefox = "Mozilla/5.0 (X11; Linux x86_64; rv:120.0) Gecko/20100101 Firefox/120.0";
        assert_eq!(browser_family(Some(firefox)), "firefox");
        assert_eq!(browser_family(Some("curl/8.0")), "other");
        assert_eq!(browser_family(None), "other");
    }

    #[test]
    fn a_refusal_clears_an_ended_session_and_explains_a_bearer_failure() {
        let response = Refusal::ended().into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let cookie = response.headers()[header::SET_COOKIE].to_str().unwrap();
        assert!(cookie.contains("__Host-oxidgene=") && cookie.contains("Max-Age=0"));
        let response = Refusal::unauthenticated().bearer().into_response();
        assert_eq!(
            response.headers()[header::WWW_AUTHENTICATE],
            "Bearer error=\"invalid_token\""
        );
        assert!(response.headers().get(header::SET_COOKIE).is_none());
    }
}
