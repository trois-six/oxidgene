//! The redactions the tree cache keeps for restricted viewers
//! (docs/architecture.md §4.1, docs/privacy.md §11 "Kept redactions"): a
//! viewer's redaction is kept between requests, and the very next request
//! after any of its inputs changes answers as if nothing had been kept,
//! over REST and GraphQL alike.
//!
//! The viewers read through one application, a replica of a deployment
//! whose tree cache the tests inspect; the owner writes through another
//! application over the same database, the way another replica would, so
//! only the database can tell the first that something changed. The
//! changes the owner makes through the viewers' own replica are those that
//! must also release the kept redactions at once.
//!
//! The tree is the canary family of the privacy tests: fictitious persons.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::http::{Method, StatusCode};
use oxidgene_api::profile::ProfileService;
use oxidgene_api::tree_cache::{CacheSettings, DEFAULT_BUDGET};
use oxidgene_core::access::TreeRole;
use oxidgene_db::repo::{AppUserRepo, MembershipRepo};
use oxidgene_db::sea_orm::DatabaseConnection;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::common::canary::{CONTEMPORARY, Canaries, EXCLUDED, OUTSIDE};
use crate::common::oidc::{Browser, MockProvider, MockUser, PUBLIC_URL};
use crate::common::{app_on, setup_db};

const WRENFIELD: &str = "Wrenfield";

struct Fixture {
    provider: MockProvider,
    db: DatabaseConnection,
    /// The replica the viewers read through.
    app: Router,
    /// Its projection service, holding its tree cache.
    profiles: Arc<ProfileService>,
    /// Another replica over the same database.
    other: Router,
    canaries: Canaries,
    tree: Uuid,
    /// The owner, signed in on the other replica.
    owner: Browser,
    /// The owner, signed in on the viewers' replica.
    owner_here: Browser,
}

impl Fixture {
    async fn new() -> Self {
        let provider = MockProvider::start().await;
        let db = setup_db().await;
        let (app, profiles) = provider.app_with_profiles(&db);
        let (other, _) = provider.app(&db, &[]);
        let canaries = Canaries::new(&app_on(db.clone()), &db).await;
        let tree = Uuid::parse_str(&canaries.tree).unwrap();
        let owner_user = MockUser::new("owner");
        let mut owner = Browser::new(&other);
        assert_eq!(
            owner.sign_in(&provider, &owner_user).await.status,
            StatusCode::SEE_OTHER
        );
        let mut owner_here = Browser::new(&app);
        assert_eq!(
            owner_here.sign_in(&provider, &owner_user).await.status,
            StatusCode::SEE_OTHER
        );
        let owner_id = user_id(&db, &provider, &owner_user).await;
        MembershipRepo::create(&db, tree, owner_id, TreeRole::Owner, None)
            .await
            .unwrap();
        Self {
            provider,
            db,
            app,
            profiles,
            other,
            canaries,
            tree,
            owner,
            owner_here,
        }
    }

    fn uri(&self, path: &str) -> String {
        format!("/api/v1/trees/{}{path}", self.tree)
    }

    fn person(&self, given: &str) -> String {
        self.canaries.person(given).to_owned()
    }

    /// A user signed in on the viewers' replica, with their id.
    async fn user(&self, name: &str) -> (Browser, Uuid) {
        let mut browser = Browser::new(&self.app);
        let user = MockUser::new(name);
        let answer = browser.sign_in(&self.provider, &user).await;
        assert_eq!(answer.status, StatusCode::SEE_OTHER, "{}", answer.json);
        let id = user_id(&self.db, &self.provider, &user).await;
        (browser, id)
    }

    /// A member of `role`, limited to `scope` when one is given.
    async fn member(&mut self, name: &str, role: &str, scope: Option<Value>) -> (Browser, Uuid) {
        let (browser, id) = self.user(name).await;
        let mut body = json!({ "username": name, "role": role });
        if let Some(scope) = scope {
            body["scope"] = scope;
        }
        let uri = self.uri("/members");
        let shared = self.owner.json(Method::POST, &uri, body).await;
        assert_eq!(shared.status, StatusCode::CREATED, "{}", shared.json);
        (browser, id)
    }

    /// A Visitor: a signed-in user of a tree opened to the site.
    async fn visitor(&mut self, name: &str) -> Browser {
        self.owner_writes(Method::PUT, "", json!({ "visibility": "site" }))
            .await;
        self.user(name).await.0
    }

    /// A write of the owner through the other replica.
    async fn owner_writes(&mut self, method: Method, path: &str, body: Value) -> Value {
        let uri = self.uri(path);
        let answer = self.owner.json(method, &uri, body).await;
        assert!(answer.status.is_success(), "{path}: {}", answer.json);
        answer.json
    }

    /// Whether `viewer` is shown person `given` in full — their profile
    /// bears their surname — over REST and GraphQL, which must agree.
    async fn shown(&self, viewer: &mut Browser, given: &str, surname: &str) -> bool {
        let id = self.person(given);
        let rest = viewer.get(&self.uri(&format!("/profiles/{id}"))).await;
        let rest_shown =
            rest.status == StatusCode::OK && rest.json["primary_name"]["surname"] == surname;
        let gql = viewer
            .gql(
                "query($tree: ID!, $id: ID!) { personProfile(treeId: $tree, personId: $id) { primaryName { surname } } }",
                json!({ "tree": self.tree, "id": id }),
            )
            .await;
        let gql_shown = gql.json["data"]["personProfile"]["primaryName"]["surname"] == surname;
        assert_eq!(
            rest_shown, gql_shown,
            "REST and GraphQL disagree on {given}: {} / {}",
            rest.json, gql.json
        );
        rest_shown
    }

    /// Whether `viewer` may read note `note`, over REST and GraphQL, which
    /// must agree.
    async fn note_shown(&self, viewer: &mut Browser, note: &str) -> bool {
        let rest = viewer.get(&self.uri(&format!("/notes/{note}"))).await;
        let gql = viewer
            .gql(
                "query($tree: ID!, $id: ID!) { note(treeId: $tree, id: $id) { id } }",
                json!({ "tree": self.tree, "id": note }),
            )
            .await;
        let gql_shown = gql.json["data"]["note"]["id"] == note;
        assert_eq!(
            rest.status == StatusCode::OK,
            gql_shown,
            "REST and GraphQL disagree on note {note}: {} / {}",
            rest.json,
            gql.json
        );
        gql_shown
    }

    /// The id of the first note about person `given`.
    async fn note_of(&mut self, given: &str) -> String {
        let person = self.person(given);
        let uri = self.uri(&format!("/notes?person_id={person}"));
        let notes = self.owner.get(&uri).await;
        notes.json["edges"][0]["node"]["id"]
            .as_str()
            .expect("a note")
            .to_owned()
    }

    /// The id of person `given`'s birth.
    async fn birth_of(&mut self, given: &str) -> String {
        let person = self.person(given);
        let uri = self.uri(&format!("/events?person_id={person}"));
        let events = self.owner.get(&uri).await;
        events.json["edges"]
            .as_array()
            .unwrap()
            .iter()
            .find(|edge| edge["node"]["event_type"] == "birth")
            .expect("a birth")["node"]["id"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    /// How many viewer classes' redactions of the tree the viewers'
    /// replica keeps.
    fn classes(&self) -> usize {
        self.profiles.cache().redaction_classes(self.tree).len()
    }
}

async fn user_id(db: &DatabaseConnection, provider: &MockProvider, user: &MockUser) -> Uuid {
    AppUserRepo::find_by_identity(db, provider.issuer(), &user.subject)
        .await
        .unwrap()
        .expect("provisioned")
        .id
}

#[tokio::test]
async fn an_exclusion_added_or_removed_reaches_the_next_request() {
    let mut f = Fixture::new().await;
    let (mut editor, _) = f.member("editorone", "editor", None).await;
    assert!(f.shown(&mut editor, "Thistle", EXCLUDED).await);
    assert_eq!(f.classes(), 1, "the Editor's redaction is kept");

    let excluded = f
        .owner_writes(
            Method::POST,
            "/exclusions",
            json!({ "kind": "person", "person_id": f.person("Thistle") }),
        )
        .await;
    assert!(!f.shown(&mut editor, "Thistle", EXCLUDED).await);
    let id = excluded["id"].as_str().unwrap();
    f.owner_writes(Method::DELETE, &format!("/exclusions/{id}"), Value::Null)
        .await;
    assert!(f.shown(&mut editor, "Thistle", EXCLUDED).await);
    assert_eq!(f.classes(), 1, "stale redactions are dropped, not piled up");
}

#[tokio::test]
async fn a_scope_redefined_reaches_the_next_request() {
    let mut f = Fixture::new().await;
    let lineage = json!({ "kind": "ancestors_of", "person_id": f.person("Harlan") });
    let (mut guest, guest_id) = f.member("guestone", "guest", Some(lineage)).await;
    assert!(f.shown(&mut guest, "Ashby", WRENFIELD).await);
    assert!(!f.shown(&mut guest, "Fallow", OUTSIDE).await);

    let elsewhere = json!({ "kind": "ancestors_of", "person_id": f.person("Fallow") });
    f.owner_writes(
        Method::PUT,
        &format!("/members/{guest_id}"),
        json!({ "scope": elsewhere }),
    )
    .await;
    assert!(f.shown(&mut guest, "Fallow", OUTSIDE).await);
    assert!(!f.shown(&mut guest, "Ashby", WRENFIELD).await);
}

#[tokio::test]
async fn a_persons_privacy_changed_reaches_the_next_request() {
    let mut f = Fixture::new().await;
    let mut visitor = f.visitor("visitorone").await;
    assert!(f.shown(&mut visitor, "Ashby", WRENFIELD).await);

    let ashby = f.person("Ashby");
    f.owner_writes(
        Method::PUT,
        &format!("/persons/{ashby}"),
        json!({ "privacy": "private" }),
    )
    .await;
    assert!(!f.shown(&mut visitor, "Ashby", WRENFIELD).await);
    f.owner_writes(
        Method::PUT,
        &format!("/persons/{ashby}"),
        json!({ "privacy": "public" }),
    )
    .await;
    assert!(f.shown(&mut visitor, "Ashby", WRENFIELD).await);
}

#[tokio::test]
async fn a_birth_that_ends_a_contemporary_reaches_the_next_request() {
    let mut f = Fixture::new().await;
    let mut visitor = f.visitor("visitorone").await;
    let note = f.note_of("Quillon").await;
    assert!(!f.shown(&mut visitor, "Quillon", CONTEMPORARY).await);
    assert!(!f.note_shown(&mut visitor, &note).await);

    let birth = f.birth_of("Quillon").await;
    f.owner_writes(
        Method::PUT,
        &format!("/events/{birth}"),
        json!({ "date_value": "1890" }),
    )
    .await;
    assert!(f.shown(&mut visitor, "Quillon", CONTEMPORARY).await);
    assert!(
        f.note_shown(&mut visitor, &note).await,
        "hidden records too"
    );
    f.owner_writes(
        Method::PUT,
        &format!("/events/{birth}"),
        json!({ "date_value": "1990" }),
    )
    .await;
    assert!(!f.shown(&mut visitor, "Quillon", CONTEMPORARY).await);
    assert!(!f.note_shown(&mut visitor, &note).await);
}

#[tokio::test]
async fn the_trees_privacy_settings_and_visibility_reach_the_next_request() {
    let mut f = Fixture::new().await;
    let mut visitor = f.visitor("visitorone").await;
    assert!(!f.shown(&mut visitor, "Quillon", CONTEMPORARY).await);

    f.owner_writes(Method::PUT, "", json!({ "contemporary_rule": false }))
        .await;
    assert!(f.shown(&mut visitor, "Quillon", CONTEMPORARY).await);
    f.owner_writes(Method::PUT, "", json!({ "default_privacy": "private" }))
        .await;
    assert!(!f.shown(&mut visitor, "Ashby", WRENFIELD).await);
    f.owner_writes(Method::PUT, "", json!({ "default_privacy": "public" }))
        .await;
    assert!(f.shown(&mut visitor, "Ashby", WRENFIELD).await);

    f.owner_writes(Method::PUT, "", json!({ "visibility": "private" }))
        .await;
    assert!(!f.shown(&mut visitor, "Ashby", WRENFIELD).await);
}

#[tokio::test]
async fn a_members_role_changed_or_revoked_reaches_the_next_request() {
    let mut f = Fixture::new().await;
    let ashby = f.person("Ashby");
    let confidential = f
        .owner_writes(
            Method::POST,
            "/notes",
            json!({ "text": "Fictitious sealed remark", "person_id": ashby, "confidential": true }),
        )
        .await;
    let confidential = confidential["id"].as_str().unwrap().to_owned();
    let (mut member, member_id) = f.member("editorone", "editor", None).await;
    assert!(f.note_shown(&mut member, &confidential).await);

    // A Guest: no confidential notes.
    f.owner_writes(
        Method::PUT,
        &format!("/members/{member_id}"),
        json!({ "role": "guest" }),
    )
    .await;
    assert!(!f.note_shown(&mut member, &confidential).await);
    assert!(f.shown(&mut member, "Ashby", WRENFIELD).await);

    // Revoked through the viewers' own replica: what was kept for the tree
    // goes at once.
    let uri = f.uri(&format!("/members/{member_id}"));
    let removed = f.owner_here.json(Method::DELETE, &uri, Value::Null).await;
    assert_eq!(removed.status, StatusCode::NO_CONTENT, "{}", removed.json);
    assert_eq!(f.classes(), 0, "released with the membership");
    assert!(!f.shown(&mut member, "Ashby", WRENFIELD).await);
}

#[tokio::test]
async fn a_revoked_share_link_reaches_the_next_request_and_releases_its_redaction() {
    let mut f = Fixture::new().await;
    let uri = f.uri("/share-links");
    let created = f
        .owner_here
        .json(Method::POST, &uri, json!({ "label": "Cousins" }))
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.json);
    let url = created.json["url"].as_str().unwrap().to_owned();
    let link = created.json["link"]["id"].as_str().unwrap().to_owned();
    let mut holder = Browser::new(&f.app);
    let opened = holder.get(url.strip_prefix(PUBLIC_URL).unwrap()).await;
    assert_eq!(opened.status, StatusCode::SEE_OTHER);
    assert!(f.shown(&mut holder, "Ashby", WRENFIELD).await);
    assert!(!f.shown(&mut holder, "Quillon", CONTEMPORARY).await);
    assert_eq!(f.classes(), 1);

    let uri = f.uri(&format!("/share-links/{link}"));
    let revoked = f.owner_here.json(Method::DELETE, &uri, Value::Null).await;
    assert_eq!(revoked.status, StatusCode::NO_CONTENT, "{}", revoked.json);
    assert_eq!(f.classes(), 0, "released with the link");
    assert!(!f.shown(&mut holder, "Ashby", WRENFIELD).await);
}

/// The withheld media and the counts a shown person's profile leaves out
/// are kept with the redaction, and follow a medium's privacy and links,
/// and a confidential note, at the next request.
#[tokio::test]
async fn withheld_media_and_counts_reach_the_next_request() {
    let mut f = Fixture::new().await;
    let mut visitor = f.visitor("visitorone").await;
    let (mut guest, _) = f.member("guestone", "guest", None).await;
    let ashby = f.person("Ashby");
    // A count of zero is left out of the payload.
    let count = |answer: &Value, field: &str| answer[field].as_u64().unwrap_or(0);
    let uri = f.uri(&format!("/profiles/{ashby}"));
    let media = count(&visitor.get(&uri).await.json, "media_count");
    let notes = count(&guest.get(&uri).await.json, "note_count");

    let document = f
        .owner_writes(
            Method::POST,
            "/media/document",
            json!({ "title": "Fictitious register" }),
        )
        .await;
    let document = document["id"].as_str().unwrap().to_owned();
    f.owner_writes(
        Method::POST,
        "/media-links",
        json!({ "media_id": document, "person_id": ashby }),
    )
    .await;
    assert_eq!(
        count(&visitor.get(&uri).await.json, "media_count"),
        media + 1
    );
    f.owner_writes(
        Method::PUT,
        &format!("/media/{document}"),
        json!({ "privacy": "private" }),
    )
    .await;
    assert_eq!(
        count(&visitor.get(&uri).await.json, "media_count"),
        media,
        "a private medium is withheld from a Visitor's count"
    );

    f.owner_writes(
        Method::POST,
        "/notes",
        json!({ "text": "Fictitious sealed remark", "person_id": ashby, "confidential": true }),
    )
    .await;
    assert_eq!(count(&guest.get(&uri).await.json, "note_count"), notes);
    f.owner_writes(
        Method::POST,
        "/notes",
        json!({ "text": "Fictitious open remark", "person_id": ashby }),
    )
    .await;
    assert_eq!(count(&guest.get(&uri).await.json, "note_count"), notes + 1);
}

#[tokio::test]
async fn a_new_year_is_a_new_class() {
    let mut f = Fixture::new().await;
    let birth = f.birth_of("Quillon").await;
    // Contemporary until 2030, a hundred years after 1930.
    f.owner_writes(
        Method::PUT,
        &format!("/events/{birth}"),
        json!({ "date_value": "1930" }),
    )
    .await;
    let mut visitor = f.visitor("visitorone").await;
    f.profiles.cache().set_year(Some(2029));
    assert!(!f.shown(&mut visitor, "Quillon", CONTEMPORARY).await);
    f.profiles.cache().set_year(Some(2030));
    assert!(f.shown(&mut visitor, "Quillon", CONTEMPORARY).await);
    let years: Vec<i32> = f
        .profiles
        .cache()
        .redaction_classes(f.tree)
        .iter()
        .map(|class| class.year)
        .collect();
    assert_eq!(years, [2030], "last year's redaction is dropped");
}

#[tokio::test]
async fn a_write_through_another_replica_reaches_the_next_request() {
    let mut f = Fixture::new().await;
    let mut visitor = f.visitor("visitorone").await;
    assert!(f.shown(&mut visitor, "Corwin", WRENFIELD).await);
    // An Editor signed in on both replicas writes through the other one.
    let user = MockUser::new("editorone");
    let mut here = Browser::new(&f.app);
    here.sign_in(&f.provider, &user).await;
    let mut there = Browser::new(&f.other);
    there.sign_in(&f.provider, &user).await;
    let id = user_id(&f.db, &f.provider, &user).await;
    MembershipRepo::create(&f.db, f.tree, id, TreeRole::Editor, None)
        .await
        .unwrap();
    assert!(f.shown(&mut here, "Corwin", WRENFIELD).await);
    let corwin = f.person("Corwin");
    let uri = f.uri(&format!("/persons/{corwin}"));
    let written = there
        .json(Method::PUT, &uri, json!({ "privacy": "private" }))
        .await;
    assert!(written.status.is_success(), "{}", written.json);
    assert!(!f.shown(&mut visitor, "Corwin", WRENFIELD).await);
    assert!(f.shown(&mut here, "Corwin", WRENFIELD).await);
}

#[tokio::test]
async fn two_classes_never_receive_each_others_redaction() {
    let mut f = Fixture::new().await;
    let (mut editor, _) = f.member("editorone", "editor", None).await;
    let mut visitor = f.visitor("visitorone").await;
    for _ in 0..2 {
        assert!(f.shown(&mut editor, "Quillon", CONTEMPORARY).await);
        assert!(!f.shown(&mut visitor, "Quillon", CONTEMPORARY).await);
    }
    assert_eq!(f.classes(), 2);
    // A second Visitor shares the first one's class.
    let mut another = f.user("visitortwo").await.0;
    assert!(!f.shown(&mut another, "Quillon", CONTEMPORARY).await);
    assert_eq!(f.classes(), 2);
}

#[tokio::test]
async fn the_owner_keeps_nothing() {
    let f = Fixture::new().await;
    let mut owner = Browser::new(&f.app);
    owner.cookies.clone_from(&f.owner_here.cookies);
    owner.csrf.clone_from(&f.owner_here.csrf);
    owner.origin.clone_from(&f.owner_here.origin);
    assert!(f.shown(&mut owner, "Quillon", CONTEMPORARY).await);
    assert!(f.shown(&mut owner, "Thistle", EXCLUDED).await);
    assert_eq!(f.classes(), 0);

    // Nor does the desktop's local owner, or the `none` mode's.
    let state = oxidgene_api::AppState::new(f.db.clone(), crate::common::test_media_root());
    let profiles = Arc::clone(&state.profiles);
    let local = oxidgene_api::build_router(crate::common::offline(state));
    let quillon = f.person("Quillon");
    crate::common::ok(
        &local,
        Method::GET,
        &f.uri(&format!("/profiles/{quillon}")),
        None,
    )
    .await;
    assert!(profiles.cache().redaction_classes(f.tree).is_empty());
}

#[tokio::test]
async fn a_deleted_tree_and_an_idle_one_release_their_redactions() {
    let mut f = Fixture::new().await;
    f.profiles.cache().configure(CacheSettings::shared(
        DEFAULT_BUDGET,
        Duration::from_millis(300),
    ));
    let mut visitor = f.visitor("visitorone").await;
    assert!(f.shown(&mut visitor, "Ashby", WRENFIELD).await);
    assert_eq!(f.classes(), 1);
    tokio::time::sleep(Duration::from_millis(900)).await;
    assert!(
        !f.profiles.cache().cached_trees().contains(&f.tree),
        "dropped when idle"
    );

    f.profiles.cache().configure(CacheSettings::shared(
        DEFAULT_BUDGET,
        Duration::from_secs(300),
    ));
    assert!(f.shown(&mut visitor, "Ashby", WRENFIELD).await);
    assert_eq!(f.classes(), 1);
    let uri = f.uri("");
    let deleted = f.owner_here.json(Method::DELETE, &uri, Value::Null).await;
    assert!(deleted.status.is_success(), "{}", deleted.json);
    assert!(!f.profiles.cache().cached_trees().contains(&f.tree));
}

#[tokio::test]
async fn a_redaction_counts_toward_the_budget() {
    let mut f = Fixture::new().await;
    let mut visitor = f.visitor("visitorone").await;
    assert!(!f.shown(&mut visitor, "Quillon", CONTEMPORARY).await);
    let cache = f.profiles.cache();
    let bytes = cache.redaction_bytes(f.tree);
    assert!(bytes > 0);
    assert!(cache.estimated_bytes() >= bytes);

    // A budget it does not fit keeps nothing, and answers the same.
    cache.clear();
    cache.configure(CacheSettings::shared(bytes - 1, Duration::from_secs(300)));
    assert!(!f.shown(&mut visitor, "Quillon", CONTEMPORARY).await);
    assert!(f.shown(&mut visitor, "Ashby", WRENFIELD).await);
    assert_eq!(f.classes(), 0);
}

#[tokio::test]
async fn a_placeholders_pseudonym_is_drawn_afresh_for_each_request() {
    let mut f = Fixture::new().await;
    f.owner_writes(Method::PUT, "", json!({ "hidden_navigation": false }))
        .await;
    let mut visitor = f.visitor("visitorone").await;
    let harlan = f.person("Harlan");
    let quillon = f.person("Quillon");
    let uri = f.uri(&format!("/profiles/{harlan}"));
    let mut seen = Vec::new();
    for _ in 0..3 {
        let profile = visitor.get(&uri).await;
        let child = profile.json["families_as_spouse"][0]["children_ids"][0]
            .as_str()
            .expect("the contemporary's placeholder")
            .to_owned();
        assert_ne!(child, quillon, "never the person's own identifier");
        seen.push(child);
    }
    assert_eq!(f.classes(), 1, "one redaction served all three");
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), 3, "no two responses link the placeholder");
}

/// When the generation cannot be read, the redaction is built for the
/// request, never taken from the cache unchecked. SQLite only: the test
/// renames the column, which SQLite's triggers follow.
#[tokio::test]
async fn an_unreadable_generation_fails_closed() {
    use oxidgene_db::sea_orm::ConnectionTrait as _;

    if std::env::var("OXIDGENE_TEST_POSTGRES_URL").is_ok() {
        return;
    }
    let mut f = Fixture::new().await;
    let mut visitor = f.visitor("visitorone").await;
    assert!(f.shown(&mut visitor, "Ashby", WRENFIELD).await);
    assert_eq!(f.classes(), 1);
    f.db.execute_unprepared(
        "ALTER TABLE projection_state RENAME COLUMN redaction_generation TO unreadable",
    )
    .await
    .unwrap();
    let ashby = f.person("Ashby");
    f.owner_writes(
        Method::PUT,
        &format!("/persons/{ashby}"),
        json!({ "privacy": "private" }),
    )
    .await;
    assert!(!f.shown(&mut visitor, "Ashby", WRENFIELD).await);
}

/// Counts the SQL statements SeaORM runs, as `query_scaling_test` does.
struct StatementCounter(Arc<std::sync::atomic::AtomicUsize>);

impl<S: tracing::Subscriber> tracing_subscriber::layer::Layer<S> for StatementCounter {
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

/// What a restricted viewer's page costs on a generated tree of 10,000
/// persons with its redaction built (cold) and kept (warm), and what the
/// kept redaction takes: estimated, and measured as the bytes the
/// allocator holds more once it is kept. The page is the pedigree page's
/// three requests. The viewers are an Editor kept from the excluded
/// ancestries of one person in twenty, a Guest limited to the root's
/// descendants, and a Visitor of the tree opened to the site, a third of
/// its persons private and the contemporary rule at 150 years.
///
/// Timing wants an optimised build: `just bench` runs it in release mode.
#[tokio::test]
#[ignore = "benchmark: run by `just bench` in release mode"]
async fn redaction_cost_on_ten_thousand_persons() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    use oxidgene_core::sharing::ScopeDefinition;
    use oxidgene_db::repo::SharedMembership;
    use tracing_subscriber::layer::SubscriberExt as _;
    use tracing_subscriber::util::SubscriberInitExt as _;

    let counter = Arc::new(AtomicUsize::new(0));
    let _guard = tracing_subscriber::registry()
        .with(StatementCounter(Arc::clone(&counter)))
        .set_default();
    let provider = MockProvider::start().await;
    let db = setup_db().await;
    let local = app_on(db.clone());
    let (tree, anchor) = crate::common::family_blocks_tree(&local, &db, 1_000).await;
    let tree_id: Uuid = tree.parse().unwrap();
    let base = format!("/api/v1/trees/{tree}");
    crate::common::ok(
        &local,
        Method::PUT,
        &base,
        Some(json!({ "contemporary_years": 150 })),
    )
    .await;
    // Opened to the site: a sharing setting the local owner has no use for.
    oxidgene_db::repo::TreeRepo::update(
        &db,
        tree_id,
        oxidgene_db::repo::TreeChanges {
            visibility: Some(oxidgene_core::sharing::TreeVisibility::Site),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    hide_from_viewers(&local, &db, &tree, &anchor).await;
    let (app, profiles) = provider.app_with_profiles(&db);
    let definition: ScopeDefinition =
        serde_json::from_value(json!({ "kind": "descendants_of", "person_id": anchor })).unwrap();
    let scope = oxidgene_api::service::access_scope::create(&db, tree_id, &definition)
        .await
        .unwrap();
    let mut viewers = Vec::new();
    for (name, role, scope_id) in [
        ("editor", Some(TreeRole::Editor), None),
        ("scoped", Some(TreeRole::Guest), Some(scope.id)),
        ("visitor", None, None),
    ] {
        let user = MockUser::new(name);
        let mut browser = Browser::new(&app);
        browser.sign_in(&provider, &user).await;
        if let Some(role) = role {
            let user_id = user_id(&db, &provider, &user).await;
            MembershipRepo::create_shared(
                &db,
                SharedMembership {
                    tree_id,
                    user_id,
                    role,
                    scope_id,
                    expires_at: None,
                    created_by: None,
                },
            )
            .await
            .unwrap();
        }
        viewers.push((name, browser));
    }
    let page = [
        format!("{base}/pedigree/{anchor}?ancestor_depth=4&descendant_depth=3"),
        format!("{base}/profiles/{anchor}"),
        format!("{base}/persons/{anchor}/detail-bundle"),
    ];
    let cache = profiles.cache();
    // The owner's page, which no redaction costs anything: the baseline.
    let mut owner = Vec::new();
    for _ in 0..6 {
        counter.store(0, Ordering::Relaxed);
        let started = Instant::now();
        for uri in &page {
            crate::common::ok(&local, Method::GET, uri, None).await;
        }
        owner.push((started.elapsed(), counter.load(Ordering::Relaxed)));
    }
    owner.sort();
    eprintln!(
        "owner: page {:?} / {} statements",
        owner[owner.len() / 2].0,
        owner[owner.len() / 2].1
    );
    // Lets a dropped redaction be freed on the blocking pool before the
    // allocator is read.
    let settle = || tokio::time::sleep(Duration::from_millis(300));
    for (name, browser) in &mut viewers {
        // What the page keeps besides the redaction kept first, so that
        // what a cold page adds is the redaction alone.
        for uri in &page {
            browser.get(uri).await;
        }
        let mut cold = Vec::new();
        let mut warm = Vec::new();
        let mut retained = Vec::new();
        for pass in 0..10 {
            if pass % 2 == 0 {
                cache.forget_redactions(tree_id);
                settle().await;
            }
            let before = oxidgene_api::memory::allocated_bytes();
            counter.store(0, Ordering::Relaxed);
            let started = Instant::now();
            for uri in &page {
                let answer = browser.get(uri).await;
                assert_eq!(
                    answer.status,
                    StatusCode::OK,
                    "{name} {uri}: {}",
                    answer.json
                );
            }
            let cost = (started.elapsed(), counter.load(Ordering::Relaxed));
            if pass % 2 == 0 {
                cold.push(cost);
                settle().await;
                if let (Some(before), Some(after)) =
                    (before, oxidgene_api::memory::allocated_bytes())
                {
                    retained.push(after.saturating_sub(before));
                }
            } else {
                warm.push(cost);
            }
        }
        cold.sort();
        warm.sort();
        retained.sort_unstable();
        let median = |costs: &[(Duration, usize)]| costs[costs.len() / 2];
        let (cold, warm) = (median(&cold), median(&warm));
        let mib = |bytes: usize| bytes as f64 / 1_048_576.0;
        eprintln!(
            "{name}: page cold {:?} / {} statements, warm {:?} / {} statements; \
             redaction estimated {:.2} MiB, allocated {:.2} MiB ({retained:?})",
            cold.0,
            cold.1,
            warm.0,
            warm.1,
            mib(cache.redaction_bytes(tree_id)),
            mib(retained.get(retained.len() / 2).copied().unwrap_or(0)),
        );
    }
}

/// The persons of the benchmark's tree that `anchor` does not descend from.
fn outside_descent(all: &[Value], anchor: &str) -> Vec<String> {
    let by_id: std::collections::HashMap<&str, &Value> = all
        .iter()
        .map(|profile| (profile["person_id"].as_str().unwrap(), profile))
        .collect();
    let children = |person: &str| -> Vec<String> {
        by_id[person]["families_as_spouse"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|family| family["children_ids"].as_array())
            .flatten()
            .filter_map(|child| child.as_str().map(str::to_owned))
            .collect()
    };
    let mut descent = std::collections::HashSet::from([anchor.to_owned()]);
    let mut next = vec![anchor.to_owned()];
    while let Some(person) = next.pop() {
        for child in children(&person) {
            if descent.insert(child.clone()) {
                next.push(child);
            }
        }
    }
    by_id
        .keys()
        .filter(|id| !descent.contains(**id))
        .map(|id| (*id).to_owned())
        .collect()
}

/// Hide the benchmark's persons from its viewers, but for `anchor` and its
/// descendants, whose page is read: the ancestries of one person in twenty
/// excluded, some 2,000 persons hidden from the Editor as a real tree's
/// excluded branch would be; and a third of the others private, standing
/// for the contemporaries of a real tree, whose generated dates are all
/// long past: what a Visitor does not see.
async fn hide_from_viewers(local: &Router, db: &DatabaseConnection, tree: &str, anchor: &str) {
    use oxidgene_db::sea_orm::ConnectionTrait as _;

    let persons = outside_descent(&crate::common::all_profiles(local, tree).await, anchor);
    for person in persons.iter().step_by(20) {
        crate::common::ok(
            local,
            Method::POST,
            &format!("/api/v1/trees/{tree}/exclusions"),
            Some(json!({ "kind": "ancestors", "person_id": person, "include_root": true })),
        )
        .await;
    }
    for person in persons.iter().step_by(3) {
        db.execute_raw(oxidgene_db::sea_orm::Statement::from_sql_and_values(
            db.get_database_backend(),
            "UPDATE person_visibility SET privacy = 'private' WHERE person_id = $1",
            [person.parse::<Uuid>().unwrap().into()],
        ))
        .await
        .unwrap();
    }
}
