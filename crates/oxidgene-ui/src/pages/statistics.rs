//! Statistics page: where the tree's events happened, demographic charts per
//! period, and the notable records. See `docs/ui-statistics.md`.

use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::{ApiClient, StatDate, StatPerson, StatYearCounts, StatYearSum, TreeStatistics};
use crate::components::charts::{
    ChartCard, ChartSeries, DonutChart, HeatMap, LineChart, PALETTE, Pyramid, YearRuler,
    basemap_paths,
};
use crate::components::date_input::format_date;
use crate::components::tree_cache::{fetch_tree_cached, use_current_person, use_tree_cache};
use crate::components::tree_icon_sidebar::{TreeIconSidebar, TreeSidebarView};
use crate::i18n::{I18n, use_i18n};
use crate::router::Route;
use crate::ui_observability::{UiPage, use_traced_resource, use_ui_load_trace};

/// The period widths offered, in years.
const INTERVALS: [i32; 4] = [10, 25, 50, 100];
const DEFAULT_INTERVAL: i32 = 25;
const INTERVAL_STORAGE_KEY: &str = "oxidgene-stats-interval";

#[derive(Clone, Copy, PartialEq, Eq)]
enum RecordTab {
    Births,
    Unions,
    Deaths,
    OldestAlive,
    LongestLives,
    Pyramid,
}

#[component]
pub fn Statistics(tree_id: String) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let nav = use_navigator();
    let tree_cache = use_tree_cache();
    let load_trace = use_ui_load_trace(UiPage::Statistics);
    let tid = tree_id.parse::<Uuid>().ok();

    let mut interval = use_signal(|| DEFAULT_INTERVAL);
    // The viewer's last choice, when the browser keeps one.
    use_effect(move || {
        spawn(async move {
            let stored = document::eval(&format!(
                "try {{ return localStorage.getItem('{INTERVAL_STORAGE_KEY}'); }} catch (e) {{ return null; }}"
            ))
            .await;
            if let Some(value) = stored
                .ok()
                .and_then(|v| v.as_str().and_then(|s| s.parse::<i32>().ok()))
                .filter(|v| INTERVALS.contains(v))
            {
                interval.set(value);
            }
        });
    });
    let mut choose_interval = move |value: i32| {
        interval.set(value);
        document::eval(&format!(
            "try {{ localStorage.setItem('{INTERVAL_STORAGE_KEY}', '{value}'); }} catch (e) {{}}"
        ));
    };
    let tab = use_signal(|| RecordTab::Births);

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
    // Fetched once: the series come by year, and the interval and the
    // range of years only regroup them here.
    let api_stats = api.clone();
    let stats = use_traced_resource(load_trace.clone(), "statistics", move || {
        let api = api_stats.clone();
        async move { api.tree_statistics(tid?).await.ok() }
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
                            {render_summary(value, &i18n)}
                            {render_places(value, paths, &i18n)}
                            PeriodCharts { stats, interval }
                            {render_records(value, &tree_id, tab, &i18n)}
                        },
                    }
                }
            }
        }
    }
}

fn render_summary(stats: &TreeStatistics, i18n: &I18n) -> Element {
    let items = [
        ("stats.persons", stats.persons),
        ("stats.women", stats.women),
        ("stats.men", stats.men),
        ("stats.unions", stats.unions),
        ("stats.places", stats.places),
    ];
    rsx! {
        div { class: "stats-summary",
            for (key, value) in items {
                span { key: "{key}", class: "stats-summary-item",
                    b { "{value}" }
                    " {i18n.t(key)}"
                }
            }
        }
    }
}

fn render_places(stats: &TreeStatistics, paths: Memo<Vec<String>>, i18n: &I18n) -> Element {
    let i18n = *i18n;
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
        }
    }
}

fn counts(entries: &[crate::api::StatCount]) -> Vec<(String, i64)> {
    entries.iter().map(|e| (e.label.clone(), e.count)).collect()
}

/// The periods the charts share: `interval` years wide and aligned on its
/// multiples, from the first to the last year shown, the first and the
/// last period cut at those years (`docs/ui-statistics.md` §5).
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

    /// Each period's average; `None` where it has no value.
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
            .map(|(sum, count)| (count > 0).then(|| round1(sum / count as f64)))
            .collect()
    }

    /// Per category, each period's share of the counts, in percent; `None`
    /// where the period has no count.
    fn shares(&self, years: &[StatYearCounts], categories: usize) -> Vec<Vec<Option<f64>>> {
        let mut totals = vec![vec![0_i64; categories]; self.len()];
        for year in years {
            if let Some(slot) = self.index(year.year).and_then(|i| totals.get_mut(i)) {
                for (total, count) in slot.iter_mut().zip(&year.counts) {
                    *total += count;
                }
            }
        }
        (0..categories)
            .map(|k| {
                totals
                    .iter()
                    .map(|period| {
                        let all: i64 = period.iter().sum();
                        (all > 0).then(|| round1(period[k] as f64 * 100.0 / all as f64))
                    })
                    .collect()
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
        &stats.parents_age.father_first_child,
        &stats.parents_age.mother_first_child,
        &stats.parents_age.father_last_child,
        &stats.parents_age.mother_last_child,
        &stats.age_at_first_union.men,
        &stats.age_at_first_union.women,
        &stats.union_duration,
        &stats.children_per_union,
        &stats.birth_spacing,
        &stats.first_last_child_gap,
        &stats.spouse_age_gap,
    ];
    let counts = [
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

fn sex_series(
    i18n: &I18n,
    periods: &Periods,
    men: &[StatYearSum],
    women: &[StatYearSum],
) -> Vec<ChartSeries> {
    vec![
        ChartSeries {
            label: i18n.t("stats.series.men"),
            color: "var(--pn-male-line)",
            values: periods.averages(men),
        },
        ChartSeries {
            label: i18n.t("stats.series.women"),
            color: "var(--pn-female-line)",
            values: periods.averages(women),
        },
    ]
}

/// One series per category (month, weekday), as shares per period.
fn share_series(
    periods: &Periods,
    years: &[StatYearCounts],
    labels: &[String],
) -> Vec<ChartSeries> {
    periods
        .shares(years, labels.len())
        .into_iter()
        .zip(labels)
        .enumerate()
        .map(|(k, (values, label))| ChartSeries {
            label: label.clone(),
            color: PALETTE[k % PALETTE.len()],
            values,
        })
        .collect()
}

fn single(label: String, periods: &Periods, years: &[StatYearSum]) -> Vec<ChartSeries> {
    vec![ChartSeries {
        label,
        color: PALETTE[0],
        values: periods.averages(years),
    }]
}

fn no_data(series: &[ChartSeries]) -> bool {
    series.iter().all(|s| s.values.iter().all(Option::is_none))
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

fn donut_card(i18n: &I18n, key: &str, items: Vec<(String, i64)>) -> Element {
    let i18n = *i18n;
    rsx! {
        ChartCard {
            title: i18n.t(&format!("stats.chart.{key}")),
            hint: i18n.t(&format!("stats.hint.{key}")),
            empty: items.is_empty(),
            i18n,
            DonutChart { items }
        }
    }
}

/// The Persons and Families sections, under the ruler choosing the years
/// their period charts cover. The range is kept here, so moving it redraws
/// these charts only.
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
        Some((from, to)) => {
            let from = from.clamp(first, last.max(first));
            (from, to.clamp(from, last.max(from)))
        }
        None => (first, last),
    };
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
                        on_change: move |range: (i32, i32)| {
                            chosen.set((range != (first, last)).then_some(range));
                        },
                    }
                    button {
                        class: "btn btn-outline btn-sm",
                        disabled: chosen().is_none(),
                        onclick: move |_| chosen.set(None),
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
        ChartSeries {
            label: i18n.t("stats.series.father_first"),
            color: "var(--pn-male-line)",
            values: periods.averages(&parents.father_first_child),
        },
        ChartSeries {
            label: i18n.t("stats.series.mother_first"),
            color: "var(--pn-female-line)",
            values: periods.averages(&parents.mother_first_child),
        },
        ChartSeries {
            label: i18n.t("stats.series.father_last"),
            color: PALETTE[2],
            values: periods.averages(&parents.father_last_child),
        },
        ChartSeries {
            label: i18n.t("stats.series.mother_last"),
            color: PALETTE[3],
            values: periods.averages(&parents.mother_last_child),
        },
    ];
    rsx! {
        section { class: "stats-section",
            h2 { class: "stats-section-title", {i18n.t("stats.section.persons")} }
            div { class: "stats-grid",
                {donut_card(i18n, "surnames", counts(&stats.top_surnames))}
                {donut_card(i18n, "given_names", counts(&stats.top_given_names))}
                {line_card(i18n, "age_at_death", p, sex_series(i18n, periods, &stats.age_at_death.men, &stats.age_at_death.women), "stats.unit.years")}
                {donut_card(i18n, "occupations", counts(&stats.top_occupations))}
                {line_card(i18n, "births_by_month", p, share_series(periods, &stats.births_by_month, &months(i18n)), "stats.unit.percent")}
                {line_card(i18n, "parents_age", p, parent_series, "stats.unit.years")}
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
                {line_card(i18n, "unions_by_weekday", p, share_series(periods, &stats.unions_by_weekday, &weekdays(i18n)), "stats.unit.percent")}
                {line_card(i18n, "unions_by_month", p, share_series(periods, &stats.unions_by_month, &months(i18n)), "stats.unit.percent")}
                {line_card(i18n, "union_duration", p, single("union_duration", &stats.union_duration), "stats.unit.years")}
                {line_card(i18n, "children_per_union", p, single("children_per_union", &stats.children_per_union), "stats.unit.children")}
                {line_card(i18n, "birth_spacing", p, single("birth_spacing", &stats.birth_spacing), "stats.unit.months")}
                {line_card(i18n, "first_last_child_gap", p, single("first_last_child_gap", &stats.first_last_child_gap), "stats.unit.months")}
                {line_card(i18n, "spouse_age_gap", p, single("spouse_age_gap", &stats.spouse_age_gap), "stats.unit.months")}
            }
        }
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

fn render_records(
    stats: &TreeStatistics,
    tree_id: &str,
    mut tab: Signal<RecordTab>,
    i18n: &I18n,
) -> Element {
    let tabs = [
        (RecordTab::Births, "stats.records.births"),
        (RecordTab::Unions, "stats.records.unions"),
        (RecordTab::Deaths, "stats.records.deaths"),
        (RecordTab::OldestAlive, "stats.records.oldest_alive"),
        (RecordTab::LongestLives, "stats.records.longest_lives"),
        (RecordTab::Pyramid, "stats.records.pyramid"),
    ];
    let person_link = |p: &StatPerson| Route::PersonDetail {
        tree_id: tree_id.to_string(),
        person_id: p.person_id.to_string(),
    };
    let body = match tab() {
        RecordTab::Pyramid => rsx! {
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
        RecordTab::Unions => rsx! {
            table { class: "stats-table",
                tbody {
                    for union in stats.recent_unions.iter() {
                        tr { key: "{union.family_id}",
                            td {
                                for (k, spouse) in union.spouses.iter().enumerate() {
                                    if k > 0 { " & " }
                                    Link {
                                        to: Route::PersonDetail {
                                            tree_id: tree_id.to_string(),
                                            person_id: spouse.person_id.to_string(),
                                        },
                                        "{spouse.name}"
                                    }
                                }
                            }
                            td { "{date_text(i18n, Some(&union.date))}" }
                            td { class: "text-muted", "{union.place.clone().unwrap_or_default()}" }
                        }
                    }
                }
            }
        },
        other => {
            let (rows, with_age) = match other {
                RecordTab::Births => (&stats.recent_births, false),
                RecordTab::Deaths => (&stats.recent_deaths, false),
                RecordTab::OldestAlive => (&stats.oldest_possibly_alive, true),
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
        // 1810: two values summing to 100; 1820: one of 70; 1850 is outside.
        let years = [sum(1810, 100.0, 2), sum(1820, 70.0, 1), sum(1850, 10.0, 1)];
        assert_eq!(periods.averages(&years), vec![Some(56.7), None]);
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
                counts: vec![1, 0],
            },
            StatYearCounts {
                year: 1905,
                counts: vec![1, 2],
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
}
