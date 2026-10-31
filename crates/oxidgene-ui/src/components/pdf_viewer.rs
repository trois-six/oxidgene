//! The PDF viewer and the gallery's PDF previews, drawn by pdf.js.
//!
//! A stored PDF opens in the media viewer as a document to read, not as a
//! picture: its pages scroll one after the other, its text can be selected,
//! copied and searched, its links work, and zooming redraws the pages rather
//! than magnifying a bitmap. pdf.js does the drawing (`pdf_viewer.js`); this
//! module holds the controls, which look and behave like the picture
//! stage's, and decides when a PDF is drawn this way.
//!
//! The library is served by the application itself — the web bundle's static
//! files, the desktop's own handler — and loaded the first time a PDF is
//! opened or previewed, never with the page. The file reaches it the way
//! `docs/cross-cutting.md` §7.1 lets media reach the engine: on the desktop
//! as a path of the shell's own origin, on the web as bytes fetched through
//! the typed client, never as a backend address.

use std::cell::Cell;
use std::rc::Rc;

use base64::Engine as _;
use dioxus::document::Eval;
use dioxus::prelude::*;
use oxidgene_core::types::Media;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::{ApiClient, ApiError, GalleryPdfPage, MediaKind, MediaSource, ScriptFile};
use crate::components::media_stage::{MIN_ZOOM, ZoomControls, zoom_in, zoom_out};
use crate::i18n::use_i18n;
use crate::listeners::use_element_id;
use crate::ui_observability::use_ui_trace;

/// The page-side half: the viewer, the previews and the library loader.
const SCRIPT: &str = include_str!("pdf_viewer.js");

/// The bytes of a file the web build hands over per message, as base64. A
/// multiple of three, so every chunk but the last encodes without padding
/// and the script can decode each on its own.
const CHUNK_BYTES: usize = 3 * 1024 * 1024;

/// The largest PDF a gallery tile draws a preview of. A preview reads the
/// whole file — over the network on the web — to draw one small page, which
/// past this size is more than a tile is worth: it keeps its icon.
pub(crate) const PREVIEW_MAX_BYTES: i64 = 32 * 1024 * 1024;

/// The zoom ceiling of a PDF, as a percentage of its actual size: pdf.js's
/// own. Its pages are redrawn at every level, so this is as sharp as the
/// picture stage's 3200% of a fitted scan, and far less memory.
const MAX_PDF_ZOOM: u32 = 1000;

/// Whether the viewer shows a medium of `kind` from `source` in the PDF
/// viewer: a PDF we hold, until pdf.js has failed to open it. A PDF only
/// known by its address is somebody else's file, which the application
/// never fetches; it keeps the panel offering its link.
pub(crate) fn opens_in_pdf_viewer(kind: MediaKind, source: MediaSource, failed: bool) -> bool {
    kind == MediaKind::Pdf && source == MediaSource::Stored && !failed
}

/// The PDF page whose first page a gallery tile of `media` draws with
/// pdf.js, if any: a PDF we hold that the server made no thumbnail for —
/// typically one of text, with no scan to take a thumbnail from — and small
/// enough to read for it. A page tile is its own; a document's is the one
/// its gallery bundle names, `pdf_page`, when none of its pages has a
/// preview. Called for a tile with no other picture.
///
/// A display convenience of this session alone: the picture is never stored
/// or sent anywhere, and it changes nothing a thumbnail decides — the medium
/// is no more a portrait, nor a picture beside an attachment, than it was.
pub(crate) fn pdf_preview_page(media: &Media, pdf_page: Option<GalleryPdfPage>) -> Option<Uuid> {
    let small = |size: i64| (0..=PREVIEW_MAX_BYTES).contains(&size);
    if media.is_document() {
        return pdf_page
            .filter(|page| small(page.file_size))
            .map(|page| page.media_id);
    }
    (oxidgene_core::types::is_pdf_mime(&media.mime_type)
        && crate::api::media_source(media) == MediaSource::Stored
        && media.thumbnail_key.is_none()
        && small(media.file_size))
    .then_some(media.id)
}

/// One step in from `percent`, within a PDF's bounds.
fn pdf_zoom_in(percent: u32) -> u32 {
    zoom_in(Some(percent)).min(MAX_PDF_ZOOM)
}

/// One step out from `percent`, within a PDF's bounds.
fn pdf_zoom_out(percent: u32) -> u32 {
    zoom_out(Some(percent))
}

/// `bytes` as the base64 messages the script reassembles.
fn chunks(bytes: &[u8]) -> impl Iterator<Item = String> + '_ {
    bytes
        .chunks(CHUNK_BYTES)
        .map(|chunk| base64::engine::general_purpose::STANDARD.encode(chunk))
}

/// What the script reports.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(tag = "event", rename_all = "snake_case")]
enum ScriptEvent {
    /// The document is open, with this many pages.
    Opened { pages: u32 },
    /// The page most in view.
    Page { page: u32 },
    /// The zoom, as a percentage of the actual size.
    Scale { percent: u32 },
    /// The match of the search shown, and how many there are.
    Found { current: u32, total: u32 },
    /// A preview's turn has come: it wants its file.
    Want,
    /// A preview is drawn, at this `blob:` address.
    Preview { url: String },
    /// pdf.js could not open or draw the file.
    Failed,
}

/// What the viewer tells the script.
#[derive(Debug, Serialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
enum Command {
    Zoom {
        percent: u32,
    },
    Fit,
    Page {
        page: u32,
    },
    /// A new search when `again` is false, its next or previous match
    /// otherwise.
    Find {
        query: String,
        again: bool,
        previous: bool,
    },
    Close,
}

/// The script's channel, shared with the component's handlers and its drop.
type Channel = Rc<Cell<Option<Eval>>>;

/// Sends `message` while the script still listens.
fn tell(channel: &Channel, message: impl Serialize) {
    if let Some(eval) = channel.get() {
        let _ = eval.send(message);
    }
}

/// Hands `file` to the script: a path to fetch, or the bytes in chunks after
/// their size. A file that could not be fetched is an empty announcement,
/// which the script reports as a failure.
fn send_file(eval: &Eval, file: Result<ScriptFile, ApiError>) -> bool {
    let sent = match file {
        Ok(ScriptFile::Hosted(path)) => eval.send(serde_json::json!({ "path": path })),
        Ok(ScriptFile::Bytes(bytes)) => eval
            .send(serde_json::json!({ "size": bytes.len() }))
            .and_then(|()| chunks(&bytes).try_for_each(|chunk| eval.send(chunk))),
        Err(_) => eval.send(serde_json::json!({ "missing": true })),
    };
    sent.is_ok()
}

/// What the viewer knows of the open document, from the script's reports.
#[derive(Clone, Copy)]
struct PdfState {
    open: Signal<bool>,
    pages: Signal<u32>,
    page: Signal<u32>,
    percent: Signal<u32>,
    matches: Signal<Option<(u32, u32)>>,
}

impl PdfState {
    /// Takes in one report; false once the script has failed.
    fn apply(mut self, event: ScriptEvent) -> bool {
        match event {
            ScriptEvent::Opened { pages } => {
                self.pages.set(pages);
                self.open.set(true);
            }
            ScriptEvent::Page { page } => self.page.set(page),
            ScriptEvent::Scale { percent } => self.percent.set(percent),
            ScriptEvent::Found { current, total } => self.matches.set(Some((current, total))),
            ScriptEvent::Failed => return false,
            ScriptEvent::Want | ScriptEvent::Preview { .. } => {}
        }
        true
    }
}

/// A stored PDF, read in the media viewer: zoom, search, and the pages one
/// under the other. The viewer's own pager shows and turns its pages, as it
/// does a document's: `position` is where the reader is, `(page, pages)`
/// from 1, and a page set in `goto` is scrolled to. `on_failed` when pdf.js
/// cannot open it, for the caller to say so in its place.
#[component]
pub(crate) fn PdfViewer(
    tree_id: Uuid,
    media_id: Uuid,
    position: Signal<(u32, u32)>,
    goto: Signal<Option<u32>>,
    on_failed: EventHandler<()>,
    /// Buttons after the zoom ones, as the picture stage's `controls`.
    #[props(default = VNode::empty())]
    controls: Element,
    /// Menus and panels over the pages, as the picture stage's `children`.
    #[props(default)]
    children: Element,
) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let trace = use_ui_trace();
    let id = use_element_id("pdf-viewer");
    let channel: Channel = use_hook(|| Rc::new(Cell::new(None)));
    let state = PdfState {
        open: use_signal(|| false),
        pages: use_signal(|| 0),
        page: use_signal(|| 1),
        percent: use_signal(|| 100),
        matches: use_signal(|| None),
    };
    let mut query = use_signal(String::new);
    use_drop({
        let channel = channel.clone();
        move || tell(&channel, Command::Close)
    });

    let start = {
        let channel = channel.clone();
        let id = id.clone();
        let page_label = i18n.t("pager.page");
        move |_| {
            let (channel, id, page_label) = (channel.clone(), id.clone(), page_label.clone());
            let (api, trace) = (api.clone(), trace.clone());
            async move {
                let mut eval = document::eval(SCRIPT);
                channel.set(Some(eval));
                let request =
                    serde_json::json!({ "mode": "viewer", "id": id, "page_label": page_label });
                let file = trace.resource("pdf_file", api.media_file_for_script(tree_id, media_id));
                let opened = eval.send(request).is_ok() && send_file(&eval, file.await);
                let mut alive = opened;
                while alive && let Ok(event) = eval.recv::<ScriptEvent>().await {
                    alive = state.apply(event);
                }
                if !alive {
                    channel.set(None);
                    on_failed.call(());
                }
            }
        }
    };
    let command = {
        let channel = channel.clone();
        move |command: Command| tell(&channel, command)
    };
    let find = {
        let command = command.clone();
        move |again: bool, previous: bool| {
            command(Command::Find {
                query: query.peek().clone(),
                again,
                previous,
            });
        }
    };

    let mut position = position;
    use_effect(move || {
        let now = ((state.page)(), (state.pages)());
        if *position.peek() != now {
            position.set(now);
        }
    });
    let mut goto = goto;
    use_effect({
        let command = command.clone();
        move || {
            if let Some(page) = goto() {
                goto.set(None);
                command(Command::Page { page });
            }
        }
    });

    let open = (state.open)();
    let percent = (state.percent)();

    rsx! {
        div { class: "pdf-viewer",
            ZoomControls {
                can_zoom_in: open && percent < MAX_PDF_ZOOM,
                can_zoom_out: open && percent > MIN_ZOOM,
                on_zoom_in: {
                    let command = command.clone();
                    move |()| command(Command::Zoom { percent: pdf_zoom_in(*state.percent.peek()) })
                },
                on_fit: {
                    let command = command.clone();
                    move |()| command(Command::Fit)
                },
                on_zoom_out: {
                    let command = command.clone();
                    move |()| command(Command::Zoom { percent: pdf_zoom_out(*state.percent.peek()) })
                },
                {controls}
                PdfSearch {
                    enabled: open,
                    query: query(),
                    matches: (state.matches)(),
                    on_query: {
                        let find = find.clone();
                        let mut matches = state.matches;
                        move |text: String| {
                            if text.trim().is_empty() {
                                matches.set(None);
                            }
                            query.set(text);
                            find(false, false);
                        }
                    },
                    on_step: move |previous| find(true, previous),
                }
            }
            div { class: "pdf-viewer-frame",
                div {
                    id: "{id}",
                    class: "pdf-viewer-scroll",
                    tabindex: "0",
                    onmounted: start,
                    div { class: "pdfViewer" }
                }
                if !open {
                    div { class: "pdf-viewer-status", {i18n.t("media.pdf_opening")} }
                }
                {children}
            }
        }
    }
}

/// What the search says of its matches: which one is shown out of how many,
/// that there is none, or nothing before a search.
fn match_label(
    i18n: &crate::i18n::I18n,
    query: &str,
    matches: Option<(u32, u32)>,
) -> Option<String> {
    match matches {
        _ if query.trim().is_empty() => None,
        Some((_, 0)) => Some(i18n.t("media.pdf_no_match")),
        Some((current, total)) => Some(i18n.t_args(
            "media.pdf_match_of",
            &[
                ("current", &current.max(1).to_string()),
                ("total", &total.to_string()),
            ],
        )),
        None => None,
    }
}

/// The search in the document's text: Enter finds the next match, Shift+Enter
/// the previous one, as the buttons beside it do.
#[component]
fn PdfSearch(
    enabled: bool,
    query: String,
    matches: Option<(u32, u32)>,
    on_query: EventHandler<String>,
    on_step: EventHandler<bool>,
) -> Element {
    let i18n = use_i18n();
    let label = match_label(&i18n, &query, matches);
    let can_step = enabled && matches.is_some_and(|(_, total)| total > 0);
    rsx! {
        div { class: "pdf-viewer-search", role: "search",
            input {
                r#type: "search",
                class: "pdf-viewer-search-input",
                placeholder: i18n.t("media.pdf_search"),
                aria_label: i18n.t("media.pdf_search"),
                disabled: !enabled,
                value: "{query}",
                oninput: move |event| on_query.call(event.value()),
                onkeydown: move |event: Event<KeyboardData>| {
                    if event.key() == Key::Enter {
                        event.prevent_default();
                        on_step.call(event.modifiers().shift());
                    }
                },
            }
            button {
                class: "isb-btn",
                r#type: "button",
                title: i18n.t("media.pdf_search_previous"),
                aria_label: i18n.t("media.pdf_search_previous"),
                disabled: !can_step,
                onclick: move |_| on_step.call(true),
                svg {
                    width: "16", height: "16", fill: "none", "viewBox": "0 0 24 24",
                    stroke: "currentColor", "strokeWidth": "2",
                    path { d: "M18 15l-6-6-6 6" }
                }
            }
            button {
                class: "isb-btn",
                r#type: "button",
                title: i18n.t("media.pdf_search_next"),
                aria_label: i18n.t("media.pdf_search_next"),
                disabled: !can_step,
                onclick: move |_| on_step.call(false),
                svg {
                    width: "16", height: "16", fill: "none", "viewBox": "0 0 24 24",
                    stroke: "currentColor", "strokeWidth": "2",
                    path { d: "M6 9l6 6 6-6" }
                }
            }
            if let Some(label) = label {
                span { class: "media-pager-count", role: "status", "{label}" }
            }
        }
    }
}

/// Page 1 of a stored PDF, drawn by pdf.js once the tile comes into view, in
/// place of `fallback` — which stays when the file cannot be drawn. The
/// picture is kept for the session, per medium, so a gallery shown again
/// draws it at once.
#[component]
pub(crate) fn PdfPreview(tree_id: Uuid, media_id: Uuid, alt: String, fallback: Element) -> Element {
    let api = use_context::<ApiClient>();
    let trace = use_ui_trace();
    let id = use_element_id("pdf-preview");
    let channel: Channel = use_hook(|| Rc::new(Cell::new(None)));
    let mut picture = use_signal(|| None::<String>);
    use_drop({
        let channel = channel.clone();
        move || tell(&channel, serde_json::json!({ "cancel": true }))
    });

    let start = {
        let channel = channel.clone();
        let id = id.clone();
        move |_| {
            let (channel, id) = (channel.clone(), id.clone());
            let (api, trace) = (api.clone(), trace.clone());
            async move {
                let mut eval = document::eval(SCRIPT);
                channel.set(Some(eval));
                let request = serde_json::json!({ "mode": "preview", "id": id, "key": media_id });
                let mut listening = eval.send(request).is_ok();
                while listening && let Ok(event) = eval.recv::<ScriptEvent>().await {
                    listening = match event {
                        ScriptEvent::Want => {
                            let file = api.media_file_for_script(tree_id, media_id);
                            send_file(&eval, trace.resource("pdf_preview", file).await)
                        }
                        ScriptEvent::Preview { url } => {
                            picture.set(Some(url));
                            false
                        }
                        _ => false,
                    };
                }
                channel.set(None);
            }
        }
    };

    rsx! {
        div { id: "{id}", class: "pdf-preview", onmounted: start,
            match picture() {
                Some(url) => rsx! { img { src: "{url}", alt: "{alt}" } },
                None => fallback,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pdf(thumbnail: bool, stored: bool, size: i64) -> Media {
        Media {
            id: Uuid::nil(),
            tree_id: Uuid::nil(),
            file_name: "register.pdf".to_string(),
            mime_type: "application/pdf".to_string(),
            file_path: if stored {
                String::new()
            } else {
                "https://archives.example.org/register.pdf".to_string()
            },
            storage_key: stored.then(|| "media/register.pdf".to_string()),
            sha256: None,
            thumbnail_key: thumbnail.then(|| "thumbnails/register.jpg".to_string()),
            card_key: None,
            thumbnail_url: None,
            width: None,
            height: None,
            page_count: 1,
            parent_media_id: Some(Uuid::nil()),
            page_index: 0,
            file_size: size,
            title: None,
            description: None,
            date_value: None,
            date_sort: None,
            date_qualifier: Default::default(),
            date_value2: None,
            calendar: Default::default(),
            source_media_type: Default::default(),
            document_category: None,
            tags: Vec::new(),
            place_id: None,
            privacy: Default::default(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            deleted_at: None,
        }
    }

    #[test]
    fn a_stored_pdf_opens_in_the_pdf_viewer_until_pdfjs_fails_on_it() {
        assert!(opens_in_pdf_viewer(
            MediaKind::Pdf,
            MediaSource::Stored,
            false
        ));
        // Then the fallback panel says so and offers the download.
        assert!(!opens_in_pdf_viewer(
            MediaKind::Pdf,
            MediaSource::Stored,
            true
        ));
        // Somebody else's file is never fetched, and a record without bytes
        // has nothing to open.
        assert!(!opens_in_pdf_viewer(
            MediaKind::Pdf,
            MediaSource::Remote,
            false
        ));
        assert!(!opens_in_pdf_viewer(
            MediaKind::Pdf,
            MediaSource::Unheld,
            false
        ));
        // Pictures stay on the stage; other formats keep the fallback.
        assert!(!opens_in_pdf_viewer(
            MediaKind::Image,
            MediaSource::Stored,
            false
        ));
        assert!(!opens_in_pdf_viewer(
            MediaKind::Document,
            MediaSource::Stored,
            false
        ));
    }

    #[test]
    fn only_a_held_pdf_without_a_server_thumbnail_gets_a_preview() {
        let page = |thumbnail, stored, size| {
            let page = pdf(thumbnail, stored, size);
            pdf_preview_page(&page, None)
        };
        assert_eq!(page(false, true, 40_000), Some(Uuid::nil()));
        // The server's thumbnail wins: it costs one small image.
        assert_eq!(page(true, true, 40_000), None);
        assert_eq!(page(false, false, 40_000), None);
        assert_eq!(page(false, true, PREVIEW_MAX_BYTES), Some(Uuid::nil()));
        assert_eq!(page(false, true, PREVIEW_MAX_BYTES + 1), None);
        let mut photo = pdf(false, true, 40_000);
        photo.mime_type = "image/jpeg".to_string();
        assert_eq!(pdf_preview_page(&photo, None), None);
    }

    #[test]
    fn a_document_draws_the_pdf_page_its_bundle_names() {
        let mut document = pdf(false, false, 0);
        document.parent_media_id = None;
        document.mime_type = oxidgene_core::types::DOCUMENT_MIME.to_string();
        let page_id = Uuid::now_v7();
        let named = |file_size| {
            Some(GalleryPdfPage {
                media_id: page_id,
                file_size,
            })
        };
        assert_eq!(pdf_preview_page(&document, named(40_000)), Some(page_id));
        assert_eq!(
            pdf_preview_page(&document, named(PREVIEW_MAX_BYTES + 1)),
            None
        );
        // A document with no such page keeps its icon.
        assert_eq!(pdf_preview_page(&document, None), None);
    }

    #[test]
    fn a_pdf_zooms_in_the_stage_steps_within_its_own_bounds() {
        assert_eq!(pdf_zoom_in(100), 120);
        assert_eq!(pdf_zoom_out(120), 100);
        assert_eq!(pdf_zoom_in(950), MAX_PDF_ZOOM);
        assert_eq!(pdf_zoom_in(MAX_PDF_ZOOM), MAX_PDF_ZOOM);
        assert_eq!(pdf_zoom_out(MIN_ZOOM), MIN_ZOOM);
    }

    #[test]
    fn a_file_is_cut_into_chunks_that_decode_one_by_one() {
        use base64::engine::general_purpose::STANDARD;
        let bytes: Vec<u8> = (0..(CHUNK_BYTES * 2 + 7))
            .map(|i| (i % 251) as u8)
            .collect();
        let parts: Vec<String> = chunks(&bytes).collect();
        assert_eq!(parts.len(), 3);
        // No chunk but the last carries padding, so each decodes alone.
        assert!(parts[..2].iter().all(|part| !part.ends_with('=')));
        let joined: Vec<u8> = parts
            .iter()
            .flat_map(|part| STANDARD.decode(part).unwrap())
            .collect();
        assert_eq!(joined, bytes);
        assert_eq!(chunks(&[]).count(), 0);
    }

    #[test]
    fn the_script_and_the_viewer_speak_the_same_messages() {
        let event = |json: &str| serde_json::from_str::<ScriptEvent>(json).unwrap();
        assert_eq!(
            event(r#"{"event":"opened","pages":3}"#),
            ScriptEvent::Opened { pages: 3 }
        );
        assert_eq!(
            event(r#"{"event":"found","current":1,"total":6}"#),
            ScriptEvent::Found {
                current: 1,
                total: 6
            }
        );
        assert_eq!(event(r#"{"event":"want"}"#), ScriptEvent::Want);
        assert_eq!(event(r#"{"event":"failed"}"#), ScriptEvent::Failed);
        assert_eq!(
            event(r#"{"event":"preview","url":"blob:x"}"#),
            ScriptEvent::Preview {
                url: "blob:x".to_string()
            }
        );
        let command = |command: Command| serde_json::to_value(command).unwrap();
        assert_eq!(
            command(Command::Zoom { percent: 120 }),
            serde_json::json!({ "cmd": "zoom", "percent": 120 })
        );
        assert_eq!(command(Command::Fit), serde_json::json!({ "cmd": "fit" }));
        assert_eq!(
            command(Command::Close),
            serde_json::json!({ "cmd": "close" })
        );
        assert_eq!(
            command(Command::Find {
                query: "Exemple".to_string(),
                again: true,
                previous: false
            }),
            serde_json::json!({ "cmd": "find", "query": "Exemple", "again": true, "previous": false })
        );
        // Every report and command the script names is one this side knows.
        for name in [
            "opened", "page", "scale", "found", "want", "preview", "failed",
        ] {
            assert!(SCRIPT.contains(&format!("event: \"{name}\"")), "{name}");
        }
        for name in ["zoom", "fit", "page", "find", "close"] {
            assert!(SCRIPT.contains(&format!("\"{name}\"")), "{name}");
        }
    }

    #[test]
    fn the_search_counts_its_matches_only_once_it_has_a_query() {
        let i18n = crate::i18n::I18n::new(crate::i18n::Language::english());
        assert_eq!(match_label(&i18n, "", Some((1, 4))), None);
        assert_eq!(match_label(&i18n, "Exemple", None), None);
        assert_eq!(
            match_label(&i18n, "Exemple", Some((0, 0))),
            Some(i18n.t("media.pdf_no_match"))
        );
        assert_eq!(
            match_label(&i18n, "Exemple", Some((2, 6))),
            Some(i18n.t_args("media.pdf_match_of", &[("current", "2"), ("total", "6")]))
        );
    }
}
