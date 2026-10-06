//! A Bach collection's `portal` settings (Archive Portals §4.8): where the
//! portal and its viewer answer, and how a locality's registers are found
//! among the finding aids.

use serde::Deserialize;

use crate::catalog::{CatalogError, Collection};
use crate::platform::markup::fold;
use crate::platform::{Access, is_https_origin};
use crate::transport::origin_of;

/// The page listing the portal's finding aids.
pub(super) const CLASSIFICATION_PATH: &str = "/archives/classification-scheme";

/// A collection's `portal` settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Settings {
    pub(super) origin: String,
    #[serde(default)]
    pub(super) transport: Access,
    /// The viewer's root: its own origin (`https://viewer.example.org`), or
    /// a path on one (`https://archives.example.org/viewer`). Its pages are
    /// `<viewer>/series/<folder>` and its image lists
    /// `<viewer>/api/info/series/<folder>`.
    pub(super) viewer: String,
    /// Where a register's image names come from.
    #[serde(default)]
    pub(super) views: Views,
    pub(super) inventory: Inventory,
}

/// Where a register's image names come from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Views {
    /// The viewer's image list, `<viewer>/api/info/series/<folder>`.
    #[default]
    Api,
    /// The first and last image a register's viewer link names (`s`, `e`),
    /// numbered in between: for a portal whose viewer list the portal's own
    /// pages cannot request, the viewer's host being behind its own
    /// anti-bot check.
    Range,
}

/// How a locality's registers are found.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Inventory {
    /// One finding aid per locality, listed by the classification scheme:
    /// the entries whose link opens `/document/<prefix>…`.
    Classification {
        prefix: String,
        /// Whether an entry names its locality in its title or in its
        /// link's text.
        locality: Label,
        /// The titles the collection's entries start with, before the
        /// locality when the title names it: `Registres paroissiaux et
        /// d'état civil :`. Entries with another title are left out; with
        /// none, every entry of the prefix is the collection's.
        #[serde(default)]
        title_prefixes: Vec<String>,
    },
    /// One finding aid for every locality, whose nodes at `level` (1 for
    /// the aid's top nodes) are the localities; without a level, the aid is
    /// searched whole, whatever the cited locality, as a series of a
    /// single office.
    Document {
        document: String,
        #[serde(default)]
        level: Option<u8>,
    },
}

/// Where a classification entry names its locality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Label {
    Title,
    Link,
}

pub(super) fn invalid(message: &str) -> CatalogError {
    CatalogError::new(format!("bach settings: {message}"))
}

/// A finding aid identifier, or the start of one: letters, digits, `_` and
/// `-`.
fn is_identifier(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
}

impl Settings {
    pub(super) fn read(collection: &Collection) -> Result<Self, CatalogError> {
        let settings =
            Self::deserialize(&collection.portal).map_err(|error| invalid(&error.to_string()))?;
        settings.check()?;
        Ok(settings)
    }

    fn check(&self) -> Result<(), CatalogError> {
        if !is_https_origin(&self.origin) {
            return Err(invalid("origin must be an https origin"));
        }
        let viewer_ok = self.viewer_origin().is_some_and(is_https_origin)
            && self.viewer_origin().is_some_and(|origin| {
                let path = &self.viewer[origin.len()..];
                path.is_empty()
                    || (path.starts_with('/')
                        && !path.ends_with('/')
                        && path[1..].split('/').all(is_identifier))
            });
        if !viewer_ok {
            return Err(invalid(
                "viewer must be an https origin, or one with a path",
            ));
        }
        match &self.inventory {
            Inventory::Classification {
                prefix,
                title_prefixes,
                ..
            } => {
                if !is_identifier(prefix) {
                    return Err(invalid("prefix must start a finding aid identifier"));
                }
                if title_prefixes.iter().any(|title| fold(title).is_empty()) {
                    return Err(invalid("title_prefixes must not be blank"));
                }
            }
            Inventory::Document { document, level } => {
                if !is_identifier(document) {
                    return Err(invalid("document must be a finding aid identifier"));
                }
                if *level == Some(0) {
                    return Err(invalid("level counts from 1"));
                }
            }
        }
        Ok(())
    }

    /// The viewer's origin, which the endpoint declares when it is not the
    /// portal's.
    pub(super) fn viewer_origin(&self) -> Option<&str> {
        origin_of(&self.viewer)
    }

    /// The finding aid's page.
    pub(super) fn document_path(document: &str) -> String {
        format!("/document/{document}")
    }

    /// The page a reader lands on without a register: the locality's
    /// finding aid when known, the list of finding aids otherwise.
    pub(super) fn landing(&self, document: Option<&str>) -> String {
        let fixed = match &self.inventory {
            Inventory::Document { document, .. } => Some(document.as_str()),
            Inventory::Classification { .. } => None,
        };
        match document.or(fixed) {
            Some(document) => format!("{}{}", self.origin, Self::document_path(document)),
            None => format!("{}{CLASSIFICATION_PATH}", self.origin),
        }
    }

    /// The register's viewer address is on the settings' viewer, and its
    /// image list: `<viewer>/series/<folder>[?query]` gives
    /// `<viewer>/api/info/series/<folder>[?query]`.
    pub(super) fn image_list(&self, link: &str) -> Option<String> {
        let rest = link.strip_prefix(&self.viewer)?.strip_prefix("/series/")?;
        (!rest.is_empty()).then(|| format!("{}/api/info/series/{rest}", self.viewer))
    }
}
