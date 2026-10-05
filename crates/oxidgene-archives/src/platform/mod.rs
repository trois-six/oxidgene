//! Portal platforms: one adapter per publishing software, shared by every
//! archive that runs it and configured per collection by its `portal`
//! settings.
//!
//! An adapter is code and an archive is data: the catalogue names the
//! platform of each collection, and the registry ([`builtin`]) maps that name
//! to the adapter.

mod arkotheque;

use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

pub use arkotheque::Arkotheque;

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
    /// The portal origin, `https://host`, every request stays on.
    pub origin: String,
    /// The page a browser transport loads before issuing requests: the
    /// collection's search page.
    pub start: String,
    pub access: Access,
}

/// One portal platform.
pub trait Platform: Send + Sync {
    /// The catalogue value of `platform` this adapter answers to.
    fn id(&self) -> &'static str;

    /// Rejects a collection's `portal` object it cannot use, at load time.
    fn validate(&self, portal: &serde_json::Value) -> Result<(), CatalogError>;

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
    vec![Box::new(Arkotheque)]
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
