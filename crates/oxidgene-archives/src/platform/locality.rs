//! The forms in which a portal may write a cited locality.
//!
//! A citation writes `Le Mas-d'Exemple`; a portal's locality list may write
//! `Mas-d'Exemple (Le)`, which is how gazetteers sort. Adapters compare
//! [`forms`] of the cited locality, folded, with the portal's own text.

use serde::Deserialize;

/// How a portal writes a locality's leading article: the setting
/// `locality_style` of the adapters that search by locality label.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LocalityStyle {
    /// `Le Mans`.
    #[default]
    Plain,
    /// `Mans (Le)`.
    ArticleSuffix,
}

impl LocalityStyle {
    /// The cited locality as the portal writes it.
    pub(crate) fn write(self, locality: &str) -> String {
        let mut forms = forms(locality);
        match self {
            Self::ArticleSuffix => forms.pop(),
            Self::Plain => None,
        }
        .unwrap_or_else(|| locality.to_owned())
    }
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
