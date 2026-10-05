//! The Prismia Vision part of the live checks (Archive Portals §9.1).
//!
//! The portal publishes its API key in `/runtimeConfig.js`; the API's facet
//! endpoint lists the values of a filter, the localities and the acts, each
//! with the label the portal shows (`Mas-d’Exemple (Le)`) and the key the
//! filter takes.

use serde::Deserialize;
use serde_json::json;

use super::{CONFIG_PATH, Prismia, Settings, page};
use crate::catalog::Collection;
use crate::citation::{Act, CitationParts};
use crate::live::{Failure, Probe, Register, Step};
use crate::platform::BoxFuture;
use crate::platform::locality::LocalityStyle;
use crate::platform::markup::fold;
use crate::transport::PortalFetch;

/// The facet values one listing asks for.
const LISTED: usize = 100;

#[derive(Deserialize)]
struct Facets {
    #[serde(rename = "searchAggsMetaTag")]
    values: Vec<Facet>,
}

#[derive(Deserialize)]
struct Facet {
    key: String,
    label: String,
}

fn settings(collection: &Collection, step: Step) -> Result<Settings, Failure> {
    Settings::read(collection)
        .map_err(|error| Failure::drift(step, "valid Prismia settings", error.to_string()))
}

/// The portal's API key, from its published configuration.
async fn key(fetch: &dyn PortalFetch, step: Step) -> Result<String, Failure> {
    let expected = "the API key of the portal's configuration";
    let config = fetch
        .get(CONFIG_PATH)
        .await
        .map_err(|error| Failure::fetch(step, expected, error))?;
    page::api_key(&config).map_err(|error| Failure::from_error(step, expected, &error))
}

impl Settings {
    /// The values a filter offers, alphabetically.
    async fn facets(
        &self,
        fetch: &dyn PortalFetch,
        key: &str,
        tag: &str,
        filter: &str,
    ) -> Result<Vec<Facet>, Failure> {
        let step = Step::SearchPage;
        let expected = "the values of the instrument's filters";
        let request = self.api_request(
            "/presentation/v1/facet/getFacetValues",
            key,
            &json!({
                "prismPathOrId": self.paths,
                "aggregateTag": tag,
                "aggregateValue": [filter],
                "text": "",
                "size": LISTED,
                "sortAlpha": true,
            }),
        );
        let answer = fetch
            .request(&request)
            .await
            .map_err(|error| Failure::fetch(step, expected, error))?;
        serde_json::from_str::<Facets>(&answer)
            .map(|facets| facets.values)
            .map_err(|_| Failure::unreadable(step, expected, &answer, "no searchAggsMetaTag"))
    }
}

async fn search_page(collection: &Collection, fetch: &dyn PortalFetch) -> Result<String, Failure> {
    let step = Step::SearchPage;
    let settings = settings(collection, step)?;
    let key = key(fetch, step).await?;
    let acts = settings
        .facets(fetch, &key, "ExtraField", &settings.filters.act)
        .await?;
    let missing: Vec<String> = settings
        .acts
        .iter()
        .filter(|(_, value)| !acts.iter().any(|act| act.key == **value))
        .map(|(code, _)| format!("act value of {code}"))
        .collect();
    if !missing.is_empty() {
        return Err(Failure::drift(
            step,
            "the settings' act values among the act filter's",
            format!("missing: {}", missing.join(", ")),
        ));
    }
    settings
        .facets(fetch, &key, "Lieux", &settings.filters.locality)
        .await?
        .into_iter()
        .map(|facet| facet.label)
        .filter(|label| !label.trim().is_empty())
        .min_by_key(|label| fold(label))
        // The portal writes a leading article behind the name.
        .map(|label| LocalityStyle::ArticleSuffix.cited(&label))
        .ok_or_else(|| Failure::drift(step, "the localities of the locality filter", "none"))
}

async fn registers(
    collection: &Collection,
    locality: &str,
    act: &Act,
    fetch: &dyn PortalFetch,
) -> Result<Vec<Register>, Failure> {
    let step = Step::Discovery;
    let settings = settings(collection, step)?;
    let key = key(fetch, step).await?;
    let search = CitationParts {
        code: String::new(),
        locality: locality.to_owned(),
        parish: None,
        act: act.clone(),
        year: None,
        period: None,
        call_number: None,
        number: None,
        views: Vec::new(),
        view_count: None,
    };
    let expected = "the registers of the first listed locality";
    let facets = fetch
        .request(&settings.locality_request(&key, &search))
        .await
        .map_err(|error| Failure::fetch(step, expected, error))?;
    let wanted = crate::platform::locality::forms(locality);
    let value = page::locality(&facets, &wanted)
        .map_err(|error| Failure::from_error(step, expected, &error))?
        .ok_or_else(|| Failure::drift(step, expected, "the locality's filter value"))?;
    let answer = fetch
        .request(&settings.search_request(&key, &value, &search))
        .await
        .map_err(|error| Failure::fetch(step, expected, error))?;
    let (rows, _) = page::registers(&answer, &settings.api, &value)
        .map_err(|error| Failure::from_error(step, expected, &error))?;
    if rows.is_empty() {
        return Err(Failure::drift(step, expected, "no register"));
    }
    Ok(rows
        .into_iter()
        .map(|row| Register {
            locality: locality.to_owned(),
            call_number: row.call_number,
            period: row.period,
            images: row.images,
            address: Some(row.payload.manifest),
            numbers: row.numbers,
        })
        .collect())
}

impl Probe for Prismia {
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
