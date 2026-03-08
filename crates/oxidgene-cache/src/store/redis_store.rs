//! Redis-backed `CacheStore` using MessagePack serialization.
//!
//! Key schema:
//!   - Person:       `oxidgene:person:{tree_id}:{person_id}`
//!   - Pedigree:     `oxidgene:pedigree:{tree_id}:{root_person_id}`
//!   - SearchIndex:  `oxidgene:search:{tree_id}`
//!   - Tree set:     `oxidgene:tree_keys:{tree_id}` (SET of all keys for bulk invalidation)

use async_trait::async_trait;
use oxidgene_core::error::OxidGeneError;
use redis::AsyncCommands;
use redis::aio::ConnectionManager;
use uuid::Uuid;

use crate::CacheStore;
use crate::types::*;

/// Prefix for all OxidGene cache keys in Redis.
const KEY_PREFIX: &str = "oxidgene";

/// Redis-backed cache store.
///
/// Uses a `ConnectionManager` which transparently reconnects on failure
/// and can be cloned cheaply (it is `Arc`-internally).
#[derive(Clone)]
pub struct RedisCacheStore {
    conn: ConnectionManager,
}

impl std::fmt::Debug for RedisCacheStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedisCacheStore")
            .field("conn", &"<redis::ConnectionManager>")
            .finish()
    }
}

impl RedisCacheStore {
    /// Create a new Redis cache store from a connection URL.
    ///
    /// The URL should be in the format `redis://[:<password>@]<host>:<port>[/<db>]`.
    pub async fn new(redis_url: &str) -> Result<Self, OxidGeneError> {
        let client = redis::Client::open(redis_url)
            .map_err(|e| OxidGeneError::Internal(format!("Redis connection error: {e}")))?;
        let conn = ConnectionManager::new(client)
            .await
            .map_err(|e| OxidGeneError::Internal(format!("Redis connection manager error: {e}")))?;
        Ok(Self { conn })
    }

    /// Create from an existing `ConnectionManager` (useful for testing).
    pub fn from_connection_manager(conn: ConnectionManager) -> Self {
        Self { conn }
    }

    // ── Key helpers ──────────────────────────────────────────────────

    fn person_key(tree_id: Uuid, person_id: Uuid) -> String {
        format!("{KEY_PREFIX}:person:{tree_id}:{person_id}")
    }

    fn pedigree_key(tree_id: Uuid, root_id: Uuid) -> String {
        format!("{KEY_PREFIX}:pedigree:{tree_id}:{root_id}")
    }

    fn search_key(tree_id: Uuid) -> String {
        format!("{KEY_PREFIX}:search:{tree_id}")
    }

    fn tree_set_key(tree_id: Uuid) -> String {
        format!("{KEY_PREFIX}:tree_keys:{tree_id}")
    }

    // ── Serialization helpers ────────────────────────────────────────

    fn serialize<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, OxidGeneError> {
        rmp_serde::to_vec(value)
            .map_err(|e| OxidGeneError::Internal(format!("MessagePack serialize error: {e}")))
    }

    fn deserialize<T: serde::de::DeserializeOwned>(data: &[u8]) -> Result<T, OxidGeneError> {
        rmp_serde::from_slice(data)
            .map_err(|e| OxidGeneError::Internal(format!("MessagePack deserialize error: {e}")))
    }

    // ── Internal helpers ─────────────────────────────────────────────

    /// Set a key and register it in the tree's key set for bulk invalidation.
    async fn set_with_tracking(
        &self,
        key: &str,
        value: &[u8],
        tree_id: Uuid,
    ) -> Result<(), OxidGeneError> {
        let set_key = Self::tree_set_key(tree_id);
        let mut conn = self.conn.clone();
        redis::pipe()
            .atomic()
            .set(key, value)
            .ignore()
            .sadd(&set_key, key)
            .ignore()
            .query_async::<()>(&mut conn)
            .await
            .map_err(|e| OxidGeneError::Internal(format!("Redis SET error: {e}")))?;
        Ok(())
    }

    /// Delete a key and remove it from the tree's key set.
    async fn delete_with_tracking(&self, key: &str, tree_id: Uuid) -> Result<(), OxidGeneError> {
        let set_key = Self::tree_set_key(tree_id);
        let mut conn = self.conn.clone();
        redis::pipe()
            .atomic()
            .del(key)
            .ignore()
            .srem(&set_key, key)
            .ignore()
            .query_async::<()>(&mut conn)
            .await
            .map_err(|e| OxidGeneError::Internal(format!("Redis DEL error: {e}")))?;
        Ok(())
    }
}

#[async_trait]
impl CacheStore for RedisCacheStore {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    // ── PersonCache ──────────────────────────────────────────────────

    async fn get_person(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
    ) -> Result<Option<CachedPerson>, OxidGeneError> {
        let key = Self::person_key(tree_id, person_id);
        let mut conn = self.conn.clone();
        let data: Option<Vec<u8>> = conn
            .get(&key)
            .await
            .map_err(|e| OxidGeneError::Internal(format!("Redis GET error: {e}")))?;
        match data {
            Some(bytes) => Ok(Some(Self::deserialize(&bytes)?)),
            None => Ok(None),
        }
    }

    async fn set_person(&self, entry: &CachedPerson) -> Result<(), OxidGeneError> {
        let key = Self::person_key(entry.tree_id, entry.person_id);
        let bytes = Self::serialize(entry)?;
        self.set_with_tracking(&key, &bytes, entry.tree_id).await
    }

    async fn set_persons_batch(&self, entries: &[CachedPerson]) -> Result<(), OxidGeneError> {
        if entries.is_empty() {
            return Ok(());
        }

        let mut conn = self.conn.clone();
        let mut pipe = redis::pipe();
        pipe.atomic();

        for entry in entries {
            let key = Self::person_key(entry.tree_id, entry.person_id);
            let bytes = Self::serialize(entry)?;
            let set_key = Self::tree_set_key(entry.tree_id);
            pipe.set(&key, bytes).ignore();
            pipe.sadd(&set_key, &key).ignore();
        }

        pipe.query_async::<()>(&mut conn)
            .await
            .map_err(|e| OxidGeneError::Internal(format!("Redis batch SET error: {e}")))?;
        Ok(())
    }

    async fn delete_person(&self, tree_id: Uuid, person_id: Uuid) -> Result<(), OxidGeneError> {
        let key = Self::person_key(tree_id, person_id);
        self.delete_with_tracking(&key, tree_id).await
    }

    async fn get_persons_batch(
        &self,
        tree_id: Uuid,
        person_ids: &[Uuid],
    ) -> Result<Vec<CachedPerson>, OxidGeneError> {
        if person_ids.is_empty() {
            return Ok(vec![]);
        }

        let keys: Vec<String> = person_ids
            .iter()
            .map(|pid| Self::person_key(tree_id, *pid))
            .collect();

        let mut conn = self.conn.clone();
        // MGET returns Vec<Option<Vec<u8>>>
        let results: Vec<Option<Vec<u8>>> = conn
            .mget(&keys)
            .await
            .map_err(|e| OxidGeneError::Internal(format!("Redis MGET error: {e}")))?;

        let mut persons = Vec::with_capacity(results.len());
        for bytes in results.into_iter().flatten() {
            persons.push(Self::deserialize(&bytes)?);
        }
        Ok(persons)
    }

    async fn get_all_persons(&self, tree_id: Uuid) -> Result<Vec<CachedPerson>, OxidGeneError> {
        let set_key = Self::tree_set_key(tree_id);
        let prefix = format!("{KEY_PREFIX}:person:{tree_id}:");

        let mut conn = self.conn.clone();
        let all_keys: Vec<String> = conn
            .smembers(&set_key)
            .await
            .map_err(|e| OxidGeneError::Internal(format!("Redis SMEMBERS error: {e}")))?;

        // Filter to only person keys for this tree
        let person_keys: Vec<&String> =
            all_keys.iter().filter(|k| k.starts_with(&prefix)).collect();

        if person_keys.is_empty() {
            return Ok(vec![]);
        }

        let results: Vec<Option<Vec<u8>>> = conn
            .mget(&person_keys)
            .await
            .map_err(|e| OxidGeneError::Internal(format!("Redis MGET error: {e}")))?;

        let mut persons = Vec::with_capacity(results.len());
        for bytes in results.into_iter().flatten() {
            persons.push(Self::deserialize(&bytes)?);
        }
        Ok(persons)
    }

    // ── PedigreeCache ────────────────────────────────────────────────

    async fn get_pedigree(
        &self,
        tree_id: Uuid,
        root_id: Uuid,
    ) -> Result<Option<CachedPedigree>, OxidGeneError> {
        let key = Self::pedigree_key(tree_id, root_id);
        let mut conn = self.conn.clone();
        let data: Option<Vec<u8>> = conn
            .get(&key)
            .await
            .map_err(|e| OxidGeneError::Internal(format!("Redis GET error: {e}")))?;
        match data {
            Some(bytes) => Ok(Some(Self::deserialize(&bytes)?)),
            None => Ok(None),
        }
    }

    async fn set_pedigree(&self, entry: &CachedPedigree) -> Result<(), OxidGeneError> {
        let key = Self::pedigree_key(entry.tree_id, entry.root_person_id);
        let bytes = Self::serialize(entry)?;
        self.set_with_tracking(&key, &bytes, entry.tree_id).await
    }

    async fn delete_pedigree(&self, tree_id: Uuid, root_id: Uuid) -> Result<(), OxidGeneError> {
        let key = Self::pedigree_key(tree_id, root_id);
        self.delete_with_tracking(&key, tree_id).await
    }

    async fn delete_all_pedigrees(&self, tree_id: Uuid) -> Result<(), OxidGeneError> {
        let set_key = Self::tree_set_key(tree_id);
        let prefix = format!("{KEY_PREFIX}:pedigree:{tree_id}:");

        let mut conn = self.conn.clone();
        let all_keys: Vec<String> = conn
            .smembers(&set_key)
            .await
            .map_err(|e| OxidGeneError::Internal(format!("Redis SMEMBERS error: {e}")))?;

        let pedigree_keys: Vec<&String> =
            all_keys.iter().filter(|k| k.starts_with(&prefix)).collect();

        if pedigree_keys.is_empty() {
            return Ok(());
        }

        let mut pipe = redis::pipe();
        pipe.atomic();
        for key in &pedigree_keys {
            pipe.del(*key).ignore();
            pipe.srem(&set_key, *key).ignore();
        }
        pipe.query_async::<()>(&mut conn)
            .await
            .map_err(|e| OxidGeneError::Internal(format!("Redis DEL pedigrees error: {e}")))?;
        Ok(())
    }

    // ── SearchIndex ──────────────────────────────────────────────────

    async fn get_search_index(
        &self,
        tree_id: Uuid,
    ) -> Result<Option<CachedSearchIndex>, OxidGeneError> {
        let key = Self::search_key(tree_id);
        let mut conn = self.conn.clone();
        let data: Option<Vec<u8>> = conn
            .get(&key)
            .await
            .map_err(|e| OxidGeneError::Internal(format!("Redis GET error: {e}")))?;
        match data {
            Some(bytes) => Ok(Some(Self::deserialize(&bytes)?)),
            None => Ok(None),
        }
    }

    async fn set_search_index(&self, entry: &CachedSearchIndex) -> Result<(), OxidGeneError> {
        let key = Self::search_key(entry.tree_id);
        let bytes = Self::serialize(entry)?;
        self.set_with_tracking(&key, &bytes, entry.tree_id).await
    }

    async fn delete_search_index(&self, tree_id: Uuid) -> Result<(), OxidGeneError> {
        let key = Self::search_key(tree_id);
        self.delete_with_tracking(&key, tree_id).await
    }

    // ── Bulk ─────────────────────────────────────────────────────────

    async fn invalidate_tree(&self, tree_id: Uuid) -> Result<(), OxidGeneError> {
        let set_key = Self::tree_set_key(tree_id);
        let mut conn = self.conn.clone();

        let all_keys: Vec<String> = conn
            .smembers(&set_key)
            .await
            .map_err(|e| OxidGeneError::Internal(format!("Redis SMEMBERS error: {e}")))?;

        if all_keys.is_empty() {
            return Ok(());
        }

        // Delete all data keys + the tracking set in one pipeline
        let mut pipe = redis::pipe();
        pipe.atomic();
        for key in &all_keys {
            pipe.del(key).ignore();
        }
        pipe.del(&set_key).ignore();

        pipe.query_async::<()>(&mut conn)
            .await
            .map_err(|e| OxidGeneError::Internal(format!("Redis invalidate_tree error: {e}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxidgene_core::enums::Sex;
    use uuid::Uuid;

    #[test]
    fn test_key_generation() {
        let tree_id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let person_id = Uuid::parse_str("6ba7b810-9dad-11d1-80b4-00c04fd430c8").unwrap();

        assert_eq!(
            RedisCacheStore::person_key(tree_id, person_id),
            "oxidgene:person:550e8400-e29b-41d4-a716-446655440000:6ba7b810-9dad-11d1-80b4-00c04fd430c8"
        );
        assert_eq!(
            RedisCacheStore::pedigree_key(tree_id, person_id),
            "oxidgene:pedigree:550e8400-e29b-41d4-a716-446655440000:6ba7b810-9dad-11d1-80b4-00c04fd430c8"
        );
        assert_eq!(
            RedisCacheStore::search_key(tree_id),
            "oxidgene:search:550e8400-e29b-41d4-a716-446655440000"
        );
        assert_eq!(
            RedisCacheStore::tree_set_key(tree_id),
            "oxidgene:tree_keys:550e8400-e29b-41d4-a716-446655440000"
        );
    }

    #[test]
    fn test_msgpack_roundtrip() {
        let entry = SearchEntry {
            person_id: Uuid::now_v7(),
            sex: Sex::Male,
            surname_normalized: "doe".to_string(),
            given_names_normalized: "john".to_string(),
            maiden_name_normalized: None,
            display_name: "John Doe".to_string(),
            birth_year: Some("1900".to_string()),
            birth_place: Some("Paris".to_string()),
            death_year: Some("1975".to_string()),
            date_sort: None,
        };

        let bytes = RedisCacheStore::serialize(&entry).unwrap();
        let decoded: SearchEntry = RedisCacheStore::deserialize(&bytes).unwrap();
        assert_eq!(decoded.person_id, entry.person_id);
        assert_eq!(decoded.display_name, entry.display_name);
        assert_eq!(decoded.birth_year, entry.birth_year);
    }
}
