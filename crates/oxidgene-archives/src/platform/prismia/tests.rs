//! The adapter over anonymized answers of the Lot-et-Garonne portal's API
//! (`fixtures/prismia/`, written by `fixtures/archinoe/generate.py`), with
//! the catalogue's own settings.

use std::pin::pin;
use std::sync::Mutex;
use std::task::{Context, Poll, Waker};

use crate::platform::{self, BoxFuture};
use crate::transport::{FetchError, Method, PortalFetch, PortalRequest};
use crate::{ArchiveRegistry, ArchiveTarget, ArchiveView, ResolveError};

const CONFIG: &str = include_str!("../../../fixtures/prismia/runtime-config.js");
const FACETS: &str = include_str!("../../../fixtures/prismia/facets.json");
const ONE: &str = include_str!("../../../fixtures/prismia/query-one.json");
const SEVERAL: &str = include_str!("../../../fixtures/prismia/query-several.json");
const NONE: &str = include_str!("../../../fixtures/prismia/query-none.json");

const API: &str = "https://ad47.backend.archives.prismia.fr/api";
const VIEWER: &str = "https://lotetgaronne.archives.prismia.fr/viewer/";

/// Drives a future whose every step completes at once.
fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
    }
}

/// The portal: its configuration, then the API, which answers a search by
/// the year it asks for.
struct Portal {
    config: String,
    facets: String,
    query: Option<fn(&str) -> &'static str>,
    requests: Mutex<Vec<PortalRequest>>,
}

fn by_year(body: &str) -> &'static str {
    if body.contains("\"1700-01-01") {
        ONE
    } else if body.contains("\"1500-01-01") {
        NONE
    } else {
        SEVERAL
    }
}

impl Portal {
    fn new() -> Self {
        Self {
            config: CONFIG.to_owned(),
            facets: FACETS.to_owned(),
            query: Some(by_year),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn requests(&self) -> Vec<PortalRequest> {
        self.requests.lock().unwrap().clone()
    }
}

impl PortalFetch for Portal {
    fn request<'a>(
        &'a self,
        request: &'a PortalRequest,
    ) -> BoxFuture<'a, Result<String, FetchError>> {
        Box::pin(async move {
            self.requests.lock().unwrap().push(request.clone());
            let body = request.body.as_deref().unwrap_or_default();
            if request.url == "/runtimeConfig.js" {
                Ok(self.config.clone())
            } else if request.url == format!("{API}/presentation/v1/facet/getFacetValues") {
                Ok(self.facets.clone())
            } else if request.url == format!("{API}/presentation/v1/Query") {
                self.query
                    .map(|route| route(body).to_owned())
                    .ok_or(FetchError::Status(404))
            } else {
                Err(FetchError::Status(404))
            }
        })
    }
}

fn resolve(title: &str, portal: &Portal) -> Result<ArchiveTarget, ResolveError> {
    let registry = ArchiveRegistry::embedded();
    let citation = registry.parse(title).expect("a normalized citation");
    let (archive, collections) = registry.candidates(&citation).expect("a catalogued act");
    let platform = registry.platform(&collections[0].platform).unwrap();
    block_on(platform.resolve(archive, collections[0], &citation, portal))
}

fn ok(title: &str) -> (ArchiveTarget, Portal) {
    let portal = Portal::new();
    let target = resolve(title, &portal).unwrap_or_else(|error| panic!("{title}: {error}"));
    (target, portal)
}

fn manifest(id: u32) -> String {
    format!(
        "https%3A%2F%2Fad47.backend.archives.prismia.fr%2Fapi%2Fiiif%2Fpresentation%2Fv3%2F{id}%2Fmanifest"
    )
}

fn json(request: &PortalRequest) -> serde_json::Value {
    serde_json::from_str(request.body.as_deref().unwrap()).unwrap()
}

#[test]
fn a_register_opens_in_the_viewer_on_the_cited_canvas() {
    let (target, portal) =
        ok("AD47 - Exampleville - (aucun) - B - 1700 - E SUP EXEMPLE GG-1 - vue 5/216");
    let url = format!("{VIEWER}{}/5", manifest(900000001));
    assert_eq!(
        target,
        ArchiveTarget::View {
            url: url.clone(),
            views: vec![ArchiveView {
                view: 5,
                url,
                ark: None,
                image: None,
            }],
            view_count: Some(216),
            call_number: Some("E SUP EXEMPLE GG-1".to_owned()),
            attribution: None,
            renumbering: None,
        }
    );

    assert_requests(&portal.requests());
}

/// The portal's configuration, the locality list, the search.
fn assert_requests(requests: &[PortalRequest]) {
    // The portal's configuration, the locality list, the search.
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].method, Method::Get);
    assert!(requests[0].headers.is_empty());
    for request in &requests[1..] {
        assert_eq!(request.method, Method::Post);
        assert_eq!(
            request.headers,
            [
                ("Content-Type".to_owned(), "application/json".to_owned()),
                ("ApiKey".to_owned(), "exampleKey0123".to_owned())
            ]
        );
    }
    let facets = json(&requests[1]);
    assert_eq!(facets["text"], "Exampleville");
    assert_eq!(
        facets["aggregateValue"],
        serde_json::json!(["geogname||Commune||Commune"])
    );
    let search = json(&requests[2]);
    assert_eq!(
        search["tagSelectedFilters"],
        serde_json::json!([
            { "keys": ["geogname||Commune||Commune"], "values": ["Exampleville"] },
            { "keys": ["extrafield||Actes"], "values": ["Baptêmes ou Naissances"] },
        ])
    );
    assert_eq!(search["periodeDeb"], "1700-01-01T00:00:00.000Z");
    assert_eq!(search["periodeFin"], "1700-12-31T00:00:00.000Z");
    assert_eq!(search["target"], serde_json::json!(["document"]));
    // The portal learns the locality, act and year, never the call number,
    // parish or view.
    let text = requests[2].body.as_deref().unwrap();
    for private in ["GG-1", "vue", "Saint-Exemple"] {
        assert!(!text.contains(private), "{private} in {text}");
    }
}

#[test]
fn a_cited_year_is_optional_and_the_key_is_not_part_of_the_settings() {
    let (_, portal) = ok("AD47 - Exampleville - (aucun) - M");
    let search = json(&portal.requests()[2]);
    assert!(search.get("periodeDeb").is_none());
    assert_eq!(search["tagSelectedFilters"][1]["values"][0], "Mariages");

    let collection = &ArchiveRegistry::embedded()
        .archive("AD47")
        .unwrap()
        .collections[0];
    assert!(!collection.portal.to_string().contains("exampleKey"));
    assert!(collection.portal.get("api_key").is_none());
}

#[test]
fn several_registers_are_told_apart_by_the_citation_parts() {
    let (target, portal) = ok("AD47 - Exampleville - (aucun) - B - 1750");
    let ArchiveTarget::Results { url, matches } = &target else {
        panic!("expected results, got {target:?}");
    };
    assert_eq!(*matches, Some(3));
    assert_eq!(
        url,
        "https://lotetgaronne.archives.prismia.fr/Recherche/Etat%20civil"
    );
    assert_eq!(portal.requests().len(), 3);

    for (title, call_number) in [
        ("AD47 - Exampleville - (aucun) - B - 1750 - 4E9-1", "4E9-1"),
        // The parish, as the stub lists it.
        (
            "AD47 - Exampleville - Notre-Dame-Exemple - B - 1750",
            "4E9-2",
        ),
        // The image count of the stubs.
        (
            "AD47 - Exampleville - (aucun) - B - 1750 - vue 3/192",
            "E SUP EXEMPLE GG-2",
        ),
    ] {
        let (target, _) = ok(title);
        let ArchiveTarget::View {
            call_number: opened,
            ..
        } = target
        else {
            panic!("{title}: expected a view");
        };
        assert_eq!(opened.as_deref(), Some(call_number), "{title}");
    }

    // The dates are the stub's years, or its bounds when it lists none: the
    // first register starts in 1745.
    let (target, _) = ok("AD47 - Exampleville - (aucun) - B - 1741");
    assert!(matches!(
        target,
        ArchiveTarget::Results {
            matches: Some(2),
            ..
        }
    ));
    let (target, _) = ok("AD47 - Exampleville - (aucun) - B - 1745 - E SUP EXEMPLE GG-2");
    assert!(matches!(target, ArchiveTarget::View { .. }));
    let (target, _) = ok("AD47 - Exampleville - (aucun) - B - 1750 - 4E9-9");
    assert!(matches!(
        target,
        ArchiveTarget::Results {
            matches: Some(3),
            ..
        }
    ));
}

#[test]
fn a_view_beyond_the_register_opens_it_on_its_first_canvas() {
    let (target, _) =
        ok("AD47 - Exampleville - (aucun) - B - 1700 - E SUP EXEMPLE GG-1 - vue 250/300");
    let ArchiveTarget::View { url, views, .. } = target else {
        panic!("expected a view");
    };
    assert!(views.is_empty());
    assert_eq!(url, format!("{VIEWER}{}", manifest(900000001)));
}

#[test]
fn the_locality_is_matched_on_the_facet_label_as_the_portal_writes_it() {
    for (title, prefix, value) in [
        (
            "AD47 - Le Mas-Exemple - (aucun) - B - 1700",
            "Mas-Exemple",
            "Mas-Exemple (Le)",
        ),
        (
            "AD47 - Mas-Exemple (Le) - (aucun) - B - 1700",
            "Mas-Exemple",
            "Mas-Exemple (Le)",
        ),
        // The portal writes an apostrophe either way.
        (
            "AD47 - Le Passage-d’Exemple - (aucun) - B - 1700",
            "Passage-d",
            "Passage-d'Exemple (Le)",
        ),
        (
            "AD47 - exampleville-d'aval - (aucun) - B - 1700",
            "exampleville-d",
            "Exampleville-d’Aval",
        ),
    ] {
        let (_, portal) = ok(title);
        let requests = portal.requests();
        assert_eq!(json(&requests[1])["text"], prefix, "{title}");
        assert_eq!(
            json(&requests[2])["tagSelectedFilters"][0]["values"][0],
            value,
            "{title}"
        );
    }
}

#[test]
fn no_register_and_no_locality_give_empty_results() {
    let (target, portal) = ok("AD47 - Exampleville - (aucun) - B - 1500");
    assert!(matches!(
        target,
        ArchiveTarget::Results {
            matches: Some(0),
            ..
        }
    ));
    assert_eq!(portal.requests().len(), 3);

    let (target, portal) = ok("AD47 - Nowhere - (aucun) - B - 1700");
    assert!(matches!(
        target,
        ArchiveTarget::Results {
            matches: Some(0),
            ..
        }
    ));
    // The search is not sent for a locality the portal does not list.
    assert_eq!(portal.requests().len(), 2);
}

#[test]
fn a_changed_portal_is_reported_as_drift() {
    let title = "AD47 - Exampleville - (aucun) - B - 1700";
    let mut variants = Vec::new();
    for config in [
        "window.prismConfig = {};",
        "window.prismConfig = { apiKey: '' };",
        "window.prismConfig = { apiKey: 'a b' };",
        "window.prismConfig = { apiKey: 1 };",
    ] {
        let mut portal = Portal::new();
        portal.config = config.to_owned();
        variants.push(portal);
    }
    for facets in ["<html>maintenance</html>", "{\"total\": 0}"] {
        let mut portal = Portal::new();
        portal.facets = facets.to_owned();
        variants.push(portal);
    }
    for query in [
        |_: &str| "<html>maintenance</html>",
        |_: &str| r#"{"total": 1, "listResponseObject": [{"prismCoteId": "x"}]}"#,
        |_: &str| r#"{"total": 1, "listResponseObject": [{"id": "https://elsewhere.example.org/manifest"}]}"#,
    ] {
        let mut portal = Portal::new();
        portal.query = Some(query);
        variants.push(portal);
    }
    for portal in variants {
        let result = resolve(title, &portal);
        assert!(
            matches!(result, Err(ResolveError::UnexpectedResponse(_))),
            "{result:?}"
        );
    }

    // An error status is the transport's.
    let mut portal = Portal::new();
    portal.query = None;
    assert!(matches!(
        resolve(title, &portal),
        Err(ResolveError::UnexpectedResponse(_))
    ));
}

#[test]
fn the_api_origin_is_declared_for_the_transport() {
    let registry = ArchiveRegistry::embedded();
    let collection = &registry.archive("AD47").unwrap().collections[0];
    let endpoint = registry
        .platform("prismia")
        .unwrap()
        .endpoint(collection)
        .unwrap();
    assert_eq!(endpoint.origin, "https://lotetgaronne.archives.prismia.fr");
    assert_eq!(
        endpoint.other_origins,
        ["https://ad47.backend.archives.prismia.fr"]
    );
    assert_eq!(endpoint.access, crate::Access::Any);
    let citation = registry
        .parse("AD47 - Exampleville - (aucun) - B - 1700")
        .unwrap();
    assert_eq!(
        registry.offline_target(&citation),
        Ok(ArchiveTarget::Results {
            url: "https://lotetgaronne.archives.prismia.fr/Recherche/Etat%20civil".to_owned(),
            matches: None,
        })
    );
}

fn registry_with(edit: impl Fn(&mut serde_json::Map<String, serde_json::Value>)) -> bool {
    let text = include_str!("../../../../../assets/archives/fr/fr-ad47.json");
    let mut value: serde_json::Value = serde_json::from_str(text).unwrap();
    edit(value["collections"][0]["portal"].as_object_mut().unwrap());
    ArchiveRegistry::new(&[("fr", &value.to_string())], platform::builtin()).is_ok()
}

#[test]
fn unusable_settings_are_refused_at_load_time() {
    assert!(registry_with(|_| {}));
    type Edit = fn(&mut serde_json::Map<String, serde_json::Value>);
    let edits: [(&str, Edit); 10] = [
        ("an unknown member", |portal| {
            portal.insert("api_key".into(), "key".into());
        }),
        ("an http origin", |portal| {
            portal.insert("origin".into(), "http://archives.example.org".into());
        }),
        ("an http api", |portal| {
            portal.insert("api".into(), "http://api.example.org/api".into());
        }),
        ("an api without a path", |portal| {
            portal.insert("api".into(), "https://api.example.org".into());
        }),
        ("an api with a trailing slash", |portal| {
            portal.insert("api".into(), "https://api.example.org/api/".into());
        }),
        ("no collection path", |portal| {
            portal.insert("paths".into(), serde_json::json!([]));
        }),
        ("an empty filter key", |portal| {
            portal["filters"]["locality"] = "".into();
        }),
        ("a relative search path", |portal| {
            portal.insert("search_path".into(), "Recherche".into());
        }),
        ("an act that is not a code", |portal| {
            portal["acts"]
                .as_object_mut()
                .unwrap()
                .insert("XX".into(), "Mariages".into());
        }),
        ("a missing act value", |portal| {
            portal["acts"].as_object_mut().unwrap().remove("TD");
        }),
    ];
    for (name, edit) in edits {
        assert!(!registry_with(edit), "{name} was accepted");
    }
}
