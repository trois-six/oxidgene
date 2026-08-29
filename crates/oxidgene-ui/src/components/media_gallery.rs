//! The media grid that sits in a person's or a couple's edit modal.
//!
//! A tile per file, then the upload cell. Clicking a tile's pencil opens an
//! inline panel under the grid rather than a second modal: the gallery already
//! lives inside a modal, and stacking one on another leaves the user with two
//! Cancel buttons and no way to tell which closes what.
//!
//! # What a tile shows without asking the server twice
//!
//! Every tile needs the thumbnail, the file type and whether it is the profile
//! photo. All three come from the one `media-links?entity_type=…` call, which
//! is why that endpoint returns the media alongside its link; a grid of twenty
//! scans is one request, not twenty-one.
//!
//! # Thumbnails are requested, never assumed
//!
//! `thumbnail_key` being absent is the server saying it could not rasterise
//! this file — a PDF, or an image whose decode failed at upload. The tile
//! draws a labelled file icon in that case instead of a broken image, which is
//! what an `<img>` onto a 404 gives you.

use chrono::NaiveDate;
use dioxus::html::geometry::WheelDelta;
use dioxus::prelude::*;
use oxidgene_core::enums::{DocumentCategory, Privacy, SourceMediaType};
use oxidgene_core::types::{PersonName, Vignette};
use uuid::Uuid;

use crate::api::{
    ApiClient, ApiError, CreateMediaLinkBody, CreateNoteBody, MediaKind, MediaSource,
    MediaWithLink, SetPortraitBody, UpdateMediaBody, UpdateNoteBody, UpdateVignetteBody,
};
use crate::components::confirm_dialog::ConfirmDialog;
use crate::components::context_menu::ContextMenuSurface;
use crate::components::date_input::{DateInput, DateParts, format_date};
use crate::components::image_cropper::ImageCropper;
use crate::components::media_input::MediaInput;
use crate::components::person_form::render_place_select;
use crate::components::search_person::SearchPerson;
use crate::i18n::use_i18n;
use crate::router::Route;
use crate::utils::parse_privacy;

/// What the gallery's media are attached to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MediaOwner {
    Person(Uuid),
    Family(Uuid),
    /// An event's evidence: the certificate, the register page, the photograph
    /// of the ceremony. Distinct from a person's media even when it is the
    /// same file — "the act that proves this marriage" and "a picture of this
    /// person" are two claims, and only the first belongs in a citation.
    Event(Uuid),
}

/// An event available from a person's read-only media gallery.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaEventLinkOption {
    pub event_id: Uuid,
    pub label: String,
    pub date: Option<String>,
    pub date_sort: Option<NaiveDate>,
}

#[component]
fn PrivateThumbnail(tree_id: Uuid, media_id: Uuid, alt: String, class: Option<String>) -> Element {
    let api = use_context::<ApiClient>();
    let image = use_resource(move || {
        let api = api.clone();
        async move { api.media_thumbnail_data_url(tree_id, media_id).await }
    });
    let url = image
        .read_unchecked()
        .as_ref()
        .and_then(|result| result.as_ref().ok())
        .cloned();

    rsx! {
        if let Some(url) = url {
            img { class, src: "{url}", alt, loading: "lazy" }
        }
    }
}

#[component]
fn PrivateVignetteImage(
    tree_id: Uuid,
    vignette_id: Uuid,
    alt: String,
    class: Option<String>,
) -> Element {
    let api = use_context::<ApiClient>();
    let image = use_resource(move || {
        let api = api.clone();
        async move { api.vignette_image_data_url(tree_id, vignette_id).await }
    });
    let url = image
        .read_unchecked()
        .as_ref()
        .and_then(|result| result.as_ref().ok())
        .cloned();

    rsx! {
        if let Some(url) = url {
            img { class, src: "{url}", alt, loading: "lazy" }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MediaEventMenu {
    Link,
}

impl MediaOwner {
    fn entity_type(&self) -> &'static str {
        match self {
            Self::Person(_) => "person",
            Self::Family(_) => "family",
            Self::Event(_) => "event",
        }
    }

    fn id(&self) -> Uuid {
        match self {
            Self::Person(id) | Self::Family(id) | Self::Event(id) => *id,
        }
    }

    /// Only a person has a profile photo — a family's card shows its spouses',
    /// and an event has no portrait to be.
    fn supports_profile(&self) -> bool {
        matches!(self, Self::Person(_))
    }
}

#[derive(Props, Clone, PartialEq)]
pub struct MediaGalleryProps {
    pub tree_id: Uuid,
    pub owner: MediaOwner,
    /// Events a media or a crop may be attached to, as (id, label) pairs.
    #[props(default)]
    pub events: Vec<(Uuid, String)>,
    /// Events the reader may link from a person's media gallery.
    #[props(default)]
    pub profile_event_links: Vec<MediaEventLinkOption>,
    /// Show the files without offering to change them.
    ///
    /// The person profile page is a reader's view: it shows what is attached
    /// and lets a file be opened, but uploading, cropping, retitling and
    /// detaching all belong to the edit modal. Rendering the same grid with
    /// its controls withheld keeps the two views looking like one gallery,
    /// which is what a reader who then clicks Edit expects to find.
    #[props(default = false)]
    pub read_only: bool,
    /// Render only the linked media as a compact row of tiles.
    #[props(default = false)]
    pub compact: bool,
    /// Bumped by a host that uploads media outside the gallery itself.
    #[props(default)]
    pub external_revision: u32,
    /// Fired after any change to what is attached here — an upload, a detach,
    /// a retitle, a portrait being chosen.
    ///
    /// The gallery refreshes itself, but it is not the only thing showing this
    /// data: a profile page draws the person's portrait from the same links,
    /// and without this it kept drawing the old one until the reader navigated
    /// away and back.
    #[props(default)]
    pub on_changed: Option<EventHandler<()>>,
}

/// Thumbnail grid + upload cell + inline edit panel.
#[component]
pub fn MediaGallery(props: MediaGalleryProps) -> Element {
    let api = use_context::<ApiClient>();

    let tree_id = props.tree_id;
    let owner = props.owner;
    let events = props.events.clone();
    let profile_event_links = props.profile_event_links.clone();
    let read_only = props.read_only;
    let compact = props.compact;
    let mut external_revision = use_signal(|| props.external_revision);
    if *external_revision.peek() != props.external_revision {
        external_revision.set(props.external_revision);
    }

    // Bumped after every write; the resource re-runs when it changes. Cheaper
    // and less error-prone than mutating a local list in eight handlers and
    // hoping they all agree with the server.
    let mut revision = use_signal(|| 0_u32);
    let on_changed = props.on_changed;
    // Every mutation goes through here rather than touching `revision`
    // directly: a bump the host is not told about is exactly the bug this
    // exists to prevent.
    // Which media (or crop) represents this person, if the gallery belongs to
    // one. Read from the person rather than from the links: the portrait is a
    // property of the person now, so there is one place to ask.
    let portrait_owner = match owner {
        MediaOwner::Person(id) => Some(id),
        MediaOwner::Family(_) | MediaOwner::Event(_) => None,
    };
    let portrait = use_resource({
        let api = api.clone();
        move || {
            let api = api.clone();
            let _ = revision();
            async move {
                let person_id = portrait_owner?;
                api.get_person(tree_id, person_id)
                    .await
                    .ok()
                    .map(|p| (p.portrait_media_id, p.portrait_vignette_id))
            }
        }
    });
    let portrait_read = portrait.read_unchecked();
    let portrait_pair = portrait_read.as_ref().and_then(|p| p.as_ref()).copied();
    drop(portrait_read);
    let portrait_media_id = portrait_pair.and_then(|(media, _)| media);
    let portrait_vignette_id = portrait_pair.and_then(|(_, vignette)| vignette);

    // Crops of larger images that show this person. A face in a group
    // photograph is one of their pictures as surely as a photograph of them
    // alone is, and until now it appeared in no gallery at all — it existed
    // only inside the scan it was drawn on.
    let vignettes = use_resource({
        let api = api.clone();
        move || {
            let api = api.clone();
            let _ = revision();
            async move {
                let person_id = portrait_owner?;
                api.list_person_vignettes(tree_id, person_id).await.ok()
            }
        }
    });
    let person_vignettes: Vec<Vignette> = vignettes
        .read_unchecked()
        .as_ref()
        .and_then(|v| v.clone())
        .unwrap_or_default();

    let changed = use_callback(move |()| {
        revision += 1;
        if let Some(handler) = on_changed {
            handler.call(());
        }
    });
    let mut editing = use_signal(|| None::<Uuid>);
    let mut cropping = use_signal(|| None::<MediaWithLink>);
    let mut viewing = use_signal(|| None::<MediaWithLink>);
    let mut error = use_signal(|| None::<String>);

    // Props are not reactive: a `use_resource` closure captures them once, so
    // navigating straight from one person to another re-renders the gallery
    // with a new `owner` and keeps showing the previous person's media. That
    // is not only stale, it is somebody else's photographs. Mirroring the prop
    // into a signal makes the read inside the resource reactive, which is what
    // re-runs it — the same shape `person_detail` uses for its person id.
    let mut showing = use_signal(|| (tree_id, owner));
    if *showing.peek() != (tree_id, owner) {
        showing.set((tree_id, owner));
    }

    let tiles = use_resource({
        let api = api.clone();
        move || {
            let api = api.clone();
            let _ = revision();
            let _ = external_revision();
            let (tree_id, owner) = showing();
            async move {
                api.list_entity_media(tree_id, owner.entity_type(), owner.id())
                    .await
            }
        }
    });

    // A media uploaded through the cell below is not attached to anything yet:
    // the upload endpoint records the file, the link is what puts it in *this*
    // gallery. Doing it here rather than server-side keeps the upload endpoint
    // usable from an importer that links nothing.
    let link_uploaded = {
        let api = api.clone();
        move |media_id: Uuid| {
            let api = api.clone();
            spawn(async move {
                let body = CreateMediaLinkBody {
                    media_id,
                    person_id: matches!(owner, MediaOwner::Person(_)).then(|| owner.id()),
                    family_id: matches!(owner, MediaOwner::Family(_)).then(|| owner.id()),
                    event_id: matches!(owner, MediaOwner::Event(_)).then(|| owner.id()),
                    source_id: None,
                    sort_order: 0,
                };
                if let Err(e) = api.create_media_link(tree_id, &body).await {
                    error.set(Some(e.to_string()));
                }
                changed.call(());
            });
        }
    };

    // Creating a document also links it here, exactly as an upload does: a
    // document nobody can find is not a document.
    let new_document = {
        let api = api.clone();
        move |_| {
            let api = api.clone();
            spawn(async move {
                match api.create_media_document(tree_id, None).await {
                    Ok(document) => {
                        let body = CreateMediaLinkBody {
                            media_id: document.id,
                            person_id: matches!(owner, MediaOwner::Person(_)).then(|| owner.id()),
                            family_id: matches!(owner, MediaOwner::Family(_)).then(|| owner.id()),
                            event_id: matches!(owner, MediaOwner::Event(_)).then(|| owner.id()),
                            source_id: None,
                            sort_order: 0,
                        };
                        if let Err(e) = api.create_media_link(tree_id, &body).await {
                            error.set(Some(e.to_string()));
                        }
                        changed.call(());
                        // Open its panel straight away: an empty document is
                        // useless until pages are added, and the panel is
                        // where they are added.
                        editing.set(Some(document.id));
                    }
                    Err(e) => error.set(Some(e.to_string())),
                }
            });
        }
    };

    let items: Vec<MediaWithLink> = match &*tiles.read_unchecked() {
        Some(Ok(items)) => items.clone(),
        Some(Err(e)) => {
            let message = e.to_string();
            return rsx! { div { class: "error-msg", "{message}" } };
        }
        None => Vec::new(),
    };

    let open_tile = editing().and_then(|id| items.iter().find(|t| t.media.id == id).cloned());

    rsx! {
        div { class: if compact { "media-grid media-grid-compact" } else { "media-grid" },
            for tile in items.iter().cloned() {
                MediaTile {
                    key: "{tile.media.id}",
                    tree_id,
                    tile: tile.clone(),
                    show_profile: owner.supports_profile(),
                    person_id: portrait_owner,
                    is_portrait: portrait_media_id == Some(tile.media.id),
                    read_only,
                    profile_event_links: profile_event_links.clone(),
                    is_open: editing() == Some(tile.media.id),
                    on_edit: move |id| {
                        editing.set(if editing() == Some(id) { None } else { Some(id) });
                    },
                    on_crop: move |tile: MediaWithLink| cropping.set(Some(tile)),
                    on_view: move |tile: MediaWithLink| viewing.set(Some(tile)),
                    on_changed: move |_| changed.call(()),
                }
            }
            for vignette in person_vignettes.iter().cloned() {
                VignetteTile {
                    key: "v{vignette.id}",
                    tree_id,
                    vignette: vignette.clone(),
                    person_id: portrait_owner,
                    is_portrait: portrait_vignette_id == Some(vignette.id),
                    on_view: move |tile| viewing.set(Some(tile)),
                    on_changed: move |()| changed.call(()),
                }
            }
            if !compact && !read_only {
                MediaInput {
                    tree_id,
                    on_uploaded: link_uploaded,
                }
            }
            if !compact && !read_only {
                // A document is created empty and then filled: the user says
                // "this is a register" first, and adds its scans afterwards,
                // which is the order the scans come out of a scanner in.
                div { class: "media-drop",
                    button {
                        class: "media-drop-btn",
                        r#type: "button",
                        onclick: new_document,
                        span { class: "media-drop-icon", "\u{1F4DA}" }
                        span { class: "media-drop-label", {use_i18n().t("media.new_document")} }
                        span { class: "media-drop-hint", {use_i18n().t("media.new_document_hint")} }
                    }
                }
            }
        }

        // A reader looking at a person with no photographs should be told so,
        // not left with an empty rectangle that could equally mean "loading".
        if !compact && read_only && items.is_empty() && tiles.read_unchecked().is_some() {
            div { class: "media-empty", {use_i18n().t("media.none")} }
        }

        if let Some(err) = error() {
            div { class: "error-msg", "{err}" }
        }

        if let Some(tile) = open_tile {
            MediaEditPanel {
                tree_id,
                tile,
                events: events.clone(),
                on_changed: move |_| changed.call(()),
                on_close: move |_| editing.set(None),
            }
        }

        if let Some(tile) = viewing() {
            MediaViewer {
                tree_id,
                tile,
                events: events.clone(),
                read_only,
                on_changed: move |()| changed.call(()),
                on_close: move |_| viewing.set(None),
            }
        }

        if let Some(tile) = cropping() {
            CropperHost {
                tree_id,
                tile,
                owner,
                events: events.clone(),
                on_close: move |_| {
                    cropping.set(None);
                    changed.call(());
                },
            }
        }
    }
}

// ── Tile ────────────────────────────────────────────────────────────

#[component]
fn MediaTile(
    tree_id: Uuid,
    tile: MediaWithLink,
    show_profile: bool,
    /// Whose gallery this is, when it is a person's — the portrait is written
    /// on them, not on the link.
    person_id: Option<Uuid>,
    /// Whether this media is currently that person's portrait.
    is_portrait: bool,
    read_only: bool,
    profile_event_links: Vec<MediaEventLinkOption>,
    is_open: bool,
    on_edit: EventHandler<Uuid>,
    on_crop: EventHandler<MediaWithLink>,
    on_view: EventHandler<MediaWithLink>,
    on_changed: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();

    let mut confirming = use_signal(|| false);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut delete_confirming = use_signal(|| false);
    let mut checking_delete = use_signal(|| false);
    let mut deleting = use_signal(|| false);
    let mut delete_error = use_signal(|| None::<String>);
    // Where the right-click menu sits, if it is open.
    let mut menu_at = use_signal(|| None::<(f64, f64)>);
    let mut event_menu = use_signal(|| None::<MediaEventMenu>);
    let mut event_menu_offset = use_signal(|| 0_usize);
    let mut event_link_revision = use_signal(|| 0_u32);

    let media_id = tile.media.id;
    let link_id = tile.link_id;
    let source = tile.source();
    let kind = tile.kind();
    let kind_label = tile.kind_label();
    let caption = tile.caption().to_string();
    let pages = tile.media.page_count;

    let event_links = use_resource({
        let api = api.clone();
        move || {
            let api = api.clone();
            let _ = event_link_revision();
            async move { api.list_media_links_of(tree_id, media_id).await }
        }
    });
    let media_event_ids: Vec<Uuid> = match &*event_links.read_unchecked() {
        Some(Ok(links)) => links.iter().filter_map(|link| link.event_id).collect(),
        _ => Vec::new(),
    };
    let has_event_link = !media_event_ids.is_empty();
    let linked_events: Vec<MediaEventLinkOption> = profile_event_links
        .iter()
        .filter(|event| media_event_ids.contains(&event.event_id))
        .cloned()
        .collect();
    let menu_mode = event_menu();
    let mut menu_events: Vec<MediaEventLinkOption> = menu_mode
        .map(|_| {
            profile_event_links
                .iter()
                .filter(|event| {
                    let linked = linked_events
                        .iter()
                        .any(|linked| linked.event_id == event.event_id);
                    !linked
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    // Dated events come first in their natural chronology; incomplete facts
    // remain available but cannot jump ahead of a known date.
    menu_events.sort_by_key(|event| (event.date_sort.is_none(), event.date_sort));
    let max_event_menu_offset = menu_events.len().saturating_sub(5);
    let current_event_menu_offset = event_menu_offset().min(max_event_menu_offset);
    let visible_menu_events = menu_events
        .iter()
        .skip(current_event_menu_offset)
        .take(5)
        .cloned()
        .collect::<Vec<_>>();

    let remote_preview = (source == MediaSource::Remote && kind == MediaKind::Image)
        .then(|| tile.media.file_path.clone());

    // Called from two places — the hover button and the right-click menu —
    // so it takes no ownership of anything it cannot clone.
    let toggle_profile = use_callback({
        let api = api.clone();
        move |()| {
            let api = api.clone();
            let Some(person_id) = person_id else {
                return;
            };
            spawn(async move {
                busy.set(true);
                // Clearing is sending neither id, which is how "use the
                // silhouette again" is said.
                let body = if is_portrait {
                    SetPortraitBody::default()
                } else {
                    SetPortraitBody {
                        media_id: Some(media_id),
                        vignette_id: None,
                    }
                };
                match api.set_person_portrait(tree_id, person_id, body).await {
                    Ok(_) => on_changed.call(()),
                    Err(e) => error.set(Some(e.to_string())),
                }
                busy.set(false);
            });
        }
    });

    // Detach, not delete: the file may document three other people, and the
    // trash on a person's tile means "not this person's", never "gone".
    let detach = {
        let api = api.clone();
        move |_| {
            let api = api.clone();
            spawn(async move {
                busy.set(true);
                match api.delete_media_link(tree_id, link_id).await {
                    Ok(()) => {
                        confirming.set(false);
                        on_changed.call(());
                    }
                    Err(e) => error.set(Some(e.to_string())),
                }
                busy.set(false);
            });
        }
    };

    let delete_if_unreferenced_elsewhere = {
        let api = api.clone();
        let retained_message = i18n.t("media.delete_kept_referenced");
        move |_| {
            let api = api.clone();
            let retained_message = retained_message.clone();
            spawn(async move {
                deleting.set(true);
                delete_error.set(None);
                match api
                    .delete_media_if_unreferenced_elsewhere(tree_id, media_id, link_id)
                    .await
                {
                    Ok(true) => {
                        delete_confirming.set(false);
                        menu_at.set(None);
                        on_changed.call(());
                    }
                    Ok(false) => {
                        delete_confirming.set(false);
                        error.set(Some(retained_message.clone()));
                    }
                    Err(err) => delete_error.set(Some(err.to_string())),
                }
                deleting.set(false);
            });
        }
    };

    let request_delete_confirmation = {
        let api = api.clone();
        let retained_message = i18n.t("media.delete_kept_referenced");
        move |_| {
            let api = api.clone();
            let retained_message = retained_message.clone();
            spawn(async move {
                checking_delete.set(true);
                error.set(None);
                menu_at.set(None);
                match api
                    .can_delete_media_if_unreferenced_elsewhere(tree_id, media_id, link_id)
                    .await
                {
                    Ok(true) => {
                        delete_error.set(None);
                        delete_confirming.set(true);
                    }
                    Ok(false) => error.set(Some(retained_message)),
                    Err(err) => error.set(Some(err.to_string())),
                }
                checking_delete.set(false);
            });
        }
    };

    let toggle_event_link = use_callback({
        let api = api.clone();
        move |(event_id, attach): (Uuid, bool)| {
            let api = api.clone();
            spawn(async move {
                busy.set(true);
                let result = if attach {
                    api.create_media_link(
                        tree_id,
                        &CreateMediaLinkBody {
                            media_id,
                            person_id: None,
                            event_id: Some(event_id),
                            source_id: None,
                            family_id: None,
                            sort_order: 0,
                        },
                    )
                    .await
                    .map(|_| ())
                } else {
                    match api.list_media_links_of(tree_id, media_id).await {
                        Ok(links) => {
                            match links.iter().find(|link| link.event_id == Some(event_id)) {
                                Some(link) => api.delete_media_link(tree_id, link.id).await,
                                None => Ok(()),
                            }
                        }
                        Err(err) => Err(err),
                    }
                };
                match result {
                    Ok(()) => {
                        event_link_revision += 1;
                        on_changed.call(());
                    }
                    Err(err) => error.set(Some(err.to_string())),
                }
                busy.set(false);
            });
        }
    });

    let tile_for_crop = tile.clone();
    let is_profile = is_portrait;

    rsx! {
        div { class: if is_open { "media-tile is-open" } else { "media-tile" },
            div {
                class: "media-thumb",
                role: "button",
                title: i18n.t("media.view"),
                onclick: {
                    let tile = tile.clone();
                    move |_| on_view.call(tile.clone())
                },
                // The portrait is set from here even on a profile page, where
                // the gallery is otherwise read-only: choosing which
                // photograph represents somebody is the one edit a reader
                // makes while *looking* at their photographs, and sending
                // them to the edit modal to do it means leaving the page that
                // prompted it. Every other action stays behind the modal.
                oncontextmenu: move |e: Event<MouseData>| {
                    if !read_only && !show_profile && profile_event_links.is_empty() {
                        return;
                    }
                    e.prevent_default();
                    let point = e.client_coordinates();
                    event_menu.set(None);
                    event_menu_offset.set(0);
                    menu_at.set(Some((point.x, point.y)));
                },
                if source == MediaSource::Stored && tile.media.thumbnail_key.is_some() {
                    PrivateThumbnail { tree_id, media_id, alt: caption.clone() }
                } else if let Some(preview) = remote_preview.clone() {
                    img { src: "{preview}", alt: "{caption}", loading: "lazy" }
                } else {
                    // An icon that says what the file is, rather than the
                    // broken image an `<img>` onto a 404 would draw.
                    div { class: "media-thumb-icon",
                        span { class: "media-glyph", {kind.icon()} }
                        span { class: "media-kind", "{kind_label}" }
                    }
                }
                if source == MediaSource::Remote {
                    span { class: "media-remote", title: i18n.t("media.source_remote"), "\u{1F517}" }
                }
                if is_portrait {
                    span { class: "media-star", title: i18n.t("media.profile_image"), "\u{2605}" }
                }
                if pages > 1 {
                    span { class: "media-pages",
                        {i18n.t_args("media.page_count", &[("count", &pages.to_string())])}
                    }
                }
                div {
                    class: "media-tile-actions",
                    onclick: move |e| e.stop_propagation(),
                    if !read_only && show_profile && kind == MediaKind::Image {
                        button {
                            class: if is_portrait { "media-act is-on" } else { "media-act" },
                            r#type: "button",
                            disabled: busy(),
                            title: i18n.t("media.set_profile_image"),
                            onclick: move |_| toggle_profile.call(()),
                            "\u{2605}"
                        }
                    }
                    if !read_only && tile.is_croppable() {
                        button {
                            class: "media-act",
                            r#type: "button",
                            title: i18n.t("media.crop"),
                            onclick: move |_| on_crop.call(tile_for_crop.clone()),
                            "\u{2702}"
                        }
                    }
                    if !read_only {
                        button {
                            class: "media-act",
                            r#type: "button",
                            title: i18n.t("common.edit"),
                            onclick: move |_| on_edit.call(media_id),
                            "\u{270E}"
                        }
                    }
                    // No "open the file" link here any more. It navigated
                    // straight to the API's own URL in a new tab, which put
                    // the backend's surface in front of the user for something
                    // the viewer already does better — and the viewer's own
                    // download covers the formats a browser will not render.
                    if !read_only {
                        button {
                            class: "media-act is-danger",
                            r#type: "button",
                            disabled: busy(),
                            title: i18n.t("media.detach"),
                            onclick: move |_| confirming.set(true),
                            "\u{1F5D1}"
                        }
                    }
                }
                if confirming() {
                    div { class: "media-confirm",
                        span { {i18n.t("media.detach_confirm")} }
                        div { class: "media-confirm-actions",
                            button {
                                class: "pf-row-btn is-danger",
                                r#type: "button",
                                disabled: busy(),
                                onclick: detach,
                                {i18n.t("common.confirm")}
                            }
                            button {
                                class: "pf-row-btn",
                                r#type: "button",
                                onclick: move |_| confirming.set(false),
                                {i18n.t("common.cancel")}
                            }
                        }
                    }
                }
            }
            div { class: "media-caption", title: "{caption}", "{caption}" }
            for event in linked_events.iter() {
                div { key: "{event.event_id}", class: "media-event-link",
                    if let Some(date) = &event.date {
                        div { class: "media-event-date", "{date}" }
                    }
                    div { class: "media-event-type", "{event.label}" }
                }
            }
            if let Some(err) = error() {
                div { class: "error-msg", "{err}" }
            }

            if let Some((x, y)) = menu_at() {
                ContextMenuSurface {
                    x,
                    y,
                    menu_class: if menu_mode.is_some() { "context-menu-events".to_string() } else { String::new() },
                    on_close: move |_| menu_at.set(None),
                    if menu_mode.is_some() {
                        button {
                            class: "context-menu-item context-menu-back",
                            r#type: "button",
                            onclick: move |_| {
                                event_menu.set(None);
                                event_menu_offset.set(0);
                            },
                            "\u{2190} {i18n.t(\"common.back\")}"
                        }
                        hr { class: "context-menu-divider" }
                        div { class: "context-menu-event-picker",
                            if current_event_menu_offset > 0 {
                                button {
                                    class: "context-menu-event-scroll",
                                    r#type: "button",
                                    title: i18n.t("media.previous_events"),
                                    aria_label: i18n.t("media.previous_events"),
                                    onclick: move |_| event_menu_offset.set(current_event_menu_offset - 1),
                                    "\u{25B2}"
                                }
                            }
                            div { class: "context-menu-event-list",
                                for event in &visible_menu_events {
                                    {
                                        let event_id = event.event_id;
                                        let label = match &event.date {
                                            Some(date) if !date.is_empty() => {
                                                format!("{date} - {}", event.label)
                                            }
                                            _ => event.label.clone(),
                                        };
                                        rsx! {
                                            button {
                                                key: "{event_id}",
                                                class: "context-menu-item context-menu-event-item",
                                                r#type: "button",
                                                disabled: busy(),
                                                title: "{label}",
                                                onclick: move |_| {
                                                    toggle_event_link.call((event_id, true));
                                                    event_menu.set(None);
                                                    menu_at.set(None);
                                                },
                                                "{label}"
                                            }
                                        }
                                    }
                                }
                            }
                            if current_event_menu_offset < max_event_menu_offset {
                                button {
                                    class: "context-menu-event-scroll",
                                    r#type: "button",
                                    title: i18n.t("media.next_events"),
                                    aria_label: i18n.t("media.next_events"),
                                    onclick: move |_| event_menu_offset.set(current_event_menu_offset + 1),
                                    "\u{25BC}"
                                }
                            }
                        }
                    } else {
                        if show_profile {
                            button {
                                class: "context-menu-item",
                                r#type: "button",
                                disabled: busy(),
                                onclick: move |_| {
                                    menu_at.set(None);
                                    toggle_profile.call(());
                                },
                                if is_profile {
                                    {i18n.t("media.clear_profile_image")}
                                } else {
                                    {i18n.t("media.set_profile_image")}
                                }
                            }
                        }
                        if !profile_event_links.is_empty() && !has_event_link {
                            button {
                                class: "context-menu-item",
                                r#type: "button",
                                onclick: move |_| {
                                    event_menu_offset.set(0);
                                    event_menu.set(Some(MediaEventMenu::Link));
                                },
                                {i18n.t("media.link_event")}
                            }
                        }
                        if let Some(event_id) = media_event_ids.first().copied() {
                            button {
                                class: "context-menu-item context-menu-danger",
                                r#type: "button",
                                onclick: move |_| {
                                    toggle_event_link.call((event_id, false));
                                    menu_at.set(None);
                                },
                                {i18n.t("media.unlink_event")}
                            }
                        }
                        if read_only {
                            button {
                                class: "context-menu-item context-menu-danger",
                                r#type: "button",
                                disabled: checking_delete(),
                                onclick: move |_| {
                                    request_delete_confirmation(());
                                },
                                {i18n.t("media.delete")}
                            }
                        }
                    }
                }
            }
            if delete_confirming() {
                ConfirmDialog {
                    title: i18n.t("media.delete_title"),
                    message: i18n.t("media.delete_message"),
                    confirm_label: i18n.t("media.delete"),
                    error: delete_error(),
                    busy: deleting(),
                    on_confirm: delete_if_unreferenced_elsewhere,
                    on_cancel: move |_| {
                        delete_confirming.set(false);
                        delete_error.set(None);
                    },
                }
            }
        }
    }
}

/// A crop of a larger image, shown as one of the person's pictures.
///
/// A face in a group photograph is one of somebody's pictures as surely as a
/// photograph of them alone is, and it used to appear in no gallery at all —
/// it existed only inside the scan it was drawn on. The image is cropped by
/// the server on read, so this is one `<img>` and no second copy of anything.
///
/// It carries the portrait action and nothing else. Editing the rectangle
/// belongs to the cropper, over the scan, where the coordinates mean
/// something; a tile is too small to move a rectangle on.
#[component]
fn VignetteTile(
    tree_id: Uuid,
    vignette: Vignette,
    person_id: Option<Uuid>,
    is_portrait: bool,
    on_view: EventHandler<MediaWithLink>,
    on_changed: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut menu_at = use_signal(|| None::<(f64, f64)>);

    let vignette_id = vignette.id;
    let caption = i18n.t("media.vignette");

    let toggle_portrait = use_callback({
        let api = api.clone();
        move |()| {
            let api = api.clone();
            let Some(person_id) = person_id else {
                return;
            };
            spawn(async move {
                busy.set(true);
                let body = if is_portrait {
                    SetPortraitBody::default()
                } else {
                    SetPortraitBody {
                        media_id: None,
                        vignette_id: Some(vignette_id),
                    }
                };
                match api.set_person_portrait(tree_id, person_id, body).await {
                    Ok(_) => on_changed.call(()),
                    Err(e) => error.set(Some(e.to_string())),
                }
                busy.set(false);
            });
        }
    });

    let delete_identification = {
        let api = api.clone();
        move |_| {
            let api = api.clone();
            spawn(async move {
                busy.set(true);
                error.set(None);
                match api.delete_vignette(tree_id, vignette_id).await {
                    Ok(()) => {
                        menu_at.set(None);
                        on_changed.call(());
                    }
                    Err(err) => error.set(Some(err.to_string())),
                }
                busy.set(false);
            });
        }
    };

    rsx! {
        div { class: "media-tile",
            div {
                class: "media-thumb is-vignette",
                role: "button",
                title: i18n.t("media.view"),
                // Opening it opens the scan it is a region of: the point of a
                // crop is the document behind it, and a viewer showing the
                // crop alone would hide what it is evidence from.
                onclick: {
                    let api = api.clone();
                    move |_| {
                        let api = api.clone();
                        spawn(async move {
                            if let Ok(media) = api.get_media(tree_id, vignette.media_id).await {
                                on_view.call(MediaWithLink {
                                    link_id: vignette_id,
                                    sort_order: 0,
                                    media,
                                });
                            }
                        });
                    }
                },
                oncontextmenu: move |e: Event<MouseData>| {
                    if person_id.is_none() {
                        return;
                    }
                    e.prevent_default();
                    let point = e.client_coordinates();
                    menu_at.set(Some((point.x, point.y)));
                },
                PrivateVignetteImage {
                    tree_id,
                    vignette_id,
                    alt: caption.clone(),
                }
                if is_portrait {
                    span { class: "media-star", title: i18n.t("media.profile_image"), "\u{2605}" }
                }
                // Says what it is: without it a crop and the whole scan look
                // like two photographs of the same thing.
                span { class: "media-vignette-badge", title: i18n.t("media.vignette"), "\u{2702}" }
            }
            div { class: "media-caption", title: "{caption}", "{caption}" }
            if let Some(err) = error() {
                div { class: "error-msg", "{err}" }
            }

            if let Some((x, y)) = menu_at() {
                ContextMenuSurface {
                    x,
                    y,
                    on_close: move |_| menu_at.set(None),
                    button {
                        class: "context-menu-item",
                        r#type: "button",
                        disabled: busy(),
                        onclick: move |_| {
                            menu_at.set(None);
                            toggle_portrait.call(());
                        },
                        if is_portrait {
                            {i18n.t("media.clear_profile_image")}
                        } else {
                            {i18n.t("media.set_profile_image")}
                        }
                    }
                    button {
                        class: "context-menu-item context-menu-danger",
                        r#type: "button",
                        disabled: busy(),
                        onclick: move |_| {
                            error.set(None);
                            menu_at.set(None);
                            delete_identification(());
                        },
                        {i18n.t("media.delete_identification")}
                    }
                }
            }
        }
    }
}

// ── Inline edit panel ───────────────────────────────────────────────

#[component]
fn MediaEditPanel(
    tree_id: Uuid,
    tile: MediaWithLink,
    events: Vec<(Uuid, String)>,
    /// Rendered inside the viewer's own frame, which already names the file
    /// and offers a way out — so the panel's head would be a second title and
    /// a second close button for the same thing.
    #[props(default)]
    embedded: bool,
    on_changed: EventHandler<()>,
    on_close: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();

    let media_id = tile.media.id;
    let source = tile.source();
    let mut title = use_signal(|| tile.media.title.clone().unwrap_or_default());
    let mut description = use_signal(|| tile.media.description.clone().unwrap_or_default());
    let mut url = use_signal(|| tile.media.file_path.clone());
    let place_id = use_signal(|| {
        tile.media
            .place_id
            .map(|id| id.to_string())
            .unwrap_or_default()
    });
    let date_parts = use_signal(|| {
        DateParts::from_fields(
            tile.media.calendar,
            tile.media.date_qualifier,
            tile.media.date_value.as_deref(),
            tile.media.date_value2.as_deref(),
        )
    });
    let mut privacy = use_signal(|| tile.media.privacy);
    let mut source_media_type = use_signal(|| tile.media.source_media_type);
    let mut document_category = use_signal(|| tile.media.document_category);
    let mut tags = use_signal(|| tile.media.tags.clone());
    let mut show_tag_form = use_signal(|| false);
    let mut note_text = use_signal(String::new);
    let mut note_id = use_signal(|| None::<Uuid>);
    let mut loaded_note = use_signal(|| false);
    let mut saving = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut link_revision = use_signal(|| 0_u32);
    let mut page_revision = use_signal(|| 0_u32);

    let places = use_resource({
        let api = api.clone();
        move || {
            let api = api.clone();
            async move { api.list_all_places(tree_id).await }
        }
    });

    // A note *about the document* — "the left-hand column is water-damaged" —
    // which is not the same thing as the caption under its tile.
    let notes = use_resource({
        let api = api.clone();
        move || {
            let api = api.clone();
            async move {
                api.list_notes(tree_id, None, None, None, None, Some(media_id))
                    .await
            }
        }
    });

    // Which events this file documents. The other direction from the event's
    // own evidence section: the same link row, read from the media end.
    let links = use_resource({
        let api = api.clone();
        move || {
            let api = api.clone();
            let _ = link_revision();
            async move { api.list_media_links_of(tree_id, media_id).await }
        }
    });

    // Seed the note field once, and only once: re-seeding on every render
    // would overwrite what the user is typing.
    if !loaded_note()
        && let Some(Ok(list)) = &*notes.read_unchecked()
    {
        if let Some(first) = list.first() {
            note_text.set(first.text.clone());
            note_id.set(Some(first.id));
        }
        loaded_note.set(true);
    }

    let place_options: Vec<(String, String)> = match &*places.read_unchecked() {
        Some(Ok(places)) => places
            .iter()
            .map(|p| (p.id.to_string(), p.name.clone()))
            .collect(),
        _ => Vec::new(),
    };

    let attached_events: Vec<Uuid> = match &*links.read_unchecked() {
        Some(Ok(list)) => list.iter().filter_map(|l| l.event_id).collect(),
        _ => Vec::new(),
    };

    let add_tag = {
        let api = api.clone();
        move |value: String| {
            if value.is_empty() || tags().iter().any(|tag| tag.eq_ignore_ascii_case(&value)) {
                return;
            }
            show_tag_form.set(false);
            let api = api.clone();
            spawn(async move {
                match api.add_media_tag(tree_id, media_id, value).await {
                    Ok(media) => {
                        tags.set(media.tags);
                        on_changed.call(());
                    }
                    Err(err) => error.set(Some(err.to_string())),
                }
            });
        }
    };

    let remove_tag = use_callback({
        let api = api.clone();
        move |tag: String| {
            let api = api.clone();
            spawn(async move {
                match api.remove_media_tag(tree_id, media_id, tag).await {
                    Ok(()) => {
                        match api.get_media(tree_id, media_id).await {
                            Ok(media) => tags.set(media.tags),
                            Err(err) => error.set(Some(err.to_string())),
                        }
                        on_changed.call(());
                    }
                    Err(err) => error.set(Some(err.to_string())),
                }
            });
        }
    });

    let save = {
        let api = api.clone();
        move |_| {
            let api = api.clone();
            let title_value = title().trim().to_string();
            let description_value = description().trim().to_string();
            let url_value = url().trim().to_string();
            let place_value = Uuid::parse_str(place_id().trim()).ok();
            let note_value = note_text().trim().to_string();
            let privacy_value = privacy();
            let medium_value = source_media_type();
            let category_value = document_category();
            let existing_note = note_id();
            let resolved = date_parts().resolved();
            spawn(async move {
                saving.set(true);
                error.set(None);
                // An emptied field clears the column rather than storing "",
                // so "no title" is one state in the database, not two.
                let body = UpdateMediaBody {
                    title: Some((!title_value.is_empty()).then_some(title_value)),
                    description: Some((!description_value.is_empty()).then_some(description_value)),
                    date_value: Some(resolved.date_value()),
                    date_value2: Some(resolved.date_value2()),
                    date_qualifier: Some(resolved.qualifier),
                    calendar: Some(resolved.calendar),
                    place_id: Some(place_value),
                    // Only a media whose bytes we do not hold owns its path;
                    // the server refuses the field for a stored one, so it is
                    // not even sent.
                    file_path: (source != MediaSource::Stored && !url_value.is_empty())
                        .then_some(url_value),
                    mime_type: None,
                    privacy: Some(privacy_value),
                    source_media_type: Some(medium_value),
                    document_category: Some(category_value),
                };
                let outcome = api.update_media(tree_id, media_id, &body).await;

                // The note is its own row, so it is its own write: created when
                // there was none, deleted when the field is emptied, updated
                // otherwise.
                let note_outcome = match (existing_note, note_value.is_empty()) {
                    (None, true) => Ok(()),
                    (None, false) => api
                        .create_note(
                            tree_id,
                            &CreateNoteBody {
                                text: note_value,
                                person_id: None,
                                event_id: None,
                                family_id: None,
                                source_id: None,
                                media_id: Some(media_id),
                            },
                        )
                        .await
                        .map(|note| note_id.set(Some(note.id))),
                    (Some(id), true) => {
                        let result = api.delete_note(tree_id, id).await;
                        if result.is_ok() {
                            note_id.set(None);
                        }
                        result
                    }
                    (Some(id), false) => api
                        .update_note(
                            tree_id,
                            id,
                            &UpdateNoteBody {
                                text: Some(note_value),
                            },
                        )
                        .await
                        .map(|_| ()),
                };

                match outcome.map(|_| ()).and(note_outcome) {
                    Ok(()) => {
                        on_changed.call(());
                        on_close.call(());
                    }
                    Err(e) => error.set(Some(e.to_string())),
                }
                saving.set(false);
            });
        }
    };

    // Copy-able so each checkbox row can take its own handle; `ApiClient` is
    // behind an `Rc`, and the closure only reads signals.
    let toggle_event = use_callback({
        let api = api.clone();
        move |(event_id, attach): (Uuid, bool)| {
            let api = api.clone();
            spawn(async move {
                let result = if attach {
                    api.create_media_link(
                        tree_id,
                        &CreateMediaLinkBody {
                            media_id,
                            person_id: None,
                            event_id: Some(event_id),
                            source_id: None,
                            family_id: None,
                            sort_order: 0,
                        },
                    )
                    .await
                    .map(|_| ())
                } else {
                    // Find the link to remove: an event may be documented by
                    // several files, so the media id alone is not enough.
                    match api.list_media_links_of(tree_id, media_id).await {
                        Ok(list) => match list.iter().find(|l| l.event_id == Some(event_id)) {
                            Some(link) => api.delete_media_link(tree_id, link.id).await,
                            None => Ok(()),
                        },
                        Err(e) => Err(e),
                    }
                };
                if let Err(e) = result {
                    error.set(Some(e.to_string()));
                }
                link_revision += 1;
            });
        }
    });

    let dimensions = match (tile.media.width, tile.media.height) {
        (Some(w), Some(h)) => Some(format!("{w} × {h}")),
        _ => None,
    };

    rsx! {
        div { class: if embedded { "media-panel is-embedded" } else { "media-panel" },
            if !embedded {
                div { class: "media-panel-head",
                    span { class: "media-panel-title", "{tile.media.file_name}" }
                    button {
                        class: "cropper-close",
                        r#type: "button",
                        onclick: move |_| on_close.call(()),
                        "\u{00D7}"
                    }
                }
            }

            div { class: "media-panel-meta",
                span { "{tile.kind_label()}" }
                span {
                    {match source {
                        MediaSource::Stored => i18n.t("media.source_stored"),
                        MediaSource::Remote => i18n.t("media.source_remote"),
                        MediaSource::Unheld => i18n.t("media.source_unheld"),
                    }}
                }
                if let Some(dimensions) = dimensions {
                    span { "{dimensions}" }
                }
                if tile.media.file_size > 0 {
                    span { {format_size(tile.media.file_size)} }
                }
                if tile.media.page_count > 1 {
                    span {
                        {i18n.t_args(
                            "media.page_count",
                            &[("count", &tile.media.page_count.to_string())],
                        )}
                    }
                }
            }

            // Only a media we do not store owns its path. For a stored one the
            // path is the GEDCOM value an export writes back, and repointing it
            // would make the export describe a file we are not serving.
            if source != MediaSource::Stored {
                div { class: "form-group",
                    label { {i18n.t("media.url")} }
                    input {
                        r#type: "text",
                        value: "{url}",
                        placeholder: "https://\u{2026}",
                        oninput: move |e: Event<FormData>| url.set(e.value()),
                    }
                    p { class: "pf-ns-hint", {i18n.t("media.url_hint")} }
                }
            }

            div { class: "form-group",
                label { {i18n.t("media.title")} }
                input {
                    r#type: "text",
                    value: "{title}",
                    oninput: move |e: Event<FormData>| title.set(e.value()),
                }
            }
            div { class: "form-group",
                label { {i18n.t("media.description")} }
                textarea {
                    rows: 3,
                    value: "{description}",
                    oninput: move |e: Event<FormData>| description.set(e.value()),
                }
            }
            div { class: "pf-subblock media-tags-editor",
                div { class: "pf-block-label",
                    button {
                        class: if show_tag_form() { "pf-add-btn is-open" } else { "pf-add-btn" },
                        r#type: "button",
                        onclick: move |_| {
                            let opening = !show_tag_form();
                            show_tag_form.set(opening);
                        },
                        {i18n.t("media.add_tag")}
                    }
                }
                if show_tag_form() {
                    MediaTagForm { on_add: add_tag }
                }
                if !tags().is_empty() {
                    div { class: "media-fact-tags media-edit-tags",
                        for tag in tags().iter() {
                            {
                                let tag = tag.clone();
                                rsx! {
                                    span { key: "{tag}", class: "media-fact-tag is-editable",
                                        "{tag}"
                                        button {
                                            class: "media-tag-remove",
                                            r#type: "button",
                                            title: i18n.t("common.delete"),
                                            onclick: move |_| remove_tag(tag.clone()),
                                            "\u{00D7}"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Two fields for what looks like one question, because it is two.
            //
            // The category is what a user can actually answer about a scan —
            // "this is a census return" — and is the one they will reach for.
            // The medium is GEDCOM's own vocabulary, which has no word for a
            // census return and calls it a manuscript; it is what an export
            // writes, and other genealogy software reads. Leaving the medium
            // alone lets the category decide it, which is why the placeholder
            // says so rather than reading as an empty required field.
            div { class: "form-group",
                label { {i18n.t("media.document_category")} }
                select {
                    class: "td-select",
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

            // Recorded, not yet enforced — the hint says so rather than
            // letting the control imply a protection that does not exist.
            div { class: "form-group",
                label { {i18n.t("media.privacy")} }
                select {
                    class: "td-select",
                    onchange: move |e: Event<FormData>| {
                        privacy.set(parse_privacy(&e.value()));
                    },
                    for (value , label) in [
                        ("Default", i18n.t("privacy.default")),
                        ("Public", i18n.t("privacy.public")),
                        ("Private", i18n.t("privacy.private")),
                    ] {
                        option {
                            key: "{value}",
                            value: "{value}",
                            selected: format!("{:?}", privacy()) == value,
                            "{label}"
                        }
                    }
                }
                p { class: "pf-ns-hint", {i18n.t("privacy.not_enforced_yet")} }
            }

            // The same date widget every fact uses, so a photograph taken
            // "around 1890" is written the way a birth around 1890 is.
            div { class: "form-group",
                label { {i18n.t("media.date")} }
                DateInput { parts: date_parts, i18n, on_change: move |()| {} }
            }

            // The shared picker, not a hand-rolled select: it sets `selected`
            // on each option rather than `value` on the element, which is what
            // makes a stored place actually show as chosen when the list is
            // built by a loop.
            {render_place_select(&i18n, place_id, &place_options, || {})}

            div { class: "form-group",
                label { {i18n.t("media.note")} }
                textarea {
                    rows: 3,
                    value: "{note_text}",
                    placeholder: i18n.t("media.note_placeholder"),
                    oninput: move |e: Event<FormData>| note_text.set(e.value()),
                }
            }

            // No source field, and that is deliberate: a media *is* a source
            // document. Asking which source backs a scan of a parish register
            // asks it to cite itself.

            if !events.is_empty() {
                div { class: "media-panel-section",
                    label { {i18n.t("media.documents_events")} }
                    div { class: "media-events",
                        for (id, label) in events.iter() {
                            {
                                let event_id = *id;
                                let attached = attached_events.contains(&event_id);
                                rsx! {
                                    label { key: "{event_id}", class: "media-event-row",
                                        input {
                                            r#type: "checkbox",
                                            checked: attached,
                                            onchange: move |e: Event<FormData>| {
                                                toggle_event.call((event_id, e.checked()));
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

            // A document's pages. Its own upload cell, because a page is
            // uploaded *into* the document rather than into the gallery.
            if tile.media.is_document {
                div { class: "media-panel-section",
                    label { {i18n.t("media.pages")} }
                    p { class: "pf-ns-hint", {i18n.t("media.pages_hint")} }
                    DocumentPages {
                        tree_id,
                        document_id: media_id,
                        on_changed: move |_| {
                            page_revision += 1;
                            on_changed.call(());
                        },
                    }
                }
            }

            if let Some(err) = error() {
                div { class: "error-msg", "{err}" }
            }
            div { class: "media-panel-actions",
                button {
                    class: "btn btn-outline",
                    r#type: "button",
                    disabled: saving(),
                    onclick: move |_| on_close.call(()),
                    {i18n.t("common.cancel")}
                }
                button {
                    class: "pf-confirm-btn",
                    r#type: "button",
                    disabled: saving(),
                    onclick: save,
                    if saving() { {i18n.t("common.saving")} } else { {i18n.t("common.save")} }
                }
            }
        }
    }
}

// ── Document pages ──────────────────────────────────────────────────

/// The page strip of a multi-page document, with its own upload cell.
///
/// Pages are ordinary media rows — they have bytes, thumbnails and crops — so
/// this shows them, lets them be moved and lets them be detached, and nothing
/// here duplicates the upload or thumbnail machinery.
#[component]
fn DocumentPages(tree_id: Uuid, document_id: Uuid, on_changed: EventHandler<()>) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let mut revision = use_signal(|| 0_u32);
    let mut error = use_signal(|| None::<String>);

    let pages = use_resource({
        let api = api.clone();
        move || {
            let api = api.clone();
            let _ = revision();
            async move { api.list_media_pages(tree_id, document_id).await }
        }
    });

    let list: Vec<oxidgene_core::types::Media> = match &*pages.read_unchecked() {
        Some(Ok(list)) => list.clone(),
        _ => Vec::new(),
    };

    // Moving a page sends the whole order, not a "move up" operation: the
    // server applies it as one list, so a failure cannot leave the pages half
    // reordered.
    let move_page = use_callback({
        let api = api.clone();
        let ids: Vec<Uuid> = list.iter().map(|p| p.id).collect();
        move |(index, delta): (usize, isize)| {
            let api = api.clone();
            let mut ids = ids.clone();
            let target = index as isize + delta;
            if target < 0 || target as usize >= ids.len() {
                return;
            }
            ids.swap(index, target as usize);
            spawn(async move {
                match api.reorder_media_pages(tree_id, document_id, &ids).await {
                    Ok(_) => {
                        revision += 1;
                        on_changed.call(());
                    }
                    Err(e) => error.set(Some(e.to_string())),
                }
            });
        }
    });

    let detach = use_callback({
        let api = api.clone();
        move |page_id: Uuid| {
            let api = api.clone();
            spawn(async move {
                match api.detach_media_page(tree_id, document_id, page_id).await {
                    Ok(_) => {
                        revision += 1;
                        on_changed.call(());
                    }
                    Err(e) => error.set(Some(e.to_string())),
                }
            });
        }
    });

    let total = list.len();

    rsx! {
        div { class: "doc-pages",
            for (index, page) in list.iter().enumerate() {
                {
                    let page_id = page.id;
                    let has_thumbnail = page.thumbnail_key.is_some();
                    let name = page.file_name.clone();
                    rsx! {
                        div { key: "{page_id}", class: "doc-page",
                            span { class: "doc-page-number", "{index + 1}" }
                            div { class: "doc-page-thumb",
                                if has_thumbnail {
                                    PrivateThumbnail { tree_id, media_id: page_id, alt: name }
                                } else {
                                    span { class: "media-glyph", "\u{1F4C4}" }
                                }
                            }
                            div { class: "doc-page-actions",
                                button {
                                    class: "pf-row-btn",
                                    r#type: "button",
                                    disabled: index == 0,
                                    title: i18n.t("media.page_move_up"),
                                    onclick: move |_| move_page.call((index, -1)),
                                    "\u{2191}"
                                }
                                button {
                                    class: "pf-row-btn",
                                    r#type: "button",
                                    disabled: index + 1 >= total,
                                    title: i18n.t("media.page_move_down"),
                                    onclick: move |_| move_page.call((index, 1)),
                                    "\u{2193}"
                                }
                                button {
                                    class: "pf-row-btn is-danger",
                                    r#type: "button",
                                    title: i18n.t("media.page_detach"),
                                    onclick: move |_| detach.call(page_id),
                                    "\u{2715}"
                                }
                            }
                        }
                    }
                }
            }

            MediaInput {
                tree_id,
                document_id,
                label: i18n.t("media.add_pages"),
                on_uploaded: move |_| {},
                on_batch_done: move |_| {
                    revision += 1;
                    on_changed.call(());
                },
            }
        }
        if let Some(err) = error() {
            div { class: "error-msg", "{err}" }
        }
    }
}

// ── Viewer ──────────────────────────────────────────────────────────

/// Full-size view of one media.
///
/// What "view" means depends on the file. An image, a video and an audio track
/// each have an element that takes a URL and plays it, so those are shown
/// where the reader is. Everything else — a PDF, a document, an archive — is
/// offered as a download instead of embedded, because an `<img>` or an
/// `<object>` onto one gives the reader a blank rectangle and no way to tell
/// whether that is the file or a failure.
///
/// This is the same overlay for a stored file and a remote URL; the only
/// difference is which URL goes into the element, which the tile has already
/// decided.
/// Everything known about a media, as prose rather than as a form.
///
/// The viewer's companion column. A scan is worth opening precisely because
/// of what is written about it — when it was taken, where, which events it
/// documents, who is identified on it — and until now that lived only inside
/// an edit form, which a reader on a profile page never sees because the
/// gallery there is read-only.
///
/// Fields with nothing in them are omitted rather than shown empty: a column
/// of eight labels reading "—" tells the reader less than four that say
/// something, and makes the ones that do harder to find.
#[component]
fn MediaFacts(
    tree_id: Uuid,
    media: oxidgene_core::types::Media,
    attachment_media_id: Uuid,
    attachment_revision: u32,
    tags: Vec<String>,
    vignettes: Vec<Vignette>,
    person_names: Vec<PersonName>,
    events: Vec<(Uuid, String)>,
    on_vignettes_changed: EventHandler<()>,
    on_vignette_hover: EventHandler<Option<Uuid>>,
    on_changed: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let media_id = media.id;
    let notes = use_resource({
        let api = api.clone();
        move || {
            let api = api.clone();
            async move {
                api.list_notes(tree_id, None, None, None, None, Some(media_id))
                    .await
                    .ok()
            }
        }
    });
    let places = use_resource({
        let api = api.clone();
        move || {
            let api = api.clone();
            async move { api.list_all_places(tree_id).await.ok() }
        }
    });
    let date = format_date(
        &i18n,
        media.calendar,
        media.date_qualifier,
        media.date_value.as_deref(),
        media.date_value2.as_deref(),
    );
    let place = media.place_id.and_then(|id| {
        places
            .read_unchecked()
            .as_ref()?
            .as_ref()?
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.name.clone())
    });
    let note = notes
        .read_unchecked()
        .as_ref()
        .and_then(|n| n.as_ref())
        .and_then(|list| list.first().map(|n| n.text.clone()));
    // Every field is listed, set or not. Omitting the empty ones was tidier
    // and told the reader nothing: a scan with no date looked identical to a
    // viewer that could not record one, so the feature read as missing rather
    // than as unfilled. An em-dash is an invitation; an absent row is not.
    //
    // There is deliberately no `source` row. A media *is* a source document —
    // asking which source backs a scan of a parish register asks it to cite
    // itself — so the field does not exist to be shown.
    rsx! {
        div { class: "media-facts",
            MediaFact { label: i18n.t("media.title"), value: media.title.clone() }
            if !tags.is_empty() {
                div { class: "form-group media-fact",
                    label { {i18n.t("media.tags")} }
                    div { class: "media-fact-tags",
                        for tag in tags.iter() {
                            span { key: "{tag}", class: "media-fact-tag", "{tag}" }
                        }
                    }
                }
            }
            MediaFact {
                label: i18n.t("media.created_at"),
                value: Some(media.created_at.format("%Y-%m-%d %H:%M UTC").to_string()),
            }
            MediaFact { label: i18n.t("media.date"), value: (!date.is_empty()).then_some(date) }
            MediaFact { label: i18n.t("media.place"), value: place }
            MediaFact {
                label: i18n.t("media.document_category"),
                value: media
                    .document_category
                    .map(|c| i18n.t(&format!("media.category.{}", c.as_str()))),
            }
            MediaFact {
                label: i18n.t("media.source_media_type"),
                value: Some(i18n.t(&format!("media.medium.{}", media.source_media_type.as_str()))),
            }
            MediaFact {
                label: i18n.t("media.privacy"),
                value: Some(i18n.t(match media.privacy {
                    Privacy::Default => "privacy.default",
                    Privacy::Public => "privacy.public",
                    Privacy::Private => "privacy.private",
                })),
            }
            MediaFact {
                label: i18n.t("media.description"),
                value: media.description.clone(),
                prose: true,
            }
            MediaFact { label: i18n.t("media.note"), value: note, prose: true }

            MediaRelations {
                key: "{attachment_revision}",
                tree_id,
                media_id: attachment_media_id,
                vignettes,
                person_names,
                on_attachments_changed: on_changed,
                on_identifications_changed: on_vignettes_changed,
                on_identification_hover: on_vignette_hover,
            }

            if !events.is_empty() {
                MediaEventLinks {
                    tree_id,
                    media_id,
                    events,
                    on_changed,
                }
            }

            // The technical facts last, and quietly: they answer "is this the
            // good scan or the phone snapshot", which is a real question, but
            // not the one the reader came with.
            div { class: "media-fact-tech",
                span {
                    {
                        media
                            .mime_type
                            .rsplit('/')
                            .next()
                            .unwrap_or("file")
                            .trim_start_matches("x-")
                            .split('+')
                            .next()
                            .unwrap_or("file")
                            .to_uppercase()
                    }
                }
                if let (Some(w), Some(h)) = (media.width, media.height) {
                    span { "{w} \u{00D7} {h}" }
                }
                if media.file_size > 0 {
                    span { {format_size(media.file_size)} }
                }
            }
        }
    }
}

#[derive(Clone)]
enum MediaRelation {
    PersonAttachment {
        link_id: Uuid,
        person_id: Uuid,
        name: String,
    },
    CoupleAttachment {
        link_id: Uuid,
        family_id: Uuid,
        label: String,
    },
    Identification {
        vignette_id: Uuid,
        name: Option<String>,
    },
}

#[component]
fn MediaRelations(
    tree_id: Uuid,
    media_id: Uuid,
    vignettes: Vec<Vignette>,
    person_names: Vec<PersonName>,
    on_attachments_changed: EventHandler<()>,
    on_identifications_changed: EventHandler<()>,
    on_identification_hover: EventHandler<Option<Uuid>>,
) -> Element {
    const ROWS_PER_PAGE: usize = 5;

    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let mut revision = use_signal(|| 0_u32);
    let mut relation_offset = use_signal(|| 0_usize);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let data = use_resource({
        let api = api.clone();
        move || {
            let api = api.clone();
            let _ = revision();
            async move {
                let links = api.list_media_links_of(tree_id, media_id).await.ok()?;
                let snapshot = api.get_tree_snapshot(tree_id).await.ok()?;
                Some((links, snapshot))
            }
        }
    });
    let data = data.read_unchecked();
    let mut relations = Vec::new();
    if let Some(Some((links, snapshot))) = data.as_ref() {
        for link in links.iter().filter(|link| link.person_id.is_some()) {
            let person_id = link.person_id.expect("filtered person link");
            if relations.iter().any(|relation| {
                matches!(
                    relation,
                    MediaRelation::PersonAttachment {
                        person_id: attached_id,
                        ..
                    } if *attached_id == person_id
                )
            }) {
                continue;
            }
            if let Some(name) = primary_person_name(&snapshot.names, person_id) {
                relations.push(MediaRelation::PersonAttachment {
                    link_id: link.id,
                    person_id,
                    name,
                });
            }
        }

        for link in links.iter().filter(|link| link.family_id.is_some()) {
            let family_id = link.family_id.expect("filtered family link");
            if relations.iter().any(|relation| {
                matches!(
                    relation,
                    MediaRelation::CoupleAttachment {
                        family_id: attached_id,
                        ..
                    } if *attached_id == family_id
                )
            }) {
                continue;
            }
            let spouse_names = snapshot
                .spouses
                .iter()
                .filter(|spouse| spouse.family_id == family_id)
                .filter_map(|spouse| primary_person_name(&snapshot.names, spouse.person_id))
                .collect::<Vec<_>>();
            let people = if spouse_names.is_empty() {
                i18n.t("media.attach_unknown_spouse")
            } else {
                spouse_names.join(" & ")
            };
            relations.push(MediaRelation::CoupleAttachment {
                link_id: link.id,
                family_id,
                label: i18n.t_args("media.attached_couple", &[("people", &people)]),
            });
        }
    }

    relations.extend(
        vignettes
            .iter()
            .filter(|vignette| vignette.person_id.is_some())
            .map(|vignette| MediaRelation::Identification {
                vignette_id: vignette.id,
                name: vignette
                    .person_id
                    .and_then(|person_id| primary_person_name(&person_names, person_id)),
            }),
    );

    if relations.is_empty() {
        return rsx! {};
    }

    let max_relation_offset = relations.len().saturating_sub(ROWS_PER_PAGE);
    let current_relation_offset = relation_offset().min(max_relation_offset);
    let visible_relations = relations
        .into_iter()
        .skip(current_relation_offset)
        .take(ROWS_PER_PAGE)
        .collect::<Vec<_>>();

    let delete_attachment = use_callback({
        let api = api.clone();
        move |link_id: Uuid| {
            let api = api.clone();
            spawn(async move {
                busy.set(true);
                error.set(None);
                match api.delete_media_link(tree_id, link_id).await {
                    Ok(()) => {
                        revision += 1;
                        on_attachments_changed.call(());
                    }
                    Err(err) => error.set(Some(err.to_string())),
                }
                busy.set(false);
            });
        }
    });

    let delete_identification = use_callback({
        let api = api.clone();
        move |vignette_id: Uuid| {
            let api = api.clone();
            spawn(async move {
                busy.set(true);
                error.set(None);
                match api.delete_vignette(tree_id, vignette_id).await {
                    Ok(_) => on_identifications_changed.call(()),
                    Err(err) => error.set(Some(err.to_string())),
                }
                busy.set(false);
            });
        }
    });

    rsx! {
        div { class: "form-group media-fact is-relations",
            label { {i18n.t("media.relations")} }
            div { class: "media-relations",
                div { class: "media-relation-list",
                    for relation in visible_relations {
                        match relation {
                            MediaRelation::PersonAttachment { link_id, person_id, name } => rsx! {
                                div { key: "person-{person_id}", class: "media-vignette-item",
                                    PrivateThumbnail {
                                        tree_id,
                                        media_id,
                                        alt: String::new(),
                                        class: "media-vignette-thumbnail",
                                    }
                                    Link {
                                        to: Route::PersonDetail {
                                            tree_id: tree_id.to_string(),
                                            person_id: person_id.to_string(),
                                        },
                                        class: "media-identification-person",
                                        "{name}"
                                    }
                                    button {
                                        class: "media-identification-delete",
                                        r#type: "button",
                                        disabled: busy(),
                                        title: i18n.t("media.delete_attachment"),
                                        onclick: move |_| delete_attachment.call(link_id),
                                        "\u{00D7}"
                                    }
                                }
                            },
                            MediaRelation::CoupleAttachment { link_id, family_id, label } => rsx! {
                                div { key: "family-{family_id}", class: "media-vignette-item",
                                    PrivateThumbnail {
                                        tree_id,
                                        media_id,
                                        alt: String::new(),
                                        class: "media-vignette-thumbnail",
                                    }
                                    span { class: "media-attachment-couple", "{label}" }
                                    button {
                                        class: "media-identification-delete",
                                        r#type: "button",
                                        disabled: busy(),
                                        title: i18n.t("media.delete_attachment"),
                                        onclick: move |_| delete_attachment.call(link_id),
                                        "\u{00D7}"
                                    }
                                }
                            },
                            MediaRelation::Identification { vignette_id, name } => rsx! {
                                div {
                                    key: "vignette-{vignette_id}",
                                    class: "media-identification",
                                    onpointerenter: move |_| on_identification_hover.call(Some(vignette_id)),
                                    onpointerleave: move |_| on_identification_hover.call(None),
                                    div { class: "media-vignette-item",
                                        div { class: "media-identification-target",
                                            PrivateVignetteImage {
                                                tree_id,
                                                vignette_id,
                                                alt: String::new(),
                                                class: "media-vignette-thumbnail",
                                            }
                                            if let Some(name) = name.as_ref() {
                                                span { class: "media-identification-person", "{name}" }
                                            }
                                        }
                                        if name.is_some() {
                                            button {
                                                class: "media-identification-delete",
                                                r#type: "button",
                                                disabled: busy(),
                                                title: i18n.t("media.delete_identification"),
                                                onclick: move |_| delete_identification.call(vignette_id),
                                                "\u{00D7}"
                                            }
                                        }
                                    }
                                }
                            },
                        }
                    }
                }
                if max_relation_offset > 0 {
                    div { class: "media-relation-pager",
                        button {
                            class: "media-relation-page-button",
                            r#type: "button",
                            disabled: current_relation_offset == 0,
                            title: i18n.t("media.previous_relations"),
                            aria_label: i18n.t("media.previous_relations"),
                            onclick: move |_| relation_offset.set(current_relation_offset.saturating_sub(1)),
                            svg {
                                width: "16", height: "16", fill: "none", "viewBox": "0 0 24 24",
                                stroke: "currentColor", "strokeWidth": "2",
                                "strokeLinecap": "round", "strokeLinejoin": "round",
                                path { d: "m18 15-6-6-6 6" }
                            }
                        }
                        button {
                            class: "media-relation-page-button",
                            r#type: "button",
                            disabled: current_relation_offset >= max_relation_offset,
                            title: i18n.t("media.next_relations"),
                            aria_label: i18n.t("media.next_relations"),
                            onclick: move |_| relation_offset.set(
                                (current_relation_offset + 1).min(max_relation_offset),
                            ),
                            svg {
                                width: "16", height: "16", fill: "none", "viewBox": "0 0 24 24",
                                stroke: "currentColor", "strokeWidth": "2",
                                "strokeLinecap": "round", "strokeLinejoin": "round",
                                path { d: "m6 9 6 6 6-6" }
                            }
                        }
                    }
                }
            }
            if let Some(message) = error() {
                div { class: "error-msg", "{message}" }
            }
        }
    }
}

/// The media-to-event links shown in the reader. The `media_id` is always
/// that of the opened media, so a document's pages are never linked alone.
#[component]
fn MediaEventLinks(
    tree_id: Uuid,
    media_id: Uuid,
    events: Vec<(Uuid, String)>,
    on_changed: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let mut revision = use_signal(|| 0_u32);
    let mut error = use_signal(|| None::<String>);
    let links = use_resource({
        let api = api.clone();
        move || {
            let api = api.clone();
            let _ = revision();
            async move { api.list_media_links_of(tree_id, media_id).await.ok() }
        }
    });
    let attached: Vec<Uuid> = links
        .read_unchecked()
        .as_ref()
        .and_then(|links| links.clone())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|link| link.event_id)
        .collect();
    let toggle = use_callback({
        let api = api.clone();
        move |(event_id, attach): (Uuid, bool)| {
            let api = api.clone();
            spawn(async move {
                let result = if attach {
                    api.create_media_link(
                        tree_id,
                        &CreateMediaLinkBody {
                            media_id,
                            person_id: None,
                            event_id: Some(event_id),
                            source_id: None,
                            family_id: None,
                            sort_order: 0,
                        },
                    )
                    .await
                    .map(|_| ())
                } else {
                    match api.list_media_links_of(tree_id, media_id).await {
                        Ok(links) => {
                            match links.iter().find(|link| link.event_id == Some(event_id)) {
                                Some(link) => api.delete_media_link(tree_id, link.id).await,
                                None => Ok(()),
                            }
                        }
                        Err(err) => Err(err),
                    }
                };
                match result {
                    Ok(()) => {
                        revision += 1;
                        on_changed.call(());
                    }
                    Err(err) => error.set(Some(err.to_string())),
                }
            });
        }
    });

    rsx! {
        div { class: "form-group media-fact",
            label { {i18n.t("media.documents_events")} }
            div { class: "media-events",
                for (event_id, label) in events.iter() {
                    {
                        let event_id = *event_id;
                        let attached = attached.contains(&event_id);
                        rsx! {
                            label { key: "{event_id}", class: "media-event-row",
                                input {
                                    r#type: "checkbox",
                                    checked: attached,
                                    onchange: move |event: Event<FormData>| toggle.call((event_id, event.checked())),
                                }
                                span { "{label}" }
                            }
                        }
                    }
                }
            }
            if let Some(message) = error() {
                div { class: "error-msg", "{message}" }
            }
        }
    }
}

/// One read-only field, using the same label/value structure as the forms.
#[component]
fn MediaFact(label: String, value: Option<String>, #[props(default)] prose: bool) -> Element {
    let filled = value.as_ref().is_some_and(|v| !v.trim().is_empty());
    rsx! {
        div { class: if prose { "form-group media-fact is-prose" } else { "form-group media-fact" },
            label { "{label}" }
            div {
                class: if filled { "media-fact-value" } else { "media-fact-value is-empty" },
                match value.filter(|v| !v.trim().is_empty()) {
                    Some(value) => value,
                    None => "\u{2014}".to_string(),
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
enum MediaZoomAnchor {
    Center,
    Pointer(f64, f64),
}

#[derive(Clone, Copy, PartialEq)]
enum MediaAttachmentMode {
    Person,
    CouplePerson,
    CoupleFamily,
}

#[derive(Clone, PartialEq)]
struct MediaFamilyChoice {
    family_id: Uuid,
    label: String,
}

#[derive(Clone, Copy)]
enum MediaAttachmentTarget {
    Person(Uuid),
    Family(Uuid),
}

async fn attach_media_to(
    api: &ApiClient,
    tree_id: Uuid,
    media_id: Uuid,
    target: MediaAttachmentTarget,
) -> Result<bool, ApiError> {
    let links = api.list_media_links_of(tree_id, media_id).await?;
    let already_linked = links.iter().any(|link| match target {
        MediaAttachmentTarget::Person(person_id) => link.person_id == Some(person_id),
        MediaAttachmentTarget::Family(family_id) => link.family_id == Some(family_id),
    });
    if already_linked {
        return Ok(false);
    }

    let body = CreateMediaLinkBody {
        media_id,
        person_id: match target {
            MediaAttachmentTarget::Person(person_id) => Some(person_id),
            MediaAttachmentTarget::Family(_) => None,
        },
        family_id: match target {
            MediaAttachmentTarget::Family(family_id) => Some(family_id),
            MediaAttachmentTarget::Person(_) => None,
        },
        event_id: None,
        source_id: None,
        sort_order: 0,
    };
    api.create_media_link(tree_id, &body).await?;
    Ok(true)
}

fn vignette_overlay_style(vignette: &Vignette, width: Option<i32>, height: Option<i32>) -> String {
    let width = width.unwrap_or(1).max(1) as f64;
    let height = height.unwrap_or(1).max(1) as f64;
    format!(
        "left:{}%;top:{}%;width:{}%;height:{}%",
        vignette.x as f64 * 100.0 / width,
        vignette.y as f64 * 100.0 / height,
        vignette.width as f64 * 100.0 / width,
        vignette.height as f64 * 100.0 / height,
    )
}

fn primary_person_name_record(names: &[PersonName], person_id: Uuid) -> Option<&PersonName> {
    names
        .iter()
        .find(|name| name.person_id == person_id && name.is_primary)
        .or_else(|| names.iter().find(|name| name.person_id == person_id))
}

fn primary_person_name(names: &[PersonName], person_id: Uuid) -> Option<String> {
    primary_person_name_record(names, person_id)
        .map(PersonName::display_name)
        .filter(|name| !name.is_empty())
}

#[component]
fn MediaViewer(
    tree_id: Uuid,
    tile: MediaWithLink,
    events: Vec<(Uuid, String)>,
    read_only: bool,
    on_changed: EventHandler<()>,
    on_close: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    // The companion column starts as prose. A reader opened this to look at
    // the document, not to fill in a form, so the form is a step they take.
    let mut editing = use_signal(|| false);
    let mut identifying = use_signal(|| false);
    let mut highlighted_vignette = use_signal(|| None::<Uuid>);
    let mut attachment_mode = use_signal(|| None::<MediaAttachmentMode>);
    let mut attachment_busy = use_signal(|| false);
    let mut attachment_error = use_signal(|| None::<String>);
    let mut attachment_notice = use_signal(|| None::<String>);
    let mut attachment_revision = use_signal(|| 0_u32);
    let mut family_choices = use_signal(Vec::<MediaFamilyChoice>::new);
    let mut media_revision = use_signal(|| 0_u32);
    let mut delete_confirming = use_signal(|| false);
    let mut deleting = use_signal(|| false);
    let mut delete_error = use_signal(|| None::<String>);

    let delete_media = {
        let api = api.clone();
        let media_id = tile.media.id;
        move |_| {
            let api = api.clone();
            spawn(async move {
                deleting.set(true);
                delete_error.set(None);
                match api.delete_media(tree_id, media_id).await {
                    Ok(()) => {
                        delete_confirming.set(false);
                        on_changed.call(());
                        on_close.call(());
                    }
                    Err(err) => delete_error.set(Some(err.to_string())),
                }
                deleting.set(false);
            });
        }
    };

    // `viewing` owns the tile that opened the overlay, so it does not change
    // when the gallery refreshes underneath it. Reload the media itself after
    // an edit to keep the read-only facts (especially tags) current.
    let current_media = use_resource({
        let api = api.clone();
        let media_id = tile.media.id;
        move || {
            let api = api.clone();
            let _ = media_revision();
            async move { api.get_media(tree_id, media_id).await.ok() }
        }
    });
    let current_media = match &*current_media.read_unchecked() {
        Some(Some(media)) => media.clone(),
        _ => tile.media.clone(),
    };
    let mut current_tile = tile.clone();
    current_tile.media = current_media.clone();

    let source = current_tile.source();
    let caption = current_tile.caption().to_string();
    let is_document = current_tile.media.is_document;
    let mut page = use_signal(|| 0_usize);
    // Zoom as a percentage of the fitted size; `None` is exactly fitted. This
    // mirrors the pedigree's multiplicative zoom without sacrificing the
    // scrollbars a large scan needs.
    let mut zoom = use_signal(|| None::<u32>);
    let mut dragging_image = use_signal(|| false);
    let mut drag_start_x = use_signal(|| 0.0_f64);
    let mut drag_start_y = use_signal(|| 0.0_f64);
    let mut wheel_zooming = use_signal(|| false);
    let mut fitted_size = use_signal(|| None::<(f64, f64)>);
    let mut zoom_overflow = use_signal(|| (false, false));
    let stage_id = format!("media-viewer-stage-{}", tile.media.id);

    let fit_image = use_callback({
        let stage_id = stage_id.clone();
        move |()| {
            let stage_id = stage_id.clone();
            spawn(async move {
                let script = format!(
                    r#"
                    const stage = document.getElementById('{stage_id}');
                    const image = stage?.querySelector('.media-viewer-image');
                    if (!stage || !image || !image.naturalWidth || !image.naturalHeight) return null;
                    const style = getComputedStyle(stage);
                    const availableWidth = stage.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight);
                    const availableHeight = stage.clientHeight - parseFloat(style.paddingTop) - parseFloat(style.paddingBottom);
                    const scale = Math.min(availableWidth / image.naturalWidth, availableHeight / image.naturalHeight);
                    const width = image.naturalWidth * scale;
                    const height = image.naturalHeight * scale;
                    image.style.width = `${{width}}px`;
                    image.style.height = `${{height}}px`;
                    image.style.maxWidth = 'none';
                    image.style.maxHeight = 'none';
                    stage.classList.remove('is-zoomed');
                    stage.classList.remove('is-overflow-x', 'is-overflow-y');
                    stage.scrollLeft = 0;
                    stage.scrollTop = 0;
                    return [width, height];
                    "#,
                );
                if let Ok(value) = document::eval(&script).await
                    && let (Some(width), Some(height)) = (
                        value.get(0).and_then(|item| item.as_f64()),
                        value.get(1).and_then(|item| item.as_f64()),
                    )
                {
                    fitted_size.set(Some((width, height)));
                    zoom.set(None);
                    zoom_overflow.set((false, false));
                }
            });
        }
    });

    // Size and scroll move in one WebView operation, then Rust adopts that
    // already-visible state. This prevents an intermediate displaced frame.
    let apply_zoom = use_callback({
        let stage_id = stage_id.clone();
        move |(level, anchor): (u32, MediaZoomAnchor)| {
            let pointer_zoom = matches!(anchor, MediaZoomAnchor::Pointer(_, _));
            if pointer_zoom && wheel_zooming() {
                return;
            }
            if pointer_zoom {
                wheel_zooming.set(true);
            }
            let Some((fit_width, fit_height)) = fitted_size() else {
                if pointer_zoom {
                    wheel_zooming.set(false);
                }
                return;
            };
            let width = fit_width * level as f64 / FIT_ZOOM as f64;
            let height = fit_height * level as f64 / FIT_ZOOM as f64;
            let is_zoomed = level > FIT_ZOOM;
            let (pointer_x, pointer_y, center) = match anchor {
                MediaZoomAnchor::Center => ("null".to_string(), "null".to_string(), true),
                MediaZoomAnchor::Pointer(x, y) => (x.to_string(), y.to_string(), false),
            };
            let stage_id = stage_id.clone();
            spawn(async move {
                let script = format!(
                    r#"
                    const stage = document.getElementById('{stage_id}');
                    const image = stage?.querySelector('.media-viewer-image');
                    if (!stage || !image) return;
                    const rect = stage.getBoundingClientRect();
                    const oldImageRect = image.getBoundingClientRect();
                    const clientX = {pointer_x} ?? oldImageRect.left + oldImageRect.width / 2;
                    const clientY = {pointer_y} ?? oldImageRect.top + oldImageRect.height / 2;
                    const screenX = clientX - rect.left;
                    const screenY = clientY - rect.top;
                    const nx = Math.max(0, Math.min(1, (clientX - oldImageRect.left) / oldImageRect.width));
                    const ny = Math.max(0, Math.min(1, (clientY - oldImageRect.top) / oldImageRect.height));
                    image.style.width = '{width}px';
                    image.style.height = '{height}px';
                    image.style.maxWidth = 'none';
                    image.style.maxHeight = 'none';
                    stage.classList.toggle('is-zoomed', {is_zoomed});
                    const style = getComputedStyle(stage);
                    const availableWidth = stage.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight);
                    const availableHeight = stage.clientHeight - parseFloat(style.paddingTop) - parseFloat(style.paddingBottom);
                    const overflowX = {width} > availableWidth;
                    const overflowY = {height} > availableHeight;
                    stage.classList.toggle('is-overflow-x', overflowX);
                    stage.classList.toggle('is-overflow-y', overflowY);

                    if ({center}) {{
                        stage.scrollLeft = Math.max(0, (stage.scrollWidth - stage.clientWidth) / 2);
                        stage.scrollTop = Math.max(0, (stage.scrollHeight - stage.clientHeight) / 2);
                    }} else {{
                        const newImageRect = image.getBoundingClientRect();
                        const imageLeft = newImageRect.left - rect.left + stage.scrollLeft;
                        const imageTop = newImageRect.top - rect.top + stage.scrollTop;
                        stage.scrollLeft = imageLeft + nx * newImageRect.width - screenX;
                        stage.scrollTop = imageTop + ny * newImageRect.height - screenY;
                    }}
                    return [overflowX, overflowY];
                    "#,
                );
                if let Ok(value) = document::eval(&script).await {
                    zoom_overflow.set((
                        value
                            .get(0)
                            .and_then(|item| item.as_bool())
                            .unwrap_or(false),
                        value
                            .get(1)
                            .and_then(|item| item.as_bool())
                            .unwrap_or(false),
                    ));
                }
                zoom.set(Some(level));
                if pointer_zoom {
                    wheel_zooming.set(false);
                }
            });
        }
    });
    let stage_id_for_move = stage_id.clone();

    // A document has no bytes of its own: what is shown is its current page,
    // which is a media in its own right. Everything below therefore reads the
    // page when there is one and the media itself when there is not.
    let pages = use_resource({
        let api = api.clone();
        let document_id = current_tile.media.id;
        move || {
            let api = api.clone();
            async move {
                if is_document {
                    api.list_media_pages(tree_id, document_id).await.ok()
                } else {
                    None
                }
            }
        }
    });
    let page_list: Vec<oxidgene_core::types::Media> = match &*pages.read_unchecked() {
        Some(Some(list)) => list.clone(),
        _ => Vec::new(),
    };
    let total_pages = page_list.len();
    // Clamp rather than trust the signal: detaching a page while the viewer is
    // open would otherwise leave it pointing past the end.
    let current = page().min(total_pages.saturating_sub(1));
    let shown = page_list.get(current);

    let kind = match shown {
        Some(page) => crate::api::media_kind(&page.mime_type),
        None if is_document => MediaKind::Document,
        None => current_tile.kind(),
    };
    let content_media_id = shown.map(|media| media.id).unwrap_or(current_tile.media.id);
    let (content_width, content_height) = shown
        .map(|media| (media.width, media.height))
        .unwrap_or((current_media.width, current_media.height));
    let content_media = shown.cloned().unwrap_or_else(|| current_media.clone());
    // A document's visible image changes with its page. Keep that id reactive
    // so the regions in both the facts column and the image follow it.
    let mut vignette_media_id = use_signal(|| content_media_id);
    let mut vignette_revision = use_signal(|| 0_u32);
    if *vignette_media_id.peek() != content_media_id {
        vignette_media_id.set(content_media_id);
    }
    let requested_asset_id = match (shown, source) {
        (Some(page), _) => Some(page.id),
        (None, MediaSource::Stored) => Some(current_tile.media.id),
        _ => None,
    };
    let mut asset_media_id = use_signal(|| requested_asset_id);
    if *asset_media_id.peek() != requested_asset_id {
        asset_media_id.set(requested_asset_id);
    }
    let media_asset = use_resource({
        let api = api.clone();
        move || {
            let api = api.clone();
            let media_id = asset_media_id();
            async move {
                match media_id {
                    Some(media_id) => api.media_file_data_url(tree_id, media_id).await.ok(),
                    None => None,
                }
            }
        }
    });
    let url = match source {
        MediaSource::Remote if shown.is_none() => Some(current_tile.media.file_path.clone()),
        _ => media_asset.read_unchecked().as_ref().and_then(Clone::clone),
    };
    let content_vignettes = use_resource({
        let api = api.clone();
        move || {
            let api = api.clone();
            let media_id = vignette_media_id();
            let _ = vignette_revision();
            async move { api.list_media_vignettes(tree_id, media_id).await.ok() }
        }
    });
    let content_vignettes: Vec<Vignette> = content_vignettes
        .read_unchecked()
        .as_ref()
        .and_then(|vignettes| vignettes.clone())
        .unwrap_or_default();
    let person_names = use_resource({
        let api = api.clone();
        move || {
            let api = api.clone();
            async move {
                api.get_tree_snapshot(tree_id)
                    .await
                    .ok()
                    .map(|snapshot| snapshot.names)
            }
        }
    });
    let person_names: Vec<PersonName> = person_names
        .read_unchecked()
        .as_ref()
        .and_then(|names| names.clone())
        .unwrap_or_default();
    let image_id = format!("media-viewer-image-{content_media_id}");
    let image_style = match (zoom(), fitted_size()) {
        (Some(level), Some((width, height))) => format!(
            "width: {}px; height: {}px; max-width: none; max-height: none;",
            width * level as f64 / FIT_ZOOM as f64,
            height * level as f64 / FIT_ZOOM as f64,
        ),
        (Some(_), None) => "width: auto; max-width: none; max-height: none;".to_string(),
        (None, Some((width, height))) => {
            format!("width: {width}px; height: {height}px; max-width: none; max-height: none;",)
        }
        (None, None) => String::new(),
    };
    use_effect({
        let image_id = image_id.clone();
        let stage_id = stage_id.clone();
        move || {
            let image_id = image_id.clone();
            let stage_id = stage_id.clone();
            spawn(async move {
                let script = format!(
                    r#"
                    const image = document.getElementById('{image_id}');
                    const stage = document.getElementById('{stage_id}');
                    if (!image || !stage) return null;
                    for (let frame = 0; frame < 8 && (!image.complete || !image.naturalWidth); frame += 1) {{
                        await new Promise(requestAnimationFrame);
                    }}
                    if (!image.naturalWidth || !image.naturalHeight) return null;
                    const style = getComputedStyle(stage);
                    return [
                        image.naturalWidth,
                        image.naturalHeight,
                        stage.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight),
                        stage.clientHeight - parseFloat(style.paddingTop) - parseFloat(style.paddingBottom),
                    ];
                    "#,
                );
                if let Ok(value) = document::eval(&script).await {
                    let width = value.get(0).and_then(|item| item.as_f64());
                    let height = value.get(1).and_then(|item| item.as_f64());
                    let space_width = value.get(2).and_then(|item| item.as_f64());
                    let space_height = value.get(3).and_then(|item| item.as_f64());
                    if let (Some(width), Some(height), Some(space_width), Some(space_height)) =
                        (width, height, space_width, space_height)
                        && width > 0.0
                        && height > 0.0
                    {
                        let scale = (space_width / width).min(space_height / height);
                        let fitted = (width * scale, height * scale);
                        let fit_changed = fitted_size().is_none_or(|(old_width, old_height)| {
                            (old_width - fitted.0).abs() > 0.5
                                || (old_height - fitted.1).abs() > 0.5
                        });
                        if fit_changed {
                            fitted_size.set(Some(fitted));
                        }
                    }
                }
            });
        }
    });

    let (overflow_x, overflow_y) = zoom_overflow();
    let mut stage_class = "media-viewer-stage is-image".to_string();
    if zoom().is_some_and(|level| level > FIT_ZOOM) {
        stage_class.push_str(" is-zoomed");
    }
    if overflow_x {
        stage_class.push_str(" is-overflow-x");
    }
    if overflow_y {
        stage_class.push_str(" is-overflow-y");
    }
    if dragging_image() {
        stage_class.push_str(" is-dragging");
    }

    let close_attachment = use_callback(move |()| {
        attachment_mode.set(None);
        attachment_busy.set(false);
        attachment_error.set(None);
        family_choices.set(Vec::new());
    });

    let attach_person = {
        let api = api.clone();
        let success = i18n.t("media.attach_success");
        let duplicate = i18n.t("media.attach_duplicate");
        move |person_id: Uuid| {
            let api = api.clone();
            let success = success.clone();
            let duplicate = duplicate.clone();
            spawn(async move {
                attachment_busy.set(true);
                attachment_error.set(None);
                attachment_notice.set(None);
                match attach_media_to(
                    &api,
                    tree_id,
                    content_media_id,
                    MediaAttachmentTarget::Person(person_id),
                )
                .await
                {
                    Ok(created) => {
                        attachment_notice.set(Some(if created { success } else { duplicate }));
                        attachment_mode.set(None);
                        attachment_revision += 1;
                        on_changed.call(());
                    }
                    Err(error) => attachment_error.set(Some(error.to_string())),
                }
                attachment_busy.set(false);
            });
        }
    };

    let select_couple_person = {
        let api = api.clone();
        move |person_id: Uuid| {
            let api = api.clone();
            spawn(async move {
                attachment_busy.set(true);
                attachment_error.set(None);
                family_choices.set(Vec::new());
                match api.get_person_profile(tree_id, person_id).await {
                    Ok(profile) => {
                        let choices: Vec<MediaFamilyChoice> = profile
                            .families_as_spouse
                            .into_iter()
                            .map(|family| {
                                let spouse = family
                                    .spouse_display_name
                                    .unwrap_or_else(|| i18n.t("media.attach_unknown_spouse"));
                                MediaFamilyChoice {
                                    family_id: family.family_id,
                                    label: i18n
                                        .t_args("media.attach_couple_with", &[("person", &spouse)]),
                                }
                            })
                            .collect();
                        if choices.is_empty() {
                            attachment_error.set(Some(i18n.t("media.attach_no_couple")));
                            attachment_mode.set(Some(MediaAttachmentMode::CouplePerson));
                        } else {
                            family_choices.set(choices);
                            attachment_mode.set(Some(MediaAttachmentMode::CoupleFamily));
                        }
                    }
                    Err(error) => attachment_error.set(Some(error.to_string())),
                }
                attachment_busy.set(false);
            });
        }
    };

    let attach_family = use_callback({
        let api = api.clone();
        let success = i18n.t("media.attach_success");
        let duplicate = i18n.t("media.attach_duplicate");
        move |family_id: Uuid| {
            let api = api.clone();
            let success = success.clone();
            let duplicate = duplicate.clone();
            spawn(async move {
                attachment_busy.set(true);
                attachment_error.set(None);
                attachment_notice.set(None);
                match attach_media_to(
                    &api,
                    tree_id,
                    content_media_id,
                    MediaAttachmentTarget::Family(family_id),
                )
                .await
                {
                    Ok(created) => {
                        attachment_notice.set(Some(if created { success } else { duplicate }));
                        attachment_mode.set(None);
                        attachment_revision += 1;
                        on_changed.call(());
                    }
                    Err(error) => attachment_error.set(Some(error.to_string())),
                }
                attachment_busy.set(false);
            });
        }
    });

    rsx! {
        div { class: "cropper-backdrop", onclick: move |_| on_close.call(()),
            div { class: "media-viewer", onclick: move |e| e.stop_propagation(),
                div { class: "cropper-head",
                    span { class: "cropper-title", "{caption}" }
                    if total_pages > 1 {
                        span { class: "media-pager-count",
                            {i18n.t_args(
                                "media.page_of",
                                &[
                                    ("page", &(current + 1).to_string()),
                                    ("total", &total_pages.to_string()),
                                ],
                            )}
                        }
                    }
                    button {
                        class: "cropper-close",
                        r#type: "button",
                        onclick: move |_| on_close.call(()),
                        "\u{00D7}"
                    }
                }


                div { class: "media-viewer-body",
                aside { class: "media-viewer-aside",
                    if editing() {
                        MediaEditPanel {
                            tree_id,
                            tile: current_tile.clone(),
                            events: events.clone(),
                            embedded: true,
                            on_changed: move |()| {
                                media_revision += 1;
                                on_changed.call(());
                            },
                            on_close: move |()| editing.set(false),
                        }
                    } else {
                        MediaFacts {
                            tree_id,
                            media: current_media.clone(),
                            attachment_media_id: content_media_id,
                            attachment_revision: attachment_revision(),
                            tags: current_media.tags.clone(),
                            vignettes: content_vignettes.clone(),
                            person_names: person_names.clone(),
                            events: events.clone(),
                            on_vignettes_changed: move |_| vignette_revision += 1,
                            on_vignette_hover: move |vignette_id| highlighted_vignette.set(vignette_id),
                            on_changed,
                        }
                        // Not gated on `read_only`. That flag governs the
                        // *gallery* — uploading, cropping, detaching — which
                        // is restructuring what a person has. Recording when a
                        // scan was taken is describing the scan itself, and
                        // the moment a reader knows that is while looking at
                        // it. Sending them to the edit modal to type a date
                        // means leaving the page that prompted them.
                        div { class: "media-facts-actions",
                            button {
                                class: "pf-confirm-btn media-facts-edit",
                                r#type: "button",
                                onclick: move |_| editing.set(true),
                                {i18n.t("media.viewer_edit")}
                            }
                            button {
                                class: "pf-delete-person-btn media-facts-delete",
                                r#type: "button",
                                disabled: deleting(),
                                onclick: move |_| {
                                    delete_error.set(None);
                                    delete_confirming.set(true);
                                },
                                {i18n.t("media.viewer_delete")}
                            }
                        }
                    }
                }

                div { class: "media-viewer-main",
                // Zoom belongs to images alone: a video and an audio track
                // have their own controls, and a fallback has nothing to
                // magnify. These use the tree sidebar's visual language so
                // they remain compact beside a large scan.
                if matches!(kind, MediaKind::Image) && url.is_some() {
                    div { class: "media-viewer-controls",
                        button {
                            class: "isb-btn",
                            r#type: "button",
                            title: i18n.t("media.zoom_in"),
                            disabled: fitted_size().is_none() || zoom().is_some_and(|z| z >= MAX_ZOOM),
                            onclick: move |_| apply_zoom.call((zoom_in(zoom()), MediaZoomAnchor::Center)),
                            svg {
                                width: "16", height: "16", fill: "none", "viewBox": "0 0 24 24",
                                stroke: "currentColor", "strokeWidth": "2",
                                circle { cx: "11", cy: "11", r: "8" }
                                line { x1: "21", y1: "21", x2: "16.65", y2: "16.65" }
                                line { x1: "11", y1: "8", x2: "11", y2: "14" }
                                line { x1: "8", y1: "11", x2: "14", y2: "11" }
                            }
                        }
                        button {
                            class: "isb-btn",
                            r#type: "button",
                            title: i18n.t("media.zoom_fit"),
                            onclick: move |_| fit_image.call(()),
                            svg {
                                width: "16", height: "16", fill: "none", "viewBox": "0 0 24 24",
                                stroke: "currentColor", "strokeWidth": "2",
                                path { d: "M3 8V5a2 2 0 0 1 2-2h3" }
                                path { d: "M16 3h3a2 2 0 0 1 2 2v3" }
                                path { d: "M21 16v3a2 2 0 0 1-2 2h-3" }
                                path { d: "M8 21H5a2 2 0 0 1-2-2v-3" }
                            }
                        }
                        button {
                            class: "isb-btn",
                            r#type: "button",
                            title: i18n.t("media.zoom_out"),
                            disabled: fitted_size().is_none() || zoom().is_some_and(|z| z <= MIN_ZOOM),
                            onclick: move |_| apply_zoom.call((zoom_out(zoom()), MediaZoomAnchor::Center)),
                            svg {
                                width: "16", height: "16", fill: "none", "viewBox": "0 0 24 24",
                                stroke: "currentColor", "strokeWidth": "2",
                                circle { cx: "11", cy: "11", r: "8" }
                                line { x1: "21", y1: "21", x2: "16.65", y2: "16.65" }
                                line { x1: "8", y1: "11", x2: "14", y2: "11" }
                            }
                        }
                        button {
                            class: "pf-confirm-btn media-identify-toolbar-btn",
                            r#type: "button",
                            onclick: move |_| identifying.set(true),
                            {i18n.t("media.identify_person")}
                        }
                        button {
                            class: "btn btn-outline media-attach-toolbar-btn",
                            r#type: "button",
                            onclick: move |_| {
                                close_attachment.call(());
                                attachment_notice.set(None);
                                attachment_mode.set(Some(MediaAttachmentMode::Person));
                            },
                            {i18n.t("media.attach_person")}
                        }
                        button {
                            class: "btn btn-outline media-attach-toolbar-btn",
                            r#type: "button",
                            onclick: move |_| {
                                close_attachment.call(());
                                attachment_notice.set(None);
                                attachment_mode.set(Some(MediaAttachmentMode::CouplePerson));
                            },
                            {i18n.t("media.attach_couple")}
                        }
                    }
                }
                if let Some(mode) = attachment_mode() {
                    div { class: "media-attachment-picker",
                        div { class: "media-attachment-picker-head",
                            strong {
                                if mode == MediaAttachmentMode::Person {
                                    {i18n.t("media.attach_person")}
                                } else if mode == MediaAttachmentMode::CouplePerson {
                                    {i18n.t("media.attach_find_couple")}
                                } else {
                                    {i18n.t("media.attach_choose_couple")}
                                }
                            }
                            button {
                                class: "person-form-close",
                                r#type: "button",
                                onclick: move |_| close_attachment.call(()),
                                "\u{00D7}"
                            }
                        }
                        if attachment_busy() {
                            div { class: "loading", {i18n.t("common.loading")} }
                        } else if mode == MediaAttachmentMode::Person {
                            SearchPerson {
                                tree_id,
                                placeholder: i18n.t("media.attach_person_placeholder"),
                                on_select: attach_person,
                                on_cancel: move |()| close_attachment.call(()),
                            }
                        } else if mode == MediaAttachmentMode::CouplePerson {
                            SearchPerson {
                                tree_id,
                                placeholder: i18n.t("media.attach_couple_placeholder"),
                                on_select: select_couple_person,
                                on_cancel: move |()| close_attachment.call(()),
                            }
                        } else {
                            div { class: "media-family-choices",
                                for family in family_choices() {
                                    button {
                                        key: "{family.family_id}",
                                        class: "btn btn-outline",
                                        r#type: "button",
                                        onclick: move |_| attach_family.call(family.family_id),
                                        "{family.label}"
                                    }
                                }
                            }
                        }
                        if let Some(message) = attachment_error() {
                            div { class: "error-msg", "{message}" }
                        }
                    }
                }
                if let Some(message) = attachment_notice() {
                    div { class: "media-attachment-notice", "{message}" }
                }
                div {
                    id: "{stage_id}",
                    class: "{stage_class}",
                    onpointermove: move |event| {
                        let coords = event.client_coordinates();
                        if dragging_image() {
                            let stage_id = stage_id_for_move.clone();
                            let delta_x = coords.x - drag_start_x();
                            let delta_y = coords.y - drag_start_y();
                            drag_start_x.set(coords.x);
                            drag_start_y.set(coords.y);
                            spawn(async move {
                                let script = format!(
                                    "const stage = document.getElementById('{stage_id}'); if (stage) {{ stage.scrollLeft -= {delta_x}; stage.scrollTop -= {delta_y}; }}"
                                );
                                let _ = document::eval(&script).await;
                            });
                        }
                    },
                    onpointerdown: move |event| {
                        if !matches!(kind, MediaKind::Image) { return; }
                        event.prevent_default();
                        let coords = event.client_coordinates();
                        drag_start_x.set(coords.x);
                        drag_start_y.set(coords.y);
                        dragging_image.set(true);
                    },
                    ondragstart: move |event| event.prevent_default(),
                    onpointerup: move |_| dragging_image.set(false),
                    onpointerleave: move |_| dragging_image.set(false),
                    onwheel: move |event| {
                        event.prevent_default();
                        let coords = event.client_coordinates();
                        let delta_y = match event.delta() {
                            WheelDelta::Lines(lines) => lines.y,
                            WheelDelta::Pixels(pixels) => pixels.y,
                            WheelDelta::Pages(pages) => pages.y,
                        };
                        let next = if delta_y > 0.0 {
                            zoom_out(zoom())
                        } else {
                            zoom_in(zoom())
                        };
                        apply_zoom.call((next, MediaZoomAnchor::Pointer(coords.x, coords.y)));
                    },
                    match (url.clone(), kind) {
                        (Some(url), MediaKind::Image) => rsx! {
                            div { class: "media-viewer-image-frame",
                                img {
                                    id: "{image_id}",
                                    class: "media-viewer-image media-viewer-static-image",
                                    src: "{url}",
                                    alt: "{caption}",
                                    draggable: "false",
                                    style: "{image_style}",
                                    onload: move |_| fit_image.call(()),
                                }
                                for vignette in content_vignettes.iter() {
                                    div {
                                        key: "{vignette.id}",
                                        class: if highlighted_vignette() == Some(vignette.id) {
                                            "media-viewer-vignette is-active"
                                        } else {
                                            "media-viewer-vignette"
                                        },
                                        style: "{vignette_overlay_style(vignette, content_width, content_height)}",
                                        onpointerdown: move |event| event.stop_propagation(),
                                        if let Some(person_id) = vignette.person_id
                                            && let Some(person) = primary_person_name_record(&person_names, person_id)
                                        {
                                            span { class: "media-viewer-vignette-label",
                                                if let Some(surname) = person.full_surname() {
                                                    span { class: "media-viewer-vignette-surname", "{surname.to_uppercase()}" }
                                                }
                                                if let Some(given_names) = person.given_names.as_ref() {
                                                    span { class: "media-viewer-vignette-given", "{given_names}" }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        },
                        (Some(url), MediaKind::Video) => rsx! {
                            video {
                                class: "media-viewer-image",
                                src: "{url}",
                                controls: true,
                                preload: "metadata",
                            }
                        },
                        (Some(url), MediaKind::Audio) => rsx! {
                            audio { class: "media-viewer-audio", src: "{url}", controls: true }
                        },
                        (Some(_), _) => rsx! {
                            div { class: "media-viewer-fallback",
                                span { class: "media-glyph-large", {kind.icon()} }
                                p { {i18n.t("media.not_embeddable")} }
                            }
                        },
                        (None, _) => rsx! {
                            div { class: "media-viewer-fallback",
                                span { class: "media-glyph-large", {kind.icon()} }
                                p { {i18n.t("media.no_file")} }
                                if !tile.media.file_path.is_empty() {
                                    code { class: "media-viewer-path", "{tile.media.file_path}" }
                                }
                            }
                        },
                    }
                }

                // The pager. Step buttons for reading front to back, jump
                // buttons for the ends, and a numbered strip because "the
                // entry is on page 27" is how a register is actually
                // referenced — counting there with a Next button is absurd.
                if total_pages > 1 {
                    div { class: "media-pager",
                        button {
                            class: "media-pager-btn",
                            r#type: "button",
                            disabled: current == 0,
                            title: i18n.t("media.first_page"),
                            onclick: move |_| {
                                page.set(0);
                                zoom.set(None);
                            },
                            "\u{23EE}"
                        }
                        button {
                            class: "media-pager-btn",
                            r#type: "button",
                            disabled: current == 0,
                            title: i18n.t("media.previous_page"),
                            onclick: move |_| {
                                page.set(current.saturating_sub(1));
                                zoom.set(None);
                            },
                            "\u{25C0}"
                        }
                        div { class: "media-pager-numbers",
                            for (slot_index , slot) in page_window(current, total_pages)
                                .into_iter()
                                .enumerate()
                            {
                                match slot {
                                    PagerSlot::Page(index) => rsx! {
                                        button {
                                            key: "p{index}",
                                            class: if index == current {
                                                "media-pager-num is-current"
                                            } else {
                                                "media-pager-num"
                                            },
                                            r#type: "button",
                                            onclick: move |_| {
                                                page.set(index);
                                                zoom.set(None);
                                            },
                                            "{index + 1}"
                                        }
                                    },
                                    PagerSlot::Gap => rsx! {
                                        span { key: "g{slot_index}", class: "media-pager-gap", "\u{2026}" }
                                    },
                                }
                            }
                        }
                        button {
                            class: "media-pager-btn",
                            r#type: "button",
                            disabled: current + 1 >= total_pages,
                            title: i18n.t("media.next_page"),
                            onclick: move |_| {
                                page.set((current + 1).min(total_pages - 1));
                                zoom.set(None);
                            },
                            "\u{25B6}"
                        }
                        button {
                            class: "media-pager-btn",
                            r#type: "button",
                            disabled: current + 1 >= total_pages,
                            title: i18n.t("media.last_page"),
                            onclick: move |_| {
                                page.set(total_pages - 1);
                                zoom.set(None);
                            },
                            "\u{23ED}"
                        }
                    }
                    }
                }
                }

                div { class: "cropper-foot",
                    if let Some(description) = tile.media.description.as_ref() {
                        p { class: "media-viewer-desc", "{description}" }
                    }
                    div { class: "cropper-actions",
                        if let Some(url) = url.clone() {
                            DownloadMediaButton {
                                source: match source {
                                    MediaSource::Remote if shown.is_none() => {
                                        MediaDownloadSource::Remote(url)
                                    }
                                    _ => MediaDownloadSource::File {
                                        tree_id,
                                        media_id: content_media_id,
                                    },
                                },
                                // The page's own name when looking at a page,
                                // and either way one the file system can open:
                                // a Geneanet deposit is titled, not named.
                                file_name: match shown {
                                    Some(page) => download_name(&page.file_name, &page.mime_type),
                                    None => download_name(&tile.media.file_name, &tile.media.mime_type),
                                },
                            }
                        }
                        // Forty scans are one document to the reader and forty
                        // save dialogs one at a time. The archive numbers them
                        // so unzipping restores the reading order.
                        if total_pages > 1 {
                            DownloadMediaButton {
                                source: MediaDownloadSource::Archive {
                                    tree_id,
                                    media_id: tile.media.id,
                                },
                                file_name: format!("{}.zip", archive_stem(&caption)),
                                label: i18n.t("media.download_all_pages"),
                            }
                        }
                        button {
                            class: "btn btn-primary",
                            r#type: "button",
                            onclick: move |_| on_close.call(()),
                            {i18n.t("common.close")}
                        }
                    }
                }
            }
            if identifying() {
                IdentificationCropperHost {
                    tree_id,
                    media: content_media,
                    on_complete: move |_| {
                        identifying.set(false);
                        vignette_revision += 1;
                    },
                }
            }
        }
        if delete_confirming() {
            ConfirmDialog {
                title: i18n.t("media.delete_title"),
                message: i18n.t("media.delete_message"),
                confirm_label: i18n.t("media.delete"),
                error: delete_error(),
                busy: deleting(),
                on_confirm: delete_media,
                on_cancel: move |_| {
                    delete_confirming.set(false);
                    delete_error.set(None);
                },
            }
        }
    }
}

#[derive(Clone, PartialEq)]
enum MediaDownloadSource {
    File { tree_id: Uuid, media_id: Uuid },
    Archive { tree_id: Uuid, media_id: Uuid },
    Remote(String),
}

impl MediaDownloadSource {
    async fn load(&self, api: &ApiClient) -> Result<Vec<u8>, crate::api::ApiError> {
        match self {
            Self::File { tree_id, media_id } => api.media_file_bytes(*tree_id, *media_id).await,
            Self::Archive { tree_id, media_id } => {
                api.media_archive_bytes(*tree_id, *media_id).await
            }
            Self::Remote(url) => {
                let response = reqwest::get(url).await?;
                let status = response.status();
                if !status.is_success() {
                    return Err(crate::api::ApiError::Api {
                        status: status.as_u16(),
                        body: String::new(),
                    });
                }
                Ok(response.bytes().await?.to_vec())
            }
        }
    }
}

/// The one download control, implemented per platform.
///
/// One button rather than two, because a reader looking at a photograph has
/// exactly one intention and offering it twice under different names invites
/// them to wonder what the difference is. There is none worth exposing — only
/// two ways of achieving it:
///
///   - **Web**: an `<a download>`. The browser owns downloading, it knows
///     where the user's downloads go, and duplicating that with our own dialog
///     would be worse than none.
///   - **Desktop**: the platform's save dialog. The embedded WebView has no
///     download UI of its own, so a `download` attribute there does nothing at
///     all — the link would look like a button and be inert.
#[component]
fn DownloadMediaButton(
    source: MediaDownloadSource,
    file_name: String,
    /// What the button says. Defaults to "Download file"; the archive button
    /// passes its own, since two identical buttons side by side would leave
    /// the reader guessing which is which.
    label: Option<String>,
) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let label = label.unwrap_or_else(|| i18n.t("media.download_file"));
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    #[cfg(target_arch = "wasm32")]
    {
        let download = move |_| {
            let api = api.clone();
            let source = source.clone();
            let file_name = file_name.clone();
            spawn(async move {
                busy.set(true);
                error.set(None);
                match source.load(&api).await {
                    Ok(bytes) => {
                        let byte_array =
                            serde_json::to_string(&bytes).unwrap_or_else(|_| "[]".to_string());
                        let download_name = serde_json::to_string(&file_name)
                            .unwrap_or_else(|_| "\"media\"".to_string());
                        document::eval(&format!(
                            r#"
                            const bytes = new Uint8Array({byte_array});
                            const blob = new Blob([bytes]);
                            const url = URL.createObjectURL(blob);
                            const anchor = document.createElement('a');
                            anchor.href = url;
                            anchor.download = {download_name};
                            document.body.appendChild(anchor);
                            anchor.click();
                            anchor.remove();
                            URL.revokeObjectURL(url);
                            "#
                        ));
                    }
                    Err(err) => error.set(Some(err.to_string())),
                }
                busy.set(false);
            });
        };
        rsx! {
            button {
                class: "btn btn-outline",
                r#type: "button",
                disabled: busy(),
                onclick: download,
                if busy() { {i18n.t("common.saving")} } else { {label} }
            }
            if let Some(err) = error() {
                div { class: "error-msg", "{err}" }
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let save = move |_| {
            let api = api.clone();
            let source = source.clone();
            let file_name = file_name.clone();
            spawn(async move {
                busy.set(true);
                error.set(None);
                // Ask for the destination first: fetching several megabytes
                // and only then discovering the user cancelled is work thrown
                // away, and a dialog that opens after a delay reads as a hang.
                let target = rfd::AsyncFileDialog::new()
                    // The button says "Download"; the OS dialog it opens is a
                    // save dialog, and titling it as one is what the platform's
                    // own conventions expect.
                    .set_title(i18n.t("media.save_as"))
                    .set_file_name(&file_name)
                    .save_file()
                    .await;
                let Some(target) = target else {
                    busy.set(false);
                    return;
                };
                match source.load(&api).await {
                    Ok(bytes) => {
                        if let Err(err) = target.write(&bytes).await {
                            error.set(Some(err.to_string()));
                        }
                    }
                    Err(e) => error.set(Some(e.to_string())),
                }
                busy.set(false);
            });
        };

        rsx! {
            button {
                class: "btn btn-outline",
                r#type: "button",
                disabled: busy(),
                onclick: save,
                if busy() { {i18n.t("common.saving")} } else { {label} }
            }
            if let Some(err) = error() {
                div { class: "error-msg", "{err}" }
            }
        }
    }
}

// ── Cropper host ────────────────────────────────────────────────────

/// Opens the cropper over a media, feeding it the crops already on that page.
#[component]
fn CropperHost(
    tree_id: Uuid,
    tile: MediaWithLink,
    owner: MediaOwner,
    events: Vec<(Uuid, String)>,
    on_close: EventHandler<()>,
) -> Element {
    let api = use_context::<ApiClient>();
    let media_id = tile.media.id;
    let mut revision = use_signal(|| 0_u32);

    let existing = use_resource({
        let api = api.clone();
        move || {
            let api = api.clone();
            let _ = revision();
            async move { api.list_media_vignettes(tree_id, media_id).await }
        }
    });

    let crops: Vec<Vignette> = match &*existing.read_unchecked() {
        Some(Ok(list)) => list.clone(),
        _ => Vec::new(),
    };

    rsx! {
        ImageCropper {
            tree_id,
            media: tile.media.clone(),
            existing: crops,
            person_id: match owner {
                MediaOwner::Person(id) => Some(id),
                MediaOwner::Family(_) | MediaOwner::Event(_) => None,
            },
            events,
            on_saved: move |_| revision += 1,
            on_close: move |_| on_close.call(()),
        }
    }
}

/// Creates an identification as one uninterrupted reader workflow: first the
/// region on the image, then the person it represents.
#[component]
fn IdentificationCropperHost(
    tree_id: Uuid,
    media: oxidgene_core::types::Media,
    on_complete: EventHandler<()>,
) -> Element {
    let api = use_context::<ApiClient>();
    let media_id = media.id;
    let mut pending = use_signal(|| None::<Vignette>);
    let existing = use_resource({
        let api = api.clone();
        move || {
            let api = api.clone();
            async move { api.list_media_vignettes(tree_id, media_id).await.ok() }
        }
    });
    let existing: Vec<Vignette> = existing
        .read_unchecked()
        .as_ref()
        .and_then(|vignettes| vignettes.clone())
        .unwrap_or_default();

    if let Some(vignette) = pending() {
        let vignette_id = vignette.id;
        let api_for_select = api.clone();
        let api_for_cancel = api.clone();
        return rsx! {
            div { class: "cropper-backdrop",
                div { class: "cropper-panel", onclick: move |event| event.stop_propagation(),
                    div { class: "cropper-head",
                        span { class: "cropper-title", {use_i18n().t("media.identify_person")} }
                    }
                    SearchPerson {
                        tree_id,
                        on_select: move |person_id| {
                            let api = api_for_select.clone();
                            spawn(async move {
                                if api.update_vignette(
                                    tree_id,
                                    vignette_id,
                                    &UpdateVignetteBody {
                                        person_id: Some(Some(person_id)),
                                        ..Default::default()
                                    },
                                ).await.is_ok() {
                                    on_complete.call(());
                                }
                            });
                        },
                        on_cancel: move |_| {
                            let api = api_for_cancel.clone();
                            spawn(async move {
                                let _ = api.delete_vignette(tree_id, vignette_id).await;
                                on_complete.call(());
                            });
                        },
                    }
                }
            }
        };
    }

    rsx! {
        ImageCropper {
            tree_id,
            media,
            existing,
            on_saved: move |vignette| pending.set(Some(vignette)),
            on_close: move |_| on_complete.call(()),
        }
    }
}

/// One slot in the pager strip: a page to jump to, or a gap where pages were
/// left out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PagerSlot {
    Page(usize),
    Gap,
}

/// Which page numbers to show, for a document of `total` pages sitting on
/// `current` (both zero-based).
///
/// A parish register runs to hundreds of pages, and drawing a button for each
/// produces a strip longer than the image above it. This keeps the ends —
/// "back to the start" and "how long is this" are both things a reader asks —
/// plus a window around where they are, and elides the rest.
///
/// A gap is only worth drawing if it hides more than one page: replacing a
/// single number with an ellipsis costs the same width and takes away a
/// destination, so a lone skipped page is shown instead.
pub(crate) fn page_window(current: usize, total: usize) -> Vec<PagerSlot> {
    /// Pages either side of the current one.
    const RADIUS: usize = 2;

    if total == 0 {
        return Vec::new();
    }
    let last = total - 1;
    let current = current.min(last);
    let window_start = current.saturating_sub(RADIUS);
    let window_end = (current + RADIUS).min(last);

    let mut slots = Vec::new();
    let mut previous: Option<usize> = None;
    for page in (0..total)
        .filter(|page| *page == 0 || *page == last || (window_start..=window_end).contains(page))
    {
        match previous {
            // Two pages apart means exactly one was skipped; show it rather
            // than spend the same space on an ellipsis.
            Some(prev) if page == prev + 2 => slots.push(PagerSlot::Page(prev + 1)),
            Some(prev) if page > prev + 1 => slots.push(PagerSlot::Gap),
            _ => {}
        }
        slots.push(PagerSlot::Page(page));
        previous = Some(page);
    }
    slots
}

/// Zoom bounds and step, as percentages of the fitted size.
///
/// The ceiling is high on purpose: the reason to zoom a parish register is to
/// read one word of secretary hand in a corner, and 200% does not get there.
pub(crate) const MIN_ZOOM: u32 = 25;
pub(crate) const MAX_ZOOM: u32 = 3200;

/// The fitted image is the zoom baseline.
const FIT_ZOOM: u32 = 100;

/// One step in, from the current level (`None` meaning "fit").
pub(crate) fn zoom_in(current: Option<u32>) -> u32 {
    match current {
        None => FIT_ZOOM * 6 / 5,
        Some(level) => (level.saturating_mul(6) / 5).min(MAX_ZOOM),
    }
}

/// One step out, from the current level (`None` meaning "fit").
pub(crate) fn zoom_out(current: Option<u32>) -> u32 {
    match current {
        None => FIT_ZOOM * 5 / 6,
        Some(level) => (level * 5 / 6).max(MIN_ZOOM),
    }
}

/// A document's name with any extension trimmed, for naming its archive.
///
/// A document titled `Livret de famille` zips to `Livret de famille.zip`; one
/// an import called `deposit_4713.jpg` should not zip to
/// `deposit_4713.jpg.zip`.
pub(crate) fn archive_stem(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return "document".to_string();
    }
    match trimmed.rsplit_once('.') {
        Some((stem, ext))
            if !stem.is_empty()
                && !ext.is_empty()
                && ext.len() <= 4
                && ext.chars().all(|c| c.is_ascii_alphanumeric()) =>
        {
            stem.to_string()
        }
        _ => trimmed.to_string(),
    }
}

/// The name a media should be saved under.
///
/// `file_name` is what the record calls the file and is the right answer when
/// it looks like a file name. It often is not: a Geneanet deposit is titled
/// "Mariage de Pierre E\u{2026}", and a browser handed that as a download name
/// saves a file the operating system will not open by double-click, because
/// nothing says it is a JPEG. Where the name carries no usable extension, the
/// MIME type supplies one.
pub(crate) fn download_name(file_name: &str, mime_type: &str) -> String {
    let trimmed = file_name.trim();
    let stem = if trimmed.is_empty() { "media" } else { trimmed };
    let has_extension = stem.rsplit_once('.').is_some_and(|(before, ext)| {
        !before.is_empty()
            && !ext.is_empty()
            && ext.len() <= 4
            && ext.chars().all(|c| c.is_ascii_alphanumeric())
    });
    if has_extension {
        return stem.to_string();
    }
    match extension_for_mime(mime_type) {
        Some(ext) => format!("{stem}.{ext}"),
        None => stem.to_string(),
    }
}

/// The conventional extension for a MIME type.
fn extension_for_mime(mime_type: &str) -> Option<&'static str> {
    Some(match mime_type.split(';').next()?.trim() {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/tiff" => "tif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        "image/svg+xml" => "svg",
        "application/pdf" => "pdf",
        "video/mp4" => "mp4",
        "video/quicktime" => "mov",
        "audio/mpeg" => "mp3",
        "audio/ogg" => "ogg",
        "audio/wav" | "audio/x-wav" => "wav",
        "text/plain" => "txt",
        _ => return None,
    })
}

/// Isolated so a keystroke in this short field cannot re-render the entire
/// media editor and its place, note, event and vignette sections.
#[component]
fn MediaTagForm(on_add: EventHandler<String>) -> Element {
    let i18n = use_i18n();

    rsx! {
        form {
            class: "pf-subform media-tag-form",
            onsubmit: move |event: Event<FormData>| {
                event.prevent_default();
                if let Some(FormValue::Text(value)) = event.get_first("media-tag") {
                    let tag = value.trim().to_string();
                    if !tag.is_empty() {
                        on_add.call(tag);
                    }
                }
            },
            div { class: "form-group",
                label { {i18n.t("media.tag")} }
                input {
                    r#type: "text",
                    name: "media-tag",
                    placeholder: i18n.t("media.tag_placeholder"),
                    autocomplete: "off",
                    spellcheck: "false",
                }
            }
            button {
                class: "pf-confirm-btn",
                r#type: "submit",
                {i18n.t("media.add_tag")}
            }
        }
    }
}

/// A file size a person can read at a glance.
fn format_size(bytes: i64) -> String {
    const KIB: f64 = 1024.0;
    let bytes = bytes.max(0) as f64;
    if bytes < KIB {
        format!("{bytes:.0} B")
    } else if bytes < KIB * KIB {
        format!("{:.0} KB", bytes / KIB)
    } else {
        format!("{:.1} MB", bytes / (KIB * KIB))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_document_shows_every_page() {
        // Nothing to elide, so eliding would only take away destinations.
        assert_eq!(
            page_window(0, 4),
            vec![
                PagerSlot::Page(0),
                PagerSlot::Page(1),
                PagerSlot::Page(2),
                PagerSlot::Page(3)
            ]
        );
    }

    #[test]
    fn a_register_of_hundreds_of_pages_stays_one_row() {
        let slots = page_window(50, 300);
        assert!(
            slots.len() <= 9,
            "the strip must not grow with the document: {slots:?}"
        );
        // Both ends stay reachable, and where the reader is stays visible.
        assert_eq!(slots.first(), Some(&PagerSlot::Page(0)));
        assert_eq!(slots.last(), Some(&PagerSlot::Page(299)));
        assert!(slots.contains(&PagerSlot::Page(50)));
        assert!(slots.contains(&PagerSlot::Gap));
    }

    #[test]
    fn the_ends_have_no_gap_beside_them_when_the_reader_is_there() {
        let slots = page_window(0, 300);
        // At the start the window already reaches page 0, so a gap belongs
        // only on the far side.
        assert_eq!(slots[0], PagerSlot::Page(0));
        assert_eq!(slots[1], PagerSlot::Page(1));
        assert_eq!(slots.iter().filter(|s| **s == PagerSlot::Gap).count(), 1);

        let slots = page_window(299, 300);
        assert_eq!(slots.iter().filter(|s| **s == PagerSlot::Gap).count(), 1);
    }

    #[test]
    fn a_single_skipped_page_is_shown_rather_than_elided() {
        // An ellipsis hiding one page costs the same width as the page and
        // takes away somewhere to go.
        let slots = page_window(4, 8);
        assert!(!slots.contains(&PagerSlot::Gap), "{slots:?}");
        assert!(slots.contains(&PagerSlot::Page(1)));
    }

    #[test]
    fn a_page_beyond_the_end_is_clamped_rather_than_panicking() {
        // Detaching a page while the viewer is open leaves the signal past the
        // end for one render.
        assert_eq!(page_window(99, 3).last(), Some(&PagerSlot::Page(2)));
        assert!(page_window(0, 0).is_empty());
    }

    #[test]
    fn a_download_keeps_an_extension_it_already_has() {
        assert_eq!(download_name("scan.jpg", "image/jpeg"), "scan.jpg");
        assert_eq!(download_name("acte.PDF", "application/pdf"), "acte.PDF");
    }

    #[test]
    fn a_titled_deposit_is_given_the_extension_its_type_implies() {
        // What a Geneanet import produces: a caption, not a file name. Saved
        // as-is, the file will not open by double-click.
        assert_eq!(
            download_name("Mariage de Pierre", "image/jpeg"),
            "Mariage de Pierre.jpg"
        );
        assert_eq!(
            download_name("Livret de famille", "application/pdf"),
            "Livret de famille.pdf"
        );
    }

    #[test]
    fn a_full_stop_in_a_title_is_not_mistaken_for_an_extension() {
        assert_eq!(
            download_name("Acte n. 12 du registre", "image/jpeg"),
            "Acte n. 12 du registre.jpg"
        );
    }

    #[test]
    fn a_type_we_have_no_extension_for_leaves_the_name_alone() {
        assert_eq!(download_name("mystery", "application/x-thing"), "mystery");
        assert_eq!(download_name("", "application/x-thing"), "media");
    }

    #[test]
    fn zooming_in_and_out_stays_within_its_bounds() {
        // Match the pedigree's 1.2 factor around the fitted size.
        assert_eq!(zoom_in(None), 120);
        assert_eq!(zoom_out(None), 83);
        assert_eq!(zoom_in(Some(120)), 144);
        assert_eq!(zoom_out(Some(144)), 120);
        assert_eq!(zoom_in(Some(MAX_ZOOM)), MAX_ZOOM);
        assert_eq!(zoom_out(Some(MIN_ZOOM)), MIN_ZOOM);
        // No overflow at the ceiling, whatever it is set to.
        assert_eq!(zoom_in(Some(u32::MAX)), MAX_ZOOM);
    }

    #[test]
    fn zooming_reaches_far_enough_to_read_a_corner_of_a_scan() {
        // The reason to zoom a register is one word of secretary hand.
        let mut level = zoom_in(None);
        for _ in 0..40 {
            level = zoom_in(Some(level));
        }
        assert_eq!(level, MAX_ZOOM);
        const { assert!(MAX_ZOOM >= 400) };
    }

    #[test]
    fn an_archive_is_named_after_the_document_not_its_first_page() {
        assert_eq!(archive_stem("Livret de famille"), "Livret de famille");
        assert_eq!(archive_stem("deposit_4713.jpg"), "deposit_4713");
        assert_eq!(archive_stem("  "), "document");
    }

    #[test]
    fn sizes_read_in_the_unit_that_fits() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(2048), "2 KB");
        assert_eq!(format_size(5 * 1024 * 1024), "5.0 MB");
    }

    #[test]
    fn an_owner_knows_its_own_wire_spelling() {
        let person = MediaOwner::Person(Uuid::nil());
        let family = MediaOwner::Family(Uuid::nil());
        let event = MediaOwner::Event(Uuid::nil());
        assert_eq!(person.entity_type(), "person");
        assert_eq!(family.entity_type(), "family");
        assert_eq!(event.entity_type(), "event");
        assert!(person.supports_profile());
        assert!(
            !family.supports_profile(),
            "a couple's card shows its spouses' portraits, not its own"
        );
        assert!(!event.supports_profile(), "an event has no portrait to be");
    }
}
