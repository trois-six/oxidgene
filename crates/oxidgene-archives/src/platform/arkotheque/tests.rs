//! The adapter over anonymized answers of the Loire-Atlantique and Sarthe
//! portals (`fixtures/arkotheque/`, written by its `generate.py`), with the
//! catalogue's own settings for both archives.

use std::pin::pin;
use std::sync::Mutex;
use std::task::{Context, Poll, Waker};

use super::*;
use crate::transport::{FetchError, PortalRequest};
use crate::{ArchiveRegistry, platform};

const AD44_ONE: &str = include_str!("../../../fixtures/arkotheque/ad44-one.json");
const AD44_SEVERAL: &str = include_str!("../../../fixtures/arkotheque/ad44-several.json");
const AD44_NONE: &str = include_str!("../../../fixtures/arkotheque/ad44-none.json");
const AD44_PERIODS: &str = include_str!("../../../fixtures/arkotheque/ad44-period-segments.json");
const AD44_VIEWER: &str = include_str!("../../../fixtures/arkotheque/ad44-viewer.json");
const AD44_INFO: &str = include_str!("../../../fixtures/arkotheque/ad44-info.json");
const AD72_SEVERAL: &str = include_str!("../../../fixtures/arkotheque/ad72-several.json");
const AD72_TEXT_MATCH: &str = include_str!("../../../fixtures/arkotheque/ad72-text-match.json");
const AD72_AFTER_1902: &str = include_str!("../../../fixtures/arkotheque/ad72-after-1902.json");

const AD44_SEARCH_PAGE: &str =
    "https://archives-numerisees.loire-atlantique.fr/chercher/etat-civil-et-registres-paroissiaux";
const AD44_VIEWER_ADDRESS: &str = "/_recherche-api/visionneuse-infos/arko_default_6a6b4ac0309a5";

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

/// Answers each kind of request with its fixture, and records the requests.
struct Fixtures {
    search: &'static str,
    viewer: &'static str,
    info: &'static str,
    requests: Mutex<Vec<String>>,
}

impl Fixtures {
    fn new(search: &'static str) -> Self {
        Self {
            search,
            viewer: AD44_VIEWER,
            info: AD44_INFO,
            requests: Mutex::new(Vec::new()),
        }
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

impl PortalFetch for Fixtures {
    fn request<'a>(
        &'a self,
        request: &'a PortalRequest,
    ) -> BoxFuture<'a, Result<String, FetchError>> {
        Box::pin(async move {
            self.requests.lock().unwrap().push(request.url.clone());
            let url = request.url.as_str();
            let body = if url.starts_with("/_recherche-api/moteur?") {
                self.search
            } else if url.starts_with("/_recherche-api/visionneuse-infos/") {
                self.viewer
            } else if url.starts_with("/_recherche-images/") && url.ends_with("/info.json") {
                self.info
            } else {
                return Err(FetchError::Status(404));
            };
            Ok(body.to_owned())
        })
    }
}

/// Resolves `title` in the first collection the registry would try.
fn resolve(
    registry: &ArchiveRegistry,
    title: &str,
    fetch: &Fixtures,
) -> Result<ArchiveTarget, ResolveError> {
    let citation = registry.parse(title).expect("a normalized citation");
    let (archive, collections) = registry.candidates(&citation).expect("a catalogued act");
    block_on(Arkotheque.resolve(archive, collections[0], &citation, fetch))
}

fn embedded(title: &str, search: &'static str) -> (Result<ArchiveTarget, ResolveError>, Fixtures) {
    let fetch = Fixtures::new(search);
    let target = resolve(ArchiveRegistry::embedded(), title, &fetch);
    (target, fetch)
}

fn view_url(record: &str, file: u32, index: u16) -> String {
    format!(
        "{AD44_SEARCH_PAGE}?detail={record}#{AD44_VIEWER_ADDRESS}/{record}/arko_default_6a6b4d70c2bc6/image/{file}/{index}"
    )
}

fn opened_record(target: &ArchiveTarget) -> &str {
    let url = target.url();
    let start = url.find("detail=").expect("a record page") + "detail=".len();
    &url[start..start + url[start..].find('#').expect("a viewer fragment")]
}

#[test]
fn one_register_opens_on_the_cited_view() {
    let (target, fetch) = embedded(
        "AD44 - Exampleville - Saint-Exemple - B - 1660 - E dépôt 99 - acte 4 - vue 2g/3",
        AD44_ONE,
    );
    let record = "arko_fiche_0000000000a01";
    assert_eq!(
        target,
        Ok(ArchiveTarget::View {
            url: view_url(record, 900001, 1),
            views: vec![ArchiveView {
                view: 2,
                url: view_url(record, 900001, 1),
                ark: Some(format!(
                    "https://archives-numerisees.loire-atlantique.fr/ark:42067/{:032x}.fiche={record}.moteur=arko_default_6a6b4ac0309a5",
                    2
                )),
                image: None,
            }],
            view_count: Some(3),
            call_number: Some("E dépôt 99".to_owned()),
            attribution: None,
        })
    );

    // The search, then the register's viewer: nothing more.
    let requests = fetch.requests();
    assert_eq!(requests.len(), 2);
    let search = &requests[0];
    assert!(search.starts_with("/_recherche-api/moteur?refUnique=arko_default_6a6b4ac0309a5&"));
    for expected in [
        "%5Bq%5D%5B%5D=Exampleville&",
        "%5Bq%5D%5B%5D=Bapt%C3%AAmes%5B%5Barko_fiche_6a6b3d70f2db3%5D%5D&",
        "%5Bq%5D%5B%5D=1660%7C1660&",
        "--resultSize=100&",
        "--contenuIds%5B%5D=1289790&",
        "--modeRestit=arko_default_6a6b4d95b8dc8",
    ] {
        assert!(search.contains(expected), "{expected} in {search}");
    }
    // The portal learns the locality, act and year, never the rest.
    for private in ["acte", "Saint-Exemple", "d%C3%A9p%C3%B4t", "vue"] {
        assert!(!search.contains(private), "{private} in {search}");
    }
    assert_eq!(
        requests[1],
        format!("{AD44_VIEWER_ADDRESS}/{record}/arko_default_6a6b4d70c2bc6/image/900001")
    );
}

#[test]
fn several_registers_give_the_filtered_results_unless_the_citation_tells_them_apart() {
    let (target, fetch) = embedded("AD44 - Exampletown - (aucun) - B - 1700", AD44_SEVERAL);
    let Ok(ArchiveTarget::Results { url, matches }) = target else {
        panic!("expected results, got {target:?}");
    };
    assert_eq!(matches, Some(4));
    assert!(url.starts_with(&format!(
        "{AD44_SEARCH_PAGE}?arko_default_6a6b4ac0309a5--ficheFocus=&"
    )));
    assert!(url.contains("%5Bq%5D%5B%5D=Exampletown&"));
    assert_eq!(fetch.requests().len(), 1);

    for (title, record) in [
        (
            "AD44 - Exampletown - Saint-Médard - B - 1700",
            "arko_fiche_0000000000a12",
        ),
        (
            "AD44 - Exampletown - (aucun) - B - 1700 - 9E109/84",
            "arko_fiche_0000000000a13",
        ),
        (
            "AD44 - Exampletown - (aucun) - B - 1700 - vue 3/48",
            "arko_fiche_0000000000a14",
        ),
    ] {
        let (target, _) = embedded(title, AD44_SEVERAL);
        assert_eq!(opened_record(&target.unwrap()), record, "{title}");
    }

    // A call number no row carries: not a guess among the others.
    let (target, _) = embedded(
        "AD44 - Exampletown - Saint-Médard - B - 1700 - 9 E 999 / 9",
        AD44_SEVERAL,
    );
    assert!(matches!(
        target,
        Ok(ArchiveTarget::Results {
            matches: Some(4),
            ..
        })
    ));
}

#[test]
fn no_register_gives_empty_results() {
    let (target, fetch) = embedded("AD44 - Exampleville - (aucun) - B - 1500", AD44_NONE);
    assert!(matches!(
        target,
        Ok(ArchiveTarget::Results {
            matches: Some(0),
            ..
        })
    ));
    assert_eq!(fetch.requests().len(), 1);
}

#[test]
fn a_changed_answer_is_reported_as_such() {
    let title = "AD44 - Exampleville - (aucun) - B - 1660";
    for search in [
        "<html>maintenance</html>",
        r#"{"resultats": {"total": 1, "results": [{"refUnique": "arko_fiche_x"}]}}"#,
        r#"{"resultats": {"total": 1, "results": [{"refUnique": "arko_fiche_x"}], "html": "<table></table>"}}"#,
    ] {
        let (target, _) = embedded(title, search);
        assert!(
            matches!(target, Err(ResolveError::UnexpectedResponse(_))),
            "{search}: {target:?}"
        );
    }

    // Rows whose cells were renamed match no locality: no register, not a
    // wrong one.
    let renamed: &'static str = AD44_ONE.replace("liaison_commune", "commune_liee").leak();
    let (target, _) = embedded(title, renamed);
    assert!(matches!(
        target,
        Ok(ArchiveTarget::Results {
            matches: Some(0),
            ..
        })
    ));

    let mut fetch = Fixtures::new(AD44_ONE);
    fetch.viewer = r#"{"medias": [{"sources": [{"src": "https://elsewhere.example.org/0"}]}]}"#;
    let target = resolve(ArchiveRegistry::embedded(), title, &fetch);
    assert!(matches!(target, Err(ResolveError::UnexpectedResponse(_))));
}

#[test]
fn the_locality_is_matched_on_the_row_not_on_the_engine_s_text_match() {
    // The engine also returns the localities whose name contains the
    // searched one; only `Bourg (Le)` rows are candidates, and the year in
    // Republican form picks the second.
    let (target, fetch) = embedded("AD72 - Le Bourg - (aucun) - N - an III", AD72_TEXT_MATCH);
    assert_eq!(opened_record(&target.unwrap()), "arko_fiche_0000000000b13");
    let search = &fetch.requests()[0];
    assert!(
        search.contains("%5Bq%5D%5B%5D=Bourg%20%28Le%29&"),
        "{search}"
    );
    // The Sarthe engines have no period filter.
    assert!(!search.contains("slider"), "{search}");

    let (target, _) = embedded("AD72 - Le Bourg - (aucun) - N - 1793", AD72_TEXT_MATCH);
    assert_eq!(opened_record(&target.unwrap()), "arko_fiche_0000000000b12");
    let (target, _) = embedded("AD72 - Le Bourg - (aucun) - N - acte 3", AD72_TEXT_MATCH);
    assert!(matches!(
        target,
        Ok(ArchiveTarget::Results {
            matches: Some(2),
            ..
        })
    ));
}

#[test]
fn a_period_of_several_segments_is_read_segment_by_segment() {
    // Both registers overlap 1650 for the engine; only the second covers it.
    let (target, _) = embedded("AD44 - Exampleville - (aucun) - S - 1650", AD44_PERIODS);
    assert_eq!(opened_record(&target.unwrap()), "arko_fiche_0000000000a22");
    let (target, _) = embedded("AD44 - Exampleville - (aucun) - S - 1660", AD44_PERIODS);
    assert_eq!(opened_record(&target.unwrap()), "arko_fiche_0000000000a21");
}

#[test]
fn registers_sharing_a_call_number_are_told_apart_by_period_and_image_count() {
    let (target, _) = embedded(
        "AD72 - Exampleville - (aucun) - B - 1700 - 1MI 999 R1",
        AD72_SEVERAL,
    );
    assert_eq!(opened_record(&target.unwrap()), "arko_fiche_0000000000b02");
    let (target, _) = embedded(
        "AD72 - Exampleville - (aucun) - BMS - acte 3 - vue 10/115",
        AD72_SEVERAL,
    );
    assert_eq!(opened_record(&target.unwrap()), "arko_fiche_0000000000b03");
    let (target, _) = embedded("AD72 - Exampleville - (aucun) - B - acte 3", AD72_SEVERAL);
    assert!(matches!(
        target,
        Ok(ArchiveTarget::Results {
            matches: Some(4),
            ..
        })
    ));
    // No row holds births: the act criterion is skipped rather than leaving
    // none, and the period finds the parish register of that year.
    let (target, _) = embedded("AD72 - Exampleville - (aucun) - N - 1700", AD72_SEVERAL);
    assert_eq!(opened_record(&target.unwrap()), "arko_fiche_0000000000b02");
}

#[test]
fn civil_status_after_1902_reads_its_period_from_the_act_cell() {
    let (target, fetch) = embedded("AD72 - Exampleville - (aucun) - N - 1915", AD72_AFTER_1902);
    assert_eq!(opened_record(&target.unwrap()), "arko_fiche_0000000000b22");
    assert!(fetch.requests()[0].contains("refUnique=arko_default_678f538cb2d58&"));
}

#[test]
fn a_view_beyond_the_register_opens_the_register() {
    let (target, _) = embedded(
        "AD44 - Exampleville - (aucun) - B - 1660 - vue 5/13",
        AD44_ONE,
    );
    let record = "arko_fiche_0000000000a01";
    assert_eq!(
        target,
        Ok(ArchiveTarget::View {
            url: view_url(record, 900001, 0),
            views: Vec::new(),
            view_count: Some(3),
            call_number: Some("E dépôt 99".to_owned()),
            attribution: None,
        })
    );
}

/// An archive shown in OxidGene's viewer, on an Arkothèque portal.
fn iiif_registry() -> ArchiveRegistry {
    let mut portal: serde_json::Value = serde_json::from_str(
        &serde_json::to_string(
            &ArchiveRegistry::embedded()
                .archive("AD44")
                .unwrap()
                .collections[0]
                .portal,
        )
        .unwrap(),
    )
    .unwrap();
    portal["origin"] = "https://archives.example.org".into();
    let document = serde_json::json!({
        "id": "fr-ad00",
        "country": "FR",
        "level": "departmental",
        "name": "Archives départementales d'Exemple",
        "citation_codes": ["AD00"],
        "website": "https://archives.example.org",
        "display": "iiif",
        "attribution": "Archives d'Exemple, {call_number}, vue {view}",
        "terms": "https://archives.example.org/conditions",
        "collections": [{
            "id": "registers",
            "acts": ["B", "N", "M", "S", "D"],
            "platform": "arkotheque",
            "portal": portal,
        }]
    })
    .to_string();
    ArchiveRegistry::new(&[("fr", document.as_str())], platform::builtin()).unwrap()
}

#[test]
fn an_iiif_archive_gets_its_images_from_the_portal_image_path() {
    let registry = iiif_registry();
    let fetch = Fixtures::new(AD44_ONE);
    let target = resolve(
        &registry,
        "AD00 - Exampleville - (aucun) - B - 1660 - vue 1d-2g/3",
        &fetch,
    )
    .unwrap();
    let ArchiveTarget::View {
        views, attribution, ..
    } = &target
    else {
        panic!("expected a view, got {target:?}");
    };
    assert_eq!(
        attribution.as_deref(),
        Some("Archives d'Exemple, E dépôt 99, vue 1-2")
    );
    let base = "https://archives.example.org/_recherche-images/show/100001/image/900001";
    for (index, view) in views.iter().enumerate() {
        assert_eq!(
            view.image,
            Some(ArchiveImage {
                picture: format!("{base}/{index}/full/!2048,2048/0/default.jpg"),
                thumbnail: format!("{base}/{index}/full/210,141/0/default.jpg"),
                width: 3352,
                height: 2248,
            })
        );
    }
    // The service's own `@id` and the files' paths name internal hosts.
    let serialized = serde_json::to_string(&target).unwrap();
    assert!(!serialized.contains("internal"), "{serialized}");

    let requests = fetch.requests();
    assert_eq!(requests.len(), 4);
    assert_eq!(
        requests[2],
        "/_recherche-images/show/100001/image/900001/0/info.json"
    );
    assert_eq!(
        requests[3],
        "/_recherche-images/show/100001/image/900001/1/info.json"
    );
}

#[test]
fn reads_the_endpoint_and_the_search_page_of_each_collection() {
    let registry = ArchiveRegistry::embedded();
    let ad44 = &registry.archive("AD44").unwrap().collections[0];
    let endpoint = Arkotheque.endpoint(ad44).unwrap();
    assert_eq!(
        endpoint.origin,
        "https://archives-numerisees.loire-atlantique.fr"
    );
    assert_eq!(endpoint.start, AD44_SEARCH_PAGE);
    assert_eq!(endpoint.access, Access::Any);
    assert!(endpoint.other_origins.is_empty());

    let ad72 = registry.archive("AD72").unwrap();
    assert_eq!(ad72.collections.len(), 2);
    for collection in &ad72.collections {
        assert_eq!(
            Arkotheque.endpoint(collection).unwrap().access,
            Access::Browser
        );
    }
    let citation = registry
        .parse("AD72 - Le Bourg - (aucun) - D - 1910")
        .unwrap();
    let (_, collections) = registry.candidates(&citation).unwrap();
    assert_eq!(collections[0].id, "civil-status-after-1902");
    let url = Arkotheque.results_url(collections[0], &citation).unwrap();
    assert!(url.starts_with(
        "https://archives.sarthe.fr/archives-en-ligne/registres-detat-civil-posterieurs-a-1902?arko_default_678f538cb2d58--"
    ));
    assert!(url.contains("D%C3%A9c%C3%A8s%5B%5Barko_fiche_6304c294c39b0%5D%5D"));
}

fn collection_with(change: impl FnOnce(&mut serde_json::Value)) -> Collection {
    let mut collection = ArchiveRegistry::embedded()
        .archive("AD44")
        .unwrap()
        .collections[0]
        .clone();
    change(&mut collection.portal);
    collection
}

#[test]
fn validates_its_settings_against_the_collection() {
    assert_eq!(Arkotheque.validate(&collection_with(|_| {})), Ok(()));
    type Change = fn(&mut serde_json::Value);
    let cases: [(&str, Change); 8] = [
        ("unknown field", |p| p["row_label"] = "x".into()),
        ("https origin", |p| {
            p["origin"] = "http://archives.example.org".into()
        }),
        ("absolute path", |p| p["search_path"] = "chercher".into()),
        ("references", |p| p["engine"] = "arko default".into()),
        ("numeric", |p| p["content_ids"] = serde_json::json!([])),
        ("record key", |p| p["acts"]["B"] = "Baptêmes".into()),
        ("no filter value for `D`", |p| {
            p["acts"].as_object_mut().unwrap().remove("D");
        }),
        ("not an act code", |p| {
            p["acts"]["X"] = "X[[arko_fiche_1]]".into()
        }),
    ];
    for (expected, change) in cases {
        let error = Arkotheque
            .validate(&collection_with(change))
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{expected}: {error}");
    }
}

#[test]
fn an_engine_without_an_act_filter_searches_a_series_only() {
    let census = |change: fn(&mut serde_json::Value)| {
        let mut collection = collection_with(|portal| {
            portal["fields"].as_object_mut().unwrap().remove("act");
            portal.as_object_mut().unwrap().remove("acts");
            portal["cells"]["numbers"] = "matricules".into();
            change(portal);
        });
        collection.acts = vec![Act::Series(crate::Series::Census)];
        collection
    };
    let collection = census(|_| {});
    assert_eq!(Arkotheque.validate(&collection), Ok(()));
    // The search sends no act filter.
    let citation = ArchiveRegistry::embedded()
        .parse("AD44 - Exampleville - Recensement - 1872 - vue 3/40")
        .unwrap();
    let url = Arkotheque.results_url(&collection, &citation).unwrap();
    assert!(url.contains("Exampleville"), "{url}");
    assert!(!url.contains("select"), "{url}");

    let error = Arkotheque
        .validate(&census(|portal| {
            portal["acts"] = serde_json::json!({"RP": "Recensements[[arko_fiche_1]]"});
        }))
        .unwrap_err();
    assert!(
        error.to_string().contains("acts without fields.act"),
        "{error}"
    );
    let mut registers = census(|_| {});
    registers.acts = vec![Act::from_code("B").unwrap()];
    let error = Arkotheque.validate(&registers).unwrap_err();
    assert!(
        error.to_string().contains("`B` needs the act filter"),
        "{error}"
    );
    // With its act filter, a series needs its value like any act.
    let mut mixed = collection_with(|_| {});
    mixed.acts.push(Act::Series(crate::Series::Census));
    let error = Arkotheque.validate(&mixed).unwrap_err();
    assert!(
        error.to_string().contains("no filter value for `RP`"),
        "{error}"
    );
}

#[test]
fn writes_the_locality_in_the_portal_style() {
    let settings = Settings::read(
        &ArchiveRegistry::embedded()
            .archive("AD72")
            .unwrap()
            .collections[0],
    )
    .unwrap();
    for (written, expected) in [
        ("Le Bourg", "Bourg (Le)"),
        ("La Ville-Example", "Ville-Example (La)"),
        ("Les Examples", "Examples (Les)"),
        ("L'Exemple", "Exemple (L')"),
        ("Bourg (Le)", "Bourg (Le)"),
        ("Exampleville", "Exampleville"),
        ("Lens-Example", "Lens-Example"),
    ] {
        let citation = CitationParts::parse(
            &format!("AD72 - {written} - (aucun) - N - 1850"),
            &crate::CitationGrammar::default(),
        )
        .unwrap();
        assert_eq!(settings.locality(&citation), expected, "{written}");
    }
}

#[test]
fn reads_a_locality_of_the_portal_back_as_cited() {
    // The catalogue's styles; the reading itself is `LocalityStyle`'s.
    let registry = ArchiveRegistry::embedded();
    let suffixed = Settings::read(&registry.archive("AD72").unwrap().collections[0]).unwrap();
    assert_eq!(suffixed.locality_style.cited("Bourg (Le)"), "Le Bourg");
    let plain = Settings::read(&registry.archive("AD44").unwrap().collections[0]).unwrap();
    assert_eq!(plain.locality_style.cited("Bourg (Le)"), "Bourg (Le)");
}
