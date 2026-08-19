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

use dioxus::prelude::*;
use oxidgene_core::enums::{DocumentCategory, SourceMediaType};
use oxidgene_core::types::Vignette;
use uuid::Uuid;

use crate::api::{
    ApiClient, CreateMediaLinkBody, CreateNoteBody, MediaKind, MediaSource, MediaWithLink,
    UpdateMediaBody, UpdateNoteBody,
};
use crate::components::date_input::{DateInput, DateParts, format_date};
use crate::components::image_cropper::ImageCropper;
use crate::components::media_input::MediaInput;
use crate::components::person_form::render_place_select;
use crate::components::vignette_linker::VignetteLinker;
use crate::i18n::use_i18n;

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
    /// Show the files without offering to change them.
    ///
    /// The person profile page is a reader's view: it shows what is attached
    /// and lets a file be opened, but uploading, cropping, retitling and
    /// detaching all belong to the edit modal. Rendering the same grid with
    /// its controls withheld keeps the two views looking like one gallery,
    /// which is what a reader who then clicks Edit expects to find.
    #[props(default = false)]
    pub read_only: bool,
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
    let read_only = props.read_only;

    // Bumped after every write; the resource re-runs when it changes. Cheaper
    // and less error-prone than mutating a local list in eight handlers and
    // hoping they all agree with the server.
    let mut revision = use_signal(|| 0_u32);
    let on_changed = props.on_changed;
    // Every mutation goes through here rather than touching `revision`
    // directly: a bump the host is not told about is exactly the bug this
    // exists to prevent.
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
        div { class: "media-grid",
            for tile in items.iter().cloned() {
                MediaTile {
                    key: "{tile.media.id}",
                    tree_id,
                    tile: tile.clone(),
                    show_profile: owner.supports_profile(),
                    read_only,
                    is_open: editing() == Some(tile.media.id),
                    on_edit: move |id| {
                        editing.set(if editing() == Some(id) { None } else { Some(id) });
                    },
                    on_crop: move |tile: MediaWithLink| cropping.set(Some(tile)),
                    on_view: move |tile: MediaWithLink| viewing.set(Some(tile)),
                    on_changed: move |_| changed.call(()),
                }
            }
            if !read_only {
                MediaInput {
                    tree_id,
                    on_uploaded: link_uploaded,
                }
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
        if read_only && items.is_empty() && tiles.read_unchecked().is_some() {
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
    read_only: bool,
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
    // Where the right-click menu sits, if it is open.
    let mut menu_at = use_signal(|| None::<(f64, f64)>);

    let media_id = tile.media.id;
    let link_id = tile.link_id;
    let source = tile.source();
    let kind = tile.kind();
    let kind_label = tile.kind_label();
    let caption = tile.caption().to_string();
    let pages = tile.media.page_count;

    // What the tile links to, and what it draws.
    //
    //  - stored:  our own endpoints. A thumbnail if the server made one.
    //  - remote:  the URL itself. No thumbnail — we never fetch it — so an
    //             image is previewed from the original and everything else
    //             falls back to its icon.
    //  - unheld:  nothing to open at all; the icon says what it would be.
    let preview_url = match (source, kind) {
        (MediaSource::Stored, _) if tile.media.thumbnail_key.is_some() => {
            Some(api.media_thumbnail_url(tree_id, media_id))
        }
        // A remote image is previewed from the original: it is the only copy
        // there is, and the browser scales it into the tile anyway.
        (MediaSource::Remote, MediaKind::Image) => Some(tile.media.file_path.clone()),
        _ => None,
    };

    // Called from two places — the hover button and the right-click menu —
    // so it takes no ownership of anything it cannot clone.
    let toggle_profile = use_callback({
        let api = api.clone();
        let currently = tile.is_profile;
        move |()| {
            let api = api.clone();
            spawn(async move {
                busy.set(true);
                match api
                    .set_profile_media_link(tree_id, link_id, !currently)
                    .await
                {
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

    let tile_for_crop = tile.clone();
    let is_profile = tile.is_profile;

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
                    if !show_profile {
                        return;
                    }
                    e.prevent_default();
                    let point = e.client_coordinates();
                    menu_at.set(Some((point.x, point.y)));
                },
                if let Some(preview) = preview_url.clone() {
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
                if tile.is_profile {
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
                            class: if tile.is_profile { "media-act is-on" } else { "media-act" },
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
            if let Some(err) = error() {
                div { class: "error-msg", "{err}" }
            }

            if let Some((x, y)) = menu_at() {
                div {
                    class: "context-menu-backdrop",
                    onclick: move |_| menu_at.set(None),
                    oncontextmenu: move |e: Event<MouseData>| {
                        e.prevent_default();
                        menu_at.set(None);
                    },
                }
                div { class: "context-menu", style: "left: {x}px; top: {y}px;",
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
    let mut source_media_type = use_signal(|| tile.media.source_media_type);
    let mut document_category = use_signal(|| tile.media.document_category);
    let mut note_text = use_signal(String::new);
    let mut note_id = use_signal(|| None::<Uuid>);
    let mut loaded_note = use_signal(|| false);
    let mut saving = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut vignette_revision = use_signal(|| 0_u32);
    let mut link_revision = use_signal(|| 0_u32);
    let mut page_revision = use_signal(|| 0_u32);

    let vignettes = use_resource({
        let api = api.clone();
        move || {
            let api = api.clone();
            let _ = vignette_revision();
            async move { api.list_media_vignettes(tree_id, media_id).await }
        }
    });

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

    let save = {
        let api = api.clone();
        move |_| {
            let api = api.clone();
            let title_value = title().trim().to_string();
            let description_value = description().trim().to_string();
            let url_value = url().trim().to_string();
            let place_value = Uuid::parse_str(place_id().trim()).ok();
            let note_value = note_text().trim().to_string();
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
                    Ok(()) => on_changed.call(()),
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

    let crops: Vec<Vignette> = match &*vignettes.read_unchecked() {
        Some(Ok(list)) => list.clone(),
        _ => Vec::new(),
    };

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

            if !crops.is_empty() || (tile.is_croppable() && !events.is_empty()) {
                div { class: "media-panel-section",
                    label { {i18n.t("media.vignettes")} }
                    VignetteLinker {
                        tree_id,
                        vignettes: crops,
                        events: events.clone(),
                        on_changed: move |_| vignette_revision += 1,
                    }
                }
            }

            if let Some(err) = error() {
                div { class: "error-msg", "{err}" }
            }
            div { class: "media-panel-actions",
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
                    let thumbnail = api.media_thumbnail_url(tree_id, page_id);
                    let name = page.file_name.clone();
                    rsx! {
                        div { key: "{page_id}", class: "doc-page",
                            span { class: "doc-page-number", "{index + 1}" }
                            div { class: "doc-page-thumb",
                                if has_thumbnail {
                                    img { src: "{thumbnail}", alt: "{name}", loading: "lazy" }
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
fn MediaFacts(tree_id: Uuid, media: oxidgene_core::types::Media) -> Element {
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
    // Who is identified on this scan, and what it stands as evidence for.
    let vignettes = use_resource({
        let api = api.clone();
        move || {
            let api = api.clone();
            async move { api.list_media_vignettes(tree_id, media_id).await.ok() }
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
    let identified: Vec<String> = vignettes
        .read_unchecked()
        .as_ref()
        .and_then(|v| v.as_ref())
        .map(|list| {
            list.iter()
                .filter(|v| v.person_id.is_some())
                .filter_map(|v| v.title.clone())
                .collect()
        })
        .unwrap_or_default();

    rsx! {
        div { class: "media-facts",
            if let Some(title) = media.title.as_ref().filter(|t| !t.trim().is_empty()) {
                div { class: "media-fact",
                    span { class: "media-fact-label", {i18n.t("media.title")} }
                    span { class: "media-fact-value", "{title}" }
                }
            }
            if !date.is_empty() {
                div { class: "media-fact",
                    span { class: "media-fact-label", {i18n.t("media.date")} }
                    span { class: "media-fact-value", "{date}" }
                }
            }
            if let Some(place) = place {
                div { class: "media-fact",
                    span { class: "media-fact-label", {i18n.t("media.place")} }
                    span { class: "media-fact-value", "{place}" }
                }
            }
            if let Some(category) = media.document_category {
                div { class: "media-fact",
                    span { class: "media-fact-label", {i18n.t("media.document_category")} }
                    span { class: "media-fact-value",
                        {i18n.t(&format!("media.category.{}", category.as_str()))}
                    }
                }
            }
            // The physical medium is only worth a line when it says something
            // the reader did not already know from the category or the file.
            if media.source_media_type != oxidgene_core::enums::SourceMediaType::Other
                && media.document_category.map(|c| c.implied_medium())
                    != Some(media.source_media_type)
            {
                div { class: "media-fact",
                    span { class: "media-fact-label", {i18n.t("media.source_media_type")} }
                    span { class: "media-fact-value",
                        {i18n.t(&format!("media.medium.{}", media.source_media_type.as_str()))}
                    }
                }
            }
            if let Some(description) = media.description.as_ref().filter(|d| !d.trim().is_empty()) {
                div { class: "media-fact is-prose",
                    span { class: "media-fact-label", {i18n.t("media.description")} }
                    p { class: "media-fact-value", "{description}" }
                }
            }
            if let Some(note) = note.filter(|n| !n.trim().is_empty()) {
                div { class: "media-fact is-prose",
                    span { class: "media-fact-label", {i18n.t("media.note")} }
                    p { class: "media-fact-value", "{note}" }
                }
            }
            if !identified.is_empty() {
                div { class: "media-fact is-prose",
                    span { class: "media-fact-label", {i18n.t("media.identified")} }
                    div { class: "media-fact-tags",
                        for name in identified.iter() {
                            span { key: "{name}", class: "media-fact-tag", "{name}" }
                        }
                    }
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

    let source = tile.source();
    let caption = tile.caption().to_string();
    let is_document = tile.media.is_document;
    let mut page = use_signal(|| 0_usize);
    // Zoom as a percentage of the natural size; `None` is "fit to the stage",
    // which is where a viewer starts and what the stage's own CSS does. A
    // scan's whole point is the handwriting in one corner, so this goes well
    // past 100%.
    let mut zoom = use_signal(|| None::<u32>);

    // A document has no bytes of its own: what is shown is its current page,
    // which is a media in its own right. Everything below therefore reads the
    // page when there is one and the media itself when there is not.
    let pages = use_resource({
        let api = api.clone();
        let document_id = tile.media.id;
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
        None => tile.kind(),
    };
    let url = match (shown, source) {
        (Some(page), _) => Some(api.media_file_url(tree_id, page.id)),
        (None, MediaSource::Stored) => Some(api.media_file_url(tree_id, tile.media.id)),
        (None, MediaSource::Remote) => Some(tile.media.file_path.clone()),
        (None, MediaSource::Unheld) => None,
    };

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

                // Zoom belongs to images alone: a video and an audio track
                // have their own controls, and a fallback has nothing to
                // magnify.
                if matches!(kind, MediaKind::Image) && url.is_some() {
                    div { class: "media-zoom",
                        button {
                            class: "media-zoom-btn",
                            r#type: "button",
                            title: i18n.t("media.zoom_out"),
                            disabled: zoom().is_some_and(|z| z <= MIN_ZOOM),
                            onclick: move |_| zoom.set(Some(zoom_out(zoom()))),
                            "\u{2212}"
                        }
                        button {
                            class: if zoom().is_none() { "media-zoom-level is-fit" } else { "media-zoom-level" },
                            r#type: "button",
                            title: i18n.t("media.zoom_fit"),
                            onclick: move |_| zoom.set(None),
                            match zoom() {
                                Some(level) => format!("{level}\u{2009}%"),
                                None => i18n.t("media.zoom_fit"),
                            }
                        }
                        button {
                            class: "media-zoom-btn",
                            r#type: "button",
                            title: i18n.t("media.zoom_in"),
                            disabled: zoom().is_some_and(|z| z >= MAX_ZOOM),
                            onclick: move |_| zoom.set(Some(zoom_in(zoom()))),
                            "+"
                        }
                    }
                }

                div { class: "media-viewer-body",
                aside { class: "media-viewer-aside",
                    if editing() {
                        MediaEditPanel {
                            tree_id,
                            tile: tile.clone(),
                            events: events.clone(),
                            embedded: true,
                            on_changed: move |()| on_changed.call(()),
                            on_close: move |()| editing.set(false),
                        }
                    } else {
                        MediaFacts {
                            tree_id,
                            media: shown.cloned().unwrap_or_else(|| tile.media.clone()),
                        }
                        if !read_only {
                            button {
                                class: "btn btn-outline media-facts-edit",
                                r#type: "button",
                                onclick: move |_| editing.set(true),
                                {i18n.t("common.edit")}
                            }
                        }
                    }
                div { class: "media-viewer-main",
                div {
                    class: if zoom().is_some() { "media-viewer-stage is-zoomed" } else { "media-viewer-stage" },
                    match (url.clone(), kind) {
                        (Some(url), MediaKind::Image) => rsx! {
                            img {
                                class: "media-viewer-image",
                                src: "{url}",
                                alt: "{caption}",
                                // Fit is the stage's own CSS; a zoom level
                                // overrides it and lets the stage scroll,
                                // which is what makes a corner readable.
                                style: match zoom() {
                                    Some(level) => format!(
                                        "width: {level}%; max-width: none; max-height: none;",
                                    ),
                                    None => String::new(),
                                },
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
                }

                div { class: "cropper-foot",
                    if let Some(description) = tile.media.description.as_ref() {
                        p { class: "media-viewer-desc", "{description}" }
                    }
                    div { class: "cropper-actions",
                        if let Some(url) = url.clone() {
                            DownloadMediaButton {
                                url,
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
                                url: api.media_archive_url(tree_id, tile.media.id),
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
    url: String,
    file_name: String,
    /// What the button says. Defaults to "Download file"; the archive button
    /// passes its own, since two identical buttons side by side would leave
    /// the reader guessing which is which.
    label: Option<String>,
) -> Element {
    let i18n = use_i18n();
    let label = label.unwrap_or_else(|| i18n.t("media.download_file"));

    #[cfg(target_arch = "wasm32")]
    {
        rsx! {
            a {
                class: "btn btn-outline",
                href: "{url}",
                target: "_blank",
                download: file_name,
                {label}
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut busy = use_signal(|| false);
        let mut error = use_signal(|| None::<String>);

        let save = move |_| {
            let url = url.clone();
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
                match reqwest::get(&url).await {
                    Ok(response) => match response.bytes().await {
                        Ok(bytes) => {
                            if let Err(e) = tokio::fs::write(target.path(), &bytes).await {
                                error.set(Some(e.to_string()));
                            }
                        }
                        Err(e) => error.set(Some(e.to_string())),
                    },
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

/// Zoom bounds and step, as percentages of natural size.
///
/// The ceiling is high on purpose: the reason to zoom a parish register is to
/// read one word of secretary hand in a corner, and 200% does not get there.
pub(crate) const MIN_ZOOM: u32 = 25;
pub(crate) const MAX_ZOOM: u32 = 800;

/// Where "fit" sits when the reader first steps away from it. Not 100%: on a
/// scan larger than the stage, fit is already well below that, and jumping
/// straight to full size would feel like a leap rather than a step.
const FIT_ZOOM: u32 = 100;

/// One step in, from the current level (`None` meaning "fit").
pub(crate) fn zoom_in(current: Option<u32>) -> u32 {
    match current {
        None => FIT_ZOOM,
        Some(level) => (level.saturating_mul(3) / 2).min(MAX_ZOOM),
    }
}

/// One step out, from the current level (`None` meaning "fit").
pub(crate) fn zoom_out(current: Option<u32>) -> u32 {
    match current {
        None => FIT_ZOOM * 2 / 3,
        Some(level) => (level * 2 / 3).max(MIN_ZOOM),
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
        // From fit, the first step is a step and not a leap.
        assert_eq!(zoom_in(None), 100);
        assert_eq!(zoom_out(None), 66);
        assert_eq!(zoom_in(Some(100)), 150);
        assert_eq!(zoom_out(Some(150)), 100);
        assert_eq!(zoom_in(Some(MAX_ZOOM)), MAX_ZOOM);
        assert_eq!(zoom_out(Some(MIN_ZOOM)), MIN_ZOOM);
        // No overflow at the ceiling, whatever it is set to.
        assert_eq!(zoom_in(Some(u32::MAX)), MAX_ZOOM);
    }

    #[test]
    fn zooming_reaches_far_enough_to_read_a_corner_of_a_scan() {
        // The reason to zoom a register is one word of secretary hand.
        let mut level = zoom_in(None);
        for _ in 0..12 {
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
