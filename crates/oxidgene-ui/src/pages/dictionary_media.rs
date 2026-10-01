//! The Dictionary's Media tab: every document of the tree, paginated by the
//! server, narrowed by a tag cloud, a name filter and a filter panel. See
//! `docs/ui-dictionary.md` (Media Tab).
//!
//! Unlike the other tabs, which load a whole aggregation and page through it
//! locally, a library can hold thousands of scans: the list is the API's
//! cursor pagination, and every filter is sent to the server so it narrows
//! the whole library rather than the page on screen.

use chrono::NaiveDate;
use dioxus::prelude::*;
use oxidgene_core::{DocumentCategory, MediaFileKind, SourceMediaType};
use uuid::Uuid;

use crate::api::{ApiClient, ApiError, MediaListFilters, MediaListItem, MediaTagFacet};
use crate::components::media_gallery::{MediaLibraryGrid, MediaLibraryTile};
use crate::components::pager::Pager;
use crate::i18n::{I18n, use_i18n};
use crate::ui_observability::use_ui_resource;

/// Page sizes offered. The API serves at most 100 per page, and a grid of
/// thumbnails is no place for an "All".
const PAGE_SIZES: [u64; 3] = [25, 50, 100];

/// How long typing pauses before the list is asked again.
const DEBOUNCE_MS: u32 = 250;

/// Font sizes of the tag cloud's least and most used tags, in rem.
const CLOUD_MIN_REM: f64 = 0.78;
const CLOUD_MAX_REM: f64 = 1.5;

/// The filters as the user is editing them. Numbers and dates stay text
/// until sent, so a half-typed year is not rejected mid-keystroke.
#[derive(Debug, Clone, Default, PartialEq)]
struct Draft {
    /// The selected tags, as the cloud spells them; a document must carry
    /// every one.
    tags: Vec<String>,
    kind: Option<MediaFileKind>,
    category: Option<DocumentCategory>,
    name: String,
    linked_name: String,
    event_from: String,
    event_to: String,
    added_from: String,
    added_to: String,
}

impl Draft {
    /// What is sent. Text that does not parse as a year or a date yet is
    /// no constraint rather than an error.
    fn filters(&self) -> MediaListFilters {
        let text = |value: &str| Some(value.trim().to_string()).filter(|value| !value.is_empty());
        let year = |value: &str| value.trim().parse::<i32>().ok();
        let day = |value: &str| NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").ok();
        MediaListFilters {
            tags: self.tags.clone(),
            kind: self.kind,
            category: self.category,
            name: text(&self.name),
            linked_name: text(&self.linked_name),
            event_from: year(&self.event_from),
            event_to: year(&self.event_to),
            added_from: day(&self.added_from),
            added_to: day(&self.added_to),
        }
    }
}

/// A tag's font size and weight in the cloud: scaled on a log of its count
/// between the least and the most used tag, so one tag carried by every
/// document does not shrink all the others to the minimum.
fn cloud_style(count: i64, min: i64, max: i64) -> String {
    let span = (max.max(1) as f64).ln() - (min.max(1) as f64).ln();
    let t = if span > 0.0 {
        ((count.max(1) as f64).ln() - (min.max(1) as f64).ln()) / span
    } else {
        0.0
    };
    let size = CLOUD_MIN_REM + t * (CLOUD_MAX_REM - CLOUD_MIN_REM);
    let weight = 400 + (t * 3.0).round() as i32 * 100;
    format!("font-size: {size:.2}rem; font-weight: {weight};")
}

/// A range for a chip: "1850–1900", with an open end left as an ellipsis.
fn range_label(from: &str, to: &str) -> String {
    let end = |value: &str| {
        let value = value.trim();
        if value.is_empty() {
            "\u{2026}".to_string()
        } else {
            value.to_string()
        }
    };
    format!("{}\u{2013}{}", end(from), end(to))
}

/// The lines under a tile: what kind of record it is, and how much it is used.
fn footnotes(item: &MediaListItem, i18n: &I18n) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(category) = item.media.document_category {
        lines.push(i18n.t(&format!("media.category.{}", category.as_str())));
    } else if item.media.source_media_type != SourceMediaType::Other {
        lines.push(i18n.t(&format!(
            "media.medium.{}",
            item.media.source_media_type.as_str()
        )));
    }
    lines.push(if item.usage_count == 0 {
        i18n.t("dictionary.media.unlinked")
    } else {
        i18n.t_plural("dictionary.media.usage", item.usage_count as usize)
    });
    lines
}

#[component]
pub fn DictionaryMedia(tree_id: Uuid) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();

    let mut draft = use_signal(Draft::default);
    let mut applied = use_signal(MediaListFilters::default);
    let mut page_size = use_signal(|| PAGE_SIZES[0]);
    // The `after` cursor of every page past the first, in order: cursor
    // pagination cannot jump, so going back is popping one.
    let mut cursors = use_signal(Vec::<String>::new);
    let mut revision = use_signal(|| 0_u32);
    let mut show_filters = use_signal(|| false);

    // Props are not reactive; mirrored so the resources follow a tree change.
    let mut tree = use_signal(|| tree_id);
    if *tree.peek() != tree_id {
        tree.set(tree_id);
        draft.set(Draft::default());
        cursors.set(Vec::new());
    }

    // Every edit reaches the list through this pause, so a word typed into
    // the name field is one request, not one per letter. A change of filters
    // goes back to the first page: the old cursors belong to another list.
    let _debounce = use_ui_resource("media_filter_debounce", move || {
        let next = draft().filters();
        async move {
            crate::utils::sleep_ms(DEBOUNCE_MS).await;
            if *applied.peek() != next {
                applied.set(next);
                cursors.set(Vec::new());
            }
        }
    });

    // The cloud counts the tags among the documents carrying the selected
    // ones, so it offers only the tags that still narrow the selection. It
    // follows the selection at once, and no other filter.
    let selected_tags = use_memo(move || draft().tags);
    let api_facets = api.clone();
    let facets = use_ui_resource("media_facets", move || {
        let api = api_facets.clone();
        let _ = revision();
        let tree_id = tree();
        let with_tags = selected_tags();
        async move { api.media_facets(tree_id, &with_tags).await }
    });
    let list = use_ui_resource("media_list", move || {
        let api = api.clone();
        let _ = revision();
        let tree_id = tree();
        let filters = applied();
        let first = page_size();
        let after = cursors().last().cloned();
        async move {
            api.list_media(tree_id, first, after.as_deref(), &filters)
                .await
        }
    });

    let facets_value = facets
        .read_unchecked()
        .as_ref()
        .and_then(|result| result.as_ref().ok())
        .cloned()
        .unwrap_or_default();
    let current = draft();
    let has_filters = current != Draft::default();
    let clear_all = move |_| {
        draft.set(Draft::default());
    };

    // ── List state ──
    let per_page = page_size();
    let page = cursors.read().len() + 1;
    let ListState {
        items,
        total,
        next_cursor,
        error,
    } = ListState::of(list.read_unchecked().as_ref(), &i18n);
    let pages = total.div_ceil(per_page as usize).max(1);
    let tiles: Vec<MediaLibraryTile> = items
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|item| MediaLibraryTile {
            media: item.media.clone(),
            footnotes: footnotes(item, &i18n),
        })
        .collect();

    rsx! {
        if !facets_value.tags.is_empty() {
            MediaTagCloud { tags: facets_value.tags.clone(), draft }
        }

        div { class: "dict-filter-row",
            input {
                r#type: "text",
                class: "dict-filter-input",
                placeholder: "{i18n.t(\"dictionary.media.name_placeholder\")}",
                value: "{current.name}",
                oninput: move |e: Event<FormData>| draft.write().name = e.value(),
            }
            div { class: "dict-page-size",
                select {
                    aria_label: i18n.t("dictionary.media.per_page"),
                    value: "{per_page}",
                    onchange: move |e: Event<FormData>| {
                        page_size.set(e.value().parse().unwrap_or(PAGE_SIZES[0]));
                        cursors.set(Vec::new());
                    },
                    for size in PAGE_SIZES {
                        option { value: "{size}", "{size}" }
                    }
                }
            }
            span { class: "sr-count dict-media-count", {i18n.t_plural("dictionary.media.count", total)} }
        }

        div { class: "sr-filters-toggle",
            button {
                class: "btn btn-outline btn-sm",
                aria_expanded: show_filters(),
                onclick: move |_| show_filters.set(!show_filters()),
                span { class: if show_filters() { "sr-chevron open" } else { "sr-chevron" }, "\u{25BC}" }
                " {i18n.t(\"dictionary.media.filters\")}"
            }
        }
        if show_filters() {
            MediaFilterPanel { facets: facets_value.clone(), draft }
        }

        if has_filters {
            ActiveMediaFilters { draft }
        }

        if let Some(message) = error {
            div { class: "empty-state", "{message}" }
        } else if items.is_none() {
            div { class: "empty-state", {i18n.t("dictionary.loading")} }
        } else if tiles.is_empty() && *applied.read() == MediaListFilters::default() {
            div { class: "empty-state", {i18n.t("dictionary.media.none")} }
        } else if tiles.is_empty() {
            div { class: "empty-state",
                p { {i18n.t("dictionary.media.no_matches")} }
                button { class: "sr-clear-filters", onclick: clear_all, {i18n.t("dictionary.media.clear_all")} }
            }
        } else {
            MediaLibraryGrid {
                tree_id,
                tiles,
                on_changed: move |()| revision += 1,
            }
        }

        // A cursor reads the library a page at a time: its pages can be
        // stepped through, not jumped to.
        Pager {
            current: page - 1,
            total: pages,
            numbered: false,
            on_select: move |index: usize| {
                if index + 1 < page {
                    cursors.write().pop();
                } else if let Some(cursor) = next_cursor.clone() {
                    cursors.write().push(cursor);
                }
            },
        }
    }
}

/// The media list as last loaded: its page, its size, where the next page
/// starts, and why it could not be read.
struct ListState {
    items: Option<Vec<MediaListItem>>,
    total: usize,
    next_cursor: Option<String>,
    error: Option<String>,
}

impl ListState {
    fn of(
        list: Option<&Result<oxidgene_core::types::Connection<MediaListItem>, ApiError>>,
        i18n: &I18n,
    ) -> Self {
        let failed = |key: &str| Self {
            items: None,
            total: 0,
            next_cursor: None,
            error: Some(i18n.t(key)),
        };
        match list {
            Some(Ok(connection)) => Self {
                items: Some(
                    connection
                        .edges
                        .iter()
                        .map(|edge| edge.node.clone())
                        .collect(),
                ),
                total: connection.total_count.max(0) as usize,
                next_cursor: connection
                    .page_info
                    .has_next_page
                    .then(|| connection.page_info.end_cursor.clone())
                    .flatten(),
                error: None,
            },
            Some(Err(ApiError::Api { status: 400, .. })) => {
                failed("dictionary.media.invalid_range")
            }
            Some(Err(_)) => failed("dictionary.error"),
            None => Self {
                items: None,
                total: 0,
                next_cursor: None,
                error: None,
            },
        }
    }
}

/// The tags, each sized by how many documents carry it; a click adds one to
/// the selection, or takes a selected one back out.
#[component]
fn MediaTagCloud(tags: Vec<MediaTagFacet>, draft: Signal<Draft>) -> Element {
    let i18n = use_i18n();
    let (min, max) = tags.iter().fold((i64::MAX, 0_i64), |(min, max), tag| {
        (min.min(tag.count), max.max(tag.count))
    });
    let selected = draft.read().tags.clone();
    let is_selected = |tag: &MediaTagFacet| {
        selected
            .iter()
            .any(|chosen| chosen.to_lowercase() == tag.tag.to_lowercase())
    };
    rsx! {
        div {
            class: "dict-media-cloud",
            role: "group",
            aria_label: i18n.t("dictionary.media.tags"),
            button {
                class: if selected.is_empty() { "dict-letter-btn active" } else { "dict-letter-btn" },
                onclick: move |_| draft.write().tags.clear(),
                {i18n.t("dictionary.letter_all")}
            }
            for tag in tags.iter() {
                {
                    let active = is_selected(tag);
                    let value = tag.tag.clone();
                    let title = i18n.t_plural("dictionary.media.count", tag.count as usize);
                    rsx! {
                        button {
                            key: "{tag.tag}",
                            class: if active { "dict-media-tag active" } else { "dict-media-tag" },
                            style: cloud_style(tag.count, min, max),
                            title: "{title}",
                            aria_pressed: active,
                            onclick: move |_| toggle_tag(draft, &value, active),
                            "{tag.tag}"
                            span { class: "dict-media-tag-count", "{tag.count}" }
                        }
                    }
                }
            }
        }
    }
}

/// Takes `tag` out of the selection when `selected`, else adds it.
fn toggle_tag(mut draft: Signal<Draft>, tag: &str, selected: bool) {
    let mut draft = draft.write();
    if selected {
        draft
            .tags
            .retain(|t| t.to_lowercase() != tag.to_lowercase());
    } else {
        draft.tags.push(tag.to_string());
    }
}

/// The filters beyond the name: the kind of file, the category, a linked
/// name, the years of the events and the days the documents were added.
#[component]
fn MediaFilterPanel(facets: crate::api::MediaFacets, draft: Signal<Draft>) -> Element {
    let i18n = use_i18n();
    let current = draft();
    rsx! {
        div { class: "sr-filters pf-embedded",
            div { class: "sr-filter-grid sr-filter-grid-event",
                div { class: "sr-filter-group",
                    label { {i18n.t("dictionary.media.kind")} }
                    select {
                        value: current.kind.map(|kind| kind.as_str()).unwrap_or_default(),
                        onchange: move |e: Event<FormData>| draft.write().kind = MediaFileKind::parse(&e.value()),
                        option { value: "", {i18n.t("dictionary.media.kind_any")} }
                        for facet in facets.kinds.iter() {
                            option {
                                value: facet.kind.as_str(),
                                {format!("{} ({})", i18n.t(&format!("dictionary.media.kind.{}", facet.kind.as_str())), facet.count)}
                            }
                        }
                    }
                }
                div { class: "sr-filter-group",
                    label { {i18n.t("dictionary.media.category")} }
                    select {
                        value: current.category.map(|category| category.as_str()).unwrap_or_default(),
                        onchange: move |e: Event<FormData>| draft.write().category = DocumentCategory::parse(&e.value()),
                        option { value: "", {i18n.t("dictionary.media.category_any")} }
                        for facet in facets.categories.iter() {
                            option {
                                value: facet.category.as_str(),
                                {format!("{} ({})", i18n.t(&format!("media.category.{}", facet.category.as_str())), facet.count)}
                            }
                        }
                    }
                }
                div { class: "sr-filter-group",
                    label { {i18n.t("dictionary.media.linked_name")} }
                    input {
                        r#type: "text",
                        placeholder: "{i18n.t(\"dictionary.media.linked_name_placeholder\")}",
                        value: "{current.linked_name}",
                        oninput: move |e: Event<FormData>| draft.write().linked_name = e.value(),
                    }
                }
                div { class: "sr-filter-group",
                    label { {i18n.t("dictionary.media.event_years")} }
                    div { class: "sr-date-range",
                        input {
                            r#type: "number",
                            placeholder: "1800",
                            aria_label: i18n.t("dictionary.media.from"),
                            value: "{current.event_from}",
                            oninput: move |e: Event<FormData>| draft.write().event_from = e.value(),
                        }
                        span { "\u{2013}" }
                        input {
                            r#type: "number",
                            placeholder: "1900",
                            aria_label: i18n.t("dictionary.media.to"),
                            value: "{current.event_to}",
                            oninput: move |e: Event<FormData>| draft.write().event_to = e.value(),
                        }
                    }
                }
                div { class: "sr-filter-group",
                    label { {i18n.t("dictionary.media.added")} }
                    div { class: "sr-date-range",
                        // A calendar day on the server's clock, not a
                        // genealogical date: the browser's own picker is
                        // the right control, not the date-phrase input.
                        input {
                            r#type: "date",
                            aria_label: i18n.t("dictionary.media.from"),
                            value: "{current.added_from}",
                            oninput: move |e: Event<FormData>| draft.write().added_from = e.value(),
                        }
                        span { "\u{2013}" }
                        input {
                            r#type: "date",
                            aria_label: i18n.t("dictionary.media.to"),
                            value: "{current.added_to}",
                            oninput: move |e: Event<FormData>| draft.write().added_to = e.value(),
                        }
                    }
                }
            }
        }
    }
}

/// A chip per filter in force, each taking it off, and one to clear them
/// all.
#[component]
fn ActiveMediaFilters(draft: Signal<Draft>) -> Element {
    let i18n = use_i18n();
    let current = draft();
    let events = !current.event_from.trim().is_empty() || !current.event_to.trim().is_empty();
    let added = !current.added_from.is_empty() || !current.added_to.is_empty();
    rsx! {
        div { class: "sr-active-filters",
            for tag in current.tags.clone() {
                button {
                    key: "tag-{tag}",
                    class: "sr-filter-chip",
                    onclick: {
                        let tag = tag.clone();
                        move |_| draft.write().tags.retain(|t| *t != tag)
                    },
                    {format!("{}: {tag}", i18n.t("dictionary.media.tag"))}
                    span { " \u{00D7}" }
                }
            }
            if !current.name.trim().is_empty() {
                button {
                    class: "sr-filter-chip",
                    onclick: move |_| draft.write().name.clear(),
                    {format!("{}: {}", i18n.t("dictionary.media.name"), current.name.trim())}
                    span { " \u{00D7}" }
                }
            }
            if let Some(kind) = current.kind {
                button {
                    class: "sr-filter-chip",
                    onclick: move |_| draft.write().kind = None,
                    {format!("{}: {}", i18n.t("dictionary.media.kind"), i18n.t(&format!("dictionary.media.kind.{}", kind.as_str())))}
                    span { " \u{00D7}" }
                }
            }
            if let Some(category) = current.category {
                button {
                    class: "sr-filter-chip",
                    onclick: move |_| draft.write().category = None,
                    {format!("{}: {}", i18n.t("dictionary.media.category"), i18n.t(&format!("media.category.{}", category.as_str())))}
                    span { " \u{00D7}" }
                }
            }
            if !current.linked_name.trim().is_empty() {
                button {
                    class: "sr-filter-chip",
                    onclick: move |_| draft.write().linked_name.clear(),
                    {format!("{}: {}", i18n.t("dictionary.media.linked_name"), current.linked_name.trim())}
                    span { " \u{00D7}" }
                }
            }
            if events {
                button {
                    class: "sr-filter-chip",
                    onclick: move |_| {
                        let mut draft = draft.write();
                        draft.event_from.clear();
                        draft.event_to.clear();
                    },
                    {format!("{}: {}", i18n.t("dictionary.media.event_years"), range_label(&current.event_from, &current.event_to))}
                    span { " \u{00D7}" }
                }
            }
            if added {
                button {
                    class: "sr-filter-chip",
                    onclick: move |_| {
                        let mut draft = draft.write();
                        draft.added_from.clear();
                        draft.added_to.clear();
                    },
                    {format!("{}: {}", i18n.t("dictionary.media.added"), range_label(&current.added_from, &current.added_to))}
                    span { " \u{00D7}" }
                }
            }
            button {
                class: "pf-row-btn",
                onclick: move |_| draft.set(Draft::default()),
                {i18n.t("dictionary.media.clear_all")}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_half_typed_filter_is_no_constraint() {
        let draft = Draft {
            event_from: "18".into(),
            event_to: "19x".into(),
            added_from: "2026-13".into(),
            name: "   ".into(),
            ..Draft::default()
        };
        let filters = draft.filters();
        assert_eq!(filters.event_from, Some(18));
        assert_eq!(filters.event_to, None);
        assert_eq!(filters.added_from, None);
        assert_eq!(filters.name, None);
    }

    #[test]
    fn the_cloud_scales_between_its_bounds() {
        assert!(cloud_style(1, 1, 50).starts_with("font-size: 0.78rem; font-weight: 400"));
        assert!(cloud_style(50, 1, 50).starts_with("font-size: 1.50rem; font-weight: 700"));
        assert!(
            cloud_style(3, 3, 3).starts_with("font-size: 0.78rem"),
            "one count everywhere is no reason to shout"
        );
    }

    #[test]
    fn an_open_range_end_reads_as_an_ellipsis() {
        assert_eq!(range_label("1850", ""), "1850\u{2013}\u{2026}");
        assert_eq!(range_label("", "1900"), "\u{2026}\u{2013}1900");
    }
}
