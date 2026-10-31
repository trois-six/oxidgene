//! The authentication settings of the web build (docs/authentication.md
//! §3.2): read raw from `oxidgene.toml` and the `OXIDGENE_*` environment by
//! the server, then checked once into an [`AuthConfig`].
//!
//! A missing or invalid value fails with a stable category and the name of
//! the variable, never its value: secrets and URLs with credentials must not
//! reach a log.

use std::time::Duration;

use base64::Engine as _;
use jsonwebtoken::{Algorithm, EncodingKey};
use reqwest::Url;
use serde::Deserialize;

use crate::service::account::{SignupPolicy, SignupRule, TreeCreationRule};

/// Why the settings cannot be used: a stable category, `auth_config_missing`
/// or `auth_config_invalid`, and the variable at fault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthConfigError {
    pub category: &'static str,
    pub variable: &'static str,
}

impl AuthConfigError {
    fn missing(variable: &'static str) -> Self {
        Self {
            category: "auth_config_missing",
            variable,
        }
    }

    fn invalid(variable: &'static str) -> Self {
        Self {
            category: "auth_config_invalid",
            variable,
        }
    }
}

impl std::fmt::Display for AuthConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.category, self.variable)
    }
}

impl std::error::Error for AuthConfigError {}

/// The settings as written: every field is the `OXIDGENE_` variable of the
/// same name, lower-cased. A secret may instead name a mounted file through
/// its `_FILE` twin.
#[derive(Clone, Deserialize)]
pub struct AuthSettings {
    #[serde(default = "default_mode")]
    pub auth_mode: String,
    #[serde(default)]
    pub public_url: Option<String>,
    #[serde(default)]
    pub oidc_issuer: Option<String>,
    #[serde(default)]
    pub oidc_backchannel_url: Option<String>,
    #[serde(default = "default_client_id")]
    pub oidc_client_id: String,
    #[serde(default = "default_client_auth")]
    pub oidc_client_auth: String,
    #[serde(default)]
    pub oidc_client_secret: Option<String>,
    #[serde(default)]
    pub oidc_client_secret_file: Option<String>,
    #[serde(default)]
    pub oidc_private_key_file: Option<String>,
    #[serde(default = "default_scopes")]
    pub oidc_scopes: String,
    #[serde(default)]
    pub oidc_audience: Option<String>,
    #[serde(default = "default_algorithms")]
    pub oidc_algorithms: String,
    #[serde(default)]
    pub oidc_trusted_clients: Option<String>,
    #[serde(default)]
    pub oidc_introspection: bool,
    #[serde(default)]
    pub oidc_roles_claim: Option<String>,
    #[serde(default)]
    pub oidc_admin_values: Option<String>,
    #[serde(default)]
    pub oidc_creator_values: Option<String>,
    #[serde(default)]
    pub oidc_allowed_values: Option<String>,
    #[serde(default)]
    pub oidc_trust_email: bool,
    #[serde(default)]
    pub oidc_account_url: Option<String>,
    #[serde(default = "default_signup")]
    pub auth_signup: String,
    #[serde(default = "default_tree_creation")]
    pub auth_tree_creation: String,
    #[serde(default)]
    pub auth_bootstrap_admin_email: Option<String>,
    #[serde(default = "default_session_store")]
    pub session_store: String,
    #[serde(default)]
    pub redis_url: Option<String>,
    #[serde(default)]
    pub redis_url_file: Option<String>,
    #[serde(default)]
    pub session_keys: Option<String>,
    #[serde(default)]
    pub session_keys_file: Option<String>,
    #[serde(default = "default_idle_timeout")]
    pub session_idle_timeout: String,
    #[serde(default = "default_absolute_lifetime")]
    pub session_absolute_lifetime: String,
}

impl std::fmt::Debug for AuthSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Only the mode: the rest holds secrets and addresses.
        f.debug_struct("AuthSettings")
            .field("auth_mode", &self.auth_mode)
            .finish_non_exhaustive()
    }
}

fn default_mode() -> String {
    "oidc".into()
}
fn default_client_id() -> String {
    "oxidgene-web".into()
}
fn default_client_auth() -> String {
    "client_secret_basic".into()
}
fn default_scopes() -> String {
    "openid profile email".into()
}
fn default_algorithms() -> String {
    "RS256,PS256,ES256,EdDSA".into()
}
fn default_signup() -> String {
    "invitation".into()
}
fn default_tree_creation() -> String {
    "everyone".into()
}
fn default_session_store() -> String {
    "memory".into()
}
fn default_idle_timeout() -> String {
    "2h".into()
}
fn default_absolute_lifetime() -> String {
    "7d".into()
}

/// The checked configuration.
#[derive(Debug, Clone)]
pub enum AuthConfig {
    /// No authentication: every request acts as the local owner of every
    /// tree (docs/authentication.md §2).
    None,
    /// OpenID Connect against one provider.
    Oidc(Box<OidcConfig>),
}

/// How the web client authenticates to the provider's token endpoint.
#[derive(Clone)]
pub enum ClientAuth {
    /// `client_secret_basic`, the default (open decision 5).
    SecretBasic(String),
    /// `client_secret_post`: the secret in the form body, for providers that
    /// refuse HTTP Basic.
    SecretPost(String),
    /// `private_key_jwt` (RFC 7523): a client assertion signed with this key.
    PrivateKeyJwt(EncodingKey, Algorithm),
}

impl std::fmt::Debug for ClientAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SecretBasic(_) => f.write_str("SecretBasic(..)"),
            Self::SecretPost(_) => f.write_str("SecretPost(..)"),
            Self::PrivateKeyJwt(_, algorithm) => write!(f, "PrivateKeyJwt({algorithm:?})"),
        }
    }
}

/// Where sessions live.
#[derive(Clone)]
pub enum StoreKind {
    Memory,
    Redis(String),
}

impl std::fmt::Debug for StoreKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Memory => f.write_str("Memory"),
            Self::Redis(_) => f.write_str("Redis(..)"),
        }
    }
}

/// The store, keys, idle timeout and absolute lifetime of sessions.
type SessionSettings = (StoreKind, Vec<[u8; 32]>, Duration, Duration);

/// The checked `oidc` configuration.
#[derive(Debug, Clone)]
pub struct OidcConfig {
    /// The application's external origin, without a trailing slash.
    pub public_url: String,
    /// `scheme://host[:port]` of [`Self::public_url`], as browsers send it in
    /// `Origin`.
    pub public_origin: String,
    pub issuer: String,
    pub backchannel_url: Option<Url>,
    pub client_id: String,
    pub client_auth: ClientAuth,
    pub scopes: String,
    pub audience: String,
    pub algorithms: Vec<Algorithm>,
    pub trusted_clients: Vec<String>,
    pub introspection: bool,
    /// The dotted path of the roles claim, split.
    pub roles_claim: Option<Vec<String>>,
    pub account_url: Option<String>,
    pub signup: SignupPolicy,
    pub store: StoreKind,
    /// The keys sealing a session's tokens; the first seals.
    pub session_keys: Vec<[u8; 32]>,
    pub idle_timeout: Duration,
    pub absolute_lifetime: Duration,
}

impl OidcConfig {
    /// The redirect URI registered with the provider.
    pub fn redirect_uri(&self) -> String {
        format!("{}/auth/callback", self.public_url)
    }

    /// Whether cookies may be marked `Secure`: always, `localhost` being a
    /// secure context to browsers (§4.4). Kept a method so a test can see
    /// the rule in one place.
    pub fn uses_https(&self) -> bool {
        self.public_url.starts_with("https://")
    }
}

impl AuthSettings {
    /// Check the settings.
    ///
    /// # Errors
    ///
    /// The category and variable of the first missing or invalid value.
    pub fn resolve(&self) -> Result<AuthConfig, AuthConfigError> {
        match self.auth_mode.trim() {
            "none" => Ok(AuthConfig::None),
            "oidc" => self
                .resolve_oidc()
                .map(|oidc| AuthConfig::Oidc(Box::new(oidc))),
            _ => Err(AuthConfigError::invalid("OXIDGENE_AUTH_MODE")),
        }
    }

    fn resolve_oidc(&self) -> Result<OidcConfig, AuthConfigError> {
        let (public_url, public_origin) = public_url(self.public_url.as_deref())?;
        let issuer = self.issuer()?;
        let (store, session_keys, idle_timeout, absolute_lifetime) = self.sessions()?;
        Ok(OidcConfig {
            audience: optional(&self.oidc_audience).unwrap_or_else(|| public_url.clone()),
            public_url,
            public_origin,
            issuer,
            backchannel_url: self.backchannel_url()?,
            client_id: self.client_id()?,
            client_auth: self.client_auth()?,
            scopes: scopes(&self.oidc_scopes)?,
            algorithms: algorithms(&self.oidc_algorithms)?,
            trusted_clients: list(&self.oidc_trusted_clients),
            introspection: self.oidc_introspection,
            roles_claim: self.roles_claim()?,
            account_url: self.account_url()?,
            signup: self.signup_policy()?,
            store,
            session_keys,
            idle_timeout,
            absolute_lifetime,
        })
    }

    /// The session store, the keys sealing sessions' tokens, and the idle
    /// and absolute lifetimes.
    fn sessions(&self) -> Result<SessionSettings, AuthConfigError> {
        let keys = secret(
            &self.session_keys,
            &self.session_keys_file,
            "OXIDGENE_SESSION_KEYS",
        )?;
        Ok((
            self.store()?,
            session_keys(&keys)?,
            lifetime(&self.session_idle_timeout, "OXIDGENE_SESSION_IDLE_TIMEOUT")?,
            lifetime(
                &self.session_absolute_lifetime,
                "OXIDGENE_SESSION_ABSOLUTE_LIFETIME",
            )?,
        ))
    }

    /// The issuer, compared exactly — a trailing slash included.
    fn issuer(&self) -> Result<String, AuthConfigError> {
        let issuer = required(&self.oidc_issuer, "OXIDGENE_OIDC_ISSUER")?;
        Url::parse(&issuer).map_err(|_| AuthConfigError::invalid("OXIDGENE_OIDC_ISSUER"))?;
        Ok(issuer)
    }

    fn client_id(&self) -> Result<String, AuthConfigError> {
        let client_id = self.oidc_client_id.trim();
        if client_id.is_empty() {
            return Err(AuthConfigError::invalid("OXIDGENE_OIDC_CLIENT_ID"));
        }
        Ok(client_id.to_string())
    }

    fn backchannel_url(&self) -> Result<Option<Url>, AuthConfigError> {
        optional(&self.oidc_backchannel_url)
            .map(|url| {
                Url::parse(&url)
                    .map_err(|_| AuthConfigError::invalid("OXIDGENE_OIDC_BACKCHANNEL_URL"))
            })
            .transpose()
    }

    /// The dotted path of the roles claim, split.
    fn roles_claim(&self) -> Result<Option<Vec<String>>, AuthConfigError> {
        let path = optional(&self.oidc_roles_claim)
            .map(|path| path.split('.').map(str::to_string).collect::<Vec<_>>());
        if path
            .as_ref()
            .is_some_and(|path| path.iter().any(String::is_empty))
        {
            return Err(AuthConfigError::invalid("OXIDGENE_OIDC_ROLES_CLAIM"));
        }
        Ok(path)
    }

    fn account_url(&self) -> Result<Option<String>, AuthConfigError> {
        let url = optional(&self.oidc_account_url);
        if url
            .as_deref()
            .is_some_and(|url| !(url.starts_with("https://") || url.starts_with("http://")))
        {
            return Err(AuthConfigError::invalid("OXIDGENE_OIDC_ACCOUNT_URL"));
        }
        Ok(url)
    }

    fn client_auth(&self) -> Result<ClientAuth, AuthConfigError> {
        match self.oidc_client_auth.trim() {
            "client_secret_basic" => secret(
                &self.oidc_client_secret,
                &self.oidc_client_secret_file,
                "OXIDGENE_OIDC_CLIENT_SECRET",
            )
            .map(ClientAuth::SecretBasic),
            "client_secret_post" => secret(
                &self.oidc_client_secret,
                &self.oidc_client_secret_file,
                "OXIDGENE_OIDC_CLIENT_SECRET",
            )
            .map(ClientAuth::SecretPost),
            "private_key_jwt" => {
                let path = required(
                    &self.oidc_private_key_file,
                    "OXIDGENE_OIDC_PRIVATE_KEY_FILE",
                )?;
                let pem = std::fs::read(path)
                    .map_err(|_| AuthConfigError::invalid("OXIDGENE_OIDC_PRIVATE_KEY_FILE"))?;
                signing_key(&pem)
                    .ok_or(AuthConfigError::invalid("OXIDGENE_OIDC_PRIVATE_KEY_FILE"))
                    .map(|(key, algorithm)| ClientAuth::PrivateKeyJwt(key, algorithm))
            }
            _ => Err(AuthConfigError::invalid("OXIDGENE_OIDC_CLIENT_AUTH")),
        }
    }

    fn signup_policy(&self) -> Result<SignupPolicy, AuthConfigError> {
        let signup = match self.auth_signup.trim() {
            "invitation" => SignupRule::Invitation,
            "claim" => SignupRule::Claim,
            "open" => SignupRule::Open,
            _ => return Err(AuthConfigError::invalid("OXIDGENE_AUTH_SIGNUP")),
        };
        let tree_creation = match self.auth_tree_creation.trim() {
            "everyone" => TreeCreationRule::Everyone,
            "claim" => TreeCreationRule::Claim,
            _ => return Err(AuthConfigError::invalid("OXIDGENE_AUTH_TREE_CREATION")),
        };
        let roles_claim = optional(&self.oidc_roles_claim).is_some();
        // A rule reading the roles claim needs a claim to read.
        if !roles_claim && (signup == SignupRule::Claim || tree_creation == TreeCreationRule::Claim)
        {
            return Err(AuthConfigError::missing("OXIDGENE_OIDC_ROLES_CLAIM"));
        }
        Ok(SignupPolicy {
            signup,
            tree_creation,
            roles_claim,
            admin_values: list(&self.oidc_admin_values),
            creator_values: list(&self.oidc_creator_values),
            allowed_values: list(&self.oidc_allowed_values),
            trust_email: self.oidc_trust_email,
            bootstrap_admin_email: optional(&self.auth_bootstrap_admin_email),
        })
    }

    fn store(&self) -> Result<StoreKind, AuthConfigError> {
        match self.session_store.trim() {
            "memory" => Ok(StoreKind::Memory),
            "redis" => {
                let url = secret(&self.redis_url, &self.redis_url_file, "OXIDGENE_REDIS_URL")?;
                if !(url.starts_with("redis://") || url.starts_with("rediss://")) {
                    return Err(AuthConfigError::invalid("OXIDGENE_REDIS_URL"));
                }
                Ok(StoreKind::Redis(url))
            }
            _ => Err(AuthConfigError::invalid("OXIDGENE_SESSION_STORE")),
        }
    }
}

/// `value` trimmed, or `None` when unset or blank.
fn optional(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn required(value: &Option<String>, variable: &'static str) -> Result<String, AuthConfigError> {
    optional(value).ok_or(AuthConfigError::missing(variable))
}

/// A secret given directly, or read from the file its `_FILE` twin names.
fn secret(
    value: &Option<String>,
    file: &Option<String>,
    variable: &'static str,
) -> Result<String, AuthConfigError> {
    if let Some(value) = optional(value) {
        return Ok(value);
    }
    let Some(path) = optional(file) else {
        return Err(AuthConfigError::missing(variable));
    };
    std::fs::read_to_string(path)
        .ok()
        .map(|content| content.trim().to_string())
        .filter(|content| !content.is_empty())
        .ok_or(AuthConfigError::invalid(variable))
}

/// The comma-separated values of `value`, trimmed, blanks dropped.
fn list(value: &Option<String>) -> Vec<String> {
    value
        .as_deref()
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

/// The public URL without its trailing slash, and its origin. It must be an
/// `http(s)` origin with no path, query or credentials: it is the redirect
/// URI's base and what `Origin` headers are compared with.
fn public_url(value: Option<&str>) -> Result<(String, String), AuthConfigError> {
    const VARIABLE: &str = "OXIDGENE_PUBLIC_URL";
    let raw = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(AuthConfigError::missing(VARIABLE))?;
    let url = Url::parse(raw).map_err(|_| AuthConfigError::invalid(VARIABLE))?;
    let plain = matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.path() == "/"
        && url.query().is_none()
        && url.fragment().is_none();
    if !plain {
        return Err(AuthConfigError::invalid(VARIABLE));
    }
    let origin = url.origin().ascii_serialization();
    Ok((origin.clone(), origin))
}

/// The scopes asked at sign-in; `openid` is required.
fn scopes(value: &str) -> Result<String, AuthConfigError> {
    let scopes: Vec<&str> = value.split_whitespace().collect();
    if !scopes.contains(&"openid") {
        return Err(AuthConfigError::invalid("OXIDGENE_OIDC_SCOPES"));
    }
    Ok(scopes.join(" "))
}

/// The signature allow-list. `none` and every HMAC algorithm are refused
/// whatever the configuration says (§5.2).
fn algorithms(value: &str) -> Result<Vec<Algorithm>, AuthConfigError> {
    let invalid = AuthConfigError::invalid("OXIDGENE_OIDC_ALGORITHMS");
    let mut algorithms = Vec::new();
    for name in value
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        let algorithm = match name {
            "RS256" => Algorithm::RS256,
            "RS384" => Algorithm::RS384,
            "RS512" => Algorithm::RS512,
            "PS256" => Algorithm::PS256,
            "PS384" => Algorithm::PS384,
            "PS512" => Algorithm::PS512,
            "ES256" => Algorithm::ES256,
            "ES384" => Algorithm::ES384,
            "EdDSA" => Algorithm::EdDSA,
            _ => return Err(invalid),
        };
        algorithms.push(algorithm);
    }
    if algorithms.is_empty() {
        return Err(invalid);
    }
    Ok(algorithms)
}

/// The keys of `OXIDGENE_SESSION_KEYS`: comma-separated base64 (standard or
/// URL-safe, padded or not) of 32 bytes each.
fn session_keys(value: &str) -> Result<Vec<[u8; 32]>, AuthConfigError> {
    let invalid = AuthConfigError::invalid("OXIDGENE_SESSION_KEYS");
    let engines = [
        base64::engine::general_purpose::STANDARD,
        base64::engine::general_purpose::STANDARD_NO_PAD,
        base64::engine::general_purpose::URL_SAFE,
        base64::engine::general_purpose::URL_SAFE_NO_PAD,
    ];
    let mut keys = Vec::new();
    for encoded in value
        .split(',')
        .map(str::trim)
        .filter(|key| !key.is_empty())
    {
        let bytes = engines
            .iter()
            .find_map(|engine| engine.decode(encoded).ok())
            .ok_or(invalid)?;
        keys.push(<[u8; 32]>::try_from(bytes.as_slice()).map_err(|_| invalid)?);
    }
    if keys.is_empty() {
        return Err(invalid);
    }
    Ok(keys)
}

/// The session lifetime `variable` sets.
fn lifetime(value: &str, variable: &'static str) -> Result<Duration, AuthConfigError> {
    duration(value).ok_or(AuthConfigError::invalid(variable))
}

/// A duration written as a whole number and a unit: `90s`, `30m`, `2h`,
/// `7d`. At least a minute.
pub(crate) fn duration(value: &str) -> Option<Duration> {
    let value = value.trim();
    let split = value.len().checked_sub(1)?;
    let (number, unit) = value.split_at(split);
    let number: u64 = number.parse().ok()?;
    let seconds = match unit {
        "s" => number,
        "m" => number.checked_mul(60)?,
        "h" => number.checked_mul(3600)?,
        "d" => number.checked_mul(86_400)?,
        _ => return None,
    };
    (seconds >= 60).then(|| Duration::from_secs(seconds))
}

/// The key a PEM file holds, and the algorithm it signs with: RSA keys sign
/// `RS256`, P-256 keys `ES256`, Ed25519 keys `EdDSA`.
fn signing_key(pem: &[u8]) -> Option<(EncodingKey, Algorithm)> {
    let text = std::str::from_utf8(pem).ok()?;
    if text.contains("RSA PRIVATE KEY") {
        return EncodingKey::from_rsa_pem(pem)
            .ok()
            .map(|key| (key, Algorithm::RS256));
    }
    if text.contains("EC PRIVATE KEY") {
        return EncodingKey::from_ec_pem(pem)
            .ok()
            .map(|key| (key, Algorithm::ES256));
    }
    // A PKCS #8 key says nothing of its type in its armour: try each.
    EncodingKey::from_ed_pem(pem)
        .ok()
        .map(|key| (key, Algorithm::EdDSA))
        .or_else(|| {
            EncodingKey::from_ec_pem(pem)
                .ok()
                .map(|key| (key, Algorithm::ES256))
        })
        .or_else(|| {
            EncodingKey::from_rsa_pem(pem)
                .ok()
                .map(|key| (key, Algorithm::RS256))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(pairs: &[(&str, &str)]) -> AuthSettings {
        let mut object = serde_json::Map::new();
        for (key, value) in pairs {
            object.insert(
                (*key).to_string(),
                serde_json::Value::String((*value).into()),
            );
        }
        serde_json::from_value(serde_json::Value::Object(object)).expect("settings")
    }

    const KEY: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";

    fn oidc(extra: &[(&str, &str)]) -> Result<OidcConfig, AuthConfigError> {
        let mut pairs = vec![
            ("public_url", "https://genealogy.example.invalid"),
            ("oidc_issuer", "https://id.example.invalid/realms/fixture"),
            ("oidc_client_secret", "fixture-secret"),
            ("session_keys", KEY),
        ];
        pairs.extend_from_slice(extra);
        match settings(&pairs).resolve()? {
            AuthConfig::Oidc(config) => Ok(*config),
            AuthConfig::None => panic!("oidc expected"),
        }
    }

    #[test]
    fn oidc_is_the_default_and_needs_an_issuer() {
        let error = settings(&[]).resolve().unwrap_err();
        assert_eq!(error.category, "auth_config_missing");
        assert_eq!(error.variable, "OXIDGENE_PUBLIC_URL");
        let error = settings(&[("public_url", "https://genealogy.example.invalid")])
            .resolve()
            .unwrap_err();
        assert_eq!(error, AuthConfigError::missing("OXIDGENE_OIDC_ISSUER"));
        assert!(matches!(
            settings(&[("auth_mode", "none")]).resolve(),
            Ok(AuthConfig::None)
        ));
        assert_eq!(
            settings(&[("auth_mode", "magic")]).resolve().unwrap_err(),
            AuthConfigError::invalid("OXIDGENE_AUTH_MODE")
        );
    }

    #[test]
    fn defaults_follow_the_specification() {
        let config = oidc(&[]).expect("valid");
        assert_eq!(config.public_url, "https://genealogy.example.invalid");
        assert_eq!(config.audience, "https://genealogy.example.invalid");
        assert_eq!(config.client_id, "oxidgene-web");
        assert_eq!(config.scopes, "openid profile email");
        assert_eq!(
            config.algorithms,
            [
                Algorithm::RS256,
                Algorithm::PS256,
                Algorithm::ES256,
                Algorithm::EdDSA
            ]
        );
        assert!(matches!(config.client_auth, ClientAuth::SecretBasic(_)));
        assert!(matches!(config.store, StoreKind::Memory));
        assert_eq!(config.idle_timeout, Duration::from_secs(2 * 3600));
        assert_eq!(config.absolute_lifetime, Duration::from_secs(7 * 86_400));
        assert_eq!(config.signup.signup, SignupRule::Invitation);
        assert_eq!(config.signup.tree_creation, TreeCreationRule::Everyone);
        assert_eq!(
            config.redirect_uri(),
            "https://genealogy.example.invalid/auth/callback"
        );
    }

    #[test]
    fn hmac_and_none_never_join_the_allow_list() {
        for list in ["HS256", "RS256,HS512", "none", ""] {
            assert_eq!(
                oidc(&[("oidc_algorithms", list)]).unwrap_err(),
                AuthConfigError::invalid("OXIDGENE_OIDC_ALGORITHMS"),
                "{list}"
            );
        }
    }

    #[test]
    fn the_public_url_is_a_bare_origin() {
        for url in [
            "genealogy.example.invalid",
            "ftp://genealogy.example.invalid",
            "https://genealogy.example.invalid/app",
            "https://user:secret@genealogy.example.invalid",
        ] {
            assert_eq!(
                oidc(&[("public_url", url)]).unwrap_err(),
                AuthConfigError::invalid("OXIDGENE_PUBLIC_URL"),
                "{url}"
            );
        }
        let config = oidc(&[("public_url", "http://localhost:8081/")]).expect("valid");
        assert_eq!(config.public_url, "http://localhost:8081");
        assert_eq!(config.public_origin, "http://localhost:8081");
    }

    #[test]
    fn session_keys_are_32_bytes_of_base64() {
        assert_eq!(
            oidc(&[("session_keys", "c2hvcnQ=")]).unwrap_err(),
            AuthConfigError::invalid("OXIDGENE_SESSION_KEYS")
        );
        let rotated = format!("{KEY}, {}", KEY.trim_end_matches('='));
        assert_eq!(
            oidc(&[("session_keys", &rotated)])
                .expect("valid")
                .session_keys
                .len(),
            2
        );
    }

    #[test]
    fn secrets_may_come_from_a_file() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("secret");
        std::fs::write(&path, "from-file\n").expect("write");
        let config = settings(&[
            ("public_url", "https://genealogy.example.invalid"),
            ("oidc_issuer", "https://id.example.invalid"),
            ("oidc_client_secret_file", path.to_str().unwrap()),
            ("session_keys", KEY),
        ])
        .resolve()
        .expect("valid");
        let AuthConfig::Oidc(config) = config else {
            panic!("oidc expected")
        };
        assert!(
            matches!(&config.client_auth, ClientAuth::SecretBasic(secret) if secret == "from-file")
        );
    }

    #[test]
    fn claim_rules_need_the_roles_claim() {
        assert_eq!(
            oidc(&[("auth_signup", "claim")]).unwrap_err(),
            AuthConfigError::missing("OXIDGENE_OIDC_ROLES_CLAIM")
        );
        let config = oidc(&[
            ("auth_signup", "claim"),
            ("oidc_roles_claim", "realm_access.roles"),
            ("oidc_allowed_values", "members, admins"),
        ])
        .expect("valid");
        assert_eq!(
            config.roles_claim,
            Some(vec!["realm_access".to_string(), "roles".to_string()])
        );
        assert_eq!(config.signup.allowed_values, ["members", "admins"]);
    }

    #[test]
    fn the_redis_store_needs_a_redis_url() {
        assert_eq!(
            oidc(&[("session_store", "redis")]).unwrap_err(),
            AuthConfigError::missing("OXIDGENE_REDIS_URL")
        );
        assert!(matches!(
            oidc(&[
                ("session_store", "redis"),
                ("redis_url", "redis://cache:6379")
            ])
            .expect("valid")
            .store,
            StoreKind::Redis(_)
        ));
    }

    #[test]
    fn durations_take_a_unit_and_at_least_a_minute() {
        assert_eq!(duration("90s"), Some(Duration::from_secs(90)));
        assert_eq!(duration("30m"), Some(Duration::from_secs(1800)));
        assert_eq!(duration("7d"), Some(Duration::from_secs(604_800)));
        for value in ["", "2", "h", "10s", "2w", "-1h"] {
            assert_eq!(duration(value), None, "{value}");
        }
    }

    #[test]
    fn errors_name_the_variable_never_the_value() {
        let error = oidc(&[("oidc_issuer", "not a url with secret-value")]).unwrap_err();
        let printed = error.to_string();
        assert_eq!(printed, "auth_config_invalid: OXIDGENE_OIDC_ISSUER");
        let debug = format!("{:?}", settings(&[("oidc_client_secret", "secret-value")]));
        assert!(!debug.contains("secret-value"), "{debug}");
    }
}
