//! Full-page search results powered by the server-side cache search index.
//!
//! Combines server-side accent-folded matching with genealogical filters,
//! sorting, and pagination.
//! Uses the shared tree sub-page layout and icon sidebar.

use std::collections::HashMap;

use dioxus::prelude::*;
use oxidgene_core::projection::{SearchEntry, SearchResult};
use oxidgene_core::{EventType, Sex};
use uuid::Uuid;

use crate::api::{
    ApiClient, ApiError, CroppedSource, PersonSearchParams, PersonSearchSort, SuggestionField,
};
use crate::components::breadcrumb::TreeBreadcrumb;
use crate::components::pedigree_chart::{PedigreeData, SharedPedigree};
use crate::components::person_form::FormSection;
use crate::components::print::{PrintHeading, PrintPageNote, search_print_title};
use crate::components::search_person::{PersonSearchSummary, render_person_search_summary};
use crate::components::suggest_input::ValueInput;
use crate::components::topbar_search::TopbarSearch;
use crate::components::tree_cache::{fetch_tree_cached, use_current_person, use_tree_cache};
use crate::components::tree_icon_sidebar::ToolPageSidebar;
use crate::i18n::{I18n, use_i18n};
use crate::router::Route;
use crate::ui_observability::{UiLoadTrace, UiPage, use_traced_resource, use_ui_load_trace};
use crate::utils::event_type_label_key;

const RESULTS_PER_PAGE: usize = 25;
/// Card (grid) view shows fewer results per page — each cell embeds a
/// mini-pedigree, so a full list-sized page would overload the layout.
const GRID_RESULTS_PER_PAGE: usize = 20;
// ── Enums ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
enum SortOrder {
    /// Server-side ranking: a name starting with what was typed comes first.
    Relevance,
    NameAZ,
    NameZA,
    BirthAsc,
    BirthDesc,
}

impl SortOrder {
    /// Every order, with the label key the sort menu shows it under.
    const ALL: [(SortOrder, &'static str); 5] = [
        (SortOrder::Relevance, "search.sort_relevance"),
        (SortOrder::NameAZ, "search.sort_name_az"),
        (SortOrder::NameZA, "search.sort_name_za"),
        (SortOrder::BirthAsc, "search.sort_birth_asc"),
        (SortOrder::BirthDesc, "search.sort_birth_desc"),
    ];

    /// The order a menu value names, by name A–Z when it names none.
    fn parse(value: &str) -> Self {
        Self::ALL
            .iter()
            .map(|(order, _)| *order)
            .find(|order| format!("{order:?}") == value)
            .unwrap_or(SortOrder::NameAZ)
    }

    fn server_sort(self) -> PersonSearchSort {
        match self {
            SortOrder::Relevance => PersonSearchSort::Relevance,
            SortOrder::NameAZ => PersonSearchSort::NameAsc,
            SortOrder::NameZA => PersonSearchSort::NameDesc,
            SortOrder::BirthAsc => PersonSearchSort::BirthAsc,
            SortOrder::BirthDesc => PersonSearchSort::BirthDesc,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum GenderFilter {
    All,
    Male,
    Female,
    Unknown,
}

impl GenderFilter {
    /// Every choice, with the label key its button shows.
    const ALL: [(GenderFilter, &'static str); 4] = [
        (GenderFilter::All, "search.all"),
        (GenderFilter::Male, "search.male"),
        (GenderFilter::Female, "search.female"),
        (GenderFilter::Unknown, "search.unknown"),
    ];

    fn sex(self) -> Option<Sex> {
        match self {
            GenderFilter::All => None,
            GenderFilter::Male => Some(Sex::Male),
            GenderFilter::Female => Some(Sex::Female),
            GenderFilter::Unknown => Some(Sex::Unknown),
        }
    }

    fn label_key(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(filter, _)| *filter == self)
            .map_or("search.all", |(_, key)| key)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ViewMode {
    List,
    Card,
}

impl ViewMode {
    fn per_page(self) -> usize {
        match self {
            ViewMode::List => RESULTS_PER_PAGE,
            ViewMode::Card => GRID_RESULTS_PER_PAGE,
        }
    }
}

fn non_empty(value: String) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn parse_filter_year(value: &str) -> Option<i32> {
    value.trim().parse().ok()
}

/// The event types the event filter offers, by menu value.
const FILTER_EVENT_TYPES: [(&str, EventType); 8] = [
    ("birth", EventType::Birth),
    ("death", EventType::Death),
    ("baptism", EventType::Baptism),
    ("burial", EventType::Burial),
    ("marriage", EventType::Marriage),
    ("residence", EventType::Residence),
    ("occupation", EventType::Occupation),
    ("census", EventType::Census),
];

fn parse_event_type(value: &str) -> Option<EventType> {
    FILTER_EVENT_TYPES
        .iter()
        .find(|(name, _)| *name == value)
        .map(|(_, event_type)| *event_type)
}

fn has_search_criteria(search: &PersonSearchParams) -> bool {
    let texts = [
        &search.surname,
        &search.given_names,
        &search.occupation,
        &search.spouse_surname,
        &search.spouse_given_names,
        &search.father_surname,
        &search.father_given_names,
        &search.mother_surname,
        &search.mother_given_names,
        &search.place,
    ];
    let years = [
        search.birth_from,
        search.birth_to,
        search.death_from,
        search.death_to,
        search.event_from,
        search.event_to,
    ];
    !search.query.trim().is_empty()
        || search.sex.is_some()
        || search.event_type.is_some()
        || search.has_media
        || texts.iter().any(|text| text.is_some())
        || years.iter().any(Option::is_some)
}

/// How removing a filter's chip clears it.
type ClearFilter = fn(SearchFilters);

/// Whether any of `fields` holds more than blanks.
fn any_filled(fields: &[Signal<String>]) -> bool {
    fields.iter().any(|field| !field.read().trim().is_empty())
}

/// Every field of the search: the names typed in the topbar, the ones
/// searched for, the filters, the order and the view.
#[derive(Clone, Copy)]
struct SearchFilters {
    last: Signal<String>,
    first: Signal<String>,
    committed_last: Signal<String>,
    committed_first: Signal<String>,
    gender: Signal<GenderFilter>,
    sort: Signal<SortOrder>,
    view: Signal<ViewMode>,
    page: Signal<usize>,
    born_from: Signal<String>,
    born_to: Signal<String>,
    died_from: Signal<String>,
    died_to: Signal<String>,
    occupation: Signal<String>,
    spouse_surname: Signal<String>,
    spouse_given_names: Signal<String>,
    father_surname: Signal<String>,
    father_given_names: Signal<String>,
    mother_surname: Signal<String>,
    mother_given_names: Signal<String>,
    place: Signal<String>,
    event_type: Signal<Option<EventType>>,
    event_from: Signal<String>,
    event_to: Signal<String>,
    has_media: Signal<bool>,
}

fn use_search_filters(last: &str, first: &str) -> SearchFilters {
    SearchFilters {
        last: use_signal(|| last.to_string()),
        first: use_signal(|| first.to_string()),
        committed_last: use_signal(|| last.to_string()),
        committed_first: use_signal(|| first.to_string()),
        gender: use_signal(|| GenderFilter::All),
        sort: use_signal(|| SortOrder::NameAZ),
        view: use_signal(|| ViewMode::List),
        page: use_signal(|| 1_usize),
        born_from: use_signal(String::new),
        born_to: use_signal(String::new),
        died_from: use_signal(String::new),
        died_to: use_signal(String::new),
        occupation: use_signal(String::new),
        spouse_surname: use_signal(String::new),
        spouse_given_names: use_signal(String::new),
        father_surname: use_signal(String::new),
        father_given_names: use_signal(String::new),
        mother_surname: use_signal(String::new),
        mother_given_names: use_signal(String::new),
        place: use_signal(String::new),
        event_type: use_signal(|| None::<EventType>),
        event_from: use_signal(String::new),
        event_to: use_signal(String::new),
        has_media: use_signal(|| false),
    }
}

impl SearchFilters {
    /// The free-text filters, which re-run the search on every keystroke.
    fn typed(&self) -> [Signal<String>; 14] {
        [
            self.born_from,
            self.born_to,
            self.died_from,
            self.died_to,
            self.occupation,
            self.spouse_surname,
            self.spouse_given_names,
            self.father_surname,
            self.father_given_names,
            self.mother_surname,
            self.mother_given_names,
            self.place,
            self.event_from,
            self.event_to,
        ]
    }

    /// The server search these fields ask for.
    fn params(&self) -> PersonSearchParams {
        let per_page = (self.view)().per_page();
        let page = (self.page)();
        PersonSearchParams {
            query: String::new(),
            limit: per_page as u32,
            offset: ((page.saturating_sub(1)) * per_page) as u32,
            sex: (self.gender)().sex(),
            surname: non_empty((self.committed_last)()),
            given_names: non_empty((self.committed_first)()),
            occupation: non_empty((self.occupation)()),
            spouse_surname: non_empty((self.spouse_surname)()),
            spouse_given_names: non_empty((self.spouse_given_names)()),
            father_surname: non_empty((self.father_surname)()),
            father_given_names: non_empty((self.father_given_names)()),
            mother_surname: non_empty((self.mother_surname)()),
            mother_given_names: non_empty((self.mother_given_names)()),
            birth_from: parse_filter_year(&(self.born_from)()),
            birth_to: parse_filter_year(&(self.born_to)()),
            death_from: parse_filter_year(&(self.died_from)()),
            death_to: parse_filter_year(&(self.died_to)()),
            place: non_empty((self.place)()),
            event_type: (self.event_type)(),
            event_from: parse_filter_year(&(self.event_from)()),
            event_to: parse_filter_year(&(self.event_to)()),
            has_media: (self.has_media)(),
            sort: (self.sort)().server_sort(),
        }
    }

    /// Back to the first page, after the search changed.
    fn restart(mut self) {
        self.page.set(1);
    }

    /// Searches for the names given, typed or not.
    fn search_names(mut self, last: String, first: String) {
        self.committed_last.set(last);
        self.committed_first.set(first);
        self.restart();
    }

    /// Empties every text field in `fields`, then searches again.
    fn clear_fields(self, fields: &[Signal<String>]) {
        for mut field in fields.iter().copied() {
            field.set(String::new());
        }
        self.restart();
    }

    fn clear_events(mut self) {
        self.event_type.set(None);
        self.clear_fields(&[self.event_from, self.event_to]);
    }

    fn clear_gender(mut self) {
        self.gender.set(GenderFilter::All);
        self.restart();
    }

    fn clear_media(mut self) {
        self.has_media.set(false);
        self.restart();
    }

    /// Clears the names and every filter.
    fn clear_all(mut self) {
        self.last.set(String::new());
        self.first.set(String::new());
        self.gender.set(GenderFilter::All);
        self.event_type.set(None);
        self.has_media.set(false);
        let mut fields = self.typed().to_vec();
        fields.extend([self.committed_last, self.committed_first]);
        self.clear_fields(&fields);
    }

    /// The filters set, each with its chip's label and what removing it
    /// clears.
    fn active_chips(&self, i18n: &I18n) -> Vec<(String, ClearFilter)> {
        let f = *self;
        let labelled = |key: &str, value: Signal<String>| format!("{}: {}", i18n.t(key), value());
        let chips: [(bool, String, ClearFilter); 10] = [
            (
                (f.gender)() != GenderFilter::All,
                i18n.t((f.gender)().label_key()),
                SearchFilters::clear_gender,
            ),
            (
                any_filled(&[f.occupation]),
                labelled("search.occupation", f.occupation),
                |f| f.clear_fields(&[f.occupation]),
            ),
            (
                any_filled(&[f.born_from, f.born_to]),
                i18n.t("search.born_between"),
                |f| f.clear_fields(&[f.born_from, f.born_to]),
            ),
            (
                any_filled(&[f.died_from, f.died_to]),
                i18n.t("search.died_between"),
                |f| f.clear_fields(&[f.died_from, f.died_to]),
            ),
            (
                any_filled(&[f.place]),
                labelled("search.place", f.place),
                |f| f.clear_fields(&[f.place]),
            ),
            (
                (f.event_type)().is_some() || any_filled(&[f.event_from, f.event_to]),
                i18n.t("search.event_criteria"),
                SearchFilters::clear_events,
            ),
            (
                any_filled(&[f.spouse_surname, f.spouse_given_names]),
                i18n.t("search.spouse"),
                |f| f.clear_fields(&[f.spouse_surname, f.spouse_given_names]),
            ),
            (
                any_filled(&[f.father_surname, f.father_given_names]),
                i18n.t("search.father"),
                |f| f.clear_fields(&[f.father_surname, f.father_given_names]),
            ),
            (
                any_filled(&[f.mother_surname, f.mother_given_names]),
                i18n.t("search.mother"),
                |f| f.clear_fields(&[f.mother_surname, f.mother_given_names]),
            ),
            (
                (f.has_media)(),
                i18n.t("search.has_media"),
                SearchFilters::clear_media,
            ),
        ];
        chips
            .into_iter()
            .filter(|(active, ..)| *active)
            .map(|(_, label, clear)| (label, clear))
            .collect()
    }
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
    let load_trace = use_ui_load_trace(UiPage::SearchResults);

    let tree_id = Uuid::parse_str(&props.tree_id).ok();
    let tree_cache = use_tree_cache();
    let api_tree = api.clone();
    let tree_resource = use_traced_resource(load_trace.clone(), "tree", move || {
        let api = api_tree.clone();
        let _generation = tree_cache.generation();
        async move {
            let tree_id = tree_id?;
            Some(fetch_tree_cached(&api, &tree_cache, tree_id).await)
        }
    });
    let tree = match &*tree_resource.read() {
        Some(Some(Ok(tree))) => Some(tree.clone()),
        _ => tree_id.and_then(|tree_id| tree_cache.tree(tree_id)),
    };
    let tree_name = tree
        .as_ref()
        .map(|tree| tree.name.clone())
        .unwrap_or_default();
    // The person last shown in this tree, else its SOSA root.
    let current_person = use_current_person();
    let selected_person_id = tree_id
        .and_then(|tid| current_person.get(tid))
        .or(tree.and_then(|tree| tree.sosa_root_person_id));

    // ── Search state ──
    let filters = use_search_filters(&props.last, &props.first);
    let mut show_filters = use_signal(|| false);
    let sections_open = [
        use_signal(|| true),
        use_signal(|| true),
        use_signal(|| true),
    ];

    // Navigation can change the query parameters while this component stays
    // mounted, so the URL has to be able to overwrite the fields.
    //
    // Comparing against the last value seen is what makes that dependency
    // explicit. An effect cannot express it: `use_effect` re-runs on the
    // signals its body *reads*, and this body only writes, so it would keep
    // re-applying the props it was first built with and reset the page number
    // on every unrelated re-render.
    let incoming_query = (props.last.clone(), props.first.clone());
    let mut last_synced_query = use_signal(|| incoming_query.clone());
    if last_synced_query() != incoming_query {
        last_synced_query.set(incoming_query.clone());
        let (mut last, mut first) = (filters.last, filters.first);
        last.set(incoming_query.0.clone());
        first.set(incoming_query.1.clone());
        filters.search_names(incoming_query.0, incoming_query.1);
    }

    // ── Server-side search ──
    //
    // The free-text filters re-run the search on every keystroke, so those
    // wait a moment for the typing to stop. Nothing else does: opening the
    // page, turning a page, sorting or ticking a box searches at once — a
    // blanket delay put 200 ms in front of every one of them.
    let typed_filters = use_hook(|| std::rc::Rc::new(std::cell::RefCell::new(None)));
    let api_search = api.clone();
    let search_resource = use_traced_resource(load_trace.clone(), "person_search", move || {
        let api = api_search.clone();
        let typed = filters.typed().map(|field| field());
        let still_typing = typed_filters
            .replace(Some(typed.clone()))
            .is_some_and(|previous| previous != typed);
        let params = filters.params();
        async move {
            if !has_search_criteria(&params) {
                return Ok(None);
            }
            let Some(tid) = tree_id else {
                return Err(crate::api::ApiError::invalid_tree_id(&i18n));
            };
            if still_typing {
                crate::utils::sleep_ms(200).await;
            }
            api.search_persons_filtered(tid, &params).await.map(Some)
        }
    });

    // Submitting from the topbar searches in place rather than navigating:
    // this *is* the results page. `TopbarSearch` owns the SOSA shortcut, so
    // reaching a person by number still jumps straight there.
    let commit_search = use_callback(move |(last, first): (String, String)| {
        if !(last.trim().is_empty() && first.trim().is_empty()) {
            filters.search_names(last, first);
        }
    });

    // ── Server-filtered, sorted, and paginated result ──
    //
    // Memoised because the page re-renders on every keystroke in any of the
    // filter fields, and none of those change the results already on screen.
    let all_entries = use_memo(move || match &*search_resource.read() {
        Some(Ok(Some(sr))) => sr.entries.clone(),
        _ => Vec::<SearchEntry>::new(),
    });
    let api_portraits = api.clone();
    let portraits_resource = use_traced_resource(load_trace.clone(), "portraits", move || {
        let api = api_portraits.clone();
        let person_ids = result_ids(&search_resource);
        async move {
            match tree_id {
                Some(tree_id) => api.portrait_map_for_ids(tree_id, &person_ids).await,
                None => Default::default(),
            }
        }
    });
    let (card_pedigrees, pedigrees_loaded) =
        use_card_pedigrees(load_trace, &api, tree_id, search_resource, filters.view);

    let per_page = (filters.view)().per_page();
    let total_filtered = match &*search_resource.read() {
        Some(Ok(Some(result))) => result.total_count,
        _ => 0,
    };
    let page = (filters.page)();
    let total_pages = total_filtered.div_ceil(per_page).max(1);
    let page_results = all_entries.read();
    let portraits = portraits_resource.read();
    let body = ResultsBody::of(
        &search_resource.read(),
        page_results.is_empty(),
        (filters.view)(),
    );
    let results = ResultsView {
        tree_id: &props.tree_id,
        origin: &props.origin,
        entries: &page_results,
        i18n,
    };

    // ── Render ──
    rsx! {
        div { class: "sub-page search-results-page",
            // ── Topbar (shared td-topbar / td-bc classes per spec §3) ──
            div { class: "td-topbar",
                TreeBreadcrumb {
                    tree_id: props.tree_id.clone(),
                    tree_name: tree_name.clone(),
                    span { class: "td-bc-current", {i18n.t("search.title")} }
                }
                TopbarSearch {
                    tree_id: props.tree_id.clone(),
                    from_person: props.origin == "person",
                    last: filters.last,
                    first: filters.first,
                    on_submit: move |query| commit_search.call(query),
                }
                PrintHeading {
                    tree_name: tree_name.clone(),
                    title: search_print_title(&i18n, &(filters.committed_last)(), &(filters.committed_first)()),
                }
            }

            div { class: "pd-page-shell",
                ToolPageSidebar {
                    tree_id: props.tree_id.clone(),
                    selected_person_id,
                }

                // ── Scrollable content ──
                div { class: "sub-page-content",

                // ── Filter panel ──
                div { class: "sr-filters-toggle",
                    button {
                        class: "btn btn-outline btn-sm",
                        onclick: move |_| show_filters.toggle(),
                        span { class: if show_filters() { "sr-chevron open" } else { "sr-chevron" }, "\u{25BC}" }
                        " {i18n.t(\"search.filters\")}"
                    }
                }
                if show_filters() {
                    // A tree id that does not parse names no tree, and the
                    // nil id's suggestions are empty.
                    {filter_panel(&i18n, tree_id.unwrap_or_default(), filters, sections_open)}
                }

                div { class: "sr-active-filters",
                    for (label, clear) in filters.active_chips(&i18n) {
                        button {
                            class: "sr-filter-chip",
                            title: "{i18n.t(\"search.clear_filters\")}",
                            onclick: move |_| clear(filters),
                            "{label}"
                            span { " \u{00D7}" }
                        }
                    }
                }

                {toolbar(&i18n, total_filtered, filters)}

                // ── Results ──
                match body {
                    ResultsBody::Message(key) => rsx! {
                        div { class: "sr-empty", p { {i18n.t(key)} } }
                    },
                    ResultsBody::Cards => results.cards(tree_id.unwrap_or_default(), &card_pedigrees.read(), pedigrees_loaded),
                    ResultsBody::List => results.list(portraits.as_ref()),
                }

                {pagination(page, total_pages, filters.page)}
                PrintPageNote { page, pages: total_pages }
                }
            }
        }
    }
}

/// The people a search found.
fn result_ids(search: &Resource<Result<Option<SearchResult>, ApiError>>) -> Vec<Uuid> {
    search
        .read()
        .as_ref()
        .and_then(|result| result.as_ref().ok())
        .and_then(|result| result.as_ref())
        .map(|result| result.entries.iter().map(|entry| entry.person_id).collect())
        .unwrap_or_default()
}

/// The card view's small pedigrees, one `SharedPedigree` per result, and
/// whether they have been fetched.
///
/// Asked for one at a time, that was a request and a traced resource per
/// card; the page asks for the whole set at once, and only while the card
/// view is showing.
fn use_card_pedigrees(
    load_trace: UiLoadTrace,
    api: &ApiClient,
    tree_id: Option<Uuid>,
    search: Resource<Result<Option<SearchResult>, ApiError>>,
    view: Signal<ViewMode>,
) -> (Memo<HashMap<Uuid, SharedPedigree>>, bool) {
    let api = api.clone();
    let pedigrees = use_traced_resource(load_trace, "result_pedigrees", move || {
        let api = api.clone();
        // Switching to the card view re-runs the search at the card view's
        // page size; the rows still on screen belong to the list. Asking
        // for their pedigrees fetched a batch nobody would see.
        let searching = *search.state().read() == UseResourceState::Pending;
        let wanted = view() == ViewMode::Card && !searching;
        let person_ids = result_ids(&search);
        async move {
            match tree_id.filter(|_| wanted && !person_ids.is_empty()) {
                Some(tree_id) => api.get_pedigrees(tree_id, &person_ids, 2, 0).await,
                None => Default::default(),
            }
        }
    });
    // Assembled once per batch rather than once per render of the grid.
    let shared = use_memo(move || {
        let Some(pedigrees) = &*pedigrees.read() else {
            return HashMap::new();
        };
        crate::ui_observability::measure_ui("pedigree_data", || {
            pedigrees
                .iter()
                .map(|(root, pedigree)| {
                    (
                        *root,
                        SharedPedigree::new(PedigreeData::from_pedigree(pedigree)),
                    )
                })
                .collect::<HashMap<_, _>>()
        })
    });
    (shared, pedigrees.read().is_some())
}

/// What the results area shows.
enum ResultsBody {
    /// A message instead of results, by its i18n key.
    Message(&'static str),
    Cards,
    List,
}

impl ResultsBody {
    fn of(
        search: &Option<Result<Option<SearchResult>, ApiError>>,
        empty: bool,
        view: ViewMode,
    ) -> Self {
        match (search, empty, view) {
            (Some(Ok(None)), ..) => Self::Message("search.start_search"),
            (None, ..) => Self::Message("search.loading"),
            (Some(Err(_)), ..) => Self::Message("search.error"),
            (_, true, _) => Self::Message("search.no_results"),
            (_, _, ViewMode::Card) => Self::Cards,
            (_, _, ViewMode::List) => Self::List,
        }
    }
}

/// One page of results.
struct ResultsView<'a> {
    tree_id: &'a str,
    origin: &'a str,
    entries: &'a [SearchEntry],
    i18n: I18n,
}

impl ResultsView<'_> {
    fn cards(
        &self,
        tree_id: Uuid,
        pedigrees: &HashMap<Uuid, SharedPedigree>,
        loaded: bool,
    ) -> Element {
        rsx! {
            div { class: "sr-grid",
                for entry in self.entries.iter() {
                    SearchPedigreeCard {
                        key: "{entry.person_id}",
                        tree_id,
                        tree_id_str: self.tree_id.to_string(),
                        person_id: entry.person_id,
                        given_names: entry.given_names.clone(),
                        surname: entry.surname.clone(),
                        sex: entry.sex,
                        birth_year: entry.birth_year.clone(),
                        death_year: entry.death_year.clone(),
                        origin: self.origin.to_string(),
                        pedigree: pedigrees.get(&entry.person_id).cloned(),
                        loaded,
                    }
                }
            }
        }
    }

    fn list(&self, portraits: Option<&HashMap<Uuid, CroppedSource>>) -> Element {
        rsx! {
            div {
                class: "search-person-results sr-results-page",
                for entry in self.entries.iter() {
                    {render_result_item(
                        entry,
                        self.tree_id,
                        self.origin,
                        portraits.and_then(|map| map.get(&entry.person_id)),
                        &self.i18n,
                    )}
                }
            }
        }
    }
}

/// The filters, in their three foldable sections.
fn filter_panel(
    i18n: &I18n,
    suggest_tree_id: Uuid,
    filters: SearchFilters,
    [person_open, event_open, relation_open]: [Signal<bool>; 3],
) -> Element {
    rsx! {
        div { class: "sr-filters pf-embedded",
            FormSection {
                title: i18n.t("search.person_criteria"),
                open: person_open,
                {person_criteria(i18n, suggest_tree_id, filters)}
            }
            FormSection {
                title: i18n.t("search.event_criteria"),
                open: event_open,
                {event_criteria(i18n, filters)}
            }
            FormSection {
                title: i18n.t("search.relation_criteria"),
                open: relation_open,
                div { class: "sr-relations-grid",
                    {relation_group(i18n, suggest_tree_id, "search.spouse", filters.spouse_surname, filters.spouse_given_names, filters)}
                    {relation_group(i18n, suggest_tree_id, "search.father", filters.father_surname, filters.father_given_names, filters)}
                    {relation_group(i18n, suggest_tree_id, "search.mother", filters.mother_surname, filters.mother_given_names, filters)}
                }
            }
            div { class: "sr-filter-actions",
                button {
                    class: "pf-row-btn",
                    onclick: move |_| filters.clear_all(),
                    {i18n.t("search.clear_filters")}
                }
            }
        }
    }
}

fn person_criteria(i18n: &I18n, suggest_tree_id: Uuid, filters: SearchFilters) -> Element {
    let SearchFilters {
        last,
        first,
        mut gender,
        occupation,
        mut has_media,
        ..
    } = filters;
    rsx! {
        div { class: "sr-filter-grid sr-filter-grid-person",
            div { class: "sr-filter-group",
                label { {i18n.t("search.surname")} }
                ValueInput {
                    value: last,
                    tree_id: suggest_tree_id,
                    field: SuggestionField::FamilyNames,
                    tree_only: true,
                    on_change: move |()| filters.search_names(last(), (filters.committed_first)()),
                }
            }
            div { class: "sr-filter-group",
                label { {i18n.t("search.given_names")} }
                ValueInput {
                    value: first,
                    tree_id: suggest_tree_id,
                    field: SuggestionField::GivenNames,
                    tree_only: true,
                    on_change: move |()| filters.search_names((filters.committed_last)(), first()),
                }
            }
            div { class: "sr-filter-group sr-filter-sex",
                label { {i18n.t("search.gender")} }
                div { class: "pf-gender-group",
                    for (choice, key) in GenderFilter::ALL {
                        button {
                            r#type: "button",
                            class: if gender() == choice { "pf-gender-btn active" } else { "pf-gender-btn" },
                            onclick: move |_| {
                                gender.set(choice);
                                filters.restart();
                            },
                            {i18n.t(key)}
                        }
                    }
                }
            }
            div { class: "sr-filter-group",
                label { {i18n.t("search.occupation")} }
                ValueInput {
                    value: occupation,
                    tree_id: suggest_tree_id,
                    field: SuggestionField::Occupations,
                    tree_only: true,
                    on_change: move |()| filters.restart(),
                }
            }
            {year_range(i18n, "search.born_between", filters.born_from, filters.born_to, filters)}
            {year_range(i18n, "search.died_between", filters.died_from, filters.died_to, filters)}
            label { class: "sr-media-filter",
                input {
                    r#type: "checkbox",
                    checked: has_media(),
                    onchange: move |e: Event<FormData>| {
                        has_media.set(e.checked());
                        filters.restart();
                    },
                }
                span { {i18n.t("search.has_media")} }
            }
        }
    }
}

fn event_criteria(i18n: &I18n, filters: SearchFilters) -> Element {
    let SearchFilters {
        mut event_type,
        mut place,
        ..
    } = filters;
    rsx! {
        div { class: "sr-filter-grid sr-filter-grid-event",
            div { class: "sr-filter-group",
                label { {i18n.t("search.event_type")} }
                select {
                    value: event_type().map(|event| event.to_string()).unwrap_or_default(),
                    onchange: move |e: Event<FormData>| {
                        event_type.set(parse_event_type(&e.value()));
                        filters.restart();
                    },
                    option { value: "", {i18n.t("search.all_events")} }
                    for (value, kind) in FILTER_EVENT_TYPES {
                        option { value, {i18n.t(event_type_label_key(kind))} }
                    }
                }
            }
            div { class: "sr-filter-group",
                label { {i18n.t("search.place")} }
                input {
                    r#type: "text",
                    value: "{place}",
                    oninput: move |e: Event<FormData>| {
                        place.set(e.value());
                        filters.restart();
                    },
                }
            }
            {year_range(i18n, "search.event_between", filters.event_from, filters.event_to, filters)}
        }
    }
}

/// A "between two years" filter.
fn year_range(
    i18n: &I18n,
    label_key: &str,
    mut from: Signal<String>,
    mut to: Signal<String>,
    filters: SearchFilters,
) -> Element {
    rsx! {
        div { class: "sr-filter-group",
            label { {i18n.t(label_key)} }
            div { class: "sr-date-range",
                input {
                    r#type: "number",
                    placeholder: "1800",
                    value: "{from}",
                    oninput: move |e: Event<FormData>| {
                        from.set(e.value());
                        filters.restart();
                    },
                }
                span { "\u{2013}" }
                input {
                    r#type: "number",
                    placeholder: "2000",
                    value: "{to}",
                    oninput: move |e: Event<FormData>| {
                        to.set(e.value());
                        filters.restart();
                    },
                }
            }
        }
    }
}

/// The names of a relative — spouse, father or mother — to search by.
fn relation_group(
    i18n: &I18n,
    suggest_tree_id: Uuid,
    label_key: &str,
    surname: Signal<String>,
    given_names: Signal<String>,
    filters: SearchFilters,
) -> Element {
    rsx! {
        div { class: "sr-relation-group pf-subform",
            div { class: "pf-block-label", {i18n.t(label_key)} }
            ValueInput {
                value: surname,
                tree_id: suggest_tree_id,
                field: SuggestionField::FamilyNames,
                placeholder: i18n.t("search.surname"),
                tree_only: true,
                on_change: move |()| filters.restart(),
            }
            ValueInput {
                value: given_names,
                tree_id: suggest_tree_id,
                field: SuggestionField::GivenNames,
                placeholder: i18n.t("search.given_names"),
                tree_only: true,
                on_change: move |()| filters.restart(),
            }
        }
    }
}

/// The result count, the sort menu and the view switch.
fn toolbar(i18n: &I18n, total: usize, filters: SearchFilters) -> Element {
    let SearchFilters { mut sort, view, .. } = filters;
    let view_button = |mode: ViewMode, title_key: &str, glyph: &str| {
        let mut view = view;
        rsx! {
            button {
                class: if view() == mode { "sr-view-btn active" } else { "sr-view-btn" },
                title: i18n.t(title_key),
                onclick: move |_| {
                    if view() != mode {
                        view.set(mode);
                        filters.restart();
                    }
                },
                "{glyph}"
            }
        }
    };
    rsx! {
        div { class: "sr-toolbar",
            span { class: "sr-count",
                {format!("{} {}", total, i18n.t("search.results"))}
            }
            div { class: "sr-sort",
                select {
                    value: "{sort():?}",
                    onchange: move |e: Event<FormData>| sort.set(SortOrder::parse(&e.value())),
                    for (order, key) in SortOrder::ALL {
                        option { value: "{order:?}", {i18n.t(key)} }
                    }
                }
            }
            div { class: "sr-view-modes",
                {view_button(ViewMode::List, "search.view_list", "\u{2630}")}
                {view_button(ViewMode::Card, "search.view_grid", "\u{25A6}")}
            }
        }
    }
}

/// The page buttons, when the results span more than one page.
fn pagination(page: usize, total_pages: usize, mut current_page: Signal<usize>) -> Element {
    if total_pages < 2 {
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

// ── Result item rendering ────────────────────────────────────────────────
//
// Reuses the same `search-person-result` / `sp-*` CSS classes as the
// SearchPerson typeahead component (used in SOSA root selector, etc.)
// so that person rows look identical everywhere.

/// One result row.
///
/// The row body is [`render_person_search_summary`], the same one the typeahead
/// picker draws, so the two cannot drift; only the wrapper differs — a `Link`
/// here, a `button` there.
fn render_result_item(
    entry: &SearchEntry,
    tree_id: &str,
    origin: &str,
    portrait: Option<&CroppedSource>,
    i18n: &I18n,
) -> Element {
    let sex_class = match entry.sex {
        Sex::Male => "male",
        Sex::Female => "female",
        Sex::Unknown => "",
    };

    let summary = PersonSearchSummary::from(entry);
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
            {render_person_search_summary(&summary, portrait.cloned(), i18n)}
        }
    }
}

// ── Grid (Card) view ─────────────────────────────────────────────────────

/// Maximum mini-pedigree scale inside a grid cell. The shared component
/// reduces it further when needed to keep all three generations visible.
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
    /// This card's fragment, assembled by the page for the whole result set.
    /// `None` while the batch is still in flight.
    pedigree: Option<SharedPedigree>,
    /// Whether the batch has answered, so a card with no fragment can tell
    /// "still loading" from "this person has no pedigree".
    loaded: bool,
) -> Element {
    let i18n = use_i18n();
    let nav = navigator();

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
    let body = match (pedigree, loaded) {
        (Some(data), _) => rsx! {
            crate::components::pedigree_chart::MiniPedigree {
                root_person_id: person_id,
                data: data,
                ancestor_levels: 2,
                descendant_levels: 0,
                on_person_navigate: on_navigate,
                scale: GRID_PEDIGREE_SCALE,
            }
        },
        (None, true) => rsx! {
            div { class: "sr-grid-ped-msg", {i18n.t("search.error")} }
        },
        (None, false) => rsx! {
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
