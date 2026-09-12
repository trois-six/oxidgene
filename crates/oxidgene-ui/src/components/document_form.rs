//! The one way to add a document to a person, a couple, or an event.
//!
//! # Why one form and not two
//!
//! A single-page photograph and a forty-page notarial act are the same record
//! here: a document row that describes the thing, and page rows that hold the
//! files. There is therefore nothing for a second "quick upload" entry point to
//! do differently — it would only be this form with most of its fields hidden,
//! which is how a photograph ends up with no date, no place and no kind while
//! the register beside it has all three.
//!
//! # Why nothing is written until Save
//!
//! The form it replaces created the document, linked it to the person, and
//! *then* asked what it was. Closing the modal at that point left an empty
//! document attached to somebody, and nothing ever cleaned it up. Here the
//! chosen files and typed addresses are held in the component until the user
//! commits, so Cancel makes no request at all. The cost is that the bytes sit
//! in memory until then, and that the progress readout starts at Save rather
//! than at the moment a file is picked.
//!
//! A save that fails part way deletes the document it had started, so the
//! gallery never shows a half-written record. Purging a document takes its
//! pages, tags, notes and links with it, so one call undoes all of it.

use dioxus::prelude::*;
use oxidgene_core::enums::{DocumentCategory, SourceMediaType};
use uuid::Uuid;

use crate::api::{
    ApiClient, CreateMediaBody, CreateMediaLinkBody, CreateNoteBody, MediaKind, MediaUpload,
    UpdateMediaBody,
};
use crate::components::date_input::{DateInput, DateParts};
use crate::components::media_gallery::{MediaOwner, MediaTagForm};
use crate::components::media_input::{MediaInput, PickedFile, friendly};
use crate::components::person_form::render_place_select;
use crate::i18n::use_i18n;
use crate::ui_observability::use_ui_resource;
use crate::utils::parse_privacy;

/// A page the user has chosen but that has not been written yet.
#[derive(Clone, PartialEq)]
enum PendingPage {
    /// Bytes read from the picker or a drop, waiting for a document to belong
    /// to.
    File { name: String, bytes: Vec<u8> },
    /// An address somebody else serves. Nothing is fetched, then or later.
    Remote { url: String, file_name: String },
}

impl PendingPage {
    fn label(&self) -> &str {
        match self {
            Self::File { name, .. } => name,
            Self::Remote { url, .. } => url,
        }
    }

    /// The address to preview from, when the page is a picture we can point an
    /// `<img>` at without holding it.
    fn preview_url(&self) -> Option<&str> {
        match self {
            Self::File { .. } => None,
            Self::Remote { url, file_name } => {
                let mime = oxidgene_core::types::normalize_mime(None, file_name);
                (crate::api::media_kind(&mime) == MediaKind::Image).then_some(url.as_str())
            }
        }
    }
}

/// Which page the address field is currently standing in for.
#[derive(Clone, Copy, PartialEq)]
enum UrlEdit {
    /// A page that does not exist yet, added by the cell at the end of the
    /// grid.
    New,
    /// The address of the page already at this position, being corrected.
    Existing(usize),
}

/// One page as the grid needs it.
///
/// Built without the bytes: the list holds whole files, and cloning it to draw
/// a row of names would copy every chosen megabyte on every render.
struct PageRow {
    index: usize,
    label: String,
    preview: Option<String>,
    /// Only an address can be retyped. A file's name follows its bytes.
    remote: bool,
}

/// The last path segment of a URL, which is the closest thing it has to a file
/// name.
///
/// Query strings and fragments are dropped: `scan.jpg?size=full` is a JPEG, and
/// keeping the query would make the extension unreadable to the MIME guess the
/// server runs on this value.
///
/// The host is dropped first, and not by trimming the last segment: a bare
/// `https://archives.example.org` has no path, and its host ends in what looks
/// exactly like an extension.
fn url_file_name(url: &str) -> String {
    let without_query = url.split(['?', '#']).next().unwrap_or(url);
    let path = match without_query.split_once("://") {
        // Behind a scheme the first segment is the host, and a host is not a
        // file name however much `.org` looks like an extension.
        Some((_, rest)) => rest.split_once('/').map_or("", |(_, path)| path),
        None => without_query,
    };
    let name = path
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("")
        .trim();
    if name.is_empty() || !name.contains('.') {
        "document".to_string()
    } else {
        name.to_string()
    }
}

#[derive(Props, Clone, PartialEq)]
pub struct DocumentFormProps {
    pub tree_id: Uuid,
    /// What the finished document is attached to.
    pub owner: MediaOwner,
    /// Events this document may be offered as evidence for, as (id, label).
    #[props(default)]
    pub events: Vec<(Uuid, String)>,
    /// Fired after the document and everything in it has been written.
    pub on_created: EventHandler<()>,
    pub on_close: EventHandler<()>,
}

/// The full document form, with its pages, over a modal backdrop.
#[component]
pub fn DocumentForm(props: DocumentFormProps) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();

    let tree_id = props.tree_id;
    let owner = props.owner;
    let events = props.events.clone();
    let on_created = props.on_created;
    let on_close = props.on_close;

    let mut pages = use_signal(Vec::<PendingPage>::new);
    let mut url_draft = use_signal(String::new);
    let mut editing_url = use_signal(|| None::<UrlEdit>);
    let mut title = use_signal(String::new);
    let mut description = use_signal(String::new);
    let mut tags = use_signal(Vec::<String>::new);
    let mut show_tag_form = use_signal(|| false);
    let mut document_category = use_signal(|| None::<DocumentCategory>);
    let mut source_media_type = use_signal(SourceMediaType::default);
    let mut privacy_value = use_signal(|| "Default".to_string());
    let date_parts = use_signal(DateParts::default);
    let place_id = use_signal(String::new);
    let mut note_text = use_signal(String::new);
    let mut selected_events = use_signal(Vec::<Uuid>::new);
    let mut saving = use_signal(|| false);
    let mut progress = use_signal(|| None::<(usize, usize)>);
    let mut error = use_signal(|| None::<String>);

    let places = use_ui_resource("document_form_places", {
        let api = api.clone();
        move || {
            let api = api.clone();
            async move { api.list_all_places(tree_id).await }
        }
    });
    let place_options: Vec<(String, String)> = match &*places.read_unchecked() {
        Some(Ok(places)) => places
            .iter()
            .map(|p| (p.id.to_string(), p.name.clone()))
            .collect(),
        _ => Vec::new(),
    };

    // One handler for both "add an address" and "correct that address": the
    // difference is a position, and a typo in a URL is found after it has been
    // added far more often than while it is being typed.
    let commit_url = use_callback(move |()| {
        let url = url_draft().trim().to_string();
        if url.is_empty() {
            return;
        }
        let file_name = url_file_name(&url);
        let page = PendingPage::Remote { url, file_name };
        match editing_url() {
            Some(UrlEdit::Existing(index)) => {
                if let Some(slot) = pages.write().get_mut(index) {
                    *slot = page;
                }
            }
            // Appended, not inserted: the address is the page the user has
            // just described, and it belongs after the ones already listed.
            Some(UrlEdit::New) | None => pages.write().push(page),
        }
        url_draft.set(String::new());
        editing_url.set(None);
    });

    let save = {
        let api = api.clone();
        move |_| {
            let api = api.clone();
            let title_value = title().trim().to_string();
            let description_value = description().trim().to_string();
            let note_value = note_text().trim().to_string();
            let place_value = Uuid::parse_str(place_id().trim()).ok();
            let privacy = parse_privacy(&privacy_value());
            let medium = source_media_type();
            let category = document_category();
            let resolved = date_parts().resolved();
            let tag_list = tags();
            let event_ids = selected_events();
            spawn(async move {
                saving.set(true);
                error.set(None);

                // Step one, and the only one that can fail without leaving
                // anything behind.
                let document = match api
                    .create_media_document(
                        tree_id,
                        (!title_value.is_empty()).then_some(title_value.as_str()),
                    )
                    .await
                {
                    Ok(document) => document,
                    Err(err) => {
                        error.set(Some(err.to_string()));
                        saving.set(false);
                        return;
                    }
                };

                // From here on a failure has something to undo. `write` returns
                // the message to show; the document is purged either way, so
                // the user sees the form they were filling in rather than a
                // half-written record in the gallery behind it.
                let outcome = write_document(
                    &api,
                    tree_id,
                    document.id,
                    owner,
                    WriteRequest {
                        pages,
                        progress,
                        description: description_value,
                        note: note_value,
                        place_id: place_value,
                        privacy,
                        medium,
                        category,
                        date: resolved,
                        tags: tag_list,
                        event_ids,
                    },
                    &i18n,
                )
                .await;

                progress.set(None);
                match outcome {
                    Ok(()) => {
                        saving.set(false);
                        on_created.call(());
                        on_close.call(());
                    }
                    Err(message) => {
                        // Best effort: if the rollback itself fails the
                        // original error is still the one worth reporting.
                        let _ = api.delete_media(tree_id, document.id).await;
                        error.set(Some(message));
                        saving.set(false);
                    }
                }
            });
        }
    };

    let rows: Vec<PageRow> = pages
        .read()
        .iter()
        .enumerate()
        .map(|(index, page)| PageRow {
            index,
            label: page.label().to_string(),
            preview: page.preview_url().map(str::to_string),
            remote: matches!(page, PendingPage::Remote { .. }),
        })
        .collect();
    let total = rows.len();
    let busy = saving();
    let url_open = editing_url().is_some();

    rsx! {
        div {
            class: "cropper-backdrop",
            onmousedown: move |event| event.stop_propagation(),
            onclick: move |_| if !busy { on_close.call(()) },
            div { class: "media-manager-modal", onclick: move |event| event.stop_propagation(),
                div { class: "cropper-head",
                    span { class: "cropper-title", {i18n.t("media.new_document")} }
                    button {
                        class: "cropper-close",
                        r#type: "button",
                        title: i18n.t("common.close"),
                        disabled: busy,
                        onclick: move |_| on_close.call(()),
                        "\u{00D7}"
                    }
                }
                div { class: "media-manager-body",
                    div { class: "media-panel is-embedded",

                        div { class: "form-group",
                            label { {i18n.t("media.title")} }
                            input {
                                r#type: "text",
                                value: "{title}",
                                disabled: busy,
                                oninput: move |e: Event<FormData>| title.set(e.value()),
                            }
                        }
                        div { class: "form-group",
                            label { {i18n.t("media.description")} }
                            textarea {
                                rows: 3,
                                value: "{description}",
                                disabled: busy,
                                oninput: move |e: Event<FormData>| description.set(e.value()),
                            }
                        }

                        div { class: "pf-subblock media-tags-editor",
                            div { class: "pf-block-label",
                                button {
                                    class: if show_tag_form() { "pf-add-btn is-open" } else { "pf-add-btn" },
                                    r#type: "button",
                                    disabled: busy,
                                    onclick: move |_| {
                                        let opening = !show_tag_form();
                                        show_tag_form.set(opening);
                                    },
                                    {i18n.t("media.add_tag")}
                                }
                            }
                            if show_tag_form() {
                                MediaTagForm {
                                    on_add: move |value: String| {
                                        if value.is_empty()
                                            || tags().iter().any(|tag| tag.eq_ignore_ascii_case(&value))
                                        {
                                            return;
                                        }
                                        show_tag_form.set(false);
                                        tags.write().push(value);
                                    },
                                }
                            }
                            if !tags().is_empty() {
                                div { class: "media-fact-tags media-edit-tags",
                                    for (index , tag) in tags().iter().enumerate() {
                                        span { key: "{tag}", class: "media-fact-tag is-editable",
                                            "{tag}"
                                            button {
                                                class: "media-tag-remove",
                                                r#type: "button",
                                                title: i18n.t("common.delete"),
                                                onclick: move |_| { tags.write().remove(index); },
                                                "\u{00D7}"
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        div { class: "form-group",
                            label { {i18n.t("media.document_category")} }
                            select {
                                class: "td-select",
                                disabled: busy,
                                onchange: move |e: Event<FormData>| {
                                    document_category.set(DocumentCategory::parse(&e.value()));
                                },
                                option {
                                    value: "",
                                    selected: document_category().is_none(),
                                    {i18n.t("media.category_none")}
                                }
                                for category in DocumentCategory::all() {
                                    option {
                                        key: "{category.as_str()}",
                                        value: "{category.as_str()}",
                                        selected: document_category() == Some(*category),
                                        {i18n.t(&format!("media.category.{}", category.as_str()))}
                                    }
                                }
                            }
                            p { class: "pf-ns-hint", {i18n.t("media.document_category_hint")} }
                        }
                        div { class: "form-group",
                            label { {i18n.t("media.source_media_type")} }
                            select {
                                class: "td-select",
                                disabled: busy,
                                onchange: move |e: Event<FormData>| {
                                    source_media_type
                                        .set(SourceMediaType::parse(&e.value()).unwrap_or_default());
                                },
                                for medium in SourceMediaType::all() {
                                    option {
                                        key: "{medium.as_str()}",
                                        value: "{medium.as_str()}",
                                        selected: source_media_type() == *medium,
                                        {i18n.t(&format!("media.medium.{}", medium.as_str()))}
                                    }
                                }
                            }
                            p { class: "pf-ns-hint", {i18n.t("media.source_media_type_hint")} }
                        }
                        div { class: "form-group",
                            label { {i18n.t("media.privacy")} }
                            select {
                                class: "td-select",
                                disabled: busy,
                                onchange: move |e: Event<FormData>| privacy_value.set(e.value()),
                                for (value , label) in [
                                    ("Default", i18n.t("privacy.default")),
                                    ("Public", i18n.t("privacy.public")),
                                    ("Private", i18n.t("privacy.private")),
                                ] {
                                    option {
                                        key: "{value}",
                                        value: "{value}",
                                        selected: privacy_value() == value,
                                        "{label}"
                                    }
                                }
                            }
                            p { class: "pf-ns-hint", {i18n.t("privacy.not_enforced_yet")} }
                        }

                        div { class: "form-group",
                            label { {i18n.t("media.date")} }
                            DateInput { parts: date_parts, i18n, on_change: move |()| {} }
                        }
                        {render_place_select(&i18n, place_id, &place_options, || {})}

                        div { class: "form-group",
                            label { {i18n.t("media.note")} }
                            textarea {
                                rows: 3,
                                value: "{note_text}",
                                placeholder: i18n.t("media.note_placeholder"),
                                disabled: busy,
                                oninput: move |e: Event<FormData>| note_text.set(e.value()),
                            }
                        }

                        if !events.is_empty() {
                            div { class: "media-panel-section",
                                label { {i18n.t("media.documents_events")} }
                                div { class: "media-events",
                                    for (id , label) in events.iter() {
                                        {
                                            let event_id = *id;
                                            let checked = selected_events().contains(&event_id);
                                            rsx! {
                                                label { key: "{event_id}", class: "media-event-row",
                                                    input {
                                                        r#type: "checkbox",
                                                        checked,
                                                        disabled: busy,
                                                        onchange: move |e: Event<FormData>| {
                                                            let mut list = selected_events.write();
                                                            if e.checked() {
                                                                if !list.contains(&event_id) {
                                                                    list.push(event_id);
                                                                }
                                                            } else {
                                                                list.retain(|id| *id != event_id);
                                                            }
                                                        },
                                                    }
                                                    span { "{label}" }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // Pages last, immediately above Save. The fields above
                        // describe the document; this is the document itself,
                        // and it is the last thing the user assembles before
                        // committing.
                        div { class: "media-panel-section",
                            label { {i18n.t("media.pages")} }
                            p { class: "pf-ns-hint", {i18n.t("media.new_document_pages_hint")} }
                            div { class: "doc-pages",
                                for row in rows {
                                    {
                                        let PageRow { index, label, preview, remote } = row;
                                        let address = label.clone();
                                        rsx! {
                                            div { key: "{index}-{label}", class: "doc-page",
                                                span { class: "doc-page-number", "{index + 1}" }
                                                div { class: "doc-page-thumb",
                                                    if let Some(preview) = preview {
                                                        img { src: "{preview}", alt: "{label}", loading: "lazy" }
                                                    } else if remote {
                                                        span { class: "media-glyph", "\u{1F517}" }
                                                    } else {
                                                        span { class: "media-glyph", "\u{1F4C4}" }
                                                    }
                                                }
                                                span { class: "doc-page-name", title: "{label}", "{label}" }
                                                div { class: "doc-page-actions",
                                                    button {
                                                        class: "pf-row-btn",
                                                        r#type: "button",
                                                        disabled: index == 0 || busy,
                                                        title: i18n.t("media.page_move_up"),
                                                        onclick: move |_| pages.write().swap(index, index - 1),
                                                        "\u{2191}"
                                                    }
                                                    button {
                                                        class: "pf-row-btn",
                                                        r#type: "button",
                                                        disabled: index + 1 >= total || busy,
                                                        title: i18n.t("media.page_move_down"),
                                                        onclick: move |_| pages.write().swap(index, index + 1),
                                                        "\u{2193}"
                                                    }
                                                    // Only an address can be retyped, so
                                                    // only an address offers the pencil.
                                                    if remote {
                                                        button {
                                                            class: "pf-row-btn",
                                                            r#type: "button",
                                                            disabled: busy,
                                                            title: i18n.t("media.edit_url"),
                                                            onclick: move |_| {
                                                                url_draft.set(address.clone());
                                                                editing_url.set(Some(UrlEdit::Existing(index)));
                                                            },
                                                            "\u{270E}"
                                                        }
                                                    }
                                                    button {
                                                        class: "pf-row-btn is-danger",
                                                        r#type: "button",
                                                        disabled: busy,
                                                        title: i18n.t("media.page_remove"),
                                                        onclick: move |_| {
                                                            pages.write().remove(index);
                                                            // The position the field
                                                            // was standing in for has
                                                            // moved.
                                                            editing_url.set(None);
                                                        },
                                                        "\u{2715}"
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }

                                // The canonical upload cell, told to hand the
                                // bytes over instead of sending them.
                                MediaInput {
                                    tree_id,
                                    label: i18n.t("media.add_pages"),
                                    on_files: move |files: Vec<PickedFile>| {
                                        let mut list = pages.write();
                                        for (name, bytes) in files {
                                            list.push(PendingPage::File { name, bytes });
                                        }
                                    },
                                }

                                // The other half of "a page is a file or an
                                // address", drawn as the same kind of cell: a
                                // page somebody else serves is a page, and
                                // adding one is the same gesture as adding a
                                // file, not a different control in a different
                                // place.
                                div { class: if url_open { "media-drop is-open" } else { "media-drop" },
                                    button {
                                        class: "media-drop-btn",
                                        r#type: "button",
                                        disabled: busy,
                                        title: i18n.t("media.link_url"),
                                        onclick: move |_| {
                                            url_draft.set(String::new());
                                            editing_url.set(Some(UrlEdit::New));
                                        },
                                        span { class: "media-drop-icon", "\u{1F517}" }
                                        span { class: "media-drop-label", {i18n.t("media.link_url")} }
                                        span { class: "media-drop-hint", {i18n.t("media.link_url_hint")} }
                                    }
                                }
                            }

                            if url_open {
                                form {
                                    class: "doc-page-url",
                                    onsubmit: move |event: Event<FormData>| {
                                        event.prevent_default();
                                        commit_url.call(());
                                    },
                                    input {
                                        r#type: "text",
                                        value: "{url_draft}",
                                        placeholder: "https://\u{2026}",
                                        autocomplete: "off",
                                        spellcheck: "false",
                                        disabled: busy,
                                        oninput: move |e: Event<FormData>| url_draft.set(e.value()),
                                    }
                                    button {
                                        class: "pf-confirm-btn btn-sm",
                                        r#type: "submit",
                                        disabled: busy || url_draft().trim().is_empty(),
                                        {i18n.t("common.save")}
                                    }
                                    button {
                                        class: "btn btn-outline btn-sm",
                                        r#type: "button",
                                        disabled: busy,
                                        onclick: move |_| {
                                            url_draft.set(String::new());
                                            editing_url.set(None);
                                        },
                                        {i18n.t("common.cancel")}
                                    }
                                }
                                p { class: "pf-ns-hint", {i18n.t("media.url_hint")} }
                            }
                        }

                        if let Some(err) = error() {
                            div { class: "error-msg", "{err}" }
                        }

                        div { class: "media-panel-actions",
                            if let Some((done, count)) = progress() {
                                span { class: "pf-ns-hint",
                                    {i18n.t_args(
                                        "media.uploading_n_of_m",
                                        &[("done", &done.to_string()), ("total", &count.to_string())],
                                    )}
                                }
                            }
                            button {
                                class: "btn btn-outline",
                                r#type: "button",
                                disabled: busy,
                                onclick: move |_| on_close.call(()),
                                {i18n.t("common.cancel")}
                            }
                            button {
                                class: "pf-confirm-btn",
                                r#type: "button",
                                // A document with no page is the thing this
                                // form exists to stop being creatable.
                                disabled: busy || total == 0,
                                onclick: save,
                                if busy { {i18n.t("common.saving")} } else { {i18n.t("common.save")} }
                            }
                        }
                        if total == 0 {
                            p { class: "pf-ns-hint", {i18n.t("media.new_document_needs_a_page")} }
                        }
                    }
                }
            }
        }
    }
}

/// Everything the save needs that is not the document's own identity.
///
/// A struct because the alternative is a twelve-argument function whose call
/// site is an unlabelled column of values.
struct WriteRequest {
    pages: Signal<Vec<PendingPage>>,
    progress: Signal<Option<(usize, usize)>>,
    description: String,
    note: String,
    place_id: Option<Uuid>,
    privacy: oxidgene_core::enums::Privacy,
    medium: SourceMediaType,
    category: Option<DocumentCategory>,
    date: DateParts,
    tags: Vec<String>,
    event_ids: Vec<Uuid>,
}

/// Fill in a freshly created document, returning the message to show on
/// failure.
///
/// Every step is fatal here, unlike an ordinary batch upload where one bad file
/// among twelve should not lose the other eleven: this document does not exist
/// yet as far as the user is concerned, so a partial result is a record they
/// did not ask for rather than progress they can keep.
async fn write_document(
    api: &ApiClient,
    tree_id: Uuid,
    document_id: Uuid,
    owner: MediaOwner,
    request: WriteRequest,
    i18n: &crate::i18n::I18n,
) -> Result<(), String> {
    let WriteRequest {
        pages,
        mut progress,
        description,
        note,
        place_id,
        privacy,
        medium,
        category,
        date,
        tags,
        event_ids,
    } = request;

    let total = pages.peek().len();
    for index in 0..total {
        // One page cloned at a time. Cloning the whole list would double the
        // memory the form is already holding, and taking it would lose the
        // user's files if a later step failed.
        let Some(page) = pages.peek().get(index).cloned() else {
            break;
        };
        progress.set(Some((index + 1, total)));
        let outcome = match page {
            PendingPage::File { name, bytes } => api
                .upload_media(
                    tree_id,
                    MediaUpload {
                        file_name: name.clone(),
                        bytes,
                        title: None,
                        description: None,
                        attach_to: None,
                        as_page_of: Some(document_id),
                    },
                )
                .await
                .map(|_| ())
                .map_err(|err| format!("{name}: {}", friendly(&err, i18n))),
            PendingPage::Remote { url, file_name } => api
                .create_media(
                    tree_id,
                    &CreateMediaBody {
                        document_id,
                        file_name,
                        // Left empty on purpose: the server guesses from the
                        // address, which is the only evidence there is for a
                        // file nobody is going to fetch.
                        mime_type: String::new(),
                        file_path: url.clone(),
                        file_size: 0,
                        title: None,
                        description: None,
                    },
                )
                .await
                .map(|_| ())
                .map_err(|err| format!("{url}: {}", friendly(&err, i18n))),
        };
        outcome?;
    }

    api.update_media(
        tree_id,
        document_id,
        &UpdateMediaBody {
            description: Some((!description.is_empty()).then_some(description)),
            date_value: Some(date.date_value()),
            date_value2: Some(date.date_value2()),
            date_qualifier: Some(date.qualifier),
            calendar: Some(date.calendar),
            place_id: Some(place_id),
            privacy: Some(privacy),
            source_media_type: Some(medium),
            document_category: Some(category),
            ..UpdateMediaBody::default()
        },
    )
    .await
    .map_err(|err| err.to_string())?;

    for tag in tags {
        api.add_media_tag(tree_id, document_id, tag)
            .await
            .map_err(|err| err.to_string())?;
    }

    if !note.is_empty() {
        api.create_note(
            tree_id,
            &CreateNoteBody {
                text: note,
                person_id: None,
                event_id: None,
                family_id: None,
                source_id: None,
                media_id: Some(document_id),
            },
        )
        .await
        .map_err(|err| err.to_string())?;
    }

    // Linked last: until this runs the document belongs to nobody, which is
    // exactly what a rollback wants to be true.
    api.create_media_link(
        tree_id,
        &CreateMediaLinkBody {
            media_id: document_id,
            person_id: matches!(owner, MediaOwner::Person(_)).then(|| owner.id()),
            family_id: matches!(owner, MediaOwner::Family(_)).then(|| owner.id()),
            event_id: matches!(owner, MediaOwner::Event(_)).then(|| owner.id()),
            source_id: None,
            sort_order: 0,
        },
    )
    .await
    .map_err(|err| err.to_string())?;

    for event_id in event_ids {
        if matches!(owner, MediaOwner::Event(id) if id == event_id) {
            continue;
        }
        api.create_media_link(
            tree_id,
            &CreateMediaLinkBody {
                media_id: document_id,
                person_id: None,
                family_id: None,
                event_id: Some(event_id),
                source_id: None,
                sort_order: 0,
            },
        )
        .await
        .map_err(|err| err.to_string())?;
    }

    Ok::<(), String>(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_url_names_its_last_segment() {
        assert_eq!(
            url_file_name("https://archives.example.org/folio/3.jpg"),
            "3.jpg"
        );
    }

    #[test]
    fn a_query_string_does_not_hide_the_extension() {
        assert_eq!(
            url_file_name("https://archives.example.org/view.jpg?size=full&page=2"),
            "view.jpg"
        );
    }

    #[test]
    fn an_address_with_no_file_still_names_something() {
        assert_eq!(
            url_file_name("https://archives.example.org/folio/"),
            "document"
        );
        // The host is not a file name, however much `.org` looks like one.
        assert_eq!(url_file_name("https://archives.example.org"), "document");
        assert_eq!(url_file_name("https://archives.example.org/"), "document");
    }

    /// A `.gw` or GEDCOM import can leave a bare path behind, and somebody
    /// pasting one in is asking for the same thing as a full address.
    #[test]
    fn an_address_with_no_scheme_still_finds_its_file() {
        assert_eq!(url_file_name("/archives/1872/folio-3.jpg"), "folio-3.jpg");
        assert_eq!(url_file_name("folio-3.jpg"), "folio-3.jpg");
    }

    #[test]
    fn only_a_remote_picture_offers_a_preview() {
        let remote = PendingPage::Remote {
            url: "https://archives.example.org/3.jpg".into(),
            file_name: "3.jpg".into(),
        };
        assert_eq!(
            remote.preview_url(),
            Some("https://archives.example.org/3.jpg")
        );

        let pdf = PendingPage::Remote {
            url: "https://archives.example.org/act.pdf".into(),
            file_name: "act.pdf".into(),
        };
        assert_eq!(pdf.preview_url(), None);

        // Bytes we hold have no address to point an `<img>` at until they are
        // uploaded.
        let local = PendingPage::File {
            name: "scan.jpg".into(),
            bytes: Vec::new(),
        };
        assert_eq!(local.preview_url(), None);
    }
}
