//! Round-trip fidelity of the export: each test imports a small anonymized
//! GEDCOM, exports it, and checks what the export writes and what a second
//! import reads back.

use oxidgene_core::{EventType, Sex};
use oxidgene_gedcom::ImportResult;
use oxidgene_gedcom::export::{
    ExportChoices, ExportOptions, ExportTarget, SubmitterSettings, destination, export_gedcom,
};
use oxidgene_gedcom::import::import_gedcom_bytes;
use uuid::Uuid;

fn import(text: &str) -> ImportResult {
    import_gedcom_bytes(text.as_bytes(), Uuid::now_v7()).expect("the text imports")
}

/// The import exported for `destination_id` with default options.
fn export_for(imported: &ImportResult, destination_id: &str) -> String {
    let options = ExportOptions {
        choices: ExportChoices {
            destination: destination(destination_id).expect("a known destination"),
            ..ExportChoices::default()
        },
        target: ExportTarget::Gedcom,
        self_person_id: None,
        submitter: SubmitterSettings::default(),
    };
    export_gedcom(&imported.records(), &options)
        .expect("the export writes")
        .gedcom
}

/// The export of `text` (5.5.1 unless `destination_id` says otherwise), and
/// the export of its own re-import, for an idempotence check.
fn round_trip(text: &str, destination_id: &str) -> (String, String) {
    let first = export_for(&import(text), destination_id);
    let second = export_for(&import(&first), destination_id);
    (first, second)
}

/// The text without its header record, which carries the export date.
fn body(text: &str) -> String {
    let mut in_header = false;
    let mut out = String::new();
    for line in text.lines() {
        if line.starts_with("0 ") {
            in_header = line == "0 HEAD";
        }
        if !in_header {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// The record starting with `0 <xref> <tag>`, up to the next level-0 line.
fn record<'t>(text: &'t str, head: &str) -> &'t str {
    let start = text
        .find(&format!("\n{head}\n"))
        .or_else(|| text.starts_with(&format!("{head}\n")).then_some(0))
        .unwrap_or_else(|| panic!("no record {head} in:\n{text}"));
    let rest = &text[start..];
    let body_start = rest.find('\n').map_or(rest.len(), |i| i + 1);
    let end = rest[body_start..]
        .find("\n0 ")
        .map_or(rest.len(), |i| body_start + i + 1);
    &rest[..end]
}

const HEADER_551: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
2 FORM LINEAGE-LINKED
1 CHAR UTF-8
";

const HEADER_70: &str = "\
0 HEAD
1 GEDC
2 VERS 7.0
1 CHAR UTF-8
";

fn gedcom_551(records: &str) -> String {
    format!("{HEADER_551}{records}0 TRLR\n")
}

fn gedcom_70(records: &str) -> String {
    format!("{HEADER_70}{records}0 TRLR\n")
}

// 1. LDS ordinances stay ordinances, even when empty.

#[test]
fn an_empty_lds_ordinance_is_written_as_its_bare_tag() {
    let text = gedcom_551(
        "\
0 @I1@ INDI
1 NAME Ann /Lane/
2 GIVN Ann
2 SURN Lane
1 SEX F
1 CONL
",
    );
    let export = export_for(&import(&text), "generic-551");
    let indi = record(&export, "0 @I1@ INDI");
    assert!(indi.contains("\n1 CONL\n"), "{indi}");
    assert!(!indi.contains("EVEN"), "{indi}");
}

// 2. A family's NCHI is written as NCHI.

#[test]
fn a_family_nchi_is_written_as_nchi_in_551_and_70() {
    let text = gedcom_551(
        "\
0 @I1@ INDI
1 NAME Ann /Lane/
2 GIVN Ann
2 SURN Lane
1 SEX F
1 FAMS @F1@
0 @F1@ FAM
1 WIFE @I1@
1 NCHI 3
",
    );
    let imported = import(&text);
    let fam = imported
        .events
        .iter()
        .find(|e| e.event_type == EventType::ChildrenCount)
        .expect("the NCHI reads as ChildrenCount on the family");
    assert!(fam.family_id.is_some());
    assert_eq!(fam.description.as_deref(), Some("3"));

    let export_551 = export_for(&imported, "generic-551");
    assert!(export_551.contains("\n1 NCHI 3\n"), "{export_551}");
    assert!(!export_551.contains("_NCHI"), "{export_551}");

    let export_70 = export_for(
        &import(&gedcom_70(
            "\
0 @I1@ INDI
1 NAME Ann /Lane/
2 GIVN Ann
2 SURN Lane
1 SEX F
1 FAMS @F1@
0 @F1@ FAM
1 WIFE @I1@
1 NCHI 3
",
        )),
        "generic-70",
    );
    assert!(export_70.contains("\n1 NCHI 3\n"), "{export_70}");
    assert!(!export_70.contains("_NCHI"), "{export_70}");
    let again = import(&export_70);
    assert!(
        again
            .events
            .iter()
            .any(|e| { e.event_type == EventType::ChildrenCount && e.family_id.is_some() })
    );
}

// 3. Family links are written in a fixed order.

const LINKS: &str = "\
0 @I1@ INDI
1 NAME Ann /Lane/
2 GIVN Ann
2 SURN Lane
1 SEX F
1 FAMS @F19@
1 FAMS @F2@
0 @I2@ INDI
1 NAME Ben /Lane/
2 GIVN Ben
2 SURN Lane
1 SEX M
1 FAMS @F2@
0 @I3@ INDI
1 NAME Cal /Lane/
2 GIVN Cal
2 SURN Lane
1 SEX M
1 FAMC @F19@
1 FAMC @F2@
0 @I4@ INDI
1 NAME Dee /Lane/
2 GIVN Dee
2 SURN Lane
1 SEX F
1 FAMC @F2@
0 @F2@ FAM
1 HUSB @I2@
1 WIFE @I1@
1 CHIL @I4@
1 CHIL @I3@
0 @F19@ FAM
1 WIFE @I1@
1 CHIL @I3@
";

#[test]
fn family_links_are_written_in_xref_order_and_stable_on_reimport() {
    let (first, second) = round_trip(&gedcom_551(LINKS), "generic-551");
    assert_eq!(body(&first), body(&second));
    // The export numbers families itself, so the links are checked by their
    // numbers: each person's links must be in ascending order.
    for (head, tag) in [("0 @I1@ INDI", "FAMS"), ("0 @I3@ INDI", "FAMC")] {
        let person = record(&first, head);
        {
            let numbers: Vec<u32> = person
                .lines()
                .filter_map(|line| line.strip_prefix(&format!("1 {tag} @F")))
                .filter_map(|rest| rest.strip_suffix('@')?.parse().ok())
                .collect();
            let mut sorted = numbers.clone();
            sorted.sort_unstable();
            assert!(!numbers.is_empty(), "{tag} missing in {head}:\n{person}");
            assert_eq!(
                numbers, sorted,
                "{tag} out of xref order in {head}:\n{person}"
            );
        }
    }
}

// 4. A citation's first TEXT is its text.

#[test]
fn a_citation_reads_its_first_text_and_keeps_the_rest_apart() {
    let text = gedcom_551(
        "\
0 @I1@ INDI
1 NAME Ann /Lane/
2 GIVN Ann
2 SURN Lane
1 SEX F
1 SOUR @S1@
2 PAGE p. 4
2 DATA
3 TEXT one
3 TEXT two
0 @S1@ SOUR
1 TITL Parish register
",
    );
    let first = import(&text);
    let cite = first
        .citations
        .iter()
        .find(|c| c.person_id.is_some())
        .expect("a citation");
    assert_eq!(cite.text.as_deref(), Some("one"));
    let export = export_for(&first, "generic-551");
    let again = import(&export);
    let cite = again
        .citations
        .iter()
        .find(|c| c.person_id.is_some())
        .expect("a citation");
    assert_eq!(cite.text.as_deref(), Some("one"));
    assert!(export.contains("3 TEXT one"), "{export}");
    assert!(export.contains("3 TEXT two"), "{export}");
    assert!(!export.contains("CONT Text two"), "{export}");
}

// 5. BASM is written as BASM whatever the sex.

#[test]
fn a_bat_mitzvah_is_written_under_the_tag_it_was_read_from() {
    let text = gedcom_551(
        "\
0 @I1@ INDI
1 NAME Ann /Lane/
2 GIVN Ann
2 SURN Lane
1 SEX F
1 BASM
2 DATE 4 APR 1860
0 @I2@ INDI
1 NAME Ben /Lane/
2 GIVN Ben
2 SURN Lane
1 SEX M
1 BARM
2 DATE 5 MAY 1861
",
    );
    let imported = import(&text);
    assert!(
        imported
            .events
            .iter()
            .any(|e| e.event_type == EventType::BasMitzvah)
    );
    let export = export_for(&imported, "generic-551");
    let ann = record(&export, "0 @I1@ INDI");
    assert!(ann.contains("\n1 BASM\n"), "{ann}");
    let ben = record(&export, "0 @I2@ INDI");
    assert!(ben.contains("\n1 BARM\n"), "{ben}");
    assert!(!export.contains("BASM") || !ben.contains("BASM"));
    // Mitzvah events of a man stay BARM even if their sex were changed.
    let reimport = import(&export);
    assert!(
        reimport
            .events
            .iter()
            .any(|e| { e.event_type == EventType::BasMitzvah && e.person_id.is_some() })
    );
    assert_eq!(
        reimport
            .persons
            .iter()
            .find(|p| p.sex == Sex::Female)
            .map(|p| p.sex),
        Some(Sex::Female)
    );
}

// 6. INDI.SOUR with a PAGE in GEDCOM 7.0.

#[test]
fn an_individual_citation_with_a_page_survives_in_70() {
    let text = gedcom_70(
        "\
0 @I1@ INDI
1 NAME Ann /Lane/
2 GIVN Ann
2 SURN Lane
1 SEX F
1 SOUR @S1@
2 PAGE p. 9
0 @S1@ SOUR
1 TITL Census book
",
    );
    let export = export_for(&import(&text), "generic-70");
    let ann = record(&export, "0 @I1@ INDI");
    assert!(ann.contains("\n1 SOUR @S1@\n2 PAGE p. 9\n"), "{ann}");
}

// 7. Text-only citations in 5.5.1.

#[test]
fn a_text_only_citation_keeps_its_text_across_a_round_trip() {
    let text = gedcom_551(
        "\
0 @I1@ INDI
1 NAME Ann /Lane/
2 GIVN Ann
2 SURN Lane
1 SEX F
1 SOUR Parish book of 1850
2 TEXT Entry for the baptism
",
    );
    let first = import(&text);
    let cite = first
        .citations
        .iter()
        .find(|c| c.person_id.is_some())
        .expect("a citation");
    assert_eq!(cite.text.as_deref(), Some("Entry for the baptism"));
    let export = export_for(&first, "generic-551");
    assert!(export.contains("3 TEXT Entry for the baptism"), "{export}");
    let again = import(&export);
    let cite = again
        .citations
        .iter()
        .find(|c| c.person_id.is_some())
        .expect("a citation");
    assert_eq!(cite.text.as_deref(), Some("Entry for the baptism"));
}

// 7. An adoption and a child sealing name the family they do not belong to.

const ADOPTED_AND_SEALED: &str = "\
0 @I1@ INDI
1 NAME Ann /Lane/
2 GIVN Ann
2 SURN Lane
1 SEX F
0 @I2@ INDI
1 NAME Ben /Lane/
2 GIVN Ben
2 SURN Lane
1 SEX M
0 @I3@ INDI
1 NAME Cy /Lane/
2 GIVN Cy
2 SURN Lane
1 SEX M
1 ADOP
2 FAMC @F2@
3 ADOP BOTH
1 SLGC
2 DATE 1 JAN 1950
2 FAMC @F1@
0 @I4@ INDI
1 NAME Dee /Lane/
2 GIVN Dee
2 SURN Lane
1 SEX M
0 @F1@ FAM
1 HUSB @I2@
1 WIFE @I1@
0 @F2@ FAM
1 HUSB @I4@
";

/// The family whose spouse has the given first name in `imported`.
fn family_with_spouse(imported: &ImportResult, given: &str) -> Uuid {
    let person = imported
        .person_names
        .iter()
        .find(|n| n.given_names.as_deref() == Some(given))
        .unwrap_or_else(|| panic!("no person named {given}"))
        .person_id;
    imported
        .family_spouses
        .iter()
        .find(|s| s.person_id == person)
        .unwrap_or_else(|| panic!("{given} is no spouse"))
        .family_id
}

/// The text after `marker` up to the next line of the same or a higher level.
fn beneath<'t>(text: &'t str, marker: &str) -> &'t str {
    let start = text
        .find(marker)
        .unwrap_or_else(|| panic!("no {marker:?} in:\n{text}"))
        + marker.len();
    let rest = &text[start..];
    let end = rest.find("\n1 ").unwrap_or(rest.len());
    &rest[..end]
}

#[test]
fn an_adoption_and_a_child_sealing_name_the_family_they_do_not_belong_to() {
    let text = gedcom_551(ADOPTED_AND_SEALED);
    let imported = import(&text);
    let adoptive = family_with_spouse(&imported, "Dee");
    let sealed_to = family_with_spouse(&imported, "Ben");

    let adoption = imported
        .events
        .iter()
        .find(|e| e.event_type == EventType::Adoption)
        .expect("the adoption imports");
    assert_eq!(adoption.related_family_id, Some(adoptive));
    assert_eq!(adoption.family_id, None);
    let sealing = imported
        .events
        .iter()
        .find(|e| e.event_type == EventType::SealingChild)
        .expect("the sealing imports");
    assert_eq!(sealing.related_family_id, Some(sealed_to));
    assert_eq!(sealing.family_id, None);

    let export = export_for(&imported, "generic-551");
    let cy = record(&export, "0 @I3@ INDI");
    let adop = beneath(cy, "\n1 ADOP\n");
    assert!(adop.starts_with("2 FAMC @F"), "{cy}");
    assert!(adop.contains("\n3 ADOP BOTH"), "{cy}");
    let slgc = beneath(cy, "\n1 SLGC\n");
    assert!(slgc.contains("\n2 FAMC @F"), "{cy}");
    assert!(!export.contains("_SLGC"), "{export}");

    // The pointers name the same families when read back.
    let reimport = import(&export);
    let adoption = reimport
        .events
        .iter()
        .find(|e| e.event_type == EventType::Adoption)
        .expect("the adoption re-imports");
    assert_eq!(
        adoption.related_family_id,
        Some(family_with_spouse(&reimport, "Dee"))
    );
    let sealing = reimport
        .events
        .iter()
        .find(|e| e.event_type == EventType::SealingChild)
        .expect("the sealing re-imports");
    assert_eq!(
        sealing.related_family_id,
        Some(family_with_spouse(&reimport, "Ben"))
    );
}

#[test]
fn an_adoption_family_round_trips_to_the_same_export() {
    let text = gedcom_551(ADOPTED_AND_SEALED);
    let (first, second) = round_trip(&text, "generic-551");
    assert_eq!(body(&first), body(&second));
}

// 2. A witness is written beneath its event in GEDCOM 7, where the event is.

/// Witnesses of an individual's birth and of a family's marriage. A 5.5.1
/// file names them beneath the event in which they are read.
const WITNESSES_551: &str = "\
0 @I1@ INDI
1 NAME Ann /Lane/
2 GIVN Ann
2 SURN Lane
1 SEX F
1 BIRT
2 DATE 1 JAN 1900
2 ASSO @I3@
3 RELA Godparent
1 FAMS @F1@
0 @I2@ INDI
1 NAME Ben /Lane/
2 GIVN Ben
2 SURN Lane
1 SEX M
1 FAMS @F1@
0 @I3@ INDI
1 NAME Cal /Lane/
2 GIVN Cal
2 SURN Lane
1 SEX M
0 @F1@ FAM
1 HUSB @I2@
1 WIFE @I1@
1 MARR
2 DATE 1 MAY 1920
2 ASSO @I3@
3 RELA Witness
";

#[test]
fn a_witness_is_written_beneath_its_event_in_70_and_beneath_the_person_in_551() {
    let imported = import(&gedcom_551(WITNESSES_551)[..].replace("0 TRLR\n", ""));
    assert_eq!(
        imported.event_witnesses.len(),
        2,
        "one per event, not twice"
    );

    let v551 = export_for(&imported, "generic-551");
    let person = record(&v551, "0 @I1@ INDI");
    assert!(
        person.contains("\n1 ASSO @I3@\n2 RELA Godparent\n"),
        "{person}"
    );
    let witness = record(&v551, "0 @I3@ INDI");
    assert!(
        witness.contains("\n1 _ASSO @F1@\n2 RELA Witness\n"),
        "{witness}"
    );
    assert!(!record(&v551, "0 @F1@ FAM").contains("ASSO"), "{v551}");

    let v70 = export_for(&imported, "generic-70");
    let person = record(&v70, "0 @I1@ INDI");
    assert!(
        !person.contains("ASSO @I3@\n2 "),
        "a 7.0 witness is not a level-1 ASSO: {person}"
    );
    let birth = person.split("\n1 BIRT\n").nth(1).expect("the birth");
    assert!(
        birth.contains("\n2 ASSO @I3@\n3 ROLE GODP\n4 PHRASE Godparent\n"),
        "{person}"
    );
    let family = record(&v70, "0 @F1@ FAM");
    let marriage = family.split("\n1 MARR\n").nth(1).expect("the marriage");
    assert!(
        marriage.contains("\n2 ASSO @I3@\n3 ROLE WITN\n4 PHRASE Witness\n"),
        "{family}"
    );
    assert!(!v70.contains("_ASSO"), "{v70}");
    assert!(!record(&v70, "0 @I3@ INDI").contains("ASSO"), "{v70}");

    // The 7.0 export reads back to the same witnesses, and to the same export.
    let (first, second) = round_trip(
        &gedcom_551(WITNESSES_551)[..].replace("0 TRLR\n", ""),
        "generic-70",
    );
    let unmarked = |text: &str| body(text.trim_start_matches('\u{feff}'));
    assert_eq!(unmarked(&first), unmarked(&second));
    let back = import(&first);
    assert_eq!(back.event_witnesses.len(), 2, "{first}");
    let relations: Vec<Option<String>> = back
        .event_witnesses
        .iter()
        .map(|w| w.relation.clone())
        .collect();
    assert!(
        relations.contains(&Some("Godparent".to_string())),
        "{relations:?}"
    );
    assert!(
        relations.contains(&Some("Witness".to_string())),
        "{relations:?}"
    );
}

// 3. Call numbers of one repository keep what each holds beneath them.

/// Two call numbers under one `REPO` (the second with a medium and its phrase)
/// and a call number under no repository (`REPO @VOID@`).
const CALL_NUMBERS_70: &str = "\
0 @R1@ REPO
1 NAME Sample Archive
0 @S1@ SOUR
1 TITL Sample register
1 REPO @R1@
2 CALN CALL-1
3 MEDI BOOK
1 REPO @R1@
2 CALN CALL-2
3 MEDI OTHER
4 PHRASE Wax cylinder
1 REPO @VOID@
2 CALN CALL-3
";

#[test]
fn a_medium_phrase_stays_with_its_own_call_number_in_70() {
    let imported = import(&gedcom_70(CALL_NUMBERS_70));
    let exported = export_for(&imported, "generic-70");
    let source = record(&exported, "0 @S1@ SOUR");
    assert!(
        source.contains("1 REPO @R1@\n2 CALN CALL-2\n3 MEDI OTHER\n4 PHRASE Wax cylinder\n"),
        "{source}"
    );
    assert!(
        source.contains("1 REPO @VOID@\n2 CALN CALL-3\n"),
        "{source}"
    );
    assert!(
        source.contains("1 REPO @R1@\n2 CALN CALL-1\n3 MEDI BOOK\n"),
        "{source}"
    );
    let back = export_for(&import(&exported), "generic-70");
    assert_eq!(body(&exported), body(&back));
}

// 4. A family's attributes read as an individual's: value and TYPE apart.

#[test]
fn a_family_count_and_fact_keep_their_type_beside_their_value_in_70() {
    let text = gedcom_70(
        "\
0 @I1@ INDI
1 NAME Ann /Lane/
2 GIVN Ann
2 SURN Lane
1 SEX F
1 FAMS @F1@
0 @F1@ FAM
1 WIFE @I1@
1 NCHI 3
2 TYPE Living children
1 FACT Sample family fact
2 TYPE Pets
",
    );
    let imported = import(&text);
    let exported = export_for(&imported, "generic-70");
    let family = record(&exported, "0 @F1@ FAM");
    assert!(
        family.contains("\n1 NCHI 3\n2 TYPE Living children\n"),
        "{family}"
    );
    assert!(
        family.contains("\n1 FACT Sample family fact\n2 TYPE Pets\n"),
        "{family}"
    );
    assert!(!family.contains("EVEN"), "{family}");
}

// 8. One person in two roles at one event is two witnesses, and stays so.

/// A birth naming one person twice: as godparent, as witness, and once more
/// as godparent in another letter case, which is the same role.
const TWO_ROLES_551: &str = "\
0 @I1@ INDI
1 NAME Ann /Lane/
2 GIVN Ann
2 SURN Lane
1 SEX F
1 BIRT
2 DATE 1 JAN 1900
2 ASSO @I3@
3 RELA Godparent
2 ASSO @I3@
3 RELA Witness
2 ASSO @I3@
3 RELA godparent
1 FAMS @F1@
0 @I2@ INDI
1 NAME Ben /Lane/
2 GIVN Ben
2 SURN Lane
1 SEX M
1 FAMS @F1@
0 @I3@ INDI
1 NAME Cal /Lane/
2 GIVN Cal
2 SURN Lane
1 SEX M
0 @F1@ FAM
1 HUSB @I2@
1 WIFE @I1@
";

#[test]
fn a_person_in_two_roles_at_one_event_is_two_witnesses_on_every_export() {
    let text = gedcom_551(TWO_ROLES_551);
    let imported = import(&text);
    let mut relations: Vec<String> = imported
        .event_witnesses
        .iter()
        .map(|w| w.relation.clone().unwrap_or_default())
        .collect();
    relations.sort();
    assert_eq!(
        relations,
        ["Godparent", "Witness"],
        "{:?}",
        imported.event_witnesses
    );

    // 7.0 nests each ASSO beneath the birth, one per role.
    let v70 = export_for(&imported, "generic-70");
    let person = record(&v70, "0 @I1@ INDI");
    let birth = person.split("\n1 BIRT\n").nth(1).expect("the birth");
    assert!(birth.contains("\n2 ASSO @I3@\n3 ROLE GODP\n"), "{person}");
    assert!(birth.contains("\n2 ASSO @I3@\n3 ROLE WITN\n"), "{person}");
    assert_eq!(birth.matches("\n2 ASSO @I3@\n").count(), 2, "{person}");

    // 5.5.1 writes each as an INDI-level ASSO with its RELA.
    let v551 = export_for(&imported, "generic-551");
    let person = record(&v551, "0 @I1@ INDI");
    assert!(
        person.contains("\n1 ASSO @I3@\n2 RELA Godparent\n"),
        "{person}"
    );
    assert!(
        person.contains("\n1 ASSO @I3@\n2 RELA Witness\n"),
        "{person}"
    );

    // Re-importing the 7.0 export reads the same two witnesses, and a second
    // export of it is the same: no witness is added on each pass.
    let (first, second) = round_trip(&text, "generic-70");
    let back = import(&first);
    assert_eq!(back.event_witnesses.len(), 2, "{first}");
    let again = import(&second);
    assert_eq!(again.event_witnesses.len(), 2, "{second}");
    let unmarked = |text: &str| body(text.trim_start_matches('\u{feff}'));
    assert_eq!(unmarked(&first), unmarked(&second));

    // The 5.5.1 export reads back to the same two witnesses too.
    let back551 = import(&export_for(&imported, "generic-551"));
    assert_eq!(back551.event_witnesses.len(), 2, "{v551}");
}

// 9. A text-only citation's TEXT is written once, and under DATA.

#[test]
fn a_text_only_citation_writes_its_text_once_and_never_as_an_underscore_text() {
    let text = gedcom_551(
        "\
0 @I1@ INDI
1 NAME Ann /Lane/
2 GIVN Ann
2 SURN Lane
1 SEX F
1 BIRT
2 DATE 1 JAN 1900
1 SOUR Parish book of 1850
2 TEXT Born at home
",
    );
    let export = export_for(&import(&text), "generic-551");
    assert_eq!(export.matches("Born at home").count(), 1, "{export}");
    assert!(!export.contains("_TEXT"), "{export}");
    assert!(
        export.contains("\n2 DATA\n3 TEXT Born at home\n"),
        "{export}"
    );

    // The re-import reads the same citation text, and the export is stable.
    let (first, second) = round_trip(&text, "generic-551");
    let unmarked = |text: &str| body(text.trim_start_matches('\u{feff}'));
    assert_eq!(unmarked(&first), unmarked(&second));
}
