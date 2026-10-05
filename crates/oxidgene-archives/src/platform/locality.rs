//! The forms in which a portal may write a cited locality.
//!
//! A citation writes `Le Mas-d'Exemple`; a portal's locality list may write
//! `Mas-d'Exemple (Le)`, which is how gazetteers sort. Adapters compare
//! [`forms`] of the cited locality, folded, with the portal's own text.

use serde::Deserialize;

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
}
