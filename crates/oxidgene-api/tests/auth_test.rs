//! Authentication of the web build against the mock provider
//! (docs/authentication.md §12): sign-in through the Backend-for-Frontend,
//! the session cookie and its CSRF defences, refresh with rotation, sign-out
//! on both channels, bearer tokens and each check they fail, the signup
//! gate, deactivation and erasure, and the logs of all of it.

use std::time::Duration;

use axum::http::{Method, StatusCode, header};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde_json::{Value, json};

use crate::common::capture::global_capture;
use crate::common::oidc::{
    Browser, CLIENT_ID, KID, MockProvider, MockUser, PUBLIC_URL, TRUSTED_CLIENT,
};
use crate::common::setup_db;

async fn bearer(app: &axum::Router, token: &str, uri: &str) -> (StatusCode, axum::http::HeaderMap) {
    use tower::ServiceExt as _;
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri(uri)
                .header(header::HOST, "genealogy.example.invalid")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    (response.status(), response.headers().clone())
}

#[tokio::test]
async fn a_user_signs_in_and_the_session_serves_rest_and_graphql() {
    let provider = MockProvider::start().await;
    let db = setup_db().await;
    let (app, _) = provider.app(&db, &[]);
    let mut browser = Browser::new(&app);

    let anonymous = browser.get("/auth/session").await;
    assert_eq!(anonymous.status, StatusCode::OK);
    assert_eq!(anonymous.json["mode"], "oidc");
    assert_eq!(anonymous.json["authenticated"], false);
    assert_eq!(anonymous.json["client_id"], CLIENT_ID);
    assert_eq!(anonymous.headers[header::CACHE_CONTROL], "no-store");
    // Until permissions decide per tree, an anonymous API request is refused.
    assert_eq!(
        browser.get("/api/v1/trees").await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        browser.get("/api/v1/openapi.json").await.status,
        StatusCode::OK
    );

    let login = browser.get("/auth/login?return_to=/trees&lang=fr").await;
    assert_eq!(login.status, StatusCode::SEE_OTHER);
    let location = login.location();
    for param in [
        "code_challenge=",
        "nonce=",
        "state=",
        "ui_locales=fr",
        "scope=openid",
    ] {
        assert!(location.contains(param), "{param} in {location}");
    }
    let binding = login
        .set_cookies()
        .into_iter()
        .find(|cookie| cookie.starts_with("__Host-oxidgene-login="))
        .expect("the login binding");
    assert!(
        binding.contains("SameSite=Lax")
            && binding.contains("HttpOnly")
            && binding.contains("Secure")
    );

    let user = MockUser::new("ada");
    let callback = provider.authorize(&location, &user);
    let answer = browser.get(&callback).await;
    assert_eq!(answer.status, StatusCode::SEE_OTHER, "{}", answer.json);
    assert_eq!(answer.location(), "/trees");
    let session_cookie = answer
        .set_cookies()
        .into_iter()
        .find(|cookie| cookie.starts_with("__Host-oxidgene="))
        .expect("the session cookie");
    for attribute in [
        "Path=/",
        "Secure",
        "HttpOnly",
        "SameSite=Strict",
        "Max-Age=604800",
    ] {
        assert!(
            session_cookie.contains(attribute),
            "{attribute} in {session_cookie}"
        );
    }
    assert!(!browser.cookies.contains_key("__Host-oxidgene-login"));

    let session = browser.get("/auth/session").await;
    assert_eq!(session.json["authenticated"], true, "{}", session.json);
    assert_eq!(session.json["user"]["display_name"], "Fictive ada");
    assert_eq!(session.json["user"]["email"], user.email());
    assert_eq!(session.json["user"]["locale"], "fr");
    assert_eq!(session.json["user"]["instance_role"], "user");
    assert_eq!(session.json["user"]["can_create_trees"], true);
    assert!(session.json["csrf_token"].as_str().is_some());
    browser.csrf = session.json["csrf_token"].as_str().map(str::to_string);

    let me = browser.get("/api/v1/me").await;
    assert_eq!(me.status, StatusCode::OK, "{}", me.json);
    assert_eq!(me.json["provider_name"], "Fictive ada");
    let created = browser
        .json(
            Method::POST,
            "/api/v1/trees",
            json!({ "name": "Fictive tree" }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.json);
    let gql = browser
        .gql("{ me { displayName email instanceRole } }", json!({}))
        .await;
    assert_eq!(
        gql.json["data"]["me"]["displayName"], "Fictive ada",
        "{}",
        gql.json
    );
}

#[tokio::test]
async fn a_replayed_or_unbound_callback_fails() {
    let provider = MockProvider::start().await;
    let db = setup_db().await;
    let (app, _) = provider.app(&db, &[]);
    let user = MockUser::new("ben");

    let mut browser = Browser::new(&app);
    let login = browser.get("/auth/login").await;
    let callback = provider.authorize(&login.location(), &user);
    // A code planted in another browser: it holds no binding cookie.
    let mut victim = Browser::new(&app);
    let planted = victim.get(&callback).await;
    assert_eq!(planted.location(), "/?auth_error=login_failed");
    assert!(!victim.cookies.contains_key("__Host-oxidgene"));
    // The planted attempt consumed the transaction: the real one fails too.
    let real = browser.get(&callback).await;
    assert_eq!(real.location(), "/?auth_error=login_failed");

    let login = browser.get("/auth/login").await;
    let callback = provider.authorize(&login.location(), &user);
    assert_eq!(browser.get(&callback).await.location(), "/");
    browser
        .cookies
        .insert("__Host-oxidgene-login".into(), "replayed".into());
    let replay = browser.get(&callback).await;
    assert_eq!(replay.location(), "/?auth_error=login_failed");

    // An issuer other than the provider's in the response (RFC 9207).
    let login = browser.get("/auth/login").await;
    let callback = provider.authorize(&login.location(), &user);
    let mixed_up = callback.split("&iss=").next().unwrap().to_string()
        + "&iss=https%3A%2F%2Fother.example.invalid";
    assert_eq!(
        browser.get(&mixed_up).await.location(),
        "/?auth_error=login_failed"
    );
}

#[tokio::test]
async fn writes_need_the_csrf_token_and_the_public_origin() {
    let provider = MockProvider::start().await;
    let db = setup_db().await;
    let (app, _) = provider.app(&db, &[]);
    let mut browser = Browser::new(&app);
    browser.sign_in(&provider, &MockUser::new("cleo")).await;
    let csrf = browser.csrf.clone();

    browser.csrf = None;
    let missing = browser
        .json(Method::POST, "/api/v1/trees", json!({ "name": "Fictive" }))
        .await;
    assert_eq!(missing.status, StatusCode::FORBIDDEN);
    assert_eq!(missing.json["error"], "forbidden");
    browser.csrf = Some("forged".into());
    let forged = browser
        .json(Method::POST, "/api/v1/trees", json!({ "name": "Fictive" }))
        .await;
    assert_eq!(forged.status, StatusCode::FORBIDDEN);
    browser.csrf = csrf;
    browser.origin = Some("https://attacker.example.invalid".into());
    let foreign = browser
        .json(Method::POST, "/api/v1/trees", json!({ "name": "Fictive" }))
        .await;
    assert_eq!(foreign.status, StatusCode::FORBIDDEN);
    browser.origin = None;
    let accepted = browser
        .json(Method::POST, "/api/v1/trees", json!({ "name": "Fictive" }))
        .await;
    assert_eq!(accepted.status, StatusCode::CREATED);
    // GraphQL over the cookie takes the same token.
    let saved = browser.csrf.take();
    let refused = browser.gql("{ me { id } }", json!({})).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    browser.csrf = saved;
    assert_eq!(
        browser.gql("{ me { id } }", json!({})).await.status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn concurrent_requests_share_one_refresh_and_the_token_rotates() {
    let provider = MockProvider::start().await;
    let db = setup_db().await;
    let (app, _) = provider.app(&db, &[]);
    let mut browser = Browser::new(&app);
    // A token within the refresh margin from the start.
    provider.controls(|controls| controls.access_lifetime = 30);
    browser.sign_in(&provider, &MockUser::new("dora")).await;
    provider.controls(|controls| {
        controls.refreshes = 0;
        controls.refresh_delay = Duration::from_millis(300);
        controls.access_lifetime = 300;
    });
    let cookies = browser.cookies.clone();
    let mut requests = Vec::new();
    for _ in 0..6 {
        let mut client = Browser::new(&app);
        client.cookies = cookies.clone();
        requests.push(tokio::spawn(async move {
            client.get("/api/v1/me").await.status
        }));
    }
    for request in requests {
        assert_eq!(request.await.unwrap(), StatusCode::OK);
    }
    let (refreshes, reuses) =
        provider.controls(|controls| (controls.refreshes, controls.refresh_reuses));
    assert_eq!(refreshes, 1, "one refresh for every waiting request");
    assert_eq!(reuses, 0, "no refresh token presented twice");
    // The rotated token was kept: the next refresh presents the new one.
    provider.controls(|controls| controls.refresh_delay = Duration::ZERO);
    assert_eq!(browser.get("/api/v1/me").await.status, StatusCode::OK);
}

#[tokio::test]
async fn a_refused_refresh_ends_the_session_and_an_unreachable_provider_is_unavailable() {
    let provider = MockProvider::start().await;
    let db = setup_db().await;
    let (app, _) = provider.app(&db, &[]);

    // The provider cannot be reached and the token has expired: 503.
    // Expired by 30 seconds: still valid within the leeway, but due.
    provider.controls(|controls| controls.access_lifetime = -30);
    let mut browser = Browser::new(&app);
    let signed = browser.sign_in(&provider, &MockUser::new("emil")).await;
    assert_eq!(signed.status, StatusCode::SEE_OTHER);
    provider.controls(|controls| controls.token_endpoint_down = true);
    let down = browser.get("/api/v1/me").await;
    assert_eq!(
        down.status,
        StatusCode::SERVICE_UNAVAILABLE,
        "{}",
        down.json
    );
    assert_eq!(down.json["error"], "auth_unavailable");
    assert!(
        browser.cookies.contains_key("__Host-oxidgene"),
        "the session is kept"
    );

    // Back, the provider refuses: the session ends.
    provider.controls(|controls| {
        controls.token_endpoint_down = false;
        controls.refuse_refresh = true;
    });
    let refused = browser.get("/api/v1/me").await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
    assert!(
        !browser.cookies.contains_key("__Host-oxidgene"),
        "the cookie is cleared"
    );
    let session = browser.get("/auth/session").await;
    assert_eq!(session.json["authenticated"], false);
}

#[tokio::test]
async fn sign_out_here_everywhere_and_from_the_back_channel() {
    let provider = MockProvider::start().await;
    let db = setup_db().await;
    let (app, _) = provider.app(&db, &[]);
    let user = MockUser::new("fern");

    let mut first = Browser::new(&app);
    first.sign_in(&provider, &user).await;
    let logout = first.json(Method::POST, "/auth/logout", json!({})).await;
    assert_eq!(logout.status, StatusCode::OK);
    let url = logout.json["logout_url"]
        .as_str()
        .expect("an end-session URL");
    assert!(
        url.starts_with("https://id.example.invalid/logout?id_token_hint="),
        "{url}"
    );
    assert!(url.contains("post_logout_redirect_uri=https%3A%2F%2Fgenealogy.example.invalid%2F"));
    assert!(url.contains(&format!("client_id={CLIENT_ID}")));
    assert!(!first.cookies.contains_key("__Host-oxidgene"));

    // Everywhere: two browsers, one sign-out.
    let mut a = Browser::new(&app);
    let mut b = Browser::new(&app);
    a.sign_in(&provider, &user).await;
    b.sign_in(&provider, &user).await;
    let sessions = a.get("/api/v1/me/sessions").await;
    assert_eq!(
        sessions.json.as_array().unwrap().len(),
        2,
        "{}",
        sessions.json
    );
    assert_eq!(
        a.json(Method::POST, "/auth/logout-everywhere", json!({}))
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(b.get("/api/v1/me").await.status, StatusCode::UNAUTHORIZED);

    // The back channel, by provider session.
    let mut c = Browser::new(&app);
    c.sign_in(&provider, &user).await;
    let sid = provider.sid_of(&user.subject).expect("a provider session");
    let token = provider.logout_token(Some(&sid), None, json!({}));
    let mut channel = Browser::new(&app);
    let form = format!("logout_token={token}");
    let answer = channel
        .send(
            Method::POST,
            "/auth/backchannel-logout",
            Some(form.clone().into()),
            Some("application/x-www-form-urlencoded"),
        )
        .await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(c.get("/api/v1/me").await.status, StatusCode::UNAUTHORIZED);
    // The same token again, and tokens missing what they must carry.
    for form in [
        form,
        format!(
            "logout_token={}",
            provider.logout_token(Some(&sid), None, json!({ "nonce": "n" }))
        ),
        format!(
            "logout_token={}",
            provider.logout_token(None, None, json!({}))
        ),
        format!(
            "logout_token={}",
            provider.logout_token(Some(&sid), None, json!({ "events": {} }))
        ),
        format!(
            "logout_token={}",
            provider.logout_token(Some(&sid), None, json!({ "aud": "another-client" }))
        ),
    ] {
        let refused = channel
            .send(
                Method::POST,
                "/auth/backchannel-logout",
                Some(form.into()),
                Some("application/x-www-form-urlencoded"),
            )
            .await;
        assert_eq!(refused.status, StatusCode::BAD_REQUEST);
    }

    // By subject: every session of the user.
    let mut d = Browser::new(&app);
    d.sign_in(&provider, &user).await;
    let token = provider.logout_token(None, Some(&user.subject), json!({}));
    channel
        .send(
            Method::POST,
            "/auth/backchannel-logout",
            Some(format!("logout_token={token}").into()),
            Some("application/x-www-form-urlencoded"),
        )
        .await;
    assert_eq!(d.get("/api/v1/me").await.status, StatusCode::UNAUTHORIZED);

    // By provider session and subject, for a session signed in without a
    // `sid`, which no `sid` names: the user's sessions without one end.
    let unindexed = MockUser::new("hollis").with("sid", Value::Null);
    let mut e = Browser::new(&app);
    e.sign_in(&provider, &unindexed).await;
    assert_eq!(e.get("/api/v1/me").await.status, StatusCode::OK);
    let token = provider.logout_token(
        Some("sid-the-provider-knows"),
        Some(&unindexed.subject),
        json!({}),
    );
    let answer = channel
        .send(
            Method::POST,
            "/auth/backchannel-logout",
            Some(format!("logout_token={token}").into()),
            Some("application/x-www-form-urlencoded"),
        )
        .await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(e.get("/api/v1/me").await.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn bearer_tokens_of_trusted_clients_alone_are_accepted() {
    let provider = MockProvider::start().await;
    let db = setup_db().await;
    let (app, _) = provider.app(&db, &[]);
    let user = MockUser::new("gus");
    Browser::new(&app).sign_in(&provider, &user).await;
    let issuer = provider.issuer().to_string();
    let now = chrono::Utc::now().timestamp();
    let claims = |overrides: Value| {
        let mut claims = json!({
            "iss": issuer,
            "sub": user.subject,
            "aud": PUBLIC_URL,
            "azp": TRUSTED_CLIENT,
            "scope": "tree:read",
            "iat": now,
            "exp": now + 300,
        });
        for (key, value) in overrides.as_object().unwrap() {
            claims[key] = value.clone();
        }
        claims
    };
    let accepted = provider.sign(&provider.header(), &claims(json!({})));
    let (status, _) = bearer(&app, &accepted, "/api/v1/me").await;
    assert_eq!(status, StatusCode::OK);
    // Within the leeway, a token that just expired still passes.
    let leeway = provider.sign(&provider.header(), &claims(json!({ "exp": now - 30 })));
    assert_eq!(bearer(&app, &leeway, "/api/v1/me").await.0, StatusCode::OK);

    let mut hmac = Header::new(Algorithm::HS256);
    hmac.kid = Some(KID.into());
    let public_key_as_secret = jsonwebtoken::encode(
        &hmac,
        &claims(json!({})),
        &EncodingKey::from_secret(b"the provider's public key"),
    )
    .unwrap();
    let mut unknown_kid = provider.header();
    unknown_kid.kid = Some("rotated-away".into());
    let alg_none = {
        use base64::Engine as _;
        let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        format!(
            "{}.{}.",
            engine.encode(br#"{"alg":"none","typ":"JWT"}"#),
            engine.encode(claims(json!({})).to_string())
        )
    };
    let mut at_jwt = provider.header();
    at_jwt.typ = Some("logout+jwt".into());
    let refused = [
        (
            "issuer",
            provider.sign(
                &provider.header(),
                &claims(json!({ "iss": "https://other.example.invalid" })),
            ),
        ),
        (
            "audience",
            provider.sign(&provider.header(), &claims(json!({ "aud": "another-api" }))),
        ),
        (
            "ID token",
            provider.sign(&provider.header(), &claims(json!({ "aud": CLIENT_ID }))),
        ),
        (
            "expired",
            provider.sign(&provider.header(), &claims(json!({ "exp": now - 120 }))),
        ),
        (
            "not yet",
            provider.sign(&provider.header(), &claims(json!({ "nbf": now + 600 }))),
        ),
        (
            "issued later",
            provider.sign(&provider.header(), &claims(json!({ "iat": now + 600 }))),
        ),
        (
            "untrusted client",
            provider.sign(&provider.header(), &claims(json!({ "azp": "another-app" }))),
        ),
        (
            "unknown user",
            provider.sign(&provider.header(), &claims(json!({ "sub": "nobody" }))),
        ),
        ("hmac", public_key_as_secret),
        ("alg none", alg_none),
        (
            "unknown kid",
            provider.sign(&unknown_kid, &claims(json!({}))),
        ),
        ("token type", provider.sign(&at_jwt, &claims(json!({})))),
        ("oversized", "a".repeat(17 * 1024)),
        ("opaque", "opaque-token".into()),
    ];
    for (case, token) in refused {
        let (status, headers) = bearer(&app, &token, "/api/v1/me").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{case}");
        assert_eq!(
            headers[header::WWW_AUTHENTICATE],
            "Bearer error=\"invalid_token\"",
            "{case}"
        );
    }
}

#[tokio::test]
async fn an_unknown_kid_refetches_the_jwks_at_most_once_a_minute() {
    let provider = MockProvider::start().await;
    let db = setup_db().await;
    let (app, _) = provider.app(&db, &[]);
    let user = MockUser::new("hana");
    Browser::new(&app).sign_in(&provider, &user).await;
    let fetched = provider.controls(|controls| controls.jwks_fetches);
    let mut header = provider.header();
    for attempt in 0..5 {
        header.kid = Some(format!("forged-{attempt}"));
        let token = provider.sign(&header, &json!({ "iss": provider.issuer(), "sub": user.subject, "aud": PUBLIC_URL, "azp": TRUSTED_CLIENT, "exp": chrono::Utc::now().timestamp() + 60 }));
        assert_eq!(
            bearer(&app, &token, "/api/v1/me").await.0,
            StatusCode::UNAUTHORIZED
        );
    }
    let refetched = provider.controls(|controls| controls.jwks_fetches) - fetched;
    assert_eq!(refetched, 1, "forged kids hammer the provider");
}

#[tokio::test]
async fn opaque_tokens_are_introspected_when_allowed() {
    let provider = MockProvider::start().await;
    let db = setup_db().await;
    let (app, _) = provider.app(&db, &[("oidc_introspection", "true")]);
    let user = MockUser::new("ines");
    Browser::new(&app).sign_in(&provider, &user).await;
    let now = chrono::Utc::now().timestamp();
    provider.controls(|controls| {
        controls.opaque.insert(
            "opaque-active".into(),
            json!({ "active": true, "iss": provider.issuer(), "aud": PUBLIC_URL, "sub": user.subject, "client_id": TRUSTED_CLIENT, "scope": "tree:read", "exp": now + 300 }),
        );
        controls.opaque.insert(
            "opaque-foreign".into(),
            json!({ "active": true, "iss": provider.issuer(), "aud": "another-api", "sub": user.subject, "client_id": TRUSTED_CLIENT, "exp": now + 300 }),
        );
    });
    assert_eq!(
        bearer(&app, "opaque-active", "/api/v1/me").await.0,
        StatusCode::OK
    );
    assert_eq!(
        bearer(&app, "opaque-foreign", "/api/v1/me").await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        bearer(&app, "opaque-inactive", "/api/v1/me").await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn the_signup_gate_and_the_bootstrap_administrator() {
    let provider = MockProvider::start().await;
    let db = setup_db().await;
    let (app, _) = provider.app(
        &db,
        &[
            ("auth_signup", "invitation"),
            ("auth_bootstrap_admin_email", "Root@Example.invalid"),
        ],
    );
    let stranger = MockUser::new("stranger");
    let refused = Browser::new(&app).sign_in(&provider, &stranger).await;
    assert_eq!(refused.location(), "/?auth_error=signup_closed");

    // An unverified address does not make an administrator.
    let unverified = MockUser::new("root").with("email_verified", json!(false));
    let refused = Browser::new(&app).sign_in(&provider, &unverified).await;
    assert_eq!(refused.location(), "/?auth_error=signup_closed");

    let root = MockUser::new("root");
    let mut admin = Browser::new(&app);
    assert_eq!(admin.sign_in(&provider, &root).await.location(), "/trees");
    assert_eq!(admin.get("/api/v1/me").await.json["instance_role"], "admin");
    // The variable is ignored once an administrator exists.
    let second = MockUser::new("root").with("email", json!("root@example.invalid"));
    let refused = Browser::new(&app).sign_in(&provider, &second).await;
    assert_eq!(refused.location(), "/?auth_error=signup_closed");
}

#[tokio::test]
async fn instance_roles_and_admission_follow_the_roles_claim() {
    let provider = MockProvider::start().await;
    let db = setup_db().await;
    let (app, _) = provider.app(
        &db,
        &[
            ("auth_signup", "claim"),
            ("auth_tree_creation", "claim"),
            ("oidc_roles_claim", "groups"),
            ("oidc_allowed_values", "members,admins"),
            ("oidc_admin_values", "admins"),
            ("oidc_creator_values", "creators"),
        ],
    );
    let outsider = MockUser::new("ivo").with("groups", json!(["others"]));
    assert_eq!(
        Browser::new(&app)
            .sign_in(&provider, &outsider)
            .await
            .location(),
        "/?auth_error=signup_closed"
    );
    let member = MockUser::new("jan").with("groups", json!(["members"]));
    let mut browser = Browser::new(&app);
    browser.sign_in(&provider, &member).await;
    let me = browser.get("/api/v1/me").await;
    assert_eq!(me.json["instance_role"], "user");
    assert_eq!(me.json["can_create_trees"], false);

    let promoted = member
        .clone()
        .with("groups", json!(["members", "admins", "creators"]));
    let mut browser = Browser::new(&app);
    browser.sign_in(&provider, &promoted).await;
    let me = browser.get("/api/v1/me").await;
    assert_eq!(me.json["instance_role"], "admin");
    assert_eq!(me.json["can_create_trees"], true);
    // The claim decides: an administrator cannot grant the role by hand.
    let id = me.json["id"].as_str().unwrap().to_string();
    let refused = browser
        .json(
            Method::PUT,
            &format!("/api/v1/admin/users/{id}"),
            json!({ "instance_role": "user" }),
        )
        .await;
    assert_eq!(refused.status, StatusCode::BAD_REQUEST);

    let dropped = member.with("groups", json!(["creators"]));
    assert_eq!(
        Browser::new(&app)
            .sign_in(&provider, &dropped)
            .await
            .location(),
        "/?auth_error=access_denied"
    );
}

/// A database that does not answer while a session is resolved signs no
/// one out: the request is `503`, the cookie kept, and the session serves
/// again once the database answers.
#[tokio::test]
async fn a_database_failure_keeps_the_session() {
    use oxidgene_db::sea_orm::ConnectionTrait;

    let provider = MockProvider::start().await;
    let db = setup_db().await;
    let (app, _) = provider.app(&db, &[]);
    let mut browser = Browser::new(&app);
    browser.sign_in(&provider, &MockUser::new("lark")).await;
    assert_eq!(browser.get("/api/v1/me").await.status, StatusCode::OK);
    db.execute_unprepared("ALTER TABLE app_user RENAME TO app_user_away")
        .await
        .expect("hides the users");
    let failed = browser.get("/api/v1/me").await;
    assert_eq!(
        failed.status,
        StatusCode::SERVICE_UNAVAILABLE,
        "{}",
        failed.json
    );
    assert!(
        failed.headers.get(header::SET_COOKIE).is_none(),
        "{:?}",
        failed.headers
    );
    db.execute_unprepared("ALTER TABLE app_user_away RENAME TO app_user")
        .await
        .expect("restores the users");
    assert_eq!(browser.get("/api/v1/me").await.status, StatusCode::OK);
}

#[tokio::test]
async fn deactivation_and_erasure_end_sessions() {
    let provider = MockProvider::start().await;
    let db = setup_db().await;
    let (app, _) = provider.app(
        &db,
        &[("auth_bootstrap_admin_email", "boss@example.invalid")],
    );
    let mut admin = Browser::new(&app);
    admin.sign_in(&provider, &MockUser::new("boss")).await;
    let user = MockUser::new("kim");
    let mut browser = Browser::new(&app);
    browser.sign_in(&provider, &user).await;
    let id = browser.get("/api/v1/me").await.json["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Not an administrator.
    let refused = browser.get("/api/v1/admin/users").await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    let listed = admin.get("/api/v1/admin/users?first=10").await;
    assert_eq!(listed.json["total_count"], 2, "{}", listed.json);

    let deactivated = admin
        .json(
            Method::PUT,
            &format!("/api/v1/admin/users/{id}"),
            json!({ "status": "deactivated" }),
        )
        .await;
    assert_eq!(deactivated.status, StatusCode::OK, "{}", deactivated.json);
    assert_eq!(deactivated.json["status"], "deactivated");
    assert_eq!(
        browser.get("/api/v1/me").await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        Browser::new(&app)
            .sign_in(&provider, &user)
            .await
            .location(),
        "/?auth_error=account_deactivated"
    );
    admin
        .json(
            Method::PUT,
            &format!("/api/v1/admin/users/{id}"),
            json!({ "status": "active" }),
        )
        .await;
    let mut browser = Browser::new(&app);
    browser.sign_in(&provider, &user).await;
    assert_eq!(browser.get("/api/v1/me").await.status, StatusCode::OK);

    let erased = browser.send(Method::DELETE, "/api/v1/me", None, None).await;
    assert_eq!(erased.status, StatusCode::NO_CONTENT);
    assert!(!browser.cookies.contains_key("__Host-oxidgene"));
    let tombstone = admin.get(&format!("/api/v1/admin/users/{id}")).await;
    assert_eq!(tombstone.json["status"], "erased");
    for field in ["display_name", "email", "issuer", "subject", "username"] {
        assert!(
            tombstone.json[field].is_null(),
            "{field}: {}",
            tombstone.json
        );
    }
    // The identity signs in again as a new user.
    let mut again = Browser::new(&app);
    again.sign_in(&provider, &user).await;
    let new_id = again.get("/api/v1/me").await.json["id"].clone();
    assert_ne!(new_id, json!(id));
}

#[tokio::test]
async fn logs_hold_no_token_email_or_subject() {
    let capture = global_capture();
    let provider = MockProvider::start().await;
    let db = setup_db().await;
    let (app, _) = provider.app(&db, &[]);
    let user = MockUser::new("lena");
    let mut browser = Browser::new(&app);
    browser.sign_in(&provider, &user).await;
    browser.get("/api/v1/me").await;
    let token = provider.access_token(&user.subject, "another-app", 300);
    bearer(&app, &token, "/api/v1/me").await;
    browser.json(Method::POST, "/auth/logout", json!({})).await;
    let cookies: Vec<String> = browser.cookies.values().cloned().collect();
    let recorded = capture.take();
    assert!(
        recorded
            .iter()
            .any(|field| field.value.contains("login_succeeded")),
        "the sign-in was logged"
    );
    for field in recorded {
        for secret in [user.subject.as_str(), &user.email(), &token, "Fictive lena"]
            .into_iter()
            .chain(cookies.iter().map(String::as_str))
        {
            assert!(
                !field.value.contains(secret),
                "{}.{} carries a secret",
                field.owner,
                field.field
            );
        }
    }
}

#[tokio::test]
async fn the_none_mode_reports_itself_and_has_no_account() {
    let db = setup_db().await;
    let app = crate::common::app_on(db);
    let mut browser = Browser::new(&app);
    let session = browser.get("/auth/session").await;
    assert_eq!(session.json["mode"], "none");
    assert_eq!(session.json["authenticated"], false);
    assert_eq!(
        browser.get("/api/v1/me").await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(browser.get("/api/v1/trees").await.status, StatusCode::OK);
    assert_eq!(
        browser.get("/auth/login").await.status,
        StatusCode::NOT_FOUND
    );
    // Nothing needing an identity exists, as on the desktop.
    for uri in [
        "/api/v1/admin/users",
        "/api/v1/me/sessions",
        "/api/v1/me/export",
    ] {
        assert_eq!(
            browser.get(uri).await.status,
            StatusCode::NOT_FOUND,
            "{uri}"
        );
    }
    let me = browser.gql("{ me { id } }", json!({})).await;
    assert_eq!(me.json["errors"][0]["extensions"]["code"], "NOT_FOUND");
}
