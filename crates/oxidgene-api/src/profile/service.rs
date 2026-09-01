//! Person-projection orchestration.
//!
//! [`ProfileService`] owns the read side of the domain: it materializes the
//! denormalized person projections into `person_denorm`, keeps
//! `person_search_fts` in step, and assembles pedigrees on demand by joining
//! the family links against those projections.
//!
//! There is no cache. Every read is a database read, and every mutation
//! rewrites the bounded set of projections it invalidates, so a projection is
//! never stale, survives a restart, and behaves identically on desktop
//! (SQLite) and web (PostgreSQL).

use std::collections::{HashMap, HashSet};

use oxidgene_core::error::OxidGeneError;
use oxidgene_core::projection::{
    Pedigree, PedigreeDelta, PedigreeDirection, PedigreeEdge, PedigreeFamily, PedigreeFamilyMember,
    PedigreeNode, PersonProfile, SearchResult,
};
use oxidgene_db::repo::{
    AncestryRepo, CitationRepo, EventRepo, FamilyChildRepo, FamilyRepo, FamilySpouseRepo,
    MediaLinkRepo, MediaRepo, NoteRepo, PersonDenormRepo, PersonNameRepo, PersonRepo,
    PersonSearchFilters, PersonSearchRepo, PersonSearchSort, PlaceRepo, VignetteRepo,
};
use sea_orm::{ConnectionTrait, DatabaseConnection};
use tracing::{debug, info, instrument};
use uuid::Uuid;

use super::builder::{
    self, TreeData, build_all_persons, build_db_search_entry, build_pedigree_node,
    search_entry_from_db,
};
use super::invalidation;

/// Above this many affected persons, rebuild from a single whole-tree fetch
/// instead of running targeted per-person queries.
///
/// Affected sets from a normal mutation are 2–10 persons, where targeted
/// queries win. Bulk paths (GEDCOM import, tree-wide fixes) blow past this,
/// where one wide read beats N narrow ones.
const FULL_FETCH_THRESHOLD: usize = 50;

/// Orchestrates the denormalized person projections and pedigree assembly.
///
/// Stored in the API's `AppState` as an `Arc<ProfileService>`; all methods
/// take `&self` so it can be shared across request handlers.
#[derive(Debug)]
pub struct ProfileService {
    db: DatabaseConnection,
}

impl ProfileService {
    /// Create a new profile service.
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    // ── Full tree rebuild ────────────────────────────────────────────────

    /// Rebuild every projection of a tree, plus its search rows.
    ///
    /// Used after a GEDCOM import, and lazily the first time a tree is read
    /// after the `person_denorm` migration.
    #[instrument(skip_all)]
    pub async fn rebuild_tree_full(
        &self,
        conn: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<usize, OxidGeneError> {
        info!("Starting full projection rebuild");

        let tree_data = self.fetch_tree_data(conn, tree_id).await?;
        let persons = build_all_persons(tree_id, &tree_data);
        debug!(count = persons.len(), "Built projections");

        PersonDenormRepo::replace_tree(conn, tree_id, &persons).await?;

        let search_entries: Vec<_> = persons.iter().map(build_db_search_entry).collect();
        PersonSearchRepo::replace_tree(conn, tree_id, &search_entries).await?;

        info!(count = persons.len(), "Completed full projection rebuild");
        Ok(persons.len())
    }

    /// Materialize a tree's projections if they have never been built.
    ///
    /// Covers the cold path for trees that predate the `person_denorm`
    /// migration, and any tree whose search rows were dropped independently.
    async fn ensure_materialized(
        &self,
        conn: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<(), OxidGeneError> {
        // `count_current` and not `count_tree`: a tree whose rows an older
        // build wrote is as unusable as one nobody has built, and answering
        // "already materialized" for it is what let a projection change stay
        // invisible until somebody happened to re-import.
        let denorm_rows = PersonDenormRepo::count_current(conn, tree_id).await?;
        let search_rows = PersonSearchRepo::count_tree(conn, tree_id).await?;
        if denorm_rows > 0 && search_rows > 0 {
            return Ok(());
        }

        debug!(
            denorm_rows,
            search_rows, "Tree projections are not materialized"
        );
        self.rebuild_tree_full(conn, tree_id).await?;
        Ok(())
    }

    // ── Person projections ───────────────────────────────────────────────

    /// Read a person's projection, building it on demand if absent.
    #[instrument(skip_all)]
    pub async fn get_or_build_person(
        &self,
        conn: &impl ConnectionTrait,
        tree_id: Uuid,
        person_id: Uuid,
    ) -> Result<PersonProfile, OxidGeneError> {
        if let Some(stored) = PersonDenormRepo::get(conn, tree_id, person_id).await? {
            return Ok(stored);
        }

        debug!("Person projection is not materialized");
        self.rebuild_person(conn, tree_id, person_id).await
    }

    /// Rebuild one person's projection and its search row.
    #[instrument(skip_all)]
    pub async fn rebuild_person(
        &self,
        conn: &impl ConnectionTrait,
        tree_id: Uuid,
        person_id: Uuid,
    ) -> Result<PersonProfile, OxidGeneError> {
        let built = self.build_single_person(conn, tree_id, person_id).await?;
        PersonDenormRepo::upsert(conn, std::slice::from_ref(&built)).await?;
        PersonSearchRepo::upsert(conn, &[build_db_search_entry(&built)]).await?;
        Ok(built)
    }

    /// Rebuild the projections of a bounded set of persons.
    ///
    /// Does not touch the search rows — callers that need them refreshed go
    /// through [`Self::rebuild_affected`].
    #[instrument(skip_all, fields(count = person_ids.len()))]
    pub async fn rebuild_persons(
        &self,
        conn: &impl ConnectionTrait,
        tree_id: Uuid,
        person_ids: &[Uuid],
    ) -> Result<Vec<PersonProfile>, OxidGeneError> {
        if person_ids.is_empty() {
            return Ok(vec![]);
        }

        let built: Vec<PersonProfile> = if person_ids.len() >= FULL_FETCH_THRESHOLD {
            let wanted: HashSet<Uuid> = person_ids.iter().copied().collect();
            let tree_data = self.fetch_tree_data(conn, tree_id).await?;
            build_all_persons(tree_id, &tree_data)
                .into_iter()
                .filter(|p| wanted.contains(&p.person_id))
                .collect()
        } else {
            let mut out = Vec::with_capacity(person_ids.len());
            for &pid in person_ids {
                out.push(self.build_single_person(conn, tree_id, pid).await?);
            }
            out
        };

        PersonDenormRepo::upsert(conn, &built).await?;
        debug!(count = built.len(), "Rebuilt projections");
        Ok(built)
    }

    /// Read every projection of a tree, materializing them if needed.
    pub async fn get_all_persons(
        &self,
        conn: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<PersonProfile>, OxidGeneError> {
        self.ensure_materialized(conn, tree_id).await?;
        PersonDenormRepo::list_tree(conn, tree_id).await
    }

    // ── Pedigree ─────────────────────────────────────────────────────────

    /// Assemble a windowed pedigree for a root person.
    ///
    /// Built fresh on every call by walking the family links and joining the
    /// reached persons against `person_denorm` — there is nothing to cache and
    /// nothing to invalidate.
    #[instrument(skip_all)]
    pub async fn get_or_build_pedigree(
        &self,
        tree_id: Uuid,
        root_person_id: Uuid,
        ancestor_depth: u32,
        descendant_depth: u32,
    ) -> Result<Pedigree, OxidGeneError> {
        let conn = &self.db;
        self.ensure_materialized(conn, tree_id).await?;
        self.build_pedigree(
            conn,
            tree_id,
            root_person_id,
            ancestor_depth,
            descendant_depth,
        )
        .await
    }

    /// Compute the nodes and edges a pedigree gains when expanded from
    /// `from_depth` to `to_depth` in one direction.
    ///
    /// `other_depth` is the depth already loaded in the *opposite* direction;
    /// pass it so the reported `*_depth_loaded` values match what the caller
    /// actually holds. Both windows are assembled and diffed — cheap now that
    /// a pedigree is a closure-table read plus a projection batch read.
    #[instrument(skip_all)]
    #[allow(clippy::too_many_arguments)]
    pub async fn expand_pedigree(
        &self,
        tree_id: Uuid,
        root_person_id: Uuid,
        direction: PedigreeDirection,
        from_depth: u32,
        to_depth: u32,
        other_depth: u32,
    ) -> Result<PedigreeDelta, OxidGeneError> {
        let conn = &self.db;
        self.ensure_materialized(conn, tree_id).await?;

        let (before, after) = match direction {
            PedigreeDirection::Ancestors => ((from_depth, other_depth), (to_depth, other_depth)),
            PedigreeDirection::Descendants => ((other_depth, from_depth), (other_depth, to_depth)),
        };

        let existing = self
            .build_pedigree(conn, tree_id, root_person_id, before.0, before.1)
            .await?;
        let expanded = self
            .build_pedigree(conn, tree_id, root_person_id, after.0, after.1)
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
    /// the database before rows are returned.
    #[allow(clippy::too_many_arguments)]
    pub async fn search_filtered(
        &self,
        tree_id: Uuid,
        query: &str,
        filters: &PersonSearchFilters,
        sort: PersonSearchSort,
        limit: usize,
        offset: usize,
    ) -> Result<SearchResult, OxidGeneError> {
        let conn = &self.db;
        self.ensure_materialized(conn, tree_id).await?;
        let page = PersonSearchRepo::search_filtered(
            conn,
            tree_id,
            query,
            filters,
            sort,
            limit as u64,
            offset as u64,
        )
        .await?;
        Ok(SearchResult {
            entries: page.entries.into_iter().map(search_entry_from_db).collect(),
            total_count: page.total_count as usize,
        })
    }

    // ── Invalidation ─────────────────────────────────────────────────────

    /// Refresh projections after a person mutation (edit person, edit name,
    /// add/edit/delete an event on a person).
    ///
    /// This is the primary entry point: it computes the affected set — the
    /// person plus everyone whose projection embeds their name — and rewrites
    /// those rows.
    #[instrument(skip_all)]
    pub async fn invalidate_for_person(
        &self,
        conn: &impl ConnectionTrait,
        tree_id: Uuid,
        person_id: Uuid,
    ) -> Result<(), OxidGeneError> {
        let affected = invalidation::affected_persons(conn, person_id).await?;
        debug!(
            count = affected.len(),
            "Refreshing projections after person mutation"
        );
        self.rebuild_affected(conn, tree_id, &affected).await
    }

    /// Refresh projections after a family event mutation (marriage, divorce…).
    #[instrument(skip_all)]
    pub async fn invalidate_for_family_event(
        &self,
        conn: &impl ConnectionTrait,
        tree_id: Uuid,
        family_id: Uuid,
    ) -> Result<(), OxidGeneError> {
        let affected = invalidation::affected_persons_for_family(conn, family_id).await?;
        debug!(
            count = affected.len(),
            "Refreshing projections after family event mutation"
        );
        self.rebuild_affected(conn, tree_id, &affected).await
    }

    /// Refresh projections after a spouse is added to or removed from a family.
    #[instrument(skip_all)]
    pub async fn invalidate_for_family_spouse_change(
        &self,
        conn: &impl ConnectionTrait,
        tree_id: Uuid,
        family_id: Uuid,
        changed_person_id: Uuid,
    ) -> Result<(), OxidGeneError> {
        let affected = invalidation::affected_persons_for_family_spouse_change(
            conn,
            family_id,
            changed_person_id,
        )
        .await?;
        debug!(
            count = affected.len(),
            "Refreshing projections after spouse mutation"
        );
        self.rebuild_affected(conn, tree_id, &affected).await
    }

    /// Refresh projections after a child is added to or removed from a family.
    #[instrument(skip_all)]
    pub async fn invalidate_for_family_child_change(
        &self,
        conn: &impl ConnectionTrait,
        tree_id: Uuid,
        family_id: Uuid,
        child_person_id: Uuid,
    ) -> Result<(), OxidGeneError> {
        let affected = invalidation::affected_persons_for_family_child_change(
            conn,
            family_id,
            child_person_id,
        )
        .await?;
        debug!(
            count = affected.len(),
            "Refreshing projections after child mutation"
        );
        self.rebuild_affected(conn, tree_id, &affected).await
    }

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

    /// Rewrite the projections and search rows of an affected set.
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
        let rebuilt = self.rebuild_persons(conn, tree_id, affected).await?;
        let entries: Vec<_> = rebuilt.iter().map(build_db_search_entry).collect();
        PersonSearchRepo::upsert(conn, &entries).await?;
        Ok(())
    }

    /// Build one person's projection with targeted queries — the person,
    /// their families, their relatives' names, and the entities attached to
    /// them. No full-tree fetch.
    async fn build_single_person(
        &self,
        conn: &impl ConnectionTrait,
        tree_id: Uuid,
        person_id: Uuid,
    ) -> Result<PersonProfile, OxidGeneError> {
        let data = self.fetch_person_data(conn, tree_id, person_id).await?;
        builder::build_person(tree_id, person_id, &data).ok_or(OxidGeneError::NotFound {
            entity: "Person",
            id: person_id,
        })
    }

    /// Fetch only what one projection needs: the person, their family
    /// memberships, all members of those families (for spouse / parent /
    /// child denormalization), their events + places, media and notes.
    async fn fetch_person_data(
        &self,
        conn: &impl ConnectionTrait,
        tree_id: Uuid,
        person_id: Uuid,
    ) -> Result<TreeData, OxidGeneError> {
        // 1. Family memberships of the person.
        let (as_spouse, as_child) = tokio::try_join!(
            FamilySpouseRepo::list_by_person(conn, person_id),
            FamilyChildRepo::list_by_person(conn, person_id),
        )?;
        let mut family_ids: Vec<Uuid> = as_spouse
            .iter()
            .map(|s| s.family_id)
            .chain(as_child.iter().map(|c| c.family_id))
            .collect();
        family_ids.sort();
        family_ids.dedup();

        // 2. All members of those families, plus attached entities.
        let (spouses, children, person_events, family_events, media_links, citations, notes) = tokio::try_join!(
            FamilySpouseRepo::list_by_families(conn, &family_ids),
            FamilyChildRepo::list_by_families(conn, &family_ids),
            EventRepo::list_by_person(conn, person_id),
            EventRepo::list_by_families(conn, &family_ids),
            MediaLinkRepo::list_by_person(conn, person_id),
            CitationRepo::list_by_person(conn, person_id),
            NoteRepo::list_by_entity(conn, tree_id, Some(person_id), None, None, None, None),
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
            PersonRepo::get_many(conn, &person_ids),
            PersonNameRepo::list_by_persons(conn, &person_ids),
            PlaceRepo::get_many(conn, &place_ids),
            MediaRepo::get_many(conn, &media_ids),
        )?;

        // The portrait crop, and the scan it sits on. Fetched after the
        // person rather than alongside, because which crop to fetch is
        // written on the person; and appended to `media` because the
        // containing scan need not be one of this person's own links — a face
        // in somebody else's group photograph is still their portrait.
        let mut media = media;
        let mut portrait_vignettes = Vec::new();
        if let Some(vignette_id) = persons
            .iter()
            .find(|p| p.id == person_id)
            .and_then(|p| p.portrait_vignette_id)
            && let Ok(vignette) = VignetteRepo::get(conn, vignette_id).await
        {
            if !media.iter().any(|m| m.id == vignette.media_id) {
                media.extend(MediaRepo::get_many(conn, &[vignette.media_id]).await?);
            }
            portrait_vignettes.push(vignette);
        }

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
            citations,
            notes,
        })
    }

    /// Fetch everything needed to build every projection of a tree.
    async fn fetch_tree_data(
        &self,
        conn: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<TreeData, OxidGeneError> {
        let (persons, events, families, places, media, citations, notes) = tokio::try_join!(
            PersonRepo::list_all(conn, tree_id),
            EventRepo::list_all(conn, tree_id),
            FamilyRepo::list_all(conn, tree_id),
            PlaceRepo::list_all(conn, tree_id),
            MediaRepo::list_all(conn, tree_id),
            CitationRepo::list_all(conn, tree_id),
            NoteRepo::list_all(conn, tree_id),
        )?;

        let person_ids: Vec<Uuid> = persons.iter().map(|p| p.id).collect();
        let names = PersonNameRepo::list_by_persons(conn, &person_ids).await?;

        let family_ids: Vec<Uuid> = families.iter().map(|f| f.id).collect();
        let (spouses, children) = tokio::try_join!(
            FamilySpouseRepo::list_by_families(conn, &family_ids),
            FamilyChildRepo::list_by_families(conn, &family_ids),
        )?;

        let media_ids: Vec<Uuid> = media.iter().map(|m| m.id).collect();
        let media_links = MediaLinkRepo::list_by_medias(conn, &media_ids).await?;

        // Only the crops that are somebody's portrait: every vignette in the
        // tree would be a large slice to carry for a field usually null.
        let portrait_ids: Vec<Uuid> = persons
            .iter()
            .filter_map(|p| p.portrait_vignette_id)
            .collect();
        let portrait_vignettes = VignetteRepo::get_many(conn, &portrait_ids).await?;

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
            citations,
            notes,
        })
    }

    /// Read the projections for a set of persons, rebuilding any that are
    /// missing (a person created before the tree was materialized).
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
            found.extend(self.rebuild_persons(conn, tree_id, &missing).await?);
        }
        Ok(found)
    }

    /// Assemble a pedigree window for a root person from the closure table
    /// and the stored projections.
    #[instrument(
        name = "pedigree.build",
        skip_all,
        fields(ancestor_depth, descendant_depth)
    )]
    async fn build_pedigree(
        &self,
        conn: &impl ConnectionTrait,
        tree_id: Uuid,
        root_person_id: Uuid,
        ancestor_depth: u32,
        descendant_depth: u32,
    ) -> Result<Pedigree, OxidGeneError> {
        debug!(ancestor_depth, descendant_depth, "Building pedigree");

        // 1. Walk the family links for ancestor and descendant IDs.
        let (ancestors, descendants) = tokio::try_join!(
            AncestryRepo::ancestors(conn, root_person_id, Some(ancestor_depth as i32)),
            AncestryRepo::descendants(conn, root_person_id, Some(descendant_depth as i32)),
        )?;

        // 2. Collect all person IDs we need.
        let mut person_ids: Vec<Uuid> = vec![root_person_id];
        person_ids.extend(ancestors.iter().map(|a| a.person_id));
        person_ids.extend(descendants.iter().map(|d| d.person_id));
        person_ids.sort();
        person_ids.dedup();

        // 3. Build a depth map: person_id -> generation (negative for
        //    ancestors, positive for descendants, 0 for root).
        let mut depth_map: HashMap<Uuid, i32> = HashMap::new();
        depth_map.insert(root_person_id, 0);
        // The walk already reports each person at their shortest distance, but
        // someone can be both an ancestor and a descendant (implex), so the
        // closest-to-root rule still has to arbitrate between the two lists.
        for a in &ancestors {
            let generation = -(a.depth);
            depth_map
                .entry(a.person_id)
                .and_modify(|existing| {
                    // Keep the smallest absolute depth (closest to root).
                    if generation.abs() < existing.abs() {
                        *existing = generation;
                    }
                })
                .or_insert(generation);
        }
        for d in &descendants {
            let generation = d.depth;
            depth_map
                .entry(d.person_id)
                .and_modify(|existing| {
                    if generation.abs() < existing.abs() {
                        *existing = generation;
                    }
                })
                .or_insert(generation);
        }

        // 4. Resolve the projections in the pedigree window.
        let window_persons = self.projections_for(conn, tree_id, &person_ids).await?;
        let mut all_person_map: HashMap<Uuid, &PersonProfile> =
            window_persons.iter().map(|p| (p.person_id, p)).collect();

        // 4b. Spouses may be neither ancestors nor descendants but still need
        //     a node for display.
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
            self.projections_for(conn, tree_id, &spouse_ids).await?
        };
        for p in &spouse_persons {
            // Assign the spouse the same generation as their partner.
            if !depth_map.contains_key(&p.person_id) {
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

        // 5. Build pedigree nodes. Sosa numbering depends on the path from the
        //    root, which the closure table alone does not give us, so only the
        //    root carries one here; the UI derives the rest from the layout.
        let mut nodes: HashMap<Uuid, PedigreeNode> = HashMap::new();
        for &pid in &person_ids {
            if let Some(person) = all_person_map.get(&pid) {
                let generation = depth_map.get(&pid).copied().unwrap_or(0);
                let sosa = if pid == root_person_id { Some(1) } else { None };
                nodes.insert(pid, build_pedigree_node(person, generation, sosa));
            }
        }

        // 6. Build edges from family relationships.
        let mut edges = Vec::new();
        for person in all_person_map.values() {
            for family_link in &person.families_as_spouse {
                for &child_id in &family_link.children_ids {
                    // Only keep edges whose parent and child are both in the
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

        // 7. Collect family events from the projections' family links.
        let mut family_events: HashMap<Uuid, Vec<oxidgene_core::projection::ProfileEvent>> =
            HashMap::new();
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
        // Deduplicate (both spouses contribute the same events).
        for events in family_events.values_mut() {
            events.sort_by_key(|e| e.event_id);
            events.dedup_by_key(|e| e.event_id);
        }

        // 8. Build the family membership map (spouse + children IDs per
        //    family). This captures childless couples, which produce no
        //    PedigreeEdge, and parental families needed for sibling events.
        let mut families: HashMap<Uuid, PedigreeFamily> = HashMap::new();
        for person in all_person_map.values() {
            for family_link in &person.families_as_spouse {
                let fam = families
                    .entry(family_link.family_id)
                    .or_insert_with(|| empty_family(family_link.family_id));
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
            if let Some(child_link) = &person.family_as_child {
                let fam = families
                    .entry(child_link.family_id)
                    .or_insert_with(|| empty_family(child_link.family_id));
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

        // 8a. For families reached via `family_as_child` whose parents are all
        //     outside the window, fetch one parent to recover the full sibling
        //     list — otherwise only the person themselves appears.
        let mut parent_ids_to_fetch: Vec<Uuid> = Vec::new();
        for fam in families.values() {
            let has_parent_in_map = fam
                .spouse_ids
                .iter()
                .any(|sid| all_person_map.contains_key(sid));
            if !has_parent_in_map
                && let Some(&pid) = fam.spouse_ids.first()
                && !parent_ids_to_fetch.contains(&pid)
            {
                parent_ids_to_fetch.push(pid);
            }
        }
        if !parent_ids_to_fetch.is_empty() {
            debug!(
                "Pedigree build: fetching {} parents outside window for sibling data",
                parent_ids_to_fetch.len()
            );
            let fetched_parents = self
                .projections_for(conn, tree_id, &parent_ids_to_fetch)
                .await?;
            // A parent's children_ids is the authoritative, birth-order-sorted
            // list for the family, so replace rather than append: appending
            // would leave whichever child was pre-seeded first (the pedigree
            // root) stuck at index 0, scrambling sibling order for anyone but
            // the eldest.
            let parent_map: HashMap<Uuid, &PersonProfile> =
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

        // 8b. Fetch family members outside the window and record their minimal
        //     info, so the event panel can show them.
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
                .projections_for(conn, tree_id, &outside_member_ids)
                .await?;
            let outside_map: HashMap<Uuid, &PersonProfile> =
                all_outside.iter().map(|p| (p.person_id, p)).collect();

            for fam in families.values_mut() {
                for &cid in &fam.children_ids {
                    if let Some(person) = outside_map.get(&cid) {
                        fam.members.push(PedigreeFamilyMember {
                            person_id: cid,
                            display_name: person
                                .primary_name
                                .as_ref()
                                .map(|n| n.display_name.clone())
                                .unwrap_or_default(),
                            given_names: person
                                .primary_name
                                .as_ref()
                                .and_then(|n| n.given_names.clone()),
                            surname: person.primary_name.as_ref().and_then(|n| n.surname.clone()),
                            sex: person.sex,
                            birth: person.birth_or_baptism().cloned(),
                            death: person.death_or_burial().cloned(),
                        });
                    }
                }
            }

            // 8c. Merge family membership for the members fetched above purely
            //     for display (a sibling next to the root, a boundary
            //     descendant) — not full nodes, just enough spouse/children IDs
            //     for the "+" hidden-relations indicator on their card to be
            //     accurate. We don't recurse: newly-referenced spouses and
            //     children are linked by ID only, never fetched.
            for person in outside_map.values() {
                for family_link in &person.families_as_spouse {
                    let fam = families
                        .entry(family_link.family_id)
                        .or_insert_with(|| empty_family(family_link.family_id));
                    if !fam.spouse_ids.contains(&person.person_id) {
                        fam.spouse_ids.push(person.person_id);
                    }
                    if let Some(sid) = family_link.spouse_id
                        && !fam.spouse_ids.contains(&sid)
                    {
                        fam.spouse_ids.push(sid);
                    }
                    // Authoritative, birth-order-sorted list — replace rather
                    // than append (see 8a for why appending scrambles order).
                    fam.children_ids = family_link.children_ids.clone();
                }
                if let Some(child_link) = &person.family_as_child {
                    let fam = families
                        .entry(child_link.family_id)
                        .or_insert_with(|| empty_family(child_link.family_id));
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

        let pedigree = Pedigree {
            tree_id,
            root_person_id,
            persons: nodes,
            edges,
            family_events,
            families,
            ancestor_depth_loaded: ancestor_depth,
            descendant_depth_loaded: descendant_depth,
            built_at: chrono::Utc::now(),
        };

        debug!(
            nodes = pedigree.persons.len(),
            edges = pedigree.edges.len(),
            families = pedigree.families.len(),
            "Built pedigree"
        );

        Ok(pedigree)
    }
}

/// An empty family unit, filled in as members are discovered.
fn empty_family(family_id: Uuid) -> PedigreeFamily {
    PedigreeFamily {
        family_id,
        spouse_ids: Vec::new(),
        children_ids: Vec::new(),
        members: Vec::new(),
    }
}
