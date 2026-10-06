//! The adapter over anonymized pages of the overseas civil-status search
//! (`fixtures/caomec2/`, written by `generate.py`), with the catalogue's own
//! settings.

use std::sync::Mutex;

use super::{Caomec2, page, portal_form};
use crate::catalog::Collection;
use crate::live::Probe;
use crate::platform::{BoxFuture, Platform};
use crate::recognize::CitationEvidence;
use crate::tests::block_on;
use crate::transport::{FetchError, PortalFetch, PortalRequest};
use crate::{Act, ArchiveRegistry, ArchiveTarget, ResolveError};

macro_rules! fixture {
    ($name:literal) => {
        include_str!(concat!("../../../fixtures/caomec2/", $name))
    };
}

const FORM: &str = fixture!("form.html");
const ONE: &str = fixture!("results-one.html");
const YEAR: &str = fixture!("results-year.html");
const SEVERAL: &str = fixture!("results-several.html");
const APOSTROPHE: &str = fixture!("results-apostrophe.html");
const NONE: &str = fixture!("results-none.html");
const VIEWER: &str = fixture!("viewer.html");

const ORIGIN: &str = "http://anom.archivesnationales.culture.gouv.fr";
const GUYANE_FORM: &str = "/caomec2/recherche.php?territoire=GUYANE";

/// A portal answering each `GET` by its address.
struct Portal {
    routes: Vec<(String, &'static str)>,
    requests: Mutex<Vec<String>>,
}

impl Portal {
    fn new(routes: &[(&str, &'static str)]) -> Self {
        Self {
            routes: routes
                .iter()
                .map(|(path, body)| ((*path).to_owned(), *body))
                .collect(),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn paths(&self) -> Vec<String> {
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
                .find(|(path, _)| *path == request.url)
                .map(|(_, body)| (*body).to_owned())
                .ok_or(FetchError::Status(404))
        })
    }
}

/// The search of `commune` in Guyane, every kind of act, in `year`.
fn search(commune: &str, year: &str) -> String {
    format!(
        "/caomec2/resultats.php?territoire=GUYANE&commune={commune}&typeacte=&theme=&annee={year}&debut=&fin=&vue="
    )
}

fn collection(id: &str) -> &'static Collection {
    ArchiveRegistry::embedded()
        .archive("ANOM")
        .unwrap()
        .collections
        .iter()
        .find(|collection| collection.id == id)
        .unwrap()
}

fn resolve_in(id: &str, title: &str, portal: &Portal) -> Result<ArchiveTarget, ResolveError> {
    let registry = ArchiveRegistry::embedded();
    let citation = registry.parse(title).expect("a normalized citation");
    let archive = registry.archive(&citation.code).expect("a catalogued code");
    block_on(Caomec2.resolve(archive, collection(id), &citation, portal))
}

fn resolve(title: &str, portal: &Portal) -> Result<ArchiveTarget, ResolveError> {
    resolve_in("guyane", title, portal)
}

/// The register's viewer, which opens on its first view.
fn register(query: &str) -> ArchiveTarget {
    ArchiveTarget::View {
        url: format!("{ORIGIN}/caomec2/osd.php?territoire=GUYANE&{query}"),
        views: Vec::new(),
        view_count: None,
        call_number: None,
        attribution: None,
        renumbering: None,
    }
}

#[test]
fn finds_the_register_of_every_act_of_a_commune_s_year() {
    let portal = Portal::new(&[(GUYANE_FORM, FORM), (&search("EXAMPLEVILLE", "1850"), ONE)]);
    assert_eq!(
        resolve(
            "ANOM - Exampleville - (aucun) - N - 1850 - vue 5/7",
            &portal
        ),
        Ok(register("commune=EXAMPLEVILLE&annee=1850"))
    );
    // The form, then the search: never the viewer.
    assert_eq!(portal.paths().len(), 2);
}

#[test]
fn tells_the_kinds_of_a_year_apart_by_their_labels() {
    let portal = Portal::new(&[
        (GUYANE_FORM, FORM),
        (&search("SAINT-EXEMPLE", "1852"), YEAR),
    ]);
    // A death, its label's accents lost to the portal's encoding.
    assert_eq!(
        resolve("ANOM - Saint-Exemple - (aucun) - D - 1852", &portal),
        Ok(register("commune=SAINT-EXEMPLE&annee=1852&typeacte=AC_DE"))
    );
    // A baptism is filed as a birth, a burial as a death.
    assert_eq!(
        resolve("ANOM - Saint-Exemple - (aucun) - B - 1852", &portal),
        Ok(register("commune=SAINT-EXEMPLE&annee=1852&typeacte=AC_NA"))
    );
    assert_eq!(
        resolve("ANOM - Saint-Exemple - (aucun) - M - 1852", &portal),
        Ok(register("commune=SAINT-EXEMPLE&annee=1852&typeacte=AC_MA"))
    );
    // A combined register none of the year's holds is not there.
    let Ok(ArchiveTarget::Results { url, matches }) =
        resolve("ANOM - Saint-Exemple - (aucun) - BMS - 1852", &portal)
    else {
        panic!("the results");
    };
    assert_eq!(matches, Some(0));
    assert_eq!(url, format!("{ORIGIN}{}", search("SAINT-EXEMPLE", "1852")));
}

#[test]
fn several_years_give_the_results() {
    let portal = Portal::new(&[(GUYANE_FORM, FORM), (&search("EXAMPLEVILLE", ""), SEVERAL)]);
    assert_eq!(
        resolve("ANOM - Exampleville - (aucun) - N - acte 3", &portal),
        Ok(ArchiveTarget::Results {
            url: format!("{ORIGIN}{}", search("EXAMPLEVILLE", "")),
            matches: Some(3),
        })
    );
}

#[test]
fn a_commune_without_registers_or_unlisted_gives_no_match() {
    let portal = Portal::new(&[(GUYANE_FORM, FORM), (&search("EXAMPLEVILLE", "1999"), NONE)]);
    assert_eq!(
        resolve("ANOM - Exampleville - (aucun) - N - 1999", &portal),
        Ok(ArchiveTarget::Results {
            url: format!("{ORIGIN}{}", search("EXAMPLEVILLE", "1999")),
            matches: Some(0),
        })
    );
    let portal = Portal::new(&[(GUYANE_FORM, FORM)]);
    assert_eq!(
        resolve("ANOM - Nowhere - (aucun) - N - 1850", &portal),
        Ok(ArchiveTarget::Results {
            url: format!("{ORIGIN}{GUYANE_FORM}"),
            matches: Some(0),
        })
    );
    assert_eq!(portal.paths(), [GUYANE_FORM]);
}

#[test]
fn searches_a_commune_as_the_form_lists_it() {
    let portal = Portal::new(&[
        (GUYANE_FORM, FORM),
        (&search("L%27EXEMPLE-SUR-MER", "1890"), APOSTROPHE),
    ]);
    assert_eq!(
        resolve("ANOM - L'Exemple-sur-Mer - (aucun) - M - 1890", &portal),
        Ok(register("commune=L%27EXEMPLE-SUR-MER&annee=1890"))
    );
    // A leading article written behind the name, as in a gazetteer.
    let portal = Portal::new(&[
        (GUYANE_FORM, FORM),
        (&search("LE%20BOURG-EXEMPLE", "1890"), NONE),
    ]);
    assert_eq!(
        resolve("ANOM - Le Bourg-Exemple - (aucun) - M - 1890", &portal)
            .map(|target| target.url().to_owned()),
        Ok(format!("{ORIGIN}{}", search("LE%20BOURG-EXEMPLE", "1890")))
    );
}

#[test]
fn a_territory_s_code_leaves_the_other_territories_unsearched() {
    let portal = Portal::new(&[]);
    assert_eq!(
        resolve("ANOM974 - Saint-Exemple - (aucun) - N - 1852", &portal),
        Ok(ArchiveTarget::Results {
            url: format!("{ORIGIN}{GUYANE_FORM}"),
            matches: None,
        })
    );
    assert!(portal.paths().is_empty());

    let portal = Portal::new(&[("/caomec2/recherche.php?territoire=REUNION", FORM)]);
    let _ = resolve_in(
        "reunion",
        "ANOM974 - Saint-Exemple - (aucun) - N - 1852",
        &portal,
    );
    assert_eq!(
        portal.paths()[0],
        "/caomec2/recherche.php?territoire=REUNION"
    );
}

#[test]
fn a_changed_shape_is_drift() {
    let portal = Portal::new(&[(GUYANE_FORM, "<html><body>Maintenance</body></html>")]);
    assert!(matches!(
        resolve("ANOM - Exampleville - (aucun) - N - 1850", &portal),
        Err(ResolveError::UnexpectedResponse(_))
    ));
    let portal = Portal::new(&[(GUYANE_FORM, FORM), (&search("EXAMPLEVILLE", "1850"), FORM)]);
    assert!(matches!(
        resolve("ANOM - Exampleville - (aucun) - N - 1850", &portal),
        Err(ResolveError::UnexpectedResponse(_))
    ));
}

#[test]
fn builds_the_filtered_search_without_a_request() {
    let registry = ArchiveRegistry::embedded();
    let citation = registry
        .parse("ANOM - Saint-Étienne-d'Exemple - (aucun) - N - 1850")
        .unwrap();
    assert_eq!(
        portal_form("Saint-Étienne-d'Exemple"),
        "SAINT-ETIENNE-D'EXEMPLE"
    );
    assert_eq!(
        Caomec2.results_url(collection("guyane"), &citation),
        Some(format!(
            "{ORIGIN}{}",
            search("SAINT-ETIENNE-D%27EXEMPLE", "1850")
        ))
    );
}

#[test]
fn reaches_its_portal_over_plain_http_as_the_catalogue_s_exception() {
    let endpoint = Caomec2.endpoint(collection("mayotte")).unwrap();
    assert_eq!(endpoint.origin, ORIGIN);
    assert!(endpoint.insecure_http);
    assert_eq!(
        endpoint.start,
        format!("{ORIGIN}/caomec2/recherche.php?territoire=MAYOTTE")
    );
    assert_eq!(crate::transport::check_scheme(&endpoint), Ok(()));
}

#[test]
fn refuses_unusable_settings() {
    let base = collection("guyane").clone();
    let refused = |change: fn(&mut Collection)| {
        let mut collection = base.clone();
        change(&mut collection);
        Caomec2.validate(&collection).unwrap_err().to_string()
    };
    assert!(refused(|c| c.insecure_http = false).contains("origin"));
    assert!(refused(|c| c.portal["territory"] = "Guyane".into()).contains("territory"));
    assert!(refused(|c| c.acts.push(Act::from_code("TD").unwrap())).contains("`TD`"));
    assert!(refused(|c| c.acts.push(Act::from_code("RM").unwrap())).contains("series"));
    assert!(refused(|c| c.portal["engine"] = "x".into()).contains("unknown field"));
    assert_eq!(Caomec2.validate(&base), Ok(()));
}

#[test]
fn reads_the_pages() {
    assert_eq!(
        page::communes(FORM).unwrap(),
        [
            "EXAMPLEVILLE",
            "EXAMPLEVILLE (HOPITAL)",
            "L'EXEMPLE-SUR-MER",
            "LE BOURG-EXEMPLE",
            "SAINT-EXEMPLE"
        ]
    );
    assert!(page::has_search_fields(FORM));
    let rows = page::results(YEAR).unwrap();
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[3].typeacte.as_deref(), Some("AC_DE"));
    assert_eq!(rows[3].year, "1852");
    assert!(page::results(NONE).unwrap().is_empty());
    assert_eq!(page::image_count(VIEWER), Ok(7));
    assert!(page::image_count(FORM).is_err());
}

#[test]
fn recognizes_the_archive_as_genealogists_name_it() {
    let registry = ArchiveRegistry::embedded();
    for text in [
        "ANOM, état civil, Exampleville, naissances 1850, vue 5",
        "ANOM 974, Saint-Exemple, décès 1852",
        "CAOM, état civil de Exampleville, mariages 1890",
        "Archives nationales d'outre-mer, Exampleville, naissances 1850",
        "Arch. nat. d'outre-mer, Exampleville, naissances 1850",
    ] {
        let evidence = CitationEvidence {
            title: text.to_owned(),
            ..CitationEvidence::default()
        };
        let recognition = registry.recognize(&evidence, None, None).expect(text);
        assert_eq!(recognition.archive.id, "fr-anom", "{text}");
    }
}

#[test]
fn probes_the_form_the_registers_and_their_count() {
    let collection = collection("guyane");
    let portal = Portal::new(&[
        (GUYANE_FORM, FORM),
        (&search("EXAMPLEVILLE", ""), SEVERAL),
        (
            "/caomec2/osd.php?territoire=GUYANE&commune=EXAMPLEVILLE&annee=1717",
            VIEWER,
        ),
    ]);
    // The first commune that is no hospital.
    let locality = block_on(Caomec2.search_page(collection, &portal)).unwrap();
    assert_eq!(locality, "EXAMPLEVILLE");
    let act = Act::from_code("N").unwrap();
    let registers = block_on(Caomec2.registers(collection, &locality, &act, &portal)).unwrap();
    assert_eq!(registers.len(), 3);
    assert_eq!(registers[0].period.as_deref(), Some("1717"));
    assert_eq!(
        block_on(Caomec2.images(collection, &registers[0], &portal)),
        Ok(Some(7))
    );
    assert!(!Caomec2.addresses_views(collection));
}
