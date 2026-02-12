//! Integration tests for GEDCOM import and export.

use uuid::Uuid;

use oxidgene_gedcom::export::export_gedcom;
use oxidgene_gedcom::import::import_gedcom;

/// Minimal GEDCOM 5.5.1 with one individual.
const MINIMAL_GEDCOM: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
2 FORM LINEAGE-LINKED
1 CHAR UTF-8
0 @I1@ INDI
1 NAME John /Doe/
2 GIVN John
2 SURN Doe
1 SEX M
1 BIRT
2 DATE 15 JAN 1842
2 PLAC London, England
1 DEAT
2 DATE 3 MAR 1910
2 PLAC Paris, France
0 TRLR
";

/// GEDCOM with two individuals and one family.
const FAMILY_GEDCOM: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
2 FORM LINEAGE-LINKED
1 CHAR UTF-8
0 @I1@ INDI
1 NAME John /Doe/
2 GIVN John
2 SURN Doe
1 SEX M
1 FAMS @F1@
0 @I2@ INDI
1 NAME Jane /Smith/
2 GIVN Jane
2 SURN Smith
1 SEX F
1 FAMS @F1@
0 @I3@ INDI
1 NAME Baby /Doe/
2 GIVN Baby
2 SURN Doe
1 SEX M
1 FAMC @F1@
0 @F1@ FAM
1 HUSB @I1@
1 WIFE @I2@
1 CHIL @I3@
1 MARR
2 DATE 5 JUN 1865
2 PLAC London, England
0 TRLR
";

/// GEDCOM with a source and citation.
const SOURCE_GEDCOM: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
0 @S1@ SOUR
1 TITL Parish Records of London
1 AUTH Church of England
1 PUBL Published in 1900
1 ABBR ParLon
0 @I1@ INDI
1 NAME John /Doe/
2 GIVN John
2 SURN Doe
1 SEX M
1 BIRT
2 DATE 15 JAN 1842
2 SOUR @S1@
3 PAGE p. 42
3 QUAY 3
0 TRLR
";

/// GEDCOM with multimedia.
const MULTIMEDIA_GEDCOM: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
0 @I1@ INDI
1 NAME John /Doe/
2 GIVN John
2 SURN Doe
1 SEX M
1 OBJE
2 FILE /photos/john_doe.jpg
3 FORM image/jpeg
2 TITL Portrait of John Doe
0 TRLR
";

// ═══════════════════════════════════════════════════════════════════════
// Import tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_import_minimal_individual() {
    let tree_id = Uuid::now_v7();
    let result = import_gedcom(MINIMAL_GEDCOM, tree_id).unwrap();

    assert_eq!(result.persons.len(), 1);
    assert_eq!(result.person_names.len(), 1);
    assert_eq!(result.events.len(), 2); // BIRT + DEAT
    assert_eq!(result.places.len(), 2); // London + Paris
    assert!(
        result.warnings.is_empty(),
        "warnings: {:?}",
        result.warnings
    );

    let person = &result.persons[0];
    assert_eq!(person.tree_id, tree_id);
    assert_eq!(person.sex, oxidgene_core::Sex::Male);

    let name = &result.person_names[0];
    assert_eq!(name.given_names.as_deref(), Some("John"));
    assert_eq!(name.surname.as_deref(), Some("Doe"));
    assert!(name.is_primary);

    // Check birth event
    let birth = result
        .events
        .iter()
        .find(|e| e.event_type == oxidgene_core::EventType::Birth)
        .expect("birth event missing");
    assert_eq!(birth.date_value.as_deref(), Some("15 JAN 1842"));
    assert!(birth.date_sort.is_some());
    assert!(birth.place_id.is_some());

    // Check death event
    let death = result
        .events
        .iter()
        .find(|e| e.event_type == oxidgene_core::EventType::Death)
        .expect("death event missing");
    assert_eq!(death.date_value.as_deref(), Some("3 MAR 1910"));

    // Check places
    let london = result
        .places
        .iter()
        .find(|p| p.name.contains("London"))
        .expect("London place missing");
    assert_eq!(london.tree_id, tree_id);
}

#[test]
fn test_import_family() {
    let tree_id = Uuid::now_v7();
    let result = import_gedcom(FAMILY_GEDCOM, tree_id).unwrap();

    assert_eq!(result.persons.len(), 3);
    assert_eq!(result.families.len(), 1);
    assert_eq!(result.family_spouses.len(), 2);
    assert_eq!(result.family_children.len(), 1);

    // Marriage event
    assert_eq!(result.events.len(), 1); // MARR
    let marr = &result.events[0];
    assert_eq!(marr.event_type, oxidgene_core::EventType::Marriage);
    assert_eq!(marr.date_value.as_deref(), Some("5 JUN 1865"));

    // Family spouses
    let husb = result
        .family_spouses
        .iter()
        .find(|s| s.role == oxidgene_core::SpouseRole::Husband)
        .expect("husband missing");
    let wife = result
        .family_spouses
        .iter()
        .find(|s| s.role == oxidgene_core::SpouseRole::Wife)
        .expect("wife missing");
    assert_ne!(husb.person_id, wife.person_id);

    // Family child
    let child = &result.family_children[0];
    assert_eq!(child.child_type, oxidgene_core::ChildType::Biological);
}

#[test]
fn test_import_ancestry_closure() {
    let tree_id = Uuid::now_v7();
    let result = import_gedcom(FAMILY_GEDCOM, tree_id).unwrap();

    // We have 3 persons: father, mother, child
    // Should have 2 ancestry entries: father→child(1), mother→child(1)
    assert_eq!(result.person_ancestry.len(), 2);

    for pa in &result.person_ancestry {
        assert_eq!(pa.depth, 1);
        assert_eq!(pa.tree_id, tree_id);
    }
}

#[test]
fn test_import_source_and_citation() {
    let tree_id = Uuid::now_v7();
    let result = import_gedcom(SOURCE_GEDCOM, tree_id).unwrap();

    assert_eq!(result.sources.len(), 1);
    let src = &result.sources[0];
    assert_eq!(src.title, "Parish Records of London");
    assert_eq!(src.author.as_deref(), Some("Church of England"));
    assert_eq!(src.publisher.as_deref(), Some("Published in 1900"));
    assert_eq!(src.abbreviation.as_deref(), Some("ParLon"));

    // Citation on the birth event
    assert_eq!(result.citations.len(), 1);
    let cite = &result.citations[0];
    assert_eq!(cite.source_id, src.id);
    assert_eq!(cite.page.as_deref(), Some("p. 42"));
    assert_eq!(cite.confidence, oxidgene_core::Confidence::High);
}

#[test]
fn test_import_multimedia() {
    let tree_id = Uuid::now_v7();
    let result = import_gedcom(MULTIMEDIA_GEDCOM, tree_id).unwrap();

    assert_eq!(result.media.len(), 1);
    let m = &result.media[0];
    assert_eq!(m.file_path, "/photos/john_doe.jpg");
    assert_eq!(m.file_name, "john_doe.jpg");
    assert_eq!(m.mime_type, "image/jpeg");
    assert_eq!(m.title.as_deref(), Some("Portrait of John Doe"));

    // Multimedia link on the individual
    assert_eq!(result.media_links.len(), 1);
    let ml = &result.media_links[0];
    assert_eq!(ml.media_id, m.id);
    assert!(ml.person_id.is_some());
}

#[test]
fn test_import_place_dedup() {
    // London appears twice (birth and marriage) — should be deduplicated
    let tree_id = Uuid::now_v7();
    let result = import_gedcom(FAMILY_GEDCOM, tree_id).unwrap();

    // Only one unique place (London, England)
    assert_eq!(result.places.len(), 1);
    assert!(result.places[0].name.contains("London"));
}

#[test]
fn test_import_invalid_gedcom() {
    let tree_id = Uuid::now_v7();
    // Empty string — should still succeed (empty data)
    let result = import_gedcom("", tree_id);
    // ged_io may or may not error on empty input; either is fine
    if let Ok(r) = result {
        assert_eq!(r.persons.len(), 0);
    }
}

#[test]
fn test_import_various_date_formats() {
    let gedcom = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
0 @I1@ INDI
1 NAME Test /Person/
1 SEX M
1 BIRT
2 DATE ABT 1842
1 DEAT
2 DATE BET 1 JAN 1900 AND 31 DEC 1910
0 TRLR
";
    let tree_id = Uuid::now_v7();
    let result = import_gedcom(gedcom, tree_id).unwrap();

    // Birth: ABT 1842 → should parse as 1842-01-01
    let birth = result
        .events
        .iter()
        .find(|e| e.event_type == oxidgene_core::EventType::Birth)
        .unwrap();
    assert!(birth.date_sort.is_some());
    assert_eq!(
        birth.date_sort.unwrap(),
        chrono::NaiveDate::from_ymd_opt(1842, 1, 1).unwrap()
    );

    // Death: BET ... AND ... → should parse first date
    let death = result
        .events
        .iter()
        .find(|e| e.event_type == oxidgene_core::EventType::Death)
        .unwrap();
    assert!(death.date_sort.is_some());
    assert_eq!(
        death.date_sort.unwrap(),
        chrono::NaiveDate::from_ymd_opt(1900, 1, 1).unwrap()
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Export tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_export_produces_valid_gedcom() {
    let tree_id = Uuid::now_v7();
    let result = import_gedcom(MINIMAL_GEDCOM, tree_id).unwrap();

    let export = export_gedcom(
        &result.persons,
        &result.person_names,
        &result.families,
        &result.family_spouses,
        &result.family_children,
        &result.events,
        &result.places,
        &result.sources,
        &result.citations,
        &result.media,
        &result.media_links,
        &result.notes,
    )
    .unwrap();

    // Should contain GEDCOM header
    assert!(export.gedcom.contains("HEAD"));
    assert!(export.gedcom.contains("GEDC"));
    assert!(export.gedcom.contains("TRLR"));

    // Should contain the individual
    assert!(export.gedcom.contains("INDI"));
    assert!(export.gedcom.contains("John"));
    assert!(export.gedcom.contains("Doe"));
}

#[test]
fn test_export_family() {
    let tree_id = Uuid::now_v7();
    let result = import_gedcom(FAMILY_GEDCOM, tree_id).unwrap();

    let export = export_gedcom(
        &result.persons,
        &result.person_names,
        &result.families,
        &result.family_spouses,
        &result.family_children,
        &result.events,
        &result.places,
        &result.sources,
        &result.citations,
        &result.media,
        &result.media_links,
        &result.notes,
    )
    .unwrap();

    assert!(export.gedcom.contains("FAM"));
    assert!(export.gedcom.contains("HUSB"));
    assert!(export.gedcom.contains("WIFE"));
    assert!(export.gedcom.contains("CHIL"));
    assert!(export.gedcom.contains("MARR"));
}

#[test]
fn test_export_source() {
    let tree_id = Uuid::now_v7();
    let result = import_gedcom(SOURCE_GEDCOM, tree_id).unwrap();

    let export = export_gedcom(
        &result.persons,
        &result.person_names,
        &result.families,
        &result.family_spouses,
        &result.family_children,
        &result.events,
        &result.places,
        &result.sources,
        &result.citations,
        &result.media,
        &result.media_links,
        &result.notes,
    )
    .unwrap();

    assert!(export.gedcom.contains("SOUR"));
    assert!(export.gedcom.contains("Parish Records of London"));
    assert!(export.gedcom.contains("Church of England"));
}

#[test]
fn test_export_empty() {
    let export = export_gedcom(&[], &[], &[], &[], &[], &[], &[], &[], &[], &[], &[], &[]).unwrap();

    // Should still produce a valid GEDCOM with header/trailer
    assert!(export.gedcom.contains("HEAD"));
    assert!(export.gedcom.contains("TRLR"));
    assert!(export.warnings.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// Round-trip tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_roundtrip_preserves_individuals() {
    let tree_id = Uuid::now_v7();
    let imported = import_gedcom(FAMILY_GEDCOM, tree_id).unwrap();

    let exported = export_gedcom(
        &imported.persons,
        &imported.person_names,
        &imported.families,
        &imported.family_spouses,
        &imported.family_children,
        &imported.events,
        &imported.places,
        &imported.sources,
        &imported.citations,
        &imported.media,
        &imported.media_links,
        &imported.notes,
    )
    .unwrap();

    // Re-import the exported GEDCOM
    let tree_id2 = Uuid::now_v7();
    let reimported = import_gedcom(&exported.gedcom, tree_id2).unwrap();

    // Same number of persons, families, events
    assert_eq!(
        reimported.persons.len(),
        imported.persons.len(),
        "person count mismatch"
    );
    assert_eq!(
        reimported.families.len(),
        imported.families.len(),
        "family count mismatch"
    );
    assert_eq!(
        reimported.family_spouses.len(),
        imported.family_spouses.len(),
        "spouse count mismatch"
    );
    assert_eq!(
        reimported.family_children.len(),
        imported.family_children.len(),
        "child count mismatch"
    );
}

#[test]
fn test_roundtrip_preserves_names() {
    let tree_id = Uuid::now_v7();
    let imported = import_gedcom(MINIMAL_GEDCOM, tree_id).unwrap();

    let exported = export_gedcom(
        &imported.persons,
        &imported.person_names,
        &imported.families,
        &imported.family_spouses,
        &imported.family_children,
        &imported.events,
        &imported.places,
        &imported.sources,
        &imported.citations,
        &imported.media,
        &imported.media_links,
        &imported.notes,
    )
    .unwrap();

    let tree_id2 = Uuid::now_v7();
    let reimported = import_gedcom(&exported.gedcom, tree_id2).unwrap();

    assert_eq!(reimported.person_names.len(), 1);
    let name = &reimported.person_names[0];
    assert_eq!(name.given_names.as_deref(), Some("John"));
    assert_eq!(name.surname.as_deref(), Some("Doe"));
}

// ═══════════════════════════════════════════════════════════════════════
// Serialization tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_import_result_serialization() {
    let tree_id = Uuid::now_v7();
    let result = import_gedcom(MINIMAL_GEDCOM, tree_id).unwrap();

    // Serialize to JSON and back
    let json = serde_json::to_string(&result).unwrap();
    let deserialized: oxidgene_gedcom::ImportResult = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.persons.len(), result.persons.len());
    assert_eq!(deserialized.person_names.len(), result.person_names.len());
    assert_eq!(deserialized.events.len(), result.events.len());
}

#[test]
fn test_export_result_serialization() {
    let export = export_gedcom(&[], &[], &[], &[], &[], &[], &[], &[], &[], &[], &[], &[]).unwrap();

    let json = serde_json::to_string(&export).unwrap();
    let deserialized: oxidgene_gedcom::ExportResult = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.gedcom, export.gedcom);
}
