//! The import modal: pick a file, or walk the Geneanet flow.
//!
//! Importing a Geneanet tree with its photos requires more than a file import.
//! Two of its three inputs cannot be downloaded, one of them exists only
//! behind a logged-in session, and most users have never heard of a `.gw`
//! file. So the Geneanet side is as much a set of instructions as a form.
//!
//! See `docs/ui-import.md`.
//!
//! # Shape
//!
//! Two tabs. **File** accepts imports in the browser and on the desktop.
//! **Geneanet** is four to six steps of which exactly one is expanded — the
//! current one. A settled step collapses to a one-line receipt of what it
//! decided; steps not yet reachable are visible but dimmed, so the whole
//! journey is legible from the first second.
//!
//! # Which steps
//!
//! The `.gw` step asks which bytes to keep for each photograph, and that
//! answer decides how long the journey is. Keeping Geneanet's own renditions
//! needs nothing but the login, so there is no archives step. Keeping the
//! original uploads needs the user's data archives — another Geneanet
//! request, an email, and gigabytes of ZIP — so the archives step appears
//! and the numbering shifts with it. Renditions is the default because it is
//! the only path a user who has never downloaded their data can take.
//!
//! The saved-connection step comes right after: a file saved by an earlier
//! run holds what the login collects and, when the archives were added
//! before saving, their originals, so it can stand in for both. Once loaded
//! it says how much of the import it covers, and the steps after it ask
//! only for the rest. The desktop goes on without one in a click.
//!
//! # What only the desktop build can do
//!
//! The archives and the login are desktop-only, and the web build shows
//! neither step:
//!
//! - **The archives step** needs to read multi-gigabyte ZIPs by path, a few kilobytes
//!   each. A browser would have to load them whole into memory to see the same
//!   central directory.
//! - **The login step** needs a second browser window on geneanet.org whose session
//!   this app can then issue requests through. A popup from a web page is
//!   cross-origin: nothing comes back out of it.
//!
//! On web the saved connection is the one way to the media: the tab imports
//! the `.gw` with the media of a file saved in the desktop application, and
//! without one points to the **File** tab, where the genealogy arrives
//! without its photos.

use std::collections::HashMap;

use dioxus::prelude::*;
use oxidgene_core::types::last_path_segment;
#[cfg(target_arch = "wasm32")]
use serde::Deserialize;
use uuid::Uuid;

use crate::api::{
    ApiClient, ApiError, ArchiveIndex, FileImportJobStatus, GeneanetImportResult,
    GeneanetIsolatedPerson, GeneanetPreview, GeneanetPreviewBody, GeneanetSessionBody,
    GwInspection, ImportProgress, ImportResult, IndexedArchive, MediaFidelity, follow_job,
};
use crate::components::cancel_job::{CancelJobButton, JobKind, can_cancel};
use crate::components::homonym_picker::{HomonymDecision, HomonymPicker};
use crate::components::modal::Modal;
use crate::geneanet::{Collect, GeneanetBridge, GeneanetEvent, WindowStrings, use_geneanet_bridge};
use crate::i18n::{I18n, use_i18n};
use crate::ui_observability::{
    UiAction, UiActionStep, UiActionTrace, trace_ui_action, trace_ui_action_step,
    use_ui_action_trace,
};

/// Whether this build can open a Geneanet login window and read local
/// archives by path.
///
/// One constant rather than a `cfg!` at each of the six places that ask,
/// because they must all agree: a tab that offered the login step but hid the archives step
/// would strand the user halfway.
const NATIVE: bool = !cfg!(target_arch = "wasm32");

/// Which half of the modal is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    File,
    Geneanet,
}

/// The Geneanet flow's steps, in order.
///
/// [`Step::Archives`] is skipped entirely by a
/// [`MediaFidelity::Renditions`] import, which has no archive to read, and
/// the web build, which reads none; [`Step::Connect`] by the web build, which
/// cannot sign in — see [`visible_steps`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Step {
    Gw,
    /// A connection saved by an earlier run: it stands in for signing in,
    /// and for the archives when it holds their originals.
    Saved,
    Archives,
    Connect,
    Preview,
    Import,
}

/// A `.gw` the user picked, and what it turned out to hold.
#[derive(Clone, PartialEq)]
struct GwFile {
    name: String,
    bytes: Vec<u8>,
    inspection: GwInspection,
}

/// Everything the login window brought back.
#[derive(Clone, PartialEq, Default)]
struct Collected {
    collection: String,
    deposit_sizes: HashMap<i64, u64>,
    cookie: Option<String>,
    account: Option<String>,
    /// Media the account holds, pages included — reported by the window, not
    /// inferred from how many deposits could be measured.
    photo_count: usize,
    /// Read from a saved connection rather than collected by signing in.
    from_file: bool,
}

/// The saved connection the user loaded, at the saved-connection step.
#[derive(Clone, PartialEq)]
struct SessionFile {
    name: String,
    /// How many media it carries.
    media: usize,
}

/// How much of this import a loaded file covers, for the media answer it
/// was worked out for.
#[derive(Clone, Copy, PartialEq)]
struct Coverage {
    fidelity: MediaFidelity,
    /// Media the import needs that the file does not hold.
    missing: usize,
    /// The same had the import kept the originals, when that is fewer: the
    /// file then holds originals the renditions answer would leave unused.
    with_originals: Option<usize>,
}

/// Where the login window has got to, for the two progress bars of the login step.
#[derive(Clone, Copy, PartialEq)]
enum Connecting {
    /// Window open, user has not signed in yet.
    WaitingForLogin,
    Collecting {
        done: usize,
        total: usize,
    },
    Sizing {
        done: usize,
        total: usize,
    },
}

/// What an import produced, so the caller can refresh and route.
#[derive(Clone, PartialEq)]
pub enum ImportOutcome {
    File(ImportResult),
    Geneanet(GeneanetImportResult),
}

/// The modal.
///
/// `tree_id` is the tree everything lands in. The modal never creates one:
/// it is opened from a tree's own menu, so the target is already chosen.
#[component]
pub fn ImportModal(
    tree_id: Uuid,
    tree_name: String,
    on_close: EventHandler<()>,
    on_imported: EventHandler<ImportOutcome>,
) -> Element {
    let i18n = use_i18n();
    let mut tab = use_signal(|| Tab::File);

    // Set while an import is in flight. Dismissing then would throw away four
    // settled steps and orphan a request that is still writing to the tree,
    // and a stray click on the page behind is all it would take. The way out
    // is the import's own Cancel button, which undoes what it wrote.
    let busy = use_signal(|| false);

    // The scroll offset lives on `.import-modal-body`, which outlives every
    // screen rendered inside it — so a step collapsing, or the result taking
    // over from a scrolled import step, can leave the offset far past the end of
    // much shorter content. The webview does not re-clamp it on its own (the
    // same fault `use_init_textarea_resize_clamp` answers for a resized
    // textarea), and with no scroll range left there is no scrollbar to drag
    // back with: the top of the screen sits above the top edge, the rest of
    // the box is blank, and the modal reads as empty. Clamping on every patch
    // keeps whatever is rendered on screen. The frame-coalescing matters: an
    // import in flight rewrites its progress line continuously, and measuring
    // `scrollHeight` per mutation would force a layout on each one.
    use_effect(|| {
        document::eval(
            r#"
            const body = document.querySelector('.import-modal-body');
            if (body && !body.dataset.oxClamp) {
                body.dataset.oxClamp = '1';
                let queued = false;
                const clamp = () => {
                    queued = false;
                    const max = Math.max(0, body.scrollHeight - body.clientHeight);
                    if (body.scrollTop > max) body.scrollTop = max;
                };
                new MutationObserver(() => {
                    if (queued) return;
                    queued = true;
                    requestAnimationFrame(clamp);
                }).observe(body, { childList: true, subtree: true, characterData: true });
            }
            "#,
        );
    });

    rsx! {
        // Never closed while busy — see `busy`.
        Modal {
            class: "import-modal",
            backdrop_class: "import-modal-backdrop",
            label: i18n.t_args("import.title", &[("tree", &tree_name)]),
            busy: busy(),
            on_close,

            div { class: "import-modal-header",
                h2 {
                    {i18n.t_args("import.title", &[("tree", &tree_name)])}
                }
                button {
                    class: "person-form-close",
                    disabled: busy(),
                    onclick: move |_| {
                        if !busy() {
                            on_close.call(());
                        }
                    },
                    "✕"
                }
            }

            div { class: "import-tabs", role: "tablist",
                button {
                    class: if tab() == Tab::File { "import-tab is-active" } else { "import-tab" },
                    role: "tab",
                    "aria-selected": tab() == Tab::File,
                    onclick: move |_| tab.set(Tab::File),
                    {i18n.t("import.tab_file")}
                }
                button {
                    class: if tab() == Tab::Geneanet { "import-tab is-active" } else { "import-tab" },
                    role: "tab",
                    "aria-selected": tab() == Tab::Geneanet,
                    onclick: move |_| tab.set(Tab::Geneanet),
                    {i18n.t("import.tab_geneanet")}
                }
            }

            div { class: "import-modal-body",
                match tab() {
                    Tab::File => rsx! {
                        FileTab { tree_id, busy, on_imported }
                    },
                    Tab::Geneanet => rsx! {
                        GeneanetTab { tree_id, busy, on_imported }
                    },
                }
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// File tab
// ═══════════════════════════════════════════════════════════════════

/// Drop or pick a `.ged`, `.gdz` or `.gw` and import it.
///
/// The extension picks the reader (see [`format_of`]); only `.gdz` brings the
/// media with it, the other two naming files the tree will not hold.
///
/// The one behavioural change from the menu item this replaces: the bytes come
/// from `read()` rather than from a path. A picked file has no path in a
/// browser at all, so the old `path()` + `tokio::fs::read` made this
/// desktop-only without anything saying so.
#[component]
fn FileTab(tree_id: Uuid, busy: Signal<bool>, on_imported: EventHandler<ImportOutcome>) -> Element {
    let api = use_context::<ApiClient>();
    let i18n = use_i18n();

    let mut picked = use_signal(|| None::<dioxus::html::FileData>);
    let mut dragging = use_signal(|| false);
    let mut busy = busy;
    let mut error = use_signal(|| None::<String>);
    let mut result = use_signal(|| None::<ImportResult>);
    let mut progress = use_signal(|| None::<FileImportUiProgress>);
    // The server job, once the upload has created it: what Cancel stops.
    let mut job = use_signal(|| None::<Uuid>);
    // Set when the last run was cancelled, which left the tree unchanged.
    let mut cancelled = use_signal(|| false);

    // Reading `result` inside the effect is what makes it re-run when the
    // import lands and the receipt replaces the drop zone.
    use_effect(move || {
        if result().is_some() {
            scroll_import_body_to_top();
        }
    });

    let api_import = api.clone();
    let do_import = move |_| {
        let api = api_import.clone();
        let Some(file) = picked() else {
            return;
        };
        if busy() {
            return;
        }
        let name = last_path_segment(&file.name()).to_string();
        busy.set(true);
        error.set(None);
        cancelled.set(false);
        progress.set(None);
        spawn(async move {
            let format = format_of(&name);
            let outcome = trace_ui_action(
                UiAction::Import(format.api_name()),
                run_file_import_job(&api, tree_id, format, file, &mut progress, job),
            )
            .await;
            busy.set(false);
            progress.set(None);
            job.set(None);
            match outcome {
                Ok(Some(imported)) => {
                    api.invalidate_tree(tree_id);
                    result.set(Some(imported.clone()));
                    on_imported.call(ImportOutcome::File(imported));
                }
                // Rolled back whole: the tree reads as it did before.
                Ok(None) => {
                    api.invalidate_tree(tree_id);
                    cancelled.set(true);
                }
                Err(e) => error.set(Some(e.message(&i18n))),
            }
        });
    };

    if let Some(imported) = result() {
        return rsx! {
            div { class: "import-done",
                div { class: "import-done-icon", "✓" }
                h3 { {i18n.t("import.done_title")} }
                ImportCounts { result: imported.clone() }
                ImportWarnings { warnings: imported.warnings }
            }
        };
    }

    rsx! {
        div { class: "import-file",
            div {
                class: if dragging() { "import-drop is-dragging" } else { "import-drop" },
                input {
                    id: "oxidgene-file-import-input",
                    class: "import-file-input",
                    r#type: "file",
                    accept: ".ged,.gdz,.gw",
                    disabled: busy(),
                    ondragenter: move |_| dragging.set(true),
                    ondragleave: move |_| dragging.set(false),
                    onchange: move |event| {
                        dragging.set(false);
                        picked.set(event.files().into_iter().next());
                        error.set(None);
                        cancelled.set(false);
                        result.set(None);
                        progress.set(None);
                    },
                }

                if let Some(file) = picked() {
                    div { class: "import-drop-icon", "📄" }
                    div { class: "import-drop-name", {last_path_segment(&file.name()).to_string()} }
                    div { class: "import-drop-hint",
                        {i18n.t_args("import.file_size", &[("size", &crate::utils::human_size(file.size()))])}
                    }
                } else {
                    div { class: "import-drop-icon", "📄" }
                    div { class: "import-drop-label", {i18n.t("import.drop_label")} }
                    div { class: "import-drop-hint", {i18n.t("import.drop_formats")} }
                }
            }

            if let Some(err) = error() {
                div { class: "error-msg", "{err}" }
            }
            if cancelled() {
                div { class: "success-msg", role: "status", {i18n.t("import.cancelled")} }
            }

            if let Some(current) = progress() {
                FileImportProgress { progress: current }
            }

            div { class: "modal-actions",
                if let Some(job_id) = job().filter(|_| server_phase(progress().as_ref()).is_some_and(can_cancel)) {
                    CancelJobButton { kind: JobKind::Import, tree_id, job_id }
                }
                button {
                    class: "btn btn-primary",
                    disabled: picked().is_none() || busy(),
                    onclick: do_import,
                    if busy() { {i18n.t("import.importing")} } else { {i18n.t("import.start")} }
                }
            }
        }
    }
}

/// The file tab's bar: the upload's bytes, then the server job's phase,
/// determinate only while it stores media.
#[component]
fn FileImportProgress(progress: FileImportUiProgress) -> Element {
    let i18n = use_i18n();
    match progress {
        FileImportUiProgress::Upload { done, total } => rsx! {
            ProgressBar {
                label: i18n.t("import.phase_upload"),
                done,
                total,
            }
        },
        FileImportUiProgress::Server { phase, done, total } => rsx! {
            ProgressBar {
                label: i18n.t(match phase.as_str() {
                    "parsing" => "import.phase_parsing",
                    "media" => "import.phase_media",
                    "database" => "import.phase_database",
                    "projections" => "import.phase_projections",
                    "cancelling" => "import.phase_cancelling",
                    _ => "import.phase_starting",
                }),
                done,
                total: if phase == "media" { total } else { 0 },
            }
        },
    }
}

/// The phase the server job last reported; none while uploading.
fn server_phase(progress: Option<&FileImportUiProgress>) -> Option<&str> {
    match progress? {
        FileImportUiProgress::Server { phase, .. } => Some(phase),
        FileImportUiProgress::Upload { .. } => None,
    }
}

#[derive(Clone, PartialEq)]
enum FileImportUiProgress {
    Upload {
        done: usize,
        total: usize,
    },
    Server {
        phase: String,
        done: usize,
        total: usize,
    },
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[cfg(target_arch = "wasm32")]
enum UploadEvent {
    Upload { done: usize, total: usize },
    Started { job_id: Uuid },
    Error,
}

/// Uploads `file`, then follows its import job — published in `job` for the
/// Cancel button — to the end: the import's result, or `None` when it was
/// cancelled and rolled back.
async fn run_file_import_job(
    api: &ApiClient,
    tree_id: Uuid,
    format: FileFormat,
    file: dioxus::html::FileData,
    progress: &mut Signal<Option<FileImportUiProgress>>,
    mut job: Signal<Option<Uuid>>,
) -> Result<Option<ImportResult>, ApiError> {
    let job_id = trace_ui_action_step(
        UiActionStep::ImportUpload,
        start_file_import_upload(api, tree_id, format, file, progress),
    )
    .await?;
    job.set(Some(job_id));
    api.invalidate_tree_list();

    let ended = trace_ui_action_step(
        UiActionStep::ImportPoll,
        follow_import_job(api, tree_id, job_id, |status| {
            progress.set(Some(FileImportUiProgress::Server {
                phase: status.phase.clone(),
                done: status.done,
                total: status.total,
            }));
        }),
    )
    .await?;
    ended
        .map(|status| {
            status
                .result
                .ok_or(ApiError::Client("error.unexpected_response"))
        })
        .transpose()
}

/// Follows import job `job_id` until it ends, handing every status it reads
/// to `report`: the status it completed with, or `None` when it was
/// cancelled — which undid everything it wrote.
///
/// `cancelling` is not an end: the job is still unwinding, and the tree is
/// only as it was once it reads `cancelled`. Each read after the first waits
/// on the server until the job changes ([`follow_job`]).
async fn follow_import_job(
    api: &ApiClient,
    tree_id: Uuid,
    job_id: Uuid,
    report: impl FnMut(&FileImportJobStatus),
) -> Result<Option<FileImportJobStatus>, ApiError> {
    let status = follow_job(|wait| api.file_import_status(tree_id, job_id, wait), report).await?;
    match status.phase.as_str() {
        "cancelled" => Ok(None),
        "failed" => Err(ApiError::job_failed(status.error)),
        _ => Ok(Some(status)),
    }
}

#[cfg(target_arch = "wasm32")]
async fn start_file_import_upload(
    api: &ApiClient,
    tree_id: Uuid,
    format: FileFormat,
    _file: dioxus::html::FileData,
    progress: &mut Signal<Option<FileImportUiProgress>>,
) -> Result<Uuid, crate::api::ApiError> {
    let endpoint = serde_json::to_string(&api.file_import_upload_url(tree_id))
        .expect("an endpoint URL is JSON serializable");
    let format = serde_json::to_string(format.api_name())
        .expect("a static import format is JSON serializable");
    // The browser sends the file itself, so the session's CSRF token is set
    // here rather than by the API client (docs/authentication.md §4.4).
    let csrf = serde_json::to_string(&api.csrf_token()).expect("a token is JSON serializable");
    let script = format!(
        r#"
        const input = document.getElementById('oxidgene-file-import-input');
        const file = input?.files?.[0];
        if (!file) {{
            dioxus.send({{ kind: 'error' }});
            return;
        }}
        const params = new URLSearchParams({{ format: {format} }});
        if ({format} === 'geneweb') params.set('filename', file.name);
        const xhr = new XMLHttpRequest();
        xhr.open('POST', {endpoint} + '?' + params.toString());
        xhr.setRequestHeader('Content-Type', 'application/octet-stream');
        const csrf = {csrf};
        if (csrf) xhr.setRequestHeader('X-OxidGene-CSRF', csrf);
        xhr.responseType = 'json';
        xhr.upload.onprogress = event => dioxus.send({{
            kind: 'upload',
            done: event.loaded,
            total: event.lengthComputable ? event.total : file.size,
        }});
        xhr.onerror = () => dioxus.send({{ kind: 'error' }});
        xhr.onload = () => {{
            if (xhr.status !== 202 || !xhr.response?.job_id) {{
                dioxus.send({{ kind: 'error' }});
                return;
            }}
            dioxus.send({{ kind: 'started', job_id: xhr.response.job_id }});
        }};
        xhr.send(file);
        "#,
    );
    let mut eval = document::eval(&script);
    loop {
        match eval.recv::<UploadEvent>().await {
            Ok(UploadEvent::Upload { done, total }) => {
                progress.set(Some(FileImportUiProgress::Upload { done, total }));
            }
            Ok(UploadEvent::Started { job_id }) => break Ok(job_id),
            Ok(UploadEvent::Error) | Err(_) => {
                break Err(crate::api::ApiError::Client("error.upload_failed"));
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
async fn start_file_import_upload(
    api: &ApiClient,
    tree_id: Uuid,
    format: FileFormat,
    file: dioxus::html::FileData,
    progress: &mut Signal<Option<FileImportUiProgress>>,
) -> Result<Uuid, crate::api::ApiError> {
    use futures_util::TryStreamExt as _;

    let total = usize::try_from(file.size()).unwrap_or(usize::MAX);
    progress.set(Some(FileImportUiProgress::Upload { done: 0, total }));
    let filename = (format == FileFormat::Geneweb).then(|| file.name());
    let uploaded = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let streamed = std::sync::Arc::clone(&uploaded);
    let stream = file
        .byte_stream()
        .inspect_ok(move |chunk| {
            streamed.fetch_add(chunk.len(), std::sync::atomic::Ordering::Relaxed);
        })
        .map_err(|error| std::io::Error::other(error.to_string()));
    let mut request = std::pin::pin!(api.start_file_import(
        tree_id,
        format.api_name(),
        filename,
        reqwest::Body::wrap_stream(stream),
    ));
    let started = loop {
        tokio::select! {
            result = &mut request => break result?,
            () = tokio::time::sleep(std::time::Duration::from_millis(50)) => {
                progress.set(Some(FileImportUiProgress::Upload {
                    done: uploaded.load(std::sync::atomic::Ordering::Relaxed),
                    total,
                }));
            }
        }
    };
    progress.set(Some(FileImportUiProgress::Upload { done: total, total }));
    Ok(started.job_id)
}

/// The counts every import reports, however it was sourced, then those
/// only this source has, `children`.
#[component]
fn ImportCounts(result: ImportResult, children: Element) -> Element {
    let i18n = use_i18n();
    let documents = if result.document_pages_count == 0 {
        i18n.t("import.stat_documents")
    } else {
        i18n.t_plural("import.stat_documents_pages", result.document_pages_count)
    };

    rsx! {
        div { class: "import-stats",
            Stat { value: result.persons_count, label: i18n.t("import.stat_persons") }
            Stat { value: result.families_count, label: i18n.t("import.stat_families") }
            Stat { value: result.events_count, label: i18n.t("import.stat_events") }
            Stat { value: result.sources_count, label: i18n.t("import.stat_sources") }
            Stat { value: result.places_count, label: i18n.t("import.stat_places") }
            Stat { value: result.images_count, label: i18n.t("import.stat_images") }
            Stat { value: result.documents_count, label: documents }
            {children}
        }
    }
}

/// An import's warnings, behind a disclosure with their count.
#[component]
fn ImportWarnings(warnings: Vec<String>) -> Element {
    let i18n = use_i18n();
    if warnings.is_empty() {
        return rsx! {};
    }
    rsx! {
        details { class: "import-warnings",
            summary {
                {i18n.t_plural("import.warning_count", warnings.len())}
            }
            ul {
                for warning in warnings.iter().take(100) {
                    li { "{warning}" }
                }
            }
        }
    }
}

#[component]
fn Stat(value: usize, label: String) -> Element {
    rsx! {
        div { class: "import-stat",
            div { class: "import-stat-value", {group_digits(value)} }
            div { class: "import-stat-label", "{label}" }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// Geneanet tab
// ═══════════════════════════════════════════════════════════════════

#[component]
fn GeneanetTab(
    tree_id: Uuid,
    busy: Signal<bool>,
    on_imported: EventHandler<ImportOutcome>,
) -> Element {
    let api = use_context::<ApiClient>();
    let i18n = use_i18n();
    let bridge = use_geneanet_bridge();

    // One trace for the whole assistant, not one per button. Provided here so
    // every step component reaches the same root, and closed when this tab
    // goes away — abandoning the assistant ends its trace.
    let trace = use_ui_action_trace(UiAction::GeneanetImport);

    let mut open = use_signal(|| Step::Gw);

    let gw = use_signal(|| None::<GwFile>);
    let gw_error = use_signal(|| None::<String>);
    let mut fidelity = use_signal(MediaFidelity::default);

    let archives = use_signal(Vec::<IndexedArchive>::new);
    let archives_skipped = use_signal(|| false);
    let archive_error = use_signal(|| None::<String>);

    // The saved connection loaded, or the choice to go without one,
    // and how much of the import the file covers.
    let session_file = use_signal(|| None::<SessionFile>);
    let mut saved_skipped = use_signal(|| false);
    let coverage = use_signal(|| None::<Coverage>);
    let saved_error = use_signal(|| None::<String>);

    let collected = use_signal(|| None::<Collected>);
    let mut connecting = use_signal(|| None::<Connecting>);
    let mut connect_error = use_signal(|| None::<String>);

    let mut preview = use_signal(|| None::<GeneanetPreview>);
    let mut preview_error = use_signal(|| None::<String>);
    let mut override_mismatch = use_signal(|| false);

    let mut importing = busy;
    // Media the login window retrieved or a loaded file holds, keyed by URL:
    // a staged local path on the desktop, whose window session owns the
    // files it fetched through the import step; an empty handle on the web, where a
    // loaded file's media stay in it.
    let fetched = use_signal(HashMap::<String, String>::new);
    let mut gathering = use_signal(|| false);
    let mut import_progress = use_signal(|| None::<crate::api::ImportProgress>);
    // Progress through the fetch pass that precedes the write, when the login
    // window has to retrieve media the archives did not cover.
    let mut fetch_progress = use_signal(|| None::<(usize, usize)>);
    // Media the import needs that nothing can fetch: those a loaded file
    // lacks, in a build with no login window.
    let mut missing = use_signal(|| 0usize);
    let mut import_error = use_signal(|| None::<String>);
    let import_result = use_signal(|| None::<GeneanetImportResult>);
    // The import job once queued, which Cancel stops; and whether the last
    // run was cancelled, leaving the tree unchanged.
    let mut import_job = use_signal(|| None::<Uuid>);
    let mut import_cancelled = use_signal(|| false);

    // Set once the window has been closed, so neither of the two paths that
    // close it can do so twice.
    let window_closed = use_signal(|| false);

    // The window exists for one thing: fetching what the archives cannot
    // account for. The preview is the first moment we know that is nothing —
    // every attached deposit single-page and every one of them size-matched —
    // and there is no reason to leave a window open with no job left.
    {
        let bridge = bridge.clone();
        use_effect(move || {
            let idle = preview().is_some_and(|stats| stats.to_match + stats.to_download == 0);
            if idle && !window_closed() {
                close_window(bridge.as_ref(), window_closed);
            }
        });
    }

    // Dismissing the wizard closes the window with it. Without this it would
    // outlive the modal that opened it, with nothing left able to reach it.
    // It also releases the photos a loaded session staged: once an import is
    // queued they are already gone, and otherwise nothing will read them.
    {
        let bridge = bridge.clone();
        let api = api.clone();
        use_drop(move || {
            bridge.iter().for_each(GeneanetBridge::close);
            release_gathered_media(&api, fetched);
            forget_loaded_file();
        });
    }

    use_file_coverage(
        gw,
        Gathered {
            session_file,
            collected,
            fetched,
        },
        fidelity,
        coverage,
        saved_error,
    );
    let covered = move || coverage().is_some_and(|c| c.missing == 0);
    let saved_settled = move || session_file().is_some() || saved_skipped();

    let can_connect = move || {
        login_reachable(
            gw().is_some(),
            saved_settled(),
            archives_settled(
                fidelity(),
                !archives.read().is_empty(),
                archives_skipped(),
                covered(),
            ),
        )
    };
    let can_preview = move || collected().is_some();
    let after_saved = move || step_after_saved(fidelity(), covered(), collected().is_some());

    // Changing the answer to the `.gw` step invalidates every decision that depended
    // on it, so they are dropped rather than left to look settled.
    let api_fidelity = api.clone();
    let choose_fidelity = move |next: MediaFidelity| {
        if next == fidelity() {
            return;
        }
        fidelity.set(next);

        // Which media are needed, and therefore what has been gathered for
        // them, is a different set under each answer: renditions want one
        // `normal` per page, originals want deposit downloads.
        preview.set(None);
        preview_error.set(None);
        override_mismatch.set(false);
        forget_for_answer(
            &api_fidelity,
            next,
            Gathered {
                session_file,
                collected,
                fetched,
            },
            archives,
            archives_skipped,
        );
    };

    // ── The login — drive the login window ───────────────────────────
    let start_connect = {
        let bridge = bridge.clone();
        let trace = trace.clone();
        move |_| {
            let Some(bridge) = bridge.clone() else { return };
            let trace = trace.clone();
            let (tx, rx) = futures_channel::mpsc::unbounded::<GeneanetEvent>();
            connect_error.set(None);
            connecting.set(Some(Connecting::WaitingForLogin));
            // The window shows words, not numbers — the bars below are the
            // only place a count appears.
            bridge.start(
                tx,
                WindowStrings {
                    title: i18n.t("geneanet.window_title"),
                    heading: i18n.t("geneanet.window_heading"),
                    reading_list: i18n.t("geneanet.step3_stage1"),
                    matching: i18n.t("geneanet.step3_stage2"),
                    cancel_hint: i18n.t("geneanet.window_cancel_hint"),
                    idle: i18n.t("geneanet.window_idle"),
                },
                collect_for(fidelity()),
            );

            spawn(async move {
                trace
                    .step(
                        UiActionStep::GeneanetConnect,
                        follow_login(rx, i18n, connecting, collected, connect_error, open),
                    )
                    .await;
            });
        }
    };

    // ── The preview — work out what will happen, and gather for it ───
    //
    // The gathering is here rather than in the import step on purpose. It is the last
    // thing that touches Geneanet, so doing it here means the login window is
    // finished with before the user commits to anything — and the import step becomes
    // purely local, which is what makes an air-gapped import possible at all.
    // Nothing is written until the import step either way.
    let api_preview = api.clone();
    let bridge_preview = bridge.clone();
    let trace_preview = trace.clone();
    let run_preview = move |_| {
        let api = api_preview.clone();
        let bridge = bridge_preview.clone();
        let trace = trace_preview.clone();
        let (Some(file), Some(collected)) = (gw(), collected()) else {
            return;
        };
        if gathering() {
            return;
        }
        preview_error.set(None);

        spawn(async move {
            let paths: Vec<String> = archives.read().iter().map(|a| a.path.clone()).collect();
            let body = GeneanetPreviewBody {
                gw_base64: encode_gw(&file.bytes),
                file_name: file.name.clone(),
                collection: collected.collection.clone(),
                deposit_sizes: collected.deposit_sizes.clone(),
                archive_paths: paths.clone(),
                media_fidelity: fidelity(),
            };

            let planned = trace
                .step(
                    UiActionStep::GeneanetPreview,
                    preview_and_plan(&api, &body, preview),
                )
                .await;
            let Some(needed) = planned
                .map_err(|e| preview_error.set(Some(e.message(&i18n))))
                .ok()
            else {
                return;
            };
            let urls = urls_to_fetch(&needed, &fetched.read());
            missing.set(unfetchable(bridge.as_ref(), &urls));
            let (Some(bridge), false) = (bridge.clone(), urls.is_empty()) else {
                return;
            };

            gathering.set(true);
            let (tx, rx) = futures_channel::mpsc::unbounded::<GeneanetEvent>();
            fetch_progress.set(Some((0, urls.len())));
            bridge.fetch(urls, tx);
            trace
                .step(
                    UiActionStep::GeneanetCollect,
                    gather_media(rx, i18n, fetched, fetch_progress, preview_error),
                )
                .await;

            fetch_progress.set(None);
            gathering.set(false);

            // The window has no more network work, but its session owns the
            // staged files until the local import has consumed them.
        });
    };

    // ── The import — write, with no network left to wait on ──────────
    let api_import = api.clone();
    let bridge_import = bridge.clone();
    let trace_import = trace.clone();
    let run_import = move |_| {
        let api = api_import.clone();
        let bridge = bridge_import.clone();
        let trace = trace_import.clone();
        let (Some(file), true) = (gw(), collected.peek().is_some()) else {
            return;
        };
        if importing() {
            return;
        }
        importing.set(true);
        import_error.set(None);
        import_cancelled.set(false);

        import_progress.set(Some(ImportProgress {
            phase: "staging".to_string(),
            done: 0,
            total: 0,
        }));

        spawn(async move {
            // The desktop names the staged and fetched files by path.
            #[cfg(not(target_arch = "wasm32"))]
            let start = {
                let collected = collected().unwrap_or_default();
                let body = crate::api::GeneanetImportBody {
                    gw_base64: encode_gw(&file.bytes),
                    file_name: file.name.clone(),
                    collection: collected.collection,
                    deposit_sizes: collected.deposit_sizes,
                    archive_paths: archives.read().iter().map(|a| a.path.clone()).collect(),
                    fetched: fetched.read().clone(),
                    media_fidelity: fidelity(),
                };
                let api = api.clone();
                async move { api.import_geneanet(tree_id, &body).await }
            };
            // The web build exchanges no path: it sends the `.gw` and the
            // loaded session file, whose media the server extracts.
            #[cfg(target_arch = "wasm32")]
            let start = start_session_import(&api, tree_id, &file, fidelity(), import_progress);
            let outcome = upload_and_follow(
                &api,
                &trace,
                tree_id,
                start,
                || close_window(bridge.as_ref(), window_closed),
                import_progress,
                import_job,
            )
            .await;
            importing.set(false);
            import_progress.set(None);
            import_job.set(None);

            settle_import(
                &i18n,
                outcome,
                &api,
                tree_id,
                ImportEndSignals {
                    result: import_result,
                    error: import_error,
                    cancelled: import_cancelled,
                },
                on_imported,
            );
            // The assistant is over either way: closing the trace here rather
            // than waiting for the tab to unmount keeps the receipt the user
            // is now reading out of the import's duration.
            trace.finish();
        });
    };

    if let Some(result) = import_result() {
        return rsx! { GeneanetDone { tree_id, result } };
    }

    // How many steps this journey has, and therefore what each one is called.
    let archives_step = fidelity().uses_archives();
    let connects = bridge.is_some();
    let steps = visible_steps(archives_step, connects);
    let number = move |step: Step| step_number(&steps, step);
    // What a saved file carries the originals of: the data archives, when
    // this run reads them.
    let saved_archive_paths: Vec<String> = if archives_step {
        archives.read().iter().map(|a| a.path.clone()).collect()
    } else {
        Vec::new()
    };

    rsx! {
        div { class: "gn-steps",

            // ── The .gw file and the media choice ────────────────────
            StepShell {
                index: number(Step::Gw),
                title: i18n.t("geneanet.step1_title"),
                open: open() == Step::Gw,
                reachable: true,
                summary: gw().map(|f| i18n.t_args(
                    "geneanet.step1_summary",
                    &[
                        ("file", &f.name),
                        ("count", &group_digits(f.inspection.person_count)),
                        ("media", &i18n.t(fidelity_label(fidelity()))),
                    ],
                )),
                on_open: move |_| open.set(Step::Gw),
                GwStep {
                    gw,
                    gw_error,
                    fidelity: fidelity(),
                    on_fidelity: choose_fidelity.clone(),
                    on_settled: move |_| open.set(Step::Saved),
                }
            }

            // ── A connection saved by an earlier run ─────────────────
            StepShell {
                index: number(Step::Saved),
                title: i18n.t("geneanet.saved_title"),
                open: open() == Step::Saved,
                reachable: gw().is_some(),
                summary: saved_summary(&i18n, session_file().as_ref(), saved_skipped()),
                on_open: move |_| open.set(Step::Saved),
                SavedStep {
                    session_file,
                    collected,
                    fetched,
                    coverage: coverage().filter(|c| c.fidelity == fidelity()),
                    error: saved_error,
                    on_fidelity: choose_fidelity,
                    on_skip: move |_| {
                        saved_skipped.set(true);
                        open.set(after_saved());
                    },
                    on_continue: move |_| open.set(after_saved()),
                }
            }

            // ── The photo archives ───────────────────────────────────
            // Only for an originals import on the desktop. A renditions one
            // never opens an archive, so offering the step would ask for
            // gigabytes of ZIP to do nothing with; the web build reads none.
            if NATIVE && archives_step {
            StepShell {
                index: number(Step::Archives),
                title: i18n.t("geneanet.step2_title"),
                open: open() == Step::Archives,
                reachable: gw().is_some() && saved_settled(),
                summary: archives_summary(&i18n, &archives.read(), archives_skipped(), covered()),
                on_open: move |_| open.set(Step::Archives),
                ArchiveStep {
                    archives,
                    archives_skipped,
                    archive_error,
                    on_settled: move |_| {
                        open.set(step_after_archives(collected().is_some()));
                    },
                }
            }
            }

            // ── Sign in and collect ──────────────────────────────────
            // A loaded file holds what the login collects, so it settles
            // this step too; signing in again replaces it.
            if connects {
            StepShell {
                index: number(Step::Connect),
                title: i18n.t("geneanet.step3_title"),
                open: open() == Step::Connect,
                reachable: can_connect(),
                summary: collected().map(|c| collected_summary(&i18n, &c)),
                on_open: move |_| open.set(Step::Connect),
                ConnectStep {
                    connecting: connecting(),
                    collected,
                    fetched,
                    archive_paths: saved_archive_paths.clone(),
                    error: connect_error,
                    on_start: start_connect,
                }
            }
            }

            // ── What will be imported ────────────────────────────────
            StepShell {
                index: number(Step::Preview),
                title: i18n.t("geneanet.step4_title"),
                open: open() == Step::Preview,
                reachable: can_preview(),
                summary: None,
                on_open: move |_| open.set(Step::Preview),
                PreviewStep {
                    preview: preview(),
                    error: preview_error(),
                    fidelity: fidelity(),
                    collected,
                    fetched,
                    archive_paths: saved_archive_paths,
                    session_error: preview_error,
                    gathering: gathering(),
                    fetch_progress: fetch_progress(),
                    override_mismatch,
                    on_compute: run_preview,
                    on_back: move |_| open.set(Step::Gw),
                    on_continue: move |_| open.set(Step::Import),
                }
            }

            // ── Import ───────────────────────────────────────────────
            StepShell {
                index: number(Step::Import),
                title: i18n.t("geneanet.step5_title"),
                open: open() == Step::Import,
                reachable: preview().is_some_and(|stats| !stats.mismatch || override_mismatch()),
                summary: None,
                on_open: move |_| open.set(Step::Import),
                ImportStep {
                    tree_id,
                    preview: preview(),
                    fidelity: fidelity(),
                    gathered: fetched.read().len(),
                    missing: missing(),
                    importing: importing(),
                    job: import_job(),
                    progress: import_progress(),
                    error: import_error(),
                    cancelled: import_cancelled(),
                    on_start: run_import,
                }
            }
        }
    }
}

/// What the loaded file covers of the import, worked out again for each
/// file, each `.gw` and each media answer into `coverage`. A plan is a
/// request, so an answer changed meanwhile leaves the stale result
/// unwritten.
fn use_file_coverage(
    gw: Signal<Option<GwFile>>,
    held: Gathered,
    fidelity: Signal<MediaFidelity>,
    mut coverage: Signal<Option<Coverage>>,
    mut error: Signal<Option<String>>,
) {
    let api = use_context::<ApiClient>();
    let trace = use_context::<UiActionTrace>();
    let i18n = use_i18n();
    use_effect(move || {
        let answer = fidelity();
        coverage.set(None);
        let (Some(_), Some(file), Some(collected)) =
            ((held.session_file)(), gw(), held.collected.peek().clone())
        else {
            return;
        };
        let (api, trace) = (api.clone(), trace.clone());
        spawn(async move {
            let worked_out = trace
                .step(
                    UiActionStep::GeneanetPreview,
                    file_coverage(&api, &file, &collected, &held.fetched.peek(), answer),
                )
                .await;
            if *fidelity.peek() != answer {
                return;
            }
            match worked_out {
                Ok(found) => coverage.set(Some(found)),
                Err(e) => error.set(Some(e.message(&i18n))),
            }
        });
    });
}

/// What the wizard holds of the account's media: the loaded file, the
/// collection, and the media gathered or staged.
#[derive(Clone, Copy)]
struct Gathered {
    session_file: Signal<Option<SessionFile>>,
    collected: Signal<Option<Collected>>,
    fetched: Signal<HashMap<String, String>>,
}

/// Drop what the media answer `next` makes wrong: the media gathered for the
/// other answer — not a loaded file's, which serve either, the import taking
/// from it what the new one needs; under originals, a collection without
/// the deposits' sizes, and the file it came from; under renditions, the
/// archives.
fn forget_for_answer(
    api: &ApiClient,
    next: MediaFidelity,
    held: Gathered,
    mut archives: Signal<Vec<IndexedArchive>>,
    mut archives_skipped: Signal<bool>,
) {
    let from_file = held.session_file.peek().is_some();
    if !from_file {
        forget_gathered_media(api, held.fetched);
    }
    if !next.uses_archives() {
        archives.write().clear();
        archives_skipped.set(false);
        return;
    }
    forget_unmeasured(held.collected);
    if from_file && held.collected.peek().is_none() {
        drop_session_file(api, held.session_file, held.fetched);
    }
}

/// The step after the saved connection: the archives when an originals
/// import on the desktop has originals left for them (`covered` says the
/// file has none left), the login when nothing was collected, else the
/// preview.
fn step_after_saved(fidelity: MediaFidelity, covered: bool, collected: bool) -> Step {
    if NATIVE && fidelity.uses_archives() && !covered {
        Step::Archives
    } else {
        step_after_archives(collected)
    }
}

/// Whether the login can be opened: once the `.gw` is chosen, the saved
/// connection answered — a file or none — and the archives settled.
const fn login_reachable(gw_chosen: bool, saved_settled: bool, archives_settled: bool) -> bool {
    gw_chosen && saved_settled && archives_settled
}

/// The step after the archives: the preview when something was collected
/// already — a loaded file — else the login.
const fn step_after_archives(collected: bool) -> Step {
    if collected {
        Step::Preview
    } else {
        Step::Connect
    }
}

/// Closes the login window, if there is one, for good.
fn close_window(bridge: Option<&GeneanetBridge>, mut window_closed: Signal<bool>) {
    if let Some(bridge) = bridge {
        bridge.close();
    }
    window_closed.set(true);
}

/// Whether the archives step is settled, by adding archives or by explicitly skipping:
/// both let the flow move on, and the difference is only whether photos get
/// downloaded. A renditions import has no archives step to settle, and a
/// loaded file that holds every original needed (`covered`) settles it.
fn archives_settled(
    fidelity: MediaFidelity,
    has_archives: bool,
    skipped: bool,
    covered: bool,
) -> bool {
    !fidelity.uses_archives() || has_archives || skipped || covered
}

/// What the login window collects: the sizing pass is one `HEAD` per deposit
/// and exists only to match the archives. A renditions import never opens
/// one, so it is several hundred requests nothing would read.
fn collect_for(fidelity: MediaFidelity) -> Collect {
    if fidelity.uses_archives() {
        Collect::ListAndSizes
    } else {
        Collect::ListOnly
    }
}

/// The archives are matched on exact byte lengths, and a list-only
/// collection has none. Keeping it would silently match nothing and download
/// everything, so the collection is taken again.
fn forget_unmeasured(mut collected: Signal<Option<Collected>>) {
    let unmeasured = collected
        .read()
        .as_ref()
        .is_some_and(|c| c.deposit_sizes.is_empty());
    if unmeasured {
        collected.set(None);
    }
}

/// Follows the login window until it has collected the account's media, is
/// closed, or fails. Closing it before signing in is not an error — the step
/// simply returns to where it was.
async fn follow_login(
    mut rx: futures_channel::mpsc::UnboundedReceiver<GeneanetEvent>,
    i18n: I18n,
    mut connecting: Signal<Option<Connecting>>,
    mut collected: Signal<Option<Collected>>,
    mut connect_error: Signal<Option<String>>,
    mut open: Signal<Step>,
) {
    use futures_util::StreamExt as _;
    while let Some(event) = rx.next().await {
        let progress = match event {
            GeneanetEvent::Opened => Connecting::WaitingForLogin,
            GeneanetEvent::SignedIn => Connecting::Collecting { done: 0, total: 0 },
            GeneanetEvent::Collecting { done, total } => Connecting::Collecting { done, total },
            GeneanetEvent::Sizing { done, total } => Connecting::Sizing { done, total },
            GeneanetEvent::Collected {
                collection,
                deposit_sizes,
                cookie,
                account,
                photo_count,
            } => {
                collected.set(Some(Collected {
                    collection,
                    deposit_sizes,
                    cookie,
                    account,
                    photo_count,
                    from_file: false,
                }));
                connecting.set(None);
                open.set(Step::Preview);
                return;
            }
            // The fetch events belong to the import step, which drives its
            // own channel; nothing here reacts to them.
            GeneanetEvent::Fetched { .. }
            | GeneanetEvent::Fetching { .. }
            | GeneanetEvent::FetchDone => continue,
            GeneanetEvent::Cancelled => {
                connecting.set(None);
                return;
            }
            GeneanetEvent::Failed(failure) => {
                connect_error.set(Some(i18n.t(failure.i18n_key())));
                connecting.set(None);
                return;
            }
        };
        connecting.set(Some(progress));
    }
}

/// The preview of the import, then the media the server cannot produce from
/// the archives. A session loaded from disk may already carry them, in which
/// case nothing here touches the network at all.
async fn preview_and_plan(
    api: &ApiClient,
    body: &GeneanetPreviewBody,
    mut preview: Signal<Option<GeneanetPreview>>,
) -> Result<Vec<crate::api::NeededMedia>, crate::api::ApiError> {
    preview.set(Some(api.preview_geneanet_import(body).await?));
    api.plan_geneanet_import(body).await.map(|plan| plan.needed)
}

/// Hand back to the backend the staged media among `media`, which the wizard
/// no longer needs: a session's photos are the account's own and are not
/// left in temporary files. Media the login window fetched are its own and
/// are ignored by the backend. Best effort — what is not released expires.
/// The web build stages nothing: a session's media stay in its file.
fn release_staged_media(api: &ApiClient, media: &HashMap<String, String>) {
    if !NATIVE || media.is_empty() {
        return;
    }
    let api = api.clone();
    let paths: Vec<String> = media.values().cloned().collect();
    // Not tied to this component: it runs as the wizard goes away.
    dioxus::core::spawn_forever(async move {
        let _ = api.release_geneanet_session_media(paths).await;
    });
}

/// Drop what was gathered for a media answer the user changed: the
/// desktop's staged and fetched files are for the other answer. On the web
/// they are the loaded file's, which stay in it: its import takes from it
/// what the new answer needs.
fn forget_gathered_media(api: &ApiClient, mut fetched: Signal<HashMap<String, String>>) {
    if NATIVE {
        release_staged_media(api, &fetched.peek());
        fetched.write().clear();
    }
}

/// Lets go of the web build's loaded file, which only an import reads.
fn forget_loaded_file() {
    if !NATIVE {
        document::eval("delete globalThis.oxidgeneGeneanetSession;");
    }
}

/// Lets go of the loaded file: what it collected, the media it staged on the
/// desktop, and the browser's hold on it.
fn drop_session_file(
    api: &ApiClient,
    mut session_file: Signal<Option<SessionFile>>,
    mut fetched: Signal<HashMap<String, String>>,
) {
    release_staged_media(api, &fetched.peek());
    fetched.write().clear();
    session_file.set(None);
    forget_loaded_file();
}

/// How much of the import of `file` under `fidelity` the media `held`, a
/// loaded file's, cover: the plan with no archive, less what it holds. When
/// renditions leave some out, the originals answer is worked out too, and
/// offered if the file covers more of it — never for a file without the
/// deposits' sizes, which that answer would drop.
async fn file_coverage(
    api: &ApiClient,
    file: &GwFile,
    collected: &Collected,
    held: &HashMap<String, String>,
    fidelity: MediaFidelity,
) -> Result<Coverage, ApiError> {
    let missing_under = |fidelity: MediaFidelity| {
        let body = GeneanetPreviewBody {
            gw_base64: encode_gw(&file.bytes),
            file_name: file.name.clone(),
            collection: collected.collection.clone(),
            deposit_sizes: collected.deposit_sizes.clone(),
            archive_paths: Vec::new(),
            media_fidelity: fidelity,
        };
        async move {
            let plan = api.plan_geneanet_import(&body).await?;
            Ok::<_, ApiError>(urls_to_fetch(&plan.needed, held).len())
        }
    };
    let missing = missing_under(fidelity).await?;
    let with_originals =
        if missing > 0 && !fidelity.uses_archives() && !collected.deposit_sizes.is_empty() {
            Some(missing_under(MediaFidelity::Originals).await?).filter(|other| *other < missing)
        } else {
            None
        };
    Ok(Coverage {
        fidelity,
        missing,
        with_originals,
    })
}

/// How many of `urls` nothing can fetch: all of them without a login
/// window — the web build, where they are what the loaded file lacks, and
/// which the import step says will be skipped.
fn unfetchable(bridge: Option<&GeneanetBridge>, urls: &[String]) -> usize {
    if bridge.is_some() { 0 } else { urls.len() }
}

/// [`release_staged_media`] on what `fetched` still holds, as the wizard
/// goes away; a signal already dropped holds nothing to release.
fn release_gathered_media(api: &ApiClient, fetched: Signal<HashMap<String, String>>) {
    if let Ok(media) = fetched.try_peek() {
        release_staged_media(api, &media);
    }
}

/// The addresses to fetch: those `needed` not fetched yet, each once — every
/// page of a single-page deposit shares one original URL, and fetching it
/// twice would double the bytes.
fn urls_to_fetch(
    needed: &[crate::api::NeededMedia],
    fetched: &HashMap<String, String>,
) -> Vec<String> {
    let mut urls: Vec<String> = Vec::new();
    for item in needed {
        if !fetched.contains_key(&item.url) && !urls.contains(&item.url) {
            urls.push(item.url.clone());
        }
    }
    urls
}

/// Records what the login window fetches until it is done. One unreachable
/// medium is reported by the import as skipped; it does not end the run.
async fn gather_media(
    mut rx: futures_channel::mpsc::UnboundedReceiver<GeneanetEvent>,
    i18n: I18n,
    mut fetched: Signal<HashMap<String, String>>,
    mut fetch_progress: Signal<Option<(usize, usize)>>,
    mut preview_error: Signal<Option<String>>,
) {
    use futures_util::StreamExt as _;
    while let Some(event) = rx.next().await {
        match event {
            GeneanetEvent::Fetched {
                url,
                path: Some(path),
                ..
            } => {
                fetched.write().insert(url, path);
            }
            GeneanetEvent::Fetching { done, total } => fetch_progress.set(Some((done, total))),
            GeneanetEvent::FetchDone | GeneanetEvent::Cancelled => break,
            GeneanetEvent::Failed(failure) => {
                preview_error.set(Some(i18n.t(failure.i18n_key())));
                break;
            }
            _ => {}
        }
    }
}

/// Where the import step shows how its import ended.
struct ImportEndSignals {
    result: Signal<Option<GeneanetImportResult>>,
    error: Signal<Option<String>>,
    cancelled: Signal<bool>,
}

/// Shows the import's receipt and tells the caller, says it was cancelled,
/// or shows why it failed.
fn settle_import(
    i18n: &I18n,
    outcome: Result<Option<GeneanetImportResult>, ApiError>,
    api: &ApiClient,
    tree_id: Uuid,
    mut end: ImportEndSignals,
    on_imported: EventHandler<ImportOutcome>,
) {
    match outcome {
        Ok(Some(result)) => {
            api.invalidate_tree(tree_id);
            end.result.set(Some(result.clone()));
            on_imported.call(ImportOutcome::Geneanet(result));
        }
        // Rolled back whole: the tree reads as it did before.
        Ok(None) => {
            api.invalidate_tree(tree_id);
            end.cancelled.set(true);
        }
        Err(e) => end.error.set(Some(e.message(i18n))),
    }
}

/// Starts the web build's import of `file` with the loaded session file,
/// which the browser held on to since the login step, both sent as one multipart
/// form; its upload shows in `import_progress`. Native builds send paths
/// instead ([`ApiClient::import_geneanet`]).
#[cfg(target_arch = "wasm32")]
async fn start_session_import(
    api: &ApiClient,
    tree_id: Uuid,
    file: &GwFile,
    fidelity: MediaFidelity,
    mut import_progress: Signal<Option<ImportProgress>>,
) -> Result<crate::api::ImportJobStarted, ApiError> {
    let json = |value: &str| serde_json::to_string(value).expect("a string is JSON serializable");
    let endpoint = json(&api.geneanet_session_import_url(tree_id));
    let gw = json(&encode_gw(&file.bytes));
    let name = json(&file.name);
    let fidelity = serde_json::to_string(&fidelity).expect("a media answer is JSON serializable");
    // The browser sends the files itself, so the session's CSRF token is
    // set here rather than by the API client (docs/authentication.md §4.4).
    let csrf = serde_json::to_string(&api.csrf_token()).expect("a token is JSON serializable");
    let script = format!(
        r#"
        const session = globalThis.oxidgeneGeneanetSession;
        if (!session) {{
            dioxus.send({{ kind: 'error' }});
            return;
        }}
        const gw = Uint8Array.from(atob({gw}), c => c.charCodeAt(0));
        const form = new FormData();
        form.append('gw', new Blob([gw]), {name});
        form.append('media_fidelity', {fidelity});
        form.append('session', session, session.name);
        const xhr = new XMLHttpRequest();
        xhr.open('POST', {endpoint});
        const csrf = {csrf};
        if (csrf) xhr.setRequestHeader('X-OxidGene-CSRF', csrf);
        xhr.upload.onprogress = event => dioxus.send({{
            kind: 'upload',
            done: event.loaded,
            total: event.lengthComputable ? event.total : 0,
        }});
        xhr.onerror = () => dioxus.send({{ kind: 'error' }});
        xhr.onload = () => {{
            const job = xhr.status === 202 ? JSON.parse(xhr.responseText)?.job_id : null;
            dioxus.send(job ? {{ kind: 'started', job_id: job }} : {{ kind: 'error' }});
        }};
        xhr.send(form);
        "#,
    );
    let mut eval = document::eval(&script);
    loop {
        match eval.recv::<UploadEvent>().await {
            Ok(UploadEvent::Upload { done, total }) => {
                import_progress.set(Some(ImportProgress {
                    phase: "upload".to_string(),
                    done,
                    total,
                }));
            }
            Ok(UploadEvent::Started { job_id }) => {
                break Ok(crate::api::ImportJobStarted { job_id });
            }
            Ok(UploadEvent::Error) | Err(_) => break Err(ApiError::Client("error.upload_failed")),
        }
    }
}

/// Uploads the import, then — the login window no longer needed, so
/// `window_done` runs — follows its job, published in `import_job` for the
/// Cancel button, to the end: the import's result, or `None` when it was
/// cancelled and rolled back.
async fn upload_and_follow(
    api: &ApiClient,
    trace: &UiActionTrace,
    tree_id: Uuid,
    start: impl std::future::Future<Output = Result<crate::api::ImportJobStarted, ApiError>>,
    window_done: impl FnOnce(),
    mut import_progress: Signal<Option<ImportProgress>>,
    mut import_job: Signal<Option<Uuid>>,
) -> Result<Option<GeneanetImportResult>, ApiError> {
    let started = trace.step(UiActionStep::GeneanetUpload, start).await?;
    window_done();
    import_job.set(Some(started.job_id));
    let ended = trace
        .step(
            UiActionStep::GeneanetPoll,
            follow_import_job(api, tree_id, started.job_id, |status| {
                import_progress.set(Some(ImportProgress {
                    phase: status.phase.clone(),
                    done: status.done,
                    total: status.total,
                }));
            }),
        )
        .await?;
    ended
        .map(|status| {
            status
                .geneanet_result
                .ok_or(ApiError::Client("error.unexpected_response"))
        })
        .transpose()
}

/// The archives step's summary: covered by the loaded file (`covered`),
/// skipped, or how many archives and files.
fn archives_summary(
    i18n: &I18n,
    archives: &[IndexedArchive],
    skipped: bool,
    covered: bool,
) -> Option<String> {
    if covered && archives.is_empty() {
        return Some(i18n.t("geneanet.step2_covered"));
    }
    if skipped {
        return Some(i18n.t("geneanet.step2_skipped"));
    }
    if archives.is_empty() {
        return None;
    }
    let files: usize = archives.iter().map(|a| a.file_count).sum();
    Some(i18n.t_args(
        "geneanet.step2_summary",
        &[
            ("archives", &archives.len().to_string()),
            ("files", &group_digits(files)),
        ],
    ))
}

/// The login step's summary: how many photographs, and whose — or that a
/// loaded file brought them.
fn collected_summary(i18n: &I18n, collected: &Collected) -> String {
    let photos = group_digits(collected.photo_count);
    match (&collected.account, collected.from_file) {
        (_, true) => i18n.t_args("geneanet.step3_from_file", &[("count", &photos)]),
        (Some(account), false) => i18n.t_args(
            "geneanet.step3_summary_named",
            &[("account", account), ("count", &photos)],
        ),
        (None, false) => i18n.t_args("geneanet.step3_summary", &[("count", &photos)]),
    }
}

/// The saved-connection step's summary: the file loaded, or none chosen.
fn saved_summary(i18n: &I18n, file: Option<&SessionFile>, skipped: bool) -> Option<String> {
    match (file, skipped) {
        (Some(file), _) => Some(i18n.t_args(
            &i18n.plural_key("geneanet.saved_summary", file.media),
            &[("file", &file.name), ("count", &group_digits(file.media))],
        )),
        (None, true) => Some(i18n.t("geneanet.saved_skipped")),
        (None, false) => None,
    }
}

/// The steps this journey shows, in order: the archives only for an
/// originals import on the desktop (`archives_step`), the login only where
/// a window can be opened (`connects`).
fn visible_steps(archives_step: bool, connects: bool) -> Vec<Step> {
    [
        Step::Gw,
        Step::Saved,
        Step::Archives,
        Step::Connect,
        Step::Preview,
        Step::Import,
    ]
    .into_iter()
    .filter(|step| match step {
        Step::Archives => NATIVE && archives_step,
        Step::Connect => connects,
        _ => true,
    })
    .collect()
}

/// What a step is called on screen: its place among the `steps` shown.
///
/// The journey has four to six steps, and the numbers have to say so: a
/// wizard that skipped from 2 to 4 would read as a step gone missing rather
/// than a step that was never needed.
fn step_number(steps: &[Step], step: Step) -> usize {
    steps
        .iter()
        .position(|shown| *shown == step)
        .map_or(0, |at| at + 1)
}

/// What to call the phase an import reports it is in.
///
/// The `matching` phase is the archive pass. A renditions run still enters it —
/// it begins the phase, finds no archive to index and leaves — so naming it
/// would claim work against archives that run never opened. It is folded into
/// the media phase it precedes instead.
fn phase_key(phase: Option<&str>, fidelity: MediaFidelity) -> &'static str {
    match phase {
        Some("matching") if fidelity.uses_archives() => "geneanet.phase_matching",
        Some("matching" | "media") => "geneanet.phase_media",
        Some("finishing" | "projections") => "geneanet.phase_finishing",
        // Whatever it had reached, it is now undoing it.
        Some("cancelling") => "import.phase_cancelling",
        // The web build sending its files.
        Some("upload") => "import.phase_upload",
        // Including `staging`, which is the request in flight: people are
        // what it is about to write.
        _ => "geneanet.phase_people",
    }
}

/// The name of the translation key for one media answer.
const fn fidelity_label(fidelity: MediaFidelity) -> &'static str {
    match fidelity {
        MediaFidelity::Renditions => "geneanet.media_renditions",
        MediaFidelity::Originals => "geneanet.media_originals",
    }
}

/// A step's chrome: the number, the title, the collapsed receipt and the
/// disclosure behaviour.
///
/// Collapsed steps are buttons, so `Enter`/`Space` reopen them; a step that is
/// not yet reachable is rendered but inert, which is what makes the whole
/// journey legible before any of it has been done.
#[component]
fn StepShell(
    index: usize,
    title: String,
    open: bool,
    reachable: bool,
    summary: Option<String>,
    on_open: EventHandler<()>,
    children: Element,
) -> Element {
    let i18n = use_i18n();
    let settled = summary.is_some();

    let class = match (open, reachable) {
        (true, _) => "gn-step is-open",
        (false, true) => "gn-step",
        (false, false) => "gn-step is-dim",
    };

    rsx! {
        section { class: "{class}",
            button {
                class: "gn-step-head",
                r#type: "button",
                disabled: !reachable,
                "aria-expanded": open,
                onclick: move |_| on_open.call(()),

                span { class: if settled { "gn-step-mark is-done" } else { "gn-step-mark" },
                    if settled { "✓" } else { "{index}" }
                }
                span { class: "gn-step-title", "{title}" }
                if let Some(summary) = summary.clone() {
                    span { class: "gn-step-summary", "{summary}" }
                }
                if !open && reachable {
                    span { class: "gn-step-edit", {i18n.t("common.edit")} }
                }
            }
            if open {
                div { class: "gn-step-body", {children} }
            }
        }
    }
}

// ── The .gw file ───────────────────────────────────────────────────

#[component]
fn GwStep(
    gw: Signal<Option<GwFile>>,
    gw_error: Signal<Option<String>>,
    /// Which bytes to keep per medium — the answer that decides whether there
    /// is an archives step at all.
    fidelity: MediaFidelity,
    on_fidelity: EventHandler<MediaFidelity>,
    on_settled: EventHandler<()>,
) -> Element {
    let api = use_context::<ApiClient>();
    let i18n = use_i18n();
    let trace = use_context::<UiActionTrace>();
    let mut busy = use_signal(|| false);

    let mut gw = gw;
    let mut gw_error = gw_error;

    let pick = move |_| {
        let api = api.clone();
        let trace = trace.clone();
        spawn(async move {
            let file = rfd::AsyncFileDialog::new()
                .add_filter("GeneWeb", &["gw"])
                .add_filter(i18n.t("common.all_files"), &["*"])
                .set_title(i18n.t("geneanet.step1_pick"))
                .pick_file()
                .await;
            let Some(file) = file else { return };

            let name = file.file_name();
            let bytes = trace.step(UiActionStep::GeneanetRead, file.read()).await;

            // Parsed on selection, not at import time. It costs nothing and it
            // is the first moment the user finds out whether they picked the
            // right export — a `.ged` fails here rather than four steps later.
            busy.set(true);
            gw_error.set(None);
            let inspected = trace
                .step(
                    UiActionStep::GeneanetInspect,
                    api.inspect_geneweb(bytes.clone(), &name),
                )
                .await;
            busy.set(false);

            match inspected {
                Ok(inspection) => {
                    gw.set(Some(GwFile {
                        name,
                        bytes,
                        inspection,
                    }));
                    on_settled.call(());
                }
                Err(e) => {
                    let message = if name.to_lowercase().ends_with(".ged") {
                        i18n.t("geneanet.error_is_gedcom")
                    } else {
                        e.message(&i18n)
                    };
                    gw_error.set(Some(message));
                }
            }
        });
    };

    rsx! {
        p { class: "gn-lead", {i18n.t("geneanet.step1_lead")} }

        ol { class: "gn-howto",
            li { {i18n.t("geneanet.step1_how1")} }
            li { {i18n.t("geneanet.step1_how2")} }
            li { {i18n.t("geneanet.step1_how3")} }
            li { {i18n.t("geneanet.step1_how4")} }
        }

        details { class: "gn-aside",
            summary { {i18n.t("geneanet.why_not_gedcom_q")} }
            p { {i18n.t("geneanet.why_not_gedcom_a")} }
        }

        if let Some(err) = gw_error() {
            div { class: "error-msg", "{err}" }
        }

        if let Some(file) = gw() {
            if file.inspection.skipped_blocks > 0 {
                div { class: "warning-msg",
                    {i18n.t_plural("geneanet.skipped_blocks", file.inspection.skipped_blocks)}
                }
            }
        }

        // Asked here, and last, because it is the answer that decides how much
        // of the rest of the wizard exists — and the step it removes is the one
        // that would otherwise send the user off to request a data export and
        // come back days later.
        MediaChoice { fidelity, on_choose: on_fidelity }

        div { class: "modal-actions",
            button {
                class: "btn btn-primary",
                disabled: busy(),
                onclick: pick,
                if busy() { {i18n.t("geneanet.reading")} } else { {i18n.t("geneanet.step1_pick")} }
            }
        }
    }
}

/// The two answers to "which bytes do you want for each photograph".
///
/// Radio inputs rather than a select or a switch: each answer needs a sentence
/// of its own to be a real choice — one costs nothing but quality, the other
/// costs a data export — and neither is legible as a label alone.
#[component]
fn MediaChoice(fidelity: MediaFidelity, on_choose: EventHandler<MediaFidelity>) -> Element {
    let i18n = use_i18n();

    let option = |value: MediaFidelity, name: &str, why: &str| {
        let selected = fidelity == value;
        rsx! {
            label {
                class: if selected { "gn-choice-opt is-on" } else { "gn-choice-opt" },
                input {
                    r#type: "radio",
                    name: "oxidgene-geneanet-fidelity",
                    checked: selected,
                    onchange: move |_| on_choose.call(value),
                }
                span { class: "gn-choice-text",
                    span { class: "gn-choice-name", {i18n.t(name)} }
                    span { class: "gn-choice-why", {i18n.t(why)} }
                }
            }
        }
    };

    rsx! {
        fieldset { class: "gn-choice",
            legend { class: "gn-choice-legend", {i18n.t("geneanet.media_choice_title")} }
            div { class: "gn-choice-opts",
                {option(
                    MediaFidelity::Renditions,
                    "geneanet.media_renditions",
                    "geneanet.media_renditions_why",
                )}
                {option(
                    MediaFidelity::Originals,
                    "geneanet.media_originals",
                    "geneanet.media_originals_why",
                )}
            }
        }
    }
}

// ── The archives ───────────────────────────────────────────────────

#[component]
fn ArchiveStep(
    archives: Signal<Vec<IndexedArchive>>,
    archives_skipped: Signal<bool>,
    archive_error: Signal<Option<String>>,
    on_settled: EventHandler<()>,
) -> Element {
    let api = use_context::<ApiClient>();
    let i18n = use_i18n();
    let trace = use_context::<UiActionTrace>();
    let mut busy = use_signal(|| false);

    let mut archives = archives;
    let mut archives_skipped = archives_skipped;
    let mut archive_error = archive_error;

    let pick = move |_| {
        let api = api.clone();
        let trace = trace.clone();
        spawn(async move {
            let files = rfd::AsyncFileDialog::new()
                .add_filter("ZIP", &["zip"])
                .set_title(i18n.t("geneanet.step2_pick"))
                .pick_files()
                .await;
            let Some(files) = files else { return };

            // Paths, not bytes. Reading a multi-gigabyte export into memory to
            // learn what its central directory already states would be
            // absurd — and this branch only runs on desktop, where the server
            // is in-process and shares the filesystem.
            let mut paths: Vec<String> = archives.read().iter().map(|a| a.path.clone()).collect();
            for file in &files {
                paths.push(selected_file_path(file));
            }

            busy.set(true);
            archive_error.set(None);
            let indexed = trace
                .step(
                    UiActionStep::GeneanetIndex,
                    api.index_geneanet_archives(paths),
                )
                .await;
            busy.set(false);

            match indexed {
                Ok(ArchiveIndex { archives: rows, .. }) => {
                    // A ZIP that would not open is reported and dropped; the
                    // ones that did still count.
                    let failed: Vec<String> = rows
                        .iter()
                        .filter(|a| a.error.is_some())
                        .map(|a| a.file_name.clone())
                        .collect();
                    if !failed.is_empty() {
                        archive_error.set(Some(i18n.t_args(
                            "geneanet.error_bad_archive",
                            &[("files", &failed.join(", "))],
                        )));
                    }
                    archives.set(rows.into_iter().filter(|a| a.error.is_none()).collect());
                    archives_skipped.set(false);
                }
                Err(e) => archive_error.set(Some(e.message(&i18n))),
            }
        });
    };

    let no_images = archives
        .read()
        .iter()
        .any(|a| a.error.is_none() && a.image_count == 0);

    rsx! {
        p { class: "gn-lead", {i18n.t("geneanet.step2_lead")} }

        ol { class: "gn-howto",
            li { {i18n.t("geneanet.step2_how1")} }
            li { {i18n.t("geneanet.step2_how2")} }
            li { {i18n.t("geneanet.step2_how3")} }
        }

        div { class: "gn-warn-box", {i18n.t("geneanet.step2_do_not_unzip")} }

        if let Some(err) = archive_error() {
            div { class: "error-msg", "{err}" }
        }
        if no_images {
            div { class: "warning-msg", {i18n.t("geneanet.warn_no_images")} }
        }

        if !archives.read().is_empty() {
            ul { class: "gn-archive-list",
                for archive in archives.read().iter() {
                    li { key: "{archive.path}",
                        span { class: "gn-archive-name", "{archive.file_name}" }
                        span { class: "gn-archive-count",
                            {i18n.t_plural("geneanet.archive_files", archive.file_count)}
                        }
                        button {
                            class: "gn-archive-remove",
                            r#type: "button",
                            "aria-label": i18n.t("common.remove"),
                            onclick: {
                                let path = archive.path.clone();
                                move |_| {
                                    archives.write().retain(|a| a.path != path);
                                }
                            },
                            "✕"
                        }
                    }
                }
            }
        }

        div { class: "modal-actions",
            button {
                class: "btn btn-outline",
                onclick: move |_| {
                    archives.write().clear();
                    archives_skipped.set(true);
                    on_settled.call(());
                },
                {i18n.t("geneanet.step2_skip")}
            }
            button {
                class: "btn btn-outline",
                disabled: busy(),
                onclick: pick,
                if busy() { {i18n.t("geneanet.reading")} } else { {i18n.t("geneanet.step2_pick")} }
            }
            button {
                class: "btn btn-primary",
                disabled: archives.read().is_empty(),
                onclick: move |_| on_settled.call(()),
                {i18n.t("common.continue")}
            }
        }
    }
}

fn selected_file_path(file: &rfd::FileHandle) -> String {
    #[cfg(not(target_arch = "wasm32"))]
    {
        file.path().display().to_string()
    }
    #[cfg(target_arch = "wasm32")]
    {
        file.file_name()
    }
}

// ── The saved connection ───────────────────────────────────────────

/// A connection saved by an earlier run, or none.
///
/// Asked before the archives and the login because the file can stand in
/// for both: it holds what the login collects, and the originals the
/// archives held when they were added before saving. Once loaded, the step
/// says how much of this import it covers, so the steps after it ask only
/// for the rest. The desktop goes on without a file in one click, signing
/// in to Geneanet as before; the web build, which can neither sign in nor
/// read archives, has no other way to the media.
#[component]
fn SavedStep(
    session_file: Signal<Option<SessionFile>>,
    collected: Signal<Option<Collected>>,
    fetched: Signal<HashMap<String, String>>,
    /// What the file covers, once worked out for the current media answer.
    coverage: Option<Coverage>,
    error: Signal<Option<String>>,
    on_fidelity: EventHandler<MediaFidelity>,
    on_skip: EventHandler<()>,
    on_continue: EventHandler<()>,
) -> Element {
    let api = use_context::<ApiClient>();
    let i18n = use_i18n();
    let trace = use_context::<UiActionTrace>();
    let mut busy = use_signal(|| false);
    let mut collected = collected;
    let mut fetched = fetched;
    let mut session_file = session_file;
    let mut error = error;

    let api_load = api.clone();
    let load = move |_: Event<MouseData>| {
        let api = api_load.clone();
        let trace = trace.clone();
        spawn(async move {
            error.set(None);
            let loaded = load_session(&api, &trace, i18n, busy).await;
            busy.set(false);
            match loaded {
                Ok(Some(session)) => {
                    // What an earlier file staged is not needed any more.
                    release_staged_media(&api, &fetched.peek());
                    session_file.set(Some(SessionFile {
                        name: session.name,
                        media: session.media.len(),
                    }));
                    collected.set(Some(session.collected));
                    fetched.set(session.media);
                }
                Ok(None) => {}
                Err(message) => error.set(Some(message)),
            }
        });
    };
    let remove = move |_: Event<MouseData>| {
        // What a later login collected stays.
        if collected.peek().as_ref().is_some_and(|c| c.from_file) {
            collected.set(None);
        }
        drop_session_file(&api, session_file, fetched);
    };

    let lead = if NATIVE {
        "geneanet.saved_lead"
    } else {
        "geneanet.saved_lead_web"
    };
    let loaded = session_file();

    rsx! {
        p { class: "gn-lead", {i18n.t(lead)} }

        if let Some(err) = error() {
            div { class: "error-msg", "{err}" }
        }

        if loaded.is_some() {
            {coverage_findings(&i18n, coverage, on_fidelity)}
        }

        // Without a file the desktop's way on is the login, one click away;
        // the web build's is the file.
        div { class: "modal-actions",
            if loaded.is_some() {
                button {
                    class: "btn btn-outline",
                    disabled: busy(),
                    onclick: remove,
                    {i18n.t("geneanet.saved_remove")}
                }
            }
            button {
                class: if loaded.is_some() || NATIVE { "btn btn-outline" } else { "btn btn-primary" },
                disabled: busy(),
                onclick: load,
                if busy() { {i18n.t("geneanet.reading")} } else { {i18n.t("geneanet.session_load")} }
            }
            if loaded.is_none() && NATIVE {
                button {
                    class: "btn btn-primary",
                    disabled: busy(),
                    onclick: move |_| on_skip.call(()),
                    {i18n.t("geneanet.saved_skip")}
                }
            }
            if loaded.is_some() {
                button {
                    class: "btn btn-primary",
                    disabled: busy() || coverage.is_none(),
                    onclick: move |_| on_continue.call(()),
                    {i18n.t("common.continue")}
                }
            }
        }
    }
}

/// What a loaded file covers of this import: everything, or how many media
/// it lacks — which the desktop still looks for in the archives and on
/// Geneanet, and the web build skips — and, when it holds originals the
/// renditions answer would leave unused, the offer to keep those instead.
fn coverage_findings(
    i18n: &I18n,
    coverage: Option<Coverage>,
    on_fidelity: EventHandler<MediaFidelity>,
) -> Element {
    let Some(coverage) = coverage else {
        return rsx! {
            div { class: "loading", {i18n.t("geneanet.saved_checking")} }
        };
    };
    let lacking = if NATIVE {
        "geneanet.saved_missing"
    } else {
        "geneanet.saved_missing_web"
    };
    rsx! {
        ul { class: "gn-findings",
            if coverage.missing == 0 {
                li { class: "is-good", {i18n.t("geneanet.saved_covered")} }
            } else {
                li { class: if NATIVE { "is-info" } else { "is-warn" },
                    {i18n.t_plural(lacking, coverage.missing)}
                }
            }
            if coverage.with_originals.is_some() {
                li { class: "is-info",
                    {i18n.t("geneanet.saved_has_originals")}
                    " "
                    button {
                        class: "btn btn-outline btn-sm",
                        r#type: "button",
                        onclick: move |_| on_fidelity.call(MediaFidelity::Originals),
                        {i18n.t("geneanet.saved_use_originals")}
                    }
                }
            }
        }
    }
}

// ── The login ──────────────────────────────────────────────────────

#[component]
fn ConnectStep(
    connecting: Option<Connecting>,
    collected: Signal<Option<Collected>>,
    fetched: Signal<HashMap<String, String>>,
    /// The data archives, whose originals a saved file carries.
    archive_paths: Vec<String>,
    error: Signal<Option<String>>,
    on_start: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();

    rsx! {
        p { class: "gn-lead", {i18n.t("geneanet.step3_lead")} }
        p { class: "gn-note", {i18n.t("geneanet.step3_password")} }

        if let Some(err) = error() {
            div { class: "error-msg", "{err}" }
        }

        match connecting {
            Some(Connecting::WaitingForLogin) => rsx! {
                div { class: "gn-progress-block",
                    div { class: "gn-progress-label", {i18n.t("geneanet.step3_waiting")} }
                }
            },
            Some(Connecting::Collecting { done, total }) => rsx! {
                ProgressBar { label: i18n.t("geneanet.step3_stage1"), done, total }
            },
            Some(Connecting::Sizing { done, total }) => rsx! {
                ProgressBar { label: i18n.t("geneanet.step3_stage2"), done, total }
            },
            None => rsx! {
                div { class: "modal-actions",
                    button {
                        class: "btn btn-primary",
                        onclick: move |_| on_start.call(()),
                        {i18n.t("geneanet.step3_open")}
                    }
                }
                SessionControls { collected, fetched, archive_paths, error }
            },
        }
    }
}

/// A saved session the user loaded: its file's name, what it collected,
/// and its media by URL — staged files on the desktop; in the web build,
/// empty handles, the media staying in the chosen file, which the import
/// sends again.
struct LoadedSession {
    name: String,
    collected: Collected,
    media: HashMap<String, String>,
}

impl LoadedSession {
    fn new(
        name: String,
        collection: String,
        deposit_sizes: HashMap<i64, u64>,
        account: Option<String>,
        photo_count: usize,
        media: HashMap<String, String>,
    ) -> Self {
        Self {
            name,
            collected: Collected {
                collection,
                deposit_sizes,
                // A saved session carries no cookie and needs none: it is the
                // collection, not a way back to the account.
                cookie: None,
                account,
                photo_count,
                from_file: true,
            },
            media,
        }
    }
}

/// Let the user pick a saved session and decode it, its media staged by the
/// in-process server; `None` when they picked nothing, the reader's message
/// when it is no session. `busy` is set once a file was picked.
#[cfg(not(target_arch = "wasm32"))]
async fn load_session(
    api: &ApiClient,
    trace: &UiActionTrace,
    i18n: I18n,
    mut busy: Signal<bool>,
) -> Result<Option<LoadedSession>, String> {
    let Some(file) = rfd::AsyncFileDialog::new()
        // A bare JSON collection still loads — the reader tells the shapes
        // apart by content, not by extension.
        .add_filter(i18n.t("geneanet.session_file_kind"), &["zip", "json"])
        .set_title(i18n.t("geneanet.session_load"))
        .pick_file()
        .await
    else {
        return Ok(None);
    };
    busy.set(true);
    let body = trace
        .step(
            UiActionStep::GeneanetRead,
            tokio::fs::File::open(file.path()),
        )
        .await
        .map(reqwest::Body::from)
        .map_err(|_| i18n.t("geneanet.file_read_error"))?;
    let session = trace
        .step(
            UiActionStep::GeneanetSessionDecode,
            api.decode_geneanet_session(body),
        )
        .await
        .map_err(|e| e.message(&i18n))?;
    Ok(Some(LoadedSession::new(
        file.file_name(),
        session.collection,
        session.deposit_sizes,
        session.account,
        session.photo_count,
        session.media,
    )))
}

/// What the browser reports while it reads a saved session.
#[derive(serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[cfg(target_arch = "wasm32")]
enum SessionReadEvent {
    Cancelled,
    Reading,
    /// The server's answer, decoded on its own: an internally tagged enum
    /// buffers its fields, and the buffer reads no number from the string
    /// keys of `deposit_sizes`.
    Read {
        name: String,
        session: serde_json::Value,
    },
    Failed {
        status: u16,
        body: String,
    },
}

/// Let the user pick a saved session and have the server read it, keeping
/// nothing: the browser holds on to the file, which the import sends again
/// ([`start_session_import`]). `None` when they picked nothing, the reader's
/// message when it is no session. `busy` is set once a file was picked.
#[cfg(target_arch = "wasm32")]
async fn load_session(
    api: &ApiClient,
    trace: &UiActionTrace,
    i18n: I18n,
    mut busy: Signal<bool>,
) -> Result<Option<LoadedSession>, String> {
    let endpoint = serde_json::to_string(&api.geneanet_session_read_url())
        .expect("an endpoint URL is JSON serializable");
    // The browser sends the file itself, so the session's CSRF token is set
    // here rather than by the API client (docs/authentication.md §4.4).
    let csrf = serde_json::to_string(&api.csrf_token()).expect("a token is JSON serializable");
    let script = format!(
        r#"
        const input = document.createElement('input');
        input.type = 'file';
        input.accept = '.zip,.json,application/zip,application/json';
        input.addEventListener('cancel', () => dioxus.send({{ kind: 'cancelled' }}));
        input.addEventListener('change', () => {{
            const file = input.files?.[0];
            if (!file) {{
                dioxus.send({{ kind: 'cancelled' }});
                return;
            }}
            dioxus.send({{ kind: 'reading' }});
            const xhr = new XMLHttpRequest();
            xhr.open('POST', {endpoint});
            xhr.setRequestHeader('Content-Type', 'application/octet-stream');
            const csrf = {csrf};
            if (csrf) xhr.setRequestHeader('X-OxidGene-CSRF', csrf);
            xhr.onerror = () => dioxus.send({{ kind: 'failed', status: 0, body: '' }});
            xhr.onload = () => {{
                if (xhr.status !== 200) {{
                    dioxus.send({{ kind: 'failed', status: xhr.status, body: xhr.responseText }});
                    return;
                }}
                // Kept for the import, which sends the same file again.
                globalThis.oxidgeneGeneanetSession = file;
                dioxus.send({{ kind: 'read', name: file.name, session: JSON.parse(xhr.responseText) }});
            }};
            xhr.send(file);
        }});
        input.click();
        "#,
    );
    let mut eval = document::eval(&script);
    let read = async {
        loop {
            match eval.recv::<SessionReadEvent>().await {
                Ok(SessionReadEvent::Reading) => busy.set(true),
                Ok(SessionReadEvent::Cancelled) => return Ok(None),
                Ok(SessionReadEvent::Read { name, session }) => {
                    return serde_json::from_value::<crate::api::GeneanetSessionContents>(session)
                        .map(|session| Some((name, session)))
                        .map_err(ApiError::from);
                }
                Ok(SessionReadEvent::Failed { status: 0, .. }) | Err(_) => {
                    return Err(ApiError::Client("error.unreachable"));
                }
                Ok(SessionReadEvent::Failed { status, body }) => {
                    return Err(ApiError::Api { status, body });
                }
            }
        }
    };
    let session = trace
        .step(UiActionStep::GeneanetSessionDecode, read)
        .await
        .map_err(|e| e.message(&i18n))?;
    Ok(session.map(|(name, session)| {
        LoadedSession::new(
            name,
            session.collection,
            session.deposit_sizes,
            session.account,
            session.photo_count,
            session
                .media_urls
                .into_iter()
                .map(|url| (url, String::new()))
                .collect(),
        )
    }))
}

/// Saving what has been collected, for a later run or the web build, which
/// loads it at the saved-connection step.
///
/// Shown in the login step and again in the preview, because the file is
/// worth saving at two different moments and they are not the same file:
/// after the login it holds the collection and the deposit sizes; after the
/// preview it holds the gathered media too and needs no Geneanet connection
/// at all. With archives added it carries their originals either way. The
/// login step also collapses once it is settled, so a control that lived
/// only there would vanish exactly when the complete file became available.
/// Only the desktop saves: the file is rebuilt from the media it staged.
#[component]
fn SessionControls(
    collected: Signal<Option<Collected>>,
    fetched: Signal<HashMap<String, String>>,
    /// The data archives: the saved file carries the originals they hold,
    /// which the web build, given no archive, imports from it.
    archive_paths: Vec<String>,
    error: Signal<Option<String>>,
) -> Element {
    let api = use_context::<ApiClient>();
    let i18n = use_i18n();
    let trace = use_context::<UiActionTrace>();
    let mut error = error;

    let save = move |_: Event<MouseData>| {
        let api = api.clone();
        let trace = trace.clone();
        let Some(current) = collected() else { return };
        let archive_paths = archive_paths.clone();
        spawn(async move {
            let body = GeneanetSessionBody {
                collection: current.collection.clone(),
                deposit_sizes: current.deposit_sizes.clone(),
                account: current.account.clone(),
                media: fetched.read().clone(),
                archive_paths,
            };
            let archive: Vec<u8> = match trace
                .step(
                    UiActionStep::GeneanetSessionEncode,
                    api.encode_geneanet_session(&body),
                )
                .await
            {
                Ok(archive) => archive,
                Err(e) => {
                    error.set(Some(e.message(&i18n)));
                    return;
                }
            };

            let Some(file) = rfd::AsyncFileDialog::new()
                .add_filter(i18n.t("geneanet.session_file_kind"), &["zip"])
                .set_file_name("geneanet-session.zip")
                .set_title(i18n.t("geneanet.session_save"))
                .save_file()
                .await
            else {
                return;
            };
            // The system's own text of the failure is English whatever the
            // reader's language: the reader is told what to check instead.
            if trace
                .step(UiActionStep::GeneanetWrite, file.write(&archive))
                .await
                .is_err()
            {
                error.set(Some(i18n.t("geneanet.file_write_error")));
            }
        });
    };

    if !NATIVE || collected().is_none() {
        return rsx! {};
    }
    let complete = !fetched.read().is_empty();

    rsx! {
        // A quiet strip rather than a block of its own. It is secondary to
        // whatever step it sits in — a way out, not the way on — so it reads
        // as a footnote and never competes with the step's own action.
        //
        // The button comes after the label that explains it: an earlier
        // version put it above, which read backwards.
        div { class: "gn-session",
            details { class: "gn-session-why",
                summary { {i18n.t("geneanet.session_keep")} }
                p { {i18n.t("geneanet.session_keep_why")} }
            }
            div { class: "gn-session-actions",
                button {
                    class: "gn-session-btn",
                    r#type: "button",
                    onclick: save,
                    span { class: "gn-session-icon", "\u{2912}" }
                    if complete {
                        {i18n.t("geneanet.session_save_complete")}
                    } else {
                        {i18n.t("geneanet.session_save")}
                    }
                }
            }
        }
    }
}

/// A labelled bar with a live region beside it.
///
/// The live region announces the *stage*, not every tick: a bar that spoke on
/// each of 614 photos would be unusable with a screen reader.
#[component]
fn ProgressBar(label: String, done: usize, total: usize) -> Element {
    let pct = progress_percent(done, total);

    rsx! {
        div { class: "gn-progress-block",
            div { class: "gn-progress-label", "aria-live": "polite", "{label}" }
            div {
                class: "gn-progress",
                role: "progressbar",
                "aria-valuenow": done as i64,
                "aria-valuemin": 0,
                "aria-valuemax": total as i64,
                // An unknown total is honest about being unknown rather than
                // drawing a bar that pretends to know how far along it is.
                div {
                    class: if total > 0 { "gn-progress-fill" } else { "gn-progress-fill is-indeterminate" },
                    style: if total > 0 { format!("width: {pct}%") } else { String::new() },
                }
            }
            if total > 0 {
                div { class: "gn-progress-count", "{pct}%" }
            }
        }
    }
}

fn progress_percent(done: usize, total: usize) -> usize {
    if total == 0 {
        return 0;
    }

    ((done.min(total) as u128 * 100) / total as u128) as usize
}

// ── The preview ────────────────────────────────────────────────────

/// What the preview found: where the media will come from, the documents,
/// the pages to match, and the references that found nobody or several.
fn preview_findings(i18n: &I18n, stats: &GeneanetPreview, fidelity: MediaFidelity) -> Element {
    rsx! {
        ul { class: "gn-findings",
            {media_source_finding(i18n, stats, fidelity)}
            if stats.documents > 0 {
                li { class: "is-good",
                    {i18n.t_args(
                        "geneanet.finding_documents",
                        &[
                            ("documents", &group_digits(stats.documents)),
                            ("pages", &group_digits(stats.document_pages)),
                        ],
                    )}
                }
            }
            if stats.to_match > 0 {
                li { class: "is-info",
                    {i18n.t_args(
                        "geneanet.finding_to_match",
                        &[("count", &group_digits(stats.to_match))],
                    )}
                }
            }
            if stats.group_photos > 0 {
                li { class: "is-info",
                    {i18n.t_plural("geneanet.finding_group_photos", stats.group_photos)}
                }
            }
            if stats.unlinked_views > 0 {
                li { class: "is-info",
                    {i18n.t_plural("geneanet.finding_unlinked", stats.unlinked_views)}
                }
            }
            {named_finding(i18n, "is-info", "geneanet.finding_unlinked_names", stats.unlinked_names, &stats.unlinked_names_sample)}
            {named_finding(i18n, "is-warn", "geneanet.finding_outside_tree", stats.outside_tree, &stats.outside_tree_names)}
            {named_finding(i18n, "is-warn", "geneanet.finding_ambiguous", stats.ambiguous, &stats.ambiguous_names)}
        }
    }
}

/// Where the photographs will come from: renditions downloaded, the
/// archives alone, or the archives and some downloads.
fn media_source_finding(i18n: &I18n, stats: &GeneanetPreview, fidelity: MediaFidelity) -> Element {
    let (local, remote) = (
        group_digits(stats.in_archives),
        group_digits(stats.to_download),
    );
    match (
        fidelity.uses_archives(),
        stats.to_download,
        stats.in_archives,
    ) {
        (false, 0, _) | (true, 0, 0) => rsx! {},
        (false, _, _) => rsx! {
            li { class: "is-info",
                {i18n.t_args("geneanet.finding_renditions", &[("count", &remote)])}
            }
        },
        (true, 0, _) => rsx! {
            li { class: "is-good",
                {i18n.t_args("geneanet.finding_all_local", &[("count", &local)])}
            }
        },
        (true, _, _) => rsx! {
            li { class: "is-info",
                {i18n.t_args(
                    "geneanet.finding_to_download",
                    &[("local", &local), ("remote", &remote)],
                )}
            }
        },
    }
}

/// `count` references of a kind, `class`, with a sample of their `names`.
fn named_finding(
    i18n: &I18n,
    class: &'static str,
    key: &str,
    count: usize,
    names: &[String],
) -> Element {
    if count == 0 {
        return rsx! {};
    }
    rsx! {
        li { class,
            details {
                summary { {i18n.t_plural(key, count)} }
                ul {
                    for name in names.iter() {
                        li { "{name}" }
                    }
                }
            }
        }
    }
}

#[component]
fn PreviewStep(
    preview: Option<GeneanetPreview>,
    error: Option<String>,
    /// Which bytes this run keeps, so the findings describe the work it will
    /// actually do rather than the archive matching it is not doing.
    fidelity: MediaFidelity,
    collected: Signal<Option<Collected>>,
    fetched: Signal<HashMap<String, String>>,
    /// The data archives, whose originals a saved file carries.
    archive_paths: Vec<String>,
    session_error: Signal<Option<String>>,
    gathering: bool,
    fetch_progress: Option<(usize, usize)>,
    override_mismatch: Signal<bool>,
    on_compute: EventHandler<()>,
    on_back: EventHandler<()>,
    on_continue: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let mut override_mismatch = override_mismatch;

    // Computed on open rather than eagerly: it needs every earlier step to
    // have settled, and re-running it is how "Edit" on the archives step takes effect.
    use_effect(move || on_compute.call(()));

    let Some(stats) = preview else {
        return rsx! {
            if let Some(err) = error {
                div { class: "error-msg", "{err}" }
            } else {
                div { class: "loading", {i18n.t("geneanet.step4_computing")} }
            }
        };
    };

    rsx! {
        div { class: "import-stats",
            Stat { value: stats.person_count, label: i18n.t("geneanet.stat_people_in_file") }
            Stat { value: stats.photo_count, label: i18n.t("geneanet.stat_photos_found") }
            Stat { value: stats.persons_with_photo, label: i18n.t("geneanet.stat_people_with_photo") }
            Stat { value: stats.attachment_count, label: i18n.t("geneanet.stat_attachments") }
        }

        {preview_findings(&i18n, &stats, fidelity)}

        // Offered here as well as in the login step, and this is the more valuable of
        // the two: once the gathering is done the file needs no Geneanet
        // connection at all. The login step also collapses once settled, so a control
        // only there disappears exactly when it becomes worth using.
        if !gathering {
            SessionControls { collected, fetched, archive_paths, error: session_error }
        }

        if gathering {
            // The last thing that touches Geneanet. Continuing is held until
            // it finishes, so the import step has everything it needs and the login
            // window can be shut before anything is written.
            if let Some((done, total)) = fetch_progress {
                ProgressBar {
                    label: i18n.t(if fidelity.uses_archives() {
                        "geneanet.step4_gathering"
                    } else {
                        "geneanet.step4_downloading"
                    }),
                    done,
                    total,
                }
            }
        } else if stats.mismatch && !override_mismatch() {
            div { class: "gn-mismatch",
                p { {i18n.t("geneanet.mismatch")} }
                div { class: "modal-actions",
                    button {
                        class: "btn btn-primary",
                        onclick: move |_| on_back.call(()),
                        {i18n.t("geneanet.mismatch_back")}
                    }
                    button {
                        class: "btn btn-outline",
                        onclick: move |_| {
                            override_mismatch.set(true);
                            on_continue.call(());
                        },
                        {i18n.t("geneanet.mismatch_anyway")}
                    }
                }
            }
        } else {
            div { class: "modal-actions",
                button {
                    class: "btn btn-primary",
                    onclick: move |_| on_continue.call(()),
                    {i18n.t("common.continue")}
                }
            }
        }
    }
}

// ── The import ─────────────────────────────────────────────────────

/// The final step. Nothing here reaches the network.
///
/// That is the whole point of gathering in the preview step, and it is what this screen
/// has to say: the media are already on this machine, so the minutes ahead are
/// decoding and thumbnailing, not waiting on Geneanet.
#[component]
fn ImportStep(
    preview: Option<GeneanetPreview>,
    /// Which bytes this run keeps, so a run that matches nothing does not
    /// announce a matching phase.
    fidelity: MediaFidelity,
    /// How many media the login window — or a loaded session — actually handed
    /// over. Nothing past this step can fetch another, so this is what says
    /// whether they are here to be imported at all.
    gathered: usize,
    /// How many media the import needs that nothing could fetch — a loaded
    /// file's gaps in a build with no login window.
    missing: usize,
    tree_id: Uuid,
    importing: bool,
    /// The import job once queued, which Cancel stops.
    job: Option<Uuid>,
    progress: Option<crate::api::ImportProgress>,
    error: Option<String>,
    /// Whether the last run was cancelled, leaving the tree unchanged.
    cancelled: bool,
    on_start: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let cancellable = job.filter(|_| progress.as_ref().is_some_and(|p| can_cancel(&p.phase)));

    rsx! {
        if let Some(stats) = preview.clone() {
            p { class: "gn-lead",
                {i18n.t_args(
                    "geneanet.step5_lead",
                    &[
                        ("people", &group_digits(stats.person_count)),
                        ("photos", &group_digits(stats.in_archives + stats.to_download)),
                    ],
                )}
            }
            // Whatever had to come from Geneanet came in the preview step, and the login
            // window is closed the moment this step is started. Promising a
            // download here read as though the network were still ahead.
            if missing > 0 {
                div { class: "warning-msg",
                    {i18n.t_plural("geneanet.step5_missing", missing)}
                }
            } else if gathered > 0 && !NATIVE {
                // The web build sends the file the user loaded.
                p { class: "gn-note", {i18n.t("geneanet.step5_from_file")} }
            } else if gathered > 0 {
                p { class: "gn-note", {i18n.t("geneanet.step5_local")} }
            } else if stats.to_download + stats.to_match > 0 {
                // Reachable: a build with no login window, or a preview step that
                // could not gather. Those media are skipped, and saying so
                // before the write beats finding it in the receipt.
                div { class: "warning-msg",
                    {i18n.t_plural("geneanet.step5_missing", stats.to_download + stats.to_match)}
                }
            }
        }

        if let Some(err) = error {
            div { class: "error-msg", "{err}" }
        }
        if cancelled {
            div { class: "success-msg", role: "status", {i18n.t("import.cancelled")} }
        }

        if importing {
            // Ten thousand people and several hundred pictures is minutes of
            // work, and a bar that cannot move makes a long import look like a
            // hung one. Undoing it has no count to show.
            match progress.as_ref().filter(|p| p.total > 0 && can_cancel(&p.phase)) {
                Some(p) => rsx! {
                    ProgressBar {
                        label: i18n.t(phase_key(Some(&p.phase), fidelity)),
                        done: p.done,
                        total: p.total,
                    }
                    div { class: "gn-progress-count", {i18n.t("geneanet.step5_dont_close")} }
                },
                None => rsx! {
                    div { class: "gn-progress-block",
                        div { class: "gn-progress-label", "aria-live": "polite",
                            {i18n.t(phase_key(progress.as_ref().map(|p| p.phase.as_str()), fidelity))}
                        }
                        div { class: "gn-progress",
                            div { class: "gn-progress-fill is-indeterminate" }
                        }
                        div { class: "gn-progress-count", {i18n.t("geneanet.step5_dont_close")} }
                    }
                },
            }
            if let Some(job_id) = cancellable {
                div { class: "modal-actions",
                    CancelJobButton { kind: JobKind::Import, tree_id, job_id }
                }
            }
        } else {
            div { class: "modal-actions",
                button {
                    class: "btn btn-primary",
                    onclick: move |_| on_start.call(()),
                    {i18n.t("geneanet.step5_start")}
                }
            }
        }
    }
}

#[component]
fn GeneanetDone(tree_id: Uuid, result: GeneanetImportResult) -> Element {
    let i18n = use_i18n();

    // The import step is the tallest screen of them all and the receipt is the
    // shortest, so this is the transition where the stale offset shows.
    use_effect(scroll_import_body_to_top);

    rsx! {
        div { class: "import-done",
            div { class: "import-done-icon", "✓" }
            h3 { {i18n.t("import.done_title")} }

            ImportCounts { result: result.receipt.clone(),
                Stat { value: result.links_count, label: i18n.t("geneanet.stat_attachments") }
            }

            if result.isolated_count > 0 || result.vignettes_count > 0 {
                ul { class: "gn-findings",
                    if result.isolated_count > 0 {
                        li { class: "is-info",
                            {i18n.t_plural("geneanet.done_isolated", result.isolated_count)}
                        }
                    }
                    if result.vignettes_count > 0 {
                        li { class: "is-info",
                            {i18n.t_plural("geneanet.done_vignettes", result.vignettes_count)}
                        }
                    }
                }
            }

            if !result.isolated_people.is_empty() {
                IsolatedHomonyms { tree_id, people: result.isolated_people.clone() }
            }

            if !result.skipped.is_empty() {
                details { class: "import-warnings",
                    summary {
                        {i18n.t_plural("geneanet.skipped_photos", result.skipped.len())}
                    }
                    ul {
                        for line in result.skipped.iter().take(200) {
                            li { "{line}" }
                        }
                    }
                }
            }
            ImportWarnings { warnings: result.receipt.warnings.clone() }
        }
    }
}

/// The people created for identifications outside the tree who bear the name
/// of somebody else in it, each with the choice of keeping them as a new
/// person or merging them into that namesake.
///
/// Geneanet's owner said the person on the photograph is not the tree's
/// person of that name, which is why the import never matches them itself.
/// The owner may still have been wrong, or the namesake may have been added
/// since; only the user can tell, and this is where they are asked.
#[component]
fn IsolatedHomonyms(tree_id: Uuid, people: Vec<GeneanetIsolatedPerson>) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let mut decided = use_signal(HashMap::<Uuid, HomonymDecision>::new);

    // One small read per person, and there are a few dozen at most. Part of
    // the Geneanet import rather than of the screen behind it; the run's own
    // trace closed when the import landed, so the lookup is a short one of
    // its own.
    let found = use_resource(move || {
        let api = api.clone();
        let people = people.clone();
        let lookup = async move {
            let mut found = Vec::new();
            for person in people {
                if let Ok(homonyms) = api.person_homonyms(tree_id, person.person_id).await
                    && !homonyms.is_empty()
                {
                    found.push((person, homonyms));
                }
            }
            found
        };
        trace_ui_action(
            UiAction::GeneanetImport,
            trace_ui_action_step(UiActionStep::GeneanetHomonyms, lookup),
        )
    });
    let rows = found.read().clone().unwrap_or_default();
    if rows.is_empty() {
        return rsx! {};
    }

    rsx! {
        section { class: "gn-homonyms",
            h4 { {i18n.t("geneanet.homonyms_title")} }
            p { class: "gn-homonyms-intro", {i18n.t("geneanet.homonyms_intro")} }
            for (person, homonyms) in rows {
                div { key: "{person.person_id}", class: "gn-homonym",
                    div { class: "gn-homonym-name",
                        span { class: "sp-surname", "{person.surname}" }
                        span { class: "sp-given", " {person.given_names}" }
                    }
                    match decided.read().get(&person.person_id).copied() {
                        Some(HomonymDecision::Distinct) => rsx! {
                            p { class: "gn-homonym-done", {i18n.t("geneanet.homonym_kept")} }
                        },
                        Some(HomonymDecision::Merged(kept)) => {
                            let into = homonyms
                                .iter()
                                .find(|entry| entry.person_id == kept)
                                .map(|entry| entry.display_name.clone())
                                .unwrap_or_default();
                            rsx! {
                                p { class: "gn-homonym-done",
                                    {i18n.t_args("geneanet.homonym_merged", &[("name", &into)])}
                                }
                            }
                        }
                        None => rsx! {
                            HomonymPicker {
                                tree_id,
                                person_id: person.person_id,
                                homonyms: homonyms.clone(),
                                new_person: true,
                                on_decided: {
                                    let id = person.person_id;
                                    move |decision| {
                                        decided.write().insert(id, decision);
                                    }
                                },
                            }
                        },
                    }
                }
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════

/// Put the modal body back at the top of a screen that just replaced another.
///
/// Clamping alone (see [`ImportModal`]) only guarantees the content is
/// reachable; a receipt still wants to open at its own first line rather than
/// wherever the step before it happened to be scrolled to. Twice, because the
/// first call lands as Dioxus patches the DOM and the second after the webview
/// has laid the new screen out.
fn scroll_import_body_to_top() {
    document::eval(
        r#"
        const top = () => {
            const body = document.querySelector('.import-modal-body');
            if (body) body.scrollTop = 0;
        };
        top();
        requestAnimationFrame(top);
        "#,
    );
}

/// Which reader a picked file goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileFormat {
    /// A GEDCOM text file — the default for anything unrecognised, because it
    /// is the one format people rename freely and the reader says so itself
    /// when handed something else.
    Gedcom,
    /// A GeneWeb `.gw` export.
    Geneweb,
    /// A GEDZIP `.gdz` archive: a GEDCOM and its media in one ZIP.
    Gedzip,
}

impl FileFormat {
    fn api_name(self) -> &'static str {
        match self {
            Self::Gedcom => "gedcom",
            Self::Geneweb => "geneweb",
            Self::Gedzip => "gedzip",
        }
    }
}

/// Which reader a picked file's extension asks for.
fn format_of(file_name: &str) -> FileFormat {
    let extension = file_name.rsplit('.').next().unwrap_or_default();
    if extension.eq_ignore_ascii_case("gw") {
        FileFormat::Geneweb
    } else if extension.eq_ignore_ascii_case("gdz") {
        FileFormat::Gedzip
    } else {
        FileFormat::Gedcom
    }
}

/// Base64 for the JSON bodies that bundle the `.gw` with other fields.
fn encode_gw(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Groups thousands with a narrow no-break space — `10 254`.
///
/// The counts here run to five digits and are the whole point of the summary
/// lines, so they are worth being readable at a glance. A narrow no-break
/// space is the typographic convention in both interface languages and, unlike
/// a comma or a period, means the same thing in each.
fn group_digits(value: usize) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push('\u{202F}');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The file dialogs' filters named "Session archive", "Session" and
    /// "All files" in English whatever the reader's language: only format
    /// names may stay literal.
    #[test]
    fn the_file_dialog_filters_are_translated() {
        let source = include_str!("import_modal.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or_default();
        for (at, _) in production.match_indices(".add_filter(\"") {
            let label = &production[at + ".add_filter(\"".len()..];
            let label = &label[..label.find('"').unwrap_or(0)];
            assert!(
                ["GeneWeb", "ZIP"].contains(&label),
                "untranslated filter label {label:?}"
            );
        }
    }

    #[test]
    fn the_extension_picks_the_reader_whatever_its_case() {
        assert_eq!(format_of("tree.gw"), FileFormat::Geneweb);
        assert_eq!(format_of("MYACCOUNT_2026-08-01.GW"), FileFormat::Geneweb);
        assert_eq!(format_of("tree.gdz"), FileFormat::Gedzip);
        assert_eq!(format_of("EXPORT.GDZ"), FileFormat::Gedzip);
        assert_eq!(format_of("tree.ged"), FileFormat::Gedcom);
        // Anything unrecognised is read as GEDCOM — the reader will say so if
        // it is not, and a renamed `.ged` is common.
        assert_eq!(format_of("tree"), FileFormat::Gedcom);
    }

    #[test]
    fn digits_group_in_threes_from_the_right() {
        assert_eq!(group_digits(0), "0");
        assert_eq!(group_digits(613), "613");
        assert_eq!(group_digits(10_254), "10\u{202F}254");
        assert_eq!(group_digits(1_234_567), "1\u{202F}234\u{202F}567");
    }

    #[test]
    fn a_large_upload_reports_progress_without_overflowing() {
        assert_eq!(progress_percent(0, 0), 0);
        assert_eq!(progress_percent(50, 100), 50);
        assert_eq!(progress_percent(31_866_880, 697_900_330), 4);
        assert_eq!(progress_percent(800_000_000, 697_900_330), 100);
    }

    #[test]
    fn dropping_a_step_renumbers_the_ones_after_it() {
        // Not cosmetic: a wizard that went 1, 2, 4, 5 would read as a step
        // gone missing rather than a step that was never needed.
        let numbers = |steps: &[Step]| -> Vec<usize> {
            steps.iter().map(|step| step_number(steps, *step)).collect()
        };
        let renditions = visible_steps(false, true);
        assert_eq!(
            renditions,
            [
                Step::Gw,
                Step::Saved,
                Step::Connect,
                Step::Preview,
                Step::Import
            ]
        );
        assert_eq!(numbers(&renditions), [1, 2, 3, 4, 5]);
        assert_eq!(step_number(&renditions, Step::Archives), 0);

        let originals = visible_steps(true, true);
        assert_eq!(numbers(&originals), [1, 2, 3, 4, 5, 6]);
        assert_eq!(step_number(&originals, Step::Connect), 4);

        // Without a login window: the saved file is the only way on.
        let web = visible_steps(true, false);
        assert!(!web.contains(&Step::Connect));
        assert_eq!(step_number(&web, Step::Preview), web.len() - 1);
    }

    #[test]
    fn a_run_that_matches_nothing_never_announces_the_matching_phase() {
        // The phase is entered either way — it begins, finds no archive to
        // index and leaves — so the bar would name archive work a renditions
        // import never does.
        assert_eq!(
            phase_key(Some("matching"), MediaFidelity::Originals),
            "geneanet.phase_matching"
        );
        assert_eq!(
            phase_key(Some("matching"), MediaFidelity::Renditions),
            "geneanet.phase_media"
        );

        // Staging is the request in flight; people are what it writes first.
        for fidelity in [MediaFidelity::Renditions, MediaFidelity::Originals] {
            assert_eq!(
                phase_key(Some("staging"), fidelity),
                "geneanet.phase_people"
            );
            assert_eq!(phase_key(None, fidelity), "geneanet.phase_people");
            assert_eq!(phase_key(Some("media"), fidelity), "geneanet.phase_media");
            assert_eq!(
                phase_key(Some("finishing"), fidelity),
                "geneanet.phase_finishing"
            );
            assert_eq!(
                phase_key(Some("projections"), fidelity),
                "geneanet.phase_finishing"
            );
            assert_eq!(
                phase_key(Some("cancelling"), fidelity),
                "import.phase_cancelling"
            );
        }
    }

    #[test]
    fn only_the_originals_answer_reaches_for_the_archives() {
        // The one predicate that decides whether the archives step exists, whether the
        // sizing pass runs, and which URLs the plan asks for.
        assert!(!MediaFidelity::default().uses_archives());
        assert!(!MediaFidelity::Renditions.uses_archives());
        assert!(MediaFidelity::Originals.uses_archives());
    }

    #[test]
    fn base64_round_trips_the_bytes_a_gw_reader_needs() {
        // The point of encoding at all: a `.gw` is ISO-8859-1 unless it says
        // otherwise, so the bytes must survive the JSON trip unchanged.
        use base64::Engine as _;
        let latin1 = vec![0x52, 0x65, 0x6E, 0xE9, 0x65]; // "Renée" in ISO-8859-1
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(encode_gw(&latin1))
            .expect("decodes");
        assert_eq!(decoded, latin1);
    }
}
