//! Resolving a cited source to the page of its archive's portal that shows
//! the cited register (docs/archives.md §5.3).
//!
//! The citation is recognized from everything the tree records about it
//! (docs/archives.md §5.1): the source, the citation's page and text, the
//! repositories holding the source with their call numbers, the source's
//! linked media addresses, and the cited event with its place — with the
//! place dictionary telling a locality from a parish or a hamlet. The
//! process shares one [`Resolver`], whose session cache spares a second
//! request for a citation already resolved. Portals whose access is `any`
//! are searched over the native transport; a `browser` portal, which only a
//! browser page passes, yields its offline filtered results.

use std::fmt;
use std::sync::{Arc, OnceLock};

use oxidgene_archives::platform::BoxFuture;
use oxidgene_archives::{
    ArchiveRegistry, ArchiveTarget, CitationEvidence, CitationParts, CitedView, FetchError, HeldAt,
    NativeTransport, PlaceLookup, PortalEndpoint, PortalFetch, PortalTransport, Recognition,
    ResolveError, Resolver, SuppliedParts, Unrecognized,
};
use oxidgene_core::error::{ArchiveFailure, OxidGeneError};
use oxidgene_core::types::{Citation, Source};
use oxidgene_db::repo::{
    CitationRepo, EventRepo, MediaLinkRepo, MediaLinkTarget, MediaRepo, PlaceRepo, RepositoryRepo,
    SourceRepo, SourceRepositoryRepo,
};
use sea_orm::ConnectionTrait;
use tracing::warn;
use uuid::Uuid;

use crate::service::scope::{TreeResource, require_tree_resource};

/// The process's archive resolution: the embedded catalogue's resolver and
/// the transport its requests take.
pub struct ArchivePortals {
    resolver: Resolver<'static>,
    /// Built on the first resolution, so a process that never resolves a
    /// citation holds no HTTP client.
    transport: OnceLock<Arc<dyn PortalTransport>>,
}

impl ArchivePortals {
    /// Over the native transport.
    pub fn native() -> Self {
        Self {
            resolver: Resolver::new(ArchiveRegistry::embedded()),
            transport: OnceLock::new(),
        }
    }

    /// Over `transport` instead: a recorded portal, in tests.
    pub fn with_transport(transport: Arc<dyn PortalTransport>) -> Self {
        let portals = Self::native();
        let _ = portals.transport.set(transport);
        portals
    }

    fn transport(&self) -> &dyn PortalTransport {
        self.transport
            .get_or_init(|| match NativeTransport::new() {
                Ok(transport) => Arc::new(transport),
                Err(_) => {
                    warn!(
                        error = "archive_transport",
                        "could not build the archive portal client"
                    );
                    Arc::new(Unavailable)
                }
            })
            .as_ref()
    }
}

impl Default for ArchivePortals {
    fn default() -> Self {
        Self::native()
    }
}

impl fmt::Debug for ArchivePortals {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ArchivePortals").finish_non_exhaustive()
    }
}

/// The transport when no HTTP client could be built: every portal is
/// unreachable.
struct Unavailable;

impl PortalTransport for Unavailable {
    fn is_browser(&self) -> bool {
        false
    }

    fn connect<'a>(
        &'a self,
        _endpoint: &'a PortalEndpoint,
    ) -> BoxFuture<'a, Result<Box<dyn PortalFetch + 'a>, FetchError>> {
        Box::pin(async { Err(FetchError::Network) })
    }
}

/// The place dictionary, as recognizing a citation consults it.
pub struct DictionaryPlaces;

impl PlaceLookup for DictionaryPlaces {
    fn areas(&self, names: &[String]) -> Vec<Vec<String>> {
        crate::reference::place_areas(names)
    }
}

/// Where source `source_id` of tree `tree_id` opens on its archive's portal,
/// as cited by `citation_id` when given: a citation of that source, whose
/// page, text and event complete what the source says.
///
/// `view`, when given, resolves that one view of the cited register instead
/// of the cited views: the reader paging to the previous or next view in
/// OxidGene's viewer, one resolution per click (docs/archives.md §6.3, §8).
/// It keeps the side the citation gives that view, if it cites it; a view
/// below 1 or beyond the cited view count is a validation error.
///
/// `supplied` holds what the reader completed in the "Find in the archives"
/// dialog, which wins over what the records say; parts the recognized
/// archive cannot search are a validation error. A citation whose archive is
/// known but whose act or locality is not, and that the reader did not
/// complete, opens the archive's filtered search page without a request; a
/// portal address of the archive found in the records is the target as it
/// is.
///
/// A source or citation that is absent, deleted or of another tree, and a
/// citation of another source, are `NotFound`. Records naming no archive
/// register, or a register no catalogued collection holds, and a failed
/// resolution are [`OxidGeneError::Archive`]; a failure is logged with its
/// code and the archive's identifier only, never the citation.
pub async fn archive_target(
    db: &impl ConnectionTrait,
    portals: &ArchivePortals,
    tree_id: Uuid,
    source_id: Uuid,
    citation_id: Option<Uuid>,
    view: Option<u16>,
    supplied: Option<SuppliedParts>,
) -> Result<ArchiveTarget, OxidGeneError> {
    let (source, citation) = cited(db, tree_id, source_id, citation_id).await?;
    let evidence = load_evidence(db, &source, citation.as_ref()).await?;

    let registry = portals.resolver.registry();
    let span = tracing::info_span!("archive.recognize");
    let reader = supplied.clone();
    let recognition = crate::service::blocking::run(span, move || {
        registry.recognize(&evidence, reader.as_ref(), Some(&DictionaryPlaces))
    })
    .await?
    .map_err(|unrecognized| {
        OxidGeneError::Archive(match unrecognized {
            Unrecognized::NotACitation => ArchiveFailure::NotACitation,
            Unrecognized::NoAdapter => ArchiveFailure::NoAdapter,
        })
    })?;
    if let Some(supplied) = &supplied {
        supplied
            .validate(recognition.archive)
            .map_err(OxidGeneError::Validation)?;
    }
    if let Some(address) = &recognition.address {
        return Ok(ArchiveTarget::View {
            url: address.clone(),
            views: Vec::new(),
            view_count: None,
            call_number: recognition
                .found
                .call_number
                .as_ref()
                .map(|call| call.as_str().to_owned()),
            attribution: None,
        });
    }
    let Some(mut citation) = recognition.citation() else {
        return Ok(unfinished(registry, &recognition));
    };
    if let Some(view) = view {
        citation.views = vec![one_view(&citation, view)?];
    }
    let archive = recognition.archive;
    portals
        .resolver
        .resolve(&citation, portals.transport())
        .await
        .map_err(|error| {
            warn!(
                error = error.code(),
                archive = archive.id.as_str(),
                "could not resolve the cited register"
            );
            OxidGeneError::Archive(failure_of(&error))
        })
}

/// The parts a reader supplied, as both surfaces receive them: the
/// document kind as its code (`N`, `BMS`, `TD`, `RP`). An unknown code is a
/// validation error; whether the archive holds it is checked once the
/// archive is recognized.
pub fn supplied_parts(
    locality: Option<String>,
    act: Option<&str>,
    year: Option<u16>,
    view: Option<u16>,
) -> Result<SuppliedParts, OxidGeneError> {
    let act = act
        .map(|code| {
            oxidgene_archives::Act::from_code(code.trim()).ok_or_else(|| {
                OxidGeneError::Validation(format!("`{code}` is not a document kind"))
            })
        })
        .transpose()?;
    Ok(SuppliedParts {
        locality,
        act,
        year,
        view,
    })
}

/// Source `source_id` of tree `tree_id`, and its citation `citation_id`
/// when given: `NotFound` for one absent, deleted or of another tree, and
/// for a citation of another source.
async fn cited(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    source_id: Uuid,
    citation_id: Option<Uuid>,
) -> Result<(Source, Option<Citation>), OxidGeneError> {
    require_tree_resource(db, tree_id, TreeResource::Source, source_id).await?;
    let source = SourceRepo::get(db, source_id).await?;
    let Some(id) = citation_id else {
        return Ok((source, None));
    };
    require_tree_resource(db, tree_id, TreeResource::Citation, id).await?;
    let citation = CitationRepo::get(db, id).await?;
    if citation.source_id != source_id {
        return Err(OxidGeneError::NotFound {
            entity: "Citation",
            id,
        });
    }
    Ok((source, Some(citation)))
}

/// The target of a citation still missing its act or its locality: the
/// archive's search page filtered by what is known, built without a
/// request, or its website without a document kind.
fn unfinished(registry: &ArchiveRegistry, recognition: &Recognition<'_>) -> ArchiveTarget {
    recognition
        .search()
        .and_then(|parts| registry.offline_target(&parts).ok())
        .unwrap_or_else(|| ArchiveTarget::Results {
            url: recognition.archive.website.clone(),
            matches: None,
        })
}

/// The web addresses of the media linked to each of `source_ids`, held as
/// links: a remote page linked itself, or the remote pages of a linked
/// document — the views attached from an archive (docs/archives.md §6.4).
pub async fn source_addresses(
    db: &impl ConnectionTrait,
    source_ids: &[Uuid],
) -> Result<Vec<(Uuid, String)>, OxidGeneError> {
    let linked =
        MediaLinkRepo::list_with_media_for_many(db, MediaLinkTarget::Source, source_ids).await?;
    let documents: Vec<Uuid> = linked
        .iter()
        .filter(|(_, media)| media.is_document())
        .map(|(_, media)| media.id)
        .collect();
    let pages = MediaRepo::list_pages_for(db, &documents).await?;
    let remote = |path: &str| path.starts_with("https://") || path.starts_with("http://");
    let mut addresses = Vec::new();
    for (link, media) in &linked {
        let Some(source_id) = link.source_id else {
            continue;
        };
        if media.is_document() {
            addresses.extend(
                pages
                    .iter()
                    .filter(|page| {
                        page.parent_media_id == Some(media.id) && remote(&page.file_path)
                    })
                    .map(|page| (source_id, page.file_path.clone())),
            );
        } else if remote(&media.file_path) {
            addresses.push((source_id, media.file_path.clone()));
        }
    }
    Ok(addresses)
}

/// Everything the tree records about a citation of `source`: the source,
/// the citation's page and text, the repositories holding the source with
/// their call numbers and websites, the addresses of the source's linked
/// media, and the cited event with its place.
async fn load_evidence(
    db: &impl ConnectionTrait,
    source: &Source,
    citation: Option<&Citation>,
) -> Result<CitationEvidence, OxidGeneError> {
    let links = SourceRepositoryRepo::list_by_source(db, source.id).await?;
    let repository_ids: Vec<Uuid> = links.iter().map(|link| link.repository_id).collect();
    let repositories = RepositoryRepo::get_many(db, &repository_ids).await?;
    let held = links
        .iter()
        .filter_map(|link| {
            let repository = repositories
                .iter()
                .find(|repository| repository.id == link.repository_id)?;
            Some(HeldAt {
                name: repository.name.clone(),
                call_number: link.call_number.clone(),
                website: repository.website.clone(),
            })
        })
        .collect();
    let urls = source_addresses(db, &[source.id])
        .await?
        .into_iter()
        .map(|(_, url)| url)
        .collect();
    let mut evidence = CitationEvidence::new(source, citation)
        .with_repositories(held)
        .with_urls(urls);
    if let Some(event_id) = citation.and_then(|citation| citation.event_id) {
        let event = EventRepo::get(db, event_id).await?;
        let place = match event.place_id {
            Some(place_id) => Some(PlaceRepo::get(db, place_id).await?.name),
            None => None,
        };
        evidence = evidence.with_event(&event, place.as_deref());
    }
    Ok(evidence)
}

/// View `view` of the cited register, with the side the citation gives it.
fn one_view(citation: &CitationParts, view: u16) -> Result<CitedView, OxidGeneError> {
    if view == 0 || citation.view_count.is_some_and(|count| view > count) {
        return Err(OxidGeneError::Validation(
            "view must be between 1 and the register's view count".to_string(),
        ));
    }
    Ok(citation
        .views
        .iter()
        .find(|cited| cited.view == view)
        .copied()
        .unwrap_or(CitedView { view, side: None }))
}

fn failure_of(error: &ResolveError) -> ArchiveFailure {
    match error {
        ResolveError::NoAdapter => ArchiveFailure::NoAdapter,
        ResolveError::UnexpectedResponse(_) => ArchiveFailure::UnexpectedResponse,
        ResolveError::Challenged => ArchiveFailure::Challenged,
        ResolveError::Timeout => ArchiveFailure::Timeout,
        ResolveError::Unreachable => ArchiveFailure::Unreachable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolution_failures_keep_their_code() {
        for error in [
            ResolveError::NoAdapter,
            ResolveError::UnexpectedResponse("a changed shape".to_owned()),
            ResolveError::Challenged,
            ResolveError::Timeout,
            ResolveError::Unreachable,
        ] {
            assert_eq!(failure_of(&error).code(), error.code());
        }
    }

    #[test]
    fn a_neighbouring_view_keeps_the_cited_side_and_stays_in_the_register() {
        let citation = ArchiveRegistry::embedded()
            .parse("AD44 - Exampleville - (aucun) - N - 1877 - 3E1/2 - vue 5d-6g/13")
            .unwrap();
        let side = |view| one_view(&citation, view).unwrap().side;
        assert_eq!(side(5), Some(oxidgene_archives::Side::Right));
        assert_eq!(side(6), Some(oxidgene_archives::Side::Left));
        assert_eq!(side(7), None);
        assert_eq!(one_view(&citation, 13).unwrap().view, 13);
        for beyond in [0, 14] {
            assert!(matches!(
                one_view(&citation, beyond),
                Err(OxidGeneError::Validation(_))
            ));
        }
    }
}
