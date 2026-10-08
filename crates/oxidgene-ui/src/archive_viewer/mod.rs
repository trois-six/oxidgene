//! Opening a cited archive register at the cited view.
//!
//! The interface recognizes citations of the archives the catalogue lists,
//! in whatever convention they are written, from the source, the citation,
//! the repositories holding the source and the cited event — both provided
//! by `oxidgene-archives` (docs/archives.md §5.1) — and offers such a source
//! as [`ArchiveSourceLink`]: a register to look up, a portal address to
//! open as it is, or, when the archive is known but the act or the locality
//! is not, the "Find in the archives" dialog ([`find`]). Every archive opens
//! on its portal: on the desktop the binary injects an
//! [`ArchiveViewerOpener`] that resolves the citation in an archive window;
//! the web client, which has none, asks the backend for the target and opens
//! it in a new browser tab. For an archive whose images OxidGene may use
//! (`display: "iiif"`), the cited views can be attached as a document
//! ([`attach`]): from the archive window on the desktop, from beside the
//! source on the web — an offer disabled for now by [`ATTACH_OFFERED`].

mod attach;
mod find;
mod register;
mod source_link;

use std::sync::Arc;

use dioxus::prelude::try_use_context;
use futures_channel::mpsc::UnboundedSender;
use oxidgene_archives::{
    Archive, ArchiveRegistry, ArchiveTarget, CitationEvidence, CitationParts, Display, Found,
    Licence, Part, SuppliedParts,
};
use uuid::Uuid;

use crate::i18n::I18n;
use crate::theme::Theme;

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
    /// What the reader completed in the "Find in the archives" dialog, sent
    /// with every resolution of this link.
    pub supplied: Option<SuppliedParts>,
    /// The records the citation was recognized from, which the desktop reads
    /// again with the place dictionary before resolving.
    pub evidence: CitationEvidence,
}

/// A portal address of a catalogued archive found in a citation's records:
/// opened as it is, without any lookup.
#[derive(Clone, Debug, PartialEq)]
pub struct ArchiveAddress {
    pub archive: &'static Archive,
    pub url: String,
    /// The source title, naming the window.
    pub title: String,
}

/// A citation of a catalogued archive that misses the act or the locality:
/// what was recognized, for the reader to complete.
#[derive(Clone, Debug, PartialEq)]
pub struct ArchiveFind {
    pub archive: &'static Archive,
    pub found: Found,
    pub missing: Vec<Part>,
    pub title: String,
    pub source_id: Uuid,
    pub citation_id: Option<Uuid>,
    pub evidence: CitationEvidence,
}

impl ArchiveFind {
    /// The parts the reader changed or added in the dialog: a field left
    /// as recognized stays the records', so that only what the reader
    /// supplied rides with the resolution and is written back.
    pub fn changes(&self, parts: SuppliedParts) -> SuppliedParts {
        let found = &self.found;
        SuppliedParts {
            locality: parts
                .locality
                .filter(|locality| found.locality.as_deref() != Some(locality.as_str())),
            act: parts.act.filter(|act| found.act.as_ref() != Some(act)),
            year: parts.year.filter(|year| found.year != Some(*year)),
            view: parts
                .view
                .filter(|view| found.views.first().map(|cited| cited.view) != Some(*view)),
        }
    }

    /// The register link the reader's parts complete, with those parts
    /// written in the archive's language for the citation's page; `None`
    /// while the act or the locality is still missing.
    pub fn complete(&self, supplied: SuppliedParts) -> Option<(ArchiveLink, Option<String>)> {
        let registry = ArchiveRegistry::embedded();
        supplied.validate(self.archive).ok()?;
        let recognition = registry
            .recognize(&self.evidence, Some(&supplied), None)
            .ok()?;
        let citation = recognition.citation()?;
        let written = recognition.written(registry);
        let link = ArchiveLink {
            archive: self.archive,
            citation,
            title: self.title.clone(),
            source_id: self.source_id,
            citation_id: self.citation_id,
            supplied: Some(supplied),
            evidence: self.evidence.clone(),
        };
        Some((link, written))
    }
}

/// What a cited source offers the reader.
#[derive(Clone, Debug, PartialEq)]
pub enum ArchiveOffer {
    /// A register to look up.
    Register(ArchiveLink),
    /// A portal address to open as it is.
    Address(ArchiveAddress),
    /// The "Find in the archives" dialog.
    Find(ArchiveFind),
}

impl ArchiveOffer {
    /// What a citation of source `source_id`, by `citation_id`, offers from
    /// its records; `None` when they name no register of a catalogued
    /// archive with an adapter.
    pub fn of(
        source_id: Uuid,
        citation_id: Option<Uuid>,
        evidence: CitationEvidence,
    ) -> Option<Self> {
        let registry = ArchiveRegistry::embedded();
        let recognition = registry.recognize(&evidence, None, None).ok()?;
        let title = evidence.title.clone();
        if let Some(url) = recognition.address.clone() {
            return Some(Self::Address(ArchiveAddress {
                archive: recognition.archive,
                url,
                title,
            }));
        }
        Some(match recognition.citation() {
            Some(citation) => Self::Register(ArchiveLink {
                archive: recognition.archive,
                citation,
                title,
                source_id,
                citation_id,
                supplied: None,
                evidence,
            }),
            None => Self::Find(ArchiveFind {
                archive: recognition.archive,
                missing: recognition.missing(),
                found: recognition.found,
                title,
                source_id,
                citation_id,
                evidence,
            }),
        })
    }

    /// The archive the citation belongs to.
    pub fn archive(&self) -> &'static Archive {
        match self {
            Self::Register(link) => link.archive,
            Self::Address(address) => address.archive,
            Self::Find(find) => find.archive,
        }
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
const BANNER_KEYS: [&str; 13] = [
    "archive_viewer.licence",
    "archive_viewer.licence_tab",
    "archive_viewer.not_found",
    "archive_viewer.ambiguous",
    "archive_viewer.go_to_view",
    "archive_viewer.renumbered",
    "archive_viewer.failed",
    "archive_viewer.no_adapter",
    "archive_viewer.not_an_archive_citation",
    "archive_viewer.unexpected_response",
    "archive_viewer.challenged",
    "archive_viewer.timeout",
    "archive_viewer.unreachable",
];

/// What to tell the reader over a [`Landing`]: a translation key, the view
/// number its text names, if any, and the image counts it compares — the
/// citation's, then the register's —, if any.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LandingBanner {
    pub key: &'static str,
    pub view: Option<u16>,
    pub counts: Option<(u16, u16)>,
}

impl LandingBanner {
    const fn of(key: &'static str) -> Self {
        Self {
            key,
            view: None,
            counts: None,
        }
    }

    /// The banner's text in the interface language.
    pub fn text(&self, i18n: &I18n) -> String {
        self.fill(i18n.t(self.key))
    }

    /// The view the banner tells the reader to go to: over a register whose
    /// portal has no address per view, which the desktop window brings to
    /// that view itself when it can (docs/archives.md §6.1).
    pub fn go_to_view(&self) -> Option<u16> {
        self.view
            .filter(|_| self.key == "archive_viewer.go_to_view")
    }

    /// `text` with its `{view}`, `{cited}` and `{count}` placeholders
    /// filled.
    fn fill(&self, mut text: String) -> String {
        if let Some(view) = self.view {
            text = text.replace("{view}", &view.to_string());
        }
        if let Some((cited, count)) = self.counts {
            text = text
                .replace("{cited}", &cited.to_string())
                .replace("{count}", &count.to_string());
        }
        text
    }
}

/// The page a resolution ends on, and what to tell the reader about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Landing {
    pub url: String,
    /// The banner, when there is something to say.
    pub banner: Option<LandingBanner>,
    /// For a target behind the portal's reuse licence, which `url` — the
    /// licence's entry — leads to: the target, which the archive window
    /// opens once the reader has accepted the licence themselves.
    pub then: Option<Onward>,
}

/// The target a [`Landing`] goes on to once the reader has passed the
/// portal's reuse licence (docs/archives.md §6.1). OxidGene never accepts
/// the licence: the archive window waits for the reader's acceptance — a
/// page behind the licence showing after the licence page — then opens it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Onward {
    pub url: String,
    /// The licence: its page, and the pages behind it.
    pub licence: Licence,
    /// The banner over the target.
    pub banner: Option<LandingBanner>,
}

impl Landing {
    /// The target, with a banner when no register or several registers
    /// match, or when the register opens on its first view though the
    /// citation names one within it: the portal has no address per view,
    /// and the reader goes to the view. On a failure, given by its code,
    /// the failure's banner over the collection's filtered search page when
    /// an anti-bot check answered — the reader may pass it there — or the
    /// portal was too slow or out of reach — the page may load for the
    /// reader —, and over the archive's website otherwise.
    ///
    /// A target behind the portal's reuse licence lands on the licence's
    /// entry, saying that the reader accepts it there, and goes on to the
    /// target ([`Onward`]).
    pub fn of(link: &ArchiveLink, outcome: Result<ArchiveTarget, &str>) -> Self {
        let resolved = outcome.is_ok();
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
                        counts: None,
                    });
                (url, banner)
            }
            // The register counts another number of images than the
            // citation: the view opened may not be the cited page.
            Ok(ArchiveTarget::View {
                url,
                views,
                view_count: Some(count),
                renumbering: Some(renumbering),
                ..
            }) => {
                let banner = views.first().map(|opened| LandingBanner {
                    key: "archive_viewer.renumbered",
                    view: Some(opened.view),
                    counts: Some((renumbering.cited_count, count)),
                });
                (url, banner)
            }
            Ok(target) => (target.url().to_owned(), None),
            Err(code @ ("challenged" | "timeout" | "unreachable")) => {
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
        let licence = resolved
            .then(|| ArchiveRegistry::embedded().licence(&link.citation.code, &url))
            .flatten();
        match licence {
            Some(licence) => Self {
                url: licence.entry.clone(),
                banner: Some(LandingBanner::of("archive_viewer.licence")),
                then: Some(Onward {
                    url,
                    licence,
                    banner,
                }),
            },
            None => Self {
                url,
                banner,
                then: None,
            },
        }
    }

    /// What a tab, which cannot go on past a licence for the reader, says
    /// beside the source: the landing's banner, or for a target behind a
    /// licence, the target's, or that the licence comes first.
    pub fn notice(&self) -> Option<LandingBanner> {
        match &self.then {
            Some(then) => then
                .banner
                .or(Some(LandingBanner::of("archive_viewer.licence_tab"))),
            None => self.banner,
        }
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
/// small banner over it, and, while it resolves, in a progress overlay
/// covering it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveViewerMessages {
    /// Heads the progress overlay.
    pub searching: String,
    /// The overlay's steps: the portal's page loading, the register being
    /// searched for, and the landing opening, on the cited view
    /// (`{view}`) when there is one.
    pub step_connecting: String,
    pub step_searching: String,
    pub step_opening: String,
    pub step_opening_view: String,
    /// How long a step has lasted, with a `{seconds}` placeholder.
    pub step_elapsed: String,
    /// The label of the overlay's button stopping the lookup.
    pub cancel: String,
    pub close: String,
    /// Asks the reader to answer an anti-bot check in the window.
    pub challenge: String,
    /// Says that the portal's certificate could not be verified.
    pub certificate: String,
    /// Says that the page on screen is a server's error page in place of
    /// the portal's: its gateway timed out (`504`), or another server
    /// error; and the label of the button loading the page again.
    pub page_timeout: String,
    pub page_error: String,
    pub reload: String,
    /// The label of the button opening the page in the system browser.
    pub open_in_browser: String,
    /// What the window says over a view the reader may attach, and the
    /// label of the button doing so.
    pub attach_hint: String,
    pub attach: String,
    /// The custom properties of the active theme (`Theme::declarations`),
    /// which the progress overlay declares over the portal's page so that
    /// it, and its [`SPINNER_STYLES`](crate::components::layout::SPINNER_STYLES),
    /// look as they do in the application.
    pub palette: String,
    /// The text of each banner of [`BANNER_KEYS`].
    banners: Vec<(&'static str, String)>,
}

impl ArchiveViewerMessages {
    /// The window's texts in the language of `i18n`, and the colours of
    /// `theme`.
    pub fn new(i18n: &I18n, theme: &Theme) -> Self {
        Self {
            palette: theme.declarations(),
            searching: i18n.t("archive_viewer.searching"),
            step_connecting: i18n.t("archive_viewer.step_connecting"),
            step_searching: i18n.t("archive_viewer.step_searching"),
            step_opening: i18n.t("archive_viewer.step_opening"),
            step_opening_view: i18n.t("archive_viewer.step_opening_view"),
            step_elapsed: i18n.t("archive_viewer.step_elapsed"),
            cancel: i18n.t("common.cancel"),
            close: i18n.t("common.close"),
            challenge: i18n.t("archive_viewer.challenge"),
            certificate: i18n.t("archive_viewer.certificate"),
            page_timeout: i18n.t("archive_viewer.page_timeout"),
            page_error: i18n.t("archive_viewer.page_error"),
            reload: i18n.t("archive_viewer.reload"),
            open_in_browser: i18n.t("archive_viewer.open_in_browser"),
            attach_hint: i18n.t("archive_viewer.attach_hint"),
            attach: i18n.t("archive_viewer.attach"),
            banners: BANNER_KEYS.map(|key| (key, i18n.t(key))).to_vec(),
        }
    }

    /// The text of a [`Landing`]'s banner.
    pub fn banner(&self, banner: LandingBanner) -> Option<String> {
        self.banners
            .iter()
            .find(|(known, _)| *known == banner.key)
            .map(|(_, text)| banner.fill(text.clone()))
    }
}

/// Whether OxidGene offers the reader to attach the cited views of an
/// archive whose images it may use (`display: "iiif"`) as a document:
/// off. Attaching is implemented, but viewing the source an event cites is
/// not where a reader collects documents; the offer is kept for a future
/// free browsing of the archives, where a reader who finds a document may
/// want to attach it to a person of the tree (docs/archives.md §6.3).
///
/// The one switch of every entry point, each reading it through
/// [`offers_attach`]: the web's **Attach as a document** button beside the
/// source, the target the desktop's archive window sends back, and the
/// offer in that window's banner. Turning it on restores all of them; the
/// attaching path itself stays built and tested either way.
pub const ATTACH_OFFERED: bool = false;

/// Whether the cited views of `archive` are offered for attaching while the
/// switch is `offered` — [`ATTACH_OFFERED`] in the application: only for an
/// archive whose images OxidGene may use.
pub fn offers_attach(archive: &Archive, offered: bool) -> bool {
    offered && archive.display == Display::Iiif
}

/// One request to open a register.
#[derive(Clone, Debug, PartialEq)]
pub struct ArchiveViewerRequest {
    pub link: ArchiveLink,
    pub messages: ArchiveViewerMessages,
    /// Where the window sends the target it shows when the reader asks to
    /// attach its views: given for an archive whose images OxidGene may use
    /// while [`ATTACH_OFFERED`] is on (see [`offers_attach`]).
    pub attach: Option<AttachSender>,
}

/// The interface's end of the archive window's « Attach as a document »:
/// the resolved target, whose views the interface attaches in its own
/// window (docs/archives.md §6.1, §6.4).
#[derive(Clone, Debug)]
pub struct AttachSender(UnboundedSender<ArchiveTarget>);

impl AttachSender {
    pub fn new(sender: UnboundedSender<ArchiveTarget>) -> Self {
        Self(sender)
    }

    /// Sends `target`; `false` when the page that asked is gone.
    pub fn send(&self, target: ArchiveTarget) -> bool {
        self.0.unbounded_send(target).is_ok()
    }
}

impl PartialEq for AttachSender {
    fn eq(&self, other: &Self) -> bool {
        self.0.same_receiver(&other.0)
    }
}

impl Eq for AttachSender {}

/// A portal page to open as it is, without resolving anything: a portal
/// address found in a citation.
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
    /// Brings the application's window forward, over the archive windows:
    /// the reader asked to attach a view there.
    fn focus(&self);
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

    pub fn focus(&self) {
        self.0.focus();
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

    /// The register link a citation of a source written `title`, with the
    /// citation's `page`, stands for, when it names one.
    pub(super) fn link(title: &str, page: Option<&str>) -> Option<ArchiveLink> {
        let evidence = CitationEvidence {
            title: title.to_owned(),
            page: page.map(str::to_owned),
            ..CitationEvidence::default()
        };
        match ArchiveOffer::of(Uuid::nil(), None, evidence)? {
            ArchiveOffer::Register(link) => Some(link),
            ArchiveOffer::Address(_) | ArchiveOffer::Find(_) => None,
        }
    }

    fn offer(title: &str, page: Option<&str>) -> Option<ArchiveOffer> {
        let evidence = CitationEvidence {
            title: title.to_owned(),
            page: page.map(str::to_owned),
            ..CitationEvidence::default()
        };
        ArchiveOffer::of(Uuid::nil(), None, evidence)
    }

    /// Attaching is disabled for now ([`ATTACH_OFFERED`]): no archive's
    /// cited views are offered, not even an archive whose images OxidGene
    /// may use, while the switch on offers those and only those.
    #[test]
    fn the_offer_to_attach_follows_its_switch() {
        let iiif = link(
            "AD37 - Exampleville - (aucun) - N - 1877 - 3E1/2 - vue 5/13",
            None,
        )
        .expect("a catalogued citation")
        .archive;
        let portal = link("AD44 - Exampleville - (aucun) - N - 1877 - vue 5/13", None)
            .expect("a catalogued citation")
            .archive;
        assert_eq!(iiif.display, Display::Iiif);
        assert_eq!(offers_attach(iiif, ATTACH_OFFERED), ATTACH_OFFERED);
        assert!(!offers_attach(iiif, false));
        assert!(offers_attach(iiif, true));
        assert!(!offers_attach(portal, true));
    }

    #[test]
    fn a_citation_offers_a_register_an_address_or_the_dialog() {
        let Some(ArchiveOffer::Register(link)) = offer(
            "Archives départementales de la Sarthe, état civil de Exampleville, naissances 1872",
            Some("vue 45, acte 312"),
        ) else {
            panic!("a register");
        };
        assert_eq!(link.archive.id, "fr-ad72");
        assert_eq!(link.citation.locality, "Exampleville");
        assert_eq!(link.citation.views[0].view, 45);
        assert_eq!(link.supplied, None);

        let Some(ArchiveOffer::Address(address)) = offer(
            "Acte de naissance",
            Some("https://archives.sarthe.fr/archives-en-ligne/ark:/99999/a0000"),
        ) else {
            panic!("an address");
        };
        assert_eq!(address.archive.id, "fr-ad72");
        assert!(address.url.ends_with("/a0000"));

        let Some(ArchiveOffer::Find(find)) = offer("AD72, 4E 1234", Some("vue 45")) else {
            panic!("a citation to complete");
        };
        assert_eq!(find.missing, [Part::Act, Part::Locality]);
        assert_eq!(find.found.views[0].view, 45);

        // A kind of document nobody holds is no link, not even the dialog.
        assert_eq!(
            offer(
                "AD72 - Exampleville - Registres d’écrou des condamnés 27 octobre 1853-14 juin 1855 - 2Y2 26 - vue 165g/226",
                None
            ),
            None
        );
        assert_eq!(
            offer("AD72, Exampleville, Minutes notariales 1750", None),
            None
        );

        assert_eq!(offer("Fictitious register", None), None);
        assert_eq!(offer("AD98, état civil de Exampleville", None), None);
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
            link("AD44 - Exampleville - (aucun) - TB - 1877", None),
            None
        );
        // A well-formed citation of an archive the catalogue does not list.
        assert_eq!(link("AD99 - Exampleville - (aucun) - N - 1877", None), None);
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

        let failed = Landing::of(&cited, Err("unexpected_response"));
        assert_eq!(failed.url, cited.archive.website);
        assert_eq!(
            banner_key(&failed),
            Some("archive_viewer.unexpected_response")
        );
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
    fn a_challenge_or_a_slow_portal_lands_on_the_filtered_search_page() {
        let cited = link("AD44 - Exampleville - (aucun) - N - 1877", None).unwrap();
        let results = ArchiveRegistry::embedded()
            .offline_target(&cited.citation)
            .unwrap();
        for code in ["challenged", "timeout", "unreachable"] {
            let landing = Landing::of(&cited, Err(code));
            assert_eq!(landing.url, results.url(), "{code}");
            assert_ne!(landing.url, cited.archive.website, "{code}");
            assert_eq!(banner_key(&landing), Some(failure_key(code)), "{code}");
        }
    }

    /// A regression: a lookup on the Sarthe portal that timed out landed on
    /// the portal's home page, its banner saying to continue on the search
    /// page. It lands on the collection's search page, filtered by the
    /// locality as the portal writes it and the act.
    #[test]
    fn a_timed_out_lookup_lands_on_the_filtered_results_of_its_collection() {
        let cited = link(
            "AD72 - Le Bourg - N - 1882 - 5Mi 999_1 - acte 12 - vue 43d/225",
            None,
        )
        .unwrap();
        let landing = Landing::of(&cited, Err("timeout"));
        let (path, query) = landing.url.split_once('?').unwrap();
        assert_eq!(
            path,
            "https://archives.sarthe.fr/archives-en-ligne/registres-paroissiaux-etat-civil"
        );
        assert!(query.contains("%5Bq%5D%5B%5D=Bourg%20%28Le%29&"), "{query}");
        assert!(
            query.contains("%5Bq%5D%5B%5D=Naissances%5B%5Barko_fiche_6304c294c56c4%5D%5D&"),
            "{query}"
        );
        assert_eq!(banner_key(&landing), Some("archive_viewer.timeout"));
    }

    #[test]
    fn a_register_without_an_address_per_view_names_the_cited_view() {
        let register = |view_count| ArchiveTarget::View {
            url: "https://archives.example.org/register".to_owned(),
            views: Vec::new(),
            view_count,
            call_number: None,
            attribution: None,
            renumbering: None,
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
                    view: Some(5),
                    counts: None,
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
            counts: None,
        };
        assert_eq!(Landing::of(&cited, Ok(register(None))).then, None);
        let text = banner.text(&i18n);
        assert!(text.contains('5') && !text.contains("{view}"), "{text}");
        assert_eq!(
            ArchiveViewerMessages::new(&i18n, &crate::theme::BUILTIN_THEMES[0]).banner(banner),
            Some(text)
        );
        // The view the desktop window brings the viewer to.
        assert_eq!(banner.go_to_view(), Some(5));
        assert_eq!(
            LandingBanner::of("archive_viewer.not_found").go_to_view(),
            None
        );
    }

    /// A regression: a register bound with earlier years since the
    /// citation was numbered opened silently on another page than the
    /// cited one. The banner says the numbering changed and which view
    /// opened.
    #[test]
    fn a_renumbered_register_says_so() {
        let cited = link(
            "AD44 - Exampleville - (aucun) - N - 1877 - vue 38d/184",
            None,
        )
        .unwrap();
        let target = |renumbering| ArchiveTarget::View {
            url: "https://archives.example.org/register#155".to_owned(),
            views: vec![oxidgene_archives::ArchiveView {
                view: 155,
                url: "https://archives.example.org/register#155".to_owned(),
                ark: None,
                image: None,
            }],
            view_count: Some(301),
            call_number: None,
            attribution: None,
            renumbering,
        };
        let landing = Landing::of(
            &cited,
            Ok(target(Some(oxidgene_archives::Renumbering {
                cited_count: 184,
                shifted_by: 117,
            }))),
        );
        assert_eq!(landing.url, "https://archives.example.org/register#155");
        let banner = landing.banner.expect("a banner");
        assert_eq!(banner.key, "archive_viewer.renumbered");
        let i18n = I18n::new(crate::i18n::Language::english());
        let text = banner.text(&i18n);
        for part in ["184", "301", "155"] {
            assert!(text.contains(part), "{text}");
        }
        assert!(!text.contains('{'), "{text}");
        assert_eq!(Landing::of(&cited, Ok(target(None))).banner, None);
    }

    /// The owner's case, anonymized: a register of the Côtes-d'Armor stands
    /// behind the portal's reuse licence. The landing is the site's entry,
    /// with the banner asking the reader to accept the licence there, and
    /// goes on to the cited view once they have; the tab says the licence
    /// comes first.
    #[test]
    fn a_target_behind_a_licence_lands_on_its_entry_and_goes_on_to_it() {
        let cited = link(
            "AD22 - Exampleville - N - 1796-1800 - acte 65 - vue 36/248",
            None,
        )
        .unwrap();
        let site = "https://sallevirtuelle.cotesdarmor.fr/EC/ecx";
        let view = format!("{site}/consult.aspx?image=910020100000036");
        let target = ArchiveTarget::View {
            url: view.clone(),
            views: vec![oxidgene_archives::ArchiveView {
                view: 36,
                url: view.clone(),
                ark: None,
                image: None,
            }],
            view_count: Some(248),
            call_number: None,
            attribution: None,
            renumbering: None,
        };
        let landing = Landing::of(&cited, Ok(target));
        assert_eq!(
            landing.url,
            format!("{site}/connexion.aspx?ref=demo&res=1920x1080")
        );
        assert_eq!(banner_key(&landing), Some("archive_viewer.licence"));
        let then = landing.then.clone().expect("the target behind the licence");
        assert_eq!(then.url, view);
        assert_eq!(then.licence.page, format!("{site}/licence.aspx"));
        assert_eq!(then.banner, None);
        assert_eq!(
            landing.notice().map(|banner| banner.key),
            Some("archive_viewer.licence_tab")
        );

        // The locality's lots, the register opening on its first view: the
        // view to go to, once there.
        let lots = ArchiveTarget::View {
            url: format!("{site}/plage.aspx?id=900000000000021"),
            views: Vec::new(),
            view_count: None,
            call_number: None,
            attribution: None,
            renumbering: None,
        };
        let landing = Landing::of(&cited, Ok(lots));
        let then = landing.then.clone().unwrap();
        assert_eq!(
            then.banner.map(|banner| banner.key),
            Some("archive_viewer.go_to_view")
        );
        assert_eq!(landing.notice(), then.banner);

        // A failure lands as before, without going on.
        assert_eq!(Landing::of(&cited, Err("timeout")).then, None);
    }
}
