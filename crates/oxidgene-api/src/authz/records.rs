//! What a viewer sees of a tree's records (docs/privacy.md §3.2): the
//! events, citations, notes, media, media links, vignettes, sources,
//! repositories and places a redaction hides, and the tags it withholds.
//!
//! [`Records`] pairs the request's [`Redaction`] with the [`HiddenRecords`]
//! it implies, read once per request and tree in one statement
//! ([`RedactionRepo::hidden_records`]: the rules of `oxidgene_db`'s
//! `hiding` module, as common table expressions) and kept in the
//! [`ViewerContext`]. The owner's is unrestricted and costs no query; so is
//! a restricted viewer's on a tree where nobody is hidden — then only
//! confidential notes and tags can be withheld, which need no read either.
//!
//! The rules, from what hides a person or withholds a family:
//!
//! - an event of a hidden person or of a withheld family is hidden;
//! - a citation on a hidden person, a withheld family or a hidden event is
//!   hidden;
//! - a medium is hidden as `Redaction::medium_withheld` decides from the
//!   persons and families it shows — linked to it, on one of its vignettes,
//!   through a linked event or a vignette's event — and its privacy; a page
//!   follows its document;
//! - a source is hidden when every citation drawn from it is, a repository
//!   when every source it holds is, a place when every event and medium at
//!   it is;
//! - a note is withheld when confidential to a viewer without
//!   `read_confidential`, or when the record it is about is hidden;
//! - a media link is hidden with its medium or with the record it attaches
//!   it to, a vignette with its medium, the person it shows or its event.
//!
//! Every read of these records goes through here: a direct read of a hidden
//! one is `404 not_found`, a list leaves it out — in SQL where the list is
//! paged or counted ([`Records::leave_out`]) — and a record shown keeps its
//! tags only for a viewer holding `read_tags`.

use std::sync::Arc;

use oxidgene_core::OxidGeneError;
use oxidgene_core::access::ViewerContext;
use oxidgene_core::redaction::Redaction;
use oxidgene_core::types::{
    Citation, Event, Family, ImageSource, Media, MediaLink, Note, Place, RecordKind, Repository,
    Source, TagRef, Vignette,
};
use oxidgene_db::repo::{LeaveOut, RedactionRepo, Unseen};
use sea_orm::DatabaseConnection;
use tokio::sync::OnceCell;
use uuid::Uuid;

use crate::profile::ProfileService;

/// The records of one tree a restricted viewer may not see, each kind
/// sorted, for lookups and SQL predicates alike.
#[derive(Debug, Default)]
pub struct HiddenRecords {
    events: Vec<Uuid>,
    citations: Vec<Uuid>,
    /// Notes about hidden records; confidential ones are decided per note.
    notes: Vec<Uuid>,
    media: Vec<Uuid>,
    sources: Vec<Uuid>,
    repositories: Vec<Uuid>,
    places: Vec<Uuid>,
    media_links: Vec<Uuid>,
    vignettes: Vec<Uuid>,
}

/// The slot of a request's [`HiddenRecords`] for one tree.
type Slot = OnceCell<Arc<HiddenRecords>>;

fn holds(ids: &[Uuid], id: Uuid) -> bool {
    ids.binary_search(&id).is_ok()
}

/// A record whose visibility [`Records`] decides.
pub trait Redactable {
    /// The kind of record.
    const KIND: RecordKind;

    fn record_id(&self) -> Uuid;

    /// The tags it carries, which a viewer without `read_tags` does not
    /// see.
    fn tags_mut(&mut self) -> &mut Vec<TagRef>;

    /// Whether it is a confidential note.
    fn confidential(&self) -> bool {
        false
    }
}

macro_rules! redactable {
    ($($type:ty => $kind:ident),* $(,)?) => {
        $(impl Redactable for $type {
            const KIND: RecordKind = RecordKind::$kind;

            fn record_id(&self) -> Uuid {
                self.id
            }

            fn tags_mut(&mut self) -> &mut Vec<TagRef> {
                &mut self.tags
            }
        })*
    };
}

impl Redactable for Note {
    const KIND: RecordKind = RecordKind::Note;

    fn record_id(&self) -> Uuid {
        self.id
    }

    fn tags_mut(&mut self) -> &mut Vec<TagRef> {
        &mut self.tags
    }

    fn confidential(&self) -> bool {
        self.confidential
    }
}

redactable!(
    Event => Event,
    Citation => Citation,
    Media => Media,
    Source => Source,
    Repository => Repository,
    Place => Place,
    Family => Family,
);

/// What one viewer sees of one tree's records, for one request.
#[derive(Debug, Clone)]
pub struct Records {
    redaction: Arc<Redaction>,
    hidden: Arc<HiddenRecords>,
    /// What the lists leave out in SQL.
    unseen: Unseen,
}

impl Records {
    /// What `viewer` sees of the records of tree `tree_id`.
    ///
    /// # Errors
    ///
    /// What resolving the viewer's access answers, and database failures.
    pub async fn of(
        db: &DatabaseConnection,
        profiles: &ProfileService,
        viewer: &ViewerContext,
        tree_id: Uuid,
    ) -> Result<Self, OxidGeneError> {
        let redaction = super::redaction::redaction(db, profiles, viewer, tree_id).await?;
        let unseen = Unseen::new(tree_id, redaction.rules());
        if redaction.is_unrestricted() || nothing_hidden(&redaction) {
            return Ok(Self {
                redaction,
                hidden: Arc::default(),
                unseen,
            });
        }
        let slot = viewer.derived::<Slot>(tree_id);
        let hidden = slot
            .get_or_try_init(|| async { hidden_records(db, &unseen).await.map(Arc::new) })
            .await?;
        Ok(Self {
            hidden: Arc::clone(hidden),
            redaction,
            unseen,
        })
    }

    /// What the owner sees: everything. For the callers that act for the
    /// local owner without a request.
    #[must_use]
    pub fn unrestricted() -> Self {
        Self {
            redaction: Arc::new(Redaction::unrestricted()),
            hidden: Arc::default(),
            unseen: Unseen::default(),
        }
    }

    /// Whether the viewer sees everything: the owner.
    #[must_use]
    pub fn is_unrestricted(&self) -> bool {
        self.redaction.is_unrestricted()
    }

    /// The viewer's redaction.
    #[must_use]
    pub fn redaction(&self) -> &Redaction {
        &self.redaction
    }

    /// The hidden records of `kind`, sorted.
    #[must_use]
    pub fn hidden(&self, kind: RecordKind) -> &[Uuid] {
        let h = &self.hidden;
        match kind {
            RecordKind::Event => &h.events,
            RecordKind::Citation => &h.citations,
            RecordKind::Note => &h.notes,
            RecordKind::Media => &h.media,
            RecordKind::Source => &h.sources,
            RecordKind::Repository => &h.repositories,
            RecordKind::Place => &h.places,
            // A family stays, its content withheld; a person is the
            // redaction's own decision.
            RecordKind::Family | RecordKind::Person => &[],
        }
    }

    /// What a list leaves out in SQL: the persons, families and records
    /// the viewer may not see, selected by the rules that hide them rather
    /// than listed, so that its bound values stay the same however many
    /// are hidden.
    #[must_use]
    pub fn unseen(&self) -> Unseen {
        self.unseen
    }

    /// What a paged list of `kind` leaves out, in SQL: a tag's records and
    /// counts as well, where a person not shown in full and a family whose
    /// content is withheld are left out too.
    #[must_use]
    pub fn leave_out(&self, kind: RecordKind) -> LeaveOut {
        LeaveOut {
            unseen: self.unseen,
            confidential: kind == RecordKind::Note && self.redaction.withholds_confidential(),
        }
    }

    /// Whether record `id` of `kind` is hidden. A person is hidden when not
    /// shown in full; a family never is, though its content may be withheld.
    #[must_use]
    pub fn hides(&self, kind: RecordKind, id: Uuid) -> bool {
        match kind {
            RecordKind::Person => !self.redaction.shows(id),
            _ => holds(self.hidden(kind), id),
        }
    }

    /// Whether `note` is withheld: confidential, or about a hidden record.
    #[must_use]
    pub fn hides_note(&self, note: &Note) -> bool {
        self.redaction
            .note_withheld(note.confidential, holds(&self.hidden.notes, note.id))
    }

    /// Whether `record` is hidden from the viewer.
    #[must_use]
    pub fn hides_record<T: Redactable>(&self, record: &T) -> bool {
        if T::KIND == RecordKind::Note {
            return self.redaction.note_withheld(
                record.confidential(),
                holds(&self.hidden.notes, record.record_id()),
            );
        }
        self.hides(T::KIND, record.record_id())
    }

    /// The media links the viewer may not see, sorted.
    #[must_use]
    pub fn hidden_media_links(&self) -> &[Uuid] {
        &self.hidden.media_links
    }

    /// Whether media link `id` is hidden: its medium is, or the record it
    /// attaches it to.
    #[must_use]
    pub fn hides_media_link(&self, id: Uuid) -> bool {
        holds(&self.hidden.media_links, id)
    }

    /// Whether vignette `id` is hidden: its medium is, the person it shows
    /// or its event.
    #[must_use]
    pub fn hides_vignette(&self, id: Uuid) -> bool {
        holds(&self.hidden.vignettes, id)
    }

    /// Whether the picture `source` draws is hidden.
    #[must_use]
    pub fn hides_image(&self, source: &ImageSource) -> bool {
        match source {
            ImageSource::Remote { .. } => false,
            ImageSource::Thumbnail { media_id } | ImageSource::Card { media_id } => {
                holds(&self.hidden.media, *media_id)
            }
            ImageSource::Crop { vignette_id } | ImageSource::CardCrop { vignette_id } => {
                holds(&self.hidden.vignettes, *vignette_id)
            }
        }
    }

    /// `links` less those the viewer may not see.
    #[must_use]
    pub fn media_links(&self, mut links: Vec<MediaLink>) -> Vec<MediaLink> {
        links.retain(|link| !self.hides_media_link(link.id));
        links
    }

    /// `vignettes` less those the viewer may not see.
    #[must_use]
    pub fn vignettes(&self, mut vignettes: Vec<Vignette>) -> Vec<Vignette> {
        vignettes.retain(|vignette| !self.hides_vignette(vignette.id));
        vignettes
    }

    /// `vignette`, or `404 not_found` when the viewer may not see it.
    ///
    /// # Errors
    ///
    /// [`OxidGeneError::NotFound`] for a hidden vignette.
    pub fn require_vignette(&self, vignette: Vignette) -> Result<Vignette, OxidGeneError> {
        if self.hides_vignette(vignette.id) {
            return Err(OxidGeneError::NotFound {
                entity: "Vignette",
                id: vignette.id,
            });
        }
        Ok(vignette)
    }

    /// `record` as the viewer sees it: `None` when hidden, its tags left out
    /// for a viewer without `read_tags`.
    #[must_use]
    pub fn show<T: Redactable>(&self, mut record: T) -> Option<T> {
        if self.is_unrestricted() {
            return Some(record);
        }
        if self.hides_record(&record) {
            return None;
        }
        self.strip_tags(&mut record);
        Some(record)
    }

    /// `record`, or `404 not_found` when the viewer may not see it.
    ///
    /// # Errors
    ///
    /// [`OxidGeneError::NotFound`] for a hidden record.
    pub fn require<T: Redactable>(&self, record: T) -> Result<T, OxidGeneError> {
        let id = record.record_id();
        self.show(record).ok_or(OxidGeneError::NotFound {
            entity: kind_name(T::KIND),
            id,
        })
    }

    /// Check that record `id` of `kind` is one the viewer may see:
    /// `404 not_found` otherwise.
    ///
    /// # Errors
    ///
    /// [`OxidGeneError::NotFound`] for a hidden record.
    pub fn require_id(&self, kind: RecordKind, id: Uuid) -> Result<(), OxidGeneError> {
        if self.hides(kind, id) {
            return Err(OxidGeneError::NotFound {
                entity: kind_name(kind),
                id,
            });
        }
        Ok(())
    }

    /// `records` less those the viewer may not see, the others' tags left
    /// out for a viewer without `read_tags`.
    #[must_use]
    pub fn retain<T: Redactable>(&self, records: Vec<T>) -> Vec<T> {
        if self.is_unrestricted() {
            return records;
        }
        records.into_iter().filter_map(|r| self.show(r)).collect()
    }

    /// Leave out the tags of `record` for a viewer without `read_tags`.
    pub fn strip_tags<T: Redactable>(&self, record: &mut T) {
        if self.redaction.withholds_tags() {
            record.tags_mut().clear();
        }
    }

    /// The records of a page SQL already narrowed, their tags left out for
    /// a viewer without `read_tags`.
    #[must_use]
    pub fn page<T: Redactable>(
        &self,
        mut page: oxidgene_core::types::Connection<T>,
    ) -> oxidgene_core::types::Connection<T> {
        if self.redaction.withholds_tags() {
            for edge in &mut page.edges {
                edge.node.tags_mut().clear();
            }
        }
        page
    }

    /// `family` as the viewer sees it: always there, its tags left out when
    /// tags are withheld or its content is.
    #[must_use]
    pub fn family(&self, mut family: Family) -> Family {
        if self.redaction.withholds_tags() || self.redaction.family_withheld(family.id) {
            family.tags.clear();
        }
        family
    }

    /// A page of families as the viewer sees them ([`Self::family`]).
    #[must_use]
    pub fn families(
        &self,
        mut page: oxidgene_core::types::Connection<Family>,
    ) -> oxidgene_core::types::Connection<Family> {
        if !self.is_unrestricted() {
            for edge in &mut page.edges {
                if self.redaction.withholds_tags() || self.redaction.family_withheld(edge.node.id) {
                    edge.node.tags.clear();
                }
            }
        }
        page
    }

    /// The identifier person `id` is shown under, `None` when absent.
    #[must_use]
    pub fn person_id(&self, id: Uuid) -> Option<Uuid> {
        self.redaction.public_id(id)
    }
}

/// The name a `not_found` gives a record of `kind`.
fn kind_name(kind: RecordKind) -> &'static str {
    match kind {
        RecordKind::Person => "Person",
        RecordKind::Family => "Family",
        RecordKind::Event => "Event",
        RecordKind::Place => "Place",
        RecordKind::Source => "Source",
        RecordKind::Citation => "Citation",
        RecordKind::Repository => "Repository",
        RecordKind::Media => "Media",
        RecordKind::Note => "Note",
    }
}

/// Whether the redaction hides no record but confidential notes and tags:
/// no hidden person, no withheld family, no private medium.
fn nothing_hidden(redaction: &Redaction) -> bool {
    redaction.hidden_persons().next().is_none()
        && redaction.withheld_families().next().is_none()
        && !redaction.withholds_private_records()
}

/// The records `unseen` hides, each kind sorted: one statement.
async fn hidden_records(
    db: &DatabaseConnection,
    unseen: &Unseen,
) -> Result<HiddenRecords, OxidGeneError> {
    let mut hidden = HiddenRecords::default();
    for (kind, id) in RedactionRepo::hidden_records(db, unseen).await? {
        let ids = match kind.as_str() {
            "event" => &mut hidden.events,
            "citation" => &mut hidden.citations,
            "note" => &mut hidden.notes,
            "media" => &mut hidden.media,
            "source" => &mut hidden.sources,
            "repository" => &mut hidden.repositories,
            "place" => &mut hidden.places,
            "media_link" => &mut hidden.media_links,
            "vignette" => &mut hidden.vignettes,
            _ => continue,
        };
        ids.push(id);
    }
    for ids in [
        &mut hidden.events,
        &mut hidden.citations,
        &mut hidden.notes,
        &mut hidden.media,
        &mut hidden.sources,
        &mut hidden.repositories,
        &mut hidden.places,
        &mut hidden.media_links,
        &mut hidden.vignettes,
    ] {
        ids.sort_unstable();
        ids.dedup();
    }
    Ok(hidden)
}
