//! Person-projection orchestration.
//!
//! [`ProfileService`] owns the read side of the domain: it materializes the
//! denormalized person projections into `person_denorm`, keeps
//! `person_search_fts` in step, and assembles pedigrees on demand by joining
//! the family links against those projections.
//!
//! Every mutation rewrites the bounded set of projections it invalidates, so
//! a projection is never stale, survives a restart, and behaves identically
//! on desktop (SQLite) and web (PostgreSQL). The service also holds the
//! process's [`TreeCache`], which keeps per tree what tree-wide reads would
//! otherwise compute again on each request, and nothing the database does
//! not hold.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use oxidgene_core::collections::sorted_unique;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::projection::{
    Pedigree, PedigreeDelta, PedigreeDirection, PedigreeEdge, PedigreeFamily, PedigreeNode,
    PersonProfile, SearchEntry, SearchResult,
};
use oxidgene_core::types::{Event, Person, RecordKind, TagRef};
use oxidgene_db::repo::{
    AncestryRepo, CitationRepo, Connections, EventRepo, FamilyChildRepo, FamilyRepo,
    FamilySpouseRepo, MediaLinkRepo, MediaRepo, NoteRepo, PersonDenormRepo, PersonDistinctRepo,
    PersonNameRepo, PersonRepo, PersonSearchFilters, PersonSearchRepo, PersonSearchSort, PlaceRepo,
    RecordTagRepo, SosaRepo, VignetteRepo, db_err,
};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, TransactionTrait};
use tracing::{Instrument as _, debug, info, instrument, warn};
use uuid::Uuid;

use super::builder::{self, TreeData, build_db_search_entry, search_entry_from_db};
use super::{invalidation, pedigree, sosa};
use crate::tree_cache::{TreeCache, TreeProfiles};

/// How many persons a projection refresh reads, builds and writes at a
/// time.
///
/// An affected set is 2 to 10 persons for an ordinary mutation and
/// thousands for a place, a tag or a surname many persons carry. Whatever
/// its size it is refreshed from targeted reads — the persons, their
/// families and what is attached to them — one slice after the other, so
/// that the work and the memory follow the affected persons and never the
/// tree: a slice costs the same statements whatever the tree around it.
const REFRESH_SLICE: usize = 500;

/// From how many persons a build runs on the blocking pool rather than on
/// the async worker that read their rows: a slice of a whole-tree rebuild
/// or of a wide refresh is tens of milliseconds of CPU, the few persons of
/// an ordinary mutation a fraction of one.
const BLOCKING_BUILD: usize = 64;

/// Page size of a person search when the caller names none.
pub const SEARCH_DEFAULT_LIMIT: usize = 25;

/// Largest page a person search returns, whatever the caller asks for.
///
/// Enforced in [`ProfileService::search_filtered`] rather than by each API
/// surface, so no transport can forget it.
pub const SEARCH_MAX_LIMIT: usize = 100;

/// Orchestrates the denormalized person projections and pedigree assembly.
///
/// Stored in the API's `AppState` as an `Arc<ProfileService>`; all methods
/// take `&self` so it can be shared across request handlers.
///
/// Its reads — pedigrees, searches, projections — go to the read pool. A
/// projection found missing or outdated there is built on the writer, which
/// those reads therefore never hold: none of them runs inside a write
/// transaction. Every other rebuild works on the connection or transaction
/// its caller passes.
#[derive(Debug)]
pub struct ProfileService {
    /// Where missing projections are materialized.
    writer: DatabaseConnection,
    /// Where projections and search rows are read.
    reader: DatabaseConnection,
    /// What the reads of a tree keep in memory between requests.
    cache: Arc<TreeCache>,
    /// The whole-tree rebuilds this process runs, one at a time per tree.
    rebuilds: Arc<Rebuilds>,
}

/// The gate requests wait at for a whole-tree rebuild.
type Gate = Arc<tokio::sync::Mutex<()>>;

/// The whole-tree rebuilds of one process: a gate per tree being rebuilt,
/// so that the requests finding a tree unmaterialized together wait for one
/// rebuild instead of each running its own, and counts of the rebuilds run
/// and of the projections built.
///
/// The gate only spares the waiting requests a connection each; what keeps
/// two rebuilds of a tree apart, across processes too, is the database
/// ([`PersonDenormRepo::lock_tree`]). SQLite has one writer, which a
/// rebuild holds for its whole transaction, so there the one gate serves
/// every tree: a second tree's rebuild waits here, where nothing times
/// out, rather than for the pool's only connection.
#[derive(Debug, Default)]
struct Rebuilds {
    gates: Mutex<HashMap<Option<Uuid>, Gate>>,
    run: AtomicU64,
    built: AtomicU64,
}

impl Rebuilds {
    fn gates(&self) -> MutexGuard<'_, HashMap<Option<Uuid>, Gate>> {
        // A panic while the lock was held left a map of gates, still
        // consistent: keep using it.
        self.gates
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The gate of `key`: a tree, or every tree.
    fn gate(&self, key: Option<Uuid>) -> Gate {
        Arc::clone(self.gates().entry(key).or_default())
    }

    /// Forget the gate of `key` once nobody else holds or awaits it.
    fn leave(&self, key: Option<Uuid>, gate: &Gate) {
        let mut gates = self.gates();
        // This handle and the map's.
        if Arc::strong_count(gate) <= 2 {
            gates.remove(&key);
        }
    }
}

impl ProfileService {
    /// Create a new profile service over `connections`: a single connection
    /// serves as both. Its cache has the default settings until
    /// [`TreeCache::configure`] changes them.
    pub fn new(connections: impl Into<Connections>) -> Self {
        let Connections { writer, reader } = connections.into();
        Self {
            writer,
            reader,
            cache: Arc::default(),
            rebuilds: Arc::default(),
        }
    }

    /// The writer: where derived rows a read finds stale are rebuilt. Only
    /// the web build's redaction rebuilds any.
    #[cfg(feature = "auth")]
    pub(crate) fn writer(&self) -> &DatabaseConnection {
        &self.writer
    }

    /// The read pool, where projections and search rows are read.
    pub(crate) fn reader(&self) -> &DatabaseConnection {
        &self.reader
    }

    /// What the reads of a tree keep in memory between requests.
    pub fn cache(&self) -> &Arc<TreeCache> {
        &self.cache
    }

    // ── Full tree rebuild ────────────────────────────────────────────────

    /// Rebuild every projection of a tree, plus its search rows, on the
    /// caller's transaction.
    ///
    /// Used by what writes a whole tree in a transaction of its own: an
    /// import, a duplication. The application never calls it on a plain
    /// connection: a rebuild deletes the tree's rows before writing them
    /// again, and outside a transaction a reader would find the tree
    /// materialized on a part of them ([`Self::rebuild_tree`] is the
    /// rebuild that opens its own).
    #[instrument(skip_all)]
    pub async fn rebuild_tree_full(
        &self,
        conn: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<usize, OxidGeneError> {
        Self::rebuild_in(conn, tree_id, &self.rebuilds).await
    }

    /// [`Self::rebuild_tree_full`] without the service: what a rebuild
    /// running on a task of its own calls.
    async fn rebuild_in(
        conn: &impl ConnectionTrait,
        tree_id: Uuid,
        rebuilds: &Rebuilds,
    ) -> Result<usize, OxidGeneError> {
        rebuilds.run.fetch_add(1, Ordering::AcqRel);

        PersonSearchRepo::delete_tree(conn, tree_id).await?;
        PersonDenormRepo::delete_tree(conn, tree_id).await?;
        let built = Self::build_tree(conn, tree_id, rebuilds).await?;
        #[cfg(feature = "auth")]
        super::visibility::rebuild_tree(conn, tree_id).await?;
        info!(count = built, "Completed full projection rebuild");
        Ok(built)
    }

    /// Build and write the projections and the search rows of every live
    /// person of a tree that has none: how many were built.
    ///
    /// The tree is built as an affected set is, a slice of persons at a
    /// time, in id order — the order persons were created in, so that a
    /// slice's relatives are mostly in it. What stays in memory for the
    /// whole rebuild is the tree's person ids, 16 bytes each.
    async fn build_tree(
        conn: &impl ConnectionTrait,
        tree_id: Uuid,
        rebuilds: &Rebuilds,
    ) -> Result<usize, OxidGeneError> {
        let mut person_ids: Vec<Uuid> = PersonRepo::live_ids(conn, tree_id)
            .await?
            .into_iter()
            .collect();
        person_ids.sort_unstable();
        let mut built = 0;
        for slice in person_ids.chunks(REFRESH_SLICE) {
            let persons = Self::build_targeted(conn, tree_id, slice, rebuilds).await?;
            PersonDenormRepo::upsert(conn, &persons).await?;
            let entries: Vec<_> = persons.iter().map(build_db_search_entry).collect();
            PersonSearchRepo::upsert(conn, &entries).await?;
            built += persons.len();
        }
        // Having just written a tree's worth of rows the planner has no
        // statistics for — an import's last bulk write: its history entry
        // stores no version. A tree without persons wrote none.
        if built > 0 {
            oxidgene_db::repo::refresh_statistics(conn).await;
        }
        Ok(built)
    }

    /// How many whole-tree rebuilds this service ran, for the tests and the
    /// measurements.
    pub fn full_rebuilds(&self) -> u64 {
        self.rebuilds.run.load(Ordering::Acquire)
    }

    /// How many projections this service built, in whole-tree rebuilds and
    /// refreshes alike, for the tests and the measurements: a refresh
    /// builds the persons it was given and no other.
    pub fn built_profiles(&self) -> u64 {
        self.rebuilds.built.load(Ordering::Acquire)
    }

    /// Whether the projections and the search rows of a tree are usable, as
    /// `conn` sees them.
    async fn is_materialized(
        conn: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<bool, OxidGeneError> {
        // Usable only when no row predates the current schema version.
        let denorm = PersonDenormRepo::is_materialized(conn, tree_id).await?;
        let search = PersonSearchRepo::has_tree(conn, tree_id).await?;
        if !(denorm && search) {
            debug!(denorm, search, "Tree projections are not materialized");
        }
        Ok(denorm && search)
    }

    /// Materialize a tree's projections if they have never been built.
    ///
    /// Covers missing or outdated projections and independently cleared
    /// search rows. Checked on the read pool; built, when the check fails,
    /// by [`Self::materialize`], which every request finding the same waits
    /// for.
    pub(super) async fn ensure_materialized(&self, tree_id: Uuid) -> Result<(), OxidGeneError> {
        if Self::is_materialized(&self.reader, tree_id).await? {
            return Ok(());
        }
        self.materialize(tree_id, false).await.map(drop)
    }

    /// Rebuild every projection of a tree and its search rows in one
    /// transaction on the writer: the projection admin operation of REST
    /// and GraphQL. How many persons were built.
    pub async fn rebuild_tree(&self, tree_id: Uuid) -> Result<usize, OxidGeneError> {
        self.materialize(tree_id, true).await
    }

    /// Rebuild the trees holding projections another build wrote — after
    /// an upgrade that changed their shape — one at a time, so that the
    /// first read of each does not have to: what a worker does when it
    /// starts and at each maintenance pass. A tree that fails is logged and
    /// left to its first read. How many trees were rebuilt.
    pub async fn rebuild_stale_trees(&self) -> Result<usize, OxidGeneError> {
        let mut rebuilt = 0;
        for tree_id in PersonDenormRepo::stale_trees(&self.reader).await? {
            match self.materialize(tree_id, false).await {
                Ok(_) => rebuilt += 1,
                Err(error) => warn!(
                    error.kind = crate::error_contract::error_kind(&error),
                    "could not rebuild the projections of a tree"
                ),
            }
        }
        Ok(rebuilt)
    }

    /// Rebuild tree `tree_id` whole — when `force`d, or when it is still
    /// not materialized once its turn comes — and return how many persons
    /// were built, 0 for a rebuild another request made meanwhile.
    ///
    /// The rebuild is one transaction on the writer: readers see the tree
    /// as it was until it commits, never a part of it. It runs on a task of
    /// its own, so a request that gives up waiting — a client gone — does
    /// not undo it for the requests waiting with it, nor for the next one.
    /// One rebuild runs at a time per tree: the requests of this process
    /// wait at the tree's gate, and the transaction takes the tree's
    /// projections ([`PersonDenormRepo::lock_tree`]) before it checks
    /// again, which other processes wait for too.
    async fn materialize(&self, tree_id: Uuid, force: bool) -> Result<usize, OxidGeneError> {
        let writer = self.writer.clone();
        let rebuilds = Arc::clone(&self.rebuilds);
        let rebuild = async move {
            let key = (writer.get_database_backend() != DbBackend::Sqlite).then_some(tree_id);
            let gate = rebuilds.gate(key);
            let built = {
                let _turn = gate.lock().await;
                Self::materialize_on(&writer, &rebuilds, tree_id, force).await
            };
            rebuilds.leave(key, &gate);
            built
        };
        tokio::spawn(rebuild.in_current_span())
            .await
            .map_err(|error| OxidGeneError::Internal(error.to_string()))?
    }

    /// The transaction of [`Self::materialize`].
    async fn materialize_on(
        writer: &DatabaseConnection,
        rebuilds: &Rebuilds,
        tree_id: Uuid,
        force: bool,
    ) -> Result<usize, OxidGeneError> {
        let txn = writer.begin().await.map_err(db_err)?;
        // A tree deleted meanwhile has nothing to build; one a request
        // ahead of this one rebuilt is read as it stands.
        let wanted = PersonDenormRepo::lock_tree(&txn, tree_id).await?
            && (force || !Self::is_materialized(&txn, tree_id).await?);
        if !wanted {
            txn.rollback().await.map_err(db_err)?;
            return Ok(0);
        }
        let built = Self::rebuild_in(&txn, tree_id, rebuilds).await?;
        txn.commit().await.map_err(db_err)?;
        Ok(built)
    }

    // ── SOSA ancestry ────────────────────────────────────────────────────

    /// Rebuild the stored SOSA ancestry of `tree_id` on the writer when a
    /// family link, a deletion or the root changed since it was built.
    #[instrument(skip_all)]
    async fn ensure_sosa(&self, tree_id: Uuid) -> Result<(), OxidGeneError> {
        if SosaRepo::is_current(&self.reader, tree_id).await? {
            return Ok(());
        }
        let txn = self.writer.begin().await.map_err(db_err)?;
        if let Some(generation) = SosaRepo::begin_rebuild(&txn, tree_id).await? {
            let ancestors = sosa::ancestry(&txn, tree_id).await?;
            debug!(count = ancestors.len(), "Rebuilt the SOSA ancestry");
            SosaRepo::replace_tree(&txn, tree_id, &ancestors, generation).await?;
        }
        txn.commit().await.map_err(db_err)
    }

    /// Those of `candidates` that are the SOSA root of `tree_id` or one of
    /// its ancestors.
    pub async fn sosa_ancestors_among(
        &self,
        tree_id: Uuid,
        candidates: &[Uuid],
    ) -> Result<HashSet<Uuid>, OxidGeneError> {
        self.ensure_sosa(tree_id).await?;
        SosaRepo::among(&self.reader, tree_id, candidates).await
    }

    /// The SOSA number of `person_id` in `tree_id`: `None` when the tree has
    /// no root, or the person is not a numbered ancestor of it.
    pub async fn sosa_number(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
    ) -> Result<Option<u64>, OxidGeneError> {
        self.ensure_sosa(tree_id).await?;
        SosaRepo::number(&self.reader, tree_id, person_id).await
    }

    // ── Person visibility ────────────────────────────────────────────────

    /// The stored inputs of the contemporary rule of `person_ids` in
    /// `tree_id` (docs/privacy.md §4.5), by person: the tree's rows are
    /// rebuilt on the writer first when they are missing or were derived by
    /// an earlier version of the rule. A person without a row is absent.
    #[cfg(feature = "auth")]
    pub async fn visibility(
        &self,
        tree_id: Uuid,
        person_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, oxidgene_core::privacy::PersonVisibility>, OxidGeneError> {
        use oxidgene_db::repo::PersonVisibilityRepo;
        if !PersonVisibilityRepo::is_materialized(&self.reader, tree_id).await? {
            let txn = self.writer.begin().await.map_err(db_err)?;
            if !PersonVisibilityRepo::is_materialized(&txn, tree_id).await? {
                super::visibility::rebuild_tree(&txn, tree_id).await?;
            }
            txn.commit().await.map_err(db_err)?;
        }
        PersonVisibilityRepo::get_many(&self.reader, person_ids).await
    }

    /// Every stored row of `tree_id`, rebuilt first as
    /// [`Self::visibility`] does.
    #[cfg(feature = "auth")]
    pub async fn tree_visibility(
        &self,
        tree_id: Uuid,
    ) -> Result<HashMap<Uuid, oxidgene_core::privacy::PersonVisibility>, OxidGeneError> {
        use oxidgene_db::repo::PersonVisibilityRepo;
        self.visibility(tree_id, &[]).await?;
        PersonVisibilityRepo::list_tree(&self.reader, tree_id).await
    }

    // ── Person projections ───────────────────────────────────────────────

    /// Read a person's projection, building it on the writer if absent.
    #[instrument(skip_all)]
    pub async fn get_or_build_person(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
    ) -> Result<PersonProfile, OxidGeneError> {
        if let Some(stored) = PersonDenormRepo::get(&self.reader, tree_id, person_id).await? {
            return Ok(stored);
        }

        debug!("Person projection is not materialized");
        self.rebuild_person(&self.writer, tree_id, person_id).await
    }

    /// [`Self::rebuild_person`] in a transaction of its own on the writer:
    /// the projection admin operation of REST and GraphQL.
    pub async fn rebuild_one(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
    ) -> Result<PersonProfile, OxidGeneError> {
        let txn = self.writer.begin().await.map_err(db_err)?;
        let profile = self.rebuild_person(&txn, tree_id, person_id).await?;
        txn.commit().await.map_err(db_err)?;
        Ok(profile)
    }

    /// [`Self::invalidate_tree`] in a transaction of its own on the writer:
    /// the projection admin operation of REST and GraphQL.
    pub async fn drop_tree(&self, tree_id: Uuid) -> Result<(), OxidGeneError> {
        let txn = self.writer.begin().await.map_err(db_err)?;
        self.invalidate_tree(&txn, tree_id).await?;
        txn.commit().await.map_err(db_err)
    }

    /// Rebuild one person's projection and its search row.
    #[instrument(skip_all)]
    pub async fn rebuild_person(
        &self,
        conn: &impl ConnectionTrait,
        tree_id: Uuid,
        person_id: Uuid,
    ) -> Result<PersonProfile, OxidGeneError> {
        let built = Self::build_targeted(conn, tree_id, &[person_id], &self.rebuilds)
            .await?
            .pop()
            .ok_or(OxidGeneError::NotFound {
                entity: "Person",
                id: person_id,
            })?;
        PersonDenormRepo::upsert(conn, std::slice::from_ref(&built)).await?;
        PersonSearchRepo::upsert(conn, &[build_db_search_entry(&built)]).await?;
        #[cfg(feature = "auth")]
        super::visibility::refresh(conn, tree_id, &[person_id]).await?;
        Ok(built)
    }

    /// Rebuild the projections of a set of persons, `REFRESH_SLICE` at a
    /// time, and return them: those a read found missing.
    ///
    /// Does not touch the search rows — callers that need them refreshed go
    /// through `Self::rebuild_affected`.
    ///
    /// # Errors
    ///
    /// `NotFound` for the first of `person_ids` that is not a live person
    /// of the tree: a read asked for it by name — a pedigree's root above
    /// all — and it must not answer about another tree's person, nor draw a
    /// window around nobody.
    #[instrument(skip_all, fields(count = person_ids.len()))]
    pub async fn rebuild_persons(
        &self,
        conn: &impl ConnectionTrait,
        tree_id: Uuid,
        person_ids: &[Uuid],
    ) -> Result<Vec<PersonProfile>, OxidGeneError> {
        let mut rebuilt = Vec::with_capacity(person_ids.len());
        for slice in person_ids.chunks(REFRESH_SLICE) {
            let built = Self::build_targeted(conn, tree_id, slice, &self.rebuilds).await?;
            if built.len() < slice.len() {
                let found: HashSet<Uuid> = built.iter().map(|p| p.person_id).collect();
                let id = slice.iter().copied().find(|id| !found.contains(id));
                return Err(OxidGeneError::NotFound {
                    entity: "Person",
                    id: id.unwrap_or_default(),
                });
            }
            PersonDenormRepo::upsert(conn, &built).await?;
            rebuilt.extend(built);
        }
        debug!(count = rebuilt.len(), "Rebuilt projections");
        Ok(rebuilt)
    }

    /// Every projection of a tree, materializing them if needed: those the
    /// tree cache keeps when the table has not changed since they were read,
    /// else read, decoded and kept.
    ///
    /// The payloads of a large tree run to tens of megabytes of JSON, so
    /// they are decoded on a blocking thread, under `profile.decode`.
    /// Requests arriving together for the same tree read them once.
    pub async fn get_all_persons(&self, tree_id: Uuid) -> Result<Arc<TreeProfiles>, OxidGeneError> {
        if let Some(kept) = self.current_profiles(tree_id).await? {
            return Ok(kept);
        }
        let gate = self.cache.profiles_gate(tree_id);
        let _reading = gate.lock().await;
        // Read by the request this one waited for.
        if let Some(kept) = self.current_profiles(tree_id).await? {
            return Ok(kept);
        }
        self.ensure_materialized(tree_id).await?;
        // Read before the rows: a write committed in between leaves the
        // projections newer than their generation, never older, and the
        // next request reads them again.
        let generation = PersonDenormRepo::generation(&self.reader, tree_id).await?;
        let encoded = PersonDenormRepo::list_tree(&self.reader, tree_id).await?;
        let bytes = encoded.payload_bytes() * crate::tree_cache::DECODED_BYTES_PER_PAYLOAD_BYTE;
        let span = tracing::info_span!("profile.decode", person.count = encoded.len());
        let decoded = crate::service::blocking::run(span, move || {
            encoded
                .decode()
                .map(|persons| Arc::new(TreeProfiles::new(persons)))
        })
        .await??;
        if let Some(generation) = generation {
            self.cache
                .keep_profiles(tree_id, generation, Arc::clone(&decoded), bytes);
        }
        Ok(decoded)
    }

    /// The projections of tree `tree_id` the tree cache keeps, when the
    /// table still holds them: the tree's projection generation has not
    /// moved since they were read. One primary-key read when the cache keeps
    /// some, none otherwise; projections found outdated are dropped.
    async fn current_profiles(
        &self,
        tree_id: Uuid,
    ) -> Result<Option<Arc<TreeProfiles>>, OxidGeneError> {
        let Some((generation, kept)) = self.cache.kept_profiles(tree_id) else {
            return Ok(None);
        };
        if PersonDenormRepo::generation(&self.reader, tree_id).await? == Some(generation) {
            return Ok(Some(kept));
        }
        drop(kept);
        self.cache.forget_profiles(tree_id, generation);
        Ok(None)
    }

    /// The projections the cache keeps for tree `tree_id` if they are
    /// current, else none after checking the tree is materialized: what a
    /// pedigree starts with.
    async fn pedigree_profiles(
        &self,
        tree_id: Uuid,
    ) -> Result<Option<Arc<TreeProfiles>>, OxidGeneError> {
        let kept = self.current_profiles(tree_id).await?;
        if kept.is_none() {
            self.ensure_materialized(tree_id).await?;
        }
        Ok(kept)
    }

    /// One page of a tree's projections, by person id, materializing the
    /// tree first if needed — what the API lists, where the whole tree in
    /// one answer ran to tens of megabytes.
    pub async fn persons_page(
        &self,
        tree_id: Uuid,
        params: &oxidgene_db::repo::PaginationParams,
    ) -> Result<oxidgene_core::types::Connection<PersonProfile>, OxidGeneError> {
        self.ensure_materialized(tree_id).await?;
        PersonDenormRepo::list_page(
            &self.reader,
            tree_id,
            params,
            oxidgene_db::repo::Unseen::NONE,
        )
        .await
    }

    // ── Pedigree ─────────────────────────────────────────────────────────

    /// Assemble a windowed pedigree for a root person.
    ///
    /// Built fresh on every call by walking the family links and joining the
    /// reached persons against their projections: those the tree cache
    /// keeps when they are current, `person_denorm` otherwise. Nothing is
    /// stored.
    #[instrument(skip_all)]
    pub async fn get_or_build_pedigree(
        &self,
        tree_id: Uuid,
        root_person_id: Uuid,
        ancestor_depth: u32,
        descendant_depth: u32,
    ) -> Result<Pedigree, OxidGeneError> {
        let kept = self.pedigree_profiles(tree_id).await?;
        self.build_pedigree(
            &self.reader,
            tree_id,
            root_person_id,
            (ancestor_depth, descendant_depth),
            kept.as_deref(),
        )
        .await
    }

    /// Assemble together the pedigrees of `roots`: each root that is a live
    /// person of the tree gets the pedigree [`Self::get_or_build_pedigree`]
    /// would give it, the others none.
    ///
    /// The batch shares its reads: one walk up and one down from all the
    /// roots, then one read per step of the assembly for all the windows,
    /// so it costs what one pedigree does, whatever the number of roots.
    pub(crate) async fn assemble_pedigrees(
        &self,
        tree_id: Uuid,
        roots: &[Uuid],
        ancestor_depth: u32,
        descendant_depth: u32,
    ) -> Result<HashMap<Uuid, Pedigree>, OxidGeneError> {
        // The projections are the tree's, not the roots': checked once for
        // the batch rather than once per pedigree.
        let kept = self.pedigree_profiles(tree_id).await?;
        let live = sorted_unique(
            PersonRepo::get_many(&self.reader, roots)
                .await?
                .into_iter()
                .filter(|person| person.tree_id == tree_id)
                .map(|person| person.id),
        );
        let pedigrees = self
            .build_pedigrees(
                &self.reader,
                tree_id,
                &live,
                (ancestor_depth, descendant_depth),
                kept.as_deref(),
            )
            .await?;
        Ok(live.into_iter().zip(pedigrees).collect())
    }

    /// Compute the nodes and edges a pedigree gains when expanded from
    /// `from_depth` to `to_depth` in one direction.
    ///
    /// `other_depth` is the depth already loaded in the *opposite* direction;
    /// pass it so the reported `*_depth_loaded` values match what the caller
    /// actually holds. Both windows are assembled and diffed — cheap now that
    /// a pedigree is a family-graph traversal plus a projection batch read.
    #[instrument(skip_all)]
    pub async fn expand_pedigree(
        &self,
        tree_id: Uuid,
        root_person_id: Uuid,
        direction: PedigreeDirection,
        from_depth: u32,
        to_depth: u32,
        other_depth: u32,
    ) -> Result<PedigreeDelta, OxidGeneError> {
        let conn = &self.reader;
        let kept = self.pedigree_profiles(tree_id).await?;

        let (before, after) = match direction {
            PedigreeDirection::Ancestors => ((from_depth, other_depth), (to_depth, other_depth)),
            PedigreeDirection::Descendants => ((other_depth, from_depth), (other_depth, to_depth)),
        };

        let existing = self
            .build_pedigree(conn, tree_id, root_person_id, before, kept.as_deref())
            .await?;
        let expanded = self
            .build_pedigree(conn, tree_id, root_person_id, after, kept.as_deref())
            .await?;

        let new_nodes: Vec<PedigreeNode> = expanded
            .persons
            .iter()
            .filter(|(id, _)| !existing.persons.contains_key(id))
            .map(|(_, node)| node.clone())
            .collect();

        let existing_edges: HashSet<(Uuid, Uuid)> = existing
            .edges
            .iter()
            .map(|e| (e.parent_id, e.child_id))
            .collect();

        let new_edges: Vec<PedigreeEdge> = expanded
            .edges
            .iter()
            .filter(|e| !existing_edges.contains(&(e.parent_id, e.child_id)))
            .cloned()
            .collect();

        Ok(PedigreeDelta {
            new_nodes,
            new_edges,
            ancestor_depth_loaded: expanded.ancestor_depth_loaded,
            descendant_depth_loaded: expanded.descendant_depth_loaded,
        })
    }

    // ── Search ───────────────────────────────────────────────────────────

    /// Search persons in a tree via the DB-native `person_search_fts` table
    /// (SQLite FTS5 / plain PostgreSQL table).
    #[instrument(skip_all)]
    pub async fn search(
        &self,
        tree_id: Uuid,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<SearchResult, OxidGeneError> {
        self.search_filtered(
            tree_id,
            query,
            &PersonSearchFilters::default(),
            PersonSearchSort::Relevance,
            limit,
            offset,
        )
        .await
    }

    /// Search persons with all filters, ordering, and pagination applied by
    /// the database before rows are returned. `limit` is capped at
    /// [`SEARCH_MAX_LIMIT`].
    pub async fn search_filtered(
        &self,
        tree_id: Uuid,
        query: &str,
        filters: &PersonSearchFilters,
        sort: PersonSearchSort,
        limit: usize,
        offset: usize,
    ) -> Result<SearchResult, OxidGeneError> {
        // SQL takes a signed offset: one past it would wrap to a negative
        // OFFSET, refused by PostgreSQL and read as none by SQLite.
        if i64::try_from(offset).is_err() {
            return Err(OxidGeneError::Validation("offset is too large".to_string()));
        }
        let conn = &self.reader;
        self.ensure_materialized(tree_id).await?;
        let page = PersonSearchRepo::search_filtered(
            conn,
            tree_id,
            query,
            filters,
            sort,
            limit.min(SEARCH_MAX_LIMIT) as u64,
            offset as u64,
        )
        .await?;
        let mut entries: Vec<SearchEntry> =
            page.entries.into_iter().map(search_entry_from_db).collect();
        crate::service::portrait::with_portraits(conn, tree_id, &mut entries).await?;
        attach_tags(conn, &mut entries).await?;
        Ok(SearchResult {
            entries,
            total_count: page.total_count as usize,
        })
    }

    /// Other persons of the tree bearing the same name as `person_id`, less
    /// those already confirmed to be somebody else.
    ///
    /// "The same name" is the folded primary surname and given names, see
    /// [`PersonSearchRepo::homonyms`]. At most [`SEARCH_MAX_LIMIT`] are read,
    /// before the confirmed ones are set aside.
    #[instrument(skip_all)]
    pub async fn homonyms(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
    ) -> Result<Vec<SearchEntry>, OxidGeneError> {
        let conn = &self.reader;
        self.ensure_materialized(tree_id).await?;
        let distinct = PersonDistinctRepo::distinct_from(conn, person_id).await?;
        let rows =
            PersonSearchRepo::homonyms(conn, tree_id, person_id, SEARCH_MAX_LIMIT as u64).await?;
        let mut entries: Vec<SearchEntry> = rows
            .into_iter()
            .filter(|row| !distinct.contains(&row.person_id))
            .map(search_entry_from_db)
            .collect();
        crate::service::portrait::with_portraits(conn, tree_id, &mut entries).await?;
        attach_tags(conn, &mut entries).await?;
        Ok(entries)
    }

    /// Search rows for a bounded set of persons, in the order asked for.
    ///
    /// Read from the projections, which are keyed by person, rather than
    /// from the search table, which a lookup by ID would have to scan.
    pub async fn search_entries(
        &self,
        tree_id: Uuid,
        person_ids: &[Uuid],
    ) -> Result<Vec<SearchEntry>, OxidGeneError> {
        let profiles = self
            .projections_for(&self.reader, tree_id, person_ids)
            .await?;
        let mut by_id: HashMap<Uuid, SearchEntry> = profiles
            .iter()
            .map(|profile| (profile.person_id, builder::build_search_entry(profile)))
            .collect();
        let mut entries: Vec<SearchEntry> = person_ids
            .iter()
            .filter_map(|id| by_id.remove(id))
            .collect();
        crate::service::portrait::with_portraits(&self.reader, tree_id, &mut entries).await?;
        Ok(entries)
    }

    // ── Invalidation ─────────────────────────────────────────────────────

    /// Drop a deleted person's projection and refresh everyone who referenced
    /// them.
    ///
    /// The `person_denorm` row would also go away on its own for a hard
    /// delete (`ON DELETE CASCADE`), but persons are soft-deleted by default,
    /// so it has to be removed explicitly.
    #[instrument(skip_all)]
    pub async fn invalidate_for_person_delete(
        &self,
        conn: &impl ConnectionTrait,
        tree_id: Uuid,
        person_id: Uuid,
    ) -> Result<(), OxidGeneError> {
        // Compute the affected set first — it is derived from the family links
        // that the delete is about to remove.
        let affected = invalidation::affected_persons(conn, person_id).await?;

        PersonDenormRepo::delete_person(conn, person_id).await?;
        PersonSearchRepo::delete_person(conn, person_id).await?;

        let remaining: Vec<Uuid> = affected.into_iter().filter(|&id| id != person_id).collect();
        if !remaining.is_empty() {
            self.rebuild_affected(conn, tree_id, &remaining).await?;
        }
        // The deleted person's row goes; their relatives' were refreshed with
        // their projections.
        #[cfg(feature = "auth")]
        super::visibility::refresh(conn, tree_id, &[person_id]).await?;

        debug!(
            count = remaining.len(),
            "Dropped projection and refreshed related persons"
        );
        Ok(())
    }

    /// Drop every projection and search row of a tree (used when the tree
    /// itself is deleted).
    #[instrument(skip_all)]
    pub async fn invalidate_tree(
        &self,
        conn: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<(), OxidGeneError> {
        info!("Dropping all projections for deleted tree");
        PersonSearchRepo::delete_tree(conn, tree_id).await?;
        #[cfg(feature = "auth")]
        oxidgene_db::repo::PersonVisibilityRepo::delete_tree(conn, tree_id).await?;
        PersonDenormRepo::delete_tree(conn, tree_id).await
    }

    /// Refresh an already-computed affected set.
    ///
    /// Used by REST and GraphQL handlers that call into the `invalidation`
    /// module themselves before mutating.
    #[instrument(skip_all, fields(count = affected.len()))]
    pub async fn invalidate_for_mutation(
        &self,
        conn: &impl ConnectionTrait,
        tree_id: Uuid,
        affected: &[Uuid],
    ) -> Result<(), OxidGeneError> {
        if affected.is_empty() {
            return Ok(());
        }
        self.rebuild_affected(conn, tree_id, affected).await
    }

    // ── Private helpers ──────────────────────────────────────────────────

    /// Rewrite the projections and search rows of an affected set,
    /// [`REFRESH_SLICE`] persons at a time, keeping none of them.
    ///
    /// Persons that no longer exist (soft-deleted, or removed between the
    /// affected-set computation and this call) are skipped rather than
    /// failing the whole refresh.
    async fn rebuild_affected(
        &self,
        conn: &impl ConnectionTrait,
        tree_id: Uuid,
        affected: &[Uuid],
    ) -> Result<(), OxidGeneError> {
        for slice in affected.chunks(REFRESH_SLICE) {
            let built = Self::build_targeted(conn, tree_id, slice, &self.rebuilds).await?;
            PersonDenormRepo::upsert(conn, &built).await?;
            let entries: Vec<_> = built.iter().map(build_db_search_entry).collect();
            PersonSearchRepo::upsert(conn, &entries).await?;
        }
        #[cfg(feature = "auth")]
        super::visibility::refresh(conn, tree_id, affected).await?;
        Ok(())
    }

    /// Build the projections of a bounded set of persons with targeted
    /// queries — the persons, their families, their relatives' names, and the
    /// entities attached to them. No full-tree fetch. Those of
    /// `person_ids` that are live persons of the tree are built, in their
    /// order.
    ///
    /// One fetch covers the whole set, so the query count does not grow with
    /// it: an edit that touches a person with a dozen relatives costs the same
    /// dozen statements as one that touches a single person.
    async fn build_targeted(
        conn: &impl ConnectionTrait,
        tree_id: Uuid,
        person_ids: &[Uuid],
        rebuilds: &Rebuilds,
    ) -> Result<Vec<PersonProfile>, OxidGeneError> {
        let data = Box::pin(Self::fetch_persons_data(conn, tree_id, person_ids)).await?;
        // A handful of persons are built where they are; a slice is CPU
        // work for a blocking thread.
        let built = if person_ids.len() < BLOCKING_BUILD {
            builder::build_persons(tree_id, person_ids, &data)
        } else {
            let person_ids = person_ids.to_vec();
            let span = tracing::info_span!("profile.build", person.count = person_ids.len());
            crate::service::blocking::run(span, move || {
                builder::build_persons(tree_id, &person_ids, &data)
            })
            .await?
        };
        rebuilds
            .built
            .fetch_add(built.len() as u64, Ordering::AcqRel);
        Ok(built)
    }

    /// The tags of the given persons and events, one grouped read per kind.
    async fn tags_of_persons_and_events(
        conn: &impl ConnectionTrait,
        persons: &[Person],
        events: &[Event],
    ) -> Result<(HashMap<Uuid, Vec<TagRef>>, HashMap<Uuid, Vec<TagRef>>), OxidGeneError> {
        let person_ids: Vec<Uuid> = persons.iter().map(|p| p.id).collect();
        let event_ids: Vec<Uuid> = events.iter().map(|e| e.id).collect();
        let (person_tags, event_tags) = tokio::try_join!(
            RecordTagRepo::tags_of(conn, RecordKind::Person, &person_ids),
            RecordTagRepo::tags_of(conn, RecordKind::Event, &event_ids),
        )?;
        Ok((person_tags, event_tags))
    }

    /// Fetch only what the given projections need: the persons, their family
    /// memberships, all members of those families (for spouse / parent /
    /// child denormalization), their events + places, media and notes.
    ///
    /// Callers box this future: the reads it runs at once make it the
    /// largest of a projection rebuild, and awaited inline it would be
    /// copied into the frame of every caller above it — every mutation's
    /// stack.
    async fn fetch_persons_data(
        conn: &impl ConnectionTrait,
        tree_id: Uuid,
        targets: &[Uuid],
    ) -> Result<TreeData, OxidGeneError> {
        // 1. Family memberships of the persons.
        let (as_spouse, as_child) = tokio::try_join!(
            FamilySpouseRepo::list_by_persons(conn, targets),
            FamilyChildRepo::list_by_persons(conn, targets),
        )?;
        let family_ids = sorted_unique(
            as_spouse
                .iter()
                .map(|s| s.family_id)
                .chain(as_child.iter().map(|c| c.family_id)),
        );
        // Deleting a family is a soft delete and leaves its membership rows
        // behind, so reaching families through them would resurrect one:
        // without this a rebuild would keep naming parents the user has
        // just removed.
        let mut family_ids = FamilyRepo::live_ids(conn, &family_ids).await?;
        family_ids.sort();

        // 2. All members of those families, plus attached entities.
        // Four statements at once at most, here as everywhere: what a
        // request takes from the pool at a time (docs/architecture.md §4).
        let (spouses, children, person_events, family_events) = tokio::try_join!(
            FamilySpouseRepo::list_by_families(conn, &family_ids),
            FamilyChildRepo::list_by_families(conn, &family_ids),
            EventRepo::list_by_persons(conn, targets),
            EventRepo::list_by_families(conn, &family_ids),
        )?;
        let (media_links, citation_counts, note_counts) = tokio::try_join!(
            MediaLinkRepo::list_by_persons(conn, targets),
            CitationRepo::count_by_persons(conn, targets),
            NoteRepo::count_by_persons(conn, tree_id, targets),
        )?;

        // 3. Related person rows + names, places, media, tags.
        let person_ids = sorted_unique(
            targets
                .iter()
                .copied()
                .chain(spouses.iter().map(|s| s.person_id))
                .chain(children.iter().map(|c| c.person_id)),
        );

        let mut events = person_events;
        events.extend(family_events);
        let place_ids = sorted_unique(events.iter().filter_map(|e| e.place_id));
        let media_ids: Vec<Uuid> = media_links.iter().map(|l| l.media_id).collect();

        let (persons, names, places, media) = tokio::try_join!(
            PersonRepo::get_many(conn, &person_ids),
            PersonNameRepo::list_by_persons(conn, &person_ids),
            PlaceRepo::get_many_untagged(conn, &place_ids),
            MediaRepo::get_many(conn, &media_ids),
        )?;

        // The pages of every linked document. A link names a document, and a
        // document holds no pixels: what a card can actually draw is its first
        // page, so the pages have to be here for the builder to find one.
        let mut media = media;
        let document_ids: Vec<Uuid> = media
            .iter()
            .filter(|item| item.is_document())
            .map(|item| item.id)
            .collect();
        media.extend(MediaRepo::list_pages_for(conn, &document_ids).await?);

        // The portrait crops, and the scans they sit on. Fetched after the
        // persons rather than alongside, because which crop to fetch is
        // written on the person; and appended to `media` because the
        // containing scan need not be one of the person's own links — a face
        // in somebody else's group photograph is still their portrait.
        let wanted: HashSet<Uuid> = targets.iter().copied().collect();
        let vignette_ids: Vec<Uuid> = persons
            .iter()
            .filter(|p| wanted.contains(&p.id))
            .filter_map(|p| p.portrait_vignette_id)
            .collect();
        let portrait_vignettes = VignetteRepo::get_many(conn, &vignette_ids).await?;
        let fetched: HashSet<Uuid> = media.iter().map(|m| m.id).collect();
        let scan_ids = sorted_unique(
            portrait_vignettes
                .iter()
                .map(|v| v.media_id)
                .filter(|id| !fetched.contains(id)),
        );
        media.extend(MediaRepo::get_many(conn, &scan_ids).await?);

        let (person_tags, event_tags) =
            Self::tags_of_persons_and_events(conn, &persons, &events).await?;

        Ok(TreeData {
            persons,
            names,
            events,
            places,
            spouses,
            children,
            media,
            media_links,
            portrait_vignettes,
            citation_counts,
            note_counts,
            person_tags,
            event_tags,
        })
    }

    /// Read the projections for a set of persons from `conn`, the read pool,
    /// rebuilding any that are missing (a person created before the tree was
    /// materialized) on the writer.
    #[instrument(name = "pedigree.projections", skip_all, fields(count = person_ids.len()))]
    async fn projections_for(
        &self,
        conn: &impl ConnectionTrait,
        tree_id: Uuid,
        person_ids: &[Uuid],
    ) -> Result<Vec<PersonProfile>, OxidGeneError> {
        if person_ids.is_empty() {
            return Ok(vec![]);
        }

        let mut found = PersonDenormRepo::get_many(conn, tree_id, person_ids).await?;
        let found_ids: HashSet<Uuid> = found.iter().map(|p| p.person_id).collect();
        let missing: Vec<Uuid> = person_ids
            .iter()
            .filter(|id| !found_ids.contains(id))
            .copied()
            .collect();

        if !missing.is_empty() {
            debug!(
                "Pedigree build: {} persons without a projection, building from DB",
                missing.len()
            );
            found.extend(
                self.rebuild_persons(&self.writer, tree_id, &missing)
                    .await?,
            );
        }
        Ok(found)
    }

    /// Gather the projections of those of `person_ids` not gathered yet:
    /// borrowed from the tree cache's when it keeps them, read otherwise.
    /// `outside` names the persons outside the pedigree windows that they
    /// are, for the log.
    async fn fetch_profiles<'c>(
        &self,
        conn: &impl ConnectionTrait,
        tree_id: Uuid,
        gathered: &mut Gathered<'c>,
        person_ids: &[Uuid],
        outside: Option<&str>,
    ) -> Result<(), OxidGeneError> {
        let profiles = &mut gathered.profiles;
        let mut wanted = sorted_unique(
            person_ids
                .iter()
                .copied()
                .filter(|id| !profiles.contains_key(id)),
        );
        if let Some(kept) = gathered.kept {
            wanted.retain(|id| match kept.get(id) {
                Some(profile) => {
                    profiles.insert(*id, Cow::Borrowed(profile));
                    false
                }
                None => true,
            });
        }
        if wanted.is_empty() {
            return Ok(());
        }
        if let Some(what) = outside {
            debug!(
                count = wanted.len(),
                "Pedigree build: fetching {what} outside the pedigree window"
            );
        }
        // Boxed: the rebuild of missing projections it may run is a deep
        // future, which every step of the assembly would otherwise hold
        // inline, and a debug build's stack a copy of at every level.
        let found = Box::pin(self.projections_for(conn, tree_id, &wanted)).await?;
        profiles.extend(found.into_iter().map(|p| (p.person_id, Cow::Owned(p))));
        Ok(())
    }

    /// Give each node of the windows its portrait and whether it is a SOSA
    /// ancestor: two queries for all the windows, the second against the
    /// stored SOSA ancestry, so the client neither asks whose portrait is
    /// what nor loads the tree's whole ancestry.
    async fn decorate_nodes(
        &self,
        conn: &impl ConnectionTrait,
        tree_id: Uuid,
        windows: &mut [HashMap<Uuid, PedigreeNode>],
    ) -> Result<(), OxidGeneError> {
        let ids = sorted_unique(windows.iter().flat_map(|nodes| nodes.keys().copied()));
        let portraits = crate::service::portrait::portrait_refs(conn, tree_id, &ids).await?;
        self.ensure_sosa(tree_id).await?;
        let sosa = SosaRepo::among(conn, tree_id, &ids).await?;
        for (id, node) in windows.iter_mut().flat_map(|nodes| nodes.iter_mut()) {
            node.portrait = portraits.get(id).cloned();
            node.sosa_ancestor = sosa.contains(id);
        }
        Ok(())
    }

    /// Assemble the pedigree window of a root person, `depths.0` generations
    /// up and `depths.1` down, from family links and the projections:
    /// `kept`, the tree cache's, first.
    async fn build_pedigree(
        &self,
        conn: &impl ConnectionTrait,
        tree_id: Uuid,
        root_person_id: Uuid,
        depths: (u32, u32),
        kept: Option<&TreeProfiles>,
    ) -> Result<Pedigree, OxidGeneError> {
        self.build_pedigrees(conn, tree_id, &[root_person_id], depths, kept)
            .await?
            .pop()
            .ok_or(OxidGeneError::NotFound {
                entity: "Person",
                id: root_person_id,
            })
    }

    /// Assemble the pedigree windows of `roots`, distinct, in their order,
    /// from family links and the projections — `kept`, the tree cache's,
    /// first, then the stored ones —: each step reads what all the windows
    /// need at once.
    #[instrument(
        name = "pedigree.build",
        skip_all,
        fields(roots = roots.len(), ancestor_depth = depths.0, descendant_depth = depths.1)
    )]
    async fn build_pedigrees(
        &self,
        conn: &impl ConnectionTrait,
        tree_id: Uuid,
        roots: &[Uuid],
        depths: (u32, u32),
        kept: Option<&TreeProfiles>,
    ) -> Result<Vec<Pedigree>, OxidGeneError> {
        let (ancestor_depth, descendant_depth) = depths;
        debug!(ancestor_depth, descendant_depth, "Building pedigrees");
        if roots.is_empty() {
            return Ok(Vec::new());
        }
        let mut gathered = Gathered {
            kept,
            profiles: HashMap::new(),
        };
        let windows = self
            .windows(conn, tree_id, roots, depths, &mut gathered)
            .await?;
        let mut nodes: Vec<_> = windows
            .iter()
            .map(|w| w.nodes(&gathered.profiles))
            .collect();
        self.decorate_nodes(conn, tree_id, &mut nodes).await?;
        let families = self
            .family_units(conn, tree_id, &windows, &nodes, &mut gathered)
            .await?;
        let profiles = gathered.profiles;

        let built_at = chrono::Utc::now();
        let pedigrees: Vec<Pedigree> = windows
            .iter()
            .zip(nodes)
            .zip(families)
            .map(|((window, nodes), families)| {
                let persons = window.persons(&profiles);
                Pedigree {
                    tree_id,
                    root_person_id: window.root,
                    edges: pedigree::edges(&persons, &nodes),
                    family_events: pedigree::family_events(&persons),
                    persons: nodes,
                    families,
                    ancestor_depth_loaded: ancestor_depth,
                    descendant_depth_loaded: descendant_depth,
                    built_at,
                }
            })
            .collect();
        debug!(
            nodes = pedigrees.iter().map(|p| p.persons.len()).sum::<usize>(),
            "Built pedigrees"
        );
        Ok(pedigrees)
    }

    /// The windows of `roots` — walked `depths.0` generations up and
    /// `depths.1` down — with the projections of their persons read into
    /// `profiles`, and the spouses outside them, who take their partner's
    /// generation.
    async fn windows(
        &self,
        conn: &impl ConnectionTrait,
        tree_id: Uuid,
        roots: &[Uuid],
        depths: (u32, u32),
        gathered: &mut Gathered<'_>,
    ) -> Result<Vec<pedigree::Window>, OxidGeneError> {
        let (mut ancestors, mut descendants) = tokio::try_join!(
            AncestryRepo::ancestors_of_many(conn, roots, Some(depths.0 as i32)),
            AncestryRepo::descendants_of_many(conn, roots, Some(depths.1 as i32)),
        )?;
        let mut windows: Vec<pedigree::Window> = roots
            .iter()
            .map(|root| {
                let up = ancestors.remove(root).unwrap_or_default();
                let down = descendants.remove(root).unwrap_or_default();
                pedigree::Window::new(*root, &up, &down)
            })
            .collect();
        let inside: Vec<Uuid> = windows
            .iter()
            .flat_map(|w| w.person_ids().iter().copied())
            .collect();
        self.fetch_profiles(conn, tree_id, gathered, &inside, None)
            .await?;

        // In id order, the order the projections come from the database in.
        let spouses: Vec<Vec<Uuid>> = windows
            .iter()
            .map(|w| sorted_unique(pedigree::spouses_outside(&w.persons(&gathered.profiles))))
            .collect();
        self.fetch_profiles(conn, tree_id, gathered, &spouses.concat(), Some("spouses"))
            .await?;
        for (window, spouses) in windows.iter_mut().zip(&spouses) {
            window.add_spouses(spouses, &gathered.profiles);
        }
        Ok(windows)
    }

    /// The family memberships of each window (spouse and children IDs per
    /// family), which capture childless couples, who produce no edge, and
    /// the parental families needed for sibling events — with the parents
    /// and family members outside the windows read into `profiles`.
    async fn family_units(
        &self,
        conn: &impl ConnectionTrait,
        tree_id: Uuid,
        windows: &[pedigree::Window],
        nodes: &[HashMap<Uuid, PedigreeNode>],
        gathered: &mut Gathered<'_>,
    ) -> Result<Vec<HashMap<Uuid, PedigreeFamily>>, OxidGeneError> {
        let mut families: Vec<_> = windows
            .iter()
            .map(|w| w.families(&gathered.profiles))
            .collect();
        let parents: Vec<Vec<Uuid>> = windows
            .iter()
            .zip(&families)
            .map(|(w, families)| {
                pedigree::parents_to_fetch(families, &w.persons(&gathered.profiles))
            })
            .collect();
        self.fetch_profiles(
            conn,
            tree_id,
            gathered,
            &parents.concat(),
            Some("parents for sibling data"),
        )
        .await?;
        let profiles = &gathered.profiles;
        for (families, parents) in families.iter_mut().zip(&parents) {
            pedigree::adopt_children_lists(families, &pedigree::picked(profiles, parents));
        }

        let members: Vec<Vec<Uuid>> = families
            .iter()
            .zip(nodes)
            .map(|(families, nodes)| pedigree::members_outside(families, nodes))
            .collect();
        self.fetch_profiles(
            conn,
            tree_id,
            gathered,
            &members.concat(),
            Some("family members"),
        )
        .await?;
        let profiles = &gathered.profiles;
        for (families, members) in families.iter_mut().zip(&members) {
            pedigree::add_members_outside(families, &pedigree::picked(profiles, members));
        }
        Ok(families)
    }
}

/// The projections a pedigree assembly has gathered, and the tree cache's,
/// which it borrows from before reading any.
struct Gathered<'c> {
    kept: Option<&'c TreeProfiles>,
    profiles: pedigree::Profiles<'c>,
}

/// Give each search row the tags its person has, read from the tag tables in
/// one grouped query. The search index holds no tags, so this is the only
/// place a row gets them.
async fn attach_tags(
    db: &impl ConnectionTrait,
    entries: &mut [SearchEntry],
) -> Result<(), OxidGeneError> {
    if entries.is_empty() {
        return Ok(());
    }
    let ids: Vec<Uuid> = entries.iter().map(|entry| entry.person_id).collect();
    let mut tags = RecordTagRepo::tags_of(db, RecordKind::Person, &ids).await?;
    for entry in entries {
        entry.tags = tags.remove(&entry.person_id).unwrap_or_default();
    }
    Ok(())
}
