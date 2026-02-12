//! Tree detail page — Geneanet-style pedigree chart view.
//!
//! Shows the tree header with edit/delete, a root-person selector, the
//! [`PedigreeChart`] as the main view, a context menu for person actions,
//! and a collapsible GEDCOM import/export section.

use std::collections::HashMap;

use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::{ApiClient, ImportGedcomBody, UpdateTreeBody};
use crate::components::confirm_dialog::ConfirmDialog;
use crate::components::context_menu::{ContextMenu, PersonAction};
use crate::components::pedigree_chart::{PedigreeChart, PedigreeData};
use crate::router::Route;
use crate::utils::resolve_name;

/// Page rendered at `/trees/:tree_id`.
#[component]
pub fn TreeDetail(tree_id: String) -> Element {
    let api = use_context::<ApiClient>();
    let nav = use_navigator();
    let mut refresh = use_signal(|| 0u32);

    let tree_id_parsed = tree_id.parse::<Uuid>().ok();

    // ── Tree edit state ──
    let mut editing = use_signal(|| false);
    let mut edit_name = use_signal(String::new);
    let mut edit_desc = use_signal(String::new);
    let mut edit_error = use_signal(|| None::<String>);

    // ── Delete tree confirmation ──
    let mut confirm_delete = use_signal(|| false);
    let mut delete_error = use_signal(|| None::<String>);

    // ── Root person selector ──
    let mut selected_root = use_signal(|| None::<Uuid>);

    // ── Context menu state ──
    let mut context_menu_person = use_signal(|| None::<(Uuid, f64, f64)>);

    // ── Delete person confirmation ──
    let mut confirm_delete_person_id = use_signal(|| None::<Uuid>);
    let mut delete_person_error = use_signal(|| None::<String>);

    // ── GEDCOM import/export state ──
    let mut show_gedcom = use_signal(|| false);
    let mut show_import_form = use_signal(|| false);
    let mut import_text = use_signal(String::new);
    let mut import_error = use_signal(|| None::<String>);
    let mut import_result = use_signal(|| None::<crate::api::ImportGedcomResult>);
    let mut importing = use_signal(|| false);
    let mut export_text = use_signal(|| None::<String>);
    let mut export_error = use_signal(|| None::<String>);
    let mut export_warnings = use_signal(Vec::<String>::new);
    let mut exporting = use_signal(|| false);

    // ── Fetch tree details ──
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

    // ── Fetch persons ──
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
            api.list_persons(tid, Some(500), None).await
        }
    });

    // ── Fetch all person names ──
    let api_names = api.clone();
    let names_resource = use_resource(move || {
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
            let persons = api.list_persons(tid, Some(500), None).await?;
            let mut name_map: HashMap<Uuid, Vec<oxidgene_core::types::PersonName>> = HashMap::new();
            for edge in &persons.edges {
                if let Ok(names) = api.list_person_names(tid, edge.node.id).await {
                    name_map.insert(edge.node.id, names);
                }
            }
            Ok(name_map)
        }
    });

    // ── Fetch families ──
    let api_families = api.clone();
    let _families_resource = use_resource(move || {
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
            api.list_families(tid, Some(500), None).await
        }
    });

    // ── Fetch all family spouses and children ──
    let api_members = api.clone();
    let members_resource = use_resource(move || {
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
            let families = api.list_families(tid, Some(500), None).await?;
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

    // ── Build pedigree data ──
    let pedigree_data: Option<PedigreeData> = {
        let persons_data = persons_resource.read();
        let names_data = names_resource.read();
        let members_data = members_resource.read();

        match (&*persons_data, &*names_data, &*members_data) {
            (Some(Ok(conn)), Some(Ok(name_map)), Some(Ok((spouses, children)))) => {
                let persons: Vec<_> = conn.edges.iter().map(|e| e.node.clone()).collect();
                Some(PedigreeData::build(
                    &persons,
                    name_map.clone(),
                    spouses,
                    children,
                ))
            }
            _ => None,
        }
    };

    // Determine root person: selected or first person.
    let root_person_id: Option<Uuid> = {
        if let Some(sel) = selected_root() {
            Some(sel)
        } else {
            let persons_data = persons_resource.read();
            match &*persons_data {
                Some(Ok(conn)) => conn.edges.first().map(|e| e.node.id),
                _ => None,
            }
        }
    };

    // Person list for root selector dropdown.
    let person_options: Vec<(Uuid, String)> = {
        let persons_data = persons_resource.read();
        let names_data = names_resource.read();
        match (&*persons_data, &*names_data) {
            (Some(Ok(conn)), Some(Ok(name_map))) => conn
                .edges
                .iter()
                .map(|e| (e.node.id, resolve_name(e.node.id, name_map)))
                .collect(),
            _ => vec![],
        }
    };

    // Context menu person name.
    let ctx_person_name: String = {
        let ctx = context_menu_person();
        match ctx {
            Some((pid, _, _)) => {
                let names_data = names_resource.read();
                match &*names_data {
                    Some(Ok(name_map)) => resolve_name(pid, name_map),
                    _ => "Unknown".to_string(),
                }
            }
            None => String::new(),
        }
    };

    // ── Handlers ──

    // Save tree edit.
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
                Err(e) => edit_error.set(Some(format!("{e}"))),
            }
        });
    };

    // Confirm delete tree.
    let api_del = api.clone();
    let on_confirm_delete = move |_| {
        let api = api_del.clone();
        let Some(tid) = tree_id_parsed else { return };
        spawn(async move {
            match api.delete_tree(tid).await {
                Ok(_) => {
                    nav.push(Route::TreeList {});
                }
                Err(e) => delete_error.set(Some(format!("{e}"))),
            }
        });
    };

    // Context menu action handler.
    let api_ctx = api.clone();
    let tree_id_ctx = tree_id.clone();
    let pedigree_data_ctx = pedigree_data.clone();
    let on_context_action = move |action: PersonAction| {
        let Some((pid, _, _)) = context_menu_person() else {
            return;
        };
        context_menu_person.set(None);

        match action {
            PersonAction::Edit => {
                // Navigate to person detail page for editing.
                nav.push(Route::PersonDetail {
                    tree_id: tree_id_ctx.clone(),
                    person_id: pid.to_string(),
                });
            }
            PersonAction::AddParents => {
                // Create a new family with this person as child.
                let api = api_ctx.clone();
                let Some(tid) = tree_id_parsed else { return };
                spawn(async move {
                    match api.create_family(tid).await {
                        Ok(family) => {
                            let body = crate::api::AddChildBody {
                                person_id: pid,
                                child_type: oxidgene_core::ChildType::Biological,
                                sort_order: 0,
                            };
                            let _ = api.add_child(tid, family.id, &body).await;
                            refresh += 1;
                        }
                        Err(_) => { /* silently fail for now */ }
                    }
                });
            }
            PersonAction::AddSpouse => {
                // Create a new family with this person as a spouse.
                let api = api_ctx.clone();
                let Some(tid) = tree_id_parsed else { return };
                spawn(async move {
                    if let Ok(family) = api.create_family(tid).await {
                        let body = crate::api::AddSpouseBody {
                            person_id: pid,
                            role: oxidgene_core::SpouseRole::Partner,
                            sort_order: 0,
                        };
                        let _ = api.add_spouse(tid, family.id, &body).await;
                        refresh += 1;
                    }
                });
            }
            PersonAction::AddChild => {
                // Find or create a family where this person is a spouse, then
                // create a new person as a child in that family.
                let api = api_ctx.clone();
                let Some(tid) = tree_id_parsed else { return };
                // Look up the family before spawning so we don't move pedigree_data.
                let existing_family_id = pedigree_data_ctx
                    .as_ref()
                    .and_then(|data| data.families_as_spouse.get(&pid))
                    .and_then(|fids| fids.first().copied());
                spawn(async move {
                    if let Some(fid) = existing_family_id {
                        // Create a new person and add as child.
                        if let Ok(new_person) = api
                            .create_person(
                                tid,
                                &crate::api::CreatePersonBody {
                                    sex: oxidgene_core::Sex::Unknown,
                                },
                            )
                            .await
                        {
                            let body = crate::api::AddChildBody {
                                person_id: new_person.id,
                                child_type: oxidgene_core::ChildType::Biological,
                                sort_order: 0,
                            };
                            let _ = api.add_child(tid, fid, &body).await;
                            refresh += 1;
                        }
                    } else {
                        // No existing family — create family, add person as spouse, create child.
                        if let Ok(family) = api.create_family(tid).await {
                            let spouse_body = crate::api::AddSpouseBody {
                                person_id: pid,
                                role: oxidgene_core::SpouseRole::Partner,
                                sort_order: 0,
                            };
                            let _ = api.add_spouse(tid, family.id, &spouse_body).await;
                            if let Ok(new_person) = api
                                .create_person(
                                    tid,
                                    &crate::api::CreatePersonBody {
                                        sex: oxidgene_core::Sex::Unknown,
                                    },
                                )
                                .await
                            {
                                let child_body = crate::api::AddChildBody {
                                    person_id: new_person.id,
                                    child_type: oxidgene_core::ChildType::Biological,
                                    sort_order: 0,
                                };
                                let _ = api.add_child(tid, family.id, &child_body).await;
                            }
                            refresh += 1;
                        }
                    }
                });
            }
            PersonAction::Delete => {
                confirm_delete_person_id.set(Some(pid));
                delete_person_error.set(None);
            }
        }
    };

    // Delete person handler.
    let api_del_person = api.clone();
    let on_confirm_delete_person = move |_| {
        let api = api_del_person.clone();
        let Some(tid) = tree_id_parsed else { return };
        let Some(pid) = confirm_delete_person_id() else {
            return;
        };
        spawn(async move {
            match api.delete_person(tid, pid).await {
                Ok(_) => {
                    confirm_delete_person_id.set(None);
                    delete_person_error.set(None);
                    // If we deleted the root person, clear the selection.
                    if selected_root() == Some(pid) {
                        selected_root.set(None);
                    }
                    refresh += 1;
                }
                Err(e) => delete_person_error.set(Some(format!("{e}"))),
            }
        });
    };

    // Empty slot handler — add a parent for a child.
    let api_empty = api.clone();
    let pedigree_data_empty = pedigree_data.clone();
    let on_empty_slot = move |(child_id, is_father): (Uuid, bool)| {
        let api = api_empty.clone();
        let Some(tid) = tree_id_parsed else { return };

        // Look up the family before spawning so we don't move pedigree_data
        // into the async block (which would make this closure FnOnce).
        let family_id = pedigree_data_empty
            .as_ref()
            .and_then(|data| data.families_as_child.get(&child_id))
            .and_then(|fids| fids.first().copied());

        spawn(async move {
            // Create a new person with appropriate sex.
            let sex = if is_father {
                oxidgene_core::Sex::Male
            } else {
                oxidgene_core::Sex::Female
            };
            let Ok(new_person) = api
                .create_person(tid, &crate::api::CreatePersonBody { sex })
                .await
            else {
                return;
            };

            let fid = if let Some(fid) = family_id {
                fid
            } else {
                // Create a new family and add the child to it.
                let Ok(family) = api.create_family(tid).await else {
                    return;
                };
                let child_body = crate::api::AddChildBody {
                    person_id: child_id,
                    child_type: oxidgene_core::ChildType::Biological,
                    sort_order: 0,
                };
                let _ = api.add_child(tid, family.id, &child_body).await;
                family.id
            };

            // Add the new person as a spouse in the family.
            let role = if is_father {
                oxidgene_core::SpouseRole::Husband
            } else {
                oxidgene_core::SpouseRole::Wife
            };
            let spouse_body = crate::api::AddSpouseBody {
                person_id: new_person.id,
                role,
                sort_order: 0,
            };
            let _ = api.add_spouse(tid, fid, &spouse_body).await;
            refresh += 1;
        });
    };

    // ── GEDCOM handlers ──

    let api_import = api.clone();
    let on_import_gedcom = move |_| {
        let api = api_import.clone();
        let Some(tid) = tree_id_parsed else { return };
        let gedcom = import_text().trim().to_string();
        spawn(async move {
            if gedcom.is_empty() {
                import_error.set(Some("GEDCOM text is required".to_string()));
                return;
            }
            importing.set(true);
            import_error.set(None);
            import_result.set(None);
            let body = ImportGedcomBody { gedcom };
            match api.import_gedcom(tid, &body.gedcom).await {
                Ok(result) => {
                    import_result.set(Some(result));
                    import_text.set(String::new());
                    importing.set(false);
                    refresh += 1;
                }
                Err(e) => {
                    import_error.set(Some(format!("{e}")));
                    importing.set(false);
                }
            }
        });
    };

    let api_export = api.clone();
    let on_export_gedcom = move |_| {
        let api = api_export.clone();
        let Some(tid) = tree_id_parsed else { return };
        spawn(async move {
            exporting.set(true);
            export_error.set(None);
            export_text.set(None);
            export_warnings.set(Vec::new());
            match api.export_gedcom(tid).await {
                Ok(result) => {
                    export_text.set(Some(result.gedcom));
                    export_warnings.set(result.warnings);
                    exporting.set(false);
                }
                Err(e) => {
                    export_error.set(Some(format!("{e}")));
                    exporting.set(false);
                }
            }
        });
    };

    // ── Render ──

    rsx! {
        // Back navigation
        div { style: "margin-bottom: 16px;",
            Link {
                to: Route::TreeList {},
                class: "back-link",
                "← Back to Trees"
            }
        }

        // Delete tree confirmation
        if confirm_delete() {
            ConfirmDialog {
                title: "Delete Tree",
                message: "Are you sure you want to delete this tree and all its data? This action cannot be undone.",
                confirm_label: "Delete",
                confirm_class: "btn btn-danger",
                error: delete_error(),
                on_confirm: move |_| on_confirm_delete(()),
                on_cancel: move |_| {
                    confirm_delete.set(false);
                    delete_error.set(None);
                },
            }
        }

        // Delete person confirmation
        if confirm_delete_person_id().is_some() {
            ConfirmDialog {
                title: "Delete Person",
                message: "Are you sure you want to delete this person? This action cannot be undone.",
                confirm_label: "Delete",
                confirm_class: "btn btn-danger",
                error: delete_person_error(),
                on_confirm: move |_| on_confirm_delete_person(()),
                on_cancel: move |_| {
                    confirm_delete_person_id.set(None);
                    delete_person_error.set(None);
                },
            }
        }

        // Context menu
        if let Some((_pid, x, y)) = context_menu_person() {
            ContextMenu {
                person_name: ctx_person_name.clone(),
                x: x,
                y: y,
                on_action: on_context_action,
                on_close: move |_| context_menu_person.set(None),
            }
        }

        // ── Tree header ──
        match &*tree_resource.read() {
            Some(Ok(tree)) => {
                let tree_name = tree.name.clone();
                let tree_desc = tree.description.clone().unwrap_or_default();
                rsx! {
                    if editing() {
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

        // ── Pedigree chart section ──
        div { class: "card", style: "margin-bottom: 24px;",
            div { class: "section-header",
                h2 { style: "font-size: 1.1rem;", "Pedigree" }
                {
                    let persons_data = persons_resource.read();
                    let total = match &*persons_data {
                        Some(Ok(conn)) => conn.total_count,
                        _ => 0,
                    };
                    rsx! {
                        span { class: "text-muted", style: "font-size: 0.85rem;",
                            "{total} person(s)"
                        }
                    }
                }
            }

            // Root person selector
            if !person_options.is_empty() {
                div { class: "root-selector",
                    label { "Root person:" }
                    select {
                        value: "{root_person_id.map(|id| id.to_string()).unwrap_or_default()}",
                        oninput: move |e: Event<FormData>| {
                            if let Ok(id) = e.value().parse::<Uuid>() {
                                selected_root.set(Some(id));
                            }
                        },
                        for (pid, name) in person_options.iter() {
                            option {
                                value: "{pid}",
                                selected: root_person_id == Some(*pid),
                                "{name}"
                            }
                        }
                    }
                }
            }

            // Chart
            if let (Some(data), Some(root_id)) = (pedigree_data.clone(), root_person_id) {
                PedigreeChart {
                    root_person_id: root_id,
                    data: data,
                    tree_id: tree_id.clone(),
                    on_person_click: move |(pid, x, y)| {
                        context_menu_person.set(Some((pid, x, y)));
                    },
                    on_empty_slot: move |(child_id, is_father)| {
                        on_empty_slot((child_id, is_father));
                    },
                }
            } else {
                // Loading or empty state
                {
                    let persons_data = persons_resource.read();
                    let names_data = names_resource.read();
                    let members_data = members_resource.read();
                    let all_loaded = persons_data.is_some()
                        && names_data.is_some()
                        && members_data.is_some();

                    if all_loaded {
                        rsx! {
                            div { class: "empty-state",
                                h3 { "No persons yet" }
                                p { "Import a GEDCOM file or use the person detail page to add people." }
                            }
                        }
                    } else {
                        rsx! {
                            div { class: "loading", "Loading pedigree data..." }
                        }
                    }
                }
            }
        }

        // ── GEDCOM Import / Export section ──
        div { class: "card",
            div { class: "section-header",
                h2 { style: "font-size: 1.1rem;", "GEDCOM" }
                button {
                    class: "btn btn-outline btn-sm",
                    onclick: move |_| show_gedcom.toggle(),
                    if show_gedcom() { "Hide" } else { "Show" }
                }
            }

            if show_gedcom() {
                // Import sub-section
                div { style: "margin-top: 16px; margin-bottom: 24px;",
                    div { class: "section-header",
                        h3 { style: "font-size: 0.95rem;", "Import" }
                        button {
                            class: "btn btn-outline btn-sm",
                            onclick: move |_| {
                                show_import_form.toggle();
                                import_error.set(None);
                            },
                            if show_import_form() { "Cancel" } else { "Import GEDCOM" }
                        }
                    }

                    if show_import_form() {
                        div { style: "padding: 16px; background: var(--color-bg); border-radius: var(--radius);",
                            p { style: "font-size: 0.85rem; color: var(--color-text-muted); margin-bottom: 12px;",
                                "Paste your GEDCOM file content below and click Import."
                            }
                            div { class: "form-group",
                                textarea {
                                    class: "gedcom-textarea",
                                    placeholder: "0 HEAD\n1 SOUR ...\n0 @I1@ INDI\n...",
                                    value: "{import_text}",
                                    oninput: move |e: Event<FormData>| import_text.set(e.value()),
                                }
                            }
                            if let Some(err) = import_error() {
                                div { class: "error-msg", "{err}" }
                            }
                            button {
                                class: "btn btn-primary",
                                disabled: importing(),
                                onclick: on_import_gedcom,
                                if importing() { "Importing..." } else { "Import" }
                            }
                        }
                    }

                    // Import result
                    if let Some(result) = import_result() {
                        div { class: "gedcom-result",
                            h4 { "Import Successful" }
                            div { class: "result-stats",
                                div { class: "result-stat",
                                    span { class: "stat-value", "{result.persons_count}" }
                                    span { class: "stat-label", "persons" }
                                }
                                div { class: "result-stat",
                                    span { class: "stat-value", "{result.families_count}" }
                                    span { class: "stat-label", "families" }
                                }
                                div { class: "result-stat",
                                    span { class: "stat-value", "{result.events_count}" }
                                    span { class: "stat-label", "events" }
                                }
                                div { class: "result-stat",
                                    span { class: "stat-value", "{result.sources_count}" }
                                    span { class: "stat-label", "sources" }
                                }
                                div { class: "result-stat",
                                    span { class: "stat-value", "{result.media_count}" }
                                    span { class: "stat-label", "media" }
                                }
                                div { class: "result-stat",
                                    span { class: "stat-value", "{result.places_count}" }
                                    span { class: "stat-label", "places" }
                                }
                                div { class: "result-stat",
                                    span { class: "stat-value", "{result.notes_count}" }
                                    span { class: "stat-label", "notes" }
                                }
                            }
                            if !result.warnings.is_empty() {
                                div { class: "gedcom-warnings",
                                    details {
                                        summary { "{result.warnings.len()} warning(s)" }
                                        ul {
                                            for warning in result.warnings.iter() {
                                                li { "{warning}" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Export sub-section
                div {
                    div { class: "section-header",
                        h3 { style: "font-size: 0.95rem;", "Export" }
                        button {
                            class: "btn btn-outline btn-sm",
                            disabled: exporting(),
                            onclick: on_export_gedcom,
                            if exporting() { "Exporting..." } else { "Export GEDCOM" }
                        }
                    }

                    if let Some(err) = export_error() {
                        div { class: "error-msg", "{err}" }
                    }

                    if let Some(gedcom) = export_text() {
                        div { style: "margin-top: 12px;",
                            div { class: "form-group",
                                label { "Exported GEDCOM" }
                                textarea {
                                    class: "gedcom-textarea",
                                    readonly: true,
                                    value: "{gedcom}",
                                }
                            }
                            if !export_warnings().is_empty() {
                                div { class: "gedcom-warnings",
                                    details {
                                        summary { "{export_warnings().len()} warning(s)" }
                                        ul {
                                            for warning in export_warnings().iter() {
                                                li { "{warning}" }
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
