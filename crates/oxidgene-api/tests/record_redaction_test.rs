//! What the restricted viewers of phase 3 see of a tree's records
//! (docs/privacy.md §3.2, §6.4): the Editor and the Guest of a tree whose
//! owner excluded a person from sharing, through REST and GraphQL alike,
//! and the confidential notes a Guest never reads.
//!
//! The fixture is the populated tree of the guards — one record of every
//! kind hanging from the fictitious "Anchor" — with the anchor excluded:
//! their events, citations, notes, document and its pages, the source only
//! their citation draws on, the repository holding it and the place only
//! their birth names are hidden from both viewers. A confidential note on a
//! visible person is withheld from the Guest alone.

use std::collections::BTreeMap;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt as _;
use oxidgene_core::access::{InstanceRole, TreeRole};
use oxidgene_db::repo::{AppUserRepo, MembershipRepo, NewAppUser};
use oxidgene_db::sea_orm::DatabaseConnection;
use serde_json::{Value, json};
use tower::ServiceExt as _;
use uuid::Uuid;

use crate::common::oidc::{MockProvider, TRUSTED_CLIENT};
use crate::common::populated::{Populated, populated_tree};
use crate::common::{app_on, gql_ok, ok, setup_db};

/// A viewer of the fixture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Who {
    Guest,
    Editor,
}

const VIEWERS: [Who; 2] = [Who::Guest, Who::Editor];

/// A confidential note's fictitious text.
const SEALED: &str = "Larchmont sealed remark";

struct Fixture {
    /// The tree's owner: the local owner of a build without authentication.
    owner: axum::Router,
    app: axum::Router,
    tokens: BTreeMap<Who, String>,
    tree: Populated,
    /// The confidential note, on a visible person.
    sealed: String,
    /// The visible person the confidential note is about.
    visible: String,
    _provider: MockProvider,
}

async fn user(db: &DatabaseConnection, provider: &MockProvider, subject: &str) -> Uuid {
    AppUserRepo::create(
        db,
        NewAppUser {
            issuer: provider.issuer().to_string(),
            subject: subject.to_string(),
            email: Some(format!("{subject}@example.invalid")),
            email_verified: true,
            provider_name: Some(format!("Fictive {subject}")),
            username: Some(subject.to_string()),
            locale: None,
            instance_role: InstanceRole::User,
            can_create_trees: false,
        },
    )
    .await
    .expect("creates the user")
    .id
}

async fn fixture() -> Fixture {
    let provider = MockProvider::start().await;
    let db = setup_db().await;
    let owner = app_on(db.clone());
    let tree = populated_tree(&owner, &db, "Redaction Fixture", 1).await;
    let base = format!("/api/v1/trees/{}", tree.tree_id);
    ok(
        &owner,
        Method::POST,
        &format!("{base}/exclusions"),
        Some(json!({ "kind": "person", "person_id": tree.id("person_id") })),
    )
    .await;
    let visible = tree.id("spouse_id").to_string();
    let sealed = ok(
        &owner,
        Method::POST,
        &format!("{base}/notes"),
        Some(json!({ "text": SEALED, "person_id": visible, "confidential": true })),
    )
    .await;
    assert_eq!(sealed["confidential"], true, "{sealed}");
    let (app, _) = provider.app(&db, &[]);
    let tree_id: Uuid = tree.tree_id.parse().unwrap();
    let mut tokens = BTreeMap::new();
    for who in VIEWERS {
        let subject = format!("{who:?}").to_lowercase();
        let id = user(&db, &provider, &subject).await;
        let role = match who {
            Who::Guest => TreeRole::Guest,
            Who::Editor => TreeRole::Editor,
        };
        MembershipRepo::create(&db, tree_id, id, role, None)
            .await
            .expect("shares the tree");
        tokens.insert(who, provider.access_token(&subject, TRUSTED_CLIENT, 3600));
    }
    Fixture {
        owner,
        app,
        tokens,
        sealed: sealed["id"].as_str().unwrap().to_string(),
        visible,
        tree,
        _provider: provider,
    }
}

impl Fixture {
    fn path(&self, path: &str) -> String {
        format!("/api/v1/trees/{}{path}", self.tree.tree_id)
    }

    async fn rest(
        &self,
        who: Who,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(self.path(path))
            .header(header::HOST, "genealogy.example.invalid")
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", self.tokens[&who]),
            );
        let body = match body {
            Some(body) => {
                request = request.header(header::CONTENT_TYPE, "application/json");
                Body::from(body.to_string())
            }
            None => Body::empty(),
        };
        let response = self
            .app
            .clone()
            .oneshot(request.body(body).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// A read `who` must be answered.
    async fn get(&self, who: Who, path: &str) -> Value {
        let (status, body) = self.rest(who, Method::GET, path, None).await;
        assert_eq!(status, StatusCode::OK, "{who:?} GET {path}: {body}");
        body
    }

    /// A read `who` must be answered `404`.
    async fn missing(&self, who: Who, path: &str) {
        let (status, body) = self.rest(who, Method::GET, path, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{who:?} GET {path}: {body}");
        assert_eq!(body["error"], "not_found", "{who:?} GET {path}");
    }

    async fn gql(&self, who: Who, query: &str) -> Value {
        let request = Request::builder()
            .method(Method::POST)
            .uri("/graphql")
            .header(header::HOST, "genealogy.example.invalid")
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", self.tokens[&who]),
            )
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({ "query": query }).to_string()))
            .unwrap();
        let response = self.app.clone().oneshot(request).await.unwrap();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// The data of a query `who` must be answered without error.
    async fn data(&self, who: Who, query: &str) -> Value {
        let response = self.gql(who, query).await;
        assert!(response["errors"].is_null(), "{who:?} {query}: {response}");
        response["data"].clone()
    }
}

/// The ids of a REST connection's nodes.
fn ids(page: &Value) -> Vec<String> {
    page["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|edge| edge["node"]["id"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn an_editor_sets_confidentiality_and_a_guest_cannot() {
    let f = fixture().await;
    let body = json!({ "text": "Editor remark", "person_id": f.visible, "confidential": true });
    let (status, note) = f
        .rest(Who::Editor, Method::POST, "/notes", Some(body.clone()))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{note}");
    assert_eq!(note["confidential"], true);
    let (status, _) = f.rest(Who::Guest, Method::POST, "/notes", Some(body)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let path = format!("/notes/{}", note["id"].as_str().unwrap());
    let (status, _) = f
        .rest(
            Who::Guest,
            Method::PUT,
            &path,
            Some(json!({ "confidential": false })),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let mutation = format!(
        "mutation {{ createNote(treeId: \"{}\", input: {{ text: \"Editor remark\", personId: \"{}\", confidential: true }}) {{ confidential }} }}",
        f.tree.tree_id, f.visible
    );
    let data = f.data(Who::Editor, &mutation).await;
    assert_eq!(data["createNote"]["confidential"], true);
    let refused = f.gql(Who::Guest, &mutation).await;
    assert_eq!(
        refused["errors"][0]["extensions"]["code"], "FORBIDDEN",
        "{refused}"
    );
}

#[tokio::test]
async fn a_guest_never_reads_a_confidential_note() {
    let f = fixture().await;
    let by_person = format!("/notes?person_id={}", f.visible);
    let editor = f.get(Who::Editor, &by_person).await;
    assert!(ids(&editor).contains(&f.sealed));
    let guest = f.get(Who::Guest, &by_person).await;
    assert!(!ids(&guest).contains(&f.sealed), "{guest}");
    assert_eq!(
        guest["total_count"].as_i64().unwrap(),
        editor["total_count"].as_i64().unwrap() - 1,
        "the count leaves it out too"
    );
    f.get(Who::Editor, &format!("/notes/{}", f.sealed)).await;
    f.missing(Who::Guest, &format!("/notes/{}", f.sealed)).await;

    let query = format!(
        "{{ note(treeId: \"{t}\", id: \"{n}\") {{ id }} notes(treeId: \"{t}\", personId: \"{p}\") {{ totalCount edges {{ node {{ id }} }} }} person(treeId: \"{t}\", id: \"{p}\") {{ notes {{ id }} }} }}",
        t = f.tree.tree_id,
        n = f.sealed,
        p = f.visible
    );
    let guest = f.data(Who::Guest, &query).await;
    assert!(guest["note"].is_null(), "{guest}");
    assert!(!guest.to_string().contains(&f.sealed), "{guest}");
    let editor = f.data(Who::Editor, &query).await;
    assert_eq!(editor["note"]["id"], f.sealed.as_str());
    assert_eq!(
        guest["notes"]["totalCount"].as_i64().unwrap(),
        editor["notes"]["totalCount"].as_i64().unwrap() - 1
    );
    assert!(editor["person"]["notes"].to_string().contains(&f.sealed));
}

#[tokio::test]
async fn the_records_of_an_excluded_person_are_hidden_over_rest() {
    let f = fixture().await;
    let t = &f.tree;
    let anchor = t.id("person_id");
    for who in VIEWERS {
        for path in [
            format!("/events/{}", t.id("event_id")),
            format!("/events/{}/witnesses", t.id("event_id")),
            format!("/notes/{}", t.id("note_id")),
            format!("/media/{}", t.id("media_id")),
            format!("/media/{}", t.id("page_id")),
            format!("/media/{}/pages", t.id("media_id")),
            format!("/media/{}/file", t.id("page_id")),
            format!("/media/{}/thumbnail", t.id("page_id")),
            format!("/media/{}/download", t.id("page_id")),
            format!("/media/{}/viewer-bundle", t.id("media_id")),
            format!("/media/{}/vignettes", t.id("page_id")),
            format!("/vignettes/{}", t.id("vignette_id")),
            format!("/vignettes/{}/image", t.id("vignette_id")),
            format!("/sources/{}", t.id("source_id")),
            format!("/sources/{}/repositories", t.id("source_id")),
            format!("/repositories/{}", t.id("repository_id")),
            format!("/repositories/{}/sources", t.id("repository_id")),
            format!("/places/{}", t.id("place_id")),
        ] {
            f.missing(who, &path).await;
        }
        let events = f.get(who, "/events?first=100").await;
        assert!(!ids(&events).contains(&t.id("event_id").to_string()));
        let all = ok(&f.owner, Method::GET, &f.path("/events?first=100"), None).await;
        assert!(events["total_count"].as_i64() < all["total_count"].as_i64());
        for (path, id) in [
            ("/citations?first=100", "citation_id"),
            ("/notes?first=100", "note_id"),
            ("/sources?first=100", "source_id"),
            ("/repositories?first=100", "repository_id"),
            ("/places?first=100", "place_id"),
            ("/media?first=100", "media_id"),
        ] {
            let page = f.get(who, path).await;
            assert!(
                !ids(&page).contains(&t.id(id).to_string()),
                "{who:?} {path}"
            );
            let all = ok(&f.owner, Method::GET, &f.path(path), None).await;
            let count = page["total_count"].as_i64().unwrap();
            assert!(
                count < all["total_count"].as_i64().unwrap(),
                "{who:?} {path}"
            );
            assert_eq!(
                count,
                ids(&page).len() as i64,
                "{who:?} {path}: the count is the page's"
            );
        }
        let spouses = f
            .get(who, &format!("/families/{}/spouses", t.id("family_id")))
            .await;
        assert!(!spouses.to_string().contains(anchor), "{spouses}");
        assert_eq!(
            spouses.as_array().unwrap().len(),
            1,
            "the other spouse stays"
        );
        let links = f.get(who, "/media-links").await;
        assert!(!links.to_string().contains(t.id("media_id")), "{links}");
        let facets = f.get(who, "/media/facets").await;
        assert!(
            !facets["tags"].to_string().contains(t.id("tag_id")),
            "only the hidden document carries the tag: {facets}"
        );
        let tags = f.get(who, "/tags").await;
        assert_eq!(tags[0]["counts"]["persons"], 0, "{tags}");
        assert_eq!(tags[0]["counts"]["media"], 0, "{tags}");
        let tagged = f
            .get(
                who,
                &format!("/tags/{}/records?kind=person", t.id("tag_id")),
            )
            .await;
        assert_eq!(tagged["total_count"], 0, "{tagged}");
        let (status, bundle) = f
            .rest(
                who,
                Method::POST,
                "/gallery-bundle",
                Some(json!({ "media_ids": [t.id("media_id")], "vignette_ids": [t.id("vignette_id")] })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{bundle}");
        assert_eq!(bundle["media"], json!([]), "{bundle}");
        assert_eq!(bundle["vignettes"], json!([]), "{bundle}");
    }
    // The owner sees them all.
    ok(
        &f.owner,
        Method::GET,
        &f.path(&format!("/sources/{}", t.id("source_id"))),
        None,
    )
    .await;
}

#[tokio::test]
async fn the_records_of_an_excluded_person_are_hidden_over_graphql() {
    let f = fixture().await;
    let t = &f.tree;
    let query = format!(
        r#"{{
            event(treeId: "{tree}", id: "{event}") {{ id }}
            note(treeId: "{tree}", id: "{note}") {{ id }}
            media(treeId: "{tree}", id: "{media}") {{ id }}
            source(treeId: "{tree}", id: "{source}") {{ id }}
            repository(treeId: "{tree}", id: "{repository}") {{ id }}
            place(treeId: "{tree}", id: "{place}") {{ id }}
            vignette(treeId: "{tree}", id: "{vignette}") {{ id }}
            events(treeId: "{tree}", first: 100) {{ totalCount edges {{ node {{ id }} }} }}
            citations(treeId: "{tree}", first: 100) {{ totalCount edges {{ node {{ id }} }} }}
            sources(treeId: "{tree}", first: 100) {{ totalCount edges {{ node {{ id }} }} }}
            repositories(treeId: "{tree}", first: 100) {{ totalCount edges {{ node {{ id }} }} }}
            places(treeId: "{tree}", first: 100) {{ totalCount edges {{ node {{ id }} }} }}
            mediaList(treeId: "{tree}", first: 100) {{ totalCount }}
            family(treeId: "{tree}", id: "{family}") {{ spouses {{ person {{ id }} }} }}
            tags(treeId: "{tree}") {{ counts {{ persons media }} }}
        }}"#,
        tree = t.tree_id,
        event = t.id("event_id"),
        note = t.id("note_id"),
        media = t.id("media_id"),
        source = t.id("source_id"),
        repository = t.id("repository_id"),
        place = t.id("place_id"),
        vignette = t.id("vignette_id"),
        family = t.id("family_id"),
    );
    let owner = gql_ok(&f.owner, &query, json!({})).await;
    for key in [
        "event",
        "note",
        "media",
        "source",
        "repository",
        "place",
        "vignette",
    ] {
        assert!(!owner[key].is_null(), "the owner reads the {key}");
    }
    for who in VIEWERS {
        let data = f.data(who, &query).await;
        for key in [
            "event",
            "note",
            "media",
            "source",
            "repository",
            "place",
            "vignette",
        ] {
            assert!(data[key].is_null(), "{who:?} {key}: {data}");
        }
        assert!(
            data["events"]["totalCount"].as_i64() < owner["events"]["totalCount"].as_i64(),
            "{who:?} events"
        );
        for key in [
            "citations",
            "sources",
            "repositories",
            "places",
            "mediaList",
        ] {
            assert!(
                data[key]["totalCount"].as_i64() < owner[key]["totalCount"].as_i64(),
                "{who:?} {key}"
            );
        }
        for key in ["events", "citations", "sources", "repositories", "places"] {
            assert_eq!(
                data[key]["totalCount"].as_u64().unwrap() as usize,
                data[key]["edges"].as_array().unwrap().len(),
                "{who:?} {key}: the count is the page's"
            );
        }
        let text = data.to_string();
        assert!(!text.contains(t.id("person_id")), "{who:?}: {text}");
        assert_eq!(data["family"]["spouses"].as_array().unwrap().len(), 1);
        assert_eq!(data["tags"][0]["counts"]["persons"], 0);
        assert_eq!(data["tags"][0]["counts"]["media"], 0);
    }
}

#[tokio::test]
async fn nested_graphql_lists_leave_hidden_records_out() {
    let f = fixture().await;
    let t = &f.tree;
    // The excluded anchor's spouse: the family's events and media are
    // withheld, and the source's citations leave the anchor's out.
    let query = format!(
        r#"{{
            family(treeId: "{tree}", id: "{family}") {{ events {{ id }} }}
            sources(treeId: "{tree}", first: 100) {{ edges {{ node {{ citations {{ id }} repositories {{ id }} }} }} }}
            repositories(treeId: "{tree}", first: 100) {{ edges {{ node {{ sources {{ id }} }} }} }}
            events(treeId: "{tree}", first: 100) {{ edges {{ node {{ citations {{ id }} notes {{ id }} media {{ id }} place {{ id }} witnesses {{ personId }} }} }} }}
        }}"#,
        tree = t.tree_id,
        family = t.id("family_id"),
    );
    let owner = gql_ok(&f.owner, &query, json!({})).await.to_string();
    for id in ["citation_id", "source_link_id"] {
        assert!(owner.contains(t.id(id)), "the owner sees {id}");
    }
    for who in VIEWERS {
        let data = f.data(who, &query).await;
        assert_eq!(data["family"]["events"], json!([]), "{who:?}: {data}");
        let text = data.to_string();
        for id in [
            "citation_id",
            "source_link_id",
            "place_id",
            "media_id",
            "note_id",
        ] {
            assert!(!text.contains(t.id(id)), "{who:?} sees {id}: {text}");
        }
    }
}

/// The records of a hidden person outlive their soft deletion: an
/// excluded person the owner deleted keeps their events, notes and
/// citations hidden (docs/privacy.md §3.2), in lists and direct reads.
#[tokio::test]
async fn the_records_of_a_deleted_hidden_person_stay_hidden() {
    let f = fixture().await;
    let t = &f.tree;
    let (status, _) = crate::common::send(
        &f.owner,
        Method::DELETE,
        &f.path(&format!("/persons/{}", t.id("person_id"))),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    for who in VIEWERS {
        for path in [
            format!("/events/{}", t.id("event_id")),
            format!("/notes/{}", t.id("note_id")),
        ] {
            f.missing(who, &path).await;
        }
        for (path, id) in [
            ("/events?first=100", "event_id"),
            ("/citations?first=100", "citation_id"),
            ("/notes?first=100", "note_id"),
        ] {
            let page = f.get(who, path).await;
            assert!(
                !ids(&page).contains(&t.id(id).to_string()),
                "{who:?} {path}: {page}"
            );
        }
        let data = f
            .data(
                who,
                &format!(
                    r#"{{ notes(treeId: "{}", first: 100) {{ edges {{ node {{ id }} }} }} }}"#,
                    t.tree_id
                ),
            )
            .await;
        assert!(
            !data.to_string().contains(t.id("note_id")),
            "{who:?}: {data}"
        );
    }
}

/// A medium the viewer may not see — a scan showing the excluded anchor —
/// is no visible person's portrait, and their profile does not count it;
/// nor does it count a confidential note a Guest may not read
/// (docs/privacy.md §3.2), on REST and GraphQL alike.
#[tokio::test]
async fn a_withheld_medium_is_nobodys_portrait_and_is_not_counted() {
    let f = fixture().await;
    let t = &f.tree;
    let (document, page, visible) = (t.id("media_id"), t.id("page_id"), f.visible.as_str());
    ok(
        &f.owner,
        Method::POST,
        &f.path("/media-links"),
        Some(json!({ "media_id": document, "person_id": visible })),
    )
    .await;
    ok(
        &f.owner,
        Method::PUT,
        &f.path(&format!("/persons/{visible}/portrait")),
        Some(json!({ "media_id": page })),
    )
    .await;
    let owners = ok(
        &f.owner,
        Method::GET,
        &f.path(&format!("/profiles/{visible}")),
        None,
    )
    .await;
    assert_eq!(owners["primary_media"]["media_id"], page, "{owners}");
    assert_eq!(owners["media_count"], 1, "{owners}");
    for who in VIEWERS {
        let profile = f.get(who, &format!("/profiles/{visible}")).await;
        // A count of none is left out of the projection.
        let count = |name: &str| profile[name].as_u64().unwrap_or(0);
        assert!(profile["primary_media"].is_null(), "{who:?}: {profile}");
        assert_eq!(count("media_count"), 0, "{who:?}: {profile}");
        let notes = match who {
            Who::Guest => 0,
            Who::Editor => 1,
        };
        assert_eq!(count("note_count"), notes, "{who:?}: {profile}");
        let pedigree = f
            .get(
                who,
                &format!("/pedigree/{visible}?ancestor_depth=1&descendant_depth=1"),
            )
            .await;
        assert!(!pedigree.to_string().contains(page), "{who:?}: {pedigree}");
        let portraits = f.get(who, "/portraits").await;
        assert!(
            !portraits.to_string().contains(page),
            "{who:?}: {portraits}"
        );
        let (_, images) = f
            .rest(
                who,
                Method::POST,
                "/portrait-images",
                Some(json!({ "person_ids": [visible] })),
            )
            .await;
        assert_eq!(images, json!([]), "{who:?}");
        let data = f
            .data(
                who,
                &format!(
                    r#"{{ personProfile(treeId: "{tree}", personId: "{visible}") {{ primaryMedia {{ mediaId }} mediaCount noteCount }}
                        pedigree(treeId: "{tree}", rootPersonId: "{visible}", ancestorDepth: 1, descendantDepth: 1) {{ nodes {{ portrait {{ __typename }} primaryMediaPath }} }} }}"#,
                    tree = t.tree_id
                ),
            )
            .await;
        let profile = &data["personProfile"];
        assert!(profile["primaryMedia"].is_null(), "{who:?}: {data}");
        assert_eq!(profile["mediaCount"], 0, "{who:?}: {data}");
        assert_eq!(profile["noteCount"], notes, "{who:?}: {data}");
        assert!(
            data["pedigree"]["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .all(|node| node["portrait"].is_null() && node["primaryMediaPath"].is_null()),
            "{who:?}: {data}"
        );
    }
}
