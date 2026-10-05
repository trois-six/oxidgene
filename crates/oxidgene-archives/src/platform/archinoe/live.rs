//! The Archinoë part of the live checks (Archive Portals §9.1), one probe
//! for the three search modules:
//!
//! - `registre`: the search page's locality select lists every commune with
//!   its identifier, and its act select every act identifier the settings
//!   name;
//! - `seriel`: the form names its inputs, and the locality autocomplete,
//!   `ir_seriel_data.php?…&query=<letters>`, answers JSON `suggestions` in
//!   the portal's label form (`Exampleville (Department, France)`);
//! - `ead`: the finding aid's root lists every commune.
//!
//! The results of no module count a register's images: the chosen
//! register's viewer page does, one `div_image_<n>` per view.

use super::{
    Archinoe, Register as Row, Search, Settings, VIEW_MARKER, ead, query, registre, seriel,
};
use crate::catalog::Collection;
use crate::citation::Act;
use crate::live::{Failure, Probe, Register, Step};
use crate::platform::BoxFuture;
use crate::platform::locality::LocalityStyle;
use crate::platform::markup::fold;
use crate::platform::select::Candidate;
use crate::transport::{PortalFetch, PortalRequest};

/// The letters the `seriel` locality lookup sends: `Saint-…` localities exist
/// in every French department.
const PROBE: &str = "Sai";

fn settings(collection: &Collection, step: Step) -> Result<Settings, Failure> {
    Settings::read(collection)
        .map_err(|error| Failure::drift(step, "valid Archinoë settings", error.to_string()))
}

/// A `GET` of the probe, its failures reported for `step`.
async fn get(
    fetch: &dyn PortalFetch,
    path: &str,
    step: Step,
    expected: &str,
) -> Result<String, Failure> {
    fetch
        .get(path)
        .await
        .map_err(|error| Failure::fetch(step, expected, error))
}

/// The alphabetically first of `names`, leaving out a placeholder such as
/// `(aucun)` and a name with a character the portal's encoding lost.
fn first(names: impl Iterator<Item = String>) -> Option<String> {
    names
        .filter(|name| !name.is_empty() && !name.starts_with('(') && !name.contains('\u{fffd}'))
        .min_by_key(|name| fold(name))
}

/// The localities a search page lists, or what it lacks of the settings.
fn localities(settings: &Settings, page: &str) -> Result<Vec<String>, Vec<String>> {
    // The `seriel` forms quote their attributes with apostrophes.
    let has = |name: &str| {
        page.contains(&format!("name=\"{name}\"")) || page.contains(&format!("name='{name}'"))
    };
    let mut missing = Vec::new();
    let localities = match &settings.search {
        Search::Registre {
            locality,
            act,
            year,
            ..
        } => {
            let acts = registre::options(page, act);
            for (code, value) in &settings.acts {
                if !acts.iter().any(|(id, _)| id == value) {
                    missing.push(format!("act value of {code}"));
                }
            }
            if !has(year) {
                missing.push(format!("input {year}"));
            }
            registre::options(page, locality)
                .into_iter()
                .map(|(_, label)| label)
                .collect()
        }
        Search::Seriel {
            locality,
            year_from,
            year_to,
            cote,
            ..
        } => {
            let inputs = [locality, year_from, year_to, cote];
            let checkboxes = settings.acts.values();
            for name in inputs.into_iter().chain(checkboxes) {
                if !has(name) {
                    missing.push(format!("input {name}"));
                }
            }
            Vec::new()
        }
        // The finding aid writes a leading article behind the name.
        Search::Ead { .. } => ead::entries(page)
            .into_iter()
            .map(|(_, name)| LocalityStyle::ArticleSuffix.cited(&name))
            .collect(),
    };
    let listed = matches!(settings.search, Search::Seriel { .. }) || !localities.is_empty();
    if !listed {
        missing.push("locality list".to_owned());
    }
    if missing.is_empty() {
        Ok(localities)
    } else {
        Err(missing)
    }
}

async fn search_page(collection: &Collection, fetch: &dyn PortalFetch) -> Result<String, Failure> {
    let step = Step::SearchPage;
    let settings = settings(collection, step)?;
    let page_path = settings
        .search_page()
        .strip_prefix(&settings.origin)
        .unwrap_or_default()
        .to_owned();
    let page = get(fetch, &page_path, step, "the search page").await?;
    let localities = localities(&settings, &page).map_err(|missing| {
        Failure::unreadable(
            step,
            "the settings' inputs, act values and locality list in the search page",
            &page,
            format!("missing: {}", missing.join(", ")),
        )
    })?;
    let Search::Seriel {
        id,
        form,
        locality_label,
        ..
    } = &settings.search
    else {
        return first(localities.into_iter())
            .ok_or_else(|| Failure::drift(step, "a locality in the list", "none"));
    };

    let expected = "the locality autocomplete's suggestions";
    let answer = get(
        fetch,
        &format!(
            "{}/ir_seriel_data.php?{}",
            settings.base,
            query(&[
                ("f", "0"),
                ("c", "0"),
                ("cle", form),
                ("id", id),
                ("query", PROBE)
            ])
        ),
        step,
        expected,
    )
    .await?;
    let suggestions: Vec<String> = serde_json::from_str::<serde_json::Value>(&answer)
        .ok()
        .and_then(|json| serde_json::from_value(json["suggestions"].clone()).ok())
        .ok_or_else(|| Failure::unreadable(step, expected, &answer, "no suggestions"))?;
    let (before, after) = locality_label.split_once("{locality}").unwrap_or_default();
    first(
        suggestions
            .iter()
            .filter_map(|label| Some(label.strip_prefix(before)?.strip_suffix(after)?.to_owned())),
    )
    .ok_or_else(|| {
        Failure::drift(
            step,
            "a suggestion labelled as the settings' locality_label",
            "none",
        )
    })
}

/// The registers of a `registre` search: the locality's identifier from the
/// search page, then the results.
async fn registre_rows(
    settings: &Settings,
    locality: &str,
    act: &Act,
    fetch: &dyn PortalFetch,
) -> Result<Vec<Candidate<Row>>, Failure> {
    let step = Step::Discovery;
    let Search::Registre {
        locality: field,
        act: act_field,
        ..
    } = &settings.search
    else {
        return Ok(Vec::new());
    };
    let expected = "the result rows of the first listed locality";
    let page = get(
        fetch,
        &format!("{}/registre.html", settings.base),
        step,
        expected,
    )
    .await?;
    let Some((commune, _)) = registre::options(&page, field)
        .into_iter()
        .find(|(_, label)| fold(label) == fold(locality))
    else {
        return Err(Failure::unreadable(
            step,
            expected,
            &page,
            "the locality's identifier",
        ));
    };
    let mut pairs = vec![(field.as_str(), commune.as_str())];
    if let Some(value) = settings.act_value(act) {
        pairs.push((act_field.as_str(), value));
    }
    pairs.push(("ajax", "true"));
    let answer = get(
        fetch,
        &format!("{}/registre_liste.html?{}", settings.base, query(&pairs)),
        step,
        expected,
    )
    .await?;
    registre::rows(&answer, act)
        .map(|(rows, _)| rows)
        .map_err(|error| Failure::from_error(step, expected, &error))
}

/// The registers of a `seriel` search, its first page.
async fn seriel_rows(
    settings: &Settings,
    locality: &str,
    act: &Act,
    fetch: &dyn PortalFetch,
) -> Result<Vec<Candidate<Row>>, Failure> {
    let step = Step::Discovery;
    let Search::Seriel {
        id,
        form,
        locality: field,
        locality_label,
        ..
    } = &settings.search
    else {
        return Ok(Vec::new());
    };
    let label = locality_label.replace("{locality}", locality);
    let mut pairs = vec![(field.as_str(), label.as_str())];
    if let Some(checkbox) = settings.act_value(act) {
        pairs.push((checkbox, "on"));
    }
    let expected = "the result rows of the first listed locality";
    let request = PortalRequest::post(
        format!(
            "{}/ir_seriel_action.php?f=0&cle={form}&id={id}",
            settings.base
        ),
        "application/x-www-form-urlencoded",
        query(&pairs),
    );
    let answer = fetch
        .request(&request)
        .await
        .map_err(|error| Failure::fetch(step, expected, error))?;
    seriel::page_rows(&answer)
        .map(|(rows, _)| rows)
        .map_err(|error| Failure::from_error(step, expected, &error))
}

/// The registers of an `ead` finding aid: the commune's act node, its
/// collections, and each collection's register blocks.
async fn ead_rows(
    settings: &Settings,
    locality: &str,
    act: &Act,
    fetch: &dyn PortalFetch,
) -> Result<Vec<Candidate<Row>>, Failure> {
    let step = Step::Discovery;
    let Search::Ead { ir, eadid } = &settings.search else {
        return Ok(Vec::new());
    };
    let expected = "the registers of the first listed locality";
    let action = format!("{}/ir_ead_visu_action.php?ir={ir}", settings.base);
    let root = get(
        fetch,
        &format!("{}/ir_ead_visu.php?eadid={eadid}&ir={ir}", settings.base),
        step,
        expected,
    )
    .await?;
    let entry = |entries: Vec<(String, String)>, wanted: &str| {
        entries
            .into_iter()
            .find(|(_, name)| fold(name) == fold(wanted))
            .map(|(id, _)| id)
    };
    let missing = |what: &str, page: &str| Failure::unreadable(step, expected, page, what);
    let commune = entry(ead::entries(&root), locality)
        .ok_or_else(|| missing("the locality's node", &root))?;
    let title = settings.act_value(act).unwrap_or_default();
    let acts = get(
        fetch,
        &format!("{action}&id={commune}&toc=1"),
        step,
        expected,
    )
    .await?;
    let node = entry(ead::entries(&acts), title).ok_or_else(|| missing("the act node", &acts))?;
    let mut collections: Vec<String> =
        ead::entries(&get(fetch, &format!("{action}&id={node}&toc=1"), step, expected).await?)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
    if collections.is_empty() {
        collections.push(node);
    }
    // The first collection's registers are enough to cite one.
    let notice = get(
        fetch,
        &format!("{action}&id={}", collections[0]),
        step,
        expected,
    )
    .await?;
    Ok(ead::registers(&notice, locality))
}

async fn registers(
    collection: &Collection,
    locality: &str,
    act: &Act,
    fetch: &dyn PortalFetch,
) -> Result<Vec<Register>, Failure> {
    let step = Step::Discovery;
    let settings = settings(collection, step)?;
    let rows = match settings.search {
        Search::Registre { .. } => registre_rows(&settings, locality, act, fetch).await?,
        Search::Seriel { .. } => seriel_rows(&settings, locality, act, fetch).await?,
        Search::Ead { .. } => ead_rows(&settings, locality, act, fetch).await?,
    };
    if rows.is_empty() {
        return Err(Failure::drift(
            step,
            "the registers of the first listed locality",
            "no register",
        ));
    }
    Ok(rows
        .into_iter()
        .map(|row| Register {
            locality: row
                .locality
                .filter(|shown| fold(shown) != fold(locality))
                .unwrap_or_else(|| locality.to_owned()),
            call_number: row.call_number,
            period: row.period,
            images: row.images,
            address: Some(row.payload.id),
            numbers: row.numbers,
        })
        .collect())
}

/// The views the chosen register's viewer page lists.
async fn images(
    collection: &Collection,
    register: &Register,
    fetch: &dyn PortalFetch,
) -> Result<Option<u16>, Failure> {
    let step = Step::Discovery;
    let settings = settings(collection, step)?;
    let Some(id) = &register.address else {
        return Ok(None);
    };
    let page = get(
        fetch,
        &settings.viewer_path(id),
        step,
        "the register's viewer page",
    )
    .await?;
    Ok(u16::try_from(page.matches(VIEW_MARKER).count()).ok())
}

impl Probe for Archinoe {
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

    fn settings(code: &str) -> Settings {
        Settings::read(
            &ArchiveRegistry::embedded()
                .archive(code)
                .unwrap()
                .collections[0],
        )
        .unwrap()
    }

    #[test]
    fn lists_the_localities_of_each_module() {
        // `registre`: the selects of the form, with the catalogue's act ids.
        let acts: String = settings("AD17")
            .acts
            .values()
            .map(|id| format!("<option value=\"{id}\">Acte</option>"))
            .collect();
        let page = format!(
            "<select id=\"inputcommune\" name=\"commune\"><option value=\"\">-- Choisir --</option>\
             <option value=\"1\">Sampleton</option><option value=\"2\">(aucun)</option>\
             <option value=\"3\">Exampleville</option></select>\
             <select id=\"inputacte\" name=\"acte\">{acts}</select><input name=\"annee\">"
        );
        let listed = localities(&settings("AD17"), &page).unwrap();
        assert_eq!(first(listed.into_iter()).as_deref(), Some("Exampleville"));
        assert_eq!(
            localities(&settings("AD17"), "<select name=\"commune\"></select>").unwrap_err(),
            [
                "act value of B",
                "act value of D",
                "act value of M",
                "act value of N",
                "act value of S",
                "act value of TD",
                "input annee",
                "locality list"
            ]
        );

        // `seriel`: inputs quoted with apostrophes, localities asked apart.
        let seriel = settings("AD62");
        let Search::Seriel {
            locality,
            year_from,
            year_to,
            cote,
            ..
        } = &seriel.search
        else {
            unreachable!("a seriel search");
        };
        let inputs: String = [locality, year_from, year_to, cote]
            .into_iter()
            .chain(seriel.acts.values())
            .map(|name| format!("<input id='{name}' name='{name}'>"))
            .collect();
        assert_eq!(localities(&seriel, &inputs), Ok(Vec::new()));

        // `ead`: the root's commune entries, a leading article written first.
        let root = include_str!("../../../fixtures/archinoe/ad21-root.html");
        let communes = localities(&settings("AD21"), root).unwrap();
        assert!(communes.contains(&"Le Hameau-Exemple".to_owned()));
        assert_eq!(first(communes.into_iter()).as_deref(), Some("Exampleville"));
    }
}
