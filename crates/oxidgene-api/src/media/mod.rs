//! Media storage: accepting a file, putting its bytes somewhere, and
//! recording enough about it to show it later.
//!
//! [`store`] holds the blob store, [`thumbnail`] the raster work and [`pages`]
//! the document page count. [`ingest`] is the one entry point handlers call:
//! it validates, stores, derives, and hands back a row's worth of metadata.

pub mod pages;
pub mod store;
pub mod thumbnail;

use std::path::PathBuf;

use oxidgene_core::error::OxidGeneError;
use uuid::Uuid;

pub use oxidgene_core::types::{guess_mime, is_remote_url, normalize_mime};
pub use store::{FsStore, MediaStore, StoredObject, sha256_hex};

/// Where media files live when nothing says otherwise.
///
/// Follows the platform's user-data convention rather than inventing a path:
/// `$XDG_DATA_HOME/oxidgene/media` (in practice `~/.local/share/oxidgene/media`)
/// on Linux, `~/Library/Application Support/oxidgene/media` on macOS,
/// `%APPDATA%\oxidgene\media` on Windows. That is the directory a user's
/// backup tool already covers, and the one the desktop app can write to
/// without asking.
///
/// The server overrides it with `OXIDGENE_MEDIA_ROOT`, which is what a
/// container deployment mounting a volume will do. Falling back to `./media`
/// only happens when the platform reports no data directory at all.
pub fn default_root() -> PathBuf {
    dirs::data_dir()
        .map(|dir| dir.join("oxidgene").join("media"))
        .unwrap_or_else(|| PathBuf::from("media"))
}

/// Largest single upload accepted, in bytes (128 MiB).
///
/// Deliberately above what the services we exchange with accept — Geneanet
/// caps a media file at 50 MB and refuses anything that is not JPEG, PNG, GIF
/// or PDF — because a ceiling that turns away a scan the user already owns
/// costs more than the memory does: a 1200 dpi colour scan of a register
/// spread, or a dossier PDF of a few hundred pages, clears 64 MiB without
/// being in any way unusual. What still bounds it is that the whole file must
/// fit in memory, because the content hash cannot be known until the last byte
/// has arrived. Anything larger is EPIC H's chunked-upload problem.
pub const MAX_UPLOAD_BYTES: usize = 128 * 1024 * 1024;

/// The formats an upload may be, keyed by the magic bytes that identify them.
///
/// The list is deliberately short. Every entry is either something a scanner
/// or camera produces, or a PDF, and every entry can be served back with a
/// `Content-Type` a browser renders inline. Audio and video are absent on
/// purpose: serving them usefully means `Range` requests and streaming, which
/// arrives with chunked uploads rather than here.
const ACCEPTED: &[(&str, &str)] = &[
    ("image/jpeg", "jpg"),
    ("image/png", "png"),
    ("image/gif", "gif"),
    ("image/bmp", "bmp"),
    ("image/tiff", "tif"),
    ("image/webp", "webp"),
    ("image/x-icon", "ico"),
    ("application/pdf", "pdf"),
];

/// The file extension to store a given MIME type under.
pub fn extension_for(mime_type: &str) -> &'static str {
    ACCEPTED
        .iter()
        .find(|(mime, _)| *mime == mime_type)
        .map(|(_, extension)| *extension)
        .unwrap_or("bin")
}

/// Identify `bytes` by their leading magic numbers.
///
/// The client's declared MIME type and the file's extension are both hints
/// from whoever is uploading, and neither survives a renamed file. What we
/// store, serve back with a `Content-Type`, and hand to a decoder is decided
/// here, from the content itself.
pub fn sniff_mime(bytes: &[u8]) -> Option<&'static str> {
    let starts = |prefix: &[u8]| bytes.starts_with(prefix);
    if starts(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if starts(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("image/png")
    } else if starts(b"GIF87a") || starts(b"GIF89a") {
        Some("image/gif")
    } else if starts(b"BM") {
        Some("image/bmp")
    } else if starts(b"II\x2A\x00")
        || starts(b"MM\x00\x2A")
        || starts(b"II\x2B\x00")
        || starts(b"MM\x00\x2B")
    {
        // The last two are BigTIFF; `pages::count` reads both.
        Some("image/tiff")
    } else if starts(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else if starts(&[0x00, 0x00, 0x01, 0x00]) {
        Some("image/x-icon")
    } else if starts(b"%PDF-") {
        Some("application/pdf")
    } else {
        None
    }
}

/// Everything an upload determined about itself, ready to become a `media` row.
#[derive(Debug, Clone)]
pub struct IngestedMedia {
    /// Original file name, as sent by the client.
    pub file_name: String,
    /// MIME type as sniffed from the content.
    pub mime_type: String,
    /// Key the bytes live under in the [`MediaStore`].
    pub storage_key: String,
    /// Hex SHA-256 of the bytes — also the `ETag` when serving them.
    pub sha256: String,
    pub file_size: i64,
    /// Key of the generated thumbnail, absent for formats we cannot rasterise.
    pub thumbnail_key: Option<String>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    /// Page count, `1` for single-page formats.
    pub page_count: i32,
    /// `true` if these exact bytes were already in the store.
    pub deduplicated: bool,
}

/// Validate an uploaded file, store it, and derive its thumbnail and page count.
///
/// Runs the CPU-bound derivations on a blocking thread. A failure to
/// thumbnail is not a failure to upload: the file is already stored and
/// correct, and a missing thumbnail costs the gallery an icon, so it is logged
/// and the row is written with `thumbnail_key` empty.
pub async fn ingest(
    store: &dyn MediaStore,
    tree_id: Uuid,
    file_name: &str,
    bytes: Vec<u8>,
) -> Result<IngestedMedia, OxidGeneError> {
    if bytes.is_empty() {
        return Err(OxidGeneError::Validation("uploaded file is empty".into()));
    }
    if bytes.len() > MAX_UPLOAD_BYTES {
        return Err(OxidGeneError::Validation(format!(
            "file is {} bytes, over the {MAX_UPLOAD_BYTES}-byte upload limit",
            bytes.len()
        )));
    }

    let Some(mime_type) = sniff_mime(&bytes) else {
        let accepted = ACCEPTED
            .iter()
            .map(|(mime, _)| *mime)
            .collect::<Vec<_>>()
            .join(", ");
        return Err(OxidGeneError::Validation(format!(
            "unsupported file type; accepted types are {accepted}"
        )));
    };

    let stored = store.put(tree_id, extension_for(mime_type), &bytes).await?;

    // Decoding a large scan is seconds of CPU; keep it off the async runtime.
    let derived = tokio::task::spawn_blocking(move || {
        let page_count = pages::count(mime_type, &bytes);
        let dimensions = thumbnail::dimensions(&bytes);
        let thumb = if thumbnail::can_thumbnail(mime_type) {
            match thumbnail::generate(&bytes) {
                Ok(thumb) => Some(thumb),
                Err(err) => {
                    tracing::warn!(%err, mime_type, "thumbnail generation failed");
                    None
                }
            }
        } else {
            None
        };
        (page_count, dimensions, thumb)
    })
    .await
    .map_err(|e| OxidGeneError::Internal(format!("media processing panicked: {e}")))?;
    let (page_count, dimensions, thumb) = derived;

    let thumbnail_key = match thumb {
        Some(thumb) => match store.put(tree_id, thumb.extension, &thumb.bytes).await {
            Ok(stored) => Some(stored.key),
            Err(err) => {
                tracing::warn!(%err, "could not store thumbnail");
                None
            }
        },
        None => None,
    };

    Ok(IngestedMedia {
        file_name: sanitize_file_name(file_name),
        mime_type: mime_type.to_string(),
        storage_key: stored.key,
        sha256: stored.sha256,
        file_size: stored.size,
        thumbnail_key,
        width: dimensions.map(|(w, _)| w as i32),
        height: dimensions.map(|(_, h)| h as i32),
        page_count: page_count as i32,
        deduplicated: stored.deduplicated,
    })
}

/// Reject a crop rectangle that does not fit the media it claims to crop.
///
/// Catching it at write time means a vignette in the database always describes
/// a region that exists, so serving one never has to decide what to do with a
/// rectangle hanging off the edge of the page. Lives here rather than in a
/// handler because REST and GraphQL both create vignettes and must agree.
pub fn validate_crop(
    media: &oxidgene_core::types::Media,
    page: i32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> Result<(), OxidGeneError> {
    let invalid = |message: String| Err(OxidGeneError::Validation(message));

    if width <= 0 || height <= 0 {
        return invalid("crop width and height must be positive".into());
    }
    if x < 0 || y < 0 {
        return invalid("crop origin must not be negative".into());
    }
    if page < 0 || page >= media.page_count {
        return invalid(format!(
            "page {page} is out of range: the media has {} page(s)",
            media.page_count
        ));
    }
    // Dimensions are only known for rasters we decoded at upload. A PDF has
    // none, and a rectangle on one is checked when it is rendered, not here.
    if let (Some(media_width), Some(media_height)) = (media.width, media.height)
        && (x.saturating_add(width) > media_width || y.saturating_add(height) > media_height)
    {
        return invalid(format!(
            "crop {width}×{height} at ({x},{y}) does not fit in {media_width}×{media_height}"
        ));
    }
    Ok(())
}

/// Reduce a client-supplied name to its last component.
///
/// The name is display metadata and a GEDCOM `OBJE.FILE` value, never a path
/// we open — but it does end up in a `Content-Disposition` header and in an
/// exported archive, so a browser or an unzip should not be able to read a
/// directory out of it.
fn sanitize_file_name(raw: &str) -> String {
    let base = raw
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(raw)
        .trim()
        .trim_start_matches('.');
    let cleaned: String = base
        .chars()
        .filter(|c| !c.is_control() && *c != '"')
        .take(255)
        .collect();
    if cleaned.is_empty() {
        "upload".to_string()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let img = image::RgbImage::new(width, height);
        let mut out = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    struct TempRoot(std::path::PathBuf);

    impl TempRoot {
        fn new(tag: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("oxidgene-ingest-{tag}-{}", Uuid::now_v7()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn media_800x600() -> oxidgene_core::types::Media {
        oxidgene_core::types::Media {
            id: Uuid::now_v7(),
            tree_id: Uuid::now_v7(),
            file_name: "scan.jpg".into(),
            mime_type: "image/jpeg".into(),
            file_path: "scan.jpg".into(),
            storage_key: Some("key".into()),
            sha256: Some("digest".into()),
            thumbnail_key: None,
            width: Some(800),
            height: Some(600),
            page_count: 1,
            parent_media_id: None,
            page_index: 0,
            is_document: false,
            file_size: 1,
            title: None,
            description: None,
            date_value: None,
            date_sort: None,
            date_qualifier: Default::default(),
            date_value2: None,
            calendar: Default::default(),
            privacy: Default::default(),
            source_media_type: Default::default(),
            document_category: None,
            place_id: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            deleted_at: None,
        }
    }

    #[test]
    fn a_crop_inside_the_image_is_accepted() {
        validate_crop(&media_800x600(), 0, 10, 10, 100, 100).expect("fits");
        // Flush against the far edge is still inside.
        validate_crop(&media_800x600(), 0, 700, 500, 100, 100).expect("fits exactly");
    }

    #[test]
    fn a_crop_hanging_off_the_edge_is_rejected() {
        assert!(validate_crop(&media_800x600(), 0, 750, 10, 100, 100).is_err());
        assert!(validate_crop(&media_800x600(), 0, 10, 550, 100, 100).is_err());
    }

    #[test]
    fn an_empty_or_negative_crop_is_rejected() {
        assert!(validate_crop(&media_800x600(), 0, 10, 10, 0, 100).is_err());
        assert!(validate_crop(&media_800x600(), 0, 10, 10, 100, -5).is_err());
        assert!(validate_crop(&media_800x600(), 0, -1, 10, 100, 100).is_err());
    }

    #[test]
    fn a_crop_whose_extent_would_overflow_is_rejected_not_wrapped() {
        // Without a saturating add, `x + width` wraps negative and the bound
        // check passes — a rectangle nobody could crop.
        assert!(validate_crop(&media_800x600(), 0, i32::MAX, 0, i32::MAX, 10).is_err());
    }

    #[test]
    fn a_page_the_document_does_not_have_is_rejected() {
        let mut media = media_800x600();
        media.page_count = 3;
        validate_crop(&media, 2, 0, 0, 10, 10).expect("last page is in range");
        assert!(validate_crop(&media, 3, 0, 0, 10, 10).is_err());
        assert!(validate_crop(&media, -1, 0, 0, 10, 10).is_err());
    }

    #[test]
    fn a_pdf_with_no_known_dimensions_is_not_bound_checked() {
        let mut media = media_800x600();
        media.mime_type = "application/pdf".into();
        media.width = None;
        media.height = None;
        media.page_count = 4;
        validate_crop(&media, 3, 5000, 5000, 100, 100).expect("no dimensions, no bound check");
    }

    #[test]
    fn the_default_root_sits_under_the_platform_data_directory() {
        let root = default_root();
        assert!(root.ends_with("oxidgene/media"), "got {}", root.display());
        if let Some(data_dir) = dirs::data_dir() {
            assert!(root.starts_with(&data_dir), "got {}", root.display());
        }
    }

    #[test]
    fn each_accepted_type_sniffs_back_to_itself() {
        assert_eq!(sniff_mime(&png(2, 2)), Some("image/png"));
        assert_eq!(sniff_mime(b"%PDF-1.7\n"), Some("application/pdf"));
        assert_eq!(sniff_mime(b"GIF89a\x01\x00"), Some("image/gif"));
        assert_eq!(sniff_mime(b"II\x2A\x00rest"), Some("image/tiff"));
        assert_eq!(sniff_mime(b"MM\x00\x2Brest"), Some("image/tiff"));
        assert_eq!(
            sniff_mime(b"RIFF\x00\x00\x00\x00WEBPVP8 "),
            Some("image/webp")
        );
        assert_eq!(sniff_mime(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
    }

    #[test]
    fn every_accepted_type_has_an_extension() {
        for (mime, extension) in ACCEPTED {
            assert_eq!(extension_for(mime), *extension);
            assert_ne!(*extension, "bin");
        }
    }

    #[test]
    fn riff_that_is_not_webp_is_not_accepted() {
        // A WAV file also starts with RIFF; only the WEBP form is ours.
        assert_eq!(sniff_mime(b"RIFF\x00\x00\x00\x00WAVEfmt "), None);
    }

    #[test]
    fn an_executable_renamed_to_jpg_is_still_an_executable() {
        assert_eq!(sniff_mime(b"\x7fELF\x02\x01\x01\x00"), None);
        assert_eq!(sniff_mime(b"MZ\x90\x00"), None);
        assert_eq!(sniff_mime(b"<?php system($_GET[0]); ?>"), None);
    }

    #[test]
    fn a_file_name_is_reduced_to_its_last_component() {
        assert_eq!(sanitize_file_name("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_file_name("C:\\scans\\act.pdf"), "act.pdf");
        assert_eq!(sanitize_file_name("  photo.jpg  "), "photo.jpg");
        assert_eq!(sanitize_file_name("re\"name.jpg"), "rename.jpg");
        assert_eq!(sanitize_file_name("/"), "upload");
        assert_eq!(sanitize_file_name(""), "upload");
    }

    #[tokio::test]
    async fn ingesting_a_photo_records_its_size_shape_and_thumbnail() {
        let root = TempRoot::new("photo");
        let store = FsStore::new(&root.0);
        let tree_id = Uuid::now_v7();

        let media = ingest(&store, tree_id, "portrait.png", png(800, 600))
            .await
            .unwrap();

        assert_eq!(media.mime_type, "image/png");
        assert_eq!((media.width, media.height), (Some(800), Some(600)));
        assert_eq!(media.page_count, 1);
        assert!(!media.deduplicated);
        let key = media.thumbnail_key.expect("a photo gets a thumbnail");
        assert!(store.exists(&key).await);
        assert!(store.exists(&media.storage_key).await);
    }

    #[tokio::test]
    async fn a_pdf_is_stored_without_a_thumbnail() {
        let root = TempRoot::new("pdf");
        let store = FsStore::new(&root.0);

        let media = ingest(
            &store,
            Uuid::now_v7(),
            "act.pdf",
            b"%PDF-1.4\nnot a real document".to_vec(),
        )
        .await
        .unwrap();

        assert_eq!(media.mime_type, "application/pdf");
        assert_eq!(media.thumbnail_key, None);
        assert_eq!(media.width, None);
        assert_eq!(media.page_count, 1);
    }

    #[tokio::test]
    async fn the_same_scan_uploaded_twice_lands_on_one_key() {
        let root = TempRoot::new("dedup");
        let store = FsStore::new(&root.0);
        let tree_id = Uuid::now_v7();
        let bytes = png(300, 300);

        let first = ingest(&store, tree_id, "census.png", bytes.clone())
            .await
            .unwrap();
        let second = ingest(&store, tree_id, "census-copy.png", bytes)
            .await
            .unwrap();

        assert_eq!(first.storage_key, second.storage_key);
        assert_eq!(first.sha256, second.sha256);
        assert!(second.deduplicated);
        // The rows still differ: two people, two names, one file on disk.
        assert_eq!(second.file_name, "census-copy.png");
    }

    #[tokio::test]
    async fn an_empty_upload_is_rejected() {
        let root = TempRoot::new("empty");
        let store = FsStore::new(&root.0);
        let err = ingest(&store, Uuid::now_v7(), "nothing.jpg", Vec::new())
            .await
            .unwrap_err();
        assert!(matches!(err, OxidGeneError::Validation(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn an_unsupported_type_is_rejected_before_anything_is_written() {
        let root = TempRoot::new("unsupported");
        let store = FsStore::new(&root.0);
        let tree_id = Uuid::now_v7();

        let err = ingest(&store, tree_id, "payload.jpg", b"\x7fELF\x02\x01".to_vec())
            .await
            .unwrap_err();

        assert!(matches!(err, OxidGeneError::Validation(_)), "got {err:?}");
        assert!(
            !root.0.join(tree_id.to_string()).exists(),
            "a rejected upload should leave no trace"
        );
    }

    #[tokio::test]
    async fn a_file_over_the_limit_is_rejected() {
        let root = TempRoot::new("oversize");
        let store = FsStore::new(&root.0);
        let mut bytes = vec![0u8; MAX_UPLOAD_BYTES + 1];
        bytes[0..3].copy_from_slice(&[0xFF, 0xD8, 0xFF]);

        let err = ingest(&store, Uuid::now_v7(), "huge.jpg", bytes)
            .await
            .unwrap_err();
        assert!(matches!(err, OxidGeneError::Validation(_)), "got {err:?}");
    }
}
