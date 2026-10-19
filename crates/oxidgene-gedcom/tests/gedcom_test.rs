//! Integration tests for GEDCOM import and export.

use chrono::Utc;
use std::collections::HashMap;
use uuid::Uuid;

use oxidgene_core::types::{MediaLink, Note, Person, PersonName, Vignette};
use oxidgene_core::{EventType, NameType, Privacy, Sex};
use oxidgene_gedcom::export::{
    DEFAULT_DESTINATION, ExportChoices, ExportOptions, ExportRecords, ExportTarget,
    SubmitterSettings, destination, destinations, export_gedcom,
};
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

/// GEDCOM with free-text SOUR citations (Geneanet/GeneWeb-style: an archive
/// reference or URL instead of a pointer to a structured SOUR record). The
/// second individual repeats the exact same free-text description as the
/// first, to exercise dedup into a single synthesized `Source`.
const FREE_TEXT_SOURCE_GEDCOM: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
0 @I1@ INDI
1 NAME John /Doe/
2 GIVN John
2 SURN Doe
1 SEX M
1 BIRT
2 DATE 15 JAN 1842
2 SOUR AD33 - Plop - N - 1833-1842 - 5MI42 - vue 10d/23
3 PAGE p. 3
0 @I2@ INDI
1 NAME Jane /Doe/
2 GIVN Jane
2 SURN Doe
1 SEX F
1 BIRT
2 DATE 20 MAR 1844
2 SOUR AD33 - Plop - N - 1833-1842 - 5MI42 - vue 10d/23
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

/// GEDCOM with two `ASSO` associations in Gramps' own convention: a
/// level-1 `ASSO` directly under the witness's INDI record, pointing at
/// either a FAM (witness of that family's marriage) or an INDI (a role —
/// e.g. godparent — at the owner's own event).
const ASSOCIATION_GEDCOM: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
0 @I1@ INDI
1 NAME Guillaume /Doe/
1 SEX M
1 FAMS @F1@
0 @I2@ INDI
1 NAME Jeanne /Roe/
1 SEX F
1 FAMS @F1@
0 @I3@ INDI
1 NAME Pierre /Witness/
1 SEX M
1 ASSO @F1@
2 TYPE FAM
2 RELA witness
0 @I4@ INDI
1 NAME Baby /Doe/
1 SEX M
1 BAPM
2 DATE 1 JAN 1800
1 ASSO @I5@
2 TYPE INDI
2 RELA Godmother
0 @I5@ INDI
1 NAME God /Mother/
1 SEX F
0 @F1@ FAM
1 HUSB @I1@
1 WIFE @I2@
1 MARR
2 DATE 5 JUN 1865
0 TRLR
";

/// Same as `ASSOCIATION_GEDCOM`'s family witness, but Gramps-style
/// redundantly encoded twice: once nested inside the `MARR` event detail
/// (`2 ASSO`, the non-standard form some exporters also emit) and once as
/// the standard level-1 `ASSO` on the witness's own INDI record — both
/// describing the same fact.
const ASSOCIATION_DUPLICATE_GEDCOM: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
0 @I1@ INDI
1 NAME Guillaume /Doe/
1 SEX M
1 FAMS @F1@
0 @I2@ INDI
1 NAME Jeanne /Roe/
1 SEX F
1 FAMS @F1@
0 @I3@ INDI
1 NAME Pierre /Witness/
1 SEX M
1 ASSO @F1@
2 TYPE FAM
2 RELA witness
0 @F1@ FAM
1 HUSB @I1@
1 WIFE @I2@
1 MARR
2 DATE 5 JUN 1865
2 ASSO @I3@
3 TYPE INDI
3 RELA Witness
0 TRLR
";

/// The first imported event of a type.
fn event_of(
    result: &oxidgene_gedcom::ImportResult,
    event_type: oxidgene_core::EventType,
) -> &oxidgene_core::types::Event {
    result
        .events
        .iter()
        .find(|e| e.event_type == event_type)
        .unwrap_or_else(|| panic!("{event_type:?} event missing"))
}

/// Everything `imported` holds, exported again with the default options.
fn reexport(imported: &oxidgene_gedcom::ImportResult) -> String {
    export_gedcom(
        &imported.records(),
        &ExportOptions {
            choices: ExportChoices::default(),
            target: ExportTarget::Gedcom,
            self_person_id: None,
            submitter: Default::default(),
        },
    )
    .expect("exports")
    .gedcom
}

/// The texts of the notes `owner` picks out, in import order.
fn note_texts(result: &oxidgene_gedcom::ImportResult, owner: impl Fn(&Note) -> bool) -> Vec<&str> {
    result
        .notes
        .iter()
        .filter(|n| owner(n))
        .map(|n| n.text.as_str())
        .collect()
}

/// A primary name with these given names and surname.
fn assert_primary_name(name: &PersonName, given_names: &str, surname: &str) {
    assert_eq!(name.given_names.as_deref(), Some(given_names));
    assert_eq!(name.surname.as_deref(), Some(surname));
    assert!(name.is_primary);
}

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

    assert_primary_name(&result.person_names[0], "John", "Doe");

    // Check birth event
    let birth = event_of(&result, oxidgene_core::EventType::Birth);
    assert_eq!(birth.date_value.as_deref(), Some("15 JAN 1842"));
    assert!(birth.date_sort.is_some());
    assert!(birth.place_id.is_some());

    // Check death event
    let death = event_of(&result, oxidgene_core::EventType::Death);
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
    assert_eq!(cite.confidence, Some(oxidgene_core::Confidence::High));
}

#[test]
fn test_import_free_text_source_citation_deduplicates() {
    let tree_id = Uuid::now_v7();
    let result = import_gedcom(FREE_TEXT_SOURCE_GEDCOM, tree_id).unwrap();

    // Both individuals cite the same free-text description — deduplicated
    // into a single synthesized Source instead of being dropped.
    assert_eq!(result.sources.len(), 1);
    let src = &result.sources[0];
    assert_eq!(
        src.title,
        "AD33 - Plop - N - 1833-1842 - 5MI42 - vue 10d/23"
    );

    assert_eq!(result.citations.len(), 2);
    assert!(result.citations.iter().all(|c| c.source_id == src.id));
    assert!(
        result
            .citations
            .iter()
            .any(|c| c.page.as_deref() == Some("p. 3"))
    );
    assert!(result.warnings.is_empty());
}

#[test]
fn test_import_multimedia() {
    let tree_id = Uuid::now_v7();
    let result = import_gedcom(MULTIMEDIA_GEDCOM, tree_id).unwrap();

    assert_eq!(result.media.len(), 2);
    let document = result.media.iter().find(|m| m.is_document()).unwrap();
    let m = result.media.iter().find(|m| !m.is_document()).unwrap();
    assert_eq!(m.parent_media_id, Some(document.id));
    assert_eq!(m.page_index, 0);
    assert_eq!(document.page_count, 1);
    assert!(document.file_path.is_empty());
    assert_eq!(m.file_path, "/photos/john_doe.jpg");
    assert_eq!(m.file_name, "john_doe.jpg");
    assert_eq!(m.mime_type, "image/jpeg");
    assert!(m.title.is_none());
    assert_eq!(document.title.as_deref(), Some("Portrait of John Doe"));

    // Multimedia link on the individual
    assert_eq!(result.media_links.len(), 1);
    let ml = &result.media_links[0];
    assert_eq!(ml.media_id, document.id);
    assert_eq!(ml.person_id, Some(result.persons[0].id));
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

/// A file whose ages are partly free text or empty.
const UNREADABLE_AGE_GEDCOM: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
1 CHAR UTF-8
0 @I1@ INDI
1 NAME Branch /Alpha/
1 BIRT
2 DATE 1801
1 MARR
2 AGE majeur
1 DEAT
2 DATE 1870
2 AGE
2 PLAC Sample Town
1 BURI
2 AGE 69y
0 TRLR
";

/// An `AGE` that is not a GEDCOM age costs that age, not the import: the
/// valid ones are kept, the rest are reported naming their record, and
/// nothing of the value leaks into the warning.
#[test]
fn an_unreadable_age_is_a_warning_rather_than_a_failed_import() {
    let result = import_gedcom(UNREADABLE_AGE_GEDCOM, Uuid::now_v7()).expect("imports");
    assert_eq!(result.persons.len(), 1);
    let death = event_of(&result, oxidgene_core::EventType::Death);
    assert_eq!(death.date_value.as_deref(), Some("1870"));
    assert_eq!(death.age, None);
    assert!(death.place_id.is_some(), "the PLAC after the AGE survives");
    let burial = event_of(&result, oxidgene_core::EventType::Burial);
    assert_eq!(burial.age.as_deref(), Some("69y"));
    assert_eq!(result.warnings.len(), 2, "{:?}", result.warnings);
    assert!(
        result
            .warnings
            .iter()
            .all(|w| w.starts_with("Individual @I1@: an AGE")),
        "{:?}",
        result.warnings
    );
    assert!(result.warnings.iter().all(|w| !w.contains("majeur")));

    let archive = oxidgene_gedcom::export::export_gedzip(UNREADABLE_AGE_GEDCOM, &[])
        .expect("writes the archive");
    let imported =
        oxidgene_gedcom::import::import_gedzip(&archive, Uuid::now_v7()).expect("imports GEDZIP");
    assert_eq!(imported.result.persons.len(), 1);
    assert_eq!(imported.result.warnings.len(), 2);
}

/// A 5.5.1 note record pointed at from a person and from one of their events.
const NOTE_RECORD_GEDCOM: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
1 CHAR UTF-8
0 @I1@ INDI
1 NAME Branch /Alpha/
1 NOTE @N1@
1 BIRT
2 DATE 1801
2 NOTE @N2@
0 @N1@ NOTE A shared remark
1 CONT on two lines
0 @N2@ NOTE Read in the
1 CONC  register
0 TRLR
";

/// `NOTE @N1@` brings the record's text, not the pointer, onto the person
/// and the event that cite it.
#[test]
fn a_note_pointer_imports_the_text_of_its_record() {
    let result = import_gedcom(NOTE_RECORD_GEDCOM, Uuid::now_v7()).expect("imports");
    let person_id = result.persons[0].id;
    let birth = event_of(&result, oxidgene_core::EventType::Birth);
    let text_of = |owner: &dyn Fn(&Note) -> bool| {
        result
            .notes
            .iter()
            .filter(|n| owner(n))
            .map(|n| n.text.as_str())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        text_of(&|n| n.person_id == Some(person_id)),
        ["A shared remark\non two lines"]
    );
    assert_eq!(
        text_of(&|n| n.event_id == Some(birth.id)),
        ["Read in the register"]
    );
    assert!(result.notes.iter().all(|n| !n.text.contains("@N")));
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);
}

/// GEDCOM 7.0 points at a shared note with `SNOTE`; a pointer to a record
/// the file does not hold is left out with a warning, the notes beside it
/// kept.
#[test]
fn a_shared_note_pointer_of_either_version_resolves_or_is_reported() {
    let gedcom = "0 HEAD\n1 GEDC\n2 VERS 7.0\n0 @I1@ INDI\n1 NAME Branch /Alpha/\n\
                  1 SNOTE @N1@\n1 NOTE @N9@\n1 NOTE Kept\n1 BIRT\n2 DATE 1801\n2 SNOTE @N1@\n\
                  0 @N1@ SNOTE Shared\n1 CONT remark\n0 TRLR\n";
    let result = import_gedcom(gedcom, Uuid::now_v7()).expect("imports");
    let person_id = result.persons[0].id;
    let birth = event_of(&result, oxidgene_core::EventType::Birth).id;
    assert_eq!(
        note_texts(&result, |n| n.person_id == Some(person_id)),
        ["Shared\nremark", "Kept"]
    );
    assert_eq!(
        note_texts(&result, |n| n.event_id == Some(birth)),
        ["Shared\nremark"]
    );
    assert_eq!(result.warnings.len(), 1, "{:?}", result.warnings);
    assert!(result.warnings[0].contains("@N9@"), "{:?}", result.warnings);
}

/// Two people citing one shared note record, and one inline note of their
/// own.
const SHARED_NOTE_GEDCOM: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
1 CHAR UTF-8
0 @N1@ NOTE Shared text
0 @I1@ INDI
1 NAME Branch /Alpha/
1 NOTE @N1@
1 NOTE Inline note
0 @I2@ INDI
1 NAME Branch /Beta/
1 NOTE @N1@
0 TRLR
";

/// The `NOTE` or `SNOTE` record lines of `gedcom`, and the pointers to them.
fn shared_note_lines(gedcom: &str) -> (Vec<&str>, Vec<&str>) {
    let records = gedcom
        .lines()
        .filter(|l| l.starts_with("0 @N") && (l.contains(" NOTE ") || l.contains(" SNOTE ")))
        .collect();
    let pointers = gedcom
        .lines()
        .filter(|l| l.starts_with("1 NOTE @N") || l.starts_with("1 SNOTE @N"))
        .collect();
    (records, pointers)
}

/// The notes read from one shared note record carry its group; an inline
/// note carries none.
#[test]
fn notes_read_from_a_shared_note_record_share_its_group() {
    let imported = import_gedcom(SHARED_NOTE_GEDCOM, Uuid::now_v7()).expect("imports");
    let shared: Vec<&Note> = imported
        .notes
        .iter()
        .filter(|n| n.text == "Shared text")
        .collect();
    assert_eq!(shared.len(), 2, "{:?}", imported.notes);
    let group = shared[0].shared_group.expect("a shared note has a group");
    assert_eq!(shared[1].shared_group, Some(group));
    assert_eq!(imported.note_by_xref.get("@N1@"), Some(&group));
    let inline = imported
        .notes
        .iter()
        .find(|n| n.text == "Inline note")
        .expect("the inline note");
    assert_eq!(inline.shared_group, None);
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
}

/// An export writes the notes of one group that say the same thing as one
/// `NOTE` record, which they point at; the inline note stays inline.
#[test]
fn a_shared_note_is_exported_as_one_record_and_pointers_in_551() {
    let imported = import_gedcom(SHARED_NOTE_GEDCOM, Uuid::now_v7()).expect("imports");
    let exported = reexport(&imported);
    let (records, pointers) = shared_note_lines(&exported);
    assert_eq!(records, ["0 @N1@ NOTE Shared text"], "{exported}");
    assert_eq!(pointers, ["1 NOTE @N1@", "1 NOTE @N1@"], "{exported}");
    assert!(exported.contains("1 NOTE Inline note\n"), "{exported}");
}

/// The GEDZIP's `gedcom.ged` is GEDCOM 7.0, so its shared note is an `SNOTE`
/// record and its notes point at it with `SNOTE`.
#[test]
fn a_shared_note_is_exported_as_an_snote_record_in_a_gedzip() {
    let imported = import_gedcom(SHARED_NOTE_GEDCOM, Uuid::now_v7()).expect("imports");
    let paths = HashMap::new();
    let exported = export_as(
        &imported,
        ExportTarget::Gedzip {
            media_paths: &paths,
        },
    );
    let (records, pointers) = shared_note_lines(&exported.gedcom);
    assert_eq!(records, ["0 @N1@ SNOTE Shared text"], "{}", exported.gedcom);
    assert_eq!(pointers.len(), 2, "{}", exported.gedcom);
    assert!(
        pointers.iter().all(|l| l.starts_with("1 SNOTE @N1@")),
        "{}",
        exported.gedcom
    );
}

/// Notes of one group that say different things become one record per text,
/// and a note whose text is changed before the export follows its new text.
#[test]
fn a_changed_shared_note_is_exported_as_its_own_record() {
    let mut imported = import_gedcom(SHARED_NOTE_GEDCOM, Uuid::now_v7()).expect("imports");
    let person_two = imported.persons[1].id;
    let changed = imported
        .notes
        .iter_mut()
        .find(|n| n.person_id == Some(person_two))
        .expect("the second person's shared note");
    changed.text = "Other text".to_string();

    let exported = reexport(&imported);
    let (records, pointers) = shared_note_lines(&exported);
    // Numbered by group, then text: "Other text" sorts before "Shared text".
    assert_eq!(
        records,
        ["0 @N1@ NOTE Other text", "0 @N2@ NOTE Shared text"],
        "{exported}"
    );
    assert_eq!(pointers, ["1 NOTE @N2@", "1 NOTE @N1@"], "{exported}");
}

/// The export says which entity each of its records was written for: the
/// first person is `@I1@`, and a shared note record is its group.
#[test]
fn the_export_reports_the_entity_behind_each_xref() {
    let imported = import_gedcom(SHARED_NOTE_GEDCOM, Uuid::now_v7()).expect("imports");
    let exported = export_as(&imported, ExportTarget::Gedcom);
    assert_eq!(
        exported.xrefs.get("@I1@"),
        Some(&imported.persons[0].id),
        "{:?}",
        exported.xrefs
    );
    assert_eq!(exported.xrefs.get("@I2@"), Some(&imported.persons[1].id));
    let group = imported
        .notes
        .iter()
        .find_map(|n| n.shared_group)
        .expect("a shared note");
    assert_eq!(exported.xrefs.get("@N1@"), Some(&group));
    assert_eq!(exported.xrefs.get("@SUBM1@"), Some(&Uuid::nil()));
}

/// A person, an event and an attribute with several notes each, as GEDCOM
/// 5.5.1 allows.
const SEVERAL_NOTES_GEDCOM: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
1 CHAR UTF-8
0 @I1@ INDI
1 NAME Branch /Alpha/
1 NOTE First remark
1 BIRT
2 DATE 1801
2 NOTE Birth one
2 NOTE Birth two
1 OCCU Miller
2 NOTE Trade one
2 NOTE Trade two
1 NOTE Second remark
2 CONT on two lines
0 TRLR
";

/// Every note of a person, an event or an attribute is imported, and every
/// one is exported again.
#[test]
fn every_note_of_a_structure_survives_an_import_and_an_export() {
    let imported = import_gedcom(SEVERAL_NOTES_GEDCOM, Uuid::now_v7()).expect("imports");
    let check = |result: &oxidgene_gedcom::ImportResult| {
        let person_id = result.persons[0].id;
        let birth = event_of(result, oxidgene_core::EventType::Birth).id;
        let trade = event_of(result, oxidgene_core::EventType::Occupation).id;
        assert_eq!(
            note_texts(result, |n| n.person_id == Some(person_id)),
            ["First remark", "Second remark\non two lines"]
        );
        assert_eq!(
            note_texts(result, |n| n.event_id == Some(birth)),
            ["Birth one", "Birth two"]
        );
        assert_eq!(
            note_texts(result, |n| n.event_id == Some(trade)),
            ["Trade one", "Trade two"]
        );
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
    };
    check(&imported);

    let exported = reexport(&imported);
    assert_eq!(exported.matches("1 NOTE ").count(), 2, "{exported}");
    assert_eq!(exported.matches("2 NOTE ").count(), 4, "{exported}");
    check(&import_gedcom(&exported, Uuid::now_v7()).expect("re-imports"));
}

/// A note longer than a GEDCOM line comes back word for word: the export
/// never splits it beside a space, which a reader would drop.
#[test]
fn a_long_note_survives_a_round_trip_word_for_word() {
    let text = format!("{} tail and {} more", "x".repeat(260), "word ".repeat(80));
    let gedcom = MINIMAL_GEDCOM.replace("0 TRLR", &format!("1 NOTE {text}\n0 TRLR"));
    let imported = import_gedcom(&gedcom, Uuid::now_v7()).expect("imports");
    let exported = reexport(&imported);
    assert!(exported.lines().all(|line| line.len() <= 255), "{exported}");
    let back = import_gedcom(&exported, Uuid::now_v7()).expect("re-imports");
    assert_eq!(
        note_texts(&back, |n| n.person_id.is_some()),
        [text.as_str()]
    );
}

/// What earlier exports wrote for a long note: split at the 255th byte, which
/// fell just before a space, so the continuation opens with the space GEDCOM
/// keeps and readers that trim a `CONC` value lose.
#[test]
fn a_continuation_opening_with_a_space_keeps_it() {
    let head = "x".repeat(248);
    let gedcom = MINIMAL_GEDCOM.replace(
        "0 TRLR",
        &format!("1 NOTE {head}\n2 CONC  tail\n2 CONC   two spaces\n0 TRLR"),
    );
    let imported = import_gedcom(&gedcom, Uuid::now_v7()).expect("imports");
    let expected = format!("{head} tail  two spaces");
    assert_eq!(
        note_texts(&imported, |n| n.person_id.is_some()),
        [expected.as_str()]
    );
}

/// A long source title, citation page and death cause are continued between
/// two words' letters, and come back as they were.
#[test]
fn long_values_survive_a_round_trip_word_for_word() {
    let long = |word: &str| format!("{}{word} closing words", "word ".repeat(55));
    let gedcom = MINIMAL_GEDCOM
        .replace(
            "1 DEAT\n",
            &format!(
                "1 DEAT\n2 CAUS {}\n2 SOUR @S1@\n3 PAGE {}\n",
                long("cause"),
                long("page")
            ),
        )
        .replace(
            "0 TRLR",
            &format!("0 @S1@ SOUR\n1 TITL {}\n0 TRLR", long("title")),
        );
    let imported = import_gedcom(&gedcom, Uuid::now_v7()).expect("imports");
    let exported = reexport(&imported);
    assert!(exported.lines().all(|line| line.len() <= 255), "{exported}");
    assert_eq!(exported.matches(" CONC ").count(), 3, "{exported}");
    let back = import_gedcom(&exported, Uuid::now_v7()).expect("re-imports");
    let death = event_of(&back, oxidgene_core::EventType::Death);
    assert_eq!(death.cause.as_deref(), Some(long("cause").as_str()));
    assert_eq!(
        back.citations[0].page.as_deref(),
        Some(long("page").as_str())
    );
    assert_eq!(back.sources[0].title, long("title"));
}

/// A one-person GEDCOM declaring `charset`, with the given name `given`
/// encoded in it.
fn one_person_in(charset: &str, given: &[u8]) -> Vec<u8> {
    let mut bytes = format!("0 HEAD\n1 GEDC\n2 VERS 5.5.1\n1 CHAR {charset}\n0 @I1@ INDI\n1 NAME ")
        .into_bytes();
    bytes.extend_from_slice(given);
    bytes.extend_from_slice(b" /Alpha/\n0 TRLR\n");
    bytes
}

/// A `.ged` in ANSEL or ANSI, as GEDCOM 5 software writes them, imports with
/// its accents rather than being refused as invalid UTF-8.
#[test]
fn a_gedcom_in_a_legacy_character_set_is_decoded() {
    for (charset, given) in [
        ("ANSI", &b"Ren\xe9e"[..]),
        ("ANSEL", &b"Ren\xe2ee"[..]),
        ("UTF-8", "Renée".as_bytes()),
    ] {
        let bytes = one_person_in(charset, given);
        let result = oxidgene_gedcom::import::import_gedcom_bytes(&bytes, Uuid::now_v7())
            .unwrap_or_else(|e| panic!("{charset}: {e}"));
        let given = result.person_names[0]
            .given_names
            .as_deref()
            .unwrap_or_default();
        assert_eq!(
            unicode_compose(given),
            "Renée",
            "{charset} decoded as {given:?}"
        );
    }
}

/// `e` followed by a combining acute accent, as ANSEL spells it, folded
/// onto the precomposed `é` the other encodings give.
fn unicode_compose(text: &str) -> String {
    text.replace("e\u{301}", "é")
}

/// GEDCOM 5.5.1 requires `HEAD.SUBM` and the record it points at: named after
/// the tree's submitter setting, else its "Who am I?" person, else `Not
/// Provided`; the setting's email and address are written when set, and the
/// import reads the submitter back — but not `Not Provided`.
#[test]
fn the_export_names_its_submitter() {
    let imported = import_gedcom(FAMILY_GEDCOM, Uuid::now_v7()).expect("imports");
    let export_with = |self_person_id: Option<Uuid>, submitter: SubmitterSettings<'_>| {
        export_gedcom(
            &ExportRecords {
                gedcom_kept: &[],
                persons: &imported.persons,
                person_names: &imported.person_names,
                ..Default::default()
            },
            &ExportOptions {
                choices: ExportChoices::default(),
                target: ExportTarget::Gedcom,
                self_person_id,
                submitter,
            },
        )
        .expect("exports")
        .gedcom
    };
    let export = |self_person_id: Option<Uuid>| export_with(self_person_id, Default::default());
    let header_and_submitter = |gedcom: &str| {
        let head = gedcom.split("\n0 ").next().unwrap_or_default().to_string();
        let record = gedcom
            .split("\n0 ")
            .find(|record| record.starts_with("@SUBM1@ SUBM"))
            .map(str::to_string);
        (head, record)
    };

    let named = export(Some(imported.persons[1].id));
    let (head, record) = header_and_submitter(&named);
    assert!(head.contains("\n1 SUBM @SUBM1@"), "{named}");
    assert_eq!(record.as_deref(), Some("@SUBM1@ SUBM\n1 NAME Jane Smith"));

    let (_, record) = header_and_submitter(&export(None));
    assert_eq!(record.as_deref(), Some("@SUBM1@ SUBM\n1 NAME Not Provided"));

    let back = import_gedcom(&named, Uuid::now_v7()).expect("re-imports");
    assert_eq!(back.persons.len(), 3);
    assert!(back.warnings.is_empty(), "{:?}", back.warnings);
    assert_eq!(
        back.submitter.and_then(|s| s.name).as_deref(),
        Some("Jane Smith")
    );
    let unnamed = import_gedcom(&export(None), Uuid::now_v7()).expect("re-imports");
    assert_eq!(unnamed.submitter, None, "Not Provided is no name");

    let settings = SubmitterSettings {
        name: Some("Alpha Fixture"),
        email: Some("alpha@example.org"),
        address: Some("1 Fixture Road\nSampleton"),
    };
    let set = export_with(Some(imported.persons[1].id), settings);
    let (_, record) = header_and_submitter(&set);
    let record = record.unwrap_or_default();
    assert!(record.contains("\n1 NAME Alpha Fixture"), "{record}");
    // GEDCOM 5.5.1 doubles every `@` of a text.
    assert!(record.contains("\n1 EMAIL alpha@@example.org"), "{record}");
    assert!(
        record.contains("\n1 ADDR 1 Fixture Road\n2 CONT Sampleton"),
        "{record}"
    );
    let back = import_gedcom(&set, Uuid::now_v7()).expect("re-imports");
    assert_eq!(
        back.submitter,
        Some(oxidgene_gedcom::ImportedSubmitter {
            name: Some("Alpha Fixture".to_string()),
            email: Some("alpha@example.org".to_string()),
            address: Some("1 Fixture Road\nSampleton".to_string()),
        })
    );
}

/// A citation's transcript, `DATA.TEXT`, written over several lines.
const CITATION_TEXT_GEDCOM: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
1 CHAR UTF-8
0 @S1@ SOUR
1 TITL Sample register
0 @I1@ INDI
1 NAME Branch /Alpha/
1 BIRT
2 DATE 1801
2 SOUR @S1@
3 PAGE folio 3
3 DATA
4 TEXT Born on the third,
5 CONT son of the miller
0 TRLR
";

/// The ages and agencies of events and attributes, and a source's agency,
/// are imported in canonical form and written back, `SOUR.DATA.AGNC`
/// included.
#[test]
fn ages_and_agencies_survive_a_round_trip() {
    let gedcom = CITATION_TEXT_GEDCOM
        .replace(
            "1 TITL Sample register\n",
            "1 TITL Sample register\n1 DATA\n2 AGNC Sample archives\n",
        )
        .replace(
            "0 TRLR",
            "1 DEAT\n2 AGE > 80y\n2 AGNC Parish of Northwick\n\
             1 OCCU Miller, Baker\n2 AGE 34y\n2 AGNC Guild of Sampleton\n\
             1 BURI\n2 AGE STILLBORN\n0 TRLR",
        );
    let check = |result: &oxidgene_gedcom::ImportResult| {
        let age_of = |event_type| {
            result
                .events
                .iter()
                .filter(|e| e.event_type == event_type)
                .map(|e| (e.age.as_deref(), e.agency.as_deref()))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            age_of(oxidgene_core::EventType::Death),
            [(Some("> 80y"), Some("Parish of Northwick"))]
        );
        assert_eq!(
            age_of(oxidgene_core::EventType::Occupation),
            [(Some("34y"), Some("Guild of Sampleton")); 2],
            "one per split occupation"
        );
        assert_eq!(
            age_of(oxidgene_core::EventType::Burial),
            [(Some("STILLBORN"), None)]
        );
        assert_eq!(result.sources[0].agency.as_deref(), Some("Sample archives"));
    };
    let imported = import_gedcom(&gedcom, Uuid::now_v7()).expect("imports");
    check(&imported);
    let exported = reexport(&imported);
    assert!(
        exported.contains("1 DATA\n2 AGNC Sample archives\n"),
        "{exported}"
    );
    assert!(!exported.contains("PHRASE"), "{exported}");
    check(&import_gedcom(&exported, Uuid::now_v7()).expect("re-imports"));
}

/// The `TYPE` beside an attribute's value says what the value is: it is kept
/// as the event's classification and written back beside the value, as
/// GEDCOM 5.5.1 and as GEDCOM 7.0 inside a GEDZIP, where it never turns into
/// `TYPE Unknown`.
#[test]
fn an_attribute_type_beside_its_value_survives_a_round_trip() {
    let gedcom = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n1 CHAR UTF-8\n\
        0 @I1@ INDI\n1 NAME Branch /Alpha/\n\
        1 FACT Sample fact\n2 TYPE Hobby\n\
        1 IDNO 123-45\n2 TYPE National register\n\
        1 OCCU Weaver\n2 TYPE Main trade\n0 TRLR\n";
    let classified = |result: &oxidgene_gedcom::ImportResult| {
        let of = |event_type| {
            result
                .events
                .iter()
                .filter(|e| e.event_type == event_type)
                .map(|e| (e.description.as_deref(), e.classification.as_deref()))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            of(oxidgene_core::EventType::Fact),
            [(Some("Sample fact"), Some("Hobby"))]
        );
        assert_eq!(
            of(oxidgene_core::EventType::NationalId),
            [(Some("123-45"), Some("National register"))]
        );
        assert_eq!(
            of(oxidgene_core::EventType::Occupation),
            [(Some("Weaver"), Some("Main trade"))]
        );
    };
    let imported = import_gedcom(gedcom, Uuid::now_v7()).expect("imports");
    classified(&imported);

    let exported = reexport(&imported);
    for line in [
        "1 FACT Sample fact\n2 TYPE Hobby\n",
        "1 IDNO 123-45\n2 TYPE National register\n",
        "1 OCCU Weaver\n2 TYPE Main trade\n",
    ] {
        assert!(exported.contains(line), "{line:?} missing from\n{exported}");
    }
    assert!(!exported.contains("TYPE Unknown"), "{exported}");
    classified(&import_gedcom(&exported, Uuid::now_v7()).expect("re-imports"));

    let gedzip_text = export_gedcom(
        &imported.records(),
        &ExportOptions {
            choices: ExportChoices::default(),
            target: ExportTarget::Gedzip {
                media_paths: &HashMap::new(),
            },
            self_person_id: None,
            submitter: Default::default(),
        },
    )
    .expect("exports")
    .gedcom;
    assert!(gedzip_text.contains("VERS 7.0"), "{gedzip_text}");
    for line in [
        "2 TYPE Hobby\n",
        "2 TYPE National register\n",
        "2 TYPE Main trade\n",
    ] {
        assert!(
            gedzip_text.contains(line),
            "{line:?} missing from\n{gedzip_text}"
        );
    }
    assert!(!gedzip_text.contains("TYPE Unknown"), "{gedzip_text}");
}

/// A family event's `HUSB.AGE` and `WIFE.AGE` become the ages of the
/// husband and the wife, and are written back under the same spouse.
#[test]
fn spouse_ages_survive_a_round_trip() {
    let gedcom = FAMILY_GEDCOM.replace(
        "2 PLAC London, England\n",
        "2 PLAC London, England\n2 HUSB\n3 AGE 30y\n2 WIFE\n3 AGE < 25y\n",
    );
    let check = |result: &oxidgene_gedcom::ImportResult| {
        let given = |id| {
            result
                .person_names
                .iter()
                .find(|n| n.person_id == id)
                .and_then(|n| n.given_names.clone())
                .unwrap_or_default()
        };
        let marriage = result
            .events
            .iter()
            .find(|e| e.event_type == oxidgene_core::EventType::Marriage)
            .expect("a marriage");
        let ages: Vec<(String, &str)> = marriage
            .spouse_ages
            .iter()
            .map(|a| (given(a.person_id), a.age.as_str()))
            .collect();
        assert_eq!(
            ages,
            [("John".to_string(), "30y"), ("Jane".to_string(), "< 25y")]
        );
        assert_eq!(marriage.age, None);
    };
    let imported = import_gedcom(&gedcom, Uuid::now_v7()).expect("imports");
    check(&imported);
    let exported = reexport(&imported);
    assert!(exported.contains("2 HUSB\n3 AGE 30y\n"), "{exported}");
    check(&import_gedcom(&exported, Uuid::now_v7()).expect("re-imports"));
}

/// Repositories with their contact details and notes, a source's citations
/// of them with call numbers and media, and a pointerless citation naming a
/// repository by its text.
const REPOSITORY_GEDCOM: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
1 CHAR UTF-8
0 @R1@ REPO
1 NAME Sample Archives
1 ADDR 1 Fixture Road
2 CONT Sampleton
1 PHON 0100
1 EMAIL desk@archives.example
1 WWW https://archives.example
1 NOTE Closed on Mondays
0 @S1@ SOUR
1 TITL Parish register
1 REPO @R1@
2 CALN E 123
3 MEDI Film
1 REPO @R9@
0 @S2@ SOUR
1 TITL Census
1 REPO Town hall of Northwick
1 REPO
2 NOTE Town hall of Northwick
0 TRLR
";

#[test]
fn repositories_survive_a_round_trip() {
    let check = |result: &oxidgene_gedcom::ImportResult| {
        let archives = result
            .repositories
            .iter()
            .find(|r| r.name == "Sample Archives")
            .expect("the REPO record");
        assert_eq!(
            archives.address.as_deref(),
            Some("1 Fixture Road\nSampleton")
        );
        assert_eq!(archives.phone.as_deref(), Some("0100"));
        assert_eq!(archives.email.as_deref(), Some("desk@archives.example"));
        assert_eq!(
            archives.website.as_deref(),
            Some("https://archives.example")
        );
        assert!(
            result
                .notes
                .iter()
                .any(|n| n.repository_id == Some(archives.id) && n.text == "Closed on Mondays")
        );
        let register = result
            .sources
            .iter()
            .find(|s| s.title == "Parish register")
            .unwrap();
        let held: Vec<_> = result
            .source_repositories
            .iter()
            .filter(|l| l.source_id == register.id)
            .collect();
        assert_eq!(held.len(), 1, "the unknown pointer is left out");
        assert_eq!(held[0].repository_id, archives.id);
        assert_eq!(held[0].call_number.as_deref(), Some("E 123"));
        assert_eq!(
            held[0].media_type,
            Some(oxidgene_core::SourceMediaType::Film)
        );
        // The two pointerless citations name one repository.
        let census = result.sources.iter().find(|s| s.title == "Census").unwrap();
        let town_hall: Vec<_> = result
            .source_repositories
            .iter()
            .filter(|l| l.source_id == census.id)
            .map(|l| l.repository_id)
            .collect();
        assert_eq!(town_hall.len(), 2);
        assert_eq!(town_hall[0], town_hall[1]);
        assert_eq!(result.repositories.len(), 2);
    };
    let imported = import_gedcom(REPOSITORY_GEDCOM, Uuid::now_v7()).expect("imports");
    check(&imported);
    assert!(
        imported
            .warnings
            .iter()
            .any(|w| w.contains("REPO @R9@ not found")),
        "{:?}",
        imported.warnings
    );
    let exported = reexport(&imported);
    assert!(
        exported.contains("2 CALN E 123\n3 MEDI film\n"),
        "{exported}"
    );
    assert!(
        exported.contains("1 ADDR 1 Fixture Road\n2 CONT Sampleton\n"),
        "{exported}"
    );
    check(&import_gedcom(&exported, Uuid::now_v7()).expect("re-imports"));
}

/// A source held at one repository under several call numbers, each with
/// its own medium, has one link per call number, and keeps them all through
/// a round trip.
#[test]
fn every_call_number_of_a_repository_citation_survives_a_round_trip() {
    let gedcom = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @R1@ REPO\n1 NAME Sample Archives\n\
                  0 @S1@ SOUR\n1 TITL Parish register\n1 REPO @R1@\n\
                  2 CALN E 123\n3 MEDI Film\n2 CALN E 124\n3 MEDI book\n2 CALN E 125\n0 TRLR\n";
    let check = |result: &oxidgene_gedcom::ImportResult| {
        let mut links: Vec<_> = result.source_repositories.iter().collect();
        links.sort_by_key(|link| link.sort_order);
        let held: Vec<_> = links
            .iter()
            .map(|link| (link.call_number.as_deref(), link.media_type))
            .collect();
        use oxidgene_core::SourceMediaType::{Book, Film};
        assert_eq!(
            held,
            [
                (Some("E 123"), Some(Film)),
                (Some("E 124"), Some(Book)),
                (Some("E 125"), None)
            ]
        );
        assert!(
            links
                .iter()
                .all(|link| link.repository_id == result.repositories[0].id)
        );
    };
    let imported = import_gedcom(gedcom, Uuid::now_v7()).expect("imports");
    check(&imported);
    let exported = reexport(&imported);
    assert!(
        exported.contains("2 CALN E 123\n3 MEDI film\n"),
        "{exported}"
    );
    check(&import_gedcom(&exported, Uuid::now_v7()).expect("re-imports"));
}

/// A line break in a note, whatever its kind, is written as a `CONT` line:
/// a carriage return inside a value would end the line for a reader.
#[test]
fn every_kind_of_line_break_in_a_note_becomes_a_cont_line() {
    let mut imported = import_gedcom(
        &MINIMAL_GEDCOM.replace("0 TRLR", "1 NOTE placeholder\n0 TRLR"),
        Uuid::now_v7(),
    )
    .expect("imports");
    imported.notes[0].text = "one\r\ntwo\rthree\n\nfive".to_string();
    let exported = reexport(&imported);
    assert!(
        exported.contains("1 NOTE one\n2 CONT two\n2 CONT three\n2 CONT\n2 CONT five\n"),
        "{exported}"
    );
    assert!(!exported.contains('\r'), "{exported}");
}

/// The text a citation quotes from its source is exported, and comes back.
#[test]
fn a_citation_text_survives_a_round_trip() {
    let imported = import_gedcom(CITATION_TEXT_GEDCOM, Uuid::now_v7()).expect("imports");
    let quoted = "Born on the third,\nson of the miller";
    assert_eq!(imported.citations[0].text.as_deref(), Some(quoted));
    let exported = reexport(&imported);
    assert!(
        exported.contains("3 DATA\n4 TEXT Born on the third,\n5 CONT son of the miller\n"),
        "{exported}"
    );
    let back = import_gedcom(&exported, Uuid::now_v7()).expect("re-imports");
    assert_eq!(back.citations[0].text.as_deref(), Some(quoted));
    assert_eq!(back.citations[0].page.as_deref(), Some("folio 3"));
}

/// Every `QUAY`, and its absence, comes back as it was: an unassessed
/// citation is not exported with an assessment it never had.
#[test]
fn every_quay_and_its_absence_survive_a_round_trip() {
    let gedcom = CITATION_TEXT_GEDCOM.replace(
        "3 PAGE folio 3\n",
        "3 PAGE p0\n3 QUAY 0\n2 SOUR @S1@\n3 PAGE p1\n3 QUAY 1\n2 SOUR @S1@\n\
         3 PAGE p2\n3 QUAY 2\n2 SOUR @S1@\n3 PAGE p3\n3 QUAY 3\n2 SOUR @S1@\n3 PAGE none\n",
    );
    let quays = |gedcom: &str| -> Vec<(String, Option<String>)> {
        let lines: Vec<&str> = gedcom.lines().collect();
        lines
            .iter()
            .enumerate()
            .filter_map(|(i, line)| {
                let page = line.strip_prefix("3 PAGE ")?.to_string();
                let quay = lines[i + 1..]
                    .iter()
                    .take_while(|next| !next.starts_with("2 ") && !next.starts_with("1 "))
                    .find_map(|next| next.strip_prefix("3 QUAY "))
                    .map(str::to_string);
                Some((page, quay))
            })
            .collect()
    };
    let imported = import_gedcom(&gedcom, Uuid::now_v7()).expect("imports");
    use oxidgene_core::Confidence::{High, Low, Medium, VeryLow};
    let levels: Vec<_> = imported
        .citations
        .iter()
        .map(|c| (c.page.clone().unwrap_or_default(), c.confidence))
        .collect();
    for expected in [
        ("p0", Some(VeryLow)),
        ("p1", Some(Low)),
        ("p2", Some(Medium)),
        ("p3", Some(High)),
        ("none", None),
    ] {
        assert!(
            levels.contains(&(expected.0.to_string(), expected.1)),
            "{levels:?}"
        );
    }
    let exported = reexport(&imported);
    assert_eq!(quays(&exported), quays(&gedcom), "{exported}");
    assert!(quays(&gedcom).contains(&("none".to_string(), None)));
}

/// A source's publication facts are exported once and come back, over
/// several lines too.
#[test]
fn a_source_publisher_survives_a_round_trip() {
    let gedcom = CITATION_TEXT_GEDCOM.replace(
        "1 TITL Sample register\n",
        "1 TITL Sample register\n1 PUBL Sample press,\n2 CONT second edition\n",
    );
    let imported = import_gedcom(&gedcom, Uuid::now_v7()).expect("imports");
    let publisher = "Sample press,\nsecond edition";
    assert_eq!(imported.sources[0].publisher.as_deref(), Some(publisher));
    let exported = reexport(&imported);
    assert_eq!(exported.matches("1 PUBL").count(), 1, "{exported}");
    let back = import_gedcom(&exported, Uuid::now_v7()).expect("re-imports");
    assert_eq!(back.sources[0].publisher.as_deref(), Some(publisher));
}

/// Partners are written as `HUSB` and `WIFE` — the only spouse slots GEDCOM
/// 5.5.1 has — by sex, else in order; a third spouse is a warning.
#[test]
fn partners_are_exported_as_husband_and_wife() {
    let mut imported = import_gedcom(FAMILY_GEDCOM, Uuid::now_v7()).expect("imports");
    for spouse in &mut imported.family_spouses {
        spouse.role = oxidgene_core::SpouseRole::Partner;
        // The wife first: her sex, not the order, gives her the WIFE slot.
        spouse.sort_order = 1 - spouse.sort_order;
    }
    let exported = reexport(&imported);
    assert!(
        exported.contains("0 @F1@ FAM\n1 HUSB @I1@\n1 WIFE @I2@\n"),
        "{exported}"
    );

    let mut third = imported.family_spouses[0].clone();
    third.person_id = imported.persons[2].id;
    third.sort_order = 2;
    imported.family_spouses.push(third);
    let export = export_gedcom(
        &ExportRecords {
            gedcom_kept: &[],
            persons: &imported.persons,
            person_names: &imported.person_names,
            families: &imported.families,
            family_spouses: &imported.family_spouses,
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
    assert_eq!(export.warnings.len(), 1, "{:?}", export.warnings);
    assert!(export.warnings[0].contains("no HUSB or WIFE"));
}

/// A Julian date with a qualifier and a Julian range are written the GEDCOM
/// 5.5.1 way — the escape after the qualifier, before each bound — and read
/// back as they were.
#[test]
fn a_qualified_date_in_another_calendar_survives_a_round_trip() {
    let gedcom = MINIMAL_GEDCOM
        .replace("2 DATE 15 JAN 1842", "2 DATE ABT @#DJULIAN@ 1700")
        .replace(
            "2 DATE 3 MAR 1910",
            "2 DATE BET @#DJULIAN@ 1760 AND @#DJULIAN@ 1765",
        );
    let imported = import_gedcom(&gedcom, Uuid::now_v7()).expect("imports");
    let exported = reexport(&imported);
    assert!(
        exported.contains("2 DATE ABT @#DJULIAN@ 1700\n"),
        "{exported}"
    );
    assert!(exported.contains("2 DATE BET @#DJULIAN@ 1760 AND @#DJULIAN@ 1765\n"));
    let back = import_gedcom(&exported, Uuid::now_v7()).expect("re-imports");
    let birth = event_of(&back, oxidgene_core::EventType::Birth);
    assert_eq!(birth.calendar, oxidgene_core::Calendar::Julian);
    assert_eq!(birth.date_qualifier, oxidgene_core::DateQualifier::About);
    assert_eq!(birth.date_value.as_deref(), Some("1700"));
    let death = event_of(&back, oxidgene_core::EventType::Death);
    assert_eq!(death.calendar, oxidgene_core::Calendar::Julian);
    assert_eq!(
        (death.date_value.as_deref(), death.date_value2.as_deref()),
        (Some("1760"), Some("1765"))
    );
}

/// The death reason the `geneweb` crate writes beneath a death becomes the
/// death's cause, exported as `CAUS`.
#[test]
fn a_geneweb_death_reason_is_the_cause_of_death() {
    let gedcom = MINIMAL_GEDCOM.replace(
        "2 PLAC Paris, France\n",
        "2 PLAC Paris, France\n2 NOTE Sample remark\n2 _GWDEATH killed\n",
    );
    let imported = import_gedcom(&gedcom, Uuid::now_v7()).expect("imports");
    let death = event_of(&imported, oxidgene_core::EventType::Death);
    assert_eq!(death.cause.as_deref(), Some("killed"));
    assert_eq!(
        note_texts(&imported, |n| n.event_id == Some(death.id)),
        ["Sample remark"]
    );
    let exported = reexport(&imported);
    assert!(exported.contains("2 CAUS killed\n"), "{exported}");
    assert!(!exported.contains("_GWDEATH"), "{exported}");
}

#[test]
fn test_import_invalid_gedcom() {
    let tree_id = Uuid::now_v7();
    // Empty string — should still succeed (empty data)
    let result = import_gedcom("", tree_id);
    // oxidged may or may not error on empty input; either is fine
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
    let birth = event_of(&result, oxidgene_core::EventType::Birth);
    assert!(birth.date_sort.is_some());
    assert_eq!(
        birth.date_sort.unwrap(),
        chrono::NaiveDate::from_ymd_opt(1842, 1, 1).unwrap()
    );

    // Death: BET ... AND ... → should parse first date
    let death = event_of(&result, oxidgene_core::EventType::Death);
    assert!(death.date_sort.is_some());
    assert_eq!(
        death.date_sort.unwrap(),
        chrono::NaiveDate::from_ymd_opt(1900, 1, 1).unwrap()
    );
}

#[test]
fn test_import_asso_witness_and_godparent() {
    let tree_id = Uuid::now_v7();
    let result = import_gedcom(ASSOCIATION_GEDCOM, tree_id).unwrap();

    assert_eq!(result.persons.len(), 5); // I1 Guillaume, I2 Jeanne, I3 witness, I4 baby, I5 godmother
    assert_eq!(result.event_witnesses.len(), 2);

    let witness_person_id = result.persons[2].id; // I3
    let marriage = event_of(&result, oxidgene_core::EventType::Marriage);
    let marriage_witness = result
        .event_witnesses
        .iter()
        .find(|w| w.event_id == marriage.id)
        .expect("marriage witness missing");
    assert_eq!(marriage_witness.person_id, witness_person_id);
    assert_eq!(marriage_witness.relation.as_deref(), Some("witness"));

    let godmother_person_id = result.persons[4].id; // I5
    let baptism = event_of(&result, oxidgene_core::EventType::Baptism);
    let baptism_witness = result
        .event_witnesses
        .iter()
        .find(|w| w.event_id == baptism.id)
        .expect("baptism association missing");
    assert_eq!(baptism_witness.person_id, godmother_person_id);
    assert_eq!(baptism_witness.relation.as_deref(), Some("Godmother"));
}

#[test]
fn asso_on_a_person_with_a_baptism_is_not_reported_as_a_guess() {
    // A godparent belongs to a baptism, and finding one is the right answer —
    // not something to warn about. Warning whenever the person merely had
    // several events buried the associations that really were arbitrary.
    let gedcom = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
0 @I1@ INDI
1 NAME Child /BRANCH_A/
1 BIRT
2 DATE 1 JAN 1900
1 BAPM
2 DATE 5 JAN 1900
1 DEAT
2 DATE 1 JAN 1980
1 ASSO @I2@
2 RELA Godparent
0 @I2@ INDI
1 NAME Godparent /BRANCH_B/
0 TRLR
";

    let result = import_gedcom(gedcom, Uuid::now_v7()).expect("imports");

    assert_eq!(result.event_witnesses.len(), 1);
    assert!(
        !result.warnings.iter().any(|w| w.contains("ASSO")),
        "a baptism is the right event, not a guess: {:?}",
        result.warnings
    );
}

#[test]
fn asso_on_a_person_with_neither_birth_nor_baptism_is_reported() {
    // Here the choice really is arbitrary, and staying silent would attach a
    // witness to whichever event happened to be first with nothing said.
    let gedcom = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
0 @I1@ INDI
1 NAME Person /BRANCH_A/
1 OCCU Baker
1 RESI
2 DATE 1920
1 DEAT
2 DATE 1 JAN 1980
1 ASSO @I2@
2 RELA Witness
0 @I2@ INDI
1 NAME Witness /BRANCH_B/
0 TRLR
";

    let result = import_gedcom(gedcom, Uuid::now_v7()).expect("imports");

    assert_eq!(result.event_witnesses.len(), 1);
    assert!(
        result.warnings.iter().any(|w| w.contains("ASSO")),
        "an arbitrary choice must be reported: {:?}",
        result.warnings
    );
}

#[test]
fn test_import_asso_dedups_against_nested_event_association() {
    // Gramps sometimes writes the same witness fact twice: once as a
    // non-standard `ASSO` nested inside the event, once as the standard
    // level-1 `ASSO` on the witness's own record. Both must collapse into
    // a single EventWitness, not two.
    let tree_id = Uuid::now_v7();
    let result = import_gedcom(ASSOCIATION_DUPLICATE_GEDCOM, tree_id).unwrap();

    assert_eq!(result.event_witnesses.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════
// Export tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_export_produces_valid_gedcom() {
    let tree_id = Uuid::now_v7();
    let result = import_gedcom(MINIMAL_GEDCOM, tree_id).unwrap();

    let export = export_gedcom(
        &result.records(),
        &ExportOptions {
            choices: ExportChoices::default(),
            target: ExportTarget::Gedcom,
            self_person_id: None,
            submitter: Default::default(),
        },
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
        &result.records(),
        &ExportOptions {
            choices: ExportChoices::default(),
            target: ExportTarget::Gedcom,
            self_person_id: None,
            submitter: Default::default(),
        },
    )
    .unwrap();

    assert!(export.gedcom.contains("FAM"));
    assert!(export.gedcom.contains("HUSB"));
    assert!(export.gedcom.contains("WIFE"));
    assert!(export.gedcom.contains("CHIL"));
    assert!(export.gedcom.contains("MARR"));

    // INDI-level back-links: without these, other GEDCOM readers see a set
    // of disconnected individuals since most rely on FAMS/FAMC rather than
    // cross-referencing the FAM record's own HUSB/WIFE/CHIL.
    assert!(export.gedcom.contains("1 FAMS"));
    assert!(export.gedcom.contains("1 FAMC"));
}

#[test]
fn test_export_source() {
    let tree_id = Uuid::now_v7();
    let result = import_gedcom(SOURCE_GEDCOM, tree_id).unwrap();

    let export = export_gedcom(
        &result.records(),
        &ExportOptions {
            choices: ExportChoices::default(),
            target: ExportTarget::Gedcom,
            self_person_id: None,
            submitter: Default::default(),
        },
    )
    .unwrap();

    assert!(export.gedcom.contains("SOUR"));
    assert!(export.gedcom.contains("Parish Records of London"));
    assert!(export.gedcom.contains("Church of England"));
}

#[test]
fn test_export_empty() {
    let export = export_gedcom(
        &ExportRecords {
            gedcom_kept: &[],
            ..Default::default()
        },
        &ExportOptions {
            choices: ExportChoices::default(),
            target: ExportTarget::Gedcom,
            self_person_id: None,
            submitter: Default::default(),
        },
    )
    .unwrap();

    // Should still produce a valid GEDCOM with header/trailer
    assert!(export.gedcom.contains("HEAD"));
    assert!(export.gedcom.contains("TRLR"));
    assert!(export.warnings.is_empty());
}

/// Every export names OxidGene as the file's producer in `HEAD.SOUR`: a
/// `.ged` for any destination, with oxidged noted beneath it, and the
/// `gedcom.ged` of a GEDZIP.
#[test]
fn exports_name_oxidgene_as_the_producer() {
    let export = |destination: &'static oxidged::profile::ExportProfile, target| {
        export_gedcom(
            &ExportRecords::default(),
            &ExportOptions {
                choices: ExportChoices {
                    destination,
                    ..ExportChoices::default()
                },
                target,
                self_person_id: None,
                submitter: Default::default(),
            },
        )
        .unwrap()
        .gedcom
    };
    let names_oxidgene = |gedcom: &str| {
        let head: Vec<&str> = gedcom
            .lines()
            .map(|line| line.trim_start_matches('\u{feff}'))
            .take_while(|line| *line == "0 HEAD" || !line.starts_with("0 "))
            .collect();
        head.contains(&"1 SOUR OXIDGENE") && head.contains(&"2 NAME OxidGene")
    };
    for profile in destinations() {
        let gedcom = export(profile, ExportTarget::Gedcom);
        assert!(names_oxidgene(&gedcom), "{}:\n{gedcom}", profile.id());
        assert!(
            gedcom.contains("\n2 _OXIDGED "),
            "{}:\n{gedcom}",
            profile.id()
        );
    }
    let paths = HashMap::new();
    let gedcom = export(
        destination(DEFAULT_DESTINATION).unwrap(),
        ExportTarget::Gedzip {
            media_paths: &paths,
        },
    );
    assert!(names_oxidgene(&gedcom), "{gedcom}");
}

#[test]
fn test_export_long_utf8_note_does_not_panic() {
    let tree_id = Uuid::now_v7();
    let person_id = Uuid::now_v7();
    let now = Utc::now();
    let person = Person {
        id: person_id,
        tree_id,
        sex: Sex::Unknown,
        privacy: Privacy::Default,
        portrait_media_id: None,
        portrait_vignette_id: None,
        tags: Vec::new(),
        created_at: now,
        updated_at: now,
        deleted_at: None,
    };
    let name = PersonName {
        id: Uuid::now_v7(),
        person_id,
        name_type: NameType::Birth,
        given_names: Some("Marie".to_string()),
        surname: Some("Martin".to_string()),
        surname_prefix: None,
        prefix: None,
        suffix: None,
        nickname: None,
        is_primary: true,
        sort_order: 0,
        created_at: now,
        updated_at: now,
    };
    let note = Note {
        id: Uuid::now_v7(),
        tree_id,
        text: format!("{}é suite de la note", "a".repeat(254)),
        person_id: Some(person_id),
        event_id: None,
        family_id: None,
        source_id: None,
        media_id: None,
        repository_id: None,
        shared_group: None,
        created_at: now,
        updated_at: now,
        deleted_at: None,
    };

    let export = export_gedcom(
        &ExportRecords {
            gedcom_kept: &[],
            persons: &[person],
            person_names: &[name],
            notes: &[note],
            ..Default::default()
        },
        &ExportOptions {
            choices: ExportChoices::default(),
            target: ExportTarget::Gedcom,
            self_person_id: None,
            submitter: Default::default(),
        },
    )
    .unwrap();

    assert!(export.gedcom.contains("1 NOTE"));
    assert!(export.gedcom.contains("2 CONC"));
    assert!(export.gedcom.contains('é'));
}

#[test]
fn test_export_association_is_level_one_not_nested_in_event() {
    // GEDCOM 5.5.1 only allows `ASSO` as a direct child of an INDI record;
    // Gramps (and other readers) reject it nested inside an event detail.
    let tree_id = Uuid::now_v7();
    let result = import_gedcom(ASSOCIATION_GEDCOM, tree_id).unwrap();

    let export = export_gedcom(
        &result.records(),
        &ExportOptions {
            choices: ExportChoices::default(),
            target: ExportTarget::Gedcom,
            self_person_id: None,
            submitter: Default::default(),
        },
    )
    .unwrap();

    // Never nested inside an event detail.
    assert!(!export.gedcom.contains("\n2 ASSO "));
    assert!(!export.gedcom.contains("\n3 ASSO "));

    // The family witness's association is on their own INDI record (@I3@,
    // the third person exported), pointing at the family (@F1@). GEDCOM
    // 5.5.1 associates individuals only, so the writer keeps it as an
    // extension, which the import reads back as the witness it is.
    assert!(export.gedcom.contains("0 @I3@ INDI"));
    assert!(export.gedcom.contains("1 _ASSO @F1@"));
    let back = import_gedcom(&export.gedcom, Uuid::now_v7()).unwrap();
    assert_eq!(back.event_witnesses.len(), result.event_witnesses.len());

    // The godmother association is on the baby's own INDI record (@I4@),
    // pointing at the godmother (@I5@).
    assert!(export.gedcom.contains("0 @I4@ INDI"));
    assert!(export.gedcom.contains("1 ASSO @I5@"));
}

// ═══════════════════════════════════════════════════════════════════════
// Round-trip tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_roundtrip_preserves_individuals() {
    let tree_id = Uuid::now_v7();
    let imported = import_gedcom(FAMILY_GEDCOM, tree_id).unwrap();

    let exported = export_gedcom(
        &imported.records(),
        &ExportOptions {
            choices: ExportChoices::default(),
            target: ExportTarget::Gedcom,
            self_person_id: None,
            submitter: Default::default(),
        },
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
        &imported.records(),
        &ExportOptions {
            choices: ExportChoices::default(),
            target: ExportTarget::Gedcom,
            self_person_id: None,
            submitter: Default::default(),
        },
    )
    .unwrap();

    let tree_id2 = Uuid::now_v7();
    let reimported = import_gedcom(&exported.gedcom, tree_id2).unwrap();

    assert_eq!(reimported.person_names.len(), 1);
    let name = &reimported.person_names[0];
    assert_eq!(name.given_names.as_deref(), Some("John"));
    assert_eq!(name.surname.as_deref(), Some("Doe"));
}

/// A surname particle must survive a full GEDCOM round trip as `SPFX`, and
/// must not be doubled when the file carries it both inside the NAME slashes
/// and in its own SPFX tag.
#[test]
fn test_roundtrip_preserves_surname_particle() {
    const WITH_SPFX: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
1 CHAR UTF-8
0 @I1@ INDI
1 NAME Lois Mary /de la Cruz/
2 GIVN Lois Mary
2 SPFX de la
2 SURN Cruz
1 SEX F
0 TRLR
";

    let tree_id = Uuid::now_v7();
    let imported = import_gedcom(WITH_SPFX, tree_id).unwrap();

    assert_eq!(imported.person_names.len(), 1);
    let name = &imported.person_names[0];
    // The particle is split off, not glued into the root and not doubled.
    assert_eq!(name.surname.as_deref(), Some("Cruz"));
    assert_eq!(name.surname_prefix.as_deref(), Some("de la"));
    assert_eq!(name.full_surname().as_deref(), Some("de la Cruz"));
    assert_eq!(name.display_name(), "Lois Mary de la Cruz");

    let exported = export_gedcom(
        &imported.records(),
        &ExportOptions {
            choices: ExportChoices::default(),
            target: ExportTarget::Gedcom,
            self_person_id: None,
            submitter: Default::default(),
        },
    )
    .unwrap();

    // SPFX is emitted, and the NAME line still carries the full surname.
    assert!(
        exported.gedcom.contains("2 SPFX de la"),
        "{}",
        exported.gedcom
    );
    assert!(
        exported.gedcom.contains("2 SURN Cruz"),
        "{}",
        exported.gedcom
    );
    assert!(
        exported.gedcom.contains("1 NAME Lois Mary /de la Cruz/"),
        "{}",
        exported.gedcom
    );

    let reimported = import_gedcom(&exported.gedcom, Uuid::now_v7()).unwrap();
    let back = &reimported.person_names[0];
    assert_eq!(back.surname.as_deref(), Some("Cruz"));
    assert_eq!(back.surname_prefix.as_deref(), Some("de la"));
}

/// Splitting the particle must not make a person look like an alias of
/// themselves: `oxidged` fills SURN with the full "de la Cruz" parsed from the
/// NAME slashes, while the stored root is "Cruz", and comparing those verbatim
/// used to synthesize a spurious second AKA name.
#[test]
fn test_particle_split_does_not_synthesize_a_self_alias() {
    const NO_SURN_TAG: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
1 CHAR UTF-8
0 @I1@ INDI
1 NAME Lois /de la Cruz/
1 SEX F
0 TRLR
";

    let imported = import_gedcom(NO_SURN_TAG, Uuid::now_v7()).unwrap();
    assert_eq!(
        imported.person_names.len(),
        1,
        "expected exactly one name, got {:?}",
        imported
            .person_names
            .iter()
            .map(|n| (n.name_type, n.surname.clone(), n.surname_prefix.clone()))
            .collect::<Vec<_>>()
    );
    assert_eq!(imported.person_names[0].name_type, NameType::Birth);
}

/// Files that carry no SPFX at all — the common case — still get a structured
/// particle, derived from the surname itself.
#[test]
fn test_import_derives_particle_when_file_has_no_spfx() {
    const NO_SPFX: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
1 CHAR UTF-8
0 @I1@ INDI
1 NAME Pierre /van der Berg/
1 SEX M
0 @I2@ INDI
1 NAME Anne /Dupont/
1 SEX F
0 TRLR
";

    let imported = import_gedcom(NO_SPFX, Uuid::now_v7()).unwrap();

    let berg = imported
        .person_names
        .iter()
        .find(|n| n.surname.as_deref() == Some("Berg"))
        .expect("van der Berg imported under its root");
    assert_eq!(berg.surname_prefix.as_deref(), Some("van der"));
    assert_eq!(berg.full_surname().as_deref(), Some("van der Berg"));

    // A surname with no particle is left entirely alone.
    let dupont = imported
        .person_names
        .iter()
        .find(|n| n.surname.as_deref() == Some("Dupont"))
        .expect("Dupont imported");
    assert_eq!(dupont.surname_prefix, None);
}

// ═══════════════════════════════════════════════════════════════════════
// Geneanet-style import tests (no GIVN/SURN, inline SOUR, OBJE, OCCU)
// ═══════════════════════════════════════════════════════════════════════

/// Mimics a typical Geneanet GEDCOM export structure:
/// - No GIVN/SURN sub-tags under NAME (name parsed from value)
/// - Inline SOUR (text, not xref) under BIRT
/// - OBJE with FILE sub-tag
/// - OCCU (occupation attribute)
/// - Family with HUSB, WIFE, CHIL, and MARR
const GENEANET_GEDCOM: &str = "\
0 HEAD
1 SOUR Geneanet
2 NAME GeneWeb
2 VERS 7.0.0
1 GEDC
2 VERS 5.5.1
2 FORM LINEAGE-LINKED
1 CHAR UTF-8
0 @I55@ INDI
1 NAME Henri Jean-Marc /DURAND/
1 SEX M
1 BIRT
2 DATE 15 MAR 1952
2 PLAC Bordeaux, France
2 SOUR Registre civil
1 RESI
2 PLAC Bordeaux, France
1 OCCU Architecte
1 FAMC @F58@
1 FAMS @F60@
0 @I115@ INDI
1 NAME Marie /WEBER/
1 SEX F
1 BIRT
2 DATE 22 JUN 1953
2 PLAC Toulouse, France
2 SOUR Registre civil
1 OCCU Infirmiere
1 FAMS @F60@
0 @I61@ INDI
1 NAME Claire Sophie Anne /DURAND/
1 SEX F
1 BIRT
2 DATE 18 APR 1980
2 PLAC Nantes, France
2 SOUR Registre civil
1 OCCU Ingenieure
1 FAMC @F60@
1 FAMS @F65@
1 OBJE
2 FILE http://example.com/photo.jpg
0 @I133@ INDI
1 NAME Thomas /MOREAU/
1 SEX M
1 BIRT
2 DATE 07 AUG 1979
2 PLAC Rennes, France
2 SOUR Registre civil
1 OCCU Professeur
1 FAMS @F65@
0 @I134@ INDI
1 NAME Emma /MOREAU/
1 SEX F
1 BIRT
2 DATE 03 FEB 2006
2 PLAC Nantes, France
2 SOUR Registre civil
1 FAMC @F65@
0 @I141@ INDI
1 NAME Lucas /MOREAU/
1 SEX M
1 BIRT
2 DATE 25 JUL 2009
2 PLAC Montpellier, France
2 SOUR Registre civil
1 FAMC @F65@
0 @F58@ FAM
1 HUSB @I500@
1 CHIL @I55@
0 @F60@ FAM
1 MARR
2 DATE 14 SEP 1975
2 PLAC Toulouse, France
2 SOUR Registre civil
1 HUSB @I55@
1 WIFE @I115@
1 CHIL @I61@
0 @F65@ FAM
1 MARR
2 DATE 21 JUN 2005
2 PLAC Rennes, France
2 SOUR Registre civil
1 HUSB @I133@
1 WIFE @I61@
1 CHIL @I134@
1 CHIL @I141@
0 TRLR
";

#[test]
fn test_import_geneanet_names_parsed_from_value() {
    let tree_id = Uuid::now_v7();
    let result = import_gedcom(GENEANET_GEDCOM, tree_id).unwrap();

    // All 6 persons imported
    assert_eq!(result.persons.len(), 6, "persons: {:?}", result.warnings);

    // Names should be parsed from value (no GIVN/SURN sub-tags)
    for pn in &result.person_names {
        assert!(
            pn.given_names.is_some() || pn.surname.is_some(),
            "Name has no given_names or surname: {:?}",
            pn
        );
    }

    // Check specific names
    let claire_person = result.persons.iter().find(|p| {
        result.person_names.iter().any(|n| {
            n.person_id == p.id
                && n.surname.as_deref() == Some("DURAND")
                && n.given_names.as_deref() == Some("Claire Sophie Anne")
        })
    });
    assert!(claire_person.is_some(), "Claire DURAND not found");

    let thomas_name = result.person_names.iter().find(|n| {
        n.surname.as_deref() == Some("MOREAU") && n.given_names.as_deref() == Some("Thomas")
    });
    assert!(thomas_name.is_some(), "Thomas MOREAU not found");
}

#[test]
fn test_import_geneanet_birth_events() {
    let tree_id = Uuid::now_v7();
    let result = import_gedcom(GENEANET_GEDCOM, tree_id).unwrap();

    // Each of the 6 persons has a BIRT event
    let birth_events: Vec<_> = result
        .events
        .iter()
        .filter(|e| e.event_type == oxidgene_core::EventType::Birth)
        .collect();
    assert_eq!(birth_events.len(), 6, "expected 6 birth events");

    // Claire's birth should have date "18 APR 1980"
    let claire_id = result
        .person_names
        .iter()
        .find(|n| n.given_names.as_deref() == Some("Claire Sophie Anne"))
        .map(|n| n.person_id)
        .expect("Claire not found");

    let claire_birth = birth_events
        .iter()
        .find(|e| e.person_id == Some(claire_id))
        .expect("Claire birth event missing");
    assert_eq!(claire_birth.date_value.as_deref(), Some("18 APR 1980"));
}

#[test]
fn test_import_geneanet_individual_attributes() {
    // OCCU and RESI are GEDCOM `INDIVIDUAL_ATTRIBUTE_STRUCTURE` tags, read
    // as attributes rather than events (their value is what they say) —
    // make sure we still pick them up as `Event`s.
    let tree_id = Uuid::now_v7();
    let result = import_gedcom(GENEANET_GEDCOM, tree_id).unwrap();

    let henri_id = result
        .person_names
        .iter()
        .find(|n| n.given_names.as_deref() == Some("Henri Jean-Marc"))
        .map(|n| n.person_id)
        .expect("Henri not found");

    let occupations: Vec<_> = result
        .events
        .iter()
        .filter(|e| e.event_type == oxidgene_core::EventType::Occupation)
        .collect();
    assert_eq!(occupations.len(), 4, "expected 4 OCCU attributes imported");

    let henri_occu = occupations
        .iter()
        .find(|e| e.person_id == Some(henri_id))
        .expect("Henri's occupation missing");
    assert_eq!(henri_occu.description.as_deref(), Some("Architecte"));

    let henri_resi = result
        .events
        .iter()
        .find(|e| {
            e.event_type == oxidgene_core::EventType::Residence && e.person_id == Some(henri_id)
        })
        .expect("Henri's residence missing");
    let place = result
        .places
        .iter()
        .find(|p| Some(p.id) == henri_resi.place_id)
        .expect("residence place missing");
    assert_eq!(place.name, "Bordeaux, France");
}

#[test]
fn test_roundtrip_occupation_exports_as_occu_tag() {
    let tree_id = Uuid::now_v7();
    let imported = import_gedcom(GENEANET_GEDCOM, tree_id).unwrap();

    let exported = export_gedcom(
        &imported.records(),
        &ExportOptions {
            choices: ExportChoices::default(),
            target: ExportTarget::Gedcom,
            self_person_id: None,
            submitter: Default::default(),
        },
    )
    .unwrap();

    // OCCU must round-trip to its own tag, not a generic EVEN.
    assert!(
        exported.gedcom.contains("1 OCCU Architecte"),
        "expected '1 OCCU Architecte' in exported GEDCOM:\n{}",
        exported.gedcom
    );
    assert!(
        !exported.gedcom.contains("2 TYPE Architecte"),
        "occupation should not be exported as a generic EVEN/TYPE"
    );

    // RESI must still round-trip too.
    assert!(
        exported.gedcom.contains("1 RESI"),
        "expected '1 RESI' in exported GEDCOM:\n{}",
        exported.gedcom
    );

    // Re-importing should preserve the same counts.
    let tree_id2 = Uuid::now_v7();
    let reimported = import_gedcom(&exported.gedcom, tree_id2).unwrap();
    let occu_count = |events: &[oxidgene_core::types::Event]| {
        events
            .iter()
            .filter(|e| e.event_type == oxidgene_core::EventType::Occupation)
            .count()
    };
    assert_eq!(occu_count(&reimported.events), occu_count(&imported.events));
}

/// Geneanet only supports a single profession field, so a person with
/// several jobs ends up with them packed into one OCCU value, e.g.
/// "presales, trainer, consultant".
const MULTI_OCCUPATION_GEDCOM: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
2 FORM LINEAGE-LINKED
1 CHAR UTF-8
0 @I1@ INDI
1 NAME Jean /Dupont/
1 SEX M
1 OCCU presales,   trainer , consultant
0 TRLR
";

#[test]
fn test_import_splits_and_normalizes_multi_occupation_occu() {
    let tree_id = Uuid::now_v7();
    let result = import_gedcom(MULTI_OCCUPATION_GEDCOM, tree_id).unwrap();

    let mut occupations: Vec<&str> = result
        .events
        .iter()
        .filter(|e| e.event_type == oxidgene_core::EventType::Occupation)
        .filter_map(|e| e.description.as_deref())
        .collect();
    occupations.sort_unstable();

    assert_eq!(
        occupations,
        vec!["Consultant", "Presales", "Trainer"],
        "expected the OCCU value split on ',' into 3 trimmed events, each with its first letter uppercased"
    );
}

#[test]
fn test_import_single_occupation_is_case_normalized_and_not_split() {
    let tree_id = Uuid::now_v7();
    let gedcom = MINIMAL_GEDCOM.replace("0 TRLR", "1 OCCU farmer\n0 TRLR");
    let result = import_gedcom(&gedcom, tree_id).unwrap();

    let occupations: Vec<&str> = result
        .events
        .iter()
        .filter(|e| e.event_type == oxidgene_core::EventType::Occupation)
        .filter_map(|e| e.description.as_deref())
        .collect();

    assert_eq!(occupations, vec!["Farmer"]);
}

/// A person with two distinct `OCCU` tags (as produced by our own export,
/// or hand-authored GEDCOM), used to exercise the destinations that read a
/// single profession field.
const TWO_OCCUPATIONS_GEDCOM: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
2 FORM LINEAGE-LINKED
1 CHAR UTF-8
0 @I1@ INDI
1 NAME Jean /Dupont/
1 SEX M
1 OCCU Farmer
1 OCCU Carpenter
0 TRLR
";

#[test]
fn test_export_default_keeps_one_occu_tag_per_profession() {
    let tree_id = Uuid::now_v7();
    let imported = import_gedcom(TWO_OCCUPATIONS_GEDCOM, tree_id).unwrap();
    assert_eq!(
        imported
            .events
            .iter()
            .filter(|e| e.event_type == oxidgene_core::EventType::Occupation)
            .count(),
        2
    );

    let exported = export_gedcom(
        &imported.records(),
        &ExportOptions {
            choices: ExportChoices::default(),
            target: ExportTarget::Gedcom,
            self_person_id: None,
            submitter: Default::default(),
        },
    )
    .unwrap();

    assert!(exported.gedcom.contains("1 OCCU Farmer"));
    assert!(exported.gedcom.contains("1 OCCU Carpenter"));
    assert!(!exported.gedcom.contains("Farmer, Carpenter"));
}

/// `imported` written as a `.ged` for the destination `id`.
fn export_for(imported: &oxidgene_gedcom::ImportResult, id: &str) -> oxidgene_gedcom::ExportResult {
    export_gedcom(
        &imported.records(),
        &ExportOptions {
            choices: ExportChoices {
                destination: destination(id).expect("a oxidged export profile"),
                ..Default::default()
            },
            target: ExportTarget::Gedcom,
            self_person_id: None,
            submitter: Default::default(),
        },
    )
    .unwrap()
}

#[test]
fn test_export_for_geneanet_merges_occupations_and_reports_the_loss() {
    let tree_id = Uuid::now_v7();
    let imported = import_gedcom(TWO_OCCUPATIONS_GEDCOM, tree_id).unwrap();

    let exported = export_for(&imported, "geneanet");

    let occu_lines: Vec<&str> = exported
        .gedcom
        .lines()
        .filter(|l| l.starts_with("1 OCCU"))
        .collect();
    assert_eq!(
        occu_lines,
        ["1 OCCU Farmer, Carpenter"],
        "{}",
        exported.gedcom
    );
    let report = exported.report.expect("a .ged reports its destination");
    assert_eq!(report.destination, "geneanet");
    assert!(
        report
            .losses
            .iter()
            .any(|loss| loss.kind == "merged" && loss.record.as_deref() == Some("@I1@")),
        "{report:?}"
    );
}

#[test]
fn test_export_for_a_standard_destination_reports_no_loss() {
    let tree_id = Uuid::now_v7();
    let imported = import_gedcom(TWO_OCCUPATIONS_GEDCOM, tree_id).unwrap();

    for (id, version) in [
        ("generic-551", "2 VERS 5.5.1"),
        ("generic-70", "2 VERS 7.0"),
    ] {
        let exported = export_for(&imported, id);
        assert!(exported.gedcom.contains(version), "{}", exported.gedcom);
        assert!(
            exported.gedcom.contains("1 OCCU Farmer\n"),
            "{}",
            exported.gedcom
        );
        assert!(
            exported.gedcom.contains("1 OCCU Carpenter\n"),
            "{}",
            exported.gedcom
        );
        let report = exported.report.expect("a .ged reports its destination");
        assert_eq!(report.destination, id);
        assert!(report.losses.is_empty(), "{report:?}");
        assert!(report.limits.is_empty(), "{report:?}");
    }
}

#[test]
fn test_destinations_list_the_standard_versions_first() {
    let all = destinations();
    let ids: Vec<&str> = all.iter().map(|p| p.id()).collect();
    assert_eq!(&ids[..2], ["generic-551", "generic-70"]);
    for id in [
        "geneanet",
        "geneweb",
        "ancestry",
        "myheritage",
        "familysearch",
        "heredis",
    ] {
        assert!(ids.contains(&id), "{id} missing from {ids:?}");
    }
    assert_eq!(
        ExportChoices::default().destination.id(),
        DEFAULT_DESTINATION
    );
    assert!(destination("no-such-program").is_none());
    // Ids are exact: the API validates them as given.
    assert!(destination("GENEANET").is_none());
}

/// Geneanet's export puts the resolved surname on the NAME line but packs
/// every surname variant it knows about (including the resolved one, or
/// not) into a single comma-separated SURN sub-tag instead of emitting one
/// NAME/SURN structure per variant.
const SURNAME_ALIAS_GEDCOM: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
2 FORM LINEAGE-LINKED
1 CHAR UTF-8
0 @I1@ INDI
1 NAME Anna /BRANCH_A/
2 SURN BRANCH_B,BRANCH_C
1 SEX F
0 TRLR
";

#[test]
fn test_import_keeps_name_surname_as_primary_over_surn() {
    let tree_id = Uuid::now_v7();
    let result = import_gedcom(SURNAME_ALIAS_GEDCOM, tree_id).unwrap();

    let primary = result
        .person_names
        .iter()
        .find(|n| n.is_primary)
        .expect("expected a primary PersonName");
    assert_eq!(primary.surname.as_deref(), Some("BRANCH_A"));
}

#[test]
fn test_import_splits_multi_alias_surn_into_also_known_as_names() {
    let tree_id = Uuid::now_v7();
    let result = import_gedcom(SURNAME_ALIAS_GEDCOM, tree_id).unwrap();

    let mut aliases: Vec<&str> = result
        .person_names
        .iter()
        .filter(|n| n.name_type == oxidgene_core::NameType::AlsoKnownAs)
        .filter_map(|n| n.surname.as_deref())
        .collect();
    aliases.sort_unstable();

    assert_eq!(
        aliases,
        vec!["BRANCH_B", "BRANCH_C"],
        "expected the SURN value split on ',' into 2 also-known-as names, excluding the primary surname"
    );
}

#[test]
fn test_export_default_keeps_one_name_tag_per_person_name() {
    let tree_id = Uuid::now_v7();
    let imported = import_gedcom(SURNAME_ALIAS_GEDCOM, tree_id).unwrap();
    assert_eq!(imported.person_names.len(), 3);

    let exported = export_gedcom(
        &imported.records(),
        &ExportOptions {
            choices: ExportChoices::default(),
            target: ExportTarget::Gedcom,
            self_person_id: None,
            submitter: Default::default(),
        },
    )
    .unwrap();

    let name_lines = exported
        .gedcom
        .lines()
        .filter(|l| l.trim_start().starts_with("1 NAME") && *l != "1 NAME Not Provided")
        .count();
    assert_eq!(
        name_lines, 3,
        "expected one '1 NAME' line per PersonName, got:\n{}",
        exported.gedcom
    );
    assert!(exported.gedcom.contains("1 NAME Anna /BRANCH_A/"));
    assert!(!exported.gedcom.contains("BRANCH_B,BRANCH_C"));
}

#[test]
fn test_export_for_geneanet_packs_alias_surnames_into_the_primary_name() {
    let tree_id = Uuid::now_v7();
    let imported = import_gedcom(SURNAME_ALIAS_GEDCOM, tree_id).unwrap();

    let exported = export_for(&imported, "geneanet");

    let name_lines = exported
        .gedcom
        .lines()
        .filter(|l| l.starts_with("1 NAME") && *l != "1 NAME Not Provided")
        .count();
    assert_eq!(name_lines, 1, "{}", exported.gedcom);
    assert!(
        exported.gedcom.contains("1 NAME Anna /BRANCH_A/"),
        "{}",
        exported.gedcom
    );
    assert!(
        exported
            .gedcom
            .contains("2 SURN BRANCH_A,BRANCH_B,BRANCH_C"),
        "{}",
        exported.gedcom
    );
}

#[test]
fn test_import_geneanet_family_spouses() {
    let tree_id = Uuid::now_v7();
    let result = import_gedcom(GENEANET_GEDCOM, tree_id).unwrap();

    // F60: Henri (HUSB) + Marie (WIFE), child: Claire
    // F65: Thomas (HUSB) + Claire (WIFE), children: Emma, Lucas
    // F58: unknown HUSB (I500 not defined), child: Henri
    assert_eq!(result.families.len(), 3);

    // F60 should have 2 spouses
    let _f60_spouses: Vec<_> = result
        .family_spouses
        .iter()
        .filter(|s| {
            let fam = result.families.iter().find(|f| f.id == s.family_id);
            // F60 has both HUSB (I55 = Henri) and WIFE (I115 = Marie)
            fam.is_some()
        })
        .collect();
    // Total spouses across all families (F58 has warning for I500)
    // F58: HUSB @I500@ → not found (warning), so 0 spouses for F58
    // F60: 2 spouses, F65: 2 spouses = 4 total
    assert_eq!(
        result.family_spouses.len(),
        4,
        "spouses: {:?}, warnings: {:?}",
        result.family_spouses,
        result.warnings
    );

    // F65 should have 2 children
    let f65_children_count = result
        .family_children
        .iter()
        .filter(|c| {
            // Check the children belong to the family of Thomas+Claire
            result.family_spouses.iter().any(|s| {
                s.family_id == c.family_id
                    && result.person_names.iter().any(|n| {
                        n.person_id == s.person_id && n.given_names.as_deref() == Some("Thomas")
                    })
            })
        })
        .count();
    assert_eq!(f65_children_count, 2, "F65 should have 2 children");
}

#[test]
fn test_import_geneanet_mother_linked() {
    let tree_id = Uuid::now_v7();
    let result = import_gedcom(GENEANET_GEDCOM, tree_id).unwrap();

    // Find Claire's person_id
    let claire_id = result
        .person_names
        .iter()
        .find(|n| n.given_names.as_deref() == Some("Claire Sophie Anne"))
        .map(|n| n.person_id)
        .expect("Claire not found");

    // Find Marie's person_id
    let marie_id = result
        .person_names
        .iter()
        .find(|n| n.given_names.as_deref() == Some("Marie"))
        .map(|n| n.person_id)
        .expect("Marie not found");

    // Claire should be a child in a family
    let claire_family_id = result
        .family_children
        .iter()
        .find(|c| c.person_id == claire_id)
        .map(|c| c.family_id)
        .expect("Claire not a child in any family");

    // That family should have Marie as a spouse (Wife)
    let marie_spouse = result
        .family_spouses
        .iter()
        .find(|s| s.family_id == claire_family_id && s.person_id == marie_id);
    assert!(
        marie_spouse.is_some(),
        "Marie not linked as spouse in Claire's parent family"
    );
    assert_eq!(marie_spouse.unwrap().role, oxidgene_core::SpouseRole::Wife);
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
    let export = export_gedcom(
        &ExportRecords {
            gedcom_kept: &[],
            ..Default::default()
        },
        &ExportOptions {
            choices: ExportChoices::default(),
            target: ExportTarget::Gedcom,
            self_person_id: None,
            submitter: Default::default(),
        },
    )
    .unwrap();

    let json = serde_json::to_string(&export).unwrap();
    let deserialized: oxidgene_gedcom::ExportResult = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.gedcom, export.gedcom);
}

/// What the `geneweb` crate writes for a `.gw` personal event GEDCOM has no
/// tag for: a generic `EVEN` whose `TYPE` names the tag it would have used,
/// and a `_GWTAG` extension beneath it recording the original `.gw` tag.
const GENEWEB_CONVERTED_GEDCOM: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
1 CHAR UTF-8
0 @I1@ INDI
1 NAME Jean /BRANCH-A/
1 EVEN
2 TYPE EDUC
2 DATE 1932
2 NOTE Institution Sainte-Marie
2 _GWTAG #educ
1 EVEN
2 TYPE OCCU
2 _GWTAG #occu
1 EVEN
2 TYPE Tutelle apres un deces
2 NOTE Acte non numerise
2 _GWTAG #Tutelle apres un deces
0 TRLR
";

#[test]
fn test_import_drops_geneweb_bookkeeping_from_events() {
    let tree_id = Uuid::now_v7();
    let result = import_gedcom(GENEWEB_CONVERTED_GEDCOM, tree_id).unwrap();

    let note_of = |event_id| {
        result
            .notes
            .iter()
            .find(|n| n.event_id == Some(event_id))
            .map(|n| n.text.as_str())
    };

    // `TYPE EDUC` is what typed this event; repeating it as the description
    // only put a bare "EDUC" in the form next to the translated type.
    let education = event_of(&result, oxidgene_core::EventType::Education);
    assert_eq!(education.description, None);
    assert_eq!(education.date_value.as_deref(), Some("1932"));
    // The note keeps what was written about the event, and nothing else.
    assert_eq!(note_of(education.id), Some("Institution Sainte-Marie"));

    // The extension makes no note.
    let occupation = event_of(&result, oxidgene_core::EventType::Occupation);
    assert_eq!(occupation.description, None);
    assert_eq!(note_of(occupation.id), None);

    // A user-defined GeneWeb event name is not bookkeeping: it is the only
    // record of what the event was, so it stays as the description.
    let other = event_of(&result, oxidgene_core::EventType::Other);
    assert_eq!(other.description.as_deref(), Some("Tutelle apres un deces"));
    assert_eq!(note_of(other.id), Some("Acte non numerise"));
}

/// `RESN` on individuals and a family, in the forms GEDCOM 5.5.1 and 7 write.
const RESTRICTION_GEDCOM: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
0 @I1@ INDI
1 NAME Anna /BRANCH_A/
1 RESN confidential
1 FAMS @F1@
0 @I2@ INDI
1 NAME Bert /BRANCH_B/
1 RESN privacy
1 FAMS @F1@
0 @I3@ INDI
1 NAME Carl /BRANCH_C/
1 RESN locked
0 @I4@ INDI
1 NAME Dora /BRANCH_D/
1 RESN LOCKED, CONFIDENTIAL
0 @I5@ INDI
1 NAME Emil /BRANCH_E/
0 @F1@ FAM
1 HUSB @I2@
1 WIFE @I1@
1 RESN confidential
0 TRLR
";

/// A private person or family is exported with `RESN confidential` and comes
/// back private; a record following the tree carries no `RESN`.
#[test]
fn privacy_survives_a_round_trip_as_resn() {
    let imported = import_gedcom(RESTRICTION_GEDCOM, Uuid::now_v7()).expect("imports");
    let exported = reexport(&imported);
    let record = |start: &str| {
        let at = exported.find(start).expect("the record");
        let rest = &exported[at + start.len()..];
        rest[..rest.find("\n0 ").unwrap_or(rest.len())].to_string()
    };
    assert!(
        record("0 @I1@ INDI").contains("\n1 RESN confidential"),
        "{exported}"
    );
    assert!(record("0 @F1@ FAM").contains("\n1 RESN confidential"));
    // `RESN locked` is no privacy OxidGene models: it is kept GEDCOM, written
    // back beneath its person, while the restrictions it does model map as
    // above, one `RESN` each.
    assert!(
        record("0 @I3@ INDI").contains("\n1 RESN locked"),
        "{exported}"
    );
    assert_eq!(exported.matches("1 RESN").count(), 5, "{exported}");
    let back = import_gedcom(&exported, Uuid::now_v7()).expect("re-imports");
    let privacy = |result: &oxidgene_gedcom::ImportResult| {
        let mut persons: Vec<Privacy> = result.persons.iter().map(|p| p.privacy).collect();
        persons.push(result.families[0].privacy);
        persons
    };
    assert_eq!(privacy(&back), privacy(&imported));
}

#[test]
fn test_import_resn_withholding_a_record_makes_it_private() {
    let result = import_gedcom(RESTRICTION_GEDCOM, Uuid::now_v7()).unwrap();
    let privacy: Vec<Privacy> = result.persons.iter().map(|p| p.privacy).collect();
    assert_eq!(
        privacy,
        [
            Privacy::Private,
            Privacy::Private,
            // `locked` forbids changes; it says nothing about who may read.
            Privacy::Default,
            Privacy::Private,
            Privacy::Default,
        ]
    );
    assert_eq!(result.families[0].privacy, Privacy::Private);
}

#[test]
fn test_import_asso_after_nested_witnesses_comes_after_them() {
    // A godparent nested in the baptism, another on the individual: both
    // land on the baptism, the second after the first.
    let gedcom = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
0 @I1@ INDI
1 NAME Child /BRANCH_A/
1 BAPM
2 DATE 5 JAN 1900
2 ASSO @I2@
3 RELA GODF
1 ASSO @I3@
2 RELA GODM
0 @I2@ INDI
1 NAME Godfather /BRANCH_B/
0 @I3@ INDI
1 NAME Godmother /BRANCH_C/
0 TRLR
";
    let result = import_gedcom(gedcom, Uuid::now_v7()).unwrap();
    let mut witnesses: Vec<(i32, Option<&str>)> = result
        .event_witnesses
        .iter()
        .map(|w| (w.sort_order, w.relation.as_deref()))
        .collect();
    witnesses.sort();
    assert_eq!(witnesses, [(0, Some("GODF")), (1, Some("GODM"))]);
}

#[test]
fn test_import_an_aka_name_adding_only_a_nickname_is_a_byname() {
    let gedcom = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
0 @I1@ INDI
1 NAME John /Doe/
2 NICK Johnny
1 NAME John /Doe/
2 TYPE aka
2 NICK Jacko
1 NAME
2 TYPE aka
2 NICK Jay
1 NAME John /Poe/
2 TYPE aka
2 NICK Jo
0 TRLR
";
    let result = import_gedcom(gedcom, Uuid::now_v7()).unwrap();
    let names: Vec<(NameType, Option<&str>, Option<&str>)> = result
        .person_names
        .iter()
        .map(|n| (n.name_type, n.surname.as_deref(), n.nickname.as_deref()))
        .collect();
    assert_eq!(
        names,
        [
            (NameType::Birth, Some("Doe"), Some("Johnny")),
            // Restating the name, or leaving it empty, to carry a nickname.
            (NameType::Byname, None, Some("Jacko")),
            (NameType::Byname, None, Some("Jay")),
            // A name of its own, with a nickname on it.
            (NameType::AlsoKnownAs, Some("Poe"), Some("Jo")),
        ]
    );
}

/// Every `EventType` there is, read from `serde`'s own list of the enum's
/// variants: a variant added later is in it without anyone remembering to
/// add it here, so the round trip below covers it too.
fn every_event_type() -> Vec<EventType> {
    let unknown =
        serde_json::from_str::<EventType>("\"__no_such_event_type__\"").expect_err("not a variant");
    let message = unknown.to_string();
    let (_, listed) = message
        .split_once("expected one of")
        .unwrap_or_else(|| panic!("serde lists the variants: {message}"));
    let all: Vec<EventType> = listed
        .split('`')
        .skip(1)
        .step_by(2)
        .map(|name| serde_json::from_value(serde_json::Value::from(name)).expect("a variant"))
        .collect();
    assert!(all.len() > 70, "{all:?}");
    all
}

/// Whether GEDCOM records an event of this type on the couple's `FAM`
/// record rather than on a person. Exhaustive on purpose: a new variant does
/// not compile until somebody decides where it belongs.
fn is_family_event(event_type: EventType) -> bool {
    match event_type {
        EventType::Marriage
        | EventType::Divorce
        | EventType::Annulment
        | EventType::Engagement
        | EventType::MarriageBann
        | EventType::MarriageContract
        | EventType::MarriageLicense
        | EventType::MarriageSettlement
        | EventType::CivilUnion
        | EventType::Separation
        | EventType::DivorceFiled
        | EventType::SealingSpouse
        | EventType::NoMarriage
        | EventType::NoMention => true,
        EventType::Birth
        | EventType::Death
        | EventType::Baptism
        | EventType::Confirmation
        | EventType::FirstCommunion
        | EventType::BarBatMitzvah
        | EventType::BasMitzvah
        | EventType::MilitaryService
        | EventType::Burial
        | EventType::Cremation
        | EventType::Graduation
        | EventType::Immigration
        | EventType::Emigration
        | EventType::Naturalization
        | EventType::Census
        | EventType::Occupation
        | EventType::Residence
        | EventType::Retirement
        | EventType::Will
        | EventType::Probate
        | EventType::Adoption
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
        | EventType::Fact
        | EventType::Blessing
        | EventType::Ordination
        | EventType::Christening
        | EventType::AdultChristening
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
        | EventType::SealingParent
        | EventType::FamilyLinkLds
        | EventType::LdsBaptism
        | EventType::LdsConfirmation
        | EventType::LdsInitiatory
        | EventType::Other => false,
    }
}

const COUPLE_GEDCOM: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
2 FORM LINEAGE-LINKED
1 CHAR UTF-8
0 @I1@ INDI
1 NAME Alpha /Sample/
1 SEX M
1 FAMS @F1@
0 @I2@ INDI
1 NAME Beta /Sample/
1 SEX F
1 FAMS @F1@
0 @F1@ FAM
1 HUSB @I1@
1 WIFE @I2@
0 TRLR
";

/// An imported event's type, description and place name.
type EventShape = (EventType, Option<String>, Option<String>);

/// What each imported event came back as, keyed by its date, which no two
/// events of the round trip share.
fn events_by_date(result: &oxidgene_gedcom::ImportResult) -> HashMap<String, EventShape> {
    let place_name: HashMap<Uuid, &str> = result
        .places
        .iter()
        .map(|p| (p.id, p.name.as_str()))
        .collect();
    result
        .events
        .iter()
        .map(|e| {
            (
                e.date_value.clone().expect("every event is dated"),
                (
                    e.event_type,
                    e.description.clone(),
                    e.place_id.map(|id| place_name[&id].to_owned()),
                ),
            )
        })
        .collect()
}

/// Adds a dated event, at a place of its own, to `tree`, and returns the
/// date that identifies it with what it must come back as.
fn add_event(
    tree: &mut oxidgene_gedcom::ImportResult,
    event_type: EventType,
    owner: (Option<Uuid>, Option<Uuid>),
    description: Option<&str>,
) -> (String, EventShape) {
    let now = Utc::now();
    let n = tree.events.len();
    let date = format!("{} JAN {}", n % 28 + 1, 1700 + n);
    let place = oxidgene_core::types::Place {
        id: Uuid::now_v7(),
        tree_id: tree.tree_id,
        name: format!("Sampletown {n}"),
        latitude: None,
        longitude: None,
        created_at: now,
        updated_at: now,
    };
    let (person_id, family_id) = owner;
    tree.events.push(oxidgene_core::types::Event {
        classification: None,
        id: Uuid::now_v7(),
        tree_id: tree.tree_id,
        event_type,
        date_value: Some(date.clone()),
        date_sort: None,
        date_qualifier: Default::default(),
        date_value2: None,
        calendar: Default::default(),
        cause: None,
        age: None,
        agency: None,
        spouse_ages: Vec::new(),
        place_id: Some(place.id),
        person_id,
        family_id,
        related_family_id: None,
        description: description.map(str::to_owned),
        tags: Vec::new(),
        created_at: now,
        updated_at: now,
        deleted_at: None,
    });
    let shape = (
        event_type,
        description.map(str::to_owned),
        Some(place.name.clone()),
    );
    tree.places.push(place);
    (date, shape)
}

/// Every generic `EVEN` of `gedcom` with no `TYPE` saying what it is.
fn untyped_generic_events(gedcom: &str) -> Vec<&str> {
    let mut untyped = Vec::new();
    let mut lines = gedcom.lines().peekable();
    while let Some(line) = lines.next() {
        if !line.starts_with("1 EVEN") {
            continue;
        }
        let mut typed = false;
        while let Some(child) = lines.next_if(|l| !l.starts_with("0 ") && !l.starts_with("1 ")) {
            typed |= child.starts_with("2 TYPE ");
        }
        if !typed {
            untyped.push(line);
        }
    }
    untyped
}

/// The dates of the events `gedcom` writes under an extension tag (`1 _INIL`).
fn extension_event_dates(gedcom: &str) -> Vec<String> {
    let mut dates = Vec::new();
    let mut in_extension = false;
    for line in gedcom.lines() {
        if line.starts_with("1 ") {
            in_extension = line.starts_with("1 _");
        } else if let Some(date) = line.strip_prefix("2 DATE ").filter(|_| in_extension) {
            dates.push(date.to_owned());
        }
    }
    dates
}

/// Every event type survives an export and a re-import under its own type,
/// with its date, place and description — as GEDCOM 5.5.1, read as GEDCOM
/// 7.0 (but for the ordinances 5.5.1 writes under extension tags), and inside
/// a GEDZIP. Each type travels twice, described and not; a
/// civil union, an untyped event and a military service also with a
/// description that names what they are or something else.
#[test]
fn every_event_type_survives_a_round_trip() {
    let mut tree = import_gedcom(COUPLE_GEDCOM, Uuid::now_v7()).expect("imports");
    let husband = (Some(tree.persons[0].id), None);
    let wife = (Some(tree.persons[1].id), None);
    let couple = (None, Some(tree.families[0].id));
    let mut expected = HashMap::new();
    for event_type in every_event_type() {
        let owner = if is_family_event(event_type) {
            couple
        } else {
            husband
        };
        let description = format!("Sample detail {}", tree.events.len());
        expected.extend([
            add_event(&mut tree, event_type, owner, Some(&description)),
            add_event(&mut tree, event_type, owner, None),
        ]);
    }
    expected.extend([
        add_event(&mut tree, EventType::BarBatMitzvah, wife, None),
        add_event(&mut tree, EventType::CivilUnion, couple, Some("PACS")),
        add_event(&mut tree, EventType::Other, couple, Some("Land lease")),
        add_event(
            &mut tree,
            EventType::Other,
            husband,
            Some("Military parade"),
        ),
        add_event(
            &mut tree,
            EventType::MilitaryService,
            husband,
            Some("Military service"),
        ),
    ]);

    let gedcom = reexport(&tree);
    for line in [
        "1 CONF\n",
        "1 FCOM\n",
        "1 BARM\n",
        "1 BASM\n",
        "2 TYPE Military service\n",
        "2 TYPE Separation\n",
        "2 TYPE PACS\n",
        "2 TYPE Land lease\n",
        "1 EVEN Military parade\n",
    ] {
        assert!(gedcom.contains(line), "{line:?} missing from\n{gedcom}");
    }
    for tag in ["1 MILI", "1 SEP"] {
        assert!(!gedcom.contains(tag), "{tag} in\n{gedcom}");
    }
    // GEDCOM 7.0 requires the `TYPE` of a generic event.
    assert_eq!(untyped_generic_events(&gedcom), Vec::<&str>::new());

    let as_gedcom_7 = gedcom.replacen("2 VERS 5.5.1", "2 VERS 7.0", 1);
    assert_ne!(as_gedcom_7, gedcom, "the header names the version");
    let archive = oxidgene_gedcom::export::export_gedzip(&gedcom, &[]).expect("writes a GEDZIP");
    for (format, back) in [
        (
            "GEDCOM 5.5.1",
            import_gedcom(&gedcom, Uuid::now_v7()).expect("imports"),
        ),
        (
            "GEDCOM 7.0",
            import_gedcom(&as_gedcom_7, Uuid::now_v7()).expect("imports"),
        ),
        (
            "GEDZIP",
            oxidgene_gedcom::import::import_gedzip(&archive, Uuid::now_v7())
                .expect("imports")
                .result,
        ),
    ] {
        // GEDCOM 5.5.1 has no structure for some ordinances, written under
        // extension tags that a GEDCOM 7.0 file does not define.
        let left_out = if format == "GEDCOM 7.0" {
            extension_event_dates(&gedcom)
        } else {
            Vec::new()
        };
        assert_eq!(
            back.events.len(),
            tree.events.len() - left_out.len(),
            "{format}"
        );
        let actual = events_by_date(&back);
        for (date, want) in &expected {
            if !left_out.contains(date) {
                assert_eq!(actual.get(date), Some(want), "{format}, event dated {date}");
            }
        }
    }
}

/// The standard tags of a confirmation, a first communion and a bar or bat
/// mitzvah — which other software and the `geneweb` crate write — import as
/// those events, not as untyped ones.
#[test]
fn standard_religious_event_tags_import_as_their_types() {
    let gedcom = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
0 @I1@ INDI
1 CONF
2 DATE 1700
1 FCOM
2 DATE 1701
1 BARM
2 DATE 1702
1 BASM
2 DATE 1703
0 TRLR
";
    let result = import_gedcom(gedcom, Uuid::now_v7()).expect("imports");
    let types: Vec<EventType> = result.events.iter().map(|e| e.event_type).collect();
    assert_eq!(
        types,
        [
            EventType::Confirmation,
            EventType::FirstCommunion,
            EventType::BarBatMitzvah,
            EventType::BasMitzvah,
        ]
    );
}

/// A generic event's line value is GEDCOM's event descriptor: it describes
/// the event alongside the `TYPE` that classifies it. A tag's own `Y` only
/// asserts that the event happened.
#[test]
fn a_generic_events_descriptor_is_part_of_its_description() {
    let gedcom = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
0 @I1@ INDI
1 EVEN Appointed committee chair
2 TYPE Civic appointment
1 EVEN Sample assignment
2 TYPE Military service
1 DEAT Y
0 TRLR
";
    let result = import_gedcom(gedcom, Uuid::now_v7()).expect("imports");
    let events: Vec<(EventType, Option<&str>)> = result
        .events
        .iter()
        .map(|e| (e.event_type, e.description.as_deref()))
        .collect();
    assert_eq!(
        events,
        [
            (
                EventType::Other,
                Some("Civic appointment: Appointed committee chair")
            ),
            (EventType::MilitaryService, Some("Sample assignment")),
            (EventType::Death, None),
        ]
    );
}

/// The name of the place an event happened at.
fn place_of<'r>(
    result: &'r oxidgene_gedcom::ImportResult,
    event: &oxidgene_core::types::Event,
) -> Option<&'r str> {
    let id = event.place_id?;
    result
        .places
        .iter()
        .find(|p| p.id == id)
        .map(|p| p.name.as_str())
}

const UNDEFINED_EVENT_TAGS_GEDCOM: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
0 @I1@ INDI
1 NAME Alpha /Sample/
1 MILI Sergeant
2 TYPE Army
2 DATE 1 MAR 1900
2 PLAC Sampletown
2 NOTE Sample service note
2 SOUR @S1@
3 PAGE folio 3
2 OBJE @M1@
1 XYZZ Sample value
2 DATE 1901
2 NOTE Sample other note
1 XYZZ
1 CREA
2 DATE 1 JAN 2000
1 SUBM @U1@
1 _MILX Custom
2 DATE 1902
0 @S1@ SOUR
1 TITL Sample register
0 @M1@ OBJE
1 FILE sample.jpg
2 FORM jpg
0 TRLR
";

/// A `MILI` — an extension no GEDCOM version defines, which `oxidged` keeps
/// whole as an extension of the person — imports as a military service
/// holding its date, place, note, citation and media; any other such tag as
/// a described event of its own. Such a tag with nothing beneath it, a
/// standard tag that is no event (`CREA`, `SUBM`) and a custom `_` tag are
/// no events.
#[test]
fn an_event_tag_no_version_defines_keeps_its_substructures() {
    let result = import_gedcom(UNDEFINED_EVENT_TAGS_GEDCOM, Uuid::now_v7()).expect("imports");
    assert_eq!(result.events.len(), 2, "{:?}", result.events);

    let service = event_of(&result, EventType::MilitaryService);
    assert_eq!(service.date_value.as_deref(), Some("1 MAR 1900"));
    assert_eq!(place_of(&result, service), Some("Sampletown"));
    assert_eq!(service.description.as_deref(), Some("Army: Sergeant"));
    assert_eq!(
        note_texts(&result, |n| n.event_id == Some(service.id)),
        ["Sample service note"]
    );
    let citation = result
        .citations
        .iter()
        .find(|c| c.event_id == Some(service.id))
        .expect("the citation is the event's");
    assert_eq!(citation.page.as_deref(), Some("folio 3"));
    assert!(
        result
            .media_links
            .iter()
            .any(|l| l.event_id == Some(service.id)),
        "{:?}",
        result.media_links
    );

    let other = event_of(&result, EventType::Other);
    assert_eq!(other.description.as_deref(), Some("XYZZ: Sample value"));
    assert_eq!(other.date_value.as_deref(), Some("1901"));
    assert_eq!(
        note_texts(&result, |n| n.event_id == Some(other.id)),
        ["Sample other note"]
    );

    // Nothing beneath either tag became the person's.
    assert!(note_texts(&result, |n| n.person_id.is_some()).is_empty());
    assert!(result.citations.iter().all(|c| c.person_id.is_none()));
    assert!(result.media_links.iter().all(|l| l.person_id.is_none()));
}

const LDS_ORDINANCES_GEDCOM: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
0 @I1@ INDI
1 NAME Alpha /Sample/
1 FAMC @F1@
1 BAPL
2 DATE 1 JAN 1900
2 TEMP SLAKE
2 PLAC Sampletown 1
1 CONL
2 DATE 2 JAN 1900
2 STAT COMPLETED
3 DATE 3 FEB 1950
2 PLAC Sampletown 2
1 ENDL
2 DATE 3 JAN 1900
2 PLAC Sampletown 3
2 NOTE Sample ordinance note
1 SLGC
2 DATE 4 JAN 1900
2 FAMC @F1@
2 PLAC Sampletown 4
0 @F1@ FAM
1 CHIL @I1@
1 SLGS
2 DATE 5 JAN 1900
2 TEMP SLAKE
2 PLAC Sampletown 5
0 TRLR
";

/// The LDS ordinances other software writes as GEDCOM's own structures
/// import as their types, with their date, place, notes and citations, and
/// with no description: the temple and status are not read into the event.
/// They survive an export and a re-import as they came.
#[test]
fn lds_ordinances_import_as_their_types_without_a_description() {
    let expected: [(EventType, &str, &str); 5] = [
        (EventType::LdsBaptism, "1 JAN 1900", "Sampletown 1"),
        (EventType::LdsConfirmation, "2 JAN 1900", "Sampletown 2"),
        (EventType::Endowment, "3 JAN 1900", "Sampletown 3"),
        (EventType::SealingChild, "4 JAN 1900", "Sampletown 4"),
        (EventType::SealingSpouse, "5 JAN 1900", "Sampletown 5"),
    ];
    let imported = import_gedcom(LDS_ORDINANCES_GEDCOM, Uuid::now_v7()).expect("imports");
    let reimported = import_gedcom(&reexport(&imported), Uuid::now_v7()).expect("re-imports");
    for (pass, result) in [("import", &imported), ("round trip", &reimported)] {
        assert_eq!(result.events.len(), expected.len(), "{pass}");
        for (event_type, date, place) in expected {
            let event = event_of(result, event_type);
            assert_eq!(
                event.date_value.as_deref(),
                Some(date),
                "{pass} {event_type:?}"
            );
            assert_eq!(
                place_of(result, event),
                Some(place),
                "{pass} {event_type:?}"
            );
            assert_eq!(event.description, None, "{pass} {event_type:?}");
            assert_eq!(
                event.family_id.is_some(),
                event_type == EventType::SealingSpouse,
                "{pass} {event_type:?}"
            );
        }
        let endowment = event_of(result, EventType::Endowment);
        assert_eq!(
            note_texts(result, |n| n.event_id == Some(endowment.id)),
            ["Sample ordinance note"],
            "{pass}"
        );
    }
}

/// An ordinance is written back as GEDCOM's own structure: `BAPL` under its
/// individual and `SLGS` under its family, with its date and place, and no
/// `TYPE` to say what it is.
#[test]
fn lds_ordinances_are_written_as_their_gedcom_structures() {
    const GEDCOM: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
0 @I1@ INDI
1 NAME Alpha /Sample/
1 BAPL
2 DATE 1 JAN 1950
2 TEMP SLAKE
2 PLAC Town
1 FAMS @F1@
0 @I2@ INDI
1 NAME Beta /Sample/
1 FAMS @F1@
0 @F1@ FAM
1 HUSB @I1@
1 WIFE @I2@
1 SLGS
2 DATE 2 JAN 1960
0 TRLR
";
    let imported = import_gedcom(GEDCOM, Uuid::now_v7()).expect("imports");
    assert_eq!(event_of(&imported, EventType::LdsBaptism).description, None);
    assert_eq!(
        event_of(&imported, EventType::SealingSpouse).description,
        None
    );

    let exported = reexport(&imported);
    assert!(
        exported.contains("1 BAPL\n2 DATE 1 JAN 1950\n2 TEMP SLAKE\n2 PLAC Town\n"),
        "{exported}"
    );
    let family = exported
        .find(" FAM\n")
        .unwrap_or_else(|| panic!("a family record: {exported}"));
    assert!(
        exported[family..].contains("1 SLGS\n2 DATE 2 JAN 1960\n"),
        "{exported}"
    );
    assert!(!exported.contains("TYPE BAPL"), "{exported}");
    assert!(!exported.contains("TYPE SLGS"), "{exported}");
}

/// An ordinance with a description has no room for it in GEDCOM's structure,
/// so it is written as the generic `EVEN` its type names, and reads back as
/// its type with the description.
#[test]
fn an_lds_ordinance_with_a_description_stays_a_generic_event() {
    const GEDCOM: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
0 @I1@ INDI
1 NAME Alpha /Sample/
1 EVEN
2 DATE 4 JAN 1951
2 TYPE BAPL
0 TRLR
";
    let mut imported = import_gedcom(GEDCOM, Uuid::now_v7()).expect("imports");
    assert_eq!(
        event_of(&imported, EventType::LdsBaptism)
            .date_value
            .as_deref(),
        Some("4 JAN 1951"),
        "a generic EVEN typed BAPL imports as its type"
    );
    let baptism = imported
        .events
        .iter_mut()
        .find(|e| e.event_type == EventType::LdsBaptism)
        .expect("a baptism");
    baptism.description = Some("Sample proxy font".to_string());

    let exported = reexport(&imported);
    assert!(
        exported.contains("1 EVEN Sample proxy font\n"),
        "{exported}"
    );
    assert!(exported.contains("2 TYPE BAPL\n"), "{exported}");
    assert!(!exported.contains("1 BAPL\n"), "{exported}");
    let back = import_gedcom(&exported, Uuid::now_v7()).expect("re-imports");
    let baptism = event_of(&back, EventType::LdsBaptism);
    assert_eq!(baptism.description.as_deref(), Some("Sample proxy font"));
    assert_eq!(baptism.date_value.as_deref(), Some("4 JAN 1951"));
}

/// GEDCOM 7.0's `INIL` imports as the initiatory ordinance, and is written back
/// as one. A generic `EVEN` typed `INIL` (what an export with a description
/// writes) reads as the same type.
#[test]
fn an_initiatory_ordinance_is_read_and_written_in_gedcom_7() {
    const GEDCOM: &str = "\
0 HEAD
1 GEDC
2 VERS 7.0
0 @I1@ INDI
1 NAME Alpha /Sample/
1 INIL
2 DATE 3 JAN 1950
0 TRLR
";
    let imported = import_gedcom(GEDCOM, Uuid::now_v7()).expect("imports");
    let initiatory = event_of(&imported, EventType::LdsInitiatory);
    assert_eq!(initiatory.date_value.as_deref(), Some("3 JAN 1950"));
    assert_eq!(initiatory.description, None);

    let gedzip = export_as(
        &imported,
        ExportTarget::Gedzip {
            media_paths: &HashMap::new(),
        },
    )
    .gedcom;
    assert!(gedzip.contains("VERS 7.0"), "{gedzip}");
    assert!(gedzip.contains("1 INIL\n2 DATE 3 JAN 1950\n"), "{gedzip}");

    // GEDCOM 5.5.1 has no `INIL`; whatever form the writer gives it reads
    // back as the same type.
    let ged = reexport(&imported);
    let back = import_gedcom(&ged, Uuid::now_v7()).expect("re-imports");
    assert_eq!(
        event_of(&back, EventType::LdsInitiatory)
            .date_value
            .as_deref(),
        Some("3 JAN 1950"),
        "{ged}"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Export choices
// ═══════════════════════════════════════════════════════════════════════

/// A family documented every way an export can leave out: notes on a
/// person, an event and the family, a source with a note held by a
/// repository with one, citations on a person and an event, and a
/// photograph linked to a person and to the family.
const DOCUMENTED_GEDCOM: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
2 FORM LINEAGE-LINKED
1 CHAR UTF-8
0 @R1@ REPO
1 NAME Sample Archives
1 NOTE Archive remark
0 @S1@ SOUR
1 TITL Sample register
1 NOTE Source remark
1 REPO @R1@
2 CALN E 1
0 @M1@ OBJE
1 FILE photos/sample.jpg
2 FORM image/jpeg
2 TITL Sample photograph
0 @I1@ INDI
1 NAME Alder /Sample/
1 SEX M
1 FAMS @F1@
1 NOTE Person remark
1 SOUR @S1@
2 PAGE folio 1
1 OBJE @M1@
1 BIRT
2 DATE 1 JAN 1850
2 NOTE Birth remark
2 SOUR @S1@
3 PAGE folio 2
0 @I2@ INDI
1 NAME Birch /Example/
1 SEX F
1 FAMS @F1@
0 @F1@ FAM
1 HUSB @I1@
1 WIFE @I2@
1 NOTE Family remark
1 OBJE @M1@
1 MARR
2 DATE 2 FEB 1875
0 TRLR
";

/// `DOCUMENTED_GEDCOM`, imported and exported again with `choices`.
fn export_documented(choices: ExportChoices) -> String {
    let imported = import_gedcom(DOCUMENTED_GEDCOM, Uuid::now_v7()).expect("imports");
    assert_eq!(imported.notes.len(), 5, "{:?}", imported.notes);
    assert_eq!(imported.citations.len(), 2);
    assert_eq!(imported.repositories.len(), 1);
    assert_eq!(imported.media_links.len(), 2);
    export_gedcom(
        &imported.records(),
        &ExportOptions {
            choices,
            target: ExportTarget::Gedcom,
            self_person_id: None,
            submitter: Default::default(),
        },
    )
    .expect("exports")
    .gedcom
}

/// The tag of each line of `gedcom` past its header, whose `SOUR` names the
/// software that wrote it.
fn tags(gedcom: &str) -> Vec<&str> {
    gedcom
        .lines()
        .skip(1)
        .skip_while(|line| !line.starts_with("0 "))
        .filter_map(|line| {
            line.split_whitespace()
                .skip(1)
                .find(|word| !word.starts_with('@'))
        })
        .collect()
}

/// Every pointer of `gedcom` names a record it holds.
fn assert_every_pointer_resolves(gedcom: &str) {
    let records: std::collections::HashSet<&str> = gedcom
        .lines()
        .filter_map(|line| line.strip_prefix("0 @"))
        .filter_map(|rest| rest.split('@').next())
        .collect();
    for line in gedcom.lines().filter(|line| !line.starts_with("0 ")) {
        let value = line.splitn(3, ' ').nth(2).unwrap_or_default();
        if let Some(xref) = value
            .strip_prefix('@')
            .and_then(|rest| rest.strip_suffix('@'))
        {
            assert!(records.contains(xref), "dangling {line:?} in:\n{gedcom}");
        }
    }
}

#[test]
fn an_export_without_notes_and_sources_holds_none_and_still_imports() {
    let exported = export_documented(ExportChoices {
        include_notes_and_sources: false,
        ..Default::default()
    });

    let tags = tags(&exported);
    for absent in ["NOTE", "SOUR", "REPO", "PAGE", "CALN"] {
        assert!(!tags.contains(&absent), "{absent} in:\n{exported}");
    }
    assert!(!exported.contains("remark"), "{exported}");
    assert!(!exported.contains("Sample register"), "{exported}");
    assert!(!exported.contains("Sample Archives"), "{exported}");
    assert_every_pointer_resolves(&exported);

    let back = import_gedcom(&exported, Uuid::now_v7()).expect("re-imports");
    assert!(back.warnings.is_empty(), "{:?}", back.warnings);
    assert_eq!(back.persons.len(), 2);
    assert_eq!(back.families.len(), 1);
    assert!(back.notes.is_empty(), "{:?}", back.notes);
    assert!(back.sources.is_empty());
    assert!(back.citations.is_empty());
    assert!(back.repositories.is_empty());
    // The media stay: the photograph, as a document of one page.
    assert_eq!(back.media.iter().filter(|m| !m.is_document()).count(), 1);
    assert_eq!(back.media_links.len(), 2);
}

#[test]
fn an_export_without_media_holds_none_and_still_imports() {
    let exported = export_documented(ExportChoices {
        include_media: false,
        ..Default::default()
    });

    assert!(!tags(&exported).contains(&"OBJE"), "{exported}");
    assert!(!exported.contains("_OXIDGENE"), "{exported}");
    assert!(!exported.contains("sample.jpg"), "{exported}");
    assert_every_pointer_resolves(&exported);

    let back = import_gedcom(&exported, Uuid::now_v7()).expect("re-imports");
    assert!(back.warnings.is_empty(), "{:?}", back.warnings);
    assert!(back.media.is_empty());
    assert!(back.media_links.is_empty());
    // The notes and sources stay.
    assert_eq!(back.notes.len(), 5);
    assert_eq!(back.citations.len(), 2);
    assert_eq!(back.repositories.len(), 1);
}

#[test]
fn the_complete_export_keeps_every_pointer_resolvable() {
    let exported = export_documented(ExportChoices::default());
    let tags = tags(&exported);
    for present in ["NOTE", "SOUR", "REPO", "OBJE"] {
        assert!(tags.contains(&present), "{present} missing:\n{exported}");
    }
    assert_every_pointer_resolves(&exported);
}

// ═══════════════════════════════════════════════════════════════════════
// Files read as they are
// ═══════════════════════════════════════════════════════════════════════

/// `CHAR ASCII` over 8-bit bytes is what Windows software of the GEDCOM 5
/// era writes for its own code page: the accents are Windows-1252, not lost.
#[test]
fn ascii_declared_over_eight_bit_bytes_is_read_as_windows_1252() {
    let bytes = one_person_in("ASCII", b"Ren\xe9e");
    let result =
        oxidgene_gedcom::import::import_gedcom_bytes(&bytes, Uuid::now_v7()).expect("imports");
    assert_eq!(result.person_names[0].given_names.as_deref(), Some("Renée"));
}

/// A GEDCOM 7.0 file: an association's relation is its `ROLE` phrase, else
/// its role; a date known only in words is that phrase; a calendar keyword
/// is the date's calendar.
const GEDCOM_7: &str = "\
0 HEAD
1 GEDC
2 VERS 7.0
0 @I1@ INDI
1 NAME Alpha /Sample/
1 BAPM
2 DATE
3 PHRASE The spring after the flood
2 ASSO @I2@
3 ROLE GODP
2 ASSO @I3@
3 ROLE OTHER
4 PHRASE Midwife
1 DEAT
2 DATE ABT JULIAN 1700
0 @I2@ INDI
1 NAME Beta /Sample/
0 @I3@ INDI
1 NAME Gamma /Sample/
0 TRLR
";

#[test]
fn a_gedcom_7_file_keeps_its_roles_phrases_and_calendar_keywords() {
    let result = import_gedcom(GEDCOM_7, Uuid::now_v7()).expect("imports");
    let baptism = event_of(&result, EventType::Baptism);
    assert_eq!(
        baptism.date_value.as_deref(),
        Some("(The spring after the flood)")
    );
    let relations: Vec<Option<&str>> = result
        .event_witnesses
        .iter()
        .filter(|w| w.event_id == baptism.id)
        .map(|w| w.relation.as_deref())
        .collect();
    assert_eq!(relations, [Some("GODP"), Some("Midwife")]);
    let death = event_of(&result, EventType::Death);
    assert_eq!(death.calendar, oxidgene_core::Calendar::Julian);
    assert_eq!(death.date_qualifier, oxidgene_core::DateQualifier::About);
    assert_eq!(death.date_value.as_deref(), Some("1700"));
}

/// Events some software writes on the other kind of record — a marriage on
/// a person, GeneWeb's `SEP` on a family — are read, and so is an LDS
/// ordinance whose status is outside GEDCOM's enumeration.
const MISPLACED_EVENTS_GEDCOM: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
0 @I1@ INDI
1 NAME Alpha /Sample/
1 MARR
2 DATE 1 MAY 1900
1 BAPL
2 STAT RECORDED
0 @F1@ FAM
1 HUSB @I1@
1 SEP
2 DATE 1910
0 @F2@ FAM
1 SEP Y
1 PROP Farm lease
2 DATE 1923
0 TRLR
";

#[test]
fn misplaced_events_and_an_unlisted_lds_status_are_read() {
    let result = import_gedcom(MISPLACED_EVENTS_GEDCOM, Uuid::now_v7()).expect("imports");
    let marriage = event_of(&result, EventType::Marriage);
    assert_eq!(marriage.date_value.as_deref(), Some("1 MAY 1900"));
    assert!(marriage.person_id.is_some() && marriage.family_id.is_none());
    let separations: Vec<_> = result
        .events
        .iter()
        .filter(|e| e.event_type == EventType::Separation)
        .collect();
    assert_eq!(separations.len(), 2, "a bare `1 SEP` is a separation too");
    assert_eq!(separations[0].date_value.as_deref(), Some("1910"));
    assert!(separations.iter().all(|e| e.family_id.is_some()));
    assert_eq!(separations[1].description, None, "`Y` describes nothing");
    let lease = event_of(&result, EventType::Property);
    assert!(lease.family_id.is_some());

    // Written back, each is the generic event its tag names on a record
    // that does not permit the tag, and reads back the same.
    let exported = reexport(&result);
    assert!(
        exported.contains("1 EVEN\n2 DATE 1 MAY 1900\n2 TYPE MARR\n"),
        "{exported}"
    );
    assert!(
        exported.contains("1 EVEN Farm lease\n2 DATE 1923\n2 TYPE PROP\n"),
        "{exported}"
    );
    let back = import_gedcom(&exported, Uuid::now_v7()).expect("re-imports");
    for event_type in [EventType::Marriage, EventType::Property] {
        let (before, after) = (event_of(&result, event_type), event_of(&back, event_type));
        assert_eq!(
            (
                &after.date_value,
                &after.description,
                after.person_id.is_some()
            ),
            (
                &before.date_value,
                &before.description,
                before.person_id.is_some()
            ),
            "{event_type:?}"
        );
    }
    // An ordinance's status is not its description (the temple and status are
    // kept as GEDCOM beside the event).
    let baptism = event_of(&result, EventType::LdsBaptism);
    assert_eq!(baptism.description, None);
    // An LDS ordinance is written as its own tag even when it holds nothing.
    assert!(exported.contains("\n1 BAPL\n"), "{exported}");
    assert!(!exported.contains("2 TYPE BAPL"), "{exported}");
}

/// `tree` as the GEDCOM 5.5.1 of a `.ged` and as the GEDCOM 7.0 of a
/// GEDZIP, each written with the default choices; and that GEDZIP. With
/// `held`, OxidGene holds every medium the tree names but a web address,
/// which the GEDZIP packs; without, it holds none, and the GEDZIP keeps them
/// as extensions.
fn both_exports(
    tree: &mut oxidgene_gedcom::ImportResult,
    held: bool,
) -> (
    oxidgene_gedcom::ExportResult,
    oxidgene_gedcom::ExportResult,
    Vec<u8>,
) {
    for medium in &mut tree.media {
        if held && !medium.is_document() && !oxidgene_core::types::is_remote_url(&medium.file_path)
        {
            medium.storage_key = Some(format!("media/{}", medium.id));
        }
    }
    let mut media_paths = HashMap::new();
    let mut files = Vec::new();
    for medium in &tree.media {
        if let Some(path) = oxidgene_gedcom::export::archive_path(medium) {
            media_paths.insert(medium.id, path.clone());
            files.push((path, b"BYTES".to_vec()));
        }
    }
    let ged = export_as(tree, ExportTarget::Gedcom);
    let gdz = export_as(
        tree,
        ExportTarget::Gedzip {
            media_paths: &media_paths,
        },
    );
    let archive =
        oxidgene_gedcom::export::export_gedzip(&gdz.gedcom, &files).expect("writes a GEDZIP");
    (ged, gdz, archive)
}

/// A GEDCOM's lines but the `FILE` ones, which a GEDZIP rewrites into its
/// archive entries.
fn without_files(gedcom: &str) -> Vec<&str> {
    gedcom
        .lines()
        .filter(|line| !line.starts_with("1 FILE "))
        .collect()
}

/// `lines` without the attributes and LDS ordinances that say nothing — no
/// value, or nothing beneath (a GeneWeb `#occu` alone, a bare `BAPL`): GEDCOM
/// 7.0 has no form for a structure without one, so a GEDZIP cannot carry it.
fn without_empty_attributes<'l>(lines: &[&'l str]) -> Vec<&'l str> {
    const ATTRIBUTES: [&str; 19] = [
        "CAST", "DSCR", "EDUC", "IDNO", "NATI", "NCHI", "NMR", "OCCU", "PROP", "RELI", "SSN",
        "TITL", "FACT", "BAPL", "CONL", "ENDL", "INIL", "SLGC", "SLGS",
    ];
    lines
        .iter()
        .enumerate()
        .filter(|(i, line)| {
            let empty_attribute = line
                .strip_prefix("1 ")
                .is_some_and(|tag| ATTRIBUTES.contains(&tag));
            let nothing_beneath = lines
                .get(i + 1)
                .is_none_or(|next| next.starts_with("0 ") || next.starts_with("1 "));
            !(empty_attribute && nothing_beneath)
        })
        .map(|(_, line)| *line)
        .collect()
}

/// What GEDCOM 5.5.1 and 7.0 write differently: age keywords, which 7.0
/// writes as durations with a phrase; interpreted dates, dual years, dates
/// before the common era and calendar escapes; text holding `@`; a long
/// note, which only 5.5.1 continues on `CONC` lines.
const VERSION_SENSITIVE_GEDCOM: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
0 @I1@ INDI
1 NAME Alder /Example/
1 DEAT
2 AGE CHILD
1 BURI
2 AGE STILLBORN
1 RESI
2 AGE INFANT
1 CENS
2 AGE < 1y 6m
1 EVEN
2 TYPE Lease
2 DATE INT 1850 (about the harvest)
1 OCCU Weaver
2 DATE (unknown)
1 EDUC Village school
2 DATE 30 JAN 1648/49
1 RELI Example faith
2 DATE 44 B.C.
1 NATI Example land
2 DATE ABT @#DJULIAN@ 1700
1 TITL Example title
2 DATE BET @#DFRENCH R@ 1 VEND 3 AND @#DFRENCH R@ 5 BRUM 4
1 DSCR Tall
2 DATE EST 1801
1 IDNO 12
2 TYPE Register
2 DATE @#DHEBREW@ 5 TSH 5600
1 NOTE Written to someone@example.org about @@ signs
1 NOTE A note long enough to run past the two hundred and fifty five \
characters a GEDCOM 5.5.1 line holds, so that a plain export continues it on \
CONC lines and a GEDZIP writes it whole on one line, the GEDCOM 7.0 way, \
which has no line limit and no CONC at all, and still reads back the same.
0 TRLR
";

/// The `gedcom.ged` of a GEDZIP is GEDCOM 7.0, conformant, and an import of
/// the archive restores exactly what an import of the `.ged` does: every
/// fixture of this file round-trips through both alike.
#[test]
fn a_gedzip_holds_conformant_gedcom_7_that_restores_what_a_gedcom_does() {
    for (name, fixture) in [
        ("MINIMAL", MINIMAL_GEDCOM),
        ("FAMILY", FAMILY_GEDCOM),
        ("SOURCE", SOURCE_GEDCOM),
        ("FREE_TEXT_SOURCE", FREE_TEXT_SOURCE_GEDCOM),
        ("MULTIMEDIA", MULTIMEDIA_GEDCOM),
        ("ASSOCIATION", ASSOCIATION_GEDCOM),
        ("ASSOCIATION_DUPLICATE", ASSOCIATION_DUPLICATE_GEDCOM),
        ("UNREADABLE_AGE", UNREADABLE_AGE_GEDCOM),
        ("NOTE_RECORD", NOTE_RECORD_GEDCOM),
        ("SEVERAL_NOTES", SEVERAL_NOTES_GEDCOM),
        ("CITATION_TEXT", CITATION_TEXT_GEDCOM),
        ("REPOSITORY", REPOSITORY_GEDCOM),
        ("GENEANET", GENEANET_GEDCOM),
        ("MULTI_OCCUPATION", MULTI_OCCUPATION_GEDCOM),
        ("TWO_OCCUPATIONS", TWO_OCCUPATIONS_GEDCOM),
        ("SURNAME_ALIAS", SURNAME_ALIAS_GEDCOM),
        ("GENEWEB_CONVERTED", GENEWEB_CONVERTED_GEDCOM),
        ("RESTRICTION", RESTRICTION_GEDCOM),
        ("COUPLE", COUPLE_GEDCOM),
        ("UNDEFINED_EVENT_TAGS", UNDEFINED_EVENT_TAGS_GEDCOM),
        ("LDS_ORDINANCES", LDS_ORDINANCES_GEDCOM),
        ("DOCUMENTED", DOCUMENTED_GEDCOM),
        ("GEDCOM_7", GEDCOM_7),
        ("MISPLACED_EVENTS", MISPLACED_EVENTS_GEDCOM),
        ("VERSION_SENSITIVE", VERSION_SENSITIVE_GEDCOM),
    ] {
        let mut tree = import_gedcom(fixture, Uuid::now_v7()).expect("imports");
        for held in [false, true] {
            gedzip_restores_what_a_gedcom_does(name, &mut tree, held);
        }
    }
}

/// [`a_gedzip_holds_conformant_gedcom_7_that_restores_what_a_gedcom_does`]
/// for one fixture, its media `held` or not.
fn gedzip_restores_what_a_gedcom_does(
    name: &str,
    tree: &mut oxidgene_gedcom::ImportResult,
    held: bool,
) {
    {
        let (ged, gdz, archive) = both_exports(tree, held);

        assert!(ged.gedcom.contains("2 VERS 5.5.1\n"), "{name}");
        assert!(
            gdz.gedcom
                .starts_with("\u{feff}0 HEAD\n1 GEDC\n2 VERS 7.0\n"),
            "{name}: {}",
            gdz.gedcom
        );
        let deviations = oxidged::spec::validate_bytes(gdz.gedcom.as_bytes());
        assert!(
            deviations.is_empty(),
            "{name}: {deviations:?}\n{}",
            gdz.gedcom
        );

        let from_ged = import_gedcom(&ged.gedcom, Uuid::now_v7()).expect("imports the .ged");
        let from_gdz = oxidgene_gedcom::import::import_gedzip(&archive, Uuid::now_v7())
            .expect("imports the GEDZIP");
        assert_eq!(
            from_gdz.files.len(),
            tree.media
                .iter()
                .filter(|m| m.storage_key.is_some())
                .count(),
            "{name}: every packed file comes back"
        );
        // Packed, a file is named by its archive entry; kept, by its path.
        let (from_gdz, from_ged) = (reexport(&from_gdz.result), reexport(&from_ged));
        let (from_gdz, from_ged) = if held {
            (without_files(&from_gdz), without_files(&from_ged))
        } else {
            (from_gdz.lines().collect(), from_ged.lines().collect())
        };
        assert_eq!(
            from_gdz,
            without_empty_attributes(&from_ged),
            "{name}, held: {held}"
        );
        assert!(gdz.warnings.is_empty(), "{name}: {:?}", gdz.warnings);
    }
}

/// OxidGene's media extensions are documented extension tags of a GEDZIP's
/// GEDCOM 7.0: its header declares each with the URI of its documentation.
#[test]
fn a_gedzip_declares_oxidgenes_extensions() {
    let mut tree = import_gedcom(MULTIMEDIA_GEDCOM, Uuid::now_v7()).expect("imports");
    let (ged, gdz, _) = both_exports(&mut tree, true);
    for tag in [
        "_OXIDGENE_MEDIA",
        "_OXIDGENE_DOC",
        "_OXIDGENE_VIGNETTE",
        "_OXIDGENE_PERSON",
        "_OXIDGENE_OBJE",
        "_OXIDGENE_POS",
    ] {
        let declaration = format!(
            "2 TAG {tag} https://github.com/trois-six/oxidgene/blob/main/docs/api.md#{}\n",
            tag.to_ascii_lowercase()
        );
        assert!(gdz.gedcom.contains(&declaration), "{}", gdz.gedcom);
    }
    assert!(gdz.gedcom.contains("1 _OXIDGENE_MEDIA {"), "{}", gdz.gedcom);
    assert!(!ged.gedcom.contains("SCHMA"), "5.5.1 has no schema");
}

/// A person with three media: one under a Windows path, one under a
/// relative path climbing out of the file's folder, one at a web address.
const THREE_MEDIA_GEDCOM: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
0 @I1@ INDI
1 NAME Ann /Example/
1 OBJE @M1@
1 OBJE @M2@
1 OBJE @M3@
0 @M1@ OBJE
1 FILE C:\\Photos\\portrait 1.jpg
2 FORM jpg
0 @M2@ OBJE
1 FILE ../scans/register.png
2 FORM png
0 @M3@ OBJE
1 FILE https://example.org/media/portrait.jpg
2 FORM jpg
0 TRLR
";

/// `THREE_MEDIA_GEDCOM` imported with the scan held, and its archive path.
fn tree_holding_the_scan() -> (oxidgene_gedcom::ImportResult, Uuid, String) {
    let mut tree = import_gedcom(THREE_MEDIA_GEDCOM, Uuid::now_v7()).expect("imports");
    let scan = tree
        .media
        .iter_mut()
        .find(|m| m.file_path.ends_with("register.png"))
        .expect("the scan");
    scan.storage_key = Some("media/scan".to_string());
    let path = oxidgene_gedcom::export::archive_path(scan).expect("held media have a path");
    let held = scan.id;
    (tree, held, path)
}

/// `tree` exported with the default choices as `target` says.
fn export_as(
    tree: &oxidgene_gedcom::ImportResult,
    target: ExportTarget<'_>,
) -> oxidgene_gedcom::ExportResult {
    export_gedcom(
        &tree.records(),
        &ExportOptions {
            choices: ExportChoices::default(),
            target,
            self_person_id: None,
            submitter: Default::default(),
        },
    )
    .expect("exports")
}

/// The file paths of the pages of `result`, sorted.
fn page_paths(result: &oxidgene_gedcom::ImportResult) -> Vec<String> {
    let mut paths: Vec<String> = result
        .media
        .iter()
        .filter(|m| !m.is_document())
        .map(|m| m.file_path.clone())
        .collect();
    paths.sort();
    paths
}

/// The file names of the pages of `result`, in its order.
fn page_names(result: &oxidgene_gedcom::ImportResult) -> Vec<String> {
    result
        .media
        .iter()
        .filter(|m| !m.is_document())
        .map(|m| m.file_name.clone())
        .collect()
}

/// A medium OxidGene holds no bytes for cannot be an `OBJE` of a GEDZIP,
/// which names the files it carries and web addresses only: the GEDZIP keeps
/// it, its path and its links as OxidGene extensions, conformant, and an
/// import restores it as the `.ged` does — the portrait, written first,
/// included.
#[test]
fn a_gedzip_keeps_the_media_it_cannot_carry_as_extensions() {
    let (tree, held, path) = tree_holding_the_scan();
    let media_paths = HashMap::from([(held, path.clone())]);
    let gdz = export_as(
        &tree,
        ExportTarget::Gedzip {
            media_paths: &media_paths,
        },
    );
    for written in [
        "1 _OXIDGENE_OBJE @M1@\n2 _OXIDGENE_POS 0\n1 OBJE @M2@\n1 OBJE @M3@\n",
        "0 @M1@ _OXIDGENE_OBJE\n1 _OXIDGENE_POS 0\n1 FILE C:\\Photos\\portrait 1.jpg\n",
        "0 @M3@ OBJE\n1 FILE https://example.org/media/portrait.jpg\n",
    ] {
        assert!(gdz.gedcom.contains(written), "{written} in {}", gdz.gedcom);
    }
    assert!(gdz.gedcom.contains(&format!("1 FILE {path}\n")));
    for absent in ["file:", ".."] {
        assert!(!gdz.gedcom.contains(absent), "{absent} in {}", gdz.gedcom);
    }
    assert!(gdz.warnings.is_empty(), "{:?}", gdz.warnings);
    assert!(oxidged::spec::validate_bytes(gdz.gedcom.as_bytes()).is_empty());

    let archive = oxidgene_gedcom::export::export_gedzip(&gdz.gedcom, &[(path, b"PNG".to_vec())])
        .expect("writes a GEDZIP");
    let back = oxidgene_gedcom::import::import_gedzip(&archive, Uuid::now_v7()).expect("imports");
    assert_eq!(back.files.len(), 1);
    assert!(
        back.result.warnings.is_empty(),
        "{:?}",
        back.result.warnings
    );
    let from_ged = import_gedcom(
        &export_as(&tree, ExportTarget::Gedcom).gedcom,
        Uuid::now_v7(),
    )
    .expect("imports the .ged");
    assert_eq!(page_paths(&back.result).len(), 3);
    assert!(page_paths(&back.result).contains(&"C:\\Photos\\portrait 1.jpg".to_string()));
    assert_eq!(back.result.media_links.len(), 3);
    assert_restored_as_the_ged_reads_them(&back.result, &from_ged);
}

/// The media a GEDZIP kept as extensions, restored in `back` as `from_ged`,
/// the import of the `.ged` of the same tree, reads them: each page where it
/// was written, in the order of the `OBJE` records, and the portrait, written
/// first, the unheld one.
fn assert_restored_as_the_ged_reads_them(
    back: &oxidgene_gedcom::ImportResult,
    from_ged: &oxidgene_gedcom::ImportResult,
) {
    assert_eq!(page_names(back), page_names(from_ged));
    let portrait = |result: &oxidgene_gedcom::ImportResult| {
        let id = result.persons[0].portrait_media_id.expect("a portrait");
        result
            .media
            .iter()
            .find(|m| m.parent_media_id == Some(id))
            .map(|m| m.file_path.clone())
    };
    assert_eq!(portrait(back), portrait(from_ged));
    assert_eq!(
        portrait(back).as_deref(),
        Some("C:\\Photos\\portrait 1.jpg")
    );
}

/// The destination shapes a `.ged` only: a GEDZIP is standard GEDCOM 7.0,
/// keeps every profession and every name whatever the destination, and
/// reports nothing.
#[test]
fn a_gedzip_reads_no_destination() {
    let mut tree = import_gedcom(MULTI_OCCUPATION_GEDCOM, Uuid::now_v7()).expect("imports");
    let (_, lossless, _) = both_exports(&mut tree, true);
    let merged = export_gedcom(
        &tree.records(),
        &ExportOptions {
            choices: ExportChoices {
                destination: destination("geneanet").expect("a oxidged export profile"),
                ..ExportChoices::default()
            },
            target: ExportTarget::Gedzip {
                media_paths: &HashMap::new(),
            },
            self_person_id: None,
            submitter: Default::default(),
        },
    )
    .expect("exports");
    assert_eq!(merged.gedcom, lossless.gedcom);
    assert!(merged.report.is_none());
    assert!(
        merged.gedcom.matches("1 OCCU ").count() > 1,
        "{}",
        merged.gedcom
    );
}

/// The family `@F2@` lists only one spouse and no child; the child `@I3@`
/// names it on their own record, `FAMC` with `PEDI adopted`.
const ONE_SIDED_CHILD_GEDCOM: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
2 FORM LINEAGE-LINKED
1 CHAR UTF-8
0 @I2@ INDI
1 NAME Beth /Example/
2 GIVN Beth
2 SURN Example
1 SEX F
1 FAMS @F2@
0 @I3@ INDI
1 NAME Cy /Example/
2 GIVN Cy
2 SURN Example
1 SEX M
1 FAMC @F2@
2 PEDI adopted
0 @F2@ FAM
1 WIFE @I2@
0 TRLR
";

/// The family `@F3@` lists no spouse; the individual `@I1@` names it on their
/// own record, `FAMS` only.
const ONE_SIDED_SPOUSE_GEDCOM: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
2 FORM LINEAGE-LINKED
1 CHAR UTF-8
0 @I1@ INDI
1 NAME Dan /Example/
2 GIVN Dan
2 SURN Example
1 SEX M
1 FAMS @F3@
0 @F3@ FAM
0 TRLR
";

/// The records of an export with a line equal to `needle`, each without its leading
/// `0 ` (the split keeps the xref and tag at the start of the record).
fn records_containing<'a>(gedcom: &'a str, needle: &str) -> Vec<&'a str> {
    gedcom
        .split("\n0 ")
        .filter(|record| record.lines().any(|line| line == needle))
        .collect()
}

/// The xref a record opens with, such as `@I3@`.
fn record_xref(record: &str) -> &str {
    record
        .split_whitespace()
        .next()
        .expect("a record opens with its xref")
}

/// A child the family does not list is linked as its child, with the `PEDI`
/// of their `FAMC` link, and the export writes both sides of the link.
#[test]
fn a_child_named_only_by_their_famc_is_linked_with_its_pedi() {
    let tree_id = Uuid::now_v7();
    let result = import_gedcom(ONE_SIDED_CHILD_GEDCOM, tree_id).expect("imports");

    let cy = result.person_by_xref["@I3@"];
    let family = result.families[0].id;
    let links: Vec<_> = result
        .family_children
        .iter()
        .filter(|fc| fc.person_id == cy && fc.family_id == family)
        .collect();
    assert_eq!(links.len(), 1, "{:?}", result.family_children.len());
    assert_eq!(links[0].child_type, oxidgene_core::ChildType::Adopted);

    let mentions: Vec<_> = result
        .warnings
        .iter()
        .filter(|warning| warning.contains("FAMC"))
        .collect();
    assert_eq!(mentions.len(), 1, "{:?}", result.warnings);

    let exported = reexport(&result);
    let person = records_containing(&exported, "1 NAME Cy /Example/");
    assert_eq!(person.len(), 1, "{exported}");
    let person_xref = record_xref(person[0]);
    let child_of = records_containing(&exported, &format!("1 CHIL {person_xref}"));
    assert_eq!(child_of.len(), 1, "{exported}");
    assert!(
        child_of[0].starts_with('@') && child_of[0].contains(" FAM"),
        "{exported}"
    );
    let family_xref = record_xref(child_of[0]);
    assert!(
        person[0].contains(&format!("1 FAMC {family_xref}\n2 PEDI adopted")),
        "{exported}"
    );
}

/// An individual naming a family that does not list them as a spouse is
/// linked as its husband, with a warning.
#[test]
fn a_spouse_named_only_by_their_fams_is_linked() {
    let tree_id = Uuid::now_v7();
    let result = import_gedcom(ONE_SIDED_SPOUSE_GEDCOM, tree_id).expect("imports");

    let dan = result.person_by_xref["@I1@"];
    let family = result.families[0].id;
    let spouses: Vec<_> = result
        .family_spouses
        .iter()
        .filter(|fs| fs.person_id == dan && fs.family_id == family)
        .collect();
    assert_eq!(spouses.len(), 1, "{:?}", result.family_spouses);
    assert_eq!(spouses[0].role, oxidgene_core::SpouseRole::Husband);
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("FAMS")),
        "{:?}",
        result.warnings
    );
}

/// A two-sided file keeps one row per link: a child the family lists and the
/// individual names is one `FamilyChild`, and no link raises a warning.
#[test]
fn a_two_sided_family_link_is_one_row_without_a_warning() {
    let result = import_gedcom(FAMILY_GEDCOM, Uuid::now_v7()).expect("imports");

    assert_eq!(result.family_children.len(), 1);
    assert_eq!(result.family_spouses.len(), 2);
    let linked: Vec<_> = result
        .warnings
        .iter()
        .filter(|warning| warning.contains("FAMC") || warning.contains("FAMS"))
        .collect();
    assert!(linked.is_empty(), "{linked:?}");
}

/// A 5.5.1 file whose structures OxidGene's model has no place for, on an
/// individual, a family, a second submitter and an extension record.
const KEPT_GEDCOM: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
2 FORM LINEAGE-LINKED
1 CHAR UTF-8
1 SUBM @U1@
0 @U1@ SUBM
1 NAME Sample Submitter
0 @U9@ SUBM
1 NAME Other
0 @X1@ _CUSTOM
1 _SUB value
0 @I1@ INDI
1 NAME Ann /Example/
2 FONE An
3 TYPE kana
1 REFN 42
1 CHAN
2 DATE 1 JAN 2000
1 SEX F
1 BIRT
2 DATE 1900
2 ADDR 1 Main Street
2 PLAC Town
3 FONE Taun
4 TYPE kana
1 BAPL
2 DATE 1 JAN 1950
2 TEMP SLAKE
2 STAT COMPLETED
3 DATE 2 JAN 1950
1 FAMC @F1@
2 STAT proven
0 @F1@ FAM
1 CHIL @I1@
0 TRLR
";

/// The structures of `gedcom` that are not records, as their lines.
fn lines_of(gedcom: &str) -> Vec<&str> {
    gedcom.lines().collect()
}

#[test]
fn kept_structures_are_written_back_on_export() {
    let imported = import_gedcom(KEPT_GEDCOM, Uuid::now_v7()).expect("imports");
    let kinds: Vec<_> = imported.kept.iter().map(|row| row.entity_kind).collect();
    assert!(kinds.contains(&oxidgene_core::types::KeptEntityKind::Person));
    assert!(kinds.contains(&oxidgene_core::types::KeptEntityKind::Record));

    let export = export_as(&imported, ExportTarget::Gedcom);
    let gedcom = export.gedcom.as_str();
    for expected in [
        "1 REFN 42\n",
        "2 FONE An\n",
        "2 ADDR 1 Main Street\n",
        "3 FONE Taun\n",
        "2 STAT proven\n",
        "2 TEMP SLAKE\n",
        "2 STAT COMPLETED\n",
        "1 _SUB value\n",
    ] {
        assert!(gedcom.contains(expected), "{expected} missing:\n{gedcom}");
    }
    let other = records_containing(gedcom, "1 NAME Other");
    assert_eq!(other.len(), 1, "{gedcom}");
    assert!(
        other[0].starts_with("@K") && other[0].contains(" SUBM"),
        "{other:?}"
    );
    assert!(
        records_containing(gedcom, "1 _SUB value")
            .iter()
            .any(|record| record.contains("_CUSTOM")),
        "{gedcom}"
    );
    assert!(
        export.warnings.iter().all(|w| !w.contains("left out")),
        "{:?}",
        export.warnings
    );
}

#[test]
fn kept_structures_survive_a_second_round_trip() {
    let first = export_as(
        &import_gedcom(KEPT_GEDCOM, Uuid::now_v7()).expect("imports"),
        ExportTarget::Gedcom,
    );
    let imported_again = import_gedcom(&first.gedcom, Uuid::now_v7()).expect("imports");
    let second = export_as(&imported_again, ExportTarget::Gedcom);
    assert_eq!(first.gedcom, second.gedcom);
}

#[test]
fn a_kept_structure_whose_entity_is_left_out_is_warned_about() {
    let mut imported = import_gedcom(KEPT_GEDCOM, Uuid::now_v7()).expect("imports");
    imported
        .events
        .retain(|event| event.event_type != oxidgene_core::EventType::Birth);
    let export = export_as(&imported, ExportTarget::Gedcom);
    assert!(!export.gedcom.contains("3 FONE Taun"), "{}", export.gedcom);
    assert!(
        export
            .warnings
            .iter()
            .any(|w| w.contains("kept GEDCOM structures had no place left")),
        "{:?}",
        export.warnings
    );
}

#[test]
fn kept_notes_are_left_out_with_the_notes() {
    let gedcom = KEPT_GEDCOM.replace("1 REFN 42\n", "1 REFN 42\n2 NOTE On the reference\n");
    let imported = import_gedcom(&gedcom, Uuid::now_v7()).expect("imports");
    let with_notes = export_as(&imported, ExportTarget::Gedcom);
    // 5.5.1 has no note under a `REFN`, so the writer keeps it as an extension.
    assert!(
        with_notes
            .gedcom
            .contains("1 REFN 42\n2 _NOTE On the reference\n")
    );

    let without_notes = export_gedcom(
        &imported.records(),
        &ExportOptions {
            choices: ExportChoices {
                include_notes_and_sources: false,
                ..ExportChoices::default()
            },
            target: ExportTarget::Gedcom,
            self_person_id: None,
            submitter: Default::default(),
        },
    )
    .expect("exports");
    assert!(without_notes.gedcom.contains("1 REFN 42\n"));
    assert!(!without_notes.gedcom.contains("On the reference"));
}

#[test]
fn kept_7_0_structures_are_written_by_a_gedzip() {
    const GEDCOM_70: &str = "\
0 HEAD
1 GEDC
2 VERS 7.0
0 @I1@ INDI
1 NAME John /Doe/
2 GIVN John
2 SURN Doe
1 SEX M
1 UID 0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d
1 EXID 123
2 TYPE http://example.org
1 NO MARR
0 TRLR
";
    let imported = import_gedcom(GEDCOM_70, Uuid::now_v7()).expect("imports");
    assert!(!imported.kept.is_empty());
    let archive = export_as(
        &imported,
        ExportTarget::Gedzip {
            media_paths: &HashMap::new(),
        },
    );
    for expected in [
        "1 UID 0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d",
        "1 EXID 123",
        "2 TYPE http://example.org",
        "1 NO MARR",
    ] {
        assert!(
            archive.gedcom.contains(expected),
            "{expected} missing:\n{}",
            archive.gedcom
        );
    }
    let as_551 = export_as(&imported, ExportTarget::Gedcom);
    let back = import_gedcom(&as_551.gedcom, Uuid::now_v7()).expect("parses back");
    assert_eq!(back.persons.len(), 1);
    assert!(
        lines_of(&as_551.gedcom)
            .iter()
            .any(|line| line.starts_with("0 TRLR"))
    );
}

#[test]
fn a_record_without_xref_is_kept_whole_and_written_without_one() {
    const GEDCOM: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
1 CHAR UTF-8
0 @I1@ INDI
1 NAME John /Doe/
0 _CUSTOMREC
1 _SUB value
0 TRLR
";
    let imported = import_gedcom(GEDCOM, Uuid::now_v7()).expect("imports");
    let export = export_as(&imported, ExportTarget::Gedcom);
    let lines = lines_of(&export.gedcom);
    let at = lines
        .iter()
        .position(|line| *line == "0 _CUSTOMREC")
        .unwrap_or_else(|| panic!("_CUSTOMREC written without an xref:\n{}", export.gedcom));
    assert_eq!(lines[at + 1], "1 _SUB value");
    assert!(
        !lines
            .iter()
            .any(|line| line.starts_with("0 @K") && line.ends_with(" _CUSTOMREC")),
        "a record without an xref must not gain one:\n{}",
        export.gedcom
    );
}

#[test]
fn a_shared_note_nobody_points_at_is_kept_as_a_record() {
    const GEDCOM: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
1 CHAR UTF-8
0 @I1@ INDI
1 NAME John /Doe/
0 @N1@ NOTE Shared text
0 TRLR
";
    let imported = import_gedcom(GEDCOM, Uuid::now_v7()).expect("imports");
    let export = export_as(&imported, ExportTarget::Gedcom);
    let lines = lines_of(&export.gedcom);
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("0 @K") && line.ends_with(" NOTE Shared text")),
        "the unreferenced note is written back as a record:\n{}",
        export.gedcom
    );
    let without_notes = ExportChoices {
        include_notes_and_sources: false,
        ..ExportChoices::default()
    };
    let export = export_gedcom(
        &imported.records(),
        &ExportOptions {
            choices: without_notes,
            target: ExportTarget::Gedcom,
            self_person_id: None,
            submitter: Default::default(),
        },
    )
    .expect("exports");
    assert!(
        !export.gedcom.contains("Shared text"),
        "a note the choices leave out is not written:\n{}",
        export.gedcom
    );
}

// ── Crops: identifications as GEDCOM's standard CROP ────────────────────

/// Two people, a birth and two pictures, anonymized: the first person is born
/// in the file, and the pictures are a group photograph and another scan.
const CROP_TREE: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
1 CHAR UTF-8
0 @I1@ INDI
1 NAME Alpha /Sample/
1 SEX M
1 BIRT
2 DATE 1850
0 @I2@ INDI
1 NAME Beta /Sample/
1 SEX F
0 @M1@ OBJE
1 FILE group.jpg
2 FORM jpg
0 @M2@ OBJE
1 FILE other.jpg
2 FORM jpg
0 TRLR
";

/// The page of the picture `file` in `result`.
fn page_named(result: &oxidgene_gedcom::ImportResult, file: &str) -> Uuid {
    result
        .media
        .iter()
        .find(|m| !m.is_document() && m.file_name == file)
        .unwrap_or_else(|| panic!("page {file} missing"))
        .id
}

fn person_named(result: &oxidgene_gedcom::ImportResult, given: &str) -> Uuid {
    result
        .person_names
        .iter()
        .find(|n| n.given_names.as_deref() == Some(given))
        .unwrap_or_else(|| panic!("person {given} missing"))
        .person_id
}

fn given_name(result: &oxidgene_gedcom::ImportResult, person_id: Uuid) -> String {
    result
        .person_names
        .iter()
        .find(|n| n.person_id == person_id)
        .and_then(|n| n.given_names.clone())
        .unwrap_or_default()
}

/// Each identification as one line: its page's file, its rectangle, the
/// person and the event it names. Sorted, so that order does not matter.
fn identifications(result: &oxidgene_gedcom::ImportResult) -> Vec<String> {
    let mut lines: Vec<String> = result
        .vignettes
        .iter()
        .map(|vignette| {
            let file = result
                .media
                .iter()
                .find(|m| m.id == vignette.media_id)
                .map_or("?", |m| m.file_name.as_str());
            let person = vignette
                .person_id
                .map_or("-".to_owned(), |id| given_name(result, id));
            let event = vignette
                .event_id
                .and_then(|id| result.events.iter().find(|e| e.id == id))
                .map_or("-".to_owned(), |e| format!("{:?}", e.event_type));
            format!(
                "{file} {} {} {} {} {person} {event}",
                vignette.x, vignette.y, vignette.width, vignette.height
            )
        })
        .collect();
    lines.sort();
    lines
}

/// Each person with a portrait, and whether it is a whole picture or a crop.
fn portraits(result: &oxidgene_gedcom::ImportResult) -> Vec<String> {
    let mut lines: Vec<String> = result
        .persons
        .iter()
        .filter_map(|person| {
            let kind = if person.portrait_vignette_id.is_some() {
                "crop"
            } else if person.portrait_media_id.is_some() {
                "whole"
            } else {
                return None;
            };
            Some(format!("{} {kind}", given_name(result, person.id)))
        })
        .collect();
    lines.sort();
    lines
}

/// [`CROP_TREE`] with every kind of identification: a person's, an event's,
/// both at once, neither, and a portrait cut from the group photograph. Beta
/// is portrayed by the crop; Alpha by the whole of the other scan, which
/// their crops follow.
fn tree_with_every_crop() -> oxidgene_gedcom::ImportResult {
    let mut tree = import_gedcom(CROP_TREE, Uuid::now_v7()).expect("imports");
    let alpha = person_named(&tree, "Alpha");
    let beta = person_named(&tree, "Beta");
    let birth = event_of(&tree, EventType::Birth).id;
    let group = page_named(&tree, "group.jpg");
    let other = page_named(&tree, "other.jpg");
    let other_document = tree
        .media
        .iter()
        .find(|m| {
            m.is_document()
                && Some(m.id)
                    == tree
                        .media
                        .iter()
                        .find(|p| p.id == other)
                        .and_then(|p| p.parent_media_id)
        })
        .expect("the document of the other scan")
        .id;
    let now = Utc::now();
    let vignette = |media_id, (x, y, width, height), person_id, event_id| Vignette {
        id: Uuid::now_v7(),
        media_id,
        x,
        y,
        width,
        height,
        person_id,
        event_id,
        created_at: now,
        updated_at: now,
    };
    let portrait = vignette(group, (7, 8, 9, 10), Some(beta), None);
    let other_whole = MediaLink {
        id: Uuid::now_v7(),
        media_id: other_document,
        person_id: Some(alpha),
        event_id: None,
        source_id: None,
        family_id: None,
        sort_order: 0,
    };
    for person in &mut tree.persons {
        if person.id == beta {
            person.portrait_vignette_id = Some(portrait.id);
        }
        if person.id == alpha {
            person.portrait_media_id = Some(other_document);
        }
    }
    tree.media_links.push(other_whole);
    tree.vignettes.extend([
        vignette(group, (120, 45, 64, 82), Some(alpha), None),
        vignette(other, (10, 20, 30, 40), None, Some(birth)),
        vignette(other, (5, 6, 70, 80), Some(alpha), Some(birth)),
        vignette(other, (1, 2, 3, 4), None, None),
        portrait,
    ]);
    tree
}

/// The export of `tree` as GEDCOM 7.0, with default options otherwise.
fn export_70(tree: &oxidgene_gedcom::ImportResult) -> String {
    export_gedcom(
        &tree.records(),
        &ExportOptions {
            choices: ExportChoices {
                destination: destination("generic-70").expect("the 7.0 destination"),
                ..ExportChoices::default()
            },
            target: ExportTarget::Gedcom,
            self_person_id: None,
            submitter: Default::default(),
        },
    )
    .expect("exports")
    .gedcom
}

/// Every identification is written as the standard `CROP` beneath the link
/// to its page (5.5.1 writes it as `_CROP`, which the standard lacks there),
/// the person of an event's crop as an `_OXIDGENE_PERSON` pointer, and the
/// crop without a person or an event beneath the page's `OBJE` record.
#[test]
fn identifications_are_written_as_crops_beneath_their_links() {
    let mut tree = tree_with_every_crop();
    let (ged, gdz, _) = both_exports(&mut tree, true);
    let v70 = export_70(&tree);

    assert!(
        ged.gedcom.contains("\n2 _CROP\n3 TOP 45\n"),
        "{}",
        ged.gedcom
    );
    assert!(v70.contains("\n2 CROP\n3 TOP 45\n"), "{v70}");
    assert!(
        gdz.gedcom.contains("\n2 CROP\n3 TOP 45\n"),
        "{}",
        gdz.gedcom
    );
    assert!(
        ged.gedcom.contains("\n3 _OXIDGENE_PERSON @I1@\n"),
        "{}",
        ged.gedcom
    );
    assert!(
        gdz.gedcom.contains("\n3 _OXIDGENE_PERSON @I1@\n"),
        "{}",
        gdz.gedcom
    );
    assert!(
        ged.gedcom.contains("\n1 _OXIDGENE_VIGNETTE 1 2 3 4\n"),
        "{}",
        ged.gedcom
    );
    assert!(
        !ged.gedcom.contains("_OXIDGENE_VIGNETTE @"),
        "the person field of the old extension is gone: {}",
        ged.gedcom
    );
}

/// Every kind of identification comes back as it was, from 5.5.1, from 7.0
/// and from a GEDZIP holding its pictures or only naming them.
#[test]
fn identifications_read_back_identically_from_each_format() {
    for held in [true, false] {
        let mut tree = tree_with_every_crop();
        let expected = identifications(&tree);
        let expected_portraits = portraits(&tree);
        let (ged, gdz, archive) = both_exports(&mut tree, held);
        let v70 = export_70(&tree);
        if !held {
            assert!(
                gdz.gedcom.contains("\n2 CROP\n") && gdz.gedcom.contains("_OXIDGENE_OBJE"),
                "unheld pictures are kept links, crop and all: {}",
                gdz.gedcom
            );
        }

        let from_551 = import_gedcom(&ged.gedcom, Uuid::now_v7()).expect("5.5.1 imports");
        let from_70 = import_gedcom(&v70, Uuid::now_v7()).expect("7.0 imports");
        let from_gdz = oxidgene_gedcom::import::import_gedzip(&archive, Uuid::now_v7())
            .expect("GEDZIP imports")
            .result;

        for (name, back) in [
            ("5.5.1", &from_551),
            ("7.0", &from_70),
            ("GEDZIP", &from_gdz),
        ] {
            assert_eq!(identifications(back), expected, "{name}, held: {held}");
            assert_eq!(portraits(back), expected_portraits, "{name}, held: {held}");
            assert!(back.warnings.is_empty(), "{name}: {:?}", back.warnings);
        }
    }
}

/// A crop portrait is the person's `portrait_vignette_id` in each format.
#[test]
fn a_crop_portrait_reads_back_as_the_portrait_in_each_format() {
    let mut tree = tree_with_every_crop();
    let (ged, _, archive) = both_exports(&mut tree, true);
    let v70 = export_70(&tree);
    let results = [
        import_gedcom(&ged.gedcom, Uuid::now_v7()).expect("5.5.1 imports"),
        import_gedcom(&v70, Uuid::now_v7()).expect("7.0 imports"),
        oxidgene_gedcom::import::import_gedzip(&archive, Uuid::now_v7())
            .expect("GEDZIP imports")
            .result,
    ];
    for (name, back) in ["5.5.1", "7.0", "GEDZIP"].iter().zip(&results) {
        let beta = back
            .persons
            .iter()
            .find(|p| given_name(back, p.id) == "Beta")
            .expect("Beta came back");
        let portrait = beta
            .portrait_vignette_id
            .unwrap_or_else(|| panic!("{name}: no portrait crop"));
        let vignette = back
            .vignettes
            .iter()
            .find(|v| v.id == portrait)
            .expect("the portrait is an identification");
        assert_eq!(
            (vignette.x, vignette.y, vignette.width, vignette.height),
            (7, 8, 9, 10),
            "{name}"
        );
        assert_eq!(beta.portrait_media_id, None, "{name}");
    }
}

/// A `CROP` another program writes is an identification, and the first `OBJE`
/// of a person decides the portrait: a crop there makes it a crop, a whole
/// picture a whole picture.
#[test]
fn a_crop_written_by_other_software_is_an_identification_and_the_first_picture_is_the_portrait() {
    const CROP_FIRST: &str = "\
0 HEAD
1 GEDC
2 VERS 7.0
1 CHAR UTF-8
0 @I1@ INDI
1 NAME Gamma /Sample/
1 OBJE @M1@
2 CROP
3 TOP 10
3 LEFT 20
3 HEIGHT 30
3 WIDTH 40
0 @M1@ OBJE
1 FILE group.jpg
2 FORM image/jpeg
0 TRLR
";
    let imported = import_gedcom(CROP_FIRST, Uuid::now_v7()).expect("imports");
    assert_eq!(imported.vignettes.len(), 1, "{:?}", imported.warnings);
    assert!(imported.media_links.is_empty(), "a crop is not a link");
    let vignette = &imported.vignettes[0];
    assert_eq!(
        (vignette.x, vignette.y, vignette.width, vignette.height),
        (20, 10, 40, 30),
        "LEFT and TOP are the origin, WIDTH and HEIGHT the size"
    );
    assert_eq!(vignette.person_id, Some(imported.persons[0].id));
    assert_eq!(vignette.event_id, None);
    assert_eq!(
        imported.persons[0].portrait_vignette_id,
        Some(vignette.id),
        "the first picture is the portrait"
    );

    let whole_first = CROP_FIRST
        .replace("1 OBJE @M1@\n2 CROP", "1 OBJE @M2@\n1 OBJE @M1@\n2 CROP")
        .replace(
            "0 TRLR\n",
            "0 @M2@ OBJE\n1 FILE other.jpg\n2 FORM image/jpeg\n0 TRLR\n",
        );
    let imported = import_gedcom(&whole_first, Uuid::now_v7()).expect("imports");
    assert_eq!(imported.vignettes.len(), 1, "{:?}", imported.warnings);
    assert_eq!(
        imported.persons[0].portrait_media_id,
        imported.media_links.first().map(|link| link.media_id),
        "the whole picture written first is the portrait"
    );
    assert_eq!(imported.persons[0].portrait_vignette_id, None);
}

/// A crop with no WIDTH on a picture whose size is unknown names no
/// rectangle: the link stays the whole picture, with a warning, and its crop
/// is kept, so that the export writes it back.
#[test]
fn a_crop_naming_no_width_on_an_unsized_picture_is_kept_as_written() {
    const NO_WIDTH: &str = "\
0 HEAD
1 GEDC
2 VERS 7.0
1 CHAR UTF-8
0 @I1@ INDI
1 NAME Delta /Sample/
1 OBJE @M1@
2 CROP
3 TOP 10
3 LEFT 20
3 HEIGHT 30
0 @M1@ OBJE
1 FILE group.jpg
2 FORM image/jpeg
0 TRLR
";
    let imported = import_gedcom(NO_WIDTH, Uuid::now_v7()).expect("imports");
    assert!(imported.vignettes.is_empty(), "no rectangle can be named");
    assert_eq!(imported.media_links.len(), 1, "the whole picture is linked");
    assert!(
        imported.warnings.iter().any(|w| w.contains("WIDTH")),
        "{:?}",
        imported.warnings
    );
    let exported = export_70(&imported);
    assert!(
        exported.contains("\n2 CROP\n3 TOP 10\n3 LEFT 20\n3 HEIGHT 30\n"),
        "the kept crop is written back: {exported}"
    );
}

/// A crop beneath a person and the same crop beneath one of that person's
/// events are one identification, which carries the event.
#[test]
fn a_crop_beneath_a_person_and_the_same_crop_beneath_their_event_are_one_identification() {
    const SAME_CROP: &str = "\
0 HEAD
1 GEDC
2 VERS 7.0
1 CHAR UTF-8
0 @I1@ INDI
1 NAME Alpha /Sample/
1 OBJE @M1@
2 CROP
3 TOP 1
3 LEFT 2
3 HEIGHT 30
3 WIDTH 40
1 BIRT
2 DATE 1850
2 OBJE @M1@
3 CROP
4 TOP 1
4 LEFT 2
4 HEIGHT 30
4 WIDTH 40
3 _OXIDGENE_PERSON @I1@
0 @M1@ OBJE
1 FILE group.jpg
2 FORM image/jpeg
0 TRLR
";
    let imported = import_gedcom(SAME_CROP, Uuid::now_v7()).expect("imports");
    assert_eq!(imported.vignettes.len(), 1, "{:?}", imported.warnings);
    let vignette = &imported.vignettes[0];
    let birth = event_of(&imported, EventType::Birth).id;
    assert_eq!(vignette.event_id, Some(birth));
    assert_eq!(vignette.person_id, Some(imported.persons[0].id));
    assert_eq!(imported.persons[0].portrait_vignette_id, Some(vignette.id));
}

/// Export, import and export again give the same text for a tree with every
/// kind of identification.
#[test]
fn identifications_export_idempotently() {
    let tree = tree_with_every_crop();
    // The header names the producer that wrote the file read, so it is left out.
    let without_header = |text: &str| {
        text.find("\n0 @")
            .map_or(String::new(), |i| text[i..].to_owned())
    };
    let first = reexport(&tree);
    let again = reexport(&import_gedcom(&first, Uuid::now_v7()).expect("imports"));
    assert_eq!(without_header(&first), without_header(&again));
    let first_70 = export_70(&tree);
    let again_70 = export_70(&import_gedcom(&first_70, Uuid::now_v7()).expect("imports"));
    assert_eq!(without_header(&first_70), without_header(&again_70));
}
