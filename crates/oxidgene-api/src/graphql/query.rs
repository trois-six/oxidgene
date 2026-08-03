//! GraphQL query root with all read operations.

use async_graphql::{Context, ID, Object, Result};
use uuid::Uuid;

use oxidgene_db::repo::{
    AncestryRepo, EventFilter, EventRepo, FamilyRepo, MediaRepo, PaginationParams, PersonRepo,
    PlaceRepo, SourceRepo, TreeRepo,
};

use super::types::{
    GqlEvent, GqlEventConnection, GqlEventType, GqlExportGedcomResult, GqlFamily,
    GqlFamilyConnection, GqlMedia, GqlMediaConnection, GqlPedigree, GqlPerson, GqlPersonConnection,
    GqlPersonProfile, GqlPersonWithDepth, GqlPlace, GqlPlaceConnection, GqlSearchResult, GqlSource,
    GqlSourceConnection, GqlTree, GqlTreeConnection, db_from_ctx, profiles_from_ctx,
};

/// The root query type.
pub struct QueryRoot;

#[Object]
impl QueryRoot {
    // ── Trees ────────────────────────────────────────────────────────

    /// List all trees with cursor-based pagination.
    async fn trees(
        &self,
        ctx: &Context<'_>,
        first: Option<u64>,
        after: Option<String>,
    ) -> Result<GqlTreeConnection> {
        let db = db_from_ctx(ctx);
        let params = PaginationParams {
            first: first.unwrap_or(25),
            after,
        };
        let conn = TreeRepo::list(db, &params).await?;
        Ok(conn.into())
    }

    /// Get a single tree by ID.
    async fn tree(&self, ctx: &Context<'_>, id: ID) -> Result<Option<GqlTree>> {
        let db = db_from_ctx(ctx);
        let uuid = Uuid::parse_str(id.as_str())?;
        match TreeRepo::get(db, uuid).await {
            Ok(t) => Ok(Some(t.into())),
            Err(oxidgene_core::OxidGeneError::NotFound { .. }) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    // ── Persons ──────────────────────────────────────────────────────

    /// List persons in a tree with cursor-based pagination.
    async fn persons(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        first: Option<u64>,
        after: Option<String>,
    ) -> Result<GqlPersonConnection> {
        let db = db_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let params = PaginationParams {
            first: first.unwrap_or(25),
            after,
        };
        let conn = PersonRepo::list(db, tid, &params).await?;
        Ok(conn.into())
    }

    /// Get a single person by ID.
    async fn person(&self, ctx: &Context<'_>, tree_id: ID, id: ID) -> Result<Option<GqlPerson>> {
        let db = db_from_ctx(ctx);
        let _tid = Uuid::parse_str(tree_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        match PersonRepo::get(db, uuid).await {
            Ok(p) => Ok(Some(p.into())),
            Err(oxidgene_core::OxidGeneError::NotFound { .. }) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Get ancestors of a person.
    async fn ancestors(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        person_id: ID,
        max_depth: Option<i32>,
    ) -> Result<Vec<GqlPersonWithDepth>> {
        let db = db_from_ctx(ctx);
        let _tid = Uuid::parse_str(tree_id.as_str())?;
        let pid = Uuid::parse_str(person_id.as_str())?;
        let rows = AncestryRepo::ancestors(db, pid, max_depth).await?;
        let mut result = Vec::new();
        for row in rows {
            let person = PersonRepo::get(db, row.person_id).await?;
            result.push(GqlPersonWithDepth {
                person: person.into(),
                depth: row.depth,
            });
        }
        Ok(result)
    }

    /// Get descendants of a person.
    async fn descendants(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        person_id: ID,
        max_depth: Option<i32>,
    ) -> Result<Vec<GqlPersonWithDepth>> {
        let db = db_from_ctx(ctx);
        let _tid = Uuid::parse_str(tree_id.as_str())?;
        let pid = Uuid::parse_str(person_id.as_str())?;
        let rows = AncestryRepo::descendants(db, pid, max_depth).await?;
        let mut result = Vec::new();
        for row in rows {
            let person = PersonRepo::get(db, row.person_id).await?;
            result.push(GqlPersonWithDepth {
                person: person.into(),
                depth: row.depth,
            });
        }
        Ok(result)
    }

    // ── Families ─────────────────────────────────────────────────────

    /// List families in a tree with cursor-based pagination.
    async fn families(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        first: Option<u64>,
        after: Option<String>,
    ) -> Result<GqlFamilyConnection> {
        let db = db_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let params = PaginationParams {
            first: first.unwrap_or(25),
            after,
        };
        let conn = FamilyRepo::list(db, tid, &params).await?;
        Ok(conn.into())
    }

    /// Get a single family by ID.
    async fn family(&self, ctx: &Context<'_>, tree_id: ID, id: ID) -> Result<Option<GqlFamily>> {
        let db = db_from_ctx(ctx);
        let _tid = Uuid::parse_str(tree_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        match FamilyRepo::get(db, uuid).await {
            Ok(f) => Ok(Some(f.into())),
            Err(oxidgene_core::OxidGeneError::NotFound { .. }) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    // ── Events ───────────────────────────────────────────────────────

    /// List events in a tree with optional filters and cursor-based pagination.
    #[allow(clippy::too_many_arguments)]
    async fn events(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        first: Option<u64>,
        after: Option<String>,
        event_type: Option<GqlEventType>,
        person_id: Option<ID>,
        family_id: Option<ID>,
    ) -> Result<GqlEventConnection> {
        let db = db_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let filter = EventFilter {
            event_type: event_type.map(|et| et.into()),
            person_id: person_id
                .as_ref()
                .map(|id| Uuid::parse_str(id.as_str()))
                .transpose()?,
            family_id: family_id
                .as_ref()
                .map(|id| Uuid::parse_str(id.as_str()))
                .transpose()?,
        };
        let params = PaginationParams {
            first: first.unwrap_or(25),
            after,
        };
        let conn = EventRepo::list(db, tid, &filter, &params).await?;
        Ok(conn.into())
    }

    /// Get a single event by ID.
    async fn event(&self, ctx: &Context<'_>, tree_id: ID, id: ID) -> Result<Option<GqlEvent>> {
        let db = db_from_ctx(ctx);
        let _tid = Uuid::parse_str(tree_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        match EventRepo::get(db, uuid).await {
            Ok(e) => Ok(Some(e.into())),
            Err(oxidgene_core::OxidGeneError::NotFound { .. }) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    // ── Places ───────────────────────────────────────────────────────

    /// List places in a tree with optional search and cursor-based pagination.
    async fn places(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        first: Option<u64>,
        after: Option<String>,
        search: Option<String>,
    ) -> Result<GqlPlaceConnection> {
        let db = db_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let params = PaginationParams {
            first: first.unwrap_or(25),
            after,
        };
        let conn = PlaceRepo::list(db, tid, search.as_deref(), &params).await?;
        Ok(conn.into())
    }

    /// Get a single place by ID.
    async fn place(&self, ctx: &Context<'_>, tree_id: ID, id: ID) -> Result<Option<GqlPlace>> {
        let db = db_from_ctx(ctx);
        let _tid = Uuid::parse_str(tree_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        match PlaceRepo::get(db, uuid).await {
            Ok(p) => Ok(Some(p.into())),
            Err(oxidgene_core::OxidGeneError::NotFound { .. }) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    // ── Sources ──────────────────────────────────────────────────────

    /// List sources in a tree with cursor-based pagination.
    async fn sources(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        first: Option<u64>,
        after: Option<String>,
    ) -> Result<GqlSourceConnection> {
        let db = db_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let params = PaginationParams {
            first: first.unwrap_or(25),
            after,
        };
        let conn = SourceRepo::list(db, tid, &params).await?;
        Ok(conn.into())
    }

    /// Get a single source by ID.
    async fn source(&self, ctx: &Context<'_>, tree_id: ID, id: ID) -> Result<Option<GqlSource>> {
        let db = db_from_ctx(ctx);
        let _tid = Uuid::parse_str(tree_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        match SourceRepo::get(db, uuid).await {
            Ok(s) => Ok(Some(s.into())),
            Err(oxidgene_core::OxidGeneError::NotFound { .. }) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    // ── Media ────────────────────────────────────────────────────────

    /// List media in a tree with cursor-based pagination.
    async fn media_list(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        first: Option<u64>,
        after: Option<String>,
    ) -> Result<GqlMediaConnection> {
        let db = db_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let params = PaginationParams {
            first: first.unwrap_or(25),
            after,
        };
        let conn = MediaRepo::list(db, tid, &params).await?;
        Ok(conn.into())
    }

    /// Get a single media by ID.
    async fn media(&self, ctx: &Context<'_>, tree_id: ID, id: ID) -> Result<Option<GqlMedia>> {
        let db = db_from_ctx(ctx);
        let _tid = Uuid::parse_str(tree_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        match MediaRepo::get(db, uuid).await {
            Ok(m) => Ok(Some(m.into())),
            Err(oxidgene_core::OxidGeneError::NotFound { .. }) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    // ── GEDCOM ────────────────────────────────────────────────────────

    /// Export all entities in a tree as a GEDCOM 5.5.1 string. Pass
    /// `merge_occupations: true` to collapse each person's multiple `OCCU`
    /// tags back into one, comma-separated (for importers, e.g. Geneanet,
    /// that only support a single profession field). Pass
    /// `merge_names: true` to collapse each person's non-primary names into
    /// the primary name's `SURN` tag, comma-separated.
    async fn export_gedcom(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        merge_occupations: Option<bool>,
        merge_names: Option<bool>,
    ) -> Result<GqlExportGedcomResult> {
        let db = db_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let data = crate::service::gedcom::load_and_export(
            db,
            tid,
            merge_occupations.unwrap_or(false),
            merge_names.unwrap_or(false),
        )
        .await?;
        Ok(GqlExportGedcomResult {
            gedcom: data.gedcom,
            warnings: data.warnings,
        })
    }

    // ── Projection queries ───────────────────────────────────────────

    /// Get a single profile (denormalised) person profile.
    ///
    /// Falls back to building it from the DB if not yet materialized.
    async fn person_profile(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        person_id: ID,
    ) -> Result<GqlPersonProfile> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let pid = Uuid::parse_str(person_id.as_str())?;
        let profile = profiles.get_or_build_person(db, tid, pid).await?;
        Ok(profile.into())
    }

    /// Get every person projection of a tree.
    ///
    /// Materializes the tree first if it has never been built.
    async fn person_profiles(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
    ) -> Result<Vec<GqlPersonProfile>> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let persons = profiles.get_all_persons(db, tid).await?;
        Ok(persons.into_iter().map(Into::into).collect())
    }

    /// Server-side person search in a tree (spec name: `searchPersons`).
    ///
    /// Backed by the `person_search_fts` DB table (SQLite FTS5 / PostgreSQL)
    /// with accent-folded, normalised matching. Returns paginated results.
    async fn search_persons(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        query: String,
        #[graphql(default = 25)] limit: usize,
        #[graphql(default = 0)] offset: usize,
    ) -> Result<GqlSearchResult> {
        let profiles = profiles_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let result = profiles.search(tid, &query, limit.min(100), offset).await?;
        Ok(result.into())
    }

    /// Get a windowed pedigree for a root person.
    ///
    /// Returns nodes and edges within the given ancestor / descendant depth,
    /// assembled on demand from the closure table and the stored projections.
    async fn pedigree(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        root_person_id: ID,
        ancestor_depth: i32,
        descendant_depth: i32,
    ) -> Result<GqlPedigree> {
        let profiles = profiles_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let rid = Uuid::parse_str(root_person_id.as_str())?;
        let pedigree = profiles
            .get_or_build_pedigree(tid, rid, ancestor_depth as u32, descendant_depth as u32)
            .await?;
        Ok(pedigree.into())
    }
}
