//! The Mnesys part of the live checks (Archive Portals §9.1).
//!
//! The search form, `/search/form/<form>`, lists every value its selects
//! offer in an `enhanced-select` element per input, `data-name` the input's
//! name without its `[]` and `data-options` a JSON list of labels:
//! the localities as the portal labels them (`Exampleville (Department,
//! France)`), and the acts. The year is a plain input.

use super::{Mnesys, RESULTS_PER_PAGE, Settings, page};
use crate::catalog::Collection;
use crate::citation::{Act, CitationParts};
use crate::live::{Failure, Probe, Register, Step};
use crate::platform::BoxFuture;
use crate::platform::markup::{self, fold};
use crate::transport::PortalFetch;

/// The labels a select of the form offers, by the input's name.
fn options(form: &str, name: &str) -> Vec<String> {
    let marker = format!("data-name=\"{}\"", name.trim_end_matches("[]"));
    let Some(at) = form.find(&marker) else {
        return Vec::new();
    };
    // The element's own attributes, up to the end of its opening tag.
    let start = form[..at].rfind('<').unwrap_or(at);
    let tag = &form[start..];
    let tag = &tag[..tag.find('>').unwrap_or(tag.len())];
    markup::attribute(tag, "data-options")
        .and_then(|options| serde_json::from_str(&options).ok())
        .unwrap_or_default()
}

/// The locality a label names, when the label follows one of the settings'
/// patterns.
fn named(settings: &Settings, label: &str) -> Option<String> {
    settings.locality_label.iter().find_map(|pattern| {
        let (before, after) = pattern.split_once("{locality}")?;
        let name = label.strip_prefix(before)?.strip_suffix(after)?;
        (!name.is_empty()).then(|| name.to_owned())
    })
}

fn settings(collection: &Collection, step: Step) -> Result<Settings, Failure> {
    Settings::read(collection)
        .map_err(|error| Failure::drift(step, "valid Mnesys settings", error.to_string()))
}

impl Settings {
    /// What the form lacks of the settings' inputs and act labels.
    fn missing_from_form(&self, form: &str, localities: &[String]) -> Vec<String> {
        let mut missing = Vec::new();
        if localities.is_empty() {
            missing.push("locality list".to_owned());
        }
        let acts = options(form, &self.fields.act);
        if acts.is_empty() {
            missing.push("act list".to_owned());
        }
        if !form.contains(&format!("name=\"{}\"", self.fields.year)) {
            missing.push("year input".to_owned());
        }
        for (code, labels) in &self.acts {
            if labels.iter().any(|label| !acts.contains(label)) {
                missing.push(format!("act label of {code}"));
            }
        }
        missing
    }
}

async fn search_page(collection: &Collection, fetch: &dyn PortalFetch) -> Result<String, Failure> {
    let step = Step::SearchPage;
    let settings = settings(collection, step)?;
    let form = fetch
        .get(&format!("/search/form/{}", settings.form))
        .await
        .map_err(|error| Failure::fetch(step, "the search form", error))?;
    let localities = options(&form, &settings.fields.locality);
    let missing = settings.missing_from_form(&form, &localities);
    if !missing.is_empty() {
        return Err(Failure::unreadable(
            step,
            "the settings' inputs and act labels in the search form",
            &form,
            format!("missing: {}", missing.join(", ")),
        ));
    }
    localities
        .iter()
        .filter_map(|label| named(&settings, label))
        .min_by_key(|name| fold(name))
        .map(|name| settings.locality_style.cited(&name))
        .ok_or_else(|| {
            Failure::drift(
                step,
                "a locality labelled as the settings' patterns",
                "none",
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
    let found = page::results(&answer, RESULTS_PER_PAGE)
        .map_err(|error| Failure::from_error(step, expected, &error))?;
    if found.rows.is_empty() {
        return Err(Failure::drift(step, expected, "no register"));
    }
    let forms = [fold(locality), fold(&settings.locality(&search))];
    Ok(settings
        .candidates(found.rows, &search, &forms)
        .into_iter()
        .map(|row| Register {
            locality: row
                .locality
                .filter(|shown| !forms.contains(&fold(shown)))
                .unwrap_or_else(|| locality.to_owned()),
            call_number: row.call_number,
            period: row.period,
            images: row.images,
            address: Some(row.payload.ark.first_image),
        })
        .collect())
}

impl Probe for Mnesys {
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
    use crate::ArchiveRegistry;

    /// A search form with the select markup of the portals and fictitious
    /// labels.
    const FORM: &str = r#"<form><div                     class="enhanced-select multiselect"
            data-input-id="0-controlledAccessGeographicName"
            data-name="0-controlledAccessGeographicName"
            data-options="[&quot;Sampleton (Marne, France)&quot;,&quot;Bourg (Le) (Marne, France)&quot;,&quot;Exampleville (Marne ; ancienne commune)&quot;,&quot;Saint-Exemple, paroisse de (Sampleton, Marne, France)&quot;]"></div>
        <div class="enhanced-select multiselect" data-name="4-controlledAccessPhysicalCharacteristic"
            data-options="[&quot;baptêmes - naissances&quot;,&quot;mariages&quot;,&quot;sépultures - décès&quot;,&quot;tables décennales&quot;]"></div>
        <input type="number" id="3-date" name="3-date"></form>"#;

    fn marne() -> Settings {
        Settings::read(
            &ArchiveRegistry::embedded()
                .archive("AD51")
                .unwrap()
                .collections[0],
        )
        .unwrap()
    }

    #[test]
    fn reads_the_selects_of_the_form() {
        let settings = marne();
        let localities = options(FORM, &settings.fields.locality);
        assert_eq!(localities.len(), 4);
        assert_eq!(
            options(FORM, &settings.fields.act),
            [
                "baptêmes - naissances",
                "mariages",
                "sépultures - décès",
                "tables décennales"
            ]
        );
        assert!(settings.missing_from_form(FORM, &localities).is_empty());
        let without_year = FORM.replace("name=\"3-date\"", "name=\"other\"");
        assert_eq!(
            settings.missing_from_form(&without_year, &[]),
            ["locality list", "year input"]
        );
    }

    #[test]
    fn names_the_localities_its_patterns_label() {
        let settings = marne();
        let named: Vec<_> = options(FORM, &settings.fields.locality)
            .iter()
            .filter_map(|label| named(&settings, label))
            .collect();
        // The parish follows no pattern; a former commune follows the second.
        assert_eq!(named, ["Sampleton", "Bourg (Le)", "Exampleville"]);
        assert_eq!(settings.locality_style.cited("Bourg (Le)"), "Le Bourg");
    }
}
