//! Floating context menu for person nodes in pedigree charts.
//!
//! Shows actions like Edit, Merge, Edit Union, Add Spouse, Add Child,
//! Add Sibling, Relationship, Delete when the user interacts with a person
//! box.

use std::rc::Rc;

use dioxus::prelude::*;
use uuid::Uuid;

use crate::i18n::use_i18n;

/// The anchor of a menu opened from a [`MenuButton`]: its right edge and the
/// gap below its bottom edge, from the trigger's client rectangle. The menu
/// is drawn right-aligned to this point (`.context-menu-anchor-right`).
fn menu_anchor(origin_x: f64, origin_y: f64, width: f64, height: f64) -> (f64, f64) {
    (origin_x + width, origin_y + height + MENU_GAP)
}

/// Space between a trigger and the menu it opens.
const MENU_GAP: f64 = 4.0;

/// The point, in client coordinates, where a page-owned menu opened from
/// `trigger` draws: under the trigger's left edge, a small gap below it.
pub async fn trigger_point(trigger: &MountedData) -> Option<(f64, f64)> {
    let rect = trigger.get_client_rect().await.ok()?;
    Some((rect.origin.x, rect.origin.y + rect.size.height + MENU_GAP))
}

/// Gives the focus back to the trigger that opened a page-owned menu, when the
/// menu closes without a choice that moves the focus elsewhere.
pub fn focus_trigger(trigger: Option<Rc<MountedData>>) {
    if let Some(button) = trigger {
        spawn(async move {
            let _ = button.set_focus(true).await;
        });
    }
}

/// Focuses the first element matching `selector` once the page has drawn it, for
/// a menu the page opens without a trigger of its own to move the focus.
pub fn focus_element(selector: &'static str) {
    let script = format!("document.querySelector('{selector}')?.focus();");
    spawn(async move {
        let _ = document::eval(&script).await;
    });
}

/// Clicks the first element matching `selector`, for a shortcut that opens a
/// menu by the same route as a click on its trigger.
pub fn click_element(selector: &'static str) {
    // A dispatched click rather than `click()`, which an SVG element lacks:
    // the chart's own menu trigger is one.
    let script = format!(
        "document.querySelector('{selector}')?.dispatchEvent(new MouseEvent('click', {{ bubbles: true }}));"
    );
    spawn(async move {
        let _ = document::eval(&script).await;
    });
}

/// Whether a key asks for a context menu: the context-menu key and Shift+F10.
pub fn opens_menu(event: &Event<KeyboardData>) -> bool {
    match event.key() {
        Key::ContextMenu => true,
        Key::F10 => event.modifiers().shift(),
        _ => false,
    }
}

/// Props of [`MenuButton`].
#[derive(Props, Clone, PartialEq)]
pub struct MenuButtonProps {
    /// The name of the element the menu acts on. The trigger is announced as
    /// "Actions for \<name\>".
    pub name: String,
    /// Extra class of the trigger, for its placement in the element.
    #[props(default)]
    pub class: String,
    /// Extra class of the menu, for a specialised width or layout.
    #[props(default)]
    pub menu_class: String,
    /// Told when the menu opens or closes, so the element can lift above its
    /// neighbours while its menu is shown.
    #[props(default)]
    pub on_open_change: Option<EventHandler<bool>>,
    /// The menu entries of a menu the trigger owns. Each receives the two ways
    /// to dismiss the menu; an entry that opens a sub-list uses neither.
    #[props(default)]
    pub items: Option<Callback<MenuClose, Element>>,
    /// Hands the trigger to the page instead of opening a menu of its own. The
    /// page draws its menu, with the same entries as its shortcuts, beside the
    /// trigger (see [`trigger_point`]) and gives the focus back with
    /// [`focus_trigger`] when that menu closes.
    #[props(default)]
    pub on_request: Option<EventHandler<Rc<MountedData>>>,
    /// Whether the menu the page owns for this trigger is open. The page draws
    /// that menu, so the trigger cannot know it from its own state: the page
    /// reports it here, and the trigger shows it as expanded.
    #[props(default = false)]
    pub expanded: bool,
    /// The shortcuts of the element the trigger belongs to. A request made
    /// there opens this trigger's menu, the same one a click opens.
    #[props(default)]
    pub shortcut: Option<MenuShortcut>,
}

/// How an entry of a [`MenuButton`] dismisses its menu.
#[derive(Clone, Copy)]
pub struct MenuClose {
    /// Dismisses the menu and returns the focus to the trigger.
    pub close: Callback<()>,
    /// Dismisses the menu and leaves the focus alone, for an entry whose
    /// action moves the focus itself, such as a dialog it opens.
    pub hand_off: Callback<()>,
}

/// Where a shortcut asks a [`MenuButton`] to open its menu.
#[derive(Clone, Copy, PartialEq)]
pub enum MenuOpen {
    /// At a point of the page: where a right click or a long press landed.
    At(f64, f64),
    /// Beside the trigger: the context-menu key and Shift+F10.
    Beside,
}

/// How long a touch must hold on an element before its menu opens.
pub(crate) const LONG_PRESS_MS: u32 = 500;

/// The shortcuts of an element whose secondary actions are a [`MenuButton`]
/// menu: a right click, the context-menu key, Shift+F10 and a long press open
/// that same menu, as a click on its trigger does. Hand it to the trigger as
/// its `shortcut` and wire the element's listeners to its methods.
#[derive(Clone, Copy, PartialEq)]
pub struct MenuShortcut {
    request: Signal<Option<MenuOpen>>,
    /// Counts the touches that began and ended, so a long press lifted before
    /// its time out does not open the menu.
    press: Signal<u32>,
}

/// The shortcut state of one element, created by its component.
pub fn use_menu_shortcut() -> MenuShortcut {
    MenuShortcut {
        request: use_signal(|| None),
        press: use_signal(|| 0),
    }
}

impl MenuShortcut {
    /// A right click opens the menu where it landed.
    pub fn right_click(mut self, event: &Event<MouseData>) {
        event.prevent_default();
        event.stop_propagation();
        let point = event.client_coordinates();
        self.request.set(Some(MenuOpen::At(point.x, point.y)));
    }

    /// The context-menu key and Shift+F10 open the menu beside its trigger;
    /// every other key is left to the element.
    pub fn key(mut self, event: &Event<KeyboardData>) {
        if opens_menu(event) {
            event.prevent_default();
            event.stop_propagation();
            self.request.set(Some(MenuOpen::Beside));
        }
    }

    /// A touch that holds for the long-press delay opens the menu where it
    /// landed.
    pub fn press_start(self, event: &Event<PointerData>) {
        if event.pointer_type() != "touch" {
            return;
        }
        event.stop_propagation();
        let point = event.client_coordinates();
        let mut request = self.request;
        long_press_start(self.press, (point.x, point.y), move |(x, y)| {
            request.set(Some(MenuOpen::At(x, y)));
        });
    }

    /// Ends a touch: a long press not yet due no longer opens the menu.
    pub fn press_end(self) {
        long_press_end(self.press);
    }
}

/// Starts a touch at `point`: `on_hold` runs there once the touch has held
/// for the long-press delay, unless [`long_press_end`] ran first. `press` counts
/// the touches that began and ended, so a touch that lifted early is ignored.
pub fn long_press_start(
    mut press: Signal<u32>,
    point: (f64, f64),
    on_hold: impl FnOnce((f64, f64)) + 'static,
) {
    let started = *press.peek() + 1;
    press.set(started);
    spawn(async move {
        crate::utils::sleep_ms(LONG_PRESS_MS).await;
        if *press.peek() == started {
            on_hold(point);
        }
    });
}

/// Ends the touch that [`long_press_start`] began.
pub fn long_press_end(mut press: Signal<u32>) {
    press += 1;
}

/// Tells the element that owns a trigger whether its menu is open.
fn notify_open(on_open_change: Option<EventHandler<bool>>, open: bool) {
    if let Some(handler) = on_open_change {
        handler.call(open);
    }
}

/// The one "⋮" trigger of every element with secondary actions, and the
/// shared menu it opens.
///
/// The trigger is always visible, muted, and stronger on hover or focus. A
/// click, the Enter or Space key, or ArrowDown opens the menu beside the
/// trigger; Escape or a click outside closes it and returns the focus to the
/// trigger. The trigger is never the only route: each element that opens this
/// The "⋮" of the element a chart has selected, drawn in the chart's own SVG
/// rather than as a [`MenuButton`] in a `foreignObject`: WebKitGTK, the
/// desktop's engine, places a `foreignObject`'s content without the
/// transforms of the groups above it, so a card's button landed elsewhere on
/// the chart. `x` and `y` are its centre in the element's coordinates and
/// `radius` its size, the chart's own. It
/// hands its activation to the page, which opens the chart's action picker:
/// a click at the click's point (`on_point`, in client coordinates, as the
/// right click does), a key beside the trigger (`on_request`).
#[component]
pub fn ChartMenuButton(
    name: String,
    x: f64,
    y: f64,
    radius: f64,
    expanded: bool,
    on_point: EventHandler<(f64, f64)>,
    on_request: EventHandler<Rc<MountedData>>,
) -> Element {
    let i18n = use_i18n();
    let mut trigger = use_signal(|| None::<Rc<MountedData>>);
    let label = i18n.t_args("common.actions_for", &[("name", &name)]);
    let title = format!("<title>{}</title>", crate::utils::escape_xml(&label));
    let request = use_callback(move |()| {
        if let Some(button) = trigger() {
            on_request.call(button);
        }
    });
    rsx! {
        g {
            class: "no-print chart-menu-button",
            transform: "translate({x},{y})",
            role: "button",
            tabindex: "0",
            "aria-label": "{label}",
            "aria-haspopup": "menu",
            "aria-expanded": if expanded { "true" } else { "false" },
            onmounted: move |evt: Event<MountedData>| trigger.set(Some(evt.data())),
            // A press on the trigger is not a long press on the card under it.
            onpointerdown: move |evt: Event<PointerData>| evt.stop_propagation(),
            onclick: move |evt: Event<MouseData>| {
                evt.stop_propagation();
                let point = evt.client_coordinates();
                on_point.call((point.x, point.y));
            },
            onkeydown: move |evt: Event<KeyboardData>| {
                let key = evt.key();
                if matches!(key, Key::Enter | Key::ArrowDown) || key == Key::Character(" ".to_string()) {
                    evt.prevent_default();
                    evt.stop_propagation();
                    request.call(());
                }
            },
            g { dangerous_inner_html: "{title}" }
            circle { class: "chart-menu-disc", r: "{radius}" }
            for cy in [-5.0, 0.0, 5.0] {
                circle { key: "{cy}", class: "chart-menu-dot", cy: "{cy}", r: "1.8" }
            }
        }
    }
}

/// menu from a right click, the menu key or a long press does so with the same
/// entries. A trigger with `on_request` hands its activation to the page.
#[component]
pub fn MenuButton(props: MenuButtonProps) -> Element {
    let i18n = use_i18n();
    let mut open = use_signal(|| false);
    let mut anchor = use_signal(|| None::<(f64, f64)>);
    let mut trigger = use_signal(|| None::<Rc<MountedData>>);
    let menu_id = use_hook(|| format!("menu-button-{}", Uuid::now_v7().simple()));
    let label = i18n.t_args("common.actions_for", &[("name", &props.name)]);
    let on_open_change = props.on_open_change;
    let on_request = props.on_request;

    let dismiss = use_callback(move |restore_focus: bool| {
        open.set(false);
        anchor.set(None);
        focus_trigger(trigger().filter(|_| restore_focus));
        notify_open(on_open_change, false);
    });
    let close = use_callback(move |()| dismiss.call(true));
    let hand_off = use_callback(move |()| dismiss.call(false));

    let show = use_callback(move |()| {
        open.set(true);
        notify_open(on_open_change, true);
        let Some(button) = trigger() else {
            return;
        };
        spawn(async move {
            match button.get_client_rect().await {
                Ok(rect) => anchor.set(Some(menu_anchor(
                    rect.origin.x,
                    rect.origin.y,
                    rect.size.width,
                    rect.size.height,
                ))),
                Err(_) => close.call(()),
            }
        });
    });

    // A shortcut opens the menu at its point, or beside the trigger.
    let open_at = use_callback(move |point: (f64, f64)| {
        open.set(true);
        anchor.set(Some(point));
        notify_open(on_open_change, true);
    });
    let shortcut = props.shortcut;
    use_effect(move || {
        let Some(shortcut) = shortcut else {
            return;
        };
        let Some(request) = (shortcut.request)() else {
            return;
        };
        let mut request_signal = shortcut.request;
        request_signal.set(None);
        match request {
            MenuOpen::At(x, y) => open_at.call((x, y)),
            MenuOpen::Beside => show.call(()),
        }
    });

    // The page's own menu, opened beside the trigger, when the trigger is
    // handed over; otherwise the menu of this trigger opens and closes.
    let hand_over = use_callback(move |()| {
        if let (Some(request), Some(button)) = (on_request, trigger()) {
            request.call(button);
        }
    });
    let toggle = use_callback(move |()| match (on_request.is_some(), open()) {
        (true, _) => hand_over.call(()),
        (false, true) => close.call(()),
        (false, false) => show.call(()),
    });
    let arrow_down = use_callback(move |()| match (on_request.is_some(), open()) {
        (true, _) => hand_over.call(()),
        (false, false) => show.call(()),
        (false, true) => {}
    });

    let menu_class = if props.menu_class.is_empty() {
        "context-menu-anchor-right".to_string()
    } else {
        format!("context-menu-anchor-right {}", props.menu_class)
    };
    let menu_id_attr = menu_id.clone();
    let is_open = open();
    // The trigger reads as expanded while either its own menu or the page's
    // menu for it is open. Only its own menu has an id the trigger can name.
    let is_expanded = is_open || props.expanded;

    rsx! {
        div {
            class: "menu-button-wrap no-print",
            // A press on the trigger belongs to the trigger: the element that
            // holds it, a row or a card, must not start its own long press.
            onpointerdown: move |evt: Event<PointerData>| evt.stop_propagation(),
            onkeydown: move |evt: Event<KeyboardData>| {
                if evt.key() == Key::Escape && open() {
                    evt.stop_propagation();
                    close.call(());
                }
            },
            button {
                class: "menu-button {props.class}",
                r#type: "button",
                title: "{label}",
                "aria-label": "{label}",
                "aria-haspopup": "menu",
                "aria-expanded": if is_expanded { "true" } else { "false" },
                "aria-controls": if is_open { Some(menu_id_attr.clone()) },
                onmounted: move |evt: Event<MountedData>| trigger.set(Some(evt.data())),
                onclick: move |evt: Event<MouseData>| {
                    evt.stop_propagation();
                    toggle.call(());
                },
                onkeydown: move |evt: Event<KeyboardData>| {
                    // The keys of the trigger belong to it, not to the element
                    // around it, where Enter or Space would toggle the row.
                    evt.stop_propagation();
                    if evt.key() == Key::ArrowDown {
                        evt.prevent_default();
                        arrow_down.call(());
                    }
                },
                // Drawn rather than typed: a font's "⋮" sits in the middle of
                // its advance, so the dots never lined up with what the
                // trigger leads (a media tile's picture), and fonts drew them
                // at different weights.
                svg {
                    class: "menu-button-icon",
                    width: "4",
                    height: "16",
                    "viewBox": "0 0 4 16",
                    "aria-hidden": "true",
                    circle { cx: "2", cy: "2", r: "1.75" }
                    circle { cx: "2", cy: "8", r: "1.75" }
                    circle { cx: "2", cy: "14", r: "1.75" }
                }
            }
            if is_open {
                if let (Some((x, y)), Some(items)) = (anchor(), props.items) {
                    MenuButtonSurface {
                        id: menu_id.clone(),
                        x,
                        y,
                        menu_class,
                        items,
                        close,
                        hand_off,
                    }
                }
            }
        }
    }
}

/// The menu a [`MenuButton`] opens: its entries in the shared surface, with
/// the focus on the first entry, so the keyboard reaches it without a Tab.
#[component]
fn MenuButtonSurface(
    id: String,
    x: f64,
    y: f64,
    menu_class: String,
    items: Callback<MenuClose, Element>,
    close: Callback<()>,
    hand_off: Callback<()>,
) -> Element {
    let focus_first = {
        let id = id.clone();
        move |_| {
            let script = format!("document.querySelector('#{id} .context-menu-item')?.focus();");
            spawn(async move {
                let _ = document::eval(&script).await;
            });
        }
    };
    rsx! {
        div {
            onmounted: focus_first,
            // The keys of the entries belong to the menu, not to the row
            // around the trigger, where Enter or Space would toggle it.
            onkeydown: move |evt: Event<KeyboardData>| evt.stop_propagation(),
            ContextMenuSurface {
                x,
                y,
                menu_class,
                id,
                on_close: move |_| close.call(()),
                {items.call(MenuClose { close, hand_off })}
            }
        }
    }
}

/// Shared fixed-position surface for contextual action menus.
///
/// It owns the click-outside backdrop, the native context-menu dismissal and
/// the dismissal on a window resize; callers supply only their
/// domain-specific actions.
#[derive(Props, Clone, PartialEq)]
pub struct ContextMenuSurfaceProps {
    pub x: f64,
    pub y: f64,
    /// Extra class for menus with a specialised width or child layout.
    #[props(default)]
    pub menu_class: String,
    /// The ARIA role of the surface. A menu takes `menu`, the default; a
    /// surface that wraps a menu of its own, or a panel of plain buttons,
    /// names the role it really is so the roles do not nest.
    #[props(default = "menu".to_string())]
    pub role: String,
    /// The element id of the menu, which the trigger names in `aria-controls`.
    #[props(default)]
    pub id: String,
    pub on_close: EventHandler<()>,
    pub children: Element,
}

#[component]
pub fn ContextMenuSurface(props: ContextMenuSurfaceProps) -> Element {
    // How far the menu is moved to stay in view: measured once drawn,
    // against the client area of the scrolling box that holds it — a page's
    // content scrolls in its own box, whose scrollbar sits inside the window
    // — or the window's.
    let mut shift = use_signal(|| (0.0_f64, 0.0_f64));
    let surface_id = use_hook(|| Uuid::now_v7().simple().to_string());
    let (shift_x, shift_y) = shift();
    let style = format!(
        "left: {}px; top: {}px;",
        props.x + shift_x,
        props.y + shift_y
    );
    let menu_class = if props.menu_class.is_empty() {
        "context-menu".to_string()
    } else {
        format!("context-menu {}", props.menu_class)
    };

    // The size the window had when the menu opened. The menu is placed at
    // coordinates measured then, which a resize moves out from under it — so
    // it closes, as a native menu does, rather than float off its anchor.
    let mut opened_at = use_signal(|| None::<(f64, f64)>);

    rsx! {
        div {
            class: "context-menu-backdrop",
            // The backdrop covers the window, so it resizes with it; but only
            // the window's own size says whether the window was resized. An
            // ancestor that is a containing block of fixed boxes — one with a
            // transform, such as an entrance animation running — sizes the
            // backdrop to itself, and it then follows that box: a page whose
            // content moves under the menu, or that animation ending, is not
            // a resize. The first report is the size it opened at.
            onresize: move |_: Event<ResizeData>| {
                spawn(async move {
                    let Some(size) = window_size().await else {
                        return;
                    };
                    match opened_at() {
                        None => opened_at.set(Some(size)),
                        Some(opened) if opened != size => props.on_close.call(()),
                        Some(_) => {}
                    }
                });
            },
            onclick: move |evt: Event<MouseData>| {
                evt.stop_propagation();
                props.on_close.call(());
            },
            oncontextmenu: move |evt: Event<MouseData>| {
                evt.prevent_default();
                props.on_close.call(());
            },
        }
        // The menu is a DOM child of whatever opened it, so swallow clicks
        // here: dismissing the menu or picking an action must never also
        // activate the element underneath.
        div {
            id: if !props.id.is_empty() { Some(props.id.clone()) },
            role: "{props.role}",
            class: "{menu_class}",
            style: "{style}",
            "data-menu-surface": "{surface_id}",
            onmounted: move |evt: Event<MountedData>| {
                let menu = evt.data();
                let surface = surface_id.clone();
                spawn(async move {
                    if let (Ok(rect), Some(area)) = (menu.get_client_rect().await, visible_area(&surface).await) {
                        shift.set(shift_into_window(
                            (rect.origin.x, rect.origin.y, rect.size.width, rect.size.height),
                            area,
                        ));
                    }
                });
            },
            onclick: move |evt: Event<MouseData>| evt.stop_propagation(),
            // Escape closes every menu the same way, whichever entry holds the
            // focus, and the close returns the focus to the trigger.
            onkeydown: move |evt: Event<KeyboardData>| {
                // Keys pressed in a menu never reach the element that holds it.
                evt.stop_propagation();
                if evt.key() == Key::Escape {
                    props.on_close.call(());
                }
            },
            {props.children}
        }
    }
}

/// Where the menu `surface` may lie, `(left, top, width, height)` in client
/// coordinates: the client area (scrollbars left out) of the nearest box
/// around it that scrolls vertically, within the window's client area.
async fn visible_area(surface: &str) -> Option<(f64, f64, f64, f64)> {
    let script = format!(
        r#"const menu = document.querySelector('[data-menu-surface="{surface}"]');
        const doc = document.documentElement;
        let [l, t, r, b] = [0, 0, doc.clientWidth, doc.clientHeight];
        for (let box = menu && menu.parentElement; box && box !== document.body; box = box.parentElement) {{
            const style = getComputedStyle(box);
            // A box with a scrollbar of its own: the menu must clear it.
            if (/(auto|scroll)/.test(style.overflowY) && box.scrollHeight > box.clientHeight) {{
                const rect = box.getBoundingClientRect();
                const left = rect.left + box.clientLeft;
                const top = rect.top + box.clientTop;
                l = Math.max(l, left);
                t = Math.max(t, top);
                r = Math.min(r, left + box.clientWidth);
                b = Math.min(b, top + box.clientHeight);
                break;
            }}
        }}
        return [l, t, r - l, b - t];"#
    );
    let value = document::eval(&script).await.ok()?;
    serde_json::from_value(value).ok()
}

/// The size of the window's viewport, `(width, height)`.
async fn window_size() -> Option<(f64, f64)> {
    let value = document::eval("return [window.innerWidth, window.innerHeight];")
        .await
        .ok()?;
    serde_json::from_value(value).ok()
}

/// The room a menu keeps from the window's edges.
const WINDOW_MARGIN: f64 = 8.0;

/// How far to move a menu drawn at `rect` (`x, y, width, height`) so that it
/// lies inside `area` (`left, top, width, height`), a margin from its edges:
/// back left or up by what overflows, then right or down if that crossed the
/// other edge. A menu larger than the area keeps its start edge in view.
fn shift_into_window(rect: (f64, f64, f64, f64), area: (f64, f64, f64, f64)) -> (f64, f64) {
    let along = |start: f64, size: f64, from: f64, room: f64| {
        let mut shift = 0.0;
        let end = start + size;
        if end > from + room - WINDOW_MARGIN {
            shift = from + room - WINDOW_MARGIN - end;
        }
        if start + shift < from + WINDOW_MARGIN {
            shift = from + WINDOW_MARGIN - start;
        }
        shift
    };
    (
        along(rect.0, rect.2, area.0, area.2),
        along(rect.1, rect.3, area.1, area.3),
    )
}

/// Actions that can be triggered from the context menu.
#[derive(Debug, Clone, PartialEq)]
pub enum PersonAction {
    Edit,
    Merge,
    AddParents,
    AddSpouse,
    AddChild,
    AddSibling,
    EditUnion,
    EditSpecificUnion(Uuid),
    /// Trace how the person is related to somebody else.
    Kinship,
    /// Put tags on the person, or take them off.
    Tags,
    /// Put a tag on the person's ancestors, descendants or both.
    TagBranch,
    /// Make a relative the chart's focus.
    GoTo(Uuid),
    Delete,
}

/// Props for [`ContextMenu`].
#[derive(Props, Clone, PartialEq)]
pub struct ContextMenuProps {
    pub person_name: String,
    pub x: f64,
    pub y: f64,
    #[props(default = false)]
    pub has_union: bool,
    /// List of unions: (family_id, partner_name, marriage_year).
    #[props(default)]
    pub unions: Vec<(Uuid, String, String)>,
    /// Relatives the chart does not draw around the person, to go to: a
    /// heading per kind, then (person, label) for each. Empty, no "Go to…".
    #[props(default)]
    pub go_to: Vec<(String, Vec<(Uuid, String)>)>,
    pub on_action: EventHandler<PersonAction>,
    pub on_close: EventHandler<()>,
}

#[component]
pub fn ContextMenu(props: ContextMenuProps) -> Element {
    let i18n = use_i18n();
    let mut show_union_sub = use_signal(|| false);
    let mut show_go_to = use_signal(|| false);

    let union_count = props.unions.len();

    rsx! {
        ContextMenuSurface {
            x: props.x,
            y: props.y,
            on_close: props.on_close,
            // The menu takes the focus when it opens, so Escape reaches it from its
            // first entry; a pointer or a shortcut opens it the same way.
            div {
                class: "context-menu-header",
                onmounted: move |_| focus_element(".context-menu .context-menu-item"),
                "{props.person_name}"
            }

            if show_go_to() {
                button {
                    role: "menuitem",
                    class: "context-menu-item context-menu-back",
                    onclick: move |_| show_go_to.set(false),
                    "\u{2190} {i18n.t(\"common.back\")}"
                }
                for (k, (heading, people)) in props.go_to.iter().enumerate() {
                    div { key: "g-{k}",
                        hr { class: "context-menu-divider" }
                        div { class: "context-menu-subheader", "{heading}" }
                        for (pid, label) in people.iter() {
                            {
                                let pid = *pid;
                                let on_action = props.on_action;
                                rsx! {
                                    button {
                                        key: "{pid}",
                                        role: "menuitem",
                                        class: "context-menu-item",
                                        onclick: move |_| on_action.call(PersonAction::GoTo(pid)),
                                        "{label}"
                                    }
                                }
                            }
                        }
                    }
                }
            } else if show_union_sub() {
                // Union sub-list: back arrow + union entries.
                button {
                    role: "menuitem",
                    class: "context-menu-item context-menu-back",
                    onclick: move |_| show_union_sub.set(false),
                    "\u{2190} {i18n.t(\"common.back\")}"
                }
                hr { class: "context-menu-divider" }
                for (fid, partner, year) in props.unions.iter() {
                    {
                        let fid = *fid;
                        let label = if year.is_empty() {
                            partner.clone()
                        } else {
                            format!("{partner}  \u{1F48D} {year}")
                        };
                        let on_action = props.on_action;
                        rsx! {
                            button {
                                role: "menuitem",
                                class: "context-menu-item",
                                onclick: move |_| on_action.call(PersonAction::EditSpecificUnion(fid)),
                                "{label}"
                            }
                        }
                    }
                }
            } else {
                // Main action list.
                button {
                    role: "menuitem",
                    class: "context-menu-item",
                    onclick: move |_| props.on_action.call(PersonAction::Edit),
                    {i18n.t("context.edit_individual")}
                }
                button {
                    role: "menuitem",
                    class: "context-menu-item",
                    onclick: move |_| props.on_action.call(PersonAction::Merge),
                    {i18n.t("context.merge")}
                }
                button {
                    role: "menuitem",
                    class: "context-menu-item",
                    onclick: move |_| props.on_action.call(PersonAction::Tags),
                    {i18n.t("tag.picker.open")}
                }
                button {
                    role: "menuitem",
                    class: "context-menu-item",
                    onclick: move |_| props.on_action.call(PersonAction::TagBranch),
                    {i18n.t("tag.branch.open")}
                }
                if props.has_union {
                    if union_count > 1 {
                        button {
                            role: "menuitem",
                            class: "context-menu-item",
                            onclick: move |_| show_union_sub.set(true),
                            {i18n.t("context.edit_union_submenu")}
                        }
                    } else {
                        button {
                            role: "menuitem",
                            class: "context-menu-item",
                            onclick: move |_| props.on_action.call(PersonAction::EditUnion),
                            {i18n.t("context.edit_union")}
                        }
                    }
                }
                button {
                    role: "menuitem",
                    class: "context-menu-item",
                    onclick: move |_| props.on_action.call(PersonAction::AddSpouse),
                    {i18n.t("context.add_spouse")}
                }
                button {
                    role: "menuitem",
                    class: "context-menu-item",
                    onclick: move |_| props.on_action.call(PersonAction::AddChild),
                    {i18n.t("context.add_child")}
                }
                button {
                    role: "menuitem",
                    class: "context-menu-item",
                    onclick: move |_| props.on_action.call(PersonAction::AddSibling),
                    {i18n.t("context.add_sibling")}
                }
                if !props.go_to.is_empty() {
                    hr { class: "context-menu-divider" }
                    button {
                        role: "menuitem",
                        class: "context-menu-item",
                        onclick: move |_| show_go_to.set(true),
                        {i18n.t("context.go_to")}
                    }
                }
                hr { class: "context-menu-divider" }
                button {
                    role: "menuitem",
                    class: "context-menu-item",
                    onclick: move |_| props.on_action.call(PersonAction::Kinship),
                    {i18n.t("context.kinship")}
                }
                hr { class: "context-menu-divider" }
                button {
                    role: "menuitem",
                    class: "context-menu-item context-menu-danger",
                    onclick: move |_| props.on_action.call(PersonAction::Delete),
                    {i18n.t("common.delete")}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MENU_GAP, menu_anchor, shift_into_window};

    #[test]
    fn a_menu_past_the_right_edge_moves_back_inside_the_client_area() {
        // A 200 px menu drawn at x = 1500 in a scrolling box whose client
        // area, scrollbar left out, ends at 1600 px.
        let (x, y) = shift_into_window((1500.0, 300.0, 200.0, 80.0), (0.0, 0.0, 1600.0, 900.0));
        assert_eq!((x, y), (-108.0, 0.0));
    }

    #[test]
    fn a_menu_stays_inside_a_centred_scrolling_box() {
        // The page's content scrolls in a box from x = 240 to 1440.
        let (x, _) = shift_into_window((1400.0, 300.0, 120.0, 40.0), (240.0, 48.0, 1200.0, 800.0));
        assert_eq!(x, -88.0);
    }

    #[test]
    fn a_menu_past_the_bottom_and_left_edges_moves_up_and_right() {
        let (x, y) = shift_into_window((-20.0, 850.0, 150.0, 120.0), (0.0, 0.0, 1000.0, 900.0));
        assert_eq!((x, y), (28.0, -78.0));
    }

    #[test]
    fn a_menu_inside_the_window_stays_where_it_is() {
        assert_eq!(
            shift_into_window((100.0, 100.0, 200.0, 80.0), (0.0, 0.0, 1000.0, 900.0)),
            (0.0, 0.0)
        );
    }

    /// The menu is right-aligned to the trigger's right edge and opens below
    /// its bottom edge with the shared gap.
    #[test]
    fn menu_opens_right_aligned_below_the_trigger() {
        let (x, y) = menu_anchor(100.0, 40.0, 24.0, 20.0);
        assert_eq!(x, 124.0);
        assert_eq!(y, 40.0 + 20.0 + MENU_GAP);
    }
}
