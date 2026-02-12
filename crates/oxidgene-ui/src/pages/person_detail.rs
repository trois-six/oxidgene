//! Person detail page — shows names, events, and related data.

use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::ApiClient;

/// Page rendered at `/trees/:tree_id/persons/:person_id`.
#[component]
pub fn PersonDetail(tree_id: String, person_id: String) -> Element {
    let api = use_context::<ApiClient>();
    let mut refresh = use_signal(|| 0u32);

    let tree_id_parsed = tree_id.parse::<Uuid>().ok();
    let person_id_parsed = person_id.parse::<Uuid>().ok();

    // Fetch person.
    let api_person = api.clone();
    let person_resource = use_resource(move || {
        let api = api_person.clone();
        let _tick = refresh();
        let tid = tree_id_parsed;
        let pid = person_id_parsed;
        async move {
            let (Some(tid), Some(pid)) = (tid, pid) else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: "Invalid IDs".to_string(),
                });
            };
            api.get_person(tid, pid).await
        }
    });

    // Fetch person names.
    let api_names = api.clone();
    let names_resource = use_resource(move || {
        let api = api_names.clone();
        let _tick = refresh();
        let tid = tree_id_parsed;
        let pid = person_id_parsed;
        async move {
            let (Some(tid), Some(pid)) = (tid, pid) else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: "Invalid IDs".to_string(),
                });
            };
            api.list_person_names(tid, pid).await
        }
    });

    // Fetch person events.
    let events_resource = use_resource(move || {
        let api = api.clone();
        let _tick = refresh();
        let tid = tree_id_parsed;
        let pid = person_id_parsed;
        async move {
            let (Some(tid), Some(pid)) = (tid, pid) else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: "Invalid IDs".to_string(),
                });
            };
            api.list_events(tid, Some(100), None, None, Some(pid), None)
                .await
        }
    });

    rsx! {
        // Person header
        match &*person_resource.read() {
            Some(Ok(person)) => rsx! {
                div { class: "page-header",
                    div {
                        h1 {
                            "Person "
                            span { class: "text-muted",
                                {person.id.to_string().chars().take(8).collect::<String>()}
                                "..."
                            }
                        }
                        span { class: "badge", {format!("{:?}", person.sex)} }
                    }
                    button {
                        class: "btn btn-outline",
                        onclick: move |_| refresh += 1,
                        "Refresh"
                    }
                }
            },
            Some(Err(e)) => rsx! {
                div { class: "error-msg", "Failed to load person: {e}" }
            },
            None => rsx! {
                div { class: "loading", "Loading person..." }
            },
        }

        // Names section
        div { class: "card", style: "margin-bottom: 24px;",
            h2 { style: "margin-bottom: 16px; font-size: 1.1rem;", "Names" }

            match &*names_resource.read() {
                Some(Ok(names)) => rsx! {
                    if names.is_empty() {
                        div { class: "empty-state",
                            p { "No names recorded." }
                        }
                    } else {
                        div { class: "table-wrapper",
                            table {
                                thead {
                                    tr {
                                        th { "Type" }
                                        th { "Given Names" }
                                        th { "Surname" }
                                        th { "Primary" }
                                    }
                                }
                                tbody {
                                    for name in names.iter() {
                                        tr {
                                            td {
                                                span { class: "badge", {format!("{:?}", name.name_type)} }
                                            }
                                            td { {name.given_names.as_deref().unwrap_or("--")} }
                                            td { {name.surname.as_deref().unwrap_or("--")} }
                                            td {
                                                if name.is_primary { "Yes" } else { "No" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
                Some(Err(e)) => rsx! {
                    div { class: "error-msg", "Failed to load names: {e}" }
                },
                None => rsx! {
                    div { class: "loading", "Loading names..." }
                },
            }
        }

        // Events section
        div { class: "card",
            h2 { style: "margin-bottom: 16px; font-size: 1.1rem;", "Events" }

            match &*events_resource.read() {
                Some(Ok(conn)) => rsx! {
                    if conn.edges.is_empty() {
                        div { class: "empty-state",
                            p { "No events recorded." }
                        }
                    } else {
                        div { class: "table-wrapper",
                            table {
                                thead {
                                    tr {
                                        th { "Type" }
                                        th { "Date" }
                                        th { "Description" }
                                    }
                                }
                                tbody {
                                    for edge in conn.edges.iter() {
                                        {
                                            let event = &edge.node;
                                            rsx! {
                                                tr {
                                                    td {
                                                        span { class: "badge", {format!("{:?}", event.event_type)} }
                                                    }
                                                    td {
                                                        {event.date_value.as_deref().unwrap_or("--")}
                                                    }
                                                    td { class: "text-muted",
                                                        {event.description.as_deref().unwrap_or("--")}
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
                Some(Err(e)) => rsx! {
                    div { class: "error-msg", "Failed to load events: {e}" }
                },
                None => rsx! {
                    div { class: "loading", "Loading events..." }
                },
            }
        }
    }
}
