//! A cited source offered as a link to its archive register.

use dioxus::document::{self, Eval};
use dioxus::prelude::*;
use uuid::Uuid;

use oxidgene_archives::Display;

use super::find::FindInArchives;
use super::viewer::ArchiveViewer;
use super::{
    ArchiveAddress, ArchiveLink, ArchiveOffer, ArchivePageRequest, ArchiveViewerBridge,
    ArchiveViewerMessages, ArchiveViewerRequest, Landing,
};
use crate::api::{ApiClient, ApiError, UpdateCitationBody};
use crate::i18n::{I18n, use_i18n};

/// `text`, the rendered citation, as a button acting on `offer`: a register
/// opens in OxidGene's viewer on both clients for an archive whose images it
/// may show; otherwise in the desktop's archive window, or, on the web, in a
/// new browser tab with a notice beside the button when the lookup has
/// something to say. A portal address opens as it is, the same way. A
/// citation missing its act or locality opens the "Find in the archives"
/// dialog, whose completed parts open the register as above. Plain text
/// where the desktop cannot open a register.
///
/// The citation documents event `event_id`, which a document attached from
/// the viewer documents too; `on_attached` fires once something is written:
/// a document saved, a region kept, or the parts the reader completed
/// written into the citation's page.
#[component]
pub fn ArchiveSourceLink(
    tree_id: Uuid,
    offer: ArchiveOffer,
    text: String,
    event_id: Uuid,
    event_label: String,
    #[props(default)] on_attached: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let bridge = super::use_archive_viewer_bridge();
    let api = use_context::<ApiClient>();
    let mut notice = use_signal(|| None::<String>);
    let mut viewing = use_signal(|| None::<ArchiveLink>);
    let mut finding = use_signal(|| false);
    let archive = offer.archive();
    let hint = i18n.t_args("person.source_open_archive", &[("archive", &archive.name)]);

    if let (ArchiveOffer::Register(link), Some(bridge)) = (&offer, &bridge)
        && archive.display != Display::Iiif
        && !bridge.supports(link)
    {
        return rsx! { "{text}" };
    }

    // Opens a register during the reader's click: the browser opens a tab
    // only then.
    let open = {
        let (bridge, api) = (bridge.clone(), api.clone());
        use_callback(move |link: ArchiveLink| {
            notice.set(None);
            if link.archive.display == Display::Iiif {
                viewing.set(Some(link));
                return;
            }
            if let Some(bridge) = &bridge {
                bridge.open(ArchiveViewerRequest {
                    link,
                    messages: ArchiveViewerMessages::new(&i18n),
                });
                return;
            }
            // Opened now, during the click: see `archive_tab.js`.
            let tab = ArchiveTab::open(&i18n.t("archive_viewer.searching"));
            let api = api.clone();
            // The lookup outlives a reader navigating on, so that the tab it
            // opened still reaches its page.
            dioxus::core::spawn_forever(async move {
                let outcome = api
                    .archive_target(
                        tree_id,
                        link.source_id,
                        link.citation_id,
                        None,
                        link.supplied.as_ref(),
                    )
                    .await;
                let code = outcome.as_ref().err().and_then(ApiError::code);
                let landing = Landing::of(
                    &link,
                    outcome.map_err(|_| code.as_deref().unwrap_or_default()),
                );
                tab.load(&landing.url);
                // The page may be gone by now; then so is the notice.
                if let Ok(mut current) = notice.try_write() {
                    *current = landing.banner.map(|banner| banner.text(&i18n));
                }
            });
        })
    };

    let on_click = {
        let (offer, bridge) = (offer.clone(), bridge.clone());
        move |_| match &offer {
            ArchiveOffer::Register(link) => open.call(link.clone()),
            ArchiveOffer::Address(address) => open_address(bridge.as_ref(), address, &i18n),
            ArchiveOffer::Find(_) => finding.set(true),
        }
    };

    rsx! {
        button {
            class: "pd-ev-source-link",
            title: "{hint}",
            onclick: on_click,
            "{text}"
        }
        if let Some(message) = notice() {
            span { class: "pd-ev-source-notice", role: "status", "{message}" }
        }
        if let (ArchiveOffer::Find(find), true) = (&offer, finding()) {
            FindInArchives {
                find: find.clone(),
                on_find: move |(link, written): (ArchiveLink, Option<String>)| {
                    finding.set(false);
                    if let (Some(written), Some(citation_id)) = (written, link.citation_id) {
                        let (api, page) = (api.clone(), link.evidence.page.clone());
                        spawn(async move {
                            if keep_parts(&api, tree_id, citation_id, page, written).await {
                                on_attached.call(());
                            }
                        });
                    }
                    open.call(link);
                },
                on_close: move |()| finding.set(false),
            }
        }
        if let Some(link) = viewing() {
            ArchiveViewer {
                tree_id,
                link,
                event_id,
                event_label: event_label.clone(),
                on_attached,
                on_close: move |()| viewing.set(None),
            }
        }
    }
}

/// Opens a portal address as it is: in an archive window on the desktop, in
/// a new tab on the web.
fn open_address(bridge: Option<&ArchiveViewerBridge>, address: &ArchiveAddress, i18n: &I18n) {
    match bridge {
        Some(bridge) => bridge.open_page(ArchivePageRequest {
            title: address.title.clone(),
            url: address.url.clone(),
            banner: None,
            messages: ArchiveViewerMessages::new(i18n),
        }),
        None => ArchiveTab::open(&i18n.t("archive_viewer.searching")).load(&address.url),
    }
}

/// Adds the parts the reader completed, `written`, to the end of the
/// citation's `page`, at the reader's request (docs/archives.md §6.5);
/// whether the citation was saved.
async fn keep_parts(
    api: &ApiClient,
    tree_id: Uuid,
    citation_id: Uuid,
    page: Option<String>,
    written: String,
) -> bool {
    let page = match page.as_deref().map(str::trim) {
        Some(page) if !page.is_empty() => format!("{page}, {written}"),
        _ => written,
    };
    let body = UpdateCitationBody {
        page: Some(Some(page)),
        ..UpdateCitationBody::default()
    };
    api.update_citation(tree_id, citation_id, &body)
        .await
        .is_ok()
}

/// A browser tab opened during the reader's click, which the lookup then
/// sends to its page (`archive_tab.js`).
struct ArchiveTab {
    eval: Eval,
}

impl ArchiveTab {
    /// Opens the tab, showing `searching` until [`load`](Self::load).
    fn open(searching: &str) -> Self {
        let searching = serde_json::to_string(searching).expect("a string is serializable");
        Self {
            eval: document::eval(&format!(
                "const searching = {searching};\n{}",
                include_str!("archive_tab.js")
            )),
        }
    }

    fn load(self, url: &str) {
        // A tab the browser refused, or the reader closed, has nothing to
        // load; the script answers nothing either way.
        let _ = self.eval.send(url);
    }
}
