//! Notifications and email (docs/messaging.md §2, §3, §6, §14): the
//! notifications sharing writes, their reading and marking on REST and
//! GraphQL, the long poll, the preferences, the outbox the invitations and
//! notifications fill, the sender's retries against a fake relay and a fake
//! SMTP server, the daily digest, the one-click unsubscribe, erasure, and
//! logs without addresses — through the Backend-for-Frontend's sessions of
//! fictitious users.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::http::{Method, StatusCode};
use oxidgene_api::mail::sender::Outcome;
use oxidgene_api::mail::{
    MailContext, MailSender, Outgoing, Relay, RelayFailure, SmtpConfig, SmtpRelay, SmtpTls,
    UnsubscribeKeys,
};
use oxidgene_core::access::TreeRole;
use oxidgene_db::entities::{mail_outbox, notification, notification_preference};
use oxidgene_db::repo::{AppUserRepo, MailOutboxRepo, MembershipRepo};
use oxidgene_db::sea_orm::sea_query::Expr;
use oxidgene_db::sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::common::capture::global_capture;
use crate::common::oidc::{Browser, MockProvider, MockUser, PUBLIC_URL};
use crate::common::{app_on, new_tree, setup_db};

/// The session keys of `MockProvider::settings`.
const SESSION_KEY: [u8; 32] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
    26, 27, 28, 29, 30, 31,
];

/// A tree owned by a signed-in owner, on an `oidc` application that mails
/// or not.
struct Fixture {
    provider: MockProvider,
    app: Router,
    db: DatabaseConnection,
    owner: Browser,
    owner_id: Uuid,
    tree: String,
}

impl Fixture {
    async fn new(mailing: bool) -> Self {
        let provider = MockProvider::start().await;
        let db = setup_db().await;
        let (app, _) = provider.app_mailing(&db, &[], mailing);
        let tree = new_tree(&app_on(db.clone()), "Fictive Lindqvar tree").await;
        let mut owner = Browser::new(&app);
        let owner_user = MockUser::new("owner");
        owner.sign_in(&provider, &owner_user).await;
        let owner_id = user_id(&db, &provider, &owner_user).await;
        MembershipRepo::create(&db, tree.parse().unwrap(), owner_id, TreeRole::Owner, None)
            .await
            .unwrap();
        Self {
            provider,
            app,
            db,
            owner,
            owner_id,
            tree,
        }
    }

    async fn user(&self, name: &str) -> (Browser, MockUser, Uuid) {
        let mut browser = Browser::new(&self.app);
        let user = MockUser::new(name);
        browser.sign_in(&self.provider, &user).await;
        let id = user_id(&self.db, &self.provider, &user).await;
        (browser, user, id)
    }

    /// Share the tree with `username` as a guest.
    async fn share(&mut self, username: &str) {
        let answer = self
            .owner
            .json(
                Method::POST,
                &format!("/api/v1/trees/{}/members", self.tree),
                json!({ "username": username, "role": "guest" }),
            )
            .await;
        assert_eq!(answer.status, StatusCode::CREATED, "{}", answer.json);
    }

    fn context(&self) -> MailContext {
        MailContext {
            public_url: PUBLIC_URL.to_string(),
            unsubscribe: UnsubscribeKeys::from_session_keys(&[SESSION_KEY]),
        }
    }
}

async fn user_id(db: &DatabaseConnection, provider: &MockProvider, user: &MockUser) -> Uuid {
    AppUserRepo::find_by_identity(db, provider.issuer(), &user.subject)
        .await
        .unwrap()
        .expect("provisioned")
        .id
}

/// Another handle on `browser`'s session, for a request held open.
fn same_session(browser: &Browser) -> Browser {
    Browser {
        app: browser.app.clone(),
        cookies: browser.cookies.clone(),
        csrf: browser.csrf.clone(),
        origin: browser.origin.clone(),
    }
}

/// The outbox's rows, as `(kind, status, to_address, attempts)`.
async fn outbox(db: &DatabaseConnection) -> Vec<(String, String, String, i32)> {
    mail_outbox::Entity::find()
        .order_by_asc(mail_outbox::Column::Id)
        .all(db)
        .await
        .unwrap()
        .into_iter()
        .map(|row| (row.kind, row.status, row.to_address, row.attempts))
        .collect()
}

/// Make every pending row due now.
async fn due_now(db: &DatabaseConnection) {
    mail_outbox::Entity::update_many()
        .col_expr(
            mail_outbox::Column::NextAttemptAt,
            Expr::value(chrono::Utc::now() - chrono::Duration::seconds(1)),
        )
        .filter(mail_outbox::Column::Status.eq("pending"))
        .exec(db)
        .await
        .unwrap();
}

/// A relay that keeps what it is given, failing as told.
#[derive(Default)]
struct FakeRelay {
    sent: Mutex<Vec<Outgoing>>,
    failure: Mutex<Option<RelayFailure>>,
}

#[async_trait::async_trait]
impl Relay for FakeRelay {
    async fn send(&self, mail: &Outgoing) -> Result<(), RelayFailure> {
        if let Some(failure) = *self.failure.lock().unwrap() {
            return Err(failure);
        }
        self.sent.lock().unwrap().push(mail.clone());
        Ok(())
    }
}

fn sender(f: &Fixture, relay: &Arc<FakeRelay>) -> MailSender {
    MailSender::new(
        f.db.clone(),
        Arc::clone(relay) as Arc<dyn Relay>,
        f.context(),
    )
}

#[tokio::test]
async fn sharing_notifies_the_member_on_both_surfaces() {
    let mut f = Fixture::new(false).await;
    let (mut guest, _, _) = f.user("guestnine").await;
    f.share("guestnine").await;

    let list = guest.get("/api/v1/me/notifications").await;
    assert_eq!(list.status, StatusCode::OK, "{}", list.json);
    assert_eq!(list.json["unread_count"], 1);
    let node = &list.json["edges"][0]["node"];
    assert_eq!(node["kind"], "tree_shared");
    assert_eq!(node["tree_name"], "Fictive Lindqvar tree");
    assert_eq!(node["actor_name"], "Fictive owner");
    assert!(node["read_at"].is_null());

    let gql = guest
        .gql(
            "{ notifications { unreadCount totalCount edges { node { kind treeName actorName treeUnavailable } } } }",
            json!({}),
        )
        .await;
    let page = &gql.json["data"]["notifications"];
    assert_eq!(page["unreadCount"], 1, "{}", gql.json);
    assert_eq!(page["edges"][0]["node"]["kind"], "TREE_SHARED");
    assert_eq!(
        page["edges"][0]["node"]["treeName"],
        "Fictive Lindqvar tree"
    );

    // Read on REST, then on GraphQL once something new arrived.
    let read = guest
        .json(Method::POST, "/api/v1/me/notifications/read", json!({}))
        .await;
    assert_eq!(read.json["unread_count"], 0, "{}", read.json);
    let removed = f
        .owner
        .send(
            Method::DELETE,
            &format!(
                "/api/v1/trees/{}/members/{}",
                f.tree,
                user_id_of(&guest).await
            ),
            None,
            None,
        )
        .await;
    assert_eq!(removed.status, StatusCode::NO_CONTENT);
    let list = guest.get("/api/v1/me/notifications").await;
    let ended = &list.json["edges"][0]["node"];
    assert_eq!(ended["kind"], "membership_ended");
    // A tree the reader no longer sees is named by nobody.
    assert!(ended["tree_name"].is_null());
    assert_eq!(ended["tree_unavailable"], true);
    let marked = guest
        .gql("mutation { markNotificationsRead }", json!({}))
        .await;
    assert_eq!(
        marked.json["data"]["markNotificationsRead"], 0,
        "{}",
        marked.json
    );

    // Without a relay, nothing was queued.
    assert!(outbox(&f.db).await.is_empty());
}

/// The notifications page by cursor and clamp `first` to 1–100 on both
/// surfaces, as every collection does (the pagination guard's application
/// has no accounts).
#[tokio::test]
async fn notifications_page_by_cursor_and_clamp_first() {
    let mut f = Fixture::new(false).await;
    let (mut member, _, member_id) = f.user("pagenine").await;
    f.share("pagenine").await;
    let updated = f
        .owner
        .json(
            Method::PUT,
            &format!("/api/v1/trees/{}/members/{member_id}", f.tree),
            json!({ "role": "editor" }),
        )
        .await;
    assert_eq!(updated.status, StatusCode::OK);
    let first = member.get("/api/v1/me/notifications?first=1").await;
    assert_eq!(first.json["edges"].as_array().unwrap().len(), 1);
    assert_eq!(first.json["page_info"]["has_next_page"], true);
    assert_eq!(first.json["total_count"], 2);
    assert_eq!(first.json["edges"][0]["node"]["kind"], "membership_changed");
    let cursor = first.json["page_info"]["end_cursor"].as_str().unwrap();
    let next = member
        .get(&format!("/api/v1/me/notifications?first=1&after={cursor}"))
        .await;
    assert_eq!(next.json["edges"][0]["node"]["kind"], "tree_shared");
    assert_eq!(next.json["page_info"]["has_next_page"], false);
    for first in [0, 101] {
        let page = member
            .get(&format!("/api/v1/me/notifications?first={first}"))
            .await;
        let rows = page.json["edges"].as_array().unwrap().len();
        assert!((1..=100).contains(&rows), "first={first}: {rows}");
        let gql = member
            .gql(
                &format!("{{ notifications(first: {first}) {{ edges {{ cursor }} }} }}"),
                json!({}),
            )
            .await;
        let rows = gql.json["data"]["notifications"]["edges"]
            .as_array()
            .unwrap()
            .len();
        assert!((1..=100).contains(&rows), "GraphQL first={first}: {rows}");
    }
}

async fn user_id_of(browser: &Browser) -> String {
    let mut browser = same_session(browser);
    browser.get("/api/v1/me").await.json["id"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn members_leaving_and_ownership_notify_the_owner_and_the_new_owner() {
    let mut f = Fixture::new(false).await;
    let (mut guest, _, guest_id) = f.user("guestten").await;
    let (mut heir, _, heir_id) = f.user("heirten").await;
    f.share("guestten").await;
    f.share("heirten").await;

    let left = guest
        .send(
            Method::DELETE,
            &format!("/api/v1/trees/{}/members/{guest_id}", f.tree),
            None,
            None,
        )
        .await;
    assert_eq!(left.status, StatusCode::NO_CONTENT);
    let owner = f.owner.get("/api/v1/me/notifications").await;
    assert_eq!(owner.json["edges"][0]["node"]["kind"], "member_left");
    assert_eq!(
        owner.json["edges"][0]["node"]["actor_name"],
        "Fictive guestten"
    );

    let transferred = f
        .owner
        .gql(
            "mutation($tree: ID!, $user: ID!) { transferTreeOwnership(treeId: $tree, userId: $user) { role } }",
            json!({ "tree": f.tree, "user": heir_id }),
        )
        .await;
    assert_eq!(
        transferred.json["data"]["transferTreeOwnership"]["role"], "owner",
        "{}",
        transferred.json
    );
    for (browser, subject_name) in [
        (&mut heir, json!("Fictive heirten")),
        (&mut f.owner, json!("Fictive heirten")),
    ] {
        let list = browser.get("/api/v1/me/notifications").await;
        let node = &list.json["edges"][0]["node"];
        assert_eq!(node["kind"], "ownership_transferred", "{}", list.json);
        assert_eq!(node["subject_id"], json!(heir_id));
        assert_eq!(node["subject_name"], subject_name);
    }
    let _ = f.owner_id;
}

#[tokio::test]
async fn the_long_poll_answers_at_once_on_change_and_at_timeout() {
    let mut f = Fixture::new(false).await;
    let (mut guest, _, _) = f.user("guesteleven").await;

    // Without a cursor: the current one, at once.
    let first = guest.get("/api/v1/me/updates").await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.json);
    assert_eq!(first.json["events"], json!([]));
    let cursor = first.json["cursor"].as_str().unwrap().to_string();

    // Nothing new: the wait runs out, with the same cursor.
    let started = Instant::now();
    let idle = guest
        .get(&format!("/api/v1/me/updates?after={cursor}&wait=1"))
        .await;
    assert!(started.elapsed() >= Duration::from_millis(900));
    assert_eq!(idle.json["events"], json!([]));
    assert_eq!(idle.json["cursor"], json!(cursor));

    // A share while it waits answers it at once.
    let mut waiting = same_session(&guest);
    let uri = format!("/api/v1/me/updates?after={cursor}&wait=20");
    let held = tokio::spawn(async move { waiting.get(&uri).await });
    tokio::time::sleep(Duration::from_millis(200)).await;
    let started = Instant::now();
    f.share("guesteleven").await;
    let answer = tokio::time::timeout(Duration::from_secs(5), held)
        .await
        .expect("the change ends the wait")
        .unwrap();
    assert!(started.elapsed() < Duration::from_secs(5));
    let events = answer.json["events"].as_array().unwrap();
    assert_eq!(events.len(), 1, "{}", answer.json);
    assert_eq!(events[0]["type"], "notification");
    let next = answer.json["cursor"].as_str().unwrap().to_string();
    assert_ne!(next, cursor);

    // Events newer than the cursor answer at once; GraphQL alike.
    let started = Instant::now();
    let gql = guest
        .gql(
            "query($after: String) { updates(after: $after, wait: 20) { cursor events { type id } } }",
            json!({ "after": cursor }),
        )
        .await;
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(
        gql.json["data"]["updates"]["cursor"],
        json!(next),
        "{}",
        gql.json
    );
    assert_eq!(
        gql.json["data"]["updates"]["events"][0]["type"],
        "notification"
    );

    // An invalid cursor is a validation error.
    let invalid = guest.get("/api/v1/me/updates?after=not-a-cursor").await;
    assert_eq!(invalid.status, StatusCode::BAD_REQUEST);
}

/// Counts the SQL statements SeaORM runs, as `query_scaling_test` does.
struct StatementCounter(Arc<std::sync::atomic::AtomicUsize>);

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for StatementCounter {
    fn on_new_span(
        &self,
        attributes: &tracing::span::Attributes<'_>,
        _id: &tracing::span::Id,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let name = attributes.metadata().name();
        if name.starts_with("sea_orm.") && !matches!(name, "sea_orm.begin" | "sea_orm.commit") {
            self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

/// Waiting costs nothing: a long poll that runs out after two idle seconds
/// runs exactly the statements of one that answers at once — its session's
/// and one read of what is new — and none while it waits.
#[tokio::test]
async fn an_idle_long_poll_reads_nothing_while_it_waits() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tracing_subscriber::layer::SubscriberExt as _;
    use tracing_subscriber::util::SubscriberInitExt as _;

    let f = Fixture::new(false).await;
    let (mut guest, _, _) = f.user("idlenine").await;
    let cursor = guest.get("/api/v1/me/updates").await.json["cursor"]
        .as_str()
        .unwrap()
        .to_string();
    let counter = Arc::new(AtomicUsize::new(0));
    let _guard = tracing_subscriber::registry()
        .with(StatementCounter(Arc::clone(&counter)))
        .set_default();
    let at_once = guest
        .get(&format!("/api/v1/me/updates?after={cursor}&wait=0"))
        .await;
    assert_eq!(at_once.json["events"], json!([]));
    let immediate = counter.swap(0, Ordering::Relaxed);
    let started = Instant::now();
    guest
        .get(&format!("/api/v1/me/updates?after={cursor}&wait=2"))
        .await;
    assert!(started.elapsed() >= Duration::from_secs(2));
    let idle = counter.load(Ordering::Relaxed);
    assert!(immediate > 0, "no statement counted");
    assert_eq!(idle, immediate, "an idle long poll read while it waited");
    println!("an idle long poll: {idle} statements per request, none while it waits");
}

#[tokio::test]
async fn preferences_are_read_and_changed_on_both_surfaces() {
    let f = Fixture::new(false).await;
    let (mut user, _, _) = f.user("prefnine").await;
    let read = user.get("/api/v1/me/notification-preferences").await;
    assert_eq!(read.status, StatusCode::OK);
    // Without a relay the email choices are hidden.
    assert_eq!(read.json["email_available"], false);
    assert_eq!(read.json["email_enabled"], true);
    let kinds = read.json["kinds"].as_array().unwrap();
    assert_eq!(kinds.len(), 7);
    let email_of = |kinds: &Value, kind: &str| {
        kinds
            .as_array()
            .unwrap()
            .iter()
            .find(|k| k["kind"] == kind)
            .unwrap()["email"]
            .clone()
    };
    assert_eq!(email_of(&read.json["kinds"], "tree_shared"), "immediate");
    assert_eq!(
        email_of(&read.json["kinds"], "invitation_accepted"),
        "immediate"
    );
    assert_eq!(email_of(&read.json["kinds"], "job_finished"), "off");

    let changed = user
        .json(
            Method::PUT,
            "/api/v1/me/notification-preferences",
            json!({ "email_enabled": false, "kinds": [{ "kind": "job_finished", "email": "daily" }] }),
        )
        .await;
    assert_eq!(changed.status, StatusCode::OK, "{}", changed.json);
    assert_eq!(changed.json["email_enabled"], false);
    assert_eq!(email_of(&changed.json["kinds"], "job_finished"), "daily");

    let gql = user
        .gql(
            "mutation { updateNotificationPreferences(input: { emailEnabled: true, kinds: [{ kind: TREE_SHARED, email: OFF }] }) { emailEnabled kinds { kind email } } }",
            json!({}),
        )
        .await;
    let preferences = &gql.json["data"]["updateNotificationPreferences"];
    assert_eq!(preferences["emailEnabled"], true, "{}", gql.json);
    let read = user
        .gql(
            "{ notificationPreferences { emailAvailable kinds { kind email } } }",
            json!({}),
        )
        .await;
    let kinds = &read.json["data"]["notificationPreferences"]["kinds"];
    let shared = kinds
        .as_array()
        .unwrap()
        .iter()
        .find(|k| k["kind"] == "TREE_SHARED")
        .unwrap();
    assert_eq!(shared["email"], "OFF");

    // Unknown values are refused.
    let refused = user
        .json(
            Method::PUT,
            "/api/v1/me/notification-preferences",
            json!({ "kinds": [{ "kind": "job_finished", "email": "hourly" }] }),
        )
        .await;
    assert!(refused.status.is_client_error());

    // The account data holds what was changed.
    let data = user.get("/api/v1/me/export").await;
    let preferences = data.json["notification_preferences"].as_array().unwrap();
    assert!(
        preferences
            .iter()
            .any(|p| p["kind"] == "job_finished" && p["email"] == "daily")
    );
}

#[tokio::test]
async fn invitations_are_queued_with_their_transaction_and_sent_in_the_language_of_the_instance() {
    let mut f = Fixture::new(true).await;
    // The owner's members section no longer asks to send the link by hand.
    let shared = f
        .owner
        .json(
            Method::POST,
            &format!("/api/v1/trees/{}/members", f.tree),
            json!({ "email": "fictive.cousin@example.invalid", "role": "editor" }),
        )
        .await;
    assert_eq!(shared.status, StatusCode::ACCEPTED, "{}", shared.json);
    assert_eq!(shared.json["invitation"]["delivery"], "email");
    let invitation = shared.json["invitation"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let rows = outbox(&f.db).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, "invitation");
    assert_eq!(rows[0].2, "fictive.cousin@example.invalid");

    // A refused share queues nothing: the transaction carries the mail.
    let refused = f
        .owner
        .json(
            Method::POST,
            &format!("/api/v1/trees/{}/members", f.tree),
            json!({ "email": "fictive.cousin@example.invalid", "role": "editor" }),
        )
        .await;
    assert_eq!(refused.status, StatusCode::CONFLICT);
    assert_eq!(outbox(&f.db).await.len(), 1);

    let relay = Arc::new(FakeRelay::default());
    let sender = sender(&f, &relay);
    assert_eq!(sender.send_next().await.unwrap(), Some(Outcome::Sent));
    assert_eq!(sender.send_next().await.unwrap(), None);
    {
        let sent = relay.sent.lock().unwrap();
        let mail = &sent[0];
        assert_eq!(mail.to, "fictive.cousin@example.invalid");
        assert_eq!(
            mail.subject,
            "Fictive owner invites you to the family tree Fictive Lindqvar tree"
        );
        assert!(mail.text.contains("as an editor"));
        assert!(mail.text.contains(&format!("{PUBLIC_URL}/invitations")));
        assert!(mail.html.contains("Fictive Lindqvar tree"));
        // An address without an account keeps no preference to change.
        assert!(mail.unsubscribe.is_none());
    }

    // Resending goes through the outbox again, on GraphQL too.
    let resent = f
        .owner
        .gql(
            "mutation($tree: ID!, $id: ID!) { resendTreeInvitation(treeId: $tree, invitationId: $id) { delivery } }",
            json!({ "tree": f.tree, "id": invitation }),
        )
        .await;
    assert_eq!(
        resent.json["data"]["resendTreeInvitation"]["delivery"], "EMAIL",
        "{}",
        resent.json
    );
    assert_eq!(outbox(&f.db).await.len(), 2);
    // Revoked before it left, it is not sent.
    let revoked = f
        .owner
        .send(
            Method::DELETE,
            &format!("/api/v1/trees/{}/invitations/{invitation}", f.tree),
            None,
            None,
        )
        .await;
    assert_eq!(revoked.status, StatusCode::NO_CONTENT);
    assert_eq!(sender.send_next().await.unwrap(), Some(Outcome::Dropped));
    assert_eq!(relay.sent.lock().unwrap().len(), 1);
    let rows = outbox(&f.db).await;
    assert_eq!(rows[1].1, "dropped");
}

#[tokio::test]
async fn resending_an_invitation_mail_counts_against_the_hourly_lookups() {
    let mut f = Fixture::new(true).await;
    // The invitation by address is the first lookup of the hour.
    let shared = f
        .owner
        .json(
            Method::POST,
            &format!("/api/v1/trees/{}/members", f.tree),
            json!({ "email": "fictive.resent@example.invalid", "role": "guest" }),
        )
        .await;
    assert_eq!(shared.status, StatusCode::ACCEPTED, "{}", shared.json);
    let invitation = shared.json["invitation"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let resend = format!("/api/v1/trees/{}/invitations/{invitation}/resend", f.tree);
    let query = "mutation($tree: ID!, $id: ID!) { resendTreeInvitation(treeId: $tree, invitationId: $id) { delivery } }";
    let variables = json!({ "tree": f.tree, "id": invitation });
    for _ in 1..oxidgene_api::service::sharing::LOOKUPS_PER_HOUR - 1 {
        let resent = f.owner.send(Method::POST, &resend, None, None).await;
        assert_eq!(resent.status, StatusCode::OK, "{}", resent.json);
    }
    let resent = f.owner.gql(query, variables.clone()).await;
    assert_eq!(
        resent.json["data"]["resendTreeInvitation"]["delivery"], "EMAIL",
        "{}",
        resent.json
    );
    // The hour's budget is spent, on both surfaces, and no mail is queued.
    let queued = outbox(&f.db).await.len();
    assert_eq!(queued, oxidgene_api::service::sharing::LOOKUPS_PER_HOUR);
    let refused = f.owner.send(Method::POST, &resend, None, None).await;
    assert_eq!(
        refused.status,
        StatusCode::TOO_MANY_REQUESTS,
        "{}",
        refused.json
    );
    let refused = f.owner.gql(query, variables).await;
    assert_eq!(
        refused.json["errors"][0]["extensions"]["code"], "RATE_LIMITED",
        "{}",
        refused.json
    );
    assert_eq!(outbox(&f.db).await.len(), queued);
}

#[tokio::test]
async fn a_failing_relay_is_retried_then_given_up_and_a_rejection_at_once() {
    let mut f = Fixture::new(true).await;
    let (_, _, _) = f.user("relaynine").await;
    f.share("relaynine").await;
    let relay = Arc::new(FakeRelay::default());
    *relay.failure.lock().unwrap() = Some(RelayFailure {
        category: "connection",
        permanent: false,
    });
    let sender = sender(&f, &relay);
    for attempt in 1..=5 {
        assert_eq!(
            sender.send_next().await.unwrap(),
            Some(Outcome::Retried),
            "attempt {attempt}"
        );
        // Not due again before its delay.
        assert_eq!(sender.send_next().await.unwrap(), None);
        due_now(&f.db).await;
    }
    assert_eq!(sender.send_next().await.unwrap(), Some(Outcome::Failed));
    let rows = outbox(&f.db).await;
    assert_eq!((rows[0].1.as_str(), rows[0].3), ("failed", 6));

    // A permanent refusal fails at once.
    let (_, _, _) = f.user("relayten").await;
    f.share("relayten").await;
    *relay.failure.lock().unwrap() = Some(RelayFailure {
        category: "rejected",
        permanent: true,
    });
    assert_eq!(sender.send_next().await.unwrap(), Some(Outcome::Failed));
}

#[tokio::test]
async fn a_shared_tree_is_mailed_at_once_and_unsubscribed_in_one_click() {
    let capture = global_capture();
    let mut f = Fixture::new(true).await;
    let (mut member, member_user, member_id) = f.user("mailnine").await;
    f.share("mailnine").await;
    let relay = Arc::new(FakeRelay::default());
    let sender = sender(&f, &relay);
    assert_eq!(sender.send_next().await.unwrap(), Some(Outcome::Sent));
    let mail = relay.sent.lock().unwrap()[0].clone();
    assert_eq!(mail.to, member_user.email());
    // The account's locale is French.
    assert_eq!(
        mail.subject,
        "Fictive owner a partagé l’arbre généalogique Fictive Lindqvar tree avec vous"
    );
    assert!(
        mail.text
            .contains(&format!("{PUBLIC_URL}/trees/{}", f.tree))
    );
    let one_click = mail
        .unsubscribe
        .clone()
        .expect("a notification can be unsubscribed");
    assert!(one_click.starts_with(&format!("{PUBLIC_URL}/api/v1/mail/unsubscribe?token=")));
    assert!(
        mail.text
            .contains(&format!("{PUBLIC_URL}/unsubscribe?token="))
    );

    // The one-click POST, without a session, as a mail client sends it.
    let token = one_click.split("token=").nth(1).unwrap().to_string();
    let mut client = Browser::new(&f.app);
    client.origin = None;
    let answer = client
        .send(
            Method::POST,
            &format!("/api/v1/mail/unsubscribe?token={token}"),
            Some("List-Unsubscribe=One-Click".into()),
            Some("application/x-www-form-urlencoded"),
        )
        .await;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.json);
    assert_eq!(answer.json["kind"], "tree_shared");
    let preferences = member.get("/api/v1/me/notification-preferences").await;
    assert_eq!(preferences.json["email_available"], true);
    let shared = preferences.json["kinds"]
        .as_array()
        .unwrap()
        .iter()
        .find(|k| k["kind"] == "tree_shared")
        .unwrap()
        .clone();
    assert_eq!(shared["email"], "off");

    // GraphQL takes the same token; a forged one is refused on both.
    let gql = client
        .gql(
            "mutation($token: String!) { unsubscribe(token: $token) }",
            json!({ "token": token }),
        )
        .await;
    assert_eq!(
        gql.json["data"]["unsubscribe"], "tree_shared",
        "{}",
        gql.json
    );
    let forged = client
        .send(
            Method::POST,
            "/api/v1/mail/unsubscribe?token=AAAA.BBBB",
            None,
            None,
        )
        .await;
    assert_eq!(forged.status, StatusCode::BAD_REQUEST);
    let forged = client
        .gql("mutation { unsubscribe(token: \"AAAA.BBBB\") }", json!({}))
        .await;
    assert!(forged.json["errors"].is_array());

    // Logs carry the kind and the outcome, never the address.
    for field in capture.take() {
        assert!(
            !field.value.contains(&member_user.email()),
            "{}.{} carries an address",
            field.owner,
            field.field
        );
    }
    let _ = member_id;
}

#[tokio::test]
async fn a_daily_digest_gathers_the_unread_notifications_of_the_day() {
    let mut f = Fixture::new(true).await;
    let (mut member, _, _) = f.user("digestnine").await;
    let changed = member
        .json(
            Method::PUT,
            "/api/v1/me/notification-preferences",
            json!({ "kinds": [
                { "kind": "tree_shared", "email": "daily" },
                { "kind": "membership_changed", "email": "daily" }
            ] }),
        )
        .await;
    assert_eq!(changed.status, StatusCode::OK);
    f.share("digestnine").await;
    let member_id = user_id_of(&member).await;
    let updated = f
        .owner
        .json(
            Method::PUT,
            &format!("/api/v1/trees/{}/members/{member_id}", f.tree),
            json!({ "role": "editor" }),
        )
        .await;
    assert_eq!(updated.status, StatusCode::OK, "{}", updated.json);
    // One digest row for the day, not yet due.
    let rows = outbox(&f.db).await;
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].0, "digest");
    let relay = Arc::new(FakeRelay::default());
    let sender = sender(&f, &relay);
    assert_eq!(sender.send_next().await.unwrap(), None);
    due_now(&f.db).await;
    assert_eq!(sender.send_next().await.unwrap(), Some(Outcome::Sent));
    let mail = relay.sent.lock().unwrap()[0].clone();
    assert_eq!(mail.subject, "Vos notifications OxidGene du jour");
    assert!(
        mail.text
            .contains("a partagé l’arbre généalogique Fictive Lindqvar tree")
    );
    assert!(mail.text.contains("a modifié votre rôle"));
    // Its unsubscribe turns the daily kinds off.
    let token = mail
        .unsubscribe
        .unwrap()
        .split("token=")
        .nth(1)
        .unwrap()
        .to_string();
    let mut client = Browser::new(&f.app);
    let answer = client
        .send(
            Method::POST,
            &format!("/api/v1/mail/unsubscribe?token={token}"),
            None,
            None,
        )
        .await;
    assert_eq!(answer.json["kind"], "digest");
    let preferences = member.get("/api/v1/me/notification-preferences").await;
    for kind in preferences.json["kinds"].as_array().unwrap() {
        assert_ne!(kind["email"], "daily", "{kind}");
    }

    // A digest whose notifications were read meanwhile is not sent.
    member
        .json(
            Method::PUT,
            "/api/v1/me/notification-preferences",
            json!({ "kinds": [{ "kind": "membership_changed", "email": "daily" }] }),
        )
        .await;
    f.owner
        .json(
            Method::PUT,
            &format!("/api/v1/trees/{}/members/{member_id}", f.tree),
            json!({ "role": "guest" }),
        )
        .await;
    member
        .json(Method::POST, "/api/v1/me/notifications/read", json!({}))
        .await;
    due_now(&f.db).await;
    assert_eq!(sender.send_next().await.unwrap(), Some(Outcome::Dropped));
}

#[tokio::test]
async fn erasure_deletes_notifications_preferences_and_queued_mail() {
    let mut f = Fixture::new(true).await;
    let (mut member, _, member_id) = f.user("erasenine").await;
    member
        .json(
            Method::PUT,
            "/api/v1/me/notification-preferences",
            json!({ "kinds": [{ "kind": "job_finished", "email": "daily" }] }),
        )
        .await;
    f.share("erasenine").await;
    assert_eq!(outbox(&f.db).await.len(), 1);
    let left = member
        .send(
            Method::DELETE,
            &format!("/api/v1/trees/{}/members/{member_id}", f.tree),
            None,
            None,
        )
        .await;
    assert_eq!(left.status, StatusCode::NO_CONTENT);
    let erased = member.send(Method::DELETE, "/api/v1/me", None, None).await;
    assert_eq!(erased.status, StatusCode::NO_CONTENT, "{}", erased.json);
    let notifications = notification::Entity::find()
        .filter(notification::Column::UserId.eq(member_id))
        .count(&f.db)
        .await
        .unwrap();
    let preferences = notification_preference::Entity::find()
        .filter(notification_preference::Column::UserId.eq(member_id))
        .count(&f.db)
        .await
        .unwrap();
    let mail = mail_outbox::Entity::find()
        .filter(mail_outbox::Column::UserId.eq(member_id))
        .count(&f.db)
        .await
        .unwrap();
    assert_eq!((notifications, preferences, mail), (0, 0, 0));
    assert_eq!(MailOutboxRepo::next_due(&f.db).await.unwrap(), None);
}

#[tokio::test]
async fn the_none_mode_has_no_notifications() {
    let db = setup_db().await;
    let app = app_on(db);
    let mut browser = Browser::new(&app);
    for uri in [
        "/api/v1/me/notifications",
        "/api/v1/me/notification-preferences",
        "/api/v1/me/updates",
    ] {
        assert_eq!(
            browser.get(uri).await.status,
            StatusCode::NOT_FOUND,
            "{uri}"
        );
    }
    let unsubscribe = browser
        .send(
            Method::POST,
            "/api/v1/mail/unsubscribe?token=a.b",
            None,
            None,
        )
        .await;
    assert_eq!(unsubscribe.status, StatusCode::NOT_FOUND);
}

/// A fake SMTP server on the loopback interface: it answers every command,
/// keeps the message it is given, and refuses the recipient when told.
async fn fake_smtp(refuse: Option<&'static str>) -> (u16, Arc<Mutex<Vec<String>>>) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let received = Arc::new(Mutex::new(Vec::new()));
    let kept = Arc::clone(&received);
    tokio::spawn(async move {
        loop {
            let Ok((socket, _)) = listener.accept().await else {
                return;
            };
            let kept = Arc::clone(&kept);
            tokio::spawn(async move {
                let (read, mut write) = socket.into_split();
                let mut lines = BufReader::new(read).lines();
                write
                    .write_all(b"220 fake.example.invalid ESMTP\r\n")
                    .await
                    .unwrap();
                let mut data = None::<String>;
                while let Ok(Some(line)) = lines.next_line().await {
                    if let Some(message) = &mut data {
                        if line == "." {
                            kept.lock().unwrap().push(std::mem::take(message));
                            data = None;
                            write.write_all(b"250 queued\r\n").await.unwrap();
                        } else {
                            message.push_str(&line);
                            message.push('\n');
                        }
                        continue;
                    }
                    let command = line.to_ascii_uppercase();
                    let reply: &[u8] = if command.starts_with("EHLO") {
                        b"250-fake.example.invalid\r\n250 8BITMIME\r\n"
                    } else if command.starts_with("RCPT") && refuse.is_some() {
                        refuse.unwrap().as_bytes()
                    } else if command.starts_with("DATA") {
                        data = Some(String::new());
                        b"354 go on\r\n"
                    } else if command.starts_with("QUIT") {
                        let _ = write.write_all(b"221 bye\r\n").await;
                        return;
                    } else {
                        b"250 ok\r\n"
                    };
                    write.write_all(reply).await.unwrap();
                }
            });
        }
    });
    (port, received)
}

fn loopback_relay(port: u16) -> SmtpRelay {
    SmtpRelay::new(
        &SmtpConfig {
            host: "127.0.0.1".into(),
            port,
            tls: SmtpTls::None,
            credentials: None,
            from: "OxidGene <no-reply@genealogy.example.invalid>".into(),
        },
        "genealogy.example.invalid",
    )
    .unwrap()
}

fn fictive_mail() -> Outgoing {
    Outgoing {
        to: "fictive.reader@example.invalid".into(),
        subject: "Fictive subject".into(),
        text: "Fictive body".into(),
        html: "<p>Fictive body</p>".into(),
        unsubscribe: Some(format!("{PUBLIC_URL}/api/v1/mail/unsubscribe?token=t")),
    }
}

#[tokio::test]
async fn the_smtp_relay_hands_the_mail_over_and_reports_refusals_by_class() {
    let (port, received) = fake_smtp(None).await;
    loopback_relay(port).send(&fictive_mail()).await.unwrap();
    let message = received.lock().unwrap()[0].clone();
    assert!(message.contains("Subject: Fictive subject"));
    assert!(message.contains("List-Unsubscribe-Post: List-Unsubscribe=One-Click"));
    assert!(message.contains("From: OxidGene <no-reply@genealogy.example.invalid>"));

    let (port, _) = fake_smtp(Some("550 no such mailbox\r\n")).await;
    let failure = loopback_relay(port)
        .send(&fictive_mail())
        .await
        .unwrap_err();
    assert_eq!(
        failure,
        RelayFailure {
            category: "rejected",
            permanent: true
        }
    );

    let (port, _) = fake_smtp(Some("451 try later\r\n")).await;
    let failure = loopback_relay(port)
        .send(&fictive_mail())
        .await
        .unwrap_err();
    assert_eq!(
        failure,
        RelayFailure {
            category: "deferred",
            permanent: false
        }
    );

    // Nobody listening: a connection failure, tried again later.
    let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = closed.local_addr().unwrap().port();
    drop(closed);
    let failure = loopback_relay(port)
        .send(&fictive_mail())
        .await
        .unwrap_err();
    assert!(!failure.permanent, "{failure:?}");
}
