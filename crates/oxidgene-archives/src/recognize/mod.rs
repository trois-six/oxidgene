//! Recognizing an archive citation in the records a genealogist keeps
//! (Archive Portals §5.1).
//!
//! Genealogists cite registers in many ways: the normalized form of
//! [`CitationParts::parse`], a comma-separated description from the archive
//! down to the act (`AD72, état civil de Exampleville, naissances 1872, vue
//! 45`), a bare call number and view, or a register named in the source
//! title with the archive in a repository record, its call number on the
//! repository link, the act and view in the citation's page, and the act's
//! kind, year and place on the cited event. [`ArchiveRegistry::recognize`]
//! gathers every one of these signals, [`CitationEvidence`], and reads them
//! in order of reliability:
//!
//! 1. what the reader supplied in the "Find in the archives" dialog;
//! 2. a portal address of a catalogued archive, opened as it is;
//! 3. the normalized form;
//! 4. the records' dedicated fields: repository names, websites and call
//!    numbers;
//! 5. the words of the title, the page and the source's other fields, read
//!    with the vocabularies of [`crate::vocabulary`];
//! 6. the cited event: its kind, year, place and agency.
//!
//! Each part keeps the [`Signal`] that gave it. The words are data per
//! language, and a country's conventions data per country: nothing here
//! knows a language.

mod district;
mod lex;
mod scan;

use std::collections::BTreeMap;

use oxidgene_core::enums::EventType;
use oxidgene_core::types::{Citation, Event, Source};
use serde::{Deserialize, Serialize};

use crate::catalog::{Archive, Level};
use crate::citation::{Act, ActKind, CallNumber, CitationParts, CitedView, Series};
use crate::transport::origin_of;
use crate::vocabulary::{Family, Vocabulary, fold};
use crate::{ArchiveRegistry, cited_text};

use district::Districts;
use lex::{Lexicon, segments};
use scan::{Facts, Placing, Scanner};

/// A repository holding the cited source, under one call number.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldAt {
    pub name: String,
    pub call_number: Option<String>,
    pub website: Option<String>,
}

/// The event a citation is attached to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CitedEvent {
    pub event_type: EventType,
    pub year: Option<u16>,
    /// The place's name as the tree writes it: `Exampleville, Sarthe, France`.
    pub place: Option<String>,
    /// The authority that recorded it: a parish, a registry office.
    pub agency: Option<String>,
}

/// Everything a citation of a source says about where its register is.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CitationEvidence {
    pub title: String,
    pub abbreviation: Option<String>,
    pub author: Option<String>,
    pub publisher: Option<String>,
    pub agency: Option<String>,
    /// Where in the source: the citation's page.
    pub page: Option<String>,
    /// The citation's extracted text, a transcription: read for addresses
    /// only, since its names are the act's people.
    pub text: Option<String>,
    pub repositories: Vec<HeldAt>,
    pub event: Option<CitedEvent>,
    /// Addresses of the source's media held as links.
    pub urls: Vec<String>,
}

impl CitationEvidence {
    /// The evidence of `source`, as cited by `citation` when given.
    pub fn new(source: &Source, citation: Option<&Citation>) -> Self {
        Self {
            title: source.title.clone(),
            abbreviation: source.abbreviation.clone(),
            author: source.author.clone(),
            publisher: source.publisher.clone(),
            agency: source.agency.clone(),
            page: citation.and_then(|citation| citation.page.clone()),
            text: citation.and_then(|citation| citation.text.clone()),
            ..Self::default()
        }
    }

    /// With the event the citation is attached to, at `place`.
    #[must_use]
    pub fn with_event(mut self, event: &Event, place: Option<&str>) -> Self {
        self.event = Some(CitedEvent {
            event_type: event.event_type,
            year: event.year().and_then(|year| u16::try_from(year).ok()),
            place: place.map(str::to_owned),
            agency: event.agency.clone(),
        });
        self
    }

    /// With the repositories holding the source.
    #[must_use]
    pub fn with_repositories(mut self, repositories: Vec<HeldAt>) -> Self {
        self.repositories = repositories;
        self
    }

    /// With the addresses of the source's linked media.
    #[must_use]
    pub fn with_urls(mut self, urls: Vec<String>) -> Self {
        self.urls = urls;
        self
    }
}

/// Where a recognized part comes from, the most reliable first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Signal {
    /// The reader, in the "Find in the archives" dialog.
    Reader,
    /// A portal address of the archive.
    Address,
    /// The normalized form.
    Normalized,
    /// A repository record or a call number of a repository link.
    Record,
    /// The words of the source and the citation.
    Words,
    /// The cited event.
    Event,
}

/// A part of a citation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Part {
    Archive,
    Locality,
    Parish,
    Act,
    Year,
    CallNumber,
    Number,
    Views,
    Folio,
}

/// What was recognized, every part optional.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Found {
    pub locality: Option<String>,
    pub parish: Option<String>,
    pub act: Option<Act>,
    pub year: Option<u16>,
    pub period: Option<String>,
    pub call_number: Option<CallNumber>,
    pub number: Option<u32>,
    pub views: Vec<CitedView>,
    pub view_count: Option<u16>,
    /// A folio or page, kept for the reader: it is not a view.
    pub folio: Option<String>,
}

/// What a reader completes in the "Find in the archives" dialog.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SuppliedParts {
    #[serde(default)]
    pub locality: Option<String>,
    #[serde(default)]
    pub act: Option<Act>,
    #[serde(default)]
    pub year: Option<u16>,
    #[serde(default)]
    pub view: Option<u16>,
}

/// The longest locality a reader may supply.
const MAX_LOCALITY_CHARS: usize = 200;

impl SuppliedParts {
    /// Refuses parts `archive` cannot search: a blank or overlong locality,
    /// an act none of its collections holds, a year outside 1000–2100, a
    /// view below 1.
    pub fn validate(&self, archive: &Archive) -> Result<(), String> {
        if let Some(locality) = &self.locality
            && (locality.trim().is_empty() || locality.chars().count() > MAX_LOCALITY_CHARS)
        {
            return Err(format!(
                "the locality must hold 1 to {MAX_LOCALITY_CHARS} characters"
            ));
        }
        if let Some(act) = &self.act
            && !archive.holds(act)
        {
            return Err(format!("the archive holds no `{act}` register"));
        }
        if self.year.is_some_and(|year| !(1000..=2100).contains(&year)) {
            return Err("the year must be between 1000 and 2100".to_owned());
        }
        if self.view == Some(0) {
            return Err("the view must be at least 1".to_owned());
        }
        Ok(())
    }
}

/// Why no archive register is recognized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unrecognized {
    /// Nothing names an archive.
    NotACitation,
    /// An archive is named that the catalogue does not list, or the cited
    /// document is one its collections do not hold.
    NoAdapter,
}

/// The place dictionary, as recognition consults it to tell a locality from
/// a parish or a hamlet (Archive Portals §5.1).
pub trait PlaceLookup {
    /// For each of `names`, the subdivisions and regions of every place so
    /// named; empty for a name that is no known place.
    fn areas(&self, names: &[String]) -> Vec<Vec<String>>;
}

/// A citation recognized as a register of a catalogued archive.
#[derive(Debug, Clone, PartialEq)]
pub struct Recognition<'r> {
    pub archive: &'r Archive,
    pub found: Found,
    /// Where each part came from.
    pub signals: BTreeMap<Part, Signal>,
    /// A portal address of the archive found in the records: the target,
    /// opened as it is.
    pub address: Option<String>,
    /// Whether the place dictionary confirmed the locality within the
    /// archive's area.
    pub confirmed_locality: bool,
}

impl Recognition<'_> {
    /// The parts to resolve, when the act is known and so is the locality —
    /// which a series may lack.
    pub fn citation(&self) -> Option<CitationParts> {
        let series = matches!(self.found.act, Some(Act::Series(_)));
        (self.found.locality.is_some() || series)
            .then(|| self.search())
            .flatten()
    }

    /// The parts known so far, the locality empty when none is: what the
    /// archive's filtered search page is built from while the reader has
    /// not completed the citation. `None` without a document kind.
    pub fn search(&self) -> Option<CitationParts> {
        Some(CitationParts {
            code: self.archive.citation_codes[0].clone(),
            locality: self.found.locality.clone().unwrap_or_default(),
            parish: self.found.parish.clone(),
            act: self.found.act.clone()?,
            year: self.found.year,
            period: self.found.period.clone(),
            call_number: self.found.call_number.clone(),
            number: self.found.number,
            views: self.found.views.clone(),
            view_count: self.found.view_count,
        })
    }

    /// The parts the reader must complete before a register can be looked
    /// up: the document kind, and the locality unless it is a series.
    pub fn missing(&self) -> Vec<Part> {
        let mut missing = Vec::new();
        if self.found.act.is_none() {
            missing.push(Part::Act);
        }
        if self.found.locality.is_none() && !matches!(self.found.act, Some(Act::Series(_))) {
            missing.push(Part::Locality);
        }
        missing
    }

    pub fn signal(&self, part: Part) -> Option<Signal> {
        self.signals.get(&part).copied()
    }

    /// The parts the reader supplied, written in the archive's language to
    /// be added to the citation's page — `Exampleville, naissance 1872, vue
    /// 45` — so that the citation reads them next time. `None` when the
    /// reader supplied nothing new or no vocabulary serves the archive's
    /// country.
    pub fn written(&self, registry: &ArchiveRegistry) -> Option<String> {
        let vocabulary = registry
            .vocabularies
            .iter()
            .find(|vocabulary| vocabulary.serves(&self.archive.country))?;
        let supplied = |part| self.signal(part) == Some(Signal::Reader);
        let mut pieces = Vec::new();
        if supplied(Part::Locality)
            && let Some(locality) = &self.found.locality
        {
            pieces.push(locality.clone());
        }
        let act = supplied(Part::Act)
            .then_some(self.found.act.as_ref())
            .flatten()
            .and_then(|act| {
                let key = match act {
                    Act::Register(kinds) => kinds.first()?.letter().to_string(),
                    other => other.to_string(),
                };
                vocabulary.written(&key).map(str::to_owned)
            });
        let year = supplied(Part::Year).then_some(self.found.year).flatten();
        match (act, year) {
            (Some(act), Some(year)) => pieces.push(format!("{act} {year}")),
            (Some(act), None) => pieces.push(act),
            (None, Some(year)) => pieces.push(year.to_string()),
            (None, None) => {}
        }
        if supplied(Part::Views)
            && let (Some(view), Some(word)) = (self.found.views.first(), vocabulary.written("view"))
        {
            pieces.push(format!("{word} {}", view.view));
        }
        (!pieces.is_empty()).then(|| pieces.join(", "))
    }
}

/// One part's value with its signal: the first offered of the most
/// reliable signal.
struct Slot<T>(Option<(T, Signal)>);

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Self(None)
    }
}

impl<T> Slot<T> {
    fn offer(&mut self, value: Option<T>, signal: Signal) {
        if let Some(value) = value {
            match &self.0 {
                Some((_, held)) if *held <= signal => {}
                _ => self.0 = Some((value, signal)),
            }
        }
    }

    fn take(self, part: Part, signals: &mut BTreeMap<Part, Signal>) -> Option<T> {
        let (value, signal) = self.0?;
        signals.insert(part, signal);
        Some(value)
    }
}

/// What the texts of the evidence say, each read once.
struct Reading {
    title: Facts,
    page: Facts,
    /// The abbreviation, author, publisher and agency of the source.
    others: Vec<Facts>,
    /// Each repository's name, in order.
    repositories: Vec<Facts>,
    place: Facts,
    agency: Facts,
    addresses: Vec<String>,
}

impl Reading {
    /// The texts naming the register, the title first.
    fn register_texts(&self) -> impl Iterator<Item = &Facts> {
        std::iter::once(&self.title)
            .chain(std::iter::once(&self.page))
            .chain(&self.others)
    }

    /// The texts naming the place within the register, the page first.
    fn location_texts(&self) -> impl Iterator<Item = &Facts> {
        std::iter::once(&self.page)
            .chain(std::iter::once(&self.title))
            .chain(&self.others)
    }

    fn is_documentary(&self) -> bool {
        self.register_texts().any(Facts::is_documentary)
    }

    fn families(&self) -> Vec<Family> {
        self.register_texts()
            .flat_map(|facts| facts.families.iter().copied())
            .collect()
    }
}

impl ArchiveRegistry {
    /// Recognizes the register a citation names, with the parts the reader
    /// `supplied` when given — validated by the caller — and the place
    /// dictionary when `places` is given.
    ///
    /// A citation is recognized once an archive with an adapter is
    /// identified and something says it cites a register of it; the result
    /// may still miss the act or the locality, which the reader completes
    /// ([`Recognition::missing`]).
    pub fn recognize(
        &self,
        evidence: &CitationEvidence,
        supplied: Option<&SuppliedParts>,
        places: Option<&dyn PlaceLookup>,
    ) -> Result<Recognition<'_>, Unrecognized> {
        let scanner = Scanner {
            lexicon: Lexicon {
                vocabularies: &self.vocabularies,
            },
            archives: &self.archives,
            districts: Districts::new(
                self.archives
                    .iter()
                    .flat_map(|archive| &archive.citation.districts),
                &self.vocabularies,
            ),
        };
        let reading = self.read(&scanner, evidence);
        let normalized = self.normalized(evidence);
        let (index, archive_signal) = self.choose(evidence, &reading, normalized.as_ref())?;
        let weighing = Weighing {
            registry: self,
            evidence,
            supplied,
            reading: &reading,
            normalized: match normalized {
                Some(Ok((at, parts))) if at == index => Some(parts),
                _ => None,
            },
            archive: &self.archives[index],
            index,
            archive_signal,
        };
        let mut signals = BTreeMap::from([(Part::Archive, archive_signal)]);
        let year = weighing.year();
        let act = weighing.act(year.0.as_ref().map(|((_, at), _)| *at))?;
        let (locality, parish, confirmed_locality) = weighing.place(places);
        let year = year.take(Part::Year, &mut signals);
        let (views, view_count) = weighing
            .views()
            .take(Part::Views, &mut signals)
            .unwrap_or_default();
        let archive = &self.archives[index];
        let mut found = Found {
            locality: locality
                .take(Part::Locality, &mut signals)
                .map(|text| archive.citation.written_locality(text, &self.vocabularies)),
            parish: parish.take(Part::Parish, &mut signals),
            act: act.take(Part::Act, &mut signals),
            year: year.as_ref().map(|(_, year)| *year),
            period: year.and_then(|(period, _)| period),
            call_number: weighing.call_number().take(Part::CallNumber, &mut signals),
            number: weighing.number().take(Part::Number, &mut signals),
            views,
            view_count,
            folio: weighing.folio().take(Part::Folio, &mut signals),
        };
        // A register's period its call number writes, for a citation
        // giving no year.
        if found.year.is_none()
            && let Some((period, year)) = found
                .call_number
                .as_ref()
                .and_then(|call_number| archive.citation.call_number_period(call_number.as_str()))
        {
            found.year = Some(year);
            found.period = Some(period);
            if let Some(signal) = signals.get(&Part::CallNumber).copied() {
                signals.insert(Part::Year, signal);
            }
        }
        Ok(Recognition {
            archive: &self.archives[index],
            found,
            signals,
            address: weighing.address(),
            confirmed_locality,
        })
    }

    /// The archive the records designate: a portal address of it, the
    /// normalized form's code, a repository holding the source, or the
    /// words of the source and the citation — its code, name, alias, or a
    /// kind of archive with its area or code. A designation wins absolutely:
    /// one of an archive the catalogue does not list leaves the citation
    /// without a link rather than another archive standing in, and the
    /// cited event, which happened where the record was not necessarily
    /// written, never designates one. A repository alone must hold a
    /// register citation: its words, or an event its registers record.
    fn choose(
        &self,
        evidence: &CitationEvidence,
        reading: &Reading,
        normalized: Option<&Result<(usize, CitationParts), ()>>,
    ) -> Result<(usize, Signal), Unrecognized> {
        if let Some(archive) = reading
            .addresses
            .iter()
            .find_map(|address| self.archive_of_address(address))
        {
            return Ok((archive, Signal::Address));
        }
        let uncatalogued = matches!(normalized, Some(Err(())))
            || reading.register_texts().any(|facts| facts.uncatalogued)
            || reading.repositories.iter().any(|facts| facts.uncatalogued);
        if uncatalogued {
            return Err(Unrecognized::NoAdapter);
        }
        if let Some(Ok((archive, _))) = normalized {
            return Ok((*archive, Signal::Normalized));
        }
        let documentary = |archive: &usize| {
            reading.is_documentary()
                || evidence.event.as_ref().is_some_and(|event| {
                    self.act_of_event(&self.archives[*archive], event, &reading.families(), None)
                        .is_some()
                })
        };
        let by_record = evidence
            .repositories
            .iter()
            .zip(&reading.repositories)
            .find_map(|(held, facts)| {
                held.website
                    .as_deref()
                    .and_then(|website| self.archive_of_address(website))
                    .or_else(|| facts.archives.first().copied())
            })
            .filter(documentary)
            .map(|archive| (archive, Signal::Record));
        let by_words = reading
            .register_texts()
            .find_map(|facts| facts.archives.first().copied())
            .map(|archive| (archive, Signal::Words));
        by_record.or(by_words).ok_or(Unrecognized::NotACitation)
    }

    /// Reads every text of the evidence once.
    fn read(&self, scanner: &Scanner<'_>, evidence: &CitationEvidence) -> Reading {
        let mut addresses = evidence.urls.clone();
        let mut read = |text: Option<&str>| -> Facts {
            let Some(text) = text else {
                return Facts::default();
            };
            let (found, rest) = lex::addresses(text);
            addresses.extend(found);
            scanner.read(&segments(&rest))
        };
        let title = read(Some(&evidence.title));
        let page = read(evidence.page.as_deref());
        let others = [
            &evidence.abbreviation,
            &evidence.author,
            &evidence.publisher,
            &evidence.agency,
        ]
        .into_iter()
        .map(|text| read(text.as_deref()))
        .collect();
        if let Some(text) = &evidence.text {
            addresses.extend(lex::addresses(text).0);
        }
        let repositories = evidence
            .repositories
            .iter()
            .map(|held| scanner.read(&segments(&held.name)))
            .collect();
        let event = evidence.event.as_ref();
        let place = event
            .and_then(|event| event.place.as_deref())
            .map(|place| scanner.read(&segments(place)))
            .unwrap_or_default();
        let agency = event
            .and_then(|event| event.agency.as_deref())
            .map(|agency| scanner.read(&segments(agency)))
            .unwrap_or_default();
        Reading {
            title,
            page,
            others,
            repositories,
            place,
            agency,
            addresses,
        }
    }

    /// The normalized form of the title completed by the page, with the
    /// archive its code names; `Err` for a code the catalogue does not list.
    fn normalized(
        &self,
        evidence: &CitationEvidence,
    ) -> Option<Result<(usize, CitationParts), ()>> {
        let text = cited_text(&evidence.title, evidence.page.as_deref());
        let parts = self.parse(&text)?;
        Some(
            self.archives
                .iter()
                .position(|archive| archive.citation_codes.contains(&parts.code))
                .map(|index| (index, parts))
                .ok_or(()),
        )
    }

    /// The archive one of whose origins serves `address`: its website's or
    /// a collection portal's.
    fn archive_of_address(&self, address: &str) -> Option<usize> {
        let origin = origin_of(address)?.to_ascii_lowercase();
        self.archives.iter().position(|archive| {
            origin_of(&archive.website).is_some_and(|website| website.eq_ignore_ascii_case(&origin))
                || archive.collections.iter().any(|collection| {
                    self.platform(&collection.platform)
                        .and_then(|platform| platform.endpoint(collection))
                        .is_some_and(|endpoint| {
                            endpoint
                                .origins()
                                .any(|known| known.eq_ignore_ascii_case(&origin))
                        })
                })
        })
    }

    /// The document kind the cited event's kind gives, as the archive's
    /// country records it: a birth is a baptism in parish registers or
    /// before civil registration began, a death a burial.
    fn act_of_event(
        &self,
        archive: &Archive,
        event: &CitedEvent,
        families: &[Family],
        year: Option<u16>,
    ) -> Option<Act> {
        let civil_from = self
            .vocabularies
            .iter()
            .filter(|vocabulary| vocabulary.serves(&archive.country))
            .find_map(Vocabulary::civil_registration_from);
        let parish = families.contains(&Family::Parish) && !families.contains(&Family::Civil)
            || !families.contains(&Family::Civil)
                && civil_from
                    .zip(year.or(event.year))
                    .is_some_and(|(from, year)| year < from);
        let kind = match event.event_type {
            EventType::Birth if parish => ActKind::Baptism,
            EventType::Birth => ActKind::Birth,
            EventType::Baptism | EventType::Christening | EventType::AdultChristening => {
                ActKind::Baptism
            }
            EventType::Death if parish => ActKind::Burial,
            EventType::Death => ActKind::Death,
            EventType::Burial if families.contains(&Family::Civil) => ActKind::Death,
            EventType::Burial => ActKind::Burial,
            EventType::Marriage => ActKind::Marriage,
            EventType::MarriageBann => ActKind::Publication,
            EventType::Census => return Some(Act::Series(Series::Census)),
            EventType::MilitaryService => return Some(Act::Series(Series::MilitaryRegister)),
            _ => return None,
        };
        Some(Act::Register(vec![kind]))
    }

    /// The localities the evidence offers, the most reliable first: those
    /// the words introduce, then those written apart; only when the words
    /// state none, the event's place — its first part, or every part when
    /// the place dictionary will tell them apart —; then the other proper
    /// names of the words. None of them is the archive's own area or name.
    fn locality_candidates(
        &self,
        reading: &Reading,
        archive: &Archive,
        every_place_part: bool,
    ) -> Vec<(String, Signal)> {
        let own: Vec<Vec<String>> = archive
            .areas
            .iter()
            .chain(&archive.aliases)
            .chain(std::iter::once(&archive.name))
            .map(|name| fold(name))
            .collect();
        let mut candidates: Vec<(String, Signal)> = Vec::new();
        let push = |candidates: &mut Vec<(String, Signal)>, text: &str, signal: Signal| {
            let folded = fold(text);
            if !folded.is_empty()
                && !own.contains(&folded)
                && !candidates.iter().any(|(known, _)| fold(known) == folded)
            {
                candidates.push((text.to_owned(), signal));
            }
        };
        let words = |candidates: &mut Vec<(String, Signal)>, placings: &[Placing]| {
            for placing in placings {
                for facts in reading.register_texts() {
                    for (text, _) in facts.localities.iter().filter(|(_, at)| at == placing) {
                        push(candidates, text, Signal::Words);
                    }
                }
            }
        };
        words(&mut candidates, &[Placing::Introduced, Placing::Standalone]);
        if candidates.is_empty() {
            let parts = reading
                .place
                .localities
                .iter()
                .filter(|(_, placing)| *placing != Placing::Loose);
            for (text, _) in parts.take(if every_place_part { usize::MAX } else { 1 }) {
                push(&mut candidates, text, Signal::Event);
            }
        }
        words(&mut candidates, &[Placing::Loose]);
        candidates
    }

    /// The call number of a repository link, from a repository naming the
    /// archive, or from the only repository.
    fn record_call_number(
        &self,
        evidence: &CitationEvidence,
        reading: &Reading,
        archive: usize,
    ) -> Option<String> {
        let only = evidence.repositories.len() == 1;
        evidence
            .repositories
            .iter()
            .zip(&reading.repositories)
            .filter(|(held, facts)| {
                only || facts.archives.contains(&archive)
                    || held
                        .website
                        .as_deref()
                        .and_then(|website| self.archive_of_address(website))
                        == Some(archive)
            })
            .find_map(|(held, _)| {
                held.call_number
                    .as_deref()
                    .map(str::trim)
                    .filter(|call| !call.is_empty())
                    .map(str::to_owned)
            })
    }
}

/// Weighing each part's signals once the archive is chosen.
struct Weighing<'a> {
    registry: &'a ArchiveRegistry,
    evidence: &'a CitationEvidence,
    supplied: Option<&'a SuppliedParts>,
    reading: &'a Reading,
    /// The normalized form, when it names the chosen archive.
    normalized: Option<CitationParts>,
    archive: &'a Archive,
    index: usize,
    archive_signal: Signal,
}

impl Weighing<'_> {
    /// The portal address of the chosen archive, beyond its home page.
    fn address(&self) -> Option<String> {
        self.reading
            .addresses
            .iter()
            .find(|address| self.registry.archive_of_address(address) == Some(self.index))
            .filter(|address| is_specific(address, self.archive))
            .cloned()
    }

    /// The year and the period as written.
    fn year(&self) -> Slot<(Option<String>, u16)> {
        let mut year = Slot::default();
        year.offer(
            self.supplied
                .and_then(|parts| parts.year)
                .map(|at| (Some(at.to_string()), at)),
            Signal::Reader,
        );
        year.offer(
            self.normalized
                .as_ref()
                .and_then(|parts| parts.year.map(|at| (parts.period.clone(), at))),
            Signal::Normalized,
        );
        year.offer(
            self.reading.register_texts().find_map(Facts::year),
            Signal::Words,
        );
        year.offer(
            self.evidence
                .event
                .as_ref()
                .and_then(|event| event.year)
                .map(|at| (None, at)),
            Signal::Event,
        );
        year
    }

    /// The document kind: one the reader, the normalized form or the words
    /// name must be held by the archive, one the event gives is dropped
    /// otherwise.
    fn act(&self, cited_year: Option<u16>) -> Result<Slot<Act>, Unrecognized> {
        let mut act = Slot::default();
        act.offer(
            self.supplied.and_then(|parts| parts.act.clone()),
            Signal::Reader,
        );
        act.offer(
            self.normalized.as_ref().map(|parts| parts.act.clone()),
            Signal::Normalized,
        );
        // The first kind the words name that the archive holds, or else the
        // first they name: `inhumation` beside `cimetière` is a cemetery's
        // register where no parish register is catalogued.
        let named: Vec<Act> = self
            .reading
            .register_texts()
            .flat_map(Facts::acts)
            .collect();
        act.offer(
            named
                .iter()
                .find(|act| self.archive.holds(act))
                .or(named.first())
                .cloned(),
            Signal::Words,
        );
        // A kind of document no collection holds, named in the words, is
        // no link — whatever else the words or the event say; only the
        // reader or the normalized form, which choose their kind, may
        // override it.
        if self.reading.register_texts().any(|facts| facts.unsupported)
            && !matches!(act.0, Some((_, Signal::Reader | Signal::Normalized)))
        {
            return Err(Unrecognized::NoAdapter);
        }
        if let Some((written, signal)) = &act.0
            && *signal != Signal::Reader
            && !self.archive.holds(written)
        {
            return Err(Unrecognized::NoAdapter);
        }
        // A combined register the records cite keeps its kinds; the event
        // only narrows it to the one it is among them: a death cited from
        // an `NMD` register is a death, never the register's first kind.
        if let Some((Act::Register(kinds), signal)) = &mut act.0
            && *signal != Signal::Reader
            && kinds.len() > 1
            && let Some(event) = &self.evidence.event
            && let Some(kind) = event_kinds(event.event_type)
                .iter()
                .find(|kind| kinds.contains(kind))
        {
            *kinds = vec![*kind];
        }
        let from_event = self.evidence.event.as_ref().and_then(|event| {
            self.registry
                .act_of_event(self.archive, event, &self.reading.families(), cited_year)
        });
        act.offer(
            from_event.filter(|act| self.archive.holds(act)),
            Signal::Event,
        );
        Ok(act)
    }

    /// The locality, the parish, and whether the place dictionary confirmed
    /// the locality.
    fn place(&self, places: Option<&dyn PlaceLookup>) -> (Slot<String>, Slot<String>, bool) {
        let mut parish = Slot::default();
        parish.offer(
            self.normalized
                .as_ref()
                .and_then(|parts| parts.parish.clone()),
            Signal::Normalized,
        );
        parish.offer(
            self.reading
                .register_texts()
                .find_map(|facts| facts.parish.clone()),
            Signal::Words,
        );
        parish.offer(self.reading.agency.parish.clone(), Signal::Event);
        let mut locality = Slot::default();
        locality.offer(
            self.supplied
                .and_then(|parts| parts.locality.as_ref().map(|text| text.trim().to_owned())),
            Signal::Reader,
        );
        locality.offer(
            self.normalized
                .as_ref()
                .map(|parts| parts.locality.clone())
                .filter(|text| !text.is_empty()),
            Signal::Normalized,
        );
        if locality.0.is_some() {
            return (locality, parish, false);
        }
        let candidates =
            self.registry
                .locality_candidates(self.reading, self.archive, places.is_some());
        if candidates.is_empty() && self.archive.level == Level::Municipal {
            // A municipal archive serves its commune.
            locality.offer(self.archive.areas.first().cloned(), self.archive_signal);
        }
        let confirmed = places
            .filter(|_| candidates.len() > 1 && !self.archive.areas.is_empty())
            .and_then(|places| confirmed(places, &candidates, self.archive));
        let Some(at) = confirmed else {
            let first = candidates.into_iter().next();
            locality.offer(
                first.as_ref().map(|(text, _)| text.clone()),
                first.map_or(Signal::Words, |(_, signal)| signal),
            );
            return (locality, parish, false);
        };
        let (text, signal) = candidates[at].clone();
        // A name written before it that is no locality of the archive's
        // area is the parish: `BMS de Saint-Exemple`.
        if let Some((before, _)) = candidates[..at]
            .iter()
            .find(|(_, signal)| *signal == Signal::Words)
        {
            parish.offer(Some(before.clone()), Signal::Words);
        }
        locality.offer(Some(text), signal);
        (locality, parish, true)
    }

    fn call_number(&self) -> Slot<CallNumber> {
        let mut call_number = Slot::default();
        call_number.offer(
            self.normalized
                .as_ref()
                .and_then(|parts| parts.call_number.clone()),
            Signal::Normalized,
        );
        call_number.offer(
            self.registry
                .record_call_number(self.evidence, self.reading, self.index)
                .map(CallNumber::new),
            Signal::Record,
        );
        let texts = || self.reading.register_texts();
        call_number.offer(
            texts()
                .find_map(|facts| facts.call_number.clone())
                .or_else(|| texts().find_map(|facts| facts.shaped_call_number.clone()))
                .map(CallNumber::new),
            Signal::Words,
        );
        call_number
    }

    fn number(&self) -> Slot<u32> {
        let mut number = Slot::default();
        number.offer(
            self.normalized.as_ref().and_then(|parts| parts.number),
            Signal::Normalized,
        );
        number.offer(
            self.reading.location_texts().find_map(|facts| facts.number),
            Signal::Words,
        );
        number
    }

    /// The cited views; a view the reader names replaces them, with the
    /// side the citation gives it and the count it does not exceed.
    fn views(&self) -> Slot<(Vec<CitedView>, Option<u16>)> {
        let mut views = Slot::default();
        views.offer(
            self.normalized
                .as_ref()
                .filter(|parts| !parts.views.is_empty())
                .map(|parts| (parts.views.clone(), parts.view_count)),
            Signal::Normalized,
        );
        views.offer(
            self.reading
                .location_texts()
                .find_map(|facts| facts.views.clone()),
            Signal::Words,
        );
        if let Some(view) = self.supplied.and_then(|parts| parts.view) {
            let (cited, count) = views
                .0
                .as_ref()
                .map(|(found, _)| found.clone())
                .unwrap_or_default();
            let side = cited
                .iter()
                .find(|cited| cited.view == view)
                .and_then(|cited| cited.side);
            let count = count.filter(|count| view <= *count);
            views.0 = Some(((vec![CitedView { view, side }], count), Signal::Reader));
        }
        views
    }

    fn folio(&self) -> Slot<String> {
        let mut folio = Slot::default();
        folio.offer(
            self.reading
                .location_texts()
                .find_map(|facts| facts.folio.clone()),
            Signal::Words,
        );
        folio
    }
}

/// The act kinds an event of `event_type` may be recorded as, the likeliest
/// first.
fn event_kinds(event_type: EventType) -> &'static [ActKind] {
    match event_type {
        EventType::Birth => &[ActKind::Birth, ActKind::Baptism],
        EventType::Baptism | EventType::Christening | EventType::AdultChristening => {
            &[ActKind::Baptism]
        }
        EventType::Death => &[ActKind::Death, ActKind::Burial],
        EventType::Burial => &[ActKind::Burial, ActKind::Death],
        EventType::Marriage => &[ActKind::Marriage],
        EventType::MarriageBann => &[ActKind::Publication, ActKind::Marriage],
        _ => &[],
    }
}

/// The first candidate the place dictionary knows within the archive's
/// area.
fn confirmed(
    places: &dyn PlaceLookup,
    candidates: &[(String, Signal)],
    archive: &Archive,
) -> Option<usize> {
    let names: Vec<String> = candidates.iter().map(|(text, _)| text.clone()).collect();
    let areas: Vec<Vec<String>> = archive.areas.iter().map(|area| fold(area)).collect();
    places
        .areas(&names)
        .iter()
        .position(|found| found.iter().any(|area| areas.contains(&fold(area))))
}

/// Whether an address names something within the portal rather than its
/// home page.
fn is_specific(address: &str, archive: &Archive) -> bool {
    let Some(origin) = origin_of(address) else {
        return false;
    };
    let path = address[origin.len()..].trim_matches('/');
    !path.is_empty() && address.trim_end_matches('/') != archive.website.trim_end_matches('/')
}

#[cfg(test)]
mod tests;
