//! A Ligeo collection's `portal` settings (Archive Portals §4.5): where its
//! search is, which inputs it fills and how its results name what selection
//! compares.

use std::collections::BTreeMap;

use serde::Deserialize;

use super::page::Register;
use crate::catalog::{CatalogError, Collection};
use crate::citation::{Act, CitationParts, CitedPerson};
use crate::platform::markup::fold;
use crate::platform::{Access, Query, is_https_origin};

/// A collection's `portal` settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Settings {
    pub(super) origin: String,
    #[serde(default)]
    pub(super) transport: Access,
    /// `/archive`, or `/archives` on some portals.
    #[serde(default = "default_prefix")]
    pub(super) prefix: String,
    /// The search's name in the path, also sent as `type`: `etatcivil`,
    /// `paroissiaux`, `etatcivil2`.
    pub(super) search: String,
    /// The menu node, `n:<node>` in the path.
    pub(super) node: u32,
    /// The search page's name in the menu, where it differs from the
    /// search's own (`ecalternatif` for `etatcivil`).
    #[serde(default)]
    pub(super) page: Option<String>,
    /// The finding aid the search runs within, whose page holds the form:
    /// `<prefix>/fonds/<fonds>/<search>/n:<node>`.
    #[serde(default)]
    pub(super) fonds: Option<String>,
    /// The results' layout, a path segment before the node, where the
    /// portal's default layout is not the one the settings read (`Tableau`,
    /// `tabulaire`).
    #[serde(default)]
    pub(super) layout: Option<String>,
    pub(super) fields: Fields,
    /// The values of a locality input that takes a fixed list — the
    /// checkboxes of a form's recruitment subdivisions —, each with the
    /// names citations give it: the cited locality is sent as the value it
    /// names, and a locality none names is not sent at all, every value
    /// then searched. Empty for an input sent the cited locality as written.
    #[serde(default)]
    pub(super) localities: BTreeMap<String, Vec<String>>,
    /// Inputs sent with every search, with their values: the department of
    /// a portal two archives share, the kind of register a collection holds.
    #[serde(default)]
    pub(super) params: BTreeMap<String, String>,
    /// The act filter of each act code; none when the form has no act filter.
    #[serde(default)]
    pub(super) acts: BTreeMap<String, ActFilter>,
    #[serde(default)]
    pub(super) columns: Columns,
}

fn default_prefix() -> String {
    "/archive".to_owned()
}

/// The names of the form inputs the adapter fills.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Fields {
    /// The locality input, the recruitment or registration bureau's for a
    /// series; none for a series searched by year alone.
    #[serde(default)]
    pub(super) locality: Option<String>,
    /// The act input, for acts written as one value (`RECH_acte[]=N`).
    #[serde(default)]
    pub(super) act: Option<String>,
    /// The years' inputs, both or neither.
    #[serde(default)]
    pub(super) year_from: Option<String>,
    #[serde(default)]
    pub(super) year_to: Option<String>,
    /// A single year's input in place of the two: an exact year, a select
    /// of census years, checkboxes of years (`RECH_date[]`).
    #[serde(default)]
    pub(super) year: Option<String>,
    /// The input of an index of persons that takes the cited matricule
    /// number, which finds the person's row.
    #[serde(default)]
    pub(super) number: Option<String>,
    /// The surname input of an index of persons (`Rech_nom`), which takes
    /// the cited person's surname only where the other parts cannot single
    /// out the person's row (Archive Portals §4.5, step 1). Only a
    /// collection of series, whose rows name their person
    /// (`columns.person`), may declare it.
    #[serde(default)]
    pub(super) surname: Option<String>,
    /// The input taking the cited call number (`RECH_cote`), which singles
    /// out the register on a portal whose search by locality alone answers
    /// too slowly for its largest communes (a city's registers by section).
    #[serde(default)]
    pub(super) call_number: Option<String>,
    /// Years searched on either side of the cited one in `year_from` and
    /// `year_to`, for a portal whose index dates a register by other years
    /// than its title shows (`mars - décembre 1672` not found for 1672).
    #[serde(default)]
    pub(super) year_margin: u16,
}

impl Fields {
    /// Every input the settings name.
    pub(super) fn all(&self) -> impl Iterator<Item = &String> {
        [
            &self.locality,
            &self.act,
            &self.year_from,
            &self.year_to,
            &self.year,
            &self.number,
            &self.surname,
            &self.call_number,
        ]
        .into_iter()
        .flatten()
    }

    fn check(&self) -> Result<(), CatalogError> {
        if !self.all().all(|name| is_name(name)) {
            return Err(invalid("input names"));
        }
        if self.year_from.is_some() != self.year_to.is_some() {
            return Err(invalid("year_from and year_to go together"));
        }
        if self.year.is_some() && self.year_from.is_some() {
            return Err(invalid("year replaces year_from and year_to"));
        }
        if self.year_margin > 0 && self.year_from.is_none() {
            return Err(invalid("year_margin needs year_from and year_to"));
        }
        Ok(())
    }
}

/// How the form expresses an act.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub(super) enum ActFilter {
    /// The value of the `fields.act` input. An input named `…[]` is a
    /// checkbox list: a combined act repeats it once per kind.
    Value(String),
    /// Inputs of their own, with their values: a document type and an act
    /// glob (`RECH_doc=EC&RECH_acte2=*aissanc*`).
    Params(BTreeMap<String, String>),
}

/// One column's header text or notice label, or several, whose cells are
/// read together: a document type and an act, or the names a column takes
/// in different answers.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(super) enum Names {
    One(String),
    Several(Vec<String>),
}

impl Names {
    pub(super) fn list(&self) -> &[String] {
        match self {
            Self::One(name) => std::slice::from_ref(name),
            Self::Several(names) => names,
        }
    }
}

/// The header texts of the result columns, or the labels of the notices'
/// items, as written by the portal, case, accents and punctuation ignored.
/// A row's locality is read from its `locality` cell, or else from its
/// `title`, which also gives the parish, acts and call number; rows showing
/// neither are all of the locality the search filtered.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Columns {
    #[serde(default)]
    pub(super) locality: Option<Names>,
    #[serde(default)]
    pub(super) title: Option<Names>,
    #[serde(default)]
    pub(super) acts: Option<Names>,
    #[serde(default)]
    pub(super) parish: Option<Names>,
    #[serde(default)]
    pub(super) period: Option<Names>,
    #[serde(default)]
    pub(super) call_number: Option<Names>,
    /// The numbers a register spans, such as the matricules of a military
    /// register (`1 à 1586`), or the one matricule of a person's row.
    #[serde(default)]
    pub(super) numbers: Option<Names>,
    /// The person a row of an index of persons names, `SURNAME, Given,
    /// Given`, compared with the cited person.
    #[serde(default)]
    pub(super) person: Option<Names>,
}

impl Columns {
    /// Every column the settings name.
    pub(super) fn all(&self) -> impl Iterator<Item = &Names> {
        [
            &self.locality,
            &self.title,
            &self.acts,
            &self.parish,
            &self.period,
            &self.call_number,
            &self.numbers,
            &self.person,
        ]
        .into_iter()
        .flatten()
    }

    /// Whether the rows show a locality.
    pub(super) fn locate(&self) -> bool {
        self.locality.is_some() || self.title.is_some()
    }

    fn check(&self) -> Result<(), CatalogError> {
        let blank = self.all().any(|names| {
            names.list().is_empty() || names.list().iter().any(|name| fold(name).is_empty())
        });
        if blank {
            return Err(invalid("column headers must not be blank"));
        }
        Ok(())
    }
}

pub(super) fn invalid(message: &str) -> CatalogError {
    CatalogError::new(format!("ligeo settings: {message}"))
}

/// An input name: letters, digits and `_ - [ ]`.
fn is_name(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-[]".contains(&byte))
}

/// A path segment: letters, digits and `_`.
fn is_segment(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// A form value: no separator or control character a query could not carry
/// once encoded as a single value.
fn is_value(text: &str) -> bool {
    !text.trim().is_empty() && !text.chars().any(char::is_control)
}

impl Settings {
    pub(super) fn read(collection: &Collection) -> Result<Self, CatalogError> {
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
        if !self.prefix.starts_with('/')
            || self.prefix.len() < 2
            || !self.prefix[1..]
                .bytes()
                .all(|byte| byte.is_ascii_lowercase())
        {
            return Err(invalid("prefix must be a path such as /archive"));
        }
        let segments = [&self.page, &self.fonds, &self.layout];
        if !is_segment(&self.search)
            || self.node == 0
            || segments
                .into_iter()
                .flatten()
                .any(|segment| !is_segment(segment))
        {
            return Err(invalid("search and node, page, fonds and layout"));
        }
        self.fields.check()?;
        let localities = self.localities.iter().all(|(value, names)| {
            is_value(value) && names.iter().all(|name| !fold(name).is_empty())
        });
        if !localities {
            return Err(invalid("localities need values and names"));
        }
        if !self.localities.is_empty() && self.fields.locality.is_none() {
            return Err(invalid("localities need fields.locality"));
        }
        let params = self
            .params
            .iter()
            .all(|(name, value)| is_name(name) && is_value(value));
        if !params {
            return Err(invalid("params need input names and values"));
        }
        self.columns.check()
    }

    /// Every act code is valid and has a usable filter, and every act the
    /// collection holds has one, unless the form has no act filter. A
    /// surname input serves only a collection of series whose rows name
    /// their person: an index of persons, never a register of acts.
    fn check_acts(&self, collection: &Collection) -> Result<(), CatalogError> {
        if self.fields.surname.is_some()
            && (self.columns.person.is_none()
                || !collection
                    .acts
                    .iter()
                    .all(|act| matches!(act, Act::Series(_))))
        {
            return Err(invalid(
                "fields.surname serves a collection of series with columns.person",
            ));
        }
        for (code, filter) in &self.acts {
            if Act::from_code(code).is_none() {
                return Err(invalid(&format!("`{code}` is not an act code")));
            }
            let usable = match filter {
                ActFilter::Value(value) => self.fields.act.is_some() && is_value(value),
                ActFilter::Params(params) => {
                    !params.is_empty()
                        && params
                            .iter()
                            .all(|(name, value)| is_name(name) && is_value(value))
                }
            };
            if !usable {
                return Err(invalid(&format!(
                    "the filter of `{code}` needs `fields.act` and a value, or inputs of its own"
                )));
            }
        }
        if self.acts.is_empty() {
            return match self.fields.act {
                Some(_) => Err(invalid("fields.act without acts")),
                None => Ok(()),
            };
        }
        match collection
            .acts
            .iter()
            .find(|act| self.act_filters(act).is_empty())
        {
            Some(act) => Err(invalid(&format!("no filter for `{act}`"))),
            None => Ok(()),
        }
    }

    /// The filters searching `act`: its own entry, or one per kind of a
    /// combined act, publications of banns searched as marriages.
    fn act_filters(&self, act: &Act) -> Vec<&ActFilter> {
        if let Some(filter) = self.acts.get(&act.to_string()) {
            return vec![filter];
        }
        let mut kinds = Vec::new();
        for kind in act.kinds().iter().map(|kind| kind.filed_as()) {
            if !kinds.contains(&kind) {
                kinds.push(kind);
            }
        }
        kinds
            .iter()
            .filter_map(|kind| self.acts.get(&kind.letter().to_string()))
            .collect()
    }

    /// The act filter's inputs: a checkbox list repeated once per kind, a
    /// value shared by two kinds (`naissance|baptême`) sent once, a single
    /// input or inputs of their own holding the first kind.
    fn act_inputs(&self, act: &Act) -> Vec<(&str, &str)> {
        let mut inputs: Vec<(&str, &str)> = Vec::new();
        for filter in self.act_filters(act) {
            match filter {
                ActFilter::Value(value) => {
                    let Some(name) = &self.fields.act else {
                        break;
                    };
                    if !inputs.contains(&(name.as_str(), value.as_str())) {
                        inputs.push((name, value));
                    }
                    // A single-valued input holds one kind.
                    if !name.ends_with("[]") {
                        break;
                    }
                }
                ActFilter::Params(params) => {
                    inputs.extend(
                        params
                            .iter()
                            .map(|(name, value)| (name.as_str(), value.as_str())),
                    );
                    break;
                }
            }
        }
        inputs
    }

    /// Whether the collection searches its persons by the cited person's
    /// surname, and the citation names one.
    pub(super) fn names(&self, citation: &CitationParts) -> bool {
        self.fields.surname.is_some() && citation.person.is_some()
    }

    /// Whether the cited person's surname goes with the first search: where
    /// no cited matricule can find the person's row — the citation gives
    /// none, or the form takes none. Otherwise the matricule is searched
    /// first, and the surname only when its rows do not single out the
    /// person (Archive Portals §4.5, step 1).
    pub(super) fn names_first(&self, citation: &CitationParts) -> bool {
        self.names(citation) && (self.fields.number.is_none() || citation.number.is_none())
    }

    /// The value the locality input is sent: the cited locality as written,
    /// or, for an input taking a fixed list, the value whose names hold it
    /// (`None` when none does: every value searched).
    fn locality_value<'s>(&'s self, cited: &'s str) -> Option<&'s str> {
        if self.localities.is_empty() {
            return Some(cited);
        }
        let wanted = fold(cited);
        if wanted.is_empty() {
            return None;
        }
        self.localities
            .iter()
            .find(|(value, names)| {
                fold(value) == wanted || names.iter().any(|name| fold(name) == wanted)
            })
            .map(|(value, _)| value.as_str())
    }

    /// The filters of a search, shared by the request and the results page:
    /// the locality, the act, the collection's own inputs, the year, an
    /// index's matricule number, the cited person's surname when `named`,
    /// and the cited call number where the form takes one. Nothing else of
    /// the citation leaves the application: never the given names, which
    /// are compared on the rows. The locality is the portal's text match,
    /// and the year is the portal's test: both are re-checked on the rows.
    pub(super) fn filters(&self, citation: &CitationParts, named: bool) -> Query {
        let mut query = Query::new();
        if let Some(input) = &self.fields.locality
            && let Some(value) = self.locality_value(&citation.locality)
        {
            query.push(input, value);
        }
        for (name, value) in self.act_inputs(&citation.act) {
            query.push(name, value);
        }
        for (name, value) in &self.params {
            query.push(name, value.as_str());
        }
        if let Some(year) = citation.year {
            let fields = &self.fields;
            let margin = fields.year_margin;
            match (&fields.year_from, &fields.year_to, &fields.year) {
                (Some(from), Some(to), _) => {
                    query
                        .push(from, year.saturating_sub(margin).to_string())
                        .push(to, year.saturating_add(margin).to_string());
                }
                (_, _, Some(single)) => {
                    query.push(single, year.to_string());
                }
                _ => {}
            }
        }
        if let (Some(name), Some(number)) = (&self.fields.number, citation.number) {
            query.push(name, number.to_string());
        }
        if let (true, Some(input), Some(CitedPerson { surname, .. })) =
            (named, &self.fields.surname, &citation.person)
        {
            query.push(input, surname.as_str());
        }
        if let (Some(name), Some(call_number)) = (&self.fields.call_number, &citation.call_number) {
            query.push(name, call_number.as_str());
        }
        query.push("type", self.search.as_str());
        query
    }

    pub(super) fn results_path(&self, filters: &Query) -> String {
        let place = match (&self.fonds, &self.layout) {
            (Some(fonds), _) => format!("fonds/{fonds}/{}", self.search),
            (None, Some(layout)) => format!("resultats/{}/{layout}", self.search),
            (None, None) => format!("resultats/{}", self.search),
        };
        format!("{}/{place}/n:{}?{filters}", self.prefix, self.node)
    }

    /// The path of the page holding the search form.
    pub(super) fn form_path(&self) -> String {
        match &self.fonds {
            Some(fonds) => format!("{}/fonds/{fonds}", self.prefix),
            None => format!(
                "{}/recherche/{}/n:{}",
                self.prefix,
                self.page.as_ref().unwrap_or(&self.search),
                self.node
            ),
        }
    }

    pub(super) fn search_page(&self) -> String {
        format!("{}{}", self.origin, self.form_path())
    }

    /// `<ark>/<tag>/<group>/<view>`, view one-based, on the portal's origin.
    pub(super) fn view_url(&self, register: &Register, view: u16) -> String {
        format!("{}{}/{view}", self.origin, register.viewer())
    }

    /// An image on the portal's own origin rather than on the host the
    /// manifest declares.
    pub(super) fn on_origin(&self, address: &str, prefix: &str) -> Option<String> {
        let path = super::page::path_of(address)?;
        path.starts_with(prefix)
            .then(|| format!("{}{path}", self.origin))
    }
}
