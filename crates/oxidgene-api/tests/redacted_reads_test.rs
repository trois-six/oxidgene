//! What the tree-wide reads leave out for a restricted viewer
//! (docs/privacy.md §6.4, §7, §8): the dictionary, the value suggestions,
//! the history, the tools and the exports, on both surfaces.
//!
//! The fixture is the canary family, of which the owner excludes the
//! contemporary canary — with their occupation, place, note and a source
//! only they are cited by — and the excluded canary, their mother. An Editor
//! and a Guest, signed in through the mock provider, must meet none of it;
//! the owner, the `none` mode's local owner, sees everything as before.

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

use crate::common::canary::{CONTEMPORARY, CONTEMPORARY_PLACE, Canaries, EXCLUDED};
use crate::common::oidc::{MockProvider, TRUSTED_CLIENT};
use crate::common::{app_on, ok, setup_db};

/// A viewer of the fixture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Who {
    /// The local owner of the `none` mode.
    Owner,
    Editor,
    Guest,
}

/// The restricted viewers.
const RESTRICTED: [Who; 2] = [Who::Editor, Who::Guest];

/// A source only the excluded contemporary canary is cited by.
const HIDDEN_SOURCE: &str = "Canary almanac";
/// A source a visible person is cited by.
const VISIBLE_SOURCE: &str = "Wrenfield register";
/// The contemporary canary's occupation.
const HIDDEN_OCCUPATION: &str = "Lamplighter";

struct Fixture {
    local: axum::Router,
    app: axum::Router,
    _db: DatabaseConnection,
    tokens: BTreeMap<Who, String>,
    canaries: Canaries,
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

impl Fixture {
    async fn new() -> Self {
        let provider = MockProvider::start().await;
        let db = setup_db().await;
        let local = app_on(db.clone());
        let canaries = Canaries::new(&local, &db).await;
        let base = format!("/api/v1/trees/{}", canaries.tree);
        for given in ["Quillon", "Thistle"] {
            ok(
                &local,
                Method::POST,
                &format!("{base}/exclusions"),
                Some(json!({ "kind": "person", "person_id": canaries.person(given) })),
            )
            .await;
        }
        for (title, given) in [(HIDDEN_SOURCE, "Quillon"), (VISIBLE_SOURCE, "Ashby")] {
            let source = ok(
                &local,
                Method::POST,
                &format!("{base}/sources"),
                Some(json!({ "title": title })),
            )
            .await;
            ok(
                &local,
                Method::POST,
                &format!("{base}/citations"),
                Some(json!({ "source_id": source["id"], "person_id": canaries.person(given) })),
            )
            .await;
        }
        let (app, _) = provider.app(&db, &[]);
        let tree_id: Uuid = canaries.tree.parse().unwrap();
        let mut tokens = BTreeMap::new();
        for (who, role) in [
            (Who::Editor, TreeRole::Editor),
            (Who::Guest, TreeRole::Guest),
        ] {
            let subject = format!("{who:?}").to_lowercase();
            let id = user(&db, &provider, &subject).await;
            MembershipRepo::create(&db, tree_id, id, role, None)
                .await
                .expect("shares the tree");
            tokens.insert(who, provider.access_token(&subject, TRUSTED_CLIENT, 3600));
        }
        Self {
            local,
            app,
            _db: db,
            tokens,
            canaries,
            _provider: provider,
        }
    }

    fn uri(&self, path: &str) -> String {
        format!("/api/v1/trees/{}{path}", self.canaries.tree)
    }

    fn tree(&self) -> &str {
        &self.canaries.tree
    }

    /// `method` on `uri` as `who`: the status and the body as text.
    async fn raw(
        &self,
        who: Who,
        method: Method,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, axum::http::HeaderMap, String) {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, "genealogy.example.invalid")
            .header(header::CONTENT_TYPE, "application/json");
        let app = match who {
            Who::Owner => &self.local,
            _ => {
                request = request.header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", self.tokens[&who]),
                );
                &self.app
            }
        };
        let body = body.map_or_else(Body::empty, |json| Body::from(json.to_string()));
        let response = app
            .clone()
            .oneshot(request.body(body).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            headers,
            String::from_utf8_lossy(&bytes).into_owned(),
        )
    }

    /// `method` on `uri` as `who`: the status and the JSON body.
    async fn rest(
        &self,
        who: Who,
        method: Method,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let (status, _, text) = self.raw(who, method, uri, body).await;
        (status, serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    /// A successful `GET uri` as `who`, as text.
    async fn get(&self, who: Who, uri: &str) -> String {
        let (status, _, text) = self.raw(who, Method::GET, uri, None).await;
        assert!(status.is_success(), "{who:?} {uri}: {status} {text}");
        text
    }

    /// GraphQL `query` as `who`: the whole response.
    async fn gql_raw(&self, who: Who, query: &str) -> Value {
        let (_, _, text) = self
            .raw(
                who,
                Method::POST,
                "/graphql",
                Some(json!({ "query": query })),
            )
            .await;
        serde_json::from_str(&text).unwrap()
    }

    /// GraphQL `query` as `who`: its `data`, which must hold no error.
    async fn gql(&self, who: Who, query: &str) -> Value {
        let (_, _, text) = self
            .raw(
                who,
                Method::POST,
                "/graphql",
                Some(json!({ "query": query })),
            )
            .await;
        let response: Value = serde_json::from_str(&text).unwrap();
        assert!(
            response.get("errors").is_none(),
            "{who:?} {query}: {response}"
        );
        response["data"].clone()
    }
}

/// Whether `text` names anything of the hidden canaries.
fn names_hidden(text: &str) -> bool {
    [
        CONTEMPORARY,
        EXCLUDED,
        CONTEMPORARY_PLACE,
        HIDDEN_SOURCE,
        HIDDEN_OCCUPATION,
    ]
    .iter()
    .any(|canary| text.contains(canary))
}

#[tokio::test]
async fn the_dictionary_lists_only_what_visible_records_carry() {
    let f = Fixture::new().await;
    let reads = [
        "/dictionary/family-names",
        "/dictionary/occupations",
        "/dictionary/places",
        "/dictionary/sources",
        "/dictionary/sources/groups",
    ];
    for path in reads {
        let owner = f.get(Who::Owner, &f.uri(path)).await;
        assert!(
            names_hidden(&owner),
            "the owner sees everything: {path} {owner}"
        );
        for who in RESTRICTED {
            let seen = f.get(who, &f.uri(path)).await;
            assert!(!names_hidden(&seen), "{who:?} {path}: {seen}");
        }
    }
    for who in RESTRICTED {
        let names: Value =
            serde_json::from_str(&f.get(who, &f.uri("/dictionary/family-names")).await).unwrap();
        let wrenfield = names
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["value"] == "Wrenfield")
            .expect("a visible family name stays");
        // Ashby, Mabel, Corwin, Edda, Harlan.
        assert_eq!(wrenfield["count"], 5, "{who:?}");
        let sources: Value =
            serde_json::from_str(&f.get(who, &f.uri("/dictionary/sources")).await).unwrap();
        let register = sources
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["title"] == VISIBLE_SOURCE)
            .expect("a visibly cited source stays");
        assert_eq!(register["count"], 1);
    }

    let query = format!(
        r#"{{ dictionaryFamilyNames(treeId: "{t}") {{ value count }}
             dictionaryOccupations(treeId: "{t}") {{ value }}
             dictionaryPlaces(treeId: "{t}") {{ place {{ name }} count }}
             dictionarySources(treeId: "{t}") {{ source {{ title }} count }}
             dictionarySourceDrill(treeId: "{t}") {{ total sources {{ source {{ title }} }} }} }}"#,
        t = f.tree()
    );
    assert!(names_hidden(&f.gql(Who::Owner, &query).await.to_string()));
    for who in RESTRICTED {
        let seen = f.gql(who, &query).await.to_string();
        assert!(!names_hidden(&seen), "{who:?}: {seen}");
        assert!(seen.contains(VISIBLE_SOURCE), "{who:?}: {seen}");
    }
}

#[tokio::test]
async fn usage_lists_and_the_descent_leave_hidden_persons_out() {
    let f = Fixture::new().await;
    let places: Value =
        serde_json::from_str(&f.get(Who::Owner, &f.uri("/dictionary/places")).await).unwrap();
    let place = places
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| {
            entry["name"]
                .as_str()
                .unwrap()
                .starts_with(CONTEMPORARY_PLACE)
        })
        .expect("the owner sees the canary's place")["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let sources: Value =
        serde_json::from_str(&f.get(Who::Owner, &f.uri("/dictionary/sources")).await).unwrap();
    let source = sources
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["title"] == HIDDEN_SOURCE)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let lists = [
        format!("/dictionary/family-names/usage?value={CONTEMPORARY}"),
        format!("/dictionary/family-names/usage?value={EXCLUDED}"),
        format!("/dictionary/occupations/usage?value={HIDDEN_OCCUPATION}"),
        format!("/dictionary/places/{place}/usage"),
        format!("/dictionary/sources/{source}/usage"),
    ];
    for path in &lists {
        let owner: Value = serde_json::from_str(&f.get(Who::Owner, &f.uri(path)).await).unwrap();
        assert_eq!(owner.as_array().unwrap().len(), 1, "{path}: {owner}");
        for who in RESTRICTED {
            let seen: Value = serde_json::from_str(&f.get(who, &f.uri(path)).await).unwrap();
            assert_eq!(seen, json!([]), "{who:?} {path}");
        }
    }
    // The Wrenfield descent: Harlan's union with the excluded canary, and
    // their contemporary son, are gone.
    let descent = f.uri("/dictionary/family-names/descent?value=Wrenfield");
    assert!(names_hidden(&f.get(Who::Owner, &descent).await));
    for who in RESTRICTED {
        let seen = f.get(who, &descent).await;
        assert!(!names_hidden(&seen), "{who:?}: {seen}");
        assert!(
            !seen.contains(f.canaries.person("Thistle")),
            "{who:?}: {seen}"
        );
        assert!(
            seen.contains(f.canaries.person("Harlan")),
            "{who:?}: {seen}"
        );
    }

    let query = format!(
        r#"{{ familyNameUsage(treeId: "{t}", value: "{CONTEMPORARY}") {{ personId }}
             occupationUsage(treeId: "{t}", value: "{HIDDEN_OCCUPATION}") {{ personId }}
             placeUsage(treeId: "{t}", placeId: "{place}") {{ personId }}
             sourceUsage(treeId: "{t}", sourceId: "{source}") {{ personId }}
             familyNameDescent(treeId: "{t}", value: "Wrenfield") {{ branches {{ lines {{ person {{ personId surname }} spouse {{ personId surname }} }} }} }} }}"#,
        t = f.tree()
    );
    let owner = f.gql(Who::Owner, &query).await;
    assert_eq!(owner["sourceUsage"].as_array().unwrap().len(), 1);
    assert!(names_hidden(&owner.to_string()), "{owner}");
    for who in RESTRICTED {
        let seen = f.gql(who, &query).await;
        for list in [
            "familyNameUsage",
            "occupationUsage",
            "placeUsage",
            "sourceUsage",
        ] {
            assert_eq!(seen[list], json!([]), "{who:?} {list}");
        }
        assert!(!names_hidden(&seen.to_string()), "{who:?}: {seen}");
    }
}

#[tokio::test]
async fn suggestions_offer_only_what_visible_records_carry() {
    let f = Fixture::new().await;
    let fields = [
        ("family-names", "FAMILY_NAMES", "canary", CONTEMPORARY),
        ("occupations", "OCCUPATIONS", "lamp", HIDDEN_OCCUPATION),
        ("places", "PLACES", "quill", CONTEMPORARY_PLACE),
        ("sources", "SOURCES", "canary", HIDDEN_SOURCE),
        ("given-names", "GIVEN_NAMES", "quill", "Quillon"),
    ];
    // A value the tree holds counts its uses; one only a reference sheet
    // knows counts none, and may stay.
    let held = |list: &Value, hidden: &str| {
        list.as_array().unwrap().iter().any(|suggestion| {
            suggestion["value"].as_str().unwrap().contains(hidden)
                && suggestion["count"].as_i64().unwrap() > 0
        })
    };
    for (path, field, query, hidden) in fields {
        let uri = f.uri(&format!("/suggestions/{path}?q={query}&lang=en"));
        let owner: Value = serde_json::from_str(&f.get(Who::Owner, &uri).await).unwrap();
        assert!(held(&owner, hidden), "{path}: {owner}");
        let gql = format!(
            r#"{{ valueSuggestions(treeId: "{}", field: {field}, language: "en", query: "{query}") {{ value count }} }}"#,
            f.tree()
        );
        assert!(held(
            &f.gql(Who::Owner, &gql).await["valueSuggestions"],
            hidden
        ));
        for who in RESTRICTED {
            let seen: Value = serde_json::from_str(&f.get(who, &uri).await).unwrap();
            assert!(!held(&seen, hidden), "{who:?} {path}: {seen}");
            let seen = f.gql(who, &gql).await;
            assert!(
                !held(&seen["valueSuggestions"], hidden),
                "{who:?} {field}: {seen}"
            );
        }
    }
    // A scoped name suggestion counts only the persons the viewer sees.
    let scoped = f.uri("/suggestions/given-names?q=t&lang=en&surname=Canaryexcluded");
    assert!(f.get(Who::Owner, &scoped).await.contains("Thistle"));
    for who in RESTRICTED {
        assert!(!f.get(who, &scoped).await.contains("Thistle"), "{who:?}");
    }
}

/// The ids and labels of the audit log `who` reads, through REST and
/// GraphQL, which must agree.
async fn audit_log(f: &Fixture, who: Who) -> (u64, String) {
    let rest = f.get(who, &f.uri("/audit?first=200")).await;
    let page: Value = serde_json::from_str(&rest).unwrap();
    let total = page["total_count"].as_u64().unwrap();
    let gql = f
        .gql(
            who,
            &format!(
                r#"{{ auditEntries(treeId: "{}", first: 200) {{ totalCount edges {{ node {{ id label subjectId }} }} }} }}"#,
                f.tree()
            ),
        )
        .await;
    assert_eq!(gql["auditEntries"]["totalCount"], total, "{who:?}");
    assert_eq!(
        gql["auditEntries"]["edges"].as_array().unwrap().len(),
        page["edges"].as_array().unwrap().len(),
        "{who:?}"
    );
    (total, format!("{rest}{gql}"))
}

#[tokio::test]
async fn the_history_leaves_out_what_is_about_hidden_persons() {
    let f = Fixture::new().await;
    let thistle = f.canaries.person("Thistle").to_owned();
    let quillon = f.canaries.person("Quillon").to_owned();
    let harlan = f.canaries.person("Harlan").to_owned();

    // The owner's log holds the exclusions and the citation of the hidden
    // canaries, and the creation of the source only they are cited by.
    let (all, owner) = audit_log(&f, Who::Owner).await;
    assert!(
        owner.contains(&thistle) && owner.contains(HIDDEN_SOURCE),
        "{owner}"
    );
    let (seen, editor) = audit_log(&f, Who::Editor).await;
    assert!(seen < all, "{seen} of {all}");
    for hidden in [&thistle, &quillon] {
        assert!(!editor.contains(hidden.as_str()), "{editor}");
    }
    assert!(!names_hidden(&editor), "{editor}");

    // An entry about the excluded canary, with its changes, is not found.
    let log: Value =
        serde_json::from_str(&f.get(Who::Owner, &f.uri("/audit?first=200")).await).unwrap();
    let about_thistle = log["edges"]
        .as_array()
        .unwrap()
        .iter()
        .find(|edge| edge["node"]["subject_id"] == thistle.as_str())
        .expect("the exclusion's entry")["node"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    for path in [
        format!("/audit/{about_thistle}"),
        format!("/audit/{about_thistle}/changes"),
        format!("/history/person/{quillon}"),
        format!("/history/person/{quillon}/1"),
    ] {
        let (status, _) = f.rest(Who::Editor, Method::GET, &f.uri(&path), None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
        let (status, _) = f.rest(Who::Owner, Method::GET, &f.uri(&path), None).await;
        assert_eq!(status, StatusCode::OK, "{path}");
    }
    for query in [
        format!(
            r#"{{ auditEntry(treeId: "{}", id: "{about_thistle}") {{ id }} }}"#,
            f.tree()
        ),
        format!(
            r#"{{ auditEntryChanges(treeId: "{}", entryId: "{about_thistle}") {{ totalCount }} }}"#,
            f.tree()
        ),
        format!(
            r#"{{ recordVersions(treeId: "{}", recordType: PERSON, recordId: "{quillon}") {{ totalCount }} }}"#,
            f.tree()
        ),
        format!(
            r#"{{ recordVersion(treeId: "{}", recordType: PERSON, recordId: "{quillon}", version: 1) {{ id }} }}"#,
            f.tree()
        ),
    ] {
        let response = f.gql_raw(Who::Editor, &query).await;
        assert_eq!(
            response["errors"][0]["extensions"]["code"], "NOT_FOUND",
            "{query}: {response}"
        );
        assert!(
            f.gql_raw(Who::Owner, &query).await.get("errors").is_none(),
            "{query}"
        );
    }
    // Restoring a hidden person's version is not found either.
    let (status, _) = f
        .rest(
            Who::Editor,
            Method::POST,
            &f.uri(&format!("/history/person/{quillon}/revert")),
            Some(json!({ "version": 1 })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let response = f
        .gql_raw(
            Who::Editor,
            &format!(
                r#"mutation {{ revertRecord(treeId: "{}", recordType: PERSON, recordId: "{quillon}", version: 1) {{ id }} }}"#,
                f.tree()
            ),
        )
        .await;
    assert_eq!(
        response["errors"][0]["extensions"]["code"], "NOT_FOUND",
        "{response}"
    );

    // A visible person's version names their hidden wife and son by no
    // identifier of theirs and no label.
    let path = f.uri(&format!("/history/person/{harlan}"));
    let owner = f.get(Who::Owner, &path).await;
    assert!(
        owner.contains(&thistle) && owner.contains(&quillon),
        "{owner}"
    );
    let editor = f.get(Who::Editor, &path).await;
    assert!(
        !editor.contains(&thistle) && !editor.contains(&quillon),
        "{editor}"
    );
    assert!(!names_hidden(&editor), "{editor}");
    let query = format!(
        r#"{{ recordVersions(treeId: "{}", recordType: PERSON, recordId: "{harlan}") {{ edges {{ node {{ labels {{ id label }} snapshot {{ person {{ unions {{ spouses {{ personId }} children {{ personId }} }} }} }} }} }} }} }}"#,
        f.tree()
    );
    assert!(
        f.gql(Who::Owner, &query)
            .await
            .to_string()
            .contains(&thistle)
    );
    let seen = f.gql(Who::Editor, &query).await.to_string();
    assert!(
        !seen.contains(&thistle) && !seen.contains(&quillon),
        "{seen}"
    );
    assert!(!names_hidden(&seen), "{seen}");
}

#[tokio::test]
async fn an_editors_tools_leave_out_hidden_persons() {
    let f = Fixture::new().await;
    let thistle = f.canaries.person("Thistle").to_owned();
    let quillon = f.canaries.person("Quillon").to_owned();
    // An occupation after death for the excluded canary and for a visible
    // ancestor: an anomaly naming each.
    for (given, year) in [("Thistle", "2010"), ("Ashby", "1950")] {
        ok(
            &f.local,
            Method::POST,
            &f.uri("/events"),
            Some(json!({
                "event_type": "occupation",
                "date_value": year,
                "person_id": f.canaries.person(given),
            })),
        )
        .await;
    }
    // A visible namesake of the excluded canary: a potential duplicate.
    let namesake = ok(
        &f.local,
        Method::POST,
        &f.uri("/persons"),
        Some(json!({ "sex": "female" })),
    )
    .await;
    let namesake = namesake["id"].as_str().unwrap().to_owned();
    ok(
        &f.local,
        Method::POST,
        &f.uri(&format!("/persons/{namesake}/names")),
        Some(json!({
            "name_type": "birth",
            "given_names": "Thistle",
            "surname": "Wrenfield",
            "is_primary": true,
        })),
    )
    .await;
    // The contemporary canary as the SOSA root.
    ok(
        &f.local,
        Method::PUT,
        &f.uri(""),
        Some(json!({ "sosa_root_person_id": quillon })),
    )
    .await;

    let anomalies = f.get(Who::Owner, &f.uri("/anomalies")).await;
    assert!(anomalies.contains(&thistle), "{anomalies}");
    let seen = f.get(Who::Editor, &f.uri("/anomalies")).await;
    assert!(!seen.contains(&thistle), "{seen}");
    assert!(
        seen.contains(f.canaries.person("Ashby")),
        "a visible anomaly stays: {seen}"
    );
    let ancestry = f.get(Who::Owner, &f.uri("/ancestry-completeness")).await;
    assert!(ancestry.contains(&quillon), "{ancestry}");
    let seen = f.get(Who::Editor, &f.uri("/ancestry-completeness")).await;
    assert!(!seen.contains(&quillon) && !names_hidden(&seen), "{seen}");
    for path in [
        "/anomalies",
        "/duplicates",
        "/unlocated-places",
        "/ancestry-completeness",
    ] {
        let seen = f.get(Who::Editor, &f.uri(path)).await;
        assert!(
            !seen.contains(&thistle) && !seen.contains(&quillon),
            "{path}: {seen}"
        );
        assert!(!names_hidden(&seen), "{path}: {seen}");
    }

    let query = format!(
        r#"{{ treeAnomalies(treeId: "{t}") {{ persons rules {{ rule count items {{ persons {{ personId name }} }} }} }}
             potentialDuplicates(treeId: "{t}") {{ count pairs {{ first {{ personId }} second {{ personId }} }} }}
             unlocatedPlaces(treeId: "{t}") {{ placeId name count }}
             ancestryCompleteness(treeId: "{t}") {{ root {{ personId name }} }} }}"#,
        t = f.tree()
    );
    let owner = f.gql(Who::Owner, &query).await;
    assert!(owner.to_string().contains(&thistle), "{owner}");
    assert_eq!(
        owner["ancestryCompleteness"]["root"]["personId"],
        quillon.as_str()
    );
    let seen = f.gql(Who::Editor, &query).await;
    let text = seen.to_string();
    assert!(
        !text.contains(&thistle) && !text.contains(&quillon),
        "{text}"
    );
    assert!(!names_hidden(&text), "{text}");
    assert_eq!(seen["ancestryCompleteness"]["root"], Value::Null);
    assert!(
        seen["treeAnomalies"]["persons"].as_i64() < owner["treeAnomalies"]["persons"].as_i64(),
        "the persons examined are those the Editor sees"
    );
}
