//! The join key between a Geneanet media reference and a `.gw` export.
//!
//! GeneWeb has no surrogate identifier for a person: someone *is* the triple
//! (surname, first name, occurrence). Geneanet's media API hands that triple
//! back as `reference_extra_geneweb.ref`, written `surname|firstname|occ` in a
//! folded form — lowercase, unaccented, with `_` and `-` turned into spaces,
//! and the occurrence left empty when it is zero:
//!
//! ```text
//! SURNAME_A Renée               → surname_a|renee|
//! LE SURNAME Georges_Auguste       → le surname|georges auguste|
//! Jean-Marie                   → jean marie
//! SURNAME_B Charles.1    → surname_b|charles|1
//! ```
//!
//! Reproducing that folding exactly is what lets the two sides meet.

use unicode_normalization::UnicodeNormalization;

/// Builds the Geneanet-style key for a person.
pub fn geneanet_key(surname: &str, first_name: &str, occ: u32) -> String {
    let occurrence = if occ == 0 {
        String::new()
    } else {
        occ.to_string()
    };

    format!("{}|{}|{occurrence}", fold(surname), fold(first_name))
}

/// Folds a name the way Geneanet does before putting it in a reference.
fn fold(name: &str) -> String {
    let mut folded = String::with_capacity(name.len());

    for character in name.chars() {
        match character {
            // GeneWeb writes spaces as `_` in a .gw file; Geneanet also treats
            // a hyphen and an apostrophe as word breaks, so `Jean-Marie` folds
            // to `jean marie` and `D'SURNAME_C` to `d surname c`.
            '_' | '-' | ' ' | '\'' | '\u{2019}' => folded.push(' '),
            // Letters with a stroke or bar have no canonical decomposition, so
            // NFD leaves them untouched. Latin-script genealogies hit these
            // often enough (Polish, Scandinavian, German) to be worth naming.
            'ł' | 'Ł' => folded.push('l'),
            'ø' | 'Ø' => folded.push('o'),
            'đ' | 'Đ' | 'ð' | 'Ð' => folded.push('d'),
            'ß' => folded.push_str("ss"),
            'æ' | 'Æ' => folded.push_str("ae"),
            'œ' | 'Œ' => folded.push_str("oe"),
            'þ' | 'Þ' => folded.push_str("th"),
            other => {
                // Decompose, then drop the combining marks: é → e + ´ → e.
                for decomposed in other.nfd() {
                    if !is_combining_mark(decomposed) {
                        folded.extend(decomposed.to_lowercase());
                    }
                }
            }
        }
    }

    // Collapse the runs of spaces the substitutions above may have produced.
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether a character is a combining mark (Unicode category `Mn`/`Mc`/`Me`).
///
/// Hand-rolled rather than pulled from a properties crate: after NFD, the only
/// marks that can appear over Latin letters live in these blocks.
fn is_combining_mark(character: char) -> bool {
    matches!(character as u32,
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
    fn folds_the_forms_observed_on_the_live_api() {
        // Every expectation here was read off a real reference payload.
        assert_eq!(geneanet_key("BRANCH_A", "Renée", 0), "branch a|renee|");
        assert_eq!(
            geneanet_key("LE SURNAME", "Georges_Auguste", 0),
            "le surname|georges auguste|"
        );
        assert_eq!(geneanet_key("BRANCH_B", "Charles", 1), "branch b|charles|1");
    }

    #[test]
    fn a_hyphen_is_a_word_break() {
        // This one cost a failed join on the first attempt: Geneanet folds
        // `Jean-Marie` to `jean marie`, not `jean-marie`.
        assert_eq!(geneanet_key("X", "Jean-Marie", 0), "x|jean marie|");
    }

    #[test]
    fn an_apostrophe_is_a_word_break() {
        // Found by the last unjoined reference on a 10 254-person tree:
        // `D'SURNAME_C` is `d surname c` on Geneanet, with
        // a space where the apostrophe was — not `d'surname_c` and not `dsurnamec`.
        assert_eq!(geneanet_key("D'SURNAME_C", "X", 0), "d surname c|x|");
        // Typographic apostrophes fold the same way.
        assert_eq!(fold("O\u{2019}Brien"), "o brien");
    }

    #[test]
    fn strips_accents_without_losing_the_letter() {
        assert_eq!(fold("Renée"), "renee");
        assert_eq!(fold("Léa"), "lea");
        assert_eq!(fold("Müller"), "muller");
        assert_eq!(fold("Rosé Amélie"), "rose amelie");
    }

    #[test]
    fn folds_letters_that_nfd_cannot_decompose() {
        // These have no combining-mark decomposition, so NFD alone leaves them
        // intact and the join would silently miss.
        assert_eq!(fold("Michał"), "michal");
        assert_eq!(fold("Søren"), "soren");
        assert_eq!(fold("Weiß"), "weiss");
        assert_eq!(fold("Ærø"), "aero");
    }

    #[test]
    fn collapses_the_whitespace_substitutions_produce() {
        assert_eq!(fold("Jean--Marie"), "jean marie");
        assert_eq!(fold("  Jean _ Marie  "), "jean marie");
        assert_eq!(fold("Jean_-_Marie"), "jean marie");
    }

    #[test]
    fn an_occurrence_of_zero_leaves_the_field_empty() {
        // Geneanet writes a trailing `|` rather than `|0`.
        assert!(geneanet_key("a", "b", 0).ends_with('|'));
        assert_eq!(geneanet_key("a", "b", 0), "a|b|");
        assert_eq!(geneanet_key("a", "b", 12), "a|b|12");
    }

    #[test]
    fn an_empty_name_does_not_panic() {
        assert_eq!(geneanet_key("", "", 0), "||");
        assert_eq!(fold("   "), "");
    }

    #[test]
    fn the_anonymous_person_folds_to_the_key_geneanet_uses() {
        // `? ?` in a .gw becomes `nn` on Geneanet's side; these are never
        // joinable anyway, but the key must not blow up.
        assert_eq!(geneanet_key("?", "?", 0), "?|?|");
    }
}
