//! The archives OxidGene knows, and how a cited source becomes the address of
//! its image on the archive's portal.
//!
//! - [`catalog`]: the archive services, one embedded JSON document each.
//! - [`citation`]: normalized citations read into [`CitationParts`].
//! - [`platform`]: one adapter per portal software.
//! - [`transport`]: how an adapter's requests reach a portal.
//!
//! [`ArchiveRegistry`] joins the catalogue to the adapters, and [`Resolver`]
//! turns parsed citations into [`ArchiveTarget`]s. The crate has no UI and no
//! storage, and compiles to WebAssembly without the `native` transport, where
//! it parses citations and builds offline targets only.

pub mod catalog;
pub mod citation;
pub mod platform;
pub mod transport;

use std::collections::HashMap;
use std::fmt;
use std::sync::{LazyLock, Mutex};

use serde::{Deserialize, Serialize};

pub use catalog::{Archive, CatalogError, Collection, Display, Level, Period};
pub use citation::{Act, ActKind, CallNumber, CitationGrammar, CitationParts, CitedView, Side};
pub use platform::{Access, Platform, PortalEndpoint};
#[cfg(feature = "native")]
pub use transport::NativeTransport;
pub use transport::{FetchError, Method, PortalFetch, PortalRequest, PortalTransport};

/// Where a resolved citation opens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ArchiveTarget {
    /// The cited views, or the register when the citation names no view.
    View {
        url: String,
        views: Vec<ArchiveView>,
        view_count: Option<u16>,
        call_number: Option<String>,
        attribution: Option<String>,
    },
    /// Several or no registers matched: the portal's filtered results.
    /// `matches` is `None` when the address was built without a request.
    Results { url: String, matches: Option<usize> },
}

impl ArchiveTarget {
    /// The page to open.
    pub fn url(&self) -> &str {
        match self {
            Self::View { url, .. } | Self::Results { url, .. } => url,
        }
    }
}

/// One cited view of a resolved register.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveView {
    /// One-based view number, as cited.
    pub view: u16,
    /// The portal page opened on this view.
    pub url: String,
    /// The image's persistent address, when the portal publishes one.
    pub ark: Option<String>,
    /// Present only for a `display: "iiif"` archive.
    pub image: Option<ArchiveImage>,
}

/// An image OxidGene may show itself, for a `display: "iiif"` archive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveImage {
    /// The address the viewer loads.
    pub picture: String,
    /// The smallest address the archive serves.
    pub thumbnail: String,
    pub width: u32,
    pub height: u32,
}

/// Why a citation could not be resolved. Each variant has a stable
/// [`code`](Self::code), which the interface translates as
/// `archive_viewer.<code>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// The archive is not catalogued, or no collection with an adapter holds
    /// the cited act.
    NoAdapter,
    /// The portal answered, but not as the adapter expects: what differed,
    /// without any response content.
    UnexpectedResponse(String),
    /// The portal did not answer in time.
    Timeout,
    /// The portal could not be reached, or answered with a server error.
    Unreachable,
}

impl ResolveError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::NoAdapter => "no_adapter",
            Self::UnexpectedResponse(_) => "unexpected_response",
            Self::Timeout => "timeout",
            Self::Unreachable => "unreachable",
        }
    }
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedResponse(detail) => write!(f, "unexpected_response: {detail}"),
            other => f.write_str(other.code()),
        }
    }
}

impl std::error::Error for ResolveError {}

impl From<FetchError> for ResolveError {
    fn from(error: FetchError) -> Self {
        match error {
            FetchError::Timeout => Self::Timeout,
            FetchError::Network => Self::Unreachable,
            FetchError::Status(status) if status >= 500 => Self::Unreachable,
            other => Self::UnexpectedResponse(other.to_string()),
        }
    }
}

/// The catalogue joined to the adapters.
pub struct ArchiveRegistry {
    archives: Vec<Archive>,
    platforms: Vec<Box<dyn Platform>>,
}

static EMBEDDED: LazyLock<ArchiveRegistry> = LazyLock::new(|| {
    ArchiveRegistry::new(catalog::EMBEDDED, platform::builtin())
        .unwrap_or_else(|error| panic!("the embedded archive catalogue: {error}"))
});

impl ArchiveRegistry {
    /// The embedded catalogue with the built-in adapters. Its validity is a
    /// test of this crate, so loading it cannot fail at run time.
    pub fn embedded() -> &'static Self {
        &EMBEDDED
    }

    /// A registry over catalogue documents given as `(country directory,
    /// JSON)`, validated against `platforms`.
    pub fn new(
        documents: &[(&str, &str)],
        platforms: Vec<Box<dyn Platform>>,
    ) -> Result<Self, CatalogError> {
        let archives = catalog::load(documents, &platforms)?;
        Ok(Self {
            archives,
            platforms,
        })
    }

    /// Every archive, in catalogue order: by country, then by file name.
    pub fn archives(&self) -> &[Archive] {
        &self.archives
    }

    /// The archive citations starting with `code` belong to.
    pub fn archive(&self, code: &str) -> Option<&Archive> {
        self.archives
            .iter()
            .find(|archive| archive.citation_codes.iter().any(|known| known == code))
    }

    /// The adapter of a platform.
    pub fn platform(&self, id: &str) -> Option<&dyn Platform> {
        self.platforms
            .iter()
            .find(|platform| platform.id() == id)
            .map(AsRef::as_ref)
    }

    /// Reads a source title with the grammar of the archive its code names,
    /// or the default grammar for a code the catalogue does not list.
    pub fn parse(&self, title: &str) -> Option<CitationParts> {
        let code = citation::code_of(title)?;
        let grammar = self
            .archive(code)
            .map(|archive| archive.citation.clone())
            .unwrap_or_default();
        CitationParts::parse(title, &grammar)
    }

    /// The archive of a source title, when the title is a citation of a
    /// catalogued archive with a collection holding its act: the condition
    /// for offering the source as a link.
    pub fn link(&self, title: &str) -> Option<(&Archive, CitationParts)> {
        let citation = self.parse(title)?;
        let archive = self.archive(&citation.code)?;
        archive.holds(&citation.act).then_some((archive, citation))
    }

    /// The archive and the collections to try, in order: those holding the
    /// act whose period contains the year, or, when none does, every one
    /// holding the act.
    pub fn candidates(&self, citation: &CitationParts) -> Option<(&Archive, Vec<&Collection>)> {
        let archive = self.archive(&citation.code)?;
        let mut collections: Vec<_> = archive.collections_for(citation).collect();
        if collections.is_empty() {
            collections = archive
                .collections
                .iter()
                .filter(|collection| collection.holds(&citation.act))
                .collect();
        }
        (!collections.is_empty()).then_some((archive, collections))
    }

    /// The target built without any request: the first candidate
    /// collection's search page, or the archive's website.
    pub fn offline_target(&self, citation: &CitationParts) -> Result<ArchiveTarget, ResolveError> {
        let (archive, collections) = self.candidates(citation).ok_or(ResolveError::NoAdapter)?;
        let url = collections
            .iter()
            .find_map(|collection| {
                self.platform(&collection.platform)?
                    .results_url(collection, citation)
            })
            .unwrap_or_else(|| archive.website.clone());
        Ok(ArchiveTarget::Results { url, matches: None })
    }
}

impl fmt::Debug for ArchiveRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ArchiveRegistry")
            .field(
                "archives",
                &self
                    .archives
                    .iter()
                    .map(|archive| &archive.id)
                    .collect::<Vec<_>>(),
            )
            .field(
                "platforms",
                &self
                    .platforms
                    .iter()
                    .map(|platform| platform.id())
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

/// How many resolved targets a resolver keeps before starting afresh.
const CACHE_CAPACITY: usize = 256;

/// Resolves citations, one per reader's click, and keeps what it found for
/// the session so that opening the same citation again sends no request.
#[derive(Debug)]
pub struct Resolver<'r> {
    registry: &'r ArchiveRegistry,
    cache: Mutex<HashMap<CitationParts, ArchiveTarget>>,
}

impl<'r> Resolver<'r> {
    pub fn new(registry: &'r ArchiveRegistry) -> Self {
        Self {
            registry,
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn registry(&self) -> &'r ArchiveRegistry {
        self.registry
    }

    /// Tries the candidate collections in order until one finds the register.
    /// When none does, the first search that ran gives the filtered results;
    /// failing that, the first error; failing that, the offline target of a
    /// collection the transport cannot reach.
    pub async fn resolve(
        &self,
        citation: &CitationParts,
        transport: &dyn PortalTransport,
    ) -> Result<ArchiveTarget, ResolveError> {
        let (archive, collections) = self
            .registry
            .candidates(citation)
            .ok_or(ResolveError::NoAdapter)?;
        if let Some(target) = self.cached(citation) {
            return Ok(target);
        }

        let mut searched = None;
        let mut offline = None;
        let mut failure = None;
        for collection in collections {
            match self
                .resolve_in(archive, collection, citation, transport)
                .await
            {
                Ok(target @ ArchiveTarget::View { .. }) => {
                    self.remember(citation, &target);
                    return Ok(target);
                }
                Ok(
                    target @ ArchiveTarget::Results {
                        matches: Some(_), ..
                    },
                ) => {
                    searched.get_or_insert(target);
                }
                Ok(target) => {
                    offline.get_or_insert(target);
                }
                Err(error) => {
                    failure.get_or_insert(error);
                }
            }
        }
        if let Some(target) = searched {
            self.remember(citation, &target);
            return Ok(target);
        }
        match (failure, offline) {
            (Some(error), _) => Err(error),
            (None, Some(target)) => Ok(target),
            (None, None) => self.registry.offline_target(citation),
        }
    }

    async fn resolve_in(
        &self,
        archive: &Archive,
        collection: &Collection,
        citation: &CitationParts,
        transport: &dyn PortalTransport,
    ) -> Result<ArchiveTarget, ResolveError> {
        let platform = self
            .registry
            .platform(&collection.platform)
            .ok_or(ResolveError::NoAdapter)?;
        let endpoint = platform
            .endpoint(collection)
            .ok_or(ResolveError::NoAdapter)?;
        if endpoint.access == Access::Browser && !transport.is_browser() {
            let url = platform
                .results_url(collection, citation)
                .unwrap_or_else(|| archive.website.clone());
            return Ok(ArchiveTarget::Results { url, matches: None });
        }
        let fetch = transport.connect(&endpoint).await?;
        platform
            .resolve(archive, collection, citation, fetch.as_ref())
            .await
    }

    fn cached(&self, citation: &CitationParts) -> Option<ArchiveTarget> {
        self.cache.lock().ok()?.get(citation).cloned()
    }

    fn remember(&self, citation: &CitationParts, target: &ArchiveTarget) {
        if let Ok(mut cache) = self.cache.lock() {
            if cache.len() >= CACHE_CAPACITY {
                cache.clear();
            }
            cache.insert(citation.clone(), target.clone());
        }
    }
}

#[cfg(test)]
mod tests;
