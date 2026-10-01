//! The charts of the Statistics page, drawn as SVG: a donut, lines over
//! periods, an age pyramid, and the heat map of places over a basemap.
//!
//! Colors come from the theme through the `--chart-*` variables the page
//! defines from theme tokens (`docs/ui-statistics.md` §6).

use std::f64::consts::PI;

use dioxus::prelude::*;

use uuid::Uuid;

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

/// A labelled moment of a [`LineChart`]: a dashed line across the chart at
/// a period, with a numbered badge on top.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartMarker {
    pub index: usize,
    pub number: usize,
    /// What the badge stands for, shown on hover.
    pub label: String,
}

const WIDTH: f64 = 520.0;
const HEIGHT: f64 = 240.0;
const LEFT: f64 = 46.0;
const RIGHT: f64 = 12.0;
const TOP: f64 = 12.0;
const BOTTOM: f64 = 26.0;

/// Values over periods, one line per series, gaps where a period has none.
/// `periods` label the x-axis; `unit` follows each value on the axis and in
/// the hover label; `markers` flag moments at some periods.
#[component]
pub fn LineChart(
    periods: Vec<String>,
    series: Vec<ChartSeries>,
    unit: String,
    #[props(default)] markers: Vec<ChartMarker>,
) -> Element {
    let mut hovered = use_signal(|| None::<(usize, usize)>);
    let mut hovered_marker = use_signal(|| None::<usize>);
    let max = series
        .iter()
        .flat_map(|s| s.values.iter().flatten())
        .copied()
        .fold(0.0_f64, f64::max);
    let ticks = axis_ticks(max);
    let plot = Plot {
        count: periods.len().max(1),
        // Marker badges sit in a band of their own above the plot, clear of
        // the value axis labels.
        top: if markers.is_empty() {
            TOP
        } else {
            TOP + MARKER_BAND
        },
        max: ticks.last().copied().unwrap_or(1.0),
    };
    let lines = line_paths(&series, plot, series.len() <= FILLED_SERIES);
    let hover_text = hover_label(
        &periods,
        &series,
        &markers,
        &unit,
        hovered(),
        hovered_marker(),
    );
    rsx! {
        div { class: "stats-lines",
            svg {
                "viewBox": "0 0 {WIDTH} {HEIGHT}",
                class: "stats-lines-svg",
                role: "img",
                onmouseleave: move |_| {
                    hovered.set(None);
                    hovered_marker.set(None);
                },
                LineAxes { plot, ticks, periods: periods.clone() }
                for (s, (_, area, color)) in lines.iter().enumerate() {
                    if !area.is_empty() {
                        path { key: "a{s}", class: "stats-area", d: "{area}", style: "fill: {color}" }
                    }
                }
                for (k, marker) in markers.iter().enumerate() {
                    line {
                        key: "m{k}",
                        class: "stats-marker",
                        x1: "{plot.x(marker.index)}",
                        x2: "{plot.x(marker.index)}",
                        y1: "{TOP + MARKER_RADIUS}",
                        y2: "{HEIGHT - BOTTOM}",
                    }
                }
                for (s, (d, _, color)) in lines.iter().enumerate() {
                    path { key: "l{s}", class: "stats-line", d: "{d}", style: "stroke: {color}" }
                }
                LineMarkers { plot, markers: markers.clone(), hovered_marker }
                LinePoints { plot, series: series.clone(), hovered }
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

/// Where a [`LineChart`] draws: `count` periods across, values up to `max`
/// from the plot's `top` down to its base.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Plot {
    count: usize,
    top: f64,
    max: f64,
}

impl Plot {
    /// The abscissa of period `i`.
    fn x(self, i: usize) -> f64 {
        if self.count == 1 {
            LEFT + (WIDTH - LEFT - RIGHT) / 2.0
        } else {
            LEFT + (WIDTH - LEFT - RIGHT) * i as f64 / (self.count - 1) as f64
        }
    }

    /// The ordinate of value `v`.
    fn y(self, v: f64) -> f64 {
        self.top + (HEIGHT - self.top - BOTTOM) * (1.0 - v / self.max)
    }
}

/// Each series' curve, its filled area when `filled`, and its colour.
///
/// A series is drawn as its runs of consecutive values: a period without a
/// value breaks the curve rather than being drawn as zero.
fn line_paths(
    series: &[ChartSeries],
    plot: Plot,
    filled: bool,
) -> Vec<(String, String, &'static str)> {
    series
        .iter()
        .map(|s| {
            let runs = value_runs(&s.values, plot);
            let stroke: String = runs.iter().map(|run| curve(run)).collect();
            let base = plot.y(0.0);
            let area: String = runs
                .iter()
                .filter(|_| filled)
                .map(|run| {
                    let (first, last) = (run[0].0, run[run.len() - 1].0);
                    format!(
                        "{}L{last:.1} {base:.1} L{first:.1} {base:.1} Z ",
                        curve(run)
                    )
                })
                .collect();
            (stroke, area, s.color)
        })
        .collect()
}

/// The runs of two or more consecutive values, as points.
fn value_runs(values: &[Option<f64>], plot: Plot) -> Vec<Vec<(f64, f64)>> {
    let mut runs: Vec<Vec<(f64, f64)>> = vec![Vec::new()];
    for (i, value) in values.iter().enumerate() {
        match (value, runs.last_mut()) {
            (Some(v), Some(run)) => run.push((plot.x(i), plot.y(*v))),
            (None, Some(run)) if !run.is_empty() => runs.push(Vec::new()),
            _ => {}
        }
    }
    runs.retain(|run| run.len() > 1);
    runs
}

/// What the hovered marker, or else the hovered point, says.
fn hover_label(
    periods: &[String],
    series: &[ChartSeries],
    markers: &[ChartMarker],
    unit: &str,
    hovered: Option<(usize, usize)>,
    hovered_marker: Option<usize>,
) -> Option<String> {
    let marker_text = hovered_marker.and_then(|k| markers.get(k)).map(|marker| {
        format!(
            "{} · {}",
            periods.get(marker.index).cloned().unwrap_or_default(),
            marker.label
        )
    });
    marker_text.or_else(|| {
        let (s, i) = hovered?;
        let line = series.get(s)?;
        let value = line.values.get(i).copied().flatten()?;
        Some(format!(
            "{} · {}: {} {unit}",
            periods.get(i)?,
            line.label,
            format_value(value),
        ))
    })
}

/// The value grid with its labels, and the period labels, a few of them
/// when there are many.
#[component]
fn LineAxes(plot: Plot, ticks: Vec<f64>, periods: Vec<String>) -> Element {
    let label_every = (plot.count / 8).max(1);
    rsx! {
        for (k, tick) in ticks.iter().enumerate() {
            g { key: "t{k}",
                line {
                    class: "stats-grid-line",
                    x1: "{LEFT}",
                    x2: "{WIDTH - RIGHT}",
                    y1: "{plot.y(*tick)}",
                    y2: "{plot.y(*tick)}",
                }
                text { class: "stats-axis", x: "{LEFT - 6.0}", y: "{plot.y(*tick) + 4.0}", "text-anchor": "end",
                    "{format_value(*tick)}"
                }
            }
        }
        for (i, period) in periods.iter().enumerate().step_by(label_every) {
            text {
                key: "p{i}",
                // Every other label gives way on a phone, where
                // the axis text is drawn larger (see the CSS).
                class: if (i / label_every) % 2 == 1 { "stats-axis stats-axis-alt" } else { "stats-axis" },
                x: "{plot.x(i)}",
                y: "{HEIGHT - 8.0}",
                "text-anchor": "middle",
                "{period}"
            }
        }
    }
}

/// The numbered badges of the markers, each naming its moment on hover.
#[component]
fn LineMarkers(
    plot: Plot,
    markers: Vec<ChartMarker>,
    hovered_marker: Signal<Option<usize>>,
) -> Element {
    rsx! {
        for (k, marker) in markers.iter().enumerate() {
            g {
                key: "b{k}",
                class: "stats-marker-badge",
                onmouseenter: move |_| hovered_marker.set(Some(k)),
                onmouseleave: move |_| hovered_marker.set(None),
                circle {
                    cx: "{plot.x(marker.index)}",
                    cy: "{TOP}",
                    r: "{MARKER_RADIUS}",
                }
                text {
                    class: "stats-marker-text",
                    x: "{plot.x(marker.index)}",
                    y: "{TOP}",
                    "text-anchor": "middle",
                    "{marker.number}"
                }
            }
        }
    }
}

/// A point on every value, larger when hovered.
#[component]
fn LinePoints(
    plot: Plot,
    series: Vec<ChartSeries>,
    hovered: Signal<Option<(usize, usize)>>,
) -> Element {
    let points = series.iter().enumerate().flat_map(|(s, line)| {
        line.values
            .iter()
            .enumerate()
            .filter_map(move |(i, value)| Some((s, i, (*value)?, line.color)))
    });
    rsx! {
        for (s, i, v, color) in points {
            circle {
                key: "c{s}-{i}",
                class: "stats-point",
                cx: "{plot.x(i)}",
                cy: "{plot.y(v)}",
                r: if hovered() == Some((s, i)) { "4" } else { "2.2" },
                style: "fill: {color}",
                onmouseenter: move |_| hovered.set(Some((s, i))),
            }
        }
    }
}

/// The radius of a [`ChartMarker`]'s badge, in chart units.
const MARKER_RADIUS: f64 = 7.0;
/// The height of the band above the plot that holds the marker badges.
const MARKER_BAND: f64 = 20.0;

/// Charts with at most this many series are filled under their curves;
/// more would only cover one another.
const FILLED_SERIES: usize = 4;

/// A smooth curve through points ordered by x, as an SVG path: a monotone
/// cubic (Fritsch and Carlson), which never overshoots its points, so a
/// curve neither dips below zero nor invents a peak between two periods.
fn curve(points: &[(f64, f64)]) -> String {
    let n = points.len();
    let Some(&(x0, y0)) = points.first() else {
        return String::new();
    };
    let mut d = format!("M{x0:.1} {y0:.1} ");
    if n < 2 {
        return d;
    }
    let slopes: Vec<f64> = points
        .windows(2)
        .map(|w| (w[1].1 - w[0].1) / (w[1].0 - w[0].0))
        .collect();
    let mut tangents: Vec<f64> = (0..n)
        .map(|i| match i {
            0 => slopes[0],
            i if i == n - 1 => slopes[n - 2],
            i if slopes[i - 1] * slopes[i] <= 0.0 => 0.0,
            i => (slopes[i - 1] + slopes[i]) / 2.0,
        })
        .collect();
    for (i, slope) in slopes.iter().enumerate() {
        if *slope == 0.0 {
            tangents[i] = 0.0;
            tangents[i + 1] = 0.0;
            continue;
        }
        let (a, b) = (tangents[i] / slope, tangents[i + 1] / slope);
        let norm = a.hypot(b);
        if norm > 3.0 {
            tangents[i] = 3.0 / norm * a * slope;
            tangents[i + 1] = 3.0 / norm * b * slope;
        }
    }
    for i in 0..n - 1 {
        let ((xa, ya), (xb, yb)) = (points[i], points[i + 1]);
        let h = (xb - xa) / 3.0;
        d.push_str(&format!(
            "C{:.1} {:.1} {:.1} {:.1} {xb:.1} {yb:.1} ",
            xa + h,
            ya + tangents[i] * h,
            xb - h,
            yb - tangents[i + 1] * h,
        ));
    }
    d
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

/// A value with as few decimals as it needs, two at most: an axis step of
/// 0.25 must not read 0.2.
fn format_value(value: f64) -> String {
    if (value - value.round()).abs() < 0.005 {
        format!("{value:.0}")
    } else if (value * 10.0 - (value * 10.0).round()).abs() < 0.05 {
        format!("{value:.1}")
    } else {
        format!("{value:.2}")
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
    let alternate = move |year: i32| {
        year.div_euclid(interval)
            .div_euclid(every.max(1))
            .rem_euclid(2)
            == 1
    };
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
                        span {
                            // Every other label gives way on a phone.
                            class: if alternate(year) { "stats-ruler-label stats-ruler-label-alt" } else { "stats-ruler-label" },
                            "{year}"
                        }
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

/// A populated place ready to label: projected, named in the interface
/// language, with the zoom it is labelled from.
#[derive(Debug, Clone, PartialEq)]
pub struct MapCity {
    pub x: f64,
    pub y: f64,
    pub name: String,
    pub zoom: f64,
    pub population: i64,
}

/// The basemap's populated places named in `lang`, projected once, in the
/// order labels are placed: from the lowest zoom, then the most populated.
pub fn basemap_cities(countries: &[BasemapCountry], lang: &str) -> Vec<MapCity> {
    let mut cities: Vec<MapCity> = countries
        .iter()
        .flat_map(|country| &country.cities)
        .map(|city| {
            let (x, y) = project(f64::from(city.lon) / 10.0, f64::from(city.lat) / 10.0);
            let name = city
                .names
                .iter()
                .find(|n| n.lang == lang)
                .map_or(&city.name, |n| &n.name)
                .clone();
            MapCity {
                x,
                y,
                name,
                zoom: f64::from(city.zoom) / 10.0,
                population: city.population,
            }
        })
        .collect();
    cities.sort_by(|a, b| {
        a.zoom
            .total_cmp(&b.zoom)
            .then(b.population.cmp(&a.population))
    });
    cities
}

/// The most place names shown at once.
const MAX_CITY_LABELS: usize = 20;
/// Map units across the map's roughly 600 pixels, at web map zoom 0 (a
/// 256-pixel world 360 degrees wide): `log2(this / width)` is the zoom
/// level a view stands for.
const ZOOM_ZERO_WIDTH: f64 = 360.0 * 600.0 / 256.0;

/// The web map zoom level a view stands for.
fn zoom_level(view: View) -> f64 {
    (ZOOM_ZERO_WIDTH / view.width).log2()
}

/// A label's box: left, top, right, bottom in map units.
type LabelBox = (f64, f64, f64, f64);

fn overlaps(a: LabelBox, b: LabelBox) -> bool {
    a.0 < b.2 && b.0 < a.2 && a.1 < b.3 && b.1 < a.3
}

/// The places to name in a view, as the usual web maps do: those whose zoom
/// the view has reached, in order of importance, each only where its name
/// overlaps no name placed before it nor `obstacles` (the numbered
/// markers), and no more than [`MAX_CITY_LABELS`]. `font` is the label size
/// in map units; a name is written to the right of its place.
fn city_labels<'a>(
    cities: &'a [MapCity],
    view: View,
    font: f64,
    obstacles: &[LabelBox],
) -> Vec<&'a MapCity> {
    let zoom = zoom_level(view);
    let (half_width, half_height) = (view.width / 2.0, view.width * MAP_ASPECT / 2.0);
    let shown = (
        view.cx - half_width,
        view.cy - half_height,
        view.cx + half_width,
        view.cy + half_height,
    );
    let mut placed: Vec<LabelBox> = obstacles.to_vec();
    cities
        .iter()
        .take_while(|city| city.zoom <= zoom)
        .filter(|city| {
            let width = city.name.chars().count() as f64 * font * 0.55;
            let label = (
                city.x - font * 0.3,
                city.y - font * 0.8,
                city.x + font * 0.6 + width,
                city.y + font * 0.4,
            );
            // Whole within the view, and clear of every name placed.
            let inside = label.0 >= shown.0
                && label.1 >= shown.1
                && label.2 <= shown.2
                && label.3 <= shown.3;
            let free = inside && placed.iter().all(|other| !overlaps(label, *other));
            if free {
                placed.push(label);
            }
            free
        })
        .take(MAX_CITY_LABELS)
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
/// The widest view a place is focused at, in projected units (about a
/// degree and a half of longitude either side: the place and its region).
const FOCUS_WIDTH: f64 = 3.0;

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

    /// Centred on a place, zoomed in to its region unless the view is
    /// already closer.
    fn focused(self, latitude: f64, longitude: f64) -> Self {
        let (cx, cy) = project(longitude, latitude);
        Self {
            cx,
            cy,
            width: self.width.min(FOCUS_WIDTH),
        }
    }

    fn zoomed(self, factor: f64) -> Self {
        Self {
            width: (self.width * factor).clamp(0.5, 400.0),
            ..self
        }
    }
}

/// A place the map is focused on: its id, latitude and longitude.
pub type MapFocus = Option<(Uuid, f64, f64)>;

/// Where the tree's places are, as heat over the country outlines, with the
/// ten most used places numbered. Setting `focus` (from a numbered marker
/// here, or from the page's list of places) zooms the map onto that place;
/// the fit button clears it.
#[component]
pub fn HeatMap(
    paths: ReadSignal<Vec<String>>,
    cities: ReadSignal<Vec<MapCity>>,
    places: Vec<StatPlace>,
    top: Vec<StatPlace>,
    focus: Signal<MapFocus>,
    i18n: I18n,
) -> Element {
    let mut focus = focus;
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
    // Follows the focus only: the view is peeked, not read, so panning
    // and zooming do not bring it back.
    use_effect(move || {
        if let Some((_, latitude, longitude)) = focus() {
            let focused = view.peek().focused(latitude, longitude);
            view.set(focused);
        }
    });
    let current = view();
    let max = located.iter().map(|(_, _, n)| *n).max().unwrap_or(1).max(1) as f64;
    // Spots are sized against the view, so the heat reads the same at any
    // zoom: from 2% to 7% of the width.
    let radius = |count: i64| current.width * (0.02 + 0.05 * (count as f64 / max).sqrt());
    let markers: Vec<(usize, f64, f64, MapFocus)> = top
        .iter()
        .enumerate()
        .filter_map(|(i, p)| {
            let (latitude, longitude) = (p.latitude?, p.longitude?);
            let (x, y) = project(longitude, latitude);
            Some((i + 1, x, y, Some((p.place_id, latitude, longitude))))
        })
        .collect();
    let marker_radius = current.width * 0.012;
    let city_font = current.width * 0.016;
    let obstacles: Vec<LabelBox> = markers
        .iter()
        .map(|(_, x, y, _)| {
            (
                x - marker_radius,
                y - marker_radius,
                x + marker_radius,
                y + marker_radius,
            )
        })
        .collect();
    let labels: Vec<MapCity> = city_labels(&cities.read(), current, city_font, &obstacles)
        .into_iter()
        .cloned()
        .collect();
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
                g { class: "stats-map-cities",
                    for (index, city) in labels.iter().enumerate() {
                        g { key: "c{index}",
                            circle {
                                class: "stats-map-city-dot",
                                cx: "{city.x}",
                                cy: "{city.y}",
                                r: "{city_font * 0.2}",
                            }
                            text {
                                class: "stats-map-city",
                                x: "{city.x + city_font * 0.45}",
                                y: "{city.y + city_font * 0.35}",
                                "font-size": "{city_font}",
                                "{city.name}"
                            }
                        }
                    }
                }
                for (number, x, y, target) in markers.iter().copied() {
                    g {
                        key: "m{number}",
                        class: "stats-map-marker-group",
                        onclick: move |_| focus.set(target),
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
                    onclick: move |_| {
                        focus.set(None);
                        view.set(fit);
                    },
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
    fn axis_values_keep_the_decimals_they_need() {
        assert_eq!(format_value(0.25), "0.25");
        assert_eq!(format_value(0.5), "0.5");
        assert_eq!(format_value(40.0), "40");
    }

    /// The control points of each cubic of a path, as (x, y) pairs.
    fn controls(path: &str) -> Vec<(f64, f64)> {
        path.split('C')
            .skip(1)
            .flat_map(|segment| {
                let n: Vec<f64> = segment
                    .split_whitespace()
                    .filter_map(|v| v.parse().ok())
                    .collect();
                [(n[0], n[1]), (n[2], n[3])]
            })
            .collect()
    }

    #[test]
    fn a_curve_never_overshoots_its_points() {
        // A plateau then a rise: the curve stays flat on the plateau and
        // within the values around each step.
        let points = [(0.0, 50.0), (10.0, 50.0), (20.0, 10.0), (30.0, 12.0)];
        let path = curve(&points);
        assert!(path.starts_with("M0.0 50.0 C"));
        for (_, y) in controls(&path) {
            assert!((10.0..=50.0).contains(&y), "{y} overshoots");
        }
        // Flat between the two equal points.
        assert_eq!(controls(&path)[0].1, 50.0);
        assert_eq!(controls(&path)[1].1, 50.0);
    }

    #[test]
    fn the_equator_projects_to_zero_and_the_north_upwards() {
        let (x, y) = project(10.0, 0.0);
        assert_eq!(x, 10.0);
        assert!(y.abs() < 1e-9);
        assert!(project(0.0, 60.0).1 < project(0.0, 30.0).1);
    }

    #[test]
    fn focusing_centres_on_the_place_and_never_zooms_out() {
        let wide = View {
            cx: 0.0,
            cy: 0.0,
            width: 60.0,
        };
        let focused = wide.focused(0.0, 10.0);
        assert_eq!((focused.cx, focused.width), (10.0, FOCUS_WIDTH));
        assert!(focused.cy.abs() < 1e-9);
        let close = View { width: 1.0, ..wide };
        assert_eq!(close.focused(0.0, 10.0).width, 1.0);
    }

    fn city(name: &str, x: f64, y: f64, zoom: f64) -> MapCity {
        MapCity {
            x,
            y,
            name: name.to_string(),
            zoom,
            population: 0,
        }
    }

    #[test]
    fn places_are_named_from_their_zoom_without_overlapping() {
        let view = View {
            cx: 0.0,
            cy: 0.0,
            width: 20.0,
        };
        // A view 20 units wide stands for zoom 5.4.
        assert!((zoom_level(view) - 5.4).abs() < 0.1);
        let cities = [
            city("City A", 0.0, 0.0, 2.0),
            // Right under the first label: left out.
            city("City B", 0.5, 0.1, 4.0),
            city("City C", 0.0, 3.0, 5.0),
            // Outside the view, and at its edge, where its name would be cut.
            city("City D", 30.0, 0.0, 5.0),
            city("City F", 9.9, 3.0, 5.0),
            // Not yet: its zoom is closer than the view.
            city("City E", 0.0, -3.0, 7.0),
        ];
        let named: Vec<&str> = city_labels(&cities, view, 0.3, &[])
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(named, ["City A", "City C"]);
        // A numbered marker keeps its place.
        let marker = (-0.2, -0.2, 0.2, 0.2);
        let named: Vec<&str> = city_labels(&cities, view, 0.3, &[marker])
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(named, ["City B", "City C"]);
    }

    #[test]
    fn the_fitting_view_holds_every_point() {
        let view = View::fitting(&[(0.0, -50.0), (10.0, -60.0)]);
        assert!(view.width >= 13.0);
        assert_eq!((view.cx, view.cy), (5.0, -55.0));
    }
}
