//! Search results page — filterable list of persons matching a search query.
//!
//! Reached by pressing **Enter** in the tree topbar search fields.
//! Route: `/trees/:tree_id/search?last=...&first=...`
//!
//! Performance: uses the single `/snapshot` endpoint instead of N+1 per-person
//! requests, and applies client-side filtering with debounced inputs.

use std::collections::HashMap;

use dioxus::prelude::*;
use oxidgene_core::types::{Event as DomainEvent, PersonName};
use oxidgene_core::{EventType, Sex};
use uuid::Uuid;

use crate::api::ApiClient;
use crate::components::tree_cache::{fetch_snapshot_cached, fetch_tree_cached, use_tree_cache};
use crate::i18n::use_i18n;
use crate::router::Route;
use crate::utils::resolve_name;

/// Sort options for search results.
#[derive(Debug, Clone, Copy, PartialEq)]
enum SortOrder {
    Relevance,
    NameAZ,
    NameZA,
    BirthAsc,
    BirthDesc,
}

/// Gender filter.
#[derive(Debug, Clone, Copy, PartialEq)]
enum GenderFilter {
    All,
    Male,
    Female,
    Unknown,
}

/// View mode for results.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ViewMode {
    List,
    Card,
}

/// A single search result with pre-resolved display data.
#[derive(Clone, Debug)]
struct SearchResult {
    person_id: Uuid,
    surname: String,
    given_names: String,
    /// Pre-computed lowercase surname for sort/filter (avoids repeated allocations).
    surname_lower: String,
    /// Pre-computed lowercase given names for sort/filter.
    given_lower: String,
    #[allow(dead_code)]
    display_name: String,
    sex: Sex,
    birth_date: Option<String>,
    death_date: Option<String>,
    birth_place: Option<String>,
    spouse_summary: Option<String>,
    child_count: usize,
    /// Fuzzy relevance score (higher = better match).
    relevance: u32,
}

const RESULTS_PER_PAGE: usize = 25;

/// Page rendered at `/trees/:tree_id/search`.
#[component]
pub fn SearchResults(tree_id: String, last: Option<String>, first: Option<String>) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let nav = use_navigator();
    let tree_cache = use_tree_cache();

    let tree_id_parsed = tree_id.parse::<Uuid>().ok();

    // Search query state.
    // `committed_last`/`committed_first` track the last submitted query (Enter / button).
    // Filtering reads only from committed signals. The live input signals live
    // inside the isolated `SearchBar` component so keystrokes never re-render this
    // heavy component.
    let committed_last = use_signal(|| last.clone().unwrap_or_default());
    let committed_first = use_signal(|| first.clone().unwrap_or_default());

    // Filters.
    let mut gender_filter = use_signal(|| GenderFilter::All);
    let mut born_from = use_signal(String::new);
    let mut born_to = use_signal(String::new);
    let mut died_from = use_signal(String::new);
    let mut died_to = use_signal(String::new);

    // Sort and view.
    let mut sort_order = use_signal(|| SortOrder::Relevance);
    let mut view_mode = use_signal(|| ViewMode::List);
    let mut current_page = use_signal(|| 0usize);

    // Filters visibility toggle.
    let mut filters_visible = use_signal(|| false);

    // ── Fetch tree details (cache-backed) ──
    let api_tree = api.clone();
    let tree_resource = use_resource(move || {
        let api = api_tree.clone();
        let _gen = tree_cache.generation();
        let tid = tree_id_parsed;
        async move {
            let Some(tid) = tid else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: "Invalid tree ID".to_string(),
                });
            };
            fetch_tree_cached(&api, &tree_cache, tid).await
        }
    });

    // ── Fetch all data via single snapshot request (cache-backed) ──
    let api_snap = api.clone();
    let snapshot_resource = use_resource(move || {
        let api = api_snap.clone();
        let _gen = tree_cache.generation();
        let tid = tree_id_parsed;
        async move {
            let Some(tid) = tid else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: "Invalid tree ID".to_string(),
                });
            };
            fetch_snapshot_cached(&api, &tree_cache, tid).await
        }
    });

    // ── Build search results ──
    let is_loading = snapshot_resource.read().is_none();

    let last_q = committed_last().trim().to_lowercase();
    let first_q = committed_first().trim().to_lowercase();
    let has_query = !last_q.is_empty() || !first_q.is_empty();

    let mut results: Vec<SearchResult> = {
        let snap_data = snapshot_resource.read();

        match &*snap_data {
            Some(Ok(snapshot)) => {
                // Build name map: person_id -> Vec<PersonName>
                let mut name_map: HashMap<Uuid, Vec<PersonName>> = HashMap::new();
                for pn in snapshot.names.iter() {
                    name_map.entry(pn.person_id).or_default().push(pn.clone());
                }

                // Pre-build event indexes.
                let mut events_by_person: HashMap<Uuid, Vec<&DomainEvent>> = HashMap::new();
                for e in snapshot.events.iter() {
                    if let Some(pid) = e.person_id {
                        events_by_person.entry(pid).or_default().push(e);
                    }
                }

                // Pre-build family indexes.
                let mut spouses_by_family: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
                for s in snapshot.spouses.iter() {
                    spouses_by_family
                        .entry(s.family_id)
                        .or_default()
                        .push(s.person_id);
                }
                let mut children_by_family: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
                for c in snapshot.children.iter() {
                    children_by_family
                        .entry(c.family_id)
                        .or_default()
                        .push(c.person_id);
                }
                let mut families_as_spouse: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
                for s in snapshot.spouses.iter() {
                    families_as_spouse
                        .entry(s.person_id)
                        .or_default()
                        .push(s.family_id);
                }

                // Places map.
                let api_places = api.clone();
                let _ = api_places; // places loaded in events already

                snapshot
                    .persons
                    .iter()
                    .filter_map(|person| {
                        let (given, surname, _nick) = name_parts(person.id, &name_map);
                        let given_str = given.unwrap_or_default();
                        let surname_str = surname.unwrap_or_default();
                        let display = resolve_name(person.id, &name_map);

                        // Pre-compute lowercase once for this person.
                        let given_lower = given_str.to_lowercase();
                        let surname_lower = surname_str.to_lowercase();

                        // Name matching.
                        if has_query {
                            let display_lower = display.to_lowercase();

                            // Check if it's a SOSA number search.
                            if let Ok(_sosa) = last_q.parse::<u64>() {
                                // TODO: SOSA number search (requires ancestry data)
                                // For now, fall through to name search.
                            }

                            let last_ok = last_q.is_empty()
                                || surname_lower.contains(&last_q)
                                || display_lower.contains(&last_q);
                            let first_ok = first_q.is_empty()
                                || given_lower.contains(&first_q)
                                || display_lower.contains(&first_q);

                            if !last_ok || !first_ok {
                                return None;
                            }
                        }

                        // Gender filter.
                        match gender_filter() {
                            GenderFilter::All => {}
                            GenderFilter::Male => {
                                if person.sex != Sex::Male {
                                    return None;
                                }
                            }
                            GenderFilter::Female => {
                                if person.sex != Sex::Female {
                                    return None;
                                }
                            }
                            GenderFilter::Unknown => {
                                if person.sex != Sex::Unknown {
                                    return None;
                                }
                            }
                        }

                        // Extract dates.
                        let person_events = events_by_person.get(&person.id);
                        let birth_date = person_events.and_then(|evts| {
                            evts.iter()
                                .find(|e| e.event_type == EventType::Birth)
                                .or_else(|| {
                                    evts.iter().find(|e| e.event_type == EventType::Baptism)
                                })
                                .and_then(|e| e.date_value.clone())
                        });
                        let death_date = person_events.and_then(|evts| {
                            evts.iter()
                                .find(|e| e.event_type == EventType::Death)
                                .or_else(|| evts.iter().find(|e| e.event_type == EventType::Burial))
                                .and_then(|e| e.date_value.clone())
                        });

                        // Date range filter.
                        let born_from_val = born_from();
                        let born_to_val = born_to();
                        let died_from_val = died_from();
                        let died_to_val = died_to();

                        if !born_from_val.is_empty() || !born_to_val.is_empty() {
                            if let Some(ref bd) = birth_date {
                                let year = extract_year(bd);
                                if let Some(y) = year {
                                    if let Ok(from) = born_from_val.parse::<i32>()
                                        && y < from
                                    {
                                        return None;
                                    }
                                    if let Ok(to) = born_to_val.parse::<i32>()
                                        && y > to
                                    {
                                        return None;
                                    }
                                } else {
                                    return None;
                                }
                            } else {
                                return None;
                            }
                        }

                        if !died_from_val.is_empty() || !died_to_val.is_empty() {
                            if let Some(ref dd) = death_date {
                                let year = extract_year(dd);
                                if let Some(y) = year {
                                    if let Ok(from) = died_from_val.parse::<i32>()
                                        && y < from
                                    {
                                        return None;
                                    }
                                    if let Ok(to) = died_to_val.parse::<i32>()
                                        && y > to
                                    {
                                        return None;
                                    }
                                } else {
                                    return None;
                                }
                            } else {
                                return None;
                            }
                        }

                        // Birth place (from birth event).
                        let birth_place = person_events.and_then(|evts| {
                            evts.iter()
                                .find(|e| e.event_type == EventType::Birth)
                                .and_then(|e| e.description.clone())
                        });

                        // Spouse summary.
                        let person_families = families_as_spouse.get(&person.id);
                        let spouse_summary = person_families.and_then(|fids| {
                            fids.iter().find_map(|fid| {
                                spouses_by_family.get(fid).and_then(|spouses_in_fam| {
                                    spouses_in_fam
                                        .iter()
                                        .find(|&&sp| sp != person.id)
                                        .map(|&sp| resolve_name(sp, &name_map))
                                })
                            })
                        });

                        // Child count.
                        let child_count: usize = person_families
                            .map(|fids| {
                                fids.iter()
                                    .filter_map(|fid| children_by_family.get(fid))
                                    .map(|c| c.len())
                                    .sum()
                            })
                            .unwrap_or(0);

                        // Relevance score (use pre-computed lowercase).
                        let mut relevance: u32 = 0;
                        if has_query {
                            if !last_q.is_empty() && surname_lower == last_q {
                                relevance += 100;
                            } else if !last_q.is_empty() && surname_lower.starts_with(&last_q) {
                                relevance += 50;
                            } else if !last_q.is_empty() && surname_lower.contains(&last_q) {
                                relevance += 25;
                            }
                            if !first_q.is_empty() && given_lower == first_q {
                                relevance += 80;
                            } else if !first_q.is_empty() && given_lower.starts_with(&first_q) {
                                relevance += 40;
                            } else if !first_q.is_empty() && given_lower.contains(&first_q) {
                                relevance += 20;
                            }
                        }

                        Some(SearchResult {
                            person_id: person.id,
                            surname: surname_str,
                            given_names: given_str,
                            surname_lower,
                            given_lower,
                            display_name: display,
                            sex: person.sex,
                            birth_date,
                            death_date,
                            birth_place,
                            spouse_summary,
                            child_count,
                            relevance,
                        })
                    })
                    .collect()
            }
            _ => vec![],
        }
    };

    // Sort results (using pre-computed lowercase fields to avoid repeated allocations).
    match sort_order() {
        SortOrder::Relevance => results.sort_by(|a, b| b.relevance.cmp(&a.relevance)),
        SortOrder::NameAZ => results.sort_by(|a, b| {
            a.surname_lower
                .cmp(&b.surname_lower)
                .then(a.given_lower.cmp(&b.given_lower))
        }),
        SortOrder::NameZA => results.sort_by(|a, b| {
            b.surname_lower
                .cmp(&a.surname_lower)
                .then(b.given_lower.cmp(&a.given_lower))
        }),
        SortOrder::BirthAsc => results.sort_by(|a, b| {
            let ya = a.birth_date.as_deref().and_then(extract_year);
            let yb = b.birth_date.as_deref().and_then(extract_year);
            ya.cmp(&yb)
        }),
        SortOrder::BirthDesc => results.sort_by(|a, b| {
            let ya = a.birth_date.as_deref().and_then(extract_year);
            let yb = b.birth_date.as_deref().and_then(extract_year);
            yb.cmp(&ya)
        }),
    }

    let total_count = results.len();
    let total_pages = (total_count + RESULTS_PER_PAGE - 1) / RESULTS_PER_PAGE.max(1);
    let page = current_page().min(total_pages.saturating_sub(1));
    let paged_results: Vec<&SearchResult> = results
        .iter()
        .skip(page * RESULTS_PER_PAGE)
        .take(RESULTS_PER_PAGE)
        .collect();

    let tree_name = {
        let guard = tree_resource.read();
        match &*guard {
            Some(Ok(t)) => t.name.clone(),
            _ => "...".to_string(),
        }
    };

    let active_filters_count = {
        let mut c = 0u32;
        if gender_filter() != GenderFilter::All {
            c += 1;
        }
        if !born_from().is_empty() || !born_to().is_empty() {
            c += 1;
        }
        if !died_from().is_empty() || !died_to().is_empty() {
            c += 1;
        }
        c
    };

    let clear_filters = move |_| {
        gender_filter.set(GenderFilter::All);
        born_from.set(String::new());
        born_to.set(String::new());
        died_from.set(String::new());
        died_to.set(String::new());
        current_page.set(0);
    };

    rsx! {
        div { class: "search-results-page",

        // ── Topbar ──
        div { class: "td-topbar",
            nav { class: "td-bc",
                Link { to: Route::Home {}, class: "td-bc-logo",
                    img {
                        src: crate::components::layout::LOGO_PNG_B64,
                        alt: "OxidGene",
                        class: "td-bc-logo-img",
                    }
                }
                Link {
                    to: Route::TreeDetail { tree_id: tree_id.clone(), person: None },
                    "{tree_name}"
                }
                span { class: "td-bc-sep", "/" }
                span { class: "td-bc-current", {i18n.t("search_results.breadcrumb")} }
            }
            SearchBar {
                tree_id: tree_id.clone(),
                initial_last: last.clone().unwrap_or_default(),
                initial_first: first.clone().unwrap_or_default(),
                committed_last: committed_last,
                committed_first: committed_first,
                current_page: current_page,
            }
        }

        // ── Content area ──
        div { class: "sr-content",

        // ── Page header ──
        div { class: "sr-header",
            if has_query {
                h2 { class: "sr-title",
                    {i18n.t("search_results.title")}
                    if !last_q.is_empty() {
                        span { class: "sr-highlight", " \"{committed_last}\"" }
                    }
                    if !first_q.is_empty() {
                        span { class: "sr-highlight", " \"{committed_first}\"" }
                    }
                }
            } else {
                h2 { class: "sr-title", {i18n.t("search_results.title_all")} }
            }
            if !is_loading {
                p { class: "sr-count",
                    {i18n.t_args("search_results.count", &[("count", &total_count.to_string())])}
                }
            }
        }

        // ── Filters ──
        div { class: "sr-filters-toggle",
            button {
                class: "td-btn",
                onclick: move |_| filters_visible.set(!filters_visible()),
                {i18n.t("search_results.filters")}
                if active_filters_count > 0 {
                    span { class: "sr-filter-badge", "{active_filters_count}" }
                }
                span { class: if filters_visible() { "sr-chevron open" } else { "sr-chevron" },
                    "\u{25BC}"
                }
            }
        }
        if filters_visible() {
            div { class: "sr-filters",
                div { class: "sr-filter-row",
                    div { class: "sr-filter-group",
                        label { {i18n.t("search_results.filter_gender")} }
                        select {
                            value: match gender_filter() {
                                GenderFilter::All => "All",
                                GenderFilter::Male => "Male",
                                GenderFilter::Female => "Female",
                                GenderFilter::Unknown => "Unknown",
                            },
                            oninput: move |e: Event<FormData>| {
                                gender_filter.set(match e.value().as_str() {
                                    "Male" => GenderFilter::Male,
                                    "Female" => GenderFilter::Female,
                                    "Unknown" => GenderFilter::Unknown,
                                    _ => GenderFilter::All,
                                });
                                current_page.set(0);
                            },
                            option { value: "All", {i18n.t("search_results.all")} }
                            option { value: "Male", {i18n.t("sex.male")} }
                            option { value: "Female", {i18n.t("sex.female")} }
                            option { value: "Unknown", {i18n.t("sex.unknown")} }
                        }
                    }
                    div { class: "sr-filter-group",
                        label { {i18n.t("search_results.born_between")} }
                        div { class: "sr-date-range",
                            input {
                                r#type: "text",
                                placeholder: "yyyy",
                                value: "{born_from}",
                                oninput: move |e: Event<FormData>| {
                                    born_from.set(e.value());
                                    current_page.set(0);
                                },
                            }
                            span { "\u{2013}" }
                            input {
                                r#type: "text",
                                placeholder: "yyyy",
                                value: "{born_to}",
                                oninput: move |e: Event<FormData>| {
                                    born_to.set(e.value());
                                    current_page.set(0);
                                },
                            }
                        }
                    }
                    div { class: "sr-filter-group",
                        label { {i18n.t("search_results.died_between")} }
                        div { class: "sr-date-range",
                            input {
                                r#type: "text",
                                placeholder: "yyyy",
                                value: "{died_from}",
                                oninput: move |e: Event<FormData>| {
                                    died_from.set(e.value());
                                    current_page.set(0);
                                },
                            }
                            span { "\u{2013}" }
                            input {
                                r#type: "text",
                                placeholder: "yyyy",
                                value: "{died_to}",
                                oninput: move |e: Event<FormData>| {
                                    died_to.set(e.value());
                                    current_page.set(0);
                                },
                            }
                        }
                    }
                }
                if active_filters_count > 0 {
                    button {
                        class: "sr-clear-filters",
                        onclick: clear_filters,
                        {i18n.t("search_results.clear_filters")}
                    }
                }
            }
        }

        // ── Toolbar: sort + view mode ──
        div { class: "sr-toolbar",
            div { class: "sr-sort",
                label { {i18n.t("search_results.sort_by")} }
                select {
                    value: match sort_order() {
                        SortOrder::Relevance => "relevance",
                        SortOrder::NameAZ => "name_az",
                        SortOrder::NameZA => "name_za",
                        SortOrder::BirthAsc => "birth_asc",
                        SortOrder::BirthDesc => "birth_desc",
                    },
                    oninput: move |e: Event<FormData>| {
                        sort_order.set(match e.value().as_str() {
                            "name_az" => SortOrder::NameAZ,
                            "name_za" => SortOrder::NameZA,
                            "birth_asc" => SortOrder::BirthAsc,
                            "birth_desc" => SortOrder::BirthDesc,
                            _ => SortOrder::Relevance,
                        });
                    },
                    option { value: "relevance", {i18n.t("search_results.sort_relevance")} }
                    option { value: "name_az", {i18n.t("search_results.sort_name_az")} }
                    option { value: "name_za", {i18n.t("search_results.sort_name_za")} }
                    option { value: "birth_asc", {i18n.t("search_results.sort_birth_asc")} }
                    option { value: "birth_desc", {i18n.t("search_results.sort_birth_desc")} }
                }
            }
            div { class: "sr-view-modes",
                button {
                    class: if view_mode() == ViewMode::List { "sr-view-btn active" } else { "sr-view-btn" },
                    title: "List view",
                    onclick: move |_| view_mode.set(ViewMode::List),
                    "\u{2630}"
                }
                button {
                    class: if view_mode() == ViewMode::Card { "sr-view-btn active" } else { "sr-view-btn" },
                    title: "Card view",
                    onclick: move |_| view_mode.set(ViewMode::Card),
                    "\u{229E}"
                }
            }
        }

        // ── Results ──
        if is_loading {
            div { class: "loading", {i18n.t("common.loading")} }
        } else if !has_query && total_count == 0 {
            div { class: "sr-empty",
                p { {i18n.t("search_results.enter_query")} }
            }
        } else if total_count == 0 {
            div { class: "sr-empty",
                p { class: "sr-empty-icon", "\u{1F50D}" }
                p { {i18n.t("search_results.no_results")} }
                p { class: "sr-empty-hint", {i18n.t("search_results.no_results_hint")} }
                if active_filters_count > 0 {
                    button {
                        class: "td-btn",
                        onclick: clear_filters,
                        {i18n.t("search_results.clear_filters")}
                    }
                }
            }
        } else {
            div { class: if view_mode() == ViewMode::Card { "sr-results sr-card-grid" } else { "sr-results sr-list" },
                for result in paged_results.iter() {
                    {
                        let _pid = result.person_id;
                        let sex_class = match result.sex {
                            Sex::Male => "male",
                            Sex::Female => "female",
                            Sex::Unknown => "",
                        };
                        let initials: String = {
                            let first_c = result.given_names.chars().next().map(|c| c.to_ascii_uppercase());
                            let last_c = result.surname.chars().next().map(|c| c.to_ascii_uppercase());
                            match (first_c, last_c) {
                                (Some(f), Some(l)) => format!("{f}{l}"),
                                (Some(f), None) => f.to_string(),
                                (None, Some(l)) => l.to_string(),
                                _ => "?".to_string(),
                            }
                        };
                        let tree_id_click = tree_id.clone();
                        let pid_str = result.person_id.to_string();
                        rsx! {
                            div {
                                class: "sr-result-item {sex_class}",
                                onclick: move |_| {
                                    nav.push(Route::TreeDetail {
                                        tree_id: tree_id_click.clone(),
                                        person: Some(pid_str.clone()),
                                    });
                                },
                                div { class: "sr-result-photo",
                                    span { class: "sr-result-initials {sex_class}", "{initials}" }
                                }
                                div { class: "sr-result-info",
                                    div { class: "sr-result-name",
                                        span { class: "sr-surname", "{result.surname}" }
                                        span { class: "sr-given", " {result.given_names}" }
                                    }
                                    div { class: "sr-result-dates",
                                        if let Some(ref bd) = result.birth_date {
                                            span { class: "sr-birth", "\u{2726} {bd}" }
                                        }
                                        if let Some(ref dd) = result.death_date {
                                            span { class: "sr-death", "\u{271D} {dd}" }
                                        }
                                    }
                                    if result.birth_place.is_some() || result.spouse_summary.is_some() {
                                        div { class: "sr-result-meta",
                                            if let Some(ref place) = result.birth_place {
                                                span { class: "sr-place", "{place}" }
                                            }
                                            if let Some(ref spouse) = result.spouse_summary {
                                                span { class: "sr-spouse",
                                                    {i18n.t_args("search_results.spouse_label", &[("name", spouse)])}
                                                }
                                                if result.child_count > 0 {
                                                    span { class: "sr-children",
                                                        " \u{00B7} "
                                                        {i18n.t_args("search_results.children_count", &[("count", &result.child_count.to_string())])}
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // ── Pagination ──
            if total_pages > 1 {
                div { class: "sr-pagination",
                    button {
                        class: "sr-page-btn",
                        disabled: page == 0,
                        onclick: move |_| current_page.set(page.saturating_sub(1)),
                        "\u{2190}"
                    }
                    for p in pagination_range(page, total_pages) {
                        {
                            let p_val = p;
                            let label = (p_val + 1).to_string();
                            rsx! {
                                button {
                                    class: if p_val == page { "sr-page-btn active" } else { "sr-page-btn" },
                                    onclick: move |_| current_page.set(p_val),
                                    "{label}"
                                }
                            }
                        }
                    }
                    button {
                        class: "sr-page-btn",
                        disabled: page >= total_pages.saturating_sub(1),
                        onclick: move |_| current_page.set((page + 1).min(total_pages.saturating_sub(1))),
                        "\u{2192}"
                    }
                    span { class: "sr-page-info",
                        {i18n.t_args("search_results.page_info", &[
                            ("current", &(page + 1).to_string()),
                            ("total", &total_pages.to_string()),
                        ])}
                    }
                }
            }
        }

        } // close .sr-content
        } // close .search-results-page
    }
}

/// Extract name parts (given, surname, nickname) from name map.
fn name_parts(
    person_id: Uuid,
    name_map: &HashMap<Uuid, Vec<PersonName>>,
) -> (Option<String>, Option<String>, Option<String>) {
    let Some(names) = name_map.get(&person_id) else {
        return (None, None, None);
    };
    let name = names
        .iter()
        .find(|n| n.is_primary)
        .or_else(|| names.first());
    match name {
        Some(n) => (n.given_names.clone(), n.surname.clone(), n.nickname.clone()),
        None => (None, None, None),
    }
}

/// Extract a 4-digit year from a date string (e.g. "30 DEC 1982" -> 1982).
fn extract_year(date_str: &str) -> Option<i32> {
    date_str.split_whitespace().rev().find_map(|part| {
        part.parse::<i32>()
            .ok()
            .filter(|&y| (100..=3000).contains(&y))
    })
}

/// Compute page numbers to display (first, last, current +/- 2).
fn pagination_range(current: usize, total: usize) -> Vec<usize> {
    if total <= 7 {
        return (0..total).collect();
    }
    let mut pages = Vec::new();
    pages.push(0);
    let start = current.saturating_sub(2).max(1);
    let end = (current + 3).min(total);
    for p in start..end {
        pages.push(p);
    }
    if total > 1 {
        pages.push(total - 1);
    }
    pages.dedup();
    pages
}

/// Isolated search bar — lives in its own component so keystrokes only
/// re-render this lightweight widget, not the entire SearchResults page.
#[component]
fn SearchBar(
    tree_id: String,
    initial_last: String,
    initial_first: String,
    committed_last: Signal<String>,
    committed_first: Signal<String>,
    current_page: Signal<usize>,
) -> Element {
    let i18n = use_i18n();
    let nav = use_navigator();
    let mut search_last = use_signal(|| initial_last.clone());
    let mut search_first = use_signal(|| initial_first.clone());

    let mut commit = move || {
        committed_last.set(search_last());
        committed_first.set(search_first());
        current_page.set(0);
    };

    let mut commit2 = commit;
    let mut commit3 = commit;
    let tree_id_esc1 = tree_id.clone();
    let tree_id_esc2 = tree_id.clone();

    let on_keydown_last = move |e: Event<KeyboardData>| {
        if e.key() == Key::Enter {
            commit();
        } else if e.key() == Key::Escape {
            nav.push(Route::TreeDetail {
                tree_id: tree_id_esc1.clone(),
                person: None,
            });
        }
    };
    let on_keydown_first = move |e: Event<KeyboardData>| {
        if e.key() == Key::Enter {
            commit2();
        } else if e.key() == Key::Escape {
            nav.push(Route::TreeDetail {
                tree_id: tree_id_esc2.clone(),
                person: None,
            });
        }
    };
    let on_btn = move |_| {
        commit3();
    };

    rsx! {
        div { class: "td-search-group",
            input {
                r#type: "text",
                class: "td-search-input",
                placeholder: "{i18n.t(\"tree.search_last\")}",
                value: "{search_last}",
                oninput: move |e: Event<FormData>| search_last.set(e.value()),
                onkeydown: on_keydown_last,
            }
            input {
                r#type: "text",
                class: "td-search-input",
                placeholder: "{i18n.t(\"tree.search_first\")}",
                value: "{search_first}",
                oninput: move |e: Event<FormData>| search_first.set(e.value()),
                onkeydown: on_keydown_first,
            }
            button {
                class: "td-search-btn",
                title: "{i18n.t(\"search_results.search\")}",
                onclick: on_btn,
                svg {
                    width: "14",
                    height: "14",
                    fill: "none",
                    "viewBox": "0 0 24 24",
                    stroke: "currentColor",
                    "strokeWidth": "2.5",
                    circle { cx: "11", cy: "11", r: "8" }
                    line { x1: "21", y1: "21", x2: "16.65", y2: "16.65" }
                }
            }
            Link {
                class: "td-back-btn",
                to: Route::TreeDetail { tree_id: tree_id.clone(), person: None },
                title: "{i18n.t(\"search_results.back_to_tree\")}",
                svg {
                    width: "16",
                    height: "16",
                    fill: "none",
                    "viewBox": "0 0 24 24",
                    stroke: "currentColor",
                    "strokeWidth": "2",
                    path { d: "M17 21v-2a4 4 0 00-4-4H5" }
                    path { d: "M9 19l-4-4 4-4" }
                    path { d: "M21 12V7a2 2 0 00-2-2h-4" }
                }
            }
        }
    }
}
