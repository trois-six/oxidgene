//! The adapter over anonymized pages of the older Mnesys interface
//! (`fixtures/mnesys-inao/`, written by `generate.py`), with the
//! catalogue's own settings.

use std::pin::pin;
use std::sync::Mutex;
use std::task::{Context, Poll, Waker};

use super::{MnesysInao, page};
use crate::catalog::Collection;
use crate::citation::Act;
use crate::platform::{BoxFuture, Platform};
use crate::transport::{FetchError, PortalFetch, PortalRequest};
use crate::{ArchiveRegistry, ArchiveTarget, ArchiveView, ResolveError};

macro_rules! fixture {
    ($name:literal) => {
        include_str!(concat!("../../../fixtures/mnesys-inao/", $name))
    };
}

const ONE: &str = fixture!("answer-one.html");
const CITY: &str = fixture!("answer-city.html");
const CITY_2: &str = fixture!("answer-city-2.html");
const CENSUS: &str = fixture!("answer-census.html");
const MILITARY: &str = fixture!("answer-military.html");
const NONE: &str = fixture!("answer-none.html");
const NOTICE: &str = fixture!("notice.html");
const NOTICE_WITHOUT_IMAGES: &str = fixture!("notice-without-images.html");
const NOTICE_ELSEWHERE: &str = fixture!("notice-elsewhere.html");
const FORM_REGISTERS: &str = fixture!("form-registers.html");

const ORIGIN: &str = "https://recherche-archives.savoie.fr";
const ARK: &str = "https://archives-numeriques.savoie.fr/ark:/99999/0123456789abcdef";

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

/// A portal answering a search (`/?form_search_…`), a further page of the
/// session's search (`&page=2`) and any notice (`_detail`).
struct Portal {
    search: &'static str,
    next: &'static str,
    notice: &'static str,
    requests: Mutex<Vec<String>>,
}

impl Portal {
    fn new(search: &'static str, notice: &'static str) -> Self {
        Self {
            search,
            next: NONE,
            notice,
            requests: Mutex::new(Vec::new()),
        }
    }

    fn paths(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

impl PortalFetch for Portal {
    fn request<'a>(
        &'a self,
        request: &'a PortalRequest,
    ) -> BoxFuture<'a, Result<String, FetchError>> {
        Box::pin(async move {
            self.requests.lock().unwrap().push(request.url.clone());
            let url = request.url.as_str();
            let body = if url.starts_with("/?form_search_") {
                self.search
            } else if url.contains("_detail&") {
                self.notice
            } else if url.contains("&page=2&") {
                self.next
            } else {
                return Err(FetchError::Status(404));
            };
            Ok(body.to_owned())
        })
    }
}

fn resolve(title: &str, portal: &Portal) -> Result<ArchiveTarget, ResolveError> {
    let registry = ArchiveRegistry::embedded();
    let citation = registry.parse(title).expect("a normalized citation");
    let (archive, collections) = registry.candidates(&citation).expect("a catalogued act");
    block_on(MnesysInao.resolve(archive, collections[0], &citation, portal))
}

fn view(views: &[u16], call_number: &str) -> ArchiveTarget {
    let views: Vec<ArchiveView> = views
        .iter()
        .map(|view| ArchiveView {
            view: *view,
            url: format!("{ARK}?vue={view}"),
            ark: None,
            image: None,
        })
        .collect();
    ArchiveTarget::View {
        url: views
            .first()
            .map_or_else(|| ARK.to_owned(), |view| view.url.clone()),
        views,
        view_count: None,
        call_number: Some(call_number.to_owned()),
        attribution: None,
        renumbering: None,
    }
}

#[test]
fn finds_a_register_and_opens_its_ark_at_the_view_without_the_viewer_host() {
    let portal = Portal::new(ONE, NOTICE);
    let target = resolve("AD73 - Exampleville - (aucun) - M - 1850 - vue 12", &portal).unwrap();
    assert_eq!(target, view(&[12], "3E 9017"));
    let paths = portal.paths();
    assert_eq!(
        paths[0],
        "/?form_search_geogname=Exampleville&form_op_geogname=ET&form_search_unitdate3=1850&form_search_unitdate=1850&form_search_dao=oui&form_search_v2_field_16994445147VTDcz=Mariages&form_op_v2_field_16994445147VTDcz=ET&form_req_v2_field_16994445147VTDcz=%7B%3Aunittitle%7D__VAL_&display_thesaurus=autocomplete&action=search&id=recherche_guidee_etat_civil_web"
    );
    assert!(paths[1].starts_with("/?id=recherche_guidee_etat_civil_web_detail&doc="));
    assert_eq!(paths.len(), 2);
}

#[test]
fn reads_the_place_parish_and_act_of_each_node() {
    // The parish the breadcrumb names after the commune decides.
    let portal = Portal::new(CITY, NOTICE);
    let target = resolve("AD73 - Exampleville - Notre-Exemple - M", &portal).unwrap();
    assert!(matches!(target, ArchiveTarget::View { .. }));
    assert!(portal.paths()[1].ends_with("page_ref=18242"));
    // A table is a table, whatever acts its title names.
    let portal = Portal::new(CITY, NOTICE);
    resolve("AD73 - Exampleville - (aucun) - TD", &portal).unwrap();
    assert!(portal.paths()[1].ends_with("page_ref=21268"));
    // Without a parish, the commune's two marriage registers.
    let portal = Portal::new(CITY, NOTICE);
    assert_eq!(
        resolve("AD73 - Exampleville - (aucun) - M", &portal).unwrap(),
        ArchiveTarget::Results {
            url: format!(
                "{ORIGIN}/?form_search_geogname=Exampleville&form_op_geogname=ET&form_search_dao=oui&form_search_v2_field_16994445147VTDcz=Mariages&form_op_v2_field_16994445147VTDcz=ET&form_req_v2_field_16994445147VTDcz=%7B%3Aunittitle%7D__VAL_&display_thesaurus=autocomplete&action=search&id=recherche_guidee_etat_civil_web"
            ),
            matches: Some(2),
        }
    );
}

#[test]
fn reads_the_session_s_further_pages_until_the_citation_decides() {
    let portal = Portal {
        next: CITY_2,
        ..Portal::new(CITY, NOTICE)
    };
    let target = resolve("AD73 - Exampleville - Saint-Exemple - M - 4E 9261", &portal).unwrap();
    assert!(matches!(target, ArchiveTarget::View { .. }));
    let paths = portal.paths();
    assert_eq!(
        paths[1],
        "/?id=recherche_guidee_etat_civil_web&doc=&page=2&page_ref="
    );
    assert!(paths[2].ends_with("page_ref=19600"));
}

#[test]
fn searches_a_census_by_place_and_a_military_register_by_class() {
    let portal = Portal::new(CENSUS, NOTICE);
    let target = resolve("AD73 - Exampleville - Recensement - 1901 - vue 3", &portal).unwrap();
    assert_eq!(target, view(&[3], "3E 9017"));
    assert!(
        portal.paths()[0].starts_with(
            "/?form_search_geogname=Exampleville&form_op_geogname=ET&form_search_unitdate3=1901&form_search_unitdate=1901&form_search_dao=oui&display_thesaurus"
        ),
        "{:?}",
        portal.paths()
    );

    // No locality: the class and the matricule find the volume.
    let portal = Portal::new(MILITARY, NOTICE);
    let target = resolve(
        "AD73 - Exampleville - Registres matricules - 1900 - matricule 600 - vue 5",
        &portal,
    )
    .unwrap();
    assert!(matches!(target, ArchiveTarget::View { .. }));
    let paths = portal.paths();
    assert!(
        paths[0].starts_with(
            "/?form_search_dao=oui&form_search_v2_field_14018083293lbOPp=Registre%20matricules%20de%20la%20classe%20SAUF%20R%C3%A9pertoire&form_op_v2_field_14018083293lbOPp=ET&form_req_v2_field_14018083293lbOPp=%7B%3Aunittitle%7D__VAL_&form_search_v2_field_1498829392winon2=%22Classe%201900%22&"
        ),
        "{paths:?}"
    );
    assert!(paths[1].ends_with("page_ref=6417"));
}

#[test]
fn a_search_without_hits_gives_the_results() {
    let portal = Portal::new(NONE, NOTICE);
    let target = resolve("AD73 - Nowhere - (aucun) - N - 1850", &portal).unwrap();
    assert!(matches!(
        target,
        ArchiveTarget::Results {
            matches: Some(0),
            ..
        }
    ));
    assert_eq!(portal.paths().len(), 1);
}

#[test]
fn a_notice_must_link_the_register_on_the_viewer_host() {
    for notice in [NOTICE_WITHOUT_IMAGES, NOTICE_ELSEWHERE] {
        let portal = Portal::new(ONE, notice);
        assert!(matches!(
            resolve("AD73 - Exampleville - (aucun) - M - 1850", &portal),
            Err(ResolveError::UnexpectedResponse(detail)) if detail.starts_with("mnesys-inao")
        ));
    }
    let portal = Portal::new(
        "<html><title>Just a moment...</title><script>window._cf_chl_opt={}</script></html>",
        NOTICE,
    );
    assert_eq!(
        resolve("AD73 - Exampleville - (aucun) - M - 1850", &portal),
        Err(ResolveError::Challenged)
    );
    let portal = Portal::new("<html><body>Maintenance</body></html>", NOTICE);
    assert!(matches!(
        resolve("AD73 - Exampleville - (aucun) - M - 1850", &portal),
        Err(ResolveError::UnexpectedResponse(_))
    ));
}

#[test]
fn reads_answers_and_forms() {
    let answer = page::answer(CITY).unwrap();
    assert_eq!((answer.total, answer.pages, answer.nodes.len()), (6, 2, 6));
    assert_eq!(answer.nodes[0].call_number, None);
    assert_eq!(answer.nodes[2].period.as_deref(), Some("1842-1850"));
    assert_eq!(
        answer.nodes[2].context,
        ["Exampleville.", "Notre-Exemple.", "Mariages religieux."]
    );
    let none = page::answer(NONE).unwrap();
    assert_eq!((none.total, none.nodes.len()), (0, 0));
    // The form alone shows no hit.
    assert_eq!(page::answer(FORM_REGISTERS).unwrap().total, 0);
    assert_eq!(page::media(NOTICE_WITHOUT_IMAGES), Ok(None));
    assert!(page::media(CITY).is_err());
}

fn collection(portal: serde_json::Value, acts: &[&str]) -> Collection {
    Collection {
        id: "registers".to_owned(),
        acts: acts
            .iter()
            .map(|code| Act::from_code(code).unwrap())
            .collect(),
        period: None,
        platform: "mnesys-inao".to_owned(),
        portal,
    }
}

#[test]
fn validates_the_settings() {
    let valid = || {
        serde_json::json!({
            "origin": "https://archives.example.org",
            "form": "recherche_guidee_exemple",
            "locality": "geogname",
            "act": "v2_field_1",
            "acts": { "N": "Naissances", "M": "Mariages" },
            "viewer": "https://viewer.example.org"
        })
    };
    assert_eq!(
        MnesysInao.validate(&collection(valid(), &["N", "M"])),
        Ok(())
    );
    assert!(MnesysInao.validate(&collection(valid(), &["D"])).is_err());
    let census = serde_json::json!({
        "origin": "https://archives.example.org",
        "form": "recherche_guidee_exemple",
        "locality": "geogname",
        "viewer": "https://viewer.example.org"
    });
    assert_eq!(MnesysInao.validate(&collection(census, &["RP"])), Ok(()));
    for (field, value) in [
        ("origin", serde_json::json!("http://archives.example.org")),
        ("viewer", serde_json::json!("https://viewer.example.org/")),
        ("form", serde_json::json!("id&x=1")),
        ("act", serde_json::Value::Null),
        ("acts", serde_json::json!({ "X": "Autre" })),
        (
            "year",
            serde_json::json!({ "field": "v2_field_2", "value": "Classe" }),
        ),
        ("unknown", serde_json::json!(1)),
    ] {
        let mut portal = valid();
        portal[field] = value;
        assert!(
            MnesysInao.validate(&collection(portal, &["N"])).is_err(),
            "{field}"
        );
    }
}

#[test]
fn offers_the_filtered_search_built_without_a_request() {
    let registry = ArchiveRegistry::embedded();
    let archive = registry.archive("AD73").unwrap();
    let citation = registry
        .parse("AD73 - Le Bourg - (aucun) - B - 1700")
        .unwrap();
    let url = MnesysInao
        .results_url(&archive.collections[0], &citation)
        .unwrap();
    assert!(
        url.starts_with(&format!("{ORIGIN}/?form_search_geogname=Le%20Bourg&")),
        "{url}"
    );
    assert!(url.contains("Bapt%C3%AAmes%20OU%20naissances"));
    let endpoint = MnesysInao.endpoint(&archive.collections[0]).unwrap();
    assert_eq!(endpoint.start, format!("{ORIGIN}/robots.txt"));
    assert!(endpoint.other_origins.is_empty());
}

#[test]
fn a_node_naming_no_place_is_the_place_index_s_answer() {
    let registry = ArchiveRegistry::embedded();
    let citation = registry
        .parse("AD73 - Exampleville - (aucun) - B - 1650")
        .unwrap();
    let localities = ["Exampleville"];
    let node = |title: &str, context: &[&str]| page::Node {
        detail: "/?id=x_detail&page_ref=1".to_owned(),
        title: title.to_owned(),
        period: Some("1616-1788".to_owned()),
        call_number: Some("5E 9015".to_owned()),
        context: context.iter().map(|entry| (*entry).to_owned()).collect(),
    };
    let place = |node: &page::Node| super::candidate(node, &citation, &localities).locality;
    // A title cut short names no place: the index matched it.
    assert_eq!(
        place(&node(
            "Collection de registres paroissiaux des paroisses réunies…",
            &[]
        )),
        Some("Exampleville".to_owned())
    );
    // A longer place, or a list of places, the word search matched too.
    assert_eq!(
        place(&node(
            "1850.",
            &["Exampleville-le-Vieux.", "Mariages religieux."]
        )),
        Some("Exampleville-le-Vieux".to_owned())
    );
    assert_eq!(
        place(&node(
            "Registre paroissial. - Sampleton, Exampleville,...",
            &[]
        )),
        Some("Sampleton, Exampleville,".to_owned())
    );
}
