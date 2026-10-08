//! The Mnesys part of the live checks (Archive Portals §9.1).
//!
//! The search form, `/search/form/<form>`, lists every value its selects
//! offer in an `enhanced-select` element per input, `data-name` the input's
//! name without its `[]` and `data-options` a JSON list of labels: the
//! localities as the portal labels them (`Exampleville (Department,
//! France)`, `Bureau de Exampleville`), the acts, and on some forms the
//! years. A year or period input may also be a plain input.

use super::{Mnesys, Settings, named_by, page};
use crate::catalog::Collection;
use crate::citation::{Act, CitationParts};
use crate::live::{Failure, Probe, Register, Step};
use crate::platform::BoxFuture;
use crate::platform::markup::fold;
use crate::transport::PortalFetch;

/// The locality a label names, when the label follows one of the settings'
/// patterns: the longest text in place of `{locality}`.
fn named(settings: &Settings, label: &str) -> Option<String> {
    settings.locality_label.iter().find_map(|pattern| {
        named_by(pattern, label)
            .first()
            .map(|name| (*name).to_owned())
    })
}

fn settings(collection: &Collection, step: Step) -> Result<Settings, Failure> {
    Settings::read(collection)
        .map_err(|error| Failure::drift(step, "valid Mnesys settings", error.to_string()))
}

impl Settings {
    /// What the form lacks of the settings' inputs and act labels. A select
    /// must be named with its `[]`, a plain input without.
    fn missing_from_form(&self, form: &str) -> Vec<String> {
        let mut missing = Vec::new();
        // A locality input is a list, or a plain input matched as text.
        if let Some(field) = &self.fields.locality
            && (!page::declares(form, field)
                || (field.ends_with("[]")
                    && page::options(form, field).is_none_or(|labels| labels.is_empty())))
        {
            missing.push("locality list".to_owned());
        }
        if let Some(field) = &self.fields.act {
            let acts = page::options(form, field).unwrap_or_default();
            if acts.is_empty() || !page::declares(form, field) {
                missing.push("act list".to_owned());
            }
            for (code, labels) in &self.acts {
                if labels.iter().any(|label| !acts.contains(label)) {
                    missing.push(format!("act label of {code}"));
                }
            }
        }
        for (what, field) in [
            ("year input", &self.fields.year),
            ("period inputs", &self.fields.period_begin),
            ("period inputs", &self.fields.period_end),
        ] {
            if let Some(field) = field
                && !page::declares(form, field)
                && !missing.iter().any(|known| known == what)
            {
                missing.push(what.to_owned());
            }
        }
        missing
    }
}

async fn search_page(collection: &Collection, fetch: &dyn PortalFetch) -> Result<String, Failure> {
    let step = Step::SearchPage;
    let settings = settings(collection, step)?;
    let form = fetch
        .get(&settings.form_path())
        .await
        .map_err(|error| Failure::fetch(step, "the search form", error))?;
    let missing = settings.missing_from_form(&form);
    if !missing.is_empty() {
        return Err(Failure::unreadable(
            step,
            "the settings' inputs and act labels in the search form",
            &form,
            format!("missing: {}", missing.join(", ")),
        ));
    }
    // A form without a locality input searches the whole department: its
    // series are cited without a locality.
    let Some(field) = &settings.fields.locality else {
        return Ok(String::new());
    };
    let labels = match page::options(&form, field) {
        Some(labels) => labels,
        // A plain input lists nothing: the rows of a search without it
        // name the localities, as their context or title does.
        None => listed_by_rows(collection, &settings, fetch).await?,
    };
    labels
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

/// The collection, context entries and titles of the rows a search of the
/// collection's first document kind lists without a locality.
async fn listed_by_rows(
    collection: &Collection,
    settings: &Settings,
    fetch: &dyn PortalFetch,
) -> Result<Vec<String>, Failure> {
    let step = Step::SearchPage;
    let expected = "the rows of a search without a locality";
    let act = collection
        .acts
        .first()
        .ok_or_else(|| Failure::drift(step, "an act in the collection", "no act"))?;
    let search = citation_of(String::new(), act);
    let found = settings
        .search(&settings.filters(&search, &[]), fetch)
        .await
        .map_err(|error| Failure::from_error(step, expected, &error))?;
    Ok(found
        .rows
        .into_iter()
        .flat_map(|row| {
            row.collection
                .into_iter()
                .chain(row.context)
                .chain([row.title])
        })
        .collect())
}

/// The search a citation of `locality` and `act` without a year would send.
fn citation_of(locality: String, act: &Act) -> CitationParts {
    CitationParts {
        code: String::new(),
        locality,
        parish: None,
        act: act.clone(),
        year: None,
        period: None,
        call_number: None,
        number: None,
        views: Vec::new(),
        view_count: None,
        alternate_localities: Vec::new(),
    }
}

async fn registers(
    collection: &Collection,
    locality: &str,
    act: &Act,
    fetch: &dyn PortalFetch,
) -> Result<Vec<Register>, Failure> {
    let step = Step::Discovery;
    let settings = settings(collection, step)?;
    let search = citation_of(locality.to_owned(), act);
    let expected = "the result rows of the first listed locality";
    let labels = settings
        .labels(&search, fetch)
        .await
        .map_err(|error| Failure::from_error(step, expected, &error))?
        .ok_or_else(|| Failure::drift(step, expected, "no label of the locality"))?;
    let found = settings
        .search(&settings.filters(&search, &labels.sent), fetch)
        .await
        .map_err(|error| Failure::from_error(step, expected, &error))?;
    if found.rows.is_empty() {
        return Err(Failure::drift(step, expected, "no register"));
    }
    let forms = settings.locality_forms(&search);
    Ok(settings
        .candidates(found.rows, &search)
        .into_iter()
        .map(|row| Register {
            locality: row
                .locality
                .filter(|shown| !forms.contains(&fold(shown)))
                .unwrap_or_else(|| locality.to_owned()),
            call_number: row.call_number,
            period: row.period,
            images: row.images,
            address: row.payload.ark.map(|ark| ark.image("", &ark.first_image)),
            numbers: row.numbers,
        })
        .collect())
}

/// The image count of a register whose row shows none, from the viewer.
async fn images(
    collection: &Collection,
    register: &Register,
    fetch: &dyn PortalFetch,
) -> Result<Option<u16>, Failure> {
    let step = Step::Discovery;
    let settings = settings(collection, step)?;
    let Some(ark) = register.address.as_deref().and_then(page::ark) else {
        return Ok(None);
    };
    settings
        .viewer_count(&ark, fetch)
        .await
        .map(Some)
        .map_err(|error| Failure::from_error(step, "the viewer's image count", &error))
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

    fn images<'a>(
        &'a self,
        collection: &'a Collection,
        register: &'a Register,
        fetch: &'a dyn PortalFetch,
    ) -> BoxFuture<'a, Result<Option<u16>, Failure>> {
        Box::pin(images(collection, register, fetch))
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

    fn marne(index: usize) -> Settings {
        Settings::read(
            &ArchiveRegistry::embedded()
                .archive("AD51")
                .unwrap()
                .collections[index],
        )
        .unwrap()
    }

    #[test]
    fn reads_the_selects_of_the_form() {
        let settings = marne(0);
        assert!(settings.missing_from_form(FORM).is_empty());
        let without_year = FORM.replace("name=\"3-date\"", "name=\"other\"");
        let without_localities = without_year.replace("data-name=\"0-", "data-name=\"9-");
        assert_eq!(
            settings.missing_from_form(&without_localities),
            ["locality list", "year input"]
        );
        // A select named without its `[]`, which the portal refuses.
        let mut plain = marne(0);
        plain.fields.act = Some("4-controlledAccessPhysicalCharacteristic".to_owned());
        assert_eq!(plain.missing_from_form(FORM), ["act list"]);
    }

    #[test]
    fn names_the_localities_its_patterns_label() {
        let settings = marne(0);
        let named: Vec<_> = page::options(FORM, settings.fields.locality.as_ref().unwrap())
            .unwrap()
            .iter()
            .filter_map(|label| named(&settings, label))
            .collect();
        // The parish follows no pattern; a former commune follows the second.
        assert_eq!(named, ["Sampleton", "Bourg (Le)", "Exampleville"]);
        assert_eq!(settings.locality_style.cited("Bourg (Le)"), "Le Bourg");
    }
}
