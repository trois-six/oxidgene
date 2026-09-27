//! Germany: every municipality of the Federal Statistical Office's register
//! (GV100AD) since its first annual edition of 1993, each under every Kreis
//! it was listed in, and the municipalities merged away before 1993 from
//! Wikidata.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::Read;

use anyhow::{Context, Result};

use crate::fetch::Fetcher;
use crate::place::{Country, Kind, Place, file, fold, unfiled};
use crate::table::decode_mixed;

use crate::wikidata::{FormerQuery, coordinates, former_municipalities};

const DESTATIS_URL: &str = "https://www.destatis.de";
/// The register's page, which links every edition.
const INDEX_PATH: &str = "/DE/Themen/Laender-Regionen/Regionales/Gemeindeverzeichnis/_inhalt.html";
/// Wikidata's property for the Amtlicher Gemeindeschlüssel.
const CODE_PROPERTY: &str = "P439";
const FORMER_INSTANCE: &str =
    "?item p:P31 ?statement . ?statement ps:P31 ?class . ?class wdt:P279* wd:Q262166 .";

pub async fn places(fetcher: &Fetcher) -> Result<Vec<Place>> {
    let editions = editions(fetcher).await?;
    let latest = editions
        .last()
        .context("no GV100AD edition was found")?
        .date
        .clone();
    eprintln!(
        "Germany: GV100AD {} editions, {} to {latest}",
        editions.len(),
        editions[0].date
    );

    // Each (code, name) with the dates it was listed on and the filings it
    // was listed under.
    let mut seen: HashMap<(String, String), Listing> = HashMap::new();
    for edition in &editions {
        for m in &edition.municipalities {
            let listing = seen.entry((m.code.clone(), m.name.clone())).or_default();
            listing.last.clone_from(&edition.date);
            listing.filings.insert((m.kreis.clone(), m.land.clone()));
        }
    }
    let live: HashMap<&str, (&str, &Municipality)> = editions
        .last()
        .map(|e| {
            e.municipalities
                .iter()
                .map(|m| (m.code.as_str(), (m.name.as_str(), m)))
                .collect()
        })
        .unwrap_or_default();
    // Live municipalities by name and Land, to recognise one whose code
    // changed with its Kreis. A name several municipalities of the Land bear
    // says nothing, and is left out.
    let mut named: HashMap<(&str, &str), Vec<&Municipality>> = HashMap::new();
    for (name, m) in live.values() {
        named.entry((*name, m.land.as_str())).or_default().push(m);
    }
    let live_by_name: HashMap<(&str, &str), &Municipality> = named
        .into_iter()
        .filter_map(|(key, found)| match found.as_slice() {
            [only] => Some((key, *only)),
            _ => None,
        })
        .collect();
    let next_edition = |date: &str| {
        editions
            .iter()
            .map(|e| e.date.as_str())
            .find(|d| *d > date)
            .map(str::to_string)
    };
    let centres = coordinates(fetcher, CODE_PROPERTY).await?;
    let former = former_municipalities(
        fetcher,
        &FormerQuery {
            country: "Q183",
            instance: FORMER_INSTANCE,
            code: CODE_PROPERTY,
            languages: &["de"],
            before_year: None,
            undated: false,
        },
    )
    .await?;
    // The register says a municipality is gone, not where it went; Wikidata
    // often knows, keyed by the municipality's own code.
    let merged_into: HashMap<&str, &Municipality> = former
        .iter()
        .filter_map(|f| {
            let (_, now) = f.codes.iter().find_map(|c| live.get(c.as_str()))?;
            Some((f.code.as_str(), *now))
        })
        .collect();

    let mut places = Vec::new();
    let mut entries: Vec<_> = seen.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    for ((code, name), listing) in entries {
        let is_live = listing.last == latest;
        let (kind, successor) = if is_live {
            (Kind::Commune, None)
        } else if let Some((_, now)) = live.get(code.as_str()) {
            (Kind::FormerName, Some(now))
        } else {
            let land = listing.filings.iter().next().map_or("", |f| f.1.as_str());
            match live_by_name.get(&(name.as_str(), land)) {
                Some(now) => (Kind::FormerName, Some(now)),
                None => (Kind::FormerCommune, merged_into.get(code.as_str())),
            }
        };
        // The editions are yearly: when a municipality first appears is
        // known to the year only, and is not recorded.
        let mut base = unfiled(Country::Germany, name, code, kind);
        if !is_live {
            // The first edition that no longer lists it: the change happened
            // in the year before, at the latest.
            base.valid_until = next_edition(&listing.last);
        }
        base.successor = successor.map(|m| m.code.clone());
        base.coordinates = centres
            .get(code)
            .or_else(|| successor.and_then(|m| centres.get(&m.code)))
            .copied();
        let today = if is_live {
            live.get(code.as_str()).map(|(_, m)| *m)
        } else {
            None
        };
        if let Some(m) = today {
            file(
                &mut places,
                &base,
                &subdivision(name, &m.kreis),
                &m.land,
                true,
            );
        }
        for (kreis, land) in &listing.filings {
            file(&mut places, &base, &subdivision(name, kreis), land, false);
        }
    }
    let register = places.len();

    // Before 1993, only Wikidata remembers them.
    let known: HashSet<(String, String)> = places
        .iter()
        .map(|p| (fold(&p.name), p.region_name().to_string()))
        .collect();
    let first_edition = editions[0].date.as_str();
    for commune in &former {
        if commune
            .end
            .as_deref()
            .is_none_or(|end| end >= first_edition)
        {
            continue;
        }
        let Some((_, now)) = commune.codes.iter().find_map(|c| live.get(c.as_str())) else {
            continue;
        };
        if known.contains(&(fold(&commune.name), now.land.clone())) {
            continue;
        }
        // An item that carries its successor's code has no code of its own.
        let own = if live.contains_key(commune.code.as_str()) {
            ""
        } else {
            &commune.code
        };
        let mut base = unfiled(Country::Germany, &commune.name, own, Kind::FormerCommune);
        base.valid_from.clone_from(&commune.start);
        base.valid_until.clone_from(&commune.end);
        base.successor = Some(now.code.clone());
        base.coordinates = commune.coordinates;
        file(
            &mut places,
            &base,
            &subdivision(&commune.name, &now.kreis),
            &now.land,
            false,
        );
    }
    eprintln!(
        "Germany: {register} rows from the register, {} from Wikidata",
        places.len() - register
    );
    Ok(places)
}

/// A kreisfreie Stadt is its own Kreis: the subdivision would repeat the name.
fn subdivision(name: &str, kreis: &str) -> String {
    if kreis == name {
        String::new()
    } else {
        kreis.to_string()
    }
}

#[derive(Default)]
struct Listing {
    last: String,
    filings: BTreeSet<(String, String)>,
}

struct Municipality {
    code: String,
    name: String,
    kreis: String,
    land: String,
}

struct Edition {
    /// `YYYY-MM-DD`, the register's Gebietsstand.
    date: String,
    municipalities: Vec<Municipality>,
}

/// Every edition the register's page links, oldest first: one per year since
/// 1993, and the monthly ones of the current year.
async fn editions(fetcher: &Fetcher) -> Result<Vec<Edition>> {
    let index = fetcher
        .bytes("gv100-index.html", &format!("{DESTATIS_URL}{INDEX_PATH}"))
        .await?;
    let index = String::from_utf8_lossy(&index);
    let pages: BTreeSet<&str> = links(&index, ".html")
        .filter(|l| l.contains("/GV100AD") && !l.contains("Aktuell"))
        .collect();
    let mut editions = Vec::new();
    for page in pages {
        let name = page
            .rsplit('/')
            .next()
            .unwrap_or(page)
            .trim_end_matches(".html");
        let html = fetcher
            .bytes(&format!("{name}.html"), &absolute(page))
            .await?;
        let html = String::from_utf8_lossy(&html);
        let Some(zip) = links(&html, ".zip").next() else {
            continue;
        };
        let archive = fetcher
            .bytes(
                &format!("{name}.zip"),
                &absolute(&zip.replace("&amp;", "&")),
            )
            .await?;
        editions.push(parse_edition(&archive).with_context(|| format!("cannot read {name}"))?);
    }
    editions.sort_by(|a, b| a.date.cmp(&b.date));
    editions.dedup_by(|a, b| a.date == b.date);
    Ok(editions)
}

fn absolute(link: &str) -> String {
    if link.starts_with("http") {
        link.to_string()
    } else {
        format!("{DESTATIS_URL}/{}", link.trim_start_matches('/'))
    }
}

/// The `href` targets in `html` containing `extension`.
fn links<'a>(html: &'a str, extension: &'a str) -> impl Iterator<Item = &'a str> {
    html.split("href=\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .filter(move |link| link.contains(extension))
}

fn parse_edition(archive: &[u8]) -> Result<Edition> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(archive))?;
    // The register is the largest text file: the archives also hold a
    // copyright notice and a read-me.
    let mut entry = None;
    for index in 0..archive.len() {
        let file = archive.by_index(index)?;
        let name = file.name().to_lowercase();
        if (name.ends_with(".txt") || name.ends_with(".asc"))
            && entry.as_ref().is_none_or(|(_, size)| file.size() > *size)
        {
            entry = Some((file.name().to_string(), file.size()));
        }
    }
    let (entry, _) = entry.context("the archive holds no register file")?;
    let mut bytes = Vec::new();
    archive.by_name(&entry)?.read_to_end(&mut bytes)?;
    parse_register(&decode_german(&bytes))
}

/// Some older editions are in the DOS code page 850, where "ü" is 0x81;
/// the others in Windows-1252 or UTF-8.
fn decode_german(bytes: &[u8]) -> String {
    let count = |set: &[u8]| bytes.iter().filter(|b| set.contains(b)).count();
    if std::str::from_utf8(bytes).is_ok()
        || count(&[0x81, 0x84, 0x94, 0xE1]) <= count(&[0xFC, 0xE4, 0xF6, 0xDF])
    {
        return decode_mixed(bytes);
    }
    bytes.iter().map(|&b| cp850(b)).collect()
}

fn cp850(byte: u8) -> char {
    match byte {
        0x00..=0x7F => char::from(byte),
        0x81 => 'ü',
        0x84 => 'ä',
        0x94 => 'ö',
        0x8E => 'Ä',
        0x99 => 'Ö',
        0x9A => 'Ü',
        0xE1 => 'ß',
        0x82 => 'é',
        0x83 => 'â',
        0x85 => 'à',
        0x87 => 'ç',
        0x88 => 'ê',
        0x89 => 'ë',
        0x8A => 'è',
        0x8B => 'ï',
        0x8C => 'î',
        0x93 => 'ô',
        0x96 => 'û',
        0x97 => 'ù',
        0xA0 => 'á',
        0xA1 => 'í',
        0xA2 => 'ó',
        0xA3 => 'ú',
        0xA4 => 'ñ',
        _ => '?',
    }
}

/// Reads the fixed-width records: type at 1–2, Gebietsstand at 3–10, the
/// area key from 11, the name at 23–72, the Textkennzeichen at 123–124.
/// Positions count characters: newer editions are UTF-8.
fn parse_register(text: &str) -> Result<Edition> {
    let mut date = None;
    let mut lands = HashMap::new();
    let mut kreise = HashMap::new();
    let mut rows = Vec::new();
    for line in text.lines() {
        let chars: Vec<char> = line.chars().collect();
        let field = |from: usize, to: usize| -> String {
            chars
                .get(from..to.min(chars.len()))
                .map(|c| c.iter().collect())
                .unwrap_or_default()
        };
        let kind = field(0, 2);
        if !["10", "40", "60"].contains(&kind.as_str()) {
            continue;
        }
        let stand = field(2, 10);
        if stand.len() != 8 || !stand.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        date.get_or_insert_with(|| format!("{}-{}-{}", &stand[..4], &stand[4..6], &stand[6..8]));
        let name = official_name(field(22, 72).trim());
        match kind.as_str() {
            "10" => {
                lands.insert(field(10, 12), name);
            }
            "40" => {
                kreise.insert(field(10, 15), name);
            }
            _ => {
                // 65 and 66 are unincorporated areas: forests and lakes.
                if matches!(field(122, 124).as_str(), "65" | "66") {
                    continue;
                }
                // The eight digits of the Gemeindeschlüssel come first; later
                // editions follow them with the four of the Gemeindeverband.
                let digits: String = field(10, 22).chars().filter(char::is_ascii_digit).collect();
                let Some(code) = digits.get(..8).map(str::to_string) else {
                    continue;
                };
                rows.push((code, name));
            }
        }
    }
    let municipalities = rows
        .into_iter()
        .map(|(code, name)| Municipality {
            kreis: kreise.get(&code[..5]).cloned().unwrap_or_default(),
            land: lands.get(&code[..2]).cloned().unwrap_or_default(),
            code,
            name,
        })
        .collect();
    Ok(Edition {
        date: date.context("the register has no Gebietsstand")?,
        municipalities,
    })
}

/// "Flensburg, Stadt" is Flensburg: what follows the comma is the kind of
/// municipality, not its name.
fn official_name(raw: &str) -> String {
    raw.split(", ").next().unwrap_or(raw).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_both_register_layouts() {
        let text = "\
101993123101          Land A\n\
401993123101001       Kreis A, Stadt\n\
601993123101001000    Stadt A, Stadt                                                                                    61\n\
6020260228010011230456Dorf B                                                                                              64\n\
6020260228010011240456Forst C                                                                                             66\n";
        let edition = parse_register(text).unwrap();
        assert_eq!(edition.date, "1993-12-31");
        let rows: Vec<_> = edition
            .municipalities
            .iter()
            .map(|m| {
                (
                    m.code.as_str(),
                    m.name.as_str(),
                    m.kreis.as_str(),
                    m.land.as_str(),
                )
            })
            .collect();
        assert_eq!(
            rows,
            [
                ("01001000", "Stadt A", "Kreis A", "Land A"),
                ("01001123", "Dorf B", "Kreis A", "Land A"),
            ]
        );
    }

    #[test]
    fn dos_editions_decode_their_umlauts() {
        assert_eq!(
            decode_german(b"Dorf B\x81ren Stra\xE1e"),
            "Dorf Büren Straße"
        );
        assert_eq!(decode_german(b"Dorf B\xFCren"), "Dorf Büren");
    }

    #[test]
    fn a_kreisfreie_stadt_is_not_its_own_subdivision() {
        assert_eq!(subdivision("Stadt A", "Stadt A"), "");
        assert_eq!(subdivision("Dorf B", "Kreis A"), "Kreis A");
    }
}
