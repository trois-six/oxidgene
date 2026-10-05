//! Reading the portal's configuration and the API's answers.

use serde::Deserialize;

use super::{Register, unexpected};
use crate::ResolveError;
use crate::platform::markup::fold;
use crate::platform::select::Candidate;

/// The API key in the portal's `/runtimeConfig.js`:
/// `apiKey: '<key>'`.
pub(super) fn api_key(config: &str) -> Result<String, ResolveError> {
    let after = config
        .split_once("apiKey")
        .and_then(|(_, rest)| rest.split_once(':'))
        .map(|(_, rest)| rest.trim_start())
        .ok_or_else(|| unexpected("the configuration lacks apiKey"))?;
    let quote = after
        .chars()
        .next()
        .filter(|quote| matches!(quote, '\'' | '"'))
        .ok_or_else(|| unexpected("the configuration lacks apiKey"))?;
    let key = after[1..]
        .split(quote)
        .next()
        .filter(|key| {
            !key.is_empty()
                && key.len() <= 128
                && key
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        })
        .ok_or_else(|| unexpected("the configuration's apiKey is malformed"))?;
    Ok(key.to_owned())
}

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

/// The facet value, as the filter writes it, of the locality the citation
/// names: the one whose label, folded, is one of the cited forms. The portal
/// writes `Mas-d’Exemple (Le)` where a citation writes `Le Mas-d'Exemple`,
/// and the apostrophe either way.
pub(super) fn locality(answer: &str, wanted: &[String]) -> Result<Option<String>, ResolveError> {
    let facets: Facets = serde_json::from_str(answer)
        .map_err(|_| unexpected("the locality answer lacks searchAggsMetaTag"))?;
    let wanted: Vec<String> = wanted.iter().map(|form| fold(form)).collect();
    Ok(facets
        .values
        .into_iter()
        .find(|facet| wanted.contains(&fold(&facet.label)))
        .map(|facet| facet.key))
}

#[derive(Deserialize)]
struct Answer {
    total: usize,
    #[serde(rename = "listResponseObject")]
    results: Vec<Stub>,
}

/// A register's manifest stub, of which the adapter keeps a few fields.
#[derive(Deserialize)]
struct Stub {
    /// The manifest's address.
    id: String,
    #[serde(rename = "prismCoteId", default)]
    call_number: Option<String>,
    #[serde(rename = "prismNbMedias", default)]
    images: Option<u32>,
    /// The years held, `1673-1681, 1686, 1692-1723`; absent on some stubs.
    #[serde(rename = "prismNavDateValue", default)]
    years: Option<String>,
    #[serde(rename = "prismNavDate", default)]
    bounds: Option<Bounds>,
    #[serde(rename = "listIndexationAgg", default)]
    indexation: Option<Vec<Index>>,
}

#[derive(Deserialize)]
struct Bounds {
    #[serde(rename = "greaterThanOrEqualTo", default)]
    first: Option<String>,
    #[serde(rename = "lessThanOrEqualTo", default)]
    last: Option<String>,
}

#[derive(Deserialize)]
struct Index {
    #[serde(rename = "tagLabel")]
    label: String,
    #[serde(rename = "tagArchivistiqueValues", default)]
    values: Option<Vec<String>>,
}

impl Stub {
    /// The period as the portal states it, or the stub's bounds.
    fn period(&self) -> Option<String> {
        if let Some(years) = self.years.as_deref().filter(|years| !years.is_empty()) {
            return Some(years.to_owned());
        }
        let year = |date: &Option<String>| date.as_deref()?.get(..4).map(str::to_owned);
        let bounds = self.bounds.as_ref()?;
        Some(format!("{}-{}", year(&bounds.first)?, year(&bounds.last)?))
    }

    fn parish(&self) -> Option<String> {
        let values = self
            .indexation
            .as_ref()?
            .iter()
            .find(|index| index.label == "Paroisse")?
            .values
            .as_ref()?;
        let parish = values.join(", ");
        (!parish.is_empty()).then_some(parish)
    }
}

/// The registers of a search answer, all in `locality` (the search filtered
/// by it), and the total the portal counts.
pub(super) fn registers(
    answer: &str,
    api: &str,
    locality: &str,
) -> Result<(Vec<Candidate<Register>>, usize), ResolveError> {
    let answer: Answer = serde_json::from_str(answer)
        .map_err(|_| unexpected("the search answer lacks total or listResponseObject"))?;
    let prefix = format!("{api}/iiif/presentation/v3/");
    let rows = answer
        .results
        .into_iter()
        .map(|stub| {
            let manifest_ok = stub.id.starts_with(&prefix)
                && stub.id.ends_with("/manifest")
                && !stub.id.contains(['?', '#', ' ']);
            if !manifest_ok {
                return Err(unexpected("a result's id is not a manifest address"));
            }
            Ok(Candidate {
                locality: Some(locality.to_owned()),
                period: stub.period(),
                parish: stub.parish(),
                call_number: stub
                    .call_number
                    .as_deref()
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(str::to_owned),
                act: None,
                images: stub.images.and_then(|count| u16::try_from(count).ok()),
                payload: Register { manifest: stub.id },
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((rows, answer.total))
}
