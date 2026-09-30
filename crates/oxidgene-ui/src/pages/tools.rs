//! Tools page: one tab per tool a tree's author checks or works it with.
//! See `docs/ui-tools.md`.
//!
//! Each tab is its own component, mounted only while it is shown, so a tab
//! asks the server for its data when it is opened and never for another's.

use dioxus::prelude::*;
use oxidgene_core::calendar::to_jdn;
use oxidgene_core::enums::{Calendar, DateQualifier};
use uuid::Uuid;

use crate::api::{
    AncestorFacts, AncestryGeneration, Anomaly, AnomalyRule, ApiClient, DuplicatePair,
    StatPersonRef, StatPlace, UpdatePlaceBody,
};
use crate::components::copy_field::CopyField;
use crate::components::date_input::{DateInput, DateParts};
use crate::components::merge_dialog::MergeDialog;
use crate::components::pedigree_chart::format_lifespan;
use crate::components::place_input::PlaceInput;
use crate::components::print::PrintAction;
use crate::components::search_person::{PersonSearchSummary, render_person_search_summary};
use crate::components::tree_cache::{fetch_tree_cached, use_current_person, use_tree_cache};
use crate::components::tree_icon_sidebar::{TreeIconSidebar, TreeSidebarView};
use crate::date_words::{self, Form, YearStart, Ymd};
use crate::i18n::{I18n, Language, use_i18n};
use crate::pages::statistics::{date_text, event_type_label, percent};
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
    Anomalies,
    Places,
    Ancestry,
    Duplicates,
    Converter,
    Words,
}

impl ToolsTab {
    const ALL: [Self; 6] = [
        Self::Anomalies,
        Self::Places,
        Self::Ancestry,
        Self::Duplicates,
        Self::Converter,
        Self::Words,
    ];

    /// The tab's name in storage and in its label's i18n key.
    fn key(self) -> &'static str {
        match self {
            Self::Anomalies => "anomalies",
            Self::Places => "places",
            Self::Ancestry => "ancestry",
            Self::Duplicates => "duplicates",
            Self::Converter => "converter",
            Self::Words => "words",
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
                PrintAction {
                    tree_name: tree_name.clone(),
                    title: i18n.t("tools.breadcrumb"),
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
                        (Some(ToolsTab::Anomalies), Some(tid)) => rsx! {
                            Anomalies { tree_id: tid, tree_route: tree_id.clone() }
                        },
                        (Some(ToolsTab::Places), Some(tid)) => rsx! {
                            UnlocatedPlaces { tree_id: tid, tree_route: tree_id.clone() }
                        },
                        (Some(ToolsTab::Ancestry), Some(tid)) => rsx! {
                            Ancestry { tree_id: tid, tree_route: tree_id.clone() }
                        },
                        (Some(ToolsTab::Duplicates), Some(tid)) => rsx! {
                            Duplicates { tree_id: tid, tree_route: tree_id.clone() }
                        },
                        (Some(ToolsTab::Converter), _) => rsx! { DateConverter {} },
                        (Some(ToolsTab::Words), _) => rsx! { DateWords {} },
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

// ── Anomalies ───────────────────────────────────────────────────────────

/// The anomaly categories, in the order the catalogue gives them.
const CATEGORIES: [&str; 5] = ["dates", "filiation", "unions", "witnesses", "data_quality"];

/// A person of an anomaly: their name linked to their profile, and a link
/// centring the pedigree on them.
fn anomaly_person(i18n: &I18n, tree_route: &str, person: &StatPersonRef) -> Element {
    let name = if person.name.trim().is_empty() {
        i18n.t("tools.anomalies.unnamed")
    } else {
        person.name.clone()
    };
    let pedigree = i18n.t("tools.anomalies.open_pedigree");
    rsx! {
        span { class: "tools-person",
            Link {
                to: Route::PersonDetail {
                    tree_id: tree_route.to_string(),
                    person_id: person.person_id.to_string(),
                },
                "{name}"
            }
            Link {
                to: Route::TreeDetail {
                    tree_id: tree_route.to_string(),
                    person: Some(person.person_id.to_string()),
                },
                class: "tools-person-pedigree",
                title: "{pedigree}",
                "aria-label": "{pedigree}",
                "\u{2197}"
            }
        }
    }
}

/// What an anomaly measured, in words: an age, a gap, the event concerned,
/// the text recorded.
fn anomaly_detail(i18n: &I18n, rule: &str, anomaly: &Anomaly) -> String {
    let mut parts = Vec::new();
    if let Some(n) = anomaly.value {
        let n = n.max(0) as usize;
        let years = || i18n.t_plural("stats.age", n);
        let days = || i18n.t_plural("stats.days", n);
        let part = match rule {
            "lived_over_105" | "centenarian_before_1900" => {
                i18n.t_args("tools.anomalies.detail.died_aged", &[("age", &years())])
            }
            "parent_too_young" | "father_too_old" | "mother_too_old" => {
                i18n.t_args("tools.anomalies.detail.parent_aged", &[("age", &years())])
            }
            "union_too_young" | "union_over_100" => {
                i18n.t_args("tools.anomalies.detail.union_aged", &[("age", &years())])
            }
            "spouses_age_gap" | "siblings_far_apart" => {
                i18n.t_args("tools.anomalies.detail.born_apart", &[("gap", &years())])
            }
            "siblings_too_close" => {
                i18n.t_args("tools.anomalies.detail.born_apart", &[("gap", &days())])
            }
            "born_long_after_father_death" => i18n.t_args(
                "tools.anomalies.detail.after_father_death",
                &[("gap", &days())],
            ),
            "repeated_union" => i18n.t_plural("stats.count.unions", n),
            _ => n.to_string(),
        };
        parts.push(part);
    }
    if let Some(event_type) = &anomaly.event_type {
        parts.push(event_type_label(i18n, event_type));
    }
    if let Some(text) = &anomaly.text {
        parts.push(format!("\u{201C}{text}\u{201D}"));
    }
    parts.join(" · ")
}

/// One rule and what it found, folded under its title.
fn render_rule(i18n: &I18n, tree_route: &str, rule: &AnomalyRule) -> Element {
    let severity = i18n.t(&format!("tools.anomalies.severity.{}", rule.severity));
    let more = rule.count - rule.items.len() as i64;
    rsx! {
        details { key: "{rule.rule}", class: "tools-generation tools-rule",
            summary {
                span { class: "tools-severity tools-severity-{rule.severity}", "{severity}" }
                span { class: "tools-rule-title", {i18n.t(&format!("tools.anomalies.rule.{}", rule.rule))} }
                span { class: "stats-legend-count", "{rule.count}" }
            }
            p { class: "tools-intro tools-rule-hint", {i18n.t(&format!("tools.anomalies.hint.{}", rule.rule))} }
            table { class: "stats-table",
                tbody {
                    for (k, anomaly) in rule.items.iter().enumerate() {
                        tr { key: "{k}",
                            td { class: "tools-persons",
                                for (j, person) in anomaly.persons.iter().enumerate() {
                                    if j > 0 {
                                        span { class: "text-muted", " · " }
                                    }
                                    {anomaly_person(i18n, tree_route, person)}
                                }
                                if let Some(family_id) = anomaly.family_id {
                                    Link {
                                        to: Route::CoupleDetail {
                                            tree_id: tree_route.to_string(),
                                            family_id: family_id.to_string(),
                                        },
                                        class: "tools-couple-link",
                                        {i18n.t("tools.anomalies.open_couple")}
                                    }
                                }
                            }
                            td { class: "text-muted tools-detail", "{anomaly_detail(i18n, &rule.rule, anomaly)}" }
                        }
                    }
                }
            }
            if more > 0 {
                p { class: "stats-note", {i18n.t_plural("tools.anomalies.more", more as usize)} }
            }
        }
    }
}

/// The tree's anomalies by category, each rule folded with its count
/// (`docs/ui-tools.md` §3).
#[component]
fn Anomalies(tree_id: Uuid, tree_route: String) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let mut category = use_signal(|| None::<&'static str>);
    let anomalies = use_ui_resource("tools_anomalies", move || {
        let api = api.clone();
        async move { api.tree_anomalies(tree_id).await }
    });

    let body = match &*anomalies.read() {
        None => rsx! { p { class: "stats-loading", {i18n.t("common.loading")} } },
        Some(Err(_)) => rsx! { p { class: "error-msg", {i18n.t("tools.load_failed")} } },
        Some(Ok(result)) if result.rules.is_empty() => rsx! {
            p { class: "stats-empty", {i18n.t_plural("tools.anomalies.none", result.persons as usize)} }
        },
        Some(Ok(result)) => {
            let total = |cat: &str| -> i64 {
                result
                    .rules
                    .iter()
                    .filter(|r| r.category == cat)
                    .map(|r| r.count)
                    .sum()
            };
            rsx! {
                div { class: "stats-tiles tools-categories",
                    for cat in CATEGORIES {
                        button {
                            key: "{cat}",
                            class: if category() == Some(cat) { "stats-tile tools-category active" } else { "stats-tile tools-category" },
                            "aria-pressed": category() == Some(cat),
                            onclick: move |_| {
                                category.set(if category() == Some(cat) { None } else { Some(cat) });
                            },
                            span { class: "stats-tile-value", "{total(cat)}" }
                            span { class: "stats-tile-label", {i18n.t(&format!("tools.anomalies.category.{cat}"))} }
                        }
                    }
                }
                for cat in CATEGORIES.into_iter().filter(|c| category().is_none_or(|chosen| chosen == *c)) {
                    if total(cat) > 0 {
                        section { key: "{cat}", class: "tools-category-section",
                            h3 { class: "stats-card-title", {i18n.t(&format!("tools.anomalies.category.{cat}"))} }
                            for rule in result.rules.iter().filter(|r| r.category == cat) {
                                {render_rule(&i18n, &tree_route, rule)}
                            }
                        }
                    }
                }
            }
        }
    };

    rsx! {
        section { class: "stats-section",
            {heading(&i18n, "anomalies")}
            {body}
        }
    }
}

// ── Places not located ──────────────────────────────────────────────────

/// Shortest place text worth looking up in the dictionary for coordinates.
const MIN_LOOKUP_CHARS: usize = 3;

/// The places the statistics cannot locate, each with its uses and a way to
/// correct its name (`docs/ui-tools.md` §4).
#[component]
fn UnlocatedPlaces(tree_id: Uuid, tree_route: String) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let api_places = api.clone();
    let mut places = use_ui_resource("tools_unlocated_places", move || {
        let api = api_places.clone();
        async move { api.unlocated_places(tree_id).await }
    });
    let mut opened = use_signal(|| None::<Uuid>);
    let mut editing = use_signal(|| None::<Uuid>);
    let edited = use_signal(String::new);
    let mut saving = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    let api_usage = api.clone();
    let usage = use_ui_resource("tools_place_usage", move || {
        let api = api_usage.clone();
        let place = opened();
        async move {
            match place {
                Some(place) => Some((
                    place,
                    api.dictionary_place_usage(tree_id, place)
                        .await
                        .unwrap_or_default(),
                )),
                None => None,
            }
        }
    });

    let save = move |place: StatPlace| {
        let api = api.clone();
        let name = edited().trim().to_string();
        spawn(async move {
            if name.is_empty() {
                return;
            }
            saving.set(true);
            error.set(None);
            // A dictionary label brings its coordinates, so the place is
            // located from now on; any other name is saved as typed.
            let known = if name.chars().count() >= MIN_LOOKUP_CHARS {
                api.place_suggestions(i18n.0.code(), &name, 8)
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .find(|s| s.label == name)
            } else {
                None
            };
            let body = UpdatePlaceBody {
                name: Some(name),
                latitude: known.as_ref().map(|s| s.latitude),
                longitude: known.as_ref().map(|s| s.longitude),
            };
            match api.update_place(tree_id, place.place_id, &body).await {
                Ok(_) => {
                    editing.set(None);
                    places.restart();
                }
                Err(_) => error.set(Some(i18n.t("tools.places.save_failed"))),
            }
            saving.set(false);
        });
    };

    let body = match &*places.read() {
        None => rsx! { p { class: "stats-loading", {i18n.t("common.loading")} } },
        Some(Err(_)) => rsx! { p { class: "error-msg", {i18n.t("tools.load_failed")} } },
        Some(Ok(list)) if list.is_empty() => rsx! {
            p { class: "stats-empty", {i18n.t("tools.places.none")} }
        },
        Some(Ok(list)) => rsx! {
            p { class: "stats-note", {i18n.t_plural("stats.unlocated", list.len())} }
            table { class: "stats-table tools-places",
                thead {
                    tr {
                        th { {i18n.t("tools.places.place")} }
                        th { class: "tools-col-fact", {i18n.t("tools.places.uses")} }
                        th {}
                    }
                }
                tbody {
                    for place in list.iter().cloned() {
                        Fragment { key: "{place.place_id}",
                        tr {
                            td {
                                if editing() == Some(place.place_id) {
                                    div { class: "tools-place-edit",
                                        PlaceInput { value: edited, options: Vec::new() }
                                        button {
                                            class: "btn btn-primary btn-sm",
                                            disabled: saving() || edited().trim().is_empty(),
                                            onclick: {
                                                let place = place.clone();
                                                let save = save.clone();
                                                move |_| save(place.clone())
                                            },
                                            {i18n.t("common.save")}
                                        }
                                        button {
                                            class: "btn btn-outline btn-sm",
                                            disabled: saving(),
                                            onclick: move |_| editing.set(None),
                                            {i18n.t("common.cancel")}
                                        }
                                    }
                                    if let Some(message) = error() {
                                        div { class: "error-msg", "{message}" }
                                    }
                                } else {
                                    span { class: "tools-place-name", "{place.name}" }
                                }
                            }
                            td { class: "tools-col-fact", "{place.count}" }
                            td { class: "tools-place-actions",
                                button {
                                    class: "btn btn-outline btn-sm",
                                    "aria-expanded": opened() == Some(place.place_id),
                                    onclick: move |_| {
                                        opened.set(if opened() == Some(place.place_id) { None } else { Some(place.place_id) });
                                    },
                                    {i18n.t("tools.places.show_uses")}
                                }
                                if editing() != Some(place.place_id) {
                                    button {
                                        class: "btn btn-outline btn-sm",
                                        onclick: {
                                            let name = place.name.clone();
                                            let mut edited = edited;
                                            move |_| {
                                                edited.set(name.clone());
                                                error.set(None);
                                                editing.set(Some(place.place_id));
                                            }
                                        },
                                        {i18n.t("tools.places.correct")}
                                    }
                                }
                            }
                        }
                        // Who uses the place opens on a row of its own, the
                        // table's full width, rather than inside the name's
                        // cell, where it pushed the count and buttons apart.
                        if opened() == Some(place.place_id) {
                            tr { class: "tools-place-usage",
                                td { colspan: 3,
                                    {render_place_usage(&i18n, &tree_route, place.place_id, &usage.read())}
                                }
                            }
                        }
                        }
                    }
                }
            }
        },
    };

    rsx! {
        section { class: "stats-section",
            {heading(&i18n, "places")}
            {body}
        }
    }
}

/// The persons whose events or media name a place, each opening the
/// pedigree on them, as the dictionary lists them.
fn render_place_usage(
    i18n: &I18n,
    tree_route: &str,
    place: Uuid,
    usage: &Option<Option<(Uuid, Vec<crate::api::PersonUsageEntry>)>>,
) -> Element {
    match usage {
        Some(Some((shown, list))) if *shown == place => rsx! {
            div { class: "dict-accordion",
                for entry in list.iter() {
                    Link {
                        key: "{entry.person_id}",
                        to: Route::TreeDetail { tree_id: tree_route.to_string(), person: Some(entry.person_id.to_string()) },
                        class: "dict-accordion-item",
                        span { class: "dict-accordion-name",
                            {format!(
                                "{} {}",
                                entry.surname.clone().unwrap_or_default(),
                                entry.given_names.clone().unwrap_or_default()
                            ).trim().to_string()}
                        }
                        {
                            let (birth, death) = entry.lifespan_years();
                            let lifespan = format_lifespan(birth, death);
                            rsx! { span { class: "dict-accordion-dates", "{lifespan}" } }
                        }
                    }
                }
                if list.is_empty() {
                    div { class: "dict-accordion-empty", {i18n.t("dictionary.usage_empty")} }
                }
            }
        },
        _ => rsx! {
            div { class: "dict-accordion",
                div { class: "dict-accordion-empty", {i18n.t("common.loading")} }
            }
        },
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

/// A fact of an ancestor: recorded or missing. A living person's death is
/// not drawn at all: nothing is expected there.
fn fact(i18n: &I18n, key: &str, recorded: bool) -> Element {
    let (class, state) = if recorded {
        ("tools-fact tools-fact-yes", "tools.ancestry.recorded")
    } else {
        ("tools-fact tools-fact-no", "tools.ancestry.missing")
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
    let lacking =
        |entry: &&crate::api::AncestryEntry| entry.person.as_ref().is_none_or(|p| !complete(p));
    // Only a generation with something to fill opens by itself: a missing
    // ancestor, listed or only counted, or an ancestor lacking a fact.
    let open = row.implied_missing > 0 || row.entries.iter().any(|entry| lacking(&entry));
    let entries: Vec<_> = row
        .entries
        .iter()
        .filter(|entry| !only_missing || lacking(entry))
        .collect();
    if entries.is_empty() && row.implied_missing == 0 {
        return rsx! {};
    }
    rsx! {
        details { key: "{row.generation}", class: "tools-generation", open,
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
                                    // The pills lay out in a box inside the
                                    // cell: a flex `td` stops being a table
                                    // cell and its border leaves the row's.
                                    td {
                                        div { class: "tools-facts",
                                            {fact(i18n, "birth", person.has_birth)}
                                            if person.has_death || !person.living {
                                                {fact(i18n, "death", person.has_death)}
                                            }
                                            if row.generation > 1 {
                                                {fact(i18n, "union", person.has_union)}
                                            }
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

// ── Potential duplicates ────────────────────────────────────────────────

/// The score from which a pair reads as *very likely* one person.
const VERY_LIKELY_SCORE: i64 = 70;
/// The score from which a pair reads as *likely* one person.
const LIKELY_SCORE: i64 = 55;

/// How sure a pair's score makes it (`docs/ui-tools.md` §6).
fn confidence(score: i64) -> &'static str {
    match score {
        s if s >= VERY_LIKELY_SCORE => "very_likely",
        s if s >= LIKELY_SCORE => "likely",
        _ => "possible",
    }
}

/// The pairs of records of the tree that may be one person, each compared
/// and settled through the homonym picker: merged, or recorded as two
/// people (`docs/ui-tools.md` §6).
#[component]
fn Duplicates(tree_id: Uuid, tree_route: String) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let tree_cache = use_tree_cache();
    let api_pairs = api.clone();
    let mut duplicates = use_ui_resource("tools_duplicates", move || {
        let api = api_pairs.clone();
        async move { api.potential_duplicates(tree_id).await }
    });
    let mut comparing = use_signal(|| None::<DuplicatePair>);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    // Settled pairs leave the list; a merge also changes the tree.
    let mut settled = move |merged: bool| {
        comparing.set(None);
        if merged {
            tree_cache.invalidate();
        }
        duplicates.restart();
    };
    let keep_apart = move |pair: DuplicatePair| {
        let api = api.clone();
        spawn(async move {
            busy.set(true);
            error.set(None);
            match api
                .mark_persons_distinct(tree_id, pair.first.person_id, &[pair.second.person_id])
                .await
            {
                Ok(()) => settled(false),
                Err(_) => error.set(Some(i18n.t("homonym.distinct_failed"))),
            }
            busy.set(false);
        });
    };

    let body = match &*duplicates.read() {
        None => rsx! { p { class: "stats-loading", {i18n.t("common.loading")} } },
        Some(Err(_)) => rsx! { p { class: "error-msg", {i18n.t("tools.load_failed")} } },
        Some(Ok(result)) if result.pairs.is_empty() => rsx! {
            p { class: "stats-empty", {i18n.t("tools.duplicates.none")} }
        },
        Some(Ok(result)) => rsx! {
            p { class: "stats-note", {i18n.t_plural("tools.duplicates.count", result.count as usize)} }
            if let Some(message) = error() {
                div { class: "error-msg", "{message}" }
            }
            div { class: "tools-pairs",
                for pair in result.pairs.iter().cloned() {
                    div { key: "{pair.first.person_id}-{pair.second.person_id}", class: "stats-card tools-pair",
                        div { class: "tools-pair-head",
                            span { class: "tools-confidence tools-confidence-{confidence(pair.score)}",
                                {i18n.t(&format!("tools.duplicates.confidence.{}", confidence(pair.score)))}
                            }
                            span { class: "text-muted", "{pair.score}" }
                            span { class: "tools-reasons",
                                for reason in pair.reasons.iter() {
                                    span { key: "{reason}", class: "tools-reason",
                                        {i18n.t(&format!("tools.duplicates.reason.{reason}"))}
                                    }
                                }
                            }
                        }
                        div { class: "tools-pair-persons",
                            for entry in [&pair.first, &pair.second] {
                                Link {
                                    key: "{entry.person_id}",
                                    to: Route::PersonDetail {
                                        tree_id: tree_route.clone(),
                                        person_id: entry.person_id.to_string(),
                                    },
                                    class: "search-person-result tools-pair-person",
                                    {render_person_search_summary(&PersonSearchSummary::from(entry), None, &i18n)}
                                }
                            }
                        }
                        div { class: "tools-pair-actions",
                            button {
                                class: "btn btn-outline btn-sm",
                                disabled: busy(),
                                onclick: {
                                    let pair = pair.clone();
                                    let keep_apart = keep_apart.clone();
                                    move |_| keep_apart(pair.clone())
                                },
                                {i18n.t("tools.duplicates.not_duplicates")}
                            }
                            button {
                                class: "btn btn-primary btn-sm",
                                disabled: busy(),
                                onclick: {
                                    let pair = pair.clone();
                                    move |_| comparing.set(Some(pair.clone()))
                                },
                                {i18n.t("tools.duplicates.compare")}
                            }
                        }
                    }
                }
            }
        },
    };

    rsx! {
        section { class: "stats-section",
            {heading(&i18n, "duplicates")}
            {body}
        }
        if let Some(pair) = comparing() {
            // Comparing is the merge wizard opened on its comparison, with
            // "two different people" as its other answer.
            MergeDialog {
                tree_id,
                person_id: pair.first.person_id,
                other_id: Some(pair.second.person_id),
                on_close: move |_| comparing.set(None),
                on_merged: move |_| settled(true),
                on_distinct: move |_| settled(false),
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

// ── Date in words ───────────────────────────────────────────────────────

/// The date a text is written from: the first date entered, in the
/// Gregorian or Julian calendar it was entered in, any other calendar's
/// date read in the Gregorian one. `None` while the entry is not a date.
fn words_date(parts: &DateParts) -> Option<(Ymd, Calendar)> {
    if parts.is_empty() || parts.validate().is_some() {
        return None;
    }
    let parts = parts.resolved();
    let parts = match parts.calendar {
        Calendar::Gregorian | Calendar::Julian => parts,
        _ => parts.expressed_in(Calendar::Gregorian)?,
    };
    Some((
        Ymd {
            year: parts.year?,
            month: parts.month,
            day: parts.day.filter(|_| parts.month.is_some()),
        },
        parts.calendar,
    ))
}

/// A date written out in every interface language and in Latin, and a
/// written date read back into the date input (`docs/ui-tools.md` §8).
#[component]
fn DateWords() -> Element {
    let i18n = use_i18n();
    let mut parts = use_signal(DateParts::default);
    let mut form = use_signal(|| Form::Long);
    let mut start = use_signal(|| YearStart::January);
    let mut text = use_signal(String::new);
    let mut read_error = use_signal(|| None::<&'static str>);

    let entered = parts();
    let date = words_date(&entered);
    let styled = date.map(|(ymd, calendar)| (ymd.in_style(start()), calendar));
    let calendar_note = match (entered.calendar, date) {
        (Calendar::Hebrew | Calendar::FrenchRepublican, Some(_)) => {
            Some(i18n.t("tools.words.read_as_gregorian"))
        }
        _ => None,
    };

    let mut read = move || match date_words::read(&text()) {
        Ok(found) => {
            parts.set(DateParts {
                year: Some(found.year),
                month: found.month,
                day: found.day,
                ..DateParts::default()
            });
            read_error.set(None);
        }
        Err(error) => read_error.set(Some(error.key())),
    };

    let outputs = match styled {
        None if entered.is_empty() => {
            rsx! { p { class: "stats-empty", {i18n.t("tools.words.enter")} } }
        }
        None => rsx! { p { class: "stats-empty", {i18n.t("tools.words.out_of_range")} } },
        Some((ymd, calendar)) => match date_words::latin_parts(ymd) {
            None => rsx! { p { class: "stats-empty", {i18n.t("tools.words.out_of_range")} } },
            Some(latin) => rsx! {
                div { class: "stats-card copy-card tools-words",
                    for language in Language::ALL {
                        CopyField {
                            key: "{language.code()}",
                            label: language.native_name().to_string(),
                            value: date_words::written(language, ymd, form()).unwrap_or_default(),
                            multiline: false,
                        }
                    }
                    CopyField {
                        label: i18n.t("tools.words.latin"),
                        value: date_words::latin(ymd, form()).unwrap_or_default(),
                        multiline: false,
                    }
                    if let Some(reckoning) = date_words::latin_roman_reckoning(calendar, ymd) {
                        CopyField {
                            label: i18n.t("tools.words.roman_reckoning"),
                            value: reckoning,
                            multiline: false,
                        }
                    }
                }
                table { class: "stats-table tools-words-parts",
                    caption { {i18n.t("tools.words.parts")} }
                    tbody {
                        if let Some((day, word, numeral)) = &latin.day {
                            tr {
                                th { {i18n.t("tools.words.part.day")} }
                                td { "{day} = {word} = {numeral}" }
                            }
                        }
                        if let (Some((word, numeral)), Some(month)) = (&latin.month, ymd.month) {
                            tr {
                                th { {i18n.t("tools.words.part.month")} }
                                td { "{month} = {word} = {numeral}" }
                            }
                        }
                        tr {
                            th { {i18n.t("tools.words.part.year")} }
                            td { "{ymd.year} = {latin.year.0} = {latin.year.1}" }
                        }
                        tr {
                            th { {i18n.t("tools.words.part.calendar")} }
                            td {
                                {i18n.t(if calendar == Calendar::Julian { "calendar.julian" } else { "calendar.gregorian" })}
                                if start() == YearStart::Annunciation {
                                    " · "
                                    {i18n.t("tools.words.style.annunciation")}
                                }
                            }
                        }
                    }
                }
            },
        },
    };

    rsx! {
        section { class: "stats-section",
            {heading(&i18n, "words")}
            div { class: "stats-card tools-converter-input",
                DateInput { parts, i18n, on_change: move |_| read_error.set(None) }
            }
            div { class: "tools-controls",
                label { class: "stats-interval",
                    {i18n.t("tools.words.form")}
                    select {
                        value: if form() == Form::Long { "long" } else { "short" },
                        onchange: move |e| form.set(if e.value() == "short" { Form::Short } else { Form::Long }),
                        option { value: "long", {i18n.t("tools.words.form.long")} }
                        option { value: "short", {i18n.t("tools.words.form.short")} }
                    }
                }
                label { class: "stats-interval",
                    {i18n.t("tools.words.style")}
                    select {
                        value: if start() == YearStart::January { "january" } else { "annunciation" },
                        onchange: move |e| start.set(if e.value() == "annunciation" { YearStart::Annunciation } else { YearStart::January }),
                        option { value: "january", {i18n.t("tools.words.style.january")} }
                        option { value: "annunciation", {i18n.t("tools.words.style.annunciation")} }
                    }
                }
            }
            if let Some(note) = calendar_note {
                p { class: "stats-note", "{note}" }
            }
            {outputs}
            h3 { class: "stats-card-title tools-words-read-title", {i18n.t("tools.words.read_title")} }
            p { class: "tools-intro", {i18n.t("tools.words.read_intro")} }
            div { class: "tools-words-read",
                input {
                    r#type: "text",
                    value: "{text}",
                    placeholder: i18n.t("tools.words.read_placeholder"),
                    "aria-label": i18n.t("tools.words.read_title"),
                    oninput: move |e| {
                        text.set(e.value());
                        read_error.set(None);
                    },
                    onkeydown: move |e: Event<KeyboardData>| {
                        if e.key() == Key::Enter {
                            read();
                        }
                    },
                }
                button {
                    class: "btn btn-primary btn-sm",
                    disabled: text().trim().is_empty(),
                    onclick: move |_| read(),
                    {i18n.t("tools.words.read")}
                }
            }
            if let Some(key) = read_error() {
                div { class: "error-msg", {i18n.t(key)} }
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
    fn a_date_is_written_from_its_gregorian_or_julian_form() {
        let republican = DateParts {
            calendar: Calendar::FrenchRepublican,
            year: Some(2),
            month: Some(10),
            day: Some(25),
            ..Default::default()
        };
        assert_eq!(
            words_date(&republican),
            Some((
                Ymd {
                    year: 1794,
                    month: Some(7),
                    day: Some(13)
                },
                Calendar::Gregorian
            ))
        );
        let julian = DateParts {
            calendar: Calendar::Julian,
            year: Some(1650),
            month: Some(2),
            day: Some(2),
            ..Default::default()
        };
        assert_eq!(words_date(&julian).map(|(_, c)| c), Some(Calendar::Julian));
        assert_eq!(words_date(&DateParts::default()), None);
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
