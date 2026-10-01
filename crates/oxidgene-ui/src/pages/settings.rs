//! Tree settings page with navigation sidebar.
//!
//! Provides tree configuration (Tree & Roots), tools stubs,
//! and GEDCOM export functionality.

use dioxus::prelude::*;
use oxidgene_core::enums::TreeDefaultPrivacy;
use uuid::Uuid;

use crate::api::{ApiClient, ApiError, UpdateTreeBody};
use crate::components::audit_log::AuditLogSection;
use crate::components::breadcrumb::TreeBreadcrumb;
use crate::components::search_person::{
    PersonSearchSummary, SearchPerson, render_person_search_summary,
};
use crate::components::tree_cache::{fetch_tree_cached, use_current_person, use_tree_cache};
use crate::components::tree_icon_sidebar::ToolPageSidebar;
use crate::i18n::{I18n, Language, use_i18n};
use crate::pages::app_settings::{
    AppearanceSection, LanguageSection, NamesSection, PedigreeDefaultsSection,
    SHARED_SETTINGS_STYLES,
};
use crate::prefs::{PedigreeDefaults, SortParticles};
use crate::ui_observability::{
    UiAction, UiActionStep, UiLoadTrace, UiPage, trace_ui_action, trace_ui_action_step,
    use_traced_resource, use_ui_load_trace,
};

async fn wait_for_export(
    api: &ApiClient,
    tree_id: Uuid,
    merge_occupations: bool,
    merge_names: bool,
) -> Result<String, ApiError> {
    let started = trace_ui_action_step(
        UiActionStep::ExportQueue,
        api.start_export_job(tree_id, merge_occupations, merge_names),
    )
    .await?;
    trace_ui_action_step(UiActionStep::ExportPoll, async {
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
    })
    .await
}

/// The settings' sections, by group: each group's label and its sections'
/// ids and labels.
const SETTINGS_NAV: [(&str, &[(&str, &str)]); 4] = [
    (
        "settings.breadcrumb",
        &[
            ("tree-roots", "settings.tree_roots"),
            ("privacy", "settings.privacy"),
            ("date-display", "settings.date_display"),
            ("entry-options", "settings.entry_options"),
        ],
    ),
    ("settings.tools", &[("history", "settings.history")]),
    ("common.export", &[("export", "settings.export_tree")]),
    (
        "settings.global_preferences",
        &[
            ("appearance", "app_settings.appearance"),
            ("language", "app_settings.language"),
            ("pedigree", "app_settings.pedigree"),
            ("names", "app_settings.names"),
        ],
    ),
];

/// Where a GEDZIP export is saved: in the browser, the save picker, opened
/// during the click, before any network await.
#[cfg(target_arch = "wasm32")]
type SaveTarget = Option<crate::api::BrowserDownload>;
/// Where a GEDZIP export is saved: on the desktop, a dialog asked afterwards,
/// so there is nothing to open on the click.
#[cfg(not(target_arch = "wasm32"))]
struct SaveTarget;

#[cfg(target_arch = "wasm32")]
fn save_target(file_name: &str, is_gedzip: bool) -> SaveTarget {
    is_gedzip.then(|| crate::api::BrowserDownload::new(file_name))
}

#[cfg(not(target_arch = "wasm32"))]
fn save_target(_file_name: &str, _is_gedzip: bool) -> SaveTarget {
    SaveTarget
}

/// What an export ended with: the message to show on success, none when the
/// user cancelled the save, or the error to show.
type ExportOutcome = Result<Option<String>, String>;

/// Exports the tree as a GEDZIP, packed by a server job, into the save
/// picker opened on the click.
#[cfg(target_arch = "wasm32")]
async fn export_gedzip(
    api: &ApiClient,
    tid: Uuid,
    (merge_occupations, merge_names): (bool, bool),
    _file_name: &str,
    i18n: &I18n,
    target: SaveTarget,
) -> ExportOutcome {
    let mut destination = target.expect("GEDZIP save session");
    match destination.ready().await {
        Ok(true) => {}
        Ok(false) => return Ok(None),
        Err(_) => return Err(i18n.t("media.save_failed")),
    }
    let download_path = wait_for_export(api, tid, merge_occupations, merge_names)
        .await
        .map_err(|error| error.to_string())?;
    api.download_in_browser(destination, &download_path)
        .await
        .map(|()| Some(i18n.t("settings.export_success")))
        .map_err(|_| i18n.t("media.download_failed"))
}

/// Exports the tree as a GEDZIP, packed by a server job, to the file the
/// user picks.
#[cfg(not(target_arch = "wasm32"))]
async fn export_gedzip(
    api: &ApiClient,
    tid: Uuid,
    (merge_occupations, merge_names): (bool, bool),
    file_name: &str,
    i18n: &I18n,
    _target: SaveTarget,
) -> ExportOutcome {
    let download_path = wait_for_export(api, tid, merge_occupations, merge_names)
        .await
        .map_err(|error| error.to_string())?;
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_title(i18n.t("gedcom.save_file"))
        .set_file_name(file_name)
        .add_filter("GEDZIP", &["gdz"])
        .add_filter("All files", &["*"])
        .save_file()
        .await
    else {
        return Ok(None);
    };
    let path = file.path().to_path_buf();
    trace_ui_action_step(
        UiActionStep::ExportSave,
        api.download_to_file(&download_path, &path),
    )
    .await
    .map_err(|error| error.to_string())?;
    let path_display = path.display().to_string();
    Ok(Some(i18n.t_args(
        "settings.export_saved_to",
        &[("path", &path_display)],
    )))
}

/// Exports the tree as a GEDCOM file.
async fn export_gedcom(
    api: &ApiClient,
    tid: Uuid,
    (merge_occupations, merge_names): (bool, bool),
    file_name: &str,
    i18n: &I18n,
) -> ExportOutcome {
    let exported = trace_ui_action_step(
        UiActionStep::ExportRequest,
        api.export_gedcom(tid, merge_occupations, merge_names),
    )
    .await
    .map_err(|error| error.to_string())?;
    save_gedcom(exported.gedcom.into_bytes(), file_name, i18n).await
}

/// Hands the GEDCOM to the browser as a download.
#[cfg(target_arch = "wasm32")]
async fn save_gedcom(bytes: Vec<u8>, file_name: &str, i18n: &I18n) -> ExportOutcome {
    let byte_array = serde_json::to_string(&bytes).unwrap_or_else(|_| "[]".to_string());
    let download_name =
        serde_json::to_string(file_name).unwrap_or_else(|_| "\"export.ged\"".to_string());
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
    Ok(Some(i18n.t("settings.export_success")))
}

/// Writes the GEDCOM to the file the user picks.
#[cfg(not(target_arch = "wasm32"))]
async fn save_gedcom(bytes: Vec<u8>, file_name: &str, i18n: &I18n) -> ExportOutcome {
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_title(i18n.t("gedcom.save_file"))
        .set_file_name(file_name)
        .add_filter("GEDCOM", &["ged"])
        .add_filter("All files", &["*"])
        .save_file()
        .await
    else {
        return Ok(None);
    };
    let path = file.path().to_path_buf();
    trace_ui_action_step(UiActionStep::ExportSave, tokio::fs::write(&path, bytes))
        .await
        .map_err(|error| {
            i18n.t_args(
                "settings.export_write_error",
                &[("error", &error.to_string())],
            )
        })?;
    let path_display = path.display().to_string();
    Ok(Some(i18n.t_args(
        "settings.export_saved_to",
        &[("path", &path_display)],
    )))
}

/// Settings page for a tree.
#[component]
pub fn Settings(tree_id: String) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let theme_state = use_context::<Signal<crate::theme::ThemeState>>();
    let lang_signal = use_context::<Signal<Language>>();
    let sort_particles = use_context::<Signal<SortParticles>>();
    let pedigree_defaults = use_context::<Signal<Option<PedigreeDefaults>>>();
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
    let tree = {
        let loaded = tree_resource.read();
        let loaded = loaded
            .as_ref()
            .and_then(Option::as_ref)
            .and_then(|tree| tree.as_ref().ok());
        tree_cache.loaded_or_cached(tree_id_parsed, loaded)
    };
    let tree_name = tree
        .as_ref()
        .map(|tree| tree.name.clone())
        .unwrap_or_default();
    // The person last shown in this tree, else its SOSA root.
    let current_person = use_current_person();
    let sosa_root = tree.and_then(|tree| tree.sosa_root_person_id);
    let selected_person_id = tree_id_parsed
        .and_then(|tid| current_person.get(tid))
        .or(sosa_root);

    // Export handler
    let api_export = api.clone();
    let export_base_name = safe_export_file_name(&tree_name);
    let on_export = move |_| {
        let Some(tid) = tree_id_parsed else {
            return;
        };
        let api = api_export.clone();
        let is_gedzip = export_format() == "gedzip";
        let merges = (
            !is_gedzip && export_merge_occupations(),
            !is_gedzip && export_merge_names(),
        );
        let extension = if is_gedzip { "gdz" } else { "ged" };
        let file_name = format!("{export_base_name}.{extension}");
        export_loading.set(true);
        export_error.set(None);
        export_success.set(None);
        // Opened during the click, before any network await.
        let target = save_target(&file_name, is_gedzip);
        let action = UiAction::Export(if is_gedzip { "gedzip" } else { "gedcom" });
        spawn(trace_ui_action(action, async move {
            let outcome = if is_gedzip {
                export_gedzip(&api, tid, merges, &file_name, &i18n, target).await
            } else {
                export_gedcom(&api, tid, merges, &file_name, &i18n).await
            };
            match outcome {
                Ok(Some(message)) => export_success.set(Some(message)),
                Ok(None) => {}
                Err(message) => export_error.set(Some(message)),
            }
            export_loading.set(false);
        }));
    };

    let sec = active_section();

    rsx! {
        style { {SETTINGS_STYLES} }
        style { {SHARED_SETTINGS_STYLES} }

        div { class: "sub-page",
            // Breadcrumb
            div { class: "td-topbar",
                TreeBreadcrumb {
                    tree_id: tree_id.clone(),
                    tree_name: tree_name.clone(),
                    span { class: "td-bc-current", {i18n.t("settings.breadcrumb")} }
                }
            }

            div { class: "pd-page-shell",
            ToolPageSidebar {
                tree_id: tree_id.clone(),
                selected_person_id,
                show_settings: false,
            }

            div { class: "sub-page-content pd-content",
            div { class: "settings-layout",
                // Left navigation
                nav { class: "settings-nav",
                    for (group, entries) in SETTINGS_NAV {
                        div { class: "settings-nav-group",
                            div { class: "settings-nav-group-label", {i18n.t(group)} }
                            for (section, label) in entries {
                                button {
                                    class: if sec == *section { "settings-nav-item active" } else { "settings-nav-item" },
                                    onclick: move |_| active_section.set(section.to_string()),
                                    {i18n.t(label)}
                                }
                            }
                        }
                    }
                }

                // Content area
                div { class: "settings-content",
                    match sec.as_str() {
                        "tree-roots" => rsx! {
                            TreeRootsSection { tree_id: tree_id.clone(), tree_resource }
                        },
                        "privacy" => rsx! {
                            PrivacySection { tree_id: tree_id.clone(), tree_resource }
                        },
                        "entry-options" => rsx! {
                            EntryOptionsSection { tree_id: tree_id.clone(), tree_resource }
                        },
                        "export" => rsx! {
                            ExportSection {
                                on_export,
                                loading: export_loading(),
                                error: export_error(),
                                success: export_success(),
                                format: export_format,
                                merge_occupations: export_merge_occupations,
                                merge_names: export_merge_names,
                            }
                        },
                        "appearance" => rsx! { AppearanceSection { theme_state } },
                        "language" => rsx! { LanguageSection { lang_signal } },
                        "pedigree" => rsx! { PedigreeDefaultsSection { pedigree_defaults } },
                        "names" => rsx! { NamesSection { sort_particles } },
                        "history" if tree_id_parsed.is_some() => rsx! {
                            AuditLogSection { tree_id: tree_id_parsed.unwrap_or_default() }
                        },
                        _ => rsx! { PlaceholderSection { section_name: sec.clone() } },
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
    let tree_id_parsed = tree_id.parse::<Uuid>().ok();
    let loaded = tree_resource.read();
    let tree = loaded
        .as_ref()
        .and_then(Option::as_ref)
        .and_then(|tree| tree.as_ref().ok());
    let name = tree.map(|tree| tree.name.clone()).unwrap_or_default();
    let (sosa_root, self_person) = tree.map_or((None, None), |tree| {
        (tree.sosa_root_person_id, tree.self_person_id)
    });
    drop(loaded);
    let Some(tree_id) = tree_id_parsed else {
        return rsx! {};
    };
    rsx! {
        div { class: "settings-section",
            div { class: "settings-section-eyebrow", {i18n.t("settings.breadcrumb")} }
            h2 { class: "settings-section-title", {i18n.t("settings.tree_roots")} }
            p { class: "settings-section-subtitle",
                {i18n.t("settings.tree_roots_desc")}
            }
            TreeNameCard { tree_id, name }
            TreePersonCard { tree_id, setting: TreePerson::SosaRoot, stored: sosa_root }
            TreePersonCard { tree_id, setting: TreePerson::SelfPerson, stored: self_person }
        }
    }
}

/// The tree's name, and the form renaming it.
#[component]
fn TreeNameCard(tree_id: Uuid, name: String) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let tree_cache = use_tree_cache();
    let mut local_name = use_signal(|| None::<String>);
    let mut loading = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut success = use_signal(|| None::<String>);
    let current = local_name().unwrap_or(name);
    let on_rename = {
        let current = current.clone();
        move |_| {
            let api = api.clone();
            let name = current.trim().to_string();
            error.set(None);
            success.set(None);
            if name.is_empty() {
                error.set(Some(i18n.t("tree.form.name_required").to_string()));
                return;
            }
            loading.set(true);
            spawn(async move {
                let body = UpdateTreeBody {
                    name: Some(name),
                    ..Default::default()
                };
                match api.update_tree(tree_id, &body).await {
                    Ok(tree) => {
                        local_name.set(Some(tree.name.clone()));
                        tree_cache.refresh_tree(tree_id, tree);
                        success.set(Some(i18n.t("settings.tree_name_saved").to_string()));
                    }
                    Err(e) => error.set(Some(e.to_string())),
                }
                loading.set(false);
            });
        }
    };
    let save_label = if loading() {
        i18n.t("common.saving")
    } else {
        i18n.t("common.save")
    };
    rsx! {
        div { class: "card settings-card",
            h3 { class: "settings-card-title",
                {i18n.t("settings.tree_name")}
            }
            p { class: "settings-card-desc",
                {i18n.t("settings.tree_name_desc")}
            }
            div { class: "settings-tree-name-form",
                input {
                    r#type: "text",
                    value: "{current}",
                    placeholder: i18n.t("tree.form.name_placeholder"),
                    disabled: loading(),
                    oninput: move |e: Event<FormData>| local_name.set(Some(e.value())),
                }
                button {
                    class: "btn btn-primary settings-tree-name-save",
                    title: "{save_label}",
                    "aria-label": "{save_label}",
                    "aria-busy": loading(),
                    disabled: loading(),
                    onclick: on_rename,
                    if loading() {
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
            if let Some(message) = success() {
                div { class: "success-msg settings-feedback", "{message}" }
            }
            if let Some(error) = error() {
                div { class: "error-msg settings-feedback", "{error}" }
            }
        }
    }
}

/// A person the tree names in its settings.
#[derive(Clone, Copy, PartialEq)]
enum TreePerson {
    /// Whom the SOSA numbers count from.
    SosaRoot,
    /// Who the user is in the tree.
    SelfPerson,
}

impl TreePerson {
    /// The card's texts: title, description, search placeholder, change,
    /// clear, none chosen, and saved.
    const fn keys(self) -> [&'static str; 7] {
        match self {
            Self::SosaRoot => [
                "settings.root_person",
                "settings.root_person_desc",
                "settings.root_person_search",
                "settings.root_person_change",
                "settings.root_person_clear",
                "settings.root_person_none",
                "settings.root_person_saved",
            ],
            Self::SelfPerson => [
                "settings.who_am_i",
                "settings.who_am_i_desc",
                "settings.self_person_search",
                "settings.self_person_change",
                "settings.self_person_clear",
                "settings.self_person_none",
                "settings.self_person_saved",
            ],
        }
    }

    /// The update setting this person to `person`, or clearing it.
    fn update(self, person: Option<Uuid>) -> UpdateTreeBody {
        match self {
            Self::SosaRoot => UpdateTreeBody {
                sosa_root_person_id: Some(person),
                ..Default::default()
            },
            Self::SelfPerson => UpdateTreeBody {
                self_person_id: Some(person),
                ..Default::default()
            },
        }
    }
}

/// One of the tree's persons: who is chosen, with their portrait, and the
/// search changing it or the button clearing it.
#[component]
fn TreePersonCard(tree_id: Uuid, setting: TreePerson, stored: Option<Uuid>) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let tree_cache = use_tree_cache();
    let load_trace = use_context::<UiLoadTrace>();
    let [title, description, search, change, clear, none, saved] = setting.keys();
    let mut show_search = use_signal(|| false);
    let mut message = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    // Local override so the UI updates immediately after save/clear, without
    // waiting for the tree to be read again.
    let mut local = use_signal(|| None::<Option<Uuid>>);
    let stored_now = crate::utils::use_synced(stored);
    let current = local().unwrap_or(stored);

    let api_person = api.clone();
    let person = use_traced_resource(load_trace, "tree_person", move || {
        let api = api_person.clone();
        let person_id = local().unwrap_or(stored_now());
        async move {
            let person_id = person_id?;
            let profile = api.get_person_profile(tree_id, person_id).await.ok();
            let portrait = api.portrait_map_for_ids(tree_id, &[person_id]).await;
            Some((
                profile.map(PersonSearchSummary::from),
                portrait.get(&person_id).cloned(),
            ))
        }
    });

    let set_person = use_callback(move |person: Option<Uuid>| {
        let api = api.clone();
        show_search.set(false);
        message.set(false);
        error.set(None);
        spawn(async move {
            match api.update_tree(tree_id, &setting.update(person)).await {
                Ok(_) => {
                    tree_cache.invalidate();
                    local.set(Some(person));
                    message.set(true);
                }
                Err(e) => error.set(Some(e.to_string())),
            }
        });
    });

    let (summary, portrait) = match (current, &*person.read()) {
        (None, _) => (None, None),
        (Some(_), Some(Some((Some(summary), portrait)))) => {
            (Some(summary.clone()), portrait.clone())
        }
        (Some(id), Some(_)) => (
            Some(PersonSearchSummary::placeholder(
                id,
                i18n.t("common.unknown"),
            )),
            None,
        ),
        (Some(id), None) => (
            Some(PersonSearchSummary::placeholder(
                id,
                i18n.t("common.loading"),
            )),
            None,
        ),
    };

    rsx! {
        div { class: "card settings-card",
            h3 { class: "settings-card-title",
                {i18n.t(title)}
            }
            p { class: "settings-card-desc",
                {i18n.t(description)}
            }
            if show_search() {
                SearchPerson {
                    tree_id,
                    placeholder: i18n.t(search),
                    on_select: move |person_id: Uuid| set_person.call(Some(person_id)),
                    on_cancel: move |_| show_search.set(false),
                }
            } else if let Some(summary) = &summary {
                div { class: "sosa-root-display",
                    div { class: "sosa-root-person",
                        {render_person_search_summary(summary, portrait.clone(), &i18n)}
                    }
                    div { class: "sosa-root-actions",
                        button {
                            class: "btn btn-outline btn-sm",
                            onclick: move |_| show_search.set(true),
                            {i18n.t(change)}
                        }
                        button {
                            class: "btn btn-outline btn-sm btn-danger-outline",
                            onclick: move |_| set_person.call(None),
                            {i18n.t(clear)}
                        }
                    }
                }
            } else {
                div { class: "sosa-root-empty",
                    p { class: "text-muted", {i18n.t(none)} }
                    button {
                        class: "btn btn-primary btn-sm",
                        onclick: move |_| show_search.set(true),
                        {i18n.t(change)}
                    }
                }
            }
            if message() {
                div { class: "success-msg settings-feedback", {i18n.t(saved)} }
            }
            if let Some(err) = error() {
                div { class: "error-msg settings-feedback", "{err}" }
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

            div { class: "card settings-card",
                h3 { class: "settings-card-title",
                    {i18n.t("settings.default_privacy")}
                }
                p { class: "settings-section-subtitle",
                    {i18n.t("settings.default_privacy_desc")}
                }
                div { class: "pf-gender-group settings-choices",
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
                p { class: "pf-ns-hint settings-hint",
                    {i18n.t("privacy.not_enforced_yet")}
                }
                if let Some(err) = &save_error() {
                    div { class: "error-msg settings-feedback", "{err}" }
                }
            }
        }
    }
}

#[component]
fn EntryOptionsSection(
    tree_id: String,
    tree_resource: Resource<Option<Result<oxidgene_core::types::Tree, crate::api::ApiError>>>,
) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let tree_cache = use_tree_cache();
    let tree_id_parsed = tree_id.parse::<Uuid>().ok();

    let mut save_error = use_signal(|| None::<String>);
    // Local override so the control answers the click, not the refetch.
    let mut local_override = use_signal(|| None::<bool>);

    let current = local_override().unwrap_or_else(|| match &*tree_resource.read() {
        Some(Some(Ok(tree))) => tree.entry_suggestions,
        _ => true,
    });

    rsx! {
        div { class: "settings-section",
            div { class: "settings-section-eyebrow", {i18n.t("settings.breadcrumb")} }
            h2 { class: "settings-section-title", {i18n.t("settings.entry_options")} }

            div { class: "card settings-card",
                h3 { class: "settings-card-title",
                    {i18n.t("settings.entry_suggestions")}
                }
                p { class: "settings-section-subtitle",
                    {i18n.t("settings.entry_suggestions_desc")}
                }
                div { class: "pf-gender-group settings-choices",
                    for (value , label) in [(true, i18n.t("common.yes")), (false, i18n.t("common.no"))] {
                        button {
                            key: "{value}",
                            class: if current == value { "pf-gender-btn active" } else { "pf-gender-btn" },
                            r#type: "button",
                            onclick: {
                                let api = api.clone();
                                move |_| {
                                    let api = api.clone();
                                    local_override.set(Some(value));
                                    spawn(async move {
                                        let Some(tid) = tree_id_parsed else { return };
                                        let body = UpdateTreeBody {
                                            entry_suggestions: Some(value),
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
                if let Some(err) = &save_error() {
                    div { class: "error-msg settings-feedback", "{err}" }
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

            div { class: "card settings-card",
                div { class: "settings-export-row",
                    div { class: "settings-export-info",
                        h3 { class: "settings-card-title",
                            "{format_title}"
                        }
                        p { class: "settings-card-desc",
                            "{format_desc}"
                        }
                    }
                    select {
                        class: "settings-export-format",
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
                        class: "settings-check settings-check-first",
                        input {
                            r#type: "checkbox",
                            checked: merge_occupations(),
                            onchange: move |e: Event<FormData>| merge_occupations.set(e.checked()),
                        }
                        div {
                            div { class: "settings-check-label",
                                {i18n.t("settings.export_merge_occupations")}
                            }
                            p { class: "settings-check-desc",
                                {i18n.t("settings.export_merge_occupations_desc")}
                            }
                        }
                    }
                    label {
                        class: "settings-check",
                        input {
                            r#type: "checkbox",
                            checked: merge_names(),
                            onchange: move |e: Event<FormData>| merge_names.set(e.checked()),
                        }
                        div {
                            div { class: "settings-check-label",
                                {i18n.t("settings.export_merge_names")}
                            }
                            p { class: "settings-check-desc",
                                {i18n.t("settings.export_merge_names_desc")}
                            }
                        }
                    }
                }
                if let Some(err) = &error {
                    div { class: "error-msg settings-feedback", "{err}" }
                }
                if let Some(message) = &success {
                    div { class: "success-msg settings-feedback",
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
        _ => section_name.clone(),
    };

    let group = i18n.t("settings.breadcrumb");

    rsx! {
        div { class: "settings-section",
            div { class: "settings-section-eyebrow", "{group}" }
            h2 { class: "settings-section-title", "{display_name}" }

            div { class: "card settings-card",
                div { class: "empty-state",
                    h3 { {i18n.t("settings.coming_soon")} }
                    p { {i18n.t("settings.coming_soon_desc")} }
                }
            }
        }
    }
}

const SETTINGS_STYLES: &str = r#"
    .settings-card { margin-top: 16px; }
    .settings-card-title {
        font-size: 0.95rem;
        margin-bottom: 6px;
        color: var(--text-primary);
    }
    .settings-card-desc {
        font-size: 0.82rem;
        color: var(--text-secondary);
        margin-bottom: 12px;
    }
    .settings-export-row .settings-card-desc { margin-bottom: 0; }
    .settings-feedback,
    .settings-choices { margin-top: 12px; }
    .settings-hint { margin-top: 8px; }
    .settings-export-info { flex: 1; }
    .settings-check {
        display: grid;
        grid-template-columns: 20px 1fr;
        column-gap: 8px;
        align-items: start;
        margin-top: 12px;
        cursor: pointer;
    }
    .settings-check-first {
        margin-top: 16px;
        padding-top: 16px;
        border-top: 1px solid var(--border);
    }
    .settings-check input { margin: 3px 0 0 0; }
    .settings-check-label {
        font-size: 0.85rem;
        color: var(--text-primary);
    }
    .settings-check-desc {
        font-size: 0.78rem;
        color: var(--text-secondary);
        margin-top: 2px;
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
    .settings-export-row {
        display: flex;
        align-items: center;
        gap: 16px;
    }
    .settings-export-format {
        width: auto;
        flex-shrink: 0;
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
        .settings-export-row {
            flex-direction: column;
            align-items: stretch;
        }
        .settings-export-format {
            width: 100%;
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
