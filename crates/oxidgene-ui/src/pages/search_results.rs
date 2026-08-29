//! Full-page search results powered by the server-side cache search index.
//!
//! Combines server-side accent-folded matching with genealogical filters,
//! sorting, and pagination.
//! Uses the shared tree sub-page layout and icon sidebar.

use dioxus::prelude::*;
use oxidgene_core::projection::SearchEntry;
use oxidgene_core::{EventType, Sex};
use uuid::Uuid;

use crate::api::{ApiClient, PersonSearchParams, PersonSearchSort};
use crate::components::pedigree_chart::default_portrait;
use crate::components::person_form::FormSection;
use crate::components::tree_cache::{fetch_tree_cached, use_tree_cache};
use crate::components::tree_icon_sidebar::{TreeIconSidebar, TreeSidebarView};
use crate::i18n::use_i18n;
use crate::router::Route;

const RESULTS_PER_PAGE: usize = 25;
/// Card (grid) view shows fewer results per page — each cell embeds a
/// mini-pedigree, so a full list-sized page would overload the layout
/// (and fire as many pedigree fetches).
const GRID_RESULTS_PER_PAGE: usize = 20;
// ── Enums ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
enum SortOrder {
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

fn non_empty(value: String) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn parse_filter_year(value: &str) -> Option<i32> {
    value.trim().parse().ok()
}

fn parse_event_type(value: &str) -> Option<EventType> {
    match value {
        "birth" => Some(EventType::Birth),
        "death" => Some(EventType::Death),
        "baptism" => Some(EventType::Baptism),
        "burial" => Some(EventType::Burial),
        "marriage" => Some(EventType::Marriage),
        "residence" => Some(EventType::Residence),
        "occupation" => Some(EventType::Occupation),
        "census" => Some(EventType::Census),
        _ => None,
    }
}

fn has_search_criteria(search: &PersonSearchParams) -> bool {
    !search.query.trim().is_empty()
        || search.sex.is_some()
        || search.surname.is_some()
        || search.given_names.is_some()
        || search.occupation.is_some()
        || search.spouse_surname.is_some()
        || search.spouse_given_names.is_some()
        || search.father_surname.is_some()
        || search.father_given_names.is_some()
        || search.mother_surname.is_some()
        || search.mother_given_names.is_some()
        || search.birth_from.is_some()
        || search.birth_to.is_some()
        || search.death_from.is_some()
        || search.death_to.is_some()
        || search.place.is_some()
        || search.event_type.is_some()
        || search.event_from.is_some()
        || search.event_to.is_some()
        || search.has_media
}

// ── Component Props ──────────────────────────────────────────────────────

#[derive(Props, Clone, PartialEq)]
pub struct SearchResultsProps {
    pub tree_id: String,
    #[props(default = String::new())]
    pub last: String,
    #[props(default = String::new())]
    pub first: String,
    /// Which view the search was launched from — see [`Route::SearchResults`].
    #[props(default = String::new())]
    pub origin: String,
}

// ── SearchResults Component ──────────────────────────────────────────────

#[component]
pub fn SearchResults(props: SearchResultsProps) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let nav = navigator();

    let tree_id = Uuid::parse_str(&props.tree_id).ok();
    let tree_cache = use_tree_cache();
    let api_tree = api.clone();
    let tree_resource = use_resource(move || {
        let api = api_tree.clone();
        let _generation = tree_cache.generation();
        async move {
            let tree_id = tree_id?;
            Some(fetch_tree_cached(&api, &tree_cache, tree_id).await)
        }
    });
    let tree_name = match &*tree_resource.read() {
        Some(Some(Ok(tree))) => tree.name.clone(),
        _ => tree_id
            .and_then(|tree_id| tree_cache.tree(tree_id))
            .map(|tree| tree.name)
            .unwrap_or_default(),
    };
    let selected_person_id = match &*tree_resource.read() {
        Some(Some(Ok(tree))) => tree.sosa_root_person_id,
        _ => tree_id
            .and_then(|tree_id| tree_cache.tree(tree_id))
            .and_then(|tree| tree.sosa_root_person_id),
    };

    let api_portraits = api.clone();
    let portraits_resource = use_resource(move || {
        let api = api_portraits.clone();
        async move {
            match tree_id {
                Some(tree_id) => match api.list_portraits(tree_id).await {
                    Ok(rows) => api.portrait_map(tree_id, &rows).await,
                    Err(_) => Default::default(),
                },
                None => Default::default(),
            }
        }
    });

    // ── Search query state ──
    let mut search_last = use_signal(|| props.last.clone());
    let mut search_first = use_signal(|| props.first.clone());
    let mut committed_last = use_signal(|| props.last.clone());
    let mut committed_first = use_signal(|| props.first.clone());

    // ── Filter/sort/view state ──
    let mut gender_filter = use_signal(|| GenderFilter::All);
    let mut sort_order = use_signal(|| SortOrder::NameAZ);
    let mut view_mode = use_signal(|| ViewMode::List);
    let mut current_page = use_signal(|| 1_usize);
    let mut show_filters = use_signal(|| false);
    let person_filters_open = use_signal(|| true);
    let event_filters_open = use_signal(|| true);
    let relation_filters_open = use_signal(|| true);
    let mut born_from = use_signal(String::new);
    let mut born_to = use_signal(String::new);
    let mut died_from = use_signal(String::new);
    let mut died_to = use_signal(String::new);
    let mut occupation_filter = use_signal(String::new);
    let mut spouse_surname = use_signal(String::new);
    let mut spouse_given_names = use_signal(String::new);
    let mut father_surname = use_signal(String::new);
    let mut father_given_names = use_signal(String::new);
    let mut mother_surname = use_signal(String::new);
    let mut mother_given_names = use_signal(String::new);
    let mut place_filter = use_signal(String::new);
    let mut event_type_filter = use_signal(|| None::<EventType>);
    let mut event_from = use_signal(String::new);
    let mut event_to = use_signal(String::new);
    let mut has_media = use_signal(|| false);

    // Sync props into signals when navigation changes the query parameters.
    let prop_last = props.last.clone();
    let prop_first = props.first.clone();
    use_effect(move || {
        search_last.set(prop_last.clone());
        search_first.set(prop_first.clone());
        committed_last.set(prop_last.clone());
        committed_first.set(prop_first.clone());
        current_page.set(1);
    });

    // ── Server-side search ──
    let api_search = api.clone();
    let search_resource = use_resource(move || {
        let api = api_search.clone();
        let page = current_page();
        let per_page = match view_mode() {
            ViewMode::List => RESULTS_PER_PAGE,
            ViewMode::Card => GRID_RESULTS_PER_PAGE,
        };
        let params = PersonSearchParams {
            query: String::new(),
            limit: per_page as u32,
            offset: ((page.saturating_sub(1)) * per_page) as u32,
            sex: match gender_filter() {
                GenderFilter::All => None,
                GenderFilter::Male => Some(Sex::Male),
                GenderFilter::Female => Some(Sex::Female),
                GenderFilter::Unknown => Some(Sex::Unknown),
            },
            surname: non_empty(committed_last()),
            given_names: non_empty(committed_first()),
            occupation: non_empty(occupation_filter()),
            spouse_surname: non_empty(spouse_surname()),
            spouse_given_names: non_empty(spouse_given_names()),
            father_surname: non_empty(father_surname()),
            father_given_names: non_empty(father_given_names()),
            mother_surname: non_empty(mother_surname()),
            mother_given_names: non_empty(mother_given_names()),
            birth_from: parse_filter_year(&born_from()),
            birth_to: parse_filter_year(&born_to()),
            death_from: parse_filter_year(&died_from()),
            death_to: parse_filter_year(&died_to()),
            place: non_empty(place_filter()),
            event_type: event_type_filter(),
            event_from: parse_filter_year(&event_from()),
            event_to: parse_filter_year(&event_to()),
            has_media: has_media(),
            sort: match sort_order() {
                SortOrder::NameAZ => PersonSearchSort::NameAsc,
                SortOrder::NameZA => PersonSearchSort::NameDesc,
                SortOrder::BirthAsc => PersonSearchSort::BirthAsc,
                SortOrder::BirthDesc => PersonSearchSort::BirthDesc,
            },
        };
        let has_criteria = has_search_criteria(&params);
        async move {
            if !has_criteria {
                return Ok(None);
            }
            let Some(tid) = tree_id else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: "Invalid tree ID".into(),
                });
            };
            crate::utils::sleep_ms(200).await;
            api.search_persons_filtered(tid, &params).await.map(Some)
        }
    });

    // Search action: combine last + first into a query string.
    //
    // Mirrors `TopbarSearch::do_search` — a family-name-only, numeric query
    // is tried as a SOSA-Stradonitz number first, jumping straight to the
    // matching person, and only falls back to a normal name search if no
    // person exists at that number (or the tree has no SOSA root).
    let api_search_action = api.clone();
    let origin = props.origin.clone();
    let tree_id_str = props.tree_id.clone();
    let mut do_search = move || {
        let last = search_last();
        let first = search_first();
        let last_trim = last.trim();
        let first_trim = first.trim();
        if last_trim.is_empty() && first_trim.is_empty() {
            return;
        }

        if first_trim.is_empty()
            && let Ok(number) = last_trim.parse::<u64>()
            && let Some(tid) = tree_id
        {
            let api = api_search_action.clone();
            let origin = origin.clone();
            let tree_id_str = tree_id_str.clone();
            spawn(async move {
                match api.get_person_by_sosa(tid, number).await {
                    Ok(person) => {
                        let person_id = person.id.to_string();
                        if origin == "person" {
                            nav.push(Route::PersonDetail {
                                tree_id: tree_id_str,
                                person_id,
                            });
                        } else {
                            nav.push(Route::TreeDetail {
                                tree_id: tree_id_str,
                                person: Some(person_id),
                            });
                        }
                    }
                    Err(_) => {
                        committed_last.set(last);
                        committed_first.set(String::new());
                        current_page.set(1);
                    }
                }
            });
            return;
        }

        committed_last.set(last);
        committed_first.set(first);
        current_page.set(1);
    };

    let mut do_search2 = do_search.clone();
    let mut do_search3 = do_search.clone();
    let on_search_enter = move |e: Event<KeyboardData>| {
        if e.key() == Key::Enter {
            do_search();
        }
    };
    let on_search_enter2 = move |e: Event<KeyboardData>| {
        if e.key() == Key::Enter {
            do_search2();
        }
    };

    // ── Server-filtered, sorted, and paginated result ──
    let all_entries: Vec<SearchEntry> = {
        let data = search_resource.read();
        match &*data {
            Some(Ok(Some(sr))) => sr.entries.clone(),
            _ => vec![],
        }
    };
    let per_page = match view_mode() {
        ViewMode::List => RESULTS_PER_PAGE,
        ViewMode::Card => GRID_RESULTS_PER_PAGE,
    };
    let total_filtered = {
        let data = search_resource.read();
        match &*data {
            Some(Ok(Some(result))) => result.total_count,
            _ => 0,
        }
    };
    let page = current_page();
    let total_pages = total_filtered.div_ceil(per_page).max(1);
    let page_results: Vec<&SearchEntry> = all_entries.iter().collect();
    let portrait_urls = {
        let data = portraits_resource.read();
        match &*data {
            Some(urls) => urls.clone(),
            None => Default::default(),
        }
    };

    let no_query = matches!(&*search_resource.read(), Some(Ok(None)));
    let is_loading = !no_query && search_resource.read().is_none();
    let is_error = matches!(&*search_resource.read(), Some(Err(_)));

    // ── Render ──
    rsx! {
        div { class: "sub-page search-results-page",
            // ── Topbar (shared td-topbar / td-bc classes per spec §3) ──
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
                            to: Route::TreeDetail { tree_id: props.tree_id.clone(), person: None },
                            class: "td-bc-link",
                            "{tree_name}"
                        }
                        span { class: "td-bc-sep", "/" }
                    }
                    span { class: "td-bc-current", {i18n.t("search.title")} }
                }
                div { class: "td-search-group",
                    input {
                        r#type: "text",
                        class: "td-search-input",
                        placeholder: "{i18n.t(\"tree.search_last\")}",
                        value: "{search_last}",
                        oninput: move |e: Event<FormData>| search_last.set(e.value()),
                        onkeydown: on_search_enter,
                    }
                    input {
                        r#type: "text",
                        class: "td-search-input",
                        placeholder: "{i18n.t(\"tree.search_first\")}",
                        value: "{search_first}",
                        oninput: move |e: Event<FormData>| search_first.set(e.value()),
                        onkeydown: on_search_enter2,
                    }
                    button {
                        class: "td-search-btn",
                        title: "{i18n.t(\"tree.search\")}",
                        onclick: move |_| do_search3(),
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
                }
            }

            div { class: "pd-page-shell",
                TreeIconSidebar {
                    active_view: TreeSidebarView::None,
                    selected_person_id,
                    show_middle_separator: false,
                    show_add_person: false,
                    on_profile_view: {
                        let tree_id = props.tree_id.clone();
                        move |person_id: Option<Uuid>| {
                            if let Some(person_id) = person_id {
                                nav.push(Route::PersonDetail {
                                    tree_id: tree_id.clone(),
                                    person_id: person_id.to_string(),
                                });
                            }
                        }
                    },
                    on_pedigree_view: {
                        let tree_id = props.tree_id.clone();
                        move |person_id: Option<Uuid>| {
                            nav.push(Route::TreeDetail {
                                tree_id: tree_id.clone(),
                                person: person_id.map(|person_id| person_id.to_string()),
                            });
                        }
                    },
                    on_add_person: move |_| {},
                    on_dictionary: {
                        let tree_id = props.tree_id.clone();
                        move |_| {
                            nav.push(Route::Dictionary { tree_id: tree_id.clone() });
                        }
                    },
                    on_settings: {
                        let tree_id = props.tree_id.clone();
                        move |_| {
                            nav.push(Route::Settings { tree_id: tree_id.clone() });
                        }
                    },
                }

                // ── Scrollable content ──
                div { class: "sub-page-content",

                // ── Filter panel ──
                div { class: "sr-filters-toggle",
                    button {
                        class: "btn btn-outline btn-sm",
                        onclick: move |_| show_filters.set(!show_filters()),
                        span { class: if show_filters() { "sr-chevron open" } else { "sr-chevron" }, "\u{25BC}" }
                        " {i18n.t(\"search.filters\")}"
                    }
                }
                if show_filters() {
                    div { class: "sr-filters pf-embedded",
                        FormSection {
                            title: i18n.t("search.person_criteria"),
                            open: person_filters_open,
                            div { class: "sr-filter-grid sr-filter-grid-person",
                            div { class: "sr-filter-group",
                                label { {i18n.t("search.surname")} }
                                input {
                                    r#type: "text",
                                    value: "{search_last}",
                                    oninput: move |e: Event<FormData>| {
                                        let value = e.value();
                                        search_last.set(value.clone());
                                        committed_last.set(value);
                                        current_page.set(1);
                                    },
                                }
                            }
                            div { class: "sr-filter-group",
                                label { {i18n.t("search.given_names")} }
                                input {
                                    r#type: "text",
                                    value: "{search_first}",
                                    oninput: move |e: Event<FormData>| {
                                        let value = e.value();
                                        search_first.set(value.clone());
                                        committed_first.set(value);
                                        current_page.set(1);
                                    },
                                }
                            }
                            div { class: "sr-filter-group sr-filter-sex",
                                label { {i18n.t("search.gender")} }
                                div { class: "pf-gender-group",
                                    button {
                                        r#type: "button",
                                        class: if gender_filter() == GenderFilter::All { "pf-gender-btn active" } else { "pf-gender-btn" },
                                        onclick: move |_| {
                                            gender_filter.set(GenderFilter::All);
                                            current_page.set(1);
                                        },
                                        {i18n.t("search.all")}
                                    }
                                    button {
                                        r#type: "button",
                                        class: if gender_filter() == GenderFilter::Male { "pf-gender-btn active" } else { "pf-gender-btn" },
                                        onclick: move |_| {
                                            gender_filter.set(GenderFilter::Male);
                                            current_page.set(1);
                                        },
                                        {i18n.t("search.male")}
                                    }
                                    button {
                                        r#type: "button",
                                        class: if gender_filter() == GenderFilter::Female { "pf-gender-btn active" } else { "pf-gender-btn" },
                                        onclick: move |_| {
                                            gender_filter.set(GenderFilter::Female);
                                            current_page.set(1);
                                        },
                                        {i18n.t("search.female")}
                                    }
                                    button {
                                        r#type: "button",
                                        class: if gender_filter() == GenderFilter::Unknown { "pf-gender-btn active" } else { "pf-gender-btn" },
                                        onclick: move |_| {
                                            gender_filter.set(GenderFilter::Unknown);
                                            current_page.set(1);
                                        },
                                        {i18n.t("search.unknown")}
                                    }
                                }
                            }
                            div { class: "sr-filter-group",
                                label { {i18n.t("search.occupation")} }
                                input {
                                    r#type: "text",
                                    value: "{occupation_filter}",
                                    oninput: move |e: Event<FormData>| {
                                        occupation_filter.set(e.value());
                                        current_page.set(1);
                                    },
                                }
                            }
                            div { class: "sr-filter-group",
                                label { {i18n.t("search.born_between")} }
                                div { class: "sr-date-range",
                                    input {
                                        r#type: "number",
                                        placeholder: "1800",
                                        value: "{born_from}",
                                        oninput: move |e: Event<FormData>| {
                                            born_from.set(e.value());
                                            current_page.set(1);
                                        },
                                    }
                                    span { "\u{2013}" }
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
                            }
                            div { class: "sr-filter-group",
                                label { {i18n.t("search.died_between")} }
                                div { class: "sr-date-range",
                                    input {
                                        r#type: "number",
                                        placeholder: "1800",
                                        value: "{died_from}",
                                        oninput: move |e: Event<FormData>| {
                                            died_from.set(e.value());
                                            current_page.set(1);
                                        },
                                    }
                                    span { "\u{2013}" }
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
                            }
                            label { class: "sr-media-filter",
                                input {
                                    r#type: "checkbox",
                                    checked: has_media(),
                                    onchange: move |e: Event<FormData>| {
                                        has_media.set(e.checked());
                                        current_page.set(1);
                                    },
                                }
                                span { {i18n.t("search.has_media")} }
                            }
                            }
                        }

                        FormSection {
                            title: i18n.t("search.event_criteria"),
                            open: event_filters_open,
                            div { class: "sr-filter-grid sr-filter-grid-event",
                            div { class: "sr-filter-group",
                                label { {i18n.t("search.event_type")} }
                                select {
                                    value: event_type_filter().map(|event| event.to_string()).unwrap_or_default(),
                                    onchange: move |e: Event<FormData>| {
                                        event_type_filter.set(parse_event_type(&e.value()));
                                        current_page.set(1);
                                    },
                                    option { value: "", {i18n.t("search.all_events")} }
                                    option { value: "birth", {i18n.t("event.type.birth")} }
                                    option { value: "death", {i18n.t("event.type.death")} }
                                    option { value: "baptism", {i18n.t("event.type.baptism")} }
                                    option { value: "burial", {i18n.t("event.type.burial")} }
                                    option { value: "marriage", {i18n.t("event.type.marriage")} }
                                    option { value: "residence", {i18n.t("event.type.residence")} }
                                    option { value: "occupation", {i18n.t("event.type.occupation")} }
                                    option { value: "census", {i18n.t("event.type.census")} }
                                }
                            }
                            div { class: "sr-filter-group",
                                label { {i18n.t("search.place")} }
                                input {
                                    r#type: "text",
                                    value: "{place_filter}",
                                    oninput: move |e: Event<FormData>| {
                                        place_filter.set(e.value());
                                        current_page.set(1);
                                    },
                                }
                            }
                            div { class: "sr-filter-group",
                                label { {i18n.t("search.event_between")} }
                                div { class: "sr-date-range",
                                    input {
                                        r#type: "number",
                                        placeholder: "1800",
                                        value: "{event_from}",
                                        oninput: move |e: Event<FormData>| {
                                            event_from.set(e.value());
                                            current_page.set(1);
                                        },
                                    }
                                    span { "\u{2013}" }
                                    input {
                                        r#type: "number",
                                        placeholder: "2000",
                                        value: "{event_to}",
                                        oninput: move |e: Event<FormData>| {
                                            event_to.set(e.value());
                                            current_page.set(1);
                                        },
                                    }
                                }
                            }
                            }
                        }

                        FormSection {
                            title: i18n.t("search.relation_criteria"),
                            open: relation_filters_open,
                            div { class: "sr-relations-grid",
                            div { class: "sr-relation-group pf-subform",
                                div { class: "pf-block-label", {i18n.t("search.spouse")} }
                                input {
                                    r#type: "text",
                                    placeholder: "{i18n.t(\"search.surname\")}",
                                    value: "{spouse_surname}",
                                    oninput: move |e: Event<FormData>| {
                                        spouse_surname.set(e.value());
                                        current_page.set(1);
                                    },
                                }
                                input {
                                    r#type: "text",
                                    placeholder: "{i18n.t(\"search.given_names\")}",
                                    value: "{spouse_given_names}",
                                    oninput: move |e: Event<FormData>| {
                                        spouse_given_names.set(e.value());
                                        current_page.set(1);
                                    },
                                }
                            }
                            div { class: "sr-relation-group pf-subform",
                                div { class: "pf-block-label", {i18n.t("search.father")} }
                                input {
                                    r#type: "text",
                                    placeholder: "{i18n.t(\"search.surname\")}",
                                    value: "{father_surname}",
                                    oninput: move |e: Event<FormData>| {
                                        father_surname.set(e.value());
                                        current_page.set(1);
                                    },
                                }
                                input {
                                    r#type: "text",
                                    placeholder: "{i18n.t(\"search.given_names\")}",
                                    value: "{father_given_names}",
                                    oninput: move |e: Event<FormData>| {
                                        father_given_names.set(e.value());
                                        current_page.set(1);
                                    },
                                }
                            }
                            div { class: "sr-relation-group pf-subform",
                                div { class: "pf-block-label", {i18n.t("search.mother")} }
                                input {
                                    r#type: "text",
                                    placeholder: "{i18n.t(\"search.surname\")}",
                                    value: "{mother_surname}",
                                    oninput: move |e: Event<FormData>| {
                                        mother_surname.set(e.value());
                                        current_page.set(1);
                                    },
                                }
                                input {
                                    r#type: "text",
                                    placeholder: "{i18n.t(\"search.given_names\")}",
                                    value: "{mother_given_names}",
                                    oninput: move |e: Event<FormData>| {
                                        mother_given_names.set(e.value());
                                        current_page.set(1);
                                    },
                                }
                            }
                            }
                        }
                        div { class: "sr-filter-actions",
                            button {
                                class: "pf-row-btn",
                                onclick: move |_| {
                                    search_last.set(String::new());
                                    search_first.set(String::new());
                                    committed_last.set(String::new());
                                    committed_first.set(String::new());
                                    gender_filter.set(GenderFilter::All);
                                    occupation_filter.set(String::new());
                                    born_from.set(String::new());
                                    born_to.set(String::new());
                                    died_from.set(String::new());
                                    died_to.set(String::new());
                                    spouse_surname.set(String::new());
                                    spouse_given_names.set(String::new());
                                    father_surname.set(String::new());
                                    father_given_names.set(String::new());
                                    mother_surname.set(String::new());
                                    mother_given_names.set(String::new());
                                    place_filter.set(String::new());
                                    event_type_filter.set(None);
                                    event_from.set(String::new());
                                    event_to.set(String::new());
                                    has_media.set(false);
                                    current_page.set(1);
                                },
                                {i18n.t("search.clear_filters")}
                            }
                        }
                    }
                }

                div { class: "sr-active-filters",
                    if gender_filter() != GenderFilter::All {
                        button {
                            class: "sr-filter-chip",
                            title: "{i18n.t(\"search.clear_filters\")}",
                            onclick: move |_| {
                                gender_filter.set(GenderFilter::All);
                                current_page.set(1);
                            },
                            {match gender_filter() {
                                GenderFilter::Male => i18n.t("search.male"),
                                GenderFilter::Female => i18n.t("search.female"),
                                GenderFilter::Unknown => i18n.t("search.unknown"),
                                GenderFilter::All => String::new(),
                            }}
                            span { " \u{00D7}" }
                        }
                    }
                    if !occupation_filter().trim().is_empty() {
                        button {
                            class: "sr-filter-chip",
                            onclick: move |_| {
                                occupation_filter.set(String::new());
                                current_page.set(1);
                            },
                            {format!("{}: {}", i18n.t("search.occupation"), occupation_filter())}
                            span { " \u{00D7}" }
                        }
                    }
                    if !born_from().is_empty() || !born_to().is_empty() {
                        button {
                            class: "sr-filter-chip",
                            onclick: move |_| {
                                born_from.set(String::new());
                                born_to.set(String::new());
                                current_page.set(1);
                            },
                            {i18n.t("search.born_between")}
                            span { " \u{00D7}" }
                        }
                    }
                    if !died_from().is_empty() || !died_to().is_empty() {
                        button {
                            class: "sr-filter-chip",
                            onclick: move |_| {
                                died_from.set(String::new());
                                died_to.set(String::new());
                                current_page.set(1);
                            },
                            {i18n.t("search.died_between")}
                            span { " \u{00D7}" }
                        }
                    }
                    if !place_filter().trim().is_empty() {
                        button {
                            class: "sr-filter-chip",
                            onclick: move |_| {
                                place_filter.set(String::new());
                                current_page.set(1);
                            },
                            {format!("{}: {}", i18n.t("search.place"), place_filter())}
                            span { " \u{00D7}" }
                        }
                    }
                    if event_type_filter().is_some() || !event_from().is_empty() || !event_to().is_empty() {
                        button {
                            class: "sr-filter-chip",
                            onclick: move |_| {
                                event_type_filter.set(None);
                                event_from.set(String::new());
                                event_to.set(String::new());
                                current_page.set(1);
                            },
                            {i18n.t("search.event_criteria")}
                            span { " \u{00D7}" }
                        }
                    }
                    if !spouse_surname().trim().is_empty() || !spouse_given_names().trim().is_empty() {
                        button {
                            class: "sr-filter-chip",
                            onclick: move |_| {
                                spouse_surname.set(String::new());
                                spouse_given_names.set(String::new());
                                current_page.set(1);
                            },
                            {i18n.t("search.spouse")}
                            span { " \u{00D7}" }
                        }
                    }
                    if !father_surname().trim().is_empty() || !father_given_names().trim().is_empty() {
                        button {
                            class: "sr-filter-chip",
                            onclick: move |_| {
                                father_surname.set(String::new());
                                father_given_names.set(String::new());
                                current_page.set(1);
                            },
                            {i18n.t("search.father")}
                            span { " \u{00D7}" }
                        }
                    }
                    if !mother_surname().trim().is_empty() || !mother_given_names().trim().is_empty() {
                        button {
                            class: "sr-filter-chip",
                            onclick: move |_| {
                                mother_surname.set(String::new());
                                mother_given_names.set(String::new());
                                current_page.set(1);
                            },
                            {i18n.t("search.mother")}
                            span { " \u{00D7}" }
                        }
                    }
                    if has_media() {
                        button {
                            class: "sr-filter-chip",
                            onclick: move |_| {
                                has_media.set(false);
                                current_page.set(1);
                            },
                            {i18n.t("search.has_media")}
                            span { " \u{00D7}" }
                        }
                    }
                }

                // ── Sort / view toolbar ──
                div { class: "sr-toolbar",
                    span { class: "sr-count",
                        {format!("{} {}", total_filtered, i18n.t("search.results"))}
                    }
                    div { class: "sr-sort",
                        select {
                            value: "{sort_order():?}",
                            onchange: move |e: Event<FormData>| {
                                sort_order.set(match e.value().as_str() {
                                    "NameAZ" => SortOrder::NameAZ,
                                    "NameZA" => SortOrder::NameZA,
                                    "BirthAsc" => SortOrder::BirthAsc,
                                    "BirthDesc" => SortOrder::BirthDesc,
                                    _ => SortOrder::NameAZ,
                                });
                            },
                            option { value: "NameAZ", {i18n.t("search.sort_name_az")} }
                            option { value: "NameZA", {i18n.t("search.sort_name_za")} }
                            option { value: "BirthAsc", {i18n.t("search.sort_birth_asc")} }
                            option { value: "BirthDesc", {i18n.t("search.sort_birth_desc")} }
                        }
                    }
                    div { class: "sr-view-modes",
                        button {
                            class: if view_mode() == ViewMode::List { "sr-view-btn active" } else { "sr-view-btn" },
                            title: "{i18n.t(\"search.view_list\")}",
                            onclick: move |_| {
                                if view_mode() != ViewMode::List {
                                    view_mode.set(ViewMode::List);
                                    current_page.set(1);
                                }
                            },
                            "\u{2630}"
                        }
                        button {
                            class: if view_mode() == ViewMode::Card { "sr-view-btn active" } else { "sr-view-btn" },
                            title: "{i18n.t(\"search.view_grid\")}",
                            onclick: move |_| {
                                if view_mode() != ViewMode::Card {
                                    view_mode.set(ViewMode::Card);
                                    current_page.set(1);
                                }
                            },
                            "\u{25A6}"
                        }
                    }
                }

                // ── Results ──
                if no_query {
                    div { class: "sr-empty",
                        p { {i18n.t("search.start_search")} }
                    }
                } else if is_loading {
                    div { class: "sr-empty", {i18n.t("search.loading")} }
                } else if is_error {
                    div { class: "sr-empty", {i18n.t("search.error")} }
                } else if page_results.is_empty() {
                    div { class: "sr-empty",
                        p { {i18n.t("search.no_results")} }
                    }
                } else if view_mode() == ViewMode::Card {
                    div { class: "sr-grid",
                        for entry in page_results.iter() {
                            SearchPedigreeCard {
                                key: "{entry.person_id}",
                                tree_id: tree_id.unwrap_or_default(),
                                tree_id_str: props.tree_id.clone(),
                                person_id: entry.person_id,
                                given_names: entry.given_names.clone(),
                                surname: entry.surname.clone(),
                                sex: entry.sex,
                                birth_year: entry.birth_year.clone(),
                                death_year: entry.death_year.clone(),
                                origin: props.origin.clone(),
                            }
                        }
                    }
                } else {
                    div {
                        class: "search-person-results sr-results-page",
                        for entry in page_results.iter() {
                            {render_result_item(
                                entry,
                                &props.tree_id,
                                &props.origin,
                                portrait_urls.get(&entry.person_id).cloned(),
                            )}
                        }
                    }
                }

                // ── Pagination ──
                if total_pages > 1 {
                    div { class: "sr-pagination",
                        button {
                            class: "sr-page-btn",
                            disabled: page <= 1,
                            onclick: move |_| current_page.set(page.saturating_sub(1).max(1)),
                            "\u{25C0}"
                        }
                        for p in pagination_range(page, total_pages) {
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
                            disabled: page >= total_pages,
                            onclick: move |_| current_page.set((page + 1).min(total_pages)),
                            "\u{25B6}"
                        }
                    }
                }
                }
            }
        }
    }
}

// ── Result item rendering ────────────────────────────────────────────────
//
// Reuses the same `search-person-result` / `sp-*` CSS classes as the
// SearchPerson typeahead component (used in SOSA root selector, etc.)
// so that person rows look identical everywhere.

fn render_result_item(
    entry: &SearchEntry,
    tree_id: &str,
    origin: &str,
    portrait_url: Option<String>,
) -> Element {
    let sex_class = match entry.sex {
        Sex::Male => "male",
        Sex::Female => "female",
        Sex::Unknown => "",
    };

    let given = entry.given_names.clone();
    let surname = entry.surname.clone();

    let portrait_src = portrait_url.unwrap_or_else(|| default_portrait(entry.sex).to_string());

    let tree_id_str = tree_id.to_string();
    let person_id_str = entry.person_id.to_string();

    let target = if origin == "person" {
        Route::PersonDetail {
            tree_id: tree_id_str,
            person_id: person_id_str,
        }
    } else {
        Route::TreeDetail {
            tree_id: tree_id_str,
            person: Some(person_id_str),
        }
    };

    rsx! {
        Link {
            to: target,
            class: "search-person-result {sex_class}",
            div { class: "sp-result-photo",
                img { class: "sp-result-portrait", src: "{portrait_src}", alt: "" }
            }
            div { class: "sp-result-info",
                div { class: "sp-result-name",
                    if !surname.is_empty() {
                        span { class: "sp-surname", "{surname}" }
                    }
                    span { class: "sp-given", " {given}" }
                    if surname.is_empty() && given.is_empty() {
                        span { class: "sp-given", "?" }
                    }
                }
                div { class: "sp-result-dates",
                    if let Some(ref by) = entry.birth_year {
                        span { class: "sp-birth", "\u{2726} {by}" }
                    }
                    if let Some(ref dy) = entry.death_year {
                        span { class: "sp-death", "\u{271D} {dy}" }
                    }
                }
                if let Some(ref bp) = entry.birth_place {
                    div { class: "sp-result-meta",
                        span { class: "sp-place", "{bp}" }
                    }
                }
            }
        }
    }
}

// ── Grid (Card) view ─────────────────────────────────────────────────────

/// Mini-pedigree zoom inside a grid cell — denser than the person-detail
/// embed (0.8) so three generations fit a card-sized viewport.
const GRID_PEDIGREE_SCALE: f64 = 0.5;

/// One cell of the grid ("Card") view: a clickable header with the person's
/// name and dates above a mini-pedigree (self + parents + grandparents)
/// served by the pedigree cache.
#[component]
fn SearchPedigreeCard(
    tree_id: Uuid,
    tree_id_str: String,
    person_id: Uuid,
    given_names: String,
    surname: String,
    sex: Sex,
    birth_year: Option<String>,
    death_year: Option<String>,
    origin: String,
) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let nav = navigator();

    let pedigree_resource = use_resource(move || {
        let api = api.clone();
        async move { api.get_pedigree(tree_id, person_id, 2, 0).await }
    });

    // Same navigation target rule as the list view: search launched from a
    // person page opens profiles, otherwise the tree centered on the person.
    let route_for = {
        let origin = origin.clone();
        let tree_id_str = tree_id_str.clone();
        move |pid: Uuid| -> Route {
            if origin == "person" {
                Route::PersonDetail {
                    tree_id: tree_id_str.clone(),
                    person_id: pid.to_string(),
                }
            } else {
                Route::TreeDetail {
                    tree_id: tree_id_str.clone(),
                    person: Some(pid.to_string()),
                }
            }
        }
    };
    let header_target = route_for(person_id);
    let route_for_nav = route_for.clone();
    let on_navigate = move |pid: Uuid| {
        nav.push(route_for_nav(pid));
    };

    let sex_class = match sex {
        Sex::Male => "male",
        Sex::Female => "female",
        Sex::Unknown => "",
    };
    let ped = pedigree_resource.read();
    let body = match &*ped {
        Some(Ok(cached)) => {
            let data = crate::components::pedigree_chart::PedigreeData::from_pedigree(cached);
            rsx! {
                crate::components::pedigree_chart::MiniPedigree {
                    root_person_id: person_id,
                    data: data,
                    ancestor_levels: 2,
                    descendant_levels: 0,
                    on_person_navigate: on_navigate,
                    scale: GRID_PEDIGREE_SCALE,
                }
            }
        }
        Some(Err(_)) => rsx! {
            div { class: "sr-grid-ped-msg", {i18n.t("search.error")} }
        },
        None => rsx! {
            div { class: "sr-grid-ped-msg", {i18n.t("search.loading")} }
        },
    };

    rsx! {
        div { class: "sr-grid-card {sex_class}",
            Link { to: header_target, class: "sr-grid-card-hd",
                div { class: "sp-result-name",
                    if !surname.is_empty() {
                        span { class: "sp-surname", "{surname}" }
                    }
                    span { class: "sp-given", " {given_names}" }
                    if surname.is_empty() && given_names.is_empty() {
                        span { class: "sp-given", "?" }
                    }
                }
                div { class: "sp-result-dates",
                    if let Some(ref by) = birth_year {
                        span { class: "sp-birth", "\u{2726} {by}" }
                    }
                    if let Some(ref dy) = death_year {
                        span { class: "sp-death", "\u{271D} {dy}" }
                    }
                }
            }
            div { class: "sr-grid-ped", {body} }
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
