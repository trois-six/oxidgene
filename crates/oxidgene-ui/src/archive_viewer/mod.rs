//! Opening a cited archive register at the cited view.
//!
//! The interface recognizes normalized citations of the archives the
//! catalogue lists, both provided by `oxidgene-archives`, and offers such a
//! source as [`ArchiveSourceLink`]. On the desktop the binary injects an
//! [`ArchiveViewerOpener`] that resolves the citation in an archive window;
//! the web client, which has none, asks the backend for the target and opens
//! it in a new browser tab.

mod source_link;

use std::sync::Arc;

use dioxus::prelude::try_use_context;
use oxidgene_archives::{Archive, ArchiveRegistry, ArchiveTarget, CitationParts, cited_text};
use uuid::Uuid;

use crate::i18n::I18n;

pub use source_link::ArchiveSourceLink;

/// A citation together with the archive that holds it.
#[derive(Clone, Debug, PartialEq)]
pub struct ArchiveLink {
    pub archive: &'static Archive,
    pub citation: CitationParts,
    /// The source title as written, used to name the window.
    pub title: String,
    /// The cited source, and the citation whose page completes its title.
    pub source_id: Uuid,
    pub citation_id: Option<Uuid>,
}

impl ArchiveLink {
    /// The link a citation of a source stands for: its title completed by
    /// the citation's page, when its archive is catalogued and has a
    /// collection holding the cited act.
    pub fn from_citation(
        source_id: Uuid,
        title: &str,
        citation_id: Option<Uuid>,
        page: Option<&str>,
    ) -> Option<Self> {
        let (archive, citation) = ArchiveRegistry::embedded().link(&cited_text(title, page))?;
        Some(Self {
            archive,
            citation,
            title: title.to_owned(),
            source_id,
            citation_id,
        })
    }
}

/// The translation key of a failure's banner, by the failure's code: a
/// resolution error's, or the error code of the backend's answer.
fn failure_key(code: &str) -> &'static str {
    match code {
        "no_adapter" => "archive_viewer.no_adapter",
        "not_an_archive_citation" => "archive_viewer.not_an_archive_citation",
        "unexpected_response" => "archive_viewer.unexpected_response",
        "challenged" => "archive_viewer.challenged",
        "timeout" => "archive_viewer.timeout",
        "unreachable" => "archive_viewer.unreachable",
        _ => "archive_viewer.failed",
    }
}

/// Every banner a [`Landing`] may name.
const BANNER_KEYS: [&str; 9] = [
    "archive_viewer.not_found",
    "archive_viewer.ambiguous",
    "archive_viewer.failed",
    "archive_viewer.no_adapter",
    "archive_viewer.not_an_archive_citation",
    "archive_viewer.unexpected_response",
    "archive_viewer.challenged",
    "archive_viewer.timeout",
    "archive_viewer.unreachable",
];

/// The page a resolution ends on, and what to tell the reader about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Landing {
    pub url: String,
    /// The translation key of the banner, when there is something to say.
    pub banner: Option<&'static str>,
}

impl Landing {
    /// The target, with a banner when no register or several registers
    /// match; on a failure, given by its code, the archive's website with
    /// the failure's banner.
    pub fn of(archive: &Archive, outcome: Result<ArchiveTarget, &str>) -> Self {
        let (url, banner) = match outcome {
            Ok(ArchiveTarget::Results {
                url,
                matches: Some(0),
            }) => (url, Some("archive_viewer.not_found")),
            Ok(ArchiveTarget::Results {
                url,
                matches: Some(2..),
            }) => (url, Some("archive_viewer.ambiguous")),
            Ok(target) => (target.url().to_owned(), None),
            Err(code) => (archive.website.clone(), Some(failure_key(code))),
        };
        Self { url, banner }
    }
}

/// What the archive window tells the reader, in the interface language.
///
/// The portal page is not ours to translate, so the window shows these in a
/// small banner over it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveViewerMessages {
    pub searching: String,
    pub close: String,
    /// The text of each banner of [`BANNER_KEYS`].
    banners: Vec<(&'static str, String)>,
}

impl ArchiveViewerMessages {
    pub fn new(i18n: &I18n) -> Self {
        Self {
            searching: i18n.t("archive_viewer.searching"),
            close: i18n.t("common.close"),
            banners: BANNER_KEYS.map(|key| (key, i18n.t(key))).to_vec(),
        }
    }

    /// The text of a [`Landing`]'s banner.
    pub fn banner(&self, key: &str) -> Option<&str> {
        self.banners
            .iter()
            .find(|(known, _)| *known == key)
            .map(|(_, text)| text.as_str())
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

    fn link(title: &str, page: Option<&str>) -> Option<ArchiveLink> {
        ArchiveLink::from_citation(Uuid::nil(), title, None, page)
    }

    #[test]
    fn links_only_catalogued_archives_holding_the_act() {
        let birth = link(
            "AD44 - Exampleville - (aucun) - N - 1877 - 3E1/2 - acte 26 - vue 5d/13",
            None,
        )
        .expect("a catalogued birth");
        assert_eq!(birth.archive.id, "fr-ad44");
        assert_eq!(birth.citation.locality, "Exampleville");

        // A catalogued archive, but a table no collection holds.
        assert_eq!(
            link("AD44 - Exampleville - (aucun) - TD - 1877", None),
            None
        );
        // A well-formed citation of an archive the catalogue does not list.
        assert_eq!(link("AD67 - Exampleville - (aucun) - N - 1877", None), None);
    }

    #[test]
    fn a_citation_page_names_the_views() {
        let birth = link(
            "AD44 - Exampleville - (aucun) - N - 1877 - 3E1/2",
            Some("acte 26 - vue 5d/13"),
        )
        .expect("a catalogued birth");
        assert_eq!(birth.citation.views.len(), 1);
        assert_eq!(birth.citation.views[0].view, 5);
        // The window keeps the source's own title.
        assert_eq!(
            birth.title,
            "AD44 - Exampleville - (aucun) - N - 1877 - 3E1/2"
        );
    }

    #[test]
    fn a_landing_says_what_the_resolution_found() {
        let archive = &ArchiveRegistry::embedded().archives()[0];
        let results = |matches| ArchiveTarget::Results {
            url: "https://archives.example.org/search".to_owned(),
            matches,
        };
        assert_eq!(Landing::of(archive, Ok(results(Some(1)))).banner, None);
        assert_eq!(Landing::of(archive, Ok(results(None))).banner, None);
        assert_eq!(
            Landing::of(archive, Ok(results(Some(0)))).banner,
            Some("archive_viewer.not_found")
        );
        assert_eq!(
            Landing::of(archive, Ok(results(Some(3)))).banner,
            Some("archive_viewer.ambiguous")
        );

        let failed = Landing::of(archive, Err("timeout"));
        assert_eq!(failed.url, archive.website);
        assert_eq!(failed.banner, Some("archive_viewer.timeout"));
        assert_eq!(
            Landing::of(archive, Err("internal_error")).banner,
            Some("archive_viewer.failed")
        );
        // Every banner a landing names has its text.
        for code in [
            "no_adapter",
            "not_an_archive_citation",
            "unexpected_response",
            "challenged",
            "timeout",
            "unreachable",
            "",
        ] {
            assert!(BANNER_KEYS.contains(&failure_key(code)), "{code}");
        }
    }
}
