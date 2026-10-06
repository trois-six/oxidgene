//! The live checks' steps over the anonymized Arkothèque fixtures, served by
//! a scripted transport: the discovery, the choice of a register, the
//! verdicts, and the outcomes of a portal that drifted or could not be
//! reached.

use std::sync::Mutex;

use serde_json::Value;

use super::*;
use crate::PortalEndpoint;
use crate::tests::block_on;

const SEARCH_PAGE: &str = include_str!("../../fixtures/arkotheque/ad44-search-page.html");
const ENGINE: &str = include_str!("../../fixtures/arkotheque/ad44-engine.json");
const ONE: &str = include_str!("../../fixtures/arkotheque/ad44-one.json");
const VIEWER: &str = include_str!("../../fixtures/arkotheque/ad44-viewer.json");

const AD44_PORTAL: &str = "https://archives-numerisees.loire-atlantique.fr";

/// The viewer answer of a register of `count` images: the fixture's first
/// image repeated.
fn viewer_of(count: usize) -> String {
    let mut answer: Value = serde_json::from_str(VIEWER).unwrap();
    let first = answer["medias"][0]["sources"][0].clone();
    let sources: Vec<Value> = (0..count)
        .map(|index| {
            let mut source = first.clone();
            let src = source["src"].as_str().unwrap();
            let base = src.rsplit_once('/').unwrap().0.to_owned();
            source["src"] = Value::from(format!("{base}/{index}"));
            source
        })
        .collect();
    answer["medias"][0]["sources"] = Value::from(sources);
    answer.to_string()
}

/// Serves the fixtures by request, or fails as `failure` says.
struct Scripted {
    page: String,
    engine: String,
    search: &'static str,
    viewer: String,
    failure: Option<FetchError>,
    requests: Mutex<Vec<String>>,
}

impl Scripted {
    fn new() -> Self {
        Self {
            page: SEARCH_PAGE.to_owned(),
            engine: ENGINE.to_owned(),
            search: ONE,
            viewer: viewer_of(46),
            failure: None,
            requests: Mutex::new(Vec::new()),
        }
    }

    fn answer(&self, url: &str) -> Result<String, FetchError> {
        if let Some(failure) = &self.failure {
            return Err(failure.clone());
        }
        let body = if url == "/robots.txt" {
            "User-agent: *\nDisallow: /private/\n"
        } else if url == "/chercher/etat-civil-et-registres-paroissiaux" {
            &self.page
        } else if url.starts_with("/_recherche-api/moteur?") && url.contains("ficheFocus") {
            self.search
        } else if url.starts_with("/_recherche-api/moteur?") {
            &self.engine
        } else if url.starts_with("/_recherche-api/visionneuse-infos/") {
            &self.viewer
        } else {
            return Err(FetchError::Status(404));
        };
        Ok(body.to_owned())
    }
}

impl PortalTransport for Scripted {
    fn is_browser(&self) -> bool {
        false
    }

    fn connect<'a>(
        &'a self,
        _endpoint: &'a PortalEndpoint,
    ) -> BoxFuture<'a, Result<Box<dyn PortalFetch + 'a>, FetchError>> {
        Box::pin(async move { Ok(Box::new(ScriptedFetch(self)) as Box<dyn PortalFetch + 'a>) })
    }
}

struct ScriptedFetch<'s>(&'s Scripted);

impl PortalFetch for ScriptedFetch<'_> {
    fn request<'a>(
        &'a self,
        request: &'a PortalRequest,
    ) -> BoxFuture<'a, Result<String, FetchError>> {
        Box::pin(async move {
            self.0.requests.lock().unwrap().push(request.url.clone());
            self.0.answer(&request.url)
        })
    }
}

fn ad44() -> &'static Archive {
    ArchiveRegistry::embedded().archive("AD44").unwrap()
}

fn check(transport: &Scripted) -> CollectionReport {
    block_on(check_collection(
        ArchiveRegistry::embedded(),
        ad44(),
        0,
        transport,
    ))
}

#[test]
fn builds_its_citation_from_the_portal_and_resolves_it() {
    let transport = Scripted::new();
    let report = check(&transport);
    assert_eq!(report.outcome, Outcome::Ok, "{:?}", report.failure);
    assert_eq!(report.transport, Transport::Native);
    assert_eq!(
        report.citation.as_deref(),
        Some("AD44 - Exampleville - (aucun) - B - 1658 - E dépôt 99 - vue 23/46")
    );
    let opening = report.opening.expect("an opening");
    assert_eq!(opening.platform, "arkotheque");
    assert_eq!(opening.view, 23);
    assert_eq!(opening.view_count, Some(46));
    assert_eq!(opening.image, None);
    assert!(opening.url.starts_with(&format!(
        "{AD44_PORTAL}/chercher/etat-civil-et-registres-paroissiaux?detail=arko_fiche_0000000000a01#/_recherche-api/visionneuse-infos/"
    )));
    assert!(opening.url.ends_with("/image/900001/22"));
}

#[test]
fn discovers_with_a_handful_of_sequential_requests() {
    let transport = Scripted::new();
    let report = check(&transport);
    // The page, the engine, the discovery search and the chosen register's
    // viewer, which counts its images, then a search and the viewer per
    // resolution, with and without the call number.
    let requests = transport.requests.lock().unwrap().clone();
    assert_eq!(report.requests, 9);
    assert_eq!(requests.len(), 9);
    // The portal's pace first, then its search page and engine.
    assert_eq!(requests[0], "/robots.txt");
    let requests = &requests[1..];
    assert_eq!(requests[0], "/chercher/etat-civil-et-registres-paroissiaux");
    assert!(requests[1].contains("contenuIds"));
    assert!(!requests[1].contains("ficheFocus"));
    // The discovery searches the first listed locality and the first act,
    // without a year.
    assert!(requests[2].contains("Exampleville"));
    assert!(requests[2].contains("Bapt%C3%AAmes%5B%5Barko_fiche_6a6b3d70f2db3"));
    assert!(!requests[2].contains("slider"));
    assert!(requests[3].starts_with("/_recherche-api/visionneuse-infos/"));
    assert!(requests[4].contains("1658%7C1658"));
}

#[test]
fn reports_what_the_portal_lacks_as_drift() {
    let mut transport = Scripted::new();
    transport.page = SEARCH_PAGE.replace("data-contenu=\"1289790\"", "data-contenu=\"1\"");
    let report = check(&transport);
    assert_eq!(report.outcome, Outcome::Drift);
    let failure = report.failure.unwrap();
    assert_eq!(failure.step, Step::SearchPage);
    assert_eq!(failure.received, "missing: content 1289790");
    assert_eq!(report.opening, None);

    let mut transport = Scripted::new();
    transport.engine = ENGINE
        .replace("Décès[[arko_fiche_6a6b3d7107318]]", "Décès[[arko_fiche_0]]")
        .replace("arko_default_6a6b4d95b8dc8", "arko_default_0");
    let failure = check(&transport).failure.unwrap();
    assert_eq!(failure.step, Step::SearchPage);
    assert_eq!(failure.received, "missing: display mode, act value of D");

    let mut transport = Scripted::new();
    transport.search = include_str!("../../fixtures/arkotheque/ad44-none.json");
    let failure = check(&transport).failure.unwrap();
    assert_eq!(
        (failure.step, failure.received.as_str()),
        (Step::Discovery, "no register")
    );

    // The register has fewer images than its row announces, as when the
    // reading room shows more: the citation counts those of the viewer.
    let mut transport = Scripted::new();
    transport.viewer = viewer_of(3);
    let report = check(&transport);
    assert_eq!(report.outcome, Outcome::Ok, "{:?}", report.failure);
    assert!(report.citation.unwrap().ends_with("vue 2/3"));
}

#[test]
fn reports_an_unanswered_portal_as_unreachable() {
    for (error, outcome) in [
        (FetchError::Timeout, Outcome::Unreachable),
        (FetchError::Network, Outcome::Unreachable),
        (FetchError::Status(502), Outcome::Unreachable),
        (FetchError::Status(404), Outcome::Drift),
        (FetchError::Challenged, Outcome::Challenged),
    ] {
        let mut transport = Scripted::new();
        transport.failure = Some(error.clone());
        let report = check(&transport);
        assert_eq!(report.outcome, outcome, "{error:?}");
        assert_eq!(report.failure.unwrap().step, Step::SearchPage);
        // `robots.txt`, then the search page.
        assert_eq!(report.requests, 2);
    }
}

#[test]
fn a_challenge_page_is_not_a_drift() {
    // An anti-bot page answered as a success, in place of the search page
    // and in place of the engine's answer.
    let challenge = "<html><script>window.location.href='/redirect_X/'</script></html>";
    let mut transport = Scripted::new();
    transport.page = challenge.to_owned();
    let failure = check(&transport).failure.unwrap();
    assert_eq!(
        (failure.step, failure.outcome),
        (Step::SearchPage, Outcome::Challenged)
    );

    let mut transport = Scripted::new();
    transport.engine = challenge.to_owned();
    let report = check(&transport);
    assert_eq!(report.outcome, Outcome::Challenged);
    assert_eq!(report.requests, 3);
}

#[test]
fn reads_the_pace_a_portal_asks_of_robots() {
    let robots = "User-agent: Googlebot\nCrawl-delay: 1\n\nUser-agent: OtherBot\nUser-agent: *\nDisallow: /archive/resultats/*?*\nCrawl-delay: 5 # seconds\n";
    assert_eq!(crawl_delay(robots), Duration::from_secs(5));
    assert_eq!(
        crawl_delay("User-agent: Googlebot\nCrawl-delay: 10\n"),
        Duration::ZERO
    );
    assert_eq!(
        crawl_delay("User-agent: *\nCrawl-delay: 3600\n"),
        MAX_CRAWL_DELAY
    );
    assert_eq!(crawl_delay("<html>Not found</html>"), Duration::ZERO);
    assert_eq!(
        crawl_delay("User-agent: *\nCrawl-delay: 0.5\n"),
        Duration::from_millis(500)
    );
}

fn register(call_number: &str, period: &str, images: u16) -> Register {
    Register {
        locality: "Exampleville".to_owned(),
        call_number: Some(call_number.to_owned()),
        period: Some(period.to_owned()),
        images: Some(images),
        address: Some("/viewer".to_owned()),
        numbers: None,
    }
}

#[test]
fn chooses_a_register_its_citation_tells_apart() {
    let collection = &ad44().collections[0];
    // Two registers share a call number, a period and an image count: the
    // third is the first a citation can single out.
    let registers = [
        register("9 E 1", "1700-1710", 40),
        register("9E1", "1700", 40),
        register("9 E 1", "1711-1720", 40),
    ];
    let chosen = choose(&registers, "Exampleville", collection).unwrap();
    assert_eq!(chosen.period.as_deref(), Some("1711-1720"));

    // Beyond the collection's period, without images or without a year,
    // a register cannot be cited.
    let uncitable = [
        register("9 E 2", "1930", 40),
        register("9 E 3", "1700", 0),
        register("9 E 4", "s.d.", 40),
    ];
    let failure = choose(&uncitable, "Exampleville", collection).unwrap_err();
    assert_eq!(failure.step, Step::Discovery);
    assert_eq!(
        failure.received,
        "3 registers: 3 with a call number, 2 with images, 3 with an image address, 2 with a year"
    );

    // The searched locality first, then any other the text match returned.
    let mut elsewhere = register("9 E 5", "1700", 40);
    elsewhere.locality = "Exampleville-sur-Mer".to_owned();
    let both = [elsewhere.clone(), register("9 E 6", "1700", 40)];
    let chosen = choose(&both, "exampleville", collection).unwrap();
    assert_eq!(chosen.call_number.as_deref(), Some("9 E 6"));
    let chosen = choose(std::slice::from_ref(&elsewhere), "Exampleville", collection).unwrap();
    assert_eq!(chosen, &elsewhere);
}

#[test]
fn shared_call_numbers_are_told_apart_by_period_and_images() {
    // Four registers of one call number, as on the Sarthe portal
    // (`ad72-several.json`).
    let registers: Vec<Register> = [
        (
            "BMS 1595-1692 (consulter le détail dans la première vue)",
            120,
        ),
        ("BMS 1692-1729", 89),
        ("BMS 1730-1764", 115),
        ("BMS 1765-1792", 113),
    ]
    .into_iter()
    .map(|(period, images)| register("1MI 999 R1", period, images))
    .collect();
    let collection = &ArchiveRegistry::embedded()
        .archive("AD72")
        .unwrap()
        .collections[0];
    let chosen = choose(&registers, "Exampleville", collection).unwrap();
    assert_eq!(chosen, &registers[0]);
    let citation = citation_of(ad44(), &Act::from_code("B").unwrap(), chosen, 1);
    assert_eq!(citation.year, Some(1595));
    assert_eq!(citation.views[0].view, 60);
}

#[test]
fn volumes_of_a_military_register_are_told_apart_by_their_matricules() {
    // Two volumes of one class, with no call number and the same image
    // count: only their matricules differ.
    let volume = |first, last| Register {
        call_number: None,
        numbers: Some((first, last)),
        ..register("", "1870", 400)
    };
    let registers = [volume(1, 500), volume(501, 1000)];
    let collection = &ad44().collections[0];
    let chosen = choose(&registers, "Exampleville", collection).unwrap();
    assert_eq!(chosen, &registers[0]);
    assert_eq!(
        registers
            .iter()
            .filter(|other| registers[1].is_confusable_with(other))
            .count(),
        1
    );

    // The citation names the series by its code and the volume by its
    // first matricule, and reads back as it was built.
    let series = Act::Series(crate::Series::MilitaryRegister);
    let citation = citation_of(ad44(), &series, &registers[1], 1);
    assert_eq!(citation.number, Some(501));
    let title = title_of(&citation);
    assert_eq!(
        title,
        "AD44 - Exampleville - (aucun) - RM - 1870 - n° 501 - vue 200/400"
    );
    assert_eq!(ArchiveRegistry::embedded().parse(&title), Some(citation));
}

#[test]
fn an_archive_takes_its_worst_outcome() {
    let report = |outcome| CollectionReport {
        collection: "registers".to_owned(),
        index: 0,
        platform: "arkotheque".to_owned(),
        transport: Transport::Native,
        outcome,
        failure: None,
        requests: 0,
        locality: None,
        citation: None,
        opening: None,
    };
    let archive = ad44();
    assert_eq!(ArchiveReport::new(archive, Vec::new()).outcome, Outcome::Ok);
    for (outcomes, worst) in [
        (vec![Outcome::Ok, Outcome::Challenged], Outcome::Challenged),
        (
            vec![Outcome::Challenged, Outcome::Unreachable],
            Outcome::Unreachable,
        ),
        (vec![Outcome::Drift, Outcome::Unreachable], Outcome::Drift),
    ] {
        let collections = outcomes.into_iter().map(report).collect();
        assert_eq!(ArchiveReport::new(archive, collections).outcome, worst);
    }
    let json = serde_json::to_value(report(Outcome::Unreachable)).unwrap();
    assert_eq!(json["outcome"], "unreachable");
    assert_eq!(json["transport"], "native");
}

#[test]
fn selects_the_checked_archives() {
    let registry = ArchiveRegistry::embedded();
    let all = archives(registry, None).unwrap();
    assert!(all.iter().any(|archive| archive.id == "fr-ad44"));
    assert!(all.iter().all(|archive| archive.live_check));
    assert_eq!(
        archives(registry, Some("fr-ad72")).unwrap()[0].id,
        "fr-ad72"
    );
    assert!(archives(registry, Some("fr-unknown")).is_err());

    let ad72 = registry.archive("AD72").unwrap();
    assert!(needs_browser(registry, &ad72.collections[0]));
    assert!(!needs_browser(registry, &ad44().collections[0]));
}

#[test]
fn an_uncounted_register_without_an_address_per_view_is_cited_at_its_first_view() {
    let registry = ArchiveRegistry::embedded();
    let corsica = registry.archive("AD2A").unwrap();
    let collection = &corsica.collections[0];
    let thot = probe("thot").unwrap();
    assert!(!thot.addresses_views(collection));
    assert!(thot.addresses_views(&registry.archive("AD35").unwrap().collections[0]));

    // Counting the register would open its viewer: it is cited at its
    // first view, uncounted.
    let register = Register {
        locality: "Exampleville".to_owned(),
        call_number: Some("99 NUM 1".to_owned()),
        period: Some("1865 - 1873".to_owned()),
        images: None,
        address: Some("/Internet_THOT/FrmLotDocFrame.asp?idlot=1".to_owned()),
        numbers: None,
    };
    let citation = citation_of(corsica, &collection.acts[0], &register, 1);
    assert_eq!(citation.views[0].view, 1);
    assert_eq!(citation.view_count, None);
    let url = "https://archives.isula.corsica/Internet_THOT/FrmLotDocFrame.asp?idlot=1";
    let target = ArchiveTarget::View {
        url: url.to_owned(),
        views: Vec::new(),
        view_count: None,
        call_number: Some("99 NUM 1".to_owned()),
        attribution: None,
    };
    let opening = check_view(corsica, collection, &citation, &target, false).unwrap();
    assert_eq!((opening.url.as_str(), opening.view), (url, 1));
    assert_eq!(opening.view_count, None);
    // The same target from a portal with an address per view is drift.
    assert!(check_view(corsica, collection, &citation, &target, true).is_err());
}

#[test]
fn an_uncounted_register_with_an_address_per_view_is_cited_at_its_second_view() {
    let registry = ArchiveRegistry::embedded();
    let savoie = registry.archive("AD73").unwrap();
    let collection = &savoie.collections[0];
    let probe = probe("mnesys-inao").unwrap();
    assert!(probe.addresses_views(collection) && !probe.counts_images(collection));
    let register = Register {
        locality: "Exampleville".to_owned(),
        call_number: Some("4E 9001".to_owned()),
        period: Some("1850-1860".to_owned()),
        images: None,
        address: Some("/?id=detail&page_ref=1".to_owned()),
        numbers: None,
    };
    let citation = citation_of(savoie, &collection.acts[0], &register, UNCOUNTED_VIEW);
    assert_eq!(citation.views[0].view, 2);
    assert_eq!(citation.view_count, None);
    let url = "https://viewer.example.org/ark:/99999/a1?vue=2";
    let target = ArchiveTarget::View {
        url: url.to_owned(),
        views: vec![crate::ArchiveView {
            view: 2,
            url: url.to_owned(),
            ark: None,
            image: None,
        }],
        view_count: None,
        call_number: Some("4E 9001".to_owned()),
        attribution: None,
    };
    let opening = check_view(savoie, collection, &citation, &target, true).unwrap();
    assert_eq!(
        (opening.url.as_str(), opening.view, opening.view_count),
        (url, 2, None)
    );
}

#[test]
fn every_adapter_has_a_probe() {
    for platform in crate::platform::builtin() {
        assert!(
            probe(platform.id()).is_some(),
            "{} has no probe",
            platform.id()
        );
    }
}

#[test]
fn a_viewer_without_an_address_per_view_is_checked_on_its_first_view() {
    let registry = ArchiveRegistry::embedded();
    let archive = registry.archive("AD09").unwrap();
    let collection = &archive.collections[0];
    let citation = registry
        .parse("AD09 - Exampleville - (aucun) - N - 1850 - 9 NUM 4 E 1 - vue 20/40")
        .unwrap();
    let url = "https://archives.example.org/viewer/1/2/900/1400".to_owned();
    let register = |views: Vec<crate::ArchiveView>| ArchiveTarget::View {
        url: url.clone(),
        views,
        view_count: None,
        call_number: Some("9NUM4E1".to_owned()),
        attribution: None,
    };
    assert_eq!(
        check_view(archive, collection, &citation, &register(Vec::new()), false),
        Ok(Opening {
            platform: "gaia".to_owned(),
            url: url.clone(),
            view: 1,
            view_count: Some(40),
            image: None,
        })
    );
    // A platform with an address per view must address the cited one.
    assert!(check_view(archive, collection, &citation, &register(Vec::new()), true).is_err());
    let view = crate::ArchiveView {
        view: 20,
        url: url.clone(),
        ark: None,
        image: None,
    };
    assert!(check_view(archive, collection, &citation, &register(vec![view]), false).is_err());
}
