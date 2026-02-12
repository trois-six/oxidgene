//! Tree list page — shows all genealogy trees with create / delete.

use dioxus::prelude::*;

use crate::api::{ApiClient, CreateTreeBody};
use crate::router::Route;

/// Page rendered at `/trees`.
#[component]
pub fn TreeList() -> Element {
    let api = use_context::<ApiClient>();
    let mut refresh_counter = use_signal(|| 0u32);

    // Fetch trees whenever refresh_counter changes.
    let api_res = api.clone();
    let trees_resource = use_resource(move || {
        let api = api_res.clone();
        let _tick = refresh_counter();
        async move { api.list_trees(Some(100), None).await }
    });

    // New-tree form state.
    let mut new_name = use_signal(String::new);
    let mut new_desc = use_signal(String::new);
    let mut show_form = use_signal(|| false);
    let mut form_error = use_signal(|| None::<String>);

    // Create tree handler.
    let on_create = move |_| {
        let api = api.clone();
        let name = new_name().trim().to_string();
        let desc = new_desc().trim().to_string();
        spawn(async move {
            if name.is_empty() {
                form_error.set(Some("Name is required".to_string()));
                return;
            }
            let body = CreateTreeBody {
                name,
                description: if desc.is_empty() { None } else { Some(desc) },
            };
            match api.create_tree(&body).await {
                Ok(_) => {
                    new_name.set(String::new());
                    new_desc.set(String::new());
                    show_form.set(false);
                    form_error.set(None);
                    refresh_counter += 1;
                }
                Err(e) => {
                    form_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    rsx! {
        div { class: "page-header",
            h1 { "Genealogy Trees" }
            button {
                class: "btn btn-primary",
                onclick: move |_| show_form.toggle(),
                if show_form() { "Cancel" } else { "New Tree" }
            }
        }

        // Create tree form
        if show_form() {
            div { class: "card", style: "margin-bottom: 24px;",
                h3 { style: "margin-bottom: 16px;", "Create New Tree" }

                if let Some(err) = form_error() {
                    div { class: "error-msg", "{err}" }
                }

                div { class: "form-group",
                    label { "Name" }
                    input {
                        r#type: "text",
                        placeholder: "e.g. Erraud Family Tree",
                        value: "{new_name}",
                        oninput: move |e: Event<FormData>| new_name.set(e.value()),
                    }
                }
                div { class: "form-group",
                    label { "Description (optional)" }
                    textarea {
                        rows: 3,
                        placeholder: "A brief description of this tree...",
                        value: "{new_desc}",
                        oninput: move |e: Event<FormData>| new_desc.set(e.value()),
                    }
                }
                button { class: "btn btn-primary", onclick: on_create, "Create" }
            }
        }

        // Trees list
        match &*trees_resource.read() {
            Some(Ok(conn)) => rsx! {
                if conn.edges.is_empty() {
                    div { class: "empty-state",
                        h3 { "No trees yet" }
                        p { "Create your first genealogy tree to get started." }
                    }
                } else {
                    div { class: "card",
                        div { class: "table-wrapper",
                            table {
                                thead {
                                    tr {
                                        th { "Name" }
                                        th { "Description" }
                                        th { "Created" }
                                    }
                                }
                                tbody {
                                    for edge in conn.edges.iter() {
                                        {
                                            let tree = &edge.node;
                                            let tid = tree.id.to_string();
                                            rsx! {
                                                tr {
                                                    td {
                                                        Link {
                                                            to: Route::TreeDetail { tree_id: tid },
                                                            "{tree.name}"
                                                        }
                                                    }
                                                    td { class: "text-muted",
                                                        {tree.description.as_deref().unwrap_or("--")}
                                                    }
                                                    td { class: "text-muted",
                                                        {tree.created_at.format("%Y-%m-%d").to_string()}
                                                    }
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
                div { class: "error-msg", "Failed to load trees: {e}" }
            },
            None => rsx! {
                div { class: "loading", "Loading trees..." }
            },
        }
    }
}
