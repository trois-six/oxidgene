//! Tree settings page with navigation sidebar.
//!
//! Provides tree configuration (Tree & Roots), tools stubs,
//! and GEDCOM export functionality.

use dioxus::prelude::*;
use oxidgene_core::enums::{Calendar, DateDisplayFormat, DateQualifier, TreeDefaultPrivacy};
use oxidgene_core::types::QualifiedYear;
use uuid::Uuid;

use crate::api::{ApiClient, ApiError, UpdateTreeBody};
use crate::components::audit_log::AuditLogSection;
use crate::components::date_input::{
    calendar_from_value, calendar_value, format_date, format_example,
};
use crate::components::history_diff::format_timestamp;
use crate::components::pedigree_chart::format_lifespan;
use crate::components::search_person::{
    PersonSearchSummary, SearchPerson, render_person_search_summary,
};
use crate::components::tree_cache::use_tree_cache;
use crate::components::tree_page::{ToolPageFrame, use_tree_page};
use crate::i18n::{DateStyle, I18n, Language, use_i18n};
use crate::pages::app_settings::{
    AppearanceSection, LanguageSection, NamesSection, PedigreeDefaultsSection,
    SHARED_SETTINGS_STYLES,
};
use crate::prefs::{PedigreeDefaults, SortParticles};
use crate::ui_observability::{
    UiAction, UiActionStep, UiLoadTrace, UiPage, trace_ui_action, trace_ui_action_step,
    use_traced_resource, use_ui_load_trace,
};

/// A completed GEDZIP export, downloadable again until it expires.
#[derive(Debug, Clone, PartialEq)]
struct CompletedExport {
    /// The API path of the archive.
    download_path: String,
    /// When the server stops serving it, an hour after completion.
    expires_at: chrono::DateTime<chrono::Utc>,
    /// The name the save dialog proposes.
    file_name: String,
}

impl CompletedExport {
    /// Whether the archive can still be downloaded.
    fn is_live(&self) -> bool {
        chrono::Utc::now() < self.expires_at
    }
}

/// Queue a GEDZIP export and wait for it: its download path and expiry.
async fn wait_for_export(
    api: &ApiClient,
    tree_id: Uuid,
    merge_occupations: bool,
    merge_names: bool,
) -> Result<(String, chrono::DateTime<chrono::Utc>), ApiError> {
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
                    return status.download_url.zip(status.expires_at).ok_or_else(|| {
                        ApiError::Api {
                            status: 500,
                            body: "completed export has no artifact".to_string(),
                        }
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
/// picker opened on the click, and keeps it in `completed` for downloading
/// again.
#[cfg(target_arch = "wasm32")]
async fn export_gedzip(
    api: &ApiClient,
    tid: Uuid,
    (merge_occupations, merge_names): (bool, bool),
    file_name: &str,
    i18n: &I18n,
    target: SaveTarget,
    mut completed: Signal<Option<CompletedExport>>,
) -> ExportOutcome {
    // The picker comes first: cancelling it starts no job.
    let Some(destination) = ready_destination(target, i18n).await? else {
        return Ok(None);
    };
    let (download_path, expires_at) = wait_for_export(api, tid, merge_occupations, merge_names)
        .await
        .map_err(|error| error.to_string())?;
    completed.set(Some(CompletedExport {
        download_path: download_path.clone(),
        expires_at,
        file_name: file_name.to_string(),
    }));
    download_to_destination(api, destination, &download_path, i18n).await
}

/// Exports the tree as a GEDZIP, packed by a server job, to the file the
/// user picks, and keeps it in `completed` for downloading again.
#[cfg(not(target_arch = "wasm32"))]
async fn export_gedzip(
    api: &ApiClient,
    tid: Uuid,
    (merge_occupations, merge_names): (bool, bool),
    file_name: &str,
    i18n: &I18n,
    target: SaveTarget,
    mut completed: Signal<Option<CompletedExport>>,
) -> ExportOutcome {
    let (download_path, expires_at) = wait_for_export(api, tid, merge_occupations, merge_names)
        .await
        .map_err(|error| error.to_string())?;
    let export = CompletedExport {
        download_path,
        expires_at,
        file_name: file_name.to_string(),
    };
    completed.set(Some(export.clone()));
    save_gedzip(api, &export, i18n, target).await
}

/// Downloads a completed GEDZIP export again, into the save picker opened
/// on the click.
#[cfg(target_arch = "wasm32")]
async fn save_gedzip(
    api: &ApiClient,
    export: &CompletedExport,
    i18n: &I18n,
    target: SaveTarget,
) -> ExportOutcome {
    let Some(destination) = ready_destination(target, i18n).await? else {
        return Ok(None);
    };
    download_to_destination(api, destination, &export.download_path, i18n).await
}

/// The save picker opened on the click, once the user has chosen where to
/// save; `None` when they cancelled.
#[cfg(target_arch = "wasm32")]
async fn ready_destination(
    target: SaveTarget,
    i18n: &I18n,
) -> Result<Option<crate::api::BrowserDownload>, String> {
    let mut destination = target.expect("GEDZIP save session");
    match destination.ready().await {
        Ok(true) => Ok(Some(destination)),
        Ok(false) => Ok(None),
        Err(_) => Err(i18n.t("media.save_failed")),
    }
}

/// Streams the archive at `download_path` into `destination`.
#[cfg(target_arch = "wasm32")]
async fn download_to_destination(
    api: &ApiClient,
    destination: crate::api::BrowserDownload,
    download_path: &str,
    i18n: &I18n,
) -> ExportOutcome {
    api.download_in_browser(destination, download_path)
        .await
        .map(|()| Some(i18n.t("settings.export_success")))
        .map_err(|_| i18n.t("media.download_failed"))
}

/// Downloads a completed GEDZIP export to the file the user picks.
#[cfg(not(target_arch = "wasm32"))]
async fn save_gedzip(
    api: &ApiClient,
    export: &CompletedExport,
    i18n: &I18n,
    _target: SaveTarget,
) -> ExportOutcome {
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_title(i18n.t("gedcom.save_file"))
        .set_file_name(&export.file_name)
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
        api.download_to_file(&export.download_path, &path),
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
    use_ui_load_trace(UiPage::Settings);
    let mut active_section = use_signal(|| "tree-roots".to_string());
    let mut export_loading = use_signal(|| false);
    let mut export_error = use_signal(|| None::<String>);
    let mut export_success = use_signal(|| None::<String>);
    let export_format = use_signal(|| "gedcom".to_string());
    let export_merge_occupations = use_signal(|| false);
    let export_merge_names = use_signal(|| false);
    let mut last_export = use_signal(|| None::<CompletedExport>);

    let tree_id_parsed = tree_id.parse::<Uuid>().ok();

    let page = use_tree_page(&tree_id);
    let tree_resource = page.resource;
    let tree_name = page.name();

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
                export_gedzip(&api, tid, merges, &file_name, &i18n, target, last_export).await
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

    // Download the last GEDZIP export again, while the server keeps it.
    let api_again = api.clone();
    let on_download_again = move |_| {
        let Some(export) = last_export() else {
            return;
        };
        export_error.set(None);
        export_success.set(None);
        if !export.is_live() {
            last_export.set(None);
            export_error.set(Some(i18n.t("settings.export_expired")));
            return;
        }
        let api = api_again.clone();
        export_loading.set(true);
        // Opened during the click, before any network await.
        let target = save_target(&export.file_name, true);
        spawn(trace_ui_action(UiAction::Export("gedzip"), async move {
            match save_gedzip(&api, &export, &i18n, target).await {
                Ok(Some(message)) => export_success.set(Some(message)),
                Ok(None) => {}
                Err(message) => export_error.set(Some(message)),
            }
            export_loading.set(false);
        }));
    };
    let downloadable_until = last_export()
        .filter(CompletedExport::is_live)
        .map(|export| format_timestamp(&i18n, export.expires_at));

    let sec = active_section();

    rsx! {
        style { {SETTINGS_STYLES} }
        style { {SHARED_SETTINGS_STYLES} }

        ToolPageFrame {
            tree_id: tree_id.clone(),
            tree_name: tree_name.clone(),
            title: i18n.t("settings.breadcrumb"),
            printed: false,
            selected_person_id: page.selected_person_id,
            content_class: "pd-content",
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
                    "date-display" => rsx! {
                        DateDisplaySection { tree_id: tree_id.clone(), tree_resource }
                    },
                    "entry-options" => rsx! {
                        EntryOptionsSection { tree_id: tree_id.clone(), tree_resource }
                    },
                    "export" => rsx! {
                        ExportSection {
                            tree_id: tree_id.clone(),
                            tree_resource,
                            on_export,
                            on_download_again,
                            downloadable_until: downloadable_until.clone(),
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
                    _ => rsx! {},
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
            // Side by side: the portrait does not wait for the profile.
            let (profile, portrait) = futures_util::future::join(
                api.get_person_profile(tree_id, person_id),
                api.portrait_map_for_ids(tree_id, &[person_id]),
            )
            .await;
            let profile = profile.ok();
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

/// The tree as its settings page loaded it.
type TreeResource = Resource<Option<Result<oxidgene_core::types::Tree, crate::api::ApiError>>>;

/// The save of one of the tree's settings, made on the click, and the error
/// it last ended with. The stored tree goes back to the tree cache, so every
/// page follows the setting at once.
fn use_save_tree_setting(
    tree_id: Option<Uuid>,
) -> (Callback<UpdateTreeBody>, Signal<Option<String>>) {
    let api = use_context::<ApiClient>();
    let tree_cache = use_tree_cache();
    let mut error = use_signal(|| None::<String>);
    let save = use_callback(move |body: UpdateTreeBody| {
        let Some(tid) = tree_id else { return };
        let api = api.clone();
        error.set(None);
        spawn(async move {
            match api.update_tree(tid, &body).await {
                Ok(tree) => tree_cache.refresh_tree(tid, tree),
                Err(e) => error.set(Some(e.to_string())),
            }
        });
    });
    (save, error)
}

/// A tree setting that is on or off: its title, what it does, and Yes / No.
#[component]
fn ToggleCard(
    title: String,
    description: String,
    value: bool,
    on_change: EventHandler<bool>,
) -> Element {
    let i18n = use_i18n();
    rsx! {
        div { class: "card settings-card",
            h3 { class: "settings-card-title", "{title}" }
            p { class: "settings-section-subtitle", "{description}" }
            div { class: "pf-gender-group settings-choices",
                for (choice , label) in [(true, i18n.t("common.yes")), (false, i18n.t("common.no"))] {
                    button {
                        key: "{choice}",
                        class: if value == choice { "pf-gender-btn active" } else { "pf-gender-btn" },
                        r#type: "button",
                        "aria-pressed": value == choice,
                        onclick: move |_| on_change.call(choice),
                        "{label}"
                    }
                }
            }
        }
    }
}

/// How the tree's pages write dates: the format with its preview, the event
/// symbols, « circa », and the calendar other ones are also given in.
#[component]
fn DateDisplaySection(tree_id: String, tree_resource: TreeResource) -> Element {
    let i18n = use_i18n();
    let (save, error) = use_save_tree_setting(tree_id.parse().ok());
    // Local override so the controls and the preview answer the click, not
    // the save.
    let mut local = use_signal(|| None::<DateStyle>);
    let stored = match &*tree_resource.read() {
        Some(Some(Ok(tree))) => DateStyle::of(tree),
        _ => DateStyle::DEFAULT,
    };
    let style = local().unwrap_or(stored);
    let mut pick = move |next: DateStyle, body: UpdateTreeBody| {
        local.set(Some(next));
        save.call(body);
    };
    rsx! {
        div { class: "settings-section",
            div { class: "settings-section-eyebrow", {i18n.t("settings.breadcrumb")} }
            h2 { class: "settings-section-title", {i18n.t("settings.date_display")} }
            p { class: "settings-section-subtitle", {i18n.t("settings.date_display_desc")} }

            div { class: "card settings-card",
                h3 { class: "settings-card-title", {i18n.t("settings.date_format")} }
                p { class: "settings-section-subtitle", {i18n.t("settings.date_format_desc")} }
                select {
                    class: "settings-date-select",
                    "aria-label": i18n.t("settings.date_format"),
                    onchange: move |e: Event<FormData>| {
                        let value = e.value();
                        let Some(format) = DateDisplayFormat::ALL
                            .into_iter()
                            .find(|format| format.as_str() == value) else { return };
                        pick(
                            DateStyle { format, ..style },
                            UpdateTreeBody { date_format: Some(format), ..Default::default() },
                        );
                    },
                    for format in DateDisplayFormat::ALL {
                        option {
                            value: format.as_str(),
                            selected: style.format == format,
                            {format_example(&i18n, format)}
                        }
                    }
                }
                DatePreview { style }
            }
            ToggleCard {
                title: i18n.t("settings.date_symbols"),
                description: i18n.t("settings.date_symbols_desc"),
                value: style.symbols,
                on_change: move |symbols| pick(
                    DateStyle { symbols, ..style },
                    UpdateTreeBody { date_symbols: Some(symbols), ..Default::default() },
                ),
            }
            ToggleCard {
                title: i18n.t("settings.date_circa"),
                description: i18n.t("settings.date_circa_desc"),
                value: style.circa,
                on_change: move |circa| pick(
                    DateStyle { circa, ..style },
                    UpdateTreeBody { date_circa: Some(circa), ..Default::default() },
                ),
            }
            div { class: "card settings-card",
                h3 { class: "settings-card-title", {i18n.t("settings.date_calendar")} }
                p { class: "settings-section-subtitle", {i18n.t("settings.date_calendar_desc")} }
                select {
                    class: "settings-date-select",
                    "aria-label": i18n.t("settings.date_calendar"),
                    onchange: move |e: Event<FormData>| {
                        let calendar = calendar_from_value(&e.value());
                        pick(
                            DateStyle { calendar, ..style },
                            UpdateTreeBody { date_calendar: Some(calendar), ..Default::default() },
                        );
                    },
                    for calendar in [
                        Calendar::Gregorian,
                        Calendar::Julian,
                        Calendar::FrenchRepublican,
                        Calendar::Hebrew,
                    ] {
                        option {
                            value: calendar_value(calendar),
                            selected: style.calendar == calendar,
                            {i18n.t(&format!("calendar.{calendar}"))}
                        }
                    }
                }
            }
            if let Some(err) = error() {
                div { class: "error-msg settings-feedback", "{err}" }
            }
        }
    }
}

/// A fictitious person's dates as the tree would write them in `style`: a
/// birth recorded in the Republican calendar, a marriage, a death known only
/// roughly, and the lifespan a card would draw.
#[component]
fn DatePreview(style: DateStyle) -> Element {
    let i18n = use_i18n();
    let sample = i18n.with_dates(style);
    let rows = [
        (
            i18n.t("event.type.birth"),
            format_date(
                &sample,
                Calendar::FrenchRepublican,
                DateQualifier::Exact,
                Some("18 BRUM 8"),
                None,
            ),
        ),
        (
            i18n.t("event.type.marriage"),
            format_date(
                &sample,
                Calendar::Gregorian,
                DateQualifier::Exact,
                Some("12 MAR 1822"),
                None,
            ),
        ),
        (
            i18n.t("event.type.death"),
            format_date(
                &sample,
                Calendar::Gregorian,
                DateQualifier::About,
                Some("1867"),
                None,
            ),
        ),
        (
            i18n.t("settings.date_preview_lifespan"),
            format_lifespan(
                style,
                Some(QualifiedYear::new(1799, DateQualifier::Exact)),
                Some(QualifiedYear::new(1867, DateQualifier::About)),
            ),
        ),
    ];
    rsx! {
        div { class: "settings-date-preview", "aria-live": "polite",
            div { class: "settings-date-preview-title", {i18n.t("settings.date_preview")} }
            dl {
                for (label , text) in rows {
                    dt { "{label}" }
                    dd { "{text}" }
                }
            }
        }
    }
}

#[component]
fn EntryOptionsSection(tree_id: String, tree_resource: TreeResource) -> Element {
    let i18n = use_i18n();
    let (save, error) = use_save_tree_setting(tree_id.parse().ok());
    // Local override so the control answers the click, not the save.
    let mut local_override = use_signal(|| None::<bool>);
    let current = local_override().unwrap_or_else(|| match &*tree_resource.read() {
        Some(Some(Ok(tree))) => tree.entry_suggestions,
        _ => true,
    });

    rsx! {
        div { class: "settings-section",
            div { class: "settings-section-eyebrow", {i18n.t("settings.breadcrumb")} }
            h2 { class: "settings-section-title", {i18n.t("settings.entry_options")} }

            ToggleCard {
                title: i18n.t("settings.entry_suggestions"),
                description: i18n.t("settings.entry_suggestions_desc"),
                value: current,
                on_change: move |value| {
                    local_override.set(Some(value));
                    save.call(UpdateTreeBody { entry_suggestions: Some(value), ..Default::default() });
                },
            }
            if let Some(err) = error() {
                div { class: "error-msg settings-feedback", "{err}" }
            }
        }
    }
}

#[component]
fn ExportSection(
    tree_id: String,
    tree_resource: Resource<Option<Result<oxidgene_core::types::Tree, crate::api::ApiError>>>,
    on_export: EventHandler<MouseEvent>,
    on_download_again: EventHandler<MouseEvent>,
    /// When the last GEDZIP export stops being downloadable, formatted;
    /// `None` when there is none to download again.
    downloadable_until: Option<String>,
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
                if let Some(until) = &downloadable_until {
                    div { class: "settings-export-row settings-feedback",
                        p { class: "settings-card-desc settings-export-info",
                            {i18n.t_args("settings.export_available_until", &[("time", until)])}
                        }
                        button {
                            class: "btn btn-outline",
                            disabled: loading,
                            onclick: on_download_again,
                            {i18n.t("settings.export_download_again")}
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

            if let (Ok(tid), Some(Some(Ok(tree)))) = (tree_id.parse::<Uuid>(), &*tree_resource.read()) {
                SubmitterCard { key: "{tid}", tree_id: tid, tree: tree.clone() }
            }
        }
    }
}

/// Who the tree's exports say they are from: GEDCOM's submitter (`SUBM`).
/// A blank name falls back to the "Who am I?" person, then to `Not
/// Provided`.
#[component]
fn SubmitterCard(tree_id: Uuid, tree: oxidgene_core::types::Tree) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let tree_cache = use_tree_cache();
    let mut name = use_signal(|| tree.submitter_name.clone().unwrap_or_default());
    let mut email = use_signal(|| tree.submitter_email.clone().unwrap_or_default());
    let mut address = use_signal(|| tree.submitter_address.clone().unwrap_or_default());
    let mut saving = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut saved = use_signal(|| false);
    let field = |text: String| Some(Some(text.trim().to_string()).filter(|t| !t.is_empty()));
    let save = move |_| {
        let api = api.clone();
        let body = UpdateTreeBody {
            submitter_name: field(name()),
            submitter_email: field(email()),
            submitter_address: field(address()),
            ..Default::default()
        };
        saving.set(true);
        saved.set(false);
        error.set(None);
        spawn(async move {
            match api.update_tree(tree_id, &body).await {
                Ok(tree) => {
                    tree_cache.refresh_tree(tree_id, tree);
                    saved.set(true);
                }
                Err(e) => error.set(Some(e.to_string())),
            }
            saving.set(false);
        });
    };
    rsx! {
        div { class: "card settings-card",
            h3 { class: "settings-card-title", {i18n.t("settings.submitter")} }
            p { class: "settings-card-desc", {i18n.t("settings.submitter_desc")} }
            div { class: "form-row",
                div { class: "form-group",
                    label { {i18n.t("settings.submitter_name")} }
                    input {
                        r#type: "text",
                        value: "{name}",
                        placeholder: i18n.t("settings.submitter_name_placeholder"),
                        oninput: move |e: Event<FormData>| name.set(e.value()),
                    }
                }
                div { class: "form-group",
                    label { {i18n.t("settings.submitter_email")} }
                    input {
                        r#type: "email",
                        value: "{email}",
                        oninput: move |e: Event<FormData>| email.set(e.value()),
                    }
                }
            }
            div { class: "form-group",
                label { {i18n.t("settings.submitter_address")} }
                textarea {
                    rows: 3,
                    value: "{address}",
                    oninput: move |e: Event<FormData>| address.set(e.value()),
                }
            }
            div { class: "settings-export-row",
                button {
                    class: "btn btn-primary",
                    disabled: saving(),
                    onclick: save,
                    if saving() { {i18n.t("common.saving")} } else { {i18n.t("common.save")} }
                }
            }
            if let Some(err) = error() {
                div { class: "error-msg settings-feedback", "{err}" }
            }
            if saved() {
                div { class: "success-msg settings-feedback", {i18n.t("settings.submitter_saved")} }
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
    .settings-date-select { width: auto; max-width: 100%; }
    .settings-date-preview {
        margin-top: 12px;
        padding: 10px 12px;
        background: var(--bg-deep);
        border: 1px solid var(--border);
        border-radius: 6px;
    }
    .settings-date-preview-title {
        font-size: 0.72rem;
        text-transform: uppercase;
        letter-spacing: 0.06em;
        color: var(--text-secondary);
        margin-bottom: 6px;
    }
    .settings-date-preview dl {
        display: grid;
        grid-template-columns: max-content 1fr;
        column-gap: 16px;
        row-gap: 4px;
        margin: 0;
        font-size: 0.85rem;
    }
    .settings-date-preview dt { color: var(--text-secondary); }
    .settings-date-preview dd { margin: 0; color: var(--text-primary); }

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
