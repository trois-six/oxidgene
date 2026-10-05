//! A source's archive target, through REST and GraphQL alike.
//!
//! The portal is the recorded one of the Arkothèque adapter tests
//! (`oxidgene-archives/fixtures/arkotheque/`), served by a stub transport:
//! no test contacts an archive portal. Citations are fictitious.

#![cfg(feature = "graphql")]

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::http::{Method, StatusCode};
use oxidgene_api::service::archive::ArchivePortals;
use oxidgene_api::{AppState, build_router};
use oxidgene_archives::platform::BoxFuture;
use oxidgene_archives::{FetchError, PortalEndpoint, PortalFetch, PortalRequest, PortalTransport};
use serde_json::{Value, json};

use common::{gql, gql_error_code, new_person, new_tree, ok, send, setup_db, test_media_root};

const AD44_ONE: &str = include_str!("../../oxidgene-archives/fixtures/arkotheque/ad44-one.json");
const AD44_VIEWER: &str =
    include_str!("../../oxidgene-archives/fixtures/arkotheque/ad44-viewer.json");
const AD44_INFO: &str = include_str!("../../oxidgene-archives/fixtures/arkotheque/ad44-info.json");

/// A Loire-Atlantique register, a portal any client reaches; the citation's
/// page names the act and the view.
const AD44_REGISTER: &str = "AD44 - Exampleville - Saint-Exemple - B - 1660 - E dépôt 99";
const AD44_PAGE: &str = "acte 4 - vue 2g/3";
/// A Sarthe register, a portal only a browser page reaches.
const AD72_REGISTER: &str = "AD72 - Exampleville - (aucun) - N - 1877";

/// How the recorded portal answers.
#[derive(Clone)]
enum Answer {
    /// The search answers this body; the viewer and the image service their
    /// recorded answers.
    Search(&'static str),
    /// Every request fails so.
    Fails(FetchError),
    /// The connection itself fails so.
    Unconnected(FetchError),
}

/// A recorded portal, counting the connections opened on it.
struct Recorded {
    answer: Answer,
    connections: AtomicUsize,
}

impl Recorded {
    fn new(answer: Answer) -> Arc<Self> {
        Arc::new(Self {
            answer,
            connections: AtomicUsize::new(0),
        })
    }

    fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }
}

impl PortalTransport for Recorded {
    fn is_browser(&self) -> bool {
        false
    }

    fn connect<'a>(
        &'a self,
        _endpoint: &'a PortalEndpoint,
    ) -> BoxFuture<'a, Result<Box<dyn PortalFetch + 'a>, FetchError>> {
        self.connections.fetch_add(1, Ordering::SeqCst);
        let answer = self.answer.clone();
        Box::pin(async move {
            match answer {
                Answer::Unconnected(error) => Err(error),
                answer => Ok(Box::new(Fetch(answer)) as Box<dyn PortalFetch>),
            }
        })
    }
}

struct Fetch(Answer);

impl PortalFetch for Fetch {
    fn request<'a>(
        &'a self,
        request: &'a PortalRequest,
    ) -> BoxFuture<'a, Result<String, FetchError>> {
        Box::pin(async move {
            let search = match &self.0 {
                Answer::Search(search) => *search,
                Answer::Fails(error) | Answer::Unconnected(error) => return Err(error.clone()),
            };
            let url = request.url.as_str();
            let body = if url.starts_with("/_recherche-api/moteur?") {
                search
            } else if url.starts_with("/_recherche-api/visionneuse-infos/") {
                AD44_VIEWER
            } else if url.starts_with("/_recherche-images/") && url.ends_with("/info.json") {
                AD44_INFO
            } else {
                return Err(FetchError::Status(404));
            };
            Ok(body.to_owned())
        })
    }
}

/// A router over a fresh database whose archive portals are `portal`.
async fn app_with(portal: &Arc<Recorded>) -> Router {
    let state = AppState::new(setup_db().await, test_media_root()).with_archive_portals(
        ArchivePortals::with_transport(Arc::clone(portal) as Arc<dyn PortalTransport>),
    );
    build_router(state)
}

async fn new_source(app: &Router, tree: &str, title: &str) -> String {
    ok(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/sources"),
        Some(json!({ "title": title })),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn new_citation(app: &Router, tree: &str, source: &str, page: &str) -> String {
    let person = new_person(app, tree).await;
    ok(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/citations"),
        Some(json!({ "source_id": source, "person_id": person, "page": page })),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// The REST answer for `source` of `tree`, cited by `citation` when given.
async fn rest(
    app: &Router,
    tree: &str,
    source: &str,
    citation: Option<&str>,
) -> (StatusCode, Value) {
    let body = match citation {
        Some(citation) => json!({ "citation_id": citation }),
        None => json!({}),
    };
    send(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/sources/{source}/archive-target"),
        Some(body),
    )
    .await
}

const SOURCE_TARGET: &str = r#"
    query($tree: ID!, $id: ID!, $citation: ID) {
        source(treeId: $tree, id: $id) {
            archiveTarget(citationId: $citation) {
                kind url matches viewCount callNumber attribution
                views { view url ark image { picture } }
            }
        }
    }"#;

/// The GraphQL answer for `source` of `tree`, cited by `citation` when given.
async fn graphql(app: &Router, tree: &str, source: &str, citation: Option<&str>) -> Value {
    gql(
        app,
        SOURCE_TARGET,
        json!({ "tree": tree, "id": source, "citation": citation }),
    )
    .await
}

#[tokio::test]
async fn a_cited_register_resolves_to_its_view_on_both_surfaces() {
    let portal = Recorded::new(Answer::Search(AD44_ONE));
    let app = app_with(&portal).await;
    let tree = new_tree(&app, "Archives").await;
    let source = new_source(&app, &tree, AD44_REGISTER).await;
    let citation = new_citation(&app, &tree, &source, AD44_PAGE).await;

    let (status, target) = rest(&app, &tree, &source, Some(&citation)).await;
    assert_eq!(status, StatusCode::OK, "{target}");
    assert_eq!(target["kind"], "view");
    assert_eq!(target["call_number"], "E dépôt 99");
    assert_eq!(target["view_count"], 3);
    assert_eq!(target["views"][0]["view"], 2);
    let url = target["url"].as_str().unwrap();
    assert!(
        url.starts_with("https://archives-numerisees.loire-atlantique.fr/")
            && url.contains("detail=arko_fiche_0000000000a01"),
        "{url}"
    );
    assert_eq!(portal.connections(), 1);

    let response = graphql(&app, &tree, &source, Some(&citation)).await;
    assert!(response.get("errors").is_none(), "{response}");
    let gql_target = &response["data"]["source"]["archiveTarget"];
    assert_eq!(gql_target["kind"], "VIEW");
    assert_eq!(gql_target["url"], target["url"]);
    assert_eq!(gql_target["callNumber"], "E dépôt 99");
    assert_eq!(gql_target["viewCount"], 3);
    assert_eq!(gql_target["views"][0]["view"], 2);
    assert_eq!(gql_target["views"][0]["url"], target["views"][0]["url"]);
    assert_eq!(gql_target["matches"], Value::Null);
    // Both surfaces share the process's resolver: the second answer came
    // from its session cache, without a request.
    assert_eq!(portal.connections(), 1);
}

#[tokio::test]
async fn a_browser_portal_yields_its_offline_results() {
    let portal = Recorded::new(Answer::Search(AD44_ONE));
    let app = app_with(&portal).await;
    let tree = new_tree(&app, "Archives").await;
    let source = new_source(&app, &tree, AD72_REGISTER).await;

    let (status, target) = rest(&app, &tree, &source, None).await;
    assert_eq!(status, StatusCode::OK, "{target}");
    assert_eq!(target["kind"], "results");
    assert_eq!(target["matches"], Value::Null);
    assert!(
        target["url"]
            .as_str()
            .unwrap()
            .starts_with("https://archives.sarthe.fr/"),
        "{target}"
    );

    let response = graphql(&app, &tree, &source, None).await;
    let gql_target = &response["data"]["source"]["archiveTarget"];
    assert_eq!(gql_target["kind"], "RESULTS", "{response}");
    assert_eq!(gql_target["url"], target["url"]);
    assert_eq!(gql_target["matches"], Value::Null);
    assert_eq!(gql_target["views"], Value::Null);
    assert_eq!(portal.connections(), 0, "the portal needs a browser");
}

#[tokio::test]
async fn a_title_that_is_no_catalogued_citation_has_no_target() {
    let portal = Recorded::new(Answer::Search(AD44_ONE));
    let app = app_with(&portal).await;
    let tree = new_tree(&app, "Archives").await;

    for (title, code) in [
        ("Fictitious register", "not_an_archive_citation"),
        // A well-formed citation of an archive the catalogue does not list.
        ("AD09 - Exampleville - (aucun) - N - 1877", "no_adapter"),
        // A catalogued archive, but a table no collection holds.
        ("AD44 - Exampleville - (aucun) - TD - 1877", "no_adapter"),
    ] {
        let source = new_source(&app, &tree, title).await;
        let (status, body) = rest(&app, &tree, &source, None).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{title}: {body}");
        assert_eq!(body["error"], code, "{title}");
        assert!(body.get("request_id").is_none());

        let response = graphql(&app, &tree, &source, None).await;
        assert_eq!(
            gql_error_code(&response),
            code.to_ascii_uppercase(),
            "{title}"
        );
        assert_eq!(response["data"]["source"]["archiveTarget"], Value::Null);
    }
    assert_eq!(portal.connections(), 0);
}

#[tokio::test]
async fn unknown_or_foreign_records_are_not_found() {
    let portal = Recorded::new(Answer::Search(AD44_ONE));
    let app = app_with(&portal).await;
    let tree_a = new_tree(&app, "Tree A").await;
    let tree_b = new_tree(&app, "Tree B").await;
    let source_a = new_source(&app, &tree_a, AD44_REGISTER).await;
    let other_a = new_source(&app, &tree_a, AD44_REGISTER).await;
    let citation_of_other = new_citation(&app, &tree_a, &other_a, AD44_PAGE).await;
    let source_b = new_source(&app, &tree_b, AD44_REGISTER).await;
    let citation_b = new_citation(&app, &tree_b, &source_b, AD44_PAGE).await;
    let unknown = "01900000-0000-7000-8000-000000000000";

    // An unknown source, and another tree's: REST answers 404, GraphQL's
    // `source` is absent.
    for source in [unknown, source_b.as_str()] {
        let (status, body) = rest(&app, &tree_a, source, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
        assert_eq!(body["error"], "not_found");
        let response = graphql(&app, &tree_a, source, None).await;
        assert!(response.get("errors").is_none(), "{response}");
        assert_eq!(response["data"]["source"], Value::Null);
    }

    // An unknown citation, another tree's, and one of another source.
    for citation in [unknown, citation_b.as_str(), citation_of_other.as_str()] {
        let (status, body) = rest(&app, &tree_a, &source_a, Some(citation)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
        assert_eq!(body["error"], "not_found");
        let response = graphql(&app, &tree_a, &source_a, Some(citation)).await;
        assert_eq!(gql_error_code(&response), "NOT_FOUND");
    }

    // A malformed citation identifier.
    let (status, body) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_a}/sources/{source_a}/archive-target"),
        Some(json!({ "citation_id": "not-an-id" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "validation_error");
    let response = graphql(&app, &tree_a, &source_a, Some("not-an-id")).await;
    assert_eq!(gql_error_code(&response), "VALIDATION_ERROR");

    assert_eq!(portal.connections(), 0);
}

#[tokio::test]
async fn portal_failures_carry_their_code() {
    for (answer, status, code) in [
        (
            Answer::Unconnected(FetchError::Timeout),
            StatusCode::GATEWAY_TIMEOUT,
            "timeout",
        ),
        (
            Answer::Fails(FetchError::Status(503)),
            StatusCode::BAD_GATEWAY,
            "unreachable",
        ),
        (
            Answer::Search("{\"unexpected\": true}"),
            StatusCode::BAD_GATEWAY,
            "unexpected_response",
        ),
        (
            Answer::Search("<script>window.location.href='/redirect_0000/chercher'</script>"),
            StatusCode::BAD_GATEWAY,
            "challenged",
        ),
    ] {
        let portal = Recorded::new(answer);
        let app = app_with(&portal).await;
        let tree = new_tree(&app, "Archives").await;
        let source = new_source(&app, &tree, AD44_REGISTER).await;

        let (got, body) = rest(&app, &tree, &source, None).await;
        assert_eq!(got, status, "{code}: {body}");
        assert_eq!(body["error"], code);
        assert!(body.get("request_id").is_none());

        // Failures are not cached: GraphQL asks the portal again.
        let response = graphql(&app, &tree, &source, None).await;
        assert_eq!(gql_error_code(&response), code.to_ascii_uppercase());
        assert_eq!(portal.connections(), 2, "{code}");
    }
}

#[tokio::test]
async fn graphql_resolves_no_target_for_the_items_of_a_list() {
    let portal = Recorded::new(Answer::Search(AD44_ONE));
    let app = app_with(&portal).await;
    let tree = new_tree(&app, "Archives").await;
    new_source(&app, &tree, AD44_REGISTER).await;

    let response = gql(
        &app,
        r#"query($tree: ID!) {
            sources(treeId: $tree) { edges { node { archiveTarget { url } } } }
        }"#,
        json!({ "tree": tree }),
    )
    .await;
    assert_eq!(gql_error_code(&response), "VALIDATION_ERROR");
    assert_eq!(portal.connections(), 0);
}

const SOURCE_VIEW: &str = r#"
    query($tree: ID!, $id: ID!, $citation: ID, $view: Int) {
        source(treeId: $tree, id: $id) {
            archiveTarget(citationId: $citation, view: $view) {
                kind url viewCount attribution views { view url }
            }
        }
    }"#;

#[tokio::test]
async fn a_neighbouring_view_resolves_on_both_surfaces() {
    let portal = Recorded::new(Answer::Search(AD44_ONE));
    let app = app_with(&portal).await;
    let tree = new_tree(&app, "Archives").await;
    let source = new_source(&app, &tree, AD44_REGISTER).await;
    let citation = new_citation(&app, &tree, &source, AD44_PAGE).await;
    let path = format!("/api/v1/trees/{tree}/sources/{source}/archive-target");

    // The cited view is 2 of 3: the reader pages to 3, then back to 1.
    for (view, connections) in [(3, 1), (1, 2)] {
        let (status, target) = send(
            &app,
            Method::POST,
            &path,
            Some(json!({ "citation_id": citation, "view": view })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{target}");
        assert_eq!(target["kind"], "view");
        assert_eq!(target["views"].as_array().unwrap().len(), 1);
        assert_eq!(target["views"][0]["view"], view);
        // One resolution per click.
        assert_eq!(portal.connections(), connections);

        let response = gql(
            &app,
            SOURCE_VIEW,
            json!({ "tree": tree, "id": source, "citation": citation, "view": view }),
        )
        .await;
        assert!(response.get("errors").is_none(), "{response}");
        let gql_target = &response["data"]["source"]["archiveTarget"];
        assert_eq!(gql_target["views"][0]["view"], view);
        assert_eq!(gql_target["views"][0]["url"], target["views"][0]["url"]);
        // The same view again comes from the session cache.
        assert_eq!(portal.connections(), connections);
    }

    // No view before the first, nor beyond the cited count.
    for view in [0, 4] {
        let (status, body) = send(
            &app,
            Method::POST,
            &path,
            Some(json!({ "citation_id": citation, "view": view })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{view}: {body}");
        assert_eq!(body["error"], "validation_error");
        let response = gql(
            &app,
            SOURCE_VIEW,
            json!({ "tree": tree, "id": source, "citation": citation, "view": view }),
        )
        .await;
        assert_eq!(gql_error_code(&response), "VALIDATION_ERROR", "{view}");
    }
    assert_eq!(portal.connections(), 2);
}

/// A citation of `source` attached to a new baptism in 1660 at `place`.
async fn new_event_citation(
    app: &Router,
    tree: &str,
    source: &str,
    place: &str,
    page: &str,
) -> String {
    let person = new_person(app, tree).await;
    let place = ok(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/places"),
        Some(json!({ "name": place })),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let event = ok(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/events"),
        Some(json!({
            "event_type": "baptism",
            "date_value": "1660",
            "person_id": person,
            "place_id": place,
        })),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    ok(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/citations"),
        Some(json!({ "source_id": source, "event_id": event, "page": page })),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// Holds `source` at a new repository named `name`, under `call_number`.
async fn hold_at(app: &Router, tree: &str, source: &str, name: &str, call_number: &str) {
    let repository = ok(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/repositories"),
        Some(json!({ "name": name })),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    ok(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/sources/{source}/repositories"),
        Some(json!({ "repository_id": repository, "call_number": call_number })),
    )
    .await;
}

const SOURCE_PARTS: &str = r#"
    query($tree: ID!, $id: ID!, $citation: ID, $parts: ArchivePartsInput) {
        source(treeId: $tree, id: $id) {
            archiveTarget(citationId: $citation, parts: $parts) {
                kind url matches callNumber views { view url }
            }
        }
    }"#;

/// Both surfaces' answers for `source`, with the reader's `parts` when
/// given; GraphQL's answer is its data or its error code.
async fn both(
    app: &Router,
    tree: &str,
    source: &str,
    citation: Option<&str>,
    parts: Option<Value>,
) -> ((StatusCode, Value), Value) {
    let rest = send(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/sources/{source}/archive-target"),
        Some(json!({ "citation_id": citation, "parts": parts })),
    )
    .await;
    let gql_parts = parts.map(|parts| {
        json!({
            "locality": parts.get("locality"),
            "act": parts.get("act"),
            "year": parts.get("year"),
            "view": parts.get("view"),
        })
    });
    let response = gql(
        app,
        SOURCE_PARTS,
        json!({ "tree": tree, "id": source, "citation": citation, "parts": gql_parts }),
    )
    .await;
    (rest, response)
}

#[tokio::test]
async fn structured_records_and_the_cited_event_resolve_on_both_surfaces() {
    let portal = Recorded::new(Answer::Search(AD44_ONE));
    let app = app_with(&portal).await;
    let tree = new_tree(&app, "Archives").await;
    // The register as the source, the archive and its call number on the
    // repository link, the act and the view in the page, the kind, year
    // and place on the cited event.
    let source = new_source(&app, &tree, "Registres paroissiaux et d'état civil").await;
    hold_at(
        &app,
        &tree,
        &source,
        "Archives départementales de Loire-Atlantique",
        "E dépôt 99",
    )
    .await;
    let citation = new_event_citation(
        &app,
        &tree,
        &source,
        "Exampleville, Loire-Atlantique, France",
        "acte 4, vue 2g/3",
    )
    .await;

    let ((status, target), response) = both(&app, &tree, &source, Some(&citation), None).await;
    assert_eq!(status, StatusCode::OK, "{target}");
    assert_eq!(target["kind"], "view");
    assert_eq!(target["call_number"], "E dépôt 99");
    assert_eq!(target["views"][0]["view"], 2);
    assert!(response.get("errors").is_none(), "{response}");
    let gql_target = &response["data"]["source"]["archiveTarget"];
    assert_eq!(gql_target["kind"], "VIEW");
    assert_eq!(gql_target["url"], target["url"]);
    assert_eq!(portal.connections(), 1);
}

#[tokio::test]
async fn an_incomplete_citation_opens_the_search_page_until_the_reader_completes_it() {
    let portal = Recorded::new(Answer::Search(AD44_ONE));
    let app = app_with(&portal).await;
    let tree = new_tree(&app, "Archives").await;
    let source = new_source(&app, &tree, "AD44, E dépôt 99").await;
    let citation = new_citation(&app, &tree, &source, "vue 2g/3").await;

    // No act, no locality: the archive's website, without a request.
    let ((status, target), response) = both(&app, &tree, &source, Some(&citation), None).await;
    assert_eq!(status, StatusCode::OK, "{target}");
    assert_eq!(target["kind"], "results");
    assert_eq!(target["matches"], Value::Null);
    assert!(
        target["url"]
            .as_str()
            .unwrap()
            .starts_with("https://archives.loire-atlantique.fr/"),
        "{target}"
    );
    let gql_target = &response["data"]["source"]["archiveTarget"];
    assert_eq!(gql_target["kind"], "RESULTS", "{response}");
    assert_eq!(gql_target["url"], target["url"]);
    assert_eq!(portal.connections(), 0);

    // The reader completes it: the register is looked up.
    let parts = json!({ "locality": "Exampleville", "act": "B", "year": 1660 });
    let ((status, target), response) =
        both(&app, &tree, &source, Some(&citation), Some(parts)).await;
    assert_eq!(status, StatusCode::OK, "{target}");
    assert_eq!(target["kind"], "view");
    assert_eq!(target["views"][0]["view"], 2);
    let gql_target = &response["data"]["source"]["archiveTarget"];
    assert_eq!(gql_target["kind"], "VIEW", "{response}");
    assert_eq!(gql_target["url"], target["url"]);
    assert_eq!(portal.connections(), 1);
}

#[tokio::test]
async fn parts_the_archive_cannot_search_are_refused_on_both_surfaces() {
    let portal = Recorded::new(Answer::Search(AD44_ONE));
    let app = app_with(&portal).await;
    let tree = new_tree(&app, "Archives").await;
    let source = new_source(&app, &tree, "AD44, E dépôt 99").await;

    for parts in [
        // A document kind no collection of the archive holds, or none at all.
        json!({ "locality": "Exampleville", "act": "TD" }),
        json!({ "locality": "Exampleville", "act": "XX" }),
        json!({ "locality": "  ", "act": "B" }),
        json!({ "locality": "Exampleville", "act": "B", "year": 900 }),
        json!({ "locality": "Exampleville", "act": "B", "view": 0 }),
    ] {
        let ((status, body), response) =
            both(&app, &tree, &source, None, Some(parts.clone())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{parts}: {body}");
        assert_eq!(body["error"], "validation_error", "{parts}");
        assert_eq!(gql_error_code(&response), "VALIDATION_ERROR", "{parts}");
    }
    assert_eq!(portal.connections(), 0);
}

#[tokio::test]
async fn a_portal_address_in_the_citation_is_the_target_as_it_is() {
    let portal = Recorded::new(Answer::Search(AD44_ONE));
    let app = app_with(&portal).await;
    let tree = new_tree(&app, "Archives").await;
    let address = "https://archives-numerisees.loire-atlantique.fr/v2/ad44/visualiseur/registre.html?id=440000000";
    let source = new_source(&app, &tree, "Acte de baptême").await;
    let citation = new_citation(&app, &tree, &source, &format!("Voir {address}")).await;

    let ((status, target), response) = both(&app, &tree, &source, Some(&citation), None).await;
    assert_eq!(status, StatusCode::OK, "{target}");
    assert_eq!(target["kind"], "view");
    assert_eq!(target["url"], address);
    assert_eq!(target["views"], json!([]));
    let gql_target = &response["data"]["source"]["archiveTarget"];
    assert_eq!(gql_target["kind"], "VIEW", "{response}");
    assert_eq!(gql_target["url"], address);
    assert_eq!(portal.connections(), 0);
}
