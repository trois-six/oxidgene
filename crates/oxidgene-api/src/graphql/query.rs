//! GraphQL query root with all read operations.

use std::collections::HashMap;

use async_graphql::{Context, ID, Object, Result};
use base64::Engine as _;
use oxidgene_geneanet::archive::LocalOriginals;
use uuid::Uuid;

use crate::service::person::Lineage;
use crate::service::scope::{TreeResource, require_tree_resource};

use oxidgene_db::repo::{
    AuditFilter, CitationFilter, DictionaryRepo, EventFilter, EventRepo, FamilyRepo, HistoryRepo,
    MediaLinkRepo, MediaLinkTarget, MediaRepo, NoteFilter, NoteRepo, PaginationParams, PersonRepo,
    PersonSearchFilters, PlaceRepo, SOURCE_DRILL_THRESHOLD, SourceRepo, TreeRepo, VignetteRepo,
};

use super::history::{
    GqlAuditCategory, GqlAuditEntry, GqlAuditEntryConnection, GqlRecordType, GqlRecordVersion,
    GqlRecordVersionConnection, GqlVersionChangeConnection,
};
use super::inputs::{
    GeneanetPreviewInput, ImageSourceInput, MediaListFilterInput, geneanet_deposit_sizes,
};
use super::scope::{live_tree, opt_uuid, uuid, uuids};
use super::types::{
    GqlCitationConnection, GqlDictionaryEntry, GqlEvent, GqlEventConnection, GqlEventType,
    GqlExportGedcomResult, GqlExportJobStatus, GqlFamily, GqlFamilyConnection, GqlGalleryBundle,
    GqlGeneanetArchiveIndex, GqlGeneanetIndexedArchive, GqlGeneanetInspection,
    GqlGeneanetNeededMedia, GqlGeneanetPreview, GqlGivenNameReference, GqlGivenNameReferenceMatch,
    GqlImportJobStatus, GqlKinship, GqlMedia, GqlMediaConnection, GqlMediaDownload, GqlMediaFacets,
    GqlMediaLink, GqlMediaWithLink, GqlNote, GqlNoteConnection, GqlOccupationReference,
    GqlOccupationReferenceMatch, GqlPedigree, GqlPedigreeEntry, GqlPerson, GqlPersonConnection,
    GqlPersonDetailBundle, GqlPersonProfile, GqlPersonSearchSort, GqlPersonUsageEntry,
    GqlPersonWithDepth, GqlPlace, GqlPlaceConnection, GqlPlaceDictionaryEntry, GqlPlaceSuggestion,
    GqlPortrait, GqlPortraitImage, GqlRelationLabels, GqlSearchEntry, GqlSearchResult, GqlSource,
    GqlSourceConnection, GqlSourceDictionaryDrill, GqlSourceDictionaryEntry,
    GqlSourceDictionaryGroup, GqlSuggestionField, GqlTree, GqlTreeConnection, GqlTreeMediaLink,
    GqlValueSuggestion, GqlVignette, db_from_ctx, media_from_ctx, profiles_from_ctx,
    require_local_file_access,
};

async fn tree_resource_exists(
    db: &impl sea_orm::ConnectionTrait,
    tree_id: Uuid,
    resource: TreeResource,
    id: Uuid,
) -> Result<bool> {
    match require_tree_resource(db, tree_id, resource, id).await {
        Ok(()) => Ok(true),
        Err(oxidgene_core::OxidGeneError::NotFound { .. }) => Ok(false),
        Err(error) => Err(error.into()),
    }
}

/// The ancestors or descendants of a person, with every person read in one
/// query rather than one per row.
async fn lineage(
    ctx: &Context<'_>,
    tree_id: &ID,
    person_id: &ID,
    direction: Lineage,
    max_depth: Option<i32>,
) -> Result<Vec<GqlPersonWithDepth>> {
    let db = db_from_ctx(ctx);
    let tree_id = live_tree(ctx, tree_id).await?;
    let links =
        crate::service::person::lineage(db, tree_id, uuid(person_id)?, direction, max_depth)
            .await?;
    let ids: Vec<Uuid> = links.iter().map(|link| link.person_id).collect();
    let mut persons: HashMap<Uuid, oxidgene_core::types::Person> = PersonRepo::get_many(db, &ids)
        .await?
        .into_iter()
        .map(|person| (person.id, person))
        .collect();
    Ok(links
        .into_iter()
        .filter_map(|link| {
            persons
                .remove(&link.person_id)
                .map(|person| GqlPersonWithDepth {
                    person: person.into(),
                    depth: link.depth,
                })
        })
        .collect())
}

/// The root query type.
pub struct QueryRoot;

#[Object]
impl QueryRoot {
    /// Load the family neighborhood and evidence rendered by one person page.
    async fn person_detail_bundle(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        person_id: ID,
    ) -> Result<GqlPersonDetailBundle> {
        Ok(crate::service::person_detail::load_person_detail_bundle(
            db_from_ctx(ctx),
            live_tree(ctx, &tree_id).await?,
            uuid(&person_id)?,
        )
        .await?
        .into())
    }

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
        Ok(crate::service::tree::list_trees(db, &params).await?.into())
    }

    // ── History ──────────────────────────────────────────────────────

    /// The tree's audit log, newest first, optionally narrowed to one
    /// category or to the writes about one record. Mirrors
    /// `GET /trees/{treeId}/audit`.
    async fn audit_entries(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        first: Option<u64>,
        after: Option<String>,
        category: Option<GqlAuditCategory>,
        subject_id: Option<ID>,
    ) -> Result<GqlAuditEntryConnection> {
        let db = db_from_ctx(ctx);
        let tid = live_tree(ctx, &tree_id).await?;
        let filter = AuditFilter {
            category: category.map(Into::into),
            subject_id: opt_uuid(subject_id)?,
        };
        let params = PaginationParams {
            first: first.unwrap_or(25),
            after,
        };
        Ok(HistoryRepo::list_entries(db, tid, filter, &params)
            .await?
            .into())
    }

    /// One audit entry. Mirrors `GET /trees/{treeId}/audit/{entryId}`.
    async fn audit_entry(&self, ctx: &Context<'_>, tree_id: ID, id: ID) -> Result<GqlAuditEntry> {
        let db = db_from_ctx(ctx);
        let tid = live_tree(ctx, &tree_id).await?;
        let entry_id = uuid(&id)?;
        Ok(HistoryRepo::get_entry(db, tid, entry_id).await?.into())
    }

    /// The versions one write produced, each beside the version it replaced.
    /// Mirrors `GET /trees/{treeId}/audit/{entryId}/changes`.
    async fn audit_entry_changes(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        entry_id: ID,
        first: Option<u64>,
        after: Option<String>,
    ) -> Result<GqlVersionChangeConnection> {
        let db = db_from_ctx(ctx);
        let tid = live_tree(ctx, &tree_id).await?;
        let entry_id = uuid(&entry_id)?;
        HistoryRepo::get_entry(db, tid, entry_id).await?;
        let params = PaginationParams {
            first: first.unwrap_or(25),
            after,
        };
        Ok(HistoryRepo::list_entry_changes(db, tid, entry_id, &params)
            .await?
            .into())
    }

    /// A record's versions, latest first. Mirrors
    /// `GET /trees/{treeId}/history/{recordType}/{recordId}`.
    async fn record_versions(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        record_type: GqlRecordType,
        record_id: ID,
        first: Option<u64>,
        after: Option<String>,
    ) -> Result<GqlRecordVersionConnection> {
        let db = db_from_ctx(ctx);
        let tid = live_tree(ctx, &tree_id).await?;
        let rid = uuid(&record_id)?;
        let params = PaginationParams {
            first: first.unwrap_or(25),
            after,
        };
        Ok(
            HistoryRepo::list_versions(db, tid, record_type.into(), rid, &params)
                .await?
                .into(),
        )
    }

    /// One version of a record. Mirrors
    /// `GET /trees/{treeId}/history/{recordType}/{recordId}/{version}`.
    async fn record_version(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        record_type: GqlRecordType,
        record_id: ID,
        version: i32,
    ) -> Result<GqlRecordVersion> {
        let db = db_from_ctx(ctx);
        let tid = live_tree(ctx, &tree_id).await?;
        let rid = uuid(&record_id)?;
        Ok(
            HistoryRepo::get_version(db, tid, record_type.into(), rid, version)
                .await?
                .into(),
        )
    }

    /// Get a single tree by ID.
    async fn tree(&self, ctx: &Context<'_>, id: ID) -> Result<Option<GqlTree>> {
        let db = db_from_ctx(ctx);
        let id = uuid(&id)?;
        match TreeRepo::get(db, id).await {
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
        search: Option<String>,
    ) -> Result<GqlPersonConnection> {
        let db = db_from_ctx(ctx);
        let tid = live_tree(ctx, &tree_id).await?;
        let params = PaginationParams {
            first: first.unwrap_or(25),
            after,
        };
        let conn = PersonRepo::list_filtered(db, tid, search.as_deref(), &params).await?;
        Ok(conn.into())
    }

    /// Get a single person by ID.
    async fn person(&self, ctx: &Context<'_>, tree_id: ID, id: ID) -> Result<Option<GqlPerson>> {
        let db = db_from_ctx(ctx);
        let tid = live_tree(ctx, &tree_id).await?;
        let id = uuid(&id)?;
        match PersonRepo::get_in_tree(db, tid, id).await {
            Ok(p) => Ok(Some(p.into())),
            Err(oxidgene_core::OxidGeneError::NotFound { .. }) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Load the names and spouse links needed to label a bounded set of relations.
    async fn relation_labels(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        person_ids: Vec<ID>,
        family_ids: Vec<ID>,
    ) -> Result<GqlRelationLabels> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        let person_ids = uuids(&person_ids)?;
        let family_ids = uuids(&family_ids)?;
        Ok(crate::service::relation_labels::load_relation_labels(
            db_from_ctx(ctx),
            tree_id,
            &person_ids,
            &family_ids,
        )
        .await?
        .into())
    }

    /// Resolve one SOSA-Stradonitz number from the tree's configured root.
    ///
    /// Returns null when the tree has no SOSA root or the ancestry chain is
    /// incomplete at that number, matching REST's not-found outcome.
    async fn person_by_sosa(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        number: u64,
    ) -> Result<Option<GqlPerson>> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        let person =
            crate::service::person::person_by_sosa(db_from_ctx(ctx), tree_id, number).await?;
        Ok(person.map(Into::into))
    }

    /// Every person's selected portrait, with enough data for a pedigree to
    /// choose its thumbnail, original file or cropped vignette endpoint.
    async fn portraits(&self, ctx: &Context<'_>, tree_id: ID) -> Result<Vec<GqlPortrait>> {
        let db = db_from_ctx(ctx);
        let portraits = PersonRepo::list_portraits(db, live_tree(ctx, &tree_id).await?).await?;
        Ok(portraits.into_iter().map(Into::into).collect())
    }

    /// Load display-ready portraits for a bounded set of people in one operation.
    async fn portrait_images(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        person_ids: Vec<ID>,
    ) -> Result<Vec<GqlPortraitImage>> {
        let person_ids = uuids(&person_ids)?;
        let images = crate::service::portrait::load_portrait_images(
            db_from_ctx(ctx),
            live_tree(ctx, &tree_id).await?,
            &person_ids,
        )
        .await?;
        Ok(images.into_iter().map(Into::into).collect())
    }

    /// Resolve held picture sources to inline `data:` URLs in one operation,
    /// for a client that cannot serve them from an origin of its own.
    /// Mirrors REST's `image-data` endpoint.
    async fn image_data(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        sources: Vec<ImageSourceInput>,
    ) -> Result<Vec<Option<String>>> {
        let sources = sources
            .into_iter()
            .map(oxidgene_core::types::ImageSource::try_from)
            .collect::<Result<Vec<_>>>()?;
        Ok(crate::service::image_bytes::load_image_data_urls(
            db_from_ctx(ctx),
            media_from_ctx(ctx),
            live_tree(ctx, &tree_id).await?,
            &sources,
        )
        .await?)
    }

    /// Load display-ready media gallery data in one bounded operation.
    async fn gallery_bundle(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        media_ids: Vec<ID>,
        vignette_ids: Vec<ID>,
    ) -> Result<GqlGalleryBundle> {
        let media_ids = uuids(&media_ids)?;
        let vignette_ids = uuids(&vignette_ids)?;
        Ok(crate::service::gallery::load_gallery_bundle(
            db_from_ctx(ctx),
            live_tree(ctx, &tree_id).await?,
            &media_ids,
            &vignette_ids,
        )
        .await?
        .into())
    }

    /// Get ancestors of a person, each at its shortest distance, down to
    /// `maxDepth` generations (at most, and by default, 64).
    async fn ancestors(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        person_id: ID,
        max_depth: Option<i32>,
    ) -> Result<Vec<GqlPersonWithDepth>> {
        lineage(ctx, &tree_id, &person_id, Lineage::Ancestors, max_depth).await
    }

    /// Get descendants of a person, each at its shortest distance, down to
    /// `maxDepth` generations (at most, and by default, 64).
    async fn descendants(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        person_id: ID,
        max_depth: Option<i32>,
    ) -> Result<Vec<GqlPersonWithDepth>> {
        lineage(ctx, &tree_id, &person_id, Lineage::Descendants, max_depth).await
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
        let tid = live_tree(ctx, &tree_id).await?;
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
        let tid = live_tree(ctx, &tree_id).await?;
        let id = uuid(&id)?;
        if !tree_resource_exists(db, tid, TreeResource::Family, id).await? {
            return Ok(None);
        }
        match FamilyRepo::get(db, id).await {
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
        let tid = live_tree(ctx, &tree_id).await?;
        let filter = EventFilter {
            event_type: event_type.map(|et| et.into()),
            person_id: opt_uuid(person_id.as_ref())?,
            family_id: opt_uuid(family_id.as_ref())?,
        };
        let params = PaginationParams {
            first: first.unwrap_or(25),
            after,
        };
        let conn = crate::service::event::list_events(db, tid, &filter, &params).await?;
        Ok(conn.into())
    }

    /// Get a single event by ID.
    async fn event(&self, ctx: &Context<'_>, tree_id: ID, id: ID) -> Result<Option<GqlEvent>> {
        let db = db_from_ctx(ctx);
        let tid = live_tree(ctx, &tree_id).await?;
        let id = uuid(&id)?;
        if !tree_resource_exists(db, tid, TreeResource::Event, id).await? {
            return Ok(None);
        }
        match EventRepo::get(db, id).await {
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
        let tid = live_tree(ctx, &tree_id).await?;
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
        let tid = live_tree(ctx, &tree_id).await?;
        let id = uuid(&id)?;
        if !tree_resource_exists(db, tid, TreeResource::Place, id).await? {
            return Ok(None);
        }
        match PlaceRepo::get(db, id).await {
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
        let tid = live_tree(ctx, &tree_id).await?;
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
        let tid = live_tree(ctx, &tree_id).await?;
        let id = uuid(&id)?;
        if !tree_resource_exists(db, tid, TreeResource::Source, id).await? {
            return Ok(None);
        }
        match SourceRepo::get(db, id).await {
            Ok(s) => Ok(Some(s.into())),
            Err(oxidgene_core::OxidGeneError::NotFound { .. }) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// List citations in a tree with optional entity filters and pagination.
    #[allow(clippy::too_many_arguments)]
    async fn citations(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        person_id: Option<ID>,
        event_id: Option<ID>,
        family_id: Option<ID>,
        source_id: Option<ID>,
        first: Option<u64>,
        after: Option<String>,
    ) -> Result<GqlCitationConnection> {
        let db = db_from_ctx(ctx);
        let tree_id = live_tree(ctx, &tree_id).await?;
        let filter = CitationFilter {
            person_id: opt_uuid(person_id)?,
            event_id: opt_uuid(event_id)?,
            family_id: opt_uuid(family_id)?,
            source_id: opt_uuid(source_id)?,
        };
        let params = PaginationParams {
            first: first.unwrap_or(25),
            after,
        };
        Ok(
            crate::service::citation::list_citations(db, tree_id, &filter, &params)
                .await?
                .into(),
        )
    }

    /// Get a single note by ID.
    async fn note(&self, ctx: &Context<'_>, tree_id: ID, id: ID) -> Result<Option<GqlNote>> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        match crate::service::note::get_note(db_from_ctx(ctx), tree_id, uuid(&id)?).await {
            Ok(note) => Ok(Some(note.into())),
            Err(oxidgene_core::OxidGeneError::NotFound { .. }) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// List notes in a tree with optional entity filters and pagination.
    #[allow(clippy::too_many_arguments)]
    async fn notes(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        person_id: Option<ID>,
        event_id: Option<ID>,
        family_id: Option<ID>,
        source_id: Option<ID>,
        media_id: Option<ID>,
        first: Option<u64>,
        after: Option<String>,
    ) -> Result<GqlNoteConnection> {
        let db = db_from_ctx(ctx);
        let tree_id = live_tree(ctx, &tree_id).await?;
        let filter = NoteFilter {
            person_id: opt_uuid(person_id)?,
            event_id: opt_uuid(event_id)?,
            family_id: opt_uuid(family_id)?,
            source_id: opt_uuid(source_id)?,
            media_id: opt_uuid(media_id)?,
        };
        let params = PaginationParams {
            first: first.unwrap_or(25),
            after,
        };
        Ok(NoteRepo::list(db, tree_id, &filter, &params).await?.into())
    }

    // ── Dictionary and reference content ────────────────────────────

    /// Distinct family names and their person counts.
    async fn dictionary_family_names(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
    ) -> Result<Vec<GqlDictionaryEntry>> {
        let db = db_from_ctx(ctx);
        Ok(
            DictionaryRepo::family_names(db, live_tree(ctx, &tree_id).await?)
                .await?
                .into_iter()
                .map(Into::into)
                .collect(),
        )
    }

    /// Distinct occupation labels and their person counts.
    async fn dictionary_occupations(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
    ) -> Result<Vec<GqlDictionaryEntry>> {
        let db = db_from_ctx(ctx);
        Ok(
            DictionaryRepo::occupations(db, live_tree(ctx, &tree_id).await?)
                .await?
                .into_iter()
                .map(Into::into)
                .collect(),
        )
    }

    /// Sources whose titles match a prefix, with citation counts.
    async fn dictionary_sources(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        prefix: Option<String>,
    ) -> Result<Vec<GqlSourceDictionaryEntry>> {
        let db = db_from_ctx(ctx);
        let entries = DictionaryRepo::sources_with_usage_by_prefix(
            db,
            live_tree(ctx, &tree_id).await?,
            prefix.as_deref().unwrap_or_default(),
        )
        .await?;
        Ok(entries
            .into_iter()
            .map(|(source, count)| GqlSourceDictionaryEntry {
                source: source.into(),
                count,
            })
            .collect())
    }

    /// The next selectable source-title prefixes for the smart drill-down.
    async fn dictionary_source_drill(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        prefix: Option<String>,
    ) -> Result<GqlSourceDictionaryDrill> {
        let db = db_from_ctx(ctx);
        let (prefix, total, groups) = DictionaryRepo::resolve_source_drill_down(
            db,
            live_tree(ctx, &tree_id).await?,
            prefix.as_deref().unwrap_or_default(),
            SOURCE_DRILL_THRESHOLD,
        )
        .await?;
        Ok(GqlSourceDictionaryDrill {
            prefix,
            total,
            groups: groups
                .into_iter()
                .map(|(label, count)| GqlSourceDictionaryGroup { label, count })
                .collect(),
        })
    }

    /// The country outlines the statistics heat map is drawn over.
    async fn basemap(&self) -> Vec<crate::reference::BasemapCountry> {
        crate::reference::basemap().to_vec()
    }

    /// A tree's statistics, time series filed by year. `approximate` lets
    /// ages and averages use dates about, calculated or estimated;
    /// `language` names places' countries, regions and subdivisions
    /// (English when omitted).
    async fn tree_statistics(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        approximate: Option<bool>,
        language: Option<String>,
    ) -> Result<crate::service::statistics::TreeStatistics> {
        let lang = crate::service::statistics::language(language.as_deref())?;
        Ok(crate::service::statistics::load(
            db_from_ctx(ctx),
            profiles_from_ctx(ctx),
            live_tree(ctx, &tree_id).await?,
            approximate.unwrap_or(false),
            lang,
        )
        .await?)
    }

    /// How many persons a tree held over the days it was worked on, day by
    /// day, with its imports.
    async fn tree_growth(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
    ) -> Result<crate::service::statistics::growth::TreeGrowth> {
        Ok(crate::service::statistics::growth::load(
            db_from_ctx(ctx),
            live_tree(ctx, &tree_id).await?,
        )
        .await?)
    }

    /// Which ancestors of the tree's SOSA root are known, generation by
    /// generation, and which of their key facts are recorded; `generations`
    /// counts the root's (8 when omitted, at most 15).
    async fn ancestry_completeness(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        generations: Option<i64>,
    ) -> Result<crate::service::ancestry::AncestryCompleteness> {
        let generations = crate::service::ancestry::generations(generations)?;
        Ok(crate::service::ancestry::load(
            db_from_ctx(ctx),
            profiles_from_ctx(ctx),
            live_tree(ctx, &tree_id).await?,
            generations,
        )
        .await?)
    }

    /// The tree's anomalies: dates, filiations, unions, witnesses and
    /// records that are impossible or unlikely, by rule.
    async fn tree_anomalies(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
    ) -> Result<crate::service::anomalies::TreeAnomalies> {
        Ok(crate::service::anomalies::load(
            db_from_ctx(ctx),
            profiles_from_ctx(ctx),
            live_tree(ctx, &tree_id).await?,
        )
        .await?)
    }

    /// The pairs of records of the tree that may be one person, best first.
    async fn potential_duplicates(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
    ) -> Result<super::types::GqlPotentialDuplicates> {
        Ok(crate::service::duplicates::load_potential_duplicates(
            db_from_ctx(ctx),
            profiles_from_ctx(ctx),
            live_tree(ctx, &tree_id).await?,
        )
        .await?
        .into())
    }

    /// The places of the tree the statistics cannot locate, with their
    /// usage, most used first.
    async fn unlocated_places(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
    ) -> Result<Vec<crate::service::statistics::PlaceUsage>> {
        Ok(crate::service::anomalies::load_unlocated_places(
            db_from_ctx(ctx),
            live_tree(ctx, &tree_id).await?,
        )
        .await?)
    }

    /// Places with their event and media usage count.
    async fn dictionary_places(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
    ) -> Result<Vec<GqlPlaceDictionaryEntry>> {
        let db = db_from_ctx(ctx);
        let entries =
            DictionaryRepo::places_with_usage(db, live_tree(ctx, &tree_id).await?).await?;
        Ok(entries
            .into_iter()
            .map(|(place, count)| GqlPlaceDictionaryEntry {
                place: place.into(),
                count,
            })
            .collect())
    }

    /// People who carry one family name.
    async fn family_name_usage(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        value: String,
    ) -> Result<Vec<GqlPersonUsageEntry>> {
        let db = db_from_ctx(ctx);
        let ids = DictionaryRepo::family_name_usage_person_ids(
            db,
            live_tree(ctx, &tree_id).await?,
            &value,
        )
        .await?;
        Ok(DictionaryRepo::resolve_person_usage_entries(db, &ids)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    /// People whose occupation exactly matches one dictionary value.
    async fn occupation_usage(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        value: String,
    ) -> Result<Vec<GqlPersonUsageEntry>> {
        let db = db_from_ctx(ctx);
        let ids = DictionaryRepo::occupation_usage_person_ids(
            db,
            live_tree(ctx, &tree_id).await?,
            &value,
        )
        .await?;
        Ok(DictionaryRepo::resolve_person_usage_entries(db, &ids)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    /// People cited by one source, directly or through an individual event.
    async fn source_usage(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        source_id: ID,
    ) -> Result<Vec<GqlPersonUsageEntry>> {
        let db = db_from_ctx(ctx);
        let source_id = uuid(&source_id)?;
        require_tree_resource(
            db,
            live_tree(ctx, &tree_id).await?,
            TreeResource::Source,
            source_id,
        )
        .await?;
        let ids = DictionaryRepo::source_usage_person_ids(db, source_id).await?;
        Ok(DictionaryRepo::resolve_person_usage_entries(db, &ids)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    /// People with an individual event at one place.
    async fn place_usage(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        place_id: ID,
    ) -> Result<Vec<GqlPersonUsageEntry>> {
        let db = db_from_ctx(ctx);
        let place_id = uuid(&place_id)?;
        require_tree_resource(
            db,
            live_tree(ctx, &tree_id).await?,
            TreeResource::Place,
            place_id,
        )
        .await?;
        let ids = DictionaryRepo::place_usage_person_ids(db, place_id).await?;
        Ok(DictionaryRepo::resolve_person_usage_entries(db, &ids)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    /// Resolve static occupation reference content for `fr` or `en`.
    async fn occupation_reference(
        &self,
        _ctx: &Context<'_>,
        language: String,
        term: String,
    ) -> Result<Option<GqlOccupationReference>> {
        let language = crate::reference::language(&language)?;
        Ok(crate::reference::lookup_occupation(language, &term).map(Into::into))
    }

    /// Resolve several static occupation references in one operation.
    async fn occupation_references(
        &self,
        _ctx: &Context<'_>,
        language: String,
        terms: Vec<String>,
    ) -> Result<Vec<GqlOccupationReferenceMatch>> {
        let language = crate::reference::language(&language)?;
        crate::reference::check_terms(&terms)?;
        Ok(crate::reference::lookup_occupations(language, &terms)
            .into_iter()
            .map(Into::into)
            .collect())
    }

    /// Resolve static given-name reference content for `fr` or `en`.
    async fn given_name_reference(
        &self,
        _ctx: &Context<'_>,
        language: String,
        term: String,
    ) -> Result<Option<GqlGivenNameReference>> {
        let language = crate::reference::language(&language)?;
        Ok(crate::reference::lookup_given_name(language, &term).map(Into::into))
    }

    /// Resolve several static given-name references in one operation.
    async fn given_name_references(
        &self,
        _ctx: &Context<'_>,
        language: String,
        terms: Vec<String>,
    ) -> Result<Vec<GqlGivenNameReferenceMatch>> {
        let language = crate::reference::language(&language)?;
        crate::reference::check_terms(&terms)?;
        Ok(crate::reference::lookup_given_names(language, &terms)
            .into_iter()
            .map(Into::into)
            .collect())
    }

    /// Suggest places from the place dictionary for `fr` or `en`. The text
    /// before the first comma matches place names; each part after a comma
    /// must start a word of the code, subdivision, region or country.
    async fn place_suggestions(
        &self,
        _ctx: &Context<'_>,
        language: String,
        query: String,
        limit: Option<usize>,
    ) -> Result<Vec<GqlPlaceSuggestion>> {
        let language = crate::reference::language(&language)?;
        let limit = crate::reference::place_limit(limit)?;
        // The first search decompresses and indexes the dictionary, and
        // every search scans it: kept off the async workers.
        let places = tokio::task::spawn_blocking(move || {
            crate::reference::search_places(language, &query, limit)
        })
        .await?;
        Ok(places.into_iter().map(Into::into).collect())
    }

    /// Values an entry-form field suggests: the tree's values with a word
    /// starting with `query`, then, for occupations and given names, the
    /// reference terms the tree does not hold yet. `limit` defaults to 10.
    /// `surname` and `givenNames` scope a name field to the persons the
    /// person search's filters of the same names find.
    #[allow(clippy::too_many_arguments)]
    async fn value_suggestions(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        field: GqlSuggestionField,
        language: String,
        query: String,
        limit: Option<usize>,
        surname: Option<String>,
        given_names: Option<String>,
    ) -> Result<Vec<GqlValueSuggestion>> {
        let db = db_from_ctx(ctx);
        Ok(crate::service::suggestions::suggest(
            db,
            live_tree(ctx, &tree_id).await?,
            field.into(),
            &language,
            &query,
            limit,
            &crate::service::suggestions::NameScope {
                surname,
                given_names,
            },
        )
        .await?
        .into_iter()
        .map(Into::into)
        .collect())
    }

    // ── Media ────────────────────────────────────────────────────────

    /// List a tree's documents with cursor-based pagination, narrowed by
    /// `filter`, each edge carrying the document's usage count.
    async fn media_list(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        first: Option<u64>,
        after: Option<String>,
        filter: Option<MediaListFilterInput>,
    ) -> Result<GqlMediaConnection> {
        let db = db_from_ctx(ctx);
        let tid = live_tree(ctx, &tree_id).await?;
        let params = PaginationParams {
            first: first.unwrap_or(25),
            after,
        };
        let conn = crate::service::media_library::list(
            db,
            tid,
            filter.unwrap_or_default().into(),
            &params,
        )
        .await?;
        Ok(conn.into())
    }

    /// The tags, file kinds and categories the tree's documents carry, each
    /// with its document count; with `tags`, the tags are counted among the
    /// documents carrying every tag given.
    async fn media_facets(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        #[graphql(default)] tags: Vec<String>,
    ) -> Result<GqlMediaFacets> {
        let tid = live_tree(ctx, &tree_id).await?;
        Ok(
            crate::service::media_library::facets(db_from_ctx(ctx), tid, tags)
                .await?
                .into(),
        )
    }

    /// Get a single media by ID.
    async fn media(&self, ctx: &Context<'_>, tree_id: ID, id: ID) -> Result<Option<GqlMedia>> {
        let db = db_from_ctx(ctx);
        let tid = live_tree(ctx, &tree_id).await?;
        let id = uuid(&id)?;
        if !tree_resource_exists(db, tid, TreeResource::Media, id).await? {
            return Ok(None);
        }
        match MediaRepo::get(db, id).await {
            Ok(m) => Ok(Some(m.into())),
            Err(oxidgene_core::OxidGeneError::NotFound { .. }) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// A checked HTTP attachment URL for one stored original of any media type.
    async fn media_download(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        id: ID,
    ) -> Result<GqlMediaDownload> {
        let db = db_from_ctx(ctx);
        let tree_id = live_tree(ctx, &tree_id).await?;
        let id = uuid(&id)?;
        let media = crate::service::media::download_record(db, tree_id, id).await?;
        let key = crate::service::media::stored_key(&media)?;
        // Open without collecting the body, preserving storage errors and bounded memory.
        let _stream = media_from_ctx(ctx).get_stream(key).await?;
        Ok(GqlMediaDownload {
            url: format!("/api/v1/trees/{tree_id}/media/{id}/download"),
        })
    }

    /// A checked HTTP attachment URL for a complete document ZIP.
    async fn media_archive(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        id: ID,
    ) -> Result<GqlMediaDownload> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        let id = uuid(&id)?;
        let (_, pages) =
            crate::service::media::archive_pages(db_from_ctx(ctx), tree_id, id).await?;
        for page in &pages {
            // A remote page contributes a shortcut, not bytes: there is no
            // stored file to check before promising the archive.
            if oxidgene_core::types::is_remote_url(&page.file_path) {
                continue;
            }
            let key = crate::service::media::stored_key(page)?;
            let _stream = media_from_ctx(ctx).get_stream(key).await?;
        }
        Ok(GqlMediaDownload {
            url: format!("/api/v1/trees/{tree_id}/media/{id}/archive"),
        })
    }

    /// Whether the supplied gallery link is this media's sole external
    /// reference. Mirrors REST's `deletion-status` endpoint.
    async fn can_delete_media(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        id: ID,
        allowed_link_id: ID,
    ) -> Result<bool> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        Ok(crate::service::media::can_delete_media(
            db_from_ctx(ctx),
            tree_id,
            uuid(&id)?,
            uuid(&allowed_link_id)?,
        )
        .await?)
    }

    /// Every media attached to one entity, with its link.
    ///
    /// `entityType` is `person`, `family`, `event` or `source`. Mirrors
    /// `GET /trees/{treeId}/media-links?entity_type=…&entity_id=…`.
    async fn entity_media(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        entity_type: String,
        entity_id: ID,
    ) -> Result<Vec<GqlMediaWithLink>> {
        let db = db_from_ctx(ctx);
        let tid = live_tree(ctx, &tree_id).await?;
        let target = MediaLinkTarget::parse(&entity_type).ok_or_else(|| {
            async_graphql::Error::new(format!(
                "unknown entityType `{entity_type}`; expected person, family, event or source"
            ))
        })?;
        let entity_id = uuid(&entity_id)?;
        let resource = match target {
            MediaLinkTarget::Person => TreeResource::Person,
            MediaLinkTarget::Family => TreeResource::Family,
            MediaLinkTarget::Event => TreeResource::Event,
            MediaLinkTarget::Source => TreeResource::Source,
        };
        require_tree_resource(db, tid, resource, entity_id).await?;
        let rows = MediaLinkRepo::list_with_media(db, target, entity_id).await?;
        Ok(rows
            .into_iter()
            .map(|(link, media)| GqlMediaWithLink {
                link_id: ID(link.id.to_string()),
                sort_order: link.sort_order,
                media: media.into(),
            })
            .collect())
    }

    /// Everything one media file is attached to.
    ///
    /// The other direction from `entityMedia`: what lets a media's own panel
    /// say which events it documents. Mirrors
    /// `GET /trees/{treeId}/media-links?media_id=…`.
    async fn media_links(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        media_id: ID,
    ) -> Result<Vec<GqlMediaLink>> {
        let db = db_from_ctx(ctx);
        let tid = live_tree(ctx, &tree_id).await?;
        let media_id = uuid(&media_id)?;
        require_tree_resource(db, tid, TreeResource::Media, media_id).await?;
        let links = MediaLinkRepo::list_by_media(db, media_id).await?;
        Ok(links.into_iter().map(Into::into).collect())
    }

    /// Every person and event media link in a tree.
    async fn tree_media_links(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
    ) -> Result<Vec<GqlTreeMediaLink>> {
        let db = db_from_ctx(ctx);
        let tree_id = live_tree(ctx, &tree_id).await?;
        TreeRepo::get(db, tree_id).await?;
        let links = MediaLinkRepo::list_for_tree(db, tree_id).await?;
        Ok(links.into_iter().map(Into::into).collect())
    }

    /// The pages of a multi-page document, in order.
    async fn media_pages(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        media_id: ID,
    ) -> Result<Vec<GqlMedia>> {
        let db = db_from_ctx(ctx);
        let tid = live_tree(ctx, &tree_id).await?;
        let media_id = uuid(&media_id)?;
        require_tree_resource(db, tid, TreeResource::Media, media_id).await?;
        let pages = MediaRepo::list_pages(db, media_id).await?;
        Ok(pages.into_iter().map(Into::into).collect())
    }

    // ── Vignettes ────────────────────────────────────────────────────

    /// Vignettes on a media file, in page order.
    async fn media_vignettes(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        media_id: ID,
    ) -> Result<Vec<GqlVignette>> {
        let db = db_from_ctx(ctx);
        let tid = live_tree(ctx, &tree_id).await?;
        let mid = uuid(&media_id)?;
        require_tree_resource(db, tid, TreeResource::Media, mid).await?;
        let vignettes = VignetteRepo::list_for_media(db, mid).await?;
        Ok(vignettes.into_iter().map(Into::into).collect())
    }

    /// Vignettes attributed to a person, or standing as evidence for an event.
    ///
    /// Exactly one of `personId` or `eventId` is required — an unfiltered list
    /// of every crop in a tree is not a view anything needs.
    async fn vignettes(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        person_id: Option<ID>,
        event_id: Option<ID>,
    ) -> Result<Vec<GqlVignette>> {
        let db = db_from_ctx(ctx);
        let tid = live_tree(ctx, &tree_id).await?;
        let vignettes = match (person_id, event_id) {
            (Some(person_id), None) => {
                let person_id = uuid(&person_id)?;
                require_tree_resource(db, tid, TreeResource::Person, person_id).await?;
                VignetteRepo::list_for_person(db, person_id).await?
            }
            (None, Some(event_id)) => {
                let event_id = uuid(&event_id)?;
                require_tree_resource(db, tid, TreeResource::Event, event_id).await?;
                VignetteRepo::list_for_event(db, event_id).await?
            }
            _ => {
                return Err(async_graphql::Error::new(
                    "exactly one of personId or eventId is required",
                ));
            }
        };
        Ok(vignettes.into_iter().map(Into::into).collect())
    }

    /// Get a single vignette by ID.
    async fn vignette(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        id: ID,
    ) -> Result<Option<GqlVignette>> {
        let db = db_from_ctx(ctx);
        let tid = live_tree(ctx, &tree_id).await?;
        let id = uuid(&id)?;
        if !tree_resource_exists(db, tid, TreeResource::Vignette, id).await? {
            return Ok(None);
        }
        match VignetteRepo::get(db, id).await {
            Ok(v) => Ok(Some(v.into())),
            Err(oxidgene_core::OxidGeneError::NotFound { .. }) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    // ── GEDCOM ────────────────────────────────────────────────────────

    /// Export all entities in a tree as a GEDCOM 5.5.1 string, recording
    /// the export in the tree's audit log. Pass `merge_occupations: true` to
    /// collapse each person's multiple `OCCU` tags back into one,
    /// comma-separated (for importers, e.g. Geneanet, that only support a
    /// single profession field). Pass `merge_names: true` to collapse each
    /// person's non-primary names into the primary name's `SURN` tag,
    /// comma-separated.
    async fn export_gedcom(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        merge_occupations: Option<bool>,
        merge_names: Option<bool>,
    ) -> Result<GqlExportGedcomResult> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        let data = crate::service::gedcom::export_gedcom(
            db_from_ctx(ctx),
            tree_id,
            merge_occupations.unwrap_or(false),
            merge_names.unwrap_or(false),
        )
        .await?;
        Ok(GqlExportGedcomResult {
            gedcom: data.gedcom,
            warnings: data.warnings,
        })
    }

    /// Poll a durable GEDZIP export created by `startExportJob`.
    async fn export_job_status(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        job_id: ID,
    ) -> Result<GqlExportJobStatus> {
        // Not `live_tree`: a running job answers from memory, without the
        // database; the service checks the tree when it has to read.
        let tree_id = uuid(&tree_id)?;
        let status = crate::service::background_job::export_job_status(
            db_from_ctx(ctx),
            tree_id,
            uuid(&job_id)?,
        )
        .await?;
        Ok(status.into())
    }

    /// Poll a durable genealogy file import created by `startFileImportJob`.
    async fn import_job_status(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        job_id: ID,
    ) -> Result<GqlImportJobStatus> {
        // Not `live_tree`: a running job answers from memory, without the
        // database; the service checks the tree when it has to read.
        let tree_id = uuid(&tree_id)?;
        let status = crate::service::background_job::import_job_status(
            db_from_ctx(ctx),
            tree_id,
            uuid(&job_id)?,
        )
        .await?;
        Ok(status.into())
    }

    // ── Geneanet import wizard ───────────────────────────────────────

    /// Inspect a GeneWeb export before selecting its destination tree.
    async fn inspect_geneweb(
        &self,
        gw_base64: String,
        file_name: String,
    ) -> Result<GqlGeneanetInspection> {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(gw_base64)
            .map_err(|error| async_graphql::Error::new(format!("invalid .gw base64: {error}")))?;
        let inspection = crate::service::geneanet::inspect_gw(&bytes, &file_name)?;
        Ok(GqlGeneanetInspection {
            person_count: inspection.person_count as i64,
            family_count: inspection.family_count as i64,
            skipped_blocks: inspection.skipped_blocks as i64,
        })
    }

    /// Index local Geneanet archives by path.
    async fn index_geneanet_archives(
        &self,
        ctx: &Context<'_>,
        paths: Vec<String>,
    ) -> Result<GqlGeneanetArchiveIndex> {
        require_local_file_access(ctx)?;
        let (set, reports) = crate::service::geneanet::index_archives(&paths);
        Ok(GqlGeneanetArchiveIndex {
            file_count: set.file_count() as i64,
            archives: reports
                .into_iter()
                .map(|report| GqlGeneanetIndexedArchive {
                    path: report.path,
                    file_name: report.file_name,
                    file_count: report.file_count as i64,
                    image_count: report.image_count as i64,
                    error: report.error,
                })
                .collect(),
        })
    }

    /// Preview a Geneanet import without writing a tree or fetching media.
    async fn geneanet_preview(
        &self,
        ctx: &Context<'_>,
        input: GeneanetPreviewInput,
    ) -> Result<GqlGeneanetPreview> {
        require_local_file_access(ctx)?;
        let gw = base64::engine::general_purpose::STANDARD
            .decode(&input.gw_base64)
            .map_err(|error| async_graphql::Error::new(format!("invalid .gw base64: {error}")))?;
        let deposit_sizes = geneanet_deposit_sizes(&input.deposit_sizes)?;
        let (archives, _) = crate::service::geneanet::index_archives(&input.archive_paths);
        Ok(crate::service::geneanet::preview(
            &gw,
            &input.file_name,
            &input.collection,
            &deposit_sizes,
            &archives,
            input.media_fidelity.into(),
        )?
        .into())
    }

    /// List the media that the signed-in Geneanet window still has to fetch.
    async fn geneanet_plan(
        &self,
        ctx: &Context<'_>,
        input: GeneanetPreviewInput,
    ) -> Result<Vec<GqlGeneanetNeededMedia>> {
        require_local_file_access(ctx)?;
        let gw = base64::engine::general_purpose::STANDARD
            .decode(&input.gw_base64)
            .map_err(|error| async_graphql::Error::new(format!("invalid .gw base64: {error}")))?;
        let deposit_sizes = geneanet_deposit_sizes(&input.deposit_sizes)?;
        let (archives, _) = crate::service::geneanet::index_archives(&input.archive_paths);
        Ok(crate::service::geneanet::plan(
            &gw,
            &input.file_name,
            &input.collection,
            &deposit_sizes,
            &archives,
            input.media_fidelity.into(),
        )?
        .into_iter()
        .map(Into::into)
        .collect())
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
        let tid = live_tree(ctx, &tree_id).await?;
        let pid = uuid(&person_id)?;
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
        let tid = live_tree(ctx, &tree_id).await?;
        let persons = profiles.get_all_persons(db, tid).await?;
        Ok(persons.into_iter().map(Into::into).collect())
    }

    /// Server-side person search in a tree (spec name: `searchPersons`).
    ///
    /// Backed by the `person_search_fts` DB table (SQLite FTS5 / PostgreSQL)
    /// with accent-folded, normalised matching. Returns paginated results.
    #[allow(clippy::too_many_arguments)]
    async fn search_persons(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        query: String,
        #[graphql(default_with = "crate::profile::service::SEARCH_DEFAULT_LIMIT")] limit: usize,
        #[graphql(default = 0)] offset: usize,
        sex: Option<super::types::GqlSex>,
        surname: Option<String>,
        given_names: Option<String>,
        occupation: Option<String>,
        spouse_surname: Option<String>,
        spouse_given_names: Option<String>,
        father_surname: Option<String>,
        father_given_names: Option<String>,
        mother_surname: Option<String>,
        mother_given_names: Option<String>,
        birth_from: Option<i32>,
        birth_to: Option<i32>,
        death_from: Option<i32>,
        death_to: Option<i32>,
        place: Option<String>,
        event_type: Option<GqlEventType>,
        event_from: Option<i32>,
        event_to: Option<i32>,
        #[graphql(default = false)] has_media: bool,
        sort: Option<GqlPersonSearchSort>,
    ) -> Result<GqlSearchResult> {
        let profiles = profiles_from_ctx(ctx);
        let tid = live_tree(ctx, &tree_id).await?;
        let filters = PersonSearchFilters {
            sex: sex.map(Into::into),
            surname,
            given_names,
            occupation,
            spouse_surname,
            spouse_given_names,
            father_surname,
            father_given_names,
            mother_surname,
            mother_given_names,
            birth_from,
            birth_to,
            death_from,
            death_to,
            place,
            event_type: event_type.map(Into::into),
            event_from,
            event_to,
            has_media,
        };
        let result = profiles
            .search_filtered(
                tid,
                &query,
                &filters,
                sort.map(Into::into).unwrap_or_default(),
                limit,
                offset,
            )
            .await?;
        Ok(result.into())
    }

    /// The other persons of the tree bearing the same folded primary surname
    /// and given names, less those already confirmed to be somebody else.
    async fn person_homonyms(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        person_id: ID,
    ) -> Result<Vec<GqlSearchEntry>> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let tid = live_tree(ctx, &tree_id).await?;
        let pid = uuid(&person_id)?;
        PersonRepo::get_in_tree(db, tid, pid).await?;
        let homonyms = profiles.homonyms(tid, pid).await?;
        Ok(homonyms.into_iter().map(Into::into).collect())
    }

    /// The persons of the tree modified most recently, newest first.
    /// Mirrors `GET /trees/{treeId}/persons/recently-modified`.
    async fn recently_modified_persons(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        #[graphql(default_with = "crate::service::history::RECENT_PERSONS_DEFAULT_LIMIT")]
        limit: usize,
    ) -> Result<Vec<GqlSearchEntry>> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let tid = live_tree(ctx, &tree_id).await?;
        let persons =
            crate::service::history::recently_modified_persons(db, profiles, tid, limit).await?;
        Ok(persons.into_iter().map(Into::into).collect())
    }

    /// Every way found to go from a person to another: their blood
    /// relationships, or the shortest paths through unions when they share
    /// no ancestor.
    async fn kinship(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        person_id: ID,
        other_person_id: ID,
    ) -> Result<GqlKinship> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let kinship = crate::service::kinship::find_kinship(
            db,
            profiles,
            live_tree(ctx, &tree_id).await?,
            uuid(&person_id)?,
            uuid(&other_person_id)?,
        )
        .await?;
        Ok(kinship.into())
    }

    /// Get a windowed pedigree for a root person.
    ///
    /// Returns nodes and edges within the given ancestor / descendant depth,
    /// assembled on demand from family links and the stored projections.
    async fn pedigree(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        root_person_id: ID,
        ancestor_depth: i32,
        descendant_depth: i32,
    ) -> Result<GqlPedigree> {
        let profiles = profiles_from_ctx(ctx);
        let tid = live_tree(ctx, &tree_id).await?;
        let rid = uuid(&root_person_id)?;
        let pedigree = crate::service::pedigrees::pedigree(
            profiles,
            tid,
            rid,
            ancestor_depth.into(),
            descendant_depth.into(),
        )
        .await?;
        Ok(pedigree.into())
    }

    /// Assemble several pedigrees in one operation, for a screen that draws one
    /// small pedigree per row. Mirrors REST's `pedigrees` endpoint.
    async fn pedigrees(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        root_person_ids: Vec<ID>,
        ancestor_depth: i32,
        descendant_depth: i32,
    ) -> Result<Vec<GqlPedigreeEntry>> {
        let root_person_ids = uuids(&root_person_ids)?;
        Ok(crate::service::pedigrees::load_pedigrees(
            profiles_from_ctx(ctx),
            live_tree(ctx, &tree_id).await?,
            &root_person_ids,
            ancestor_depth.into(),
            descendant_depth.into(),
        )
        .await?
        .into_iter()
        .map(Into::into)
        .collect())
    }
}
