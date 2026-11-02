//! The account operations of the web build on REST and GraphQL alike
//! (docs/authentication.md §6, docs/api.md Accounts): the profile and its
//! avatar, the sessions, the account data, who may see whom, and the
//! administrator's user management. Fictitious users of the mock provider.

use std::io::Cursor;

use axum::body::Body;
use axum::http::{Method, StatusCode, header};
use base64::Engine as _;
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use serde_json::{Value, json};

use crate::common::oidc::{Browser, MockProvider, MockUser};
use crate::common::setup_db;

/// A signed-in browser of `user` and its account id.
async fn signed_in(
    app: &axum::Router,
    provider: &MockProvider,
    user: &MockUser,
) -> (Browser, String) {
    let mut browser = Browser::new(app);
    browser.sign_in(provider, user).await;
    let id = browser.get("/api/v1/me").await.json["id"]
        .as_str()
        .expect("signed in")
        .to_string();
    (browser, id)
}

fn png(width: u32, height: u32) -> Vec<u8> {
    let image = RgbaImage::from_fn(width, height, |x, _| Rgba([(x % 256) as u8, 40, 90, 255]));
    let mut out = Vec::new();
    DynamicImage::ImageRgba8(image)
        .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
        .unwrap();
    out
}

/// A multipart body holding `file` and the text `fields`.
fn multipart(file: &[u8], fields: &[(&str, &str)]) -> (Body, String) {
    let boundary = "fixture-boundary";
    let mut body = Vec::new();
    for (name, value) in fields {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"portrait.png\"\r\nContent-Type: image/png\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(file);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (
        Body::from(body),
        format!("multipart/form-data; boundary={boundary}"),
    )
}

#[tokio::test]
async fn the_display_name_and_language_are_the_users_on_both_surfaces() {
    let provider = MockProvider::start().await;
    let db = setup_db().await;
    let (app, _) = provider.app(&db, &[]);
    let ada = MockUser::new("ada");
    let (mut browser, _) = signed_in(&app, &provider, &ada).await;

    let renamed = browser
        .json(
            Method::PUT,
            "/api/v1/me",
            json!({ "display_name": "  Ada Fictive  ", "locale": "de" }),
        )
        .await;
    assert_eq!(renamed.status, StatusCode::OK, "{}", renamed.json);
    assert_eq!(renamed.json["display_name"], "Ada Fictive");
    assert_eq!(renamed.json["locale"], "de");
    for invalid in [
        json!({ "display_name": "" }),
        json!({ "display_name": "a\u{7}b" }),
        json!({ "locale": "ja" }),
    ] {
        let refused = browser.json(Method::PUT, "/api/v1/me", invalid).await;
        assert_eq!(refused.status, StatusCode::BAD_REQUEST);
        assert_eq!(refused.json["error"], "validation_error");
    }
    // Signing in again keeps both: the provider's name never overwrites.
    browser.sign_in(&provider, &ada).await;
    let again = browser.get("/api/v1/me").await;
    assert_eq!(again.json["display_name"], "Ada Fictive");
    assert_eq!(again.json["locale"], "de");

    let cleared = browser
        .gql(
            "mutation { updateMe(input: { displayName: null }) { displayName providerName locale } }",
            json!({}),
        )
        .await;
    assert_eq!(
        cleared.json["data"]["updateMe"]["displayName"], "Fictive ada",
        "{}",
        cleared.json
    );
    assert_eq!(cleared.json["data"]["updateMe"]["locale"], "de");
    let refused = browser
        .gql(
            "mutation { updateMe(input: { locale: \"ja\" }) { locale } }",
            json!({}),
        )
        .await;
    assert_eq!(
        refused.json["errors"][0]["extensions"]["code"],
        "VALIDATION_ERROR"
    );
}

#[tokio::test]
async fn an_uploaded_avatar_is_cropped_reencoded_served_and_removed() {
    let provider = MockProvider::start().await;
    let db = setup_db().await;
    let (app, _) = provider.app(&db, &[]);
    let (mut browser, id) = signed_in(&app, &provider, &MockUser::new("ben")).await;

    let (body, content_type) =
        multipart(&png(300, 200), &[("x", "50"), ("y", "0"), ("size", "200")]);
    let uploaded = browser
        .send(
            Method::PUT,
            "/api/v1/me/avatar",
            Some(body),
            Some(&content_type),
        )
        .await;
    assert_eq!(uploaded.status, StatusCode::OK, "{}", uploaded.json);
    let version = uploaded.json["avatar"]["version"]
        .as_str()
        .expect("an avatar")
        .to_string();
    assert_eq!(version.len(), 16, "{version}");

    for size in [256, 64] {
        // Immutable at its version (docs/architecture.md §4.3).
        let current = format!("/api/v1/users/{id}/avatar?size={size}&v={version}");
        let image = browser.get(&current).await;
        assert_eq!(image.status, StatusCode::OK);
        assert_eq!(image.headers[header::CONTENT_TYPE], "image/jpeg");
        assert_eq!(
            image.headers[header::CACHE_CONTROL],
            "private, max-age=31536000, immutable"
        );
        assert_eq!(image.headers[header::VARY], "Cookie, Authorization");
        assert_eq!(image.headers[header::CONTENT_SECURITY_POLICY], "sandbox");
        assert!(image.headers.get(header::ETAG).is_none());
        let decoded = image::load_from_memory(&image.bytes).expect("a JPEG");
        assert_eq!((decoded.width(), decoded.height()), (size, size));
        // Redirected there from any other version, or none.
        for other in [
            format!("/api/v1/users/{id}/avatar?size={size}"),
            format!("/api/v1/users/{id}/avatar?size={size}&v=0000000000000000"),
        ] {
            let moved = browser.get(&other).await;
            assert_eq!(moved.status, StatusCode::TEMPORARY_REDIRECT, "{other}");
            assert_eq!(moved.headers[header::LOCATION], current.as_str());
            assert_eq!(moved.headers[header::CACHE_CONTROL], "no-store");
        }
    }
    let out_of_image = multipart(&png(100, 100), &[("x", "50"), ("y", "0"), ("size", "80")]);
    let refused = browser
        .send(
            Method::PUT,
            "/api/v1/me/avatar",
            Some(out_of_image.0),
            Some(&out_of_image.1),
        )
        .await;
    assert_eq!(refused.status, StatusCode::BAD_REQUEST);
    assert_eq!(
        browser
            .get(&format!("/api/v1/users/{id}/avatar?size=128"))
            .await
            .status,
        StatusCode::BAD_REQUEST
    );

    // GraphQL: the profile gives the image's address; an upload replaces it.
    let profile = browser
        .gql(
            "query($id: ID!) { userProfile(userId: $id) { displayName avatar { version } avatarUrl(size: 64) } }",
            json!({ "id": id }),
        )
        .await;
    let url = profile.json["data"]["userProfile"]["avatarUrl"]
        .as_str()
        .expect("a URL");
    assert_eq!(
        url,
        format!("/api/v1/users/{id}/avatar?size=64&v={version}")
    );
    let encoded = base64::engine::general_purpose::STANDARD.encode(png(64, 64));
    let replaced = browser
        .gql(
            "mutation($image: String!) { uploadMyAvatar(input: { contentBase64: $image }) { avatar { version } } }",
            json!({ "image": encoded }),
        )
        .await;
    let new_version = replaced.json["data"]["uploadMyAvatar"]["avatar"]["version"]
        .as_str()
        .expect("replaced")
        .to_string();
    assert_ne!(new_version, version);
    // The old address redirects to the new one, never serving it itself.
    let moved = browser
        .get(&format!("/api/v1/users/{id}/avatar?size=64&v={version}"))
        .await;
    assert_eq!(moved.status, StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(
        moved.headers[header::LOCATION],
        format!("/api/v1/users/{id}/avatar?size=64&v={new_version}").as_str()
    );
    // The replaced avatar's files are gone.
    let stored = |version: &str| {
        std::fs::read_dir(crate::common::test_media_root().join(format!("users/{id}/avatar")))
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                name.starts_with(version) && name.ends_with("-256.jpg")
            })
            .collect::<Vec<_>>()
    };
    assert!(stored(&version).is_empty());
    let current = stored(&new_version)
        .pop()
        .expect("the new avatar is stored");

    let removed = browser.gql("mutation { deleteMyAvatar }", json!({})).await;
    assert_eq!(
        removed.json["data"]["deleteMyAvatar"], true,
        "{}",
        removed.json
    );
    assert!(!current.exists());
    assert!(browser.get("/api/v1/me").await.json["avatar"].is_null());
    assert_eq!(
        browser
            .get(&format!("/api/v1/users/{id}/avatar"))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let (body, content_type) = multipart(&png(40, 40), &[]);
    browser
        .send(
            Method::PUT,
            "/api/v1/me/avatar",
            Some(body),
            Some(&content_type),
        )
        .await;
    assert_eq!(
        browser
            .send(Method::DELETE, "/api/v1/me/avatar", None, None)
            .await
            .status,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn a_user_lists_and_ends_their_sessions_on_both_surfaces() {
    let provider = MockProvider::start().await;
    let db = setup_db().await;
    let (app, _) = provider.app(&db, &[]);
    let user = MockUser::new("dan");
    let (mut laptop, _) = signed_in(&app, &provider, &user).await;
    let (mut phone, _) = signed_in(&app, &provider, &user).await;

    let sessions = laptop.get("/api/v1/me/sessions").await;
    let sessions = sessions.json.as_array().unwrap().clone();
    assert_eq!(sessions.len(), 2);
    let current: Vec<&Value> = sessions.iter().filter(|s| s["current"] == true).collect();
    assert_eq!(current.len(), 1);
    assert_eq!(current[0]["browser"], "firefox");
    assert_eq!(current[0]["client_id"], "oxidgene-web");
    let other = sessions
        .iter()
        .find(|session| session["current"] == false)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let listed = laptop
        .gql("{ mySessions { id current browser } }", json!({}))
        .await;
    assert_eq!(
        listed.json["data"]["mySessions"].as_array().unwrap().len(),
        2
    );
    let ended = laptop
        .gql(
            "mutation($id: ID!) { endMySession(sessionId: $id) }",
            json!({ "id": other }),
        )
        .await;
    assert_eq!(ended.json["data"]["endMySession"], true, "{}", ended.json);
    // Another session ended: this browser keeps its pictures.
    assert!(ended.headers.get("clear-site-data").is_none());
    assert_eq!(
        phone.get("/api/v1/me").await.status,
        StatusCode::UNAUTHORIZED
    );
    let gone = laptop
        .send(
            Method::DELETE,
            &format!("/api/v1/me/sessions/{other}"),
            None,
            None,
        )
        .await;
    assert_eq!(gone.status, StatusCode::NOT_FOUND);

    // Ending the current one signs out.
    let mine = laptop.get("/api/v1/me/sessions").await.json[0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let signed_out = laptop
        .send(
            Method::DELETE,
            &format!("/api/v1/me/sessions/{mine}"),
            None,
            None,
        )
        .await;
    assert_eq!(signed_out.status, StatusCode::NO_CONTENT);
    assert!(!laptop.cookies.contains_key("__Host-oxidgene"));
    assert_eq!(signed_out.headers["clear-site-data"], "\"cache\"");
}

#[tokio::test]
async fn nobody_sees_a_user_they_share_nothing_with() {
    let provider = MockProvider::start().await;
    let db = setup_db().await;
    let (app, _) = provider.app(
        &db,
        &[("auth_bootstrap_admin_email", "eve@example.invalid")],
    );
    let (mut admin, _) = signed_in(&app, &provider, &MockUser::new("eve")).await;
    let (mut finn, finn_id) = signed_in(&app, &provider, &MockUser::new("finn")).await;
    let (mut gail, _) = signed_in(&app, &provider, &MockUser::new("gail")).await;

    assert_eq!(
        gail.get(&format!("/api/v1/users/{finn_id}")).await.status,
        StatusCode::NOT_FOUND
    );
    let hidden = gail
        .gql(
            "query($id: ID!) { userProfile(userId: $id) { id } }",
            json!({ "id": finn_id }),
        )
        .await;
    assert!(
        hidden.json["data"]["userProfile"].is_null(),
        "{}",
        hidden.json
    );
    let own = finn.get(&format!("/api/v1/users/{finn_id}")).await;
    assert_eq!(own.json["display_name"], "Fictive finn");
    let seen = admin.get(&format!("/api/v1/users/{finn_id}")).await;
    assert_eq!(seen.status, StatusCode::OK);

    // Administration: forbidden to a user on both surfaces.
    assert_eq!(
        gail.get("/api/v1/admin/users").await.status,
        StatusCode::FORBIDDEN
    );
    let refused = gail
        .gql("{ users(first: 5) { totalCount } }", json!({}))
        .await;
    assert_eq!(refused.json["errors"][0]["extensions"]["code"], "FORBIDDEN");
    let refused = gail
        .gql(
            "mutation($id: ID!) { updateUser(userId: $id, input: { status: \"deactivated\" }) { id } }",
            json!({ "id": finn_id }),
        )
        .await;
    assert_eq!(refused.json["errors"][0]["extensions"]["code"], "FORBIDDEN");
    let users = admin.gql("{ users(first: 2) { totalCount edges { node { id status } } pageInfo { hasNextPage } } }", json!({})).await;
    assert_eq!(
        users.json["data"]["users"]["totalCount"], 3,
        "{}",
        users.json
    );
    assert_eq!(users.json["data"]["users"]["pageInfo"]["hasNextPage"], true);
    for (first, rows) in [(0, 1), (101, 3)] {
        let page = admin
            .get(&format!("/api/v1/admin/users?first={first}"))
            .await;
        assert_eq!(
            page.json["edges"].as_array().unwrap().len(),
            rows,
            "first={first}"
        );
    }
    let page = admin.get("/api/v1/admin/users?first=1").await;
    let cursor = page.json["page_info"]["end_cursor"]
        .as_str()
        .unwrap()
        .to_string();
    let next = admin
        .get(&format!("/api/v1/admin/users?first=1&after={cursor}"))
        .await;
    assert_ne!(
        next.json["edges"][0]["node"]["id"],
        page.json["edges"][0]["node"]["id"]
    );
}

#[tokio::test]
async fn an_administrator_manages_users_on_both_surfaces() {
    let provider = MockProvider::start().await;
    let db = setup_db().await;
    let (app, _) = provider.app(
        &db,
        &[("auth_bootstrap_admin_email", "hal@example.invalid")],
    );
    let (mut admin, _) = signed_in(&app, &provider, &MockUser::new("hal")).await;
    let ida = MockUser::new("ida");
    let (mut browser, ida_id) = signed_in(&app, &provider, &ida).await;
    let (_, jon_id) = signed_in(&app, &provider, &MockUser::new("jon")).await;

    // Promotion ends the user's sessions: they sign in again with the role.
    let promoted = admin
        .gql(
            "mutation($id: ID!) { updateUser(userId: $id, input: { instanceRole: \"admin\", canCreateTrees: false }) { instanceRole canCreateTrees } }",
            json!({ "id": ida_id }),
        )
        .await;
    assert_eq!(
        promoted.json["data"]["updateUser"]["instanceRole"], "admin",
        "{}",
        promoted.json
    );
    assert_eq!(
        browser.get("/api/v1/me").await.status,
        StatusCode::UNAUTHORIZED
    );
    let invalid = admin
        .json(
            Method::PUT,
            &format!("/api/v1/admin/users/{ida_id}"),
            json!({ "status": "erased" }),
        )
        .await;
    assert_eq!(invalid.status, StatusCode::BAD_REQUEST);

    // Rebinding: an identity another user holds is a conflict.
    let one = admin
        .get(&format!("/api/v1/admin/users/{jon_id}"))
        .await
        .json;
    let taken = admin
        .json(
            Method::PUT,
            &format!("/api/v1/admin/users/{ida_id}/identity"),
            json!({ "issuer": one["issuer"], "subject": one["subject"] }),
        )
        .await;
    assert_eq!(taken.status, StatusCode::CONFLICT);
    let moved = MockUser {
        subject: format!("{}-moved", ida.subject),
        ..ida.clone()
    };
    let rebound = admin
        .gql(
            "mutation($id: ID!, $issuer: String!, $subject: String!) { rebindUserIdentity(userId: $id, input: { issuer: $issuer, subject: $subject }) { subject } }",
            json!({ "id": ida_id, "issuer": provider.issuer(), "subject": moved.subject }),
        )
        .await;
    assert_eq!(
        rebound.json["data"]["rebindUserIdentity"]["subject"],
        json!(moved.subject),
        "{}",
        rebound.json
    );
    let (_, same) = signed_in(&app, &provider, &moved).await;
    assert_eq!(same, ida_id, "the new identity is the same user");

    // The account data, then erasure through GraphQL.
    let (mut again, _) = signed_in(&app, &provider, &moved).await;
    let export = again.get("/api/v1/me/export").await;
    assert_eq!(
        export.headers[header::CONTENT_DISPOSITION],
        "attachment; filename=\"oxidgene-account.json\""
    );
    assert_eq!(export.json["user"]["id"], json!(ida_id));
    assert_eq!(export.json["user"]["email"], ida.email());
    let data = again
        .gql("{ myAccountData { user { id email } } }", json!({}))
        .await;
    assert_eq!(
        data.json["data"]["myAccountData"]["user"]["email"],
        ida.email()
    );
    let erased = admin
        .gql(
            "mutation($id: ID!) { eraseUser(userId: $id) }",
            json!({ "id": ida_id }),
        )
        .await;
    assert_eq!(erased.json["data"]["eraseUser"], true, "{}", erased.json);
    assert_eq!(
        again.get("/api/v1/me").await.status,
        StatusCode::UNAUTHORIZED
    );
    let tombstone = admin.get(&format!("/api/v1/admin/users/{ida_id}")).await;
    assert_eq!(tombstone.json["status"], "erased");
    assert_eq!(
        admin
            .send(
                Method::DELETE,
                &format!("/api/v1/admin/users/{ida_id}"),
                None,
                None
            )
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn an_anonymous_request_is_refused_alike_on_both_surfaces() {
    let provider = MockProvider::start().await;
    let db = setup_db().await;
    let (app, _) = provider.app(&db, &[]);
    let mut anonymous = Browser::new(&app);
    for uri in ["/api/v1/me", "/api/v1/admin/users", "/api/v1/me/sessions"] {
        let answer = anonymous.get(uri).await;
        assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{uri}");
        assert_eq!(answer.json["error"], "unauthenticated");
    }
    // A GraphQL request without a credential executes as an anonymous
    // viewer, and each field it may not use answers `UNAUTHENTICATED`
    // (docs/authentication.md §4.8); an open field still answers.
    let graphql = anonymous.gql("{ me { id } }", json!({})).await;
    assert_eq!(graphql.status, StatusCode::OK);
    assert_eq!(
        graphql.json["errors"][0]["extensions"]["code"],
        "UNAUTHENTICATED"
    );
    assert_eq!(graphql.json["errors"][0]["path"][0], "me");
    let open = anonymous
        .gql("{ exportDestinations { id } }", json!({}))
        .await;
    assert!(
        open.json["data"]["exportDestinations"].is_array(),
        "{}",
        open.json
    );
}

/// Ending one's own session over GraphQL signs the browser out as REST does:
/// its cookie and the pictures it kept go (docs/authentication.md §9.1).
#[tokio::test]
async fn ending_ones_own_session_over_graphql_clears_the_browsers_cache() {
    let provider = MockProvider::start().await;
    let db = setup_db().await;
    let (app, _) = provider.app(&db, &[]);
    let (mut browser, _) = signed_in(&app, &provider, &MockUser::new("gil")).await;
    let mine = browser.get("/api/v1/me/sessions").await.json[0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let ended = browser
        .gql(
            "mutation($id: ID!) { endMySession(sessionId: $id) }",
            json!({ "id": mine }),
        )
        .await;
    assert_eq!(ended.json["data"]["endMySession"], true, "{}", ended.json);
    assert_eq!(ended.headers["clear-site-data"], "\"cache\"");
    assert!(!browser.cookies.contains_key("__Host-oxidgene"));
}
