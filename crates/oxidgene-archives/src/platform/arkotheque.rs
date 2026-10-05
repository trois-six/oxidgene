//! Arkothèque (1 égal 2), the publishing software of many French
//! departmental archives.
//!
//! This adapter reads the settings every Arkothèque collection shares — the
//! portal origin, its transport and the collection's search page — and
//! answers with that search page. Searching the portal's request interface
//! for the cited register (Archive Portals §4.3) is not implemented yet, so
//! [`Platform::resolve`] returns the offline `Results` target. Other settings
//! in a collection's `portal` object are left to the desktop's form driver,
//! which reads them until the request interface replaces it.

use serde::Deserialize;

use super::{Access, BoxFuture, Platform, PortalEndpoint, is_https_origin};
use crate::catalog::{Archive, CatalogError, Collection};
use crate::citation::CitationParts;
use crate::transport::PortalFetch;
use crate::{ArchiveTarget, ResolveError};

/// The Arkothèque adapter.
pub struct Arkotheque;

/// The settings this adapter reads from a collection's `portal` object.
#[derive(Debug, Deserialize)]
struct Settings {
    origin: String,
    #[serde(default)]
    transport: Access,
    search_path: String,
}

impl Settings {
    fn read(portal: &serde_json::Value) -> Result<Self, CatalogError> {
        let settings = Self::deserialize(portal)
            .map_err(|error| CatalogError::new(format!("arkotheque settings: {error}")))?;
        if !is_https_origin(&settings.origin) {
            return Err(CatalogError::new(
                "arkotheque settings: origin must be an https origin",
            ));
        }
        if !settings.search_path.starts_with('/') || settings.search_path.starts_with("//") {
            return Err(CatalogError::new(
                "arkotheque settings: search_path must be an absolute path",
            ));
        }
        Ok(settings)
    }

    fn search_page(&self) -> String {
        format!("{}{}", self.origin, self.search_path)
    }
}

impl Platform for Arkotheque {
    fn id(&self) -> &'static str {
        "arkotheque"
    }

    fn validate(&self, portal: &serde_json::Value) -> Result<(), CatalogError> {
        Settings::read(portal).map(drop)
    }

    fn endpoint(&self, collection: &Collection) -> Option<PortalEndpoint> {
        let settings = Settings::read(&collection.portal).ok()?;
        Some(PortalEndpoint {
            start: settings.search_page(),
            origin: settings.origin,
            access: settings.transport,
        })
    }

    fn results_url(&self, collection: &Collection, _citation: &CitationParts) -> Option<String> {
        Settings::read(&collection.portal)
            .ok()
            .map(|settings| settings.search_page())
    }

    fn resolve<'a>(
        &'a self,
        _archive: &'a Archive,
        collection: &'a Collection,
        citation: &'a CitationParts,
        _fetch: &'a dyn PortalFetch,
    ) -> BoxFuture<'a, Result<ArchiveTarget, ResolveError>> {
        Box::pin(async move {
            let url = self
                .results_url(collection, citation)
                .ok_or(ResolveError::NoAdapter)?;
            Ok(ArchiveTarget::Results { url, matches: None })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn portal(transport: Option<&str>) -> serde_json::Value {
        let mut portal = serde_json::json!({
            "origin": "https://archives.example.org",
            "search_path": "/chercher/registres",
        });
        if let Some(transport) = transport {
            portal["transport"] = transport.into();
        }
        portal
    }

    #[test]
    fn reads_the_endpoint_of_a_collection() {
        let collection = Collection {
            id: "registers".to_owned(),
            acts: Vec::new(),
            period: None,
            platform: "arkotheque".to_owned(),
            portal: portal(Some("browser")),
        };
        assert_eq!(
            Arkotheque.endpoint(&collection),
            Some(PortalEndpoint {
                origin: "https://archives.example.org".to_owned(),
                start: "https://archives.example.org/chercher/registres".to_owned(),
                access: Access::Browser,
            })
        );
    }

    #[test]
    fn validates_its_settings() {
        assert_eq!(Arkotheque.validate(&portal(None)), Ok(()));
        assert_eq!(Arkotheque.validate(&portal(Some("any"))), Ok(()));
        assert!(Arkotheque.validate(&portal(Some("carrier"))).is_err());

        let mut wrong = portal(None);
        wrong["origin"] = "http://archives.example.org".into();
        assert!(Arkotheque.validate(&wrong).is_err());
        let mut wrong = portal(None);
        wrong["search_path"] = "//elsewhere.example.org/".into();
        assert!(Arkotheque.validate(&wrong).is_err());
        assert!(Arkotheque.validate(&serde_json::json!({})).is_err());
    }
}
