//! The `seriel` module: a form that reads only a form-encoded `POST`.
//!
//! The locality is the portal's label, `Exampleville (Pas-de-Calais, France)`, which
//! its autocomplete lists; the setting `locality_label` writes it, so the
//! search needs no request for the list. The answer is a fragment of three
//! registers per page, each with a notice table of call number, locality and
//! period, and the parish as the notice's heading.

use super::{
    Found, Register, Search, Settings, choose, number_after, query, unchallenged, unexpected,
};
use crate::ResolveError;
use crate::citation::CitationParts;
use crate::platform::markup::{self, first_number};
use crate::platform::select::{Candidate, narrow};
use crate::transport::{PortalFetch, PortalRequest};

/// The most result pages read: three registers each. A search that leaves
/// more is too wide to choose from.
const MAX_PAGES: usize = 5;

/// The form's fields of one attempt: the locality label, the year as both
/// bounds, the act's checkbox and the call number when one is searched.
fn form_body(
    settings: &Settings,
    citation: &CitationParts,
    form: &Form<'_>,
    cote_value: Option<&str>,
) -> String {
    let mut pairs = vec![(form.locality, form.label.as_str())];
    if let Some(year) = &form.year {
        pairs.push((form.year_from, year.as_str()));
        pairs.push((form.year_to, year.as_str()));
    }
    if let Some(act) = settings.act_value(&citation.act) {
        pairs.push((act, "on"));
    }
    if let Some(cote_value) = cote_value {
        pairs.push((form.cote, cote_value));
    }
    query(&pairs)
}

/// The fixed parts of the search form of one citation.
struct Form<'a> {
    action: String,
    label: String,
    year: Option<String>,
    locality: &'a str,
    year_from: &'a str,
    year_to: &'a str,
    cote: &'a str,
}

/// One search, read page by page: the registers the citation decides, or
/// the number of registers when it is too wide to choose from.
async fn read_pages(
    settings: &Settings,
    citation: &CitationParts,
    localities: &[&str],
    fetch: &dyn PortalFetch,
    form: &Form<'_>,
    body: &str,
) -> Result<Found, ResolveError> {
    let mut rows = Vec::new();
    let mut total = 0;
    for page in 0..MAX_PAGES {
        let url = if page == 0 {
            form.action.clone()
        } else {
            format!("{}&r=0&page={page}", form.action)
        };
        let request = PortalRequest::post(url, "application/x-www-form-urlencoded", body);
        let answer = unchallenged(fetch.request(&request).await?)?;
        let (listed, announced) = page_rows(&answer)?;
        total = announced;
        if listed.is_empty() {
            break;
        }
        rows.extend(listed);
        // Stop at the register the citation decides, or when the
        // search is read to its end.
        if narrow(&rows, citation, localities).len() == 1 || rows.len() >= total {
            return Ok(
                match choose(settings, &rows, citation, localities, fetch).await? {
                    Ok(chosen) => Found::One(chosen),
                    Err(matches) => Found::Many(matches.max(total)),
                },
            );
        }
    }
    // More registers than the pages read, and none decided.
    Ok(Found::Many(total))
}

pub(super) async fn find(
    settings: &Settings,
    citation: &CitationParts,
    localities: &[&str],
    fetch: &dyn PortalFetch,
) -> Result<Found, ResolveError> {
    let Search::Seriel {
        id,
        form: form_key,
        locality,
        year_from,
        year_to,
        cote,
        locality_label,
    } = &settings.search
    else {
        return Err(unexpected("settings of another search"));
    };
    let form = Form {
        action: format!(
            "{}/ir_seriel_action.php?f=0&cle={form_key}&id={id}",
            settings.base
        ),
        label: locality_label.replace("{locality}", &citation.locality),
        year: citation.year.map(|year| year.to_string()),
        locality,
        year_from,
        year_to,
        cote,
    };
    let call_number = citation
        .call_number
        .as_ref()
        .map(|call_number| call_number.as_str().to_owned());

    // The portal's call number search matches words and prefixes, so it
    // narrows the register's year-long listing; a call number the portal
    // does not recognize is searched again without it.
    let mut attempts = vec![call_number.as_deref()];
    if call_number.is_some() {
        attempts.push(None);
    }
    for cote_value in attempts {
        let body = form_body(settings, citation, &form, cote_value);
        let found = read_pages(settings, citation, localities, fetch, &form, &body).await?;
        if !matches!(found, Found::Many(0)) {
            return Ok(found);
        }
    }
    Ok(Found::Many(0))
}

/// The registers of one page of results, and the total the portal announces.
pub(super) fn page_rows(html: &str) -> Result<(Vec<Candidate<Register>>, usize), ResolveError> {
    let announced = markup::text_after(html, "class='cnres' >")
        .ok_or_else(|| unexpected("the results lack their total"))?;
    let total = if announced.starts_with("Aucun") {
        0
    } else if announced.starts_with("Un ") {
        1
    } else {
        first_number(&announced).ok_or_else(|| unexpected("the results lack their total"))?
    };
    let mut rows = Vec::new();
    for row in markup::split_after(html, "<tr class=\"rechGrilleLignes") {
        let id = number_after(row, "afficheImage(")
            .ok_or_else(|| unexpected("a row lacks its viewer id"))?;
        let detail = |label: &str| {
            markup::text_after(
                row,
                &format!("class=\"rechDetailLibelle\">{label}</td><td class=\"rechDetailValeur\">"),
            )
            .filter(|text| !text.is_empty())
        };
        // The content cell opens with the parish: `<h3>Paroisse Saint-Exemple</h3>`.
        let parish = row
            .find(">Contenu</td>")
            .and_then(|at| markup::text_after(&row[at..], "<h3>"))
            .map(|text| text.strip_prefix("Paroisse ").unwrap_or(&text).to_owned())
            .filter(|text| !text.is_empty());
        rows.push(Candidate {
            locality: detail("Lieu"),
            call_number: detail("Cote"),
            act: None,
            parish,
            period: detail("Dates extrêmes"),
            images: None,
            payload: Register { id },
        });
    }
    if rows.is_empty() && total > 0 {
        return Err(unexpected("the results list no register"));
    }
    Ok((rows, total))
}
