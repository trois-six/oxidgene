//! The "salle virtuelle" of the Côtes-d'Armor archives, an ASP.NET
//! WebForms application (its windows are named `VisualysInfo` and
//! `VisualysConsult`) serving one site per family of documents under one
//! origin: `/EC/ecx` for the parish and civil registers, `/RM/rmx` for the
//! military registers.
//!
//! A visitor enters a site through its home page's "Entrer"
//! (`connexion.aspx?ref=demo`), which opens a session and shows the
//! archive's reuse licence (`licence.aspx`); accepting it leads to the
//! site's search page. The portal does not enforce the licence — its pages
//! answer a session that never accepted it — but the reader must pass it:
//! OxidGene never accepts it on the reader's behalf, nor opens a page
//! beyond it. The adapter searches as a visitor whose licence is pending,
//! to tell whether the cited register is there, and its target is the
//! site's entry, where the reader passes the licence and goes on to the
//! register. A site finds its registers in one of two ways, its `mode`:
//!
//! - `localities`: the alphabetical list of localities, then a locality's
//!   lots of images, in two blocks the session opens and closes;
//! - `search`: a form of criteria posted back with the page's state.
//!
//! Archive Portals §4.12 specifies the requests.

#[cfg(any(test, feature = "live"))]
mod live;
mod page;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use serde::Deserialize;

use super::locality::{LocalityStyle, forms};
use super::markup::{self, fold};
use super::select::{Candidate, narrow};
use super::view::view_target;
use super::{Access, BoxFuture, Platform, PortalEndpoint, Query, is_https_origin, refuse_series};
use crate::catalog::{Archive, CatalogError, Collection};
use crate::citation::{Act, ActKind, CitationParts};
use crate::transport::{PortalFetch, PortalRequest};
use crate::{ArchiveTarget, ResolveError};
use page::{Blocks, Locality};

/// The Visualys adapter.
pub struct Visualys;

/// The most rows of the alphabetical list read for one cited locality: a
/// town and the parishes or bodies listed apart under its name.
const MAX_LOCALITIES: usize = 3;

/// The first year of the civil status: earlier marriages are in the parish
/// registers' block.
const FIRST_CIVIL_YEAR: u16 = 1793;

fn invalid(message: &str) -> CatalogError {
    CatalogError::new(format!("visualys settings: {message}"))
}

async fn get(fetch: &dyn PortalFetch, path: &str) -> Result<String, ResolveError> {
    let body = fetch.get(path).await?;
    if markup::is_challenge(&body) {
        Err(ResolveError::Challenged)
    } else {
        Ok(body)
    }
}

/// How a site finds its registers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Mode {
    /// The alphabetical list of localities, then a locality's lots.
    Localities,
    /// A form of criteria: the year, the office, the kind of register.
    Search,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    origin: String,
    #[serde(default)]
    transport: Access,
    base: String,
    mode: Mode,
    #[serde(default)]
    locality_style: LocalityStyle,
    #[serde(default)]
    acts: BTreeMap<String, String>,
}

/// The settings of one collection, checked.
#[derive(Debug, Clone)]
struct Settings {
    origin: String,
    transport: Access,
    /// The site's path: `/EC/ecx`.
    base: String,
    mode: Mode,
    locality_style: LocalityStyle,
    /// `search` mode: the kind-of-register select's value of each series.
    acts: BTreeMap<String, String>,
}

/// Whether `base` is a site's path, `/<family>/<site>`.
fn is_site_path(base: &str) -> bool {
    let mut parts = base.split('/');
    parts.next() == Some("")
        && parts.clone().count() == 2
        && parts.all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        })
}

impl Settings {
    fn read(collection: &Collection) -> Result<Self, CatalogError> {
        let raw =
            Raw::deserialize(&collection.portal).map_err(|error| invalid(&error.to_string()))?;
        if !is_https_origin(&raw.origin) {
            return Err(invalid("origin must be an https origin"));
        }
        if !is_site_path(&raw.base) {
            return Err(invalid("base must be a site's path, `/<family>/<site>`"));
        }
        for code in raw.acts.keys() {
            if Act::from_code(code).is_none() {
                return Err(invalid(&format!("`{code}` is not a document kind's code")));
            }
        }
        match raw.mode {
            Mode::Localities => {
                refuse_series("visualys", collection)?;
                if !raw.acts.is_empty() {
                    return Err(invalid("acts belong to the `search` mode"));
                }
            }
            Mode::Search => {
                if raw.locality_style != LocalityStyle::Plain {
                    return Err(invalid("locality_style belongs to the `localities` mode"));
                }
                if let Some(act) = collection
                    .acts
                    .iter()
                    .find(|act| !raw.acts.contains_key(&act.to_string()))
                {
                    return Err(invalid(&format!("no value for `{act}`")));
                }
            }
        }
        Ok(Self {
            origin: raw.origin,
            transport: raw.transport,
            base: raw.base,
            mode: raw.mode,
            locality_style: raw.locality_style,
            acts: raw.acts,
        })
    }

    /// The site's entry for a visitor, the home page's "Entrer": it opens a
    /// session and shows the reuse licence, after which the reader reaches
    /// the site's search page. Every target of the site.
    fn entry(&self) -> String {
        format!("{}{}", self.origin, self.entry_path())
    }

    fn entry_path(&self) -> String {
        format!("{}/connexion.aspx?ref=demo&res=1920x1080", self.base)
    }

    /// The search page of a `search` site, and the list of a `localities`
    /// site's initial.
    fn search_path(&self, initial: char) -> String {
        format!("{}/commune.aspx?lettre={initial}", self.base)
    }
}

/// The initial under which the alphabetical list files a locality written
/// as the portal writes it: `B` for `Bourg (Le)`, `E` for `Étables`.
fn initial(listed: &str) -> char {
    fold(listed)
        .chars()
        .find(char::is_ascii_alphabetic)
        .map_or('*', |letter| letter.to_ascii_uppercase())
}

/// The blocks of a locality's lots holding the cited act: the parish
/// registers' (baptisms, burials, and the marriages they hold) and the civil
/// status' (births, deaths, tables, and the marriages it holds). A marriage
/// cited alone is in the parish registers before 1793 and in the civil
/// status from then on; without a year it is looked for in both.
fn wanted_blocks(act: &Act, year: Option<u16>) -> (bool, bool) {
    let Act::Register(kinds) = act else {
        return (false, true);
    };
    let parish = kinds
        .iter()
        .any(|kind| matches!(kind, ActKind::Baptism | ActKind::Burial));
    let civil = kinds
        .iter()
        .any(|kind| matches!(kind, ActKind::Birth | ActKind::Death));
    if parish || civil {
        return (parish, civil);
    }
    (
        year.is_none_or(|year| year < FIRST_CIVIL_YEAR),
        year.is_none_or(|year| year >= FIRST_CIVIL_YEAR),
    )
}

/// The lots page of a locality with the blocks holding the cited act open.
/// The session keeps each block open or closed, and the page's links toggle
/// them (`&r=0`, `&r=1`), redirecting to the page: a block is toggled only
/// when it is wanted and closed.
async fn lots_page(
    settings: &Settings,
    locality: &Locality,
    citation: &CitationParts,
    fetch: &dyn PortalFetch,
) -> Result<String, ResolveError> {
    let path = format!("{}/plage.aspx?id={}", settings.base, locality.id);
    let mut page = get(fetch, &path).await?;
    let (parish, civil) = wanted_blocks(&citation.act, citation.year);
    for (wanted, block) in [(parish, 0), (civil, 1)] {
        let Blocks {
            parish: parish_open,
            civil: civil_open,
        } = page::blocks(&page)?;
        let open = if block == 0 { parish_open } else { civil_open };
        if wanted && open == Some(false) {
            page = get(fetch, &format!("{path}&r={block}")).await?;
        }
    }
    Ok(page)
}

/// The rows of the alphabetical list naming the cited locality: those of
/// the cited parish when the citation names one and a row is listed for it,
/// otherwise the locality's own row (listed without a parish), otherwise
/// every row of its name.
fn chosen_localities<'l>(
    settings: &Settings,
    listed: &'l [Locality],
    citation: &CitationParts,
    localities: &[&str],
) -> Vec<&'l Locality> {
    let wanted: Vec<String> = localities.iter().map(|locality| fold(locality)).collect();
    let named: Vec<&Locality> = listed
        .iter()
        .filter(|row| wanted.contains(&fold(&settings.locality_style.cited(&row.name))))
        .collect();
    let parish = citation.parish.as_deref().map(fold);
    let of_parish: Vec<&Locality> = named
        .iter()
        .copied()
        .filter(|row| parish.is_some() && row.parish.as_deref().map(fold) == parish)
        .collect();
    if !of_parish.is_empty() {
        return of_parish;
    }
    let own: Vec<&Locality> = named
        .iter()
        .copied()
        .filter(|row| row.parish.is_none())
        .collect();
    if own.is_empty() { named } else { own }
}

/// What a search found: the registers read, with their lots, how many rows of the list
/// named the locality when none was read, and whether the registers are
/// those of the cited locality or, for a locality naming no office of a
/// search by criteria, of every office.
struct Found {
    candidates: Vec<Candidate<String>>,
    total: usize,
    anywhere: bool,
}

/// `localities` mode: the visitor's session, the initial's list, then the
/// lots of each row naming the locality.
async fn find_in_localities(
    settings: &Settings,
    citation: &CitationParts,
    localities: &[&str],
    fetch: &dyn PortalFetch,
) -> Result<Found, ResolveError> {
    get(fetch, &settings.entry_path()).await?;
    let listed_as = settings.locality_style.write(&citation.locality);
    let list = get(fetch, &settings.search_path(initial(&listed_as))).await?;
    let listed = page::localities(&list)?;
    let rows = chosen_localities(settings, &listed, citation, localities);
    if rows.is_empty() || rows.len() > MAX_LOCALITIES {
        return Ok(Found {
            candidates: Vec::new(),
            total: rows.len(),
            anywhere: false,
        });
    }
    let mut candidates = Vec::new();
    for row in rows {
        let page = lots_page(settings, row, citation, fetch).await?;
        candidates.extend(page::lots(&page).into_iter().map(|lot| Candidate {
            locality: Some(settings.locality_style.cited(&row.name)),
            call_number: None,
            act: Some(lot.act),
            parish: row.parish.clone(),
            period: Some(lot.period),
            images: lot.images,
            numbers: None,
            payload: lot.id,
        }));
    }
    Ok(Found {
        candidates,
        total: 0,
        anywhere: false,
    })
}

/// `search` mode: the visitor's session, the search page, then the form
/// posted with the year (a class), the office when the cited locality is
/// one, and the kind of register. The portal requires a year or an office.
async fn find_by_criteria(
    settings: &Settings,
    citation: &CitationParts,
    localities: &[&str],
    fetch: &dyn PortalFetch,
) -> Result<Option<Found>, ResolveError> {
    get(fetch, &settings.entry_path()).await?;
    let search = get(fetch, &settings.search_path('*')).await?;
    let form = page::form(&search, &settings.base)?;
    let offices = page::options(&search, "lstBureau");
    let wanted: Vec<String> = localities.iter().map(|locality| fold(locality)).collect();
    let office = offices
        .iter()
        .find(|office| wanted.contains(&fold(office)))
        .cloned();
    let kind = settings
        .acts
        .get(&citation.act.to_string())
        .ok_or(ResolveError::NoAdapter)?;
    if !page::options(&search, "lstRegistre").contains(kind) {
        return Err(ResolveError::UnexpectedResponse(
            "visualys: the search form does not offer the kind of register".to_owned(),
        ));
    }
    let year = citation
        .year
        .map(|year| year.to_string())
        .filter(|year| page::options(&search, "lstAnnee1").contains(year));
    if year.is_none() && office.is_none() {
        return Ok(None);
    }
    let mut query = Query::new();
    for (name, value) in &form.hidden {
        query.push(name.as_str(), value.as_str());
    }
    query
        .push("lstAnnee1", year.unwrap_or_default())
        .push("lstAnnee2", "")
        .push("lstBureau", office.clone().unwrap_or_default())
        .push("lstRegistre", kind.as_str())
        .push("btnFind", "Rechercher");
    let request = PortalRequest::post(
        &form.action,
        "application/x-www-form-urlencoded",
        query.to_string(),
    );
    let answer = fetch.request(&request).await?;
    if markup::is_challenge(&answer) {
        return Err(ResolveError::Challenged);
    }
    let candidates = page::volumes(&answer)?
        .into_iter()
        .map(|volume| Candidate {
            locality: Some(volume.office),
            call_number: volume.call_number,
            act: None,
            parish: None,
            period: Some(volume.year),
            images: None,
            numbers: None,
            payload: volume.lot,
        })
        .collect();
    Ok(Some(Found {
        candidates,
        total: 0,
        anywhere: office.is_none(),
    }))
}

impl Platform for Visualys {
    fn id(&self) -> &'static str {
        "visualys"
    }

    fn validate(&self, collection: &Collection) -> Result<(), CatalogError> {
        Settings::read(collection).map(drop)
    }

    fn endpoint(&self, collection: &Collection) -> Option<PortalEndpoint> {
        let settings = Settings::read(collection).ok()?;
        // The sites' shared stylesheet: a page of the origin that opens no
        // session and navigates nowhere.
        let family = &settings.base[..settings.base.rfind('/').unwrap_or_default()];
        Some(PortalEndpoint {
            start: format!("{}{family}/slv.css", settings.origin),
            origin: settings.origin,
            other_origins: Vec::new(),
            access: settings.transport,
        })
    }

    fn results_url(&self, collection: &Collection, _citation: &CitationParts) -> Option<String> {
        // Every page lies beyond the reuse licence, which the reader passes
        // from the site's entry.
        Some(Settings::read(collection).ok()?.entry())
    }

    fn resolve<'a>(
        &'a self,
        archive: &'a Archive,
        collection: &'a Collection,
        citation: &'a CitationParts,
        fetch: &'a dyn PortalFetch,
    ) -> BoxFuture<'a, Result<ArchiveTarget, ResolveError>> {
        Box::pin(resolve(archive, collection, citation, fetch))
    }
}

async fn resolve(
    archive: &Archive,
    collection: &Collection,
    citation: &CitationParts,
    fetch: &dyn PortalFetch,
) -> Result<ArchiveTarget, ResolveError> {
    let settings = Settings::read(collection).map_err(|_| ResolveError::NoAdapter)?;
    let wanted = forms(&citation.locality);
    let mut localities: Vec<&str> = wanted.iter().map(String::as_str).collect();
    let found = match settings.mode {
        Mode::Localities => find_in_localities(&settings, citation, &localities, fetch).await?,
        Mode::Search => match find_by_criteria(&settings, citation, &localities, fetch).await? {
            Some(found) => found,
            // Neither a year the form lists nor an office: no search.
            None => {
                return Ok(ArchiveTarget::Results {
                    url: settings.entry(),
                    matches: None,
                });
            }
        },
    };
    // A locality naming no office (a commune, a department) keeps the
    // registers of every office.
    if found.anywhere {
        localities = vec![""];
    }
    let matches = match narrow(&found.candidates, citation, &localities).as_slice() {
        // The register is there: the reader opens it from the site's entry,
        // past the licence, the image count and the view to go to known.
        [only] => {
            return Ok(view_target(
                archive,
                citation,
                only.call_number.as_deref(),
                only.images.map_or(usize::MAX, usize::from),
                settings.entry(),
                Vec::new(),
            ));
        }
        [] if found.candidates.is_empty() => found.total,
        many => many.len(),
    };
    Ok(ArchiveTarget::Results {
        url: settings.entry(),
        matches: Some(matches),
    })
}
