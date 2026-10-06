//! Mnesys Expo (Naoned), the publishing software of many departmental
//! archives, Indre-et-Loire, Calvados and Marne among them.
//!
//! A portal has one search form per collection: one for its parish
//! registers and civil status, usually, and one per series — censuses,
//! military registers, conscription lists, tables of successions. Every form
//! is a plain `GET` of `/search/results` filtered by its own inputs, which
//! the settings name: the locality's exact label, the act's label, the year
//! or a period, each where the form has it. The answer's rows are selected
//! down to one register (`select`), whose image is addressed by its own ARK,
//! `/ark:/<naan>/<name>/<image id>`: the portal's viewer opens on it, and it
//! is also the view's persistent address. Archive Portals §4.4 specifies the
//! requests.

#[cfg(any(test, feature = "live"))]
mod live;
mod page;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use serde::Deserialize;

use super::iiif::image_info;
use super::locality::{LocalityStyle, forms};
use super::markup::{self, fold};
use super::select::{Candidate, Selection, number_range, select};
use super::view::{cited_views, view_target};
use super::{Access, BoxFuture, Platform, PortalEndpoint, Query, is_https_origin};
use crate::catalog::{Archive, CatalogError, Collection, Display};
use crate::citation::{Act, ActKind, CallNumber, CitationParts, Series};
use crate::transport::PortalFetch;
use crate::{ArchiveImage, ArchiveTarget, ArchiveView, ResolveError};

/// The rows one results page asks for: the portal serves 20, 40 or 80.
const RESULTS_PER_PAGE: usize = 80;

/// The widest run of images one viewer request asks for.
const MAX_WINDOW: u16 = 10;

/// The Mnesys adapter.
pub struct Mnesys;

/// A collection's `portal` settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    origin: String,
    #[serde(default)]
    transport: Access,
    /// The search form's UUID.
    form: String,
    fields: Fields,
    /// The patterns of a locality's value, with `{locality}`. The portal
    /// needs the exact label, and its labels differ for communes that still
    /// exist and communes since merged: every pattern is sent, and a label
    /// the portal does not know is ignored. With `locality_lookup`, a `*`
    /// after `{locality}` stands for any text (`{locality} (ancienne
    /// commune*, Somme, France)`).
    #[serde(default)]
    locality_label: Vec<String>,
    #[serde(default)]
    locality_style: LocalityStyle,
    /// Whether the labels sent are those of the form's own locality list
    /// that name the cited locality, case, accents and punctuation ignored,
    /// rather than the patterns filled in: one more request, for a list
    /// whose labels the citation cannot spell (capitals without accents,
    /// dated former communes).
    #[serde(default)]
    locality_lookup: bool,
    /// The value of the year input, with `{year}`, where the form lists
    /// classes by label (`Classe {year}.`); the bare year otherwise.
    #[serde(default)]
    year_label: Option<String>,
    /// The portal's labels of each document code: the act filter's values
    /// where the form has one, and in any case the words that tell a row's
    /// acts and tables. Several codes may share a label.
    #[serde(default)]
    acts: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    call_number: CallNumberSource,
    image_source: ImageSource,
    /// What the collection's rows may be, from its document kinds.
    #[serde(skip)]
    holding: Holding,
}

/// The names of the form's inputs, which carry a prefix per portal
/// (`0-controlledAccessGeographicName[]`), each where the form has it.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Fields {
    #[serde(default)]
    locality: Option<String>,
    #[serde(default)]
    act: Option<String>,
    #[serde(default)]
    year: Option<String>,
    /// A period's bounds, both sent as the cited year: the form returns the
    /// registers whose period contains it.
    #[serde(default)]
    period_begin: Option<String>,
    #[serde(default)]
    period_end: Option<String>,
}

impl Fields {
    fn names(&self) -> impl Iterator<Item = &String> {
        [
            &self.locality,
            &self.act,
            &self.year,
            &self.period_begin,
            &self.period_end,
        ]
        .into_iter()
        .flatten()
    }
}

/// Where the rows show the register's call number.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CallNumberSource {
    /// The `Cote` cell.
    #[default]
    Row,
    /// The `Cote` cell, or else the context entry before the title.
    Context,
    /// The portal shows none: the cited call number cannot select a row.
    None,
}

/// Where the images of a register are listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ImageSource {
    /// The register's IIIF manifest: every image with its pixel size, in one
    /// request that grows with the register (hundreds of kilobytes).
    Manifest,
    /// The viewer's own endpoint, asked for the cited images only; the pixel
    /// size of an image comes from its `info.json`.
    Visualizer,
}

/// What the rows of a collection may be: every row is of its one series,
/// or a table where it holds tables only, a register where it holds
/// registers only; otherwise a row is read for what it is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Holding {
    Series(Series),
    Tables,
    Registers,
    #[default]
    Mixed,
}

impl Holding {
    fn of(collection: &Collection) -> Self {
        let mut series = collection.series();
        if let (Some(first), true) = (series.next(), collection.acts.iter().all(is_series)) {
            if series.all(|other| other == first) {
                return Self::Series(first);
            }
            return Self::Mixed;
        }
        let tables = collection.acts.iter().filter(|act| is_table(act)).count();
        match tables {
            0 if !collection.acts.iter().any(is_series) => Self::Registers,
            all if all == collection.acts.len() => Self::Tables,
            _ => Self::Mixed,
        }
    }
}

fn is_series(act: &Act) -> bool {
    matches!(act, Act::Series(_))
}

fn is_table(act: &Act) -> bool {
    matches!(act, Act::Table(_))
}

fn invalid(message: &str) -> CatalogError {
    CatalogError::new(format!("mnesys settings: {message}"))
}

fn unexpected(detail: &str) -> ResolveError {
    ResolveError::UnexpectedResponse(format!("mnesys: {detail}"))
}

/// An input name: letters, digits, `-`, `_` and the `[]` of a repeatable one.
fn is_field_name(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_[]".contains(&byte))
}

/// A form UUID: five hexadecimal groups of 8, 4, 4, 4 and 12 digits.
fn is_uuid(text: &str) -> bool {
    let groups: Vec<&str> = text.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(group, len)| group.len() == len && group.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// The words of a text that identify an act: folded, at least four letters,
/// plural marks removed.
fn words(text: &str) -> Vec<String> {
    fold(text)
        .split(' ')
        .filter(|word| word.len() >= 4)
        .map(|word| word.strip_suffix('s').unwrap_or(word).to_owned())
        .collect()
}

/// The act kinds a title writes as letter codes — `BMS 1739-1750`,
/// `N (1849-1852)`, `EXAMPLEVILLE / BMS-NMD [1700-1800]` —, as one code: each
/// word of capitals that is an act code adds its kinds.
fn title_codes(title: &str) -> Option<String> {
    let mut kinds: Vec<ActKind> = Vec::new();
    for token in title.split(|c: char| !c.is_alphanumeric()) {
        if token.is_empty() || !token.chars().all(|c| c.is_ascii_uppercase()) {
            continue;
        }
        if let Some(Act::Register(found)) = Act::from_code(token) {
            for kind in found {
                if !kinds.contains(&kind) {
                    kinds.push(kind);
                }
            }
        }
    }
    (!kinds.is_empty()).then(|| kinds.iter().map(|kind| kind.letter()).collect())
}

/// The localities `label` names under `pattern`: the texts in place of
/// `{locality}`, longest first, one for each way the pattern's `*` may
/// match. `Exampleville (ancienne commune av. 1790, Somme, France)` names
/// `Exampleville` under `{locality} (ancienne commune*, Somme, France)`.
fn named_by<'l>(pattern: &str, label: &'l str) -> Vec<&'l str> {
    let Some((before, after)) = pattern.split_once("{locality}") else {
        return Vec::new();
    };
    let Some(rest) = label.strip_prefix(before) else {
        return Vec::new();
    };
    let parts: Vec<&str> = after.split('*').collect();
    (1..=rest.len())
        .rev()
        .filter(|end| rest.is_char_boundary(*end) && matches_parts(&rest[*end..], &parts))
        .map(|end| &rest[..end])
        .collect()
}

/// Whether `text` is `parts` in order with any texts between them.
fn matches_parts(text: &str, parts: &[&str]) -> bool {
    let Some((first, others)) = parts.split_first() else {
        return text.is_empty();
    };
    let Some(mut rest) = text.strip_prefix(first) else {
        return false;
    };
    let Some((last, middle)) = others.split_last() else {
        return rest.is_empty();
    };
    for part in middle {
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    rest.ends_with(last)
}

impl Settings {
    fn read(collection: &Collection) -> Result<Self, CatalogError> {
        let mut settings =
            Self::deserialize(&collection.portal).map_err(|error| invalid(&error.to_string()))?;
        settings.holding = Holding::of(collection);
        settings.check()?;
        settings.check_localities()?;
        settings.check_acts(collection)?;
        Ok(settings)
    }

    fn check(&self) -> Result<(), CatalogError> {
        if !is_https_origin(&self.origin) {
            return Err(invalid("origin must be an https origin"));
        }
        if !is_uuid(&self.form) {
            return Err(invalid("form must be a UUID"));
        }
        if !self.fields.names().all(|name| is_field_name(name)) {
            return Err(invalid("input names"));
        }
        if self.fields.period_begin.is_some() != self.fields.period_end.is_some() {
            return Err(invalid("period_begin and period_end go together"));
        }
        if let Some(label) = &self.year_label
            && (self.fields.year.is_none() || label.matches("{year}").count() != 1)
        {
            return Err(invalid("year_label needs a year input and one {year}"));
        }
        Ok(())
    }

    fn check_localities(&self) -> Result<(), CatalogError> {
        if self.fields.locality.is_none() {
            return match self.locality_label.is_empty() && !self.locality_lookup {
                true => Ok(()),
                false => Err(invalid(
                    "locality_label and locality_lookup need a locality input",
                )),
            };
        }
        if self.locality_label.is_empty()
            || !self
                .locality_label
                .iter()
                .all(|label| label.matches("{locality}").count() == 1)
        {
            return Err(invalid("each locality label needs one {locality}"));
        }
        let misplaced_star = self.locality_label.iter().any(|label| {
            label
                .split("{locality}")
                .next()
                .is_some_and(|before| before.contains('*'))
                || (label.contains('*') && !self.locality_lookup)
        });
        if misplaced_star {
            return Err(invalid(
                "a `*` in a locality label needs locality_lookup, after {locality}",
            ));
        }
        Ok(())
    }

    /// Every document code is valid and has labels; every kind the act
    /// filter searches has some, and where the form has no act filter, so
    /// do the tables of a collection that also holds registers, which tell
    /// its rows apart.
    fn check_acts(&self, collection: &Collection) -> Result<(), CatalogError> {
        for (code, labels) in &self.acts {
            if Act::from_code(code).is_none() {
                return Err(invalid(&format!("`{code}` is not an act code")));
            }
            if labels.is_empty() || labels.iter().any(|label| label.trim().is_empty()) {
                return Err(invalid(&format!("`{code}` has an empty label")));
            }
        }
        let needs_labels = |act: &&Act| match self.fields.act {
            Some(_) => true,
            None => self.holding == Holding::Mixed && is_table(act),
        };
        match collection
            .acts
            .iter()
            .filter(needs_labels)
            .find(|act| self.act_labels(act).is_empty())
        {
            Some(act) => Err(invalid(&format!("no label for `{act}`"))),
            None => Ok(()),
        }
    }

    /// The labels of an act: its own, or for a combined act (`BMS`) those of
    /// its first kind, whose category holds the mixed registers.
    fn act_labels(&self, act: &Act) -> &[String] {
        self.acts
            .get(&act.to_string())
            .or_else(|| {
                let first = act.primary_kind()?;
                self.acts.get(&first.letter().to_string())
            })
            .map_or(&[], Vec::as_slice)
    }

    /// The locality as the portal writes it.
    fn locality(&self, citation: &CitationParts) -> String {
        self.locality_style.write(&citation.locality)
    }

    /// The folded forms of the cited locality a row or a label may show.
    fn locality_forms(&self, citation: &CitationParts) -> Vec<String> {
        let mut folded: Vec<String> = forms(&citation.locality)
            .iter()
            .chain([&self.locality(citation)])
            .map(|form| fold(form))
            .filter(|form| !form.is_empty())
            .collect();
        folded.dedup();
        folded
    }

    /// The locality labels the patterns spell, without a lookup: those
    /// without a `*`, filled in with the locality in the portal's style.
    fn built_labels(&self, citation: &CitationParts) -> Vec<String> {
        if citation.locality.is_empty() || self.fields.locality.is_none() {
            return Vec::new();
        }
        let locality = self.locality(citation);
        self.locality_label
            .iter()
            .filter(|pattern| !pattern.contains('*'))
            .map(|pattern| pattern.replace("{locality}", &locality))
            .collect()
    }

    /// The labels of the form's locality list that name the cited locality
    /// under one of the patterns.
    fn listed_labels(&self, options: &[String], citation: &CitationParts) -> Vec<String> {
        let wanted = self.locality_forms(citation);
        options
            .iter()
            .filter(|label| {
                self.locality_label.iter().any(|pattern| {
                    named_by(pattern, label)
                        .into_iter()
                        .any(|name| wanted.contains(&fold(name)))
                })
            })
            .cloned()
            .collect()
    }

    /// Whether the search sends the labels of the form's own list.
    fn looks_up(&self, citation: &CitationParts) -> bool {
        self.locality_lookup && !citation.locality.is_empty()
    }

    fn form_path(&self) -> String {
        format!("/search/form/{}", self.form)
    }

    /// The locality labels a search sends: the patterns filled in, or with
    /// `locality_lookup` those of the form's list naming the locality —
    /// `None` when the list names it nowhere.
    async fn labels(
        &self,
        citation: &CitationParts,
        fetch: &dyn PortalFetch,
    ) -> Result<Option<Vec<String>>, ResolveError> {
        let Some(field) = self.fields.locality.as_deref() else {
            return Ok(Some(Vec::new()));
        };
        if !self.looks_up(citation) {
            return Ok(Some(self.built_labels(citation)));
        }
        let form = fetch.get(&self.form_path()).await?;
        let options = page::options(&form, field).ok_or_else(|| {
            markup::unreadable(
                &form,
                "mnesys: the search form lacks the locality list".to_owned(),
            )
        })?;
        let labels = self.listed_labels(&options, citation);
        Ok((!labels.is_empty()).then_some(labels))
    }

    /// The filters of a search, shared by the request and the search page:
    /// the locality's labels, the act's labels, and the year in each input
    /// that takes it. Nothing else of the citation leaves the application.
    fn filters(&self, citation: &CitationParts, labels: &[String]) -> Query {
        let mut query = Query::new();
        query
            .push("formUuid", self.form.as_str())
            .push("mode", "list")
            .push("sort", "date_asc");
        if let Some(field) = &self.fields.locality {
            for label in labels {
                query.push(field.as_str(), label.as_str());
            }
        }
        if let Some(field) = &self.fields.act {
            for label in self.act_labels(&citation.act) {
                query.push(field.as_str(), label.as_str());
            }
        }
        if let Some(year) = citation.year {
            if let Some(field) = &self.fields.year {
                let value = self.year_label.as_ref().map_or_else(
                    || year.to_string(),
                    |label| label.replace("{year}", &year.to_string()),
                );
                query.push(field.as_str(), value);
            }
            for field in [&self.fields.period_begin, &self.fields.period_end]
                .into_iter()
                .flatten()
            {
                query.push(field.as_str(), year.to_string());
            }
        }
        query
    }

    fn search_request(&self, filters: &Query) -> String {
        format!("/search/results?{filters}&resultsPerPage={RESULTS_PER_PAGE}")
    }

    fn search_page(&self, filters: &Query) -> String {
        format!("{}/search/results?{filters}", self.origin)
    }

    /// The results page of a search.
    async fn search(
        &self,
        filters: &Query,
        fetch: &dyn PortalFetch,
    ) -> Result<page::Results, ResolveError> {
        let answer = fetch.get(&self.search_request(filters)).await?;
        page::results(&answer, RESULTS_PER_PAGE).map_err(|error| {
            if markup::is_challenge(&answer) {
                ResolveError::Challenged
            } else {
                error
            }
        })
    }

    /// The words that mark a table among a row's texts: those of the table
    /// acts' labels that no other label holds (`Tables décennales des
    /// naissances` marks a table by `table` and `décennale`, not by
    /// `naissances`).
    fn table_words(&self) -> Vec<String> {
        let (tables, others): (Vec<_>, Vec<_>) = self
            .acts
            .iter()
            .partition(|(code, _)| matches!(Act::from_code(code), Some(Act::Table(_))));
        let label_words = |acts: Vec<(&String, &Vec<String>)>| -> Vec<String> {
            acts.into_iter()
                .flat_map(|(_, labels)| labels.iter().flat_map(|label| words(label)))
                .collect()
        };
        let others = label_words(others);
        label_words(tables)
            .into_iter()
            .filter(|word| !others.contains(word))
            .collect()
    }

    /// Whether a row is a table: every row of a collection of tables, none
    /// of one without, and otherwise a row whose text holds a table word.
    fn row_is_table(&self, row_words: &[String], table_words: &[String]) -> bool {
        match self.holding {
            Holding::Tables => true,
            Holding::Series(_) | Holding::Registers => false,
            Holding::Mixed => row_words.iter().any(|word| table_words.contains(word)),
        }
    }

    /// The acts of a row, written as a code: the collection's series, or the
    /// single-kind acts whose labels' words the row's words hold, or else the
    /// act codes its title writes.
    fn row_act(&self, row_words: &[String], title: &str) -> Option<String> {
        if let Holding::Series(series) = self.holding {
            return Some(series.code().to_owned());
        }
        let kinds: String = self
            .acts
            .iter()
            .filter_map(|(code, labels)| match Act::from_code(code)? {
                Act::Register(kinds) if kinds.len() == 1 => Some((kinds[0], labels)),
                _ => None,
            })
            .filter(|(_, labels)| {
                labels
                    .iter()
                    .flat_map(|label| words(label))
                    .any(|word| row_words.contains(&word))
            })
            .map(|(kind, _): (ActKind, _)| kind.letter())
            .collect();
        (!kinds.is_empty())
            .then_some(kinds)
            .or_else(|| title_codes(title))
    }

    /// The call number a row shows: the one the citation cites where the
    /// row shows several, its first otherwise.
    fn row_call_number(&self, row: &page::Row, cited: Option<&CallNumber>) -> Option<String> {
        let mut shown: Vec<&str> = row.call_numbers.iter().map(String::as_str).collect();
        match self.call_number {
            CallNumberSource::None => return None,
            CallNumberSource::Context if shown.is_empty() => {
                // The last entry repeats the title.
                shown.extend(
                    row.context
                        .len()
                        .checked_sub(2)
                        .map(|at| row.context[at].as_str()),
                );
            }
            _ => {}
        }
        cited
            .and_then(|cited| shown.iter().find(|number| cited.matches(number)))
            .or(shown.first())
            .map(|number| (*number).to_owned())
    }

    /// The locality of a row as selection compares it: the cited locality
    /// where a context entry or the title is the locality, as written or as
    /// one of the settings' labels (`Exampleville (Department, France)`),
    /// where the labels sent are the portal's own, or for a series where the
    /// collection, a context entry or the title names it among other words
    /// (`Bureau de l'Enregistrement d'Exampleville`); the first context
    /// entry otherwise.
    fn row_locality(
        &self,
        row: &page::Row,
        citation: &CitationParts,
        forms: &[String],
    ) -> Option<String> {
        let is_locality = |entry: &String| {
            forms.contains(&fold(entry))
                || self.locality_label.iter().any(|pattern| {
                    named_by(pattern, entry)
                        .into_iter()
                        .any(|name| forms.contains(&fold(name)))
                })
        };
        let names = |entry: &String| {
            let entry = format!(" {} ", fold(entry));
            forms
                .iter()
                .any(|form| entry.contains(&format!(" {form} ")))
        };
        let mut entries = row
            .collection
            .iter()
            .chain(&row.context)
            .chain([&row.title]);
        if row.context.iter().chain([&row.title]).any(is_locality)
            || self.looks_up(citation)
            || (is_series(&citation.act) && entries.any(names))
        {
            return Some(citation.locality.clone());
        }
        row.context.first().cloned()
    }

    /// The registers of a results page as selection candidates, without the
    /// rows the cited act excludes. The act filter is not exclusive — a
    /// search for births also returns the births' decennial tables — so a row
    /// is read for what it is: a table, or a register of the acts its text
    /// mentions.
    fn candidates(
        &self,
        rows: Vec<page::Row>,
        citation: &CitationParts,
    ) -> Vec<Candidate<Register>> {
        let forms = self.locality_forms(citation);
        let table_words = self.table_words();
        let wants_table = is_table(&citation.act);
        let parish = citation.parish.as_deref().map(fold);
        let mut candidates: Vec<Candidate<Register>> = rows
            .into_iter()
            .filter_map(|row| {
                let text = std::iter::once(row.title.as_str())
                    .chain(row.collection.as_deref())
                    .chain(row.context.iter().map(String::as_str))
                    .collect::<Vec<_>>()
                    .join(" ");
                let row_words = words(&text);
                (self.row_is_table(&row_words, &table_words) == wants_table)
                    .then_some((row, row_words))
            })
            .map(|(row, row_words)| Candidate {
                locality: self.row_locality(&row, citation, &forms),
                call_number: self.row_call_number(&row, citation.call_number.as_ref()),
                act: self.row_act(&row_words, &row.title),
                parish: parish.as_ref().and_then(|parish| {
                    row.context
                        .iter()
                        .find(|entry| {
                            let entry = fold(entry);
                            entry == *parish || entry.ends_with(&format!(" {parish}"))
                        })
                        .cloned()
                }),
                period: row.period,
                images: row.images,
                numbers: std::iter::once(&row.title)
                    .chain(&row.context)
                    .find_map(|text| number_range(text, true)),
                payload: Register { ark: row.ark },
            })
            .collect();
        // A form without a locality input lists every bureau: rows naming
        // the cited one are kept, and every row when none does.
        let at_locality = |candidate: &Candidate<Register>| {
            candidate
                .locality
                .as_deref()
                .is_some_and(|locality| forms.contains(&fold(locality)))
        };
        if self.fields.locality.is_none() && !candidates.iter().any(at_locality) {
            for candidate in &mut candidates {
                candidate.locality = Some(citation.locality.clone());
            }
        }
        candidates
    }

    /// The image count of a register whose row shows none, or one over
    /// several lots: the viewer's state for its first image counts the
    /// images of that image's lot.
    async fn viewer_count(
        &self,
        ark: &page::Ark,
        fetch: &dyn PortalFetch,
    ) -> Result<u16, ResolveError> {
        let answer = fetch
            .get(&format!(
                "/visualizer/api?arkName={}&uuid={}",
                ark.name, ark.first_image
            ))
            .await?;
        page::viewer_count(&answer)
    }
}

/// What the adapter keeps of a register to open it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Register {
    /// `None` for a register listed without images.
    ark: Option<page::Ark>,
}

impl Platform for Mnesys {
    fn id(&self) -> &'static str {
        "mnesys"
    }

    fn validate(&self, collection: &Collection) -> Result<(), CatalogError> {
        Settings::read(collection).map(drop)
    }

    fn endpoint(&self, collection: &Collection) -> Option<PortalEndpoint> {
        let settings = Settings::read(collection).ok()?;
        Some(PortalEndpoint {
            start: format!("{}{}", settings.origin, settings.form_path()),
            origin: settings.origin,
            other_origins: Vec::new(),
            access: settings.transport,
            insecure_http: false,
        })
    }

    fn results_url(&self, collection: &Collection, citation: &CitationParts) -> Option<String> {
        let settings = Settings::read(collection).ok()?;
        let labels = settings.built_labels(citation);
        Some(settings.search_page(&settings.filters(citation, &labels)))
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
    // A locality the form's list does not name has no register here.
    let Some(labels) = settings.labels(citation, fetch).await? else {
        let filters = settings.filters(citation, &settings.built_labels(citation));
        return Ok(ArchiveTarget::Results {
            url: settings.search_page(&filters),
            matches: Some(0),
        });
    };
    let filters = settings.filters(citation, &labels);
    let results = |matches| ArchiveTarget::Results {
        url: settings.search_page(&filters),
        matches: Some(matches),
    };

    let page = settings.search(&filters, fetch).await?;
    // Rows beyond the page are unseen: a twin of the selected one may be
    // among them, so the search page is the answer.
    if page.total > page.rows.len() {
        return Ok(results(page.total));
    }

    let candidates = settings.candidates(page.rows, citation);
    // Rows showing no call number cannot be told apart by the cited one.
    let without_call_number;
    let cited = if settings.call_number == CallNumberSource::None && citation.call_number.is_some()
    {
        without_call_number = CitationParts {
            call_number: None,
            ..citation.clone()
        };
        &without_call_number
    } else {
        citation
    };
    let styled = settings.locality(citation);
    let row = match select(&candidates, cited, &[&citation.locality, &styled]) {
        Selection::One(row) => row,
        Selection::Many(matches) => return Ok(results(matches)),
    };
    // A register listed without images cannot be opened on a view.
    let Some(ark) = &row.payload.ark else {
        return Ok(results(1));
    };
    let image_count = match row.images {
        Some(count) => count,
        None => settings.viewer_count(ark, fetch).await?,
    };
    if image_count == 0 {
        return Ok(results(1));
    }
    let views = views(
        archive,
        &settings,
        citation,
        (ark, row.period.as_deref()),
        image_count,
        fetch,
    )
    .await?;
    Ok(view_target(
        archive,
        citation,
        row.call_number.as_deref(),
        usize::from(image_count),
        ark.image(&settings.origin, &ark.first_image),
        views,
    ))
}

/// The cited views of a register of `image_count` images.
async fn views(
    archive: &Archive,
    settings: &Settings,
    citation: &CitationParts,
    (ark, period): (&page::Ark, Option<&str>),
    image_count: u16,
    fetch: &dyn PortalFetch,
) -> Result<Vec<ArchiveView>, ResolveError> {
    let cited_views = cited_views(citation, usize::from(image_count), period);
    let mut numbers: Vec<u16> = cited_views.iter().map(|view| view.view).collect();
    numbers.sort_unstable();
    numbers.dedup();
    let images = images(settings, ark, &numbers, fetch).await?;

    let mut views = Vec::with_capacity(numbers.len());
    for (view, image) in numbers.into_iter().zip(images) {
        let address = ark.image(&settings.origin, &image.id);
        views.push(ArchiveView {
            view,
            url: address.clone(),
            ark: Some(address),
            image: match archive.display {
                Display::Iiif => Some(iiif_image(settings, ark, &image, fetch).await?),
                Display::Portal => None,
            },
        });
    }
    Ok(views)
}

/// One cited image: its identifier and, when the source gives it, its size.
struct Image {
    id: String,
    size: Option<(u32, u32)>,
}

/// The identifiers of the cited views' images, in order.
async fn images(
    settings: &Settings,
    ark: &page::Ark,
    views: &[u16],
    fetch: &dyn PortalFetch,
) -> Result<Vec<Image>, ResolveError> {
    if views.is_empty() {
        return Ok(Vec::new());
    }
    match settings.image_source {
        ImageSource::Manifest => {
            let answer = fetch
                .get(&format!(
                    "/iiif/ark:/{}/{}/manifest.json",
                    ark.naan, ark.name
                ))
                .await?;
            let canvases = page::manifest_canvases(&answer)?;
            views
                .iter()
                .map(|view| {
                    let canvas = canvases.get(usize::from(*view) - 1).ok_or_else(|| {
                        unexpected("the manifest lists fewer images than the row")
                    })?;
                    Ok(Image {
                        id: canvas.image.clone(),
                        size: Some((canvas.width, canvas.height)),
                    })
                })
                .collect()
        }
        ImageSource::Visualizer => {
            let mut images = Vec::with_capacity(views.len());
            for (start, end) in windows(views) {
                let answer = fetch
                    .get(&format!(
                        "/visualizer/api?arkName={}&start={}&end={}&group=0",
                        ark.name,
                        start - 1,
                        end - 1
                    ))
                    .await?;
                for view in views.iter().filter(|view| (start..=end).contains(view)) {
                    images.push(Image {
                        id: page::window_image(&answer, start - 1, view - 1)?,
                        size: None,
                    });
                }
            }
            Ok(images)
        }
    }
}

/// The one-based `(first, last)` views of the requests that cover `views`,
/// ascending: neighbouring views share a request, up to [`MAX_WINDOW`].
fn windows(views: &[u16]) -> Vec<(u16, u16)> {
    let mut windows: Vec<(u16, u16)> = Vec::new();
    for view in views {
        match windows.last_mut() {
            Some((start, end)) if view - *start < MAX_WINDOW => *end = *view,
            _ => windows.push((*view, *view)),
        }
    }
    windows
}

/// The image of one view, for a `display: "iiif"` archive. The service is
/// level 0 and serves the whole image only, so the picture is the full image
/// and the thumbnail is the portal's own, a few kilobytes. The size comes
/// from the manifest, or from the image's `info.json`.
async fn iiif_image(
    settings: &Settings,
    ark: &page::Ark,
    image: &Image,
    fetch: &dyn PortalFetch,
) -> Result<ArchiveImage, ResolveError> {
    let path = format!("/iiif/ark:/{}/{}/{}", ark.naan, ark.name, image.id);
    let thumbnail = format!("{}/images/{}_thumbnail.jpg", settings.origin, image.id);
    let base = format!("{}{path}", settings.origin);
    Ok(match image.size {
        Some((width, height)) => ArchiveImage {
            picture: format!("{base}/full/max/0/default.jpg"),
            thumbnail,
            width,
            height,
        },
        None => ArchiveImage {
            thumbnail,
            ..image_info(&fetch.get(&format!("{path}/info.json")).await?)?.image(&base)
        },
    })
}
