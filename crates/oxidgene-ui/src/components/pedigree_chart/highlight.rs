//! Highlighting a tag in the tree views (docs/ui-genealogy-tree.md): the sidebar's list
//! of the tags persons carry, the chip that ends the highlight, and the marks
//! and dimming every chart draws for the chosen tag.
//!
//! The chart provides the marks through a context signal, as it provides the
//! portraits: each card reads the signal and draws its own mark, so the
//! charts need no new props.

use std::collections::HashSet;

use dioxus::prelude::*;
use oxidgene_core::types::RecordKind;
use uuid::Uuid;

use crate::api::{ApiClient, ApiError, TagSummary};
use crate::components::modal::Modal;
use crate::components::tags::{Vocabulary, dot_style, filter_matches, tag_colour};
use crate::i18n::{I18n, use_i18n};
use crate::ui_observability::use_ui_resource;

/// The tagged records read per request. The server's page size cap is 100.
const CARRIER_PAGE_SIZE: u64 = 100;

/// The class of a card drawn dimmed while a tag is highlighted. Printing
/// resets it, so a printed chart keeps its marks and not its dimming.
pub(super) const DIM_CLASS: &str = "ped-tag-dim";

/// The list of tags above which the dialog gets a search field. A shorter
/// list stays as plain as it was.
const SEARCH_MIN_TAGS: usize = 10;

/// Whether a list of `count` tags gets its search field.
pub(super) fn shows_search(count: usize) -> bool {
    count > SEARCH_MIN_TAGS
}

/// The tags of `carried` that the search field `typed` leaves, in their order.
/// An empty filter leaves them all.
pub(super) fn listed_tags(carried: &[TagSummary], typed: &str) -> Vec<TagSummary> {
    carried
        .iter()
        .filter(|tag| filter_matches(&tag.label, typed))
        .cloned()
        .collect()
}

/// The highlight the reader chose: the tag and whether the other cards are
/// dimmed. Kept in the tree page's history entry, not in the settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct TagHighlight {
    pub(super) tag_id: Uuid,
    pub(super) dim: bool,
}

/// What the charts draw for the highlighted tag, once its carriers are known.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ChartMarks {
    /// The tag's palette slot.
    pub(super) color: i16,
    /// The persons carrying the tag.
    pub(super) carriers: HashSet<Uuid>,
    pub(super) dim: bool,
}

/// The carriers read for the highlighted tag, keyed by that tag so a result
/// read for a previous highlight is never drawn under the new one. `None`
/// while nothing is highlighted.
pub(super) type CarrierRead = Result<Option<(Uuid, HashSet<Uuid>)>, ApiError>;

/// The palette slot drawn for a highlighted tag missing from the loaded
/// vocabulary (deleted elsewhere since it was read): the default tag colour,
/// so the carriers are still marked.
const UNKNOWN_TAG_COLOR: i16 = 0;

/// What the charts draw for `choice`: its tag's colour and the carriers read
/// for it. `None` while the carriers are not known yet, or while the carriers
/// on hand were read for another tag. A tag missing from `tags` is drawn in
/// the default colour rather than not at all. The caller passes the
/// vocabulary only once it is read, so no mark takes a colour before then.
pub(super) fn highlight_marks(
    choice: Option<TagHighlight>,
    tags: &[TagSummary],
    carriers: &Option<CarrierRead>,
) -> Option<ChartMarks> {
    let choice = choice?;
    let color = tags
        .iter()
        .find(|tag| tag.id == choice.tag_id)
        .map_or(UNKNOWN_TAG_COLOR, |tag| tag.color);
    match carriers {
        Some(Ok(Some((tag_id, carriers)))) if *tag_id == choice.tag_id => Some(ChartMarks {
            color,
            carriers: carriers.clone(),
            dim: choice.dim,
        }),
        _ => None,
    }
}

/// The marks the charts read, provided by the chart being rendered.
#[derive(Clone, Copy)]
struct ChartHighlight(Signal<Option<ChartMarks>>);

/// Provides `marks` to the cards of the chart being rendered, and keeps them
/// current when a later render passes others. Only the cards reading the
/// signal are redrawn when it changes.
pub(super) fn use_chart_highlight(marks: Option<ChartMarks>) {
    let mut provided = use_context_provider(|| ChartHighlight(Signal::new(marks.clone()))).0;
    if *provided.peek() != marks {
        provided.set(marks);
    }
}

/// How one card is drawn under the highlight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct CardMark {
    /// The tag's colour, as a theme token reference.
    pub(super) colour: &'static str,
    /// Whether the person carries the tag: their card gets the dot and the
    /// border.
    pub(super) carrier: bool,
    /// Whether the card is dimmed: the highlight dims the others.
    pub(super) dim: bool,
}

/// How the card of `person` is drawn, or `None` while nothing is highlighted.
pub(super) fn card_mark(person: Uuid) -> Option<CardMark> {
    let ChartHighlight(marks) = try_use_context::<ChartHighlight>()?;
    let marks = marks.read();
    marks.as_ref().map(|marks| mark_of(marks, person))
}

/// How the card of `person` is drawn under `marks`.
fn mark_of(marks: &ChartMarks, person: Uuid) -> CardMark {
    let carrier = marks.carriers.contains(&person);
    CardMark {
        colour: tag_colour(marks.color),
        carrier,
        dim: marks.dim && !carrier,
    }
}

/// The class a card adds for its dimming, empty when it is not dimmed.
pub(super) fn dim_class(mark: Option<CardMark>) -> &'static str {
    match mark {
        Some(CardMark { dim: true, .. }) => DIM_CLASS,
        _ => "",
    }
}

/// The stroke of a carrier's border, `None` for anyone else.
pub(super) fn mark_stroke(mark: Option<CardMark>) -> Option<&'static str> {
    match mark {
        Some(CardMark {
            colour,
            carrier: true,
            ..
        }) => Some(colour),
        _ => None,
    }
}

/// The dot a carrier's card draws at `(x, y)`, in the card's own frame; nothing
/// for anyone else.
pub(super) fn mark_dot(mark: Option<CardMark>, x: f64, y: f64, r: f64) -> Element {
    match mark_stroke(mark) {
        Some(colour) => rsx! {
            circle {
                class: "ped-tag-dot",
                cx: "{x:.2}",
                cy: "{y:.2}",
                r: "{r:.2}",
                style: "fill:{colour};stroke:var(--pn-bg);stroke-width:1;pointer-events:none",
            }
        },
        None => rsx! {},
    }
}

/// Every person carrying `tag_id`, read a page of the tagged records at a time.
async fn load_carriers(
    api: &ApiClient,
    tree_id: Uuid,
    tag_id: Uuid,
) -> Result<HashSet<Uuid>, ApiError> {
    let mut carriers = HashSet::new();
    let mut after: Option<String> = None;
    loop {
        let page = api
            .list_tagged_records(
                tree_id,
                tag_id,
                RecordKind::Person,
                CARRIER_PAGE_SIZE,
                after.as_deref(),
            )
            .await?;
        carriers.extend(
            page.edges
                .iter()
                .map(|edge| edge.node.person_id.unwrap_or(edge.node.id)),
        );
        match page.page_info.end_cursor {
            Some(cursor) if page.page_info.has_next_page => after = Some(cursor),
            _ => return Ok(carriers),
        }
    }
}

/// The carriers of the highlighted tag: `None` while nothing is highlighted.
/// Reading the highlight in the closure makes the read follow each change.
pub(super) fn use_carriers(
    tree_id: Option<Uuid>,
    highlight: Signal<Option<TagHighlight>>,
) -> Resource<CarrierRead> {
    let api = use_context::<ApiClient>();
    use_ui_resource("tag_carriers", move || {
        let api = api.clone();
        let choice = highlight();
        async move {
            match (choice, tree_id) {
                (Some(choice), Some(tree_id)) => load_carriers(&api, tree_id, choice.tag_id)
                    .await
                    .map(|carriers| Some((choice.tag_id, carriers))),
                _ => Ok(None),
            }
        }
    })
}

/// What the dialog says in place of the list while it has nothing to list: the
/// failed read, the read in progress, or that no person carries a tag yet.
/// `None` once there is a list to show.
fn empty_notice(i18n: &I18n, vocabulary: &Vocabulary, none_carried: bool) -> Option<Element> {
    match vocabulary {
        Vocabulary::Failed(message) => Some(rsx! {
            p { class: "tag-picker-error", role: "alert", "{message}" }
        }),
        Vocabulary::Loading => Some(rsx! {
            p { class: "tag-picker-empty", {i18n.t("tag.picker.loading")} }
        }),
        _ if none_carried => Some(rsx! {
            p { class: "tag-picker-empty", {i18n.t("tag.highlight.empty")} }
        }),
        _ => None,
    }
}

/// The "Highlight a tag" button of the tree's sidebar, its chip while a tag
/// is highlighted, and the list of the tags persons carry that opens from the
/// button. `vocabulary` is the tree's vocabulary, with its counts, read once
/// the list is first opened (`on_open`).
#[component]
pub(super) fn HighlightTool(
    highlight: Signal<Option<TagHighlight>>,
    vocabulary: Vocabulary,
    carriers_failed: bool,
    on_open: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let mut open = use_signal(|| false);
    // The "Dim the others" choice made before a tag is picked; once a tag is
    // highlighted, the choice is the highlight's own.
    let mut dim_pending = use_signal(|| true);
    // The search field's text, cleared each time the dialog opens.
    let mut query = use_signal(String::new);
    let tags: Vec<TagSummary> = match &vocabulary {
        Vocabulary::Ready(tags) => tags.clone(),
        _ => Vec::new(),
    };

    let choice = highlight();
    let dim = choice.map_or(dim_pending(), |choice| choice.dim);
    let chosen = choice.and_then(|choice| tags.iter().find(|tag| tag.id == choice.tag_id).cloned());
    // A highlight whose tag is missing from the loaded vocabulary (deleted
    // elsewhere, or not loaded yet) still gets its chip, so it can be ended.
    // A failed read says the tag is unavailable; otherwise it is only unnamed.
    let unnamed_key = if matches!(vocabulary, Vocabulary::Failed(_)) {
        "tag.highlight.unavailable"
    } else {
        "tag.highlight.unknown"
    };
    let chip_label = chosen
        .as_ref()
        .map_or_else(|| i18n.t(unnamed_key), |tag| tag.label.clone());
    let chip_clear = i18n.t_args("tag.highlight.clear", &[("label", &chip_label)]);
    let carried: Vec<TagSummary> = tags
        .iter()
        .filter(|tag| tag.counts.persons > 0)
        .cloned()
        .collect();
    let open_label = i18n.t("tag.highlight.open");
    let search = shows_search(carried.len());
    // The text only filters while the field shows: a list that shrinks back
    // to the plain size is never left filtered by a text nobody can see.
    let typed = if search { query() } else { String::new() };
    let listed = listed_tags(&carried, &typed);
    let filter_label = i18n.t("tag.highlight.filter");
    let button_class = if choice.is_some() {
        "isb-btn isb-btn-active"
    } else {
        "isb-btn"
    };

    rsx! {
        button {
            class: "{button_class}",
            r#type: "button",
            title: "{open_label}",
            "aria-label": "{open_label}",
            "aria-haspopup": "dialog",
            onclick: move |_| {
                on_open.call(());
                query.set(String::new());
                open.set(true);
            },
            svg {
                width: "16",
                height: "16",
                fill: "none",
                "viewBox": "0 0 24 24",
                stroke: "currentColor",
                "strokeWidth": "2",
                path { d: "M20.6 13.4 13.4 20.6a2 2 0 0 1-2.8 0L3 13V3h10l7.6 7.6a2 2 0 0 1 0 2.8z" }
                circle { cx: "7.5", cy: "7.5", r: "1.5" }
            }
        }

        if choice.is_some() {
            div { class: "tag-highlight-chip",
                if let Some(tag) = chosen.as_ref() {
                    span { class: "tag-dot", style: dot_style(tag.color), aria_hidden: "true" }
                }
                span { class: "tag-highlight-chip-label", title: "{chip_label}", "{chip_label}" }
                button {
                    class: "tag-highlight-chip-clear",
                    r#type: "button",
                    title: "{chip_clear}",
                    "aria-label": "{chip_clear}",
                    onclick: move |_| highlight.set(None),
                    "\u{00D7}"
                }
            }
        }
        if carriers_failed {
            p { class: "tag-picker-error", role: "alert", {i18n.t("tag.highlight.failed")} }
        }

        if open() {
            Modal {
                class: "modal-card tag-highlight-card",
                label: i18n.t("tag.highlight.title"),
                on_close: move |()| open.set(false),
                h3 { {i18n.t("tag.highlight.title")} }
                label { class: "tag-highlight-dim",
                    input {
                        r#type: "checkbox",
                        checked: dim,
                        onchange: move |event: Event<FormData>| {
                            let next = event.checked();
                            dim_pending.set(next);
                            if let Some(choice) = highlight() {
                                highlight.set(Some(TagHighlight { dim: next, ..choice }));
                            }
                        },
                    }
                    span { {i18n.t("tag.highlight.dim")} }
                }
                if let Some(notice) = empty_notice(&i18n, &vocabulary, carried.is_empty()) {
                    {notice}
                } else {
                    if search {
                        input {
                            r#type: "text",
                            class: "tag-picker-input",
                            placeholder: "{filter_label}",
                            aria_label: "{filter_label}",
                            autocomplete: "off",
                            spellcheck: "false",
                            value: "{typed}",
                            onmounted: move |event: Event<MountedData>| {
                                spawn(async move {
                                    let _ = event.set_focus(true).await;
                                });
                            },
                            oninput: move |event: Event<FormData>| query.set(event.value()),
                        }
                    }
                    if listed.is_empty() {
                        p { class: "tag-picker-empty", {i18n.t("tag.highlight.no_match")} }
                    } else {
                        div { class: "tag-highlight-list",
                            for tag in listed.iter() {
                                {
                                    let tag_id = tag.id;
                                    let pressed = choice.is_some_and(|choice| choice.tag_id == tag_id);
                                    let count = i18n.t_plural("tag.count.persons", tag.counts.persons as usize);
                                    rsx! {
                                        button {
                                            key: "{tag_id}",
                                            class: if pressed { "tag-highlight-option is-active" } else { "tag-highlight-option" },
                                            r#type: "button",
                                            "aria-pressed": "{pressed}",
                                            onclick: move |_| {
                                                // The dimming the reader sees: the current highlight's own
                                                // choice when one is set, the pending choice otherwise.
                                                let dim = highlight().map_or(dim_pending(), |choice| choice.dim);
                                                highlight.set(Some(TagHighlight { tag_id, dim }));
                                                open.set(false);
                                            },
                                            span { class: "tag-dot", style: dot_style(tag.color), aria_hidden: "true" }
                                            span { class: "tag-highlight-label", "{tag.label}" }
                                            span { class: "tag-highlight-count", "{count}" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marks(dim: bool) -> ChartMarks {
        ChartMarks {
            color: 3,
            carriers: HashSet::from([Uuid::from_u128(1)]),
            dim,
        }
    }

    fn tag(id: u128, persons: i64) -> TagSummary {
        TagSummary {
            id: Uuid::from_u128(id),
            label: "Example".to_string(),
            color: 3,
            counts: crate::api::TagCounts {
                persons,
                families: 0,
                events: 0,
                places: 0,
                sources: 0,
                citations: 0,
                repositories: 0,
                media: 0,
                notes: 0,
            },
        }
    }

    fn named(id: u128, label: &str) -> TagSummary {
        TagSummary {
            label: label.to_string(),
            ..tag(id, 1)
        }
    }

    #[test]
    fn the_search_field_shows_only_above_ten_tags() {
        assert!(!shows_search(3));
        assert!(!shows_search(10));
        assert!(shows_search(11));
    }

    #[test]
    fn the_search_leaves_the_tags_whose_folded_label_contains_the_filter() {
        let carried = [
            named(1, "À vérifier"),
            named(2, "Soldats 1914"),
            named(3, "Zoé"),
        ];
        let labels = |typed: &str| -> Vec<String> {
            listed_tags(&carried, typed)
                .into_iter()
                .map(|tag| tag.label)
                .collect()
        };
        assert_eq!(labels("a verifier"), ["À vérifier"]);
        assert_eq!(labels("SOLDATS"), ["Soldats 1914"]);
        assert_eq!(labels("zoe"), ["Zoé"]);
        assert_eq!(labels(""), ["À vérifier", "Soldats 1914", "Zoé"]);
        assert!(labels("inconnu").is_empty());
    }

    #[test]
    fn a_carrier_is_marked_and_never_dimmed() {
        let carrier = mark_of(&marks(true), Uuid::from_u128(1));
        assert!(carrier.carrier);
        assert!(!carrier.dim);
        assert_eq!(carrier.colour, "var(--tag-3)");
    }

    #[test]
    fn the_others_are_dimmed_unless_the_reader_turned_dimming_off() {
        let other = mark_of(&marks(true), Uuid::from_u128(2));
        assert!(!other.carrier);
        assert!(other.dim);
        assert!(!mark_of(&marks(false), Uuid::from_u128(2)).dim);
    }

    #[test]
    fn nothing_is_drawn_until_the_carriers_are_known() {
        let choice = Some(TagHighlight {
            tag_id: Uuid::from_u128(9),
            dim: true,
        });
        let tags = [tag(9, 1)];
        assert_eq!(highlight_marks(choice, &tags, &None), None);
        assert_eq!(highlight_marks(choice, &tags, &Some(Ok(None))), None);
        let carriers = Some(Ok(Some((
            Uuid::from_u128(9),
            HashSet::from([Uuid::from_u128(1)]),
        ))));
        assert_eq!(highlight_marks(choice, &tags, &carriers), Some(marks(true)));
        assert_eq!(highlight_marks(None, &tags, &carriers), None);
    }

    #[test]
    fn carriers_read_for_another_tag_are_not_drawn() {
        let choice = Some(TagHighlight {
            tag_id: Uuid::from_u128(9),
            dim: true,
        });
        let tags = [tag(9, 1), tag(8, 1)];
        // The previous highlight's carriers stay on hand while the new tag's
        // are loading: they must not be drawn in the new tag's colour.
        let previous = Some(Ok(Some((
            Uuid::from_u128(8),
            HashSet::from([Uuid::from_u128(1)]),
        ))));
        assert_eq!(highlight_marks(choice, &tags, &previous), None);
    }

    #[test]
    fn a_tag_missing_from_the_vocabulary_still_marks_its_carriers() {
        let choice = Some(TagHighlight {
            tag_id: Uuid::from_u128(9),
            dim: true,
        });
        // The vocabulary failed to load or the tag was deleted elsewhere: the
        // carriers loaded fine, so they are drawn in the default colour.
        let carriers = Some(Ok(Some((
            Uuid::from_u128(9),
            HashSet::from([Uuid::from_u128(1)]),
        ))));
        let found = highlight_marks(choice, &[], &carriers).expect("the carriers are known");
        assert_eq!(found.color, UNKNOWN_TAG_COLOR);
        assert_eq!(found.carriers, HashSet::from([Uuid::from_u128(1)]));
        assert!(found.dim);
    }

    #[test]
    fn the_highlight_follows_the_tag_colour() {
        let choice = Some(TagHighlight {
            tag_id: Uuid::from_u128(9),
            dim: false,
        });
        let carriers = Some(Ok(Some((Uuid::from_u128(9), HashSet::new()))));
        let mut known = tag(9, 0);
        known.color = 7;
        let found = highlight_marks(choice, &[known], &carriers).expect("the tag is known");
        assert_eq!(found.color, 7);
        assert!(!found.dim);
    }
}
