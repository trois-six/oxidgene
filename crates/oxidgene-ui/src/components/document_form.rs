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
use oxidgene_core::enums::{DocumentCategory, Privacy, SourceMediaType};
use uuid::Uuid;

use crate::api::{
    ApiClient, ApiError, CreateMediaBody, CreateMediaLinkBody, CreateNoteBody, MediaKind,
    MediaUpload, UpdateMediaBody,
};
use crate::archive_viewer::{ArchiveRegister, ViewPage};
use crate::components::date_input::{DateInput, DateParts};
use crate::components::media_gallery::{
    MediaClassification, MediaEventsChecklist, MediaOwner, MediaTagForm,
};
use crate::components::media_input::{MediaInput, PickedFile, friendly};
use crate::components::place_input::{render_place_input, resolve_place};
use crate::i18n::use_i18n;

/// A page somebody else serves, held by its address. Nothing is fetched,
/// then or later.
#[derive(Clone, Debug, PartialEq)]
pub struct RemotePage {
    pub url: String,
    pub file_name: String,
    /// A small picture of the page its server also serves, which gallery
    /// tiles draw instead of the full one.
    pub thumbnail_url: Option<String>,
    /// The picture's pixel size, when its server states it.
    pub size: Option<(i32, i32)>,
    /// The register view this page is, for a page of an archive register.
    pub view: Option<u16>,
}

impl RemotePage {
    /// A page typed as an address, of which nothing else is known.
    fn typed(url: String) -> Self {
        Self {
            file_name: url_file_name(&url),
            url,
            thumbnail_url: None,
            size: None,
            view: None,
        }
    }

    /// An archive's view: its picture, its size and its thumbnail.
    pub fn of_view(page: &ViewPage) -> Self {
        let url = page.image.picture.clone();
        // The archive's picture address names no file worth showing —
        // `default.jpg` on every view — so the page is named after its view.
        let extension = url_file_name(&url)
            .rsplit_once('.')
            .map(|(_, extension)| extension.to_ascii_lowercase())
            .unwrap_or_else(|| "jpg".to_string());
        Self {
            file_name: format!("{}.{extension}", page.view),
            url,
            thumbnail_url: Some(page.image.thumbnail.clone()),
            size: Some((
                i32::try_from(page.image.width).unwrap_or(i32::MAX),
                i32::try_from(page.image.height).unwrap_or(i32::MAX),
            )),
            view: Some(page.view),
        }
    }
}

/// A page the user has chosen but that has not been written yet.
#[derive(Clone, PartialEq)]
enum PendingPage {
    /// Bytes read from the picker or a drop, waiting for a document to belong
    /// to.
    File { name: String, bytes: Vec<u8> },
    /// An address somebody else serves.
    Remote(RemotePage),
}

impl PendingPage {
    fn label(&self) -> &str {
        match self {
            Self::File { name, .. } => name,
            Self::Remote(page) => &page.url,
        }
    }

    /// The address to preview from, when the page is a picture we can point an
    /// `<img>` at without holding it: its thumbnail when its server serves
    /// one.
    fn preview_url(&self) -> Option<&str> {
        match self {
            Self::File { .. } => None,
            Self::Remote(page) => {
                if let Some(thumbnail) = &page.thumbnail_url {
                    return Some(thumbnail);
                }
                let mime = oxidgene_core::types::normalize_mime(None, &page.file_name);
                (crate::api::media_kind(&mime) == MediaKind::Image).then_some(page.url.as_str())
            }
        }
    }

    /// The register view this page is, if any.
    fn view(&self) -> Option<u16> {
        match self {
            Self::File { .. } => None,
            Self::Remote(page) => page.view,
        }
    }
}

/// What the form opens with when its caller has already assembled the
/// document, as attaching an archive's cited views does: nothing of
/// it is written until Save.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct DocumentDraft {
    pub title: String,
    pub description: String,
    pub category: Option<DocumentCategory>,
    pub medium: SourceMediaType,
    pub pages: Vec<RemotePage>,
    /// The events the document is checked as documenting.
    pub event_ids: Vec<Uuid>,
    /// The source the document is linked to.
    pub source_id: Option<Uuid>,
    /// The archive register the pages are views of: the form then offers its
    /// previous and next views, and names the document after its views until
    /// the reader names it.
    pub register: Option<ArchiveRegister>,
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
    /// What the form opens with, when its caller assembled the document.
    #[props(default)]
    pub draft: Option<DocumentDraft>,
    /// Fired with the document after it and everything in it has been
    /// written.
    pub on_created: EventHandler<Uuid>,
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

    let draft = props.draft.clone().unwrap_or_default();
    let register = draft.register.clone();
    let source_id = draft.source_id;
    let pages = use_signal(|| {
        draft
            .pages
            .iter()
            .cloned()
            .map(PendingPage::Remote)
            .collect::<Vec<_>>()
    });
    let mut title = use_signal(|| draft.title.clone());
    let mut description = use_signal(|| draft.description.clone());
    let tags = use_signal(Vec::<String>::new);
    let document_category = use_signal(|| draft.category);
    let source_media_type = use_signal(|| draft.medium);
    let privacy = use_signal(Privacy::default);
    let date_parts = use_signal(DateParts::default);
    let place_id = use_signal(String::new);
    let mut note_text = use_signal(String::new);
    let mut selected_events = use_signal(|| draft.event_ids.clone());
    // Whether the reader wrote the title or the description: until then they
    // follow the views of an archive register as views are added or removed.
    let mut title_written = use_signal(|| false);
    let mut description_written = use_signal(|| false);
    use_effect({
        let register = register.clone();
        move || {
            let views: Vec<u16> = pages.read().iter().filter_map(PendingPage::view).collect();
            let Some(register) = &register else { return };
            if views.is_empty() {
                return;
            }
            if !*title_written.peek() {
                title.set(register.document_title(&i18n, &views));
            }
            if !*description_written.peek() {
                description.set(register.attribution(&views).unwrap_or_default());
            }
        }
    });
    let mut saving = use_signal(|| false);
    let mut progress = use_signal(|| None::<(usize, usize)>);
    let mut error = use_signal(|| None::<String>);

    // A new document sits on no place yet: the field only suggests, and the
    // server answers that.
    let place_options: Vec<(String, String)> = Vec::new();

    let save = {
        let api = api.clone();
        move |_| {
            let api = api.clone();
            let title_value = title().trim().to_string();
            let place_text = place_id();
            let request = WriteRequest {
                pages,
                progress,
                description: description().trim().to_string(),
                note: note_text().trim().to_string(),
                place_id: None,
                privacy: privacy(),
                medium: source_media_type(),
                category: document_category(),
                date: date_parts().resolved(),
                tags: tags(),
                event_ids: selected_events(),
                source_id,
            };
            spawn(async move {
                saving.set(true);
                error.set(None);
                let created = create_document(
                    &api,
                    tree_id,
                    owner,
                    (title_value, place_text),
                    request,
                    &i18n,
                )
                .await;
                progress.set(None);
                saving.set(false);
                match created {
                    Ok(document_id) => {
                        on_created.call(document_id);
                        on_close.call(());
                    }
                    Err(message) => error.set(Some(message)),
                }
            });
        }
    };

    let total = pages.read().len();
    let busy = saving();

    rsx! {
        div {
            class: "cropper-backdrop",
            onmousedown: move |event| event.stop_propagation(),
            onclick: move |_| if !busy { on_close.call(()) },
            div { class: "document-form-modal", onclick: move |event| event.stop_propagation(),
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
                div { class: "document-form-body",
                    div { class: "media-panel is-embedded",

                        div { class: "form-group",
                            label { {i18n.t("media.title")} }
                            input {
                                r#type: "text",
                                value: "{title}",
                                disabled: busy,
                                oninput: move |e: Event<FormData>| {
                                    title_written.set(true);
                                    title.set(e.value());
                                },
                            }
                        }
                        div { class: "form-group",
                            label { {i18n.t("media.description")} }
                            textarea {
                                rows: 3,
                                value: "{description}",
                                disabled: busy,
                                oninput: move |e: Event<FormData>| {
                                    description_written.set(true);
                                    description.set(e.value());
                                },
                            }
                        }

                        PendingTagsEditor { tags, disabled: busy }
                        MediaClassification {
                            document_category,
                            source_media_type,
                            privacy,
                            disabled: busy,
                        }

                        div { class: "form-group",
                            label { {i18n.t("media.date")} }
                            DateInput { parts: date_parts, i18n, on_change: move |()| {} }
                        }
                        {render_place_input(&i18n, tree_id, place_id, &place_options, || {})}

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
                            MediaEventsChecklist {
                                events: events.clone(),
                                attached: selected_events(),
                                disabled: busy,
                                on_toggle: move |(event_id, on): (Uuid, bool)| {
                                    let mut list = selected_events.write();
                                    list.retain(|id| *id != event_id);
                                    if on {
                                        list.push(event_id);
                                    }
                                },
                            }
                        }

                        // Pages last, immediately above Save. The fields above
                        // describe the document; this is the document itself,
                        // and it is the last thing the user assembles before
                        // committing.
                        PendingPagesEditor { tree_id, pages, busy, register: register.clone() }

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

/// Creates the document titled `title`, at the place named `place`, and
/// fills it in; the message to show on failure.
///
/// The place is resolved before anything is written, so a place that cannot
/// be resolved leaves nothing to undo. Creating the document is the only
/// step that can fail without leaving anything behind; after it, a failure
/// purges the document — best effort: if the rollback itself fails the
/// original error is still the one worth reporting — so the user sees the
/// form they were filling in rather than a half-written record in the
/// gallery behind it.
async fn create_document(
    api: &ApiClient,
    tree_id: Uuid,
    owner: MediaOwner,
    (title, place): (String, String),
    mut request: WriteRequest,
    i18n: &crate::i18n::I18n,
) -> Result<Uuid, String> {
    request.place_id = resolve_place(api, tree_id, &place, i18n.0.reference_code())
        .await
        .map_err(|err| err.to_string())?;
    let title = Some(title.as_str()).filter(|title| !title.is_empty());
    let document = api
        .create_media_document(tree_id, title)
        .await
        .map_err(|err| err.to_string())?;
    let written = write_document(api, tree_id, document.id, owner, request, i18n).await;
    if written.is_err() {
        let _ = api.delete_media(tree_id, document.id).await;
    }
    written.map(|()| document.id)
}

/// The document's tags, each removable, and the form adding one — held
/// here until the document is written.
#[component]
fn PendingTagsEditor(tags: Signal<Vec<String>>, disabled: bool) -> Element {
    let i18n = use_i18n();
    let mut show_tag_form = use_signal(|| false);
    rsx! {
        div { class: "pf-subblock media-tags-editor",
            div { class: "pf-block-label",
                button {
                    class: if show_tag_form() { "pf-add-btn is-open" } else { "pf-add-btn" },
                    r#type: "button",
                    disabled,
                    onclick: move |_| show_tag_form.toggle(),
                    {i18n.t("media.add_tag")}
                }
            }
            if show_tag_form() {
                MediaTagForm {
                    on_add: move |value: String| {
                        let known = tags().iter().any(|tag| tag.eq_ignore_ascii_case(&value));
                        if !value.is_empty() && !known {
                            show_tag_form.set(false);
                            tags.write().push(value);
                        }
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
                                onclick: move |_| {
                                    tags.write().remove(index);
                                },
                                "\u{00D7}"
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The document's pages: moved, retyped when they are addresses, removed,
/// and added as files or as addresses.
#[component]
fn PendingPagesEditor(
    tree_id: Uuid,
    pages: Signal<Vec<PendingPage>>,
    busy: bool,
    register: Option<ArchiveRegister>,
) -> Element {
    let i18n = use_i18n();
    let mut url_draft = use_signal(String::new);
    let mut editing_url = use_signal(|| None::<UrlEdit>);
    // One handler for both "add an address" and "correct that address": the
    // difference is a position, and a typo in a URL is found after it has
    // been added far more often than while it is being typed.
    let commit_url = use_callback(move |()| {
        let url = url_draft().trim().to_string();
        if url.is_empty() {
            return;
        }
        place_page(
            pages,
            editing_url(),
            PendingPage::Remote(RemotePage::typed(url)),
        );
        url_draft.set(String::new());
        editing_url.set(None);
    });
    let rows: Vec<PageRow> = pages
        .read()
        .iter()
        .enumerate()
        .map(|(index, page)| PageRow {
            index,
            label: page.label().to_string(),
            preview: page.preview_url().map(str::to_string),
            remote: matches!(page, PendingPage::Remote(_)),
        })
        .collect();
    let total = rows.len();
    let url_open = editing_url().is_some();
    rsx! {
        div { class: "media-panel-section",
            label { {i18n.t("media.pages")} }
            p { class: "pf-ns-hint", {i18n.t("media.new_document_pages_hint")} }
            div { class: "doc-pages",
                for row in rows {
                    PendingPageRow {
                        key: "{row.index}-{row.label}",
                        index: row.index,
                        label: row.label.clone(),
                        preview: row.preview.clone(),
                        remote: row.remote,
                        last: row.index + 1 >= total,
                        busy,
                        on_move: move |(from, to): (usize, usize)| pages.write().swap(from, to),
                        on_retype: move |(index, address): (usize, String)| {
                            url_draft.set(address);
                            editing_url.set(Some(UrlEdit::Existing(index)));
                        },
                        on_remove: move |index: usize| {
                            pages.write().remove(index);
                            // The position the field was standing in for has
                            // moved.
                            editing_url.set(None);
                        },
                    }
                }

                // The canonical upload cell, told to hand the bytes over
                // instead of sending them.
                MediaInput {
                    tree_id,
                    label: i18n.t("media.add_pages"),
                    on_files: move |files: Vec<PickedFile>| {
                        pages
                            .write()
                            .extend(files.into_iter().map(|(name, bytes)| PendingPage::File { name, bytes }));
                    },
                }

                // The other half of "a page is a file or an address", drawn
                // as the same kind of cell: a page somebody else serves is a
                // page, and adding one is the same gesture as adding a file,
                // not a different control in a different place.
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
            if let Some(register) = register {
                RegisterViewButtons { register, pages, busy }
            }
        }
    }
}

/// Which end of an archive register's run of views a view is added at.
#[derive(Clone, Copy, PartialEq)]
enum RegisterEnd {
    Previous,
    Next,
}

/// The view before or after the archive views among `pages`, when the
/// register has one.
fn neighbour_view(
    pages: &[PendingPage],
    register: &ArchiveRegister,
    end: RegisterEnd,
) -> Option<u16> {
    let views = pages.iter().filter_map(PendingPage::view);
    let view = match end {
        RegisterEnd::Previous => views.min()?.checked_sub(1)?,
        RegisterEnd::Next => views.max()?.checked_add(1)?,
    };
    register.has_view(view).then_some(view)
}

/// Adds the previous or the next view of the register the pages are views
/// of, each resolved on its click.
#[component]
fn RegisterViewButtons(
    register: ArchiveRegister,
    pages: Signal<Vec<PendingPage>>,
    busy: bool,
) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let mut adding = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let add = use_callback({
        let register = register.clone();
        move |end: RegisterEnd| {
            let Some(view) = neighbour_view(&pages.peek(), &register, end) else {
                return;
            };
            let (api, register) = (api.clone(), register.clone());
            spawn(async move {
                adding.set(true);
                error.set(None);
                match register.view(&api, view).await {
                    Ok(Some(page)) => {
                        let page = PendingPage::Remote(RemotePage::of_view(&page));
                        let mut list = pages.write();
                        let at = match end {
                            RegisterEnd::Previous => list
                                .iter()
                                .position(|page| page.view().is_some())
                                .unwrap_or(0),
                            RegisterEnd::Next => list
                                .iter()
                                .rposition(|page| page.view().is_some())
                                .map_or(list.len(), |at| at + 1),
                        };
                        list.insert(at, page);
                    }
                    Ok(None) => error.set(Some(i18n.t("archive_viewer.view_unavailable"))),
                    Err(err) => error.set(Some(err.to_string())),
                }
                adding.set(false);
            });
        }
    });
    let list = pages.read();
    let previous = neighbour_view(&list, &register, RegisterEnd::Previous);
    let next = neighbour_view(&list, &register, RegisterEnd::Next);
    let disabled = busy || adding();
    rsx! {
        div { class: "doc-register-views",
            button {
                class: "btn btn-outline btn-sm",
                r#type: "button",
                disabled: disabled || previous.is_none(),
                onclick: move |_| add.call(RegisterEnd::Previous),
                {i18n.t("archive_viewer.add_previous_view")}
            }
            button {
                class: "btn btn-outline btn-sm",
                r#type: "button",
                disabled: disabled || next.is_none(),
                onclick: move |_| add.call(RegisterEnd::Next),
                {i18n.t("archive_viewer.add_next_view")}
            }
        }
        if let Some(message) = error() {
            div { class: "error-msg", "{message}" }
        }
    }
}

/// Puts `page` where the address field was standing: over the page it
/// corrects, or — appended, not inserted: the address is the page the user
/// has just described, and it belongs after the ones already listed — at the
/// end.
fn place_page(mut pages: Signal<Vec<PendingPage>>, at: Option<UrlEdit>, page: PendingPage) {
    let mut pages = pages.write();
    match at {
        Some(UrlEdit::Existing(index)) => {
            if let Some(slot) = pages.get_mut(index) {
                *slot = page;
            }
        }
        Some(UrlEdit::New) | None => pages.push(page),
    }
}

/// One page of the document: its number, its thumbnail, its name, and its
/// moves.
#[component]
fn PendingPageRow(
    index: usize,
    label: String,
    preview: Option<String>,
    remote: bool,
    last: bool,
    busy: bool,
    on_move: EventHandler<(usize, usize)>,
    on_retype: EventHandler<(usize, String)>,
    on_remove: EventHandler<usize>,
) -> Element {
    let i18n = use_i18n();
    let address = label.clone();
    rsx! {
        div { class: "doc-page",
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
                    onclick: move |_| on_move.call((index, index - 1)),
                    "\u{2191}"
                }
                button {
                    class: "pf-row-btn",
                    r#type: "button",
                    disabled: last || busy,
                    title: i18n.t("media.page_move_down"),
                    onclick: move |_| on_move.call((index, index + 1)),
                    "\u{2193}"
                }
                // Only an address can be retyped, so only an address offers
                // the pencil.
                if remote {
                    button {
                        class: "pf-row-btn",
                        r#type: "button",
                        disabled: busy,
                        title: i18n.t("media.edit_url"),
                        onclick: move |_| on_retype.call((index, address.clone())),
                        "\u{270E}"
                    }
                }
                button {
                    class: "pf-row-btn is-danger",
                    r#type: "button",
                    disabled: busy,
                    title: i18n.t("media.page_remove"),
                    onclick: move |_| on_remove.call(index),
                    "\u{2715}"
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
    /// The source the document is linked to, if any.
    source_id: Option<Uuid>,
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
        source_id,
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
        add_page(api, tree_id, document_id, page, i18n).await?;
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
                repository_id: None,
            },
        )
        .await
        .map_err(|err| err.to_string())?;
    }

    // Linked last: until this runs the document belongs to nobody, which is
    // exactly what a rollback wants to be true.
    link_document(api, tree_id, document_id, owner, event_ids, source_id)
        .await
        .map_err(|err| err.to_string())
}

/// Add one page to a document: uploaded when it is a file, recorded by its
/// address when somebody else serves it. The error names the page.
async fn add_page(
    api: &ApiClient,
    tree_id: Uuid,
    document_id: Uuid,
    page: PendingPage,
    i18n: &crate::i18n::I18n,
) -> Result<(), String> {
    match page {
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
        PendingPage::Remote(page) => api
            .create_media(
                tree_id,
                &CreateMediaBody {
                    document_id,
                    file_name: page.file_name,
                    // Left empty on purpose: the server guesses from the
                    // address, which is the only evidence there is for a
                    // file nobody is going to fetch.
                    mime_type: String::new(),
                    file_path: page.url.clone(),
                    file_size: 0,
                    title: None,
                    description: None,
                    thumbnail_url: page.thumbnail_url,
                    width: page.size.map(|(width, _)| width),
                    height: page.size.map(|(_, height)| height),
                },
            )
            .await
            .map(|_| ())
            .map_err(|err| format!("{}: {}", page.url, friendly(&err, i18n))),
    }
}

/// Attach a document to its owner, then to every event it proves that is not
/// that owner already, then to its source.
async fn link_document(
    api: &ApiClient,
    tree_id: Uuid,
    document_id: Uuid,
    owner: MediaOwner,
    event_ids: Vec<Uuid>,
    source_id: Option<Uuid>,
) -> Result<(), ApiError> {
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
    .await?;

    for event_id in event_ids {
        if matches!(owner, MediaOwner::Event(id) if id == event_id) {
            continue;
        }
        api.create_media_link(
            tree_id,
            &CreateMediaLinkBody::to_event(document_id, event_id),
        )
        .await?;
    }
    if let Some(source_id) = source_id {
        api.create_media_link(
            tree_id,
            &CreateMediaLinkBody::to_source(document_id, source_id),
        )
        .await?;
    }
    Ok(())
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
        let remote = PendingPage::Remote(RemotePage::typed(
            "https://archives.example.org/3.jpg".into(),
        ));
        assert_eq!(
            remote.preview_url(),
            Some("https://archives.example.org/3.jpg")
        );

        let pdf = PendingPage::Remote(RemotePage::typed(
            "https://archives.example.org/act.pdf".into(),
        ));
        assert_eq!(pdf.preview_url(), None);

        // A page whose server serves a thumbnail is previewed from it.
        let view = PendingPage::Remote(RemotePage {
            thumbnail_url: Some("https://archives.example.org/3_thumbnail.jpg".into()),
            ..RemotePage::typed("https://archives.example.org/iiif/3/full/max/0/default".into())
        });
        assert_eq!(
            view.preview_url(),
            Some("https://archives.example.org/3_thumbnail.jpg")
        );

        // Bytes we hold have no address to point an `<img>` at until they are
        // uploaded.
        let local = PendingPage::File {
            name: "scan.jpg".into(),
            bytes: Vec::new(),
        };
        assert_eq!(local.preview_url(), None);
    }
}
