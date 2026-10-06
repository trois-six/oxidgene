//! The adapter over anonymized answers (`fixtures/thot/`, written by its
//! `generate.py`), with the catalogue's own settings for the
//! Ille-et-Vilaine (`views: "ark"`) and Corsica (`views: "register"`)
//! archives.

use std::pin::pin;
use std::sync::Mutex;
use std::task::{Context, Poll, Waker};

use serde_json::json;

use super::{Settings, Thot};
use crate::catalog::Collection;
use crate::platform::{BoxFuture, Platform};
use crate::transport::{FetchError, Method, PortalFetch, PortalRequest};
use crate::{ArchiveRegistry, ArchiveTarget, ResolveError};

const DROITE: &str = include_str!("../../../fixtures/thot/droite.html");
const CHECKED: &str = include_str!("../../../fixtures/thot/checked.html");
const COOKIES_REFUSED: &str = include_str!("../../../fixtures/thot/cookies-refused.html");
const EXPIRED: &str = include_str!("../../../fixtures/thot/expired.html");
const MODULE: &str = include_str!("../../../fixtures/thot/module.html");
const HAUT: &str = include_str!("../../../fixtures/thot/haut.html");
const CHALLENGE: &str = include_str!("../../../fixtures/thot/challenge.html");
const FORM_REGISTERS: &str = include_str!("../../../fixtures/thot/form-registers.html");
const FORM_CENSUS: &str = include_str!("../../../fixtures/thot/form-census.html");
const FORM_MILITARY: &str = include_str!("../../../fixtures/thot/form-military.html");
const FORM_CORSE: &str = include_str!("../../../fixtures/thot/form-corse.html");
const FORM_SUCCESSIONS: &str = include_str!("../../../fixtures/thot/form-successions.html");
const RESULTS_SUCCESSIONS: &str = include_str!("../../../fixtures/thot/results-successions.html");
const RESULTS_ONE: &str = include_str!("../../../fixtures/thot/results-one.html");
const RESULTS_COPIES: &str = include_str!("../../../fixtures/thot/results-copies.html");
const RESULTS_TABLE: &str = include_str!("../../../fixtures/thot/results-table.html");
const RESULTS_RESTRICTED: &str = include_str!("../../../fixtures/thot/results-restricted.html");
const RESULTS_PAGE_1: &str = include_str!("../../../fixtures/thot/results-page-1.html");
const RESULTS_PAGE_2: &str = include_str!("../../../fixtures/thot/results-page-2.html");
const RESULTS_NONE: &str = include_str!("../../../fixtures/thot/results-none.html");
const RESULTS_CENSUS: &str = include_str!("../../../fixtures/thot/results-census.html");
const RESULTS_MILITARY: &str = include_str!("../../../fixtures/thot/results-military.html");
const RESULTS_CORSE: &str = include_str!("../../../fixtures/thot/results-corse.html");
const VIEWER: &str = include_str!("../../../fixtures/thot/viewer.html");
const SLIDES_ARK: &str = include_str!("../../../fixtures/thot/slides-ark.xml");
const SLIDES_PLAIN: &str = include_str!("../../../fixtures/thot/slides-plain.xml");

const IV: &str = "https://archives-en-ligne.ille-et-vilaine.fr/thot_internet";
const CORSE: &str = "https://archives.isula.corsica/Internet_THOT";

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

/// What the portal answers to a search's body, or to any other request it
/// is not the session's.
type Search = Box<dyn Fn(&PortalRequest) -> Option<&'static str> + Send + Sync>;

/// A portal answering the session's pages from the fixtures, the searches
/// through `search`, and recording every request.
struct Portal {
    base: &'static str,
    form: &'static str,
    /// The cookie check's answer.
    checked: &'static str,
    /// Whether the session is open already: the first page sends the browser
    /// on to the summary.
    open: bool,
    search: Search,
    requests: Mutex<Vec<PortalRequest>>,
}

impl Portal {
    fn new(
        base: &'static str,
        form: &'static str,
        search: impl Fn(&PortalRequest) -> Option<&'static str> + Send + Sync + 'static,
    ) -> Self {
        Self {
            base,
            form,
            checked: CHECKED,
            open: false,
            search: Box::new(search),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn requests(&self) -> Vec<PortalRequest> {
        self.requests.lock().unwrap().clone()
    }

    /// The bodies of the searches posted.
    fn bodies(&self) -> Vec<String> {
        self.requests()
            .into_iter()
            .filter(|request| request.method == Method::Post)
            .filter_map(|request| request.body)
            .collect()
    }

    fn answer(&self, request: &PortalRequest) -> Option<String> {
        let base = self.base;
        let path = request.url.as_str();
        let session = |page: &str| page.replace("/thot_internet", base);
        if path == format!("{base}/FrmAccueilDroite.asp") {
            return Some(session(if self.open { CHECKED } else { DROITE }));
        }
        let fixed = match path.strip_prefix(base) {
            Some("/FrmAccueilDroite.asp?checkCookie=20000101000000") => self.checked,
            Some(rest) if rest.starts_with("/Recherche/FrmRechFrame.asp?MOD=") => MODULE,
            Some(rest) if rest.starts_with("/Recherche/FrmRechHaut.asp?MOD=") => HAUT,
            Some(rest) if rest.starts_with("/Recherche/FrmRechDOCCritere.asp?MOD=") => self.form,
            _ => return (self.search)(request).map(str::to_owned),
        };
        Some(fixed.to_owned())
    }
}

impl PortalFetch for Portal {
    fn request<'a>(
        &'a self,
        request: &'a PortalRequest,
    ) -> BoxFuture<'a, Result<String, FetchError>> {
        Box::pin(async move {
            self.requests.lock().unwrap().push(request.clone());
            self.answer(request).ok_or(FetchError::Status(404))
        })
    }
}

/// The posted search, by what its body asks.
fn posted(request: &PortalRequest) -> Option<&str> {
    (request.method == Method::Post).then(|| request.body.as_deref().unwrap_or_default())
}

/// The viewer page and slide file of the Ille-et-Vilaine portal.
fn ark_viewer(request: &PortalRequest) -> Option<&'static str> {
    if request
        .url
        .starts_with("/thot_internet/FrmLotDocFrame.asp?")
    {
        Some(VIEWER)
    } else if request.url == "/thot_internet/download/thot/100000001/slides_00000001.xml" {
        Some(SLIDES_ARK)
    } else {
        None
    }
}

fn resolve(title: &str, collection: usize, portal: &Portal) -> Result<ArchiveTarget, ResolveError> {
    let registry = ArchiveRegistry::embedded();
    let citation = registry.parse(title).expect("a normalized citation");
    let archive = registry
        .archive(&citation.code)
        .expect("a catalogued archive");
    block_on(Thot.resolve(archive, &archive.collections[collection], &citation, portal))
}

fn resolved(title: &str, collection: usize, portal: &Portal) -> ArchiveTarget {
    resolve(title, collection, portal).unwrap_or_else(|error| panic!("{title}: {error}"))
}

fn matches_of(target: &ArchiveTarget) -> Option<usize> {
    match target {
        ArchiveTarget::Results { matches, .. } => *matches,
        ArchiveTarget::View { .. } => panic!("expected results, got {target:?}"),
    }
}

fn resolver(view: u16) -> String {
    format!("{IV}/gestionARK.asp?a=99999%2Fexmpl0000000%2F100001%2F{view}")
}

#[test]
fn resolves_a_register_to_the_ark_of_the_cited_view() {
    let portal = Portal::new("/thot_internet", FORM_REGISTERS, |request| {
        posted(request)
            .map(|_| RESULTS_ONE)
            .or_else(|| ark_viewer(request))
    });
    let target = resolved(
        "AD35 - Exampleville - (aucun) - N - 1850 - vue 2/3",
        0,
        &portal,
    );
    let ArchiveTarget::View {
        url,
        views,
        view_count,
        call_number,
        ..
    } = &target
    else {
        panic!("expected a view, got {target:?}");
    };
    assert_eq!(*url, resolver(2));
    assert_eq!(
        views[0].ark.as_deref(),
        Some(format!("{IV}/ark:/99999/exmpl0000000/100001/2").as_str())
    );
    assert_eq!(*view_count, Some(3));
    assert_eq!(call_number.as_deref(), Some("9 NUM 99001 12"));

    // The session, the module, its form, the search, the viewer and the
    // slide file, in this order.
    let urls: Vec<String> = portal.requests().into_iter().map(|r| r.url).collect();
    assert_eq!(
        urls,
        [
            "/thot_internet/FrmAccueilDroite.asp",
            "/thot_internet/FrmAccueilDroite.asp?checkCookie=20000101000000",
            "/thot_internet/Recherche/FrmRechFrame.asp?MOD=10",
            "/thot_internet/Recherche/FrmRechHaut.asp?MOD=10",
            "/thot_internet/Recherche/FrmRechDOCCritere.asp?MOD=10",
            "/thot_internet/Recherche/FrmRechDOCCritere.asp",
            "/thot_internet/FrmLotDocFrame.asp?idlot=700012&idfic=900012&ref=700012&appliCindoc=THOPDESC&resX=1400&resY=900&init=1&visionneuseHTML5=0",
            "/thot_internet/download/thot/100000001/slides_00000001.xml",
        ]
    );
    let body = &portal.bodies()[0];
    for field in [
        "txt_CIN_IDX0=1",
        "txt_CIN_CH0=EXAMPLEVILLE",
        "txt_CIN_IDX2=1",
        "txt_CIN_CH2=naissances",
        "txt_CIN_DEX_D=1850",
        "txt_CIN_DEX_F=1850",
        "b_ExecForm=1",
        "txt_IDX_OCC=0",
    ] {
        assert!(
            body.split('&').any(|pair| pair == field),
            "{field} in {body}"
        );
    }
    assert!(!body.contains("cbx_"), "{body}");
}

#[test]
fn searches_in_a_session_already_open() {
    // The window keeps the session between lookups: its first page sends
    // the browser on to the summary, and the cookie check is not asked.
    let mut portal = Portal::new("/thot_internet", FORM_REGISTERS, |request| {
        posted(request)
            .map(|_| RESULTS_ONE)
            .or_else(|| ark_viewer(request))
    });
    portal.open = true;
    let target = resolved(
        "AD35 - Exampleville - (aucun) - N - 1850 - vue 2/3",
        0,
        &portal,
    );
    assert_eq!(target.url(), resolver(2));
    assert!(
        !portal
            .requests()
            .iter()
            .any(|request| request.url.contains("checkCookie"))
    );
}

#[test]
fn opens_the_first_view_of_a_register_cited_beyond_its_views() {
    let portal = Portal::new("/thot_internet", FORM_REGISTERS, |request| {
        posted(request)
            .map(|_| RESULTS_ONE)
            .or_else(|| ark_viewer(request))
    });
    let target = resolved(
        "AD35 - Exampleville - (aucun) - N - 1850 - vue 7/9",
        0,
        &portal,
    );
    let ArchiveTarget::View { url, views, .. } = &target else {
        panic!("expected a view, got {target:?}");
    };
    assert!(views.is_empty());
    assert_eq!(*url, resolver(1));
}

#[test]
fn tells_two_copies_of_a_register_apart_by_their_call_number() {
    let portal = || {
        Portal::new("/thot_internet", FORM_REGISTERS, |request| {
            posted(request)
                .map(|_| RESULTS_COPIES)
                .or_else(|| ark_viewer(request))
        })
    };
    // The not digitized copy is not listed; the other two are the
    // communal and the clerk's copies of one year.
    let several = portal();
    let target = resolved("AD35 - Exampleville - (aucun) - N - 1793", 0, &several);
    assert_eq!(matches_of(&target), Some(2));
    assert_eq!(target.url(), format!("{IV}/FrmAccueilFrame.asp"));

    let named = portal();
    let target = resolved(
        "AD35 - Exampleville - (aucun) - N - 1793 - 9 NUM 99001 52 - vue 1/3",
        0,
        &named,
    );
    assert!(matches!(target, ArchiveTarget::View { .. }), "{target:?}");
    assert!(
        named
            .requests()
            .iter()
            .any(|request| request.url.contains("idlot=700052&idfic=900052"))
    );
}

#[test]
fn leaves_a_table_aside_for_a_cited_act() {
    let portal = Portal::new("/thot_internet", FORM_REGISTERS, |request| {
        posted(request)
            .map(|_| RESULTS_TABLE)
            .or_else(|| ark_viewer(request))
    });
    let target = resolved(
        "AD35 - Exampleville - (aucun) - B - 1700 - vue 1/3",
        0,
        &portal,
    );
    assert_eq!(target.url(), resolver(1));
    assert!(
        portal
            .requests()
            .iter()
            .any(|request| request.url.contains("idlot=700008"))
    );
}

#[test]
fn searches_the_label_writing_the_article_behind_the_name() {
    let portal = Portal::new("/thot_internet", FORM_REGISTERS, |request| {
        posted(request).map(|_| RESULTS_NONE)
    });
    let target = resolved("AD35 - Le Bourg-Exemple - (aucun) - B - 1700", 0, &portal);
    assert_eq!(matches_of(&target), Some(0));
    assert!(
        portal.bodies()[0].contains("txt_CIN_CH0=BOURG-EXEMPLE%20%28LE%29"),
        "{:?}",
        portal.bodies()
    );

    // A locality the list does not name is not searched.
    let unknown = Portal::new("/thot_internet", FORM_REGISTERS, |_| None);
    let target = resolved("AD35 - Elsewhere - (aucun) - B - 1700", 0, &unknown);
    assert_eq!(matches_of(&target), Some(0));
    assert!(unknown.bodies().is_empty());
}

#[test]
fn reads_the_next_page_until_the_citation_decides() {
    let portal = Portal::new("/thot_internet", FORM_REGISTERS, |request| {
        if posted(request).is_some() {
            Some(RESULTS_PAGE_1)
        } else if request.url == "/thot_internet/Recherche/FrmRechListeHaut.asp?RechDoc=1&page=2" {
            Some(RESULTS_PAGE_2)
        } else {
            ark_viewer(request)
        }
    });
    let target = resolved(
        "AD35 - Le Bourg-Exemple - (aucun) - N - 1841 - vue 1/3",
        0,
        &portal,
    );
    assert!(matches!(target, ArchiveTarget::View { .. }), "{target:?}");
    assert!(
        portal
            .requests()
            .iter()
            .any(|request| request.url.contains("idlot=710041"))
    );
}

#[test]
fn skips_a_restricted_register() {
    let portal = Portal::new("/thot_internet", FORM_REGISTERS, |request| {
        posted(request).map(|_| RESULTS_RESTRICTED)
    });
    let target = resolved("AD35 - Exampleville - (aucun) - D - 1925", 0, &portal);
    assert_eq!(matches_of(&target), Some(1));
}

#[test]
fn searches_a_census_by_its_years_interval() {
    let portal = Portal::new("/thot_internet", FORM_CENSUS, |request| {
        posted(request)
            .map(|_| RESULTS_CENSUS)
            .or_else(|| ark_viewer(request))
    });
    let target = resolved(
        "AD35 - Exampleville - Recensement - 1856 - vue 2/3",
        1,
        &portal,
    );
    assert_eq!(target.url(), resolver(2));
    let body = &portal.bodies()[0];
    for field in [
        "txt_CIN_CH0=EXAMPLEVILLE",
        "txt_CIN_CH1=1856%7C1856",
        "intervalleDate1_1=1856",
        "intervalleDate2_1=1856",
    ] {
        assert!(
            body.split('&').any(|pair| pair == field),
            "{field} in {body}"
        );
    }
    // The plain label is preferred to the district one.
    assert!(!body.contains("NORD-EST"), "{body}");
}

#[test]
fn chooses_a_military_register_by_its_matricules() {
    let portal = Portal::new("/thot_internet", FORM_MILITARY, |request| {
        posted(request)
            .map(|_| RESULTS_MILITARY)
            .or_else(|| ark_viewer(request))
    });
    let target = resolved(
        "AD35 - Sampleton - Registres matricules - 1900 - matricule 600 - vue 2/3",
        2,
        &portal,
    );
    let ArchiveTarget::View { call_number, .. } = &target else {
        panic!("expected a view, got {target:?}");
    };
    assert_eq!(call_number.as_deref(), Some("9 R 992"));
    // `SAMPLETON` is written as cited: the subdivision's and the district's
    // labels are not searched.
    let bodies = portal.bodies();
    assert_eq!(bodies.len(), 1);
    assert!(bodies[0].contains("txt_CIN_CH0=SAMPLETON&"), "{bodies:?}");
    assert!(
        bodies[0].contains("txt_CIN_CH1=REGISTRES%20MATRICULES"),
        "{bodies:?}"
    );
    assert!(bodies[0].contains("txt_CIN_CH2=1900%7C1900"), "{bodies:?}");
}

#[test]
fn searches_successions_by_office_and_chooses_by_period() {
    // The portal finds a table of successions by a year within its extreme
    // dates only when they are that year: the year is not sent.
    let portal = Portal::new("/thot_internet", FORM_SUCCESSIONS, |request| {
        posted(request)
            .map(|_| RESULTS_SUCCESSIONS)
            .or_else(|| ark_viewer(request))
    });
    let target = resolved(
        "AD35 - Exampleville - Tables de successions - 1822 - vue 2/3",
        3,
        &portal,
    );
    let ArchiveTarget::View { call_number, .. } = &target else {
        panic!("expected a view, got {target:?}");
    };
    assert_eq!(call_number.as_deref(), Some("9 Q 9/5"));
    let body = &portal.bodies()[0];
    assert!(
        body.contains("txt_CIN_CH0=EXAMPLEVILLE%20%28BUREAU%20DE%20L%27ENREGISTREMENT%29"),
        "{body}"
    );
    assert!(!body.contains("1822"), "{body}");
}

#[test]
fn opens_a_corsican_register_on_its_first_view_without_opening_it() {
    let portal = Portal::new("/Internet_THOT", FORM_CORSE, |request| {
        posted(request).map(|_| RESULTS_CORSE)
    });
    let target = resolved(
        "AD2B - Exampleville - (aucun) - N - 1876 - vue 12/40",
        0,
        &portal,
    );
    assert_eq!(
        target,
        ArchiveTarget::View {
            url: format!(
                "{CORSE}/FrmLotDocFrame.asp?idlot=THOTDESC_740003&idfic=940003&ref=740003&appliCindoc=THOTDESC&resX=1400&resY=900&init=1&visionneuseHTML5=0"
            ),
            views: Vec::new(),
            view_count: None,
            call_number: Some("99 NUM 3".to_owned()),
            attribution: None,
        }
    );
    // The viewer, whose every opening writes a slide file on the server, is
    // left to the window.
    assert!(
        !portal
            .requests()
            .iter()
            .any(|request| request.url.contains("FrmLotDocFrame"))
    );
    let body = &portal.bodies()[0];
    for field in [
        "txt_CIN_CH0=EXAMPLEVILLE%20%28EXEMPLE%2C%20FRANCE%29",
        "txt_CIN_CH2=NAISSANCES",
        "cbx_txt_CIN_CH2=NAISSANCES",
    ] {
        assert!(
            body.split('&').any(|pair| pair == field),
            "{field} in {body}"
        );
    }
}

#[test]
fn reads_the_acts_of_a_corsican_title() {
    // The table of the same years is set apart by its title.
    let portal = Portal::new("/Internet_THOT", FORM_CORSE, |request| {
        posted(request).map(|_| RESULTS_CORSE)
    });
    let target = resolved("AD2A - Exampleville - (aucun) - N - 1873", 0, &portal);
    assert!(target.url().contains("idlot=THOTDESC_740001"), "{target:?}");
    let tables = Portal::new("/Internet_THOT", FORM_CORSE, |request| {
        posted(request).map(|_| RESULTS_CORSE)
    });
    let target = resolved("AD2A - Exampleville - (aucun) - TD - 1873", 0, &tables);
    assert!(target.url().contains("idlot=THOTDESC_740005"), "{target:?}");
}

#[test]
fn reports_a_changed_portal_and_a_challenge_apart() {
    let answering = |answer: &'static str| {
        let portal = Portal::new("/thot_internet", FORM_REGISTERS, move |request| {
            posted(request).map(|_| answer)
        });
        resolve("AD35 - Exampleville - (aucun) - N - 1850", 0, &portal).unwrap_err()
    };
    let changed = |error: ResolveError, expected: &str| match error {
        ResolveError::UnexpectedResponse(detail) => assert!(detail.contains(expected), "{detail}"),
        other => panic!("expected a changed shape, got {other:?}"),
    };
    changed(answering(EXPIRED), "session expired");
    changed(answering(HAUT), "no count");
    assert_eq!(answering(CHALLENGE), ResolveError::Challenged);

    // The cookie check refusing the session.
    let mut portal = Portal::new("/thot_internet", FORM_REGISTERS, |_| None);
    portal.checked = COOKIES_REFUSED;
    changed(
        resolve("AD35 - Exampleville - (aucun) - N - 1850", 0, &portal).unwrap_err(),
        "session cookie",
    );

    // A slide file without ARKs where the settings expect them.
    let portal = Portal::new("/thot_internet", FORM_REGISTERS, |request| {
        if posted(request).is_some() {
            Some(RESULTS_ONE)
        } else if request.url.contains("slides_") {
            Some(SLIDES_PLAIN)
        } else {
            ark_viewer(request)
        }
    });
    changed(
        resolve("AD35 - Exampleville - (aucun) - N - 1850", 0, &portal).unwrap_err(),
        "ARK",
    );
}

#[test]
fn starts_on_the_stylesheet_and_lands_on_the_home_page() {
    let registry = ArchiveRegistry::embedded();
    let archive = registry.archive("AD2A").unwrap();
    let collection = &archive.collections[0];
    let endpoint = Thot.endpoint(collection).unwrap();
    assert_eq!(endpoint.start, format!("{CORSE}/css/general.css"));
    assert_eq!(endpoint.access, crate::platform::Access::Browser);
    let citation = registry
        .parse("AD2A - Exampleville - (aucun) - N - 1870")
        .unwrap();
    assert_eq!(
        Thot.results_url(collection, &citation).as_deref(),
        Some(format!("{CORSE}/FrmAccueilFrame.asp").as_str())
    );
}

fn collection(portal: serde_json::Value, acts: &[&str]) -> Collection {
    serde_json::from_value(json!({
        "id": "registers",
        "acts": acts,
        "platform": "thot",
        "portal": portal,
    }))
    .unwrap()
}

#[test]
fn validates_its_settings() {
    let valid = json!({
        "origin": "https://archives.example.org",
        "base": "/thot",
        "module": 10,
        "criteria": {"locality": 0, "act": 2, "year": "dex"},
        "acts": {"B": "baptemes", "TD": "tables"},
        "views": "ark"
    });
    assert!(Settings::read(&collection(valid.clone(), &["B", "TD"])).is_ok());
    // A combined act is searched by its first kind.
    assert!(Settings::read(&collection(valid.clone(), &["BMS"])).is_ok());
    let census = json!({
        "origin": "https://archives.example.org",
        "base": "/thot",
        "module": 16,
        "criteria": {"locality": 0, "year": 1},
        "views": "register"
    });
    assert!(Settings::read(&collection(census, &["RP"])).is_ok());

    let with = |key: &str, value: serde_json::Value| {
        let mut settings = valid.clone();
        settings[key] = value;
        settings
    };
    for (settings, acts) in [
        (
            with("origin", json!("http://archives.example.org")),
            vec!["B"],
        ),
        (with("base", json!("/thot/")), vec!["B"]),
        (with("module", json!(0)), vec!["B"]),
        (with("views", json!("page")), vec!["B"]),
        (
            with("criteria", json!({"locality": 0, "act": 0, "year": "dex"})),
            vec!["B"],
        ),
        (
            with(
                "criteria",
                json!({"locality": 0, "act": 2, "year": "years"}),
            ),
            vec!["B"],
        ),
        (
            with("criteria", json!({"locality": 0, "year": "dex"})),
            vec!["B"],
        ),
        (with("acts", json!({"B": "bapt\u{ea}mes"})), vec!["B"]),
        (with("acts", json!({"X": "x"})), vec!["B"]),
        (with("unknown", json!(1)), vec!["B"]),
        // An act the settings have no value for.
        (valid.clone(), vec!["N"]),
    ] {
        assert!(
            Settings::read(&collection(settings.clone(), &acts)).is_err(),
            "{settings} {acts:?}"
        );
    }
}
