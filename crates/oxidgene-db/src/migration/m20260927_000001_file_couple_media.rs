//! File the Geneanet media that document a couple event under the couple.
//!
//! Geneanet attaches a media only to people, so a wedding photograph arrived
//! as a reference on each spouse carrying the marriage. Imports before this
//! change linked the media to each spouse and to the marriage, never to the
//! family, so the photograph showed in each spouse's own media and the
//! couple's stayed empty. The import now files such media under the couple;
//! this migration repairs trees imported before it, by the same rule.
//!
//! A person link to a Geneanet media is replaced by a link to family `F`
//! when the media is linked to an event of `F` (an event stored on the
//! family, not on a person) and the person is a spouse of `F`. The spouse
//! keeps their own link when the media holds their portrait. The import's
//! other exception, another reference of theirs without a couple event, left
//! no trace in the database: a media links to one event at most, so the
//! reference that lost that race is not recorded anywhere.
//!
//! Only media imported from Geneanet are touched: a page the import named
//! `geneanet-…`, or a document holding one. Older imports linked the page
//! itself, newer ones its document; both are repaired. A photograph a user
//! linked to both a spouse and the marriage by hand is theirs to arrange.
//!
//! Media links count in each person's projection (`media_count`, the
//! fallback primary media), so the accompanying `PROJECTION_SCHEMA_VERSION`
//! bump rebuilds them on the next read. The change is data only and is not
//! undone by `down`: the links it removes cannot be told apart afterwards from
//! links that never existed.

use std::collections::{HashMap, HashSet};

use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, Set,
};
use uuid::Uuid;

use crate::entities::{event, family_spouse, media, media_link, person, vignette};
use crate::repo::batch::MAX_BOUND_IDS;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        file_couple_media(manager.get_connection())
            .await
            .map(|_| ())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

/// What the repair changed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Filed {
    pub family_links: usize,
    pub person_links_removed: usize,
}

/// Apply the rule to every tree; see the module documentation.
pub async fn file_couple_media(db: &impl ConnectionTrait) -> Result<Filed, DbErr> {
    let mut couple_of_media = couples_of_event_media(db).await?;
    // Only Geneanet imports.
    let candidate_media: Vec<Uuid> = couple_of_media.keys().copied().collect();
    let (geneanet_media, document_of_page) = geneanet_media(db, &candidate_media).await?;
    couple_of_media.retain(|media_id, _| geneanet_media.contains(media_id));
    if couple_of_media.is_empty() {
        return Ok(Filed::default());
    }

    let candidate_media: Vec<Uuid> = couple_of_media.keys().copied().collect();
    let person_links = in_chunks(&candidate_media, |chunk| {
        media_link::Entity::find()
            .filter(media_link::Column::MediaId.is_in(chunk))
            .all(db)
    })
    .await?;
    let spouses = spouses_of(db, &couple_of_media).await?;
    let portraits = Portraits::load(db, &person_links, document_of_page).await?;

    let mut existing_family_links: HashSet<(Uuid, Uuid)> = person_links
        .iter()
        .filter_map(|link| Some((link.media_id, link.family_id?)))
        .collect();
    let mut filed = Filed::default();
    for link in &person_links {
        let Some((person_id, &family_id)) = link.person_id.zip(couple_of_media.get(&link.media_id))
        else {
            continue;
        };
        if !spouses.contains(&(family_id, person_id)) || portraits.is_of(person_id, link.media_id) {
            continue;
        }
        if existing_family_links.insert((link.media_id, family_id)) {
            link_to_family(db, link.media_id, family_id).await?;
            filed.family_links += 1;
        }
        media_link::Entity::delete_by_id(link.id).exec(db).await?;
        filed.person_links_removed += 1;
    }
    Ok(filed)
}

/// Media linked to an event of a couple (stored on the family, not on a
/// person), with that couple.
async fn couples_of_event_media(db: &impl ConnectionTrait) -> Result<HashMap<Uuid, Uuid>, DbErr> {
    let event_links = media_link::Entity::find()
        .filter(media_link::Column::EventId.is_not_null())
        .all(db)
        .await?;
    let event_ids: Vec<Uuid> = event_links.iter().filter_map(|l| l.event_id).collect();
    let events: HashMap<Uuid, event::Model> = in_chunks(&event_ids, |chunk| {
        event::Entity::find()
            .filter(event::Column::Id.is_in(chunk))
            .filter(event::Column::DeletedAt.is_null())
            .all(db)
    })
    .await?
    .into_iter()
    .map(|event| (event.id, event))
    .collect();
    Ok(event_links
        .iter()
        .filter_map(|link| {
            let event = events.get(&link.event_id?)?;
            match (event.person_id, event.family_id) {
                (None, Some(family_id)) => Some((link.media_id, family_id)),
                _ => None,
            }
        })
        .collect())
}

/// Among `candidates`, the media of a Geneanet import — a page the import
/// named, or a document holding one — and the document of each of their
/// pages.
async fn geneanet_media(
    db: &impl ConnectionTrait,
    candidates: &[Uuid],
) -> Result<(HashSet<Uuid>, HashMap<Uuid, Uuid>), DbErr> {
    let is_geneanet = |row: &media::Model| row.file_name.starts_with("geneanet-");
    let named_pages = in_chunks(candidates, |chunk| {
        media::Entity::find()
            .filter(media::Column::Id.is_in(chunk))
            .all(db)
    })
    .await?;
    let pages = in_chunks(candidates, |chunk| {
        media::Entity::find()
            .filter(media::Column::ParentMediaId.is_in(chunk))
            .all(db)
    })
    .await?;
    let geneanet: HashSet<Uuid> = named_pages
        .iter()
        .filter(|row| is_geneanet(row))
        .map(|row| row.id)
        .chain(
            pages
                .iter()
                .filter(|page| is_geneanet(page))
                .filter_map(|page| page.parent_media_id),
        )
        .collect();
    let document_of_page = pages
        .iter()
        .filter_map(|page| Some((page.id, page.parent_media_id?)))
        .collect();
    Ok((geneanet, document_of_page))
}

/// The (family, person) spouse pairs of the couples in `couple_of_media`.
async fn spouses_of(
    db: &impl ConnectionTrait,
    couple_of_media: &HashMap<Uuid, Uuid>,
) -> Result<HashSet<(Uuid, Uuid)>, DbErr> {
    let families: Vec<Uuid> = couple_of_media
        .values()
        .copied()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    Ok(in_chunks(&families, |chunk| {
        family_spouse::Entity::find()
            .filter(family_spouse::Column::FamilyId.is_in(chunk))
            .all(db)
    })
    .await?
    .into_iter()
    .map(|spouse| (spouse.family_id, spouse.person_id))
    .collect())
}

/// The portraits of the persons linked to the candidate media.
struct Portraits {
    persons: HashMap<Uuid, person::Model>,
    /// The media each portrait vignette boxes.
    vignette_media: HashMap<Uuid, Uuid>,
    document_of_page: HashMap<Uuid, Uuid>,
}

impl Portraits {
    async fn load(
        db: &impl ConnectionTrait,
        person_links: &[media_link::Model],
        document_of_page: HashMap<Uuid, Uuid>,
    ) -> Result<Self, DbErr> {
        let linked_people: Vec<Uuid> = person_links.iter().filter_map(|l| l.person_id).collect();
        let persons: HashMap<Uuid, person::Model> = in_chunks(&linked_people, |chunk| {
            person::Entity::find()
                .filter(person::Column::Id.is_in(chunk))
                .all(db)
        })
        .await?
        .into_iter()
        .map(|person| (person.id, person))
        .collect();
        let portrait_vignettes: Vec<Uuid> = persons
            .values()
            .filter_map(|person| person.portrait_vignette_id)
            .collect();
        let vignette_media = in_chunks(&portrait_vignettes, |chunk| {
            vignette::Entity::find()
                .filter(vignette::Column::Id.is_in(chunk))
                .all(db)
        })
        .await?
        .into_iter()
        .map(|vignette| (vignette.id, vignette.media_id))
        .collect();
        Ok(Self {
            persons,
            vignette_media,
            document_of_page,
        })
    }

    /// Whether the person's portrait is this media: the whole of it, or a
    /// box on it or on one of its pages.
    fn is_of(&self, person_id: Uuid, media_id: Uuid) -> bool {
        let Some(person) = self.persons.get(&person_id) else {
            return false;
        };
        let boxed = person
            .portrait_vignette_id
            .and_then(|id| self.vignette_media.get(&id).copied());
        person.portrait_media_id == Some(media_id)
            || boxed == Some(media_id)
            || boxed.and_then(|page| self.document_of_page.get(&page).copied()) == Some(media_id)
    }
}

/// Link `media_id` to family `family_id`.
async fn link_to_family(
    db: &impl ConnectionTrait,
    media_id: Uuid,
    family_id: Uuid,
) -> Result<(), DbErr> {
    media_link::ActiveModel {
        id: Set(Uuid::now_v7()),
        media_id: Set(media_id),
        person_id: Set(None),
        event_id: Set(None),
        source_id: Set(None),
        family_id: Set(Some(family_id)),
        sort_order: Set(0),
    }
    .insert(db)
    .await?;
    Ok(())
}

/// Run `query` over `ids` one bounded slice at a time.
async fn in_chunks<T, F, Fut>(ids: &[Uuid], mut query: F) -> Result<Vec<T>, DbErr>
where
    F: FnMut(Vec<Uuid>) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<T>, DbErr>>,
{
    let mut rows = Vec::new();
    for chunk in ids.chunks(MAX_BOUND_IDS) {
        rows.extend(query(chunk.to_vec()).await?);
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::{
        EventRepo, FamilyRepo, FamilySpouseRepo, MediaLinkRepo, MediaRepo, PersonRepo, TreeRepo,
        connect, run_migrations,
    };
    use chrono::Utc;
    use oxidgene_core::enums::{Calendar, DateQualifier, EventType, Sex, SpouseRole};
    use oxidgene_core::types::Portrait;
    use sea_orm::DatabaseConnection;

    struct Couple {
        tree_id: Uuid,
        husband: Uuid,
        wife: Uuid,
        family: Uuid,
        marriage: Uuid,
    }

    async fn couple(db: &DatabaseConnection) -> Couple {
        let tree_id = Uuid::now_v7();
        TreeRepo::create(db, tree_id, "Fictional Tree".into(), None)
            .await
            .unwrap();
        let (husband, wife, family) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
        PersonRepo::create(db, husband, tree_id, Sex::Male)
            .await
            .unwrap();
        PersonRepo::create(db, wife, tree_id, Sex::Female)
            .await
            .unwrap();
        FamilyRepo::create(db, family, tree_id).await.unwrap();
        for (order, (person_id, role)) in [(husband, SpouseRole::Husband), (wife, SpouseRole::Wife)]
            .into_iter()
            .enumerate()
        {
            FamilySpouseRepo::create(db, Uuid::now_v7(), family, person_id, role, order as i32)
                .await
                .unwrap();
        }
        let marriage = event(db, tree_id, EventType::Marriage, None, Some(family)).await;
        Couple {
            tree_id,
            husband,
            wife,
            family,
            marriage,
        }
    }

    async fn event(
        db: &DatabaseConnection,
        tree_id: Uuid,
        event_type: EventType,
        person_id: Option<Uuid>,
        family_id: Option<Uuid>,
    ) -> Uuid {
        let id = Uuid::now_v7();
        EventRepo::create(
            db,
            id,
            tree_id,
            event_type,
            Some("1890".into()),
            None,
            None,
            person_id,
            family_id,
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

    /// A one-page document, its page named as the import names it.
    async fn document(db: &DatabaseConnection, tree_id: Uuid, page_name: &str) -> (Uuid, Uuid) {
        let (document_id, page_id) = (Uuid::now_v7(), Uuid::now_v7());
        MediaRepo::create_document(db, document_id, tree_id, Some("Wedding".into()), Utc::now())
            .await
            .unwrap();
        MediaRepo::create(
            db,
            page_id,
            tree_id,
            Some(document_id),
            page_name.into(),
            "image/jpeg".into(),
            page_name.into(),
            0,
            None,
            None,
        )
        .await
        .unwrap();
        (document_id, page_id)
    }

    async fn link(
        db: &DatabaseConnection,
        media_id: Uuid,
        person_id: Option<Uuid>,
        event_id: Option<Uuid>,
    ) {
        MediaLinkRepo::create(
            db,
            Uuid::now_v7(),
            media_id,
            person_id,
            event_id,
            None,
            None,
            0,
        )
        .await
        .unwrap();
    }

    async fn owners(db: &DatabaseConnection, media_id: Uuid) -> Vec<(Option<Uuid>, Option<Uuid>)> {
        let mut owners: Vec<_> = media_link::Entity::find()
            .filter(media_link::Column::MediaId.eq(media_id))
            .filter(media_link::Column::EventId.is_null())
            .all(db)
            .await
            .unwrap()
            .into_iter()
            .map(|link| (link.person_id, link.family_id))
            .collect();
        owners.sort();
        owners
    }

    async fn setup() -> DatabaseConnection {
        let db = connect("sqlite::memory:").await.unwrap();
        run_migrations(&db).await.unwrap();
        db
    }

    #[tokio::test]
    async fn a_geneanet_wedding_photograph_moves_to_the_couple() {
        let db = setup().await;
        let c = couple(&db).await;
        let guest = Uuid::now_v7();
        PersonRepo::create(&db, guest, c.tree_id, Sex::Unknown)
            .await
            .unwrap();
        let (media, _) = document(&db, c.tree_id, "geneanet-7.jpg").await;
        link(&db, media, None, Some(c.marriage)).await;
        for person in [c.husband, c.wife, guest] {
            link(&db, media, Some(person), None).await;
        }

        let filed = file_couple_media(&db).await.unwrap();

        assert_eq!(
            filed,
            Filed {
                family_links: 1,
                person_links_removed: 2
            }
        );
        let mut expected = vec![(Some(guest), None), (None, Some(c.family))];
        expected.sort();
        assert_eq!(owners(&db, media).await, expected);
        // Run again: nothing left to move.
        assert_eq!(file_couple_media(&db).await.unwrap(), Filed::default());
    }

    #[tokio::test]
    async fn an_older_import_linking_the_page_itself_is_repaired_too() {
        let db = setup().await;
        let c = couple(&db).await;
        let (_, page) = document(&db, c.tree_id, "geneanet-11.jpg").await;
        link(&db, page, None, Some(c.marriage)).await;
        link(&db, page, Some(c.husband), None).await;
        link(&db, page, Some(c.wife), None).await;

        let filed = file_couple_media(&db).await.unwrap();

        assert_eq!(
            filed,
            Filed {
                family_links: 1,
                person_links_removed: 2
            }
        );
        assert_eq!(owners(&db, page).await, vec![(None, Some(c.family))]);
    }

    #[tokio::test]
    async fn a_portrait_an_individual_event_or_a_hand_made_link_stays_put() {
        let db = setup().await;
        let c = couple(&db).await;

        // The wedding photograph is the wife's portrait, boxed on its page.
        let (portrait_media, portrait_page) = document(&db, c.tree_id, "geneanet-8.jpg").await;
        link(&db, portrait_media, None, Some(c.marriage)).await;
        link(&db, portrait_media, Some(c.wife), None).await;
        let vignette = crate::repo::VignetteRepo::create(
            &db,
            Uuid::now_v7(),
            crate::repo::VignetteInput {
                media_id: portrait_page,
                x: 0,
                y: 0,
                width: 10,
                height: 10,
                person_id: Some(c.wife),
                event_id: None,
            },
        )
        .await
        .unwrap();
        PersonRepo::set_portrait(&db, c.wife, Portrait::Vignette(vignette.id))
            .await
            .unwrap();

        // Linked to the husband's own birth: not a couple's event.
        let birth = event(&db, c.tree_id, EventType::Birth, Some(c.husband), None).await;
        let (record, _) = document(&db, c.tree_id, "geneanet-9.jpg").await;
        link(&db, record, None, Some(birth)).await;
        link(&db, record, Some(c.husband), None).await;

        // Attached by hand, not imported from Geneanet.
        let (hand_made, _) = document(&db, c.tree_id, "scan.jpg").await;
        let other_marriage = event(&db, c.tree_id, EventType::Marriage, None, Some(c.family)).await;
        link(&db, hand_made, None, Some(other_marriage)).await;
        link(&db, hand_made, Some(c.husband), None).await;

        assert_eq!(file_couple_media(&db).await.unwrap(), Filed::default());
        assert_eq!(
            owners(&db, portrait_media).await,
            vec![(Some(c.wife), None)]
        );
        assert_eq!(owners(&db, record).await, vec![(Some(c.husband), None)]);
        assert_eq!(owners(&db, hand_made).await, vec![(Some(c.husband), None)]);
    }
}
