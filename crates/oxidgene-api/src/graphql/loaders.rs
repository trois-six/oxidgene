//! Request-scoped batching of the records a GraphQL response nests.
//!
//! A page of a hundred persons asking for each one's names, events and notes
//! would run a query per person and per list. Each nested field queues its
//! parent's key with the request's [`Batcher`] instead, and waits. The
//! operation runs inside [`Rounds`], which watches it: once every field it
//! can resolve has been polled and all of them wait — the operation is
//! pending without having yielded to the runtime on purpose — the keys
//! queued so far are read together, one `… IN (…)` query per relation
//! through the repository layer, and each field gets its parent's records
//! whole, in the order and with the filters the field documents. The
//! answers of a round are handed out together, so the next level of the
//! response queues its keys in one pass too.
//!
//! A response therefore costs a number of statements set by its shape, never
//! by the number of records it holds, and the same number on every run:
//! rounds follow the operation's progress, not a timer. Nothing is cached,
//! and nothing outlives the request.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::future::Future;
use std::hash::Hash;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::task::{Context as TaskContext, Poll, Wake, Waker};

use async_graphql::{Context, Error, Request, Response, Result};
use futures_util::future::{BoxFuture, join_all};
use oxidgene_core::OxidGeneError;
use oxidgene_core::collections::sorted_unique;
use oxidgene_core::types::{
    Citation, Event, EventWitness, Family, FamilyChild, FamilySpouse, Media, MediaLink, Note,
    Person, PersonName, Place, Repository, Source, SourceRepository,
};
use oxidgene_db::repo::{
    CitationRepo, EventRepo, EventWitnessRepo, FamilyChildRepo, FamilyRepo, FamilySpouseRepo,
    MediaLinkRepo, MediaLinkTarget, NoteRepo, PersonNameRepo, PersonRepo, PlaceRepo,
    RepositoryRepo, SourceRepo, SourceRepositoryRepo,
};
use sea_orm::DatabaseConnection;
use tokio::sync::oneshot;
use tracing::{Instrument as _, Span};
use uuid::Uuid;

use super::types::reader_from_ctx;

// ── The batcher ──────────────────────────────────────────────────────

/// Execute `request` against `schema` with a [`Batcher`] of its own, in
/// [`Rounds`].
///
/// Done around the execution rather than as a schema extension: an
/// extension takes part in the resolution of every field, and adds its
/// frames to the stack at every level of a deep document.
pub(super) async fn execute(schema: &super::OxidGeneSchema, request: Request) -> Response {
    let batcher = Arc::new(Batcher::default());
    Rounds {
        operation: Box::pin(schema.execute(request.data(Arc::clone(&batcher)))),
        batcher: &batcher,
        round: None,
    }
    .await
}

/// A relation the batcher reads for several parents at once: the key of
/// one parent (or of one record), and how to read the values of many keys.
pub(crate) trait Relation: Clone + Eq + Hash + Send + Sync + 'static {
    /// What one key answers: a parent's records, or one record.
    type Value: Clone + Send + Sync + 'static;

    /// The values of `keys`, in as few statements as the relation needs; a
    /// key with none is absent.
    fn load(
        db: &DatabaseConnection,
        keys: &[Self],
    ) -> impl Future<Output = Result<HashMap<Self, Self::Value>, OxidGeneError>> + Send;
}

/// What a field waiting on a key is eventually given.
type Answer<V> = Result<Option<V>>;

/// The keys of one relation queued since the last round, with the fields
/// waiting on each.
struct Waiting<K: Relation> {
    replies: HashMap<K, Vec<oneshot::Sender<Answer<K::Value>>>>,
    /// Where the first field to queue a key was traced: the read is
    /// recorded there, under the root field that needs it.
    span: Span,
}

/// [`Waiting`], whatever its relation.
trait Queued: Send {
    fn as_any(&mut self) -> &mut dyn Any;

    /// Read the values of the queued keys and answer every waiting field.
    fn read(self: Box<Self>, db: DatabaseConnection) -> BoxFuture<'static, ()>;
}

impl<K: Relation> Queued for Waiting<K> {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    fn read(self: Box<Self>, db: DatabaseConnection) -> BoxFuture<'static, ()> {
        let Waiting { replies, span } = *self;
        Box::pin(
            async move {
                let keys: Vec<K> = replies.keys().cloned().collect();
                let found = K::load(&db, &keys).await.map_err(Error::from);
                for (key, waiting) in replies {
                    let answer = match &found {
                        Ok(found) => Ok(found.get(&key).cloned()),
                        Err(error) => Err(error.clone()),
                    };
                    for reply in waiting {
                        // A field that stopped waiting needs no answer.
                        let _ = reply.send(answer.clone());
                    }
                }
            }
            .instrument(span),
        )
    }
}

/// The keys one request's fields have queued and not yet had read.
#[derive(Default)]
pub(crate) struct Batcher {
    /// The read pool, as the first field to queue a key gives it.
    db: OnceLock<DatabaseConnection>,
    queued: Mutex<HashMap<TypeId, Box<dyn Queued>>>,
}

impl Batcher {
    /// Queue `key`, to be read through `db`; its value arrives once the next
    /// round has read it.
    fn queue<K: Relation>(
        &self,
        db: &DatabaseConnection,
        key: K,
    ) -> oneshot::Receiver<Answer<K::Value>> {
        self.db.get_or_init(|| db.clone());
        let (reply, answer) = oneshot::channel();
        let mut queued = self.queued.lock().unwrap_or_else(PoisonError::into_inner);
        let waiting = queued.entry(TypeId::of::<K>()).or_insert_with(|| {
            Box::new(Waiting::<K> {
                replies: HashMap::new(),
                span: Span::current(),
            })
        });
        waiting
            .as_any()
            .downcast_mut::<Waiting<K>>()
            .expect("the queue of a relation is filed under its key type")
            .replies
            .entry(key)
            .or_default()
            .push(reply);
        answer
    }

    /// The reads of everything queued, as one round.
    fn round(&self) -> Option<BoxFuture<'static, ()>> {
        let queued =
            std::mem::take(&mut *self.queued.lock().unwrap_or_else(PoisonError::into_inner));
        let db = self.db.get().filter(|_| !queued.is_empty())?;
        let reads: Vec<_> = queued
            .into_values()
            .map(|waiting| waiting.read(db.clone()))
            .collect();
        Some(Box::pin(async move {
            join_all(reads).await;
        }))
    }
}

/// An operation and the rounds of reads its fields queue.
struct Rounds<'a> {
    operation: Pin<Box<dyn Future<Output = Response> + Send + 'a>>,
    batcher: &'a Batcher,
    /// The round being read: its answers are handed out when all of its
    /// reads are done, never one relation at a time.
    round: Option<BoxFuture<'static, ()>>,
}

impl Future for Rounds<'_> {
    type Output = Response;

    fn poll(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Response> {
        let this = &mut *self;
        if let Some(round) = this.round.as_mut()
            && round.as_mut().poll(cx).is_ready()
        {
            this.round = None;
        }

        let yielded = Arc::new(Yielded {
            woken: AtomicBool::new(false),
            task: cx.waker().clone(),
        });
        let waker = Waker::from(Arc::clone(&yielded));
        if let Poll::Ready(response) = this
            .operation
            .as_mut()
            .poll(&mut TaskContext::from_waker(&waker))
        {
            return Poll::Ready(response);
        }
        // Woken while polled, the operation still has fields to run, and
        // perhaps keys to queue; a round in flight finishes first.
        if yielded.woken.load(Ordering::Acquire) || this.round.is_some() {
            return Poll::Pending;
        }
        if let Some(mut round) = this.batcher.round() {
            if round.as_mut().poll(cx).is_ready() {
                // Answered at once: the waiting fields go on at the next poll.
                cx.waker().wake_by_ref();
            } else {
                this.round = Some(round);
            }
        }
        Poll::Pending
    }
}

/// The waker the operation is polled with: it wakes the task, and records
/// that it did.
struct Yielded {
    woken: AtomicBool,
    task: Waker,
}

impl Wake for Yielded {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.woken.store(true, Ordering::Release);
        self.task.wake_by_ref();
    }
}

// ── What the resolvers call ──────────────────────────────────────────

/// The value of `key`, read in the next round.
async fn answer<K: Relation>(ctx: &Context<'_>, key: K) -> Answer<K::Value> {
    let answer = batcher(ctx)?.queue(reader_from_ctx(ctx), key);
    answer.await.map_err(abandoned)?
}

/// The request's batcher.
fn batcher<'a>(ctx: &'a Context<'_>) -> Result<&'a Batcher> {
    Ok(ctx.data::<Arc<Batcher>>()?)
}

/// A round dropped before it answered: the operation ended under the field.
fn abandoned(_: oneshot::error::RecvError) -> Error {
    Error::from(OxidGeneError::Internal(
        "a batched read ended without an answer".to_string(),
    ))
}

/// The records nested under `key`, none when it has none.
pub(crate) async fn list<K, T>(ctx: &Context<'_>, key: K) -> Result<Vec<T>>
where
    K: Relation<Value = Vec<T>>,
{
    Ok(answer(ctx, key).await?.unwrap_or_default())
}

/// The record `key` names, if it is there.
pub(crate) async fn one<K: Relation>(ctx: &Context<'_>, key: K) -> Answer<K::Value> {
    answer(ctx, key).await
}

/// The records `keys` name, by key, less those that are not there.
pub(crate) async fn many<K: Relation>(
    ctx: &Context<'_>,
    keys: impl IntoIterator<Item = K>,
) -> Result<HashMap<K, K::Value>> {
    let (batcher, db) = (batcher(ctx)?, reader_from_ctx(ctx));
    let answers: Vec<_> = keys
        .into_iter()
        .map(|key| (key.clone(), batcher.queue(db, key)))
        .collect();
    let mut found = HashMap::new();
    for (key, answer) in answers {
        if let Some(value) = answer.await.map_err(abandoned)?? {
            found.insert(key, value);
        }
    }
    Ok(found)
}

// ── The relations ────────────────────────────────────────────────────

/// The ids `keys` carry.
fn ids<K>(keys: &[K], id: impl Fn(&K) -> Uuid) -> Vec<Uuid> {
    keys.iter().map(id).collect()
}

/// `rows` under the key of the parent each belongs to, in the order they
/// came.
fn grouped<K: Hash + Eq, T>(
    rows: impl IntoIterator<Item = T>,
    parent: impl Fn(&T) -> Option<K>,
) -> HashMap<K, Vec<T>> {
    let mut groups: HashMap<K, Vec<T>> = HashMap::new();
    for row in rows {
        if let Some(key) = parent(&row) {
            groups.entry(key).or_default().push(row);
        }
    }
    groups
}

/// A parent's list of records, by the parent's id: `$read` reads the rows of
/// many parents, `$parent` gives the parent of a row.
macro_rules! children {
    ($(#[$doc:meta])* $key:ident => $row:ty, $read:path, $parent:expr) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub(crate) struct $key(pub(crate) Uuid);

        impl Relation for $key {
            type Value = Vec<$row>;

            async fn load(
                db: &DatabaseConnection,
                keys: &[Self],
            ) -> Result<HashMap<Self, Vec<$row>>, OxidGeneError> {
                let rows = $read(db, &ids(keys, |key| key.0)).await?;
                Ok(grouped(rows, |row: &$row| ($parent)(row).map($key)))
            }
        }
    };
}

/// [`children!`] for a read scoped to the parent's tree, keyed by the tree
/// and the parent's id: one query per tree the round reaches, which is one
/// tree unless an operation reads several.
macro_rules! tree_children {
    ($(#[$doc:meta])* $key:ident => $row:ty, $read:path, $parent:expr) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub(crate) struct $key(pub(crate) Uuid, pub(crate) Uuid);

        impl Relation for $key {
            type Value = Vec<$row>;

            async fn load(
                db: &DatabaseConnection,
                keys: &[Self],
            ) -> Result<HashMap<Self, Vec<$row>>, OxidGeneError> {
                let mut found = HashMap::new();
                for (tree, parents) in grouped(keys.iter().copied(), |key| Some(key.0)) {
                    let rows = $read(db, tree, &ids(&parents, |key| key.1)).await?;
                    found.extend(grouped(rows, |row: &$row| {
                        ($parent)(row).map(|id| $key(tree, id))
                    }));
                }
                Ok(found)
            }
        }
    };
}

/// Records by id: `$read` reads the live ones among many ids.
macro_rules! records {
    ($(#[$doc:meta])* $key:ident => $row:ty, $read:path) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub(crate) struct $key(pub(crate) Uuid);

        impl Relation for $key {
            type Value = $row;

            async fn load(
                db: &DatabaseConnection,
                keys: &[Self],
            ) -> Result<HashMap<Self, $row>, OxidGeneError> {
                let rows = $read(db, &ids(keys, |key| key.0)).await?;
                Ok(rows.into_iter().map(|row| ($key(row.id), row)).collect())
            }
        }
    };
}

children!(
    /// A person's names: primary first, then in the author's order.
    PersonNames => PersonName, PersonNameRepo::list_by_persons, |n: &PersonName| Some(n.person_id)
);
children!(
    /// A person's live events, in no particular order.
    PersonEvents => Event, EventRepo::list_by_persons, |e: &Event| e.person_id
);
children!(
    /// A family's live events, with the ages their records give the spouses.
    FamilyEvents => Event, EventRepo::list_by_families, |e: &Event| e.family_id
);
children!(
    /// A family's spouse links.
    FamilySpouses => FamilySpouse, FamilySpouseRepo::list_by_families, |s: &FamilySpouse| Some(s.family_id)
);
children!(
    /// A family's child links.
    FamilyChildren => FamilyChild, FamilyChildRepo::list_by_families, |c: &FamilyChild| Some(c.family_id)
);
children!(
    /// An event's witnesses, by their order.
    EventWitnesses => EventWitness, EventWitnessRepo::list_by_events, |w: &EventWitness| Some(w.event_id)
);
children!(
    /// The citations drawn from a source.
    SourceCitations => Citation, CitationRepo::list_by_sources, |c: &Citation| Some(c.source_id)
);
children!(
    /// A source's links to live repositories, in order.
    SourceRepositories => SourceRepository, SourceRepositoryRepo::list_by_sources, |l: &SourceRepository| Some(l.source_id)
);
children!(
    /// A repository's links from live sources, in order.
    RepositorySources => SourceRepository, SourceRepositoryRepo::list_by_repositories, |l: &SourceRepository| Some(l.repository_id)
);

tree_children!(
    /// The citations attached directly to person `.1` of tree `.0`, from
    /// live sources.
    PersonCitations => Citation, CitationRepo::list_for_persons, |c: &Citation| c.person_id
);
tree_children!(
    /// The citations of event `.1` of tree `.0`, from live sources.
    EventCitations => Citation, CitationRepo::list_for_events, |c: &Citation| c.event_id
);
tree_children!(
    /// The live notes on person `.1` of tree `.0`.
    PersonNotes => Note, NoteRepo::list_by_persons, |n: &Note| n.person_id
);
tree_children!(
    /// The live notes on event `.1` of tree `.0`.
    EventNotes => Note, NoteRepo::list_by_events, |n: &Note| n.event_id
);

records!(
    /// A live person.
    PersonById => Person, PersonRepo::get_many
);
records!(
    /// A live family.
    FamilyById => Family, FamilyRepo::get_many
);
records!(
    /// A place.
    PlaceById => Place, PlaceRepo::get_many
);
records!(
    /// A live source, reached from a record of its tree.
    SourceById => Source, SourceRepo::get_many_by_id
);
records!(
    /// A live repository.
    RepositoryById => Repository, RepositoryRepo::get_many
);

/// The live families a person is a spouse in, by id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct PersonFamilies(pub(crate) Uuid);

impl Relation for PersonFamilies {
    type Value = Vec<Family>;

    async fn load(
        db: &DatabaseConnection,
        keys: &[Self],
    ) -> Result<HashMap<Self, Vec<Family>>, OxidGeneError> {
        let links = FamilySpouseRepo::list_by_persons(db, &ids(keys, |key| key.0)).await?;
        let family_ids = sorted_unique(links.iter().map(|link| link.family_id));
        let families: HashMap<Uuid, Family> = FamilyRepo::get_many(db, &family_ids)
            .await?
            .into_iter()
            .map(|family| (family.id, family))
            .collect();
        Ok(grouped(links, |link: &FamilySpouse| {
            Some(PersonFamilies(link.person_id))
        })
        .into_iter()
        .map(|(person, links)| {
            let mut theirs: Vec<Family> = links
                .iter()
                .filter_map(|link| families.get(&link.family_id).cloned())
                .collect();
            theirs.sort_by_key(|family| family.id);
            theirs.dedup_by_key(|family| family.id);
            (person, theirs)
        })
        .collect())
    }
}

/// The live media linked to entity `.1` of kind `.0`, in gallery order,
/// each once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct LinkedMedia(pub(crate) MediaLinkTarget, pub(crate) Uuid);

/// The entity of kind `target` that `link` attaches its media to.
fn linked_entity(target: MediaLinkTarget, link: &MediaLink) -> Option<Uuid> {
    match target {
        MediaLinkTarget::Person => link.person_id,
        MediaLinkTarget::Family => link.family_id,
        MediaLinkTarget::Event => link.event_id,
        MediaLinkTarget::Source => link.source_id,
    }
}

impl Relation for LinkedMedia {
    type Value = Vec<Media>;

    async fn load(
        db: &DatabaseConnection,
        keys: &[Self],
    ) -> Result<HashMap<Self, Vec<Media>>, OxidGeneError> {
        let mut found: HashMap<Self, Vec<Media>> = HashMap::new();
        for (target, entities) in grouped(keys.iter().copied(), |key| Some(key.0)) {
            let ids = ids(&entities, |key| key.1);
            for (link, media) in MediaLinkRepo::list_with_media_for_many(db, target, &ids).await? {
                let Some(entity) = linked_entity(target, &link) else {
                    continue;
                };
                let gallery = found.entry(LinkedMedia(target, entity)).or_default();
                if gallery.iter().all(|seen| seen.id != media.id) {
                    gallery.push(media);
                }
            }
        }
        Ok(found)
    }
}

/// How many live persons a tree holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct TreePersonCount(pub(crate) Uuid);

impl Relation for TreePersonCount {
    type Value = i64;

    async fn load(
        db: &DatabaseConnection,
        keys: &[Self],
    ) -> Result<HashMap<Self, i64>, OxidGeneError> {
        let counts = PersonRepo::count_by_trees(db, &ids(keys, |key| key.0)).await?;
        Ok(counts
            .into_iter()
            .map(|(tree, count)| (TreePersonCount(tree), count))
            .collect())
    }
}

/// How many live families a tree holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct TreeFamilyCount(pub(crate) Uuid);

impl Relation for TreeFamilyCount {
    type Value = i64;

    async fn load(
        db: &DatabaseConnection,
        keys: &[Self],
    ) -> Result<HashMap<Self, i64>, OxidGeneError> {
        let counts = FamilyRepo::count_by_trees(db, &ids(keys, |key| key.0)).await?;
        Ok(counts
            .into_iter()
            .map(|(tree, count)| (TreeFamilyCount(tree), count))
            .collect())
    }
}
