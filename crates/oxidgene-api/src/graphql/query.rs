//! GraphQL query root with all read operations.

use async_graphql::{Context, ID, Object, Result};
use base64::Engine as _;
use oxidgene_geneanet::archive::LocalOriginals;
use uuid::Uuid;

use crate::rest::state::{TreeResource, require_tree_resource};

use oxidgene_db::repo::{
    AncestryRepo, BackgroundJobKind, BackgroundJobRepo, BackgroundJobStatus, CitationFilter,
    CitationRepo, DictionaryRepo, EventFilter, EventRepo, FamilyChildRepo, FamilyRepo,
    FamilySpouseRepo, MediaLinkRepo, MediaLinkTarget, MediaRepo, NoteFilter, NoteRepo,
    PaginationParams, PersonNameRepo, PersonRepo, PersonSearchFilters, PersonSearchSort, PlaceRepo,
    SOURCE_DRILL_THRESHOLD, SourceRepo, TreeRepo, VignetteRepo,
};

use super::inputs::{GeneanetPreviewInput, geneanet_deposit_sizes};
use super::types::{
    GqlCitationConnection, GqlDictionaryEntry, GqlEvent, GqlEventConnection, GqlEventType,
    GqlExportGedcomResult, GqlExportJobStatus, GqlFamily, GqlFamilyConnection,
    GqlGeneanetArchiveIndex, GqlGeneanetImportResult, GqlGeneanetIndexedArchive,
    GqlGeneanetInspection, GqlGeneanetNeededMedia, GqlGeneanetPreview, GqlGivenNameReference,
    GqlImportJobStatus, GqlImportResult, GqlMedia, GqlMediaConnection, GqlMediaLink,
    GqlMediaWithLink, GqlNoteConnection, GqlOccupationReference, GqlPedigree, GqlPerson,
    GqlPersonConnection, GqlPersonProfile, GqlPersonSearchSort, GqlPersonUsageEntry,
    GqlPersonWithDepth, GqlPlace, GqlPlaceConnection, GqlPlaceDictionaryEntry, GqlPortrait,
    GqlSearchResult, GqlSource, GqlSourceConnection, GqlSourceDictionaryDrill,
    GqlSourceDictionaryEntry, GqlSourceDictionaryGroup, GqlTree, GqlTreeConnection,
    GqlTreeMediaLink, GqlTreeSnapshot, GqlVignette, db_from_ctx, profiles_from_ctx,
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
        search: Option<String>,
    ) -> Result<GqlPersonConnection> {
        let db = db_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
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
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        match PersonRepo::get_in_tree(db, tid, uuid).await {
            Ok(p) => Ok(Some(p.into())),
            Err(oxidgene_core::OxidGeneError::NotFound { .. }) => Ok(None),
            Err(e) => Err(e.into()),
        }
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
        let db = db_from_ctx(ctx);
        let person = crate::rest::person::resolve_sosa_number(
            db,
            Uuid::parse_str(tree_id.as_str())?,
            number,
        )
        .await?;
        Ok(person.map(Into::into))
    }

    /// Every person's selected portrait, with enough data for a pedigree to
    /// choose its thumbnail, original file or cropped vignette endpoint.
    async fn portraits(&self, ctx: &Context<'_>, tree_id: ID) -> Result<Vec<GqlPortrait>> {
        let db = db_from_ctx(ctx);
        let portraits = PersonRepo::list_portraits(db, Uuid::parse_str(tree_id.as_str())?).await?;
        Ok(portraits.into_iter().map(Into::into).collect())
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
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let pid = Uuid::parse_str(person_id.as_str())?;
        require_tree_resource(db, tid, TreeResource::Person, pid).await?;
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
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let pid = Uuid::parse_str(person_id.as_str())?;
        require_tree_resource(db, tid, TreeResource::Person, pid).await?;
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
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        if !tree_resource_exists(db, tid, TreeResource::Family, uuid).await? {
            return Ok(None);
        }
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
        if let Some(person_id) = filter.person_id {
            require_tree_resource(db, tid, TreeResource::Person, person_id).await?;
        }
        if let Some(family_id) = filter.family_id {
            require_tree_resource(db, tid, TreeResource::Family, family_id).await?;
        }
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
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        if !tree_resource_exists(db, tid, TreeResource::Event, uuid).await? {
            return Ok(None);
        }
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
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        if !tree_resource_exists(db, tid, TreeResource::Place, uuid).await? {
            return Ok(None);
        }
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
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        if !tree_resource_exists(db, tid, TreeResource::Source, uuid).await? {
            return Ok(None);
        }
        match SourceRepo::get(db, uuid).await {
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
        let tree_id = Uuid::parse_str(tree_id.as_str())?;
        let filter = CitationFilter {
            person_id: person_id
                .map(|id| Uuid::parse_str(id.as_str()))
                .transpose()?,
            event_id: event_id
                .map(|id| Uuid::parse_str(id.as_str()))
                .transpose()?,
            family_id: family_id
                .map(|id| Uuid::parse_str(id.as_str()))
                .transpose()?,
            source_id: source_id
                .map(|id| Uuid::parse_str(id.as_str()))
                .transpose()?,
        };
        let params = PaginationParams {
            first: first.unwrap_or(25),
            after,
        };
        Ok(CitationRepo::list(db, tree_id, &filter, &params)
            .await?
            .into())
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
        let tree_id = Uuid::parse_str(tree_id.as_str())?;
        let filter = NoteFilter {
            person_id: person_id
                .map(|id| Uuid::parse_str(id.as_str()))
                .transpose()?,
            event_id: event_id
                .map(|id| Uuid::parse_str(id.as_str()))
                .transpose()?,
            family_id: family_id
                .map(|id| Uuid::parse_str(id.as_str()))
                .transpose()?,
            source_id: source_id
                .map(|id| Uuid::parse_str(id.as_str()))
                .transpose()?,
            media_id: media_id
                .map(|id| Uuid::parse_str(id.as_str()))
                .transpose()?,
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
            DictionaryRepo::family_names(db, Uuid::parse_str(tree_id.as_str())?)
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
            DictionaryRepo::occupations(db, Uuid::parse_str(tree_id.as_str())?)
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
            Uuid::parse_str(tree_id.as_str())?,
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
            Uuid::parse_str(tree_id.as_str())?,
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

    /// Places with their event and media usage count.
    async fn dictionary_places(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
    ) -> Result<Vec<GqlPlaceDictionaryEntry>> {
        let db = db_from_ctx(ctx);
        let entries =
            DictionaryRepo::places_with_usage(db, Uuid::parse_str(tree_id.as_str())?).await?;
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
            Uuid::parse_str(tree_id.as_str())?,
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
            Uuid::parse_str(tree_id.as_str())?,
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
        source_id: ID,
    ) -> Result<Vec<GqlPersonUsageEntry>> {
        let db = db_from_ctx(ctx);
        let ids = DictionaryRepo::source_usage_person_ids(db, Uuid::parse_str(source_id.as_str())?)
            .await?;
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
        place_id: ID,
    ) -> Result<Vec<GqlPersonUsageEntry>> {
        let db = db_from_ctx(ctx);
        let ids =
            DictionaryRepo::place_usage_person_ids(db, Uuid::parse_str(place_id.as_str())?).await?;
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
        let language = crate::reference::ReferenceLang::from_code(&language)
            .ok_or_else(|| async_graphql::Error::new("language must be `fr` or `en`"))?;
        Ok(crate::reference::lookup_occupation(language, &term).map(Into::into))
    }

    /// Resolve static given-name reference content for `fr` or `en`.
    async fn given_name_reference(
        &self,
        _ctx: &Context<'_>,
        language: String,
        term: String,
    ) -> Result<Option<GqlGivenNameReference>> {
        let language = crate::reference::ReferenceLang::from_code(&language)
            .ok_or_else(|| async_graphql::Error::new("language must be `fr` or `en`"))?;
        Ok(crate::reference::lookup_given_name(language, &term).map(Into::into))
    }

    /// Return the legacy all-at-once tree snapshot.
    async fn tree_snapshot(&self, ctx: &Context<'_>, tree_id: ID) -> Result<GqlTreeSnapshot> {
        let db = db_from_ctx(ctx);
        let tree_id = Uuid::parse_str(tree_id.as_str())?;
        let (persons, families) = tokio::try_join!(
            PersonRepo::list_all(db, tree_id),
            FamilyRepo::list_all(db, tree_id),
        )?;
        let person_ids: Vec<Uuid> = persons.iter().map(|person| person.id).collect();
        let family_ids: Vec<Uuid> = families.iter().map(|family| family.id).collect();
        let (names, events, places, spouses, children) = tokio::try_join!(
            PersonNameRepo::list_by_persons(db, &person_ids),
            EventRepo::list_all(db, tree_id),
            PlaceRepo::list_all(db, tree_id),
            FamilySpouseRepo::list_by_families(db, &family_ids),
            FamilyChildRepo::list_by_families(db, &family_ids),
        )?;
        Ok(GqlTreeSnapshot {
            persons: persons.into_iter().map(Into::into).collect(),
            names: names.into_iter().map(Into::into).collect(),
            events: events.into_iter().map(Into::into).collect(),
            places: places.into_iter().map(Into::into).collect(),
            spouses: spouses.into_iter().map(Into::into).collect(),
            children: children.into_iter().map(Into::into).collect(),
        })
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
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        if !tree_resource_exists(db, tid, TreeResource::Media, uuid).await? {
            return Ok(None);
        }
        match MediaRepo::get(db, uuid).await {
            Ok(m) => Ok(Some(m.into())),
            Err(oxidgene_core::OxidGeneError::NotFound { .. }) => Ok(None),
            Err(e) => Err(e.into()),
        }
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
        let db = db_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let media_id = Uuid::parse_str(id.as_str())?;
        let link_id = Uuid::parse_str(allowed_link_id.as_str())?;
        require_tree_resource(db, tid, TreeResource::Media, media_id).await?;
        require_tree_resource(db, tid, TreeResource::MediaLink, link_id).await?;
        Ok(MediaRepo::can_purge_if_unreferenced_elsewhere(db, media_id, link_id).await?)
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
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let target = MediaLinkTarget::parse(&entity_type).ok_or_else(|| {
            async_graphql::Error::new(format!(
                "unknown entityType `{entity_type}`; expected person, family, event or source"
            ))
        })?;
        let entity_id = Uuid::parse_str(entity_id.as_str())?;
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
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let media_id = Uuid::parse_str(media_id.as_str())?;
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
        let tree_id = Uuid::parse_str(tree_id.as_str())?;
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
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let media_id = Uuid::parse_str(media_id.as_str())?;
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
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let mid = Uuid::parse_str(media_id.as_str())?;
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
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let vignettes = match (person_id, event_id) {
            (Some(person_id), None) => {
                let person_id = Uuid::parse_str(person_id.as_str())?;
                require_tree_resource(db, tid, TreeResource::Person, person_id).await?;
                VignetteRepo::list_for_person(db, person_id).await?
            }
            (None, Some(event_id)) => {
                let event_id = Uuid::parse_str(event_id.as_str())?;
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
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        if !tree_resource_exists(db, tid, TreeResource::Vignette, uuid).await? {
            return Ok(None);
        }
        match VignetteRepo::get(db, uuid).await {
            Ok(v) => Ok(Some(v.into())),
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
            // GraphQL hands back the GEDCOM text; there is no archive to
            // reference, so the media keep their producers' paths.
            false,
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
        let tree_id = Uuid::parse_str(tree_id.as_str())?;
        let job_id = Uuid::parse_str(job_id.as_str())?;
        if let Some(progress) = crate::service::background_job::live_job_progress(
            tree_id,
            job_id,
            BackgroundJobKind::Export,
        ) {
            return Ok(GqlExportJobStatus {
                phase: progress.phase,
                done: progress.done,
                total: progress.total,
                download_url: None,
                warnings: Vec::new(),
                error: None,
            });
        }

        let db = db_from_ctx(ctx);
        let job = BackgroundJobRepo::get_in_tree(db, tree_id, job_id).await?;
        if job.kind != BackgroundJobKind::Export.as_str() {
            return Err(oxidgene_core::OxidGeneError::NotFound {
                entity: "ExportJob",
                id: job_id,
            }
            .into());
        }
        let warnings = job
            .result_json
            .as_deref()
            .and_then(|result| serde_json::from_str::<serde_json::Value>(result).ok())
            .and_then(|result| result.get("warnings").cloned())
            .and_then(|warnings| serde_json::from_value(warnings).ok())
            .unwrap_or_default();
        let download_url = (job.status == BackgroundJobStatus::Completed.as_str())
            .then(|| format!("/api/v1/trees/{tree_id}/export-jobs/{job_id}/download"));
        Ok(GqlExportJobStatus {
            phase: job.phase,
            done: job.done,
            total: job.total,
            download_url,
            warnings,
            error: job.error_code,
        })
    }

    /// Poll a durable genealogy file import created by `startFileImportJob`.
    async fn import_job_status(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        job_id: ID,
    ) -> Result<GqlImportJobStatus> {
        let tree_id = Uuid::parse_str(tree_id.as_str())?;
        let job_id = Uuid::parse_str(job_id.as_str())?;
        if let Some(progress) = crate::service::background_job::live_job_progress(
            tree_id,
            job_id,
            BackgroundJobKind::Import,
        ) {
            return Ok(GqlImportJobStatus {
                phase: progress.phase,
                done: progress.done,
                total: progress.total,
                result: None,
                geneanet_result: None,
                error: None,
            });
        }

        let db = db_from_ctx(ctx);
        let job = BackgroundJobRepo::get_in_tree(db, tree_id, job_id).await?;
        if job.kind != BackgroundJobKind::Import.as_str() {
            return Err(oxidgene_core::OxidGeneError::NotFound {
                entity: "ImportJob",
                id: job_id,
            }
            .into());
        }
        let (result, geneanet_result) = if job.format == "geneanet" {
            let summary = job
                .result_json
                .as_deref()
                .map(serde_json::from_str::<crate::service::geneanet::GeneanetImportSummary>)
                .transpose()
                .map_err(|error| async_graphql::Error::new(error.to_string()))?;
            (
                None,
                summary.map(|summary| GqlGeneanetImportResult {
                    persons_count: summary.persons_count as i64,
                    families_count: summary.families_count as i64,
                    events_count: summary.events_count as i64,
                    sources_count: summary.sources_count as i64,
                    places_count: summary.places_count as i64,
                    notes_count: summary.notes_count as i64,
                    media_count: summary.media_count as i64,
                    links_count: summary.links_count as i64,
                    portraits_count: summary.portraits_count as i64,
                    isolated_count: summary.isolated_count as i64,
                    vignettes_count: summary.vignettes_count as i64,
                    skipped: summary.skipped,
                    warnings: summary.warnings,
                }),
            )
        } else {
            let summary = job
                .result_json
                .as_deref()
                .map(serde_json::from_str::<crate::service::gedcom::ImportSummary>)
                .transpose()
                .map_err(|error| async_graphql::Error::new(error.to_string()))?;
            (
                summary.map(|summary| GqlImportResult {
                    persons_count: summary.persons_count as i32,
                    families_count: summary.families_count as i32,
                    events_count: summary.events_count as i32,
                    sources_count: summary.sources_count as i32,
                    media_count: summary.media_count as i32,
                    places_count: summary.places_count as i32,
                    notes_count: summary.notes_count as i32,
                    warnings: summary.warnings,
                }),
                None,
            )
        };
        Ok(GqlImportJobStatus {
            phase: job.phase,
            done: job.done,
            total: job.total,
            result,
            geneanet_result,
            error: job.error_code,
        })
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
    #[allow(clippy::too_many_arguments)]
    async fn search_persons(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        query: String,
        #[graphql(default = 25)] limit: usize,
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
        let tid = Uuid::parse_str(tree_id.as_str())?;
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
                sort.map(Into::into).unwrap_or(PersonSearchSort::Relevance),
                limit.min(100),
                offset,
            )
            .await?;
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
