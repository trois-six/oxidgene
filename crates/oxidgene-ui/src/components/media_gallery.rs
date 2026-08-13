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

use crate::api::{ApiClient, CreateMediaLinkBody, MediaWithLink, UpdateMediaBody};
use crate::components::image_cropper::ImageCropper;
use crate::components::media_input::MediaInput;
use crate::components::vignette_linker::VignetteLinker;
use crate::i18n::use_i18n;

/// What the gallery's media are attached to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MediaOwner {
    Person(Uuid),
    Family(Uuid),
}

impl MediaOwner {
    fn entity_type(&self) -> &'static str {
        match self {
            Self::Person(_) => "person",
            Self::Family(_) => "family",
        }
    }

    fn id(&self) -> Uuid {
        match self {
            Self::Person(id) | Self::Family(id) => *id,
        }
    }

    /// Only a person has a profile photo — a family's card shows its spouses'.
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
                    event_id: None,
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
                    on_changed: move |_| revision += 1,
                }
            }
            if !read_only {
                MediaInput {
                    tree_id,
                    on_uploaded: link_uploaded,
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
    on_changed: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();

    let mut confirming = use_signal(|| false);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    let media_id = tile.media.id;
    let link_id = tile.link_id;
    let has_thumbnail = tile.media.thumbnail_key.is_some();
    let thumbnail_url = api.media_thumbnail_url(tree_id, media_id);
    let file_url = api.media_file_url(tree_id, media_id);
    let is_image = tile.is_image();
    let kind = tile.kind_label();
    let caption = tile.caption().to_string();
    let pages = tile.media.page_count;

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
            div { class: "media-thumb",
                if has_thumbnail {
                    img { src: "{thumbnail_url}", alt: "{caption}", loading: "lazy" }
                } else {
                    div { class: "media-thumb-icon",
                        span { class: "media-kind", "{kind}" }
                    }
                }
                if tile.is_profile {
                    span { class: "media-star", title: i18n.t("media.profile_image"), "\u{2605}" }
                }
                if pages > 1 {
                    span { class: "media-pages",
                        {i18n.t_args("media.page_count", &[("count", &pages.to_string())])}
                    }
                }
                div { class: "media-tile-actions",
                    if !read_only && show_profile && is_image {
                        button {
                            class: if tile.is_profile { "media-act is-on" } else { "media-act" },
                            r#type: "button",
                            disabled: busy(),
                            title: i18n.t("media.set_profile_image"),
                            onclick: toggle_profile,
                            "\u{2605}"
                        }
                    }
                    if !read_only && is_image {
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
                    a {
                        class: "media-act",
                        href: "{file_url}",
                        target: "_blank",
                        title: i18n.t("media.open_file"),
                        "\u{2197}"
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
    let mut title = use_signal(|| tile.media.title.clone().unwrap_or_default());
    let mut description = use_signal(|| tile.media.description.clone().unwrap_or_default());
    let mut saving = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut vignette_revision = use_signal(|| 0_u32);

    let vignettes = use_resource({
        let api = api.clone();
        move || {
            let api = api.clone();
            let _ = vignette_revision();
            async move { api.list_media_vignettes(tree_id, media_id).await }
        }
    });

    let save = {
        let api = api.clone();
        move |_| {
            let api = api.clone();
            let title_value = title().trim().to_string();
            let description_value = description().trim().to_string();
            spawn(async move {
                saving.set(true);
                error.set(None);
                // An emptied field clears the column rather than storing "",
                // so "no title" is one state in the database, not two.
                let body = UpdateMediaBody {
                    title: Some((!title_value.is_empty()).then_some(title_value)),
                    description: Some((!description_value.is_empty()).then_some(description_value)),
                };
                match api.update_media(tree_id, media_id, &body).await {
                    Ok(_) => on_changed.call(()),
                    Err(e) => error.set(Some(e.to_string())),
                }
                saving.set(false);
            });
        }
    };

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
                if let Some(dimensions) = dimensions {
                    span { "{dimensions}" }
                }
                span { {format_size(tile.media.file_size)} }
                if tile.media.page_count > 1 {
                    span {
                        {i18n.t_args(
                            "media.page_count",
                            &[("count", &tile.media.page_count.to_string())],
                        )}
                    }
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

            if !crops.is_empty() || !events.is_empty() {
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
                MediaOwner::Family(_) => None,
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
        assert_eq!(person.entity_type(), "person");
        assert_eq!(family.entity_type(), "family");
        assert!(person.supports_profile());
        assert!(
            !family.supports_profile(),
            "a couple's card shows its spouses' portraits, not its own"
        );
    }
}
