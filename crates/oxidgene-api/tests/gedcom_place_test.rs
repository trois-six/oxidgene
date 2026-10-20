//! One place name gives one place row when a document's metadata and an
//! event's `PLAC` both name it (docs/api.md, `_OXIDGENE_DOC` and `_OXIDGENE_PLACE`).
//!
//! All data is fictitious.

mod common;

use common::{app_on, import_gedcom, new_tree, setup_db};
use oxidgene_db::entities::place;
use oxidgene_db::sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use uuid::Uuid;

/// One person whose birth is placed at `Sample town`, and one document whose
/// metadata places it at the same name, with coordinates.
const DOCUMENT_AND_EVENT_GEDCOM: &str = "0 HEAD\n\
1 GEDC\n\
2 VERS 5.5.1\n\
2 FORM LINEAGE-LINKED\n\
1 CHAR UTF-8\n\
0 @I1@ INDI\n\
1 NAME Sample /Person/\n\
1 BIRT\n\
2 PLAC Sample town\n\
0 @M1@ OBJE\n\
1 FILE parish-register.jpg\n\
2 FORM jpeg\n\
1 _OXIDGENE_DOC {\"version\":1,\"doc\":\"D1\",\"index\":0,\"count\":1,\"meta\":{\"file_name\":\"parish-register.jpg\",\"place\":{\"name\":\"Sample town\",\"latitude\":48.5,\"longitude\":2.25},\"notes\":[]}}\n\
0 TRLR\n";

#[tokio::test]
async fn a_document_and_an_event_naming_one_place_store_one_place_row() {
    let db = setup_db().await;
    let app = app_on(db.clone());
    let tree_id = new_tree(&app, "Shared Place Tree").await;
    let tree = Uuid::parse_str(&tree_id).expect("tree id");
    import_gedcom(&app, &db, &tree_id, DOCUMENT_AND_EVENT_GEDCOM).await;

    let places = place::Entity::find()
        .filter(place::Column::TreeId.eq(tree))
        .all(&db)
        .await
        .expect("lists places");
    assert_eq!(places.len(), 1, "one name, one place row: {places:?}");
    assert_eq!(places[0].name, "Sample town");
    assert_eq!(places[0].latitude, Some(48.5));
    assert_eq!(places[0].longitude, Some(2.25));
}

/// A document placed at `Sample town ` (a trailing blank) and an event at
/// `Sample town`: the names differ only in blanks, so one place row holds
/// the document's coordinates.
const DOCUMENT_WITH_BLANK_AND_EVENT_GEDCOM: &str = "0 HEAD\n\
1 GEDC\n\
2 VERS 5.5.1\n\
2 FORM LINEAGE-LINKED\n\
1 CHAR UTF-8\n\
0 @I1@ INDI\n\
1 NAME Sample /Person/\n\
1 BIRT\n\
2 PLAC Sample town\n\
0 @M1@ OBJE\n\
1 FILE parish-register.jpg\n\
2 FORM jpeg\n\
1 _OXIDGENE_DOC {\"version\":1,\"doc\":\"D1\",\"index\":0,\"count\":1,\"meta\":{\"file_name\":\"parish-register.jpg\",\"place\":{\"name\":\"Sample town \",\"latitude\":48.5,\"longitude\":2.25},\"notes\":[]}}\n\
0 TRLR\n";

#[tokio::test]
async fn a_document_and_an_event_naming_one_place_up_to_blanks_store_one_place_row() {
    let db = setup_db().await;
    let app = app_on(db.clone());
    let tree_id = new_tree(&app, "Blank Place Tree").await;
    let tree = Uuid::parse_str(&tree_id).expect("tree id");
    import_gedcom(&app, &db, &tree_id, DOCUMENT_WITH_BLANK_AND_EVENT_GEDCOM).await;

    let places = place::Entity::find()
        .filter(place::Column::TreeId.eq(tree))
        .all(&db)
        .await
        .expect("lists places");
    assert_eq!(
        places.len(),
        1,
        "one trimmed name, one place row: {places:?}"
    );
    assert_eq!(places[0].name, "Sample town");
    assert_eq!(places[0].latitude, Some(48.5));
    assert_eq!(places[0].longitude, Some(2.25));
}
