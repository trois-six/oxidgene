//! Ligeo Diffusion (Boscop), the publishing software of many French
//! departmental archives.
//!
//! Each collection is one search of the portal ("recherche"), named in the
//! path and placed in the menu by a node number, or the search within one
//! finding aid. Its form is a plain `GET` whose results are server-rendered
//! HTML, a table or a list of notices whose columns or labels differ from
//! portal to portal and are read by their text. The cited register is
//! selected among the rows (`select`), after each row's places are matched
//! with the cited locality and parish; its viewer address is
//! `/ark:/<naan>/<id>/<tag>/<group>/<view>`, and for a `display: "iiif"`
//! archive its IIIF Presentation 2 manifest gives the image count and sizes.
//! Archive Portals §4.5 specifies the requests.

#[cfg(any(test, feature = "live"))]
mod live;
mod page;
mod place;
mod settings;
#[cfg(test)]
mod tests;

use super::iiif::{PICTURE_BOUND, THUMBNAIL_MIN_WIDTH, image_info};
use super::locality::{self, OtherNames};
use super::markup::fold;
use super::select::{Candidate, Selection, parish_key, select};
use super::view::{cited_views, view_target};
use super::{BoxFuture, Platform, PortalEndpoint};
use crate::catalog::{Archive, CatalogError, Collection, Display};
use crate::citation::CitationParts;
use crate::transport::PortalFetch;
use crate::{ArchiveImage, ArchiveTarget, ArchiveView, ResolveError};
use page::Row;
use settings::Settings;
#[cfg(test)]
use settings::{Columns, Names};

/// The Ligeo adapter.
pub struct Ligeo;

impl Platform for Ligeo {
    fn id(&self) -> &'static str {
        "ligeo"
    }

    fn validate(&self, collection: &Collection) -> Result<(), CatalogError> {
        Settings::read(collection).map(drop)
    }

    fn endpoint(&self, collection: &Collection) -> Option<PortalEndpoint> {
        let settings = Settings::read(collection).ok()?;
        Some(PortalEndpoint {
            start: settings.search_page(),
            origin: settings.origin,
            other_origins: Vec::new(),
            access: settings.transport,
            insecure_http: false,
        })
    }

    fn results_url(&self, collection: &Collection, citation: &CitationParts) -> Option<String> {
        let settings = Settings::read(collection).ok()?;
        let path = settings.results_path(&settings.filters(citation));
        Some(format!("{}{path}", settings.origin))
    }

    fn resolve<'a>(
        &'a self,
        archive: &'a Archive,
        collection: &'a Collection,
        citation: &'a CitationParts,
        fetch: &'a dyn PortalFetch,
    ) -> BoxFuture<'a, Result<ArchiveTarget, ResolveError>> {
        Box::pin(resolve(archive, collection, citation, fetch))
    }
}

async fn resolve(
    archive: &Archive,
    collection: &Collection,
    citation: &CitationParts,
    fetch: &dyn PortalFetch,
) -> Result<ArchiveTarget, ResolveError> {
    let settings = Settings::read(collection).map_err(|_| ResolveError::NoAdapter)?;
    let (path, found) = search(&settings, citation, fetch).await?;
    let results = |matches| ArchiveTarget::Results {
        url: format!("{}{path}", settings.origin),
        matches: Some(matches),
    };

    // More answers than rows: the portal paginates, and the cited register
    // may be on a page not read.
    if found.total.is_some_and(|total| total > found.rows.len()) {
        return Ok(results(found.total.unwrap_or_default()));
    }
    let row = match choose(&found.rows, citation, &settings) {
        Selection::One(row) => row,
        Selection::Many(matches) => return Ok(results(matches)),
    };
    let Some(register) = &row.payload.register else {
        return Ok(results(1));
    };

    let mut canvases = Vec::new();
    let image_count = match archive.display {
        Display::Iiif => {
            canvases = page::manifest(&fetch.get(&register.manifest()).await?)?;
            canvases.len()
        }
        // Without a manifest the count is the row's; a row that shows none
        // leaves the portal's viewer to bound the view.
        Display::Portal => row.images.map_or(usize::MAX, usize::from),
    };

    let cited = cited_views(citation, image_count, row.period.as_deref());
    let mut views = Vec::with_capacity(cited.len());
    for view in cited {
        let canvas = canvases.get(usize::from(view.view) - 1);
        views.push(ArchiveView {
            view: view.view,
            url: settings.view_url(register, view.view),
            ark: canvas
                .and_then(|canvas| canvas.ark.as_deref())
                .and_then(|ark| settings.on_origin(ark, "/ark:/")),
            image: match canvas {
                Some(canvas) => Some(image(&settings, canvas, fetch).await?),
                None => None,
            },
        });
    }
    Ok(view_target(
        archive,
        citation,
        row.call_number.as_deref(),
        image_count,
        settings.view_url(register, 1),
        views,
    ))
}

/// The search's path and answer. A cited call number the portal writes
/// otherwise finds nothing: the search by the other parts then runs, once.
/// A portal listing the cited locality under another name finds no register
/// of it: the search for that name then runs, once (Archive Portals §4.5,
/// step 1).
async fn search(
    settings: &Settings,
    citation: &CitationParts,
    fetch: &dyn PortalFetch,
) -> Result<(String, page::Found), ResolveError> {
    let path = settings.results_path(&settings.filters(citation));
    let found = page::results(&fetch.get(&path).await?, &settings.columns)?;
    let (searched, path, found) = if !found.rows.is_empty()
        || settings.fields.call_number.is_none()
        || citation.call_number.is_none()
    {
        (citation.clone(), path, found)
    } else {
        let mut plain = citation.clone();
        plain.call_number = None;
        let path = settings.results_path(&settings.filters(&plain));
        let found = page::results(&fetch.get(&path).await?, &settings.columns)?;
        (plain, path, found)
    };
    let Some(locality) = search_again(&found, citation, settings) else {
        return Ok((path, found));
    };
    let renamed = CitationParts {
        locality,
        ..searched
    };
    let again = settings.results_path(&settings.filters(&renamed));
    let answer = page::results(&fetch.get(&again).await?, &settings.columns)?;
    Ok(if answer.rows.is_empty() {
        (path, found)
    } else {
        (again, answer)
    })
}

/// The name the collection is searched for once more when its answer for
/// the cited locality holds no register of it: a portal that lists the
/// commune under another name — a name the place dictionary knows for it,
/// or the cited name without its qualifier (`Exampleville` for
/// `Exampleville-sous-Bois`), whose substring match finds every name
/// extending it. `None` when the answer names the cited locality, runs past
/// its page, or the collection searches by no locality.
fn search_again(
    found: &page::Found,
    citation: &CitationParts,
    settings: &Settings,
) -> Option<String> {
    if settings.fields.locality.is_none()
        || !settings.columns.locate()
        || found.total.is_some_and(|total| total > found.rows.len())
    {
        return None;
    }
    let wanted = fold(&citation.locality);
    let others = OtherNames::new(citation, &[&wanted]);
    let named = found.rows.iter().any(|row| {
        row.payload
            .places
            .iter()
            .any(|place| place.as_locality(&wanted).is_some() || others.is_known(&place.name))
    });
    if named || !matches!(choose(&found.rows, citation, settings), Selection::Many(0)) {
        return None;
    }
    locality::search_again(citation)
}

/// A row as the citation reads it: when one of its places is the cited
/// locality, or lies within it, the row's locality is the cited one, with
/// the place within it as its parish; failing that, when one of its places
/// names the cited locality under another name ([`OtherNames`]), the row's
/// locality is that place's name, which selection weighs. When one of the
/// parishes it names is the cited parish, its parish is the cited one.
fn as_cited(
    row: &Candidate<Row>,
    wanted: &str,
    others: &OtherNames,
    parish: Option<&str>,
) -> Candidate<()> {
    let mut read = Candidate {
        locality: row.locality.clone(),
        call_number: row.call_number.clone(),
        act: row.act.clone(),
        parish: row.parish.clone(),
        period: row.period.clone(),
        images: row.images,
        numbers: row.numbers,
        payload: (),
    };
    let places = &row.payload.places;
    if let Some(within) = places.iter().find_map(|place| place.as_locality(wanted)) {
        read.locality = Some(wanted.to_owned());
        read.parish = read.parish.or(within);
    } else if !wanted.is_empty()
        && let Some(place) = others.best(places, |place| place.name.clone()).first()
    {
        read.locality = Some(place.name.clone());
    }
    if let Some(parish) = parish {
        let named = row
            .payload
            .parishes
            .iter()
            .chain(places.iter().filter_map(|place| place.parish.as_ref()))
            .chain(places.iter().map(|place| &place.name))
            .any(|name| parish_key(name) == parish);
        if named {
            read.parish = Some(parish.to_owned());
        }
    }
    read
}

/// The row of the cited register, among those with a viewer link: a
/// register listed without one is not digitised. Rows are compared as the
/// citation reads them ([`as_cited`]); a collection whose rows show no
/// locality, a series searched by year alone, keeps every row whatever the
/// cited locality. The call number only breaks a tie: the portals show none
/// (Ain shows an internal reference), or one shared by the registers of
/// every locality or of several acts (Ardèche), so a cited call number a
/// row does not carry must not discard it.
fn choose<'r>(
    rows: &'r [Candidate<Row>],
    citation: &CitationParts,
    settings: &Settings,
) -> Selection<'r, Row> {
    let wanted = if settings.fields.locality.is_some() && settings.columns.locate() {
        fold(&citation.locality)
    } else {
        String::new()
    };
    let parish = citation.parish.as_deref().map(parish_key);
    let viewable: Vec<&Candidate<Row>> = rows
        .iter()
        .filter(|row| row.payload.register.is_some())
        .collect();
    let mut cited = citation.clone();
    cited.locality.clone_from(&wanted);
    cited.parish.clone_from(&parish);
    let localities = [wanted.as_str()];
    let others = OtherNames::new(&cited, &localities);
    let read: Vec<Candidate<()>> = viewable
        .iter()
        .map(|row| as_cited(row, &wanted, &others, parish.as_deref()))
        .collect();
    let index = |chosen: &Candidate<()>| {
        read.iter()
            .position(|candidate| std::ptr::eq(candidate, chosen))
            .map(|at| viewable[at])
    };

    let mut without = cited.clone();
    without.call_number = None;
    // A renamed commune's register is kept on the cited call number too
    // (`select`), which the first selection ignores: none kept, the call
    // number may keep it.
    let selection = match select(&read, &without, &localities) {
        Selection::Many(count) if count != 1 && cited.call_number.is_some() => {
            match select(&read, &cited, &localities) {
                Selection::One(one) => Selection::One(one),
                Selection::Many(again) => Selection::Many(if count == 0 { again } else { count }),
            }
        }
        other => other,
    };
    match selection {
        Selection::One(one) => index(one).map_or(Selection::Many(1), Selection::One),
        Selection::Many(count) => Selection::Many(count),
    }
}

/// The image of one view, on the portal's own origin: the level 1 service's
/// whole image bounded to the screen, and a tile. The size is the service's
/// own, from its `info.json`: the manifest's canvases declare another size
/// and other proportions than the image the service serves.
async fn image(
    settings: &Settings,
    canvas: &page::Canvas,
    fetch: &dyn PortalFetch,
) -> Result<ArchiveImage, ResolveError> {
    let base = canvas
        .service
        .as_deref()
        .and_then(|service| settings.on_origin(service, "/iiif/"))
        .ok_or_else(|| {
            ResolveError::UnexpectedResponse("ligeo: a canvas lacks its image service".to_owned())
        })?;
    let path = &base[settings.origin.len()..];
    let info = image_info(&fetch.get(&format!("{path}/info.json")).await?)?;
    let (width, height) = (info.width, info.height);
    let picture = if width.max(height) > PICTURE_BOUND {
        // Level 1 sizes by width or height alone, not by a bounding box.
        if width >= height {
            format!("{PICTURE_BOUND},")
        } else {
            format!(",{PICTURE_BOUND}")
        }
    } else {
        "full".to_owned()
    };
    Ok(ArchiveImage {
        picture: format!("{base}/full/{picture}/0/default.jpg"),
        thumbnail: format!("{base}/full/{THUMBNAIL_MIN_WIDTH},/0/default.jpg"),
        width,
        height,
    })
}
