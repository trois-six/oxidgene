//! The import modal: pick a file, or walk the Geneanet flow.
//!
//! Replaces the bare native file picker the tree card's menu used to open.
//! That picker was fine for the one case it handled and no use at all for the
//! other: importing a Geneanet tree *with its photos* is not a file import.
//! Two of its three inputs cannot be downloaded, one of them exists only
//! behind a logged-in session, and most users have never heard of a `.gw`
//! file. So the Geneanet side is as much a set of instructions as a form.
//!
//! See `docs/specifications/ui-geneanet-import.md`.
//!
//! # Shape
//!
//! Two tabs. **File** is the old behaviour, made to work in a browser as well
//! as on the desktop. **Geneanet** is five steps of which exactly one is
//! expanded — the current one. A settled step collapses to a one-line receipt
//! of what it decided; steps not yet reachable are visible but dimmed, so the
//! whole journey is legible from the first second.
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

use dioxus::html::HasFileData;
use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::{
    ApiClient, ArchiveIndex, GeneanetImportBody, GeneanetImportResult, GeneanetPreview,
    GeneanetPreviewBody, GwInspection, ImportResult, IndexedArchive,
};
use crate::geneanet::{GeneanetEvent, use_geneanet_bridge};
use crate::i18n::use_i18n;

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

/// The Geneanet flow's five steps, in order.
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

    rsx! {
        div {
            class: "modal-backdrop",
            // Dismiss on press, not click: a click fires on the common
            // ancestor of mousedown/mouseup, so selecting text inside and
            // releasing outside would close the modal.
            onmousedown: move |_| on_close.call(()),
            div {
                class: "import-modal",
                onmousedown: move |e: Event<MouseData>| e.stop_propagation(),

                div { class: "import-modal-header",
                    h2 {
                        {i18n.t_args("import.title", &[("tree", &tree_name)])}
                    }
                    button {
                        class: "person-form-close",
                        onclick: move |_| on_close.call(()),
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
                            FileTab { tree_id, on_imported }
                        },
                        Tab::Geneanet => rsx! {
                            GeneanetTab { tree_id, on_imported }
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

/// Drop or pick a `.ged`/`.gw` and import it.
///
/// The one behavioural change from the menu item this replaces: the bytes come
/// from `read()` rather than from a path. A picked file has no path in a
/// browser at all, so the old `path()` + `tokio::fs::read` made this
/// desktop-only without anything saying so.
#[component]
fn FileTab(tree_id: Uuid, on_imported: EventHandler<ImportOutcome>) -> Element {
    let api = use_context::<ApiClient>();
    let i18n = use_i18n();

    let mut picked = use_signal(|| None::<(String, Vec<u8>)>);
    let mut dragging = use_signal(|| false);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut result = use_signal(|| None::<ImportResult>);

    let pick = move |_| {
        spawn(async move {
            let file = rfd::AsyncFileDialog::new()
                .add_filter("GEDCOM / GeneWeb", &["ged", "gw"])
                .add_filter("GEDCOM", &["ged"])
                .add_filter("GeneWeb", &["gw"])
                .add_filter("All files", &["*"])
                .set_title(i18n.t("gedcom.select_file"))
                .pick_file()
                .await;
            let Some(file) = file else { return };

            // `read()` and not `path()`: the one accessor that means the same
            // thing on the desktop build and in a browser.
            picked.set(Some((file.file_name(), file.read().await)));
            error.set(None);
            result.set(None);
        });
    };

    let api_import = api.clone();
    let do_import = move |_| {
        let api = api_import.clone();
        let Some((name, bytes)) = picked() else {
            return;
        };
        if busy() {
            return;
        }
        busy.set(true);
        error.set(None);
        spawn(async move {
            let outcome = if is_geneweb(&name) {
                api.import_geneweb(tree_id, bytes, &name).await
            } else {
                match String::from_utf8(bytes) {
                    Ok(gedcom) => api.import_gedcom(tree_id, &gedcom).await,
                    Err(_) => {
                        error.set(Some(i18n.t("import.not_utf8")));
                        busy.set(false);
                        return;
                    }
                }
            };
            busy.set(false);
            match outcome {
                Ok(imported) => {
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
                // Both handlers must cancel the default, or the engine
                // navigates to the dropped file and the app disappears.
                ondragover: move |e| {
                    e.prevent_default();
                    dragging.set(true);
                },
                ondragleave: move |_| dragging.set(false),
                ondrop: move |e: Event<DragData>| {
                    e.prevent_default();
                    dragging.set(false);
                    let dropped = e.files();
                    spawn(async move {
                        let Some(file) = dropped.into_iter().next() else { return };
                        if let Ok(bytes) = file.read_bytes().await {
                            picked.set(Some((short_name(&file.name()), bytes.to_vec())));
                            error.set(None);
                        }
                    });
                },
                onclick: pick,

                if let Some((name, bytes)) = picked() {
                    div { class: "import-drop-icon", "📄" }
                    div { class: "import-drop-name", "{name}" }
                    div { class: "import-drop-hint",
                        {i18n.t_args("import.file_size", &[("size", &human_size(bytes.len()))])}
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
fn GeneanetTab(tree_id: Uuid, on_imported: EventHandler<ImportOutcome>) -> Element {
    let api = use_context::<ApiClient>();
    let i18n = use_i18n();
    let bridge = use_geneanet_bridge();

    let mut open = use_signal(|| Step::Gw);

    let gw = use_signal(|| None::<GwFile>);
    let gw_error = use_signal(|| None::<String>);

    let archives = use_signal(Vec::<IndexedArchive>::new);
    let archives_skipped = use_signal(|| false);
    let archive_error = use_signal(|| None::<String>);

    let mut collected = use_signal(|| None::<Collected>);
    let mut connecting = use_signal(|| None::<Connecting>);
    let mut connect_error = use_signal(|| None::<String>);

    let mut preview = use_signal(|| None::<GeneanetPreview>);
    let mut preview_error = use_signal(|| None::<String>);
    let override_mismatch = use_signal(|| false);

    let mut importing = use_signal(|| false);
    let mut import_error = use_signal(|| None::<String>);
    let mut import_result = use_signal(|| None::<GeneanetImportResult>);

    // Step 2 is settled either by adding archives or by explicitly skipping;
    // both let the flow move on, and the difference is only whether photos get
    // downloaded.
    let archives_settled = move || !archives.read().is_empty() || archives_skipped();
    let can_connect = move || gw().is_some() && (archives_settled() || !NATIVE);
    let can_preview = move || collected().is_some();

    // ── Step 3 — drive the login window ──────────────────────────────
    let start_connect = {
        let bridge = bridge.clone();
        move |_| {
            let Some(bridge) = bridge.clone() else { return };
            let (tx, mut rx) = futures_channel::mpsc::unbounded::<GeneanetEvent>();
            connect_error.set(None);
            connecting.set(Some(Connecting::WaitingForLogin));
            bridge.start(tx);

            spawn(async move {
                use futures_util::StreamExt as _;
                while let Some(event) = rx.next().await {
                    match event {
                        GeneanetEvent::Opened => {
                            connecting.set(Some(Connecting::WaitingForLogin));
                        }
                        GeneanetEvent::SignedIn => {
                            connecting.set(Some(Connecting::Collecting { done: 0, total: 0 }));
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
                        } => {
                            collected.set(Some(Collected {
                                collection,
                                deposit_sizes,
                                cookie,
                                account,
                            }));
                            connecting.set(None);
                            open.set(Step::Preview);
                            break;
                        }
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
            });
        }
    };

    // ── Step 4 — compute the preview when it opens ───────────────────
    let api_preview = api.clone();
    let run_preview = move |_| {
        let api = api_preview.clone();
        let (Some(file), Some(collected)) = (gw(), collected()) else {
            return;
        };
        preview_error.set(None);
        spawn(async move {
            let body = GeneanetPreviewBody {
                gw_base64: encode_gw(&file.bytes),
                file_name: file.name.clone(),
                collection: collected.collection.clone(),
                deposit_sizes: collected.deposit_sizes.clone(),
                archive_paths: archives.read().iter().map(|a| a.path.clone()).collect(),
            };
            match api.preview_geneanet_import(&body).await {
                Ok(stats) => preview.set(Some(stats)),
                Err(e) => preview_error.set(Some(format!("{e}"))),
            }
        });
    };

    // ── Step 5 — write ───────────────────────────────────────────────
    let api_import = api.clone();
    let run_import = move |_| {
        let api = api_import.clone();
        let (Some(file), Some(collected)) = (gw(), collected()) else {
            return;
        };
        if importing() {
            return;
        }
        importing.set(true);
        import_error.set(None);
        spawn(async move {
            let body = GeneanetImportBody {
                gw_base64: encode_gw(&file.bytes),
                file_name: file.name.clone(),
                collection: collected.collection.clone(),
                deposit_sizes: collected.deposit_sizes.clone(),
                archive_paths: archives.read().iter().map(|a| a.path.clone()).collect(),
                cookie: collected.cookie.clone(),
            };
            let outcome = api.import_geneanet(tree_id, &body).await;
            importing.set(false);
            match outcome {
                Ok(result) => {
                    import_result.set(Some(result.clone()));
                    on_imported.call(ImportOutcome::Geneanet(result));
                }
                Err(e) => import_error.set(Some(format!("{e}"))),
            }
        });
    };

    if let Some(result) = import_result() {
        return rsx! { GeneanetDone { result } };
    }

    rsx! {
        div { class: "gn-steps",

            // ── Step 1 — the .gw file ────────────────────────────────
            StepShell {
                index: 1,
                title: i18n.t("geneanet.step1_title"),
                open: open() == Step::Gw,
                reachable: true,
                summary: gw().map(|f| i18n.t_args(
                    "geneanet.step1_summary",
                    &[("file", &f.name), ("count", &group_digits(f.inspection.person_count))],
                )),
                on_open: move |_| open.set(Step::Gw),
                GwStep {
                    gw,
                    gw_error,
                    on_settled: move |_| {
                        open.set(if NATIVE { Step::Archives } else { Step::Connect });
                    },
                }
            }

            // ── Step 2 — the photo archives ──────────────────────────
            StepShell {
                index: 2,
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

            // ── Step 3 — sign in and collect ─────────────────────────
            StepShell {
                index: 3,
                title: i18n.t("geneanet.step3_title"),
                open: open() == Step::Connect,
                reachable: can_connect(),
                summary: collected().map(|c| {
                    let photos = group_digits(c.deposit_sizes.len().max(1));
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
                    error: connect_error(),
                    on_start: start_connect,
                }
            }

            // ── Step 4 — what will be imported ───────────────────────
            StepShell {
                index: 4,
                title: i18n.t("geneanet.step4_title"),
                open: open() == Step::Preview,
                reachable: can_preview(),
                summary: None,
                on_open: move |_| open.set(Step::Preview),
                PreviewStep {
                    preview: preview(),
                    error: preview_error(),
                    override_mismatch,
                    on_compute: run_preview,
                    on_back: move |_| open.set(Step::Gw),
                    on_continue: move |_| open.set(Step::Import),
                }
            }

            // ── Step 5 — import ──────────────────────────────────────
            StepShell {
                index: 5,
                title: i18n.t("geneanet.step5_title"),
                open: open() == Step::Import,
                reachable: preview().is_some() && (!preview().unwrap_or_default().mismatch || override_mismatch()),
                summary: None,
                on_open: move |_| open.set(Step::Import),
                ImportStep {
                    preview: preview(),
                    importing: importing(),
                    error: import_error(),
                    on_start: run_import,
                }
            }
        }
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
    on_settled: EventHandler<()>,
) -> Element {
    let api = use_context::<ApiClient>();
    let i18n = use_i18n();
    let mut busy = use_signal(|| false);

    let mut gw = gw;
    let mut gw_error = gw_error;

    let pick = move |_| {
        let api = api.clone();
        spawn(async move {
            let file = rfd::AsyncFileDialog::new()
                .add_filter("GeneWeb", &["gw"])
                .add_filter("All files", &["*"])
                .set_title(i18n.t("geneanet.step1_pick"))
                .pick_file()
                .await;
            let Some(file) = file else { return };

            let name = file.file_name();
            let bytes = file.read().await;

            // Parsed on selection, not at import time. It costs nothing and it
            // is the first moment the user finds out whether they picked the
            // right export — a `.ged` fails here rather than four steps later.
            busy.set(true);
            gw_error.set(None);
            let inspected = api.inspect_geneweb(bytes.clone(), &name).await;
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
                paths.push(file.path().display().to_string());
            }

            busy.set(true);
            archive_error.set(None);
            let indexed = api.index_geneanet_archives(paths).await;
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

// ── Step 3 ──────────────────────────────────────────────────────────

#[component]
fn ConnectStep(
    available: bool,
    connecting: Option<Connecting>,
    error: Option<String>,
    on_start: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();

    if !available {
        return rsx! {
            p { class: "gn-lead", {i18n.t("geneanet.step3_lead")} }
            div { class: "gn-desktop-only",
                strong { {i18n.t("geneanet.desktop_only_title")} }
                p { {i18n.t("geneanet.step3_web")} }
            }
        };
    }

    rsx! {
        p { class: "gn-lead", {i18n.t("geneanet.step3_lead")} }
        p { class: "gn-note", {i18n.t("geneanet.step3_password")} }

        if let Some(err) = error {
            div { class: "error-msg", "{err}" }
        }

        match connecting {
            Some(Connecting::WaitingForLogin) => rsx! {
                div { class: "gn-progress-block",
                    div { class: "gn-progress-label", {i18n.t("geneanet.step3_waiting")} }
                }
            },
            Some(Connecting::Collecting { done, total }) => rsx! {
                ProgressBar {
                    label: i18n.t("geneanet.step3_stage1"),
                    done,
                    total,
                }
            },
            Some(Connecting::Sizing { done, total }) => rsx! {
                ProgressBar {
                    label: i18n.t("geneanet.step3_stage2"),
                    done,
                    total,
                }
            },
            None => rsx! {
                div { class: "modal-actions",
                    button {
                        class: "btn btn-primary",
                        onclick: move |_| on_start.call(()),
                        {i18n.t("geneanet.step3_open")}
                    }
                }
            },
        }
    }
}

/// A labelled bar with a live region beside it.
///
/// The live region announces the *stage*, not every tick: a bar that spoke on
/// each of 614 photos would be unusable with a screen reader.
#[component]
fn ProgressBar(label: String, done: usize, total: usize) -> Element {
    let pct = (done * 100).checked_div(total).unwrap_or(0).min(100);

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
            div { class: "gn-progress-count",
                if total > 0 { "{done} / {total}" } else { "{done}" }
            }
        }
    }
}

// ── Step 4 ──────────────────────────────────────────────────────────

#[component]
fn PreviewStep(
    preview: Option<GeneanetPreview>,
    error: Option<String>,
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
            if stats.to_download == 0 && stats.in_archives > 0 {
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

        if stats.mismatch && !override_mismatch() {
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
            div { class: "gn-progress-block",
                div { class: "gn-progress-label", "aria-live": "polite",
                    {i18n.t("geneanet.step5_running")}
                }
                div { class: "gn-progress",
                    div { class: "gn-progress-fill is-indeterminate" }
                }
                div { class: "gn-progress-count", {i18n.t("geneanet.step5_dont_close")} }
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

/// Whether a picked file should go through the GeneWeb reader.
fn is_geneweb(file_name: &str) -> bool {
    file_name
        .rsplit('.')
        .next()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("gw"))
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
    fn only_a_gw_extension_takes_the_geneweb_path() {
        assert!(is_geneweb("tree.gw"));
        assert!(is_geneweb("MYACCOUNT_2026-08-01.GW"));
        assert!(!is_geneweb("tree.ged"));
        assert!(!is_geneweb("tree"));
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
