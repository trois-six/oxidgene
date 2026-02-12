//! Typeahead search component for finding and linking existing persons.
//!
//! Used in the Geneanet-style UI for "Add Spouse", "Add Parents", "Add Child"
//! flows where the user can either create a new person or link to an existing one.

use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::ApiClient;
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
    #[props(default = "Search for a person...".to_string())]
    pub placeholder: String,
    /// Called when the user selects a person from the results.
    pub on_select: EventHandler<Uuid>,
    /// Called when the user wants to cancel the search.
    pub on_cancel: EventHandler<()>,
}

/// A typeahead search input that queries the person list and presents matches.
///
/// The search filters client-side against the pre-fetched person list (names).
/// This avoids a server-side search endpoint for now and works well for
/// moderate-sized trees.
#[component]
pub fn SearchPerson(props: SearchPersonProps) -> Element {
    let api = use_context::<ApiClient>();
    let mut query = use_signal(String::new);
    let tree_id = props.tree_id;

    // Fetch all persons and names for the tree.
    let api_persons = api.clone();
    let persons_resource = use_resource(move || {
        let api = api_persons.clone();
        async move { api.list_persons(tree_id, Some(500), None).await }
    });

    let names_resource = use_resource(move || {
        let api = api.clone();
        async move {
            let conn = api.list_persons(tree_id, Some(500), None).await.ok();
            if let Some(conn) = conn {
                let ids: Vec<Uuid> = conn.edges.iter().map(|e| e.node.id).collect();
                let mut name_map = std::collections::HashMap::new();
                for pid in ids {
                    if let Ok(names) = api.list_person_names(tree_id, pid).await {
                        name_map.insert(pid, names);
                    }
                }
                Ok(name_map)
            } else {
                Err(crate::api::ApiError::Api {
                    status: 0,
                    body: "Failed to load persons".to_string(),
                })
            }
        }
    });

    // Build search results from loaded data.
    let results: Vec<PersonSearchResult> = {
        let q = query().to_lowercase();
        let persons_data = persons_resource.read();
        let names_data = names_resource.read();

        match (&*persons_data, &*names_data) {
            (Some(Ok(conn)), Some(Ok(name_map))) => {
                if q.is_empty() {
                    // Show all persons when query is empty (limited).
                    conn.edges
                        .iter()
                        .take(20)
                        .map(|e| {
                            let name = resolve_name(e.node.id, name_map);
                            let sex_label = format!("{:?}", e.node.sex);
                            PersonSearchResult {
                                id: e.node.id,
                                name,
                                sex_label,
                            }
                        })
                        .collect()
                } else {
                    conn.edges
                        .iter()
                        .filter_map(|e| {
                            let name = resolve_name(e.node.id, name_map);
                            if name.to_lowercase().contains(&q) {
                                Some(PersonSearchResult {
                                    id: e.node.id,
                                    name,
                                    sex_label: format!("{:?}", e.node.sex),
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

    let is_loading = persons_resource.read().is_none() || names_resource.read().is_none();

    rsx! {
        div { class: "search-person",
            div { class: "search-person-input-row",
                input {
                    r#type: "text",
                    placeholder: "{props.placeholder}",
                    value: "{query}",
                    oninput: move |e: Event<FormData>| query.set(e.value()),
                }
                button {
                    class: "btn btn-outline btn-sm",
                    onclick: move |_| props.on_cancel.call(()),
                    "Cancel"
                }
            }

            if is_loading {
                div { class: "loading", "Loading persons..." }
            } else if results.is_empty() {
                div { class: "text-muted", style: "padding: 8px;",
                    "No matching persons found."
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
