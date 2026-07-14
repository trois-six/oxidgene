//! Dictionary page: read-only index of family names, sources, places, and
//! occupations across a tree, each paired with a usage count. See
//! `docs/specifications/ui-dictionary.md`.

use std::collections::HashSet;

use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::{
    ApiClient, ApiError, DictionaryEntry, PlaceDictionaryEntry, SourceDictionaryEntry,
};
use crate::components::tree_cache::{fetch_tree_cached, use_tree_cache};
use crate::components::tree_icon_sidebar::{TreeIconSidebar, TreeSidebarView};
use crate::i18n::{I18n, use_i18n};
use crate::router::Route;

/// Above this many filtered entries, selecting the "All" page size shows a
/// perf warning instead of silently rendering everything.
const LARGE_LIST_THRESHOLD: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DictTab {
    FamilyNames,
    Sources,
    Places,
    Occupations,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageSize {
    Fixed(usize),
    All,
}

impl PageSize {
    fn as_option(self) -> Option<usize> {
        match self {
            PageSize::Fixed(n) => Some(n),
            PageSize::All => None,
        }
    }
}

/// Identifies which row's usage accordion is currently expanded.
#[derive(Debug, Clone, PartialEq, Eq)]
enum UsageKey {
    Source(Uuid),
    Place(Uuid),
    Occupation(String),
}

#[component]
pub fn Dictionary(tree_id: String) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let nav = use_navigator();
    let tree_cache = use_tree_cache();

    let mut tree_id_parsed = use_signal(|| tree_id.parse::<Uuid>().ok());
    let new_parsed = tree_id.parse::<Uuid>().ok();
    let tree_changed = new_parsed != *tree_id_parsed.peek();
    if tree_changed {
        *tree_id_parsed.write() = new_parsed;
    }

    let mut active_tab = use_signal(|| DictTab::FamilyNames);
    let quick_filter = use_signal(String::new);
    let letter_filter = use_signal(|| None::<char>);
    let page_size = use_signal(|| PageSize::Fixed(25));
    let mut current_page = use_signal(|| 1_usize);
    let mut expanded = use_signal(|| None::<UsageKey>);

    // Reset filters/pagination/expansion when switching tabs.
    let mut prev_tab = use_signal(|| DictTab::FamilyNames);
    if prev_tab() != active_tab() {
        prev_tab.set(active_tab());
        quick_filter.clone().set(String::new());
        letter_filter.clone().set(None);
        current_page.set(1);
        expanded.set(None);
    }

    // ── Data fetching (one aggregation call per tab) ──
    let api_tree = api.clone();
    let mut tree_resource = use_resource(move || {
        let api = api_tree.clone();
        let tid = tree_id_parsed();
        let _gen = tree_cache.generation();
        async move {
            let tid = tid?;
            Some(fetch_tree_cached(&api, &tree_cache, tid).await)
        }
    });

    let api_fn = api.clone();
    let mut family_names_resource = use_resource(move || {
        let api = api_fn.clone();
        let tid = tree_id_parsed();
        async move {
            let Some(tid) = tid else {
                return Err(ApiError::Api {
                    status: 400,
                    body: "Invalid tree ID".to_string(),
                });
            };
            api.dictionary_family_names(tid).await
        }
    });

    let api_occ = api.clone();
    let mut occupations_resource = use_resource(move || {
        let api = api_occ.clone();
        let tid = tree_id_parsed();
        async move {
            let Some(tid) = tid else {
                return Err(ApiError::Api {
                    status: 400,
                    body: "Invalid tree ID".to_string(),
                });
            };
            api.dictionary_occupations(tid).await
        }
    });

    let api_src = api.clone();
    let mut sources_resource = use_resource(move || {
        let api = api_src.clone();
        let tid = tree_id_parsed();
        async move {
            let Some(tid) = tid else {
                return Err(ApiError::Api {
                    status: 400,
                    body: "Invalid tree ID".to_string(),
                });
            };
            api.dictionary_sources(tid).await
        }
    });

    let api_place = api.clone();
    let mut places_resource = use_resource(move || {
        let api = api_place.clone();
        let tid = tree_id_parsed();
        async move {
            let Some(tid) = tid else {
                return Err(ApiError::Api {
                    status: 400,
                    body: "Invalid tree ID".to_string(),
                });
            };
            api.dictionary_places(tid).await
        }
    });

    if tree_changed {
        tree_resource.restart();
        family_names_resource.restart();
        occupations_resource.restart();
        sources_resource.restart();
        places_resource.restart();
    }

    // ── Usage drill-down for the currently expanded row ──
    let api_usage = api.clone();
    let usage_resource = use_resource(move || {
        let api = api_usage.clone();
        let key = expanded();
        let tid = tree_id_parsed();
        async move {
            let (Some(key), Some(tid)) = (key, tid) else {
                return Vec::new();
            };
            let ids = match &key {
                UsageKey::Source(id) => api.dictionary_source_usage(tid, *id).await,
                UsageKey::Place(id) => api.dictionary_place_usage(tid, *id).await,
                UsageKey::Occupation(value) => api.dictionary_occupation_usage(tid, value).await,
            }
            .unwrap_or_default();

            let mut people: Vec<(Uuid, Option<String>)> = Vec::with_capacity(ids.len());
            for pid in ids {
                let names = api.list_person_names(tid, pid).await.unwrap_or_default();
                let primary = names.iter().find(|n| n.is_primary).or(names.first());
                let name = primary.map(|n| n.display_name()).filter(|s| !s.is_empty());
                people.push((pid, name));
            }
            people.sort_by(|a, b| a.1.cmp(&b.1));
            people
        }
    });

    // Resolve the name synchronously from the cache while the resource is
    // pending, so the breadcrumb never flashes a loading label.
    let tree_name = match &*tree_resource.read() {
        Some(Some(Ok(tree))) => tree.name.clone(),
        _ => tree_id_parsed()
            .and_then(|tid| tree_cache.tree(tid))
            .map(|tree| tree.name)
            .unwrap_or_default(),
    };
    let selected_person_id = match &*tree_resource.read() {
        Some(Some(Ok(tree))) => tree.sosa_root_person_id,
        _ => tree_id_parsed()
            .and_then(|tid| tree_cache.tree(tid))
            .and_then(|tree| tree.sosa_root_person_id),
    };

    // ── Render ──
    rsx! {
        div { class: "sub-page",
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
                    span { class: "td-bc-current", {i18n.t("dictionary.breadcrumb")} }
                }
            }

            div { class: "pd-page-shell",
            TreeIconSidebar {
                active_view: TreeSidebarView::None,
                selected_person_id: selected_person_id,
                show_middle_separator: false,
                show_add_person: false,
                show_dictionary: false,
                show_settings: true,
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
                on_dictionary: move |_| {},
                on_settings: {
                    let tree_id = tree_id.clone();
                    move |_| {
                        nav.push(Route::Settings {
                            tree_id: tree_id.clone(),
                        });
                    }
                },
            }

            div { class: "sub-page-content",
                div { class: "dict-tabs",
                    button {
                        class: if active_tab() == DictTab::FamilyNames { "dict-tab active" } else { "dict-tab" },
                        onclick: move |_| active_tab.set(DictTab::FamilyNames),
                        {i18n.t("dictionary.tab.family_names")}
                    }
                    button {
                        class: if active_tab() == DictTab::Sources { "dict-tab active" } else { "dict-tab" },
                        onclick: move |_| active_tab.set(DictTab::Sources),
                        {i18n.t("dictionary.tab.sources")}
                    }
                    button {
                        class: if active_tab() == DictTab::Places { "dict-tab active" } else { "dict-tab" },
                        onclick: move |_| active_tab.set(DictTab::Places),
                        {i18n.t("dictionary.tab.places")}
                    }
                    button {
                        class: if active_tab() == DictTab::Occupations { "dict-tab active" } else { "dict-tab" },
                        onclick: move |_| active_tab.set(DictTab::Occupations),
                        {i18n.t("dictionary.tab.occupations")}
                    }
                }

                match active_tab() {
                    DictTab::FamilyNames => render_value_tab(
                        i18n,
                        &tree_id,
                        family_names_resource,
                        quick_filter,
                        letter_filter,
                        page_size,
                        current_page,
                        "dictionary.no_entries_family_names",
                        true,
                        expanded,
                        usage_resource,
                    ),
                    DictTab::Occupations => render_value_tab(
                        i18n,
                        &tree_id,
                        occupations_resource,
                        quick_filter,
                        letter_filter,
                        page_size,
                        current_page,
                        "dictionary.no_entries_occupations",
                        false,
                        expanded,
                        usage_resource,
                    ),
                    DictTab::Sources => render_sources_tab(
                        i18n,
                        &tree_id,
                        sources_resource,
                        quick_filter,
                        letter_filter,
                        page_size,
                        current_page,
                        expanded,
                        usage_resource,
                    ),
                    DictTab::Places => render_places_tab(
                        i18n,
                        &tree_id,
                        places_resource,
                        quick_filter,
                        letter_filter,
                        page_size,
                        current_page,
                        expanded,
                        usage_resource,
                    ),
                }
            }
            }
        }
    }
}

// ── Shared filter/pagination helpers ─────────────────────────────────────

fn matches_filters(label: &str, quick: &str, letter: Option<char>) -> bool {
    if let Some(l) = letter {
        let first = label.chars().next().map(|c| c.to_ascii_uppercase());
        if first != Some(l) {
            return false;
        }
    }
    if !quick.is_empty() && !label.to_lowercase().contains(&quick.to_lowercase()) {
        return false;
    }
    true
}

fn available_letters<'a>(labels: impl Iterator<Item = &'a str>) -> HashSet<char> {
    labels
        .filter_map(|l| l.chars().next().map(|c| c.to_ascii_uppercase()))
        .collect()
}

/// Splits a sorted, already-filtered slice into `(header_letter, item)` pairs
/// — `header_letter` is `Some` only on the first row of each letter group
/// within the page, so consecutive rows sharing a letter don't repeat it.
fn with_headers<'a, T>(
    items: &[&'a T],
    label_of: impl Fn(&T) -> &str,
) -> Vec<(Option<char>, &'a T)> {
    let mut out = Vec::with_capacity(items.len());
    let mut prev: Option<char> = None;
    for &item in items {
        let letter = label_of(item)
            .chars()
            .next()
            .map(|c| c.to_ascii_uppercase());
        let header = if letter != prev { letter } else { None };
        prev = letter;
        out.push((header, item));
    }
    out
}

fn paginate<T>(items: Vec<T>, page: usize, per_page: Option<usize>) -> Vec<T> {
    match per_page {
        None => items,
        Some(pp) => {
            let start = page.saturating_sub(1) * pp;
            items.into_iter().skip(start).take(pp).collect()
        }
    }
}

fn total_pages(total: usize, per_page: Option<usize>) -> usize {
    match per_page {
        None => 1,
        Some(pp) => (total + pp - 1).max(1) / pp,
    }
}

/// Build a pagination range with ellipsis (0 = ellipsis placeholder).
fn pagination_range(current: usize, total: usize) -> Vec<usize> {
    if total <= 7 {
        return (1..=total).collect();
    }
    let mut pages = Vec::new();
    pages.push(1);
    if current > 3 {
        pages.push(0);
    }
    let start = current.saturating_sub(1).max(2);
    let end = (current + 1).min(total - 1);
    for p in start..=end {
        pages.push(p);
    }
    if current < total - 2 {
        pages.push(0);
    }
    if *pages.last().unwrap_or(&0) != total {
        pages.push(total);
    }
    pages
}

// ── Shared toolbar (alphabet index + quick filter + page size + count) ──

#[allow(clippy::too_many_arguments)]
fn render_toolbar(
    i18n: I18n,
    letters: &HashSet<char>,
    letter_filter: Signal<Option<char>>,
    mut current_page: Signal<usize>,
    quick_filter: Signal<String>,
    page_size: Signal<PageSize>,
    total_filtered: usize,
) -> Element {
    let mut quick_filter = quick_filter;
    let mut page_size = page_size;
    let mut letter_filter = letter_filter;
    rsx! {
        div { class: "dict-alphabet",
            button {
                class: if letter_filter().is_none() { "dict-letter-btn active" } else { "dict-letter-btn" },
                onclick: move |_| {
                    letter_filter.set(None);
                    current_page.set(1);
                },
                {i18n.t("dictionary.letter_all")}
            }
            for c in ('A'..='Z') {
                button {
                    key: "{c}",
                    class: if letter_filter() == Some(c) { "dict-letter-btn active" } else { "dict-letter-btn" },
                    disabled: !letters.contains(&c),
                    onclick: move |_| {
                        letter_filter.set(Some(c));
                        current_page.set(1);
                    },
                    "{c}"
                }
            }
        }
        div { class: "dict-filter-row",
            input {
                r#type: "text",
                class: "dict-filter-input",
                placeholder: "{i18n.t(\"dictionary.filter_placeholder\")}",
                value: "{quick_filter}",
                oninput: move |e: Event<FormData>| {
                    quick_filter.set(e.value());
                    current_page.set(1);
                },
            }
            span { class: "sr-count", {i18n.t_plural("dictionary.count", total_filtered)} }
            div { class: "dict-page-size",
                label { {i18n.t("dictionary.page_size")} }
                select {
                    value: match page_size() {
                        PageSize::Fixed(n) => n.to_string(),
                        PageSize::All => "all".to_string(),
                    },
                    onchange: move |e: Event<FormData>| {
                        page_size.set(match e.value().as_str() {
                            "50" => PageSize::Fixed(50),
                            "100" => PageSize::Fixed(100),
                            "all" => PageSize::All,
                            _ => PageSize::Fixed(25),
                        });
                        current_page.set(1);
                    },
                    option { value: "25", "25" }
                    option { value: "50", "50" }
                    option { value: "100", "100" }
                    option { value: "all", {i18n.t("dictionary.page_size_all")} }
                }
            }
        }
        if page_size() == PageSize::All && total_filtered > LARGE_LIST_THRESHOLD {
            div { class: "dict-warning",
                {i18n.t_args("dictionary.large_list_warning", &[("count", &total_filtered.to_string())])}
            }
        }
    }
}

fn render_pagination(mut current_page: Signal<usize>, page: usize, pages: usize) -> Element {
    if pages <= 1 {
        return rsx! {};
    }
    rsx! {
        div { class: "sr-pagination",
            button {
                class: "sr-page-btn",
                disabled: page <= 1,
                onclick: move |_| current_page.set(page.saturating_sub(1).max(1)),
                "\u{25C0}"
            }
            for p in pagination_range(page, pages) {
                if p == 0 {
                    span { class: "sr-page-info", "\u{2026}" }
                } else {
                    button {
                        class: if p == page { "sr-page-btn active" } else { "sr-page-btn" },
                        onclick: move |_| current_page.set(p),
                        "{p}"
                    }
                }
            }
            button {
                class: "sr-page-btn",
                disabled: page >= pages,
                onclick: move |_| current_page.set((page + 1).min(pages)),
                "\u{25B6}"
            }
        }
    }
}

fn render_clear_filters(
    i18n: I18n,
    mut quick_filter: Signal<String>,
    mut letter_filter: Signal<Option<char>>,
    mut current_page: Signal<usize>,
) -> Element {
    rsx! {
        div { class: "sr-empty",
            p { {i18n.t("dictionary.no_matches")} }
            button {
                class: "sr-clear-filters",
                onclick: move |_| {
                    quick_filter.set(String::new());
                    letter_filter.set(None);
                    current_page.set(1);
                },
                {i18n.t("dictionary.clear_filter")}
            }
        }
    }
}

fn render_usage_accordion(
    i18n: I18n,
    tree_id: &str,
    people: Resource<Vec<(Uuid, Option<String>)>>,
) -> Element {
    match &*people.read() {
        Some(list) if !list.is_empty() => rsx! {
            div { class: "dict-accordion",
                for (pid , name) in list.iter() {
                    Link {
                        key: "{pid}",
                        to: Route::PersonDetail { tree_id: tree_id.to_string(), person_id: pid.to_string() },
                        class: "dict-accordion-item",
                        {name.clone().unwrap_or_else(|| i18n.t("common.unnamed"))}
                    }
                }
            }
        },
        Some(_) => rsx! {
            div { class: "dict-accordion",
                div { class: "dict-accordion-empty", {i18n.t("dictionary.usage_empty")} }
            }
        },
        None => rsx! {
            div { class: "dict-accordion",
                div { class: "dict-accordion-empty", {i18n.t("common.loading")} }
            }
        },
    }
}

// ── Family Names / Occupations tab (plain value + count) ────────────────

#[allow(clippy::too_many_arguments)]
fn render_value_tab(
    i18n: I18n,
    tree_id: &str,
    resource: Resource<Result<Vec<DictionaryEntry>, ApiError>>,
    quick_filter: Signal<String>,
    letter_filter: Signal<Option<char>>,
    page_size: Signal<PageSize>,
    current_page: Signal<usize>,
    empty_key: &'static str,
    // Family names link out to search; occupations expand an inline usage list.
    navigable: bool,
    mut expanded: Signal<Option<UsageKey>>,
    usage_people: Resource<Vec<(Uuid, Option<String>)>>,
) -> Element {
    let all_entries: Vec<DictionaryEntry> = match &*resource.read() {
        Some(Ok(entries)) => entries.clone(),
        _ => Vec::new(),
    };
    let is_loading = resource.read().is_none();
    let is_error = matches!(&*resource.read(), Some(Err(_)));

    let letters = available_letters(all_entries.iter().map(|e| e.value.as_str()));
    let quick = quick_filter();
    let letter = letter_filter();
    let filtered: Vec<&DictionaryEntry> = all_entries
        .iter()
        .filter(|e| matches_filters(&e.value, &quick, letter))
        .collect();
    let total_filtered = filtered.len();
    let per_page = page_size().as_option();
    let page = current_page();
    let pages = total_pages(total_filtered, per_page);
    let page_items = paginate(filtered, page, per_page);
    let rows = with_headers(&page_items, |e| e.value.as_str());

    rsx! {
        {render_toolbar(i18n, &letters, letter_filter, current_page, quick_filter, page_size, total_filtered)}

        if is_loading {
            div { class: "sr-empty", {i18n.t("dictionary.loading")} }
        } else if is_error {
            div { class: "sr-empty", {i18n.t("dictionary.error")} }
        } else if all_entries.is_empty() {
            div { class: "sr-empty", {i18n.t(empty_key)} }
        } else if rows.is_empty() {
            {render_clear_filters(i18n, quick_filter, letter_filter, current_page)}
        } else {
            div { class: "dict-list",
                for (header , entry) in rows.iter() {
                    if let Some(c) = header {
                        div { key: "hdr-{c}", class: "dict-group-header", "{c}" }
                    }
                    if navigable {
                        Link {
                            key: "{entry.value}",
                            to: Route::SearchResults {
                                tree_id: tree_id.to_string(),
                                last: entry.value.clone(),
                                first: String::new(),
                                origin: String::new(),
                            },
                            class: "dict-row",
                            title: "{i18n.t(\"dictionary.view_in_search\")}",
                            div { class: "dict-row-main",
                                span { class: "dict-row-value", "{entry.value}" }
                            }
                            span { class: "dict-row-count", {i18n.t_plural("dictionary.person_count", entry.count as usize)} }
                        }
                    } else {
                        {
                            let key = UsageKey::Occupation(entry.value.clone());
                            let is_open = expanded() == Some(key.clone());
                            rsx! {
                                div { key: "{entry.value}",
                                    div {
                                        class: "dict-row",
                                        onclick: {
                                            let key = key.clone();
                                            move |_| {
                                                if expanded() == Some(key.clone()) {
                                                    expanded.set(None);
                                                } else {
                                                    expanded.set(Some(key.clone()));
                                                }
                                            }
                                        },
                                        div { class: "dict-row-main",
                                            span { class: "dict-row-value", "{entry.value}" }
                                        }
                                        span { class: "dict-row-count", {i18n.t_plural("dictionary.person_count", entry.count as usize)} }
                                        button {
                                            class: "dict-row-action",
                                            title: "{i18n.t(\"dictionary.view_usage\")}",
                                            if is_open { "\u{25B2}" } else { "\u{25BC}" }
                                        }
                                    }
                                    if is_open {
                                        {render_usage_accordion(i18n, tree_id, usage_people)}
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        {render_pagination(current_page, page, pages)}
    }
}

// ── Sources tab ───────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn render_sources_tab(
    i18n: I18n,
    tree_id: &str,
    resource: Resource<Result<Vec<SourceDictionaryEntry>, ApiError>>,
    quick_filter: Signal<String>,
    letter_filter: Signal<Option<char>>,
    page_size: Signal<PageSize>,
    current_page: Signal<usize>,
    mut expanded: Signal<Option<UsageKey>>,
    usage_people: Resource<Vec<(Uuid, Option<String>)>>,
) -> Element {
    let all_entries: Vec<SourceDictionaryEntry> = match &*resource.read() {
        Some(Ok(entries)) => entries.clone(),
        _ => Vec::new(),
    };
    let is_loading = resource.read().is_none();
    let is_error = matches!(&*resource.read(), Some(Err(_)));

    let letters = available_letters(all_entries.iter().map(|e| e.source.title.as_str()));
    let quick = quick_filter();
    let letter = letter_filter();
    let filtered: Vec<&SourceDictionaryEntry> = all_entries
        .iter()
        .filter(|e| matches_filters(&e.source.title, &quick, letter))
        .collect();
    let total_filtered = filtered.len();
    let per_page = page_size().as_option();
    let page = current_page();
    let pages = total_pages(total_filtered, per_page);
    let page_items = paginate(filtered, page, per_page);
    let rows = with_headers(&page_items, |e| e.source.title.as_str());

    rsx! {
        {render_toolbar(i18n, &letters, letter_filter, current_page, quick_filter, page_size, total_filtered)}

        if is_loading {
            div { class: "sr-empty", {i18n.t("dictionary.loading")} }
        } else if is_error {
            div { class: "sr-empty", {i18n.t("dictionary.error")} }
        } else if all_entries.is_empty() {
            div { class: "sr-empty", {i18n.t("dictionary.no_entries_sources")} }
        } else if rows.is_empty() {
            {render_clear_filters(i18n, quick_filter, letter_filter, current_page)}
        } else {
            div { class: "dict-list",
                for (header , entry) in rows.iter() {
                    if let Some(c) = header {
                        div { key: "hdr-{c}", class: "dict-group-header", "{c}" }
                    }
                    {
                        let key = UsageKey::Source(entry.source.id);
                        let is_open = expanded() == Some(key.clone());
                        let meta = [entry.source.author.clone(), entry.source.repository_name.clone()]
                            .into_iter()
                            .flatten()
                            .collect::<Vec<_>>()
                            .join(" \u{00B7} ");
                        rsx! {
                            div { key: "{entry.source.id}",
                                div {
                                    class: "dict-row",
                                    onclick: {
                                        let key = key.clone();
                                        move |_| {
                                            if expanded() == Some(key.clone()) {
                                                expanded.set(None);
                                            } else {
                                                expanded.set(Some(key.clone()));
                                            }
                                        }
                                    },
                                    div { class: "dict-row-main",
                                        span { class: "dict-row-value", "{entry.source.title}" }
                                        if !meta.is_empty() {
                                            span { class: "dict-row-meta", "{meta}" }
                                        }
                                    }
                                    span { class: "dict-row-count", {i18n.t_plural("dictionary.citation_count", entry.count as usize)} }
                                    button {
                                        class: "dict-row-action",
                                        title: "{i18n.t(\"dictionary.view_usage\")}",
                                        if is_open { "\u{25B2}" } else { "\u{25BC}" }
                                    }
                                }
                                if is_open {
                                    {render_usage_accordion(i18n, tree_id, usage_people)}
                                }
                            }
                        }
                    }
                }
            }
        }

        {render_pagination(current_page, page, pages)}
    }
}

// ── Places tab ────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn render_places_tab(
    i18n: I18n,
    tree_id: &str,
    resource: Resource<Result<Vec<PlaceDictionaryEntry>, ApiError>>,
    quick_filter: Signal<String>,
    letter_filter: Signal<Option<char>>,
    page_size: Signal<PageSize>,
    current_page: Signal<usize>,
    mut expanded: Signal<Option<UsageKey>>,
    usage_people: Resource<Vec<(Uuid, Option<String>)>>,
) -> Element {
    let all_entries: Vec<PlaceDictionaryEntry> = match &*resource.read() {
        Some(Ok(entries)) => entries.clone(),
        _ => Vec::new(),
    };
    let is_loading = resource.read().is_none();
    let is_error = matches!(&*resource.read(), Some(Err(_)));

    let letters = available_letters(all_entries.iter().map(|e| e.place.name.as_str()));
    let quick = quick_filter();
    let letter = letter_filter();
    let filtered: Vec<&PlaceDictionaryEntry> = all_entries
        .iter()
        .filter(|e| matches_filters(&e.place.name, &quick, letter))
        .collect();
    let total_filtered = filtered.len();
    let per_page = page_size().as_option();
    let page = current_page();
    let pages = total_pages(total_filtered, per_page);
    let page_items = paginate(filtered, page, per_page);
    let rows = with_headers(&page_items, |e| e.place.name.as_str());

    rsx! {
        {render_toolbar(i18n, &letters, letter_filter, current_page, quick_filter, page_size, total_filtered)}

        if is_loading {
            div { class: "sr-empty", {i18n.t("dictionary.loading")} }
        } else if is_error {
            div { class: "sr-empty", {i18n.t("dictionary.error")} }
        } else if all_entries.is_empty() {
            div { class: "sr-empty", {i18n.t("dictionary.no_entries_places")} }
        } else if rows.is_empty() {
            {render_clear_filters(i18n, quick_filter, letter_filter, current_page)}
        } else {
            div { class: "dict-list",
                for (header , entry) in rows.iter() {
                    if let Some(c) = header {
                        div { key: "hdr-{c}", class: "dict-group-header", "{c}" }
                    }
                    {
                        let key = UsageKey::Place(entry.place.id);
                        let is_open = expanded() == Some(key.clone());
                        let has_coords = entry.place.latitude.is_some() && entry.place.longitude.is_some();
                        rsx! {
                            div { key: "{entry.place.id}",
                                div {
                                    class: "dict-row",
                                    onclick: {
                                        let key = key.clone();
                                        move |_| {
                                            if expanded() == Some(key.clone()) {
                                                expanded.set(None);
                                            } else {
                                                expanded.set(Some(key.clone()));
                                            }
                                        }
                                    },
                                    div { class: "dict-row-main",
                                        span { class: if has_coords { "dict-row-value dict-pin" } else { "dict-row-value" },
                                            if has_coords { "\u{1F4CD} " } else { "" }
                                            "{entry.place.name}"
                                        }
                                    }
                                    span { class: "dict-row-count", {i18n.t_plural("dictionary.reference_count", entry.count as usize)} }
                                    button {
                                        class: "dict-row-action",
                                        title: "{i18n.t(\"dictionary.view_usage\")}",
                                        if is_open { "\u{25B2}" } else { "\u{25BC}" }
                                    }
                                }
                                if is_open {
                                    {render_usage_accordion(i18n, tree_id, usage_people)}
                                }
                            }
                        }
                    }
                }
            }
        }

        {render_pagination(current_page, page, pages)}
    }
}
