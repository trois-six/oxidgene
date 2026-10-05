//! The Arkothèque part of the live checks (Archive Portals §9.1).
//!
//! The search page names its engine and content components
//! (`data-moteur`, `data-contenu`); everything else the settings reference
//! is declared by the engine's bare answer, the one the page requests on
//! load, `/_recherche-api/moteur?refUnique=<engine>&<engine>--contenuIds[]=…`:
//! its `filtres` (each filter's reference and indexed field), its `restits`
//! (the display modes), and per field an aggregation of the values the
//! filter offers, `resultats.aggregations[0][<field>][<field>_terms]`, the
//! most frequent first. The localities and act values there carry their
//! record keys, `Name[[arko_fiche_…]]`.

use serde::Deserialize;
use serde_json::{Map, Value};

use super::{Arkotheque, Settings, page};
use crate::catalog::Collection;
use crate::citation::{Act, CitationParts};
use crate::live::{Failure, Probe, Register, Step};
use crate::platform::markup::fold;
use crate::platform::{BoxFuture, Query};
use crate::transport::PortalFetch;

/// What the live check reads of the engine's bare answer.
#[derive(Deserialize)]
struct EngineAnswer {
    filtres: Vec<Filter>,
    restits: Vec<Restit>,
    resultats: Aggregated,
}

#[derive(Deserialize)]
struct Filter {
    #[serde(rename = "refUnique")]
    reference: String,
    #[serde(default)]
    properties: Vec<Property>,
}

#[derive(Deserialize)]
struct Property {
    #[serde(rename = "fieldName")]
    field: String,
}

#[derive(Deserialize)]
struct Restit {
    #[serde(rename = "refUnique")]
    reference: String,
}

#[derive(Deserialize)]
struct Aggregated {
    #[serde(default)]
    aggregations: Vec<Map<String, Value>>,
}

impl EngineAnswer {
    /// The indexed field of a filter, when the engine has the filter.
    fn field(&self, reference: &str) -> Option<&str> {
        self.filtres
            .iter()
            .find(|filter| filter.reference == reference)
            .map(|filter| {
                filter
                    .properties
                    .first()
                    .map_or("", |property| property.field.as_str())
            })
    }

    /// The values a field's filter offers, the most frequent first, with their
    /// record keys.
    fn values(&self, field: &str) -> Vec<&str> {
        self.resultats
            .aggregations
            .iter()
            .find_map(|aggregation| aggregation.get(field))
            .and_then(|aggregation| aggregation[format!("{field}_terms")]["buckets"].as_array())
            .map(|buckets| {
                buckets
                    .iter()
                    .filter_map(|bucket| bucket["key"].as_str())
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// A filter value without its record key: `Exampleville` for
/// `Exampleville[[arko_fiche_…]]`.
fn without_key(value: &str) -> &str {
    value
        .split_once("[[")
        .map_or(value, |(name, _)| name)
        .trim()
}

impl Settings {
    /// The engine's bare answer, as the search page requests it on load.
    fn engine_request(&self) -> String {
        let mut query = Query::new();
        query.push("refUnique", self.engine.as_str());
        for id in &self.content_ids {
            query.push(format!("{}--contenuIds[]", self.engine), id.as_str());
        }
        format!("{}?{query}", super::SEARCH_PATH)
    }

    /// What the search page lacks of the settings' references.
    fn missing_from_page(&self, page: &str) -> Vec<String> {
        let mut missing = Vec::new();
        if !page.contains(&format!("data-moteur=\"{}\"", self.engine)) {
            missing.push("engine".to_owned());
        }
        for id in &self.content_ids {
            if !page.contains(&format!("data-contenu=\"{id}\"")) {
                missing.push(format!("content {id}"));
            }
        }
        missing
    }

    /// What the engine's answer lacks of the settings' references: filters,
    /// display mode, act values.
    fn missing_from_engine(&self, engine: &EngineAnswer) -> Vec<String> {
        let mut missing = Vec::new();
        let filters = [
            ("locality", Some(&self.fields.locality)),
            ("act", Some(&self.fields.act)),
            ("period", self.fields.period.as_ref()),
        ];
        for (role, reference) in filters {
            if let Some(reference) = reference
                && engine.field(reference).is_none()
            {
                missing.push(format!("{role} filter"));
            }
        }
        if !engine
            .restits
            .iter()
            .any(|restit| restit.reference == self.display_mode)
        {
            missing.push("display mode".to_owned());
        }
        let acts = engine
            .field(&self.fields.act)
            .map(|field| engine.values(field))
            .unwrap_or_default();
        for (code, value) in &self.acts {
            if !acts.contains(&value.as_str()) {
                missing.push(format!("act value of {code}"));
            }
        }
        missing
    }
}

fn settings(collection: &Collection, step: Step) -> Result<Settings, Failure> {
    Settings::read(collection)
        .map_err(|error| Failure::drift(step, "valid Arkothèque settings", error.to_string()))
}

async fn search_page(collection: &Collection, fetch: &dyn PortalFetch) -> Result<String, Failure> {
    let step = Step::SearchPage;
    let settings = settings(collection, step)?;
    let page = fetch
        .get(&settings.search_path)
        .await
        .map_err(|error| Failure::fetch(step, "the collection's search page", error))?;
    let missing = settings.missing_from_page(&page);
    if !missing.is_empty() {
        return Err(Failure::unreadable(
            step,
            "the engine and content references in the search page",
            &page,
            format!("missing: {}", missing.join(", ")),
        ));
    }

    let expected = "the engine's filters, display modes and aggregations";
    let answer = fetch
        .get(&settings.engine_request())
        .await
        .map_err(|error| Failure::fetch(step, expected, error))?;
    let engine: EngineAnswer = serde_json::from_str(&answer).map_err(|_| {
        Failure::unreadable(
            step,
            expected,
            &answer,
            "an answer without filtres, restits or resultats",
        )
    })?;
    let missing = settings.missing_from_engine(&engine);
    if !missing.is_empty() {
        return Err(Failure::drift(
            step,
            "the settings' references in the engine",
            format!("missing: {}", missing.join(", ")),
        ));
    }
    // The engine lists the most populated locality first, whose search is
    // the slowest; the alphabetical first is an ordinary one.
    engine
        .field(&settings.fields.locality)
        .and_then(|field| {
            engine
                .values(field)
                .into_iter()
                .map(without_key)
                .filter(|locality| !locality.is_empty())
                .min_by_key(|locality| fold(locality))
        })
        .map(|locality| settings.locality_style.cited(locality))
        .ok_or_else(|| {
            Failure::drift(
                step,
                "the localities of the locality filter",
                "no locality listed",
            )
        })
}

async fn registers(
    collection: &Collection,
    locality: &str,
    act: &Act,
    fetch: &dyn PortalFetch,
) -> Result<Vec<Register>, Failure> {
    let step = Step::Discovery;
    let settings = settings(collection, step)?;
    let search = CitationParts {
        code: String::new(),
        locality: locality.to_owned(),
        parish: None,
        act: act.clone(),
        year: None,
        period: None,
        call_number: None,
        views: Vec::new(),
        view_count: None,
    };
    let expected = "the result rows of the first listed locality";
    let answer = fetch
        .get(&settings.search_request(&settings.filters(&search)))
        .await
        .map_err(|error| Failure::fetch(step, expected, error))?;
    let rows = page::search_rows(&answer, &settings.cells)
        .map_err(|error| Failure::from_error(step, expected, &error))?;
    if rows.is_empty() {
        return Err(Failure::drift(step, expected, "no register"));
    }
    Ok(rows
        .into_iter()
        .map(|row| Register {
            locality: settings
                .locality_style
                .cited(row.locality.as_deref().unwrap_or_default()),
            call_number: row.call_number,
            period: row.period,
            images: row.images,
            address: row.payload.viewer,
        })
        .collect())
}

impl Probe for Arkotheque {
    fn search_page<'a>(
        &'a self,
        collection: &'a Collection,
        fetch: &'a dyn PortalFetch,
    ) -> BoxFuture<'a, Result<String, Failure>> {
        Box::pin(search_page(collection, fetch))
    }

    fn registers<'a>(
        &'a self,
        collection: &'a Collection,
        locality: &'a str,
        act: &'a Act,
        fetch: &'a dyn PortalFetch,
    ) -> BoxFuture<'a, Result<Vec<Register>, Failure>> {
        Box::pin(registers(collection, locality, act, fetch))
    }
}
