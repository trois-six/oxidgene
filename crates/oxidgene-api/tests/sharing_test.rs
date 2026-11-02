//! Sharing (docs/authorization.md §6, §12 "Scopes"): sharing with users of
//! the instance, invitations by email, share links, the tree's visibility
//! with its visitor scope, access scopes kept current, and a scoped
//! Editor's writes — on REST and GraphQL alike, through the
//! Backend-for-Frontend's sessions of fictitious users.

use axum::Router;
use axum::http::{Method, StatusCode};
use oxidgene_core::access::TreeRole;
use oxidgene_db::repo::{AccessScopeRepo, AppUserRepo, MembershipRepo};
use oxidgene_db::sea_orm::DatabaseConnection;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::common::canary::Canaries;
use crate::common::oidc::{Browser, MockProvider, MockUser, PUBLIC_URL};
use crate::common::{app_on, ok, setup_db};

/// A tree of the canary family owned by a signed-in owner, on an `oidc`
/// application.
struct Fixture {
    provider: MockProvider,
    app: Router,
    /// The same database without authentication, to build fixtures.
    local: Router,
    db: DatabaseConnection,
    owner: Browser,
    canaries: Canaries,
}

impl Fixture {
    async fn new(extra: &[(&str, &str)]) -> Self {
        let provider = MockProvider::start().await;
        let db = setup_db().await;
        let (app, _) = provider.app(&db, extra);
        let local = app_on(db.clone());
        let canaries = Canaries::new(&local, &db).await;
        let mut owner = Browser::new(&app);
        let owner_user = MockUser::new("owner");
        let signed = owner.sign_in(&provider, &owner_user).await;
        assert_eq!(signed.status, StatusCode::SEE_OTHER);
        let owner_id = user_id(&db, &provider, &owner_user).await;
        let tree = Uuid::parse_str(&canaries.tree).unwrap();
        MembershipRepo::create(&db, tree, owner_id, TreeRole::Owner, None)
            .await
            .unwrap();
        Self {
            provider,
            app,
            local,
            db,
            owner,
            canaries,
        }
    }

    fn uri(&self, path: &str) -> String {
        format!("/api/v1/trees/{}{path}", self.canaries.tree)
    }

    fn person(&self, given: &str) -> String {
        self.canaries.person(given).to_owned()
    }

    /// A signed-in browser of a new fictitious user.
    async fn user(&self, name: &str) -> (Browser, MockUser) {
        let mut browser = Browser::new(&self.app);
        let user = MockUser::new(name);
        let answer = browser.sign_in(&self.provider, &user).await;
        assert_eq!(answer.status, StatusCode::SEE_OTHER, "{}", answer.json);
        (browser, user)
    }
}

async fn user_id(db: &DatabaseConnection, provider: &MockProvider, user: &MockUser) -> Uuid {
    AppUserRepo::find_by_identity(db, provider.issuer(), &user.subject)
        .await
        .unwrap()
        .expect("provisioned")
        .id
}

fn role_of(list: &Value, tree: &str) -> Option<String> {
    list["edges"]
        .as_array()?
        .iter()
        .find(|edge| edge["node"]["id"] == tree)
        .map(|edge| {
            edge["node"]["viewer_access"]["role"]
                .as_str()
                .unwrap()
                .to_owned()
        })
}

#[tokio::test]
async fn users_are_shared_with_by_email_username_and_among_known_people() {
    let mut f = Fixture::new(&[]).await;
    let (mut guest, guest_user) = f.user("guestone").await;
    let (mut editor, _) = f.user("editorone").await;

    // By exact email, on REST: a membership at once.
    let shared = f
        .owner
        .json(
            Method::POST,
            &f.uri("/members"),
            json!({ "email": guest_user.email().to_uppercase(), "role": "guest" }),
        )
        .await;
    assert_eq!(shared.status, StatusCode::CREATED, "{}", shared.json);
    assert_eq!(shared.json["member"]["role"], "guest");
    assert!(shared.json["invitation"].is_null());
    let list = guest.get("/api/v1/trees").await;
    assert_eq!(
        role_of(&list.json, &f.canaries.tree).as_deref(),
        Some("guest")
    );

    // By exact username, on GraphQL, limited to a scope.
    let answer = f
        .owner
        .gql(
            "mutation($tree: ID!, $root: ID!) { shareTree(treeId: $tree, input: { username: \"editorone\", role: \"editor\", scope: { kind: DESCENDANTS_OF, personId: $root } }) { member { role scope { kind personName personDeleted } } invitation { id } } }",
            json!({ "tree": f.canaries.tree, "root": f.person("Corwin") }),
        )
        .await;
    let member = &answer.json["data"]["shareTree"]["member"];
    assert_eq!(member["role"], "editor", "{}", answer.json);
    assert_eq!(member["scope"]["kind"], "DESCENDANTS_OF");
    assert_eq!(member["scope"]["personDeleted"], false);
    let tree = editor.get(&f.uri("")).await;
    assert_eq!(tree.json["viewer_access"]["scoped"], true, "{}", tree.json);

    // Someone already a member, an unknown username, the owner's role.
    for (body, status) in [
        (
            json!({ "username": "guestone", "role": "guest" }),
            StatusCode::CONFLICT,
        ),
        (
            json!({ "username": "nobody-here", "role": "guest" }),
            StatusCode::NOT_FOUND,
        ),
        (
            json!({ "username": "guestone", "role": "owner" }),
            StatusCode::BAD_REQUEST,
        ),
        (json!({ "role": "guest" }), StatusCode::BAD_REQUEST),
    ] {
        let answer = f
            .owner
            .json(Method::POST, &f.uri("/members"), body.clone())
            .await;
        assert_eq!(answer.status, status, "{body} {}", answer.json);
    }

    // The people the owner already shares with: the members of their other
    // trees, never an address.
    let other = ok(
        &f.local,
        Method::POST,
        "/api/v1/trees",
        Some(json!({ "name": "Elsewhere" })),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .parse::<Uuid>()
        .unwrap();
    let owner_id = MembershipRepo::owner(&f.db, f.canaries.tree.parse().unwrap())
        .await
        .unwrap()
        .unwrap()
        .user_id;
    MembershipRepo::create(&f.db, other, owner_id, TreeRole::Owner, None)
        .await
        .unwrap();
    let (_, cousin_user) = f.user("cousinone").await;
    let cousin = user_id(&f.db, &f.provider, &cousin_user).await;
    MembershipRepo::create(&f.db, other, cousin, TreeRole::Guest, None)
        .await
        .unwrap();
    let candidates = f.owner.get(&f.uri("/share-candidates")).await;
    assert_eq!(candidates.status, StatusCode::OK);
    let candidates = candidates.json.as_array().unwrap().clone();
    assert_eq!(candidates.len(), 1, "{candidates:?}");
    assert_eq!(candidates[0]["user_id"], cousin.to_string());
    assert!(!candidates[0].to_string().contains("example.invalid"));
    let gql = f
        .owner
        .gql(
            "query($tree: ID!) { shareCandidates(treeId: $tree) { userId displayName } }",
            json!({ "tree": f.canaries.tree }),
        )
        .await;
    assert_eq!(
        gql.json["data"]["shareCandidates"][0]["userId"],
        cousin.to_string()
    );
    let picked = f
        .owner
        .json(
            Method::POST,
            &f.uri("/members"),
            json!({ "user_id": cousin, "role": "guest" }),
        )
        .await;
    assert_eq!(picked.status, StatusCode::CREATED, "{}", picked.json);
    // A user nobody shares with is not found by id.
    let stranger = f
        .owner
        .json(
            Method::POST,
            &f.uri("/members"),
            json!({ "user_id": Uuid::now_v7(), "role": "guest" }),
        )
        .await;
    assert_eq!(stranger.status, StatusCode::NOT_FOUND);

    // A member's scope changes, and is cleared, through the member update.
    let editor_id =
        MembershipRepo::list(&f.db, f.canaries.tree.parse().unwrap(), &Default::default())
            .await
            .unwrap()
            .edges
            .into_iter()
            .map(|edge| edge.node)
            .find(|m| m.role == TreeRole::Editor)
            .unwrap()
            .user_id;
    let cleared = f
        .owner
        .json(
            Method::PUT,
            &f.uri(&format!("/members/{editor_id}")),
            json!({ "scope": null }),
        )
        .await;
    assert_eq!(cleared.status, StatusCode::OK, "{}", cleared.json);
    assert!(cleared.json["scope"].is_null());
    let tree = editor.get(&f.uri("")).await;
    assert_eq!(tree.json["viewer_access"]["scoped"], false);

    // The history records each share under `access`.
    let audit = f.owner.get(&f.uri("/audit?category=access")).await;
    let shares = audit.json["edges"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|edge| edge["node"]["entity"] == "membership")
        .count();
    assert!(shares >= 4, "{}", audit.json);
}

#[tokio::test]
async fn lookups_are_limited_to_thirty_an_hour() {
    let mut f = Fixture::new(&[]).await;
    let mut last = StatusCode::OK;
    for n in 0..31 {
        let answer = f
            .owner
            .json(
                Method::POST,
                &f.uri("/members"),
                json!({ "username": format!("absent-{n}"), "role": "guest" }),
            )
            .await;
        last = answer.status;
        if n < 30 {
            assert_eq!(answer.status, StatusCode::NOT_FOUND);
        } else {
            assert!(answer.headers.contains_key("retry-after"));
        }
    }
    assert_eq!(last, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn an_unknown_address_is_invited_and_accepts_once_signed_in() {
    let mut f = Fixture::new(&[
        ("auth_signup", "invitation"),
        ("auth_bootstrap_admin_email", "owner@example.invalid"),
    ])
    .await;
    // The signup gate refuses an identity nobody invited.
    let mut stranger = Browser::new(&f.app);
    let refused = stranger
        .sign_in(&f.provider, &MockUser::new("uninvited"))
        .await;
    assert!(
        refused.location().contains("signup_closed"),
        "{}",
        refused.location()
    );

    let invited = f
        .owner
        .json(
            Method::POST,
            &f.uri("/members"),
            json!({ "email": "newcomer@example.invalid", "role": "guest", "scope": { "kind": "ancestors_of", "person_id": f.person("Harlan") } }),
        )
        .await;
    assert_eq!(invited.status, StatusCode::ACCEPTED, "{}", invited.json);
    let invitation = &invited.json["invitation"];
    assert_eq!(invitation["delivery"], "manual");
    assert_eq!(invitation["link"], format!("{PUBLIC_URL}/invitations"));
    assert_eq!(invitation["email"], "newcomer@example.invalid");
    let invitation_id = invitation["id"].as_str().unwrap().to_owned();
    // A second invitation of the same address waits for the first.
    let again = f
        .owner
        .json(
            Method::POST,
            &f.uri("/members"),
            json!({ "email": "newcomer@example.invalid", "role": "guest" }),
        )
        .await;
    assert_eq!(again.status, StatusCode::CONFLICT);

    // Listed on both surfaces, sent again.
    let list = f.owner.get(&f.uri("/invitations")).await;
    assert_eq!(list.json["total_count"], 1, "{}", list.json);
    let gql = f
        .owner
        .gql(
            "query($tree: ID!) { treeInvitations(treeId: $tree) { totalCount edges { node { email delivery scope { kind } } } } }",
            json!({ "tree": f.canaries.tree }),
        )
        .await;
    let node = &gql.json["data"]["treeInvitations"]["edges"][0]["node"];
    assert_eq!(node["delivery"], "MANUAL", "{}", gql.json);
    assert_eq!(node["scope"]["kind"], "ANCESTORS_OF");
    let resent = f
        .owner
        .send(
            Method::POST,
            &f.uri(&format!("/invitations/{invitation_id}/resend")),
            None,
            None,
        )
        .await;
    assert_eq!(resent.status, StatusCode::OK, "{}", resent.json);

    // The invitation opens the signup gate for its address.
    let mut newcomer = Browser::new(&f.app);
    let newcomer_user = MockUser::new("newcomer");
    let signed = newcomer.sign_in(&f.provider, &newcomer_user).await;
    assert!(
        !signed.location().contains("auth_error"),
        "{}",
        signed.location()
    );
    let mine = newcomer.get("/api/v1/me/invitations").await;
    let mine = mine.json.as_array().unwrap().clone();
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0]["tree_name"], "Wrenfield");
    assert_eq!(mine[0]["scoped"], true);

    // Another account, whoever holds the link, accepts nothing.
    let (mut other, _) = {
        let mut browser = Browser::new(&f.app);
        let user = MockUser::new("newcomer").with("email", json!("someone-else@example.invalid"));
        // Not invited: refused at the gate, so share it by name first.
        browser.sign_in(&f.provider, &user).await;
        (browser, user)
    };
    let foreign = other
        .send(
            Method::POST,
            &format!("/api/v1/me/invitations/{invitation_id}/accept"),
            None,
            None,
        )
        .await;
    assert!(
        matches!(
            foreign.status,
            StatusCode::NOT_FOUND | StatusCode::UNAUTHORIZED
        ),
        "{}",
        foreign.status
    );

    let accepted = newcomer
        .send(
            Method::POST,
            &format!("/api/v1/me/invitations/{invitation_id}/accept"),
            None,
            None,
        )
        .await;
    assert_eq!(accepted.status, StatusCode::OK, "{}", accepted.json);
    assert_eq!(accepted.json["member"]["role"], "guest");
    assert_eq!(accepted.json["member"]["scope"]["kind"], "ancestors_of");
    let list = newcomer.get("/api/v1/trees").await;
    assert_eq!(
        role_of(&list.json, &f.canaries.tree).as_deref(),
        Some("guest")
    );
    assert!(
        newcomer
            .get("/api/v1/me/invitations")
            .await
            .json
            .as_array()
            .unwrap()
            .is_empty()
    );
    let pending = f.owner.get(&f.uri("/invitations")).await;
    assert_eq!(pending.json["total_count"], 0);

    // Declined on GraphQL, revoked on REST.
    let second = f
        .owner
        .json(
            Method::POST,
            &f.uri("/members"),
            json!({ "email": "laterone@example.invalid", "role": "editor" }),
        )
        .await;
    let second_id = second.json["invitation"]["id"].as_str().unwrap().to_owned();
    let mut later = Browser::new(&f.app);
    later.sign_in(&f.provider, &MockUser::new("laterone")).await;
    let declined = later
        .gql(
            "mutation($id: ID!) { declineInvitation(invitationId: $id) }",
            json!({ "id": second_id }),
        )
        .await;
    assert_eq!(
        declined.json["data"]["declineInvitation"], true,
        "{}",
        declined.json
    );
    let third = f
        .owner
        .gql(
            "mutation($tree: ID!) { shareTree(treeId: $tree, input: { email: \"thirdone@example.invalid\", role: \"guest\" }) { invitation { id link } } }",
            json!({ "tree": f.canaries.tree }),
        )
        .await;
    let third_id = third.json["data"]["shareTree"]["invitation"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let revoked = f
        .owner
        .send(
            Method::DELETE,
            &f.uri(&format!("/invitations/{third_id}")),
            None,
            None,
        )
        .await;
    assert_eq!(revoked.status, StatusCode::NO_CONTENT);
    let audit = f.owner.get(&f.uri("/audit?category=access")).await;
    assert!(
        audit.json.to_string().contains("tree_invitation"),
        "{}",
        audit.json
    );
}

/// Open share link `url` in `browser`: the answer, a redirect to the tree.
async fn open(browser: &mut Browser, url: &str) -> crate::common::oidc::Answer {
    let path = url.strip_prefix(PUBLIC_URL).unwrap();
    browser.get(path).await
}

#[tokio::test]
async fn a_share_link_makes_a_visitor_of_its_holder_until_revoked() {
    let mut f = Fixture::new(&[]).await;
    let created = f
        .owner
        .json(
            Method::POST,
            &f.uri("/share-links"),
            json!({ "label": "Cousins" }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.json);
    let url = created.json["url"].as_str().unwrap().to_owned();
    assert!(
        url.starts_with(&format!("{PUBLIC_URL}/auth/share/ogs_")),
        "{url}"
    );
    let link_id = created.json["link"]["id"].as_str().unwrap().to_owned();
    // Never readable again.
    let links = f.owner.get(&f.uri("/share-links")).await;
    assert!(!links.json.to_string().contains("ogs_"));

    // Without it, an anonymous browser is asked to sign in.
    let mut holder = Browser::new(&f.app);
    assert_eq!(
        holder.get(&f.uri("")).await.status,
        StatusCode::UNAUTHORIZED
    );
    let opened = open(&mut holder, &url).await;
    assert_eq!(opened.status, StatusCode::SEE_OTHER);
    assert_eq!(opened.location(), format!("/trees/{}", f.canaries.tree));
    assert!(holder.cookies.contains_key("__Host-oxidgene-share"));
    let tree = holder.get(&f.uri("")).await;
    assert_eq!(tree.status, StatusCode::OK, "{}", tree.json);
    assert_eq!(tree.json["viewer_access"]["role"], "visitor");
    assert_eq!(tree.json["viewer_access"]["permissions"], json!(["read"]));
    let gql = holder
        .gql(
            "query($tree: ID!) { tree(id: $tree) { name viewerAccess { role } } }",
            json!({ "tree": f.canaries.tree }),
        )
        .await;
    assert_eq!(
        gql.json["data"]["tree"]["viewerAccess"]["role"], "visitor",
        "{}",
        gql.json
    );
    // A Visitor reads; a write is refused.
    let write = holder
        .json(
            Method::POST,
            &f.uri("/persons"),
            json!({ "sex": "unknown" }),
        )
        .await;
    assert_eq!(write.status, StatusCode::FORBIDDEN);

    // A signed-in user keeps their own access and finds the tree in their
    // list as a Visitor.
    let (mut outsider, _) = f.user("outsider").await;
    assert_eq!(outsider.get(&f.uri("")).await.status, StatusCode::NOT_FOUND);
    open(&mut outsider, &url).await;
    let list = outsider.get("/api/v1/trees").await;
    assert_eq!(
        role_of(&list.json, &f.canaries.tree).as_deref(),
        Some("visitor")
    );

    // Counted, then revoked on GraphQL: every grant ends at the next request.
    let links = f
        .owner
        .gql(
            "query($tree: ID!) { shareLinks(treeId: $tree) { edges { node { id label useCount lastUsedAt } } } }",
            json!({ "tree": f.canaries.tree }),
        )
        .await;
    let node = &links.json["data"]["shareLinks"]["edges"][0]["node"];
    assert_eq!(node["useCount"], 2, "{}", links.json);
    assert_eq!(node["label"], "Cousins");
    let revoked = f
        .owner
        .gql(
            "mutation($tree: ID!, $id: ID!) { revokeShareLink(treeId: $tree, linkId: $id) }",
            json!({ "tree": f.canaries.tree, "id": link_id }),
        )
        .await;
    assert_eq!(
        revoked.json["data"]["revokeShareLink"], true,
        "{}",
        revoked.json
    );
    assert_eq!(
        holder.get(&f.uri("")).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(outsider.get(&f.uri("")).await.status, StatusCode::NOT_FOUND);
    // A revoked or unknown token opens nothing.
    let stale = open(&mut Browser::new(&f.app), &url).await;
    assert!(stale.location().contains("auth_error=share_link_invalid"));

    // A scoped link, changed through the update.
    let scoped = f
        .owner
        .json(
            Method::POST,
            &f.uri("/share-links"),
            json!({ "scope": { "kind": "descendants_of", "person_id": f.person("Harlan") }, "expires_at": "2999-01-01T00:00:00Z" }),
        )
        .await;
    let scoped_id = scoped.json["link"]["id"].as_str().unwrap().to_owned();
    let mut visitor = Browser::new(&f.app);
    open(&mut visitor, scoped.json["url"].as_str().unwrap()).await;
    let tree = visitor.get(&f.uri("")).await;
    assert_eq!(tree.json["viewer_access"]["scoped"], true);
    let updated = f
        .owner
        .json(
            Method::PUT,
            &f.uri(&format!("/share-links/{scoped_id}")),
            json!({ "label": "Branch", "scope": null }),
        )
        .await;
    assert_eq!(updated.status, StatusCode::OK, "{}", updated.json);
    assert!(updated.json["scope"].is_null());
    assert_eq!(
        visitor.get(&f.uri("")).await.json["viewer_access"]["scoped"],
        false
    );
}

#[tokio::test]
async fn the_trees_visibility_makes_visitors_of_the_site_or_the_web() {
    let mut f = Fixture::new(&[]).await;
    let mut anonymous = Browser::new(&f.app);
    let (mut outsider, _) = f.user("outsider").await;
    assert_eq!(
        anonymous.get(&f.uri("")).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(outsider.get(&f.uri("")).await.status, StatusCode::NOT_FOUND);

    // `site`, on REST: every signed-in user, not the anonymous.
    let site = f
        .owner
        .json(Method::PUT, &f.uri(""), json!({ "visibility": "site" }))
        .await;
    assert_eq!(site.status, StatusCode::OK, "{}", site.json);
    assert_eq!(site.json["visibility"], "site");
    assert_eq!(
        anonymous.get(&f.uri("")).await.status,
        StatusCode::UNAUTHORIZED
    );
    let seen = outsider.get(&f.uri("")).await;
    assert_eq!(
        seen.json["viewer_access"]["role"], "visitor",
        "{}",
        seen.json
    );
    // No directory: a `site` tree is reached by its address, not listed.
    let list = outsider.get("/api/v1/trees").await;
    assert_eq!(role_of(&list.json, &f.canaries.tree), None);

    // `public` with a visitor scope, on GraphQL.
    let public = f
        .owner
        .gql(
            "mutation($tree: ID!, $root: ID!) { updateTree(id: $tree, input: { visibility: PUBLIC, visitorScope: { kind: ANCESTORS_OF, personId: $root } }) { visibility visitorScopeId } }",
            json!({ "tree": f.canaries.tree, "root": f.person("Harlan") }),
        )
        .await;
    assert_eq!(
        public.json["data"]["updateTree"]["visibility"], "PUBLIC",
        "{}",
        public.json
    );
    let tree = anonymous.get(&f.uri("")).await;
    assert_eq!(tree.status, StatusCode::OK);
    assert_eq!(tree.json["viewer_access"]["role"], "visitor");
    assert_eq!(tree.json["viewer_access"]["scoped"], true);
    // A member keeps their role.
    let owner_view = f.owner.get(&f.uri("")).await;
    assert_eq!(owner_view.json["viewer_access"]["role"], "owner");
    // The visitor scope, read by the owner alone.
    let scope = f.owner.get(&f.uri("/visitor-scope")).await;
    assert_eq!(scope.json["kind"], "ancestors_of", "{}", scope.json);
    assert_eq!(scope.json["person_name"], "Harlan Wrenfield");
    let gql = f
        .owner
        .gql(
            "query($tree: ID!) { visitorScope(treeId: $tree) { kind personId } }",
            json!({ "tree": f.canaries.tree }),
        )
        .await;
    assert_eq!(gql.json["data"]["visitorScope"]["kind"], "ANCESTORS_OF");
    assert_eq!(
        outsider.get(&f.uri("/visitor-scope")).await.status,
        StatusCode::FORBIDDEN
    );

    // Back to private, the visitor scope cleared: nobody but the members.
    let private = f
        .owner
        .json(
            Method::PUT,
            &f.uri(""),
            json!({ "visibility": "private", "visitor_scope": null }),
        )
        .await;
    assert!(
        private.json["visitor_scope_id"].is_null(),
        "{}",
        private.json
    );
    assert_eq!(
        anonymous.get(&f.uri("")).await.status,
        StatusCode::UNAUTHORIZED
    );
    let audit = f.owner.get(&f.uri("/audit?category=access")).await;
    assert!(
        audit.json.to_string().contains("visibility"),
        "{}",
        audit.json
    );
}

#[tokio::test]
async fn a_local_owner_has_nobody_to_share_with() {
    let db = setup_db().await;
    let app = app_on(db.clone());
    let tree = crate::common::new_tree(&app, "Alone").await;
    let (status, _) = crate::common::send(
        &app,
        Method::PUT,
        &format!("/api/v1/trees/{tree}"),
        Some(json!({ "visibility": "public" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    for (method, path) in [
        (Method::GET, "/share-links"),
        (Method::GET, "/invitations"),
        (Method::GET, "/share-candidates"),
    ] {
        let (status, _) =
            crate::common::send(&app, method, &format!("/api/v1/trees/{tree}{path}"), None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
    }
}

/// The persons of scope `scope` as last built, by given name.
async fn scope_names(f: &Fixture, scope: Uuid) -> Vec<String> {
    let persons = AccessScopeRepo::persons(&f.db, scope).await.unwrap();
    let mut names: Vec<String> = f
        .canaries
        .persons
        .iter()
        .filter(|(_, id)| persons.contains(&id.parse::<Uuid>().unwrap()))
        .map(|(name, _)| name.clone())
        .collect();
    names.sort();
    names
}

#[tokio::test]
async fn scopes_are_materialized_kept_current_and_follow_a_merge() {
    let mut f = Fixture::new(&[]).await;
    let (_, editor_user) = f.user("editorone").await;
    let shared = f
        .owner
        .json(
            Method::POST,
            &f.uri("/members"),
            json!({ "email": editor_user.email(), "role": "editor", "scope": { "kind": "descendants_of", "person_id": f.person("Corwin"), "include_spouses": true } }),
        )
        .await;
    let scope: Uuid = shared.json["member"]["scope"]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        scope_names(&f, scope).await,
        ["Corwin", "Harlan", "Quillon", "Thistle"]
    );

    // Materialized as the bulk target walks: the preview counts it alike.
    let target = f
        .owner
        .json(
            Method::POST,
            &f.uri("/tags/target"),
            json!({ "target": { "descendants_of": f.person("Corwin"), "include_self": true, "include_spouses": true } }),
        )
        .await;
    let preview = f
        .owner
        .json(
            Method::POST,
            &f.uri("/access-scopes/preview"),
            json!({ "scope": { "kind": "descendants_of", "person_id": f.person("Corwin"), "include_spouses": true } }),
        )
        .await;
    assert_eq!(preview.status, StatusCode::OK, "{}", preview.json);
    assert_eq!(
        preview.json["person_count"], target.json["total"],
        "{}",
        target.json
    );
    // A Visitor would not see the contemporary Quillon; nobody is excluded
    // yet.
    assert_eq!(preview.json["visitor_count"], 3);
    assert_eq!(preview.json["excluded_count"], 0);
    let whole = f
        .owner
        .gql(
            "query($tree: ID!) { accessScopePreview(treeId: $tree) { personCount visitorCount } }",
            json!({ "tree": f.canaries.tree }),
        )
        .await;
    assert_eq!(
        whole.json["data"]["accessScopePreview"]["personCount"], 9,
        "{}",
        whole.json
    );
    assert_eq!(whole.json["data"]["accessScopePreview"]["visitorCount"], 7);

    // A child added under the shared root enters it at the next read.
    let child = crate::common::new_person(&f.local, &f.canaries.tree).await;
    let family = ok(&f.local, Method::POST, &f.uri("/families"), Some(json!({}))).await;
    let family = family["id"].as_str().unwrap().to_owned();
    for (path, body) in [
        (
            format!("/families/{family}/spouses"),
            json!({ "person_id": f.person("Quillon"), "role": "husband", "sort_order": 0 }),
        ),
        (
            format!("/families/{family}/children"),
            json!({ "person_id": child, "child_type": "biological", "sort_order": 0 }),
        ),
    ] {
        ok(&f.local, Method::POST, &f.uri(&path), Some(body)).await;
    }
    let (generation, built) = AccessScopeRepo::generations(&f.db, scope)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(Some(generation), built, "the link moved the generation");
    let persons = oxidgene_api::service::access_scope::scope_persons(&f.db, scope)
        .await
        .unwrap();
    assert!(persons.contains(&child.parse().unwrap()));

    // A merge absorbing the root moves the scope to the person kept.
    let twin = crate::common::new_person(&f.local, &f.canaries.tree).await;
    let merged = crate::common::send(
        &f.local,
        Method::POST,
        &f.uri(&format!("/persons/{twin}/merge")),
        Some(json!({ "duplicate_id": f.person("Corwin") })),
    )
    .await;
    assert_eq!(merged.0, StatusCode::OK, "{}", merged.1);
    let moved = AccessScopeRepo::get(&f.db, scope).await.unwrap().unwrap();
    assert_eq!(moved.definition.person_id.to_string(), twin);
    let members = f.owner.get(&f.uri("/members")).await;
    assert!(
        members
            .json
            .to_string()
            .contains("\"person_deleted\":false")
    );

    // A deleted root leaves the scope empty, and the members list says so.
    crate::common::send(
        &f.local,
        Method::DELETE,
        &f.uri(&format!("/persons/{twin}")),
        None,
    )
    .await;
    let persons = oxidgene_api::service::access_scope::scope_persons(&f.db, scope)
        .await
        .unwrap();
    assert!(persons.is_empty(), "{persons:?}");
    let members = f.owner.get(&f.uri("/members")).await;
    assert!(
        members.json.to_string().contains("\"person_deleted\":true"),
        "{}",
        members.json
    );
}

#[tokio::test]
async fn a_scoped_editor_writes_inside_the_scope_only() {
    let mut f = Fixture::new(&[]).await;
    let (mut editor, editor_user) = f.user("editorone").await;
    f.owner
        .json(
            Method::POST,
            &f.uri("/members"),
            json!({ "email": editor_user.email(), "role": "editor", "scope": { "kind": "descendants_of", "person_id": f.person("Harlan") } }),
        )
        .await;
    // A person of the scope, then one outside it.
    let inside = editor
        .json(
            Method::PUT,
            &f.uri(&format!("/persons/{}", f.person("Quillon"))),
            json!({ "sex": "male" }),
        )
        .await;
    assert_eq!(inside.status, StatusCode::OK, "{}", inside.json);
    let outside = editor
        .json(
            Method::PUT,
            &f.uri(&format!("/persons/{}", f.person("Corwin"))),
            json!({ "sex": "male" }),
        )
        .await;
    assert_eq!(outside.status, StatusCode::NOT_FOUND, "{}", outside.json);
    let gql = editor
        .gql(
            "mutation($tree: ID!, $id: ID!) { updatePerson(treeId: $tree, id: $id, input: { sex: MALE }) { id } }",
            json!({ "tree": f.canaries.tree, "id": f.person("Corwin") }),
        )
        .await;
    assert_eq!(
        gql.json["errors"][0]["extensions"]["code"], "NOT_FOUND",
        "{}",
        gql.json
    );

    // An event of a person outside, named through the event's body.
    let event = editor
        .json(
            Method::POST,
            &f.uri("/events"),
            json!({ "event_type": "occupation", "person_id": f.person("Ashby") }),
        )
        .await;
    assert_eq!(event.status, StatusCode::NOT_FOUND, "{}", event.json);

    // A new person, linked as a child of a person of the scope, comes in.
    let created = editor
        .json(Method::POST, &f.uri("/persons"), json!({ "sex": "female" }))
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.json);
    let child = created.json["id"].as_str().unwrap().to_owned();
    let family = editor
        .json(Method::POST, &f.uri("/families"), json!({}))
        .await;
    let family = family.json["id"].as_str().unwrap().to_owned();
    for (path, body) in [
        (
            format!("/families/{family}/spouses"),
            json!({ "person_id": f.person("Quillon"), "role": "husband", "sort_order": 0 }),
        ),
        (
            format!("/families/{family}/children"),
            json!({ "person_id": child, "child_type": "biological", "sort_order": 0 }),
        ),
    ] {
        let linked = editor.json(Method::POST, &f.uri(&path), body).await;
        assert!(linked.status.is_success(), "{path}: {}", linked.json);
    }
    let edited = editor
        .json(
            Method::PUT,
            &f.uri(&format!("/persons/{child}")),
            json!({ "sex": "male" }),
        )
        .await;
    assert_eq!(edited.status, StatusCode::OK, "{}", edited.json);

    // Shared records are read and linked, never edited.
    let place = editor
        .json(
            Method::POST,
            &f.uri("/places"),
            json!({ "name": "Northfield" }),
        )
        .await;
    assert_eq!(place.status, StatusCode::CREATED, "{}", place.json);
    let place_id = place.json["id"].as_str().unwrap().to_owned();
    let renamed = editor
        .json(
            Method::PUT,
            &f.uri(&format!("/places/{place_id}")),
            json!({ "name": "Southfield" }),
        )
        .await;
    assert_eq!(renamed.status, StatusCode::FORBIDDEN);
    let gql = editor
        .gql(
            "mutation($tree: ID!, $id: ID!) { deletePlace(treeId: $tree, id: $id) }",
            json!({ "tree": f.canaries.tree, "id": place_id }),
        )
        .await;
    assert_eq!(
        gql.json["errors"][0]["extensions"]["code"], "FORBIDDEN",
        "{}",
        gql.json
    );
}

/// An Editor's writes naming a person the owner excluded are `404`, as a
/// read of that person is, and those touching a family with an excluded
/// spouse `403` (docs/privacy.md §6.4), on REST and GraphQL alike; nothing
/// they asked for is written, and the Editor's other writes go through.
#[tokio::test]
async fn an_editors_writes_naming_an_excluded_person_are_refused() {
    let mut f = Fixture::new(&[]).await;
    let (mut editor, editor_user) = f.user("editortwo").await;
    f.owner
        .json(
            Method::POST,
            &f.uri("/members"),
            json!({ "email": editor_user.email(), "role": "editor" }),
        )
        .await;
    let excluded = f.person("Thistle");
    ok(
        &f.local,
        Method::POST,
        &f.uri("/exclusions"),
        Some(json!({ "kind": "person", "person_id": excluded })),
    )
    .await;
    let profiles = crate::common::all_profiles(&f.local, &f.canaries.tree).await;
    let profile = profiles
        .iter()
        .find(|p| p["person_id"] == excluded.as_str())
        .expect("the excluded person");
    let text = |pointer: &str| {
        profile
            .pointer(pointer)
            .and_then(Value::as_str)
            .unwrap()
            .to_owned()
    };
    let (birth, name, family) = (
        text("/birth/event_id"),
        text("/primary_name/name_id"),
        text("/families_as_spouse/0/family_id"),
    );
    let note = ok(
        &f.local,
        Method::POST,
        &f.uri("/notes"),
        Some(json!({ "text": "Fictitious remark", "person_id": excluded })),
    )
    .await;
    let note = note["id"].as_str().unwrap().to_owned();
    let child = ok(
        &f.local,
        Method::POST,
        &f.uri("/persons"),
        Some(json!({ "sex": "unknown" })),
    )
    .await;
    let child = child["id"].as_str().unwrap().to_owned();

    let refused = [
        (
            Method::PUT,
            format!("/persons/{excluded}"),
            Some(json!({ "sex": "male" })),
            StatusCode::NOT_FOUND,
        ),
        (
            Method::DELETE,
            format!("/persons/{excluded}"),
            None,
            StatusCode::NOT_FOUND,
        ),
        (
            Method::POST,
            "/events".to_string(),
            Some(json!({ "event_type": "occupation", "person_id": excluded })),
            StatusCode::NOT_FOUND,
        ),
        (
            Method::PUT,
            format!("/events/{birth}"),
            Some(json!({})),
            StatusCode::NOT_FOUND,
        ),
        (
            Method::PUT,
            format!("/persons/{excluded}/names/{name}"),
            Some(json!({})),
            StatusCode::NOT_FOUND,
        ),
        (
            Method::PUT,
            format!("/notes/{note}"),
            Some(json!({})),
            StatusCode::NOT_FOUND,
        ),
        (
            Method::POST,
            format!("/persons/{}/merge", f.person("Corwin")),
            Some(json!({ "duplicate_id": excluded })),
            StatusCode::NOT_FOUND,
        ),
        (
            Method::PUT,
            format!("/families/{family}"),
            Some(json!({})),
            StatusCode::FORBIDDEN,
        ),
        (
            Method::POST,
            format!("/families/{family}/children"),
            Some(json!({ "person_id": child, "child_type": "biological", "sort_order": 1 })),
            StatusCode::FORBIDDEN,
        ),
    ];
    for (method, path, body, status) in refused {
        let answer = match body {
            Some(body) => editor.json(method.clone(), &f.uri(&path), body).await,
            None => editor.send(method.clone(), &f.uri(&path), None, None).await,
        };
        assert_eq!(answer.status, status, "{method} {path}: {}", answer.json);
        assert!(
            !answer
                .json
                .to_string()
                .contains(crate::common::canary::EXCLUDED),
            "{method} {path}: {}",
            answer.json
        );
    }
    for (query, variables, code) in [
        (
            "mutation($tree: ID!, $id: ID!) { updatePerson(treeId: $tree, id: $id, input: { sex: MALE }) { id } }",
            json!({ "tree": f.canaries.tree, "id": excluded }),
            "NOT_FOUND",
        ),
        (
            "mutation($tree: ID!, $id: ID!) { deletePerson(treeId: $tree, id: $id) }",
            json!({ "tree": f.canaries.tree, "id": excluded }),
            "NOT_FOUND",
        ),
        (
            "mutation($tree: ID!, $id: ID!) { updateEvent(treeId: $tree, id: $id, input: {}) { id } }",
            json!({ "tree": f.canaries.tree, "id": birth }),
            "NOT_FOUND",
        ),
        (
            "mutation($tree: ID!, $family: ID!, $child: String!) { addChild(treeId: $tree, familyId: $family, input: { personId: $child, childType: BIOLOGICAL }) { personId } }",
            json!({ "tree": f.canaries.tree, "family": family, "child": child }),
            "FORBIDDEN",
        ),
    ] {
        let answer = editor.gql(query, variables).await;
        assert_eq!(
            answer.json["errors"][0]["extensions"]["code"], code,
            "{query}: {}",
            answer.json
        );
    }

    // Nothing was written; the Editor's other writes go through.
    let person = ok(
        &f.local,
        Method::GET,
        &f.uri(&format!("/persons/{excluded}")),
        None,
    )
    .await;
    assert_eq!(person["sex"], "female", "{person}");
    let profiles = crate::common::all_profiles(&f.local, &f.canaries.tree).await;
    assert!(
        profiles
            .iter()
            .all(|p| p["person_id"] != child.as_str() || p["family_as_child"].is_null()),
        "the child was linked"
    );
    let allowed = editor
        .json(
            Method::PUT,
            &f.uri(&format!("/persons/{}", f.person("Corwin"))),
            json!({ "sex": "male" }),
        )
        .await;
    assert_eq!(allowed.status, StatusCode::OK, "{}", allowed.json);
}

/// The persons a scoped Editor's body names are checked whatever JSON
/// media type it is sent as: the JSON extractor accepts any spelling of
/// `application/json` and any `+json` type, and a body without a type is
/// read by the routes that take raw bytes (`PUT …/families/{id}`).
#[tokio::test]
async fn a_scoped_editors_body_is_checked_whatever_its_media_type() {
    let mut f = Fixture::new(&[]).await;
    let (mut editor, editor_user) = f.user("editorone").await;
    f.owner
        .json(
            Method::POST,
            &f.uri("/members"),
            json!({ "email": editor_user.email(), "role": "editor", "scope": { "kind": "descendants_of", "person_id": f.person("Harlan") } }),
        )
        .await;
    let body = json!({ "event_type": "occupation", "person_id": f.person("Ashby") }).to_string();
    for content_type in [
        Some("Application/JSON"),
        Some("application/vnd.test+json"),
        Some("application/merge-patch+json; charset=utf-8"),
        Some("text/plain"),
        None,
    ] {
        let answer = editor
            .send(
                Method::POST,
                &f.uri("/events"),
                Some(axum::body::Body::from(body.clone())),
                content_type,
            )
            .await;
        assert!(
            !answer.status.is_success(),
            "{content_type:?}: {} {}",
            answer.status,
            answer.json
        );
        if content_type.is_some_and(|t| t.contains("json") || t.contains("JSON")) {
            assert_eq!(answer.status, StatusCode::NOT_FOUND, "{content_type:?}");
        }
    }
    // A malformed body is refused before it reaches the handler.
    let malformed = editor
        .send(
            Method::POST,
            &f.uri("/events"),
            Some(axum::body::Body::from("{ not json")),
            Some("application/vnd.test+json"),
        )
        .await;
    assert_eq!(
        malformed.status,
        StatusCode::BAD_REQUEST,
        "{}",
        malformed.json
    );
    let events = ok(
        &f.local,
        Method::GET,
        &f.uri(&format!("/events?person_id={}", f.person("Ashby"))),
        None,
    )
    .await;
    assert!(
        events["edges"]
            .as_array()
            .unwrap()
            .iter()
            .all(|edge| edge["node"]["event_type"] != "occupation"),
        "{events}"
    );
}

/// What keeping scopes current costs on a generated tree of 10 000
/// persons, printed: building a scope (one read of the persons, one of the
/// links, the walk, the rows), rebuilding it once a link moved, the
/// whole-tree preview, and writing and reading 10 000 materialized rows.
/// Timing wants an optimised build: `just bench` runs it in release mode.
#[tokio::test]
#[ignore = "benchmark: run by `just bench` in release mode"]
async fn scope_cost_on_ten_thousand_persons() {
    use std::time::Instant;

    use oxidgene_api::service::access_scope;
    use oxidgene_core::sharing::{ScopeDefinition, ScopeKind};

    let db = setup_db().await;
    let app = app_on(db.clone());
    let (tree, anchor) = crate::common::family_blocks_tree(&app, &db, 1_000).await;
    let tree_id: Uuid = tree.parse().unwrap();
    let live = oxidgene_db::repo::PersonRepo::live_ids(&db, tree_id)
        .await
        .unwrap();
    assert_eq!(live.len(), 10_000);
    let definition = ScopeDefinition {
        kind: ScopeKind::AncestorsAndDescendantsOf,
        person_id: anchor.parse().unwrap(),
        depth: None,
        include_self: true,
        include_spouses: true,
        collateral: true,
    };

    let started = Instant::now();
    let scope = access_scope::create(&db, tree_id, &definition)
        .await
        .unwrap();
    let built = started.elapsed();

    // A link moves the tree's ancestry generation: the next read rebuilds.
    let child = crate::common::new_person(&app, &tree).await;
    let family = ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/families"),
        Some(json!({})),
    )
    .await;
    let family = family["id"].as_str().unwrap();
    for (path, body) in [
        (
            format!("/api/v1/trees/{tree}/families/{family}/spouses"),
            json!({ "person_id": anchor, "role": "husband", "sort_order": 0 }),
        ),
        (
            format!("/api/v1/trees/{tree}/families/{family}/children"),
            json!({ "person_id": child, "child_type": "biological", "sort_order": 0 }),
        ),
    ] {
        ok(&app, Method::POST, &path, Some(body)).await;
    }
    let started = Instant::now();
    let persons = access_scope::scope_persons(&db, scope.id).await.unwrap();
    let rebuilt = started.elapsed();
    assert!(persons.contains(&child.parse().unwrap()));
    let started = Instant::now();
    access_scope::scope_persons(&db, scope.id).await.unwrap();
    let current = started.elapsed();

    let profiles = std::sync::Arc::new(oxidgene_api::profile::ProfileService::new(db.clone()));
    let started = Instant::now();
    let preview = access_scope::preview(&db, &profiles, tree_id, &Default::default())
        .await
        .unwrap();
    let previewed = started.elapsed();
    assert_eq!(preview.person_count, 10_001);

    // The rows of a scope reaching the whole tree.
    let all: std::collections::HashSet<Uuid> =
        oxidgene_db::repo::PersonRepo::live_ids(&db, tree_id)
            .await
            .unwrap();
    let (generation, _) = AccessScopeRepo::generations(&db, scope.id)
        .await
        .unwrap()
        .unwrap();
    let started = Instant::now();
    AccessScopeRepo::replace_persons(&db, scope.id, &all, generation)
        .await
        .unwrap();
    let written = started.elapsed();
    let started = Instant::now();
    assert_eq!(
        AccessScopeRepo::persons(&db, scope.id).await.unwrap().len(),
        10_001
    );
    let read = started.elapsed();

    eprintln!(
        "scope on 10 000 persons: built {built:?} ({} persons), rebuilt after a link {rebuilt:?}, \
         read while current {current:?}, whole-tree preview {previewed:?}, \
         10 001 rows written {written:?}, read {read:?}",
        persons.len()
    );
}
