//! Tags beneath persons, couples, documents, events, sources, citations, places,
//! repositories and notes, as
//! `_OXIDGENE_TAG` lines of a `.ged` and of a GEDZIP (docs/api.md, OxidGene
//! extensions, `_OXIDGENE_TAG` and `_OXIDGENE_PLACE`).

use std::collections::HashMap;

use chrono::Utc;
use oxidgene_core::enums::{EventType, Privacy, Sex};
use oxidgene_core::types::{
    Citation, DOCUMENT_MIME, Event, Family, Media, Note, Person, Place, Repository, Source, TagRef,
};
use uuid::Uuid;

use crate::ImportResult;
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
        card_key: None,
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

/// A tag line beneath an event is read onto that event, and not kept whole as
/// untyped GEDCOM (docs/api.md, `_OXIDGENE_TAG`). A standalone `OBJE` is a document page
/// of its own on import, so its tag line is read too.
#[test]
fn a_tag_line_beneath_an_event_is_read_onto_it() {
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

    let birth = back.events.first().expect("the birth came back");
    assert_eq!(
        spelled(&birth.tags),
        [("Under an event".to_string(), 4)],
        "the tag line beneath the event is read onto it"
    );
    assert!(
        back.persons.first().is_some_and(|p| p.tags.is_empty()),
        "no tag is read beneath the person"
    );
    assert!(
        back.kept
            .iter()
            .all(|row| !row.nodes.contains("Under an event")),
        "the tag line beneath the event is not kept"
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

/// A person with a birth, an occupation, a source and a citation of the birth,
/// each carrying tags of its own (docs/api.md, `_OXIDGENE_TAG`).
struct EventSample {
    person: Person,
    birth: Event,
    occupation: Event,
    source: Source,
    citation: Citation,
}

fn event_sample() -> EventSample {
    let tree_id = Uuid::now_v7();
    let now = Utc::now();
    let person = Person {
        id: Uuid::now_v7(),
        tree_id,
        sex: Sex::Unknown,
        privacy: Privacy::Public,
        portrait_media_id: None,
        portrait_vignette_id: None,
        tags: vec![tag("Hérault", 3)],
        created_at: now,
        updated_at: now,
        deleted_at: None,
    };
    let birth = Event {
        id: Uuid::now_v7(),
        tree_id,
        event_type: EventType::Birth,
        date_value: None,
        date_sort: None,
        date_qualifier: Default::default(),
        date_value2: None,
        calendar: Default::default(),
        cause: None,
        age: None,
        agency: None,
        classification: None,
        spouse_ages: Vec::new(),
        place_id: None,
        person_id: Some(person.id),
        family_id: None,
        related_family_id: None,
        description: None,
        tags: vec![tag("Approximate date", 2)],
        created_at: now,
        updated_at: now,
        deleted_at: None,
    };
    let occupation = Event {
        id: Uuid::now_v7(),
        event_type: EventType::Occupation,
        description: Some("Notary".to_string()),
        tags: vec![tag("Trade card", 4)],
        ..birth.clone()
    };
    let source = Source {
        id: Uuid::now_v7(),
        tree_id,
        title: "Parish register of Sample-ville".to_string(),
        author: None,
        publisher: None,
        abbreviation: None,
        agency: None,
        tags: vec![tag("Fully abstracted", 6)],
        created_at: now,
        updated_at: now,
        deleted_at: None,
    };
    let citation = Citation {
        id: Uuid::now_v7(),
        source_id: source.id,
        person_id: None,
        event_id: Some(birth.id),
        family_id: None,
        page: Some("Folio 12".to_string()),
        confidence: None,
        text: None,
        tags: vec![tag("Doubtful reading", 8)],
        created_at: now,
        updated_at: now,
    };
    EventSample {
        person,
        birth,
        occupation,
        source,
        citation,
    }
}

/// The GEDCOM text of `sample` written for `target`, with nothing lost.
fn export_event_sample(sample: &EventSample, target: ExportTarget<'_>) -> String {
    let events = [sample.birth.clone(), sample.occupation.clone()];
    let sources = [sample.source.clone()];
    let citations = [sample.citation.clone()];
    let export = export_gedcom(
        &ExportRecords {
            persons: std::slice::from_ref(&sample.person),
            events: &events,
            sources: &sources,
            citations: &citations,
            ..Default::default()
        },
        &options(target),
    )
    .expect("exports");
    assert!(
        export.warnings.is_empty(),
        "nothing is lost: {:?}",
        export.warnings
    );
    export.gedcom
}

/// The tags of the events, source and citation of `sample`, as `back` reads
/// them, with nothing kept beyond the submitter.
fn assert_event_tags_came_back(back: &ImportResult, sample: &EventSample) {
    let birth = back
        .events
        .iter()
        .find(|event| event.event_type == EventType::Birth)
        .expect("the birth came back");
    assert_eq!(
        spelled(&birth.tags),
        spelled(&sample.birth.tags),
        "an event's tags, colours included"
    );
    let occupation = back
        .events
        .iter()
        .find(|event| event.event_type == EventType::Occupation)
        .expect("the occupation came back");
    assert_eq!(
        spelled(&occupation.tags),
        spelled(&sample.occupation.tags),
        "an attribute's tags sit beneath its structure"
    );
    let source = back.sources.first().expect("the source came back");
    assert_eq!(spelled(&source.tags), spelled(&sample.source.tags));
    let citation = back.citations.first().expect("the citation came back");
    assert_eq!(
        spelled(&citation.tags),
        spelled(&sample.citation.tags),
        "a citation's tags sit beneath its SOUR pointer"
    );
    assert_nothing_kept(&back.kept);
}

#[test]
fn event_source_and_citation_tags_survive_a_ged_round_trip() {
    let sample = event_sample();
    let text = export_event_sample(&sample, ExportTarget::Gedcom);
    assert_eq!(
        text.matches("_OXIDGENE_TAG").count(),
        5,
        "one line each for the person, the birth, the occupation, the source and the citation"
    );

    let back = import_gedcom(&text, Uuid::now_v7()).expect("imports");
    assert_eq!(spelled(&back.persons[0].tags), spelled(&sample.person.tags));
    assert_event_tags_came_back(&back, &sample);
}

#[test]
fn event_source_and_citation_tags_survive_a_gedzip_round_trip() {
    let sample = event_sample();
    let media_paths = HashMap::new();
    let text = export_event_sample(
        &sample,
        ExportTarget::Gedzip {
            media_paths: &media_paths,
        },
    );
    let archive = export_gedzip(&text, &[]).expect("creates GEDZIP");
    let back = import_gedzip(&archive, Uuid::now_v7())
        .expect("imports GEDZIP")
        .result;

    assert_event_tags_came_back(&back, &sample);
}

#[test]
fn a_re_export_writes_event_source_and_citation_tags_once_and_keeps_nothing() {
    let sample = event_sample();
    let text = export_event_sample(&sample, ExportTarget::Gedcom);
    let imported = import_gedcom(&text, Uuid::now_v7()).expect("imports");
    let export =
        export_gedcom(&imported.records(), &options(ExportTarget::Gedcom)).expect("exports");

    assert_eq!(
        export.gedcom.matches("_OXIDGENE_TAG").count(),
        5,
        "each tag line is written once, beneath its structure"
    );
    let back = import_gedcom(&export.gedcom, Uuid::now_v7()).expect("imports again");
    assert_event_tags_came_back(&back, &sample);
}

/// A tag the file carries beneath an event, which OxidGene drops after the
/// import, is not written back from the file as kept GEDCOM.
#[test]
fn an_event_tag_dropped_in_oxidgene_is_not_written_back_from_the_file() {
    const FILE: &str = "0 HEAD\n\
1 GEDC\n\
2 VERS 5.5.1\n\
2 FORM LINEAGE-LINKED\n\
1 CHAR UTF-8\n\
0 @I1@ INDI\n\
1 NAME Sample /Person/\n\
1 BIRT\n\
2 DATE 1900\n\
2 _OXIDGENE_TAG {\"v\":1,\"label\":\"Dropped later\",\"color\":4}\n\
0 TRLR\n";
    let mut back = import_gedcom(FILE, Uuid::now_v7()).expect("imports");
    assert_eq!(back.events[0].tags.len(), 1, "read onto the birth");
    back.events[0].tags.clear();

    let export = export_gedcom(&back.records(), &options(ExportTarget::Gedcom)).expect("exports");
    assert!(
        !export.gedcom.contains("Dropped later"),
        "the dropped tag is not written back"
    );
    let again = import_gedcom(&export.gedcom, Uuid::now_v7()).expect("imports again");
    assert!(again.events[0].tags.is_empty());
    assert_nothing_kept(&again.kept);
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

/// A tag line beneath an LDS ordinance or a military service is read onto that
/// event, and a duplicate of it (folded alike) is read once and kept nowhere: the
/// export writes each tag once beneath the structure the import reads it from
/// (docs/api.md, `_OXIDGENE_TAG`).
#[test]
fn a_duplicated_tag_line_beneath_an_ordinance_or_military_service_is_read_once() {
    const FILE: &str = "0 HEAD\n\
1 GEDC\n\
2 VERS 5.5.1\n\
2 FORM LINEAGE-LINKED\n\
1 CHAR UTF-8\n\
0 @I1@ INDI\n\
1 NAME Sample /Person/\n\
1 BAPL\n\
2 DATE 1900\n\
2 _OXIDGENE_TAG {\"v\":1,\"label\":\"Ordained\",\"color\":4}\n\
2 _OXIDGENE_TAG {\"v\":1,\"label\":\"ordained\",\"color\":4}\n\
1 MILI\n\
2 DATE 1918\n\
2 _OXIDGENE_TAG {\"v\":1,\"label\":\"Served\",\"color\":6}\n\
2 _OXIDGENE_TAG {\"v\":1,\"label\":\"served\",\"color\":6}\n\
0 TRLR\n";
    let back = import_gedcom(FILE, Uuid::now_v7()).expect("imports");

    let baptism = back
        .events
        .iter()
        .find(|event| event.event_type == EventType::LdsBaptism)
        .expect("the baptism came back");
    assert_eq!(spelled(&baptism.tags), [("Ordained".to_string(), 4)]);
    let service = back
        .events
        .iter()
        .find(|event| event.event_type == EventType::MilitaryService)
        .expect("the military service came back");
    assert_eq!(spelled(&service.tags), [("Served".to_string(), 6)]);
    assert_nothing_kept(&back.kept);

    let export = export_gedcom(&back.records(), &options(ExportTarget::Gedcom)).expect("exports");
    assert_eq!(
        export.gedcom.matches("_OXIDGENE_TAG").count(),
        2,
        "one line beneath the baptism and one beneath the service: {}",
        export.gedcom
    );
    let again = import_gedcom(&export.gedcom, Uuid::now_v7()).expect("imports again");
    assert_nothing_kept(&again.kept);
}

/// A person with a birth and an occupation both held at one place, a
/// repository, an inline note and two notes sharing one text, each carrying
/// tags (docs/api.md, `_OXIDGENE_TAG` and `_OXIDGENE_PLACE`).
struct PlaceSample {
    person: Person,
    events: [Event; 2],
    place: Place,
    repository: Repository,
    notes: [Note; 3],
}

fn place_sample() -> PlaceSample {
    let tree_id = Uuid::now_v7();
    let now = Utc::now();
    let person = Person {
        tags: Vec::new(),
        id: Uuid::now_v7(),
        tree_id,
        sex: Sex::Unknown,
        privacy: Privacy::Public,
        portrait_media_id: None,
        portrait_vignette_id: None,
        created_at: now,
        updated_at: now,
        deleted_at: None,
    };
    let place = Place {
        id: Uuid::now_v7(),
        tree_id,
        name: "Sample town".to_string(),
        latitude: None,
        longitude: None,
        tags: vec![tag("Vanished parish", 5)],
        created_at: now,
        updated_at: now,
    };
    let birth = Event {
        id: Uuid::now_v7(),
        tree_id,
        event_type: EventType::Birth,
        date_value: None,
        date_sort: None,
        date_qualifier: Default::default(),
        date_value2: None,
        calendar: Default::default(),
        cause: None,
        age: None,
        agency: None,
        classification: None,
        spouse_ages: Vec::new(),
        place_id: Some(place.id),
        person_id: Some(person.id),
        family_id: None,
        related_family_id: None,
        description: None,
        tags: Vec::new(),
        created_at: now,
        updated_at: now,
        deleted_at: None,
    };
    let occupation = Event {
        id: Uuid::now_v7(),
        event_type: EventType::Occupation,
        description: Some("Notary".to_string()),
        ..birth.clone()
    };
    let repository = Repository {
        id: Uuid::now_v7(),
        tree_id,
        name: "Sample archive".to_string(),
        address: None,
        phone: None,
        email: None,
        website: None,
        tags: vec![tag("Closed", 6)],
        created_at: now,
        updated_at: now,
        deleted_at: None,
    };
    let group = Uuid::now_v7();
    let note = |text: &str, shared_group: Option<Uuid>, tags: Vec<TagRef>| Note {
        id: Uuid::now_v7(),
        tree_id,
        text: text.to_string(),
        person_id: Some(person.id),
        event_id: None,
        family_id: None,
        source_id: None,
        media_id: None,
        repository_id: None,
        shared_group,
        tags,
        created_at: now,
        updated_at: now,
        deleted_at: None,
    };
    let notes = [
        note("An inline hypothesis", None, vec![tag("Hypothesis", 2)]),
        note("Shared transcript", Some(group), vec![tag("To write", 3)]),
        note("Shared transcript", Some(group), vec![tag("To check", 4)]),
    ];
    PlaceSample {
        person,
        events: [birth, occupation],
        place,
        repository,
        notes,
    }
}

/// The GEDCOM text of `sample`, written for `target`, with nothing lost.
fn export_place_sample(sample: &PlaceSample, target: ExportTarget<'_>) -> String {
    let export = export_gedcom(
        &ExportRecords {
            persons: std::slice::from_ref(&sample.person),
            events: &sample.events,
            places: std::slice::from_ref(&sample.place),
            repositories: std::slice::from_ref(&sample.repository),
            notes: &sample.notes,
            ..Default::default()
        },
        &options(target),
    )
    .expect("exports");
    assert!(
        export.warnings.is_empty(),
        "nothing is lost: {:?}",
        export.warnings
    );
    export.gedcom
}

/// The tags of the place, repository and notes of `sample`, as `back` reads
/// them, with nothing kept beyond the submitter.
fn assert_place_tags_came_back(back: &ImportResult, sample: &PlaceSample) {
    let place = back
        .places
        .iter()
        .find(|place| place.name == "Sample town")
        .expect("the place came back");
    assert_eq!(
        spelled(&place.tags),
        spelled(&sample.place.tags),
        "a place's tags are the union of its PLAC structures', once"
    );
    let repository = back.repositories.first().expect("the repository came back");
    assert_eq!(spelled(&repository.tags), spelled(&sample.repository.tags));
    let inline = back
        .notes
        .iter()
        .find(|note| note.text == "An inline hypothesis")
        .expect("the inline note came back");
    assert_eq!(spelled(&inline.tags), spelled(&sample.notes[0].tags));
    let shared: Vec<&Note> = back
        .notes
        .iter()
        .filter(|note| note.text == "Shared transcript")
        .collect();
    assert_eq!(shared.len(), 2, "both rows of the shared note came back");
    for note in shared {
        assert_eq!(
            spelled(&note.tags),
            [("To check".to_string(), 4), ("To write".to_string(), 3)],
            "each row of a shared note carries the record's union of tags"
        );
    }
    assert_nothing_kept(&back.kept);
}

#[test]
fn place_repository_and_note_tags_survive_a_ged_round_trip() {
    let sample = place_sample();
    let text = export_place_sample(&sample, ExportTarget::Gedcom);
    assert_eq!(
        text.matches("_OXIDGENE_TAG").count(),
        6,
        "one line beneath each PLAC of the place, the repository, the inline note, and each tag of the shared record"
    );
    assert_eq!(
        text.lines().filter(|line| line.starts_with("0 @N")).count(),
        1,
        "the shared note is written once"
    );

    let back = import_gedcom(&text, Uuid::now_v7()).expect("imports");
    assert_place_tags_came_back(&back, &sample);
}

#[test]
fn place_repository_and_note_tags_survive_a_gedzip_round_trip() {
    let sample = place_sample();
    let media_paths = HashMap::new();
    let text = export_place_sample(
        &sample,
        ExportTarget::Gedzip {
            media_paths: &media_paths,
        },
    );
    let archive = export_gedzip(&text, &[]).expect("creates GEDZIP");
    let back = import_gedzip(&archive, Uuid::now_v7())
        .expect("imports GEDZIP")
        .result;

    assert_place_tags_came_back(&back, &sample);
}

#[test]
fn a_re_export_writes_place_repository_and_note_tags_once_and_keeps_nothing() {
    let sample = place_sample();
    let text = export_place_sample(&sample, ExportTarget::Gedcom);
    let imported = import_gedcom(&text, Uuid::now_v7()).expect("imports");
    let export =
        export_gedcom(&imported.records(), &options(ExportTarget::Gedcom)).expect("exports");

    assert_eq!(
        export.gedcom.matches("_OXIDGENE_TAG").count(),
        6,
        "each tag line is written once beneath its structure"
    );
    let back = import_gedcom(&export.gedcom, Uuid::now_v7()).expect("imports again");
    assert_place_tags_came_back(&back, &sample);
}

/// Two `PLAC` structures of one name, each with a tag of its own, make one
/// place carrying both (docs/api.md, `_OXIDGENE_TAG` and `_OXIDGENE_PLACE`).
#[test]
fn the_plac_structures_of_one_place_give_it_the_union_of_their_tags() {
    const FILE: &str = "0 HEAD\n\
1 GEDC\n\
2 VERS 5.5.1\n\
2 FORM LINEAGE-LINKED\n\
1 CHAR UTF-8\n\
0 @I1@ INDI\n\
1 NAME Sample /Person/\n\
1 BIRT\n\
2 PLAC Sample town\n\
3 _OXIDGENE_TAG {\"v\":1,\"label\":\"Vanished parish\",\"color\":5}\n\
1 DEAT\n\
2 PLAC Sample town\n\
3 _OXIDGENE_TAG {\"v\":1,\"label\":\"To locate\",\"color\":2}\n\
3 _OXIDGENE_TAG {\"v\":1,\"label\":\"vanished PARISH\",\"color\":9}\n\
0 TRLR\n";
    let back = import_gedcom(FILE, Uuid::now_v7()).expect("imports");

    assert_eq!(back.places.len(), 1, "one name is one place");
    assert_eq!(
        spelled(&back.places[0].tags),
        [
            ("To locate".to_string(), 2),
            ("Vanished parish".to_string(), 5)
        ],
        "the place carries the union, the first spelling of each tag kept"
    );
    assert_nothing_kept(&back.kept);
}

/// A tagged place with coordinates that no event names, so that no `PLAC`
/// carries its tags (docs/api.md, `_OXIDGENE_PLACE`).
fn unnamed_place() -> Place {
    let now = Utc::now();
    Place {
        id: Uuid::now_v7(),
        tree_id: Uuid::now_v7(),
        name: "Unused hamlet".to_string(),
        latitude: Some(48.5),
        longitude: Some(-2.25),
        tags: vec![tag("To locate", 2), tag("Vanished parish", 5)],
        created_at: now,
        updated_at: now,
    }
}

/// The GEDCOM text of an export holding only `place`, with nothing lost.
fn export_unnamed_place(place: &Place, target: ExportTarget<'_>) -> String {
    let export = export_gedcom(
        &ExportRecords {
            places: std::slice::from_ref(place),
            ..Default::default()
        },
        &options(target),
    )
    .expect("exports");
    assert!(
        export.warnings.is_empty(),
        "nothing is lost: {:?}",
        export.warnings
    );
    export.gedcom
}

/// The place `back` reads back for `place`: its tags and coordinates, with
/// nothing kept beyond the submitter.
fn assert_unnamed_place_came_back(back: &ImportResult, place: &Place) {
    let found = back
        .places
        .iter()
        .find(|item| item.name == place.name)
        .expect("the place came back");
    assert_eq!(spelled(&found.tags), spelled(&place.tags));
    assert_eq!(found.latitude, place.latitude);
    assert_eq!(found.longitude, place.longitude);
    assert_nothing_kept(&back.kept);
}

#[test]
fn a_tagged_place_no_event_names_survives_a_ged_round_trip() {
    let place = unnamed_place();
    let text = export_unnamed_place(&place, ExportTarget::Gedcom);
    assert!(
        text.contains("0 @P1@ _OXIDGENE_PLACE"),
        "the place is a record of its own"
    );
    assert_eq!(text.matches("_OXIDGENE_TAG").count(), 2);

    let back = import_gedcom(&text, Uuid::now_v7()).expect("imports");
    assert_unnamed_place_came_back(&back, &place);
}

#[test]
fn a_tagged_place_no_event_names_survives_a_gedzip_round_trip() {
    let place = unnamed_place();
    let media_paths = HashMap::new();
    let text = export_unnamed_place(
        &place,
        ExportTarget::Gedzip {
            media_paths: &media_paths,
        },
    );
    let archive = export_gedzip(&text, &[]).expect("creates GEDZIP");
    let back = import_gedzip(&archive, Uuid::now_v7())
        .expect("imports GEDZIP")
        .result;

    assert_unnamed_place_came_back(&back, &place);
}

#[test]
fn a_re_export_writes_an_unnamed_place_once_and_keeps_nothing() {
    let place = unnamed_place();
    let text = export_unnamed_place(&place, ExportTarget::Gedcom);
    let imported = import_gedcom(&text, Uuid::now_v7()).expect("imports");
    let export =
        export_gedcom(&imported.records(), &options(ExportTarget::Gedcom)).expect("exports");

    assert_eq!(
        export.gedcom.matches("0 @P").count(),
        1,
        "the place is written once"
    );
    let back = import_gedcom(&export.gedcom, Uuid::now_v7()).expect("imports again");
    assert_unnamed_place_came_back(&back, &place);
}

/// A tagged place named only by an event of a person the export does not
/// write (a soft-deleted one, say): the event is not written, so no `PLAC`
/// carries the place's tags and the place is a record of its own.
#[test]
fn a_tagged_place_named_only_by_an_unwritten_event_survives_a_ged_round_trip() {
    let place = unnamed_place();
    let now = Utc::now();
    let event = Event {
        id: Uuid::now_v7(),
        tree_id: place.tree_id,
        event_type: EventType::Birth,
        date_value: None,
        date_sort: None,
        date_qualifier: Default::default(),
        date_value2: None,
        calendar: Default::default(),
        cause: None,
        age: None,
        agency: None,
        classification: None,
        spouse_ages: Vec::new(),
        place_id: Some(place.id),
        person_id: Some(Uuid::now_v7()),
        family_id: None,
        related_family_id: None,
        description: None,
        tags: Vec::new(),
        created_at: now,
        updated_at: now,
        deleted_at: None,
    };
    let export = export_gedcom(
        &ExportRecords {
            places: std::slice::from_ref(&place),
            events: std::slice::from_ref(&event),
            ..Default::default()
        },
        &options(ExportTarget::Gedcom),
    )
    .expect("exports");
    assert!(
        export.warnings.is_empty(),
        "nothing is lost: {:?}",
        export.warnings
    );
    assert!(
        export.gedcom.contains("0 @P1@ _OXIDGENE_PLACE"),
        "the place no written event names is a record of its own"
    );
    assert!(!export.gedcom.contains("BIRT"), "the event is not written");

    let back = import_gedcom(&export.gedcom, Uuid::now_v7()).expect("imports");
    assert_unnamed_place_came_back(&back, &place);
}

/// A page and its document, with a transcript on the page and a note on the
/// document, each carrying tags of its own.
fn transcript_sample() -> (Media, Media, [Note; 2]) {
    let tree_id = Uuid::now_v7();
    let now = Utc::now();
    let (document, page) = stored_document(tree_id, Vec::new());
    let note = |text: &str, media_id: Uuid, tags: Vec<TagRef>| Note {
        id: Uuid::now_v7(),
        tree_id,
        text: text.to_string(),
        person_id: None,
        event_id: None,
        family_id: None,
        source_id: None,
        media_id: Some(media_id),
        repository_id: None,
        shared_group: None,
        tags,
        created_at: now,
        updated_at: now,
        deleted_at: None,
    };
    let notes = [
        note("Page transcript", page.id, vec![tag("To write", 3)]),
        note("Document note", document.id, vec![tag("To check", 4)]),
    ];
    (document, page, notes)
}

/// The GEDCOM text of the transcript sample, written for `target`.
fn export_transcript_sample(target: ExportTarget<'_>) -> String {
    let (document, page, notes) = transcript_sample();
    let rows = [document, page];
    let export = export_gedcom(
        &ExportRecords {
            media: &rows,
            notes: &notes,
            ..Default::default()
        },
        &options(target),
    )
    .expect("exports");
    assert!(
        export.warnings.is_empty(),
        "nothing is lost: {:?}",
        export.warnings
    );
    export.gedcom
}

/// The transcript and the document note of the sample, with their tags.
fn assert_transcript_tags_came_back(back: &ImportResult) {
    let transcript = back
        .notes
        .iter()
        .find(|note| note.text == "Page transcript")
        .expect("the transcript came back");
    assert_eq!(spelled(&transcript.tags), [("To write".to_string(), 3)]);
    let document_note = back
        .notes
        .iter()
        .find(|note| note.text == "Document note")
        .expect("the document note came back");
    assert_eq!(spelled(&document_note.tags), [("To check".to_string(), 4)]);
    assert_nothing_kept(&back.kept);
}

#[test]
fn transcript_and_document_note_tags_survive_a_ged_round_trip() {
    let text = export_transcript_sample(ExportTarget::Gedcom);
    let back = import_gedcom(&text, Uuid::now_v7()).expect("imports");
    assert_transcript_tags_came_back(&back);
}

#[test]
fn transcript_and_document_note_tags_survive_a_gedzip_round_trip() {
    let media_paths = HashMap::new();
    let text = export_transcript_sample(ExportTarget::Gedzip {
        media_paths: &media_paths,
    });
    let archive = export_gedzip(&text, &[]).expect("creates GEDZIP");
    let back = import_gedzip(&archive, Uuid::now_v7())
        .expect("imports GEDZIP")
        .result;
    assert_transcript_tags_came_back(&back);
}

#[test]
fn an_untagged_transcript_is_written_without_a_tags_key() {
    let (document, page, mut notes) = transcript_sample();
    notes[0].tags.clear();
    notes[1].tags.clear();
    let rows = [document, page];
    let text = export_gedcom(
        &ExportRecords {
            media: &rows,
            notes: &notes,
            ..Default::default()
        },
        &options(ExportTarget::Gedcom),
    )
    .expect("exports")
    .gedcom;
    assert!(
        !text.contains("\"tags\""),
        "a transcript without tags looks as it did before"
    );
    assert!(!text.contains("_OXIDGENE_TAG"));
}

#[test]
fn a_transcript_tag_line_of_another_version_is_skipped_with_a_warning() {
    let text = export_transcript_sample(ExportTarget::Gedcom).replace("\"v\":1", "\"v\":2");
    let back = import_gedcom(&text, Uuid::now_v7()).expect("imports");
    let transcript = back
        .notes
        .iter()
        .find(|note| note.text == "Page transcript")
        .expect("the transcript came back");
    assert!(
        transcript.tags.is_empty(),
        "a tag line of another version is not read"
    );
    assert!(
        back.warnings
            .iter()
            .any(|warning| warning.contains("Unsupported _OXIDGENE_TAG version 2")),
        "the skipped line is reported: {:?}",
        back.warnings
    );
}
