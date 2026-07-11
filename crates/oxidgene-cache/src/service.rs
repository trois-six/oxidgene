//! Cache service — orchestrates cache builds, lookups, and invalidation.
//!
//! [`CacheService`] is the single entry point that the API layer uses to
//! interact with the cache. It wraps a [`CacheStore`] implementation and
//! coordinates with the DB via repository methods, the [`builder`] module for
//! constructing cache entries, and the [`invalidation`] module for computing
//! affected sets.

use std::fmt;
use std::sync::Arc;

use oxidgene_core::error::OxidGeneError;
use oxidgene_db::repo::{
    EventRepo, FamilyChildRepo, FamilyRepo, FamilySpouseRepo, MediaLinkRepo, MediaRepo, NoteRepo,
    PersonAncestryRepo, PersonNameRepo, PersonRepo, PersonSearchRepo, PlaceRepo, TreeRepo,
};
use oxidgene_db::sea_orm::DatabaseConnection;
use tracing::{debug, info, instrument};
use uuid::Uuid;

use crate::builder::{
    self, TreeData, build_all_persons, build_db_search_entry, build_pedigree_node,
    search_entry_from_db,
};
use crate::invalidation;
use crate::store::CacheStore;
use crate::types::{
    CachedPedigree, CachedPerson, PedigreeDelta, PedigreeDirection, PedigreeEdge, PedigreeNode,
    SearchResult,
};

/// The cache service orchestrates all cache operations.
///
/// It is designed to be stored in the API's `AppState` as an `Arc<CacheService>`.
/// All methods take `&self` so it can be shared across request handlers.
pub struct CacheService {
    store: Arc<dyn CacheStore>,
    db: DatabaseConnection,
}

impl fmt::Debug for CacheService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CacheService")
            .field("store", &"<dyn CacheStore>")
            .finish()
    }
}

impl CacheService {
    /// Create a new cache service.
    pub fn new(store: Arc<dyn CacheStore>, db: DatabaseConnection) -> Self {
        Self { store, db }
    }

    /// Access the underlying cache store (e.g. for direct reads).
    pub fn store(&self) -> &dyn CacheStore {
        &*self.store
    }

    // ── Full tree rebuild ────────────────────────────────────────────────

    /// Rebuild the entire cache for a tree (all persons, search index,
    /// and optionally the pedigree for the sosa root).
    ///
    /// Used after GEDCOM import or when the cache is cold. This runs
    /// eagerly and populates all three cache layers.
    #[instrument(skip(self), fields(tree_id = %tree_id))]
    pub async fn rebuild_tree_full(&self, tree_id: Uuid) -> Result<usize, OxidGeneError> {
        info!("Starting full cache rebuild for tree {}", tree_id);

        // 1. Fetch all data in parallel.
        let tree_data = self.fetch_tree_data(tree_id).await?;

        // 2. Build all CachedPerson entries.
        let cached_persons = build_all_persons(tree_id, &tree_data);
        debug!(
            "Built {} cached persons for tree {}",
            cached_persons.len(),
            tree_id
        );

        // 3. Store persons in batch (no-op on stores that don't cache persons).
        self.store.set_persons_batch(&cached_persons).await?;

        // 4. Rebuild the DB-native search table (SQLite FTS5 / PG table).
        let search_entries: Vec<_> = cached_persons.iter().map(build_db_search_entry).collect();
        PersonSearchRepo::replace_tree(&self.db, tree_id, &search_entries).await?;
        debug!(
            "Rebuilt person_search_fts with {} rows for tree {}",
            search_entries.len(),
            tree_id
        );

        // 5. If the tree has a sosa root, build the default pedigree.
        let tree = TreeRepo::get(&self.db, tree_id).await?;
        if let Some(root_id) = tree.sosa_root_person_id {
            self.rebuild_pedigree(tree_id, root_id, 4, 1).await?;
        }

        info!("Completed full cache rebuild for tree {}", tree_id);
        Ok(cached_persons.len())
    }

    // ── Person cache ─────────────────────────────────────────────────────

    /// Get a cached person, building it on-demand if not in cache.
    #[instrument(skip(self), fields(tree_id = %tree_id, person_id = %person_id))]
    pub async fn get_or_build_person(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
    ) -> Result<CachedPerson, OxidGeneError> {
        // Try cache first.
        if let Some(cached) = self.store.get_person(tree_id, person_id).await? {
            return Ok(cached);
        }

        // Cache miss — build from DB.
        debug!("Cache miss for person {}, building from DB", person_id);
        let cached = self.build_single_person(tree_id, person_id).await?;
        self.store.set_person(&cached).await?;
        Ok(cached)
    }

    /// Rebuild the cache for a single person (and refresh their search row).
    #[instrument(skip(self), fields(tree_id = %tree_id, person_id = %person_id))]
    pub async fn rebuild_person(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
    ) -> Result<CachedPerson, OxidGeneError> {
        let cached = self.build_single_person(tree_id, person_id).await?;
        self.store.set_person(&cached).await?;
        PersonSearchRepo::upsert(&self.db, &[build_db_search_entry(&cached)]).await?;
        Ok(cached)
    }

    /// Rebuild the cache for multiple persons (used after invalidation).
    ///
    /// On stores that cache persons (Redis), all tree data is fetched once
    /// and the requested entries are rebuilt from it and stored. On stores
    /// that don't (desktop), each person is built with targeted queries
    /// against local SQLite — affected sets are bounded (2–10 persons).
    #[instrument(skip(self, person_ids), fields(tree_id = %tree_id, count = person_ids.len()))]
    pub async fn rebuild_persons(
        &self,
        tree_id: Uuid,
        person_ids: &[Uuid],
    ) -> Result<Vec<CachedPerson>, OxidGeneError> {
        if person_ids.is_empty() {
            return Ok(vec![]);
        }

        let requested: Vec<CachedPerson> = if self.store.caches_persons() {
            // Fetch all tree data once and rebuild from it.
            let tree_data = self.fetch_tree_data(tree_id).await?;
            let all_cached = build_all_persons(tree_id, &tree_data);
            all_cached
                .into_iter()
                .filter(|p| person_ids.contains(&p.person_id))
                .collect()
        } else {
            let mut built = Vec::with_capacity(person_ids.len());
            for &pid in person_ids {
                built.push(self.build_single_person(tree_id, pid).await?);
            }
            built
        };

        self.store.set_persons_batch(&requested).await?;

        debug!(
            "Rebuilt {} person caches for tree {}",
            requested.len(),
            tree_id
        );

        Ok(requested)
    }

    /// Get all cached persons for a tree.
    ///
    /// On stores that don't cache persons (desktop), they are built directly
    /// from the database. Otherwise a full rebuild is triggered if the cache
    /// is empty.
    pub async fn get_all_persons(&self, tree_id: Uuid) -> Result<Vec<CachedPerson>, OxidGeneError> {
        if !self.store.caches_persons() {
            let tree_data = self.fetch_tree_data(tree_id).await?;
            return Ok(build_all_persons(tree_id, &tree_data));
        }

        let cached = self.store.get_all_persons(tree_id).await?;
        if !cached.is_empty() {
            return Ok(cached);
        }

        // Cache is empty — full rebuild.
        debug!(
            "No cached persons for tree {}, triggering full rebuild",
            tree_id
        );
        self.rebuild_tree_full(tree_id).await?;
        self.store.get_all_persons(tree_id).await
    }

    // ── Pedigree cache ───────────────────────────────────────────────────

    /// Get a pedigree, building on-demand if not cached or if additional
    /// depth is needed.
    #[instrument(skip(self), fields(tree_id = %tree_id, root_person_id = %root_person_id))]
    pub async fn get_or_build_pedigree(
        &self,
        tree_id: Uuid,
        root_person_id: Uuid,
        ancestor_depth: u32,
        descendant_depth: u32,
    ) -> Result<CachedPedigree, OxidGeneError> {
        if let Some(cached) = self.store.get_pedigree(tree_id, root_person_id).await? {
            // Check if the cached pedigree has sufficient depth.
            if cached.ancestor_depth_loaded >= ancestor_depth
                && cached.descendant_depth_loaded >= descendant_depth
            {
                return Ok(cached);
            }
            // Insufficient depth — rebuild with the larger depth.
            debug!(
                "Cached pedigree has depth ({}/{}), need ({}/{}), rebuilding",
                cached.ancestor_depth_loaded,
                cached.descendant_depth_loaded,
                ancestor_depth,
                descendant_depth
            );
        }

        self.rebuild_pedigree(tree_id, root_person_id, ancestor_depth, descendant_depth)
            .await
    }

    /// Expand an existing pedigree by additional levels in a given direction.
    ///
    /// Returns a [`PedigreeDelta`] containing only the new nodes and edges,
    /// so the frontend can merge them incrementally without re-fetching the
    /// entire tree.
    #[instrument(skip(self), fields(tree_id = %tree_id, root_person_id = %root_person_id))]
    pub async fn expand_pedigree(
        &self,
        tree_id: Uuid,
        root_person_id: Uuid,
        direction: PedigreeDirection,
        additional_levels: u32,
    ) -> Result<PedigreeDelta, OxidGeneError> {
        let existing = self
            .store
            .get_pedigree(tree_id, root_person_id)
            .await?
            .ok_or(OxidGeneError::NotFound {
                entity: "CachedPedigree",
                id: root_person_id,
            })?;

        let (new_ancestor_depth, new_descendant_depth) = match direction {
            PedigreeDirection::Ancestors => (
                existing.ancestor_depth_loaded + additional_levels,
                existing.descendant_depth_loaded,
            ),
            PedigreeDirection::Descendants => (
                existing.ancestor_depth_loaded,
                existing.descendant_depth_loaded + additional_levels,
            ),
        };

        // Rebuild the full pedigree with expanded depth.
        let new_pedigree = self
            .rebuild_pedigree(
                tree_id,
                root_person_id,
                new_ancestor_depth,
                new_descendant_depth,
            )
            .await?;

        // Compute delta: new nodes and edges that weren't in the old pedigree.
        let new_nodes: Vec<PedigreeNode> = new_pedigree
            .persons
            .iter()
            .filter(|(id, _)| !existing.persons.contains_key(id))
            .map(|(_, node)| node.clone())
            .collect();

        let existing_edges: std::collections::HashSet<(Uuid, Uuid)> = existing
            .edges
            .iter()
            .map(|e| (e.parent_id, e.child_id))
            .collect();

        let new_edges: Vec<PedigreeEdge> = new_pedigree
            .edges
            .iter()
            .filter(|e| !existing_edges.contains(&(e.parent_id, e.child_id)))
            .cloned()
            .collect();

        Ok(PedigreeDelta {
            new_nodes,
            new_edges,
            ancestor_depth_loaded: new_pedigree.ancestor_depth_loaded,
            descendant_depth_loaded: new_pedigree.descendant_depth_loaded,
        })
    }

    // ── Search ───────────────────────────────────────────────────────────

    /// Search persons in a tree via the DB-native `person_search_fts` table
    /// (SQLite FTS5 / plain PostgreSQL table).
    ///
    /// If the table has no rows for this tree (e.g. first run after the E.6
    /// migration), it is populated from the database on demand.
    #[instrument(skip(self), fields(tree_id = %tree_id, query = %query))]
    pub async fn search(
        &self,
        tree_id: Uuid,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<SearchResult, OxidGeneError> {
        self.ensure_search_index(tree_id).await?;
        let page =
            PersonSearchRepo::search(&self.db, tree_id, query, limit as u64, offset as u64).await?;
        Ok(SearchResult {
            entries: page.entries.into_iter().map(search_entry_from_db).collect(),
            total_count: page.total_count as usize,
        })
    }

    /// Populate `person_search_fts` for a tree if it has no rows yet.
    async fn ensure_search_index(&self, tree_id: Uuid) -> Result<(), OxidGeneError> {
        if PersonSearchRepo::count_tree(&self.db, tree_id).await? > 0 {
            return Ok(());
        }

        debug!(
            "person_search_fts empty for tree {}, populating from DB",
            tree_id
        );
        let tree_data = self.fetch_tree_data(tree_id).await?;
        if tree_data.persons.is_empty() {
            return Ok(());
        }
        let persons = build_all_persons(tree_id, &tree_data);
        let entries: Vec<_> = persons.iter().map(build_db_search_entry).collect();
        PersonSearchRepo::replace_tree(&self.db, tree_id, &entries).await?;

        // Warm the person cache too while we have everything built (Redis path).
        if self.store.caches_persons() {
            self.store.set_persons_batch(&persons).await?;
        }
        Ok(())
    }

    // ── Invalidation ─────────────────────────────────────────────────────

    /// Invalidate caches after a person mutation (edit person, edit name,
    /// add/edit/delete event on a person).
    ///
    /// This is the primary invalidation entry point. It:
    /// 1. Computes the affected person set.
    /// 2. Rebuilds their `CachedPerson` entries.
    /// 3. Updates the search index.
    /// 4. Patches any loaded pedigrees that contain affected persons.
    #[instrument(skip(self), fields(tree_id = %tree_id, person_id = %person_id))]
    pub async fn invalidate_for_person(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
    ) -> Result<(), OxidGeneError> {
        let affected = invalidation::affected_persons(&self.db, person_id).await?;
        debug!(
            "Invalidating {} persons for mutation on person {}",
            affected.len(),
            person_id
        );
        self.rebuild_affected(tree_id, &affected).await
    }

    /// Invalidate caches after a family event mutation (marriage, divorce, etc.).
    #[instrument(skip(self), fields(tree_id = %tree_id, family_id = %family_id))]
    pub async fn invalidate_for_family_event(
        &self,
        tree_id: Uuid,
        family_id: Uuid,
    ) -> Result<(), OxidGeneError> {
        let affected = invalidation::affected_persons_for_family(&self.db, family_id).await?;
        debug!(
            "Invalidating {} persons for family event on family {}",
            affected.len(),
            family_id
        );
        self.rebuild_affected(tree_id, &affected).await
    }

    /// Invalidate caches after a family spouse change (add/remove spouse).
    #[instrument(skip(self), fields(tree_id = %tree_id, family_id = %family_id))]
    pub async fn invalidate_for_family_spouse_change(
        &self,
        tree_id: Uuid,
        family_id: Uuid,
        changed_person_id: Uuid,
    ) -> Result<(), OxidGeneError> {
        let affected = invalidation::affected_persons_for_family_spouse_change(
            &self.db,
            family_id,
            changed_person_id,
        )
        .await?;
        debug!(
            "Invalidating {} persons for spouse change in family {}",
            affected.len(),
            family_id
        );
        self.rebuild_affected(tree_id, &affected).await
    }

    /// Invalidate caches after a family child change (add/remove child).
    #[instrument(skip(self), fields(tree_id = %tree_id, family_id = %family_id))]
    pub async fn invalidate_for_family_child_change(
        &self,
        tree_id: Uuid,
        family_id: Uuid,
        child_person_id: Uuid,
    ) -> Result<(), OxidGeneError> {
        let affected = invalidation::affected_persons_for_family_child_change(
            &self.db,
            family_id,
            child_person_id,
        )
        .await?;
        debug!(
            "Invalidating {} persons for child change in family {}",
            affected.len(),
            family_id
        );
        self.rebuild_affected(tree_id, &affected).await
    }

    /// Invalidate after a person is deleted.
    ///
    /// Removes the person from the cache and rebuilds everyone who
    /// referenced them.
    #[instrument(skip(self), fields(tree_id = %tree_id, person_id = %person_id))]
    pub async fn invalidate_for_person_delete(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
    ) -> Result<(), OxidGeneError> {
        // Compute affected set BEFORE the person is deleted from cache,
        // since we need to know who references this person.
        let affected = invalidation::affected_persons(&self.db, person_id).await?;

        // Remove the deleted person from the cache and the search table.
        self.store.delete_person(tree_id, person_id).await?;
        PersonSearchRepo::delete_person(&self.db, person_id).await?;

        // Rebuild remaining affected persons (excluding the deleted one)
        // and refresh their search rows.
        let remaining: Vec<Uuid> = affected.into_iter().filter(|&id| id != person_id).collect();
        if !remaining.is_empty() {
            let rebuilt = self.rebuild_persons(tree_id, &remaining).await?;
            let entries: Vec<_> = rebuilt.iter().map(build_db_search_entry).collect();
            PersonSearchRepo::upsert(&self.db, &entries).await?;
        }

        // Delete pedigrees that contain this person — they need full rebuild.
        // For simplicity, delete all pedigrees for the tree; they'll be
        // rebuilt on next access.
        self.store.delete_all_pedigrees(tree_id).await?;

        debug!(
            "Invalidated cache for deleted person {}, rebuilt {} related persons",
            person_id,
            remaining.len()
        );

        Ok(())
    }

    /// Invalidate all caches for a tree (used when the tree is deleted).
    #[instrument(skip(self), fields(tree_id = %tree_id))]
    pub async fn invalidate_tree(&self, tree_id: Uuid) -> Result<(), OxidGeneError> {
        info!("Invalidating all caches for tree {}", tree_id);
        PersonSearchRepo::delete_tree(&self.db, tree_id).await?;
        self.store.invalidate_tree(tree_id).await
    }

    /// Invalidate and rebuild caches for a given set of affected persons.
    ///
    /// This is the generic entry-point used by REST and GraphQL handlers that
    /// have already computed the affected set via the `invalidation` module.
    /// It rebuilds each person's cache, the search index, and drops pedigrees.
    #[instrument(skip(self, affected), fields(tree_id = %tree_id, count = affected.len()))]
    pub async fn invalidate_for_mutation(
        &self,
        tree_id: Uuid,
        affected: &[Uuid],
    ) -> Result<(), OxidGeneError> {
        if affected.is_empty() {
            return Ok(());
        }
        self.rebuild_affected(tree_id, affected).await
    }

    // ── Private helpers ──────────────────────────────────────────────────

    /// Rebuild the affected persons' caches, their search rows, and drop
    /// the tree's pedigrees.
    async fn rebuild_affected(
        &self,
        tree_id: Uuid,
        affected: &[Uuid],
    ) -> Result<(), OxidGeneError> {
        // 1. Rebuild person caches.
        let rebuilt = self.rebuild_persons(tree_id, affected).await?;

        // 2. Refresh only the affected rows in person_search_fts (bounded
        //    set — no full index rebuild needed).
        let entries: Vec<_> = rebuilt.iter().map(build_db_search_entry).collect();
        PersonSearchRepo::upsert(&self.db, &entries).await?;

        // 3. Patch pedigrees — for now, delete all pedigrees for the tree
        //    so they're rebuilt on next access. A more sophisticated approach
        //    would patch only the affected nodes, but that's an optimization
        //    for a later sprint.
        self.store.delete_all_pedigrees(tree_id).await?;

        Ok(())
    }

    /// Build a single person's cache entry from the database with targeted
    /// queries (the person, their families, relatives' names, attached
    /// events/places/media/notes) — no full-tree fetch.
    async fn build_single_person(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
    ) -> Result<CachedPerson, OxidGeneError> {
        let data = self.fetch_person_data(tree_id, person_id).await?;
        builder::build_person(tree_id, person_id, &data).ok_or(OxidGeneError::NotFound {
            entity: "Person",
            id: person_id,
        })
    }

    /// Fetch only the data needed to build one `CachedPerson`: the person,
    /// their family memberships, all members of those families (for spouse /
    /// parent / child denormalization), their events + places, media and
    /// notes.
    async fn fetch_person_data(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
    ) -> Result<TreeData, OxidGeneError> {
        // 1. Family memberships of the person.
        let (as_spouse, as_child) = tokio::try_join!(
            FamilySpouseRepo::list_by_person(&self.db, person_id),
            FamilyChildRepo::list_by_person(&self.db, person_id),
        )?;
        let mut family_ids: Vec<Uuid> = as_spouse
            .iter()
            .map(|s| s.family_id)
            .chain(as_child.iter().map(|c| c.family_id))
            .collect();
        family_ids.sort();
        family_ids.dedup();

        // 2. All members of those families, plus attached entities.
        let (spouses, children, person_events, family_events, media_links, notes) = tokio::try_join!(
            FamilySpouseRepo::list_by_families(&self.db, &family_ids),
            FamilyChildRepo::list_by_families(&self.db, &family_ids),
            EventRepo::list_by_person(&self.db, person_id),
            EventRepo::list_by_families(&self.db, &family_ids),
            MediaLinkRepo::list_by_person(&self.db, person_id),
            NoteRepo::list_by_entity(&self.db, tree_id, Some(person_id), None, None, None),
        )?;

        // 3. Related person rows + names, places, media.
        let mut person_ids: Vec<Uuid> = vec![person_id];
        person_ids.extend(spouses.iter().map(|s| s.person_id));
        person_ids.extend(children.iter().map(|c| c.person_id));
        person_ids.sort();
        person_ids.dedup();

        let mut events = person_events;
        events.extend(family_events);
        let mut place_ids: Vec<Uuid> = events.iter().filter_map(|e| e.place_id).collect();
        place_ids.sort();
        place_ids.dedup();
        let media_ids: Vec<Uuid> = media_links.iter().map(|l| l.media_id).collect();

        let (persons, names, places, media) = tokio::try_join!(
            PersonRepo::get_many(&self.db, &person_ids),
            PersonNameRepo::list_by_persons(&self.db, &person_ids),
            PlaceRepo::get_many(&self.db, &place_ids),
            MediaRepo::get_many(&self.db, &media_ids),
        )?;

        Ok(TreeData {
            persons,
            names,
            events,
            places,
            spouses,
            children,
            media,
            media_links,
            notes,
        })
    }

    /// Fetch all data needed to build cache entries for a tree.
    async fn fetch_tree_data(&self, tree_id: Uuid) -> Result<TreeData, OxidGeneError> {
        // Fetch all entities in parallel.
        let (persons, events, families, places, media, notes) = tokio::try_join!(
            PersonRepo::list_all(&self.db, tree_id),
            EventRepo::list_all(&self.db, tree_id),
            FamilyRepo::list_all(&self.db, tree_id),
            PlaceRepo::list_all(&self.db, tree_id),
            MediaRepo::list_all(&self.db, tree_id),
            NoteRepo::list_all(&self.db, tree_id),
        )?;

        // Get person IDs for batch name lookup.
        let person_ids: Vec<Uuid> = persons.iter().map(|p| p.id).collect();
        let names = PersonNameRepo::list_by_persons(&self.db, &person_ids).await?;

        // Get family IDs for batch spouse/child lookup.
        let family_ids: Vec<Uuid> = families.iter().map(|f| f.id).collect();
        let (spouses, children) = tokio::try_join!(
            FamilySpouseRepo::list_by_families(&self.db, &family_ids),
            FamilyChildRepo::list_by_families(&self.db, &family_ids),
        )?;

        // Get media IDs for batch media link lookup.
        let media_ids: Vec<Uuid> = media.iter().map(|m| m.id).collect();
        let media_links = MediaLinkRepo::list_by_medias(&self.db, &media_ids).await?;

        Ok(TreeData {
            persons,
            names,
            events,
            places,
            spouses,
            children,
            media,
            media_links,
            notes,
        })
    }

    /// Resolve `CachedPerson` entries for a pedigree build.
    ///
    /// When the store doesn't cache persons (desktop), `local` holds all
    /// persons of the tree, built once per pedigree build — lookups are pure
    /// map reads. Otherwise (Redis), entries are batch-read from the store
    /// and any missing ones are rebuilt from the database.
    async fn persons_for_pedigree(
        &self,
        tree_id: Uuid,
        person_ids: &[Uuid],
        local: &Option<std::collections::HashMap<Uuid, CachedPerson>>,
    ) -> Result<Vec<CachedPerson>, OxidGeneError> {
        if person_ids.is_empty() {
            return Ok(vec![]);
        }

        if let Some(map) = local {
            return Ok(person_ids
                .iter()
                .filter_map(|id| map.get(id).cloned())
                .collect());
        }

        let mut found = self.store.get_persons_batch(tree_id, person_ids).await?;
        let found_ids: std::collections::HashSet<Uuid> =
            found.iter().map(|p| p.person_id).collect();
        let missing: Vec<Uuid> = person_ids
            .iter()
            .filter(|id| !found_ids.contains(id))
            .copied()
            .collect();
        if !missing.is_empty() {
            debug!(
                "Pedigree build: {} persons not in cache, building from DB",
                missing.len()
            );
            found.extend(self.rebuild_persons(tree_id, &missing).await?);
        }
        Ok(found)
    }

    /// Build a pedigree for a root person with given ancestor and descendant
    /// depths, and store it in the cache.
    async fn rebuild_pedigree(
        &self,
        tree_id: Uuid,
        root_person_id: Uuid,
        ancestor_depth: u32,
        descendant_depth: u32,
    ) -> Result<CachedPedigree, OxidGeneError> {
        debug!(
            "Building pedigree for root {} (ancestors: {}, descendants: {})",
            root_person_id, ancestor_depth, descendant_depth
        );

        // 1. Get ancestor and descendant IDs from the closure table.
        let (ancestors, descendants) = tokio::try_join!(
            PersonAncestryRepo::ancestors(&self.db, root_person_id, Some(ancestor_depth as i32)),
            PersonAncestryRepo::descendants(
                &self.db,
                root_person_id,
                Some(descendant_depth as i32)
            ),
        )?;

        // 2. Collect all person IDs we need.
        let mut person_ids: Vec<Uuid> = Vec::new();
        person_ids.push(root_person_id);
        for a in &ancestors {
            person_ids.push(a.ancestor_id);
        }
        for d in &descendants {
            person_ids.push(d.descendant_id);
        }
        person_ids.sort();
        person_ids.dedup();

        // 3. Build a depth map: person_id -> generation (negative for ancestors,
        //    positive for descendants, 0 for root).
        let mut depth_map: std::collections::HashMap<Uuid, i32> = std::collections::HashMap::new();
        depth_map.insert(root_person_id, 0);
        for a in &ancestors {
            // Ancestors have negative generation numbers.
            let generation = -(a.depth);
            depth_map
                .entry(a.ancestor_id)
                .and_modify(|existing| {
                    // Keep the smallest absolute depth (closest to root).
                    if generation.abs() < existing.abs() {
                        *existing = generation;
                    }
                })
                .or_insert(generation);
        }
        for d in &descendants {
            // Descendants have positive generation numbers.
            let generation = d.depth;
            depth_map
                .entry(d.descendant_id)
                .and_modify(|existing| {
                    if generation.abs() < existing.abs() {
                        *existing = generation;
                    }
                })
                .or_insert(generation);
        }

        // 4. Resolve the persons in the pedigree window.
        //    On stores without a person cache (desktop), fetch the tree data
        //    once and build every person from it — all subsequent lookups
        //    (spouses, parents, outside members) are then free map reads
        //    instead of repeated database round-trips.
        let local_persons: Option<std::collections::HashMap<Uuid, CachedPerson>> =
            if self.store.caches_persons() {
                None
            } else {
                let tree_data = self.fetch_tree_data(tree_id).await?;
                Some(
                    build_all_persons(tree_id, &tree_data)
                        .into_iter()
                        .map(|p| (p.person_id, p))
                        .collect(),
                )
            };

        let cached_persons = self
            .persons_for_pedigree(tree_id, &person_ids, &local_persons)
            .await?;

        // Build a lookup map.
        let mut all_person_map: std::collections::HashMap<Uuid, &CachedPerson> =
            cached_persons.iter().map(|p| (p.person_id, p)).collect();

        // 4b. Collect spouse IDs that are not already in the pedigree window.
        //     Spouses may not be ancestors/descendants but still need nodes for display.
        let mut spouse_ids: Vec<Uuid> = Vec::new();
        for person in all_person_map.values() {
            for family_link in &person.families_as_spouse {
                if let Some(sid) = family_link.spouse_id
                    && !all_person_map.contains_key(&sid)
                    && !spouse_ids.contains(&sid)
                {
                    spouse_ids.push(sid);
                }
            }
        }
        let spouse_persons = if spouse_ids.is_empty() {
            Vec::new()
        } else {
            debug!(
                "Pedigree build: fetching {} spouses outside pedigree window",
                spouse_ids.len()
            );
            self.persons_for_pedigree(tree_id, &spouse_ids, &local_persons)
                .await?
        };
        for p in &spouse_persons {
            // Assign spouse the same generation as their partner.
            if !depth_map.contains_key(&p.person_id) {
                // Find the partner's generation from families_as_spouse.
                let partner_gen = p
                    .families_as_spouse
                    .iter()
                    .filter_map(|fl| fl.spouse_id)
                    .find_map(|sid| depth_map.get(&sid).copied())
                    .unwrap_or(0);
                depth_map.insert(p.person_id, partner_gen);
            }
            all_person_map.insert(p.person_id, p);
            person_ids.push(p.person_id);
        }

        // 5. Build pedigree nodes.
        let mut nodes = std::collections::HashMap::new();
        for &pid in &person_ids {
            if let Some(person) = all_person_map.get(&pid) {
                let generation = depth_map.get(&pid).copied().unwrap_or(0);
                // Compute sosa number for ancestors (power of 2 based on
                // generation). For generation 0 (root) sosa = 1.
                // For ancestors, sosa numbering depends on the path, which
                // requires parent-child relationship info. For now, set
                // sosa only for root.
                let sosa = if pid == root_person_id { Some(1) } else { None };
                let node = build_pedigree_node(person, generation, sosa);
                nodes.insert(pid, node);
            }
        }

        // 6. Build edges from family relationships.
        let mut edges = Vec::new();
        for person in all_person_map.values() {
            // Edges from this person as parent to their children.
            for family_link in &person.families_as_spouse {
                for &child_id in &family_link.children_ids {
                    // Only include edges where both parent and child are in our
                    // pedigree window.
                    if nodes.contains_key(&child_id) && nodes.contains_key(&person.person_id) {
                        edges.push(PedigreeEdge {
                            parent_id: person.person_id,
                            child_id,
                            family_id: family_link.family_id,
                            edge_type: oxidgene_core::enums::ChildType::Biological,
                        });
                    }
                }
            }
        }

        // De-duplicate edges (a child has two parents, each adding an edge).
        edges.sort_by(|a, b| {
            a.parent_id
                .cmp(&b.parent_id)
                .then(a.child_id.cmp(&b.child_id))
        });
        edges.dedup_by(|a, b| a.parent_id == b.parent_id && a.child_id == b.child_id);

        // 7. Collect family events from CachedPerson family links.
        let mut family_events: std::collections::HashMap<Uuid, Vec<crate::types::CachedEvent>> =
            std::collections::HashMap::new();
        for person in all_person_map.values() {
            for family_link in &person.families_as_spouse {
                if !family_link.events.is_empty() {
                    family_events
                        .entry(family_link.family_id)
                        .or_default()
                        .extend(family_link.events.iter().cloned());
                }
            }
        }
        // Deduplicate family events (both spouses may contribute the same events).
        for events in family_events.values_mut() {
            events.sort_by_key(|e| e.event_id);
            events.dedup_by_key(|e| e.event_id);
        }

        // 8. Build family membership map (spouse IDs + children IDs per family).
        //    This captures childless couples that produce no PedigreeEdge entries,
        //    and parental families needed for sibling events.
        let mut families: std::collections::HashMap<Uuid, crate::types::CachedFamily> =
            std::collections::HashMap::new();
        for person in all_person_map.values() {
            // Families where this person is a spouse.
            for family_link in &person.families_as_spouse {
                let fam = families.entry(family_link.family_id).or_insert_with(|| {
                    crate::types::CachedFamily {
                        family_id: family_link.family_id,
                        spouse_ids: Vec::new(),
                        children_ids: Vec::new(),
                        members: Vec::new(),
                    }
                });
                if !fam.spouse_ids.contains(&person.person_id) {
                    fam.spouse_ids.push(person.person_id);
                }
                // Authoritative, birth-order-sorted list for the family. Replace
                // rather than append: `all_person_map` is a HashMap, so iteration
                // order is unpredictable — if this family's `family_as_child`
                // branch below already ran for a different person and seeded
                // just their own ID, appending would leave that person stuck
                // ahead of siblings who actually precede them.
                fam.children_ids = family_link.children_ids.clone();
            }
            // Family where this person is a child.
            if let Some(child_link) = &person.family_as_child {
                let fam = families.entry(child_link.family_id).or_insert_with(|| {
                    crate::types::CachedFamily {
                        family_id: child_link.family_id,
                        spouse_ids: Vec::new(),
                        children_ids: Vec::new(),
                        members: Vec::new(),
                    }
                });
                if !fam.children_ids.contains(&person.person_id) {
                    fam.children_ids.push(person.person_id);
                }
                // Add parents as spouses if known.
                if let Some(father_id) = child_link.father_id
                    && !fam.spouse_ids.contains(&father_id)
                {
                    fam.spouse_ids.push(father_id);
                }
                if let Some(mother_id) = child_link.mother_id
                    && !fam.spouse_ids.contains(&mother_id)
                {
                    fam.spouse_ids.push(mother_id);
                }
            }
        }

        // 8a. For families created via family_as_child where the parents are outside
        //     the pedigree window, fetch a parent's CachedPerson to get the full
        //     children list (siblings). Without this, only the person themselves
        //     appears in children_ids.
        let mut parent_ids_to_fetch: Vec<Uuid> = Vec::new();
        for fam in families.values() {
            // If this family was created from family_as_child and NO parent is
            // in all_person_map, we need to fetch a parent to get full children.
            let has_parent_in_map = fam
                .spouse_ids
                .iter()
                .any(|sid| all_person_map.contains_key(sid));
            if !has_parent_in_map {
                // Pick first available parent to fetch.
                if let Some(&pid) = fam.spouse_ids.first()
                    && !parent_ids_to_fetch.contains(&pid)
                {
                    parent_ids_to_fetch.push(pid);
                }
            }
        }
        if !parent_ids_to_fetch.is_empty() {
            debug!(
                "Pedigree build: fetching {} parents outside window for sibling data",
                parent_ids_to_fetch.len()
            );
            let fetched_parents = self
                .persons_for_pedigree(tree_id, &parent_ids_to_fetch, &local_persons)
                .await?;
            // Use the fetched parents' families_as_spouse to fill in missing children.
            // A parent's children_ids is the authoritative, birth-order-sorted list
            // for the family, so replace rather than append: appending would leave
            // whichever child was pre-seeded first (the pedigree root) stuck at
            // index 0, scrambling sibling order for anyone but the eldest.
            let parent_map: std::collections::HashMap<Uuid, &CachedPerson> =
                fetched_parents.iter().map(|p| (p.person_id, p)).collect();
            for fam in families.values_mut() {
                for &sid in &fam.spouse_ids.clone() {
                    if let Some(parent) = parent_map.get(&sid)
                        && let Some(fl) = parent
                            .families_as_spouse
                            .iter()
                            .find(|fl| fl.family_id == fam.family_id)
                    {
                        fam.children_ids = fl.children_ids.clone();
                    }
                }
            }
        }

        // 8b. Fetch family members outside the pedigree window and populate
        //     CachedFamily.members with their minimal info (for event panel).
        let mut outside_member_ids: Vec<Uuid> = Vec::new();
        for fam in families.values() {
            for &cid in &fam.children_ids {
                if !nodes.contains_key(&cid) && !outside_member_ids.contains(&cid) {
                    outside_member_ids.push(cid);
                }
            }
        }
        if !outside_member_ids.is_empty() {
            debug!(
                "Pedigree build: fetching {} family members outside pedigree window",
                outside_member_ids.len()
            );
            let all_outside = self
                .persons_for_pedigree(tree_id, &outside_member_ids, &local_persons)
                .await?;
            // Build a lookup for outside members.
            let outside_map: std::collections::HashMap<Uuid, &CachedPerson> =
                all_outside.iter().map(|p| (p.person_id, p)).collect();
            // Populate CachedFamily.members for each family.
            for fam in families.values_mut() {
                for &cid in &fam.children_ids {
                    if let Some(person) = outside_map.get(&cid) {
                        let display_name = person
                            .primary_name
                            .as_ref()
                            .map(|n| n.display_name.clone())
                            .unwrap_or_default();
                        let birth_year = person.birth.as_ref().and_then(builder::extract_year);
                        let death_year = person.death.as_ref().and_then(builder::extract_year);
                        fam.members.push(crate::types::CachedFamilyMember {
                            person_id: cid,
                            display_name,
                            sex: person.sex,
                            birth_year,
                            death_year,
                        });
                    }
                }
            }

            // 8c. Merge family membership (spouse + children linkage) for the
            //     members fetched above purely for display (e.g. a sibling
            //     shown next to the root, or a boundary descendant) — not
            //     full nodes, just enough spouse/children IDs so the "+"
            //     hidden-relations indicator on their card is accurate. We
            //     don't recurse further: newly-referenced spouses/children
            //     are linked by ID only, not fetched themselves.
            for person in outside_map.values() {
                for family_link in &person.families_as_spouse {
                    let fam = families.entry(family_link.family_id).or_insert_with(|| {
                        crate::types::CachedFamily {
                            family_id: family_link.family_id,
                            spouse_ids: Vec::new(),
                            children_ids: Vec::new(),
                            members: Vec::new(),
                        }
                    });
                    if !fam.spouse_ids.contains(&person.person_id) {
                        fam.spouse_ids.push(person.person_id);
                    }
                    if let Some(sid) = family_link.spouse_id
                        && !fam.spouse_ids.contains(&sid)
                    {
                        fam.spouse_ids.push(sid);
                    }
                    // Authoritative, birth-order-sorted list for the family — replace
                    // rather than append (see 8a for why appending scrambles order).
                    fam.children_ids = family_link.children_ids.clone();
                }
                if let Some(child_link) = &person.family_as_child {
                    let fam = families.entry(child_link.family_id).or_insert_with(|| {
                        crate::types::CachedFamily {
                            family_id: child_link.family_id,
                            spouse_ids: Vec::new(),
                            children_ids: Vec::new(),
                            members: Vec::new(),
                        }
                    });
                    if !fam.children_ids.contains(&person.person_id) {
                        fam.children_ids.push(person.person_id);
                    }
                    if let Some(father_id) = child_link.father_id
                        && !fam.spouse_ids.contains(&father_id)
                    {
                        fam.spouse_ids.push(father_id);
                    }
                    if let Some(mother_id) = child_link.mother_id
                        && !fam.spouse_ids.contains(&mother_id)
                    {
                        fam.spouse_ids.push(mother_id);
                    }
                }
            }
        }

        // 9. Build and store the pedigree.
        let pedigree = CachedPedigree {
            tree_id,
            root_person_id,
            persons: nodes,
            edges,
            family_events,
            families,
            ancestor_depth_loaded: ancestor_depth,
            descendant_depth_loaded: descendant_depth,
            cached_at: chrono::Utc::now(),
        };

        self.store.set_pedigree(&pedigree).await?;

        debug!(
            "Built pedigree with {} nodes, {} edges, {} families for root {}",
            pedigree.persons.len(),
            pedigree.edges.len(),
            pedigree.families.len(),
            root_person_id
        );

        Ok(pedigree)
    }
}

#[cfg(test)]
mod tests {
    // CacheService integration tests require a database connection.
    // They will be added in a later sprint with test fixtures.
    //
    // The service is a thin orchestrator — its correctness depends on:
    //   - builder.rs (tested with unit tests)
    //   - invalidation.rs (tested with integration tests)
    //   - store implementations (tested with unit tests)
    //   - DB repos (tested separately)
}
