//! How the reader moves about a pedigree: the depth, zoom and fit tools of
//! the sidebar, dragging and wheel-zooming the canvas, and the saved view
//! every move updates.

use dioxus::html::geometry::WheelDelta;
use dioxus::prelude::*;
use uuid::Uuid;

use super::{
    EVENT_PANEL_AUTO_COLLAPSE_WIDTH, FitTarget, PedigreeView, PedigreeViewState, PedigreeZoomValue,
    VIEWPORT_DEFAULT_W, ViewStateCache, ViewportRect, ViewportTransform,
    WAIT_FOR_EVENT_PANEL_TRANSITION_JS, ZOOM_FACTOR, fit_graph_in_viewport, zoom_about, zoom_step,
};
use crate::i18n::use_i18n;

/// Where the chart saves its view, and what it saves: the pan and zoom of a
/// root and the depths drawn around it.
#[derive(Clone, Copy, PartialEq)]
pub(super) struct ViewSaver {
    pub cache: ViewStateCache,
    pub tree_id: Option<Uuid>,
    pub root: Uuid,
    pub transform: Signal<ViewportTransform>,
    pub ancestor_levels: Signal<usize>,
    pub descendant_levels: Signal<usize>,
}

impl ViewSaver {
    /// Persist a settled view without making the chart render subscribe to
    /// the rapidly changing pan and zoom signals.
    pub fn save(self) {
        let Some(tree_id) = self.tree_id else {
            return;
        };
        let transform = (self.transform)();
        self.cache.save(PedigreeViewState {
            tree_id,
            offset_x: transform.x,
            offset_y: transform.y,
            scale: transform.scale,
            ancestor_levels: (self.ancestor_levels)(),
            descendant_levels: (self.descendant_levels)(),
            selected_root: Some(self.root),
        });
    }

    /// Frames `target` once the DOM has drawn the viewport, saves the view,
    /// and lets transitions animate again.
    pub async fn fit(
        self,
        viewport_rect: Signal<ViewportRect>,
        target: FitTarget,
        mut animating: Signal<bool>,
    ) {
        crate::utils::sleep_ms(30).await;
        fit_graph_in_viewport(self.transform, viewport_rect, target).await;
        self.save();
        crate::utils::sleep_ms(20).await;
        animating.set(true);
    }
}

/// The sidebar's depth, zoom and fit tools.
#[component]
pub(super) fn PedigreeTools(
    view: PedigreeView,
    saver: ViewSaver,
    viewport_rect: Signal<ViewportRect>,
    max_zoom: f64,
    fit_target: FitTarget,
) -> Element {
    let i18n = use_i18n();
    let zoom = move |factor: f64| {
        let transform = saver.transform;
        // A button has no cursor to anchor to, so it holds the middle of the
        // viewport still.
        if let Some(new_scale) = zoom_step(transform().scale, factor, max_zoom) {
            zoom_about(transform, viewport_rect().center(), new_scale);
            saver.save();
        }
    };
    rsx! {
        DepthPopover {
            view,
            ancestor_levels: saver.ancestor_levels,
            descendant_levels: saver.descendant_levels,
        }
        div { class: "isb-hr" }
        button {
            class: "isb-btn",
            title: "{i18n.t(\"pedigree.zoom_in\")}",
            onclick: move |_| zoom(ZOOM_FACTOR),
            svg {
                width: "16",
                height: "16",
                fill: "none",
                "viewBox": "0 0 24 24",
                stroke: "currentColor",
                "strokeWidth": "2",
                circle { cx: "11", cy: "11", r: "8" }
                line { x1: "21", y1: "21", x2: "16.65", y2: "16.65" }
                line { x1: "11", y1: "8", x2: "11", y2: "14" }
                line { x1: "8", y1: "11", x2: "14", y2: "11" }
            }
        }
        button {
            class: "isb-btn",
            title: "{i18n.t(\"pedigree.zoom_out\")}",
            onclick: move |_| zoom(1.0 / ZOOM_FACTOR),
            svg {
                width: "16",
                height: "16",
                fill: "none",
                "viewBox": "0 0 24 24",
                stroke: "currentColor",
                "strokeWidth": "2",
                circle { cx: "11", cy: "11", r: "8" }
                line { x1: "21", y1: "21", x2: "16.65", y2: "16.65" }
                line { x1: "8", y1: "11", x2: "14", y2: "11" }
            }
        }
        PedigreeZoomValue { transform: saver.transform }
        button {
            class: "isb-btn",
            title: "{i18n.t(\"pedigree.fit_screen\")}",
            onclick: move |_| {
                spawn(async move {
                    fit_graph_in_viewport(saver.transform, viewport_rect, fit_target).await;
                    saver.save();
                });
            },
            svg {
                width: "16",
                height: "16",
                fill: "none",
                "viewBox": "0 0 24 24",
                stroke: "currentColor",
                "strokeWidth": "2",
                // Maximize/fit-screen icon (four corners)
                path { d: "M3 8V5a2 2 0 0 1 2-2h3" }
                path { d: "M16 3h3a2 2 0 0 1 2 2v3" }
                path { d: "M21 16v3a2 2 0 0 1-2 2h-3" }
                path { d: "M8 21H5a2 2 0 0 1-2-2v-3" }
            }
        }
        div { class: "isb-hr" }
    }
}

/// The depth button and the popover it opens on hover, with a row per
/// direction the view draws.
#[component]
fn DepthPopover(
    view: PedigreeView,
    ancestor_levels: Signal<usize>,
    descendant_levels: Signal<usize>,
) -> Element {
    let i18n = use_i18n();
    let mut open = use_signal(|| false);
    let mut hover_generation = use_signal(|| 0u32);
    rsx! {
        div {
            class: "isb-depth-wrap",
            onmouseenter: move |_| {
                // Bump generation to cancel any pending close task.
                hover_generation += 1;
                open.set(true);
            },
            onmouseleave: move |_| {
                // Close after 200ms unless mouse re-enters (generation changes).
                let leave_generation = hover_generation();
                spawn(async move {
                    crate::utils::sleep_ms(200).await;
                    if hover_generation() == leave_generation {
                        open.set(false);
                    }
                });
            },
            button {
                class: "isb-btn",
                title: "{i18n.t(\"pedigree.depth\")}",
                svg {
                    width: "16",
                    height: "16",
                    fill: "none",
                    "viewBox": "0 0 24 24",
                    stroke: "currentColor",
                    "strokeWidth": "2",
                    // Layers/depth icon
                    path { d: "M12 2 2 7l10 5 10-5-10-5z" }
                    path { d: "M2 17l10 5 10-5" }
                    path { d: "M2 12l10 5 10-5" }
                }
            }
            if open() {
                div { class: "pedigree-depth-popover",
                    // A view drawing descendants only has no ancestor depth to
                    // set, and one drawing ancestors only no descendant depth.
                    if view.shows_ancestors() {
                        DepthRow { arrow: "\u{2191}", levels: ancestor_levels }
                    }
                    if view.shows_descendants() {
                        DepthRow { arrow: "\u{2193}", levels: descendant_levels }
                    }
                }
            }
        }
    }
}

/// The deepest a pedigree draws, either way.
const MAX_LEVELS: usize = 10;

/// One direction's depth, between none and [`MAX_LEVELS`].
#[component]
fn DepthRow(arrow: &'static str, levels: Signal<usize>) -> Element {
    rsx! {
        div { class: "pedigree-depth-row",
            span { class: "pedigree-depth-arrow", "{arrow}" }
            button {
                class: "pedigree-depth-btn",
                onclick: move |_| {
                    if levels() > 0 {
                        levels -= 1;
                    }
                },
                "\u{2212}" // −
            }
            span { class: "pedigree-depth-val", "{levels()}" }
            button {
                class: "pedigree-depth-btn",
                onclick: move |_| {
                    if levels() < MAX_LEVELS {
                        levels += 1;
                    }
                },
                "+"
            }
        }
    }
}

/// After a window resize: collapses the events panel if the window just
/// became too narrow for it, waits for it to move, and asks for a fit.
pub(super) async fn refit_after_resize(
    mut last_viewport_width: Signal<f64>,
    mut panel_collapsed: Signal<bool>,
    mut needs_fit: Signal<bool>,
) {
    let width =
        document::eval("return window.innerWidth || document.documentElement.clientWidth || 1024")
            .await
            .ok()
            .map(|val| val.as_f64().unwrap_or(VIEWPORT_DEFAULT_W));
    if let Some(width) = width {
        let narrowed = last_viewport_width() > EVENT_PANEL_AUTO_COLLAPSE_WIDTH
            && width <= EVENT_PANEL_AUTO_COLLAPSE_WIDTH;
        let panel_changed = narrowed && !panel_collapsed();
        if panel_changed {
            panel_collapsed.set(true);
        }
        last_viewport_width.set(width);
        if panel_changed {
            let _ = document::eval(WAIT_FOR_EVENT_PANEL_TRANSITION_JS).await;
        }
    }
    needs_fit.set(true);
}

/// Where a drag started: the pointer, and the pan it started from.
#[derive(Clone, Copy, PartialEq)]
struct Drag {
    pointer: (f64, f64),
    origin: (f64, f64),
}

/// The canvas the reader drags to pan and wheels to zoom about the cursor.
#[component]
pub(super) fn PanZoomViewport(
    class: String,
    saver: ViewSaver,
    viewport_rect: Signal<ViewportRect>,
    animating: Signal<bool>,
    max_zoom: f64,
    children: Element,
) -> Element {
    let mut drag = use_signal(|| None::<Drag>);
    let mut transform = saver.transform;
    let mut end_drag = move || {
        if drag.peek().is_some() {
            drag.set(None);
            saver.save();
        }
    };
    rsx! {
        div {
            class,
            onpointerdown: move |evt| {
                // Direct manipulation tracks the pointer 1:1 — the CSS
                // transition is only for programmatic jumps (fit/center).
                animating.set(false);
                let coords = evt.client_coordinates();
                let current = transform();
                drag.set(Some(Drag {
                    pointer: (coords.x, coords.y),
                    origin: (current.x, current.y),
                }));
            },
            onpointermove: move |evt| {
                let Some(Drag { pointer, origin }) = drag() else {
                    return;
                };
                let coords = evt.client_coordinates();
                transform.set(ViewportTransform {
                    x: origin.0 + coords.x - pointer.0,
                    y: origin.1 + coords.y - pointer.1,
                    ..transform()
                });
            },
            onpointerup: move |_| {
                drag.set(None);
                saver.save();
            },
            onpointerleave: move |_| end_drag(),
            onwheel: move |evt| wheel_zoom(&evt, saver, viewport_rect, animating, max_zoom),
            {children}
        }
    }
}

/// Zooms a step about the cursor, in or out as the wheel turns.
fn wheel_zoom(
    evt: &Event<WheelData>,
    saver: ViewSaver,
    viewport_rect: Signal<ViewportRect>,
    mut animating: Signal<bool>,
    max_zoom: f64,
) {
    let delta_y = match evt.delta() {
        WheelDelta::Lines(l) => l.y * 20.0,
        WheelDelta::Pixels(p) => p.y,
        WheelDelta::Pages(p) => p.y * 400.0,
    };
    let factor = if delta_y > 0.0 { 0.9 } else { 1.0 / 0.9 };
    let Some(new_scale) = zoom_step((saver.transform)().scale, factor, max_zoom) else {
        return;
    };
    // Same reasoning as a drag: a wheel gesture is a stream of many small
    // updates, each of which must land instantly or they visibly fight the
    // CSS transition and the zoom feels laggy. Reading the cached viewport
    // rect (refreshed on each fit) instead of an async DOM query per tick
    // keeps this handler fully synchronous, so there is no round-trip
    // latency and no risk of updates applying out of order.
    animating.set(false);
    let coords = evt.client_coordinates();
    let rect = viewport_rect();
    // The wheel holds the point under the cursor still.
    let anchor = (coords.x - rect.page_x, coords.y - rect.page_y);
    zoom_about(saver.transform, anchor, new_scale);
    saver.save();
}
