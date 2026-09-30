//! Text folding: the forms two spellings of a word are compared in.
//!
//! [`normalize_for_search`] writes the stored search tokens and folds the
//! queries run against them; [`fold_words`] is what the interface compares
//! typed text with.

/// Normalize a string for search: lowercase + accent folding.
///
/// Used both when writing rows to the `person_search_fts` table (and the
/// media library's normalized tags) and when normalizing incoming queries,
/// so stored tokens and query tokens always match regardless of the
/// database backend.
///
/// It deliberately differs from [`fold_words`]: each character folds to
/// exactly one (`æ` to `a`, `ß` to `s`), punctuation is left to the full-text
/// tokenizer, and fewer letters are covered. The stored tokens were written
/// this way, so a change here is only safe together with a rebuild of every
/// tree's search rows; until then a query folded the new way would miss rows
/// folded the old way.
pub fn normalize_for_search(s: &str) -> String {
    // One pass instead of `to_lowercase()` followed by a fold: the intermediate
    // lowercased `String` was pure overhead. `char::to_lowercase` yields an
    // iterator because a few characters lowercase to several (`İ` becomes `i`
    // plus a combining dot), hence the `flat_map`.
    //
    // Unlike `str::to_lowercase` this does not special-case Greek final sigma
    // (`Σ` folds to `σ` here rather than `ς`). Harmless: stored tokens and query
    // tokens both come through this function, so the two sides still agree.
    let mut out = String::with_capacity(s.len());
    out.extend(s.chars().flat_map(char::to_lowercase).map(fold_accent));
    out
}

/// Fold a single accented character to its ASCII equivalent.
fn fold_accent(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'æ' => 'a', // simplified
        'ç' => 'c',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ñ' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'ý' | 'ÿ' => 'y',
        'ð' => 'd',
        'ø' => 'o',
        'ß' => 's',
        _ => c,
    }
}

/// Fold text for matching words typed against words written: lowercase,
/// without the diacritics of Latin scripts, every character other than a
/// letter or a digit read as a word break, words separated by single spaces.
///
/// "Saint-Étienne-d'Œuf" folds to "saint etienne d oeuf", "Łąka" to "laka",
/// "Dreißig" to "dreissig".
///
/// The interface folds with this: the place field matches the tree's places
/// with it and the written-date reader looks its number words up with it. It
/// runs in WASM, so it spells out the Latin letters (Latin-1, Latin
/// Extended-A and the Romanian comma letters) rather than carrying Unicode's
/// decomposition tables. The server folds dictionary names with
/// `oxidgene_api::reference::normalize_key`, which decomposes any accented
/// letter; on the letters listed here the two agree, which a test in
/// oxidgene-api checks letter by letter, so a tree place and a dictionary
/// place match the same typed text.
pub fn fold_words(text: &str) -> String {
    let mut folded = String::with_capacity(text.len());
    for c in text.chars().flat_map(char::to_lowercase) {
        match fold_letter(c) {
            Some(plain) => folded.push_str(plain),
            // A decomposed accent (`e` + U+0301) belongs to its letter.
            None if is_combining_mark(c) => {}
            None if c.is_alphanumeric() => folded.push(c),
            None => folded.push(' '),
        }
    }
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The unaccented spelling of a lowercase Latin letter carrying a diacritic,
/// of a ligature, or of a letter with a stroke.
fn fold_letter(c: char) -> Option<&'static str> {
    Some(match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => "a",
        'æ' => "ae",
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => "c",
        'ð' | 'ď' | 'đ' => "d",
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => "e",
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => "g",
        'ĥ' => "h",
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => "i",
        'ĵ' => "j",
        'ķ' => "k",
        'ĺ' | 'ļ' | 'ľ' | 'ł' => "l",
        'ñ' | 'ń' | 'ņ' | 'ň' => "n",
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => "o",
        'œ' => "oe",
        'ŕ' | 'ŗ' | 'ř' => "r",
        'ś' | 'ŝ' | 'ş' | 'š' | 'ș' => "s",
        'ß' => "ss",
        'ţ' | 'ť' | 'ț' => "t",
        'þ' => "th",
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => "u",
        'ŵ' => "w",
        'ý' | 'ÿ' | 'ŷ' => "y",
        'ź' | 'ż' | 'ž' => "z",
        _ => return None,
    })
}

/// Whether a character is a combining mark over a Latin letter, as a text
/// already in decomposed form (NFD) carries them.
fn is_combining_mark(c: char) -> bool {
    matches!(c as u32,
        0x0300..=0x036F   // Combining Diacritical Marks
        | 0x1AB0..=0x1AFF // …Extended
        | 0x1DC0..=0x1DFF // …Supplement
        | 0x20D0..=0x20FF // …for Symbols
        | 0xFE20..=0xFE2F // Combining Half Marks
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_for_search() {
        assert_eq!(normalize_for_search("Éloïse"), "eloise");
        assert_eq!(normalize_for_search("François"), "francois");
        assert_eq!(normalize_for_search("Müller"), "muller");
        assert_eq!(normalize_for_search("Ñoño"), "nono");
        assert_eq!(normalize_for_search("DUPONT"), "dupont");
    }

    #[test]
    fn test_fold_accent() {
        assert_eq!(fold_accent('é'), 'e');
        assert_eq!(fold_accent('ç'), 'c');
        assert_eq!(fold_accent('ü'), 'u');
        assert_eq!(fold_accent('x'), 'x');
    }

    #[test]
    fn words_fold_without_accents_case_or_punctuation() {
        assert_eq!(fold_words("Saint-Étienne-d'Œuf"), "saint etienne d oeuf");
        assert_eq!(fold_words("  Łąka  (Źródło) "), "laka zrodlo");
        assert_eq!(fold_words("Dreißig"), "dreissig");
        assert_eq!(fold_words("sześćset"), "szescset");
        assert_eq!(fold_words("Oraș"), "oras");
    }

    #[test]
    fn a_decomposed_accent_folds_with_its_letter() {
        assert_eq!(fold_words("E\u{301}loi\u{308}se"), "eloise");
        assert_eq!(fold_words("\u{130}LK"), "ilk");
    }
}
