//! OxidGene domain model → GEDCOM export.
//!
//! Converts domain model entities into a GEDCOM 5.5.1 string using `ged_io`.

use std::collections::HashMap;

use ged_io::GedcomWriter;
use ged_io::types::GedcomData;
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
use ged_io::types::source::quay::CertaintyAssessment;
use uuid::Uuid;

use oxidgene_core::enums::SourceMediaType;
use oxidgene_core::types::{
    Citation, Event, EventWitness, Family, FamilyChild, FamilySpouse, Media, MediaLink, Note,
    Person, PersonName, Place, Source, Vignette,
};
use oxidgene_core::{ChildType, Confidence, EventType, NameType, Sex, SpouseRole};

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

    // ── Build UUID → xref maps ──────────────────────────────────────
    let mut person_xref: HashMap<Uuid, String> = HashMap::new();
    for (i, p) in persons.iter().enumerate() {
        person_xref.insert(p.id, format!("@I{}@", i + 1));
    }

    let mut family_xref: HashMap<Uuid, String> = HashMap::new();
    for (i, f) in families.iter().enumerate() {
        family_xref.insert(f.id, format!("@F{}@", i + 1));
    }

    let mut source_xref: HashMap<Uuid, String> = HashMap::new();
    for (i, s) in sources.iter().enumerate() {
        source_xref.insert(s.id, format!("@S{}@", i + 1));
    }

    // Only pages become records: a document holds no bytes and is dissolved
    // into them. Numbering just the pages keeps the xrefs contiguous rather
    // than leaving a gap wherever a document sat in the list.
    let mut media_xref: HashMap<Uuid, String> = HashMap::new();
    for (i, m) in media.iter().filter(|m| !m.is_document()).enumerate() {
        media_xref.insert(m.id, format!("@M{}@", i + 1));
    }

    // ── Build lookup indexes ─────────────────────────────────────────
    let place_map: HashMap<Uuid, &Place> = places.iter().map(|p| (p.id, p)).collect();

    // person_id → names
    let mut names_by_person: HashMap<Uuid, Vec<&PersonName>> = HashMap::new();
    for pn in person_names {
        names_by_person.entry(pn.person_id).or_default().push(pn);
    }

    // entity_id → events
    let mut events_by_person: HashMap<Uuid, Vec<&Event>> = HashMap::new();
    let mut events_by_family: HashMap<Uuid, Vec<&Event>> = HashMap::new();
    for evt in events {
        if let Some(pid) = evt.person_id {
            events_by_person.entry(pid).or_default().push(evt);
        }
        if let Some(fid) = evt.family_id {
            events_by_family.entry(fid).or_default().push(evt);
        }
    }

    // event_id → witnesses
    let mut witnesses_by_event: HashMap<Uuid, Vec<&EventWitness>> = HashMap::new();
    for w in event_witnesses {
        witnesses_by_event.entry(w.event_id).or_default().push(w);
    }

    // GEDCOM only allows `ASSO` as a level-1 substructure of an INDI record
    // (GEDCOM 5.5.1 grammar; confirmed against real-world Gramps output and
    // rejected by Gramps as an unsupported tag when nested inside an event
    // detail). Which INDI record it's attached to, and what it points at,
    // depends on whether the witnessed event belongs to a person or a family:
    //   - family event (e.g. a marriage): attached to the *witness's* own
    //     INDI record, pointing at the family — mirrors how Gramps itself
    //     writes it (`1 ASSO @F1@` / `2 RELA witness`).
    //   - individual event (e.g. a burial): attached to the *event owner's*
    //     own INDI record, pointing at the witness
    //     (`1 ASSO @I2@` / `2 RELA witness`).
    // A person with several individual events sharing a witness can't
    // disambiguate which event on GEDCOM re-import (the format has no way
    // to nest ASSO under a specific event and still be portable) — this is
    // an inherent GEDCOM/Gramps limitation, not something round-tripped.
    let mut assoc_by_person: HashMap<Uuid, Vec<GedAssociation>> = HashMap::new();
    for evt in events {
        let Some(witnesses) = witnesses_by_event.get(&evt.id) else {
            continue;
        };
        for w in witnesses {
            let Some(witness_xref) = person_xref.get(&w.person_id) else {
                continue;
            };
            let (owner, target_xref) = if let Some(family_id) = evt.family_id {
                let Some(fam_xref) = family_xref.get(&family_id) else {
                    continue;
                };
                (w.person_id, fam_xref.clone())
            } else if let Some(person_id) = evt.person_id {
                (person_id, witness_xref.clone())
            } else {
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

    // entity_id → citations
    let mut cites_by_person: HashMap<Uuid, Vec<&Citation>> = HashMap::new();
    let mut cites_by_event: HashMap<Uuid, Vec<&Citation>> = HashMap::new();
    let mut cites_by_family: HashMap<Uuid, Vec<&Citation>> = HashMap::new();
    for cite in citations {
        if let Some(pid) = cite.person_id {
            cites_by_person.entry(pid).or_default().push(cite);
        }
        if let Some(eid) = cite.event_id {
            cites_by_event.entry(eid).or_default().push(cite);
        }
        if let Some(fid) = cite.family_id
            && cite.event_id.is_none()
        {
            cites_by_family.entry(fid).or_default().push(cite);
        }
    }

    // entity_id → notes
    let mut notes_by_person: HashMap<Uuid, Vec<&Note>> = HashMap::new();
    let mut notes_by_family: HashMap<Uuid, Vec<&Note>> = HashMap::new();
    let mut notes_by_source: HashMap<Uuid, Vec<&Note>> = HashMap::new();
    let mut notes_by_event: HashMap<Uuid, Vec<&Note>> = HashMap::new();
    let mut notes_by_media: HashMap<Uuid, Vec<&Note>> = HashMap::new();
    for note in notes {
        if let Some(pid) = note.person_id {
            notes_by_person.entry(pid).or_default().push(note);
        }
        if let Some(fid) = note.family_id {
            notes_by_family.entry(fid).or_default().push(note);
        }
        if let Some(sid) = note.source_id {
            notes_by_source.entry(sid).or_default().push(note);
        }
        if let Some(eid) = note.event_id {
            notes_by_event.entry(eid).or_default().push(note);
        }
        if let Some(mid) = note.media_id {
            notes_by_media.entry(mid).or_default().push(note);
        }
    }

    // entity_id → media links
    let mut mlinks_by_person: HashMap<Uuid, Vec<&MediaLink>> = HashMap::new();
    let mut mlinks_by_event: HashMap<Uuid, Vec<&MediaLink>> = HashMap::new();
    let mut mlinks_by_family: HashMap<Uuid, Vec<&MediaLink>> = HashMap::new();
    let media_by_id: HashMap<Uuid, &Media> = media.iter().map(|m| (m.id, m)).collect();
    for ml in media_links {
        if let Some(pid) = ml.person_id {
            mlinks_by_person.entry(pid).or_default().push(ml);
        }
        if let Some(eid) = ml.event_id {
            mlinks_by_event.entry(eid).or_default().push(ml);
        }
        if let Some(fid) = ml.family_id {
            mlinks_by_family.entry(fid).or_default().push(ml);
        }
    }

    // family_id → spouses / children
    let mut spouses_by_family: HashMap<Uuid, Vec<&FamilySpouse>> = HashMap::new();
    for fs in family_spouses {
        spouses_by_family.entry(fs.family_id).or_default().push(fs);
    }
    let mut children_by_family: HashMap<Uuid, Vec<&FamilyChild>> = HashMap::new();
    for fc in family_children {
        children_by_family.entry(fc.family_id).or_default().push(fc);
    }

    // person_id → families (for INDI-level FAMS/FAMC back-links, without
    // which the exported file has no individual↔family linkage at all —
    // most GEDCOM readers rely on FAMS/FAMC rather than cross-referencing
    // FAM's own HUSB/WIFE/CHIL back to individuals).
    let mut fams_by_person: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for fs in family_spouses {
        fams_by_person
            .entry(fs.person_id)
            .or_default()
            .push(fs.family_id);
    }
    let mut famc_by_person: HashMap<Uuid, Vec<(Uuid, ChildType)>> = HashMap::new();
    for fc in family_children {
        famc_by_person
            .entry(fc.person_id)
            .or_default()
            .push((fc.family_id, fc.child_type));
    }

    // ── Build GEDCOM Header ──────────────────────────────────────────
    let header = Header {
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
    };

    // ── Build GedcomData ─────────────────────────────────────────────
    let mut data = GedcomData {
        header: Some(header),
        ..Default::default()
    };

    // ── Export Sources ────────────────────────────────────────────────
    for src in sources {
        let xref = source_xref.get(&src.id).cloned();
        let ged_notes: Vec<GedNote> = notes_by_source
            .get(&src.id)
            .map(|ns| ns.iter().map(|n| to_ged_note(&n.text)).collect())
            .unwrap_or_default();

        data.sources.push(GedSource {
            xref,
            title: Some(src.title.clone()),
            author: src.author.clone(),
            publication_facts: src.publisher.clone(),
            abbreviation: src.abbreviation.clone(),
            notes: ged_notes,
            ..Default::default()
        });
    }

    // ── Export Multimedia ─────────────────────────────────────────────
    // A multi-page document is a container, and GEDCOM has no container at
    // all. Rather than fake one, the document dissolves: its pages are
    // exported as ordinary standalone media, and anything linked to the
    // document is linked to every one of them.
    //
    // The document's own row is not exported. It holds no bytes — its
    // `file_path` is its *title*, which is what made a GEDZIP warn about an
    // archive entry that could never exist — and writing it as its cover
    // instead only produced a duplicate of page one while leaving the other
    // thirty-seven attached to nobody.
    let mut pages_of: HashMap<Uuid, Vec<&Media>> = HashMap::new();
    for page in media.iter().filter(|m| m.parent_media_id.is_some()) {
        if let Some(parent) = page.parent_media_id {
            pages_of.entry(parent).or_default().push(page);
        }
    }
    for pages in pages_of.values_mut() {
        pages.sort_by_key(|page| (page.page_index, page.id));
    }

    for m in media {
        // Dissolved above; its pages carry the bytes.
        if m.is_document() {
            continue;
        }
        let xref = media_xref.get(&m.id).cloned();
        // Title, description, category and medium describe the document, not
        // the scan. Reading them from the parent is what keeps a `.ged` other
        // software opens from showing thirty-eight untitled files — and what
        // keeps a one-page document, which is what an ordinary photograph now
        // is, exporting under the title its owner gave it.
        let document = m.parent_media_id.and_then(|id| media_by_id.get(&id));
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
        data.multimedia.push(GedMultimedia {
            xref,
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
                document.map(|d| pages_of_len(&pages_of, d.id)).unwrap_or(1),
                m.page_index,
            ),
            note_structure: described.description.as_deref().map(to_ged_note),
            ..Default::default()
        });
    }

    // ── Export Individuals ────────────────────────────────────────────
    for person in persons {
        let xref = person_xref.get(&person.id).cloned();

        // Sex
        let sex = Some(Gender {
            value: convert_sex(person.sex),
            fact: None,
            sources: Vec::new(),
            custom_data: Vec::new(),
        });

        // Names (GEDCOM allows {0:M} NAME structures; primary goes first
        // so `names.first()` on the way back in matches what we exported).
        let mut names: Vec<GedName> = names_by_person
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

        // Events (GEDCOM INDIVIDUAL_EVENT_STRUCTURE) and attributes
        // (INDIVIDUAL_ATTRIBUTE_STRUCTURE, e.g. OCCU) — split so each
        // round-trips to its own tag rather than a generic EVEN.
        let mut indi_events: Vec<GedDetail> = Vec::new();
        let mut indi_attributes: Vec<GedAttributeDetail> = Vec::new();
        for evt in events_by_person.get(&person.id).into_iter().flatten() {
            match event_type_to_attribute(evt.event_type) {
                Some(attribute) => indi_attributes.push(to_ged_attribute_detail(
                    evt,
                    attribute,
                    &place_map,
                    &cites_by_event,
                    &notes_by_event,
                    &mlinks_by_event,
                    &media_by_id,
                    &source_xref,
                    &media_xref,
                    &pages_of,
                    &mut warnings,
                )),
                None => indi_events.push(to_ged_detail(
                    evt,
                    &place_map,
                    &cites_by_event,
                    &notes_by_event,
                    &mlinks_by_event,
                    &media_by_id,
                    &source_xref,
                    &media_xref,
                    &pages_of,
                    &mut warnings,
                )),
            }
        }
        if merge_occupations {
            indi_attributes = merge_occupation_attributes(indi_attributes);
        }

        // Source citations on the individual
        let source_cites: Vec<GedCitation> = cites_by_person
            .get(&person.id)
            .map(|cs| {
                cs.iter()
                    .filter_map(|c| to_ged_citation(c, &source_xref, &mut warnings))
                    .collect()
            })
            .unwrap_or_default();

        // Note on the individual (take the first one for GEDCOM 5.5.1)
        let note = notes_by_person
            .get(&person.id)
            .and_then(|ns| ns.first())
            .map(|n| to_ged_note(&n.text));

        // Multimedia links, portrait first.
        //
        // GEDCOM has no primary-photo flag, so the choice cannot be stated —
        // but it can be *implied*, because order survives and our own import
        // takes a person's first picture when no portrait is recorded. Writing
        // the portrait first is therefore what carries the choice across a
        // round trip; without it the person kept every photograph and came back
        // represented by whichever one happened to be written first.
        //
        // A crop portrait has no whole media to lead with, so those trees fall
        // back to the first picture as before: GEDCOM cannot express a region
        // of an image as somebody's portrait at all.
        let multimedia: Vec<GedMultimedia> = mlinks_by_person
            .get(&person.id)
            .map(|mls| {
                let mut ordered: Vec<&&MediaLink> = mls.iter().collect();
                ordered.sort_by_key(|ml| {
                    (
                        person.portrait_media_id != Some(ml.media_id),
                        ml.sort_order,
                        ml.id,
                    )
                });
                ordered
                    .into_iter()
                    .flat_map(|ml| {
                        to_ged_multimedia_refs(ml.media_id, &media_by_id, &media_xref, &pages_of)
                    })
                    .collect()
            })
            .unwrap_or_default();

        // FAMS/FAMC back-links to the families this person belongs to.
        let family_links = to_ged_family_links(
            person.id,
            &fams_by_person,
            &famc_by_person,
            &family_xref,
            &mut warnings,
        );

        data.individuals.push(Individual {
            xref,
            names,
            sex,
            families: family_links,
            events: indi_events,
            attributes: indi_attributes,
            source: source_cites,
            note,
            multimedia,
            associations: assoc_by_person.get(&person.id).cloned().unwrap_or_default(),
            ..Default::default()
        });
    }

    // ── Export Families ───────────────────────────────────────────────
    for fam in families {
        let xref = family_xref.get(&fam.id).cloned();

        // Find HUSB and WIFE
        let spouses = spouses_by_family.get(&fam.id);
        let individual1 = spouses.and_then(|ss| {
            ss.iter()
                .find(|s| s.role == SpouseRole::Husband)
                .and_then(|s| person_xref.get(&s.person_id).cloned())
        });
        let individual2 = spouses.and_then(|ss| {
            ss.iter()
                .find(|s| s.role == SpouseRole::Wife)
                .and_then(|s| person_xref.get(&s.person_id).cloned())
        });

        // Children
        let children_list: Vec<String> = children_by_family
            .get(&fam.id)
            .map(|cs| {
                let mut sorted: Vec<&&FamilyChild> = cs.iter().collect();
                sorted.sort_by_key(|fc| fc.sort_order);
                sorted
                    .iter()
                    .filter_map(|fc| person_xref.get(&fc.person_id).cloned())
                    .collect()
            })
            .unwrap_or_default();

        // Family events
        let fam_events: Vec<GedDetail> = events_by_family
            .get(&fam.id)
            .map(|evts| {
                evts.iter()
                    .map(|evt| {
                        to_ged_detail(
                            evt,
                            &place_map,
                            &cites_by_event,
                            &notes_by_event,
                            &mlinks_by_event,
                            &media_by_id,
                            &source_xref,
                            &media_xref,
                            &pages_of,
                            &mut warnings,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Source citations on the family
        // (citations with family_id but no event_id)
        let fam_sources: Vec<GedCitation> = cites_by_family
            .get(&fam.id)
            .map(|cs| {
                cs.iter()
                    .filter_map(|c| to_ged_citation(c, &source_xref, &mut warnings))
                    .collect()
            })
            .unwrap_or_default();

        // Notes on the family
        let fam_notes: Vec<GedNote> = notes_by_family
            .get(&fam.id)
            .map(|ns| ns.iter().map(|n| to_ged_note(&n.text)).collect())
            .unwrap_or_default();

        // Multimedia links
        let fam_multimedia: Vec<GedMultimedia> = mlinks_by_family
            .get(&fam.id)
            .map(|mls| {
                mls.iter()
                    .flat_map(|ml| {
                        to_ged_multimedia_refs(ml.media_id, &media_by_id, &media_xref, &pages_of)
                    })
                    .collect()
            })
            .unwrap_or_default();

        data.families.push(GedFamily {
            xref,
            individual1,
            individual2,
            children: children_list,
            events: fam_events,
            sources: fam_sources,
            notes: fam_notes,
            multimedia: fam_multimedia,
            ..Default::default()
        });
    }

    drop(build_guard);

    // ── Serialize ────────────────────────────────────────────────────
    let write_span =
        tracing::info_span!("export.write", export.output_bytes = tracing::field::Empty,);
    let gedcom = write_span
        .in_scope(|| GedcomWriter::new().write_to_string(&data))
        .map_err(|e| format!("GEDCOM write error: {e}"))?;
    write_span.record("export.output_bytes", gedcom.len());

    let extensions_span = tracing::info_span!(
        "export.inject_extensions",
        export.input_bytes = gedcom.len(),
        export.output_bytes = tracing::field::Empty,
        export.vignette_count = vignettes.len(),
    );
    let (gedcom, extension_warnings) = extensions_span.in_scope(|| {
        inject_oxidgene_media_extensions(
            gedcom,
            media,
            vignettes,
            &media_xref,
            &person_xref,
            &place_map,
            &notes_by_media,
        )
    });
    extensions_span.record("export.output_bytes", gedcom.len());
    warnings.extend(extension_warnings);

    Ok(ExportResult { gedcom, warnings })
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
    fams_by_person: &HashMap<Uuid, Vec<Uuid>>,
    famc_by_person: &HashMap<Uuid, Vec<(Uuid, ChildType)>>,
    family_xref: &HashMap<Uuid, String>,
    warnings: &mut Vec<String>,
) -> Vec<FamilyLink> {
    let mut links = Vec::new();

    for &family_id in fams_by_person.get(&person_id).into_iter().flatten() {
        let Some(xref) = family_xref.get(&family_id) else {
            warnings.push(format!(
                "Person {person_id}: spouse family {family_id} not found"
            ));
            continue;
        };
        links.push(FamilyLink {
            xref: xref.clone(),
            family_link_type: FamilyLinkType::Spouse,
            pedigree_linkage_type: None,
            child_linkage_status: None,
            adopted_by: None,
            note: None,
            custom_data: Vec::new(),
        });
    }

    for &(family_id, child_type) in famc_by_person.get(&person_id).into_iter().flatten() {
        let Some(xref) = family_xref.get(&family_id) else {
            warnings.push(format!(
                "Person {person_id}: parental family {family_id} not found"
            ));
            continue;
        };
        links.push(FamilyLink {
            xref: xref.clone(),
            family_link_type: FamilyLinkType::Child,
            pedigree_linkage_type: convert_child_type_to_pedigree(child_type),
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

fn convert_confidence(c: Confidence) -> CertaintyAssessment {
    match c {
        Confidence::VeryLow => CertaintyAssessment::Unreliable,
        Confidence::Low => CertaintyAssessment::Questionable,
        Confidence::Medium => CertaintyAssessment::Secondary,
        Confidence::High | Confidence::VeryHigh => CertaintyAssessment::Direct,
    }
}

fn to_ged_note(text: &str) -> GedNote {
    GedNote {
        value: Some(text.to_string()),
        ..Default::default()
    }
}

#[allow(clippy::too_many_arguments)]
fn to_ged_detail(
    evt: &Event,
    place_map: &HashMap<Uuid, &Place>,
    cites_by_event: &HashMap<Uuid, Vec<&Citation>>,
    notes_by_event: &HashMap<Uuid, Vec<&Note>>,
    mlinks_by_event: &HashMap<Uuid, Vec<&MediaLink>>,
    media_by_id: &HashMap<Uuid, &Media>,
    source_xref: &HashMap<Uuid, String>,
    media_xref: &HashMap<Uuid, String>,
    pages_of: &HashMap<Uuid, Vec<&Media>>,
    warnings: &mut Vec<String>,
) -> GedDetail {
    let event = convert_event_type(evt.event_type);
    // Recompose the calendar escape and qualifier tag the columns were split
    // from on import — see `crate::date`.
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

    let place = evt.place_id.and_then(|pid| {
        place_map.get(&pid).map(|p| {
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
        })
    });

    let citations: Vec<GedCitation> = cites_by_event
        .get(&evt.id)
        .map(|cs| {
            cs.iter()
                .filter_map(|c| to_ged_citation(c, source_xref, warnings))
                .collect()
        })
        .unwrap_or_default();

    let note = notes_by_event
        .get(&evt.id)
        .and_then(|ns| ns.first())
        .map(|n| to_ged_note(&n.text));

    let multimedia: Vec<GedMultimedia> = mlinks_by_event
        .get(&evt.id)
        .map(|mls| {
            mls.iter()
                .flat_map(|ml| {
                    to_ged_multimedia_refs(ml.media_id, media_by_id, media_xref, pages_of)
                })
                .collect()
        })
        .unwrap_or_default();

    // Witnesses/godparents are exported separately as a level-1 `ASSO` on
    // the relevant INDI record (see `assoc_by_person` in `export_gedcom`),
    // not nested here — GEDCOM 5.5.1 only allows `ASSO` directly under an
    // INDIVIDUAL_RECORD, and readers (Gramps included) reject it as a
    // substructure of an event.
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
        age: None,
        agency: None,
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
#[allow(clippy::too_many_arguments)]
fn to_ged_attribute_detail(
    evt: &Event,
    attribute: GedIndividualAttribute,
    place_map: &HashMap<Uuid, &Place>,
    cites_by_event: &HashMap<Uuid, Vec<&Citation>>,
    notes_by_event: &HashMap<Uuid, Vec<&Note>>,
    mlinks_by_event: &HashMap<Uuid, Vec<&MediaLink>>,
    media_by_id: &HashMap<Uuid, &Media>,
    source_xref: &HashMap<Uuid, String>,
    media_xref: &HashMap<Uuid, String>,
    pages_of: &HashMap<Uuid, Vec<&Media>>,
    warnings: &mut Vec<String>,
) -> GedAttributeDetail {
    // Recompose the calendar escape and qualifier tag the columns were split
    // from on import — see `crate::date`.
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

    let place = evt.place_id.and_then(|pid| {
        place_map.get(&pid).map(|p| {
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
        })
    });

    let sources: Vec<GedCitation> = cites_by_event
        .get(&evt.id)
        .map(|cs| {
            cs.iter()
                .filter_map(|c| to_ged_citation(c, source_xref, warnings))
                .collect()
        })
        .unwrap_or_default();

    let note = notes_by_event
        .get(&evt.id)
        .and_then(|ns| ns.first())
        .map(|n| to_ged_note(&n.text));

    // An attribute documents itself as readily as an event does: a scan of the
    // trade card behind an OCCU, of the deed behind a TITL. The same links, read
    // the same way as `to_ged_detail` reads them.
    let multimedia: Vec<GedMultimedia> = mlinks_by_event
        .get(&evt.id)
        .map(|mls| {
            mls.iter()
                .flat_map(|ml| {
                    to_ged_multimedia_refs(ml.media_id, media_by_id, media_xref, pages_of)
                })
                .collect()
        })
        .unwrap_or_default();

    GedAttributeDetail {
        attribute,
        // The attribute's own line value (e.g. "Account Manager" for OCCU) —
        // mirrors the import side, which reads this same field back from
        // `detail.value` first (falling back to the TYPE sub-tag).
        value: evt.description.clone(),
        place,
        date,
        sources,
        note,
        multimedia,
        attribute_type: None,
        restriction: None,
        age: None,
        address: None,
        cause: evt.cause.clone(),
        agency: None,
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
        data: None,
        note: None,
        certainty_assessment: Some(convert_confidence(cite.confidence)),
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
            confidence: Confidence::High,
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
        assert_eq!(imported.file_name, media.file_name);
        assert_eq!(imported.created_at, media.created_at);
        assert_eq!(imported.updated_at, media.updated_at);
        assert_eq!(imported.title, media.title);
        assert_eq!(imported.description, media.description);
        assert_eq!(imported.date_value, media.date_value);
        assert_eq!(imported.date_qualifier, media.date_qualifier);
        assert_eq!(imported.date_value2, media.date_value2);
        assert_eq!(imported.calendar, media.calendar);
        assert_eq!(imported.privacy, media.privacy);
        assert_eq!(imported.source_media_type, media.source_media_type);
        assert_eq!(imported.document_category, media.document_category);
        assert_eq!(imported.tags, media.tags);

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
        )
        .expect("exports");
        assert!(export.gedcom.contains("C:\\Photos\\original.jpg"));
    }
}
