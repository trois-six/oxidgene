//! The canary persons of the privacy tests (docs/privacy.md §11,
//! docs/authorization.md §12): a fictitious family whose members a viewer
//! may see, and canaries a viewer who may not see them must never meet — in
//! a body, a count, a rank, a suggestion, a statistic or an export.
//!
//! Every canary bears a surname found nowhere else, so a test looks for it
//! in whatever it reads:
//!
//! | Canary | Why it is hidden from a Visitor |
//! |---|---|
//! | [`CONTEMPORARY`] | Born in 1990, no death recorded: the contemporary rule |
//! | [`PRIVATE`] | Long dead, but marked private (`RESN confidential`) |
//! | [`EXCLUDED`] | Long dead and public, excluded from sharing by its test |
//! | [`OUTSIDE`] | Long dead and public, unrelated to the SOSA root: outside a scope of the root's lineage |
//!
//! The contemporary canary also carries details of their own — an
//! occupation at a place, a note — that must go with them:
//! [`CONTEMPORARY_PLACE`], [`CONTEMPORARY_NOTE`].

#[cfg(feature = "auth")]
use std::collections::HashMap;

use axum::Router;
use oxidgene_db::sea_orm::DatabaseConnection;

#[cfg(feature = "auth")]
use super::all_profiles;
use super::{import_gedcom, new_tree};

/// The contemporary canary's surname.
pub const CONTEMPORARY: &str = "Canarycontemporary";
/// The private canary's surname.
pub const PRIVATE: &str = "Canaryprivate";
/// The excluded canary's surname.
pub const EXCLUDED: &str = "Canaryexcluded";
/// The surname of the canary outside the root's lineage.
pub const OUTSIDE: &str = "Canaryoutside";
/// Where the contemporary canary works.
pub const CONTEMPORARY_PLACE: &str = "Quillhaven";
/// The note on the contemporary canary.
pub const CONTEMPORARY_NOTE: &str = "Canary ledger remark";

/// Every canary surname.
pub const CANARIES: &[&str] = &[CONTEMPORARY, PRIVATE, EXCLUDED, OUTSIDE];

/// Four generations of the fictitious Wrenfield family, long dead and
/// public, with the canaries among them: the private canary is a child of
/// the first couple, the excluded canary a spouse in the third generation,
/// the contemporary canary a child of the fourth, and the outside canary
/// nobody's relative.
const GEDCOM: &str = "0 HEAD\n\
1 GEDC\n\
2 VERS 5.5.1\n\
2 FORM LINEAGE-LINKED\n\
1 CHAR UTF-8\n\
0 @I1@ INDI\n\
1 NAME Ashby /Wrenfield/\n\
1 SEX M\n\
1 BIRT\n\
2 DATE 1850\n\
1 DEAT\n\
2 DATE 1920\n\
1 FAMS @F1@\n\
0 @I3@ INDI\n\
1 NAME Mabel /Wrenfield/\n\
1 SEX F\n\
1 BIRT\n\
2 DATE 1855\n\
1 DEAT\n\
2 DATE 1930\n\
1 FAMS @F1@\n\
0 @I4@ INDI\n\
1 NAME Corwin /Wrenfield/\n\
1 SEX M\n\
1 BIRT\n\
2 DATE 1880\n\
1 DEAT\n\
2 DATE 1950\n\
1 FAMC @F1@\n\
1 FAMS @F2@\n\
0 @I5@ INDI\n\
1 NAME Brindle /Canaryprivate/\n\
1 SEX F\n\
1 RESN confidential\n\
1 BIRT\n\
2 DATE 1882\n\
1 DEAT\n\
2 DATE 1960\n\
1 FAMC @F1@\n\
0 @I6@ INDI\n\
1 NAME Edda /Wrenfield/\n\
1 SEX F\n\
1 BIRT\n\
2 DATE 1885\n\
1 DEAT\n\
2 DATE 1955\n\
1 FAMS @F2@\n\
0 @I7@ INDI\n\
1 NAME Harlan /Wrenfield/\n\
1 SEX M\n\
1 BIRT\n\
2 DATE 1910\n\
1 DEAT\n\
2 DATE 1990\n\
1 FAMC @F2@\n\
1 FAMS @F3@\n\
0 @I8@ INDI\n\
1 NAME Thistle /Canaryexcluded/\n\
1 SEX F\n\
1 BIRT\n\
2 DATE 1912\n\
1 DEAT\n\
2 DATE 1999\n\
1 FAMS @F3@\n\
0 @I2@ INDI\n\
1 NAME Quillon /Canarycontemporary/\n\
1 SEX M\n\
1 BIRT\n\
2 DATE 1990\n\
1 OCCU Lamplighter\n\
2 DATE 2015\n\
2 PLAC Quillhaven, Shire\n\
1 NOTE Canary ledger remark\n\
1 FAMC @F3@\n\
0 @I9@ INDI\n\
1 NAME Fallow /Canaryoutside/\n\
1 SEX U\n\
1 BIRT\n\
2 DATE 1800\n\
1 DEAT\n\
2 DATE 1870\n\
0 @F1@ FAM\n\
1 HUSB @I1@\n\
1 WIFE @I3@\n\
1 CHIL @I4@\n\
1 CHIL @I5@\n\
1 MARR\n\
2 DATE 1878\n\
0 @F2@ FAM\n\
1 HUSB @I4@\n\
1 WIFE @I6@\n\
1 CHIL @I7@\n\
0 @F3@ FAM\n\
1 HUSB @I7@\n\
1 WIFE @I8@\n\
1 CHIL @I2@\n\
0 TRLR\n";

/// A tree holding the canaries, and each person's id by given name (read
/// by the exclusion tests, which only the `auth` build has).
pub struct Canaries {
    pub tree: String,
    #[cfg(feature = "auth")]
    pub persons: HashMap<String, String>,
}

impl Canaries {
    /// Import the canary family into a new tree of `app` over `db`.
    pub async fn new(app: &Router, db: &DatabaseConnection) -> Self {
        let tree = new_tree(app, "Wrenfield").await;
        import_gedcom(app, db, &tree, GEDCOM).await;
        #[cfg(feature = "auth")]
        let persons = all_profiles(app, &tree)
            .await
            .iter()
            .map(|profile| {
                (
                    profile["primary_name"]["given_names"]
                        .as_str()
                        .unwrap()
                        .to_owned(),
                    profile["person_id"].as_str().unwrap().to_owned(),
                )
            })
            .collect();
        Self {
            tree,
            #[cfg(feature = "auth")]
            persons,
        }
    }

    /// The id of the person whose given name is `given`.
    #[cfg(feature = "auth")]
    pub fn person(&self, given: &str) -> &str {
        &self.persons[given]
    }
}
