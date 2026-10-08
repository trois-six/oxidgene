//! The back and forward buttons leading the application's first bar, and
//! the list of the pages either way that a long press or a secondary click
//! opens on them (see [`crate::nav_history`]).

use std::rc::Rc;

use dioxus::html::input_data::MouseButton;
use dioxus::prelude::*;

use crate::components::context_menu::ContextMenuSurface;
use crate::i18n::{I18n, use_i18n};
use crate::nav_history::{AppHistory, Entry, use_app_history};
use crate::router::Route;

/// How long a press lasts before it lists the pages rather than moving.
const LONG_PRESS_MS: u32 = 500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Direction {
    Back,
    Forward,
}

impl Direction {
    fn step(self) -> isize {
        match self {
            Self::Back => -1,
            Self::Forward => 1,
        }
    }

    fn label_key(self) -> &'static str {
        match self {
            Self::Back => "nav_history.back",
            Self::Forward => "nav_history.forward",
        }
    }

    fn tooltip_key(self) -> &'static str {
        match self {
            Self::Back => "nav_history.back_tooltip",
            Self::Forward => "nav_history.forward_tooltip",
        }
    }

    fn list_key(self) -> &'static str {
        match self {
            Self::Back => "nav_history.back_list",
            Self::Forward => "nav_history.forward_list",
        }
    }

    /// The arrow's strokes (Lucide's `arrow-left` and `arrow-right`).
    fn arrow(self) -> [&'static str; 2] {
        match self {
            Self::Back => ["m12 19-7-7 7-7", "M19 12H5"],
            Self::Forward => ["m12 5 7 7-7 7", "M5 12h14"],
        }
    }
}

/// The list of pages open, which way, and where.
#[derive(Clone, Copy, Debug, PartialEq)]
struct HistoryList {
    direction: Direction,
    x: f64,
    y: f64,
}

/// Holds the list the history buttons open.
///
/// The list is drawn once, beside the router, rather than inside the bar
/// whose button opened it: the navbar's blur makes it the frame of anything
/// fixed inside it, so a list drawn there could not cover the page.
pub fn use_init_history_list() {
    use_context_provider(|| Signal::new(None::<HistoryList>));
}

/// The back and forward buttons, each disabled when there is no page that
/// way.
#[component]
pub fn HistoryNav() -> Element {
    let i18n = use_i18n();
    // Read on every navigation, which is when the history catches up.
    let _route = use_route::<Route>();
    let Some(history) = use_app_history() else {
        return VNode::empty();
    };
    let (back, forward) = history.read(|h| (h.can_go_back(), h.can_go_forward()));
    rsx! {
        div {
            class: "nav-history no-print",
            role: "group",
            "aria-label": i18n.t("nav_history.label"),
            HistoryButton { direction: Direction::Back, enabled: back }
            HistoryButton { direction: Direction::Forward, enabled: forward }
        }
    }
}

#[component]
fn HistoryButton(direction: Direction, enabled: bool) -> Element {
    let i18n = use_i18n();
    let history = use_app_history();
    let mut list = use_context::<Signal<Option<HistoryList>>>();
    let mut element = use_signal(|| None::<Rc<MountedData>>);
    // The press under way, numbered so that a timer outliving its press
    // does nothing.
    let mut press = use_signal(|| None::<u32>);
    let mut presses = use_signal(|| 0_u32);
    // The press opened the list: the click that ends it does not move.
    let mut listed = use_signal(|| false);

    let open_list = move || {
        let Some(element) = element() else {
            return;
        };
        spawn(async move {
            if let Ok(rect) = element.get_client_rect().await {
                list.set(Some(HistoryList {
                    direction,
                    x: rect.origin.x,
                    y: rect.origin.y + rect.size.height + 4.0,
                }));
            }
        });
    };
    let [head, shaft] = direction.arrow();

    rsx! {
        button {
            class: "nav-history-btn",
            r#type: "button",
            disabled: !enabled,
            title: i18n.t(direction.tooltip_key()),
            "aria-label": i18n.t(direction.label_key()),
            "aria-haspopup": "menu",
            onmounted: move |e: MountedEvent| element.set(Some(e.data())),
            onclick: move |_| {
                if listed() {
                    listed.set(false);
                } else if let Some(history) = &history {
                    history.go(direction.step());
                }
            },
            oncontextmenu: move |e: Event<MouseData>| {
                e.prevent_default();
                open_list();
            },
            onpointerdown: move |e: Event<PointerData>| {
                listed.set(false);
                if e.trigger_button() != Some(MouseButton::Primary) {
                    return;
                }
                let id = presses() + 1;
                presses.set(id);
                press.set(Some(id));
                spawn(async move {
                    crate::utils::sleep_ms(LONG_PRESS_MS).await;
                    if press() == Some(id) {
                        press.set(None);
                        listed.set(true);
                        open_list();
                    }
                });
            },
            onpointerup: move |_| press.set(None),
            onpointerleave: move |_| press.set(None),
            onpointercancel: move |_| press.set(None),
            onkeydown: move |e: Event<KeyboardData>| {
                if e.key() == Key::ArrowDown {
                    e.prevent_default();
                    open_list();
                }
            },
            svg {
                width: "16",
                height: "16",
                fill: "none",
                "viewBox": "0 0 24 24",
                stroke: "currentColor",
                "strokeWidth": "2",
                "strokeLinecap": "round",
                "strokeLinejoin": "round",
                "aria-hidden": "true",
                path { d: head }
                path { d: shaft }
            }
        }
    }
}

/// The open list of the pages back or forward, nearest first: choosing one
/// moves there in one go.
#[component]
pub fn HistoryListHost() -> Element {
    let i18n = use_i18n();
    let mut list = use_context::<Signal<Option<HistoryList>>>();
    let history = use_app_history();
    let (Some(HistoryList { direction, x, y }), Some(history)) = (list(), history) else {
        return VNode::empty();
    };
    let entries = history.read(|h| {
        let entries: Vec<_> = match direction {
            Direction::Back => h.back_entries().collect(),
            Direction::Forward => h.forward_entries().collect(),
        };
        entries
            .into_iter()
            .map(|(delta, entry)| (delta, entry_label(&i18n, entry)))
            .collect::<Vec<_>>()
    });
    let close = move |()| list.set(None);
    rsx! {
        ContextMenuSurface { x, y, menu_class: "nav-history-menu", on_close: close,
            div {
                role: "menu",
                "aria-label": i18n.t(direction.list_key()),
                onkeydown: move |e: Event<KeyboardData>| {
                    if e.key() == Key::Escape {
                        list.set(None);
                    }
                },
                for (n, (delta, label)) in entries.into_iter().enumerate() {
                    {menu_item(&history, list, delta, label, n == 0)}
                }
            }
        }
    }
}

fn menu_item(
    history: &Rc<AppHistory>,
    mut list: Signal<Option<HistoryList>>,
    delta: isize,
    label: String,
    first: bool,
) -> Element {
    let history = history.clone();
    rsx! {
        button {
            key: "{delta}",
            class: "context-menu-item",
            r#type: "button",
            role: "menuitem",
            title: "{label}",
            // The list opens on the nearest page, so the keyboard can go
            // on from there.
            onmounted: move |e: MountedEvent| async move {
                if first {
                    let _ = e.set_focus(true).await;
                }
            },
            onclick: move |_| {
                list.set(None);
                history.go(delta);
            },
            "{label}"
        }
    }
}

/// An entry as the list reads it: the kind of page, then what it is about
/// when the page named it.
fn entry_label(i18n: &I18n, entry: &Entry<Route>) -> String {
    let page = i18n.t(entry.route().page_label_key());
    match entry.subject().filter(|subject| !subject.is_empty()) {
        Some(subject) => i18n.t_args(
            "nav_history.entry",
            &[("page", page.as_str()), ("subject", subject)],
        ),
        None => page,
    }
}
