//! Text folding: the one form in which two spellings of a word are compared.
//!
//! [`fold_words`] is used everywhere a typed word meets a written one: the
//! stored search tokens and the queries run against them, media tag keys,
//! the reference dictionaries, the place field and the written-date reader
//! in the interface. [`fold_text`] is the same folding with a say over the
//! characters that are neither letters nor digits, for a key another system
//! defines (Geneanet's person references).

use unicode_normalization::UnicodeNormalization;

/// What [`fold_text`] makes of a character that is neither a letter nor a
/// digit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Separator {
    /// A word break: the words on either side stay apart, one space between.
    Break,
    /// Kept as written (lowercased).
    Keep,
}

/// Fold text for matching words typed against words written: lowercase,
/// without diacritics in any script, every character other than a letter or
/// a digit read as a word break, words separated by single spaces.
///
/// "Saint-Étienne-d'Œuf" folds to "saint etienne d oeuf", "Łąka" to "laka",
/// "Dreißig" to "dreissig", "Nguyễn" to "nguyen".
pub fn fold_words(text: &str) -> String {
    fold_text(text, |_| Separator::Break)
}

/// [`fold_words`], with `other` deciding what becomes of each character that
/// is neither a letter nor a digit. Whitespace is always a break.
///
/// Letters are decomposed (NFD) and their combining marks dropped, so an
/// accented letter of any script loses its accent; the few Latin letters
/// with no decomposition — a stroke, a ligature, a thorn — are spelled out
/// (`ł` → `l`, `æ` → `ae`, `ß` → `ss`, `þ` → `th`).
pub fn fold_text(text: &str, other: impl Fn(char) -> Separator) -> String {
    let mut folded = String::with_capacity(text.len());
    for c in text.nfd() {
        if is_combining_mark(c) {
            continue;
        }
        if let Some(plain) = spelled_out(c) {
            folded.push_str(plain);
        } else if c.is_alphanumeric() {
            folded.extend(c.to_lowercase().filter(|l| !is_combining_mark(*l)));
        } else if c.is_whitespace() || other(c) == Separator::Break {
            folded.push(' ');
        } else {
            folded.extend(c.to_lowercase());
        }
    }
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The unaccented spelling of a Latin letter that canonical decomposition
/// leaves whole: a letter with a stroke or bar, a ligature, a thorn, a
/// dotless i.
fn spelled_out(c: char) -> Option<&'static str> {
    Some(match c {
        'ł' | 'Ł' => "l",
        'ø' | 'Ø' => "o",
        'đ' | 'Đ' | 'ð' | 'Ð' => "d",
        'ħ' | 'Ħ' => "h",
        'ı' => "i",
        'ŧ' | 'Ŧ' => "t",
        'ƀ' => "b",
        'æ' | 'Æ' => "ae",
        'œ' | 'Œ' => "oe",
        'ß' | 'ẞ' => "ss",
        'þ' | 'Þ' => "th",
        _ => return None,
    })
}

/// Whether a character is a combining mark, as canonical decomposition
/// leaves them after their base letter.
fn is_combining_mark(c: char) -> bool {
    unicode_normalization::char::is_combining_mark(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_fold_without_accents_case_or_punctuation() {
        assert_eq!(fold_words("Éloïse"), "eloise");
        assert_eq!(fold_words("François"), "francois");
        assert_eq!(fold_words("DUPONT"), "dupont");
        assert_eq!(fold_words("Saint-Étienne-d'Œuf"), "saint etienne d oeuf");
        assert_eq!(fold_words("  Łąka  (Źródło) "), "laka zrodlo");
        assert_eq!(fold_words("Dreißig"), "dreissig");
        assert_eq!(fold_words("sześćset"), "szescset");
        assert_eq!(fold_words("Oraș"), "oras");
        assert_eq!(fold_words("St.Malo"), "st malo");
        assert_eq!(fold_words("Laboureur/euse"), "laboureur euse");
        assert_eq!(fold_words("Cœur-d’Ŵy"), "coeur d wy");
        assert_eq!(fold_words("Søren Đorđe"), "soren dorde");
    }

    #[test]
    fn every_script_loses_its_accents() {
        assert_eq!(fold_words("Nguyễn Thị"), "nguyen thi");
        assert_eq!(fold_words("Ἀθῆναι"), "αθηναι");
        assert_eq!(fold_words("Йосиф"), "иосиф");
    }

    #[test]
    fn a_decomposed_accent_folds_with_its_letter() {
        assert_eq!(fold_words("E\u{301}loi\u{308}se"), "eloise");
        assert_eq!(fold_words("\u{130}LK"), "ilk");
    }

    #[test]
    fn kept_separators_stay_as_written() {
        let geneanet = |c: char| {
            if matches!(c, '_' | '-' | '\'' | '\u{2019}') {
                Separator::Break
            } else {
                Separator::Keep
            }
        };
        assert_eq!(
            fold_text("D'Été_Anne-Marie ?", geneanet),
            "d ete anne marie ?"
        );
        assert_eq!(fold_text("A.B", geneanet), "a.b");
    }
}
