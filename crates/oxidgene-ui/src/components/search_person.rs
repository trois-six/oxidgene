//! Typeahead search component for finding and linking existing persons.
//!
//! Used in the Geneanet-style UI for "Add Spouse", "Add Parents", "Add Child"
//! flows where the user can either create a new person or link to an existing one.
//!
//! Performance: uses the server-side `/cache/search` endpoint which queries the
//! accent-folded, normalised search index instead of downloading the full tree.

use dioxus::prelude::*;
use oxidgene_cache::types::SearchEntry;
use oxidgene_core::Sex;
use uuid::Uuid;

use crate::api::ApiClient;
use crate::i18n::use_i18n;

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
            #[cfg(target_arch = "wasm32")]
            gloo_timers::future::TimeoutFuture::new(200).await;
            #[cfg(not(target_arch = "wasm32"))]
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
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
                return api.cache_search(tree_id, "", 20, 0).await;
            }
            api.cache_search(tree_id, &q, 20, 0).await
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
                        {render_search_entry(entry, props.on_select)}
                    }
                }
            }
        }
    }
}

/// Render a single search result row.
fn render_search_entry(entry: &SearchEntry, on_select: EventHandler<Uuid>) -> Element {
    let rid = entry.person_id;
    let sex_class = match entry.sex {
        Sex::Male => "male",
        Sex::Female => "female",
        Sex::Unknown => "",
    };

    // Parse display_name into given_names + surname for initials.
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

    rsx! {
        button {
            class: "search-person-result {sex_class}",
            onclick: move |_| on_select.call(rid),
            div { class: "sp-result-photo",
                span { class: "sp-result-initials {sex_class}", "{initials}" }
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
                    if let Some(ref bd) = entry.birth_year {
                        span { class: "sp-birth", "\u{2726} {bd}" }
                    }
                    if let Some(ref dd) = entry.death_year {
                        span { class: "sp-death", "\u{271D} {dd}" }
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
