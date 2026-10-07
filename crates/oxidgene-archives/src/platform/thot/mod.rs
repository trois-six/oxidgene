//! THOT, an ASP portal of framesets ("THOT Internet"), run by the
//! Ille-et-Vilaine and Corsica archives.
//!
//! Each family of documents is a module (`MOD`) of the portal's documentary
//! search, a form of criteria posted back to itself: a locality chosen from
//! the form's list, a type of document, years. The portal keeps the module
//! and the search in its ASP session, which it opens only for a client that
//! passes its cookie check. Each result row is a register with its call
//! number and a link to a Zoomify viewer fed by a per-register slide file;
//! where the archive publishes them (Ille-et-Vilaine), the slide file gives
//! each view's ARK, whose resolver opens the register at that view with no
//! session. Archive Portals §4.9 specifies the requests.

#[cfg(any(test, feature = "live"))]
mod live;
mod page;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use serde::Deserialize;

use super::locality::{forms, label_name, matching_labels};
use super::markup;
use super::query::encode;
use super::select::{Candidate, narrow};
use super::view::{cited_views, view_target};
use super::{Access, BoxFuture, Platform, PortalEndpoint, Query, is_https_origin};
use crate::catalog::{Archive, CatalogError, Collection};
use crate::citation::{Act, CitationParts};
use crate::transport::{PortalFetch, PortalRequest};
use crate::{ArchiveTarget, ArchiveView, ResolveError};
use page::{Form, Lot, Row};

/// The THOT adapter.
pub struct Thot;

/// The most labels of the locality list searched for one cited locality
/// (`EXAMPLEVILLE (BUREAU DE L'ENREGISTREMENT)` and `EXAMPLEVILLE
/// (SUBDIVISION MILITAIRE)` beside no plain `EXAMPLEVILLE`).
const MAX_LABELS: usize = 3;

/// The most result pages read, of 40 rows each.
const MAX_PAGES: usize = 5;

fn invalid(message: &str) -> CatalogError {
    CatalogError::new(format!("thot settings: {message}"))
}

fn unexpected(detail: &str) -> ResolveError {
    ResolveError::UnexpectedResponse(format!("thot: {detail}"))
}

/// A body that is not an anti-bot challenge, which is reported as such and
/// not as a change of the portal's shape.
fn unchallenged(body: String) -> Result<String, ResolveError> {
    if markup::is_challenge(&body) {
        Err(ResolveError::Challenged)
    } else {
        Ok(body)
    }
}

async fn get(fetch: &dyn PortalFetch, path: &str) -> Result<String, ResolveError> {
    unchallenged(fetch.get(path).await?)
}

/// How a module's form takes the years.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Years {
    /// `txt_CIN_DEX_D` and `txt_CIN_DEX_F`, the first and last year.
    Dex,
    /// The interval criterion `k`: `txt_CIN_CH<k>` = `<first>|<last>`, from
    /// the inputs `intervalleDate1_<k>` and `intervalleDate2_<k>`.
    Interval(u8),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawYears {
    Name(String),
    Criterion(u8),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCriteria {
    locality: u8,
    #[serde(default)]
    act: Option<u8>,
    #[serde(default)]
    year: Option<RawYears>,
}

/// Where a register's views open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Views {
    /// Each view's ARK, from the slide file, opens the register at that view.
    Ark,
    /// The viewer page of the register, in the session that searched it, on
    /// its first view: the portal has no address per view.
    Register,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    origin: String,
    #[serde(default)]
    transport: Access,
    base: String,
    module: u16,
    criteria: RawCriteria,
    #[serde(default)]
    acts: BTreeMap<String, String>,
    views: Views,
}

/// The settings of one collection, checked.
#[derive(Debug, Clone)]
struct Settings {
    origin: String,
    transport: Access,
    /// The portal's path: `/thot_internet`.
    base: String,
    module: u16,
    /// The locality criterion's number.
    locality: u8,
    /// The type-of-document criterion's number, with the value of each
    /// document kind in `acts`.
    act: Option<u8>,
    /// How the form takes the years; `None` where the module's year
    /// criterion does not find a register by a year it holds (the tables of
    /// successions, whose dates are a register's extreme dates), the years
    /// then told apart by the rows' periods.
    years: Option<Years>,
    acts: BTreeMap<String, String>,
    views: Views,
}

/// A value the portal's form posts: printable ASCII, since the portal reads
/// its forms as windows-1252.
fn is_form_value(text: &str) -> bool {
    !text.trim().is_empty() && text.bytes().all(|byte| (b' '..=b'~').contains(&byte))
}

impl Settings {
    fn read(collection: &Collection) -> Result<Self, CatalogError> {
        let raw =
            Raw::deserialize(&collection.portal).map_err(|error| invalid(&error.to_string()))?;
        let settings = Self::from_raw(raw)?;
        settings.check_acts(collection)?;
        Ok(settings)
    }

    fn from_raw(raw: Raw) -> Result<Self, CatalogError> {
        if !is_https_origin(&raw.origin) {
            return Err(invalid("origin must be an https origin"));
        }
        if !raw.base.starts_with('/')
            || raw.base.ends_with('/')
            || raw.base.contains(['?', '#', ' ', '\\'])
        {
            return Err(invalid(
                "base must be an absolute path without a trailing slash",
            ));
        }
        if raw.module == 0 {
            return Err(invalid("module must be a module number"));
        }
        let years = match raw.criteria.year {
            None => None,
            Some(RawYears::Name(name)) if name == "dex" => Some(Years::Dex),
            Some(RawYears::Criterion(criterion)) => Some(Years::Interval(criterion)),
            Some(RawYears::Name(_)) => {
                return Err(invalid("criteria.year is `dex` or a criterion"));
            }
        };
        let RawCriteria { locality, act, .. } = raw.criteria;
        let mut numbers = vec![locality];
        numbers.extend(act);
        if let Some(Years::Interval(criterion)) = years {
            numbers.push(criterion);
        }
        numbers.sort_unstable();
        if numbers.windows(2).any(|pair| pair[0] == pair[1]) || numbers.iter().any(|n| *n > 9) {
            return Err(invalid("criteria are distinct criterion numbers, 0 to 9"));
        }
        if act.is_none() != raw.acts.is_empty() {
            return Err(invalid(
                "acts are the values of criteria.act, given with it",
            ));
        }
        if !raw.acts.values().all(|value| is_form_value(value)) {
            return Err(invalid("acts are the form's printable ASCII values"));
        }
        Ok(Self {
            origin: raw.origin,
            transport: raw.transport,
            base: raw.base,
            module: raw.module,
            locality,
            act,
            years,
            acts: raw.acts,
            views: raw.views,
        })
    }

    /// Every act code is valid, and every document kind the collection
    /// holds has a value when the module has a type criterion.
    fn check_acts(&self, collection: &Collection) -> Result<(), CatalogError> {
        for code in self.acts.keys() {
            if Act::from_code(code).is_none() {
                return Err(invalid(&format!("`{code}` is not a document kind's code")));
            }
        }
        if self.act.is_none() {
            return Ok(());
        }
        match collection
            .acts
            .iter()
            .find(|act| self.act_value(act).is_none())
        {
            Some(act) => Err(invalid(&format!("no value for `{act}`"))),
            None => Ok(()),
        }
    }

    /// The form's value of a document kind: its own code's, or for a
    /// combined act (`BMS`) its first kind's, publications of banns as
    /// marriages.
    fn act_value(&self, act: &Act) -> Option<&str> {
        self.acts
            .get(&act.to_string())
            .or_else(|| {
                let first = act.primary_kind()?;
                self.acts.get(&first.letter().to_string())
            })
            .map(String::as_str)
    }

    /// The portal's home page, a frameset a reader searches from.
    fn home(&self) -> String {
        format!("{}{}/FrmAccueilFrame.asp", self.origin, self.base)
    }

    /// The viewer page's path and query for a register.
    fn viewer_path(&self, lot: &Lot) -> String {
        let mut query = Query::new();
        query
            .push("idlot", lot.idlot.as_str())
            .push("idfic", lot.idfic.as_str())
            .push("ref", lot.reference.as_str())
            .push("appliCindoc", lot.application.as_str())
            .push("resX", "1400")
            .push("resY", "900")
            .push("init", "1")
            .push("visionneuseHTML5", "0");
        format!("{}/FrmLotDocFrame.asp?{query}", self.base)
    }
}

/// Opens the portal's session: the first page sets the session cookie and
/// sends the browser to the cookie check, which the portal requires before
/// any search; then selects the module, which the session keeps.
async fn open_session(settings: &Settings, fetch: &dyn PortalFetch) -> Result<(), ResolveError> {
    let first = get(fetch, &format!("{}/FrmAccueilDroite.asp", settings.base)).await?;
    // A session already open, the window's from an earlier lookup, sends
    // the browser on to the summary at once.
    if !page::session_open(&first) {
        let check = page::cookie_check(&first, &settings.base).ok_or_else(|| {
            markup::unreadable(
                &first,
                "thot: the first page has no cookie check".to_owned(),
            )
        })?;
        let checked = get(fetch, &check).await?;
        if let Some(detail) = page::session_refused(&checked) {
            return Err(unexpected(detail));
        }
    }
    get(
        fetch,
        &format!(
            "{}/Recherche/FrmRechFrame.asp?MOD={}",
            settings.base, settings.module
        ),
    )
    .await?;
    Ok(())
}

/// The module's search form, read once the session holds the module.
async fn form_page(settings: &Settings, fetch: &dyn PortalFetch) -> Result<String, ResolveError> {
    let search = format!("{}/Recherche", settings.base);
    get(
        fetch,
        &format!("{search}/FrmRechHaut.asp?MOD={}", settings.module),
    )
    .await?;
    let page = get(
        fetch,
        &format!("{search}/FrmRechDOCCritere.asp?MOD={}", settings.module),
    )
    .await?;
    if let Some(detail) = page::session_refused(&page) {
        return Err(unexpected(detail));
    }
    Ok(page)
}

/// What one search found.
struct Found {
    /// The registers read, with their viewer's arguments.
    candidates: Vec<Candidate<Lot>>,
    /// The records the portal counted, every label and page together.
    total: usize,
}

/// The body of the form's submission: its hidden inputs, the label and the
/// document kind's value selected as the page's scripts select them, and
/// the years.
fn submission(
    settings: &Settings,
    form: &Form<'_>,
    label: &str,
    act: Option<&str>,
    year: Option<u16>,
) -> String {
    let mut values: Vec<(String, String)> = form.hidden();
    let mut set = |name: String, value: &str| {
        match values.iter_mut().find(|(known, _)| *known == name) {
            Some((_, known)) => value.clone_into(known),
            None => values.push((name, value.to_owned())),
        };
    };
    set(format!("txt_CIN_IDX{}", settings.locality), "1");
    set(format!("txt_CIN_CH{}", settings.locality), label);
    if let (Some(criterion), Some(value)) = (settings.act, act) {
        set(format!("txt_CIN_IDX{criterion}"), "1");
        set(format!("txt_CIN_CH{criterion}"), value);
        if form.has_checkboxes(criterion) {
            set(page::checkbox(criterion), value);
        }
    }
    let year = year.map(|year| year.to_string()).unwrap_or_default();
    match settings.years {
        None => {}
        Some(Years::Dex) => {
            set("txt_CIN_DEX_D".to_owned(), &year);
            set("txt_CIN_DEX_F".to_owned(), &year);
        }
        Some(Years::Interval(criterion)) => {
            let interval = if year.is_empty() {
                String::new()
            } else {
                format!("{year}|{year}")
            };
            set(format!("txt_CIN_CH{criterion}"), &interval);
            set(format!("intervalleDate1_{criterion}"), &year);
            set(format!("intervalleDate2_{criterion}"), &year);
        }
    }
    set("b_ExecForm".to_owned(), "1");
    set("txt_IDX_OCC".to_owned(), "0");
    let mut query = Query::new();
    for (name, value) in &values {
        query.push(name.as_str(), value.as_str());
    }
    query.to_string()
}

/// Whether the form still has what the settings select: the locality list,
/// the document kind's value, the year inputs.
fn check_form(settings: &Settings, form: &Form<'_>, act: Option<&str>) -> Result<(), ResolveError> {
    if form.values(settings.locality).is_empty() {
        return Err(unexpected("the form has no locality list"));
    }
    if let (Some(criterion), Some(value)) = (settings.act, act)
        && !form
            .values(criterion)
            .iter()
            .any(|offered| offered == value)
    {
        return Err(unexpected(
            "the form does not offer the document kind's value",
        ));
    }
    let years = match settings.years {
        None => true,
        Some(Years::Dex) => form.has_input("txt_CIN_DEX_D"),
        Some(Years::Interval(criterion)) => form.has_input(&format!("intervalleDate1_{criterion}")),
    };
    if !years {
        return Err(unexpected("the form has no year input"));
    }
    Ok(())
}

/// Searches the module for the cited locality, document kind and year: the
/// session, the form's locality list, then one submission per label naming
/// the locality, its pages read until the citation decides. `localities`
/// are the forms of the cited locality.
async fn search(
    settings: &Settings,
    citation: &CitationParts,
    localities: &[&str],
    fetch: &dyn PortalFetch,
) -> Result<Found, ResolveError> {
    open_session(settings, fetch).await?;
    let page = form_page(settings, fetch).await?;
    let form = Form::of(&page)
        .ok_or_else(|| markup::unreadable(&page, "thot: the search page has no form".to_owned()))?;
    let act = settings.act_value(&citation.act);
    check_form(settings, &form, act)?;
    let labels = form.values(settings.locality);
    let named = matching_labels(&labels, localities);
    if named.is_empty() || named.len() > MAX_LABELS {
        return Ok(Found {
            candidates: Vec::new(),
            total: named.len(),
        });
    }
    let action = format!("{}/Recherche/FrmRechDOCCritere.asp", settings.base);
    let list = format!("{}/Recherche/FrmRechListeHaut.asp?RechDoc=1", settings.base);
    let mut found = Found {
        candidates: Vec::new(),
        total: 0,
    };
    for label in named {
        let body = submission(settings, &form, label, act, citation.year);
        let request = PortalRequest::post(&action, "application/x-www-form-urlencoded", body);
        let mut results = page::results(&unchallenged(fetch.request(&request).await?)?)?;
        found.total += results.total;
        let mut read = 1;
        loop {
            found.candidates.extend(
                results
                    .rows
                    .iter()
                    .map(|row| candidate(row, label, citation)),
            );
            let decided = narrow(&found.candidates, citation, localities).len() == 1;
            if decided || read >= results.pages.min(MAX_PAGES) {
                break;
            }
            read += 1;
            results = page::results(&get(fetch, &format!("{list}&page={read}")).await?)?;
        }
    }
    Ok(found)
}

/// A result row as selection reads it: its locality cell, or the label
/// searched where the list has none, as a citation writes it.
fn candidate(row: &Row, label: &str, citation: &CitationParts) -> Candidate<Lot> {
    let series = matches!(citation.act, Act::Series(_));
    Candidate {
        locality: Some(label_name(row.locality.as_deref().unwrap_or(label))),
        call_number: row.call_number.clone(),
        // A series module's type filter is the portal's own.
        act: if series { None } else { row.act_code() },
        parish: None,
        period: row.period.clone(),
        images: None,
        numbers: row.numbers(),
        payload: row.lot.clone(),
    }
}

/// The view `n` of an ARK-addressed register: the ARK and its resolver,
/// which opens the register at that view without a session.
struct ArkView {
    ark: String,
    resolver: String,
}

/// Reads a register's slide file for its views: the viewer page names the
/// file, one `<SLIDE>` per view with its ARK.
async fn ark_views(
    settings: &Settings,
    lot: &Lot,
    fetch: &dyn PortalFetch,
) -> Result<Vec<ArkView>, ResolveError> {
    let viewer = get(fetch, &settings.viewer_path(lot)).await?;
    let path = page::slide_path(&viewer, &settings.base).ok_or_else(|| {
        markup::unreadable(
            &viewer,
            "thot: the viewer page names no slide file".to_owned(),
        )
    })?;
    let slides = page::slides(&get(fetch, &path).await?)?;
    let prefix = format!("{}{}/ark:/", settings.origin, settings.base);
    let naan = slides
        .ark
        .as_deref()
        .and_then(|ark| ark.strip_prefix(&prefix))
        .filter(|naan| !naan.is_empty() && naan.bytes().all(|byte| byte.is_ascii_digit()))
        .ok_or_else(|| unexpected("the slide file has no ARK of the portal"))?;
    let valid = |link: &String| {
        !link.is_empty()
            && link
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'/')
    };
    if slides.links.len() != slides.count || !slides.links.iter().all(valid) {
        return Err(unexpected("a view of the slide file has no ARK"));
    }
    Ok(slides
        .links
        .iter()
        .map(|link| ArkView {
            ark: format!("{prefix}{naan}/{link}"),
            resolver: format!(
                "{}{}/gestionARK.asp?a={}",
                settings.origin,
                settings.base,
                encode(&format!("{naan}/{link}"))
            ),
        })
        .collect())
}

/// The `View` target of a chosen register.
async fn view(
    settings: &Settings,
    archive: &Archive,
    citation: &CitationParts,
    chosen: &Candidate<Lot>,
    fetch: &dyn PortalFetch,
) -> Result<ArchiveTarget, ResolveError> {
    let call_number = chosen.call_number.as_deref();
    match settings.views {
        // The viewer page opens the register on its first view, in the
        // session that searched it; its size is not read, since opening the
        // viewer writes a slide file on the server and the window opens it.
        Views::Register => Ok(view_target(
            archive,
            citation,
            call_number,
            usize::MAX,
            format!(
                "{}{}",
                settings.origin,
                settings.viewer_path(&chosen.payload)
            ),
            Vec::new(),
        )),
        Views::Ark => {
            let arks = ark_views(settings, &chosen.payload, fetch).await?;
            let views = cited_views(citation, arks.len(), chosen.period.as_deref())
                .iter()
                .filter_map(|cited| {
                    let at = arks.get(usize::from(cited.view).checked_sub(1)?)?;
                    Some(ArchiveView {
                        view: cited.view,
                        url: at.resolver.clone(),
                        ark: Some(at.ark.clone()),
                        image: None,
                    })
                })
                .collect();
            Ok(view_target(
                archive,
                citation,
                call_number,
                arks.len(),
                arks[0].resolver.clone(),
                views,
            ))
        }
    }
}

impl Platform for Thot {
    fn id(&self) -> &'static str {
        "thot"
    }

    fn validate(&self, collection: &Collection) -> Result<(), CatalogError> {
        Settings::read(collection).map(drop)
    }

    fn endpoint(&self, collection: &Collection) -> Option<PortalEndpoint> {
        let settings = Settings::read(collection).ok()?;
        // The portal's stylesheet: a page of its origin that the portal's
        // anti-bot measure guards like the others and that navigates nowhere,
        // whereas the home frameset's frames send the top page on to the
        // summary while the requests run.
        Some(PortalEndpoint {
            start: format!("{}{}/css/general.css", settings.origin, settings.base),
            origin: settings.origin,
            other_origins: Vec::new(),
            access: settings.transport,
        })
    }

    fn results_url(&self, collection: &Collection, _citation: &CitationParts) -> Option<String> {
        // The search lives in the session: the home page is where a reader
        // starts it.
        Some(Settings::read(collection).ok()?.home())
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
    let localities: Vec<&str> = wanted.iter().map(String::as_str).collect();
    let found = search(&settings, citation, &localities, fetch).await?;
    let matches = match narrow(&found.candidates, citation, &localities).as_slice() {
        [only] => return view(&settings, archive, citation, only, fetch).await,
        // No label names the locality (0), too many do, or the records
        // found open no viewer: what the portal counted.
        [] if found.candidates.is_empty() => found.total,
        many => many.len(),
    };
    Ok(ArchiveTarget::Results {
        url: settings.home(),
        matches: Some(matches),
    })
}
