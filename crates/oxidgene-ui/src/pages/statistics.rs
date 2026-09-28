//! Statistics page: an overview of the tree, where its events happened, its
//! names, demographic charts per period, its records and notable lists.
//! See `docs/ui-statistics.md`.

use dioxus::prelude::*;
use oxidgene_core::EventType;
use uuid::Uuid;

use crate::api::{
    ApiClient, StatCount, StatDate, StatPerson, StatPersonRef, StatRecord, StatSummary,
    StatYearCounts, StatYearSum, TreeStatistics,
};
use crate::components::charts::{
    BarChart, ChartCard, ChartSeries, DonutChart, HeatMap, LineChart, PALETTE, Pyramid, YearRuler,
    basemap_paths,
};
use crate::components::date_input::format_date;
use crate::components::tree_cache::{fetch_tree_cached, use_current_person, use_tree_cache};
use crate::components::tree_icon_sidebar::{TreeIconSidebar, TreeSidebarView};
use crate::i18n::{I18n, Language, use_i18n};
use crate::router::Route;
use crate::ui_observability::{UiPage, use_traced_resource, use_ui_load_trace};
use crate::utils::event_type_label_key;

/// The period widths offered, in years.
const INTERVALS: [i32; 4] = [10, 25, 50, 100];
const DEFAULT_INTERVAL: i32 = 25;
const INTERVAL_STORAGE_KEY: &str = "oxidgene-stats-interval";
const APPROXIMATE_STORAGE_KEY: &str = "oxidgene-stats-approximate";
const DAYS_PER_YEAR: f64 = 365.2425;
/// A period's average, share or ratio is shown only when it rests on at
/// least this many values: fewer make noise, not a trend.
const MIN_VALUES: i64 = 10;
/// The first view of the period charts starts at the first span of this
/// many years that holds at least `DENSE_SHARE` of the tree's dated events,
/// so a few early records do not stretch the axis over empty centuries.
const DENSE_SPAN: i32 = 25;
const DENSE_SHARE: f64 = 0.01;
const DAYS_PER_MONTH: f64 = 30.436875;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ListTab {
    Births,
    Unions,
    Deaths,
    OldestAlive,
    LongestLives,
    LargestFamilies,
    Pyramid,
}

/// Reads a value this page keeps in the browser, when it keeps one.
async fn stored(key: &str) -> Option<String> {
    document::eval(&format!(
        "try {{ return localStorage.getItem('{key}'); }} catch (e) {{ return null; }}"
    ))
    .await
    .ok()
    .and_then(|v| v.as_str().map(str::to_string))
}

fn store(key: &str, value: &str) {
    document::eval(&format!(
        "try {{ localStorage.setItem('{key}', '{value}'); }} catch (e) {{}}"
    ));
}

#[component]
pub fn Statistics(tree_id: String) -> Element {
    let i18n = use_i18n();
    let language: Signal<Language> = use_context();
    let api = use_context::<ApiClient>();
    let nav = use_navigator();
    let tree_cache = use_tree_cache();
    let load_trace = use_ui_load_trace(UiPage::Statistics);
    let tid = tree_id.parse::<Uuid>().ok();

    let mut interval = use_signal(|| DEFAULT_INTERVAL);
    // Unknown until the browser answers, so the statistics are asked once,
    // with the viewer's own choice.
    let mut approximate = use_signal(|| None::<bool>);
    use_effect(move || {
        spawn(async move {
            if let Some(value) = stored(INTERVAL_STORAGE_KEY)
                .await
                .and_then(|s| s.parse::<i32>().ok())
                .filter(|v| INTERVALS.contains(v))
            {
                interval.set(value);
            }
            let chosen = stored(APPROXIMATE_STORAGE_KEY).await.as_deref() == Some("true");
            approximate.set(Some(chosen));
        });
    });
    let mut choose_interval = move |value: i32| {
        interval.set(value);
        store(INTERVAL_STORAGE_KEY, &value.to_string());
    };
    let mut choose_approximate = move |value: bool| {
        approximate.set(Some(value));
        store(APPROXIMATE_STORAGE_KEY, &value.to_string());
    };
    let tab = use_signal(|| ListTab::Births);

    let api_tree = api.clone();
    let tree = use_traced_resource(load_trace.clone(), "tree", move || {
        let api = api_tree.clone();
        let _gen = tree_cache.generation();
        async move { fetch_tree_cached(&api, &tree_cache, tid?).await.ok() }
    });
    // The person last shown in this tree, else its SOSA root.
    let current_person = use_current_person();
    let selected_person_id = tid.and_then(|tid| current_person.get(tid)).or_else(|| {
        tree.read()
            .as_ref()
            .and_then(|tree| tree.as_ref())
            .and_then(|tree| tree.sosa_root_person_id)
    });
    // The series come by year: the interval and the range of years only
    // regroup them here. Only the dates option and the language, which
    // names the places' countries, ask again.
    let api_stats = api.clone();
    let stats = use_traced_resource(load_trace.clone(), "statistics", move || {
        let api = api_stats.clone();
        let approximate = approximate();
        let lang = language().code();
        async move { api.tree_statistics(tid?, approximate?, lang).await.ok() }
    });
    let api_map = api.clone();
    let basemap = use_traced_resource(load_trace, "basemap", move || {
        let api = api_map.clone();
        async move { api.basemap().await.unwrap_or_default() }
    });
    let paths = use_memo(move || {
        basemap
            .read()
            .as_ref()
            .map(|countries| basemap_paths(countries))
            .unwrap_or_default()
    });

    let tree_name = tree
        .read()
        .as_ref()
        .and_then(|t| t.as_ref().map(|t| t.name.clone()))
        .unwrap_or_default();
    let stats_read = stats.read();
    let stats_value = stats_read.as_ref().and_then(Option::as_ref);

    rsx! {
        div { class: "sub-page stats-page",
            div { class: "td-topbar",
                nav { class: "td-bc",
                    Link { to: Route::Home {}, class: "td-bc-logo",
                        img {
                            src: crate::components::layout::LOGO_PNG_B64,
                            alt: "OxidGene",
                            class: "td-bc-logo-img",
                        }
                    }
                    if !tree_name.is_empty() {
                        Link {
                            to: Route::TreeDetail { tree_id: tree_id.clone(), person: None },
                            class: "td-bc-link",
                            "{tree_name}"
                        }
                        span { class: "td-bc-sep", "/" }
                    }
                    span { class: "td-bc-current", {i18n.t("stats.breadcrumb")} }
                }
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
                label { class: "stats-interval",
                    {i18n.t("stats.interval")}
                    select {
                        class: "td-select",
                        onchange: move |e: Event<FormData>| {
                            if let Ok(value) = e.value().parse::<i32>() {
                                choose_interval(value);
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
            }

            div { class: "pd-page-shell",
                TreeIconSidebar {
                    active_view: TreeSidebarView::None,
                    selected_person_id,
                    show_middle_separator: false,
                    show_add_person: false,
                    on_profile_view: {
                        let tree_id = tree_id.clone();
                        move |pid: Option<Uuid>| {
                            if let Some(pid) = pid {
                                nav.push(Route::PersonDetail {
                                    tree_id: tree_id.clone(),
                                    person_id: pid.to_string(),
                                });
                            }
                        }
                    },
                    on_pedigree_view: {
                        let tree_id = tree_id.clone();
                        move |pid: Option<Uuid>| {
                            nav.push(Route::TreeDetail {
                                tree_id: tree_id.clone(),
                                person: pid.map(|pid| pid.to_string()),
                            });
                        }
                    },
                    on_add_person: move |_| {},
                    on_dictionary: {
                        let tree_id = tree_id.clone();
                        move |_| {
                            nav.push(Route::Dictionary { tree_id: tree_id.clone() });
                        }
                    },
                    on_settings: {
                        let tree_id = tree_id.clone();
                        move |_| {
                            nav.push(Route::Settings { tree_id: tree_id.clone() });
                        }
                    },
                }

                div { class: "sub-page-content stats-content",
                    match stats_value {
                        None => rsx! { p { class: "stats-loading", {i18n.t("common.loading")} } },
                        Some(value) => rsx! {
                            {render_overview(value, &i18n)}
                            {render_places(value, paths, &i18n)}
                            {render_names(value, &i18n)}
                            PeriodCharts { stats, interval }
                            {render_distributions(value, &i18n)}
                            {render_extremes(value, &tree_id, &i18n)}
                            {render_lists(value, &tree_id, tab, &i18n)}
                        },
                    }
                }
            }
        }
    }
}

// ── Formatting ──────────────────────────────────────────────────────────

fn tenth(value: f64) -> String {
    format!("{value:.1}")
}

fn percent(i18n: &I18n, part: i64, whole: i64) -> String {
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

fn date_text(i18n: &I18n, date: Option<&StatDate>) -> String {
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
    rsx! {
        section { class: "stats-section",
            h2 { class: "stats-section-title", {i18n.t("stats.section.overview")} }
            div { class: "stats-tiles",
                {tile("stats.persons", stats.persons.to_string(), i18n.t("stats.persons"), sexes)}
                {count("stats.unions", stats.unions)}
                {count("stats.places", stats.places)}
                {count("stats.sources", stats.sources)}
                {tile("stats.years_covered", years, i18n.t("stats.years_covered"), Vec::new())}
                {count("stats.surnames", stats.surnames)}
                {count("stats.given_names", stats.given_names)}
            }
            div { class: "stats-tiles",
                {share("stats.dated_births", stats.dated_births)}
                {share("stats.dated_deaths", stats.dated_deaths)}
                {share("stats.without_parents", stats.without_parents)}
                {share("stats.without_children", stats.without_children)}
                {share("stats.without_union", stats.without_union)}
            }
            div { class: "stats-tiles",
                {summary_tile(i18n, "lifespan", &stats.lifespan.all, "stats.unit.years", by_sex(i18n, &stats.lifespan.men, &stats.lifespan.women))}
                {summary_tile(i18n, "first_union", &stats.first_union_age.all, "stats.unit.years", by_sex(i18n, &stats.first_union_age.men, &stats.first_union_age.women))}
                {summary_tile(i18n, "generation", &stats.generation_interval, "stats.unit.years", Vec::new())}
                {summary_tile(i18n, "family_size", &stats.family_size, "stats.unit.children", Vec::new())}
            }
        }
    }
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

fn render_places(stats: &TreeStatistics, paths: Memo<Vec<String>>, i18n: &I18n) -> Element {
    let i18n = *i18n;
    let area = |key: &str, entries: &[StatCount], total: i64, noun: &str| {
        donut_card_noted(
            &i18n,
            key,
            counts(entries),
            Some(i18n.t_plural(noun, total as usize)),
        )
    };
    rsx! {
        section { class: "stats-section",
            h2 { class: "stats-section-title", {i18n.t("stats.section.places")} }
            div { class: "stats-places",
                if stats.located_places.is_empty() {
                    p { class: "stats-empty", {i18n.t("stats.map_empty")} }
                } else {
                    HeatMap {
                        paths,
                        places: stats.located_places.clone(),
                        top: stats.top_places.clone(),
                        i18n,
                    }
                }
                div { class: "stats-top-places",
                    h3 { class: "stats-card-title", {i18n.t("stats.top_places")} }
                    ol {
                        for place in stats.top_places.iter() {
                            li { key: "{place.place_id}",
                                span { class: "stats-top-place-name", "{place.name}" }
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
            div { class: "stats-grid stats-grid-3",
                {area("births_by_country", &stats.births_by_country, stats.countries, "stats.countries")}
                {area("births_by_region", &stats.births_by_region, stats.regions, "stats.regions")}
                {area("births_by_subdivision", &stats.births_by_subdivision, stats.subdivisions, "stats.subdivisions")}
            }
        }
    }
}

// ── Names ───────────────────────────────────────────────────────────────

fn render_names(stats: &TreeStatistics, i18n: &I18n) -> Element {
    let i18n = *i18n;
    let rare = [
        (
            "men",
            i18n.t("stats.series.men"),
            &stats.rare_given_names_men,
        ),
        (
            "women",
            i18n.t("stats.series.women"),
            &stats.rare_given_names_women,
        ),
    ];
    let no_rare = rare.iter().all(|(_, _, names)| names.is_empty());
    rsx! {
        section { class: "stats-section",
            h2 { class: "stats-section-title", {i18n.t("stats.section.names")} }
            div { class: "stats-grid",
                {donut_card(&i18n, "surnames", counts(&stats.top_surnames))}
                {donut_card(&i18n, "occupations", counts(&stats.top_occupations))}
                {donut_card(&i18n, "given_names_men", counts(&stats.top_given_names_men))}
                {donut_card(&i18n, "given_names_women", counts(&stats.top_given_names_women))}
            }
            ChartCard {
                title: i18n.t("stats.chart.rare_given_names"),
                hint: i18n.t("stats.hint.rare_given_names"),
                empty: no_rare,
                i18n,
                div { class: "stats-rare",
                    for (key, label, names) in rare {
                        div { key: "{key}", class: "stats-rare-column",
                            h4 { class: "stats-rare-title", "{label} ({names.len()})" }
                            p { class: "stats-rare-names", {names.join(", ")} }
                        }
                    }
                }
            }
        }
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
            LineChart { periods: periods.to_vec(), series, unit: i18n.t(unit) }
        }
    }
}

/// The period charts, under the ruler choosing the years they cover. The
/// range is kept here, so moving it redraws these charts only.
#[component]
fn PeriodCharts(stats: Resource<Option<TreeStatistics>>, interval: Signal<i32>) -> Element {
    let i18n = use_i18n();
    let mut chosen = use_signal(|| None::<(i32, i32)>);
    let stats_read = stats.read();
    let Some(stats) = stats_read.as_ref().and_then(Option::as_ref) else {
        return rsx! {};
    };
    let bounds = year_bounds(stats);
    let (first, last) = bounds.unwrap_or((0, -1));
    let (from, to) = match chosen() {
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
            if last > first {
                div { class: "stats-timeline",
                    span { class: "stats-timeline-title", {i18n.t("stats.years")} }
                    span { class: "stats-timeline-years",
                        {i18n.t_args("stats.years_range", &[("from", &from.to_string()), ("to", &to.to_string())])}
                    }
                    YearRuler {
                        min: first,
                        max: last,
                        from,
                        to,
                        interval: interval(),
                        from_label: i18n.t("stats.years_from"),
                        to_label: i18n.t("stats.years_to"),
                        on_change: move |range: (i32, i32)| chosen.set(Some(range)),
                    }
                    button {
                        class: "btn btn-outline btn-sm",
                        disabled: whole,
                        onclick: move |_| chosen.set(Some((first, last))),
                        {i18n.t("stats.years_all")}
                    }
                }
            }
            {render_persons(stats, &periods, &i18n)}
            {render_families(stats, &periods, &i18n)}
        }
    }
}

fn render_persons(stats: &TreeStatistics, periods: &Periods, i18n: &I18n) -> Element {
    let p = &periods.labels();
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
    rsx! {
        section { class: "stats-section",
            h2 { class: "stats-section-title", {i18n.t("stats.section.persons")} }
            div { class: "stats-grid",
                {line_card(i18n, "events", p, events, "stats.unit.events")}
                {line_card(i18n, "sex_ratio", p, sex_ratio, "stats.unit.per_100_women")}
                {line_card(i18n, "age_at_death", p, sex_series(i18n, periods, &stats.age_at_death.men, &stats.age_at_death.women), "stats.unit.years")}
                {line_card(i18n, "life_expectancy", p, sex_series(i18n, periods, &stats.life_expectancy.men, &stats.life_expectancy.women), "stats.unit.years")}
                {line_card(i18n, "mortality", p, mortality, "stats.unit.percent")}
                {line_card(i18n, "births_by_month", p, category_series(periods.shares(&stats.births_by_month, 12), &months(i18n)), "stats.unit.percent")}
                {line_card(i18n, "parents_age", p, parent_series, "stats.unit.years")}
                {line_card(i18n, "generation_interval", p, generation, "stats.unit.years")}
            }
        }
    }
}

fn render_families(stats: &TreeStatistics, periods: &Periods, i18n: &I18n) -> Element {
    let p = &periods.labels();
    let single = |key: &str, years: &[StatYearSum]| {
        single(i18n.t(&format!("stats.chart.{key}")), periods, years)
    };
    rsx! {
        section { class: "stats-section",
            h2 { class: "stats-section-title", {i18n.t("stats.section.families")} }
            div { class: "stats-grid",
                {line_card(i18n, "age_at_first_union", p, sex_series(i18n, periods, &stats.age_at_first_union.men, &stats.age_at_first_union.women), "stats.unit.years")}
                {line_card(i18n, "unions_by_weekday", p, category_series(periods.shares(&stats.unions_by_weekday, 7), &weekdays(i18n)), "stats.unit.percent")}
                {line_card(i18n, "unions_by_month", p, category_series(periods.shares(&stats.unions_by_month, 12), &months(i18n)), "stats.unit.percent")}
                {line_card(i18n, "union_duration", p, single("union_duration", &stats.union_duration), "stats.unit.years")}
                {line_card(i18n, "children_per_union", p, single("children_per_union", &stats.children_per_union), "stats.unit.children")}
                {line_card(i18n, "birth_spacing", p, single("birth_spacing", &stats.birth_spacing), "stats.unit.months")}
                {line_card(i18n, "first_last_child_gap", p, single("first_last_child_gap", &stats.first_last_child_gap), "stats.unit.months")}
                {line_card(i18n, "spouse_age_gap", p, single("spouse_age_gap", &stats.spouse_age_gap), "stats.unit.months")}
            }
        }
    }
}

// ── Distributions ───────────────────────────────────────────────────────

fn event_type_label(i18n: &I18n, label: &str) -> String {
    serde_json::from_value::<EventType>(serde_json::Value::String(label.to_string()))
        .map(|kind| i18n.t(event_type_label_key(kind)))
        .unwrap_or_else(|_| label.to_string())
}

fn render_distributions(stats: &TreeStatistics, i18n: &I18n) -> Element {
    let i18n = *i18n;
    let total: i64 = stats.event_types.iter().map(|e| e.count).sum();
    let types: Vec<(String, i64)> = stats
        .event_types
        .iter()
        .take(10)
        .map(|e| (event_type_label(&i18n, &e.label), e.count))
        .collect();
    let children: Vec<(String, i64)> = stats
        .children_histogram
        .iter()
        .enumerate()
        .filter(|(_, unions)| **unions > 0)
        .map(|(n, unions)| (n.to_string(), *unions))
        .collect();
    rsx! {
        section { class: "stats-section",
            h2 { class: "stats-section-title", {i18n.t("stats.section.events")} }
            div { class: "stats-grid",
                {donut_card_noted(&i18n, "event_types", types, Some(i18n.t_plural("stats.events_total", total as usize)))}
                ChartCard {
                    title: i18n.t("stats.chart.children_histogram"),
                    hint: i18n.t("stats.hint.children_histogram"),
                    empty: children.is_empty(),
                    i18n,
                    BarChart { items: children }
                }
            }
        }
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

fn person_links(tree_id: &str, persons: &[StatPersonRef], separator: &str) -> Element {
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
        (ListTab::Pyramid, "stats.records.pyramid"),
    ];
    let person_link = |p: &StatPerson| Route::PersonDetail {
        tree_id: tree_id.to_string(),
        person_id: p.person_id.to_string(),
    };
    let body = match tab() {
        ListTab::Pyramid => rsx! {
            if stats.pyramid.is_empty() {
                p { class: "stats-empty", {i18n.t("stats.empty")} }
            } else {
                Pyramid {
                    bands: stats.pyramid.clone(),
                    men: i18n.t("stats.series.men"),
                    women: i18n.t("stats.series.women"),
                }
            }
        },
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
            div { class: "dict-tabs",
                for (value, key) in tabs {
                    button {
                        key: "{key}",
                        class: if tab() == value { "dict-tab active" } else { "dict-tab" },
                        onclick: move |_| tab.set(value),
                        {i18n.t(key)}
                    }
                }
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
        let i18n = I18n(Language::En);
        assert_eq!(duration(&i18n, 1.0), "1 day");
        assert_eq!(duration(&i18n, 61.0), "2 months");
        assert_eq!(duration(&i18n, 25567.0), "70 years");
    }
}
