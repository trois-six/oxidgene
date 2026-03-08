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
    PersonAncestryRepo, PersonNameRepo, PersonRepo, PlaceRepo, TreeRepo,
};
use oxidgene_db::sea_orm::DatabaseConnection;
use tracing::{debug, info, instrument, warn};
use uuid::Uuid;

use crate::builder::{self, TreeData, build_all_persons, build_pedigree_node, build_search_index};
use crate::invalidation;
use crate::store::CacheStore;
use crate::types::{
    CachedPedigree, CachedPerson, CachedSearchIndex, PedigreeDelta, PedigreeDirection,
    PedigreeEdge, PedigreeNode, SearchResult,
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

        // 3. Store persons in batch.
        self.store.set_persons_batch(&cached_persons).await?;

        // 4. Build and store the search index.
        let search_index = build_search_index(tree_id, &cached_persons);
        self.store.set_search_index(&search_index).await?;
        debug!(
            "Built search index with {} entries for tree {}",
            search_index.entries.len(),
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

    /// Rebuild the cache for a single person.
    #[instrument(skip(self), fields(tree_id = %tree_id, person_id = %person_id))]
    pub async fn rebuild_person(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
    ) -> Result<CachedPerson, OxidGeneError> {
        let cached = self.build_single_person(tree_id, person_id).await?;
        self.store.set_person(&cached).await?;
        Ok(cached)
    }

    /// Rebuild the cache for multiple persons (used after invalidation).
    #[instrument(skip(self, person_ids), fields(tree_id = %tree_id, count = person_ids.len()))]
    pub async fn rebuild_persons(
        &self,
        tree_id: Uuid,
        person_ids: &[Uuid],
    ) -> Result<Vec<CachedPerson>, OxidGeneError> {
        if person_ids.is_empty() {
            return Ok(vec![]);
        }

        // For efficiency, fetch all tree data once and rebuild from it.
        let tree_data = self.fetch_tree_data(tree_id).await?;
        let all_cached = build_all_persons(tree_id, &tree_data);

        // Filter to only the requested persons and store them.
        let requested: Vec<CachedPerson> = all_cached
            .into_iter()
            .filter(|p| person_ids.contains(&p.person_id))
            .collect();

        self.store.set_persons_batch(&requested).await?;

        debug!(
            "Rebuilt {} person caches for tree {}",
            requested.len(),
            tree_id
        );

        Ok(requested)
    }

    /// Get all cached persons for a tree. Triggers a full rebuild if the cache
    /// is empty.
    pub async fn get_all_persons(&self, tree_id: Uuid) -> Result<Vec<CachedPerson>, OxidGeneError> {
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

    /// Search persons in a tree using the cached search index.
    ///
    /// Falls back to building the index on-demand if not cached.
    #[instrument(skip(self), fields(tree_id = %tree_id, query = %query))]
    pub async fn search(
        &self,
        tree_id: Uuid,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<SearchResult, OxidGeneError> {
        let index = self.get_or_build_search_index(tree_id).await?;
        Ok(builder::search_index(&index, query, limit, offset))
    }

    /// Get the search index, building on-demand if not cached.
    async fn get_or_build_search_index(
        &self,
        tree_id: Uuid,
    ) -> Result<CachedSearchIndex, OxidGeneError> {
        if let Some(index) = self.store.get_search_index(tree_id).await? {
            return Ok(index);
        }

        debug!("No search index for tree {}, building from DB", tree_id);

        // Need all persons to build the index.
        let persons = self.get_all_persons(tree_id).await?;
        let index = build_search_index(tree_id, &persons);
        self.store.set_search_index(&index).await?;
        Ok(index)
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

        // Remove the deleted person from cache.
        self.store.delete_person(tree_id, person_id).await?;

        // Rebuild remaining affected persons (excluding the deleted one).
        let remaining: Vec<Uuid> = affected.into_iter().filter(|&id| id != person_id).collect();
        if !remaining.is_empty() {
            self.rebuild_persons(tree_id, &remaining).await?;
        }

        // Update search index.
        self.rebuild_search_index(tree_id).await?;

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

    /// Rebuild the affected persons' caches, search index, and pedigrees.
    async fn rebuild_affected(
        &self,
        tree_id: Uuid,
        affected: &[Uuid],
    ) -> Result<(), OxidGeneError> {
        // 1. Rebuild person caches.
        self.rebuild_persons(tree_id, affected).await?;

        // 2. Rebuild the search index (relatively cheap for bounded sets,
        //    but we rebuild the full index for correctness).
        self.rebuild_search_index(tree_id).await?;

        // 3. Patch pedigrees — for now, delete all pedigrees for the tree
        //    so they're rebuilt on next access. A more sophisticated approach
        //    would patch only the affected nodes, but that's an optimization
        //    for a later sprint.
        self.store.delete_all_pedigrees(tree_id).await?;

        Ok(())
    }

    /// Rebuild the search index for a tree from all cached persons.
    async fn rebuild_search_index(&self, tree_id: Uuid) -> Result<(), OxidGeneError> {
        let all_persons = self.store.get_all_persons(tree_id).await?;
        if all_persons.is_empty() {
            // If cache is empty, skip — the index will be built on next access.
            return Ok(());
        }
        let index = build_search_index(tree_id, &all_persons);
        self.store.set_search_index(&index).await
    }

    /// Build a single person's cache entry from the database.
    ///
    /// This fetches the full tree data and extracts just the one person.
    /// For single-person rebuilds this is slightly wasteful, but it reuses
    /// the battle-tested `build_all_persons` pipeline. For bulk rebuilds,
    /// `rebuild_persons` is more efficient.
    async fn build_single_person(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
    ) -> Result<CachedPerson, OxidGeneError> {
        let tree_data = self.fetch_tree_data(tree_id).await?;
        let all_cached = build_all_persons(tree_id, &tree_data);

        all_cached
            .into_iter()
            .find(|p| p.person_id == person_id)
            .ok_or(OxidGeneError::NotFound {
                entity: "Person",
                id: person_id,
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

        // 4. Get cached persons (or build them if not cached).
        let cached_persons = self.store.get_persons_batch(tree_id, &person_ids).await?;

        // Build a lookup map.
        let person_map: std::collections::HashMap<Uuid, &CachedPerson> =
            cached_persons.iter().map(|p| (p.person_id, p)).collect();

        // If some persons are not in cache, we need to build them.
        let missing: Vec<Uuid> = person_ids
            .iter()
            .filter(|id| !person_map.contains_key(id))
            .copied()
            .collect();

        let mut extra_persons = Vec::new();
        if !missing.is_empty() {
            warn!(
                "Pedigree build: {} persons not in cache, building from DB",
                missing.len()
            );
            extra_persons = self.rebuild_persons(tree_id, &missing).await?;
        }

        // Merge into a single lookup.
        let mut all_person_map: std::collections::HashMap<Uuid, &CachedPerson> = person_map;
        for p in &extra_persons {
            all_person_map.insert(p.person_id, p);
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

        // 7. Build and store the pedigree.
        let pedigree = CachedPedigree {
            tree_id,
            root_person_id,
            persons: nodes,
            edges,
            ancestor_depth_loaded: ancestor_depth,
            descendant_depth_loaded: descendant_depth,
            cached_at: chrono::Utc::now(),
        };

        self.store.set_pedigree(&pedigree).await?;

        debug!(
            "Built pedigree with {} nodes and {} edges for root {}",
            pedigree.persons.len(),
            pedigree.edges.len(),
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
