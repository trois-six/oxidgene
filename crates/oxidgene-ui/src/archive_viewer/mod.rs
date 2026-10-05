//! Opening a cited archive register at the cited view.
//!
//! The interface recognizes normalized citations of the archives the
//! catalogue lists, both provided by `oxidgene-archives`, and offers such a
//! source as [`ArchiveSourceLink`]. On the desktop the binary injects an
//! [`ArchiveViewerOpener`] that resolves the citation in an archive window;
//! the web client, which has none, asks the backend for the target and opens
//! it in a new browser tab. An archive whose images OxidGene may show
//! (`display: "iiif"`) is resolved by the backend on both clients and shown
//! in OxidGene's own viewer instead ([`viewer`]), from which the reader may
//! attach the cited views as a document.

mod register;
mod source_link;
mod viewer;

use std::sync::Arc;

use dioxus::prelude::try_use_context;
use oxidgene_archives::{Archive, ArchiveRegistry, ArchiveTarget, CitationParts, cited_text};
use uuid::Uuid;

use crate::i18n::I18n;

pub use register::{ArchiveRegister, ViewPage};
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
const BANNER_KEYS: [&str; 10] = [
    "archive_viewer.not_found",
    "archive_viewer.ambiguous",
    "archive_viewer.go_to_view",
    "archive_viewer.failed",
    "archive_viewer.no_adapter",
    "archive_viewer.not_an_archive_citation",
    "archive_viewer.unexpected_response",
    "archive_viewer.challenged",
    "archive_viewer.timeout",
    "archive_viewer.unreachable",
];

/// What to tell the reader over a [`Landing`]: a translation key, and the
/// view number its text names, if any.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LandingBanner {
    pub key: &'static str,
    pub view: Option<u16>,
}

impl LandingBanner {
    const fn of(key: &'static str) -> Self {
        Self { key, view: None }
    }

    /// The banner's text in the interface language.
    pub fn text(&self, i18n: &I18n) -> String {
        fill(i18n.t(self.key), self.view)
    }
}

/// `text` with its `{view}` placeholder filled.
fn fill(text: String, view: Option<u16>) -> String {
    match view {
        Some(view) => text.replace("{view}", &view.to_string()),
        None => text,
    }
}

/// The page a resolution ends on, and what to tell the reader about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Landing {
    pub url: String,
    /// The banner, when there is something to say.
    pub banner: Option<LandingBanner>,
}

impl Landing {
    /// The target, with a banner when no register or several registers
    /// match, or when the register opens on its first view though the
    /// citation names one within it: the portal has no address per view,
    /// and the reader goes to the view. On a failure, given by its code,
    /// the failure's banner over the collection's filtered search page when
    /// an anti-bot check answered — the reader may pass it there — and over
    /// the archive's website otherwise.
    pub fn of(link: &ArchiveLink, outcome: Result<ArchiveTarget, &str>) -> Self {
        let (url, banner) = match outcome {
            Ok(ArchiveTarget::Results {
                url,
                matches: Some(0),
            }) => (url, Some(LandingBanner::of("archive_viewer.not_found"))),
            Ok(ArchiveTarget::Results {
                url,
                matches: Some(2..),
            }) => (url, Some(LandingBanner::of("archive_viewer.ambiguous"))),
            Ok(ArchiveTarget::View {
                url,
                views,
                view_count,
                ..
            }) if views.is_empty() => {
                let banner =
                    unaddressed_view(&link.citation, view_count).map(|view| LandingBanner {
                        key: "archive_viewer.go_to_view",
                        view: Some(view),
                    });
                (url, banner)
            }
            Ok(target) => (target.url().to_owned(), None),
            Err(code @ "challenged") => {
                let url = ArchiveRegistry::embedded()
                    .offline_target(&link.citation)
                    .map_or_else(
                        |_| link.archive.website.clone(),
                        |target| target.url().to_owned(),
                    );
                (url, Some(LandingBanner::of(failure_key(code))))
            }
            Err(code) => (
                link.archive.website.clone(),
                Some(LandingBanner::of(failure_key(code))),
            ),
        };
        Self { url, banner }
    }
}

/// The first cited view of a register target that opens on its first view
/// although every cited view lies within the register — or the register's
/// size is unknown —: a portal without an address per view. A citation
/// naming no view, or one beyond the register (§7), has none.
fn unaddressed_view(citation: &CitationParts, view_count: Option<u16>) -> Option<u16> {
    let first = citation.views.first()?.view;
    let within =
        view_count.is_none_or(|count| citation.views.iter().all(|cited| cited.view <= count));
    within.then_some(first)
}

/// What the archive window tells the reader, in the interface language.
///
/// The portal page is not ours to translate, so the window shows these in a
/// small banner over it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveViewerMessages {
    pub searching: String,
    pub close: String,
    /// Asks the reader to answer an anti-bot check in the window.
    pub challenge: String,
    /// Says that the portal's certificate could not be verified.
    pub certificate: String,
    /// The label of the button opening the page in the system browser.
    pub open_in_browser: String,
    /// The text of each banner of [`BANNER_KEYS`].
    banners: Vec<(&'static str, String)>,
}

impl ArchiveViewerMessages {
    pub fn new(i18n: &I18n) -> Self {
        Self {
            searching: i18n.t("archive_viewer.searching"),
            close: i18n.t("common.close"),
            challenge: i18n.t("archive_viewer.challenge"),
            certificate: i18n.t("archive_viewer.certificate"),
            open_in_browser: i18n.t("archive_viewer.open_in_browser"),
            banners: BANNER_KEYS.map(|key| (key, i18n.t(key))).to_vec(),
        }
    }

    /// The text of a [`Landing`]'s banner.
    pub fn banner(&self, banner: LandingBanner) -> Option<String> {
        self.banners
            .iter()
            .find(|(known, _)| *known == banner.key)
            .map(|(_, text)| fill(text.clone(), banner.view))
    }
}

/// One request to open a register.
#[derive(Clone, Debug, PartialEq)]
pub struct ArchiveViewerRequest {
    pub link: ArchiveLink,
    pub messages: ArchiveViewerMessages,
}

/// A portal page to open as it is, without resolving anything: the page of a
/// view OxidGene already shows, or the landing of a resolution the backend
/// ran.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchivePageRequest {
    /// The window's title.
    pub title: String,
    pub url: String,
    /// What to tell the reader over the page, in the interface language.
    pub banner: Option<String>,
    /// What else the window may say: its close button, an anti-bot check.
    pub messages: ArchiveViewerMessages,
}

/// The platform side of the archive viewer.
pub trait ArchiveViewerOpener: Send + Sync {
    /// Whether this platform can open `link`.
    fn supports(&self, link: &ArchiveLink) -> bool;
    fn open(&self, request: ArchiveViewerRequest);
    /// Opens a portal page in an archive window.
    fn open_page(&self, request: ArchivePageRequest);
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

    pub fn open_page(&self, request: ArchivePageRequest) {
        self.0.open_page(request);
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

    fn banner_key(landing: &Landing) -> Option<&'static str> {
        landing.banner.map(|banner| banner.key)
    }

    #[test]
    fn a_landing_says_what_the_resolution_found() {
        let cited = link(
            "AD44 - Exampleville - (aucun) - N - 1877 - 3E1/2 - vue 5d/13",
            None,
        )
        .unwrap();
        let results = |matches| ArchiveTarget::Results {
            url: "https://archives.example.org/search".to_owned(),
            matches,
        };
        assert_eq!(Landing::of(&cited, Ok(results(Some(1)))).banner, None);
        assert_eq!(Landing::of(&cited, Ok(results(None))).banner, None);
        assert_eq!(
            banner_key(&Landing::of(&cited, Ok(results(Some(0))))),
            Some("archive_viewer.not_found")
        );
        assert_eq!(
            banner_key(&Landing::of(&cited, Ok(results(Some(3))))),
            Some("archive_viewer.ambiguous")
        );

        let failed = Landing::of(&cited, Err("timeout"));
        assert_eq!(failed.url, cited.archive.website);
        assert_eq!(banner_key(&failed), Some("archive_viewer.timeout"));
        assert_eq!(
            banner_key(&Landing::of(&cited, Err("internal_error"))),
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

    #[test]
    fn a_challenge_lands_on_the_filtered_search_page() {
        let cited = link("AD44 - Exampleville - (aucun) - N - 1877", None).unwrap();
        let landing = Landing::of(&cited, Err("challenged"));
        let results = ArchiveRegistry::embedded()
            .offline_target(&cited.citation)
            .unwrap();
        assert_eq!(landing.url, results.url());
        assert_ne!(landing.url, cited.archive.website);
        assert_eq!(banner_key(&landing), Some("archive_viewer.challenged"));
    }

    #[test]
    fn a_register_without_an_address_per_view_names_the_cited_view() {
        let register = |view_count| ArchiveTarget::View {
            url: "https://archives.example.org/register".to_owned(),
            views: Vec::new(),
            view_count,
            call_number: None,
            attribution: None,
        };
        let cited = link(
            "AD44 - Exampleville - (aucun) - N - 1877 - vue 5d-6g/13",
            None,
        )
        .unwrap();
        for count in [Some(13), Some(6), None] {
            let landing = Landing::of(&cited, Ok(register(count)));
            assert_eq!(landing.url, "https://archives.example.org/register");
            assert_eq!(
                landing.banner,
                Some(LandingBanner {
                    key: "archive_viewer.go_to_view",
                    view: Some(5)
                }),
                "{count:?}"
            );
        }
        // A cited view beyond the register: it opens on its first view.
        assert_eq!(Landing::of(&cited, Ok(register(Some(5)))).banner, None);
        // A citation naming no view.
        let whole = link("AD44 - Exampleville - (aucun) - N - 1877", None).unwrap();
        assert_eq!(Landing::of(&whole, Ok(register(Some(13)))).banner, None);

        let i18n = I18n::new(crate::i18n::Language::english());
        let banner = LandingBanner {
            key: "archive_viewer.go_to_view",
            view: Some(5),
        };
        let text = banner.text(&i18n);
        assert!(text.contains('5') && !text.contains("{view}"), "{text}");
        assert_eq!(ArchiveViewerMessages::new(&i18n).banner(banner), Some(text));
    }
}
