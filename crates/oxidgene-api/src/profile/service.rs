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

use oxidgene_core::collections::sorted_unique;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::projection::{
    Pedigree, PedigreeDelta, PedigreeDirection, PedigreeEdge, PedigreeFamily, PedigreeNode,
    PersonProfile, SearchEntry, SearchResult,
};
use oxidgene_db::repo::{
    AncestryRepo, CitationRepo, EventRepo, FamilyChildRepo, FamilyRepo, FamilySpouseRepo,
    MediaLinkRepo, MediaRepo, NoteRepo, PersonDenormRepo, PersonDistinctRepo, PersonNameRepo,
    PersonRepo, PersonSearchFilters, PersonSearchRepo, PersonSearchSort, PlaceRepo, VignetteRepo,
    db_err,
};
use sea_orm::{ConnectionTrait, DatabaseConnection, TransactionSession, TransactionTrait};
use tracing::{debug, info, instrument};
use uuid::Uuid;

use super::builder::{
    self, TreeData, build_all_persons, build_db_search_entry, search_entry_from_db,
};
use super::{invalidation, pedigree};

/// Above this many affected persons, rebuild from a single whole-tree fetch
/// instead of running targeted per-person queries.
///
/// Affected sets from a normal mutation are 2–10 persons, where targeted
/// queries win. Bulk paths (GEDCOM import, tree-wide fixes) blow past this,
/// where one wide read beats N narrow ones.
const FULL_FETCH_THRESHOLD: usize = 50;

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
    /// Used after imports and when current projections or search rows are absent.
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
        // Having just written a tree's worth of rows the planner has no
        // statistics for. Imports and duplications refresh them once more
        // after their history, their last bulk write (`history::record_import`).
        oxidgene_db::repo::refresh_statistics(conn).await;

        info!(count = persons.len(), "Completed full projection rebuild");
        Ok(persons.len())
    }

    #[instrument(skip_all)]
    pub(crate) async fn rebuild_tree_full_transactional(
        &self,
        conn: &(impl ConnectionTrait + TransactionTrait),
        tree_id: Uuid,
    ) -> Result<usize, OxidGeneError> {
        info!("Starting transactional full projection rebuild");

        let tree_data = self.fetch_tree_data(conn, tree_id).await?;
        let persons = build_all_persons(tree_id, &tree_data);
        debug!(count = persons.len(), "Built projections");

        let search_entries: Vec<_> = persons.iter().map(build_db_search_entry).collect();
        let txn = conn.begin().await.map_err(db_err)?;
        PersonDenormRepo::replace_tree(&txn, tree_id, &persons).await?;
        PersonSearchRepo::replace_tree(&txn, tree_id, &search_entries).await?;
        txn.commit().await.map_err(db_err)?;
        // No statistics refresh here: its only caller, the import job, writes
        // the import's history next, and `history::record_import` refreshes
        // them once that last bulk write is in.

        info!(
            count = persons.len(),
            "Completed transactional full projection rebuild"
        );
        Ok(persons.len())
    }

    /// Materialize a tree's projections if they have never been built.
    ///
    /// Covers missing or outdated projections and independently cleared search rows.
    async fn ensure_materialized(
        &self,
        conn: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<(), OxidGeneError> {
        // Usable only when no row predates the current schema version.
        let denorm = PersonDenormRepo::is_materialized(conn, tree_id).await?;
        let search = PersonSearchRepo::has_tree(conn, tree_id).await?;
        if denorm && search {
            return Ok(());
        }

        debug!(denorm, search, "Tree projections are not materialized");
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
        let built = self
            .build_targeted(conn, tree_id, &[person_id])
            .await?
            .pop()
            .ok_or(OxidGeneError::NotFound {
                entity: "Person",
                id: person_id,
            })?;
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
            self.build_targeted(conn, tree_id, person_ids).await?
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
    /// a pedigree is a family-graph traversal plus a projection batch read.
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
    /// the database before rows are returned. `limit` is capped at
    /// [`SEARCH_MAX_LIMIT`].
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
            limit.min(SEARCH_MAX_LIMIT) as u64,
            offset as u64,
        )
        .await?;
        Ok(SearchResult {
            entries: page.entries.into_iter().map(search_entry_from_db).collect(),
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
        let conn = &self.db;
        self.ensure_materialized(conn, tree_id).await?;
        let distinct = PersonDistinctRepo::distinct_from(conn, person_id).await?;
        let rows =
            PersonSearchRepo::homonyms(conn, tree_id, person_id, SEARCH_MAX_LIMIT as u64).await?;
        Ok(rows
            .into_iter()
            .filter(|row| !distinct.contains(&row.person_id))
            .map(search_entry_from_db)
            .collect())
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
        let conn = &self.db;
        let profiles = self.projections_for(conn, tree_id, person_ids).await?;
        let mut by_id: HashMap<Uuid, SearchEntry> = profiles
            .iter()
            .map(|profile| (profile.person_id, builder::build_search_entry(profile)))
            .collect();
        Ok(person_ids
            .iter()
            .filter_map(|id| by_id.remove(id))
            .collect())
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

    /// Build the projections of a bounded set of persons with targeted
    /// queries — the persons, their families, their relatives' names, and the
    /// entities attached to them. No full-tree fetch.
    ///
    /// One fetch covers the whole set, so the query count does not grow with
    /// it: an edit that touches a person with a dozen relatives costs the same
    /// dozen statements as one that touches a single person.
    async fn build_targeted(
        &self,
        conn: &impl ConnectionTrait,
        tree_id: Uuid,
        person_ids: &[Uuid],
    ) -> Result<Vec<PersonProfile>, OxidGeneError> {
        let data = self.fetch_persons_data(conn, tree_id, person_ids).await?;
        builder::build_persons(tree_id, person_ids, &data).map_err(|id| OxidGeneError::NotFound {
            entity: "Person",
            id,
        })
    }

    /// Fetch only what the given projections need: the persons, their family
    /// memberships, all members of those families (for spouse / parent /
    /// child denormalization), their events + places, media and notes.
    async fn fetch_persons_data(
        &self,
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
        // behind, so reaching families through them would resurrect one. The
        // whole-tree path gets this for free from `FamilyRepo::list_all`;
        // without this the two paths disagree, and a targeted rebuild would
        // keep naming parents the user has just removed.
        let mut family_ids = FamilyRepo::live_ids(conn, &family_ids).await?;
        family_ids.sort();

        // 2. All members of those families, plus attached entities.
        let (spouses, children, person_events, family_events, media_links, citations, notes) = tokio::try_join!(
            FamilySpouseRepo::list_by_families(conn, &family_ids),
            FamilyChildRepo::list_by_families(conn, &family_ids),
            EventRepo::list_by_persons(conn, targets),
            EventRepo::list_by_families(conn, &family_ids),
            MediaLinkRepo::list_by_persons(conn, targets),
            CitationRepo::list_by_persons(conn, targets),
            NoteRepo::list_by_persons(conn, tree_id, targets),
        )?;

        // 3. Related person rows + names, places, media.
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
            PlaceRepo::get_many(conn, &place_ids),
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
        let vignette_ids: Vec<Uuid> = persons
            .iter()
            .filter(|p| targets.contains(&p.id))
            .filter_map(|p| p.portrait_vignette_id)
            .collect();
        let portrait_vignettes = VignetteRepo::get_many(conn, &vignette_ids).await?;
        let scan_ids = sorted_unique(
            portrait_vignettes
                .iter()
                .map(|v| v.media_id)
                .filter(|id| !media.iter().any(|m| m.id == *id)),
        );
        media.extend(MediaRepo::get_many(conn, &scan_ids).await?);

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

    /// The projections of `person_ids`, persons outside the pedigree window
    /// fetched for display: `what` names them in the log.
    async fn projections_outside(
        &self,
        conn: &impl ConnectionTrait,
        tree_id: Uuid,
        person_ids: &[Uuid],
        what: &str,
    ) -> Result<Vec<PersonProfile>, OxidGeneError> {
        if person_ids.is_empty() {
            return Ok(Vec::new());
        }
        debug!(
            count = person_ids.len(),
            "Pedigree build: fetching {what} outside the pedigree window"
        );
        self.projections_for(conn, tree_id, person_ids).await
    }

    /// Assemble a pedigree window for a root person from family links
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

        // Walk the family links for ancestor and descendant IDs.
        let (ancestors, descendants) = tokio::try_join!(
            AncestryRepo::ancestors(conn, root_person_id, Some(ancestor_depth as i32)),
            AncestryRepo::descendants(conn, root_person_id, Some(descendant_depth as i32)),
        )?;
        let mut person_ids = sorted_unique(
            std::iter::once(root_person_id)
                .chain(ancestors.iter().map(|a| a.person_id))
                .chain(descendants.iter().map(|d| d.person_id)),
        );
        let mut depths = pedigree::generations(root_person_id, &ancestors, &descendants);

        // The projections in the window, and the spouses outside it, which
        // take their partner's generation.
        let window_persons = self.projections_for(conn, tree_id, &person_ids).await?;
        let mut persons: HashMap<Uuid, &PersonProfile> =
            window_persons.iter().map(|p| (p.person_id, p)).collect();
        let spouse_persons = self
            .projections_outside(
                conn,
                tree_id,
                &pedigree::spouses_outside(&persons),
                "spouses",
            )
            .await?;
        for p in &spouse_persons {
            let generation = pedigree::partner_generation(p, &depths);
            depths.entry(p.person_id).or_insert(generation);
            persons.insert(p.person_id, p);
            person_ids.push(p.person_id);
        }

        let nodes = pedigree::nodes(&person_ids, &persons, &depths, root_person_id);
        let edges = pedigree::edges(&persons, &nodes);
        let family_events = pedigree::family_events(&persons);

        // The family memberships (spouse and children IDs per family), which
        // capture childless couples, who produce no edge, and the parental
        // families needed for sibling events.
        let mut families: HashMap<Uuid, PedigreeFamily> = HashMap::new();
        for person in persons.values() {
            pedigree::record_membership(&mut families, person, false);
        }
        let parents = pedigree::parents_to_fetch(&families, &persons);
        let parents = self
            .projections_outside(conn, tree_id, &parents, "parents for sibling data")
            .await?;
        pedigree::adopt_children_lists(&mut families, &parents);
        let members = pedigree::members_outside(&families, &nodes);
        let members = self
            .projections_outside(conn, tree_id, &members, "family members")
            .await?;
        pedigree::add_members_outside(&mut families, &members);

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
