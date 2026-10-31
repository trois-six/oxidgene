//! Validation of the provider's tokens (docs/authentication.md §5.2, §5.3):
//! access tokens at every request, ID tokens at sign-in and refresh, logout
//! tokens on the back channel.
//!
//! A JWT is accepted only signed with an allow-listed asymmetric algorithm,
//! by the JWKS key its header names, of the matching type; `none` and HMAC
//! never verify, whatever the header says. Its `iss` is the issuer exactly,
//! its `exp`, `nbf` and `iat` hold with 60 seconds of leeway, and its `aud`
//! names the resource identifier for an access token, the web client for an
//! ID or logout token — so an ID token presented as an access token fails.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use jsonwebtoken::errors::ErrorKind;
use jsonwebtoken::jwk::{AlgorithmParameters, Jwk, PublicKeyUse};
use jsonwebtoken::{Algorithm, AlgorithmFamily, DecodingKey, Validation};
use serde_json::{Map, Value};

use super::provider::{Provider, ProviderError};

/// Tokens over this size are refused before they are parsed.
pub(crate) const MAX_TOKEN_BYTES: usize = 16 * 1024;
/// The clock skew tolerated on `exp`, `nbf` and `iat`.
const LEEWAY: u64 = 60;
/// The longest an introspection answer is reused.
const INTROSPECTION_LIFETIME: Duration = Duration::from_secs(60);

/// Which check a token failed: the `check` of the `token_invalid` log
/// category (§9.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TokenFailure {
    Signature,
    Issuer,
    Audience,
    Expired,
    Algorithm,
    /// Not a token of the expected shape or type, or missing a claim.
    Malformed,
    /// The provider could not be reached to read its keys or introspect.
    Unavailable,
}

impl TokenFailure {
    pub(crate) fn check(self) -> &'static str {
        match self {
            Self::Signature => "signature",
            Self::Issuer => "issuer",
            Self::Audience => "audience",
            Self::Expired => "expired",
            Self::Algorithm => "algorithm",
            Self::Malformed => "malformed",
            Self::Unavailable => "unavailable",
        }
    }
}

/// What a token is for: it decides the audience, the accepted `typ` and the
/// claims required.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Purpose {
    Access,
    Id,
    Logout,
}

impl Purpose {
    fn accepts_type(self, typ: Option<&str>) -> bool {
        let Some(typ) = typ else {
            return true;
        };
        let typ = typ.to_ascii_lowercase();
        match self {
            // RFC 9068.
            Self::Access => matches!(typ.as_str(), "jwt" | "at+jwt" | "application/at+jwt"),
            Self::Id => typ == "jwt",
            Self::Logout => matches!(typ.as_str(), "jwt" | "logout+jwt"),
        }
    }
}

/// The validator of one deployment: its issuer, audience, client and
/// algorithms.
#[derive(Debug)]
pub(crate) struct Validator {
    pub issuer: String,
    pub audience: String,
    pub client_id: String,
    pub algorithms: Vec<Algorithm>,
}

impl Validator {
    /// The claims of `token` once it passes every check for `purpose`.
    pub(crate) async fn validate(
        &self,
        provider: &Provider,
        token: &str,
        purpose: Purpose,
    ) -> Result<Map<String, Value>, TokenFailure> {
        if token.len() > MAX_TOKEN_BYTES {
            return Err(TokenFailure::Malformed);
        }
        let header = jsonwebtoken::decode_header(token).map_err(|_| TokenFailure::Malformed)?;
        if !self.algorithms.contains(&header.alg) {
            return Err(TokenFailure::Algorithm);
        }
        if !purpose.accepts_type(header.typ.as_deref()) {
            return Err(TokenFailure::Malformed);
        }
        let kid = header.kid.as_deref().ok_or(TokenFailure::Signature)?;
        let jwk = provider
            .key(kid)
            .await
            .map_err(|_| TokenFailure::Unavailable)?
            .ok_or(TokenFailure::Signature)?;
        let key = verifying_key(&jwk, header.alg).ok_or(TokenFailure::Signature)?;
        let audience = match purpose {
            Purpose::Access => &self.audience,
            Purpose::Id | Purpose::Logout => &self.client_id,
        };
        let mut validation = Validation::new(header.alg);
        validation.leeway = LEEWAY;
        validation.validate_nbf = true;
        validation.set_issuer(&[&self.issuer]);
        validation.set_audience(&[audience]);
        let mut required = vec!["iss", "aud"];
        if purpose != Purpose::Logout {
            required.extend(["exp", "sub"]);
        } else {
            // A logout token need not expire; it is checked if it does.
            validation.validate_exp = true;
        }
        validation.set_required_spec_claims(&required);
        let data = jsonwebtoken::decode::<Map<String, Value>>(token, &key, &validation)
            .map_err(|error| failure(error.kind()))?;
        let claims = data.claims;
        let iat = claims.get("iat").and_then(Value::as_i64);
        if purpose != Purpose::Access && iat.is_none() {
            return Err(TokenFailure::Malformed);
        }
        if iat.is_some_and(|iat| iat > super::store::now() + LEEWAY as i64) {
            return Err(TokenFailure::Expired);
        }
        Ok(claims)
    }
}

/// The category of a `jsonwebtoken` error.
fn failure(kind: &ErrorKind) -> TokenFailure {
    match kind {
        ErrorKind::InvalidSignature | ErrorKind::InvalidKeyFormat => TokenFailure::Signature,
        ErrorKind::InvalidIssuer => TokenFailure::Issuer,
        ErrorKind::InvalidAudience => TokenFailure::Audience,
        ErrorKind::ExpiredSignature | ErrorKind::ImmatureSignature => TokenFailure::Expired,
        ErrorKind::InvalidAlgorithm | ErrorKind::InvalidAlgorithmName => TokenFailure::Algorithm,
        _ => TokenFailure::Malformed,
    }
}

/// The key `jwk` verifies `algorithm` with: the key must be of the family
/// the algorithm belongs to, meant for signatures, and announce no other
/// algorithm.
fn verifying_key(jwk: &Jwk, algorithm: Algorithm) -> Option<DecodingKey> {
    let family = match &jwk.algorithm {
        AlgorithmParameters::RSA(_) => AlgorithmFamily::Rsa,
        AlgorithmParameters::EllipticCurve(_) => AlgorithmFamily::Ec,
        AlgorithmParameters::OctetKeyPair(_) => AlgorithmFamily::Ed,
        _ => return None,
    };
    if !family.algorithms().contains(&algorithm) {
        return None;
    }
    if matches!(&jwk.common.public_key_use, Some(usage) if *usage != PublicKeyUse::Signature) {
        return None;
    }
    if let Some(announced) = jwk.common.key_algorithm
        && format!("{announced:?}") != format!("{algorithm:?}")
    {
        return None;
    }
    DecodingKey::from_jwk(jwk).ok()
}

/// The identity a validated access token carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AccessIdentity {
    pub issuer: String,
    pub subject: String,
    /// `client_id`, else `azp`.
    pub client_id: Option<String>,
    pub scopes: Vec<String>,
    /// Unix seconds.
    pub expires_at: i64,
    /// The claims themselves, for the roles claim.
    pub claims: Map<String, Value>,
}

impl AccessIdentity {
    pub(crate) fn from_claims(claims: Map<String, Value>) -> Result<Self, TokenFailure> {
        let text = |name: &str| claims.get(name).and_then(Value::as_str).map(str::to_string);
        Ok(Self {
            issuer: text("iss").ok_or(TokenFailure::Malformed)?,
            subject: text("sub").ok_or(TokenFailure::Malformed)?,
            client_id: text("client_id").or_else(|| text("azp")),
            scopes: scopes(&claims),
            expires_at: claims
                .get("exp")
                .and_then(Value::as_i64)
                .ok_or(TokenFailure::Malformed)?,
            claims,
        })
    }
}

/// The scopes of `scope` (space-separated) or `scp` (a list, or a string).
pub(crate) fn scopes(claims: &Map<String, Value>) -> Vec<String> {
    match (claims.get("scope"), claims.get("scp")) {
        (Some(Value::String(scope)), _) | (None, Some(Value::String(scope))) => {
            scope.split_whitespace().map(str::to_string).collect()
        }
        (_, Some(Value::Array(scopes))) => scopes
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

/// Introspection answers, kept for the token's lifetime and at most a
/// minute, keyed by the token's digest: one token, one viewer.
#[derive(Debug, Default)]
pub(crate) struct IntrospectionCache {
    answers: Mutex<HashMap<String, (AccessIdentity, Instant)>>,
}

impl IntrospectionCache {
    /// The identity an opaque `token` carries, by introspection.
    pub(crate) async fn identity(
        &self,
        validator: &Validator,
        provider: &Provider,
        token: &str,
    ) -> Result<AccessIdentity, TokenFailure> {
        if token.len() > MAX_TOKEN_BYTES {
            return Err(TokenFailure::Malformed);
        }
        let key = super::crypto::digest(token);
        let now = Instant::now();
        if let Some((identity, until)) = self.lock().get(&key)
            && *until > now
        {
            return Ok(identity.clone());
        }
        let answer = provider
            .introspect(token)
            .await
            .map_err(|error| match error {
                ProviderError::Unreachable => TokenFailure::Unavailable,
                ProviderError::InvalidGrant | ProviderError::Refused => TokenFailure::Malformed,
            })?;
        if !answer.active {
            return Err(TokenFailure::Expired);
        }
        let claims = answer.claims;
        if claims.get("iss").and_then(Value::as_str) != Some(validator.issuer.as_str()) {
            return Err(TokenFailure::Issuer);
        }
        if !audience_contains(claims.get("aud"), &validator.audience) {
            return Err(TokenFailure::Audience);
        }
        let identity = AccessIdentity::from_claims(claims)?;
        let remaining = identity.expires_at - super::store::now();
        if remaining <= 0 {
            return Err(TokenFailure::Expired);
        }
        let lifetime =
            Duration::from_secs(u64::try_from(remaining).unwrap_or(0)).min(INTROSPECTION_LIFETIME);
        let mut answers = self.lock();
        answers.retain(|_, (_, until)| *until > now);
        answers.insert(key, (identity.clone(), now + lifetime));
        Ok(identity)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, (AccessIdentity, Instant)>> {
        self.answers
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }
}

fn audience_contains(aud: Option<&Value>, expected: &str) -> bool {
    match aud {
        Some(Value::String(aud)) => aud == expected,
        Some(Value::Array(auds)) => auds.iter().any(|aud| aud.as_str() == Some(expected)),
        _ => false,
    }
}

/// Whether `token` has the three parts of a JWS compact serialization.
pub(crate) fn is_jwt(token: &str) -> bool {
    token.bytes().filter(|byte| *byte == b'.').count() == 2
}

/// The string at the dotted `path` of `claims`, as a list: the roles claim
/// (§6.3). A list of strings is taken as is; an object, as Zitadel's
/// project roles, gives its keys; a lone string is one value.
pub(crate) fn claim_values(claims: &Map<String, Value>, path: &[String]) -> Option<Vec<String>> {
    let (first, rest) = path.split_first()?;
    let mut value = claims.get(first)?;
    for segment in rest {
        value = value.get(segment)?;
    }
    Some(match value {
        Value::Array(items) => items
            .iter()
            .filter_map(Value::as_str)
            .filter(|item| !item.is_empty())
            .map(str::to_string)
            .collect(),
        Value::Object(object) => object.keys().cloned().collect(),
        // A provider sends `[""]` for no group: no value.
        Value::String(single) if !single.is_empty() => vec![single.clone()],
        _ => Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn map(value: Value) -> Map<String, Value> {
        value.as_object().cloned().unwrap()
    }

    #[test]
    fn scopes_come_from_scope_or_scp() {
        assert_eq!(
            scopes(&map(json!({"scope": "openid tree:read"}))),
            ["openid", "tree:read"]
        );
        assert_eq!(scopes(&map(json!({"scp": ["tree:read"]}))), ["tree:read"]);
        assert_eq!(
            scopes(&map(json!({"scp": "tree:read export"}))),
            ["tree:read", "export"]
        );
        assert!(scopes(&map(json!({}))).is_empty());
    }

    #[test]
    fn the_roles_claim_is_read_by_its_dotted_path() {
        let claims = map(json!({
            "groups": ["oxidgene-admins", ""],
            "realm_access": {"roles": ["creator", 7]},
            "urn:zitadel:iam:org:project:roles": {"admin": {"org": "x"}},
        }));
        let path = |p: &str| p.split('.').map(str::to_string).collect::<Vec<_>>();
        assert_eq!(
            claim_values(&claims, &path("groups")),
            Some(vec!["oxidgene-admins".into()])
        );
        assert_eq!(
            claim_values(&claims, &path("realm_access.roles")),
            Some(vec!["creator".into()])
        );
        assert_eq!(
            claim_values(&claims, &["urn:zitadel:iam:org:project:roles".to_string()]),
            Some(vec!["admin".into()])
        );
        assert_eq!(claim_values(&claims, &path("missing.roles")), None);
    }

    #[test]
    fn each_purpose_takes_its_token_types() {
        assert!(Purpose::Access.accepts_type(Some("at+jwt")));
        assert!(Purpose::Access.accepts_type(Some("JWT")));
        assert!(Purpose::Access.accepts_type(None));
        assert!(!Purpose::Access.accepts_type(Some("logout+jwt")));
        assert!(!Purpose::Id.accepts_type(Some("at+jwt")));
        assert!(Purpose::Logout.accepts_type(Some("logout+jwt")));
    }

    #[test]
    fn a_jwt_has_three_parts() {
        assert!(is_jwt("a.b.c"));
        assert!(!is_jwt("opaque-token"));
        assert!(!is_jwt("a.b.c.d.e"));
    }
}
