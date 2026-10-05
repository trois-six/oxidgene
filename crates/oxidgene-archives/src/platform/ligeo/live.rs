//! The Ligeo part of the live checks (Archive Portals §9.1).
//!
//! The search page is the form (`arc_form_rech`) whose inputs the settings
//! name. Its locality input is backed by a thesaurus, declared in the page's
//! script, `new VT_Control("…",{…"str":"<thesaurus>",…"aform":"<search>",
//! "afield":"<input>_Index"})`, whose autocomplete answers a form-encoded
//! `POST <prefix>/xhr/gettheslist/<thesaurus>/0/<search>/<input>_Index` with
//! a list of labels starting with the letters sent (`Exampleville (Department,
//! France)`). The portals answer only past three letters; the check sends
//! [`PROBE`], with which many French locality names start.

use super::{Ligeo, Settings, page};
use crate::catalog::Collection;
use crate::citation::{Act, CitationParts};
use crate::live::{Failure, Probe, Register, Step};
use crate::platform::BoxFuture;
use crate::platform::markup::{self, fold};
use crate::platform::query::encode;
use crate::transport::{PortalFetch, PortalRequest};

/// The letters the locality lookup sends: `Saint-…` localities exist in every
/// French department.
const PROBE: &str = "Sai";

/// A member `"name":"value"` of the script's object that declares `field`.
fn control_member(page: &str, field: &str, name: &str) -> Option<String> {
    let at = page.find(&format!("\"afield\":\"{field}_Index\""))?;
    let start = page[..at].rfind('{')?;
    let end = at + page[at..].find('}')?;
    let object = &page[start..end];
    let marker = format!("\"{name}\":\"");
    let value = &object[object.find(&marker)? + marker.len()..];
    let value = &value[..value.find('"')?];
    (!value.is_empty()).then(|| value.replace("\\/", "/"))
}

/// The locality a thesaurus label names, when it names a locality rather
/// than a parish, a place within one, or a former commune.
fn named(label: &str) -> Option<String> {
    let label = label.trim();
    let folded = fold(label);
    if label.contains(" -- ") || folded.contains("lieu dit") || folded.contains("ancienne commune")
    {
        return None;
    }
    let name = label.split(" (").next()?.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

fn settings(collection: &Collection, step: Step) -> Result<Settings, Failure> {
    Settings::read(collection)
        .map_err(|error| Failure::drift(step, "valid Ligeo settings", error.to_string()))
}

impl Settings {
    /// What the search form lacks of the settings' inputs and act values.
    fn missing_from_form(&self, form: &str) -> Vec<String> {
        let has = |name: &str| form.contains(&format!("name=\"{name}\""));
        let mut missing = Vec::new();
        if !form.contains("id=\"arc_form_rech\"") {
            missing.push("search form".to_owned());
        }
        let fields = &self.fields;
        let inputs = [
            Some(&fields.locality),
            fields.act.as_ref(),
            fields.year_from.as_ref(),
            fields.year_to.as_ref(),
        ];
        for name in inputs.into_iter().flatten() {
            if !has(name) {
                missing.push(format!("input {name}"));
            }
        }
        for (code, filter) in &self.acts {
            let present = match filter {
                super::ActFilter::Value(value) => form.contains(&format!("value=\"{value}\"")),
                super::ActFilter::Params(params) => params.keys().all(|name| has(name)),
            };
            if !present {
                missing.push(format!("act filter of {code}"));
            }
        }
        missing
    }
}

async fn search_page(collection: &Collection, fetch: &dyn PortalFetch) -> Result<String, Failure> {
    let step = Step::SearchPage;
    let settings = settings(collection, step)?;
    let path = format!(
        "{}/recherche/{}/n:{}",
        settings.prefix, settings.search, settings.node
    );
    let form = fetch
        .get(&path)
        .await
        .map_err(|error| Failure::fetch(step, "the search form", error))?;
    let mut missing = settings.missing_from_form(&form);
    let locality = &settings.fields.locality;
    let thesaurus = control_member(&form, locality, "str");
    let search = control_member(&form, locality, "aform");
    if thesaurus.is_none() || search.is_none() {
        missing.push("the locality's thesaurus".to_owned());
    }
    let (Some(thesaurus), Some(search), true) = (thesaurus, search, missing.is_empty()) else {
        return Err(Failure::unreadable(
            step,
            "the settings' inputs and the locality thesaurus in the search form",
            &form,
            format!("missing: {}", missing.join(", ")),
        ));
    };

    let expected = "the locality thesaurus's labels";
    let lookup = PortalRequest::post(
        format!(
            "{}/xhr/gettheslist/{thesaurus}/0/{search}/{locality}_Index",
            settings.prefix
        ),
        "application/x-www-form-urlencoded",
        format!("{}={PROBE}", encode(locality)),
    );
    let labels = fetch
        .request(&lookup)
        .await
        .map_err(|error| Failure::fetch(step, expected, error))?;
    markup::split_after(&labels, "<li")
        .into_iter()
        .filter_map(|item| {
            let item = item.split_once('>')?.1;
            let item = item.split("</li>").next()?;
            named(&markup::strip_tags(item))
        })
        .min_by_key(|name| fold(name))
        .ok_or_else(|| Failure::unreadable(step, expected, &labels, "no locality"))
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
        number: None,
        views: Vec::new(),
        view_count: None,
    };
    let expected = "the result rows of the first listed locality";
    let answer = fetch
        .get(&settings.results_path(&settings.filters(&search)))
        .await
        .map_err(|error| Failure::fetch(step, expected, error))?;
    let found = page::results(&answer, &settings.columns)
        .map_err(|error| Failure::from_error(step, expected, &error))?;
    if found.rows.is_empty() {
        return Err(Failure::drift(step, expected, "no register"));
    }
    Ok(found
        .rows
        .into_iter()
        .map(|row| Register {
            locality: row.locality.unwrap_or_else(|| locality.to_owned()),
            call_number: row.call_number,
            period: row.period,
            images: row.images,
            address: row.payload.map(|register| register.viewer()),
            numbers: row.numbers,
        })
        .collect())
}

impl Probe for Ligeo {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_locality_thesaurus_from_the_page_script() {
        let page = r#"<script>new VT_Control("ArchivesRECHCommuneListe",{"id":"ArchivesRECHCommuneListe","url":"\/archive\/recherche\/etatcivil","name":"RECH_commune_Liste"});
            new VT_Control("ArchivesRECHCommune",{"id":"ArchivesRECHCommune","acurl":"","str":"Communesdexemple","autocomplete":true,"aform":"etatcivil","afield":"RECH_commune_Index"});</script>"#;
        assert_eq!(
            control_member(page, "RECH_commune", "str").as_deref(),
            Some("Communesdexemple")
        );
        assert_eq!(
            control_member(page, "RECH_commune", "aform").as_deref(),
            Some("etatcivil")
        );
        assert_eq!(control_member(page, "RECH_comm", "str"), None);
    }

    #[test]
    fn keeps_the_labels_that_name_a_locality() {
        assert_eq!(named("Saint-Exemple").as_deref(), Some("Saint-Exemple"));
        assert_eq!(
            named("Saint-Exemple (Department, France)").as_deref(),
            Some("Saint-Exemple")
        );
        assert_eq!(
            named("Saint-Exemple (commune ; Department, France)").as_deref(),
            Some("Saint-Exemple")
        );
        for label in [
            "Saint-Exemple (Department, France) -- Paroisse Saint-Pierre",
            "Saint-Exemple (Exampleville, Department, France ; lieu-dit)",
            "Saint-Exemple (Department, France ; ancienne commune)",
        ] {
            assert_eq!(named(label), None, "{label}");
        }
    }
}
