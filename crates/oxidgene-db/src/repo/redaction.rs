//! The rows a restricted viewer's redaction derives hidden records from
//! (docs/privacy.md §3.2): who owns each event, citation, note, medium,
//! media link and vignette, which source a citation draws on, which sources
//! a repository holds and which records use a place.
//!
//! Each read starts from a set of hidden records — the hidden persons, the
//! withheld families, then what they own — and reads only the rows that name
//! them, a bounded slice of ids at a time. Nothing here decides: the API's
//! `authz::records` applies the policy to these rows.

use oxidgene_core::enums::Privacy;
use oxidgene_core::error::OxidGeneError;
use sea_orm::entity::prelude::*;
use sea_orm::{Condition, ConnectionTrait, QueryFilter, QuerySelect};
use uuid::Uuid;

use crate::entities::{
    citation, event, media, media_link, note, sea_enums, source_repository, vignette,
};
use crate::repo::batch::{MAX_BOUND_IDS, in_chunks};
use crate::repo::db_err;

/// `column NOT IN ids`, one bounded list per slice of ids; always true for
/// no ids. What a paged list adds to leave out the records a viewer may not
/// see, so that its pages and its `total_count` agree.
pub(crate) fn not_in<C: ColumnTrait>(column: C, ids: &[Uuid]) -> Condition {
    ids.chunks(MAX_BOUND_IDS)
        .fold(Condition::all(), |condition, chunk| {
            condition.add(column.is_not_in(chunk.iter().copied()))
        })
}

/// What a paged list leaves out for a viewer who may not see everything,
/// applied in SQL so that the list's pages and its `total_count` agree.
#[derive(Debug, Default, Clone, Copy)]
pub struct LeaveOut<'a> {
    /// The records of the list's kind the viewer may not see, sorted.
    pub ids: &'a [Uuid],
    /// For notes: leave out the confidential ones too.
    pub confidential: bool,
}

impl LeaveOut<'_> {
    /// Nothing left out: the owner's lists.
    pub const NONE: LeaveOut<'static> = LeaveOut {
        ids: &[],
        confidential: false,
    };

    /// The predicate keeping what the viewer may see of a list whose
    /// records' id is `column`.
    pub(crate) fn keeping<C: ColumnTrait>(&self, column: C) -> Condition {
        not_in(column, self.ids)
    }
}

/// An event, with what it belongs to and where it took place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventOwner {
    pub id: Uuid,
    pub person_id: Option<Uuid>,
    pub family_id: Option<Uuid>,
    pub place_id: Option<Uuid>,
}

/// A medium's privacy, document and place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediumOwner {
    pub id: Uuid,
    pub privacy: Privacy,
    /// The document a page belongs to.
    pub document_id: Option<Uuid>,
    pub place_id: Option<Uuid>,
}

/// A media link and the record it attaches its medium to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkOwner {
    pub id: Uuid,
    pub media_id: Uuid,
    pub person_id: Option<Uuid>,
    pub family_id: Option<Uuid>,
    pub event_id: Option<Uuid>,
    pub source_id: Option<Uuid>,
}

/// A vignette and whom it shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VignetteOwner {
    pub id: Uuid,
    pub media_id: Uuid,
    pub person_id: Option<Uuid>,
    pub event_id: Option<Uuid>,
}

/// The records whose notes [`RedactionRepo::notes_of`] reads.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoteOwners<'a> {
    pub persons: &'a [Uuid],
    pub families: &'a [Uuid],
    pub events: &'a [Uuid],
    pub media: &'a [Uuid],
    pub sources: &'a [Uuid],
    pub repositories: &'a [Uuid],
}

/// The reads of the redaction's hidden records.
pub struct RedactionRepo;

type EventRow = (Uuid, Option<Uuid>, Option<Uuid>, Option<Uuid>);
type LinkRow = (
    Uuid,
    Uuid,
    Option<Uuid>,
    Option<Uuid>,
    Option<Uuid>,
    Option<Uuid>,
);

impl RedactionRepo {
    /// The live events of tree `tree_id` that `column` names one of `ids`
    /// in.
    async fn events_by(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        column: event::Column,
        ids: &[Uuid],
    ) -> Result<Vec<EventOwner>, OxidGeneError> {
        in_chunks(ids, |chunk| async move {
            let rows: Vec<EventRow> = event::Entity::find()
                .select_only()
                .column(event::Column::Id)
                .column(event::Column::PersonId)
                .column(event::Column::FamilyId)
                .column(event::Column::PlaceId)
                .filter(event::Column::TreeId.eq(tree_id))
                .filter(event::Column::DeletedAt.is_null())
                .filter(column.is_in(chunk))
                .into_tuple()
                .all(db)
                .await
                .map_err(db_err)?;
            Ok(rows
                .into_iter()
                .map(|(id, person_id, family_id, place_id)| EventOwner {
                    id,
                    person_id,
                    family_id,
                    place_id,
                })
                .collect())
        })
        .await
    }

    /// The live events of tree `tree_id` held by one of `persons` or one of
    /// `families`.
    pub async fn events_of(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        persons: &[Uuid],
        families: &[Uuid],
    ) -> Result<Vec<EventOwner>, OxidGeneError> {
        let mut events = Self::events_by(db, tree_id, event::Column::PersonId, persons).await?;
        events.extend(Self::events_by(db, tree_id, event::Column::FamilyId, families).await?);
        events.sort_by_key(|e| e.id);
        events.dedup_by_key(|e| e.id);
        Ok(events)
    }

    /// The live events of tree `tree_id` that took place at one of `places`.
    pub async fn events_at(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        places: &[Uuid],
    ) -> Result<Vec<EventOwner>, OxidGeneError> {
        Self::events_by(db, tree_id, event::Column::PlaceId, places).await
    }

    /// The citations, with their source, that `column` attaches to one of
    /// `ids`.
    async fn citations_by(
        db: &impl ConnectionTrait,
        column: citation::Column,
        ids: &[Uuid],
    ) -> Result<Vec<(Uuid, Uuid)>, OxidGeneError> {
        in_chunks(ids, |chunk| async move {
            citation::Entity::find()
                .select_only()
                .column(citation::Column::Id)
                .column(citation::Column::SourceId)
                .filter(column.is_in(chunk))
                .into_tuple()
                .all(db)
                .await
                .map_err(db_err)
        })
        .await
    }

    /// The citations, with their source, of one of `persons`, `families`
    /// or `events`.
    pub async fn citations_of(
        db: &impl ConnectionTrait,
        persons: &[Uuid],
        families: &[Uuid],
        events: &[Uuid],
    ) -> Result<Vec<(Uuid, Uuid)>, OxidGeneError> {
        let mut rows = Self::citations_by(db, citation::Column::PersonId, persons).await?;
        rows.extend(Self::citations_by(db, citation::Column::FamilyId, families).await?);
        rows.extend(Self::citations_by(db, citation::Column::EventId, events).await?);
        rows.sort_unstable();
        rows.dedup();
        Ok(rows)
    }

    /// Every citation, with its source, drawn from one of `sources`.
    pub async fn citations_of_sources(
        db: &impl ConnectionTrait,
        sources: &[Uuid],
    ) -> Result<Vec<(Uuid, Uuid)>, OxidGeneError> {
        Self::citations_by(db, citation::Column::SourceId, sources).await
    }

    /// The links `column` names one of `ids` in.
    async fn links_by(
        db: &impl ConnectionTrait,
        column: media_link::Column,
        ids: &[Uuid],
    ) -> Result<Vec<LinkOwner>, OxidGeneError> {
        in_chunks(ids, |chunk| async move {
            let rows: Vec<LinkRow> = media_link::Entity::find()
                .select_only()
                .column(media_link::Column::Id)
                .column(media_link::Column::MediaId)
                .column(media_link::Column::PersonId)
                .column(media_link::Column::FamilyId)
                .column(media_link::Column::EventId)
                .column(media_link::Column::SourceId)
                .filter(column.is_in(chunk))
                .into_tuple()
                .all(db)
                .await
                .map_err(db_err)?;
            Ok(rows
                .into_iter()
                .map(
                    |(id, media_id, person_id, family_id, event_id, source_id)| LinkOwner {
                        id,
                        media_id,
                        person_id,
                        family_id,
                        event_id,
                        source_id,
                    },
                )
                .collect())
        })
        .await
    }

    /// The media links attaching a medium to one of `persons`, `families`,
    /// `events` or `sources`.
    pub async fn links_of(
        db: &impl ConnectionTrait,
        persons: &[Uuid],
        families: &[Uuid],
        events: &[Uuid],
        sources: &[Uuid],
    ) -> Result<Vec<LinkOwner>, OxidGeneError> {
        let mut links = Self::links_by(db, media_link::Column::PersonId, persons).await?;
        links.extend(Self::links_by(db, media_link::Column::FamilyId, families).await?);
        links.extend(Self::links_by(db, media_link::Column::EventId, events).await?);
        links.extend(Self::links_by(db, media_link::Column::SourceId, sources).await?);
        links.sort_by_key(|l| l.id);
        links.dedup_by_key(|l| l.id);
        Ok(links)
    }

    /// The links of one of `media`.
    pub async fn links_of_media(
        db: &impl ConnectionTrait,
        media: &[Uuid],
    ) -> Result<Vec<LinkOwner>, OxidGeneError> {
        Self::links_by(db, media_link::Column::MediaId, media).await
    }

    /// The vignettes `column` names one of `ids` in.
    async fn vignettes_by(
        db: &impl ConnectionTrait,
        column: vignette::Column,
        ids: &[Uuid],
    ) -> Result<Vec<VignetteOwner>, OxidGeneError> {
        in_chunks(ids, |chunk| async move {
            let rows: Vec<(Uuid, Uuid, Option<Uuid>, Option<Uuid>)> = vignette::Entity::find()
                .select_only()
                .column(vignette::Column::Id)
                .column(vignette::Column::MediaId)
                .column(vignette::Column::PersonId)
                .column(vignette::Column::EventId)
                .filter(column.is_in(chunk))
                .into_tuple()
                .all(db)
                .await
                .map_err(db_err)?;
            Ok(rows
                .into_iter()
                .map(|(id, media_id, person_id, event_id)| VignetteOwner {
                    id,
                    media_id,
                    person_id,
                    event_id,
                })
                .collect())
        })
        .await
    }

    /// The vignettes showing one of `persons` or cut for one of `events`,
    /// and those of one of `media`.
    pub async fn vignettes_of(
        db: &impl ConnectionTrait,
        persons: &[Uuid],
        events: &[Uuid],
        media: &[Uuid],
    ) -> Result<Vec<VignetteOwner>, OxidGeneError> {
        let mut vignettes = Self::vignettes_by(db, vignette::Column::PersonId, persons).await?;
        vignettes.extend(Self::vignettes_by(db, vignette::Column::EventId, events).await?);
        vignettes.extend(Self::vignettes_by(db, vignette::Column::MediaId, media).await?);
        vignettes.sort_by_key(|v| v.id);
        vignettes.dedup_by_key(|v| v.id);
        Ok(vignettes)
    }

    /// The live media `condition` selects in tree `tree_id`.
    async fn media_where(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        condition: Condition,
    ) -> Result<Vec<MediumOwner>, OxidGeneError> {
        let rows: Vec<(Uuid, sea_enums::Privacy, Option<Uuid>, Option<Uuid>)> =
            media::Entity::find()
                .select_only()
                .column(media::Column::Id)
                .column(media::Column::Privacy)
                .column(media::Column::ParentMediaId)
                .column(media::Column::PlaceId)
                .filter(media::Column::TreeId.eq(tree_id))
                .filter(media::Column::DeletedAt.is_null())
                .filter(condition)
                .into_tuple()
                .all(db)
                .await
                .map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|(id, privacy, document_id, place_id)| MediumOwner {
                id,
                privacy: privacy.into(),
                document_id,
                place_id,
            })
            .collect())
    }

    /// The live media of tree `tree_id` whose `column` is one of `ids`.
    async fn media_by(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        column: media::Column,
        ids: &[Uuid],
    ) -> Result<Vec<MediumOwner>, OxidGeneError> {
        in_chunks(ids, |chunk| async move {
            Self::media_where(db, tree_id, Condition::all().add(column.is_in(chunk))).await
        })
        .await
    }

    /// The live media among `ids`.
    pub async fn media(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        ids: &[Uuid],
    ) -> Result<Vec<MediumOwner>, OxidGeneError> {
        Self::media_by(db, tree_id, media::Column::Id, ids).await
    }

    /// The live pages of one of `documents`.
    pub async fn pages_of(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        documents: &[Uuid],
    ) -> Result<Vec<MediumOwner>, OxidGeneError> {
        Self::media_by(db, tree_id, media::Column::ParentMediaId, documents).await
    }

    /// The live media of tree `tree_id` that took place at one of `places`.
    pub async fn media_at(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        places: &[Uuid],
    ) -> Result<Vec<MediumOwner>, OxidGeneError> {
        Self::media_by(db, tree_id, media::Column::PlaceId, places).await
    }

    /// The live media of tree `tree_id` not explicitly public: those a
    /// viewer without `read_private` may find private.
    pub async fn unpublished_media(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<MediumOwner>, OxidGeneError> {
        Self::media_where(
            db,
            tree_id,
            Condition::all().add(media::Column::Privacy.ne(sea_enums::Privacy::Public)),
        )
        .await
    }

    /// The links between sources and repositories `column` names one of
    /// `ids` in, as `(source, repository)`.
    async fn holdings_by(
        db: &impl ConnectionTrait,
        column: source_repository::Column,
        ids: &[Uuid],
    ) -> Result<Vec<(Uuid, Uuid)>, OxidGeneError> {
        in_chunks(ids, |chunk| async move {
            source_repository::Entity::find()
                .select_only()
                .column(source_repository::Column::SourceId)
                .column(source_repository::Column::RepositoryId)
                .filter(column.is_in(chunk))
                .into_tuple()
                .all(db)
                .await
                .map_err(db_err)
        })
        .await
    }

    /// The holdings of one of `sources`, as `(source, repository)`.
    pub async fn holdings_of_sources(
        db: &impl ConnectionTrait,
        sources: &[Uuid],
    ) -> Result<Vec<(Uuid, Uuid)>, OxidGeneError> {
        Self::holdings_by(db, source_repository::Column::SourceId, sources).await
    }

    /// The holdings of one of `repositories`, as `(source, repository)`.
    pub async fn holdings_of_repositories(
        db: &impl ConnectionTrait,
        repositories: &[Uuid],
    ) -> Result<Vec<(Uuid, Uuid)>, OxidGeneError> {
        Self::holdings_by(db, source_repository::Column::RepositoryId, repositories).await
    }

    /// The live notes of tree `tree_id` about one of the records `owners`
    /// lists: persons, families, events, media, sources and repositories.
    pub async fn notes_of(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        owners: &NoteOwners<'_>,
    ) -> Result<Vec<Uuid>, OxidGeneError> {
        let mut notes = Vec::new();
        for (column, ids) in [
            (note::Column::PersonId, owners.persons),
            (note::Column::FamilyId, owners.families),
            (note::Column::EventId, owners.events),
            (note::Column::MediaId, owners.media),
            (note::Column::SourceId, owners.sources),
            (note::Column::RepositoryId, owners.repositories),
        ] {
            notes.extend(Self::notes_by(db, tree_id, column, ids).await?);
        }
        notes.sort_unstable();
        notes.dedup();
        Ok(notes)
    }

    /// The live notes of tree `tree_id` that `column` attaches to one of
    /// `ids`.
    async fn notes_by(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        column: note::Column,
        ids: &[Uuid],
    ) -> Result<Vec<Uuid>, OxidGeneError> {
        in_chunks(ids, |chunk| async move {
            note::Entity::find()
                .select_only()
                .column(note::Column::Id)
                .filter(note::Column::TreeId.eq(tree_id))
                .filter(note::Column::DeletedAt.is_null())
                .filter(column.is_in(chunk))
                .into_tuple()
                .all(db)
                .await
                .map_err(db_err)
        })
        .await
    }
}
