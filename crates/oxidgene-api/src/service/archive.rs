//! Resolving a cited source to the page of its archive's portal that shows
//! the cited register (docs/archives.md §5.3).
//!
//! The source title, completed by the citation's page when a citation is
//! named, is read as a normalized archive citation. The process shares one
//! [`Resolver`], whose session cache spares a second request for a citation
//! already resolved. Portals whose access is `any` are searched over the
//! native transport; a `browser` portal, which only a browser page passes,
//! yields its offline filtered results.

use std::fmt;
use std::sync::{Arc, OnceLock};

use oxidgene_archives::platform::BoxFuture;
use oxidgene_archives::{
    ArchiveRegistry, ArchiveTarget, CitationParts, CitedView, FetchError, NativeTransport,
    PortalEndpoint, PortalFetch, PortalTransport, ResolveError, Resolver, cited_text,
};
use oxidgene_core::error::{ArchiveFailure, OxidGeneError};
use oxidgene_db::repo::{CitationRepo, SourceRepo};
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

/// Where source `source_id` of tree `tree_id` opens on its archive's portal,
/// as cited by `citation_id` when given: a citation of that source, whose
/// page completes the title.
///
/// `view`, when given, resolves that one view of the cited register instead
/// of the cited views: the reader paging to the previous or next view in
/// OxidGene's viewer, one resolution per click (docs/archives.md §6.3, §8).
/// It keeps the side the citation gives that view, if it cites it; a view
/// below 1 or beyond the cited view count is a validation error.
///
/// A source or citation that is absent, deleted or of another tree, and a
/// citation of another source, are `NotFound`. A title that is no archive
/// citation, or cites an act no catalogued collection holds, and a failed
/// resolution are [`OxidGeneError::Archive`]; a failure is logged with its
/// code and the archive's identifier only, never the citation.
pub async fn archive_target(
    db: &impl ConnectionTrait,
    portals: &ArchivePortals,
    tree_id: Uuid,
    source_id: Uuid,
    citation_id: Option<Uuid>,
    view: Option<u16>,
) -> Result<ArchiveTarget, OxidGeneError> {
    require_tree_resource(db, tree_id, TreeResource::Source, source_id).await?;
    let source = SourceRepo::get(db, source_id).await?;
    let page = match citation_id {
        Some(id) => {
            require_tree_resource(db, tree_id, TreeResource::Citation, id).await?;
            let citation = CitationRepo::get(db, id).await?;
            if citation.source_id != source_id {
                return Err(OxidGeneError::NotFound {
                    entity: "Citation",
                    id,
                });
            }
            citation.page
        }
        None => None,
    };

    let text = cited_text(&source.title, page.as_deref());
    let registry = portals.resolver.registry();
    let Some((archive, mut citation)) = registry.link(&text) else {
        let failure = if registry.parse(&text).is_some() {
            ArchiveFailure::NoAdapter
        } else {
            ArchiveFailure::NotACitation
        };
        return Err(OxidGeneError::Archive(failure));
    };
    if let Some(view) = view {
        citation.views = vec![one_view(&citation, view)?];
    }
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
