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
    /// The search form, for a lookup of its locality list.
    form: &'static str,
    /// The viewer's answer for a window starting at the first image, and for
    /// any other.
    first: &'static str,
    window: &'static str,
    /// The viewer's state for a register's first image.
    state: &'static str,
    info: &'static str,
    manifest: &'static str,
    requests: Mutex<Vec<String>>,
}

impl Fixtures {
    fn new(search: &str, window: &'static str) -> Self {
        Self {
            search: search.to_owned(),
            form: "",
            first: AD37_FIRST,
            window,
            state: "",
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
            } else if url.starts_with("/search/form/") {
                self.form
            } else if url.starts_with("/visualizer/api?") && url.contains("&uuid=") {
                self.state
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
            renumbering: None,
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
            renumbering: None,
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
        ..
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
fn the_registers_come_first_and_hold_every_act() {
    let registry = ArchiveRegistry::embedded();
    for code in ["AD37", "AD14", "AD51"] {
        let registers = &registry.archive(code).unwrap().collections[0];
        for act in ["B", "M", "S", "N", "D", "TD"] {
            assert!(
                registers.holds(&Act::from_code(act).unwrap()),
                "{code} holds {act}"
            );
        }
    }
}

const AD19_FORM: &str = include_str!("../../../fixtures/mnesys/ad19-military-form.html");
const AD19_MILITARY: &str = include_str!("../../../fixtures/mnesys/ad19-military.html");
const AD19_WINDOW: &str = include_str!("../../../fixtures/mnesys/ad19-visualizer.json");
const AD25_REGISTERS: &str = include_str!("../../../fixtures/mnesys/ad25-registers.html");
const AD25_WINDOW: &str = include_str!("../../../fixtures/mnesys/ad25-visualizer.json");
const AD27_FORMER: &str = include_str!("../../../fixtures/mnesys/ad27-former.html");
const AD27_WINDOW: &str = include_str!("../../../fixtures/mnesys/ad27-visualizer.json");
const AD58_MILITARY: &str = include_str!("../../../fixtures/mnesys/ad58-military.html");
const AD58_WINDOW: &str = include_str!("../../../fixtures/mnesys/ad58-visualizer.json");
const AD59_FORM: &str = include_str!("../../../fixtures/mnesys/ad59-form.html");
const AD59_REGISTERS: &str = include_str!("../../../fixtures/mnesys/ad59-registers.html");
const AD59_WINDOW: &str = include_str!("../../../fixtures/mnesys/ad59-visualizer.json");
const AD68_REGISTERS: &str = include_str!("../../../fixtures/mnesys/ad68-registers.html");
const AD69_MILITARY: &str = include_str!("../../../fixtures/mnesys/ad69-military.html");
const AD69_STATE: &str = include_str!("../../../fixtures/mnesys/ad69-viewer-state.json");
const AD69_WINDOW: &str = include_str!("../../../fixtures/mnesys/ad69-visualizer.json");
const AD90_SUCCESSION: &str = include_str!("../../../fixtures/mnesys/ad90-succession.html");
const AD90_WINDOW: &str = include_str!("../../../fixtures/mnesys/ad90-visualizer.json");
const AD14_CENSUS: &str = include_str!("../../../fixtures/mnesys/ad14-census.html");
const AD14_STATE: &str = include_str!("../../../fixtures/mnesys/ad14-viewer-state.json");

/// A citation of the embedded catalogue resolved over `fetch`, with its
/// requests.
fn resolved(title: &str, fetch: &Fixtures) -> (Result<ArchiveTarget, ResolveError>, Vec<String>) {
    let target = resolve_in(ArchiveRegistry::embedded(), title, fetch);
    (target, fetch.requests())
}

#[test]
fn a_military_register_is_chosen_by_bureau_class_and_matricule() {
    let mut fetch = Fixtures::new(AD19_MILITARY, AD19_WINDOW);
    fetch.form = AD19_FORM;
    let (target, requests) = resolved(
        "AD19 - Exampleville - Registres matricules - 1890 - matricule 640 - vue 100/557",
        &fetch,
    );
    let target = target.unwrap();
    assert_eq!(register_of(&target), "aaaaaaaaaa19");
    let ArchiveTarget::View { views, .. } = &target else {
        panic!("expected a view");
    };
    assert_eq!(views[0].view, 100);
    // The form's list, the search, the cited image.
    assert_eq!(requests.len(), 3);
    assert_eq!(
        requests[0],
        "/search/form/85c4d2cc-6374-489a-8be0-e79d0e0755b6"
    );
    for expected in [
        "0-controlledAccessGeographicName%5B%5D=Exampleville%20%28Corr%C3%A8ze%2C%20France%29&",
        "3-controlledAccessPhysicalCharacteristic%5B%5D=registre%20matricule&",
        "1-date%5B%5D=1890&",
    ] {
        assert!(
            requests[1].contains(expected),
            "{expected} in {}",
            requests[1]
        );
    }
    assert!(!requests[1].contains("640"));
    assert_eq!(
        requests[2],
        "/visualizer/api?arkName=aaaaaaaaaa19&start=99&end=99&group=0"
    );

    // Without the matricule or the image count, the four volumes.
    let mut fetch = Fixtures::new(AD19_MILITARY, AD19_WINDOW);
    fetch.form = AD19_FORM;
    let (target, _) = resolved(
        "AD19 - Exampleville - Registres matricules - 1890 - vue 100",
        &fetch,
    );
    assert_eq!(results_matches(&target), 4);

    // A bureau the form's list does not name has no register: no search.
    let mut fetch = Fixtures::new(AD19_MILITARY, AD19_WINDOW);
    fetch.form = AD19_FORM;
    let (target, requests) = resolved("AD19 - Elsewhere - Registres matricules - 1890", &fetch);
    assert_eq!(results_matches(&target), 0);
    assert_eq!(requests.len(), 1);
}

#[test]
fn a_former_commune_is_searched_and_read_under_its_own_label() {
    let fetch = Fixtures::new(AD27_FORMER, AD27_WINDOW);
    let (target, requests) = resolved(
        "AD27 - Exampleville - Saint-Exemple - BMS - 1646-1792 - 9 Mi 9999 - vue 220g/585",
        &fetch,
    );
    let target = target.unwrap();
    assert_eq!(register_of(&target), "aaaaaaaaaa27");
    let ArchiveTarget::View { views, .. } = &target else {
        panic!("expected a view");
    };
    assert_eq!(views[0].view, 220);
    // The label of a commune and of a former one, in one search.
    for label in [
        "Exampleville%20%28Eure%2C%20France%29&",
        "Exampleville%20%28ancienne%20commune%29%20%28Eure%2C%20France%29&",
    ] {
        assert!(requests[0].contains(label), "{label} in {}", requests[0]);
    }
    assert!(requests[0].contains("&1-date=1646&"));

    // A label cut short past the name still shows the locality.
    let fetch = Fixtures::new(AD27_FORMER, AD27_WINDOW);
    let (target, _) = resolved(
        "AD27 - Saint-Exemple-la-Longue - (aucun) - BMS - 1650 - 9 Mi 9998",
        &fetch,
    );
    assert_eq!(register_of(&target.unwrap()), "bbbbbbbbbb27");

    // Neither the commune a former one joined nor a name the cut label
    // only starts is the locality.
    for title in [
        "AD27 - Sampleton - (aucun) - BMS - 1650",
        "AD27 - Saint-Exemple - (aucun) - BMS - 1650",
    ] {
        let fetch = Fixtures::new(AD27_FORMER, AD27_WINDOW);
        let (target, _) = resolved(title, &fetch);
        assert_eq!(results_matches(&target), 0, "{title}");
    }
}

#[test]
fn a_shortened_label_shows_the_locality_only_past_its_name() {
    let registry = ArchiveRegistry::embedded();
    let shows = |code: &str, locality: &str, entry: &str| {
        let settings = Settings::read(&registry.archive(code).unwrap().collections[0]).unwrap();
        let citation = registry
            .parse(&format!("{code} - {locality} - (aucun) - B - 1700"))
            .unwrap();
        settings.shows_label(entry, &citation)
    };
    for (locality, entry, expected) in [
        (
            "Exampleville",
            "Exampleville (ancienne commune) (Eure, France)/Sampleton...",
            true,
        ),
        (
            "Exampleville",
            "Exampleville (ancienne commune) (Eure,...",
            true,
        ),
        ("Exampleville", "Exampleville (Eure,\u{2026}", true),
        ("Exampleville", "Exampleville...", false),
        (
            "Exampleville",
            "Exampleville-la-Haute (ancienne commune) (Eure,...",
            false,
        ),
        (
            "Sampleton",
            "Exampleville (ancienne commune) (Eure, France)/Sampleton...",
            false,
        ),
    ] {
        assert_eq!(shows("AD27", locality, entry), expected, "{entry}");
    }
    // A bare pattern: the name alone, before the current commune.
    assert!(shows("AD25", "Exampleville", "Exampleville/Sampleton"));
    assert!(!shows(
        "AD25",
        "Exampleville",
        "Exampleville la Haute/Sampleton"
    ));
    assert!(!shows("AD25", "Exampleville", "Exampleville la..."));
}

#[test]
fn acts_written_as_letters_in_the_title_select_the_register() {
    let fetch = Fixtures::new(AD25_REGISTERS, AD25_WINDOW);
    let (target, requests) = resolved(
        "AD25 - Exampleville - (aucun) - M - 1800 - vue 5/80",
        &fetch,
    );
    assert_eq!(register_of(&target.unwrap()), "bbbbbbbbbb25");
    // The form has no act or year input: the locality alone is sent.
    assert!(requests[0].contains("0-controlledAccessGeographicName%5B%5D=Exampleville&"));
    assert!(!requests[0].contains("-date"));
    assert!(!requests[0].contains("1800"));

    // `BMS-NMD` holds marriages, and only it covers 1760.
    let fetch = Fixtures::new(AD25_REGISTERS, AD25_WINDOW);
    let (target, _) = resolved("AD25 - Exampleville - (aucun) - M - 1760", &fetch);
    assert_eq!(register_of(&target.unwrap()), "dddddddddd25");
    // A birth of 1745: no register of births covers it, so the act leaves
    // the births of 1793 and the combined register, which no period tells
    // apart from it: the results.
    let fetch = Fixtures::new(AD25_REGISTERS, AD25_WINDOW);
    let (target, _) = resolved("AD25 - Exampleville - (aucun) - N - 1745", &fetch);
    assert_eq!(results_matches(&target), 2);
}

#[test]
fn a_form_without_a_locality_keeps_the_rows_naming_the_bureau() {
    let fetch = Fixtures::new(AD58_MILITARY, AD58_WINDOW);
    let (target, requests) = resolved(
        "AD58 - Exampleville - Registres matricules - 1890 - matricule 640 - vue 10/829",
        &fetch,
    );
    assert_eq!(register_of(&target.unwrap()), "cccccccccc58");
    assert!(requests[0].contains("&0-date%5B%5D=1890&"));
    assert!(!requests[0].contains("Exampleville"));

    // Both bureaux have a volume spanning matricule 400: without a bureau,
    // or with one no row names, both stay.
    for title in [
        "AD58 - Registres matricules - 1890 - matricule 400",
        "AD58 - Elsewhere - Registres matricules - 1890 - matricule 400",
    ] {
        let fetch = Fixtures::new(AD58_MILITARY, AD58_WINDOW);
        let (target, _) = resolved(title, &fetch);
        assert_eq!(results_matches(&target), 2, "{title}");
    }
}

#[test]
fn a_lookup_sends_the_labels_the_form_lists() {
    let mut fetch = Fixtures::new(AD59_REGISTERS, AD59_WINDOW);
    fetch.form = AD59_FORM;
    let (target, requests) = resolved(
        "AD59 - Exampleville - (aucun) - M - 1750 - vue 3/381",
        &fetch,
    );
    assert_eq!(register_of(&target.unwrap()), "aaaaaaaaaa59");
    assert_eq!(
        requests[0],
        "/search/form/dc4e871d-0b62-41fb-9921-5ded573781b8"
    );
    for expected in [
        "0-controlledAccessGeographicName%5B%5D=EXAMPLEVILLE&",
        "1-controlledAccessPhysicalCharacteristic%5B%5D=Mariages&",
        "2-date=1750&",
    ] {
        assert!(
            requests[1].contains(expected),
            "{expected} in {}",
            requests[1]
        );
    }
    assert_eq!(requests.len(), 3);

    // Case, accents and punctuation aside, the label names the locality.
    let registry = ArchiveRegistry::embedded();
    let settings = Settings::read(&registry.archive("AD59").unwrap().collections[0]).unwrap();
    let options = page::options(AD59_FORM, "0-controlledAccessGeographicName[]").unwrap();
    for (locality, expected) in [
        ("Saint-Éxemple", vec!["SAINT-EXEMPLE"]),
        ("saint exemple les bois", vec!["SAINT-EXEMPLE-LES-BOIS"]),
        ("Sampleton", vec![]),
    ] {
        let citation = registry
            .parse(&format!("AD59 - {locality} - (aucun) - N - 1850"))
            .unwrap();
        assert_eq!(
            settings.listed_labels(&options, &citation).sent,
            expected,
            "{locality}"
        );
    }
    // A commune the list names otherwise: the cited name without its
    // qualifier rather than a name extending it, which the rows then show
    // for selection to weigh; nothing else of the citation's name.
    for (locality, sent, renamed) in [
        (
            "Exampleville-sur-Mer",
            vec!["EXAMPLEVILLE"],
            Some("EXAMPLEVILLE"),
        ),
        (
            "Saint-Exemple-sous-Bois",
            vec!["SAINT-EXEMPLE"],
            Some("SAINT-EXEMPLE"),
        ),
        ("Saint-Exemple", vec!["SAINT-EXEMPLE"], None),
        ("Sampleton-sur-Mer", vec![], None),
    ] {
        let citation = registry
            .parse(&format!("AD59 - {locality} - (aucun) - N - 1850"))
            .unwrap();
        let labels = settings.listed_labels(&options, &citation);
        assert_eq!(labels.sent, sent, "{locality}");
        assert_eq!(labels.renamed.as_deref(), renamed, "{locality}");
    }

    // A form without the list, or an anti-bot page in its place.
    for (form, challenged) in [
        ("<html><form></form></html>", false),
        (
            "<html><title>Just a moment...</title><script>window._cf_chl_opt={}</script></html>",
            true,
        ),
    ] {
        let mut fetch = Fixtures::new(AD59_REGISTERS, AD59_WINDOW);
        fetch.form = form;
        let (target, _) = resolved("AD59 - Exampleville - (aucun) - M - 1750", &fetch);
        assert_eq!(
            matches!(target, Err(ResolveError::Challenged)),
            challenged,
            "{target:?}"
        );
        assert!(target.is_err());
    }
}

#[test]
fn a_challenge_in_place_of_the_results_is_not_drift() {
    let challenge =
        "<html><title>Just a moment...</title><script>window._cf_chl_opt={}</script></html>";
    let (target, _) = embedded(
        "AD37 - Exampleville - (aucun) - N - 1850",
        challenge,
        AD37_WINDOW,
    );
    assert_eq!(target, Err(ResolveError::Challenged));
}

#[test]
fn the_call_number_may_be_a_context_entry() {
    let registry = ArchiveRegistry::embedded();
    let settings = Settings::read(&registry.archive("AD68").unwrap().collections[0]).unwrap();
    let citation = registry
        .parse("AD68 - Exampleville - (aucun) - N - 1850")
        .unwrap();
    let rows = page::results(AD68_REGISTERS, RESULTS_PER_PAGE)
        .unwrap()
        .rows;
    let call_numbers: Vec<_> = rows
        .iter()
        .map(|row| settings.row_call_number(row, None))
        .collect();
    assert_eq!(
        call_numbers,
        [Some("9Mi9/9".to_owned()), Some("9E/9/1".to_owned())]
    );
    assert!(
        settings
            .candidates(rows, &citation)
            .iter()
            .all(|row| row.locality.as_deref() == Some("Exampleville"))
    );
}

#[test]
fn a_register_in_several_lots_is_counted_by_the_viewer() {
    let mut fetch = Fixtures::new(AD69_MILITARY, AD69_WINDOW);
    fetch.state = AD69_STATE;
    let (target, requests) = resolved(
        "AD69 - Exampleville Central - Registres matricules - 1900 - matricule 600 - vue 446",
        &fetch,
    );
    let ArchiveTarget::View {
        views,
        view_count,
        call_number,
        ..
    } = target.unwrap()
    else {
        panic!("expected a view");
    };
    assert_eq!(view_count, Some(891));
    // The cell `444, 9RP9992`: the call number, not the internal number.
    assert_eq!(call_number.as_deref(), Some("9RP9992"));
    assert!(views[0].url.ends_with(&image_id(7445)));
    assert_eq!(
        requests[1],
        format!(
            "/visualizer/api?arkName=bbbbbbbbbb69&uuid={}",
            image_id(7000)
        )
    );
    assert_eq!(requests.len(), 3);

    // Either call number of the cell selects the register.
    let mut fetch = Fixtures::new(AD69_MILITARY, AD69_WINDOW);
    fetch.state = AD69_STATE;
    let (target, _) = resolved(
        "AD69 - Exampleville Central - Registres matricules - 1900 - 9RP9993",
        &fetch,
    );
    assert_eq!(register_of(&target.unwrap()), "cccccccccc69");

    let mut fetch = Fixtures::new(AD69_MILITARY, AD69_WINDOW);
    fetch.state = r#"{"counts": {}}"#;
    let (target, _) = resolved(
        "AD69 - Exampleville Central - Registres matricules - 1900 - matricule 600",
        &fetch,
    );
    assert!(matches!(target, Err(ResolveError::UnexpectedResponse(_))));
}

#[test]
fn a_table_listed_without_images_is_skipped_or_answers_the_results() {
    let fetch = Fixtures::new(AD90_SUCCESSION, AD90_WINDOW);
    let (target, requests) = resolved(
        "AD90 - Exampleville - Tables des successions et absences - 1875 - vue 10/191",
        &fetch,
    );
    assert_eq!(register_of(&target.unwrap()), "bbbbbbbbbb90");
    for expected in [
        "0-title%5B%5D=Bureau%20de%20Exampleville&",
        "1-controlledAccessPhysicalCharacteristic%5B%5D=Table%20de%20successions&",
        "2-date_begin=1875&2-date_end=1875",
    ] {
        assert!(
            requests[0].contains(expected),
            "{expected} in {}",
            requests[0]
        );
    }
    // The missing table, cited by its call number: listed, not openable.
    let fetch = Fixtures::new(AD90_SUCCESSION, AD90_WINDOW);
    let (target, requests) = resolved(
        "AD90 - Exampleville - Tables des successions et absences - 1860 - 3 Q 99/3",
        &fetch,
    );
    assert_eq!(results_matches(&target), 1);
    assert_eq!(requests.len(), 1);
}

#[test]
fn a_row_without_an_image_count_is_counted_by_the_viewer() {
    let mut fetch = Fixtures::new(AD14_CENSUS, AD14_WINDOW);
    fetch.state = AD14_STATE;
    let (target, requests) = resolved("AD14 - Exampleville - Recensement - 1876", &fetch);
    let ArchiveTarget::View {
        view_count, url, ..
    } = target.unwrap()
    else {
        panic!("expected a view");
    };
    assert_eq!(view_count, Some(40));
    assert!(url.ends_with(&image_id(1400)));
    assert_eq!(requests.len(), 2);
}

#[test]
fn builds_the_search_of_each_form_shape() {
    let registry = ArchiveRegistry::embedded();
    let url = |title: &str| {
        let citation = registry.parse(title).unwrap();
        let (_, collections) = registry.candidates(&citation).unwrap();
        Mnesys.results_url(collections[0], &citation).unwrap()
    };
    // A period sent as the cited year at both ends.
    assert!(
        url("AD26 - Exampleville - (aucun) - N - 1850")
            .contains("&1-date_begin=1850&1-date_end=1850")
    );
    // A class sent as the form's label, in a select.
    assert!(
        url("AD51 - Exampleville - Registres matricules - 1890")
            .ends_with("&2-controlledAccessPhysicalCharacteristic%5B%5D=Registre%20matricule&0-title%5B%5D=Classe%201890")
    );
    // No locality input: the cited bureau is never sent.
    let military = url("AD55 - Exampleville - Registres matricules - 1890");
    assert!(!military.contains("Exampleville"));
    assert!(
        military.ends_with(
            "&1-controlledAccessPhysicalCharacteristic%5B%5D=Registre&0-date%5B%5D=1890"
        )
    );
    // A lookup's patterns with a `*` are not spelled offline.
    let census = url("AD80 - Exampleville - Recensement - 1901");
    assert!(census.contains("Exampleville%20%28Somme%2C%20France%29"));
    assert!(!census.contains("ancienne"));
}

#[test]
fn a_pattern_names_the_locality_of_a_label() {
    for (pattern, label, expected) in [
        (
            "{locality} (Somme, France)",
            "Exampleville (Somme, France)",
            vec!["Exampleville"],
        ),
        (
            "{locality} (ancienne commune*, Somme, France)",
            "Exampleville (ancienne commune av. 1790, Somme, France)",
            vec!["Exampleville"],
        ),
        (
            "Subdivision de {locality} (*)",
            "Subdivision de Bourg (Le) (1867-1901)",
            vec!["Bourg (Le)", "Bourg"],
        ),
        (
            "{locality} Commune",
            "EXAMPLEVILLE Commune",
            vec!["EXAMPLEVILLE"],
        ),
        ("{locality} Commune", "EXAMPLEVILLE Canton", vec![]),
        ("Bureau de {locality}", "Bureau d'Exampleville", vec![]),
    ] {
        assert_eq!(named_by(pattern, label), expected, "{pattern} / {label}");
    }
}

#[test]
fn validates_the_shapes_of_its_forms() {
    type Change = fn(&mut serde_json::Value);
    let cases: [(&str, Change); 5] = [
        ("period_begin and period_end go together", |p| {
            p["fields"]["period_begin"] = "1-date_begin".into()
        }),
        ("year_label needs a year input", |p| {
            p["year_label"] = "Classe {year}".into();
            p["fields"].as_object_mut().unwrap().remove("year");
        }),
        ("need a locality input", |p| {
            p["fields"].as_object_mut().unwrap().remove("locality");
        }),
        ("needs locality_lookup", |p| {
            p["locality_label"] = serde_json::json!(["{locality} (*)"])
        }),
        ("after {locality}", |p| {
            p["locality_lookup"] = true.into();
            p["locality_label"] = serde_json::json!(["* {locality}"])
        }),
    ];
    for (expected, change) in cases {
        let error = Mnesys
            .validate(&collection_with(change))
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{expected}: {error}");
    }
    // Without an act input, labels are needed only to tell the tables of a
    // collection that also holds registers.
    assert_eq!(
        Mnesys.validate(&collection_with(|p| {
            p["fields"].as_object_mut().unwrap().remove("act");
            p["acts"] = serde_json::json!({"TD": ["tables décennales"]});
        })),
        Ok(())
    );
    let error = Mnesys
        .validate(&collection_with(|p| {
            p["fields"].as_object_mut().unwrap().remove("act");
            p["acts"] = serde_json::json!({});
        }))
        .unwrap_err()
        .to_string();
    assert!(error.contains("no label for `TD`"), "{error}");
}

/// The live probe over the same fixtures (Archive Portals §9.1).
#[test]
fn the_live_probe_reads_each_form_shape() {
    use crate::live::Probe;
    let registry = ArchiveRegistry::embedded();
    let collection =
        |code: &str, index: usize| registry.archive(code).unwrap().collections[index].clone();

    // A lookup form lists its labels in capitals: the first is cited as
    // listed, and the probe searches it through the form's list again.
    let ad59 = collection("AD59", 0);
    let mut fetch = Fixtures::new(AD59_REGISTERS, AD59_WINDOW);
    fetch.form = AD59_FORM;
    assert_eq!(
        block_on(Mnesys.search_page(&ad59, &fetch)),
        Ok("EXAMPLEVILLE".to_owned())
    );
    let act = Act::from_code("B").unwrap();
    let registers = block_on(Mnesys.registers(&ad59, "EXAMPLEVILLE", &act, &fetch)).unwrap();
    assert_eq!(registers.len(), 3);
    assert_eq!(registers[0].locality, "EXAMPLEVILLE");
    assert_eq!(registers[0].call_number.as_deref(), Some("9 Mi 999 R 001"));

    // A form without a locality input: its series are cited without one.
    let ad58 = collection("AD58", 2);
    let fetch = Fixtures::new(AD58_MILITARY, AD58_WINDOW);
    let mut form = Fixtures::new(AD58_MILITARY, AD58_WINDOW);
    form.form = r#"<div class="enhanced-select multiselect" data-name="0-date" data-options="[&quot;1890&quot;]"></div>"#;
    assert_eq!(
        block_on(Mnesys.search_page(&ad58, &form)),
        Ok(String::new())
    );
    let registers =
        block_on(Mnesys.registers(&ad58, "", &Act::from_code("RM").unwrap(), &fetch)).unwrap();
    assert_eq!(
        registers
            .iter()
            .map(|register| register.numbers)
            .collect::<Vec<_>>(),
        [None, Some((1, 500)), Some((501, 1000)), Some((1, 498))]
    );

    // Rows in several lots count no images: the viewer's state does.
    let ad69 = collection("AD69", 2);
    let mut fetch = Fixtures::new(AD69_MILITARY, AD69_WINDOW);
    fetch.state = AD69_STATE;
    let registers = block_on(Mnesys.registers(
        &ad69,
        "Exampleville Central",
        &Act::from_code("RM").unwrap(),
        &fetch,
    ))
    .unwrap();
    assert_eq!(registers[1].images, None);
    assert_eq!(
        block_on(Mnesys.images(&ad69, &registers[1], &fetch)),
        Ok(Some(891))
    );
}
