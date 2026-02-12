//! Tree detail page — shows tree info, persons, and families, with edit & delete.

use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::{AddChildBody, AddSpouseBody, ApiClient, CreatePersonBody, UpdateTreeBody};
use crate::router::Route;
use oxidgene_core::{ChildType, Sex, SpouseRole};

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

    // Create family state.
    let mut family_create_error = use_signal(|| None::<String>);

    // Delete family confirmation state.
    let mut confirm_delete_family_id = use_signal(|| None::<Uuid>);
    let mut delete_family_error = use_signal(|| None::<String>);

    // Add spouse form state: which family is adding a spouse.
    let mut adding_spouse_family_id = use_signal(|| None::<Uuid>);
    let mut spouse_person_id = use_signal(String::new);
    let mut spouse_role = use_signal(|| "Husband".to_string());
    let mut spouse_form_error = use_signal(|| None::<String>);

    // Add child form state: which family is adding a child.
    let mut adding_child_family_id = use_signal(|| None::<Uuid>);
    let mut child_person_id = use_signal(String::new);
    let mut child_type = use_signal(|| "Biological".to_string());
    let mut child_form_error = use_signal(|| None::<String>);

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

    // Fetch person names in the tree (to resolve person_id → display name).
    let api_names = api.clone();
    let all_names_resource = use_resource(move || {
        let api = api_names.clone();
        let _tick = refresh();
        let tid = tree_id_parsed;
        async move {
            let Some(tid) = tid else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: "Invalid tree ID".to_string(),
                });
            };
            // Load persons first, then load names for each person.
            let persons = api.list_persons(tid, Some(100), None).await?;
            let mut all_names = Vec::new();
            for edge in &persons.edges {
                if let Ok(names) = api.list_person_names(tid, edge.node.id).await {
                    all_names.extend(names);
                }
            }
            Ok(all_names)
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

    // Fetch all family spouses and children for display.
    let api_members = api.clone();
    let family_members_resource = use_resource(move || {
        let api = api_members.clone();
        let _tick = refresh();
        let tid = tree_id_parsed;
        async move {
            let Some(tid) = tid else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: "Invalid tree ID".to_string(),
                });
            };
            let families = api.list_families(tid, Some(100), None).await?;
            let mut all_spouses = Vec::new();
            let mut all_children = Vec::new();
            for edge in &families.edges {
                let fid = edge.node.id;
                if let Ok(spouses) = api.list_family_spouses(tid, fid).await {
                    all_spouses.extend(spouses);
                }
                if let Ok(children) = api.list_family_children(tid, fid).await {
                    all_children.extend(children);
                }
            }
            Ok((all_spouses, all_children))
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
    let api_person = api.clone();
    let on_create_person = move |_| {
        let api = api_person.clone();
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

    // Create family handler.
    let api_create_fam = api.clone();
    let on_create_family = move |_| {
        let api = api_create_fam.clone();
        let Some(tid) = tree_id_parsed else { return };
        spawn(async move {
            match api.create_family(tid).await {
                Ok(_) => {
                    family_create_error.set(None);
                    refresh += 1;
                }
                Err(e) => {
                    family_create_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    // Delete family handler.
    let api_del_fam = api.clone();
    let on_confirm_delete_family = move |_| {
        let api = api_del_fam.clone();
        let Some(tid) = tree_id_parsed else { return };
        let Some(fid) = confirm_delete_family_id() else {
            return;
        };
        spawn(async move {
            match api.delete_family(tid, fid).await {
                Ok(_) => {
                    confirm_delete_family_id.set(None);
                    delete_family_error.set(None);
                    refresh += 1;
                }
                Err(e) => {
                    delete_family_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    // Add spouse handler.
    let api_add_spouse = api.clone();
    let on_add_spouse = move |_| {
        let api = api_add_spouse.clone();
        let Some(tid) = tree_id_parsed else { return };
        let Some(fid) = adding_spouse_family_id() else {
            return;
        };
        let pid_str = spouse_person_id();
        let role_str = spouse_role();
        spawn(async move {
            let Ok(pid) = pid_str.parse::<Uuid>() else {
                spouse_form_error.set(Some("Please select a person".to_string()));
                return;
            };
            let role = parse_spouse_role(&role_str);
            let body = AddSpouseBody {
                person_id: pid,
                role,
                sort_order: 0,
            };
            match api.add_spouse(tid, fid, &body).await {
                Ok(_) => {
                    adding_spouse_family_id.set(None);
                    spouse_person_id.set(String::new());
                    spouse_role.set("Husband".to_string());
                    spouse_form_error.set(None);
                    refresh += 1;
                }
                Err(e) => {
                    spouse_form_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    // Remove spouse handler.
    let api_rm_spouse = api.clone();

    // Add child handler.
    let api_add_child = api.clone();
    let on_add_child = move |_| {
        let api = api_add_child.clone();
        let Some(tid) = tree_id_parsed else { return };
        let Some(fid) = adding_child_family_id() else {
            return;
        };
        let pid_str = child_person_id();
        let ct_str = child_type();
        spawn(async move {
            let Ok(pid) = pid_str.parse::<Uuid>() else {
                child_form_error.set(Some("Please select a person".to_string()));
                return;
            };
            let ct = parse_child_type(&ct_str);
            let body = AddChildBody {
                person_id: pid,
                child_type: ct,
                sort_order: 0,
            };
            match api.add_child(tid, fid, &body).await {
                Ok(_) => {
                    adding_child_family_id.set(None);
                    child_person_id.set(String::new());
                    child_type.set("Biological".to_string());
                    child_form_error.set(None);
                    refresh += 1;
                }
                Err(e) => {
                    child_form_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    // Remove child handler.
    let api_rm_child = api.clone();

    // Helper: resolve person_id to display name from loaded names.
    let person_display_name = |pid: Uuid| -> String {
        let names_data = all_names_resource.read();
        match &*names_data {
            Some(Ok(names)) => {
                let person_names: Vec<_> = names.iter().filter(|n| n.person_id == pid).collect();
                let primary = person_names
                    .iter()
                    .find(|n| n.is_primary)
                    .or(person_names.first());
                match primary {
                    Some(name) => {
                        let dn = name.display_name();
                        if dn.is_empty() {
                            pid.to_string()[..8].to_string()
                        } else {
                            dn
                        }
                    }
                    None => pid.to_string()[..8].to_string(),
                }
            }
            _ => pid.to_string()[..8].to_string(),
        }
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

        // Delete tree confirmation dialog
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

        // Delete family confirmation dialog
        if confirm_delete_family_id().is_some() {
            div { class: "modal-backdrop",
                div { class: "modal-card",
                    h3 { "Delete Family" }
                    p { style: "margin: 12px 0;",
                        "Are you sure you want to delete this family? All spouse and child links will be removed."
                    }
                    if let Some(err) = delete_family_error() {
                        div { class: "error-msg", "{err}" }
                    }
                    div { class: "modal-actions",
                        button {
                            class: "btn btn-outline",
                            onclick: move |_| {
                                confirm_delete_family_id.set(None);
                                delete_family_error.set(None);
                            },
                            "Cancel"
                        }
                        button {
                            class: "btn btn-danger",
                            onclick: on_confirm_delete_family,
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
            div { class: "section-header",
                h2 { style: "font-size: 1.1rem;", "Families" }
                button {
                    class: "btn btn-primary btn-sm",
                    onclick: on_create_family,
                    "Create Family"
                }
            }

            if let Some(err) = family_create_error() {
                div { class: "error-msg", "{err}" }
            }

            match &*families_resource.read() {
                Some(Ok(conn)) => rsx! {
                    if conn.edges.is_empty() {
                        div { class: "empty-state",
                            p { "No families in this tree yet." }
                        }
                    } else {
                        for edge in conn.edges.iter() {
                            {
                                let family = &edge.node;
                                let fid = family.id;
                                let fid_short = family.id.to_string().chars().take(8).collect::<String>();

                                // Get spouses and children for this family.
                                let (family_spouses, family_children) = {
                                    let members = family_members_resource.read();
                                    match &*members {
                                        Some(Ok((spouses, children))) => {
                                            let fs: Vec<_> = spouses.iter().filter(|s| s.family_id == fid).cloned().collect();
                                            let fc: Vec<_> = children.iter().filter(|c| c.family_id == fid).cloned().collect();
                                            (fs, fc)
                                        }
                                        _ => (vec![], vec![]),
                                    }
                                };

                                // Build persons list for person pickers.
                                let persons_for_picker = {
                                    let persons = persons_resource.read();
                                    match &*persons {
                                        Some(Ok(conn)) => conn.edges.iter().map(|e| (e.node.id, person_display_name(e.node.id))).collect::<Vec<_>>(),
                                        _ => vec![],
                                    }
                                };
                                let persons_for_picker2 = persons_for_picker.clone();

                                let is_adding_spouse = adding_spouse_family_id() == Some(fid);
                                let is_adding_child = adding_child_family_id() == Some(fid);

                                rsx! {
                                    div {
                                        class: "card",
                                        style: "margin-bottom: 16px; padding: 16px;",

                                        // Family header
                                        div { class: "section-header",
                                            h3 { style: "font-size: 0.95rem;",
                                                "Family "
                                                span { class: "text-muted", "{fid_short}..." }
                                            }
                                            button {
                                                class: "btn btn-danger btn-sm",
                                                onclick: move |_| {
                                                    confirm_delete_family_id.set(Some(fid));
                                                    delete_family_error.set(None);
                                                },
                                                "Delete"
                                            }
                                        }

                                        // Spouses subsection
                                        div { style: "margin-bottom: 12px;",
                                            div { style: "display: flex; align-items: center; gap: 8px; margin-bottom: 8px;",
                                                h4 { style: "font-size: 0.85rem; font-weight: 600; color: var(--color-text-muted);", "SPOUSES" }
                                                button {
                                                    class: "btn btn-outline btn-sm",
                                                    onclick: move |_| {
                                                        if is_adding_spouse {
                                                            adding_spouse_family_id.set(None);
                                                            spouse_form_error.set(None);
                                                        } else {
                                                            adding_spouse_family_id.set(Some(fid));
                                                            spouse_person_id.set(String::new());
                                                            spouse_role.set("Husband".to_string());
                                                            spouse_form_error.set(None);
                                                        }
                                                    },
                                                    if is_adding_spouse { "Cancel" } else { "Add" }
                                                }
                                            }

                                            // Add spouse form
                                            if is_adding_spouse {
                                                div { style: "margin-bottom: 8px; padding: 12px; background: var(--color-bg); border-radius: var(--radius);",
                                                    if let Some(err) = spouse_form_error() {
                                                        div { class: "error-msg", "{err}" }
                                                    }
                                                    div { class: "form-row",
                                                        div { class: "form-group",
                                                            label { "Person" }
                                                            select {
                                                                value: "{spouse_person_id}",
                                                                oninput: move |e: Event<FormData>| spouse_person_id.set(e.value()),
                                                                option { value: "", "-- Select --" }
                                                                for (pid, name) in persons_for_picker.iter() {
                                                                    option {
                                                                        value: "{pid}",
                                                                        "{name}"
                                                                    }
                                                                }
                                                            }
                                                        }
                                                        div { class: "form-group",
                                                            label { "Role" }
                                                            select {
                                                                value: "{spouse_role}",
                                                                oninput: move |e: Event<FormData>| spouse_role.set(e.value()),
                                                                option { value: "Husband", "Husband" }
                                                                option { value: "Wife", "Wife" }
                                                                option { value: "Partner", "Partner" }
                                                            }
                                                        }
                                                    }
                                                    button {
                                                        class: "btn btn-primary btn-sm",
                                                        onclick: on_add_spouse.clone(),
                                                        "Add Spouse"
                                                    }
                                                }
                                            }

                                            if family_spouses.is_empty() {
                                                p { class: "text-muted", style: "font-size: 0.85rem;", "No spouses." }
                                            } else {
                                                for spouse in family_spouses.iter() {
                                                    {
                                                        let sid = spouse.id;
                                                        let sp_name = person_display_name(spouse.person_id);
                                                        let sp_role = format!("{:?}", spouse.role);
                                                        let sp_pid = spouse.person_id.to_string();
                                                        let sp_tid = tree_id.clone();
                                                        let api_rm = api_rm_spouse.clone();
                                                        rsx! {
                                                            div { style: "display: flex; align-items: center; gap: 8px; margin-bottom: 4px;",
                                                                Link {
                                                                    to: Route::PersonDetail {
                                                                        tree_id: sp_tid,
                                                                        person_id: sp_pid,
                                                                    },
                                                                    style: "font-size: 0.9rem;",
                                                                    "{sp_name}"
                                                                }
                                                                span { class: "badge", "{sp_role}" }
                                                                button {
                                                                    class: "btn btn-danger btn-sm",
                                                                    onclick: move |_| {
                                                                        let api = api_rm.clone();
                                                                        let Some(tid) = tree_id_parsed else { return };
                                                                        spawn(async move {
                                                                            match api.remove_spouse(tid, fid, sid).await {
                                                                                Ok(_) => { refresh += 1; }
                                                                                Err(e) => {
                                                                                    spouse_form_error.set(Some(format!("{e}")));
                                                                                }
                                                                            }
                                                                        });
                                                                    },
                                                                    "Remove"
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }

                                        // Children subsection
                                        div {
                                            div { style: "display: flex; align-items: center; gap: 8px; margin-bottom: 8px;",
                                                h4 { style: "font-size: 0.85rem; font-weight: 600; color: var(--color-text-muted);", "CHILDREN" }
                                                button {
                                                    class: "btn btn-outline btn-sm",
                                                    onclick: move |_| {
                                                        if is_adding_child {
                                                            adding_child_family_id.set(None);
                                                            child_form_error.set(None);
                                                        } else {
                                                            adding_child_family_id.set(Some(fid));
                                                            child_person_id.set(String::new());
                                                            child_type.set("Biological".to_string());
                                                            child_form_error.set(None);
                                                        }
                                                    },
                                                    if is_adding_child { "Cancel" } else { "Add" }
                                                }
                                            }

                                            // Add child form
                                            if is_adding_child {
                                                div { style: "margin-bottom: 8px; padding: 12px; background: var(--color-bg); border-radius: var(--radius);",
                                                    if let Some(err) = child_form_error() {
                                                        div { class: "error-msg", "{err}" }
                                                    }
                                                    div { class: "form-row",
                                                        div { class: "form-group",
                                                            label { "Person" }
                                                            select {
                                                                value: "{child_person_id}",
                                                                oninput: move |e: Event<FormData>| child_person_id.set(e.value()),
                                                                option { value: "", "-- Select --" }
                                                                for (pid, name) in persons_for_picker2.iter() {
                                                                    option {
                                                                        value: "{pid}",
                                                                        "{name}"
                                                                    }
                                                                }
                                                            }
                                                        }
                                                        div { class: "form-group",
                                                            label { "Child Type" }
                                                            select {
                                                                value: "{child_type}",
                                                                oninput: move |e: Event<FormData>| child_type.set(e.value()),
                                                                option { value: "Biological", "Biological" }
                                                                option { value: "Adopted", "Adopted" }
                                                                option { value: "Foster", "Foster" }
                                                                option { value: "Step", "Step" }
                                                                option { value: "Unknown", "Unknown" }
                                                            }
                                                        }
                                                    }
                                                    button {
                                                        class: "btn btn-primary btn-sm",
                                                        onclick: on_add_child.clone(),
                                                        "Add Child"
                                                    }
                                                }
                                            }

                                            if family_children.is_empty() {
                                                p { class: "text-muted", style: "font-size: 0.85rem;", "No children." }
                                            } else {
                                                for child in family_children.iter() {
                                                    {
                                                        let cid = child.id;
                                                        let ch_name = person_display_name(child.person_id);
                                                        let ch_type = format!("{:?}", child.child_type);
                                                        let ch_pid = child.person_id.to_string();
                                                        let ch_tid = tree_id.clone();
                                                        let api_rm = api_rm_child.clone();
                                                        rsx! {
                                                            div { style: "display: flex; align-items: center; gap: 8px; margin-bottom: 4px;",
                                                                Link {
                                                                    to: Route::PersonDetail {
                                                                        tree_id: ch_tid,
                                                                        person_id: ch_pid,
                                                                    },
                                                                    style: "font-size: 0.9rem;",
                                                                    "{ch_name}"
                                                                }
                                                                span { class: "badge", "{ch_type}" }
                                                                button {
                                                                    class: "btn btn-danger btn-sm",
                                                                    onclick: move |_| {
                                                                        let api = api_rm.clone();
                                                                        let Some(tid) = tree_id_parsed else { return };
                                                                        spawn(async move {
                                                                            match api.remove_child(tid, fid, cid).await {
                                                                                Ok(_) => { refresh += 1; }
                                                                                Err(e) => {
                                                                                    child_form_error.set(Some(format!("{e}")));
                                                                                }
                                                                            }
                                                                        });
                                                                    },
                                                                    "Remove"
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

// ── Helper functions ─────────────────────────────────────────────────

fn parse_spouse_role(s: &str) -> SpouseRole {
    match s {
        "Husband" => SpouseRole::Husband,
        "Wife" => SpouseRole::Wife,
        "Partner" => SpouseRole::Partner,
        _ => SpouseRole::Partner,
    }
}

fn parse_child_type(s: &str) -> ChildType {
    match s {
        "Biological" => ChildType::Biological,
        "Adopted" => ChildType::Adopted,
        "Foster" => ChildType::Foster,
        "Step" => ChildType::Step,
        _ => ChildType::Unknown,
    }
}
