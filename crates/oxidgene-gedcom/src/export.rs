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
use ged_io::types::header::meta::HeadMeta;
use ged_io::types::header::source::HeadSour;
use ged_io::types::individual::Individual;
use ged_io::types::individual::gender::{Gender, GenderType};
use ged_io::types::individual::name::{Name as GedName, NameType as GedNameType};
use ged_io::types::multimedia::Multimedia as GedMultimedia;
use ged_io::types::multimedia::file::Reference;
use ged_io::types::multimedia::format::Format;
use ged_io::types::note::Note as GedNote;
use ged_io::types::place::{MapCoordinates, Place as GedPlace};
use ged_io::types::source::Source as GedSource;
use ged_io::types::source::citation::Citation as GedCitation;
use ged_io::types::source::quay::CertaintyAssessment;
use uuid::Uuid;

use oxidgene_core::types::{
    Citation, Event, Family, FamilyChild, FamilySpouse, Media, MediaLink, Note, Person, PersonName,
    Place, Source,
};
use oxidgene_core::{Confidence, EventType, NameType, Sex, SpouseRole};

use crate::ExportResult;

/// Export domain model entities to a GEDCOM 5.5.1 string.
///
/// All entity slices should belong to the same tree.
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
    places: &[Place],
    sources: &[Source],
    citations: &[Citation],
    media: &[Media],
    media_links: &[MediaLink],
    notes: &[Note],
) -> Result<ExportResult, String> {
    let mut warnings: Vec<String> = Vec::new();

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

    let mut media_xref: HashMap<Uuid, String> = HashMap::new();
    for (i, m) in media.iter().enumerate() {
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

    // entity_id → citations
    let mut cites_by_person: HashMap<Uuid, Vec<&Citation>> = HashMap::new();
    let mut cites_by_event: HashMap<Uuid, Vec<&Citation>> = HashMap::new();
    for cite in citations {
        if let Some(pid) = cite.person_id {
            cites_by_person.entry(pid).or_default().push(cite);
        }
        if let Some(eid) = cite.event_id {
            cites_by_event.entry(eid).or_default().push(cite);
        }
    }

    // entity_id → notes
    let mut notes_by_person: HashMap<Uuid, Vec<&Note>> = HashMap::new();
    let mut notes_by_family: HashMap<Uuid, Vec<&Note>> = HashMap::new();
    let mut notes_by_source: HashMap<Uuid, Vec<&Note>> = HashMap::new();
    let mut notes_by_event: HashMap<Uuid, Vec<&Note>> = HashMap::new();
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
        encoding: None,
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
    for m in media {
        let xref = media_xref.get(&m.id).cloned();
        data.multimedia.push(GedMultimedia {
            xref,
            file: Some(Reference {
                value: Some(m.file_path.clone()),
                form: Some(Format {
                    value: Some(m.mime_type.clone()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            title: m.title.clone(),
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

        // Primary name
        let name = names_by_person.get(&person.id).and_then(|names| {
            // Prefer primary name
            names
                .iter()
                .find(|n| n.is_primary)
                .or(names.first())
                .map(|pn| to_ged_name(pn))
        });

        // Events
        let indi_events: Vec<GedDetail> = events_by_person
            .get(&person.id)
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
                            &mut warnings,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();

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

        // Multimedia links
        let multimedia: Vec<GedMultimedia> = mlinks_by_person
            .get(&person.id)
            .map(|mls| {
                mls.iter()
                    .filter_map(|ml| to_ged_multimedia_ref(ml.media_id, &media_by_id, &media_xref))
                    .collect()
            })
            .unwrap_or_default();

        data.individuals.push(Individual {
            xref,
            name,
            sex,
            events: indi_events,
            source: source_cites,
            note,
            multimedia,
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
                            &mut warnings,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Source citations on the family
        // (citations with family_id but no event_id)
        let fam_sources: Vec<GedCitation> = citations
            .iter()
            .filter(|c| c.family_id == Some(fam.id) && c.event_id.is_none())
            .filter_map(|c| to_ged_citation(c, &source_xref, &mut warnings))
            .collect();

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
                    .filter_map(|ml| to_ged_multimedia_ref(ml.media_id, &media_by_id, &media_xref))
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

    // ── Serialize ────────────────────────────────────────────────────
    let gedcom = GedcomWriter::new()
        .write_to_string(&data)
        .map_err(|e| format!("GEDCOM write error: {e}"))?;

    Ok(ExportResult { gedcom, warnings })
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

fn to_ged_name(pn: &PersonName) -> GedName {
    // Build the GEDCOM full name value: "Given /Surname/"
    let given_part = pn.given_names.as_deref().unwrap_or("");
    let surname_part = pn.surname.as_deref().unwrap_or("");
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
        NameType::Other => None,
    };

    GedName {
        value,
        given: pn.given_names.clone(),
        surname: pn.surname.clone(),
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
        EventType::Other | EventType::Occupation => GedEvent::Other,
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
    warnings: &mut Vec<String>,
) -> GedDetail {
    let event = convert_event_type(evt.event_type);
    let date = evt.date_value.as_ref().map(|dv| Date {
        value: Some(dv.clone()),
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
                .filter_map(|ml| to_ged_multimedia_ref(ml.media_id, media_by_id, media_xref))
                .collect()
        })
        .unwrap_or_default();

    GedDetail {
        event,
        value: None,
        date,
        place,
        note,
        family_link: None,
        family_event_details: Vec::new(),
        event_type: None,
        citations,
        multimedia,
        sort_date: None,
        associations: Vec::new(),
        cause: evt.description.clone(),
        restriction: None,
        age: None,
        agency: None,
        religion: None,
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
        xref,
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

fn to_ged_multimedia_ref(
    media_id: Uuid,
    media_by_id: &HashMap<Uuid, &Media>,
    media_xref: &HashMap<Uuid, String>,
) -> Option<GedMultimedia> {
    let xref = media_xref.get(&media_id)?.clone();
    // For inline references we only need the xref
    let _media = media_by_id.get(&media_id)?;
    Some(GedMultimedia {
        xref: Some(xref),
        ..Default::default()
    })
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
