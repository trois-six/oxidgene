//! The adapter over anonymized answers (`fixtures/pleade/`, written by its
//! `generate.py`), with the catalogue's own settings for the Mayenne
//! (`form`, and `tree` for the military registers) and
//! Pyrénées-Atlantiques (`tree`) archives.

use std::pin::pin;
use std::sync::Mutex;
use std::task::{Context, Poll, Waker};

use serde_json::json;

use super::{Pleade, Settings, tree};
use crate::catalog::Collection;
use crate::platform::{BoxFuture, Platform};
use crate::transport::{FetchError, PortalFetch, PortalRequest};
use crate::{ArchiveRegistry, ArchiveTarget, ResolveError};

const MAY_FORM: &str = include_str!("../../../fixtures/pleade/may-form.html");
const MAY_ONE: &str = include_str!("../../../fixtures/pleade/may-results-one.html");
const MAY_SEVERAL: &str = include_str!("../../../fixtures/pleade/may-results-several.html");
const MAY_PAGE_2: &str = include_str!("../../../fixtures/pleade/may-results-page-2.html");
const MAY_TABLES: &str = include_str!("../../../fixtures/pleade/may-results-tables.html");
const MAY_SAMPLETON: &str = include_str!("../../../fixtures/pleade/may-results-sampleton.html");
const MAY_NONE: &str = include_str!("../../../fixtures/pleade/may-results-none.html");
const MANIFEST_12: &str = include_str!("../../../fixtures/pleade/manifest-12.json");
const MANIFEST_40: &str = include_str!("../../../fixtures/pleade/manifest-40.json");
const AID_ROOT: &str = include_str!("../../../fixtures/pleade/aid-root.html");
const AID_COMMUNE: &str = include_str!("../../../fixtures/pleade/aid-commune.html");
const AID_DEPARTEMENTALE: &str = include_str!("../../../fixtures/pleade/aid-departementale.html");
const AID_COMMUNALE: &str = include_str!("../../../fixtures/pleade/aid-communale.html");
const BMS_DEPARTEMENTALE: &str =
    include_str!("../../../fixtures/pleade/fragment-bms-departementale.html");
const BMS_COMMUNALE: &str = include_str!("../../../fixtures/pleade/fragment-bms-communale.html");
const NMD_DEPARTEMENTALE: &str =
    include_str!("../../../fixtures/pleade/fragment-nmd-departementale.html");
const NMD_COMMUNALE: &str = include_str!("../../../fixtures/pleade/fragment-nmd-communale.html");
const WITHOUT_IMAGES: &str = include_str!("../../../fixtures/pleade/fragment-without-images.html");
const TABLES: &str = include_str!("../../../fixtures/pleade/fragment-tables.html");
const MANIFEST_82: &str = include_str!("../../../fixtures/pleade/manifest-p64-82.json");
const MANIFEST_90: &str = include_str!("../../../fixtures/pleade/manifest-p64-90.json");
const RM_ROOT: &str = include_str!("../../../fixtures/pleade/rm-root.html");
const RM_COLLECTION: &str = include_str!("../../../fixtures/pleade/rm-collection.html");
const RM_CLASS: &str = include_str!("../../../fixtures/pleade/rm-class.html");
const RM_FRAGMENT: &str = include_str!("../../../fixtures/pleade/rm-fragment.html");

const MAY: &str = "https://archives.lamayenne.fr/archives-en-ligne";
const P64: &str = "https://earchives.le64.fr/archives-en-ligne";
const RESULTS: &str = "/archives-en-ligne/functions/ead/custom-results.ajax-html?";
const TOC: &str = "/archives-en-ligne/functions/ead/get-toc-fragment/";
const FRAGMENT: &str = "/archives-en-ligne/ead-fragment.xsp?c=";

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

type Router = Box<dyn Fn(&str) -> Option<&'static str> + Send + Sync>;

/// A portal answering each path through a router, recording the requests.
struct Portal {
    router: Router,
    requests: Mutex<Vec<String>>,
}

impl Portal {
    fn new(router: impl Fn(&str) -> Option<&'static str> + Send + Sync + 'static) -> Self {
        Self {
            router: Box::new(router),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn urls(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }

    fn count(&self, prefix: &str) -> usize {
        self.urls()
            .iter()
            .filter(|url| url.starts_with(prefix))
            .count()
    }
}

impl PortalFetch for Portal {
    fn request<'a>(
        &'a self,
        request: &'a PortalRequest,
    ) -> BoxFuture<'a, Result<String, FetchError>> {
        Box::pin(async move {
            self.requests.lock().unwrap().push(request.url.clone());
            (self.router)(&request.url)
                .map(str::to_owned)
                .ok_or(FetchError::Status(404))
        })
    }
}

fn resolve(title: &str, collection: usize, portal: &Portal) -> Result<ArchiveTarget, ResolveError> {
    let registry = ArchiveRegistry::embedded();
    let citation = registry.parse(title).expect("a normalized citation");
    let archive = registry
        .archive(&citation.code)
        .expect("a catalogued archive");
    block_on(Pleade.resolve(archive, &archive.collections[collection], &citation, portal))
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

/// The Mayenne form, its results by the label searched and the year.
fn mayenne(url: &str) -> Option<&'static str> {
    if url == "/archives-en-ligne/etat-civil-search-form.html" {
        return Some(MAY_FORM);
    }
    let query = url.strip_prefix(RESULTS)?;
    Some(if query.contains("query1=Sampleton") {
        MAY_SAMPLETON
    } else if !query.contains("query1=Exampleville-sur-Mer") {
        MAY_NONE
    } else if query.contains("query2=Tables") {
        MAY_TABLES
    } else if query.contains("du3=1850") {
        MAY_ONE
    } else if query.contains("&p=2") {
        MAY_PAGE_2
    } else if query.contains("du3=1795") || query.contains("du3=1925") {
        MAY_SEVERAL
    } else {
        MAY_NONE
    })
}

fn mayenne_manifests(url: &str) -> Option<&'static str> {
    match url {
        "/archives-en-ligne/iiif/ark:/99999/rexample0002/manifest.json" => Some(MANIFEST_12),
        "/archives-en-ligne/iiif/ark:/99999/rexample0003/manifest.json" => Some(MANIFEST_40),
        _ => mayenne(url),
    }
}

#[test]
fn opens_the_register_a_form_search_lists_at_the_cited_view() {
    let portal = Portal::new(mayenne);
    let target = resolved(
        "AD53 - Exampleville-sur-Mer - (aucun) - N - 1850 - 9 E 99/11 - vue 12/269",
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
    assert_eq!(*url, format!("{MAY}/ark:/99999/rexample0001/f12"));
    assert_eq!(views[0].ark.as_deref(), Some(url.as_str()));
    assert_eq!(*view_count, None);
    assert_eq!(call_number.as_deref(), Some("9 E 99/11"));
    // The form, then the search; no manifest when the citation decides.
    let urls = portal.urls();
    assert_eq!(urls.len(), 2, "{urls:?}");
    for field in [
        "base=ead2",
        "facets=udate%3Bfgeogcommune%3Bfanciennescommune%3Bfinstitution_paroisse%3Bftypedocec%3Bflisteacte%3Bfbdate",
        "champ1=fcommunes_paroisses",
        "query1=Exampleville-sur-Mer%20%28Exemple%2C%20France%29",
        "query2=Registres%20d%27actes",
        "du3=1850",
        "db3=",
        "de3=",
    ] {
        assert!(
            urls[1].split(['?', '&']).any(|pair| pair == field),
            "{field} in {}",
            urls[1]
        );
    }
}

#[test]
fn counts_the_images_of_the_registers_a_citation_leaves_apart() {
    // Two civil registers of 1795: the cited image count tells them apart.
    let portal = Portal::new(mayenne_manifests);
    let target = resolved(
        "AD53 - Exampleville-sur-Mer - (aucun) - N - 1795 - vue 30/40",
        0,
        &portal,
    );
    let ArchiveTarget::View {
        url, view_count, ..
    } = &target
    else {
        panic!("expected a view, got {target:?}");
    };
    assert_eq!(*url, format!("{MAY}/ark:/99999/rexample0003/f30"));
    assert_eq!(*view_count, Some(40));
    assert_eq!(portal.count("/archives-en-ligne/iiif/"), 2);

    // Without a count, the results page a reader opens on the same search.
    let portal = Portal::new(mayenne_manifests);
    let target = resolved(
        "AD53 - Exampleville-sur-Mer - (aucun) - N - 1795",
        0,
        &portal,
    );
    assert_eq!(matches_of(&target), Some(2));
    assert!(
        target
            .url()
            .starts_with(&format!("{MAY}/custom-results.html?base=ead2&"))
    );
    assert_eq!(portal.count("/archives-en-ligne/iiif/"), 0);
}

#[test]
fn reads_the_next_page_until_the_citation_decides() {
    let portal = Portal::new(mayenne);
    let target = resolved(
        "AD53 - Exampleville-sur-Mer - (aucun) - N - 1925 - vue 3/90",
        0,
        &portal,
    );
    assert_eq!(target.url(), format!("{MAY}/ark:/99999/rexample0021/f3"));
    assert_eq!(portal.count(RESULTS), 2);
}

#[test]
fn searches_tables_by_their_kind() {
    let portal = Portal::new(mayenne);
    let target = resolved(
        "AD53 - Exampleville-sur-Mer - (aucun) - TD - 1850",
        0,
        &portal,
    );
    // A decennial table of each kind, one call number.
    assert_eq!(matches_of(&target), Some(3));
    assert!(
        portal.urls()[1].contains("query2=Tables&"),
        "{:?}",
        portal.urls()
    );
}

#[test]
fn searches_every_label_of_a_locality() {
    // A commune and its former namesake.
    let portal = Portal::new(mayenne);
    let target = resolved(
        "AD53 - Sampleton - (aucun) - N - 1805 - vue 1/12",
        0,
        &portal,
    );
    assert_eq!(target.url(), format!("{MAY}/ark:/99999/rexample0031/f1"));
    assert_eq!(portal.count(RESULTS), 2);

    let unknown = Portal::new(mayenne);
    let target = resolved("AD53 - Elsewhere - (aucun) - N - 1805", 0, &unknown);
    assert_eq!(matches_of(&target), Some(0));
    assert_eq!(unknown.count(RESULTS), 0);
}

/// The Pyrénées-Atlantiques finding aid.
fn aid(url: &str) -> Option<&'static str> {
    if let Some(node) = url.strip_prefix(TOC) {
        return match node {
            "FRAD064003_IR0002/FRAD064003_IR0002.ajax-html" => Some(AID_ROOT),
            "FRAD064003_IR0002/FRAD999_IR0001_G1.ajax-html" => Some(AID_COMMUNE),
            "FRAD064003_IR0002/FRAD999_IR0001_e0000100.ajax-html" => Some(AID_DEPARTEMENTALE),
            "FRAD064003_IR0002/FRAD999_IR0001_e0000200.ajax-html" => Some(AID_COMMUNALE),
            _ => None,
        };
    }
    if let Some(component) = url.strip_prefix(FRAGMENT) {
        return match component {
            "FRAD999_IR0001_EX1" => Some(BMS_DEPARTEMENTALE),
            "FRAD999_IR0001_EX2" => Some(BMS_COMMUNALE),
            "FRAD999_IR0001_e0000112" => Some(NMD_DEPARTEMENTALE),
            "FRAD999_IR0001_e0000212" => Some(NMD_COMMUNALE),
            _ => None,
        };
    }
    match url {
        "/archives-en-ligne/iiif/ark:/99999/rnmdexample01/manifest.json" => Some(MANIFEST_82),
        "/archives-en-ligne/iiif/ark:/99999/rnmdexample02/manifest.json" => Some(MANIFEST_90),
        _ => None,
    }
}

#[test]
fn walks_a_finding_aid_down_to_the_cited_copy() {
    let portal = Portal::new(aid);
    let target = resolved(
        "AD64 - Exampleville - (aucun) - N - 1850 - vue 12/90",
        0,
        &portal,
    );
    let ArchiveTarget::View {
        url,
        view_count,
        call_number,
        ..
    } = &target
    else {
        panic!("expected a view, got {target:?}");
    };
    assert_eq!(*url, format!("{P64}/ark:/99999/rnmdexample02/f12"));
    assert_eq!(*view_count, Some(90));
    assert_eq!(*call_number, None);
    // The aid, the commune, its two copies' kinds and years, the two
    // registers' components and manifests; the tables and the parish
    // registers are not read.
    assert_eq!(portal.count(TOC), 4, "{:?}", portal.urls());
    assert_eq!(portal.count(FRAGMENT), 2);
    assert_eq!(portal.count("/archives-en-ligne/iiif/"), 2);

    // Without a count, the two copies are the results.
    let portal = Portal::new(aid);
    let target = resolved("AD64 - Exampleville - (aucun) - N - 1850", 0, &portal);
    assert_eq!(matches_of(&target), Some(2));
    assert_eq!(target.url(), format!("{P64}/ead.html?id=FRAD064003_IR0002"));
}

#[test]
fn dates_a_whole_collection_from_its_component() {
    // The parish registers' copies are registers themselves, undated in the
    // table of contents: their components date them.
    let portal = Portal::new(aid);
    let target = resolved("AD64 - Exampleville - (aucun) - B - 1720", 0, &portal);
    assert_eq!(target.url(), format!("{P64}/ark:/99999/rbmsexample02/f1"));
    assert_eq!(portal.count(TOC), 2);

    // A table, listed with its years under the commune.
    let portal = Portal::new(|url| {
        if url == format!("{FRAGMENT}FRAD999_IR0001_e0000302") {
            Some(TABLES)
        } else {
            aid(url)
        }
    });
    let target = resolved(
        "AD64 - Exampleville - (aucun) - TD - 1855 - vue 2/9",
        0,
        &portal,
    );
    assert_eq!(target.url(), format!("{P64}/ark:/99999/rtdexample01/f2"));
    assert_eq!(portal.count(TOC), 2);
}

#[test]
fn finds_no_register_of_an_unknown_locality() {
    let portal = Portal::new(aid);
    let target = resolved("AD64 - Elsewhere - (aucun) - N - 1850", 0, &portal);
    assert_eq!(matches_of(&target), Some(0));
    assert_eq!(portal.count(TOC), 1);
    // A locality whose article the aid writes behind the name.
    let portal = Portal::new(|url| {
        if url.ends_with("FRAD999_IR0001_G2.ajax-html") {
            Some("<div><ul id=\"treeRoot\"></ul></div>")
        } else {
            aid(url)
        }
    });
    let target = resolved("AD64 - Les Hauts-Exemples - (aucun) - N - 1850", 0, &portal);
    assert_eq!(matches_of(&target), Some(0));
    assert!(
        portal
            .urls()
            .iter()
            .any(|url| url.ends_with("FRAD999_IR0001_G2.ajax-html"))
    );
}

/// The Mayenne military registers' finding aid.
fn military(url: &str) -> Option<&'static str> {
    match url.strip_prefix(TOC)? {
        "FRAD053_2NUM109_RM/FRAD053_2NUM109_RM.ajax-html" => Some(RM_ROOT),
        "FRAD053_2NUM109_RM/FRAD999_RM_tt1-1.ajax-html" => Some(RM_COLLECTION),
        "FRAD053_2NUM109_RM/FRAD999_RM_tt3-41.ajax-html" => Some(RM_CLASS),
        _ => None,
    }
}

#[test]
fn chooses_a_military_register_by_office_class_and_matricule() {
    let portal = Portal::new(|url| {
        if url == "/archives-en-ligne/ead-fragment.xsp?c=FRAD999_RM_de-174" {
            Some(RM_FRAGMENT)
        } else {
            military(url)
        }
    });
    let target = resolved(
        "AD53 - Exampleville - Registres matricules - 1900 - matricule 600 - vue 3/412",
        1,
        &portal,
    );
    let ArchiveTarget::View {
        url, call_number, ..
    } = &target
    else {
        panic!("expected a view, got {target:?}");
    };
    assert_eq!(*url, format!("{MAY}/ark:/99999/rrmexample01/f3"));
    assert_eq!(call_number.as_deref(), Some("R 9002"));
    // The conscripts' lists and the other classes are not read.
    assert_eq!(portal.count(TOC), 3, "{:?}", portal.urls());
}

#[test]
fn reports_a_changed_portal_and_a_challenge_apart() {
    let changed = |error: ResolveError, expected: &str| match error {
        ResolveError::UnexpectedResponse(detail) => assert!(detail.contains(expected), "{detail}"),
        other => panic!("expected a changed shape, got {other:?}"),
    };
    let portal = Portal::new(|url| {
        if url.starts_with(RESULTS) {
            Some("<div class=\"pl-results\"></div>")
        } else {
            mayenne(url)
        }
    });
    changed(
        resolve(
            "AD53 - Exampleville-sur-Mer - (aucun) - N - 1850",
            0,
            &portal,
        )
        .unwrap_err(),
        "no count",
    );
    let portal = Portal::new(|_| Some("<div>no tree</div>"));
    changed(
        resolve("AD64 - Exampleville - (aucun) - N - 1850", 0, &portal).unwrap_err(),
        "no tree",
    );
    let portal = Portal::new(|_| {
        Some("<html><title>Just a moment...</title><script>window._cf_chl_opt={}</script></html>")
    });
    assert_eq!(
        resolve("AD64 - Exampleville - (aucun) - N - 1850", 0, &portal).unwrap_err(),
        ResolveError::Challenged
    );
    // A register whose component has no viewer link.
    let portal = Portal::new(|url| {
        if url.starts_with(FRAGMENT) {
            Some(WITHOUT_IMAGES)
        } else {
            aid(url)
        }
    });
    changed(
        resolve("AD64 - Exampleville - (aucun) - N - 1850", 0, &portal).unwrap_err(),
        "no viewer link",
    );
}

#[test]
fn reads_the_titles_of_a_finding_aid() {
    assert!(tree::is_period("1793-1806"));
    assert!(tree::is_period("Classe 1900"));
    assert!(!tree::is_period("Matricules 1-502 \u{2022} R 1535"));
    assert!(!tree::is_period("Collection communale"));
    assert_eq!(
        tree::locality_of(
            "Bureau de recrutement de Exampleville",
            "Bureau de recrutement de {locality}"
        )
        .as_deref(),
        Some("Exampleville")
    );
    assert_eq!(
        tree::locality_of("Hauts-Exemples (Les)", "{locality}").as_deref(),
        Some("Les Hauts-Exemples")
    );
    assert_eq!(
        tree::locality_of("Classe 1900", "Bureau de {locality}"),
        None
    );
}

#[test]
fn starts_on_robots_and_lands_on_the_search_page() {
    let registry = ArchiveRegistry::embedded();
    let citation = registry
        .parse("AD53 - Exampleville - (aucun) - N - 1850")
        .unwrap();
    let mayenne = registry.archive("AD53").unwrap();
    let endpoint = Pleade.endpoint(&mayenne.collections[0]).unwrap();
    assert_eq!(endpoint.start, "https://archives.lamayenne.fr/robots.txt");
    assert_eq!(
        Pleade
            .results_url(&mayenne.collections[0], &citation)
            .as_deref(),
        Some(format!("{MAY}/etat-civil-search-form.html").as_str())
    );
    let p64 = registry.archive("AD64").unwrap();
    assert_eq!(
        Pleade
            .results_url(&p64.collections[0], &citation)
            .as_deref(),
        Some(format!("{P64}/ead.html?id=FRAD064003_IR0002").as_str())
    );
}

fn collection(portal: serde_json::Value, acts: &[&str]) -> Collection {
    serde_json::from_value(json!({
        "id": "registers",
        "acts": acts,
        "platform": "pleade",
        "portal": portal,
    }))
    .unwrap()
}

#[test]
fn validates_its_settings() {
    let form = json!({
        "origin": "https://archives.example.org",
        "path": "/archives-en-ligne",
        "mode": "form",
        "form": "etat-civil-search-form.html",
        "results": "functions/ead/custom-results.ajax-html",
        "criteria": {"locality": 1, "kind": 2, "year": 3},
        "kinds": {"registers": "Registres", "tables": "Tables"}
    });
    let tree = json!({
        "origin": "https://archives.example.org",
        "path": "/archives-en-ligne",
        "mode": "tree",
        "aid": "FRAD999_IR0001",
        "locality_depth": 2,
        "locality_label": "{locality}"
    });
    assert!(Settings::read(&collection(form.clone(), &["B", "TD"])).is_ok());
    assert!(Settings::read(&collection(tree.clone(), &["B", "RM"])).is_ok());
    // The form searches acts and tables, not series.
    assert!(Settings::read(&collection(form.clone(), &["RM"])).is_err());

    let with = |base: &serde_json::Value, key: &str, value: serde_json::Value| {
        let mut settings = base.clone();
        settings[key] = value;
        settings
    };
    for settings in [
        with(&form, "origin", json!("http://archives.example.org")),
        with(&form, "path", json!("archives-en-ligne")),
        with(&form, "form", json!("/etat-civil-search-form.html")),
        with(&form, "results", json!("../x?y")),
        with(
            &form,
            "criteria",
            json!({"locality": 1, "kind": 1, "year": 3}),
        ),
        with(
            &form,
            "kinds",
            json!({"registers": " ", "tables": "Tables"}),
        ),
        with(&form, "aid", json!("FRAD999")),
        with(&form, "unknown", json!(1)),
        with(&tree, "aid", json!("FRAD 999")),
        with(&tree, "locality_depth", json!(0)),
        with(&tree, "locality_label", json!("Bureau")),
        with(&tree, "form", json!("x.html")),
        with(&tree, "mode", json!("search")),
    ] {
        assert!(
            Settings::read(&collection(settings.clone(), &["B"])).is_err(),
            "{settings}"
        );
    }
}
