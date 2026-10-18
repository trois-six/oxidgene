//! Tags on records (docs/tags.md §6.1 and §6.3): the picker that puts a tag
//! on a record or takes it off, and the chips and dots that show a record's
//! tags. The picker is one component for every kind of record.

use dioxus::prelude::*;
use oxidgene_core::search::fold_words;
use oxidgene_core::types::{RecordKind, TAG_COLOR_SLOTS, TagRef};
use uuid::Uuid;

use crate::api::{ApiClient, ApiError, TagSummary};
use crate::components::context_menu::ContextMenuSurface;
use crate::i18n::{I18n, use_i18n};
use crate::router::Route;
use crate::ui_observability::use_ui_resource;

/// Dots a row shows before the `+n` that counts the rest.
const DOT_LIMIT: usize = 3;

/// The colour of each palette slot, as a `var()` reference to its theme
/// token. The references are spelled out here so the stylesheet guard
/// finds each one and checks that the themes define it.
const TAG_VARS: [&str; 12] = [
    "var(--tag-0)",
    "var(--tag-1)",
    "var(--tag-2)",
    "var(--tag-3)",
    "var(--tag-4)",
    "var(--tag-5)",
    "var(--tag-6)",
    "var(--tag-7)",
    "var(--tag-8)",
    "var(--tag-9)",
    "var(--tag-10)",
    "var(--tag-11)",
];

const _: () = assert!(TAG_VARS.len() == TAG_COLOR_SLOTS as usize);

/// The colour a new tag takes: the slot the fewest of the tree's tags use,
/// the lowest index on a tie, so the first twelve tags all differ
/// (docs/tags.md §2.1).
pub(crate) fn next_color(tags: &[TagSummary]) -> i16 {
    (0..TAG_COLOR_SLOTS)
        .min_by_key(|slot| tags.iter().filter(|tag| tag.color == *slot).count())
        .unwrap_or(0)
}

/// The palette slot a tag draws with, kept inside the twelve declared.
fn tag_slot(color: i16) -> usize {
    color.clamp(0, TAG_COLOR_SLOTS - 1) as usize
}

/// The inline style that paints a dot in a tag's colour.
pub(crate) fn dot_style(color: i16) -> String {
    format!("background: {};", TAG_VARS[tag_slot(color)])
}

/// Remembers the element that has focus before the picker takes it, so
/// Escape can give the focus back to it.
const REMEMBER_OPENER_JS: &str = "window.oxgTagOpener = document.activeElement;";

/// Gives the focus back to the element remembered by [`REMEMBER_OPENER_JS`].
const RESTORE_OPENER_JS: &str = "const opener = window.oxgTagOpener; \
    window.oxgTagOpener = null; \
    if (opener && typeof opener.focus === 'function') { opener.focus(); }";

/// Scrolls the option with this DOM id into its listbox, so the keyboard
/// position stays visible.
fn scroll_option_into_view(id: &str) {
    let _ = document::eval(&format!(
        "document.getElementById('{id}')?.scrollIntoView({{ block: 'nearest' }});"
    ));
}

/// The DOM id of the option at `index` in the picker of a record.
fn option_dom_id(record_id: Uuid, index: usize) -> String {
    format!("tag-picker-{record_id}-{index}")
}

/// The label of a tag as the picker matches it.
fn folded(label: &str) -> String {
    fold_words(label)
}

/// Keeps a record's tags in the order the vocabulary lists them.
fn sort_tags(tags: &mut [TagRef]) {
    tags.sort_by_key(|tag| folded(&tag.label));
}

/// The vocabulary tags the filter `typed` leaves, the tags the record carries
/// first, and whether the typed label is new, so the picker offers to
/// create it.
fn picker_options(
    vocabulary: &[TagSummary],
    carried: &[TagRef],
    typed: &str,
) -> (Vec<TagSummary>, bool) {
    let typed = typed.trim();
    let needle = folded(typed);
    let mut options: Vec<TagSummary> = vocabulary
        .iter()
        .filter(|tag| folded(&tag.label).contains(&needle))
        .cloned()
        .collect();
    // The tags the record carries come first; the stable sort keeps the
    // vocabulary's order within each group.
    options.sort_by_key(|tag| !carried.iter().any(|carried| carried.id == tag.id));
    let create = !typed.is_empty() && !vocabulary.iter().any(|tag| folded(&tag.label) == needle);
    (options, create)
}

/// What a failed tag write says, translated from the server's error code.
pub(crate) fn error_message(i18n: &I18n, error: &ApiError) -> String {
    let key = match error.code().as_deref() {
        Some("conflict") => "tag.error.conflict",
        Some("validation_error") => "tag.error.validation",
        Some("not_found") => "tag.error.not_found",
        _ => "tag.error.generic",
    };
    i18n.t(key)
}

/// The keys the picker answers to, reduced to what it does with each.
enum Step {
    Move(usize),
    Activate,
    Close,
}

/// The step a key asks for. Space toggles only where the list has focus:
/// in the filter field it is a letter of the search.
fn key_step(key: &Key, in_list: bool, active: usize, count: usize) -> Option<Step> {
    match key {
        Key::ArrowDown => Some(Step::Move((active + 1).min(count.saturating_sub(1)))),
        Key::ArrowUp => Some(Step::Move(active.saturating_sub(1))),
        Key::Enter => Some(Step::Activate),
        Key::Character(typed) if in_list && typed == " " => Some(Step::Activate),
        Key::Escape => Some(Step::Close),
        _ => None,
    }
}

/// Props of [`TagPicker`].
#[derive(Props, Clone, PartialEq)]
pub struct TagPickerProps {
    pub tree_id: Uuid,
    pub kind: RecordKind,
    pub record_id: Uuid,
    /// The tags the record carries now.
    pub tags: Vec<TagRef>,
    /// Where the menu opens, in client coordinates.
    pub x: f64,
    pub y: f64,
    pub on_close: EventHandler<()>,
    /// The record's tags after each write, in vocabulary order.
    pub on_changed: EventHandler<Vec<TagRef>>,
}

/// The tag picker: a filter over the tree's vocabulary, a checkbox per tag
/// and a line that creates the typed label. Checking a tag writes at once.
#[component]
pub fn TagPicker(props: TagPickerProps) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let tree_id = props.tree_id;
    let kind = props.kind;
    let record_id = props.record_id;
    let on_changed = props.on_changed;

    // Bumped when a tag is created, so the vocabulary is read again.
    let mut revision = use_signal(|| 0_u32);
    let vocabulary = {
        let api = api.clone();
        use_ui_resource("tag_vocabulary", move || {
            let api = api.clone();
            let _ = revision();
            async move { api.list_tags(tree_id).await }
        })
    };

    let mut carried = use_signal(|| {
        let mut tags = props.tags.clone();
        sort_tags(&mut tags);
        tags
    });
    let mut query = use_signal(String::new);
    let mut active = use_signal(|| 0_usize);
    let mut error = use_signal(|| None::<String>);

    let (tags, load_failure) = vocabulary_of(&i18n, &vocabulary.read_unchecked());
    let loading = vocabulary.read_unchecked().is_none();

    let carried_now = carried();
    let typed = query().trim().to_string();
    let (options, create) = picker_options(&tags, &carried_now, &typed);
    let count = options.len() + usize::from(create);
    let list_id = format!("tag-picker-{record_id}");
    let option_id = |index: usize| option_dom_id(record_id, index);
    let active_index = active().min(count.saturating_sub(1));
    let active_id = (count > 0).then(|| option_id(active_index));

    // Checks or unchecks a tag: the line follows at once, and a failed
    // write takes it back and says why.
    let api_for_create = api.clone();
    let toggle = use_callback(move |tag: TagRef| {
        let api = api.clone();
        let on = !carried.read().iter().any(|carried| carried.id == tag.id);
        carried.with_mut(|list| {
            if on {
                list.push(tag.clone());
                sort_tags(list);
            } else {
                list.retain(|carried| carried.id != tag.id);
            }
        });
        spawn(async move {
            let written = if on {
                api.tag_record(tree_id, tag.id, kind, record_id).await
            } else {
                api.untag_record(tree_id, tag.id, kind, record_id).await
            };
            match written {
                Ok(()) => {
                    error.set(None);
                    on_changed.call(carried.read().clone());
                }
                Err(err) => {
                    carried.with_mut(|list| {
                        if on {
                            list.retain(|carried| carried.id != tag.id);
                        } else {
                            list.push(tag.clone());
                            sort_tags(list);
                        }
                    });
                    error.set(Some(error_message(&i18n, &err)));
                }
            }
        });
    });

    // Creates the typed label and checks it on the record.
    let create_tag = use_callback(move |label: String| {
        let api = api_for_create.clone();
        spawn(async move {
            let summary = match api.create_tag(tree_id, &label, None).await {
                Ok(summary) => summary,
                Err(err) => {
                    error.set(Some(error_message(&i18n, &err)));
                    return;
                }
            };
            revision += 1;
            let tag = TagRef {
                id: summary.id,
                label: summary.label,
                color: summary.color,
            };
            match api.tag_record(tree_id, tag.id, kind, record_id).await {
                Ok(()) => {
                    carried.with_mut(|list| {
                        if !list.iter().any(|carried| carried.id == tag.id) {
                            list.push(tag);
                            sort_tags(list);
                        }
                    });
                    query.set(String::new());
                    active.set(0);
                    error.set(None);
                    on_changed.call(carried.read().clone());
                }
                Err(err) => error.set(Some(error_message(&i18n, &err))),
            }
        });
    });

    // Enter or Space on option `index`: the tag, or the create line.
    let create_index = options.len();
    let activate_options = options.clone();
    let activate_typed = typed.clone();
    let activate = use_callback(move |index: usize| {
        if let Some(tag) = activate_options.get(index) {
            toggle.call(TagRef {
                id: tag.id,
                label: tag.label.clone(),
                color: tag.color,
            });
        } else if create && index == create_index {
            create_tag.call(activate_typed.clone());
        }
    });

    let mut on_key = move |event: Event<KeyboardData>, in_list: bool| {
        let Some(step) = key_step(&event.key(), in_list, active(), count) else {
            return;
        };
        event.prevent_default();
        match step {
            Step::Move(index) => {
                active.set(index);
                scroll_option_into_view(&option_dom_id(record_id, index));
            }
            Step::Activate => activate.call(active_index),
            Step::Close => {
                let _ = document::eval(RESTORE_OPENER_JS);
                props.on_close.call(());
            }
        }
    };

    let filter_label = i18n.t("tag.picker.filter");
    let list_label = i18n.t("tag.picker.title");
    let empty_text = picker_empty_text(&i18n, loading, load_failure);
    let create_text = i18n.t_args("tag.picker.create", &[("label", &typed)]);

    rsx! {
        ContextMenuSurface {
            x: props.x,
            y: props.y,
            menu_class: "tag-picker",
            on_close: props.on_close,
            div { class: "tag-picker-title", {i18n.t("tag.picker.title")} }
            input {
                r#type: "text",
                class: "tag-picker-input",
                placeholder: "{filter_label}",
                aria_label: "{filter_label}",
                aria_controls: "{list_id}",
                aria_activedescendant: active_id.clone(),
                autocomplete: "off",
                spellcheck: "false",
                value: "{query}",
                onmounted: move |event: Event<MountedData>| {
                    let _ = document::eval(REMEMBER_OPENER_JS);
                    spawn(async move {
                        let _ = event.set_focus(true).await;
                    });
                },
                oninput: move |event: Event<FormData>| {
                    query.set(event.value());
                    active.set(0);
                },
                onkeydown: move |event: Event<KeyboardData>| on_key(event, false),
            }
            if let Some(message) = error() {
                p { class: "tag-picker-error", role: "alert", "{message}" }
            }
            div {
                id: "{list_id}",
                class: "tag-picker-list",
                role: "listbox",
                tabindex: "0",
                aria_label: "{list_label}",
                aria_multiselectable: "true",
                aria_activedescendant: active_id,
                onkeydown: move |event: Event<KeyboardData>| on_key(event, true),
                TagPickerRows {
                    record_id,
                    options: options.clone(),
                    carried: carried_now.clone(),
                    active,
                    active_index,
                    create,
                    create_index,
                    create_text: create_text.clone(),
                    toggle,
                    activate,
                }
                if count == 0 {
                    div { class: "tag-picker-empty", "{empty_text}" }
                }
            }
        }
    }
}

/// The vocabulary the picker shows, and why it shows none when it failed.
fn vocabulary_of(
    i18n: &I18n,
    read: &Option<Result<Vec<TagSummary>, ApiError>>,
) -> (Vec<TagSummary>, Option<String>) {
    match read {
        Some(Ok(tags)) => (tags.clone(), None),
        Some(Err(err)) => (Vec::new(), Some(error_message(i18n, err))),
        None => (Vec::new(), None),
    }
}

/// What the empty list says: that the vocabulary is loading, why it failed,
/// or that there is no tag yet.
fn picker_empty_text(i18n: &I18n, loading: bool, failure: Option<String>) -> String {
    if loading {
        i18n.t("tag.picker.loading")
    } else if let Some(message) = failure {
        message
    } else {
        i18n.t("tag.picker.empty")
    }
}

/// The rows of the picker's list: one checkable line per tag, then the line
/// creating the typed label when no tag folds to it.
#[component]
fn TagPickerRows(
    record_id: Uuid,
    options: Vec<TagSummary>,
    carried: Vec<TagRef>,
    mut active: Signal<usize>,
    active_index: usize,
    create: bool,
    create_index: usize,
    create_text: String,
    toggle: Callback<TagRef>,
    activate: Callback<usize>,
) -> Element {
    let option_id = |index: usize| option_dom_id(record_id, index);
    rsx! {
        for (index, tag) in options.iter().enumerate() {
            {
                let checked = carried.iter().any(|carried| carried.id == tag.id);
                let row = TagRef { id: tag.id, label: tag.label.clone(), color: tag.color };
                rsx! {
                    div {
                        key: "{tag.id}",
                        id: "{option_id(index)}",
                        class: if index == active_index { "tag-picker-option is-active" } else { "tag-picker-option" },
                        role: "option",
                        aria_checked: if checked { "true" } else { "false" },
                        onclick: move |_| {
                            active.set(index);
                            toggle.call(row.clone());
                        },
                        span { class: "tag-picker-check", aria_hidden: "true",
                            if checked { "\u{2713}" }
                        }
                        span { class: "tag-dot", style: dot_style(tag.color), aria_hidden: "true" }
                        span { class: "tag-picker-label", "{tag.label}" }
                    }
                }
            }
        }
        if create {
            div {
                id: "{option_id(create_index)}",
                class: if active_index == create_index { "tag-picker-option is-active is-create" } else { "tag-picker-option is-create" },
                role: "option",
                aria_checked: "false",
                onclick: move |_| {
                    active.set(create_index);
                    activate.call(create_index);
                },
                span { class: "tag-picker-check", aria_hidden: "true", "+" }
                span { class: "tag-picker-label", "{create_text}" }
            }
        }
    }
}

/// The icon button that opens the picker on a record, drawn among the
/// record's header actions. The picker opens beside the button.
#[component]
pub fn TagsButton(
    tree_id: Uuid,
    kind: RecordKind,
    record_id: Uuid,
    tags: Vec<TagRef>,
    on_changed: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let mut picker_at = use_signal(|| None::<(f64, f64)>);
    rsx! {
        button {
            class: "btn btn-outline pd-header-action-btn",
            r#type: "button",
            title: i18n.t("tag.picker.open"),
            aria_label: i18n.t("tag.picker.open"),
            onclick: move |event: Event<MouseData>| {
                let at = event.client_coordinates();
                picker_at.set(Some((at.x, at.y)));
            },
            svg {
                class: "pd-header-action-icon",
                width: "16", height: "16", fill: "none", "viewBox": "0 0 24 24",
                stroke: "currentColor", "strokeWidth": "2",
                path { d: "M20.6 13.4 13.4 20.6a2 2 0 0 1-2.8 0L3 13V3h10l7.6 7.6a2 2 0 0 1 0 2.8z" }
                circle { cx: "7.5", cy: "7.5", r: "1.5" }
            }
        }
        if let Some((x, y)) = picker_at() {
            TagPicker {
                tree_id,
                kind,
                record_id,
                tags: tags.clone(),
                x,
                y,
                on_close: move |_| picker_at.set(None),
                on_changed: move |_now: Vec<TagRef>| on_changed.call(()),
            }
        }
    }
}

/// The route that opens the Dictionary's Tags tab on a tag: what a chip or a
/// dot follows (docs/tags.md §6.3).
fn tag_route(tree_id: &str, tag: &TagRef) -> Route {
    Route::DictionaryTag {
        tree_id: tree_id.to_string(),
        tag: tag.id.to_string(),
    }
}

/// The tags of a record as chips: a colour dot and the label, one after the
/// other, each opening the Tags tab on its tag. Nothing is drawn for a record
/// without tags (docs/tags.md §6.3). Chips carry no remove button.
#[component]
pub fn TagChips(tree_id: String, tags: Vec<TagRef>) -> Element {
    let i18n = use_i18n();
    if tags.is_empty() {
        return rsx! {};
    }
    rsx! {
        div { class: "tag-chips",
            for tag in tags.iter() {
                Link {
                    key: "{tag.id}",
                    to: tag_route(&tree_id, tag),
                    class: "tag-chip",
                    aria_label: i18n.t_args("tag.open", &[("label", &tag.label)]),
                    span { class: "tag-dot", style: dot_style(tag.color), aria_hidden: "true" }
                    span { class: "tag-chip-label", "{tag.label}" }
                }
            }
        }
    }
}

/// The tags of a record as dots, at most three and then `+n`. Each dot opens
/// the Tags tab on its tag; its label is in the tooltip and the accessible
/// name, so a dot never carries its tag by colour alone (docs/tags.md §6.3).
#[component]
pub fn TagDots(tree_id: String, tags: Vec<TagRef>) -> Element {
    let i18n = use_i18n();
    if tags.is_empty() {
        return rsx! {};
    }
    let rest = tags.len().saturating_sub(DOT_LIMIT);
    rsx! {
        span { class: "tag-dots",
            for tag in tags.iter().take(DOT_LIMIT) {
                Link {
                    key: "{tag.id}",
                    to: tag_route(&tree_id, tag),
                    class: "tag-dot-link",
                    title: "{tag.label}",
                    aria_label: i18n.t_args("tag.open", &[("label", &tag.label)]),
                    span { class: "tag-dot", style: dot_style(tag.color) }
                }
            }
            if rest > 0 {
                span { class: "tag-dots-more", "+{rest}" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::TagCounts;
    use crate::i18n::Language;

    fn en() -> I18n {
        I18n::new(Language::english())
    }

    fn summary(label: &str, color: i16) -> TagSummary {
        TagSummary {
            id: Uuid::now_v7(),
            label: label.to_string(),
            color,
            counts: TagCounts {
                persons: 0,
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

    fn as_ref(tag: &TagSummary) -> TagRef {
        TagRef {
            id: tag.id,
            label: tag.label.clone(),
            color: tag.color,
        }
    }

    fn labels(options: &[TagSummary]) -> Vec<&str> {
        options.iter().map(|tag| tag.label.as_str()).collect()
    }

    #[test]
    fn space_toggles_only_where_the_list_has_focus() {
        let space = Key::Character(" ".to_string());
        assert!(matches!(key_step(&space, true, 0, 2), Some(Step::Activate)));
        assert!(key_step(&space, false, 0, 2).is_none());
    }

    #[test]
    fn arrows_move_within_the_options() {
        assert!(matches!(
            key_step(&Key::ArrowDown, true, 0, 3),
            Some(Step::Move(1))
        ));
        assert!(matches!(
            key_step(&Key::ArrowDown, true, 2, 3),
            Some(Step::Move(2))
        ));
        assert!(matches!(
            key_step(&Key::ArrowDown, false, 0, 0),
            Some(Step::Move(0))
        ));
        assert!(matches!(
            key_step(&Key::ArrowUp, true, 0, 3),
            Some(Step::Move(0))
        ));
        assert!(matches!(
            key_step(&Key::ArrowUp, true, 2, 3),
            Some(Step::Move(1))
        ));
    }

    #[test]
    fn enter_activates_escape_closes_and_letters_pass_through() {
        assert!(matches!(
            key_step(&Key::Enter, false, 0, 1),
            Some(Step::Activate)
        ));
        assert!(matches!(
            key_step(&Key::Escape, true, 0, 1),
            Some(Step::Close)
        ));
        assert!(key_step(&Key::Character("a".to_string()), true, 0, 1).is_none());
    }

    #[test]
    fn the_carried_tags_come_first_in_vocabulary_order() {
        let alpha = summary("Sample Alpha", 0);
        let beta = summary("Sample Beta", 1);
        let gamma = summary("Sample Gamma", 2);
        let vocabulary = [alpha.clone(), beta.clone(), gamma.clone()];
        let (options, create) = picker_options(&vocabulary, &[as_ref(&gamma)], "");
        assert_eq!(
            labels(&options),
            ["Sample Gamma", "Sample Alpha", "Sample Beta"]
        );
        assert!(!create);
    }

    #[test]
    fn the_filter_ignores_case_and_accents() {
        let verify = summary("À vérifier", 0);
        let vocabulary = [summary("Sample Alpha", 1), verify];
        let (options, _) = picker_options(&vocabulary, &[], "VERIFIER");
        assert_eq!(labels(&options), ["À vérifier"]);
    }

    #[test]
    fn a_typed_label_that_folds_to_a_tag_offers_no_new_one() {
        let vocabulary = [summary("À vérifier", 0)];
        let (_, create) = picker_options(&vocabulary, &[], "a verifier");
        assert!(!create);
        let (_, create) = picker_options(&vocabulary, &[], "Sample Beta");
        assert!(create);
        let (_, create) = picker_options(&vocabulary, &[], "   ");
        assert!(!create);
    }

    #[test]
    fn a_failed_write_names_the_reason_it_was_given() {
        let i18n = en();
        let api_error = |code: &str| ApiError::Api {
            status: 400,
            body: format!(r#"{{"error":"{code}"}}"#),
        };
        assert_eq!(
            error_message(&i18n, &api_error("conflict")),
            i18n.t("tag.error.conflict")
        );
        assert_eq!(
            error_message(&i18n, &api_error("validation_error")),
            i18n.t("tag.error.validation")
        );
        assert_eq!(
            error_message(&i18n, &api_error("not_found")),
            i18n.t("tag.error.not_found")
        );
        assert_eq!(
            error_message(&i18n, &api_error("something_else")),
            i18n.t("tag.error.generic")
        );
    }

    #[test]
    fn a_dot_takes_the_colour_of_its_slot_and_clamps_outside_the_palette() {
        assert_eq!(dot_style(3), "background: var(--tag-3);");
        assert_eq!(dot_style(-5), "background: var(--tag-0);");
        assert_eq!(dot_style(TAG_COLOR_SLOTS), "background: var(--tag-11);");
        assert_eq!(dot_style(99), "background: var(--tag-11);");
    }
}
