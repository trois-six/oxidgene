//! The adapter over anonymized answers shaped like those of the Ain,
//! Ardèche and Haute-Garonne portals (`fixtures/ligeo/`, written by its
//! `generate.py`), with the catalogue's own settings for the three archives.

use std::pin::pin;
use std::sync::Mutex;
use std::task::{Context, Poll, Waker};

use super::*;
use crate::platform::Access;
use crate::transport::{FetchError, PortalRequest};
use crate::{ArchiveRegistry, platform};

const AIN_SEVERAL: &str = include_str!("../../../fixtures/ligeo/ain-several.html");
const AIN_ONE: &str = include_str!("../../../fixtures/ligeo/ain-one.html");
const AIN_NONE: &str = include_str!("../../../fixtures/ligeo/ain-none.html");
const AIN_PAGINATED: &str = include_str!("../../../fixtures/ligeo/ain-paginated.html");
const AIN_TABLES: &str = include_str!("../../../fixtures/ligeo/ain-tables.html");
const ARDECHE_CIVIL: &str = include_str!("../../../fixtures/ligeo/ardeche-civil.html");
const ARDECHE_PARISH: &str = include_str!("../../../fixtures/ligeo/ardeche-parish.html");
const HG_SEVERAL: &str = include_str!("../../../fixtures/ligeo/hg-several.html");
const MANIFEST: &str = include_str!("../../../fixtures/ligeo/manifest.json");
const INFO_WIDE: &str = include_str!("../../../fixtures/ligeo/info-wide.json");
const INFO_TALL: &str = include_str!("../../../fixtures/ligeo/info-tall.json");
const INFO_SMALL: &str = include_str!("../../../fixtures/ligeo/info-small.json");

const AIN: &str = "https://www.archives.ain.fr";
const ARDECHE: &str = "https://archives.ardeche.fr";

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

/// Answers a search, a manifest and an image service's `info.json` with
/// their fixtures, and records the requests.
struct Fixtures {
    search: &'static str,
    manifest: &'static str,
    info: &'static str,
    requests: Mutex<Vec<String>>,
}

impl Fixtures {
    fn new(search: &'static str) -> Self {
        Self {
            search,
            manifest: MANIFEST,
            info: INFO_TALL,
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
            let body = if url.contains("/resultats/") || url.contains("/fonds/") {
                self.search
            } else if url.starts_with("/ark:/") && url.ends_with("/manifest") {
                self.manifest
            } else if url.starts_with("/iiif/") && url.ends_with("/info.json") {
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
    block_on(Ligeo.resolve(archive, collections[0], &citation, fetch))
}

fn embedded(title: &str, search: &'static str) -> (Result<ArchiveTarget, ResolveError>, Fixtures) {
    let fetch = Fixtures::new(search);
    let target = resolve(ArchiveRegistry::embedded(), title, &fetch);
    (target, fetch)
}

/// The register of a `View` target, as its address ends the viewer path.
fn viewed(target: &ArchiveTarget) -> &str {
    let url = target.url();
    let start = url.find("/ark:/99999/").expect("a register ARK") + "/ark:/99999/".len();
    &url[start..start + url[start..].find('/').expect("a viewer path")]
}

fn matches(target: &Result<ArchiveTarget, ResolveError>) -> Option<usize> {
    match target {
        Ok(ArchiveTarget::Results { matches, .. }) => *matches,
        other => panic!("expected results, got {other:?}"),
    }
}

#[test]
fn one_register_opens_on_the_cited_view_with_its_images() {
    let (target, fetch) = embedded(
        "AD01 - Exampleville - (aucun) - N - 1880 - 9 E 99 - vue 5/120",
        AIN_ONE,
    );
    let base = format!("{AIN}/ark:/99999/vtaexample0011");
    let image_base = format!("{AIN}/iiif/EC/EX_0011/EX_0011_005.jpg");
    assert_eq!(
        target,
        Ok(ArchiveTarget::View {
            url: format!("{base}/daogrp/0/5"),
            views: vec![ArchiveView {
                view: 5,
                url: format!("{base}/daogrp/0/5"),
                ark: Some(format!("{base}/img:EX_0011_005")),
                // Sized by its service, not by the manifest's canvas.
                image: Some(ArchiveImage {
                    picture: format!("{image_base}/full/,2048/0/default.jpg"),
                    thumbnail: format!("{image_base}/full/150,/0/default.jpg"),
                    width: 1780,
                    height: 2704,
                }),
            }],
            // The manifest's count, not the row's.
            view_count: Some(6),
            // The portal shows an internal reference, not a call number: the
            // cited one stands.
            call_number: Some("9 E 99".to_owned()),
            attribution: Some("Archives départementales de l'Ain, 9 E 99, vue 5".to_owned()),
        })
    );

    // The search, the register's manifest, the view's image service:
    // nothing more.
    assert_eq!(
        fetch.requests(),
        [
            "/archive/resultats/etatcivil/n:88?RECH_commune=Exampleville&RECH_acte%5B%5D=N\
             &RECH_unitdate_debut=1880&RECH_unitdate_fin=1880&type=etatcivil",
            "/ark:/99999/vtaexample0011/manifest",
            "/iiif/EC/EX_0011/EX_0011_005.jpg/info.json",
        ]
    );
    // The portal learns the locality, act and year, never the rest.
    for private in ["9%20E", "vue", "120"] {
        assert!(!fetch.requests()[0].contains(private), "{private}");
    }
    // The manifest's own host names the portal's internal side.
    let serialized = serde_json::to_string(target.as_ref().unwrap()).unwrap();
    assert!(!serialized.contains("invalid"), "{serialized}");
}

#[test]
fn images_are_sized_by_their_service_on_the_level_1_service() {
    // Wider than tall, taller than wide, and below the bound, whatever the
    // canvases declare.
    for (info, picture, size) in [
        (INFO_WIDE, "full/2048,", (2704, 1780)),
        (INFO_TALL, "full/,2048", (1780, 2704)),
        (INFO_SMALL, "full/full", (1000, 800)),
    ] {
        let mut fetch = Fixtures::new(AIN_ONE);
        fetch.info = info;
        let target = resolve(
            ArchiveRegistry::embedded(),
            "AD01 - Exampleville - (aucun) - N - 1880 - vue 1/120",
            &fetch,
        );
        let Ok(ArchiveTarget::View { views, .. }) = target else {
            panic!("expected a view, got {target:?}");
        };
        let image = views[0].image.as_ref().unwrap();
        assert!(
            image
                .picture
                .ends_with(&format!("/EX_0011_001.jpg/{picture}/0/default.jpg")),
            "{}",
            image.picture
        );
        assert_eq!((image.width, image.height), size);
    }

    let (target, fetch) = embedded(
        "AD01 - Exampleville - (aucun) - N - 1880 - vue 1-6/120",
        AIN_ONE,
    );
    let Ok(ArchiveTarget::View {
        views, attribution, ..
    }) = target
    else {
        panic!("expected a view, got {target:?}");
    };
    assert_eq!(views.len(), 6);
    // One service per view.
    assert_eq!(fetch.requests().len(), 2 + 6);
    assert_eq!(
        attribution.as_deref(),
        Some("Archives départementales de l'Ain, , vue 1-6")
    );
    // A service whose `info.json` lacks a size is a changed answer.
    let mut fetch = Fixtures::new(AIN_ONE);
    fetch.info = "<html>maintenance</html>";
    let target = resolve(
        ArchiveRegistry::embedded(),
        "AD01 - Exampleville - (aucun) - N - 1880 - vue 1/120",
        &fetch,
    );
    assert!(matches!(target, Err(ResolveError::UnexpectedResponse(_))));
}

#[test]
fn several_registers_give_the_filtered_results_unless_the_citation_tells_them_apart() {
    // The substring match also returned `Exampleville-lès-Bois`: not a
    // candidate.
    let (target, fetch) = embedded("AD01 - Exampleville - (aucun) - B - 1700", AIN_SEVERAL);
    assert_eq!(matches(&target), Some(3));
    assert_eq!(fetch.requests().len(), 1);
    let Ok(ArchiveTarget::Results { url, .. }) = &target else {
        unreachable!("results");
    };
    assert_eq!(
        url,
        &format!(
            "{AIN}/archive/resultats/etatcivil/n:88?RECH_commune=Exampleville&RECH_acte%5B%5D=B\
             &RECH_unitdate_debut=1700&RECH_unitdate_fin=1700&type=etatcivil"
        )
    );

    // The image count tells the registers apart, and the rows' acts: `T, B`
    // is a register of baptisms, `T, S` of burials.
    let (target, _) = embedded(
        "AD01 - Exampleville - (aucun) - B - 1700 - vue 3/77",
        AIN_SEVERAL,
    );
    assert_eq!(viewed(&target.unwrap()), "vtaexample0001");
    let (target, _) = embedded("AD01 - Exampleville - (aucun) - S - 1700", AIN_SEVERAL);
    assert_eq!(matches(&target), Some(3));
    let (target, _) = embedded(
        "AD01 - Exampleville - (aucun) - S - 1700 - vue 3/294",
        AIN_SEVERAL,
    );
    assert_eq!(viewed(&target.unwrap()), "vtaexample0004");

    // A combined act repeats the checkbox once per kind and keeps the
    // registers holding all of them.
    let (target, fetch) = embedded("AD01 - Exampleville - (aucun) - BMS - 1700", AIN_SEVERAL);
    assert_eq!(matches(&target), Some(2));
    assert!(fetch.requests()[0].contains("RECH_acte%5B%5D=B&RECH_acte%5B%5D=M&RECH_acte%5B%5D=S&"));
}

#[test]
fn decennial_tables_are_searched_and_read_as_tables() {
    let (target, fetch) = embedded("AD01 - Exampleville - (aucun) - TD - 1880", AIN_TABLES);
    assert_eq!(viewed(&target.unwrap()), "vtaexample0021");
    assert!(fetch.requests()[0].contains("RECH_acte%5B%5D=tables&"));
}

#[test]
fn no_register_gives_empty_results() {
    let (target, fetch) = embedded("AD01 - Exampleville - (aucun) - B - 1500", AIN_NONE);
    assert_eq!(matches(&target), Some(0));
    assert_eq!(fetch.requests().len(), 1);
}

#[test]
fn a_paginated_answer_is_not_guessed_from_its_first_page() {
    // The first page alone would select a register; the count says more.
    let (target, _) = embedded(
        "AD01 - Exampleville - (aucun) - B - 1700 - vue 3/77",
        AIN_PAGINATED,
    );
    assert_eq!(matches(&target), Some(59));
}

#[test]
fn a_changed_answer_is_reported_as_such() {
    let title = "AD01 - Exampleville - (aucun) - N - 1880";
    let renamed: &'static str = AIN_ONE.replace("Type d’acte", "Genre").leak();
    for search in ["<html>maintenance</html>", "", renamed] {
        let (target, _) = embedded(title, search);
        assert!(
            matches!(target, Err(ResolveError::UnexpectedResponse(_))),
            "{search}: {target:?}"
        );
    }

    let mut fetch = Fixtures::new(AIN_ONE);
    for manifest in [
        "{}",
        r#"{"sequences": [{"canvases": []}]}"#,
        r#"{"sequences": [{"canvases": [{"width": 0, "height": 10}]}]}"#,
    ] {
        fetch.manifest = manifest;
        let target = resolve(ArchiveRegistry::embedded(), title, &fetch);
        assert!(
            matches!(target, Err(ResolveError::UnexpectedResponse(_))),
            "{manifest}: {target:?}"
        );
    }
    // An image service outside the portal's image path is not followed.
    fetch.manifest = Box::leak(
        MANIFEST
            .replace("/iiif/EC/", "/elsewhere/EC/")
            .into_boxed_str(),
    );
    let target = resolve(
        ArchiveRegistry::embedded(),
        "AD01 - Exampleville - (aucun) - N - 1880 - vue 1/6",
        &fetch,
    );
    assert!(matches!(target, Err(ResolveError::UnexpectedResponse(_))));
}

#[test]
fn an_anti_bot_challenge_is_not_drift() {
    let anubis = r#"<!doctype html><html><head><title>Making sure you're not a bot!</title>
        <script id="anubis_version" type="application/json">"1.0"</script></head></html>"#;
    let f5 = r#"<html><head><script>window["bobcmn"]="1011...";</script>
        <script src="/TSPD/0842?type=25"></script></head></html>"#;
    let denied = "<html><body><h1>Request Rejected</h1></body></html>";
    for answer in [anubis, f5, denied] {
        let (target, _) = embedded("AD01 - Exampleville - (aucun) - N - 1880", answer);
        assert_eq!(target, Err(ResolveError::Challenged), "{answer}");
    }
    assert_eq!(ResolveError::Challenged.code(), "challenged");
}

#[test]
fn civil_status_filters_by_document_type_and_act_and_reads_a_shared_call_number() {
    let (target, fetch) = embedded("AD07 - Exampleville - (aucun) - M - 1880", ARDECHE_CIVIL);
    // The locality cell carries the thesaurus qualifier, which is dropped.
    let target = target.unwrap();
    assert_eq!(
        target,
        ArchiveTarget::View {
            url: format!("{ARDECHE}/ark:/99999/vtaexample0033/daogrp/0/1"),
            views: Vec::new(),
            view_count: Some(6),
            call_number: Some("NC 99001".to_owned()),
            attribution: Some("Archives départementales de l'Ardèche, NC 99001, vue ".to_owned()),
        }
    );
    assert_eq!(
        fetch.requests()[0],
        "/archive/resultats/etatcivil/n:96?RECH_comm=Exampleville&RECH_acte2=%2Aariage%2A\
         &RECH_doc=EC&RECH_unitdate_debut=1880&RECH_unitdate_fin=1880&type=etatcivil"
    );

    // Two births of 1880: the shared call number only breaks the tie.
    let (target, _) = embedded("AD07 - Exampleville - (aucun) - N - 1880", ARDECHE_CIVIL);
    assert_eq!(matches(&target), Some(2));
    for (cited, register) in [
        ("NC 99002", "vtaexample0032"),
        ("NC99001", "vtaexample0031"),
    ] {
        let (target, _) = embedded(
            &format!("AD07 - Exampleville - (aucun) - N - 1880 - {cited}"),
            ARDECHE_CIVIL,
        );
        assert_eq!(viewed(&target.unwrap()), register, "{cited}");
    }
    // A call number no row carries does not discard them either.
    let (target, _) = embedded(
        "AD07 - Exampleville - (aucun) - N - 1880 - NC 99999",
        ARDECHE_CIVIL,
    );
    assert_eq!(matches(&target), Some(2));
    // A call number does not decide alone: the act does.
    let (target, _) = embedded(
        "AD07 - Exampleville - (aucun) - D - 1880 - NC 99001",
        ARDECHE_CIVIL,
    );
    assert_eq!(viewed(&target.unwrap()), "vtaexample0034");
}

#[test]
fn decennial_tables_of_the_civil_status_are_a_document_type() {
    let (target, fetch) = embedded("AD07 - Exampleville - (aucun) - TD - 1880", ARDECHE_CIVIL);
    // One table per act kind, which the cited act does not tell apart.
    assert_eq!(matches(&target), Some(3));
    let search = &fetch.requests()[0];
    assert!(search.contains("RECH_doc=TD&"), "{search}");
    assert!(!search.contains("RECH_acte2"), "{search}");
}

#[test]
fn parish_registers_open_in_the_group_the_row_link_names() {
    let (target, fetch) = embedded(
        "AD07 - Exampleville - (aucun) - B - 1700 - vue 3/392",
        ARDECHE_PARISH,
    );
    let register = format!("{ARDECHE}/ark:/99999/vtaexample0041");
    let Ok(ArchiveTarget::View { url, views, .. }) = &target else {
        panic!("expected a view, got {target:?}");
    };
    assert_eq!(url, &format!("{register}/daoloc/0/3"));
    assert_eq!(views[0].url, format!("{register}/daoloc/0/3"));
    assert_eq!(
        fetch.requests()[0],
        "/archive/resultats/paroissiaux/n:164?RECH_commune=Exampleville\
         &RECH_unitdate_debut=1700&RECH_unitdate_fin=1700&type=paroissiaux"
    );
    // The register is named `BMS` by the link's title, not by a column.
    let (target, _) = embedded("AD07 - Exampleville - (aucun) - S - 1790", ARDECHE_PARISH);
    assert_eq!(viewed(&target.unwrap()), "vtaexample0041");
}

#[test]
fn a_view_beyond_the_register_opens_its_first_image() {
    let (target, _) = embedded(
        "AD07 - Exampleville - (aucun) - B - 1700 - vue 7/392",
        ARDECHE_PARISH,
    );
    assert_eq!(
        target,
        Ok(ArchiveTarget::View {
            url: format!("{ARDECHE}/ark:/99999/vtaexample0041/daoloc/0/1"),
            views: Vec::new(),
            view_count: Some(6),
            call_number: None,
            attribution: Some("Archives départementales de l'Ardèche, , vue ".to_owned()),
        })
    );
}

#[test]
fn a_title_only_table_gives_everything_from_the_title() {
    let (target, fetch) = embedded("AD31 - Exampleville - Saint-Exemple - B - 1760", HG_SEVERAL);
    // The collection's registers (greffe) and the communal register of the
    // same parish both cover 1760.
    assert_eq!(matches(&target), Some(2));
    assert_eq!(
        fetch.requests()[0],
        "/archive/resultats/etatcivil/n:97?RECH_commune=Exampleville\
         &RECH_unitdate_debut=1760&RECH_unitdate_fin=1760&type=etatcivil"
    );

    let (target, _) = embedded(
        "AD31 - Exampleville - Saint-Exemple - B - 1760 - 1GG8",
        HG_SEVERAL,
    );
    assert_eq!(viewed(&target.unwrap()), "vtaexample0053");
    let (target, _) = embedded("AD31 - Exampleville - Saint-Autre - B - 1760", HG_SEVERAL);
    assert_eq!(viewed(&target.unwrap()), "vtaexample0052");
    // The table of 1850 is the decennial one, not the annual one.
    let (target, _) = embedded("AD31 - Exampleville - (aucun) - TD - 1850", HG_SEVERAL);
    assert_eq!(viewed(&target.unwrap()), "vtaexample0055");
}

#[test]
fn reads_each_field_of_a_title_row() {
    let columns = Columns {
        title: Some(Names::One("Intitulé".to_owned())),
        period: Some(Names::One("Date".to_owned())),
        ..Columns::default()
    };
    let found = page::results(HG_SEVERAL, &columns).unwrap();
    assert_eq!(found.total, Some(6));
    let fields: Vec<_> = found
        .rows
        .iter()
        .map(|row| {
            (
                row.locality.as_deref().unwrap(),
                row.parish.as_deref(),
                row.call_number.as_deref(),
                row.act.as_deref(),
                row.period.as_deref().unwrap(),
                row.images.unwrap(),
            )
        })
        .collect();
    assert_eq!(
        fields,
        [
            (
                "Exampleville",
                Some("Saint-Exemple"),
                None,
                Some("BMS"),
                "1756-1775",
                323
            ),
            (
                "Exampleville",
                Some("Saint-Autre"),
                None,
                Some("BMS"),
                "1737-1790",
                291
            ),
            (
                "Exampleville",
                Some("Saint-Exemple"),
                Some("1 GG 8"),
                Some("BMS"),
                "1751-1762",
                150
            ),
            (
                "Exampleville",
                Some("Saint-Exemple"),
                Some("1 GG 12"),
                Some("TA"),
                "1674-1802",
                20
            ),
            (
                "Exampleville",
                None,
                Some("4 E 1"),
                Some("TD"),
                "1802-1863",
                60
            ),
            (
                "Exampleville-lès-Bois",
                Some("Saint-Test"),
                Some("2 GG 1"),
                Some("BMS"),
                "1700-1750",
                80
            ),
        ]
    );
    // The notice link of the action cell is not the viewer link.
    assert!(found.rows.iter().all(|row| row.payload.register.is_some()));
}

#[test]
fn reads_the_acts_a_text_names() {
    use crate::platform::select::act_code;
    for (text, codes, expected) in [
        ("B, M, S", true, Some("BMS")),
        ("N", true, Some("N")),
        ("T, B", true, Some("B")),
        ("Naissances.", true, Some("N")),
        ("Tables décennales des décès.", true, Some("TD")),
        ("tables décennales", true, Some("TD")),
        ("BMS", true, Some("BMS")),
        ("Di", true, None),
        ("autres", true, None),
        ("Publications de mariages", true, Some("M")),
        // A title's initial is not an act, and an unrelated word is none.
        ("Exampleville (MQ S. 1762)", false, None),
        ("Saint-Jean-Baptiste", false, None),
        (
            "registre paroissial : baptêmes, sépultures",
            false,
            Some("BS"),
        ),
    ] {
        assert_eq!(act_code(text, codes).as_deref(), expected, "{text}");
    }
}

#[test]
fn reads_viewer_links_only_of_the_expected_shape() {
    for (href, expected) in [
        (
            "/ark:/99999/vtaexample0001/daogrp/0/layout:table/idsearch:RECH_x",
            Some("/ark:/99999/vtaexample0001/daogrp/0"),
        ),
        (
            "/ark:/99999/vtaexample0001/daoloc/0",
            Some("/ark:/99999/vtaexample0001/daoloc/0"),
        ),
        (
            "/ark:/99999/1178893.example/dao/0",
            Some("/ark:/99999/1178893.example/dao/0"),
        ),
        ("/ark:/99999/vtaexample0001", None),
        ("/ark:/99999/vtaexample0001/daogrp/x", None),
        ("/ark:/99999/vta example/daogrp/0", None),
        ("/ark:/99999/vtaexample0001/daogrp/0/9", None),
        ("/other/99999/vtaexample0001/daogrp/0", None),
    ] {
        let parsed = page::Register::parse(href).map(|register| register.viewer());
        assert_eq!(parsed.as_deref(), expected, "{href}");
    }
}

/// An archive of the Ain portal shown in the portal's own viewer only.
fn portal_registry() -> ArchiveRegistry {
    let portal = &ArchiveRegistry::embedded()
        .archive("AD01")
        .unwrap()
        .collections[0]
        .portal;
    let document = serde_json::json!({
        "id": "fr-ad00",
        "country": "FR",
        "level": "departmental",
        "name": "Archives départementales d'Exemple",
        "citation_codes": ["AD00"],
        "website": "https://archives.example.org",
        "collections": [{
            "id": "registers",
            "acts": ["B", "N", "M", "S", "D"],
            "platform": "ligeo",
            "portal": portal,
        }]
    })
    .to_string();
    ArchiveRegistry::new(&[("fr", document.as_str())], platform::builtin()).unwrap()
}

#[test]
fn a_portal_archive_reads_the_image_count_from_the_row_and_fetches_no_manifest() {
    let registry = portal_registry();
    let fetch = Fixtures::new(AIN_ONE);
    let target = resolve(
        &registry,
        "AD00 - Exampleville - (aucun) - N - 1880 - vue 5/120",
        &fetch,
    )
    .unwrap();
    assert_eq!(fetch.requests().len(), 1);
    let ArchiveTarget::View {
        views, view_count, ..
    } = &target
    else {
        panic!("expected a view, got {target:?}");
    };
    assert_eq!(*view_count, Some(120));
    assert_eq!(views.len(), 1);
    assert_eq!((views[0].ark.as_deref(), &views[0].image), (None, &None));

    let target = resolve(
        &registry,
        "AD00 - Exampleville - (aucun) - N - 1880 - vue 121/120",
        &Fixtures::new(AIN_ONE),
    )
    .unwrap();
    assert!(matches!(&target, ArchiveTarget::View { views, .. } if views.is_empty()));
}

#[test]
fn reads_the_endpoint_and_the_search_page_of_each_collection() {
    let registry = ArchiveRegistry::embedded();
    let ain = &registry.archive("AD01").unwrap().collections[0];
    let endpoint = Ligeo.endpoint(ain).unwrap();
    assert_eq!(endpoint.origin, AIN);
    assert_eq!(
        endpoint.start,
        format!("{AIN}/archive/recherche/etatcivil/n:88")
    );
    assert_eq!(endpoint.access, Access::Any);
    assert!(endpoint.other_origins.is_empty());

    let ad07 = registry.archive("AD07").unwrap();
    assert_eq!(ad07.collections[0].id, "parish-registers");
    let citation = registry
        .parse("AD07 - Exampleville - (aucun) - B - 1700")
        .unwrap();
    let (_, collections) = registry.candidates(&citation).unwrap();
    assert_eq!(collections[0].id, "parish-registers");
    assert_eq!(
        Ligeo.results_url(collections[0], &citation).unwrap(),
        format!(
            "{ARDECHE}/archive/resultats/paroissiaux/n:164?RECH_commune=Exampleville\
             &RECH_unitdate_debut=1700&RECH_unitdate_fin=1700&type=paroissiaux"
        )
    );
    // A citation without a year leaves the years out.
    let citation = registry
        .parse("AD01 - Exampleville - (aucun) - N - acte 3")
        .unwrap();
    let (_, collections) = registry.candidates(&citation).unwrap();
    let url = Ligeo.results_url(collections[0], &citation).unwrap();
    assert_eq!(
        url,
        format!(
            "{AIN}/archive/resultats/etatcivil/n:88?RECH_commune=Exampleville&RECH_acte%5B%5D=N&type=etatcivil"
        )
    );
}

fn collection_with(change: impl FnOnce(&mut serde_json::Value)) -> Collection {
    let mut collection = ArchiveRegistry::embedded()
        .archive("AD01")
        .unwrap()
        .collections[0]
        .clone();
    change(&mut collection.portal);
    collection
}

#[test]
fn a_portal_under_the_archives_prefix_and_behind_a_challenge_is_a_setting() {
    let collection = collection_with(|portal| {
        portal["prefix"] = "/archives".into();
        portal["transport"] = "browser".into();
    });
    assert_eq!(Ligeo.validate(&collection), Ok(()));
    let endpoint = Ligeo.endpoint(&collection).unwrap();
    assert_eq!(endpoint.access, Access::Browser);
    assert_eq!(
        endpoint.start,
        format!("{AIN}/archives/recherche/etatcivil/n:88")
    );
    let citation = ArchiveRegistry::embedded()
        .parse("AD01 - Exampleville - (aucun) - N - 1880")
        .unwrap();
    assert!(
        Ligeo
            .results_url(&collection, &citation)
            .unwrap()
            .starts_with(&format!("{AIN}/archives/resultats/etatcivil/n:88?"))
    );
}

#[test]
fn validates_its_settings_against_the_collection() {
    assert_eq!(Ligeo.validate(&collection_with(|_| {})), Ok(()));
    type Change = fn(&mut serde_json::Value);
    let cases: [(&str, Change); 13] = [
        ("unknown field", |p| p["thesaurus"] = "x".into()),
        ("https origin", |p| {
            p["origin"] = "http://archives.example.org".into()
        }),
        ("prefix must be a path", |p| p["prefix"] = "archive".into()),
        ("search and node", |p| p["search"] = "etat civil".into()),
        ("search and node", |p| p["node"] = 0.into()),
        ("input names", |p| p["fields"]["locality"] = "".into()),
        ("go together", |p| {
            p["fields"].as_object_mut().unwrap().remove("year_to");
        }),
        ("fields.act without acts", |p| {
            p.as_object_mut().unwrap().remove("acts");
        }),
        ("not an act code", |p| p["acts"]["X"] = "X".into()),
        ("no filter for `D`", |p| {
            p["acts"].as_object_mut().unwrap().remove("D");
        }),
        ("needs `fields.act`", |p| {
            p["fields"].as_object_mut().unwrap().remove("act");
        }),
        ("year replaces", |p| {
            p["fields"]["year"] = "RECH_annee".into()
        }),
        ("must not be blank", |p| p["columns"]["period"] = " ".into()),
    ];
    for (expected, change) in cases {
        let error = Ligeo
            .validate(&collection_with(change))
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{expected}: {error}");
    }
}

const MATRICULES: &str = include_str!("../../../fixtures/ligeo/matricules.html");

/// A fictitious archive whose military registers are searched by bureau and
/// class, without an act filter; `columns` are the columns read.
fn matricules_registry(columns: serde_json::Value) -> ArchiveRegistry {
    let document = serde_json::json!({
        "id": "fr-ad00",
        "country": "FR",
        "level": "departmental",
        "name": "Archives départementales d'Exemple",
        "citation_codes": ["AD00"],
        "website": "https://archives.example.org",
        "collections": [{
            "id": "military-registers",
            "acts": ["RM"],
            "platform": "ligeo",
            "portal": {
                "origin": "https://archives.example.org",
                "search": "matricules",
                "node": 77,
                "fields": {
                    "locality": "RECH_bureau",
                    "year_from": "RECH_classe_debut",
                    "year_to": "RECH_classe_fin"
                },
                "columns": columns
            }
        }]
    })
    .to_string();
    ArchiveRegistry::new(&[("fr", document.as_str())], platform::builtin()).unwrap()
}

#[test]
fn a_military_register_is_chosen_by_bureau_class_and_matricule() {
    let registry = matricules_registry(serde_json::json!({
        "locality": "Bureau de recrutement",
        "period": "Classe",
        "call_number": "Cote",
        "numbers": "Matricules"
    }));
    // The matricule falls in the second volume of the class; the cited call
    // number, which no row carries, does not discard them.
    let fetch = Fixtures::new(MATRICULES);
    let target = resolve(
        &registry,
        "AD00 - Exampleville - Registres matricules - 1870 - 1 R 9999 - 612 - 300/398",
        &fetch,
    )
    .unwrap();
    assert_eq!(viewed(&target), "vtaexample0062");
    let ArchiveTarget::View {
        views,
        view_count,
        call_number,
        ..
    } = &target
    else {
        panic!("expected a view, got {target:?}");
    };
    assert_eq!(
        (views[0].view, *view_count, call_number.as_deref()),
        (300, Some(398), Some("1 R 902"))
    );
    // The portal learns the bureau and the class: no act filter, never the
    // matricule or the call number.
    assert_eq!(
        fetch.requests(),
        [
            "/archive/resultats/matricules/n:77?RECH_bureau=Exampleville\
          &RECH_classe_debut=1870&RECH_classe_fin=1870&type=matricules"
        ]
    );

    // The class written inside a free field, and the matricule after its
    // word.
    let target = resolve(
        &registry,
        "AD00 - Exampleville - Registre matricules - 1 R 9999 - \
         Bureau de Exampleville n° 1 à 500 (1870) - matricule 12 - vue 5/412",
        &Fixtures::new(MATRICULES),
    )
    .unwrap();
    assert_eq!(viewed(&target), "vtaexample0061");

    // Without a matricule or a view count, the two volumes of the class.
    let target = resolve(
        &registry,
        "AD00 - Exampleville - Registres matricules - 1870",
        &Fixtures::new(MATRICULES),
    );
    assert_eq!(matches(&target), Some(2));
}

#[test]
fn the_matricules_of_a_volume_are_read_from_its_title_without_their_column() {
    let registry = matricules_registry(serde_json::json!({
        "locality": "Bureau de recrutement",
        "period": "Classe"
    }));
    let target = resolve(
        &registry,
        "AD00 - Exampleville - Registres matricules - 1870 - matricule 501",
        &Fixtures::new(MATRICULES),
    )
    .unwrap();
    assert_eq!(viewed(&target), "vtaexample0062");
    // The rows read as military registers from their links' titles.
    let columns = Columns {
        locality: Some(Names::One("Bureau de recrutement".to_owned())),
        ..Columns::default()
    };
    let found = page::results(MATRICULES, &columns).unwrap();
    assert!(
        found
            .rows
            .iter()
            .all(|row| row.act.as_deref() == Some("RM"))
    );
    assert_eq!(found.rows[0].numbers, Some((1, 500)));
}

#[test]
fn a_series_collection_needs_a_filter_only_where_the_form_has_one() {
    let with_acts = |acts: serde_json::Value, portal: serde_json::Value| {
        let mut collection = collection_with(|p| *p = portal);
        collection.acts = serde_json::from_value(acts).unwrap();
        collection
    };
    let ain = collection_with(|_| {}).portal;
    // Ain's register search has an act filter: a census needs its value.
    let error = Ligeo
        .validate(&with_acts(serde_json::json!(["N", "RP"]), ain.clone()))
        .unwrap_err();
    assert!(error.to_string().contains("no filter for `RP`"), "{error}");
    let mut portal = ain;
    portal["acts"]["RP"] = "recensements".into();
    assert_eq!(
        Ligeo.validate(&with_acts(serde_json::json!(["N", "RP"]), portal)),
        Ok(())
    );
}

const NOTICES: &str = include_str!("../../../fixtures/ligeo/notices.html");
const QUALIFIED: &str = include_str!("../../../fixtures/ligeo/qualified.html");
const INDEX: &str = include_str!("../../../fixtures/ligeo/index.html");
const FONDS: &str = include_str!("../../../fixtures/ligeo/fonds.html");
const TITLES: &str = include_str!("../../../fixtures/ligeo/titles.html");

const EXAMPLE: &str = "https://archives.example.org";

/// A fictitious archive with one collection holding `acts`, searched with
/// the `portal` settings, shown in the portal's viewer.
fn registry_with(acts: serde_json::Value, portal: serde_json::Value) -> ArchiveRegistry {
    let document = serde_json::json!({
        "id": "fr-ad00",
        "country": "FR",
        "level": "departmental",
        "name": "Archives départementales d'Exemple",
        "citation_codes": ["AD00"],
        "website": EXAMPLE,
        "collections": [{
            "id": "registers",
            "acts": acts,
            "platform": "ligeo",
            "portal": portal,
        }]
    })
    .to_string();
    ArchiveRegistry::new(&[("fr", document.as_str())], platform::builtin()).unwrap()
}

/// The register a title resolves to over `search`, or the count of results.
fn chosen(registry: &ArchiveRegistry, title: &str, search: &'static str) -> Result<String, usize> {
    match resolve(registry, title, &Fixtures::new(search)) {
        Ok(target @ ArchiveTarget::View { .. }) => Ok(viewed(&target).to_owned()),
        Ok(ArchiveTarget::Results { matches, .. }) => Err(matches.unwrap_or_default()),
        Err(error) => panic!("{title}: {error:?}"),
    }
}

fn notices_registry() -> ArchiveRegistry {
    registry_with(
        serde_json::json!(["B", "M", "S", "N", "D", "TD"]),
        serde_json::json!({
            "origin": EXAMPLE,
            "search": "etatcivil",
            "node": 11,
            "fields": {
                "locality": "RECH_commune",
                "act": "RECH_acte[]",
                "year_from": "RECH_dates_debut",
                "year_to": "RECH_dates_fin"
            },
            "acts": {
                "B": "Bapteme", "N": "Naissance", "M": "Mariage",
                "S": "Sepulture", "D": "Deces", "TD": "Table"
            },
            "columns": {
                "locality": "Commune ou lieu-dit",
                "parish": "Paroisse",
                "acts": "Sujet",
                "period": "Dates",
                "call_number": "cote"
            }
        }),
    )
}

#[test]
fn notices_are_read_by_their_labels_and_their_heading() {
    let columns = Columns {
        locality: Some(Names::One("Commune ou lieu-dit".to_owned())),
        parish: Some(Names::One("Paroisse".to_owned())),
        acts: Some(Names::One("Sujet".to_owned())),
        period: Some(Names::One("Dates".to_owned())),
        call_number: Some(Names::One("cote".to_owned())),
        ..Columns::default()
    };
    let found = page::results(NOTICES, &columns).unwrap();
    assert_eq!(found.total, Some(5));
    let read: Vec<_> = found
        .rows
        .iter()
        .map(|row| {
            (
                row.locality.as_deref(),
                row.call_number.as_deref(),
                row.act.as_deref(),
                row.parish.as_deref(),
                row.period.as_deref(),
                row.images,
            )
        })
        .collect();
    assert_eq!(
        read[..4],
        [
            (
                Some("Exampleville"),
                Some("9 E 71/2"),
                Some("NMD"),
                None,
                Some("1829-1861"),
                Some(269)
            ),
            (
                Some("Exampleville"),
                Some("9 Mi 72"),
                Some("DMN"),
                None,
                Some("1841 1860"),
                Some(173)
            ),
            (
                Some("Exampleville"),
                Some("9 Mi 73"),
                Some("TD"),
                None,
                Some("1843 1852"),
                Some(16)
            ),
            (
                Some("Exampleville"),
                Some("9 Mi 74"),
                Some("BSM"),
                Some("Saint-Exemple"),
                Some("1620 1746"),
                Some(348)
            ),
        ]
    );
    // The notice of a register not digitised has no viewer link.
    assert_eq!(found.rows[4].payload.register, None);
    // The linear layout's links open the same viewer path.
    assert_eq!(
        found.rows[0]
            .payload
            .register
            .as_ref()
            .map(page::Register::viewer)
            .as_deref(),
        Some("/ark:/99999/vtaexample0071/daogrp/0")
    );
}

#[test]
fn notices_select_their_register_and_skip_one_not_digitised() {
    let registry = notices_registry();
    // Two registers of births cover 1850, written `1841 1860` and
    // `1829-1861`; the one not digitised is not a candidate.
    assert_eq!(
        chosen(
            &registry,
            "AD00 - Exampleville - (aucun) - N - 1850",
            NOTICES
        ),
        Err(2)
    );
    assert_eq!(
        chosen(
            &registry,
            "AD00 - Exampleville - (aucun) - N - 1850 - 9 Mi 72",
            NOTICES
        )
        .as_deref(),
        Ok("vtaexample0072")
    );
    assert_eq!(
        chosen(
            &registry,
            "AD00 - Exampleville - (aucun) - N - 1850 - 9 E 75/1",
            NOTICES
        ),
        Err(2)
    );
    assert_eq!(
        chosen(
            &registry,
            "AD00 - Exampleville - (aucun) - TD - 1850",
            NOTICES
        )
        .as_deref(),
        Ok("vtaexample0073")
    );
    assert_eq!(
        chosen(
            &registry,
            "AD00 - Exampleville - Saint-Exemple - B - 1700",
            NOTICES
        )
        .as_deref(),
        Ok("vtaexample0074")
    );
}

fn qualified_registry() -> ArchiveRegistry {
    registry_with(
        serde_json::json!(["B", "M", "S", "N", "D", "TD"]),
        serde_json::json!({
            "origin": EXAMPLE,
            "search": "etatcivil",
            "node": 12,
            "fields": {
                "locality": "RECH_commune",
                "year_from": "RECH_unitdate_debut",
                "year_to": "RECH_unitdate_fin"
            },
            "columns": {
                "locality": "Commune",
                "acts": ["Type de document", "Type d'acte"],
                "period": "Dates",
                "call_number": "Cote"
            }
        }),
    )
}

#[test]
fn a_locality_cell_names_its_commune_with_the_places_within_it() {
    let registry = qualified_registry();
    // The hamlet's register, dated with full dates, and the commune's.
    assert_eq!(
        chosen(
            &registry,
            "AD00 - Exampleville - (aucun) - B - 1700",
            QUALIFIED
        ),
        Err(2)
    );
    for (title, register) in [
        // The hamlet is the place within its commune, read as the parish.
        ("AD00 - Exampleville - Hameau - B - 1700", "vtaexample0081"),
        ("AD00 - Hameau - (aucun) - B - 1700", "vtaexample0081"),
        // A parish after its commune.
        (
            "AD00 - Exampleville - Saint-Exemple - B - 1700",
            "vtaexample0082",
        ),
        // A cell naming a former commune, a note and the current one.
        ("AD00 - Ancienne - (aucun) - N - 1850", "vtaexample0083"),
        (
            "AD00 - Exampleville - (aucun) - N - 1850 - 9 E 85/1",
            "vtaexample0085",
        ),
        // The document type tells the decennial table from the births.
        (
            "AD00 - Exampleville - (aucun) - TD - 1850",
            "vtaexample0084",
        ),
    ] {
        assert_eq!(
            chosen(&registry, title, QUALIFIED).as_deref(),
            Ok(register),
            "{title}"
        );
    }
    assert_eq!(
        chosen(
            &registry,
            "AD00 - Exampleville - (aucun) - N - 1850",
            QUALIFIED
        ),
        Err(2)
    );
    // The register of deaths, listed without a viewer link, is no
    // candidate: the act test would leave none, so the others of the year
    // remain.
    assert_eq!(
        chosen(
            &registry,
            "AD00 - Exampleville - (aucun) - D - 1850",
            QUALIFIED
        ),
        Err(3)
    );
}

#[test]
fn an_index_of_persons_finds_the_person_by_the_cited_matricule() {
    let registry = registry_with(
        serde_json::json!(["RM"]),
        serde_json::json!({
            "origin": EXAMPLE,
            "search": "matricules",
            "node": 91,
            "fields": {
                "locality": "RECH_bureau[]",
                "year": "RECH_Classe_exacte",
                "number": "rech_mat"
            },
            "columns": {
                "locality": "Bureau",
                "period": "Classe",
                "call_number": "Cote",
                "numbers": "Matricule"
            }
        }),
    );
    let fetch = Fixtures::new(INDEX);
    let target = resolve(
        &registry,
        "AD00 - Exampleville - Registres matricules - 1890 - 9 R 1 - 984 - 579/833",
        &fetch,
    )
    .unwrap();
    // The class is a single year's input; the matricule goes to the index.
    assert_eq!(
        fetch.requests(),
        [
            "/archive/resultats/matricules/n:91?RECH_bureau%5B%5D=Exampleville\
          &RECH_Classe_exacte=1890&rech_mat=984&type=matricules"
        ]
    );
    // The person's row opens on its one view, whatever the register's view
    // the citation counts.
    assert_eq!(
        target,
        ArchiveTarget::View {
            url: format!("{EXAMPLE}/ark:/99999/vtaexample0091/daoloc/0/1"),
            views: Vec::new(),
            view_count: Some(1),
            call_number: Some("9R0001".to_owned()),
            attribution: None,
        }
    );
}

#[test]
fn a_search_within_a_finding_aid_reads_the_commune_from_the_notices_path() {
    let registry = registry_with(
        serde_json::json!(["B", "M", "S", "N", "D"]),
        serde_json::json!({
            "origin": EXAMPLE,
            "prefix": "/archives",
            "fonds": "FRAD000_1",
            "search": "inventaire",
            "node": 3,
            "fields": { "locality": "RECH_S" },
            "params": { "RECH_eadid": "FRAD000_1" },
            "columns": {
                "locality": "Contexte",
                "acts": "unittitle",
                "period": "date",
                "call_number": "cote"
            }
        }),
    );
    let collection = &registry.archive("AD00").unwrap().collections[0];
    assert_eq!(
        Ligeo.endpoint(collection).unwrap().start,
        format!("{EXAMPLE}/archives/fonds/FRAD000_1")
    );
    let fetch = Fixtures::new(FONDS);
    let target = resolve(
        &registry,
        "AD00 - Exampleville - (aucun) - D - 1900",
        &fetch,
    )
    .unwrap();
    assert_eq!(viewed(&target), "vtaexample0102");
    assert_eq!(
        fetch.requests(),
        [
            "/archives/fonds/FRAD000_1/inventaire/n:3?RECH_S=Exampleville&RECH_eadid=FRAD000_1&type=inventaire"
        ]
    );
    let ArchiveTarget::View {
        url, call_number, ..
    } = &target
    else {
        unreachable!("a view");
    };
    assert_eq!(url, &format!("{EXAMPLE}/ark:/99999/vtaexample0102/dao/0/1"));
    assert_eq!(call_number.as_deref(), Some("9 NUM /1EC3"));
    // The text search also found another commune's notice naming this one.
    assert_eq!(
        chosen(&registry, "AD00 - Exampleville - (aucun) - N - 1874", FONDS).as_deref(),
        Ok("vtaexample0101")
    );
}

#[test]
fn a_title_ends_its_locality_at_a_full_stop_and_dash_and_may_start_with_a_call_number() {
    let columns = Columns {
        title: Some(Names::One("Commune et type d'acte".to_owned())),
        period: Some(Names::One("Date".to_owned())),
        ..Columns::default()
    };
    let found = page::results(TITLES, &columns).unwrap();
    let read: Vec<_> = found
        .rows
        .iter()
        .map(|row| {
            (
                row.locality.as_deref(),
                row.act.as_deref(),
                row.call_number.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        read,
        [
            (Some("Exampleville"), Some("BMS"), None),
            (Some("Exampleville"), Some("TD"), None),
            (Some("Exampleville"), Some("RP"), Some("9 M 99")),
        ]
    );
}

#[test]
fn layout_page_params_and_single_years_shape_the_addresses() {
    let collection = collection_with(|portal| {
        portal["layout"] = "Tableau".into();
        portal["page"] = "menu".into();
        portal["params"] = serde_json::json!({ "RECH_dep": "Exampledept" });
        let fields = portal["fields"].as_object_mut().unwrap();
        fields.remove("year_from");
        fields.remove("year_to");
        fields.insert("year".to_owned(), "RECH_annee".into());
    });
    assert_eq!(Ligeo.validate(&collection), Ok(()));
    assert_eq!(
        Ligeo.endpoint(&collection).unwrap().start,
        format!("{AIN}/archive/recherche/menu/n:88")
    );
    let citation = ArchiveRegistry::embedded()
        .parse("AD01 - Exampleville - (aucun) - N - 1880")
        .unwrap();
    assert_eq!(
        Ligeo.results_url(&collection, &citation).unwrap(),
        format!(
            "{AIN}/archive/resultats/etatcivil/Tableau/n:88?RECH_commune=Exampleville\
             &RECH_acte%5B%5D=N&RECH_dep=Exampledept&RECH_annee=1880&type=etatcivil"
        )
    );
    // A margin widens the years searched, for a portal indexing registers
    // by other years than their titles show.
    let widened = collection_with(|portal| portal["fields"]["year_margin"] = 1.into());
    assert!(
        Ligeo
            .results_url(&widened, &citation)
            .unwrap()
            .contains("&RECH_unitdate_debut=1879&RECH_unitdate_fin=1881&")
    );
}

#[test]
fn validates_the_settings_of_the_new_shapes() {
    type Change = fn(&mut serde_json::Value);
    let cases: [(&str, Change); 5] = [
        ("year replaces", |p| {
            p["fields"]["year"] = "RECH_annee".into()
        }),
        ("year_margin needs", |p| {
            let fields = p["fields"].as_object_mut().unwrap();
            fields.remove("year_from");
            fields.remove("year_to");
            fields.insert("year_margin".to_owned(), 1.into());
        }),
        ("page, fonds and layout", |p| {
            p["layout"] = "Tab leau".into()
        }),
        ("page, fonds and layout", |p| p["fonds"] = "../x".into()),
        ("params need", |p| {
            p["params"] = serde_json::json!({ "RECH_dep": " " })
        }),
    ];
    for (expected, change) in cases {
        let error = Ligeo
            .validate(&collection_with(change))
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{expected}: {error}");
    }
    // A series searched by year alone needs no locality input or column.
    let by_year = collection_with(|p| {
        p["fields"].as_object_mut().unwrap().remove("locality");
        p["columns"].as_object_mut().unwrap().remove("locality");
    });
    assert_eq!(Ligeo.validate(&by_year), Ok(()));
}
