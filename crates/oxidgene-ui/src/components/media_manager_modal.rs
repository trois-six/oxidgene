use dioxus::prelude::*;
use uuid::Uuid;

use crate::components::media_gallery::{MediaEventLinkOption, MediaGallery, MediaOwner};
use crate::i18n::use_i18n;

#[component]
pub fn MediaManagerModal(
    tree_id: Uuid,
    owner: MediaOwner,
    #[props(default)] events: Vec<(Uuid, String)>,
    #[props(default)] profile_event_links: Vec<MediaEventLinkOption>,
    on_changed: EventHandler<()>,
    on_close: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();

    rsx! {
        div {
            class: "cropper-backdrop",
            onmousedown: move |event| event.stop_propagation(),
            onclick: move |_| on_close.call(()),
            div { class: "media-manager-modal", onclick: move |event| event.stop_propagation(),
                div { class: "cropper-head",
                    span { class: "cropper-title", {i18n.t("media.manager_title")} }
                    button {
                        class: "cropper-close",
                        r#type: "button",
                        title: i18n.t("common.close"),
                        onclick: move |_| on_close.call(()),
                        "\u{00D7}"
                    }
                }
                div { class: "media-manager-body",
                    MediaGallery {
                        tree_id,
                        owner,
                        events,
                        profile_event_links,
                        on_changed: move |()| on_changed.call(()),
                    }
                }
            }
        }
    }
}
