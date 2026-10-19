//! Tags beneath persons, couples and documents, as `_OXIDGENE_TAG` lines of a
//! `.ged` and of a GEDZIP (docs/api.md, OxidGene extensions).

use std::collections::HashMap;

use chrono::Utc;
use oxidgene_core::enums::{Privacy, Sex};
use oxidgene_core::types::{DOCUMENT_MIME, Family, Media, Person, TagRef};
use uuid::Uuid;

use crate::export::{
    ExportChoices, ExportOptions, ExportRecords, ExportTarget, SubmitterSettings, archive_path,
    export_gedcom, export_gedzip,
};
use crate::import::{import_gedcom, import_gedzip};

fn tag(label: &str, color: i16) -> TagRef {
    TagRef {
        id: Uuid::now_v7(),
        label: label.to_string(),
        color,
    }
}

/// The `(label, colour)` pairs of `tags`, in order.
fn spelled(tags: &[TagRef]) -> Vec<(String, i16)> {
    tags.iter()
        .map(|tag| (tag.label.clone(), tag.color))
        .collect()
}

fn options(target: ExportTarget<'_>) -> ExportOptions<'_> {
    ExportOptions {
        choices: ExportChoices::default(),
        target,
        self_person_id: None,
        submitter: SubmitterSettings::default(),
    }
}

/// A document of one stored page, carrying `tags`, and the page.
fn stored_document(tree_id: Uuid, tags: Vec<TagRef>) -> (Media, Media) {
    let now = Utc::now();
    let document = Media {
        id: Uuid::now_v7(),
        tree_id,
        file_name: "parish-register.jpg".to_string(),
        mime_type: DOCUMENT_MIME.to_string(),
        file_path: String::new(),
        storage_key: None,
        sha256: None,
        thumbnail_key: None,
        thumbnail_url: None,
        width: None,
        height: None,
        page_count: 1,
        parent_media_id: None,
        page_index: 0,
        title: Some("Parish register".to_string()),
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
        tags,
        place_id: None,
        created_at: now,
        updated_at: now,
        deleted_at: None,
    };
    let page = Media {
        id: Uuid::now_v7(),
        file_name: "page-1.jpg".to_string(),
        mime_type: "image/jpeg".to_string(),
        file_path: "C:\\Scans\\page-1.jpg".to_string(),
        storage_key: Some("ab/cdef".to_string()),
        parent_media_id: Some(document.id),
        tags: Vec::new(),
        title: None,
        document_category: None,
        ..document.clone()
    };
    (document, page)
}

/// A person, a couple and a document, each carrying tags with colours.
struct Sample {
    person: Person,
    family: Family,
    document: Media,
    page: Media,
}

fn sample() -> Sample {
    let tree_id = Uuid::now_v7();
    let now = Utc::now();
    let person = Person {
        id: Uuid::now_v7(),
        tree_id,
        sex: Sex::Unknown,
        privacy: Privacy::Public,
        portrait_media_id: None,
        portrait_vignette_id: None,
        tags: vec![tag("Hérault", 3), tag("To check", 7)],
        created_at: now,
        updated_at: now,
        deleted_at: None,
    };
    let family = Family {
        id: Uuid::now_v7(),
        tree_id,
        privacy: Privacy::Public,
        tags: vec![tag("Witnessed", 1)],
        created_at: now,
        updated_at: now,
        deleted_at: None,
    };
    let (document, page) = stored_document(tree_id, vec![tag("Parish register", 9)]);
    Sample {
        person,
        family,
        document,
        page,
    }
}

/// The `.ged` text of `sample`, written for a plain `.ged`.
fn export_ged(sample: &Sample) -> String {
    let rows = [sample.document.clone(), sample.page.clone()];
    export_gedcom(
        &ExportRecords {
            persons: std::slice::from_ref(&sample.person),
            families: std::slice::from_ref(&sample.family),
            media: &rows,
            ..Default::default()
        },
        &options(ExportTarget::Gedcom),
    )
    .expect("exports")
    .gedcom
}

/// No structure of the file is kept: the submitter's row is always kept, and
/// empty.
fn assert_nothing_kept(kept: &[oxidgene_core::types::GedcomKept]) {
    assert!(
        kept.iter().all(|row| row.nodes == "[]"),
        "kept beyond the submitter: {kept:?}"
    );
}

/// A file with one person and one couple carrying folded-alike labels.
const DUPLICATE_LABELS: &str = "0 HEAD\n\
1 GEDC\n\
2 VERS 5.5.1\n\
2 FORM LINEAGE-LINKED\n\
1 CHAR UTF-8\n\
0 @I1@ INDI\n\
1 NAME Sample /Person/\n\
1 _OXIDGENE_TAG {\"v\":1,\"label\":\"a verifier\",\"color\":2}\n\
1 _OXIDGENE_TAG {\"v\":1,\"label\":\"À vérifier\",\"color\":5}\n\
0 @F1@ FAM\n\
1 HUSB @I1@\n\
1 _OXIDGENE_TAG {\"v\":1,\"label\":\"A VERIFIER\",\"color\":9}\n\
0 TRLR\n";

#[test]
fn person_couple_and_document_tags_survive_a_ged_round_trip() {
    let sample = sample();
    let text = export_ged(&sample);
    assert!(text.contains("_OXIDGENE_TAG"), "the tags are written");

    let back = import_gedcom(&text, Uuid::now_v7()).expect("imports");
    let person = back.persons.first().expect("the person came back");
    assert_eq!(
        spelled(&person.tags),
        spelled(&sample.person.tags),
        "a person's tags, colours included"
    );
    let family = back.families.first().expect("the couple came back");
    assert_eq!(spelled(&family.tags), spelled(&sample.family.tags));
    let document = back
        .media
        .iter()
        .find(|item| item.is_document())
        .expect("the document came back");
    assert_eq!(
        spelled(&document.tags),
        spelled(&sample.document.tags),
        "a document's tags sit beneath its first page"
    );
    assert_nothing_kept(&back.kept);
}

#[test]
fn person_couple_and_document_tags_survive_a_gedzip_round_trip() {
    let sample = sample();
    let archive_path = archive_path(&sample.page).expect("stored media has an archive path");
    let media_paths = HashMap::from([(sample.page.id, archive_path.clone())]);
    let rows = [sample.document.clone(), sample.page.clone()];
    let export = export_gedcom(
        &ExportRecords {
            persons: std::slice::from_ref(&sample.person),
            families: std::slice::from_ref(&sample.family),
            media: &rows,
            ..Default::default()
        },
        &options(ExportTarget::Gedzip {
            media_paths: &media_paths,
        }),
    )
    .expect("exports");
    let archive = export_gedzip(&export.gedcom, &[(archive_path, b"IMAGE BYTES".to_vec())])
        .expect("creates GEDZIP");
    let back = import_gedzip(&archive, Uuid::now_v7())
        .expect("imports GEDZIP")
        .result;

    assert_eq!(spelled(&back.persons[0].tags), spelled(&sample.person.tags));
    assert_eq!(
        spelled(&back.families[0].tags),
        spelled(&sample.family.tags)
    );
    let document = back
        .media
        .iter()
        .find(|item| item.is_document())
        .expect("the document came back");
    assert_eq!(spelled(&document.tags), spelled(&sample.document.tags));
    assert_nothing_kept(&back.kept);
}

#[test]
fn a_tagged_document_with_no_page_is_not_exported_and_says_so() {
    let tree_id = Uuid::now_v7();
    let (document, _) = stored_document(tree_id, vec![tag("Parish register", 9)]);
    let rows = [document.clone()];
    let export = export_gedcom(
        &ExportRecords {
            media: &rows,
            ..Default::default()
        },
        &options(ExportTarget::Gedcom),
    )
    .expect("exports");

    assert!(
        !export.gedcom.contains("_OXIDGENE_TAG"),
        "no OBJE carries the empty document's tags"
    );
    assert_eq!(
        export.warnings,
        [format!(
            "Media {} tags are not exported: the document has no page to carry them",
            document.id
        )],
        "the loss is reported"
    );
}

#[test]
fn a_tagged_document_with_a_page_exports_without_warning() {
    let sample = sample();
    let rows = [sample.document.clone(), sample.page.clone()];
    let export = export_gedcom(
        &ExportRecords {
            media: &rows,
            ..Default::default()
        },
        &options(ExportTarget::Gedcom),
    )
    .expect("exports");

    assert!(
        export.gedcom.contains("_OXIDGENE_TAG"),
        "the tags are written"
    );
    assert!(
        export.warnings.is_empty(),
        "nothing is lost: {:?}",
        export.warnings
    );
}

#[test]
fn labels_that_fold_alike_are_one_tag_with_the_first_spelling_and_colour() {
    let back = import_gedcom(DUPLICATE_LABELS, Uuid::now_v7()).expect("imports");

    assert_eq!(back.tags.len(), 1, "one vocabulary tag for three spellings");
    let person = &back.persons[0];
    assert_eq!(person.tags.len(), 1, "the second spelling is the same tag");
    assert_eq!(spelled(&person.tags), [("a verifier".to_string(), 2)]);
    let family = &back.families[0];
    assert_eq!(
        family.tags.iter().map(|tag| tag.id).collect::<Vec<_>>(),
        person.tags.iter().map(|tag| tag.id).collect::<Vec<_>>(),
        "the couple shares the tag of the person"
    );
    assert_nothing_kept(&back.kept);
}

#[test]
fn a_re_export_writes_each_tag_once_and_keeps_nothing() {
    let imported = import_gedcom(DUPLICATE_LABELS, Uuid::now_v7()).expect("imports");
    let export =
        export_gedcom(&imported.records(), &options(ExportTarget::Gedcom)).expect("exports");

    assert_eq!(
        export.gedcom.matches("_OXIDGENE_TAG").count(),
        2,
        "one line for the person's tag and one for the couple's"
    );
    let again = import_gedcom(&export.gedcom, Uuid::now_v7()).expect("imports again");
    assert_eq!(
        spelled(&again.persons[0].tags),
        [("a verifier".to_string(), 2)]
    );
    assert_nothing_kept(&again.kept);
}

/// A tag line beneath an event is neither read nor written, so it is kept
/// whole as untyped GEDCOM. A standalone `OBJE` is a document page of its own
/// on import, so its tag line is read.
#[test]
fn a_tag_line_beneath_an_event_is_kept_untyped() {
    const FILE: &str = "0 HEAD\n\
1 GEDC\n\
2 VERS 5.5.1\n\
2 FORM LINEAGE-LINKED\n\
1 CHAR UTF-8\n\
0 @I1@ INDI\n\
1 NAME Sample /Person/\n\
1 BIRT\n\
2 DATE 1900\n\
2 _OXIDGENE_TAG {\"v\":1,\"label\":\"Under an event\",\"color\":4}\n\
0 @M1@ OBJE\n\
1 FILE photo.jpg\n\
1 _OXIDGENE_TAG {\"v\":1,\"label\":\"Standalone\",\"color\":6}\n\
0 TRLR\n";
    let back = import_gedcom(FILE, Uuid::now_v7()).expect("imports");

    assert!(
        back.persons.first().is_some_and(|p| p.tags.is_empty()),
        "no tag is read beneath an event"
    );
    assert!(
        back.kept
            .iter()
            .any(|row| row.nodes.contains("Under an event")),
        "the tag line beneath the event is kept"
    );
    assert!(
        back.kept
            .iter()
            .all(|row| !row.nodes.contains("Standalone")),
        "the standalone OBJE's tag line is read, not kept"
    );
    assert!(
        back.tags.iter().any(|tag| tag.label == "Standalone"),
        "the standalone OBJE's tag is read into the vocabulary"
    );
}

/// A `_OXIDGENE_TAG` line without a value is neither read nor kept, so the
/// import says so in a warning rather than dropping it silently.
#[test]
fn an_empty_tag_line_is_skipped_with_a_warning() {
    const FILE: &str = "0 HEAD\n\
1 GEDC\n\
2 VERS 5.5.1\n\
2 FORM LINEAGE-LINKED\n\
1 CHAR UTF-8\n\
0 @M1@ OBJE\n\
1 FILE photo.jpg\n\
1 _OXIDGENE_TAG\n\
0 TRLR\n";
    let back = import_gedcom(FILE, Uuid::now_v7()).expect("imports");

    assert!(
        back.tags.is_empty(),
        "an empty tag line adds nothing to the vocabulary"
    );
    assert!(
        back.warnings
            .iter()
            .any(|warning| warning.starts_with("Empty _OXIDGENE_TAG line")),
        "the skipped empty line is reported: {:?}",
        back.warnings
    );
}
