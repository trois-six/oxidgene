//! Where sessions, sign-in transactions, refresh locks and seen logout
//! tokens live (docs/authentication.md §4.5): a map in the process, or Redis
//! when several backend replicas share them.
//!
//! The store never holds a usable secret. A session is keyed by the digest
//! of its cookie value, its CSRF token is held as a digest too, and the
//! tokens it carries are sealed by [`super::crypto::Sealer`] before they
//! reach it.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A store that cannot be reached: the request answers
/// `503 auth_unavailable`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreUnavailable;

/// A browser session, as stored.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Session {
    /// The digest of the cookie value: the store's key.
    pub id_hash: String,
    /// The opaque handle a user's session list shows and ends it by.
    pub handle: Uuid,
    /// The digest of the session's CSRF token.
    pub csrf_hash: String,
    pub user_id: Uuid,
    pub issuer: String,
    pub subject: String,
    /// The client the user signed in through.
    pub client_id: String,
    /// The provider's session id, for back-channel logout.
    pub sid: Option<String>,
    /// The sealed access token, its expiry (Unix seconds) and its scopes.
    pub access_token: String,
    pub access_expires_at: i64,
    pub scopes: Vec<String>,
    /// The sealed refresh token, when the provider issued one.
    pub refresh_token: Option<String>,
    /// The sealed ID token, the logout hint.
    pub id_token: Option<String>,
    /// The browser family read from the user agent at sign-in.
    pub browser: String,
    pub created_at: i64,
    pub last_used_at: i64,
    pub idle_expires_at: i64,
    pub absolute_expires_at: i64,
}

impl Session {
    /// When the session ends if nothing uses it, whichever limit comes first.
    pub(crate) fn expires_at(&self) -> i64 {
        self.idle_expires_at.min(self.absolute_expires_at)
    }
}

/// A sign-in under way between `/auth/login` and `/auth/callback`. Its
/// `Debug` leaves the PKCE verifier out.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LoginTransaction {
    /// The digest of the value of the `__Host-oxidgene-login` cookie.
    pub binding_hash: String,
    /// The digest of the `nonce` sent to the provider.
    pub nonce_hash: String,
    pub pkce_verifier: String,
    pub return_to: String,
}

impl std::fmt::Debug for LoginTransaction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoginTransaction")
            .field("return_to", &self.return_to)
            .finish_non_exhaustive()
    }
}

/// The share-link grants a browser holds (docs/authorization.md §6.3):
/// keyed by the digest of the `__Host-oxidgene-share` cookie, whether or
/// not anyone is signed in. Each link is checked against the database at
/// every request, so a revoked one grants nothing at once.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LinkGrants {
    /// The share links whose grant the browser holds.
    pub links: Vec<Uuid>,
    /// When the grants end, Unix seconds: at most the browser session's
    /// absolute lifetime, and at most the expiry of the links they hold.
    pub expires_at: i64,
}

/// The operations a session store answers. Keys are digests: base64url,
/// no separator a key scheme could confuse.
#[async_trait::async_trait]
pub trait SessionStore: Send + Sync + std::fmt::Debug {
    /// Write `session`, which lives until its idle or absolute expiry, and
    /// index it under its user and its provider session.
    async fn put(&self, session: &Session) -> Result<(), StoreUnavailable>;
    async fn get(&self, id_hash: &str) -> Result<Option<Session>, StoreUnavailable>;
    async fn delete(&self, id_hash: &str) -> Result<(), StoreUnavailable>;
    /// The live sessions of `user_id`.
    async fn user_sessions(&self, user_id: Uuid) -> Result<Vec<Session>, StoreUnavailable>;
    /// The keys of the live sessions of the provider session `sid`.
    async fn sid_sessions(&self, sid: &str) -> Result<Vec<String>, StoreUnavailable>;
    /// Keep `transaction` under `state_hash` for `ttl`, replacing any.
    async fn put_login(
        &self,
        state_hash: &str,
        transaction: &LoginTransaction,
        ttl: Duration,
    ) -> Result<(), StoreUnavailable>;
    /// Read and delete the transaction of `state_hash`: a second call finds
    /// nothing.
    async fn take_login(
        &self,
        state_hash: &str,
    ) -> Result<Option<LoginTransaction>, StoreUnavailable>;
    /// Take the lock `name` for `ttl`: the holder's token, which alone
    /// releases it; `None` when someone holds it.
    async fn lock(&self, name: &str, ttl: Duration) -> Result<Option<String>, StoreUnavailable>;
    /// Release the lock `name` if `holder` still holds it: a lock that
    /// lapsed and was taken by another is theirs.
    async fn unlock(&self, name: &str, holder: &str) -> Result<(), StoreUnavailable>;
    /// Whether `name` is seen for the first time within `ttl`: a logout
    /// token's `jti`.
    async fn first_seen(&self, name: &str, ttl: Duration) -> Result<bool, StoreUnavailable>;
    /// Keep `grants` under `id_hash` until their expiry, replacing any.
    async fn put_grants(&self, id_hash: &str, grants: &LinkGrants) -> Result<(), StoreUnavailable>;
    /// The unexpired grants of `id_hash`.
    async fn get_grants(&self, id_hash: &str) -> Result<Option<LinkGrants>, StoreUnavailable>;
    async fn delete_grants(&self, id_hash: &str) -> Result<(), StoreUnavailable>;
}

/// The time to live of `grants` at `now`.
fn grants_ttl(grants: &LinkGrants, now: i64) -> Duration {
    Duration::from_secs(u64::try_from((grants.expires_at - now).max(1)).unwrap_or(1))
}

/// Seconds since the Unix epoch.
pub(crate) fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// The time to live of a session written at `now`.
fn ttl_of(session: &Session, now: i64) -> Duration {
    Duration::from_secs(u64::try_from((session.expires_at() - now).max(1)).unwrap_or(1))
}

/// Sessions in the process: one backend process. A restart ends them all.
#[derive(Debug, Default)]
pub struct MemoryStore {
    inner: Mutex<Memory>,
}

#[derive(Debug, Default)]
struct Memory {
    sessions: HashMap<String, Session>,
    grants: HashMap<String, LinkGrants>,
    logins: HashMap<String, (LoginTransaction, Instant)>,
    /// Seen names, with when they lapse.
    marks: HashMap<String, Instant>,
    /// Locks, with their holder's token and when they lapse.
    locks: HashMap<String, (String, Instant)>,
    last_sweep: Option<Instant>,
}

impl Memory {
    /// Drop what has expired, at most once a minute.
    fn sweep(&mut self) {
        let now = Instant::now();
        if self
            .last_sweep
            .is_some_and(|last| now.duration_since(last) < Duration::from_secs(60))
        {
            return;
        }
        self.last_sweep = Some(now);
        let wall = self::now();
        self.sessions
            .retain(|_, session| session.expires_at() > wall);
        self.grants.retain(|_, grants| grants.expires_at > wall);
        self.logins.retain(|_, (_, until)| *until > now);
        self.marks.retain(|_, until| *until > now);
        self.locks.retain(|_, (_, until)| *until > now);
    }

    fn live(&self, id_hash: &str) -> Option<&Session> {
        self.sessions
            .get(id_hash)
            .filter(|session| session.expires_at() > now())
    }
}

impl MemoryStore {
    fn with<T>(&self, f: impl FnOnce(&mut Memory) -> T) -> T {
        let mut memory = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        memory.sweep();
        f(&mut memory)
    }
}

#[async_trait::async_trait]
impl SessionStore for MemoryStore {
    async fn put(&self, session: &Session) -> Result<(), StoreUnavailable> {
        self.with(|memory| {
            memory
                .sessions
                .insert(session.id_hash.clone(), session.clone());
        });
        Ok(())
    }

    async fn get(&self, id_hash: &str) -> Result<Option<Session>, StoreUnavailable> {
        Ok(self.with(|memory| memory.live(id_hash).cloned()))
    }

    async fn delete(&self, id_hash: &str) -> Result<(), StoreUnavailable> {
        self.with(|memory| memory.sessions.remove(id_hash));
        Ok(())
    }

    async fn user_sessions(&self, user_id: Uuid) -> Result<Vec<Session>, StoreUnavailable> {
        let wall = now();
        Ok(self.with(|memory| {
            memory
                .sessions
                .values()
                .filter(|session| session.user_id == user_id && session.expires_at() > wall)
                .cloned()
                .collect()
        }))
    }

    async fn sid_sessions(&self, sid: &str) -> Result<Vec<String>, StoreUnavailable> {
        Ok(self.with(|memory| {
            memory
                .sessions
                .values()
                .filter(|session| session.sid.as_deref() == Some(sid))
                .map(|session| session.id_hash.clone())
                .collect()
        }))
    }

    async fn put_login(
        &self,
        state_hash: &str,
        transaction: &LoginTransaction,
        ttl: Duration,
    ) -> Result<(), StoreUnavailable> {
        self.with(|memory| {
            memory.logins.insert(
                state_hash.to_string(),
                (transaction.clone(), Instant::now() + ttl),
            );
        });
        Ok(())
    }

    async fn take_login(
        &self,
        state_hash: &str,
    ) -> Result<Option<LoginTransaction>, StoreUnavailable> {
        Ok(self.with(|memory| {
            memory
                .logins
                .remove(state_hash)
                .filter(|(_, until)| *until > Instant::now())
                .map(|(transaction, _)| transaction)
        }))
    }

    async fn lock(&self, name: &str, ttl: Duration) -> Result<Option<String>, StoreUnavailable> {
        Ok(self.with(|memory| {
            let now = Instant::now();
            if memory
                .locks
                .get(name)
                .is_some_and(|(_, until)| *until > now)
            {
                return None;
            }
            let holder = super::crypto::random_token();
            memory
                .locks
                .insert(name.to_string(), (holder.clone(), now + ttl));
            Some(holder)
        }))
    }

    async fn unlock(&self, name: &str, holder: &str) -> Result<(), StoreUnavailable> {
        self.with(|memory| {
            if memory
                .locks
                .get(name)
                .is_some_and(|(held, _)| held == holder)
            {
                memory.locks.remove(name);
            }
        });
        Ok(())
    }

    async fn first_seen(&self, name: &str, ttl: Duration) -> Result<bool, StoreUnavailable> {
        Ok(self.with(|memory| mark(&mut memory.marks, &format!("seen:{name}"), ttl)))
    }

    async fn put_grants(&self, id_hash: &str, grants: &LinkGrants) -> Result<(), StoreUnavailable> {
        self.with(|memory| {
            memory.grants.insert(id_hash.to_string(), grants.clone());
        });
        Ok(())
    }

    async fn get_grants(&self, id_hash: &str) -> Result<Option<LinkGrants>, StoreUnavailable> {
        let wall = now();
        Ok(self.with(|memory| {
            memory
                .grants
                .get(id_hash)
                .filter(|grants| grants.expires_at > wall)
                .cloned()
        }))
    }

    async fn delete_grants(&self, id_hash: &str) -> Result<(), StoreUnavailable> {
        self.with(|memory| memory.grants.remove(id_hash));
        Ok(())
    }
}

/// Set `key` in `marks` for `ttl` unless it is set and current: `SET NX`.
fn mark(marks: &mut HashMap<String, Instant>, key: &str, ttl: Duration) -> bool {
    let now = Instant::now();
    if marks.get(key).is_some_and(|until| *until > now) {
        return false;
    }
    marks.insert(key.to_string(), now + ttl);
    true
}

/// Sessions in Redis, shared by every backend replica.
///
/// `oxidgene:session:{hash}` holds a session as JSON with its expiry as time
/// to live; `oxidgene:user-sessions:{user_id}` and `oxidgene:sid:{sid}` are
/// sets of session keys, so that a user's or a provider session's sessions
/// end together. A key in an index whose session has lapsed is dropped as it
/// is met.
#[derive(Clone)]
pub struct RedisStore {
    connection: redis::aio::ConnectionManager,
}

impl std::fmt::Debug for RedisStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RedisStore")
    }
}

const SESSION: &str = "oxidgene:session:";
const USER_SESSIONS: &str = "oxidgene:user-sessions:";
const SID: &str = "oxidgene:sid:";
const LOGIN: &str = "oxidgene:login:";
const LOCK: &str = "oxidgene:lock:";
const SEEN: &str = "oxidgene:seen:";
const GRANTS: &str = "oxidgene:share-grants:";

impl RedisStore {
    /// Connect to the Redis at `url`.
    ///
    /// # Errors
    ///
    /// When the URL is invalid or the server cannot be reached.
    pub async fn connect(url: &str) -> Result<Self, StoreUnavailable> {
        let client = redis::Client::open(url).map_err(|_| StoreUnavailable)?;
        let config = redis::aio::ConnectionManagerConfig::new()
            .set_connection_timeout(Some(Duration::from_secs(5)))
            .set_response_timeout(Some(Duration::from_secs(5)));
        let connection = redis::aio::ConnectionManager::new_with_config(client, config)
            .await
            .map_err(|_| StoreUnavailable)?;
        Ok(Self { connection })
    }

    async fn query<T: redis::FromRedisValue>(
        &self,
        command: &redis::Cmd,
    ) -> Result<T, StoreUnavailable> {
        let mut connection = self.connection.clone();
        command
            .query_async(&mut connection)
            .await
            .map_err(|_| StoreUnavailable)
    }

    async fn run(&self, pipeline: &redis::Pipeline) -> Result<(), StoreUnavailable> {
        let mut connection = self.connection.clone();
        pipeline
            .query_async::<()>(&mut connection)
            .await
            .map_err(|_| StoreUnavailable)
    }

    async fn sessions_of(&self, index: &str) -> Result<Vec<Session>, StoreUnavailable> {
        let keys: Vec<String> = self.query(redis::cmd("SMEMBERS").arg(index)).await?;
        let mut sessions = Vec::new();
        for key in keys {
            match self.get(&key).await? {
                Some(session) => sessions.push(session),
                None => {
                    self.query::<()>(redis::cmd("SREM").arg(index).arg(&key))
                        .await?;
                }
            }
        }
        Ok(sessions)
    }
}

/// Make the index `key` live at least `ttl` seconds: an index lives as long
/// as the longest session it holds, never shortened by a shorter one's write
/// (`EXPIRE … GT`, and `NX` for an index without expiry yet; Redis 7).
fn extend(pipeline: &mut redis::Pipeline, key: &str, ttl: u64) {
    pipeline.cmd("EXPIRE").arg(key).arg(ttl).arg("GT").ignore();
    pipeline.cmd("EXPIRE").arg(key).arg(ttl).arg("NX").ignore();
}

/// `SET key value NX PX ttl`.
fn set_nx(key: String, value: &str, ttl: Duration) -> redis::Cmd {
    let mut command = redis::cmd("SET");
    command
        .arg(key)
        .arg(value)
        .arg("NX")
        .arg("PX")
        .arg(u64::try_from(ttl.as_millis()).unwrap_or(u64::MAX));
    command
}

#[async_trait::async_trait]
impl SessionStore for RedisStore {
    async fn put(&self, session: &Session) -> Result<(), StoreUnavailable> {
        let now = now();
        let ttl = ttl_of(session, now).as_secs();
        let index_ttl = u64::try_from((session.absolute_expires_at - now).max(1)).unwrap_or(1);
        let json = serde_json::to_string(session).map_err(|_| StoreUnavailable)?;
        let mut pipeline = redis::pipe();
        pipeline.atomic();
        pipeline
            .cmd("SET")
            .arg(format!("{SESSION}{}", session.id_hash))
            .arg(json)
            .arg("EX")
            .arg(ttl)
            .ignore();
        let user_index = format!("{USER_SESSIONS}{}", session.user_id);
        pipeline
            .cmd("SADD")
            .arg(&user_index)
            .arg(&session.id_hash)
            .ignore();
        extend(&mut pipeline, &user_index, index_ttl);
        if let Some(sid) = &session.sid {
            let sid_index = format!("{SID}{}", super::crypto::digest(sid));
            pipeline
                .cmd("SADD")
                .arg(&sid_index)
                .arg(&session.id_hash)
                .ignore();
            extend(&mut pipeline, &sid_index, index_ttl);
        }
        self.run(&pipeline).await
    }

    async fn get(&self, id_hash: &str) -> Result<Option<Session>, StoreUnavailable> {
        let json: Option<String> = self
            .query(redis::cmd("GET").arg(format!("{SESSION}{id_hash}")))
            .await?;
        Ok(json.and_then(|json| serde_json::from_str(&json).ok()))
    }

    async fn delete(&self, id_hash: &str) -> Result<(), StoreUnavailable> {
        let Some(session) = self.get(id_hash).await? else {
            return Ok(());
        };
        let mut pipeline = redis::pipe();
        pipeline.atomic();
        pipeline
            .cmd("DEL")
            .arg(format!("{SESSION}{id_hash}"))
            .ignore();
        pipeline
            .cmd("SREM")
            .arg(format!("{USER_SESSIONS}{}", session.user_id))
            .arg(id_hash)
            .ignore();
        if let Some(sid) = &session.sid {
            pipeline
                .cmd("SREM")
                .arg(format!("{SID}{}", super::crypto::digest(sid)))
                .arg(id_hash)
                .ignore();
        }
        self.run(&pipeline).await
    }

    async fn user_sessions(&self, user_id: Uuid) -> Result<Vec<Session>, StoreUnavailable> {
        self.sessions_of(&format!("{USER_SESSIONS}{user_id}")).await
    }

    async fn sid_sessions(&self, sid: &str) -> Result<Vec<String>, StoreUnavailable> {
        Ok(self
            .sessions_of(&format!("{SID}{}", super::crypto::digest(sid)))
            .await?
            .into_iter()
            .map(|session| session.id_hash)
            .collect())
    }

    async fn put_login(
        &self,
        state_hash: &str,
        transaction: &LoginTransaction,
        ttl: Duration,
    ) -> Result<(), StoreUnavailable> {
        let json = serde_json::to_string(transaction).map_err(|_| StoreUnavailable)?;
        self.query::<()>(
            redis::cmd("SET")
                .arg(format!("{LOGIN}{state_hash}"))
                .arg(json)
                .arg("EX")
                .arg(ttl.as_secs().max(1)),
        )
        .await
    }

    async fn take_login(
        &self,
        state_hash: &str,
    ) -> Result<Option<LoginTransaction>, StoreUnavailable> {
        let json: Option<String> = self
            .query(redis::cmd("GETDEL").arg(format!("{LOGIN}{state_hash}")))
            .await?;
        Ok(json.and_then(|json| serde_json::from_str(&json).ok()))
    }

    async fn lock(&self, name: &str, ttl: Duration) -> Result<Option<String>, StoreUnavailable> {
        let holder = super::crypto::random_token();
        let set: Option<String> = self
            .query(&set_nx(format!("{LOCK}{name}"), &holder, ttl))
            .await?;
        Ok(set.map(|_| holder))
    }

    async fn unlock(&self, name: &str, holder: &str) -> Result<(), StoreUnavailable> {
        // Compare and delete in one step: the lock may have lapsed and been
        // taken by another since.
        const RELEASE: &str = "if redis.call('GET', KEYS[1]) == ARGV[1] then \
             return redis.call('DEL', KEYS[1]) else return 0 end";
        self.query::<i64>(
            redis::cmd("EVAL")
                .arg(RELEASE)
                .arg(1)
                .arg(format!("{LOCK}{name}"))
                .arg(holder),
        )
        .await
        .map(|_| ())
    }

    async fn first_seen(&self, name: &str, ttl: Duration) -> Result<bool, StoreUnavailable> {
        let set: Option<String> = self
            .query(&set_nx(format!("{SEEN}{name}"), "1", ttl))
            .await?;
        Ok(set.is_some())
    }

    async fn put_grants(&self, id_hash: &str, grants: &LinkGrants) -> Result<(), StoreUnavailable> {
        let json = serde_json::to_string(grants).map_err(|_| StoreUnavailable)?;
        self.query::<()>(
            redis::cmd("SET")
                .arg(format!("{GRANTS}{id_hash}"))
                .arg(json)
                .arg("EX")
                .arg(grants_ttl(grants, now()).as_secs()),
        )
        .await
    }

    async fn get_grants(&self, id_hash: &str) -> Result<Option<LinkGrants>, StoreUnavailable> {
        let json: Option<String> = self
            .query(redis::cmd("GET").arg(format!("{GRANTS}{id_hash}")))
            .await?;
        let wall = now();
        Ok(json
            .and_then(|json| serde_json::from_str::<LinkGrants>(&json).ok())
            .filter(|grants| grants.expires_at > wall))
    }

    async fn delete_grants(&self, id_hash: &str) -> Result<(), StoreUnavailable> {
        self.query::<()>(redis::cmd("DEL").arg(format!("{GRANTS}{id_hash}")))
            .await
    }
}

/// The sessions of `store` that `keys` name, deleted; how many there were.
pub(crate) async fn delete_all(
    store: &dyn SessionStore,
    keys: impl IntoIterator<Item = String>,
) -> Result<usize, StoreUnavailable> {
    let mut count = 0;
    let mut seen = HashSet::new();
    for key in keys {
        if seen.insert(key.clone()) {
            store.delete(&key).await?;
            count += 1;
        }
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(user_id: Uuid, sid: Option<&str>, lifetime: i64) -> Session {
        let now = now();
        Session {
            id_hash: super::super::crypto::random_token(),
            handle: Uuid::now_v7(),
            csrf_hash: "csrf".into(),
            user_id,
            issuer: "https://id.example.invalid".into(),
            subject: "subject-1".into(),
            client_id: "oxidgene-web".into(),
            sid: sid.map(str::to_string),
            access_token: "sealed".into(),
            access_expires_at: now + 300,
            scopes: vec!["openid".into()],
            refresh_token: None,
            id_token: None,
            browser: "firefox".into(),
            created_at: now,
            last_used_at: now,
            idle_expires_at: now + lifetime,
            absolute_expires_at: now + lifetime * 2,
        }
    }

    /// The behaviour every store shares; run on the memory store here and
    /// on Redis by the opt-in `on_redis_` test.
    async fn exercise(store: &dyn SessionStore) {
        exercise_sessions(store).await;
        exercise_logins(store).await;
        exercise_marks(store).await;
    }

    async fn exercise_sessions(store: &dyn SessionStore) {
        let user = Uuid::now_v7();
        let sid = format!("provider-session-{user}");
        let a = session(user, Some(&sid), 600);
        let b = session(user, None, 600);
        store.put(&a).await.unwrap();
        store.put(&b).await.unwrap();
        assert_eq!(store.get(&a.id_hash).await.unwrap(), Some(a.clone()));
        assert_eq!(store.user_sessions(user).await.unwrap().len(), 2);
        assert_eq!(
            store.sid_sessions(&sid).await.unwrap(),
            vec![a.id_hash.clone()]
        );
        exercise_deletion(store, &a, &b, &sid).await;
    }

    async fn exercise_deletion(store: &dyn SessionStore, a: &Session, b: &Session, sid: &str) {
        store.delete(&a.id_hash).await.unwrap();
        assert_eq!(store.get(&a.id_hash).await.unwrap(), None);
        assert!(store.sid_sessions(sid).await.unwrap().is_empty());
        assert_eq!(
            store.user_sessions(b.user_id).await.unwrap(),
            vec![b.clone()]
        );
    }

    async fn exercise_logins(store: &dyn SessionStore) {
        let transaction = LoginTransaction {
            binding_hash: "binding".into(),
            nonce_hash: "nonce".into(),
            pkce_verifier: "verifier".into(),
            return_to: "/trees".into(),
        };
        let state = super::super::crypto::random_token();
        store
            .put_login(&state, &transaction, Duration::from_secs(600))
            .await
            .unwrap();
        assert_eq!(store.take_login(&state).await.unwrap(), Some(transaction));
        assert_eq!(store.take_login(&state).await.unwrap(), None, "a replay");
    }

    async fn exercise_marks(store: &dyn SessionStore) {
        exercise_locks(store).await;
        let jti = super::super::crypto::random_token();
        assert!(
            store
                .first_seen(&jti, Duration::from_secs(60))
                .await
                .unwrap()
        );
        assert!(
            !store
                .first_seen(&jti, Duration::from_secs(60))
                .await
                .unwrap()
        );
    }

    /// A lock is taken once, released by its holder only, and a lapsed one
    /// taken by another is theirs.
    async fn exercise_locks(store: &dyn SessionStore) {
        let lock = super::super::crypto::random_token();
        let holder = store.lock(&lock, Duration::from_secs(10)).await.unwrap();
        let holder = holder.expect("takes the lock");
        assert!(
            store
                .lock(&lock, Duration::from_secs(10))
                .await
                .unwrap()
                .is_none()
        );
        // Only its holder releases it.
        store.unlock(&lock, "another holder").await.unwrap();
        assert!(
            store
                .lock(&lock, Duration::from_secs(10))
                .await
                .unwrap()
                .is_none()
        );
        store.unlock(&lock, &holder).await.unwrap();
        let next = store.lock(&lock, Duration::from_millis(50)).await.unwrap();
        let next = next.expect("takes the released lock");
        // A lapsed lock taken by another is theirs: the late holder's
        // release leaves it.
        tokio::time::sleep(Duration::from_millis(100)).await;
        let third = store.lock(&lock, Duration::from_secs(10)).await.unwrap();
        assert!(third.is_some(), "a lapsed lock is free");
        store.unlock(&lock, &next).await.unwrap();
        assert!(
            store
                .lock(&lock, Duration::from_secs(10))
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn the_memory_store_keeps_sessions_and_their_indexes() {
        exercise(&MemoryStore::default()).await;
    }

    #[test]
    fn a_login_transaction_never_prints_its_verifier() {
        let login = LoginTransaction {
            binding_hash: "binding-digest".into(),
            nonce_hash: "nonce-digest".into(),
            pkce_verifier: "verifier-value".into(),
            return_to: "/trees".into(),
        };
        let printed = format!("{login:?}");
        assert!(!printed.contains("verifier-value"), "{printed}");
        assert!(printed.contains("/trees"), "{printed}");
    }

    #[tokio::test]
    async fn an_expired_session_is_gone() {
        let store = MemoryStore::default();
        let lapsed = session(Uuid::now_v7(), None, -1);
        store.put(&lapsed).await.unwrap();
        assert_eq!(store.get(&lapsed.id_hash).await.unwrap(), None);
        assert!(
            store
                .user_sessions(lapsed.user_id)
                .await
                .unwrap()
                .is_empty()
        );
    }

    /// The same behaviour on the Redis `OXIDGENE_TEST_REDIS_URL` names.
    #[tokio::test]
    #[ignore = "needs a Redis server: run by `just test-redis`"]
    async fn on_redis_the_store_keeps_sessions_and_their_indexes() {
        let url = std::env::var("OXIDGENE_TEST_REDIS_URL").expect("OXIDGENE_TEST_REDIS_URL");
        let store = RedisStore::connect(&url).await.expect("Redis");
        exercise(&store).await;
    }
}
