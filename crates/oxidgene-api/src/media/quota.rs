//! The storage quota of a tree's media (docs/api.md, Media): how many
//! bytes of files a tree may hold in the media store.
//!
//! The quota is a tree's, not an owner's: what a tree holds is one indexed
//! sum over its own rows, a tree outlives its owner — ownership is
//! transferred, a tree can have none, and the `none` mode has no account at
//! all — and what an instance holds in all is bounded by who may create
//! trees. It counts the files a tree's media rows hold, each at its size,
//! whether or not two rows share their bytes.
//!
//! [`QuotaStore`] enforces it where bytes enter the store, whatever brings
//! them — an upload, an archive being imported, a Geneanet import: a file
//! that would take the tree past its quota is refused before it is written.
//! The desktop has no quota: the disk is its user's.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use axum::body::Bytes;
use oxidgene_core::OxidGeneError;
use oxidgene_db::repo::MediaRepo;
use sea_orm::ConnectionTrait;
use uuid::Uuid;

use super::store::{BlobStream, MediaStore, StoredCrop, StoredObject};

/// A [`MediaStore`] that refuses the content objects taking a tree past its
/// media quota.
#[derive(Debug)]
pub struct QuotaStore<'a> {
    inner: &'a dyn MediaStore,
    /// The bytes the tree may still take; `u64::MAX` without a quota.
    remaining: AtomicU64,
    /// Whether an object was refused: an import goes on past a file it
    /// could not store, and fails as a whole once done ([`Self::check`]).
    exceeded: AtomicBool,
}

impl<'a> QuotaStore<'a> {
    /// `inner`, within `quota` bytes of stored media for tree `tree_id`,
    /// what the tree already holds counted; unbounded without a quota.
    ///
    /// # Errors
    ///
    /// Database failures reading what the tree holds.
    pub async fn open(
        inner: &'a dyn MediaStore,
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        quota: Option<u64>,
    ) -> Result<Self, OxidGeneError> {
        let remaining = match quota {
            Some(quota) => quota.saturating_sub(MediaRepo::stored_bytes(db, tree_id).await?),
            None => u64::MAX,
        };
        Ok(Self {
            inner,
            remaining: AtomicU64::new(remaining),
            exceeded: AtomicBool::new(false),
        })
    }

    /// Take `bytes` of what the tree may still hold.
    fn take(&self, bytes: u64) -> Result<(), OxidGeneError> {
        let taken = self
            .remaining
            .try_update(Ordering::AcqRel, Ordering::Acquire, |remaining| {
                if remaining == u64::MAX {
                    Some(remaining)
                } else {
                    remaining.checked_sub(bytes)
                }
            });
        if taken.is_err() {
            self.exceeded.store(true, Ordering::Release);
            return Err(OxidGeneError::QuotaExceeded);
        }
        Ok(())
    }

    /// Fail when an object was refused for the quota: what an import, which
    /// keeps a record whose file it could not store, ends with.
    ///
    /// # Errors
    ///
    /// [`OxidGeneError::QuotaExceeded`].
    pub fn check(&self) -> Result<(), OxidGeneError> {
        if self.exceeded.load(Ordering::Acquire) {
            return Err(OxidGeneError::QuotaExceeded);
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl MediaStore for QuotaStore<'_> {
    async fn put_content(
        &self,
        key: &str,
        sha256: String,
        bytes: Bytes,
    ) -> Result<StoredObject, OxidGeneError> {
        self.take(bytes.len() as u64)?;
        self.inner.put_content(key, sha256, bytes).await
    }

    /// A thumbnail is not a file of the tree: it is not counted, as the
    /// sum of what the tree holds does not count it.
    async fn put_rendition(
        &self,
        tree_id: Uuid,
        extension: &str,
        bytes: Bytes,
    ) -> Result<StoredObject, OxidGeneError> {
        self.inner.put_rendition(tree_id, extension, bytes).await
    }

    async fn put_job_bytes(&self, key: &str, bytes: &[u8]) -> Result<(), OxidGeneError> {
        self.inner.put_job_bytes(key, bytes).await
    }

    #[cfg(feature = "auth")]
    async fn put_user_bytes(&self, key: &str, bytes: &[u8]) -> Result<(), OxidGeneError> {
        self.inner.put_user_bytes(key, bytes).await
    }

    async fn job_keys(&self, job_id: Uuid) -> Result<Vec<String>, OxidGeneError> {
        self.inner.job_keys(job_id).await
    }

    async fn get(&self, key: &str) -> Result<Vec<u8>, OxidGeneError> {
        self.inner.get(key).await
    }

    async fn put_file(&self, key: &str, path: &Path) -> Result<StoredObject, OxidGeneError> {
        self.inner.put_file(key, path).await
    }

    async fn get_to_file(&self, key: &str, path: &Path) -> Result<(), OxidGeneError> {
        self.inner.get_to_file(key, path).await
    }

    async fn get_stream(&self, key: &str) -> Result<BlobStream, OxidGeneError> {
        self.inner.get_stream(key).await
    }

    async fn exists(&self, key: &str) -> bool {
        self.inner.exists(key).await
    }

    async fn delete(&self, key: &str) -> Result<(), OxidGeneError> {
        self.inner.delete(key).await
    }

    async fn delete_tree(&self, tree_id: Uuid) -> Result<(), OxidGeneError> {
        self.inner.delete_tree(tree_id).await
    }

    async fn delete_job(&self, job_id: Uuid) -> Result<(), OxidGeneError> {
        self.inner.delete_job(job_id).await
    }

    async fn job_ids(&self) -> Result<Vec<Uuid>, OxidGeneError> {
        self.inner.job_ids().await
    }

    async fn put_crop(&self, key: &str, bytes: Bytes) -> Result<(), OxidGeneError> {
        self.inner.put_crop(key, bytes).await
    }

    async fn crop_objects(&self, tree_id: Uuid) -> Result<Vec<StoredCrop>, OxidGeneError> {
        self.inner.crop_objects(tree_id).await
    }

    async fn delete_crops(
        &self,
        tree_id: Uuid,
        vignette_id: Option<Uuid>,
    ) -> Result<(), OxidGeneError> {
        self.inner.delete_crops(tree_id, vignette_id).await
    }

    async fn tree_ids(&self) -> Result<Vec<Uuid>, OxidGeneError> {
        self.inner.tree_ids().await
    }
}
