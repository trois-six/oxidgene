//! The older Mnesys interface ("iNAO, powered by Naoned"), which the Savoie
//! archives' search runs, beside a viewer on another host.
//!
//! Each guided search (`/?id=recherche_guidee_…`) is a `GET` form whose
//! fields are named after the finding aids' elements: a locality matched by
//! word against the place index (`geogname`), dates (`unitdate`), the
//! digitized filter (`dao`), and portal-generated selects
//! (`v2_field_<id>`) matched against the nodes' titles (`{:unittitle}`),
//! for the kind of act or a class. Its hits are nodes of EAD finding aids,
//! registers and the headings over them, twenty to a page kept in the
//! session. A register's notice links its images on the viewer host at the
//! register's ARK, which opens view `n` with `?vue=<n>`: the target is
//! built without any request to that host, which turns away non-browser
//! clients. Archive Portals §4.14 specifies the requests.

#[cfg(any(test, feature = "live"))]
mod live;
mod page;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use serde::Deserialize;

use super::locality::forms;
use super::markup::{self, fold};
use super::select::{Candidate, act_code, narrow, number_range};
use super::view::view_target;
use super::{Access, BoxFuture, Platform, PortalEndpoint, Query, is_https_origin};
use crate::catalog::{Archive, CatalogError, Collection};
use crate::citation::{Act, CitationParts};
use crate::transport::PortalFetch;
use crate::{ArchiveTarget, ArchiveView, ResolveError};
use page::Node;

/// The older Mnesys interface's adapter.
pub struct MnesysInao;

/// The most pages of twenty hits read for one citation.
const MAX_PAGES: usize = 5;

/// The value of a select's companion field (`form_req_<field>`): the
/// value is matched against the node's title.
const TITLE_MATCH: &str = "{:unittitle}__VAL_";

fn invalid(message: &str) -> CatalogError {
    CatalogError::new(format!("mnesys-inao settings: {message}"))
}

fn unexpected(detail: &str) -> ResolveError {
    ResolveError::UnexpectedResponse(format!("mnesys-inao: {detail}"))
}

async fn get(fetch: &dyn PortalFetch, path: &str) -> Result<String, ResolveError> {
    let body = fetch.get(path).await?;
    if markup::is_challenge(&body) {
        Err(ResolveError::Challenged)
    } else {
        Ok(body)
    }
}

/// A select matched against the nodes' titles, whose value carries the
/// year: a military class, `"Classe {year}"`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct TitledYear {
    field: String,
    value: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    origin: String,
    #[serde(default)]
    transport: Access,
    form: String,
    #[serde(default)]
    locality: Option<String>,
    #[serde(default)]
    act: Option<String>,
    #[serde(default)]
    acts: BTreeMap<String, String>,
    #[serde(default)]
    year: Option<TitledYear>,
    viewer: String,
}

/// The settings of one collection, checked.
#[derive(Debug, Clone)]
struct Settings {
    origin: String,
    transport: Access,
    /// The guided search's identifier: `recherche_guidee_etat_civil_web`.
    form: String,
    /// The locality's field, `geogname`; none where the search has no
    /// locality (a department's military registers).
    locality: Option<String>,
    /// The kind-of-document select, with its value for each kind.
    act: Option<String>,
    acts: BTreeMap<String, String>,
    /// A select carrying the year; the date fields otherwise.
    year: Option<TitledYear>,
    /// The viewer host's origin, where the registers' ARKs open.
    viewer: String,
}

/// A form field's name: letters, digits and underscores.
fn is_field(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

impl Settings {
    fn read(collection: &Collection) -> Result<Self, CatalogError> {
        let raw =
            Raw::deserialize(&collection.portal).map_err(|error| invalid(&error.to_string()))?;
        if !is_https_origin(&raw.origin) || !is_https_origin(&raw.viewer) {
            return Err(invalid("origin and viewer must be https origins"));
        }
        let fields = [
            Some(&raw.form),
            raw.locality.as_ref(),
            raw.act.as_ref(),
            raw.year.as_ref().map(|year| &year.field),
        ];
        if !fields.into_iter().flatten().all(|name| is_field(name)) {
            return Err(invalid("form and fields are the portal's names"));
        }
        if raw
            .year
            .as_ref()
            .is_some_and(|year| year.value.matches("{year}").count() != 1)
        {
            return Err(invalid("year.value holds one `{year}`"));
        }
        if raw.act.is_none() != raw.acts.is_empty() {
            return Err(invalid("acts are the values of act, given with it"));
        }
        for code in raw.acts.keys() {
            if Act::from_code(code).is_none() {
                return Err(invalid(&format!("`{code}` is not a document kind's code")));
            }
        }
        let settings = Self {
            origin: raw.origin,
            transport: raw.transport,
            form: raw.form,
            locality: raw.locality,
            act: raw.act,
            acts: raw.acts,
            year: raw.year,
            viewer: raw.viewer,
        };
        if settings.act.is_some()
            && let Some(act) = collection
                .acts
                .iter()
                .find(|act| settings.act_value(act).is_none())
        {
            return Err(invalid(&format!("no value for `{act}`")));
        }
        Ok(settings)
    }

    /// The kind select's value for a document kind: its own code's, or for
    /// a combined act (`BMS`) its first kind's, banns as marriages.
    fn act_value(&self, act: &Act) -> Option<&str> {
        self.acts
            .get(&act.to_string())
            .or_else(|| {
                let first = act.primary_kind()?;
                self.acts.get(&first.letter().to_string())
            })
            .map(String::as_str)
    }

    /// The search's path and query, as the form submits it: the locality,
    /// the year, digitized documents only, the kind of document.
    fn search_path(&self, citation: &CitationParts) -> String {
        let mut query = Query::new();
        if let Some(field) = &self.locality {
            query
                .push(format!("form_search_{field}"), citation.locality.as_str())
                .push(format!("form_op_{field}"), "ET");
        }
        if let (None, Some(year)) = (&self.year, citation.year) {
            query
                .push("form_search_unitdate3", year.to_string())
                .push("form_search_unitdate", year.to_string());
        }
        query.push("form_search_dao", "oui");
        let mut titled = |field: &str, value: &str| {
            query
                .push(format!("form_search_{field}"), value)
                .push(format!("form_op_{field}"), "ET")
                .push(format!("form_req_{field}"), TITLE_MATCH);
        };
        if let (Some(field), Some(value)) = (&self.act, self.act_value(&citation.act)) {
            titled(field, value);
        }
        if let (Some(year_field), Some(year)) = (&self.year, citation.year) {
            titled(
                &year_field.field,
                &year_field.value.replace("{year}", &year.to_string()),
            );
        }
        query
            .push("display_thesaurus", "autocomplete")
            .push("action", "search")
            .push("id", self.form.as_str());
        format!("/?{query}")
    }

    /// A further page of the session's search.
    fn page_path(&self, page: usize) -> String {
        format!("/?id={}&doc=&page={page}&page_ref=", self.form)
    }
}

/// The names a node gives its place: the breadcrumb's entries, each read
/// up to its first full stop (`Exampleville. 1876-1936 (6M 1-12)`), and the
/// title's end after a dash (`Registre paroissial : mariages. -
/// Exampleville.`).
fn place_names(node: &Node) -> Vec<String> {
    let read = |text: &str| {
        let text = text.trim();
        let name = text.split(". ").next().unwrap_or(text);
        name.trim_end_matches('.').trim().to_owned()
    };
    let mut names: Vec<String> = node.context.iter().map(|entry| read(entry)).collect();
    if let Some((_, tail)) = node.title.rsplit_once(" - ") {
        names.push(read(tail));
    }
    names.retain(|name| !name.is_empty());
    names
}

/// The act of a register node, from its title and breadcrumb: a table
/// (`Table chrono-alphabétique des baptêmes…`, `Tables décennales…`) is
/// `TD`, otherwise the kinds its words name.
fn node_act(node: &Node) -> Option<String> {
    let text = format!("{} {}", node.title, node.context.join(" "));
    if fold(&text).split(' ').any(|word| word.starts_with("tabl")) {
        return Some("TD".to_owned());
    }
    act_code(&text, false)
}

/// Whether a folded place name holds the words of a wanted one without
/// being it: a longer place the index's word search also matched
/// (`exampleville le vieux` for `exampleville`), or a list of places.
fn names_another(name: &str, wanted: &[String]) -> bool {
    let words: Vec<&str> = name.split(' ').collect();
    wanted.iter().filter(|form| !form.is_empty()).any(|form| {
        let form: Vec<&str> = form.split(' ').collect();
        words.len() > form.len() && words.windows(form.len()).any(|window| window == form)
    })
}

/// A node as selection reads it: the place it names that is the cited
/// locality; failing one, a longer place or a list of places holding its
/// name, which the place index's word search matched too; failing both,
/// the cited locality itself, the place index's answer, which a title cut
/// short or a collection's heading does not repeat. Then the parish the
/// breadcrumb names after the place, the act of a register, the matricules
/// a military register spans.
fn candidate(node: &Node, citation: &CitationParts, localities: &[&str]) -> Candidate<String> {
    let wanted: Vec<String> = localities.iter().map(|locality| fold(locality)).collect();
    let names = place_names(node);
    let at = names.iter().position(|name| wanted.contains(&fold(name)));
    let locality = match at {
        Some(at) => Some(names[at].clone()),
        None => names
            .iter()
            .find(|name| names_another(&fold(name), &wanted))
            .cloned()
            .or_else(|| {
                wanted
                    .iter()
                    .any(|form| !form.is_empty())
                    .then(|| citation.locality.clone())
            }),
    };
    let parish = at
        .and_then(|at| node.context.get(at + 1))
        .map(|entry| entry.trim().trim_end_matches('.').to_owned())
        .filter(|entry| act_code(entry, false).is_none() && !fold(entry).starts_with("tabl"));
    let series = matches!(citation.act, Act::Series(_));
    Candidate {
        locality,
        call_number: node.call_number.clone(),
        act: if series { None } else { node_act(node) },
        parish,
        period: node.period.clone(),
        images: None,
        numbers: number_range(&node.title, true),
        payload: node.detail.clone(),
    }
}

/// What a search found.
struct Found {
    candidates: Vec<Candidate<String>>,
    total: usize,
}

/// Searches the cited locality, kind and year among digitized documents,
/// reading further pages of the session's answer until the citation
/// decides. A node without a call number is a heading over registers, which
/// the search lists themselves: it is left out.
async fn search(
    settings: &Settings,
    citation: &CitationParts,
    localities: &[&str],
    fetch: &dyn PortalFetch,
) -> Result<Found, ResolveError> {
    let mut answer = page::answer(&get(fetch, &settings.search_path(citation)).await?)?;
    let mut found = Found {
        candidates: Vec::new(),
        total: answer.total,
    };
    let mut read = 1;
    loop {
        found.candidates.extend(
            answer
                .nodes
                .iter()
                .filter(|node| node.call_number.is_some())
                .map(|node| candidate(node, citation, localities)),
        );
        let decided = narrow(&found.candidates, citation, localities).len() == 1;
        if decided || read >= answer.pages.min(MAX_PAGES) {
            return Ok(found);
        }
        read += 1;
        answer = page::answer(&get(fetch, &settings.page_path(read)).await?)?;
    }
}

/// The `View` target of a chosen register: its notice's link to the viewer
/// host, at the register's ARK, view `n` at `?vue=<n>`. The register's size
/// is not known without the viewer host.
async fn view(
    settings: &Settings,
    archive: &Archive,
    citation: &CitationParts,
    chosen: &Candidate<String>,
    fetch: &dyn PortalFetch,
) -> Result<ArchiveTarget, ResolveError> {
    let notice = get(fetch, &chosen.payload).await?;
    let media = page::media(&notice)?
        .ok_or_else(|| unexpected("the register's notice links no digitized document"))?;
    let ark = format!("{}/ark:/", settings.viewer);
    let valid = media.url.strip_prefix(&ark).is_some_and(|name| {
        !name.is_empty()
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'/')
    });
    if !valid {
        return Err(unexpected("the notice links no ARK of the viewer host"));
    }
    let views = citation
        .views
        .iter()
        .map(|cited| ArchiveView {
            view: cited.view,
            url: format!("{}?vue={}", media.url, cited.view),
            ark: None,
            image: None,
        })
        .collect();
    Ok(view_target(
        archive,
        citation,
        media
            .call_number
            .as_deref()
            .or(chosen.call_number.as_deref()),
        usize::MAX,
        media.url,
        views,
    ))
}

impl Platform for MnesysInao {
    fn id(&self) -> &'static str {
        "mnesys-inao"
    }

    fn validate(&self, collection: &Collection) -> Result<(), CatalogError> {
        Settings::read(collection).map(drop)
    }

    fn endpoint(&self, collection: &Collection) -> Option<PortalEndpoint> {
        let settings = Settings::read(collection).ok()?;
        Some(PortalEndpoint {
            start: format!("{}/robots.txt", settings.origin),
            origin: settings.origin,
            other_origins: Vec::new(),
            access: settings.transport,
        })
    }

    fn results_url(&self, collection: &Collection, citation: &CitationParts) -> Option<String> {
        let settings = Settings::read(collection).ok()?;
        Some(format!(
            "{}{}",
            settings.origin,
            settings.search_path(citation)
        ))
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
    // A search without a locality (the military registers) keeps every
    // node, whatever place a citation names.
    let wanted = match settings.locality {
        Some(_) => forms(&citation.locality),
        None => vec![String::new()],
    };
    let localities: Vec<&str> = wanted.iter().map(String::as_str).collect();
    let found = search(&settings, citation, &localities, fetch).await?;
    let matches = match narrow(&found.candidates, citation, &localities).as_slice() {
        [only] => return view(&settings, archive, citation, only, fetch).await,
        [] if found.candidates.is_empty() => found.total,
        many => many.len(),
    };
    Ok(ArchiveTarget::Results {
        url: format!("{}{}", settings.origin, settings.search_path(citation)),
        matches: Some(matches),
    })
}
