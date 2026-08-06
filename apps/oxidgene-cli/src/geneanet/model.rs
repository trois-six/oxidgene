//! Wire types for Geneanet's private media API, plus the manifest we emit.
//!
//! The API is the one behind <https://www.geneanet.org/media/manager>. It is
//! undocumented, so every struct here is deliberately lenient: unknown fields
//! are ignored and anything Geneanet may omit is an `Option`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

// ─── Wire types (what Geneanet sends) ───────────────────────────────────────

/// One deposit — a single upload, which may hold several pages (`views`).
#[derive(Debug, Clone, Deserialize)]
pub struct Deposit {
    pub id: i64,
    pub title: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub private: bool,
    pub date_create: Option<String>,
    #[serde(default)]
    pub views: Vec<View>,
}

/// One page of a deposit. This is the level media links attach to.
#[derive(Debug, Clone, Deserialize)]
pub struct View {
    pub id: i64,
    pub page: Option<i64>,
    /// Rendition name (`normal`, `medium`, `screen`, `thumbnail`) → URL path.
    #[serde(default)]
    pub files: BTreeMap<String, String>,
}

/// A person a view is attached to.
///
/// `reference_extra_geneweb` is absent for references that point outside the
/// GeneWeb tree — those carry a name but no key we can join on.
#[derive(Debug, Clone, Deserialize)]
pub struct Reference {
    pub firstname: Option<String>,
    pub lastname: Option<String>,
    pub reference_extra_geneweb: Option<GenewebReference>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GenewebReference {
    /// The GeneWeb key, `lastname|firstname|occurrence` — the join key against
    /// a `.gw` export.
    #[serde(rename = "ref")]
    pub key: String,
}

/// One person↔media link as `/media/api/references` returns it.
///
/// This endpoint is what makes the collection cheap: it carries the whole
/// deposit inline, so a handful of paginated calls replace one request per
/// view. What it does *not* say is which page of a multi-page deposit the link
/// sits on — it lists every view — so those few are located separately.
#[derive(Debug, Clone, Deserialize)]
pub struct ReferenceEntry {
    pub deposit: Deposit,
    pub firstname: Option<String>,
    pub lastname: Option<String>,
    pub reference_extra_geneweb: Option<GenewebReference>,
}

impl ReferenceEntry {
    /// Splits off the person half, dropping the deposit.
    pub fn into_reference(self) -> Reference {
        Reference {
            firstname: self.firstname,
            lastname: self.lastname,
            reference_extra_geneweb: self.reference_extra_geneweb,
        }
    }
}

/// Person links keyed by the `(deposit, view)` they sit on.
pub type LocatedReferences = BTreeMap<(i64, i64), Vec<Reference>>;

/// What the browser-assisted collection hands back.
///
/// Same three payloads the networked path fetches, gathered by the user's own
/// browser instead. Portable by construction: the browser does the talking, so
/// nothing here depends on the platform or on which TLS stack the CLI was
/// built against.
#[derive(Debug, Clone, Deserialize)]
pub struct BrowserCollection {
    pub deposits: Vec<Deposit>,
    /// Links from `/media/api/references`, deposit inline.
    #[serde(default)]
    pub references: Vec<ReferenceEntry>,
    /// Links located page by page, keyed `"<depositId>:<viewId>"` — only for
    /// multi-page deposits, which the bulk endpoint cannot pin to a page.
    #[serde(default)]
    pub view_references: BTreeMap<String, Vec<Reference>>,
}

impl BrowserCollection {
    /// Folds both halves into the `(deposit, view) → persons` map the manifest
    /// is built from.
    ///
    /// Mirrors what the networked path does, minus the requests: a link on a
    /// single-page deposit can only belong to that page, and the rest were
    /// already located by the browser.
    pub fn into_references(self) -> (Vec<Deposit>, LocatedReferences) {
        let mut located = LocatedReferences::new();

        for entry in self.references {
            if let [only] = entry.deposit.views.as_slice() {
                let key = (entry.deposit.id, only.id);
                located.entry(key).or_default().push(entry.into_reference());
            }
        }

        for (key, refs) in self.view_references {
            if let Some((deposit_id, view_id)) = parse_view_key(&key) {
                located.insert((deposit_id, view_id), refs);
            }
        }

        (self.deposits, located)
    }
}

/// Parses a `"<depositId>:<viewId>"` key.
fn parse_view_key(key: &str) -> Option<(i64, i64)> {
    let (deposit, view) = key.split_once(':')?;
    Some((deposit.trim().parse().ok()?, view.trim().parse().ok()?))
}

// ─── Manifest (what we emit) ────────────────────────────────────────────────

/// The person↔media mapping a Geneanet export cannot express.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    /// Geneanet host the manifest was collected from.
    pub source: String,
    pub deposit_count: usize,
    pub view_count: usize,
    /// Views attached to at least one person.
    pub linked_view_count: usize,
    /// Distinct GeneWeb keys seen across all views.
    pub person_count: usize,
    /// References that named a person but carried no GeneWeb key, so they
    /// cannot be joined to a `.gw` export.
    pub unjoinable_reference_count: usize,
    pub deposits: Vec<ManifestDeposit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestDeposit {
    pub id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub private: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_create: Option<String>,
    /// Path of the downloaded original, relative to the manifest. Filled in by
    /// `fetch`; absent until then.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_file: Option<String>,
    pub views: Vec<ManifestView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestView {
    pub id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<i64>,
    // `default` pairs with `skip_serializing_if`: an empty collection is left
    // out of the JSON, so reading it back must not require the field.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub files: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<ManifestReference>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestReference {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub firstname: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lastname: Option<String>,
    /// `lastname|firstname|occurrence`, or `None` for a person outside the tree.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geneweb_ref: Option<String>,
}

impl Manifest {
    /// Builds a manifest from collected deposits and their per-view references.
    ///
    /// `references` is keyed by `(deposit_id, view_id)`; a missing entry means
    /// the view has no links, which is not an error — 235 of 614 views were
    /// unlinked on the tree this was built against.
    pub fn build(
        source: String,
        deposits: Vec<Deposit>,
        mut references: LocatedReferences,
    ) -> Self {
        let mut view_count = 0;
        let mut linked_view_count = 0;
        let mut unjoinable_reference_count = 0;
        let mut persons = std::collections::BTreeSet::new();

        let deposits = deposits
            .into_iter()
            .map(|deposit| {
                let views = deposit
                    .views
                    .into_iter()
                    .map(|view| {
                        view_count += 1;
                        let refs = references
                            .remove(&(deposit.id, view.id))
                            .unwrap_or_default();
                        if !refs.is_empty() {
                            linked_view_count += 1;
                        }
                        let references = refs
                            .into_iter()
                            .map(|r| {
                                let geneweb_ref = r.reference_extra_geneweb.map(|g| g.key);
                                match &geneweb_ref {
                                    Some(key) => {
                                        persons.insert(key.clone());
                                    }
                                    None => unjoinable_reference_count += 1,
                                }
                                ManifestReference {
                                    firstname: r.firstname,
                                    lastname: r.lastname,
                                    geneweb_ref,
                                }
                            })
                            .collect();
                        ManifestView {
                            id: view.id,
                            page: view.page,
                            files: view.files,
                            references,
                        }
                    })
                    .collect();

                ManifestDeposit {
                    id: deposit.id,
                    title: deposit.title,
                    kind: deposit.kind,
                    private: deposit.private,
                    date_create: deposit.date_create,
                    local_file: None,
                    views,
                }
            })
            .collect::<Vec<_>>();

        Self {
            source,
            deposit_count: deposits.len(),
            view_count,
            linked_view_count,
            person_count: persons.len(),
            unjoinable_reference_count,
            deposits,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(id: i64, page: i64) -> View {
        View {
            id,
            page: Some(page),
            files: BTreeMap::from([("normal".to_string(), format!("/n/{id}.jpg"))]),
        }
    }

    fn deposit(id: i64, views: Vec<View>) -> Deposit {
        Deposit {
            id,
            title: Some(format!("deposit {id}")),
            kind: Some("portraits".to_string()),
            private: true,
            date_create: Some("2019-04-26".to_string()),
            views,
        }
    }

    fn reference(key: Option<&str>) -> Reference {
        Reference {
            firstname: Some("person_a".to_string()),
            lastname: Some("BRANCH_A".to_string()),
            reference_extra_geneweb: key.map(|k| GenewebReference { key: k.to_string() }),
        }
    }

    /// Guards the shape of the two payloads the whole collector depends on.
    ///
    /// The API is undocumented, so this is the test that fails first if
    /// Geneanet reshapes it. Structure is verbatim from a live response; names
    /// and ids are made up.
    #[test]
    fn decodes_the_live_wire_shape() {
        let deposits: Vec<Deposit> = serde_json::from_str(
            r#"[{
                "thumb": "/public/img/media/deposits/private/aa/bb/222/deadbeef/medium.jpg?t=1",
                "slug": "person-a-111",
                "id": 111,
                "username": "account_a",
                "username_sender": "account_a",
                "title": "person_a",
                "type": "portraits",
                "private": true,
                "views": [{
                    "files": {
                        "normal": "/public/img/media/deposits/private/aa/bb/222/deadbeef/normal.jpg?t=1",
                        "medium": "/public/img/media/deposits/private/aa/bb/222/deadbeef/medium.jpg?t=1",
                        "screen": "/public/img/media/deposits/private/aa/bb/222/deadbeef/screen.jpg?t=1",
                        "thumbnail": "/public/img/media/deposits/private/aa/bb/222/deadbeef/thumbnail.jpg?t=1"
                    },
                    "id": 222,
                    "page": 1
                }],
                "date_create": "2019-04-26T13:58:10+02:00"
            }]"#,
        )
        .expect("the deposits payload decodes");

        assert_eq!(deposits[0].id, 111);
        assert_eq!(deposits[0].kind.as_deref(), Some("portraits"));
        assert!(deposits[0].private);
        assert_eq!(deposits[0].views[0].id, 222);
        // `normal` is the largest rendition the API exposes; there is no
        // `original` (that only comes from /media/download).
        assert!(deposits[0].views[0].files.contains_key("normal"));

        let references: Vec<Reference> = serde_json::from_str(
            r#"[{
                "id": 333,
                "firstname": "person_a",
                "lastname": "BRANCH_A",
                "reference_extra_geneweb": {
                    "id": 444,
                    "ref": "branch_a|person_a|",
                    "link_tree": "https://gw.geneanet.org/account_a"
                }
            }, {
                "id": 555,
                "firstname": "person_b",
                "lastname": "BRANCH_B"
            }]"#,
        )
        .expect("the references payload decodes");

        assert_eq!(
            references[0]
                .reference_extra_geneweb
                .as_ref()
                .map(|g| g.key.as_str()),
            Some("branch_a|person_a|")
        );
        // A person outside the tree: named, but with no key to join on.
        assert!(references[1].reference_extra_geneweb.is_none());
    }

    #[test]
    fn counts_views_persons_and_unjoinable_references() {
        let deposits = vec![
            deposit(1, vec![view(10, 1), view(11, 2)]),
            deposit(2, vec![view(20, 1)]),
        ];
        let references = BTreeMap::from([
            ((1, 10), vec![reference(Some("branch_a|person_a|"))]),
            // Same person again: distinct views, one distinct person.
            ((1, 11), vec![reference(Some("branch_a|person_a|"))]),
            // Named but outside the tree — cannot be joined.
            ((2, 20), vec![reference(None)]),
        ]);

        let manifest = Manifest::build("test".to_string(), deposits, references);

        assert_eq!(manifest.deposit_count, 2);
        assert_eq!(manifest.view_count, 3);
        assert_eq!(manifest.linked_view_count, 3);
        assert_eq!(manifest.person_count, 1);
        assert_eq!(manifest.unjoinable_reference_count, 1);
    }

    #[test]
    fn a_view_with_no_references_is_not_an_error() {
        let deposits = vec![deposit(1, vec![view(10, 1)])];

        let manifest = Manifest::build("test".to_string(), deposits, BTreeMap::new());

        assert_eq!(manifest.view_count, 1);
        assert_eq!(manifest.linked_view_count, 0);
        assert_eq!(manifest.person_count, 0);
        assert!(manifest.deposits[0].views[0].references.is_empty());
    }

    #[test]
    fn a_view_can_link_several_persons() {
        let deposits = vec![deposit(1, vec![view(10, 1)])];
        let references = BTreeMap::from([(
            (1, 10),
            vec![
                reference(Some("branch_a|person_a|")),
                reference(Some("branch_b|person_b|1")),
            ],
        )]);

        let manifest = Manifest::build("test".to_string(), deposits, references);

        assert_eq!(manifest.person_count, 2);
        assert_eq!(manifest.linked_view_count, 1);
        assert_eq!(manifest.deposits[0].views[0].references.len(), 2);
    }

    #[test]
    fn local_file_is_omitted_until_fetch_fills_it() {
        let manifest = Manifest::build(
            "test".to_string(),
            vec![deposit(1, vec![view(10, 1)])],
            BTreeMap::new(),
        );

        let json = serde_json::to_string(&manifest).expect("serialises");

        assert!(!json.contains("local_file"));
    }

    /// A manifest must survive its own round trip.
    ///
    /// The empty collections are the trap: they are skipped on the way out, so
    /// reading them back needs `serde(default)`. Without it every manifest
    /// holding an unlinked view — the majority of them — failed to reload.
    #[test]
    fn a_manifest_with_an_unlinked_view_reloads() {
        let manifest = Manifest::build(
            "test".to_string(),
            vec![deposit(1, vec![view(10, 1)])],
            BTreeMap::new(),
        );

        let json = serde_json::to_string(&manifest).expect("serialises");
        let reloaded: Manifest = serde_json::from_str(&json).expect("round-trips");

        assert_eq!(reloaded.deposits.len(), 1);
        assert!(reloaded.deposits[0].views[0].references.is_empty());
    }
}
