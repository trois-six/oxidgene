//! A GEDCOM file's tags reach the database: each folded label is one tag of
//! the tree, linked to the persons, couples and documents that carry it, and
//! an export writes them back once (docs/tags.md §3).
//!
//! All data is fictitious.

mod common;

use std::collections::HashMap;

use axum::http::{Method, StatusCode};
use common::{app_on, import_gedcom, new_tree, send, setup_db};
use oxidgene_db::entities::{record_tag, tag};
use oxidgene_db::repo::TagRepo;
use oxidgene_db::sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use uuid::Uuid;

/// Two persons, one couple and one document. The second person's label folds
/// alike the first's, so the file holds one tag of it.
const TAGGED_GEDCOM: &str = "0 HEAD\n\
1 GEDC\n\
2 VERS 5.5.1\n\
2 FORM LINEAGE-LINKED\n\
1 CHAR UTF-8\n\
0 @I1@ INDI\n\
1 NAME Sample /Person/\n\
1 _OXIDGENE_TAG {\"v\":1,\"label\":\"Hérault\",\"color\":3}\n\
1 _OXIDGENE_TAG {\"v\":1,\"label\":\"A VERIFIER\",\"color\":5}\n\
0 @I2@ INDI\n\
1 NAME Other /Person/\n\
1 _OXIDGENE_TAG {\"v\":1,\"label\":\"a verifier\",\"color\":8}\n\
0 @F1@ FAM\n\
1 HUSB @I1@\n\
1 WIFE @I2@\n\
1 _OXIDGENE_TAG {\"v\":1,\"label\":\"Witnessed\",\"color\":1}\n\
0 @M1@ OBJE\n\
1 FILE parish-register.jpg\n\
2 FORM jpeg\n\
1 _OXIDGENE_TAG {\"v\":1,\"label\":\"Parish register\",\"color\":9}\n\
0 TRLR\n";

async fn tree_tags(db: &DatabaseConnection, tree: Uuid) -> Vec<tag::Model> {
    tag::Entity::find()
        .filter(tag::Column::TreeId.eq(tree))
        .all(db)
        .await
        .expect("lists tags")
}

async fn tree_links(db: &DatabaseConnection, tree: Uuid) -> Vec<record_tag::Model> {
    record_tag::Entity::find()
        .filter(record_tag::Column::TreeId.eq(tree))
        .all(db)
        .await
        .expect("lists links")
}

#[tokio::test]
async fn a_file_tags_its_persons_couples_and_documents_once_per_label() {
    let db = setup_db().await;
    let app = app_on(db.clone());
    let tree_id = new_tree(&app, "Tagged Tree").await;
    let tree = Uuid::parse_str(&tree_id).expect("tree id");
    import_gedcom(&app, &db, &tree_id, TAGGED_GEDCOM).await;

    let tags = tree_tags(&db, tree).await;
    let colours: HashMap<&str, i16> = tags
        .iter()
        .map(|tag| (tag.label.as_str(), tag.color))
        .collect();
    assert_eq!(tags.len(), 4, "three spellings of one label are one tag");
    assert_eq!(colours["Hérault"], 3);
    assert_eq!(
        colours["A VERIFIER"], 5,
        "the first spelling and colour win"
    );
    assert_eq!(colours["Witnessed"], 1);
    assert_eq!(colours["Parish register"], 9);

    let links = tree_links(&db, tree).await;
    assert_eq!(
        links.iter().filter(|link| link.person_id.is_some()).count(),
        3
    );
    assert_eq!(
        links.iter().filter(|link| link.family_id.is_some()).count(),
        1
    );
    assert_eq!(
        links.iter().filter(|link| link.media_id.is_some()).count(),
        1
    );
    assert_eq!(links.len(), 5);

    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/gedcom/export"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let exported = body["gedcom"].as_str().expect("gedcom text");
    assert_eq!(
        exported.matches("_OXIDGENE_TAG").count(),
        5,
        "each carried tag is written once"
    );
}

#[tokio::test]
async fn a_tag_the_tree_already_has_is_joined_and_keeps_its_colour() {
    let db = setup_db().await;
    let app = app_on(db.clone());
    let tree_id = new_tree(&app, "Joined Tree").await;
    let tree = Uuid::parse_str(&tree_id).expect("tree id");
    let (existing, created) = TagRepo::create(&db, tree, "witnessed")
        .await
        .expect("creates the tag");
    assert!(created);

    import_gedcom(&app, &db, &tree_id, TAGGED_GEDCOM).await;

    let tags = tree_tags(&db, tree).await;
    let witnessed: Vec<_> = tags
        .iter()
        .filter(|tag| tag.normalized_label == existing.normalized_label)
        .collect();
    assert_eq!(witnessed.len(), 1, "no second tag for the same label");
    assert_eq!(witnessed[0].id, existing.id);
    assert_eq!(
        witnessed[0].color, existing.color,
        "the tree's own colour is kept"
    );
    assert!(
        tree_links(&db, tree)
            .await
            .iter()
            .any(|link| link.tag_id == existing.id && link.family_id.is_some()),
        "the couple is linked to the existing tag"
    );
}
