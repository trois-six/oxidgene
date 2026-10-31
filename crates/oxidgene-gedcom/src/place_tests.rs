//! One place name gives one place on import, whether a document, an event or
//! a tagged place names it (docs/api.md, `_OXIDGENE_PLACE` and `_OXIDGENE_DOC`).

use chrono::Utc;
use oxidgene_core::enums::{EventType, Privacy, Sex};
use oxidgene_core::types::{DOCUMENT_MIME, Event, Media, Person, Place, TagRef};
use uuid::Uuid;

use crate::ImportResult;
use crate::export::{
    ExportChoices, ExportOptions, ExportRecords, ExportTarget, SubmitterSettings, archive_path,
    export_gedcom, export_gedzip,
};
use crate::import::{import_gedcom, import_gedzip};

const TOWN: &str = "Sample town";

fn options(target: ExportTarget<'_>) -> ExportOptions<'_> {
    ExportOptions {
        choices: ExportChoices::default(),
        target,
        self_person_id: None,
        submitter: SubmitterSettings::default(),
    }
}

fn place(
    tree_id: Uuid,
    name: &str,
    latitude: Option<f64>,
    longitude: Option<f64>,
    tags: Vec<TagRef>,
) -> Place {
    let now = Utc::now();
    Place {
        id: Uuid::now_v7(),
        tree_id,
        name: name.to_string(),
        latitude,
        longitude,
        tags,
        created_at: now,
        updated_at: now,
    }
}

fn tag(label: &str, color: i16) -> TagRef {
    TagRef {
        id: Uuid::now_v7(),
        label: label.to_string(),
        color,
    }
}

/// A document of one stored page, placed at `place_id`.
fn document_at(tree_id: Uuid, place_id: Uuid) -> (Media, Media) {
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
        tags: Vec::new(),
        place_id: Some(place_id),
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
        title: None,
        place_id: None,
        ..document.clone()
    };
    (document, page)
}

fn birth_at(tree_id: Uuid, person: &Person, place_id: Uuid) -> Event {
    let now = Utc::now();
    Event {
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
        place_id: Some(place_id),
        person_id: Some(person.id),
        family_id: None,
        related_family_id: None,
        description: None,
        tags: Vec::new(),
        created_at: now,
        updated_at: now,
        deleted_at: None,
    }
}

fn person(tree_id: Uuid) -> Person {
    let now = Utc::now();
    Person {
        id: Uuid::now_v7(),
        tree_id,
        sex: Sex::Unknown,
        privacy: Privacy::Public,
        portrait_media_id: None,
        portrait_vignette_id: None,
        tags: Vec::new(),
        created_at: now,
        updated_at: now,
        deleted_at: None,
    }
}

/// The places of `back` named `name`.
fn places_named<'a>(back: &'a ImportResult, name: &str) -> Vec<&'a Place> {
    back.places
        .iter()
        .filter(|place| place.name == name)
        .collect()
}

/// A document and an event that name the same place: the export of both,
/// with the place once.
fn document_and_event_sample() -> (Place, Event, Media, Media, Person) {
    let tree_id = Uuid::now_v7();
    let town = place(tree_id, TOWN, None, None, Vec::new());
    let person = person(tree_id);
    let event = birth_at(tree_id, &person, town.id);
    let (document, page) = document_at(tree_id, town.id);
    (town, event, document, page, person)
}

#[test]
fn a_document_and_an_event_naming_one_place_import_one_place_from_a_ged() {
    let (town, event, document, page, person) = document_and_event_sample();
    let export = export_gedcom(
        &ExportRecords {
            persons: std::slice::from_ref(&person),
            events: std::slice::from_ref(&event),
            places: std::slice::from_ref(&town),
            media: &[document.clone(), page],
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

    let back = import_gedcom(&export.gedcom, Uuid::now_v7()).expect("imports");
    let named = places_named(&back, TOWN);
    assert_eq!(named.len(), 1, "one name, one place: {named:?}");
    let imported_document = back
        .media
        .iter()
        .find(|item| item.is_document())
        .expect("the document came back");
    assert_eq!(imported_document.place_id, Some(named[0].id));
    let imported_event = back
        .events
        .iter()
        .find(|item| item.place_id.is_some())
        .expect("the event came back");
    assert_eq!(imported_event.place_id, Some(named[0].id));
}

#[test]
fn a_document_and_an_event_naming_one_place_import_one_place_from_a_gedzip() {
    let (town, event, document, page, person) = document_and_event_sample();
    let archive_path = archive_path(&page).expect("stored media has an archive path");
    let media_paths = std::collections::HashMap::from([(page.id, archive_path.clone())]);
    let export = export_gedcom(
        &ExportRecords {
            persons: std::slice::from_ref(&person),
            events: std::slice::from_ref(&event),
            places: std::slice::from_ref(&town),
            media: &[document, page],
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
    assert_eq!(places_named(&back, TOWN).len(), 1, "one name, one place");
}

/// The labels of `tags`, sorted, for comparisons that ignore their order.
fn labels(tags: &[TagRef]) -> Vec<String> {
    let mut labels: Vec<String> = tags.iter().map(|tag| tag.label.clone()).collect();
    labels.sort();
    labels
}

/// A tagged place no event names, named `Sample town`, and an event whose
/// place is `Sample town ` with a trailing blank: the name is one place.
#[test]
fn a_standalone_place_and_an_event_naming_one_place_import_one_place_from_a_ged() {
    let tree_id = Uuid::now_v7();
    let person = person(tree_id);
    let standalone = place(tree_id, TOWN, None, None, vec![tag("Hérault", 3)]);
    let event_place = place(
        tree_id,
        "Sample town ",
        Some(48.5),
        Some(2.25),
        vec![tag("Vanished parish", 5)],
    );
    let event = birth_at(tree_id, &person, event_place.id);
    let export = export_gedcom(
        &ExportRecords {
            persons: std::slice::from_ref(&person),
            events: std::slice::from_ref(&event),
            places: &[standalone, event_place],
            ..Default::default()
        },
        &options(ExportTarget::Gedcom),
    )
    .expect("exports");
    assert!(
        export.gedcom.contains("0 @P1@ _OXIDGENE_PLACE"),
        "the tagged place no event names is a record of its own"
    );

    let back = import_gedcom(&export.gedcom, Uuid::now_v7()).expect("imports");
    let named = places_named(&back, TOWN);
    assert_eq!(named.len(), 1, "one name, one place: {named:?}");
    assert_eq!(
        labels(&named[0].tags),
        vec!["Hérault".to_string(), "Vanished parish".to_string()],
        "the place takes the union of its records' tags"
    );
    let imported_event = back
        .events
        .iter()
        .find(|item| item.place_id.is_some())
        .expect("the event came back");
    assert_eq!(imported_event.place_id, Some(named[0].id));
}

/// A document placed at `Sample town ` (a trailing blank) and an event at
/// `Sample town`: one place, carrying the coordinates the event read first.
#[test]
fn a_document_and_an_event_naming_one_place_up_to_blanks_import_one_place_from_a_ged() {
    let tree_id = Uuid::now_v7();
    let person = person(tree_id);
    let document_place = place(tree_id, "Sample town ", Some(48.5), Some(2.25), Vec::new());
    let event_place = place(tree_id, TOWN, None, None, Vec::new());
    let event = birth_at(tree_id, &person, event_place.id);
    let (document, page) = document_at(tree_id, document_place.id);
    let export = export_gedcom(
        &ExportRecords {
            persons: std::slice::from_ref(&person),
            events: std::slice::from_ref(&event),
            places: &[document_place, event_place],
            media: &[document, page],
            ..Default::default()
        },
        &options(ExportTarget::Gedcom),
    )
    .expect("exports");

    let back = import_gedcom(&export.gedcom, Uuid::now_v7()).expect("imports");
    let named = places_named(&back, TOWN);
    assert_eq!(named.len(), 1, "one name, one place: {named:?}");
    assert_eq!(named[0].latitude, Some(48.5));
    assert_eq!(named[0].longitude, Some(2.25));
    let imported_document = back
        .media
        .iter()
        .find(|item| item.is_document())
        .expect("the document came back");
    assert_eq!(imported_document.place_id, Some(named[0].id));
}

/// Two events naming one place give it their `MAP` coordinates: the first
/// pair read is kept, a later pair does not replace it (docs/api.md,
/// `_OXIDGENE_PLACE`, place identity on import).
#[test]
fn the_first_coordinates_read_for_a_place_named_by_two_events_are_kept() {
    const FILE: &str = "0 HEAD\n\
1 GEDC\n\
2 VERS 5.5.1\n\
2 FORM LINEAGE-LINKED\n\
1 CHAR UTF-8\n\
0 @I1@ INDI\n\
1 NAME Sample /Person/\n\
1 BIRT\n\
2 PLAC Sample town\n\
3 MAP\n\
4 LATI N48.5\n\
4 LONG E2.25\n\
1 DEAT\n\
2 PLAC Sample town\n\
3 MAP\n\
4 LATI N10.0\n\
4 LONG E20.0\n\
0 TRLR\n";
    let back = import_gedcom(FILE, Uuid::now_v7()).expect("imports");

    let named = places_named(&back, TOWN);
    assert_eq!(named.len(), 1, "one name, one place: {named:?}");
    assert_eq!(named[0].latitude, Some(48.5), "the first latitude is kept");
    assert_eq!(
        named[0].longitude,
        Some(2.25),
        "the first longitude is kept"
    );
}
