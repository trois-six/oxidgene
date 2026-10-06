//! The adapter over anonymized GAIA pages (`fixtures/gaia/`, written by
//! `generate.py`), with the catalogue's own settings. The pages are
//! ISO-8859-1 and read as the transports read them, decoded as UTF-8 with
//! a U+FFFD for each accented letter, unless a test says otherwise.

use std::pin::pin;
use std::sync::Mutex;
use std::task::{Context, Poll, Waker};

use super::page;
use crate::platform::BoxFuture;
use crate::transport::{FetchError, Method, PortalFetch, PortalRequest};
use crate::{ArchiveRegistry, ArchiveTarget, ResolveError};

macro_rules! fixture {
    ($name:literal) => {
        include_bytes!(concat!("../../../fixtures/gaia/", $name))
    };
}

const LIST_E: &[u8] = fixture!("list-e.html");
const TYPES: &[u8] = fixture!("types.html");
const DATES: &[u8] = fixture!("dates.html");
const DATED: &[u8] = fixture!("dated.html");
const ONE: &[u8] = fixture!("results-one.html");
const SEVERAL: &[u8] = fixture!("results-several.html");
const NONE: &[u8] = fixture!("results-none.html");
const MAINTENANCE: &[u8] = fixture!("maintenance.html");
const VIEWER: &[u8] = fixture!("viewer.html");

const ARIEGE: &str = "https://mdr-archives.ariege.fr";
const WIZARD: &str = "/mdr/index.php/rechercheTheme/requeteConstructor";

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

/// A portal answering each request by its method and path; a search
/// (`…/T/0/0`), with `searches`, by the locality entry chosen last, as the
/// portal's session would.
struct Portal {
    routes: Vec<(Method, String, &'static [u8])>,
    searches: Vec<(String, &'static [u8])>,
    decode: fn(&[u8]) -> String,
    requests: Mutex<Vec<PortalRequest>>,
}

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| char::from(*byte)).collect()
}

impl Portal {
    fn requests(&self) -> Vec<(Method, String, Option<String>)> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|request| (request.method, request.url.clone(), request.body.clone()))
            .collect()
    }
}

impl PortalFetch for Portal {
    fn request<'a>(
        &'a self,
        request: &'a PortalRequest,
    ) -> BoxFuture<'a, Result<String, FetchError>> {
        Box::pin(async move {
            let mut requests = self.requests.lock().unwrap();
            requests.push(request.clone());
            let entry = requests.iter().rev().find_map(|earlier| {
                self.searches
                    .iter()
                    .find(|(path, _)| *path == earlier.url)
                    .map(|(_, answer)| *answer)
            });
            if request.url.ends_with("/T/0/0")
                && let Some(answer) = entry
            {
                return Ok((self.decode)(answer));
            }
            self.routes
                .iter()
                .find(|(method, path, _)| *method == request.method && *path == request.url)
                .map(|(_, _, body)| (self.decode)(body))
                .ok_or(FetchError::Status(404))
        })
    }
}

fn get(path: &str, body: &'static [u8]) -> (Method, String, &'static [u8]) {
    (Method::Get, path.to_owned(), body)
}

fn post(path: &str, body: &'static [u8]) -> (Method, String, &'static [u8]) {
    (Method::Post, path.to_owned(), body)
}

fn portal(routes: Vec<(Method, String, &'static [u8])>) -> Portal {
    Portal {
        routes,
        searches: Vec::new(),
        decode: lossy,
        requests: Mutex::new(Vec::new()),
    }
}

fn resolve(title: &str, portal: &Portal) -> Result<ArchiveTarget, ResolveError> {
    let registry = ArchiveRegistry::embedded();
    let citation = registry.parse(title).expect("a normalized citation");
    let (archive, collections) = registry.candidates(&citation).expect("a catalogued act");
    let platform = registry.platform(&collections[0].platform).unwrap();
    block_on(platform.resolve(archive, collections[0], &citation, portal))
}

/// The Ariège wizard: the registers of a type of a commune of the letter
/// E, with `answer` as the search's, from the commune's entry or from its
/// parish's and its protestant church's.
fn wizard(type_id: u32, answer: &'static [u8]) -> Portal {
    portal(vec![
        get(&format!("{WIZARD}/1/1/R/E/0"), LIST_E),
        get(&format!("{WIZARD}/1/1/A/900102/x"), TYPES),
        get(&format!("{WIZARD}/1/1/A/900103/x"), TYPES),
        get(&format!("{WIZARD}/1/1/A/900104/x"), TYPES),
        get(&format!("{WIZARD}/1/2/A/{type_id}/x"), DATES),
        post(&format!("{WIZARD}/1/3/A/0/0"), DATED),
        post(&format!("{WIZARD}/1/3/T/0/0"), answer),
    ])
}

fn births(answer: &'static [u8]) -> Portal {
    wizard(900202, answer)
}

fn register(path: &str, call_number: &str) -> ArchiveTarget {
    ArchiveTarget::View {
        url: format!("{ARIEGE}/mdr/index.php/docnumViewer/calculHierarchieDocNum/{path}/900/1400"),
        views: Vec::new(),
        view_count: None,
        call_number: Some(call_number.to_owned()),
        attribution: None,
    }
}

#[test]
fn a_register_opens_in_the_viewer_on_its_first_view() {
    let portal = births(ONE);
    let target = resolve(
        "AD09 - Exampleville - (aucun) - N - 1850 - 9 NUM / 4 E 1 - vue 5/40",
        &portal,
    )
    .unwrap();
    assert_eq!(
        target,
        register("900301/900001:900102:900202:900301", "9NUM/4E1")
    );
    assert_eq!(
        portal.requests(),
        [
            (Method::Get, format!("{WIZARD}/1/1/R/E/0"), None),
            (Method::Get, format!("{WIZARD}/1/1/A/900102/x"), None),
            (Method::Get, format!("{WIZARD}/1/2/A/900202/x"), None),
            (
                Method::Post,
                format!("{WIZARD}/1/3/A/0/0"),
                Some("typeDate=simple&dateDeb=&dateFin=&dateSimple=1850".to_owned())
            ),
            (
                Method::Post,
                format!("{WIZARD}/1/3/T/0/0"),
                Some("forcepost=essai".to_owned())
            ),
        ]
    );
}

#[test]
fn reads_the_pages_decoded_as_latin_1_too() {
    let mut portal = births(ONE);
    portal.decode = latin1;
    let target = resolve("AD09 - Exampleville - (aucun) - N - 1850", &portal).unwrap();
    assert_eq!(
        target,
        register("900301/900001:900102:900202:900301", "9NUM/4E1")
    );
}

#[test]
fn a_type_label_with_a_lost_letter_is_still_found() {
    // Decoded as UTF-8, `décès` reads `d\u{fffd}c\u{fffd}s`.
    let portal = wizard(900206, NONE);
    let _ = resolve("AD09 - Exampleville - (aucun) - D - 1850", &portal);
    assert_eq!(portal.requests()[2].1, format!("{WIZARD}/1/2/A/900206/x"));
}

#[test]
fn a_commune_entry_without_the_register_has_its_other_entries_searched() {
    let mut portal = wizard(900201, ONE);
    portal.searches = vec![
        (format!("{WIZARD}/1/1/A/900102/x"), ONE),
        (
            format!("{WIZARD}/1/1/A/900103/x"),
            fixture!("results-parish.html"),
        ),
        (format!("{WIZARD}/1/1/A/900104/x"), NONE),
    ];
    // The commune's entry lists births only: its parish's lists the
    // baptisms.
    let target = resolve("AD09 - Exampleville - (aucun) - B - 1750", &portal).unwrap();
    assert_eq!(
        target,
        register("900305/900001:900103:900201:900305", "9E99/1")
    );
    let entries: Vec<String> = portal
        .requests()
        .into_iter()
        .map(|(_, url, _)| url)
        .filter(|url| url.contains("/1/1/A/"))
        .collect();
    assert_eq!(entries.len(), 3);
}

#[test]
fn several_registers_are_told_apart_by_call_number_or_period() {
    let target = resolve(
        "AD09 - Exampleville - (aucun) - N - 1850 - 9 NUM 1 / 4 E 2",
        &births(SEVERAL),
    )
    .unwrap();
    assert_eq!(
        target,
        register("900303/900001:900102:900202:900303", "9NUM1/4E2")
    );
    // 1835 lies in `An XI-1872` alone.
    let target = resolve("AD09 - Exampleville - (aucun) - N - 1835", &births(SEVERAL)).unwrap();
    assert_eq!(
        target,
        register("900302/900001:900102:900202:900302", "9NUM/4E1")
    );
    // 1850 lies in both registers with images.
    let target = resolve("AD09 - Exampleville - (aucun) - N - 1850", &births(SEVERAL)).unwrap();
    assert_eq!(
        target,
        ArchiveTarget::Results {
            url: format!("{ARIEGE}{WIZARD}/1/1/R/E/0"),
            matches: Some(2),
        }
    );
}

#[test]
fn no_register_no_locality_and_no_type_give_no_match() {
    let none = |title: &str, portal: &Portal| {
        assert_eq!(
            resolve(title, portal).unwrap(),
            ArchiveTarget::Results {
                url: format!("{ARIEGE}{WIZARD}/1/1/R/E/0"),
                matches: Some(0),
            },
            "{title}"
        );
    };
    none("AD09 - Exampleville - (aucun) - N - 1850", &births(NONE));
    let portal = births(ONE);
    none("AD09 - Elsewhere - (aucun) - N - 1850", &portal);
    assert_eq!(portal.requests().len(), 1);
}

#[test]
fn a_parish_entry_serves_the_cited_parish_and_a_commune_its_own() {
    let commune = |id: &str| {
        portal(vec![
            get(&format!("{WIZARD}/1/1/R/E/0"), LIST_E),
            get(&format!("{WIZARD}/1/1/A/{id}/x"), TYPES),
        ])
    };
    let portal = commune("900103");
    let _ = resolve("AD09 - Exampleville - Saint-Exemple - B - 1700", &portal);
    assert_eq!(portal.requests()[1].1, format!("{WIZARD}/1/1/A/900103/x"));
    // A qualifier in parentheses or brackets is no parish.
    for (title, id) in [
        ("AD09 - Exemple-le-Haut - (aucun) - B - 1700", "900107"),
        ("AD09 - Exemple-sur-Mer - (aucun) - B - 1700", "900105"),
        ("AD09 - L'Ermite - (aucun) - B - 1700", "900101"),
        ("AD09 - Étang-Exemple - (aucun) - B - 1700", "900106"),
    ] {
        let portal = commune(id);
        let _ = resolve(title, &portal);
        assert_eq!(
            portal.requests()[1].1,
            format!("{WIZARD}/1/1/A/{id}/x"),
            "{title}"
        );
    }
}

#[test]
fn a_page_of_another_shape_is_drift() {
    let portal = portal(vec![get(&format!("{WIZARD}/1/1/R/E/0"), MAINTENANCE)]);
    let error = resolve("AD09 - Exampleville - (aucun) - N - 1850", &portal).unwrap_err();
    assert!(
        matches!(&error, ResolveError::UnexpectedResponse(detail) if detail.starts_with("gaia: ")),
        "{error:?}"
    );
}

#[test]
fn a_census_without_a_year_form_is_searched_through_its_pages() {
    let census = |title: &str| {
        let portal = portal(vec![
            get(&format!("{WIZARD}/2/1/R/E/0"), fixture!("census-list.html")),
            get(
                &format!("{WIZARD}/2/1/A/900401/x"),
                fixture!("census-commune.html"),
            ),
            post(
                &format!("{WIZARD}/2/2/T/0/0"),
                fixture!("census-page-0.html"),
            ),
            get(
                "/mdr/index.php/rechercheTheme/paginer/20",
                fixture!("census-page-1.html"),
            ),
            get(
                "/mdr/index.php/rechercheTheme/paginer/40",
                fixture!("census-page-2.html"),
            ),
        ]);
        let target = resolve(title, &portal).unwrap();
        (target, portal.requests())
    };
    let (target, requests) = census("AD61 - Exampleville - (aucun) - RP - 1830");
    let ArchiveTarget::View { call_number, .. } = target else {
        panic!("a register: {target:?}");
    };
    assert_eq!(call_number.as_deref(), Some("9NUM6M1/29"));
    // The list, the commune, the search, then the page holding 1830.
    assert_eq!(requests.len(), 4);
    assert_eq!(requests[3].1, "/mdr/index.php/rechercheTheme/paginer/20");

    let (target, requests) = census("AD61 - Exampleville - (aucun) - RP - 1845");
    assert!(matches!(target, ArchiveTarget::View { .. }), "{target:?}");
    assert_eq!(requests.len(), 5);
    // Early years are on the first page.
    let (_, requests) = census("AD61 - Exampleville - (aucun) - RP - 1805");
    assert_eq!(requests.len(), 3);
    // Without a year, the count.
    let (target, _) = census("AD61 - Exampleville - (aucun) - RP");
    assert_eq!(
        target,
        ArchiveTarget::Results {
            url: format!("https://gaia.orne.fr{WIZARD}/2/1/R/E/0"),
            matches: Some(45),
        }
    );
}

#[test]
fn a_search_without_localities_reads_the_locality_from_the_titles() {
    let military = || {
        portal(vec![
            get(&format!("{WIZARD}/3/1/R/0/0"), fixture!("kinds.html")),
            get(
                &format!("{WIZARD}/3/1/A/900502/x"),
                fixture!("class-dates.html"),
            ),
            post(&format!("{WIZARD}/3/2/A/0/0"), fixture!("class-dated.html")),
            post(&format!("{WIZARD}/3/3/T/0/0"), fixture!("military.html")),
        ])
    };
    let target = resolve(
        "AD61 - Exampleville - (aucun) - RM - 1890 - n° 600",
        &military(),
    )
    .unwrap();
    let ArchiveTarget::View { call_number, .. } = target else {
        panic!("a register: {target:?}");
    };
    assert_eq!(call_number.as_deref(), Some("R9051"));
    // Another bureau's volume.
    let target = resolve(
        "AD61 - Autreville - (aucun) - RM - 1890 - n° 300",
        &military(),
    )
    .unwrap();
    assert!(matches!(target, ArchiveTarget::View { call_number: Some(ref c), .. } if c == "R9053"));
    // A locality no title names: every volume holding the number.
    let target = resolve(
        "AD61 - Elsewhere - (aucun) - RM - 1890 - n° 300",
        &military(),
    )
    .unwrap();
    assert_eq!(
        target,
        ArchiveTarget::Results {
            url: format!("https://gaia.orne.fr{WIZARD}/3/1/R/0/0"),
            matches: Some(2),
        }
    );
}

#[test]
fn a_search_without_kinds_reads_the_acts_from_the_titles() {
    let wizard = || {
        portal(vec![
            get(&format!("{WIZARD}/14/1/R/L/0"), fixture!("list-l.html")),
            get(
                &format!("{WIZARD}/14/1/A/900701/x"),
                fixture!("dates-14.html"),
            ),
            post(&format!("{WIZARD}/14/4/A/0/0"), fixture!("dated-14.html")),
            post(
                &format!("{WIZARD}/14/4/T/0/0"),
                fixture!("results-titles.html"),
            ),
        ])
    };
    let origin = "https://archives-en-ligne.seine-et-marne.fr";
    let target = resolve("AD77 - La Ville-Exemple - (aucun) - N - 1850", &wizard()).unwrap();
    assert_eq!(
        target,
        ArchiveTarget::View {
            url: format!(
                "{origin}/mdr/index.php/docnumViewer/calculHierarchieDocNum/900803/900004:900007:900701:900803/900/1400"
            ),
            views: Vec::new(),
            view_count: None,
            call_number: Some("5MI999".to_owned()),
            attribution: None,
        }
    );
    let target = resolve("AD77 - La Ville-Exemple - (aucun) - TD - 1850", &wizard()).unwrap();
    let ArchiveTarget::View {
        url, call_number, ..
    } = target
    else {
        panic!("a table: {target:?}");
    };
    assert!(url.contains("/900801/"), "{url}");
    assert_eq!(call_number, None);
}

#[test]
fn a_two_level_portal_follows_each_label_and_skips_an_unnamed_list() {
    let base = "/mdr_aude/index.php/rechercheTheme/requeteConstructor";
    let wizard = |answer: &'static [u8]| {
        portal(vec![
            get(
                &format!("{base}/1/1/R/E/0"),
                fixture!("two-level-list.html"),
            ),
            get(
                &format!("{base}/1/1/A/901001/x"),
                fixture!("categories.html"),
            ),
            get(&format!("{base}/1/2/A/901101/x"), fixture!("acts.html")),
            get(&format!("{base}/1/2/A/901102/x"), fixture!("acts.html")),
            get(&format!("{base}/1/2/A/901103/x"), fixture!("acts.html")),
            get(
                &format!("{base}/1/3/A/901203/x"),
                fixture!("two-level-dates.html"),
            ),
            post(
                &format!("{base}/1/4/F/0/0"),
                fixture!("two-level-dates.html"),
            ),
            post(
                &format!("{base}/1/5/A/0/0"),
                fixture!("two-level-dated.html"),
            ),
            post(&format!("{base}/1/5/T/0/0"), answer),
        ])
    };
    let portal = wizard(fixture!("two-level-one.html"));
    let target = resolve("AD11 - Exampleville - (aucun) - D - 1850", &portal).unwrap();
    assert!(
        matches!(&target, ArchiveTarget::View { call_number: Some(c), .. } if c == "99NUM/5E9/15"),
        "{target:?}"
    );
    assert_eq!(portal.requests().len(), 6);

    // The parish registers' category is written with its period.
    let portal = wizard(fixture!("two-level-one.html"));
    let _ = resolve("AD11 - Exampleville - (aucun) - B - 1750", &portal);
    assert_eq!(portal.requests()[2].1, format!("{base}/1/2/A/901101/x"));

    let portal = wizard(fixture!("two-level-tables.html"));
    let target = resolve("AD11 - Exampleville - (aucun) - TD - 1850", &portal).unwrap();
    assert!(
        matches!(&target, ArchiveTarget::View { call_number: Some(c), .. } if c == "99NUM/5E9/20"),
        "{target:?}"
    );
    let requests = portal.requests();
    assert_eq!(
        requests[3],
        (
            Method::Post,
            format!("{base}/1/4/F/0/0"),
            Some(String::new())
        )
    );
}

#[test]
fn a_list_of_years_is_followed_to_the_cited_one() {
    let base = "/mdr_aude/index.php/rechercheTheme/requeteConstructor";
    let portal = portal(vec![
        get(
            &format!("{base}/3/1/R/E/0"),
            fixture!("census-years-list.html"),
        ),
        get(&format!("{base}/3/1/A/901400/x"), fixture!("years.html")),
        get(
            &format!("{base}/3/2/A/901402/x"),
            fixture!("year-dated.html"),
        ),
        post(&format!("{base}/3/3/T/0/0"), fixture!("year-one.html")),
    ]);
    let target = resolve("AD11 - Exampleville - (aucun) - RP - 1846", &portal).unwrap();
    assert!(matches!(target, ArchiveTarget::View { .. }), "{target:?}");
    assert_eq!(portal.requests().len(), 4);
}

#[test]
fn builds_the_results_page_without_requests() {
    let registry = ArchiveRegistry::embedded();
    for (title, expected) in [
        (
            "AD09 - Le Bourg - (aucun) - N - 1850",
            format!("{ARIEGE}{WIZARD}/1/1/R/B/0"),
        ),
        (
            "AD77 - La Ville-Exemple - (aucun) - N - 1850",
            format!("https://archives-en-ligne.seine-et-marne.fr{WIZARD}/14/1/R/L/0"),
        ),
        (
            "AD77 - Exampleville - (aucun) - TSA - 1850",
            format!("https://archives-en-ligne.seine-et-marne.fr{WIZARD}/22/1/R/0/0"),
        ),
    ] {
        let citation = registry.parse(title).unwrap();
        let (_, collections) = registry.candidates(&citation).unwrap();
        let platform = registry.platform("gaia").unwrap();
        assert_eq!(
            platform.results_url(collections[0], &citation).as_deref(),
            Some(expected.as_str()),
            "{title}"
        );
    }
}

#[test]
fn counts_the_views_of_a_viewer_page() {
    assert_eq!(page::views(&lossy(VIEWER)), Some(3));
    assert_eq!(page::views(&lossy(MAINTENANCE)), None);
}

#[test]
fn reads_a_locality_label() {
    for (label, prefix, name, detail) in [
        ("EXAMPLEVILLE", None, "EXAMPLEVILLE", ""),
        ("Bourg (Le)", None, "Bourg (Le)", ""),
        (
            "Bourg (Le), paroisse Saint-Exemple",
            None,
            "Bourg (Le)",
            "paroisse Saint-Exemple",
        ),
        ("Exampleville (après 1793)", None, "Exampleville", ""),
        (
            "La Ville-Exemple (Département)",
            None,
            "La Ville-Exemple",
            "",
        ),
        ("AVAL (L') [jusqu'en 1789]", None, "AVAL (L')", ""),
        (
            "Exampleville. - Subdivision",
            None,
            "Exampleville",
            "- Subdivision",
        ),
        (
            "EXAMPLEVILLE, 1843-1968 (avant : voir Aval)",
            None,
            "EXAMPLEVILLE",
            "1843-1968 (avant : voir Aval)",
        ),
        (
            "Bureau de Exampleville",
            Some("Bureau de "),
            "Exampleville",
            "",
        ),
    ] {
        let entry = page::entry(label, prefix);
        assert_eq!((entry.name, entry.detail), (name, detail), "{label}");
    }
}

#[test]
fn reads_the_acts_of_a_title() {
    for (title, act) in [
        ("EXAMPLEVILLE. Naissances. ", Some("N")),
        ("Naissances, mariages, d\u{fffd}c\u{fffd}s.", Some("NMD")),
        ("BMS [ document lacunaire]", Some("BMS")),
        ("N + T", Some("N")),
        ("Table d\u{e9}cennale des d\u{e9}c\u{e8}s.", Some("TD")),
        (
            "Exampleville. Tables des naissances, mariages, d\u{e9}c\u{e8}s.",
            Some("TD"),
        ),
    ] {
        assert_eq!(page::title_act(title).as_deref(), act, "{title}");
    }
}

#[test]
fn validates_the_settings() {
    let registry = ArchiveRegistry::embedded();
    let gaia = registry.platform("gaia").unwrap();
    let mut collection = registry.archive("AD09").unwrap().collections[0].clone();
    assert_eq!(gaia.validate(&collection), Ok(()));
    let endpoint = gaia.endpoint(&collection).unwrap();
    assert_eq!(endpoint.start, format!("{ARIEGE}{WIZARD}/1/1/R/0/0"));
    for (field, value, message) in [
        ("origin", serde_json::json!("http://example.org"), "origin"),
        ("base", serde_json::json!("/mdr/"), "base"),
        ("theme", serde_json::json!(0), "theme"),
        (
            "locality_style",
            serde_json::json!("district"),
            "locality_style",
        ),
        ("types", serde_json::json!({"X": ["a"]}), "`X`"),
        ("types", serde_json::json!({"N": []}), "labels of `N`"),
        (
            "types",
            serde_json::json!({"N": ["naissances"]}),
            "no labels for `B`",
        ),
        ("unknown", serde_json::json!(1), "unknown"),
    ] {
        let mut changed = collection.clone();
        changed.portal[field] = value;
        let error = gaia.validate(&changed).unwrap_err().to_string();
        assert!(error.contains(message), "{field}: {error}");
    }
    collection.portal["localities"] = serde_json::json!(false);
    let error = gaia.validate(&collection).unwrap_err().to_string();
    assert!(error.contains("letters"), "{error}");
}

#[test]
fn a_class_is_followed_and_dates_its_lists() {
    let classes = || {
        portal(vec![
            get(&format!("{WIZARD}/31/1/R/0/0"), fixture!("classes.html")),
            get(
                &format!("{WIZARD}/31/1/A/900901/x"),
                fixture!("class-chosen.html"),
            ),
            post(
                &format!("{WIZARD}/31/2/T/0/0"),
                fixture!("class-lists.html"),
            ),
            post(&format!("{WIZARD}/31/1/T/0/0"), NONE),
        ])
    };
    // The class of 1816, whose lists are dated 1817.
    let portal = classes();
    let target = resolve(
        "AD77 - Exampleville - (aucun) - CM - 1816 - 9 R 235",
        &portal,
    )
    .unwrap();
    assert!(
        matches!(&target, ArchiveTarget::View { call_number: Some(c), .. } if c == "9R235"),
        "{target:?}"
    );
    assert_eq!(portal.requests().len(), 3);
    // Without a class, the list is searched as it stands, which finds none.
    let target = resolve("AD77 - Exampleville - (aucun) - CM", &classes()).unwrap();
    assert_eq!(
        target,
        ArchiveTarget::Results {
            url: format!("https://archives-en-ligne.seine-et-marne.fr{WIZARD}/31/1/R/0/0"),
            matches: Some(0),
        }
    );
}
