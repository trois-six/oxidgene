//! GEDCOM → OxidGene domain model import.
//!
//! Parses a GEDCOM string and converts it into OxidGene domain model entities.
//! Tracks xref → UUID mappings so that cross-references between GEDCOM records
//! are correctly translated into foreign-key relationships.

use std::collections::HashMap;

use chrono::Utc;
use ged_io::GedcomBuilder;
use ged_io::types::GedcomData;
use ged_io::types::event::Event as GedEvent;
use ged_io::types::source::citation::CitationSource;
use uuid::Uuid;

use oxidgene_core::enums::SourceMediaType;
use oxidgene_core::types::{
    Citation, Event, EventWitness, Family, FamilyChild, FamilySpouse, Media, MediaLink, Note,
    Person, PersonName, Place, Source, normalize_mime, split_surname_particle, split_surname_with,
};
use oxidgene_core::{ChildType, Confidence, EventType, NameType, Privacy, Sex, SpouseRole};

use crate::ImportResult;

/// Import a GEDCOM string into OxidGene domain model entities.
///
/// All entities are assigned to the given `tree_id`.
///
/// # Errors
///
/// Returns `Err` if the GEDCOM string cannot be parsed.
pub fn import_gedcom(gedcom_str: &str, tree_id: Uuid) -> Result<ImportResult, String> {
    let data = GedcomBuilder::new()
        .build_from_str(gedcom_str)
        .map_err(|e| format!("GEDCOM parse error: {e}"))?;

    import_gedcom_data(&data, tree_id)
}

/// Import an already-parsed GEDCOM model into OxidGene domain model entities.
///
/// This is the conversion half of [`import_gedcom`], exposed separately so that
/// other readers producing a [`GedcomData`] — notably the GeneWeb `.gw` reader
/// in [`crate::geneweb`] — reuse the exact same mapping.
///
/// All entities are assigned to the given `tree_id`.
///
/// # Errors
///
/// Returns `Err` if the model cannot be converted.
pub fn import_gedcom_data(data: &GedcomData, tree_id: Uuid) -> Result<ImportResult, String> {
    let now = Utc::now();
    let mut result = ImportResult::default();

    // ── xref → UUID maps ────────────────────────────────────────────
    let mut indi_map: HashMap<String, Uuid> = HashMap::new();
    let mut fam_map: HashMap<String, Uuid> = HashMap::new();
    let mut source_map: HashMap<String, Uuid> = HashMap::new();
    let mut media_map: HashMap<String, Uuid> = HashMap::new();
    // Place name → UUID (dedup by exact name match)
    let mut place_map: HashMap<String, Uuid> = HashMap::new();
    // Free-text SOUR description → UUID of a synthesized Source (dedup by
    // exact text match) — see `get_or_create_text_source` below.
    let mut text_source_map: HashMap<String, Uuid> = HashMap::new();

    // ── Pass 1: Allocate UUIDs for all top-level records ────────────
    for indi in &data.individuals {
        if let Some(xref) = &indi.xref {
            indi_map.insert(xref.clone(), Uuid::now_v7());
        }
    }
    for fam in &data.families {
        if let Some(xref) = &fam.xref {
            fam_map.insert(xref.clone(), Uuid::now_v7());
        }
    }
    for src in &data.sources {
        if let Some(xref) = &src.xref {
            source_map.insert(xref.clone(), Uuid::now_v7());
        }
    }
    for mm in &data.multimedia {
        if let Some(xref) = &mm.xref {
            media_map.insert(xref.clone(), Uuid::now_v7());
        }
    }

    // ── Helper: get or create a Place by name ───────────────────────
    let mut get_or_create_place = |name: &str, result: &mut ImportResult| -> Uuid {
        if let Some(&id) = place_map.get(name) {
            return id;
        }
        let id = Uuid::now_v7();
        place_map.insert(name.to_string(), id);
        result.places.push(Place {
            id,
            tree_id,
            name: name.to_string(),
            latitude: None,
            longitude: None,
            created_at: now,
            updated_at: now,
        });
        id
    };

    // ── Helper: get or create a Source from a free-text SOUR description ──
    //
    // Some exporters write a free-text description
    // (an archive reference, a URL...) instead of a pointer to a structured
    // SOUR record — valid per the GEDCOM 5.5.1 SOURCE_CITATION grammar. We
    // synthesize a `Source` from that text so the citation survives import
    // instead of being dropped, deduplicating by exact text so citations
    // that repeat the same reference (e.g. several people in the same
    // parish register) share one `Source`.
    let mut get_or_create_text_source = |text: &str, result: &mut ImportResult| -> Uuid {
        if let Some(&id) = text_source_map.get(text) {
            return id;
        }
        let id = Uuid::now_v7();
        text_source_map.insert(text.to_string(), id);
        result.sources.push(Source {
            id,
            tree_id,
            title: text.to_string(),
            author: None,
            publisher: None,
            abbreviation: None,
            repository_name: None,
            created_at: now,
            updated_at: now,
            deleted_at: None,
        });
        id
    };

    // ── Import Sources ──────────────────────────────────────────────
    for src in &data.sources {
        let xref = match &src.xref {
            Some(x) => x,
            None => {
                result.warnings.push("Skipping source without xref".into());
                continue;
            }
        };
        let id = source_map[xref];
        result.sources.push(Source {
            id,
            tree_id,
            title: src.title.clone().unwrap_or_else(|| "Untitled".into()),
            author: src.author.clone(),
            publisher: src.publication_facts.clone(),
            abbreviation: src.abbreviation.clone(),
            repository_name: None, // repo_citations not directly mappable to a single name
            created_at: now,
            updated_at: now,
            deleted_at: None,
        });

        // Notes on the source
        for note in &src.notes {
            import_note(
                &note.value,
                tree_id,
                now,
                None,
                None,
                None,
                Some(id),
                &mut result,
            );
        }
    }

    // ── Import Multimedia ───────────────────────────────────────────
    for mm in &data.multimedia {
        let xref = match &mm.xref {
            Some(x) => x,
            None => {
                result
                    .warnings
                    .push("Skipping multimedia without xref".into());
                continue;
            }
        };
        let id = media_map[xref];

        // Extract file info from the multimedia record
        // GEDCOM's `FORM` is the "multimedia format", and what producers put
        // there is an extension (`jpeg`) when they put anything at all — the
        // sample exports carry `FORM application/octet-stream` or no FORM and
        // a URL ending `.jpg`. Taking it verbatim as a MIME type is how a
        // photograph that renders perfectly well in an `<img>` ends up
        // labelled OCTET-STREAM in the gallery beside it.
        //
        // `FORM.TYPE` is the separate question of what the thing physically
        // is — `PHOTO`, `MANUSCRIPT`, `TOMBSTONE`. A value outside GEDCOM's
        // enumeration is a producer writing its own vocabulary, and is kept
        // as `Other` rather than guessed at.
        let (file_path, mime_type, source_media_type) = if let Some(ref file_ref) = mm.file {
            let path = file_ref.value.clone().unwrap_or_default();
            let form = file_ref.form.as_ref();
            let declared = form.and_then(|f| f.value.as_deref());
            let mime = normalize_mime(declared, &path);
            let medium = form
                .and_then(|f| f.source_media_type.as_deref())
                .and_then(SourceMediaType::parse)
                .unwrap_or_default();
            (path, mime, medium)
        } else {
            (
                String::new(),
                "application/octet-stream".into(),
                SourceMediaType::default(),
            )
        };

        let file_name: String = file_path
            .rsplit('/')
            .next()
            .unwrap_or(&file_path)
            .to_string();

        result.media.push(Media {
            id,
            tree_id,
            file_name,
            mime_type,
            file_path,
            // A GEDCOM names a file; it does not carry one. Everything below
            // stays empty until the bytes are uploaded and attached.
            storage_key: None,
            sha256: None,
            thumbnail_key: None,
            width: None,
            height: None,
            page_count: 1,
            parent_media_id: None,
            page_index: 0,
            is_document: false,
            file_size: 0, // Unknown from GEDCOM
            title: mm.title.clone(),
            description: None,
            date_value: None,
            date_sort: None,
            date_qualifier: Default::default(),
            date_value2: None,
            calendar: Default::default(),
            source_media_type,
            // GEDCOM has no field for this; it stays unset until a user
            // classifies the record or a Geneanet import supplies one.
            document_category: None,
            place_id: None,
            created_at: now,
            updated_at: now,
            deleted_at: None,
        });
    }

    // ── Import Individuals ──────────────────────────────────────────
    for indi in &data.individuals {
        let xref = match &indi.xref {
            Some(x) => x,
            None => {
                result
                    .warnings
                    .push("Skipping individual without xref".into());
                continue;
            }
        };
        let person_id = indi_map[xref];

        // Sex
        let sex = indi
            .sex
            .as_ref()
            .map(|g| convert_gender(&g.value))
            .unwrap_or(Sex::Unknown);

        result.persons.push(Person {
            id: person_id,
            tree_id,
            sex,
            privacy: Privacy::default(),
            created_at: now,
            updated_at: now,
            deleted_at: None,
        });

        // Names (GEDCOM allows {0:M} NAME structures per individual; the
        // first is primary, the rest import as additional PersonNames).
        for (i, name) in indi.names.iter().enumerate() {
            let (person_name, aliases) = convert_name(name, person_id, i == 0, now);
            result.person_names.push(person_name);
            result.person_names.extend(aliases);
        }

        // Events
        for evt_detail in &indi.events {
            import_event_detail(
                evt_detail,
                tree_id,
                Some(person_id),
                None,
                now,
                &source_map,
                &media_map,
                &indi_map,
                &mut get_or_create_place,
                &mut get_or_create_text_source,
                &mut result,
            );
        }

        // Attributes (GEDCOM's INDIVIDUAL_ATTRIBUTE_STRUCTURE: OCCU, RESI,
        // TITL, ... — distinct from INDIVIDUAL_EVENT_STRUCTURE in both the
        // 5.5.1 and 7.0 specs, but modeled as `Event`s in our domain).
        for attr_detail in &indi.attributes {
            import_attribute_detail(
                attr_detail,
                tree_id,
                person_id,
                now,
                &source_map,
                &mut get_or_create_place,
                &mut get_or_create_text_source,
                &mut result,
            );
        }

        // Source citations on the individual
        for cite in &indi.source {
            import_citation(
                cite,
                Some(person_id),
                None,
                None,
                &source_map,
                &mut get_or_create_text_source,
                &mut result,
            );
        }

        // Note on the individual
        if let Some(ref note) = indi.note {
            import_note(
                &note.value,
                tree_id,
                now,
                Some(person_id),
                None,
                None,
                None,
                &mut result,
            );
        }

        // Multimedia links on the individual
        for mm in &indi.multimedia {
            let media_id = resolve_or_create_media(mm, tree_id, now, &media_map, &mut result);
            if let Some(media_id) = media_id {
                result.media_links.push(MediaLink {
                    id: Uuid::now_v7(),
                    media_id,
                    person_id: Some(person_id),
                    event_id: None,
                    source_id: None,
                    family_id: None,
                    sort_order: 0,
                    is_profile: false,
                });
            }
        }
    }

    // ── Import Families ─────────────────────────────────────────────
    for fam in &data.families {
        let xref = match &fam.xref {
            Some(x) => x,
            None => {
                result.warnings.push("Skipping family without xref".into());
                continue;
            }
        };
        let family_id = fam_map[xref];

        result.families.push(Family {
            id: family_id,
            tree_id,
            created_at: now,
            updated_at: now,
            deleted_at: None,
        });

        // Spouses
        let mut sort_order = 0i32;
        if let Some(ref husb_xref) = fam.individual1 {
            if let Some(&person_id) = indi_map.get(husb_xref) {
                result.family_spouses.push(FamilySpouse {
                    id: Uuid::now_v7(),
                    family_id,
                    person_id,
                    role: SpouseRole::Husband,
                    sort_order,
                });
                sort_order += 1;
            } else {
                result
                    .warnings
                    .push(format!("Family {xref}: HUSB {husb_xref} not found"));
            }
        }
        if let Some(ref wife_xref) = fam.individual2 {
            if let Some(&person_id) = indi_map.get(wife_xref) {
                result.family_spouses.push(FamilySpouse {
                    id: Uuid::now_v7(),
                    family_id,
                    person_id,
                    role: SpouseRole::Wife,
                    sort_order,
                });
            } else {
                result
                    .warnings
                    .push(format!("Family {xref}: WIFE {wife_xref} not found"));
            }
        }

        // Children
        for (idx, child_xref) in fam.children.iter().enumerate() {
            if let Some(&person_id) = indi_map.get(child_xref) {
                result.family_children.push(FamilyChild {
                    id: Uuid::now_v7(),
                    family_id,
                    person_id,
                    child_type: ChildType::Biological, // default; PEDI tag handled below
                    sort_order: idx as i32,
                });
            } else {
                result
                    .warnings
                    .push(format!("Family {xref}: CHIL {child_xref} not found"));
            }
        }

        // Family events
        for evt_detail in &fam.events {
            import_event_detail(
                evt_detail,
                tree_id,
                None,
                Some(family_id),
                now,
                &source_map,
                &media_map,
                &indi_map,
                &mut get_or_create_place,
                &mut get_or_create_text_source,
                &mut result,
            );
        }
        // Some GEDCOM files put family events in family_event field
        for evt_detail in &fam.family_event {
            import_event_detail(
                evt_detail,
                tree_id,
                None,
                Some(family_id),
                now,
                &source_map,
                &media_map,
                &indi_map,
                &mut get_or_create_place,
                &mut get_or_create_text_source,
                &mut result,
            );
        }

        // Source citations on the family
        for cite in &fam.sources {
            import_citation(
                cite,
                None,
                None,
                Some(family_id),
                &source_map,
                &mut get_or_create_text_source,
                &mut result,
            );
        }

        // Notes on the family
        for note in &fam.notes {
            import_note(
                &note.value,
                tree_id,
                now,
                None,
                None,
                Some(family_id),
                None,
                &mut result,
            );
        }

        // Multimedia links on the family
        for mm in &fam.multimedia {
            let media_id = resolve_or_create_media(mm, tree_id, now, &media_map, &mut result);
            if let Some(media_id) = media_id {
                result.media_links.push(MediaLink {
                    id: Uuid::now_v7(),
                    media_id,
                    person_id: None,
                    event_id: None,
                    source_id: None,
                    family_id: Some(family_id),
                    sort_order: 0,
                    is_profile: false,
                });
            }
        }
    }

    // ── Pedigree linkage (update child_type from FAMC PEDI) ─────────
    // The FamilyLink on each individual's families vec tells us
    // the pedigree type. We update the FamilyChild records.
    for indi in &data.individuals {
        let indi_xref = match &indi.xref {
            Some(x) => x,
            None => continue,
        };
        let person_id = match indi_map.get(indi_xref) {
            Some(&id) => id,
            None => continue,
        };

        for fl in &indi.families {
            if !matches!(
                fl.family_link_type,
                ged_io::types::individual::family_link::FamilyLinkType::Child
            ) {
                continue;
            }
            let fam_xref = &fl.xref;
            if fam_xref.is_empty() {
                continue;
            }
            if let Some(ref pedi) = fl.pedigree_linkage_type {
                let child_type = convert_pedigree(pedi);
                // Find and update the matching FamilyChild
                for fc in &mut result.family_children {
                    if fc.person_id == person_id
                        && let Some(&fam_id) = fam_map.get(fam_xref)
                        && fc.family_id == fam_id
                    {
                        fc.child_type = child_type;
                    }
                }
            }
        }
    }

    // ── Associations (top-level `1 ASSO` — Gramps' witness/godparent
    // convention) ─────────────────────────────────────────────────────
    // Gramps (and the GEDCOM 5.5.1 grammar) places `ASSO` as a direct
    // child of the INDI record that "owns" the relationship, never nested
    // inside a specific event. Which direction it describes depends on
    // what the xref resolves to:
    //   - target is a FAM: the owner witnessed that family's marriage
    //     (`1 ASSO @F1@` / `2 RELA witness` on the witness's own record).
    //   - target is an INDI: the target holds a role (godparent, ...) at
    //     the owner's own birth/baptism (`1 ASSO @I2@` / `2 RELA GODM` on
    //     the godchild's own record).
    // GEDCOM's ASSO has no way to name which specific event it applies to
    // when the owner has several candidate events, so that case is a
    // best-effort guess (flagged with a warning), not a guarantee.
    //
    // Some exporters (Gramps included) redundantly *also* nest an ASSO
    // inside the witnessed event's own detail (caught above by
    // `import_event_detail`'s `detail.associations` loop) for the same
    // fact this pass would otherwise reconstruct — skip any (event,
    // person) pair already present in `result.event_witnesses` so the
    // same witness isn't recorded twice.
    let mut seen_witnesses: std::collections::HashSet<(Uuid, Uuid)> = result
        .event_witnesses
        .iter()
        .map(|w| (w.event_id, w.person_id))
        .collect();
    let mut event_witness_sort: HashMap<Uuid, i32> = HashMap::new();
    for indi in &data.individuals {
        let owner_xref = match &indi.xref {
            Some(x) => x,
            None => continue,
        };
        let Some(&owner_person_id) = indi_map.get(owner_xref) else {
            continue;
        };

        for assoc in &indi.associations {
            if let Some(&family_id) = fam_map.get(&assoc.xref) {
                let target_event = result
                    .events
                    .iter()
                    .filter(|e| e.family_id == Some(family_id))
                    .find(|e| e.event_type == EventType::Marriage)
                    .or_else(|| {
                        result
                            .events
                            .iter()
                            .find(|e| e.family_id == Some(family_id))
                    });

                match target_event {
                    Some(evt) if seen_witnesses.contains(&(evt.id, owner_person_id)) => {}
                    Some(evt) => {
                        let event_id = evt.id;
                        seen_witnesses.insert((event_id, owner_person_id));
                        let sort_order = event_witness_sort.entry(event_id).or_insert(0);
                        result.event_witnesses.push(EventWitness {
                            id: Uuid::now_v7(),
                            event_id,
                            person_id: owner_person_id,
                            relation: assoc.relationship.clone(),
                            sort_order: *sort_order,
                        });
                        *sort_order += 1;
                    }
                    None => result.warnings.push(format!(
                        "Individual {owner_xref}: ASSO {} (family) has no event to attach the witness to — skipped",
                        assoc.xref
                    )),
                }
            } else if let Some(&role_holder_id) = indi_map.get(&assoc.xref) {
                let candidates: Vec<&Event> = result
                    .events
                    .iter()
                    .filter(|e| e.person_id == Some(owner_person_id))
                    .collect();
                // GEDCOM 5.5.1 puts `ASSO` on the individual, not on an event
                // — Gramps rejects the event-nested form — so the event has to
                // be inferred. Baptism first, because that is where a
                // godparent belongs and godparents are most of what this tag
                // carries; then birth, which stands in for a baptism nobody
                // recorded.
                let baptism = candidates
                    .iter()
                    .find(|e| e.event_type == EventType::Baptism)
                    .copied();
                let birth = candidates
                    .iter()
                    .find(|e| e.event_type == EventType::Birth)
                    .copied();
                let target_event = baptism.or(birth).or_else(|| candidates.first().copied());

                match target_event {
                    Some(evt) if seen_witnesses.contains(&(evt.id, role_holder_id)) => {}
                    Some(evt) => {
                        // Only when the choice was genuinely arbitrary. An
                        // earlier version warned whenever the person had more
                        // than one event, which fired even where a baptism had
                        // been found and the answer was simply right — so a
                        // clean import reported warnings nobody could act on,
                        // and the ones that mattered were lost among them.
                        if baptism.is_none() && birth.is_none() && candidates.len() > 1 {
                            result.warnings.push(format!(
                                "Individual {owner_xref}: ASSO {} attached to its {:?} event — \
                                 {owner_xref} has several events and none of them is a birth or \
                                 a baptism, so this is a guess",
                                assoc.xref, evt.event_type
                            ));
                        }
                        let event_id = evt.id;
                        seen_witnesses.insert((event_id, role_holder_id));
                        let sort_order = event_witness_sort.entry(event_id).or_insert(0);
                        result.event_witnesses.push(EventWitness {
                            id: Uuid::now_v7(),
                            event_id,
                            person_id: role_holder_id,
                            relation: assoc.relationship.clone(),
                            sort_order: *sort_order,
                        });
                        *sort_order += 1;
                    }
                    None => result.warnings.push(format!(
                        "Individual {owner_xref}: ASSO {} has no individual event to attach to — skipped",
                        assoc.xref
                    )),
                }
            } else {
                result.warnings.push(format!(
                    "Individual {owner_xref}: ASSO {} target not found in file — skipped",
                    assoc.xref
                ));
            }
        }
    }

    // Handed back so a caller holding links keyed by something outside the
    // domain model can resolve them to person ids — see the field's docs.
    result.person_by_xref = indi_map;

    Ok(result)
}

// ═══════════════════════════════════════════════════════════════════════
// Conversion helpers
// ═══════════════════════════════════════════════════════════════════════

fn convert_gender(g: &ged_io::types::individual::gender::GenderType) -> Sex {
    use ged_io::types::individual::gender::GenderType;
    match g {
        GenderType::Male => Sex::Male,
        GenderType::Female => Sex::Female,
        _ => Sex::Unknown,
    }
}

/// Converts a GEDCOM `NAME` structure into a `PersonName`, plus any surname
/// aliases packed into its `SURN` sub-tag (see `surname_aliases_from_surn`).
fn convert_name(
    name: &ged_io::types::individual::name::Name,
    person_id: Uuid,
    is_primary: bool,
    now: chrono::DateTime<Utc>,
) -> (PersonName, Vec<PersonName>) {
    let name_type = name
        .name_type
        .as_ref()
        .map(convert_name_type)
        .unwrap_or(NameType::Birth);

    // ged_io populates name.given / name.surname only when GIVN/SURN sub-tags
    // exist. Most GEDCOM files only have the full name on the NAME line
    // (e.g. "John /DOE/"), stored in name.value. The NAME line's surname
    // wins over SURN: some exporters (Geneanet) pack a comma-separated list
    // of surname aliases into SURN instead of matching NAME, so SURN can't
    // be trusted as the primary surname.
    let (parsed_given, parsed_surname) = parse_name_value(name.value.as_deref());
    let given_names = name.given.clone().or(parsed_given);
    let surname = parsed_surname.or_else(|| name.surname.clone());

    let (surname_prefix, surname) =
        split_import_surname(surname.as_deref(), name.surname_prefix.as_deref());

    // Compare SURN against the *root*, not the full surname: a file writing
    // `1 NAME Lois /de la Cruz/` + `2 SURN Cruz` is stating the same surname
    // twice, and matching on the full form would read the SURN as an alias.
    let aliases = surname_aliases_from_surn(name.surname.as_deref(), surname.as_deref())
        .into_iter()
        .map(|alias_surname| {
            // Aliases carry no SPFX of their own, so derive one.
            let (alias_prefix, alias_root) = split_import_surname(Some(&alias_surname), None);
            PersonName {
                id: Uuid::now_v7(),
                person_id,
                name_type: NameType::AlsoKnownAs,
                given_names: given_names.clone(),
                surname: alias_root,
                surname_prefix: alias_prefix,
                prefix: None,
                suffix: None,
                nickname: None,
                is_primary: false,
                sort_order: 0,
                created_at: now,
                updated_at: now,
            }
        })
        .collect();

    let person_name = PersonName {
        id: Uuid::now_v7(),
        person_id,
        name_type,
        given_names,
        surname,
        surname_prefix,
        prefix: name.prefix.clone(),
        suffix: name.suffix.clone(),
        nickname: name.nickname.clone(),
        is_primary,
        sort_order: 0,
        created_at: now,
        updated_at: now,
    };

    (person_name, aliases)
}

/// Separates an imported surname into `(particle, root)`.
///
/// The particle comes from the file's `SPFX` when it has one, and is derived
/// from the surname itself otherwise. The two need reconciling because GEDCOM
/// repeats the particle in both places: `1 NAME Lois /de la Cruz/` carries it
/// inside the slashes *and* in `2 SPFX de la`, so taking both at face value
/// would yield "de la de la Cruz".
fn split_import_surname(
    surname: Option<&str>,
    spfx: Option<&str>,
) -> (Option<String>, Option<String>) {
    let raw = surname.map(str::trim).filter(|s| !s.is_empty());
    let spfx = spfx.map(str::trim).filter(|s| !s.is_empty());

    let Some(raw) = raw else {
        return (spfx.map(str::to_string), None);
    };

    // An explicit SPFX overrides detection, exactly as a user correcting the
    // particle in the person form does — same helper, so the two agree.
    let (particle, root) = match spfx {
        Some(spfx) => split_surname_with(raw, spfx),
        None => split_surname_particle(raw),
    };
    (particle, Some(root))
}

/// Splits a `SURN` value on `,` and returns the parts that differ from the
/// resolved primary surname. Geneanet's exporter packs surname variants into
/// a single `SURN` tag (e.g. "LE NADEN,NADAM") instead of emitting one
/// `NAME`/`SURN` structure per variant; each distinct part becomes its own
/// "also known as" `PersonName` so the alternate spellings aren't lost.
fn surname_aliases_from_surn(surn: Option<&str>, primary_root: Option<&str>) -> Vec<String> {
    let Some(surn) = surn else {
        return Vec::new();
    };
    let primary = primary_root.unwrap_or_default().trim().to_lowercase();
    surn.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        // Compare roots, not raw strings: `1 NAME Lois /de la Cruz/` makes
        // ged_io fill SURN with the full "de la Cruz" while `primary_root` is
        // the split-off "Cruz". Comparing those verbatim would read the
        // person's own surname back as an alias of itself.
        .filter(|s| split_surname_particle(s).1.to_lowercase() != primary)
        .collect()
}

/// Parse given name and surname from a GEDCOM NAME value (e.g. "John /DOE/").
/// Surname is the text between `/` delimiters; given names are everything before the first `/`.
fn parse_name_value(value: Option<&str>) -> (Option<String>, Option<String>) {
    let Some(val) = value else {
        return (None, None);
    };

    let surname = val.find('/').and_then(|start| {
        val[start + 1..].find('/').and_then(|end| {
            let s = val[start + 1..start + 1 + end].trim();
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        })
    });

    let given = val.find('/').and_then(|slash| {
        let g = val[..slash].trim();
        if g.is_empty() {
            None
        } else {
            Some(g.to_string())
        }
    });

    (given, surname)
}

fn convert_name_type(nt: &ged_io::types::individual::name::NameType) -> NameType {
    use ged_io::types::individual::name::NameType as GedNameType;
    match nt {
        GedNameType::Birth => NameType::Birth,
        GedNameType::Married => NameType::Married,
        GedNameType::Maiden => NameType::Maiden,
        GedNameType::Religious => NameType::Religious,
        GedNameType::Aka => NameType::AlsoKnownAs,
        GedNameType::Immigrant | GedNameType::Professional => NameType::Other,
        GedNameType::Other(_) => NameType::Other,
    }
}

fn convert_event_type(evt: &GedEvent, type_text: Option<&str>) -> EventType {
    match evt {
        GedEvent::Birth => EventType::Birth,
        GedEvent::Death => EventType::Death,
        GedEvent::Baptism => EventType::Baptism,
        GedEvent::Burial => EventType::Burial,
        GedEvent::Cremation => EventType::Cremation,
        GedEvent::Graduation => EventType::Graduation,
        GedEvent::Immigration => EventType::Immigration,
        GedEvent::Emigration => EventType::Emigration,
        GedEvent::Naturalization => EventType::Naturalization,
        GedEvent::Census => EventType::Census,
        GedEvent::Residence => EventType::Residence,
        GedEvent::Retired => EventType::Retirement,
        GedEvent::Will => EventType::Will,
        GedEvent::Probate => EventType::Probate,
        GedEvent::Marriage => EventType::Marriage,
        GedEvent::Divorce => EventType::Divorce,
        GedEvent::Annulment => EventType::Annulment,
        GedEvent::Engagement => EventType::Engagement,
        GedEvent::MarriageBann => EventType::MarriageBann,
        GedEvent::MarriageContract => EventType::MarriageContract,
        GedEvent::MarriageLicense => EventType::MarriageLicense,
        GedEvent::MarriageSettlement => EventType::MarriageSettlement,
        GedEvent::Separated => EventType::Separation,
        GedEvent::DivorceFiled => EventType::DivorceFiled,
        GedEvent::Adoption => EventType::Adoption,
        GedEvent::Blessing => EventType::Blessing,
        GedEvent::Ordination => EventType::Ordination,
        GedEvent::Christening => EventType::Christening,
        GedEvent::AdultChristening => EventType::AdultChristening,
        // A generic `EVEN` carries its meaning in its free-text `TYPE`.
        GedEvent::Event => event_type_from_type_text(type_text).unwrap_or(EventType::Other),
        _ => EventType::Other,
    }
}

/// Reads a `TYPE` that is exactly a GEDCOM tag name, matched whole so that
/// "will" here is the tag and not the "will" inside another word.
///
/// Kept apart from the descriptive phrases [`event_type_from_type_text`] also
/// recognises, because these two kinds of `TYPE` deserve opposite treatment as
/// a description: see [`type_text_restates_event_type`]. `t` is expected
/// trimmed and lowercased.
fn event_type_from_gedcom_tag(t: &str) -> Option<EventType> {
    match t {
        "adop" => Some(EventType::Adoption),
        "bapm" => Some(EventType::Baptism),
        "barm" | "basm" => Some(EventType::BarBatMitzvah),
        "buri" => Some(EventType::Burial),
        "cast" => Some(EventType::CasteName),
        "cens" => Some(EventType::Census),
        "chra" => Some(EventType::AdultChristening),
        "conf" => Some(EventType::Confirmation),
        "crem" => Some(EventType::Cremation),
        "dscr" => Some(EventType::PhysicalDescription),
        "educ" => Some(EventType::Education),
        "emig" => Some(EventType::Emigration),
        "fcom" => Some(EventType::FirstCommunion),
        "grad" => Some(EventType::Graduation),
        "idno" => Some(EventType::NationalId),
        "immi" => Some(EventType::Immigration),
        "mili" => Some(EventType::MilitaryService),
        "nati" => Some(EventType::NationalOrigin),
        "natu" => Some(EventType::Naturalization),
        "nchi" => Some(EventType::ChildrenCount),
        "nmr" => Some(EventType::MarriagesCount),
        "occu" => Some(EventType::Occupation),
        "prob" => Some(EventType::Probate),
        "prop" => Some(EventType::Property),
        "reli" => Some(EventType::Religion),
        "resi" => Some(EventType::Residence),
        "reti" => Some(EventType::Retirement),
        "ssn" => Some(EventType::SocialSecurityNumber),
        "titl" => Some(EventType::NobilityTitle),
        "will" => Some(EventType::Will),
        _ => None,
    }
}

/// A `TYPE` that is nothing but a name of an event type, in whichever language
/// the exporter wrote it. Matched whole: these are fixed vocabularies, so
/// recognising them by substring would only add false hits, and a `TYPE` that
/// merely *contains* one ("Military service in Algeria") says more than the
/// name alone and is not one of these.
///
/// Two vocabularies feed it. `GENEWEB_LABELS` are the labels the `geneweb`
/// crate writes for events GEDCOM cannot express (see its
/// `src/gedcom/event.rs`); `RESTATED_TAGS` are the ordinary GEDCOM tags an
/// exporter chose to spell out in words instead of using.
fn type_name_phrase(t: &str) -> Option<EventType> {
    const GENEWEB_LABELS: &[(&str, EventType)] = &[
        ("accomplishment", EventType::Accomplishment),
        ("acquisition", EventType::Acquisition),
        ("membership", EventType::Membership),
        ("change name", EventType::ChangeName),
        ("circumcision", EventType::Circumcision),
        ("award", EventType::Award),
        ("military discharge", EventType::MilitaryDischarge),
        ("degree", EventType::Degree),
        ("distinction", EventType::Distinction),
        ("election", EventType::Election),
        ("excommunication", EventType::Excommunication),
        ("funeral", EventType::Funeral),
        ("hospitalization", EventType::Hospitalization),
        ("illness", EventType::Illness),
        ("passenger list", EventType::PassengerList),
        ("military distinction", EventType::MilitaryDistinction),
        ("military promotion", EventType::MilitaryPromotion),
        ("military mobilization", EventType::MilitaryMobilization),
        ("property sale", EventType::PropertySale),
        ("endl", EventType::Endowment),
        ("dotationlds", EventType::LdsDotation),
        ("slgc", EventType::SealingChild),
        ("slgs", EventType::SealingSpouse),
        ("scellent parent lds", EventType::SealingParent),
        ("family link lds", EventType::FamilyLinkLds),
        ("bapl", EventType::LdsBaptism),
        ("conl", EventType::LdsConfirmation),
        ("unmarried", EventType::NoMarriage),
        ("nomen", EventType::NoMention),
    ];
    // Deliberately absent: the civil-union wordings. "PACS", "Concubinage"
    // and "Cohabitation" all resolve to `CivilUnion`, so there the phrase is
    // the only thing telling one union from another and must survive as the
    // description.
    const RESTATED_TAGS: &[(&str, EventType)] = &[
        ("military service", EventType::MilitaryService),
        ("service militaire", EventType::MilitaryService),
        ("physical description", EventType::PhysicalDescription),
        ("description physique", EventType::PhysicalDescription),
        ("national origin", EventType::NationalOrigin),
        ("origine nationale", EventType::NationalOrigin),
        ("nationality", EventType::NationalOrigin),
        ("nationalité", EventType::NationalOrigin),
        ("national id", EventType::NationalId),
        ("identity number", EventType::NationalId),
        ("social security number", EventType::SocialSecurityNumber),
        (
            "numéro de sécurité sociale",
            EventType::SocialSecurityNumber,
        ),
        ("number of children", EventType::ChildrenCount),
        ("nombre d'enfants", EventType::ChildrenCount),
        ("number of marriages", EventType::MarriagesCount),
        ("nombre de mariages", EventType::MarriagesCount),
        ("nobility title", EventType::NobilityTitle),
        ("titre de noblesse", EventType::NobilityTitle),
        ("first communion", EventType::FirstCommunion),
        ("première communion", EventType::FirstCommunion),
        ("premiere communion", EventType::FirstCommunion),
        ("bar mitzvah", EventType::BarBatMitzvah),
        ("bat mitzvah", EventType::BarBatMitzvah),
        ("confirmation", EventType::Confirmation),
        ("naturalization", EventType::Naturalization),
        ("naturalisation", EventType::Naturalization),
        ("immigration", EventType::Immigration),
        ("emigration", EventType::Emigration),
        ("émigration", EventType::Emigration),
        ("graduation", EventType::Graduation),
        ("diplôme", EventType::Graduation),
        ("diplome", EventType::Graduation),
        ("occupation", EventType::Occupation),
        ("profession", EventType::Occupation),
        ("métier", EventType::Occupation),
        ("residence", EventType::Residence),
        ("résidence", EventType::Residence),
        ("domicile", EventType::Residence),
        ("retirement", EventType::Retirement),
        ("retraite", EventType::Retirement),
        ("property", EventType::Property),
        ("propriété", EventType::Property),
        ("possessions", EventType::Property),
        ("religion", EventType::Religion),
        ("education", EventType::Education),
        ("éducation", EventType::Education),
        ("caste", EventType::CasteName),
        ("caste name", EventType::CasteName),
        ("census", EventType::Census),
        ("recensement", EventType::Census),
        ("baptism", EventType::Baptism),
        ("baptême", EventType::Baptism),
        ("bapteme", EventType::Baptism),
        ("burial", EventType::Burial),
        ("inhumation", EventType::Burial),
        ("enterrement", EventType::Burial),
        ("cremation", EventType::Cremation),
        ("crémation", EventType::Cremation),
        ("probate", EventType::Probate),
        ("homologation", EventType::Probate),
        ("last will", EventType::Will),
        ("testament", EventType::Will),
        ("adoption", EventType::Adoption),
    ];
    GENEWEB_LABELS
        .iter()
        .chain(RESTATED_TAGS)
        .find(|(l, _)| *l == t)
        .map(|(_, et)| *et)
}

/// Whether a `TYPE` says nothing the resolved [`EventType`] does not already
/// say, and so should not also become the event's description.
///
/// Two shapes say nothing. A bare GEDCOM tag name — `2 TYPE EDUC` under an
/// event already typed `Education` is the same fact twice, in a spelling
/// nobody typed, which surfaced as a profession labelled "OCCU" in the person
/// form and as `1 EDUC EDUC` on the way back out. And the type's own name
/// spelled out — a `Military service` description beside a badge that already
/// reads « Service militaire » repeats it, and repeats it in the exporter's
/// language rather than the reader's.
///
/// A `TYPE` that says more is still kept: "PACS" and "Concubinage" both arrive
/// as `CivilUnion` and only the description tells them apart, and "Military
/// service in Algeria" carries a fact the type does not.
fn type_text_restates_event_type(type_text: &str, event_type: EventType) -> bool {
    let t = type_text.trim().to_lowercase();
    event_type_from_gedcom_tag(&t) == Some(event_type) || type_name_phrase(&t) == Some(event_type)
}

/// Reads a generic `EVEN`'s free-text `TYPE` sub-tag as an [`EventType`].
///
/// Exporters record anything GEDCOM has no tag for as `EVEN` plus a `TYPE`
/// describing it — a civil union, but also plain restatements of tags they
/// chose not to use ("PROP", "Military service"). Left unread, all of it
/// imported as [`EventType::Other`], so a whole shelf of events showed up
/// under one meaningless label.
///
/// The `TYPE` text is kept as the event's description only when it carries
/// more than the type does — see [`type_text_restates_event_type`].
fn event_type_from_type_text(type_text: Option<&str>) -> Option<EventType> {
    let t = type_text?.trim().to_lowercase();
    if t.is_empty() {
        return None;
    }

    if let Some(et) = event_type_from_gedcom_tag(&t) {
        return Some(et);
    }
    if let Some(et) = type_name_phrase(&t) {
        return Some(et);
    }

    // Otherwise a descriptive phrase, in English or French. Order matters: the
    // first match wins, so anything that is a substring of another entry comes
    // after it. Only phrases specific enough to be unambiguous appear — a bare
    // "title" or "will" would swallow "job title" and "Williams".
    const KEYWORDS: &[(&str, EventType)] = &[
        // Unmarried partnerships: GEDCOM has no tag, so this is always EVEN.
        ("civil union", EventType::CivilUnion),
        ("civil partnership", EventType::CivilUnion),
        ("domestic partnership", EventType::CivilUnion),
        ("registered partnership", EventType::CivilUnion),
        ("cohabitation", EventType::CivilUnion),
        ("common-law", EventType::CivilUnion),
        ("common law", EventType::CivilUnion),
        ("pacs", EventType::CivilUnion),
        ("union libre", EventType::CivilUnion),
        ("concubinage", EventType::CivilUnion),
        // Tags an exporter restated in words.
        ("military", EventType::MilitaryService),
        ("service militaire", EventType::MilitaryService),
        ("physical description", EventType::PhysicalDescription),
        ("description physique", EventType::PhysicalDescription),
        ("national origin", EventType::NationalOrigin),
        ("origine nationale", EventType::NationalOrigin),
        ("nationalit", EventType::NationalOrigin),
        ("national id", EventType::NationalId),
        ("identity number", EventType::NationalId),
        ("social security", EventType::SocialSecurityNumber),
        ("sécurité sociale", EventType::SocialSecurityNumber),
        ("securite sociale", EventType::SocialSecurityNumber),
        ("number of children", EventType::ChildrenCount),
        ("nombre d'enfants", EventType::ChildrenCount),
        ("number of marriages", EventType::MarriagesCount),
        ("nombre de mariages", EventType::MarriagesCount),
        ("nobility", EventType::NobilityTitle),
        ("noblesse", EventType::NobilityTitle),
        ("first communion", EventType::FirstCommunion),
        ("première communion", EventType::FirstCommunion),
        ("premiere communion", EventType::FirstCommunion),
        ("bar mitzvah", EventType::BarBatMitzvah),
        ("bat mitzvah", EventType::BarBatMitzvah),
        ("confirmation", EventType::Confirmation),
        ("naturalisation", EventType::Naturalization),
        ("naturalization", EventType::Naturalization),
        ("immigration", EventType::Immigration),
        ("emigration", EventType::Emigration),
        ("émigration", EventType::Emigration),
        ("graduation", EventType::Graduation),
        ("diplôme", EventType::Graduation),
        ("diplome", EventType::Graduation),
        ("occupation", EventType::Occupation),
        ("profession", EventType::Occupation),
        ("métier", EventType::Occupation),
        ("residence", EventType::Residence),
        ("résidence", EventType::Residence),
        ("domicile", EventType::Residence),
        ("retirement", EventType::Retirement),
        ("retraite", EventType::Retirement),
        ("property", EventType::Property),
        ("possessions", EventType::Property),
        ("propriété", EventType::Property),
        ("religion", EventType::Religion),
        ("religious", EventType::Religion),
        ("education", EventType::Education),
        ("éducation", EventType::Education),
        ("scholastic", EventType::Education),
        ("caste", EventType::CasteName),
        ("census", EventType::Census),
        ("recensement", EventType::Census),
        ("baptism", EventType::Baptism),
        ("baptême", EventType::Baptism),
        ("bapteme", EventType::Baptism),
        ("burial", EventType::Burial),
        ("inhumation", EventType::Burial),
        ("enterrement", EventType::Burial),
        ("cremation", EventType::Cremation),
        ("crémation", EventType::Cremation),
        ("incinération", EventType::Cremation),
        ("probate", EventType::Probate),
        ("homologation", EventType::Probate),
        ("last will", EventType::Will),
        ("testament", EventType::Will),
        ("adoption", EventType::Adoption),
    ];
    KEYWORDS
        .iter()
        .find(|(k, _)| t.contains(k))
        .map(|(_, et)| *et)
}

/// Maps a GEDCOM `INDIVIDUAL_ATTRIBUTE_STRUCTURE` tag (OCCU, RESI, TITL, ...)
/// to our domain `EventType`.
fn convert_individual_attribute(
    attr: &ged_io::types::individual::attribute::IndividualAttribute,
) -> EventType {
    use ged_io::types::individual::attribute::IndividualAttribute;
    match attr {
        IndividualAttribute::Occupation => EventType::Occupation,
        IndividualAttribute::ResidesAt => EventType::Residence,
        IndividualAttribute::CastName => EventType::CasteName,
        IndividualAttribute::PhysicalDescription => EventType::PhysicalDescription,
        IndividualAttribute::ScholasticAchievement => EventType::Education,
        IndividualAttribute::NationalIDNumber => EventType::NationalId,
        IndividualAttribute::NationalOrTribalOrigin => EventType::NationalOrigin,
        IndividualAttribute::CountOfChildren => EventType::ChildrenCount,
        IndividualAttribute::CountOfMarriages => EventType::MarriagesCount,
        IndividualAttribute::Possessions => EventType::Property,
        IndividualAttribute::ReligiousAffiliation => EventType::Religion,
        IndividualAttribute::SocialSecurityNumber => EventType::SocialSecurityNumber,
        IndividualAttribute::NobilityTypeTitle => EventType::NobilityTitle,
        IndividualAttribute::Fact => EventType::Fact,
    }
}

fn convert_pedigree(
    pedi: &ged_io::types::individual::family_link::pedigree::Pedigree,
) -> ChildType {
    use ged_io::types::individual::family_link::pedigree::Pedigree;
    match pedi {
        Pedigree::Birth => ChildType::Biological,
        Pedigree::Adopted => ChildType::Adopted,
        Pedigree::Foster => ChildType::Foster,
        Pedigree::Sealing => ChildType::Unknown,
    }
}

fn convert_quay(quay: Option<&ged_io::types::source::quay::CertaintyAssessment>) -> Confidence {
    use ged_io::types::source::quay::CertaintyAssessment;
    match quay {
        Some(CertaintyAssessment::Unreliable) => Confidence::VeryLow,
        Some(CertaintyAssessment::Questionable) => Confidence::Low,
        Some(CertaintyAssessment::Secondary) => Confidence::Medium,
        Some(CertaintyAssessment::Direct) => Confidence::High,
        Some(CertaintyAssessment::None) | None => Confidence::Medium,
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Import sub-record helpers
// ═══════════════════════════════════════════════════════════════════════

#[allow(clippy::too_many_arguments)]
fn import_event_detail(
    detail: &ged_io::types::event::detail::Detail,
    tree_id: Uuid,
    person_id: Option<Uuid>,
    family_id: Option<Uuid>,
    now: chrono::DateTime<Utc>,
    source_map: &HashMap<String, Uuid>,
    media_map: &HashMap<String, Uuid>,
    indi_map: &HashMap<String, Uuid>,
    get_or_create_place: &mut dyn FnMut(&str, &mut ImportResult) -> Uuid,
    get_or_create_text_source: &mut dyn FnMut(&str, &mut ImportResult) -> Uuid,
    result: &mut ImportResult,
) {
    let event_type = convert_event_type(&detail.event, detail.event_type.as_deref());

    // Date — split into calendar / qualifier / value(s), see `crate::date`.
    let date = detail
        .date
        .as_ref()
        .and_then(|d| d.value.as_deref())
        .map(crate::date::parse)
        .unwrap_or_default();

    // Place
    let place_id = detail.place.as_ref().and_then(|p| {
        p.value.as_ref().map(|name| {
            let pid = get_or_create_place(name, result);
            // Update lat/long if available
            if let Some(ref map) = p.map
                && let (Some(lat_str), Some(lon_str)) = (&map.latitude, &map.longitude)
                && let (Ok(lat), Ok(lon)) =
                    (parse_gedcom_coord(lat_str), parse_gedcom_coord(lon_str))
                && let Some(place) = result.places.iter_mut().find(|pl| pl.id == pid)
            {
                place.latitude = Some(lat);
                place.longitude = Some(lon);
            }
            pid
        })
    });

    let cause = detail.cause.clone();

    // The GEDCOM `TYPE` sub-tag classifies a generic `EVEN`/`FACT` event
    // (e.g. "PACS", "Concubinage") — preserve it as the description so the
    // original wording survives even when several TYPE values map to the
    // same EventType (see `is_civil_union_type`). Unless it only restates the
    // type it was just read as, which is not wording anyone chose.
    let description = detail
        .event_type
        .clone()
        .filter(|t| !type_text_restates_event_type(t, event_type));

    // An individual `ADOP` event may carry its own nested `FAMC`, pointing
    // at the adoptive family. It is NOT captured: `Event.family_id` is used
    // throughout the codebase (cache builder, REST, GraphQL, export) as the
    // discriminant for "this is a family-level event" whenever it's set,
    // regardless of `person_id` — reusing it here would make this
    // individual event masquerade as belonging to the adoptive family
    // everywhere. Capturing the adoptive family properly would need a
    // dedicated field (or a join table), not `family_id`.

    let event_id = Uuid::now_v7();
    result.events.push(Event {
        id: event_id,
        tree_id,
        event_type,
        date_value: date.value,
        date_sort: date.sort,
        date_qualifier: date.qualifier,
        date_value2: date.value2,
        calendar: date.calendar,
        cause,
        place_id,
        person_id,
        family_id,
        description,
        created_at: now,
        updated_at: now,
        deleted_at: None,
    });

    // Associations (witnesses, godparents, ...) attached to this event —
    // only associations pointing at a known INDI xref can be captured
    // (an `ASSO` may point at a FAM record per the GEDCOM grammar, which
    // has no home in `EventWitness`).
    for (idx, assoc) in detail.associations.iter().enumerate() {
        if let Some(&witness_person_id) = indi_map.get(&assoc.xref) {
            result.event_witnesses.push(EventWitness {
                id: Uuid::now_v7(),
                event_id,
                person_id: witness_person_id,
                relation: assoc.relationship.clone(),
                sort_order: idx as i32,
            });
        }
    }

    // Source citations on the event
    for cite in &detail.citations {
        import_citation(
            cite,
            None,
            Some(event_id),
            family_id,
            source_map,
            get_or_create_text_source,
            result,
        );
    }

    // Multimedia on the event
    for mm in &detail.multimedia {
        let mid = resolve_or_create_media(mm, tree_id, now, media_map, result);
        if let Some(media_id) = mid {
            result.media_links.push(MediaLink {
                id: Uuid::now_v7(),
                media_id,
                person_id: None,
                event_id: Some(event_id),
                source_id: None,
                family_id: None,
                sort_order: 0,
                is_profile: false,
            });
        }
    }

    // Note on the event
    if let Some(ref note) = detail.note {
        import_note(
            &note.value,
            tree_id,
            now,
            None,
            Some(event_id),
            None,
            None,
            result,
        );
    }
}

/// Imports a GEDCOM individual attribute (OCCU, RESI, TITL, ...) as one or
/// more `Event`s. Mirrors `import_event_detail`, adapted to
/// `AttributeDetail`'s shape: the tag's own value (e.g. "Presales" for OCCU)
/// or its TYPE sub-tag is preserved as the description, and there is no
/// multimedia sub-structure on attributes. OCCU is special-cased: its value
/// is split on common separators and case-normalized into one Occupation
/// event per profession (see `split_occupations`/`normalize_occupation_case`).
#[allow(clippy::too_many_arguments)]
fn import_attribute_detail(
    detail: &ged_io::types::individual::attribute::detail::AttributeDetail,
    tree_id: Uuid,
    person_id: Uuid,
    now: chrono::DateTime<Utc>,
    source_map: &HashMap<String, Uuid>,
    get_or_create_place: &mut dyn FnMut(&str, &mut ImportResult) -> Uuid,
    get_or_create_text_source: &mut dyn FnMut(&str, &mut ImportResult) -> Uuid,
    result: &mut ImportResult,
) {
    let event_type = convert_individual_attribute(&detail.attribute);

    // Date — split into calendar / qualifier / value(s), see `crate::date`.
    let date = detail
        .date
        .as_ref()
        .and_then(|d| d.value.as_deref())
        .map(crate::date::parse)
        .unwrap_or_default();

    // Place
    let place_id = detail.place.as_ref().and_then(|p| {
        p.value.as_ref().map(|name| {
            let pid = get_or_create_place(name, result);
            if let Some(ref map) = p.map
                && let (Some(lat_str), Some(lon_str)) = (&map.latitude, &map.longitude)
                && let (Ok(lat), Ok(lon)) =
                    (parse_gedcom_coord(lat_str), parse_gedcom_coord(lon_str))
                && let Some(place) = result.places.iter_mut().find(|pl| pl.id == pid)
            {
                place.latitude = Some(lat);
                place.longitude = Some(lon);
            }
            pid
        })
    });

    let cause = detail.cause.clone();

    // Preserve the tag's own value (e.g. "Acccount Manager", "Presales, Trainer"
    // for OCCU) or, failing that, its TYPE sub-tag — but not either of them
    // when all it says is the name of the tag it sits under.
    let description = detail
        .value
        .clone()
        .or_else(|| detail.attribute_type.clone())
        .filter(|t| !type_text_restates_event_type(t, event_type));

    // Some exporters (e.g. Geneanet) pack several professions into a single
    // OCCU value ("Presales, Trainer") because they only support one
    // profession field. Split those into distinct Occupation events so each
    // can be edited and sourced independently, and uppercase each one's
    // first letter (some exporters write them all lowercase) — the rest of
    // the string is left as written. Every other attribute tag keeps its
    // raw value verbatim.
    let descriptions: Vec<Option<String>> = match &description {
        Some(text) if event_type == EventType::Occupation => split_occupations(text)
            .into_iter()
            .map(|p| Some(normalize_occupation_case(&p)))
            .collect(),
        _ => vec![description],
    };

    for description in descriptions {
        let event_id = Uuid::now_v7();
        result.events.push(Event {
            id: event_id,
            tree_id,
            event_type,
            date_value: date.value.clone(),
            date_sort: date.sort,
            date_qualifier: date.qualifier,
            date_value2: date.value2.clone(),
            calendar: date.calendar,
            cause: cause.clone(),
            place_id,
            person_id: Some(person_id),
            family_id: None,
            description,
            created_at: now,
            updated_at: now,
            deleted_at: None,
        });

        // Source citations on the attribute
        for cite in &detail.sources {
            import_citation(
                cite,
                None,
                Some(event_id),
                None,
                source_map,
                get_or_create_text_source,
                result,
            );
        }

        // Note on the attribute
        if let Some(ref note) = detail.note {
            import_note(
                &note.value,
                tree_id,
                now,
                None,
                Some(event_id),
                None,
                None,
                result,
            );
        }
    }
}

/// Splits a free-text value on common list separators, trimming whitespace
/// and dropping empty segments. Used to break up multi-profession OCCU
/// values from exporters that only support a single profession field.
fn split_occupations(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Normalizes a profession's case to "first letter upper, rest as it was written"
fn normalize_occupation_case(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn import_citation(
    cite: &ged_io::types::source::citation::Citation,
    person_id: Option<Uuid>,
    event_id: Option<Uuid>,
    family_id: Option<Uuid>,
    source_map: &HashMap<String, Uuid>,
    get_or_create_text_source: &mut dyn FnMut(&str, &mut ImportResult) -> Uuid,
    result: &mut ImportResult,
) {
    let source_id = match &cite.source {
        CitationSource::Xref(xref) if xref.is_empty() => {
            result
                .warnings
                .push("Skipping citation without source xref".into());
            return;
        }
        CitationSource::Xref(xref) => match source_map.get(xref) {
            Some(&id) => id,
            None => {
                result
                    .warnings
                    .push(format!("Citation references unknown source {xref}"));
                return;
            }
        },
        CitationSource::Description(text) if text.is_empty() => {
            result
                .warnings
                .push("Skipping citation without source xref".into());
            return;
        }
        CitationSource::Description(text) => get_or_create_text_source(text, result),
    };

    let confidence = convert_quay(cite.certainty_assessment.as_ref());
    let page = cite.page.clone();
    let text = cite
        .data
        .as_ref()
        .and_then(|d| d.text.as_ref())
        .and_then(|t| t.value.clone());

    result.citations.push(Citation {
        id: Uuid::now_v7(),
        source_id,
        person_id,
        event_id,
        family_id,
        page,
        confidence,
        text,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    });
}

/// The marker the `geneweb` crate appends to an event's note text when it
/// converts `.gw` to GEDCOM.
///
/// GEDCOM has no tag for most of GeneWeb's event vocabulary, so those events
/// are emitted as a generic `EVEN` with a `TYPE`. To keep its own
/// GEDCOM → `.gw` direction reversible, the crate records which `.gw` tag the
/// event came from — but writes it *into the note's text*, as a trailing
/// `_GWTAG #educ` line, rather than as a GEDCOM custom sub-tag. Read back, it
/// is a line of machine bookkeeping sitting in the middle of what the user
/// wrote about the event.
const GENEWEB_EVENT_TAG_MARKER: &str = "_GWTAG";

/// Drops that marker from a note body.
///
/// Nothing is lost: a `.gw` tag GeneWeb itself defines (`#educ`, `#occu`, …)
/// has already become the `EventType` this event carries, and a user-defined
/// one is already the event's description, verbatim — the marker only ever
/// restates one of the two.
///
/// Deliberately not extended to the crate's other in-note marker,
/// `_GWDEATH`: that one carries a death reason ("died young", "presumed
/// dead", a cause label) which nothing else in the import captures, so
/// dropping it would lose the only copy.
fn strip_geneweb_event_marker(text: &str) -> String {
    if !text.contains(GENEWEB_EVENT_TAG_MARKER) {
        return text.to_string();
    }
    text.lines()
        .filter(|line| {
            let line = line.trim_start();
            !line
                .strip_prefix(GENEWEB_EVENT_TAG_MARKER)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

#[allow(clippy::too_many_arguments)]
fn import_note(
    value: &Option<String>,
    tree_id: Uuid,
    now: chrono::DateTime<Utc>,
    person_id: Option<Uuid>,
    event_id: Option<Uuid>,
    family_id: Option<Uuid>,
    source_id: Option<Uuid>,
    result: &mut ImportResult,
) {
    // A note that held nothing but the marker leaves no note at all, rather
    // than an empty row for the UI to render as a blank entry.
    let text = match value.as_deref().map(strip_geneweb_event_marker) {
        Some(t) if !t.is_empty() => t,
        _ => return,
    };

    result.notes.push(Note {
        id: Uuid::now_v7(),
        tree_id,
        text,
        person_id,
        event_id,
        family_id,
        source_id,
        // GEDCOM attaches a NOTE to a record, never to an OBJE's bytes.
        media_id: None,
        created_at: now,
        updated_at: now,
        deleted_at: None,
    });
}

/// Resolve a multimedia reference to a `Media` UUID.
///
/// If the multimedia has an xref that matches a top-level OBJE record, return
/// its UUID. Otherwise, if it has inline file data, create a new `Media` entry
/// and return its UUID. Returns `None` if neither case applies.
fn resolve_or_create_media(
    mm: &ged_io::types::multimedia::Multimedia,
    tree_id: Uuid,
    now: chrono::DateTime<Utc>,
    media_map: &HashMap<String, Uuid>,
    result: &mut ImportResult,
) -> Option<Uuid> {
    // Case 1: cross-reference to a top-level OBJE record
    if let Some(ref xref) = mm.xref
        && let Some(&media_id) = media_map.get(xref)
    {
        return Some(media_id);
    }

    // Case 2: inline multimedia with file data
    if let Some(ref file_ref) = mm.file {
        let file_path = file_ref.value.clone().unwrap_or_default();
        if file_path.is_empty() {
            return None;
        }
        // Same as the top-level OBJE path above: the FORM is an extension at
        // best, and absent at worst, so the file name is the better evidence.
        let mime_type = normalize_mime(
            file_ref.form.as_ref().and_then(|f| f.value.as_deref()),
            &file_path,
        );
        let source_media_type = file_ref
            .form
            .as_ref()
            .and_then(|f| f.source_media_type.as_deref())
            .and_then(SourceMediaType::parse)
            .unwrap_or_default();
        let file_name = file_path
            .rsplit('/')
            .next()
            .unwrap_or(&file_path)
            .to_string();

        let id = Uuid::now_v7();
        result.media.push(Media {
            id,
            tree_id,
            file_name,
            mime_type,
            file_path,
            storage_key: None,
            sha256: None,
            thumbnail_key: None,
            width: None,
            height: None,
            page_count: 1,
            parent_media_id: None,
            page_index: 0,
            is_document: false,
            file_size: 0,
            title: mm.title.clone(),
            description: None,
            date_value: None,
            date_sort: None,
            date_qualifier: Default::default(),
            date_value2: None,
            calendar: Default::default(),
            source_media_type,
            document_category: None,
            place_id: None,
            created_at: now,
            updated_at: now,
            deleted_at: None,
        });
        return Some(id);
    }

    None
}

/// Parse a GEDCOM coordinate string (e.g. `"N01.4242"` or `"W1.4242"`).
fn parse_gedcom_coord(s: &str) -> Result<f64, std::num::ParseFloatError> {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix('N').or_else(|| s.strip_prefix('E')) {
        rest.parse::<f64>()
    } else if let Some(rest) = s.strip_prefix('S').or_else(|| s.strip_prefix('W')) {
        rest.parse::<f64>().map(|v| -v)
    } else {
        s.parse::<f64>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_name_value_full() {
        let (g, s) = parse_name_value(Some("John /DOE/"));
        assert_eq!(g, Some("John".to_string()));
        assert_eq!(s, Some("DOE".to_string()));
    }

    #[test]
    fn test_parse_name_value_multiple_given() {
        let (g, s) = parse_name_value(Some("John Mickael Louis /DOE/"));
        assert_eq!(g, Some("John Mickael Louis".to_string()));
        assert_eq!(s, Some("DOE".to_string()));
    }

    #[test]
    fn test_parse_name_value_surname_only() {
        let (g, s) = parse_name_value(Some("/Doe/"));
        assert_eq!(g, None);
        assert_eq!(s, Some("Doe".to_string()));
    }

    #[test]
    fn test_parse_name_value_no_slashes() {
        let (g, s) = parse_name_value(Some("John"));
        assert_eq!(g, None);
        assert_eq!(s, None);
    }

    #[test]
    fn test_parse_name_value_none() {
        let (g, s) = parse_name_value(None);
        assert_eq!(g, None);
        assert_eq!(s, None);
    }

    #[test]
    fn test_parse_name_value_empty_surname() {
        let (g, s) = parse_name_value(Some("John //"));
        assert_eq!(g, Some("John".to_string()));
        assert_eq!(s, None);
    }
}

#[cfg(test)]
mod type_text_description_tests {
    use super::*;

    #[test]
    fn a_bare_tag_name_is_not_kept_as_a_description() {
        // `geneweb` emits every event GEDCOM cannot express as `EVEN` + a
        // `TYPE` naming the tag it would have used. Read back, the type is
        // already `Education`, so keeping "EDUC" beside it said nothing and
        // showed up in the form as a bare "EDUC".
        assert!(type_text_restates_event_type("EDUC", EventType::Education));
        assert!(type_text_restates_event_type("OCCU", EventType::Occupation));
        // Case and surrounding space are the exporter's business, not a
        // difference in meaning.
        assert!(type_text_restates_event_type(
            "  prop ",
            EventType::Property
        ));
    }

    #[test]
    fn a_descriptive_type_is_kept() {
        // Several of these collapse onto one EventType, so the description is
        // the only thing telling them apart.
        assert!(!type_text_restates_event_type(
            "PACS",
            EventType::CivilUnion
        ));
        assert!(!type_text_restates_event_type(
            "Concubinage",
            EventType::CivilUnion
        ));
        // A phrase that says more than the type does.
        assert!(!type_text_restates_event_type(
            "Military service in Algeria",
            EventType::MilitaryService
        ));
        // A user-defined GeneWeb event name is the whole point of the field.
        assert!(!type_text_restates_event_type(
            "Succession",
            EventType::Other
        ));
    }

    /// A `TYPE` spelling out the very type it resolved to is the badge again,
    /// in the exporter's language — which is how a French tree ended up with
    /// an event badged « Service militaire » described "Military service".
    #[test]
    fn a_type_spelled_out_in_words_is_not_kept_as_a_description() {
        for (text, event_type) in [
            ("Military service", EventType::MilitaryService),
            ("service militaire", EventType::MilitaryService),
            ("  Occupation  ", EventType::Occupation),
            ("Recensement", EventType::Census),
            // Now that a sale has its own type, the phrase adds nothing.
            ("Property sale", EventType::PropertySale),
            ("Funeral", EventType::Funeral),
        ] {
            assert!(
                type_text_restates_event_type(text, event_type),
                "for {text}"
            );
        }
    }

    /// The rule is "names *this* type", not "names some type".
    #[test]
    fn a_phrase_naming_another_type_is_kept() {
        assert!(!type_text_restates_event_type(
            "Military service",
            EventType::Occupation
        ));
    }

    #[test]
    fn a_tag_naming_some_other_type_is_kept() {
        // The rule is "says what this event already is", not "looks like a
        // tag" — an OCCU sitting on a Residence still carries information.
        assert!(!type_text_restates_event_type("OCCU", EventType::Residence));
    }

    #[test]
    fn every_recognised_tag_round_trips_through_the_full_reader() {
        // `event_type_from_type_text` must keep answering for bare tags now
        // that the table lives in its own function.
        for (text, expected) in [
            ("EDUC", EventType::Education),
            ("occu", EventType::Occupation),
            ("WILL", EventType::Will),
            ("nmr", EventType::MarriagesCount),
        ] {
            assert_eq!(
                event_type_from_type_text(Some(text)),
                Some(expected),
                "for {text}"
            );
        }
    }
}

#[cfg(test)]
mod geneweb_marker_tests {
    use super::*;

    #[test]
    fn the_trailing_tag_marker_is_dropped() {
        // The reported shape: one line the user wrote, one line of the
        // converter's own bookkeeping.
        assert_eq!(
            strip_geneweb_event_marker("Institution Saint-Joseph\n_GWTAG #educ"),
            "Institution Saint-Joseph"
        );
    }

    #[test]
    fn a_note_that_was_only_the_marker_becomes_empty() {
        // Which stops `import_note` creating a row at all.
        assert_eq!(strip_geneweb_event_marker("_GWTAG #occu"), "");
    }

    #[test]
    fn a_user_defined_event_label_goes_with_it() {
        // GeneWeb lets an event be named freely, and the marker then repeats
        // that whole label — it is already the event's description.
        assert_eq!(
            strip_geneweb_event_marker(
                "Document non officiellement numérisé\n_GWTAG #Tutelle après un décès"
            ),
            "Document non officiellement numérisé"
        );
    }

    #[test]
    fn the_rest_of_a_multi_line_note_is_kept_intact() {
        assert_eq!(
            strip_geneweb_event_marker("Matricule: 559\nRéformé pour maladie\n_GWTAG #mser"),
            "Matricule: 559\nRéformé pour maladie"
        );
    }

    #[test]
    fn a_note_merely_mentioning_the_word_is_left_alone() {
        // Only a line that *is* the marker counts, so prose that happens to
        // name it — or a URL containing it — survives.
        for text in [
            "The exporter writes _GWTAG lines into notes.",
            "_GWTAGGED is not the marker",
            "https://example.org/_GWTAG",
        ] {
            assert_eq!(strip_geneweb_event_marker(text), text);
        }
    }

    #[test]
    fn the_death_reason_marker_is_not_touched() {
        // `_GWDEATH` is the only copy of that information — see the doc on
        // `strip_geneweb_event_marker`.
        let text = "Mort au combat\n_GWDEATH killed";
        assert_eq!(strip_geneweb_event_marker(text), text);
    }
}

#[cfg(test)]
mod event_type_text_tests {
    use super::*;

    #[test]
    fn a_bare_gedcom_tag_names_its_own_type() {
        for (text, expected) in [
            ("PROP", EventType::Property),
            ("prop", EventType::Property),
            ("  MILI  ", EventType::MilitaryService),
            ("OCCU", EventType::Occupation),
            ("TITL", EventType::NobilityTitle),
            ("WILL", EventType::Will),
        ] {
            assert_eq!(
                event_type_from_type_text(Some(text)),
                Some(expected),
                "for {text}"
            );
        }
    }

    #[test]
    fn a_described_type_is_recognised_in_either_language() {
        for (text, expected) in [
            ("Military service", EventType::MilitaryService),
            ("Service militaire", EventType::MilitaryService),
            // Its own type now that GeneWeb's vocabulary is modelled — a sale
            // is not merely a possession.
            ("Property sale", EventType::PropertySale),
            ("Vente de propriété", EventType::Property),
            ("Recensement", EventType::Census),
            ("Nombre d'enfants", EventType::ChildrenCount),
        ] {
            assert_eq!(
                event_type_from_type_text(Some(text)),
                Some(expected),
                "for {text}"
            );
        }
    }

    /// The behaviour this table replaced, kept working.
    #[test]
    fn civil_unions_still_win_over_everything_else() {
        for text in ["PACS", "Union libre", "Common law marriage", "Cohabitation"] {
            assert_eq!(
                event_type_from_type_text(Some(text)),
                Some(EventType::CivilUnion),
                "for {text}"
            );
        }
    }

    /// Guessing wrong is worse than not guessing: an unrecognised type must
    /// stay `Other` rather than be forced into a neighbouring meaning.
    #[test]
    fn an_unrecognised_type_is_not_guessed() {
        for text in ["", "   ", "Bought a horse", "Sold the farm", "Divers"] {
            assert_eq!(event_type_from_type_text(Some(text)), None, "for {text}");
        }
        assert_eq!(event_type_from_type_text(None), None);
    }

    /// Every label the `geneweb` crate emits must come back as its own type:
    /// this is the join between the two crates, and a typo on either side
    /// silently lands the event in `Other`.
    #[test]
    fn every_geneweb_label_is_recognised() {
        for (label, expected) in [
            ("Accomplishment", EventType::Accomplishment),
            ("Acquisition", EventType::Acquisition),
            ("Membership", EventType::Membership),
            ("Change name", EventType::ChangeName),
            ("Circumcision", EventType::Circumcision),
            ("Award", EventType::Award),
            ("Military discharge", EventType::MilitaryDischarge),
            ("Degree", EventType::Degree),
            ("Distinction", EventType::Distinction),
            ("Election", EventType::Election),
            ("Excommunication", EventType::Excommunication),
            ("Funeral", EventType::Funeral),
            ("Hospitalization", EventType::Hospitalization),
            ("Illness", EventType::Illness),
            ("Passenger list", EventType::PassengerList),
            ("Military distinction", EventType::MilitaryDistinction),
            ("Military promotion", EventType::MilitaryPromotion),
            ("Military mobilization", EventType::MilitaryMobilization),
            ("Property sale", EventType::PropertySale),
            ("BAPL", EventType::LdsBaptism),
            ("CONL", EventType::LdsConfirmation),
            ("ENDL", EventType::Endowment),
            ("DotationLDS", EventType::LdsDotation),
            ("SLGC", EventType::SealingChild),
            ("SLGS", EventType::SealingSpouse),
            ("Scellent parent LDS", EventType::SealingParent),
            ("Family link LDS", EventType::FamilyLinkLds),
            ("unmarried", EventType::NoMarriage),
            ("nomen", EventType::NoMention),
            ("OCCU", EventType::Occupation),
            ("PROP", EventType::Property),
        ] {
            assert_eq!(
                event_type_from_type_text(Some(label)),
                Some(expected),
                "geneweb label {label}"
            );
        }
    }

    /// Substrings that would misfire if the table used looser keywords.
    #[test]
    fn a_word_containing_a_tag_is_not_that_tag() {
        // "Williams" contains "will"; "job title" contains "title".
        assert_eq!(event_type_from_type_text(Some("Estate of Williams")), None);
        assert_eq!(event_type_from_type_text(Some("Job title change")), None);
    }
}
