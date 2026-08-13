//! Where media bytes live.
//!
//! [`MediaStore`] is the seam between the API and whatever holds the actual
//! files. Sprint F.1 ships one implementation, [`FsStore`], which writes to a
//! directory tree on local disk; object storage is an EPIC H concern and slots
//! in behind the same trait without touching a handler.
//!
//! # Content addressing
//!
//! A file's key is derived from the SHA-256 of its bytes, not from its name:
//!
//! ```text
//! {tree_id}/{first 2 hex}/{next 2 hex}/{full 64 hex}.{ext}
//! ```
//!
//! Three properties fall out of that, and all three are things genealogy
//! imports need. Uploading the same scan twice writes one file, which matters
//! when a census page documents eight siblings and arrives once per person.
//! The digest is a free, exact `ETag`, so a browser that has the file never
//! asks for the bytes again. And a corrupted transfer cannot masquerade as the
//! original, because the name would no longer match the content.
//!
//! Keys are scoped per tree rather than global. Deduplication stops at the
//! tree boundary — two trees holding the same photo store it twice — which is
//! the price of being able to delete a tree by removing one directory, with no
//! reference counting and no chance of a purge pulling a file out from under
//! another tree.

use std::path::{Path, PathBuf};

use oxidgene_core::error::OxidGeneError;
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// The outcome of writing bytes to a [`MediaStore`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredObject {
    /// Store-relative key — what goes in `media.storage_key`.
    pub key: String,
    /// Lowercase hex SHA-256 of the content.
    pub sha256: String,
    /// Byte length of the content.
    pub size: i64,
    /// `true` if an identical file was already present and nothing was written.
    pub deduplicated: bool,
}

/// A content-addressed blob store for media files.
#[async_trait::async_trait]
pub trait MediaStore: Send + Sync + std::fmt::Debug {
    /// Write `bytes` under `tree_id` and return the key they landed on.
    ///
    /// `extension` is the lowercase extension to hang off the key (no dot),
    /// used so a file served straight off disk keeps a recognisable name. It
    /// does not affect addressing: the same bytes stored under two different
    /// extensions are two entries.
    async fn put(
        &self,
        tree_id: Uuid,
        extension: &str,
        bytes: &[u8],
    ) -> Result<StoredObject, OxidGeneError>;

    /// Read back the bytes at `key`.
    async fn get(&self, key: &str) -> Result<Vec<u8>, OxidGeneError>;

    /// Whether `key` currently resolves to a stored object.
    async fn exists(&self, key: &str) -> bool;

    /// Remove the object at `key`. Removing a missing key is not an error.
    async fn delete(&self, key: &str) -> Result<(), OxidGeneError>;

    /// Remove everything stored for `tree_id`. Used by the purge worker.
    async fn delete_tree(&self, tree_id: Uuid) -> Result<(), OxidGeneError>;
}

/// Lowercase hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Build the store key for a digest under a tree.
fn key_for(tree_id: Uuid, digest: &str, extension: &str) -> String {
    if extension.is_empty() {
        format!("{tree_id}/{}/{}/{digest}", &digest[0..2], &digest[2..4])
    } else {
        format!(
            "{tree_id}/{}/{}/{digest}.{extension}",
            &digest[0..2],
            &digest[2..4]
        )
    }
}

/// Reject anything that is not a key this store could have produced.
///
/// Keys reach us from the database, and `media.file_path` has always been free
/// text copied out of a GEDCOM `OBJE.FILE` tag — a file someone else authored.
/// Validating the shape rather than the resolved path means a key never gets
/// far enough to be joined onto the store root, so there is no window in which
/// `../../etc/shadow` is a `PathBuf` we are holding.
fn validate_key(key: &str) -> Result<(), OxidGeneError> {
    let reject = |why: &str| {
        Err(OxidGeneError::Validation(format!(
            "invalid media storage key: {why}"
        )))
    };

    let mut parts = key.split('/');
    let (Some(tree), Some(a), Some(b), Some(file), None) = (
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
    ) else {
        return reject("expected {tree}/{aa}/{bb}/{digest}[.ext]");
    };

    if Uuid::parse_str(tree).is_err() {
        return reject("first segment is not a tree UUID");
    }
    let is_lower_hex = |s: &str| {
        s.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase())
    };
    if a.len() != 2 || b.len() != 2 || !is_lower_hex(a) || !is_lower_hex(b) {
        return reject("fan-out segments must be two lowercase hex digits");
    }

    let (digest, extension) = match file.split_once('.') {
        Some((digest, extension)) => (digest, extension),
        None => (file, ""),
    };
    if digest.len() != 64 || !is_lower_hex(digest) {
        return reject("file name must be a 64-character lowercase hex digest");
    }
    if !extension.chars().all(|c| c.is_ascii_alphanumeric()) {
        return reject("extension must be alphanumeric");
    }
    if a != &digest[0..2] || b != &digest[2..4] {
        return reject("fan-out segments do not match the digest");
    }
    Ok(())
}

/// A [`MediaStore`] backed by a directory on the local filesystem.
#[derive(Debug, Clone)]
pub struct FsStore {
    root: PathBuf,
}

impl FsStore {
    /// Create a store rooted at `root`. The directory is created on first
    /// write, so pointing at a path that does not exist yet is fine.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The directory this store writes into.
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn path_for(&self, key: &str) -> Result<PathBuf, OxidGeneError> {
        validate_key(key)?;
        Ok(self.root.join(key))
    }
}

#[async_trait::async_trait]
impl MediaStore for FsStore {
    async fn put(
        &self,
        tree_id: Uuid,
        extension: &str,
        bytes: &[u8],
    ) -> Result<StoredObject, OxidGeneError> {
        let digest = sha256_hex(bytes);
        let key = key_for(tree_id, &digest, extension);
        let path = self.path_for(&key)?;
        let size = bytes.len() as i64;

        if tokio::fs::metadata(&path).await.is_ok() {
            return Ok(StoredObject {
                key,
                sha256: digest,
                size,
                deduplicated: true,
            });
        }

        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Write beside the target and rename into place. A reader that finds
        // the final name finds the whole file: an upload killed halfway leaves
        // a stray temp file, never a truncated one that would then be trusted
        // as a match for its own digest.
        let temp = path.with_extension(format!("{extension}.part-{}", Uuid::now_v7()));
        tokio::fs::write(&temp, bytes).await?;
        if let Err(err) = tokio::fs::rename(&temp, &path).await {
            let _ = tokio::fs::remove_file(&temp).await;
            return Err(err.into());
        }

        Ok(StoredObject {
            key,
            sha256: digest,
            size,
            deduplicated: false,
        })
    }

    async fn get(&self, key: &str) -> Result<Vec<u8>, OxidGeneError> {
        let path = self.path_for(key)?;
        tokio::fs::read(&path).await.map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                OxidGeneError::Internal(format!("media file missing from store: {key}"))
            } else {
                OxidGeneError::Io(err)
            }
        })
    }

    async fn exists(&self, key: &str) -> bool {
        match self.path_for(key) {
            Ok(path) => tokio::fs::metadata(&path).await.is_ok(),
            Err(_) => false,
        }
    }

    async fn delete(&self, key: &str) -> Result<(), OxidGeneError> {
        let path = self.path_for(key)?;
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err.into()),
        }
    }

    async fn delete_tree(&self, tree_id: Uuid) -> Result<(), OxidGeneError> {
        let path = self.root.join(tree_id.to_string());
        match tokio::fs::remove_dir_all(&path).await {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory that cleans up when the test ends.
    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new(tag: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("oxidgene-store-{tag}-{}", Uuid::now_v7()));
            std::fs::create_dir_all(&path).expect("create temp root");
            Self(path)
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn tree() -> Uuid {
        Uuid::now_v7()
    }

    #[test]
    fn the_digest_of_the_empty_input_is_the_known_sha256() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn a_key_fans_out_on_the_first_four_digest_characters() {
        let id = Uuid::nil();
        let digest = "a".repeat(64);
        assert_eq!(
            key_for(id, &digest, "jpg"),
            format!("{id}/aa/aa/{digest}.jpg")
        );
    }

    #[test]
    fn an_extensionless_key_has_no_trailing_dot() {
        let id = Uuid::nil();
        let digest = "b".repeat(64);
        assert_eq!(key_for(id, &digest, ""), format!("{id}/bb/bb/{digest}"));
    }

    #[test]
    fn keys_this_store_produces_validate() {
        let id = tree();
        validate_key(&key_for(id, &sha256_hex(b"scan"), "jpg")).expect("own key is valid");
        validate_key(&key_for(id, &sha256_hex(b"scan"), "")).expect("extensionless key is valid");
    }

    #[test]
    fn a_gedcom_style_relative_path_is_not_a_key() {
        // What `media.file_path` actually holds for a GEDCOM-imported row.
        for path in [
            "media/photo.jpg",
            "../../etc/shadow",
            "/var/lib/oxidgene/x.jpg",
            "C:\\Users\\x\\photo.jpg",
            "",
        ] {
            assert!(
                validate_key(path).is_err(),
                "{path} should not validate as a store key"
            );
        }
    }

    #[test]
    fn traversal_dressed_up_as_a_key_is_rejected() {
        let id = tree();
        let digest = sha256_hex(b"x");
        // Right shape, wrong segments — each one a way of climbing out.
        for key in [
            format!("../{id}/{}/{}/{digest}.jpg", &digest[0..2], &digest[2..4]),
            format!("{id}/../../etc/{digest}.jpg"),
            format!("{id}/{}/{}/../{digest}.jpg", &digest[0..2], &digest[2..4]),
        ] {
            assert!(validate_key(&key).is_err(), "{key} should be rejected");
        }
    }

    #[test]
    fn fan_out_segments_must_agree_with_the_digest() {
        let id = tree();
        let digest = sha256_hex(b"x");
        let forged = format!("{id}/00/00/{digest}.jpg");
        assert!(validate_key(&forged).is_err());
    }

    #[tokio::test]
    async fn a_stored_file_reads_back_byte_for_byte() {
        let root = TempRoot::new("roundtrip");
        let store = FsStore::new(&root.0);
        let id = tree();

        let stored = store.put(id, "jpg", b"not really a jpeg").await.unwrap();
        assert!(!stored.deduplicated);
        assert_eq!(stored.size, 17);
        assert_eq!(stored.sha256, sha256_hex(b"not really a jpeg"));
        assert_eq!(store.get(&stored.key).await.unwrap(), b"not really a jpeg");
    }

    #[tokio::test]
    async fn storing_the_same_bytes_twice_writes_one_file() {
        let root = TempRoot::new("dedup");
        let store = FsStore::new(&root.0);
        let id = tree();

        let first = store.put(id, "jpg", b"census page").await.unwrap();
        let second = store.put(id, "jpg", b"census page").await.unwrap();

        assert_eq!(first.key, second.key);
        assert!(!first.deduplicated);
        assert!(second.deduplicated, "the second write should be a no-op");
    }

    #[tokio::test]
    async fn two_trees_do_not_share_a_key() {
        let root = TempRoot::new("scoping");
        let store = FsStore::new(&root.0);

        let a = store.put(tree(), "jpg", b"shared photo").await.unwrap();
        let b = store.put(tree(), "jpg", b"shared photo").await.unwrap();

        assert_ne!(a.key, b.key, "tree scoping is what makes purge safe");
        assert!(!b.deduplicated);
    }

    #[tokio::test]
    async fn deleting_a_tree_takes_its_files_and_leaves_the_others() {
        let root = TempRoot::new("purge");
        let store = FsStore::new(&root.0);
        let doomed = tree();
        let kept = tree();

        let a = store.put(doomed, "jpg", b"one").await.unwrap();
        let b = store.put(kept, "jpg", b"two").await.unwrap();

        store.delete_tree(doomed).await.unwrap();

        assert!(!store.exists(&a.key).await);
        assert!(store.exists(&b.key).await);
    }

    #[tokio::test]
    async fn deleting_what_is_not_there_is_not_an_error() {
        let root = TempRoot::new("idempotent-delete");
        let store = FsStore::new(&root.0);
        let key = key_for(tree(), &sha256_hex(b"never stored"), "jpg");

        store.delete(&key).await.expect("delete is idempotent");
        store.delete_tree(tree()).await.expect("so is delete_tree");
    }

    #[tokio::test]
    async fn a_missing_file_reads_as_internal_not_as_a_bare_io_error() {
        let root = TempRoot::new("missing");
        let store = FsStore::new(&root.0);
        let key = key_for(tree(), &sha256_hex(b"absent"), "jpg");

        // The row says the bytes exist and they do not: that is our
        // inconsistency to report, not a 404 the client can act on.
        let err = store.get(&key).await.unwrap_err();
        assert!(matches!(err, OxidGeneError::Internal(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn reading_a_non_key_never_touches_the_filesystem() {
        let root = TempRoot::new("traversal-read");
        let store = FsStore::new(&root.0);

        let err = store.get("../../etc/passwd").await.unwrap_err();
        assert!(matches!(err, OxidGeneError::Validation(_)), "got {err:?}");
        assert!(!store.exists("../../etc/passwd").await);
    }

    #[tokio::test]
    async fn a_partial_write_leaves_nothing_addressable_behind() {
        let root = TempRoot::new("no-partials");
        let store = FsStore::new(&root.0);
        let id = tree();

        let stored = store.put(id, "png", b"complete").await.unwrap();
        let dir = root.0.join(&stored.key).parent().unwrap().to_path_buf();
        let names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();

        assert_eq!(names.len(), 1, "temp files should not survive: {names:?}");
        assert!(names[0].ends_with(".png"));
    }
}
