//! Reading one text — a source title, a citation's page, a repository's
//! name — into the facts its words state, order-independent: each segment
//! is read token by token, a keyword of the vocabulary deciding what the
//! tokens after it are.

use crate::catalog::{Archive, Level};
use crate::citation::{Act, ActKind, CallNumber, CitedView, republican_numeral, republican_start};
use crate::vocabulary::{Family, Meaning, fold};

use super::lex::{Lexicon, Segment, Token, matches_at, year};

/// How a locality is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Placing {
    /// After a document word and a preposition: `état civil de Exampleville`.
    Introduced,
    /// A segment of its own: `AD99, Exampleville, BMS 1745`.
    Standalone,
    /// Proper names within a segment saying something else.
    Loose,
}

/// What one text states.
#[derive(Debug, Clone, Default)]
pub(super) struct Facts {
    /// Archives named, by their index in the catalogue: by a citation code
    /// (`AD44`), a name or an alias, or a kind of archive with its area or
    /// code (`Arch. dép. de la Sarthe`, `AD 72`).
    pub archives: Vec<usize>,
    /// An archive is named that the catalogue does not list.
    pub uncatalogued: bool,
    pub kinds: Vec<ActKind>,
    /// Act codes (`BMS`), tables and series.
    pub documents: Vec<Act>,
    pub families: Vec<Family>,
    /// A period written with a document, or a range of years.
    pub period: Option<(String, u16)>,
    pub years: Vec<(String, u16)>,
    /// The years of full dates, the act's rather than the register's.
    pub date_years: Vec<u16>,
    pub call_number: Option<String>,
    /// A segment shaped like a call number, without its keyword.
    pub shaped_call_number: Option<String>,
    pub number: Option<u32>,
    pub views: Option<(Vec<CitedView>, Option<u16>)>,
    pub folio: Option<String>,
    pub localities: Vec<(String, Placing)>,
    pub parish: Option<String>,
}

impl Facts {
    /// Whether the text reads as a register citation: it names a document,
    /// a view, an act number, a folio or a call number.
    pub fn is_documentary(&self) -> bool {
        !self.kinds.is_empty()
            || !self.documents.is_empty()
            || !self.families.is_empty()
            || self.views.is_some()
            || self.number.is_some()
            || self.folio.is_some()
            || self.call_number.is_some()
    }

    /// The document kind the text names: its act kinds when it names some —
    /// `baptême` within `BMS de Saint-Exemple` — or else the first act code,
    /// table or series.
    pub fn act(&self) -> Option<Act> {
        if !self.kinds.is_empty() {
            let mut kinds = Vec::new();
            for kind in &self.kinds {
                if !kinds.contains(kind) {
                    kinds.push(*kind);
                }
            }
            return Some(Act::Register(kinds));
        }
        self.documents.first().cloned()
    }

    /// The register's year: a period written with its document — a
    /// military series' class among them —, a year alone, or failing those the year of a full date.
    pub fn year(&self) -> Option<(Option<String>, u16)> {
        if let Some((text, year)) = &self.period {
            return Some((Some(text.clone()), *year));
        }
        if let Some((text, year)) = self.years.first() {
            return Some((Some(text.clone()), *year));
        }
        self.date_years.first().map(|year| (None, *year))
    }
}

/// What the tokens just read were about, deciding what follows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Context {
    None,
    /// An act, a register, a table or a series.
    Document,
}

/// Reads the segments of one text.
pub(super) struct Scanner<'a> {
    pub lexicon: Lexicon<'a>,
    pub archives: &'a [Archive],
}

impl Scanner<'_> {
    pub fn read(&self, segments: &[Segment]) -> Facts {
        let mut facts = Facts::default();
        for segment in segments {
            self.segment(segment, &mut facts);
        }
        facts
    }

    fn segment(&self, segment: &Segment, facts: &mut Facts) {
        let tokens = &segment.tokens;
        if tokens.len() == 1
            && let Some(views) = self.bare_views(&tokens[0].raw)
        {
            facts.views.get_or_insert(views);
            return;
        }
        if self.is_call_number(segment) {
            facts.shaped_call_number.get_or_insert(segment.whole());
            return;
        }
        let mut reading = Reading {
            context: Context::None,
            other_number: false,
            after_folio: false,
        };
        let mut at = 0;
        while at < tokens.len() {
            at = self.step(segment, at, &mut reading, facts).max(at + 1);
        }
    }

    /// A segment shaped like a call number (`4E 1234`, `GG 45`, `6 M 123`)
    /// that starts with nothing else the vocabulary or the catalogue knows.
    fn is_call_number(&self, segment: &Segment) -> bool {
        let text = segment.whole();
        let first = &segment.tokens[0];
        CallNumber::is_shaped(&text)
            && first.year().is_none()
            && self.code_at(segment, 0).is_none()
            && !self.is_uncatalogued_code(first)
            && act_code(&first.raw).is_none()
            && self.lexicon.at(segment, 0).is_none()
    }

    /// Reads from token `at`, returning where the next reading starts.
    fn step(
        &self,
        segment: &Segment,
        at: usize,
        reading: &mut Reading,
        facts: &mut Facts,
    ) -> usize {
        let tokens = &segment.tokens;
        let token = &tokens[at];
        if let Some((archive, taken)) = self.code_at(segment, at) {
            facts.archives.push(archive);
            return at + taken;
        }
        if self.is_uncatalogued_code(token) {
            facts.uncatalogued = true;
            return at + 1;
        }
        if let Some((year, taken)) = self.date_at(segment, at) {
            facts.date_years.push(year);
            return at + taken;
        }
        if let Some((text, year, taken, range)) = self.period_at(segment, at) {
            if reading.context == Context::Document || range {
                facts.period.get_or_insert((text, year));
            } else {
                facts.years.push((text, year));
            }
            return at + taken;
        }
        if let Some(act) = act_code(&token.raw) {
            let alone = tokens.len() == 1;
            let dated = at + 1 < tokens.len() && self.period_at(segment, at + 1).is_some();
            if token.raw.len() >= 2 || alone || dated {
                facts.documents.push(act);
                reading.context = Context::Document;
                return at + 1;
            }
        }
        if let Some(archive) = self.alias_at(segment, at) {
            facts.archives.push(archive);
            return at + tokens_of_alias(segment, at, &self.archives[archive]);
        }
        if let Some((taken, meanings)) = self.lexicon.at(segment, at) {
            return self.keyword(segment, at, taken, &meanings, reading, facts);
        }
        if let Some((text, end)) = self.place_run(segment, at) {
            Self::place(segment, text, at, end, facts);
            return end;
        }
        at + 1
    }

    /// Reads a keyword of `taken` tokens at `at`.
    fn keyword(
        &self,
        segment: &Segment,
        at: usize,
        taken: usize,
        meanings: &[&Meaning],
        reading: &mut Reading,
        facts: &mut Facts,
    ) -> usize {
        let next = at + taken;
        let has = |wanted: fn(&Meaning) -> bool| meanings.iter().any(|meaning| wanted(meaning));
        if reading.after_folio && has(|m| matches!(m, Meaning::Recto | Meaning::Verso)) {
            if let Some(folio) = &mut facts.folio {
                folio.push(' ');
                folio.push_str(&segment.text(at, next));
            }
            reading.after_folio = false;
            return next;
        }
        reading.after_folio = false;
        let first = meanings
            .iter()
            .find(|meaning| !matches!(meaning, Meaning::Recto | Meaning::Verso | Meaning::Range))
            .copied();
        match first {
            Some(Meaning::Archive(level)) => self.archive(segment, next, *level, facts),
            Some(Meaning::Act(kind)) => {
                facts.kinds.push(*kind);
                reading.context = Context::Document;
                next
            }
            Some(Meaning::Family(family)) => {
                facts.families.push(*family);
                reading.context = Context::Document;
                next
            }
            Some(Meaning::Document(act)) => {
                facts.documents.push(act.clone());
                reading.context = Context::Document;
                next
            }
            Some(Meaning::View) => match self.views_at(segment, next) {
                Some((views, end)) => {
                    facts.views.get_or_insert(views);
                    end
                }
                None => next,
            },
            Some(Meaning::Number) => self.number(segment, next, reading, facts),
            Some(Meaning::OtherNumber) => {
                reading.other_number = true;
                next
            }
            Some(Meaning::Folio | Meaning::Page) => {
                if !segment
                    .tokens
                    .get(next)
                    .is_some_and(|token| is_folio_number(&token.raw))
                {
                    return next;
                }
                facts.folio.get_or_insert(segment.text(at, next + 1));
                reading.after_folio = true;
                next + 1
            }
            Some(Meaning::CallNumber) => {
                let mut end = next;
                while end < segment.tokens.len()
                    && !self.lexicon.starts(segment, end, |meaning| {
                        matches!(
                            meaning,
                            Meaning::View
                                | Meaning::Number
                                | Meaning::Folio
                                | Meaning::Page
                                | Meaning::CallNumber
                        )
                    })
                {
                    end += 1;
                }
                if end > next {
                    facts.call_number.get_or_insert(segment.text(next, end));
                }
                end
            }
            Some(Meaning::Parish) => {
                let start = self.skip_prepositions(segment, next);
                match self.place_run(segment, start) {
                    Some((text, end)) => {
                        facts.parish.get_or_insert(text);
                        end
                    }
                    None => next,
                }
            }
            Some(Meaning::Bureau) => {
                reading.context = Context::Document;
                let start = self.skip_prepositions(segment, next);
                match self.place_run(segment, start) {
                    Some((text, end)) => {
                        facts.localities.push((text, Placing::Introduced));
                        end
                    }
                    None => next,
                }
            }
            Some(Meaning::Place) if reading.context == Context::Document => {
                let start = self.skip_prepositions(segment, at);
                match self.place_run(segment, start) {
                    Some((text, end)) => {
                        facts.localities.push((text, Placing::Introduced));
                        end
                    }
                    None => next,
                }
            }
            _ => next,
        }
    }

    /// The archive a kind of archive names with what follows it — its area,
    /// its code — from token `at`; an area the catalogue does not list marks
    /// an uncatalogued archive.
    fn archive(
        &self,
        segment: &Segment,
        at: usize,
        level: Option<Level>,
        facts: &mut Facts,
    ) -> usize {
        let start = self.skip_prepositions(segment, at);
        let Some(token) = segment.tokens.get(start) else {
            return start;
        };
        if let Some(archive) = self.by_jurisdiction(&token.raw, level) {
            facts.archives.push(archive);
            return start + 1;
        }
        for (index, archive) in self.archives.iter().enumerate() {
            if level.is_some_and(|level| level != archive.level) {
                continue;
            }
            for area in archive.areas.iter().chain(&archive.aliases) {
                if let Some(taken) = matches_at(segment, start, &fold(area)) {
                    facts.archives.push(index);
                    return start + taken;
                }
            }
        }
        if let Some((_, end)) = self.place_run(segment, start) {
            facts.uncatalogued = true;
            return end;
        }
        if token.number().is_some() && level.is_some() {
            facts.uncatalogued = true;
            return start + 1;
        }
        at
    }

    /// An act or matricule number after its keyword: `acte n° 312`.
    fn number(
        &self,
        segment: &Segment,
        mut at: usize,
        reading: &mut Reading,
        facts: &mut Facts,
    ) -> usize {
        while let Some((taken, meanings)) = self.lexicon.at(segment, at)
            && meanings
                .iter()
                .any(|meaning| matches!(meaning, Meaning::Number))
        {
            at += taken;
        }
        let Some(number) = segment.tokens.get(at).and_then(Token::number) else {
            return at;
        };
        if !std::mem::take(&mut reading.other_number) {
            facts.number.get_or_insert(number);
        }
        at + 1
    }

    /// A locality read at `at..end`. An area alone names no archive: the
    /// archive is designated, never inferred.
    fn place(segment: &Segment, text: String, at: usize, end: usize, facts: &mut Facts) {
        let placing = if at == 0 && end == segment.tokens.len() {
            Placing::Standalone
        } else {
            Placing::Loose
        };
        facts.localities.push((text, placing));
    }

    /// The proper name starting at token `at`: capitalized words, with the
    /// particles between them (`Saint-Exemple-sur-Loire`, `La Roche des
    /// Exemples`), and no keyword.
    pub fn place_run(&self, segment: &Segment, at: usize) -> Option<(String, usize)> {
        let tokens = &segment.tokens;
        let mut end = None;
        let mut index = at;
        while index < tokens.len() {
            let token = &tokens[index];
            let keyword = self.lexicon.starts(segment, index, |meaning| {
                !matches!(
                    meaning,
                    Meaning::Place
                        | Meaning::Range
                        | Meaning::Month(_)
                        | Meaning::Recto
                        | Meaning::Verso
                )
            });
            if token.is_capitalized() && !keyword {
                index += 1;
                end = Some(index);
            } else if end.is_some()
                && self.lexicon.is_particle(token)
                && tokens
                    .get(index + 1)
                    .is_some_and(|next| next.is_capitalized() || self.lexicon.is_particle(next))
            {
                index += 1;
            } else {
                break;
            }
        }
        end.map(|end| (segment.text(at, end).trim_end_matches('.').to_owned(), end))
    }

    /// Past the prepositions and particles at `at`: `de la`, `d'`.
    fn skip_prepositions(&self, segment: &Segment, mut at: usize) -> usize {
        while let Some(token) = segment.tokens.get(at) {
            let preposition = self.lexicon.at(segment, at).filter(|(_, meanings)| {
                meanings
                    .iter()
                    .any(|meaning| matches!(meaning, Meaning::Place))
            });
            if let Some((taken, _)) = preposition {
                at += taken;
            } else if self.lexicon.is_particle(token) && !token.is_capitalized() {
                at += 1;
            } else {
                break;
            }
        }
        at
    }

    /// A catalogued citation code written over one to three tokens: `AD44`,
    /// `AD 44`, `A.D. 44`.
    fn code_at(&self, segment: &Segment, at: usize) -> Option<(usize, usize)> {
        let mut joined = String::new();
        for (offset, token) in segment.tokens.get(at..)?.iter().take(3).enumerate() {
            joined.extend(
                token
                    .raw
                    .chars()
                    .filter(char::is_ascii_alphanumeric)
                    .map(|c| c.to_ascii_uppercase()),
            );
            if !token
                .raw
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.')
            {
                return None;
            }
            if let Some(index) = self
                .archives
                .iter()
                .position(|archive| archive.citation_codes.contains(&joined))
            {
                let shaped = token.raw.chars().any(|c| c.is_ascii_digit()) || offset == 0;
                return shaped.then_some((index, offset + 1));
            }
        }
        None
    }

    /// A code shaped like a catalogued kind of archive followed by digits,
    /// such as `AD33`, that the catalogue does not list.
    fn is_uncatalogued_code(&self, token: &Token) -> bool {
        let letters: String = token
            .raw
            .chars()
            .take_while(char::is_ascii_uppercase)
            .collect();
        let rest = &token.raw[letters.len()..];
        let digits = rest.trim_end_matches(['A', 'B']);
        !letters.is_empty()
            && (1..=3).contains(&digits.len())
            && digits.bytes().all(|byte| byte.is_ascii_digit())
            && self.lexicon.vocabularies.iter().any(|vocabulary| {
                vocabulary.phrases().iter().any(|(phrase, meaning)| {
                    matches!(meaning, Meaning::Archive(Some(_)))
                        && phrase.len() == 1
                        && phrase[0] == letters.to_lowercase()
                })
            })
    }

    /// The archive whose alias or citation-free name starts at `at`.
    fn alias_at(&self, segment: &Segment, at: usize) -> Option<usize> {
        self.archives
            .iter()
            .enumerate()
            .find_map(|(index, archive)| {
                std::iter::once(&archive.name)
                    .chain(&archive.aliases)
                    .any(|name| matches_at(segment, at, &fold(name)).is_some())
                    .then_some(index)
            })
    }

    /// The archive of `level` (any level when `None`) serving the area of
    /// jurisdiction code `code`: `AD 72`.
    fn by_jurisdiction(&self, code: &str, level: Option<Level>) -> Option<usize> {
        self.archives.iter().position(|archive| {
            level.is_none_or(|level| level == archive.level)
                && archive
                    .jurisdiction
                    .iter()
                    .any(|known| known.eq_ignore_ascii_case(code))
        })
    }

    /// A full date's year: `12/03/1752`, `12.03.1752`, `12 mars 1752`,
    /// `1er mars 1752`.
    fn date_at(&self, segment: &Segment, at: usize) -> Option<(u16, usize)> {
        let token = &segment.tokens[at];
        let parts: Vec<&str> = token.raw.split(['/', '.']).collect();
        if parts.len() == 3
            && parts[..2].iter().all(|part| {
                (1..=2).contains(&part.len()) && part.bytes().all(|b| b.is_ascii_digit())
            })
            && let Some(year) = year(parts[2])
        {
            return Some((year, 1));
        }
        let day = token.raw.trim_end_matches(|c: char| c.is_alphabetic());
        let is_day = !day.is_empty()
            && day.len() <= 2
            && day.bytes().all(|b| b.is_ascii_digit())
            && day.parse::<u8>().is_ok_and(|day| (1..=31).contains(&day));
        if !is_day {
            return None;
        }
        let (taken, meanings) = self.lexicon.at(segment, at + 1)?;
        if !meanings
            .iter()
            .any(|meaning| matches!(meaning, Meaning::Month(_)))
        {
            return None;
        }
        let year = segment.tokens.get(at + 1 + taken)?.year()?;
        Some((year, taken + 2))
    }

    /// A period from token `at`: `1745-1760`, `1745 à 1760`, `an XII`,
    /// `an XI-XII`, or a year alone. Its text, first year, tokens taken, and
    /// whether it is a range.
    fn period_at(&self, segment: &Segment, at: usize) -> Option<Period> {
        let token = &segment.tokens[at];
        if let Some((first, last)) = token.raw.split_once(['-', '–'])
            && let (Some(start), Some(end)) = (year(first), year(last))
            && start <= end
        {
            return Some((token.raw.clone(), start, 1, true));
        }
        match token.year() {
            Some(start) => Some(self.joined_period(segment, at, start).unwrap_or((
                token.raw.clone(),
                start,
                1,
                false,
            ))),
            None => self.republican_at(segment, at),
        }
    }

    /// The range a year at `at` starts: `1745 - 1760`, `1745 à 1760`.
    fn joined_period(&self, segment: &Segment, at: usize, start: u16) -> Option<Period> {
        let next = segment.tokens.get(at + 1)?;
        let taken = if matches!(next.raw.as_str(), "-" | "–") {
            1
        } else {
            let (taken, meanings) = self.lexicon.at(segment, at + 1)?;
            meanings
                .iter()
                .any(|meaning| matches!(meaning, Meaning::Range))
                .then_some(taken)?
        };
        let end = segment.tokens.get(at + 1 + taken)?.year()?;
        (start <= end).then(|| (format!("{start}-{end}"), start, taken + 2, true))
    }

    /// A Republican year at `at`: `an XII`, `an XI-XII`.
    fn republican_at(&self, segment: &Segment, at: usize) -> Option<Period> {
        let (taken, meanings) = self.lexicon.at(segment, at)?;
        if !meanings
            .iter()
            .any(|meaning| matches!(meaning, Meaning::RepublicanYear))
        {
            return None;
        }
        let numeral = segment.tokens.get(at + taken)?;
        let (first, last) = match numeral.raw.split_once('-') {
            Some((first, last)) => (first, Some(last)),
            None => (numeral.raw.as_str(), None),
        };
        let start = republican_start(republican_numeral(first)?)?;
        if let Some(last) = last {
            republican_numeral(last.trim_start_matches("an "))?;
        }
        Some((
            segment.text(at, at + taken + 1),
            start,
            taken + 1,
            last.is_some(),
        ))
    }

    /// The views written from token `at`: `45`, `45/200`, `5d-6g/13`,
    /// `45 sur 200`. The views and where the reading stops.
    fn views_at(&self, segment: &Segment, at: usize) -> Option<(Views, usize)> {
        let token = segment.tokens.get(at)?;
        let (mut views, mut count) = self.view_spec(&token.raw)?;
        let mut end = at + 1;
        if count.is_none()
            && let Some((taken, meanings)) = self.lexicon.at(segment, end)
            && meanings
                .iter()
                .any(|meaning| matches!(meaning, Meaning::ViewCount))
            && let Some(total) = segment.tokens.get(end + taken).and_then(Token::number)
        {
            count = u16::try_from(total).ok();
            end += taken + 1;
        } else if count.is_none()
            && segment
                .tokens
                .get(end)
                .is_some_and(|token| token.raw == "/")
            && let Some(total) = segment.tokens.get(end + 1).and_then(Token::number)
        {
            count = u16::try_from(total).ok();
            end += 2;
        }
        if count.is_some_and(|count| views.iter().any(|view| view.view > count)) {
            views.clear();
            count = None;
        }
        (!views.is_empty()).then_some(((views, count), end))
    }

    /// A bare `45/200` or `45d/200`: views without their word.
    fn bare_views(&self, text: &str) -> Option<(Vec<CitedView>, Option<u16>)> {
        let (views, count) = self.view_spec(text)?;
        (count.is_some() && !views.is_empty()).then_some((views, count))
    }

    /// `45`, `45d`, `45/200`, `5d-6g/13`, the sides as the vocabularies
    /// write them.
    fn view_spec(&self, spec: &str) -> Option<(Vec<CitedView>, Option<u16>)> {
        let (range, count) = match spec.split_once('/') {
            Some((range, count)) => (range, Some(count.trim().parse::<u16>().ok()?)),
            None => (spec, None),
        };
        let views = match range.split_once(['-', '–']) {
            Some((first, last)) => {
                crate::citation::view_range(self.view(first)?, self.view(last)?)?
            }
            None => vec![self.view(range)?],
        };
        if count.is_some_and(|count| views.iter().any(|view| view.view > count)) {
            return None;
        }
        Some((views, count))
    }

    fn view(&self, token: &str) -> Option<CitedView> {
        let token = token.trim();
        let digits_end = token
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(token.len());
        let (number, suffix) = token.split_at(digits_end);
        let view = number.parse::<u16>().ok().filter(|view| *view > 0)?;
        let side = if suffix.is_empty() {
            None
        } else {
            Some(self.lexicon.side(&fold(suffix).join(" "))?)
        };
        Some(CitedView { view, side })
    }
}

/// Cited views and the register's view count.
type Views = (Vec<CitedView>, Option<u16>);

/// A period as written, its first year, the tokens it takes, and whether it
/// is a range.
type Period = (String, u16, usize, bool);

/// What the reading of one segment carries from token to token.
struct Reading {
    context: Context,
    /// The number about to be read is a household's or a house's.
    other_number: bool,
    /// A folio was just read, which a recto or verso may follow.
    after_folio: bool,
}

/// An act code written in capitals: `BMS`, `NMD`, `N`, `TD`. A series code
/// is not read so — `RP` is the parish registers in free text — but in
/// words or in the normalized form.
fn act_code(raw: &str) -> Option<Act> {
    if raw.is_empty() || !raw.bytes().all(|byte| byte.is_ascii_uppercase()) {
        return None;
    }
    Act::from_code(raw).filter(|act| !matches!(act, Act::Series(_)))
}

/// A folio or page number: `23`, `23v`, `23r°`.
fn is_folio_number(raw: &str) -> bool {
    raw.starts_with(|c: char| c.is_ascii_digit()) && raw.len() <= 8
}

/// How many tokens the archive's name or alias starting at `at` takes.
fn tokens_of_alias(segment: &Segment, at: usize, archive: &Archive) -> usize {
    std::iter::once(&archive.name)
        .chain(&archive.aliases)
        .find_map(|name| matches_at(segment, at, &fold(name)))
        .unwrap_or(1)
}
