//! The adapter over anonymized pages of the "salle virtuelle"
//! (`fixtures/visualys/`, written by `generate.py`), with the catalogue's
//! own settings.

use std::pin::pin;
use std::sync::Mutex;
use std::task::{Context, Poll, Waker};

use super::{Visualys, initial, page, wanted_blocks};
use crate::catalog::Collection;
use crate::citation::Act;
use crate::platform::{BoxFuture, Platform};
use crate::transport::{FetchError, Method, PortalFetch, PortalRequest};
use crate::{ArchiveRegistry, ArchiveTarget, ResolveError};

macro_rules! fixture {
    ($name:literal) => {
        include_str!(concat!("../../../fixtures/visualys/", $name))
    };
}

const LICENCE: &str = fixture!("licence.html");
const LIST_B: &str = fixture!("list-b.html");
const LIST_E: &str = fixture!("list-e.html");
const LIST_S: &str = fixture!("list-s.html");
const LOTS_CLOSED: &str = fixture!("lots-closed.html");
const LOTS_PARISH: &str = fixture!("lots-parish.html");
const LOTS_CIVIL: &str = fixture!("lots-civil.html");
const LOTS_BOTH: &str = fixture!("lots-both.html");
const LOTS_CIVIL_ONLY: &str = fixture!("lots-civil-only.html");
const MILITARY_FORM: &str = fixture!("military-form.html");
const MILITARY_RESULTS: &str = fixture!("military-results.html");
const MILITARY_NONE: &str = fixture!("military-none.html");

const ENTRY: &str =
    "https://sallevirtuelle.cotesdarmor.fr/EC/ecx/connexion.aspx?ref=demo&res=1920x1080";
const MILITARY_ENTRY: &str =
    "https://sallevirtuelle.cotesdarmor.fr/RM/rmx/connexion.aspx?ref=demo&res=1920x1080";
const BOURG: &str = "/EC/ecx/plage.aspx?id=900000000000011";

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

/// A portal answering each request by its method and address, a redirect
/// followed as the transports follow it.
struct Portal {
    routes: Vec<(Method, String, &'static str)>,
    requests: Mutex<Vec<PortalRequest>>,
}

impl Portal {
    fn new(routes: &[(Method, &str, &'static str)]) -> Self {
        Self {
            routes: routes
                .iter()
                .map(|(method, path, body)| (*method, (*path).to_owned(), *body))
                .collect(),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn paths(&self) -> Vec<String> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|request| request.url.clone())
            .collect()
    }

    fn bodies(&self) -> Vec<Option<String>> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|request| request.body.clone())
            .collect()
    }
}

impl PortalFetch for Portal {
    fn request<'a>(
        &'a self,
        request: &'a PortalRequest,
    ) -> BoxFuture<'a, Result<String, FetchError>> {
        Box::pin(async move {
            self.requests.lock().unwrap().push(request.clone());
            self.routes
                .iter()
                .find(|(method, path, _)| *method == request.method && *path == request.url)
                .map(|(_, _, body)| (*body).to_owned())
                .ok_or(FetchError::Status(404))
        })
    }
}

const GET: Method = Method::Get;
const POST: Method = Method::Post;

fn entry(base: &str) -> String {
    format!("{base}/connexion.aspx?ref=demo&res=1920x1080")
}

fn resolve(title: &str, portal: &Portal) -> Result<ArchiveTarget, ResolveError> {
    let registry = ArchiveRegistry::embedded();
    let citation = registry.parse(title).expect("a normalized citation");
    let (archive, collections) = registry.candidates(&citation).expect("a catalogued act");
    block_on(Visualys.resolve(archive, collections[0], &citation, portal))
}

/// The registers' site, with `lots` as each locality's lots page after
/// any toggle the adapter sends.
fn registers(list: &'static str, lots: &[(&str, &'static str)]) -> Portal {
    let entry = entry("/EC/ecx");
    let mut routes = vec![
        (GET, entry.as_str(), LICENCE),
        (GET, "/EC/ecx/commune.aspx?lettre=B", list),
        (GET, "/EC/ecx/commune.aspx?lettre=E", list),
        (GET, "/EC/ecx/commune.aspx?lettre=S", list),
    ];
    routes.extend(lots.iter().map(|(path, body)| (GET, *path, *body)));
    Portal::new(&routes)
}

fn register_target(view_count: Option<u16>) -> ArchiveTarget {
    ArchiveTarget::View {
        url: ENTRY.to_owned(),
        views: Vec::new(),
        view_count,
        call_number: None,
        attribution: None,
        renumbering: None,
    }
}

#[test]
fn finds_a_register_and_lands_on_the_site_entry_before_the_licence() {
    let toggled = format!("{BOURG}&r=1");
    let portal = registers(
        LIST_B,
        &[(BOURG, LOTS_CLOSED), (toggled.as_str(), LOTS_CIVIL)],
    );
    let target = resolve("AD22 - Le Bourg - (aucun) - N - 1795 - vue 30/248", &portal).unwrap();
    assert_eq!(target, register_target(Some(248)));
    // The visitor's entry, the initial's list, the lots, the civil block
    // opened: never the licence's acceptance, never a viewer.
    assert_eq!(
        portal.paths(),
        [
            entry("/EC/ecx"),
            "/EC/ecx/commune.aspx?lettre=B".to_owned(),
            BOURG.to_owned(),
            toggled
        ]
    );
    assert!(portal.bodies().iter().all(Option::is_none));
}

#[test]
fn opens_only_the_blocks_holding_the_act_that_are_closed() {
    let parish = format!("{BOURG}&r=0");
    let civil = format!("{BOURG}&r=1");
    // A marriage before 1793 is in the parish block alone.
    let portal = registers(
        LIST_B,
        &[(BOURG, LOTS_CLOSED), (parish.as_str(), LOTS_PARISH)],
    );
    let target = resolve("AD22 - Le Bourg - (aucun) - M - 1675", &portal).unwrap();
    assert_eq!(target, register_target(Some(611)));
    assert_eq!(portal.paths().last(), Some(&parish));

    // Without a year, both blocks, each opened by its own toggle: four
    // registers hold marriages.
    let portal = registers(
        LIST_B,
        &[
            (BOURG, LOTS_CLOSED),
            (parish.as_str(), LOTS_PARISH),
            (civil.as_str(), LOTS_BOTH),
        ],
    );
    let target = resolve("AD22 - Le Bourg - (aucun) - M", &portal).unwrap();
    assert_eq!(
        target,
        ArchiveTarget::Results {
            url: ENTRY.to_owned(),
            matches: Some(4)
        }
    );
    assert_eq!(portal.paths()[3..], [parish.clone(), civil.clone()]);

    // A block the session already holds open is not toggled, which would
    // close it.
    let portal = registers(LIST_B, &[(BOURG, LOTS_CIVIL)]);
    let target = resolve("AD22 - Le Bourg - (aucun) - TD - 1805", &portal).unwrap();
    assert_eq!(target, register_target(Some(14)));
    assert_eq!(portal.paths().len(), 3);
}

#[test]
fn chooses_the_row_of_the_parish_or_the_locality_s_own() {
    let parish_lots = "/EC/ecx/plage.aspx?id=900000000000022";
    let portal = registers(
        LIST_E,
        &[
            (parish_lots, LOTS_CLOSED),
            ("/EC/ecx/plage.aspx?id=900000000000022&r=0", LOTS_PARISH),
        ],
    );
    let target = resolve("AD22 - Exampleville - Saint-Exemple - B - 1685", &portal).unwrap();
    assert_eq!(target, register_target(Some(264)));

    // Without a parish, the locality's own row, whose lots have no parish
    // block.
    let own = "/EC/ecx/plage.aspx?id=900000000000021";
    let portal = registers(LIST_E, &[(own, LOTS_CIVIL_ONLY)]);
    let target = resolve("AD22 - Exampleville - (aucun) - D - 1800", &portal).unwrap();
    assert_eq!(target, register_target(Some(205)));
    assert_eq!(portal.paths().last().map(String::as_str), Some(own));

    // A locality listed only by its parishes: each of them.
    let portal = registers(
        LIST_S,
        &[
            ("/EC/ecx/plage.aspx?id=900000000000031", LOTS_PARISH),
            ("/EC/ecx/plage.aspx?id=900000000000032", LOTS_PARISH),
        ],
    );
    let target = resolve("AD22 - Sampleton - (aucun) - B - 1685", &portal).unwrap();
    assert_eq!(
        target,
        ArchiveTarget::Results {
            url: ENTRY.to_owned(),
            matches: Some(2)
        }
    );
}

#[test]
fn a_locality_the_list_does_not_name_finds_nothing() {
    let portal = registers(LIST_B, &[]);
    let target = resolve("AD22 - Bourgtown - (aucun) - N - 1795", &portal).unwrap();
    assert_eq!(
        target,
        ArchiveTarget::Results {
            url: ENTRY.to_owned(),
            matches: Some(0)
        }
    );
    assert_eq!(portal.paths().len(), 2);
}

#[test]
fn reports_a_changed_page_or_a_challenge() {
    let entry = entry("/EC/ecx");
    let portal = Portal::new(&[
        (GET, entry.as_str(), LICENCE),
        (GET, "/EC/ecx/commune.aspx?lettre=B", LICENCE),
    ]);
    assert!(matches!(
        resolve("AD22 - Le Bourg - (aucun) - N - 1795", &portal),
        Err(ResolveError::UnexpectedResponse(detail)) if detail.contains("visualys")
    ));
    let portal = Portal::new(&[
        (GET, entry.as_str(), LICENCE),
        (
            GET,
            "/EC/ecx/commune.aspx?lettre=B",
            "<html><title>Just a moment...</title><script>window._cf_chl_opt={}</script></html>",
        ),
    ]);
    assert_eq!(
        resolve("AD22 - Le Bourg - (aucun) - N - 1795", &portal),
        Err(ResolveError::Challenged)
    );
}

fn military(answer: &'static str) -> Portal {
    let entry = entry("/RM/rmx");
    Portal::new(&[
        (GET, entry.as_str(), LICENCE),
        (GET, "/RM/rmx/commune.aspx?lettre=*", MILITARY_FORM),
        (POST, "/RM/rmx/commune.aspx?lettre=*", answer),
    ])
}

#[test]
fn searches_the_military_registers_by_class_and_office() {
    let portal = military(MILITARY_RESULTS);
    let target = resolve(
        "AD22 - Exampleville - Registres matricules - 1900 - 01R 9002 - matricule 640",
        &portal,
    )
    .unwrap();
    assert_eq!(
        target,
        ArchiveTarget::View {
            url: MILITARY_ENTRY.to_owned(),
            views: Vec::new(),
            view_count: None,
            call_number: Some("01R9002".to_owned()),
            attribution: None,
            renumbering: None,
        }
    );
    let body = portal.bodies()[2].clone().unwrap();
    for field in [
        "__VIEWSTATE=STATE%2F1%2B2%3D",
        "__EVENTVALIDATION=CHECK%2F3%2B4%3D",
        "lstAnnee1=1900",
        "lstBureau=Exampleville",
        "lstRegistre=Registre%20matricule",
        "btnFind=Rechercher",
    ] {
        assert!(body.contains(field), "{field} in {body}");
    }

    // Two volumes of the office and class: the call number decides, and
    // without it the results.
    let portal = military(MILITARY_RESULTS);
    let target = resolve(
        "AD22 - Exampleville - Registres matricules - 1900 - matricule 640",
        &portal,
    )
    .unwrap();
    assert_eq!(
        target,
        ArchiveTarget::Results {
            url: MILITARY_ENTRY.to_owned(),
            matches: Some(2)
        }
    );

    // A locality naming no office searches every office of the class.
    let portal = military(MILITARY_RESULTS);
    let target = resolve(
        "AD22 - Bourgville - Registres matricules - 1900 - 01R9010",
        &portal,
    )
    .unwrap();
    assert!(
        matches!(target, ArchiveTarget::View { call_number: Some(call), .. } if call == "01R9010")
    );
    assert!(
        portal.bodies()[2]
            .as_deref()
            .unwrap()
            .contains("lstBureau=&")
    );

    let portal = military(MILITARY_NONE);
    let target = resolve("AD22 - Exampleville - Registres matricules - 1900", &portal).unwrap();
    assert_eq!(
        target,
        ArchiveTarget::Results {
            url: MILITARY_ENTRY.to_owned(),
            matches: Some(0)
        }
    );
}

#[test]
fn a_military_citation_without_a_class_or_an_office_is_not_searched() {
    let portal = military(MILITARY_RESULTS);
    let target = resolve("AD22 - Bourgville - Registres matricules", &portal).unwrap();
    assert_eq!(
        target,
        ArchiveTarget::Results {
            url: MILITARY_ENTRY.to_owned(),
            matches: None
        }
    );
    assert_eq!(portal.paths().len(), 2);
}

#[test]
fn reads_the_lists_and_the_blocks() {
    let listed = page::localities(LIST_E).unwrap();
    assert_eq!(listed.len(), 4);
    assert_eq!(listed[1].parish.as_deref(), Some("Saint-Exemple"));
    assert_eq!(listed[0].parish, None);
    assert_eq!(
        page::blocks(LOTS_CLOSED).unwrap(),
        page::Blocks {
            parish: Some(false),
            civil: Some(false)
        }
    );
    assert_eq!(
        page::blocks(LOTS_CIVIL_ONLY).unwrap(),
        page::Blocks {
            parish: None,
            civil: Some(true)
        }
    );
    let lots = page::lots(LOTS_BOTH);
    assert_eq!(lots.len(), 8);
    assert_eq!(lots[0].period, "1641-1681");
    assert_eq!(lots[0].act, "BMS");
    assert_eq!(lots[0].images, Some(611));
    assert!(page::lots(LOTS_CLOSED).is_empty());
}

#[test]
fn reads_the_search_form_and_its_answers() {
    let form = page::form(MILITARY_FORM, "/RM/rmx").unwrap();
    assert_eq!(form.action, "/RM/rmx/commune.aspx?lettre=*");
    assert_eq!(form.hidden.len(), 3);
    assert_eq!(page::options(MILITARY_FORM, "lstBureau").len(), 3);
    assert!(page::volumes(MILITARY_NONE).unwrap().is_empty());
    assert!(page::volumes(LICENCE).is_err());
}

#[test]
fn files_a_locality_under_its_initial_and_its_acts_under_their_blocks() {
    assert_eq!(initial("Bourg (Le)"), 'B');
    assert_eq!(initial("Étables"), 'E');
    let act = |code| Act::from_code(code).unwrap();
    assert_eq!(wanted_blocks(&act("B"), None), (true, false));
    assert_eq!(wanted_blocks(&act("N"), Some(1700)), (false, true));
    assert_eq!(wanted_blocks(&act("M"), Some(1792)), (true, false));
    assert_eq!(wanted_blocks(&act("M"), Some(1793)), (false, true));
    assert_eq!(wanted_blocks(&act("M"), None), (true, true));
    assert_eq!(wanted_blocks(&act("BMS"), Some(1800)), (true, false));
    assert_eq!(wanted_blocks(&act("TD"), None), (false, true));
}

fn collection(portal: serde_json::Value, acts: &[&str]) -> Collection {
    Collection {
        id: "registers".to_owned(),
        acts: acts
            .iter()
            .map(|code| Act::from_code(code).unwrap())
            .collect(),
        period: None,
        platform: "visualys".to_owned(),
        portal,
        insecure_http: false,
    }
}

#[test]
fn validates_the_settings() {
    let valid = serde_json::json!({
        "origin": "https://archives.example.org",
        "base": "/EC/ecx",
        "mode": "localities",
        "locality_style": "article_suffix"
    });
    assert_eq!(
        Visualys.validate(&collection(valid.clone(), &["N"])),
        Ok(())
    );
    assert!(Visualys.validate(&collection(valid, &["RM"])).is_err());
    let search = serde_json::json!({
        "origin": "https://archives.example.org",
        "base": "/RM/rmx",
        "mode": "search",
        "acts": { "RM": "Registre matricule" }
    });
    assert_eq!(
        Visualys.validate(&collection(search.clone(), &["RM"])),
        Ok(())
    );
    assert!(Visualys.validate(&collection(search, &["RP"])).is_err());
    for (field, value) in [
        ("origin", serde_json::json!("http://archives.example.org")),
        ("base", serde_json::json!("/EC/ecx/")),
        ("base", serde_json::json!("/ecx")),
        ("mode", serde_json::json!("other")),
        ("unknown", serde_json::json!(1)),
    ] {
        let mut portal = serde_json::json!({
            "origin": "https://archives.example.org",
            "base": "/EC/ecx",
            "mode": "localities"
        });
        portal[field] = value;
        assert!(
            Visualys.validate(&collection(portal, &["N"])).is_err(),
            "{field}"
        );
    }
}

#[test]
fn every_target_is_the_site_entry_and_the_window_starts_on_the_stylesheet() {
    let registry = ArchiveRegistry::embedded();
    let archive = registry.archive("AD22").unwrap();
    let citation = registry
        .parse("AD22 - Le Bourg - (aucun) - N - 1795")
        .unwrap();
    for (collection, entry) in archive.collections.iter().zip([ENTRY, MILITARY_ENTRY]) {
        assert_eq!(
            Visualys.results_url(collection, &citation).as_deref(),
            Some(entry)
        );
    }
    let endpoint = Visualys.endpoint(&archive.collections[1]).unwrap();
    assert_eq!(
        endpoint.start,
        "https://sallevirtuelle.cotesdarmor.fr/RM/slv.css"
    );
}
