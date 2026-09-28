//! Tools page: one tab per tool a tree's author checks or works it with.
//! See `docs/ui-tools.md`.
//!
//! Each tab is its own component, mounted only while it is shown, so a tab
//! asks the server for its data when it is opened and never for another's.

use dioxus::prelude::*;
use oxidgene_core::calendar::to_jdn;
use oxidgene_core::enums::{Calendar, DateQualifier};
use uuid::Uuid;

use crate::api::{AncestorFacts, AncestryGeneration, ApiClient};
use crate::components::date_input::{DateInput, DateParts};
use crate::components::tree_cache::{fetch_tree_cached, use_current_person, use_tree_cache};
use crate::components::tree_icon_sidebar::{TreeIconSidebar, TreeSidebarView};
use crate::i18n::{I18n, use_i18n};
use crate::pages::statistics::{date_text, percent};
use crate::prefs::{store, stored};
use crate::router::Route;
use crate::ui_observability::{UiPage, use_traced_resource, use_ui_load_trace, use_ui_resource};

const TAB_STORAGE_KEY: &str = "oxidgene-tools-tab";
const GENERATIONS_STORAGE_KEY: &str = "oxidgene-tools-generations";
/// The ancestry depths offered, the root's generation included.
const GENERATION_CHOICES: std::ops::RangeInclusive<u32> = 4..=15;
const DEFAULT_GENERATIONS: u32 = 8;

/// The page's tabs, one per tool.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ToolsTab {
    Ancestry,
    Converter,
}

impl ToolsTab {
    const ALL: [Self; 2] = [Self::Ancestry, Self::Converter];

    /// The tab's name in storage and in its label's i18n key.
    fn key(self) -> &'static str {
        match self {
            Self::Ancestry => "ancestry",
            Self::Converter => "converter",
        }
    }

    fn parse(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tab| tab.key() == key)
    }
}

#[component]
pub fn Tools(tree_id: String) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let nav = use_navigator();
    let tree_cache = use_tree_cache();
    let load_trace = use_ui_load_trace(UiPage::Tools);
    let tid = tree_id.parse::<Uuid>().ok();

    // Unknown until the browser answers, so that no tab mounts, and asks
    // for its data, before the one the viewer left the page on.
    let mut tab = use_signal(|| None::<ToolsTab>);
    use_effect(move || {
        spawn(async move {
            let stored_tab = stored(TAB_STORAGE_KEY)
                .await
                .as_deref()
                .and_then(ToolsTab::parse);
            tab.set(Some(stored_tab.unwrap_or(ToolsTab::ALL[0])));
        });
    });
    let mut choose_tab = move |value: ToolsTab| {
        tab.set(Some(value));
        store(TAB_STORAGE_KEY, value.key());
    };

    let tree = use_traced_resource(load_trace, "tree", move || {
        let api = api.clone();
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
    let tree_name = tree
        .read()
        .as_ref()
        .and_then(|t| t.as_ref().map(|t| t.name.clone()))
        .unwrap_or_default();

    rsx! {
        div { class: "sub-page tools-page",
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
                    span { class: "td-bc-current", {i18n.t("tools.breadcrumb")} }
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

                div { class: "sub-page-content tools-content",
                    div { class: "dict-tabs stats-tabs", role: "tablist",
                        for choice in ToolsTab::ALL {
                            button {
                                key: "{choice.key()}",
                                role: "tab",
                                "aria-selected": tab() == Some(choice),
                                class: if tab() == Some(choice) { "dict-tab active" } else { "dict-tab" },
                                onclick: move |_| choose_tab(choice),
                                {i18n.t(&format!("tools.tab.{}", choice.key()))}
                            }
                        }
                    }
                    match (tab(), tid) {
                        (Some(ToolsTab::Ancestry), Some(tid)) => rsx! {
                            Ancestry { tree_id: tid, tree_route: tree_id.clone() }
                        },
                        (Some(ToolsTab::Converter), _) => rsx! { DateConverter {} },
                        _ => rsx! {},
                    }
                }
            }
        }
    }
}

/// A tab's title, with its explanation under it.
fn heading(i18n: &I18n, key: &str) -> Element {
    rsx! {
        h2 { class: "stats-section-title", {i18n.t(&format!("tools.{key}.title"))} }
        p { class: "tools-intro", {i18n.t(&format!("tools.{key}.intro"))} }
    }
}

// ── Ancestry completeness ───────────────────────────────────────────────

/// How complete a share is, for its bar: green above 70%, orange from 40%,
/// red below.
fn completeness_class(part: i64, whole: i64) -> &'static str {
    let share = if whole > 0 { part * 100 / whole } else { 0 };
    match share {
        s if s > 70 => "tools-bar tools-bar-good",
        s if s >= 40 => "tools-bar tools-bar-fair",
        _ => "tools-bar tools-bar-poor",
    }
}

/// A fact of an ancestor: recorded, missing, or, for a death, not due yet.
fn fact(i18n: &I18n, key: &str, recorded: bool, living: bool) -> Element {
    let (class, state) = match (recorded, living) {
        (true, _) => ("tools-fact tools-fact-yes", "tools.ancestry.recorded"),
        (false, true) => ("tools-fact tools-fact-living", "tools.ancestry.living"),
        (false, false) => ("tools-fact tools-fact-no", "tools.ancestry.missing"),
    };
    let label = i18n.t(&format!("tools.ancestry.fact.{key}"));
    let state = i18n.t(state);
    rsx! {
        span { class: "{class}", title: "{label}: {state}", "aria-label": "{label}: {state}",
            "{label}"
        }
    }
}

/// Generation by generation from the SOSA root, who is known and which
/// key facts are recorded (`docs/ui-tools.md` §3).
#[component]
fn Ancestry(tree_id: Uuid, tree_route: String) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    // Unknown until the browser answers, so the ancestry is asked once,
    // at the viewer's own depth.
    let mut generations = use_signal(|| None::<u32>);
    use_effect(move || {
        spawn(async move {
            let depth = stored(GENERATIONS_STORAGE_KEY)
                .await
                .and_then(|s| s.parse::<u32>().ok())
                .filter(|n| GENERATION_CHOICES.contains(n));
            generations.set(Some(depth.unwrap_or(DEFAULT_GENERATIONS)));
        });
    });
    let mut only_missing = use_signal(|| false);
    let ancestry = use_ui_resource("tools_ancestry", move || {
        let api = api.clone();
        let depth = generations();
        async move { Some(api.ancestry_completeness(tree_id, depth?).await) }
    });

    let body = match &*ancestry.read() {
        None | Some(None) => rsx! { p { class: "stats-loading", {i18n.t("common.loading")} } },
        Some(Some(Err(_))) => rsx! { p { class: "error-msg", {i18n.t("tools.load_failed")} } },
        Some(Some(Ok(result))) if result.root.is_none() => rsx! {
            div { class: "empty-state tools-empty",
                p { {i18n.t("tools.ancestry.no_root")} }
                Link {
                    to: Route::Settings { tree_id: tree_route.clone() },
                    class: "btn btn-outline btn-sm",
                    {i18n.t("tools.ancestry.choose_root")}
                }
            }
        },
        Some(Some(Ok(result))) => rsx! {
            table { class: "stats-table tools-generations",
                thead {
                    tr {
                        th { {i18n.t("tools.ancestry.generation")} }
                        th { {i18n.t("tools.ancestry.found")} }
                        th { class: "tools-col-fact", {i18n.t("tools.ancestry.fact.birth")} }
                        th { class: "tools-col-fact", {i18n.t("tools.ancestry.fact.death")} }
                        th { class: "tools-col-fact", {i18n.t("tools.ancestry.fact.union")} }
                    }
                }
                tbody {
                    for row in result.generations.iter() {
                        tr { key: "{row.generation}",
                            td { {i18n.t_args("tools.ancestry.generation_n", &[("n", &row.generation.to_string())])} }
                            td {
                                div { class: "tools-completeness",
                                    span { "{row.found} / {row.expected}" }
                                    span { class: "tools-bar-track",
                                        span {
                                            class: completeness_class(row.found, row.expected),
                                            style: "width: {row.found * 100 / row.expected.max(1)}%",
                                        }
                                    }
                                    span { class: "text-muted", "{percent(&i18n, row.found, row.expected)}" }
                                }
                            }
                            td { class: "tools-col-fact", "{row.with_birth}" }
                            td { class: "tools-col-fact", "{row.with_death + row.living}" }
                            td { class: "tools-col-fact",
                                if row.generation > 1 { "{row.with_union}" } else { "–" }
                            }
                        }
                    }
                }
            }
            label { class: "stats-option tools-filter",
                input {
                    r#type: "checkbox",
                    checked: only_missing(),
                    onchange: move |e: Event<FormData>| only_missing.set(e.checked()),
                }
                {i18n.t("tools.ancestry.only_missing")}
            }
            for row in result.generations.iter() {
                {render_generation(&i18n, &tree_route, row, only_missing())}
            }
        },
    };

    rsx! {
        section { class: "stats-section",
            {heading(&i18n, "ancestry")}
            div { class: "tools-controls",
                label { class: "stats-interval",
                    {i18n.t("tools.ancestry.depth")}
                    select {
                        value: "{generations().unwrap_or(DEFAULT_GENERATIONS)}",
                        disabled: generations().is_none(),
                        onchange: move |e| {
                            if let Ok(value) = e.value().parse::<u32>() {
                                generations.set(Some(value));
                                store(GENERATIONS_STORAGE_KEY, &value.to_string());
                            }
                        },
                        for n in GENERATION_CHOICES {
                            option { key: "{n}", value: "{n}", "{n}" }
                        }
                    }
                }
            }
            {body}
        }
    }
}

/// One generation's ancestors, in SOSA order; with `only_missing`, only the
/// missing ones and those missing a fact.
fn render_generation(
    i18n: &I18n,
    tree_route: &str,
    row: &AncestryGeneration,
    only_missing: bool,
) -> Element {
    let complete = |person: &AncestorFacts| {
        person.has_birth
            && (person.has_death || person.living)
            && (row.generation == 1 || person.has_union)
    };
    let entries: Vec<_> = row
        .entries
        .iter()
        .filter(|entry| !only_missing || entry.person.as_ref().is_none_or(|p| !complete(p)))
        .collect();
    if entries.is_empty() && row.implied_missing == 0 {
        return rsx! {};
    }
    rsx! {
        details { key: "{row.generation}", class: "tools-generation", open: row.generation <= 3,
            summary {
                {i18n.t_args("tools.ancestry.generation_n", &[("n", &row.generation.to_string())])}
                span { class: "text-muted", " · {row.found} / {row.expected}" }
            }
            table { class: "stats-table",
                tbody {
                    for entry in entries {
                        tr { key: "{entry.sosa}",
                            td { class: "tools-sosa", "{entry.sosa}" }
                            match &entry.person {
                                Some(person) => rsx! {
                                    td {
                                        Link {
                                            to: Route::PersonDetail {
                                                tree_id: tree_route.to_string(),
                                                person_id: person.person_id.to_string(),
                                            },
                                            "{person.name}"
                                        }
                                        div { class: "tools-dates text-muted",
                                            if person.birth.is_some() {
                                                span { "\u{2726} {date_text(i18n, person.birth.as_ref())}" }
                                            }
                                            if person.death.is_some() {
                                                span { "\u{271D} {date_text(i18n, person.death.as_ref())}" }
                                            }
                                        }
                                    }
                                    td { class: "tools-facts",
                                        {fact(i18n, "birth", person.has_birth, false)}
                                        {fact(i18n, "death", person.has_death, person.living)}
                                        if row.generation > 1 {
                                            {fact(i18n, "union", person.has_union, false)}
                                        }
                                    }
                                },
                                None => rsx! {
                                    td { class: "tools-missing", colspan: "2",
                                        {i18n.t("tools.ancestry.missing_ancestor")}
                                    }
                                },
                            }
                        }
                    }
                }
            }
            if row.implied_missing > 0 {
                p { class: "stats-note",
                    {i18n.t_plural("tools.ancestry.implied", row.implied_missing as usize)}
                }
            }
        }
    }
}

// ── Date converter ──────────────────────────────────────────────────────

/// The calendars a date can be entered in, in the order the date input
/// offers them.
const CALENDARS: [(Calendar, &str); 4] = [
    (Calendar::Gregorian, "calendar.gregorian"),
    (Calendar::Julian, "calendar.julian"),
    (Calendar::Hebrew, "calendar.hebrew"),
    (Calendar::FrenchRepublican, "calendar.french_republican"),
];

/// The weekday of a single, complete date, Monday being 1: the same in every
/// calendar, since they all count the same days.
fn weekday(parts: &DateParts) -> Option<u8> {
    let resolved = parts.resolved();
    if resolved.needs_second_date() || resolved.qualifier != DateQualifier::Exact {
        return None;
    }
    let jdn = to_jdn(
        resolved.calendar,
        resolved.year?,
        resolved.month?,
        resolved.day?,
    )?;
    // Julian Day 0 fell on a Monday.
    Some(jdn.rem_euclid(7) as u8 + 1)
}

/// A date entered in one calendar, shown in every calendar the application
/// records dates in (`docs/ui-tools.md` §7).
#[component]
fn DateConverter() -> Element {
    let i18n = use_i18n();
    let parts = use_signal(DateParts::default);
    let entered = parts();
    let usable = !entered.is_empty() && entered.validate().is_none();
    let day_of_week = usable.then(|| weekday(&entered)).flatten();

    rsx! {
        section { class: "stats-section",
            {heading(&i18n, "converter")}
            div { class: "stats-card tools-converter-input",
                DateInput { parts, i18n, on_change: move |_| {} }
            }
            div { class: "stats-tiles tools-converter-results",
                for (calendar, key) in CALENDARS {
                    {
                        let converted = usable.then(|| entered.expressed_in(calendar)).flatten();
                        let text = match (usable, converted) {
                            (false, _) => "–".to_string(),
                            (true, Some(date)) => date.literal(&i18n),
                            (true, None) => i18n.t("tools.converter.unexpressible"),
                        };
                        rsx! {
                            div {
                                key: "{key}",
                                class: if calendar == entered.calendar { "stats-tile tools-converter-result active" } else { "stats-tile tools-converter-result" },
                                span { class: "stats-tile-label", {i18n.t(key)} }
                                span { class: "tools-converter-date", "{text}" }
                                if calendar == entered.calendar && usable {
                                    span { class: "stats-tile-detail", {i18n.t("tools.converter.entered")} }
                                }
                            }
                        }
                    }
                }
            }
            if let Some(day) = day_of_week {
                p { class: "tools-converter-weekday",
                    {i18n.t_args("tools.converter.weekday", &[("day", &i18n.t(&format!("stats.weekday.{day}")))])}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_complete_date_has_a_weekday_in_every_calendar() {
        // 14 July 1789 was a Tuesday; 25 messidor an II a Sunday
        // (13 July 1794).
        let gregorian = DateParts {
            year: Some(1789),
            month: Some(7),
            day: Some(14),
            ..Default::default()
        };
        assert_eq!(weekday(&gregorian), Some(2));
        let republican = DateParts {
            calendar: Calendar::FrenchRepublican,
            year: Some(2),
            month: Some(10),
            day: Some(25),
            ..Default::default()
        };
        assert_eq!(weekday(&republican), Some(7));
    }

    #[test]
    fn a_partial_or_uncertain_date_has_no_weekday() {
        let year_only = DateParts {
            year: Some(1789),
            ..Default::default()
        };
        assert_eq!(weekday(&year_only), None);
        let about = DateParts {
            qualifier: DateQualifier::About,
            year: Some(1789),
            month: Some(7),
            day: Some(14),
            ..Default::default()
        };
        assert_eq!(weekday(&about), None);
    }
}
