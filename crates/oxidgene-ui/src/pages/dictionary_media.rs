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

    // ── Tag cloud ──
    let (min, max) = facets_value
        .tags
        .iter()
        .fold((i64::MAX, 0_i64), |(min, max), tag| {
            (min.min(tag.count), max.max(tag.count))
        });
    let is_selected = |tag: &MediaTagFacet| {
        current
            .tags
            .iter()
            .any(|selected| selected.to_lowercase() == tag.tag.to_lowercase())
    };

    // ── List state ──
    let per_page = page_size();
    let page = cursors.read().len() + 1;
    let list_read = list.read_unchecked();
    let (items, total, next_cursor, error) = match &*list_read {
        Some(Ok(connection)) => (
            Some(
                connection
                    .edges
                    .iter()
                    .map(|edge| edge.node.clone())
                    .collect::<Vec<_>>(),
            ),
            connection.total_count.max(0) as usize,
            connection
                .page_info
                .has_next_page
                .then(|| connection.page_info.end_cursor.clone())
                .flatten(),
            None,
        ),
        Some(Err(ApiError::Api { status: 400, .. })) => (
            None,
            0,
            None,
            Some(i18n.t("dictionary.media.invalid_range")),
        ),
        Some(Err(_)) => (None, 0, None, Some(i18n.t("dictionary.error"))),
        None => (None, 0, None, None),
    };
    drop(list_read);
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
            div {
                class: "dict-media-cloud",
                role: "group",
                aria_label: i18n.t("dictionary.media.tags"),
                button {
                    class: if current.tags.is_empty() { "dict-letter-btn active" } else { "dict-letter-btn" },
                    onclick: move |_| draft.write().tags.clear(),
                    {i18n.t("dictionary.letter_all")}
                }
                for tag in facets_value.tags.iter() {
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
                                // A click adds the tag to the selection, or
                                // takes a selected one back out.
                                onclick: move |_| {
                                    let mut draft = draft.write();
                                    if active {
                                        draft.tags.retain(|t| t.to_lowercase() != value.to_lowercase());
                                    } else {
                                        draft.tags.push(value.clone());
                                    }
                                },
                                "{tag.tag}"
                                span { class: "dict-media-tag-count", "{tag.count}" }
                            }
                        }
                    }
                }
            }
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
            div { class: "sr-filters pf-embedded",
                div { class: "sr-filter-grid sr-filter-grid-event",
                    div { class: "sr-filter-group",
                        label { {i18n.t("dictionary.media.kind")} }
                        select {
                            value: current.kind.map(|kind| kind.as_str()).unwrap_or_default(),
                            onchange: move |e: Event<FormData>| draft.write().kind = MediaFileKind::parse(&e.value()),
                            option { value: "", {i18n.t("dictionary.media.kind_any")} }
                            for facet in facets_value.kinds.iter() {
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
                            for facet in facets_value.categories.iter() {
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

        if has_filters {
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
                if !current.event_from.trim().is_empty() || !current.event_to.trim().is_empty() {
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
                if !current.added_from.is_empty() || !current.added_to.is_empty() {
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
                    onclick: clear_all,
                    {i18n.t("dictionary.media.clear_all")}
                }
            }
        }

        if let Some(message) = error {
            div { class: "sr-empty", "{message}" }
        } else if items.is_none() {
            div { class: "sr-empty", {i18n.t("dictionary.loading")} }
        } else if tiles.is_empty() && *applied.read() == MediaListFilters::default() {
            div { class: "sr-empty", {i18n.t("dictionary.media.none")} }
        } else if tiles.is_empty() {
            div { class: "sr-empty",
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

        if pages > 1 {
            div { class: "sr-pagination",
                button {
                    class: "sr-page-btn",
                    disabled: page <= 1,
                    aria_label: i18n.t("dictionary.media.previous_page"),
                    onclick: move |_| {
                        cursors.write().pop();
                    },
                    "\u{25C0}"
                }
                span { class: "sr-page-info",
                    {i18n.t_args("dictionary.media.page", &[("page", &page.to_string()), ("pages", &pages.to_string())])}
                }
                button {
                    class: "sr-page-btn",
                    disabled: next_cursor.is_none(),
                    aria_label: i18n.t("dictionary.media.next_page"),
                    onclick: move |_| {
                        if let Some(cursor) = next_cursor.clone() {
                            cursors.write().push(cursor);
                        }
                    },
                    "\u{25B6}"
                }
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
