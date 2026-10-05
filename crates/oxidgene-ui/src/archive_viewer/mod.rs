//! Opening a cited archive register at the cited view.
//!
//! The interface recognizes normalized citations of the archives the
//! catalogue lists, both provided by `oxidgene-archives`. Opening one is a
//! desktop capability: the desktop binary injects an [`ArchiveViewerOpener`]
//! that opens the archive window, and the web client, which has none, shows
//! the citation as plain text.

use std::sync::Arc;

use dioxus::prelude::try_use_context;
use oxidgene_archives::{Archive, ArchiveRegistry, CitationParts};
use serde::Serialize;

use crate::i18n::I18n;

/// A citation together with the archive that holds it.
#[derive(Clone, Debug, PartialEq)]
pub struct ArchiveLink {
    pub archive: &'static Archive,
    pub citation: CitationParts,
    /// The source title as written, used to name the window.
    pub title: String,
}

impl ArchiveLink {
    /// The link a source title stands for, when its archive is catalogued and
    /// has a collection holding the cited act.
    pub fn from_source_title(title: &str) -> Option<Self> {
        let (archive, citation) = ArchiveRegistry::embedded().link(title)?;
        Some(Self {
            archive,
            citation,
            title: title.to_owned(),
        })
    }
}

/// What the archive window tells the reader, in the interface language.
///
/// The portal page is not ours to translate, so the window shows these in a
/// small banner over it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ArchiveViewerMessages {
    pub searching: String,
    pub not_found: String,
    pub ambiguous: String,
    pub view_not_selected: String,
    pub failed: String,
    pub close: String,
}

impl ArchiveViewerMessages {
    pub fn new(i18n: &I18n) -> Self {
        Self {
            searching: i18n.t("archive_viewer.searching"),
            not_found: i18n.t("archive_viewer.not_found"),
            ambiguous: i18n.t("archive_viewer.ambiguous"),
            view_not_selected: i18n.t("archive_viewer.view_not_selected"),
            failed: i18n.t("archive_viewer.failed"),
            close: i18n.t("common.close"),
        }
    }
}

/// One request to open a register.
#[derive(Clone, Debug, PartialEq)]
pub struct ArchiveViewerRequest {
    pub link: ArchiveLink,
    pub messages: ArchiveViewerMessages,
}

/// The platform side of the archive viewer.
pub trait ArchiveViewerOpener: Send + Sync {
    /// Whether this platform can open `link`.
    fn supports(&self, link: &ArchiveLink) -> bool;
    fn open(&self, request: ArchiveViewerRequest);
}

#[derive(Clone)]
pub struct ArchiveViewerBridge(Arc<dyn ArchiveViewerOpener>);

impl ArchiveViewerBridge {
    pub fn new(opener: Arc<dyn ArchiveViewerOpener>) -> Self {
        Self(opener)
    }

    /// Whether `link` can be opened here.
    pub fn supports(&self, link: &ArchiveLink) -> bool {
        self.0.supports(link)
    }

    pub fn open(&self, request: ArchiveViewerRequest) {
        self.0.open(request);
    }
}

impl std::fmt::Debug for ArchiveViewerBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ArchiveViewerBridge")
    }
}

pub fn use_archive_viewer_bridge() -> Option<ArchiveViewerBridge> {
    try_use_context::<ArchiveViewerBridge>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_only_catalogued_archives_holding_the_act() {
        let link = ArchiveLink::from_source_title(
            "AD44 - Exampleville - (aucun) - N - 1877 - 3E1/2 - acte 26 - vue 5d/13",
        )
        .expect("a catalogued birth");
        assert_eq!(link.archive.id, "fr-ad44");
        assert_eq!(link.citation.locality, "Exampleville");

        // A catalogued archive, but a table no collection holds.
        assert_eq!(
            ArchiveLink::from_source_title("AD44 - Exampleville - (aucun) - TD - 1877"),
            None
        );
        // A well-formed citation of an archive the catalogue does not list.
        assert_eq!(
            ArchiveLink::from_source_title("AD67 - Exampleville - (aucun) - N - 1877"),
            None
        );
    }
}
