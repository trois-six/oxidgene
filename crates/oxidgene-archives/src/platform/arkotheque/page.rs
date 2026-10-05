//! Reading the request interface's answers: the search's result rows, the
//! engine's lists of filter values, and the viewer's image list.
//!
//! The search answer lists its registers twice: `resultats.results` gives
//! each one's record reference and title, and `resultats.html` renders the
//! same registers, in the same order, as table rows whose cells carry the
//! locality, parish, acts, period and call number — in `data-champ` spans,
//! or as a cell's bare text — and whose viewer button carries the viewer
//! address and the image count. The rows are read with the shared markup
//! scans.

use serde::Deserialize;
use serde_json::{Map, Value};

use super::settings::{CellList, Cells, is_reference};
use crate::ResolveError;
use crate::platform::markup;
use crate::platform::select::{Candidate, number_range};

/// What the adapter keeps of a register to open it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Register {
    /// The record reference, `arko_fiche_…`.
    pub(super) record: String,
    /// `/_recherche-api/visionneuse-infos/…`, when the register has images.
    pub(super) viewer: Option<String>,
    /// Every locality the row names, the first being the candidate's: a
    /// register of several communes or parishes is each one's.
    pub(super) localities: Vec<String>,
}

/// Where a row shows one of its parts: a cell setting.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub(super) enum Cell {
    /// The `data-champ` spans of that name.
    Champ(String),
    /// The text of the row's n-th cell, one-based (`#3`), for a part shown
    /// without a name.
    Column(usize),
    /// The record's title, `resultats.results[].intitule` (`#title`).
    Title,
    /// Nowhere (`#none`): for a call number the rows do not show, where
    /// the title holds something else.
    Nowhere,
}

/// The widest table a portal renders.
const MAX_COLUMN: usize = 30;

impl TryFrom<String> for Cell {
    type Error = String;

    fn try_from(setting: String) -> Result<Self, Self::Error> {
        let column = setting
            .strip_prefix('#')
            .and_then(|number| number.parse().ok())
            .filter(|number| (1..=MAX_COLUMN).contains(number));
        match (setting.as_str(), column) {
            ("#title", _) => Ok(Self::Title),
            ("#none", _) => Ok(Self::Nowhere),
            (_, Some(column)) => Ok(Self::Column(column)),
            (name, None) if is_reference(name) => Ok(Self::Champ(setting)),
            _ => Err(format!(
                "`{setting}` is no cell: a data-champ name, `#<column>`, `#title` or `#none`"
            )),
        }
    }
}

fn unexpected(detail: &str) -> ResolveError {
    ResolveError::UnexpectedResponse(format!("arkotheque: {detail}"))
}

#[derive(Deserialize)]
struct SearchAnswer {
    resultats: Resultats,
}

#[derive(Deserialize)]
struct Resultats {
    results: Vec<ResultEntry>,
    html: String,
    /// Every row the search matched, across its pages.
    #[serde(default)]
    total: Option<usize>,
}

#[derive(Deserialize)]
struct ResultEntry {
    #[serde(rename = "refUnique")]
    record: String,
    #[serde(default)]
    intitule: Option<String>,
}

/// One rendered row and its record's title.
struct Row<'h> {
    html: &'h str,
    title: Option<String>,
    champs: Vec<(String, String)>,
}

impl Row<'_> {
    /// Every text the cell shows, in order.
    fn texts(&self, cell: &Cell) -> Vec<String> {
        let texts: Vec<String> = match cell {
            Cell::Champ(name) => self
                .champs
                .iter()
                .filter(|(champ, _)| champ == name)
                .map(|(_, text)| text.clone())
                .collect(),
            Cell::Column(column) => markup::split_after(self.html, "<td")
                .get(column - 1)
                .map(|cell| {
                    let content = cell.split_once('>').map_or("", |(_, content)| content);
                    markup::strip_tags(content.split("</td>").next().unwrap_or_default())
                })
                .into_iter()
                .collect(),
            Cell::Title => self.title.iter().cloned().collect(),
            Cell::Nowhere => Vec::new(),
        };
        texts
            .into_iter()
            .map(|text| text.trim().to_owned())
            .filter(|text| !text.is_empty())
            .collect()
    }

    /// The first text of a cell.
    fn first(&self, cell: Option<&Cell>) -> Option<String> {
        self.texts(cell?).into_iter().next()
    }
}

/// The registers of a search answer and the count of all it matched, their
/// parts read from the cells
/// `cells` names, as the portal wrote them; the act joins every text of its
/// cell.
pub(super) fn search_rows(
    answer: &str,
    cells: &Cells,
) -> Result<(Vec<Candidate<Register>>, usize), ResolveError> {
    let answer: SearchAnswer = serde_json::from_str(answer).map_err(|_| {
        markup::unreadable(
            answer,
            "arkotheque: the search answer lacks resultats.results or html".to_owned(),
        )
    })?;
    let rendered = markup::split_after(&answer.resultats.html, "<tr class=\"resultat_container");
    if rendered.len() != answer.resultats.results.len() {
        return Err(unexpected("the rendered rows do not match the results"));
    }
    let total = answer
        .resultats
        .total
        .unwrap_or(answer.resultats.results.len());
    let rows = answer
        .resultats
        .results
        .into_iter()
        .zip(rendered)
        .map(|(entry, html)| {
            let html = html.split("</tr>").next().unwrap_or(html);
            let row = Row {
                html,
                title: entry.intitule,
                champs: markup::labelled_texts(html, "data-champ"),
            };
            let period = row.first(cells.period.as_ref());
            let localities = cells
                .locality
                .as_ref()
                .map(|cell| row.texts(cell))
                .unwrap_or_default();
            let locality = localities.first().cloned();
            let act = cells
                .act
                .as_ref()
                .map(|cell| row.texts(cell).join(", "))
                .filter(|text| !text.is_empty());
            Candidate {
                call_number: match &cells.call_number {
                    Some(cell) => row.first(Some(cell)),
                    None => row.first(Some(&Cell::Title)).and_then(|title| {
                        call_number_in_title(&title, locality.as_deref(), period.as_deref())
                    }),
                },
                locality,
                act,
                parish: row.first(cells.parish.as_ref()),
                period,
                images: markup::text_after(html, "class=\"nombre_images\">")
                    .and_then(|text| markup::first_number(&text)),
                numbers: cells
                    .numbers
                    .as_ref()
                    .and_then(|cells| numbers(&row, cells)),
                payload: Register {
                    record: entry.record,
                    localities,
                    viewer: markup::attribute(html, "data-visionneuse-url").filter(|address| {
                        address.starts_with("/_recherche-api/visionneuse-infos/")
                    }),
                },
            }
        })
        .collect();
    Ok((rows, total))
}

/// The call number in a record's title, which some portals follow with the
/// row's locality or the start of it (`9 E 99 Exampleville`), or with its
/// period (`9 E 99 1798 - 1800`); `None` for a title that is the locality
/// alone.
fn call_number_in_title(
    title: &str,
    locality: Option<&str>,
    period: Option<&str>,
) -> Option<String> {
    let title = title.trim();
    if locality == Some(title) {
        return None;
    }
    let tail_at = title.match_indices(' ').map(|(at, _)| at).find(|at| {
        let rest = title[at + 1..].trim();
        let names_locality = rest.chars().any(char::is_alphabetic)
            && locality.is_some_and(|name| name.starts_with(rest));
        names_locality
            || period.is_some_and(|period| same_text(rest, period))
            || is_year_range(rest)
    });
    let call_number = &title[..tail_at.unwrap_or(title.len())];
    Some(call_number.trim_end().to_owned()).filter(|text| !text.is_empty())
}

/// Whether two texts are the same but for their spaces.
fn same_text(one: &str, other: &str) -> bool {
    one.split_whitespace().eq(other.split_whitespace())
        || one.replace(' ', "") == other.replace(' ', "")
}

/// `1798 - 1800`: two years and a dash, nothing else.
fn is_year_range(text: &str) -> bool {
    let year = |part: &str| {
        let part = part.trim();
        part.len() == 4 && part.bytes().all(|byte| byte.is_ascii_digit())
    };
    text.split_once('-')
        .is_some_and(|(first, last)| year(first) && year(last))
}

/// The numbers a register spans: in one cell (`1 à 500`), or the last
/// number of each of two (`… (acte n° 4024)`, `… (acte n° 4081)`).
fn numbers(row: &Row<'_>, CellList(cells): &CellList) -> Option<(u32, u32)> {
    let texts: Vec<String> = cells
        .iter()
        .filter_map(|cell| row.first(Some(cell)))
        .collect();
    match texts.as_slice() {
        [one] => number_range(one, false),
        [first, last] => {
            let last_number = |text: &str| {
                text.rsplit(|c: char| !c.is_ascii_digit())
                    .find(|digits| !digits.is_empty())
                    .and_then(|digits| digits.parse::<u32>().ok())
            };
            last_number(first)
                .zip(last_number(last))
                .filter(|(first, last)| first <= last)
        }
        _ => None,
    }
}

/// The folded word stems of the act kinds, with their letters.
const ACT_STEMS: [(&str, char); 6] = [
    ("bapt", 'B'),
    ("naiss", 'N'),
    ("mariag", 'M'),
    ("sepult", 'S'),
    ("deces", 'D'),
    ("publication", 'P'),
];

/// The code of the acts a cell names in French words — `BMS` for
/// `Baptêmes, Mariages, Sépultures`, `N` for `1850 (naissances)`, `TD` for
/// `Tables décennales des naissances, mariages, décès` — or `None` when it
/// names none, or a table other than a decennial one.
pub(super) fn act_code(text: &str) -> Option<String> {
    let folded = markup::fold(text);
    let words: Vec<&str> = folded.split(' ').collect();
    let has = |stem: &str| words.iter().any(|word| word.starts_with(stem));
    if has("table") {
        return has("decenn").then(|| "TD".to_owned());
    }
    let code: String = ACT_STEMS
        .iter()
        .filter(|(stem, _)| has(stem))
        .map(|(_, letter)| *letter)
        .collect();
    (!code.is_empty()).then_some(code)
}

/// The engine's bare answer: its filters, display modes and, per indexed
/// field, the values a filter lists.
#[derive(Deserialize)]
pub(super) struct EngineAnswer {
    pub(super) filtres: Vec<EngineFilter>,
    /// The display modes, which only the live checks compare.
    #[cfg(any(test, feature = "live"))]
    pub(super) restits: Vec<Restit>,
    resultats: Aggregated,
}

#[derive(Deserialize)]
pub(super) struct EngineFilter {
    #[serde(rename = "refUnique")]
    pub(super) reference: String,
    #[serde(default)]
    properties: Vec<Property>,
}

#[derive(Deserialize)]
struct Property {
    #[serde(rename = "fieldName")]
    field: String,
}

#[cfg(any(test, feature = "live"))]
#[derive(Deserialize)]
pub(super) struct Restit {
    #[serde(rename = "refUnique")]
    pub(super) reference: String,
}

#[derive(Deserialize)]
struct Aggregated {
    #[serde(default)]
    aggregations: Vec<Map<String, Value>>,
}

/// Reads the engine's bare answer.
pub(super) fn engine_answer(answer: &str) -> Result<EngineAnswer, ResolveError> {
    serde_json::from_str(answer).map_err(|_| {
        markup::unreadable(
            answer,
            "arkotheque: the engine answer lacks filtres, restits or resultats".to_owned(),
        )
    })
}

impl EngineAnswer {
    /// The indexed field of a filter, when the engine has the filter.
    pub(super) fn field(&self, reference: &str) -> Option<&str> {
        self.filtres
            .iter()
            .find(|filter| filter.reference == reference)
            .map(|filter| {
                filter
                    .properties
                    .first()
                    .map_or("", |property| property.field.as_str())
            })
    }

    /// The values a field's filter lists, the most frequent first, with
    /// their record keys where the field has them. The engines nest the
    /// buckets under `<field>_terms` or directly under the field.
    pub(super) fn values(&self, field: &str) -> Vec<&str> {
        self.counted_values(field)
            .into_iter()
            .map(|(value, _)| value)
            .collect()
    }

    /// The values a field's filter lists with the number of records each
    /// names, the most frequent first.
    pub(super) fn counted_values(&self, field: &str) -> Vec<(&str, u64)> {
        self.resultats
            .aggregations
            .iter()
            .find_map(|aggregation| aggregation.get(field))
            .map(|aggregation| {
                aggregation
                    .get(format!("{field}_terms"))
                    .unwrap_or(aggregation)
            })
            .and_then(|terms| terms["buckets"].as_array())
            .map(|buckets| {
                buckets
                    .iter()
                    .filter_map(|bucket| {
                        let count = bucket["doc_count"].as_u64().unwrap_or_default();
                        Some((bucket["key"].as_str()?, count))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The values of a filter, by its reference.
    pub(super) fn filter_values(&self, reference: &str) -> Vec<&str> {
        self.field(reference)
            .map(|field| self.values(field))
            .unwrap_or_default()
    }
}

/// A filter value without its record key: `Exampleville` for
/// `Exampleville[[arko_fiche_…]]`.
pub(super) fn without_key(value: &str) -> &str {
    value
        .split_once("[[")
        .map_or(value, |(name, _)| name)
        .trim()
}

/// One image of a register, as the viewer endpoint lists it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(super) struct Source {
    /// `/_recherche-images/show/…/<index>`: the image's IIIF base on the
    /// portal's origin.
    pub(super) src: String,
    /// `/ark:<naan>/<name>…`, the image's persistent address.
    #[serde(rename = "ARKLink", default)]
    pub(super) ark: Option<String>,
}

#[derive(Deserialize)]
struct ViewerAnswer {
    medias: Vec<Media>,
}

#[derive(Deserialize)]
struct Media {
    sources: Vec<Source>,
}

/// The path of an absolute address on the portal's `origin`, its host
/// written with or without `www.`.
fn path_on<'a>(address: &'a str, origin: &str) -> Option<&'a str> {
    let host = |url: &'a str| {
        url.strip_prefix("https://")
            .map(|rest| rest.trim_start_matches("www."))
    };
    let rest = host(address)?;
    let origin_host = origin.strip_prefix("https://")?.trim_start_matches("www.");
    rest.strip_prefix(origin_host)
        .filter(|path| path.starts_with('/'))
}

/// The images of a register, in order, their paths on the portal's
/// `origin`, which some portals write in full. Nothing else of the answer is
/// read: its other fields name internal hosts and files.
pub(super) fn viewer_sources(answer: &str, origin: &str) -> Result<Vec<Source>, ResolveError> {
    let answer: ViewerAnswer = serde_json::from_str(answer).map_err(|_| {
        markup::unreadable(
            answer,
            "arkotheque: the viewer answer lacks medias[].sources".to_owned(),
        )
    })?;
    let mut sources = answer
        .medias
        .into_iter()
        .next()
        .map(|media| media.sources)
        .unwrap_or_default();
    for source in &mut sources {
        if let Some(path) = path_on(&source.src, origin) {
            source.src = path.to_owned();
        }
    }
    if sources.is_empty()
        || !sources.iter().all(|source| {
            source.src.starts_with("/_recherche-images/") && !source.src.contains(['?', '#'])
        })
    {
        return Err(unexpected("the viewer answer lists no image path"));
    }
    for source in &mut sources {
        source.ark = source
            .ark
            .take()
            .filter(|ark| ark.starts_with("/ark:") && !ark.contains(['?', '#', ' ']));
    }
    Ok(sources)
}
