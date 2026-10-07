//! The adapter over anonymized pages of the Gers portal
//! (`fixtures/archives32/`, written by `generate.py`), with the
//! catalogue's own settings.

use std::pin::pin;
use std::sync::Mutex;
use std::task::{Context, Poll, Waker};

use super::{Archives32, page};
use crate::catalog::Collection;
use crate::citation::Act;
use crate::platform::{BoxFuture, Platform};
use crate::transport::{FetchError, Method, PortalFetch, PortalRequest};
use crate::{ArchiveRegistry, ArchiveTarget, ArchiveView, ResolveError};

macro_rules! fixture {
    ($name:literal) => {
        include_str!(concat!("../../../fixtures/archives32/", $name))
    };
}

const EC_EXAMPLEVILLE: &str = fixture!("ec-exampleville.html");
const EC_NONE: &str = fixture!("ec-none.html");
const EC_ISLE: &str = fixture!("ec-isle.html");
const EC_FORMER: &str = fixture!("ec-former.html");
const RP_BOURG: &str = fixture!("rp-bourg.html");
const TD_EXAMPLEVILLE: &str = fixture!("td-exampleville.html");
const CENSUS: &str = fixture!("census-exampleville.html");
const SUCCESSIONS: &str = fixture!("successions-exampleville.html");
const VIEWER: &str = fixture!("viewer-listed.html");
const CHALLENGE: &str = fixture!("challenge.html");

const ORIGIN: &str = "https://www.archives32.fr";
const PORTAL: &str = "/archives_numerisees/portail";

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

/// A portal answering a `POST` by the first route whose fragment its body
/// holds, and a `GET` by its address.
struct Portal {
    posts: Vec<(&'static str, &'static str)>,
    gets: Vec<(String, &'static str)>,
    requests: Mutex<Vec<PortalRequest>>,
}

impl Portal {
    fn new(posts: &[(&'static str, &'static str)]) -> Self {
        Self {
            posts: posts.to_vec(),
            gets: Vec::new(),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn with_get(mut self, path: String, body: &'static str) -> Self {
        self.gets.push((path, body));
        self
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
            let body = match request.method {
                Method::Post => {
                    let sent = request.body.as_deref().unwrap_or_default();
                    self.posts
                        .iter()
                        .find(|(fragment, _)| sent.contains(fragment))
                        .map(|(_, body)| *body)
                }
                Method::Get => self
                    .gets
                    .iter()
                    .find(|(path, _)| *path == request.url)
                    .map(|(_, body)| *body),
            };
            body.map(str::to_owned).ok_or(FetchError::Status(404))
        })
    }
}

fn resolve(title: &str, portal: &Portal) -> Result<ArchiveTarget, ResolveError> {
    let registry = ArchiveRegistry::embedded();
    let citation = registry.parse(title).expect("a normalized citation");
    let (archive, collections) = registry.candidates(&citation).expect("a catalogued act");
    block_on(Archives32.resolve(archive, collections[0], &citation, portal))
}

fn viewer(module: &str, query: &str) -> String {
    format!("{ORIGIN}{PORTAL}/{module}/visu/?{query}")
}

fn results(module: &str, matches: usize) -> ArchiveTarget {
    ArchiveTarget::Results {
        url: format!("{ORIGIN}{PORTAL}/{module}/recherche/"),
        matches: Some(matches),
    }
}

#[test]
fn finds_a_register_in_one_search_and_computes_the_view() {
    let portal = Portal::new(&[("lieu=Exampleville", EC_EXAMPLEVILLE)]);
    let target = resolve(
        "AD32 - Exampleville - (aucun) - N - 1850 - vue 5/512",
        &portal,
    )
    .unwrap();
    let url = viewer(
        "etats_civils/ec",
        "id=9003&fichier=700304&lieu=Exampleville&annee=1843",
    );
    assert_eq!(
        target,
        ArchiveTarget::View {
            url: url.clone(),
            views: vec![ArchiveView {
                view: 5,
                url,
                ark: None,
                image: None
            }],
            view_count: Some(512),
            call_number: Some("5 E 9003".to_owned()),
            attribution: None,
            renumbering: None,
        }
    );
    let requests = portal.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].url,
        "/archives_numerisees/portail/etats_civils/ec/recherche/"
    );
    // No year: the portal's filter is not reliable, the periods decide.
    assert_eq!(
        requests[0].body.as_deref(),
        Some("lieu=Exampleville&ancienne=all&annee_d=&annee_f=&chk_naissance=on&valider=valider")
    );
}

#[test]
fn reads_the_acts_from_the_content_s_first_line_only() {
    // The marriages' register notes births and deaths it does not hold.
    let portal = Portal::new(&[("lieu=Exampleville", EC_EXAMPLEVILLE)]);
    let target = resolve("AD32 - Exampleville - (aucun) - N - 1800", &portal).unwrap();
    assert!(
        matches!(&target, ArchiveTarget::View { call_number: Some(call), .. } if call == "5 E 9001"),
        "{target:?}"
    );
    let portal = Portal::new(&[("lieu=Exampleville", EC_EXAMPLEVILLE)]);
    let target = resolve("AD32 - Exampleville - (aucun) - M - 1800", &portal).unwrap();
    assert_eq!(target, results("etats_civils/ec", 2));
    assert!(
        portal.requests()[0]
            .body
            .as_deref()
            .unwrap()
            .contains("&chk_mariage=on&")
    );
}

#[test]
fn searches_again_with_the_list_s_label_or_a_former_commune() {
    // The list writes the commune with a trailing space.
    let portal = Portal::new(&[
        ("lieu=Bourg-Exemple%20&", RP_BOURG),
        ("lieu=Bourg-Exemple&", EC_NONE),
    ]);
    let target = resolve(
        "AD32 - Bourg-Exemple - Saint-Exemple - B - 1745 - vue 3/20",
        &portal,
    )
    .unwrap();
    assert!(
        matches!(&target, ArchiveTarget::View { url, .. } if *url == viewer("etats_civils/rp", "td=9301&fichier=80002&lieu=Bourg-Exemple&annee=1740")),
        "{target:?}"
    );
    assert_eq!(portal.requests().len(), 2);
    // The image count tells the two parishes' registers apart.
    let portal = Portal::new(&[
        ("lieu=Bourg-Exemple%20&", RP_BOURG),
        ("lieu=Bourg-Exemple&", EC_NONE),
    ]);
    let target = resolve(
        "AD32 - Bourg-Exemple - (aucun) - B - 1745 - vue 3/53",
        &portal,
    )
    .unwrap();
    assert!(
        matches!(&target, ArchiveTarget::View { call_number: Some(call), .. } if call == "5 E 9302 (1)"),
        "{target:?}"
    );

    let portal = Portal::new(&[
        ("lieu=all&ancienne=Vieux-Exemple&", EC_FORMER),
        ("lieu=Vieux-Exemple&", EC_NONE),
    ]);
    let target = resolve("AD32 - Vieux-Exemple - (aucun) - N - 1800", &portal).unwrap();
    assert!(
        matches!(&target, ArchiveTarget::View { call_number: Some(call), .. } if call == "5 E 9201"),
        "{target:?}"
    );

    let portal = Portal::new(&[
        ("lieu=L%27isle-exemple&", EC_NONE),
        ("lieu=L%27Isle", EC_ISLE),
    ]);
    let target = resolve("AD32 - L'isle-exemple - (aucun) - N - 1800", &portal).unwrap();
    assert!(matches!(target, ArchiveTarget::View { .. }), "{target:?}");
}

#[test]
fn a_locality_no_list_names_finds_nothing() {
    let portal = Portal::new(&[("lieu=Nowhere&", EC_NONE)]);
    let target = resolve("AD32 - Nowhere - (aucun) - N - 1800", &portal).unwrap();
    assert_eq!(target, results("etats_civils/ec", 0));
    assert_eq!(portal.requests().len(), 1);
}

#[test]
fn tells_the_two_copies_of_a_table_apart_by_call_number_or_count() {
    let portal = || Portal::new(&[("lieu=Exampleville", TD_EXAMPLEVILLE)]);
    assert_eq!(
        resolve("AD32 - Exampleville - (aucun) - TD - 1805", &portal()).unwrap(),
        results("etats_civils/td", 2)
    );
    let portal = portal();
    let target = resolve(
        "AD32 - Exampleville - (aucun) - TD - 1805 - 5 E 9402",
        &portal,
    )
    .unwrap();
    assert!(
        matches!(
            &target,
            ArchiveTarget::View {
                view_count: Some(17),
                ..
            }
        ),
        "{target:?}"
    );
    assert!(
        portal.requests()[0]
            .body
            .as_deref()
            .unwrap()
            .contains("&chk_naissance=on&chk_mariage=on&chk_deces=on&")
    );
}

#[test]
fn reads_a_listed_viewer_for_the_census_views() {
    let query = "rec=9502&fichier=22993&lieu=Exampleville&annee=1841";
    let portal = Portal::new(&[(
        "LIEU=Exampleville&ANCIENNE=all&annee_d=&annee_f=&valider",
        CENSUS,
    )])
    .with_get(
        format!("{PORTAL}/recensement_population/visu/?{query}"),
        VIEWER,
    );
    let target = resolve(
        "AD32 - Exampleville - Recensement - 1841 - vue 2/4",
        &portal,
    )
    .unwrap();
    let url = viewer(
        "recensement_population",
        "rec=9502&fichier=37762&lieu=Exampleville&annee=1841",
    );
    assert_eq!(
        target,
        ArchiveTarget::View {
            url: url.clone(),
            views: vec![ArchiveView {
                view: 2,
                url,
                ark: None,
                image: None
            }],
            view_count: Some(4),
            call_number: Some("6 M 902".to_owned()),
            attribution: None,
            renumbering: None,
        }
    );
    assert_eq!(portal.requests().len(), 2);

    // Without a cited view, the viewer is not read.
    let portal = Portal::new(&[("lieu=Exampleville", SUCCESSIONS)]);
    let target = resolve("AD32 - Exampleville - (aucun) - TSA - 1850", &portal).unwrap();
    assert!(
        matches!(&target, ArchiveTarget::View { url, views, .. } if views.is_empty() && *url == viewer("successions_absences", "sa=9601&fichier=20037&lieu=Exampleville&annee=1844")),
        "{target:?}"
    );
    assert_eq!(portal.requests().len(), 1);
    assert_eq!(
        portal.requests()[0].body.as_deref(),
        Some("lieu=Exampleville&annee_d=&annee_f=&valider=valider")
    );
}

#[test]
fn reports_a_challenge_or_a_changed_page() {
    let portal = Portal::new(&[("lieu=", CHALLENGE)]);
    assert_eq!(
        resolve("AD32 - Exampleville - (aucun) - N - 1850", &portal),
        Err(ResolveError::Challenged)
    );
    let portal = Portal::new(&[("lieu=", "<html><body>Maintenance</body></html>")]);
    assert!(matches!(
        resolve("AD32 - Exampleville - (aucun) - N - 1850", &portal),
        Err(ResolveError::UnexpectedResponse(detail)) if detail.contains("archives32")
    ));
}

#[test]
fn reads_the_results_and_the_viewer() {
    let rows = page::rows(RP_BOURG).unwrap().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].parish.as_deref(), Some("Saint-Exemple"));
    assert_eq!(rows[1].parish.as_deref(), Some("Bourg-Exemple"));
    assert_eq!(
        rows[0].acts.as_deref(),
        Some("Baptêmes / Mariages / Sépultures /")
    );
    assert_eq!(rows[0].images, Some(20));
    let rows = page::rows(SUCCESSIONS).unwrap().unwrap();
    assert_eq!(rows[0].locality.as_deref(), Some("Exampleville"));
    assert_eq!(rows[0].acts, None);
    assert!(page::rows(EC_NONE).unwrap().unwrap().is_empty());
    assert_eq!(
        page::views(VIEWER).unwrap(),
        [22_994, 37_762, 51_730, 123_544]
    );
    assert_eq!(
        page::options(EC_NONE, "lieu"),
        [
            "Bourg-Exemple ",
            "Exampleville",
            "L'Isle-Exemple",
            "Sampleton"
        ]
    );
    assert_eq!(
        page::viewer_at("id=1&fichier=10&lieu=X", 12),
        "id=1&fichier=12&lieu=X"
    );
}

fn collection(portal: serde_json::Value, acts: &[&str]) -> Collection {
    Collection {
        id: "registers".to_owned(),
        acts: acts
            .iter()
            .map(|code| Act::from_code(code).unwrap())
            .collect(),
        period: None,
        platform: "archives32".to_owned(),
        portal,
    }
}

#[test]
fn validates_the_settings() {
    let valid = || {
        serde_json::json!({
            "origin": "https://archives.example.org",
            "transport": "browser",
            "path": "/portail/ec",
            "fields": { "locality": "lieu", "former": "ancienne" },
            "acts": { "N": ["chk_naissance"], "M": ["chk_mariage"], "D": ["chk_deces"] },
            "views": "contiguous"
        })
    };
    assert_eq!(
        Archives32.validate(&collection(valid(), &["N", "M", "D"])),
        Ok(())
    );
    // A combined act searched by its kinds' boxes, banns as marriages.
    assert_eq!(
        Archives32.validate(&collection(valid(), &["NMD", "P"])),
        Ok(())
    );
    assert!(Archives32.validate(&collection(valid(), &["B"])).is_err());
    let mut series = valid();
    series["acts"] = serde_json::json!({});
    assert_eq!(Archives32.validate(&collection(series, &["RP"])), Ok(()));
    for (field, value) in [
        ("origin", serde_json::json!("http://archives.example.org")),
        ("path", serde_json::json!("/portail/ec/")),
        ("path", serde_json::json!("portail")),
        ("fields", serde_json::json!({ "locality": "li eu" })),
        ("acts", serde_json::json!({ "N": [] })),
        ("acts", serde_json::json!({ "X": ["chk"] })),
        ("views", serde_json::json!("other")),
        ("unknown", serde_json::json!(1)),
    ] {
        let mut portal = valid();
        portal[field] = value;
        assert!(
            Archives32.validate(&collection(portal, &["N"])).is_err(),
            "{field}"
        );
    }
}

#[test]
fn starts_on_robots_and_offers_the_search_page() {
    let registry = ArchiveRegistry::embedded();
    let archive = registry.archive("AD32").unwrap();
    let citation = registry
        .parse("AD32 - Exampleville - (aucun) - N - 1850")
        .unwrap();
    let collection = &archive.collections[1];
    assert_eq!(
        Archives32.results_url(collection, &citation).as_deref(),
        Some("https://www.archives32.fr/archives_numerisees/portail/etats_civils/ec/recherche/")
    );
    let endpoint = Archives32.endpoint(collection).unwrap();
    assert_eq!(endpoint.start, "https://www.archives32.fr/robots.txt");
    assert_eq!(endpoint.access, crate::Access::Browser);
}
