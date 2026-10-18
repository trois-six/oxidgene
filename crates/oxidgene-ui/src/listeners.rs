//! Listeners for the events that fire continuously: pointer and mouse moves,
//! wheel turns.
//!
//! On the desktop, Dioxus answers every *bubbling* event through a single
//! listener on the document, and hands each event to Rust synchronously: the
//! WebView waits for the handler before it goes on. One `onmousemove`,
//! `onpointermove` or `onwheel` mounted anywhere therefore sends every move or
//! turn in the window across and holds the page up for each, scrolling
//! included — the engine moves the pointer over the content it scrolls. So
//! no page keeps such a listener while nothing needs it
//! (docs/ui-common.md §6.5): a drag attaches its move listeners for its
//! duration only ([`while_active`]), and a wheel is read by a listener of the
//! page's own that never waits on Rust ([`forward_wheel`]).

use std::sync::atomic::{AtomicU64, Ordering};

use dioxus::prelude::*;

/// `listeners` while `active`, none otherwise: what an element spreads
/// (`..while_active(…)`) to listen to moves during a gesture only.
pub fn while_active(active: bool, listeners: impl FnOnce() -> Vec<Attribute>) -> Vec<Attribute> {
    if !active {
        return Vec::new();
    }
    let mut listeners = listeners();
    // The renderer diffs an element's spread attributes as a sorted list.
    listeners.sort_by_key(|attribute| attribute.name);
    listeners
}

/// An element id no other element of the page has, kept for the life of the
/// component: what a page-side listener finds its element by.
pub fn use_element_id(prefix: &str) -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    use_hook(|| format!("{prefix}-{}", NEXT.fetch_add(1, Ordering::Relaxed)))
}

/// One turn of the wheel: how far it scrolls vertically, in pixels, and
/// where the pointer was, in client coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WheelTurn {
    pub delta_y: f64,
    pub client_x: f64,
    pub client_y: f64,
}

/// What `WheelEvent.deltaMode` says a delta counts, as pixels: lines are
/// counted 20 pixels and pages 400, as the wheel handlers always did.
fn delta_pixels(mode: u32, delta: f64) -> f64 {
    match mode {
        1 => delta * 20.0,
        2 => delta * 400.0,
        _ => delta,
    }
}

/// Hands the wheel turns over the element `id` to `on_turn`, for as long as
/// the calling task lives — start it from the element's `onmounted`. The
/// page's own listener reads them and sends them on without waiting, so the
/// engine never stops for Rust. With `prevent_default`, a turn does not
/// also scroll whatever holds the element.
pub async fn forward_wheel(id: &str, prevent_default: bool, mut on_turn: impl FnMut(WheelTurn)) {
    let id = serde_json::to_string(id).unwrap_or_default();
    let prevent = if prevent_default {
        "event.preventDefault();"
    } else {
        ""
    };
    let mut turns = document::eval(&format!(
        "const element = document.getElementById({id});\n\
         if (element) element.addEventListener('wheel', event => {{\n\
             {prevent}\n\
             dioxus.send([event.deltaMode, event.deltaY, event.clientX, event.clientY]);\n\
         }}, {{ passive: {passive} }});\n\
         await new Promise(() => {{}});",
        passive = !prevent_default,
    ));
    while let Ok((mode, delta, client_x, client_y)) = turns.recv::<(u32, f64, f64, f64)>().await {
        on_turn(WheelTurn {
            delta_y: delta_pixels(mode, delta),
            client_x,
            client_y,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_and_page_deltas_are_counted_in_pixels() {
        assert_eq!(delta_pixels(0, 3.0), 3.0);
        assert_eq!(delta_pixels(1, 3.0), 60.0);
        assert_eq!(delta_pixels(2, -1.0), -400.0);
    }

    #[test]
    fn listeners_are_attached_only_while_active_in_name_order() {
        // Plain attributes stand for the listeners, which only a running
        // renderer can build; the order and the emptiness are the same.
        let listeners = || {
            vec![
                Attribute::new("onpointerup", "", None, false),
                Attribute::new("onpointermove", "", None, false),
            ]
        };
        assert!(while_active(false, listeners).is_empty());
        let names: Vec<_> = while_active(true, listeners)
            .iter()
            .map(|attribute| attribute.name)
            .collect();
        assert_eq!(names, ["onpointermove", "onpointerup"]);
    }
}
