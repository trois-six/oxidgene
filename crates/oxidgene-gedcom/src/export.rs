//! OxidGene domain model → GEDCOM export.
//!
//! Converts domain model entities into a GEDCOM 5.5.1 string using `ged_io`.

use std::collections::HashMap;

use ged_io::GedcomWriter;
use ged_io::types::GedcomData;
use ged_io::types::age::{Age as GedAge, AgeModifier as GedAgeModifier};
use ged_io::types::date::Date;
use ged_io::types::event::Event as GedEvent;
use ged_io::types::event::detail::Detail as GedDetail;
use ged_io::types::family::Family as GedFamily;
use ged_io::types::header::Header;
use ged_io::types::header::encoding::Encoding;
use ged_io::types::header::meta::HeadMeta;
use ged_io::types::header::source::HeadSour;
use ged_io::types::individual::Individual;
use ged_io::types::individual::association::Association as GedAssociation;
use ged_io::types::individual::attribute::IndividualAttribute as GedIndividualAttribute;
use ged_io::types::individual::attribute::detail::AttributeDetail as GedAttributeDetail;
use ged_io::types::individual::family_link::pedigree::Pedigree as GedPedigree;
use ged_io::types::individual::family_link::{FamilyLink, FamilyLinkType};
use ged_io::types::individual::gender::{Gender, GenderType};
use ged_io::types::individual::name::{Name as GedName, NameType as GedNameType};
use ged_io::types::multimedia::Multimedia as GedMultimedia;
use ged_io::types::multimedia::file::Reference;
use ged_io::types::multimedia::format::Format;
use ged_io::types::note::Note as GedNote;
use ged_io::types::place::{MapCoordinates, Place as GedPlace};
use ged_io::types::source::Source as GedSource;
use ged_io::types::source::citation::Citation as GedCitation;
use ged_io::types::source::citation::CitationSource;
use ged_io::types::source::citation::data::SourceCitationData;
use ged_io::types::source::quay::CertaintyAssessment;
use ged_io::types::source::text::Text as GedText;
use ged_io::types::submitter::Submitter;
use uuid::Uuid;

use oxidgene_core::enums::SourceMediaType;
use oxidgene_core::types::{
    Citation, Event, EventWitness, Family, FamilyChild, FamilySpouse, Media, MediaLink, Note,
    Person, PersonName, Place, Source, Vignette,
};
use oxidgene_core::{ChildType, Confidence, EventType, NameType, Privacy, Sex, SpouseRole};

use crate::{
    DocumentExtension, DocumentMetadataExtension, ExportResult, MediaMetadataExtension,
    MediaNoteExtension, MediaPlaceExtension,
};

/// Export domain model entities to a GEDCOM 5.5.1 string.
///
/// All entity slices should belong to the same tree.
///
/// `merge_occupations` collapses every `EventType::Occupation` event for a
/// person back into a single `OCCU` tag (values joined with `", "`) instead
/// of one `OCCU` tag per event. Some importers — Geneanet in particular —
/// only support a single profession field per individual, so this is an
/// opt-in, lossy compatibility option; leave it `false` to keep the
/// lossless one-`OCCU`-per-profession export.
///
/// `merge_names` collapses every non-primary `PersonName` for a person into
/// the primary name's `SURN` tag (surnames joined with `,`) instead of one
/// `NAME`/`SURN` structure per name. Geneanet's own exporter only emits one
/// `NAME` per individual and packs every other surname it knows into that
/// `SURN` sub-tag, so this is an opt-in, lossy compatibility option; leave
/// it `false` to keep the lossless one-`NAME`-per-`PersonName` export.
///
/// `self_person_id` is the tree's "Who am I?" person: the `SUBM` record GEDCOM
/// 5.5.1 requires is named after them, or `Not Provided` without one.
///
/// # Errors
///
/// Returns `Err` if the GEDCOM writer encounters an I/O error.
#[allow(clippy::too_many_arguments)]
pub fn export_gedcom(
    persons: &[Person],
    person_names: &[PersonName],
    families: &[Family],
    family_spouses: &[FamilySpouse],
    family_children: &[FamilyChild],
    events: &[Event],
    event_witnesses: &[EventWitness],
    places: &[Place],
    sources: &[Source],
    citations: &[Citation],
    media: &[Media],
    media_links: &[MediaLink],
    vignettes: &[Vignette],
    notes: &[Note],
    merge_occupations: bool,
    merge_names: bool,
    media_paths: &HashMap<Uuid, String>,
    self_person_id: Option<Uuid>,
) -> Result<ExportResult, String> {
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

    let xrefs = Xrefs::new(persons, families, sources, media);
    let assoc_by_person = associations(events, event_witnesses, &xrefs);
    let index = ExportIndex {
        place_map: places.iter().map(|p| (p.id, p)).collect(),
        media_by_id: media.iter().map(|m| (m.id, m)).collect(),
        pages_of: pages_of(media),
        names_by_person: group_by(person_names, |pn| Some(pn.person_id)),
        events_by_person: group_by(events, |evt| evt.person_id),
        events_by_family: group_by(events, |evt| evt.family_id),
        assoc_by_person,
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
        xrefs,
    };

    let mut data = GedcomData {
        header: Some(gedcom_header()),
        ..Default::default()
    };
    data.submitters = vec![index.submitter(self_person_id)];
    data.sources = sources.iter().map(|src| index.source(src)).collect();
    data.multimedia = media
        .iter()
        // Dissolved into its pages, which carry the bytes; see `pages_of`.
        .filter(|m| !m.is_document())
        .map(|m| index.multimedia(m, media_paths))
        .collect();
    for person in persons {
        let individual = index.individual(person, merge_names, merge_occupations, &mut warnings);
        data.individuals.push(individual);
    }
    for fam in families {
        let family = index.family(fam, &mut warnings);
        data.families.push(family);
    }

    drop(build_guard);

    let gedcom = write_gedcom(&data)?;
    let gedcom = crate::finish::finish(
        &gedcom,
        |owner| index.notes_of(owner),
        &index.additions(persons, families, sources),
    );
    let (gedcom, extension_warnings) = inject_extensions(gedcom, media, vignettes, &index);
    warnings.extend(extension_warnings);

    Ok(ExportResult { gedcom, warnings })
}

/// The GEDCOM cross-reference of every record the export writes, by id.
struct Xrefs {
    person: HashMap<Uuid, String>,
    family: HashMap<Uuid, String>,
    source: HashMap<Uuid, String>,
    media: HashMap<Uuid, String>,
}

impl Xrefs {
    fn new(persons: &[Person], families: &[Family], sources: &[Source], media: &[Media]) -> Self {
        Self {
            person: numbered("I", persons.iter().map(|p| p.id)),
            family: numbered("F", families.iter().map(|f| f.id)),
            source: numbered("S", sources.iter().map(|s| s.id)),
            // Only pages become records: a document holds no bytes and is
            // dissolved into them. Numbering just the pages keeps the xrefs
            // contiguous rather than leaving a gap wherever a document sat in
            // the list.
            media: numbered("M", media.iter().filter(|m| !m.is_document()).map(|m| m.id)),
        }
    }
}

/// `@{prefix}{n}@` for each id, numbered from 1 in order.
fn numbered(prefix: &str, ids: impl Iterator<Item = Uuid>) -> HashMap<Uuid, String> {
    ids.enumerate()
        .map(|(i, id)| (id, format!("@{prefix}{}@", i + 1)))
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

/// The `ASSO` structures each individual record carries for the witnesses.
///
/// GEDCOM only allows `ASSO` as a level-1 substructure of an INDI record
/// (GEDCOM 5.5.1 grammar; confirmed against real-world Gramps output and
/// rejected by Gramps as an unsupported tag when nested inside an event
/// detail). Which INDI record it's attached to, and what it points at,
/// depends on whether the witnessed event belongs to a person or a family:
///   - family event (e.g. a marriage): attached to the *witness's* own
///     INDI record, pointing at the family — mirrors how Gramps itself
///     writes it (`1 ASSO @F1@` / `2 RELA witness`).
///   - individual event (e.g. a burial): attached to the *event owner's*
///     own INDI record, pointing at the witness
///     (`1 ASSO @I2@` / `2 RELA witness`).
///
/// A person with several individual events sharing a witness can't
/// disambiguate which event on GEDCOM re-import (the format has no way
/// to nest ASSO under a specific event and still be portable) — this is
/// an inherent GEDCOM/Gramps limitation, not something round-tripped.
fn associations(
    events: &[Event],
    event_witnesses: &[EventWitness],
    xrefs: &Xrefs,
) -> HashMap<Uuid, Vec<GedAssociation>> {
    let witnesses_by_event = group_by(event_witnesses, |w| Some(w.event_id));
    let mut assoc_by_person: HashMap<Uuid, Vec<GedAssociation>> = HashMap::new();
    for evt in events {
        for w in witnesses_by_event.get(&evt.id).into_iter().flatten() {
            let Some((owner, target_xref)) = association_target(evt, w, xrefs) else {
                continue;
            };
            assoc_by_person
                .entry(owner)
                .or_default()
                .push(GedAssociation {
                    xref: target_xref,
                    relationship: w.relation.clone(),
                    association_type: None,
                    note: None,
                    custom_data: Vec::new(),
                });
        }
    }
    assoc_by_person
}

/// The person whose record carries a witness's `ASSO`, and the xref it points
/// at — see [`associations`]. `None` when either end is not exported.
fn association_target(evt: &Event, w: &EventWitness, xrefs: &Xrefs) -> Option<(Uuid, String)> {
    let witness_xref = xrefs.person.get(&w.person_id)?;
    if let Some(family_id) = evt.family_id {
        return Some((w.person_id, xrefs.family.get(&family_id)?.clone()));
    }
    Some((evt.person_id?, witness_xref.clone()))
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

/// The GEDCOM header naming OxidGene as the producer, and the submitter
/// GEDCOM 5.5.1 requires it to point at.
fn gedcom_header() -> Header {
    Header {
        submitter_tag: Some(SUBMITTER_XREF.to_string()),
        gedcom: Some(HeadMeta {
            version: Some("5.5.1".to_string()),
            form: Some("LINEAGE-LINKED".to_string()),
        }),
        source: Some(HeadSour {
            value: Some("OXIDGENE".to_string()),
            name: Some("OxidGene".to_string()),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
            ..Default::default()
        }),
        encoding: Some(Encoding {
            value: Some("UTF-8".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Serialize the model to GEDCOM text.
fn write_gedcom(data: &GedcomData) -> Result<String, String> {
    let write_span =
        tracing::info_span!("export.write", export.output_bytes = tracing::field::Empty,);
    let gedcom = write_span
        // Unwrapped: `crate::finish` continues long lines itself, as
        // `ged_io` would split them beside a space.
        .in_scope(|| {
            GedcomWriter::new()
                .max_line_length(usize::MAX)
                .write_to_string(data)
        })
        .map_err(|e| format!("GEDCOM write error: {e}"))?;
    write_span.record("export.output_bytes", gedcom.len());
    Ok(gedcom)
}

/// Add the OxidGene media extensions to the written GEDCOM.
fn inject_extensions(
    gedcom: String,
    media: &[Media],
    vignettes: &[Vignette],
    index: &ExportIndex,
) -> (String, Vec<String>) {
    let extensions_span = tracing::info_span!(
        "export.inject_extensions",
        export.input_bytes = gedcom.len(),
        export.output_bytes = tracing::field::Empty,
        export.vignette_count = vignettes.len(),
    );
    let (gedcom, warnings) = extensions_span.in_scope(|| {
        inject_oxidgene_media_extensions(
            gedcom,
            media,
            vignettes,
            &index.xrefs.media,
            &index.xrefs.person,
            &index.place_map,
            &index.notes_by_media,
        )
    });
    extensions_span.record("export.output_bytes", gedcom.len());
    (gedcom, warnings)
}

/// Every lookup the export reads, built once from the rows it writes.
struct ExportIndex<'a> {
    xrefs: Xrefs,
    sex_by_person: HashMap<Uuid, Sex>,
    place_map: HashMap<Uuid, &'a Place>,
    media_by_id: HashMap<Uuid, &'a Media>,
    pages_of: HashMap<Uuid, Vec<&'a Media>>,
    names_by_person: HashMap<Uuid, Vec<&'a PersonName>>,
    events_by_person: HashMap<Uuid, Vec<&'a Event>>,
    events_by_family: HashMap<Uuid, Vec<&'a Event>>,
    assoc_by_person: HashMap<Uuid, Vec<GedAssociation>>,
    cites_by_person: HashMap<Uuid, Vec<&'a Citation>>,
    cites_by_event: HashMap<Uuid, Vec<&'a Citation>>,
    cites_by_family: HashMap<Uuid, Vec<&'a Citation>>,
    notes_by_person: HashMap<Uuid, Vec<&'a Note>>,
    notes_by_family: HashMap<Uuid, Vec<&'a Note>>,
    notes_by_source: HashMap<Uuid, Vec<&'a Note>>,
    notes_by_event: HashMap<Uuid, Vec<&'a Note>>,
    notes_by_media: HashMap<Uuid, Vec<&'a Note>>,
    mlinks_by_person: HashMap<Uuid, Vec<&'a MediaLink>>,
    mlinks_by_event: HashMap<Uuid, Vec<&'a MediaLink>>,
    mlinks_by_family: HashMap<Uuid, Vec<&'a MediaLink>>,
    spouses_by_family: HashMap<Uuid, Vec<&'a FamilySpouse>>,
    children_by_family: HashMap<Uuid, Vec<&'a FamilyChild>>,
    fams_by_person: HashMap<Uuid, Vec<&'a FamilySpouse>>,
    famc_by_person: HashMap<Uuid, Vec<&'a FamilyChild>>,
}

impl ExportIndex<'_> {
    /// The `SUBM` record: who the file is from.
    ///
    /// The display name of the tree's "Who am I?" person, and `Not Provided`
    /// — Gramps' wording for the same gap — when the tree names nobody, or
    /// somebody nameless.
    fn submitter(&self, self_person_id: Option<Uuid>) -> Submitter {
        let name = self_person_id
            .and_then(|id| self.names_by_person.get(&id))
            .and_then(|names| PersonName::primary(names.iter().copied()))
            .map(PersonName::display_name)
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| "Not Provided".to_string());
        Submitter {
            xref: Some(SUBMITTER_XREF.to_string()),
            name: Some(name),
            ..Default::default()
        }
    }

    /// A `SOUR` record.
    fn source(&self, src: &Source) -> GedSource {
        GedSource {
            xref: self.xrefs.source.get(&src.id).cloned(),
            title: Some(src.title.clone()),
            author: src.author.clone(),
            // `ged_io` writes no `PUBL`: `additions` adds it to the text.
            publication_facts: None,
            abbreviation: src.abbreviation.clone(),
            notes: all_notes(self.notes_by_source.get(&src.id)),
            ..Default::default()
        }
    }

    /// What the text `ged_io` writes lacks, by record xref: the `RESN` of a
    /// private person or family, and each source's publication facts and
    /// agency — `ged_io` 0.16 writes neither a record's `RESN`, nor a `PUBL`,
    /// nor a source's `DATA`.
    ///
    /// A private record is `RESN confidential`, GEDCOM's word for data its
    /// owner marked to be kept from reports and exports, and the one the
    /// import reads back as private. `Public` has no `RESN` to be written as:
    /// it comes back as following the tree.
    fn additions(
        &self,
        persons: &[Person],
        families: &[Family],
        sources: &[Source],
    ) -> HashMap<String, Vec<crate::finish::Addition>> {
        let mut additions: HashMap<String, Vec<crate::finish::Addition>> = HashMap::new();
        let private = persons
            .iter()
            .filter(|p| p.privacy == Privacy::Private)
            .filter_map(|p| self.xrefs.person.get(&p.id))
            .chain(
                families
                    .iter()
                    .filter(|f| f.privacy == Privacy::Private)
                    .filter_map(|f| self.xrefs.family.get(&f.id)),
            );
        for xref in private {
            additions
                .entry(xref.clone())
                .or_default()
                .push(crate::finish::Addition::new("RESN", "confidential"));
        }
        for src in sources {
            let Some(xref) = self.xrefs.source.get(&src.id) else {
                continue;
            };
            let entry = additions.entry(xref.clone()).or_default();
            if let Some(publisher) = src.publisher.as_deref().filter(|p| !p.trim().is_empty()) {
                entry.push(crate::finish::Addition::new("PUBL", publisher));
            }
            if let Some(agency) = src.agency.as_deref().filter(|a| !a.trim().is_empty()) {
                entry.push(
                    crate::finish::Addition::new("DATA", "")
                        .with(crate::finish::Addition::new("AGNC", agency)),
                );
            }
        }
        additions
    }

    /// An `OBJE` record for one page.
    fn multimedia(&self, m: &Media, media_paths: &HashMap<Uuid, String>) -> GedMultimedia {
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
        // `media_paths` holds those, and is empty for every other export.
        let path = media_paths
            .get(&m.id)
            .cloned()
            .unwrap_or_else(|| m.file_path.clone());
        // A category the user chose is the better answer and implies a
        // medium; the stored medium is what they said when they answered
        // GEDCOM's own question directly, so it wins where both are set.
        let medium = match (described.document_category, described.source_media_type) {
            (Some(category), SourceMediaType::Other) => category.implied_medium(),
            (_, medium) => medium,
        };
        GedMultimedia {
            xref: self.xrefs.media.get(&m.id).cloned(),
            file: Some(Reference {
                value: Some(path),
                form: Some(Format {
                    value: Some(m.mime_type.clone()),
                    source_media_type: Some(medium.gedcom_value().to_string()),
                }),
                ..Default::default()
            }),
            title: page_title(
                described,
                document
                    .map(|d| pages_of_len(&self.pages_of, d.id))
                    .unwrap_or(1),
                m.page_index,
            ),
            note_structure: described.description.as_deref().map(to_ged_note),
            ..Default::default()
        }
    }

    /// An `INDI` record.
    fn individual(
        &self,
        person: &Person,
        merge_names: bool,
        merge_occupations: bool,
        warnings: &mut Vec<String>,
    ) -> Individual {
        // Names (GEDCOM allows {0:M} NAME structures; primary goes first
        // so `names.first()` on the way back in matches what we exported).
        let mut names: Vec<GedName> = self
            .names_by_person
            .get(&person.id)
            .map(|names| {
                let mut ordered: Vec<_> = names.iter().collect();
                ordered.sort_by_key(|n| !n.is_primary);
                ordered.into_iter().map(|pn| to_ged_name(pn)).collect()
            })
            .unwrap_or_default();
        if merge_names {
            names = merge_name_aliases_into_surn(names);
        }

        let (events, mut attributes) = self.individual_events(person.id, warnings);
        if merge_occupations {
            attributes = merge_occupation_attributes(attributes);
        }

        let source = self.citations(self.cites_by_person.get(&person.id), warnings);
        // The model holds one note; it stands in for all of them.
        let note =
            crate::finish::note_slot(person.id, self.notes_by_person.contains_key(&person.id));
        let multimedia = self.portrait_first_multimedia(person);
        // FAMS/FAMC back-links to the families this person belongs to.
        let families = to_ged_family_links(
            person.id,
            &self.fams_by_person,
            &self.famc_by_person,
            &self.xrefs.family,
            warnings,
        );

        Individual {
            xref: self.xrefs.person.get(&person.id).cloned(),
            names,
            sex: Some(Gender {
                value: convert_sex(person.sex),
                fact: None,
                sources: Vec::new(),
                custom_data: Vec::new(),
            }),
            families,
            events,
            attributes,
            source,
            note,
            multimedia,
            associations: self
                .assoc_by_person
                .get(&person.id)
                .cloned()
                .unwrap_or_default(),
            ..Default::default()
        }
    }

    /// A person's events (GEDCOM INDIVIDUAL_EVENT_STRUCTURE) and attributes
    /// (INDIVIDUAL_ATTRIBUTE_STRUCTURE, e.g. OCCU) — split so each
    /// round-trips to its own tag rather than a generic EVEN.
    fn individual_events(
        &self,
        person_id: Uuid,
        warnings: &mut Vec<String>,
    ) -> (Vec<GedDetail>, Vec<GedAttributeDetail>) {
        let mut events: Vec<GedDetail> = Vec::new();
        let mut attributes: Vec<GedAttributeDetail> = Vec::new();
        for evt in self.events_by_person.get(&person_id).into_iter().flatten() {
            match event_type_to_attribute(evt.event_type) {
                Some(attribute) => {
                    attributes.push(to_ged_attribute_detail(evt, attribute, self, warnings));
                }
                None => events.push(to_ged_detail(evt, self, warnings)),
            }
        }
        (events, attributes)
    }

    /// A person's multimedia links, portrait first.
    ///
    /// GEDCOM has no primary-photo flag, so the choice cannot be stated —
    /// but it can be *implied*, because order survives and our own import
    /// takes a person's first picture when no portrait is recorded. Writing
    /// the portrait first is therefore what carries the choice across a
    /// round trip; without it the person kept every photograph and came back
    /// represented by whichever one happened to be written first.
    ///
    /// A crop portrait has no whole media to lead with, so those trees fall
    /// back to the first picture as before: GEDCOM cannot express a region
    /// of an image as somebody's portrait at all.
    fn portrait_first_multimedia(&self, person: &Person) -> Vec<GedMultimedia> {
        let Some(mls) = self.mlinks_by_person.get(&person.id) else {
            return Vec::new();
        };
        let mut ordered: Vec<&&MediaLink> = mls.iter().collect();
        ordered.sort_by_key(|ml| {
            (
                person.portrait_media_id != Some(ml.media_id),
                ml.sort_order,
                ml.id,
            )
        });
        self.multimedia_refs(ordered.into_iter().copied())
    }

    /// A `FAM` record.
    fn family(&self, fam: &Family, warnings: &mut Vec<String>) -> GedFamily {
        let (husband, wife) = self.spouse_slots(fam, warnings);
        let children: Vec<String> = self
            .children_by_family
            .get(&fam.id)
            .map(|cs| {
                let mut sorted: Vec<&&FamilyChild> = cs.iter().collect();
                sorted.sort_by_key(|fc| fc.sort_order);
                sorted
                    .iter()
                    .filter_map(|fc| self.xrefs.person.get(&fc.person_id).cloned())
                    .collect()
            })
            .unwrap_or_default();
        let events: Vec<GedDetail> = self
            .events_by_family
            .get(&fam.id)
            .map(|evts| {
                evts.iter()
                    .map(|evt| to_ged_detail(evt, self, warnings))
                    .collect()
            })
            .unwrap_or_default();

        GedFamily {
            xref: self.xrefs.family.get(&fam.id).cloned(),
            individual1: husband,
            individual2: wife,
            children,
            events,
            // Citations with a family_id but no event_id.
            sources: self.citations(self.cites_by_family.get(&fam.id), warnings),
            notes: all_notes(self.notes_by_family.get(&fam.id)),
            multimedia: self.multimedia_refs(
                self.mlinks_by_family
                    .get(&fam.id)
                    .into_iter()
                    .flatten()
                    .copied(),
            ),
            ..Default::default()
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
    ) -> (Option<String>, Option<String>) {
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
        let xref = |slot: Option<Uuid>| slot.and_then(|id| self.xrefs.person.get(&id).cloned());
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
            .filter_map(|c| to_ged_citation(c, &self.xrefs.source, warnings))
            .collect()
    }

    /// The `OBJE` pointers of some media links, in order.
    fn multimedia_refs<'l>(
        &self,
        links: impl Iterator<Item = &'l MediaLink>,
    ) -> Vec<GedMultimedia> {
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

    /// The texts of every note of a person or an event, for
    /// [`crate::finish::finish`] to write where the model held one.
    fn notes_of(&self, owner: Uuid) -> Vec<&str> {
        self.notes_by_person
            .get(&owner)
            .or_else(|| self.notes_by_event.get(&owner))
            .into_iter()
            .flatten()
            .map(|note| note.text.as_str())
            .collect()
    }

    /// What an event and an attribute write alike: its date, place,
    /// citations, notes and media.
    fn event_parts(&self, evt: &Event, warnings: &mut Vec<String>) -> EventParts {
        // Recompose the calendar escape and qualifier tag the columns were
        // split from on import — see `crate::date`.
        let date = crate::date::format(
            evt.calendar,
            evt.date_qualifier,
            evt.date_value.as_deref(),
            evt.date_value2.as_deref(),
        )
        .map(|value| Date {
            value: Some(value),
            ..Default::default()
        });
        EventParts {
            date,
            place: evt
                .place_id
                .and_then(|pid| self.place_map.get(&pid))
                .map(|p| to_ged_place(p)),
            citations: self.citations(self.cites_by_event.get(&evt.id), warnings),
            // The model holds one note; it stands in for all of them.
            note: crate::finish::note_slot(evt.id, self.notes_by_event.contains_key(&evt.id)),
            multimedia: self.multimedia_refs(
                self.mlinks_by_event
                    .get(&evt.id)
                    .into_iter()
                    .flatten()
                    .copied(),
            ),
        }
    }
}

/// The parts [`to_ged_detail`] and [`to_ged_attribute_detail`] share.
struct EventParts {
    date: Option<Date>,
    place: Option<GedPlace>,
    citations: Vec<GedCitation>,
    note: Option<GedNote>,
    multimedia: Vec<GedMultimedia>,
}

/// A `PLAC` with its `MAP` coordinates when both are known.
fn to_ged_place(p: &Place) -> GedPlace {
    let map = match (p.latitude, p.longitude) {
        (Some(lat), Some(lon)) => Some(MapCoordinates {
            latitude: Some(format_coord(lat, true)),
            longitude: Some(format_coord(lon, false)),
        }),
        _ => None,
    };
    GedPlace {
        value: Some(p.name.clone()),
        map,
        ..Default::default()
    }
}

/// Every note of a record.
fn all_notes(notes: Option<&Vec<&Note>>) -> Vec<GedNote> {
    notes
        .into_iter()
        .flatten()
        .map(|n| to_ged_note(&n.text))
        .collect()
}

fn inject_oxidgene_media_extensions(
    gedcom: String,
    media: &[Media],
    vignettes: &[Vignette],
    media_xref: &HashMap<Uuid, String>,
    person_xref: &HashMap<Uuid, String>,
    places: &HashMap<Uuid, &Place>,
    notes_by_media: &HashMap<Uuid, Vec<&Note>>,
) -> (String, Vec<String>) {
    let mut warnings = Vec::new();
    let mut by_media = HashMap::<String, Vec<String>>::new();

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
        let Some(xref) = media_xref.get(&item.id) else {
            continue;
        };
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
                .entry(xref.clone())
                .or_default()
                .push(format!("1 _OXIDGENE_MEDIA {value}")),
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
                tags: document.tags.clone(),
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
                .entry(xref.clone())
                .or_default()
                .push(format!("1 _OXIDGENE_DOC {value}")),
            Err(err) => warnings.push(format!(
                "Media {} document metadata could not be serialized: {err}",
                item.id
            )),
        }
    }

    for vignette in vignettes {
        let Some(media) = media_xref.get(&vignette.media_id) else {
            warnings.push(format!(
                "Vignette {} references media {} which is not part of this export",
                vignette.id, vignette.media_id
            ));
            continue;
        };
        let person = match vignette.person_id {
            Some(person_id) => {
                let Some(person) = person_xref.get(&person_id) else {
                    warnings.push(format!(
                        "Vignette {} references person {} who is not part of this export",
                        vignette.id, person_id
                    ));
                    continue;
                };
                person.as_str()
            }
            None => "-",
        };
        by_media.entry(media.clone()).or_default().push(format!(
            "1 _OXIDGENE_VIGNETTE {person} {} {} {} {}",
            vignette.x, vignette.y, vignette.width, vignette.height
        ));
    }

    let mut output = String::with_capacity(gedcom.len() + media.len() * 256 + vignettes.len() * 64);
    for line in gedcom.lines() {
        output.push_str(line);
        output.push('\n');
        if let Some(header) = line.strip_prefix("0 ") {
            let mut fields = header.split_whitespace();
            if let (Some(xref), Some("OBJE")) = (fields.next(), fields.next())
                && let Some(rows) = by_media.get(xref)
            {
                for row in rows {
                    output.push_str(row);
                    output.push('\n');
                }
            }
        }
    }
    (output, warnings)
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

/// Wrap a GEDCOM string and its media into a GEDZIP archive, per GEDCOM 7.0.
///
/// `files` pairs each archive path — the same value the corresponding `FILE`
/// line carries, from [`archive_path`] — with the bytes to store there. An
/// empty slice produces the bare `gedcom.ged` archive, which is what this
/// wrote unconditionally before: the format's entire point is that the media
/// travel with the data, and a `.gdz` holding only the GEDCOM is a `.ged` in
/// a costume.
///
/// # Errors
///
/// Returns `Err` if the ZIP archive cannot be written.
pub fn export_gedzip(gedcom: &str, files: &[(String, String, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let cursor = std::io::Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(cursor);
    write_gedcom_entry(&mut writer, gedcom)?;
    for (path, mime_type, bytes) in files {
        write_media_entry(&mut writer, path, mime_type, bytes)?;
    }
    let cursor = writer.finish().map_err(|e| format!("GEDZIP error: {e}"))?;
    Ok(cursor.into_inner())
}

/// A GEDZIP archive written directly to a local file.
pub struct GedzipFileWriter {
    writer: zip::ZipWriter<std::fs::File>,
}

impl GedzipFileWriter {
    /// Create an archive and write its mandatory `gedcom.ged` entry.
    pub fn create(path: &std::path::Path, gedcom: &str) -> Result<Self, String> {
        let file = std::fs::File::create(path).map_err(|e| format!("GEDZIP error: {e}"))?;
        let mut writer = zip::ZipWriter::new(file);
        write_gedcom_entry(&mut writer, gedcom)?;
        Ok(Self { writer })
    }

    /// Add one media entry. Callers can release `bytes` before loading the next.
    pub fn add_media_file(
        &mut self,
        path: &str,
        mime_type: &str,
        bytes: &[u8],
    ) -> Result<(), String> {
        write_media_entry(&mut self.writer, path, mime_type, bytes)
    }

    /// Finalize the ZIP central directory and flush the output file.
    pub fn finish(self) -> Result<(), String> {
        self.writer
            .finish()
            .map(|_| ())
            .map_err(|e| format!("GEDZIP error: {e}"))
    }
}

fn write_gedcom_entry<W: std::io::Write + std::io::Seek>(
    writer: &mut zip::ZipWriter<W>,
    gedcom: &str,
) -> Result<(), String> {
    let options = zip::write::FileOptions::<()>::default()
        .compression_method(zip::CompressionMethod::Deflated);
    writer
        .start_file(ged_io::gedzip::GEDCOM_FILENAME, options)
        .map_err(|e| format!("GEDZIP error: {e}"))?;
    std::io::Write::write_all(writer, gedcom.as_bytes()).map_err(|e| format!("GEDZIP error: {e}"))
}

fn write_media_entry<W: std::io::Write + std::io::Seek>(
    writer: &mut zip::ZipWriter<W>,
    path: &str,
    mime_type: &str,
    bytes: &[u8],
) -> Result<(), String> {
    let options = zip::write::FileOptions::<()>::default()
        .compression_method(media_compression_method(mime_type));
    writer
        .start_file(path, options)
        .map_err(|e| format!("GEDZIP error: {e}"))?;
    std::io::Write::write_all(writer, bytes).map_err(|e| format!("GEDZIP error: {e}"))
}

fn media_compression_method(mime_type: &str) -> zip::CompressionMethod {
    match mime_type {
        "image/jpeg" | "image/png" | "image/gif" | "image/webp" | "application/pdf" => {
            zip::CompressionMethod::Stored
        }
        _ => zip::CompressionMethod::Deflated,
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Conversion helpers
// ═══════════════════════════════════════════════════════════════════════

fn convert_sex(sex: Sex) -> GenderType {
    match sex {
        Sex::Male => GenderType::Male,
        Sex::Female => GenderType::Female,
        Sex::Unknown => GenderType::Unknown,
    }
}

/// Builds a person's INDI-level `FAMS`/`FAMC` back-links: one `FamilyLink`
/// per family they're a spouse in, then one per family they're a child in.
/// Without these, the exported file only encodes family membership on the
/// `FAM` record's own `HUSB`/`WIFE`/`CHIL` — most GEDCOM readers instead (or
/// additionally) expect the reverse links on `INDI`, so omitting them makes
/// the file read as a set of disconnected individuals in other software.
fn to_ged_family_links(
    person_id: Uuid,
    fams_by_person: &HashMap<Uuid, Vec<&FamilySpouse>>,
    famc_by_person: &HashMap<Uuid, Vec<&FamilyChild>>,
    family_xref: &HashMap<Uuid, String>,
    warnings: &mut Vec<String>,
) -> Vec<FamilyLink> {
    let spouse_links = fams_by_person
        .get(&person_id)
        .into_iter()
        .flatten()
        .map(|fs| (fs.family_id, FamilyLinkType::Spouse, None, "spouse"));
    let child_links = famc_by_person
        .get(&person_id)
        .into_iter()
        .flatten()
        .map(|fc| {
            (
                fc.family_id,
                FamilyLinkType::Child,
                convert_child_type_to_pedigree(fc.child_type),
                "parental",
            )
        });
    let mut links = Vec::new();
    for (family_id, family_link_type, pedigree_linkage_type, kind) in
        spouse_links.chain(child_links)
    {
        let Some(xref) = family_xref.get(&family_id) else {
            warnings.push(format!(
                "Person {person_id}: {kind} family {family_id} not found"
            ));
            continue;
        };
        links.push(FamilyLink {
            xref: xref.clone(),
            family_link_type,
            pedigree_linkage_type,
            child_linkage_status: None,
            adopted_by: None,
            note: None,
            custom_data: Vec::new(),
        });
    }
    links
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

fn to_ged_name(pn: &PersonName) -> GedName {
    // Build the GEDCOM full name value: "Given /Surname/". The surname goes in
    // with its particle, as GEDCOM expects — SPFX below repeats it separately.
    let given_part = pn.given_names.as_deref().unwrap_or("");
    let full_surname = pn.full_surname().unwrap_or_default();
    let surname_part = full_surname.as_str();
    let value = if !given_part.is_empty() || !surname_part.is_empty() {
        Some(
            format!("{given_part} /{surname_part}/")
                .trim_start()
                .to_string(),
        )
    } else {
        None
    };

    let name_type = match pn.name_type {
        NameType::Birth => Some(GedNameType::Birth),
        NameType::Married => Some(GedNameType::Married),
        NameType::Maiden => Some(GedNameType::Maiden),
        NameType::AlsoKnownAs => Some(GedNameType::Aka),
        NameType::Religious => Some(GedNameType::Religious),
        // GEDCOM's NAME.TYPE enumeration has no finer-grained "also known as",
        // so OxidGene's four refinements all export as `aka`. The distinction
        // survives internally, not across a GEDCOM round trip.
        NameType::GivenName | NameType::Alias | NameType::Byname | NameType::Sobriquet => {
            Some(GedNameType::Aka)
        }
        NameType::Other => None,
    };

    GedName {
        value,
        given: pn.given_names.clone(),
        // SURN is the root alone; the particle rides in SPFX beside it.
        surname: pn.surname.clone(),
        surname_prefix: pn.surname_prefix.clone(),
        prefix: pn.prefix.clone(),
        suffix: pn.suffix.clone(),
        nickname: pn.nickname.clone(),
        name_type,
        ..Default::default()
    }
}

fn convert_event_type(et: EventType) -> GedEvent {
    match et {
        EventType::Birth => GedEvent::Birth,
        EventType::Death => GedEvent::Death,
        EventType::Baptism => GedEvent::Baptism,
        EventType::Burial => GedEvent::Burial,
        EventType::Cremation => GedEvent::Cremation,
        EventType::Graduation => GedEvent::Graduation,
        EventType::Immigration => GedEvent::Immigration,
        EventType::Emigration => GedEvent::Emigration,
        EventType::Naturalization => GedEvent::Naturalization,
        EventType::Census => GedEvent::Census,
        EventType::Residence => GedEvent::Residence,
        EventType::Retirement => GedEvent::Retired,
        EventType::Will => GedEvent::Will,
        EventType::Probate => GedEvent::Probate,
        EventType::Marriage => GedEvent::Marriage,
        EventType::Divorce => GedEvent::Divorce,
        EventType::Annulment => GedEvent::Annulment,
        EventType::Engagement => GedEvent::Engagement,
        EventType::MarriageBann => GedEvent::MarriageBann,
        EventType::MarriageContract => GedEvent::MarriageContract,
        EventType::MarriageLicense => GedEvent::MarriageLicense,
        EventType::MarriageSettlement => GedEvent::MarriageSettlement,
        EventType::Separation => GedEvent::Separated,
        EventType::DivorceFiled => GedEvent::DivorceFiled,
        // No dedicated GEDCOM tag exists for civil unions/PACS/cohabitation —
        // written back as a generic EVEN with the TYPE sub-tag set from
        // `description` (see `to_ged_detail`).
        EventType::CivilUnion => GedEvent::Event,
        EventType::Adoption => GedEvent::Adoption,
        EventType::Blessing => GedEvent::Blessing,
        EventType::Ordination => GedEvent::Ordination,
        EventType::Christening => GedEvent::Christening,
        EventType::AdultChristening => GedEvent::AdultChristening,
        EventType::Other | EventType::Occupation => GedEvent::Other,
        // The individual-attribute variants (CasteName, PhysicalDescription,
        // Education, ...) always round-trip through `to_ged_attribute_detail`
        // instead (see the per-person event/attribute split in
        // `export_gedcom`) — this arm only exists for exhaustiveness.
        EventType::Confirmation
        | EventType::FirstCommunion
        | EventType::BarBatMitzvah
        | EventType::MilitaryService
        | EventType::CasteName
        | EventType::PhysicalDescription
        | EventType::Education
        | EventType::NationalId
        | EventType::NationalOrigin
        | EventType::ChildrenCount
        | EventType::MarriagesCount
        | EventType::Property
        | EventType::Religion
        | EventType::SocialSecurityNumber
        | EventType::NobilityTitle
        | EventType::Fact => GedEvent::Other,
        // GeneWeb's vocabulary: no GEDCOM tag, so a generic EVEN whose TYPE
        // names the event (see `gedcom_type_label`).
        EventType::Accomplishment
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
        | EventType::NoMention => GedEvent::Event,
    }
}

/// The `TYPE` label a generic `EVEN` must carry to name this event type.
///
/// `None` for types that are a GEDCOM tag of their own, and for `Other` and
/// `CivilUnion`, whose classification lives in the event's description.
///
/// This is the inverse of the table `oxidgene_gedcom::import` reads, so an
/// event exported here is recognised as the same type when read back.
fn gedcom_type_label(et: EventType) -> Option<&'static str> {
    match et {
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
        EventType::NoMention => Some("nomen"),
        _ => None,
    }
}

/// The `QUAY` a confidence is written as; none for a citation nobody
/// assessed. `QUAY` has four values for five levels, so `VeryHigh` shares
/// `3` with `High`. The inverse of `import`'s `convert_quay`.
fn convert_confidence(c: Option<Confidence>) -> Option<CertaintyAssessment> {
    Some(match c? {
        Confidence::VeryLow => CertaintyAssessment::Unreliable,
        Confidence::Low => CertaintyAssessment::Questionable,
        Confidence::Medium => CertaintyAssessment::Secondary,
        Confidence::High | Confidence::VeryHigh => CertaintyAssessment::Direct,
    })
}

/// The `ged_io` age a stored age is written as. `phrase` stays unset: the
/// writer would emit GEDCOM 7's `PHRASE` into a 5.5.1 file.
fn to_ged_age(age: Option<&str>) -> Option<GedAge> {
    use oxidgene_core::types::age::{AgeAtEvent, AgeModifier};
    let unit = |count: Option<u16>| count.and_then(|c| u8::try_from(c).ok());
    Some(match age?.parse::<AgeAtEvent>().ok()? {
        AgeAtEvent::Child => GedAge::Child,
        AgeAtEvent::Infant => GedAge::Infant,
        AgeAtEvent::Stillborn => GedAge::Stillborn,
        AgeAtEvent::Duration {
            modifier,
            years,
            months,
            weeks,
            days,
        } => GedAge::Numeric {
            years,
            months: unit(months),
            weeks: unit(weeks),
            days: unit(days),
            modifier: match modifier {
                AgeModifier::Exact => GedAgeModifier::Exact,
                AgeModifier::LessThan => GedAgeModifier::LessThan,
                AgeModifier::GreaterThan => GedAgeModifier::GreaterThan,
            },
            phrase: None,
        },
    })
}

fn to_ged_note(text: &str) -> GedNote {
    GedNote {
        value: Some(text.to_string()),
        ..Default::default()
    }
}

fn to_ged_detail(evt: &Event, index: &ExportIndex, warnings: &mut Vec<String>) -> GedDetail {
    let event = convert_event_type(evt.event_type);
    let EventParts {
        date,
        place,
        citations,
        note,
        multimedia,
    } = index.event_parts(evt, warnings);

    // Witnesses/godparents are exported separately as a level-1 `ASSO` on
    // the relevant INDI record (see `associations`), not nested here —
    // GEDCOM 5.5.1 only allows `ASSO` directly under an INDIVIDUAL_RECORD,
    // and readers (Gramps included) reject it as a substructure of an event.
    let associations: Vec<GedAssociation> = Vec::new();

    // An adoption event's adoptive family is not captured on import (see
    // `import_event_detail`'s comment: `Event.family_id` can't be reused
    // for it without the event masquerading as a family-level event
    // elsewhere), so there's nothing to round-trip into `family_link` here.
    GedDetail {
        event,
        value: None,
        date,
        place,
        note,
        family_link: None,
        family_event_details: Vec::new(),
        // Round-trips the classification back into the GEDCOM TYPE sub-tag it
        // was read from. A type that names itself writes its own label; the
        // rest (Other, CivilUnion) fall back to the free-text description,
        // which is where their classification lives.
        event_type: gedcom_type_label(evt.event_type)
            .map(str::to_owned)
            .or_else(|| evt.description.clone()),
        citations,
        multimedia,
        sort_date: None,
        associations,
        cause: evt.cause.clone(),
        restriction: None,
        // A family event's ages are its spouses' own.
        age: to_ged_age(evt.age.as_deref()).filter(|_| evt.family_id.is_none()),
        agency: evt.agency.clone(),
        religion: None,
    }
}

/// Maps the `EventType` variants that represent a GEDCOM
/// `INDIVIDUAL_ATTRIBUTE_STRUCTURE` (OCCU, RESI, TITL, ...) to their
/// `ged_io` attribute tag, so they round-trip to their original tag
/// instead of a generic `EVEN`. `None` for event-shaped types, which are
/// exported via `to_ged_detail` instead.
fn event_type_to_attribute(et: EventType) -> Option<GedIndividualAttribute> {
    match et {
        EventType::Occupation => Some(GedIndividualAttribute::Occupation),
        EventType::CasteName => Some(GedIndividualAttribute::CastName),
        EventType::PhysicalDescription => Some(GedIndividualAttribute::PhysicalDescription),
        EventType::Education => Some(GedIndividualAttribute::ScholasticAchievement),
        EventType::NationalId => Some(GedIndividualAttribute::NationalIDNumber),
        EventType::NationalOrigin => Some(GedIndividualAttribute::NationalOrTribalOrigin),
        EventType::ChildrenCount => Some(GedIndividualAttribute::CountOfChildren),
        EventType::MarriagesCount => Some(GedIndividualAttribute::CountOfMarriages),
        EventType::Property => Some(GedIndividualAttribute::Possessions),
        EventType::Religion => Some(GedIndividualAttribute::ReligiousAffiliation),
        EventType::SocialSecurityNumber => Some(GedIndividualAttribute::SocialSecurityNumber),
        EventType::NobilityTitle => Some(GedIndividualAttribute::NobilityTypeTitle),
        EventType::Fact => Some(GedIndividualAttribute::Fact),
        _ => None,
    }
}

/// Collapses every `OCCU` attribute in a person's attribute list into one,
/// for the `merge_occupations` export option (see `export_gedcom`). Values
/// are joined with `", "`; the first occupation's date/place/cause/etc. are
/// kept, and every occupation's source citations and first note are
/// preserved on the merged entry. A no-op if the person has 0 or 1 `OCCU`.
fn merge_occupation_attributes(attributes: Vec<GedAttributeDetail>) -> Vec<GedAttributeDetail> {
    let occupation_count = attributes
        .iter()
        .filter(|a| a.attribute == GedIndividualAttribute::Occupation)
        .count();
    if occupation_count <= 1 {
        return attributes;
    }

    let mut result = Vec::with_capacity(attributes.len() - occupation_count + 1);
    let mut merged: Option<GedAttributeDetail> = None;
    for attr in attributes {
        if attr.attribute != GedIndividualAttribute::Occupation {
            result.push(attr);
            continue;
        }
        match &mut merged {
            None => merged = Some(attr),
            Some(m) => {
                if let Some(value) = attr.value {
                    match &mut m.value {
                        Some(existing) => {
                            existing.push_str(", ");
                            existing.push_str(&value);
                        }
                        None => m.value = Some(value),
                    }
                }
                m.sources.extend(attr.sources);
                // One OCCU line became several professions on import, each
                // carrying the same scan. Merging them back must not write
                // that scan once per profession.
                for media in attr.multimedia {
                    if !m.multimedia.iter().any(|kept| kept.xref == media.xref) {
                        m.multimedia.push(media);
                    }
                }
                if m.note.is_none() {
                    m.note = attr.note;
                }
            }
        }
    }
    if let Some(m) = merged {
        result.push(m);
    }
    result
}

/// Collapses a person's non-primary `PersonName`s into the primary name's
/// `SURN` tag, for the `merge_names` export option (see `export_gedcom`).
/// Geneanet only reads the first `NAME` structure, so it packs every other
/// surname it knows about into that `NAME`'s `SURN` sub-tag as a
/// comma-separated list instead of emitting one `NAME`/`SURN` per alias
/// (mirrors what its own exporter produces). `names` must have the primary
/// name first (see `export_gedcom`'s ordering). A no-op if the person has 0
/// or 1 names.
fn merge_name_aliases_into_surn(mut names: Vec<GedName>) -> Vec<GedName> {
    if names.len() <= 1 {
        return names;
    }
    let mut primary = names.remove(0);
    let alias_surnames: Vec<String> = names.into_iter().filter_map(|n| n.surname).collect();
    if !alias_surnames.is_empty() {
        primary.surname = Some(alias_surnames.join(","));
    }
    vec![primary]
}

/// Exports an individual attribute (e.g. `EventType::Occupation`, GEDCOM
/// `OCCU`) as an `AttributeDetail` under `Individual.attributes`, so it
/// round-trips to its original tag instead of a generic `EVEN`.
fn to_ged_attribute_detail(
    evt: &Event,
    attribute: GedIndividualAttribute,
    index: &ExportIndex,
    warnings: &mut Vec<String>,
) -> GedAttributeDetail {
    // An attribute documents itself as readily as an event does: a scan of the
    // trade card behind an OCCU, of the deed behind a TITL. The same parts,
    // media links included, read the same way as `to_ged_detail` reads them.
    let EventParts {
        date,
        place,
        citations,
        note,
        multimedia,
    } = index.event_parts(evt, warnings);

    GedAttributeDetail {
        attribute,
        // The attribute's own line value (e.g. "Account Manager" for OCCU) —
        // mirrors the import side, which reads this same field back from
        // `detail.value` first (falling back to the TYPE sub-tag).
        value: evt.description.clone(),
        place,
        date,
        sources: citations,
        note,
        multimedia,
        attribute_type: None,
        restriction: None,
        age: to_ged_age(evt.age.as_deref()),
        address: None,
        cause: evt.cause.clone(),
        agency: evt.agency.clone(),
    }
}

fn to_ged_citation(
    cite: &Citation,
    source_xref: &HashMap<Uuid, String>,
    warnings: &mut Vec<String>,
) -> Option<GedCitation> {
    let xref = match source_xref.get(&cite.source_id) {
        Some(x) => x.clone(),
        None => {
            warnings.push(format!(
                "Citation {} references unknown source {}",
                cite.id, cite.source_id
            ));
            return None;
        }
    };

    Some(GedCitation {
        source: CitationSource::Xref(xref),
        page: cite.page.clone(),
        // The transcript of what the source says, GEDCOM's `DATA.TEXT`.
        data: cite
            .text
            .as_deref()
            .filter(|text| !text.trim().is_empty())
            .map(|text| SourceCitationData {
                date: None,
                text: Some(GedText {
                    value: Some(text.to_string()),
                }),
            }),
        note: None,
        certainty_assessment: convert_confidence(cite.confidence),
        submitter_registered_rfn: None,
        multimedia: Vec::new(),
        custom_data: Vec::new(),
        event_type: None,
        role: None,
    })
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
    media_xref: &HashMap<Uuid, String>,
    pages_of: &HashMap<Uuid, Vec<&Media>>,
) -> Vec<GedMultimedia> {
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
            Some(GedMultimedia {
                xref: Some(media_xref.get(&id)?.clone()),
                ..Default::default()
            })
        })
        .collect()
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
            Some(CertaintyAssessment::Direct)
        );
        assert_eq!(
            convert_confidence(Some(Confidence::Medium)),
            Some(CertaintyAssessment::Secondary)
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
            repository_name: None,
            agency: None,
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
            created_at: now,
            updated_at: now,
        };

        let export = export_gedcom(
            &[],
            &[],
            std::slice::from_ref(&family),
            &[],
            &[],
            &[],
            &[],
            &[],
            std::slice::from_ref(&source),
            std::slice::from_ref(&citation),
            &[],
            &[],
            &[],
            &[],
            false,
            false,
            &HashMap::new(),
            None,
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
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &rows,
            &[],
            &[],
            &[],
            false,
            false,
            &paths,
            None,
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

        let bytes = export_gedzip(
            &export.gedcom,
            &[(
                path.clone(),
                "image/jpeg".to_string(),
                b"JPEGBYTES".to_vec(),
            )],
        )
        .expect("zips");
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("reads back");
        assert_eq!(
            archive
                .by_name(ged_io::gedzip::GEDCOM_FILENAME)
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
            &[(
                "media/scan.bmp".to_string(),
                "image/bmp".to_string(),
                vec![0; 1024],
            )],
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
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &m,
            &[],
            &[],
            &[],
            false,
            false,
            &HashMap::new(),
            None,
        )
        .expect("exports");
        assert!(
            export.gedcom.contains("3 TYPE TOMBSTONE"),
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
        document.tags = vec!["ceremony".to_string(), "outdoors".to_string()];
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
        assert_eq!(imported.tags, original.tags);
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
            created_at: document.created_at,
            updated_at: document.updated_at,
            deleted_at: None,
        };
        let archive_path = archive_path(&page).expect("stored media has an archive path");
        let media_paths = HashMap::from([(page.id, archive_path.clone())]);
        let media = document.clone();
        let rows = [document, page];

        let export = export_gedcom(
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            std::slice::from_ref(&place),
            &[],
            &[],
            &rows,
            &[],
            &[],
            std::slice::from_ref(&note),
            false,
            false,
            &media_paths,
            None,
        )
        .expect("exports");
        assert!(export.gedcom.contains("1 _OXIDGENE_MEDIA {"));
        assert!(export.gedcom.contains("1 _OXIDGENE_DOC {"));
        assert!(export.gedcom.contains("1 NOTE First line"));
        assert!(export.gedcom.contains("2 CONT Second line"));

        let archive = export_gedzip(
            &export.gedcom,
            &[(
                archive_path,
                "image/jpeg".to_string(),
                b"IMAGE BYTES".to_vec(),
            )],
        )
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
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &m,
            &[],
            &[],
            &[],
            false,
            false,
            &HashMap::new(),
            None,
        )
        .expect("exports");
        assert!(export.gedcom.contains("MANUSCRIPT"), "{}", export.gedcom);
    }

    #[test]
    fn an_explicit_medium_is_not_overridden_by_the_category() {
        // The user answered both questions; neither answer is ours to discard.
        let (mut document, page) = document_with_page("microfilm.jpg", "image/jpeg", true);
        document.document_category = Some(DocumentCategory::CivilRecord);
        document.source_media_type = SourceMediaType::Fiche;
        let export = export_gedcom(
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[document, page],
            &[],
            &[],
            &[],
            false,
            false,
            &HashMap::new(),
            None,
        )
        .expect("exports");
        assert!(export.gedcom.contains("FICHE"), "{}", export.gedcom);
        assert!(!export.gedcom.contains("MANUSCRIPT"));
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
            std::slice::from_ref(&person),
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &rows,
            std::slice::from_ref(&link),
            &[],
            &[],
            false,
            false,
            &HashMap::new(),
            None,
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
            std::slice::from_ref(&person),
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &rows,
            std::slice::from_ref(&link),
            std::slice::from_ref(&vignette),
            &[],
            false,
            false,
            &paths,
            None,
        )
        .expect("exports");
        assert!(
            export
                .gedcom
                .contains("1 _OXIDGENE_VIGNETTE @I1@ 120 45 64 82"),
            "{}",
            export.gedcom
        );

        let archive = export_gedzip(
            &export.gedcom,
            &[(path, "image/jpeg".to_string(), b"JPEGBYTES".to_vec())],
        )
        .expect("writes GEDZIP");
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
            place_id: None,
            person_id: Some(person.id),
            family_id: None,
            description: None,
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
            std::slice::from_ref(&person),
            &[],
            &[],
            &[],
            &[],
            std::slice::from_ref(&event),
            &[],
            &[],
            &[],
            &[],
            &rows,
            std::slice::from_ref(&link),
            &[],
            &[],
            false,
            false,
            &HashMap::new(),
            None,
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
        // import and merges back into one line on export, and the scan must
        // come through that once, not once per profession.
        let person = person_row();
        let occupation = Event {
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
            place_id: None,
            person_id: Some(person.id),
            family_id: None,
            description: Some("Baker, Miller".to_string()),
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
            std::slice::from_ref(&person),
            &[],
            &[],
            &[],
            &[],
            std::slice::from_ref(&occupation),
            &[],
            &[],
            &[],
            &[],
            &rows,
            std::slice::from_ref(&link),
            &[],
            &[],
            false,
            false,
            &HashMap::new(),
            None,
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

        // And back out again, from the shape the import produced: two
        // Occupation events on one person, both naming the same scan. The
        // merge writes one OCCU line, and must write the scan once.
        let events: Vec<Event> = back
            .events
            .iter()
            .map(|event| Event {
                person_id: Some(person.id),
                tree_id: person.tree_id,
                ..event.clone()
            })
            .collect();
        let links: Vec<MediaLink> = events
            .iter()
            .map(|event| MediaLink {
                id: Uuid::now_v7(),
                media_id: medium.id,
                person_id: None,
                event_id: Some(event.id),
                source_id: None,
                family_id: None,
                sort_order: 0,
            })
            .collect();
        let again = export_gedcom(
            std::slice::from_ref(&person),
            &[],
            &[],
            &[],
            &[],
            &events,
            &[],
            &[],
            &[],
            &[],
            &rows,
            &links,
            &[],
            &[],
            // Merging the professions back into one OCCU is the case under
            // test: it is what has to not multiply the scan.
            true,
            false,
            &HashMap::new(),
            None,
        )
        .expect("exports");

        assert_eq!(
            again.gedcom.matches("1 OCCU").count(),
            1,
            "{}",
            again.gedcom
        );
        assert_eq!(
            again.gedcom.matches("2 OBJE @M1@").count(),
            1,
            "{}",
            again.gedcom
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
                created_at: second_page.created_at,
                updated_at: second_page.updated_at,
                deleted_at: None,
            },
        ];
        let rows = [document, second_page, first_page];

        let export = export_gedcom(
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &rows,
            &[],
            &[],
            &notes,
            false,
            false,
            &HashMap::new(),
            None,
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
            std::slice::from_ref(&person),
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[
                other.clone(),
                other_page.clone(),
                chosen.clone(),
                chosen_page.clone(),
            ],
            &links,
            &[],
            &[],
            false,
            false,
            &HashMap::new(),
            None,
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
            std::slice::from_ref(&person),
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &rows,
            std::slice::from_ref(&link),
            &[],
            &[],
            false,
            false,
            &HashMap::new(),
            None,
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
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[document, cover],
            &[],
            &[],
            &[],
            false,
            false,
            &HashMap::new(),
            None,
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
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[document, page],
            &[],
            &[],
            &[],
            false,
            false,
            // No archive: nothing to point into.
            &HashMap::new(),
            None,
        )
        .expect("exports");
        assert!(export.gedcom.contains("C:\\Photos\\original.jpg"));
    }
}
