//! Dictionary page: read-only index of family names, sources, places, and
//! occupations across a tree, each paired with a usage count. See
//! `docs/specifications/ui-dictionary.md`.

use std::collections::HashSet;

use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::{
    ApiClient, ApiError, DictionaryEntry, PersonUsageEntry, PlaceDictionaryEntry,
    SourceDictionaryEntry, SourceGroupEntry,
};
use crate::components::pedigree_chart::format_lifespan;
use crate::components::tree_cache::{fetch_tree_cached, use_tree_cache};
use crate::components::tree_icon_sidebar::{TreeIconSidebar, TreeSidebarView};
use crate::i18n::{I18n, Language, use_i18n};
use crate::router::Route;

/// Above this many filtered entries, selecting the "All" page size shows a
/// perf warning instead of silently rendering everything.
const LARGE_LIST_THRESHOLD: usize = 500;

/// The Sources tab's smart drill-down is either showing the next level of
/// prefix groups, or — once a prefix's count is small enough — the final
/// flat list of matching sources. Both variants carry the *resolved*
/// prefix (the backend auto-skips forced single-choice levels, so this may
/// be longer than the last prefix the user actually clicked — see
/// ui-dictionary.md §8.10). Whichever mode is active is decided entirely by
/// the backend (`groups` empty means "fetch the list").
#[derive(Debug, Clone)]
enum SourcesView {
    Groups {
        prefix: String,
        total: i64,
        groups: Vec<SourceGroupEntry>,
    },
    List {
        prefix: String,
        sources: Vec<SourceDictionaryEntry>,
    },
}

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
    FamilyName(String),
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
    // Sources tab drill-down history: each entry is a branch label the user
    // clicked (see ui-dictionary.md §8.10). Empty = "All sources" root.
    let mut source_history = use_signal(Vec::<String>::new);

    // Reset filters/pagination/expansion when switching tabs.
    let mut prev_tab = use_signal(|| DictTab::FamilyNames);
    if prev_tab() != active_tab() {
        prev_tab.set(active_tab());
        quick_filter.clone().set(String::new());
        letter_filter.clone().set(None);
        current_page.set(1);
        expanded.set(None);
        source_history.set(Vec::new());
    }

    // Scroll back to the top of the scrollable content area whenever the
    // page changes (pagination, or a filter/tab switch resetting to page
    // 1). Without this, a scroll position picked up while browsing a long
    // list (e.g. to reach the pagination controls at the bottom) persists
    // onto the next, possibly much shorter, result set — leaving the tabs
    // and filter toolbar scrolled out of view above the visible area.
    use_effect(move || {
        current_page();
        document::eval(
            "document.querySelector('.sub-page-content')?.scrollTo({ top: 0, behavior: 'instant' });",
        );
    });

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
    let mut sources_view_resource = use_resource(move || {
        let api = api_src.clone();
        let tid = tree_id_parsed();
        let query_prefix = source_history().last().cloned().unwrap_or_default();
        async move {
            let Some(tid) = tid else {
                return Err(ApiError::Api {
                    status: 400,
                    body: "Invalid tree ID".to_string(),
                });
            };
            // The backend resolves the drill-down itself, auto-skipping
            // any forced single-choice levels — `resolved.prefix` may be
            // longer than `query_prefix`. See ui-dictionary.md §8.10.
            let resolved = api.dictionary_source_groups(tid, &query_prefix).await?;
            if resolved.groups.is_empty() {
                let sources = api.dictionary_sources(tid, &resolved.prefix).await?;
                Ok(SourcesView::List {
                    prefix: resolved.prefix,
                    sources,
                })
            } else {
                Ok(SourcesView::Groups {
                    prefix: resolved.prefix,
                    total: resolved.total,
                    groups: resolved.groups,
                })
            }
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
        source_history.set(Vec::new());
        sources_view_resource.restart();
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
            match &key {
                UsageKey::FamilyName(value) => api.dictionary_family_name_usage(tid, value).await,
                UsageKey::Source(id) => api.dictionary_source_usage(tid, *id).await,
                UsageKey::Place(id) => api.dictionary_place_usage(tid, *id).await,
                UsageKey::Occupation(value) => api.dictionary_occupation_usage(tid, value).await,
            }
            .unwrap_or_default()
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
                        source_history,
                        sources_view_resource,
                        quick_filter,
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
            div { class: "dict-letter-strip",
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
            span { class: "sr-count dict-total-count", {i18n.t_plural("dictionary.count", total_filtered)} }
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
            div { class: "dict-page-size",
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

/// Renders a usage-list entry as "SURNAME Given" — surname uppercased and
/// first, matching genealogical convention — falling back to a localized
/// placeholder when both name parts are missing.
fn surname_first(entry: &PersonUsageEntry, i18n: &I18n) -> String {
    let surname = entry.surname.as_deref().map(str::to_uppercase);
    let given = entry.given_names.as_deref();
    match (surname, given) {
        (Some(s), Some(g)) => format!("{s} {g}"),
        (Some(s), None) => s,
        (None, Some(g)) => g.to_string(),
        (None, None) => i18n.t("common.unnamed"),
    }
}

fn render_usage_accordion(
    i18n: I18n,
    tree_id: &str,
    people: Resource<Vec<PersonUsageEntry>>,
) -> Element {
    match &*people.read() {
        Some(list) if !list.is_empty() => rsx! {
            div { class: "dict-accordion",
                for entry in list.iter() {
                    Link {
                        key: "{entry.person_id}",
                        to: Route::TreeDetail { tree_id: tree_id.to_string(), person: Some(entry.person_id.to_string()) },
                        class: "dict-accordion-item",
                        span { class: "dict-accordion-name", {surname_first(entry, &i18n)} }
                        {
                            let lifespan = format_lifespan(entry.birth_year, entry.death_year);
                            if lifespan.is_empty() {
                                rsx! {}
                            } else {
                                rsx! { span { class: "dict-accordion-dates", "{lifespan}" } }
                            }
                        }
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
    // Family names and occupations expand an inline usage list.
    navigable: bool,
    mut expanded: Signal<Option<UsageKey>>,
    usage_people: Resource<Vec<PersonUsageEntry>>,
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
                    {
                        let key = if navigable {
                            UsageKey::FamilyName(entry.value.clone())
                        } else {
                            UsageKey::Occupation(entry.value.clone())
                        };
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

        {render_pagination(current_page, page, pages)}
    }
}

// ── Sources tab — intelligent letter/prefix drill-down ──────────────────
//
// Unlike Family Names / Places / Occupations, most sources in a genealogy
// tree share long common prefixes (e.g. French "AD44 - ..." for Archives
// Départementales), making a flat A-Z index nearly useless. Instead, the
// Sources tab drills down one character at a time — only letters/prefixes
// that actually occur are ever shown — until the current prefix matches
// <= `SOURCES_DRILL_THRESHOLD` sources, at which point the full matching
// list is displayed at once (no pagination). See ui-dictionary.md §8.

/// Selects the French/English plural suffix for a count, matching the rule
/// `I18n::t_plural` uses internally — duplicated locally because the
/// Sources tab needs to combine pluralisation with a second `{prefix}`
/// placeholder, which `t_plural` (count-only) doesn't support.
fn plural_suffix(i18n: &I18n, count: usize) -> &'static str {
    match i18n.0 {
        Language::Fr => {
            if count <= 1 {
                "_one"
            } else {
                "_other"
            }
        }
        Language::En => {
            if count == 1 {
                "_one"
            } else {
                "_other"
            }
        }
    }
}

fn sources_total_label(i18n: &I18n, count: usize, prefix: &str) -> String {
    let suffix = plural_suffix(i18n, count);
    if prefix.is_empty() {
        i18n.t_args(
            &format!("dictionary.sources_total{suffix}"),
            &[("count", &count.to_string())],
        )
    } else {
        i18n.t_args(
            &format!("dictionary.sources_total_prefix{suffix}"),
            &[("count", &count.to_string()), ("prefix", prefix)],
        )
    }
}

/// Breadcrumb above the Sources tab content: "All sources > AD44 > AD44 -
/// HOTEL - (". Each history entry is a branch the user actually chose
/// (real, multi-way choices only — see `Dictionary`'s `source_history`);
/// clicking one truncates history back to that point. If the backend
/// auto-skipped ahead of the last click (forced single-choice levels — see
/// ui-dictionary.md §8.10), the resolved `active_prefix` is appended as one
/// extra, non-clickable crumb representing where that skip landed.
fn render_sources_breadcrumb(
    i18n: I18n,
    mut history: Signal<Vec<String>>,
    mut quick_filter: Signal<String>,
    active_prefix: &str,
) -> Element {
    let hist = history();
    let last_is_active = hist.last().map(String::as_str) == Some(active_prefix);
    rsx! {
        div { class: "dict-src-breadcrumb",
            button {
                class: if hist.is_empty() && active_prefix.is_empty() { "dict-src-crumb active" } else { "dict-src-crumb" },
                onclick: move |_| {
                    history.set(Vec::new());
                    quick_filter.set(String::new());
                },
                {i18n.t("dictionary.sources_breadcrumb_root")}
            }
            for (i , label) in hist.iter().enumerate() {
                {
                    let idx = i;
                    let is_last_history_entry = i == hist.len() - 1;
                    let is_active = is_last_history_entry && last_is_active;
                    rsx! {
                        span { key: "sep-{i}", class: "dict-src-crumb-sep", "\u{203A}" }
                        button {
                            class: if is_active { "dict-src-crumb active" } else { "dict-src-crumb" },
                            onclick: move |_| {
                                let mut h = history();
                                h.truncate(idx + 1);
                                history.set(h);
                                quick_filter.set(String::new());
                            },
                            "{label}"
                        }
                    }
                }
            }
            if !active_prefix.is_empty() && !last_is_active {
                span { class: "dict-src-crumb-sep", "\u{203A}" }
                span { class: "dict-src-crumb active", "{active_prefix}" }
            }
        }
    }
}

/// Renders the prefix-group buttons for the current drill-down level: only
/// groups that actually occur in this tree are ever passed in, so every
/// button is clickable (contrast with the disabled-letter A-Z index used by
/// the other tabs). Clicking a group pushes it onto `history` — the next
/// resolve request may auto-skip further forced single-choice levels
/// beyond it (see ui-dictionary.md §8.10).
fn render_sources_groups(
    i18n: I18n,
    mut history: Signal<Vec<String>>,
    prefix: &str,
    total: i64,
    groups: &[SourceGroupEntry],
    mut quick_filter: Signal<String>,
) -> Element {
    let quick = quick_filter();
    let filtered: Vec<&SourceGroupEntry> = groups
        .iter()
        .filter(|g| quick.is_empty() || g.label.to_lowercase().contains(&quick.to_lowercase()))
        .collect();

    rsx! {
        div { class: "dict-src-summary", {sources_total_label(&i18n, total.max(0) as usize, prefix)} }
        div { class: "dict-filter-row",
            input {
                r#type: "text",
                class: "dict-filter-input",
                placeholder: "{i18n.t(\"dictionary.filter_placeholder\")}",
                value: "{quick_filter}",
                oninput: move |e: Event<FormData>| quick_filter.set(e.value()),
            }
        }
        div { class: "dict-src-groups-label", {i18n.t("dictionary.sources_choose_letter")} }
        if filtered.is_empty() {
            div { class: "sr-empty",
                p { {i18n.t("dictionary.no_matches")} }
                button {
                    class: "sr-clear-filters",
                    onclick: move |_| quick_filter.set(String::new()),
                    {i18n.t("dictionary.clear_filter")}
                }
            }
        } else {
            div { class: "dict-letter-strip",
                for g in filtered.iter() {
                    button {
                        key: "{g.label}",
                        class: "dict-letter-btn",
                        onclick: {
                            let label = g.label.clone();
                            move |_| {
                                let mut h = history();
                                h.push(label.clone());
                                history.set(h);
                                quick_filter.set(String::new());
                            }
                        },
                        "{g.label}"
                    }
                }
            }
        }
    }
}

/// Renders the final flat list once the current prefix matches
/// <= `SOURCES_DRILL_THRESHOLD` sources — no pagination, everything shown.
fn render_sources_list(
    i18n: I18n,
    tree_id: &str,
    sources: &[SourceDictionaryEntry],
    mut quick_filter: Signal<String>,
    mut expanded: Signal<Option<UsageKey>>,
    usage_people: Resource<Vec<PersonUsageEntry>>,
) -> Element {
    let quick = quick_filter();
    let filtered: Vec<&SourceDictionaryEntry> = sources
        .iter()
        .filter(|e| {
            quick.is_empty()
                || e.source
                    .title
                    .to_lowercase()
                    .contains(&quick.to_lowercase())
        })
        .collect();

    rsx! {
        div { class: "dict-src-summary", {i18n.t_plural("dictionary.count", filtered.len())} }
        div { class: "dict-filter-row",
            input {
                r#type: "text",
                class: "dict-filter-input",
                placeholder: "{i18n.t(\"dictionary.filter_placeholder\")}",
                value: "{quick_filter}",
                oninput: move |e: Event<FormData>| quick_filter.set(e.value()),
            }
        }

        if filtered.is_empty() {
            div { class: "sr-empty",
                p { {i18n.t("dictionary.no_matches")} }
                button {
                    class: "sr-clear-filters",
                    onclick: move |_| quick_filter.set(String::new()),
                    {i18n.t("dictionary.clear_filter")}
                }
            }
        } else {
            div { class: "dict-list",
                for entry in filtered.iter() {
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
    }
}

#[allow(clippy::too_many_arguments)]
fn render_sources_tab(
    i18n: I18n,
    tree_id: &str,
    history: Signal<Vec<String>>,
    resource: Resource<Result<SourcesView, ApiError>>,
    quick_filter: Signal<String>,
    expanded: Signal<Option<UsageKey>>,
    usage_people: Resource<Vec<PersonUsageEntry>>,
) -> Element {
    let is_loading = resource.read().is_none();
    let is_error = matches!(&*resource.read(), Some(Err(_)));
    let view: Option<SourcesView> = match &*resource.read() {
        Some(Ok(v)) => Some(v.clone()),
        _ => None,
    };
    // Falls back to the last clicked branch while the resolve request for
    // it is still in flight, so the breadcrumb doesn't flash back to root.
    let active_prefix = match &view {
        Some(SourcesView::Groups { prefix, .. }) | Some(SourcesView::List { prefix, .. }) => {
            prefix.clone()
        }
        None => history().last().cloned().unwrap_or_default(),
    };

    rsx! {
        {render_sources_breadcrumb(i18n, history, quick_filter, &active_prefix)}

        if is_loading {
            div { class: "sr-empty", {i18n.t("dictionary.loading")} }
        } else if is_error {
            div { class: "sr-empty", {i18n.t("dictionary.error")} }
        } else {
            match view {
                Some(SourcesView::Groups { prefix, total, groups }) if !groups.is_empty() => {
                    render_sources_groups(i18n, history, &prefix, total, &groups, quick_filter)
                }
                Some(SourcesView::List { sources, .. }) => {
                    render_sources_list(i18n, tree_id, &sources, quick_filter, expanded, usage_people)
                }
                _ => rsx! {
                    div { class: "sr-empty", {i18n.t("dictionary.no_entries_sources")} }
                },
            }
        }
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
    usage_people: Resource<Vec<PersonUsageEntry>>,
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
