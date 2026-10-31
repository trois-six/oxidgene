//! A mock OpenID Connect provider (docs/authentication.md §12): an Axum
//! router on a loopback port serving discovery, the JWKS, the token,
//! end-session and introspection endpoints, signing with keys generated in
//! the test. No external service. Its tokens are shaped as Rauthy, the
//! provider of the development stack, issues them: EdDSA, `at+jwt` access
//! tokens naming the web client in `azp` and in `aud` beside the resource,
//! no `client_id`, `groups` with `""` for none.
//!
//! The authorization endpoint is not served over HTTP: a test hands the URL
//! the application redirected to to [`MockProvider::authorize`], which
//! plays the user signing in and answers the callback URL.
//!
//! A [`Browser`] drives the application as a cookie-keeping client.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aws_lc_rs::signature::{Ed25519KeyPair, KeyPair};
use axum::body::Body;
use axum::extract::{Form, State};
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use http_body_util::BodyExt;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use oxidgene_api::AppState;
use oxidgene_api::auth::{Auth, AuthConfig, AuthSettings, MemoryStore};
use oxidgene_db::sea_orm::DatabaseConnection;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tower::ServiceExt as _;

use super::{offline, test_media_root};

/// The web client of the tests.
pub const CLIENT_ID: &str = "oxidgene-web";
pub const CLIENT_SECRET: &str = "fixture-client-secret";
/// A client trusted for bearer tokens.
pub const TRUSTED_CLIENT: &str = "oxidgene-mcp";
/// The application's public URL in the tests.
pub const PUBLIC_URL: &str = "https://genealogy.example.invalid";

/// A user of the mock provider: fictitious claims.
#[derive(Clone, Debug)]
pub struct MockUser {
    pub subject: String,
    pub claims: Value,
}

impl MockUser {
    /// A user with a verified address `name@example.invalid`.
    pub fn new(name: &str) -> Self {
        Self {
            subject: format!("subject-{name}-{}", uuid::Uuid::now_v7().simple()),
            claims: json!({
                "email": format!("{name}@example.invalid"),
                "email_verified": true,
                "name": format!("Fictive {name}"),
                "preferred_username": name,
                "locale": "fr-FR",
            }),
        }
    }

    pub fn email(&self) -> String {
        self.claims["email"].as_str().unwrap().to_string()
    }

    /// The same user with `claim` set to `value`.
    pub fn with(mut self, claim: &str, value: Value) -> Self {
        self.claims[claim] = value;
        self
    }
}

#[derive(Clone)]
struct Grant {
    user: MockUser,
    nonce: Option<String>,
    challenge: String,
    sid: String,
}

/// What a test can tune or observe.
#[derive(Default)]
pub struct Controls {
    /// Seconds an access token lives.
    pub access_lifetime: i64,
    /// The refresh grant answers `invalid_grant`.
    pub refuse_refresh: bool,
    /// The token endpoint answers `503`.
    pub token_endpoint_down: bool,
    /// How long a refresh takes, to make concurrent requests overlap.
    pub refresh_delay: Duration,
    pub refreshes: usize,
    /// A refresh token presented twice: the reuse a provider punishes.
    pub refresh_reuses: usize,
    pub jwks_fetches: usize,
    /// Opaque tokens and the identity they introspect to.
    pub opaque: HashMap<String, Value>,
}

struct Shared {
    issuer: String,
    key: EncodingKey,
    jwk: Value,
    codes: Mutex<HashMap<String, Grant>>,
    refresh: Mutex<HashMap<String, Grant>>,
    used: Mutex<HashSet<String>>,
    /// The provider session of each subject's last sign-in.
    sids: Mutex<HashMap<String, String>>,
    controls: Mutex<Controls>,
}

/// The running mock provider.
#[derive(Clone)]
pub struct MockProvider {
    shared: Arc<Shared>,
}

pub const KID: &str = "test-key-1";

impl MockProvider {
    /// Start a provider on a loopback port.
    pub async fn start() -> Self {
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&aws_lc_rs::rand::SystemRandom::new())
            .expect("an Ed25519 key");
        let pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).expect("the key");
        let jwk = json!({
            "kty": "OKP",
            "crv": "Ed25519",
            "x": URL_SAFE_NO_PAD.encode(pair.public_key().as_ref()),
            "kid": KID,
            "use": "sig",
            "alg": "EdDSA",
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback port");
        // With the trailing slash Rauthy's issuer has.
        let issuer = format!("http://{}/auth/v1/", listener.local_addr().unwrap());
        let shared = Arc::new(Shared {
            issuer,
            key: EncodingKey::from_ed_der(pkcs8.as_ref()),
            jwk,
            codes: Mutex::default(),
            refresh: Mutex::default(),
            used: Mutex::default(),
            sids: Mutex::default(),
            controls: Mutex::new(Controls {
                access_lifetime: 300,
                ..Controls::default()
            }),
        });
        let router = Router::new()
            .route("/auth/v1/.well-known/openid-configuration", get(discovery))
            .route("/auth/v1/oidc/certs", get(jwks))
            .route("/auth/v1/oidc/token", post(token))
            .route("/auth/v1/oidc/introspect", post(introspect))
            .with_state(Arc::clone(&shared));
        tokio::spawn(async move {
            axum::serve(listener, router).await.ok();
        });
        Self { shared }
    }

    pub fn issuer(&self) -> &str {
        &self.shared.issuer
    }

    /// Change or read the controls.
    pub fn controls<T>(&self, f: impl FnOnce(&mut Controls) -> T) -> T {
        f(&mut self.shared.controls.lock().unwrap())
    }

    /// Sign `claims` with the provider's key, under `header`.
    pub fn sign(&self, header: &Header, claims: &Value) -> String {
        jsonwebtoken::encode(header, claims, &self.shared.key).expect("signed")
    }

    /// The provider's access-token header: EdDSA, `at+jwt`, its `kid`.
    pub fn header(&self) -> Header {
        let mut header = Header::new(Algorithm::EdDSA);
        header.typ = Some("at+jwt".into());
        header.kid = Some(KID.into());
        header
    }

    /// An access token for `subject` issued to `client`, valid for
    /// `lifetime` seconds from now (negative: expired).
    pub fn access_token(&self, subject: &str, client: &str, lifetime: i64) -> String {
        self.sign(
            &self.header(),
            &access_claims(&self.shared.issuer, subject, client, lifetime),
        )
    }

    /// Play the user `user` signing in at the authorization URL the
    /// application redirected to; the callback URL the provider sends the
    /// browser back to, as a path of the application.
    pub fn authorize(&self, location: &str, user: &MockUser) -> String {
        let query = location.split_once('?').expect("a query").1;
        let params: HashMap<String, String> = form(query);
        assert_eq!(params["response_type"], "code");
        assert_eq!(params["client_id"], CLIENT_ID);
        assert_eq!(params["code_challenge_method"], "S256");
        assert_eq!(
            params["redirect_uri"],
            format!("{PUBLIC_URL}/auth/callback")
        );
        let code = format!("code-{}", uuid::Uuid::now_v7().simple());
        let sid = format!("sid-{}", uuid::Uuid::now_v7().simple());
        self.shared
            .sids
            .lock()
            .unwrap()
            .insert(user.subject.clone(), sid.clone());
        self.shared.codes.lock().unwrap().insert(
            code.clone(),
            Grant {
                user: user.clone(),
                nonce: params.get("nonce").cloned(),
                challenge: params["code_challenge"].clone(),
                sid,
            },
        );
        format!(
            "/auth/callback?code={code}&state={}&iss={}",
            encode(&params["state"]),
            encode(&self.shared.issuer)
        )
    }

    /// A back-channel logout token for `sid`, or for the user `subject`.
    pub fn logout_token(&self, sid: Option<&str>, subject: Option<&str>, extra: Value) -> String {
        let now = chrono::Utc::now().timestamp();
        let mut claims = json!({
            "iss": self.shared.issuer,
            "aud": CLIENT_ID,
            "iat": now,
            "exp": now + 120,
            "jti": uuid::Uuid::now_v7().to_string(),
            "events": { "http://schemas.openid.net/event/backchannel-logout": {} },
        });
        if let Some(sid) = sid {
            claims["sid"] = json!(sid);
        }
        if let Some(subject) = subject {
            claims["sub"] = json!(subject);
        }
        for (key, value) in extra.as_object().into_iter().flatten() {
            claims[key] = value.clone();
        }
        let mut header = self.header();
        header.typ = Some("logout+jwt".into());
        self.sign(&header, &claims)
    }

    /// The `sid` of the last sign-in this provider granted `subject`.
    pub fn sid_of(&self, subject: &str) -> Option<String> {
        self.shared.sids.lock().unwrap().get(subject).cloned()
    }

    /// The settings of an application using this provider.
    pub fn settings(&self, extra: &[(&str, &str)]) -> AuthSettings {
        let mut object = serde_json::Map::new();
        for (key, value) in [
            ("public_url", PUBLIC_URL),
            ("oidc_issuer", self.issuer()),
            ("oidc_client_secret", CLIENT_SECRET),
            ("oidc_trusted_clients", TRUSTED_CLIENT),
            ("auth_signup", "open"),
            (
                "session_keys",
                "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=",
            ),
        ]
        .into_iter()
        .chain(extra.iter().copied())
        {
            object.insert(key.to_string(), json!(value));
        }
        for flag in ["oidc_introspection", "oidc_trust_email"] {
            if let Some(Value::String(value)) = object.get(flag) {
                let value = value == "true";
                object.insert(flag.to_string(), json!(value));
            }
        }
        serde_json::from_value(Value::Object(object)).expect("settings")
    }

    /// The application over `db`, authenticating against this provider.
    pub fn app(&self, db: &DatabaseConnection, extra: &[(&str, &str)]) -> (Router, Arc<Auth>) {
        let AuthConfig::Oidc(config) = self.settings(extra).resolve().expect("valid") else {
            panic!("oidc expected");
        };
        let auth = Arc::new(Auth::with_store(*config, Arc::new(MemoryStore::default())));
        let state =
            offline(AppState::new(db.clone(), test_media_root())).with_auth(Arc::clone(&auth));
        (oxidgene_api::build_router(state), auth)
    }
}

fn access_claims(issuer: &str, subject: &str, client: &str, lifetime: i64) -> Value {
    let now = chrono::Utc::now().timestamp();
    json!({
        "iss": issuer,
        "sub": subject,
        "aud": [CLIENT_ID, PUBLIC_URL],
        "azp": client,
        "scope": "openid profile email",
        "iat": now,
        "exp": now + lifetime,
    })
}

fn form(query: &str) -> HashMap<String, String> {
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(key, value)| (decode(key), decode(value)))
        .collect()
}

fn decode(value: &str) -> String {
    let bytes = value.replace('+', " ").into_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap();
            out.push(u8::from_str_radix(hex, 16).unwrap());
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap()
}

fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

async fn discovery(State(shared): State<Arc<Shared>>) -> Json<Value> {
    let issuer = &shared.issuer;
    Json(json!({
        "issuer": issuer,
        "authorization_endpoint": "https://id.example.invalid/authorize",
        "token_endpoint": format!("{issuer}oidc/token"),
        "jwks_uri": format!("{issuer}oidc/certs"),
        "end_session_endpoint": "https://id.example.invalid/logout",
        "introspection_endpoint": format!("{issuer}oidc/introspect"),
        "authorization_response_iss_parameter_supported": true,
    }))
}

async fn jwks(State(shared): State<Arc<Shared>>) -> Response {
    shared.controls.lock().unwrap().jwks_fetches += 1;
    (
        [(header::CACHE_CONTROL, "max-age=600")],
        Json(json!({ "keys": [shared.jwk] })),
    )
        .into_response()
}

fn oauth_error(error: &str) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))).into_response()
}

fn authenticated(headers: &HeaderMap) -> bool {
    let expected = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("{CLIENT_ID}:{CLIENT_SECRET}"))
    );
    headers
        .get(header::AUTHORIZATION)
        .is_some_and(|value| value.as_bytes() == expected.as_bytes())
}

async fn token(
    State(shared): State<Arc<Shared>>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    if shared.controls.lock().unwrap().token_endpoint_down {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    if !authenticated(&headers) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "invalid_client" })),
        )
            .into_response();
    }
    let grant = match form.get("grant_type").map(String::as_str) {
        Some("authorization_code") => {
            let Some(grant) = shared.codes.lock().unwrap().remove(&form["code"]) else {
                return oauth_error("invalid_grant");
            };
            let challenge =
                URL_SAFE_NO_PAD.encode(Sha256::digest(form["code_verifier"].as_bytes()));
            if challenge != grant.challenge {
                return oauth_error("invalid_grant");
            }
            grant
        }
        Some("refresh_token") => {
            let (refuse, delay) = {
                let mut controls = shared.controls.lock().unwrap();
                controls.refreshes += 1;
                (controls.refuse_refresh, controls.refresh_delay)
            };
            tokio::time::sleep(delay).await;
            let presented = &form["refresh_token"];
            if !shared.used.lock().unwrap().insert(presented.clone()) {
                shared.controls.lock().unwrap().refresh_reuses += 1;
                return oauth_error("invalid_grant");
            }
            if refuse {
                return oauth_error("invalid_grant");
            }
            match shared.refresh.lock().unwrap().remove(presented) {
                Some(grant) => grant,
                None => return oauth_error("invalid_grant"),
            }
        }
        _ => return oauth_error("unsupported_grant_type"),
    };
    issue(&shared, grant)
}

/// Tokens for `grant`: an access token, a rotated refresh token and an ID
/// token.
fn issue(shared: &Shared, grant: Grant) -> Response {
    let lifetime = shared.controls.lock().unwrap().access_lifetime;
    let now = chrono::Utc::now().timestamp();
    let mut access_header = Header::new(Algorithm::EdDSA);
    access_header.typ = Some("at+jwt".into());
    access_header.kid = Some(KID.into());
    let mut header = Header::new(Algorithm::EdDSA);
    header.kid = Some(KID.into());
    let mut access = access_claims(&shared.issuer, &grant.user.subject, CLIENT_ID, lifetime);
    access["groups"] = json!([""]);
    for (key, value) in grant.user.claims.as_object().into_iter().flatten() {
        if matches!(key.as_str(), "groups" | "email" | "email_verified") {
            access[key] = value.clone();
        }
    }
    let mut id = json!({
        "iss": shared.issuer,
        "sub": grant.user.subject,
        "aud": CLIENT_ID,
        "azp": CLIENT_ID,
        "iat": now,
        "exp": now + 300,
        "sid": grant.sid,
    });
    if let Some(nonce) = &grant.nonce {
        id["nonce"] = json!(nonce);
    }
    for (key, value) in grant.user.claims.as_object().into_iter().flatten() {
        id[key] = value.clone();
    }
    let sign = |claims: &Value| jsonwebtoken::encode(&header, claims, &shared.key).unwrap();
    let access_token = jsonwebtoken::encode(&access_header, &access, &shared.key).unwrap();
    let refresh_token = format!("refresh-{}", uuid::Uuid::now_v7().simple());
    let id_token = sign(&id);
    shared.refresh.lock().unwrap().insert(
        refresh_token.clone(),
        Grant {
            nonce: None,
            ..grant
        },
    );
    Json(json!({
        "access_token": access_token,
        "token_type": "Bearer",
        "expires_in": lifetime,
        "refresh_token": refresh_token,
        "id_token": id_token,
    }))
    .into_response()
}

async fn introspect(
    State(shared): State<Arc<Shared>>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    if !authenticated(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let answer = shared
        .controls
        .lock()
        .unwrap()
        .opaque
        .get(&form["token"])
        .cloned()
        .unwrap_or(json!({ "active": false }));
    Json(answer).into_response()
}

/// A cookie-keeping client of the application.
pub struct Browser {
    pub app: Router,
    pub cookies: HashMap<String, String>,
    /// Sent as `X-OxidGene-CSRF` on writes when set.
    pub csrf: Option<String>,
    /// Sent as `Origin` on writes when set.
    pub origin: Option<String>,
}

/// An answer: status, headers and JSON body (`Null` when not JSON).
pub struct Answer {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub json: Value,
    /// The raw body, for binary answers.
    pub bytes: Vec<u8>,
}

impl Answer {
    pub fn location(&self) -> String {
        self.headers[header::LOCATION].to_str().unwrap().to_string()
    }

    /// The `Set-Cookie` headers of the answer.
    pub fn set_cookies(&self) -> Vec<String> {
        self.headers
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|value| value.to_str().unwrap().to_string())
            .collect()
    }
}

impl Browser {
    pub fn new(app: &Router) -> Self {
        Self {
            app: app.clone(),
            cookies: HashMap::new(),
            csrf: None,
            origin: Some(PUBLIC_URL.to_string()),
        }
    }

    /// Send a request; cookies the answer sets are kept.
    pub async fn send(
        &mut self,
        method: Method,
        uri: &str,
        body: Option<Body>,
        content_type: Option<&str>,
    ) -> Answer {
        let mut request = Request::builder()
            .method(method.clone())
            .uri(uri)
            .header(header::HOST, "genealogy.example.invalid")
            .header(
                header::USER_AGENT,
                "Mozilla/5.0 (X11; Linux x86_64; rv:120.0) Gecko/20100101 Firefox/120.0",
            );
        if !self.cookies.is_empty() {
            let cookie = self
                .cookies
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect::<Vec<_>>()
                .join("; ");
            request = request.header(header::COOKIE, cookie);
        }
        let writes = !matches!(method, Method::GET | Method::HEAD);
        if writes {
            if let Some(csrf) = &self.csrf {
                request = request.header("x-oxidgene-csrf", csrf);
            }
            if let Some(origin) = &self.origin {
                request = request.header(header::ORIGIN, origin);
            }
        }
        if let Some(content_type) = content_type {
            request = request.header(header::CONTENT_TYPE, content_type);
        }
        let response = self
            .app
            .clone()
            .oneshot(request.body(body.unwrap_or_default()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec();
        for cookie in headers.get_all(header::SET_COOKIE) {
            let cookie = cookie.to_str().unwrap();
            let (pair, attributes) = cookie.split_once(';').unwrap_or((cookie, ""));
            let (name, value) = pair.split_once('=').unwrap();
            if attributes.contains("Max-Age=0") {
                self.cookies.remove(name);
            } else {
                self.cookies.insert(name.to_string(), value.to_string());
            }
        }
        Answer {
            status,
            headers,
            json: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
            bytes,
        }
    }

    pub async fn get(&mut self, uri: &str) -> Answer {
        self.send(Method::GET, uri, None, None).await
    }

    pub async fn json(&mut self, method: Method, uri: &str, body: Value) -> Answer {
        self.send(
            method,
            uri,
            Some(Body::from(serde_json::to_vec(&body).unwrap())),
            Some("application/json"),
        )
        .await
    }

    /// Run a GraphQL operation.
    pub async fn gql(&mut self, query: &str, variables: Value) -> Answer {
        self.json(
            Method::POST,
            "/graphql",
            json!({ "query": query, "variables": variables }),
        )
        .await
    }

    /// Sign in as `user` through the Backend-for-Frontend; the callback's
    /// answer. On success the session's CSRF token is kept.
    pub async fn sign_in(&mut self, provider: &MockProvider, user: &MockUser) -> Answer {
        let login = self.get("/auth/login?return_to=/trees&lang=fr").await;
        assert_eq!(login.status, StatusCode::SEE_OTHER, "{}", login.json);
        let callback = provider.authorize(&login.location(), user);
        let answer = self.get(&callback).await;
        if self.cookies.contains_key("__Host-oxidgene") {
            let session = self.get("/auth/session").await;
            self.csrf = session.json["csrf_token"].as_str().map(str::to_string);
        }
        answer
    }
}
