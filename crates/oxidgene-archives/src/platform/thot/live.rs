//! The THOT part of the live checks (Archive Portals §9.1): the module's
//! form, once the session is open, lists the localities and offers the
//! settings' document kinds and year inputs; a search lists registers
//! without image counts, which the chosen register's slide file gives where
//! its views have addresses. A portal without them (`views: "register"`)
//! opens the register on its first view, uncounted.

use super::page::{self, Form};
use super::{Settings, Thot, Views, check_form, form_page, open_session, search};
use crate::catalog::Collection;
use crate::citation::{Act, CitationParts};
use crate::live::{Failure, Probe, Register, Step};
use crate::platform::BoxFuture;
use crate::platform::locality::{forms, label_name};
use crate::platform::markup::fold;
use crate::transport::PortalFetch;

fn settings(collection: &Collection, step: Step) -> Result<Settings, Failure> {
    Settings::read(collection)
        .map_err(|error| Failure::drift(step, "valid THOT settings", error.to_string()))
}

/// The alphabetically first locality a list names, as a citation writes
/// it: a hamlet, written with its commune, and a placeholder such as
/// `AUTRES DEPARTEMENTS (HORS EXEMPLE)` are passed over.
fn first(labels: &[String]) -> Option<String> {
    labels
        .iter()
        .map(|label| label_name(label))
        .filter(|name| !name.contains(['(', ')']) && !fold(name).starts_with("autres"))
        .min_by_key(|name| fold(name))
}

async fn search_page(collection: &Collection, fetch: &dyn PortalFetch) -> Result<String, Failure> {
    let step = Step::SearchPage;
    let settings = settings(collection, step)?;
    let expected = "the module's form with the settings' criteria";
    open_session(&settings, fetch)
        .await
        .map_err(|error| Failure::from_error(step, expected, &error))?;
    let page = form_page(&settings, fetch)
        .await
        .map_err(|error| Failure::from_error(step, expected, &error))?;
    let form =
        Form::of(&page).ok_or_else(|| Failure::unreadable(step, expected, &page, "no form"))?;
    for value in settings.acts.values() {
        check_form(&settings, &form, Some(value))
            .map_err(|error| Failure::from_error(step, expected, &error))?;
    }
    check_form(&settings, &form, None)
        .map_err(|error| Failure::from_error(step, expected, &error))?;
    first(&form.values(settings.locality))
        .ok_or_else(|| Failure::drift(step, "a locality in the list", "none"))
}

async fn registers(
    collection: &Collection,
    locality: &str,
    act: &Act,
    fetch: &dyn PortalFetch,
) -> Result<Vec<Register>, Failure> {
    let step = Step::Discovery;
    let settings = settings(collection, step)?;
    let expected = "the registers of the first listed locality";
    let citation = CitationParts {
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
    let wanted = forms(locality);
    let localities: Vec<&str> = wanted.iter().map(String::as_str).collect();
    let found = search(&settings, &citation, &localities, fetch)
        .await
        .map_err(|error| Failure::from_error(step, expected, &error))?;
    if found.candidates.is_empty() {
        return Err(Failure::drift(step, expected, "no register"));
    }
    Ok(found
        .candidates
        .into_iter()
        .map(|row| Register {
            locality: row.locality.unwrap_or_else(|| locality.to_owned()),
            call_number: row.call_number,
            period: row.period,
            images: None,
            address: Some(settings.viewer_path(&row.payload)),
            numbers: row.numbers,
        })
        .collect())
}

/// The views of the chosen register's slide file, the viewer page naming
/// it: one opening of the register, as a reader's. A register without
/// addresses per view is not counted, which would cost an opening.
async fn images(
    collection: &Collection,
    register: &Register,
    fetch: &dyn PortalFetch,
) -> Result<Option<u16>, Failure> {
    let step = Step::Discovery;
    let settings = settings(collection, step)?;
    let Some(viewer) = register
        .address
        .as_ref()
        .filter(|_| settings.views == Views::Ark)
    else {
        return Ok(None);
    };
    let expected = "the register's slide file";
    let page = fetch
        .get(viewer)
        .await
        .map_err(|error| Failure::fetch(step, expected, error))?;
    let path = page::slide_path(&page, &settings.base)
        .ok_or_else(|| Failure::unreadable(step, expected, &page, "no slide file"))?;
    let file = fetch
        .get(&path)
        .await
        .map_err(|error| Failure::fetch(step, expected, error))?;
    let slides =
        page::slides(&file).map_err(|error| Failure::from_error(step, expected, &error))?;
    Ok(u16::try_from(slides.count).ok())
}

impl Probe for Thot {
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

    fn addresses_views(&self, collection: &Collection) -> bool {
        Settings::read(collection).is_ok_and(|settings| settings.views == Views::Ark)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_with_a_commune_not_a_hamlet_nor_a_placeholder() {
        let labels: Vec<String> = [
            "AUTRES DEPARTEMENTS (HORS EXEMPLE)",
            "ABBAYE (EXAMPLEVILLE, EXEMPLE, FRANCE ; HAMEAU)",
            "SAMPLETON (EXEMPLE, FRANCE)",
            "BOURG-EXEMPLE (LE)",
        ]
        .map(str::to_owned)
        .into();
        assert_eq!(first(&labels).as_deref(), Some("Le BOURG-EXEMPLE"));
        assert_eq!(first(&[]), None);
    }
}
