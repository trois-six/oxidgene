//! Full-page search results powered by the server-side cache search index.
//!
//! Combines server-side accent-folded name matching with lightweight
//! client-side filters (gender, date range), sorting, and pagination.

use dioxus::prelude::*;
use oxidgene_cache::types::SearchEntry;
use oxidgene_core::Sex;
use uuid::Uuid;

use crate::api::ApiClient;
use crate::i18n::use_i18n;
use crate::router::Route;

const RESULTS_PER_PAGE: usize = 25;
/// How many results to request from the server (enough for client-side filtering).
const SERVER_LIMIT: u32 = 500;

// ── Enums ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
enum SortOrder {
    Relevance,
    NameAZ,
    NameZA,
    BirthAsc,
    BirthDesc,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum GenderFilter {
    All,
    Male,
    Female,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ViewMode {
    List,
    Card,
}

// ── Component Props ──────────────────────────────────────────────────────

#[derive(Props, Clone, PartialEq)]
pub struct SearchResultsProps {
    pub tree_id: String,
    #[props(default = String::new())]
    pub last: String,
    #[props(default = String::new())]
    pub first: String,
}

// ── SearchResults Component ──────────────────────────────────────────────

#[component]
pub fn SearchResults(props: SearchResultsProps) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let nav = navigator();

    let tree_id = Uuid::parse_str(&props.tree_id).ok();

    // ── Search query state ──
    let mut committed_query = use_signal(|| {
        let parts: Vec<&str> = [props.last.as_str(), props.first.as_str()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect();
        parts.join(" ")
    });

    // ── Filter/sort/view state ──
    let mut gender_filter = use_signal(|| GenderFilter::All);
    let mut sort_order = use_signal(|| SortOrder::Relevance);
    let mut view_mode = use_signal(|| ViewMode::List);
    let mut current_page = use_signal(|| 1_usize);
    let mut show_filters = use_signal(|| false);
    let mut born_from = use_signal(String::new);
    let mut born_to = use_signal(String::new);
    let mut died_from = use_signal(String::new);
    let mut died_to = use_signal(String::new);

    // ── Server-side search ──
    let api_search = api.clone();
    let search_resource = use_resource(move || {
        let api = api_search.clone();
        let q = committed_query();
        async move {
            let Some(tid) = tree_id else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: "Invalid tree ID".into(),
                });
            };
            api.cache_search(tid, &q, SERVER_LIMIT, 0).await
        }
    });

    // ── Apply client-side filters, sort, and paginate ──
    let all_entries: Vec<SearchEntry> = {
        let data = search_resource.read();
        match &*data {
            Some(Ok(sr)) => sr.entries.clone(),
            _ => vec![],
        }
    };

    // 1) Gender filter
    let gender = gender_filter();
    let after_gender: Vec<&SearchEntry> = all_entries
        .iter()
        .filter(|e| match gender {
            GenderFilter::All => true,
            GenderFilter::Male => e.sex == Sex::Male,
            GenderFilter::Female => e.sex == Sex::Female,
            GenderFilter::Unknown => e.sex == Sex::Unknown,
        })
        .collect();

    // 2) Date range filters
    let bf = born_from().parse::<i32>().ok();
    let bt = born_to().parse::<i32>().ok();
    let df = died_from().parse::<i32>().ok();
    let dt = died_to().parse::<i32>().ok();

    let after_dates: Vec<&SearchEntry> = after_gender
        .into_iter()
        .filter(|e| {
            let by = e.birth_year.as_ref().and_then(|y| y.parse::<i32>().ok());
            let dy = e.death_year.as_ref().and_then(|y| y.parse::<i32>().ok());
            if bf.is_some_and(|min| by.is_none_or(|y| y < min)) {
                return false;
            }
            if bt.is_some_and(|max| by.is_none_or(|y| y > max)) {
                return false;
            }
            if df.is_some_and(|min| dy.is_none_or(|y| y < min)) {
                return false;
            }
            if dt.is_some_and(|max| dy.is_none_or(|y| y > max)) {
                return false;
            }
            true
        })
        .collect();

    // 3) Sort
    let sort = sort_order();
    let mut sorted: Vec<&SearchEntry> = after_dates;
    match sort {
        SortOrder::Relevance => {} // Server already returns relevance order
        SortOrder::NameAZ => sorted.sort_by(|a, b| {
            a.surname_normalized
                .cmp(&b.surname_normalized)
                .then(a.given_names_normalized.cmp(&b.given_names_normalized))
        }),
        SortOrder::NameZA => sorted.sort_by(|a, b| {
            b.surname_normalized
                .cmp(&a.surname_normalized)
                .then(b.given_names_normalized.cmp(&a.given_names_normalized))
        }),
        SortOrder::BirthAsc => sorted.sort_by(|a, b| a.date_sort.cmp(&b.date_sort)),
        SortOrder::BirthDesc => sorted.sort_by(|a, b| b.date_sort.cmp(&a.date_sort)),
    }

    // 4) Paginate
    let total_filtered = sorted.len();
    let page = current_page();
    let total_pages = (total_filtered + RESULTS_PER_PAGE - 1).max(1) / RESULTS_PER_PAGE.max(1);
    let start = (page - 1) * RESULTS_PER_PAGE;
    let page_results: Vec<&SearchEntry> = sorted
        .into_iter()
        .skip(start)
        .take(RESULTS_PER_PAGE)
        .collect();

    let is_loading = search_resource.read().is_none();
    let is_error = matches!(&*search_resource.read(), Some(Err(_)));
    let tree_id_str = props.tree_id.clone();

    // ── Render ──
    rsx! {
        div { class: "search-results-page",
            // ── Topbar ──
            div { class: "sr-topbar",
                div { class: "sr-breadcrumb",
                    Link { to: Route::Home {}, class: "breadcrumb-link",
                        {i18n.t("nav.my_trees")}
                    }
                    span { class: "breadcrumb-sep", " / " }
                    Link {
                        to: Route::TreeDetail { tree_id: tree_id_str.clone(), person: None },
                        class: "breadcrumb-link",
                        {i18n.t("nav.tree")}
                    }
                    span { class: "breadcrumb-sep", " / " }
                    span { class: "breadcrumb-current", {i18n.t("search.title")} }
                }
                // Inline search bar
                SearchBar {
                    initial_query: committed_query(),
                    on_search: move |q: String| {
                        committed_query.set(q);
                        current_page.set(1);
                    },
                    on_back: move |_| {
                        nav.push(Route::TreeDetail { tree_id: tree_id_str.clone(), person: None });
                    },
                }
            }

            // ── Filter panel ──
            div { class: "sr-filter-bar",
                button {
                    class: "btn btn-outline btn-sm",
                    onclick: move |_| show_filters.set(!show_filters()),
                    if show_filters() { "\u{25B2} " } else { "\u{25BC} " }
                    {i18n.t("search.filters")}
                }
                if show_filters() {
                    div { class: "sr-filters",
                        div { class: "sr-filter-group",
                            label { {i18n.t("search.gender")} }
                            select {
                                value: "{gender_filter():?}",
                                onchange: move |e: Event<FormData>| {
                                    gender_filter.set(match e.value().as_str() {
                                        "Male" => GenderFilter::Male,
                                        "Female" => GenderFilter::Female,
                                        "Unknown" => GenderFilter::Unknown,
                                        _ => GenderFilter::All,
                                    });
                                    current_page.set(1);
                                },
                                option { value: "All", {i18n.t("search.all")} }
                                option { value: "Male", {i18n.t("search.male")} }
                                option { value: "Female", {i18n.t("search.female")} }
                                option { value: "Unknown", {i18n.t("search.unknown")} }
                            }
                        }
                        div { class: "sr-filter-group",
                            label { {i18n.t("search.born_between")} }
                            input {
                                r#type: "number",
                                placeholder: "1800",
                                value: "{born_from}",
                                oninput: move |e: Event<FormData>| {
                                    born_from.set(e.value());
                                    current_page.set(1);
                                },
                            }
                            span { " – " }
                            input {
                                r#type: "number",
                                placeholder: "2000",
                                value: "{born_to}",
                                oninput: move |e: Event<FormData>| {
                                    born_to.set(e.value());
                                    current_page.set(1);
                                },
                            }
                        }
                        div { class: "sr-filter-group",
                            label { {i18n.t("search.died_between")} }
                            input {
                                r#type: "number",
                                placeholder: "1800",
                                value: "{died_from}",
                                oninput: move |e: Event<FormData>| {
                                    died_from.set(e.value());
                                    current_page.set(1);
                                },
                            }
                            span { " – " }
                            input {
                                r#type: "number",
                                placeholder: "2000",
                                value: "{died_to}",
                                oninput: move |e: Event<FormData>| {
                                    died_to.set(e.value());
                                    current_page.set(1);
                                },
                            }
                        }
                        button {
                            class: "btn btn-outline btn-sm",
                            onclick: move |_| {
                                gender_filter.set(GenderFilter::All);
                                born_from.set(String::new());
                                born_to.set(String::new());
                                died_from.set(String::new());
                                died_to.set(String::new());
                                current_page.set(1);
                            },
                            {i18n.t("search.clear_filters")}
                        }
                    }
                }
            }

            // ── Sort / view controls ──
            div { class: "sr-sort-bar",
                span { class: "sr-count",
                    {format!("{} {}", total_filtered, i18n.t("search.results"))}
                }
                select {
                    class: "sr-sort-select",
                    value: "{sort_order():?}",
                    onchange: move |e: Event<FormData>| {
                        sort_order.set(match e.value().as_str() {
                            "NameAZ" => SortOrder::NameAZ,
                            "NameZA" => SortOrder::NameZA,
                            "BirthAsc" => SortOrder::BirthAsc,
                            "BirthDesc" => SortOrder::BirthDesc,
                            _ => SortOrder::Relevance,
                        });
                    },
                    option { value: "Relevance", {i18n.t("search.sort_relevance")} }
                    option { value: "NameAZ", {i18n.t("search.sort_name_az")} }
                    option { value: "NameZA", {i18n.t("search.sort_name_za")} }
                    option { value: "BirthAsc", {i18n.t("search.sort_birth_asc")} }
                    option { value: "BirthDesc", {i18n.t("search.sort_birth_desc")} }
                }
                div { class: "sr-view-toggle",
                    button {
                        class: if view_mode() == ViewMode::List { "active" } else { "" },
                        onclick: move |_| view_mode.set(ViewMode::List),
                        "\u{2630}" // ☰ list icon
                    }
                    button {
                        class: if view_mode() == ViewMode::Card { "active" } else { "" },
                        onclick: move |_| view_mode.set(ViewMode::Card),
                        "\u{25A6}" // ▦ card icon
                    }
                }
            }

            // ── Results ──
            if is_loading {
                div { class: "sr-loading", {i18n.t("search.loading")} }
            } else if is_error {
                div { class: "sr-error", {i18n.t("search.error")} }
            } else if page_results.is_empty() {
                div { class: "sr-empty",
                    p { {i18n.t("search.no_results")} }
                }
            } else {
                div {
                    class: if view_mode() == ViewMode::Card { "sr-results sr-card-grid" } else { "sr-results sr-list" },
                    for entry in page_results.iter() {
                        {render_result_item(entry, &props.tree_id, view_mode())}
                    }
                }
            }

            // ── Pagination ──
            if total_pages > 1 {
                div { class: "sr-pagination",
                    button {
                        class: "btn btn-sm",
                        disabled: page <= 1,
                        onclick: move |_| current_page.set(page.saturating_sub(1).max(1)),
                        "\u{25C0}"
                    }
                    for p in pagination_range(page, total_pages) {
                        if p == 0 {
                            span { class: "sr-page-ellipsis", "…" }
                        } else {
                            button {
                                class: if p == page { "btn btn-sm active" } else { "btn btn-sm" },
                                onclick: move |_| current_page.set(p),
                                "{p}"
                            }
                        }
                    }
                    button {
                        class: "btn btn-sm",
                        disabled: page >= total_pages,
                        onclick: move |_| current_page.set((page + 1).min(total_pages)),
                        "\u{25B6}"
                    }
                }
            }
        }
    }
}

// ── Result item rendering ────────────────────────────────────────────────

fn render_result_item(entry: &SearchEntry, tree_id: &str, view: ViewMode) -> Element {
    let sex_class = match entry.sex {
        Sex::Male => "male",
        Sex::Female => "female",
        Sex::Unknown => "",
    };

    let (given, surname) = match entry.display_name.rsplit_once(' ') {
        Some((g, s)) => (g.to_string(), s.to_string()),
        None => (entry.display_name.clone(), String::new()),
    };

    let initials: String = {
        let first_c = given.chars().next().map(|c| c.to_ascii_uppercase());
        let last_c = surname.chars().next().map(|c| c.to_ascii_uppercase());
        match (first_c, last_c) {
            (Some(f), Some(l)) => format!("{f}{l}"),
            (Some(f), None) => f.to_string(),
            (None, Some(l)) => l.to_string(),
            _ => "?".to_string(),
        }
    };

    let tree_id_str = tree_id.to_string();
    let person_id_str = entry.person_id.to_string();

    let item_class = if view == ViewMode::Card {
        format!("sr-item sr-card {sex_class}")
    } else {
        format!("sr-item sr-row {sex_class}")
    };

    rsx! {
        Link {
            to: Route::PersonDetail {
                tree_id: tree_id_str,
                person_id: person_id_str,
            },
            class: "{item_class}",
            div { class: "sr-item-photo",
                span { class: "sr-item-initials {sex_class}", "{initials}" }
            }
            div { class: "sr-item-info",
                div { class: "sr-item-name",
                    if !surname.is_empty() {
                        span { class: "sr-surname", "{surname}" }
                    }
                    span { class: "sr-given", " {given}" }
                    if surname.is_empty() && given.is_empty() {
                        span { class: "sr-given", "?" }
                    }
                }
                div { class: "sr-item-dates",
                    if let Some(ref by) = entry.birth_year {
                        span { class: "sr-birth", "\u{2726} {by}" }
                    }
                    if let Some(ref dy) = entry.death_year {
                        span { class: "sr-death", " \u{271D} {dy}" }
                    }
                }
                if let Some(ref bp) = entry.birth_place {
                    div { class: "sr-item-meta",
                        span { class: "sr-place", "{bp}" }
                    }
                }
            }
        }
    }
}

// ── SearchBar sub-component ──────────────────────────────────────────────

#[derive(Props, Clone, PartialEq)]
struct SearchBarProps {
    initial_query: String,
    on_search: EventHandler<String>,
    on_back: EventHandler<()>,
}

#[component]
fn SearchBar(props: SearchBarProps) -> Element {
    let i18n = use_i18n();
    let mut query = use_signal(|| props.initial_query.clone());

    rsx! {
        div { class: "sr-searchbar",
            input {
                r#type: "text",
                class: "sr-input",
                placeholder: "{i18n.t(\"search.placeholder\")}",
                value: "{query}",
                oninput: move |e: Event<FormData>| query.set(e.value()),
                onkeydown: move |e: Event<KeyboardData>| {
                    if e.key() == Key::Enter {
                        props.on_search.call(query());
                    } else if e.key() == Key::Escape {
                        props.on_back.call(());
                    }
                },
            }
            button {
                class: "btn btn-primary btn-sm",
                onclick: move |_| props.on_search.call(query()),
                {i18n.t("search.go")}
            }
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────

/// Build a pagination range with ellipsis (0 = ellipsis placeholder).
fn pagination_range(current: usize, total: usize) -> Vec<usize> {
    if total <= 7 {
        return (1..=total).collect();
    }
    let mut pages = Vec::new();
    pages.push(1);
    if current > 3 {
        pages.push(0); // ellipsis
    }
    let start = current.saturating_sub(1).max(2);
    let end = (current + 1).min(total - 1);
    for p in start..=end {
        pages.push(p);
    }
    if current < total - 2 {
        pages.push(0); // ellipsis
    }
    if *pages.last().unwrap_or(&0) != total {
        pages.push(total);
    }
    pages
}
