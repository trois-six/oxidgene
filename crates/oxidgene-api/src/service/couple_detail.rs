//! Targeted read model for the couple page: the family, both spouses' person
//! bundles, and what the couple holds itself — its notes and its media.
//!
//! Loaded piece by piece, the page asked for the family, then its spouses,
//! then each spouse's bundle, notes and pictures: three round trips deep
//! before anything could be drawn.

use oxidgene_core::OxidGeneError;
use oxidgene_core::collections::sorted_unique;
use oxidgene_core::types::{Family, FamilySpouse, Note};
use oxidgene_db::repo::{
    FamilyRepo, FamilySpouseRepo, MediaLinkRepo, MediaLinkTarget, NoteFilter, NoteRepo,
    PaginationParams,
};
use sea_orm::DatabaseConnection;
use serde::Serialize;
use uuid::Uuid;

use crate::service::gallery::{GalleryBundle, load_gallery_bundle};
use crate::service::person_detail::{
    PersonDetailBundle, ProfileMediaTile, load_person_detail_bundle,
};
use crate::service::scope::{TreeResource, require_tree_resource};

/// The most notes a couple page shows for each of the family and its
/// spouses: the largest page a note list serves.
const NOTES_PER_OWNER: u64 = 100;

#[derive(Debug, Clone, Serialize)]
pub struct CoupleDetailBundle {
    pub family: Family,
    pub spouses: Vec<FamilySpouse>,
    /// One per spouse, in the order of `spouses`.
    pub persons: Vec<PersonDetailBundle>,
    /// The family's notes, then each spouse's, oldest first within each.
    pub notes: Vec<Note>,
    /// The media attached to the family itself.
    pub media: Vec<ProfileMediaTile>,
    /// The addresses of those media's pictures.
    pub gallery: GalleryBundle,
}

#[tracing::instrument(name = "couple_detail.load", skip_all)]
pub async fn load_couple_detail_bundle(
    db: &DatabaseConnection,
    tree_id: Uuid,
    family_id: Uuid,
) -> Result<CoupleDetailBundle, OxidGeneError> {
    require_tree_resource(db, tree_id, TreeResource::Family, family_id).await?;
    let (family, spouses, links) = tokio::try_join!(
        FamilyRepo::get(db, family_id),
        FamilySpouseRepo::list_by_family(db, family_id),
        MediaLinkRepo::list_with_media(db, MediaLinkTarget::Family, family_id),
    )?;

    let persons = futures_util::future::try_join_all(
        spouses
            .iter()
            .map(|spouse| load_person_detail_bundle(db, tree_id, spouse.person_id)),
    )
    .await?;

    let mut notes = owner_notes(
        db,
        tree_id,
        NoteFilter {
            family_id: Some(family_id),
            ..NoteFilter::default()
        },
    )
    .await?;
    for spouse in &spouses {
        let filter = NoteFilter {
            person_id: Some(spouse.person_id),
            ..NoteFilter::default()
        };
        notes.extend(owner_notes(db, tree_id, filter).await?);
    }

    let media: Vec<ProfileMediaTile> = links
        .into_iter()
        .map(|(link, media)| ProfileMediaTile {
            link_id: link.id,
            sort_order: link.sort_order,
            family_id: None,
            media,
        })
        .collect();
    let media_ids = sorted_unique(media.iter().map(|tile| tile.media.id));
    let gallery = load_gallery_bundle(db, tree_id, &media_ids, &[]).await?;

    Ok(CoupleDetailBundle {
        family,
        spouses,
        persons,
        notes,
        media,
        gallery,
    })
}

async fn owner_notes(
    db: &DatabaseConnection,
    tree_id: Uuid,
    filter: NoteFilter,
) -> Result<Vec<Note>, OxidGeneError> {
    let page = PaginationParams {
        first: NOTES_PER_OWNER,
        after: None,
    };
    Ok(NoteRepo::list(db, tree_id, &filter, &page)
        .await?
        .edges
        .into_iter()
        .map(|edge| edge.node)
        .collect())
}
