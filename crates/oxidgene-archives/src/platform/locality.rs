//! The forms in which a portal may write a cited locality.
//!
//! A citation writes `Le Mas-d'Exemple`; a portal's locality list may write
//! `Mas-d'Exemple (Le)`, which is how gazetteers sort. Adapters compare
//! [`forms`] of the cited locality, folded, with the portal's own text.

use serde::Deserialize;

use super::markup::fold;
use crate::citation::CitationParts;

/// How a portal writes a locality: the setting `locality_style` of the
/// adapters that search by locality label.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LocalityStyle {
    /// `Le Mans`.
    #[default]
    Plain,
    /// `Mans (Le)`.
    ArticleSuffix,
    /// `Mans, Le`.
    ArticleComma,
    /// `Mans - Le`.
    ArticleDash,
    /// `Le Mans (Sarthe, France)`: the name qualified by its department and
    /// country, searched by the name alone. A hamlet's
    /// `Hamlet (Commune, Department, France)` keeps its qualifier, so it is
    /// never read as its commune.
    Qualified,
    /// `05` for `Paris 5e`: the number of a city's district, two digits.
    District,
    /// `Saint-Exemple (EXAMPLEVILLE)`: a parish of a city listed apart, or
    /// an office, followed by the city in parentheses, read as the city.
    Enclosing,
}

impl LocalityStyle {
    /// The cited locality as the portal writes it.
    pub(crate) fn write(self, locality: &str) -> String {
        match self {
            Self::ArticleSuffix => forms(locality).pop(),
            Self::ArticleComma => article_after(locality, ", "),
            Self::ArticleDash => article_after(locality, " - "),
            Self::District => district(locality),
            Self::Plain | Self::Qualified | Self::Enclosing => None,
        }
        .unwrap_or_else(|| locality.to_owned())
    }

    /// A locality as the portal wrote it, as a citation writes it: `Le Bourg`
    /// for `Bourg (Le)` in the `article_suffix` style and for `Bourg, Le` in
    /// the `article_comma` one and `BOURG - LE` in the `article_dash` one
    /// (articles read whatever their case), `Le Bourg` for
    /// `Le Bourg (Exemple, France)` in the `qualified` one, `05` for `Paris 5e` in the `district` one,
    /// `EXAMPLEVILLE` for `Saint-Exemple (EXAMPLEVILLE)` in the `enclosing`
    /// one.
    /// Adapters compare a row's locality read this way with the cited one
    /// read this way too, and the live checks cite the localities a portal
    /// lists.
    pub(crate) fn cited(self, portal: &str) -> String {
        match self {
            Self::Plain => None,
            Self::ArticleSuffix => article_in_front(portal, " (", ")"),
            Self::ArticleComma => article_in_front(portal, ", ", ""),
            Self::ArticleDash => article_in_front(portal, " - ", ""),
            Self::Qualified => {
                let name = without_qualifier(portal);
                Some(article_in_front(name, " (", ")").unwrap_or_else(|| name.to_owned()))
            }
            Self::District => district(portal),
            Self::Enclosing => enclosed(portal).map(|(_, city)| city.to_owned()),
        }
        .unwrap_or_else(|| portal.to_owned())
    }

    /// The parish a locality names in the `enclosing` style: `Saint-Exemple`
    /// for `Saint-Exemple (EXAMPLEVILLE)`.
    pub(crate) fn parish(self, portal: &str) -> Option<String> {
        match self {
            Self::Enclosing => enclosed(portal).map(|(parish, _)| parish.to_owned()),
            _ => None,
        }
    }
}

/// The parish and the city of `Parish (City)`, the city without the
/// department and country that may qualify it (`Office (City, Department,
/// France)`).
fn enclosed(portal: &str) -> Option<(&str, &str)> {
    portal
        .strip_suffix(')')
        .and_then(|rest| rest.rsplit_once(" ("))
        .map(|(parish, city)| (parish, city.split(',').next().unwrap_or_default().trim()))
        .filter(|(parish, city)| !parish.is_empty() && !city.is_empty())
}

/// `Le Bourg` for `Bourg` followed by `opening`, an article and `closing`
/// (`Bourg (Le)`, `Bourg, Le`).
fn article_in_front(portal: &str, opening: &str, closing: &str) -> Option<String> {
    let (name, article) = portal.strip_suffix(closing)?.rsplit_once(opening)?;
    let article = ARTICLES
        .iter()
        .map(|known| known.trim_end())
        .find(|known| known.eq_ignore_ascii_case(article))?;
    if name.is_empty() {
        return None;
    }
    let separator = if article.ends_with(['\'', '\u{2019}']) {
        ""
    } else {
        " "
    };
    Some(format!("{article}{separator}{name}"))
}

/// The locality with its leading article behind its name, after
/// `separator`: `Bourg, Le`.
fn article_after(locality: &str, separator: &str) -> Option<String> {
    split_article(locality)
        .map(|(article, name)| format!("{name}{separator}{}", article.trim_end()))
}

/// The leading article of a locality, as listed in [`ARTICLES`], and the
/// name after it.
fn split_article(locality: &str) -> Option<(&'static str, &str)> {
    ARTICLES.iter().find_map(|article| {
        locality
            .strip_prefix(article)
            .filter(|name| !name.is_empty())
            .map(|name| (*article, name))
    })
}

/// A locality without its trailing `(Department, Country)` qualifier: two
/// parts in the parentheses, so that a hamlet's three keep theirs.
fn without_qualifier(portal: &str) -> &str {
    portal
        .strip_suffix(')')
        .and_then(|rest| rest.rsplit_once(" ("))
        .filter(|(name, qualifier)| !name.is_empty() && qualifier.matches(',').count() == 1)
        .map_or(portal, |(name, _)| name.trim_end())
}

/// The first number in a locality, of one or two digits, written with two:
/// `05` for `Paris 5e`, `Paris 05` or `5`.
fn district(locality: &str) -> Option<String> {
    locality
        .split(|c: char| !c.is_ascii_digit())
        .find(|digits| !digits.is_empty())
        .filter(|digits| digits.len() <= 2)
        .and_then(|digits| digits.parse::<u8>().ok())
        .map(|number| format!("{number:02}"))
}

/// The articles moved behind the name in a gazetteer's form.
const ARTICLES: [&str; 5] = ["Les ", "Le ", "La ", "L'", "L\u{2019}"];

/// The cited locality as written, then, when it starts with an article, in
/// the gazetteer form `Name (Article)`.
pub(crate) fn forms(locality: &str) -> Vec<String> {
    let mut forms = vec![locality.to_owned()];
    for article in ARTICLES {
        if let Some(name) = locality.strip_prefix(article)
            && !name.is_empty()
        {
            forms.push(format!("{name} ({})", article.trim_end()));
            break;
        }
    }
    forms
}

/// The locality a label of a portal's list names, as a citation writes it.
///
/// Thesaurus lists qualify their labels: by a department and a country
/// (`Exampleville (Exemple, France)`), with a note after a semicolon
/// (`Exampleville (Exemple, France ; jusqu'à 1919)`), by an office or a part
/// of a city (`EXAMPLEVILLE (BUREAU DE L'ENREGISTREMENT)`,
/// `EXAMPLEVILLE (NORD-EST)`), and follow them with a bracketed note
/// (`[aujourd'hui : …]`); they write a leading article behind the name
/// (`BOURG (LE)`). The name is the label without its note and qualifier,
/// its article in front. A hamlet, whose qualifier also names its commune
/// (`Hameau (Exampleville, Exemple, France ; hameau)`), keeps its whole
/// label: it is not its commune.
pub(crate) fn label_name(label: &str) -> String {
    let mut name = label.trim();
    if name.ends_with(']')
        && let Some((before, _)) = name.rsplit_once(" [")
    {
        name = before.trim_end();
    }
    if let Some(cited) = article_in_front(name, " (", ")") {
        return cited;
    }
    let Some((before, qualifier)) = name
        .strip_suffix(')')
        .and_then(|rest| rest.rsplit_once(" ("))
        .filter(|(before, _)| !before.trim().is_empty())
    else {
        return name.to_owned();
    };
    let place = qualifier.split(';').next().unwrap_or_default();
    if place.matches(',').count() >= 2 {
        return name.to_owned();
    }
    let before = before.trim_end();
    article_in_front(before, " (", ")").unwrap_or_else(|| before.to_owned())
}

/// The labels of a portal's list that name the cited locality, given in
/// its `forms`: those written exactly as cited, case, accents and
/// punctuation aside, or failing any, those whose [`label_name`] is the
/// cited one (`Exampleville (Exemple, France)` and
/// `EXAMPLEVILLE (BUREAU DE L'ENREGISTREMENT)` for `Exampleville`).
pub(crate) fn matching_labels<'l>(labels: &'l [String], forms: &[&str]) -> Vec<&'l str> {
    let wanted: Vec<String> = forms.iter().map(|form| fold(form)).collect();
    let exact: Vec<&str> = labels
        .iter()
        .filter(|label| wanted.contains(&fold(label)))
        .map(String::as_str)
        .collect();
    if !exact.is_empty() {
        return exact;
    }
    labels
        .iter()
        .filter(|label| wanted.contains(&fold(&label_name(label))))
        .map(String::as_str)
        .collect()
}

/// The words, folded, that open the qualifier a commune's name may have
/// gained or lost when it was renamed: `-sur-Seine`, `-sous-Bois`,
/// `-en-Vexin`, `-lès-Exemple`, `-la-Forêt`, `-d'Exemple`.
const QUALIFIER_WORDS: [&str; 15] = [
    "sur", "sous", "en", "les", "lez", "pres", "de", "du", "des", "d", "la", "le", "l", "aux", "au",
];

/// The locality without the qualifier closing its name, as a portal may
/// list a commune renamed since the citation was written: `Exampleville`
/// for `Exampleville-sous-Bois` or `Exampleville sous Bois` (a folded
/// form), `La Ville` for `La Ville-en-Plaine`. The qualifier starts at the
/// first word of [`QUALIFIER_WORDS`] after the name's first word, a leading
/// article aside, and has a word after it. `None` for a name without one.
pub(crate) fn shortened(locality: &str) -> Option<&str> {
    let separators = locality
        .char_indices()
        .filter(|(_, c)| matches!(c, '-' | ' '))
        .map(|(at, _)| at);
    for at in separators {
        let before = locality[..at].trim_end();
        let named = !before.is_empty()
            && !ARTICLES
                .iter()
                .any(|article| fold(article.trim_end()) == fold(before));
        if !named {
            continue;
        }
        let rest = fold(&locality[at + 1..]);
        let mut words = rest.split(' ');
        let opens = words
            .next()
            .is_some_and(|word| QUALIFIER_WORDS.contains(&word));
        if opens && words.next().is_some_and(|word| !word.is_empty()) {
            return Some(before);
        }
    }
    None
}

/// How a portal's name stands for the cited locality under another name.
/// The order is the order of preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Renamed {
    /// One of the other names the place dictionary knows for it within the
    /// archive's areas: the same commune.
    Known,
    /// The cited name without its qualifier ([`shortened`]): `Exampleville`
    /// for `Exampleville-sous-Bois`.
    Shortened,
    /// A name extending the cited one, its shortened form or one of its
    /// known names at a word: `Exampleville-en-Plaine`,
    /// `Exampleville-Billancourt`, `Exampleville (Department, France)`.
    Extended,
}

/// The names under which a portal may list the cited locality when it lists
/// none as cited, folded: what the place dictionary knows of it, and what
/// its own name suggests (Archive Portals §5.1).
#[derive(Debug, Clone)]
pub(crate) struct OtherNames {
    cited: Vec<String>,
    known: Vec<String>,
    shortened: Option<String>,
}

impl OtherNames {
    /// The other names of the cited locality, which a portal writes in the
    /// `written` forms (as cited, and in the portal's style).
    pub(crate) fn new(citation: &CitationParts, written: &[&str]) -> Self {
        let mut cited: Vec<String> = written
            .iter()
            .copied()
            .chain([citation.locality.as_str()])
            .map(fold)
            .filter(|form| !form.is_empty())
            .collect();
        cited.dedup();
        let mut known: Vec<String> = Vec::new();
        for name in &citation.alternate_localities {
            for form in forms(name) {
                let form = fold(&form);
                if !form.is_empty() && !cited.contains(&form) && !known.contains(&form) {
                    known.push(form);
                }
            }
        }
        let shortened = shortened(&citation.locality)
            .map(fold)
            .filter(|name| !name.is_empty() && !cited.contains(name));
        Self {
            cited,
            known,
            shortened,
        }
    }

    /// How a portal's name, read as a citation writes it, stands for the
    /// cited locality under another name; `None` for the cited name itself
    /// and for a name of no relation to it.
    pub(crate) fn renamed(&self, name: &str) -> Option<Renamed> {
        let folded = fold(name);
        if folded.is_empty() || self.cited.contains(&folded) {
            return None;
        }
        if self.known.contains(&folded) {
            return Some(Renamed::Known);
        }
        if self.shortened.as_ref() == Some(&folded) {
            return Some(Renamed::Shortened);
        }
        self.cited
            .iter()
            .chain(&self.known)
            .chain(&self.shortened)
            .any(|base| extends(name, base))
            .then_some(Renamed::Extended)
    }

    /// Whether a portal's name is one the place dictionary knows for the
    /// cited locality.
    pub(crate) fn is_known(&self, name: &str) -> bool {
        self.renamed(name) == Some(Renamed::Known)
    }

    /// The items whose name, as `name` reads it, stands best for the cited
    /// locality under another name: all those of the most preferred
    /// [`Renamed`] any of them has, none when none has one.
    pub(crate) fn best<T>(
        &self,
        items: impl IntoIterator<Item = T>,
        name: impl Fn(&T) -> String,
    ) -> Vec<T> {
        let mut best: Vec<T> = Vec::new();
        let mut rank = None;
        for item in items {
            let Some(renamed) = self.renamed(&name(&item)) else {
                continue;
            };
            if rank.is_none_or(|rank| renamed < rank) {
                rank = Some(renamed);
                best.clear();
            }
            if rank == Some(renamed) {
                best.push(item);
            }
        }
        best
    }
}

/// Whether `name` extends the folded `base` at a word of its own name: a
/// hyphen or a space, then a word or a parenthesised qualifier
/// (`Exampleville-en-Plaine`, `Exampleville Billancourt`, `Exampleville
/// (Department, France)`), not a part of the place after a dash, a comma or
/// a slash (`EXAMPLEVILLE - Section A`, `Exampleville, paroisse …`).
fn extends(name: &str, base: &str) -> bool {
    name.char_indices()
        .filter(|(_, c)| matches!(c, '-' | ' '))
        .any(|(at, separator)| {
            let mut after = name[at + separator.len_utf8()..].chars();
            let word = match (separator, after.next()) {
                ('-', Some(next)) => next.is_alphanumeric(),
                (' ', Some(next)) => next.is_alphanumeric() || next == '(',
                _ => false,
            };
            let before = &name[..at];
            word && before.chars().last().is_some_and(char::is_alphanumeric) && fold(before) == base
        })
}

/// The name a portal matching the locality as text is searched for once
/// more when the cited name finds nothing there: the first name the place
/// dictionary knows for it that does not hold the cited one — a search for
/// the cited name has found any that does —, else the cited name
/// shortened, whose matches hold every name extending it.
pub(crate) fn search_again(citation: &CitationParts) -> Option<String> {
    let cited = fold(&citation.locality);
    if cited.is_empty() {
        return None;
    }
    citation
        .alternate_localities
        .iter()
        .find(|name| {
            let folded = fold(name);
            !folded.is_empty() && !folded.contains(&cited)
        })
        .cloned()
        .or_else(|| shortened(&citation.locality).map(str::to_owned))
}

/// The labels of a portal's list that name the cited locality: as
/// [`matching_labels`] finds them, or failing any, those naming it under
/// another name ([`OtherNames::best`]), each read by [`label_name`]. A
/// register a label found that way is chosen only on the citation's
/// evidence ([`super::select`]).
pub(crate) fn naming_labels<'l>(
    labels: &'l [String],
    forms: &[&str],
    citation: &CitationParts,
) -> Vec<&'l str> {
    let exact = matching_labels(labels, forms);
    if !exact.is_empty() {
        return exact;
    }
    OtherNames::new(citation, forms)
        .best(labels.iter().map(String::as_str), |label| label_name(label))
}

/// The start of the locality's name, without its article, up to the first
/// space or apostrophe: what a portal's prefix lookup matches whichever way
/// the rest is written (`Mas-d'Exemple` and `Mas-d’Exemple` both start with
/// `Mas-d`), at most `limit` characters.
pub(crate) fn name_start(locality: &str, limit: usize) -> String {
    let name = ARTICLES
        .iter()
        .find_map(|article| locality.strip_prefix(article))
        .unwrap_or(locality);
    let start: String = name
        .chars()
        .take_while(|c| !c.is_whitespace() && !matches!(c, '\'' | '\u{2019}'))
        .take(limit)
        .collect();
    start.trim_end_matches('-').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_leading_article_behind_the_name() {
        assert_eq!(forms("Exampleville"), ["Exampleville"]);
        assert_eq!(forms("Le Bourg"), ["Le Bourg", "Bourg (Le)"]);
        assert_eq!(forms("L'Isle"), ["L'Isle", "Isle (L')"]);
        assert_eq!(forms("Les Hauts"), ["Les Hauts", "Hauts (Les)"]);
        assert_eq!(forms("Lemont"), ["Lemont"]);
    }

    #[test]
    fn writes_a_locality_in_the_portals_style() {
        assert_eq!(LocalityStyle::Plain.write("Le Bourg"), "Le Bourg");
        assert_eq!(LocalityStyle::ArticleSuffix.write("Le Bourg"), "Bourg (Le)");
        assert_eq!(LocalityStyle::ArticleSuffix.write("Bourg"), "Bourg");
    }

    #[test]
    fn reads_a_locality_of_the_portal_back_as_cited() {
        let suffixed = LocalityStyle::ArticleSuffix;
        for (portal, cited) in [
            ("Bourg (Le)", "Le Bourg"),
            ("Ville-Example (La)", "La Ville-Example"),
            ("Examples (Les)", "Les Examples"),
            ("Exemple (L')", "L'Exemple"),
            ("Exampleville", "Exampleville"),
            (
                "Exampleville (Saint-Exemple)",
                "Exampleville (Saint-Exemple)",
            ),
            (" (Le)", " (Le)"),
        ] {
            assert_eq!(suffixed.cited(portal), cited, "{portal}");
        }
        // Read back, then written again, a listed locality is searched as
        // listed.
        assert_eq!(suffixed.write(&suffixed.cited("Bourg (Le)")), "Bourg (Le)");
        assert_eq!(LocalityStyle::Plain.cited("Bourg (Le)"), "Bourg (Le)");
    }

    #[test]
    fn writes_and_reads_an_article_after_a_comma() {
        let comma = LocalityStyle::ArticleComma;
        for (cited, portal) in [
            ("Le Bourg", "Bourg, Le"),
            ("L'Exemple", "Exemple, L'"),
            ("Les Hauts-Exemples", "Hauts-Exemples, Les"),
            ("Exampleville", "Exampleville"),
        ] {
            assert_eq!(comma.write(cited), portal, "{cited}");
            assert_eq!(comma.cited(portal), cited, "{portal}");
        }
        // A comma not followed by an article is part of the name.
        assert_eq!(comma.cited("Exemple, Saint"), "Exemple, Saint");

        // The same behind a dash, the article read whatever its case.
        let dash = LocalityStyle::ArticleDash;
        assert_eq!(dash.write("Le Bourg"), "Bourg - Le");
        assert_eq!(dash.cited("BOURG - LE"), "Le BOURG");
        assert_eq!(dash.cited("Saint-Exemple - Ouest"), "Saint-Exemple - Ouest");
    }

    #[test]
    fn reads_a_parish_or_an_office_as_the_city_enclosing_it() {
        let enclosing = LocalityStyle::Enclosing;
        assert_eq!(enclosing.write("Exampleville"), "Exampleville");
        assert_eq!(
            enclosing.cited("Saint-Exemple (EXAMPLEVILLE)"),
            "EXAMPLEVILLE"
        );
        assert_eq!(
            enclosing.cited("Bureau d'enregistrement (Exampleville, Exemple, France)"),
            "Exampleville"
        );
        assert_eq!(enclosing.cited("Exampleville"), "Exampleville");
        assert_eq!(
            enclosing.parish("Saint-Exemple (EXAMPLEVILLE)").as_deref(),
            Some("Saint-Exemple")
        );
        assert_eq!(
            LocalityStyle::Plain.parish("Saint-Exemple (EXAMPLEVILLE)"),
            None
        );
    }

    #[test]
    fn reads_a_qualified_locality_but_not_its_hamlets() {
        let qualified = LocalityStyle::Qualified;
        assert_eq!(qualified.write("Le Bourg"), "Le Bourg");
        for (portal, cited) in [
            ("Exampleville (Exemple, France)", "Exampleville"),
            ("Bourg (Le) (Exemple, France)", "Le Bourg"),
            ("Le Bourg (Exemple, France)", "Le Bourg"),
            ("Exampleville", "Exampleville"),
            // A hamlet is not its commune.
            (
                "Hameau (Exampleville, Exemple, France)",
                "Hameau (Exampleville, Exemple, France)",
            ),
        ] {
            assert_eq!(qualified.cited(portal), cited, "{portal}");
        }
    }

    #[test]
    fn writes_a_district_with_two_digits() {
        let district = LocalityStyle::District;
        for (cited, portal) in [
            ("Exampleville 5e", "05"),
            ("Exampleville 05", "05"),
            ("12", "12"),
            ("Exampleville", "Exampleville"),
            ("Exampleville 75005", "Exampleville 75005"),
        ] {
            assert_eq!(district.write(cited), portal, "{cited}");
            assert_eq!(district.cited(cited), portal, "{cited}");
        }
    }

    #[test]
    fn reads_the_locality_a_thesaurus_label_names() {
        for (label, name) in [
            ("Exampleville (Exemple, France)", "Exampleville"),
            ("EXAMPLEVILLE (EXEMPLE, FRANCE)", "EXAMPLEVILLE"),
            (
                "Exampleville (Exemple, France ; jusqu'à 1919) [aujourd'hui : Sampleton (Exemple, France)]",
                "Exampleville",
            ),
            ("EXAMPLEVILLE (BUREAU DE L'ENREGISTREMENT)", "EXAMPLEVILLE"),
            ("EXAMPLEVILLE (NORD-EST)", "EXAMPLEVILLE"),
            ("BOURG-EXEMPLE (LE)", "Le BOURG-EXEMPLE"),
            ("Bourg (Le) (Exemple, France)", "Le Bourg"),
            ("L-EXEMPLE", "L-EXEMPLE"),
            ("Exampleville", "Exampleville"),
            // A hamlet keeps the commune it belongs to.
            (
                "HAMEAU (EXAMPLEVILLE, EXEMPLE, FRANCE ; HAMEAU)",
                "HAMEAU (EXAMPLEVILLE, EXEMPLE, FRANCE ; HAMEAU)",
            ),
            (" (Exemple, France)", "(Exemple, France)"),
        ] {
            assert_eq!(label_name(label), name, "{label}");
        }
    }

    #[test]
    fn matches_the_labels_naming_a_locality() {
        let labels: Vec<String> = [
            "EXAMPLEVILLE",
            "EXAMPLEVILLE (NORD-EST)",
            "BOURG-EXEMPLE (LE)",
            "SAMPLETON (BUREAU DE L'ENREGISTREMENT)",
            "SAMPLETON (SUBDIVISION MILITAIRE)",
        ]
        .map(str::to_owned)
        .into();
        // A label written as cited is preferred to a qualified one.
        assert_eq!(
            matching_labels(&labels, &["Exampleville"]),
            ["EXAMPLEVILLE"]
        );
        assert_eq!(
            matching_labels(&labels, &["Le Bourg-Exemple", "Bourg-Exemple (Le)"]),
            ["BOURG-EXEMPLE (LE)"]
        );
        assert_eq!(matching_labels(&labels, &["Sampleton"]).len(), 2);
        assert!(matching_labels(&labels, &["Elsewhere"]).is_empty());
    }

    #[test]
    fn starts_the_name_before_a_space_or_an_apostrophe() {
        assert_eq!(name_start("Le Mas-d'Exemple", 30), "Mas-d");
        assert_eq!(
            name_start("Exampleville-sur-Mer", 30),
            "Exampleville-sur-Mer"
        );
        assert_eq!(name_start("Saint Exemple", 30), "Saint");
        assert_eq!(name_start("L\u{2019}Isle", 30), "Isle");
        assert_eq!(name_start("Exampleville-sur-Mer", 7), "Example");
    }

    #[test]
    fn shortens_a_name_before_its_qualifier() {
        for (locality, short) in [
            ("Exampleville-sous-Bois", Some("Exampleville")),
            ("Exampleville-sur-Mer", Some("Exampleville")),
            ("Saint-Exemple-en-Plaine", Some("Saint-Exemple")),
            ("Saint-Exemple-lès-Exampleville", Some("Saint-Exemple")),
            ("Pont-d'Exemple", Some("Pont")),
            ("Bourg-la-Forêt", Some("Bourg")),
            ("La Ville-aux-Bois", Some("La Ville")),
            ("Les Examples-sous-Bois", Some("Les Examples")),
            // A folded name, its hyphens become spaces.
            ("exampleville sous bois", Some("exampleville")),
            // No qualifier: a compound name, a leading article, a name
            // ending with the qualifier's word.
            ("Saint-Exemple", None),
            ("Le Bourg", None),
            ("La Ville-Example", None),
            ("Exampleville-le", None),
            ("Exampleville", None),
        ] {
            assert_eq!(shortened(locality), short, "{locality}");
        }
    }

    fn parts(locality: &str, alternates: &[&str]) -> CitationParts {
        let mut citation = CitationParts::parse(
            &format!("AB12 - {locality} - (aucun) - N - 1903"),
            &crate::citation::CitationGrammar::default(),
        )
        .unwrap();
        citation.alternate_localities = alternates.iter().map(|name| (*name).to_owned()).collect();
        citation
    }

    #[test]
    fn tells_how_a_portal_s_name_stands_for_a_renamed_locality() {
        let citation = parts("Exampleville-sous-Bois", &["Nouvelleville"]);
        let others = OtherNames::new(&citation, &[&citation.locality]);
        for (name, renamed) in [
            ("Exampleville-sous-Bois", None),
            ("EXAMPLEVILLE SOUS BOIS", None),
            ("Nouvelleville", Some(Renamed::Known)),
            ("Exampleville", Some(Renamed::Shortened)),
            ("Exampleville-Billancourt", Some(Renamed::Extended)),
            (
                "Exampleville (Exampledept, France)",
                Some(Renamed::Extended),
            ),
            (
                "Exampleville-sous-Bois-et-Autreville",
                Some(Renamed::Extended),
            ),
            ("Nouvelleville-en-Plaine", Some(Renamed::Extended)),
            // A part of the place after a dash or a comma, another name.
            ("EXAMPLEVILLE - Section A", None),
            ("Exampleville, paroisse Saint-Exemple", None),
            ("Examplevilleneuve", None),
            ("Autreville", None),
        ] {
            assert_eq!(others.renamed(name), renamed, "{name}");
        }
        assert!(others.is_known("NOUVELLEVILLE"));
        // The most preferred kind wins, all of its names kept.
        let names = [
            "Exampleville-Billancourt",
            "Exampleville",
            "Autreville",
            "Exampleville-la-Forêt",
        ];
        assert_eq!(
            others.best(names, |name| (*name).to_owned()),
            ["Exampleville"]
        );
        assert_eq!(
            others.best(
                ["Exampleville-Billancourt", "Exampleville-la-Forêt"],
                |name| (*name).to_owned()
            ),
            ["Exampleville-Billancourt", "Exampleville-la-Forêt"]
        );
        assert!(
            others
                .best(["Autreville"], |name| (*name).to_owned())
                .is_empty()
        );
    }

    #[test]
    fn searches_again_for_a_known_name_or_the_shortened_one() {
        // A known name the cited one's search has not found already.
        assert_eq!(
            search_again(&parts("Ancienville", &["Nouvelleville"])).as_deref(),
            Some("Nouvelleville")
        );
        assert_eq!(
            search_again(&parts("Exampleville", &["Exampleville-en-Vexin"])),
            None
        );
        assert_eq!(
            search_again(&parts("Exampleville-sous-Bois", &[])).as_deref(),
            Some("Exampleville")
        );
        assert_eq!(search_again(&parts("Exampleville", &[])), None);
    }

    #[test]
    fn names_a_label_under_another_name_only_when_none_bears_the_cited_one() {
        let labels: Vec<String> = [
            "EXAMPLEVILLE (EXEMPLE, FRANCE)",
            "EXAMPLEVILLE - Section A",
            "AUTREVILLE",
        ]
        .map(str::to_owned)
        .into();
        let citation = parts("Exampleville-sous-Bois", &[]);
        assert_eq!(
            naming_labels(&labels, &[&citation.locality], &citation),
            ["EXAMPLEVILLE (EXEMPLE, FRANCE)"]
        );
        let citation = parts("Autreville", &[]);
        assert_eq!(
            naming_labels(&labels, &[&citation.locality], &citation),
            ["AUTREVILLE"]
        );
        let citation = parts("Elsewhere", &[]);
        assert!(naming_labels(&labels, &[&citation.locality], &citation).is_empty());
    }
}
