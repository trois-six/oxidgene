//! The stage of the shared media viewer: a picture fitted to the space,
//! zoomed and dragged.
//!
//! One stage serves every picture the viewer shows — a stored page, a page
//! held as somebody else's address, an archive's view not yet attached — so a
//! reader zooms and drags a register the same way wherever it came from.
//! What sits over the picture (the regions identified on it, the half of a
//! double page a citation names) and what the stage shows instead of a
//! picture (a video, a fallback panel) belong to its caller.

use dioxus::html::geometry::WheelDelta;
use dioxus::prelude::*;

use crate::i18n::use_i18n;

/// Zoom bounds and step, as percentages of the fitted size.
///
/// The ceiling is high on purpose: the reason to zoom a parish register is to
/// read one word of secretary hand in a corner, and 200% does not get there.
pub(crate) const MIN_ZOOM: u32 = 25;
pub(crate) const MAX_ZOOM: u32 = 3200;

/// The fitted image is the zoom baseline.
const FIT_ZOOM: u32 = 100;

/// One step in, from the current level (`None` meaning "fit").
pub(crate) fn zoom_in(current: Option<u32>) -> u32 {
    match current {
        None => FIT_ZOOM * 6 / 5,
        Some(level) => (level.saturating_mul(6) / 5).min(MAX_ZOOM),
    }
}

/// One step out, from the current level (`None` meaning "fit").
pub(crate) fn zoom_out(current: Option<u32>) -> u32 {
    match current {
        None => FIT_ZOOM * 5 / 6,
        Some(level) => (level * 5 / 6).max(MIN_ZOOM),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum ZoomAnchor {
    Center,
    Pointer(f64, f64),
}

/// The picture a stage draws.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StagePicture {
    pub url: String,
    pub alt: String,
}

/// The zoom of one stage: its level, the fitted size it is relative to, and
/// on which axes the zoomed picture overflows the stage.
#[derive(Clone, Copy)]
struct StageZoom {
    /// A percentage of the fitted size; `None` is exactly fitted. This
    /// mirrors the pedigree's multiplicative zoom without sacrificing the
    /// scrollbars a large scan needs.
    level: Signal<Option<u32>>,
    fitted: Signal<Option<(f64, f64)>>,
    overflow: Signal<(bool, bool)>,
    /// Whether a wheel step is being applied, so that steps do not pile up.
    wheeling: Signal<bool>,
}

/// The stage's inner size and the picture's natural one, measured in the
/// page, as `[natural width, natural height, space width, space height]`.
const MEASURE_SCRIPT: &str = r#"
    const image = document.getElementById(IMAGE_ID);
    const stage = document.getElementById(STAGE_ID);
    if (!image || !stage) return null;
    for (let frame = 0; frame < 8 && (!image.complete || !image.naturalWidth); frame += 1) {
        await new Promise(requestAnimationFrame);
    }
    if (!image.naturalWidth || !image.naturalHeight) return null;
    const style = getComputedStyle(stage);
    return [
        image.naturalWidth,
        image.naturalHeight,
        stage.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight),
        stage.clientHeight - parseFloat(style.paddingTop) - parseFloat(style.paddingBottom),
    ];
"#;

/// Fits the picture to the stage and returns its fitted `[width, height]`.
const FIT_SCRIPT: &str = r#"
    const stage = document.getElementById(STAGE_ID);
    const image = stage?.querySelector('.media-viewer-image');
    if (!stage || !image || !image.naturalWidth || !image.naturalHeight) return null;
    const style = getComputedStyle(stage);
    const availableWidth = stage.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight);
    const availableHeight = stage.clientHeight - parseFloat(style.paddingTop) - parseFloat(style.paddingBottom);
    const scale = Math.min(availableWidth / image.naturalWidth, availableHeight / image.naturalHeight);
    const width = image.naturalWidth * scale;
    const height = image.naturalHeight * scale;
    image.style.width = `${width}px`;
    image.style.height = `${height}px`;
    image.style.maxWidth = 'none';
    image.style.maxHeight = 'none';
    stage.classList.remove('is-zoomed');
    stage.classList.remove('is-overflow-x', 'is-overflow-y');
    stage.scrollLeft = 0;
    stage.scrollTop = 0;
    return [width, height];
"#;

/// Sizes the picture to `WIDTH`×`HEIGHT` and scrolls in the same operation,
/// keeping the point under the pointer (or the centre) in place; returns on
/// which axes it overflows.
const ZOOM_SCRIPT: &str = r#"
    const stage = document.getElementById(STAGE_ID);
    const image = stage?.querySelector('.media-viewer-image');
    if (!stage || !image) return;
    const rect = stage.getBoundingClientRect();
    const oldImageRect = image.getBoundingClientRect();
    const clientX = POINTER_X ?? oldImageRect.left + oldImageRect.width / 2;
    const clientY = POINTER_Y ?? oldImageRect.top + oldImageRect.height / 2;
    const screenX = clientX - rect.left;
    const screenY = clientY - rect.top;
    const nx = Math.max(0, Math.min(1, (clientX - oldImageRect.left) / oldImageRect.width));
    const ny = Math.max(0, Math.min(1, (clientY - oldImageRect.top) / oldImageRect.height));
    image.style.width = WIDTH + 'px';
    image.style.height = HEIGHT + 'px';
    image.style.maxWidth = 'none';
    image.style.maxHeight = 'none';
    stage.classList.toggle('is-zoomed', IS_ZOOMED);
    const style = getComputedStyle(stage);
    const availableWidth = stage.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight);
    const availableHeight = stage.clientHeight - parseFloat(style.paddingTop) - parseFloat(style.paddingBottom);
    const overflowX = WIDTH > availableWidth;
    const overflowY = HEIGHT > availableHeight;
    stage.classList.toggle('is-overflow-x', overflowX);
    stage.classList.toggle('is-overflow-y', overflowY);
    if (CENTER) {
        stage.scrollLeft = Math.max(0, (stage.scrollWidth - stage.clientWidth) / 2);
        stage.scrollTop = Math.max(0, (stage.scrollHeight - stage.clientHeight) / 2);
    } else {
        const newImageRect = image.getBoundingClientRect();
        const imageLeft = newImageRect.left - rect.left + stage.scrollLeft;
        const imageTop = newImageRect.top - rect.top + stage.scrollTop;
        stage.scrollLeft = imageLeft + nx * newImageRect.width - screenX;
        stage.scrollTop = imageTop + ny * newImageRect.height - screenY;
    }
    return [overflowX, overflowY];
"#;

/// A script with its element identifiers bound, as JavaScript strings.
fn bound(script: &str, stage_id: &str, image_id: &str) -> String {
    let quoted = |id: &str| serde_json::to_string(id).unwrap_or_default();
    script
        .replace("STAGE_ID", &quoted(stage_id))
        .replace("IMAGE_ID", &quoted(image_id))
}

/// The numbers at `index` of a script's answer.
fn number_at(value: &serde_json::Value, index: usize) -> Option<f64> {
    value.get(index).and_then(serde_json::Value::as_f64)
}

/// The fitted size of a picture of `natural` size in a stage of `space`.
fn fitted_in(
    (width, height): (f64, f64),
    (space_width, space_height): (f64, f64),
) -> Option<(f64, f64)> {
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let scale = (space_width / width).min(space_height / height);
    Some((width * scale, height * scale))
}

/// Whether two fitted sizes differ by more than half a pixel.
fn moved(old: Option<(f64, f64)>, new: (f64, f64)) -> bool {
    old.is_none_or(|(width, height)| (width - new.0).abs() > 0.5 || (height - new.1).abs() > 0.5)
}

/// The picture's inline size at a zoom level over a fitted size.
fn image_style(level: Option<u32>, fitted: Option<(f64, f64)>) -> String {
    let scaled = |(width, height): (f64, f64), level: u32| {
        let factor = f64::from(level) / f64::from(FIT_ZOOM);
        (width * factor, height * factor)
    };
    let size = match (level, fitted) {
        (Some(level), Some(fitted)) => Some(scaled(fitted, level)),
        (None, fitted) => fitted,
        (Some(_), None) => {
            return "width: auto; max-width: none; max-height: none;".to_string();
        }
    };
    size.map(|(width, height)| {
        format!("width: {width}px; height: {height}px; max-width: none; max-height: none;")
    })
    .unwrap_or_default()
}

/// The stage's classes: zoomed, overflowing on an axis, being dragged.
fn stage_class(
    level: Option<u32>,
    (overflow_x, overflow_y): (bool, bool),
    dragging: bool,
) -> String {
    let flags = [
        (level.is_some_and(|level| level > FIT_ZOOM), " is-zoomed"),
        (overflow_x, " is-overflow-x"),
        (overflow_y, " is-overflow-y"),
        (dragging, " is-dragging"),
    ];
    flags.iter().filter(|(on, _)| *on).fold(
        "media-viewer-stage is-image".to_string(),
        |class, (_, flag)| class + flag,
    )
}

/// How far a wheel turned, in its own unit: positive away from the reader.
fn wheel_delta(delta: WheelDelta) -> f64 {
    match delta {
        WheelDelta::Lines(lines) => lines.y,
        WheelDelta::Pixels(pixels) => pixels.y,
        WheelDelta::Pages(pages) => pages.y,
    }
}

/// Fits the picture to the stage, the zoom baseline.
fn use_fit(zoom: StageZoom, stage_id: String) -> Callback<()> {
    let StageZoom {
        mut level,
        mut fitted,
        mut overflow,
        ..
    } = zoom;
    use_callback(move |()| {
        let script = bound(FIT_SCRIPT, &stage_id, "");
        spawn(async move {
            let Ok(value) = document::eval(&script).await else {
                return;
            };
            if let (Some(width), Some(height)) = (number_at(&value, 0), number_at(&value, 1)) {
                fitted.set(Some((width, height)));
                level.set(None);
                overflow.set((false, false));
            }
        });
    })
}

/// Zooms to a level around an anchor. Size and scroll move in one WebView
/// operation, then Rust adopts that already-visible state, which prevents an
/// intermediate displaced frame.
fn use_apply_zoom(zoom: StageZoom, stage_id: String) -> Callback<(u32, ZoomAnchor)> {
    let StageZoom {
        mut level,
        fitted,
        mut overflow,
        mut wheeling,
    } = zoom;
    use_callback(move |(target, anchor): (u32, ZoomAnchor)| {
        let by_wheel = matches!(anchor, ZoomAnchor::Pointer(_, _));
        if by_wheel && wheeling() {
            return;
        }
        let Some((fit_width, fit_height)) = fitted() else {
            return;
        };
        wheeling.set(by_wheel);
        let factor = f64::from(target) / f64::from(FIT_ZOOM);
        let (pointer_x, pointer_y, center) = match anchor {
            ZoomAnchor::Center => ("null".to_string(), "null".to_string(), true),
            ZoomAnchor::Pointer(x, y) => (x.to_string(), y.to_string(), false),
        };
        let script = bound(ZOOM_SCRIPT, &stage_id, "")
            .replace("POINTER_X", &pointer_x)
            .replace("POINTER_Y", &pointer_y)
            .replace("WIDTH", &(fit_width * factor).to_string())
            .replace("HEIGHT", &(fit_height * factor).to_string())
            .replace("IS_ZOOMED", &(target > FIT_ZOOM).to_string())
            .replace("CENTER", &center.to_string());
        spawn(async move {
            if let Ok(value) = document::eval(&script).await {
                let axis = |index: usize| value.get(index).and_then(serde_json::Value::as_bool);
                overflow.set((axis(0).unwrap_or(false), axis(1).unwrap_or(false)));
            }
            level.set(Some(target));
            wheeling.set(false);
        });
    })
}

/// Measures the fitted size once the picture has decoded, and again whenever
/// the stage renders, so a resized window refits it.
fn use_measure(zoom: StageZoom, stage_id: String, image_id: String) {
    let mut fitted = zoom.fitted;
    use_effect(move || {
        let script = bound(MEASURE_SCRIPT, &stage_id, &image_id);
        spawn(async move {
            let Ok(value) = document::eval(&script).await else {
                return;
            };
            let natural = number_at(&value, 0).zip(number_at(&value, 1));
            let space = number_at(&value, 2).zip(number_at(&value, 3));
            let Some(new) = natural
                .zip(space)
                .and_then(|(natural, space)| fitted_in(natural, space))
            else {
                return;
            };
            if moved(fitted(), new) {
                fitted.set(Some(new));
            }
        });
    });
}

/// The viewer's stage: the zoom controls, then the picture fitted to the
/// space, zoomed by the wheel and the controls and dragged when it overflows.
///
/// `stage_key` distinguishes the stage's elements in the page; a caller
/// showing another picture gives the component another `key`, which starts
/// it fitted again. `children` sit between the controls and the stage,
/// `overlays` over the picture in its own frame — positioned in percentages
/// of it — and `controls` after the zoom buttons. Without a picture the
/// stage shows `fallback`, and no controls.
#[component]
pub(crate) fn MediaStage(
    stage_key: String,
    picture: Option<StagePicture>,
    #[props(default = VNode::empty())] overlays: Element,
    #[props(default = VNode::empty())] controls: Element,
    #[props(default = VNode::empty())] fallback: Element,
    #[props(default)] children: Element,
    on_picture_error: EventHandler<()>,
) -> Element {
    let zoom = StageZoom {
        level: use_signal(|| None),
        fitted: use_signal(|| None),
        overflow: use_signal(|| (false, false)),
        wheeling: use_signal(|| false),
    };
    let mut dragging = use_signal(|| false);
    let mut drag_from = use_signal(|| (0.0_f64, 0.0_f64));
    let stage_id = format!("media-viewer-stage-{stage_key}");
    let image_id = format!("media-viewer-image-{stage_key}");
    let fit = use_fit(zoom, stage_id.clone());
    let apply_zoom = use_apply_zoom(zoom, stage_id.clone());
    use_measure(zoom, stage_id.clone(), image_id.clone());
    let is_picture = picture.is_some();
    let stage_id_for_move = stage_id.clone();

    rsx! {
        if is_picture {
            ZoomControls { level: (zoom.level)(), fitted: (zoom.fitted)().is_some(), fit, apply_zoom, {controls} }
        }
        {children}
        div {
            id: "{stage_id}",
            class: stage_class((zoom.level)(), (zoom.overflow)(), dragging()),
            onpointermove: move |event| {
                if !dragging() {
                    return;
                }
                let point = event.client_coordinates();
                let (from_x, from_y) = drag_from();
                drag_from.set((point.x, point.y));
                let stage_id = serde_json::to_string(&stage_id_for_move).unwrap_or_default();
                let (delta_x, delta_y) = (point.x - from_x, point.y - from_y);
                spawn(async move {
                    let script = format!(
                        "const stage = document.getElementById({stage_id}); if (stage) {{ stage.scrollLeft -= {delta_x}; stage.scrollTop -= {delta_y}; }}"
                    );
                    let _ = document::eval(&script).await;
                });
            },
            onpointerdown: move |event| {
                if is_picture {
                    event.prevent_default();
                    let point = event.client_coordinates();
                    drag_from.set((point.x, point.y));
                    dragging.set(true);
                }
            },
            ondragstart: move |event| event.prevent_default(),
            onpointerup: move |_| dragging.set(false),
            onpointerleave: move |_| dragging.set(false),
            onwheel: move |event| {
                event.prevent_default();
                let point = event.client_coordinates();
                let current = (zoom.level)();
                let next = if wheel_delta(event.delta()) > 0.0 { zoom_out(current) } else { zoom_in(current) };
                apply_zoom.call((next, ZoomAnchor::Pointer(point.x, point.y)));
            },
            match picture {
                Some(picture) => rsx! {
                    div { class: "media-viewer-image-frame",
                        img {
                            id: "{image_id}",
                            class: "media-viewer-image media-viewer-static-image",
                            src: "{picture.url}",
                            alt: "{picture.alt}",
                            draggable: "false",
                            style: image_style((zoom.level)(), (zoom.fitted)()),
                            onload: move |_| fit.call(()),
                            onerror: move |_| on_picture_error.call(()),
                        }
                        {overlays}
                    }
                },
                None => fallback,
            }
        }
    }
}

/// Zoom belongs to pictures alone: a video and an audio track have their own
/// controls, and a fallback has nothing to magnify. These use the tree
/// sidebar's visual language so they remain compact beside a large scan;
/// `children` follow them.
#[component]
fn ZoomControls(
    level: Option<u32>,
    fitted: bool,
    fit: Callback<()>,
    apply_zoom: Callback<(u32, ZoomAnchor)>,
    children: Element,
) -> Element {
    let i18n = use_i18n();
    rsx! {
        div { class: "media-viewer-controls",
            button {
                class: "isb-btn",
                r#type: "button",
                title: i18n.t("media.zoom_in"),
                disabled: !fitted || level.is_some_and(|z| z >= MAX_ZOOM),
                onclick: move |_| apply_zoom.call((zoom_in(level), ZoomAnchor::Center)),
                svg {
                    width: "16", height: "16", fill: "none", "viewBox": "0 0 24 24",
                    stroke: "currentColor", "strokeWidth": "2",
                    circle { cx: "11", cy: "11", r: "8" }
                    line { x1: "21", y1: "21", x2: "16.65", y2: "16.65" }
                    line { x1: "11", y1: "8", x2: "11", y2: "14" }
                    line { x1: "8", y1: "11", x2: "14", y2: "11" }
                }
            }
            button {
                class: "isb-btn",
                r#type: "button",
                title: i18n.t("media.zoom_fit"),
                onclick: move |_| fit.call(()),
                svg {
                    width: "16", height: "16", fill: "none", "viewBox": "0 0 24 24",
                    stroke: "currentColor", "strokeWidth": "2",
                    path { d: "M3 8V5a2 2 0 0 1 2-2h3" }
                    path { d: "M16 3h3a2 2 0 0 1 2 2v3" }
                    path { d: "M21 16v3a2 2 0 0 1-2 2h-3" }
                    path { d: "M8 21H5a2 2 0 0 1-2-2v-3" }
                }
            }
            button {
                class: "isb-btn",
                r#type: "button",
                title: i18n.t("media.zoom_out"),
                disabled: !fitted || level.is_some_and(|z| z <= MIN_ZOOM),
                onclick: move |_| apply_zoom.call((zoom_out(level), ZoomAnchor::Center)),
                svg {
                    width: "16", height: "16", fill: "none", "viewBox": "0 0 24 24",
                    stroke: "currentColor", "strokeWidth": "2",
                    circle { cx: "11", cy: "11", r: "8" }
                    line { x1: "21", y1: "21", x2: "16.65", y2: "16.65" }
                    line { x1: "8", y1: "11", x2: "14", y2: "11" }
                }
            }
            {children}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_fits_its_stage_and_zooms_from_there() {
        assert_eq!(
            fitted_in((2000.0, 1000.0), (1000.0, 1000.0)),
            Some((1000.0, 500.0))
        );
        assert_eq!(fitted_in((0.0, 1000.0), (1000.0, 1000.0)), None);
        assert!(moved(None, (1.0, 1.0)));
        assert!(!moved(Some((1.0, 1.0)), (1.2, 1.0)));
        assert_eq!(
            image_style(Some(200), Some((100.0, 50.0))),
            "width: 200px; height: 100px; max-width: none; max-height: none;"
        );
        assert_eq!(image_style(None, None), "");
        assert_eq!(
            stage_class(Some(150), (true, false), true),
            "media-viewer-stage is-image is-zoomed is-overflow-x is-dragging"
        );
        assert_eq!(
            stage_class(None, (false, false), false),
            "media-viewer-stage is-image"
        );
    }

    #[test]
    fn a_script_names_its_elements_as_strings() {
        assert_eq!(
            bound(
                "getElementById(STAGE_ID) getElementById(IMAGE_ID)",
                "stage-1",
                "image-1"
            ),
            r#"getElementById("stage-1") getElementById("image-1")"#
        );
    }

    #[test]
    fn zooming_in_and_out_stays_within_its_bounds() {
        // Match the pedigree's 1.2 factor around the fitted size.
        assert_eq!(zoom_in(None), 120);
        assert_eq!(zoom_out(None), 83);
        assert_eq!(zoom_in(Some(120)), 144);
        assert_eq!(zoom_out(Some(144)), 120);
        assert_eq!(zoom_in(Some(MAX_ZOOM)), MAX_ZOOM);
        assert_eq!(zoom_out(Some(MIN_ZOOM)), MIN_ZOOM);
        // No overflow at the ceiling, whatever it is set to.
        assert_eq!(zoom_in(Some(u32::MAX)), MAX_ZOOM);
    }

    #[test]
    fn zooming_reaches_far_enough_to_read_a_corner_of_a_scan() {
        // The reason to zoom a register is one word of secretary hand.
        let mut level = zoom_in(None);
        for _ in 0..40 {
            level = zoom_in(Some(level));
        }
        assert_eq!(level, MAX_ZOOM);
        const { assert!(MAX_ZOOM >= 400) };
    }
}
