//! The adapter over anonymized answers of the three search modules
//! (`fixtures/archinoe/`, written by its `generate.py`), with the catalogue's
//! own settings for the Charente-Maritime and Oise (`registre`),
//! Pas-de-Calais (`seriel`) and Côte-d'Or (`ead`) archives.

use std::pin::pin;
use std::sync::Mutex;
use std::task::{Context, Poll, Waker};

use crate::platform::{self, BoxFuture};
use crate::transport::{FetchError, Method, PortalFetch, PortalRequest};
use crate::{ArchiveRegistry, ArchiveTarget, ArchiveView, ResolveError};

const AD17_FORM: &str = include_str!("../../../fixtures/archinoe/ad17-form.html");
const AD17_ONE: &str = include_str!("../../../fixtures/archinoe/ad17-one.html");
const AD17_SEVERAL: &str = include_str!("../../../fixtures/archinoe/ad17-several.html");
const AD17_WITH_TABLE: &str = include_str!("../../../fixtures/archinoe/ad17-with-table.html");
const AD17_NONE: &str = include_str!("../../../fixtures/archinoe/ad17-none.html");
const AD60_LICENCE: &str = include_str!("../../../fixtures/archinoe/ad60-licence.html");
const AD60_FORM: &str = include_str!("../../../fixtures/archinoe/ad60-form.html");
const AD60_ONE: &str = include_str!("../../../fixtures/archinoe/ad60-one.html");
const AD60_SEVERAL: &str = include_str!("../../../fixtures/archinoe/ad60-several.html");
const AD60_NONE: &str = include_str!("../../../fixtures/archinoe/ad60-none.html");
const VIEWER_12: &str = include_str!("../../../fixtures/archinoe/viewer-12.html");
const VIEWER_40: &str = include_str!("../../../fixtures/archinoe/viewer-40.html");
const AD62_PAGE_0: &str = include_str!("../../../fixtures/archinoe/ad62-page-0.html");
const AD62_PAGE_1: &str = include_str!("../../../fixtures/archinoe/ad62-page-1.html");
const AD62_ONE: &str = include_str!("../../../fixtures/archinoe/ad62-one.html");
const AD62_NONE: &str = include_str!("../../../fixtures/archinoe/ad62-none.html");
const AD62_ARTICLE: &str = include_str!("../../../fixtures/archinoe/ad62-article.html");
const AD62_CHALLENGE: &str = include_str!("../../../fixtures/archinoe/ad62-challenge.html");
const AD21_ROOT: &str = include_str!("../../../fixtures/archinoe/ad21-root.html");
const AD21_TOC_COMMUNE: &str = include_str!("../../../fixtures/archinoe/ad21-toc-commune.html");
const AD21_TOC_ACTES: &str = include_str!("../../../fixtures/archinoe/ad21-toc-actes.html");
const AD21_COMMUNALE: &str = include_str!("../../../fixtures/archinoe/ad21-notice-communale.html");
const AD21_DEPARTEMENTALE: &str =
    include_str!("../../../fixtures/archinoe/ad21-notice-departementale.html");
const AD21_TABLES: &str = include_str!("../../../fixtures/archinoe/ad21-notice-tables.html");

const AD17_SEARCH: &str = "https://archinoe.com/v2/ad17/registre.html";
const AD62_ACTION: &str = "/console/ir_seriel_action.php?f=0&cle=formulaire_etat_civil&id=56";

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

/// What the portal answers to a request, or `None` for a status 404.
type Router = fn(&PortalRequest) -> Option<String>;

/// Answers each request through a router and records them.
struct Portal {
    router: Router,
    requests: Mutex<Vec<PortalRequest>>,
}

impl Portal {
    fn new(router: Router) -> Self {
        Self {
            router,
            requests: Mutex::new(Vec::new()),
        }
    }

    fn requests(&self) -> Vec<PortalRequest> {
        self.requests.lock().unwrap().clone()
    }

    /// The paths of the recorded requests.
    fn urls(&self) -> Vec<String> {
        self.requests().into_iter().map(|r| r.url).collect()
    }
}

impl PortalFetch for Portal {
    fn request<'a>(
        &'a self,
        request: &'a PortalRequest,
    ) -> BoxFuture<'a, Result<String, FetchError>> {
        Box::pin(async move {
            self.requests.lock().unwrap().push(request.clone());
            (self.router)(request).ok_or(FetchError::Status(404))
        })
    }
}

/// Resolves `title` with a fresh adapter, which has kept no commune index.
fn resolve(title: &str, portal: &Portal) -> Result<ArchiveTarget, ResolveError> {
    resolve_with(&platform::Archinoe::new(), title, portal)
}

fn resolve_with(
    adapter: &platform::Archinoe,
    title: &str,
    portal: &Portal,
) -> Result<ArchiveTarget, ResolveError> {
    let registry = ArchiveRegistry::embedded();
    let citation = registry.parse(title).expect("a normalized citation");
    let (archive, collections) = registry.candidates(&citation).expect("a catalogued act");
    assert_eq!(collections[0].platform, "archinoe");
    block_on(platform::Platform::resolve(
        adapter,
        archive,
        collections[0],
        &citation,
        portal,
    ))
}

fn ok(title: &str, router: Router) -> (ArchiveTarget, Portal) {
    let portal = Portal::new(router);
    let target = resolve(title, &portal).unwrap_or_else(|error| panic!("{title}: {error}"));
    (target, portal)
}

fn matches_of(target: &ArchiveTarget) -> Option<usize> {
    match target {
        ArchiveTarget::Results { matches, .. } => *matches,
        ArchiveTarget::View { .. } => panic!("expected results, got {target:?}"),
    }
}

// ------------------------------------------------------------ registre

fn ad17(request: &PortalRequest) -> Option<String> {
    let url = request.url.as_str();
    let body = if url == "/v2/ad17/registre.html" {
        AD17_FORM
    } else if url.starts_with("/v2/ad17/registre_liste.html?") {
        if url.contains("annee=1620") {
            AD17_ONE
        } else if url.contains("annee=1699") {
            AD17_SEVERAL
        } else if url.contains("annee=1912") {
            AD17_WITH_TABLE
        } else {
            AD17_NONE
        }
    } else if url == "/v2/ad17/visualiseur/registre.html?id=100000103" {
        VIEWER_40
    } else if url.starts_with("/v2/ad17/visualiseur/registre.html?id=") {
        VIEWER_12
    } else {
        return None;
    };
    Some(body.to_owned())
}

#[test]
fn a_register_is_found_with_the_locality_list_and_one_search() {
    let (target, portal) = ok(
        "AD17 - Exampleville - Saint-Exemple - B - 1620 - 9 E 99/1 - vue 5/12",
        ad17,
    );
    assert_eq!(
        target,
        ArchiveTarget::View {
            url: "https://archinoe.com/v2/ad17/visualiseur/registre.html?id=100000101&vue=5"
                .to_owned(),
            views: vec![ArchiveView {
                view: 5,
                url: "https://archinoe.com/v2/ad17/visualiseur/registre.html?id=100000101&vue=5"
                    .to_owned(),
                ark: None,
                image: None,
            }],
            // The search shows no image count, and the viewer was not read.
            view_count: None,
            call_number: Some("9 E 99/1".to_owned()),
            attribution: None,
            renumbering: None,
        }
    );
    // The form's locality list, then the search: the portal learns the
    // locality, act and year, never the call number, parish or view.
    assert_eq!(
        portal.urls(),
        [
            "/v2/ad17/registre.html",
            "/v2/ad17/registre_liste.html?commune=100000001&acte=170000001&annee=1620&ajax=true"
        ]
    );
    assert!(portal.requests().iter().all(|r| r.method == Method::Get));
}

#[test]
fn several_registers_are_told_apart_by_the_citation_parts() {
    let (target, portal) = ok("AD17 - Exampleville - (aucun) - B - 1699", ad17);
    assert_eq!(matches_of(&target), Some(3));
    assert_eq!(target.url(), AD17_SEARCH);
    assert_eq!(portal.urls().len(), 2);

    // The parish is the observations cell without its `Paroisse` prefix.
    let (target, _) = ok("AD17 - Exampleville - Notre-Dame-Exemple - B - 1699", ad17);
    assert!(target.url().ends_with("id=100000103"), "{target:?}");
    let (target, _) = ok("AD17 - Exampleville - (aucun) - B - 1699 - 9E99/4", ad17);
    assert!(target.url().ends_with("id=100000104"), "{target:?}");
    // A call number no row carries is not a guess among the others.
    let (target, _) = ok("AD17 - Exampleville - (aucun) - B - 1699 - 9 E 99/9", ad17);
    assert_eq!(matches_of(&target), Some(3));
}

#[test]
fn the_image_count_is_read_from_the_viewers_of_the_remaining_registers() {
    let (target, portal) = ok("AD17 - Exampleville - (aucun) - B - 1699 - vue 3/40", ad17);
    let ArchiveTarget::View {
        url, view_count, ..
    } = &target
    else {
        panic!("expected a view, got {target:?}");
    };
    assert_eq!(
        url,
        "https://archinoe.com/v2/ad17/visualiseur/registre.html?id=100000103&vue=3"
    );
    assert_eq!(*view_count, Some(40));
    // The form, the search, and the three registers' viewers.
    assert_eq!(portal.urls().len(), 5);

    // No register of that size: still the results.
    let (target, _) = ok("AD17 - Exampleville - (aucun) - B - 1699 - vue 3/99", ad17);
    assert_eq!(matches_of(&target), Some(3));
}

#[test]
fn a_list_of_tables_is_not_a_register_of_the_cited_acts() {
    // The births search also returns the table of the same years.
    let (target, _) = ok("AD17 - Exampleville - (aucun) - N - 1912", ad17);
    assert!(target.url().ends_with("id=100000106"), "{target:?}");
    // Cited as tables, they are what is wanted.
    let (target, _) = ok("AD17 - Exampleville - (aucun) - TD - 1912", ad17);
    assert_eq!(matches_of(&target), Some(2));
}

#[test]
fn no_register_and_no_locality_give_empty_results() {
    let (target, portal) = ok("AD17 - Exampleville - (aucun) - B - 1500", ad17);
    assert_eq!(matches_of(&target), Some(0));
    assert_eq!(portal.urls().len(), 2);

    let (target, portal) = ok("AD17 - Nowhere - (aucun) - B - 1620", ad17);
    assert_eq!(matches_of(&target), Some(0));
    assert_eq!(portal.urls().len(), 1);
}

#[test]
fn the_locality_is_matched_without_case_accents_or_article_order() {
    for (title, commune) in [
        ("AD17 - Le Bourg-Exemple - (aucun) - B - 1620", "100000002"),
        (
            "AD17 - Saint-Exemple-d'Aval - (aucun) - B - 1620",
            "100000003",
        ),
        (
            "AD17 - Saint Exemple d’Aval - (aucun) - B - 1620",
            "100000003",
        ),
    ] {
        let (_, portal) = ok(title, ad17);
        assert!(
            portal.urls()[1].contains(&format!("commune={commune}&")),
            "{title}: {:?}",
            portal.urls()
        );
    }
}

#[test]
fn a_changed_search_is_reported_as_drift() {
    fn without_total(request: &PortalRequest) -> Option<String> {
        let answer = ad17(request)?;
        Some(answer.replace("class=\"total\"", "class=\"sum\""))
    }
    fn without_cote_column(request: &PortalRequest) -> Option<String> {
        let answer = ad17(request)?;
        Some(answer.replace(">Cote<", ">Reference<"))
    }
    fn without_locality_list(request: &PortalRequest) -> Option<String> {
        let answer = ad17(request)?;
        Some(answer.replace("name=\"commune\"", "name=\"town\""))
    }
    fn short_rows(request: &PortalRequest) -> Option<String> {
        let answer = ad17(request)?;
        Some(answer.replace("<span class='Cell'><span>Table</span></span>", ""))
    }
    for router in [
        without_total as Router,
        without_cote_column,
        without_locality_list,
        short_rows,
    ] {
        let portal = Portal::new(router);
        let result = resolve("AD17 - Exampleville - (aucun) - B - 1620", &portal);
        assert!(
            matches!(result, Err(ResolveError::UnexpectedResponse(_))),
            "{result:?}"
        );
    }
}

fn ad60(request: &PortalRequest) -> Option<String> {
    // A reader who has not accepted the licence reaches its page.
    (request.url == "/v2/ad60/registre.html").then(|| AD60_LICENCE.to_owned())
}

fn ad60_accepted(request: &PortalRequest) -> Option<String> {
    let url = request.url.as_str();
    let body = if url == "/v2/ad60/registre.html" {
        AD60_FORM
    } else if url.starts_with("/v2/ad60/registre_liste.html?commune=600000001&") {
        AD60_ONE
    } else if url.starts_with("/v2/ad60/registre_liste.html?commune=600000002&") {
        AD60_SEVERAL
    } else if url.starts_with("/v2/ad60/registre_liste.html?commune=600000003&") {
        AD60_NONE
    } else {
        return None;
    };
    Some(body.to_owned())
}

#[test]
fn a_licence_is_left_to_the_reader() {
    let (target, portal) = ok("AD60 - Exampleville - (aucun) - B - 1663", ad60);
    assert_eq!(
        target,
        ArchiveTarget::Results {
            url: "https://ressources.archives.oise.fr/v2/ad60/registre.html".to_owned(),
            matches: None,
        }
    );
    // Nothing is sent to accept the licence for the reader.
    assert_eq!(portal.urls(), ["/v2/ad60/registre.html"]);
}

#[test]
fn the_oise_rows_have_a_parish_column_and_upper_case_localities() {
    let (target, portal) = ok(
        "AD60 - Exampleville - (aucun) - B - 1663 - 3E1/1 - vue 2/50",
        ad60_accepted,
    );
    assert_eq!(
        target.url(),
        "https://ressources.archives.oise.fr/v2/ad60/visualiseur/registre.html?id=600000101&vue=2"
    );
    assert_eq!(
        portal.urls()[1],
        "/v2/ad60/registre_liste.html?commune=600000001&acte=600000001&annee=1663&ajax=true"
    );

    let (target, _) = ok(
        "AD60 - Le Bourg-Exemple - Notre-Dame-Exemple - B - 1650",
        ad60_accepted,
    );
    assert!(target.url().ends_with("id=600000102"), "{target:?}");
    let (target, _) = ok(
        "AD60 - Le Bourg-Exemple - (aucun) - B - 1650",
        ad60_accepted,
    );
    assert!(target.url().ends_with("id=600000102"), "{target:?}");
    let (target, _) = ok(
        "AD60 - Saint-Exemple-sur-Mer - (aucun) - B - 1500",
        ad60_accepted,
    );
    assert_eq!(matches_of(&target), Some(0));
}

// -------------------------------------------------------------- seriel

fn ad62(request: &PortalRequest) -> Option<String> {
    assert_eq!(request.method, Method::Post, "{}", request.url);
    assert!(request.url.starts_with(AD62_ACTION), "{}", request.url);
    let body = request.body.as_deref().unwrap_or_default();
    let page = if request.url.ends_with("&r=0&page=1") {
        AD62_PAGE_1
    } else if request.url == AD62_ACTION {
        AD62_PAGE_0
    } else {
        return None;
    };
    let answer = if body.contains("f_0_3=5%20X%2010%2F4") {
        AD62_ONE
    } else if body.contains("f_0_3=") || body.contains("1500") {
        AD62_NONE
    } else if body.contains("Le%20Bourg-Exemple") {
        AD62_ARTICLE
    } else {
        page
    };
    Some(answer.to_owned())
}

#[test]
fn the_form_is_posted_with_the_portal_s_locality_label() {
    let (target, portal) = ok(
        "AD62 - Exampleville - (aucun) - B - 1747 - 5 X 10/4 - vue 7/300",
        ad62,
    );
    assert_eq!(
        target.url(),
        "https://archivesenligne.pasdecalais.fr/v2/ad62/visualiseur/etat_civil.html?id=300000104&vue=7"
    );
    let requests = portal.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].headers,
        [(
            "Content-Type".to_owned(),
            "application/x-www-form-urlencoded".to_owned()
        )]
    );
    assert_eq!(
        requests[0].body.as_deref(),
        Some(
            "f_0_0=Exampleville%20%28Pas-de-Calais%2C%20France%29&f_0_1_0=1747&f_0_1_1=1747&f_0_2_1=on&f_0_3=5%20X%2010%2F4"
        )
    );
}

#[test]
fn the_pages_are_read_until_the_citation_decides() {
    // Five registers over two pages, three of which cover 1747: no part of
    // the citation tells them apart.
    let (target, portal) = ok("AD62 - Exampleville - (aucun) - B - 1747", ad62);
    assert_eq!(matches_of(&target), Some(5));
    assert_eq!(
        portal.urls(),
        [AD62_ACTION.to_owned(), format!("{AD62_ACTION}&r=0&page=1")]
    );
    // Those two requests carry the same form, without a call number.
    let requests = portal.requests();
    assert_eq!(requests[0].body, requests[1].body);
    assert!(!requests[0].body.as_deref().unwrap().contains("f_0_3"));

    // The parish decides at the second page.
    let (target, portal) = ok(
        "AD62 - Exampleville - Saint-Exemple-le-Haut - B - 1747",
        ad62,
    );
    assert!(target.url().ends_with("id=300000104&vue=0") || target.url().ends_with("id=300000104"));
    assert_eq!(portal.urls().len(), 2);
}

#[test]
fn a_call_number_the_portal_does_not_know_is_searched_again_without_it() {
    let (target, portal) = ok("AD62 - Exampleville - (aucun) - B - 1747 - 5 X 10/9", ad62);
    assert_eq!(matches_of(&target), Some(5));
    let requests = portal.requests();
    assert_eq!(requests.len(), 3);
    assert!(requests[0].body.as_deref().unwrap().contains("f_0_3="));
    assert!(!requests[1].body.as_deref().unwrap().contains("f_0_3="));
}

#[test]
fn an_empty_search_and_an_article_are_read() {
    let (target, portal) = ok("AD62 - Exampleville - (aucun) - B - 1500", ad62);
    assert_eq!(matches_of(&target), Some(0));
    assert_eq!(portal.urls().len(), 1);

    let (target, portal) = ok("AD62 - Le Bourg-Exemple - (aucun) - N - 1927", ad62);
    assert!(target.url().ends_with("id=300000106"), "{target:?}");
    assert!(
        portal.requests()[0]
            .body
            .as_deref()
            .unwrap()
            .starts_with("f_0_0=Le%20Bourg-Exemple%20%28Pas-de-Calais%2C%20France%29&"),
    );
}

#[test]
fn a_challenge_is_reported_apart_from_a_changed_portal() {
    fn challenge(_: &PortalRequest) -> Option<String> {
        Some(AD62_CHALLENGE.to_owned())
    }
    let portal = Portal::new(challenge);
    assert_eq!(
        resolve("AD62 - Exampleville - (aucun) - B - 1747", &portal),
        Err(ResolveError::Challenged)
    );
    // The same for a form's page, whichever module reads it.
    let portal = Portal::new(challenge);
    assert_eq!(
        resolve("AD17 - Exampleville - (aucun) - B - 1620", &portal),
        Err(ResolveError::Challenged)
    );

    fn changed(_: &PortalRequest) -> Option<String> {
        Some("<html>maintenance</html>".to_owned())
    }
    let portal = Portal::new(changed);
    assert!(matches!(
        resolve("AD62 - Exampleville - (aucun) - B - 1747", &portal),
        Err(ResolveError::UnexpectedResponse(_))
    ));
}

// ------------------------------------------------------------------ ead

fn ad21(request: &PortalRequest) -> Option<String> {
    let url = request.url.as_str();
    if url == "/console/ir_ead_visu.php?eadid=FRAD021_000000912&ir=26564" {
        return Some(AD21_ROOT.to_owned());
    }
    let node = url.strip_prefix("/console/ir_ead_visu_action.php?ir=26564&id=")?;
    let body = match node {
        "400000010&toc=1" => AD21_TOC_ACTES,
        // The tables are listed on the act node itself.
        "400000020&toc=1" => "",
        toc if toc.ends_with("&toc=1") => AD21_TOC_COMMUNE,
        "400000011" => AD21_COMMUNALE,
        "400000012" => AD21_DEPARTEMENTALE,
        "400000020" => AD21_TABLES,
        _ => return None,
    };
    Some(body.to_owned())
}

#[test]
fn the_finding_aid_is_browsed_from_the_commune_to_its_register_block() {
    let (target, portal) = ok(
        "AD21 - Exampleville - (aucun) - B - 1650 - FRAD021EC 9/001 - vue 5/107",
        ad21,
    );
    assert_eq!(
        target,
        ArchiveTarget::View {
            url: "https://archives.cotedor.fr/v2/ad21/visualiseur/ir_ead_visu_lien.html?ir=26564&id=400000201&vue=5"
                .to_owned(),
            views: vec![ArchiveView {
                view: 5,
                url: "https://archives.cotedor.fr/v2/ad21/visualiseur/ir_ead_visu_lien.html?ir=26564&id=400000201&vue=5"
                    .to_owned(),
                ark: None,
                image: None,
            }],
            view_count: Some(107),
            call_number: Some("FRAD021EC 9/001".to_owned()),
            attribution: None,
            renumbering: None,
        }
    );
    // The aid's page, the commune's nodes, the act node's collections and
    // the first collection's notice, which decides: the second is not read.
    assert_eq!(
        portal.urls(),
        [
            "/console/ir_ead_visu.php?eadid=FRAD021_000000912&ir=26564",
            "/console/ir_ead_visu_action.php?ir=26564&id=400000001&toc=1",
            "/console/ir_ead_visu_action.php?ir=26564&id=400000010&toc=1",
            "/console/ir_ead_visu_action.php?ir=26564&id=400000011",
        ]
    );
}

#[test]
fn the_collections_are_read_in_turn_until_one_register_matches() {
    // 1745 is in no block: both collections are read, three registers with
    // images remain (a block without a viewer link has none).
    let (target, portal) = ok("AD21 - Exampleville - (aucun) - B - 1745", ad21);
    assert_eq!(matches_of(&target), Some(3));
    assert_eq!(portal.urls().len(), 5);

    // The period tells the registers of the first collection apart.
    let (target, _) = ok("AD21 - Exampleville - (aucun) - B - 1720", ad21);
    assert!(target.url().contains("id=400000202"), "{target:?}");
    // Both collections hold 1720, with the same image count: still two.
    let (target, _) = ok("AD21 - Exampleville - (aucun) - B - 1720 - vue 3/194", ad21);
    assert!(target.url().contains("id=400000202"), "{target:?}");
}

#[test]
fn a_node_without_children_is_its_own_notice() {
    let (target, portal) = ok("AD21 - Exampleville - (aucun) - TD - 1800", ad21);
    assert!(target.url().contains("id=400000205"), "{target:?}");
    assert_eq!(
        portal.urls()[2..],
        [
            "/console/ir_ead_visu_action.php?ir=26564&id=400000020&toc=1",
            "/console/ir_ead_visu_action.php?ir=26564&id=400000020"
        ]
    );
}

#[test]
fn the_commune_is_matched_whatever_the_page_s_decoding_and_article_order() {
    // The page is ISO-8859-1: decoded as UTF-8, `Châtel` reads `Ch\u{fffd}tel`.
    for (title, node) in [
        ("AD21 - Châtel-Exemple - (aucun) - B - 1650", "400000002"),
        ("AD21 - Chatel-Exemple - (aucun) - B - 1650", "400000002"),
        ("AD21 - L'Étang-Exemple - (aucun) - B - 1650", "400000003"),
        ("AD21 - Le Hameau-Exemple - (aucun) - B - 1650", "400000004"),
    ] {
        let (_, portal) = ok(title, ad21);
        assert_eq!(
            portal.urls()[1],
            format!("/console/ir_ead_visu_action.php?ir=26564&id={node}&toc=1"),
            "{title}"
        );
    }
    let (target, portal) = ok("AD21 - Nowhere - (aucun) - B - 1650", ad21);
    assert_eq!(matches_of(&target), Some(0));
    assert_eq!(portal.urls().len(), 1);
}

#[test]
fn a_view_beyond_the_block_s_image_count_opens_the_register() {
    let (target, _) = ok(
        "AD21 - Exampleville - (aucun) - B - 1650 - FRAD021EC 9/001 - vue 150/200",
        ad21,
    );
    let ArchiveTarget::View {
        url,
        views,
        view_count,
        ..
    } = target
    else {
        panic!("expected a view");
    };
    assert!(views.is_empty());
    assert_eq!(view_count, Some(107));
    assert_eq!(
        url,
        "https://archives.cotedor.fr/v2/ad21/visualiseur/ir_ead_visu_lien.html?ir=26564&id=400000201"
    );
}

#[test]
fn the_commune_index_is_kept_for_the_session_and_read_again_once_dated() {
    const ROOT: &str = "/console/ir_ead_visu.php?eadid=FRAD021_000000912&ir=26564";
    let adapter = platform::Archinoe::new();
    let portal = Portal::new(ad21);
    let title = "AD21 - Exampleville - (aucun) - B - 1650 - FRAD021EC 9/001 - vue 5/107";
    let first = resolve_with(&adapter, title, &portal).unwrap();
    // The aid's page is read once: the next lookups start at the commune.
    let second = resolve_with(&adapter, title, &portal).unwrap();
    assert_eq!(first, second);
    let roots = |portal: &Portal| portal.urls().iter().filter(|url| *url == ROOT).count();
    assert_eq!(roots(&portal), 1);
    assert_eq!(portal.urls().len(), 7);

    // The portal published the aid anew: the kept commune's node answers
    // nothing, and the page is read again within the same lookup.
    fn republished(request: &PortalRequest) -> Option<String> {
        if request.url.ends_with("id=400000001&toc=1") {
            return Some("Erreur #3002: Section non trouvée".to_owned());
        }
        let answer = ad21(request)?;
        Some(answer.replace("showEntry(400000001)", "showEntry(400000009)"))
    }
    let portal = Portal::new(republished);
    let target = resolve_with(&adapter, title, &portal).unwrap();
    assert_eq!(target.url(), first.url());
    assert_eq!(
        portal.urls()[..3],
        [
            "/console/ir_ead_visu_action.php?ir=26564&id=400000001&toc=1",
            ROOT,
            "/console/ir_ead_visu_action.php?ir=26564&id=400000009&toc=1",
        ]
    );
}

#[test]
fn a_changed_finding_aid_is_reported_as_drift() {
    fn no_commune(request: &PortalRequest) -> Option<String> {
        let answer = ad21(request)?;
        Some(answer.replace("javascript:showEntry(", "javascript:openEntry("))
    }
    let portal = Portal::new(no_commune);
    assert!(matches!(
        resolve("AD21 - Exampleville - (aucun) - B - 1650", &portal),
        Err(ResolveError::UnexpectedResponse(_))
    ));
}

// ------------------------------------------------------------- settings

fn registry_with(edit: impl Fn(&mut serde_json::Value)) -> Result<ArchiveRegistry, String> {
    let documents = [
        (
            "fr-ad17",
            include_str!("../../../../../assets/archives/fr/fr-ad17.json"),
        ),
        (
            "fr-ad62",
            include_str!("../../../../../assets/archives/fr/fr-ad62.json"),
        ),
        (
            "fr-ad21",
            include_str!("../../../../../assets/archives/fr/fr-ad21.json"),
        ),
    ];
    let edited: Vec<(&str, String)> = documents
        .iter()
        .map(|(_, text)| {
            let mut value: serde_json::Value = serde_json::from_str(text).unwrap();
            edit(&mut value);
            ("fr", value.to_string())
        })
        .collect();
    let borrowed: Vec<(&str, &str)> = edited.iter().map(|(c, t)| (*c, t.as_str())).collect();
    ArchiveRegistry::new(&borrowed, platform::builtin()).map_err(|error| error.to_string())
}

fn portal_of(value: &mut serde_json::Value) -> &mut serde_json::Map<String, serde_json::Value> {
    value["collections"][0]["portal"].as_object_mut().unwrap()
}

#[test]
fn the_catalogued_settings_are_accepted() {
    registry_with(|_| {}).unwrap();
}

#[test]
fn unusable_settings_are_refused_at_load_time() {
    type Edit = fn(&mut serde_json::Map<String, serde_json::Value>);
    let edits: [(&str, Edit); 9] = [
        ("an unknown member", |portal| {
            portal.insert("unknown".into(), 1.into());
        }),
        ("an http origin", |portal| {
            portal.insert("origin".into(), "http://archives.example.org".into());
        }),
        ("a base with a slash", |portal| {
            portal.insert("base".into(), "/console/".into());
        }),
        ("a viewer without its id", |portal| {
            portal.insert("viewer".into(), "/v2/x/visualiseur.html?n=1".into());
        }),
        ("an act that is not a code", |portal| {
            portal["acts"]
                .as_object_mut()
                .unwrap()
                .insert("XX".into(), "1".into());
        }),
        ("a missing act value", |portal| {
            portal["acts"].as_object_mut().unwrap().remove("B");
        }),
        ("a member of another search", |portal| {
            portal.insert("ir".into(), "1".into());
        }),
        ("an unknown search", |portal| {
            portal.insert("search".into(), "other".into());
        }),
        ("an act value that is not an identifier", |portal| {
            portal["acts"]
                .as_object_mut()
                .unwrap()
                .insert("B".into(), "word word".into());
        }),
    ];
    for (name, edit) in edits {
        // Each edit is refused in at least the mode it was written for; the
        // other documents may take it.
        let refused = registry_with(|value| edit(portal_of(value))).is_err();
        assert!(refused, "{name} was accepted");
    }
}

#[test]
fn each_search_needs_its_own_members() {
    let without = |archive: &'static str, member: &'static str| {
        registry_with(move |value| {
            if value["id"] == archive {
                portal_of(value).remove(member);
            }
        })
    };
    assert!(without("fr-ad62", "form").is_err());
    assert!(without("fr-ad62", "locality_label").is_err());
    assert!(without("fr-ad21", "eadid").is_err());
    assert!(without("fr-ad17", "fields").is_err());
    assert!(without("fr-ad17", "viewer").is_err());
}

#[test]
fn the_search_page_is_the_results_page() {
    let registry = ArchiveRegistry::embedded();
    for (title, url) in [
        (
            "AD17 - Exampleville - (aucun) - B - 1620",
            "https://archinoe.com/v2/ad17/registre.html",
        ),
        (
            "AD60 - Exampleville - (aucun) - B - 1663",
            "https://ressources.archives.oise.fr/v2/ad60/registre.html",
        ),
        (
            "AD62 - Exampleville - (aucun) - B - 1747",
            "https://archivesenligne.pasdecalais.fr/console/ir_seriel.php?id=56&p=formulaire_etat_civil",
        ),
        (
            "AD21 - Exampleville - (aucun) - B - 1650",
            "https://archives.cotedor.fr/console/ir_ead_visu.php?eadid=FRAD021_000000912&ir=26564",
        ),
    ] {
        let citation = registry.parse(title).unwrap();
        assert_eq!(
            registry.offline_target(&citation),
            Ok(ArchiveTarget::Results {
                url: url.to_owned(),
                matches: None
            }),
            "{title}"
        );
    }
}

#[test]
fn only_the_portals_that_redirect_other_clients_need_a_browser() {
    let registry = ArchiveRegistry::embedded();
    for (code, browser) in [
        ("AD17", true),
        ("AD60", true),
        ("AD62", false),
        ("AD21", false),
    ] {
        let archive = registry.archive(code).unwrap();
        let collection = &archive.collections[0];
        let endpoint = registry
            .platform("archinoe")
            .unwrap()
            .endpoint(collection)
            .unwrap();
        assert_eq!(endpoint.access == crate::Access::Browser, browser, "{code}");
        assert!(endpoint.other_origins.is_empty());
    }
}
