//! The import modal: pick a file, or walk the Geneanet flow.
//!
//! Replaces the bare native file picker the tree card's menu used to open.
//! That picker was fine for the one case it handled and no use at all for the
//! other: importing a Geneanet tree *with its photos* is not a file import.
//! Two of its three inputs cannot be downloaded, one of them exists only
//! behind a logged-in session, and most users have never heard of a `.gw`
//! file. So the Geneanet side is as much a set of instructions as a form.
//!
//! See `docs/specifications/ui-import.md`.
//!
//! # Shape
//!
//! Two tabs. **File** is the old behaviour, made to work in a browser as well
//! as on the desktop. **Geneanet** is four or five steps of which exactly one
//! is expanded — the current one. A settled step collapses to a one-line
//! receipt of what it decided; steps not yet reachable are visible but dimmed,
//! so the whole journey is legible from the first second.
//!
//! # Four steps or five
//!
//! Step 1 asks which bytes to keep for each photograph, and that answer
//! decides how long the journey is. Keeping Geneanet's own renditions needs
//! nothing but the login, so there is no archive step at all and the flow is
//! four steps. Keeping the original uploads needs the user's data archives —
//! another Geneanet request, an email, and gigabytes of ZIP — so step 2
//! appears and the numbering shifts with it. Renditions is the default because
//! it is the only path a user who has never downloaded their data can take.
//!
//! # What only the desktop build can do
//!
//! Steps 2, 3 and the photo half of 5 are desktop-only, and the tab says so
//! rather than offering controls that cannot work:
//!
//! - **Step 2** needs to read multi-gigabyte ZIPs by path, a few kilobytes
//!   each. A browser would have to load them whole into memory to see the same
//!   central directory.
//! - **Step 3** needs a second browser window on geneanet.org whose session
//!   this app can then issue requests through. A popup from a web page is
//!   cross-origin: nothing comes back out of it.
//!
//! On web the tab still runs step 1 and imports the `.gw` — the genealogy
//! arrives, the photos do not.

use std::collections::HashMap;

use dioxus::prelude::*;
#[cfg(target_arch = "wasm32")]
use serde::Deserialize;
use uuid::Uuid;

use crate::api::{
    ApiClient, ArchiveIndex, GeneanetImportBody, GeneanetImportResult, GeneanetPreview,
    GeneanetPreviewBody, GeneanetSessionBody, GwInspection, ImportProgress, ImportResult,
    IndexedArchive, MediaFidelity,
};
use crate::geneanet::{Collect, GeneanetEvent, WindowStrings, use_geneanet_bridge};
use crate::i18n::use_i18n;
use crate::ui_observability::{
    UiAction, UiActionStep, UiActionTrace, trace_ui_action, trace_ui_action_step,
    use_ui_action_trace,
};

/// Whether this build can open a Geneanet login window and read local
/// archives by path.
///
/// One constant rather than a `cfg!` at each of the six places that ask,
/// because they must all agree: a tab that offered step 3 but hid step 2
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
/// [`MediaFidelity::Renditions`] import, which has no archive to read — see
/// [`step_number`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Step {
    Gw,
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
}

/// Where the login window has got to, for the two progress bars of step 3.
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
    // and a stray click on the page behind is all it would take.
    let busy = use_signal(|| false);

    // The scroll offset lives on `.import-modal-body`, which outlives every
    // screen rendered inside it — so a step collapsing, or the result taking
    // over from a scrolled step 5, can leave the offset far past the end of
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
        div {
            class: "modal-backdrop import-modal-backdrop",
            // Dismiss on press, not click: a click fires on the common
            // ancestor of mousedown/mouseup, so selecting text inside and
            // releasing outside would close the modal. Never while busy —
            // see `busy`.
            onmousedown: move |_| {
                if !busy() {
                    on_close.call(());
                }
            },
            div {
                class: "import-modal",
                onmousedown: move |e: Event<MouseData>| e.stop_propagation(),

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
        let name = short_name(&file.name());
        busy.set(true);
        error.set(None);
        progress.set(None);
        spawn(async move {
            let format = format_of(&name);
            let outcome = trace_ui_action(
                UiAction::Import(format.api_name()),
                run_file_import_job(&api, tree_id, format, file, &mut progress),
            )
            .await;
            busy.set(false);
            progress.set(None);
            match outcome {
                Ok(imported) => {
                    api.invalidate_tree(tree_id);
                    result.set(Some(imported.clone()));
                    on_imported.call(ImportOutcome::File(imported));
                }
                Err(e) => error.set(Some(format!("{e}"))),
            }
        });
    };

    if let Some(imported) = result() {
        return rsx! {
            div { class: "import-done",
                div { class: "import-done-icon", "✓" }
                h3 { {i18n.t("import.done_title")} }
                ImportCounts { result: imported }
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
                        result.set(None);
                        progress.set(None);
                    },
                }

                if let Some(file) = picked() {
                    div { class: "import-drop-icon", "📄" }
                    div { class: "import-drop-name", {short_name(&file.name())} }
                    div { class: "import-drop-hint",
                        {i18n.t_args("import.file_size", &[("size", &human_size(file.size().try_into().unwrap_or(usize::MAX)))])}
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

            if let Some(current) = progress() {
                match current {
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
                                _ => "import.phase_starting",
                            }),
                            done,
                            total: if phase == "media" { total } else { 0 },
                        }
                    },
                }
            }

            div { class: "modal-actions",
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

async fn run_file_import_job(
    api: &ApiClient,
    tree_id: Uuid,
    format: FileFormat,
    file: dioxus::html::FileData,
    progress: &mut Signal<Option<FileImportUiProgress>>,
) -> Result<ImportResult, crate::api::ApiError> {
    let job_id = trace_ui_action_step(
        UiActionStep::ImportUpload,
        start_file_import_upload(api, tree_id, format, file, progress),
    )
    .await?;
    api.invalidate_tree_list();

    trace_ui_action_step(UiActionStep::ImportPoll, async {
        loop {
            let status = api.file_import_status(tree_id, job_id).await?;
            progress.set(Some(FileImportUiProgress::Server {
                phase: status.phase.clone(),
                done: status.done,
                total: status.total,
            }));
            match status.phase.as_str() {
                "completed" => {
                    return status.result.ok_or_else(|| crate::api::ApiError::Api {
                        status: 500,
                        body: "completed import has no result".to_string(),
                    });
                }
                "failed" => {
                    return Err(crate::api::ApiError::Api {
                        status: 422,
                        body: status.error.unwrap_or_else(|| "import_failed".to_string()),
                    });
                }
                _ => crate::utils::sleep_ms(500).await,
            }
        }
    })
    .await
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
                break Err(crate::api::ApiError::Api {
                    status: 0,
                    body: "file upload failed".to_string(),
                });
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

/// The counts every import reports, however it was sourced.
#[component]
fn ImportCounts(result: ImportResult) -> Element {
    let i18n = use_i18n();

    rsx! {
        div { class: "import-stats",
            Stat { value: result.persons_count, label: i18n.t("import.stat_persons") }
            Stat { value: result.families_count, label: i18n.t("import.stat_families") }
            Stat { value: result.events_count, label: i18n.t("import.stat_events") }
            Stat { value: result.sources_count, label: i18n.t("import.stat_sources") }
            Stat { value: result.places_count, label: i18n.t("import.stat_places") }
            Stat { value: result.media_count, label: i18n.t("import.stat_media") }
        }
        if !result.warnings.is_empty() {
            details { class: "import-warnings",
                summary {
                    {i18n.t_plural("import.warning_count", result.warnings.len())}
                }
                ul {
                    for warning in result.warnings.iter().take(100) {
                        li { "{warning}" }
                    }
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

    let mut archives = use_signal(Vec::<IndexedArchive>::new);
    let mut archives_skipped = use_signal(|| false);
    let archive_error = use_signal(|| None::<String>);

    let mut collected = use_signal(|| None::<Collected>);
    let mut connecting = use_signal(|| None::<Connecting>);
    let mut connect_error = use_signal(|| None::<String>);

    let mut preview = use_signal(|| None::<GeneanetPreview>);
    let mut preview_error = use_signal(|| None::<String>);
    let mut override_mismatch = use_signal(|| false);

    let mut importing = busy;
    // Media the login window retrieved, keyed by URL and represented by a
    // staged local path. The window session owns those files through step 5.
    let mut fetched = use_signal(HashMap::<String, String>::new);
    let mut gathering = use_signal(|| false);
    let mut import_progress = use_signal(|| None::<crate::api::ImportProgress>);
    // Progress through the fetch pass that precedes the write, when the login
    // window has to retrieve media the archives did not cover.
    let mut fetch_progress = use_signal(|| None::<(usize, usize)>);
    let mut import_error = use_signal(|| None::<String>);
    let mut import_result = use_signal(|| None::<GeneanetImportResult>);

    // Set once the window has been closed, so neither of the two paths that
    // close it can do so twice.
    let mut window_closed = use_signal(|| false);

    // The window exists for one thing: fetching what the archives cannot
    // account for. The preview is the first moment we know that is nothing —
    // every attached deposit single-page and every one of them size-matched —
    // and there is no reason to leave a window open with no job left.
    {
        let bridge = bridge.clone();
        use_effect(move || {
            let Some(stats) = preview() else { return };
            if stats.to_match + stats.to_download > 0 || window_closed() {
                return;
            }
            if let Some(bridge) = &bridge {
                bridge.close();
            }
            window_closed.set(true);
        });
    }

    // Dismissing the wizard closes the window with it. Without this it would
    // outlive the modal that opened it, with nothing left able to reach it.
    {
        let bridge = bridge.clone();
        use_drop(move || {
            if let Some(bridge) = &bridge {
                bridge.close();
            }
        });
    }

    // Step 2 is settled either by adding archives or by explicitly skipping;
    // both let the flow move on, and the difference is only whether photos get
    // downloaded. A renditions import has no step 2 to settle.
    let archives_settled =
        move || !fidelity().uses_archives() || !archives.read().is_empty() || archives_skipped();
    let can_connect = move || gw().is_some() && (archives_settled() || !NATIVE);
    let can_preview = move || collected().is_some();

    // Changing the answer to step 1 invalidates every decision that depended
    // on it, so they are dropped rather than left to look settled.
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
        fetched.write().clear();

        if next.uses_archives() {
            // The archives are matched on exact byte lengths, and a list-only
            // collection has none. Keeping it would silently match nothing and
            // download everything, so the collection is taken again.
            let unmeasured = collected
                .read()
                .as_ref()
                .is_some_and(|c| c.deposit_sizes.is_empty());
            if unmeasured {
                collected.set(None);
            }
        } else {
            archives.write().clear();
            archives_skipped.set(false);
        }
    };

    // ── Step 3 — drive the login window ──────────────────────────────
    let start_connect = {
        let bridge = bridge.clone();
        let trace = trace.clone();
        move |_| {
            let Some(bridge) = bridge.clone() else { return };
            let trace = trace.clone();
            let (tx, mut rx) = futures_channel::mpsc::unbounded::<GeneanetEvent>();
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
                    invalid_collection: i18n.t("geneanet.error_collection_invalid"),
                    cancel_hint: i18n.t("geneanet.window_cancel_hint"),
                    idle: i18n.t("geneanet.window_idle"),
                },
                // The sizing pass is one `HEAD` per deposit and exists only to
                // match the archives. A renditions import never opens one, so
                // it is several hundred requests nothing would read.
                if fidelity().uses_archives() {
                    Collect::ListAndSizes
                } else {
                    Collect::ListOnly
                },
            );

            spawn(async move {
                trace
                    .step(UiActionStep::GeneanetConnect, async {
                        use futures_util::StreamExt as _;
                        while let Some(event) = rx.next().await {
                            match event {
                                GeneanetEvent::Opened => {
                                    connecting.set(Some(Connecting::WaitingForLogin));
                                }
                                GeneanetEvent::SignedIn => {
                                    connecting
                                        .set(Some(Connecting::Collecting { done: 0, total: 0 }));
                                }
                                GeneanetEvent::Collecting { done, total } => {
                                    connecting.set(Some(Connecting::Collecting { done, total }));
                                }
                                GeneanetEvent::Sizing { done, total } => {
                                    connecting.set(Some(Connecting::Sizing { done, total }));
                                }
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
                                    }));
                                    connecting.set(None);
                                    open.set(Step::Preview);
                                    break;
                                }
                                // The fetch events belong to the import step, which
                                // drives its own channel; nothing here reacts to them.
                                GeneanetEvent::Fetched { .. }
                                | GeneanetEvent::Fetching { .. }
                                | GeneanetEvent::FetchDone => {}
                                // Closing the window before signing in is not an
                                // error — the step simply returns to where it was.
                                GeneanetEvent::Cancelled => {
                                    connecting.set(None);
                                    break;
                                }
                                GeneanetEvent::Failed(message) => {
                                    connect_error.set(Some(message));
                                    connecting.set(None);
                                    break;
                                }
                            }
                        }
                    })
                    .await;
            });
        }
    };

    // ── Step 4 — work out what will happen, and gather what it needs ──
    //
    // The gathering is here rather than in step 5 on purpose. It is the last
    // thing that touches Geneanet, so doing it here means the login window is
    // finished with before the user commits to anything — and step 5 becomes
    // purely local, which is what makes an air-gapped import possible at all.
    // Nothing is written until step 5 either way.
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

            let needed = match trace
                .step(UiActionStep::GeneanetPreview, async {
                    match api.preview_geneanet_import(&body).await {
                        Ok(stats) => preview.set(Some(stats)),
                        Err(e) => return Err(e),
                    }

                    // Anything the server cannot produce from the archives. A session
                    // loaded from disk may already carry it, in which case nothing
                    // here touches the network at all.
                    api.plan_geneanet_import(&body)
                        .await
                        .map(|plan| plan.needed)
                })
                .await
            {
                Ok(needed) => needed,
                Err(e) => {
                    preview_error.set(Some(format!("{e}")));
                    return;
                }
            };

            let mut urls: Vec<String> = Vec::new();
            for item in &needed {
                // Deduplicated: every page of a single-page deposit shares one
                // original URL, and fetching it twice would double the bytes.
                if !fetched.read().contains_key(&item.url) && !urls.contains(&item.url) {
                    urls.push(item.url.clone());
                }
            }

            if urls.is_empty() {
                return;
            }

            let Some(bridge) = bridge.clone() else {
                preview_error.set(Some(i18n.t("geneanet.error_window_needed")));
                return;
            };

            gathering.set(true);
            let (tx, mut rx) = futures_channel::mpsc::unbounded::<GeneanetEvent>();
            fetch_progress.set(Some((0, urls.len())));
            bridge.fetch(urls, tx);

            trace
                .step(UiActionStep::GeneanetCollect, async {
                    use futures_util::StreamExt as _;
                    while let Some(event) = rx.next().await {
                        match event {
                            GeneanetEvent::Fetched { url, path, error } => {
                                if let Some(path) = path {
                                    fetched.write().insert(url, path);
                                } else if error.is_some() {
                                    // One unreachable medium is reported by the import
                                    // as skipped; it does not end the run.
                                }
                            }
                            GeneanetEvent::Fetching { done, total } => {
                                fetch_progress.set(Some((done, total)));
                            }
                            GeneanetEvent::FetchDone => break,
                            GeneanetEvent::Failed(message) => {
                                preview_error.set(Some(message));
                                break;
                            }
                            GeneanetEvent::Cancelled => break,
                            _ => {}
                        }
                    }
                })
                .await;

            fetch_progress.set(None);
            gathering.set(false);

            // The window has no more network work, but its session owns the
            // staged files until the local import has consumed them.
        });
    };

    // ── Step 5 — write, with no network left to wait on ──────────────
    let api_import = api.clone();
    let bridge_import = bridge.clone();
    let trace_import = trace.clone();
    let run_import = move |_| {
        let api = api_import.clone();
        let bridge = bridge_import.clone();
        let trace = trace_import.clone();
        let (Some(file), Some(collected)) = (gw(), collected()) else {
            return;
        };
        if importing() {
            return;
        }
        importing.set(true);
        import_error.set(None);

        import_progress.set(Some(ImportProgress {
            phase: "staging".to_string(),
            done: 0,
            total: 0,
        }));

        spawn(async move {
            let body = GeneanetImportBody {
                gw_base64: encode_gw(&file.bytes),
                file_name: file.name.clone(),
                collection: collected.collection.clone(),
                deposit_sizes: collected.deposit_sizes.clone(),
                archive_paths: archives.read().iter().map(|a| a.path.clone()).collect(),
                fetched: fetched.read().clone(),
                media_fidelity: fidelity(),
            };
            let outcome = async {
                let started = trace
                    .step(
                        UiActionStep::GeneanetUpload,
                        api.import_geneanet(tree_id, &body),
                    )
                    .await?;
                if let Some(bridge) = &bridge {
                    bridge.close();
                }
                window_closed.set(true);

                trace
                    .step(UiActionStep::GeneanetPoll, async {
                        loop {
                            let status = api.file_import_status(tree_id, started.job_id).await?;
                            import_progress.set(Some(ImportProgress {
                                phase: status.phase.clone(),
                                done: status.done,
                                total: status.total,
                            }));
                            match status.phase.as_str() {
                                "completed" => {
                                    break status.geneanet_result.ok_or_else(|| {
                                        crate::api::ApiError::Api {
                                            status: 500,
                                            body: "completed Geneanet import has no result"
                                                .to_string(),
                                        }
                                    });
                                }
                                "failed" => {
                                    break Err(crate::api::ApiError::Api {
                                        status: 422,
                                        body: status
                                            .error
                                            .unwrap_or_else(|| "import_failed".to_string()),
                                    });
                                }
                                _ => crate::utils::sleep_ms(500).await,
                            }
                        }
                    })
                    .await
            }
            .await;
            importing.set(false);
            import_progress.set(None);

            match outcome {
                Ok(result) => {
                    api.invalidate_tree(tree_id);
                    import_result.set(Some(result.clone()));
                    on_imported.call(ImportOutcome::Geneanet(result));
                }
                Err(e) => import_error.set(Some(format!("{e}"))),
            }
            // The assistant is over either way: closing the trace here rather
            // than waiting for the tab to unmount keeps the receipt the user
            // is now reading out of the import's duration.
            trace.finish();
        });
    };

    if let Some(result) = import_result() {
        return rsx! { GeneanetDone { result } };
    }

    // How many steps this journey has, and therefore what each one is called.
    let archives_step = fidelity().uses_archives();
    let number = move |step: Step| step_number(step, archives_step);

    rsx! {
        div { class: "gn-steps",

            // ── Step 1 — the .gw file and the media choice ───────────
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
                    on_fidelity: choose_fidelity,
                    on_settled: move |_| {
                        open.set(if NATIVE && archives_step { Step::Archives } else { Step::Connect });
                    },
                }
            }

            // ── Step 2 — the photo archives ──────────────────────────
            // Only for an originals import. A renditions one never opens an
            // archive, so offering the step would ask for gigabytes of ZIP to
            // do nothing with.
            if archives_step {
            StepShell {
                index: number(Step::Archives),
                title: i18n.t("geneanet.step2_title"),
                open: open() == Step::Archives,
                reachable: gw().is_some(),
                summary: if archives_skipped() {
                    Some(i18n.t("geneanet.step2_skipped"))
                } else if archives.read().is_empty() {
                    None
                } else {
                    Some(i18n.t_args(
                        "geneanet.step2_summary",
                        &[
                            ("archives", &archives.read().len().to_string()),
                            ("files", &group_digits(
                                archives.read().iter().map(|a| a.file_count).sum::<usize>()
                            )),
                        ],
                    ))
                },
                on_open: move |_| open.set(Step::Archives),
                ArchiveStep {
                    archives,
                    archives_skipped,
                    archive_error,
                    on_settled: move |_| open.set(Step::Connect),
                }
            }
            }

            // ── Step 3 — sign in and collect ─────────────────────────
            StepShell {
                index: number(Step::Connect),
                title: i18n.t("geneanet.step3_title"),
                open: open() == Step::Connect,
                reachable: can_connect(),
                summary: collected().map(|c| {
                    let photos = group_digits(c.photo_count);
                    match c.account {
                        Some(account) => i18n.t_args(
                            "geneanet.step3_summary_named",
                            &[("account", &account), ("count", &photos)],
                        ),
                        None => i18n.t_args("geneanet.step3_summary", &[("count", &photos)]),
                    }
                }),
                on_open: move |_| open.set(Step::Connect),
                ConnectStep {
                    available: bridge.is_some(),
                    connecting: connecting(),
                    collected,
                    fetched,
                    error: connect_error,
                    on_start: start_connect,
                    on_loaded: move |_| open.set(Step::Preview),
                }
            }

            // ── Step 4 — what will be imported ───────────────────────
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
                    session_error: preview_error,
                    gathering: gathering(),
                    fetch_progress: fetch_progress(),
                    override_mismatch,
                    on_compute: run_preview,
                    on_back: move |_| open.set(Step::Gw),
                    on_continue: move |_| open.set(Step::Import),
                }
            }

            // ── Step 5 — import ──────────────────────────────────────
            StepShell {
                index: number(Step::Import),
                title: i18n.t("geneanet.step5_title"),
                open: open() == Step::Import,
                reachable: preview().is_some() && (!preview().unwrap_or_default().mismatch || override_mismatch()),
                summary: None,
                on_open: move |_| open.set(Step::Import),
                ImportStep {
                    preview: preview(),
                    importing: importing(),
                    progress: import_progress(),
                    error: import_error(),
                    on_start: run_import,
                }
            }
        }
    }
}

/// What a step is called on screen.
///
/// The journey is four steps or five depending on step 1's answer, and the
/// numbers have to say so: a wizard that skipped from 1 to 3 would read as a
/// step gone missing rather than a step that was never needed.
const fn step_number(step: Step, archives_step: bool) -> usize {
    // What step 2 costs the steps after it: one, or nothing at all.
    let shift = archives_step as usize;
    match step {
        Step::Gw => 1,
        Step::Archives => 2,
        Step::Connect => 2 + shift,
        Step::Preview => 3 + shift,
        Step::Import => 4 + shift,
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

// ── Step 1 ──────────────────────────────────────────────────────────

#[component]
fn GwStep(
    gw: Signal<Option<GwFile>>,
    gw_error: Signal<Option<String>>,
    /// Which bytes to keep per medium — the answer that decides whether there
    /// is a step 2 at all.
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
                .add_filter("All files", &["*"])
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
                        format!("{e}")
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

// ── Step 2 ──────────────────────────────────────────────────────────

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

    if !NATIVE {
        return rsx! {
            p { class: "gn-lead", {i18n.t("geneanet.step2_lead")} }
            div { class: "gn-desktop-only",
                strong { {i18n.t("geneanet.desktop_only_title")} }
                p { {i18n.t("geneanet.step2_web")} }
            }
        };
    }

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
                Err(e) => archive_error.set(Some(format!("{e}"))),
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

// ── Step 3 ──────────────────────────────────────────────────────────

#[component]
fn ConnectStep(
    available: bool,
    connecting: Option<Connecting>,
    collected: Signal<Option<Collected>>,
    fetched: Signal<HashMap<String, String>>,
    error: Signal<Option<String>>,
    on_start: EventHandler<()>,
    on_loaded: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();

    // `on_loaded` is called by the control that does the loading, not by an
    // effect watching `collected`. An effect runs on mount too, so reopening
    // an already-settled step 3 would read "a session exists" as "a session
    // just arrived" and bounce the user forward to step 4.
    let reuse = rsx! { SessionControls { collected, fetched, error, on_loaded } };

    if !available {
        return rsx! {
            p { class: "gn-lead", {i18n.t("geneanet.step3_lead")} }
            div { class: "gn-desktop-only",
                strong { {i18n.t("geneanet.desktop_only_title")} }
                p { {i18n.t("geneanet.step3_web")} }
            }
            if let Some(err) = error() {
                div { class: "error-msg", "{err}" }
            }
            {reuse}
        };
    }

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
                {reuse}
            },
        }
    }
}

/// Saving what has been collected, and loading it back.
///
/// Shown in step 3 and again in step 4, because the file is worth saving at
/// two different moments and they are not the same file: after step 3 it holds
/// the collection and the deposit sizes; after step 4 it holds the gathered
/// media too and needs no Geneanet connection at all. Step 3 also collapses
/// once it is settled, so a control that lived only there would vanish exactly
/// when the complete file became available.
#[component]
fn SessionControls(
    collected: Signal<Option<Collected>>,
    fetched: Signal<HashMap<String, String>>,
    error: Signal<Option<String>>,
    /// Fired only when a file has just been read — never merely because one
    /// was loaded earlier.
    on_loaded: Option<EventHandler<()>>,
) -> Element {
    let api = use_context::<ApiClient>();
    let i18n = use_i18n();
    let trace = use_context::<UiActionTrace>();
    let mut collected = collected;
    let mut fetched = fetched;
    let mut error = error;
    let mut busy = use_signal(|| false);

    let api_save = api.clone();
    let trace_save = trace.clone();
    let save = move |_: Event<MouseData>| {
        let api = api_save.clone();
        let trace = trace_save.clone();
        let Some(current) = collected() else { return };
        spawn(async move {
            let body = GeneanetSessionBody {
                collection: current.collection.clone(),
                deposit_sizes: current.deposit_sizes.clone(),
                account: current.account.clone(),
                media: fetched.read().clone(),
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
                    error.set(Some(format!("{e}")));
                    return;
                }
            };

            let Some(file) = rfd::AsyncFileDialog::new()
                .add_filter("Session archive", &["zip"])
                .set_file_name("geneanet-session.zip")
                .set_title(i18n.t("geneanet.session_save"))
                .save_file()
                .await
            else {
                return;
            };
            if let Err(e) = trace
                .step(UiActionStep::GeneanetWrite, file.write(&archive))
                .await
            {
                error.set(Some(format!("{e}")));
            }
        });
    };

    let api_load = api.clone();
    let trace_load = trace.clone();
    let load = move |_: Event<MouseData>| {
        let api = api_load.clone();
        let trace = trace_load.clone();
        spawn(async move {
            let Some(file) = rfd::AsyncFileDialog::new()
                // A bare JSON collection still loads — the reader tells the
                // shapes apart by content, not by extension.
                .add_filter("Session", &["zip", "json"])
                .set_title(i18n.t("geneanet.session_load"))
                .pick_file()
                .await
            else {
                return;
            };

            busy.set(true);
            error.set(None);
            let bytes = trace.step(UiActionStep::GeneanetRead, file.read()).await;
            let restored = trace
                .step(
                    UiActionStep::GeneanetSessionDecode,
                    api.decode_geneanet_session(bytes),
                )
                .await;
            busy.set(false);

            match restored {
                Ok(session) => {
                    collected.set(Some(Collected {
                        collection: session.collection,
                        deposit_sizes: session.deposit_sizes,
                        // A saved session carries no cookie and needs none: it
                        // is the collection, not a way back to the account.
                        cookie: None,
                        account: session.account,
                        photo_count: session.photo_count,
                    }));
                    // A file saved after step 4 brings the media with it, and
                    // step 4 then asks the window for nothing.
                    fetched.set(session.media);
                    if let Some(on_loaded) = &on_loaded {
                        on_loaded.call(());
                    }
                }
                Err(e) => error.set(Some(format!("{e}"))),
            }
        });
    };

    let ready = collected().is_some();
    let complete = !fetched.read().is_empty();

    rsx! {
        // A quiet strip rather than a block of its own. These are secondary to
        // whatever step they sit in — a way out, not the way on — so they read
        // as a footnote and never compete with the step's own action.
        //
        // The buttons come after the label that explains them: an earlier
        // version put them above it, which read backwards.
        div { class: "gn-session",
            details { class: "gn-session-why",
                summary { {i18n.t("geneanet.session_reuse")} }
                p { {i18n.t("geneanet.session_reuse_why")} }
            }
            div { class: "gn-session-actions",
                button {
                    class: "gn-session-btn",
                    r#type: "button",
                    disabled: busy(),
                    onclick: load,
                    span { class: "gn-session-icon", "\u{2913}" }
                    if busy() { {i18n.t("geneanet.reading")} } else { {i18n.t("geneanet.session_load")} }
                }
                if ready {
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

// ── Step 4 ──────────────────────────────────────────────────────────

#[component]
fn PreviewStep(
    preview: Option<GeneanetPreview>,
    error: Option<String>,
    /// Which bytes this run keeps, so the findings describe the work it will
    /// actually do rather than the archive matching it is not doing.
    fidelity: MediaFidelity,
    collected: Signal<Option<Collected>>,
    fetched: Signal<HashMap<String, String>>,
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
    // have settled, and re-running it is how "Edit" on step 2 takes effect.
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

        ul { class: "gn-findings",
            if !fidelity.uses_archives() {
                if stats.to_download > 0 {
                    li { class: "is-info",
                        {i18n.t_args(
                            "geneanet.finding_renditions",
                            &[("count", &group_digits(stats.to_download))],
                        )}
                    }
                }
            } else if stats.to_download == 0 && stats.in_archives > 0 {
                li { class: "is-good",
                    {i18n.t_args(
                        "geneanet.finding_all_local",
                        &[("count", &group_digits(stats.in_archives))],
                    )}
                }
            } else if stats.to_download > 0 {
                li { class: "is-info",
                    {i18n.t_args(
                        "geneanet.finding_to_download",
                        &[
                            ("local", &group_digits(stats.in_archives)),
                            ("remote", &group_digits(stats.to_download)),
                        ],
                    )}
                }
            }
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
            if stats.unlinked_names > 0 {
                li { class: "is-info",
                    details {
                        summary {
                            {i18n.t_plural("geneanet.finding_unlinked_names", stats.unlinked_names)}
                        }
                        ul {
                            for name in stats.unlinked_names_sample.iter() {
                                li { "{name}" }
                            }
                        }
                    }
                }
            }
            if stats.outside_tree > 0 {
                li { class: "is-warn",
                    details {
                        summary {
                            {i18n.t_plural("geneanet.finding_outside_tree", stats.outside_tree)}
                        }
                        ul {
                            for name in stats.outside_tree_names.iter() {
                                li { "{name}" }
                            }
                        }
                    }
                }
            }
            if stats.ambiguous > 0 {
                li { class: "is-warn",
                    details {
                        summary {
                            {i18n.t_plural("geneanet.finding_ambiguous", stats.ambiguous)}
                        }
                        ul {
                            for name in stats.ambiguous_names.iter() {
                                li { "{name}" }
                            }
                        }
                    }
                }
            }
        }

        // Offered here as well as in step 3, and this is the more valuable of
        // the two: once the gathering is done the file needs no Geneanet
        // connection at all. Step 3 also collapses once settled, so a control
        // only there disappears exactly when it becomes worth using.
        if !gathering {
            SessionControls { collected, fetched, error: session_error, on_loaded: None }
        }

        if gathering {
            // The last thing that touches Geneanet. Continuing is held until
            // it finishes, so step 5 has everything it needs and the login
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

// ── Step 5 ──────────────────────────────────────────────────────────

#[component]
fn ImportStep(
    preview: Option<GeneanetPreview>,
    importing: bool,
    progress: Option<crate::api::ImportProgress>,
    error: Option<String>,
    on_start: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();

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
            if stats.to_download > 0 {
                p { class: "gn-note",
                    {i18n.t_plural("geneanet.step5_downloads", stats.to_download)}
                }
            }
        }

        if let Some(err) = error {
            div { class: "error-msg", "{err}" }
        }

        if importing {
            // Ten thousand people and several hundred pictures is minutes of
            // work, and a bar that cannot move makes a long import look like a
            // hung one.
            match progress.as_ref().filter(|p| p.total > 0) {
                Some(p) => rsx! {
                    ProgressBar {
                        label: i18n.t(match p.phase.as_str() {
                            "people" => "geneanet.phase_people",
                            "matching" => "geneanet.phase_matching",
                            "finishing" => "geneanet.phase_finishing",
                            _ => "geneanet.phase_media",
                        }),
                        done: p.done,
                        total: p.total,
                    }
                    div { class: "gn-progress-count", {i18n.t("geneanet.step5_dont_close")} }
                },
                None => rsx! {
                    div { class: "gn-progress-block",
                        div { class: "gn-progress-label", "aria-live": "polite",
                            {i18n.t(match progress.as_ref().map(|p| p.phase.as_str()) {
                                Some("matching") => "geneanet.phase_matching",
                                Some("finishing") => "geneanet.phase_finishing",
                                Some("media") => "geneanet.phase_media",
                                _ => "geneanet.phase_people",
                            })}
                        }
                        div { class: "gn-progress",
                            div { class: "gn-progress-fill is-indeterminate" }
                        }
                        div { class: "gn-progress-count", {i18n.t("geneanet.step5_dont_close")} }
                    }
                },
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
fn GeneanetDone(result: GeneanetImportResult) -> Element {
    let i18n = use_i18n();

    // Step 5 is the tallest screen of the five and the receipt is the
    // shortest, so this is the transition where the stale offset shows.
    use_effect(scroll_import_body_to_top);

    rsx! {
        div { class: "import-done",
            div { class: "import-done-icon", "✓" }
            h3 { {i18n.t("import.done_title")} }

            div { class: "import-stats",
                Stat { value: result.persons_count, label: i18n.t("import.stat_persons") }
                Stat { value: result.families_count, label: i18n.t("import.stat_families") }
                Stat { value: result.media_count, label: i18n.t("import.stat_media") }
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
            if !result.warnings.is_empty() {
                details { class: "import-warnings",
                    summary {
                        {i18n.t_plural("import.warning_count", result.warnings.len())}
                    }
                    ul {
                        for warning in result.warnings.iter().take(100) {
                            li { "{warning}" }
                        }
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

/// The last path component of an engine-reported file name.
fn short_name(raw: &str) -> String {
    raw.rsplit(['/', '\\']).next().unwrap_or(raw).to_string()
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

/// A byte count in the largest unit that leaves it above one.
fn human_size(bytes: usize) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[0])
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_dropped_path_is_reduced_to_its_file_name() {
        assert_eq!(short_name("/home/a/tree.gw"), "tree.gw");
        assert_eq!(short_name(r"C:\exports\tree.gw"), "tree.gw");
        assert_eq!(short_name("tree.gw"), "tree.gw");
    }

    #[test]
    fn digits_group_in_threes_from_the_right() {
        assert_eq!(group_digits(0), "0");
        assert_eq!(group_digits(613), "613");
        assert_eq!(group_digits(10_254), "10\u{202F}254");
        assert_eq!(group_digits(1_234_567), "1\u{202F}234\u{202F}567");
    }

    #[test]
    fn sizes_read_in_the_largest_unit_above_one() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(2048), "2.0 KB");
        assert_eq!(human_size(5 * 1024 * 1024), "5.0 MB");
    }

    #[test]
    fn a_large_upload_reports_progress_without_overflowing() {
        assert_eq!(progress_percent(0, 0), 0);
        assert_eq!(progress_percent(50, 100), 50);
        assert_eq!(progress_percent(31_866_880, 697_900_330), 4);
        assert_eq!(progress_percent(800_000_000, 697_900_330), 100);
    }

    #[test]
    fn dropping_the_archive_step_renumbers_the_ones_after_it() {
        // Not cosmetic: a wizard that went 1, 3, 4, 5 would read as a step
        // gone missing rather than a step that was never needed.
        let renditions: Vec<usize> = [Step::Gw, Step::Connect, Step::Preview, Step::Import]
            .into_iter()
            .map(|step| step_number(step, false))
            .collect();
        assert_eq!(renditions, [1, 2, 3, 4]);

        let originals: Vec<usize> = [
            Step::Gw,
            Step::Archives,
            Step::Connect,
            Step::Preview,
            Step::Import,
        ]
        .into_iter()
        .map(|step| step_number(step, true))
        .collect();
        assert_eq!(originals, [1, 2, 3, 4, 5]);
    }

    #[test]
    fn only_the_originals_answer_reaches_for_the_archives() {
        // The one predicate that decides whether step 2 exists, whether the
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
