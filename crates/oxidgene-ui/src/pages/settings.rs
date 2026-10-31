//! Tree settings page with navigation sidebar.
//!
//! Provides tree configuration (Tree & Roots), tools stubs,
//! and GEDCOM export functionality.

use dioxus::prelude::*;
use oxidgene_core::enums::TreeDefaultPrivacy;
use serde_json::json;
use uuid::Uuid;

use crate::api::{
    ApiClient, ApiError, ArchiveChoices, DestinationReport, DownloadableExport, ExportChoices,
    ExportDestination, TagSummary, UpdateTreeBody, follow_job,
};
use crate::components::audit_log::AuditLogSection;
use crate::components::cancel_job::{CancelJobButton, JobKind, can_cancel};
use crate::components::history_diff::format_timestamp;
use crate::components::person_picker::PersonPicker;
use crate::components::search_person::{PersonSearchSummary, render_person_search_summary};
use crate::components::tags::{TagSelect, carried_tags};
use crate::components::tree_cache::use_tree_cache;
use crate::components::tree_page::{ToolPageFrame, use_tree_page};
use crate::i18n::{I18n, Language, use_i18n};
use crate::pages::app_settings::{
    AppearanceSection, DateDisplaySection, EntryOptionsSection, LanguageSection, NamesSection,
    PedigreeDefaultsSection, use_settings_scroll_reset,
};
use crate::prefs::{PedigreeDefaults, SortParticles};
use crate::ui_observability::{
    UiAction, UiActionStep, UiLoadTrace, UiPage, trace_ui_action, trace_ui_action_step,
    use_traced_resource, use_ui_load_trace, use_ui_resource,
};

/// Whether a completed GEDZIP export can still be downloaded.
fn is_live(export: &DownloadableExport) -> bool {
    chrono::Utc::now() < export.expires_at
}

/// A GEDZIP export job the page started, and the phase it last reported.
///
/// Kept once the job ended only when it was cancelled, which the section
/// then says; a completed or failed one says so through its message.
#[derive(Debug, Clone, PartialEq)]
struct ExportJobView {
    job_id: Uuid,
    phase: String,
}

impl ExportJobView {
    fn cancelled(&self) -> bool {
        self.phase == "cancelled"
    }

    fn cancelling(&self) -> bool {
        self.phase == "cancelling"
    }
}

/// The export button's label: what the export is doing while it runs,
/// else what it downloads.
fn export_button_label(
    i18n: &I18n,
    is_gedzip: bool,
    loading: bool,
    job: Option<&ExportJobView>,
) -> String {
    let key = if job.is_some_and(ExportJobView::cancelling) {
        "settings.export_cancelling"
    } else if loading {
        "common.exporting"
    } else if is_gedzip {
        "settings.download_gedzip"
    } else {
        "settings.download_ged"
    };
    i18n.t(key)
}

/// What the page keeps of its GEDZIP exports: the job running or last
/// cancelled, and the archive the last completed one produced.
#[derive(Clone, Copy)]
struct GedzipSignals {
    job: Signal<Option<ExportJobView>>,
    completed: Signal<Option<DownloadableExport>>,
}

/// Queue a GEDZIP export and wait for it, publishing it in `job` for the
/// Cancel button: the archive to download, or `None` when it was cancelled.
async fn wait_for_export(
    api: &ApiClient,
    tree_id: Uuid,
    choices: ArchiveChoices,
    mut job: Signal<Option<ExportJobView>>,
) -> Result<Option<DownloadableExport>, ApiError> {
    let started = trace_ui_action_step(
        UiActionStep::ExportQueue,
        api.start_export_job(tree_id, choices),
    )
    .await?;
    let polled = poll_export(api, tree_id, started.job_id, job).await;
    // A cancelled job stays, for the section to say so.
    if !job.peek().as_ref().is_some_and(ExportJobView::cancelled) {
        job.set(None);
    }
    polled
}

/// Follows export job `job_id` until it ends, keeping its phase in `job`:
/// the archive to download, or `None` when it was cancelled. `cancelling`
/// is not an end: the job is still stopping. Each read after the first
/// waits on the server until the job changes ([`follow_job`]).
async fn poll_export(
    api: &ApiClient,
    tree_id: Uuid,
    job_id: Uuid,
    mut job: Signal<Option<ExportJobView>>,
) -> Result<Option<DownloadableExport>, ApiError> {
    let status = trace_ui_action_step(
        UiActionStep::ExportPoll,
        follow_job(
            |wait| api.export_job_status(tree_id, job_id, wait),
            |status: &crate::api::ExportJobStatus| {
                job.set(Some(ExportJobView {
                    job_id,
                    phase: status.phase.clone(),
                }));
            },
        ),
    )
    .await?;
    match status.phase.as_str() {
        "cancelled" => Ok(None),
        "failed" => Err(ApiError::Api {
            status: 422,
            body: status.error.unwrap_or_else(|| "export_failed".to_string()),
        }),
        _ => {
            let size_bytes = status.size_bytes;
            status
                .download_url
                .zip(status.expires_at)
                .map(|(download_url, expires_at)| DownloadableExport {
                    format: "gedzip".to_string(),
                    download_url,
                    expires_at,
                    size_bytes,
                })
                .map(Some)
                .ok_or_else(|| ApiError::Api {
                    status: 500,
                    body: "completed export has no artifact".to_string(),
                })
        }
    }
}

/// The settings' sections, by group: each group's label and its sections'
/// ids and labels.
const SETTINGS_NAV: [(&str, &[(&str, &str)]); 4] = [
    (
        "settings.breadcrumb",
        &[
            ("tree-roots", "settings.tree_roots"),
            ("privacy", "settings.privacy"),
            ("anomalies", "settings.anomalies"),
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
            ("date-display", "app_settings.date_display"),
            ("entry-options", "app_settings.entry_options"),
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
/// picker opened on the click — which already proposed the file name — and
/// keeps it in `completed` for downloading again. A cancelled job saves
/// nothing, and the picker's session is released.
#[cfg(target_arch = "wasm32")]
async fn export_gedzip(
    api: &ApiClient,
    tid: Uuid,
    choices: ArchiveChoices,
    _file_name: &str,
    i18n: &I18n,
    target: SaveTarget,
    mut signals: GedzipSignals,
) -> ExportOutcome {
    // The picker comes first: cancelling it starts no job.
    let Some(destination) = ready_destination(target, i18n).await? else {
        return Ok(None);
    };
    let Some(export) = wait_for_export(api, tid, choices, signals.job)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let download_path = export.download_url.clone();
    signals.completed.set(Some(export));
    download_to_destination(api, destination, &download_path, i18n).await
}

/// Exports the tree as a GEDZIP, packed by a server job, to the file the
/// user picks, and keeps it in `completed` for downloading again. A
/// cancelled job asks for no file.
#[cfg(not(target_arch = "wasm32"))]
async fn export_gedzip(
    api: &ApiClient,
    tid: Uuid,
    choices: ArchiveChoices,
    file_name: &str,
    i18n: &I18n,
    target: SaveTarget,
    mut signals: GedzipSignals,
) -> ExportOutcome {
    let Some(export) = wait_for_export(api, tid, choices, signals.job)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    signals.completed.set(Some(export.clone()));
    save_gedzip(api, &export, file_name, i18n, target).await
}

/// Downloads a completed GEDZIP export again, into the save picker opened
/// on the click.
#[cfg(target_arch = "wasm32")]
async fn save_gedzip(
    api: &ApiClient,
    export: &DownloadableExport,
    _file_name: &str,
    i18n: &I18n,
    target: SaveTarget,
) -> ExportOutcome {
    let Some(destination) = ready_destination(target, i18n).await? else {
        return Ok(None);
    };
    download_to_destination(api, destination, &export.download_url, i18n).await
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
    export: &DownloadableExport,
    file_name: &str,
    i18n: &I18n,
    _target: SaveTarget,
) -> ExportOutcome {
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
        api.download_to_file(&export.download_url, &path),
    )
    .await
    .map_err(|error| error.to_string())?;
    let path_display = path.display().to_string();
    Ok(Some(i18n.t_args(
        "settings.export_saved_to",
        &[("path", &path_display)],
    )))
}

/// Exports the tree as a GEDCOM file written for the chosen destination,
/// and keeps in `report` what writing for it lost once the file is saved.
async fn export_gedcom(
    api: &ApiClient,
    tid: Uuid,
    choices: &ExportChoices,
    file_name: &str,
    i18n: &I18n,
    mut report: Signal<Option<DestinationReport>>,
) -> ExportOutcome {
    let exported =
        trace_ui_action_step(UiActionStep::ExportRequest, api.export_gedcom(tid, choices))
            .await
            .map_err(|error| error.to_string())?;
    let outcome = save_gedcom(exported.gedcom.into_bytes(), file_name, i18n).await;
    if matches!(outcome, Ok(Some(_))) {
        report.set(Some(exported.report));
    }
    outcome
}

/// The destination a GEDCOM export is written for unless one is chosen:
/// standard GEDCOM 5.5.1.
const DEFAULT_DESTINATION: &str = "generic-551";

/// The part of a destination's translation keys its id makes.
fn destination_key(id: &str) -> String {
    id.replace('-', "_")
}

/// A destination as the select names it: a standard version in the
/// reader's language, a program by its own name.
fn destination_name(i18n: &I18n, destination: &ExportDestination) -> String {
    let key = format!(
        "export_destination.name.{}",
        destination_key(&destination.id)
    );
    destination
        .standard
        .then(|| i18n.try_t(&key))
        .flatten()
        .unwrap_or_else(|| destination.label.clone())
}

/// What writing for a destination does, in one line; `None` for a
/// destination this version of the interface does not describe yet.
fn destination_description(i18n: &I18n, id: &str) -> Option<String> {
    i18n.try_t(&format!("export_destination.desc.{}", destination_key(id)))
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

/// The tree's GEDZIP export that can be downloaded again: the one this
/// page made, or, whenever the export section opens, the latest the server
/// still keeps, so that a reloaded page offers it too. It is dropped when it
/// expires, even while the page stays open.
fn use_kept_export(
    api: &ApiClient,
    tree_id: Option<Uuid>,
    active_section: Signal<String>,
) -> Signal<Option<DownloadableExport>> {
    let mut kept = use_signal(|| None::<DownloadableExport>);
    let api = api.clone();
    use_effect(move || {
        let (Some(tree_id), true) = (tree_id, active_section() == "export") else {
            return;
        };
        let api = api.clone();
        spawn(async move {
            // Without an answer, there is simply nothing to offer again.
            let Ok(Some(found)) = api.downloadable_export(tree_id).await else {
                return;
            };
            let newer = kept
                .peek()
                .as_ref()
                .is_none_or(|current| current.expires_at < found.expires_at);
            if newer {
                kept.set(Some(found));
            }
        });
    });
    use_effect(move || {
        let Some(export) = kept() else {
            return;
        };
        spawn(async move {
            let left = (export.expires_at - chrono::Utc::now()).num_milliseconds();
            crate::utils::sleep_ms(u32::try_from(left.max(0)).unwrap_or(u32::MAX)).await;
            if kept.peek().as_ref() == Some(&export) {
                kept.set(None);
            }
        });
    });
    kept
}

/// What the export that can be downloaded again is, and until when.
fn kept_export_label(i18n: &I18n, export: &DownloadableExport) -> String {
    let time = format_timestamp(i18n, export.expires_at);
    match export.size_bytes {
        Some(size) => i18n.t_args(
            "settings.export_available_archive",
            &[
                ("format", &export.format.to_uppercase()),
                ("size", &crate::utils::human_size(size)),
                ("time", &time),
            ],
        ),
        None => i18n.t_args("settings.export_available_until", &[("time", &time)]),
    }
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
    use_settings_scroll_reset(active_section);
    let mut export_loading = use_signal(|| false);
    let mut export_error = use_signal(|| None::<String>);
    let mut export_success = use_signal(|| None::<String>);
    let export_format = use_signal(|| "gedcom".to_string());
    let export_destination = use_signal(|| DEFAULT_DESTINATION.to_string());
    let mut export_report = use_signal(|| None::<DestinationReport>);
    let export_notes_and_sources = use_signal(|| true);
    let export_media = use_signal(|| true);
    let export_scope = use_signal(ExportScope::default);
    let tree_id_parsed = tree_id.parse::<Uuid>().ok();
    let mut last_export = use_kept_export(&api, tree_id_parsed, active_section);
    let mut export_job = use_signal(|| None::<ExportJobView>);

    let page = use_tree_page(&tree_id);
    let tree_resource = page.resource;
    let tree_name = page.name();

    // Export handler
    let api_export = api.clone();
    let export_base_name = safe_export_file_name(&tree_name);
    let base_name = export_base_name.clone();
    let on_export = move |_| {
        let Some(tid) = tree_id_parsed else {
            return;
        };
        let api = api_export.clone();
        let is_gedzip = export_format() == "gedzip";
        let Some(scope) = export_scope().target() else {
            let key = if export_scope().depth_error() {
                "settings.export_scope_depth_invalid"
            } else {
                "settings.export_scope_incomplete"
            };
            export_error.set(Some(i18n.t(key)));
            return;
        };
        // The destination is a plain GEDCOM's, the media a GEDZIP's.
        let choices = ExportChoices {
            destination: export_destination(),
            include_notes_and_sources: export_notes_and_sources(),
            scope: scope.clone(),
        };
        let archive_choices = ArchiveChoices {
            include_notes_and_sources: export_notes_and_sources(),
            include_media: export_media(),
            scope,
        };
        let extension = if is_gedzip { "gdz" } else { "ged" };
        let file_name = format!("{base_name}.{extension}");
        export_loading.set(true);
        export_error.set(None);
        export_success.set(None);
        export_report.set(None);
        export_job.set(None);
        // Opened during the click, before any network await.
        let target = save_target(&file_name, is_gedzip);
        let action = UiAction::Export(if is_gedzip { "gedzip" } else { "gedcom" });
        spawn(trace_ui_action(action, async move {
            let outcome = if is_gedzip {
                export_gedzip(
                    &api,
                    tid,
                    archive_choices,
                    &file_name,
                    &i18n,
                    target,
                    GedzipSignals {
                        job: export_job,
                        completed: last_export,
                    },
                )
                .await
            } else {
                export_gedcom(&api, tid, &choices, &file_name, &i18n, export_report).await
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
    let again_base_name = export_base_name.clone();
    let on_download_again = move |_| {
        let Some(export) = last_export() else {
            return;
        };
        export_error.set(None);
        export_success.set(None);
        export_report.set(None);
        export_job.set(None);
        if !is_live(&export) {
            last_export.set(None);
            export_error.set(Some(i18n.t("settings.export_expired")));
            return;
        }
        let api = api_again.clone();
        export_loading.set(true);
        let file_name = format!("{again_base_name}.gdz");
        // Opened during the click, before any network await.
        let target = save_target(&file_name, true);
        spawn(trace_ui_action(UiAction::Export("gedzip"), async move {
            match save_gedzip(&api, &export, &file_name, &i18n, target).await {
                Ok(Some(message)) => export_success.set(Some(message)),
                Ok(None) => {}
                Err(message) => export_error.set(Some(message)),
            }
            export_loading.set(false);
        }));
    };
    let downloadable = last_export()
        .filter(is_live)
        .map(|export| kept_export_label(&i18n, &export));

    let sec = active_section();

    rsx! {
        style { {SETTINGS_STYLES} }

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
                    "anomalies" => rsx! {
                        AnomaliesSection { tree_resource }
                    },
                    "export" => rsx! {
                        ExportSection {
                            tree_id: tree_id.clone(),
                            tree_resource,
                            on_export,
                            on_download_again,
                            downloadable: downloadable.clone(),
                            loading: export_loading(),
                            job: export_job(),
                            error: export_error(),
                            success: export_success(),
                            format: export_format,
                            destination: export_destination,
                            report: export_report(),
                            notes_and_sources: export_notes_and_sources,
                            media: export_media,
                            scope: export_scope,
                        }
                    },
                    "appearance" => rsx! { AppearanceSection { theme_state } },
                    "language" => rsx! { LanguageSection { lang_signal } },
                    "pedigree" => rsx! { PedigreeDefaultsSection { pedigree_defaults } },
                    "names" => rsx! { NamesSection { sort_particles } },
                    "date-display" => rsx! { DateDisplaySection {} },
                    "entry-options" => rsx! { EntryOptionsSection {} },
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
            PersonPicker {
                tree_id,
                selected: summary
                    .as_ref()
                    .map(|summary| render_person_search_summary(summary, portrait.clone(), &i18n)),
                search_placeholder: i18n.t(search),
                change_label: i18n.t(change),
                clear_label: i18n.t(clear),
                empty_label: i18n.t(none),
                on_change: move |person| set_person.call(person),
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

/// The thresholds of the anomaly rules, by the category of the rules that
/// apply them, as `AnomalyThresholds` names them.
const THRESHOLD_GROUPS: [(&str, &[&str]); 4] = [
    (
        "dates",
        &[
            "max_lifespan_years",
            "old_centenarian_years",
            "centenarian_before_year",
            "max_age_alive_years",
            "recorded_age_tolerance_years",
            "approximate_slack_years",
        ],
    ),
    (
        "filiation",
        &[
            "min_parent_age_years",
            "max_father_age_years",
            "max_mother_age_years",
            "max_posthumous_birth_days",
            "twin_days",
            "min_sibling_gap_days",
            "max_sibling_gap_years",
        ],
    ),
    (
        "unions",
        &[
            "min_union_age_years",
            "max_union_age_years",
            "max_spouse_gap_years",
        ],
    ),
    ("places", &["max_same_day_distance_km"]),
];

/// The thresholds the anomaly rules of the Tools page apply to this tree
/// (`docs/ui-settings.md` §8b).
#[component]
fn AnomaliesSection(
    tree_resource: Resource<Option<Result<oxidgene_core::types::Tree, crate::api::ApiError>>>,
) -> Element {
    let i18n = use_i18n();
    rsx! {
        div { class: "settings-section",
            div { class: "settings-section-eyebrow", {i18n.t("settings.breadcrumb")} }
            h2 { class: "settings-section-title", {i18n.t("settings.anomalies")} }
            p { class: "settings-section-subtitle", {i18n.t("settings.anomalies_desc")} }
            if let Some(Some(Ok(tree))) = &*tree_resource.read() {
                ThresholdsForm { key: "{tree.id}", tree_id: tree.id, thresholds: tree.anomaly_thresholds }
            }
        }
    }
}

/// Why `values` cannot be saved, as an i18n key: a threshold out of its
/// range is said beside it, so only the two that must agree are named here.
fn thresholds_problem(values: &oxidgene_core::types::AnomalyThresholds) -> Option<&'static str> {
    if values.old_centenarian_years > values.max_lifespan_years {
        Some("settings.thresholds_centenarian")
    } else if values.twin_days >= values.min_sibling_gap_days {
        Some("settings.thresholds_twins")
    } else {
        None
    }
}

/// One card per category of rules, a number field per threshold, saved all
/// at once.
#[component]
fn ThresholdsForm(tree_id: Uuid, thresholds: oxidgene_core::types::AnomalyThresholds) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let tree_cache = use_tree_cache();
    let text_of = |t: &oxidgene_core::types::AnomalyThresholds| {
        t.ranges()
            .iter()
            .map(|(name, value, _)| (*name, value.to_string()))
            .collect::<std::collections::BTreeMap<&'static str, String>>()
    };
    let mut texts = use_signal(|| text_of(&thresholds));
    let mut saving = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut saved = use_signal(|| false);

    // The values typed, each `None` while it is not a number in its range.
    let defaults = oxidgene_core::types::AnomalyThresholds::default();
    let mut values = defaults;
    let mut out_of_range = Vec::new();
    for (name, (min, max)) in defaults
        .ranges()
        .iter()
        .map(|(name, _, range)| (*name, *range))
    {
        let typed = texts
            .read()
            .get(name)
            .and_then(|text| text.trim().parse::<i32>().ok())
            .filter(|v| (min..=max).contains(v));
        match typed {
            Some(value) => {
                if let Some((_, field)) = values.fields_mut().into_iter().find(|(n, _)| *n == name)
                {
                    *field = value;
                }
            }
            None => out_of_range.push(name),
        }
    }
    let problem = thresholds_problem(&values);
    let can_save = out_of_range.is_empty() && problem.is_none() && !saving();

    let save = move |_| {
        let api = api.clone();
        let body = UpdateTreeBody {
            anomaly_thresholds: Some(values),
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
        for (category, names) in THRESHOLD_GROUPS {
            div { key: "{category}", class: "card settings-card",
                h3 { class: "settings-card-title", {i18n.t(&format!("tools.anomalies.category.{category}"))} }
                div { class: "form-row",
                    for name in names.iter().copied() {
                        {
                            let (min, max) = defaults
                                .ranges()
                                .iter()
                                .find(|(n, ..)| *n == name)
                                .map(|(_, _, range)| *range)
                                .unwrap_or_default();
                            let id = format!("threshold-{name}");
                            let text = texts.read().get(name).cloned().unwrap_or_default();
                            rsx! {
                                div { key: "{name}", class: "form-group",
                                    label { r#for: "{id}", {i18n.t(&format!("settings.threshold.{name}"))} }
                                    input {
                                        id: "{id}",
                                        r#type: "number",
                                        min: "{min}",
                                        max: "{max}",
                                        value: "{text}",
                                        oninput: move |e: Event<FormData>| {
                                            texts.write().insert(name, e.value());
                                            saved.set(false);
                                        },
                                    }
                                    if out_of_range.contains(&name) {
                                        p { class: "field-hint field-hint-warn",
                                            {i18n.t_args("settings.thresholds_range", &[("min", &min.to_string()), ("max", &max.to_string())])}
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        div { class: "card settings-card",
            if let Some(key) = problem {
                p { class: "field-hint field-hint-warn", {i18n.t(key)} }
            }
            div { class: "settings-export-row",
                button {
                    class: "btn btn-outline",
                    r#type: "button",
                    disabled: saving(),
                    onclick: move |_| {
                        texts.set(text_of(&oxidgene_core::types::AnomalyThresholds::default()));
                        saved.set(false);
                    },
                    {i18n.t("settings.thresholds_reset")}
                }
                button {
                    class: "btn btn-primary",
                    r#type: "button",
                    disabled: !can_save,
                    onclick: save,
                    if saving() { {i18n.t("common.saving")} } else { {i18n.t("common.save")} }
                }
            }
            if let Some(err) = error() {
                div { class: "error-msg settings-feedback", "{err}" }
            }
            if saved() {
                div { class: "success-msg settings-feedback", {i18n.t("settings.thresholds_saved")} }
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
    /// The GEDZIP export that can be downloaded again, described; `None`
    /// when there is none.
    downloadable: Option<String>,
    loading: bool,
    /// The GEDZIP export job running, which Cancel stops, or the last one
    /// when it was cancelled.
    job: Option<ExportJobView>,
    error: Option<String>,
    success: Option<String>,
    format: Signal<String>,
    /// The destination a plain GEDCOM is written for.
    destination: Signal<String>,
    /// What the last GEDCOM export lost writing for its destination.
    report: Option<DestinationReport>,
    notes_and_sources: Signal<bool>,
    media: Signal<bool>,
    /// What the export is limited to.
    scope: Signal<ExportScope>,
) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let is_gedzip = format() == "gedzip";
    // Without the list (still loading, or unreachable), the default alone.
    let listed = use_ui_resource("export_destinations", move || {
        let api = api.clone();
        async move { api.export_destinations().await.unwrap_or_default() }
    });
    let destinations = listed_or_default(listed.read().clone());
    let button_label = export_button_label(&i18n, is_gedzip, loading, job.as_ref());
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
                    if let (Some(running), Ok(tid)) = (job.as_ref().filter(|j| can_cancel(&j.phase)), tree_id.parse::<Uuid>()) {
                        CancelJobButton { kind: JobKind::Export, tree_id: tid, job_id: running.job_id }
                    }
                    button {
                        class: "btn btn-primary",
                        disabled: loading,
                        onclick: on_export,
                        "{button_label}"
                    }
                }
                if !is_gedzip {
                    DestinationPicker { destination, destinations: destinations.clone() }
                }
                if let Some(tid) = tree_id.parse::<Uuid>().ok() {
                    ExportScopePicker { scope, tree_id: tid }
                }
                ExportToggle {
                    checked: notes_and_sources(),
                    label: i18n.t("settings.export_notes_and_sources"),
                    description: Some(i18n.t("settings.export_notes_and_sources_desc")),
                    on_change: move |checked: bool| notes_and_sources.set(checked),
                    first: is_gedzip,
                }
                if is_gedzip {
                    ExportToggle {
                        checked: media(),
                        label: i18n.t("settings.export_media"),
                        description: Some(i18n.t("settings.export_media_desc")),
                        on_change: move |checked: bool| media.set(checked),
                    }
                }
                if let Some(description) = &downloadable {
                    div { class: "settings-export-row settings-feedback",
                        p { class: "settings-card-desc settings-export-info",
                            "{description}"
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
                if job.as_ref().is_some_and(ExportJobView::cancelled) {
                    div { class: "success-msg settings-feedback", role: "status",
                        {i18n.t("settings.export_cancelled")}
                    }
                }
                if let Some(report) = &report {
                    ExportReport { report: report.clone(), destinations: destinations.clone() }
                }
            }

            if let (Ok(tid), Some(Some(Ok(tree)))) = (tree_id.parse::<Uuid>(), &*tree_resource.read()) {
                SubmitterCard { key: "{tid}", tree_id: tid, tree: tree.clone() }
            }
        }
    }
}

/// The side of a branch's root person a branch export walks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BranchMode {
    Descendants,
    Ancestors,
    Both,
}

/// What an export is limited to (docs/tags.md §2.3), as the export section
/// holds it until the export is made.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum ExportScope {
    /// Every person of the tree.
    #[default]
    Whole,
    /// A root person and the persons on one side of them.
    Branch {
        root: Option<Uuid>,
        mode: BranchMode,
        /// Generations walked; `None` for all of them.
        depth: Option<i64>,
        /// The typed generations are neither empty nor a number from 1 to
        /// 30: the export stays blocked until they are corrected.
        depth_error: bool,
        include_self: bool,
        include_spouses: bool,
    },
    /// The persons carrying a tag.
    Tag(Option<Uuid>),
}

impl ExportScope {
    /// The branch scope a select of `branch` picks, with the defaults of a
    /// branch: all descendants, the root included.
    fn branch() -> Self {
        Self::Branch {
            root: None,
            mode: BranchMode::Descendants,
            depth: None,
            depth_error: false,
            include_self: true,
            include_spouses: false,
        }
    }

    /// Whether the generations typed for a branch are out of range.
    fn depth_error(self) -> bool {
        matches!(
            self,
            Self::Branch {
                depth_error: true,
                ..
            }
        )
    }

    /// The target the export sends: `Some(None)` for the whole tree, and
    /// `None` while the scope names no root person or no tag, or while the
    /// typed generations are invalid.
    fn target(self) -> Option<Option<String>> {
        let target = match self {
            Self::Whole => return Some(None),
            Self::Tag(tag) => json!({ "tag": tag? }),
            Self::Branch {
                root,
                mode,
                depth,
                depth_error,
                include_self,
                include_spouses,
            } => {
                if depth_error {
                    return None;
                }
                let key = match mode {
                    BranchMode::Descendants => "descendants_of",
                    BranchMode::Ancestors => "ancestors_of",
                    BranchMode::Both => "ancestors_and_descendants_of",
                };
                let mut target = json!({ (key): root?, "include_self": include_self });
                if let Some(depth) = depth {
                    target["depth"] = json!(depth);
                }
                if mode == BranchMode::Descendants {
                    target["include_spouses"] = json!(include_spouses);
                }
                target
            }
        };
        Some(Some(target.to_string()))
    }
}

/// The label of a branch's root person in the picker: the person's row once
/// their profile has loaded, their id until then, or while unknown.
fn root_person_row(
    root: Uuid,
    loaded: &Option<Option<PersonSearchSummary>>,
    i18n: &I18n,
) -> Element {
    let summary = match loaded {
        Some(Some(summary)) => summary.clone(),
        Some(None) => PersonSearchSummary::placeholder(root, i18n.t("common.unknown")),
        None => PersonSearchSummary::placeholder(root, i18n.t("common.loading")),
    };
    render_person_search_summary(&summary, None, i18n)
}

/// The scope picker of the export section: the whole tree, a branch of one
/// person, or the persons carrying a tag.
#[component]
fn ExportScopePicker(mut scope: Signal<ExportScope>, tree_id: Uuid) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let load_trace = use_context::<UiLoadTrace>();
    let tags_api = api.clone();
    let tags = use_ui_resource("export_scope_tags", move || {
        let api = tags_api.clone();
        async move { api.list_tags(tree_id).await.unwrap_or_default() }
    });
    let root_api = api.clone();
    let root_person = use_traced_resource(load_trace, "export_scope_root", move || {
        let api = root_api.clone();
        let root = match scope() {
            ExportScope::Branch { root, .. } => root,
            _ => None,
        };
        async move {
            let root = root?;
            api.get_person_profile(tree_id, root)
                .await
                .ok()
                .map(PersonSearchSummary::from)
        }
    });

    let current = scope();
    let kind = match current {
        ExportScope::Whole => "whole",
        ExportScope::Branch { .. } => "branch",
        ExportScope::Tag(_) => "tag",
    };
    rsx! {
        div { class: "settings-check-first",
            label { class: "settings-check-label", r#for: "export-scope",
                {i18n.t("settings.export_scope")}
            }
            select {
                id: "export-scope",
                class: "settings-date-select",
                onchange: move |e: Event<FormData>| {
                    scope.set(match e.value().as_str() {
                        "branch" => ExportScope::branch(),
                        "tag" => ExportScope::Tag(None),
                        _ => ExportScope::Whole,
                    });
                },
                option { value: "whole", selected: kind == "whole", {i18n.t("settings.export_scope_whole")} }
                option { value: "branch", selected: kind == "branch", {i18n.t("settings.export_scope_branch")} }
                option { value: "tag", selected: kind == "tag", {i18n.t("settings.export_scope_tag")} }
            }
            if let ExportScope::Branch { root, mode, depth, depth_error, include_self, include_spouses } = current {
                ExportBranchFields {
                    scope,
                    tree_id,
                    root,
                    root_person,
                    mode,
                    depth,
                    depth_error,
                    include_self,
                    include_spouses,
                }
            }
            if let ExportScope::Tag(chosen) = current {
                ExportTagSelect { scope, chosen, tags }
            }
        }
    }
}

/// The fields of a branch scope: the root person, the side walked, the
/// generations and the two inclusions.
#[component]
fn ExportBranchFields(
    mut scope: Signal<ExportScope>,
    tree_id: Uuid,
    root: Option<Uuid>,
    root_person: Resource<Option<PersonSearchSummary>>,
    mode: BranchMode,
    depth: Option<i64>,
    depth_error: bool,
    include_self: bool,
    include_spouses: bool,
) -> Element {
    let i18n = use_i18n();
    // The field shows what was typed, so an invalid value stays visible.
    let mut depth_text = use_signal(|| depth.map(|depth| depth.to_string()).unwrap_or_default());
    let selected = root.map(|root| root_person_row(root, &root_person.read(), &i18n));
    rsx! {
        PersonPicker {
            tree_id,
            selected,
            search_placeholder: i18n.t("settings.export_scope_search"),
            change_label: i18n.t("settings.export_scope_change"),
            on_change: move |person: Option<Uuid>| {
                scope.with_mut(|scope| {
                    if let ExportScope::Branch { root, .. } = scope {
                        *root = person;
                    }
                });
            },
        }
        label { class: "settings-check-label", r#for: "export-scope-mode",
            {i18n.t("settings.export_scope_mode")}
        }
        select {
            id: "export-scope-mode",
            class: "settings-date-select",
            onchange: move |e: Event<FormData>| {
                let mode = match e.value().as_str() {
                    "ancestors" => BranchMode::Ancestors,
                    "both" => BranchMode::Both,
                    _ => BranchMode::Descendants,
                };
                scope.with_mut(|scope| {
                    if let ExportScope::Branch { mode: current, .. } = scope {
                        *current = mode;
                    }
                });
            },
            option { value: "descendants", selected: mode == BranchMode::Descendants, {i18n.t("settings.export_scope_descendants")} }
            option { value: "ancestors", selected: mode == BranchMode::Ancestors, {i18n.t("settings.export_scope_ancestors")} }
            option { value: "both", selected: mode == BranchMode::Both, {i18n.t("settings.export_scope_both")} }
        }
        label { class: "settings-check-label", r#for: "export-scope-depth",
            {i18n.t("settings.export_scope_depth")}
        }
        input {
            id: "export-scope-depth",
            class: "settings-date-select",
            r#type: "number",
            min: "1",
            max: "30",
            placeholder: i18n.t("settings.export_scope_depth_all"),
            value: "{depth_text}",
            onchange: move |e: Event<FormData>| {
                let text = e.value();
                let (depth, depth_error) = parse_depth(text.trim());
                depth_text.set(text);
                scope.with_mut(|scope| {
                    if let ExportScope::Branch { depth: current, depth_error: error, .. } = scope {
                        *current = depth;
                        *error = depth_error;
                    }
                });
            },
        }
        if depth_error {
            p { class: "field-hint field-hint-warn", {i18n.t("settings.export_scope_depth_invalid")} }
        }
        ExportToggle {
            checked: include_self,
            label: i18n.t("settings.export_scope_include_self"),
            on_change: move |checked: bool| {
                scope.with_mut(|scope| {
                    if let ExportScope::Branch { include_self, .. } = scope {
                        *include_self = checked;
                    }
                });
            },
        }
        if mode == BranchMode::Descendants {
            ExportToggle {
                checked: include_spouses,
                label: i18n.t("settings.export_scope_include_spouses"),
                on_change: move |checked: bool| {
                    scope.with_mut(|scope| {
                        if let ExportScope::Branch { include_spouses, .. } = scope {
                            *include_spouses = checked;
                        }
                    });
                },
            }
        }
    }
}

/// The generations a branch walks, from the text typed for them: `None` for
/// an empty field (all of them), and an error for anything that is neither
/// empty nor a number from 1 to 30.
fn parse_depth(text: &str) -> (Option<i64>, bool) {
    if text.is_empty() {
        return (None, false);
    }
    match text.parse::<i64>() {
        Ok(depth) if (1..=30).contains(&depth) => (Some(depth), false),
        _ => (None, true),
    }
}

/// The tag a tag scope names: a select of the tags carried by at least one
/// person, the labels as they were written.
#[component]
fn ExportTagSelect(
    mut scope: Signal<ExportScope>,
    chosen: Option<Uuid>,
    tags: Resource<Vec<TagSummary>>,
) -> Element {
    let i18n = use_i18n();
    let options = carried_tags(tags.read().as_deref().unwrap_or_default());
    let empty = options.is_empty();
    rsx! {
        label { class: "settings-check-label", r#for: "export-scope-tag", {i18n.t("stats.tag")} }
        TagSelect {
            id: "export-scope-tag",
            tags: options,
            selected: chosen,
            placeholder: i18n.t("settings.export_scope_pick_tag"),
            on_change: move |value| scope.set(ExportScope::Tag(value)),
            class: "settings-date-select",
        }
        if empty {
            p { class: "settings-check-desc", {i18n.t("settings.export_scope_no_tag")} }
        }
    }
}

/// The destinations the server listed; without them (still loading, or
/// unreachable), the default alone.
fn listed_or_default(listed: Option<Vec<ExportDestination>>) -> Vec<ExportDestination> {
    listed.filter(|list| !list.is_empty()).unwrap_or_else(|| {
        vec![ExportDestination {
            id: DEFAULT_DESTINATION.to_string(),
            label: String::new(),
            version: "5.5.1".to_string(),
            standard: true,
            to_be_verified: false,
        }]
    })
}

/// The destination program a GEDCOM export is written for: a select of the
/// `destinations`, the chosen one's description beneath it and, for a
/// program whose profile is still to be verified, a hint saying so.
#[component]
fn DestinationPicker(
    mut destination: Signal<String>,
    destinations: Vec<ExportDestination>,
) -> Element {
    let i18n = use_i18n();
    let chosen = destinations
        .iter()
        .find(|candidate| candidate.id == destination())
        .cloned();
    let description = chosen
        .as_ref()
        .and_then(|chosen| destination_description(&i18n, &chosen.id));
    let to_be_verified = chosen.is_some_and(|chosen| chosen.to_be_verified);
    rsx! {
        div { class: "settings-check-first",
            label { class: "settings-check-label", r#for: "export-destination",
                {i18n.t("settings.export_destination")}
            }
            select {
                id: "export-destination",
                class: "settings-date-select settings-destination-select",
                onchange: move |e: Event<FormData>| destination.set(e.value()),
                for candidate in destinations.iter() {
                    option {
                        key: "{candidate.id}",
                        value: "{candidate.id}",
                        selected: candidate.id == destination(),
                        {destination_name(&i18n, candidate)}
                    }
                }
            }
            if let Some(description) = description {
                p { class: "settings-check-desc", "{description}" }
            }
            if to_be_verified {
                p { class: "field-hint field-hint-warn",
                    {i18n.t("settings.export_destination_to_verify")}
                }
            }
        }
    }
}

/// What a GEDCOM export lost writing for its destination: the count of the
/// changes that lost something and of the destination's limits the file
/// exceeds, each behind a disclosure listing them. A program's export that
/// lost nothing says so; a standard one, which reshapes nothing, says
/// nothing.
#[component]
fn ExportReport(report: DestinationReport, destinations: Vec<ExportDestination>) -> Element {
    let i18n = use_i18n();
    let listed = destinations
        .iter()
        .find(|candidate| candidate.id == report.destination);
    let standard = listed.is_some_and(|listed| listed.standard);
    let destination = listed.map_or_else(
        || report.destination.clone(),
        |listed| destination_name(&i18n, listed),
    );
    if report.losses.is_empty() && report.limits.is_empty() {
        if standard {
            return rsx! {};
        }
        return rsx! {
            p { class: "settings-card-desc settings-feedback",
                {i18n.t_args("settings.export_report_lossless", &[("destination", &destination)])}
            }
        };
    }
    let counted = |key: &str, count: usize| {
        i18n.t_args(
            &i18n.plural_key(key, count),
            &[("count", &count.to_string()), ("destination", &destination)],
        )
    };
    let loss = |kind: &str, record: Option<&str>, detail: &str| {
        let kind = i18n
            .try_t(&format!("export_report.kind.{kind}"))
            .unwrap_or_else(|| kind.to_string());
        match record {
            Some(record) => i18n.t_args(
                "settings.export_report_item_record",
                &[("kind", &kind), ("record", record), ("detail", detail)],
            ),
            None => i18n.t_args(
                "settings.export_report_item",
                &[("kind", &kind), ("detail", detail)],
            ),
        }
    };
    let limit = |record: Option<&str>, detail: &str| match record {
        Some(record) => i18n.t_args(
            "settings.export_report_limit_record",
            &[("record", record), ("detail", detail)],
        ),
        None => detail.to_string(),
    };
    rsx! {
        if !report.limits.is_empty() {
            details { class: "import-warnings",
                summary { {counted("settings.export_report_limits", report.limits.len())} }
                ul {
                    for item in report.limits.iter() {
                        li { {limit(item.record.as_deref(), &item.detail)} }
                    }
                }
            }
        }
        if !report.losses.is_empty() {
            details { class: "import-warnings",
                summary { {counted("settings.export_report_losses", report.losses.len())} }
                ul {
                    for item in report.losses.iter().take(500) {
                        li { {loss(&item.kind, item.record.as_deref(), &item.detail)} }
                    }
                }
            }
        }
    }
}

/// A checkbox with its label and, when it has one, the description beneath
/// it. The first of the options is set off from the format row above it.
#[component]
fn ExportToggle(
    checked: bool,
    label: String,
    description: Option<String>,
    on_change: EventHandler<bool>,
    #[props(default)] first: bool,
) -> Element {
    rsx! {
        label {
            class: if first { "settings-check settings-check-first" } else { "settings-check" },
            input {
                r#type: "checkbox",
                checked,
                onchange: move |e: Event<FormData>| on_change.call(e.checked()),
            }
            div {
                div { class: "settings-check-label", "{label}" }
                if let Some(description) = description {
                    p { class: "settings-check-desc", "{description}" }
                }
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
    .settings-card { margin-top: var(--space-8); }
    .settings-card-title {
        font-size: var(--text-95);
        margin-bottom: var(--space-3);
        color: var(--text-primary);
    }
    .settings-card-desc {
        font-size: var(--text-80);
        color: var(--text-secondary);
        margin-bottom: var(--space-6);
    }
    .settings-export-row .settings-card-desc { margin-bottom: 0; }
    .settings-feedback,
    .settings-choices { margin-top: var(--space-6); }
    .settings-hint { margin-top: var(--space-4); }
    .settings-export-info { flex: 1; }
    .settings-check {
        display: grid;
        grid-template-columns: 20px 1fr;
        column-gap: var(--space-4);
        align-items: start;
        margin-top: var(--space-6);
        cursor: pointer;
    }
    .settings-check-first {
        margin-top: var(--space-8);
        padding-top: var(--space-8);
        border-top: 1px solid var(--border);
    }
    .settings-check input { margin: 3px 0 0 0; }
    .settings-check-label {
        font-size: var(--text-85);
        color: var(--text-primary);
    }
    .settings-check-desc {
        font-size: var(--text-80);
        color: var(--text-secondary);
        margin-top: var(--space-1);
    }
    .settings-tree-name-form {
        display: flex;
        gap: var(--space-4);
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
        gap: var(--space-8);
    }
    .settings-export-format {
        width: auto;
        flex-shrink: 0;
    }
    .settings-destination-select {
        display: block;
        margin-top: var(--space-3);
    }

    @media (max-width: 768px) {
        .settings-export-row {
            flex-direction: column;
            align-items: stretch;
        }
        .settings-export-format {
            width: 100%;
        }
    }
"#;
