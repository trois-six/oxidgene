//! A collection's `portal` settings: the engine, its filters, the act filter
//! values and the result cells, checked when the catalogue loads, and the
//! queries they make.

use std::collections::BTreeMap;

use serde::Deserialize;

use super::SEARCH_PATH;
use super::page::Cell;

/// The rows of the search page a reader lands on.
const READER_PAGE_SIZE: &str = "25";
use crate::catalog::{CatalogError, Collection};
use crate::citation::{Act, CitationParts};
use crate::platform::locality::LocalityStyle;
use crate::platform::{Access, Query, is_https_origin};

/// A collection's `portal` settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Settings {
    pub(super) origin: String,
    #[serde(default)]
    pub(super) transport: Access,
    /// The collection's search page, where records and views open.
    pub(super) search_path: String,
    /// The engine's unique reference, `arko_default_…`.
    pub(super) engine: String,
    /// The search component's content identifiers.
    pub(super) content_ids: Vec<String>,
    /// The list display mode, whose rows carry the cells read below.
    pub(super) display_mode: String,
    #[serde(default)]
    pub(super) fields: Fields,
    /// The act filter value or values of each document code, as the portal
    /// writes them: `Baptêmes[[arko_fiche_…]]` for a value of a list with
    /// its record key, without which a list matches nothing, or a plain
    /// value of a text filter (`Registre matricule`). Several values are
    /// searched together, any of them matching. Empty for an engine without
    /// an act filter.
    #[serde(default)]
    pub(super) acts: BTreeMap<String, Values>,
    #[serde(default)]
    pub(super) locality_style: LocalityStyle,
    #[serde(default)]
    pub(super) cells: Cells,
}

/// The engine's filters the search sends, each optional.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Fields {
    /// The commune, or for a series the office that kept it: a recruitment
    /// or registration bureau.
    #[serde(default)]
    pub(super) locality: Option<Filter>,
    #[serde(default)]
    pub(super) act: Option<Filter>,
    /// The year, or for a military series the class.
    #[serde(default)]
    pub(super) period: Option<Filter>,
}

/// One filter of the engine: its reference alone, or with how the engine
/// takes its value.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(from = "FilterSetting")]
pub(super) struct Filter {
    pub(super) reference: String,
    /// Default `popup` for the locality, `select` for the act, `slider`
    /// for the period.
    pub(super) mode: Option<Mode>,
    /// The values need their record keys, read from the engine's list of
    /// them: `Exampleville[[arko_fiche_…]]`, `Classe 1911[[…]]`.
    pub(super) keyed: bool,
    /// A period searched by two inputs: the reference of the last year's.
    pub(super) end: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum FilterSetting {
    Reference(String),
    Detailed(DetailedFilter),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DetailedFilter {
    #[serde(rename = "ref")]
    reference: String,
    #[serde(default)]
    mode: Option<Mode>,
    #[serde(default)]
    keyed: bool,
    #[serde(default)]
    end: Option<String>,
}

impl From<FilterSetting> for Filter {
    fn from(setting: FilterSetting) -> Self {
        match setting {
            FilterSetting::Reference(reference) => Self {
                reference,
                mode: None,
                keyed: false,
                end: None,
            },
            FilterSetting::Detailed(filter) => Self {
                reference: filter.reference,
                mode: filter.mode,
                keyed: filter.keyed,
                end: filter.end,
            },
        }
    }
}

/// How a filter takes its value, the request's `[extras][mode]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Mode {
    Popup,
    Select,
    /// A range, `<year>|<year>`.
    Slider,
    Input,
    Autocomplete,
}

impl Mode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Popup => "popup",
            Self::Select => "select",
            Self::Slider => "slider",
            Self::Input => "input",
            Self::Autocomplete => "autocomplete",
        }
    }
}

/// One act filter value, or several.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(from = "OneOrMany<String>")]
pub(super) struct Values(pub(super) Vec<String>);

/// One cell, or several whose texts are read together: the first and last
/// numbers of a register in two cells.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(from = "OneOrMany<Cell>")]
pub(super) struct CellList(pub(super) Vec<Cell>);

#[derive(Deserialize)]
#[serde(untagged)]
enum OneOrMany<T> {
    One(T),
    Many(Vec<T>),
}

impl<T> OneOrMany<T> {
    fn into_vec(self) -> Vec<T> {
        match self {
            Self::One(value) => vec![value],
            Self::Many(values) => values,
        }
    }
}

impl From<OneOrMany<String>> for Values {
    fn from(values: OneOrMany<String>) -> Self {
        Self(values.into_vec())
    }
}

impl From<OneOrMany<Cell>> for CellList {
    fn from(cells: OneOrMany<Cell>) -> Self {
        Self(cells.into_vec())
    }
}

/// Where the result rows show what selection reads (`page::Cell`): a
/// `data-champ` name, `#<n>` for the n-th cell of a row that has no name, or
/// `#title` for the record's title, or `#none` for nowhere.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Cells {
    /// Absent where no cell names the searched locality, such as a census
    /// listed by canton: the engine's filter is then trusted.
    #[serde(default)]
    pub(super) locality: Option<Cell>,
    #[serde(default)]
    pub(super) parish: Option<Cell>,
    /// Every cell of the name, joined: a register's kinds in several spans.
    #[serde(default)]
    pub(super) act: Option<Cell>,
    #[serde(default)]
    pub(super) period: Option<Cell>,
    /// The numbers a register spans, such as the matricules of a military
    /// register (`1 à 1586`), in one cell, or in two that end with the first
    /// and the last.
    #[serde(default)]
    pub(super) numbers: Option<CellList>,
    /// Default `#title`.
    #[serde(default)]
    pub(super) call_number: Option<Cell>,
}

/// The values of the keyed filters, read from the engine's lists.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Keys {
    pub(super) locality: Option<String>,
    pub(super) period: Option<String>,
}

fn invalid(message: &str) -> CatalogError {
    CatalogError::new(format!("arkotheque settings: {message}"))
}

/// A reference of the request interface: letters, digits and `_`.
pub(super) fn is_reference(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

impl Filter {
    fn is_valid(&self) -> bool {
        is_reference(&self.reference) && self.end.as_deref().is_none_or(is_reference)
    }
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
        if !self.search_path.starts_with('/') || self.search_path.contains(['?', '#', ' ']) {
            return Err(invalid("search_path must be an absolute path"));
        }
        let filters = [&self.fields.locality, &self.fields.act, &self.fields.period];
        if !is_reference(&self.engine)
            || !is_reference(&self.display_mode)
            || !filters.into_iter().flatten().all(Filter::is_valid)
        {
            return Err(invalid("engine, display mode and filter references"));
        }
        let ended = [&self.fields.locality, &self.fields.act];
        if ended
            .into_iter()
            .flatten()
            .any(|filter| filter.end.is_some())
        {
            return Err(invalid("only the period filter has an end"));
        }
        if self.content_ids.is_empty()
            || !self
                .content_ids
                .iter()
                .all(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()))
        {
            return Err(invalid("content_ids must be numeric identifiers"));
        }
        Ok(())
    }

    /// Every act code is valid and has filter values, and every document
    /// kind the collection holds has them. An engine without an act filter
    /// has no values: its rows are told apart by their act cell, which a
    /// collection of several kinds needs.
    fn check_acts(&self, collection: &Collection) -> Result<(), CatalogError> {
        if self.fields.act.is_none() {
            if !self.acts.is_empty() {
                return Err(invalid("acts without fields.act"));
            }
            if collection.acts.len() > 1 && self.cells.act.is_none() {
                return Err(invalid(
                    "several document kinds without an act filter need cells.act",
                ));
            }
            return Ok(());
        }
        for (code, Values(values)) in &self.acts {
            if Act::from_code(code).is_none() {
                return Err(invalid(&format!("`{code}` is not an act code")));
            }
            if values.is_empty() || values.iter().any(|value| value.trim().is_empty()) {
                return Err(invalid(&format!("`{code}` has an empty filter value")));
            }
        }
        match collection
            .acts
            .iter()
            .find(|act| self.act_values(act).is_none())
        {
            Some(act) => Err(invalid(&format!("no filter value for `{act}`"))),
            None => Ok(()),
        }
    }

    /// The act filter values: the act's own, or for a combined act (`BMS`)
    /// its first kind's, whose category holds the mixed registers.
    pub(super) fn act_values(&self, act: &Act) -> Option<&[String]> {
        self.acts
            .get(&act.to_string())
            .or_else(|| {
                let first = act.primary_kind()?;
                self.acts.get(&first.letter().to_string())
            })
            .map(|Values(values)| values.as_slice())
    }

    /// Whether the rows' act cells must be read for the cited act, because
    /// the search cannot single it out: the engine has no act filter, or
    /// the act's filter values are another code's too (`Baptêmes /
    /// Naissances` for both `B` and `N`).
    pub(super) fn reads_acts(&self, act: &Act) -> bool {
        let Some(values) = self.act_values(act) else {
            return true;
        };
        self.acts
            .values()
            .filter(|Values(other)| other.iter().any(|value| values.contains(value)))
            .count()
            > 1
    }

    /// Whether a filter's value must be read from the engine's lists first.
    pub(super) fn needs_keys(&self, citation: &CitationParts) -> bool {
        let keyed = |filter: &Option<Filter>| filter.as_ref().is_some_and(|filter| filter.keyed);
        (keyed(&self.fields.locality) && !citation.locality.is_empty())
            || (keyed(&self.fields.period) && citation.year.is_some())
    }

    /// The locality as the portal writes it.
    pub(super) fn locality(&self, citation: &CitationParts) -> String {
        self.locality_style.write(&citation.locality)
    }

    /// The forms of the cited locality a row's locality, read in the
    /// portal's style, may take; none where no cell shows the locality, so
    /// that every row the engine returned is kept.
    pub(super) fn localities(&self, citation: &CitationParts) -> Vec<String> {
        if self.cells.locality.is_none() {
            return Vec::new();
        }
        vec![
            citation.locality.clone(),
            self.locality_style.cited(&citation.locality),
        ]
    }

    /// The filters of a search for the page of `size` rows (25, 50 or 100)
    /// starting at row `from`, shared by the requests and the search page:
    /// the locality, the act category and, where the engine has the filter,
    /// the year. A keyed filter whose key `keys` lacks is left out. Nothing
    /// else of the citation leaves the application.
    pub(super) fn page_filters(
        &self,
        citation: &CitationParts,
        keys: &Keys,
        size: &str,
        from: usize,
    ) -> Query {
        let engine = &self.engine;
        let mut query = Query::new();
        query
            .push(format!("{engine}--ficheFocus"), "")
            .push(format!("{engine}--filtreGroupes[mode]"), "simple")
            .push(format!("{engine}--filtreGroupes[op]"), "AND");
        let mut filter = |field: &str, values: &[String], mode: Mode| {
            let prefix = format!("{engine}--filtreGroupes[groupes][0][{field}]");
            let op = if values.len() > 1 { "OR" } else { "AND" };
            query.push(format!("{prefix}[op]"), op);
            for value in values {
                query.push(format!("{prefix}[q][]"), value.as_str());
            }
            query.push(format!("{prefix}[extras][mode]"), mode.as_str());
        };
        if let Some(field) = &self.fields.locality
            && let Some(value) = keyed_or(field, &keys.locality, || self.locality(citation))
            // A series cited without a locality is searched everywhere.
            && !value.is_empty()
        {
            filter(
                &field.reference,
                &[value],
                field.mode.unwrap_or(Mode::Popup),
            );
        }
        if let (Some(field), Some(values)) = (&self.fields.act, self.act_values(&citation.act)) {
            filter(&field.reference, values, field.mode.unwrap_or(Mode::Select));
        }
        if let (Some(field), Some(year)) = (&self.fields.period, citation.year) {
            let mode = field.mode.unwrap_or(Mode::Slider);
            let plain = || match mode {
                Mode::Slider => format!("{year}|{year}"),
                _ => year.to_string(),
            };
            if let Some(value) = keyed_or(field, &keys.period, plain) {
                filter(&field.reference, &[value], mode);
            }
            if let Some(end) = &field.end {
                filter(end, &[year.to_string()], mode);
            }
        }
        query
            .push(format!("{engine}--from"), from.to_string())
            .push(format!("{engine}--resultSize"), size);
        for id in &self.content_ids {
            query.push(format!("{engine}--contenuIds[]"), id.as_str());
        }
        query.push(format!("{engine}--modeRestit"), self.display_mode.as_str());
        query
    }

    pub(super) fn search_request(&self, filters: &Query) -> String {
        let mut query = Query::new();
        query.push("refUnique", self.engine.as_str());
        format!("{SEARCH_PATH}?{query}&{filters}")
    }

    /// The engine's bare answer, as the search page requests it on load: its
    /// filters, display modes and the values each filter lists.
    pub(super) fn engine_request(&self) -> String {
        let mut query = Query::new();
        query.push("refUnique", self.engine.as_str());
        for id in &self.content_ids {
            query.push(format!("{}--contenuIds[]", self.engine), id.as_str());
        }
        format!("{SEARCH_PATH}?{query}")
    }

    /// The search page filtered as the search is, on the portal's default
    /// page of 25 rows, which renders faster than the adapter's 100.
    pub(super) fn results_page(&self, citation: &CitationParts, keys: &Keys) -> String {
        let filters = self.page_filters(citation, keys, READER_PAGE_SIZE, 0);
        format!("{}{}?{filters}", self.origin, self.search_path)
    }

    /// The record page opened on image `index`, zero-based.
    pub(super) fn view_url(&self, record: &str, viewer: &str, index: u16) -> String {
        format!(
            "{}{}?detail={}#{viewer}/{index}",
            self.origin,
            self.search_path,
            crate::platform::query::encode(record)
        )
    }
}

/// A filter's value: its key when the filter is keyed, the plain value
/// otherwise; `None` for a keyed filter without its key.
fn keyed_or(
    filter: &Filter,
    key: &Option<String>,
    plain: impl FnOnce() -> String,
) -> Option<String> {
    if filter.keyed {
        key.clone()
    } else {
        Some(plain())
    }
}
