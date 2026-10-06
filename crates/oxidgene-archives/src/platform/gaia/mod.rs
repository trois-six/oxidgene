//! GAIA 9, the search engine under `/mdr/index.php/rechercheTheme/…`.
//!
//! Each of the portal's "thematic searches" is a wizard whose choices the
//! PHP session keeps: a locality from an alphabetical list, the kind of
//! register, sometimes a further list, the years, then the search, whose
//! answer lists the registers twenty at a time. The pages are ISO-8859-1,
//! answer a plain client with a cookie jar, and have no anti-bot check. The
//! viewer has no address per view and opens on the first image, so the
//! target is the register (`View` with no views) and the window names the
//! cited view to go to. Archive Portals §4.8 specifies the requests.

#[cfg(any(test, feature = "live"))]
mod live;
mod page;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use serde::Deserialize;

use super::locality::{LocalityStyle, forms};
use super::markup::{is_named, letters};
use super::select::{Candidate, Selection, covers, holds_act, period_ranges, select};
use super::view::view_target;
use super::{Access, BoxFuture, Platform, PortalEndpoint, Query, is_https_origin};
use crate::catalog::{Archive, CatalogError, Collection};
use crate::citation::{Act, CitationParts};
use crate::transport::{PortalFetch, PortalRequest};
use crate::{ArchiveTarget, ResolveError};

pub(crate) use page::Row;

/// The GAIA adapter.
pub struct Gaia;

/// The list steps the wizard may still offer once the settings' path is
/// followed, at most.
const MAX_LIST_STEPS: usize = 3;

/// The other entries of a cited locality one resolution searches, at most.
const MAX_OTHER_ENTRIES: usize = 2;

/// The further pages of an answer one resolution reads, at most.
const MAX_PAGES: usize = 5;

/// The form a step's answer posts to restrict the search to one year.
const FORM: &str = "application/x-www-form-urlencoded";

/// A collection's `portal` settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    origin: String,
    /// The engine's path on the origin, `/mdr` unless the portal moved it.
    #[serde(default = "default_base")]
    base: String,
    #[serde(default)]
    transport: Access,
    /// The thematic search: `requeteConstructor/<theme>/…`.
    theme: u16,
    /// Whether the search starts with a list of localities (communes,
    /// parishes, registration offices); `false` when it starts with the
    /// kinds of registers.
    #[serde(default = "yes")]
    localities: bool,
    /// Whether that list is split by initial letter.
    #[serde(default = "yes")]
    letters: bool,
    /// How the list writes a leading article, which decides the letter.
    #[serde(default)]
    locality_style: LocalityStyle,
    /// The words the list writes before every locality: `Bureau de `.
    #[serde(default)]
    prefix: Option<String>,
    /// For each document kind, the labels of the lists to choose from, in
    /// order, after the locality; empty when the search offers no kind.
    #[serde(default)]
    types: BTreeMap<String, Vec<String>>,
}

fn default_base() -> String {
    "/mdr".to_owned()
}

const fn yes() -> bool {
    true
}

/// What the adapter keeps of a register to open it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Register {
    /// The viewer's path, up to the window size it takes.
    viewer: String,
}

fn invalid(message: &str) -> CatalogError {
    CatalogError::new(format!("gaia settings: {message}"))
}

fn unexpected(detail: &str) -> ResolveError {
    ResolveError::UnexpectedResponse(format!("gaia: {detail}"))
}

fn is_text(text: &str) -> bool {
    !text.trim().is_empty() && !text.chars().any(char::is_control)
}

impl Settings {
    fn read(collection: &Collection) -> Result<Self, CatalogError> {
        let settings =
            Self::deserialize(&collection.portal).map_err(|error| invalid(&error.to_string()))?;
        settings.check()?;
        settings.check_acts(collection)?;
        Ok(settings)
    }

    fn check(&self) -> Result<(), CatalogError> {
        if !is_https_origin(&self.origin) {
            return Err(invalid("origin must be an https origin"));
        }
        let base_ok = self.base.starts_with('/')
            && self.base.len() > 1
            && !self.base.ends_with('/')
            && self
                .base
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"/-_".contains(&byte));
        if !base_ok {
            return Err(invalid("base must be a path such as `/mdr`"));
        }
        if self.theme == 0 {
            return Err(invalid("theme must be a thematic search's number"));
        }
        if self.letters && !self.localities {
            return Err(invalid("letters needs a list of localities"));
        }
        if !matches!(
            self.locality_style,
            LocalityStyle::Plain | LocalityStyle::ArticleSuffix
        ) {
            return Err(invalid("locality_style must be plain or article_suffix"));
        }
        if self
            .prefix
            .as_deref()
            .is_some_and(|prefix| !is_text(prefix))
        {
            return Err(invalid("prefix must be text"));
        }
        if !self.localities && self.prefix.is_some() {
            return Err(invalid("prefix needs a list of localities"));
        }
        Ok(())
    }

    /// Every kind code is valid, with a path of labels, and every kind the
    /// collection holds has one, unless the search offers no kind at all.
    fn check_acts(&self, collection: &Collection) -> Result<(), CatalogError> {
        for (code, path) in &self.types {
            if Act::from_code(code).is_none() {
                return Err(invalid(&format!("`{code}` is not an act code")));
            }
            if path.is_empty() || !path.iter().all(|label| is_text(label)) {
                return Err(invalid(&format!("the labels of `{code}` are empty")));
            }
        }
        match collection.acts.iter().find(|act| self.path(act).is_none()) {
            Some(act) => Err(invalid(&format!("no labels for `{act}`"))),
            None => Ok(()),
        }
    }

    /// The labels to choose for an act: its own, or a combined act's
    /// (`BMS`) first kind's, banns filed with the marriages; none to choose
    /// when the search offers no kind.
    fn path(&self, act: &Act) -> Option<&[String]> {
        if self.types.is_empty() {
            return Some(&[]);
        }
        self.types
            .get(&act.to_string())
            .or_else(|| {
                let kind = act.primary_kind()?;
                self.types.get(&kind.letter().to_string())
            })
            .map(Vec::as_slice)
    }

    /// The wizard's address of `step` and `mode` with its two arguments.
    fn step(&self, step: u16, mode: char, first: &str, second: &str) -> String {
        format!(
            "{}/index.php/rechercheTheme/requeteConstructor/{}/{step}/{mode}/{first}/{second}",
            self.base, self.theme
        )
    }

    /// The first list of the search, or its letter's.
    fn list(&self, letter: Option<char>) -> String {
        match letter {
            Some(letter) => self.step(1, 'R', &letter.to_string(), "0"),
            None => self.step(1, 'R', "0", "0"),
        }
    }

    /// The initial under which the list files the cited locality: that of
    /// the name as the portal writes it, prefix included.
    fn letter(&self, locality: &str) -> Option<char> {
        let written = format!(
            "{}{}",
            self.prefix.as_deref().unwrap_or_default(),
            self.locality_style.write(locality)
        );
        letters(&written)
            .first()
            .filter(|letter| letter.is_ascii_alphabetic())
            .map(char::to_ascii_uppercase)
    }

    /// The list the cited locality is in: its letter's when the list is
    /// split by letter, the whole list otherwise.
    fn locality_list(&self, citation: &CitationParts) -> String {
        let letter = (self.localities && self.letters)
            .then(|| self.letter(&citation.locality))
            .flatten();
        self.list(letter)
    }

    fn absolute(&self, path: &str) -> String {
        format!("{}{path}", self.origin)
    }

    /// The viewer of a register, at a window size it lays itself out for.
    fn viewer_url(&self, viewer: &str) -> String {
        format!("{}{viewer}900/1400", self.origin)
    }

    /// The answer's further page starting at `offset`.
    fn page(&self, offset: usize) -> String {
        format!("{}/index.php/rechercheTheme/paginer/{offset}", self.base)
    }
}

impl Platform for Gaia {
    fn id(&self) -> &'static str {
        "gaia"
    }

    fn validate(&self, collection: &Collection) -> Result<(), CatalogError> {
        Settings::read(collection).map(drop)
    }

    fn endpoint(&self, collection: &Collection) -> Option<PortalEndpoint> {
        let settings = Settings::read(collection).ok()?;
        Some(PortalEndpoint {
            start: settings.absolute(&settings.list(None)),
            other_origins: Vec::new(),
            origin: settings.origin,
            access: settings.transport,
            insecure_http: false,
        })
    }

    fn results_url(&self, collection: &Collection, citation: &CitationParts) -> Option<String> {
        // The wizard's choices live in the session: the list holding the
        // cited locality is the closest page a reader can open.
        let settings = Settings::read(collection).ok()?;
        Some(settings.absolute(&settings.locality_list(citation)))
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

/// What a search is run for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Purpose {
    /// A citation's register: the further pages it needs are read.
    Resolve,
    /// Any register of a locality and a kind, for the live checks: a list
    /// left that the search cannot skip is answered by its first choice.
    #[cfg(any(test, feature = "live"))]
    Discover,
}

/// What a search found.
enum Found {
    /// The registers it listed, the pages read, and the count it gave.
    Rows {
        rows: Vec<Row>,
        total: usize,
        complete: bool,
        /// The year of the list choice the search went through, when it is
        /// one year (a class, a census): the year the registers are cited
        /// by, which their own dates may follow (a class's lists are drawn
        /// the next year).
        year: Option<u16>,
    },
    /// No list choice fits the citation, or several do: their count.
    Choices(usize),
}

/// Where the searches of a citation start.
enum Start {
    /// The page after the chosen locality, or the search's first page; and
    /// the locality's other entries, its parishes and bodies, when its own
    /// was chosen.
    Page {
        answer: String,
        others: Vec<page::Link>,
    },
    /// No locality entry fits the citation, or several do: their count.
    Choices(usize),
}

/// What one search makes of the citation.
struct Verdict {
    selection: Result<Box<Candidate<Register>>, usize>,
    /// The registers holding the cited act and covering the cited year.
    fitting: usize,
}

async fn resolve(
    archive: &Archive,
    collection: &Collection,
    citation: &CitationParts,
    fetch: &dyn PortalFetch,
) -> Result<ArchiveTarget, ResolveError> {
    let settings = Settings::read(collection).map_err(|_| ResolveError::NoAdapter)?;
    let results = |matches| ArchiveTarget::Results {
        url: settings.absolute(&settings.locality_list(citation)),
        matches: Some(matches),
    };
    let (answer, others) = match start(&settings, citation, fetch).await? {
        Start::Page { answer, others } => (answer, others),
        Start::Choices(count) => return Ok(results(count)),
    };
    let found = search_from(&settings, citation, fetch, Purpose::Resolve, answer).await?;
    let mut verdict = judge(&settings, citation, found);
    // A commune's own entry may hold its civil status alone, its parish
    // registers being listed under its parishes (Orne).
    if verdict.fitting == 0 && !others.is_empty() {
        if others.len() > MAX_OTHER_ENTRIES {
            return Ok(results(others.len()));
        }
        if let Some(found) = search_others(&settings, citation, fetch, &others).await? {
            verdict = found;
        }
    }
    Ok(match verdict.selection {
        Ok(row) => view_target(
            archive,
            citation,
            row.call_number.as_deref(),
            usize::MAX,
            settings.viewer_url(&row.payload.viewer),
            Vec::new(),
        ),
        Err(count) => results(count),
    })
}

/// The verdict of the locality's other entries searched in turn: the one
/// register found among them, or the count of those that fit; `None` when
/// none fits.
async fn search_others(
    settings: &Settings,
    citation: &CitationParts,
    fetch: &dyn PortalFetch,
    others: &[page::Link],
) -> Result<Option<Verdict>, ResolveError> {
    let mut chosen = Vec::new();
    let mut several = 0;
    for other in others {
        let answer = fetch.get(&other.path).await?;
        let found = search_from(settings, citation, fetch, Purpose::Resolve, answer).await?;
        match judge(settings, citation, found) {
            Verdict { fitting: 0, .. } => {}
            Verdict {
                selection: Ok(row), ..
            } => chosen.push(row),
            Verdict {
                selection: Err(count),
                ..
            } => several += count,
        }
    }
    let fitting = chosen.len() + several;
    Ok(match chosen.pop() {
        Some(row) if fitting == 1 => Some(Verdict {
            selection: Ok(row),
            fitting,
        }),
        _ if fitting > 0 => Some(Verdict {
            selection: Err(fitting),
            fitting,
        }),
        _ => None,
    })
}

/// The register the rows of one search select, or the count of `Results`:
/// the matches, or the search's count when pages left unread may hold more
/// (without a year, or with no match).
fn judge(settings: &Settings, citation: &CitationParts, found: Found) -> Verdict {
    let (rows, total, complete) = match found {
        Found::Rows {
            rows,
            total,
            complete,
            year,
        } => (dated(rows, year), total, complete),
        Found::Choices(count) => {
            return Verdict {
                selection: Err(count),
                fitting: count,
            };
        }
    };
    let candidates = candidates(settings, citation, rows);
    let localities = row_localities(settings, citation, &candidates);
    let localities: Vec<&str> = localities.iter().map(String::as_str).collect();
    let fitting = candidates
        .iter()
        .filter(|candidate| fits(candidate, citation))
        .count();
    let selection = match select(&candidates, citation, &localities) {
        Selection::One(row) => Ok(Box::new(row.clone())),
        Selection::Many(matches) if !complete && (matches == 0 || citation.year.is_none()) => {
            Err(total)
        }
        Selection::Many(matches) => Err(matches),
    };
    Verdict { selection, fitting }
}

/// Whether a register holds the cited act, as its title reads, and covers
/// the cited year.
fn fits(candidate: &Candidate<Register>, citation: &CitationParts) -> bool {
    holds_act(candidate.act.as_deref(), &citation.act)
        && citation.year.is_none_or(|year| {
            candidate
                .period
                .as_deref()
                .is_some_and(|period| covers(period, year))
        })
}

/// The first steps of a search: the list holding the cited locality, then
/// the entry naming it, or the search's first page without localities.
async fn start(
    settings: &Settings,
    citation: &CitationParts,
    fetch: &dyn PortalFetch,
) -> Result<Start, ResolveError> {
    let list = fetch.get(&settings.locality_list(citation)).await?;
    if !settings.localities {
        return Ok(Start::Page {
            answer: list,
            others: Vec::new(),
        });
    }
    let links = page::links(&list, settings)?;
    let (chosen, others) = choose_locality(settings, &links, citation);
    Ok(match chosen.as_slice() {
        [one] => Start::Page {
            answer: fetch.get(&one.path).await?,
            others: others.into_iter().cloned().collect(),
        },
        many => Start::Choices(many.len()),
    })
}

/// Runs the rest of the wizard from `answer`, the page after the locality:
/// the kind's labels, the lists left, the year, the search, then the
/// further pages the citation needs.
async fn search_from(
    settings: &Settings,
    citation: &CitationParts,
    fetch: &dyn PortalFetch,
    purpose: Purpose,
    mut answer: String,
) -> Result<Found, ResolveError> {
    let Some(path) = settings.path(&citation.act) else {
        return Ok(Found::Choices(0));
    };
    for label in path {
        let links = page::links(&answer, settings)?;
        let wanted = [letters(label)];
        let Some(link) = links.iter().find(|link| is_named(&link.label, &wanted)) else {
            return Ok(Found::Choices(0));
        };
        answer = fetch.get(&link.path).await?;
    }
    let year;
    (answer, year) = remaining_lists(settings, citation, fetch, answer, purpose).await?;

    if let (Some(action), Some(year)) = (page::action(&answer, settings, 'A'), citation.year) {
        let mut form = Query::new();
        form.push("typeDate", "simple")
            .push("dateDeb", "")
            .push("dateFin", "")
            .push("dateSimple", year.to_string());
        answer = fetch
            .request(&PortalRequest::post(action, FORM, form.to_string()))
            .await?;
    }
    let action = page::action(&answer, settings, 'T')
        .ok_or_else(|| page::unreadable(&answer, "the step offers no search"))?;
    let first = fetch
        .request(&PortalRequest::post(action, FORM, "forcepost=essai"))
        .await?;
    let (mut rows, total) = page::rows(&first, settings)?;
    let size = rows.len();
    let mut complete = total <= size;
    if purpose == Purpose::Resolve && !complete {
        complete = more_pages(settings, citation, fetch, size, total, &mut rows).await?;
    }
    Ok(Found::Rows {
        rows,
        total,
        complete,
        year,
    })
}

/// The rows of a search that went through a choice of one year, dated by
/// that year.
fn dated(rows: Vec<Row>, year: Option<u16>) -> Vec<Row> {
    match year {
        Some(year) => rows
            .into_iter()
            .map(|row| Row {
                period: Some(year.to_string()),
                ..row
            })
            .collect(),
        None => rows,
    }
}

/// Follows the lists the settings do not name: the one whose label covers
/// the cited year (a census year, a registration office's period), or the
/// step's "search all" form, until the step asks for the years or offers
/// the search. A list that has neither is searched as it stands, which
/// finds nothing: a conscription list's class must be cited. The answer,
/// and the year of a choice of one year that was followed.
async fn remaining_lists(
    settings: &Settings,
    citation: &CitationParts,
    fetch: &dyn PortalFetch,
    mut answer: String,
    purpose: Purpose,
) -> Result<(String, Option<u16>), ResolveError> {
    let mut year = None;
    for _ in 0..MAX_LIST_STEPS {
        if page::action(&answer, settings, 'A').is_some() {
            break;
        }
        let links = page::links(&answer, settings)?;
        if links.is_empty() {
            break;
        }
        let covering: Vec<&page::Link> = citation
            .year
            .map(|year| {
                links
                    .iter()
                    .filter(|link| {
                        period_ranges(&link.label)
                            .iter()
                            .any(|(first, last)| (*first..=*last).contains(&year))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let followed = match (covering.as_slice(), page::action(&answer, settings, 'F')) {
            ([one], _) => *one,
            // A search through every choice may fail on the server (the
            // Aude censuses answer 500): discovering, the first is enough.
            _ if purpose != Purpose::Resolve => &links[0],
            (_, Some(skip)) => {
                answer = fetch
                    .request(&PortalRequest::post(skip, FORM, String::new()))
                    .await?;
                continue;
            }
            _ => break,
        };
        if let [(first, last)] = period_ranges(&followed.label).as_slice()
            && first == last
        {
            year = Some(*first);
        }
        answer = fetch.get(&followed.path).await?;
    }
    Ok((answer, year))
}

/// Reads further pages of an answer of `total` rows, `size` a page: with a
/// year, those a search through the pages, sorted by date, reaches; with a
/// call number and no year, the next ones until it is found. Whether every
/// page was read.
async fn more_pages(
    settings: &Settings,
    citation: &CitationParts,
    fetch: &dyn PortalFetch,
    size: usize,
    total: usize,
    rows: &mut Vec<Row>,
) -> Result<bool, ResolveError> {
    if size == 0 {
        return Ok(false);
    }
    let last = (total - 1) / size;
    if let Some(year) = citation.year {
        // The pages run by date: look for the last one starting by the
        // cited year, which holds the registers starting that year.
        let (mut low, mut high) = (0, last);
        if first_years(rows).last().is_some_and(|first| *first > year) {
            high = 0;
        }
        let mut read = 1;
        while low < high && read <= MAX_PAGES {
            let middle = (low + high).div_ceil(2);
            let (page, _) = page::rows(&fetch.get(&settings.page(middle * size)).await?, settings)?;
            read += 1;
            let starts = first_years(&page);
            rows.extend(page);
            match (starts.first(), starts.last()) {
                (Some(first), _) if *first > year => high = middle - 1,
                (_, Some(end)) if *end > year => (low, high) = (middle, middle),
                _ => low = middle,
            }
        }
        return Ok(read > last);
    }
    if citation.call_number.is_none() {
        return Ok(false);
    }
    for index in 1..=last.min(MAX_PAGES) {
        let (page, _) = page::rows(&fetch.get(&settings.page(index * size)).await?, settings)?;
        rows.extend(page);
        let candidates = candidates(settings, citation, rows.clone());
        let localities = row_localities(settings, citation, &candidates);
        let localities: Vec<&str> = localities.iter().map(String::as_str).collect();
        if matches!(
            select(&candidates, citation, &localities),
            Selection::One(_)
        ) {
            return Ok(index == last);
        }
    }
    Ok(last <= MAX_PAGES)
}

/// The first year of each dated row, in order.
fn first_years(rows: &[Row]) -> Vec<u16> {
    rows.iter()
        .filter_map(|row| {
            period_ranges(row.period.as_deref().unwrap_or(&row.title))
                .first()
                .map(|(first, _)| *first)
        })
        .collect()
}

/// The list entries naming the cited locality: those of the cited parish,
/// or else the locality's own entry, with its parishes and bodies
/// (`Exampleville, paroisse Saint-Exemple`) apart, the other entries
/// searched when the own one holds no register of the citation.
fn choose_locality<'l>(
    settings: &Settings,
    links: &'l [page::Link],
    citation: &CitationParts,
) -> (Vec<&'l page::Link>, Vec<&'l page::Link>) {
    let wanted: Vec<Vec<char>> = forms(&citation.locality)
        .iter()
        .map(|form| letters(form))
        .collect();
    let named: Vec<(&page::Link, page::Entry<'_>)> = links
        .iter()
        .map(|link| (link, page::entry(&link.label, settings.prefix.as_deref())))
        .filter(|(_, entry)| is_named(entry.name, &wanted))
        .collect();
    let keep = |test: &dyn Fn(&page::Entry<'_>) -> bool| -> Vec<&'l page::Link> {
        named
            .iter()
            .filter(|(_, entry)| test(entry))
            .map(|(link, _)| *link)
            .collect()
    };
    if let Some(parish) = citation.parish.as_deref() {
        let parish = letters(parish);
        let of_parish = keep(&|entry| page::contains_letters(entry.detail, &parish));
        if !of_parish.is_empty() {
            return (of_parish, Vec::new());
        }
    }
    let own = keep(&|entry| entry.detail.is_empty());
    if own.is_empty() {
        (keep(&|_| true), Vec::new())
    } else {
        (own, keep(&|entry| !entry.detail.is_empty()))
    }
}

/// The candidates of the rows with images: at the locality the search
/// chose, or, in a search without localities, at the cited locality when a
/// title starts with it (`Exampleville, matricules n° 1-500`) and at the
/// title's own otherwise; with the acts a title names.
fn candidates(
    settings: &Settings,
    citation: &CitationParts,
    rows: Vec<Row>,
) -> Vec<Candidate<Register>> {
    let wanted: Vec<Vec<char>> = forms(&citation.locality)
        .iter()
        .map(|form| letters(form))
        .collect();
    rows.into_iter()
        .filter_map(|row| {
            let viewer = row.viewer?;
            let titled = page::title_locality(&row.title);
            let locality = if settings.localities || is_named(titled, &wanted) {
                citation.locality.clone()
            } else {
                titled.to_owned()
            };
            let act = match citation.act {
                // A series' search holds that series alone.
                Act::Series(_) => None,
                _ => page::title_act(&row.title),
            };
            Some(Candidate {
                locality: Some(locality),
                numbers: page::title_numbers(&row.title),
                period: row.period.or_else(|| Some(row.title.clone())),
                call_number: row.call_number,
                act,
                parish: None,
                images: None,
                payload: Register { viewer },
            })
        })
        .collect()
}

/// The localities a candidate may show: the cited one, or, in a search
/// without localities whose titles do not name it, any.
fn row_localities(
    settings: &Settings,
    citation: &CitationParts,
    candidates: &[Candidate<Register>],
) -> Vec<String> {
    let named = !citation.locality.is_empty()
        && candidates
            .iter()
            .any(|candidate| candidate.locality.as_deref() == Some(citation.locality.as_str()));
    if settings.localities || named {
        forms(&citation.locality)
    } else {
        vec![String::new()]
    }
}
