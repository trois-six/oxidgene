//! Bind a crop to what it documents.
//!
//! A vignette is worth keeping the moment it is drawn — "the fourth entry on
//! this page" is a fact about the page — but it earns its place once it says
//! *whose* entry it is and *which* event it evidences. This is the control
//! that says so, and the list of crops already made, side by side.
//!
//! It is deliberately not part of the cropper. Attribution is usually decided
//! after the fact, looking at several crops at once, and forcing the choice at
//! the moment of drawing turns "crop the page" into four interrupted tasks.

use dioxus::prelude::*;
use oxidgene_core::types::Vignette;
use uuid::Uuid;

use crate::api::{ApiClient, UpdateVignetteBody};
use crate::i18n::use_i18n;

#[derive(Props, Clone, PartialEq)]
pub struct VignetteLinkerProps {
    pub tree_id: Uuid,
    /// The crops on one media file.
    pub vignettes: Vec<Vignette>,
    /// Events these crops may be attached to, as (id, label) pairs.
    #[props(default)]
    pub events: Vec<(Uuid, String)>,
    /// Called whenever a crop is re-attributed or deleted, so the caller can
    /// refetch.
    pub on_changed: EventHandler<()>,
}

/// The list of crops on a media, each with what it documents.
#[component]
pub fn VignetteLinker(props: VignetteLinkerProps) -> Element {
    let i18n = use_i18n();

    if props.vignettes.is_empty() {
        return rsx! {
            div { class: "vg-empty", {i18n.t("media.no_vignettes")} }
        };
    }

    rsx! {
        div { class: "vg-list",
            for vignette in props.vignettes.iter().cloned() {
                VignetteRow {
                    key: "{vignette.id}",
                    tree_id: props.tree_id,
                    vignette,
                    events: props.events.clone(),
                    on_changed: props.on_changed,
                }
            }
        }
    }
}

#[component]
fn VignetteRow(
    tree_id: Uuid,
    vignette: Vignette,
    events: Vec<(Uuid, String)>,
    on_changed: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();

    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut confirming = use_signal(|| false);

    let vignette_id = vignette.id;
    let image_url = api.vignette_image_url(tree_id, vignette_id);
    let selected_event = vignette
        .event_id
        .map(|id| id.to_string())
        .unwrap_or_default();

    let attach = {
        let api = api.clone();
        move |e: Event<FormData>| {
            let api = api.clone();
            let chosen = e.value();
            spawn(async move {
                busy.set(true);
                error.set(None);
                // An empty selection clears the attribution rather than
                // leaving it alone — `Some(None)` is the patch that says so.
                let body = UpdateVignetteBody {
                    event_id: Some(Uuid::parse_str(chosen.trim()).ok()),
                    ..Default::default()
                };
                match api.update_vignette(tree_id, vignette_id, &body).await {
                    Ok(_) => on_changed.call(()),
                    Err(err) => error.set(Some(err.to_string())),
                }
                busy.set(false);
            });
        }
    };

    let remove = {
        let api = api.clone();
        move |_| {
            let api = api.clone();
            spawn(async move {
                busy.set(true);
                error.set(None);
                match api.delete_vignette(tree_id, vignette_id).await {
                    Ok(()) => {
                        confirming.set(false);
                        on_changed.call(());
                    }
                    Err(err) => error.set(Some(err.to_string())),
                }
                busy.set(false);
            });
        }
    };

    rsx! {
        div { class: "vg-row",
            img { class: "vg-thumb", src: "{image_url}", alt: "" }
            div { class: "vg-body",
                div { class: "vg-name",
                    {i18n.t_args(
                        "media.crop_readout_short",
                        &[
                            ("width", &vignette.width.to_string()),
                            ("height", &vignette.height.to_string()),
                        ],
                    )}
                }
                if !events.is_empty() {
                    select {
                        class: "vg-select",
                        disabled: busy(),
                        value: "{selected_event}",
                        oninput: attach,
                        option {
                            value: "",
                            selected: selected_event.is_empty(),
                            {i18n.t("media.crop_no_event")}
                        }
                        for (id, label) in events.iter() {
                            option {
                                key: "{id}",
                                value: "{id}",
                                selected: selected_event == id.to_string(),
                                "{label}"
                            }
                        }
                    }
                }
                if let Some(err) = error() {
                    div { class: "error-msg", "{err}" }
                }
            }
            div { class: "vg-actions",
                if confirming() {
                    button {
                        class: "pf-row-btn is-danger",
                        r#type: "button",
                        disabled: busy(),
                        onclick: remove,
                        {i18n.t("common.confirm")}
                    }
                    button {
                        class: "pf-row-btn",
                        r#type: "button",
                        onclick: move |_| confirming.set(false),
                        {i18n.t("common.cancel")}
                    }
                } else {
                    button {
                        class: "pf-row-btn",
                        r#type: "button",
                        title: i18n.t("common.delete"),
                        onclick: move |_| confirming.set(true),
                        "\u{1F5D1}"
                    }
                }
            }
        }
    }
}
