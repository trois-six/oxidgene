//! Typeahead search component for finding and linking existing persons.
//!
//! Used in the Geneanet-style UI for "Add Spouse", "Add Parents", "Add Child"
//! flows where the user can either create a new person or link to an existing one.
//!
//! Performance: uses the single `/snapshot` endpoint (cached) instead of N+1
//! per-person requests.

use std::collections::HashMap;

use dioxus::prelude::*;
use oxidgene_core::types::PersonName;
use uuid::Uuid;

use crate::api::ApiClient;
use crate::i18n::use_i18n;
use crate::utils::resolve_name;

/// A search result entry.
#[derive(Debug, Clone, PartialEq)]
pub struct PersonSearchResult {
    /// Person ID.
    pub id: Uuid,
    /// Display name (already resolved).
    pub name: String,
    /// Sex display string.
    pub sex_label: String,
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

/// A typeahead search input that queries the person list and presents matches.
///
/// Uses the snapshot endpoint (cached by `ApiClient`) so that even with 10 000+
/// persons only a single HTTP request is made.
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

    // Debounce state: the actual query used for filtering is updated with a
    // small delay so we don't re-filter 10 000+ persons on every keystroke.
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

    // Fetch all data via snapshot (single HTTP request, cached).
    let api_snap = api.clone();
    let snapshot_resource = use_resource(move || {
        let api = api_snap.clone();
        async move { api.get_tree_snapshot(tree_id).await }
    });

    // Build search results from loaded data.
    let results: Vec<PersonSearchResult> = {
        let q = debounced_query().to_lowercase();
        let snap_data = snapshot_resource.read();

        match &*snap_data {
            Some(Ok(snapshot)) => {
                // Build name map from snapshot names.
                let mut name_map: HashMap<Uuid, Vec<PersonName>> = HashMap::new();
                for pn in snapshot.names.iter() {
                    name_map.entry(pn.person_id).or_default().push(pn.clone());
                }

                if q.is_empty() {
                    snapshot
                        .persons
                        .iter()
                        .take(20)
                        .map(|p| {
                            let name = resolve_name(p.id, &name_map);
                            let sex_label = format!("{:?}", p.sex);
                            PersonSearchResult {
                                id: p.id,
                                name,
                                sex_label,
                            }
                        })
                        .collect()
                } else {
                    snapshot
                        .persons
                        .iter()
                        .filter_map(|p| {
                            let name = resolve_name(p.id, &name_map);
                            if name.to_lowercase().contains(&q) {
                                Some(PersonSearchResult {
                                    id: p.id,
                                    name,
                                    sex_label: format!("{:?}", p.sex),
                                })
                            } else {
                                None
                            }
                        })
                        .take(20)
                        .collect()
                }
            }
            _ => vec![],
        }
    };

    let is_loading = snapshot_resource.read().is_none();

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
                    for result in results.iter() {
                        {
                            let rid = result.id;
                            rsx! {
                                button {
                                    class: "search-person-result",
                                    onclick: move |_| props.on_select.call(rid),
                                    span { class: "search-person-name", "{result.name}" }
                                    span { class: "search-person-sex badge", "{result.sex_label}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
