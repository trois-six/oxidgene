//! Placeholders say why they are one (docs/privacy.md §5,
//! docs/authorization.md §6.5): a relative outside a scoped viewer's access
//! scope is marked `not_shared`, a person the privacy rules hide from a
//! Visitor `private`, on the profile's family links, the pedigree's cards
//! and family members, the person page's records and kinship's persons,
//! through REST and GraphQL alike; a person shown in full carries none.
//!
//! The fixture is a fictitious couple, Ashby and Bryony, with two children,
//! Corin and the private Dorran. A Guest limited to Ashby's descendants
//! without their spouses sees Bryony as a *Not shared* boundary; a Visitor
//! of the `site` tree sees Dorran as a *Private* stub.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt as _;
use oxidgene_core::access::{InstanceRole, TreeRole};
use oxidgene_core::sharing::{ScopeDefinition, TreeVisibility};
use oxidgene_db::repo::{
    AppUserRepo, MembershipRepo, NewAppUser, SharedMembership, TreeChanges, TreeRepo,
};
use serde_json::{Value, json};
use tower::ServiceExt as _;
use uuid::Uuid;

use crate::common::oidc::{MockProvider, TRUSTED_CLIENT};
use crate::common::{all_profiles, app_on, import_gedcom, new_tree, setup_db};

const GEDCOM: &str = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n1 CHAR UTF-8\n\
0 @I1@ INDI\n1 NAME Ashby /Wrenfield/\n1 SEX M\n1 BIRT\n2 DATE 1850\n1 DEAT\n2 DATE 1910\n\
0 @I2@ INDI\n1 NAME Bryony /Calloway/\n1 SEX F\n1 BIRT\n2 DATE 1852\n1 DEAT\n2 DATE 1915\n\
0 @I3@ INDI\n1 NAME Corin /Wrenfield/\n1 SEX M\n1 BIRT\n2 DATE 1880\n1 DEAT\n2 DATE 1940\n\
0 @I4@ INDI\n1 NAME Dorran /Wrenfield/\n1 SEX F\n1 RESN privacy\n1 BIRT\n2 DATE 1882\n\
0 @F1@ FAM\n1 HUSB @I1@\n1 WIFE @I2@\n1 CHIL @I3@\n1 CHIL @I4@\n1 MARR\n2 DATE 1878\n\
0 TRLR\n";

struct Fixture {
    app: axum::Router,
    tree: String,
    ids: std::collections::HashMap<String, String>,
    guest: String,
    visitor: String,
    _provider: MockProvider,
}

async fn fixture() -> Fixture {
    let provider = MockProvider::start().await;
    let db = setup_db().await;
    let local = app_on(db.clone());
    let tree = new_tree(&local, "Placeholder Fixture").await;
    import_gedcom(&local, &db, &tree, GEDCOM).await;
    let ids = all_profiles(&local, &tree)
        .await
        .into_iter()
        .map(|p| {
            (
                p["primary_name"]["given_names"]
                    .as_str()
                    .unwrap()
                    .to_string(),
                p["person_id"].as_str().unwrap().to_string(),
            )
        })
        .collect::<std::collections::HashMap<_, _>>();
    let tree_id: Uuid = tree.parse().unwrap();
    TreeRepo::update(
        &db,
        tree_id,
        TreeChanges {
            visibility: Some(TreeVisibility::Site),
            ..TreeChanges::default()
        },
    )
    .await
    .expect("opens the tree to the site");
    let definition: ScopeDefinition = serde_json::from_value(json!({
        "kind": "descendants_of",
        "person_id": ids["Ashby"],
        "include_spouses": false,
    }))
    .unwrap();
    let scope = oxidgene_api::service::access_scope::create(&db, tree_id, &definition)
        .await
        .expect("creates the scope");
    let mut tokens = Vec::new();
    for subject in ["guest", "visitor"] {
        let user = AppUserRepo::create(
            &db,
            NewAppUser {
                issuer: provider.issuer().to_string(),
                subject: subject.into(),
                email: Some(format!("{subject}@example.invalid")),
                email_verified: true,
                provider_name: Some(format!("Fictive {subject}")),
                username: Some(subject.into()),
                locale: None,
                instance_role: InstanceRole::User,
                can_create_trees: false,
            },
        )
        .await
        .expect("creates the user");
        if subject == "guest" {
            MembershipRepo::create_shared(
                &db,
                SharedMembership {
                    tree_id,
                    user_id: user.id,
                    role: TreeRole::Guest,
                    scope_id: Some(scope.id),
                    expires_at: None,
                    created_by: None,
                },
            )
            .await
            .expect("shares the tree");
        }
        tokens.push(provider.access_token(subject, TRUSTED_CLIENT, 3600));
    }
    let (app, _) = provider.app(&db, &[]);
    let visitor = tokens.pop().unwrap();
    let guest = tokens.pop().unwrap();
    Fixture {
        app,
        tree,
        ids,
        guest,
        visitor,
        _provider: provider,
    }
}

impl Fixture {
    async fn read(&self, token: &str, uri: &str, body: Option<Value>) -> Value {
        let request = Request::builder()
            .method(if body.is_some() {
                Method::POST
            } else {
                Method::GET
            })
            .uri(uri)
            .header(header::HOST, "genealogy.example.invalid")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
            .unwrap();
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8_lossy(&bytes).into_owned();
        assert_eq!(status, StatusCode::OK, "{uri}: {text}");
        serde_json::from_str(&text).unwrap()
    }

    fn base(&self) -> String {
        format!("/api/v1/trees/{}", self.tree)
    }
}

/// The pedigree cards of `pedigree` labelled `label`.
fn cards_labelled<'a>(pedigree: &'a Value, label: &str) -> Vec<&'a Value> {
    pedigree["persons"]
        .as_object()
        .unwrap()
        .values()
        .filter(|node| node["placeholder"] == label)
        .collect()
}

#[tokio::test]
async fn a_relative_outside_the_scope_is_marked_not_shared() {
    let f = fixture().await;
    let base = f.base();
    let (ashby, bryony, corin) = (&f.ids["Ashby"], &f.ids["Bryony"], &f.ids["Corin"]);

    let profile = f
        .read(&f.guest, &format!("{base}/profiles/{ashby}"), None)
        .await;
    assert!(profile.get("placeholder").is_none(), "{profile}");
    let link = &profile["families_as_spouse"][0];
    assert_eq!(link["spouse_placeholder"], "not_shared", "{link}");
    assert!(link.get("spouse_given_names").is_none(), "{link}");
    assert_ne!(link["spouse_id"], json!(bryony), "{link}");

    let child = f
        .read(&f.guest, &format!("{base}/profiles/{corin}"), None)
        .await;
    let parents = &child["family_as_child"];
    assert_eq!(parents["mother_placeholder"], "not_shared", "{parents}");
    assert!(parents.get("father_placeholder").is_none(), "{parents}");

    let pedigree = f
        .read(
            &f.guest,
            &format!("{base}/pedigree/{corin}?ancestor_depth=2&descendant_depth=1"),
            None,
        )
        .await;
    let stubs = cards_labelled(&pedigree, "not_shared");
    assert_eq!(stubs.len(), 1, "{pedigree}");
    assert_eq!(stubs[0]["display_name"], "", "{pedigree}");
    assert!(!pedigree.to_string().contains("Bryony"), "{pedigree}");
    // The children without a card of their own are inside the scope.
    assert!(
        pedigree["families"]
            .as_object()
            .unwrap()
            .values()
            .flat_map(|family| family["members"].as_array().unwrap())
            .all(|member| member.get("placeholder").is_none()),
        "{pedigree}"
    );

    let bundle = f
        .read(
            &f.guest,
            &format!("{base}/persons/{ashby}/detail-bundle"),
            None,
        )
        .await;
    let marked: Vec<&Value> = bundle["persons"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|person| person["placeholder"] == "not_shared")
        .collect();
    assert_eq!(marked.len(), 1, "{bundle}");

    #[cfg(feature = "graphql")]
    {
        let query = format!(
            "{{ personProfile(treeId: \"{}\", personId: \"{ashby}\") \
             {{ placeholder familiesAsSpouse {{ spousePlaceholder }} }} }}",
            f.tree
        );
        let body = f
            .read(&f.guest, "/graphql", Some(json!({ "query": query })))
            .await;
        assert!(body.get("errors").is_none(), "{body}");
        let profile = &body["data"]["personProfile"];
        assert_eq!(profile["placeholder"], Value::Null, "{body}");
        assert_eq!(
            profile["familiesAsSpouse"][0]["spousePlaceholder"], "NOT_SHARED",
            "{body}"
        );
    }
}

#[tokio::test]
async fn a_person_hidden_by_the_privacy_rules_is_marked_private() {
    let f = fixture().await;
    let base = f.base();
    let ashby = &f.ids["Ashby"];

    let pedigree = f
        .read(
            &f.visitor,
            &format!("{base}/pedigree/{ashby}?ancestor_depth=1&descendant_depth=1"),
            None,
        )
        .await;
    let stubs = cards_labelled(&pedigree, "private");
    assert_eq!(stubs.len(), 1, "{pedigree}");
    assert!(!pedigree.to_string().contains("Dorran"), "{pedigree}");
    assert!(cards_labelled(&pedigree, "not_shared").is_empty());

    let kinship = f
        .read(
            &f.visitor,
            &format!("{base}/persons/{ashby}/kinship/{}", f.ids["Corin"]),
            None,
        )
        .await;
    assert!(
        kinship["persons"]
            .as_array()
            .unwrap()
            .iter()
            .all(|person| person.get("placeholder").is_none()),
        "{kinship}"
    );

    #[cfg(feature = "graphql")]
    {
        let query = format!(
            "{{ pedigree(treeId: \"{}\", rootPersonId: \"{ashby}\", ancestorDepth: 1, \
             descendantDepth: 1) {{ nodes {{ placeholder }} }} }}",
            f.tree
        );
        let body = f
            .read(&f.visitor, "/graphql", Some(json!({ "query": query })))
            .await;
        assert!(body.get("errors").is_none(), "{body}");
        let labels: Vec<&Value> = body["data"]["pedigree"]["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|node| &node["placeholder"])
            .filter(|label| !label.is_null())
            .collect();
        assert_eq!(labels, [&json!("PRIVATE")], "{body}");
    }
}
