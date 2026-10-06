//! The Ligeo part of the live checks (Archive Portals §9.1).
//!
//! The search page is the form (`arc_form_rech`) whose inputs the settings
//! name, or the finding aid's page for a search within one. The localities
//! to probe come from what backs the locality input, the first of:
//!
//! - a thesaurus declared in the page's script, `new
//!   VT_Control("…",{…"str":"<thesaurus>",…"aform":"<search>",
//!   "afield":"<input>_Index"})`, whose autocomplete answers a form-encoded
//!   `POST <prefix>/xhr/gettheslist/<thesaurus>/0/<search>/<input>_Index`
//!   with the labels starting with the letters sent (`Exampleville
//!   (Department, France)`);
//! - a typed facet, `new VT_Control("…",{…"url":"/arcfacette.php?…&id=
//!   <input>&autoc=1"…})`, whose answer to that address and `&<input>=`
//!   letters lists the matching values as `button.facette-select`;
//! - the input's own `<option>`s or checkboxes (the bureaux of a military
//!   search);
//! - for a finding aid, the branches of its tree (`title="Détail de la
//!   branche <locality>"`).
//!
//! The autocompletes answer only past three letters; the check sends
//! [`PROBE`], with which many French locality names start, and a plain
//! text input nothing backs is sent the letters themselves, which the portal
//! matches as text. A search keyed by year alone has no locality to probe.

use super::Ligeo;
use super::page;
use super::settings::{ActFilter, Settings};
use crate::catalog::Collection;
use crate::citation::{Act, CitationParts};
use crate::live::{Failure, Outcome, Probe, Register, Step};
use crate::platform::BoxFuture;
use crate::platform::markup::{self, attribute, fold};
use crate::platform::query::encode;
use crate::platform::select::period_ranges;
use crate::transport::{PortalFetch, PortalRequest};

/// The letters the locality lookup sends: `Saint-…` localities exist in every
/// French department.
const PROBE: &str = "Sai";

/// The object of the page script's `VT_Control` declarations whose member
/// `member` has a value `matches` accepts.
fn control_with(page: &str, member: &str, matches: impl Fn(&str) -> bool) -> Option<String> {
    page.split("new VT_Control(").skip(1).find_map(|control| {
        let object = control.split("});").next()?;
        let value = object
            .split(&format!("\"{member}\":\""))
            .nth(1)?
            .split('"')
            .next()?
            .replace("\\/", "/");
        matches(&value).then(|| object.to_owned())
    })
}

/// A member `"name":"value"` of the script's object that declares `field`.
fn control_member(page: &str, field: &str, name: &str) -> Option<String> {
    let afield = format!("{field}_Index");
    let object = control_with(page, "afield", |value| value == afield)?;
    let marker = format!("\"{name}\":\"");
    let value = &object[object.find(&marker)? + marker.len()..];
    let value = &value[..value.find('"')?];
    (!value.is_empty()).then(|| value.replace("\\/", "/"))
}

/// The address of the typed facet backing `field`'s autocomplete.
fn facet_url(page: &str, field: &str) -> Option<String> {
    let marker = format!("&id={field}&autoc=1");
    let object = control_with(page, "url", |value| value.contains(&marker))?;
    let marker = "\"url\":\"";
    let value = &object[object.find(marker)? + marker.len()..];
    let url = value[..value.find('"')?].replace("\\/", "/");
    url.starts_with("/arcfacette.php?").then_some(url)
}

/// The values a facet's answer lists for `field`, the answer listing the
/// form's other facets too.
fn facet_values(answer: &str, field: &str) -> Vec<String> {
    let marker = format!(
        "class=\"facette-select facette-select-{}\"",
        field.to_ascii_lowercase()
    );
    markup::split_after(answer, &marker)
        .into_iter()
        .filter_map(|button| attribute(button.split('>').next()?, "value"))
        .map(|value| value.replace("\\'", "'"))
        .collect()
}

/// The labels qualified by a value the settings' own inputs send, when
/// some are: a portal two archives share qualifies its localities by the
/// department its `params` filter (`Exampleville (Exampledept, France)`).
fn of_settings(settings: &Settings, labels: Vec<String>) -> Vec<String> {
    let qualifies = |label: &String| {
        label.split_once(" (").is_some_and(|(_, qualifier)| {
            settings.params.values().any(|value| {
                qualifier
                    .split([',', ';', ')'])
                    .any(|part| part.trim() == value)
            })
        })
    };
    if labels.iter().any(qualifies) {
        labels.into_iter().filter(qualifies).collect()
    } else {
        labels
    }
}

/// The values the form offers `field`: its options, or its checkboxes'
/// values, without the empty and `0` placeholders.
fn offered(form: &str, field: &str) -> Vec<String> {
    let marker = format!("name=\"{field}\"");
    let mut values = Vec::new();
    for (at, _) in form.match_indices(&marker) {
        let tag_start = form[..at].rfind('<').unwrap_or(at);
        let tag = &form[tag_start..];
        if tag.starts_with("<select") {
            let options = tag.split("</select>").next().unwrap_or(tag);
            values.extend(
                markup::split_after(options, "<option")
                    .into_iter()
                    .filter_map(|option| attribute(option.split('>').next()?, "value")),
            );
        } else if let Some(value) = attribute(tag.split('>').next().unwrap_or(tag), "value") {
            values.push(value);
        }
    }
    values.retain(|value| !value.trim().is_empty() && value != "0");
    values
}

/// The localities a finding aid's tree lists.
fn branches(page: &str) -> Vec<String> {
    markup::split_after(page, "title=\"Détail de la branche ")
        .into_iter()
        .filter_map(|title| Some(markup::decode_entities(title.split('"').next()?.trim())))
        .collect()
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
        if self.fonds.is_none() && !form.contains("id=\"arc_form_rech\"") {
            missing.push("search form".to_owned());
        }
        let inputs = self.fields.all().chain(self.params.keys());
        for name in inputs {
            if !has(name) {
                missing.push(format!("input {name}"));
            }
        }
        // A choice list must offer the act's value; a text input backed by a
        // thesaurus or a facet takes any.
        let offered_acts = self
            .fields
            .act
            .as_deref()
            .map(|act| offered(form, act))
            .unwrap_or_default();
        for (code, filter) in &self.acts {
            let present = match filter {
                ActFilter::Value(value) => {
                    offered_acts.is_empty() || offered_acts.iter().any(|offer| offer == value)
                }
                ActFilter::Params(params) => params.keys().all(|name| has(name)),
            };
            if !present {
                missing.push(format!("act filter of {code}"));
            }
        }
        missing
    }
}

/// The localities the portal lists for the locality input, from whichever
/// source backs it; `None` for a plain text input that nothing backs.
async fn localities(
    settings: &Settings,
    form: &str,
    fetch: &dyn PortalFetch,
) -> Result<Option<Vec<String>>, Failure> {
    let step = Step::SearchPage;
    let Some(locality) = &settings.fields.locality else {
        return Ok(None);
    };
    let expected = "the localities backing the locality input";
    let (thesaurus, search) = (
        control_member(form, locality, "str"),
        control_member(form, locality, "aform"),
    );
    if let (Some(thesaurus), Some(search)) = (thesaurus, search) {
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
        return Ok(Some(
            markup::split_after(&labels, "<li")
                .into_iter()
                .filter_map(|item| {
                    let item = item.split_once('>')?.1;
                    named(&markup::strip_tags(item.split("</li>").next()?))
                })
                .collect(),
        ));
    }
    if let Some(url) = facet_url(form, locality) {
        // A short list, such as recruitment bureaux, may hold no name
        // starting with the probe's letters: its most frequent values stand
        // in.
        for letters in [PROBE, ""] {
            let answer = fetch
                .get(&format!("{url}&{}={letters}", encode(locality)))
                .await
                .map_err(|error| Failure::fetch(step, expected, error))?;
            let values: Vec<String> = of_settings(settings, facet_values(&answer, locality))
                .iter()
                .filter_map(|value| named(value))
                .collect();
            if !values.is_empty() {
                return Ok(Some(values));
            }
        }
        return Ok(Some(Vec::new()));
    }
    if settings.fonds.is_some() {
        return Ok(Some(branches(form)));
    }
    // The values are sent as offered. A place's name starts with a capital:
    // `liste nominative` among a form's bureaux is a kind of register, unless
    // every value is written so (`subdivision de …`).
    let mut offered = offered(form, locality);
    if offered
        .iter()
        .any(|value| !value.starts_with(char::is_lowercase))
    {
        offered.retain(|value| !value.starts_with(char::is_lowercase));
    }
    Ok((!offered.is_empty()).then_some(offered))
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
            "the settings' inputs in the search form",
            &form,
            format!("missing: {}", missing.join(", ")),
        ));
    }
    // A locality typed in a plain input is probed with the letters, which
    // the portal matches as text; a search by year alone with none.
    let Some(localities) = localities(&settings, &form, fetch).await? else {
        let probe = settings.fields.locality.as_ref().map_or("", |_| PROBE);
        return Ok(probe.to_owned());
    };
    localities
        .into_iter()
        .min_by_key(|name| fold(name))
        .ok_or_else(|| {
            Failure::unreadable(
                step,
                "the localities backing the locality input",
                &form,
                "no locality",
            )
        })
}

/// The rows a search of `locality` and `act` lists, without a year.
async fn search(
    settings: &Settings,
    locality: &str,
    act: &Act,
    fetch: &dyn PortalFetch,
) -> Result<page::Found, Failure> {
    let step = Step::Discovery;
    let search = CitationParts {
        code: String::new(),
        locality: locality.to_owned(),
        parish: None,
        act: act.clone(),
        year: None,
        period: None,
        call_number: None,
        // An index of persons lists every person of the locality: the
        // first matricule of each class is enough to cite one, and its
        // search is quick.
        number: settings.fields.number.as_ref().map(|_| 1),
        views: Vec::new(),
        view_count: None,
    };
    let expected = "the result rows of the first listed locality";
    let answer = fetch
        .get(&settings.results_path(&settings.filters(&search)))
        .await
        .map_err(|error| Failure::fetch(step, expected, error))?;
    page::results(&answer, &settings.columns)
        .map_err(|error| Failure::from_error(step, expected, &error))
}

/// Whether a listed row, whose act reads as the code `read`, is one of the
/// searched act's: not a table for a register of acts, and not another
/// series for a series; a row whose act reads as nothing is.
fn is_of_act(read: Option<&str>, act: &Act) -> bool {
    let read = read.and_then(Act::from_code);
    match act {
        Act::Register(_) => !matches!(read, Some(Act::Table(_))),
        Act::Series(_) => read.is_none_or(|read| read == *act),
        Act::Table(_) => true,
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
    // A sparse series, such as Protestant registers, may hold nothing of the
    // first locality listed by a thesaurus the portal shares between its
    // searches, or with a department of a shared portal, which some
    // portals answer with an error: the search's registers of every
    // locality stand in.
    let viewable = |found: &page::Found| {
        found.rows.iter().any(|row| {
            row.payload.register.is_some()
                && row
                    .period
                    .as_deref()
                    .and_then(|period| period_ranges(period).first().copied())
                    .is_some_and(|(first, _)| collection.covers(Some(first)))
        })
    };
    let (locality, found) = match search(&settings, locality, act, fetch).await {
        Ok(found) if viewable(&found) || locality.is_empty() => (locality, found),
        Err(failure) if locality.is_empty() || failure.outcome == Outcome::Challenged => {
            return Err(failure);
        }
        Ok(_) | Err(_) => ("", search(&settings, "", act, fetch).await?),
    };
    if found.rows.is_empty() {
        return Err(Failure::drift(
            step,
            "the result rows of the first listed locality",
            "no register",
        ));
    }
    let wanted = fold(locality);
    // Oldest first, whatever order the portal sorts by (one lists first a
    // register its year index leaves undated, which no cited year finds).
    // A search by act also lists the decennial tables of that act, and a
    // series search other series the portal files with it (the lists of the
    // contingent among military registers): only the registers of the act
    // are cited by it.
    let mut rows = found.rows;
    rows.retain(|row| is_of_act(row.act.as_deref(), act));
    rows.sort_by_key(|row| {
        row.period
            .as_deref()
            .and_then(|period| period_ranges(period).first().map(|(first, _)| *first))
            .unwrap_or(u16::MAX)
    });
    Ok(rows
        .into_iter()
        .map(|row| {
            // The searched locality when the row's places name it, as a
            // citation of the row would.
            let places = &row.payload.places;
            let cited = places
                .iter()
                .any(|place| place.as_locality(&wanted).is_some())
                .then(|| locality.to_owned());
            Register {
                locality: cited
                    .or(row.locality)
                    .unwrap_or_else(|| locality.to_owned()),
                call_number: row.call_number,
                period: row.period,
                images: row.images,
                address: row.payload.register.map(|register| register.viewer()),
                numbers: row.numbers,
            }
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
    fn cites_only_the_rows_of_the_act_searched() {
        let act = |code: &str| Act::from_code(code).unwrap();
        // The lists of the contingent a portal files among its military
        // registers are not cited as registers.
        assert!(is_of_act(Some("RM"), &act("RM")));
        assert!(!is_of_act(Some("CM"), &act("RM")));
        assert!(is_of_act(None, &act("RM")));
        // The decennial tables a search of births lists beside the acts.
        assert!(!is_of_act(Some("TD"), &act("N")));
        assert!(is_of_act(Some("NMD"), &act("N")));
        assert!(is_of_act(Some("N"), &act("TD")));
    }

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
        assert_eq!(facet_url(page, "RECH_commune"), None);
    }

    #[test]
    fn reads_the_typed_facet_of_the_locality_input() {
        let page = r#"<script>new VT_Control("document",{"id":"document","url":"\/arcfacette.php?ind=x&fld=RECH_ville|RECH_type&nav=1&reload=1"});
            new VT_Control("ArchivesRECHVille",{"id":"ArchivesRECHVille","event":"keydownac","url":"\/arcfacette.php?ind=x&fld=RECH_ville|RECH_type&nav=1&limit=4&id=RECH_ville&autoc=1","conditions":{"stop":{"relatedTarget":[{"type":"button","class":"facette-select"}]}},"bcontinue":true});</script>"#;
        assert_eq!(
            facet_url(page, "RECH_ville").as_deref(),
            Some(
                "/arcfacette.php?ind=x&fld=RECH_ville|RECH_type&nav=1&limit=4&id=RECH_ville&autoc=1"
            )
        );
        assert_eq!(facet_url(page, "RECH_type"), None);
        let answer = r#"<script>$('x').innerHTML = '<ul><li><button class="facette-select facette-select-rech_ville" value="Saint-Exemple" type="button">Saint-Exemple<span> (3)</span></button></li><li><button class="facette-select facette-select-rech_ville" value="Saint-Autre-d\'Exemple" type="button">x</button></li></ul>';</script>"#;
        assert_eq!(
            facet_values(answer, "RECH_ville"),
            ["Saint-Exemple", "Saint-Autre-d'Exemple"]
        );
    }

    #[test]
    fn reads_the_options_checkboxes_and_branches_offered() {
        let form = r#"<form id="arc_form_rech"><select name="RECH_annee" id="a"><option value="0">--</option><option value="1901">1901</option><option value="1896">1896</option></select>
            <input type="checkbox" name="RECH_bureau[]" value="Exampleville" /><input type="checkbox" name="RECH_bureau[]" value="Autreville" /></form>"#;
        assert_eq!(offered(form, "RECH_annee"), ["1901", "1896"]);
        assert_eq!(
            offered(form, "RECH_bureau[]"),
            ["Exampleville", "Autreville"]
        );
        assert!(offered(form, "RECH_other").is_empty());
        let tree = r#"<li id="tv_node-notice-1"><a href="/archives/fonds/X/view:1" title="Détail de la branche EXAMPLEVILLE">x</a></li>
            <li><a title="Détail de la branche AUTREVILLE">y</a></li>"#;
        assert_eq!(branches(tree), ["EXAMPLEVILLE", "AUTREVILLE"]);
    }

    #[test]
    fn prefers_the_localities_of_the_department_a_shared_portal_filters() {
        let settings: Settings = serde_json::from_value(serde_json::json!({
            "origin": "https://archives.example.org",
            "search": "etatcivil",
            "node": 1,
            "fields": { "locality": "RECH_commune" },
            "params": { "RECH_departement": "Exampledept" }
        }))
        .unwrap();
        let labels = vec![
            "Autreville (Otherdept, France)".to_owned(),
            "Exampleville (Exampledept, France)".to_owned(),
        ];
        assert_eq!(
            of_settings(&settings, labels),
            ["Exampleville (Exampledept, France)"]
        );
        let others = vec!["Autreville (Otherdept, France)".to_owned()];
        assert_eq!(of_settings(&settings, others.clone()), others);
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
