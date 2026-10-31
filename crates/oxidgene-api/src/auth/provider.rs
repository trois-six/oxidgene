//! The identity provider, over plain HTTP (docs/authentication.md §3, §5.2):
//! discovery, the JWKS, the token endpoint (code exchange and refresh),
//! introspection, and the addresses the browser is sent to.
//!
//! The server-to-server requests go to `OXIDGENE_OIDC_BACKCHANNEL_URL` when
//! it is set (§3.4): discovery is read there, under the issuer's path, and
//! its endpoints are used as returned. The issuer it announces must still be
//! the configured one exactly.

use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use jsonwebtoken::jwk::{Jwk, JwkSet};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};

use super::config::{ClientAuth, OidcConfig};

/// How long a request to the provider may take.
const TIMEOUT: Duration = Duration::from_secs(10);
/// How long discovery is kept before it is read again.
const METADATA_LIFETIME: Duration = Duration::from_secs(24 * 3600);
/// How long after a failed fetch the provider is asked again: a provider
/// that is down is not asked on every request.
const RETRY_AFTER_FAILURE: Duration = Duration::from_secs(10);
/// The bounds of the JWKS's lifetime, whatever its `Cache-Control` says.
const JWKS_MIN_LIFETIME: Duration = Duration::from_secs(5 * 60);
const JWKS_MAX_LIFETIME: Duration = Duration::from_secs(24 * 3600);
/// How often an unknown `kid` may make the JWKS be fetched again.
const UNKNOWN_KID_REFETCH: Duration = Duration::from_secs(60);

/// What the provider announces at `/.well-known/openid-configuration`.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Metadata {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub jwks_uri: String,
    #[serde(default)]
    pub end_session_endpoint: Option<String>,
    #[serde(default)]
    pub introspection_endpoint: Option<String>,
    /// RFC 9207: the authorization response carries `iss`.
    #[serde(default)]
    pub authorization_response_iss_parameter_supported: bool,
}

/// A request to the provider that did not succeed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderError {
    /// No answer, or no usable one: `503 auth_unavailable`.
    Unreachable,
    /// The token endpoint refused the grant (`invalid_grant`): the user
    /// was disabled, signed out, or their provider session ended.
    InvalidGrant,
    /// Any other refusal.
    Refused,
}

/// The token endpoint's answer.
#[derive(Clone, Deserialize)]
pub(crate) struct TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub id_token: Option<String>,
}

/// The introspection endpoint's answer (RFC 7662).
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Introspection {
    #[serde(default)]
    pub active: bool,
    #[serde(flatten)]
    pub claims: serde_json::Map<String, serde_json::Value>,
}

/// A cached value and when it lapses.
struct Cached<T> {
    value: T,
    until: Instant,
}

#[derive(Default)]
struct Keys {
    set: Option<Cached<Vec<Jwk>>>,
    last_unknown_refetch: Option<Instant>,
}

/// The provider of the `oidc` mode.
pub(crate) struct Provider {
    issuer: String,
    /// `OXIDGENE_OIDC_BACKCHANNEL_URL`: the scheme, host and port the
    /// server-to-server requests go to.
    backchannel: Option<Url>,
    discovery_url: String,
    client_id: String,
    client_auth: ClientAuth,
    http: reqwest::Client,
    metadata: RwLock<Option<Cached<Arc<Metadata>>>>,
    last_failure: RwLock<Option<Instant>>,
    keys: RwLock<Keys>,
    /// One discovery fetch at a time.
    fetching: Mutex<()>,
    /// One JWKS fetch at a time.
    fetching_keys: Mutex<()>,
}

impl std::fmt::Debug for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Provider")
            .field("client_id", &self.client_id)
            .finish_non_exhaustive()
    }
}

impl Provider {
    pub(crate) fn new(config: &OidcConfig) -> Self {
        Self {
            issuer: config.issuer.clone(),
            backchannel: config.backchannel_url.clone(),
            discovery_url: discovery_url(&config.issuer, config.backchannel_url.as_ref()),
            client_id: config.client_id.clone(),
            client_auth: config.client_auth.clone(),
            http: reqwest::Client::builder()
                .timeout(TIMEOUT)
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("an HTTP client"),
            metadata: RwLock::new(None),
            last_failure: RwLock::new(None),
            keys: RwLock::new(Keys::default()),
            fetching: Mutex::new(()),
            fetching_keys: Mutex::new(()),
        }
    }

    /// The provider's discovery document: cached for a day, read again
    /// after, and kept while the provider cannot be reached.
    pub(crate) async fn metadata(&self) -> Result<Arc<Metadata>, ProviderError> {
        if let Some(cached) = self.metadata.read().await.as_ref()
            && cached.until > Instant::now()
        {
            return Ok(Arc::clone(&cached.value));
        }
        let _fetching = self.fetching.lock().await;
        if let Some(cached) = self.metadata.read().await.as_ref()
            && cached.until > Instant::now()
        {
            return Ok(Arc::clone(&cached.value));
        }
        let stale = self
            .metadata
            .read()
            .await
            .as_ref()
            .map(|cached| Arc::clone(&cached.value));
        if self.failed_recently().await {
            return stale.ok_or(ProviderError::Unreachable);
        }
        match self.fetch_metadata().await {
            Ok(metadata) => {
                let metadata = Arc::new(metadata);
                *self.metadata.write().await = Some(Cached {
                    value: Arc::clone(&metadata),
                    until: Instant::now() + METADATA_LIFETIME,
                });
                Ok(metadata)
            }
            Err(error) => {
                self.record_failure().await;
                stale.ok_or(error)
            }
        }
    }

    async fn fetch_metadata(&self) -> Result<Metadata, ProviderError> {
        let response = self
            .http
            .get(&self.discovery_url)
            .send()
            .await
            .map_err(|_| unreachable())?;
        if !response.status().is_success() {
            return Err(unreachable());
        }
        let metadata: Metadata = response.json().await.map_err(|_| unreachable())?;
        // The provider must call itself by the configured issuer, whatever
        // address discovery was read from.
        if metadata.issuer != self.issuer {
            tracing::error!(
                error = "idp_unreachable",
                reason = "issuer_mismatch",
                "The identity provider announces another issuer"
            );
            return Err(ProviderError::Unreachable);
        }
        Ok(self.through_backchannel(metadata))
    }

    /// `metadata` with the endpoints OxidGene calls itself — token, JWKS,
    /// introspection — moved to the back channel when they sit on the
    /// issuer's origin: some providers always advertise their public host.
    /// The browser's endpoints, authorization and end-session, are left as
    /// discovery gives them.
    fn through_backchannel(&self, mut metadata: Metadata) -> Metadata {
        let Some(backchannel) = &self.backchannel else {
            return metadata;
        };
        let Ok(issuer) = Url::parse(&self.issuer) else {
            return metadata;
        };
        let moved = |endpoint: &str| -> String {
            match Url::parse(endpoint) {
                Ok(mut url) if url.origin() == issuer.origin() => {
                    let _ = url.set_scheme(backchannel.scheme());
                    let _ = url.set_host(backchannel.host_str());
                    let _ = url.set_port(backchannel.port());
                    url.into()
                }
                _ => endpoint.to_string(),
            }
        };
        metadata.token_endpoint = moved(&metadata.token_endpoint);
        metadata.jwks_uri = moved(&metadata.jwks_uri);
        metadata.introspection_endpoint = metadata.introspection_endpoint.as_deref().map(moved);
        metadata
    }

    async fn failed_recently(&self) -> bool {
        self.last_failure
            .read()
            .await
            .is_some_and(|at| at.elapsed() < RETRY_AFTER_FAILURE)
    }

    async fn record_failure(&self) {
        *self.last_failure.write().await = Some(Instant::now());
    }

    /// The JWKS key `kid`. An unknown `kid` fetches the set again, at most
    /// once a minute, so that a rotated key is found at once and forged
    /// `kid`s cannot make OxidGene hammer the provider.
    pub(crate) async fn key(&self, kid: &str) -> Result<Option<Jwk>, ProviderError> {
        let now = Instant::now();
        {
            let keys = self.keys.read().await;
            if let Some(set) = keys.set.as_ref().filter(|set| set.until > now) {
                if let Some(key) = find(&set.value, kid) {
                    return Ok(Some(key));
                }
                if keys
                    .last_unknown_refetch
                    .is_some_and(|at| now.duration_since(at) < UNKNOWN_KID_REFETCH)
                {
                    return Ok(None);
                }
            }
        }
        let refetched = self.refresh_keys(kid).await?;
        Ok(find(&refetched, kid))
    }

    /// Fetch the JWKS, unless another request just did, or one did for an
    /// unknown `kid` less than a minute ago.
    async fn refresh_keys(&self, kid: &str) -> Result<Vec<Jwk>, ProviderError> {
        let _fetching = self.fetching_keys.lock().await;
        if let Some(current) = self.current_keys(kid).await {
            return Ok(current);
        }
        let metadata = self.metadata().await?;
        match self.fetch_keys(&metadata.jwks_uri).await {
            Ok((set, lifetime)) => {
                let mut keys = self.keys.write().await;
                if keys.set.is_some() {
                    keys.last_unknown_refetch = Some(Instant::now());
                }
                keys.set = Some(Cached {
                    value: set.clone(),
                    until: Instant::now() + lifetime,
                });
                Ok(set)
            }
            Err(error) => {
                self.record_failure().await;
                log_unreachable("jwks");
                // A provider that is down keeps the keys it last published.
                let keys = self.keys.read().await;
                keys.set.as_ref().map(|set| set.value.clone()).ok_or(error)
            }
        }
    }

    /// The cached set, when it is current and either holds `kid` or was
    /// fetched again for an unknown `kid` less than a minute ago.
    async fn current_keys(&self, kid: &str) -> Option<Vec<Jwk>> {
        let now = Instant::now();
        let keys = self.keys.read().await;
        let set = keys.set.as_ref().filter(|set| set.until > now)?;
        let recent = keys
            .last_unknown_refetch
            .is_some_and(|at| now.duration_since(at) < UNKNOWN_KID_REFETCH);
        (find(&set.value, kid).is_some() || recent).then(|| set.value.clone())
    }

    async fn fetch_keys(&self, uri: &str) -> Result<(Vec<Jwk>, Duration), ProviderError> {
        let response = self.http.get(uri).send().await.map_err(|_| unreachable())?;
        if !response.status().is_success() {
            return Err(unreachable());
        }
        let lifetime = response
            .headers()
            .get(reqwest::header::CACHE_CONTROL)
            .and_then(|value| value.to_str().ok())
            .and_then(max_age)
            .unwrap_or(JWKS_MIN_LIFETIME)
            .clamp(JWKS_MIN_LIFETIME, JWKS_MAX_LIFETIME);
        let set: JwkSet = response.json().await.map_err(|_| unreachable())?;
        Ok((set.keys, lifetime))
    }

    /// Exchange an authorization code with its PKCE verifier.
    pub(crate) async fn exchange_code(
        &self,
        code: &str,
        verifier: &str,
        redirect_uri: &str,
    ) -> Result<TokenResponse, ProviderError> {
        self.token_request(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("code_verifier", verifier),
            ("redirect_uri", redirect_uri),
        ])
        .await
    }

    /// Exchange a refresh token for new tokens; the provider rotates it.
    pub(crate) async fn refresh(
        &self,
        refresh_token: &str,
    ) -> Result<TokenResponse, ProviderError> {
        self.token_request(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ])
        .await
    }

    async fn token_request(&self, form: &[(&str, &str)]) -> Result<TokenResponse, ProviderError> {
        let metadata = self.metadata().await?;
        let response = self
            .authenticated_post(&metadata.token_endpoint, form)
            .await?;
        let status = response.status();
        if status.is_success() {
            return response.json().await.map_err(|_| ProviderError::Refused);
        }
        if status.is_server_error() {
            return Err(ProviderError::Unreachable);
        }
        #[derive(Deserialize)]
        struct OAuthError {
            #[serde(default)]
            error: String,
        }
        let error = response
            .json::<OAuthError>()
            .await
            .map(|body| body.error)
            .unwrap_or_default();
        Err(if error == "invalid_grant" {
            ProviderError::InvalidGrant
        } else {
            ProviderError::Refused
        })
    }

    /// Introspect an opaque token (RFC 7662) with the web client's
    /// credentials.
    pub(crate) async fn introspect(&self, token: &str) -> Result<Introspection, ProviderError> {
        let metadata = self.metadata().await?;
        let endpoint = metadata
            .introspection_endpoint
            .as_deref()
            .ok_or(ProviderError::Refused)?;
        let response = self
            .authenticated_post(endpoint, &[("token", token)])
            .await?;
        if response.status().is_server_error() {
            return Err(ProviderError::Unreachable);
        }
        if !response.status().is_success() {
            return Err(ProviderError::Refused);
        }
        response.json().await.map_err(|_| ProviderError::Refused)
    }

    /// POST `form` to `endpoint` with the web client's authentication.
    async fn authenticated_post(
        &self,
        endpoint: &str,
        form: &[(&str, &str)],
    ) -> Result<reqwest::Response, ProviderError> {
        let mut fields: Vec<(&str, String)> = form
            .iter()
            .map(|(name, value)| (*name, (*value).to_string()))
            .collect();
        let mut request = self.http.post(endpoint);
        match &self.client_auth {
            ClientAuth::SecretBasic(secret) => {
                // RFC 6749 §2.3.1: both are form-encoded before base64.
                let credentials =
                    format!("{}:{}", form_encode(&self.client_id), form_encode(secret));
                request = request.header(
                    reqwest::header::AUTHORIZATION,
                    format!(
                        "Basic {}",
                        base64::engine::general_purpose::STANDARD.encode(credentials)
                    ),
                );
            }
            ClientAuth::SecretPost(secret) => {
                fields.push(("client_id", self.client_id.clone()));
                fields.push(("client_secret", secret.clone()));
            }
            ClientAuth::PrivateKeyJwt(key, algorithm) => {
                fields.push(("client_id", self.client_id.clone()));
                fields.push((
                    "client_assertion_type",
                    "urn:ietf:params:oauth:client-assertion-type:jwt-bearer".to_string(),
                ));
                fields.push((
                    "client_assertion",
                    self.client_assertion(endpoint, key, *algorithm)?,
                ));
            }
        }
        request
            .form(&fields)
            .send()
            .await
            .map_err(|_| unreachable())
    }

    /// An RFC 7523 client assertion for `audience`, valid for a minute.
    fn client_assertion(
        &self,
        audience: &str,
        key: &jsonwebtoken::EncodingKey,
        algorithm: jsonwebtoken::Algorithm,
    ) -> Result<String, ProviderError> {
        #[derive(Serialize)]
        struct Assertion<'a> {
            iss: &'a str,
            sub: &'a str,
            aud: &'a str,
            jti: String,
            iat: i64,
            exp: i64,
        }
        let now = super::store::now();
        jsonwebtoken::encode(
            &jsonwebtoken::Header::new(algorithm),
            &Assertion {
                iss: &self.client_id,
                sub: &self.client_id,
                aud: audience,
                jti: super::crypto::random_token(),
                iat: now,
                exp: now + 60,
            },
            key,
        )
        .map_err(|_| ProviderError::Refused)
    }
}

/// Log that the provider's `endpoint` could not be reached.
pub(crate) fn log_unreachable(endpoint: &'static str) {
    tracing::warn!(
        auth = "idp_unreachable",
        endpoint,
        "The identity provider could not be reached"
    );
}

fn unreachable() -> ProviderError {
    ProviderError::Unreachable
}

/// `issuer`'s discovery address — the issuer without its trailing slash,
/// then `/.well-known/openid-configuration` — read under `backchannel` when
/// set: its scheme, host and port replace the issuer's, whose path is kept.
fn discovery_url(issuer: &str, backchannel: Option<&Url>) -> String {
    let issuer = issuer.trim_end_matches('/');
    let base = match (backchannel, Url::parse(issuer)) {
        (Some(backchannel), Ok(parsed)) => format!(
            "{}{}",
            backchannel.origin().ascii_serialization(),
            parsed.path().trim_end_matches('/')
        ),
        _ => issuer.to_string(),
    };
    format!("{base}/.well-known/openid-configuration")
}

fn find(keys: &[Jwk], kid: &str) -> Option<Jwk> {
    keys.iter()
        .find(|key| key.common.key_id.as_deref() == Some(kid))
        .cloned()
}

/// The `max-age` of a `Cache-Control` value.
fn max_age(value: &str) -> Option<Duration> {
    value.split(',').find_map(|directive| {
        directive
            .trim()
            .strip_prefix("max-age=")
            .and_then(|seconds| seconds.trim().parse().ok())
            .map(Duration::from_secs)
    })
}

/// `value` as `application/x-www-form-urlencoded` writes it.
fn form_encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'*') {
            encoded.push(char::from(byte));
        } else if byte == b' ' {
            encoded.push('+');
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

/// `base` with the query parameters `params` added.
pub(crate) fn with_query(base: &str, params: &[(&str, &str)]) -> Option<String> {
    let mut url = Url::parse(base).ok()?;
    {
        let mut query = url.query_pairs_mut();
        for (name, value) in params {
            query.append_pair(name, value);
        }
    }
    Some(url.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_is_read_under_the_backchannel_with_the_issuers_path() {
        assert_eq!(
            discovery_url("https://id.example.invalid/realms/fixture/", None),
            "https://id.example.invalid/realms/fixture/.well-known/openid-configuration"
        );
        let backchannel = Url::parse("http://keycloak:8080").unwrap();
        assert_eq!(
            discovery_url(
                "https://id.example.invalid/realms/fixture",
                Some(&backchannel)
            ),
            "http://keycloak:8080/realms/fixture/.well-known/openid-configuration"
        );
        let rauthy = Url::parse("http://rauthy:8080").unwrap();
        assert_eq!(
            discovery_url("http://localhost:8180/auth/v1/", Some(&rauthy)),
            "http://rauthy:8080/auth/v1/.well-known/openid-configuration"
        );
    }

    #[test]
    fn the_jwks_lifetime_comes_from_max_age() {
        assert_eq!(
            max_age("public, max-age=600"),
            Some(Duration::from_secs(600))
        );
        assert_eq!(max_age("no-cache"), None);
    }

    #[test]
    fn client_credentials_are_form_encoded() {
        assert_eq!(form_encode("oxidgene-web"), "oxidgene-web");
        assert_eq!(form_encode("a b:c/é"), "a+b%3Ac%2F%C3%A9");
    }
}
