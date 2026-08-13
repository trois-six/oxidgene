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
use oxidgene_core::types::Vignette;
use uuid::Uuid;

use crate::api::{
    ApiClient, CreateMediaLinkBody, CreateNoteBody, MediaKind, MediaSource, MediaWithLink,
    UpdateMediaBody, UpdateNoteBody,
};
use crate::components::date_input::{DateInput, DateParts};
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
    let mut editing = use_signal(|| None::<Uuid>);
    let mut cropping = use_signal(|| None::<MediaWithLink>);
    let mut viewing = use_signal(|| None::<MediaWithLink>);
    let mut error = use_signal(|| None::<String>);

    let tiles = use_resource({
        let api = api.clone();
        move || {
            let api = api.clone();
            let _ = revision();
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
                revision += 1;
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
                        revision += 1;
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
                    on_changed: move |_| revision += 1,
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
                on_changed: move |_| revision += 1,
                on_close: move |_| editing.set(None),
            }
        }

        if let Some(tile) = viewing() {
            MediaViewer {
                tree_id,
                tile,
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
                    revision += 1;
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
    let file_url = match source {
        MediaSource::Stored => Some(api.media_file_url(tree_id, media_id)),
        MediaSource::Remote => Some(tile.media.file_path.clone()),
        MediaSource::Unheld => None,
    };
    let preview_url = match (source, kind) {
        (MediaSource::Stored, _) if tile.media.thumbnail_key.is_some() => {
            Some(api.media_thumbnail_url(tree_id, media_id))
        }
        // A remote image is previewed from the original: it is the only copy
        // there is, and the browser scales it into the tile anyway.
        (MediaSource::Remote, MediaKind::Image) => Some(tile.media.file_path.clone()),
        _ => None,
    };

    let toggle_profile = {
        let api = api.clone();
        let currently = tile.is_profile;
        move |_| {
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
    };

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
                            onclick: toggle_profile,
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
                    if let Some(url) = file_url.clone() {
                        a {
                            class: "media-act",
                            href: "{url}",
                            target: "_blank",
                            // `download` on a format the browser cannot render
                            // saves the file instead of navigating to a page
                            // that would show nothing.
                            download: (!kind.is_embeddable()).then(|| caption.clone()),
                            title: if kind.is_embeddable() {
                                i18n.t("media.open_file")
                            } else {
                                i18n.t("media.download_file")
                            },
                            {if kind.is_embeddable() { "\u{2197}" } else { "\u{2913}" }}
                        }
                    }
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
        }
    }
}

// ── Inline edit panel ───────────────────────────────────────────────

#[component]
fn MediaEditPanel(
    tree_id: Uuid,
    tile: MediaWithLink,
    events: Vec<(Uuid, String)>,
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
        div { class: "media-panel",
            div { class: "media-panel-head",
                span { class: "media-panel-title", "{tile.media.file_name}" }
                button {
                    class: "cropper-close",
                    r#type: "button",
                    onclick: move |_| on_close.call(()),
                    "\u{00D7}"
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
#[component]
fn MediaViewer(tree_id: Uuid, tile: MediaWithLink, on_close: EventHandler<()>) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();

    let source = tile.source();
    let caption = tile.caption().to_string();
    let is_document = tile.media.is_document;
    let mut page = use_signal(|| 0_usize);

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

                div { class: "media-viewer-stage",
                    match (url.clone(), kind) {
                        (Some(url), MediaKind::Image) => rsx! {
                            img { class: "media-viewer-image", src: "{url}", alt: "{caption}" }
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
                            onclick: move |_| page.set(0),
                            "\u{23EE}"
                        }
                        button {
                            class: "media-pager-btn",
                            r#type: "button",
                            disabled: current == 0,
                            title: i18n.t("media.previous_page"),
                            onclick: move |_| page.set(current.saturating_sub(1)),
                            "\u{25C0}"
                        }
                        div { class: "media-pager-numbers",
                            for index in 0..total_pages {
                                button {
                                    key: "{index}",
                                    class: if index == current {
                                        "media-pager-num is-current"
                                    } else {
                                        "media-pager-num"
                                    },
                                    r#type: "button",
                                    onclick: move |_| page.set(index),
                                    "{index + 1}"
                                }
                            }
                        }
                        button {
                            class: "media-pager-btn",
                            r#type: "button",
                            disabled: current + 1 >= total_pages,
                            title: i18n.t("media.next_page"),
                            onclick: move |_| page.set((current + 1).min(total_pages - 1)),
                            "\u{25B6}"
                        }
                        button {
                            class: "media-pager-btn",
                            r#type: "button",
                            disabled: current + 1 >= total_pages,
                            title: i18n.t("media.last_page"),
                            onclick: move |_| page.set(total_pages - 1),
                            "\u{23ED}"
                        }
                    }
                }

                div { class: "cropper-foot",
                    if let Some(description) = tile.media.description.as_ref() {
                        p { class: "media-viewer-desc", "{description}" }
                    }
                    div { class: "cropper-actions",
                        if let Some(url) = url.clone() {
                            a {
                                class: "btn btn-outline",
                                href: "{url}",
                                target: "_blank",
                                download: caption.clone(),
                                {i18n.t("media.download_file")}
                            }
                            // Desktop only: the embedded WebView has no
                            // download UI of its own, so `download` on a link
                            // does nothing there. A native save dialog is the
                            // only way a desktop user gets the file out.
                            SaveMediaButton { url: url.clone(), file_name: caption.clone() }
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

/// "Save as\u{2026}" through the platform's own dialog. Desktop only.
///
/// On the web build this renders nothing: the browser already owns downloading,
/// and a second button next to its own would be worse than none.
#[component]
fn SaveMediaButton(url: String, file_name: String) -> Element {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (url, file_name);
        rsx! {}
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let i18n = use_i18n();
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
                if busy() { {i18n.t("common.saving")} } else { {i18n.t("media.save_as")} }
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
