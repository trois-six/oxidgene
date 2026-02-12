//! Person detail page — shows names, events, and related data with edit/delete.

use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::ApiClient;
use crate::router::Route;

/// Page rendered at `/trees/:tree_id/persons/:person_id`.
#[component]
pub fn PersonDetail(tree_id: String, person_id: String) -> Element {
    let api = use_context::<ApiClient>();
    let nav = use_navigator();
    let mut refresh = use_signal(|| 0u32);

    let tree_id_parsed = tree_id.parse::<Uuid>().ok();
    let person_id_parsed = person_id.parse::<Uuid>().ok();

    // Delete confirmation state.
    let mut confirm_delete = use_signal(|| false);
    let mut delete_error = use_signal(|| None::<String>);

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
    let api_events = api.clone();
    let events_resource = use_resource(move || {
        let api = api_events.clone();
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

    // Derive display name from loaded names.
    let display_name = match &*names_resource.read() {
        Some(Ok(names)) => {
            // Prefer primary name, fallback to first name.
            let primary = names.iter().find(|n| n.is_primary).or(names.first());
            match primary {
                Some(name) => {
                    let dn = name.display_name();
                    if dn.is_empty() {
                        "Unnamed".to_string()
                    } else {
                        dn
                    }
                }
                None => "Unnamed".to_string(),
            }
        }
        _ => "Loading...".to_string(),
    };

    // Delete handler.
    let tree_id_nav = tree_id.clone();
    let on_confirm_delete = move |_| {
        let api = api.clone();
        let Some(tid) = tree_id_parsed else { return };
        let Some(pid) = person_id_parsed else { return };
        let tree_id_nav = tree_id_nav.clone();
        spawn(async move {
            match api.delete_person(tid, pid).await {
                Ok(_) => {
                    nav.push(Route::TreeDetail {
                        tree_id: tree_id_nav,
                    });
                }
                Err(e) => {
                    delete_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    rsx! {
        // Back navigation
        div { style: "margin-bottom: 16px;",
            Link {
                to: Route::TreeDetail { tree_id: tree_id.clone() },
                class: "back-link",
                "← Back to Tree"
            }
        }

        // Delete confirmation dialog
        if confirm_delete() {
            div { class: "modal-backdrop",
                div { class: "modal-card",
                    h3 { "Delete Person" }
                    p { style: "margin: 12px 0;",
                        "Are you sure you want to delete "
                        strong { "{display_name}" }
                        "? This action cannot be undone."
                    }
                    if let Some(err) = delete_error() {
                        div { class: "error-msg", "{err}" }
                    }
                    div { class: "modal-actions",
                        button {
                            class: "btn btn-outline",
                            onclick: move |_| {
                                confirm_delete.set(false);
                                delete_error.set(None);
                            },
                            "Cancel"
                        }
                        button {
                            class: "btn btn-danger",
                            onclick: on_confirm_delete,
                            "Delete"
                        }
                    }
                }
            }
        }

        // Person header
        match &*person_resource.read() {
            Some(Ok(person)) => rsx! {
                div { class: "page-header",
                    div {
                        h1 { "{display_name}" }
                        div { style: "display: flex; align-items: center; gap: 8px; margin-top: 4px;",
                            span { class: "badge", {format!("{:?}", person.sex)} }
                            span { class: "text-muted", style: "font-size: 0.85rem;",
                                "ID: "
                                {person.id.to_string().chars().take(8).collect::<String>()}
                                "..."
                            }
                        }
                    }
                    div { style: "display: flex; gap: 8px;",
                        button {
                            class: "btn btn-danger",
                            onclick: move |_| {
                                confirm_delete.set(true);
                                delete_error.set(None);
                            },
                            "Delete"
                        }
                        button {
                            class: "btn btn-outline",
                            onclick: move |_| refresh += 1,
                            "Refresh"
                        }
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
