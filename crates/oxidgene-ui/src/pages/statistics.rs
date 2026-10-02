//! Statistics page: an overview of the tree, where its events happened, its
//! names, demographic charts per period, its records and notable lists, and
//! how many persons it held over the days it was worked on.
//! See `docs/ui-statistics.md`.

use std::collections::BTreeMap;

use chrono::{Datelike, Months, NaiveDate, Utc};
use dioxus::prelude::*;
use oxidgene_core::EventType;
use oxidgene_core::calendar::{DAYS_PER_MONTH, DAYS_PER_YEAR};
use uuid::Uuid;

use crate::api::{
    ApiClient, GrowthDay, StatCount, StatDate, StatPerson, StatPersonRef, StatRecord, StatSummary,
    StatYearCounts, StatYearSum, TreeGrowth, TreeStatistics,
};
use crate::components::charts::{
    BarChart, ChartCard, ChartMarker, ChartSeries, DonutChart, HeatMap, LineChart, MapCity,
    MapFocus, PALETTE, Pyramid, YearRuler, basemap_cities, basemap_paths,
};
use crate::components::date_input::{format_date, format_day};
use crate::components::history_diff::format_timestamp;
use crate::components::tabs::{Tab, Tabs, use_stored_tab};
use crate::components::tree_page::{ToolPageFrame, use_tree_page};
use crate::i18n::{I18n, Language, use_i18n};
use crate::prefs::{store, stored};
use crate::router::Route;
use crate::ui_observability::{UiPage, measure_ui, use_traced_resource, use_ui_load_trace};
use crate::utils::event_type_label_key;

/// The period widths offered, in years.
const INTERVALS: [i32; 4] = [10, 25, 50, 100];
const DEFAULT_INTERVAL: i32 = 25;
const INTERVAL_STORAGE_KEY: &str = "oxidgene-stats-interval";
const APPROXIMATE_STORAGE_KEY: &str = "oxidgene-stats-approximate";
/// A period's average, share or ratio is shown only when it rests on at
/// least this many values: fewer make noise, not a trend.
const MIN_VALUES: i64 = 10;
/// The first view of the period charts starts at the first span of this
/// many years that holds at least `DENSE_SHARE` of the tree's dated events,
/// so a few early records do not stretch the axis over empty centuries.
const DENSE_SPAN: i32 = 25;
const DENSE_SHARE: f64 = 0.01;

const TAB_STORAGE_KEY: &str = "oxidgene-stats-tab";

/// The page's tabs, one per kind of statistics.
#[derive(Clone, Copy, PartialEq, Eq)]
enum StatsTab {
    Overview,
    Population,
    Families,
    Places,
    Names,
    Records,
    Growth,
}

impl Tab for StatsTab {
    const ALL: &'static [Self] = &[
        Self::Overview,
        Self::Population,
        Self::Families,
        Self::Places,
        Self::Names,
        Self::Records,
        Self::Growth,
    ];

    /// The tab's name in storage and in its label's i18n key.
    fn key(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Population => "population",
            Self::Families => "families",
            Self::Places => "places",
            Self::Names => "names",
            Self::Records => "records",
            Self::Growth => "growth",
        }
    }
}

/// The notable lists of the Records tab.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ListTab {
    Births,
    Unions,
    Deaths,
    OldestAlive,
    LongestLives,
    LargestFamilies,
}

/// Marks a request as wanted, once: a later tab asking again changes
/// nothing, and does not ask it again.
fn want_once(mut wanted: Signal<bool>) {
    if !*wanted.peek() {
        wanted.set(true);
    }
}

/// Restores the viewer's interval and dates option.
async fn restore_choices(mut interval: Signal<i32>, mut approximate: Signal<Option<bool>>) {
    if let Some(value) = stored(INTERVAL_STORAGE_KEY)
        .await
        .and_then(|s| s.parse::<i32>().ok())
        .filter(|v| INTERVALS.contains(v))
    {
        interval.set(value);
    }
    let chosen = stored(APPROXIMATE_STORAGE_KEY).await.as_deref() == Some("true");
    approximate.set(Some(chosen));
}

#[component]
pub fn Statistics(tree_id: String) -> Element {
    let i18n = use_i18n();
    let language: Signal<Language> = use_context();
    let api = use_context::<ApiClient>();
    let load_trace = use_ui_load_trace(UiPage::Statistics);
    let page = use_tree_page(&tree_id);
    let tid = tree_id.parse::<Uuid>().ok();

    let (tab, choose_tab) = use_stored_tab::<StatsTab>(TAB_STORAGE_KEY);
    let interval = use_signal(|| DEFAULT_INTERVAL);
    // The years the period charts cover, shared by the two tabs that show
    // them; `None` until the viewer moves the ruler.
    let range = use_signal(|| None::<(i32, i32)>);
    // Unknown until the browser answers, so the statistics are asked once,
    // with the viewer's own choice.
    let mut approximate = use_signal(|| None::<bool>);
    // Which of the two requests the tabs shown so far need: the Growth tab
    // has its own, every other tab shares the statistics. Each is asked the
    // first time a tab needs it, never before.
    let statistics_wanted = use_signal(|| false);
    let growth_wanted = use_signal(|| false);
    // The base map is drawn by the Places tab alone: 1.6 MB of outlines
    // nobody else needs.
    let basemap_wanted = use_signal(|| false);
    let want = move |value: StatsTab| {
        let wanted = if value == StatsTab::Growth {
            growth_wanted
        } else {
            statistics_wanted
        };
        want_once(wanted);
        if value == StatsTab::Places {
            want_once(basemap_wanted);
        }
    };
    use_effect(move || {
        spawn(restore_choices(interval, approximate));
    });
    // The tab shown asks for its data the first time it shows.
    use_effect(move || {
        if let Some(value) = tab() {
            want(value);
        }
    });
    let mut choose_approximate = move |value: bool| {
        approximate.set(Some(value));
        store(APPROXIMATE_STORAGE_KEY, &value.to_string());
    };
    let list = use_signal(|| ListTab::Births);
    let map_focus = use_signal(|| None);

    // The series come by year: the interval and the range of years only
    // regroup them here. Only the dates option and the language, which
    // names the places' countries, ask again.
    let api_stats = api.clone();
    let stats = use_traced_resource(load_trace.clone(), "statistics", move || {
        let api = api_stats.clone();
        let approximate = approximate();
        let lang = language().reference_code();
        let wanted = statistics_wanted();
        async move {
            if !wanted {
                return None;
            }
            api.tree_statistics(tid?, approximate?, lang).await.ok()
        }
    });
    let api_growth = api.clone();
    let growth = use_traced_resource(load_trace.clone(), "growth", move || {
        let api = api_growth.clone();
        let wanted = growth_wanted();
        async move {
            if !wanted {
                return None;
            }
            api.tree_growth(tid?).await.ok()
        }
    });
    let api_map = api.clone();
    let basemap = use_traced_resource(load_trace.clone(), "basemap", move || {
        let api = api_map.clone();
        let wanted = basemap_wanted();
        async move {
            if !wanted {
                return Vec::new();
            }
            api.basemap().await.unwrap_or_default()
        }
    });
    let paths_trace = load_trace.clone();
    let paths = use_memo(move || {
        paths_trace.measure("statistics_basemap_paths", || {
            basemap
                .read()
                .as_ref()
                .map(|countries| basemap_paths(countries))
                .unwrap_or_default()
        })
    });
    let cities = use_memo(move || {
        load_trace.measure("statistics_basemap_cities", || {
            basemap
                .read()
                .as_ref()
                .map(|countries| basemap_cities(countries, language().reference_code()))
                .unwrap_or_default()
        })
    });

    let stats_read = stats.read();
    let stats_value = stats_read.as_ref().and_then(Option::as_ref);
    let growth_read = growth.read();
    let growth_value = growth_read.as_ref().and_then(Option::as_ref);

    rsx! {
        ToolPageFrame {
            tree_id: tree_id.clone(),
            tree_name: page.name(),
            title: i18n.t("stats.breadcrumb"),
            selected_person_id: page.selected_person_id,
            page_class: "stats-page",
            content_class: "stats-content",
            topbar: rsx! {
                label {
                    class: "stats-option",
                    title: "{i18n.t(\"stats.approximate_hint\")}",
                    input {
                        r#type: "checkbox",
                        checked: approximate().unwrap_or(false),
                        disabled: approximate().is_none(),
                        onchange: move |e: Event<FormData>| choose_approximate(e.checked()),
                    }
                    {i18n.t("stats.approximate")}
                }
            },
            // The tabs show once the viewer's stored tab is known.
            if approximate().is_some() {
                Tabs {
                    tabs: StatsTab::ALL
                        .iter()
                        .map(|choice| (*choice, i18n.t(&format!("stats.tab.{}", choice.key()))))
                        .collect::<Vec<_>>(),
                    current: tab(),
                    on_select: choose_tab,
                    class: "stats-tabs",
                }
            }
            match (tab(), stats_value) {
                (None, _) => rsx! {},
                (Some(StatsTab::Growth), _) => match growth_value {
                    None => rsx! { p { class: "stats-loading", {i18n.t("common.loading")} } },
                    Some(value) => measure_ui("statistics_growth", || {
                        render_growth(value, Utc::now().date_naive(), &i18n)
                    }),
                },
                (_, None) => rsx! { p { class: "stats-loading", {i18n.t("common.loading")} } },
                (Some(current), Some(value)) => rsx! {
                    match current {
                        StatsTab::Overview => render_overview(value, &i18n),
                        StatsTab::Population => rsx! {
                            PeriodCharts { stats, interval, range, view: PeriodView::Population }
                        },
                        StatsTab::Families => rsx! {
                            PeriodCharts { stats, interval, range, view: PeriodView::Families }
                        },
                        StatsTab::Places => measure_ui("statistics_places", || {
                            render_places(value, paths, cities, map_focus, &i18n)
                        }),
                        StatsTab::Names => render_names(value, &i18n),
                        StatsTab::Records => rsx! {
                            {render_extremes(value, &tree_id, &i18n)}
                            {render_lists(value, &tree_id, list, &i18n)}
                        },
                        StatsTab::Growth => rsx! {},
                    }
                },
            }
        }
    }
}

// ── Formatting ──────────────────────────────────────────────────────────

fn tenth(value: f64) -> String {
    format!("{value:.1}")
}

pub(crate) fn percent(i18n: &I18n, part: i64, whole: i64) -> String {
    let share = if whole > 0 {
        part as f64 * 100.0 / whole as f64
    } else {
        0.0
    };
    i18n.t_args("stats.percent", &[("n", &tenth(share))])
}

/// An age or a duration given in days, in the largest whole unit that
/// fits: days under a month, months under a year, else years.
fn duration(i18n: &I18n, days: f64) -> String {
    if days < DAYS_PER_MONTH {
        i18n.t_plural("stats.days", days.max(0.0) as usize)
    } else if days < DAYS_PER_YEAR {
        i18n.t_plural("stats.months", (days / DAYS_PER_MONTH) as usize)
    } else {
        i18n.t_plural("stats.age", (days / DAYS_PER_YEAR) as usize)
    }
}

pub(crate) fn date_text(i18n: &I18n, date: Option<&StatDate>) -> String {
    date.map(|d| {
        format_date(
            i18n,
            d.calendar,
            d.qualifier,
            d.value.as_deref(),
            d.value2.as_deref(),
        )
    })
    .unwrap_or_default()
}

/// A titled block of a tab.
fn block(i18n: &I18n, key: &str, body: Element) -> Element {
    rsx! {
        section { class: "stats-section",
            h2 { class: "stats-section-title", {i18n.t(&format!("stats.section.{key}"))} }
            {body}
        }
    }
}

// ── Overview ────────────────────────────────────────────────────────────

/// One figure of the overview: a value, what it counts, and detail lines.
fn tile(key: &str, value: String, label: String, details: Vec<String>) -> Element {
    rsx! {
        div { key: "{key}", class: "stats-tile",
            span { class: "stats-tile-value", "{value}" }
            span { class: "stats-tile-label", "{label}" }
            for (k, detail) in details.into_iter().enumerate() {
                span { key: "{k}", class: "stats-tile-detail", "{detail}" }
            }
        }
    }
}

/// A summary as a tile: its mean with the unit, then its median, spread
/// and range.
fn summary_tile(
    i18n: &I18n,
    key: &str,
    summary: &StatSummary,
    unit: &str,
    mut details: Vec<String>,
) -> Element {
    let value = summary
        .mean
        .map(|mean| format!("{} {}", tenth(mean), i18n.t(unit)))
        .unwrap_or_else(|| "–".to_string());
    if let (Some(median), Some(std_dev)) = (summary.median, summary.std_dev) {
        details.insert(
            0,
            format!(
                "{} · {}",
                i18n.t_args("stats.summary.median", &[("n", &tenth(median))]),
                i18n.t_args("stats.summary.std_dev", &[("n", &tenth(std_dev))]),
            ),
        );
    }
    if let (Some(min), Some(max)) = (summary.min, summary.max) {
        details.push(i18n.t_args(
            "stats.summary.range",
            &[("min", &tenth(min)), ("max", &tenth(max))],
        ));
    }
    tile(key, value, i18n.t(&format!("stats.summary.{key}")), details)
}

/// Men's and women's means, when either is known.
fn by_sex(i18n: &I18n, men: &StatSummary, women: &StatSummary) -> Vec<String> {
    let mean = |s: &StatSummary| s.mean.map(tenth).unwrap_or_else(|| "–".to_string());
    if men.mean.is_none() && women.mean.is_none() {
        return Vec::new();
    }
    vec![format!(
        "{} · {}",
        i18n.t_args("stats.summary.men", &[("n", &mean(men))]),
        i18n.t_args("stats.summary.women", &[("n", &mean(women))]),
    )]
}

fn render_overview(stats: &TreeStatistics, i18n: &I18n) -> Element {
    let count = |key: &str, value: i64| tile(key, value.to_string(), i18n.t(key), Vec::new());
    let share = |key: &str, part: i64| {
        tile(
            key,
            percent(i18n, part, stats.persons),
            i18n.t(key),
            vec![part.to_string()],
        )
    };
    let sexes = vec![format!(
        "{} {} · {} {} · {} {}",
        stats.men,
        i18n.t("stats.men"),
        stats.women,
        i18n.t("stats.women"),
        stats.unknown_sex,
        i18n.t("stats.unknown_sex"),
    )];
    let years = match (stats.first_year, stats.last_year) {
        (Some(from), Some(to)) => i18n.t_args(
            "stats.years_range",
            &[("from", &from.to_string()), ("to", &to.to_string())],
        ),
        _ => "–".to_string(),
    };
    let total: i64 = stats.event_types.iter().map(|e| e.count).sum();
    let types: Vec<(String, i64)> = stats
        .event_types
        .iter()
        .take(10)
        .map(|e| (event_type_label(i18n, &e.label), e.count))
        .collect();
    rsx! {
        {block(i18n, "counts", rsx! {
            div { class: "stats-tiles",
                {tile("stats.persons", stats.persons.to_string(), i18n.t("stats.persons"), sexes)}
                {count("stats.unions", stats.unions)}
                {count("stats.places", stats.places)}
                {count("stats.sources", stats.sources)}
                {tile("stats.years_covered", years, i18n.t("stats.years_covered"), Vec::new())}
                {count("stats.surnames", stats.surnames)}
                {count("stats.given_names", stats.given_names)}
            }
        })}
        {block(i18n, "completeness", rsx! {
            div { class: "stats-tiles",
                {share("stats.dated_births", stats.dated_births)}
                {share("stats.dated_deaths", stats.dated_deaths)}
                {share("stats.without_parents", stats.without_parents)}
                {share("stats.without_children", stats.without_children)}
                {share("stats.without_union", stats.without_union)}
            }
        })}
        {block(i18n, "averages", rsx! {
            div { class: "stats-tiles",
                {summary_tile(i18n, "lifespan", &stats.lifespan.all, "stats.unit.years", by_sex(i18n, &stats.lifespan.men, &stats.lifespan.women))}
                {summary_tile(i18n, "first_union", &stats.first_union_age.all, "stats.unit.years", by_sex(i18n, &stats.first_union_age.men, &stats.first_union_age.women))}
                {summary_tile(i18n, "generation", &stats.generation_interval, "stats.unit.years", Vec::new())}
                {summary_tile(i18n, "family_size", &stats.family_size, "stats.unit.children", Vec::new())}
            }
        })}
        {block(i18n, "event_types", rsx! {
            div { class: "stats-grid",
                {donut_card_noted(i18n, "event_types", types, Some(i18n.t_plural("stats.events_total", total as usize)))}
            }
        })}
    }
}

pub(crate) fn event_type_label(i18n: &I18n, label: &str) -> String {
    serde_json::from_value::<EventType>(serde_json::Value::String(label.to_string()))
        .map(|kind| i18n.t(event_type_label_key(kind)))
        .unwrap_or_else(|_| label.to_string())
}

// ── Places ──────────────────────────────────────────────────────────────

fn counts(entries: &[StatCount]) -> Vec<(String, i64)> {
    entries.iter().map(|e| (e.label.clone(), e.count)).collect()
}

fn donut_card(i18n: &I18n, key: &str, items: Vec<(String, i64)>) -> Element {
    donut_card_noted(i18n, key, items, None)
}

/// A donut in its card, with a note under it.
fn donut_card_noted(
    i18n: &I18n,
    key: &str,
    items: Vec<(String, i64)>,
    note: Option<String>,
) -> Element {
    let i18n = *i18n;
    rsx! {
        ChartCard {
            title: i18n.t(&format!("stats.chart.{key}")),
            hint: i18n.t(&format!("stats.hint.{key}")),
            empty: items.is_empty(),
            i18n,
            DonutChart { items }
            if let Some(note) = note {
                p { class: "stats-note", "{note}" }
            }
        }
    }
}

fn render_places(
    stats: &TreeStatistics,
    paths: Memo<Vec<String>>,
    cities: Memo<Vec<MapCity>>,
    mut focus: Signal<MapFocus>,
    i18n: &I18n,
) -> Element {
    let i18n = *i18n;
    let focused = focus().map(|(id, _, _)| id);
    let area = |key: &str, entries: &[StatCount], total: i64, noun: &str| {
        donut_card_noted(
            &i18n,
            key,
            counts(entries),
            Some(i18n.t_plural(noun, total as usize)),
        )
    };
    rsx! {
        {block(&i18n, "map", rsx! {
            div { class: "stats-places",
                if stats.located_places.is_empty() {
                    p { class: "stats-empty", {i18n.t("stats.map_empty")} }
                } else {
                    HeatMap {
                        paths,
                        cities,
                        places: stats.located_places.clone(),
                        top: stats.top_places.clone(),
                        focus,
                        i18n,
                    }
                }
                div { class: "stats-top-places",
                    h3 { class: "stats-card-title", {i18n.t("stats.top_places")} }
                    ol {
                        for place in stats.top_places.iter() {
                            li { key: "{place.place_id}",
                                if let (Some(latitude), Some(longitude)) = (place.latitude, place.longitude) {
                                    button {
                                        class: if focused == Some(place.place_id) { "stats-top-place-name stats-top-place-link active" } else { "stats-top-place-name stats-top-place-link" },
                                        title: "{i18n.t(\"stats.zoom_place\")}",
                                        onclick: {
                                            let id = place.place_id;
                                            move |_| focus.set(Some((id, latitude, longitude)))
                                        },
                                        "{place.name}"
                                    }
                                } else {
                                    span { class: "stats-top-place-name", "{place.name}" }
                                }
                                span { class: "stats-legend-count", "{place.count}" }
                            }
                        }
                    }
                    if stats.unlocated_places > 0 {
                        p { class: "stats-note",
                            {i18n.t_plural("stats.unlocated", stats.unlocated_places as usize)}
                        }
                    }
                }
            }
        })}
        {block(&i18n, "births_by_area", rsx! {
            div { class: "stats-grid stats-grid-3",
                {area("births_by_country", &stats.births_by_country, stats.countries, "stats.countries")}
                {area("births_by_region", &stats.births_by_region, stats.regions, "stats.regions")}
                {area("births_by_subdivision", &stats.births_by_subdivision, stats.subdivisions, "stats.subdivisions")}
            }
        })}
    }
}

// ── Names ───────────────────────────────────────────────────────────────

fn render_names(stats: &TreeStatistics, i18n: &I18n) -> Element {
    let i18n = *i18n;
    rsx! {
        {block(&i18n, "surnames_occupations", rsx! {
            div { class: "stats-grid",
                {donut_card(&i18n, "surnames", counts(&stats.top_surnames))}
                {donut_card(&i18n, "occupations", counts(&stats.top_occupations))}
            }
        })}
        {block(&i18n, "given_names", rsx! {
            div { class: "stats-grid",
                {donut_card(&i18n, "given_names_men", counts(&stats.top_given_names_men))}
                {donut_card(&i18n, "given_names_women", counts(&stats.top_given_names_women))}
            }
        })}
    }
}

// ── Period charts ───────────────────────────────────────────────────────

/// The periods the charts share: `interval` years wide and aligned on its
/// multiples, from the first to the last year shown, the first and the
/// last period cut at those years (`docs/ui-statistics.md` §7).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Periods {
    from: i32,
    to: i32,
    interval: i32,
}

impl Periods {
    fn start(&self, year: i32) -> i32 {
        year.div_euclid(self.interval) * self.interval
    }

    fn len(&self) -> usize {
        if self.to < self.from {
            0
        } else {
            ((self.start(self.to) - self.start(self.from)) / self.interval + 1) as usize
        }
    }

    fn index(&self, year: i32) -> Option<usize> {
        (self.from..=self.to)
            .contains(&year)
            .then(|| ((self.start(year) - self.start(self.from)) / self.interval) as usize)
    }

    /// Each period's first year shown, for the x-axis.
    fn labels(&self) -> Vec<i32> {
        (0..self.len() as i32)
            .map(|k| (self.start(self.from) + k * self.interval).max(self.from))
            .collect()
    }

    /// Each period's average; `None` where it has fewer than
    /// [`MIN_VALUES`] values.
    fn averages(&self, years: &[StatYearSum]) -> Vec<Option<f64>> {
        let mut totals = vec![(0.0, 0_i64); self.len()];
        for year in years {
            if let Some(slot) = self.index(year.year).and_then(|i| totals.get_mut(i)) {
                slot.0 += year.sum;
                slot.1 += year.count;
            }
        }
        totals
            .into_iter()
            .map(|(sum, count)| (count >= MIN_VALUES).then(|| round1(sum / count as f64)))
            .collect()
    }

    /// Each period's counts per category; `None` where the period has no
    /// year at all.
    fn totals(&self, years: &[StatYearCounts], categories: usize) -> Vec<Option<Vec<i64>>> {
        let mut totals: Vec<Option<Vec<i64>>> = vec![None; self.len()];
        for year in years {
            if let Some(slot) = self.index(year.year).and_then(|i| totals.get_mut(i)) {
                let slot = slot.get_or_insert_with(|| vec![0; categories]);
                for (total, count) in slot.iter_mut().zip(&year.counts) {
                    *total += count;
                }
            }
        }
        totals
    }

    /// Per category, each period's count.
    fn sums(&self, years: &[StatYearCounts], categories: usize) -> Vec<Vec<Option<f64>>> {
        let totals = self.totals(years, categories);
        (0..categories)
            .map(|k| {
                totals
                    .iter()
                    .map(|period| period.as_ref().map(|counts| counts[k] as f64))
                    .collect()
            })
            .collect()
    }

    /// Per category, each period's share of the counts, in percent; `None`
    /// where the period has fewer than [`MIN_VALUES`] counts.
    fn shares(&self, years: &[StatYearCounts], categories: usize) -> Vec<Vec<Option<f64>>> {
        let totals = self.totals(years, categories);
        (0..categories)
            .map(|k| {
                totals
                    .iter()
                    .map(|period| {
                        let counts = period.as_ref()?;
                        let all: i64 = counts.iter().sum();
                        (all >= MIN_VALUES).then(|| round1(counts[k] as f64 * 100.0 / all as f64))
                    })
                    .collect()
            })
            .collect()
    }

    /// Each period's count of category `part` per `per` of category `of`;
    /// `None` where `of` counts fewer than [`MIN_VALUES`].
    fn ratio(
        &self,
        years: &[StatYearCounts],
        categories: usize,
        part: usize,
        of: usize,
        per: f64,
    ) -> Vec<Option<f64>> {
        self.totals(years, categories)
            .into_iter()
            .map(|period| {
                let counts = period?;
                (counts[of] >= MIN_VALUES)
                    .then(|| round1(counts[part] as f64 * per / counts[of] as f64))
            })
            .collect()
    }
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

/// The first and last years any period chart has a value for.
fn year_bounds(stats: &TreeStatistics) -> Option<(i32, i32)> {
    let sums = [
        &stats.age_at_death.men,
        &stats.age_at_death.women,
        &stats.life_expectancy.men,
        &stats.life_expectancy.women,
        &stats.parents_age.father_first_child,
        &stats.parents_age.mother_first_child,
        &stats.parents_age.father_last_child,
        &stats.parents_age.mother_last_child,
        &stats.parents_age.father_every_child,
        &stats.parents_age.mother_every_child,
        &stats.age_at_first_union.men,
        &stats.age_at_first_union.women,
        &stats.union_duration,
        &stats.children_per_union,
        &stats.birth_spacing,
        &stats.first_last_child_gap,
        &stats.spouse_age_gap,
    ];
    let counts = [
        &stats.events_by_year,
        &stats.births_by_sex,
        &stats.mortality,
        &stats.births_by_month,
        &stats.unions_by_weekday,
        &stats.unions_by_month,
    ];
    sums.iter()
        .flat_map(|s| s.iter().map(|y| y.year))
        .chain(counts.iter().flat_map(|s| s.iter().map(|y| y.year)))
        .fold(None, |bounds, year| match bounds {
            None => Some((year, year)),
            Some((first, last)) => Some((first.min(year), last.max(year))),
        })
}

/// Where the first view of the period charts starts: the first year
/// opening a [`DENSE_SPAN`] that holds at least [`DENSE_SHARE`] of the
/// dated events, else the first year with any.
fn dense_start(events: &[StatYearCounts]) -> Option<i32> {
    let total: i64 = events.iter().flat_map(|y| &y.counts).sum();
    let needed = (total as f64 * DENSE_SHARE).max(1.0);
    let count = |y: &StatYearCounts| y.counts.iter().sum::<i64>();
    let mut end = 0;
    let mut window = 0;
    for (start, year) in events.iter().enumerate() {
        while end < events.len() && events[end].year < year.year + DENSE_SPAN {
            window += count(&events[end]);
            end += 1;
        }
        if window as f64 >= needed {
            return Some(year.year);
        }
        window -= count(&events[start]);
    }
    events.first().map(|y| y.year)
}

fn series(label: String, color: &'static str, values: Vec<Option<f64>>) -> ChartSeries {
    ChartSeries {
        label,
        color,
        values,
    }
}

fn sex_series(
    i18n: &I18n,
    periods: &Periods,
    men: &[StatYearSum],
    women: &[StatYearSum],
) -> Vec<ChartSeries> {
    vec![
        series(
            i18n.t("stats.series.men"),
            "var(--pn-male-line)",
            periods.averages(men),
        ),
        series(
            i18n.t("stats.series.women"),
            "var(--pn-female-line)",
            periods.averages(women),
        ),
    ]
}

/// One series per category, from per-category values.
fn category_series(values: Vec<Vec<Option<f64>>>, labels: &[String]) -> Vec<ChartSeries> {
    values
        .into_iter()
        .zip(labels)
        .enumerate()
        .map(|(k, (values, label))| series(label.clone(), PALETTE[k % PALETTE.len()], values))
        .collect()
}

fn single(label: String, periods: &Periods, years: &[StatYearSum]) -> Vec<ChartSeries> {
    vec![series(label, PALETTE[0], periods.averages(years))]
}

fn no_data(series: &[ChartSeries]) -> bool {
    series.iter().all(|s| s.values.iter().all(Option::is_none))
}

fn labels(i18n: &I18n, keys: &[&str]) -> Vec<String> {
    keys.iter().map(|key| i18n.t(key)).collect()
}

fn months(i18n: &I18n) -> Vec<String> {
    (1..=12)
        .map(|m| i18n.t(&format!("date.month.{m}")))
        .collect()
}

fn weekdays(i18n: &I18n) -> Vec<String> {
    (1..=7)
        .map(|d| i18n.t(&format!("stats.weekday.{d}")))
        .collect()
}

/// A line chart in its card, with the empty state when no series has data.
fn line_card(
    i18n: &I18n,
    key: &str,
    periods: &[i32],
    series: Vec<ChartSeries>,
    unit: &str,
) -> Element {
    let i18n = *i18n;
    rsx! {
        ChartCard {
            title: i18n.t(&format!("stats.chart.{key}")),
            hint: i18n.t(&format!("stats.hint.{key}")),
            empty: no_data(&series),
            i18n,
            LineChart {
                periods: periods.iter().map(i32::to_string).collect::<Vec<_>>(),
                series,
                unit: i18n.t(unit),
            }
        }
    }
}

/// Which tab of period charts to draw.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PeriodView {
    Population,
    Families,
}

/// A tab of period charts, under the bar choosing their interval and the
/// years they cover. The range lives in the page, so both tabs share it,
/// and is only read here, so moving it redraws these charts alone.
#[component]
fn PeriodCharts(
    stats: Resource<Option<TreeStatistics>>,
    interval: Signal<i32>,
    range: Signal<Option<(i32, i32)>>,
    view: PeriodView,
) -> Element {
    let i18n = use_i18n();
    let mut interval = interval;
    let mut range = range;
    let stats_read = stats.read();
    let Some(stats) = stats_read.as_ref().and_then(Option::as_ref) else {
        return rsx! {};
    };
    let bounds = year_bounds(stats);
    let (first, last) = bounds.unwrap_or((0, -1));
    let (from, to) = match range() {
        Some((from, to)) => (from, to),
        None => (dense_start(&stats.events_by_year).unwrap_or(first), last),
    };
    let from = from.clamp(first, last.max(first));
    let to = to.clamp(from, last.max(from));
    let whole = (from, to) == (first, last);
    let periods = Periods {
        from,
        to,
        interval: interval(),
    };
    rsx! {
        div { class: "stats-period-charts",
            div { class: "stats-timeline",
                span { class: "stats-timeline-title", {i18n.t("stats.years")} }
                span { class: "stats-timeline-years",
                    {i18n.t_args("stats.years_range", &[("from", &from.to_string()), ("to", &to.to_string())])}
                }
                if last > first {
                    YearRuler {
                        min: first,
                        max: last,
                        from,
                        to,
                        interval: interval(),
                        from_label: i18n.t("stats.years_from"),
                        to_label: i18n.t("stats.years_to"),
                        on_change: move |chosen: (i32, i32)| range.set(Some(chosen)),
                    }
                } else {
                    span {}
                }
                label { class: "stats-interval",
                    {i18n.t("stats.interval")}
                    select {
                        class: "td-select",
                        onchange: move |e: Event<FormData>| {
                            if let Ok(value) = e.value().parse::<i32>() {
                                interval.set(value);
                                store(INTERVAL_STORAGE_KEY, &value.to_string());
                            }
                        },
                        for value in INTERVALS {
                            option {
                                key: "{value}",
                                value: "{value}",
                                selected: interval() == value,
                                {i18n.t_args("stats.interval_years", &[("n", &value.to_string())])}
                            }
                        }
                    }
                }
                button {
                    class: "btn btn-outline btn-sm",
                    disabled: whole,
                    onclick: move |_| range.set(Some((first, last))),
                    {i18n.t("stats.years_all")}
                }
            }
            match view {
                PeriodView::Population => measure_ui("statistics_population", || {
                    render_population(stats, &periods, &i18n)
                }),
                PeriodView::Families => measure_ui("statistics_families", || {
                    render_families(stats, &periods, &i18n)
                }),
            }
        }
    }
}

fn render_population(stats: &TreeStatistics, periods: &Periods, i18n: &I18n) -> Element {
    let p = &periods.labels();
    let events = category_series(
        periods.sums(&stats.events_by_year, 5),
        &labels(
            i18n,
            &[
                "stats.series.births",
                "stats.series.baptisms",
                "stats.series.unions",
                "stats.series.deaths",
                "stats.series.burials",
            ],
        ),
    );
    let sex_ratio = vec![series(
        i18n.t("stats.chart.sex_ratio"),
        PALETTE[0],
        periods.ratio(&stats.births_by_sex, 2, 0, 1, 100.0),
    )];
    let mortality = vec![
        series(
            i18n.t("stats.series.infant"),
            PALETTE[0],
            periods.ratio(&stats.mortality, 3, 1, 0, 100.0),
        ),
        series(
            i18n.t("stats.series.under_five"),
            PALETTE[1],
            periods.ratio(&stats.mortality, 3, 2, 0, 100.0),
        ),
    ];
    let pyramid = {
        let i18n = *i18n;
        rsx! {
            ChartCard {
                title: i18n.t("stats.chart.pyramid"),
                hint: i18n.t("stats.hint.pyramid"),
                empty: stats.pyramid.is_empty(),
                i18n,
                Pyramid {
                    bands: stats.pyramid.clone(),
                    men: i18n.t("stats.series.men"),
                    women: i18n.t("stats.series.women"),
                }
            }
        }
    };
    rsx! {
        {block(i18n, "births_deaths", rsx! {
            div { class: "stats-grid",
                {line_card(i18n, "events", p, events, "stats.unit.events")}
                {line_card(i18n, "sex_ratio", p, sex_ratio, "stats.unit.per_100_women")}
                {line_card(i18n, "births_by_month", p, category_series(periods.shares(&stats.births_by_month, 12), &months(i18n)), "stats.unit.percent")}
                {line_card(i18n, "mortality", p, mortality, "stats.unit.percent")}
            }
        })}
        {block(i18n, "lifespan", rsx! {
            div { class: "stats-grid",
                {line_card(i18n, "age_at_death", p, sex_series(i18n, periods, &stats.age_at_death.men, &stats.age_at_death.women), "stats.unit.years")}
                {line_card(i18n, "life_expectancy", p, sex_series(i18n, periods, &stats.life_expectancy.men, &stats.life_expectancy.women), "stats.unit.years")}
                {pyramid}
            }
        })}
    }
}

fn render_families(stats: &TreeStatistics, periods: &Periods, i18n: &I18n) -> Element {
    let p = &periods.labels();
    let single = |key: &str, years: &[StatYearSum]| {
        single(i18n.t(&format!("stats.chart.{key}")), periods, years)
    };
    let parents = &stats.parents_age;
    let parent_series = vec![
        series(
            i18n.t("stats.series.father_first"),
            "var(--pn-male-line)",
            periods.averages(&parents.father_first_child),
        ),
        series(
            i18n.t("stats.series.mother_first"),
            "var(--pn-female-line)",
            periods.averages(&parents.mother_first_child),
        ),
        series(
            i18n.t("stats.series.father_last"),
            PALETTE[2],
            periods.averages(&parents.father_last_child),
        ),
        series(
            i18n.t("stats.series.mother_last"),
            PALETTE[3],
            periods.averages(&parents.mother_last_child),
        ),
    ];
    let generation = vec![
        series(
            i18n.t("stats.series.fathers"),
            "var(--pn-male-line)",
            periods.averages(&parents.father_every_child),
        ),
        series(
            i18n.t("stats.series.mothers"),
            "var(--pn-female-line)",
            periods.averages(&parents.mother_every_child),
        ),
    ];
    let histogram = {
        let i18n = *i18n;
        let children: Vec<(String, i64)> = stats
            .children_histogram
            .iter()
            .enumerate()
            .filter(|(_, unions)| **unions > 0)
            .map(|(n, unions)| (n.to_string(), *unions))
            .collect();
        rsx! {
            ChartCard {
                title: i18n.t("stats.chart.children_histogram"),
                hint: i18n.t("stats.hint.children_histogram"),
                empty: children.is_empty(),
                i18n,
                BarChart { items: children }
            }
        }
    };
    rsx! {
        {block(i18n, "unions", rsx! {
            div { class: "stats-grid",
                {line_card(i18n, "age_at_first_union", p, sex_series(i18n, periods, &stats.age_at_first_union.men, &stats.age_at_first_union.women), "stats.unit.years")}
                {line_card(i18n, "union_duration", p, single("union_duration", &stats.union_duration), "stats.unit.years")}
                {line_card(i18n, "unions_by_weekday", p, category_series(periods.shares(&stats.unions_by_weekday, 7), &weekdays(i18n)), "stats.unit.percent")}
                {line_card(i18n, "unions_by_month", p, category_series(periods.shares(&stats.unions_by_month, 12), &months(i18n)), "stats.unit.percent")}
                {line_card(i18n, "spouse_age_gap", p, single("spouse_age_gap", &stats.spouse_age_gap), "stats.unit.months")}
            }
        })}
        {block(i18n, "children", rsx! {
            div { class: "stats-grid",
                {line_card(i18n, "children_per_union", p, single("children_per_union", &stats.children_per_union), "stats.unit.children")}
                {histogram}
                {line_card(i18n, "birth_spacing", p, single("birth_spacing", &stats.birth_spacing), "stats.unit.months")}
                {line_card(i18n, "first_last_child_gap", p, single("first_last_child_gap", &stats.first_last_child_gap), "stats.unit.months")}
                {line_card(i18n, "parents_age", p, parent_series, "stats.unit.years")}
                {line_card(i18n, "generation_interval", p, generation, "stats.unit.years")}
            }
        })}
    }
}

// ── Growth ──────────────────────────────────────────────────────────────

/// How wide the Growth tab's periods are, chosen from the days the tree
/// was worked on (`docs/ui-statistics.md` §10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Granularity {
    Day,
    Week,
    Month,
    Year,
}

impl Granularity {
    /// Days up to a month, weeks up to half a year, months up to five
    /// years, years beyond: from a few dozen points to about sixty.
    fn for_span(days: i64) -> Self {
        match days {
            i64::MIN..=31 => Self::Day,
            32..=182 => Self::Week,
            183..=1826 => Self::Month,
            _ => Self::Year,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
            Self::Year => "year",
        }
    }

    /// The first day of the period holding `date`; weeks start on Monday.
    fn start(self, date: NaiveDate) -> NaiveDate {
        match self {
            Self::Day => date,
            Self::Week => {
                date - chrono::Days::new(u64::from(date.weekday().num_days_from_monday()))
            }
            Self::Month => date.with_day(1).unwrap_or(date),
            Self::Year => NaiveDate::from_ymd_opt(date.year(), 1, 1).unwrap_or(date),
        }
    }

    /// The first day of the period after the one starting on `start`.
    fn next(self, start: NaiveDate) -> NaiveDate {
        let next = match self {
            Self::Day => start.succ_opt(),
            Self::Week => start.checked_add_days(chrono::Days::new(7)),
            Self::Month => start.checked_add_months(Months::new(1)),
            Self::Year => start.checked_add_months(Months::new(12)),
        };
        next.unwrap_or(NaiveDate::MAX)
    }

    /// A period's x-axis label: its first day and month for days and weeks,
    /// its month and year, or its year.
    fn label(self, i18n: &I18n, start: NaiveDate) -> String {
        let month = || i18n.t(&format!("date.month.{}", start.month()));
        match self {
            Self::Day | Self::Week => format!("{} {}", start.day(), month()),
            Self::Month => format!("{} {}", month(), start.year()),
            Self::Year => start.year().to_string(),
        }
    }
}

/// One period of the Growth tab: the persons at its end, and those added
/// and removed during it.
#[derive(Debug, Clone, PartialEq)]
struct GrowthPeriod {
    start: NaiveDate,
    persons: i64,
    added: i64,
    removed: i64,
}

/// The periods from the first day the person count changed to `today`,
/// every one of them, those without a change keeping the count; `None`
/// when no person was ever added.
fn growth_periods(
    days: &[GrowthDay],
    today: NaiveDate,
) -> Option<(Granularity, Vec<GrowthPeriod>)> {
    let first = days.first()?.date;
    let last = days.last().map_or(today, |day| day.date.max(today));
    let granularity = Granularity::for_span((last - first).num_days());
    let mut periods = Vec::new();
    let mut start = granularity.start(first);
    let (mut persons, mut next_day) = (0, 0);
    while start <= last {
        let end = granularity.next(start);
        let (mut added, mut removed) = (0, 0);
        while let Some(day) = days.get(next_day).filter(|day| day.date < end) {
            added += day.added;
            removed += day.removed;
            next_day += 1;
        }
        persons += added - removed;
        periods.push(GrowthPeriod {
            start,
            persons,
            added,
            removed,
        });
        start = end;
    }
    Some((granularity, periods))
}

/// The index of the period holding `date`, if the periods cover it.
fn period_index(
    periods: &[GrowthPeriod],
    date: NaiveDate,
    granularity: Granularity,
) -> Option<usize> {
    let index = periods
        .partition_point(|p| p.start <= date)
        .checked_sub(1)?;
    (date < granularity.next(periods[index].start)).then_some(index)
}

fn render_growth(growth: &TreeGrowth, today: NaiveDate, i18n: &I18n) -> Element {
    let i18n = *i18n;
    let Some((granularity, periods)) = growth_periods(&growth.days, today) else {
        return block(
            &i18n,
            "growth",
            rsx! {
                p { class: "stats-empty", {i18n.t("stats.growth.empty")} }
            },
        );
    };
    let labels: Vec<String> = periods
        .iter()
        .map(|p| granularity.label(&i18n, p.start))
        .collect();
    // One numbered badge per period holding an import; each import is
    // listed under the chart with its badge's number.
    let mut by_period: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (k, import) in growth.imports.iter().enumerate() {
        if let Some(index) = period_index(&periods, import.occurred_at.date_naive(), granularity) {
            by_period.entry(index).or_default().push(k);
        }
    }
    let import_label = |k: usize| {
        i18n.t_plural(
            "stats.growth.import",
            growth.imports[k].persons.max(0) as usize,
        )
    };
    let mut numbers: BTreeMap<usize, usize> = BTreeMap::new();
    let markers: Vec<ChartMarker> = by_period
        .iter()
        .enumerate()
        .map(|(n, (index, imports))| {
            for k in imports {
                numbers.insert(*k, n + 1);
            }
            ChartMarker {
                index: *index,
                number: n + 1,
                label: imports
                    .iter()
                    .map(|k| import_label(*k))
                    .collect::<Vec<_>>()
                    .join(", "),
            }
        })
        .collect();
    let persons = vec![series(
        i18n.t("stats.series.persons"),
        PALETTE[0],
        periods.iter().map(|p| Some(p.persons as f64)).collect(),
    )];
    let changes = vec![
        series(
            i18n.t("stats.series.added"),
            "var(--green)",
            periods.iter().map(|p| Some(p.added as f64)).collect(),
        ),
        series(
            i18n.t("stats.series.removed"),
            "var(--red)",
            periods.iter().map(|p| Some(p.removed as f64)).collect(),
        ),
    ];
    let last_day = growth.days.last().map_or(today, |day| day.date.max(today));
    let span = i18n.t_args(
        &format!("stats.growth.per.{}", granularity.key()),
        &[
            ("from", &format_day(&i18n, growth.days[0].date)),
            ("to", &format_day(&i18n, last_day)),
        ],
    );
    let unit = i18n.t("stats.unit.persons");
    rsx! {
        {block(&i18n, "growth", rsx! {
            p { class: "stats-note", "{span}" }
            div { class: "stats-grid",
                ChartCard {
                    title: i18n.t("stats.chart.growth_persons"),
                    hint: i18n.t("stats.hint.growth_persons"),
                    empty: false,
                    i18n,
                    LineChart { periods: labels.clone(), series: persons, unit: unit.clone(), markers }
                    if !growth.imports.is_empty() {
                        ul { class: "stats-markers",
                            for (k, import) in growth.imports.iter().enumerate() {
                                li { key: "{k}",
                                    if let Some(number) = numbers.get(&k) {
                                        span { class: "stats-markers-number", "{number}" }
                                    }
                                    span { {format_timestamp(&i18n, import.occurred_at)} }
                                    span { "· {import_label(k)}" }
                                    if let Some(file) = import.file_name.as_ref() {
                                        span { class: "stats-markers-file", "{file}" }
                                    }
                                }
                            }
                        }
                    }
                }
                ChartCard {
                    title: i18n.t("stats.chart.growth_changes"),
                    hint: i18n.t("stats.hint.growth_changes"),
                    empty: false,
                    i18n,
                    LineChart { periods: labels, series: changes, unit }
                }
            }
        })}
    }
}

// ── Records ─────────────────────────────────────────────────────────────

/// What a record's value says, in words: an age or a duration, a count, or
/// nothing for a record that is a date.
fn record_value(i18n: &I18n, record: &StatRecord) -> Option<String> {
    let value = record.value?;
    let count = value.max(0.0) as usize;
    Some(match record.kind.as_str() {
        "most_unions" => i18n.t_plural("stats.count.unions", count),
        "most_places" => i18n.t_plural("stats.count.places", count),
        "most_generations" => i18n.t_plural("stats.count.generations", count),
        "most_children" => {
            let children = i18n.t_plural("stats.count.children", count);
            match record.value2 {
                Some(spouses) if spouses > 0 => format!(
                    "{children} {}",
                    i18n.t_plural("stats.record.with_spouses", spouses as usize)
                ),
                _ => children,
            }
        }
        _ => duration(i18n, value),
    })
}

pub(crate) fn person_links(tree_id: &str, persons: &[StatPersonRef], separator: &str) -> Element {
    rsx! {
        for (k, person) in persons.iter().enumerate() {
            span { key: "{k}",
                if k > 0 { "{separator}" }
                Link {
                    to: Route::PersonDetail {
                        tree_id: tree_id.to_string(),
                        person_id: person.person_id.to_string(),
                    },
                    "{person.name}"
                }
            }
        }
    }
}

fn render_extremes(stats: &TreeStatistics, tree_id: &str, i18n: &I18n) -> Element {
    if stats.records.is_empty() {
        return rsx! {};
    }
    rsx! {
        section { class: "stats-section",
            h2 { class: "stats-section-title",
                {i18n.t("stats.section.extremes")}
                span {
                    class: "stats-hint",
                    title: "{i18n.t(\"stats.hint.extremes\")}",
                    "aria-label": "{i18n.t(\"stats.hint.extremes\")}",
                    "?"
                }
            }
            div { class: "stats-feats",
                for record in stats.records.iter() {
                    div { key: "{record.kind}", class: "stats-feat",
                        span { class: "stats-feat-title", {i18n.t(&format!("stats.record.{}", record.kind))} }
                        span { class: "stats-feat-who",
                            {
                                let separator = if record.kind == "longest_widowhood" {
                                    format!(" {} ", i18n.t("stats.record.outlived"))
                                } else {
                                    " & ".to_string()
                                };
                                person_links(tree_id, &record.persons, &separator)
                            }
                        }
                        if let Some(value) = record_value(i18n, record) {
                            span { class: "stats-feat-value", "{value}" }
                        }
                        if record.date.is_some() {
                            span { class: "stats-feat-date", "{date_text(i18n, record.date.as_ref())}" }
                        }
                    }
                }
            }
        }
    }
}

// ── Lists ───────────────────────────────────────────────────────────────

fn render_lists(
    stats: &TreeStatistics,
    tree_id: &str,
    mut tab: Signal<ListTab>,
    i18n: &I18n,
) -> Element {
    let tabs = [
        (ListTab::Births, "stats.records.births"),
        (ListTab::Unions, "stats.records.unions"),
        (ListTab::Deaths, "stats.records.deaths"),
        (ListTab::OldestAlive, "stats.records.oldest_alive"),
        (ListTab::LongestLives, "stats.records.longest_lives"),
        (ListTab::LargestFamilies, "stats.records.largest_families"),
    ];
    let person_link = |p: &StatPerson| Route::PersonDetail {
        tree_id: tree_id.to_string(),
        person_id: p.person_id.to_string(),
    };
    let body = match tab() {
        ListTab::Unions => rsx! {
            table { class: "stats-table",
                tbody {
                    for union in stats.recent_unions.iter() {
                        tr { key: "{union.family_id}",
                            td { {person_links(tree_id, &union.spouses, " & ")} }
                            td { "{date_text(i18n, Some(&union.date))}" }
                            td { class: "text-muted", "{union.place.clone().unwrap_or_default()}" }
                        }
                    }
                }
            }
        },
        ListTab::LargestFamilies => rsx! {
            table { class: "stats-table",
                tbody {
                    for family in stats.largest_families.iter() {
                        tr { key: "{family.family_id}",
                            td { {person_links(tree_id, &family.spouses, " & ")} }
                            td { "{date_text(i18n, family.date.as_ref())}" }
                            td { class: "stats-age",
                                {i18n.t_plural("stats.count.children", family.children as usize)}
                            }
                        }
                    }
                }
            }
            if stats.largest_families.is_empty() {
                p { class: "stats-empty", {i18n.t("stats.empty")} }
            }
        },
        other => {
            let (rows, with_age) = match other {
                ListTab::Births => (&stats.recent_births, false),
                ListTab::Deaths => (&stats.recent_deaths, false),
                ListTab::OldestAlive => (&stats.oldest_possibly_alive, true),
                _ => (&stats.longest_lives, true),
            };
            rsx! {
                table { class: "stats-table",
                    tbody {
                        for person in rows.iter() {
                            tr { key: "{person.person_id}",
                                td { Link { to: person_link(person), "{person.name}" } }
                                td { "{date_text(i18n, person.date.as_ref())}" }
                                td { class: "text-muted", "{person.place.clone().unwrap_or_default()}" }
                                if with_age {
                                    td { class: "stats-age",
                                        {person.age.map(|a| i18n.t_plural("stats.age", a as usize)).unwrap_or_default()}
                                    }
                                }
                            }
                        }
                    }
                }
                if rows.is_empty() {
                    p { class: "stats-empty", {i18n.t("stats.empty")} }
                }
            }
        }
    };
    rsx! {
        section { class: "stats-section",
            h2 { class: "stats-section-title", {i18n.t("stats.section.records")} }
            Tabs {
                tabs: tabs.iter().map(|(value, key)| (*value, i18n.t(key))).collect::<Vec<_>>(),
                current: Some(tab()),
                on_select: move |value| tab.set(value),
            }
            {body}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sum(year: i32, sum: f64, count: i64) -> StatYearSum {
        StatYearSum { year, sum, count }
    }

    #[test]
    fn periods_are_aligned_on_the_interval_and_cut_at_the_range() {
        let periods = Periods {
            from: 1712,
            to: 1760,
            interval: 25,
        };
        assert_eq!(periods.labels(), vec![1712, 1725, 1750]);
        assert_eq!(periods.index(1711), None);
        assert_eq!(periods.index(1712), Some(0));
        assert_eq!(periods.index(1724), Some(0));
        assert_eq!(periods.index(1725), Some(1));
        assert_eq!(periods.index(1760), Some(2));
        assert_eq!(periods.index(1761), None);
    }

    #[test]
    fn averages_weigh_each_year_by_its_count() {
        let periods = Periods {
            from: 1800,
            to: 1849,
            interval: 25,
        };
        // 1810: eight values summing to 400; 1820: four summing to 280;
        // 1850 is outside.
        let years = [
            sum(1810, 400.0, 8),
            sum(1820, 280.0, 4),
            sum(1850, 10.0, 20),
        ];
        assert_eq!(periods.averages(&years), vec![Some(56.7), None]);
    }

    #[test]
    fn too_few_values_make_no_point() {
        let periods = Periods {
            from: 1800,
            to: 1849,
            interval: 25,
        };
        let years = [sum(1810, 90.0, 9), sum(1830, 100.0, 10)];
        assert_eq!(periods.averages(&years), vec![None, Some(10.0)]);
        let few = [StatYearCounts {
            year: 1810,
            counts: vec![5, 4],
        }];
        assert_eq!(periods.shares(&few, 2), vec![vec![None, None]; 2]);
    }

    #[test]
    fn the_first_view_skips_a_sparse_beginning() {
        let year = |year: i32, n: i64| StatYearCounts {
            year,
            counts: vec![n, 0, 0, 0, 0],
        };
        // Three early records, then a dense tree from 1650.
        let events = [
            year(800, 1),
            year(1200, 1),
            year(1500, 1),
            year(1650, 40),
            year(1660, 60),
            year(1700, 100),
        ];
        assert_eq!(dense_start(&events), Some(1650));
        // A tree dense from its start keeps it.
        assert_eq!(dense_start(&events[3..]), Some(1650));
        assert_eq!(dense_start(&[]), None);
    }

    #[test]
    fn shares_add_up_the_counts_of_a_period() {
        let periods = Periods {
            from: 1900,
            to: 1919,
            interval: 10,
        };
        let years = [
            StatYearCounts {
                year: 1901,
                counts: vec![5, 0],
            },
            StatYearCounts {
                year: 1905,
                counts: vec![5, 10],
            },
        ];
        assert_eq!(
            periods.shares(&years, 2),
            vec![vec![Some(50.0), None], vec![Some(50.0), None]]
        );
    }

    #[test]
    fn an_empty_range_has_no_period() {
        let periods = Periods {
            from: 0,
            to: -1,
            interval: 25,
        };
        assert!(periods.labels().is_empty());
        assert!(periods.averages(&[sum(0, 1.0, 1)]).is_empty());
    }

    #[test]
    fn counts_and_ratios_follow_the_periods() {
        let periods = Periods {
            from: 1900,
            to: 1929,
            interval: 10,
        };
        let years = [
            StatYearCounts {
                year: 1901,
                counts: vec![6, 2],
            },
            StatYearCounts {
                year: 1909,
                counts: vec![5, 8],
            },
            StatYearCounts {
                year: 1925,
                counts: vec![1, 0],
            },
        ];
        // Counts are shown however few they are.
        assert_eq!(
            periods.sums(&years, 2),
            vec![
                vec![Some(11.0), None, Some(1.0)],
                vec![Some(10.0), None, Some(0.0)]
            ]
        );
        // Boys per 100 girls; none where fewer than ten girls were born.
        assert_eq!(
            periods.ratio(&years, 2, 0, 1, 100.0),
            vec![Some(110.0), None, None]
        );
    }

    #[test]
    fn durations_take_the_largest_whole_unit() {
        let i18n = I18n::new(Language::english());
        assert_eq!(duration(&i18n, 1.0), "1 day");
        assert_eq!(duration(&i18n, 61.0), "2 months");
        assert_eq!(duration(&i18n, 25567.0), "70 years");
    }

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn change(date: NaiveDate, added: i64, removed: i64) -> GrowthDay {
        GrowthDay {
            date,
            added,
            removed,
        }
    }

    #[test]
    fn the_growth_granularity_follows_the_span() {
        assert_eq!(Granularity::for_span(0), Granularity::Day);
        assert_eq!(Granularity::for_span(31), Granularity::Day);
        assert_eq!(Granularity::for_span(32), Granularity::Week);
        assert_eq!(Granularity::for_span(182), Granularity::Week);
        assert_eq!(Granularity::for_span(183), Granularity::Month);
        assert_eq!(Granularity::for_span(1826), Granularity::Month);
        assert_eq!(Granularity::for_span(1827), Granularity::Year);
        // Weeks start on Monday; months and years on their first day.
        assert_eq!(Granularity::Week.start(day(2026, 9, 27)), day(2026, 9, 21));
        assert_eq!(Granularity::Month.next(day(2026, 12, 1)), day(2027, 1, 1));
        assert_eq!(Granularity::Year.start(day(2026, 9, 27)), day(2026, 1, 1));
    }

    #[test]
    fn a_tree_without_persons_has_no_growth() {
        assert_eq!(growth_periods(&[], day(2026, 9, 28)), None);
    }

    #[test]
    fn a_tree_filled_today_is_one_point() {
        let (granularity, periods) =
            growth_periods(&[change(day(2026, 9, 28), 1200, 0)], day(2026, 9, 28)).unwrap();
        assert_eq!(granularity, Granularity::Day);
        assert_eq!(
            periods,
            [GrowthPeriod {
                start: day(2026, 9, 28),
                persons: 1200,
                added: 1200,
                removed: 0,
            }]
        );
    }

    #[test]
    fn growth_periods_run_to_today_keeping_the_count() {
        // An import in January, work in March, a deletion in May; seen in
        // September: eight months, one point each.
        let days = [
            change(day(2026, 1, 15), 500, 0),
            change(day(2026, 3, 2), 3, 0),
            change(day(2026, 3, 20), 2, 1),
            change(day(2026, 5, 9), 0, 4),
        ];
        let (granularity, periods) = growth_periods(&days, day(2026, 9, 28)).unwrap();
        assert_eq!(granularity, Granularity::Month);
        let counts: Vec<(i64, i64, i64)> = periods
            .iter()
            .map(|p| (p.persons, p.added, p.removed))
            .collect();
        assert_eq!(
            counts,
            [
                (500, 500, 0),
                (500, 0, 0),
                (504, 5, 1),
                (504, 0, 0),
                (500, 0, 4),
                (500, 0, 0),
                (500, 0, 0),
                (500, 0, 0),
                (500, 0, 0),
            ]
        );
        assert_eq!(periods[0].start, day(2026, 1, 1));
        let i18n = I18n::new(Language::english());
        assert_eq!(granularity.label(&i18n, periods[2].start), "Mar 2026");
        assert_eq!(Granularity::Day.label(&i18n, day(2026, 3, 2)), "2 Mar");
        // An import is marked in the period that holds it.
        assert_eq!(
            period_index(&periods, day(2026, 3, 31), granularity),
            Some(2)
        );
        assert_eq!(period_index(&periods, day(2025, 12, 31), granularity), None);
        assert_eq!(period_index(&periods, day(2026, 10, 1), granularity), None);
    }
}
