//! Reading the request interface's answers: the search's result rows and
//! the viewer's image list.
//!
//! The search answer lists its registers twice: `resultats.results` gives
//! each one's record reference and call number, and `resultats.html` renders
//! the same registers, in the same order, as table rows whose `data-champ`
//! cells carry the locality, parish, acts and period, and whose viewer
//! button carries the viewer address and the image count. The rows are read
//! with the shared markup scans.

use serde::Deserialize;

use super::Cells;
use crate::ResolveError;
use crate::platform::markup;
use crate::platform::select::Candidate;

/// What the adapter keeps of a register to open it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Register {
    /// The record reference, `arko_fiche_…`.
    pub(super) record: String,
    /// `/_recherche-api/visionneuse-infos/…`, when the register has images.
    pub(super) viewer: Option<String>,
}

fn unexpected(detail: &str) -> ResolveError {
    ResolveError::UnexpectedResponse(format!("arkotheque: {detail}"))
}

#[derive(Deserialize)]
struct SearchAnswer {
    resultats: Resultats,
}

#[derive(Deserialize)]
struct Resultats {
    results: Vec<ResultEntry>,
    html: String,
}

#[derive(Deserialize)]
struct ResultEntry {
    #[serde(rename = "refUnique")]
    record: String,
    #[serde(default)]
    intitule: Option<String>,
}

/// The registers of a search answer, their cells read by the names `cells`
/// gives.
pub(super) fn search_rows(
    answer: &str,
    cells: &Cells,
) -> Result<Vec<Candidate<Register>>, ResolveError> {
    let answer: SearchAnswer = serde_json::from_str(answer)
        .map_err(|_| unexpected("the search answer lacks resultats.results or html"))?;
    let rendered = markup::split_after(&answer.resultats.html, "<tr class=\"resultat_container");
    if rendered.len() != answer.resultats.results.len() {
        return Err(unexpected("the rendered rows do not match the results"));
    }
    Ok(answer
        .resultats
        .results
        .into_iter()
        .zip(rendered)
        .map(|(entry, html)| {
            let texts = markup::labelled_texts(html, "data-champ");
            let cell = |name: Option<&str>| {
                let name = name?;
                texts
                    .iter()
                    .find(|(cell, _)| cell == name)
                    .map(|(_, text)| text.clone())
            };
            Candidate {
                locality: cell(Some(&cells.locality)),
                call_number: entry
                    .intitule
                    .map(|text| text.trim().to_owned())
                    .filter(|text| !text.is_empty()),
                act: cell(cells.act.as_deref()),
                parish: cell(cells.parish.as_deref()),
                period: cell(cells.period.as_deref()),
                images: markup::text_after(html, "class=\"nombre_images\">")
                    .and_then(|text| markup::first_number(&text)),
                payload: Register {
                    record: entry.record,
                    viewer: markup::attribute(html, "data-visionneuse-url").filter(|address| {
                        address.starts_with("/_recherche-api/visionneuse-infos/")
                    }),
                },
            }
        })
        .collect())
}

/// One image of a register, as the viewer endpoint lists it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(super) struct Source {
    /// `/_recherche-images/show/…/<index>`: the image's IIIF base on the
    /// portal's origin.
    pub(super) src: String,
    /// `/ark:<naan>/<name>…`, the image's persistent address.
    #[serde(rename = "ARKLink", default)]
    pub(super) ark: Option<String>,
}

#[derive(Deserialize)]
struct ViewerAnswer {
    medias: Vec<Media>,
}

#[derive(Deserialize)]
struct Media {
    sources: Vec<Source>,
}

/// The images of a register, in order. Nothing else of the answer is read:
/// its other fields name internal hosts and files.
pub(super) fn viewer_sources(answer: &str) -> Result<Vec<Source>, ResolveError> {
    let answer: ViewerAnswer = serde_json::from_str(answer)
        .map_err(|_| unexpected("the viewer answer lacks medias[].sources"))?;
    let mut sources = answer
        .medias
        .into_iter()
        .next()
        .map(|media| media.sources)
        .unwrap_or_default();
    if sources.is_empty()
        || !sources.iter().all(|source| {
            source.src.starts_with("/_recherche-images/") && !source.src.contains(['?', '#'])
        })
    {
        return Err(unexpected("the viewer answer lists no image path"));
    }
    for source in &mut sources {
        source.ark = source
            .ark
            .take()
            .filter(|ark| ark.starts_with("/ark:") && !ark.contains(['?', '#', ' ']));
    }
    Ok(sources)
}
