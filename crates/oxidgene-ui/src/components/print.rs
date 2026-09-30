//! Printing a page.
//!
//! One action prints every printable page: [`PrintAction`], an icon button
//! of the tree pages' icon sidebar, just above Settings. Each page places a
//! [`PrintHeading`] in its topbar — the page title, the tree and the day —
//! which the print stylesheet in `layout.rs` shows in place of the topbar it
//! hides.
//!
//! What the button does differs per platform, and this module is the one
//! place that knows:
//!
//! - **Web** calls `window.print()`, the browser's own dialog and preview.
//! - **Desktop** installs a [`PrintBridge`]. A WebView's `window.print()` is
//!   not reliable — WKWebView ignores it outright — so the shell opens the
//!   platform's print dialog on the WebView itself instead.
//!
//! Either way the page is laid out by the `@media print` rules, and
//! Ctrl/Cmd+P presses the same button, so the shortcut and the button cannot
//! print two different things.

use std::sync::Arc;

use chrono::NaiveDate;
use dioxus::prelude::*;

use crate::components::date_input::format_day;
use crate::i18n::{I18n, use_i18n};
use crate::router::Route;

// ── Which pages print ───────────────────────────────────────────────────────

/// Whether `route` offers the print action.
///
/// Everything that shows the tree's content prints. The home page and the two
/// settings pages are controls rather than content, and a 404 has nothing to
/// print. The match is exhaustive on purpose: a new route does not compile
/// until someone decides whether it prints.
#[must_use]
pub fn is_printable(route: &Route) -> bool {
    match route {
        Route::Home {}
        | Route::Settings { .. }
        | Route::AppSettings {}
        | Route::NotFound { .. } => false,
        Route::TreeDetail { .. }
        | Route::SearchResults { .. }
        | Route::PersonDetail { .. }
        | Route::PersonHistory { .. }
        | Route::CoupleDetail { .. }
        | Route::Kinship { .. }
        | Route::Dictionary { .. }
        | Route::Statistics { .. }
        | Route::Tools { .. } => true,
    }
}

// ── Desktop capability ──────────────────────────────────────────────────────

/// Opens the platform's print dialog on the page.
///
/// Implemented by `oxidgene-desktop`. One method, because there is exactly one
/// thing the UI cannot do for itself inside a WebView.
pub trait PagePrinter: Send + Sync {
    /// Prints the page as the print stylesheet lays it out.
    fn print(&self);
}

/// Context handle the print action looks for.
#[derive(Clone)]
pub struct PrintBridge(Arc<dyn PagePrinter>);

impl PrintBridge {
    pub fn new(printer: Arc<dyn PagePrinter>) -> Self {
        Self(printer)
    }

    pub fn print(&self) {
        self.0.print();
    }
}

impl std::fmt::Debug for PrintBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PrintBridge")
    }
}

// ── Printed header ──────────────────────────────────────────────────────────

/// What the top of a printed sheet says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintHeader {
    /// The page, as its breadcrumb names it.
    pub title: String,
    /// The tree the page belongs to; empty while it loads.
    pub tree: String,
    /// « Printed on 29 Sep 2026 », in the reader's language.
    pub printed_on: String,
}

impl PrintHeader {
    #[must_use]
    pub fn new(i18n: &I18n, tree: &str, title: &str, day: NaiveDate) -> Self {
        Self {
            title: title.trim().to_string(),
            tree: tree.trim().to_string(),
            printed_on: i18n.t_args("print.printed_on", &[("date", &format_day(i18n, day))]),
        }
    }
}

/// The printed title of a search: the query itself, since the fields that
/// hold it on screen do not print.
#[must_use]
pub fn search_print_title(i18n: &I18n, last: &str, first: &str) -> String {
    let query = [last.trim(), first.trim()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if query.is_empty() {
        i18n.t("search.title")
    } else {
        i18n.t_args("print.search_for", &[("query", &query)])
    }
}

// ── Browser side ────────────────────────────────────────────────────────────

/// Installed once per window: the print hooks every page relies on.
///
/// - `__oxPreparePrint` snapshots the chart for printing (see below). It runs
///   on `beforeprint`, which also covers the browser's own menu, and the
///   action calls it again before a desktop print, whose dialog may not
///   raise the event.
/// - `afterprint` drops the snapshot again.
/// - Ctrl/Cmd+P presses the page's print button when it has one, and leaves
///   the key alone on a page that does not print.
///
/// A pedigree view only draws what its viewport reaches, and pans and zooms
/// it with a transform sized for the screen, so the live chart cannot simply
/// be reflowed onto paper. The snapshot copies the largest SVG in the
/// viewport with its `viewBox` narrowed to the area the reader sees, trimmed
/// to what is drawn, and appends it to `body` as `.print-chart`, which the
/// print stylesheet scales to the sheet. It works on whichever view is on
/// screen, since all it reads is the viewport's rectangle and the SVG's own
/// coordinate system, mapped from the box the SVG is drawn in (not
/// `getScreenCTM()`, which WebKit computes without the CSS zoom).
/// Identifiers are prefixed so the copy's references do not resolve into the
/// hidden original, and custom properties set inline on
/// the chart's ancestors are carried over. When `beforeprint` arrives with the
/// page already laid out for paper, the canvas has no area to measure, and
/// the snapshot the action took on screen a moment earlier is kept.
const PRINT_HOOKS_JS: &str = r#"
    if (!window.__oxPrintHooks) {
        window.__oxPrintHooks = true;
        const dropSnapshot = () => {
            document.querySelectorAll('.print-chart').forEach(node => node.remove());
        };
        window.__oxDropPrintSnapshot = dropSnapshot;
        // Screen point to the SVG's own coordinates, read off the box the SVG
        // is drawn in rather than `getScreenCTM()`: WebKit (the Linux and
        // macOS desktop) leaves the CSS zoom of an HTML ancestor out of that
        // matrix, which framed a zoomed-out chart as if it were at 100 %.
        const screenToChart = svg => {
            const box = svg.getBoundingClientRect();
            const base = svg.viewBox && svg.viewBox.baseVal;
            const vb = base && base.width > 0 && base.height > 0
                ? base
                : { x: 0, y: 0, width: svg.width.baseVal.value, height: svg.height.baseVal.value };
            if (!(box.width > 0 && box.height > 0 && vb.width > 0 && vb.height > 0)) return null;
            // `preserveAspectRatio` left at its default: meet, centred.
            const scale = Math.min(box.width / vb.width, box.height / vb.height);
            const left = box.left + (box.width - vb.width * scale) / 2;
            const top = box.top + (box.height - vb.height * scale) / 2;
            return (x, y) => ({ x: vb.x + (x - left) / scale, y: vb.y + (y - top) / scale });
        };
        window.__oxPreparePrint = () => {
            const viewport = document.querySelector('.pedigree-viewport');
            if (!viewport) {
                dropSnapshot();
                return;
            }
            // Laid out for print already, the canvas has no area to measure:
            // keep the snapshot the action took on screen a moment before.
            const rect = viewport.getBoundingClientRect();
            if (rect.width < 1 || rect.height < 1) return;
            dropSnapshot();
            let svg = null;
            let largest = 0;
            viewport.querySelectorAll('svg').forEach(candidate => {
                const r = candidate.getBoundingClientRect();
                if (r.width * r.height > largest) {
                    largest = r.width * r.height;
                    svg = candidate;
                }
            });
            const toChart = svg && screenToChart(svg);
            if (!toChart) return;
            let right = rect.right;
            const panel = document.querySelector('.ev-panel:not(.ev-panel-collapsed)');
            if (panel) {
                const p = panel.getBoundingClientRect();
                if (p.left > rect.left && p.left < right && p.right > rect.left) right = p.left;
            }
            const a = toChart(rect.left, rect.top);
            const b = toChart(right, rect.bottom);
            const drawn = svg.getBBox();
            const x0 = Math.max(Math.min(a.x, b.x), drawn.x);
            const y0 = Math.max(Math.min(a.y, b.y), drawn.y);
            const x1 = Math.min(Math.max(a.x, b.x), drawn.x + drawn.width);
            const y1 = Math.min(Math.max(a.y, b.y), drawn.y + drawn.height);
            if (!(x1 > x0 && y1 > y0)) return;

            const copy = svg.cloneNode(true);
            copy.setAttribute('viewBox', `${x0} ${y0} ${x1 - x0} ${y1 - y0}`);
            copy.setAttribute('preserveAspectRatio', 'xMidYMin meet');
            ['width', 'height', 'style'].forEach(name => copy.removeAttribute(name));
            const renamed = new Map();
            copy.querySelectorAll('[id]').forEach(node => {
                renamed.set(node.id, 'print-' + node.id);
                node.id = 'print-' + node.id;
            });
            if (renamed.size) {
                const rewrite = value => value
                    .replace(/url\(#([^)]+)\)/g, (m, id) => renamed.has(id) ? `url(#${renamed.get(id)})` : m)
                    .replace(/^#(.+)$/, (m, id) => renamed.has(id) ? `#${renamed.get(id)}` : m);
                copy.querySelectorAll('*').forEach(node => {
                    for (const attr of Array.from(node.attributes)) {
                        const next = rewrite(attr.value);
                        if (next !== attr.value) node.setAttribute(attr.name, next);
                    }
                });
            }
            const host = document.createElement('div');
            host.className = 'print-chart';
            host.setAttribute('aria-hidden', 'true');
            for (let node = svg.parentElement; node && node !== document.body; node = node.parentElement) {
                for (const name of Array.from(node.style)) {
                    if (name.startsWith('--') && !host.style.getPropertyValue(name)) {
                        host.style.setProperty(name, node.style.getPropertyValue(name));
                    }
                }
            }
            host.appendChild(copy);
            document.body.appendChild(host);
        };
        window.addEventListener('beforeprint', () => window.__oxPreparePrint());
        window.addEventListener('afterprint', dropSnapshot);
        document.addEventListener('keydown', event => {
            if (!(event.ctrlKey || event.metaKey) || event.altKey || event.shiftKey) return;
            if ((event.key || '').toLowerCase() !== 'p') return;
            const button = document.querySelector('.td-print-btn');
            if (!button) return;
            event.preventDefault();
            button.click();
        });
    }
"#;

/// Installs the print hooks. Called once, by the application shell.
pub fn use_init_print() {
    use_effect(|| {
        document::eval(PRINT_HOOKS_JS);
    });
}

/// The stylesheet that recolours the printed page with the `light` theme.
///
/// Paper is white whatever theme is on screen. Rather than spell colours out
/// in the stylesheet, the print media gets the `light` palette's own block,
/// emitted after the active one so it wins on paper only.
#[must_use]
pub fn print_palette_css() -> String {
    crate::theme::builtin_theme(crate::theme::DEFAULT_THEME_ID)
        .map(|theme| format!("@media print {{\n{}}}\n", theme.css()))
        .unwrap_or_default()
}

// ── Components ──────────────────────────────────────────────────────────────

/// The print button of the tree pages' icon sidebar, just above Settings.
///
/// It renders nothing on a route that does not print, so the sidebar the
/// settings page shares cannot offer it. The header the sheet prints under is
/// [`PrintHeading`], which each page places in its topbar.
#[component]
pub fn PrintAction() -> Element {
    let i18n = use_i18n();
    let route = use_route::<Route>();
    let bridge = try_use_context::<PrintBridge>();
    use_drop(|| {
        document::eval("window.__oxDropPrintSnapshot && window.__oxDropPrintSnapshot();");
    });
    if !is_printable(&route) {
        return rsx! {};
    }
    let label = i18n.t("print.action");
    let tooltip = i18n.t("print.tooltip");

    rsx! {
        button {
            r#type: "button",
            class: "isb-btn td-print-btn",
            title: "{tooltip}",
            "aria-label": "{label}",
            onclick: move |_| {
                let bridge = bridge.clone();
                spawn(async move {
                    // Awaited, so the snapshot exists before a native dialog
                    // lays the page out.
                    let _ = document::eval(
                        "window.__oxPreparePrint && window.__oxPreparePrint(); return true;",
                    )
                    .await;
                    match bridge {
                        Some(bridge) => bridge.print(),
                        None => {
                            document::eval("window.print();");
                        }
                    }
                });
            },
            svg {
                width: "16",
                height: "16",
                fill: "none",
                "viewBox": "0 0 24 24",
                stroke: "currentColor",
                "strokeWidth": "2",
                "aria-hidden": "true",
                path { d: "M6 9V3h12v6" }
                path { d: "M6 18H4a2 2 0 0 1-2-2v-5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2h-2" }
                rect { x: "6", y: "14", width: "12", height: "7" }
            }
        }
    }
}

/// The header a printed sheet carries: the page, the tree and the day.
///
/// Placed as the last child of the page's `.td-topbar`: on screen it does not
/// show; on paper the print stylesheet hides every other child of the topbar
/// and shows it instead.
#[component]
pub fn PrintHeading(
    /// The tree's name, as the breadcrumb shows it.
    tree_name: String,
    /// The page, as the breadcrumb's last crumb names it.
    title: String,
) -> Element {
    let i18n = use_i18n();
    let header = PrintHeader::new(&i18n, &tree_name, &title, chrono::Local::now().date_naive());
    rsx! {
        div { class: "print-header",
            div { class: "print-header-main",
                div { class: "print-header-title", "{header.title}" }
                if !header.tree.is_empty() {
                    div { class: "print-header-tree", "{header.tree}" }
                }
            }
            div { class: "print-header-date", "{header.printed_on}" }
        }
    }
}

/// « Page 2 of 7 » under a paginated list, on paper only.
///
/// A paginated list prints the page on screen, and its pagination controls do
/// not print, so this is what tells the reader the sheet is one page of more.
#[component]
pub fn PrintPageNote(page: usize, pages: usize) -> Element {
    let i18n = use_i18n();
    if pages <= 1 {
        return rsx! {};
    }
    rsx! {
        p { class: "print-page-note",
            {i18n.t_args("print.page_of", &[("page", &page.to_string()), ("total", &pages.to_string())])}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Language;

    fn tree() -> String {
        "00000000-0000-7000-8000-000000000001".to_string()
    }

    fn id() -> String {
        "00000000-0000-7000-8000-000000000002".to_string()
    }

    #[test]
    fn content_pages_print() {
        let printable = [
            Route::TreeDetail {
                tree_id: tree(),
                person: None,
            },
            Route::SearchResults {
                tree_id: tree(),
                last: "Doe".into(),
                first: String::new(),
                origin: String::new(),
            },
            Route::PersonDetail {
                tree_id: tree(),
                person_id: id(),
            },
            Route::PersonHistory {
                tree_id: tree(),
                person_id: id(),
            },
            Route::CoupleDetail {
                tree_id: tree(),
                family_id: id(),
            },
            Route::Kinship {
                tree_id: tree(),
                from: id(),
                to: String::new(),
            },
            Route::Dictionary { tree_id: tree() },
            Route::Statistics { tree_id: tree() },
            Route::Tools { tree_id: tree() },
        ];
        for route in printable {
            assert!(is_printable(&route), "{route} should print");
        }
    }

    #[test]
    fn home_settings_and_missing_pages_do_not_print() {
        let excluded = [
            Route::Home {},
            Route::Settings { tree_id: tree() },
            Route::AppSettings {},
            Route::NotFound {
                segments: vec!["nowhere".into()],
            },
        ];
        for route in excluded {
            assert!(!is_printable(&route), "{route} should not print");
        }
    }

    /// The guard in [`PrintAction`] already keeps the button off these pages;
    /// this keeps anyone from giving them a printed header in the first place.
    #[test]
    fn home_and_settings_pages_place_no_print_heading() {
        let pages = [
            ("home", include_str!("../pages/home.rs")),
            ("settings", include_str!("../pages/settings.rs")),
            ("app_settings", include_str!("../pages/app_settings.rs")),
        ];
        for (name, source) in pages {
            assert!(
                !source.contains("PrintHeading"),
                "the {name} page must not offer printing"
            );
        }
    }

    #[test]
    fn header_names_the_page_the_tree_and_the_day() {
        let i18n = I18n(Language::En);
        let day = NaiveDate::from_ymd_opt(2026, 9, 29).unwrap();
        let header = PrintHeader::new(&i18n, " Sample tree ", "Statistics", day);
        assert_eq!(header.title, "Statistics");
        assert_eq!(header.tree, "Sample tree");
        assert_eq!(header.printed_on, "Printed on 29 Sep 2026");
    }

    #[test]
    fn header_date_follows_the_reader_language() {
        let i18n = I18n(Language::Fr);
        let day = NaiveDate::from_ymd_opt(2026, 9, 29).unwrap();
        let header = PrintHeader::new(&i18n, "", "Statistiques", day);
        assert!(header.tree.is_empty());
        assert!(
            header.printed_on.starts_with("Imprimé le 29 "),
            "{}",
            header.printed_on
        );
        assert!(
            header.printed_on.ends_with(" 2026"),
            "{}",
            header.printed_on
        );
    }

    #[test]
    fn a_printed_search_names_its_query() {
        let i18n = I18n(Language::En);
        assert_eq!(
            search_print_title(&i18n, " Doe ", "Jane"),
            "Search: Doe Jane"
        );
        assert_eq!(search_print_title(&i18n, "", "Jane"), "Search: Jane");
        assert_eq!(search_print_title(&i18n, " ", ""), "Search");
    }

    #[test]
    fn print_palette_is_the_light_theme_for_print_media_only() {
        let css = print_palette_css();
        assert!(css.starts_with("@media print {"));
        let light = crate::theme::builtin_theme(crate::theme::DEFAULT_THEME_ID).unwrap();
        assert!(css.contains(&light.css()));
    }
}
