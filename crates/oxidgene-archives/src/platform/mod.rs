//! Portal platforms: one adapter per publishing software, shared by every
//! archive that runs it and configured per collection by its `portal`
//! settings.
//!
//! An adapter is code and an archive is data: the catalogue names the
//! platform of each collection, and the registry ([`builtin`]) maps that name
//! to the adapter.
//!
//! What does not depend on a platform is shared by every adapter:
//! [`query`] percent-encodes query strings, [`markup`] scans portal markup,
//! [`select`] chooses the cited register among a search's results,
//! [`iiif`] reads an image service and builds a view's image, and [`view`]
//! assembles the `View` target of the chosen register.

mod arkotheque;
pub(crate) mod iiif;
mod ligeo;
pub(crate) mod markup;
mod mnesys;
pub(crate) mod query;
pub(crate) mod select;
pub(crate) mod view;

use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

pub use arkotheque::Arkotheque;
pub use ligeo::Ligeo;
pub use mnesys::Mnesys;
pub(crate) use query::Query;

use crate::catalog::{Archive, CatalogError, Collection};
use crate::citation::CitationParts;
use crate::transport::PortalFetch;
use crate::{ArchiveTarget, ResolveError};

/// A boxed future, as the trait methods of adapters and transports return.
///
/// The bound is `Send` on every target: the resolver runs in the server's
/// request handlers and on the desktop's runtime, which move work between
/// threads, and the web build only parses and builds offline targets, so it
/// never implements a transport whose futures could not be `Send`.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// How a portal may be reached.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    /// Any HTTP client.
    #[default]
    Any,
    /// Only a browser: an anti-bot challenge blocks other clients.
    Browser,
}

/// Where a collection's portal answers, as a transport needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortalEndpoint {
    /// The portal origin, `https://host`: the window's page, and the origin
    /// of every request written as a path.
    pub origin: String,
    /// Further origins the portal's own pages call, such as an API host,
    /// declared by the adapter's settings. A request may name one in an
    /// absolute address; no other origin is ever reached.
    pub other_origins: Vec<String>,
    /// The page a browser transport loads before issuing requests: the
    /// collection's search page.
    pub start: String,
    pub access: Access,
}

impl PortalEndpoint {
    /// Every origin a request may reach, the portal's first.
    pub fn origins(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.origin.as_str()).chain(self.other_origins.iter().map(String::as_str))
    }
}

/// One portal platform.
pub trait Platform: Send + Sync {
    /// The catalogue value of `platform` this adapter answers to.
    fn id(&self) -> &'static str;

    /// Rejects a collection whose `portal` settings it cannot use, or whose
    /// acts the settings do not search, at load time.
    fn validate(&self, collection: &Collection) -> Result<(), CatalogError>;

    /// Where the collection's portal answers; `None` only for settings that
    /// [`Platform::validate`] would refuse.
    fn endpoint(&self, collection: &Collection) -> Option<PortalEndpoint>;

    /// The collection's search page, filtered by the citation where the
    /// platform allows it, built without any request.
    fn results_url(&self, collection: &Collection, citation: &CitationParts) -> Option<String>;

    /// Resolves a citation to a target in this collection: `View` when one
    /// register matches, `Results` with the match count otherwise.
    fn resolve<'a>(
        &'a self,
        archive: &'a Archive,
        collection: &'a Collection,
        citation: &'a CitationParts,
        fetch: &'a dyn PortalFetch,
    ) -> BoxFuture<'a, Result<ArchiveTarget, ResolveError>>;
}

/// Every adapter OxidGene ships.
pub fn builtin() -> Vec<Box<dyn Platform>> {
    vec![Box::new(Arkotheque), Box::new(Ligeo), Box::new(Mnesys)]
}

/// Whether `text` is an `https` origin: a scheme and a host, no path.
pub(crate) fn is_https_origin(text: &str) -> bool {
    text.strip_prefix("https://").is_some_and(|host| {
        !host.is_empty() && !host.contains(['/', '?', '#', ' ']) && !host.ends_with(':')
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_an_https_origin() {
        assert!(is_https_origin("https://archives.example.org"));
        assert!(is_https_origin("https://archives.example.org:8443"));
        for text in [
            "http://archives.example.org",
            "https://archives.example.org/",
            "https://archives.example.org/path",
            "https://",
            "archives.example.org",
        ] {
            assert!(!is_https_origin(text), "{text}");
        }
    }

    #[test]
    fn adapter_ids_are_unique() {
        let platforms = builtin();
        for (index, platform) in platforms.iter().enumerate() {
            assert!(
                platforms[..index]
                    .iter()
                    .all(|other| other.id() != platform.id()),
                "{} is registered twice",
                platform.id()
            );
        }
    }
}
