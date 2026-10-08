//! The adapter over anonymized Bach pages (`fixtures/bach/`, written by
//! `fixtures/bach/generate.py`), with settings of each inventory kind.

use std::pin::pin;
use std::sync::Mutex;
use std::task::{Context, Poll, Waker};

use serde_json::{Value, json};

use super::page::{self, Node};
use super::{Bach, entry_locality, strip_words};
use crate::catalog::{Archive, Collection};
use crate::citation::{Act, CallNumber, CitationParts, CitedView};
use crate::live::Probe;
use crate::platform::{Access, BoxFuture, Platform};
use crate::transport::{FetchError, PortalFetch, PortalRequest};
use crate::{ArchiveTarget, ArchiveView, ResolveError};

const CLASSIFICATION: &str = include_str!("../../../fixtures/bach/classification.html");
const AID_COMMUNE: &str = include_str!("../../../fixtures/bach/aid-commune.html");
const AID_DEPARTMENT: &str = include_str!("../../../fixtures/bach/aid-department.html");
const AID_MILITARY: &str = include_str!("../../../fixtures/bach/aid-military.html");
const AID_TABLES: &str = include_str!("../../../fixtures/bach/aid-tables.html");
const SHOW_RANGE: &str = include_str!("../../../fixtures/bach/show-range.html");
const SHOW_FOLDER: &str = include_str!("../../../fixtures/bach/show-folder.html");
const SHOW_NONE: &str = include_str!("../../../fixtures/bach/show-none.html");
const SHOW_SEVERAL: &str = include_str!("../../../fixtures/bach/show-several.html");
const IMAGES: &str = include_str!("../../../fixtures/bach/images.json");
const CHALLENGE: &str = include_str!("../../../fixtures/bach/challenge.html");

const ORIGIN: &str = "https://archives.example.org";
const VIEWER: &str = "https://viewer.example.org";
const COMMUNE: &str = "FRAD099_RPEC_EXAMPLEVILLE";
const DEPARTMENT: &str = "FRAD099_00000001E";
const MILITARY: &str = "FRAD099_IR_00197";
const RANGE_LINK: &str = "https://viewer.example.org/series/EXEMPLE/REGISTRES/EX_001_00001?s=EX_001_00001_0001.jpg&e=EX_001_00001_0012.jpg&levelDescription=FRAD099_RPEC_EXAMPLEVILLE_de-1";
const FOLDER_LINK: &str = "https://archives.example.org/viewer/series/E/9E/EX_0001_001_01/";
const FOLDER_LIST: &str =
    "https://archives.example.org/viewer/api/info/series/E/9E/EX_0001_001_01/";

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

/// The portal: an answer per address, an address ending with `*` answering
/// every address it starts.
struct Portal {
    routes: Vec<(String, &'static str)>,
    requests: Mutex<Vec<String>>,
}

impl Portal {
    fn new(routes: &[(&str, &'static str)]) -> Self {
        Self {
            routes: routes
                .iter()
                .map(|(url, body)| ((*url).to_owned(), *body))
                .collect(),
            requests: Mutex::new(Vec::new()),
        }
    }

    /// The pages of a classification collection whose registers open on
    /// the range a link names.
    fn communes() -> Self {
        Self::new(&[
            ("/archives/classification-scheme", CLASSIFICATION),
            ("/document/FRAD099_RPEC_EXAMPLEVILLE", AID_COMMUNE),
            (
                "/archives/show/FRAD099_RPEC_EXAMPLEVILLE_de-2",
                SHOW_SEVERAL,
            ),
            ("/archives/show/FRAD099_RPEC_EXAMPLEVILLE_de-4", SHOW_NONE),
            ("/archives/show/FRAD099_RPEC_EXAMPLEVILLE_*", SHOW_RANGE),
        ])
    }

    /// The pages of a collection of one finding aid, whose registers open
    /// in a viewer listing their images.
    fn department(document: &str, aid: &'static str) -> Self {
        let mut portal = Self::new(&[("/archives/show/*", SHOW_FOLDER), (FOLDER_LIST, IMAGES)]);
        portal
            .routes
            .insert(0, (format!("/document/{document}"), aid));
        portal
    }

    fn requests(&self) -> Vec<String> {
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
            self.routes
                .iter()
                .find(|(url, _)| match url.strip_suffix('*') {
                    Some(start) => request.url.starts_with(start),
                    None => *url == request.url,
                })
                .map(|(_, body)| (*body).to_owned())
                .ok_or(FetchError::Status(404))
        })
    }
}

fn archive() -> Archive {
    serde_json::from_value(json!({
        "id": "fr-ad99",
        "country": "FR",
        "level": "departmental",
        "name": "Archives d'Exemple",
        "citation_codes": ["AD99"],
        "website": "https://www.example.org",
    }))
    .unwrap()
}

fn collection(portal: Value) -> Collection {
    serde_json::from_value(json!({
        "id": "registers",
        "acts": ["B", "M", "S", "N", "D", "TD", "RM"],
        "platform": "bach",
        "portal": portal,
    }))
    .unwrap()
}

/// One aid per commune, named by the entries' titles, whose viewer is on
/// its own origin and whose links name their images' range.
fn by_title() -> Collection {
    collection(json!({
        "origin": ORIGIN,
        "transport": "browser",
        "viewer": VIEWER,
        "views": "range",
        "inventory": {"kind": "classification", "prefix": "FRAD099_RPEC_", "locality": "title"},
    }))
}

/// One aid per commune, named by the links after a common title.
fn by_link() -> Collection {
    collection(json!({
        "origin": ORIGIN,
        "viewer": VIEWER,
        "inventory": {
            "kind": "classification",
            "prefix": "FRAD099_IR_",
            "locality": "link",
            "title_prefixes": ["Registres paroissiaux et d'état civil :"],
        },
    }))
}

/// One aid for every locality, its viewer on the portal's origin.
fn by_document(document: &str, level: Option<u8>) -> Collection {
    collection(json!({
        "origin": ORIGIN,
        "viewer": format!("{ORIGIN}/viewer"),
        "inventory": {"kind": "document", "document": document, "level": level},
    }))
}

fn cite(
    locality: &str,
    act: &str,
    year: Option<u16>,
    call_number: Option<&str>,
    views: &[u16],
    count: Option<u16>,
) -> CitationParts {
    CitationParts {
        code: "AD99".to_owned(),
        locality: locality.to_owned(),
        parish: None,
        act: Act::from_code(act).unwrap(),
        year,
        period: year.map(|year| year.to_string()),
        call_number: call_number.map(CallNumber::new),
        number: None,
        views: views
            .iter()
            .map(|view| CitedView {
                view: *view,
                side: None,
            })
            .collect(),
        view_count: count,
        alternate_localities: Vec::new(),
    }
}

fn resolve(
    collection: &Collection,
    citation: &CitationParts,
    portal: &Portal,
) -> Result<ArchiveTarget, ResolveError> {
    block_on(Bach.resolve(&archive(), collection, citation, portal))
}

fn results(url: &str, matches: usize) -> ArchiveTarget {
    ArchiveTarget::Results {
        url: url.to_owned(),
        matches: Some(matches),
    }
}

fn view(view: u16, url: String) -> ArchiveView {
    ArchiveView {
        view,
        url,
        ark: None,
        image: None,
    }
}

#[test]
fn a_register_opens_on_its_image_named_in_the_range_of_its_link() {
    let portal = Portal::communes();
    let citation = cite(
        "Exampleville",
        "B",
        Some(1650),
        Some("GG 1"),
        &[5],
        Some(12),
    );
    let url = format!("{RANGE_LINK}&img=EX_001_00001_0005.jpg");
    assert_eq!(
        resolve(&by_title(), &citation, &portal),
        Ok(ArchiveTarget::View {
            url: url.clone(),
            views: vec![view(5, url)],
            view_count: Some(12),
            call_number: Some("GG 1".to_owned()),
            attribution: None,
            renumbering: None,
        })
    );
    // The list of finding aids, the commune's, the register's page: the
    // range names the images, without the viewer's list.
    assert_eq!(
        portal.requests(),
        [
            "/archives/classification-scheme",
            "/document/FRAD099_RPEC_EXAMPLEVILLE",
            "/archives/show/FRAD099_RPEC_EXAMPLEVILLE_de-1",
        ]
    );
}

#[test]
fn a_register_opens_on_its_image_named_by_the_viewer() {
    let portal = Portal::department(DEPARTMENT, AID_DEPARTMENT);
    let citation = cite("Aval-Exemple", "N", Some(1810), None, &[7, 8], Some(12));
    let first = format!("{FOLDER_LINK}?img=EX_0001_001_01_0007.jpg");
    let second = format!("{FOLDER_LINK}?img=EX_0001_001_01_0008.jpg");
    assert_eq!(
        resolve(&by_document(DEPARTMENT, Some(2)), &citation, &portal),
        Ok(ArchiveTarget::View {
            url: first.clone(),
            views: vec![view(7, first), view(8, second)],
            view_count: Some(12),
            call_number: Some("9 E 1/3".to_owned()),
            attribution: None,
            renumbering: None,
        })
    );
    assert_eq!(
        portal.requests(),
        [
            "/document/FRAD099_00000001E",
            "/archives/show/FRAD099_00000001E_de-3",
            FOLDER_LIST,
        ]
    );
    // Its decennial tables are in the same register.
    let tables = cite("Aval-Exemple", "TD", Some(1810), None, &[], None);
    assert!(matches!(
        resolve(&by_document(DEPARTMENT, Some(2)), &tables, &portal),
        Ok(ArchiveTarget::View { call_number, .. }) if call_number.as_deref() == Some("9 E 1/3")
    ));
}

#[test]
fn a_locality_is_found_however_the_portal_writes_it() {
    let portal = Portal::communes();
    let citation = cite("Le Mas-d'Exemple", "B", Some(1700), None, &[], None);
    // Found in the list (its aid is not recorded).
    let _ = resolve(&by_title(), &citation, &portal);
    assert_eq!(portal.requests()[1], "/document/FRAD099_RPEC_MAS_D_EXEMPLE");

    // Capitals, and the article behind the name.
    let portal = Portal::department(DEPARTMENT, AID_DEPARTMENT);
    let citation = cite("Le Mas-d'Exemple", "B", Some(1660), None, &[], None);
    assert!(matches!(
        resolve(&by_document(DEPARTMENT, Some(2)), &citation, &portal),
        Ok(ArchiveTarget::View { call_number, .. }) if call_number.as_deref() == Some("9 E 3/1")
    ));
}

#[test]
fn several_registers_or_none_land_on_the_finding_aid() {
    let aid = format!("{ORIGIN}/document/{COMMUNE}");
    let several = cite("Exampleville", "B", Some(1650), None, &[], None);
    assert_eq!(
        resolve(&by_title(), &several, &Portal::communes()),
        Ok(results(&aid, 3))
    );
    let none = cite("Exampleville", "B", Some(1500), None, &[], None);
    assert_eq!(
        resolve(&by_title(), &none, &Portal::communes()),
        Ok(results(&aid, 0))
    );
}

#[test]
fn a_locality_the_list_does_not_name_once_lands_on_the_list() {
    let list = format!("{ORIGIN}/archives/classification-scheme");
    for (locality, matches) in [("Nowhere", 0), ("Doubleville", 2)] {
        let portal = Portal::communes();
        let citation = cite(locality, "B", Some(1700), None, &[], None);
        assert_eq!(
            resolve(&by_title(), &citation, &portal),
            Ok(results(&list, matches))
        );
        assert_eq!(portal.requests().len(), 1);
    }
}

#[test]
fn entries_name_their_locality_in_their_title_or_their_link() {
    let prefixes = ["Registres paroissiaux et d'état civil :".to_owned()];
    let named = |documents: &str, label, title_prefixes: &[String]| {
        page::entries(CLASSIFICATION, documents)
            .unwrap()
            .iter()
            .filter_map(|entry| {
                entry_locality(entry, label, title_prefixes)
                    .map(|locality| (entry.document.clone(), locality))
            })
            .collect::<Vec<_>>()
    };
    let pair = |document: &str, locality: &str| (document.to_owned(), locality.to_owned());
    assert_eq!(
        named("FRAD099_Ec_", super::Label::Title, &prefixes),
        [
            pair("FRAD099_Ec_Bourg_Exemple", "Bourg-Exemple"),
            pair("FRAD099_Ec_Saint_Exemple", "Saint-Exemple"),
        ]
    );
    assert_eq!(
        named("FRAD099_IR_", super::Label::Link, &prefixes),
        [
            pair("FRAD099_IR_00001", "Albeville"),
            pair("FRAD099_IR_00002", "Exampleville"),
        ]
    );
    let entries = page::entries(CLASSIFICATION, "FRAD099_").unwrap();
    // A title inside its link.
    assert!(
        entries
            .iter()
            .any(|entry| entry.document == "FRAD099_IR_00351"
                && entry.title == "Catalogue de la bibliothèque")
    );

    let portal = Portal::communes();
    let citation = cite("Exampleville", "B", Some(1700), None, &[], None);
    let _ = resolve(&by_link(), &citation, &portal);
    assert_eq!(portal.requests()[1], "/document/FRAD099_IR_00002");
}

#[test]
fn strips_a_title_s_first_words_whatever_their_writing() {
    let prefix = "Registres paroissiaux et d'état civil :";
    assert_eq!(
        strip_words(
            "Registres paroissiaux et d’état civil : Saint-Exemple",
            prefix
        ),
        Some("Saint-Exemple")
    );
    assert_eq!(
        strip_words("REGISTRES PAROISSIAUX ET D'ETAT CIVIL", prefix),
        Some("")
    );
    assert_eq!(strip_words("Registres paroissiaux", prefix), None);
    assert_eq!(
        strip_words("Registres paroissiaux et d'état civilisés", prefix),
        None
    );
}

#[test]
fn periods_come_from_dates_titles_and_ancestors() {
    let pick = |act: &str, year: u16, call_number: Option<&str>| {
        let citation = cite("Exampleville", act, Some(year), call_number, &[], None);
        match resolve(&by_title(), &citation, &Portal::communes()) {
            Ok(ArchiveTarget::View { call_number, .. }) => call_number,
            Ok(ArchiveTarget::Results { matches, .. }) => matches.map(|count| count.to_string()),
            Err(error) => panic!("{error}"),
        }
    };
    // Months around the years.
    assert_eq!(pick("N", 1815, None).as_deref(), Some("9 E 10"));
    // A Republican year, in a collection implying its acts.
    assert_eq!(pick("N", 1800, None).as_deref(), Some("9 E 2"));
    // The period of the parent, and the call number breaking the tie.
    assert_eq!(pick("M", 1820, None).as_deref(), Some("2"));
    assert_eq!(pick("M", 1820, Some("9 E 12")).as_deref(), Some("9 E 12"));
    // Publications of banns apart from the marriages they announce.
    assert_eq!(pick("P", 1815, None).as_deref(), Some("9 E 13"));
    // The one register of the year, under another call number than the
    // cited one, which another collection may hold.
    assert_eq!(pick("N", 1815, Some("9 E 99")).as_deref(), Some("1"));
}

#[test]
fn tables_are_told_from_the_acts_they_index() {
    let pick = |act: &str, year: u16| {
        let citation = cite("Exampleville", act, Some(year), None, &[], None);
        resolve(&by_title(), &citation, &Portal::communes()).unwrap()
    };
    assert!(matches!(
        pick("TD", 1805),
        ArchiveTarget::View { call_number, .. } if call_number.as_deref() == Some("TD 4")
    ));
    assert_eq!(
        pick("TD", 1795),
        results(&format!("{ORIGIN}/document/{COMMUNE}"), 2)
    );
    // The births of 1795 are not their table's.
    assert!(matches!(
        pick("N", 1795),
        ArchiveTarget::View { call_number, .. } if call_number.as_deref() == Some("9 E 2")
    ));
}

#[test]
fn an_aid_s_title_says_what_its_registers_hold() {
    let document = "FRAD099_00000164M";
    let portal = Portal::department(document, AID_TABLES);
    let tables = cite("Aval-Exemple", "TD", Some(1815), None, &[], None);
    assert!(matches!(
        resolve(&by_document(document, Some(2)), &tables, &portal),
        Ok(ArchiveTarget::View { call_number, .. }) if call_number.as_deref() == Some("9 M 1/2")
    ));
    // Not the births of 1815.
    let births = cite("Aval-Exemple", "N", Some(1815), None, &[], None);
    assert_eq!(
        resolve(&by_document(document, Some(2)), &births, &portal),
        Ok(results(&format!("{ORIGIN}/document/{document}"), 0))
    );
}

#[test]
fn a_military_register_is_chosen_by_bureau_class_and_number() {
    let collection = by_document(MILITARY, Some(3));
    let pick = |locality: &str, year: u16, number: Option<u32>| {
        let mut citation = cite(locality, "RM", Some(year), None, &[], None);
        citation.number = number;
        let portal = Portal::department(MILITARY, AID_MILITARY);
        resolve(&collection, &citation, &portal).unwrap()
    };
    let call_number = |target: ArchiveTarget| match target {
        ArchiveTarget::View { call_number, .. } => call_number,
        ArchiveTarget::Results { .. } => None,
    };
    assert_eq!(
        call_number(pick("Exampleville", 1873, Some(640))).as_deref(),
        Some("9 R 2/10")
    );
    // The volume, not its alphabetical index.
    assert_eq!(
        call_number(pick("Exampleville", 1872, None)).as_deref(),
        Some("9 R 2/7")
    );
    // An office named with a parenthesis after its place.
    assert_eq!(
        call_number(pick("Saint-Exemple", 1872, None)).as_deref(),
        Some("9 R 3/1")
    );
}

#[test]
fn a_view_beyond_the_register_opens_its_first_image() {
    let citation = cite(
        "Exampleville",
        "B",
        Some(1650),
        Some("GG 1"),
        &[20],
        Some(20),
    );
    assert_eq!(
        resolve(&by_title(), &citation, &Portal::communes()),
        Ok(ArchiveTarget::View {
            url: RANGE_LINK.to_owned(),
            views: Vec::new(),
            view_count: Some(12),
            call_number: Some("GG 1".to_owned()),
            attribution: None,
            renumbering: None,
        })
    );
}

#[test]
fn a_register_without_one_viewer_link_lands_on_its_place_in_the_aid() {
    let aid = format!("{ORIGIN}/document/{COMMUNE}");
    let without = cite("Exampleville", "B", Some(1780), Some("GG 5"), &[], None);
    assert_eq!(
        resolve(&by_title(), &without, &Portal::communes()),
        Ok(results(&format!("{aid}#de-4"), 1))
    );
    let several = cite("Exampleville", "B", Some(1780), Some("GG 2"), &[], None);
    assert_eq!(
        resolve(&by_title(), &several, &Portal::communes()),
        Ok(results(&format!("{aid}#de-2"), 2))
    );
}

#[test]
fn an_anti_bot_check_is_reported_apart_from_drift() {
    let citation = cite(
        "Exampleville",
        "B",
        Some(1650),
        Some("GG 1"),
        &[5],
        Some(12),
    );
    let portal = Portal::new(&[
        ("/archives/classification-scheme", CLASSIFICATION),
        ("/document/FRAD099_RPEC_EXAMPLEVILLE", AID_COMMUNE),
        ("/archives/show/*", CHALLENGE),
    ]);
    assert_eq!(
        resolve(&by_title(), &citation, &portal),
        Err(ResolveError::Challenged)
    );
    let portal = Portal::new(&[("/archives/classification-scheme", CHALLENGE)]);
    assert_eq!(
        resolve(&by_title(), &citation, &portal),
        Err(ResolveError::Challenged)
    );
}

#[test]
fn a_changed_portal_is_reported() {
    let citation = cite(
        "Exampleville",
        "B",
        Some(1650),
        Some("GG 1"),
        &[5],
        Some(12),
    );
    let drift =
        |portal: &Portal, collection: &Collection| match resolve(collection, &citation, portal) {
            Err(ResolveError::UnexpectedResponse(detail)) => detail,
            other => panic!("{other:?}"),
        };
    let empty = Portal::new(&[(
        "/archives/classification-scheme",
        "<html><body></body></html>",
    )]);
    assert!(drift(&empty, &by_title()).starts_with("bach: "));
    let other_prefix = collection(json!({
        "origin": ORIGIN,
        "viewer": VIEWER,
        "inventory": {"kind": "classification", "prefix": "FRAD099_GONE_", "locality": "title"},
    }));
    assert!(drift(&Portal::communes(), &other_prefix).contains("none of the collection"));
    let no_tree = Portal::new(&[("/document/FRAD099_00000001E", SHOW_FOLDER)]);
    assert!(drift(&no_tree, &by_document(DEPARTMENT, Some(2))).contains("no tree"));
    // A viewer other than the settings'.
    let moved = collection(json!({
        "origin": ORIGIN,
        "viewer": "https://moved.example.org",
        "inventory": {"kind": "classification", "prefix": "FRAD099_RPEC_", "locality": "title"},
    }));
    assert!(drift(&Portal::communes(), &moved).contains("off the settings' viewer"));
    // An image list of another shape.
    let mut broken = Portal::department(DEPARTMENT, AID_DEPARTMENT);
    broken
        .routes
        .insert(0, (FOLDER_LIST.to_owned(), "{\"count\": 2, \"data\": []}"));
    let citation = cite("Aval-Exemple", "N", Some(1810), None, &[7], None);
    assert!(matches!(
        resolve(&by_document(DEPARTMENT, Some(2)), &citation, &broken),
        Err(ResolveError::UnexpectedResponse(_))
    ));
}

#[test]
fn the_results_page_is_built_without_a_request() {
    let citation = cite("Exampleville", "B", Some(1650), None, &[], None);
    assert_eq!(
        Bach.results_url(&by_title(), &citation).as_deref(),
        Some("https://archives.example.org/archives/classification-scheme")
    );
    assert_eq!(
        Bach.results_url(&by_document(DEPARTMENT, Some(2)), &citation)
            .as_deref(),
        Some("https://archives.example.org/document/FRAD099_00000001E")
    );
}

#[test]
fn the_endpoint_declares_the_viewer_and_a_start_page() {
    let endpoint = Bach.endpoint(&by_title()).unwrap();
    assert_eq!(endpoint.other_origins, [VIEWER]);
    assert_eq!(endpoint.access, Access::Browser);
    // A page the anti-bot check guards, passed before the requests.
    assert_eq!(
        endpoint.start,
        "https://archives.example.org/archives/classification-scheme"
    );
    let endpoint = Bach.endpoint(&by_document(DEPARTMENT, Some(2))).unwrap();
    assert!(endpoint.other_origins.is_empty());
    assert_eq!(endpoint.start, "https://archives.example.org/robots.txt");
}

#[test]
fn settings_are_validated() {
    let valid = json!({
        "origin": ORIGIN,
        "viewer": VIEWER,
        "inventory": {"kind": "classification", "prefix": "FRAD099_RPEC_", "locality": "title"},
    });
    assert_eq!(Bach.validate(&collection(valid.clone())), Ok(()));
    let invalid = |change: fn(&mut Value)| {
        let mut portal = valid.clone();
        change(&mut portal);
        Bach.validate(&collection(portal)).unwrap_err().to_string()
    };
    for (change, message) in [
        (
            (|portal: &mut Value| portal["origin"] = "http://archives.example.org".into())
                as fn(&mut Value),
            "origin",
        ),
        (
            |portal| portal["viewer"] = "https://viewer.example.org/".into(),
            "viewer",
        ),
        (
            |portal| portal["viewer"] = "viewer.example.org".into(),
            "viewer",
        ),
        (
            |portal| portal["viewer"] = "https://x.org/a b".into(),
            "viewer",
        ),
        (|portal| portal["views"] = "guess".into(), "unknown variant"),
        (
            |portal| portal["inventory"]["prefix"] = "FRAD 099".into(),
            "prefix",
        ),
        (
            |portal| portal["inventory"]["locality"] = "both".into(),
            "unknown variant",
        ),
        (
            |portal| portal["inventory"]["title_prefixes"] = json!([" : "]),
            "title_prefixes",
        ),
        (
            |portal| portal["inventory"]["level"] = 2.into(),
            "unknown field",
        ),
        (
            |portal| portal["inventory"] = json!({"kind": "document", "document": "FRAD099/X"}),
            "document",
        ),
        (
            |portal| {
                portal["inventory"] =
                    json!({"kind": "document", "document": "FRAD099_X", "level": 0})
            },
            "level",
        ),
        (
            |portal| portal["inventory"] = json!({"kind": "search"}),
            "unknown variant",
        ),
        (|portal| portal["engine"] = "bach".into(), "unknown field"),
    ] {
        let error = invalid(change);
        assert!(error.contains(message), "{message}: {error}");
    }
}

#[test]
fn a_range_names_the_images_between_its_ends() {
    assert_eq!(
        page::range_names("https://v.example.org/series/A?s=X_0008.jpg&e=X_0011.jpg").unwrap(),
        ["X_0008.jpg", "X_0009.jpg", "X_0010.jpg", "X_0011.jpg"]
    );
    assert_eq!(
        page::range_names(
            "https://v.example.org/series/A?s=X_098_C.jpg&e=X_101_C.jpg&levelDescription=Y"
        )
        .unwrap(),
        ["X_098_C.jpg", "X_099_C.jpg", "X_100_C.jpg", "X_101_C.jpg"]
    );
    assert_eq!(
        page::range_names("https://v.example.org/series/A?s=X%5F1.jpg&e=X%5F2.jpg").unwrap(),
        ["X_1.jpg", "X_2.jpg"]
    );
    for link in [
        "https://v.example.org/series/A",
        "https://v.example.org/series/A?levelDescription=Y",
        "https://v.example.org/series/A?s=X_0002.jpg&e=X_0001.jpg",
        "https://v.example.org/series/A?s=X_0001.jpg&e=Y_0009.jpg",
    ] {
        assert_eq!(page::range_names(link), None, "{link}");
    }
}

#[test]
fn a_tree_reads_its_nodes_depths_and_registers() {
    let aid = page::tree(AID_MILITARY, MILITARY).unwrap();
    assert_eq!(aid.root.title, "Préparation et recrutement militaire");
    let nodes = aid.nodes;
    let node = |id: &str| nodes.iter().find(|node| node.id == id).unwrap().clone();
    assert_eq!(nodes.len(), 17);
    assert_eq!(
        node("tt3-1"),
        Node {
            id: "tt3-1".to_owned(),
            depth: 3,
            title: "Bureau de recrutement d'Exampleville".to_owned(),
            date: Some("1872-1873".to_owned()),
            call_number: None,
            leaf: false,
        }
    );
    assert_eq!(
        node("de-9"),
        Node {
            id: "de-9".to_owned(),
            depth: 5,
            title: "N° 501-1000.".to_owned(),
            date: Some("1873".to_owned()),
            call_number: Some("9 R 2/10".to_owned()),
            leaf: true,
        }
    );
    assert!(node("de-1").leaf && !node("tt2-1").leaf);
    // Another aid's nodes are not this one's.
    assert!(
        page::tree(AID_MILITARY, "FRAD099_OTHER")
            .unwrap()
            .nodes
            .is_empty()
    );
}

#[test]
fn the_viewer_s_list_names_every_image() {
    let names = page::image_names(IMAGES).unwrap();
    assert_eq!(names.len(), 12);
    assert_eq!(names[0], "EX_0001_001_01_0001.jpg");
    assert!(page::image_names("{\"count\": 1, \"data\": [{\"name\": \"\"}]}").is_err());
    assert_eq!(page::image_names(CHALLENGE), Err(ResolveError::Challenged));
}

#[test]
fn the_probe_lists_localities_registers_and_images() {
    let portal = Portal::communes();
    assert_eq!(
        block_on(Bach.search_page(&by_title(), &portal)).unwrap(),
        "Doubleville"
    );

    let military = by_document(MILITARY, Some(3));
    let portal = Portal::department(MILITARY, AID_MILITARY);
    assert_eq!(
        block_on(Bach.search_page(&military, &portal)).unwrap(),
        "Exampleville"
    );
    let act = Act::from_code("RM").unwrap();
    let registers = block_on(Bach.registers(&military, "Exampleville", &act, &portal)).unwrap();
    let call_numbers: Vec<_> = registers
        .iter()
        .map(|register| register.call_number.as_deref().unwrap())
        .collect();
    assert_eq!(call_numbers, ["9 R 2/7", "9 R 2/9", "9 R 2/10"]);
    assert_eq!(registers[2].numbers, Some((501, 1000)));
    assert_eq!(registers[2].period.as_deref(), Some("1873"));
    assert_eq!(
        registers[2].address.as_deref(),
        Some("/archives/show/FRAD099_IR_00197_de-9")
    );
    // The first register opened has images.
    let counts: Vec<_> = registers.iter().map(|register| register.images).collect();
    assert_eq!(counts, [Some(12), None, None]);
    assert_eq!(
        block_on(Bach.images(&military, &registers[2], &portal)).unwrap(),
        Some(12)
    );

    // Registers without one viewer link are passed over.
    let communes = Portal::new(&[
        ("/archives/classification-scheme", CLASSIFICATION),
        ("/document/FRAD099_RPEC_EXAMPLEVILLE", AID_COMMUNE),
        ("/archives/show/FRAD099_RPEC_EXAMPLEVILLE_de-1", SHOW_NONE),
        (
            "/archives/show/FRAD099_RPEC_EXAMPLEVILLE_de-2",
            SHOW_SEVERAL,
        ),
        ("/archives/show/FRAD099_RPEC_EXAMPLEVILLE_*", SHOW_RANGE),
    ]);
    let baptisms = Act::from_code("B").unwrap();
    let registers =
        block_on(Bach.registers(&by_title(), "Exampleville", &baptisms, &communes)).unwrap();
    let opened: Vec<_> = registers
        .iter()
        .filter_map(|register| Some((register.call_number.as_deref()?, register.images?)))
        .collect();
    assert_eq!(opened, [("GG 1", 0), ("GG 2", 0), ("GG 3", 12)]);

    // A series of a single office has no locality to list.
    let whole = by_document(MILITARY, None);
    assert_eq!(block_on(Bach.search_page(&whole, &portal)).unwrap(), "");
    let registers = block_on(Bach.registers(&whole, "", &act, &portal)).unwrap();
    assert_eq!(registers.len(), 4);
}

#[test]
fn every_catalogued_collection_is_valid() {
    let registry = crate::ArchiveRegistry::embedded();
    let collections: Vec<&Collection> = registry
        .archives()
        .iter()
        .flat_map(|archive| &archive.collections)
        .filter(|collection| collection.platform == "bach")
        .collect();
    for collection in collections {
        assert_eq!(Bach.validate(collection), Ok(()), "{}", collection.id);
        assert!(Bach.endpoint(collection).is_some());
    }
}
