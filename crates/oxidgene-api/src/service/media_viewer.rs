//! Targeted read model for the media viewer: a document, its pages, and
//! everything the viewer says about the page on screen.
//!
//! Loaded piece by piece, opening a document asked for the media, then its
//! document, its pages, its note and the page's transcript, its place, the
//! links of the document and of the page, the regions on the page, the names
//! of everybody those name, and each attached person's portrait and sex — a
//! score of requests, several repeated by nested components. One read model
//! answers all of it, in a fixed number of statements whatever the number of
//! pages, links and regions.

use std::collections::HashMap;

use oxidgene_core::OxidGeneError;
use oxidgene_core::collections::sorted_unique;
use oxidgene_core::enums::Sex;
use oxidgene_core::types::{Media, MediaLink, Note, Place, PortraitRef, Vignette};
use oxidgene_db::repo::{MediaLinkRepo, MediaRepo, NoteRepo, PersonRepo, PlaceRepo, VignetteRepo};
use sea_orm::DatabaseConnection;
use serde::Serialize;
use uuid::Uuid;

use crate::service::gallery::{GalleryBundle, load_gallery_bundle};
use crate::service::portrait::portrait_refs;
use crate::service::relation_labels::{RelationLabels, load_labels};
use crate::service::scope::{TreeResource, require_tree_resource};

/// Everything the media viewer draws for one page of a document.
#[derive(Debug, Clone, Serialize)]
pub struct MediaViewerBundle {
    /// The document opened. Asked for one of its pages, the document holding
    /// it: a page is always read inside its document.
    pub media: Media,
    /// The document's pages, in order; empty for a document with none.
    pub pages: Vec<Media>,
    /// The zero-based position in `pages` of the page described below: the
    /// one asked for, else the page named by the media id, else the first,
    /// clamped to the last page. `0` when there are no pages.
    pub page: usize,
    /// The note about the whole document, the oldest when there are several.
    pub note: Option<Note>,
    /// The transcript of the page, the oldest of its notes.
    pub page_note: Option<Note>,
    /// Where the document is filed.
    pub place: Option<Place>,
    /// The links of the document and of the page, each `media_id` telling
    /// which of the two it attaches.
    pub links: Vec<MediaLink>,
    /// The regions identified on the page, oldest first.
    pub vignettes: Vec<Vignette>,
    /// The names of every person the links and the regions name, and the
    /// spouses of every couple the links name.
    pub labels: RelationLabels,
    /// The persons the links attach, each with what stands in for a page the
    /// viewer cannot draw: their portrait, and their sex for the silhouette.
    pub persons: Vec<MediaViewerPerson>,
    /// Where the page's thumbnail and the regions' pictures are drawn from.
    pub gallery: GalleryBundle,
}

/// A person attached to the document or to its page.
#[derive(Debug, Clone, Serialize)]
pub struct MediaViewerPerson {
    pub person_id: Uuid,
    pub sex: Sex,
    /// Where their portrait is drawn from, when they have one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub portrait: Option<PortraitRef>,
}

/// The viewer's bundle of media `media_id` of tree `tree_id`, describing its
/// page at position `page` (see [`MediaViewerBundle::page`]).
#[tracing::instrument(name = "media_viewer.load", skip_all)]
pub async fn load_media_viewer_bundle(
    db: &DatabaseConnection,
    tree_id: Uuid,
    media_id: Uuid,
    page: Option<usize>,
) -> Result<MediaViewerBundle, OxidGeneError> {
    require_tree_resource(db, tree_id, TreeResource::Media, media_id).await?;
    let opened = MediaRepo::get(db, media_id).await?;
    let (media, named_page) = match opened.parent_media_id {
        Some(document_id) => (MediaRepo::get(db, document_id).await?, Some(opened.id)),
        None => (opened, None),
    };
    let pages = MediaRepo::list_pages(db, media.id).await?;
    let named = named_page.and_then(|id| pages.iter().position(|item| item.id == id));
    let page = page
        .or(named)
        .unwrap_or(0)
        .min(pages.len().saturating_sub(1));
    // The row whose regions, links and transcript are shown: the page on
    // screen, or the document itself while it has no page.
    let shown = pages.get(page).map(|item| item.id);
    let described = sorted_unique(std::iter::once(media.id).chain(shown));

    let (notes, links, vignettes, place) = tokio::try_join!(
        NoteRepo::list_by_medias(db, tree_id, &described),
        MediaLinkRepo::list_by_medias(db, &described),
        VignetteRepo::list_for_medias(db, shown.as_slice()),
        async {
            match media.place_id {
                Some(place_id) => PlaceRepo::get_many(db, &[place_id])
                    .await
                    .map(|places| places.into_iter().next()),
                None => Ok(None),
            }
        },
    )?;
    let first_note = |owner: Option<Uuid>| {
        notes
            .iter()
            .filter(|note| owner.is_some() && note.media_id == owner)
            .min_by_key(|note| note.id)
            .cloned()
    };
    let (note, page_note) = (first_note(Some(media.id)), first_note(shown));

    let attached = sorted_unique(links.iter().filter_map(|link| link.person_id));
    let named_persons = sorted_unique(
        attached
            .iter()
            .copied()
            .chain(vignettes.iter().filter_map(|vignette| vignette.person_id)),
    );
    let families = sorted_unique(links.iter().filter_map(|link| link.family_id));
    let vignette_ids = vignettes
        .iter()
        .map(|vignette| vignette.id)
        .collect::<Vec<_>>();
    let (labels, persons, mut portraits, gallery) = tokio::try_join!(
        load_labels(db, tree_id, &named_persons, &families),
        PersonRepo::get_many(db, &attached),
        portrait_refs(db, tree_id, &attached),
        load_gallery_bundle(db, tree_id, shown.as_slice(), &vignette_ids),
    )?;
    let sexes = persons
        .into_iter()
        .filter(|person| person.tree_id == tree_id)
        .map(|person| (person.id, person.sex))
        .collect::<HashMap<_, _>>();
    let persons = attached
        .into_iter()
        .filter_map(|person_id| {
            Some(MediaViewerPerson {
                person_id,
                sex: *sexes.get(&person_id)?,
                portrait: portraits.remove(&person_id),
            })
        })
        .collect();

    Ok(MediaViewerBundle {
        media,
        pages,
        page,
        note,
        page_note,
        place,
        links,
        vignettes,
        labels,
        persons,
        gallery,
    })
}
