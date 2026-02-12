//! Tree detail page — shows tree info, persons, and families, with edit & delete.

use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::{ApiClient, CreatePersonBody, UpdateTreeBody};
use crate::router::Route;
use oxidgene_core::Sex;

/// Page rendered at `/trees/:tree_id`.
#[component]
pub fn TreeDetail(tree_id: String) -> Element {
    let api = use_context::<ApiClient>();
    let nav = use_navigator();
    let mut refresh = use_signal(|| 0u32);

    let tree_id_parsed = tree_id.parse::<Uuid>().ok();

    // Edit state.
    let mut editing = use_signal(|| false);
    let mut edit_name = use_signal(String::new);
    let mut edit_desc = use_signal(String::new);
    let mut edit_error = use_signal(|| None::<String>);

    // Delete confirmation state.
    let mut confirm_delete = use_signal(|| false);
    let mut delete_error = use_signal(|| None::<String>);

    // Create person form state.
    let mut show_person_form = use_signal(|| false);
    let mut new_person_sex = use_signal(|| "Unknown".to_string());
    let mut person_form_error = use_signal(|| None::<String>);

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
    let api_families = api.clone();
    let families_resource = use_resource(move || {
        let api = api_families.clone();
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

    // Save edit handler.
    let api_edit = api.clone();
    let on_save_edit = move |_| {
        let api = api_edit.clone();
        let Some(tid) = tree_id_parsed else { return };
        let name = edit_name().trim().to_string();
        let desc = edit_desc().trim().to_string();
        spawn(async move {
            if name.is_empty() {
                edit_error.set(Some("Name is required".to_string()));
                return;
            }
            let body = UpdateTreeBody {
                name: Some(name),
                description: Some(if desc.is_empty() { None } else { Some(desc) }),
            };
            match api.update_tree(tid, &body).await {
                Ok(_) => {
                    editing.set(false);
                    edit_error.set(None);
                    refresh += 1;
                }
                Err(e) => {
                    edit_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    // Confirm delete handler.
    let api_del = api.clone();
    let on_confirm_delete = move |_| {
        let api = api_del.clone();
        let Some(tid) = tree_id_parsed else { return };
        spawn(async move {
            match api.delete_tree(tid).await {
                Ok(_) => {
                    nav.push(Route::TreeList {});
                }
                Err(e) => {
                    delete_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    // Create person handler.
    let on_create_person = move |_| {
        let api = api.clone();
        let Some(tid) = tree_id_parsed else { return };
        let sex_str = new_person_sex();
        spawn(async move {
            let sex = match sex_str.as_str() {
                "Male" => Sex::Male,
                "Female" => Sex::Female,
                _ => Sex::Unknown,
            };
            let body = CreatePersonBody { sex };
            match api.create_person(tid, &body).await {
                Ok(_) => {
                    show_person_form.set(false);
                    new_person_sex.set("Unknown".to_string());
                    person_form_error.set(None);
                    refresh += 1;
                }
                Err(e) => {
                    person_form_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    rsx! {
        // Back navigation
        div { style: "margin-bottom: 16px;",
            Link {
                to: Route::TreeList {},
                class: "back-link",
                "← Back to Trees"
            }
        }

        // Delete confirmation dialog
        if confirm_delete() {
            div { class: "modal-backdrop",
                div { class: "modal-card",
                    h3 { "Delete Tree" }
                    p { style: "margin: 12px 0;",
                        "Are you sure you want to delete this tree and all its data? This action cannot be undone."
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

        // Tree header
        match &*tree_resource.read() {
            Some(Ok(tree)) => {
                let tree_name = tree.name.clone();
                let tree_desc = tree.description.clone().unwrap_or_default();
                rsx! {
                    if editing() {
                        // Edit form
                        div { class: "card", style: "margin-bottom: 24px;",
                            h2 { style: "margin-bottom: 16px; font-size: 1.1rem;", "Edit Tree" }

                            if let Some(err) = edit_error() {
                                div { class: "error-msg", "{err}" }
                            }

                            div { class: "form-group",
                                label { "Name" }
                                input {
                                    r#type: "text",
                                    value: "{edit_name}",
                                    oninput: move |e: Event<FormData>| edit_name.set(e.value()),
                                }
                            }
                            div { class: "form-group",
                                label { "Description (optional)" }
                                textarea {
                                    rows: 3,
                                    value: "{edit_desc}",
                                    oninput: move |e: Event<FormData>| edit_desc.set(e.value()),
                                }
                            }
                            div { style: "display: flex; gap: 8px;",
                                button { class: "btn btn-primary", onclick: on_save_edit, "Save" }
                                button {
                                    class: "btn btn-outline",
                                    onclick: move |_| {
                                        editing.set(false);
                                        edit_error.set(None);
                                    },
                                    "Cancel"
                                }
                            }
                        }
                    } else {
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
                                    onclick: move |_| {
                                        edit_name.set(tree_name.clone());
                                        edit_desc.set(tree_desc.clone());
                                        edit_error.set(None);
                                        editing.set(true);
                                    },
                                    "Edit"
                                }
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
            div { class: "section-header",
                h2 { style: "font-size: 1.1rem;", "Persons" }
                button {
                    class: "btn btn-primary btn-sm",
                    onclick: move |_| show_person_form.toggle(),
                    if show_person_form() { "Cancel" } else { "Add Person" }
                }
            }

            // Create person form
            if show_person_form() {
                div { style: "margin-bottom: 16px; padding: 16px; background: var(--color-bg); border-radius: var(--radius);",
                    h3 { style: "margin-bottom: 12px; font-size: 0.95rem;", "New Person" }

                    if let Some(err) = person_form_error() {
                        div { class: "error-msg", "{err}" }
                    }

                    div { class: "form-group",
                        label { "Sex" }
                        select {
                            value: "{new_person_sex}",
                            oninput: move |e: Event<FormData>| new_person_sex.set(e.value()),
                            option { value: "Unknown", "Unknown" }
                            option { value: "Male", "Male" }
                            option { value: "Female", "Female" }
                        }
                    }
                    button { class: "btn btn-primary btn-sm", onclick: on_create_person, "Create" }
                }
            }

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
