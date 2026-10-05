//! The places a Ligeo locality cell names.
//!
//! The portals fill their locality cells from a thesaurus, each in its own
//! way: a bare name (`Exampleville`), a name qualified by the places it lies
//! within (`Exampleville (Exampledept, France)`, `Hameau (Exampleville,
//! Exampledept ; lieu-dit)`), a commune followed by a parish, a hamlet or a
//! street (`Exampleville / Saint-Exemple (paroisse)`, `Exampleville —
//! Ancienne`, `Exampleville -- Rue …`, `EXAMPLEVILLE Hameau`,
//! `Exampleville, paroisse Saint-Exemple`), several places in one cell
//! (`Exampleville (…), Autreville (…)`, or one after another with nothing
//! between them), a note in brackets (`[aujourd'hui : …]`), or the path of
//! the register in the finding aid (`Registres > Exampleville`). [`places`]
//! reads every place, so selection can match the cited locality on the
//! commune whatever the cell adds.

use crate::platform::markup::fold;

/// One place a locality cell names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Place {
    /// The name, its article written before it (`Le Bourg` for `Bourg
    /// (Le)`).
    pub(super) name: String,
    /// The places its qualifier names it within, the commune of a hamlet or
    /// of a former commune first (`Exampleville` for `Hameau (Exampleville,
    /// Exampledept)`).
    pub(super) within: Vec<String>,
    /// The parish, hamlet or street the cell names within the place.
    pub(super) parish: Option<String>,
}

/// The words naming an office or a district after its seat: `Bureau de
/// Exampleville`, a recruitment or registration bureau, `subdivision de
/// Exampleville` and `Canton de Exampleville` stand for their seat.
const OFFICES: [&str; 8] = [
    "bureau de ",
    "bureau d'",
    "bureau d\u{2019}",
    "bureau du ",
    "subdivision de ",
    "subdivision d'",
    "canton de ",
    "canton d'",
];

/// The seat an office's or a district's name names.
fn seat(name: &str) -> Option<String> {
    OFFICES.iter().find_map(|office| {
        let head = name.get(..office.len())?;
        let seat = name[office.len()..].trim();
        (head.eq_ignore_ascii_case(office) && !seat.is_empty()).then(|| seat.to_owned())
    })
}

impl Place {
    /// A place by its name alone; an office's seat is the place it lies
    /// within.
    pub(super) fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            within: seat(name).into_iter().collect(),
            parish: None,
        }
    }

    /// How this place stands for the cited locality, folded: the place
    /// itself, with its own parish, or a place within it, which is then the
    /// parish. `None` when it is neither.
    pub(super) fn as_locality(&self, wanted: &str) -> Option<Option<String>> {
        if fold(&self.name) == wanted {
            return Some(self.parish.clone());
        }
        self.within
            .iter()
            .any(|place| fold(place) == wanted)
            .then(|| Some(self.parish.clone().unwrap_or_else(|| self.name.clone())))
    }
}

/// The articles a thesaurus writes behind a name: `Bourg (Le)`.
const ARTICLES: [&str; 5] = ["Le", "La", "Les", "L'", "L\u{2019}"];

/// The separators between a place and a place within it.
const WITHIN: [&str; 5] = [" / ", " \u{2014} ", " \u{2013} ", " -- ", " - "];

/// Every place a locality cell names, in order.
pub(super) fn places(cell: &str) -> Vec<Place> {
    // The path of a register in its finding aid ends with its place.
    let cell = cell.rsplit(" > ").next().unwrap_or(cell);
    let mut places: Vec<Place> = Vec::new();
    for item in items(&without_notes(cell)) {
        // `Exampleville, paroisse Saint-Exemple`: the parish of the place
        // before it.
        if let Some(parish) = parish_of(&item)
            && let Some(last) = places.last_mut()
        {
            last.parish = Some(parish);
            continue;
        }
        places.extend(place(&item));
    }
    places
}

/// The parish a `paroisse <name>` or `paroisse de <name>` text names, in any
/// case.
fn parish_of(text: &str) -> Option<String> {
    let head = text.get(..9)?;
    if !head.eq_ignore_ascii_case("paroisse ") {
        return None;
    }
    let parish = text[9..].trim();
    let parish = parish.strip_prefix("de ").unwrap_or(parish).trim();
    (!parish.is_empty()).then(|| parish.to_owned())
}

/// The text without its bracketed notes.
fn without_notes(cell: &str) -> String {
    let mut text = String::with_capacity(cell.len());
    let mut depth = 0_usize;
    for c in cell.chars() {
        match c {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => text.push(c),
            _ => {}
        }
    }
    text
}

/// The items of a cell: separated by a comma or a semicolon outside a
/// qualifier, or following a qualifier with nothing between them.
fn items(cell: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut depth = 0_usize;
    let mut closed = false;
    for c in cell.chars() {
        let ends = depth == 0 && (matches!(c, ',' | ';') || closed && c.is_alphanumeric());
        if ends {
            items.push(std::mem::take(&mut current));
        }
        closed = false;
        match c {
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                closed = depth == 0;
            }
            _ => {}
        }
        if !(ends && matches!(c, ',' | ';')) {
            current.push(c);
        }
        // A space after a qualifier keeps it closed: the next word starts
        // another place unless a separator comes first.
        if c == ' ' && current.trim_end().ends_with(')') && depth == 0 {
            closed = true;
        }
    }
    items.push(current);
    items
        .into_iter()
        .map(|item| item.trim().to_owned())
        .filter(|item| !item.is_empty())
        .collect()
}

/// The places an item names: the first, whose parish is the place named
/// after it, then each place named after it, within it (`Exampleville /
/// Autreville / Troisville`, the communes a registration office serves).
fn place(item: &str) -> Vec<Place> {
    let mut segments = split_within(item).into_iter();
    let Some(mut first) = segments.next().and_then(qualified) else {
        return Vec::new();
    };
    let mut after: Vec<Place> = segments.filter_map(qualified).collect();
    // `EXAMPLEVILLE Hameau`: a commune in capitals, a place within it after.
    if after.is_empty()
        && let Some((commune, rest)) = capitals_first(&first.name)
    {
        first.name = commune;
        after.push(Place::new(&rest));
    }
    if let Some(next) = after.first() {
        first.parish.get_or_insert_with(|| next.name.clone());
    }
    for place in &mut after {
        place.within.insert(0, first.name.clone());
    }
    let mut places = vec![first];
    places.append(&mut after);
    places
}

/// The segments of an item separated by [`WITHIN`] outside a qualifier.
fn split_within(item: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut start = 0;
    let mut depth = 0_usize;
    let mut at = 0;
    while at < item.len() {
        let rest = &item[at..];
        let c = rest.chars().next().unwrap_or(' ');
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ => {}
        }
        if depth == 0
            && let Some(separator) = WITHIN
                .iter()
                .find(|separator| rest.starts_with(**separator))
        {
            segments.push(&item[start..at]);
            at += separator.len();
            start = at;
            continue;
        }
        at += c.len_utf8();
    }
    segments.push(&item[start..]);
    segments
        .into_iter()
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .collect()
}

/// A name and its qualifier: `Name (part, part ; part)`. An article in the
/// qualifier goes before the name, a `paroisse …` part is its parish, and
/// any other part a place it lies within.
fn qualified(segment: &str) -> Option<Place> {
    // The qualifiers closing the segment, the last first: `Exampledept
    // (France). Bureau de l'enregistrement (Exampleville, Exampledept)` is
    // an office qualified by its seat.
    let mut name = segment.trim().trim_end_matches(['.', ',', ':']);
    let mut qualifiers = Vec::new();
    while let Some(inner) = name.strip_suffix(')')
        && let Some((head, qualifier)) = inner.rsplit_once(" (")
    {
        qualifiers.push(qualifier);
        name = head.trim_end();
    }
    if name.is_empty() {
        return None;
    }
    let mut place = Place::new(name);
    let parts = qualifiers
        .iter()
        .rev()
        .flat_map(|qualifier| qualifier.split([',', ';', ':']))
        .map(str::trim);
    for part in parts {
        if let Some(article) = ARTICLES.iter().find(|article| **article == part) {
            let space = if article.ends_with(['\'', '\u{2019}']) {
                ""
            } else {
                " "
            };
            place.name = format!("{article}{space}{name}");
        } else if let Some(parish) = parish_of(part) {
            place.parish = Some(parish);
        } else if !part.is_empty() {
            place.within.push(part.to_owned());
            place.within.extend(seat(part));
        }
    }
    Some(place)
}

/// `EXAMPLEVILLE Hameau` as the commune and the place after it: one or more
/// words in capitals, then a word that is not.
fn capitals_first(name: &str) -> Option<(String, String)> {
    let words: Vec<&str> = name.split(' ').collect();
    let capitals = words
        .iter()
        .take_while(|word| {
            word.chars().any(char::is_alphabetic) && !word.chars().any(char::is_lowercase)
        })
        .count();
    (capitals > 0 && capitals < words.len())
        .then(|| (words[..capitals].join(" "), words[capitals..].join(" ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(cell: &str) -> Vec<(String, Vec<String>, Option<String>)> {
        places(cell)
            .into_iter()
            .map(|place| (place.name, place.within, place.parish))
            .collect()
    }

    fn one(
        name: &str,
        within: &[&str],
        parish: Option<&str>,
    ) -> (String, Vec<String>, Option<String>) {
        (
            name.to_owned(),
            within.iter().map(|place| (*place).to_owned()).collect(),
            parish.map(str::to_owned),
        )
    }

    #[test]
    fn reads_a_name_and_its_qualifier() {
        assert_eq!(read("Exampleville"), [one("Exampleville", &[], None)]);
        assert_eq!(
            read("Exampleville (Exampledept, France)"),
            [one("Exampleville", &["Exampledept", "France"], None)]
        );
        assert_eq!(
            read("HAMEAU (EXAMPLEVILLE, Exampledept, lieu-dit)"),
            [one(
                "HAMEAU",
                &["EXAMPLEVILLE", "Exampledept", "lieu-dit"],
                None
            )]
        );
        assert_eq!(
            read("Exampleville (commune ; Exampledept, France)"),
            [one(
                "Exampleville",
                &["commune", "Exampledept", "France"],
                None
            )]
        );
        assert_eq!(read("Bourg (Le)"), [one("Le Bourg", &[], None)]);
        assert_eq!(read("Isle (L')"), [one("L'Isle", &[], None)]);
        assert_eq!(
            read("Exampleville (paroisse Saint-Exemple)"),
            [one("Exampleville", &[], Some("Saint-Exemple"))]
        );
        assert_eq!(
            read("Exampleville (1784-1968)"),
            [one("Exampleville", &["1784-1968"], None)]
        );
    }

    #[test]
    fn reads_a_place_within_a_commune() {
        assert_eq!(
            read("Exampleville / Saint-Exemple (paroisse) / Autreville......"),
            [
                one("Exampleville", &[], Some("Saint-Exemple")),
                one("Saint-Exemple", &["Exampleville", "paroisse"], None),
                one("Autreville", &["Exampleville"], None)
            ]
        );
        assert_eq!(
            read("Exampleville \u{2014} Ancienne"),
            [
                one("Exampleville", &[], Some("Ancienne")),
                one("Ancienne", &["Exampleville"], None)
            ]
        );
        assert_eq!(
            read("Exampleville -- Rue de l'Exemple")[0],
            one("Exampleville", &[], Some("Rue de l'Exemple"))
        );
        assert_eq!(
            read("EXAMPLEVILLE Hameau"),
            [
                one("EXAMPLEVILLE", &[], Some("Hameau")),
                one("Hameau", &["EXAMPLEVILLE"], None)
            ]
        );
        assert_eq!(
            read("SAINT-EXEMPLE-SUR-MER"),
            [one("SAINT-EXEMPLE-SUR-MER", &[], None)]
        );
        assert_eq!(
            read("Exampleville, paroisse Notre-Dame-de-l'Exemple"),
            [one("Exampleville", &[], Some("Notre-Dame-de-l'Exemple"))]
        );
        assert_eq!(
            read("Exampleville (Exampledept, France), Paroisse Saint Exemple"),
            [one(
                "Exampleville",
                &["Exampledept", "France"],
                Some("Saint Exemple")
            )]
        );
    }

    #[test]
    fn reads_several_places_and_drops_notes_and_paths() {
        assert_eq!(
            read("Exampleville (Exampledept, France), Autreville (Exampledept, France)"),
            [
                one("Exampleville", &["Exampledept", "France"], None),
                one("Autreville", &["Exampledept", "France"], None)
            ]
        );
        assert_eq!(
            read(
                "Ancienne (Exampledept, France) [aujourd'hui : Exampleville (Exampledept, France)] \
                 Exampleville (Exampledept, France) Saint-Exemple (Exampleville, Exampledept, France ; paroisse)"
            ),
            [
                one("Ancienne", &["Exampledept", "France"], None),
                one("Exampleville", &["Exampledept", "France"], None),
                one(
                    "Saint-Exemple",
                    &["Exampleville", "Exampledept", "France", "paroisse"],
                    None
                )
            ]
        );
        assert_eq!(
            read("Exampleville (Exampledept, France) ; Autreville (Exampledept, France)").len(),
            2
        );
        assert_eq!(
            read("Registres paroissiaux et état civil > EXAMPLEVILLE"),
            [one("EXAMPLEVILLE", &[], None)]
        );
        assert_eq!(read(""), []);
    }

    #[test]
    fn stands_for_the_cited_locality_itself_or_as_its_commune() {
        let [hamlet] = &places("Hameau (Exampleville, Exampledept, lieu-dit)")[..] else {
            panic!("one place");
        };
        assert_eq!(hamlet.as_locality("hameau"), Some(None));
        assert_eq!(
            hamlet.as_locality("exampleville"),
            Some(Some("Hameau".to_owned()))
        );
        assert_eq!(hamlet.as_locality("autreville"), None);
        let [commune, parish] = &places("Exampleville / Saint-Exemple (paroisse)")[..] else {
            panic!("two places");
        };
        assert_eq!(
            commune.as_locality("exampleville"),
            Some(Some("Saint-Exemple".to_owned()))
        );
        assert_eq!(
            parish.as_locality("exampleville"),
            Some(Some("Saint-Exemple".to_owned()))
        );
    }
}
