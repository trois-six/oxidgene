//! Cutting a citation's text into segments and tokens, and finding the
//! vocabulary's phrases among them.
//!
//! A segment is what genealogists separate with a comma, a semicolon, a
//! spaced dash, a line break or parentheses: usually one fact each
//! (`vue 45/200`, `acte n° 312`, `état civil de Exampleville`). A token is a
//! word or a number as written, apostrophes and glued abbreviations split
//! (`d'Exampleville`, `n°312`), so that a phrase of the vocabulary is matched
//! against whole tokens.

use crate::vocabulary::{Meaning, Phrase, Vocabulary, fold};

/// One word or number as written, and its folded words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Token {
    pub raw: String,
    pub words: Phrase,
}

impl Token {
    fn new(raw: &str) -> Self {
        Self {
            raw: raw.to_owned(),
            words: fold(raw),
        }
    }

    /// A number of up to seven digits.
    pub fn number(&self) -> Option<u32> {
        let digits = &self.raw;
        ((1..=7).contains(&digits.len()) && digits.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| digits.parse().ok())
            .flatten()
    }

    /// A year of four digits, within the years registers are cited from.
    pub fn year(&self) -> Option<u16> {
        year(&self.raw)
    }

    /// Whether the token starts a proper name: a capital first, at least two
    /// letters, and not a short run of capitals such as a code (`GG`, `AD`).
    pub fn is_capitalized(&self) -> bool {
        let letters = self.raw.chars().filter(|c| c.is_alphabetic()).count();
        let starts = self
            .raw
            .chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() && c.is_uppercase());
        let has_lowercase = self.raw.chars().any(char::is_lowercase);
        starts
            && letters >= 2
            && (has_lowercase || letters >= 4)
            && self
                .raw
                .chars()
                .all(|c| c.is_alphabetic() || "-'’.".contains(c))
    }

    /// Whether the token ends with an apostrophe, as `d'` does.
    pub fn is_elided(&self) -> bool {
        self.raw.ends_with(['\'', '’'])
    }
}

/// A four-digit year from 1000 to 2100.
pub(super) fn year(text: &str) -> Option<u16> {
    (text.len() == 4 && text.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| text.parse::<u16>().ok())
        .flatten()
        .filter(|year| (1000..=2100).contains(year))
}

/// What a comma, a semicolon, a spaced dash, a line break or parentheses
/// separate.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Segment {
    pub tokens: Vec<Token>,
}

impl Segment {
    /// The tokens from `from` to `to`, as written.
    pub fn text(&self, from: usize, to: usize) -> String {
        let mut text = String::new();
        for (at, token) in self.tokens[from..to].iter().enumerate() {
            if at > 0 && !self.tokens[from + at - 1].is_elided() {
                text.push(' ');
            }
            text.push_str(&token.raw);
        }
        text
    }

    pub fn whole(&self) -> String {
        self.text(0, self.tokens.len())
    }
}

/// The web addresses written in `text`, with the text left once they are
/// taken out.
pub(super) fn addresses(text: &str) -> (Vec<String>, String) {
    let mut found = Vec::new();
    let mut rest = String::with_capacity(text.len());
    for piece in text.split_inclusive(char::is_whitespace) {
        let word = piece.trim_end();
        let start = word.find("https://").or_else(|| word.find("http://"));
        match start {
            Some(start) => {
                let address =
                    word[start..].trim_end_matches(['.', ',', ';', ')', ']', '>', '"', '\'']);
                if address.len() > "https://".len() {
                    found.push(address.to_owned());
                }
                rest.push_str(&word[..start]);
                rest.push(' ');
            }
            None => rest.push_str(piece),
        }
    }
    (found, rest)
}

/// The segments of a text.
pub(super) fn segments(text: &str) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut flush = |current: &mut String| {
        let tokens = tokens(current);
        if !tokens.is_empty() {
            segments.push(Segment { tokens });
        }
        current.clear();
    };
    let mut at = 0;
    while at < chars.len() {
        let c = chars[at];
        let spaced_dash = matches!(c, '-' | '–' | '—')
            && at > 0
            && chars[at - 1].is_whitespace()
            && chars.get(at + 1).is_some_and(|next| next.is_whitespace());
        match c {
            ',' | ';' | '|' | '\n' | '\r' | '(' | ')' | '[' | ']' => flush(&mut current),
            _ if spaced_dash => flush(&mut current),
            _ => current.push(c),
        }
        at += 1;
    }
    flush(&mut current);
    merge_split_periods(segments)
}

/// Joins `1745 - 1760`, which the spaced dash cut in two, back into one
/// period: a segment holding a year alone after one ending with a year.
fn merge_split_periods(segments: Vec<Segment>) -> Vec<Segment> {
    let mut merged: Vec<Segment> = Vec::with_capacity(segments.len());
    for segment in segments {
        let lone_year = segment.tokens.len() == 1 && segment.tokens[0].year().is_some();
        if lone_year
            && let Some(previous) = merged.last_mut()
            && let Some(last) = previous.tokens.last()
            && let (Some(first), Some(second)) = (last.year(), segment.tokens[0].year())
            && first <= second
        {
            let joined = format!("{}-{}", last.raw, segment.tokens[0].raw);
            *previous.tokens.last_mut().expect("a last token") = Token::new(&joined);
            continue;
        }
        merged.push(segment);
    }
    merged
}

/// The tokens of a segment: its words split on whitespace, the quotes and
/// colons around them dropped, an elided article split from its word
/// (`d'Exampleville`), and an abbreviation glued to its number split from
/// it (`n°312`, `v.45`).
fn tokens(segment: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    for piece in segment.split_whitespace() {
        let piece = piece
            .trim_start_matches(['"', '\'', '«', '»', '“', '”', '‘', '’', ':', '{', '}'])
            .trim_end_matches(['"', '«', '»', '“', '”', '’', ':', '{', '}', '!', '?']);
        let piece = piece.trim_end_matches('\'');
        // A full stop after a number ends a sentence; after letters it may
        // end an abbreviation (`v.`, `fol.`), which keeps it.
        let piece = match piece.strip_suffix('.') {
            Some(number) if number.ends_with(|c: char| c.is_ascii_digit()) => number,
            _ => piece,
        };
        let mut rest = piece;
        while !rest.is_empty() {
            let cut = split_point(rest);
            let (head, tail) = rest.split_at(cut);
            if !fold(head).is_empty() || head.bytes().any(|b| b.is_ascii_digit()) {
                tokens.push(Token::new(head));
            } else if !head.trim().is_empty() && matches!(head, "-" | "–" | "—" | "/") {
                tokens.push(Token {
                    raw: head.to_owned(),
                    words: Vec::new(),
                });
            }
            rest = tail;
        }
    }
    tokens
}

/// Where a piece splits: after an apostrophe between two letters, or after
/// the mark ending an abbreviation that a digit follows (`n°3`, `v.45`);
/// otherwise its end.
fn split_point(piece: &str) -> usize {
    let chars: Vec<(usize, char)> = piece.char_indices().collect();
    for window in 0..chars.len().saturating_sub(1) {
        let (_, c) = chars[window];
        let (next_at, next) = chars[window + 1];
        let before_letter = window > 0 && chars[window - 1].1.is_alphabetic();
        if matches!(c, '\'' | '’') && before_letter && next.is_alphabetic() {
            return next_at;
        }
        if matches!(c, '°' | 'º' | '.') && before_letter && next.is_ascii_digit() {
            let letters_only = chars[..window].iter().all(|(_, c)| c.is_alphabetic());
            if letters_only {
                return next_at;
            }
        }
    }
    piece.len()
}

/// The vocabularies, read together: a phrase means what any of them says.
pub(super) struct Lexicon<'v> {
    pub vocabularies: &'v [Vocabulary],
}

impl Lexicon<'_> {
    /// The longest phrase starting at token `at`: how many tokens it takes,
    /// and every meaning the vocabularies give it.
    pub fn at(&self, segment: &Segment, at: usize) -> Option<(usize, Vec<&Meaning>)> {
        let mut best: Option<(usize, usize, Vec<&Meaning>)> = None;
        for vocabulary in self.vocabularies {
            for (phrase, meaning) in vocabulary.phrases() {
                let Some(taken) = matches_at(segment, at, phrase) else {
                    continue;
                };
                match best.as_ref().map(|(length, _, _)| *length) {
                    Some(length) if length == phrase.len() => {
                        let meanings = &mut best.as_mut().expect("a best phrase").2;
                        if !meanings.contains(&meaning) {
                            meanings.push(meaning);
                        }
                    }
                    Some(length) if length > phrase.len() => {}
                    _ => best = Some((phrase.len(), taken, vec![meaning])),
                }
            }
        }
        best.map(|(_, taken, meanings)| (taken, meanings))
    }

    /// Whether the token at `at` begins a phrase of one of these meanings.
    pub fn starts(&self, segment: &Segment, at: usize, wanted: impl Fn(&Meaning) -> bool) -> bool {
        self.at(segment, at)
            .is_some_and(|(_, meanings)| meanings.into_iter().any(wanted))
    }

    pub fn is_particle(&self, token: &Token) -> bool {
        token.words.len() == 1
            && self
                .vocabularies
                .iter()
                .any(|vocabulary| vocabulary.is_particle(&token.words[0]))
    }

    pub fn side(&self, suffix: &str) -> Option<crate::citation::Side> {
        self.vocabularies
            .iter()
            .find_map(|vocabulary| vocabulary.side(suffix))
    }
}

/// How many tokens from `at` spell `phrase`, a word also matching its plural
/// in `s` or `x` when it has three letters or more; the phrase must end on a
/// token's end.
pub(super) fn matches_at(segment: &Segment, at: usize, phrase: &[String]) -> Option<usize> {
    let mut wanted = phrase.iter();
    let mut next = wanted.next()?;
    for (offset, token) in segment.tokens.get(at..)?.iter().enumerate() {
        if token.words.is_empty() {
            return None;
        }
        for (index, word) in token.words.iter().enumerate() {
            if !same_word(word, next) {
                return None;
            }
            match wanted.next() {
                Some(following) => next = following,
                None => return (index + 1 == token.words.len()).then_some(offset + 1),
            }
        }
    }
    None
}

fn same_word(word: &str, wanted: &str) -> bool {
    word == wanted || wanted.len() >= 3 && word.strip_suffix(['s', 'x']) == Some(wanted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(segment: &Segment) -> Vec<&str> {
        segment
            .tokens
            .iter()
            .map(|token| token.raw.as_str())
            .collect()
    }

    #[test]
    fn cuts_segments_at_the_usual_separators() {
        let segments = segments("AD99, état civil de Exampleville; vue 45/200 - acte n°312 (72)");
        let texts: Vec<Vec<&str>> = segments.iter().map(raw).collect();
        assert_eq!(
            texts,
            [
                vec!["AD99"],
                vec!["état", "civil", "de", "Exampleville"],
                vec!["vue", "45/200"],
                vec!["acte", "n°", "312"],
                vec!["72"],
            ]
        );
    }

    #[test]
    fn splits_elisions_and_glued_abbreviations() {
        let segment = &segments("registres d'état civil de l'Exampleshire, f°23 v°, v.45")[..];
        assert_eq!(
            raw(&segment[0]),
            [
                "registres",
                "d'",
                "état",
                "civil",
                "de",
                "l'",
                "Exampleshire"
            ]
        );
        assert_eq!(raw(&segment[1]), ["f°", "23", "v°"]);
        assert_eq!(raw(&segment[2]), ["v.", "45"]);
        assert_eq!(segment[0].text(4, 7), "de l'Exampleshire");
    }

    #[test]
    fn a_period_cut_by_a_spaced_dash_is_joined_back() {
        let segments = segments("BMS 1745 - 1760 - vue 3");
        assert_eq!(raw(&segments[0]), ["BMS", "1745-1760"]);
        assert_eq!(raw(&segments[1]), ["vue", "3"]);
    }

    #[test]
    fn takes_web_addresses_out() {
        let (found, rest) = addresses("Voir https://archives.example.org/ark:/1/a2. Merci");
        assert_eq!(found, ["https://archives.example.org/ark:/1/a2"]);
        assert_eq!(rest.trim(), "Voir  Merci");
    }

    #[test]
    fn proper_names_start_with_a_capital() {
        for (text, capitalized) in [
            ("Exampleville", true),
            ("Saint-Exemple", true),
            ("EXAMPLEVILLE", true),
            ("GG", false),
            ("E", false),
            ("4E", false),
            ("exampleville", false),
        ] {
            assert_eq!(Token::new(text).is_capitalized(), capitalized, "{text}");
        }
    }
}
