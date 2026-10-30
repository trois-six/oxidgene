//! The descent of a family name over REST and GraphQL: the same branches,
//! lines, SOSA marks, unfolding and errors on both surfaces.
//!
//! All data is fictitious.

use crate::common;

use axum::Router;
use axum::http::{Method, StatusCode};
use serde_json::{Value, json};

use common::{
    all_profiles, app_on, gql, gql_error_code, import_gedcom, new_tree, ok, send, setup_db,
};

/// Two branches of the Ashdown name: a root with a son and a married
/// daughter, the son's wife carrying the name as her married name, and a
/// second root whose father bears another name.
const GEDCOM: &str = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n2 FORM LINEAGE-LINKED\n1 CHAR UTF-8\n\
0 @I1@ INDI\n1 NAME Arthur /Ashdown/\n1 SEX M\n1 BIRT\n2 DATE 1800\n1 FAMS @F1@\n\
0 @I2@ INDI\n1 NAME Clara /Birchley/\n1 SEX F\n1 FAMS @F1@\n\
0 @I3@ INDI\n1 NAME Bernard /Ashdown/\n1 SEX M\n1 BIRT\n2 DATE 1830\n1 FAMC @F1@\n1 FAMS @F2@\n\
0 @I4@ INDI\n1 NAME Delia /Ashdown/\n1 SEX F\n1 BIRT\n2 DATE 1825\n1 FAMC @F1@\n1 FAMS @F3@\n\
0 @I5@ INDI\n1 NAME Cyril /Coldwell/\n1 SEX M\n1 FAMS @F3@\n\
0 @I6@ INDI\n1 NAME Alice /Coldwell/\n1 SEX F\n1 FAMC @F3@\n\
0 @I7@ INDI\n1 NAME Beatrice /Dunmore/\n1 SEX F\n1 NAME Beatrice /Ashdown/\n2 TYPE married\n1 FAMS @F2@\n\
0 @I8@ INDI\n1 NAME Dorian /Ashdown/\n1 SEX M\n1 BIRT\n2 DATE ABT 1860\n1 DEAT Y\n1 FAMC @F2@\n\
0 @I9@ INDI\n1 NAME Cyril /Ashdown/\n1 SEX M\n1 FAMC @F4@\n\
0 @I10@ INDI\n1 NAME Bernard /Elmsworth/\n1 SEX M\n1 FAMS @F4@\n\
0 @F1@ FAM\n1 HUSB @I1@\n1 WIFE @I2@\n1 CHIL @I3@\n1 CHIL @I4@\n1 MARR\n2 DATE 1822\n\
0 @F2@ FAM\n1 HUSB @I3@\n1 WIFE @I7@\n1 CHIL @I8@\n1 MARR\n2 DATE 1855\n\
0 @F3@ FAM\n1 HUSB @I5@\n1 WIFE @I4@\n1 CHIL @I6@\n\
0 @F4@ FAM\n1 HUSB @I10@\n1 CHIL @I9@\n\
0 TRLR\n";

/// The first page of the Ashdown descent, as both surfaces must show it.
const FIRST_PAGE: [&[&str]; 2] = [
    &[
        "0 person Arthur Ashdown 1800- sosa & 1822 Clara Birchley sosa",
        "1 person Delia Ashdown 1825- & Cyril Coldwell",
        "2 others 1",
        "1 person Bernard Ashdown 1830- sosa & 1855 Beatrice Dunmore sosa",
        "2 person Dorian Ashdown ca 1860-† sosa",
    ],
    &["0 person Cyril Ashdown parents"],
];

const QUERY: &str = "query($treeId: ID!, $value: String!, $offset: Int, $personId: ID) {
  familyNameDescent(treeId: $treeId, value: $value, offset: $offset, personId: $personId) {
    value bearerCount branchCount offset
    byMarriage { givenNames }
    branches {
      number rootHasParents personCount
      lines {
        kind depth count branch unfoldPersonId
        marriage { year qualifier year2 }
        person { ...P }
        spouse { ...P }
      }
    }
  }
}
fragment P on GqlDescentPerson {
  personId sex givenNames surname deceased sosaAncestor
  birth { year qualifier year2 }
  death { year qualifier year2 }
}";

/// The tree, its persons' ids by given and surname, and the app.
struct Fixture {
    app: Router,
    tree: String,
    ids: Vec<(String, String)>,
}

impl Fixture {
    async fn new() -> Self {
        let db = setup_db().await;
        let app = app_on(db.clone());
        let tree = new_tree(&app, "Descent Fixture").await;
        import_gedcom(&app, &db, &tree, GEDCOM).await;
        let ids = all_profiles(&app, &tree)
            .await
            .iter()
            .map(|p| {
                let name = &p["primary_name"];
                (
                    format!(
                        "{} {}",
                        name["given_names"].as_str().unwrap(),
                        name["surname"].as_str().unwrap()
                    ),
                    p["person_id"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        let fixture = Self { app, tree, ids };
        // Dorian is the SOSA root: his father's line are his ancestors.
        let root = fixture.id("Dorian Ashdown");
        ok(
            &fixture.app,
            Method::PUT,
            &format!("/api/v1/trees/{}", fixture.tree),
            Some(json!({ "sosa_root_person_id": root })),
        )
        .await;
        fixture
    }

    fn id(&self, name: &str) -> String {
        self.ids
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("no {name}"))
            .1
            .clone()
    }

    fn rest_uri(&self, query: &str) -> String {
        format!(
            "/api/v1/trees/{}/dictionary/family-names/descent?{query}",
            self.tree
        )
    }

    async fn rest(&self, query: &str) -> Value {
        ok(&self.app, Method::GET, &self.rest_uri(query), None).await
    }

    async fn graphql(&self, variables: Value) -> Value {
        let mut variables = variables;
        variables["treeId"] = json!(self.tree);
        gql(&self.app, QUERY, variables).await
    }
}

/// A year as a line shows it: `1800`, `ca 1860`, `1691..1693`.
fn year(value: &Value) -> String {
    let Some(year) = value.get("year").and_then(Value::as_i64) else {
        return String::new();
    };
    let qualifier = value["qualifier"]
        .as_str()
        .unwrap_or_default()
        .to_lowercase();
    let mark = match qualifier.as_str() {
        "about" => "ca ",
        _ => "",
    };
    format!("{mark}{year}")
}

/// A person, read from either surface's field names.
fn person(value: &Value, camel: bool) -> String {
    let field =
        |snake: &str, camel_name: &str| value[if camel { camel_name } else { snake }].clone();
    let birth = year(&value["birth"]);
    let mut death = year(&value["death"]);
    if death.is_empty() && value["deceased"] == true {
        death = "†".to_string();
    }
    let mut text = format!(
        "{} {}",
        field("given_names", "givenNames")
            .as_str()
            .unwrap_or_default(),
        value["surname"].as_str().unwrap_or_default()
    );
    if !birth.is_empty() || !death.is_empty() {
        text += &format!(" {birth}-{death}");
    }
    if field("sosa_ancestor", "sosaAncestor") == true {
        text += " sosa";
    }
    text
}

/// A line as `depth kind person & marriage spouse`, the same text whichever
/// surface it came from.
fn line(value: &Value, branch: &Value, camel: bool) -> String {
    let kind = value["kind"].as_str().unwrap().to_lowercase();
    let mut text = format!("{} {kind}", value["depth"]);
    if !value["person"].is_null() {
        text += &format!(" {}", person(&value["person"], camel));
    }
    if value["depth"] == 0 {
        let parents = if camel {
            "rootHasParents"
        } else {
            "root_has_parents"
        };
        if branch[parents] == true {
            text += " parents";
        }
    }
    if !value["spouse"].is_null() || !value["marriage"].is_null() {
        text += " &";
        let married = year(&value["marriage"]);
        if !married.is_empty() {
            text += &format!(" {married}");
        }
        if !value["spouse"].is_null() {
            text += &format!(" {}", person(&value["spouse"], camel));
        }
    }
    if matches!(kind.as_str(), "others" | "more") {
        text += &format!(" {}", value["count"]);
    }
    text
}

fn branches(descent: &Value, camel: bool) -> Vec<Vec<String>> {
    descent["branches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|branch| {
            branch["lines"]
                .as_array()
                .unwrap()
                .iter()
                .map(|l| line(l, branch, camel))
                .collect()
        })
        .collect()
}

fn expected(page: &[&[&str]]) -> Vec<Vec<String>> {
    page.iter()
        .map(|lines| lines.iter().map(|l| (*l).to_string()).collect())
        .collect()
}

#[tokio::test]
async fn the_branches_of_a_name_read_the_same_on_both_surfaces() {
    let fixture = Fixture::new().await;

    let rest = fixture.rest("value=Ashdown").await;
    assert_eq!(rest["bearer_count"], 5);
    assert_eq!(rest["branch_count"], 2);
    assert_eq!(branches(&rest, false), expected(&FIRST_PAGE));
    // Beatrice carries the name by marriage only, beside her husband.
    assert_eq!(rest["by_marriage"], json!([]));

    let gql = fixture.graphql(json!({ "value": "Ashdown" })).await;
    assert!(gql.get("errors").is_none(), "{gql}");
    let descent = &gql["data"]["familyNameDescent"];
    assert_eq!(descent["bearerCount"], 5);
    assert_eq!(descent["branchCount"], 2);
    assert_eq!(branches(descent, true), expected(&FIRST_PAGE));
}

#[tokio::test]
async fn a_bearer_unfolds_alone_and_a_page_starts_at_its_offset() {
    let fixture = Fixture::new().await;
    let bernard = fixture.id("Bernard Ashdown");
    let unfolded: &[&[&str]] = &[&[
        "0 person Bernard Ashdown 1830- sosa & 1855 Beatrice Dunmore sosa",
        "1 person Dorian Ashdown ca 1860-† sosa",
    ]];

    let rest = fixture
        .rest(&format!("value=Ashdown&person_id={bernard}"))
        .await;
    assert_eq!(branches(&rest, false), expected(unfolded));
    assert_eq!(rest["branches"][0]["number"], 1);
    let gql = fixture
        .graphql(json!({ "value": "Ashdown", "personId": bernard }))
        .await;
    assert_eq!(
        branches(&gql["data"]["familyNameDescent"], true),
        expected(unfolded)
    );

    let second: &[&[&str]] = &[FIRST_PAGE[1]];
    let rest = fixture.rest("value=Ashdown&offset=1").await;
    assert_eq!(rest["offset"], 1);
    assert_eq!(branches(&rest, false), expected(second));
    let gql = fixture
        .graphql(json!({ "value": "Ashdown", "offset": 1 }))
        .await;
    assert_eq!(gql["data"]["familyNameDescent"]["offset"], 1);
    assert_eq!(
        branches(&gql["data"]["familyNameDescent"], true),
        expected(second)
    );
}

#[tokio::test]
async fn a_married_name_alone_is_listed_apart_and_an_unknown_name_is_empty() {
    let fixture = Fixture::new().await;

    let rest = fixture.rest("value=Dunmore").await;
    assert_eq!(rest["branch_count"], 1);
    let rest = fixture.rest("value=Nobody").await;
    assert_eq!(rest["bearer_count"], 0);
    assert_eq!(rest["branches"], json!([]));
    let gql = fixture.graphql(json!({ "value": "Nobody" })).await;
    assert_eq!(gql["data"]["familyNameDescent"]["branches"], json!([]));
}

#[tokio::test]
async fn a_blank_name_or_a_person_outside_the_descent_is_refused_on_both_surfaces() {
    let fixture = Fixture::new().await;

    let (status, _) = send(
        &fixture.app,
        Method::GET,
        &fixture.rest_uri("value=%20"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let gql = fixture.graphql(json!({ "value": " " })).await;
    assert_eq!(gql_error_code(&gql), "VALIDATION_ERROR");

    let stranger = fixture.id("Cyril Coldwell");
    let (status, _) = send(
        &fixture.app,
        Method::GET,
        &fixture.rest_uri(&format!("value=Ashdown&person_id={stranger}")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let gql = fixture
        .graphql(json!({ "value": "Ashdown", "personId": stranger }))
        .await;
    assert_eq!(gql_error_code(&gql), "NOT_FOUND");
}
