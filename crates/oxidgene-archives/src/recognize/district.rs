//! A city's numbered districts as citations write them — `Paris 11e`,
//! `Paris 11ème`, `Paris XIe`, `Paris (11e)`, `Paris-11`, `11e
//! arrondissement`, `1er arrondissement de Paris` — read by the cities the
//! catalogue lists in an archive's `citation.districts` and the district
//! words and ordinal suffixes of the vocabularies, and written alike:
//! `Paris 11e`.

use crate::citation::{CitationGrammar, District};
use crate::vocabulary::{Phrase, Vocabulary, fold};

/// A district named at the start of some words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Mention {
    /// The index of the rule whose city the words name, if they name one.
    pub city: Option<usize>,
    pub number: u8,
    /// How many words it takes.
    pub words: usize,
    /// A bare Arabic number after the city (`Paris 11`, `Paris-11`), which
    /// only a whole segment or locality reads as a district.
    pub bare: bool,
}

/// The district rules of a catalogue, with their cities folded.
pub(super) struct Districts<'a> {
    rules: Vec<(Phrase, &'a District)>,
    vocabularies: &'a [Vocabulary],
}

impl<'a> Districts<'a> {
    pub fn new(
        rules: impl IntoIterator<Item = &'a District>,
        vocabularies: &'a [Vocabulary],
    ) -> Self {
        Self {
            rules: rules
                .into_iter()
                .map(|district| (fold(&district.city), district))
                .collect(),
            vocabularies,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// The district the folded `words` name from their first: the city and
    /// its number (`Paris 11e`, `Paris XIe arrondissement`), or the number
    /// and a district word, then possibly the city (`11e arrondissement de
    /// Paris`).
    pub fn mention(&self, words: &[String]) -> Option<Mention> {
        for (index, (city, district)) in self.rules.iter().enumerate() {
            let Some(rest) = words.strip_prefix(city.as_slice()) else {
                continue;
            };
            let Some((number, taken, marked)) = self.number(rest) else {
                continue;
            };
            if !(1..=district.count).contains(&number) {
                continue;
            }
            let named = self.district_word(&rest[taken..]);
            return Some(Mention {
                city: Some(index),
                number,
                words: city.len() + taken + named,
                bare: !marked && named == 0,
            });
        }
        let (number, taken, _) = self.number(words)?;
        let named = self.district_word(&words[taken..]);
        if named == 0 {
            return None;
        }
        let end = taken + named;
        let after = end + self.particles(&words[end..]);
        let city = self
            .rules
            .iter()
            .enumerate()
            .find_map(|(index, (city, district))| {
                (words[after..].starts_with(city) && number <= district.count)
                    .then_some((index, after + city.len()))
            });
        let fits = self
            .rules
            .iter()
            .any(|(_, district)| number <= district.count);
        match city {
            Some((index, words)) => Some(Mention {
                city: Some(index),
                number,
                words,
                bare: false,
            }),
            None => fits.then_some(Mention {
                city: None,
                number,
                words: end,
                bare: false,
            }),
        }
    }

    /// The district a whole locality names, written as citations write it
    /// in the archive's language: `Paris 11e` for `Paris XIe`, `Paris
    /// (11e)`, `11e arrondissement`. A number without its city is the first
    /// city's whose count it fits.
    pub fn written(&self, locality: &str) -> Option<String> {
        let words = fold(locality);
        let mention = self
            .mention(&words)
            .filter(|mention| mention.words == words.len())?;
        let (_, district) = match mention.city {
            Some(index) => self.rules.get(index)?,
            None => self
                .rules
                .iter()
                .find(|(_, district)| mention.number <= district.count)?,
        };
        let suffix = self
            .vocabularies
            .iter()
            .find_map(|vocabulary| vocabulary.written_ordinal(mention.number))
            .unwrap_or_default();
        Some(format!("{} {}{suffix}", district.city, mention.number))
    }

    /// A district number at the start of `words`: Arabic or Roman, with an
    /// ordinal suffix glued (`11e`, `XIe`, `1er`) or as the next word
    /// (`11 ème`), or none. Its value, the words it takes, and whether it
    /// is marked as a district's — by a suffix, or by Roman numerals.
    fn number(&self, words: &[String]) -> Option<(u8, usize, bool)> {
        let word = words.first()?;
        let digits = word.len() - word.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        let (value, suffix, roman) = if digits > 0 {
            let value = word[..digits].parse::<u8>().ok().filter(|_| digits <= 2)?;
            (value, &word[digits..], false)
        } else {
            let (value, suffix) =
                roman_prefix(word, |suffix| suffix.is_empty() || self.is_ordinal(suffix))?;
            (value, suffix, true)
        };
        if !suffix.is_empty() {
            return self.is_ordinal(suffix).then_some((value, 1, true));
        }
        match words.get(1) {
            Some(next) if self.is_ordinal(next) => Some((value, 2, true)),
            _ => Some((value, 1, roman)),
        }
    }

    fn is_ordinal(&self, suffix: &str) -> bool {
        self.vocabularies
            .iter()
            .any(|vocabulary| vocabulary.is_ordinal(suffix))
    }

    /// How many words a district word at the start of `words` takes.
    fn district_word(&self, words: &[String]) -> usize {
        self.vocabularies
            .iter()
            .flat_map(Vocabulary::districts)
            .filter(|phrase| words.starts_with(phrase))
            .map(Vec::len)
            .max()
            .unwrap_or(0)
    }

    /// How many particles start `words`: `de`, `du`.
    fn particles(&self, words: &[String]) -> usize {
        words
            .iter()
            .take_while(|word| {
                self.vocabularies
                    .iter()
                    .any(|vocabulary| vocabulary.is_particle(word))
            })
            .count()
    }
}

/// The value of the Roman numeral `word` starts with, from 1 to 99, and the
/// rest, the longest numeral whose rest `accepts`.
fn roman_prefix(word: &str, accepts: impl Fn(&str) -> bool) -> Option<(u8, &str)> {
    let letters = word.len() - word.trim_start_matches(['i', 'v', 'x', 'l', 'c']).len();
    (1..=letters).rev().find_map(|length| {
        let value = roman_value(&word[..length])?;
        accepts(&word[length..]).then_some((value, &word[length..]))
    })
}

/// The value of a Roman numeral written in its canonical form, from 1 to
/// 99: `xi` but not `viiii`.
fn roman_value(numeral: &str) -> Option<u8> {
    (1..=99).find(|value| to_roman(*value) == numeral)
}

/// The canonical Roman numeral of a number from 1 to 99.
fn to_roman(mut number: u8) -> String {
    let mut written = String::new();
    for (symbol, worth) in [
        ("xc", 90),
        ("l", 50),
        ("xl", 40),
        ("x", 10),
        ("ix", 9),
        ("v", 5),
        ("iv", 4),
        ("i", 1),
    ] {
        while number >= worth {
            written.push_str(symbol);
            number -= worth;
        }
    }
    written
}

impl CitationGrammar {
    /// A recognized locality as the archive's citations are searched by: the
    /// name its portal knows it by (`localities`), or a district written
    /// alike (`Paris 11e` for `Paris XIe`).
    pub(super) fn written_locality(&self, locality: String, vocabularies: &[Vocabulary]) -> String {
        if let Some(name) = self.locality_name(&locality) {
            return name.to_owned();
        }
        Districts::new(&self.districts, vocabularies)
            .written(&locality)
            .unwrap_or(locality)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_roman_numerals_in_their_canonical_form_only() {
        for (numeral, value) in [("i", 1), ("iv", 4), ("xi", 11), ("xix", 19), ("xx", 20)] {
            assert_eq!(roman_value(numeral), Some(value), "{numeral}");
        }
        for numeral in ["", "iiii", "viiii", "xxxxx", "ic"] {
            assert_eq!(roman_value(numeral), None, "{numeral}");
        }
        let ordinal = |suffix: &str| ["", "e", "er"].contains(&suffix);
        assert_eq!(roman_prefix("xie", ordinal), Some((11, "e")));
        assert_eq!(roman_prefix("ier", ordinal), Some((1, "er")));
        assert_eq!(roman_prefix("xiv", ordinal), Some((14, "")));
        assert_eq!(roman_prefix("vin", ordinal), None);
    }
}
