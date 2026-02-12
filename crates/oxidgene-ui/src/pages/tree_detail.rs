//! Tree detail page — shows tree info, persons, and families, with edit & delete.

use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::{
    AddChildBody, AddSpouseBody, ApiClient, CreatePersonBody, CreatePlaceBody, CreateSourceBody,
    ImportGedcomBody, UpdatePlaceBody, UpdateSourceBody, UpdateTreeBody,
};
use crate::components::confirm_dialog::ConfirmDialog;
use crate::router::Route;
use crate::utils::{opt_str, parse_child_type, parse_sex, parse_spouse_role};

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

    // ── Place form state ──
    let mut show_place_form = use_signal(|| false);
    let mut place_form_name = use_signal(String::new);
    let mut place_form_lat = use_signal(String::new);
    let mut place_form_lon = use_signal(String::new);
    let mut place_form_error = use_signal(|| None::<String>);
    let mut editing_place_id = use_signal(|| None::<uuid::Uuid>);
    let mut edit_place_name = use_signal(String::new);
    let mut edit_place_lat = use_signal(String::new);
    let mut edit_place_lon = use_signal(String::new);
    let mut edit_place_error = use_signal(|| None::<String>);
    let mut confirm_delete_place_id = use_signal(|| None::<uuid::Uuid>);
    let mut delete_place_error = use_signal(|| None::<String>);

    // ── Source form state ──
    let mut show_source_form = use_signal(|| false);
    let mut source_form_title = use_signal(String::new);
    let mut source_form_author = use_signal(String::new);
    let mut source_form_publisher = use_signal(String::new);
    let mut source_form_abbreviation = use_signal(String::new);
    let mut source_form_repo = use_signal(String::new);
    let mut source_form_error = use_signal(|| None::<String>);
    let mut editing_source_id = use_signal(|| None::<uuid::Uuid>);
    let mut edit_source_title = use_signal(String::new);
    let mut edit_source_author = use_signal(String::new);
    let mut edit_source_publisher = use_signal(String::new);
    let mut edit_source_abbreviation = use_signal(String::new);
    let mut edit_source_repo = use_signal(String::new);
    let mut edit_source_error = use_signal(|| None::<String>);
    let mut confirm_delete_source_id = use_signal(|| None::<uuid::Uuid>);
    let mut delete_source_error = use_signal(|| None::<String>);

    // ── GEDCOM import/export state ──
    let mut show_import_form = use_signal(|| false);
    let mut import_text = use_signal(String::new);
    let mut import_error = use_signal(|| None::<String>);
    let mut import_result = use_signal(|| None::<crate::api::ImportGedcomResult>);
    let mut importing = use_signal(|| false);
    let mut export_text = use_signal(|| None::<String>);
    let mut export_error = use_signal(|| None::<String>);
    let mut export_warnings = use_signal(Vec::<String>::new);
    let mut exporting = use_signal(|| false);

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

    // Fetch places in the tree.
    let api_places = api.clone();
    let places_resource = use_resource(move || {
        let api = api_places.clone();
        let _tick = refresh();
        let tid = tree_id_parsed;
        async move {
            let Some(tid) = tid else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: "Invalid tree ID".to_string(),
                });
            };
            api.list_places(tid, Some(100), None, None).await
        }
    });

    // Fetch sources in the tree.
    let api_sources = api.clone();
    let sources_resource = use_resource(move || {
        let api = api_sources.clone();
        let _tick = refresh();
        let tid = tree_id_parsed;
        async move {
            let Some(tid) = tid else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: "Invalid tree ID".to_string(),
                });
            };
            api.list_sources(tid, Some(100), None).await
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
            let sex = parse_sex(&sex_str);
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

    // ── Place handlers ──

    // Create place handler.
    let api_create_place = api.clone();
    let on_create_place = move |_| {
        let api = api_create_place.clone();
        let Some(tid) = tree_id_parsed else { return };
        let name = place_form_name().trim().to_string();
        let lat_str = place_form_lat().trim().to_string();
        let lon_str = place_form_lon().trim().to_string();
        spawn(async move {
            if name.is_empty() {
                place_form_error.set(Some("Name is required".to_string()));
                return;
            }
            let latitude = if lat_str.is_empty() {
                None
            } else {
                match lat_str.parse::<f64>() {
                    Ok(v) => Some(v),
                    Err(_) => {
                        place_form_error.set(Some("Invalid latitude".to_string()));
                        return;
                    }
                }
            };
            let longitude = if lon_str.is_empty() {
                None
            } else {
                match lon_str.parse::<f64>() {
                    Ok(v) => Some(v),
                    Err(_) => {
                        place_form_error.set(Some("Invalid longitude".to_string()));
                        return;
                    }
                }
            };
            let body = CreatePlaceBody {
                name,
                latitude,
                longitude,
            };
            match api.create_place(tid, &body).await {
                Ok(_) => {
                    show_place_form.set(false);
                    place_form_name.set(String::new());
                    place_form_lat.set(String::new());
                    place_form_lon.set(String::new());
                    place_form_error.set(None);
                    refresh += 1;
                }
                Err(e) => {
                    place_form_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    // Save place edit handler.
    let api_save_place = api.clone();
    let on_save_place_edit = move |_| {
        let api = api_save_place.clone();
        let Some(tid) = tree_id_parsed else { return };
        let Some(pid) = editing_place_id() else {
            return;
        };
        let name = edit_place_name().trim().to_string();
        let lat_str = edit_place_lat().trim().to_string();
        let lon_str = edit_place_lon().trim().to_string();
        spawn(async move {
            if name.is_empty() {
                edit_place_error.set(Some("Name is required".to_string()));
                return;
            }
            let latitude: Option<Option<f64>> = if lat_str.is_empty() {
                Some(None)
            } else {
                match lat_str.parse::<f64>() {
                    Ok(v) => Some(Some(v)),
                    Err(_) => {
                        edit_place_error.set(Some("Invalid latitude".to_string()));
                        return;
                    }
                }
            };
            let longitude: Option<Option<f64>> = if lon_str.is_empty() {
                Some(None)
            } else {
                match lon_str.parse::<f64>() {
                    Ok(v) => Some(Some(v)),
                    Err(_) => {
                        edit_place_error.set(Some("Invalid longitude".to_string()));
                        return;
                    }
                }
            };
            let body = UpdatePlaceBody {
                name: Some(name),
                latitude,
                longitude,
            };
            match api.update_place(tid, pid, &body).await {
                Ok(_) => {
                    editing_place_id.set(None);
                    edit_place_error.set(None);
                    refresh += 1;
                }
                Err(e) => {
                    edit_place_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    // Delete place handler.
    let api_del_place = api.clone();
    let on_confirm_delete_place = move |_| {
        let api = api_del_place.clone();
        let Some(tid) = tree_id_parsed else { return };
        let Some(pid) = confirm_delete_place_id() else {
            return;
        };
        spawn(async move {
            match api.delete_place(tid, pid).await {
                Ok(_) => {
                    confirm_delete_place_id.set(None);
                    delete_place_error.set(None);
                    refresh += 1;
                }
                Err(e) => {
                    delete_place_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    // ── Source handlers ──

    // Create source handler.
    let api_create_source = api.clone();
    let on_create_source = move |_| {
        let api = api_create_source.clone();
        let Some(tid) = tree_id_parsed else { return };
        let title = source_form_title().trim().to_string();
        let author = source_form_author().trim().to_string();
        let publisher = source_form_publisher().trim().to_string();
        let abbreviation = source_form_abbreviation().trim().to_string();
        let repo_name = source_form_repo().trim().to_string();
        spawn(async move {
            if title.is_empty() {
                source_form_error.set(Some("Title is required".to_string()));
                return;
            }
            let body = CreateSourceBody {
                title,
                author: opt_str(&author),
                publisher: opt_str(&publisher),
                abbreviation: opt_str(&abbreviation),
                repository_name: opt_str(&repo_name),
            };
            match api.create_source(tid, &body).await {
                Ok(_) => {
                    show_source_form.set(false);
                    source_form_title.set(String::new());
                    source_form_author.set(String::new());
                    source_form_publisher.set(String::new());
                    source_form_abbreviation.set(String::new());
                    source_form_repo.set(String::new());
                    source_form_error.set(None);
                    refresh += 1;
                }
                Err(e) => {
                    source_form_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    // Save source edit handler.
    let api_save_source = api.clone();
    let on_save_source_edit = move |_| {
        let api = api_save_source.clone();
        let Some(tid) = tree_id_parsed else { return };
        let Some(sid) = editing_source_id() else {
            return;
        };
        let title = edit_source_title().trim().to_string();
        let author = edit_source_author().trim().to_string();
        let publisher = edit_source_publisher().trim().to_string();
        let abbreviation = edit_source_abbreviation().trim().to_string();
        let repo_name = edit_source_repo().trim().to_string();
        spawn(async move {
            if title.is_empty() {
                edit_source_error.set(Some("Title is required".to_string()));
                return;
            }
            let body = UpdateSourceBody {
                title: Some(title),
                author: Some(opt_str(&author)),
                publisher: Some(opt_str(&publisher)),
                abbreviation: Some(opt_str(&abbreviation)),
                repository_name: Some(opt_str(&repo_name)),
            };
            match api.update_source(tid, sid, &body).await {
                Ok(_) => {
                    editing_source_id.set(None);
                    edit_source_error.set(None);
                    refresh += 1;
                }
                Err(e) => {
                    edit_source_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    // Delete source handler.
    let api_del_source = api.clone();
    let on_confirm_delete_source = move |_| {
        let api = api_del_source.clone();
        let Some(tid) = tree_id_parsed else { return };
        let Some(sid) = confirm_delete_source_id() else {
            return;
        };
        spawn(async move {
            match api.delete_source(tid, sid).await {
                Ok(_) => {
                    confirm_delete_source_id.set(None);
                    delete_source_error.set(None);
                    refresh += 1;
                }
                Err(e) => {
                    delete_source_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    // ── GEDCOM handlers ──

    // Import GEDCOM handler.
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

    // Export GEDCOM handler.
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

        // Delete family confirmation dialog
        if confirm_delete_family_id().is_some() {
            ConfirmDialog {
                title: "Delete Family",
                message: "Are you sure you want to delete this family? All spouse and child links will be removed.",
                confirm_label: "Delete",
                confirm_class: "btn btn-danger",
                error: delete_family_error(),
                on_confirm: move |_| on_confirm_delete_family(()),
                on_cancel: move |_| {
                    confirm_delete_family_id.set(None);
                    delete_family_error.set(None);
                },
            }
        }

        // Delete place confirmation dialog
        if confirm_delete_place_id().is_some() {
            ConfirmDialog {
                title: "Delete Place",
                message: "Are you sure you want to delete this place?",
                confirm_label: "Delete",
                confirm_class: "btn btn-danger",
                error: delete_place_error(),
                on_confirm: move |_| on_confirm_delete_place(()),
                on_cancel: move |_| {
                    confirm_delete_place_id.set(None);
                    delete_place_error.set(None);
                },
            }
        }

        // Delete source confirmation dialog
        if confirm_delete_source_id().is_some() {
            ConfirmDialog {
                title: "Delete Source",
                message: "Are you sure you want to delete this source?",
                confirm_label: "Delete",
                confirm_class: "btn btn-danger",
                error: delete_source_error(),
                on_confirm: move |_| on_confirm_delete_source(()),
                on_cancel: move |_| {
                    confirm_delete_source_id.set(None);
                    delete_source_error.set(None);
                },
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
        div { class: "card", style: "margin-bottom: 24px;",
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

        // ── Places section ──
        div { class: "card", style: "margin-bottom: 24px;",
            div { class: "section-header",
                h2 { style: "font-size: 1.1rem;", "Places" }
                button {
                    class: "btn btn-primary btn-sm",
                    onclick: move |_| show_place_form.toggle(),
                    if show_place_form() { "Cancel" } else { "Add Place" }
                }
            }

            // Create place form
            if show_place_form() {
                div { style: "margin-bottom: 16px; padding: 16px; background: var(--color-bg); border-radius: var(--radius);",
                    h3 { style: "margin-bottom: 12px; font-size: 0.95rem;", "New Place" }
                    if let Some(err) = place_form_error() {
                        div { class: "error-msg", "{err}" }
                    }
                    div { class: "form-row",
                        div { class: "form-group",
                            label { "Name *" }
                            input {
                                r#type: "text",
                                value: "{place_form_name}",
                                oninput: move |e: Event<FormData>| place_form_name.set(e.value()),
                            }
                        }
                        div { class: "form-group",
                            label { "Latitude" }
                            input {
                                r#type: "text",
                                placeholder: "e.g. 48.8566",
                                value: "{place_form_lat}",
                                oninput: move |e: Event<FormData>| place_form_lat.set(e.value()),
                            }
                        }
                        div { class: "form-group",
                            label { "Longitude" }
                            input {
                                r#type: "text",
                                placeholder: "e.g. 2.3522",
                                value: "{place_form_lon}",
                                oninput: move |e: Event<FormData>| place_form_lon.set(e.value()),
                            }
                        }
                    }
                    button { class: "btn btn-primary btn-sm", onclick: on_create_place, "Create" }
                }
            }

            match &*places_resource.read() {
                Some(Ok(conn)) => rsx! {
                    if conn.edges.is_empty() {
                        div { class: "empty-state",
                            p { "No places in this tree yet." }
                        }
                    } else {
                        div { class: "table-wrapper",
                            table {
                                thead {
                                    tr {
                                        th { "Name" }
                                        th { "Latitude" }
                                        th { "Longitude" }
                                        th { "Actions" }
                                    }
                                }
                                tbody {
                                    for edge in conn.edges.iter() {
                                        {
                                            let place = &edge.node;
                                            let pid = place.id;
                                            let p_name = place.name.clone();
                                            let p_lat = place.latitude;
                                            let p_lon = place.longitude;
                                            let is_editing = editing_place_id() == Some(pid);

                                            if is_editing {
                                                rsx! {
                                                    tr {
                                                        td {
                                                            input {
                                                                r#type: "text",
                                                                value: "{edit_place_name}",
                                                                oninput: move |e: Event<FormData>| edit_place_name.set(e.value()),
                                                            }
                                                        }
                                                        td {
                                                            input {
                                                                r#type: "text",
                                                                value: "{edit_place_lat}",
                                                                oninput: move |e: Event<FormData>| edit_place_lat.set(e.value()),
                                                            }
                                                        }
                                                        td {
                                                            input {
                                                                r#type: "text",
                                                                value: "{edit_place_lon}",
                                                                oninput: move |e: Event<FormData>| edit_place_lon.set(e.value()),
                                                            }
                                                        }
                                                        td {
                                                            if let Some(err) = edit_place_error() {
                                                                div { class: "error-msg", "{err}" }
                                                            }
                                                            div { style: "display: flex; gap: 4px;",
                                                                button {
                                                                    class: "btn btn-primary btn-sm",
                                                                    onclick: on_save_place_edit.clone(),
                                                                    "Save"
                                                                }
                                                                button {
                                                                    class: "btn btn-outline btn-sm",
                                                                    onclick: move |_| {
                                                                        editing_place_id.set(None);
                                                                        edit_place_error.set(None);
                                                                    },
                                                                    "Cancel"
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            } else {
                                                rsx! {
                                                    tr {
                                                        td { "{p_name}" }
                                                        td { class: "text-muted",
                                                            {p_lat.map(|v| format!("{v:.4}")).unwrap_or_default()}
                                                        }
                                                        td { class: "text-muted",
                                                            {p_lon.map(|v| format!("{v:.4}")).unwrap_or_default()}
                                                        }
                                                        td {
                                                            div { style: "display: flex; gap: 4px;",
                                                                button {
                                                                    class: "btn btn-outline btn-sm",
                                                                    onclick: move |_| {
                                                                        editing_place_id.set(Some(pid));
                                                                        edit_place_name.set(p_name.clone());
                                                                        edit_place_lat.set(p_lat.map(|v| v.to_string()).unwrap_or_default());
                                                                        edit_place_lon.set(p_lon.map(|v| v.to_string()).unwrap_or_default());
                                                                        edit_place_error.set(None);
                                                                    },
                                                                    "Edit"
                                                                }
                                                                button {
                                                                    class: "btn btn-danger btn-sm",
                                                                    onclick: move |_| {
                                                                        confirm_delete_place_id.set(Some(pid));
                                                                        delete_place_error.set(None);
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
                        p { class: "text-muted", style: "margin-top: 8px; font-size: 0.85rem;",
                            "Total: {conn.total_count}"
                        }
                    }
                },
                Some(Err(e)) => rsx! {
                    div { class: "error-msg", "Failed to load places: {e}" }
                },
                None => rsx! {
                    div { class: "loading", "Loading places..." }
                },
            }
        }

        // ── Sources section ──
        div { class: "card", style: "margin-bottom: 24px;",
            div { class: "section-header",
                h2 { style: "font-size: 1.1rem;", "Sources" }
                button {
                    class: "btn btn-primary btn-sm",
                    onclick: move |_| show_source_form.toggle(),
                    if show_source_form() { "Cancel" } else { "Add Source" }
                }
            }

            // Create source form
            if show_source_form() {
                div { style: "margin-bottom: 16px; padding: 16px; background: var(--color-bg); border-radius: var(--radius);",
                    h3 { style: "margin-bottom: 12px; font-size: 0.95rem;", "New Source" }
                    if let Some(err) = source_form_error() {
                        div { class: "error-msg", "{err}" }
                    }
                    div { class: "form-row",
                        div { class: "form-group",
                            label { "Title *" }
                            input {
                                r#type: "text",
                                value: "{source_form_title}",
                                oninput: move |e: Event<FormData>| source_form_title.set(e.value()),
                            }
                        }
                        div { class: "form-group",
                            label { "Author" }
                            input {
                                r#type: "text",
                                value: "{source_form_author}",
                                oninput: move |e: Event<FormData>| source_form_author.set(e.value()),
                            }
                        }
                    }
                    div { class: "form-row",
                        div { class: "form-group",
                            label { "Publisher" }
                            input {
                                r#type: "text",
                                value: "{source_form_publisher}",
                                oninput: move |e: Event<FormData>| source_form_publisher.set(e.value()),
                            }
                        }
                        div { class: "form-group",
                            label { "Abbreviation" }
                            input {
                                r#type: "text",
                                value: "{source_form_abbreviation}",
                                oninput: move |e: Event<FormData>| source_form_abbreviation.set(e.value()),
                            }
                        }
                        div { class: "form-group",
                            label { "Repository" }
                            input {
                                r#type: "text",
                                value: "{source_form_repo}",
                                oninput: move |e: Event<FormData>| source_form_repo.set(e.value()),
                            }
                        }
                    }
                    button { class: "btn btn-primary btn-sm", onclick: on_create_source, "Create" }
                }
            }

            match &*sources_resource.read() {
                Some(Ok(conn)) => rsx! {
                    if conn.edges.is_empty() {
                        div { class: "empty-state",
                            p { "No sources in this tree yet." }
                        }
                    } else {
                        div { class: "table-wrapper",
                            table {
                                thead {
                                    tr {
                                        th { "Title" }
                                        th { "Author" }
                                        th { "Publisher" }
                                        th { "Actions" }
                                    }
                                }
                                tbody {
                                    for edge in conn.edges.iter() {
                                        {
                                            let source = &edge.node;
                                            let sid = source.id;
                                            let s_title = source.title.clone();
                                            let s_author = source.author.clone().unwrap_or_default();
                                            let s_publisher = source.publisher.clone().unwrap_or_default();
                                            let s_abbreviation = source.abbreviation.clone().unwrap_or_default();
                                            let s_repo = source.repository_name.clone().unwrap_or_default();
                                            let is_editing = editing_source_id() == Some(sid);

                                            if is_editing {
                                                rsx! {
                                                    tr {
                                                        td {
                                                            input {
                                                                r#type: "text",
                                                                value: "{edit_source_title}",
                                                                oninput: move |e: Event<FormData>| edit_source_title.set(e.value()),
                                                            }
                                                        }
                                                        td {
                                                            input {
                                                                r#type: "text",
                                                                value: "{edit_source_author}",
                                                                oninput: move |e: Event<FormData>| edit_source_author.set(e.value()),
                                                            }
                                                        }
                                                        td {
                                                            input {
                                                                r#type: "text",
                                                                value: "{edit_source_publisher}",
                                                                oninput: move |e: Event<FormData>| edit_source_publisher.set(e.value()),
                                                            }
                                                        }
                                                        td {
                                                            div { style: "margin-bottom: 8px;",
                                                                div { class: "form-group", style: "margin-bottom: 4px;",
                                                                    label { style: "font-size: 0.8rem;", "Abbreviation" }
                                                                    input {
                                                                        r#type: "text",
                                                                        value: "{edit_source_abbreviation}",
                                                                        oninput: move |e: Event<FormData>| edit_source_abbreviation.set(e.value()),
                                                                    }
                                                                }
                                                                div { class: "form-group", style: "margin-bottom: 4px;",
                                                                    label { style: "font-size: 0.8rem;", "Repository" }
                                                                    input {
                                                                        r#type: "text",
                                                                        value: "{edit_source_repo}",
                                                                        oninput: move |e: Event<FormData>| edit_source_repo.set(e.value()),
                                                                    }
                                                                }
                                                            }
                                                            if let Some(err) = edit_source_error() {
                                                                div { class: "error-msg", "{err}" }
                                                            }
                                                            div { style: "display: flex; gap: 4px;",
                                                                button {
                                                                    class: "btn btn-primary btn-sm",
                                                                    onclick: on_save_source_edit.clone(),
                                                                    "Save"
                                                                }
                                                                button {
                                                                    class: "btn btn-outline btn-sm",
                                                                    onclick: move |_| {
                                                                        editing_source_id.set(None);
                                                                        edit_source_error.set(None);
                                                                    },
                                                                    "Cancel"
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            } else {
                                                rsx! {
                                                    tr {
                                                        td { "{s_title}" }
                                                        td { class: "text-muted", "{s_author}" }
                                                        td { class: "text-muted", "{s_publisher}" }
                                                        td {
                                                            div { style: "display: flex; gap: 4px;",
                                                                button {
                                                                    class: "btn btn-outline btn-sm",
                                                                    onclick: move |_| {
                                                                        editing_source_id.set(Some(sid));
                                                                        edit_source_title.set(s_title.clone());
                                                                        edit_source_author.set(s_author.clone());
                                                                        edit_source_publisher.set(s_publisher.clone());
                                                                        edit_source_abbreviation.set(s_abbreviation.clone());
                                                                        edit_source_repo.set(s_repo.clone());
                                                                        edit_source_error.set(None);
                                                                    },
                                                                    "Edit"
                                                                }
                                                                button {
                                                                    class: "btn btn-danger btn-sm",
                                                                    onclick: move |_| {
                                                                        confirm_delete_source_id.set(Some(sid));
                                                                        delete_source_error.set(None);
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
                        p { class: "text-muted", style: "margin-top: 8px; font-size: 0.85rem;",
                            "Total: {conn.total_count}"
                        }
                    }
                },
                Some(Err(e)) => rsx! {
                    div { class: "error-msg", "Failed to load sources: {e}" }
                },
                None => rsx! {
                    div { class: "loading", "Loading sources..." }
                },
            }
        }

        // ── GEDCOM Import / Export section ──
        div { class: "card",
            h2 { style: "font-size: 1.1rem; margin-bottom: 16px;", "GEDCOM" }

            // ── Import sub-section ──
            div { style: "margin-bottom: 24px;",
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

                // Import result display
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

            // ── Export sub-section ──
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
