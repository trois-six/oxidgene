//! Tree detail page — shows persons and families in a tree.

use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::ApiClient;
use crate::router::Route;

/// Page rendered at `/trees/:tree_id`.
#[component]
pub fn TreeDetail(tree_id: String) -> Element {
    let api = use_context::<ApiClient>();
    let mut refresh = use_signal(|| 0u32);

    let tree_id_parsed = tree_id.parse::<Uuid>().ok();

    // Fetch tree details.
    let api_tree = api.clone();
    let tree_resource = use_resource(move || {
        let api = api_tree.clone();
        let _tick = refresh();
        let tid = tree_id_parsed;
        async move {
            let Some(tid) = tid else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: "Invalid tree ID".to_string(),
                });
            };
            api.get_tree(tid).await
        }
    });

    // Fetch persons in the tree.
    let api_persons = api.clone();
    let persons_resource = use_resource(move || {
        let api = api_persons.clone();
        let _tick = refresh();
        let tid = tree_id_parsed;
        async move {
            let Some(tid) = tid else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: "Invalid tree ID".to_string(),
                });
            };
            api.list_persons(tid, Some(100), None).await
        }
    });

    // Fetch families in the tree.
    let families_resource = use_resource(move || {
        let api = api.clone();
        let _tick = refresh();
        let tid = tree_id_parsed;
        async move {
            let Some(tid) = tid else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: "Invalid tree ID".to_string(),
                });
            };
            api.list_families(tid, Some(100), None).await
        }
    });

    rsx! {
        // Tree header
        match &*tree_resource.read() {
            Some(Ok(tree)) => rsx! {
                div { class: "page-header",
                    div {
                        h1 { "{tree.name}" }
                        if let Some(desc) = &tree.description {
                            p { class: "text-muted", "{desc}" }
                        }
                    }
                    div { style: "display: flex; gap: 8px;",
                        button {
                            class: "btn btn-outline",
                            onclick: move |_| refresh += 1,
                            "Refresh"
                        }
                    }
                }
            },
            Some(Err(e)) => rsx! {
                div { class: "error-msg", "Failed to load tree: {e}" }
            },
            None => rsx! {
                div { class: "loading", "Loading tree..." }
            },
        }

        // Persons section
        div { class: "card", style: "margin-bottom: 24px;",
            h2 { style: "margin-bottom: 16px; font-size: 1.1rem;", "Persons" }

            match &*persons_resource.read() {
                Some(Ok(conn)) => rsx! {
                    if conn.edges.is_empty() {
                        div { class: "empty-state",
                            p { "No persons in this tree yet." }
                        }
                    } else {
                        div { class: "table-wrapper",
                            table {
                                thead {
                                    tr {
                                        th { "ID" }
                                        th { "Sex" }
                                        th { "Created" }
                                    }
                                }
                                tbody {
                                    for edge in conn.edges.iter() {
                                        {
                                            let person = &edge.node;
                                            let pid = person.id.to_string();
                                            let tid = tree_id.clone();
                                            rsx! {
                                                tr {
                                                    td {
                                                        Link {
                                                            to: Route::PersonDetail {
                                                                tree_id: tid,
                                                                person_id: pid,
                                                            },
                                                            {person.id.to_string().chars().take(8).collect::<String>()}
                                                            "..."
                                                        }
                                                    }
                                                    td {
                                                        span { class: "badge", {format!("{:?}", person.sex)} }
                                                    }
                                                    td { class: "text-muted",
                                                        {person.created_at.format("%Y-%m-%d").to_string()}
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        p { class: "text-muted", style: "margin-top: 8px; font-size: 0.85rem;",
                            "Total: {conn.total_count}"
                        }
                    }
                },
                Some(Err(e)) => rsx! {
                    div { class: "error-msg", "Failed to load persons: {e}" }
                },
                None => rsx! {
                    div { class: "loading", "Loading persons..." }
                },
            }
        }

        // Families section
        div { class: "card",
            h2 { style: "margin-bottom: 16px; font-size: 1.1rem;", "Families" }

            match &*families_resource.read() {
                Some(Ok(conn)) => rsx! {
                    if conn.edges.is_empty() {
                        div { class: "empty-state",
                            p { "No families in this tree yet." }
                        }
                    } else {
                        div { class: "table-wrapper",
                            table {
                                thead {
                                    tr {
                                        th { "Family ID" }
                                        th { "Created" }
                                    }
                                }
                                tbody {
                                    for edge in conn.edges.iter() {
                                        {
                                            let family = &edge.node;
                                            rsx! {
                                                tr {
                                                    td {
                                                        {family.id.to_string().chars().take(8).collect::<String>()}
                                                        "..."
                                                    }
                                                    td { class: "text-muted",
                                                        {family.created_at.format("%Y-%m-%d").to_string()}
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        p { class: "text-muted", style: "margin-top: 8px; font-size: 0.85rem;",
                            "Total: {conn.total_count}"
                        }
                    }
                },
                Some(Err(e)) => rsx! {
                    div { class: "error-msg", "Failed to load families: {e}" }
                },
                None => rsx! {
                    div { class: "loading", "Loading families..." }
                },
            }
        }
    }
}
