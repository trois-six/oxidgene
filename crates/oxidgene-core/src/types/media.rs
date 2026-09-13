use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::enums::{Calendar, DateQualifier, DocumentCategory, Privacy, SourceMediaType};

/// The MIME type a document row carries.
///
/// Not the type of anything: a document holds no bytes, its pages do. It names
/// what the row *is*, so a reader branching on `mime_type` alone does not
/// mistake it for an image it can render.
pub const DOCUMENT_MIME: &str = "application/x-oxidgene-document";

/// A document shell or one of its file pages (image, PDF, video, etc.).
///
/// `PartialEq` so Dioxus props holding one can diff — a gallery tile is keyed
/// on the media it shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Media {
    pub id: Uuid,
    pub tree_id: Uuid,
    pub file_name: String,
    pub mime_type: String,
    /// Path as it appears in GEDCOM (`OBJE.FILE`) — the producer's own path,
    /// preserved verbatim so an export round-trips. Not where our copy lives.
    pub file_path: String,
    /// Key of the stored bytes in the media store, or `None` when the record
    /// names a file we have never received — every GEDCOM-imported row starts
    /// that way.
    pub storage_key: Option<String>,
    /// Hex SHA-256 of the stored bytes. Doubles as the `ETag`.
    pub sha256: Option<String>,
    /// Key of the generated thumbnail. `None` for formats we cannot rasterise
    /// (PDFs) and for records with no bytes.
    pub thumbnail_key: Option<String>,
    /// Intrinsic pixel size, after applying any EXIF orientation.
    pub width: Option<i32>,
    pub height: Option<i32>,
    /// How many pages this row has, which means one of two things depending on
    /// which kind of row it is — see [`Media::is_document`].
    ///
    /// On a document, the number of page media assembled into it, `0` included:
    /// a document whose pages have all been removed is an empty shell that
    /// still carries its metadata. On a page, the number of images inside the
    /// file itself — a multi-page TIFF is one page media holding several.
    pub page_count: i32,
    /// The document this is a page of, or `None` when this row *is* a document.
    ///
    /// This is the only thing that separates the two kinds of row. A page holds
    /// the bytes (or the remote URL) and nothing else of consequence; the
    /// document holds the title, date, place, category, medium, privacy,
    /// description, tags and note that describe the whole, and holds no bytes
    /// at all.
    pub parent_media_id: Option<Uuid>,
    /// Zero-based position within that document.
    #[serde(default)]
    pub page_index: i32,
    pub file_size: i64,
    pub title: Option<String>,
    pub description: Option<String>,
    /// Date the media was created or applies to — the same shape as an event's,
    /// down to the qualifier and calendar, so one date widget edits both.
    pub date_value: Option<String>,
    /// Normalized Gregorian date for sorting. Derived server-side from
    /// `calendar` + `date_value`; never accepted from a client.
    pub date_sort: Option<NaiveDate>,
    #[serde(default)]
    pub date_qualifier: DateQualifier,
    /// The second date of a range (`Between`, `From`/`To`).
    pub date_value2: Option<String>,
    #[serde(default)]
    pub calendar: Calendar,
    /// What the medium physically is, in GEDCOM's own vocabulary. Exported as
    /// `OBJE.FILE.FORM.TYPE` and read back from it, so this round-trips.
    #[serde(default)]
    pub source_media_type: SourceMediaType,
    /// What kind of *record* it is — the distinction GEDCOM's enumeration
    /// cannot draw, since a census return and a marriage contract are both
    /// `Manuscript` to it. `None` when unclassified, which a photograph
    /// ordinarily is.
    #[serde(default)]
    pub document_category: Option<DocumentCategory>,
    /// Free-form labels used to group and find this media. Tags belong to the
    /// document row, not its individual pages.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Location where the media was created or applies to.
    pub place_id: Option<Uuid>,
    /// Whether this is shown when the tree is published. Recorded now,
    /// enforced when authentication lands — see the roadmap.
    #[serde(default)]
    pub privacy: Privacy,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

impl Media {
    /// Whether this row is a document — the container a gallery shows — rather
    /// than one of its pages.
    ///
    /// Every media a gallery lists is a document, and every set of bytes is a
    /// page of one. An ordinary photograph is not a different kind of thing
    /// from a forty-page register: it is a document with one page. That is why
    /// this is derived from [`Media::parent_media_id`] instead of being stored
    /// beside it — a second column saying the same thing is a second column
    /// that can disagree.
    #[must_use]
    pub fn is_document(&self) -> bool {
        self.parent_media_id.is_none()
    }

    /// Validate page coordinates, checking each known dimension independently.
    /// Imported pages and PDFs may have no known pixel dimensions yet.
    pub fn validate_crop(
        &self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> Result<(), crate::OxidGeneError> {
        let invalid = |message: &str| Err(crate::OxidGeneError::Validation(message.into()));
        if self.is_document() {
            return invalid("a vignette must refer to a page, not a document");
        }
        if width <= 0 || height <= 0 {
            return invalid("crop width and height must be positive");
        }
        if x < 0 || y < 0 {
            return invalid("crop origin must not be negative");
        }
        let (Some(right), Some(bottom)) = (x.checked_add(width), y.checked_add(height)) else {
            return invalid("crop coordinates exceed the supported range");
        };
        if self.width.is_some_and(|bound| right > bound)
            || self.height.is_some_and(|bound| bottom > bound)
        {
            return invalid("crop does not fit within the page dimensions");
        }
        Ok(())
    }
}

/// A link between a media item and a person, event, source, or family.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaLink {
    pub id: Uuid,
    pub media_id: Uuid,
    pub person_id: Option<Uuid>,
    pub event_id: Option<Uuid>,
    pub source_id: Option<Uuid>,
    pub family_id: Option<Uuid>,
    pub sort_order: i32,
}

/// What a person's portrait is, as a single value.
///
/// Modelled as one enum rather than two optional ids so that "both set" is not
/// a state anything has to check for, or handle when it happens anyway.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "id")]
pub enum Portrait {
    /// A whole media file.
    Media(Uuid),
    /// A region of one — a face in a group photograph.
    Vignette(Uuid),
    /// None chosen; a card draws the silhouette.
    None,
}

impl Portrait {
    /// The pair of columns to store, as (media_id, vignette_id).
    #[must_use]
    pub fn to_columns(self) -> (Option<Uuid>, Option<Uuid>) {
        match self {
            Self::Media(id) => (Some(id), None),
            Self::Vignette(id) => (None, Some(id)),
            Self::None => (None, None),
        }
    }
}

/// A rectangular region of a stored media file, kept as coordinates rather
/// than as a second copy of the pixels.
///
/// One parish-register page routinely documents several unrelated families.
/// Recording each entry as a rectangle on the single stored scan means the
/// scan is stored once, a better scan can replace it without orphaning
/// anything, and the crop can still be served as if it were its own image.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Vignette {
    pub id: Uuid,
    /// The media this is a region of. Always a page — the row that holds the
    /// pixels — never the document that groups pages together.
    pub media_id: Uuid,
    /// Crop rectangle, in the source image's own pixel coordinates.
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    /// Who the region shows, if attributed.
    pub person_id: Option<Uuid>,
    /// The event this region is evidence for, if any.
    pub event_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A region to cut out of an image the reader will fetch for themselves.
///
/// Sent instead of a cropped image when the crop cannot be cut here: cutting
/// means re-decoding our own copy, and a remote file is never fetched by us.
/// The rectangle and the size it was measured against are enough for the
/// reader to show exactly the same region, so a face identified on somebody
/// else's photograph draws as a face and not as the whole group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageCrop {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    /// The full image's pixel size, which is what `x`/`y` are measured in.
    pub source_width: i32,
    pub source_height: i32,
}

impl ImageCrop {
    /// A rectangle and the size it was measured against, when both are usable.
    ///
    /// `None` when the pixel size was never recorded, or either is degenerate:
    /// there is then no scale to cut at, and the caller shows the whole image
    /// rather than guessing at one.
    #[must_use]
    pub fn new(
        (x, y, width, height): (i32, i32, i32, i32),
        source: (Option<i32>, Option<i32>),
    ) -> Option<Self> {
        let (source_width, source_height) = (source.0?, source.1?);
        (source_width > 0 && source_height > 0 && width > 0 && height > 0).then_some(Self {
            x,
            y,
            width,
            height,
            source_width,
            source_height,
        })
    }

    /// The crop a reader has to perform themselves, if any.
    ///
    /// `None` whenever the region can be cut here instead — we hold the bytes —
    /// or when nothing could draw it anyway: a file that is not a picture, or
    /// one whose pixel size was never recorded.
    #[must_use]
    pub fn for_remote(media: &Media, x: i32, y: i32, width: i32, height: i32) -> Option<Self> {
        if media.storage_key.is_some() || !is_remote_url(&media.file_path) {
            return None;
        }
        if !is_image_mime(&media.mime_type) {
            return None;
        }
        Self::new((x, y, width, height), (media.width, media.height))
    }
}

/// Where a picture the reader is about to see comes from.
///
/// Never the picture itself. A payload that lists a hundred images — a
/// gallery, a pedigree's portraits — says where each one lives and stays a few
/// kilobytes; the bytes travel over their own request, which the rendering
/// engine caches, decodes off the main thread, and skips entirely for an image
/// that never scrolls into view. Inlining them instead cost a third again in
/// base64, put the whole album through one JSON parse, and defeated every one
/// of those.
///
/// A held variant names the resource, not a URL. Turning it into something
/// drawable is the client's business, and deliberately so: until authentication
/// ships, no backend address may appear in the markup (see
/// `docs/specifications/cross-cutting.md` §7.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImageSource {
    /// An address outside our control, which the reader's engine fetches for
    /// itself. We never proxy somebody else's bandwidth.
    Remote { url: String },
    /// The thumbnail this backend generated for a media it holds.
    Thumbnail { media_id: Uuid },
    /// The region this backend cuts out of a media it holds.
    Crop { vignette_id: Uuid },
}

/// Whether a MIME type names a picture an `<img>` can draw.
#[must_use]
pub fn is_image_mime(mime_type: &str) -> bool {
    mime_type.trim().to_ascii_lowercase().starts_with("image/")
}

/// Whether a media's `file_path` points at something on the web.
///
/// A media does not have to be a file we hold. A GEDCOM `OBJE.FILE` is
/// routinely a URL — an archive's viewer, a photograph on a family site — and
/// those are worth recording even though the bytes are somebody else's. Such a
/// record has no `storage_key` and never will; the browser fetches it directly
/// from the URL, which also means we never become a proxy for someone else's
/// bandwidth.
pub fn is_remote_url(file_path: &str) -> bool {
    let path = file_path.trim();
    path.starts_with("http://") || path.starts_with("https://")
}

/// Guess a MIME type from a file name, a URL, or a bare extension.
///
/// Content sniffing is not available here — a remote media exists precisely so
/// that we never fetch it, and a GEDCOM record names a file we do not have. The
/// extension is the only evidence there is. It decides one thing: whether a
/// viewer embeds the media or offers it as a download, so a wrong guess costs a
/// click, not a security property.
///
/// A bare extension is accepted because that is what GEDCOM's `OBJE.FILE.FORM`
/// actually carries — the 5.5.1 spec calls it the "multimedia format" and the
/// values in the wild are `jpeg`, `bmp`, `png`, not MIME types.
pub fn guess_mime(file_name: &str) -> Option<&'static str> {
    // Strip a query string and fragment first: Geneanet serves
    // `medium.jpg?t=1524948994`, which is a jpg.
    let path = file_name
        .split(['?', '#'])
        .next()
        .unwrap_or(file_name)
        .trim_end_matches('/');
    // `rsplit('.')` on a string with no dot yields the whole string, which is
    // what makes a bare "jpeg" resolve.
    let extension = path
        .rsplit(['.', '/', '\\'])
        .next()
        .filter(|e| !e.is_empty())?
        .to_ascii_lowercase();
    Some(match extension.as_str() {
        "jpg" | "jpeg" | "jpe" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "heic" | "heif" => "image/heic",
        "pdf" => "application/pdf",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "ogv" => "video/ogg",
        "mov" => "video/quicktime",
        "avi" => "video/x-msvideo",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "ogg" | "oga" => "audio/ogg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "txt" => "text/plain",
        "html" | "htm" => "text/html",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "odt" => "application/vnd.oasis.opendocument.text",
        "zip" => "application/zip",
        _ => return None,
    })
}

/// Whether a string looks like a MIME type we can act on.
///
/// `application/octet-stream` answers `false`: it is the value a producer
/// writes when it has nothing to say, so treating it as an answer is how a
/// photograph ends up labelled "OCTET-STREAM" in a gallery while the very same
/// file renders fine in an `<img>` elsewhere.
fn is_informative_mime(mime: &str) -> bool {
    let mime = mime.trim();
    mime.contains('/') && !mime.eq_ignore_ascii_case("application/octet-stream")
}

/// Whether a media is worth handing to an `<img>` rather than an icon.
///
/// A declared picture, obviously. And also one nothing declared: a remote
/// address carrying no extension — a CDN naming its file
/// `AF2bZy…=s64-c-mo` — leaves us no guess to make, so the browser fetching
/// the bytes is the only reader able to identify them. Drawing a folder glyph
/// over a photograph is the worse of the two wrong answers, and the one the
/// reader cannot undo; a picture that fails to load can still fall back.
#[must_use]
pub fn may_draw_as_image(mime_type: &str) -> bool {
    is_image_mime(mime_type) || !is_informative_mime(mime_type)
}

/// The MIME type to believe for a media, given what its producer declared and
/// what its file is called.
///
/// Order of evidence: a real MIME type if one was declared; otherwise whatever
/// the declaration turns out to be an extension for (GEDCOM `FORM` says
/// `jpeg`); otherwise the file name or URL; and `application/octet-stream` only
/// when nothing says anything, which is then honest rather than a default
/// wearing an answer's clothes.
pub fn normalize_mime(declared: Option<&str>, file_name: &str) -> String {
    if let Some(declared) = declared.map(str::trim).filter(|d| !d.is_empty()) {
        if is_informative_mime(declared) {
            return declared.to_string();
        }
        // A bare `FORM jpeg` is information, just not in the shape claimed.
        if let Some(guessed) = guess_mime(declared) {
            return guessed.to_string();
        }
    }
    guess_mime(file_name)
        .unwrap_or("application/octet-stream")
        .to_string()
}

#[cfg(test)]
mod crop_tests {
    use super::*;

    fn page(file_path: &str, mime_type: &str) -> Media {
        Media {
            id: Uuid::nil(),
            tree_id: Uuid::nil(),
            file_name: "medium.jpg".into(),
            mime_type: mime_type.into(),
            file_path: file_path.into(),
            storage_key: None,
            sha256: None,
            thumbnail_key: None,
            width: Some(1600),
            height: Some(1200),
            page_count: 1,
            parent_media_id: Some(Uuid::nil()),
            page_index: 0,
            file_size: 0,
            title: None,
            description: None,
            date_value: None,
            date_sort: None,
            date_qualifier: DateQualifier::default(),
            date_value2: None,
            calendar: Calendar::default(),
            source_media_type: SourceMediaType::default(),
            document_category: None,
            tags: Vec::new(),
            place_id: None,
            privacy: Privacy::default(),
            created_at: DateTime::<Utc>::MIN_UTC,
            updated_at: DateTime::<Utc>::MIN_UTC,
            deleted_at: None,
        }
    }

    const URL: &str = "https://archives.example.org/group/7.jpg";

    #[test]
    fn a_region_of_a_remote_picture_travels_with_the_size_it_was_measured_against() {
        let crop = ImageCrop::for_remote(&page(URL, "image/jpeg"), 120, 40, 200, 260)
            .expect("a remote picture of known size can be cut by the reader");

        assert_eq!(
            (crop.x, crop.y, crop.width, crop.height),
            (120, 40, 200, 260)
        );
        assert_eq!((crop.source_width, crop.source_height), (1600, 1200));
    }

    #[test]
    fn a_region_we_can_cut_ourselves_is_not_sent_as_one() {
        // We hold the bytes: the caller cuts them and sends the region itself,
        // which is smaller and needs no arithmetic at the far end.
        let mut ours = page(URL, "image/jpeg");
        ours.storage_key = Some("tree/scan.jpg".into());

        assert!(ImageCrop::for_remote(&ours, 120, 40, 200, 260).is_none());
    }

    #[test]
    fn nothing_is_sent_for_what_could_not_be_drawn_anyway() {
        // A local path nobody uploaded has no address to draw from; a PDF has
        // no still to cut; and a picture nobody has measured has no scale to
        // cut at, so the whole of it is shown instead of a guess.
        let mut unmeasured = page(URL, "image/jpeg");
        unmeasured.width = None;

        for media in [
            page("media/photo.jpg", "image/jpeg"),
            page(URL, "application/pdf"),
            unmeasured,
        ] {
            assert!(ImageCrop::for_remote(&media, 120, 40, 200, 260).is_none());
        }
        // A degenerate rectangle is not a region.
        assert!(ImageCrop::for_remote(&page(URL, "image/jpeg"), 0, 0, 0, 10).is_none());
    }
}

#[cfg(test)]
mod mime_tests {
    use super::*;

    #[test]
    fn a_url_is_recognised_as_remote_and_a_path_is_not() {
        assert!(is_remote_url("https://archives.example.org/scan/42.jpg"));
        assert!(is_remote_url("http://example.org/photo.png"));
        assert!(is_remote_url("  https://example.org/x.jpg  "));
        // What a GEDCOM more often carries: somebody else's local path.
        assert!(!is_remote_url("D:\\Photos\\grandpere.jpg"));
        assert!(!is_remote_url("media/photo.jpg"));
        assert!(!is_remote_url("ftp://example.org/x.jpg"));
        assert!(!is_remote_url(""));
    }

    #[test]
    fn a_mime_type_is_guessed_from_the_extension() {
        assert_eq!(guess_mime("scan.JPG"), Some("image/jpeg"));
        assert_eq!(guess_mime("acte.pdf"), Some("application/pdf"));
        assert_eq!(guess_mime("interview.mp4"), Some("video/mp4"));
        assert_eq!(guess_mime("recording.mp3"), Some("audio/mpeg"));
    }

    #[test]
    fn a_bare_extension_resolves_because_that_is_what_gedcom_form_carries() {
        assert_eq!(guess_mime("jpeg"), Some("image/jpeg"));
        assert_eq!(guess_mime("JPG"), Some("image/jpeg"));
        assert_eq!(guess_mime("bmp"), Some("image/bmp"));
    }

    #[test]
    fn a_query_string_does_not_hide_the_extension() {
        // Exactly the shape Geneanet writes into `OBJE.FILE`.
        assert_eq!(
            guess_mime("http://gw.geneanet.org/public/img/media/medium.jpg?t=1785419513"),
            Some("image/jpeg")
        );
        assert_eq!(
            guess_mime("http://gw.geneanet.org/public/img/media/medium.PNG?t=1524949083"),
            Some("image/png")
        );
        assert_eq!(
            guess_mime("https://example.org/a.png#top"),
            Some("image/png")
        );
    }

    #[test]
    fn a_dot_in_a_directory_does_not_become_the_extension() {
        // `rsplit` also on the separators, or `site.org/viewer` would resolve
        // its extension to "org/viewer".
        assert_eq!(guess_mime("https://site.org/viewer"), None);
        assert_eq!(guess_mime("archive.xyz"), None);
        assert_eq!(guess_mime(""), None);
    }

    #[test]
    fn a_declared_mime_type_is_believed() {
        assert_eq!(
            normalize_mime(Some("image/webp"), "photo.jpg"),
            "image/webp"
        );
    }

    #[test]
    fn octet_stream_is_treated_as_no_answer() {
        // Both real cases from the sample files: an exporter that wrote
        // `FORM application/octet-stream`, and one that wrote no FORM at all.
        assert_eq!(
            normalize_mime(
                Some("application/octet-stream"),
                "http://gw.geneanet.org/public/img/media/medium.jpg?t=1"
            ),
            "image/jpeg"
        );
        assert_eq!(
            normalize_mime(
                None,
                "http://gw.geneanet.org/public/img/media/medium.bmp?t=1"
            ),
            "image/bmp"
        );
    }

    #[test]
    fn a_gedcom_form_extension_is_read_as_one() {
        assert_eq!(normalize_mime(Some("jpeg"), "unknown"), "image/jpeg");
    }

    #[test]
    fn nothing_known_stays_honestly_unknown() {
        assert_eq!(
            normalize_mime(None, "https://example.org/viewer"),
            "application/octet-stream"
        );
        assert_eq!(normalize_mime(Some(""), ""), "application/octet-stream");
    }

    #[test]
    fn an_unidentified_file_is_offered_to_an_img_anyway() {
        assert!(may_draw_as_image("image/png"));
        // The case this exists for: a CDN address with no extension, which
        // leaves the browser as the only reader able to identify the bytes.
        assert!(may_draw_as_image("application/octet-stream"));
        assert!(may_draw_as_image(""));
        // A declared non-picture is not guessed at: it has said what it is.
        assert!(!may_draw_as_image("application/pdf"));
        assert!(!may_draw_as_image("video/mp4"));
        assert!(!may_draw_as_image(DOCUMENT_MIME));
    }
}
