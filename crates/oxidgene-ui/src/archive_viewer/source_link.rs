//! A cited source offered as a link to its archive register.

use dioxus::document::{self, Eval};
use dioxus::prelude::*;
use uuid::Uuid;

use super::{ArchiveLink, ArchiveViewerMessages, ArchiveViewerRequest, Landing};
use crate::api::{ApiClient, ApiError};
use crate::i18n::use_i18n;

/// `text`, the rendered citation, as a button opening `link`'s register: in
/// the desktop's archive window, or, on the web, in a new browser tab with
/// a notice beside the button when the lookup has something to say. Plain
/// text where the desktop cannot open the link.
#[component]
pub fn ArchiveSourceLink(tree_id: Uuid, link: ArchiveLink, text: String) -> Element {
    let i18n = use_i18n();
    let bridge = super::use_archive_viewer_bridge();
    let api = use_context::<ApiClient>();
    let mut notice = use_signal(|| None::<String>);
    let hint = i18n.t_args(
        "person.source_open_archive",
        &[("archive", &link.archive.name)],
    );

    match bridge {
        Some(bridge) if bridge.supports(&link) => rsx! {
            button {
                class: "pd-ev-source-link",
                title: "{hint}",
                onclick: move |_| {
                    bridge.open(ArchiveViewerRequest {
                        link: link.clone(),
                        messages: ArchiveViewerMessages::new(&i18n),
                    })
                },
                "{text}"
            }
        },
        Some(_) => rsx! { "{text}" },
        None => rsx! {
            button {
                class: "pd-ev-source-link",
                title: "{hint}",
                onclick: move |_| {
                    // Opened now, during the click: see `archive_tab.js`.
                    let tab = ArchiveTab::open(&i18n.t("archive_viewer.searching"));
                    notice.set(None);
                    let (api, link) = (api.clone(), link.clone());
                    // The lookup outlives a reader navigating on, so that the
                    // tab it opened still reaches its page.
                    dioxus::core::spawn_forever(async move {
                        let outcome = api
                            .archive_target(tree_id, link.source_id, link.citation_id)
                            .await;
                        let code = outcome.as_ref().err().and_then(ApiError::code);
                        let landing = Landing::of(
                            link.archive,
                            outcome.map_err(|_| code.as_deref().unwrap_or_default()),
                        );
                        tab.load(&landing.url);
                        // The page may be gone by now; then so is the notice.
                        if let Ok(mut current) = notice.try_write() {
                            *current = landing.banner.map(|key| i18n.t(key));
                        }
                    });
                },
                "{text}"
            }
            if let Some(message) = notice() {
                span { class: "pd-ev-source-notice", role: "status", "{message}" }
            }
        },
    }
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
