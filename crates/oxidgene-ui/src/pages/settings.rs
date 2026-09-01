//! Tree settings page with navigation sidebar.
//!
//! Provides tree configuration (Tree & Roots), tools stubs,
//! and GEDCOM export functionality.

use dioxus::prelude::*;
use oxidgene_core::enums::TreeDefaultPrivacy;
use uuid::Uuid;

use crate::api::{ApiClient, ApiError, UpdateTreeBody};
use crate::components::search_person::{
    PersonSearchSummary, SearchPerson, render_person_search_summary,
};
use crate::components::tree_cache::{fetch_tree_cached, use_tree_cache};
use crate::components::tree_icon_sidebar::{TreeIconSidebar, TreeSidebarView};
use crate::i18n::{Language, use_i18n};
use crate::pages::app_settings::{
    APP_SETTINGS_WIDGET_STYLES, AppearanceSection, LanguageSection, NamesSection,
};
use crate::prefs::SortParticles;
use crate::router::Route;
use crate::ui_observability::{UiLoadTrace, UiPage, use_traced_resource, use_ui_load_trace};

async fn wait_for_export(
    api: &ApiClient,
    tree_id: Uuid,
    merge_occupations: bool,
    merge_names: bool,
) -> Result<String, ApiError> {
    let started = api
        .start_export_job(tree_id, merge_occupations, merge_names)
        .await?;
    loop {
        let status = api.export_job_status(tree_id, started.job_id).await?;
        match status.phase.as_str() {
            "completed" => {
                return status.download_url.ok_or_else(|| ApiError::Api {
                    status: 500,
                    body: "completed export has no artifact".to_string(),
                });
            }
            "failed" => {
                return Err(ApiError::Api {
                    status: 422,
                    body: status.error.unwrap_or_else(|| "export_failed".to_string()),
                });
            }
            _ => crate::utils::sleep_ms(500).await,
        }
    }
}

/// Settings page for a tree.
#[component]
pub fn Settings(tree_id: String) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let nav = use_navigator();
    let is_dark = use_context::<Signal<bool>>();
    let lang_signal = use_context::<Signal<Language>>();
    let sort_particles = use_context::<Signal<SortParticles>>();
    let load_trace = use_ui_load_trace(UiPage::Settings);
    let refresh = use_signal(|| 0u32);
    let mut active_section = use_signal(|| "tree-roots".to_string());
    let mut export_loading = use_signal(|| false);
    let mut export_error = use_signal(|| None::<String>);
    let mut export_success = use_signal(|| None::<String>);
    let export_format = use_signal(|| "gedcom".to_string());
    let export_merge_occupations = use_signal(|| false);
    let export_merge_names = use_signal(|| false);

    let tree_id_parsed = tree_id.parse::<Uuid>().ok();

    // Fetch tree info
    let tree_cache = use_tree_cache();
    let api_tree = api.clone();
    let tree_resource = use_traced_resource(load_trace.clone(), "tree", move || {
        let api = api_tree.clone();
        let _tick = refresh();
        let _gen = tree_cache.generation();
        async move {
            let tid = tree_id_parsed?;
            Some(fetch_tree_cached(&api, &tree_cache, tid).await)
        }
    });

    // Resolve the name synchronously from the cache while the resource is
    // pending, so the breadcrumb never flashes a loading label.
    let tree_name = match &*tree_resource.read() {
        Some(Some(Ok(tree))) => tree.name.clone(),
        _ => tree_id_parsed
            .and_then(|tid| tree_cache.tree(tid))
            .map(|t| t.name)
            .unwrap_or_default(),
    };
    let selected_person_id = match &*tree_resource.read() {
        Some(Some(Ok(tree))) => tree.sosa_root_person_id,
        _ => tree_id_parsed
            .and_then(|tid| tree_cache.tree(tid))
            .and_then(|tree| tree.sosa_root_person_id),
    };

    // Export handler
    let api_export = api.clone();
    let export_base_name = safe_export_file_name(&tree_name);
    let on_export = move |_| {
        let api = api_export.clone();
        let base_name = export_base_name.clone();
        let is_gedzip = export_format() == "gedzip";
        let merge_occupations = !is_gedzip && export_merge_occupations();
        let merge_names = !is_gedzip && export_merge_names();
        export_loading.set(true);
        export_error.set(None);
        export_success.set(None);
        spawn(async move {
            if let Some(tid) = tree_id_parsed {
                let extension = if is_gedzip { "gdz" } else { "ged" };
                let file_name = format!("{base_name}.{extension}");
                if is_gedzip {
                    match wait_for_export(&api, tid, merge_occupations, merge_names).await {
                        Ok(download_path) => {
                            #[cfg(target_arch = "wasm32")]
                            {
                                let url =
                                    serde_json::to_string(&api.export_download_url(&download_path))
                                        .unwrap_or_else(|_| "\"\"".to_string());
                                let download_name = serde_json::to_string(&file_name)
                                    .unwrap_or_else(|_| "\"export.gdz\"".to_string());
                                document::eval(&format!(
                                    r#"
                                    const a = document.createElement('a');
                                    a.href = {url};
                                    a.download = {download_name};
                                    document.body.appendChild(a);
                                    a.click();
                                    document.body.removeChild(a);
                                    "#
                                ));
                                export_success.set(Some(i18n.t("settings.export_success")));
                            }
                            #[cfg(not(target_arch = "wasm32"))]
                            {
                                let file = rfd::AsyncFileDialog::new()
                                    .set_title(i18n.t("gedcom.save_file"))
                                    .set_file_name(&file_name)
                                    .add_filter("GEDZIP", &["gdz"])
                                    .add_filter("All files", &["*"])
                                    .save_file()
                                    .await;
                                if let Some(file) = file {
                                    let path = file.path().to_path_buf();
                                    match api.download_export_to_file(&download_path, &path).await {
                                        Ok(()) => {
                                            let path_display = path.display().to_string();
                                            export_success.set(Some(i18n.t_args(
                                                "settings.export_saved_to",
                                                &[("path", &path_display)],
                                            )));
                                        }
                                        Err(error) => export_error.set(Some(error.to_string())),
                                    }
                                }
                            }
                        }
                        Err(error) => export_error.set(Some(error.to_string())),
                    }
                    export_loading.set(false);
                    return;
                }

                match api.export_gedcom(tid, merge_occupations, merge_names).await {
                    Ok(result) => {
                        let bytes = result.gedcom.into_bytes();
                        #[cfg(target_arch = "wasm32")]
                        {
                            let byte_array =
                                serde_json::to_string(&bytes).unwrap_or_else(|_| "[]".to_string());
                            let download_name = serde_json::to_string(&file_name)
                                .unwrap_or_else(|_| "\"export.ged\"".to_string());
                            document::eval(&format!(
                                r#"
                                const bytes = new Uint8Array({byte_array});
                                const blob = new Blob([bytes], {{ type: 'text/plain' }});
                                const url = URL.createObjectURL(blob);
                                const a = document.createElement('a');
                                a.href = url;
                                a.download = {download_name};
                                document.body.appendChild(a);
                                a.click();
                                document.body.removeChild(a);
                                URL.revokeObjectURL(url);
                                "#
                            ));
                            export_success.set(Some(i18n.t("settings.export_success")));
                        }
                        #[cfg(not(target_arch = "wasm32"))]
                        {
                            let file = rfd::AsyncFileDialog::new()
                                .set_title(i18n.t("gedcom.save_file"))
                                .set_file_name(&file_name)
                                .add_filter("GEDCOM", &["ged"])
                                .add_filter("All files", &["*"])
                                .save_file()
                                .await;
                            if let Some(file) = file {
                                let path = file.path().to_path_buf();
                                match tokio::fs::write(&path, bytes).await {
                                    Ok(()) => {
                                        let path_display = path.display().to_string();
                                        export_success.set(Some(i18n.t_args(
                                            "settings.export_saved_to",
                                            &[("path", &path_display)],
                                        )));
                                    }
                                    Err(error) => export_error.set(Some(i18n.t_args(
                                        "settings.export_write_error",
                                        &[("error", &error.to_string())],
                                    ))),
                                }
                            }
                        }
                    }
                    Err(error) => export_error.set(Some(error.to_string())),
                }
                export_loading.set(false);
            }
        });
    };

    let sec = active_section();

    rsx! {
        style { {SETTINGS_STYLES} }
        style { {APP_SETTINGS_WIDGET_STYLES} }

        div { class: "sub-page",
            // Breadcrumb
            div { class: "td-topbar",
                nav { class: "td-bc",
                    Link { to: Route::Home {}, class: "td-bc-logo",
                        img {
                            src: crate::components::layout::LOGO_PNG_B64,
                            alt: "OxidGene",
                            class: "td-bc-logo-img",
                        }
                    }
                    if !tree_name.is_empty() {
                        Link {
                            to: Route::TreeDetail { tree_id: tree_id.clone(), person: None },
                            class: "td-bc-link",
                            "{tree_name}"
                        }
                        span { class: "td-bc-sep", "/" }
                    }
                    span { class: "td-bc-current", {i18n.t("settings.breadcrumb")} }
                }
            }

            div { class: "pd-page-shell",
            TreeIconSidebar {
                active_view: TreeSidebarView::None,
                selected_person_id: selected_person_id,
                show_middle_separator: false,
                show_add_person: false,
                show_dictionary: true,
                show_settings: false,
                on_profile_view: {
                    let tree_id = tree_id.clone();
                    move |pid: Option<Uuid>| {
                        if let Some(pid) = pid {
                            nav.push(Route::PersonDetail {
                                tree_id: tree_id.clone(),
                                person_id: pid.to_string(),
                            });
                        }
                    }
                },
                on_pedigree_view: {
                    let tree_id = tree_id.clone();
                    move |pid: Option<Uuid>| {
                        nav.push(Route::TreeDetail {
                            tree_id: tree_id.clone(),
                            person: pid.map(|pid| pid.to_string()),
                        });
                    }
                },
                on_add_person: move |_| {},
                on_dictionary: {
                    let tree_id = tree_id.clone();
                    move |_| {
                        nav.push(Route::Dictionary {
                            tree_id: tree_id.clone(),
                        });
                    }
                },
                on_settings: move |_| {},
            }

            div { class: "sub-page-content pd-content",
            div { class: "settings-layout",
                // Left navigation
                nav { class: "settings-nav",
                    div { class: "settings-nav-group",
                        div { class: "settings-nav-group-label", {i18n.t("settings.breadcrumb")} }
                        button {
                            class: if sec == "tree-roots" { "settings-nav-item active" } else { "settings-nav-item" },
                            onclick: move |_| active_section.set("tree-roots".to_string()),
                            {i18n.t("settings.tree_roots")}
                        }
                        button {
                            class: if sec == "privacy" { "settings-nav-item active" } else { "settings-nav-item" },
                            onclick: move |_| active_section.set("privacy".to_string()),
                            {i18n.t("settings.privacy")}
                        }
                        button {
                            class: if sec == "date-display" { "settings-nav-item active" } else { "settings-nav-item" },
                            onclick: move |_| active_section.set("date-display".to_string()),
                            {i18n.t("settings.date_display")}
                        }
                        button {
                            class: if sec == "entry-options" { "settings-nav-item active" } else { "settings-nav-item" },
                            onclick: move |_| active_section.set("entry-options".to_string()),
                            {i18n.t("settings.entry_options")}
                        }
                    }
                    div { class: "settings-nav-group",
                        div { class: "settings-nav-group-label", {i18n.t("settings.tools")} }
                        button {
                            class: if sec == "history" { "settings-nav-item active" } else { "settings-nav-item" },
                            onclick: move |_| active_section.set("history".to_string()),
                            {i18n.t("settings.history")}
                        }
                        button {
                            class: if sec == "anomalies" { "settings-nav-item active" } else { "settings-nav-item" },
                            onclick: move |_| active_section.set("anomalies".to_string()),
                            {i18n.t("settings.anomalies")}
                        }
                        button {
                            class: if sec == "duplicates" { "settings-nav-item active" } else { "settings-nav-item" },
                            onclick: move |_| active_section.set("duplicates".to_string()),
                            {i18n.t("settings.duplicates")}
                        }
                    }
                    div { class: "settings-nav-group",
                        div { class: "settings-nav-group-label", {i18n.t("common.export")} }
                        button {
                            class: if sec == "export" { "settings-nav-item active" } else { "settings-nav-item" },
                            onclick: move |_| active_section.set("export".to_string()),
                            {i18n.t("settings.export_tree")}
                        }
                    }
                    div { class: "settings-nav-group",
                        div { class: "settings-nav-group-label", {i18n.t("settings.global_preferences")} }
                        button {
                            class: if sec == "appearance" { "settings-nav-item active" } else { "settings-nav-item" },
                            onclick: move |_| active_section.set("appearance".to_string()),
                            {i18n.t("app_settings.appearance")}
                        }
                        button {
                            class: if sec == "language" { "settings-nav-item active" } else { "settings-nav-item" },
                            onclick: move |_| active_section.set("language".to_string()),
                            {i18n.t("app_settings.language")}
                        }
                        button {
                            class: if sec == "names" { "settings-nav-item active" } else { "settings-nav-item" },
                            onclick: move |_| active_section.set("names".to_string()),
                            {i18n.t("app_settings.names")}
                        }
                    }
                }

                // Content area
                div { class: "settings-content",
                    if sec == "tree-roots" {
                        TreeRootsSection {
                            tree_id: tree_id.clone(),
                            tree_resource: tree_resource,
                        }
                    } else if sec == "privacy" {
                        PrivacySection {
                            tree_id: tree_id.clone(),
                            tree_resource: tree_resource,
                        }
                    } else if sec == "export" {
                        ExportSection {
                            on_export: on_export,
                            loading: export_loading(),
                            error: export_error(),
                            success: export_success(),
                            format: export_format,
                            merge_occupations: export_merge_occupations,
                            merge_names: export_merge_names,
                        }
                    } else if sec == "appearance" {
                        AppearanceSection { is_dark }
                    } else if sec == "language" {
                        LanguageSection { lang_signal }
                    } else if sec == "names" {
                        NamesSection { sort_particles }
                    } else {
                        PlaceholderSection { section_name: sec.clone() }
                    }
                }
            }
            }
            }
        }
    }
}

#[component]
fn TreeRootsSection(
    tree_id: String,
    tree_resource: Resource<Option<Result<oxidgene_core::types::Tree, crate::api::ApiError>>>,
) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let tree_cache = use_tree_cache();
    let load_trace = use_context::<UiLoadTrace>();
    let tree_id_parsed = tree_id.parse::<Uuid>().ok();
    let api_portraits = api.clone();
    let portraits_resource = use_traced_resource(load_trace.clone(), "portraits", move || {
        let api = api_portraits.clone();
        async move {
            match tree_id_parsed {
                Some(tree_id) => match api.list_portraits(tree_id).await {
                    Ok(rows) => api.portrait_map(tree_id, &rows).await,
                    Err(_) => Default::default(),
                },
                None => Default::default(),
            }
        }
    });

    let mut show_search = use_signal(|| false);
    let mut save_message = use_signal(|| None::<String>);
    let mut save_error = use_signal(|| None::<String>);
    let mut local_tree_name = use_signal(|| None::<String>);
    let mut rename_loading = use_signal(|| false);
    let mut rename_error = use_signal(|| None::<String>);
    let mut rename_success = use_signal(|| None::<String>);
    // Local override so the UI updates immediately after save/clear,
    // without waiting for tree_resource to re-fetch.
    // None = use tree_resource value, Some(x) = override with x.
    let mut local_sosa_override = use_signal(|| None::<Option<Uuid>>);
    let mut show_self_search = use_signal(|| false);
    let mut self_save_message = use_signal(|| None::<String>);
    let mut self_save_error = use_signal(|| None::<String>);
    let mut local_self_override = use_signal(|| None::<Option<Uuid>>);

    // Current sosa_root_person_id: local override takes precedence
    let current_sosa_root = match local_sosa_override() {
        Some(val) => val,
        None => match &*tree_resource.read() {
            Some(Some(Ok(tree))) => tree.sosa_root_person_id,
            _ => None,
        },
    };
    let current_self_person = match local_self_override() {
        Some(value) => value,
        None => match &*tree_resource.read() {
            Some(Some(Ok(tree))) => tree.self_person_id,
            _ => None,
        },
    };
    let current_tree_name = local_tree_name().unwrap_or_else(|| match &*tree_resource.read() {
        Some(Some(Ok(tree))) => tree.name.clone(),
        _ => String::new(),
    });

    let api_rename = api.clone();
    let tree_name_for_rename = current_tree_name.clone();
    let on_rename = move |_| {
        let api = api_rename.clone();
        let name = local_tree_name()
            .unwrap_or_else(|| tree_name_for_rename.clone())
            .trim()
            .to_string();
        rename_error.set(None);
        rename_success.set(None);
        if name.is_empty() {
            rename_error.set(Some(i18n.t("tree.form.name_required").to_string()));
            return;
        }
        rename_loading.set(true);
        spawn(async move {
            let Some(tid) = tree_id_parsed else {
                rename_loading.set(false);
                return;
            };
            let body = UpdateTreeBody {
                name: Some(name),
                ..Default::default()
            };
            match api.update_tree(tid, &body).await {
                Ok(tree) => {
                    local_tree_name.set(Some(tree.name.clone()));
                    tree_cache.refresh_tree(tid, tree);
                    rename_success.set(Some(i18n.t("settings.tree_name_saved").to_string()));
                }
                Err(e) => rename_error.set(Some(e.to_string())),
            }
            rename_loading.set(false);
        });
    };

    // Fetch root person's identity directly (no full tree snapshot needed).
    // Reactive reads MUST happen inside the closure so use_resource re-runs
    // when tree_resource or local_sosa_override change.
    let api_root_person = api.clone();
    let root_person_resource = use_traced_resource(load_trace.clone(), "root_person", move || {
        let api = api_root_person.clone();
        let root_id = match local_sosa_override() {
            Some(val) => val,
            None => match &*tree_resource.read() {
                Some(Some(Ok(tree))) => tree.sosa_root_person_id,
                _ => None,
            },
        };
        let tid = tree_id_parsed;
        async move {
            let (Some(rid), Some(tid)) = (root_id, tid) else {
                return None;
            };
            api.get_person_profile(tid, rid)
                .await
                .ok()
                .map(PersonSearchSummary::from)
        }
    });

    // Resolve the current root person's search summary.
    let root_person_summary = {
        if let Some(root_id) = current_sosa_root {
            let data = root_person_resource.read();
            match &*data {
                Some(Some(summary)) => Some(summary.clone()),
                Some(None) => Some(PersonSearchSummary::placeholder(
                    root_id,
                    i18n.t("common.unknown"),
                )),
                None => Some(PersonSearchSummary::placeholder(
                    root_id,
                    i18n.t("common.loading"),
                )),
            }
        } else {
            None
        }
    };

    let api_self_person = api.clone();
    let self_person_resource = use_traced_resource(load_trace, "self_person", move || {
        let api = api_self_person.clone();
        let self_person_id = match local_self_override() {
            Some(value) => value,
            None => match &*tree_resource.read() {
                Some(Some(Ok(tree))) => tree.self_person_id,
                _ => None,
            },
        };
        let tid = tree_id_parsed;
        async move {
            let (Some(person_id), Some(tid)) = (self_person_id, tid) else {
                return None;
            };
            api.get_person_profile(tid, person_id)
                .await
                .ok()
                .map(PersonSearchSummary::from)
        }
    });

    let self_person_summary = {
        if let Some(person_id) = current_self_person {
            let data = self_person_resource.read();
            match &*data {
                Some(Some(summary)) => Some(summary.clone()),
                Some(None) => Some(PersonSearchSummary::placeholder(
                    person_id,
                    i18n.t("common.unknown"),
                )),
                None => Some(PersonSearchSummary::placeholder(
                    person_id,
                    i18n.t("common.loading"),
                )),
            }
        } else {
            None
        }
    };
    let portrait_urls = {
        let data = portraits_resource.read();
        match &*data {
            Some(urls) => urls.clone(),
            None => Default::default(),
        }
    };
    let root_person_portrait =
        current_sosa_root.and_then(|person_id| portrait_urls.get(&person_id).cloned());
    let self_person_portrait =
        current_self_person.and_then(|person_id| portrait_urls.get(&person_id).cloned());

    // Handler: save the selected person as sosa root
    let api_save = api.clone();
    let on_select_root = move |person_id: Uuid| {
        let api = api_save.clone();
        show_search.set(false);
        save_message.set(None);
        save_error.set(None);
        spawn(async move {
            if let Some(tid) = tree_id_parsed {
                let body = UpdateTreeBody {
                    default_privacy: None,
                    name: None,
                    description: None,
                    sosa_root_person_id: Some(Some(person_id)),
                    self_person_id: None,
                };
                match api.update_tree(tid, &body).await {
                    Ok(_) => {
                        tree_cache.invalidate();
                        local_sosa_override.set(Some(Some(person_id)));
                        save_message.set(Some("saved".to_string()));
                    }
                    Err(e) => {
                        save_error.set(Some(format!("{e}")));
                    }
                }
            }
        });
    };

    // Handler: clear the root person
    let api_clear = api.clone();
    let on_clear_root = move |_| {
        let api = api_clear.clone();
        save_message.set(None);
        save_error.set(None);
        spawn(async move {
            if let Some(tid) = tree_id_parsed {
                let body = UpdateTreeBody {
                    default_privacy: None,
                    name: None,
                    description: None,
                    sosa_root_person_id: Some(None),
                    self_person_id: None,
                };
                match api.update_tree(tid, &body).await {
                    Ok(_) => {
                        tree_cache.invalidate();
                        local_sosa_override.set(Some(None));
                        save_message.set(Some("saved".to_string()));
                    }
                    Err(e) => {
                        save_error.set(Some(format!("{e}")));
                    }
                }
            }
        });
    };

    let api_save_self = api.clone();
    let on_select_self = move |person_id: Uuid| {
        let api = api_save_self.clone();
        show_self_search.set(false);
        self_save_message.set(None);
        self_save_error.set(None);
        spawn(async move {
            if let Some(tid) = tree_id_parsed {
                let body = UpdateTreeBody {
                    self_person_id: Some(Some(person_id)),
                    ..Default::default()
                };
                match api.update_tree(tid, &body).await {
                    Ok(_) => {
                        tree_cache.invalidate();
                        local_self_override.set(Some(Some(person_id)));
                        self_save_message.set(Some("saved".to_string()));
                    }
                    Err(e) => self_save_error.set(Some(e.to_string())),
                }
            }
        });
    };

    let api_clear_self = api.clone();
    let on_clear_self = move |_| {
        let api = api_clear_self.clone();
        self_save_message.set(None);
        self_save_error.set(None);
        spawn(async move {
            if let Some(tid) = tree_id_parsed {
                let body = UpdateTreeBody {
                    self_person_id: Some(None),
                    ..Default::default()
                };
                match api.update_tree(tid, &body).await {
                    Ok(_) => {
                        tree_cache.invalidate();
                        local_self_override.set(Some(None));
                        self_save_message.set(Some("saved".to_string()));
                    }
                    Err(e) => self_save_error.set(Some(e.to_string())),
                }
            }
        });
    };

    rsx! {
        div { class: "settings-section",
            div { class: "settings-section-eyebrow", {i18n.t("settings.breadcrumb")} }
            h2 { class: "settings-section-title", {i18n.t("settings.tree_roots")} }
            p { class: "settings-section-subtitle",
                {i18n.t("settings.tree_roots_desc")}
            }

            div { class: "card", style: "margin-top: 16px;",
                h3 { style: "font-size: 0.95rem; margin-bottom: 6px; color: var(--text-primary);",
                    {i18n.t("settings.tree_name")}
                }
                p { style: "font-size: 0.82rem; color: var(--text-secondary); margin-bottom: 12px;",
                    {i18n.t("settings.tree_name_desc")}
                }
                div { class: "settings-tree-name-form",
                    input {
                        r#type: "text",
                        value: "{current_tree_name}",
                        placeholder: i18n.t("tree.form.name_placeholder"),
                        disabled: rename_loading(),
                        oninput: move |e: Event<FormData>| local_tree_name.set(Some(e.value())),
                    }
                    button {
                        class: "btn btn-primary settings-tree-name-save",
                        title: if rename_loading() { i18n.t("common.saving") } else { i18n.t("common.save") },
                        "aria-label": if rename_loading() { i18n.t("common.saving") } else { i18n.t("common.save") },
                        "aria-busy": rename_loading(),
                        disabled: rename_loading(),
                        onclick: on_rename,
                        if rename_loading() {
                            span { class: "btn-spinner" }
                        } else {
                            svg {
                                width: "18",
                                height: "18",
                                fill: "none",
                                "viewBox": "0 0 24 24",
                                stroke: "currentColor",
                                "strokeWidth": "2",
                                polyline { points: "17 21 17 13 7 13 7 21" }
                                polyline { points: "7 3 7 8 15 8" }
                                path { d: "M5 3h11l5 5v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z" }
                            }
                        }
                    }
                }
                if let Some(message) = rename_success() {
                    div { class: "success-msg", style: "margin-top: 12px;", "{message}" }
                }
                if let Some(error) = rename_error() {
                    div { class: "error-msg", style: "margin-top: 12px;", "{error}" }
                }
            }

            div { class: "card", style: "margin-top: 16px;",
                h3 { style: "font-size: 0.95rem; margin-bottom: 12px; color: var(--text-primary);",
                    {i18n.t("settings.root_person")}
                }
                p { style: "font-size: 0.82rem; color: var(--text-secondary); margin-bottom: 12px;",
                    {i18n.t("settings.root_person_desc")}
                }

                if show_search() {
                    if let Some(tid) = tree_id_parsed {
                        SearchPerson {
                            tree_id: tid,
                            placeholder: i18n.t("settings.root_person_search"),
                            on_select: on_select_root,
                            on_cancel: move |_| show_search.set(false),
                        }
                    }
                } else if let Some(summary) = &root_person_summary {
                    // Show current root person
                    div { class: "sosa-root-display",
                        div { class: "sosa-root-person",
                            {render_person_search_summary(summary, root_person_portrait.clone())}
                        }
                        div { class: "sosa-root-actions",
                            button {
                                class: "btn btn-outline btn-sm",
                                onclick: move |_| show_search.set(true),
                                {i18n.t("settings.root_person_change")}
                            }
                            button {
                                class: "btn btn-outline btn-sm btn-danger-outline",
                                onclick: on_clear_root,
                                {i18n.t("settings.root_person_clear")}
                            }
                        }
                    }
                } else {
                    // No root person set
                    div { class: "sosa-root-empty",
                        p { class: "text-muted", {i18n.t("settings.root_person_none")} }
                        button {
                            class: "btn btn-primary btn-sm",
                            onclick: move |_| show_search.set(true),
                            {i18n.t("settings.root_person_change")}
                        }
                    }
                }

                if let Some(_msg) = &save_message() {
                    div { class: "success-msg", style: "margin-top: 12px;",
                        {i18n.t("settings.root_person_saved")}
                    }
                }
                if let Some(err) = &save_error() {
                    div { class: "error-msg", style: "margin-top: 12px;", "{err}" }
                }
            }

            div { class: "card", style: "margin-top: 16px;",
                h3 { style: "font-size: 0.95rem; margin-bottom: 12px; color: var(--text-primary);",
                    {i18n.t("settings.who_am_i")}
                }
                p { style: "font-size: 0.82rem; color: var(--text-secondary); margin-bottom: 12px;",
                    {i18n.t("settings.who_am_i_desc")}
                }
                if show_self_search() {
                    if let Some(tid) = tree_id_parsed {
                        SearchPerson {
                            tree_id: tid,
                            placeholder: i18n.t("settings.self_person_search"),
                            on_select: on_select_self,
                            on_cancel: move |_| show_self_search.set(false),
                        }
                    }
                } else if let Some(summary) = &self_person_summary {
                    div { class: "sosa-root-display",
                        div { class: "sosa-root-person",
                            {render_person_search_summary(summary, self_person_portrait.clone())}
                        }
                        div { class: "sosa-root-actions",
                            button {
                                class: "btn btn-outline btn-sm",
                                onclick: move |_| show_self_search.set(true),
                                {i18n.t("settings.self_person_change")}
                            }
                            button {
                                class: "btn btn-outline btn-sm btn-danger-outline",
                                onclick: on_clear_self,
                                {i18n.t("settings.self_person_clear")}
                            }
                        }
                    }
                } else {
                    div { class: "sosa-root-empty",
                        p { class: "text-muted", {i18n.t("settings.self_person_none")} }
                        button {
                            class: "btn btn-primary btn-sm",
                            onclick: move |_| show_self_search.set(true),
                            {i18n.t("settings.self_person_change")}
                        }
                    }
                }
                if self_save_message().is_some() {
                    div { class: "success-msg", style: "margin-top: 12px;",
                        {i18n.t("settings.self_person_saved")}
                    }
                }
                if let Some(error) = self_save_error() {
                    div { class: "error-msg", style: "margin-top: 12px;", "{error}" }
                }
            }
        }
    }
}

/// What `Default` privacy means for everything in this tree.
///
/// Every person, couple and document defaults to "follows the tree", and
/// until this setting existed there was no tree setting to follow — the
/// commonest value in the model pointed at nothing.
#[component]
fn PrivacySection(
    tree_id: String,
    tree_resource: Resource<Option<Result<oxidgene_core::types::Tree, crate::api::ApiError>>>,
) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let tree_cache = use_tree_cache();
    let tree_id_parsed = tree_id.parse::<Uuid>().ok();

    let mut save_error = use_signal(|| None::<String>);
    // Local override so the control answers the click, not the refetch.
    let mut local_privacy_override = use_signal(|| None::<TreeDefaultPrivacy>);

    let current_privacy =
        local_privacy_override().unwrap_or_else(|| match &*tree_resource.read() {
            Some(Some(Ok(tree))) => tree.default_privacy,
            _ => TreeDefaultPrivacy::default(),
        });

    rsx! {
        div { class: "settings-section",
            div { class: "settings-section-eyebrow", {i18n.t("settings.breadcrumb")} }
            h2 { class: "settings-section-title", {i18n.t("settings.privacy")} }
            p { class: "settings-section-subtitle",
                {i18n.t("settings.privacy_desc")}
            }

            div { class: "card", style: "margin-top: 16px;",
                h3 { style: "font-size: 0.95rem; margin-bottom: 6px; color: var(--text-primary);",
                    {i18n.t("settings.default_privacy")}
                }
                p { class: "settings-section-subtitle",
                    {i18n.t("settings.default_privacy_desc")}
                }
                div { class: "pf-gender-group", style: "margin-top: 12px;",
                    for (value , label) in [
                        (TreeDefaultPrivacy::Private, i18n.t("privacy.private")),
                        (TreeDefaultPrivacy::Public, i18n.t("privacy.public")),
                    ] {
                        button {
                            key: "{value.as_str()}",
                            class: if current_privacy == value {
                                "pf-gender-btn active"
                            } else {
                                "pf-gender-btn"
                            },
                            r#type: "button",
                            onclick: {
                                let api = api.clone();
                                move |_| {
                                    let api = api.clone();
                                    local_privacy_override.set(Some(value));
                                    spawn(async move {
                                        let Some(tid) = tree_id_parsed else { return };
                                        let body = UpdateTreeBody {
                                            default_privacy: Some(value),
                                            ..Default::default()
                                        };
                                        match api.update_tree(tid, &body).await {
                                            Ok(_) => tree_cache.invalidate(),
                                            Err(e) => save_error.set(Some(e.to_string())),
                                        }
                                    });
                                }
                            },
                            "{label}"
                        }
                    }
                }
                p { class: "pf-ns-hint", style: "margin-top: 8px;",
                    {i18n.t("privacy.not_enforced_yet")}
                }
                if let Some(err) = &save_error() {
                    div { class: "error-msg", style: "margin-top: 12px;", "{err}" }
                }
            }
        }
    }
}

#[component]
fn ExportSection(
    on_export: EventHandler<MouseEvent>,
    loading: bool,
    error: Option<String>,
    success: Option<String>,
    format: Signal<String>,
    merge_occupations: Signal<bool>,
    merge_names: Signal<bool>,
) -> Element {
    let i18n = use_i18n();
    let is_gedzip = format() == "gedzip";
    let download_label = if is_gedzip {
        i18n.t("settings.download_gedzip")
    } else {
        i18n.t("settings.download_ged")
    };
    let format_title = if is_gedzip {
        i18n.t("settings.gedzip_title")
    } else {
        i18n.t("settings.gedcom_title")
    };
    let format_desc = if is_gedzip {
        i18n.t("settings.gedzip_desc")
    } else {
        i18n.t("settings.gedcom_desc")
    };
    rsx! {
        div { class: "settings-section",
            div { class: "settings-section-eyebrow", {i18n.t("common.export")} }
            h2 { class: "settings-section-title", {i18n.t("settings.export_tree")} }
            p { class: "settings-section-subtitle",
                {i18n.t("settings.export_desc")}
            }

            div { class: "card", style: "margin-top: 16px;",
                div { style: "display: flex; align-items: center; gap: 16px;",
                    div { style: "flex: 1;",
                        h3 { style: "font-size: 0.95rem; margin-bottom: 4px; color: var(--text-primary);",
                            "{format_title}"
                        }
                        p { style: "font-size: 0.82rem; color: var(--text-secondary);",
                            "{format_desc}"
                        }
                    }
                    select {
                        style: "width: auto; flex-shrink: 0;",
                        value: "{format}",
                        oninput: move |e: Event<FormData>| format.set(e.value()),
                        option { value: "gedcom", {i18n.t("settings.export_format_gedcom")} }
                        option { value: "gedzip", {i18n.t("settings.export_format_gedzip")} }
                    }
                    button {
                        class: "btn btn-primary",
                        disabled: loading,
                        onclick: on_export,
                        if loading { {i18n.t("common.exporting")} } else { {download_label} }
                    }
                }
                if !is_gedzip {
                    label {
                        style: "display: grid; grid-template-columns: 20px 1fr; column-gap: 8px; align-items: start; margin-top: 16px; padding-top: 16px; border-top: 1px solid var(--border); cursor: pointer;",
                        input {
                            r#type: "checkbox",
                            style: "margin: 3px 0 0 0;",
                            checked: merge_occupations(),
                            onchange: move |e: Event<FormData>| merge_occupations.set(e.checked()),
                        }
                        div {
                            div { style: "font-size: 0.85rem; color: var(--text-primary);",
                                {i18n.t("settings.export_merge_occupations")}
                            }
                            p { style: "font-size: 0.78rem; color: var(--text-secondary); margin-top: 2px;",
                                {i18n.t("settings.export_merge_occupations_desc")}
                            }
                        }
                    }
                    label {
                        style: "display: grid; grid-template-columns: 20px 1fr; column-gap: 8px; align-items: start; margin-top: 12px;",
                        input {
                            r#type: "checkbox",
                            style: "margin: 3px 0 0 0;",
                            checked: merge_names(),
                            onchange: move |e: Event<FormData>| merge_names.set(e.checked()),
                        }
                        div {
                            div { style: "font-size: 0.85rem; color: var(--text-primary);",
                                {i18n.t("settings.export_merge_names")}
                            }
                            p { style: "font-size: 0.78rem; color: var(--text-secondary); margin-top: 2px;",
                                {i18n.t("settings.export_merge_names_desc")}
                            }
                        }
                    }
                }
                if let Some(err) = &error {
                    div { class: "error-msg", style: "margin-top: 12px;", "{err}" }
                }
                if let Some(message) = &success {
                    div { class: "success-msg", style: "margin-top: 12px;",
                        "{message}"
                    }
                }
            }
        }
    }
}

fn safe_export_file_name(tree_name: &str) -> String {
    let safe = tree_name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect::<String>()
        .trim()
        .trim_matches('.')
        .to_string();

    if safe.is_empty() {
        "export".to_string()
    } else {
        safe
    }
}

#[component]
fn PlaceholderSection(section_name: String) -> Element {
    let i18n = use_i18n();
    let display_name = match section_name.as_str() {
        "privacy" => i18n.t("settings.privacy"),
        "date-display" => i18n.t("settings.date_display"),
        "entry-options" => i18n.t("settings.entry_options"),
        "history" => i18n.t("settings.history"),
        "anomalies" => i18n.t("settings.anomalies"),
        "duplicates" => i18n.t("settings.duplicates"),
        _ => section_name.clone(),
    };

    let group = match section_name.as_str() {
        "privacy" | "date-display" | "entry-options" => i18n.t("settings.breadcrumb"),
        "history" | "anomalies" | "duplicates" => i18n.t("settings.tools"),
        _ => i18n.t("settings.breadcrumb"),
    };

    rsx! {
        div { class: "settings-section",
            div { class: "settings-section-eyebrow", "{group}" }
            h2 { class: "settings-section-title", "{display_name}" }

            div { class: "card", style: "margin-top: 16px;",
                div { class: "empty-state",
                    h3 { {i18n.t("settings.coming_soon")} }
                    p { {i18n.t("settings.coming_soon_desc")} }
                }
            }
        }
    }
}

const SETTINGS_STYLES: &str = r#"
    .settings-layout {
        display: flex;
        gap: 24px;
        min-height: 0;
    }

    .settings-nav {
        width: 200px;
        min-width: 200px;
        flex-shrink: 0;
    }

    .settings-nav-group {
        margin-bottom: 20px;
    }

    .settings-nav-group-label {
        font-size: 0.68rem;
        font-weight: 700;
        color: var(--orange);
        text-transform: uppercase;
        letter-spacing: 0.5px;
        margin-bottom: 6px;
        padding: 0 8px;
    }

    .settings-nav-item {
        display: block;
        width: 100%;
        padding: 6px 8px;
        text-align: left;
        background: none;
        border: none;
        border-radius: 5px;
        font-size: 0.85rem;
        color: var(--text-secondary);
        cursor: pointer;
        transition: background 0.12s, color 0.12s;
        font-family: var(--font-sans);
    }
    .settings-nav-item:hover {
        background: var(--bg-card-hover);
        color: var(--text-primary);
    }
    .settings-nav-item.active {
        background: var(--sel-bg);
        color: var(--text-primary);
        font-weight: 600;
    }

    .settings-content {
        flex: 1;
        min-width: 0;
        max-width: 860px;
    }

    .settings-section-eyebrow {
        font-size: 0.68rem;
        font-weight: 700;
        color: var(--orange);
        text-transform: uppercase;
        letter-spacing: 0.5px;
        margin-bottom: 4px;
    }

    .settings-section-title {
        font-family: var(--font-heading);
        font-size: 1.2rem;
        font-weight: 600;
        color: var(--text-primary);
        margin-bottom: 4px;
    }

    .settings-section-subtitle {
        font-size: 0.85rem;
        color: var(--text-secondary);
    }

    .settings-placeholder {
        padding: 16px;
        text-align: center;
        color: var(--text-muted);
        font-size: 0.85rem;
        font-style: italic;
    }

    .settings-tree-name-form {
        display: flex;
        gap: 8px;
    }
    .settings-tree-name-form input {
        min-width: 0;
        flex: 1;
    }
    .settings-tree-name-save {
        width: 38px;
        height: 38px;
        flex: 0 0 38px;
        padding: 0;
        display: inline-flex;
        align-items: center;
        justify-content: center;
    }
    .settings-tree-name-save svg {
        display: block;
    }

    /* SOSA root person display */
    .sosa-root-display {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
        padding: 10px 12px;
        background: var(--bg-deep);
        border: 1px solid var(--border);
        border-radius: 6px;
    }
    .sosa-root-person {
        display: flex;
        align-items: center;
        gap: 10px;
        min-width: 0;
    }
    .sosa-root-actions {
        display: flex;
        gap: 6px;
        flex-shrink: 0;
    }
    .sosa-root-empty {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
        padding: 10px 12px;
        background: var(--bg-deep);
        border: 1px dashed var(--border);
        border-radius: 6px;
    }
    .btn-danger-outline {
        color: var(--red) !important;
        border-color: var(--red) !important;
    }
    .btn-danger-outline:hover {
        background: color-mix(in srgb, var(--red) 10%, transparent) !important;
    }

    @media (max-width: 768px) {
        .settings-layout {
            flex-direction: column;
        }
        .settings-nav {
            width: 100%;
            min-width: 0;
            display: flex;
            flex-wrap: nowrap;
            align-items: center;
            gap: 12px;
            overflow-x: auto;
            padding-bottom: 4px;
        }
        .settings-nav-group {
            display: flex;
            flex: none;
            align-items: center;
            flex-wrap: nowrap;
            gap: 4px;
            margin-bottom: 0;
        }
        .settings-nav-group-label {
            width: auto;
            margin: 0 4px 0 0;
            padding: 0 8px 0 0;
            border-right: 1px solid var(--border);
            white-space: nowrap;
        }
        .settings-nav-item {
            width: auto;
            flex: none;
            white-space: nowrap;
        }
        .sosa-root-display {
            flex-direction: column;
            align-items: stretch;
        }
        .sosa-root-person {
            align-items: flex-start;
        }
        .sosa-root-person .sp-result-name {
            white-space: normal;
            overflow: visible;
            text-overflow: clip;
            overflow-wrap: anywhere;
        }
        .sosa-root-person .sp-result-meta {
            overflow-wrap: anywhere;
        }
        .sosa-root-actions {
            justify-content: flex-end;
        }
    }
"#;
