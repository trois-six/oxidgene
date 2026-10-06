//! A finding aid's registers as a citation reads them.
//!
//! A register is a leaf of the aid's tree; what it holds is written in its
//! own title and in its ancestors' (`Registres paroissiaux > Catholiques >
//! Paroisse Saint-Exemple • 1631-1721 - GG 1`, `Tables décennales >
//! 1793-an X > Naissances`). Its acts are read from the words naming them,
//! never from letters; its period from the first of its dates and titles
//! that holds a year; its parish from a title naming one; its call number
//! from the one it shows.

use crate::citation::{Act, ActKind, CitationGrammar, CitationParts};
use crate::platform::locality::label_name;
use crate::platform::markup::fold;
use crate::platform::select::{Candidate, act_code, covers, number_range, period_ranges};

use super::page::{Aid, Node};

/// A register the adapter can open: its node's identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Register {
    pub(super) id: String,
}

/// A register with its ancestors, nearest first.
pub(super) struct Leaf<'t> {
    pub(super) node: &'t Node,
    pub(super) ancestors: Vec<&'t Node>,
}

/// Every register of a finding aid, or of the subtrees whose root at
/// `level` names the cited locality, the aid itself as their last ancestor.
pub(super) fn leaves<'t>(aid: &'t Aid, level: Option<usize>, locality: &str) -> Vec<Leaf<'t>> {
    let mut path: Vec<&Node> = Vec::new();
    let mut found = Vec::new();
    for node in &aid.nodes {
        path.truncate(node.depth.saturating_sub(1));
        if node.leaf {
            let within = level.is_none_or(|level| {
                path.get(level - 1)
                    .is_some_and(|root| names_locality(&root.title, locality))
            });
            if within {
                found.push(Leaf {
                    node,
                    ancestors: path
                        .iter()
                        .rev()
                        .copied()
                        .chain(std::iter::once(&aid.root))
                        .collect(),
                });
            }
        } else {
            path.push(node);
        }
    }
    found
}

/// The leading articles a place's name may carry, folded.
const ARTICLES: [&str; 4] = ["le ", "la ", "les ", "l "];

/// A folded place name without its leading article.
fn bare(folded: &str) -> &str {
    ARTICLES
        .iter()
        .find_map(|article| folded.strip_prefix(article))
        .unwrap_or(folded)
}

/// Whether the portal's name of a place is the cited locality: as written
/// (`Le Mas-d'Exemple`), with its article behind it (`Mas-d'Exemple (Le)`)
/// or without it (`Mas-d'Exemple`), whatever the case and accents.
pub(super) fn same_place(portal: &str, cited: &str) -> bool {
    let portal = fold(&label_name(portal));
    let cited = fold(cited);
    !cited.is_empty() && (portal == cited || bare(&portal) == bare(&cited))
}

/// Whether a node of a finding aid names the cited locality: the place
/// itself, or an office of it (`Bureau de recrutement d'Exampleville`,
/// `Consistoire de Saint-Exemple`), a qualifier after it aside.
fn names_locality(title: &str, cited: &str) -> bool {
    let title = label_name(title);
    if same_place(&title, cited) {
        return true;
    }
    // Each place named after `de`, `d'`, `du` or `des`.
    let lowered = title.to_lowercase();
    [" de ", " d'", " d\u{2019}", " du ", " des "]
        .iter()
        .flat_map(|separator| {
            lowered
                .match_indices(separator)
                .map(move |(at, _)| at + separator.len())
        })
        .any(|at| {
            title
                .get(at..)
                .is_some_and(|place| same_place(place, cited))
        })
}

/// How a table reads in a title.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Table {
    /// `Tables décennales`: `TD`.
    Decennial,
    /// `Répertoires annuels`, `Tables annuelles`: `TA`.
    Annual,
    /// `Tables`, `Répertoire alphabétique`: a table of no stated period.
    Other,
}

impl Table {
    fn code(self) -> Option<&'static str> {
        match self {
            Self::Decennial => Some("TD"),
            Self::Annual => Some("TA"),
            Self::Other => None,
        }
    }
}

/// What one title says a register holds.
#[derive(Debug, Default)]
struct Reading {
    /// The act kinds its items name in words, outside a table.
    kinds: Vec<ActKind>,
    /// The table one of its items names.
    table: Option<Table>,
}

/// Reads a title item by item (`Naissances, mariages, décès ; tables
/// décennales`, `Baptêmes et naissances`).
fn read(title: &str) -> Reading {
    let mut reading = Reading::default();
    for item in title.split([',', ';']) {
        let folded = fold(item);
        for part in folded.split(" et ").flat_map(|part| part.split(" puis ")) {
            let words: Vec<&str> = part.split(' ').filter(|word| !word.is_empty()).collect();
            if let Some(table) = table_of(&words) {
                reading.table = reading.table.or(Some(table));
                continue;
            }
            for kind in kinds_of(part, &words) {
                if !reading.kinds.contains(&kind) {
                    reading.kinds.push(kind);
                }
            }
        }
    }
    reading
}

fn table_of(words: &[&str]) -> Option<Table> {
    let named = words.iter().any(|word| {
        word.starts_with("table") || word.starts_with("repertoire") || *word == "index"
    });
    if !named {
        return None;
    }
    Some(if words.iter().any(|word| word.starts_with("decen")) {
        Table::Decennial
    } else if words.iter().any(|word| word.starts_with("annuel")) {
        Table::Annual
    } else {
        Table::Other
    })
}

/// The act kinds one item names in words (`Baptêmes`, `Naissances,
/// mariages, décès`), as `select` reads them, never letters; publications
/// of banns alone when it names them (`publications de mariage`), which a
/// register keeps apart from the marriages they announce.
fn kinds_of(part: &str, words: &[&str]) -> Vec<ActKind> {
    if words
        .iter()
        .any(|word| word.starts_with("publication") || *word == "bans")
    {
        return vec![ActKind::Publication];
    }
    match act_code(part, false).and_then(|code| Act::from_code(&code)) {
        Some(Act::Register(kinds)) => kinds,
        _ => Vec::new(),
    }
}

/// The kinds a register's nearest title implying any implies, without
/// naming them: parish registers hold baptisms, marriages and burials,
/// civil status births, marriages and deaths, a title naming both all six.
fn implied_kinds<'n>(titles: impl Iterator<Item = &'n str>) -> Vec<ActKind> {
    titles
        .map(|title| {
            let folded = fold(title);
            let parish = folded.split(' ').any(|word| {
                word.starts_with("paroissia")
                    || word.starts_with("catholicite")
                    || word.starts_with("pastora")
            });
            let mut kinds = Vec::new();
            if parish {
                kinds.extend([ActKind::Baptism, ActKind::Marriage, ActKind::Burial]);
            }
            if folded.contains("etat civil") {
                kinds.extend([ActKind::Birth, ActKind::Marriage, ActKind::Death]);
            }
            kinds.dedup();
            kinds
        })
        .find(|kinds| !kinds.is_empty())
        .unwrap_or_default()
}

/// How well a register answers the cited document kind, the best first:
/// `None` when it cannot hold it.
fn fit(leaf: &Leaf, act: &Act) -> Option<u8> {
    let own = read(&leaf.node.title);
    let above: Vec<Reading> = leaf
        .ancestors
        .iter()
        .map(|ancestor| read(&ancestor.title))
        .collect();
    let table = above.iter().find_map(|reading| reading.table).or(own.table);
    // A table's kinds are those it indexes, not acts it holds.
    let table_only = above.iter().any(|reading| reading.table.is_some())
        || (own.table.is_some() && own.kinds.is_empty());
    match act {
        Act::Register(cited) => {
            if table_only {
                return None;
            }
            let named = std::iter::once(&own)
                .chain(above.iter())
                .find(|reading| !reading.kinds.is_empty());
            let holds = |kinds: &[ActKind]| {
                let held = Act::Register(kinds.to_vec());
                cited.iter().all(|kind| held.includes(*kind))
            };
            if let Some(named) = named {
                // A register naming the cited kinds themselves before one
                // filing them with others: publications of banns apart from
                // the marriages.
                let exact = cited.iter().all(|kind| named.kinds.contains(kind));
                return holds(&named.kinds).then_some(if exact { 0 } else { 1 });
            }
            let implied = implied_kinds(
                leaf.ancestors
                    .iter()
                    .map(|ancestor| ancestor.title.as_str()),
            );
            if implied.is_empty() {
                Some(3)
            } else {
                holds(&implied).then_some(2)
            }
        }
        Act::Table(code) => match table?.code() {
            Some(written) => (written == code).then_some(0),
            None => Some(1),
        },
        Act::Series(series) => {
            if own.table.is_some() {
                return Some(2);
            }
            let grammar = CitationGrammar::default();
            let named = std::iter::once(leaf.node)
                .chain(leaf.ancestors.iter().copied())
                .any(|node| grammar.series_of(&node.title) == Some(*series));
            Some(if named { 0 } else { 1 })
        }
    }
}

/// The period of a register: the first of its dates and titles, its own
/// then its ancestors', that holds a year. Its date comes before its title,
/// which may hold other numbers (`N° 1001-1500`).
fn period(leaf: &Leaf) -> Option<String> {
    std::iter::once(leaf.node)
        .chain(leaf.ancestors.iter().copied())
        .flat_map(|node| [node.date.as_deref(), Some(node.title.as_str())])
        .flatten()
        .find(|text| !period_ranges(text).is_empty())
        .map(str::to_owned)
}

/// The parish a register's titles name: what follows `paroisse` in the
/// nearest title naming one, without a confession or a leading `de`.
fn parish(leaf: &Leaf) -> Option<String> {
    std::iter::once(leaf.node)
        .chain(leaf.ancestors.iter().copied())
        .find_map(|node| {
            let lowered = node.title.to_lowercase();
            let at = lowered
                .find("paroisses ")
                .map(|at| at + "paroisses ".len())
                .or_else(|| lowered.find("paroisse ").map(|at| at + "paroisse ".len()))?;
            let mut name = node.title.get(at..)?.trim();
            for word in ["catholique ", "protestante ", "réformée "] {
                name = strip_prefix_ignoring_case(name, word).unwrap_or(name);
            }
            for article in ["de la ", "de l'", "du ", "des ", "de ", "d'", "d\u{2019}"] {
                if let Some(rest) = strip_prefix_ignoring_case(name, article) {
                    name = rest;
                    break;
                }
            }
            let name = name.trim();
            (!name.is_empty()).then(|| name.to_owned())
        })
}

fn strip_prefix_ignoring_case<'t>(text: &'t str, prefix: &str) -> Option<&'t str> {
    let head = text.get(..prefix.len())?;
    (head.to_lowercase() == prefix).then(|| &text[prefix.len()..])
}

/// The registers that may hold the cited document in the cited year, as
/// candidates of the cited locality, as a portal's search filtered by act
/// and year would list them: those of the cited year that hold the cited
/// document, of the best fit only (the registers naming the cited acts
/// before those whose collection implies them). A register showing no year
/// is kept.
pub(super) fn candidates(leaves: &[Leaf], citation: &CitationParts) -> Vec<Candidate<Register>> {
    let read: Vec<(Option<u8>, Option<String>)> = leaves
        .iter()
        .map(|leaf| (fit(leaf, &citation.act), period(leaf)))
        .collect();
    let dated = |period: &Option<String>| {
        citation
            .year
            .is_none_or(|year| period.as_deref().is_none_or(|period| covers(period, year)))
    };
    let Some(best) = read
        .iter()
        .filter(|(_, period)| dated(period))
        .filter_map(|(fit, _)| *fit)
        .min()
    else {
        return Vec::new();
    };
    leaves
        .iter()
        .zip(read)
        .filter(|(_, (fit, period))| *fit == Some(best) && dated(period))
        .map(|(leaf, (_, period))| Candidate {
            locality: Some(citation.locality.clone()),
            call_number: leaf.node.call_number.clone(),
            act: None,
            parish: parish(leaf),
            period,
            images: None,
            numbers: number_range(&leaf.node.title, true),
            payload: Register {
                id: leaf.node.id.clone(),
            },
        })
        .collect()
}
