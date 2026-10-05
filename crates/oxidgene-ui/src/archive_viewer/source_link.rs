//! A cited source offered as a link to its archive register.

use dioxus::document::{self, Eval};
use dioxus::prelude::*;
use futures_util::StreamExt;
use uuid::Uuid;

use oxidgene_archives::{ArchiveTarget, Display};

use super::attach::AttachForm;
use super::find::FindInArchives;
use super::{
    ArchiveAddress, ArchiveLink, ArchiveOffer, ArchivePageRequest, ArchiveRegister,
    ArchiveViewerBridge, ArchiveViewerMessages, ArchiveViewerRequest, AttachSender, Landing,
    ViewPage,
};
use crate::api::{ApiClient, ApiError, UpdateCitationBody};
use crate::i18n::{I18n, use_i18n};

/// The views a reader is attaching, and the register they belong to.
type Attaching = (ArchiveRegister, Vec<ViewPage>);

/// `text`, the rendered citation, as a button acting on `offer`. A register
/// opens on its portal, for every archive alike: in the desktop's archive
/// window, or, on the web, in a new browser tab with a notice beside the
/// button when the lookup has something to say. A portal address opens as
/// it is, the same way. A citation missing its act or locality opens the
/// "Find in the archives" dialog, whose completed parts open the register as
/// above. Plain text where the desktop cannot open a register.
///
/// For an archive whose images OxidGene may use (`display: "iiif"`), the
/// reader may attach the cited views as a document of event `event_id`:
/// from the archive window on the desktop, which sends its target back
/// here, and from a button beside the source on the web. `on_attached`
/// fires once something is written: a document saved, or the parts the
/// reader completed written into the citation's page.
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
    let mut finding = use_signal(|| false);
    let mut attaching = use_signal(|| None::<Attaching>);
    let archive = offer.archive();
    let attachable = archive.display == Display::Iiif;
    let hint = i18n.t_args("person.source_open_archive", &[("archive", &archive.name)]);

    let received = use_attach_targets(tree_id, &offer, bridge.clone(), attaching);

    if let (ArchiveOffer::Register(link), Some(bridge)) = (&offer, &bridge)
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
            if let Some(bridge) = &bridge {
                let attach = (link.archive.display == Display::Iiif)
                    .then(|| AttachSender::new(received.tx()));
                bridge.open(ArchiveViewerRequest {
                    link,
                    messages: ArchiveViewerMessages::new(&i18n),
                    attach,
                });
                return;
            }
            open_in_tab(&api, tree_id, link, notice, i18n);
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
    // On the web, the cited views are attached from beside the source: one
    // lookup on the reader's click.
    let on_attach = {
        let (offer, api) = (offer.clone(), api.clone());
        move |_| {
            let ArchiveOffer::Register(link) = offer.clone() else {
                return;
            };
            notice.set(None);
            let api = api.clone();
            spawn(async move {
                match attachable_views(&api, tree_id, &link).await {
                    Ok(found) => attaching.set(Some(found)),
                    Err(message) => notice.set(Some(message.text(&i18n))),
                }
            });
        }
    };

    rsx! {
        button {
            class: "pd-ev-source-link",
            title: "{hint}",
            onclick: on_click,
            "{text}"
        }
        if attachable && bridge.is_none() && matches!(offer, ArchiveOffer::Register(_)) {
            button {
                class: "pd-ev-source-attach",
                r#type: "button",
                onclick: on_attach,
                {i18n.t("archive_viewer.attach")}
            }
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
        if let Some((register, pages)) = attaching() {
            AttachForm {
                register,
                pages,
                event: (event_id, event_label.clone()),
                on_created: move |_| {
                    attaching.set(None);
                    on_attached.call(());
                },
                on_close: move |()| attaching.set(None),
            }
        }
    }
}

/// The targets the desktop's archive window sends back when the reader asks
/// to attach the views it shows: each opens the document form here, and
/// brings the application's window forward.
fn use_attach_targets(
    tree_id: Uuid,
    offer: &ArchiveOffer,
    bridge: Option<ArchiveViewerBridge>,
    mut attaching: Signal<Option<Attaching>>,
) -> Coroutine<ArchiveTarget> {
    let offer = offer.clone();
    use_coroutine(move |mut targets: UnboundedReceiver<ArchiveTarget>| {
        let (bridge, offer) = (bridge.clone(), offer.clone());
        async move {
            while let Some(target) = targets.next().await {
                let ArchiveOffer::Register(link) = &offer else {
                    continue;
                };
                if let Some(found) = ArchiveRegister::of(tree_id, link, &target) {
                    attaching.set(Some(found));
                    if let Some(bridge) = &bridge {
                        bridge.focus();
                    }
                }
            }
        }
    })
}

/// Opens `link`'s register in a new browser tab, during the reader's click,
/// and says beside the source what the lookup found.
fn open_in_tab(
    api: &ApiClient,
    tree_id: Uuid,
    link: ArchiveLink,
    notice: Signal<Option<String>>,
    i18n: I18n,
) {
    // Opened now, during the click: see `archive_tab.js`.
    let tab = ArchiveTab::open(&i18n.t("archive_viewer.searching"));
    let api = api.clone();
    // The lookup outlives a reader navigating on, so that the tab it opened
    // still reaches its page.
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
        let mut notice = notice;
        if let Ok(mut current) = notice.try_write() {
            *current = landing.banner.map(|banner| banner.text(&i18n));
        }
    });
}

/// Why no views can be attached, said beside the source.
enum Unattachable {
    /// The lookup ended elsewhere than on views with images: its landing.
    Landing(Landing),
    /// It failed, with this code.
    Failed(Option<String>),
}

impl Unattachable {
    fn text(&self, i18n: &I18n) -> String {
        match self {
            Self::Landing(landing) => landing.banner.map_or_else(
                || i18n.t("archive_viewer.no_image"),
                |banner| banner.text(i18n),
            ),
            Self::Failed(code) => i18n.t(super::failure_key(code.as_deref().unwrap_or_default())),
        }
    }
}

/// The cited views of `link`'s register with their images, resolved by the
/// backend.
async fn attachable_views(
    api: &ApiClient,
    tree_id: Uuid,
    link: &ArchiveLink,
) -> Result<Attaching, Unattachable> {
    let target = api
        .archive_target(
            tree_id,
            link.source_id,
            link.citation_id,
            None,
            link.supplied.as_ref(),
        )
        .await
        .map_err(|error| Unattachable::Failed(error.code()))?;
    ArchiveRegister::of(tree_id, link, &target)
        .ok_or_else(|| Unattachable::Landing(Landing::of(link, Ok(target))))
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
