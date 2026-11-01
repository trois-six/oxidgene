//! What a viewer sees of a tree's records (docs/privacy.md §3.2): the
//! events, citations, notes, media, media links, vignettes, sources,
//! repositories and places a redaction hides, and the tags it withholds.
//!
//! [`Records`] pairs the request's [`Redaction`] with the [`HiddenRecords`]
//! it implies, derived once per request and tree and kept in the
//! [`ViewerContext`]. The owner's is unrestricted and costs no query; so is
//! a restricted viewer's on a tree where nobody is hidden — then only
//! confidential notes and tags can be withheld, which need no read either.
//!
//! The rules, from what hides a person or withholds a family:
//!
//! - an event of a hidden person or of a withheld family is hidden;
//! - a citation on a hidden person, a withheld family or a hidden event is
//!   hidden;
//! - a medium is hidden as [`Redaction::medium_withheld`] decides from the
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
use oxidgene_db::repo::{LeaveOut, NoteOwners, RedactionRepo, Unseen};
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

fn sorted(ids: impl IntoIterator<Item = Uuid>) -> Vec<Uuid> {
    oxidgene_core::collections::sorted_unique(ids)
}

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
            .get_or_try_init(|| async {
                hidden_records(db, tree_id, &redaction).await.map(Arc::new)
            })
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

/// The records of tree `tree_id` that `redaction` hides.
async fn hidden_records(
    db: &DatabaseConnection,
    tree_id: Uuid,
    redaction: &Redaction,
) -> Result<HiddenRecords, OxidGeneError> {
    let persons = sorted(redaction.hidden_persons());
    let families = sorted(redaction.withheld_families());

    let event_rows = RedactionRepo::events_of(db, tree_id, &persons, &families).await?;
    let events = sorted(event_rows.iter().map(|e| e.id));

    let media = hidden_media(db, tree_id, redaction, &persons, &families, &event_rows).await?;
    let medium_ids = sorted(media.iter().map(|m| m.id));

    let citation_rows = RedactionRepo::citations_of(db, &persons, &families, &events).await?;
    let citations = sorted(citation_rows.iter().map(|(id, _)| *id));

    let (sources, repositories) = hidden_sources(db, &citation_rows, &citations).await?;
    let places = hidden_places(db, tree_id, &event_rows, &events, &media, &medium_ids).await?;

    let notes = RedactionRepo::notes_of(
        db,
        tree_id,
        &NoteOwners {
            persons: &persons,
            families: &families,
            events: &events,
            media: &medium_ids,
            sources: &sources,
            repositories: &repositories,
        },
    )
    .await?;

    let mut links = RedactionRepo::links_of(db, &persons, &families, &events, &sources).await?;
    links.extend(RedactionRepo::links_of_media(db, &medium_ids).await?);
    let media_links = sorted(links.into_iter().map(|l| l.id));
    let vignettes = sorted(
        RedactionRepo::vignettes_of(db, &persons, &events, &medium_ids)
            .await?
            .into_iter()
            .map(|v| v.id),
    );

    Ok(HiddenRecords {
        events,
        citations,
        notes: sorted(notes),
        media: medium_ids,
        sources,
        repositories,
        places,
        media_links,
        vignettes,
    })
}

/// Of `candidates`, those no visible user names: `uses` lists each use of
/// a candidate with whether its user is hidden.
fn used_only_by_hidden(
    candidates: Vec<Uuid>,
    uses: impl IntoIterator<Item = (Uuid, bool)>,
) -> Vec<Uuid> {
    let shown: std::collections::HashSet<Uuid> = uses
        .into_iter()
        .filter_map(|(candidate, hidden)| (!hidden).then_some(candidate))
        .collect();
    candidates
        .into_iter()
        .filter(|candidate| !shown.contains(candidate))
        .collect()
}

/// The sources only hidden citations draw on — of those `citations` (with
/// their sources) do — and the repositories only holding such sources.
async fn hidden_sources(
    db: &DatabaseConnection,
    citation_rows: &[(Uuid, Uuid)],
    citations: &[Uuid],
) -> Result<(Vec<Uuid>, Vec<Uuid>), OxidGeneError> {
    let cited = sorted(citation_rows.iter().map(|(_, source)| *source));
    let uses = RedactionRepo::citations_of_sources(db, &cited).await?;
    let sources = used_only_by_hidden(
        cited,
        uses.into_iter()
            .map(|(citation, source)| (source, holds(citations, citation))),
    );
    let held = sorted(
        RedactionRepo::holdings_of_sources(db, &sources)
            .await?
            .into_iter()
            .map(|(_, repository)| repository),
    );
    let holdings = RedactionRepo::holdings_of_repositories(db, &held).await?;
    let repositories = used_only_by_hidden(
        held,
        holdings
            .into_iter()
            .map(|(source, repository)| (repository, holds(&sources, source))),
    );
    Ok((sources, repositories))
}

/// The places only hidden events and media use, of those they use.
async fn hidden_places(
    db: &DatabaseConnection,
    tree_id: Uuid,
    event_rows: &[oxidgene_db::repo::EventOwner],
    events: &[Uuid],
    media: &[oxidgene_db::repo::MediumOwner],
    medium_ids: &[Uuid],
) -> Result<Vec<Uuid>, OxidGeneError> {
    let used = sorted(
        event_rows
            .iter()
            .filter_map(|e| e.place_id)
            .chain(media.iter().filter_map(|m| m.place_id)),
    );
    let at_events = RedactionRepo::events_at(db, tree_id, &used).await?;
    let at_media = RedactionRepo::media_at(db, tree_id, &used).await?;
    let uses = at_events
        .iter()
        .filter_map(|e| Some((e.place_id?, holds(events, e.id))))
        .chain(
            at_media
                .iter()
                .filter_map(|m| Some((m.place_id?, holds(medium_ids, m.id)))),
        );
    Ok(used_only_by_hidden(used, uses))
}

/// The media of tree `tree_id` the redaction withholds, pages included:
/// those showing a hidden person or a withheld family, through a link, a
/// vignette or an event, and the private ones.
async fn hidden_media(
    db: &DatabaseConnection,
    tree_id: Uuid,
    redaction: &Redaction,
    persons: &[Uuid],
    families: &[Uuid],
    events: &[oxidgene_db::repo::EventOwner],
) -> Result<Vec<oxidgene_db::repo::MediumOwner>, OxidGeneError> {
    use std::collections::HashMap;

    let event_ids = sorted(events.iter().map(|e| e.id));
    let owner: HashMap<Uuid, &oxidgene_db::repo::EventOwner> =
        events.iter().map(|e| (e.id, e)).collect();
    // What each medium shows: the hidden persons and withheld families.
    let mut shows: HashMap<Uuid, (Vec<Uuid>, Vec<Uuid>)> = HashMap::new();
    let mut show =
        |medium: Uuid, person: Option<Uuid>, family: Option<Uuid>, event: Option<Uuid>| {
            let entry = shows.entry(medium).or_default();
            let by_event = event.and_then(|e| owner.get(&e));
            entry
                .0
                .extend(person.into_iter().chain(by_event.and_then(|e| e.person_id)));
            entry
                .1
                .extend(family.into_iter().chain(by_event.and_then(|e| e.family_id)));
        };
    for link in RedactionRepo::links_of(db, persons, families, &event_ids, &[]).await? {
        show(link.media_id, link.person_id, link.family_id, link.event_id);
    }
    for vignette in RedactionRepo::vignettes_of(db, persons, &event_ids, &[]).await? {
        show(
            vignette.media_id,
            vignette.person_id,
            None,
            vignette.event_id,
        );
    }
    let candidates = sorted(shows.keys().copied());
    let mut media = RedactionRepo::media(db, tree_id, &candidates).await?;
    if redaction.withholds_private_records() {
        media.extend(RedactionRepo::unpublished_media(db, tree_id).await?);
    }
    // A page shows what its document shows: the document decides.
    let documents = sorted(media.iter().filter_map(|m| m.document_id));
    media.extend(RedactionRepo::media(db, tree_id, &documents).await?);
    let privacy: HashMap<Uuid, oxidgene_core::enums::Privacy> =
        media.iter().map(|m| (m.id, m.privacy)).collect();
    let mut decided: HashMap<Uuid, (Vec<Uuid>, Vec<Uuid>)> = HashMap::new();
    for medium in &media {
        let root = medium.document_id.unwrap_or(medium.id);
        let entry = decided.entry(root).or_default();
        if let Some((p, f)) = shows.get(&medium.id) {
            entry.0.extend(p);
            entry.1.extend(f);
        }
    }
    let withheld: Vec<Uuid> = sorted(decided.into_iter().filter_map(|(root, (p, f))| {
        let privacy = privacy.get(&root).copied()?;
        redaction.medium_withheld(privacy, p, f).then_some(root)
    }));
    let mut hidden = RedactionRepo::media(db, tree_id, &withheld).await?;
    hidden.extend(RedactionRepo::pages_of(db, tree_id, &withheld).await?);
    hidden.sort_by_key(|m| m.id);
    hidden.dedup_by_key(|m| m.id);
    Ok(hidden)
}
