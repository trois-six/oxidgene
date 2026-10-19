//! OxidGene domain model → GEDCOM export.
//!
//! Converts domain model entities into `oxidged`'s model and writes it as
//! GEDCOM 5.5.1 for a `.ged`, or as GEDCOM 7.0 for the `gedcom.ged` of a
//! GEDZIP. `oxidged`'s writer is conformant: it converts the model to the
//! version written, writes every line by the version's line rules (in 5.5.1,
//! 255 characters at most and a long text continued on `CONC` lines; a line
//! break of any kind on a `CONT` line, `@` escaped) and repairs, as extension
//! structures, what the version does not permit.

use std::collections::{HashMap, HashSet};

use oxidged::gedzip::GedzipWriter;
use oxidged::model::{
    Address as GedAddress, Age as GedAge, Association as GedAssociation, CallNumber, Certainty,
    ChildLink, Citation as GedCitation, CitationData, CitationSource, Crop, Dataset, Date,
    EnumList, Event as GedEvent, EventFamily, EventKind, EventSpouse, Family as GedFamily, File,
    FileForm, Header, HeaderSource, Individual, IndividualRef, Map, Medium,
    Multimedia as GedMultimedia, MultimediaLink, Name as GedName, NamePiece, NamePieceKind,
    NameType as GedNameType, Node, Note as GedNote, Ordinance, OrdinanceKind,
    Pedigree as GedPedigree, Phrased, Place as GedPlace, PlaceDetail, Record,
    Repository as GedRepository, RepositoryCitation, Restriction, Schema, Sex as GedSex,
    SharedNote as GedSharedNote, Source as GedSource, SourceData, SourceText, SpouseLink, Store,
    Submitter, TagId, Text, Value, XrefId,
};
pub use oxidged::profile::ExportProfile;
use oxidged::profile::{ExportOptions as ProfileOptions, Exporter, FoldKind};
use oxidged::tree::{Payload, Structure, Tag};
use oxidged::{Bom, GedcomVersion, GedcomWriter};
use uuid::Uuid;

use oxidgene_core::enums::SourceMediaType;
use oxidgene_core::types::{
    Citation, Event, EventWitness, Family, FamilyChild, FamilySpouse, GedcomKept, Media, MediaLink,
    Note, Person, PersonName, Place, Repository, Source, SourceRepository, TagRef, Vignette,
};
use oxidgene_core::{ChildType, Confidence, EventType, NameType, Privacy, Sex, SpouseRole};

use crate::keeping;
use crate::{
    DestinationReport, DocumentExtension, DocumentMetadataExtension, ExportLimit, ExportLoss,
    ExportResult, MediaMetadataExtension, MediaNoteExtension, MediaPlaceExtension, TagExtension,
};

/// Every record of one tree an export writes.
#[derive(Debug, Clone, Copy, Default)]
pub struct ExportRecords<'a> {
    pub persons: &'a [Person],
    pub person_names: &'a [PersonName],
    pub families: &'a [Family],
    pub family_spouses: &'a [FamilySpouse],
    pub family_children: &'a [FamilyChild],
    pub events: &'a [Event],
    pub event_witnesses: &'a [EventWitness],
    pub places: &'a [Place],
    pub sources: &'a [Source],
    pub repositories: &'a [Repository],
    pub source_repositories: &'a [SourceRepository],
    pub citations: &'a [Citation],
    pub media: &'a [Media],
    pub media_links: &'a [MediaLink],
    pub vignettes: &'a [Vignette],
    pub notes: &'a [Note],
    /// What GEDCOM the import kept beside these records (see `docs/api.md`,
    /// "Kept GEDCOM"): put back beneath the records written, the rows of the
    /// notes, sources, repositories or media left out with them dropped.
    pub gedcom_kept: &'a [GedcomKept],
}

impl ExportRecords<'_> {
    /// These records without those `choices` leave out: the notes, and the
    /// sources with their citations and repositories, unless they are
    /// included; the media with their links and identifications, unless
    /// they are.
    ///
    /// Nothing written points at a record left out: every pointer is looked
    /// up among the records written, and a repository is reached only
    /// through a source.
    #[must_use]
    pub fn chosen(mut self, choices: ExportChoices) -> Self {
        if !choices.include_notes_and_sources {
            self.notes = &[];
            self.sources = &[];
            self.citations = &[];
            self.repositories = &[];
            self.source_repositories = &[];
        }
        if !choices.include_media {
            self.media = &[];
            self.media_links = &[];
            self.vignettes = &[];
        }
        self
    }
}

/// What the person exporting chose: what the file holds, and the program
/// it is written for. The default is the complete export, standard GEDCOM
/// 5.5.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportChoices {
    /// The program a `.ged` is written for: `oxidged`'s export profile, which
    /// chooses the version and the line layout and folds the data into the
    /// structures that program reads (see [`destination`]). A GEDZIP is
    /// standard GEDCOM 7.0 and does not read it.
    pub destination: &'static ExportProfile,
    /// Write the notes, and the sources with their citations and
    /// repositories.
    pub include_notes_and_sources: bool,
    /// Write the media, their links and their identifications.
    pub include_media: bool,
}

impl Default for ExportChoices {
    fn default() -> Self {
        Self {
            destination: destination(DEFAULT_DESTINATION)
                .expect("oxidged has a generic GEDCOM 5.5.1 export profile"),
            include_notes_and_sources: true,
            include_media: true,
        }
    }
}

/// The destination of a `.ged` when none is chosen: any GEDCOM 5.5.1
/// importer.
pub const DEFAULT_DESTINATION: &str = "generic-551";

/// The destination `id` names: one of `oxidged`'s export profiles, by its
/// exact id (`generic-551`, `generic-70`, `geneanet`, `ancestry`, …).
#[must_use]
pub fn destination(id: &str) -> Option<&'static ExportProfile> {
    oxidged::profile::export_profiles()
        .iter()
        .copied()
        .find(|profile| profile.id() == id)
}

/// Every destination a `.ged` can be written for: the standard GEDCOM
/// versions (`oxidged`'s tier 0) first, then the programs by name.
#[must_use]
pub fn destinations() -> Vec<&'static ExportProfile> {
    let mut all = oxidged::profile::export_profiles().to_vec();
    all.sort_by_cached_key(|profile| (profile.tier() != 0, profile.label().to_lowercase()));
    all
}

/// Whether `profile`'s facts come from the programs' published and observed
/// behaviour, much of it dated, and are to be re-verified against their
/// recent versions: `oxidged`'s tier-1 profiles.
#[must_use]
pub fn to_be_verified(profile: &ExportProfile) -> bool {
    profile.tier() == 1
}

/// What an export writes: a plain GEDCOM file, or the `gedcom.ged` of a
/// GEDZIP archive.
#[derive(Debug, Clone, Copy)]
pub enum ExportTarget<'a> {
    /// A `.ged`, written for the chosen destination
    /// ([`ExportChoices::destination`]): GEDCOM 5.5.1 by default, the version
    /// every genealogy program and site reads. Its media keep their
    /// producers' paths, since no archive carries their bytes.
    Gedcom,
    /// The `gedcom.ged` of a `.gdz`: GEDCOM 7.0, which the GEDZIP format
    /// requires, with the extensions it uses declared in `HEAD.SCHMA`.
    /// `media_paths` holds the archive entry each medium packed in the
    /// archive is written as; a medium neither packed nor at a web address
    /// is kept as an OxidGene extension (see [`export_gedcom`]).
    Gedzip {
        media_paths: &'a HashMap<Uuid, String>,
    },
}

/// How an export writes the records: see [`export_gedcom`].
#[derive(Debug, Clone, Copy)]
pub struct ExportOptions<'a> {
    /// What the file holds and how it writes it.
    pub choices: ExportChoices,
    /// The file the export writes.
    pub target: ExportTarget<'a>,
    /// The tree's "Who am I?" person.
    pub self_person_id: Option<Uuid>,
    /// The tree's submitter settings.
    pub submitter: SubmitterSettings<'a>,
}

/// Who a tree's exports say they are from, as its settings hold it.
#[derive(Debug, Clone, Copy, Default)]
pub struct SubmitterSettings<'a> {
    pub name: Option<&'a str>,
    pub email: Option<&'a str>,
    pub address: Option<&'a str>,
}

/// Export domain model entities to GEDCOM text: a `.ged` written for the
/// chosen destination, or the GEDCOM 7.0 `gedcom.ged` of a GEDZIP (see
/// [`ExportTarget`]).
///
/// All records should belong to the same tree.
///
/// Both are built as one 5.5.1 dataset. The `.ged` is written by `oxidged`'s
/// [`Exporter`] with the destination's export profile, which chooses the
/// version, the character set and the line layout the program reads best,
/// folds the data into the standard structures it reads (Geneanet's single
/// occupation and single name, for one) and reports every fold that lost
/// something, and every limit of the program the file exceeds, in the
/// result's [`DestinationReport`]. The standard destinations fold nothing.
/// The credentials a dataset may hold are left out, as `oxidged` does by
/// default; OxidGene writes none.
///
/// The GEDZIP's is written as 7.0 by
/// `oxidged`'s writer, which converts it as the FamilySearch migration guide
/// says (note records become `SNOTE`, `RELA` a `ROLE` with its phrase, file
/// formats media types, dates and ages 7.0's grammar) and starts the text
/// with the byte-order mark GEDCOM 7.0 recommends. Its `OBJE` records are
/// the media the archive packs, under their archive entries
/// ([`archive_path`]), and those named by a web address: GEDZIP permits no
/// other `FILE`, neither a local `file:` URL nor a relative path the archive
/// does not hold. A medium whose bytes OxidGene does not hold, which only
/// its producer's path names, is kept as the OxidGene extension record
/// `_OXIDGENE_OBJE` instead, its links as `_OXIDGENE_OBJE` pointers (see
/// `keep_as_extensions`): other programs ignore them, and an OxidGene
/// import restores the medium as a `.ged` would.
///
/// The destination is a `.ged`'s: no export profile writes a GEDZIP, which
/// stays the complete, standard GEDCOM 7.0 export.
///
/// `include_notes_and_sources` and `include_media` leave the notes and the
/// sources, or the media, out of the file: see [`ExportRecords::chosen`].
///
/// `self_person_id` is the tree's "Who am I?" person: the `SUBM` record GEDCOM
/// 5.5.1 requires is named after them, or `Not Provided` without one.
///
/// # Errors
///
/// Returns `Err` if the GEDCOM writer fails.
pub fn export_gedcom(
    records: &ExportRecords<'_>,
    options: &ExportOptions<'_>,
) -> Result<ExportResult, String> {
    let ExportOptions {
        choices,
        target,
        self_person_id,
        submitter,
    } = *options;
    let media_paths = match target {
        ExportTarget::Gedcom => None,
        ExportTarget::Gedzip { media_paths } => Some(media_paths),
    };
    let ExportRecords {
        persons,
        person_names,
        families,
        family_spouses,
        family_children,
        events,
        event_witnesses,
        places,
        sources,
        repositories,
        source_repositories,
        citations,
        media,
        media_links,
        vignettes,
        notes,
        gedcom_kept,
    } = records.chosen(choices);
    let mut warnings: Vec<String> = Vec::new();
    let build_span = tracing::info_span!(
        "export.build_model",
        export.person_count = persons.len(),
        export.family_count = families.len(),
        export.event_count = events.len(),
        export.source_count = sources.len(),
        export.citation_count = citations.len(),
        export.media_count = media.len(),
    );
    let build_guard = build_span.enter();

    let mut data = Dataset::new(GedcomVersion::V5_5_1);
    let xrefs = Xrefs::new(
        data.store_mut(),
        persons,
        families,
        sources,
        repositories,
        media,
        notes,
    );
    let person_tag = data.store_mut().intern_tag(PERSON_TAG);
    let tag_tag = data.store_mut().intern_tag(TAG_TAG);
    // GEDCOM 7.x writes a witness beneath its event; a 5.5.1 file beneath
    // an individual record. A GEDZIP's `gedcom.ged` is 7.0 whatever the
    // destination.
    let under_events = match target {
        ExportTarget::Gedzip { .. } => true,
        ExportTarget::Gedcom => choices.destination.target().version().is_v7(),
    };
    let media_by_id: HashMap<Uuid, &Media> = media.iter().map(|m| (m.id, m)).collect();
    let written_events = written_events(events);
    let linked = linked_events(
        media_links,
        vignettes,
        &xrefs,
        &media_by_id,
        &written_events,
    );
    let ordinances: HashSet<Uuid> = events
        .iter()
        .filter(|evt| ordinance_written_as(evt, linked.contains(&evt.id)).is_some())
        .map(|evt| evt.id)
        .collect();
    let witnessed = associations(events, event_witnesses, &ordinances, under_events, &xrefs);
    let mut index = ExportIndex {
        place_map: places.iter().map(|p| (p.id, p)).collect(),
        media_by_id,
        pages_of: pages_of(media),
        names_by_person: group_by(person_names, |pn| Some(pn.person_id)),
        events_by_person: group_by(events, |evt| evt.person_id),
        events_by_family: group_by(events, |evt| evt.family_id),
        assoc_by_person: witnessed.by_person,
        assoc_by_event: witnessed.by_event,
        cites_by_person: group_by(citations, |cite| cite.person_id),
        cites_by_event: group_by(citations, |cite| cite.event_id),
        // A family's own citations: those on its events are written there.
        cites_by_family: group_by(citations, |cite| {
            cite.family_id.filter(|_| cite.event_id.is_none())
        }),
        notes_by_person: group_by(notes, |note| note.person_id),
        notes_by_family: group_by(notes, |note| note.family_id),
        notes_by_source: group_by(notes, |note| note.source_id),
        notes_by_event: group_by(notes, |note| note.event_id),
        notes_by_media: group_by(notes, |note| note.media_id),
        notes_by_repository: group_by(notes, |note| note.repository_id),
        repos_by_source: {
            let mut links = group_by(source_repositories, |l| Some(l.source_id));
            for links in links.values_mut() {
                links.sort_by_key(|l| (l.sort_order, l.id));
            }
            links
        },
        mlinks_by_person: group_by(media_links, |ml| ml.person_id),
        mlinks_by_event: group_by(media_links, |ml| ml.event_id),
        mlinks_by_family: group_by(media_links, |ml| ml.family_id),
        spouses_by_family: group_by(family_spouses, |fs| Some(fs.family_id)),
        sex_by_person: persons.iter().map(|p| (p.id, p.sex)).collect(),
        children_by_family: group_by(family_children, |fc| Some(fc.family_id)),
        // person_id → families (for INDI-level FAMS/FAMC back-links, without
        // which the exported file has no individual↔family linkage at all —
        // most GEDCOM readers rely on FAMS/FAMC rather than cross-referencing
        // FAM's own HUSB/WIFE/CHIL back to individuals).
        fams_by_person: group_by(family_spouses, |fs| Some(fs.person_id)),
        famc_by_person: group_by(family_children, |fc| Some(fc.person_id)),
        written_events,
        vignettes_by_id: HashMap::new(),
        crops_by_person: HashMap::new(),
        crops_by_event: HashMap::new(),
        person_tag,
        tag_tag,
        xrefs,
    };
    index.group_vignettes(vignettes);

    let (mut extensions, extension_warnings) =
        media_extensions(data.store_mut(), media, vignettes, &index);
    warnings.extend(extension_warnings);
    warnings.extend(untaggable_documents(media));

    data.set_header(Some(gedcom_header(
        index.xrefs.submitter,
        media_paths.is_some() && !media.is_empty(),
    )));
    // Each record is stored in the dataset's tables as it is built; `push`
    // adds it unchecked, as reading does, since a record points at records
    // pushed after it.
    data.push(Record::Submitter(Box::new(
        index.submitter(submitter, self_person_id),
    )));
    for shared in &index.xrefs.shared_notes {
        data.push(Record::Note(Box::new(GedSharedNote {
            xref: Some(shared.xref),
            text: Text::from(shared.text.as_str()),
            ..GedSharedNote::default()
        })));
    }
    for src in sources {
        data.push(Record::Source(Box::new(index.source(src))));
    }
    for repo in repositories {
        data.push(Record::Repository(Box::new(index.repository(repo))));
    }
    // A document is dissolved into its pages, which carry the bytes; see
    // `pages_of`.
    for m in media.iter().filter(|m| !m.is_document()) {
        let extra = extensions.remove(&m.id).unwrap_or_default();
        data.push(Record::Multimedia(Box::new(index.multimedia(
            m,
            media_paths,
            extra,
        ))));
    }
    for person in persons {
        let individual = index.individual(person, &mut warnings);
        data.push(Record::Individual(Box::new(individual)));
    }
    for fam in families {
        data.push(Record::Family(Box::new(index.family(fam, &mut warnings))));
    }

    // The media a GEDZIP keeps as extensions: those it neither packs nor
    // reaches at a web address.
    let kept: HashSet<String> = media_paths
        .map(|paths| {
            media
                .iter()
                .filter(|m| {
                    !m.is_document() && !paths.contains_key(&m.id) && !is_web_address(&m.file_path)
                })
                .filter_map(|m| index.xrefs.media.get(&m.id))
                .map(|xref| data.store().xref(*xref).to_string())
                .collect()
        })
        .unwrap_or_default();

    drop(build_guard);

    let xrefs = index.exported_xrefs(data.store());
    if !gedcom_kept.is_empty() {
        data = with_kept(&data, gedcom_kept, choices, &xrefs, &mut warnings);
    }
    let (gedcom, report) = write_gedcom(&data, target, choices.destination, &kept)?;
    Ok(ExportResult {
        gedcom,
        warnings,
        report,
        xrefs,
    })
}

/// `data` with what the import kept put back beneath its records (see
/// [`keeping::put_back`]), rebuilt from its text so that the writer sees the
/// records as they are written. The header is written anew by the writer.
fn with_kept(
    data: &Dataset,
    gedcom_kept: &[GedcomKept],
    choices: ExportChoices,
    xrefs: &HashMap<String, Uuid>,
    warnings: &mut Vec<String>,
) -> Dataset {
    let mut structures = data.to_structures();
    let left_out = keeping::put_back(&mut structures, gedcom_kept, choices, xrefs, warnings);
    if left_out > 0 {
        warnings.push(format!(
            "{left_out} kept GEDCOM structures had no place left in the tree and were left out"
        ));
    }
    let text: String = structures
        .iter()
        .map(|structure| structure.to_gedcom(0, GedcomVersion::V5_5_1))
        .collect();
    Dataset::parse(text)
}

/// The GEDCOM cross-reference of every record the export writes, by id,
/// interned in the dataset's store.
struct Xrefs {
    person: HashMap<Uuid, XrefId>,
    family: HashMap<Uuid, XrefId>,
    source: HashMap<Uuid, XrefId>,
    repository: HashMap<Uuid, XrefId>,
    media: HashMap<Uuid, XrefId>,
    /// The shared note record (`NOTE` in 5.5.1, `SNOTE` in 7.x) each group
    /// and text a note is written as points at. A group whose notes say
    /// different things has one record per text.
    note_text: HashMap<(Uuid, String), XrefId>,
    /// Every shared note record the export writes, in xref order.
    shared_notes: Vec<SharedNoteRecord>,
    /// The one `SUBM` record an export writes.
    submitter: XrefId,
}

/// A shared note record the export writes: its xref and the text it holds.
struct SharedNoteRecord {
    xref: XrefId,
    text: String,
}

impl Xrefs {
    fn new(
        store: &mut Store,
        persons: &[Person],
        families: &[Family],
        sources: &[Source],
        repositories: &[Repository],
        media: &[Media],
        notes: &[Note],
    ) -> Self {
        // Each distinct (group, text) of the notes the export writes as
        // pointers is one shared record, numbered in group then text order.
        let mut shared: Vec<(Uuid, &str)> = notes
            .iter()
            .filter(|note| is_written_as_pointer(note))
            .filter_map(|note| note.shared_group.map(|group| (group, note.text.as_str())))
            .collect();
        shared.sort_unstable();
        shared.dedup();
        let mut note_text = HashMap::new();
        let mut shared_notes = Vec::with_capacity(shared.len());
        for (i, (group, text)) in shared.into_iter().enumerate() {
            let xref = intern(store, &format!("@N{}@", i + 1));
            note_text.insert((group, text.to_string()), xref);
            shared_notes.push(SharedNoteRecord {
                xref,
                text: text.to_string(),
            });
        }
        Self {
            person: numbered(store, "I", persons.iter().map(|p| p.id)),
            family: numbered(store, "F", families.iter().map(|f| f.id)),
            source: numbered(store, "S", sources.iter().map(|s| s.id)),
            repository: numbered(store, "R", repositories.iter().map(|r| r.id)),
            // Only pages become records: a document holds no bytes and is
            // dissolved into them. Numbering just the pages keeps the xrefs
            // contiguous rather than leaving a gap wherever a document sat in
            // the list.
            media: numbered(
                store,
                "M",
                media.iter().filter(|m| !m.is_document()).map(|m| m.id),
            ),
            note_text,
            shared_notes,
            submitter: intern(store, SUBMITTER_XREF),
        }
    }
}

/// Whether the export writes `note` as a record's note, rather than as a
/// media transcript or not at all: it hangs off a record the export writes
/// as a `NOTE` structure.
fn is_written_as_pointer(note: &Note) -> bool {
    note.media_id.is_none()
        && [
            note.person_id,
            note.family_id,
            note.source_id,
            note.event_id,
            note.repository_id,
        ]
        .iter()
        .any(Option::is_some)
}

/// Interns an identifier. A store holds four billion of them; a tree never
/// comes close.
fn intern(store: &mut Store, xref: &str) -> XrefId {
    store
        .intern_xref(xref)
        .expect("a store holds four billion identifiers")
}

/// `@{prefix}{n}@` for each id, numbered from 1 in order.
fn numbered(
    store: &mut Store,
    prefix: &str,
    ids: impl Iterator<Item = Uuid>,
) -> HashMap<Uuid, XrefId> {
    ids.enumerate()
        .map(|(i, id)| (id, intern(store, &format!("@{prefix}{}@", i + 1))))
        .collect()
}

/// The rows of a slice grouped by the record `key` attaches each to, in slice
/// order. A row `key` attaches to nothing is left out.
fn group_by<T>(rows: &[T], key: impl Fn(&T) -> Option<Uuid>) -> HashMap<Uuid, Vec<&T>> {
    let mut grouped: HashMap<Uuid, Vec<&T>> = HashMap::new();
    for row in rows {
        if let Some(id) = key(row) {
            grouped.entry(id).or_default().push(row);
        }
    }
    grouped
}

/// The `ASSO` structures the witnesses are written as, by where each is
/// written: beneath its own individual record, or beneath the event it
/// witnesses.
#[derive(Default)]
struct Associations {
    /// The `ASSO` of each individual record (see [`associations`]).
    by_person: HashMap<Uuid, Vec<GedAssociation>>,
    /// The `ASSO` beneath each event, which GEDCOM 7.x permits of an
    /// individual's and a family's events alike.
    by_event: HashMap<Uuid, Vec<GedAssociation>>,
}

/// The `ASSO` structures the witnesses are written as.
///
/// GEDCOM 7.x permits `ASSO` beneath any event of an individual or a family,
/// pointing at the witness: under `under_events`, each witness of an event
/// that is not written as an ordinance is written there, with its relation
/// (the writer makes the 7.0 `ROLE` of it). An ordinance has no `ASSO` in
/// any version, so its witnesses, and every witness under a 5.5.1 file,
/// are written beneath an individual record, as described below.
///
/// GEDCOM 5.5.1 only allows `ASSO` as a level-1 substructure of an INDI
/// record, pointing at an individual (confirmed against real-world Gramps
/// output and rejected by Gramps as an unsupported tag when nested inside an
/// event detail). Which INDI record it's attached to, and what it points at,
/// depends on whether the witnessed event belongs to a person or a family:
///   - family event (e.g. a marriage): attached to the *witness's* own
///     INDI record, pointing at the family — Gramps' own convention
///     (`1 ASSO @F1@` / `2 RELA witness`). GEDCOM 5.5.1 has no association
///     with a family, so the conformant writer keeps it as the extension
///     `1 _ASSO @F1@`, which the import reads back.
///   - individual event (e.g. a burial): attached to the *event owner's*
///     own INDI record, pointing at the witness
///     (`1 ASSO @I2@` / `2 RELA witness`).
///
/// A person with several individual events sharing a witness can't
/// disambiguate which event on GEDCOM re-import (the format has no way
/// to nest ASSO under a specific event and still be portable) — this is
/// an inherent GEDCOM/Gramps limitation, not something round-tripped.
///
/// `RELA`, which GEDCOM 5.5.1 requires, is the witness's relation; the
/// writer fills a witness without one with its placeholder, `Unknown`.
fn associations(
    events: &[Event],
    event_witnesses: &[EventWitness],
    ordinances: &HashSet<Uuid>,
    under_events: bool,
    xrefs: &Xrefs,
) -> Associations {
    let witnesses_by_event = group_by(event_witnesses, |w| Some(w.event_id));
    let mut associations = Associations::default();
    for evt in events {
        for w in witnesses_by_event.get(&evt.id).into_iter().flatten() {
            let relation = w.relation.as_deref().map(Text::from);
            if under_events && !ordinances.contains(&evt.id) {
                if let Some(witness) = xrefs.person.get(&w.person_id) {
                    associations
                        .by_event
                        .entry(evt.id)
                        .or_default()
                        .push(GedAssociation {
                            individual: Some(*witness),
                            relation,
                            ..GedAssociation::default()
                        });
                }
                continue;
            }
            let Some((owner, target)) = association_target(evt, w, xrefs) else {
                continue;
            };
            associations
                .by_person
                .entry(owner)
                .or_default()
                .push(GedAssociation {
                    individual: Some(target),
                    relation,
                    ..GedAssociation::default()
                });
        }
    }
    associations
}

/// The person whose record carries a witness's `ASSO`, and the xref it points
/// at — see [`associations`]. `None` when either end is not exported.
fn association_target(evt: &Event, w: &EventWitness, xrefs: &Xrefs) -> Option<(Uuid, XrefId)> {
    let witness_xref = xrefs.person.get(&w.person_id)?;
    if let Some(family_id) = evt.family_id {
        return Some((w.person_id, *xrefs.family.get(&family_id)?));
    }
    Some((evt.person_id?, *witness_xref))
}

/// The pages of each multi-page document, in reading order.
///
/// A multi-page document is a container, and GEDCOM has no container at
/// all. Rather than fake one, the document dissolves: its pages are
/// exported as ordinary standalone media, and anything linked to the
/// document is linked to every one of them.
///
/// The document's own row is not exported. It holds no bytes — its
/// `file_path` is its *title*, which is what made a GEDZIP warn about an
/// archive entry that could never exist — and writing it as its cover
/// instead only produced a duplicate of page one while leaving the other
/// thirty-seven attached to nobody.
fn pages_of(media: &[Media]) -> HashMap<Uuid, Vec<&Media>> {
    let mut pages_of = group_by(media, |m| m.parent_media_id);
    for pages in pages_of.values_mut() {
        pages.sort_by_key(|page| (page.page_index, page.id));
    }
    pages_of
}

/// The xref of the one `SUBM` record an export writes.
const SUBMITTER_XREF: &str = "@SUBM1@";

/// Where OxidGene's GEDCOM extensions are documented: the anchors of the
/// API contract's section on them.
const EXTENSIONS_DOC: &str = concat!(env!("CARGO_PKG_REPOSITORY"), "/blob/main/docs/api.md#");

/// OxidGene's own extension tags, each with the anchor of its
/// documentation, which a GEDCOM 7.0 header declares (`HEAD.SCHMA`).
const EXTENSION_TAGS: [(&str, &str); 7] = [
    (MEDIA_TAG, "_oxidgene_media"),
    (DOCUMENT_TAG, "_oxidgene_doc"),
    (VIGNETTE_TAG, "_oxidgene_vignette"),
    (PERSON_TAG, "_oxidgene_person"),
    (KEPT_MEDIA_TAG, "_oxidgene_obje"),
    (POSITION_TAG, "_oxidgene_pos"),
    (TAG_TAG, "_oxidgene_tag"),
];

/// What a page says about itself: see [`MediaMetadataExtension`].
const MEDIA_TAG: &str = "_OXIDGENE_MEDIA";
/// The document a page belongs to: see [`DocumentExtension`].
const DOCUMENT_TAG: &str = "_OXIDGENE_DOC";
/// A region of a page that identifies nobody attributed to it, with its
/// rectangle. An attributed crop is a `CROP` beneath the `OBJE` link instead.
const VIGNETTE_TAG: &str = "_OXIDGENE_VIGNETTE";
/// The person a crop beneath an event identifies, as a pointer.
pub(crate) const PERSON_TAG: &str = "_OXIDGENE_PERSON";
/// A medium a GEDZIP cannot name as an `OBJE`, and a link to it.
pub(crate) const KEPT_MEDIA_TAG: &str = "_OXIDGENE_OBJE";
/// The position of a kept medium among the `OBJE` records, or of a link to
/// one among the links beside it.
pub(crate) const POSITION_TAG: &str = "_OXIDGENE_POS";
/// One tag a person, a couple or a document carries: see [`TagExtension`].
pub(crate) const TAG_TAG: &str = "_OXIDGENE_TAG";

/// The GEDCOM header naming OxidGene as the producer, and the submitter
/// GEDCOM 5.5.1 requires it to point at. The writer completes it with what
/// the version requires: in 5.5.1 `GEDC` with `VERS 5.5.1` and `FORM
/// LINEAGE-LINKED`, and the `CHAR` of its output, `UTF-8`; in 7.0 `GEDC`
/// with `VERS 7.0`. A `.ged`'s export profile keeps OxidGene as the
/// producer and notes oxidged, which wrote the file, beneath it
/// (`HEAD.SOUR._OXIDGED`; see [`write_gedcom`]).
///
/// With `extensions`, the header declares OxidGene's extension tags with
/// the URIs of their documentation (`SCHMA`, GEDCOM 7.0 only), which makes
/// them documented extension tags.
fn gedcom_header(submitter: XrefId, extensions: bool) -> Header {
    let schema = extensions.then(|| Schema {
        tags: EXTENSION_TAGS
            .iter()
            .map(|(tag, anchor)| Text::from(format!("{tag} {EXTENSIONS_DOC}{anchor}")))
            .collect(),
        ..Schema::default()
    });
    Header {
        submitter: Some(submitter),
        source: Some(HeaderSource {
            product: Text::from("OXIDGENE"),
            name: Some(Text::from("OxidGene")),
            version: Some(Text::from(env!("CARGO_PKG_VERSION"))),
            ..HeaderSource::default()
        }),
        schema,
        ..Header::default()
    }
}

/// Serialize the model to GEDCOM text: for a `.ged`, as `destination`'s
/// export profile writes it, with what it lost; for a GEDZIP, converted to
/// 7.0 after a byte-order mark, with the `kept` media records (by xref) and
/// their links as OxidGene extensions.
fn write_gedcom(
    data: &Dataset,
    target: ExportTarget<'_>,
    destination: &'static ExportProfile,
    kept: &HashSet<String>,
) -> Result<(String, Option<DestinationReport>), String> {
    let write_span = tracing::info_span!(
        "export.write",
        export.destination = destination.id(),
        export.output_bytes = tracing::field::Empty,
        export.lossy_folds = tracing::field::Empty,
    );
    let written = write_span
        .in_scope(|| match target {
            ExportTarget::Gedcom => {
                // The credentials are left out, as `oxidged` does by default.
                let exporter = Exporter::new(destination).options(ProfileOptions::default());
                let (gedcom, report) = exporter.write_to_string(data)?;
                Ok((gedcom, Some(destination_report(destination, &report))))
            }
            ExportTarget::Gedzip { .. } => {
                let writer = GedcomWriter::new()
                    .gedcom_version(GedcomVersion::V7_0)
                    .bom(Bom::Always);
                if kept.is_empty() {
                    return Ok((writer.write_to_string(data)?, None));
                }
                let mut records = data.to_structures();
                keep_as_extensions(&mut records, kept);
                let mut bytes = Vec::new();
                writer.write_structures(&mut bytes, &records)?;
                Ok((String::from_utf8_lossy(&bytes).into_owned(), None))
            }
        })
        .map_err(|e: oxidged::WriteError| format!("GEDCOM write error: {e}"))?;
    write_span.record("export.output_bytes", written.0.len());
    if let Some(report) = &written.1 {
        write_span.record("export.lossy_folds", report.losses.len());
    }
    Ok(written)
}

/// What writing for `destination` lost, as `oxidged`'s export `report` says
/// it: the folds that lost something, and the program's limits the file
/// exceeds. The folds that lost nothing (a value escaped, the records
/// reordered) and the writer's conformance repairs are left out.
fn destination_report(
    destination: &ExportProfile,
    report: &oxidged::profile::ExportReport,
) -> DestinationReport {
    DestinationReport {
        destination: destination.id().to_string(),
        losses: report
            .lossy()
            .map(|fold| ExportLoss {
                kind: fold_kind(fold.kind).to_string(),
                record: fold.record.as_ref().map(|x| x.as_str().to_string()),
                detail: fold.detail.clone(),
            })
            .collect(),
        limits: report
            .limits
            .iter()
            .map(|limit| ExportLimit {
                record: limit.record.as_ref().map(|x| x.as_str().to_string()),
                detail: limit.detail.clone(),
            })
            .collect(),
    }
}

/// The stable name of a fold's kind, which the interface translates.
fn fold_kind(kind: FoldKind) -> &'static str {
    match kind {
        FoldKind::Merged => "merged",
        FoldKind::Moved => "moved",
        FoldKind::Relabelled => "relabelled",
        FoldKind::Added => "added",
        FoldKind::AsText => "as_text",
        FoldKind::Omitted => "omitted",
        FoldKind::Reordered => "reordered",
        FoldKind::Recoded => "recoded",
        // A kind a later `oxidged` adds.
        _ => "other",
    }
}

/// Turn the `kept` `OBJE` records (by xref) into OxidGene's
/// `_OXIDGENE_OBJE` extension records, and every `OBJE` pointer to one of
/// them into an `_OXIDGENE_OBJE` pointer.
///
/// A record keeps its substructures — the producer's `FILE` path as it is,
/// its format, title, notes and the other OxidGene extensions — which an
/// extension record may hold as extension-defined substructures. A pointer
/// becomes an `_OXIDGENE_OBJE` pointer in the same place. Each carries its
/// position among the `OBJE` records, or among the `OBJE` links of the
/// structure holding it, as `_OXIDGENE_POS`: a reader keeps extensions
/// apart from the standard structures, and the order of a person's links
/// says which picture is their portrait.
fn keep_as_extensions(records: &mut [Structure], kept: &HashSet<String>) {
    let mut position = 0usize;
    for record in records {
        if record.tag.as_str() == "OBJE" {
            if record
                .xref
                .as_ref()
                .is_some_and(|x| kept.contains(x.as_str()))
            {
                record.tag = Tag::new(KEPT_MEDIA_TAG);
                record.substructures.insert(0, position_structure(position));
            }
            position += 1;
        } else {
            keep_links_as_extensions(&mut record.substructures, kept);
        }
    }
}

/// [`keep_as_extensions`] for the links among `structures`, at any depth.
fn keep_links_as_extensions(structures: &mut [Structure], kept: &HashSet<String>) {
    let mut position = 0usize;
    for structure in structures {
        if structure.tag.as_str() == "OBJE" {
            if let Payload::Pointer(xref) = &structure.payload
                && kept.contains(xref.as_str())
            {
                structure.tag = Tag::new(KEPT_MEDIA_TAG);
                structure.substructures.push(position_structure(position));
            }
            position += 1;
        } else {
            keep_links_as_extensions(&mut structure.substructures, kept);
        }
    }
}

/// An `_OXIDGENE_POS` structure holding `position`.
fn position_structure(position: usize) -> Structure {
    Structure {
        payload: Payload::Text(position.to_string().into()),
        ..Structure::new(POSITION_TAG)
    }
}

/// Whether a `FILE` is a web address, which GEDCOM 7.0 permits as an
/// `ftp`, `http` or `https` URL.
fn is_web_address(path: &str) -> bool {
    let path = path.trim();
    ["http://", "https://", "ftp://"].iter().any(|scheme| {
        path.get(..scheme.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(scheme))
    })
}

/// Every lookup the export reads, built once from the rows it writes.
struct ExportIndex<'a> {
    xrefs: Xrefs,
    sex_by_person: HashMap<Uuid, Sex>,
    place_map: HashMap<Uuid, &'a Place>,
    media_by_id: HashMap<Uuid, &'a Media>,
    pages_of: HashMap<Uuid, Vec<&'a Media>>,
    /// The events that are written, beneath a person or a family.
    written_events: HashSet<Uuid>,
    /// Every vignette that can be written, by id.
    vignettes_by_id: HashMap<Uuid, &'a Vignette>,
    /// The crops of a person that name no event, by id.
    crops_by_person: HashMap<Uuid, Vec<&'a Vignette>>,
    /// The crops beneath an event, by id.
    crops_by_event: HashMap<Uuid, Vec<&'a Vignette>>,
    /// The interned `_OXIDGENE_PERSON` tag.
    person_tag: TagId,
    /// The interned `_OXIDGENE_TAG` tag.
    tag_tag: TagId,
    names_by_person: HashMap<Uuid, Vec<&'a PersonName>>,
    events_by_person: HashMap<Uuid, Vec<&'a Event>>,
    events_by_family: HashMap<Uuid, Vec<&'a Event>>,
    assoc_by_person: HashMap<Uuid, Vec<GedAssociation>>,
    assoc_by_event: HashMap<Uuid, Vec<GedAssociation>>,
    cites_by_person: HashMap<Uuid, Vec<&'a Citation>>,
    cites_by_event: HashMap<Uuid, Vec<&'a Citation>>,
    cites_by_family: HashMap<Uuid, Vec<&'a Citation>>,
    notes_by_person: HashMap<Uuid, Vec<&'a Note>>,
    notes_by_family: HashMap<Uuid, Vec<&'a Note>>,
    notes_by_source: HashMap<Uuid, Vec<&'a Note>>,
    notes_by_event: HashMap<Uuid, Vec<&'a Note>>,
    notes_by_media: HashMap<Uuid, Vec<&'a Note>>,
    notes_by_repository: HashMap<Uuid, Vec<&'a Note>>,
    repos_by_source: HashMap<Uuid, Vec<&'a SourceRepository>>,
    mlinks_by_person: HashMap<Uuid, Vec<&'a MediaLink>>,
    mlinks_by_event: HashMap<Uuid, Vec<&'a MediaLink>>,
    mlinks_by_family: HashMap<Uuid, Vec<&'a MediaLink>>,
    spouses_by_family: HashMap<Uuid, Vec<&'a FamilySpouse>>,
    children_by_family: HashMap<Uuid, Vec<&'a FamilyChild>>,
    fams_by_person: HashMap<Uuid, Vec<&'a FamilySpouse>>,
    famc_by_person: HashMap<Uuid, Vec<&'a FamilyChild>>,
}

/// A text holding `value` when it says something.
fn some_text(value: Option<&str>) -> Option<Text> {
    non_blank(value).map(Text::from)
}

/// The texts of `value`, as many as it says something: none or one.
fn texts<C: FromIterator<Text>>(value: Option<&str>) -> C {
    some_text(value).into_iter().collect()
}

impl<'a> ExportIndex<'a> {
    /// Index the vignettes that can be written, by person and by event: the
    /// crops of a person name no event, the others name one.
    fn group_vignettes(&mut self, vignettes: &'a [Vignette]) {
        for vignette in vignettes {
            if vignette_problem(
                vignette,
                &self.xrefs,
                &self.media_by_id,
                &self.written_events,
            )
            .is_some()
            {
                continue;
            }
            self.vignettes_by_id.insert(vignette.id, vignette);
            match (vignette.person_id, vignette.event_id) {
                (Some(person), None) => self
                    .crops_by_person
                    .entry(person)
                    .or_default()
                    .push(vignette),
                (_, Some(event)) => self.crops_by_event.entry(event).or_default().push(vignette),
                (None, None) => {}
            }
        }
        for crops in self
            .crops_by_person
            .values_mut()
            .chain(self.crops_by_event.values_mut())
        {
            crops.sort_by_key(|vignette| vignette.id);
        }
    }

    /// The `OBJE` link of a crop: the page it cuts out, with the `CROP` of
    /// its rectangle and, beneath an event, the person it identifies as an
    /// `_OXIDGENE_PERSON` pointer. `None` when the vignette cannot be written.
    fn crop_link(&self, vignette: &Vignette, with_person: bool) -> Option<MultimediaLink> {
        let object = *self.xrefs.media.get(&vignette.media_id)?;
        let crop = Crop {
            top: Some(u32::try_from(vignette.y).ok()?),
            left: Some(u32::try_from(vignette.x).ok()?),
            height: Some(u32::try_from(vignette.height).ok()?),
            width: Some(u32::try_from(vignette.width).ok()?),
            ..Default::default()
        };
        let person = match (with_person, vignette.person_id) {
            (true, Some(person_id)) => Some(*self.xrefs.person.get(&person_id)?),
            _ => None,
        };
        let extra: Vec<Node> = person
            .map(|xref| Node {
                payload: Value::Pointer(xref),
                ..Node::new(self.person_tag)
            })
            .into_iter()
            .collect();
        Some(MultimediaLink {
            object: Some(object),
            crop: Some(Box::new(crop)),
            extra: extra.into(),
            ..Default::default()
        })
    }

    /// The crop links beneath an event, in vignette id order.
    fn event_crops(&self, event_id: Uuid) -> Vec<MultimediaLink> {
        self.crops_by_event
            .get(&event_id)
            .into_iter()
            .flatten()
            .filter_map(|vignette| self.crop_link(vignette, true))
            .collect()
    }
}

impl ExportIndex<'_> {
    /// The `SUBM` record: who the file is from.
    ///
    /// The tree's submitter name; else the display name of its "Who am I?"
    /// person; else `Not Provided` — Gramps' wording for the same gap — when
    /// the tree names nobody, or somebody nameless. Its email and its
    /// address, over several lines, when the settings hold them.
    /// The shared note record `note` is written as, when it was read from
    /// one; its own text otherwise.
    fn note_for(&self, note: &Note) -> GedNote {
        match note
            .shared_group
            .and_then(|group| self.xrefs.note_text.get(&(group, note.text.clone())))
        {
            Some(&xref) => GedNote::shared(xref),
            None => to_ged_note(&note.text),
        }
    }

    /// Every note of a record, each as its own text or as a pointer to the
    /// shared note record it was read from.
    fn all_notes<C: FromIterator<GedNote>>(&self, notes: Option<&Vec<&Note>>) -> C {
        notes
            .into_iter()
            .flatten()
            .map(|n| self.note_for(n))
            .collect()
    }

    /// The xref text of every record the export writes, mapped to the entity
    /// it was written for: the submitter to the nil UUID, a shared note
    /// record to its group.
    fn exported_xrefs(&self, store: &Store) -> HashMap<String, Uuid> {
        let text = |xref: XrefId| store.xref(xref).to_string();
        let mut xrefs = HashMap::new();
        for map in [
            &self.xrefs.person,
            &self.xrefs.family,
            &self.xrefs.source,
            &self.xrefs.repository,
            &self.xrefs.media,
        ] {
            for (&id, &xref) in map {
                xrefs.insert(text(xref), id);
            }
        }
        for (&(group, _), &xref) in &self.xrefs.note_text {
            xrefs.insert(text(xref), group);
        }
        xrefs.insert(text(self.xrefs.submitter), Uuid::nil());
        xrefs
    }

    fn submitter(
        &self,
        settings: SubmitterSettings<'_>,
        self_person_id: Option<Uuid>,
    ) -> Submitter {
        let name = settings
            .name
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .or_else(|| {
                self_person_id
                    .and_then(|id| self.names_by_person.get(&id))
                    .and_then(|names| PersonName::primary(names.iter().copied()))
                    .map(PersonName::display_name)
                    .filter(|name| !name.trim().is_empty())
            })
            .unwrap_or_else(|| "Not Provided".to_string());
        Submitter {
            xref: Some(self.xrefs.submitter),
            name: Some(Text::from(name)),
            address: settings.address.and_then(to_ged_address),
            emails: texts(settings.email),
            ..Default::default()
        }
    }

    /// A `REPO` record: its name, its address over several lines, its phone,
    /// email and website, and its notes.
    fn repository(&self, repo: &Repository) -> GedRepository {
        GedRepository {
            xref: self.xrefs.repository.get(&repo.id).copied(),
            name: Some(Text::from(repo.name.as_str())),
            address: repo.address.as_deref().and_then(to_ged_address),
            phones: texts(repo.phone.as_deref()),
            emails: texts(repo.email.as_deref()),
            websites: texts(repo.website.as_deref()),
            notes: self.all_notes(self.notes_by_repository.get(&repo.id)),
            ..Default::default()
        }
    }

    /// A `SOUR` record, with its publication facts, the agency responsible
    /// for its data, and the repositories holding it.
    fn source(&self, src: &Source) -> GedSource {
        GedSource {
            xref: self.xrefs.source.get(&src.id).copied(),
            title: Some(Text::from(src.title.as_str())),
            author: src.author.as_deref().map(Text::from),
            publication: some_text(src.publisher.as_deref()),
            abbreviation: src.abbreviation.as_deref().map(Text::from),
            data: some_text(src.agency.as_deref()).map(|agency| SourceData {
                agency: Some(agency),
                ..SourceData::default()
            }),
            notes: self.all_notes(self.notes_by_source.get(&src.id)),
            repositories: self.repository_citations(src.id),
            extra: tag_nodes(self.tag_tag, &src.tags).into(),
            ..Default::default()
        }
    }

    /// An `OBJE` record for one page, with the OxidGene extensions written
    /// beneath it (see [`media_extensions`]).
    fn multimedia(
        &self,
        m: &Media,
        media_paths: Option<&HashMap<Uuid, String>>,
        extra: Vec<Node>,
    ) -> GedMultimedia {
        // Title, description, category and medium describe the document, not
        // the scan. Reading them from the parent is what keeps a `.ged` other
        // software opens from showing thirty-eight untitled files — and what
        // keeps a one-page document, which is what an ordinary photograph now
        // is, exporting under the title its owner gave it.
        let document = m.parent_media_id.and_then(|id| self.media_by_id.get(&id));
        let described = document.copied().unwrap_or(m);
        // `file_path` is the producer's own path, preserved so a plain `.ged`
        // round-trips to whatever wrote it. A GEDZIP carries the bytes, so
        // there the `FILE` must name the entry inside the archive instead —
        // `media_paths` holds those; a GEDZIP's other media are web
        // addresses, or kept as extensions (see `keep_as_extensions`).
        let path = media_paths
            .and_then(|paths| paths.get(&m.id))
            .cloned()
            .unwrap_or_else(|| m.file_path.clone());
        // A category the user chose is the better answer and implies a
        // medium; the stored medium is what they said when they answered
        // GEDCOM's own question directly, so it wins where both are set.
        let medium = match (described.document_category, described.source_media_type) {
            (Some(category), SourceMediaType::Other) => category.implied_medium(),
            (_, medium) => medium,
        };
        let title = page_title(
            described,
            document
                .map(|d| pages_of_len(&self.pages_of, d.id))
                .unwrap_or(1),
            m.page_index,
        );
        let file = File {
            path: Text::from(path),
            form: Some(FileForm {
                // The MIME type: the writer writes it in the target version's
                // vocabulary (`jpg` in 5.5.1, `image/jpeg` in 7.0).
                format: Text::from(m.mime_type.as_str()),
                medium_type: to_ged_medium(medium).map(Box::new),
                ..FileForm::default()
            }),
            title: title.map(Text::from),
            ..File::default()
        };
        GedMultimedia {
            xref: self.xrefs.media.get(&m.id).copied(),
            files: std::iter::once(file).collect(),
            notes: described
                .description
                .as_deref()
                .map(to_ged_note)
                .into_iter()
                .collect(),
            extra: extra.into(),
            ..Default::default()
        }
    }

    /// An `INDI` record.
    fn individual(&self, person: &Person, warnings: &mut Vec<String>) -> Individual {
        // Names (GEDCOM allows {0:M} NAME structures; primary goes first
        // so `names.first()` on the way back in matches what we exported).
        let names: Vec<GedName> = self
            .names_by_person
            .get(&person.id)
            .map(|names| {
                let mut ordered: Vec<_> = names.iter().collect();
                ordered.sort_by_key(|n| !n.is_primary);
                ordered.into_iter().map(|pn| to_ged_name(pn)).collect()
            })
            .unwrap_or_default();

        let (events, ordinances) = self.individual_events(person.id, warnings);

        let (spouse_of, child_of) = to_ged_family_links(
            person.id,
            &self.fams_by_person,
            &self.famc_by_person,
            &self.xrefs.family,
            warnings,
        );
        let mut individual = Individual {
            xref: self.xrefs.person.get(&person.id).copied(),
            names,
            sex: Some(convert_sex(person.sex)),
            events,
            // FAMS/FAMC back-links to the families this person belongs to.
            spouse_of: spouse_of.into(),
            child_of: child_of.into(),
            citations: self
                .citations(self.cites_by_person.get(&person.id), warnings)
                .into(),
            notes: self.all_notes(self.notes_by_person.get(&person.id)),
            extra: tag_nodes(self.tag_tag, &person.tags).into(),
            ..Default::default()
        };
        let restriction = restriction(person.privacy);
        let associations = self
            .assoc_by_person
            .get(&person.id)
            .cloned()
            .unwrap_or_default();
        let multimedia = self.portrait_first_multimedia(person);
        if restriction.is_some() || !associations.is_empty() || !multimedia.is_empty() {
            let detail = individual.detail_mut();
            detail.restriction = restriction;
            detail.associations = associations;
            detail.multimedia = multimedia;
        }
        if !ordinances.is_empty() {
            individual.detail_mut().ordinances = ordinances;
        }
        individual
    }

    /// A person's events (GEDCOM INDIVIDUAL_EVENT_STRUCTURE) and attributes
    /// (INDIVIDUAL_ATTRIBUTE_STRUCTURE, e.g. OCCU), in order, each under its
    /// own tag rather than a generic EVEN, and the LDS ordinances the record
    /// holds (`BAPL`, `CONL`, `ENDL`, `INIL`, `SLGC`) apart from them.
    fn individual_events(
        &self,
        person_id: Uuid,
        warnings: &mut Vec<String>,
    ) -> (Vec<GedEvent>, Vec<Ordinance>) {
        let mut events = Vec::new();
        let mut ordinances = Vec::new();
        for evt in self.events_by_person.get(&person_id).into_iter().flatten() {
            if let Some(ordinance) = self.ordinance_of(evt, warnings) {
                ordinances.push(ordinance);
                continue;
            }
            events.push(match event_type_to_attribute(evt.event_type) {
                Some(attribute) => to_ged_attribute_detail(evt, attribute, self, warnings),
                None => to_ged_detail(evt, self, warnings),
            });
        }
        (events, ordinances)
    }

    /// A person's multimedia links: the portrait first, then the whole
    /// pictures, then the other crops.
    ///
    /// GEDCOM has no primary-photo flag, so the choice cannot be stated —
    /// but it can be *implied*, because order survives and our own import
    /// takes a person's first picture as the portrait. Writing the portrait
    /// first is therefore what carries the choice across a round trip. A
    /// crop portrait is written first too, as a `CROP` beneath the link; if
    /// it names an event, it is also written beneath that event.
    fn portrait_first_multimedia(&self, person: &Person) -> Vec<MultimediaLink> {
        let portrait = person
            .portrait_vignette_id
            .and_then(|id| self.vignettes_by_id.get(&id).copied())
            .filter(|vignette| vignette.person_id == Some(person.id));
        let mut whole: Vec<&&MediaLink> = self
            .mlinks_by_person
            .get(&person.id)
            .into_iter()
            .flatten()
            .collect();
        whole.sort_by_key(|ml| {
            (
                person.portrait_media_id != Some(ml.media_id),
                ml.sort_order,
                ml.id,
            )
        });
        let mut links: Vec<MultimediaLink> = portrait
            .and_then(|vignette| self.crop_link(vignette, false))
            .into_iter()
            .collect();
        links.extend(self.multimedia_refs(whole.into_iter().copied()));
        links.extend(
            self.crops_by_person
                .get(&person.id)
                .into_iter()
                .flatten()
                .filter(|vignette| portrait.is_none_or(|p| p.id != vignette.id))
                .filter_map(|vignette| self.crop_link(vignette, false)),
        );
        links
    }

    /// A source's `REPO` citations: one per repository link, with its call
    /// number and medium. GEDCOM 5.5.1 puts `MEDI` under `CALN`: a medium
    /// without a call number is written under an empty one.
    fn repository_citations(&self, source_id: Uuid) -> oxidged::model::ThinVec<RepositoryCitation> {
        self.repos_by_source
            .get(&source_id)
            .into_iter()
            .flatten()
            .filter_map(|link| {
                let xref = self.xrefs.repository.get(&link.repository_id)?;
                let call_number = link.call_number.clone().unwrap_or_default();
                let call_numbers = if call_number.is_empty() && link.media_type.is_none() {
                    Vec::new()
                } else {
                    vec![CallNumber {
                        value: Text::from(call_number),
                        medium: link.media_type.and_then(to_ged_medium).map(Phrased::new),
                        ..CallNumber::default()
                    }]
                };
                Some(RepositoryCitation {
                    repository: Some(*xref),
                    call_numbers,
                    ..Default::default()
                })
            })
            .collect()
    }

    /// A `FAM` record.
    fn family(&self, fam: &Family, warnings: &mut Vec<String>) -> GedFamily {
        let (husband, wife) = self.spouse_slots(fam, warnings);
        let children: Vec<IndividualRef> = self
            .children_by_family
            .get(&fam.id)
            .map(|cs| {
                // Ties in `sort_order` fall back to the children's xrefs.
                let mut sorted: Vec<(i32, XrefId)> = cs
                    .iter()
                    .filter_map(|fc| {
                        Some((
                            fc.sort_order,
                            self.xrefs.person.get(&fc.person_id).copied()?,
                        ))
                    })
                    .collect();
                sorted.sort_by_key(|(order, xref)| (*order, *xref));
                sorted
                    .into_iter()
                    .map(|(_, xref)| IndividualRef::new(xref))
                    .collect()
            })
            .unwrap_or_default();
        let mut events: Vec<GedEvent> = Vec::new();
        let mut ordinances: Vec<Ordinance> = Vec::new();
        for evt in self.events_by_family.get(&fam.id).into_iter().flatten() {
            if let Some(ordinance) = self.ordinance_of(evt, warnings) {
                ordinances.push(ordinance);
                continue;
            }
            // A family's count (NCHI) and fact (FACT) are attributes, written as
            // their line value with their TYPE beside it, as an individual's
            // are, not as a generic event. A family's other attributes (PROP)
            // are not permitted on its record and stay generic events.
            let mut event = match event_type_to_attribute(evt.event_type) {
                Some(attribute @ (EventKind::ChildrenCount | EventKind::Fact)) => {
                    to_ged_attribute_detail(evt, attribute, self, warnings)
                }
                _ => to_ged_detail(evt, self, warnings),
            };
            self.spouse_ages(&mut event, evt, [husband, wife], warnings);
            events.push(event);
        }

        let mut family = GedFamily {
            xref: self.xrefs.family.get(&fam.id).copied(),
            husband: husband.map(IndividualRef::new),
            wife: wife.map(IndividualRef::new),
            children,
            events,
            // Citations with a family_id but no event_id.
            citations: self
                .citations(self.cites_by_family.get(&fam.id), warnings)
                .into(),
            notes: self.all_notes(self.notes_by_family.get(&fam.id)),
            extra: tag_nodes(self.tag_tag, &fam.tags).into(),
            ..Default::default()
        };
        let restriction = restriction(fam.privacy);
        let multimedia = self.multimedia_refs(
            self.mlinks_by_family
                .get(&fam.id)
                .into_iter()
                .flatten()
                .copied(),
        );
        if restriction.is_some() || !multimedia.is_empty() {
            let detail = family.detail_mut();
            detail.restriction = restriction;
            detail.multimedia = multimedia;
        }
        if !ordinances.is_empty() {
            family.detail_mut().ordinances = ordinances;
        }
        family
    }

    /// The `HUSB.AGE` and `WIFE.AGE` of a family event, given the xrefs of
    /// the family's `HUSB` and `WIFE`. The age of a spouse written in neither
    /// slot is a warning.
    fn spouse_ages(
        &self,
        event: &mut GedEvent,
        evt: &Event,
        [husband, wife]: [Option<XrefId>; 2],
        warnings: &mut Vec<String>,
    ) {
        for spouse in &evt.spouse_ages {
            let xref = self.xrefs.person.get(&spouse.person_id).copied();
            let Some(age) = to_ged_age(Some(&spouse.age)) else {
                continue;
            };
            let member = Some(EventSpouse {
                age: Some(age),
                ..EventSpouse::default()
            });
            if xref.is_some() && xref == husband {
                event.detail_mut().husband = member;
            } else if xref.is_some() && xref == wife {
                event.detail_mut().wife = member;
            } else {
                warnings.push(format!(
                    "Event {}: the age of spouse {} has no HUSB or WIFE to be written under",
                    evt.id, spouse.person_id
                ));
            }
        }
    }

    /// A family's `HUSB` and `WIFE` xrefs.
    ///
    /// GEDCOM 5.5.1 has these two slots and no other. A husband and a wife
    /// take their own; a partner — or a second husband or wife — takes the
    /// slot their sex points to, else whichever is free, so that two partners
    /// are both written. A spouse left with no slot is a warning: GEDCOM
    /// cannot hold a third.
    fn spouse_slots(
        &self,
        fam: &Family,
        warnings: &mut Vec<String>,
    ) -> (Option<XrefId>, Option<XrefId>) {
        let mut spouses: Vec<&FamilySpouse> = self
            .spouses_by_family
            .get(&fam.id)
            .into_iter()
            .flatten()
            .copied()
            .filter(|s| self.xrefs.person.contains_key(&s.person_id))
            .collect();
        spouses.sort_by_key(|s| s.sort_order);
        let mut slots: [Option<Uuid>; 2] = [None, None];
        let mut unplaced = Vec::new();
        for spouse in spouses {
            let own = match spouse.role {
                SpouseRole::Husband => Some(0),
                SpouseRole::Wife => Some(1),
                SpouseRole::Partner => None,
            };
            match own {
                Some(slot) if slots[slot].is_none() => slots[slot] = Some(spouse.person_id),
                _ => unplaced.push(spouse),
            }
        }
        for spouse in unplaced {
            let preferred =
                usize::from(self.sex_by_person.get(&spouse.person_id) == Some(&Sex::Female));
            match [preferred, 1 - preferred]
                .into_iter()
                .find(|&slot| slots[slot].is_none())
            {
                Some(slot) => slots[slot] = Some(spouse.person_id),
                None => warnings.push(format!(
                    "Family {}: spouse {} has no HUSB or WIFE left to be written as",
                    fam.id, spouse.person_id
                )),
            }
        }
        let xref = |slot: Option<Uuid>| slot.and_then(|id| self.xrefs.person.get(&id).copied());
        (xref(slots[0]), xref(slots[1]))
    }

    /// The citations of a record, leaving out (with a warning) those whose
    /// source is not exported.
    fn citations(
        &self,
        cites: Option<&Vec<&Citation>>,
        warnings: &mut Vec<String>,
    ) -> Vec<GedCitation> {
        cites
            .into_iter()
            .flatten()
            .filter_map(|c| to_ged_citation(c, &self.xrefs.source, self.tag_tag, warnings))
            .collect()
    }

    /// The `OBJE` pointers of some media links, in order.
    fn multimedia_refs<'l>(
        &self,
        links: impl Iterator<Item = &'l MediaLink>,
    ) -> Vec<MultimediaLink> {
        links
            .flat_map(|ml| {
                to_ged_multimedia_refs(
                    ml.media_id,
                    &self.media_by_id,
                    &self.xrefs.media,
                    &self.pages_of,
                )
            })
            .collect()
    }

    /// The `PLAC` of an event, when it has a place the export writes.
    fn event_place(&self, evt: &Event) -> Option<GedPlace> {
        evt.place_id
            .and_then(|pid| self.place_map.get(&pid))
            .map(|p| to_ged_place(p))
    }

    /// The LDS ordinance an event is written as, when GEDCOM's ordinance
    /// structure has room for all of it: the kind's own tag, on the individual
    /// (or, for a couple's sealing, the family). Its description, media, age,
    /// agency, cause, classification and spouses' ages have no room in it, so
    /// an event holding any of them stays a generic event `to_ged_detail`
    /// writes. `None` for an event that is not an ordinance, or is one on the
    /// other record.
    fn ordinance_of(&self, evt: &Event, warnings: &mut Vec<String>) -> Option<Ordinance> {
        let has_media = self
            .mlinks_by_event
            .get(&evt.id)
            .is_some_and(|links| !links.is_empty())
            || self.crops_by_event.contains_key(&evt.id);
        let kind = ordinance_written_as(evt, has_media)?;
        let mut ordinance = Ordinance::new(kind);
        ordinance.date = event_date(evt);
        ordinance.place = self.event_place(evt);
        if kind == OrdinanceKind::ChildSealing {
            ordinance.family = self.related_family_xref(evt);
        }
        ordinance.citations = self
            .citations(self.cites_by_event.get(&evt.id), warnings)
            .into();
        ordinance.notes = self.all_notes(self.notes_by_event.get(&evt.id));
        ordinance.extra = tag_nodes(self.tag_tag, &evt.tags).into();
        Some(ordinance)
    }

    /// The `FAMC` an individual event names without belonging to it is
    /// written with: the export xref of its related family. `None` when that
    /// family is not exported, which leaves the pointer out.
    fn related_family_xref(&self, evt: &Event) -> Option<XrefId> {
        self.xrefs.family.get(&evt.related_family_id?).copied()
    }

    /// What an event and an attribute write alike: its date, place,
    /// citations, notes and media.
    fn event_parts(&self, evt: &Event, warnings: &mut Vec<String>) -> GedEvent {
        let mut event = GedEvent {
            date: event_date(evt),
            place: self.event_place(evt),
            citations: self
                .citations(self.cites_by_event.get(&evt.id), warnings)
                .into(),
            extra: tag_nodes(self.tag_tag, &evt.tags).into(),
            ..GedEvent::default()
        };
        let notes: Vec<GedNote> = self.all_notes(self.notes_by_event.get(&evt.id));
        let mut multimedia = self.multimedia_refs(
            self.mlinks_by_event
                .get(&evt.id)
                .into_iter()
                .flatten()
                .copied(),
        );
        multimedia.extend(self.event_crops(evt.id));
        let associations = self
            .assoc_by_event
            .get(&evt.id)
            .cloned()
            .unwrap_or_default();
        if !notes.is_empty() || !multimedia.is_empty() || !associations.is_empty() {
            let detail = event.detail_mut();
            detail.notes = notes;
            detail.multimedia = multimedia;
            detail.associations = associations;
        }
        event
    }
}

/// A `PLAC` with its `MAP` coordinates when both are known.
fn to_ged_place(p: &Place) -> GedPlace {
    let mut place = GedPlace {
        name: Text::from(p.name.as_str()),
        ..Default::default()
    };
    if let (Some(lat), Some(lon)) = (p.latitude, p.longitude) {
        place.detail = Some(Box::new(PlaceDetail {
            map: Some(Map {
                latitude: Some(Text::from(format_coord(lat, true))),
                longitude: Some(Text::from(format_coord(lon, false))),
                ..Map::default()
            }),
            ..PlaceDetail::default()
        }));
    }
    place
}

/// `text` when it says something.
fn non_blank(text: Option<&str>) -> Option<&str> {
    text.filter(|text| !text.trim().is_empty())
}

/// An address held as one text over several lines, as GEDCOM's `ADDR`
/// writes it, one `CONT` per further line.
fn to_ged_address(text: &str) -> Option<GedAddress> {
    Some(GedAddress {
        value: Text::from(non_blank(Some(text))?.trim()),
        ..Default::default()
    })
}

/// The `RESN` of a person or a family: `confidential` for a private record,
/// GEDCOM's word for data its owner marked to be kept from reports and
/// exports, and the one the import reads back as private. A record
/// following the tree, or public, has none: it comes back as following the
/// tree.
fn restriction(privacy: Privacy) -> Option<EnumList<Restriction>> {
    (privacy == Privacy::Private).then(|| EnumList(vec![Restriction::Confidential]))
}

/// An OxidGene extension structure holding `value`.
fn extension(tag: TagId, value: String) -> Node {
    Node {
        payload: Value::Text(Text::from(value)),
        ..Node::new(tag)
    }
}

/// One `_OXIDGENE_TAG` line per tag, beneath the record carrying them
/// (docs/api.md, OxidGene extensions).
fn tag_nodes(tag: TagId, tags: &[TagRef]) -> Vec<Node> {
    tags.iter()
        .filter_map(|tag_ref| {
            let value = serde_json::to_string(&TagExtension {
                v: 1,
                label: tag_ref.label.clone(),
                color: tag_ref.color,
            })
            .ok()?;
            Some(extension(tag, value))
        })
        .collect()
}

/// A warning for each tagged document with no page to carry its tags.
///
/// A document's tags are written beneath its first page's `OBJE` (see
/// docs/api.md, OxidGene extensions), and a document holding no page has no `OBJE` to carry
/// them: they are not exported, and the export says so.
fn untaggable_documents(media: &[Media]) -> Vec<String> {
    media
        .iter()
        .filter(|m| m.is_document() && !m.tags.is_empty())
        .filter(|document| {
            !media
                .iter()
                .any(|m| m.parent_media_id == Some(document.id) && m.page_index == 0)
        })
        .map(|document| {
            format!(
                "Media {} tags are not exported: the document has no page to carry them",
                document.id
            )
        })
        .collect()
}

/// The OxidGene extensions written beneath each page's `OBJE` record, by
/// page id: `_OXIDGENE_MEDIA` (what the page says about itself),
/// `_OXIDGENE_DOC` (the document it belongs to) and one `_OXIDGENE_VIGNETTE`
/// per crop that names neither a person nor an event. The attributed crops
/// are written as `CROP`s beneath their links instead (see
/// [`ExportIndex::crop_link`]).
fn media_extensions(
    store: &mut Store,
    media: &[Media],
    vignettes: &[Vignette],
    index: &ExportIndex,
) -> (HashMap<Uuid, Vec<Node>>, Vec<String>) {
    let media_tag = store.intern_tag(MEDIA_TAG);
    let document_tag = store.intern_tag(DOCUMENT_TAG);
    let vignette_tag = store.intern_tag(VIGNETTE_TAG);
    let tag_tag = store.intern_tag(TAG_TAG);
    let mut warnings = Vec::new();
    let mut by_media = HashMap::<Uuid, Vec<Node>>::new();
    let ExportIndex {
        xrefs,
        place_map: places,
        notes_by_media,
        ..
    } = index;

    let media_by_id: HashMap<Uuid, &Media> = media.iter().map(|m| (m.id, m)).collect();
    let notes_of = |media_id: Uuid| -> Vec<MediaNoteExtension> {
        notes_by_media
            .get(&media_id)
            .into_iter()
            .flatten()
            .map(|note| MediaNoteExtension {
                text: note.text.clone(),
                created_at: Some(note.created_at),
                updated_at: Some(note.updated_at),
            })
            .collect()
    };

    // A token per document, stable within this file and meaningless outside
    // it: the pages of one document are grouped by it on the way back in.
    let mut document_token = HashMap::<Uuid, String>::new();
    let mut page_count_of = HashMap::<Uuid, i32>::new();
    for item in media.iter().filter(|item| !item.is_document()) {
        if let Some(document_id) = item.parent_media_id {
            let next = document_token.len() + 1;
            document_token
                .entry(document_id)
                .or_insert_with(|| format!("D{next}"));
            *page_count_of.entry(document_id).or_insert(0) += 1;
        }
    }

    for item in media.iter().filter(|item| !item.is_document()) {
        if !xrefs.media.contains_key(&item.id) {
            continue;
        }
        // Page-level, and only page-level: the file it was, and its transcript.
        let metadata = MediaMetadataExtension {
            version: 1,
            file_name: item.file_name.clone(),
            created_at: Some(item.created_at),
            updated_at: Some(item.updated_at),
            notes: notes_of(item.id),
        };
        match serde_json::to_string(&metadata) {
            Ok(value) => by_media
                .entry(item.id)
                .or_default()
                .push(extension(media_tag, value)),
            Err(err) => warnings.push(format!(
                "Media {} metadata could not be serialized: {err}",
                item.id
            )),
        }

        // The container. GEDCOM has none, so this is what reassembles the
        // pages — and carries, on the first of them, everything the document
        // says about itself.
        let Some(document_id) = item.parent_media_id else {
            continue;
        };
        let Some(token) = document_token.get(&document_id) else {
            continue;
        };
        let meta = (item.page_index == 0)
            .then(|| media_by_id.get(&document_id))
            .flatten()
            .map(|document| DocumentMetadataExtension {
                file_name: document.file_name.clone(),
                created_at: Some(document.created_at),
                updated_at: Some(document.updated_at),
                title: document.title.clone(),
                description: document.description.clone(),
                date_value: document.date_value.clone(),
                date_qualifier: document.date_qualifier,
                date_value2: document.date_value2.clone(),
                calendar: document.calendar,
                privacy: document.privacy,
                source_media_type: document.source_media_type,
                document_category: document.document_category,
                place: document.place_id.and_then(|place_id| {
                    places.get(&place_id).map(|place| MediaPlaceExtension {
                        name: place.name.clone(),
                        latitude: place.latitude,
                        longitude: place.longitude,
                    })
                }),
                notes: notes_of(document.id),
            });
        let container = DocumentExtension {
            version: 1,
            doc: token.clone(),
            index: item.page_index,
            count: page_count_of.get(&document_id).copied().unwrap_or(0),
            meta,
        };
        match serde_json::to_string(&container) {
            Ok(value) => by_media
                .entry(item.id)
                .or_default()
                .push(extension(document_tag, value)),
            Err(err) => warnings.push(format!(
                "Media {} document metadata could not be serialized: {err}",
                item.id
            )),
        }

        // The document's tags ride beneath its first page (docs/api.md, OxidGene extensions).
        if item.page_index == 0
            && let Some(document) = media_by_id.get(&document_id)
        {
            by_media
                .entry(item.id)
                .or_default()
                .extend(tag_nodes(tag_tag, &document.tags));
        }
    }

    for vignette in vignettes {
        if let Some(problem) =
            vignette_problem(vignette, xrefs, &index.media_by_id, &index.written_events)
        {
            warnings.push(format!("Vignette {} {problem}", vignette.id));
            continue;
        }
        // An attributed crop is a `CROP` beneath its link, not this record.
        if vignette.person_id.is_some() || vignette.event_id.is_some() {
            continue;
        }
        by_media
            .entry(vignette.media_id)
            .or_default()
            .push(extension(
                vignette_tag,
                format!(
                    "{} {} {} {}",
                    vignette.x, vignette.y, vignette.width, vignette.height
                ),
            ));
    }
    (by_media, warnings)
}

/// The events that are written, beneath a person or a family.
fn written_events(events: &[Event]) -> HashSet<Uuid> {
    events
        .iter()
        .filter(|evt| evt.person_id.is_some() || evt.family_id.is_some())
        .map(|evt| evt.id)
        .collect()
}

/// The events that have media: a link to one, or a crop beneath it that can
/// be written. An ordinance with media is written as a generic event.
fn linked_events(
    media_links: &[MediaLink],
    vignettes: &[Vignette],
    xrefs: &Xrefs,
    media_by_id: &HashMap<Uuid, &Media>,
    written_events: &HashSet<Uuid>,
) -> HashSet<Uuid> {
    let linked_by_crop = vignettes
        .iter()
        .filter(|v| vignette_problem(v, xrefs, media_by_id, written_events).is_none())
        .filter_map(|v| v.event_id);
    media_links
        .iter()
        .filter_map(|ml| ml.event_id)
        .chain(linked_by_crop)
        .collect()
}

/// Why a vignette cannot be written, if it cannot: its page, person and event
/// must be part of the export, and its rectangle must be one GEDCOM can state.
fn vignette_problem(
    vignette: &Vignette,
    xrefs: &Xrefs,
    media_by_id: &HashMap<Uuid, &Media>,
    written_events: &HashSet<Uuid>,
) -> Option<String> {
    if !xrefs.media.contains_key(&vignette.media_id) {
        return Some(format!(
            "references media {} which is not part of this export",
            vignette.media_id
        ));
    }
    if media_by_id
        .get(&vignette.media_id)
        .is_some_and(|media| media.is_document())
    {
        return Some(format!(
            "refers to document {} rather than to a page",
            vignette.media_id
        ));
    }
    if let Some(person_id) = vignette.person_id
        && !xrefs.person.contains_key(&person_id)
    {
        return Some(format!(
            "references person {person_id} who is not part of this export"
        ));
    }
    if let Some(event_id) = vignette.event_id
        && !written_events.contains(&event_id)
    {
        return Some(format!(
            "references event {event_id} which is not part of this export"
        ));
    }
    if vignette.x < 0 || vignette.y < 0 || vignette.width <= 0 || vignette.height <= 0 {
        return Some("has a crop rectangle that GEDCOM cannot state".to_owned());
    }
    None
}

/// Where a media's bytes live inside a GEDZIP, if we hold any.
///
/// `None` for a record with no stored bytes — a GEDCOM import that named a
/// file nobody ever uploaded, or a remote URL we deliberately never fetched.
/// Those keep their original `FILE` value, which is the only thing we know
/// about them.
///
/// The name is the media's id rather than its own file name: two scans called
/// `photo.jpg` are routine in one tree, and an archive cannot hold both under
/// that name. The extension is kept so the file opens by double-click after
/// unzipping.
#[must_use]
pub fn archive_path(media: &Media) -> Option<String> {
    media.storage_key.as_ref()?;
    let extension = media
        .file_name
        .rsplit_once('.')
        .map(|(_, ext)| ext)
        .filter(|ext| !ext.is_empty() && ext.len() <= 4 && ext.chars().all(char::is_alphanumeric))
        .map(str::to_ascii_lowercase)
        .or_else(|| extension_for(&media.mime_type).map(str::to_string));
    Some(match extension {
        Some(ext) => format!("media/{}.{ext}", media.id),
        None => format!("media/{}", media.id),
    })
}

/// The conventional extension for a MIME type, for media whose file name
/// carries none.
fn extension_for(mime_type: &str) -> Option<&'static str> {
    Some(match mime_type {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/tiff" => "tif",
        "image/webp" => "webp",
        "application/pdf" => "pdf",
        _ => return None,
    })
}

/// Wrap a GEDCOM text and its media into a GEDZIP archive, per GEDCOM 7.0.
///
/// `gedcom` is the archive's `gedcom.ged` as it is stored: the GEDCOM 7.0
/// [`export_gedcom`] writes for [`ExportTarget::Gedzip`]. `files` pairs each
/// archive path — the same value the corresponding `FILE` line carries, from
/// [`archive_path`] — with the bytes to store there. An
/// empty slice produces the bare `gedcom.ged` archive, which is what this
/// wrote unconditionally before: the format's entire point is that the media
/// travel with the data, and a `.gdz` holding only the GEDCOM is a `.ged` in
/// a costume. A file already compressed — a JPEG, a PNG, a PDF — is stored as
/// it is, any other deflated.
///
/// # Errors
///
/// Returns `Err` if the ZIP archive cannot be written.
pub fn export_gedzip(gedcom: &str, files: &[(String, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let mut writer = GedzipFileWriter::new(std::io::Cursor::new(Vec::new()), gedcom)?;
    for (path, bytes) in files {
        writer.add_media_file(path, bytes)?;
    }
    writer.finish().map(std::io::Cursor::into_inner)
}

/// A GEDZIP archive written as its media are read, so that each can be
/// released before the next is loaded.
pub struct GedzipFileWriter<W: std::io::Write + std::io::Seek = std::fs::File> {
    writer: GedzipWriter<W>,
}

impl GedzipFileWriter {
    /// Create an archive file and write its mandatory `gedcom.ged` entry:
    /// `gedcom`, as [`export_gedzip`] takes it.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the file cannot be created or written.
    pub fn create(path: &std::path::Path, gedcom: &str) -> Result<Self, String> {
        let file = std::fs::File::create(path).map_err(|e| format!("GEDZIP error: {e}"))?;
        Self::new(file, gedcom)
    }
}

impl<W: std::io::Write + std::io::Seek> GedzipFileWriter<W> {
    fn new(destination: W, gedcom: &str) -> Result<Self, String> {
        let mut writer =
            GedzipWriter::new(destination).map_err(|e| format!("GEDZIP error: {e}"))?;
        writer
            .write_gedcom_bytes(gedcom.as_bytes())
            .map_err(|e| format!("GEDZIP error: {e}"))?;
        Ok(Self { writer })
    }

    /// Add one media entry, stored as it is when already compressed.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the entry cannot be written.
    pub fn add_media_file(&mut self, path: &str, bytes: &[u8]) -> Result<(), String> {
        self.writer
            .add_media_file(path, bytes)
            .map_err(|e| format!("GEDZIP error: {e}"))
    }

    /// Finalize the ZIP central directory and hand back the destination.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the archive cannot be finalized.
    pub fn finish(self) -> Result<W, String> {
        self.writer
            .finish()
            .map_err(|e| format!("GEDZIP error: {e}"))
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Conversion helpers
// ═══════════════════════════════════════════════════════════════════════

fn convert_sex(sex: Sex) -> GedSex {
    match sex {
        Sex::Male => GedSex::Male,
        Sex::Female => GedSex::Female,
        Sex::Unknown => GedSex::Undetermined,
    }
}

/// Builds a person's INDI-level `FAMS`/`FAMC` back-links: one per family
/// they're a spouse in, and one per family they're a child in. Without these,
/// the exported file only encodes family membership on the `FAM` record's own
/// `HUSB`/`WIFE`/`CHIL` — most GEDCOM readers instead (or additionally)
/// expect the reverse links on `INDI`, so omitting them makes the file read
/// as a set of disconnected individuals in other software.
fn to_ged_family_links(
    person_id: Uuid,
    fams_by_person: &HashMap<Uuid, Vec<&FamilySpouse>>,
    famc_by_person: &HashMap<Uuid, Vec<&FamilyChild>>,
    family_xref: &HashMap<Uuid, XrefId>,
    warnings: &mut Vec<String>,
) -> (Vec<SpouseLink>, Vec<ChildLink>) {
    let mut found = |family_id: Uuid, kind: &str| {
        let xref = family_xref.get(&family_id).copied();
        if xref.is_none() {
            warnings.push(format!(
                "Person {person_id}: {kind} family {family_id} not found"
            ));
        }
        xref
    };
    // Links are written in the order of the families' own xrefs (`F1`,
    // `F2`, ...), which is the order the families were numbered in. Their
    // rows were created in no fixed order, so without this a re-import would
    // reorder them.
    let mut spouse_links: Vec<(XrefId, SpouseLink)> = fams_by_person
        .get(&person_id)
        .into_iter()
        .flatten()
        .filter_map(|fs| found(fs.family_id, "spouse").map(|x| (x, SpouseLink::new(x))))
        .collect();
    spouse_links.sort_by_key(|(xref, _)| *xref);
    let mut child_links: Vec<(XrefId, ChildLink)> = famc_by_person
        .get(&person_id)
        .into_iter()
        .flatten()
        .filter_map(|fc| {
            let xref = found(fc.family_id, "parental")?;
            let mut link = ChildLink::new(xref);
            if let Some(pedigree) = convert_child_type_to_pedigree(fc.child_type) {
                link.detail_mut().pedigree = Some(Phrased::new(pedigree));
            }
            Some((xref, link))
        })
        .collect();
    child_links.sort_by_key(|(xref, _)| *xref);
    (
        spouse_links.into_iter().map(|(_, link)| link).collect(),
        child_links.into_iter().map(|(_, link)| link).collect(),
    )
}

/// The inverse of `import`'s `convert_pedigree`. `ChildType::Step` and
/// `::Unknown` have no GEDCOM 5.5.1 `PEDI` equivalent, so `PEDI` is simply
/// omitted for those (a valid, optional tag).
fn convert_child_type_to_pedigree(child_type: ChildType) -> Option<GedPedigree> {
    match child_type {
        ChildType::Biological => Some(GedPedigree::Birth),
        ChildType::Adopted => Some(GedPedigree::Adopted),
        ChildType::Foster => Some(GedPedigree::Foster),
        ChildType::Step | ChildType::Unknown => None,
    }
}

/// The pieces of a name, in GEDCOM's order, each written when it is set.
fn name_pieces(pn: &PersonName) -> oxidged::model::ThinVec<NamePiece> {
    [
        (NamePieceKind::Prefix, pn.prefix.as_deref()),
        (NamePieceKind::Given, pn.given_names.as_deref()),
        (NamePieceKind::Nickname, pn.nickname.as_deref()),
        (NamePieceKind::SurnamePrefix, pn.surname_prefix.as_deref()),
        // SURN is the root alone; the particle rides in SPFX beside it.
        (NamePieceKind::Surname, pn.surname.as_deref()),
        (NamePieceKind::Suffix, pn.suffix.as_deref()),
    ]
    .into_iter()
    .filter_map(|(kind, value)| {
        Some(NamePiece {
            kind,
            value: Text::from(value?),
        })
    })
    .collect()
}

fn to_ged_name(pn: &PersonName) -> GedName {
    // Build the GEDCOM full name value: "Given /Surname/". The surname goes in
    // with its particle, as GEDCOM expects — SPFX below repeats it separately.
    let given_part = pn.given_names.as_deref().unwrap_or("");
    let full_surname = pn.full_surname().unwrap_or_default();
    let surname_part = full_surname.as_str();
    let value = if !given_part.is_empty() || !surname_part.is_empty() {
        format!("{given_part} /{surname_part}/")
            .trim_start()
            .to_string()
    } else {
        String::new()
    };

    let name_type = match pn.name_type {
        NameType::Birth => Some(GedNameType::Birth),
        NameType::Married => Some(GedNameType::Married),
        NameType::Maiden => Some(GedNameType::Maiden),
        NameType::AlsoKnownAs => Some(GedNameType::Aka),
        // GEDCOM 5.5.1's NAME.TYPE is an open set; 7.0 would make it `OTHER`
        // with this phrase.
        NameType::Religious => Some(GedNameType::Unknown(Text::from("religious"))),
        // GEDCOM's NAME.TYPE enumeration has no finer-grained "also known as",
        // so OxidGene's four refinements all export as `aka`. The distinction
        // survives internally, not across a GEDCOM round trip.
        NameType::GivenName | NameType::Alias | NameType::Byname | NameType::Sobriquet => {
            Some(GedNameType::Aka)
        }
        NameType::Other => None,
    };

    let mut name = GedName {
        value: Text::from(value),
        pieces: name_pieces(pn),
        ..Default::default()
    };
    if let Some(kind) = name_type {
        name.detail_mut().kind = Some(Phrased::new(kind));
    }
    name
}

/// The LDS ordinance an event type is written as, when it has one.
fn ordinance_kind(et: EventType) -> Option<OrdinanceKind> {
    match et {
        EventType::LdsBaptism => Some(OrdinanceKind::Baptism),
        EventType::LdsConfirmation => Some(OrdinanceKind::Confirmation),
        EventType::Endowment => Some(OrdinanceKind::Endowment),
        EventType::LdsInitiatory => Some(OrdinanceKind::Initiatory),
        EventType::SealingChild => Some(OrdinanceKind::ChildSealing),
        EventType::SealingSpouse => Some(OrdinanceKind::SpouseSealing),
        _ => None,
    }
}

/// The `DATE` of an event: its date, recomposed with the calendar escape and
/// qualifier tag the columns were split from on import (see `crate::date`).
fn event_date(evt: &Event) -> Option<Date> {
    crate::date::format(
        evt.calendar,
        evt.date_qualifier,
        evt.date_value.as_deref(),
        evt.date_value2.as_deref(),
    )
    .map(Date::new)
}

/// The GEDCOM tag an event type is written under: its own wherever GEDCOM
/// 5.5.1 and 7.0 define one, else the generic `EVEN`, which
/// [`even_type_label`] names. A bar or bat mitzvah is written under the tag
/// it was read from (`BARM` or `BASM`), so it does not depend on the sex of
/// the person.
///
/// Neither version defines `MILI` (an extension some software writes) nor
/// `SEP` (which GeneWeb writes), so a military service and a separation are
/// an `EVEN` any reader understands.
fn convert_event_type(et: EventType) -> EventKind {
    match et {
        EventType::Birth => EventKind::Birth,
        EventType::Death => EventKind::Death,
        EventType::Baptism => EventKind::Baptism,
        EventType::Confirmation => EventKind::Confirmation,
        EventType::FirstCommunion => EventKind::FirstCommunion,
        EventType::BarBatMitzvah => EventKind::BarMitzvah,
        EventType::BasMitzvah => EventKind::BasMitzvah,
        EventType::Burial => EventKind::Burial,
        EventType::Cremation => EventKind::Cremation,
        EventType::Graduation => EventKind::Graduation,
        EventType::Immigration => EventKind::Immigration,
        EventType::Emigration => EventKind::Emigration,
        EventType::Naturalization => EventKind::Naturalization,
        EventType::Census => EventKind::Census,
        EventType::Residence => EventKind::Residence,
        EventType::Retirement => EventKind::Retirement,
        EventType::Will => EventKind::Will,
        EventType::Probate => EventKind::Probate,
        EventType::Marriage => EventKind::Marriage,
        EventType::Divorce => EventKind::Divorce,
        EventType::Annulment => EventKind::Annulment,
        EventType::Engagement => EventKind::Engagement,
        EventType::MarriageBann => EventKind::MarriageBann,
        EventType::MarriageContract => EventKind::MarriageContract,
        EventType::MarriageLicense => EventKind::MarriageLicense,
        EventType::MarriageSettlement => EventKind::MarriageSettlement,
        EventType::DivorceFiled => EventKind::DivorceFiled,
        EventType::Adoption => EventKind::Adoption,
        EventType::Blessing => EventKind::Blessing,
        EventType::Ordination => EventKind::Ordination,
        EventType::Christening => EventKind::Christening,
        EventType::AdultChristening => EventKind::AdultChristening,
        // The individual-attribute variants are written by
        // `to_ged_attribute_detail` under their own attribute tag (see
        // `individual_events`); a family's attributes (`NCHI`, `RESI`,
        // `FACT`) under theirs.
        EventType::Occupation => EventKind::Occupation,
        EventType::CasteName => EventKind::Caste,
        EventType::PhysicalDescription => EventKind::PhysicalDescription,
        EventType::Education => EventKind::Education,
        EventType::NationalId => EventKind::IdNumber,
        EventType::NationalOrigin => EventKind::Nationality,
        EventType::ChildrenCount => EventKind::ChildrenCount,
        EventType::MarriagesCount => EventKind::MarriageCount,
        EventType::Property => EventKind::Property,
        EventType::Religion => EventKind::Religion,
        EventType::SocialSecurityNumber => EventKind::SocialSecurityNumber,
        EventType::NobilityTitle => EventKind::Title,
        EventType::Fact => EventKind::Fact,
        // The LDS ordinances have tags of their own (`BAPL`, `CONL`, `ENDL`,
        // `INIL`, `SLGC`, `SLGS`), written as ordinance structures by
        // `ordinance_of`. An event that holds a description, media, age,
        // agency or cause, which an ordinance has no room for, is the generic
        // `EVEN` that `even_type_label` names, so that it is written whole.
        EventType::MilitaryService
        | EventType::CivilUnion
        | EventType::Separation
        | EventType::Accomplishment
        | EventType::Acquisition
        | EventType::Membership
        | EventType::ChangeName
        | EventType::Circumcision
        | EventType::Award
        | EventType::MilitaryDischarge
        | EventType::Degree
        | EventType::Distinction
        | EventType::Election
        | EventType::Excommunication
        | EventType::Funeral
        | EventType::Hospitalization
        | EventType::Illness
        | EventType::PassengerList
        | EventType::MilitaryDistinction
        | EventType::MilitaryPromotion
        | EventType::MilitaryMobilization
        | EventType::PropertySale
        | EventType::Endowment
        | EventType::LdsDotation
        | EventType::SealingChild
        | EventType::SealingSpouse
        | EventType::SealingParent
        | EventType::FamilyLinkLds
        | EventType::NoMarriage
        | EventType::LdsBaptism
        | EventType::LdsConfirmation
        | EventType::LdsInitiatory
        | EventType::NoMention
        | EventType::Other => EventKind::Event,
    }
}

/// The `TYPE` that names this event type on a generic `EVEN`; `None` for a
/// type written under a tag of its own.
///
/// The import reads each of these back as the type it names, and as nothing
/// more: the label is the type, not a description of the event.
pub(crate) fn even_type_label(et: EventType) -> Option<&'static str> {
    match et {
        EventType::MilitaryService => Some("Military service"),
        EventType::CivilUnion => Some("Civil union"),
        EventType::Separation => Some("Separation"),
        EventType::Accomplishment => Some("Accomplishment"),
        EventType::Acquisition => Some("Acquisition"),
        EventType::Membership => Some("Membership"),
        EventType::ChangeName => Some("Change name"),
        EventType::Circumcision => Some("Circumcision"),
        EventType::Award => Some("Award"),
        EventType::MilitaryDischarge => Some("Military discharge"),
        EventType::Degree => Some("Degree"),
        EventType::Distinction => Some("Distinction"),
        EventType::Election => Some("Election"),
        EventType::Excommunication => Some("Excommunication"),
        EventType::Funeral => Some("Funeral"),
        EventType::Hospitalization => Some("Hospitalization"),
        EventType::Illness => Some("Illness"),
        EventType::PassengerList => Some("Passenger list"),
        EventType::MilitaryDistinction => Some("Military distinction"),
        EventType::MilitaryPromotion => Some("Military promotion"),
        EventType::MilitaryMobilization => Some("Military mobilization"),
        EventType::PropertySale => Some("Property sale"),
        EventType::Endowment => Some("ENDL"),
        EventType::LdsDotation => Some("DotationLDS"),
        EventType::SealingChild => Some("SLGC"),
        EventType::SealingSpouse => Some("SLGS"),
        EventType::SealingParent => Some("Scellent parent LDS"),
        EventType::FamilyLinkLds => Some("Family link LDS"),
        EventType::NoMarriage => Some("unmarried"),
        EventType::LdsBaptism => Some("BAPL"),
        EventType::LdsConfirmation => Some("CONL"),
        EventType::LdsInitiatory => Some("INIL"),
        EventType::NoMention => Some("nomen"),
        EventType::Other => Some("Other"),
        _ => None,
    }
}

/// The line value and `TYPE` of an event written as a generic `EVEN`.
///
/// GEDCOM classifies an `EVEN` by its `TYPE`, which 7.0 requires, and lets
/// its line value describe it. A description that reads back as this very
/// type — "PACS" for a civil union, "Land lease" for an event of no other
/// type — is the `TYPE`, as other software writes it. Any other description
/// goes on the line under the type's own label, so that neither the type nor
/// the description changes on the way back in.
fn even_value_and_type(et: EventType, description: Option<&str>) -> (Option<String>, String) {
    let label = even_type_label(et).unwrap_or("Other");
    match description.filter(|d| !d.trim().is_empty()) {
        None => (None, label.to_owned()),
        Some(d) if crate::import::type_text_reads_as(d, et) => (None, d.to_owned()),
        Some(d) => (Some(d.to_owned()), label.to_owned()),
    }
}

/// The `QUAY` a confidence is written as; none for a citation nobody
/// assessed. `QUAY` has four values for five levels, so `VeryHigh` shares
/// `3` with `High`. The inverse of `import`'s `convert_quay`.
fn convert_confidence(c: Option<Confidence>) -> Option<Certainty> {
    Some(match c? {
        Confidence::VeryLow => Certainty::Unreliable,
        Confidence::Low => Certainty::Questionable,
        Confidence::Medium => Certainty::Secondary,
        Confidence::High | Confidence::VeryHigh => Certainty::Primary,
    })
}

/// The `AGE` a stored age is written as. A stored age is canonical GEDCOM
/// (`34y`, `< 1y 6m`, `CHILD`), so it has no phrase to carry; one that is not
/// is left out.
fn to_ged_age(age: Option<&str>) -> Option<GedAge> {
    let age = age?.parse::<oxidgene_core::types::AgeAtEvent>().ok()?;
    Some(GedAge::new(age.to_string()))
}

/// A note holding `text`. Its line breaks, of any kind, are `CONT` lines.
fn to_ged_note(text: &str) -> GedNote {
    GedNote::text(text)
}

fn to_ged_detail(evt: &Event, index: &ExportIndex, warnings: &mut Vec<String>) -> GedEvent {
    let mut kind = convert_event_type(evt.event_type);
    // A tag of its own says what the event is, and its `TYPE` refines it
    // with the description ("College" under a `GRAD`); a generic `EVEN`
    // needs its `TYPE` to say what it is. An event whose tag GEDCOM does not
    // permit on its record — a family's property (`PROP`), a person's
    // marriage — is the generic event its tag names (`2 TYPE PROP`), with
    // its description on the line, which reads back as the same type.
    let (value, type_text) = if kind == EventKind::Event {
        let (value, type_text) = even_value_and_type(evt.event_type, evt.description.as_deref());
        (value, Some(type_text))
    } else if !is_permitted(&kind, evt.family_id.is_some()) {
        let tag = kind.as_str(GedcomVersion::V5_5_1).unwrap_or("EVEN");
        let value = non_blank(evt.description.as_deref()).map(str::to_string);
        kind = EventKind::Event;
        (value, Some(tag.to_string()))
    } else {
        (None, evt.description.clone())
    };

    // Witnesses/godparents are exported separately as a level-1 `ASSO` on
    // the relevant INDI record (see `associations`), not nested here —
    // GEDCOM 5.5.1 only allows `ASSO` directly under an INDIVIDUAL_RECORD,
    // and readers (Gramps included) reject it as a substructure of an event.
    //
    let mut event = GedEvent {
        kind,
        value: value.map(Text::from).unwrap_or_default(),
        ..index.event_parts(evt, warnings)
    };
    let classification = type_text.map(Text::from);
    let cause = evt.cause.as_deref().map(Text::from);
    // A family event's ages are its spouses' own.
    let age = to_ged_age(evt.age.as_deref()).filter(|_| evt.family_id.is_none());
    let agency = evt.agency.as_deref().map(Text::from);
    // The family an individual event names without belonging to it (an
    // adoption's adoptive family) is its `FAMC`. Beneath an event GEDCOM
    // gives none (a child sealing written as an `EVEN`), oxidged writes it
    // as `_FAMC` and reads it back.
    let related_family = index.related_family_xref(evt).map(|family| EventFamily {
        family: Some(family),
        ..EventFamily::default()
    });
    if classification.is_some()
        || cause.is_some()
        || age.is_some()
        || agency.is_some()
        || related_family.is_some()
    {
        let detail = event.detail_mut();
        detail.classification = classification;
        detail.cause = cause;
        detail.age = age;
        detail.agency = agency;
        detail.family = related_family;
    }
    event
}

/// The ordinance `evt` is written as, when it is one: a `BAPL`, `CONL`,
/// `ENDL`, `INIL` or `SLGC` of a person, or an `SLGS` of a family, holding
/// nothing an ordinance has no room for. `has_media` says whether it links
/// media.
fn ordinance_written_as(evt: &Event, has_media: bool) -> Option<OrdinanceKind> {
    let kind = ordinance_kind(evt.event_type)?;
    if evt.family_id.is_some() != (kind == OrdinanceKind::SpouseSealing) {
        return None;
    }
    if non_blank(evt.description.as_deref()).is_some()
        || evt.age.is_some()
        || evt.agency.is_some()
        || evt.cause.is_some()
        || evt.classification.is_some()
        || !evt.spouse_ages.is_empty()
        || has_media
    {
        return None;
    }
    Some(kind)
}

/// Whether GEDCOM puts an event or an attribute of this kind on a family's
/// record (`family`) or on a person's.
fn is_permitted(kind: &EventKind, family: bool) -> bool {
    let on_family = matches!(
        kind,
        EventKind::Annulment
            | EventKind::Census
            | EventKind::Divorce
            | EventKind::DivorceFiled
            | EventKind::Engagement
            | EventKind::MarriageBann
            | EventKind::MarriageContract
            | EventKind::MarriageLicense
            | EventKind::Marriage
            | EventKind::MarriageSettlement
            | EventKind::Event
            | EventKind::Residence
            | EventKind::ChildrenCount
            | EventKind::Fact
    );
    let on_family_only = on_family
        && !matches!(
            kind,
            EventKind::Census
                | EventKind::Event
                | EventKind::Residence
                | EventKind::ChildrenCount
                | EventKind::Fact
        );
    if family { on_family } else { !on_family_only }
}

/// Maps the `EventType` variants that represent a GEDCOM
/// `INDIVIDUAL_ATTRIBUTE_STRUCTURE` (OCCU, RESI, TITL, ...) to their
/// attribute tag, so they round-trip to their original tag instead of a
/// generic `EVEN`. `None` for event-shaped types, which are exported via
/// `to_ged_detail` instead.
fn event_type_to_attribute(et: EventType) -> Option<EventKind> {
    match et {
        EventType::Occupation => Some(EventKind::Occupation),
        EventType::CasteName => Some(EventKind::Caste),
        EventType::PhysicalDescription => Some(EventKind::PhysicalDescription),
        EventType::Education => Some(EventKind::Education),
        EventType::NationalId => Some(EventKind::IdNumber),
        EventType::NationalOrigin => Some(EventKind::Nationality),
        EventType::ChildrenCount => Some(EventKind::ChildrenCount),
        EventType::MarriagesCount => Some(EventKind::MarriageCount),
        EventType::Property => Some(EventKind::Property),
        EventType::Religion => Some(EventKind::Religion),
        EventType::SocialSecurityNumber => Some(EventKind::SocialSecurityNumber),
        EventType::NobilityTitle => Some(EventKind::Title),
        EventType::Fact => Some(EventKind::Fact),
        _ => None,
    }
}

/// Exports an individual attribute (e.g. `EventType::Occupation`, GEDCOM
/// `OCCU`) under its own tag instead of a generic `EVEN`: the description is
/// its line value.
fn to_ged_attribute_detail(
    evt: &Event,
    attribute: EventKind,
    index: &ExportIndex,
    warnings: &mut Vec<String>,
) -> GedEvent {
    // An attribute documents itself as readily as an event does: a scan of the
    // trade card behind an OCCU, of the deed behind a TITL. The same parts,
    // media links included, read the same way as `to_ged_detail` reads them.
    let description = evt.description.as_deref();
    // A count (`NCHI`, `NMR`) is a number in GEDCOM 5.5.1: one described in
    // words, or not at all, is the generic event its tag names
    // (`1 EVEN About a dozen` / `2 TYPE NCHI`), which reads back the same.
    let counted = matches!(
        attribute,
        EventKind::ChildrenCount | EventKind::MarriageCount
    );
    let as_generic = counted && !description.is_some_and(|d| d.trim().parse::<u32>().is_ok());
    let mut event = GedEvent {
        kind: if as_generic {
            EventKind::Event
        } else {
            attribute.clone()
        },
        // The attribute's own line value (e.g. "Account Manager" for OCCU) —
        // mirrors the import side, which reads this same field back first
        // (falling back to the TYPE sub-tag).
        value: non_blank(description).map(Text::from).unwrap_or_default(),
        ..index.event_parts(evt, warnings)
    };
    if as_generic {
        let tag = attribute.as_str(GedcomVersion::V5_5_1).unwrap_or("EVEN");
        event.detail_mut().classification = Some(Text::from(tag));
    }
    let cause = evt.cause.as_deref().map(Text::from);
    let age = to_ged_age(evt.age.as_deref());
    let agency = evt.agency.as_deref().map(Text::from);
    // The `TYPE` beside the value, kept as it was read. A generic event's
    // `TYPE` is its tag, which is what reads it back, so it has no room for
    // one.
    let classification = if as_generic {
        None
    } else {
        non_blank(evt.classification.as_deref()).map(Text::from)
    };
    if classification.is_some() || cause.is_some() || age.is_some() || agency.is_some() {
        let detail = event.detail_mut();
        if classification.is_some() {
            detail.classification = classification;
        }
        detail.cause = cause;
        detail.age = age;
        detail.agency = agency;
    }
    event
}

fn to_ged_citation(
    cite: &Citation,
    source_xref: &HashMap<Uuid, XrefId>,
    tag: TagId,
    warnings: &mut Vec<String>,
) -> Option<GedCitation> {
    let Some(&xref) = source_xref.get(&cite.source_id) else {
        warnings.push(format!(
            "Citation {} references unknown source {}",
            cite.id, cite.source_id
        ));
        return None;
    };

    let mut citation = GedCitation {
        source: CitationSource::Pointer(xref),
        page: cite.page.as_deref().map(Text::from),
        extra: tag_nodes(tag, &cite.tags).into(),
        ..Default::default()
    };
    // The transcript of what the source says, GEDCOM's `DATA.TEXT`.
    let data = non_blank(cite.text.as_deref()).map(|text| CitationData {
        texts: vec![SourceText {
            text: Text::from(text),
            ..SourceText::default()
        }],
        ..CitationData::default()
    });
    let quality = convert_confidence(cite.confidence);
    if data.is_some() || quality.is_some() {
        let detail = citation.detail_mut();
        detail.data = data;
        detail.quality = quality;
    }
    Some(citation)
}

/// How many pages a document holds in this export.
fn pages_of_len(pages_of: &HashMap<Uuid, Vec<&Media>>, document_id: Uuid) -> usize {
    pages_of.get(&document_id).map_or(0, Vec::len)
}

/// The `TITL` written on one exported page.
///
/// The document's title, and for a document of several pages the page number
/// alongside it. Other software lists these records side by side, where one
/// title repeated thirty-eight times says nothing about which scan is which.
/// A single-page document — an ordinary photograph — keeps its bare title.
fn page_title(document: &Media, page_count: usize, page_index: i32) -> Option<String> {
    let title = document.title.clone()?;
    if page_count <= 1 {
        return Some(title);
    }
    Some(format!(
        "{title} \u{2014} page {}/{page_count}",
        page_index + 1
    ))
}

/// The `OBJE` pointers one media link becomes.
///
/// Usually one. A link to a multi-page document becomes one per page, in
/// reading order: the document itself is not exported — GEDCOM has no
/// container — so linking to it would point at a record that is not there, and
/// linking only to its cover would leave the other pages attached to nobody.
/// Somebody whose naturalisation dossier runs to thirty-eight scans keeps all
/// thirty-eight.
fn to_ged_multimedia_refs(
    media_id: Uuid,
    media_by_id: &HashMap<Uuid, &Media>,
    media_xref: &HashMap<Uuid, XrefId>,
    pages_of: &HashMap<Uuid, Vec<&Media>>,
) -> Vec<MultimediaLink> {
    let Some(media) = media_by_id.get(&media_id) else {
        return Vec::new();
    };
    let targets: Vec<Uuid> = if media.is_document() {
        pages_of
            .get(&media_id)
            .map(|pages| pages.iter().map(|page| page.id).collect())
            .unwrap_or_default()
    } else {
        vec![media_id]
    };
    targets
        .into_iter()
        .filter_map(|id| {
            Some(MultimediaLink {
                object: Some(*media_xref.get(&id)?),
                ..Default::default()
            })
        })
        .collect()
}

/// The GEDCOM medium a physical medium is: none for `Other`, which GEDCOM
/// 5.5.1 does not have (and which an absent medium reads back as).
fn to_ged_medium(medium: SourceMediaType) -> Option<Medium> {
    match medium {
        SourceMediaType::Other => None,
        known => Some(Medium::parse(known.gedcom_value())),
    }
}

/// Format a float coordinate as a GEDCOM coordinate string.
///
/// Latitude: positive → `N`, negative → `S`
/// Longitude: positive → `E`, negative → `W`
fn format_coord(value: f64, is_latitude: bool) -> String {
    let (prefix, abs) = if is_latitude {
        if value >= 0.0 {
            ("N", value)
        } else {
            ("S", -value)
        }
    } else if value >= 0.0 {
        ("E", value)
    } else {
        ("W", -value)
    };
    format!("{prefix}{abs}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxidgene_core::enums::{Calendar, DateQualifier, DocumentCategory, Privacy};
    use oxidgene_core::types::DOCUMENT_MIME;

    /// `QUAY` has no fifth value: `VeryHigh` is written as `3`, like `High`,
    /// and a citation nobody assessed gets no `QUAY`.
    #[test]
    fn confidence_maps_onto_the_four_quay_values() {
        assert_eq!(
            convert_confidence(Some(Confidence::VeryHigh)),
            Some(Certainty::Primary)
        );
        assert_eq!(
            convert_confidence(Some(Confidence::Medium)),
            Some(Certainty::Secondary)
        );
        assert_eq!(convert_confidence(None), None);
    }

    fn person_row() -> Person {
        Person {
            id: Uuid::now_v7(),
            tree_id: Uuid::now_v7(),
            sex: Sex::Unknown,
            privacy: Default::default(),
            portrait_media_id: None,
            portrait_vignette_id: None,
            tags: Vec::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            deleted_at: None,
        }
    }

    /// A document and the single page holding its bytes — the shape every
    /// media has now. Returns (document, page); link to the document.
    fn document_with_page(file_name: &str, mime_type: &str, stored: bool) -> (Media, Media) {
        let mut document = medium(file_name, DOCUMENT_MIME, false);
        document.title = Some(file_name.to_string());
        document.file_path = String::new();
        document.page_count = 1;
        let mut page = medium(file_name, mime_type, stored);
        page.parent_media_id = Some(document.id);
        page.page_index = 0;
        page.tree_id = document.tree_id;
        (document, page)
    }

    fn medium(file_name: &str, mime_type: &str, stored: bool) -> Media {
        Media {
            id: Uuid::now_v7(),
            tree_id: Uuid::now_v7(),
            file_name: file_name.to_string(),
            mime_type: mime_type.to_string(),
            file_path: "C:\\Photos\\original.jpg".to_string(),
            storage_key: stored.then(|| "ab/cdef".to_string()),
            sha256: None,
            thumbnail_key: None,
            thumbnail_url: None,
            width: None,
            height: None,
            page_count: 1,
            parent_media_id: None,
            page_index: 0,
            title: None,
            description: None,
            file_size: 0,
            date_value: None,
            date_sort: None,
            date_qualifier: Default::default(),
            date_value2: None,
            calendar: Default::default(),
            privacy: Default::default(),
            source_media_type: Default::default(),
            document_category: None,
            tags: vec![],
            place_id: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            deleted_at: None,
        }
    }

    #[test]
    fn a_family_citation_survives_an_export_and_a_re_import() {
        let tree_id = Uuid::now_v7();
        let now = chrono::Utc::now();
        let family = Family {
            id: Uuid::now_v7(),
            tree_id,
            privacy: Privacy::Public,
            tags: Vec::new(),
            created_at: now,
            updated_at: now,
            deleted_at: None,
        };
        let source = Source {
            id: Uuid::now_v7(),
            tree_id,
            title: "Municipal register".to_string(),
            author: None,
            publisher: None,
            abbreviation: None,
            agency: None,
            tags: Vec::new(),
            created_at: now,
            updated_at: now,
            deleted_at: None,
        };
        let citation = Citation {
            id: Uuid::now_v7(),
            source_id: source.id,
            person_id: None,
            event_id: None,
            family_id: Some(family.id),
            page: Some("folio 12".to_string()),
            confidence: Some(Confidence::High),
            text: None,
            tags: Vec::new(),
            created_at: now,
            updated_at: now,
        };

        let export = export_gedcom(
            &ExportRecords {
                gedcom_kept: &[],
                families: std::slice::from_ref(&family),
                sources: std::slice::from_ref(&source),
                citations: std::slice::from_ref(&citation),
                ..Default::default()
            },
            &ExportOptions {
                choices: ExportChoices::default(),
                target: ExportTarget::Gedcom,
                self_person_id: None,
                submitter: Default::default(),
            },
        )
        .expect("exports");

        let imported = crate::import::import_gedcom(&export.gedcom, Uuid::now_v7())
            .expect("imports exported GEDCOM");
        assert_eq!(imported.citations.len(), 1);
        assert!(imported.citations[0].family_id.is_some());
        assert_eq!(imported.citations[0].page.as_deref(), Some("folio 12"));
    }

    #[test]
    fn a_medium_we_hold_is_filed_under_its_id_so_two_photo_jpgs_can_coexist() {
        let first = medium("photo.jpg", "image/jpeg", true);
        let second = medium("photo.jpg", "image/jpeg", true);
        let (Some(a), Some(b)) = (archive_path(&first), archive_path(&second)) else {
            panic!("both are stored")
        };
        assert_ne!(a, b, "one name for both would lose a file");
        assert_eq!(a, format!("media/{}.jpg", first.id));
    }

    #[test]
    fn a_medium_with_no_bytes_has_no_place_in_the_archive() {
        // A GEDCOM import that named a file nobody uploaded. There is nothing
        // to pack, so its `FILE` keeps whatever the producer wrote.
        assert_eq!(
            archive_path(&medium("photo.jpg", "image/jpeg", false)),
            None
        );
    }

    #[test]
    fn an_extension_is_recovered_from_the_type_when_the_name_carries_none() {
        let m = medium("scan", "image/png", true);
        assert_eq!(archive_path(&m), Some(format!("media/{}.png", m.id)));
    }

    #[test]
    fn a_full_stop_in_a_title_is_not_mistaken_for_an_extension() {
        let m = medium("Acte n. 12 du registre", "image/jpeg", true);
        // "12 du registre" is not an extension; the MIME type decides.
        assert_eq!(archive_path(&m), Some(format!("media/{}.jpg", m.id)));
    }

    #[test]
    fn an_archive_carries_the_media_and_the_gedcom_names_them() {
        let (document, m) = document_with_page("photo.jpg", "image/jpeg", true);
        let path = archive_path(&m).expect("stored");
        let mut paths = HashMap::new();
        paths.insert(m.id, path.clone());
        let rows = [document, m];

        let export = export_gedcom(
            &ExportRecords {
                gedcom_kept: &[],
                media: &rows,
                ..Default::default()
            },
            &ExportOptions {
                choices: ExportChoices::default(),
                target: ExportTarget::Gedzip {
                    media_paths: &paths,
                },
                self_person_id: None,
                submitter: Default::default(),
            },
        )
        .expect("exports");
        // The FILE line points into the archive, not at the Windows path the
        // record was imported with.
        assert!(
            export.gedcom.contains(&path),
            "the GEDCOM must name the entry it ships: {}",
            export.gedcom
        );
        assert!(!export.gedcom.contains("C:\\Photos"));

        let bytes =
            export_gedzip(&export.gedcom, &[(path.clone(), b"JPEGBYTES".to_vec())]).expect("zips");
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("reads back");
        assert_eq!(
            archive
                .by_name(oxidged::gedzip::GEDCOM_FILENAME)
                .expect("GEDCOM entry")
                .compression(),
            zip::CompressionMethod::Deflated
        );
        // The bug this pins: the archive used to hold gedcom.ged and nothing
        // else, so every photograph was silently dropped on export.
        let mut entry = archive.by_name(&path).expect("the photo travelled");
        assert_eq!(entry.compression(), zip::CompressionMethod::Stored);
        let mut held = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut held).expect("reads");
        assert_eq!(held, b"JPEGBYTES");
    }

    #[test]
    fn a_bitmap_is_compressed_inside_a_gedzip() {
        let bytes = export_gedzip(
            "0 HEAD\n0 TRLR\n",
            &[("media/scan.bmp".to_string(), vec![0; 1024])],
        )
        .expect("writes GEDZIP");
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("reads back");
        let entry = archive.by_name("media/scan.bmp").expect("bitmap entry");
        assert_eq!(entry.compression(), zip::CompressionMethod::Deflated);
    }

    #[test]
    fn the_physical_medium_survives_an_export_and_a_re_import() {
        let (mut document, page) = document_with_page("headstone.jpg", "image/jpeg", true);
        document.source_media_type = SourceMediaType::Tombstone;
        let m = [document, page];
        let export = export_gedcom(
            &ExportRecords {
                gedcom_kept: &[],
                media: &m,
                ..Default::default()
            },
            &ExportOptions {
                choices: ExportChoices::default(),
                target: ExportTarget::Gedcom,
                self_person_id: None,
                submitter: Default::default(),
            },
        )
        .expect("exports");
        assert!(
            export.gedcom.contains("3 TYPE tombstone"),
            "the medium must reach the file: {}",
            export.gedcom
        );

        let back = crate::import::import_gedcom(&export.gedcom, Uuid::now_v7()).expect("imports");
        assert_eq!(
            back.media
                .iter()
                .find(|m| m.is_document())
                .map(|m| m.source_media_type),
            Some(SourceMediaType::Tombstone),
            "the medium describes the document"
        );
    }

    /// A document described in every way the extensions carry, filed at
    /// `place`, and its one stored page.
    fn described_document_with_page(place: &Place) -> (Media, Media) {
        // Everything descriptive belongs to the document; the page carries
        // the bytes and nothing else.
        let (mut document, mut page) =
            document_with_page("original group photo.jpg", "image/jpeg", true);
        document.created_at = chrono::DateTime::parse_from_rfc3339("2024-01-02T03:04:05Z")
            .expect("valid timestamp")
            .to_utc();
        document.updated_at = chrono::DateTime::parse_from_rfc3339("2024-06-07T08:09:10Z")
            .expect("valid timestamp")
            .to_utc();
        document.file_name = "Family celebration".to_string();
        document.title = Some("Family celebration".to_string());
        document.description = Some("First line\nSecond line".to_string());
        document.date_value = Some("3 SEP 1946".to_string());
        document.date_qualifier = DateQualifier::About;
        document.calendar = Calendar::Gregorian;
        document.privacy = Privacy::Private;
        document.source_media_type = SourceMediaType::Photo;
        document.document_category = Some(DocumentCategory::GroupPhoto);
        document.tags = vec![
            TagRef {
                id: Uuid::now_v7(),
                label: "ceremony".to_string(),
                color: 4,
            },
            TagRef {
                id: Uuid::now_v7(),
                label: "outdoors".to_string(),
                color: 6,
            },
        ];
        document.place_id = Some(place.id);
        page.created_at = document.created_at;
        page.updated_at = document.updated_at;
        (document, page)
    }

    /// The document that came back describes itself as `original` did.
    fn assert_same_description(imported: &Media, original: &Media) {
        assert_eq!(imported.file_name, original.file_name);
        assert_eq!(imported.created_at, original.created_at);
        assert_eq!(imported.updated_at, original.updated_at);
        assert_eq!(imported.title, original.title);
        assert_eq!(imported.description, original.description);
        assert_eq!(imported.date_value, original.date_value);
        assert_eq!(imported.date_qualifier, original.date_qualifier);
        assert_eq!(imported.date_value2, original.date_value2);
        assert_eq!(imported.calendar, original.calendar);
        assert_eq!(imported.privacy, original.privacy);
        assert_eq!(imported.source_media_type, original.source_media_type);
        assert_eq!(imported.document_category, original.document_category);
        let spelled = |tags: &[TagRef]| {
            tags.iter()
                .map(|tag| (tag.label.clone(), tag.color))
                .collect::<Vec<_>>()
        };
        assert_eq!(spelled(&imported.tags), spelled(&original.tags));
    }

    /// The imported document's place and note are the exported ones.
    fn assert_place_and_note_came_back(
        back: &crate::ImportResult,
        imported: &Media,
        place: &Place,
        note: &Note,
    ) {
        let imported_place = back
            .places
            .iter()
            .find(|item| Some(item.id) == imported.place_id)
            .expect("media place");
        assert_eq!(imported_place.name, place.name);
        assert_eq!(imported_place.latitude, place.latitude);
        assert_eq!(imported_place.longitude, place.longitude);

        let imported_note = back
            .notes
            .iter()
            .find(|item| item.media_id == Some(imported.id))
            .expect("media note");
        assert_eq!(imported_note.text, note.text);
        assert_eq!(imported_note.created_at, note.created_at);
        assert_eq!(imported_note.updated_at, note.updated_at);
    }

    #[test]
    fn oxidgene_media_metadata_survives_an_export_and_a_re_import() {
        let place = Place {
            id: Uuid::now_v7(),
            tree_id: Uuid::now_v7(),
            name: "Sample Village".to_string(),
            latitude: Some(48.25),
            longitude: Some(-2.75),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        let (document, page) = described_document_with_page(&place);
        let note = Note {
            id: Uuid::now_v7(),
            tree_id: document.tree_id,
            text: "The left edge is damaged".to_string(),
            person_id: None,
            event_id: None,
            family_id: None,
            source_id: None,
            media_id: Some(document.id),
            repository_id: None,
            shared_group: None,
            created_at: document.created_at,
            updated_at: document.updated_at,
            deleted_at: None,
        };
        let archive_path = archive_path(&page).expect("stored media has an archive path");
        let media_paths = HashMap::from([(page.id, archive_path.clone())]);
        let media = document.clone();
        let rows = [document, page];

        let export = export_gedcom(
            &ExportRecords {
                gedcom_kept: &[],
                places: std::slice::from_ref(&place),
                media: &rows,
                notes: std::slice::from_ref(&note),
                ..Default::default()
            },
            &ExportOptions {
                choices: ExportChoices::default(),
                target: ExportTarget::Gedzip {
                    media_paths: &media_paths,
                },
                self_person_id: None,
                submitter: Default::default(),
            },
        )
        .expect("exports");
        assert!(export.gedcom.contains("1 _OXIDGENE_MEDIA {"));
        assert!(export.gedcom.contains("1 _OXIDGENE_DOC {"));
        assert!(export.gedcom.contains("1 NOTE First line"));
        assert!(export.gedcom.contains("2 CONT Second line"));

        let archive = export_gedzip(&export.gedcom, &[(archive_path, b"IMAGE BYTES".to_vec())])
            .expect("creates GEDZIP");
        let imported_archive =
            crate::import::import_gedzip(&archive, Uuid::now_v7()).expect("imports GEDZIP");
        assert_eq!(imported_archive.files.len(), 1);
        assert_eq!(imported_archive.files[0].1, b"IMAGE BYTES");
        let back = imported_archive.result;
        let imported = back
            .media
            .iter()
            .find(|item| item.is_document())
            .expect("the document came back");
        assert_eq!(back.media.len(), 2, "a document and its one page");
        assert_eq!(imported.page_count, 1);
        assert_same_description(imported, &media);
        assert_place_and_note_came_back(&back, imported, &place, &note);
    }

    #[test]
    fn a_category_gedcom_cannot_express_still_exports_the_medium_it_implies() {
        // A census return is `MANUSCRIPT` to GEDCOM. Writing `OTHER` because
        // the user answered the richer question instead of the poorer one
        // would make our own export worse than the classification we hold.
        let (mut document, page) = document_with_page("recensement.jpg", "image/jpeg", true);
        document.document_category = Some(DocumentCategory::Census);
        let m = [document, page];
        let export = export_gedcom(
            &ExportRecords {
                gedcom_kept: &[],
                media: &m,
                ..Default::default()
            },
            &ExportOptions {
                choices: ExportChoices::default(),
                target: ExportTarget::Gedcom,
                self_person_id: None,
                submitter: Default::default(),
            },
        )
        .expect("exports");
        assert!(
            export.gedcom.contains("3 TYPE manuscript"),
            "{}",
            export.gedcom
        );
    }

    #[test]
    fn an_explicit_medium_is_not_overridden_by_the_category() {
        // The user answered both questions; neither answer is ours to discard.
        let (mut document, page) = document_with_page("microfilm.jpg", "image/jpeg", true);
        document.document_category = Some(DocumentCategory::CivilRecord);
        document.source_media_type = SourceMediaType::Fiche;
        let export = export_gedcom(
            &ExportRecords {
                gedcom_kept: &[],
                media: &[document, page],
                ..Default::default()
            },
            &ExportOptions {
                choices: ExportChoices::default(),
                target: ExportTarget::Gedcom,
                self_person_id: None,
                submitter: Default::default(),
            },
        )
        .expect("exports");
        assert!(export.gedcom.contains("3 TYPE fiche"), "{}", export.gedcom);
        assert!(!export.gedcom.contains("manuscript"));
    }

    #[test]
    fn a_persons_photographs_are_still_theirs_after_a_round_trip() {
        // A record-level `OBJE` pointer must carry the person-media link
        // through the full export and import path.
        let person = person_row();
        let (medium, page) = document_with_page("portrait.jpg", "image/jpeg", true);
        let rows = [medium.clone(), page];
        let link = MediaLink {
            id: Uuid::now_v7(),
            media_id: medium.id,
            person_id: Some(person.id),
            event_id: None,
            source_id: None,
            family_id: None,
            sort_order: 0,
        };

        let export = export_gedcom(
            &ExportRecords {
                gedcom_kept: &[],
                persons: std::slice::from_ref(&person),
                media: &rows,
                media_links: std::slice::from_ref(&link),
                ..Default::default()
            },
            &ExportOptions {
                choices: ExportChoices::default(),
                target: ExportTarget::Gedcom,
                self_person_id: None,
                submitter: Default::default(),
            },
        )
        .expect("exports");
        assert!(export.gedcom.contains("1 OBJE @M1@"), "{}", export.gedcom);

        let back = crate::import::import_gedcom(&export.gedcom, Uuid::now_v7()).expect("imports");
        assert_eq!(
            back.media.len(),
            2,
            "the photograph and the document holding it"
        );
        assert_eq!(
            back.media_links.len(),
            1,
            "and it is still somebody's: {:#?}",
            back.media_links
        );
        assert_eq!(
            back.media_links[0].person_id,
            back.persons.first().map(|p| p.id)
        );
        assert_eq!(back.media_links[0].media_id, back.media[0].id);
    }

    #[test]
    fn a_photo_identification_survives_a_gedzip_round_trip() {
        let person = person_row();
        let (medium, page) = document_with_page("group-photo.jpg", "image/jpeg", true);
        let rows = [medium.clone(), page.clone()];
        // Attached to the document, identified on the page: the link is about
        // the record, the crop is about the pixels.
        let link = MediaLink {
            id: Uuid::now_v7(),
            media_id: medium.id,
            person_id: Some(person.id),
            event_id: None,
            source_id: None,
            family_id: None,
            sort_order: 0,
        };
        let vignette = Vignette {
            id: Uuid::now_v7(),
            media_id: page.id,
            x: 120,
            y: 45,
            width: 64,
            height: 82,
            person_id: Some(person.id),
            event_id: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        let path = archive_path(&page).expect("stored medium has an archive path");
        let mut paths = HashMap::new();
        paths.insert(page.id, path.clone());

        let export = export_gedcom(
            &ExportRecords {
                gedcom_kept: &[],
                persons: std::slice::from_ref(&person),
                media: &rows,
                media_links: std::slice::from_ref(&link),
                vignettes: std::slice::from_ref(&vignette),
                ..Default::default()
            },
            &ExportOptions {
                choices: ExportChoices::default(),
                target: ExportTarget::Gedzip {
                    media_paths: &paths,
                },
                self_person_id: None,
                submitter: Default::default(),
            },
        )
        .expect("exports");
        // Attributed to the person, the crop is a standard CROP beneath the
        // link to the page.
        assert!(
            export
                .gedcom
                .contains("1 OBJE @M1@\n2 CROP\n3 TOP 45\n3 LEFT 120\n3 HEIGHT 82\n3 WIDTH 64\n"),
            "{}",
            export.gedcom
        );

        let archive =
            export_gedzip(&export.gedcom, &[(path, b"JPEGBYTES".to_vec())]).expect("writes GEDZIP");
        let imported = crate::import::import_gedzip(&archive, Uuid::now_v7()).expect("imports");
        let back = imported.result;

        assert_eq!(back.media_links.len(), 1, "whole-image attachment");
        assert_eq!(back.vignettes.len(), 1, "cropped identification");
        let restored = &back.vignettes[0];
        assert_eq!(
            restored.media_id,
            back.media
                .iter()
                .find(|m| !m.is_document())
                .expect("the page came back")
                .id,
            "a crop is on the page, not on the document"
        );
        assert_eq!(restored.person_id, Some(back.persons[0].id));
        assert_eq!(
            (restored.x, restored.y, restored.width, restored.height),
            (120, 45, 64, 82)
        );
    }

    #[test]
    fn an_events_media_link_survives_a_round_trip() {
        let person = person_row();
        let event = Event {
            classification: None,
            id: Uuid::now_v7(),
            tree_id: person.tree_id,
            event_type: EventType::Birth,
            date_value: None,
            date_sort: None,
            date_qualifier: Default::default(),
            date_value2: None,
            calendar: Default::default(),
            cause: None,
            age: None,
            agency: None,
            spouse_ages: Vec::new(),
            place_id: None,
            person_id: Some(person.id),
            family_id: None,
            related_family_id: None,
            description: None,
            tags: Vec::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            deleted_at: None,
        };
        let (medium, page) = document_with_page("birth-record.jpg", "image/jpeg", true);
        let rows = [medium.clone(), page];
        let link = MediaLink {
            id: Uuid::now_v7(),
            media_id: medium.id,
            person_id: None,
            event_id: Some(event.id),
            source_id: None,
            family_id: None,
            sort_order: 0,
        };

        let export = export_gedcom(
            &ExportRecords {
                gedcom_kept: &[],
                persons: std::slice::from_ref(&person),
                events: std::slice::from_ref(&event),
                media: &rows,
                media_links: std::slice::from_ref(&link),
                ..Default::default()
            },
            &ExportOptions {
                choices: ExportChoices::default(),
                target: ExportTarget::Gedcom,
                self_person_id: None,
                submitter: Default::default(),
            },
        )
        .expect("exports");
        assert!(export.gedcom.contains("2 OBJE @M1@"), "{}", export.gedcom);

        let back = crate::import::import_gedcom(&export.gedcom, Uuid::now_v7()).expect("imports");
        assert_eq!(back.events.len(), 1);
        assert_eq!(back.media_links.len(), 1);
        assert_eq!(back.media_links[0].event_id, Some(back.events[0].id));
        let document = back
            .media
            .iter()
            .find(|m| m.is_document())
            .expect("the document came back");
        assert_eq!(back.media_links[0].media_id, document.id);
    }

    #[test]
    fn an_attributes_media_link_survives_a_round_trip_once_per_profession() {
        // An attribute documents itself as readily as an event: the trade card
        // behind an OCCU. The value splits into one event per profession on
        // import, and the scan must document each of them.
        let person = person_row();
        let occupation = Event {
            classification: None,
            id: Uuid::now_v7(),
            tree_id: person.tree_id,
            event_type: EventType::Occupation,
            date_value: None,
            date_sort: None,
            date_qualifier: Default::default(),
            date_value2: None,
            calendar: Default::default(),
            cause: None,
            age: None,
            agency: None,
            spouse_ages: Vec::new(),
            place_id: None,
            person_id: Some(person.id),
            family_id: None,
            related_family_id: None,
            description: Some("Baker, Miller".to_string()),
            tags: Vec::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            deleted_at: None,
        };
        let (medium, page) = document_with_page("trade-card.jpg", "image/jpeg", true);
        let rows = [medium.clone(), page];
        let link = MediaLink {
            id: Uuid::now_v7(),
            media_id: medium.id,
            person_id: None,
            event_id: Some(occupation.id),
            source_id: None,
            family_id: None,
            sort_order: 0,
        };

        let export = export_gedcom(
            &ExportRecords {
                gedcom_kept: &[],
                persons: std::slice::from_ref(&person),
                events: std::slice::from_ref(&occupation),
                media: &rows,
                media_links: std::slice::from_ref(&link),
                ..Default::default()
            },
            &ExportOptions {
                choices: ExportChoices::default(),
                target: ExportTarget::Gedcom,
                self_person_id: None,
                submitter: Default::default(),
            },
        )
        .expect("exports");
        assert!(export.gedcom.contains("1 OCCU"), "{}", export.gedcom);
        assert_eq!(
            export.gedcom.matches("2 OBJE @M1@").count(),
            1,
            "{}",
            export.gedcom
        );

        let back = crate::import::import_gedcom(&export.gedcom, Uuid::now_v7()).expect("imports");
        assert_eq!(back.events.len(), 2, "one event per profession");
        assert_eq!(
            back.media_links.len(),
            2,
            "one line documented both professions"
        );
        let document = back
            .media
            .iter()
            .find(|m| m.is_document())
            .expect("the document came back");
        assert!(
            back.media_links
                .iter()
                .all(|link| link.media_id == document.id)
        );
    }

    #[test]
    fn page_transcripts_remain_attached_to_their_pages_after_a_round_trip() {
        let mut document = medium("Sample register", "image/jpeg", false);
        document.page_count = 2;

        let mut first_page = medium("page-1.jpg", "image/jpeg", true);
        first_page.parent_media_id = Some(document.id);
        first_page.page_index = 0;
        let mut second_page = medium("page-2.jpg", "image/jpeg", true);
        second_page.parent_media_id = Some(document.id);
        second_page.page_index = 1;
        let notes = [
            Note {
                id: Uuid::now_v7(),
                tree_id: first_page.tree_id,
                text: "First page transcript".to_string(),
                person_id: None,
                event_id: None,
                family_id: None,
                source_id: None,
                media_id: Some(first_page.id),
                repository_id: None,
                shared_group: None,
                created_at: first_page.created_at,
                updated_at: first_page.updated_at,
                deleted_at: None,
            },
            Note {
                id: Uuid::now_v7(),
                tree_id: second_page.tree_id,
                text: "Second page transcript".to_string(),
                person_id: None,
                event_id: None,
                family_id: None,
                source_id: None,
                media_id: Some(second_page.id),
                repository_id: None,
                shared_group: None,
                created_at: second_page.created_at,
                updated_at: second_page.updated_at,
                deleted_at: None,
            },
        ];
        let rows = [document, second_page, first_page];

        let export = export_gedcom(
            &ExportRecords {
                gedcom_kept: &[],
                media: &rows,
                notes: &notes,
                ..Default::default()
            },
            &ExportOptions {
                choices: ExportChoices::default(),
                target: ExportTarget::Gedcom,
                self_person_id: None,
                submitter: Default::default(),
            },
        )
        .expect("exports");

        let back = crate::import::import_gedcom(&export.gedcom, Uuid::now_v7()).expect("imports");
        for (file_name, expected) in [
            ("page-1.jpg", "First page transcript"),
            ("page-2.jpg", "Second page transcript"),
        ] {
            let media = back
                .media
                .iter()
                .find(|media| media.file_name == file_name)
                .expect("page is imported");
            let note = back
                .notes
                .iter()
                .find(|note| note.media_id == Some(media.id))
                .expect("page transcript is imported");
            assert_eq!(note.text, expected);
        }
    }

    #[test]
    fn the_portrait_still_represents_the_person_after_a_round_trip() {
        // GEDCOM has no primary-photo flag, so the choice is carried by
        // *order*: our import takes a person's first picture when no portrait
        // is stored. Without writing the portrait first, somebody with several
        // photographs kept all of them and came back represented by whichever
        // one happened to be written first.
        // Distinct paths, or the two are indistinguishable after a round trip:
        // the shared fixture gives every medium the same one.
        let (chosen, mut chosen_page) = document_with_page("chosen.jpg", "image/jpeg", true);
        chosen_page.file_path = "chosen.jpg".to_string();
        let (other, mut other_page) = document_with_page("other.jpg", "image/jpeg", true);
        other_page.file_path = "other.jpg".to_string();

        let mut person = person_row();
        person.portrait_media_id = Some(chosen.id);

        // `other` is attached first and sorts first, so only the portrait
        // itself can put `chosen` at the head of the list.
        let links: Vec<MediaLink> = [(&other, 0), (&chosen, 1)]
            .into_iter()
            .map(|(m, order)| MediaLink {
                id: Uuid::now_v7(),
                media_id: m.id,
                person_id: Some(person.id),
                event_id: None,
                source_id: None,
                family_id: None,
                sort_order: order,
            })
            .collect();

        let export = export_gedcom(
            &ExportRecords {
                gedcom_kept: &[],
                persons: std::slice::from_ref(&person),
                media: &[
                    other.clone(),
                    other_page.clone(),
                    chosen.clone(),
                    chosen_page.clone(),
                ],
                media_links: &links,
                ..Default::default()
            },
            &ExportOptions {
                choices: ExportChoices::default(),
                target: ExportTarget::Gedcom,
                self_person_id: None,
                submitter: Default::default(),
            },
        )
        .expect("exports");

        let back = crate::import::import_gedcom(&export.gedcom, Uuid::now_v7()).expect("imports");
        let first = back
            .media_links
            .iter()
            .filter(|l| l.person_id.is_some())
            .min_by_key(|l| l.sort_order)
            .expect("she has pictures");
        // The link names the document; the file name it was written from is
        // its page's.
        let name = back
            .media
            .iter()
            .filter(|m| m.parent_media_id == Some(first.media_id))
            .map(|m| m.file_name.as_str())
            .next();
        assert_eq!(name, Some("chosen.jpg"), "{:#?}", back.media_links);
        assert_eq!(back.media_links.len(), 2, "and she kept the other one");

        // And it is *recorded* as the portrait, not merely drawn as one: the
        // gallery marks the stored choice, so an implied portrait came back
        // without its star and with nothing to un-choose.
        let imported = back.persons.first().expect("she is there");
        assert_eq!(imported.portrait_media_id, Some(first.media_id));
    }

    #[test]
    fn a_link_to_a_document_becomes_a_link_to_every_page() {
        // Sala's naturalisation dossier: thirty-eight scans, and she was
        // linked to the document rather than to any page. Exporting the
        // document as its cover gave her page one and left the other
        // thirty-seven in the tree attached to nobody.
        let person = person_row();
        let mut document = medium("Dossier de naturalisation", DOCUMENT_MIME, false);
        document.title = Some("Dossier de naturalisation".to_string());
        document.file_path = String::new();
        document.page_count = 3;

        let mut pages = Vec::new();
        for index in 0..3 {
            let mut page = medium(&format!("page{index}.jpg"), "image/jpeg", true);
            // Distinct paths, or the order this asserts is unobservable: the
            // shared fixture gives every medium the same one.
            page.file_path = format!("page{index}.jpg");
            page.parent_media_id = Some(document.id);
            page.page_index = index;
            pages.push(page);
        }

        let link = MediaLink {
            id: Uuid::now_v7(),
            media_id: document.id,
            person_id: Some(person.id),
            event_id: None,
            source_id: None,
            family_id: None,
            sort_order: 0,
        };

        // Deliberately out of order, and the document last: neither the row
        // order nor the insertion order decides the reading order.
        let mut rows = vec![pages[2].clone(), pages[0].clone(), pages[1].clone()];
        rows.push(document.clone());

        let export = export_gedcom(
            &ExportRecords {
                gedcom_kept: &[],
                persons: std::slice::from_ref(&person),
                media: &rows,
                media_links: std::slice::from_ref(&link),
                ..Default::default()
            },
            &ExportOptions {
                choices: ExportChoices::default(),
                target: ExportTarget::Gedcom,
                self_person_id: None,
                submitter: Default::default(),
            },
        )
        .expect("exports");

        // Every page is written as an `OBJE` and she is pointed at all three,
        // so software that knows nothing of our extensions attaches the whole
        // dossier rather than its cover.
        assert_eq!(
            export.gedcom.matches("1 OBJE @").count(),
            3,
            "{}",
            export.gedcom
        );

        let back = crate::import::import_gedcom(&export.gedcom, Uuid::now_v7()).expect("imports");
        // And on the way back the container is restored: one document holding
        // three pages, and one link — to the document, which is what she was
        // attached to before the export dissolved it.
        let documents: Vec<_> = back.media.iter().filter(|m| m.is_document()).collect();
        assert_eq!(documents.len(), 1, "{:#?}", back.media);
        assert_eq!(documents[0].page_count, 3);
        assert_eq!(back.media.len(), 4, "the document and its three pages");
        assert_eq!(back.media_links.len(), 1, "{:#?}", back.media_links);
        assert_eq!(back.media_links[0].media_id, documents[0].id);
        assert_eq!(back.media_links[0].person_id, Some(back.persons[0].id));

        // The pages are in reading order — `page_index`, not the order the
        // rows happened to arrive in.
        let mut restored: Vec<_> = back.media.iter().filter(|m| !m.is_document()).collect();
        restored.sort_by_key(|page| page.page_index);
        let ordered: Vec<&str> = restored.iter().map(|m| m.file_name.as_str()).collect();
        assert_eq!(ordered, vec!["page0.jpg", "page1.jpg", "page2.jpg"]);
    }

    #[test]
    fn a_document_is_never_written_as_a_file_of_its_own() {
        // A document holds no bytes, so writing it as an `OBJE` with a `FILE`
        // made a GEDZIP warn once per document about an archive entry that
        // could not have existed. Only its pages are records.
        let (document, cover) = document_with_page("page1.jpg", "image/jpeg", true);

        let export = export_gedcom(
            &ExportRecords {
                gedcom_kept: &[],
                media: &[document, cover],
                ..Default::default()
            },
            &ExportOptions {
                choices: ExportChoices::default(),
                target: ExportTarget::Gedcom,
                self_person_id: None,
                submitter: Default::default(),
            },
        )
        .expect("exports");
        assert_eq!(
            export.gedcom.matches(" OBJE").count(),
            1,
            "only the page is a record: {}",
            export.gedcom
        );
        assert_eq!(
            export.gedcom.matches("1 FILE ").count(),
            1,
            "and only it names a file: {}",
            export.gedcom
        );
    }

    #[test]
    fn a_plain_gedcom_export_still_references_the_producers_own_path() {
        let (document, page) = document_with_page("photo.jpg", "image/jpeg", true);
        let export = export_gedcom(
            &ExportRecords {
                gedcom_kept: &[],
                media: &[document, page],
                ..Default::default()
            },
            &ExportOptions {
                choices: ExportChoices::default(),
                // No archive: nothing to point into.
                target: ExportTarget::Gedcom,
                self_person_id: None,
                submitter: Default::default(),
            },
        )
        .expect("exports");
        assert!(export.gedcom.contains("C:\\Photos\\original.jpg"));
    }
}
