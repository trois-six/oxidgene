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
    ArchiveRegistry, ArchiveTarget, FetchError, NativeTransport, PortalEndpoint, PortalFetch,
    PortalTransport, ResolveError, Resolver, cited_text,
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
    let Some((archive, citation)) = registry.link(&text) else {
        let failure = if registry.parse(&text).is_some() {
            ArchiveFailure::NoAdapter
        } else {
            ArchiveFailure::NotACitation
        };
        return Err(OxidGeneError::Archive(failure));
    };
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
}
