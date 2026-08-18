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
    /// Where on the picture this person is, if the owner drew a box round them.
    ///
    /// Geneanet's media manager shows these as labelled rectangles, and they
    /// are the only record of *which* face in a group photo is whom. On a real
    /// account 245 of 550 links carry one.
    #[serde(default)]
    pub face: Option<Face>,
}

/// A rectangle drawn round somebody on a picture.
#[derive(Debug, Clone, Deserialize)]
pub struct Face {
    pub position: FacePosition,
}

/// The rectangle, as percentages of the picture's own width and height.
///
/// Strings in the payload, and percentages rather than pixels — which is
/// convenient, because it means the box survives being matched to whichever
/// rendition or original we ended up storing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FacePosition {
    pub x1: String,
    pub y1: String,
    pub x2: String,
    pub y2: String,
}

impl FacePosition {
    /// The rectangle in the pixels of an image of this size.
    ///
    /// Returns `None` when the numbers do not parse or the box has no area —
    /// a zero-width crop is not a region, and storing one would put an empty
    /// rectangle on a photograph.
    #[must_use]
    pub fn to_pixels(&self, width: i32, height: i32) -> Option<(i32, i32, i32, i32)> {
        let percent = |value: &str, of: i32| -> Option<i32> {
            let fraction = value.parse::<f64>().ok()? / 100.0;
            Some((fraction * f64::from(of)).round().clamp(0.0, f64::from(of)) as i32)
        };

        let x1 = percent(&self.x1, width)?;
        let y1 = percent(&self.y1, height)?;
        let x2 = percent(&self.x2, width)?;
        let y2 = percent(&self.y2, height)?;

        let (x, y) = (x1.min(x2), y1.min(y2));
        let (w, h) = ((x1 - x2).abs(), (y1 - y2).abs());

        (w > 0 && h > 0).then_some((x, y, w, h))
    }
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
    #[serde(default)]
    pub face: Option<Face>,
}

impl ReferenceEntry {
    /// Splits off the person half, dropping the deposit.
    pub fn into_reference(self) -> Reference {
        Reference {
            firstname: self.firstname,
            lastname: self.lastname,
            reference_extra_geneweb: self.reference_extra_geneweb,
            face: self.face,
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
    /// Absolute URL of the uncompressed original.
    ///
    /// Deposit-level on purpose: `/media/download/` serves a *deposit*, so this
    /// is one image when `views` holds one entry and a ZIP of every page when
    /// it holds several. `views[].files` carries only downsized renditions —
    /// there is no per-page original, and the API exposes none.
    ///
    /// Derivable from `id`, but materialised so a consumer never has to know
    /// how to build it.
    pub original: String,
    /// Path of the downloaded original, relative to the manifest. Filled in by
    /// `fetch`; absent until then.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_file: Option<String>,
    pub views: Vec<ManifestView>,
}

/// Builds the download URL for a deposit's original.
///
/// Absolute, unlike the rendition paths in `views[].files`: those are served
/// from `gw.geneanet.org` while this lives on the API host, so a relative path
/// would be ambiguous about which.
pub fn original_url(base_url: &str, deposit_id: i64) -> String {
    format!(
        "{}/media/download/?deposits[]={deposit_id}",
        base_url.trim_end_matches('/')
    )
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
    /// The box drawn round this person, as percentages. Kept so the import can
    /// turn it into a vignette on the stored picture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face: Option<FacePosition>,
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
                                    face: r.face.map(|f| f.position),
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
                    original: original_url(&source, deposit.id),
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
            face: None,
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
    fn a_face_box_converts_from_percentages_to_pixels() {
        // Geneanet gives percentages as strings; a vignette wants pixels of
        // the picture we actually stored.
        let face = FacePosition {
            x1: "10.0".into(),
            y1: "20.0".into(),
            x2: "60.0".into(),
            y2: "70.0".into(),
        };

        assert_eq!(face.to_pixels(1000, 800), Some((100, 160, 500, 400)));
    }

    #[test]
    fn a_face_box_drawn_backwards_still_has_positive_size() {
        // Dragged bottom-right to top-left, the corners arrive reversed. A
        // negative width would be rejected by the crop validator, so the box
        // is normalised rather than trusted.
        let face = FacePosition {
            x1: "60.0".into(),
            y1: "70.0".into(),
            x2: "10.0".into(),
            y2: "20.0".into(),
        };

        assert_eq!(face.to_pixels(1000, 800), Some((100, 160, 500, 400)));
    }

    #[test]
    fn a_face_box_with_no_area_is_not_a_region() {
        let flat = FacePosition {
            x1: "10.0".into(),
            y1: "20.0".into(),
            x2: "10.0".into(),
            y2: "70.0".into(),
        };
        assert_eq!(flat.to_pixels(1000, 800), None);

        let unparseable = FacePosition {
            x1: "a".into(),
            y1: "20.0".into(),
            x2: "60.0".into(),
            y2: "70.0".into(),
        };
        assert_eq!(unparseable.to_pixels(1000, 800), None);
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
    fn every_deposit_carries_an_absolute_original_url() {
        let manifest = Manifest::build(
            "https://www.geneanet.org".to_string(),
            vec![deposit(16053569, vec![view(10, 1)])],
            BTreeMap::new(),
        );

        assert_eq!(
            manifest.deposits[0].original,
            "https://www.geneanet.org/media/download/?deposits[]=16053569"
        );
    }

    #[test]
    fn a_multi_page_deposit_still_carries_one_original() {
        // Deposit-level, so it is defined whatever the page count — it is the
        // deposit's download, which is a ZIP here. `views.len()` is what tells
        // a consumer which of the two it will get.
        let manifest = Manifest::build(
            "https://www.geneanet.org".to_string(),
            vec![deposit(43994698, vec![view(10, 1), view(11, 2)])],
            BTreeMap::new(),
        );

        assert!(
            manifest.deposits[0]
                .original
                .ends_with("deposits[]=43994698")
        );
        assert_eq!(manifest.deposits[0].views.len(), 2);
    }

    #[test]
    fn the_original_is_never_confused_with_a_rendition() {
        // `views[].files` holds downsized renditions only. Putting the deposit
        // download in there would hand a consumer a whole ZIP under a key that
        // reads like a per-page image.
        let manifest = Manifest::build(
            "https://www.geneanet.org".to_string(),
            vec![deposit(1, vec![view(10, 1)])],
            BTreeMap::new(),
        );

        assert!(!manifest.deposits[0].views[0].files.contains_key("original"));
    }

    #[test]
    fn a_trailing_slash_on_the_base_url_does_not_double_up() {
        assert_eq!(
            original_url("https://www.geneanet.org/", 7),
            "https://www.geneanet.org/media/download/?deposits[]=7"
        );
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
