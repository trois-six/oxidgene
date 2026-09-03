//! Targeted read model for the person detail page.

use std::collections::HashSet;
use std::sync::Arc;

use oxidgene_core::OxidGeneError;
use oxidgene_core::types::{
    Citation, Event, FamilyChild, FamilySpouse, Media, Person, PersonName, Place, Source, Vignette,
};
use oxidgene_db::repo::{
    AncestryRepo, CitationRepo, EventRepo, FamilyChildRepo, FamilySpouseRepo, MediaLinkRepo,
    PersonNameRepo, PersonRepo, PlaceRepo, SourceRepo, TreeRepo, VignetteRepo,
};
use sea_orm::DatabaseConnection;
use serde::Serialize;
use uuid::Uuid;

use crate::media::MediaStore;
use crate::service::gallery::{GalleryBundle, load_gallery_bundle};

const GALLERY_BATCH_SIZE: usize = 1_024;

#[derive(Debug, Clone, Serialize)]
pub struct PersonDetailBundle {
    pub sosa_number: Option<u64>,
    pub persons: Vec<Person>,
    pub names: Vec<PersonName>,
    pub events: Vec<Event>,
    pub places: Vec<Place>,
    pub spouses: Vec<FamilySpouse>,
    pub children: Vec<FamilyChild>,
    pub citations: Vec<Citation>,
    pub sources: Vec<Source>,
    pub profile_media: Vec<ProfileMediaTile>,
    pub profile_vignettes: Vec<Vignette>,
    pub event_media: Vec<EventMediaTile>,
    pub gallery: GalleryBundle,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProfileMediaTile {
    pub link_id: Uuid,
    pub sort_order: i32,
    #[serde(flatten)]
    pub media: Media,
}

#[derive(Debug, Clone, Serialize)]
pub struct EventMediaTile {
    pub event_id: Uuid,
    pub link_id: Uuid,
    pub sort_order: i32,
    #[serde(flatten)]
    pub media: Media,
}

pub async fn load_person_detail_bundle(
    db: &DatabaseConnection,
    store: &Arc<dyn MediaStore>,
    tree_id: Uuid,
    person_id: Uuid,
) -> Result<PersonDetailBundle, OxidGeneError> {
    let person = PersonRepo::get(db, person_id).await?;
    if person.tree_id != tree_id {
        return Err(OxidGeneError::NotFound {
            entity: "Person",
            id: person_id,
        });
    }
    let sosa_number = compute_sosa_number(db, tree_id, person_id).await?;

    let (own_spouse_links, own_child_links) = tokio::try_join!(
        FamilySpouseRepo::list_by_person(db, person_id),
        FamilyChildRepo::list_by_person(db, person_id),
    )?;
    let profile_family_ids = unique_ids(own_spouse_links.iter().map(|link| link.family_id));
    let direct_family_ids = unique_ids(
        own_spouse_links
            .iter()
            .map(|link| link.family_id)
            .chain(own_child_links.iter().map(|link| link.family_id)),
    );
    let direct_spouses = FamilySpouseRepo::list_by_families(db, &direct_family_ids).await?;

    let child_family_ids = own_child_links
        .iter()
        .map(|link| link.family_id)
        .collect::<HashSet<_>>();
    let parent_ids = unique_ids(
        direct_spouses
            .iter()
            .filter(|link| child_family_ids.contains(&link.family_id))
            .map(|link| link.person_id),
    );
    let parent_spouse_links = FamilySpouseRepo::list_by_persons(db, &parent_ids).await?;
    let family_ids = unique_ids(
        direct_family_ids
            .iter()
            .copied()
            .chain(parent_spouse_links.iter().map(|link| link.family_id)),
    );
    let (spouses, children) = tokio::try_join!(
        FamilySpouseRepo::list_by_families(db, &family_ids),
        FamilyChildRepo::list_by_families(db, &family_ids),
    )?;

    let person_ids = unique_ids(
        std::iter::once(person_id)
            .chain(spouses.iter().map(|link| link.person_id))
            .chain(children.iter().map(|link| link.person_id)),
    );
    let related_person_ids = person_ids
        .iter()
        .copied()
        .filter(|id| *id != person_id)
        .collect::<Vec<_>>();
    let direct_family_ids = direct_family_ids.into_iter().collect::<HashSet<_>>();
    let direct_family_id_list = direct_family_ids.iter().copied().collect::<Vec<_>>();
    let timeline_person_ids = unique_ids(
        std::iter::once(person_id)
            .chain(parent_ids.iter().copied())
            .chain(children.iter().map(|link| link.person_id)),
    );

    let (mut persons, names, mut events, family_events) = tokio::try_join!(
        PersonRepo::get_many(db, &related_person_ids),
        PersonNameRepo::list_by_persons(db, &person_ids),
        EventRepo::list_by_persons(db, &timeline_person_ids),
        EventRepo::list_by_families(db, &direct_family_id_list),
    )?;
    persons.push(person);
    persons.sort_by_key(|person| person.id);
    events.extend(family_events);
    events.sort_by_key(|event| event.id);
    events.dedup_by_key(|event| event.id);

    let place_ids = unique_ids(events.iter().filter_map(|event| event.place_id));
    let event_ids = events.iter().map(|event| event.id).collect::<Vec<_>>();
    let (places, citations, media_rows, mut profile_media_rows, profile_vignettes) = tokio::try_join!(
        PlaceRepo::get_many(db, &place_ids),
        CitationRepo::list_for_person_events(db, tree_id, person_id, &event_ids),
        MediaLinkRepo::list_with_media_for_events(db, &event_ids),
        MediaLinkRepo::list_with_media_for_profile(db, person_id, &profile_family_ids),
        VignetteRepo::list_for_person(db, person_id),
    )?;
    let source_ids = unique_ids(citations.iter().map(|citation| citation.source_id));
    let sources = SourceRepo::get_many(db, tree_id, &source_ids).await?;

    let event_media = media_rows
        .into_iter()
        .filter_map(|(link, media)| {
            Some(EventMediaTile {
                event_id: link.event_id?,
                link_id: link.id,
                sort_order: link.sort_order,
                media,
            })
        })
        .collect::<Vec<_>>();
    profile_media_rows
        .sort_by_key(|(link, _)| (link.person_id != Some(person_id), link.sort_order, link.id));
    let mut seen_profile_media = HashSet::new();
    let profile_media = profile_media_rows
        .into_iter()
        .filter_map(|(link, media)| {
            seen_profile_media
                .insert(media.id)
                .then_some(ProfileMediaTile {
                    link_id: link.id,
                    sort_order: link.sort_order,
                    media,
                })
        })
        .collect::<Vec<_>>();
    let media_ids = unique_ids(
        event_media
            .iter()
            .map(|item| item.media.id)
            .chain(profile_media.iter().map(|item| item.media.id)),
    );
    let vignette_ids = unique_ids(profile_vignettes.iter().map(|item| item.id));
    let mut gallery = GalleryBundle {
        media: Vec::new(),
        vignettes: Vec::new(),
    };
    let (mut media_offset, mut vignette_offset) = (0, 0);
    while media_offset < media_ids.len() || vignette_offset < vignette_ids.len() {
        let media_end = (media_offset + GALLERY_BATCH_SIZE).min(media_ids.len());
        let remaining = GALLERY_BATCH_SIZE - (media_end - media_offset);
        let vignette_end = (vignette_offset + remaining).min(vignette_ids.len());
        let batch = load_gallery_bundle(
            db,
            store,
            tree_id,
            &media_ids[media_offset..media_end],
            &vignette_ids[vignette_offset..vignette_end],
        )
        .await?;
        gallery.media.extend(batch.media);
        gallery.vignettes.extend(batch.vignettes);
        media_offset = media_end;
        vignette_offset = vignette_end;
    }

    Ok(PersonDetailBundle {
        sosa_number,
        persons,
        names,
        events,
        places,
        spouses,
        children,
        citations,
        sources,
        profile_media,
        profile_vignettes,
        event_media,
        gallery,
    })
}

/// Resolve one SOSA number while visiting only the ancestry of the configured
/// root, rather than loading every family in the tree.
pub async fn compute_sosa_number(
    db: &DatabaseConnection,
    tree_id: Uuid,
    person_id: Uuid,
) -> Result<Option<u64>, OxidGeneError> {
    let Some(root) = TreeRepo::get(db, tree_id).await?.sosa_root_person_id else {
        return Ok(None);
    };
    AncestryRepo::sosa_number(db, root, person_id).await
}

fn unique_ids(ids: impl IntoIterator<Item = Uuid>) -> Vec<Uuid> {
    let mut ids = ids.into_iter().collect::<Vec<_>>();
    ids.sort_unstable();
    ids.dedup();
    ids
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::store::FsStore;
    use oxidgene_core::enums::{
        Calendar, ChildType, Confidence, DateQualifier, EventType, Sex, SpouseRole,
    };
    use oxidgene_db::repo::{
        FamilyRepo, MediaRepo, PersonRepo, PlaceRepo, SourceRepo, TreeRepo, connect, run_migrations,
    };

    async fn create_person(db: &DatabaseConnection, tree_id: Uuid, sex: Sex) -> Uuid {
        let id = Uuid::now_v7();
        PersonRepo::create(db, id, tree_id, sex).await.unwrap();
        id
    }

    async fn create_birth(
        db: &DatabaseConnection,
        tree_id: Uuid,
        person_id: Uuid,
        place_id: Uuid,
    ) -> Uuid {
        let id = Uuid::now_v7();
        EventRepo::create(
            db,
            id,
            tree_id,
            EventType::Birth,
            Some("1900".into()),
            None,
            Some(place_id),
            Some(person_id),
            None,
            None,
            DateQualifier::default(),
            None,
            Calendar::default(),
            None,
        )
        .await
        .unwrap();
        id
    }

    async fn create_media(db: &DatabaseConnection, tree_id: Uuid, name: &str) -> Uuid {
        let id = Uuid::now_v7();
        MediaRepo::create(
            db,
            id,
            tree_id,
            name.into(),
            "image/jpeg".into(),
            name.into(),
            0,
            None,
            None,
        )
        .await
        .unwrap();
        id
    }

    #[tokio::test]
    async fn bundle_limits_records_to_the_relevant_family_neighborhood() {
        let db = connect("sqlite::memory:").await.unwrap();
        run_migrations(&db).await.unwrap();
        let tree_id = Uuid::now_v7();
        TreeRepo::create(&db, tree_id, "Fictional Tree".into(), None)
            .await
            .unwrap();

        let father = create_person(&db, tree_id, Sex::Male).await;
        let mother = create_person(&db, tree_id, Sex::Female).await;
        let other_parent = create_person(&db, tree_id, Sex::Unknown).await;
        let target = create_person(&db, tree_id, Sex::Unknown).await;
        let sibling = create_person(&db, tree_id, Sex::Unknown).await;
        let half_sibling = create_person(&db, tree_id, Sex::Unknown).await;
        let unrelated = create_person(&db, tree_id, Sex::Unknown).await;

        let direct_family = Uuid::now_v7();
        FamilyRepo::create(&db, direct_family, tree_id)
            .await
            .unwrap();
        for (person_id, role, order) in [
            (father, SpouseRole::Husband, 0),
            (mother, SpouseRole::Wife, 1),
        ] {
            FamilySpouseRepo::create(&db, Uuid::now_v7(), direct_family, person_id, role, order)
                .await
                .unwrap();
        }
        for (person_id, order) in [(target, 0), (sibling, 1)] {
            FamilyChildRepo::create(
                &db,
                Uuid::now_v7(),
                direct_family,
                person_id,
                ChildType::Biological,
                order,
            )
            .await
            .unwrap();
        }

        let other_family = Uuid::now_v7();
        FamilyRepo::create(&db, other_family, tree_id)
            .await
            .unwrap();
        for (person_id, role, order) in [
            (father, SpouseRole::Husband, 0),
            (other_parent, SpouseRole::Partner, 1),
        ] {
            FamilySpouseRepo::create(&db, Uuid::now_v7(), other_family, person_id, role, order)
                .await
                .unwrap();
        }
        FamilyChildRepo::create(
            &db,
            Uuid::now_v7(),
            other_family,
            half_sibling,
            ChildType::Biological,
            0,
        )
        .await
        .unwrap();

        let partner = create_person(&db, tree_id, Sex::Unknown).await;
        let own_family = Uuid::now_v7();
        FamilyRepo::create(&db, own_family, tree_id).await.unwrap();
        for (person_id, order) in [(target, 0), (partner, 1)] {
            FamilySpouseRepo::create(
                &db,
                Uuid::now_v7(),
                own_family,
                person_id,
                SpouseRole::Partner,
                order,
            )
            .await
            .unwrap();
        }

        let direct_media = create_media(&db, tree_id, "direct.jpg").await;
        let family_media = create_media(&db, tree_id, "family.jpg").await;
        let unrelated_media = create_media(&db, tree_id, "unrelated.jpg").await;
        for (media_id, person_id, family_id) in [
            (direct_media, Some(target), None),
            (family_media, None, Some(own_family)),
            (unrelated_media, Some(unrelated), None),
        ] {
            MediaLinkRepo::create(
                &db,
                Uuid::now_v7(),
                media_id,
                person_id,
                None,
                None,
                family_id,
                0,
            )
            .await
            .unwrap();
        }

        let relevant_place = Uuid::now_v7();
        PlaceRepo::create(
            &db,
            relevant_place,
            tree_id,
            "Example City".into(),
            None,
            None,
        )
        .await
        .unwrap();
        let unrelated_place = Uuid::now_v7();
        PlaceRepo::create(
            &db,
            unrelated_place,
            tree_id,
            "Other City".into(),
            None,
            None,
        )
        .await
        .unwrap();
        let relevant_event = create_birth(&db, tree_id, half_sibling, relevant_place).await;
        let unrelated_event = create_birth(&db, tree_id, unrelated, unrelated_place).await;

        let relevant_source = Uuid::now_v7();
        SourceRepo::create(
            &db,
            relevant_source,
            tree_id,
            "Relevant Register".into(),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let unrelated_source = Uuid::now_v7();
        SourceRepo::create(
            &db,
            unrelated_source,
            tree_id,
            "Unrelated Register".into(),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        CitationRepo::create(
            &db,
            Uuid::now_v7(),
            relevant_source,
            None,
            Some(relevant_event),
            None,
            None,
            Confidence::High,
            None,
        )
        .await
        .unwrap();
        CitationRepo::create(
            &db,
            Uuid::now_v7(),
            unrelated_source,
            None,
            Some(unrelated_event),
            None,
            None,
            Confidence::Low,
            None,
        )
        .await
        .unwrap();

        let media_root = tempfile::tempdir().unwrap();
        let store: Arc<dyn MediaStore> = Arc::new(FsStore::new(media_root.path()));
        let bundle = load_person_detail_bundle(&db, &store, tree_id, target)
            .await
            .unwrap();

        let person_ids = bundle
            .persons
            .iter()
            .map(|person| person.id)
            .collect::<HashSet<_>>();
        assert!(person_ids.contains(&target));
        assert!(person_ids.contains(&sibling));
        assert!(person_ids.contains(&half_sibling));
        assert!(!person_ids.contains(&unrelated));
        assert!(bundle.events.iter().any(|event| event.id == relevant_event));
        assert!(
            !bundle
                .events
                .iter()
                .any(|event| event.id == unrelated_event)
        );
        assert_eq!(
            bundle
                .places
                .iter()
                .map(|place| place.id)
                .collect::<Vec<_>>(),
            vec![relevant_place]
        );
        assert_eq!(
            bundle
                .sources
                .iter()
                .map(|source| source.id)
                .collect::<Vec<_>>(),
            vec![relevant_source]
        );
        assert_eq!(bundle.citations.len(), 1);
        assert!(bundle.event_media.is_empty());
        assert!(bundle.profile_vignettes.is_empty());
        let profile_media_ids = bundle
            .profile_media
            .iter()
            .map(|item| item.media.id)
            .collect::<HashSet<_>>();
        assert_eq!(
            profile_media_ids,
            HashSet::from([direct_media, family_media])
        );
        let gallery_media_ids = bundle
            .gallery
            .media
            .iter()
            .map(|item| item.media_id)
            .collect::<HashSet<_>>();
        assert_eq!(gallery_media_ids, profile_media_ids);
    }
}
