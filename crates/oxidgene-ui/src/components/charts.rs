//! The charts of the Statistics page, drawn as SVG: a donut, lines over
//! periods, an age pyramid, and the heat map of places over a basemap.
//!
//! Colors come from the theme through the `--chart-*` variables the page
//! defines from theme tokens (`docs/ui-statistics.md` §6).

use std::f64::consts::PI;

use dioxus::prelude::*;

use crate::api::{BasemapCountry, StatPlace, StatPyramidBand};
use crate::i18n::I18n;

/// The chart palette, as CSS variables set on `.stats-page`.
pub const PALETTE: [&str; 12] = [
    "var(--chart-1)",
    "var(--chart-2)",
    "var(--chart-3)",
    "var(--chart-4)",
    "var(--chart-5)",
    "var(--chart-6)",
    "var(--chart-7)",
    "var(--chart-8)",
    "var(--chart-9)",
    "var(--chart-10)",
    "var(--chart-11)",
    "var(--chart-12)",
];

/// A titled chart frame with its hint, or the empty state.
#[component]
pub fn ChartCard(
    title: String,
    hint: String,
    empty: bool,
    i18n: I18n,
    children: Element,
) -> Element {
    rsx! {
        section { class: "stats-card",
            h3 { class: "stats-card-title",
                "{title}"
                span { class: "stats-hint", title: "{hint}", "aria-label": "{hint}", "?" }
            }
            if empty {
                p { class: "stats-empty", {i18n.t("stats.empty")} }
            } else {
                {children}
            }
        }
    }
}

/// Ten labelled counts as a ring, with a legend.
#[component]
pub fn DonutChart(items: Vec<(String, i64)>) -> Element {
    let total: i64 = items.iter().map(|(_, n)| n).sum();
    let mut start = -PI / 2.0;
    let arcs: Vec<(String, &str)> = items
        .iter()
        .enumerate()
        .map(|(i, (_, n))| {
            let sweep = if total > 0 {
                *n as f64 / total as f64 * 2.0 * PI
            } else {
                0.0
            };
            let path = arc(start, (start + sweep).min(start + 2.0 * PI - 1e-4));
            start += sweep;
            (path, PALETTE[i % PALETTE.len()])
        })
        .collect();
    rsx! {
        div { class: "stats-donut",
            svg { "viewBox": "-110 -110 220 220", class: "stats-donut-svg", role: "img",
                for (index, (path, color)) in arcs.iter().enumerate() {
                    path { key: "{index}", d: "{path}", style: "fill: {color}" }
                }
            }
            ul { class: "stats-legend",
                for (index, (label, count)) in items.iter().enumerate() {
                    li { key: "{index}",
                        span { class: "stats-swatch", style: "background: {PALETTE[index % PALETTE.len()]}" }
                        span { class: "stats-legend-label", "{label}" }
                        span { class: "stats-legend-count", "{count}" }
                    }
                }
            }
        }
    }
}

/// A ring segment between two angles, outer radius 100, inner 60.
fn arc(from: f64, to: f64) -> String {
    let (outer, inner) = (100.0, 60.0);
    let point = |r: f64, a: f64| (r * a.cos(), r * a.sin());
    let large = i32::from(to - from > PI);
    let (x0, y0) = point(outer, from);
    let (x1, y1) = point(outer, to);
    let (x2, y2) = point(inner, to);
    let (x3, y3) = point(inner, from);
    format!(
        "M{x0:.2} {y0:.2} A{outer} {outer} 0 {large} 1 {x1:.2} {y1:.2} L{x2:.2} {y2:.2} A{inner} {inner} 0 {large} 0 {x3:.2} {y3:.2} Z"
    )
}

/// One line of a [`LineChart`].
#[derive(Debug, Clone, PartialEq)]
pub struct ChartSeries {
    pub label: String,
    pub color: &'static str,
    pub values: Vec<Option<f64>>,
}

const WIDTH: f64 = 520.0;
const HEIGHT: f64 = 240.0;
const LEFT: f64 = 46.0;
const RIGHT: f64 = 12.0;
const TOP: f64 = 12.0;
const BOTTOM: f64 = 26.0;

/// Values over periods, one line per series, gaps where a period has none.
/// `unit` follows each value on the axis and in the hover label.
#[component]
pub fn LineChart(periods: Vec<i32>, series: Vec<ChartSeries>, unit: String) -> Element {
    let mut hovered = use_signal(|| None::<(usize, usize)>);
    let max = series
        .iter()
        .flat_map(|s| s.values.iter().flatten())
        .copied()
        .fold(0.0_f64, f64::max);
    let ticks = axis_ticks(max);
    let top = ticks.last().copied().unwrap_or(1.0);
    let count = periods.len().max(1);
    let x = |i: usize| {
        if count == 1 {
            LEFT + (WIDTH - LEFT - RIGHT) / 2.0
        } else {
            LEFT + (WIDTH - LEFT - RIGHT) * i as f64 / (count - 1) as f64
        }
    };
    let y = |v: f64| TOP + (HEIGHT - TOP - BOTTOM) * (1.0 - v / top);
    let label_every = (count / 8).max(1);
    let lines: Vec<(String, &'static str)> = series
        .iter()
        .map(|s| {
            let mut d = String::new();
            let mut pen_down = false;
            for (i, value) in s.values.iter().enumerate() {
                match value {
                    Some(v) => {
                        d.push_str(&format!(
                            "{}{:.1} {:.1} ",
                            if pen_down { "L" } else { "M" },
                            x(i),
                            y(*v)
                        ));
                        pen_down = true;
                    }
                    None => pen_down = false,
                }
            }
            (d, s.color)
        })
        .collect();
    let hover_text = hovered().and_then(|(s, i)| {
        let value = series.get(s)?.values.get(i).copied().flatten()?;
        Some(format!(
            "{} · {}: {} {}",
            periods.get(i)?,
            series[s].label,
            format_value(value),
            unit
        ))
    });
    rsx! {
        div { class: "stats-lines",
            svg {
                "viewBox": "0 0 {WIDTH} {HEIGHT}",
                class: "stats-lines-svg",
                role: "img",
                onmouseleave: move |_| hovered.set(None),
                for (k, tick) in ticks.iter().enumerate() {
                    g { key: "t{k}",
                        line {
                            class: "stats-grid",
                            x1: "{LEFT}",
                            x2: "{WIDTH - RIGHT}",
                            y1: "{y(*tick)}",
                            y2: "{y(*tick)}",
                        }
                        text { class: "stats-axis", x: "{LEFT - 6.0}", y: "{y(*tick) + 4.0}", "text-anchor": "end",
                            "{format_value(*tick)}"
                        }
                    }
                }
                for (i, period) in periods.iter().enumerate() {
                    if i % label_every == 0 {
                        text {
                            key: "p{i}",
                            class: "stats-axis",
                            x: "{x(i)}",
                            y: "{HEIGHT - 8.0}",
                            "text-anchor": "middle",
                            "{period}"
                        }
                    }
                }
                for (s, (d, color)) in lines.iter().enumerate() {
                    path { key: "l{s}", class: "stats-line", d: "{d}", style: "stroke: {color}" }
                }
                for (s, line) in series.iter().enumerate() {
                    for (i, value) in line.values.iter().enumerate() {
                        if let Some(v) = value {
                            circle {
                                key: "c{s}-{i}",
                                class: "stats-point",
                                cx: "{x(i)}",
                                cy: "{y(*v)}",
                                r: if hovered() == Some((s, i)) { "4" } else { "2.2" },
                                style: "fill: {line.color}",
                                onmouseenter: move |_| hovered.set(Some((s, i))),
                            }
                        }
                    }
                }
            }
            p { class: "stats-hover", "{hover_text.clone().unwrap_or_default()}" }
            // A single line is already named by the card title.
            if series.len() > 1 {
                ul { class: "stats-legend stats-legend-inline",
                    for (index, line) in series.iter().enumerate() {
                    li { key: "{index}",
                        span { class: "stats-swatch", style: "background: {line.color}" }
                            span { class: "stats-legend-label", "{line.label}" }
                        }
                    }
                }
            }
        }
    }
}

/// The ticks of a value axis, from 0 to at least `max` in at most four
/// round steps: 1, 2, 2.5 or 5 times a power of ten.
fn axis_ticks(max: f64) -> Vec<f64> {
    let max = if max > 0.0 { max } else { 1.0 };
    let magnitude = 10_f64.powf((max / 4.0).log10().floor());
    let step = [1.0, 2.0, 2.5, 5.0, 10.0]
        .into_iter()
        .map(|step| step * magnitude)
        .find(|step| step * 4.0 >= max)
        .unwrap_or(10.0 * magnitude);
    let count = (max / step - 1e-9).ceil() as i32;
    (0..=count).map(|k| step * f64::from(k)).collect()
}

fn format_value(value: f64) -> String {
    if (value - value.round()).abs() < 0.05 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    }
}

/// Labelled counts as horizontal bars, in the order given.
#[component]
pub fn BarChart(items: Vec<(String, i64)>) -> Element {
    let max = items.iter().map(|(_, n)| *n).max().unwrap_or(1).max(1) as f64;
    rsx! {
        div { class: "stats-bars",
            for (index, (label, count)) in items.iter().enumerate() {
                div { key: "{index}", class: "stats-bar-row",
                    span { class: "stats-bar-label", "{label}" }
                    span { class: "stats-bar-track",
                        span {
                            class: "stats-bar",
                            style: "width: {*count as f64 / max * 100.0}%",
                        }
                    }
                    span { class: "stats-legend-count", "{count}" }
                }
            }
        }
    }
}

/// The most year labels a [`YearRuler`] prints; ticks beyond are unlabelled.
const RULER_LABELS: i32 = 12;
/// The width of a [`YearRuler`] handle, in pixels (`.stats-ruler-input`).
/// A native slider's handle travels from half its width to the width minus
/// half, so the ticks and the range are laid out on that span.
const RULER_THUMB: f64 = 16.0;

/// Where a share (0 to 100) of the ruler falls, as a CSS length.
fn place(percent: f64) -> String {
    format!(
        "calc({half}px + (100% - {RULER_THUMB}px) * {share})",
        half = RULER_THUMB / 2.0,
        share = percent / 100.0
    )
}

/// A timeline from `min` to `max` with two handles choosing the first and
/// the last year shown, to the year. Ticks mark the multiples of `interval`,
/// where the periods start. `on_change` receives `(from, to)`, never crossed.
#[component]
pub fn YearRuler(
    min: i32,
    max: i32,
    from: i32,
    to: i32,
    interval: i32,
    on_change: EventHandler<(i32, i32)>,
    from_label: String,
    to_label: String,
) -> Element {
    let span = f64::from((max - min).max(1));
    let at = move |year: i32| f64::from(year - min) / span * 100.0;
    let first_tick = min.div_euclid(interval) * interval
        + if min.rem_euclid(interval) == 0 {
            0
        } else {
            interval
        };
    let ticks: Vec<i32> = (first_tick..=max).step_by(interval as usize).collect();
    // Label every n-th multiple of the interval, n chosen so the labels fit
    // and stay on the same years whatever the range.
    let every = (ticks.len() as i32 + RULER_LABELS - 1) / RULER_LABELS;
    let labelled = move |year: i32| year.div_euclid(interval).rem_euclid(every.max(1)) == 0;
    // The start handle goes on top once it nears the end, so both handles
    // stay reachable when they meet there.
    let from_on_top = from > min + (max - min) / 2;
    rsx! {
        div { class: "stats-ruler",
            div { class: "stats-ruler-track" }
            div {
                class: "stats-ruler-range",
                style: "left: {place(at(from))}; width: calc((100% - {RULER_THUMB}px) * {(at(to) - at(from)) / 100.0})",
            }
            for year in ticks {
                div {
                    key: "{year}",
                    class: if labelled(year) { "stats-ruler-tick major" } else { "stats-ruler-tick" },
                    style: "left: {place(at(year))}",
                    if labelled(year) {
                        span { class: "stats-ruler-label", "{year}" }
                    }
                }
            }
            input {
                r#type: "range",
                class: "stats-ruler-input",
                style: if from_on_top { "z-index: 3" } else { "" },
                min: "{min}",
                max: "{max}",
                step: "1",
                value: "{from}",
                "aria-label": "{from_label}",
                oninput: move |e: Event<FormData>| {
                    if let Ok(year) = e.value().parse::<i32>() {
                        on_change.call((year.min(to), to));
                    }
                },
            }
            input {
                r#type: "range",
                class: "stats-ruler-input",
                min: "{min}",
                max: "{max}",
                step: "1",
                value: "{to}",
                "aria-label": "{to_label}",
                oninput: move |e: Event<FormData>| {
                    if let Ok(year) = e.value().parse::<i32>() {
                        on_change.call((from, year.max(from)));
                    }
                },
            }
        }
    }
}

/// Persons by age at death, men to the left and women to the right.
#[component]
pub fn Pyramid(bands: Vec<StatPyramidBand>, men: String, women: String) -> Element {
    let max = bands
        .iter()
        .map(|b| b.men.max(b.women))
        .max()
        .unwrap_or(1)
        .max(1) as f64;
    let mut rows = bands.clone();
    rows.sort_by_key(|band| std::cmp::Reverse(band.from));
    rsx! {
        div { class: "stats-pyramid",
            div { class: "stats-pyramid-head",
                span { "{men}" }
                span {}
                span { "{women}" }
            }
            for band in rows.iter() {
                div { key: "{band.from}", class: "stats-pyramid-row",
                    div { class: "stats-pyramid-side stats-pyramid-men",
                        span { class: "stats-pyramid-count", "{band.men}" }
                        span {
                            class: "stats-pyramid-bar",
                            style: "width: {band.men as f64 / max * 100.0}%; background: var(--pn-male-line)",
                        }
                    }
                    span { class: "stats-pyramid-age", "{band.from}–{band.from + 4}" }
                    div { class: "stats-pyramid-side stats-pyramid-women",
                        span {
                            class: "stats-pyramid-bar",
                            style: "width: {band.women as f64 / max * 100.0}%; background: var(--pn-female-line)",
                        }
                        span { class: "stats-pyramid-count", "{band.women}" }
                    }
                }
            }
        }
    }
}

// ── Heat map ────────────────────────────────────────────────────────────

/// Mercator projection in degree-like units: x is the longitude, y grows
/// southward so it can be used as an SVG coordinate.
fn project(longitude: f64, latitude: f64) -> (f64, f64) {
    let latitude = latitude.clamp(-85.0, 85.0).to_radians();
    let y = (PI / 4.0 + latitude / 2.0).tan().ln().to_degrees();
    (longitude, -y)
}

/// The basemap's rings as SVG path data, projected once.
pub fn basemap_paths(countries: &[BasemapCountry]) -> Vec<String> {
    countries
        .iter()
        .map(|country| {
            let mut d = String::new();
            for ring in &country.rings {
                for (k, [lon, lat]) in ring.as_chunks::<2>().0.iter().enumerate() {
                    let (x, y) = project(f64::from(*lon) / 10.0, f64::from(*lat) / 10.0);
                    d.push_str(&format!("{}{x:.2} {y:.2}", if k == 0 { "M" } else { "L" }));
                }
                d.push('Z');
            }
            d
        })
        .collect()
}

/// The part of the map shown: its centre and width, in projected units, at
/// the map's fixed aspect ratio.
#[derive(Debug, Clone, Copy, PartialEq)]
struct View {
    cx: f64,
    cy: f64,
    width: f64,
}

const MAP_ASPECT: f64 = 0.62;

impl View {
    fn view_box(self) -> String {
        let height = self.width * MAP_ASPECT;
        format!(
            "{:.3} {:.3} {:.3} {:.3}",
            self.cx - self.width / 2.0,
            self.cy - height / 2.0,
            self.width,
            height
        )
    }

    /// The view that shows every point, with a margin.
    fn fitting(points: &[(f64, f64)]) -> Self {
        if points.is_empty() {
            return Self {
                cx: 5.0,
                cy: -55.0,
                width: 60.0,
            };
        }
        let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for (x, y) in points {
            x0 = x0.min(*x);
            x1 = x1.max(*x);
            y0 = y0.min(*y);
            y1 = y1.max(*y);
        }
        let width = ((x1 - x0) * 1.3).max((y1 - y0) * 1.3 / MAP_ASPECT).max(4.0);
        Self {
            cx: (x0 + x1) / 2.0,
            cy: (y0 + y1) / 2.0,
            width,
        }
    }

    fn zoomed(self, factor: f64) -> Self {
        Self {
            width: (self.width * factor).clamp(0.5, 400.0),
            ..self
        }
    }
}

/// Where the tree's places are, as heat over the country outlines, with the
/// ten most used places numbered.
#[component]
pub fn HeatMap(
    paths: ReadSignal<Vec<String>>,
    places: Vec<StatPlace>,
    top: Vec<StatPlace>,
    i18n: I18n,
) -> Element {
    let located: Vec<(f64, f64, i64)> = places
        .iter()
        .filter_map(|p| {
            let (x, y) = project(p.longitude?, p.latitude?);
            Some((x, y, p.count))
        })
        .collect();
    let fit = View::fitting(&located.iter().map(|(x, y, _)| (*x, *y)).collect::<Vec<_>>());
    let mut view = use_signal(|| fit);
    let mut drag = use_signal(|| None::<(f64, f64, View)>);
    let current = view();
    let max = located.iter().map(|(_, _, n)| *n).max().unwrap_or(1).max(1) as f64;
    // Spots are sized against the view, so the heat reads the same at any
    // zoom: from 2% to 7% of the width.
    let radius = |count: i64| current.width * (0.02 + 0.05 * (count as f64 / max).sqrt());
    let markers: Vec<(usize, f64, f64)> = top
        .iter()
        .enumerate()
        .filter_map(|(i, p)| {
            let (x, y) = project(p.longitude?, p.latitude?);
            Some((i + 1, x, y))
        })
        .collect();
    let marker_radius = current.width * 0.012;
    rsx! {
        div { class: "stats-map",
            svg {
                class: "stats-map-svg",
                "viewBox": "{current.view_box()}",
                "preserveAspectRatio": "xMidYMid meet",
                role: "img",
                "aria-label": "{i18n.t(\"stats.map_title\")}",
                onwheel: move |e: Event<WheelData>| {
                    e.prevent_default();
                    let factor = if e.delta().strip_units().y > 0.0 { 1.2 } else { 1.0 / 1.2 };
                    view.set(view().zoomed(factor));
                },
                onmousedown: move |e: Event<MouseData>| {
                    let point = e.client_coordinates();
                    drag.set(Some((point.x, point.y, view())));
                },
                onmousemove: move |e: Event<MouseData>| {
                    if let Some((x0, y0, start)) = drag() {
                        let point = e.client_coordinates();
                        // One screen pixel is roughly this many map units on
                        // a map about 600px wide.
                        let scale = start.width / 600.0;
                        view.set(View {
                            cx: start.cx - (point.x - x0) * scale,
                            cy: start.cy - (point.y - y0) * scale,
                            ..start
                        });
                    }
                },
                onmouseup: move |_| drag.set(None),
                onmouseleave: move |_| drag.set(None),
                defs {
                    radialGradient { id: "stats-heat",
                        stop { offset: "0%", style: "stop-color: var(--red); stop-opacity: 0.55" }
                        stop { offset: "45%", style: "stop-color: var(--orange); stop-opacity: 0.3" }
                        stop { offset: "100%", style: "stop-color: var(--orange); stop-opacity: 0" }
                    }
                }
                for (index, d) in paths.read().iter().enumerate() {
                    path { key: "{index}", class: "stats-map-land", d: "{d}" }
                }
                for (index, (x, y, count)) in located.iter().enumerate() {
                    circle {
                        key: "h{index}",
                        cx: "{x}",
                        cy: "{y}",
                        r: "{radius(*count)}",
                        fill: "url(#stats-heat)",
                    }
                }
                for (number, x, y) in markers.iter() {
                    g { key: "m{number}",
                        circle {
                            class: "stats-map-marker",
                            cx: "{x}",
                            cy: "{y}",
                            r: "{marker_radius}",
                        }
                        text {
                            class: "stats-map-marker-text",
                            x: "{x}",
                            y: "{y + marker_radius * 0.4}",
                            "font-size": "{marker_radius * 1.2}",
                            "text-anchor": "middle",
                            "{number}"
                        }
                    }
                }
            }
            div { class: "stats-map-controls",
                button {
                    class: "isb-btn",
                    title: "{i18n.t(\"stats.zoom_in\")}",
                    onclick: move |_| view.set(view().zoomed(1.0 / 1.5)),
                    "+"
                }
                button {
                    class: "isb-btn",
                    title: "{i18n.t(\"stats.zoom_out\")}",
                    onclick: move |_| view.set(view().zoomed(1.5)),
                    "−"
                }
                button {
                    class: "isb-btn",
                    title: "{i18n.t(\"stats.zoom_fit\")}",
                    onclick: move |_| view.set(fit),
                    "⤢"
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axes_end_on_round_numbers() {
        assert_eq!(axis_ticks(0.0), [0.0, 0.25, 0.5, 0.75, 1.0]);
        assert_eq!(axis_ticks(37.0), [0.0, 10.0, 20.0, 30.0, 40.0]);
        assert_eq!(axis_ticks(82.0), [0.0, 25.0, 50.0, 75.0, 100.0]);
        assert_eq!(axis_ticks(3.3), [0.0, 1.0, 2.0, 3.0, 4.0]);
        assert_eq!(axis_ticks(40.0), [0.0, 10.0, 20.0, 30.0, 40.0]);
    }

    #[test]
    fn the_equator_projects_to_zero_and_the_north_upwards() {
        let (x, y) = project(10.0, 0.0);
        assert_eq!(x, 10.0);
        assert!(y.abs() < 1e-9);
        assert!(project(0.0, 60.0).1 < project(0.0, 30.0).1);
    }

    #[test]
    fn the_fitting_view_holds_every_point() {
        let view = View::fitting(&[(0.0, -50.0), (10.0, -60.0)]);
        assert!(view.width >= 13.0);
        assert_eq!((view.cx, view.cy), (5.0, -55.0));
    }
}
