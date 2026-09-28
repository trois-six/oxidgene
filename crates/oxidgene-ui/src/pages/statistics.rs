//! Statistics page: where the tree's events happened, demographic charts per
//! period, and the notable records. See `docs/ui-statistics.md`.

use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::{ApiClient, StatDate, StatPerson, StatShares, TreeStatistics};
use crate::components::charts::{
    ChartCard, ChartSeries, DonutChart, HeatMap, LineChart, PALETTE, Pyramid, basemap_paths,
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
    let api_stats = api.clone();
    let stats = use_traced_resource(load_trace.clone(), "statistics", move || {
        let api = api_stats.clone();
        let interval = interval();
        async move { api.tree_statistics(tid?, interval).await.ok() }
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
    let stats_value: Option<TreeStatistics> = stats.read().as_ref().and_then(|s| s.clone());

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
                        Some(stats) => rsx! {
                            {render_summary(&stats, &i18n)}
                            {render_places(&stats, paths, &i18n)}
                            {render_persons(&stats, &i18n)}
                            {render_families(&stats, &i18n)}
                            {render_records(&stats, &tree_id, tab, &i18n)}
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

fn sex_series(i18n: &I18n, men: &[Option<f64>], women: &[Option<f64>]) -> Vec<ChartSeries> {
    vec![
        ChartSeries {
            label: i18n.t("stats.series.men"),
            color: "var(--pn-male-line)",
            values: men.to_vec(),
        },
        ChartSeries {
            label: i18n.t("stats.series.women"),
            color: "var(--pn-female-line)",
            values: women.to_vec(),
        },
    ]
}

/// One series per category (month, weekday) out of per-period shares.
fn share_series(shares: &[StatShares], labels: &[String]) -> Vec<ChartSeries> {
    labels
        .iter()
        .enumerate()
        .map(|(k, label)| ChartSeries {
            label: label.clone(),
            color: PALETTE[k % PALETTE.len()],
            values: shares.iter().map(|s| s.values.get(k).copied()).collect(),
        })
        .collect()
}

fn single(label: String, values: &[Option<f64>]) -> Vec<ChartSeries> {
    vec![ChartSeries {
        label,
        color: PALETTE[0],
        values: values.to_vec(),
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

fn render_persons(stats: &TreeStatistics, i18n: &I18n) -> Element {
    let p = &stats.periods;
    let parents = &stats.parents_age;
    let parent_series = vec![
        ChartSeries {
            label: i18n.t("stats.series.father_first"),
            color: "var(--pn-male-line)",
            values: parents.father_first_child.clone(),
        },
        ChartSeries {
            label: i18n.t("stats.series.mother_first"),
            color: "var(--pn-female-line)",
            values: parents.mother_first_child.clone(),
        },
        ChartSeries {
            label: i18n.t("stats.series.father_last"),
            color: PALETTE[2],
            values: parents.father_last_child.clone(),
        },
        ChartSeries {
            label: i18n.t("stats.series.mother_last"),
            color: PALETTE[3],
            values: parents.mother_last_child.clone(),
        },
    ];
    rsx! {
        section { class: "stats-section",
            h2 { class: "stats-section-title", {i18n.t("stats.section.persons")} }
            div { class: "stats-grid",
                {donut_card(i18n, "surnames", counts(&stats.top_surnames))}
                {donut_card(i18n, "given_names", counts(&stats.top_given_names))}
                {line_card(i18n, "age_at_death", p, sex_series(i18n, &stats.age_at_death.men, &stats.age_at_death.women), "stats.unit.years")}
                {donut_card(i18n, "occupations", counts(&stats.top_occupations))}
                {line_card(i18n, "births_by_month", p, share_series(&stats.births_by_month, &months(i18n)), "stats.unit.percent")}
                {line_card(i18n, "parents_age", p, parent_series, "stats.unit.years")}
            }
        }
    }
}

fn render_families(stats: &TreeStatistics, i18n: &I18n) -> Element {
    let p = &stats.periods;
    rsx! {
        section { class: "stats-section",
            h2 { class: "stats-section-title", {i18n.t("stats.section.families")} }
            div { class: "stats-grid",
                {line_card(i18n, "age_at_first_union", p, sex_series(i18n, &stats.age_at_first_union.men, &stats.age_at_first_union.women), "stats.unit.years")}
                {line_card(i18n, "unions_by_weekday", p, share_series(&stats.unions_by_weekday, &weekdays(i18n)), "stats.unit.percent")}
                {line_card(i18n, "unions_by_month", p, share_series(&stats.unions_by_month, &months(i18n)), "stats.unit.percent")}
                {line_card(i18n, "union_duration", p, single(i18n.t("stats.chart.union_duration"), &stats.union_duration), "stats.unit.years")}
                {line_card(i18n, "children_per_union", p, single(i18n.t("stats.chart.children_per_union"), &stats.children_per_union), "stats.unit.children")}
                {line_card(i18n, "birth_spacing", p, single(i18n.t("stats.chart.birth_spacing"), &stats.birth_spacing), "stats.unit.months")}
                {line_card(i18n, "first_last_child_gap", p, single(i18n.t("stats.chart.first_last_child_gap"), &stats.first_last_child_gap), "stats.unit.months")}
                {line_card(i18n, "spouse_age_gap", p, single(i18n.t("stats.chart.spouse_age_gap"), &stats.spouse_age_gap), "stats.unit.months")}
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
