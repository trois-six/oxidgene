//! The upload cell that ends every media gallery.
//!
//! One control, two ways in: click it to open the platform's file dialog, or
//! drop files onto it. Both land in the same [`upload_files`] loop, so a photo
//! dropped from a file manager and one picked from the dialog are the same
//! request.
//!
//! Uploads run one at a time rather than concurrently. A user selecting a
//! folder of scans is sending tens of megabytes over a connection they do not
//! control, and firing them all at once turns "3 of 12" — which reads as
//! progress — into twelve stalled requests that finish in an unpredictable
//! order.

use dioxus::html::HasFileData;
use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::{ApiClient, MediaUpload};
use crate::i18n::use_i18n;

/// How far along a batch of uploads is.
#[derive(Debug, Clone, PartialEq)]
pub struct UploadProgress {
    pub done: usize,
    pub total: usize,
    /// The file currently going up.
    pub current: String,
}

#[derive(Props, Clone, PartialEq)]
pub struct MediaInputProps {
    pub tree_id: Uuid,
    /// When set, each uploaded file is appended as a page of this document
    /// rather than becoming a media of its own.
    #[props(default)]
    pub document_id: Option<Uuid>,
    /// Label shown on the cell. Defaults to the ordinary "Upload".
    #[props(default)]
    pub label: Option<String>,
    /// Called once per successfully uploaded file, with the new media's id.
    ///
    /// Per file rather than per batch so the caller can link and show each
    /// tile as it lands, instead of a gallery that stays empty until the
    /// slowest file finishes.
    pub on_uploaded: EventHandler<Uuid>,
    /// Called when the whole batch is done, successfully or not.
    #[props(default)]
    pub on_batch_done: Option<EventHandler<()>>,
}

/// The "+ Upload" cell: file picker, drag target, and progress readout.
#[component]
pub fn MediaInput(props: MediaInputProps) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let progress = use_signal(|| None::<UploadProgress>);
    let error = use_signal(|| None::<String>);
    let mut dragging = use_signal(|| false);

    let tree_id = props.tree_id;
    let document_id = props.document_id;
    let label = props
        .label
        .clone()
        .unwrap_or_else(|| i18n.t("media.upload"));
    let on_uploaded = props.on_uploaded;
    let on_batch_done = props.on_batch_done;

    let pick_files = {
        let api = api.clone();
        move |_| {
            let api = api.clone();
            spawn(async move {
                let files = rfd::AsyncFileDialog::new()
                    .add_filter(
                        "Images & documents",
                        &[
                            "jpg", "jpeg", "png", "gif", "bmp", "tif", "tiff", "webp", "pdf",
                        ],
                    )
                    .add_filter("Images", &["jpg", "jpeg", "png", "gif", "bmp", "webp"])
                    .add_filter("Documents", &["pdf", "tif", "tiff"])
                    .add_filter("All files", &["*"])
                    .set_title(i18n.t("media.select_files"))
                    .pick_files()
                    .await;
                let Some(files) = files else { return };

                // `read()` rather than a path: it is the one accessor that works
                // the same on the desktop build and in a browser, where the picked
                // file has no path at all.
                let mut payloads = Vec::with_capacity(files.len());
                for file in files {
                    payloads.push((file.file_name(), file.read().await));
                }

                upload_files(
                    tree_id,
                    document_id,
                    payloads,
                    progress,
                    error,
                    api,
                    on_uploaded,
                    on_batch_done,
                    &i18n,
                )
                .await;
            });
        }
    };

    let drop_api = api.clone();
    let busy = progress().is_some();

    rsx! {
        div {
            class: if dragging() { "media-drop is-dragging" } else { "media-drop" },
            // Both handlers must cancel the default, or the engine navigates
            // away to the dropped file and the whole app disappears.
            ondragover: move |e| {
                e.prevent_default();
                dragging.set(true);
            },
            ondragleave: move |_| dragging.set(false),
            ondrop: move |e: Event<DragData>| {
                e.prevent_default();
                dragging.set(false);
                let dropped = e.files();
                let api = drop_api.clone();
                spawn(async move {
                    let mut payloads = Vec::new();
                    for file in dropped {
                        // A file the engine cannot read is skipped rather than
                        // failing the drop: dragging a selection that includes
                        // a directory is an ordinary mistake, not an error.
                        if let Ok(bytes) = file.read_bytes().await {
                            payloads.push((short_name(&file.name()), bytes.to_vec()));
                        }
                    }
                    if payloads.is_empty() {
                        return;
                    }
                    upload_files(
                        tree_id,
                        document_id,
                        payloads,
                        progress,
                        error,
                        api,
                        on_uploaded,
                        on_batch_done,
                        &i18n,
                    )
                    .await;
                });
            },
            button {
                class: "media-drop-btn",
                r#type: "button",
                disabled: busy,
                onclick: pick_files,
                if let Some(p) = progress() {
                    span { class: "media-drop-icon", "\u{2191}" }
                    span { class: "media-drop-label",
                        {i18n.t_args(
                            "media.uploading_n_of_m",
                            &[("done", &(p.done + 1).to_string()), ("total", &p.total.to_string())],
                        )}
                    }
                    span { class: "media-drop-hint", "{p.current}" }
                } else {
                    span { class: "media-drop-icon", "+" }
                    span { class: "media-drop-label", "{label}" }
                    span { class: "media-drop-hint", {i18n.t("media.drop_hint")} }
                }
            }
        }
        if let Some(err) = error() {
            div { class: "error-msg", "{err}" }
        }
    }
}

/// The last path component of an engine-reported file name.
///
/// A drop can report a full path on some platforms; the server sanitizes it
/// too, but showing the user the whole path in a progress line is noise.
fn short_name(raw: &str) -> String {
    raw.rsplit(['/', '\\']).next().unwrap_or(raw).to_string()
}

/// Upload a batch sequentially, reporting each file as it lands.
///
/// One failure does not abandon the rest: a folder of scans where the third
/// file is a `.DS_Store` should still deliver the other eleven. The message
/// names the file, so "unsupported file type" is actionable.
#[allow(clippy::too_many_arguments)]
async fn upload_files(
    tree_id: Uuid,
    document_id: Option<Uuid>,
    files: Vec<(String, Vec<u8>)>,
    mut progress: Signal<Option<UploadProgress>>,
    mut error: Signal<Option<String>>,
    api: ApiClient,
    on_uploaded: EventHandler<Uuid>,
    on_batch_done: Option<EventHandler<()>>,
    i18n: &crate::i18n::I18n,
) {
    let total = files.len();
    let mut failures: Vec<String> = Vec::new();
    error.set(None);

    for (index, (file_name, bytes)) in files.into_iter().enumerate() {
        progress.set(Some(UploadProgress {
            done: index,
            total,
            current: file_name.clone(),
        }));
        match api
            .upload_media(
                tree_id,
                MediaUpload {
                    file_name: file_name.clone(),
                    bytes,
                    title: None,
                    description: None,
                    attach_to: None,
                    as_page_of: document_id,
                },
            )
            .await
        {
            Ok(media) => on_uploaded.call(media.id),
            Err(e) => failures.push(format!("{file_name}: {}", friendly(&e, i18n))),
        }
    }

    progress.set(None);
    if !failures.is_empty() {
        error.set(Some(failures.join("\n")));
    }
    if let Some(done) = on_batch_done {
        done.call(());
    }
}

/// Turn an API error into something worth showing a user.
///
/// The upload endpoint answers `400` with a message that already says what is
/// wrong ("unsupported file type; accepted types are …"), so a rejected file
/// is worth quoting; anything else is plumbing and reads better generically.
fn friendly(err: &crate::api::ApiError, i18n: &crate::i18n::I18n) -> String {
    match err {
        crate::api::ApiError::Api { status: 400, body } => {
            serde_json::from_str::<serde_json::Value>(body)
                .ok()
                .and_then(|v| v["message"].as_str().map(str::to_string))
                .unwrap_or_else(|| body.clone())
        }
        crate::api::ApiError::Api { status: 413, .. } => i18n.t("media.error_too_large"),
        other => other.to_string(),
    }
}
