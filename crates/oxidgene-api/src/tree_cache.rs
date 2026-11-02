//! What the API keeps in memory per tree between two requests, and when it
//! lets it go (docs/architecture.md, Tree cache).
//!
//! The durable read models are database tables (`person_denorm`,
//! `person_search_fts`, `sosa_ancestor`). This cache only spares a request
//! work whose result the previous one already computed, and holds nothing
//! the database does not: dropping any of it at any time changes no answer,
//! only how long the next one takes. It keeps, per tree:
//!
//! - the place dictionary's reading of each place label the tree uses
//!   ([`LabelReading`]), which the statistics, the anomalies and the list of
//!   places not located would otherwise read again on every request
//!   (120 to 355 ms on a tree of 10,000 persons). A reading depends on the
//!   label and the embedded dictionary alone, so no write can make one wrong:
//!   a renamed, merged or deleted place's label is dropped from the tree's
//!   readings by the next request reading all of them, and a new label is
//!   read when first needed. What depends on the tree — the places, their
//!   usages and their own coordinates — is read from the database on each
//!   request and weighed against the readings then.
//! - the tree's decoded person projections ([`TreeProfiles`]), which the
//!   statistics, the anomalies, the duplicates and the ancestry completeness
//!   would otherwise read from `person_denorm` and decode on every request
//!   (75 to 125 ms), and which a pedigree reads its window's persons from
//!   when they are kept. They are shared read-only, never copied, and kept
//!   with the generation of the tree's projections they were read at
//!   (`projection_state`, moved by triggers in the transaction of every
//!   `person_denorm` write, whatever process makes it): a request reads the
//!   current generation first — one primary-key read — and uses them only
//!   while it has not moved, so it never sees projections older than what
//!   it would read from the table.
//! - on the web, the redactions of restricted viewers ([`ClassRecords`]),
//!   one per [`ViewerClass`] — the redacting permissions of the viewer's
//!   access, its scope and the year — with the redaction generation of the
//!   tree they were read at (`projection_state.redaction_generation`,
//!   moved by triggers in the transaction of every write to a row a
//!   redaction reads). A request uses one only while that generation has
//!   not moved and the year is its own; the pseudonyms of a request are
//!   never part of it.
//!
//! What it holds is bounded and given back:
//!
//! - **One tree, or a budget.** On the desktop ([`CacheScope::OneTree`]) a
//!   request about another tree drops everything held for the previous one.
//!   On the web ([`CacheScope::Shared`]) trees are kept within a budget of
//!   estimated bytes, the least recently used dropped first; a tree that
//!   alone exceeds it is not kept.
//! - **Idleness.** A tree no request has used for [`CacheSettings::idle`]
//!   is dropped.
//! - **Classes.** A tree keeps [`MAX_CLASSES`] redactions at most, the
//!   least recently used dropped first; those of another generation or
//!   year are dropped as soon as a request finds them, and all of a tree's
//!   when its access changes ([`TreeCache::forget_redactions`]).
//!
//! Whatever is dropped is handed back to the system
//! ([`crate::memory::release_free_memory`]) once freed.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use oxidgene_core::OxidGeneError;
use oxidgene_core::projection::PersonProfile;
use oxidgene_core::redaction::{ClassRedaction, Viewing};
use tokio::task::JoinHandle;
use tokio::time::Instant;
use uuid::Uuid;

use crate::authz::records::HiddenRecords;
use crate::reference::LabelReading;

/// How long a tree's cached data outlives the last request that used it.
///
/// Five minutes: a person working on a tree moves between its statistics,
/// its tools and its pedigrees within seconds or a few minutes, and each
/// return within that time is answered without reading the tree again. Past
/// it the session has moved on, and reading the tree again — a fraction of
/// a second — costs less than holding its tens of megabytes for good.
pub const DEFAULT_IDLE: Duration = Duration::from_secs(5 * 60);

/// The budget of a shared cache when the deployment names none: 256 MiB.
///
/// A quarter of the web backend's memory limit in the Helm chart (1 GiB),
/// which leaves the rest to the requests themselves and the place
/// dictionary's index; it holds the projections of about 70,000 persons at
/// the 3.5 KB a person of a real tree, 100,000 of the generated reference one.
pub const DEFAULT_BUDGET: usize = 256 * 1024 * 1024;

/// How many viewer classes' redactions a tree keeps at most: the owner's
/// Editors, Guests and Visitors, each with or without a scope or two, and a
/// new year's beside the last one's for a moment. Past it the least
/// recently used class is dropped, so classes nobody uses any more age out
/// rather than pile up.
pub const MAX_CLASSES: usize = 8;

/// Which trees the cache keeps at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheScope {
    /// One: the desktop, where one person works on one tree at a time. A
    /// request about another tree drops everything held for the previous one.
    OneTree,
    /// As many as fit in `budget` estimated bytes, the least recently used
    /// dropped first: the web, where several people work on several trees.
    Shared { budget: usize },
}

/// What the cache keeps, and for how long.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheSettings {
    pub scope: CacheScope,
    /// How long a tree's data outlives the last request using it.
    pub idle: Duration,
}

impl Default for CacheSettings {
    fn default() -> Self {
        Self {
            scope: CacheScope::Shared {
                budget: DEFAULT_BUDGET,
            },
            idle: DEFAULT_IDLE,
        }
    }
}

impl CacheSettings {
    /// One tree at a time, dropped after `idle`: the desktop.
    pub fn one_tree(idle: Duration) -> Self {
        Self {
            scope: CacheScope::OneTree,
            idle,
        }
    }

    /// Trees within `budget` bytes, each dropped after `idle`: the web.
    pub fn shared(budget: usize, idle: Duration) -> Self {
        Self {
            scope: CacheScope::Shared { budget },
            idle,
        }
    }
}

/// The per-tree cache of one API process. See the module documentation.
#[derive(Debug, Default)]
pub struct TreeCache {
    inner: Mutex<Inner>,
    /// How many times dropped data was handed back to the system.
    releases: AtomicU64,
    /// The year redactions are made for when set, the clock's otherwise:
    /// the tests' clock.
    year: AtomicI32,
}

/// Who a restricted viewer's redaction is for: everything it depends on
/// besides the tree's rows (docs/architecture.md §4.1). It is made of the
/// access the request resolved from the database and of the server's
/// clock, never of anything the client sends; the build of a redaction
/// receives it and nothing else about the viewer, so two viewers of one
/// class are shown the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ViewerClass {
    /// The redacting permissions of the access.
    pub viewing: Viewing,
    /// The access scope the viewer is limited to, none for the whole tree.
    pub scope_id: Option<Uuid>,
    /// The year the contemporary rule is applied in.
    pub year: i32,
}

/// What a viewer class sees of a tree, as the tree cache keeps it: the
/// decisions of its redaction and the hidden records they imply, shared
/// read-only by the requests of the class. A request's pseudonyms are not
/// part of it.
#[derive(Debug)]
pub struct ClassRecords {
    pub redaction: Arc<ClassRedaction>,
    pub hidden: Arc<HiddenRecords>,
}

impl ClassRecords {
    /// The bytes it takes in memory, as the budget counts them.
    pub fn estimated_bytes(&self) -> usize {
        size_of::<Self>() + self.redaction.estimated_bytes() + self.hidden.estimated_bytes()
    }
}

#[derive(Debug, Default)]
struct Inner {
    settings: CacheSettings,
    trees: HashMap<Uuid, Entry>,
    /// The task dropping idle trees, while there are trees to watch.
    sweeper: Option<JoinHandle<()>>,
}

/// What is kept for one tree.
#[derive(Debug)]
struct Entry {
    last_used: Instant,
    /// The dictionary's reading of each place label the tree uses.
    places: HashMap<Box<str>, Arc<LabelReading>>,
    place_bytes: usize,
    /// Held while the tree's missing readings are read, so that requests
    /// arriving together read them once.
    places_gate: Arc<tokio::sync::Mutex<()>>,
    /// The tree's decoded projections, with the generation they were read
    /// at.
    profiles: Option<KeptProfiles>,
    /// Held while the tree's projections are read, so that requests
    /// arriving together read them once.
    profiles_gate: Arc<tokio::sync::Mutex<()>>,
    /// The redactions of restricted viewer classes, at most [`MAX_CLASSES`].
    redactions: Vec<KeptRedaction>,
    /// Held while a redaction of the tree is built, so that requests of one
    /// class arriving together build it once.
    redactions_gate: Arc<tokio::sync::Mutex<()>>,
}

#[derive(Debug)]
struct KeptRedaction {
    class: ViewerClass,
    /// The tree's redaction generation it was read at.
    generation: i64,
    records: Arc<ClassRecords>,
    bytes: usize,
    last_used: Instant,
}

#[derive(Debug)]
struct KeptProfiles {
    generation: i64,
    profiles: Arc<TreeProfiles>,
    bytes: usize,
}

impl Entry {
    fn new(now: Instant) -> Self {
        Self {
            last_used: now,
            places: HashMap::new(),
            place_bytes: 0,
            places_gate: Arc::default(),
            profiles: None,
            profiles_gate: Arc::default(),
            redactions: Vec::new(),
            redactions_gate: Arc::default(),
        }
    }

    fn bytes(&self) -> usize {
        self.place_bytes
            + self.profiles.as_ref().map_or(0, |kept| kept.bytes)
            + self.redactions.iter().map(|kept| kept.bytes).sum::<usize>()
    }

    /// Take out the redactions read before `generation`, the tree's
    /// current one, or for another year than `year`. Generations only
    /// grow: one read after `generation`, by a request that read the
    /// generation later, is newer and stays.
    fn take_stale_redactions(&mut self, generation: i64, year: i32) -> Vec<KeptRedaction> {
        let (stale, current) = std::mem::take(&mut self.redactions)
            .into_iter()
            .partition(|kept| kept.generation < generation || kept.class.year != year);
        self.redactions = current;
        stale
    }
}

/// Every person projection of a tree, decoded once and shared read-only by
/// the requests that read them, with an index by person.
#[derive(Debug, Default)]
pub struct TreeProfiles {
    persons: Vec<PersonProfile>,
    index: HashMap<Uuid, usize>,
}

impl TreeProfiles {
    /// The projections `persons`, indexed.
    pub fn new(persons: Vec<PersonProfile>) -> Self {
        let index = persons
            .iter()
            .enumerate()
            .map(|(at, person)| (person.person_id, at))
            .collect();
        Self { persons, index }
    }

    /// The projection of `person_id`, if the tree has one.
    pub fn get(&self, person_id: &Uuid) -> Option<&PersonProfile> {
        self.index.get(person_id).map(|at| &self.persons[*at])
    }
}

impl std::ops::Deref for TreeProfiles {
    type Target = [PersonProfile];

    fn deref(&self) -> &[PersonProfile] {
        &self.persons
    }
}

/// How many bytes decoded projections take in memory for each byte of their
/// stored payload: on a generated tree of 10,000 persons, release build, the
/// 13 MiB of payload took 25 to 27 MiB of resident memory once kept.
pub const DECODED_BYTES_PER_PAYLOAD_BYTE: usize = 2;

impl TreeCache {
    /// An empty cache with `settings`.
    pub fn new(settings: CacheSettings) -> Self {
        let cache = Self::default();
        cache.configure(settings);
        cache
    }

    /// Keep what `settings` allow from now on. Trees already kept stay until
    /// their next use or idle sweep applies the new settings.
    pub fn configure(&self, settings: CacheSettings) {
        self.lock().settings = settings;
    }

    /// The settings in force.
    pub fn settings(&self) -> CacheSettings {
        self.lock().settings
    }

    /// The trees with something kept, for tests and the measurements.
    pub fn cached_trees(&self) -> Vec<Uuid> {
        self.lock().trees.keys().copied().collect()
    }

    /// The estimated bytes kept for all trees.
    pub fn estimated_bytes(&self) -> usize {
        self.lock().trees.values().map(Entry::bytes).sum()
    }

    /// How many times dropped data was handed back to the system.
    pub fn releases(&self) -> u64 {
        self.releases.load(Ordering::Acquire)
    }

    /// The year redactions are made for: the clock's, unless
    /// [`Self::set_year`] fixed one.
    pub fn current_year(&self) -> i32 {
        match self.year.load(Ordering::Acquire) {
            0 => chrono::Datelike::year(&chrono::Utc::now()),
            year => year,
        }
    }

    /// Make redactions for `year`, or for the clock's year again with
    /// `None`: the tests' clock.
    pub fn set_year(&self, year: Option<i32>) {
        self.year.store(year.unwrap_or(0), Ordering::Release);
    }

    /// The viewer classes whose redaction of tree `tree_id` is kept, for
    /// the tests and the measurements.
    pub fn redaction_classes(&self, tree_id: Uuid) -> Vec<ViewerClass> {
        self.lock()
            .trees
            .get(&tree_id)
            .map(|entry| entry.redactions.iter().map(|kept| kept.class).collect())
            .unwrap_or_default()
    }

    /// The estimated bytes of the redactions kept for tree `tree_id`.
    pub fn redaction_bytes(&self, tree_id: Uuid) -> usize {
        self.lock().trees.get(&tree_id).map_or(0, |entry| {
            entry.redactions.iter().map(|kept| kept.bytes).sum()
        })
    }

    /// Drop everything kept, and give the memory back.
    pub fn clear(&self) {
        let dropped: Vec<Entry> = self.lock().trees.drain().map(|(_, e)| e).collect();
        self.release(dropped);
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // A panic while the lock was held left a cache of plain data, still
        // consistent: keep using it.
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Note a request about `tree_id`: its entry, created if need be, is
    /// used now. With [`CacheScope::OneTree`] every other tree is dropped.
    fn enter<T>(self: &Arc<Self>, tree_id: Uuid, then: impl FnOnce(&mut Entry) -> T) -> T {
        let now = Instant::now();
        let (result, dropped) = {
            let mut inner = self.lock();
            let dropped = match inner.settings.scope {
                CacheScope::OneTree => {
                    let others: Vec<Uuid> = inner
                        .trees
                        .keys()
                        .filter(|id| **id != tree_id)
                        .copied()
                        .collect();
                    others
                        .iter()
                        .filter_map(|id| inner.trees.remove(id))
                        .collect()
                }
                CacheScope::Shared { .. } => Vec::new(),
            };
            let entry = inner
                .trees
                .entry(tree_id)
                .or_insert_with(|| Entry::new(now));
            entry.last_used = now;
            let result = then(entry);
            self.watch(&mut inner);
            (result, dropped)
        };
        self.release(dropped);
        result
    }

    /// Make room for `extra` more bytes for `tree_id` within a shared
    /// budget, dropping the least recently used other trees into `dropped`:
    /// whether they fit, `tree_id`'s current bytes counted.
    fn make_room(inner: &mut Inner, tree_id: Uuid, extra: usize, dropped: &mut Vec<Entry>) -> bool {
        let CacheScope::Shared { budget } = inner.settings.scope else {
            return true;
        };
        let own = inner.trees.get(&tree_id).map_or(0, Entry::bytes);
        if own + extra > budget {
            return false;
        }
        loop {
            let total: usize = inner.trees.values().map(Entry::bytes).sum();
            if total + extra <= budget {
                return true;
            }
            let Some(oldest) = inner
                .trees
                .iter()
                .filter(|(id, entry)| **id != tree_id && entry.bytes() > 0)
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(id, _)| *id)
            else {
                return false;
            };
            dropped.extend(inner.trees.remove(&oldest));
        }
    }

    /// The dictionary's readings of `labels` for tree `tree_id`, in order:
    /// those kept, and the others read now and kept.
    ///
    /// `tree_labels`, when the caller read them all, are every label the
    /// tree's places bear: the readings of any other label — a place renamed,
    /// merged or deleted since — are dropped.
    pub async fn place_readings(
        self: &Arc<Self>,
        tree_id: Uuid,
        labels: &[&str],
        tree_labels: Option<&HashSet<&str>>,
    ) -> Result<Vec<Arc<LabelReading>>, OxidGeneError> {
        let gate = self.enter(tree_id, |entry| Arc::clone(&entry.places_gate));
        let _reading = gate.lock().await;
        let mut kept = self.kept_readings(tree_id, labels);
        let missing: Vec<String> = {
            let mut seen = HashSet::new();
            labels
                .iter()
                .filter(|label| !kept.contains_key(**label) && seen.insert(**label))
                .map(|label| (*label).to_string())
                .collect()
        };
        let read = if missing.is_empty() {
            Vec::new()
        } else {
            let span = tracing::info_span!("tree_cache.read_places", label.count = missing.len());
            let names = missing.clone();
            let read = crate::service::blocking::run(span, move || {
                let names: Vec<&str> = names.iter().map(String::as_str).collect();
                crate::reference::read_labels(&names)
            })
            .await?;
            missing
                .into_iter()
                .zip(read.into_iter().map(Arc::new))
                .collect()
        };
        for (label, reading) in &read {
            kept.insert(label.as_str().into(), Arc::clone(reading));
        }
        self.keep_readings(tree_id, read, tree_labels);
        Ok(labels
            .iter()
            .map(|label| Arc::clone(&kept[*label]))
            .collect())
    }

    /// The readings kept for those of `labels` the tree has.
    fn kept_readings(
        &self,
        tree_id: Uuid,
        labels: &[&str],
    ) -> HashMap<Box<str>, Arc<LabelReading>> {
        let inner = self.lock();
        let Some(entry) = inner.trees.get(&tree_id) else {
            return HashMap::new();
        };
        labels
            .iter()
            .filter_map(|label| Some(((*label).into(), Arc::clone(entry.places.get(*label)?))))
            .collect()
    }

    /// Keep the readings just read for tree `tree_id`, unless the tree was
    /// dropped meanwhile or they do not fit, after dropping those of labels
    /// the tree no longer has (`tree_labels`).
    fn keep_readings(
        &self,
        tree_id: Uuid,
        read: Vec<(String, Arc<LabelReading>)>,
        tree_labels: Option<&HashSet<&str>>,
    ) {
        let mut dropped = Vec::new();
        let mut stale = Vec::new();
        {
            let mut inner = self.lock();
            let Some(entry) = inner.trees.get_mut(&tree_id) else {
                return;
            };
            if let Some(tree_labels) = tree_labels {
                let before = entry.places.len();
                entry.places.retain(|label, reading| {
                    let used = tree_labels.contains(&**label);
                    if !used {
                        stale.push(Arc::clone(reading));
                    }
                    used
                });
                if entry.places.len() != before {
                    entry.place_bytes = entry
                        .places
                        .iter()
                        .map(|(label, reading)| reading.estimated_bytes(label))
                        .sum();
                }
            }
            let extra: usize = read
                .iter()
                .map(|(label, reading)| reading.estimated_bytes(label))
                .sum();
            if extra > 0
                && Self::make_room(&mut inner, tree_id, extra, &mut dropped)
                && let Some(entry) = inner.trees.get_mut(&tree_id)
            {
                for (label, reading) in read {
                    entry.places.insert(label.into_boxed_str(), reading);
                }
                entry.place_bytes += extra;
            }
        }
        drop(stale);
        self.release(dropped);
    }

    /// The projections kept for tree `tree_id`, with the generation they
    /// were read at, noting a request about the tree: with one tree at a
    /// time, every other tree is dropped. The caller checks the generation
    /// is still the table's before using them.
    pub fn kept_profiles(self: &Arc<Self>, tree_id: Uuid) -> Option<(i64, Arc<TreeProfiles>)> {
        self.enter(tree_id, |entry| {
            entry
                .profiles
                .as_ref()
                .map(|kept| (kept.generation, Arc::clone(&kept.profiles)))
        })
    }

    /// The gate a request reading tree `tree_id`'s projections holds, so
    /// that requests arriving together read them once.
    pub fn profiles_gate(self: &Arc<Self>, tree_id: Uuid) -> Arc<tokio::sync::Mutex<()>> {
        self.enter(tree_id, |entry| Arc::clone(&entry.profiles_gate))
    }

    /// Drop tree `tree_id`'s projections read at `generation`, which the
    /// table has moved past, and give their memory back before the caller
    /// reads them again.
    pub fn forget_profiles(&self, tree_id: Uuid, generation: i64) {
        let stale = {
            let mut inner = self.lock();
            inner
                .trees
                .get_mut(&tree_id)
                .filter(|entry| {
                    entry
                        .profiles
                        .as_ref()
                        .is_some_and(|kept| kept.generation == generation)
                })
                .and_then(|entry| entry.profiles.take())
        };
        if let Some(stale) = stale {
            self.release_profiles(stale);
        }
    }

    /// Keep tree `tree_id`'s projections, read at `generation` and taking
    /// about `bytes`, unless the tree was dropped meanwhile or they do not
    /// fit the budget.
    pub fn keep_profiles(
        &self,
        tree_id: Uuid,
        generation: i64,
        profiles: Arc<TreeProfiles>,
        bytes: usize,
    ) {
        let mut dropped = Vec::new();
        let replaced = {
            let mut inner = self.lock();
            let Some(entry) = inner.trees.get_mut(&tree_id) else {
                return;
            };
            let replaced = entry.profiles.take();
            if Self::make_room(&mut inner, tree_id, bytes, &mut dropped)
                && let Some(entry) = inner.trees.get_mut(&tree_id)
            {
                entry.profiles = Some(KeptProfiles {
                    generation,
                    profiles,
                    bytes,
                });
            }
            replaced
        };
        if let Some(replaced) = replaced {
            self.release_profiles(replaced);
        }
        self.release(dropped);
    }

    fn release_profiles(&self, kept: KeptProfiles) {
        let mut entry = Entry::new(Instant::now());
        entry.profiles = Some(kept);
        self.release(vec![entry]);
    }

    /// The redaction of `class` kept for tree `tree_id` at `generation`,
    /// the tree's current redaction generation, noting a request about the
    /// tree: with one tree at a time, every other tree is dropped. The
    /// tree's redactions read at another generation or for another year
    /// are dropped and their memory given back.
    pub fn kept_redaction(
        self: &Arc<Self>,
        tree_id: Uuid,
        class: ViewerClass,
        generation: i64,
    ) -> Option<Arc<ClassRecords>> {
        let (found, stale) = self.enter(tree_id, |entry| {
            let stale = entry.take_stale_redactions(generation, class.year);
            let found = entry
                .redactions
                .iter_mut()
                .find(|kept| kept.class == class && kept.generation == generation)
                .map(|kept| {
                    kept.last_used = Instant::now();
                    Arc::clone(&kept.records)
                });
            (found, stale)
        });
        self.release_redactions(stale);
        found
    }

    /// The gate a request building a redaction of tree `tree_id` holds, so
    /// that requests of one class arriving together build it once.
    pub fn redactions_gate(self: &Arc<Self>, tree_id: Uuid) -> Arc<tokio::sync::Mutex<()>> {
        self.enter(tree_id, |entry| Arc::clone(&entry.redactions_gate))
    }

    /// Keep `records`, the redaction of `class` for tree `tree_id` read at
    /// redaction generation `generation`, unless the tree was dropped
    /// meanwhile, it keeps one of the class as new already, or it does not
    /// fit the budget: after dropping the tree's redactions of an older
    /// generation or another year, and the least recently used class when
    /// the tree keeps [`MAX_CLASSES`].
    pub fn keep_redaction(
        &self,
        tree_id: Uuid,
        class: ViewerClass,
        generation: i64,
        records: Arc<ClassRecords>,
    ) {
        let bytes = records.estimated_bytes();
        let mut dropped = Vec::new();
        let mut gone = Vec::new();
        {
            let mut inner = self.lock();
            let Some(entry) = inner.trees.get_mut(&tree_id) else {
                return;
            };
            gone.extend(entry.take_stale_redactions(generation, class.year));
            if entry.redactions.iter().any(|kept| kept.class == class) {
                drop(inner);
                self.release_redactions(gone);
                return;
            }
            if entry.redactions.len() >= MAX_CLASSES
                && let Some(oldest) = entry
                    .redactions
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, kept)| kept.last_used)
                    .map(|(at, _)| at)
            {
                gone.push(entry.redactions.swap_remove(oldest));
            }
            if Self::make_room(&mut inner, tree_id, bytes, &mut dropped)
                && let Some(entry) = inner.trees.get_mut(&tree_id)
            {
                entry.redactions.push(KeptRedaction {
                    class,
                    generation,
                    records,
                    bytes,
                    last_used: Instant::now(),
                });
            }
        }
        self.release_redactions(gone);
        self.release(dropped);
    }

    /// Drop every redaction kept for tree `tree_id` and give their memory
    /// back: an access to the tree changed — a membership, a share link,
    /// its visibility — or the tree is gone.
    pub fn forget_redactions(&self, tree_id: Uuid) {
        let gone = self
            .lock()
            .trees
            .get_mut(&tree_id)
            .map(|entry| std::mem::take(&mut entry.redactions))
            .unwrap_or_default();
        self.release_redactions(gone);
    }

    /// Drop everything kept for tree `tree_id`, deleted or purged, and give
    /// the memory back.
    pub fn forget_tree(&self, tree_id: Uuid) {
        let gone = self.lock().trees.remove(&tree_id);
        self.release(gone.into_iter().collect());
    }

    fn release_redactions(&self, gone: Vec<KeptRedaction>) {
        if gone.is_empty() {
            return;
        }
        let mut entry = Entry::new(Instant::now());
        entry.redactions = gone;
        self.release(vec![entry]);
    }

    /// Start the idle sweep if trees are kept and none runs.
    fn watch(self: &Arc<Self>, inner: &mut Inner) {
        if inner.trees.is_empty() || inner.sweeper.as_ref().is_some_and(|s| !s.is_finished()) {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let cache = Arc::downgrade(self);
        inner.sweeper = Some(runtime.spawn(async move {
            while let Some(deadline) = Self::sweep(&cache) {
                tokio::time::sleep_until(deadline).await;
            }
        }));
    }

    /// Drop the trees idle for longer than the settings allow: when to look
    /// again, or `None` once nothing is kept (or the cache is gone).
    fn sweep(cache: &Weak<Self>) -> Option<Instant> {
        let cache = cache.upgrade()?;
        let now = Instant::now();
        let (dropped, next) = {
            let mut inner = cache.lock();
            let idle = inner.settings.idle;
            let expired: Vec<Uuid> = inner
                .trees
                .iter()
                .filter(|(_, entry)| entry.last_used + idle <= now)
                .map(|(id, _)| *id)
                .collect();
            let dropped: Vec<Entry> = expired
                .iter()
                .filter_map(|id| inner.trees.remove(id))
                .collect();
            let next = inner.trees.values().map(|e| e.last_used + idle).min();
            if next.is_none() {
                inner.sweeper = None;
            }
            (dropped, next)
        };
        if !dropped.is_empty() {
            tracing::debug!(count = dropped.len(), "Dropped idle trees from the cache");
        }
        cache.release(dropped);
        next
    }

    /// Free `dropped` and hand the memory back to the system, off the async
    /// workers: freeing a tree's projections and trimming the allocator's
    /// arenas take milliseconds. A request still using some of the data
    /// frees it when it ends, and the idle release after requests
    /// ([`crate::memory::release_when_idle`]) hands that back.
    fn release(&self, dropped: Vec<Entry>) {
        if dropped.is_empty() {
            return;
        }
        self.releases.fetch_add(1, Ordering::AcqRel);
        let work = move || {
            drop(dropped);
            crate::memory::release_free_memory();
        };
        if tokio::runtime::Handle::try_current().is_ok() {
            drop(crate::service::blocking::spawn(work));
        } else {
            work();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> Uuid {
        Uuid::now_v7()
    }

    /// Keep a reading of each of `labels` for `tree`, as a request having
    /// read them would; no dictionary is read.
    fn fill(cache: &Arc<TreeCache>, tree: Uuid, labels: &[&str]) {
        cache.enter(tree, |_| ());
        let read = labels
            .iter()
            .map(|label| ((*label).to_string(), Arc::new(LabelReading::nowhere())))
            .collect();
        cache.keep_readings(tree, read, None);
    }

    fn kept(cache: &TreeCache, tree: Uuid, labels: &[&str]) -> usize {
        cache.kept_readings(tree, labels).len()
    }

    #[tokio::test]
    async fn kept_readings_are_reused_and_dropped_with_the_labels_the_tree_no_longer_has() {
        let cache = Arc::new(TreeCache::new(CacheSettings::default()));
        let tree = tree();
        fill(&cache, tree, &["Nowhere-A", "Nowhere-B"]);
        let first = cache.kept_readings(tree, &["Nowhere-A"]);
        // All kept: nothing is read.
        let again = cache
            .place_readings(tree, &["Nowhere-A", "Nowhere-B"], None)
            .await
            .unwrap();
        assert!(Arc::ptr_eq(&first["Nowhere-A"], &again[0]));
        assert!(cache.estimated_bytes() > 0);

        // "Nowhere-B" was renamed: the next whole read drops its reading.
        let renamed: HashSet<&str> = ["Nowhere-A"].into();
        cache
            .place_readings(tree, &["Nowhere-A"], Some(&renamed))
            .await
            .unwrap();
        assert_eq!(kept(&cache, tree, &["Nowhere-A"]), 1);
        assert_eq!(kept(&cache, tree, &["Nowhere-B"]), 0);
        assert_eq!(
            cache.estimated_bytes(),
            LabelReading::nowhere().estimated_bytes("Nowhere-A")
        );
    }

    #[tokio::test]
    async fn one_tree_scope_drops_the_previous_tree_and_releases_its_memory() {
        let cache = Arc::new(TreeCache::new(CacheSettings::one_tree(DEFAULT_IDLE)));
        let (a, b) = (tree(), tree());
        fill(&cache, a, &["Nowhere-A"]);
        assert_eq!(cache.cached_trees(), [a]);
        let releases = cache.releases();
        // The first request about b, whatever it reads.
        cache.place_readings(b, &[], None).await.unwrap();
        assert_eq!(cache.cached_trees(), [b]);
        assert_eq!(cache.releases(), releases + 1);
    }

    #[tokio::test]
    async fn a_shared_budget_drops_the_least_recently_used_tree() {
        let one = LabelReading::nowhere().estimated_bytes("Nowhere-A");
        let cache = Arc::new(TreeCache::new(CacheSettings::shared(2 * one, DEFAULT_IDLE)));
        let (a, b, c) = (tree(), tree(), tree());
        fill(&cache, a, &["Nowhere-A"]);
        fill(&cache, b, &["Nowhere-A"]);
        // Tree a used again: b is now the least recently used.
        cache.place_readings(a, &["Nowhere-A"], None).await.unwrap();
        let releases = cache.releases();
        fill(&cache, c, &["Nowhere-A"]);
        assert_eq!(kept(&cache, a, &["Nowhere-A"]), 1);
        assert_eq!(kept(&cache, b, &["Nowhere-A"]), 0);
        assert_eq!(kept(&cache, c, &["Nowhere-A"]), 1);
        assert_eq!(cache.releases(), releases + 1);
        assert!(cache.estimated_bytes() <= 2 * one);

        // A tree whose readings alone exceed the budget is not kept.
        let d = tree();
        let labels = ["Nowhere-A", "Nowhere-B", "Nowhere-C"];
        fill(&cache, d, &labels);
        assert_eq!(kept(&cache, d, &labels), 0);
        assert_eq!(kept(&cache, a, &["Nowhere-A"]), 1, "nothing dropped for it");
    }

    #[tokio::test]
    async fn projections_are_kept_by_generation_within_the_budget() {
        let cache = Arc::new(TreeCache::new(CacheSettings::shared(1000, DEFAULT_IDLE)));
        let (a, b) = (tree(), tree());
        let profiles = Arc::new(TreeProfiles::default());
        assert!(cache.kept_profiles(a).is_none());
        cache.keep_profiles(a, 7, Arc::clone(&profiles), 600);
        let (generation, kept) = cache.kept_profiles(a).unwrap();
        assert_eq!(generation, 7);
        assert!(Arc::ptr_eq(&kept, &profiles), "shared, never copied");

        // Another generation's forgetting leaves them; their own drops them.
        cache.forget_profiles(a, 6);
        assert!(cache.kept_profiles(a).is_some());
        let releases = cache.releases();
        cache.forget_profiles(a, 7);
        assert!(cache.kept_profiles(a).is_none());
        assert_eq!(cache.releases(), releases + 1);

        // b's projections make room by dropping a's, the least recent.
        cache.keep_profiles(a, 8, Arc::clone(&profiles), 600);
        cache.kept_profiles(b);
        cache.keep_profiles(b, 1, Arc::clone(&profiles), 600);
        assert!(cache.kept_profiles(a).is_none());
        assert!(cache.kept_profiles(b).is_some());
        // Projections larger than the whole budget are not kept.
        cache.keep_profiles(b, 2, profiles, 1001);
        assert!(cache.kept_profiles(b).is_none());
        assert_eq!(cache.estimated_bytes(), 0);
    }

    #[tokio::test]
    async fn projections_of_a_dropped_tree_are_not_kept() {
        let cache = Arc::new(TreeCache::new(CacheSettings::one_tree(DEFAULT_IDLE)));
        let (a, b) = (tree(), tree());
        // A request about a reads its projections while b's request comes.
        let _gate = cache.profiles_gate(a);
        cache.kept_profiles(b);
        cache.keep_profiles(a, 1, Arc::default(), 10);
        assert_eq!(cache.cached_trees(), [b]);
    }

    #[tokio::test]
    async fn an_idle_tree_is_dropped_and_its_memory_released() {
        let idle = Duration::from_millis(200);
        let cache = Arc::new(TreeCache::new(CacheSettings::shared(DEFAULT_BUDGET, idle)));
        let (a, b) = (tree(), tree());
        fill(&cache, a, &["Nowhere-A"]);
        tokio::time::sleep(idle / 2).await;
        fill(&cache, b, &["Nowhere-A"]);
        let releases = cache.releases();
        // Past a's idle time, within b's.
        tokio::time::sleep(idle * 3 / 4).await;
        assert_eq!(cache.cached_trees(), [b]);
        assert_eq!(cache.releases(), releases + 1);
        tokio::time::sleep(idle).await;
        assert!(cache.cached_trees().is_empty());
        assert_eq!(cache.estimated_bytes(), 0);
        assert_eq!(cache.releases(), releases + 2);
    }
}
