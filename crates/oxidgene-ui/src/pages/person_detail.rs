//! Person detail page — shows names, events, and related data with edit/delete.

use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::{ApiClient, CreatePersonNameBody, UpdatePersonBody, UpdatePersonNameBody};
use crate::router::Route;
use oxidgene_core::NameType;

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

    // Edit person sex state.
    let mut editing_sex = use_signal(|| false);
    let mut edit_sex_val = use_signal(|| "Unknown".to_string());
    let mut edit_sex_error = use_signal(|| None::<String>);

    // Add name form state.
    let mut show_name_form = use_signal(|| false);
    let mut name_form_type = use_signal(|| "Birth".to_string());
    let mut name_form_given = use_signal(String::new);
    let mut name_form_surname = use_signal(String::new);
    let mut name_form_prefix = use_signal(String::new);
    let mut name_form_suffix = use_signal(String::new);
    let mut name_form_nickname = use_signal(String::new);
    let mut name_form_primary = use_signal(|| true);
    let mut name_form_error = use_signal(|| None::<String>);

    // Edit name state: which name is being edited.
    let mut editing_name_id = use_signal(|| None::<Uuid>);
    let mut edit_name_type = use_signal(|| "Birth".to_string());
    let mut edit_name_given = use_signal(String::new);
    let mut edit_name_surname = use_signal(String::new);
    let mut edit_name_prefix = use_signal(String::new);
    let mut edit_name_suffix = use_signal(String::new);
    let mut edit_name_nickname = use_signal(String::new);
    let mut edit_name_primary = use_signal(|| false);
    let mut edit_name_error = use_signal(|| None::<String>);

    // Delete name confirmation.
    let mut confirm_delete_name_id = use_signal(|| None::<Uuid>);
    let mut delete_name_error = use_signal(|| None::<String>);

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

    // Delete person handler.
    let tree_id_nav = tree_id.clone();
    let api_del = api.clone();
    let on_confirm_delete = move |_| {
        let api = api_del.clone();
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

    // Save sex handler.
    let api_sex = api.clone();
    let on_save_sex = move |_| {
        let api = api_sex.clone();
        let Some(tid) = tree_id_parsed else { return };
        let Some(pid) = person_id_parsed else { return };
        let sex_str = edit_sex_val();
        spawn(async move {
            let sex = parse_sex(&sex_str);
            let body = UpdatePersonBody { sex: Some(sex) };
            match api.update_person(tid, pid, &body).await {
                Ok(_) => {
                    editing_sex.set(false);
                    edit_sex_error.set(None);
                    refresh += 1;
                }
                Err(e) => {
                    edit_sex_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    // Create name handler.
    let api_create_name = api.clone();
    let on_create_name = move |_| {
        let api = api_create_name.clone();
        let Some(tid) = tree_id_parsed else { return };
        let Some(pid) = person_id_parsed else { return };
        let given = name_form_given().trim().to_string();
        let surname = name_form_surname().trim().to_string();
        let prefix = name_form_prefix().trim().to_string();
        let suffix = name_form_suffix().trim().to_string();
        let nickname = name_form_nickname().trim().to_string();
        let name_type_str = name_form_type();
        let is_primary = name_form_primary();
        spawn(async move {
            if given.is_empty() && surname.is_empty() {
                name_form_error.set(Some("Given names or surname is required".to_string()));
                return;
            }
            let body = CreatePersonNameBody {
                name_type: parse_name_type(&name_type_str),
                given_names: opt_str(&given),
                surname: opt_str(&surname),
                prefix: opt_str(&prefix),
                suffix: opt_str(&suffix),
                nickname: opt_str(&nickname),
                is_primary,
            };
            match api.create_person_name(tid, pid, &body).await {
                Ok(_) => {
                    show_name_form.set(false);
                    name_form_given.set(String::new());
                    name_form_surname.set(String::new());
                    name_form_prefix.set(String::new());
                    name_form_suffix.set(String::new());
                    name_form_nickname.set(String::new());
                    name_form_type.set("Birth".to_string());
                    name_form_primary.set(true);
                    name_form_error.set(None);
                    refresh += 1;
                }
                Err(e) => {
                    name_form_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    // Save name edit handler.
    let api_edit_name = api.clone();
    let on_save_name_edit = move |_| {
        let api = api_edit_name.clone();
        let Some(tid) = tree_id_parsed else { return };
        let Some(pid) = person_id_parsed else { return };
        let Some(nid) = editing_name_id() else { return };
        let given = edit_name_given().trim().to_string();
        let surname = edit_name_surname().trim().to_string();
        let prefix = edit_name_prefix().trim().to_string();
        let suffix = edit_name_suffix().trim().to_string();
        let nickname = edit_name_nickname().trim().to_string();
        let name_type_str = edit_name_type();
        let is_primary = edit_name_primary();
        spawn(async move {
            let body = UpdatePersonNameBody {
                name_type: Some(parse_name_type(&name_type_str)),
                given_names: Some(opt_str(&given)),
                surname: Some(opt_str(&surname)),
                prefix: Some(opt_str(&prefix)),
                suffix: Some(opt_str(&suffix)),
                nickname: Some(opt_str(&nickname)),
                is_primary: Some(is_primary),
            };
            match api.update_person_name(tid, pid, nid, &body).await {
                Ok(_) => {
                    editing_name_id.set(None);
                    edit_name_error.set(None);
                    refresh += 1;
                }
                Err(e) => {
                    edit_name_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    // Delete name handler.
    let on_confirm_delete_name = move |_| {
        let api = api.clone();
        let Some(tid) = tree_id_parsed else { return };
        let Some(pid) = person_id_parsed else { return };
        let Some(nid) = confirm_delete_name_id() else {
            return;
        };
        spawn(async move {
            match api.delete_person_name(tid, pid, nid).await {
                Ok(_) => {
                    confirm_delete_name_id.set(None);
                    delete_name_error.set(None);
                    refresh += 1;
                }
                Err(e) => {
                    delete_name_error.set(Some(format!("{e}")));
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

        // Delete person confirmation dialog
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

        // Delete name confirmation dialog
        if confirm_delete_name_id().is_some() {
            div { class: "modal-backdrop",
                div { class: "modal-card",
                    h3 { "Delete Name" }
                    p { style: "margin: 12px 0;",
                        "Are you sure you want to delete this name?"
                    }
                    if let Some(err) = delete_name_error() {
                        div { class: "error-msg", "{err}" }
                    }
                    div { class: "modal-actions",
                        button {
                            class: "btn btn-outline",
                            onclick: move |_| {
                                confirm_delete_name_id.set(None);
                                delete_name_error.set(None);
                            },
                            "Cancel"
                        }
                        button {
                            class: "btn btn-danger",
                            onclick: on_confirm_delete_name,
                            "Delete"
                        }
                    }
                }
            }
        }

        // Person header
        match &*person_resource.read() {
            Some(Ok(person)) => {
                let sex_str = format!("{:?}", person.sex);
                rsx! {
                    div { class: "page-header",
                        div {
                            h1 { "{display_name}" }
                            div { style: "display: flex; align-items: center; gap: 8px; margin-top: 4px;",
                                if editing_sex() {
                                    select {
                                        value: "{edit_sex_val}",
                                        oninput: move |e: Event<FormData>| edit_sex_val.set(e.value()),
                                        option { value: "Unknown", "Unknown" }
                                        option { value: "Male", "Male" }
                                        option { value: "Female", "Female" }
                                    }
                                    button {
                                        class: "btn btn-primary btn-sm",
                                        onclick: on_save_sex,
                                        "Save"
                                    }
                                    button {
                                        class: "btn btn-outline btn-sm",
                                        onclick: move |_| {
                                            editing_sex.set(false);
                                            edit_sex_error.set(None);
                                        },
                                        "Cancel"
                                    }
                                    if let Some(err) = edit_sex_error() {
                                        span { class: "error-msg", style: "margin: 0; padding: 4px 8px; font-size: 0.8rem;", "{err}" }
                                    }
                                } else {
                                    span { class: "badge", "{sex_str}" }
                                    button {
                                        class: "btn btn-outline btn-sm",
                                        onclick: move |_| {
                                            edit_sex_val.set(sex_str.clone());
                                            editing_sex.set(true);
                                        },
                                        "Edit Sex"
                                    }
                                }
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
            div { class: "section-header",
                h2 { style: "font-size: 1.1rem;", "Names" }
                button {
                    class: "btn btn-primary btn-sm",
                    onclick: move |_| show_name_form.toggle(),
                    if show_name_form() { "Cancel" } else { "Add Name" }
                }
            }

            // Add name form
            if show_name_form() {
                div { style: "margin-bottom: 16px; padding: 16px; background: var(--color-bg); border-radius: var(--radius);",
                    h3 { style: "margin-bottom: 12px; font-size: 0.95rem;", "New Name" }

                    if let Some(err) = name_form_error() {
                        div { class: "error-msg", "{err}" }
                    }

                    div { class: "form-row",
                        div { class: "form-group",
                            label { "Name Type" }
                            select {
                                value: "{name_form_type}",
                                oninput: move |e: Event<FormData>| name_form_type.set(e.value()),
                                option { value: "Birth", "Birth" }
                                option { value: "Married", "Married" }
                                option { value: "AlsoKnownAs", "Also Known As" }
                                option { value: "Maiden", "Maiden" }
                                option { value: "Religious", "Religious" }
                                option { value: "Other", "Other" }
                            }
                        }
                        div { class: "form-group",
                            label { "Primary" }
                            select {
                                value: if name_form_primary() { "true" } else { "false" },
                                oninput: move |e: Event<FormData>| name_form_primary.set(e.value() == "true"),
                                option { value: "true", "Yes" }
                                option { value: "false", "No" }
                            }
                        }
                    }
                    div { class: "form-row",
                        div { class: "form-group",
                            label { "Given Names" }
                            input {
                                r#type: "text",
                                placeholder: "e.g. Jean-Pierre",
                                value: "{name_form_given}",
                                oninput: move |e: Event<FormData>| name_form_given.set(e.value()),
                            }
                        }
                        div { class: "form-group",
                            label { "Surname" }
                            input {
                                r#type: "text",
                                placeholder: "e.g. Dupont",
                                value: "{name_form_surname}",
                                oninput: move |e: Event<FormData>| name_form_surname.set(e.value()),
                            }
                        }
                    }
                    div { class: "form-row",
                        div { class: "form-group",
                            label { "Prefix" }
                            input {
                                r#type: "text",
                                placeholder: "e.g. Dr.",
                                value: "{name_form_prefix}",
                                oninput: move |e: Event<FormData>| name_form_prefix.set(e.value()),
                            }
                        }
                        div { class: "form-group",
                            label { "Suffix" }
                            input {
                                r#type: "text",
                                placeholder: "e.g. Jr.",
                                value: "{name_form_suffix}",
                                oninput: move |e: Event<FormData>| name_form_suffix.set(e.value()),
                            }
                        }
                        div { class: "form-group",
                            label { "Nickname" }
                            input {
                                r#type: "text",
                                placeholder: "e.g. JP",
                                value: "{name_form_nickname}",
                                oninput: move |e: Event<FormData>| name_form_nickname.set(e.value()),
                            }
                        }
                    }
                    button { class: "btn btn-primary btn-sm", onclick: on_create_name, "Create Name" }
                }
            }

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
                                        th { style: "width: 140px;", "Actions" }
                                    }
                                }
                                tbody {
                                    for name in names.iter() {
                                        {
                                            let nid = name.id;
                                            let is_editing = editing_name_id() == Some(nid);
                                            let nt = format!("{:?}", name.name_type);
                                            let gn = name.given_names.clone().unwrap_or_default();
                                            let sn = name.surname.clone().unwrap_or_default();
                                            let pfx = name.prefix.clone().unwrap_or_default();
                                            let sfx = name.suffix.clone().unwrap_or_default();
                                            let nick = name.nickname.clone().unwrap_or_default();
                                            let prim = name.is_primary;
                                            if is_editing {
                                                rsx! {
                                                    tr {
                                                        td { colspan: 5,
                                                            div { style: "padding: 8px; background: var(--color-bg); border-radius: var(--radius);",
                                                                if let Some(err) = edit_name_error() {
                                                                    div { class: "error-msg", "{err}" }
                                                                }
                                                                div { class: "form-row",
                                                                    div { class: "form-group",
                                                                        label { "Name Type" }
                                                                        select {
                                                                            value: "{edit_name_type}",
                                                                            oninput: move |e: Event<FormData>| edit_name_type.set(e.value()),
                                                                            option { value: "Birth", "Birth" }
                                                                            option { value: "Married", "Married" }
                                                                            option { value: "AlsoKnownAs", "Also Known As" }
                                                                            option { value: "Maiden", "Maiden" }
                                                                            option { value: "Religious", "Religious" }
                                                                            option { value: "Other", "Other" }
                                                                        }
                                                                    }
                                                                    div { class: "form-group",
                                                                        label { "Primary" }
                                                                        select {
                                                                            value: if edit_name_primary() { "true" } else { "false" },
                                                                            oninput: move |e: Event<FormData>| edit_name_primary.set(e.value() == "true"),
                                                                            option { value: "true", "Yes" }
                                                                            option { value: "false", "No" }
                                                                        }
                                                                    }
                                                                }
                                                                div { class: "form-row",
                                                                    div { class: "form-group",
                                                                        label { "Given Names" }
                                                                        input {
                                                                            r#type: "text",
                                                                            value: "{edit_name_given}",
                                                                            oninput: move |e: Event<FormData>| edit_name_given.set(e.value()),
                                                                        }
                                                                    }
                                                                    div { class: "form-group",
                                                                        label { "Surname" }
                                                                        input {
                                                                            r#type: "text",
                                                                            value: "{edit_name_surname}",
                                                                            oninput: move |e: Event<FormData>| edit_name_surname.set(e.value()),
                                                                        }
                                                                    }
                                                                }
                                                                div { class: "form-row",
                                                                    div { class: "form-group",
                                                                        label { "Prefix" }
                                                                        input {
                                                                            r#type: "text",
                                                                            value: "{edit_name_prefix}",
                                                                            oninput: move |e: Event<FormData>| edit_name_prefix.set(e.value()),
                                                                        }
                                                                    }
                                                                    div { class: "form-group",
                                                                        label { "Suffix" }
                                                                        input {
                                                                            r#type: "text",
                                                                            value: "{edit_name_suffix}",
                                                                            oninput: move |e: Event<FormData>| edit_name_suffix.set(e.value()),
                                                                        }
                                                                    }
                                                                    div { class: "form-group",
                                                                        label { "Nickname" }
                                                                        input {
                                                                            r#type: "text",
                                                                            value: "{edit_name_nickname}",
                                                                            oninput: move |e: Event<FormData>| edit_name_nickname.set(e.value()),
                                                                        }
                                                                    }
                                                                }
                                                                div { style: "display: flex; gap: 8px;",
                                                                    button {
                                                                        class: "btn btn-primary btn-sm",
                                                                        onclick: on_save_name_edit.clone(),
                                                                        "Save"
                                                                    }
                                                                    button {
                                                                        class: "btn btn-outline btn-sm",
                                                                        onclick: move |_| {
                                                                            editing_name_id.set(None);
                                                                            edit_name_error.set(None);
                                                                        },
                                                                        "Cancel"
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            } else {
                                                rsx! {
                                                    tr {
                                                        td {
                                                            span { class: "badge", {format!("{:?}", name.name_type)} }
                                                        }
                                                        td { {name.given_names.as_deref().unwrap_or("--")} }
                                                        td { {name.surname.as_deref().unwrap_or("--")} }
                                                        td {
                                                            if name.is_primary { "Yes" } else { "No" }
                                                        }
                                                        td {
                                                            div { style: "display: flex; gap: 4px;",
                                                                button {
                                                                    class: "btn btn-outline btn-sm",
                                                                    onclick: move |_| {
                                                                        editing_name_id.set(Some(nid));
                                                                        edit_name_type.set(nt.clone());
                                                                        edit_name_given.set(gn.clone());
                                                                        edit_name_surname.set(sn.clone());
                                                                        edit_name_prefix.set(pfx.clone());
                                                                        edit_name_suffix.set(sfx.clone());
                                                                        edit_name_nickname.set(nick.clone());
                                                                        edit_name_primary.set(prim);
                                                                        edit_name_error.set(None);
                                                                    },
                                                                    "Edit"
                                                                }
                                                                button {
                                                                    class: "btn btn-danger btn-sm",
                                                                    onclick: move |_| {
                                                                        confirm_delete_name_id.set(Some(nid));
                                                                        delete_name_error.set(None);
                                                                    },
                                                                    "Delete"
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

// ── Helper functions ─────────────────────────────────────────────────

fn parse_sex(s: &str) -> oxidgene_core::Sex {
    match s {
        "Male" => oxidgene_core::Sex::Male,
        "Female" => oxidgene_core::Sex::Female,
        _ => oxidgene_core::Sex::Unknown,
    }
}

fn parse_name_type(s: &str) -> NameType {
    match s {
        "Birth" => NameType::Birth,
        "Married" => NameType::Married,
        "AlsoKnownAs" => NameType::AlsoKnownAs,
        "Maiden" => NameType::Maiden,
        "Religious" => NameType::Religious,
        _ => NameType::Other,
    }
}

fn opt_str(s: &str) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}
