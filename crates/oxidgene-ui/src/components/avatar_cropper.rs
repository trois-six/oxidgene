//! Choosing the square of a picture that becomes a user's avatar
//! (docs/authentication.md §6.7, §10).
//!
//! The vignette cropper ([`super::image_cropper`]) draws a free rectangle
//! on a tree's stored media; an avatar is a square, chosen before anything
//! is stored, from a file that never was a media. So this is a selector of
//! its own: the picture fitted into a square stage, a square over it,
//! centred by default, that the reader drags, moves with the arrow keys and
//! resizes with the zoom. The square is sent in pixels of the upright
//! picture, which the browser draws upright from its EXIF orientation just
//! as the server reads it.

use dioxus::prelude::*;

use crate::api::account::AvatarCrop;
use crate::components::modal::Modal;
use crate::i18n::use_i18n;

/// The side of the stage the picture is fitted into, in CSS pixels.
const STAGE: f64 = 320.0;

/// How far the zoom goes: the square shrinks to a quarter of the picture's
/// shorter side.
const MAX_ZOOM: f64 = 4.0;

/// How far an arrow key moves the square, in CSS pixels of the stage.
const KEY_STEP: f64 = 8.0;

/// The square of a `natural`-sized picture at `zoom` (1 is the shorter
/// side), centred on `center` — the picture's middle when `None` — and kept
/// inside the picture.
pub fn crop_square(natural: (u32, u32), zoom: f64, center: Option<(f64, f64)>) -> AvatarCrop {
    let (width, height) = natural;
    let shorter = width.min(height).max(1);
    let zoom = zoom.clamp(1.0, MAX_ZOOM);
    let size = ((f64::from(shorter) / zoom).round() as u32).clamp(1, shorter);
    let (cx, cy) = center.unwrap_or((f64::from(width) / 2.0, f64::from(height) / 2.0));
    let half = f64::from(size) / 2.0;
    let place = |c: f64, extent: u32| {
        let room = extent.saturating_sub(size);
        ((c - half).round().max(0.0) as u32).min(room)
    };
    AvatarCrop {
        x: place(cx, width),
        y: place(cy, height),
        size,
    }
}

/// The centre of `crop`, in pixels of the picture.
fn centre_of(crop: AvatarCrop) -> (f64, f64) {
    let half = f64::from(crop.size) / 2.0;
    (f64::from(crop.x) + half, f64::from(crop.y) + half)
}

/// The natural size of the picture `id` names, once it has decoded.
async fn natural_size(id: &str) -> Option<(u32, u32)> {
    let script = format!(
        "const image = document.getElementById('{id}');\n\
         return image ? [image.naturalWidth, image.naturalHeight] : null;"
    );
    let value = document::eval(&script).await.ok()?;
    let side = |index: usize| value.get(index).and_then(serde_json::Value::as_u64);
    let (width, height) = (side(0)?, side(1)?);
    (width > 0 && height > 0).then_some((width as u32, height as u32))
}

/// The dialog choosing the avatar's square in `image`, a `data:` URL.
#[component]
pub fn AvatarCropper(
    image: String,
    /// Whether the upload is under way.
    #[props(default)]
    busy: bool,
    #[props(default)] error: Option<String>,
    on_confirm: EventHandler<AvatarCrop>,
    on_cancel: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let image_id = crate::listeners::use_element_id("avatar-crop-image");
    let mut natural = use_signal(|| None::<(u32, u32)>);
    let mut zoom = use_signal(|| 1.0_f64);
    let mut center = use_signal(|| None::<(f64, f64)>);
    // Where a drag started: the pointer, in client pixels, and the centre.
    let mut drag = use_signal(|| None::<((f64, f64), (f64, f64))>);

    let crop = natural().map(|size| crop_square(size, zoom(), center()));
    let scale = natural().map_or(0.0, |(width, height)| STAGE / f64::from(width.max(height)));
    let (stage_width, stage_height) = natural().map_or((STAGE, STAGE), |(width, height)| {
        (f64::from(width) * scale, f64::from(height) * scale)
    });

    // Moves are listened to while the square is dragged only (see
    // `crate::listeners`).
    let dragging = crate::listeners::while_active(drag().is_some(), || {
        vec![
            dioxus::html::events::onpointermove(move |event: Event<PointerData>| {
                let (Some(((x0, y0), (cx, cy))), Some(size)) = (*drag.peek(), *natural.peek())
                else {
                    return;
                };
                if scale <= 0.0 {
                    return;
                }
                let point = event.client_coordinates();
                let moved = (cx + (point.x - x0) / scale, cy + (point.y - y0) / scale);
                let kept = crop_square(size, *zoom.peek(), Some(moved));
                center.set(Some(centre_of(kept)));
            }),
            dioxus::html::events::onpointerup(move |_| drag.set(None)),
            dioxus::html::events::onpointerleave(move |_| drag.set(None)),
        ]
    });

    let mut nudge = move |dx: f64, dy: f64| {
        let (Some(size), Some(crop)) = (natural(), crop) else {
            return;
        };
        if scale <= 0.0 {
            return;
        }
        let (cx, cy) = centre_of(crop);
        let kept = crop_square(size, zoom(), Some((cx + dx / scale, cy + dy / scale)));
        center.set(Some(centre_of(kept)));
    };

    rsx! {
        Modal {
            class: "modal-card avatar-cropper",
            label: i18n.t("profile.avatar_crop_title"),
            busy,
            on_close: move |()| on_cancel.call(()),
            h3 { {i18n.t("profile.avatar_crop_title")} }
            p { class: "avatar-crop-hint", {i18n.t("profile.avatar_crop_hint")} }
            div {
                class: "gesture-listeners",
                ..dragging,
                div {
                    class: "avatar-crop-stage",
                    style: "width: {stage_width}px; height: {stage_height}px;",
                    img {
                        id: "{image_id}",
                        class: "avatar-crop-image",
                        src: "{image}",
                        alt: "",
                        draggable: "false",
                        onload: {
                            let image_id = image_id.clone();
                            move |_| {
                                let image_id = image_id.clone();
                                spawn(async move {
                                    natural.set(natural_size(&image_id).await);
                                });
                            }
                        },
                    }
                    if let Some(crop) = crop {
                        div {
                            class: "avatar-crop-square",
                            tabindex: "0",
                            role: "slider",
                            "aria-label": i18n.t("profile.avatar_crop_square"),
                            "aria-valuetext": "{crop.x}, {crop.y}",
                            style: "left: {f64::from(crop.x) * scale}px; top: {f64::from(crop.y) * scale}px; \
                                    width: {f64::from(crop.size) * scale}px; height: {f64::from(crop.size) * scale}px;",
                            onpointerdown: move |event: Event<PointerData>| {
                                event.prevent_default();
                                let point = event.client_coordinates();
                                drag.set(Some(((point.x, point.y), centre_of(crop))));
                            },
                            onkeydown: move |event: Event<KeyboardData>| {
                                let step = match event.key() {
                                    Key::ArrowLeft => (-KEY_STEP, 0.0),
                                    Key::ArrowRight => (KEY_STEP, 0.0),
                                    Key::ArrowUp => (0.0, -KEY_STEP),
                                    Key::ArrowDown => (0.0, KEY_STEP),
                                    _ => return,
                                };
                                event.prevent_default();
                                nudge(step.0, step.1);
                            },
                        }
                    }
                }
            }
            label { class: "avatar-crop-zoom",
                span { {i18n.t("profile.avatar_zoom")} }
                input {
                    r#type: "range",
                    min: "1",
                    max: "{MAX_ZOOM}",
                    step: "0.01",
                    value: "{zoom}",
                    disabled: natural().is_none(),
                    oninput: move |event: Event<FormData>| {
                        if let Ok(value) = event.value().parse::<f64>() {
                            zoom.set(value);
                        }
                    },
                }
            }
            if let Some(error) = error {
                div { class: "error-msg", role: "alert", "{error}" }
            }
            div { class: "modal-actions",
                button {
                    class: "btn btn-outline",
                    r#type: "button",
                    disabled: busy,
                    onclick: move |_| on_cancel.call(()),
                    {i18n.t("common.cancel")}
                }
                button {
                    class: "btn btn-primary",
                    r#type: "button",
                    disabled: busy || crop.is_none(),
                    onclick: move |_| {
                        if let Some(crop) = crop {
                            on_confirm.call(crop);
                        }
                    },
                    if busy {
                        span { class: "btn-spinner" }
                    }
                    {i18n.t("profile.avatar_use")}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_square_starts_centred_on_the_shorter_side() {
        assert_eq!(
            crop_square((800, 600), 1.0, None),
            AvatarCrop {
                x: 100,
                y: 0,
                size: 600
            }
        );
        assert_eq!(
            crop_square((300, 900), 1.0, None),
            AvatarCrop {
                x: 0,
                y: 300,
                size: 300
            }
        );
    }

    #[test]
    fn zooming_shrinks_the_square_around_its_centre() {
        assert_eq!(
            crop_square((800, 600), 2.0, None),
            AvatarCrop {
                x: 250,
                y: 150,
                size: 300
            }
        );
        // Beyond the range, the zoom is held to it.
        assert_eq!(crop_square((800, 600), 10.0, None).size, 150);
        assert_eq!(crop_square((800, 600), 0.5, None).size, 600);
    }

    #[test]
    fn the_square_stays_inside_the_picture() {
        let corner = crop_square((800, 600), 2.0, Some((0.0, 0.0)));
        assert_eq!((corner.x, corner.y), (0, 0));
        let far = crop_square((800, 600), 2.0, Some((10_000.0, 10_000.0)));
        assert_eq!((far.x, far.y), (500, 300));
        assert_eq!(centre_of(far), (650.0, 450.0));
    }

    #[test]
    fn a_tiny_picture_still_has_a_square() {
        assert_eq!(
            crop_square((1, 1), 4.0, None),
            AvatarCrop {
                x: 0,
                y: 0,
                size: 1
            }
        );
    }
}
