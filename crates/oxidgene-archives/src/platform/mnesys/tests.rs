//! The adapter over anonymized answers shaped like those of the Indre-et-Loire,
//! Calvados and Marne portals (`fixtures/mnesys/`, written by its
//! `generate.py`), with the catalogue's own settings for the three archives.

use std::pin::pin;
use std::sync::Mutex;
use std::task::{Context, Poll, Waker};

use super::*;
use crate::transport::{FetchError, PortalRequest};
use crate::{ArchiveRegistry, platform};

const AD37_ONE: &str = include_str!("../../../fixtures/mnesys/ad37-one.html");
const AD37_SEVERAL: &str = include_str!("../../../fixtures/mnesys/ad37-several.html");
const AD37_TABLES: &str = include_str!("../../../fixtures/mnesys/ad37-tables.html");
const AD37_NONE: &str = include_str!("../../../fixtures/mnesys/ad37-none.html");
const AD37_FIRST: &str = include_str!("../../../fixtures/mnesys/ad37-visualizer-first.json");
const AD37_WINDOW: &str = include_str!("../../../fixtures/mnesys/ad37-visualizer-window.json");
const AD37_INFO: &str = include_str!("../../../fixtures/mnesys/ad37-info.json");
const AD37_MANIFEST: &str = include_str!("../../../fixtures/mnesys/ad37-manifest.json");
const AD14_RESULTS: &str = include_str!("../../../fixtures/mnesys/ad14-results.html");
const AD14_WINDOW: &str = include_str!("../../../fixtures/mnesys/ad14-visualizer.json");
const AD51_RESULTS: &str = include_str!("../../../fixtures/mnesys/ad51-results.html");
const AD51_ONE: &str = include_str!("../../../fixtures/mnesys/ad51-one.html");
const AD51_WINDOW: &str = include_str!("../../../fixtures/mnesys/ad51-visualizer.json");

const AD37_ORIGIN: &str = "https://archives.touraine.fr";

/// The image identifier the fixtures give the register's `serial`-th image.
fn image_id(serial: u32) -> String {
    format!("00000000-0000-4000-8000-{serial:012}")
}

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
    search: String,
    /// The viewer's answer for a window starting at the first image, and for
    /// any other.
    first: &'static str,
    window: &'static str,
    info: &'static str,
    manifest: &'static str,
    requests: Mutex<Vec<String>>,
}

impl Fixtures {
    fn new(search: &str, window: &'static str) -> Self {
        Self {
            search: search.to_owned(),
            first: AD37_FIRST,
            window,
            info: AD37_INFO,
            manifest: AD37_MANIFEST,
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
            let body = if url.starts_with("/search/results?") {
                self.search.as_str()
            } else if url.starts_with("/visualizer/api?") {
                if url.contains("&start=0&") {
                    self.first
                } else {
                    self.window
                }
            } else if url.ends_with("/info.json") {
                self.info
            } else if url.ends_with("/manifest.json") {
                self.manifest
            } else {
                return Err(FetchError::Status(404));
            };
            Ok(body.to_owned())
        })
    }
}

fn resolve_in(
    registry: &ArchiveRegistry,
    title: &str,
    fetch: &Fixtures,
) -> Result<ArchiveTarget, ResolveError> {
    let citation = registry.parse(title).expect("a normalized citation");
    let (archive, collections) = registry.candidates(&citation).expect("a catalogued act");
    block_on(Mnesys.resolve(archive, collections[0], &citation, fetch))
}

fn embedded(
    title: &str,
    search: &str,
    window: &'static str,
) -> (Result<ArchiveTarget, ResolveError>, Fixtures) {
    let fetch = Fixtures::new(search, window);
    let target = resolve_in(ArchiveRegistry::embedded(), title, &fetch);
    (target, fetch)
}

/// The register's name in the address a target opens: `/ark:/<naan>/<name>/…`.
fn register_of(target: &ArchiveTarget) -> &str {
    let url = target.url();
    let start = url.find("/ark:/").expect("an ARK") + "/ark:/".len();
    url[start..].split('/').nth(1).expect("a register name")
}

fn results_matches(target: &Result<ArchiveTarget, ResolveError>) -> usize {
    match target {
        Ok(ArchiveTarget::Results { matches, .. }) => matches.expect("a count"),
        other => panic!("expected results, got {other:?}"),
    }
}

#[test]
fn one_register_opens_on_the_cited_image_with_its_own_ark() {
    let (target, fetch) = embedded(
        "AD37 - Exampleville - (aucun) - N - 1850 - 6NUM8/999/050 - vue 150/315",
        AD37_ONE,
        AD37_WINDOW,
    );
    let ark = format!("{AD37_ORIGIN}/ark:/99937/aaaaaaaaaaaa/{}", image_id(249));
    let base = format!(
        "{AD37_ORIGIN}/iiif/ark:/99937/aaaaaaaaaaaa/{}",
        image_id(249)
    );
    assert_eq!(
        target,
        Ok(ArchiveTarget::View {
            url: ark.clone(),
            views: vec![ArchiveView {
                view: 150,
                url: ark.clone(),
                ark: Some(ark),
                image: Some(ArchiveImage {
                    picture: format!("{base}/full/max/0/default.jpg"),
                    thumbnail: format!("{AD37_ORIGIN}/images/{}_thumbnail.jpg", image_id(249)),
                    width: 3600,
                    height: 2614,
                }),
            }],
            view_count: Some(315),
            call_number: Some("6NUM8/999/050".to_owned()),
            attribution: Some(
                "Archives départementales d'Indre-et-Loire, 6NUM8/999/050, vue 150".to_owned()
            ),
        })
    );

    // The search, the one image's viewer window, its `info.json`: no list.
    let requests = fetch.requests();
    assert_eq!(requests.len(), 3);
    let search = &requests[0];
    assert!(search.starts_with("/search/results?formUuid=e9414896-40cc-4ec3-936c-8acdfdb11770&"));
    for expected in [
        "mode=list&",
        "sort=date_asc&",
        "0-controlledAccessGeographicName%5B%5D=Exampleville%20%28Indre-et-Loire%2C%20France%29&",
        "2-controlledAccessPhysicalCharacteristic%5B%5D=Naissances&",
        "4-date=1850&",
        "resultsPerPage=80",
    ] {
        assert!(search.contains(expected), "{expected} in {search}");
    }
    // The portal learns the locality, act and year, never the rest.
    for private in ["6NUM8", "vue", "150", "315"] {
        assert!(!search.contains(private), "{private} in {search}");
    }
    assert_eq!(
        requests[1],
        "/visualizer/api?arkName=aaaaaaaaaaaa&start=149&end=149&group=0"
    );
    assert_eq!(
        requests[2],
        format!("/iiif/ark:/99937/aaaaaaaaaaaa/{}/info.json", image_id(249))
    );
}

#[test]
fn the_first_image_comes_in_an_array_and_others_in_an_object() {
    let (target, _) = embedded(
        "AD37 - Exampleville - (aucun) - N - 1850 - vue 1/315",
        AD37_ONE,
        AD37_WINDOW,
    );
    let Ok(ArchiveTarget::View { views, .. }) = target else {
        panic!("expected a view, got {target:?}");
    };
    assert_eq!(
        views[0].url,
        format!("{AD37_ORIGIN}/ark:/99937/aaaaaaaaaaaa/{}", image_id(100))
    );
}

#[test]
fn a_range_of_views_shares_one_window() {
    let (target, fetch) = embedded(
        "AD37 - Exampleville - (aucun) - N - 1850 - vue 150-151/315",
        AD37_ONE,
        AD37_WINDOW,
    );
    let Ok(ArchiveTarget::View {
        views, attribution, ..
    }) = target
    else {
        panic!("expected a view, got {target:?}");
    };
    assert_eq!(
        views.iter().map(|view| view.view).collect::<Vec<_>>(),
        [150, 151]
    );
    assert!(views[1].url.ends_with(&image_id(250)));
    assert_eq!(
        attribution.as_deref(),
        Some("Archives départementales d'Indre-et-Loire, 6NUM8/999/050, vue 150-151")
    );
    let requests = fetch.requests();
    assert_eq!(
        requests[1],
        "/visualizer/api?arkName=aaaaaaaaaaaa&start=149&end=150&group=0"
    );
    // The window, then one `info.json` per view.
    assert_eq!(requests.len(), 4);
}

#[test]
fn windows_group_neighbouring_views_up_to_the_limit() {
    assert_eq!(windows(&[5]), [(5, 5)]);
    assert_eq!(windows(&[5, 6]), [(5, 6)]);
    assert_eq!(windows(&[5, 14]), [(5, 14)]);
    assert_eq!(windows(&[5, 15, 16]), [(5, 5), (15, 16)]);
    assert_eq!(windows(&[1, 300]), [(1, 1), (300, 300)]);
}

#[test]
fn a_manifest_gives_the_images_and_their_sizes_in_one_request() {
    let registry = with_portal("AD37", |portal| portal["image_source"] = "manifest".into());
    let fetch = Fixtures::new(AD37_ONE, AD37_WINDOW);
    let target = resolve_in(
        &registry,
        "AD37 - Exampleville - (aucun) - N - 1850 - vue 2/315",
        &fetch,
    )
    .unwrap();
    let ArchiveTarget::View { views, .. } = &target else {
        panic!("expected a view, got {target:?}");
    };
    assert_eq!(
        views[0].image,
        Some(ArchiveImage {
            picture: format!(
                "{AD37_ORIGIN}/iiif/ark:/99937/aaaaaaaaaaaa/{}/full/max/0/default.jpg",
                image_id(101)
            ),
            thumbnail: format!("{AD37_ORIGIN}/images/{}_thumbnail.jpg", image_id(101)),
            width: 3566,
            height: 2579,
        })
    );
    assert!(views[0].url.ends_with(&image_id(101)));
    // The search and the manifest: no `info.json`, no viewer request.
    let requests = fetch.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1], "/iiif/ark:/99937/aaaaaaaaaaaa/manifest.json");

    // A manifest listing fewer images than the row is a changed shape.
    let mut short = Fixtures::new(AD37_ONE, AD37_WINDOW);
    short.manifest = r#"{"items": []}"#;
    let target = resolve_in(
        &registry,
        "AD37 - Exampleville - (aucun) - N - 1850 - vue 2/315",
        &short,
    );
    assert!(matches!(target, Err(ResolveError::UnexpectedResponse(_))));
}

#[test]
fn several_registers_give_the_filtered_results_unless_the_citation_tells_them_apart() {
    // Births are not parish registers: the portal's act filter is a hint, and
    // the marriage register is out; the two others cover 1700.
    let (target, fetch) = embedded(
        "AD37 - Exampleville - (aucun) - B - 1700",
        AD37_SEVERAL,
        AD37_WINDOW,
    );
    assert_eq!(results_matches(&target), 2);
    let Ok(ArchiveTarget::Results { url, .. }) = &target else {
        unreachable!();
    };
    assert!(url.starts_with(&format!(
        "{AD37_ORIGIN}/search/results?formUuid=e9414896-40cc-4ec3-936c-8acdfdb11770&mode=list&"
    )));
    assert!(url.contains("Exampleville%20%28Indre-et-Loire%2C%20France%29"));
    assert!(!url.contains("resultsPerPage"));
    assert_eq!(fetch.requests().len(), 1);

    for (title, register) in [
        (
            "AD37 - Exampleville - Saint-Exemple - B - 1695",
            "dddddddddddd",
        ),
        (
            "AD37 - Exampleville - (aucun) - B - 1700 - 6NUM7/999/008",
            "eeeeeeeeeeee",
        ),
        (
            "AD37 - Exampleville - (aucun) - B - 1700 - vue 3/140",
            "eeeeeeeeeeee",
        ),
        (
            "AD37 - Exampleville - (aucun) - M - 1700 - 6NUM6/999/005",
            "ffffffffffff",
        ),
    ] {
        let (target, _) = embedded(title, AD37_SEVERAL, AD37_WINDOW);
        assert_eq!(register_of(&target.unwrap()), register, "{title}");
    }

    // A call number no row carries: not a guess among the others.
    let (target, _) = embedded(
        "AD37 - Exampleville - Saint-Exemple - B - 1695 - 6NUM7/999/099",
        AD37_SEVERAL,
        AD37_WINDOW,
    );
    assert_eq!(results_matches(&target), 3);
}

#[test]
fn no_register_gives_empty_results() {
    let (target, fetch) = embedded(
        "AD37 - Exampleville - (aucun) - B - 1500",
        AD37_NONE,
        AD37_WINDOW,
    );
    assert_eq!(results_matches(&target), 0);
    assert_eq!(fetch.requests().len(), 1);
}

#[test]
fn rows_beyond_the_page_make_the_search_page_the_answer() {
    let (open, close) = (
        AD37_ONE.find("<li ").unwrap(),
        AD37_ONE.find("</ol>").unwrap(),
    );
    let rows = AD37_ONE[open..close].repeat(RESULTS_PER_PAGE);
    let page = format!("{}{rows}{}", &AD37_ONE[..open], &AD37_ONE[close..])
        .replace("1 résultat", "503 résultats");
    let (target, fetch) = embedded(
        "AD37 - Exampleville - (aucun) - N - 1850",
        &page,
        AD37_WINDOW,
    );
    assert_eq!(results_matches(&target), 503);
    assert_eq!(fetch.requests().len(), 1);
}

#[test]
fn a_decennial_table_is_not_a_register_of_births() {
    // The portal's act filter returns the births' table beside the register,
    // and a table of several acts.
    let (target, _) = embedded(
        "AD37 - Exampleville - (aucun) - N - 1855",
        AD37_TABLES,
        AD37_WINDOW,
    );
    assert_eq!(register_of(&target.unwrap()), "aaaaaaaaaaaa");

    // A cited table is one of the two tables, told apart by its image count.
    let (target, _) = embedded(
        "AD37 - Exampleville - (aucun) - TD - 1855",
        AD37_TABLES,
        AD37_WINDOW,
    );
    assert_eq!(results_matches(&target), 2);
    let (target, _) = embedded(
        "AD37 - Exampleville - (aucun) - TD - 1855 - vue 3/30",
        AD37_TABLES,
        AD37_WINDOW,
    );
    assert_eq!(register_of(&target.unwrap()), "bbbbbbbbbbbb");
}

#[test]
fn a_view_beyond_the_register_opens_its_first_image() {
    // View 316 of a register the citation counts at 400 images, while the
    // portal lists 315.
    let (target, fetch) = embedded(
        "AD37 - Exampleville - (aucun) - N - 1850 - vue 316/400",
        AD37_ONE,
        AD37_WINDOW,
    );
    let first = format!("{AD37_ORIGIN}/ark:/99937/aaaaaaaaaaaa/{}", image_id(100));
    assert_eq!(
        target,
        Ok(ArchiveTarget::View {
            url: first,
            views: Vec::new(),
            view_count: Some(315),
            call_number: Some("6NUM8/999/050".to_owned()),
            attribution: Some(
                "Archives départementales d'Indre-et-Loire, 6NUM8/999/050, vue ".to_owned()
            ),
        })
    );
    assert_eq!(fetch.requests().len(), 1);
}

#[test]
fn a_changed_answer_is_reported_as_such() {
    let title = "AD37 - Exampleville - (aucun) - N - 1850 - vue 150/315";
    let renamed = AD37_ONE.replace("class=\"result\"", "class=\"total\"");
    let without_ark = AD37_ONE.replace("/ark:/", "/arc:/");
    let without_context = AD37_ONE.replace("class=\"context", "class=\"links");
    let one_of_two = AD37_ONE.replace("1 résultat", "2 résultats");
    for search in [
        "<html>maintenance</html>",
        "",
        renamed.as_str(),
        without_ark.as_str(),
        without_context.as_str(),
        one_of_two.as_str(),
    ] {
        let (target, _) = embedded(title, search, AD37_WINDOW);
        assert!(
            matches!(target, Err(ResolveError::UnexpectedResponse(_))),
            "{search:.60}: {target:?}"
        );
    }

    for window in ["{}", "[]", "not json", r#"{"149": {"uuid": "../x"}}"#] {
        let (target, _) = embedded(title, AD37_ONE, window);
        assert!(
            matches!(target, Err(ResolveError::UnexpectedResponse(_))),
            "{window}: {target:?}"
        );
    }

    let mut fetch = Fixtures::new(AD37_ONE, AD37_WINDOW);
    fetch.info = r#"{"width": 0, "height": 0}"#;
    let target = resolve_in(ArchiveRegistry::embedded(), title, &fetch);
    assert!(matches!(target, Err(ResolveError::UnexpectedResponse(_))));
}

#[test]
fn a_register_without_images_gives_the_results() {
    let page = AD37_ONE.replace("315 medias", "0 media");
    let (target, fetch) = embedded(
        "AD37 - Exampleville - (aucun) - N - 1850",
        &page,
        AD37_WINDOW,
    );
    assert_eq!(results_matches(&target), 1);
    assert_eq!(fetch.requests().len(), 1);
}

#[test]
fn calvados_rows_have_no_call_number_and_the_act_is_in_the_context() {
    // The portal returns the births' decennial table first: the row is a
    // table, whatever the act filter, and a combined parish register of
    // another period holds no births.
    let (target, fetch) = embedded(
        "AD14 - Exampleville - (aucun) - N - 1850",
        AD14_RESULTS,
        AD14_WINDOW,
    );
    let ArchiveTarget::View {
        url,
        views,
        call_number,
        attribution,
        view_count,
    } = target.unwrap()
    else {
        panic!("expected a view");
    };
    assert_eq!(
        url,
        format!(
            "https://archives.calvados.fr/ark:/99914/iiiiiiiiiiii/{}",
            image_id(800)
        )
    );
    assert!(views.is_empty());
    assert_eq!(
        (call_number, attribution, view_count),
        (None, None, Some(518))
    );
    let search = &fetch.requests()[0];
    assert!(search.contains(
        "controlledAccessGeographicName%5B%5D=Exampleville%20%28Calvados%2C%20France%29&"
    ));
    assert!(search.contains("controlledAccessPhysicalCharacteristic%5B%5D=Naissances&"));
    assert!(search.contains("date=1850&"));
    assert_eq!(fetch.requests().len(), 1);

    // The viewer gives the cited image's own ARK; the portal displays it
    // (no `info.json` for a `portal` archive).
    let (target, fetch) = embedded(
        "AD14 - Exampleville - (aucun) - N - 1850 - vue 21/518",
        AD14_RESULTS,
        AD14_WINDOW,
    );
    let ArchiveTarget::View { views, .. } = target.unwrap() else {
        panic!("expected a view");
    };
    assert_eq!(
        views[0].ark.as_deref(),
        Some(
            format!(
                "https://archives.calvados.fr/ark:/99914/iiiiiiiiiiii/{}",
                image_id(820)
            )
            .as_str()
        )
    );
    assert_eq!(views[0].image, None);
    assert_eq!(
        fetch.requests()[1],
        "/visualizer/api?arkName=iiiiiiiiiiii&start=20&end=20&group=0"
    );
    assert_eq!(fetch.requests().len(), 2);

    // A cited call number cannot select among rows that show none, and is
    // kept as the register's own.
    let (target, _) = embedded(
        "AD14 - Exampleville - (aucun) - N - 1850 - 9 E 99 / 1",
        AD14_RESULTS,
        AD14_WINDOW,
    );
    let Ok(ArchiveTarget::View { call_number, .. }) = target else {
        panic!("expected a view, got {target:?}");
    };
    assert_eq!(call_number.as_deref(), Some("9 E 99 / 1"));

    // The parish is read from the context (`Paroisse Saint-Exemple`).
    let (target, _) = embedded(
        "AD14 - Exampleville - Saint-Exemple - B - 1650",
        AD14_RESULTS,
        AD14_WINDOW,
    );
    assert_eq!(register_of(&target.unwrap()), "jjjjjjjjjjjj");
}

#[test]
fn marne_writes_articles_after_the_name_and_sends_both_label_forms() {
    let (target, fetch) = embedded(
        "AD51 - Le Bourg - (aucun) - B - 1720",
        AD51_RESULTS,
        AD51_WINDOW,
    );
    assert_eq!(register_of(&target.unwrap()), "kkkkkkkkkkkk");
    let search = &fetch.requests()[0];
    for expected in [
        "0-controlledAccessGeographicName%5B%5D=Bourg%20%28Le%29%20%28Marne%2C%20France%29&",
        "0-controlledAccessGeographicName%5B%5D=Bourg%20%28Le%29%20%28Marne%20%3B%20ancienne%20commune%29&",
        "4-controlledAccessPhysicalCharacteristic%5B%5D=bapt%C3%AAmes%20-%20naissances&",
        "3-date=1720&",
    ] {
        assert!(search.contains(expected), "{expected} in {search}");
    }

    // Both registers cover 1730: the call number separates them.
    let (target, _) = embedded(
        "AD51 - Le Bourg - (aucun) - BMS - 1730",
        AD51_RESULTS,
        AD51_WINDOW,
    );
    assert_eq!(results_matches(&target), 2);
    let (target, _) = embedded(
        "AD51 - Le Bourg - (aucun) - BMS - 1730 - 2 E 999/1",
        AD51_RESULTS,
        AD51_WINDOW,
    );
    assert_eq!(register_of(&target.unwrap()), "llllllllllll");

    // Births and baptisms share a label: the row's title and period decide.
    let (target, fetch) = embedded(
        "AD51 - EXAMPLEVILLE - (aucun) - N - 1850 - vue 2/60",
        AD51_ONE,
        AD51_WINDOW,
    );
    let ArchiveTarget::View {
        views,
        attribution,
        call_number,
        ..
    } = target.unwrap()
    else {
        panic!("expected a view");
    };
    // `portal` display: the ARK only, no image of OxidGene's own.
    assert_eq!(views[0].image, None);
    assert!(views[0].url.ends_with(&image_id(1301)));
    assert_eq!(attribution, None);
    assert_eq!(call_number.as_deref(), Some("2 E 999/71"));
    assert_eq!(fetch.requests().len(), 2);
}

#[test]
fn reads_the_endpoint_and_the_search_page_of_each_collection() {
    let registry = ArchiveRegistry::embedded();
    let ad37 = &registry.archive("AD37").unwrap().collections[0];
    let endpoint = Mnesys.endpoint(ad37).unwrap();
    assert_eq!(endpoint.origin, AD37_ORIGIN);
    assert_eq!(
        endpoint.start,
        format!("{AD37_ORIGIN}/search/form/e9414896-40cc-4ec3-936c-8acdfdb11770")
    );
    assert_eq!(endpoint.access, Access::Any);
    assert!(endpoint.other_origins.is_empty());

    let citation = registry
        .parse("AD37 - Exampleville - (aucun) - D - 1910")
        .unwrap();
    let url = Mnesys.results_url(ad37, &citation).unwrap();
    assert_eq!(
        url,
        format!(
            "{AD37_ORIGIN}/search/results?formUuid=e9414896-40cc-4ec3-936c-8acdfdb11770&mode=list&sort=date_asc&0-controlledAccessGeographicName%5B%5D=Exampleville%20%28Indre-et-Loire%2C%20France%29&2-controlledAccessPhysicalCharacteristic%5B%5D=D%C3%A9c%C3%A8s&4-date=1910"
        )
    );
    // Without a year the search is not narrowed by one.
    let citation = registry
        .parse("AD37 - Exampleville - (aucun) - TD")
        .unwrap();
    let url = Mnesys.results_url(ad37, &citation).unwrap();
    assert!(url.contains("Table%20d%C3%A9cennale"));
    assert!(!url.contains("4-date"));
}

/// A registry whose archive `code` has its first collection's portal
/// settings changed.
fn with_portal(code: &str, change: impl FnOnce(&mut serde_json::Value)) -> ArchiveRegistry {
    let registry = ArchiveRegistry::embedded();
    let mut archive = registry.archive(code).unwrap().clone();
    change(&mut archive.collections[0].portal);
    let document = serde_json::to_string(&archive).unwrap();
    ArchiveRegistry::new(&[("fr", document.as_str())], platform::builtin()).unwrap()
}

fn collection_with(change: impl FnOnce(&mut serde_json::Value)) -> Collection {
    let mut collection = ArchiveRegistry::embedded()
        .archive("AD51")
        .unwrap()
        .collections[0]
        .clone();
    change(&mut collection.portal);
    collection
}

#[test]
fn validates_its_settings_against_the_collection() {
    assert_eq!(Mnesys.validate(&collection_with(|_| {})), Ok(()));
    type Change = fn(&mut serde_json::Value);
    let cases: [(&str, Change); 11] = [
        ("unknown field", |p| p["parish"] = "x".into()),
        ("https origin", |p| {
            p["origin"] = "http://archives.example.org".into()
        }),
        ("must be a UUID", |p| p["form"] = "ecf01748".into()),
        ("input names", |p| p["fields"]["act"] = "act name".into()),
        ("one {locality}", |p| {
            p["locality_label"] = serde_json::json!(["{locality} {locality}"])
        }),
        ("one {locality}", |p| {
            p["locality_label"] = serde_json::json!([])
        }),
        ("empty label", |p| p["acts"]["B"] = serde_json::json!([])),
        ("no label for `D`", |p| {
            p["acts"].as_object_mut().unwrap().remove("D");
        }),
        ("not an act code", |p| {
            p["acts"]["X"] = serde_json::json!(["x"])
        }),
        ("image_source", |p| {
            p.as_object_mut().unwrap().remove("image_source");
        }),
        ("unknown variant", |p| p["image_source"] = "iiif".into()),
    ];
    for (expected, change) in cases {
        let error = Mnesys
            .validate(&collection_with(change))
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{expected}: {error}");
    }
}

#[test]
fn writes_the_locality_in_the_portal_style() {
    let settings = Settings::read(
        &ArchiveRegistry::embedded()
            .archive("AD51")
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
            &format!("AD51 - {written} - (aucun) - N - 1850"),
            &crate::CitationGrammar::default(),
        )
        .unwrap();
        assert_eq!(settings.locality(&citation), expected, "{written}");
    }
}

#[test]
fn every_catalogued_archive_has_one_collection_for_each_act() {
    let registry = ArchiveRegistry::embedded();
    for code in ["AD37", "AD14", "AD51"] {
        let archive = registry.archive(code).unwrap();
        assert_eq!(archive.collections.len(), 1, "{code}");
        for act in ["B", "M", "S", "N", "D", "TD"] {
            assert!(
                archive.holds(&Act::from_code(act).unwrap()),
                "{code} holds {act}"
            );
        }
    }
}
