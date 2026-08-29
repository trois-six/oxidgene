//! Typeahead search component for finding and linking existing persons.
//!
//! Used in the UI for "Add Spouse", "Add Parents", "Add Child"
//! flows where the user can either create a new person or link to an existing one.
//!
//! Performance: uses the server-side `/persons/search?q=...` endpoint, backed
//! by the `person_search_fts` DB table (SQLite FTS5 / PostgreSQL) with
//! accent-folded matching, instead of downloading the full tree.

use dioxus::prelude::*;
use oxidgene_core::Sex;
use oxidgene_core::projection::{PersonProfile, SearchEntry};
use uuid::Uuid;

use crate::api::ApiClient;
use crate::components::pedigree_chart::default_portrait;
use crate::i18n::use_i18n;

#[derive(Clone)]
pub(crate) struct PersonSearchSummary {
    person_id: Uuid,
    sex: Sex,
    surname: String,
    given_names: String,
    birth_year: Option<String>,
    birth_place: Option<String>,
    death_year: Option<String>,
}

impl PersonSearchSummary {
    pub(crate) fn placeholder(person_id: Uuid, label: String) -> Self {
        Self {
            person_id,
            sex: Sex::Unknown,
            surname: String::new(),
            given_names: label,
            birth_year: None,
            birth_place: None,
            death_year: None,
        }
    }
}

impl From<&SearchEntry> for PersonSearchSummary {
    fn from(entry: &SearchEntry) -> Self {
        Self {
            person_id: entry.person_id,
            sex: entry.sex,
            surname: entry.surname.clone(),
            given_names: entry.given_names.clone(),
            birth_year: entry.birth_year.clone(),
            birth_place: entry.birth_place.clone(),
            death_year: entry.death_year.clone(),
        }
    }
}

impl From<PersonProfile> for PersonSearchSummary {
    fn from(profile: PersonProfile) -> Self {
        let primary_name = profile.primary_name.as_ref();
        Self {
            person_id: profile.person_id,
            sex: profile.sex,
            surname: primary_name
                .and_then(|name| name.surname.clone())
                .unwrap_or_default(),
            given_names: primary_name
                .and_then(|name| name.given_names.clone())
                .unwrap_or_default(),
            birth_year: profile.birth.as_ref().and_then(profile_event_year),
            birth_place: profile
                .birth
                .as_ref()
                .and_then(|event| event.place_name.clone()),
            death_year: profile.death.as_ref().and_then(profile_event_year),
        }
    }
}

fn profile_event_year(event: &oxidgene_core::projection::ProfileEvent) -> Option<String> {
    oxidgene_core::types::year_from_date(event.date_sort, event.date_value.as_deref())
        .map(|year| format!("{year:04}"))
}

/// Props for [`SearchPerson`].
#[derive(Props, Clone, PartialEq)]
pub struct SearchPersonProps {
    /// Tree ID to search within.
    pub tree_id: Uuid,
    /// Placeholder text for the input.
    #[props(default = String::new())]
    pub placeholder: String,
    /// Called when the user selects a person from the results.
    pub on_select: EventHandler<Uuid>,
    /// Called when the user wants to cancel the search.
    pub on_cancel: EventHandler<()>,
}

/// A typeahead search input that queries the server-side search index.
///
/// Keystroke input is debounced by 200 ms before the search request fires.
/// At most 20 results are fetched per query.
#[component]
pub fn SearchPerson(props: SearchPersonProps) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let mut query = use_signal(String::new);
    let tree_id = props.tree_id;

    let api_portraits = api.clone();
    let portraits_resource = use_resource(move || {
        let api = api_portraits.clone();
        async move {
            match api.list_portraits(tree_id).await {
                Ok(rows) => api.portrait_map(tree_id, &rows).await,
                Err(_) => Default::default(),
            }
        }
    });

    let placeholder = if props.placeholder.is_empty() {
        i18n.t("search.placeholder")
    } else {
        props.placeholder.clone()
    };

    // Debounce: update the committed query after a short delay.
    let mut debounced_query = use_signal(String::new);
    let _debounce_task = use_resource(move || {
        let raw = query();
        async move {
            crate::utils::sleep_ms(200).await;
            debounced_query.set(raw);
        }
    });

    // Server-side search: fires when debounced_query changes.
    let api_search = api.clone();
    let search_resource = use_resource(move || {
        let api = api_search.clone();
        let q = debounced_query();
        async move {
            if q.is_empty() {
                // Empty query: return first 20 persons (no filter).
                return api.search_persons(tree_id, "", 20, 0).await;
            }
            api.search_persons(tree_id, &q, 20, 0).await
        }
    });

    let results: Vec<SearchEntry> = {
        let data = search_resource.read();
        match &*data {
            Some(Ok(sr)) => sr.entries.clone(),
            _ => vec![],
        }
    };

    let is_loading = search_resource.read().is_none();
    let portrait_urls = {
        let data = portraits_resource.read();
        match &*data {
            Some(urls) => urls.clone(),
            _ => Default::default(),
        }
    };

    rsx! {
        div { class: "search-person",
            div { class: "search-person-input-row",
                input {
                    r#type: "text",
                    placeholder: "{placeholder}",
                    value: "{query}",
                    oninput: move |e: Event<FormData>| query.set(e.value()),
                }
                button {
                    class: "btn btn-outline btn-sm",
                    onclick: move |_| props.on_cancel.call(()),
                    {i18n.t("common.cancel")}
                }
            }

            if is_loading {
                div { class: "loading", {i18n.t("search.loading")} }
            } else if results.is_empty() {
                div { class: "text-muted", style: "padding: 8px;",
                    {i18n.t("search.no_match")}
                }
            } else {
                div { class: "search-person-results",
                    for entry in results.iter() {
                        {render_search_entry(
                            entry,
                            props.on_select,
                            portrait_urls.get(&entry.person_id).cloned(),
                        )}
                    }
                }
            }
        }
    }
}

/// Render a single search result row.
fn render_search_entry(
    entry: &SearchEntry,
    on_select: EventHandler<Uuid>,
    portrait_url: Option<String>,
) -> Element {
    let summary = PersonSearchSummary::from(entry);
    let rid = summary.person_id;
    let sex_class = match summary.sex {
        Sex::Male => "male",
        Sex::Female => "female",
        Sex::Unknown => "",
    };

    rsx! {
        button {
            class: "search-person-result {sex_class}",
            onclick: move |_| on_select.call(rid),
            {render_person_search_summary(&summary, portrait_url)}
        }
    }
}

pub(crate) fn render_person_search_summary(
    summary: &PersonSearchSummary,
    portrait_url: Option<String>,
) -> Element {
    let given = &summary.given_names;
    let surname = &summary.surname;
    let portrait_src = portrait_url.unwrap_or_else(|| default_portrait(summary.sex).to_string());

    rsx! {
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
                if let Some(ref birth_year) = summary.birth_year {
                    span { class: "sp-birth", "\u{2726} {birth_year}" }
                }
                if let Some(ref death_year) = summary.death_year {
                    span { class: "sp-death", "\u{271D} {death_year}" }
                }
            }
            if let Some(ref birth_place) = summary.birth_place {
                div { class: "sp-result-meta",
                    span { class: "sp-place", "{birth_place}" }
                }
            }
        }
    }
}
