//! OxidGene's viewer for an archive whose images it may show
//! (`display: "iiif"`, docs/archives.md §6.3, §6.4).
//!
//! The cited views open in the shared media viewer's frame and stage as an
//! unsaved document whose pages are the archive's picture addresses. The
//! reader pages to the register's previous and next views, each resolved on
//! its click; reads the archive's attribution, linked to its reuse terms,
//! under the picture; opens the view on the portal; and may attach the views
//! as a document through the canonical `DocumentForm`. Nothing is written
//! while the reader only looks.

use dioxus::prelude::*;
use oxidgene_archives::Side;
use oxidgene_core::enums::SourceMediaType;
use oxidgene_core::types::Media;
use uuid::Uuid;

use super::{
    ArchiveLink, ArchivePageRequest, ArchiveRegister, ArchiveViewerMessages, Landing, ViewPage,
    failure_key, use_archive_viewer_bridge,
};
use crate::api::ApiClient;
use crate::components::document_form::{DocumentDraft, DocumentForm, RemotePage};
use crate::components::image_cropper::{CropHalf, ImageCropper};
use crate::components::media_gallery::{MediaFact, MediaOwner};
use crate::components::media_stage::{MediaStage, StagePicture};
use crate::components::pager::Pager;
use crate::i18n::use_i18n;

/// Where the viewer stands.
#[derive(Clone, Debug, PartialEq)]
enum Phase {
    /// The backend is resolving the citation.
    Searching,
    /// The views are shown.
    Shown,
    /// The resolution ended elsewhere than on views OxidGene may show: the
    /// portal page to open, and what to say about it.
    Elsewhere(Landing),
}

/// The half of the picture a cited side is: `d` the right, `g` the left.
fn crop_half(side: Side) -> CropHalf {
    match side {
        Side::Right => CropHalf::Right,
        Side::Left => CropHalf::Left,
    }
}

/// The class of the mark over the cited half of a double page.
fn side_class(side: Side) -> &'static str {
    match side {
        Side::Right => "media-viewer-side is-right",
        Side::Left => "media-viewer-side is-left",
    }
}

/// The page of an attached document holding `page`'s picture.
fn attached_page<'a>(attached: &'a [Media], page: &ViewPage) -> Option<&'a Media> {
    attached
        .iter()
        .find(|media| media.file_path.trim() == page.image.picture)
}

/// What the viewer holds while the reader looks: nothing of it is written.
#[derive(Clone, Copy, PartialEq)]
struct Viewing {
    phase: Signal<Phase>,
    register: Signal<Option<ArchiveRegister>>,
    /// The views loaded so far, in register order.
    pages: Signal<Vec<ViewPage>>,
    /// The index of the view on screen among `pages`.
    current: Signal<usize>,
    /// Whether a view is being resolved.
    turning: Signal<bool>,
    turn_error: Signal<Option<String>>,
    /// Views whose picture the browser could not show.
    failed: Signal<Vec<u16>>,
}

impl Viewing {
    fn shown(&self) -> Option<ViewPage> {
        let pages = (self.pages)();
        let at = (self.current)().min(pages.len().saturating_sub(1));
        pages.get(at).cloned()
    }
}

/// Resolves the citation once, on opening — the reader's click — and shows
/// the views, or ends on the portal: in the desktop's archive window with
/// what OxidGene found, or as a link on the web, where a tab opened now,
/// outside the reader's click, would be blocked.
fn use_resolution(tree_id: Uuid, link: &ArchiveLink, viewing: Viewing, on_close: EventHandler<()>) {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let bridge = use_archive_viewer_bridge();
    let Viewing {
        mut phase,
        mut register,
        mut pages,
        ..
    } = viewing;
    let link = link.clone();
    use_hook(move || {
        spawn(async move {
            let outcome = api
                .archive_target(tree_id, link.source_id, link.citation_id, None)
                .await;
            let landing = match outcome {
                Ok(target) => match ArchiveRegister::of(tree_id, &link, &target) {
                    Some((found, views)) => {
                        register.set(Some(found));
                        pages.set(views);
                        phase.set(Phase::Shown);
                        return;
                    }
                    None => Landing::of(&link, Ok(target)),
                },
                Err(error) => {
                    let code = error.code();
                    Landing::of(&link, Err(code.as_deref().unwrap_or_default()))
                }
            };
            let Some(bridge) = bridge else {
                phase.set(Phase::Elsewhere(landing));
                return;
            };
            bridge.open_page(ArchivePageRequest {
                title: link.title.clone(),
                url: landing.url.clone(),
                banner: landing.banner.map(|banner| banner.text(&i18n)),
                messages: ArchiveViewerMessages::new(&i18n),
            });
            on_close.call(());
        });
    });
}

/// Turns to a view: the one already loaded, or the register's view resolved
/// on this click.
fn use_turn_to(viewing: Viewing) -> Callback<u16> {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let Viewing {
        register,
        mut pages,
        mut current,
        mut turning,
        mut turn_error,
        ..
    } = viewing;
    use_callback(move |view: u16| {
        turn_error.set(None);
        if let Some(at) = pages.peek().iter().position(|page| page.view == view) {
            current.set(at);
            return;
        }
        let Some(found) = register.peek().clone().filter(|found| found.has_view(view)) else {
            return;
        };
        let api = api.clone();
        spawn(async move {
            turning.set(true);
            match found.view(&api, view).await {
                Ok(Some(page)) => {
                    let at = pages.peek().partition_point(|loaded| loaded.view < view);
                    pages.write().insert(at, page);
                    current.set(at);
                }
                Ok(None) => turn_error.set(Some(i18n.t("archive_viewer.view_unavailable"))),
                Err(error) => {
                    let code = error.code();
                    turn_error.set(Some(
                        i18n.t(failure_key(code.as_deref().unwrap_or_default())),
                    ));
                }
            }
            turning.set(false);
        });
    })
}

/// OxidGene's viewer over the cited views of `link`, which documents event
/// `event_id`. `on_attached` fires once the views are saved as a document,
/// and once a region is kept from one of them.
#[component]
pub fn ArchiveViewer(
    tree_id: Uuid,
    link: ArchiveLink,
    event_id: Uuid,
    event_label: String,
    on_attached: EventHandler<()>,
    on_close: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let viewing = Viewing {
        phase: use_signal(|| Phase::Searching),
        register: use_signal(|| None),
        pages: use_signal(Vec::new),
        current: use_signal(|| 0),
        turning: use_signal(|| false),
        turn_error: use_signal(|| None),
        failed: use_signal(Vec::new),
    };
    let mut attaching = use_signal(|| false);
    let mut attached = use_signal(|| None::<Vec<Media>>);
    let mut cropping = use_signal(|| None::<(Media, Option<CropHalf>)>);
    use_resolution(tree_id, &link, viewing, on_close);
    let turn_to = use_turn_to(viewing);
    let open_portal = {
        let (bridge, title) = (use_archive_viewer_bridge(), link.title.clone());
        use_callback(move |url: String| {
            if let Some(bridge) = &bridge {
                bridge.open_page(ArchivePageRequest {
                    title: title.clone(),
                    url,
                    banner: None,
                    messages: ArchiveViewerMessages::new(&i18n),
                });
            }
        })
    };
    // Run here rather than in the form, which closes as soon as it saved.
    let record_attached = {
        let api = use_context::<ApiClient>();
        use_callback(move |document_id: Uuid| {
            on_attached.call(());
            let api = api.clone();
            spawn(async move {
                let document = api
                    .list_media_pages(tree_id, document_id)
                    .await
                    .unwrap_or_default();
                attached.set(Some(document));
            });
        })
    };
    let title = link.title.clone();
    let event = (event_id, event_label);

    rsx! {
        div { class: "cropper-backdrop", onclick: move |_| on_close.call(()),
            div { class: "media-viewer", onclick: move |event| event.stop_propagation(),
                div { class: "cropper-head",
                    span { class: "cropper-title", "{title}" }
                    button {
                        class: "cropper-close",
                        r#type: "button",
                        aria_label: i18n.t("common.close"),
                        onclick: move |_| on_close.call(()),
                        "\u{00D7}"
                    }
                }
                div { class: "media-viewer-body",
                    ViewerAside {
                        archive: link.archive.name.clone(),
                        register: (viewing.register)(),
                        shown: viewing.shown(),
                        attached: attached(),
                        open_portal,
                        on_attach: move |()| attaching.set(true),
                        on_crop: move |crop| cropping.set(Some(crop)),
                    }
                    div { class: "media-viewer-main",
                        ViewerMain { viewing, title: title.clone(), open_portal, turn_to }
                    }
                }
                div { class: "cropper-foot",
                    div { class: "cropper-actions",
                        button {
                            class: "btn btn-primary",
                            r#type: "button",
                            onclick: move |_| on_close.call(()),
                            {i18n.t("common.close")}
                        }
                    }
                }
            }
            if attaching() && let Some(register) = (viewing.register)() {
                AttachForm {
                    register,
                    pages: (viewing.pages)(),
                    event: event.clone(),
                    on_created: record_attached,
                    on_close: move |()| attaching.set(false),
                }
            }
            if let Some((media, half)) = cropping() {
                ImageCropper {
                    tree_id,
                    media,
                    events: vec![event.clone()],
                    half,
                    on_saved: move |_| on_attached.call(()),
                    on_close: move |()| cropping.set(None),
                }
            }
        }
    }
}

/// The viewer's side column: the archive, the call number and the view on
/// screen, then what the reader may do with it.
#[component]
fn ViewerAside(
    archive: String,
    register: Option<ArchiveRegister>,
    shown: Option<ViewPage>,
    attached: Option<Vec<Media>>,
    open_portal: Callback<String>,
    on_attach: EventHandler<()>,
    on_crop: EventHandler<(Media, Option<CropHalf>)>,
) -> Element {
    let i18n = use_i18n();
    let view = shown.as_ref().map(|page| {
        let number = page.view.to_string();
        if page.cited {
            i18n.t_args("archive_viewer.cited_view", &[("view", &number)])
        } else {
            number
        }
    });
    // Once attached, the page on screen may be cut, starting on its cited half.
    let crop = attached
        .as_ref()
        .zip(shown.as_ref())
        .and_then(|(document, page)| {
            attached_page(document, page).map(|media| (media.clone(), page.side.map(crop_half)))
        });
    rsx! {
        aside { class: "media-viewer-aside",
            div { class: "media-facts",
                MediaFact { label: i18n.t("archive_viewer.archive"), value: Some(archive) }
                MediaFact {
                    label: i18n.t("archive_viewer.call_number"),
                    value: register.as_ref().and_then(|register| register.call_number.clone()),
                }
                MediaFact { label: i18n.t("archive_viewer.view"), value: view }
            }
            div { class: "media-facts-actions",
                if let Some(page) = &shown {
                    PortalLink { url: page.portal_url.clone(), on_open: open_portal }
                }
                if register.is_some() && attached.is_none() {
                    button {
                        class: "pf-confirm-btn",
                        r#type: "button",
                        onclick: move |_| on_attach.call(()),
                        {i18n.t("archive_viewer.attach")}
                    }
                }
                if let Some(crop) = crop {
                    button {
                        class: "btn btn-outline",
                        r#type: "button",
                        onclick: move |_| on_crop.call(crop.clone()),
                        {i18n.t("archive_viewer.crop_act")}
                    }
                }
            }
            if attached.is_some() {
                p { class: "media-attachment-notice", role: "status", {i18n.t("archive_viewer.attached")} }
            }
        }
    }
}

/// The viewer's main column: the lookup, the portal page it ended on, or the
/// view on screen.
#[component]
fn ViewerMain(
    viewing: Viewing,
    title: String,
    open_portal: Callback<String>,
    turn_to: Callback<u16>,
) -> Element {
    let i18n = use_i18n();
    match ((viewing.phase)(), viewing.shown()) {
        (Phase::Searching, _) => rsx! {
            div { class: "media-viewer-fallback", role: "status",
                p { {i18n.t("archive_viewer.searching")} }
            }
        },
        (Phase::Elsewhere(landing), _) => rsx! {
            div { class: "media-viewer-fallback", role: "status",
                p {
                    {landing.banner.map_or_else(|| i18n.t("archive_viewer.no_image"), |banner| banner.text(&i18n))}
                }
                PortalLink { url: landing.url.clone(), on_open: open_portal }
            }
        },
        (Phase::Shown, Some(page)) => rsx! {
            ShownView { viewing, page, title, open_portal, turn_to }
        },
        (Phase::Shown, None) => rsx! {},
    }
}

/// A view on the shared stage, its cited half marked, the archive's credit
/// under it linked to the reuse terms, and the pager over the register.
#[component]
fn ShownView(
    viewing: Viewing,
    page: ViewPage,
    title: String,
    open_portal: Callback<String>,
    turn_to: Callback<u16>,
) -> Element {
    let i18n = use_i18n();
    let mut failed = viewing.failed;
    let register = (viewing.register)();
    let attribution = register
        .as_ref()
        .and_then(|register| register.attribution(&[page.view]));
    let terms = register
        .as_ref()
        .and_then(|register| register.terms().map(str::to_string));
    let count = register.as_ref().and_then(|register| register.view_count);
    let number = page.view.to_string();
    let status = match count {
        Some(count) => i18n.t_args(
            "archive_viewer.view_of",
            &[("view", &number), ("count", &count.to_string())],
        ),
        None => i18n.t_args("archive_viewer.view_n", &[("view", &number)]),
    };
    let view = page.view;
    rsx! {
        MediaStage {
            key: "{page.view}",
            stage_key: format!("archive-{}", page.view),
            picture: (!failed().contains(&page.view)).then(|| StagePicture {
                url: page.image.picture.clone(),
                alt: attribution.clone().unwrap_or(title),
            }),
            on_picture_error: move |()| failed.write().push(view),
            overlays: rsx! {
                if let Some(side) = page.side {
                    div { class: side_class(side), title: i18n.t("archive_viewer.cited_half") }
                }
            },
            // §7: a picture that does not load leaves the portal's page.
            fallback: rsx! {
                div { class: "media-viewer-fallback",
                    p { {i18n.t("archive_viewer.picture_failed")} }
                    PortalLink { url: page.portal_url.clone(), on_open: open_portal }
                }
            },
        }
        if let Some(attribution) = attribution {
            p { class: "media-viewer-attribution",
                match terms {
                    Some(terms) => rsx! {
                        a {
                            href: "{terms}",
                            target: "_blank",
                            rel: "noopener noreferrer",
                            title: i18n.t("archive_viewer.terms"),
                            "{attribution}"
                        }
                    },
                    None => rsx! { "{attribution}" },
                }
            }
        }
        if let Some(message) = (viewing.turn_error)() {
            div { class: "error-msg", role: "status", "{message}" }
        }
        Pager {
            current: usize::from(page.view.saturating_sub(1)),
            total: count.map_or(usize::from(page.view) + 1, usize::from),
            numbered: false,
            disabled: (viewing.turning)(),
            status,
            previous_label: i18n.t("archive_viewer.previous_view"),
            next_label: i18n.t("archive_viewer.next_view"),
            class: "media-pager",
            on_select: move |index: usize| {
                if let Ok(view) = u16::try_from(index + 1) {
                    turn_to.call(view);
                }
            },
        }
    }
}

/// The canonical document form, prefilled with the cited views (§6.4).
#[component]
fn AttachForm(
    register: ArchiveRegister,
    pages: Vec<ViewPage>,
    event: (Uuid, String),
    on_created: Callback<Uuid>,
    on_close: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let tree_id = register.tree_id;
    let cited: Vec<ViewPage> = pages.into_iter().filter(|page| page.cited).collect();
    let views: Vec<u16> = cited.iter().map(|page| page.view).collect();
    let draft = DocumentDraft {
        title: register.document_title(&i18n, &views),
        description: register.attribution(&views).unwrap_or_default(),
        category: register.category(),
        medium: SourceMediaType::Manuscript,
        pages: cited.iter().map(RemotePage::of_view).collect(),
        event_ids: vec![event.0],
        source_id: Some(register.link.source_id),
        register: Some(register.clone()),
    };
    rsx! {
        DocumentForm {
            tree_id,
            owner: MediaOwner::Event(event.0),
            events: vec![event.clone()],
            draft: Some(draft),
            on_created: move |document_id: Uuid| on_created.call(document_id),
            on_close,
        }
    }
}

/// « Open on the archive's site »: the archive window on the desktop, a tab
/// that can neither reach the application nor tell the portal where the
/// reader came from on the web (§6.1, §6.2).
#[component]
fn PortalLink(url: String, on_open: Callback<String>) -> Element {
    let i18n = use_i18n();
    let label = i18n.t("archive_viewer.open_portal");
    if use_archive_viewer_bridge().is_some() {
        return rsx! {
            button {
                class: "btn btn-outline",
                r#type: "button",
                onclick: move |_| on_open.call(url.clone()),
                "{label}"
            }
        };
    }
    rsx! {
        a {
            class: "btn btn-outline",
            href: "{url}",
            target: "_blank",
            rel: "noopener noreferrer",
            "{label}"
        }
    }
}

#[cfg(test)]
mod tests {
    use oxidgene_archives::ArchiveImage;

    use super::*;

    fn view(view: u16) -> ViewPage {
        ViewPage {
            view,
            side: None,
            cited: true,
            portal_url: format!("https://archives.example.org/ark:/00000/a1/{view}"),
            image: ArchiveImage {
                picture: format!("https://archives.example.org/iiif/{view}/full/max/0/default.jpg"),
                thumbnail: format!("https://archives.example.org/images/{view}_thumbnail.jpg"),
                width: 3000,
                height: 2000,
            },
        }
    }

    #[test]
    fn a_cited_side_is_that_half() {
        assert_eq!(crop_half(Side::Right), CropHalf::Right);
        assert_eq!(crop_half(Side::Left), CropHalf::Left);
        assert_eq!(side_class(Side::Right), "media-viewer-side is-right");
    }

    #[test]
    fn an_attached_view_is_found_by_its_picture() {
        let mut page: Media = serde_json::from_value(serde_json::json!({
            "id": Uuid::nil(), "tree_id": Uuid::nil(), "file_name": "5.jpg",
            "mime_type": "image/jpeg",
            "file_path": "https://archives.example.org/iiif/5/full/max/0/default.jpg",
            "storage_key": null, "sha256": null, "thumbnail_key": null,
            "width": 3000, "height": 2000, "page_count": 1,
            "parent_media_id": Uuid::nil(), "page_index": 0, "file_size": 0,
            "title": null, "description": null, "date_value": null,
            "date_sort": null, "date_value2": null, "place_id": null,
            "created_at": "2026-01-01T00:00:00Z", "updated_at": "2026-01-01T00:00:00Z",
            "deleted_at": null
        }))
        .unwrap();
        let attached = [page.clone()];
        assert!(attached_page(&attached, &view(5)).is_some());
        assert!(attached_page(&attached, &view(6)).is_none());
        page.file_path = "https://archives.example.org/elsewhere.jpg".into();
        assert!(attached_page(&[page], &view(5)).is_none());
    }
}
