//! Dictionary page: index of family names, sources, places, and
//! occupations across a tree, each paired with a usage count, plus the
//! family-name editor (rename, merge, particle re-cut). See
//! `docs/ui-dictionary.md`.

use std::collections::HashSet;

use dioxus::prelude::*;
use oxidgene_core::types::{split_surname_at_head, split_surname_particle};
use uuid::Uuid;

use crate::api::SuggestionField;
use crate::api::{
    ApiClient, ApiError, DictionaryEntry, PersonUsageEntry, PlaceDictionaryEntry,
    SourceDictionaryEntry, SourceGroupEntry,
};
use crate::components::empty_state::EmptyState;
use crate::components::modal::Modal;
use crate::components::pager::Pager;
use crate::components::pedigree_chart::format_lifespan;
use crate::components::print::PrintPageNote;
use crate::components::source_forms::SourceEditor;
use crate::components::suggest_input::ValueInput;
use crate::components::tabs::Tabs;
use crate::components::tree_page::{ToolPageFrame, use_tree_page};
use crate::i18n::{I18n, use_i18n};
use crate::nav_history::{use_restored_view, use_saved_view};
use crate::pages::dictionary_media::DictionaryMedia;
use crate::pages::dictionary_repositories::DictionaryRepositories;
use crate::prefs::{SortParticles, use_sort_particles};
use crate::router::Route;
use crate::ui_observability::{UiPage, use_traced_resource, use_ui_load_trace};

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
    Repositories,
    Places,
    Occupations,
    Media,
}

impl DictTab {
    /// The tabs in their order, each with its label's key.
    const ALL: [(Self, &'static str); 6] = [
        (Self::FamilyNames, "dictionary.tab.family_names"),
        (Self::Sources, "dictionary.tab.sources"),
        (Self::Repositories, "dictionary.tab.repositories"),
        (Self::Places, "dictionary.tab.places"),
        (Self::Occupations, "dictionary.tab.occupations"),
        (Self::Media, "dictionary.tab.media"),
    ];
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

/// The family name the editor is open on (see [`FamilyNameEditor`]).
///
/// `value` is the surname as listed, particle included — the only stable
/// identity of a dictionary entry, and what the API matches rows on.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FamilyNameEdit {
    value: String,
    /// Persons carrying the name on any of their names, so the dialog can say
    /// what a re-cut is about to touch before it touches it.
    count: i64,
    /// Those carrying it as their primary name: the ones a rename reaches.
    primary_count: i64,
    /// Where the name is cut today; empty when it has no particle.
    current_particle: String,
}

impl FamilyNameEdit {
    fn new(entry: &DictionaryEntry) -> Self {
        Self {
            value: entry.value.clone(),
            count: entry.count,
            primary_count: entry.primary_count.unwrap_or(entry.count),
            current_particle: entry_particle_split(entry)
                .map(|(particle, _)| particle)
                .unwrap_or_default(),
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

/// The sources level under `query_prefix`: its groups, or its sources when
/// it has no group left. The backend resolves the drill-down itself,
/// auto-skipping any forced single-choice levels — the prefix returned may be
/// longer than `query_prefix`. See ui-dictionary.md §8.10.
async fn load_sources_view(
    api: &ApiClient,
    tid: Uuid,
    query_prefix: &str,
) -> Result<SourcesView, ApiError> {
    let resolved = api.dictionary_source_groups(tid, query_prefix).await?;
    if !resolved.groups.is_empty() {
        return Ok(SourcesView::Groups {
            prefix: resolved.prefix,
            total: resolved.total,
            groups: resolved.groups,
        });
    }
    // The last level comes with its sources.
    let sources = resolved.sources.unwrap_or_default();
    Ok(SourcesView::List {
        prefix: resolved.prefix,
        sources,
    })
}

/// The persons behind a dictionary row; none when they cannot be read.
async fn load_usage(api: &ApiClient, tid: Uuid, key: &UsageKey) -> Vec<PersonUsageEntry> {
    match key {
        UsageKey::FamilyName(value) => api.dictionary_family_name_usage(tid, value).await,
        UsageKey::Source(id) => api.dictionary_source_usage(tid, *id).await,
        UsageKey::Place(id) => api.dictionary_place_usage(tid, *id).await,
        UsageKey::Occupation(value) => api.dictionary_occupation_usage(tid, value).await,
    }
    .unwrap_or_default()
}

/// The value tabs opened so far: each one's data is asked for the first
/// time it shows, then kept.
#[derive(Clone, Copy)]
struct OpenedTabs {
    family_names: Signal<bool>,
    sources: Signal<bool>,
    places: Signal<bool>,
    occupations: Signal<bool>,
}

impl OpenedTabs {
    fn family_names(&self) -> bool {
        (self.family_names)()
    }
    fn sources(&self) -> bool {
        (self.sources)()
    }
    fn places(&self) -> bool {
        (self.places)()
    }
    fn occupations(&self) -> bool {
        (self.occupations)()
    }

    /// Marks `tab` opened, once: reopening it changes nothing, so its data
    /// is not asked again.
    fn open(&self, tab: DictTab) {
        let mut opened = match tab {
            DictTab::FamilyNames => self.family_names,
            DictTab::Sources => self.sources,
            DictTab::Places => self.places,
            DictTab::Occupations => self.occupations,
            DictTab::Repositories | DictTab::Media => return,
        };
        if !*opened.peek() {
            opened.set(true);
        }
    }
}

/// The tabs opened so far, the page opening on `first`.
fn use_opened_tabs(first: DictTab) -> OpenedTabs {
    OpenedTabs {
        family_names: use_signal(|| first == DictTab::FamilyNames),
        sources: use_signal(|| first == DictTab::Sources),
        places: use_signal(|| first == DictTab::Places),
        occupations: use_signal(|| first == DictTab::Occupations),
    }
}

/// What the reader was looking at, kept with the page's history entry so
/// that coming back to it — from the pedigree a usage list led to, say —
/// finds the tab, filters, page and open entry as they were left.
#[derive(Debug, Clone, PartialEq)]
struct DictionaryView {
    tab: DictTab,
    quick: String,
    letter: Option<char>,
    page_size: PageSize,
    page: usize,
    expanded: Option<UsageKey>,
    source_history: Vec<String>,
}

impl Default for DictionaryView {
    fn default() -> Self {
        Self {
            tab: DictTab::FamilyNames,
            quick: String::new(),
            letter: None,
            page_size: PageSize::Fixed(25),
            page: 1,
            expanded: None,
            source_history: Vec::new(),
        }
    }
}

/// How the history names the dictionary: the tab, then the entry whose
/// usage is open, as the loaded lists name it.
fn history_subject(
    i18n: &I18n,
    tab: DictTab,
    expanded: Option<&UsageKey>,
    places: &FiledEntries<PlaceDictionaryEntry>,
    sources: Option<&SourcesView>,
) -> String {
    let tab_label = DictTab::ALL
        .iter()
        .find(|(t, _)| *t == tab)
        .map(|(_, key)| i18n.t(key))
        .unwrap_or_default();
    let entry = match expanded {
        Some(UsageKey::FamilyName(value) | UsageKey::Occupation(value)) => Some(value.clone()),
        Some(UsageKey::Place(id)) => places
            .entries
            .iter()
            .find(|entry| entry.place.id == *id)
            .map(|entry| entry.place.name.clone()),
        Some(UsageKey::Source(id)) => match sources {
            Some(SourcesView::List { sources, .. }) => sources
                .iter()
                .find(|entry| entry.source.id == *id)
                .map(|entry| entry.source.title.clone()),
            _ => None,
        },
        None => None,
    };
    match entry {
        Some(entry) => i18n.t_args(
            "nav_history.entry",
            &[("page", tab_label.as_str()), ("subject", entry.as_str())],
        ),
        None => tab_label,
    }
}

/// A value tab's data: `None` until the tab is first opened, then the
/// aggregation's answer.
type TabResource<T> = Resource<Option<Result<T, ApiError>>>;

#[component]
pub fn Dictionary(tree_id: String) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let load_trace = use_ui_load_trace(UiPage::Dictionary);

    let mut tree_id_parsed = use_signal(|| tree_id.parse::<Uuid>().ok());
    let new_parsed = tree_id.parse::<Uuid>().ok();
    let tree_changed = new_parsed != *tree_id_parsed.peek();
    if tree_changed {
        *tree_id_parsed.write() = new_parsed;
    }

    // The view the reader left here, when coming back through the history.
    let restored = use_restored_view::<DictionaryView>().unwrap_or_default();
    let mut active_tab = use_signal(|| restored.tab);
    let quick_filter = use_signal(|| restored.quick.clone());
    let letter_filter = use_signal(|| restored.letter);
    let page_size = use_signal(|| restored.page_size);
    let mut current_page = use_signal(|| restored.page);
    let mut expanded = use_signal(|| restored.expanded.clone());
    // Sources tab drill-down history: each entry is a branch label the user
    // clicked (see ui-dictionary.md §8.10). Empty = "All sources" root.
    let mut source_history = use_signal(|| restored.source_history.clone());
    use_saved_view(move || DictionaryView {
        tab: active_tab(),
        quick: quick_filter(),
        letter: letter_filter(),
        page_size: page_size(),
        page: current_page(),
        expanded: expanded(),
        source_history: source_history(),
    });

    // Reset filters/pagination/expansion when switching tabs.
    let mut prev_tab = use_signal(|| restored.tab);
    if prev_tab() != active_tab() {
        prev_tab.set(active_tab());
        quick_filter.clone().set(String::new());
        letter_filter.clone().set(None);
        current_page.set(1);
        expanded.set(None);
        // Only when there is something to clear: setting it marks it changed,
        // which re-ran the Sources request the tab switch had just sent.
        if !source_history.peek().is_empty() {
            source_history.set(Vec::new());
        }
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
    let page = use_tree_page(&tree_id);

    let api_fn = api.clone();
    let sort_particles = use_sort_particles();
    let mut family_name_edit = use_signal(|| None::<FamilyNameEdit>);
    let mut source_edit = use_signal(|| None::<Uuid>);

    // Each tab asks for its data the first time it is opened, not when the
    // page opens: a tree's dictionary is four aggregations over all of it.
    let opened = use_opened_tabs(restored.tab);

    let mut family_names_resource =
        use_traced_resource(load_trace.clone(), "family_names", move || {
            let api = api_fn.clone();
            let tid = tree_id_parsed();
            let wanted = opened.family_names();
            async move {
                if !wanted {
                    return None;
                }
                let Some(tid) = tid else {
                    return Some(Err(ApiError::invalid_tree_id(&i18n)));
                };
                Some(api.dictionary_family_names(tid).await)
            }
        });

    let api_occ = api.clone();
    let mut occupations_resource =
        use_traced_resource(load_trace.clone(), "occupations", move || {
            let api = api_occ.clone();
            let tid = tree_id_parsed();
            let wanted = opened.occupations();
            async move {
                if !wanted {
                    return None;
                }
                let Some(tid) = tid else {
                    return Some(Err(ApiError::invalid_tree_id(&i18n)));
                };
                Some(api.dictionary_occupations(tid).await)
            }
        });

    let api_src = api.clone();
    let mut sources_view_resource = use_traced_resource(load_trace.clone(), "sources", move || {
        let api = api_src.clone();
        let tid = tree_id_parsed();
        let query_prefix = source_history().last().cloned().unwrap_or_default();
        let wanted = opened.sources();
        async move {
            if !wanted {
                return None;
            }
            let Some(tid) = tid else {
                return Some(Err(ApiError::invalid_tree_id(&i18n)));
            };
            Some(load_sources_view(&api, tid, &query_prefix).await)
        }
    });

    let api_place = api.clone();
    let mut places_resource = use_traced_resource(load_trace.clone(), "places", move || {
        let api = api_place.clone();
        let tid = tree_id_parsed();
        let wanted = opened.places();
        async move {
            if !wanted {
                return None;
            }
            let Some(tid) = tid else {
                return Some(Err(ApiError::invalid_tree_id(&i18n)));
            };
            Some(api.dictionary_places(tid).await)
        }
    });

    if tree_changed {
        family_names_resource.restart();
        occupations_resource.restart();
        source_history.set(Vec::new());
        sources_view_resource.restart();
        places_resource.restart();
    }

    // ── Usage drill-down for the currently expanded row ──
    //
    // Tagged with the `UsageKey` it was fetched for: `use_resource` keeps
    // returning the last *completed* value while a new fetch (for a
    // different key) is in flight, so an untagged `Vec<PersonUsageEntry>`
    // briefly — and, if the two fetches race, sometimes persistently —
    // rendered the previous row's people under the newly expanded row.
    // `render_usage_accordion` only trusts a value whose tag matches the
    // row it's rendering, and shows a loading state otherwise.
    let api_usage = api.clone();
    let usage_resource = use_traced_resource(load_trace.clone(), "usage", move || {
        let api = api_usage.clone();
        let key = expanded();
        let tid = tree_id_parsed();
        async move {
            let (Some(key), Some(tid)) = (key.clone(), tid) else {
                return (key, Vec::new());
            };
            let people = load_usage(&api, tid, &key).await;
            (Some(key), people)
        }
    });

    // Filed once per fetch, so a keystroke in the quick filter re-filters an
    // already-sorted list instead of re-cloning and re-sorting the dictionary.
    let filed_family_names = use_filed_values(family_names_resource, sort_particles);
    let filed_occupations = use_filed_values(occupations_resource, sort_particles);
    let filed_places = use_filed_places(places_resource);

    let ctx = TabContext {
        i18n,
        tree_id: &tree_id,
        filters: ListFilters {
            quick: quick_filter,
            letter: letter_filter,
            page_size,
            page: current_page,
        },
        expanded,
        usage_people: usage_resource,
    };

    // ── Render ──
    rsx! {
        ToolPageFrame {
            tree_id: tree_id.clone(),
            tree_name: page.name(),
            title: i18n.t("dictionary.breadcrumb"),
            subject: history_subject(
                &i18n,
                active_tab(),
                expanded().as_ref(),
                &filed_places.read(),
                sources_view_resource.read().as_ref().and_then(Option::as_ref).and_then(|view| view.as_ref().ok()),
            ),
            selected_person_id: page.selected_person_id,
            Tabs {
                tabs: DictTab::ALL.iter().map(|(tab, label)| (*tab, i18n.t(label))).collect::<Vec<_>>(),
                current: Some(active_tab()),
                on_select: move |tab| {
                    opened.open(tab);
                    active_tab.set(tab);
                },
            }

            match active_tab() {
                DictTab::FamilyNames => render_value_tab(
                    ctx,
                    ValueTab {
                        resource: family_names_resource,
                        empty_key: "dictionary.no_entries_family_names",
                        navigable: true,
                        sort_particles,
                        family_name_edit: Some(family_name_edit),
                        filed: filed_family_names,
                    },
                ),
                DictTab::Occupations => render_value_tab(
                    ctx,
                    ValueTab {
                        resource: occupations_resource,
                        empty_key: "dictionary.no_entries_occupations",
                        navigable: false,
                        sort_particles,
                        family_name_edit: None,
                        filed: filed_occupations,
                    },
                ),
                DictTab::Sources => {
                    render_sources_tab(ctx, source_history, sources_view_resource, source_edit)
                }
                // Its own module: a list of its own, edited in place.
                DictTab::Repositories => match tree_id_parsed() {
                    Some(tree_id) => rsx! { DictionaryRepositories { tree_id } },
                    None => rsx! {},
                },
                DictTab::Places => render_places_tab(ctx, places_resource, filed_places),
                // Its own module: server-paginated, with filters of its own.
                DictTab::Media => match tree_id_parsed() {
                    Some(tree_id) => rsx! { DictionaryMedia { tree_id } },
                    None => rsx! {},
                },
            }

            if let (Some(source_id), Some(tid)) = (source_edit(), tree_id_parsed()) {
                SourceEditor {
                    key: "{source_id}",
                    tree_id: tid,
                    source_id,
                    on_close: move |()| source_edit.set(None),
                    on_saved: move |()| {
                        source_edit.set(None);
                        sources_view_resource.restart();
                    },
                }
            }

            if let (Some(edit), Some(tid)) = (family_name_edit(), tree_id_parsed()) {
                FamilyNameEditor {
                    key: "{edit.value}",
                    tree_id: tid,
                    edit,
                    known: filed_family_names,
                    on_close: move |()| family_name_edit.set(None),
                    on_saved: move |()| {
                        family_name_edit.set(None);
                        family_names_resource.restart();
                    },
                }
            }
        }
    }
}

/// The particle the editor shows: the merged name's cut, else what was
/// typed, else the particle detected in a new name (`renamed_to`), else the
/// entry's own.
fn shown_particle(
    target: Option<&DictionaryEntry>,
    typed: Option<String>,
    renamed_to: Option<&str>,
    edit: &FamilyNameEdit,
) -> String {
    match (target, typed, renamed_to) {
        (Some(target), _, _) => entry_particle_split(target)
            .map(|(particle, _)| particle)
            .unwrap_or_default(),
        (None, Some(typed), _) => typed,
        (None, None, Some(name)) => split_surname_particle(name).0.unwrap_or_default(),
        (None, None, None) => edit.current_particle.clone(),
    }
}

/// Renames `value` to `new_value`, cut after `particle` when one was typed.
async fn rename(
    api: &ApiClient,
    tree_id: Uuid,
    value: &str,
    new_value: &str,
    particle: Option<&str>,
) -> Result<(), ApiError> {
    api.rename_family_name(tree_id, value, new_value, particle)
        .await
        .map(|_| ())
}

/// Cuts `value` after `particle`.
async fn recut(
    api: &ApiClient,
    tree_id: Uuid,
    value: &str,
    particle: &str,
) -> Result<(), ApiError> {
    api.set_family_name_particle(tree_id, value, particle)
        .await
        .map(|_| ())
}

/// How the name will be cut and filed, or why it cannot be cut so.
fn cut_preview(i18n: I18n, preview: Option<&(Option<String>, String)>, new_value: &str) -> Element {
    match preview {
        Some((particle, root)) => rsx! {
            div { class: "dict-particle-preview",
                div { class: "dict-particle-preview-row",
                    span { class: "dict-particle-preview-key", {i18n.t("dictionary.particle.preview_particle")} }
                    span { class: "dict-particle-preview-val",
                        {particle.clone().unwrap_or_else(|| i18n.t("dictionary.particle.none"))}
                    }
                }
                div { class: "dict-particle-preview-row",
                    span { class: "dict-particle-preview-key", {i18n.t("dictionary.particle.preview_root")} }
                    span { class: "dict-particle-preview-val", "{root}" }
                }
                div { class: "dict-particle-preview-row",
                    span { class: "dict-particle-preview-key", {i18n.t("dictionary.particle.preview_files_under")} }
                    span { class: "dict-particle-preview-val",
                        {root.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default()}
                    }
                }
            }
        },
        None if new_value.is_empty() => rsx! {},
        None => rsx! {
            div { class: "error-msg",
                {i18n.t_args("dictionary.particle.not_at_head", &[("name", new_value)])}
            }
        },
    }
}

/// The family-name editor: renames a surname across the persons whose main
/// name carries it, or re-cuts it between particle and root when the name is
/// left as it is. See ui-dictionary.md §7.1.
///
/// Shows what the edit will produce before applying it, because it touches
/// every person carrying the name at once.
#[component]
fn FamilyNameEditor(
    tree_id: Uuid,
    edit: FamilyNameEdit,
    /// Every listed family name, to tell a rename from a merge.
    known: Memo<FiledEntries<DictionaryEntry>>,
    on_close: EventHandler<()>,
    on_saved: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let name = use_signal(|| edit.value.clone());
    // What the user typed in the particle field; `None` while it follows the
    // name. Changing the name drops it, since a particle only fits the name it
    // was typed for.
    let mut typed_particle = use_signal(|| None::<String>);
    let mut error = use_signal(|| None::<String>);
    let mut saving = use_signal(|| false);

    let new_value = name().trim().to_string();
    let renaming = new_value != edit.value;
    // Renaming into a listed name merges into it, with the cut it already has.
    let target: Option<DictionaryEntry> = renaming
        .then(|| {
            known
                .read()
                .entries
                .iter()
                .find(|e| e.value == new_value)
                .cloned()
        })
        .flatten();
    let particle = shown_particle(
        target.as_ref(),
        typed_particle(),
        renaming.then_some(new_value.as_str()),
        &edit,
    );
    // The particle must already sit at the head of the name: the edit only
    // chooses where to cut, so anything else is rejected before it is sent.
    let preview = split_surname_at_head(&new_value, &particle).filter(|_| !new_value.is_empty());
    let others = (edit.count - edit.primary_count).max(0) as usize;
    let can_apply = preview.is_some() && !saving() && (!renaming || edit.primary_count > 0);

    let apply = {
        let edit = edit.clone();
        let new_value = new_value.clone();
        let particle = particle.clone();
        // Sent only when typed: otherwise the server picks the very cut the
        // preview shows — the merged name's, or the detected one.
        let sent_particle = if target.is_some() {
            None
        } else {
            typed_particle()
        };
        move |_| {
            let api = api.clone();
            let (value, new_value, particle, sent_particle) = (
                edit.value.clone(),
                new_value.clone(),
                particle.clone(),
                sent_particle.clone(),
            );
            saving.set(true);
            error.set(None);
            spawn(async move {
                let result = if renaming {
                    rename(&api, tree_id, &value, &new_value, sent_particle.as_deref()).await
                } else {
                    recut(&api, tree_id, &value, &particle).await
                };
                match result {
                    Ok(()) => on_saved.call(()),
                    Err(e) => {
                        saving.set(false);
                        error.set(Some(e.to_string()));
                    }
                }
            });
        }
    };

    rsx! {
        Modal {
            class: "dict-edit-modal",
            label: i18n.t("dictionary.family_name.title"),
            on_close,

            div { class: "dict-edit-header",
                h2 { {i18n.t("dictionary.family_name.title")} }
                button {
                    class: "person-form-close",
                    onclick: move |_| on_close.call(()),
                    "✕"
                }
            }

            div { class: "dict-particle-body",
                p { class: "dict-particle-intro",
                    {i18n.t_args("dictionary.family_name.intro", &[("name", &edit.value)])}
                }

                div { class: "form-group",
                    label { {i18n.t("dictionary.family_name.name_label")} }
                    ValueInput {
                        value: name,
                        tree_id,
                        field: SuggestionField::FamilyNames,
                        surname: true,
                        on_change: move |()| {
                            typed_particle.set(None);
                            error.set(None);
                        },
                    }
                }

                if renaming {
                    p { class: "dict-particle-scope",
                        {i18n.t_plural("dictionary.family_name.rename_scope", edit.primary_count as usize)}
                    }
                    if others > 0 {
                        p { class: "dict-particle-hint",
                            {i18n.t_plural("dictionary.family_name.others_keep", others)}
                        }
                    }
                    if let Some(target) = &target {
                        p { class: "field-hint field-hint-warn",
                            {i18n.t_args(
                                &i18n.plural_key("dictionary.family_name.merge", target.count as usize),
                                &[("name", &target.value), ("count", &target.count.to_string())],
                            )}
                        }
                    }
                } else {
                    p { class: "dict-particle-scope",
                        {i18n.t_plural("dictionary.particle.scope", edit.count as usize)}
                    }
                }

                div { class: "form-group",
                    label { {i18n.t("dictionary.particle.label")} }
                    input {
                        r#type: "text",
                        value: "{particle}",
                        disabled: target.is_some(),
                        placeholder: "{i18n.t(\"dictionary.particle.placeholder\")}",
                        oninput: move |e: Event<FormData>| {
                            typed_particle.set(Some(e.value()));
                            error.set(None);
                        },
                    }
                    if target.is_some() {
                        p { class: "dict-particle-hint",
                            {i18n.t_args("dictionary.family_name.merge_particle", &[("name", &new_value)])}
                        }
                    } else if !renaming {
                        p { class: "dict-particle-hint", {i18n.t("dictionary.particle.hint")} }
                    }
                }

                {cut_preview(i18n, preview.as_ref(), &new_value)}

                if let Some(err) = error() {
                    div { class: "error-msg", "{err}" }
                }
            }

            div { class: "modal-actions",
                button {
                    class: "td-btn",
                    onclick: move |_| on_close.call(()),
                    {i18n.t("common.cancel")}
                }
                button {
                    class: "td-btn td-btn-primary",
                    disabled: !can_apply,
                    onclick: apply,
                    if saving() {
                        {i18n.t("common.saving")}
                    } else if renaming {
                        {i18n.t("dictionary.family_name.rename")}
                    } else {
                        {i18n.t("dictionary.particle.apply")}
                    }
                }
            }
        }
    }
}

// ── Shared filter/pagination helpers ─────────────────────────────────────

/// The label an entry files under, given the viewer's particle preference.
///
/// A named function rather than a closure: the returned `&str` borrows from
/// the entry, which a closure in this position cannot express.
fn filing_label(entry: &DictionaryEntry, file_by_root: bool) -> &str {
    if file_by_root {
        entry.sort_key.as_str()
    } else {
        entry.value.as_str()
    }
}

/// Recovers where an entry is currently cut, as `(particle, root)`.
///
/// The boundary comes from `sort_key` — the root the backend filed the entry
/// under — rather than from re-detecting a particle here, so a name whose
/// particle was corrected by hand still splits where the user put it.
///
/// Returns `None` when there is no particle to separate, which is every
/// occupation and most surnames.
fn entry_particle_split(entry: &DictionaryEntry) -> Option<(String, String)> {
    let value = entry.value.trim();
    let root_chars = entry.sort_key.chars().count();
    let value_chars = value.chars().count();
    if root_chars == 0 || root_chars >= value_chars {
        return None;
    }

    // Cut from the end: the particle is the head, and only the root's length
    // is known. Lowercasing can change a string's length (ß → ss), so the
    // candidate is verified rather than trusted — a mismatch just means the
    // entry is left as written.
    let split_at = value_chars - root_chars;
    let particle: String = value.chars().take(split_at).collect();
    let root: String = value.chars().skip(split_at).collect();
    if root.to_lowercase() != entry.sort_key {
        return None;
    }

    let particle = particle.trim();
    if particle.is_empty() {
        return None;
    }
    Some((particle.to_string(), root))
}

/// How an entry reads when it files under its root: root first, particle
/// parenthesised behind it — "d'Aubigné" listed under A reads "Aubigné (d')".
fn row_label(entry: &DictionaryEntry, file_by_root: bool) -> String {
    if !file_by_root {
        return entry.value.clone();
    }
    match entry_particle_split(entry) {
        Some((particle, root)) => format!("{root} ({particle})"),
        None => entry.value.clone(),
    }
}

/// `filing` decides which letter group the entry belongs to, `display` is what
/// the free-text box searches. They differ only for surnames filed under their
/// root: "de la Cruz" files under C but is still found by typing "de la".
fn matches_filters(filing: &str, display: &str, quick: &str, letter: Option<char>) -> bool {
    if let Some(l) = letter {
        let first = filing.chars().next().map(|c| c.to_ascii_uppercase());
        if first != Some(l) {
            return false;
        }
    }
    quick.is_empty() || display.to_lowercase().contains(&quick.to_lowercase())
}

/// One tab's entries, filed and indexed once per fetch.
///
/// Filing and the letter index depend on the fetched data and the filing
/// preference — never on the quick filter, which is read on every keystroke.
/// Computed inline in the render body they were redone on every render, so
/// typing four characters re-cloned, re-sorted and re-scanned the whole
/// dictionary four times. The dictionary endpoints paginate nothing: they
/// return every distinct value in the tree.
#[derive(Clone, PartialEq)]
struct FiledEntries<T> {
    entries: Vec<T>,
    letters: HashSet<char>,
}

impl<T> Default for FiledEntries<T> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            letters: HashSet::new(),
        }
    }
}

/// File a value tab's entries under the viewer's preferred key.
///
/// Entries arrive sorted by `value` (particles included). Re-file them on
/// `sort_key` when the viewer prefers surnames under their root — for
/// occupations the two are the same string, so this changes nothing.
fn use_filed_values(
    resource: TabResource<Vec<DictionaryEntry>>,
    sort_particles: SortParticles,
) -> Memo<FiledEntries<DictionaryEntry>> {
    use_memo(move || {
        let Some(Some(Ok(entries))) = &*resource.read() else {
            return FiledEntries::default();
        };
        let file_by_root = !sort_particles.0;
        let mut entries = entries.clone();
        if file_by_root {
            entries.sort_by(|a, b| a.sort_key.cmp(&b.sort_key));
        }
        let letters = available_letters(entries.iter().map(|e| filing_label(e, file_by_root)));
        FiledEntries { entries, letters }
    })
}

fn use_filed_places(
    resource: TabResource<Vec<PlaceDictionaryEntry>>,
) -> Memo<FiledEntries<PlaceDictionaryEntry>> {
    use_memo(move || {
        let Some(Some(Ok(entries))) = &*resource.read() else {
            return FiledEntries::default();
        };
        let letters = available_letters(entries.iter().map(|e| e.place.name.as_str()));
        FiledEntries {
            entries: entries.clone(),
            letters,
        }
    })
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

/// The filters the alphabetical tabs share: the quick filter, the letter,
/// the page size and the page shown.
#[derive(Clone, Copy)]
struct ListFilters {
    quick: Signal<String>,
    letter: Signal<Option<char>>,
    page_size: Signal<PageSize>,
    page: Signal<usize>,
}

/// What every tab renders with: the language, the tree, the shared filters
/// and the usage list a row expands.
#[derive(Clone, Copy)]
struct TabContext<'a> {
    i18n: I18n,
    tree_id: &'a str,
    filters: ListFilters,
    expanded: Signal<Option<UsageKey>>,
    usage_people: Resource<(Option<UsageKey>, Vec<PersonUsageEntry>)>,
}

/// A tab of plain values with their counts: family names or occupations.
struct ValueTab {
    resource: TabResource<Vec<DictionaryEntry>>,
    empty_key: &'static str,
    /// Family names (rather than occupations) key their usage list.
    navigable: bool,
    sort_particles: SortParticles,
    /// `Some` only on the Family Names tab: occupations are not edited here.
    family_name_edit: Option<Signal<Option<FamilyNameEdit>>>,
    filed: Memo<FiledEntries<DictionaryEntry>>,
}

// ── Shared toolbar (alphabet index + quick filter + page size + count) ──

fn render_toolbar(
    i18n: I18n,
    letters: &HashSet<char>,
    filters: ListFilters,
    total_filtered: usize,
) -> Element {
    let ListFilters {
        quick: mut quick_filter,
        letter: mut letter_filter,
        mut page_size,
        page: mut current_page,
    } = filters;
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
    rsx! {
        Pager {
            current: page.saturating_sub(1),
            total: pages,
            on_select: move |index: usize| current_page.set(index + 1),
        }
        PrintPageNote { page, pages }
    }
}

fn render_clear_filters(i18n: I18n, filters: ListFilters) -> Element {
    let ListFilters {
        quick: mut quick_filter,
        letter: mut letter_filter,
        page: mut current_page,
        ..
    } = filters;
    rsx! {
        EmptyState {
            action: rsx! {
                button {
                    class: "sr-clear-filters",
                    onclick: move |_| {
                        quick_filter.set(String::new());
                        letter_filter.set(None);
                        current_page.set(1);
                    },
                    {i18n.t("dictionary.clear_filter")}
                }
            },
            p { {i18n.t("dictionary.no_matches")} }
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
    expected_key: &UsageKey,
    people: Resource<(Option<UsageKey>, Vec<PersonUsageEntry>)>,
) -> Element {
    let snapshot = people.read();
    // Ignore a resolved value fetched for a different (typically the
    // previously expanded) row — see the comment on `usage_resource`.
    let current = match &*snapshot {
        Some((Some(key), list)) if key == expected_key => Some(list),
        _ => None,
    };
    match current {
        Some(list) if !list.is_empty() => rsx! {
            div { class: "dict-accordion",
                for entry in list.iter() {
                    Link {
                        key: "{entry.person_id}",
                        to: Route::TreeDetail { tree_id: tree_id.to_string(), person: Some(entry.person_id.to_string()) },
                        class: "dict-accordion-item",
                        span { class: "dict-accordion-name", {surname_first(entry, &i18n)} }
                        {
                            let (birth, death) = entry.lifespan_years();
                            let lifespan = format_lifespan(i18n.dates(), birth, death);
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

fn render_value_tab(ctx: TabContext, tab: ValueTab) -> Element {
    let TabContext {
        i18n,
        tree_id,
        filters,
        mut expanded,
        usage_people,
    } = ctx;
    let ListFilters {
        quick: quick_filter,
        letter: letter_filter,
        page_size,
        page: current_page,
    } = filters;
    let ValueTab {
        resource,
        empty_key,
        navigable,
        sort_particles,
        family_name_edit,
        filed,
    } = tab;
    let file_by_root = !sort_particles.0;
    let filed = filed.read();
    let all_entries = &filed.entries;
    let letters = &filed.letters;

    let is_loading = !matches!(&*resource.read(), Some(Some(_)));
    let is_error = matches!(&*resource.read(), Some(Some(Err(_))));

    let quick = quick_filter();
    let letter = letter_filter();
    let filtered: Vec<&DictionaryEntry> = all_entries
        .iter()
        .filter(|e| matches_filters(filing_label(e, file_by_root), &e.value, &quick, letter))
        .collect();
    let total_filtered = filtered.len();
    let per_page = page_size().as_option();
    let page = current_page();
    let pages = total_pages(total_filtered, per_page);
    let page_items = paginate(filtered, page, per_page);
    let rows = with_headers(&page_items, |e| filing_label(e, file_by_root));

    rsx! {
        {render_toolbar(i18n, letters, filters, total_filtered)}

        if is_loading {
            div { class: "loading", {i18n.t("dictionary.loading")} }
        } else if is_error {
            div { class: "error-msg", {i18n.t("dictionary.error")} }
        } else if all_entries.is_empty() {
            EmptyState { p { {i18n.t(empty_key)} } }
        } else if rows.is_empty() {
            {render_clear_filters(i18n, filters)}
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
                        let label = row_label(entry, file_by_root);
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
                                        span { class: "dict-row-value", "{label}" }
                                    }
                                    span { class: "dict-row-count", {i18n.t_plural("dictionary.person_count", entry.count as usize)} }
                                    if let Some(mut family_name_edit) = family_name_edit {
                                        button {
                                            class: "dict-row-action",
                                            title: "{i18n.t(\"dictionary.family_name.edit\")}",
                                            onclick: {
                                                let entry = (*entry).clone();
                                                move |e: Event<MouseData>| {
                                                    // Without this the row's own
                                                    // handler would also toggle the
                                                    // usage accordion underneath.
                                                    e.stop_propagation();
                                                    family_name_edit.set(Some(FamilyNameEdit::new(&entry)));
                                                }
                                            },
                                            "\u{270E}"
                                        }
                                    }
                                    button {
                                        class: "dict-row-action",
                                        title: "{i18n.t(\"dictionary.view_usage\")}",
                                        if is_open { "\u{25B2}" } else { "\u{25BC}" }
                                    }
                                }
                                if is_open {
                                    {render_usage_accordion(i18n, tree_id, &key, usage_people)}
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

fn sources_total_label(i18n: &I18n, count: usize, prefix: &str) -> String {
    let suffix = i18n.0.plural_suffix(count);
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
            EmptyState {
                action: rsx! {
                    button {
                    class: "sr-clear-filters",
                    onclick: move |_| quick_filter.set(String::new()),
                    {i18n.t("dictionary.clear_filter")}
                }
                },
                p { {i18n.t("dictionary.no_matches")} }
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
    ctx: TabContext,
    sources: &[SourceDictionaryEntry],
    mut source_edit: Signal<Option<Uuid>>,
) -> Element {
    let TabContext {
        i18n,
        tree_id,
        filters,
        mut expanded,
        usage_people,
    } = ctx;
    let mut quick_filter = filters.quick;
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
            EmptyState {
                action: rsx! {
                    button {
                    class: "sr-clear-filters",
                    onclick: move |_| quick_filter.set(String::new()),
                    {i18n.t("dictionary.clear_filter")}
                }
                },
                p { {i18n.t("dictionary.no_matches")} }
            }
        } else {
            div { class: "dict-list",
                for entry in filtered.iter() {
                    {
                        let key = UsageKey::Source(entry.source.id);
                        let is_open = expanded() == Some(key.clone());
                        // The author, then the repositories holding the source.
                        let meta = entry
                            .source
                            .author
                            .iter()
                            .chain(entry.repositories.iter())
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(" \u{00B7} ");
                        let source_id = entry.source.id;
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
                                        title: "{i18n.t(\"common.edit\")}",
                                        onclick: move |e: Event<MouseData>| {
                                            e.stop_propagation();
                                            source_edit.set(Some(source_id));
                                        },
                                        "\u{270E}"
                                    }
                                    button {
                                        class: "dict-row-action",
                                        title: "{i18n.t(\"dictionary.view_usage\")}",
                                        if is_open { "\u{25B2}" } else { "\u{25BC}" }
                                    }
                                }
                                if is_open {
                                    {render_usage_accordion(i18n, tree_id, &key, usage_people)}
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn render_sources_tab(
    ctx: TabContext,
    history: Signal<Vec<String>>,
    resource: TabResource<SourcesView>,
    source_edit: Signal<Option<Uuid>>,
) -> Element {
    let TabContext { i18n, filters, .. } = ctx;
    let quick_filter = filters.quick;
    let is_loading = !matches!(&*resource.read(), Some(Some(_)));
    let is_error = matches!(&*resource.read(), Some(Some(Err(_))));
    let view: Option<SourcesView> = match &*resource.read() {
        Some(Some(Ok(v))) => Some(v.clone()),
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
            div { class: "loading", {i18n.t("dictionary.loading")} }
        } else if is_error {
            div { class: "error-msg", {i18n.t("dictionary.error")} }
        } else {
            match view {
                Some(SourcesView::Groups { prefix, total, groups }) if !groups.is_empty() => {
                    render_sources_groups(i18n, history, &prefix, total, &groups, quick_filter)
                }
                Some(SourcesView::List { sources, .. }) => {
                    render_sources_list(ctx, &sources, source_edit)
                }
                _ => rsx! {
                    EmptyState { p { {i18n.t("dictionary.no_entries_sources")} } }
                },
            }
        }
    }
}

// ── Places tab ────────────────────────────────────────────────────────────

fn render_places_tab(
    ctx: TabContext,
    resource: TabResource<Vec<PlaceDictionaryEntry>>,
    filed: Memo<FiledEntries<PlaceDictionaryEntry>>,
) -> Element {
    let TabContext {
        i18n,
        tree_id,
        filters,
        mut expanded,
        usage_people,
    } = ctx;
    let ListFilters {
        quick: quick_filter,
        letter: letter_filter,
        page_size,
        page: current_page,
    } = filters;
    let filed = filed.read();
    let all_entries = &filed.entries;
    let letters = &filed.letters;

    let is_loading = !matches!(&*resource.read(), Some(Some(_)));
    let is_error = matches!(&*resource.read(), Some(Some(Err(_))));

    let quick = quick_filter();
    let letter = letter_filter();
    let filtered: Vec<&PlaceDictionaryEntry> = all_entries
        .iter()
        .filter(|e| matches_filters(&e.place.name, &e.place.name, &quick, letter))
        .collect();
    let total_filtered = filtered.len();
    let per_page = page_size().as_option();
    let page = current_page();
    let pages = total_pages(total_filtered, per_page);
    let page_items = paginate(filtered, page, per_page);
    let rows = with_headers(&page_items, |e| e.place.name.as_str());

    rsx! {
        {render_toolbar(i18n, letters, filters, total_filtered)}

        if is_loading {
            div { class: "loading", {i18n.t("dictionary.loading")} }
        } else if is_error {
            div { class: "error-msg", {i18n.t("dictionary.error")} }
        } else if all_entries.is_empty() {
            EmptyState { p { {i18n.t("dictionary.no_entries_places")} } }
        } else if rows.is_empty() {
            {render_clear_filters(i18n, filters)}
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
                                    {render_usage_accordion(i18n, tree_id, &key, usage_people)}
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

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(value: &str, sort_key: &str) -> DictionaryEntry {
        DictionaryEntry {
            value: value.to_string(),
            sort_key: sort_key.to_string(),
            count: 1,
            primary_count: None,
        }
    }

    #[test]
    fn the_editor_opens_on_the_names_current_cut_and_primary_carriers() {
        let mut e = entry("de la Cruz", "la cruz");
        e.count = 3;
        e.primary_count = Some(2);
        let edit = FamilyNameEdit::new(&e);
        assert_eq!(edit.current_particle, "de");
        assert_eq!((edit.count, edit.primary_count), (3, 2));
        // A server that does not report primary carriers: assume all are.
        let edit = FamilyNameEdit::new(&entry("Thornby", "thornby"));
        assert_eq!(edit.current_particle, "");
        assert_eq!(edit.primary_count, 1);
    }

    #[test]
    fn a_particle_moves_behind_the_root_when_filing_by_root() {
        // Listed under A, so the A-group reads "Aubigné (d')" rather than
        // burying the root behind a particle the sort just ignored.
        let e = entry("d'Aubigné", "aubigné");
        assert_eq!(row_label(&e, true), "Aubigné (d')");
        // Filing with particles included leaves the name as written.
        assert_eq!(row_label(&e, false), "d'Aubigné");

        let e = entry("de la Cruz", "cruz");
        assert_eq!(row_label(&e, true), "Cruz (de la)");
    }

    #[test]
    fn a_name_without_a_particle_is_left_alone() {
        let e = entry("Dupont", "dupont");
        assert_eq!(row_label(&e, true), "Dupont");
        // A surname whose leading article is part of the name files whole, so
        // there is nothing to move behind anything.
        let e = entry("Le Branch", "le branch");
        assert_eq!(row_label(&e, true), "Le Branch");
    }

    #[test]
    fn the_boundary_comes_from_the_sort_key_not_from_detection() {
        // A particle narrowed by hand: detection would say "de la", but the
        // backend filed this under "la cruz" and that is what must be shown.
        let e = entry("de la Cruz", "la cruz");
        assert_eq!(
            entry_particle_split(&e),
            Some(("de".into(), "la Cruz".into()))
        );
        assert_eq!(row_label(&e, true), "la Cruz (de)");
    }

    #[test]
    fn a_sort_key_that_does_not_match_the_value_is_ignored() {
        // Occupations file under the whole value, so nothing splits.
        assert_eq!(entry_particle_split(&entry("Baker", "baker")), None);
        // A key that is not the tail of the value (stale or unrelated) must
        // not produce a bogus split.
        assert_eq!(entry_particle_split(&entry("de la Cruz", "smith")), None);
        assert_eq!(entry_particle_split(&entry("Cruz", "de la cruz")), None);
    }
}
