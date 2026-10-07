//! The adapter over anonymized answers shaped like those of the catalogued
//! portals (`fixtures/arkotheque/`, written by its `generate.py`), with the
//! catalogue's own settings of the collection each answer stands for.

use std::pin::pin;
use std::sync::Mutex;
use std::task::{Context, Poll, Waker};

use super::*;
use crate::Access;
use crate::citation::Act;
use crate::transport::{FetchError, PortalRequest};
use crate::{ArchiveRegistry, platform};

const AD44_ONE: &str = include_str!("../../../fixtures/arkotheque/ad44-one.json");
const AD44_SEVERAL: &str = include_str!("../../../fixtures/arkotheque/ad44-several.json");
const AD44_NONE: &str = include_str!("../../../fixtures/arkotheque/ad44-none.json");
const AD44_PERIODS: &str = include_str!("../../../fixtures/arkotheque/ad44-period-segments.json");
const AD44_VIEWER: &str = include_str!("../../../fixtures/arkotheque/ad44-viewer.json");
const AD44_ENGINE: &str = include_str!("../../../fixtures/arkotheque/ad44-engine.json");
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
    /// The search's next page, asked from a row other than the first.
    next: &'static str,
    /// The engine's bare answer, where a filter's values are read.
    engine: &'static str,
    viewer: &'static str,
    info: &'static str,
    requests: Mutex<Vec<String>>,
}

impl Fixtures {
    fn new(search: &'static str) -> Self {
        Self {
            search,
            next: AD44_NONE,
            engine: AD44_ENGINE,
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
            let first_page = url.contains("--from=0&");
            let body = if url.starts_with("/_recherche-api/moteur?") && !url.contains("ficheFocus")
            {
                self.engine
            } else if url.starts_with("/_recherche-api/moteur?") && first_page {
                self.search
            } else if url.starts_with("/_recherche-api/moteur?") {
                self.next
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
            renumbering: None,
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

/// A regression (Maine-et-Loire): a register digitised in parts lists its
/// images from several files while its row's viewer address names the first
/// file only. The target named that file with the view's index in the whole
/// register, beyond the file's images, and the viewer opened the register
/// on its first image. Each view is addressed by its own file and position.
#[test]
fn a_register_of_several_files_opens_each_view_in_its_own_file() {
    let mut fetch = Fixtures::new(AD44_ONE);
    fetch.viewer = include_str!("../../../fixtures/arkotheque/ad44-viewer-files.json");
    let target = resolve(
        ArchiveRegistry::embedded(),
        "AD44 - Exampleville - Saint-Exemple - B - 1660 - E dépôt 99 - vue 3-5/6",
        &fetch,
    );
    let record = "arko_fiche_0000000000a01";
    let Ok(ArchiveTarget::View {
        url,
        views,
        view_count,
        ..
    }) = target
    else {
        panic!("expected a view, got {target:?}");
    };
    assert_eq!(view_count, Some(6));
    let opened: Vec<(u16, &str)> = views
        .iter()
        .map(|view| (view.view, view.url.as_str()))
        .collect();
    assert_eq!(
        opened,
        [
            (3, view_url(record, 900002, 0).as_str()),
            (4, view_url(record, 900003, 0).as_str()),
            (5, view_url(record, 900003, 1).as_str()),
        ]
    );
    assert_eq!(url, view_url(record, 900002, 0));
    // The register's own ARKs, counted across its files.
    assert!(
        views[2]
            .ark
            .as_deref()
            .is_some_and(|ark| ark.contains(&format!("/{:032x}.", 5)))
    );
}

/// A regression: a register bound with earlier years since the citation
/// was numbered (the citation's 4 images of 1662-1668 are the last of the
/// register's 6 of 1658-1668) opened on the cited number, an earlier year's
/// page. The view is moved by the images added before it, and the target
/// says that the numbering differs.
#[test]
fn a_register_bound_with_earlier_years_opens_on_the_estimated_view() {
    let mut fetch = Fixtures::new(AD44_ONE);
    fetch.viewer = include_str!("../../../fixtures/arkotheque/ad44-viewer-files.json");
    let target = resolve(
        ArchiveRegistry::embedded(),
        "AD44 - Exampleville - Saint-Exemple - B - 1662-1668 - E dépôt 99 - vue 2/4",
        &fetch,
    );
    let Ok(ArchiveTarget::View {
        url, renumbering, ..
    }) = target
    else {
        panic!("expected a view, got {target:?}");
    };
    assert_eq!(url, view_url("arko_fiche_0000000000a01", 900003, 0));
    assert_eq!(
        renumbering,
        Some(crate::Renumbering {
            cited_count: 4,
            shifted_by: 2
        })
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
            renumbering: None,
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
    // The lightest page of the portal, which fetches nothing on load.
    assert_eq!(
        endpoint.start,
        "https://archives-numerisees.loire-atlantique.fr/robots.txt"
    );
    assert_eq!(endpoint.access, Access::Any);
    assert!(endpoint.other_origins.is_empty());

    let ad72 = registry.archive("AD72").unwrap();
    assert_eq!(ad72.collections.len(), 6);
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
    // The reader's page shows the portal's default 25 rows, which render
    // faster than the 100 the adapter reads.
    assert!(url.contains("--resultSize=25&"), "{url}");
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
    let cases: [(&str, Change); 10] = [
        ("unknown field", |p| p["row_label"] = "x".into()),
        ("https origin", |p| {
            p["origin"] = "http://archives.example.org".into()
        }),
        ("absolute path", |p| p["search_path"] = "chercher".into()),
        ("references", |p| p["engine"] = "arko default".into()),
        ("numeric", |p| p["content_ids"] = serde_json::json!([])),
        ("empty filter value", |p| p["acts"]["B"] = "".into()),
        (
            "only the period filter has an end",
            |p| {
                p["fields"]["locality"] =
                    serde_json::json!({"ref": "arko_default_1", "end": "arko_default_2"})
            },
        ),
        ("is no cell", |p| p["cells"]["locality"] = "#0".into()),
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
    // Kinds an engine cannot filter are told apart by the act cell.
    let mut registers = census(|_| {});
    registers.acts = vec![Act::from_code("B").unwrap(), Act::from_code("N").unwrap()];
    assert_eq!(Arkotheque.validate(&registers), Ok(()));
    let mut registers = census(|portal| {
        portal["cells"].as_object_mut().unwrap().remove("act");
    });
    registers.acts = vec![Act::from_code("B").unwrap(), Act::from_code("N").unwrap()];
    let error = Arkotheque.validate(&registers).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("several document kinds without an act filter need cells.act"),
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

// The shapes of the other portals' engines, over the catalogue's settings
// of the collection each fixture names (`generate.py`).

const AD08_MATRICULES: &str = include_str!("../../../fixtures/arkotheque/ad08-matricules.json");
const AD10_TITLES: &str = include_str!("../../../fixtures/arkotheque/ad10-titles.json");
const AD15_QUALIFIED: &str = include_str!("../../../fixtures/arkotheque/ad15-qualified.json");
const AD24_CENSUS: &str = include_str!("../../../fixtures/arkotheque/ad24-census.json");
const AD24_CENSUS_ENGINE: &str =
    include_str!("../../../fixtures/arkotheque/ad24-census-engine.json");
const AD36_SHARED_ACTS: &str = include_str!("../../../fixtures/arkotheque/ad36-shared-acts.json");
const AD65_CODES: &str = include_str!("../../../fixtures/arkotheque/ad65-codes.json");
const AD72_CENSUS: &str = include_str!("../../../fixtures/arkotheque/ad72-census.json");
const AD72_PAGE_1: &str = include_str!("../../../fixtures/arkotheque/ad72-page-1.json");
const AD72_PAGE_2: &str = include_str!("../../../fixtures/arkotheque/ad72-page-2.json");

/// The record a fixture names by its number.
fn fixture_record(number: u32) -> String {
    format!("arko_fiche_{number:013x}")
}

#[test]
fn a_filter_value_shared_by_two_kinds_is_told_apart_by_the_act_cells() {
    // `Baptêmes / Naissances` finds both kinds; one span per kind says
    // which register holds births.
    let (target, fetch) = embedded("AD36 - Exampleville - (aucun) - N - 1795", AD36_SHARED_ACTS);
    assert_eq!(opened_record(&target.unwrap()), fixture_record(0xd02));
    let search = &fetch.requests()[0];
    assert!(
        search.contains("%5Bq%5D%5B%5D=Bapt%C3%AAmes%20%2F%20Naissances&"),
        "{search}"
    );
    let (target, _) = embedded("AD36 - Exampleville - (aucun) - B - 1795", AD36_SHARED_ACTS);
    assert_eq!(opened_record(&target.unwrap()), fixture_record(0xd01));
}

#[test]
fn a_qualified_locality_is_its_commune_and_never_one_of_its_hamlets() {
    let (target, fetch) = embedded("AD15 - Exampleville - (aucun) - B - 1750", AD15_QUALIFIED);
    let target = target.unwrap();
    assert_eq!(opened_record(&target), fixture_record(0xe01));
    // The call number is the named cell's: the title holds the period.
    let ArchiveTarget::View { call_number, .. } = &target else {
        panic!("expected a view, got {target:?}");
    };
    assert_eq!(call_number.as_deref(), Some("9 Mi 99/2"));
    // The locality is searched by its name alone.
    assert!(fetch.requests()[0].contains("%5Bq%5D%5B%5D=Exampleville&"));
    // The births of the commune, read from the acts of a bare cell.
    let (target, _) = embedded("AD15 - Exampleville - (aucun) - N - 1850", AD15_QUALIFIED);
    assert_eq!(opened_record(&target.unwrap()), fixture_record(0xe03));
    // The hamlet is not the commune it names: its call number is on none of
    // the commune's rows.
    let (target, _) = embedded(
        "AD15 - Exampleville - (aucun) - B - 1785 - 9 Mi 98/5",
        AD15_QUALIFIED,
    );
    assert!(matches!(
        target,
        Ok(ArchiveTarget::Results {
            matches: Some(2),
            ..
        })
    ));
}

#[test]
fn several_values_of_one_kind_are_searched_together() {
    let registry = ArchiveRegistry::embedded();
    let citation = registry
        .parse("AD44 - Exampleville - Liste du contingent - 1870 - vue 3/40")
        .unwrap();
    assert_eq!(citation.act, Act::Series(crate::Series::ConscriptionList));
    let (_, collections) = registry.candidates(&citation).unwrap();
    let url = Arkotheque.results_url(collections[0], &citation).unwrap();
    for expected in [
        "%5Bop%5D=OR&",
        "%5Bq%5D%5B%5D=Liste%20d%C3%A9partementale%20du%20contingent&",
        "%5Bq%5D%5B%5D=Liste%20de%20la%20garde%20nationale%20mobile&",
        // The class is typed, the bureau chosen.
        "%5Bq%5D%5B%5D=1870&",
        "%5Bextras%5D%5Bmode%5D=autocomplete&",
        "%5Bextras%5D%5Bmode%5D=select&",
    ] {
        assert!(url.contains(expected), "{expected} in {url}");
    }
}

#[test]
fn matricules_shown_in_two_cells_single_out_a_volume() {
    let title = "AD08 - Exampleville - Registres matricules - 1900 - matricule 150 - vue 3/150";
    let (target, fetch) = embedded(title, AD08_MATRICULES);
    assert_eq!(opened_record(&target.unwrap()), fixture_record(0xf02));
    // The engine has no locality filter: the bureau is read from the rows.
    let search = &fetch.requests()[0];
    assert!(!search.contains("Exampleville"), "{search}");
    assert!(search.contains("%5Bq%5D%5B%5D=1900%7C1900&"), "{search}");
}

#[test]
fn a_title_holds_the_call_number_before_the_locality_or_the_period() {
    let (target, _) = embedded(
        "AD10 - Le Bourg - (aucun) - N - 1850 - 9E99901",
        AD10_TITLES,
    );
    let target = target.unwrap();
    assert_eq!(opened_record(&target), fixture_record(0xa01));
    let ArchiveTarget::View { call_number, .. } = &target else {
        panic!("expected a view, got {target:?}");
    };
    assert_eq!(call_number.as_deref(), Some("9E99901"));

    // A row naming several localities is each one's; the kinds are codes,
    // and the period is searched by its first and last year.
    let (target, fetch) = embedded(
        "AD65 - Exampleville - (aucun) - M - an XI - 9 E 9/5",
        AD65_CODES,
    );
    let target = target.unwrap();
    assert_eq!(opened_record(&target), fixture_record(0xb02));
    let ArchiveTarget::View { call_number, .. } = &target else {
        panic!("expected a view, got {target:?}");
    };
    assert_eq!(call_number.as_deref(), Some("9 E 9/5"));
    let search = &fetch.requests()[0];
    for end in ["arko_default_63692b66ded44", "arko_default_63bd8f04715bc"] {
        assert!(
            search.contains(&format!("%5B{end}%5D%5Bq%5D%5B%5D=1802&")),
            "{end} in {search}"
        );
    }
}

#[test]
fn a_census_of_several_lists_a_year_is_told_apart_by_call_number_and_images() {
    let title = "AD72 - Exampleville - Recensement - 1931 (A-H, collection communale) - 9 Mi 9999_ 19 - vue 490d/662";
    let (target, _) = embedded(title, AD72_CENSUS);
    assert_eq!(opened_record(&target.unwrap()), fixture_record(0xc01));
    let (target, _) = embedded(
        "AD72 - Exampleville - Recensement - 1931 - vue 12/540",
        AD72_CENSUS,
    );
    assert_eq!(opened_record(&target.unwrap()), fixture_record(0xc02));
    // Two lists of that year have as many images: the citation needs its
    // call number.
    let (target, _) = embedded(
        "AD72 - Exampleville - Recensement - 1931 - vue 490/662",
        AD72_CENSUS,
    );
    assert!(matches!(
        target,
        Ok(ArchiveTarget::Results {
            matches: Some(2),
            ..
        })
    ));
}

#[test]
fn a_cited_call_number_missing_from_the_first_page_is_looked_for_on_the_next() {
    // A populated locality without a period filter: 3 rows of 5 on the
    // first page, the cited register on the second.
    let title =
        "AD72 - Le Bourg - (aucun) - M - 1880-1882 - 9Mi 999_374-376 - acte 238 - vue 289d/564";
    let mut fetch = Fixtures::new(AD72_PAGE_1);
    fetch.next = AD72_PAGE_2;
    let target = resolve(ArchiveRegistry::embedded(), title, &fetch).unwrap();
    assert_eq!(opened_record(&target), fixture_record(0x2000));
    let requests = fetch.requests();
    assert_eq!(requests.len(), 3, "{requests:?}");
    assert!(requests[0].contains("--from=0&"));
    assert!(requests[1].contains("--from=3&"));
    assert!(requests[2].starts_with("/_recherche-api/visionneuse-infos/"));

    // One microfilm of the cited range is that register too.
    let fetch = Fixtures {
        next: AD72_PAGE_2,
        ..Fixtures::new(AD72_PAGE_1)
    };
    let target = resolve(
        ArchiveRegistry::embedded(),
        "AD72 - Le Bourg - (aucun) - M - 1881 - 9Mi 999_375",
        &fetch,
    );
    assert_eq!(opened_record(&target.unwrap()), fixture_record(0x2000));

    // A citation without a call number reads the first page only.
    let fetch = Fixtures {
        next: AD72_PAGE_2,
        ..Fixtures::new(AD72_PAGE_1)
    };
    let target = resolve(
        ArchiveRegistry::embedded(),
        "AD72 - Le Bourg - (aucun) - M - 1881",
        &fetch,
    );
    assert!(target.is_ok());
    let searches = fetch
        .requests()
        .iter()
        .filter(|request| request.starts_with("/_recherche-api/moteur?"))
        .count();
    assert_eq!(searches, 1);
}

#[test]
fn a_keyed_value_is_read_from_the_engine_lists_first() {
    let mut fetch = Fixtures::new(AD24_CENSUS);
    fetch.engine = AD24_CENSUS_ENGINE;
    let target = resolve(
        ArchiveRegistry::embedded(),
        "AD24 - Exampleville - Recensement - 1836 - vue 3/3",
        &fetch,
    );
    assert_eq!(opened_record(&target.unwrap()), fixture_record(0x3001));
    let requests = fetch.requests();
    assert!(!requests[0].contains("ficheFocus"), "{}", requests[0]);
    assert!(
        requests[1]
            .contains("%5Bq%5D%5B%5D=1836%20%5B%5B0000000000000000000000000000000000001836%5D%5D&"),
        "{}",
        requests[1]
    );
    // A register its row shows without an image count opens all the same.
    let mut fetch = Fixtures::new(AD24_CENSUS);
    fetch.engine = AD24_CENSUS_ENGINE;
    let target = resolve(
        ArchiveRegistry::embedded(),
        "AD24 - Exampleville - Recensement - 1841 - vue 3/3",
        &fetch,
    );
    assert_eq!(opened_record(&target.unwrap()), fixture_record(0x3002));
}

#[test]
fn reads_act_words_cells_and_titles() {
    for (text, code) in [
        ("Baptêmes, Mariages, Sépultures", Some("BMS")),
        ("1850 (naissances)", Some("N")),
        ("Sépultures puis décès", Some("SD")),
        ("Publications de mariages", Some("MP")),
        (
            "Tables décennales des naissances, mariages, décès",
            Some("TD"),
        ),
        ("Table alphabétique", None),
        ("Liste nominative", None),
    ] {
        assert_eq!(page::act_code(text).as_deref(), code, "{text}");
    }
    for (setting, cell) in [
        ("cote", Some(page::Cell::Champ("cote".to_owned()))),
        ("#3", Some(page::Cell::Column(3))),
        ("#title", Some(page::Cell::Title)),
        ("#none", Some(page::Cell::Nowhere)),
        ("#0", None),
        ("bad cell", None),
    ] {
        assert_eq!(
            page::Cell::try_from(setting.to_owned()).ok(),
            cell,
            "{setting}"
        );
    }
}

#[test]
fn reads_image_paths_written_on_the_portal_host() {
    let answer = r#"{"medias": [{"sources": [{"src": "https://www.archives.example.org/_recherche-images/show/1/image/2/0"}]}]}"#;
    let sources = page::viewer_sources(answer, "https://archives.example.org").unwrap();
    assert_eq!(sources[0].src, "/_recherche-images/show/1/image/2/0");
    let elsewhere = r#"{"medias": [{"sources": [{"src": "https://elsewhere.example.org/_recherche-images/show/1/image/2/0"}]}]}"#;
    assert!(page::viewer_sources(elsewhere, "https://archives.example.org").is_err());
}

const AD40_RENDERED: &str = include_str!("../../../fixtures/arkotheque/ad40-rendered.html");
const AD40_RENDERED_NONE: &str =
    include_str!("../../../fixtures/arkotheque/ad40-rendered-none.html");
const AD40_SEARCH_PAGE: &str =
    "https://archives.landes.fr/faire-une-recherche/archives-numerisees/etat-civil";

/// A portal read by its pages: each page load is answered with `page` and
/// recorded, and so is a script's request, which the portal would refuse.
struct Pages {
    page: String,
    loads: Mutex<Vec<(String, String)>>,
    requests: Mutex<Vec<String>>,
}

impl Pages {
    fn new(page: &str) -> Self {
        Self {
            page: page.to_owned(),
            loads: Mutex::new(Vec::new()),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn loads(&self) -> Vec<(String, String)> {
        self.loads.lock().unwrap().clone()
    }
}

impl PortalFetch for Pages {
    fn request<'a>(
        &'a self,
        request: &'a PortalRequest,
    ) -> BoxFuture<'a, Result<String, FetchError>> {
        self.requests.lock().unwrap().push(request.url.clone());
        Box::pin(async { Err(FetchError::NotAllowed) })
    }

    fn page<'a>(
        &'a self,
        path_and_query: &'a str,
        ready: &'a str,
    ) -> BoxFuture<'a, Result<String, FetchError>> {
        self.loads
            .lock()
            .unwrap()
            .push((path_and_query.to_owned(), ready.to_owned()));
        Box::pin(async { Ok(self.page.clone()) })
    }
}

/// Resolves `title` in the Landes registers, read by their pages.
fn resolve_pages(title: &str, pages: &Pages) -> Result<ArchiveTarget, ResolveError> {
    let registry = ArchiveRegistry::embedded();
    let citation = registry.parse(title).expect("a normalized citation");
    let (archive, collections) = registry.candidates(&citation).expect("a catalogued act");
    block_on(Arkotheque.resolve(archive, collections[0], &citation, pages))
}

fn landes_view_url(record: &str, file: u32, index: u16) -> String {
    format!(
        "{AD40_SEARCH_PAGE}?detail={record}#/_recherche-api/visionneuse-infos/arko_default_62a88e82782fb/{record}/arko_default_0000000004001/image/{file}/{index}"
    )
}

#[test]
fn a_portal_read_by_its_pages_is_searched_by_loading_its_search_page() {
    let registry = ArchiveRegistry::embedded();
    let landes = &registry.archive("AD40").unwrap().collections[0];
    let endpoint = Arkotheque.endpoint(landes).unwrap();
    assert_eq!(endpoint.access, Access::Page);
    assert_eq!(endpoint.start, "https://archives.landes.fr/robots.txt");

    let pages = Pages::new(AD40_RENDERED);
    let target = resolve_pages(
        "AD40 - Exampleville - (aucun) - N - 1850 - vue 12/296",
        &pages,
    );
    // The locality's text match also lists Exampleville-lès-Bois; the
    // period then singles out the register, counted by its row, since the
    // portal refuses the viewer's image list to a script.
    let record = "arko_fiche_0000000004002";
    let url = landes_view_url(record, 994002, 11);
    assert_eq!(
        target,
        Ok(ArchiveTarget::View {
            url: url.clone(),
            views: vec![ArchiveView {
                view: 12,
                url,
                ark: None,
                image: None,
            }],
            view_count: Some(296),
            call_number: Some("9 E 99/2".to_owned()),
            attribution: None,
            renumbering: None,
        })
    );
    assert!(pages.requests.lock().unwrap().is_empty());
    let loads = pages.loads();
    assert_eq!(loads.len(), 1);
    let (path, ready) = &loads[0];
    assert!(
        path.starts_with(
            "/faire-une-recherche/archives-numerisees/etat-civil?arko_default_62a88e82782fb--ficheFocus="
        ),
        "{path}"
    );
    assert!(path.contains("=Exampleville&"), "{path}");
    assert!(path.contains("=1850%7C1850&"), "{path}");
    assert_eq!(ready, page::RENDERED_RESULTS);

    // A view beyond the row's count opens the register on its first view.
    let target = resolve_pages(
        "AD40 - Exampleville - (aucun) - N - 1850 - vue 400/400",
        &pages,
    );
    let Ok(ArchiveTarget::View { url, views, .. }) = target else {
        panic!("a view target: {target:?}");
    };
    assert!(views.is_empty());
    assert_eq!(url, landes_view_url(record, 994002, 0));
}

#[test]
fn a_rendered_page_without_rows_or_kept_by_an_anti_bot_page() {
    let target = resolve_pages(
        "AD40 - Exampleville - (aucun) - N - 1650",
        &Pages::new(AD40_RENDERED_NONE),
    );
    let Ok(ArchiveTarget::Results { url, matches }) = target else {
        panic!("results: {target:?}");
    };
    assert_eq!(matches, Some(0));
    assert!(url.starts_with(AD40_SEARCH_PAGE), "{url}");

    let blocked = "<html><head><title>Attention Required! | Cloudflare</title></head>\
        <body><div id=\"cf-error-details\">Sorry, you have been blocked</div></body></html>";
    assert_eq!(
        resolve_pages(
            "AD40 - Exampleville - (aucun) - N - 1850",
            &Pages::new(blocked)
        ),
        Err(ResolveError::Challenged)
    );
    assert!(matches!(
        resolve_pages(
            "AD40 - Exampleville - (aucun) - N - 1850",
            &Pages::new("<html><body>Maintenance</body></html>")
        ),
        Err(ResolveError::UnexpectedResponse(_))
    ));
}

#[test]
fn a_portal_read_by_its_pages_reads_every_part_from_a_cell() {
    let paged = |change: fn(&mut serde_json::Value)| {
        collection_with(|portal| {
            portal["transport"] = "page".into();
            portal["cells"]["call_number"] = "cote".into();
            change(portal);
        })
    };
    assert_eq!(Arkotheque.validate(&paged(|_| {})), Ok(()));
    type Change = fn(&mut serde_json::Value);
    let cases: [(&str, Change); 3] = [
        (
            "no keyed filter",
            |p| {
                p["fields"]["locality"] =
                    serde_json::json!({"ref": "arko_default_6a3b8762b0f5a", "keyed": true})
            },
        ),
        ("call number in a cell", |p| {
            p["cells"].as_object_mut().unwrap().remove("call_number");
        }),
        ("no #title", |p| p["cells"]["parish"] = "#title".into()),
    ];
    for (expected, change) in cases {
        let error = Arkotheque.validate(&paged(change)).unwrap_err().to_string();
        assert!(error.contains(expected), "{expected}: {error}");
    }
}

#[test]
fn the_live_probe_reads_a_portal_by_its_rendered_pages() {
    use crate::live::{Outcome, Probe, Step};

    let registry = ArchiveRegistry::embedded();
    let landes = &registry.archive("AD40").unwrap().collections[0];
    let pages = Pages::new(AD40_RENDERED);
    // The first locality the unfiltered page's rows name.
    assert_eq!(
        block_on(Arkotheque.search_page(landes, &pages)).as_deref(),
        Ok("Exampleville")
    );
    let (path, _) = &pages.loads()[0];
    assert!(path.contains("--resultSize=25&"), "{path}");
    assert!(!path.contains("groupes%5D%5B0"), "{path}");

    let act = Act::from_code("N").unwrap();
    let registers = block_on(Arkotheque.registers(landes, "Exampleville", &act, &pages)).unwrap();
    assert_eq!(registers.len(), 3);
    // Counted by their rows: the viewer's list is not to be asked for.
    assert_eq!(registers[1].images, Some(296));
    assert_eq!(registers[1].call_number.as_deref(), Some("9 E 99/2"));
    assert!(pages.requests.lock().unwrap().is_empty());

    let without_act_filter = AD40_RENDERED.replace("aria-filtre-arko_default_62a88ef500cdf", "");
    let failure =
        block_on(Arkotheque.search_page(landes, &Pages::new(&without_act_filter))).unwrap_err();
    assert_eq!(
        (failure.step, failure.outcome),
        (Step::SearchPage, Outcome::Drift)
    );
    assert!(failure.received.contains("act filter"), "{failure:?}");
}

#[test]
fn only_an_adapter_that_reads_pages_takes_a_portal_read_by_its_pages() {
    let document = |platform: &str, portal: serde_json::Value| {
        serde_json::json!({
            "id": "fr-ad00", "country": "FR", "level": "departmental",
            "name": "Archives of Example", "citation_codes": ["AD00"],
            "website": "https://archives.example.org",
            "collections": [{"id": "registers", "acts": ["N"], "platform": platform, "portal": portal}]
        })
        .to_string()
    };
    let landes = &ArchiveRegistry::embedded()
        .archive("AD40")
        .unwrap()
        .collections[0];
    let arkotheque = document("arkotheque", landes.portal.clone());
    assert!(ArchiveRegistry::new(&[("fr", arkotheque.as_str())], platform::builtin()).is_ok());
    let ligeo = document(
        "ligeo",
        serde_json::json!({
            "origin": "https://archives.example.org", "transport": "page",
            "search": "etatcivil", "node": 1,
            "fields": {"locality": "RECH_commune", "act": "RECH_acte"},
            "acts": {"N": "naissance"}
        }),
    );
    let Err(error) = ArchiveRegistry::new(&[("fr", ligeo.as_str())], platform::builtin()) else {
        panic!("a refused collection");
    };
    let error = error.to_string();
    assert!(error.contains("searches no portal by its pages"), "{error}");
}

const AD75_CEMETERY: &str = include_str!("../../../fixtures/arkotheque/ad75-cemetery.json");
const AD75_CEMETERY_VIEWER: &str =
    include_str!("../../../fixtures/arkotheque/ad75-cemetery-viewer.json");

/// Resolves a citation of the Paris cemeteries' burial registers over the
/// anonymized parts of two registers of one cemetery.
fn cemetery(title: &str) -> (Result<ArchiveTarget, ResolveError>, Fixtures) {
    let fetch = Fixtures {
        viewer: AD75_CEMETERY_VIEWER,
        ..Fixtures::new(AD75_CEMETERY)
    };
    let target = resolve(ArchiveRegistry::embedded(), title, &fetch);
    (target, fetch)
}

#[test]
fn a_burial_opens_in_the_part_of_its_register_holding_its_entry() {
    let (target, fetch) =
        cemetery("AD75 - Exampleville - C - 1918 - XXX_RJ19181918_01 - ordre 981 - vue 20/31");
    let target = target.unwrap();
    assert_eq!(opened_record(&target), fixture_record(0x7504));
    let ArchiveTarget::View {
        views,
        view_count,
        call_number,
        ..
    } = &target
    else {
        panic!("expected a view, got {target:?}");
    };
    assert_eq!(views.len(), 1);
    assert!(
        views[0].url.ends_with("/image/975004/19"),
        "{}",
        views[0].url
    );
    assert_eq!(*view_count, Some(31));
    assert_eq!(call_number.as_deref(), Some("XXX_RJ19181918_01"));
    // The cemetery chosen in its list, the year on the slider.
    let search = &fetch.requests()[0];
    for expected in [
        "%5Bq%5D%5B%5D=Exampleville&",
        "%5Bextras%5D%5Bmode%5D=select&",
        "%5Bq%5D%5B%5D=1918%7C1918&",
    ] {
        assert!(search.contains(expected), "{expected} in {search}");
    }
}

#[test]
fn a_register_over_two_years_is_searched_over_both() {
    // No year in the citation: its call number's period is searched, and
    // the entry number picks the part, each part being dated by its start.
    let (target, fetch) =
        cemetery("AD75 - Exampleville - C - XXX_RJ19171918_04 - ordre 1000 - vue 9/31");
    assert_eq!(opened_record(&target.unwrap()), fixture_record(0x7501));
    let search = &fetch.requests()[0];
    assert!(search.contains("%5Bq%5D%5B%5D=1917%7C1918&"), "{search}");
    // A cited year after the part's start.
    let (target, _) =
        cemetery("AD75 - Exampleville - C - 1918 - XXX_RJ19171918_04 - ordre 300 - vue 5/31");
    assert_eq!(opened_record(&target.unwrap()), fixture_record(0x7500));
    // Without the call number, the entry numbers of both registers match.
    let (target, _) = cemetery("AD75 - Exampleville - C - 1918 - ordre 981 - vue 20/31");
    assert!(matches!(
        target,
        Ok(ArchiveTarget::Results {
            matches: Some(2),
            ..
        })
    ));
}

#[test]
fn only_a_slider_spans_the_cited_period() {
    let registry = ArchiveRegistry::embedded();
    let paris = registry.archive("AD75").unwrap();
    let mut collection = paris
        .collections
        .iter()
        .find(|collection| collection.id == "cemetery-registers")
        .unwrap()
        .clone();
    assert!(Settings::read(&collection).is_ok());
    collection.portal["fields"]["period"]["mode"] = "input".into();
    assert!(Settings::read(&collection).is_err());
    collection.portal["fields"]["period"]["mode"] = "slider".into();
    collection.portal["fields"]["locality"]["span"] = true.into();
    assert!(Settings::read(&collection).is_err());
}
