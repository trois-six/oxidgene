//! The media objects an import job writes, recorded so that they can be
//! taken back.
//!
//! An import writes its rows in one database transaction, which a
//! cancellation or a failure rolls back; the media files it stored meanwhile
//! are outside that transaction. [`RecordingStore`] notes each content key
//! the job is about to write as a small object of its own under the job's
//! prefix, `jobs/{job_id}/created-N`, *before* writing it, so no object the
//! job wrote can escape the record, whatever stops the process.
//! [`discard_unreferenced`] then deletes the recorded objects that no media
//! row names — every one after a rollback, none after a commit, apart from
//! a file whose row was never written — and the records with them.
//!
//! A recorded key may have been in store before the job, deduplicated by an
//! earlier upload: a row names it, and it is kept. Keys are scoped to their
//! tree and a tree runs one job at a time, so no other writer is storing the
//! same key while the job holds the tree.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use oxidgene_core::OxidGeneError;
use oxidgene_db::repo::MediaRepo;
use sea_orm::ConnectionTrait;
use uuid::Uuid;

use super::store::{BlobStream, MediaStore, StoredObject, content_key_tree};

/// The prefix of a job's records of the objects it wrote.
const CREATED: &str = "created-";

/// A [`MediaStore`] that records, under job `job_id`, every content object
/// written through it, before writing it.
#[derive(Debug)]
pub(crate) struct RecordingStore<'a> {
    inner: &'a dyn MediaStore,
    job_id: Uuid,
}

impl<'a> RecordingStore<'a> {
    pub(crate) fn new(inner: &'a dyn MediaStore, job_id: Uuid) -> Self {
        Self { inner, job_id }
    }
}

#[async_trait::async_trait]
impl MediaStore for RecordingStore<'_> {
    async fn put_content(
        &self,
        key: &str,
        sha256: String,
        bytes: &[u8],
    ) -> Result<StoredObject, OxidGeneError> {
        // Named after a fresh time-ordered id, so that no record replaces
        // another, of this attempt or of an earlier one.
        let record = format!("jobs/{}/{CREATED}{}", self.job_id, Uuid::now_v7().as_u128());
        self.inner.put_job_bytes(&record, key.as_bytes()).await?;
        self.inner.put_content(key, sha256, bytes).await
    }

    async fn put_job_bytes(&self, key: &str, bytes: &[u8]) -> Result<(), OxidGeneError> {
        self.inner.put_job_bytes(key, bytes).await
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
}

/// Delete the objects job `job_id` recorded writing that no media row
/// names, then its records; how many objects were deleted.
///
/// Run once the job's transaction has ended, committed or rolled back,
/// while the job still holds its tree. Idempotent: a pass interrupted
/// halfway leaves records the next pass finds again, and a record whose
/// object is gone already costs nothing.
pub(crate) async fn discard_unreferenced(
    db: &impl ConnectionTrait,
    store: &dyn MediaStore,
    job_id: Uuid,
) -> Result<usize, OxidGeneError> {
    let records: Vec<String> = store
        .job_keys(job_id)
        .await?
        .into_iter()
        .filter(|key| {
            key.rsplit('/')
                .next()
                .is_some_and(|name| name.starts_with(CREATED))
        })
        .collect();
    if records.is_empty() {
        return Ok(0);
    }
    // Keys by tree: a key names its tree, and only that tree's rows can
    // name it.
    let mut by_tree: BTreeMap<Uuid, HashSet<String>> = BTreeMap::new();
    for record in &records {
        let key = String::from_utf8(store.get(record).await?)
            .map_err(|error| OxidGeneError::Internal(error.to_string()))?;
        if let Some(tree_id) = content_key_tree(&key) {
            by_tree.entry(tree_id).or_default().insert(key);
        }
    }
    let mut deleted = 0;
    for (tree_id, keys) in by_tree {
        let keys: Vec<String> = keys.into_iter().collect();
        let referenced = MediaRepo::referenced_keys(db, tree_id, &keys).await?;
        for key in keys.iter().filter(|key| !referenced.contains(*key)) {
            store.delete(key).await?;
            deleted += 1;
        }
    }
    for record in &records {
        store.delete(record).await?;
    }
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::FsStore;

    #[tokio::test]
    async fn every_written_object_is_recorded_before_it_is_written() {
        let root = tempfile::tempdir().expect("a media root");
        let inner = FsStore::new(root.path());
        let job_id = Uuid::now_v7();
        let tree_id = Uuid::now_v7();
        let store = RecordingStore::new(&inner, job_id);

        let first = store.put(tree_id, "jpg", b"first").await.unwrap();
        let second = store.put(tree_id, "jpg", b"second").await.unwrap();

        let mut recorded = Vec::new();
        for record in inner.job_keys(job_id).await.unwrap() {
            recorded.push(String::from_utf8(inner.get(&record).await.unwrap()).unwrap());
        }
        recorded.sort();
        let mut expected = vec![first.key, second.key];
        expected.sort();
        assert_eq!(recorded, expected);
    }
}
