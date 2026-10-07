//! The recognizer over a corpus of fictitious citations written the ways
//! French genealogists write them: the classic description from the archive
//! down to the act, its short form, the normalized form, structured records
//! with the cited event, and portal addresses. Every locality, person and
//! call number is invented; the archives of the embedded catalogue are
//! public services, named as they are.

use std::cell::Cell;

use oxidgene_core::enums::EventType;

use super::*;
use crate::citation::{CitationGrammar, Side};
use crate::tests::Scripted;

/// A fictitious department with every kind of register, and a municipal
/// archive within it.
fn registry() -> ArchiveRegistry {
    let department = serde_json::json!({
        "id": "fr-ad99",
        "country": "FR",
        "level": "departmental",
        "name": "Archives départementales de l'Exampleshire",
        "jurisdiction": ["99"],
        "areas": ["Exampleshire", "Basse-Exampleshire"],
        "aliases": ["ADEX"],
        "citation_codes": ["AD99"],
        "website": "https://archives.example.org",
        "collections": [{
            "id": "parish-registers",
            "acts": ["B", "M", "S"],
            "period": [null, 1792],
            "platform": "scripted",
            "portal": { "outcome": "view" },
        }, {
            "id": "civil-status",
            "acts": ["N", "M", "D", "TD"],
            "period": [1793, null],
            "platform": "scripted",
            "portal": { "outcome": "view" },
        }, {
            "id": "series",
            "acts": ["RP", "RM", "CM", "TSA"],
            "platform": "scripted",
            "portal": { "outcome": "view" },
        }]
    })
    .to_string();
    let municipal = serde_json::json!({
        "id": "fr-am-exampleton",
        "country": "FR",
        "level": "municipal",
        "name": "Archives municipales d'Exampleton",
        "jurisdiction": ["99350"],
        "areas": ["Exampleton"],
        "citation_codes": ["AMEXT"],
        "website": "https://archives.exampleton.example",
        "collections": [{
            "id": "civil-status",
            "acts": ["N", "M", "D"],
            "platform": "scripted",
            "portal": { "outcome": "view" },
        }]
    })
    .to_string();
    ArchiveRegistry::new(
        &[("fr", department.as_str()), ("fr", municipal.as_str())],
        vec![Box::new(Scripted)],
    )
    .expect("a valid catalogue")
}

/// The parts a case expects, every one compared: an unnamed part is
/// expected absent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Parts {
    archive: String,
    act: Option<String>,
    locality: Option<String>,
    parish: Option<String>,
    year: Option<u16>,
    call_number: Option<String>,
    number: Option<u32>,
    /// `45`, `5d,6g`.
    views: Option<String>,
    view_count: Option<u16>,
    folio: Option<String>,
}

fn at(archive: &str) -> Parts {
    Parts {
        archive: archive.to_owned(),
        ..Parts::default()
    }
}

impl Parts {
    fn act(mut self, act: &str) -> Self {
        self.act = Some(act.to_owned());
        self
    }
    fn locality(mut self, locality: &str) -> Self {
        self.locality = Some(locality.to_owned());
        self
    }
    fn parish(mut self, parish: &str) -> Self {
        self.parish = Some(parish.to_owned());
        self
    }
    fn year(mut self, year: u16) -> Self {
        self.year = Some(year);
        self
    }
    fn call(mut self, call_number: &str) -> Self {
        self.call_number = Some(call_number.to_owned());
        self
    }
    fn number(mut self, number: u32) -> Self {
        self.number = Some(number);
        self
    }
    fn views(mut self, views: &str) -> Self {
        self.views = Some(views.to_owned());
        self
    }
    fn of(mut self, count: u16) -> Self {
        self.view_count = Some(count);
        self
    }
    fn folio(mut self, folio: &str) -> Self {
        self.folio = Some(folio.to_owned());
        self
    }

    fn from(recognition: &Recognition<'_>) -> Self {
        let found = &recognition.found;
        let views = (!found.views.is_empty()).then(|| {
            found
                .views
                .iter()
                .map(|view| {
                    let side = match view.side {
                        Some(Side::Right) => "d",
                        Some(Side::Left) => "g",
                        None => "",
                    };
                    format!("{}{side}", view.view)
                })
                .collect::<Vec<_>>()
                .join(",")
        });
        Self {
            archive: recognition.archive.id.clone(),
            act: found.act.as_ref().map(ToString::to_string),
            locality: found.locality.clone(),
            parish: found.parish.clone(),
            year: found.year,
            call_number: found
                .call_number
                .as_ref()
                .map(|call| call.as_str().to_owned()),
            number: found.number,
            views,
            view_count: found.view_count,
            folio: found.folio.clone(),
        }
    }
}

fn title(title: &str) -> CitationEvidence {
    CitationEvidence {
        title: title.to_owned(),
        ..CitationEvidence::default()
    }
}

fn recognized(registry: &ArchiveRegistry, evidence: &CitationEvidence) -> Parts {
    let recognition = registry
        .recognize(evidence, None, None)
        .unwrap_or_else(|error| panic!("{error:?}: {evidence:?}"));
    Parts::from(&recognition)
}

/// Runs a corpus of titles, returning how many cases it held.
fn corpus(registry: &ArchiveRegistry, cases: &[(&str, Parts)]) -> usize {
    for (text, expected) in cases {
        assert_eq!(&recognized(registry, &title(text)), expected, "{text}");
    }
    cases.len()
}

const AD99: &str = "fr-ad99";

#[test]
fn reads_the_classic_description_from_the_archive_to_the_act() {
    let cases = [
        (
            "AD99, état civil de Exampleville, naissances 1872, cote 4E 1234, vue 45/200, acte n° 312",
            at(AD99)
                .act("N")
                .locality("Exampleville")
                .year(1872)
                .call("4E 1234")
                .views("45")
                .of(200)
                .number(312),
        ),
        (
            "Archives départementales de l'Exampleshire, état civil de Exampleville, naissances 1872, \
             cote 4E 1234, vue 45/200, acte n° 312",
            at(AD99)
                .act("N")
                .locality("Exampleville")
                .year(1872)
                .call("4E 1234")
                .views("45")
                .of(200)
                .number(312),
        ),
        (
            "AD99, GG 45, BMS de Saint-Exemple 1745-1760, f° 23 v°, baptême du 12/03/1752",
            at(AD99)
                .act("B")
                .locality("Saint-Exemple")
                .year(1745)
                .call("GG 45")
                .folio("f° 23 v°"),
        ),
        (
            "Arch. dép. Exampleshire, état civil, Exampleville, décès 1901, vue 12",
            at(AD99)
                .act("D")
                .locality("Exampleville")
                .year(1901)
                .views("12"),
        ),
        (
            "AD 99, registres paroissiaux de Saint-Exemple, mariages 1750-1759, vue 102/180",
            at(AD99)
                .act("M")
                .locality("Saint-Exemple")
                .year(1750)
                .views("102")
                .of(180),
        ),
        (
            "Archives départementales de l'Exampleshire (AD99), 3 E 45/12, Exampleville, sépultures 1712, f° 4 r°",
            at(AD99)
                .act("S")
                .locality("Exampleville")
                .year(1712)
                .call("3 E 45/12")
                .folio("f° 4 r°"),
        ),
        (
            "A.D. 99 - Exampleville - mariage du 3 février 1823 - vue 7",
            at(AD99)
                .act("M")
                .locality("Exampleville")
                .year(1823)
                .views("7"),
        ),
        (
            "AD99, tables décennales de Exampleville 1873-1882, vue 5",
            at(AD99)
                .act("TD")
                .locality("Exampleville")
                .year(1873)
                .views("5"),
        ),
        (
            "AD99, Exampleville, publications de mariage 1850, vue 9",
            at(AD99)
                .act("P")
                .locality("Exampleville")
                .year(1850)
                .views("9"),
        ),
        (
            "Archives de l'Exampleshire, Exampleville, BMS 1700-1710, image 33",
            at(AD99)
                .act("BMS")
                .locality("Exampleville")
                .year(1700)
                .views("33"),
        ),
        (
            "AD99, état civil de Saint-Exemple-sur-Loire, naissances, 1880, acte 26, vue 5d/13",
            at(AD99)
                .act("N")
                .locality("Saint-Exemple-sur-Loire")
                .year(1880)
                .number(26)
                .views("5d")
                .of(13),
        ),
        (
            "AD99, NMD 1802-1812, Exampleville, vue 210",
            at(AD99)
                .act("NMD")
                .locality("Exampleville")
                .year(1802)
                .views("210"),
        ),
        (
            "AD99, état civil, Exampleville, naissances an XII, vue 14",
            at(AD99)
                .act("N")
                .locality("Exampleville")
                .year(1803)
                .views("14"),
        ),
        (
            "AD99, paroisse Saint-Exemple, Exampleville, baptêmes 1689, vue 3",
            at(AD99)
                .act("B")
                .locality("Exampleville")
                .parish("Saint-Exemple")
                .year(1689)
                .views("3"),
        ),
        (
            "AD99 ; Exampleville ; décès ; 1915 ; vue 88 sur 120",
            at(AD99)
                .act("D")
                .locality("Exampleville")
                .year(1915)
                .views("88")
                .of(120),
        ),
        (
            "Arch. dép. de l'Exampleshire, cote 2 E 1745, registre paroissial de Exampleville, 1745, p. 12",
            at(AD99)
                .locality("Exampleville")
                .year(1745)
                .call("2 E 1745")
                .folio("p. 12"),
        ),
        (
            "AD99. Naissance de Exampleville, 1854, n° 41, vue 12/48",
            at(AD99)
                .act("N")
                .locality("Exampleville")
                .year(1854)
                .number(41)
                .views("12")
                .of(48),
        ),
        (
            "Archives départementales de la Basse-Exampleshire, Exampleville, décès 1840",
            at(AD99).act("D").locality("Exampleville").year(1840),
        ),
        (
            "AD99, Exampleville, mariages 1793-1794, cote 4 E 12, vue 30",
            at(AD99)
                .act("M")
                .locality("Exampleville")
                .year(1793)
                .call("4 E 12")
                .views("30"),
        ),
        (
            "AD99, état civil de Exampleville, décès 1880 à 1889, vue 77",
            at(AD99)
                .act("D")
                .locality("Exampleville")
                .year(1880)
                .views("77"),
        ),
        (
            "AD99, Exampleville, naissances 1890 – 1899, vue 15",
            at(AD99)
                .act("N")
                .locality("Exampleville")
                .year(1890)
                .views("15"),
        ),
        (
            "Archives départementales de l'Exampleshire, registres d'état civil, commune de Exampleville, \
             décès, 1911, acte n°7",
            at(AD99)
                .act("D")
                .locality("Exampleville")
                .year(1911)
                .number(7),
        ),
        (
            "ADEX, Exampleville, baptêmes et sépultures 1701, vue 2",
            at(AD99)
                .act("BS")
                .locality("Exampleville")
                .year(1701)
                .views("2"),
        ),
        (
            "AD99, acte de naissance de Exampleville, 1er mars 1861, v. 52",
            at(AD99)
                .act("N")
                .locality("Exampleville")
                .year(1861)
                .views("52"),
        ),
        (
            "AD99 : état civil d'Exampleville (naissances, 1871-1880), vue 101 sur 412",
            at(AD99)
                .act("N")
                .locality("Exampleville")
                .year(1871)
                .views("101")
                .of(412),
        ),
        (
            "\"AD99\", \"Exampleville\", \"Décès\", \"1899\", \"Vue 12/80\"",
            at(AD99)
                .act("D")
                .locality("Exampleville")
                .year(1899)
                .views("12")
                .of(80),
        ),
        (
            "AD99, Exampleville, registres paroissiaux 1680-1700, sépulture du 4 juin 1690, vues 40-41/220",
            at(AD99)
                .act("S")
                .locality("Exampleville")
                .year(1680)
                .views("40,41")
                .of(220),
        ),
        (
            "Archives départementales de l'Exampleshire, Exampleville, mariages, 1820, 4 E 12/3, vue 6g",
            at(AD99)
                .act("M")
                .locality("Exampleville")
                .year(1820)
                .call("4 E 12/3")
                .views("6g"),
        ),
        (
            "AD99 | Exampleville | TD 1853-1862 | vue 4",
            at(AD99)
                .act("TD")
                .locality("Exampleville")
                .year(1853)
                .views("4"),
        ),
        (
            "AD99, Saint-Exemple-de-la-Forêt, décès 1871, n°3",
            at(AD99)
                .act("D")
                .locality("Saint-Exemple-de-la-Forêt")
                .year(1871)
                .number(3),
        ),
    ];
    assert_eq!(corpus(&registry(), &cases), 30);
}

#[test]
fn reads_the_short_form() {
    let cases = [
        (
            "AD99, 4E 1234, v. 45, n° 312",
            at(AD99).call("4E 1234").views("45").number(312),
        ),
        (
            "AD99, 4E 1234, v. 45/200",
            at(AD99).call("4E 1234").views("45").of(200),
        ),
        (
            "AD99, 1 Mi 456, vue 12",
            at(AD99).call("1 Mi 456").views("12"),
        ),
        ("AD99 4E1234 v.45", at(AD99).views("45")),
        (
            "AD99, Exampleville, N 1877, v. 5",
            at(AD99)
                .act("N")
                .locality("Exampleville")
                .year(1877)
                .views("5"),
        ),
        (
            "AD99, Exampleville, B 1702",
            at(AD99).act("B").locality("Exampleville").year(1702),
        ),
        (
            "AD99, Exampleville, BMS 1650-1699",
            at(AD99).act("BMS").locality("Exampleville").year(1650),
        ),
        (
            "AD99, Exampleville, D 1880, n° 4",
            at(AD99)
                .act("D")
                .locality("Exampleville")
                .year(1880)
                .number(4),
        ),
        (
            "AD99, Exampleville, 1872",
            at(AD99).locality("Exampleville").year(1872),
        ),
        ("AD99, f° 12 r°", at(AD99).folio("f° 12 r°")),
        (
            "AD99, 3E73/14, 579/833",
            at(AD99).call("3E73/14").views("579").of(833),
        ),
        (
            "AD99, Exampleville, naiss. 1888, v. 7",
            at(AD99)
                .act("N")
                .locality("Exampleville")
                .year(1888)
                .views("7"),
        ),
        (
            "AD99, Exampleville, sép. 1730",
            at(AD99).act("S").locality("Exampleville").year(1730),
        ),
        (
            "AD99, Exampleville, bapt. 1730, fol. 3",
            at(AD99)
                .act("B")
                .locality("Exampleville")
                .year(1730)
                .folio("fol. 3"),
        ),
    ];
    assert_eq!(corpus(&registry(), &cases), 14);
}

#[test]
fn reads_series_in_words() {
    let cases = [
        (
            "AD99, 6M 123, recensement de Exampleville 1896, vue 34, ménage n° 56",
            at(AD99)
                .act("RP")
                .locality("Exampleville")
                .year(1896)
                .call("6M 123")
                .views("34"),
        ),
        (
            "AD99, 1R 456, classe 1905, bureau de Exampleville, matricule n° 789",
            at(AD99)
                .act("RM")
                .locality("Exampleville")
                .year(1905)
                .call("1R 456")
                .number(789),
        ),
        (
            "AD99, recensement de population, Exampleville, 1906, vue 23",
            at(AD99)
                .act("RP")
                .locality("Exampleville")
                .year(1906)
                .views("23"),
        ),
        (
            "AD99, liste nominative de Exampleville, 1872, maison n° 12",
            at(AD99).act("RP").locality("Exampleville").year(1872),
        ),
        (
            "AD99, registres matricules, classe 1912, bureau de Exampleville, matricule 1234, vue 400/800",
            at(AD99)
                .act("RM")
                .locality("Exampleville")
                .year(1912)
                .number(1234)
                .views("400")
                .of(800),
        ),
        (
            "AD99, 1 R 1201, registre matricule, classe 1890",
            at(AD99).act("RM").year(1890).call("1 R 1201"),
        ),
        (
            "AD99, tables des successions et absences, bureau de Exampleville, 1850-1860, vue 13/159",
            at(AD99)
                .act("TSA")
                .locality("Exampleville")
                .year(1850)
                .views("13")
                .of(159),
        ),
        (
            "AD99, tirage au sort, classe 1855, Exampleville",
            at(AD99).act("CM").locality("Exampleville").year(1855),
        ),
        (
            "AD99, conscrits de l'an XIII, Exampleville",
            at(AD99).act("CM").locality("Exampleville").year(1804),
        ),
        (
            "Archives départementales de l'Exampleshire, dénombrement de Exampleville, 1836, vue 8",
            at(AD99)
                .act("RP")
                .locality("Exampleville")
                .year(1836)
                .views("8"),
        ),
    ];
    assert_eq!(corpus(&registry(), &cases), 10);
}

/// The normalized titles the strict grammar reads, with their pages.
const NORMALIZED: &[(&str, Option<&str>)] = &[
    (
        "AD99 - Exampleville - (aucun) - N - 1877 - 3E1/2 - acte 26 - vue 5d/13",
        None,
    ),
    (
        "AD99 - Example - Part - Saint-Example - B - 1791 - 3E1/2 - acte 4 - vue 3g/12",
        None,
    ),
    (
        "AD99 - Exampleville - (aucun) - N - 1877 - 3E1/2",
        Some("acte 26 - vue 5d/13"),
    ),
    ("AD99 - Exampleville - (aucun) - TD - 1803", None),
    ("AD99 - Exampleville - (aucun) - D - an XII", None),
    (
        "AD99 - Exampleville - (aucun) - M - 1850 - vue 4-6/13",
        None,
    ),
    ("AD99 - Exampleville - (aucun) - NPMD - 1877", None),
    (
        "AD99 - Exampleville - Recensement - 1872 - 6 M 999 - vue 12g/40",
        None,
    ),
    (
        "AD99 - Exampleville - Registres matricules - 1898 - 1 R 9999 - 348 - 579/833",
        None,
    ),
    (
        "AD99 - Registres matricules des classes 1859 à 1940 - 1871 - 1 R 9999 - vue 181/196",
        None,
    ),
    (
        "AD99 - Exampleville - Tables des successions et absences - 1897-1898 - Q_NUM_EXA_50 - vue 13/159",
        None,
    ),
    ("AD99 - Exampleville - Conscrits militaires - 1897", None),
];

#[test]
fn reads_the_normalized_form_exactly_as_the_strict_grammar() {
    let registry = registry();
    for (text, page) in NORMALIZED {
        let evidence = CitationEvidence {
            page: page.map(str::to_owned),
            ..title(text)
        };
        let recognition = registry.recognize(&evidence, None, None).expect(text);
        let strict = CitationParts::parse(&cited_text(text, *page), &CitationGrammar::default())
            .expect("a normalized citation");
        assert_eq!(recognition.citation(), Some(strict), "{text}");
        assert_eq!(recognition.signal(Part::Archive), Some(Signal::Normalized));
        assert_eq!(
            recognition.signal(Part::Act),
            Some(Signal::Normalized),
            "{text}"
        );
    }
    // An archive the catalogue does not list, and a table no collection
    // holds.
    for text in [
        "AD67 - Exampleville - (aucun) - N - 1877",
        "AD99 - Exampleville - (aucun) - TB - 1750",
    ] {
        assert_eq!(
            registry.recognize(&title(text), None, None),
            Err(Unrecognized::NoAdapter),
            "{text}"
        );
    }
}

fn birth(year: u16, place: &str) -> CitedEvent {
    CitedEvent {
        event_type: EventType::Birth,
        year: Some(year),
        place: Some(place.to_owned()),
        agency: None,
    }
}

fn held(name: &str, call_number: Option<&str>) -> HeldAt {
    HeldAt {
        name: name.to_owned(),
        call_number: call_number.map(str::to_owned),
        website: None,
    }
}

#[test]
fn reads_structured_records_and_the_cited_event() {
    let registry = registry();
    let event = |event_type, year, place: &str| CitedEvent {
        event_type,
        year,
        place: Some(place.to_owned()),
        agency: None,
    };
    let cases = [
        // The register as the source, the archive and its call number on
        // the repository, the act and view in the page, the rest on the
        // event.
        (
            CitationEvidence {
                page: Some("vue 45, acte 312".to_owned()),
                repositories: vec![held(
                    "Archives départementales de l'Exampleshire",
                    Some("4 E 1234"),
                )],
                event: Some(birth(1872, "Exampleville, Exampleshire, France")),
                ..title("Registres paroissiaux et d'état civil")
            },
            at(AD99)
                .act("N")
                .locality("Exampleville")
                .year(1872)
                .call("4 E 1234")
                .views("45")
                .number(312),
        ),
        (
            CitationEvidence {
                page: Some("f° 23".to_owned()),
                repositories: vec![held("AD99", None)],
                event: Some(event(EventType::Death, Some(1885), "Exampleville")),
                ..title("État civil de Exampleville")
            },
            at(AD99)
                .act("D")
                .locality("Exampleville")
                .year(1885)
                .folio("f° 23"),
        ),
        // A birth in a parish register is a baptism; a death before civil
        // registration a burial.
        (
            CitationEvidence {
                event: Some(birth(1752, "Exampleville, Exampleshire")),
                ..title("AD99, registres paroissiaux de Exampleville, 1750-1760, vue 12")
            },
            at(AD99)
                .act("B")
                .locality("Exampleville")
                .year(1750)
                .views("12"),
        ),
        (
            CitationEvidence {
                repositories: vec![held("Archives de l'Exampleshire", None)],
                event: Some(event(EventType::Death, Some(1760), "Exampleville")),
                ..title("Registres")
            },
            at(AD99).act("S").locality("Exampleville").year(1760),
        ),
        // A GEDCOM-shaped source: the locality in the title, the period and
        // view in the page.
        (
            CitationEvidence {
                page: Some("1801-1802, vue 210/350".to_owned()),
                repositories: vec![held(
                    "Archives départementales de l'Exampleshire",
                    Some("3 E 12/4"),
                )],
                event: Some(event(
                    EventType::Marriage,
                    Some(1801),
                    "Exampleville, Exampleshire",
                )),
                ..title("Exampleville, Registres paroissiaux et d'état civil")
            },
            at(AD99)
                .act("M")
                .locality("Exampleville")
                .year(1801)
                .call("3 E 12/4")
                .views("210")
                .of(350),
        ),
        // A source held at an archive, cited for an event its registers
        // record.
        (
            CitationEvidence {
                repositories: vec![held("AD99", None)],
                event: Some(birth(1890, "Exampleville")),
                ..title("Notes de famille")
            },
            at(AD99).act("N").locality("Exampleville").year(1890),
        ),
        // The call number of the archive's link, not another repository's.
        (
            CitationEvidence {
                repositories: vec![
                    held("Société généalogique d'Exemple", Some("BIB-12")),
                    held("AD99", Some("4 E 9")),
                ],
                event: Some(birth(1890, "Exampleville")),
                ..title("État civil")
            },
            at(AD99)
                .act("N")
                .locality("Exampleville")
                .year(1890)
                .call("4 E 9"),
        ),
        // The parish from the event's agency.
        (
            CitationEvidence {
                event: Some(CitedEvent {
                    agency: Some("Paroisse Saint-Exemple".to_owned()),
                    ..event(EventType::Baptism, Some(1700), "Exampleville, Exampleshire")
                }),
                repositories: vec![held("AD99", None)],
                ..title("Registres paroissiaux")
            },
            at(AD99)
                .act("B")
                .locality("Exampleville")
                .parish("Saint-Exemple")
                .year(1700),
        ),
        // The archive from a repository's website.
        (
            CitationEvidence {
                repositories: vec![HeldAt {
                    website: Some("https://archives.example.org/".to_owned()),
                    ..held("Archives", None)
                }],
                event: Some(birth(1880, "Exampleville")),
                ..title("État civil")
            },
            at(AD99).act("N").locality("Exampleville").year(1880),
        ),
        // A municipal archive serves its commune.
        (
            CitationEvidence {
                page: Some("vue 3".to_owned()),
                ..title("Archives municipales d'Exampleton, naissances 1890")
            },
            at("fr-am-exampleton")
                .act("N")
                .locality("Exampleton")
                .year(1890)
                .views("3"),
        ),
        (
            CitationEvidence {
                event: Some(birth(1890, "Exampleton, 99350")),
                ..title("AMEXT, état civil, acte n° 12")
            },
            at("fr-am-exampleton")
                .act("N")
                .locality("Exampleton")
                .year(1890)
                .number(12),
        ),
    ];
    for (evidence, expected) in &cases {
        assert_eq!(&recognized(&registry, evidence), expected, "{evidence:?}");
    }
    assert_eq!(cases.len(), 11);
}

#[test]
fn each_part_keeps_its_signal() {
    let registry = registry();
    let evidence = CitationEvidence {
        page: Some("vue 45, acte 312".to_owned()),
        repositories: vec![held(
            "Archives départementales de l'Exampleshire",
            Some("4 E 1234"),
        )],
        event: Some(birth(1872, "Exampleville, Exampleshire, France")),
        ..title("Registres paroissiaux et d'état civil")
    };
    let recognition = registry.recognize(&evidence, None, None).unwrap();
    for (part, signal) in [
        (Part::Archive, Signal::Record),
        (Part::CallNumber, Signal::Record),
        (Part::Views, Signal::Words),
        (Part::Number, Signal::Words),
        (Part::Act, Signal::Event),
        (Part::Year, Signal::Event),
        (Part::Locality, Signal::Event),
    ] {
        assert_eq!(recognition.signal(part), Some(signal), "{part:?}");
    }
    assert_eq!(recognition.signal(Part::Folio), None);
    assert!(recognition.citation().is_some());
    assert!(recognition.missing().is_empty());

    // The words win over the event for what they say.
    let evidence = CitationEvidence {
        event: Some(birth(1752, "Le Bourg, Exampleville")),
        ..title("AD99, BMS de Saint-Exemple 1745-1760, baptême du 12/03/1752")
    };
    let recognition = registry.recognize(&evidence, None, None).unwrap();
    assert_eq!(recognition.found.locality.as_deref(), Some("Saint-Exemple"));
    assert_eq!(recognition.signal(Part::Locality), Some(Signal::Words));
    assert_eq!(recognition.found.year, Some(1745));
    assert_eq!(recognition.found.period.as_deref(), Some("1745-1760"));
}

#[test]
fn a_portal_address_is_opened_as_it_is() {
    let registry = registry();
    let cases = [
        (
            CitationEvidence {
                page: Some("https://archives.example.org/ark:/99999/a011234/v0045".to_owned()),
                ..title("Acte de naissance")
            },
            Some("https://archives.example.org/ark:/99999/a011234/v0045"),
        ),
        (
            CitationEvidence {
                urls: vec!["https://archives.example.org/viewer?id=12&view=4".to_owned()],
                ..title("Naissance")
            },
            Some("https://archives.example.org/viewer?id=12&view=4"),
        ),
        (
            title("Voir https://archives.example.org/ark:/99999/a2 (vue 4)."),
            Some("https://archives.example.org/ark:/99999/a2"),
        ),
        // The home page names the archive, not a register.
        (
            CitationEvidence {
                page: Some("https://archives.example.org/".to_owned()),
                ..title("État civil de Exampleville, naissances 1872")
            },
            None,
        ),
        // The address wins over the normalized form, whose parts remain.
        (
            CitationEvidence {
                text: Some("Lien : https://archives.example.org/ark:/99999/a3".to_owned()),
                ..title("AD99 - Exampleville - (aucun) - N - 1877 - vue 5/13")
            },
            Some("https://archives.example.org/ark:/99999/a3"),
        ),
        (
            title("https://archives.exampleton.example/registre/12"),
            Some("https://archives.exampleton.example/registre/12"),
        ),
    ];
    for (evidence, address) in &cases {
        let recognition = registry
            .recognize(evidence, None, None)
            .unwrap_or_else(|error| panic!("{error:?}: {evidence:?}"));
        assert_eq!(recognition.address.as_deref(), *address, "{evidence:?}");
        assert_eq!(recognition.signal(Part::Archive), Some(Signal::Address));
    }
    let recognition = registry.recognize(&cases[4].0, None, None).unwrap();
    assert_eq!(recognition.found.locality.as_deref(), Some("Exampleville"));
    assert_eq!(recognition.signal(Part::Locality), Some(Signal::Normalized));
    let recognition = registry.recognize(&cases[5].0, None, None).unwrap();
    assert_eq!(recognition.archive.id, "fr-am-exampleton");

    // An address of no catalogued portal names no archive.
    assert_eq!(
        registry.recognize(&title("https://www.example.com/registre/12"), None, None),
        Err(Unrecognized::NotACitation)
    );
}

#[test]
fn refuses_what_names_no_catalogued_register() {
    let registry = registry();
    for (evidence, outcome) in [
        (title("Fictitious register"), Unrecognized::NotACitation),
        (
            title("Le Petit Exemple, journal, 1890"),
            Unrecognized::NotACitation,
        ),
        (title("Recensement 1896, vue 4"), Unrecognized::NotACitation),
        (
            title("Archives départementales de l'Imaginaire, état civil de Exampleville"),
            Unrecognized::NoAdapter,
        ),
        (
            title("AD67, état civil de Exampleville, naissances 1872"),
            Unrecognized::NoAdapter,
        ),
        (
            title("AD99, table des baptêmes de Exampleville 1750"),
            Unrecognized::NoAdapter,
        ),
        // A repository of the archive, but nothing cites a register.
        (
            CitationEvidence {
                repositories: vec![held("AD99", None)],
                event: Some(CitedEvent {
                    event_type: EventType::Occupation,
                    year: Some(1890),
                    place: Some("Exampleville".to_owned()),
                    agency: None,
                }),
                ..title("Notes de famille")
            },
            Unrecognized::NotACitation,
        ),
        // A place in the archive's area, but nothing cites a register.
        (
            CitationEvidence {
                event: Some(CitedEvent {
                    event_type: EventType::Occupation,
                    year: None,
                    place: Some("Exampleville, Exampleshire".to_owned()),
                    agency: None,
                }),
                ..title("Annuaire du commerce")
            },
            Unrecognized::NotACitation,
        ),
    ] {
        assert_eq!(
            registry.recognize(&evidence, None, None),
            Err(outcome),
            "{evidence:?}"
        );
    }

    // A transcription's names are the act's people, not the register's
    // locality.
    let evidence = CitationEvidence {
        text: Some("Jean Exemple, fils de Pierre Exemple et de Marie Exemple".to_owned()),
        ..title("AD99, naissances 1872")
    };
    let recognition = registry.recognize(&evidence, None, None).unwrap();
    assert_eq!(recognition.found.locality, None);
    assert_eq!(recognition.missing(), [Part::Locality]);
}

#[test]
fn a_kind_of_document_nobody_holds_is_no_link() {
    let registry = registry();
    for text in [
        "AD99 - Exampleville - Registres d’écrou des condamnés 27 octobre 1853-14 juin 1855 - 2Y2 26 - vue 165g/226",
        "AD99, Exampleville, Registre d'entrées de l'hôpital, 1880, vue 12",
        "AD99, Exampleville, Minutes notariales 1750-1760, vue 3",
        "AD99, Exampleville, Matrice cadastrale, vue 8",
        "AD99, Hypothèques, Exampleville, 1820",
        "AD99, Exampleville, Répertoire des actes, 1810",
        "AD99, Exampleville, registres d'écrou, vue 4",
    ] {
        // The event's kind never stands in for the unknown one.
        let evidence = CitationEvidence {
            event: Some(birth(1853, "Exampleville")),
            ..title(text)
        };
        assert_eq!(
            registry.recognize(&evidence, None, None),
            Err(Unrecognized::NoAdapter),
            "{text}"
        );
    }
    // The reader's own kind overrides the words.
    let supplied = SuppliedParts {
        act: Some(Act::from_code("N").unwrap()),
        ..SuppliedParts::default()
    };
    assert!(
        registry
            .recognize(
                &title("AD99, Exampleville, Matrice cadastrale"),
                Some(&supplied),
                None
            )
            .is_ok()
    );
}

#[test]
fn free_fields_and_known_kinds_are_not_refused() {
    let registry = registry();
    for text in [
        "AD99, Canton est, collection communale, vue 4",
        "AD99, Exampleville, registre, 1872, vue 4",
        "AD99, Exampleville, registre des naissances 1872, vue 4",
        "AD99, registre de l'état civil de Exampleville, 1872",
        "AD99, registres de Exampleville, 1872",
        "AD99, 4E 1234, v. 45, n° 312",
    ] {
        assert!(
            registry.recognize(&title(text), None, None).is_ok(),
            "{text}"
        );
    }
    // Archive known, act missing: still the dialog's case.
    let recognition = registry
        .recognize(&title("AD99, Exampleville, Canton est, vue 4"), None, None)
        .unwrap();
    assert_eq!(recognition.missing(), [Part::Act]);
}

#[test]
fn an_incomplete_citation_says_what_is_missing() {
    let registry = registry();
    let recognition = registry
        .recognize(&title("AD99, 4E 1234, v. 45, n° 312"), None, None)
        .unwrap();
    assert_eq!(recognition.missing(), [Part::Act, Part::Locality]);
    assert_eq!(recognition.citation(), None);

    // A series needs no locality.
    let recognition = registry
        .recognize(&title("AD99, registre matricule, classe 1890"), None, None)
        .unwrap();
    assert!(recognition.missing().is_empty());
    assert_eq!(recognition.citation().unwrap().locality, "");
}

#[test]
fn the_reader_completes_a_citation_and_may_keep_what_they_wrote() {
    let registry = registry();
    let evidence = CitationEvidence {
        page: Some("v. 45, n° 312".to_owned()),
        ..title("AD99, 4E 1234")
    };
    let supplied = SuppliedParts {
        locality: Some(" Exampleville ".to_owned()),
        act: Act::from_code("N"),
        year: Some(1872),
        view: None,
    };
    let archive = &registry.archives()[0];
    assert_eq!(supplied.validate(archive), Ok(()));
    let recognition = registry
        .recognize(&evidence, Some(&supplied), None)
        .unwrap();
    assert_eq!(
        Parts::from(&recognition),
        at(AD99)
            .act("N")
            .locality("Exampleville")
            .year(1872)
            .call("4E 1234")
            .views("45")
            .number(312)
    );
    assert_eq!(recognition.signal(Part::Locality), Some(Signal::Reader));
    assert_eq!(recognition.signal(Part::Views), Some(Signal::Words));

    // Written into the page, the parts are read back without the reader.
    let written = recognition.written(&registry).expect("parts to write");
    assert_eq!(written, "Exampleville, naissance 1872");
    let kept = CitationEvidence {
        page: Some(format!("v. 45, n° 312, {written}")),
        ..evidence.clone()
    };
    let again = registry.recognize(&kept, None, None).unwrap();
    assert_eq!(again.citation(), recognition.citation());

    // A view the reader names replaces the cited ones, keeping the count
    // and the side the citation gives it.
    let evidence = title("AD99, Exampleville, naissances 1872, vue 45d/200");
    let supplied = SuppliedParts {
        view: Some(45),
        ..SuppliedParts::default()
    };
    let recognition = registry
        .recognize(&evidence, Some(&supplied), None)
        .unwrap();
    assert_eq!(
        recognition.found.views,
        [CitedView {
            view: 45,
            side: Some(Side::Right)
        }]
    );
    assert_eq!(recognition.found.view_count, Some(200));
    let supplied = SuppliedParts {
        view: Some(46),
        ..SuppliedParts::default()
    };
    let recognition = registry
        .recognize(&evidence, Some(&supplied), None)
        .unwrap();
    assert_eq!(
        recognition.found.views,
        [CitedView {
            view: 46,
            side: None
        }]
    );
    assert_eq!(recognition.written(&registry).as_deref(), Some("vue 46"));

    // What a reader may not supply.
    for invalid in [
        SuppliedParts {
            locality: Some("  ".to_owned()),
            ..SuppliedParts::default()
        },
        SuppliedParts {
            locality: Some("x".repeat(201)),
            ..SuppliedParts::default()
        },
        SuppliedParts {
            act: Act::from_code("TB"),
            ..SuppliedParts::default()
        },
        SuppliedParts {
            year: Some(900),
            ..SuppliedParts::default()
        },
        SuppliedParts {
            view: Some(0),
            ..SuppliedParts::default()
        },
    ] {
        assert!(invalid.validate(archive).is_err(), "{invalid:?}");
    }
}

/// A place dictionary knowing a few fictitious places, counting its
/// lookups.
struct Places {
    lookups: Cell<usize>,
}

impl PlaceLookup for Places {
    fn areas(&self, names: &[String]) -> Vec<Vec<String>> {
        self.lookups.set(self.lookups.get() + 1);
        names
            .iter()
            .map(|name| match name.as_str() {
                "Exampleville" | "Exampleton" => {
                    vec!["Exampleshire".to_owned(), "Example Region".to_owned()]
                }
                "Saint-Exemple" => vec!["Othershire".to_owned()],
                _ => Vec::new(),
            })
            .collect()
    }
}

#[test]
fn the_place_dictionary_tells_the_locality_from_a_parish_or_a_hamlet() {
    let registry = registry();
    let places = Places {
        lookups: Cell::new(0),
    };
    // Two names in the words: the one the dictionary knows in the
    // archive's area is the locality, the other the parish.
    let evidence = title("AD99, Saint-Exemple, Exampleville, BMS 1745-1760, vue 12");
    let recognition = registry.recognize(&evidence, None, Some(&places)).unwrap();
    assert_eq!(recognition.found.locality.as_deref(), Some("Exampleville"));
    assert_eq!(recognition.found.parish.as_deref(), Some("Saint-Exemple"));
    assert_eq!(recognition.signal(Part::Locality), Some(Signal::Words));
    assert!(recognition.confirmed_locality);
    assert_eq!(places.lookups.get(), 1);
    // Without the dictionary, the first.
    let recognition = registry.recognize(&evidence, None, None).unwrap();
    assert_eq!(recognition.found.locality.as_deref(), Some("Saint-Exemple"));
    assert!(!recognition.confirmed_locality);
}

#[test]
fn the_place_dictionary_reads_the_events_place_only_when_the_words_name_no_locality() {
    let registry = registry();
    let places = Places {
        lookups: Cell::new(0),
    };
    // No locality in the words: the event's place, its hamlet set aside.
    let evidence = CitationEvidence {
        event: Some(birth(1752, "Le Bourg, Exampleville, Exampleshire")),
        ..title("AD99, BMS 1745-1760, vue 12")
    };
    let recognition = registry.recognize(&evidence, None, Some(&places)).unwrap();
    assert_eq!(recognition.found.locality.as_deref(), Some("Exampleville"));
    assert_eq!(recognition.signal(Part::Locality), Some(Signal::Event));
    assert!(recognition.confirmed_locality);
    assert_eq!(places.lookups.get(), 1);
    let recognition = registry.recognize(&evidence, None, None).unwrap();
    assert_eq!(recognition.found.locality.as_deref(), Some("Le Bourg"));

    // The words' locality stands, whatever the event's place: a single
    // candidate is not looked up.
    let evidence = CitationEvidence {
        event: Some(birth(1752, "Exampleville, Exampleshire")),
        ..title("AD99, BMS de Saint-Exemple 1745-1760")
    };
    let recognition = registry.recognize(&evidence, None, Some(&places)).unwrap();
    assert_eq!(recognition.found.locality.as_deref(), Some("Saint-Exemple"));
    assert_eq!(recognition.signal(Part::Locality), Some(Signal::Words));
    assert_eq!(places.lookups.get(), 1);
}

fn death(year: u16, place: &str) -> CitedEvent {
    CitedEvent {
        event_type: EventType::Death,
        year: Some(year),
        place: Some(place.to_owned()),
        agency: None,
    }
}

/// A death cited from a register its text designates elsewhere than where
/// the death happened: the text's archive, locality, kinds and year stand,
/// and an archive the catalogue does not list leaves the citation without a
/// link — the event's place never designates another.
#[test]
fn an_archive_designated_in_the_text_wins_over_the_cited_event() {
    let registry = registry();
    let places = Places {
        lookups: Cell::new(0),
    };
    // The death happened in AD99's area.
    let elsewhere = death(1911, "Le Bourg, Exampleton, Exampleshire");
    let with = |text: &str| CitationEvidence {
        event: Some(elsewhere.clone()),
        ..title(text)
    };
    for evidence in [
        with("AD98 - Exampleville-sur-Example - NMD - 1911 - 2 E 9999 - acte 38 - vue 11g/52"),
        with("AD98 - Exampleville - (aucun) - NMD - 1911 - 2 E 9999 - acte 38 - vue 11g/52"),
        with("Archives départementales de l'Imaginaire, Exampleville, décès 1911"),
        CitationEvidence {
            repositories: vec![held(
                "Archives départementales de l'Imaginaire",
                Some("2 E 9999"),
            )],
            ..with("Registres d'état civil")
        },
        // Nothing designates an archive: the event's place does not either.
        with("Registres d'état civil, vue 11"),
        with("NMD 1911, acte 38"),
    ] {
        let expected = if evidence.title.starts_with("Registres d'état civil, vue")
            || evidence.title.starts_with("NMD")
        {
            Unrecognized::NotACitation
        } else {
            Unrecognized::NoAdapter
        };
        for lookup in [None, Some(&places as &dyn PlaceLookup)] {
            assert_eq!(
                registry.recognize(&evidence, None, lookup),
                Err(expected),
                "{evidence:?}"
            );
        }
    }

    // The same citation of a catalogued archive: the death narrows the
    // combined register to its deaths, and nothing else of the event wins.
    let evidence =
        with("AD99 - Exampleville-sur-Example - NMD - 1911 - 2 E 9999 - acte 38 - vue 11g/52");
    for lookup in [None, Some(&places as &dyn PlaceLookup)] {
        let recognition = registry.recognize(&evidence, None, lookup).unwrap();
        assert_eq!(
            Parts::from(&recognition),
            at(AD99)
                .act("D")
                .locality("Exampleville-sur-Example")
                .year(1911)
                .call("2 E 9999")
                .number(38)
                .views("11g")
                .of(52)
        );
        for part in [Part::Archive, Part::Locality, Part::Act, Part::Year] {
            assert_eq!(recognition.signal(part), Some(Signal::Words), "{part:?}");
        }
    }
    // The event narrows a combined register to one of its own kinds only.
    for (event_type, act) in [
        (EventType::Birth, "N"),
        (EventType::Marriage, "M"),
        (EventType::Burial, "D"),
        (EventType::Occupation, "NMD"),
        (EventType::Baptism, "NMD"),
    ] {
        let evidence = CitationEvidence {
            event: Some(CitedEvent {
                event_type,
                ..elsewhere.clone()
            }),
            ..title("AD99, Exampleville, NMD 1911, vue 11")
        };
        let recognition = registry.recognize(&evidence, None, None).unwrap();
        assert_eq!(
            recognition.found.act.map(|act| act.to_string()).as_deref(),
            Some(act),
            "{event_type:?}"
        );
    }

    // With the embedded catalogue too: an uncatalogued department's code
    // never becomes the department where the death happened.
    let evidence = CitationEvidence {
        event: Some(death(
            1911,
            "Exampleton, 72181, Sarthe, Pays de la Loire, France",
        )),
        ..title("AD98 - Exampleville - NMD - 1911 - 2 E 9999 - acte 38 - vue 11g/52")
    };
    assert_eq!(
        ArchiveRegistry::embedded().recognize(&evidence, None, None),
        Err(Unrecognized::NoAdapter)
    );
}

#[test]
fn archives_of_the_embedded_catalogue_are_named_as_genealogists_write_them() {
    let registry = ArchiveRegistry::embedded();
    let cases = [
        (
            "Archives départementales de la Sarthe, état civil de Exampleville, naissances 1872, vue 45",
            at("fr-ad72")
                .act("N")
                .locality("Exampleville")
                .year(1872)
                .views("45"),
        ),
        (
            "AD 72, Exampleville, décès 1901",
            at("fr-ad72").act("D").locality("Exampleville").year(1901),
        ),
        (
            "AD72, Exampleville, BMS 1750",
            at("fr-ad72").act("BMS").locality("Exampleville").year(1750),
        ),
        (
            "ADLA, Exampleville, baptêmes 1702",
            at("fr-ad44").act("B").locality("Exampleville").year(1702),
        ),
        (
            "Archives départementales de la Loire-Inférieure, Exampleville, mariages 1850",
            at("fr-ad44").act("M").locality("Exampleville").year(1850),
        ),
        (
            "Arch. dép. Côte-d'Or, Exampleville, BMS 1750",
            at("fr-ad21").act("BMS").locality("Exampleville").year(1750),
        ),
        (
            "Archives d'Indre-et-Loire, Exampleville, sépultures 1720",
            at("fr-ad37").act("S").locality("Exampleville").year(1720),
        ),
        (
            "Arch. dép. du Pas-de-Calais, Exampleville, tables décennales 1883-1892",
            at("fr-ad62").act("TD").locality("Exampleville").year(1883),
        ),
        (
            "AD 37, Exampleville, naissances 1890",
            at("fr-ad37").act("N").locality("Exampleville").year(1890),
        ),
        (
            "Archives départementales de l'Ain, Exampleville, décès 1875",
            at("fr-ad01").act("D").locality("Exampleville").year(1875),
        ),
    ];
    assert_eq!(corpus(registry, &cases), 10);

    assert_eq!(
        registry.recognize(
            &title("AD98, état civil de Exampleville, naissances 1872"),
            None,
            None
        ),
        Err(Unrecognized::NoAdapter)
    );
    let evidence = CitationEvidence {
        event: Some(CitedEvent {
            event_type: EventType::Baptism,
            year: Some(1702),
            place: Some("Exampleville, 72181, Sarthe, Pays de la Loire, France".to_owned()),
            agency: None,
        }),
        ..title("Registres paroissiaux")
    };
    // The event's place designates no archive…
    assert_eq!(
        registry.recognize(&evidence, None, None),
        Err(Unrecognized::NotACitation)
    );
    // …but completes a citation that names one.
    let evidence = CitationEvidence {
        repositories: vec![held("Archives départementales de la Sarthe", None)],
        ..evidence
    };
    assert_eq!(
        recognized(registry, &evidence),
        at("fr-ad72").act("B").locality("Exampleville").year(1702)
    );
    let recognition = registry
        .recognize(
            &title("https://archives.sarthe.fr/archives-en-ligne/ark:/99999/a0000"),
            None,
            None,
        )
        .unwrap();
    assert_eq!(recognition.archive.id, "fr-ad72");
    assert!(recognition.address.is_some());
}

/// A language other than French, added as data: a test-only vocabulary and
/// a fictitious archive of a fictitious country.
#[test]
fn a_second_language_is_data() {
    let vocabulary = Vocabulary::parse(
        r#"{
            "language": "xx",
            "countries": ["ZZ"],
            "archives": { "regional": ["state archive"] },
            "acts": { "B": ["baptism"], "M": ["marriage"], "S": ["burial"] },
            "registers": { "parish": ["parish register"] },
            "views": ["image", "frame"],
            "view_counts": ["of"],
            "numbers": ["entry", "no."],
            "pages": ["p."],
            "places": ["of", "in"],
            "particles": ["upon", "on"],
            "ranges": ["to"]
        }"#,
    )
    .expect("a vocabulary");
    let archive = serde_json::json!({
        "id": "zz-sa-north",
        "country": "ZZ",
        "level": "regional",
        "name": "North Example State Archive",
        "areas": ["Northshire"],
        "citation_codes": ["NSA"],
        "website": "https://archive.north.example",
        "collections": [{
            "id": "parish-registers",
            "acts": ["B", "M", "S"],
            "platform": "scripted",
            "portal": { "outcome": "view" },
        }]
    })
    .to_string();
    let registry = ArchiveRegistry::new(&[("zz", archive.as_str())], vec![Box::new(Scripted)])
        .unwrap()
        .with_vocabularies(vec![vocabulary]);
    let cases = [
        (
            "State Archive of Northshire, Examplebury parish register, baptisms 1750 to 1760, image 45 of 200, entry 12",
            at("zz-sa-north")
                .act("B")
                .locality("Examplebury")
                .year(1750)
                .views("45")
                .of(200)
                .number(12),
        ),
        (
            "NSA; marriages in Newton-upon-Example, 1801; p. 4",
            at("zz-sa-north")
                .act("M")
                .locality("Newton-upon-Example")
                .year(1801)
                .folio("p. 4"),
        ),
    ];
    assert_eq!(corpus(&registry, &cases), 2);
    // French words mean nothing to it.
    let recognition = registry
        .recognize(&title("NSA, naissances 1872, vue 4"), None, None)
        .unwrap();
    assert_eq!(recognition.found.act, None);
    assert!(recognition.found.views.is_empty());
}

#[test]
fn reads_an_approximate_period_s_years() {
    let cases = [
        (
            "AD99 - Exampleville - TD - env 1792-1952 - 9NUM/8E99 - vue 5d/353",
            at(AD99)
                .act("TD")
                .locality("Exampleville")
                .year(1792)
                .call("9NUM/8E99")
                .views("5d")
                .of(353),
        ),
        (
            "AD99, Exampleville, BMS env. 1702, v. 5",
            at(AD99)
                .act("BMS")
                .locality("Exampleville")
                .year(1702)
                .views("5"),
        ),
        (
            "AD99, Exampleville, BMS Vers 1702",
            at(AD99).act("BMS").locality("Exampleville").year(1702),
        ),
        (
            "AD99, Exampleville, N ca 1880",
            at(AD99).act("N").locality("Exampleville").year(1880),
        ),
        (
            "AD99, Exampleville, N ~1880",
            at(AD99).act("N").locality("Exampleville").year(1880),
        ),
        // The word without a period is a name: a commune called so.
        (
            "AD99, Vers, BMS 1702",
            at(AD99).act("BMS").locality("Vers").year(1702),
        ),
    ];
    assert_eq!(corpus(&registry(), &cases), 6);
}

#[test]
fn reads_a_section_of_a_city_s_registers_as_its_parish() {
    let cases = [
        (
            "AD99 - Exampleville - D - 1893 - 3e section - 4 E 99993 - acte 520 - vue 72d/293",
            at(AD99)
                .act("D")
                .locality("Exampleville")
                .parish("3e section")
                .year(1893)
                .call("4 E 99993")
                .number(520)
                .views("72d")
                .of(293),
        ),
        (
            "AD99, Exampleville, D 1893, section 2, v. 7",
            at(AD99)
                .act("D")
                .locality("Exampleville")
                .parish("section 2")
                .year(1893)
                .views("7"),
        ),
        (
            "AD99, Exampleville, N 1893, 1re section",
            at(AD99)
                .act("N")
                .locality("Exampleville")
                .parish("1re section")
                .year(1893),
        ),
    ];
    assert_eq!(corpus(&registry(), &cases), 3);
}
