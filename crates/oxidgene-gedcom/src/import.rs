//! GEDCOM → OxidGene domain model import.
//!
//! Parses a GEDCOM string and converts it into OxidGene domain model entities.
//! Tracks xref → UUID mappings so that cross-references between GEDCOM records
//! are correctly translated into foreign-key relationships.

use std::collections::HashMap;

use chrono::{NaiveDate, Utc};
use ged_io::GedcomBuilder;
use ged_io::types::event::Event as GedEvent;
use uuid::Uuid;

use oxidgene_core::types::{
    Citation, Event, Family, FamilyChild, FamilySpouse, Media, MediaLink, Note, Person,
    PersonAncestry, PersonName, Place, Source,
};
use oxidgene_core::{ChildType, Confidence, EventType, NameType, Sex, SpouseRole};

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

    let now = Utc::now();
    let mut result = ImportResult::default();

    // ── xref → UUID maps ────────────────────────────────────────────
    let mut indi_map: HashMap<String, Uuid> = HashMap::new();
    let mut fam_map: HashMap<String, Uuid> = HashMap::new();
    let mut source_map: HashMap<String, Uuid> = HashMap::new();
    let mut media_map: HashMap<String, Uuid> = HashMap::new();
    // Place name → UUID (dedup by exact name match)
    let mut place_map: HashMap<String, Uuid> = HashMap::new();

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
        let (file_path, mime_type) = if let Some(ref file_ref) = mm.file {
            let path = file_ref.value.clone().unwrap_or_default();
            let mime = file_ref
                .form
                .as_ref()
                .and_then(|f| f.value.clone())
                .unwrap_or_else(|| "application/octet-stream".into());
            (path, mime)
        } else {
            (String::new(), "application/octet-stream".into())
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
            file_size: 0, // Unknown from GEDCOM
            title: mm.title.clone(),
            description: None,
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
            created_at: now,
            updated_at: now,
            deleted_at: None,
        });

        // Names
        if let Some(ref name) = indi.name {
            let person_name = convert_name(name, person_id, true, now);
            result.person_names.push(person_name);
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
                &mut get_or_create_place,
                &mut result,
            );
        }

        // Source citations on the individual
        for cite in &indi.source {
            import_citation(
                cite,
                tree_id,
                Some(person_id),
                None,
                None,
                &source_map,
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
                &mut get_or_create_place,
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
                &mut get_or_create_place,
                &mut result,
            );
        }

        // Source citations on the family
        for cite in &fam.sources {
            import_citation(
                cite,
                tree_id,
                None,
                None,
                Some(family_id),
                &source_map,
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

    // ── Build PersonAncestry closure table ───────────────────────────
    result.person_ancestry =
        build_ancestry_closure(&result.family_spouses, &result.family_children, tree_id);

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

fn convert_name(
    name: &ged_io::types::individual::name::Name,
    person_id: Uuid,
    is_primary: bool,
    now: chrono::DateTime<Utc>,
) -> PersonName {
    // ged_io Name has: given, surname, prefix, suffix, nickname, name_type
    let name_type = name
        .name_type
        .as_ref()
        .map(convert_name_type)
        .unwrap_or(NameType::Birth);

    PersonName {
        id: Uuid::now_v7(),
        person_id,
        name_type,
        given_names: name.given.clone(),
        surname: name.surname.clone(),
        prefix: name.prefix.clone(),
        suffix: name.suffix.clone(),
        nickname: name.nickname.clone(),
        is_primary,
        created_at: now,
        updated_at: now,
    }
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

fn convert_event_type(evt: &GedEvent) -> EventType {
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
        _ => EventType::Other,
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

/// Try to parse a GEDCOM date string into a `NaiveDate` for sorting.
///
/// Handles common formats:
/// - `DD MMM YYYY` (e.g. `15 JAN 1842`)
/// - `MMM YYYY` (e.g. `JAN 1842`) → first of month
/// - `YYYY` (e.g. `1842`) → first of year
/// - Prefixes like `ABT`, `BEF`, `AFT`, `CAL`, `EST` are stripped
/// - Range formats `BET ... AND ...` → first date
fn parse_gedcom_date(s: &str) -> Option<NaiveDate> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    // Strip common prefixes
    let stripped = s
        .strip_prefix("ABT ")
        .or_else(|| s.strip_prefix("BEF "))
        .or_else(|| s.strip_prefix("AFT "))
        .or_else(|| s.strip_prefix("CAL "))
        .or_else(|| s.strip_prefix("EST "))
        .or_else(|| s.strip_prefix("FROM "))
        .or_else(|| s.strip_prefix("TO "))
        .unwrap_or(s);

    // Handle BET ... AND ... → take first date
    let stripped = if let Some(rest) = stripped.strip_prefix("BET ") {
        rest.split(" AND ").next().unwrap_or(rest)
    } else {
        stripped
    };

    let stripped = stripped.trim();
    let parts: Vec<&str> = stripped.split_whitespace().collect();

    match parts.len() {
        3 => {
            // DD MMM YYYY
            let day: u32 = parts[0].parse().ok()?;
            let month = gedcom_month(parts[1])?;
            let year: i32 = parts[2].parse().ok()?;
            NaiveDate::from_ymd_opt(year, month, day)
        }
        2 => {
            // MMM YYYY
            let month = gedcom_month(parts[0])?;
            let year: i32 = parts[1].parse().ok()?;
            NaiveDate::from_ymd_opt(year, month, 1)
        }
        1 => {
            // YYYY
            let year: i32 = parts[0].parse().ok()?;
            NaiveDate::from_ymd_opt(year, 1, 1)
        }
        _ => None,
    }
}

fn gedcom_month(s: &str) -> Option<u32> {
    match s.to_uppercase().as_str() {
        "JAN" => Some(1),
        "FEB" => Some(2),
        "MAR" => Some(3),
        "APR" => Some(4),
        "MAY" => Some(5),
        "JUN" => Some(6),
        "JUL" => Some(7),
        "AUG" => Some(8),
        "SEP" => Some(9),
        "OCT" => Some(10),
        "NOV" => Some(11),
        "DEC" => Some(12),
        _ => None,
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
    get_or_create_place: &mut dyn FnMut(&str, &mut ImportResult) -> Uuid,
    result: &mut ImportResult,
) {
    let event_type = convert_event_type(&detail.event);

    // Date
    let date_value = detail.date.as_ref().and_then(|d| d.value.clone());
    let date_sort = date_value.as_deref().and_then(parse_gedcom_date);

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

    let description = detail.cause.clone();

    let event_id = Uuid::now_v7();
    result.events.push(Event {
        id: event_id,
        tree_id,
        event_type,
        date_value,
        date_sort,
        place_id,
        person_id,
        family_id,
        description,
        created_at: now,
        updated_at: now,
        deleted_at: None,
    });

    // Source citations on the event
    for cite in &detail.citations {
        import_citation(
            cite,
            tree_id,
            None,
            Some(event_id),
            family_id,
            source_map,
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

fn import_citation(
    cite: &ged_io::types::source::citation::Citation,
    _tree_id: Uuid,
    person_id: Option<Uuid>,
    event_id: Option<Uuid>,
    family_id: Option<Uuid>,
    source_map: &HashMap<String, Uuid>,
    result: &mut ImportResult,
) {
    let source_id = if cite.xref.is_empty() {
        result
            .warnings
            .push("Skipping citation without source xref".into());
        return;
    } else {
        match source_map.get(&cite.xref) {
            Some(&id) => id,
            None => {
                result
                    .warnings
                    .push(format!("Citation references unknown source {}", cite.xref));
                return;
            }
        }
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
    let text = match value {
        Some(t) if !t.is_empty() => t.clone(),
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
        let mime_type = file_ref
            .form
            .as_ref()
            .and_then(|f| f.value.clone())
            .unwrap_or_else(|| "application/octet-stream".into());
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
            file_size: 0,
            title: mm.title.clone(),
            description: None,
            created_at: now,
            updated_at: now,
            deleted_at: None,
        });
        return Some(id);
    }

    None
}

/// Parse a GEDCOM coordinate string (e.g. `"N50.8333"` or `"W1.5833"`).
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

// ═══════════════════════════════════════════════════════════════════════
// Ancestry closure table builder
// ═══════════════════════════════════════════════════════════════════════

/// Build the `PersonAncestry` closure table from family relationships.
///
/// For each parent→child link (derived from `FamilySpouse` + `FamilyChild`),
/// we add depth-1 entries and then propagate transitively.
fn build_ancestry_closure(
    spouses: &[FamilySpouse],
    children: &[FamilyChild],
    tree_id: Uuid,
) -> Vec<PersonAncestry> {
    // Build parent → [child] map
    // A parent is anyone in FamilySpouse whose family also has FamilyChild entries.
    let mut family_parents: HashMap<Uuid, Vec<Uuid>> = HashMap::new(); // family_id -> [parent_person_id]
    for sp in spouses {
        family_parents
            .entry(sp.family_id)
            .or_default()
            .push(sp.person_id);
    }

    // Direct parent→child edges
    let mut parent_children: HashMap<Uuid, Vec<Uuid>> = HashMap::new(); // parent_id -> [child_id]
    for ch in children {
        if let Some(parents) = family_parents.get(&ch.family_id) {
            for &parent_id in parents {
                parent_children
                    .entry(parent_id)
                    .or_default()
                    .push(ch.person_id);
            }
        }
    }

    // BFS/DFS to build full closure
    let mut entries: Vec<PersonAncestry> = Vec::new();
    let mut seen: HashMap<(Uuid, Uuid), i32> = HashMap::new(); // (ancestor, descendant) -> depth

    // For every person who is a parent, traverse downward
    for (&parent_id, direct_children) in &parent_children {
        let mut stack: Vec<(Uuid, i32)> = Vec::new(); // (descendant_id, depth)
        for &child_id in direct_children {
            stack.push((child_id, 1));
        }

        while let Some((desc_id, depth)) = stack.pop() {
            let key = (parent_id, desc_id);
            if let Some(&existing_depth) = seen.get(&key)
                && existing_depth <= depth
            {
                continue; // already have a shorter/equal path
            }
            seen.insert(key, depth);

            // Continue traversal: desc_id's children are at depth+1
            if let Some(grandchildren) = parent_children.get(&desc_id) {
                for &gc_id in grandchildren {
                    stack.push((gc_id, depth + 1));
                }
            }
        }
    }

    for ((ancestor_id, descendant_id), depth) in &seen {
        entries.push(PersonAncestry {
            id: Uuid::now_v7(),
            tree_id,
            ancestor_id: *ancestor_id,
            descendant_id: *descendant_id,
            depth: *depth,
        });
    }

    entries
}
