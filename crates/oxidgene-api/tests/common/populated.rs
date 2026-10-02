//! A tree holding one record of every kind, for the guards that must see
//! them all: cross-tree access, purge completeness, history, logs.
//!
//! Built through REST only, from the fictitious family-blocks GEDCOM plus a
//! source, a repository and its link, a citation, a note, a two-page
//! document with a scan, a media link, a vignette used as a portrait, a tag,
//! a witness, an edit or two, and a completed export job. Every free text
//! carries a `NOTE_MARKER`/`PLACE_MARKER` so a test can look for it.

use std::collections::BTreeMap;
use std::io::Cursor;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, header};
use http_body_util::BodyExt;
use oxidgene_db::sea_orm::DatabaseConnection;
use serde_json::{Value, json};
use tower::ServiceExt;

use super::{all_profiles, family_blocks_gedcom, ok, worker_on};

/// The text of the fixture's note: fictitious, and rare enough to search
/// logs for.
pub const NOTE_MARKER: &str = "Quillfeather ledger entry";

/// A place name the fixture records, likewise.
pub const PLACE_MARKER: &str = "Brackenmoor";

/// The fixture's given names and surnames that must never reach a log.
pub const NAME_MARKERS: &[&str] = &["Anchor", NOTE_MARKER, PLACE_MARKER];

/// A tree with every kind of record, and the id of one record of each kind
/// under the path parameter name the routes use (`person_id`, `family_id`,
/// `media_id`, …).
pub struct Populated {
    pub tree_id: String,
    pub ids: BTreeMap<&'static str, String>,
}

impl Populated {
    pub fn id(&self, name: &str) -> &str {
        self.ids
            .get(name)
            .unwrap_or_else(|| panic!("the fixture holds no {name}"))
    }
}

pub fn png(width: u32, height: u32) -> Vec<u8> {
    let mut img = image::RgbImage::new(width, height);
    for (x, y, pixel) in img.enumerate_pixels_mut() {
        *pixel = image::Rgb([(x % 256) as u8, (y % 256) as u8, 90]);
    }
    let mut out = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner()
}

/// Upload `bytes` as a page of `document`; the page.
pub async fn upload_page(app: &Router, tree: &str, document: &str, bytes: &[u8]) -> Value {
    let boundary = "----oxidgeneGuardBoundary";
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"scan.png\"\r\n\
             Content-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(
        format!(
            "\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"document_id\"\r\n\r\n{document}\r\n--{boundary}--\r\n"
        )
        .as_bytes(),
    );
    let request = Request::builder()
        .method(Method::POST)
        .uri(format!("/api/v1/trees/{tree}/media/upload"))
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let page: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    assert!(status.is_success(), "upload failed: {status} {page}");
    page
}

fn id_of(value: &Value) -> String {
    value["id"]
        .as_str()
        .unwrap_or_else(|| panic!("no id in {value}"))
        .to_owned()
}

/// Requests against one tree, recording the ids of what they create.
struct Builder<'a> {
    app: &'a Router,
    tree_id: String,
    ids: BTreeMap<&'static str, String>,
}

impl Builder<'_> {
    fn path(&self, path: &str) -> String {
        format!("/api/v1/trees/{}{path}", self.tree_id)
    }

    fn id(&self, key: &str) -> String {
        self.ids[key].clone()
    }

    async fn send(&self, method: Method, path: &str, body: Value) -> Value {
        ok(self.app, method, &self.path(path), Some(body)).await
    }

    /// POST `body` to `path` and record the created record's id as `key`.
    async fn create(&mut self, key: &'static str, path: &str, body: Value) {
        let created = self.send(Method::POST, path, body).await;
        self.ids.insert(key, id_of(&created));
    }

    /// Import the family blocks as an import job run by hand, so its id is
    /// known.
    async fn import(&mut self, db: &DatabaseConnection, blocks: usize) {
        let request = Request::builder()
            .method(Method::POST)
            .uri(self.path("/import-jobs?format=gedcom"))
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .body(Body::from(family_blocks_gedcom(blocks)))
            .unwrap();
        let response = self.app.clone().oneshot(request).await.unwrap();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let started: Value = serde_json::from_slice(&bytes).unwrap();
        let job = started["job_id"]
            .as_str()
            .expect("an import job")
            .to_owned();
        assert!(worker_on(db).run_once().await.expect("runs the import"));
        let status = ok(
            self.app,
            Method::GET,
            &self.path(&format!("/import-jobs/{job}")),
            None,
        )
        .await;
        assert_eq!(status["phase"], "completed", "fixture import: {status}");
        self.ids.insert("job_id", job);
    }

    /// The anchor, its family, spouse, parents' family, birth and name, and
    /// a person unrelated to it; the anchor becomes the SOSA root.
    async fn people(&mut self) {
        let profiles = all_profiles(self.app, &self.tree_id).await;
        let anchor = profiles
            .iter()
            .find(|p| p["primary_name"]["given_names"] == "Anchor")
            .expect("the anchor");
        let text = |pointer: &str| {
            anchor
                .pointer(pointer)
                .and_then(Value::as_str)
                .unwrap()
                .to_owned()
        };
        let person = text("/person_id");
        let spouse = text("/families_as_spouse/0/spouse_id");
        for (key, pointer) in [
            ("person_id", "/person_id"),
            ("root_person_id", "/person_id"),
            ("child_id", "/person_id"),
            ("record_id", "/person_id"),
            ("spouse_id", "/families_as_spouse/0/spouse_id"),
            ("family_id", "/families_as_spouse/0/family_id"),
            ("child_family_id", "/family_as_child/family_id"),
            ("event_id", "/birth/event_id"),
            ("name_id", "/primary_name/name_id"),
        ] {
            self.ids.insert(key, text(pointer));
        }
        let other = profiles
            .iter()
            .filter_map(|p| p["person_id"].as_str())
            .find(|p| *p != person && *p != spouse)
            .unwrap()
            .to_owned();
        self.ids.insert("other_person_id", other);
        self.send(Method::PUT, "", json!({ "sosa_root_person_id": person }))
            .await;
    }

    /// A place on the anchor's birth, a source held at a repository, a
    /// citation and a note.
    async fn records(&mut self) {
        self.create("place_id", "/places", json!({ "name": PLACE_MARKER }))
            .await;
        let birth = format!("/events/{}", self.id("event_id"));
        self.send(
            Method::PUT,
            &birth,
            json!({ "place_id": self.id("place_id") }),
        )
        .await;
        self.create(
            "source_id",
            "/sources",
            json!({ "title": "Fictitious register" }),
        )
        .await;
        self.create(
            "repository_id",
            "/repositories",
            json!({ "name": "Fictitious archive" }),
        )
        .await;
        let links = format!("/sources/{}/repositories", self.id("source_id"));
        let link = json!({ "repository_id": self.id("repository_id"), "call_number": "F-1" });
        self.create("source_link_id", &links, link).await;
        let citation = json!({ "source_id": self.id("source_id"), "person_id": self.id("person_id"), "page": "f. 1" });
        self.create("citation_id", "/citations", citation).await;
        let note = json!({ "text": NOTE_MARKER, "person_id": self.id("person_id") });
        self.create("note_id", "/notes", note).await;
        let witnesses = format!("{birth}/witnesses");
        let witness = json!({ "person_id": self.id("other_person_id") });
        self.create("witness_id", &witnesses, witness).await;
    }

    /// A two-page document linked to the anchor, tagged, with a crop of its
    /// first page as the anchor's portrait.
    async fn media(&mut self) {
        self.create(
            "media_id",
            "/media/document",
            json!({ "title": "Fictitious scan" }),
        )
        .await;
        let document = self.id("media_id");
        let page = upload_page(self.app, &self.tree_id, &document, &png(64, 48)).await;
        self.ids.insert("page_id", id_of(&page));
        upload_page(self.app, &self.tree_id, &document, &png(40, 30)).await;
        let link = json!({ "media_id": document, "person_id": self.id("person_id") });
        self.create("media_link_id", "/media-links", link).await;
        let crop =
            json!({ "x": 2, "y": 2, "width": 20, "height": 20, "person_id": self.id("person_id") });
        let vignettes = format!("/media/{}/vignettes", self.id("page_id"));
        self.create("vignette_id", &vignettes, crop).await;
        let portrait = format!("/persons/{}/portrait", self.id("person_id"));
        self.send(
            Method::PUT,
            &portrait,
            json!({ "vignette_id": self.id("vignette_id") }),
        )
        .await;
        let tags = format!("/media/{document}/tags");
        self.send(Method::POST, &tags, json!({ "tag": "fictitious" }))
            .await;
    }

    /// Two edits, so the history holds versions; a completed export job;
    /// the newest audit entry.
    async fn history_and_jobs(&mut self, db: &DatabaseConnection) {
        let person = format!("/persons/{}", self.id("person_id"));
        self.send(Method::PUT, &person, json!({ "sex": "male" }))
            .await;
        let name = format!("{person}/names/{}", self.id("name_id"));
        self.send(
            Method::PUT,
            &name,
            json!({ "nickname": "Anchor the elder" }),
        )
        .await;
        let export = ok(self.app, Method::POST, &self.path("/export-jobs"), None).await;
        let job = export["job_id"].as_str().expect("an export job").to_owned();
        self.ids.insert("export_job_id", job);
        assert!(worker_on(db).run_once().await.expect("runs the export"));
        let audit = ok(self.app, Method::GET, &self.path("/audit?first=1"), None).await;
        let entry = audit["edges"][0]["node"]["id"]
            .as_str()
            .expect("an audit entry");
        self.ids.insert("entry_id", entry.to_owned());
    }
}

/// Build the populated tree named `name` in the router over `db`, from
/// `blocks` family blocks (ten persons each).
pub async fn populated_tree(
    app: &Router,
    db: &DatabaseConnection,
    name: &str,
    blocks: usize,
) -> Populated {
    let tree = ok(
        app,
        Method::POST,
        "/api/v1/trees",
        Some(json!({ "name": name })),
    )
    .await;
    let mut builder = Builder {
        app,
        tree_id: id_of(&tree),
        ids: BTreeMap::new(),
    };
    builder.import(db, blocks).await;
    builder.people().await;
    builder.records().await;
    builder.media().await;
    builder.history_and_jobs(db).await;
    Populated {
        tree_id: builder.tree_id,
        ids: builder.ids,
    }
}
