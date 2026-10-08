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
use crate::components::modal::Modal;
use crate::i18n::{I18n, use_i18n};
use crate::router::Route;
use crate::ui_observability::{UiActionStep, UiCommand, trace_ui_action, trace_ui_action_step};

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
    match search_query(last, first) {
        Some(query) => i18n.t_args("print.search_for", &[("query", &query)]),
        None => i18n.t("search.title"),
    }
}

/// The names searched for, family names first; none for an empty search.
pub fn search_query(last: &str, first: &str) -> Option<String> {
    let query = [last.trim(), first.trim()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    (!query.is_empty()).then_some(query)
}

// ── The whole chart over several sheets ────────────────────────────────────

/// Width of the part of the chart one sheet prints, in millimetres.
///
/// With the sheet's 10 mm margins it fits the printable area of both A4
/// and US Letter in landscape, whichever the printer holds, and stays clear
/// of the 3–6 mm at the paper's edge most printers cannot reach.
pub const TILE_W_MM: f64 = 255.0;
/// Height of the part of the chart one sheet prints, in millimetres, below
/// its one-line caption.
pub const TILE_H_MM: f64 = 175.0;
/// How far each sheet runs on under the next, so they can be assembled.
/// Its edge is drawn as a dashed line, inside the printed area.
pub const OVERLAP_MM: f64 = 10.0;
/// The most sheets a chart is printed on: beyond, the reader zooms out.
pub const MAX_SHEETS: usize = 50;
/// Millimetres per CSS pixel, which prints at 96 per inch.
const MM_PER_PX: f64 = 25.4 / 96.0;

/// What the chart draws, as a box in its own units, and how many screen
/// pixels one unit takes at the zoom shown, as `__oxMeasureChart` reads
/// them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChartMeasure {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub scale: f64,
    /// Whether all of it is on screen, clear of the events panel: then what
    /// the screen shows is the whole chart, and it prints on one sheet.
    pub on_screen: bool,
}

/// One sheet's part of the chart.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Tile {
    pub row: usize,
    pub col: usize,
    /// `x y width height` in chart units.
    pub view_box: [f64; 4],
    /// Where the next sheet to the right starts, as a vertical line, when
    /// there is one.
    pub guide_x: Option<f64>,
    /// Where the next sheet below starts, as a horizontal line.
    pub guide_y: Option<f64>,
    pub caption: String,
}

/// The sheets a chart prints on at the zoom shown: it keeps on paper the
/// size it has on screen, cut into tiles of [`TILE_W_MM`] × [`TILE_H_MM`]
/// overlapping by [`OVERLAP_MM`], row by row.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct TilePlan {
    pub cols: usize,
    pub rows: usize,
    pub width_mm: f64,
    pub height_mm: f64,
    pub tiles: Vec<Tile>,
}

impl TilePlan {
    pub fn sheets(&self) -> usize {
        self.cols * self.rows
    }
}

/// How many steps of `step` after a first `tile` cover `extent`.
fn tiles_along(extent: f64, tile: f64, step: f64) -> usize {
    if extent <= tile {
        1
    } else {
        1 + ((extent - tile) / step).ceil() as usize
    }
}

/// How much smaller than its screen size a chart may print to save sheets:
/// a chart a centimetre taller than a sheet prints on one, slightly reduced,
/// rather than on two with a strip on the second.
pub const MAX_SHRINK: f64 = 0.85;

/// How many sheets `chart` takes along each axis at `scale` screen pixels
/// per chart unit.
fn sheet_counts(chart: ChartMeasure, scale: f64) -> (usize, usize) {
    let mm = |extent: f64| extent * scale * MM_PER_PX;
    (
        tiles_along(mm(chart.width), TILE_W_MM, TILE_W_MM - OVERLAP_MM),
        tiles_along(mm(chart.height), TILE_H_MM, TILE_H_MM - OVERLAP_MM),
    )
}

/// The scale the chart prints at: its screen scale, or up to
/// [`MAX_SHRINK`] of it when that takes fewer sheets — then just small
/// enough for those sheets.
fn print_scale(chart: ChartMeasure) -> f64 {
    let scale = chart.scale.max(f64::EPSILON);
    let (cols, rows) = sheet_counts(chart, scale);
    let (fewer_cols, fewer_rows) = sheet_counts(chart, scale * MAX_SHRINK);
    if fewer_cols * fewer_rows >= cols * rows {
        return scale;
    }
    // The largest scale at which `count` tiles still cover `extent` units.
    let fitting = |extent: f64, count: usize, tile: f64| {
        (tile + (count as f64 - 1.0) * (tile - OVERLAP_MM)) / (extent * MM_PER_PX)
    };
    scale
        .min(fitting(chart.width, fewer_cols, TILE_W_MM))
        .min(fitting(chart.height, fewer_rows, TILE_H_MM))
}

/// Plans the sheets for `chart` at its zoom (see [`print_scale`]),
/// captioning each with `caption(n, total, row, col)`. Only the counts are
/// computed while there would be more than [`MAX_SHEETS`]: the sheets are
/// then not offered.
pub fn tile_plan(
    chart: ChartMeasure,
    caption: impl Fn(usize, usize, usize, usize) -> String,
) -> TilePlan {
    let mm_per_unit = print_scale(chart) * MM_PER_PX;
    let (tile_w, tile_h) = (TILE_W_MM / mm_per_unit, TILE_H_MM / mm_per_unit);
    let overlap = OVERLAP_MM / mm_per_unit;
    let (step_w, step_h) = (tile_w - overlap, tile_h - overlap);
    let cols = tiles_along(chart.width, tile_w, step_w);
    let rows = tiles_along(chart.height, tile_h, step_h);
    let total = cols * rows;
    let mut tiles = Vec::new();
    if total <= MAX_SHEETS {
        for row in 0..rows {
            for col in 0..cols {
                let (x, y) = (chart.x + col as f64 * step_w, chart.y + row as f64 * step_h);
                tiles.push(Tile {
                    row,
                    col,
                    view_box: [x, y, tile_w, tile_h],
                    guide_x: (col + 1 < cols).then_some(x + step_w),
                    guide_y: (row + 1 < rows).then_some(y + step_h),
                    caption: caption(tiles.len() + 1, total, row + 1, col + 1),
                });
            }
        }
    }
    TilePlan {
        cols,
        rows,
        width_mm: TILE_W_MM,
        height_mm: TILE_H_MM,
        tiles,
    }
}

/// Set while a chart prints over several sheets: every view then draws its
/// whole chart instead of the part near the viewport, so the copy printed
/// holds all of it.
#[derive(Clone, Copy)]
pub struct PrintEverything(pub Signal<bool>);

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
            window.__oxTiled = false;
        };
        window.__oxDropPrintSnapshot = dropSnapshot;
        // The chart on screen: the largest SVG of the canvas.
        const chartSvg = () => {
            const viewport = document.querySelector('.pedigree-viewport');
            if (!viewport) return null;
            let svg = null;
            let largest = 0;
            viewport.querySelectorAll('svg').forEach(candidate => {
                const r = candidate.getBoundingClientRect();
                if (r.width * r.height > largest) {
                    largest = r.width * r.height;
                    svg = candidate;
                }
            });
            return svg;
        };
        const viewBoxOf = svg => {
            const base = svg.viewBox && svg.viewBox.baseVal;
            return base && base.width > 0 && base.height > 0
                ? base
                : { x: 0, y: 0, width: svg.width.baseVal.value, height: svg.height.baseVal.value };
        };
        // Screen pixels per chart unit, read off the box the SVG is drawn in
        // rather than `getScreenCTM()`: WebKit (the Linux and macOS desktop)
        // leaves the CSS zoom of an HTML ancestor out of that matrix, which
        // framed a zoomed-out chart as if it were at 100 %.
        const screenScale = svg => {
            const box = svg.getBoundingClientRect();
            const vb = viewBoxOf(svg);
            if (!(box.width > 0 && box.height > 0 && vb.width > 0 && vb.height > 0)) return null;
            // `preserveAspectRatio` left at its default: meet, centred.
            return Math.min(box.width / vb.width, box.height / vb.height);
        };
        // Screen point to the SVG's own coordinates.
        const screenToChart = svg => {
            const scale = screenScale(svg);
            if (!scale) return null;
            const box = svg.getBoundingClientRect();
            const vb = viewBoxOf(svg);
            const left = box.left + (box.width - vb.width * scale) / 2;
            const top = box.top + (box.height - vb.height * scale) / 2;
            return (x, y) => ({ x: vb.x + (x - left) / scale, y: vb.y + (y - top) / scale });
        };
        // A copy whose identifiers do not resolve into the hidden original.
        const renameIds = copy => {
            const renamed = new Map();
            copy.querySelectorAll('[id]').forEach(node => {
                renamed.set(node.id, 'print-' + node.id);
                node.id = 'print-' + node.id;
            });
            if (!renamed.size) return;
            const rewrite = value => value
                .replace(/url\(#([^)]+)\)/g, (m, id) => renamed.has(id) ? `url(#${renamed.get(id)})` : m)
                .replace(/^#(.+)$/, (m, id) => renamed.has(id) ? `#${renamed.get(id)}` : m);
            copy.querySelectorAll('*').forEach(node => {
                for (const attr of Array.from(node.attributes)) {
                    const next = rewrite(attr.value);
                    if (next !== attr.value) node.setAttribute(attr.name, next);
                }
            });
        };
        // The host of a printed copy, carrying the custom properties set
        // inline on the chart's ancestors.
        const printHost = (svg, className) => {
            const host = document.createElement('div');
            host.className = className;
            host.setAttribute('aria-hidden', 'true');
            for (let node = svg.parentElement; node && node !== document.body; node = node.parentElement) {
                for (const name of Array.from(node.style)) {
                    if (name.startsWith('--') && !host.style.getPropertyValue(name)) {
                        host.style.setProperty(name, node.style.getPropertyValue(name));
                    }
                }
            }
            return host;
        };
        // The right edge of the canvas the events panel leaves free.
        const freeRight = rect => {
            let right = rect.right;
            const panel = document.querySelector('.ev-panel:not(.ev-panel-collapsed)');
            if (panel) {
                const p = panel.getBoundingClientRect();
                if (p.left > rect.left && p.left < right && p.right > rect.left) right = p.left;
            }
            return right;
        };
        window.__oxPreparePrint = () => {
            // A multi-sheet print prepared by the action is what prints.
            if (window.__oxTiled) return;
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
            const svg = chartSvg();
            const toChart = svg && screenToChart(svg);
            if (!toChart) return;
            const right = freeRight(rect);
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
            renameIds(copy);
            const host = printHost(svg, 'print-chart');
            host.appendChild(copy);
            document.body.appendChild(host);
        };
        // What the chart draws, the screen's zoom, and whether all of it is
        // on screen, for the sheet plan. Read once the chart is drawn whole.
        window.__oxMeasureChart = () => {
            const svg = chartSvg();
            const scale = svg && screenScale(svg);
            if (!scale) return null;
            const drawn = svg.getBBox();
            const vb = viewBoxOf(svg);
            const box = svg.getBoundingClientRect();
            const left = box.left + (box.width - vb.width * scale) / 2 + (drawn.x - vb.x) * scale;
            const top = box.top + (box.height - vb.height * scale) / 2 + (drawn.y - vb.y) * scale;
            const rect = document.querySelector('.pedigree-viewport').getBoundingClientRect();
            const onScreen = left >= rect.left - 1 && top >= rect.top - 1
                && left + drawn.width * scale <= freeRight(rect) + 1
                && top + drawn.height * scale <= rect.bottom + 1;
            return [drawn.x, drawn.y, drawn.width, drawn.height, scale, onScreen];
        };
        // The whole chart over several sheets: one hidden copy, and a sheet
        // per tile showing its part of it, the dashed lines where the next
        // sheets overlap, and its caption. `plan` comes from `tile_plan`.
        window.__oxPrepareTiledPrint = plan => {
            dropSnapshot();
            const svg = chartSvg();
            if (!svg) return 0;
            const ns = 'http://www.w3.org/2000/svg';
            const host = printHost(svg, 'print-chart print-tiles');
            const source = svg.cloneNode(true);
            renameIds(source);
            const holder = document.createElementNS(ns, 'svg');
            holder.setAttribute('class', (svg.getAttribute('class') || '') + ' print-tiles-source');
            holder.setAttribute('width', '0');
            holder.setAttribute('height', '0');
            const defs = document.createElementNS(ns, 'defs');
            const group = document.createElementNS(ns, 'g');
            group.id = 'print-tiles-chart';
            Array.from(source.childNodes).forEach(child => group.appendChild(child));
            defs.appendChild(group);
            holder.appendChild(defs);
            host.appendChild(holder);
            const title = document.querySelector('.print-header-title');
            const tree = document.querySelector('.print-header-tree');
            const heading = [title, tree].filter(Boolean).map(n => n.textContent.trim()).filter(Boolean).join(' · ');
            plan.tiles.forEach(tile => {
                const sheet = document.createElement('div');
                sheet.className = 'print-tile';
                const caption = document.createElement('div');
                caption.className = 'print-tile-caption';
                caption.textContent = heading ? `${heading} — ${tile.caption}` : tile.caption;
                sheet.appendChild(caption);
                const part = document.createElementNS(ns, 'svg');
                part.setAttribute('class', svg.getAttribute('class') || '');
                part.setAttribute('viewBox', tile.view_box.join(' '));
                part.setAttribute('preserveAspectRatio', 'xMinYMin meet');
                part.style.width = `${plan.width_mm}mm`;
                part.style.height = `${plan.height_mm}mm`;
                const use = document.createElementNS(ns, 'use');
                use.setAttribute('href', '#print-tiles-chart');
                part.appendChild(use);
                const [x, y, w, h] = tile.view_box;
                const guide = (x1, y1, x2, y2) => {
                    const line = document.createElementNS(ns, 'line');
                    line.setAttribute('class', 'print-tile-guide');
                    [['x1', x1], ['y1', y1], ['x2', x2], ['y2', y2]].forEach(([k, v]) => line.setAttribute(k, v));
                    part.appendChild(line);
                };
                if (tile.guide_x !== null) guide(tile.guide_x, y, tile.guide_x, y + h);
                if (tile.guide_y !== null) guide(x, tile.guide_y, x + w, tile.guide_y);
                sheet.appendChild(part);
                host.appendChild(sheet);
            });
            document.body.appendChild(host);
            window.__oxTiled = true;
            return plan.tiles.length;
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

/// Installs the print hooks and provides [`PrintEverything`]. Called once,
/// by the application shell.
pub fn use_init_print() {
    use_context_provider(|| PrintEverything(Signal::new(false)));
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

/// Prints the page as the print stylesheet lays it out: through the desktop
/// shell's dialog when there is one, `window.print()` on the web.
async fn print_now(bridge: Option<PrintBridge>) {
    trace_ui_action_step(UiActionStep::PrintDialog, async {
        match bridge {
            Some(bridge) => bridge.print(),
            None => {
                let _ = document::eval("window.print(); return true;").await;
            }
        }
    })
    .await;
}

/// Prints what the chart shows on screen, on one sheet.
async fn print_screen(bridge: Option<PrintBridge>) {
    // Awaited, so the snapshot exists before a native dialog lays the page
    // out.
    trace_ui_action_step(UiActionStep::PrintPrepare, async {
        let _ =
            document::eval("window.__oxPreparePrint && window.__oxPreparePrint(); return true;")
                .await;
    })
    .await;
    print_now(bridge).await;
}

/// Has every view draw its whole chart (`true`) or only what is near the
/// viewport again (`false`); drawing whole waits for the chart to be laid
/// out: a pause for the render, then two frames for the browser.
async fn draw_whole(everything: Option<PrintEverything>, whole: bool) {
    let Some(PrintEverything(mut all)) = everything else {
        return;
    };
    all.set(whole);
    if whole {
        crate::utils::sleep_ms(150).await;
        let _ = document::eval(
            "await new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r))); return true;",
        )
        .await;
    }
}

/// Prints the whole chart over the sheets of `plan`, drawn whole by the
/// caller; every view then goes back to drawing what is near.
async fn print_tiles(
    bridge: Option<PrintBridge>,
    plan: TilePlan,
    everything: Option<PrintEverything>,
) {
    let script = format!(
        "return window.__oxPrepareTiledPrint({});",
        serde_json::to_string(&plan).unwrap_or_else(|_| "{\"tiles\":[]}".to_string())
    );
    trace_ui_action_step(UiActionStep::PrintPrepare, async {
        let _ = document::eval(&script).await;
        draw_whole(everything, false).await;
    })
    .await;
    print_now(bridge).await;
}

/// The chart on screen, measured, when the page shows one.
async fn measure_chart() -> Option<ChartMeasure> {
    let value =
        document::eval("return window.__oxMeasureChart ? window.__oxMeasureChart() : null;")
            .await
            .ok()?;
    let (x, y, width, height, scale, on_screen): (f64, f64, f64, f64, f64, bool) =
        serde_json::from_value(value).ok()?;
    Some(ChartMeasure {
        x,
        y,
        width,
        height,
        scale,
        on_screen,
    })
}

/// The print button of the tree pages' icon sidebar, just above Settings.
///
/// It renders nothing on a route that does not print, so the sidebar the
/// settings page shares cannot offer it. On a page showing a chart that
/// runs past the screen and is larger than one sheet at its zoom, it first
/// asks whether to print what the screen shows or the whole chart over
/// several sheets; a chart all on screen prints on one sheet at once. The header the sheet
/// prints under is [`PrintHeading`], which each page places in its topbar.
#[component]
pub fn PrintAction() -> Element {
    let i18n = use_i18n();
    let route = use_route::<Route>();
    let bridge = try_use_context::<PrintBridge>();
    let everything = try_use_context::<PrintEverything>();
    let mut choice = use_signal(|| None::<TilePlan>);
    use_drop(|| {
        document::eval("window.__oxDropPrintSnapshot && window.__oxDropPrintSnapshot();");
    });
    if !is_printable(&route) {
        return rsx! {};
    }
    let label = i18n.t("print.action");
    let tooltip = i18n.t("print.tooltip");
    let bridge_screen = bridge.clone();
    let bridge_tiles = bridge.clone();

    rsx! {
        button {
            r#type: "button",
            class: "isb-btn td-print-btn",
            title: "{tooltip}",
            "aria-label": "{label}",
            onclick: move |_| {
                let bridge = bridge.clone();
                spawn(trace_ui_action(UiCommand::Print, async move {
                    // Measured drawn whole, so what it takes is known even
                    // for the parts the view left out; it stays whole while
                    // the choice is open.
                    let chart = trace_ui_action_step(UiActionStep::PrintMeasure, async {
                        draw_whole(everything, true).await;
                        measure_chart().await
                    })
                    .await;
                    let plan = chart
                        .filter(|chart| !chart.on_screen)
                        .map(|chart| {
                            tile_plan(chart, |n, total, row, col| sheet_caption(&i18n, n, total, row, col))
                        })
                        .filter(|plan| plan.sheets() > 1);
                    match plan {
                        Some(plan) => choice.set(Some(plan)),
                        None => {
                            draw_whole(everything, false).await;
                            print_screen(bridge).await;
                        }
                    }
                }));
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
        if let Some(plan) = choice() {
            PrintChoice {
                plan,
                on_screen: move |_| {
                    choice.set(None);
                    let bridge = bridge_screen.clone();
                    spawn(trace_ui_action(UiCommand::Print, async move {
                        draw_whole(everything, false).await;
                        print_screen(bridge).await;
                    }));
                },
                on_tiles: move |plan: TilePlan| {
                    choice.set(None);
                    let bridge = bridge_tiles.clone();
                    spawn(trace_ui_action(UiCommand::Print, async move {
                        print_tiles(bridge, plan, everything).await;
                    }));
                },
                on_cancel: move |_| {
                    choice.set(None);
                    spawn(async move { draw_whole(everything, false).await });
                },
            }
        }
    }
}

/// « Sheet 2 of 6 · row 1, column 2 », in the reader's language.
fn sheet_caption(i18n: &I18n, n: usize, total: usize, row: usize, col: usize) -> String {
    i18n.t_args(
        "print.sheet",
        &[
            ("n", &n.to_string()),
            ("total", &total.to_string()),
            ("row", &row.to_string()),
            ("col", &col.to_string()),
        ],
    )
}

/// What to print of a chart larger than a sheet: what the screen shows, or
/// the whole chart at this zoom over several sheets.
#[component]
fn PrintChoice(
    plan: TilePlan,
    on_screen: EventHandler<()>,
    on_tiles: EventHandler<TilePlan>,
    on_cancel: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let sheets = plan.sheets();
    let too_many = sheets > MAX_SHEETS;
    let count = i18n.t_args(
        "print.sheets",
        &[
            ("count", &sheets.to_string()),
            ("cols", &plan.cols.to_string()),
            ("rows", &plan.rows.to_string()),
        ],
    );
    rsx! {
        Modal {
            class: "modal-card print-choice",
            label: i18n.t("print.choose_title"),
            on_close: on_cancel,
            h3 { {i18n.t("print.choose_title")} }
            div { class: "print-choice-options",
                button {
                    class: "print-choice-option",
                    onclick: move |_| on_screen.call(()),
                    span { class: "print-choice-name", {i18n.t("print.what_shows")} }
                    span { class: "print-choice-detail", {i18n.t("print.one_sheet")} }
                }
                button {
                    class: "print-choice-option",
                    disabled: too_many,
                    onclick: {
                        let plan = plan.clone();
                        move |_| on_tiles.call(plan.clone())
                    },
                    span { class: "print-choice-name", {i18n.t("print.whole_tree")} }
                    span { class: "print-choice-detail", "{count}" }
                }
            }
            if too_many {
                p { class: "stats-note",
                    {i18n.t_args("print.too_many", &[("max", &MAX_SHEETS.to_string())])}
                }
            } else {
                p { class: "stats-note", {i18n.t("print.tiles_hint")} }
            }
            div { class: "modal-actions",
                button { class: "btn btn-outline", onclick: move |_| on_cancel.call(()),
                    {i18n.t("common.cancel")}
                }
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

    fn chart(width: f64, height: f64, scale: f64) -> ChartMeasure {
        ChartMeasure {
            x: 10.0,
            y: 20.0,
            width,
            height,
            scale,
            on_screen: false,
        }
    }

    fn caption(n: usize, total: usize, row: usize, col: usize) -> String {
        format!("{n}/{total} r{row}c{col}")
    }

    /// A chart at its screen size: 96 CSS pixels to the inch. At 100 % a
    /// chart one tile wide or less prints on one sheet.
    #[test]
    fn a_chart_that_fits_one_tile_takes_one_sheet() {
        let tile_px = TILE_W_MM / MM_PER_PX;
        let plan = tile_plan(chart(tile_px - 1.0, 100.0, 1.0), caption);
        assert_eq!((plan.cols, plan.rows, plan.sheets()), (1, 1, 1));
        assert_eq!(plan.tiles[0].guide_x, None);
        assert_eq!(plan.tiles[0].guide_y, None);
    }

    /// Tiles cover the whole chart row by row, each overlapping the next by
    /// the overlap, marked where the next sheet starts; the zoom shown keeps
    /// the chart's size on paper.
    #[test]
    fn tiles_cover_the_chart_and_overlap_where_marked() {
        let scale = 0.5;
        let unit_mm = scale * MM_PER_PX;
        let (tile_w, tile_h) = (TILE_W_MM / unit_mm, TILE_H_MM / unit_mm);
        let overlap = OVERLAP_MM / unit_mm;
        let (w, h) = (2.5 * tile_w, 1.2 * tile_h);
        let plan = tile_plan(chart(w, h, scale), caption);
        assert_eq!((plan.cols, plan.rows), (3, 2));
        assert_eq!(plan.tiles.len(), 6);
        let first = &plan.tiles[0];
        assert_eq!(first.view_box[..2], [10.0, 20.0]);
        assert!((first.view_box[2] - tile_w).abs() < 1e-9);
        let right = &plan.tiles[1];
        assert!((right.view_box[0] - (first.view_box[0] + tile_w - overlap)).abs() < 1e-9);
        assert_eq!(first.guide_x, Some(right.view_box[0]));
        let below = &plan.tiles[3];
        assert_eq!(first.guide_y, Some(below.view_box[1]));
        let last = plan.tiles.last().unwrap();
        assert_eq!((last.guide_x, last.guide_y), (None, None));
        assert!(
            last.view_box[0] + last.view_box[2] >= 10.0 + w - 1e-9,
            "the right edge is covered"
        );
        assert!(
            last.view_box[1] + last.view_box[3] >= 20.0 + h - 1e-9,
            "the bottom is covered"
        );
        assert_eq!(plan.tiles[4].caption, "5/6 r2c2");
    }

    /// A chart a little larger than a sheet prints on one, reduced by less
    /// than [`MAX_SHRINK`], rather than on two with a strip on the second;
    /// one much larger keeps its size.
    #[test]
    fn a_chart_barely_larger_than_a_sheet_is_reduced_onto_it() {
        let tile_h_px = TILE_H_MM / MM_PER_PX;
        let plan = tile_plan(chart(400.0, tile_h_px * 1.06, 1.0), caption);
        assert_eq!(plan.sheets(), 1, "6 % over: reduced");
        let covered = plan.tiles[0].view_box[3];
        assert!(covered >= tile_h_px * 1.06 - 1e-6, "the whole height fits");
        let plan = tile_plan(chart(400.0, tile_h_px * 1.5, 1.0), caption);
        assert_eq!(plan.sheets(), 2, "50 % over: two sheets at screen size");
        assert!((plan.tiles[0].view_box[3] - tile_h_px).abs() < 1e-6);
    }

    /// Past the most sheets offered, only the count is planned.
    #[test]
    fn too_many_sheets_are_counted_not_planned() {
        let plan = tile_plan(chart(100_000.0, 100_000.0, 1.0), caption);
        assert!(plan.sheets() > MAX_SHEETS);
        assert!(plan.tiles.is_empty());
    }

    #[test]
    fn header_names_the_page_the_tree_and_the_day() {
        let i18n = I18n::new(Language::english());
        let day = NaiveDate::from_ymd_opt(2026, 9, 29).unwrap();
        let header = PrintHeader::new(&i18n, " Sample tree ", "Statistics", day);
        assert_eq!(header.title, "Statistics");
        assert_eq!(header.tree, "Sample tree");
        assert_eq!(header.printed_on, "Printed on 29 Sep 2026");
    }

    #[test]
    fn header_date_follows_the_reader_language() {
        let i18n = I18n::new(Language::try_from_code("fr").unwrap());
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
        let i18n = I18n::new(Language::english());
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
