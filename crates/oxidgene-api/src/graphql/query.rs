//! GraphQL query root with all read operations.

use async_graphql::{Context, ID, Object, Result};
use base64::Engine as _;
use oxidgene_geneanet::archive::LocalOriginals;
use uuid::Uuid;

use oxidgene_db::repo::{
    AncestryRepo, DictionaryRepo, EventFilter, EventRepo, FamilyChildRepo, FamilyRepo,
    FamilySpouseRepo, MediaLinkRepo, MediaLinkTarget, MediaRepo, PaginationParams, PersonNameRepo,
    PersonRepo, PlaceRepo, SOURCE_DRILL_THRESHOLD, SourceRepo, TreeRepo, VignetteRepo,
};

use super::inputs::{GeneanetPreviewInput, geneanet_deposit_sizes};
use super::types::{
    GqlDictionaryEntry, GqlEvent, GqlEventConnection, GqlEventType, GqlExportGedcomResult,
    GqlExportGedzipResult, GqlFamily, GqlFamilyConnection, GqlGeneanetArchiveIndex,
    GqlGeneanetImportPhase, GqlGeneanetImportProgress, GqlGeneanetIndexedArchive,
    GqlGeneanetInspection, GqlGeneanetNeededMedia, GqlGeneanetPreview, GqlGivenNameReference,
    GqlMedia, GqlMediaConnection, GqlMediaLink, GqlMediaWithLink, GqlOccupationReference,
    GqlPedigree, GqlPerson, GqlPersonConnection, GqlPersonProfile, GqlPersonUsageEntry,
    GqlPersonWithDepth, GqlPlace, GqlPlaceConnection, GqlPlaceDictionaryEntry, GqlPortrait,
    GqlSearchResult, GqlSource, GqlSourceConnection, GqlSourceDictionaryDrill,
    GqlSourceDictionaryEntry, GqlSourceDictionaryGroup, GqlTree, GqlTreeConnection,
    GqlTreeSnapshot, GqlVignette, db_from_ctx, imports_from_ctx, media_from_ctx, profiles_from_ctx,
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
        let _tid = Uuid::parse_str(tree_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
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
        let _tid = Uuid::parse_str(tree_id.as_str())?;
        Ok(MediaRepo::can_purge_if_unreferenced_elsewhere(
            db,
            Uuid::parse_str(id.as_str())?,
            Uuid::parse_str(allowed_link_id.as_str())?,
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
        let _tid = Uuid::parse_str(tree_id.as_str())?;
        let target = MediaLinkTarget::parse(&entity_type).ok_or_else(|| {
            async_graphql::Error::new(format!(
                "unknown entityType `{entity_type}`; expected person, family, event or source"
            ))
        })?;
        let rows = MediaLinkRepo::list_with_media(db, target, Uuid::parse_str(entity_id.as_str())?)
            .await?;
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
        let _tid = Uuid::parse_str(tree_id.as_str())?;
        let links = MediaLinkRepo::list_by_media(db, Uuid::parse_str(media_id.as_str())?).await?;
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
        let _tid = Uuid::parse_str(tree_id.as_str())?;
        let pages = MediaRepo::list_pages(db, Uuid::parse_str(media_id.as_str())?).await?;
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
        let _tid = Uuid::parse_str(tree_id.as_str())?;
        let mid = Uuid::parse_str(media_id.as_str())?;
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
        let _tid = Uuid::parse_str(tree_id.as_str())?;
        let vignettes = match (person_id, event_id) {
            (Some(person_id), None) => {
                VignetteRepo::list_for_person(db, Uuid::parse_str(person_id.as_str())?).await?
            }
            (None, Some(event_id)) => {
                VignetteRepo::list_for_event(db, Uuid::parse_str(event_id.as_str())?).await?
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
        let _tid = Uuid::parse_str(tree_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
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

    /// Export a GEDZIP archive containing GEDCOM and stored media, encoded as
    /// base64 for GraphQL's JSON transport.
    async fn export_gedzip(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        merge_occupations: Option<bool>,
        merge_names: Option<bool>,
    ) -> Result<GqlExportGedzipResult> {
        use base64::Engine as _;

        let db = db_from_ctx(ctx);
        let media = media_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let data = crate::service::gedcom::load_and_export(
            db,
            tid,
            merge_occupations.unwrap_or(false),
            merge_names.unwrap_or(false),
            true,
        )
        .await?;
        let mut files = Vec::with_capacity(data.media_files.len());
        for (key, path) in &data.media_files {
            match media.get(key).await {
                Ok(bytes) => files.push((path.clone(), bytes)),
                Err(error) => {
                    tracing::warn!(%key, %error, "media absent from the store; not packed")
                }
            }
        }
        let archive = oxidgene_gedcom::export::export_gedzip(&data.gedcom, &files)
            .map_err(oxidgene_core::OxidGeneError::Gedcom)?;

        Ok(GqlExportGedzipResult {
            gedzip_base64: base64::engine::general_purpose::STANDARD.encode(archive),
            warnings: data.warnings,
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

    /// Index local Geneanet archives by path. This is desktop-only in practice.
    async fn index_geneanet_archives(&self, paths: Vec<String>) -> Result<GqlGeneanetArchiveIndex> {
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
    async fn geneanet_preview(&self, input: GeneanetPreviewInput) -> Result<GqlGeneanetPreview> {
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
        input: GeneanetPreviewInput,
    ) -> Result<Vec<GqlGeneanetNeededMedia>> {
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

    /// Report the state of a currently running Geneanet import.
    async fn geneanet_import_progress(
        &self,
        ctx: &Context<'_>,
        progress_id: ID,
    ) -> Result<Option<GqlGeneanetImportProgress>> {
        let progress_id = Uuid::parse_str(progress_id.as_str())?;
        let progress = imports_from_ctx(ctx)
            .lock()
            .ok()
            .and_then(|imports| imports.get(&progress_id).cloned());
        Ok(progress.map(|progress| {
            let (phase, done, total) = progress.read();
            GqlGeneanetImportProgress {
                phase: GqlGeneanetImportPhase::from(phase),
                done: done as i64,
                total: total as i64,
            }
        }))
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
