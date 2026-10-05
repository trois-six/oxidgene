//! The archive catalogue: one JSON document per archive service under
//! `assets/archives/<country>/`, embedded at build time and validated when
//! first loaded.
//!
//! Adding an archive whose portal runs an already supported platform is a
//! data change. Each collection's `portal` object belongs to its platform's
//! adapter, which validates it here, so a catalogue mistake fails the tests
//! instead of a reader's click.

use std::collections::HashSet;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::citation::{Act, CitationGrammar, CitationParts};
use crate::platform::Platform;

/// The administrative level of an archive service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    National,
    Regional,
    Departmental,
    Cantonal,
    Municipal,
    Other,
}

/// Where the archive's images may be shown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Display {
    /// In the portal's own viewer only.
    #[default]
    Portal,
    /// Also in OxidGene's viewer, over IIIF, with the attribution.
    Iiif,
}

/// One archive service.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Archive {
    /// Lowercase slug starting with the lowercase country code: `fr-ad44`.
    pub id: String,
    /// ISO 3166-1 alpha-2 code, matching the catalogue directory.
    pub country: String,
    pub level: Level,
    /// The archive's own name, shown verbatim.
    pub name: String,
    /// Official codes of the area served: department, commune, canton.
    #[serde(default)]
    pub jurisdiction: Vec<String>,
    /// The codes a citation of this archive may start with, such as `AD44`.
    pub citation_codes: Vec<String>,
    /// The archive's home page.
    pub website: String,
    /// The searchable collections of registers, in resolution order.
    #[serde(default)]
    pub collections: Vec<Collection>,
    #[serde(default)]
    pub display: Display,
    /// Credit required by the reuse terms, with `{call_number}` and `{view}`
    /// placeholders; never translated.
    #[serde(default)]
    pub attribution: Option<String>,
    /// Address of the reuse terms.
    #[serde(default)]
    pub terms: Option<String>,
    /// Adjustments of the citation grammar for this archive.
    #[serde(default)]
    pub citation: CitationGrammar,
    /// Whether the scheduled live checks visit this archive.
    #[serde(default = "enabled")]
    pub live_check: bool,
}

const fn enabled() -> bool {
    true
}

/// One searchable collection of an archive, with its own engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Collection {
    /// Slug unique within the archive: `parish-registers`, `civil-status`.
    pub id: String,
    /// The act kinds and table codes the collection holds.
    pub acts: Vec<Act>,
    /// The years the collection covers, when bounded.
    #[serde(default)]
    pub period: Option<Period>,
    /// The adapter that searches it.
    pub platform: String,
    /// The adapter's settings for this collection.
    pub portal: serde_json::Value,
}

/// `[first year, last year]`; either bound may be `null`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "[Option<u16>; 2]", into = "[Option<u16>; 2]")]
pub struct Period {
    pub first: Option<u16>,
    pub last: Option<u16>,
}

impl From<[Option<u16>; 2]> for Period {
    fn from([first, last]: [Option<u16>; 2]) -> Self {
        Self { first, last }
    }
}

impl From<Period> for [Option<u16>; 2] {
    fn from(period: Period) -> Self {
        [period.first, period.last]
    }
}

impl Period {
    pub fn contains(&self, year: u16) -> bool {
        self.first.is_none_or(|first| first <= year) && self.last.is_none_or(|last| year <= last)
    }
}

impl Collection {
    /// Whether the collection holds the act: every kind of a combined act
    /// (`BMS`), or the table code itself.
    pub fn holds(&self, act: &Act) -> bool {
        match act {
            Act::Register(kinds) => kinds
                .iter()
                .all(|kind| self.acts.iter().any(|held| held.kinds().contains(kind))),
            Act::Table(_) => self.acts.contains(act),
        }
    }

    /// Whether the collection may hold a register of `year`; a citation
    /// without a year fits every collection.
    pub fn covers(&self, year: Option<u16>) -> bool {
        self.period
            .zip(year)
            .is_none_or(|(period, year)| period.contains(year))
    }
}

impl Archive {
    /// The collections that may hold the cited register, in catalogue order:
    /// those holding its act whose period contains its year.
    pub fn collections_for<'a, 'c>(
        &'a self,
        citation: &'c CitationParts,
    ) -> impl Iterator<Item = &'a Collection> + use<'a, 'c> {
        self.collections.iter().filter(|collection| {
            collection.holds(&citation.act) && collection.covers(citation.year)
        })
    }

    /// Whether a collection holds the act, whatever the year: the condition
    /// for offering a citation as a link.
    pub fn holds(&self, act: &Act) -> bool {
        self.collections
            .iter()
            .any(|collection| collection.holds(act))
    }

    /// The attribution template filled with the call number and the views
    /// (`5`, `5-6`), when the archive has one.
    pub fn attribution_for(&self, call_number: Option<&str>, views: &[u16]) -> Option<String> {
        let views = match views {
            [] => String::new(),
            [only] => only.to_string(),
            [first, .., last] => format!("{first}-{last}"),
        };
        self.attribution.as_ref().map(|template| {
            template
                .replace("{call_number}", call_number.unwrap_or_default())
                .replace("{view}", &views)
        })
    }
}

/// A catalogue document the loader refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogError(String);

impl CatalogError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    fn within(self, context: &str) -> Self {
        Self(format!("{context}: {}", self.0))
    }
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CatalogError {}

/// The embedded documents, each with the directory it was found in.
pub(crate) const EMBEDDED: &[(&str, &str)] = include!(concat!(env!("OUT_DIR"), "/archives.rs"));

/// Reads and validates catalogue documents, given as `(directory, JSON)`.
pub(crate) fn load(
    documents: &[(&str, &str)],
    platforms: &[Box<dyn Platform>],
) -> Result<Vec<Archive>, CatalogError> {
    let mut archives = Vec::with_capacity(documents.len());
    let mut ids = HashSet::new();
    let mut codes = HashSet::new();
    for (directory, document) in documents {
        let archive: Archive = serde_json::from_str(document)
            .map_err(|error| CatalogError::new(error.to_string()).within(directory))?;
        validate(&archive, directory, platforms).map_err(|error| error.within(&archive.id))?;
        if !ids.insert(archive.id.clone()) {
            return Err(CatalogError::new("the id is used twice").within(&archive.id));
        }
        if let Some(code) = archive
            .citation_codes
            .iter()
            .find(|code| !codes.insert((*code).clone()))
        {
            return Err(
                CatalogError::new(format!("citation code {code} is used twice"))
                    .within(&archive.id),
            );
        }
        archives.push(archive);
    }
    Ok(archives)
}

fn is_slug(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn is_web_address(text: &str) -> bool {
    ["https://", "http://"].iter().any(|scheme| {
        text.strip_prefix(scheme)
            .is_some_and(|rest| !rest.is_empty() && !rest.starts_with('/'))
    })
}

fn validate(
    archive: &Archive,
    directory: &str,
    platforms: &[Box<dyn Platform>],
) -> Result<(), CatalogError> {
    validate_identity(archive, directory)?;
    validate_display(archive)?;
    archive.citation.validate().map_err(CatalogError::new)?;
    let mut collection_ids = HashSet::new();
    for collection in &archive.collections {
        if !collection_ids.insert(collection.id.as_str()) {
            return Err(CatalogError::new(format!(
                "collection `{}` is listed twice",
                collection.id
            )));
        }
        validate_collection(collection, platforms).map_err(|error| error.within(&collection.id))?;
        if archive.display == Display::Iiif && !admits_any_client(collection, platforms) {
            return Err(CatalogError::new(
                "an iiif archive's portal must answer any client, since the backend resolves its views",
            )
            .within(&collection.id));
        }
    }
    Ok(())
}

/// Whether a collection's portal answers any HTTP client, rather than only a
/// browser page that passes its challenge.
fn admits_any_client(collection: &Collection, platforms: &[Box<dyn Platform>]) -> bool {
    platforms
        .iter()
        .find(|platform| platform.id() == collection.platform)
        .and_then(|platform| platform.endpoint(collection))
        .is_some_and(|endpoint| endpoint.access == crate::platform::Access::Any)
}

fn validate_identity(archive: &Archive, directory: &str) -> Result<(), CatalogError> {
    let country = &archive.country;
    if country.len() != 2 || !country.bytes().all(|byte| byte.is_ascii_uppercase()) {
        return Err(CatalogError::new(
            "country must be an ISO 3166-1 alpha-2 code",
        ));
    }
    if directory != country.to_ascii_lowercase() {
        return Err(CatalogError::new(format!(
            "an {country} archive belongs in assets/archives/{}/",
            country.to_ascii_lowercase()
        )));
    }
    if !is_slug(&archive.id) || !archive.id.starts_with(&format!("{directory}-")) {
        return Err(CatalogError::new(
            "the id must be a lowercase slug starting with the lowercase country code",
        ));
    }
    if archive.name.trim().is_empty() || !is_web_address(&archive.website) {
        return Err(CatalogError::new("name and website are required"));
    }
    if archive
        .jurisdiction
        .iter()
        .any(|code| code.trim().is_empty())
    {
        return Err(CatalogError::new("jurisdiction codes must not be blank"));
    }
    let valid_code = |code: &String| {
        code.bytes().any(|byte| byte.is_ascii_uppercase())
            && crate::citation::code_of(code) == Some(code.as_str())
    };
    if archive.citation_codes.is_empty() || !archive.citation_codes.iter().all(valid_code) {
        return Err(CatalogError::new(
            "citation codes must be uppercase letters and digits",
        ));
    }
    Ok(())
}

fn validate_display(archive: &Archive) -> Result<(), CatalogError> {
    if let Some(terms) = &archive.terms
        && !is_web_address(terms)
    {
        return Err(CatalogError::new("terms must be a web address"));
    }
    if archive
        .attribution
        .as_ref()
        .is_some_and(|attribution| attribution.trim().is_empty())
    {
        return Err(CatalogError::new("the attribution must not be blank"));
    }
    if archive.display == Display::Iiif
        && (archive.attribution.is_none() || archive.terms.is_none())
    {
        return Err(CatalogError::new(
            "an iiif archive needs an attribution and its terms",
        ));
    }
    Ok(())
}

fn validate_collection(
    collection: &Collection,
    platforms: &[Box<dyn Platform>],
) -> Result<(), CatalogError> {
    if !is_slug(&collection.id) || collection.acts.is_empty() {
        return Err(CatalogError::new(
            "a collection needs a slug id and at least one act",
        ));
    }
    if let Some(Period {
        first: Some(first),
        last: Some(last),
    }) = collection.period
        && last < first
    {
        return Err(CatalogError::new("the period ends before it begins"));
    }
    let platform = platforms
        .iter()
        .find(|platform| platform.id() == collection.platform)
        .ok_or_else(|| {
            CatalogError::new(format!(
                "no adapter for the `{}` platform",
                collection.platform
            ))
        })?;
    platform.validate(collection)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::citation::ActKind;
    use crate::platform::builtin;

    /// Arkothèque settings, with every act code of the test documents.
    fn portal(search_path: &str) -> serde_json::Value {
        let act = |label: &str| format!("{label}[[arko_fiche_00000000000{}]]", label.len());
        serde_json::json!({
            "origin": "https://archives.example.org",
            "search_path": search_path,
            "engine": "arko_default_000000000001",
            "content_ids": ["1"],
            "display_mode": "arko_default_000000000002",
            "fields": { "locality": "arko_default_000000000003", "act": "arko_default_000000000004" },
            "acts": {
                "B": act("Baptêmes"), "M": act("Mariages"), "S": act("Sépultures"),
                "N": act("Naissances"), "D": act("Décès"), "TD": act("Tables décennales")
            },
            "cells": { "locality": "commune" }
        })
    }

    fn document() -> serde_json::Value {
        serde_json::json!({
            "id": "fr-ad00",
            "country": "FR",
            "level": "departmental",
            "name": "Archives départementales d'Exemple",
            "jurisdiction": ["00"],
            "citation_codes": ["AD00"],
            "website": "https://archives.example.org",
            "collections": [{
                "id": "parish-registers",
                "acts": ["B", "M", "S"],
                "period": [null, 1792],
                "platform": "arkotheque",
                "portal": portal("/registres")
            }, {
                "id": "civil-status",
                "acts": ["N", "M", "D", "TD"],
                "period": [1792, null],
                "platform": "arkotheque",
                "portal": portal("/etat-civil")
            }]
        })
    }

    fn load_one(
        directory: &str,
        document: &serde_json::Value,
    ) -> Result<Vec<Archive>, CatalogError> {
        let text = document.to_string();
        load(&[(directory, text.as_str())], &builtin())
    }

    fn rejected(change: impl FnOnce(&mut serde_json::Value)) -> CatalogError {
        let mut document = document();
        change(&mut document);
        load_one("fr", &document).expect_err("an invalid document")
    }

    #[test]
    fn the_embedded_catalogue_is_valid() {
        let archives = load(EMBEDDED, &builtin()).expect("a valid catalogue");
        let archive = archives
            .iter()
            .find(|archive| archive.citation_codes.iter().any(|code| code == "AD44"))
            .expect("the Loire-Atlantique archive");
        assert_eq!(archive.id, "fr-ad44");
        assert_eq!(archive.level, Level::Departmental);
        assert_eq!(archive.display, Display::Portal);
        assert!(archive.live_check);
        assert!(archive.holds(&Act::Register(vec![ActKind::Birth])));
        assert!(archive.holds(&Act::Register(vec![
            ActKind::Baptism,
            ActKind::Marriage,
            ActKind::Burial
        ])));
    }

    #[test]
    fn reads_a_document_with_its_defaults() {
        let archives = load_one("fr", &document()).expect("a valid document");
        let archive = &archives[0];
        assert_eq!(archive.display, Display::Portal);
        assert_eq!(archive.citation, CitationGrammar::default());
        assert!(archive.live_check);
        assert_eq!(
            archive.collections[0].period,
            Some(Period {
                first: None,
                last: Some(1792)
            })
        );
    }

    #[test]
    fn picks_collections_by_act_and_period_in_catalogue_order() {
        let archives = load_one("fr", &document()).unwrap();
        let archive = &archives[0];
        let parts = |title: &str| CitationParts::parse(title, &archive.citation).unwrap();
        let ids = |title: &str| {
            let parts = parts(title);
            archive
                .collections_for(&parts)
                .map(|collection| collection.id.clone())
                .collect::<Vec<_>>()
        };

        assert_eq!(
            ids("AD00 - Exampleville - (aucun) - B - 1750"),
            ["parish-registers"]
        );
        assert_eq!(
            ids("AD00 - Exampleville - (aucun) - N - 1850"),
            ["civil-status"]
        );
        // A marriage of 1792 may sit in either collection: order decides.
        assert_eq!(
            ids("AD00 - Exampleville - (aucun) - M - 1792"),
            ["parish-registers", "civil-status"]
        );
        // Without a year, every collection holding the act.
        assert_eq!(
            ids("AD00 - Exampleville - (aucun) - M - acte 3"),
            ["parish-registers", "civil-status"]
        );
        assert_eq!(
            ids("AD00 - Exampleville - (aucun) - BMS - 1750"),
            ["parish-registers"]
        );
        assert_eq!(
            ids("AD00 - Exampleville - (aucun) - TD - 1803"),
            ["civil-status"]
        );
        assert!(ids("AD00 - Exampleville - (aucun) - TB - 1803").is_empty());
        assert!(ids("AD00 - Exampleville - (aucun) - N - 1750").is_empty());
    }

    #[test]
    fn fills_the_attribution() {
        let mut document = document();
        document["attribution"] = "Archives d'Exemple, {call_number}, vue {view}".into();
        let archive = load_one("fr", &document).unwrap().remove(0);
        assert_eq!(
            archive.attribution_for(Some("3E1/2"), &[5, 6]).as_deref(),
            Some("Archives d'Exemple, 3E1/2, vue 5-6")
        );
        assert_eq!(
            archive.attribution_for(Some("3E1/2"), &[5]).as_deref(),
            Some("Archives d'Exemple, 3E1/2, vue 5")
        );
    }

    #[test]
    fn rejects_inconsistent_documents() {
        type Change = fn(&mut serde_json::Value);
        let cases: [(&str, Change); 12] = [
            ("country", |d| d["id"] = "ch-ad00".into()),
            ("slug", |d| d["id"] = "fr-AD00".into()),
            ("alpha-2", |d| d["country"] = "FRA".into()),
            ("citation codes", |d| {
                d["citation_codes"] = serde_json::json!(["ad00"])
            }),
            ("citation codes", |d| {
                d["citation_codes"] = serde_json::json!([])
            }),
            ("website", |d| d["website"] = "archives.example.org".into()),
            ("iiif", |d| d["display"] = "iiif".into()),
            ("listed twice", |d| {
                d["collections"][1]["id"] = "parish-registers".into()
            }),
            ("no adapter", |d| {
                d["collections"][0]["platform"] = "unknown".into()
            }),
            ("period", |d| {
                d["collections"][0]["period"] = serde_json::json!([1800, 1700])
            }),
            ("at least one act", |d| {
                d["collections"][0]["acts"] = serde_json::json!([])
            }),
            ("search_path", |d| {
                d["collections"][0]["portal"]["search_path"] = "registres".into();
            }),
        ];
        for (expected, change) in cases {
            let error = rejected(change).to_string();
            assert!(error.contains(expected), "{expected}: {error}");
        }
        assert!(
            rejected(|d| d["collections"][0]["acts"] = serde_json::json!(["X"]))
                .to_string()
                .contains("not an act code")
        );
        assert!(
            rejected(|d| d["unknown"] = true.into())
                .to_string()
                .contains("unknown field")
        );
    }

    #[test]
    fn rejects_an_archive_in_another_country_directory() {
        let error = load_one("ch", &document()).unwrap_err().to_string();
        assert!(error.contains("assets/archives/fr/"), "{error}");
    }

    #[test]
    fn rejects_repeated_ids_and_citation_codes() {
        let first = document().to_string();
        let mut second = document();
        second["citation_codes"] = serde_json::json!(["AD01"]);
        let second = second.to_string();
        let error = load(
            &[("fr", first.as_str()), ("fr", second.as_str())],
            &builtin(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("id is used twice"), "{error}");

        let mut second = document();
        second["id"] = "fr-ad01".into();
        let second = second.to_string();
        let error = load(
            &[("fr", first.as_str()), ("fr", second.as_str())],
            &builtin(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("AD00 is used twice"), "{error}");
    }

    #[test]
    fn an_iiif_archive_with_its_terms_is_accepted() {
        let mut document = document();
        document["display"] = "iiif".into();
        document["attribution"] = "Archives d'Exemple, {call_number}, vue {view}".into();
        document["terms"] = "https://archives.example.org/conditions".into();
        assert_eq!(load_one("fr", &document).unwrap()[0].display, Display::Iiif);
    }

    #[test]
    fn an_iiif_archive_answers_any_client() {
        let mut document = document();
        document["display"] = "iiif".into();
        document["attribution"] = "Archives d'Exemple, {call_number}, vue {view}".into();
        document["terms"] = "https://archives.example.org/conditions".into();
        document["collections"][1]["portal"]["transport"] = "browser".into();
        let error = load_one("fr", &document).unwrap_err().to_string();
        assert!(error.contains("must answer any client"), "{error}");
    }
}
