//! Draw a rectangle on a scan and keep it as a vignette.
//!
//! # Two coordinate systems, one conversion
//!
//! The user drags in whatever pixels the image happens to occupy on screen; a
//! vignette is stored in the source image's own pixels, because that is the
//! only frame that survives a window resize, a zoom, or the modal being opened
//! on a phone. Everything here is about keeping those two apart and converting
//! once, at the boundary.
//!
//! The source dimensions come from the `media` row — recorded at upload
//! precisely so the frontend never has to decode an image to find out how big
//! it is — and the displayed dimensions come from the element's own client
//! rect, measured on mount and again whenever the box changes. Their ratio is
//! the only thing the conversion needs.
//!
//! # Existing crops are shown, not hidden
//!
//! A register page with four entries already cropped shows those four
//! rectangles while you draw the fifth. Without them the user has no way to
//! see what has been covered, and the usual outcome is the same entry cropped
//! twice.

use dioxus::prelude::*;
use oxidgene_core::types::{Media, Vignette};
use uuid::Uuid;

use crate::api::{ApiClient, CreateVignetteBody};
use crate::i18n::use_i18n;

/// A rectangle in the source image's pixel space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CropRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// The smallest crop worth saving, in source pixels.
///
/// A click that moves three pixels is a click, not a drag. Without a floor,
/// every stray click on the image would try to save a 1×1 vignette and be
/// rejected by the server, which reads as the app throwing errors at you for
/// touching it.
const MIN_CROP_EDGE: i32 = 16;

#[derive(Props, Clone, PartialEq)]
pub struct ImageCropperProps {
    pub tree_id: Uuid,
    /// The image being cropped. Its `width`/`height` are what the drag is
    /// converted into, so a media without them cannot be cropped.
    pub media: Media,
    /// Crops already recorded on this media, drawn as an overlay.
    #[props(default)]
    pub existing: Vec<Vignette>,
    /// Pre-fill the new vignette's attribution with this person.
    #[props(default)]
    pub person_id: Option<Uuid>,
    /// Events the crop may be attached to, as (id, label) pairs.
    #[props(default)]
    pub events: Vec<(Uuid, String)>,
    /// Called with the saved vignette once it is stored.
    pub on_saved: EventHandler<Vignette>,
    pub on_close: EventHandler<()>,
}

/// Interactive crop overlay: drag a rectangle, name it, save it as a vignette.
#[component]
pub fn ImageCropper(props: ImageCropperProps) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();

    // Where the image actually sits on screen, in CSS pixels. Zero until the
    // element mounts; the drag maths is skipped while it is.
    let mut displayed = use_signal(|| (0.0_f64, 0.0_f64));
    let mut drag_start = use_signal(|| None::<(f64, f64)>);
    // The live rectangle, in *displayed* pixels — converted only on save, so
    // dragging never accumulates rounding error.
    let mut drag_rect = use_signal(|| None::<(f64, f64, f64, f64)>);
    let mut event_id = use_signal(String::new);
    let mut saving = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    let tree_id = props.tree_id;
    let media = props.media.clone();
    let media_id = media.id;
    let source = (media.width.unwrap_or(0), media.height.unwrap_or(0));
    let image_url = api.media_file_url(tree_id, media_id);
    let on_saved = props.on_saved;
    let on_close = props.on_close;
    let person_id = props.person_id;
    let events = props.events.clone();

    // A media whose dimensions were never recorded — a PDF, or a row imported
    // from GEDCOM that has no bytes — cannot be cropped, and saying so beats
    // showing a canvas where dragging does nothing.
    if source.0 <= 0 || source.1 <= 0 {
        return rsx! {
            div { class: "cropper-backdrop", onclick: move |_| on_close.call(()),
                div { class: "cropper-panel", onclick: move |e| e.stop_propagation(),
                    div { class: "cropper-empty", {i18n.t("media.cannot_crop")} }
                    button {
                        class: "btn btn-outline",
                        r#type: "button",
                        onclick: move |_| on_close.call(()),
                        {i18n.t("common.close")}
                    }
                }
            }
        };
    }

    let scale = move || {
        let (w, _) = displayed();
        if w > 0.0 { w / source.0 as f64 } else { 0.0 }
    };

    // The drag, converted into source pixels and clamped to the image.
    let to_source = move || -> Option<CropRect> {
        let (x, y, w, h) = drag_rect()?;
        let s = scale();
        if s <= 0.0 {
            return None;
        }
        let rect = CropRect {
            x: ((x / s).round() as i32).clamp(0, source.0),
            y: ((y / s).round() as i32).clamp(0, source.1),
            width: (w / s).round() as i32,
            height: (h / s).round() as i32,
        };
        // Clamp the extent too: a drag released past the edge of the image
        // would otherwise produce a rectangle the server has to reject.
        let rect = CropRect {
            width: rect.width.min(source.0 - rect.x),
            height: rect.height.min(source.1 - rect.y),
            ..rect
        };
        (rect.width >= MIN_CROP_EDGE && rect.height >= MIN_CROP_EDGE).then_some(rect)
    };

    let pending = to_source();

    let save = move |_| {
        let Some(rect) = to_source() else { return };
        let api = api.clone();
        let event = Uuid::parse_str(event_id().trim()).ok();
        spawn(async move {
            saving.set(true);
            error.set(None);
            let body = CreateVignetteBody {
                page: None,
                x: rect.x,
                y: rect.y,
                width: rect.width,
                height: rect.height,
                person_id,
                event_id: event,
            };
            match api.create_vignette(tree_id, media_id, &body).await {
                Ok(vignette) => {
                    // Clear the draft, keep the cropper open: a page with four
                    // entries is four crops in a row, and closing after each
                    // would mean reopening the same scan four times.
                    drag_rect.set(None);
                    event_id.set(String::new());
                    on_saved.call(vignette);
                }
                Err(e) => error.set(Some(e.to_string())),
            }
            saving.set(false);
        });
    };

    let s = scale();

    rsx! {
        div { class: "cropper-backdrop", onclick: move |_| on_close.call(()),
            div { class: "cropper-panel", onclick: move |e| e.stop_propagation(),
                div { class: "cropper-head",
                    span { class: "cropper-title", "{media.file_name}" }
                    button {
                        class: "cropper-close",
                        r#type: "button",
                        onclick: move |_| on_close.call(()),
                        "\u{00D7}"
                    }
                }

                div {
                    class: "cropper-stage",
                    // Measured rather than assumed: the image is laid out by
                    // CSS (`max-height`, aspect ratio), so only the engine
                    // knows what size it ended up.
                    onmounted: move |e| async move {
                        if let Ok(rect) = e.get_client_rect().await {
                            displayed.set((rect.size.width, rect.size.height));
                        }
                    },
                    onresize: move |e| {
                        let size = e.get_content_box_size().unwrap_or_default();
                        if size.width > 0.0 {
                            displayed.set((size.width, size.height));
                        }
                    },
                    onmousedown: move |e: Event<MouseData>| {
                        let p = e.element_coordinates();
                        drag_start.set(Some((p.x, p.y)));
                        drag_rect.set(None);
                    },
                    onmousemove: move |e: Event<MouseData>| {
                        let Some((sx, sy)) = drag_start() else { return };
                        let p = e.element_coordinates();
                        // Normalised so dragging up-left works the same as
                        // down-right; a negative width is not a rectangle.
                        drag_rect.set(Some((
                            sx.min(p.x),
                            sy.min(p.y),
                            (p.x - sx).abs(),
                            (p.y - sy).abs(),
                        )));
                    },
                    onmouseup: move |_| drag_start.set(None),
                    onmouseleave: move |_| drag_start.set(None),

                    img {
                        class: "cropper-image",
                        src: "{image_url}",
                        alt: "{media.file_name}",
                        // The browser would otherwise start its own drag of the
                        // image, which cancels ours halfway through.
                        draggable: "false",
                    }

                    // Crops already on this page, so the user can see what is
                    // covered while drawing the next one.
                    if s > 0.0 {
                        for existing in props.existing.iter() {
                            div {
                                key: "{existing.id}",
                                class: "cropper-existing",
                                style: "left:{existing.x as f64 * s}px;top:{existing.y as f64 * s}px;\
                                        width:{existing.width as f64 * s}px;height:{existing.height as f64 * s}px",
                            }
                        }
                    }

                    if let Some((x, y, w, h)) = drag_rect() {
                        div {
                            class: "cropper-selection",
                            style: "left:{x}px;top:{y}px;width:{w}px;height:{h}px",
                        }
                    }
                }

                div { class: "cropper-foot",
                    if let Some(rect) = pending {
                        div { class: "cropper-readout",
                            {i18n.t_args(
                                "media.crop_readout",
                                &[
                                    ("width", &rect.width.to_string()),
                                    ("height", &rect.height.to_string()),
                                    ("x", &rect.x.to_string()),
                                    ("y", &rect.y.to_string()),
                                ],
                            )}
                        }
                        div { class: "cropper-fields",
                            if !events.is_empty() {
                                div { class: "form-group",
                                    label { {i18n.t("media.crop_event")} }
                                    select {
                                        value: "{event_id}",
                                        oninput: move |e: Event<FormData>| event_id.set(e.value()),
                                        option { value: "", {i18n.t("media.crop_no_event")} }
                                        for (id, label) in events.iter() {
                                            option { key: "{id}", value: "{id}", "{label}" }
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        div { class: "cropper-hint", {i18n.t("media.crop_hint")} }
                    }

                    if let Some(err) = error() {
                        div { class: "error-msg", "{err}" }
                    }

                    div { class: "cropper-actions",
                        button {
                            class: "btn btn-outline",
                            r#type: "button",
                            onclick: move |_| on_close.call(()),
                            {i18n.t("common.close")}
                        }
                        button {
                            class: "btn btn-primary",
                            r#type: "button",
                            disabled: pending.is_none() || saving(),
                            onclick: save,
                            if saving() { {i18n.t("common.saving")} }
                            else { {i18n.t("media.save_crop")} }
                        }
                    }
                }
            }
        }
    }
}
