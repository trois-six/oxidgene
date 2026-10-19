//! Tags on records (docs/ui-common.md §4.18): the picker that puts a tag
//! on a record or takes it off, and the chips and dots that show a record's
//! tags. The picker is one component for every kind of record.

use std::collections::HashMap;

use dioxus::prelude::*;
use oxidgene_core::search::fold_words;
use oxidgene_core::types::{RecordKind, TAG_COLOR_SLOTS, TagRef};
use uuid::Uuid;

use crate::api::{ApiClient, ApiError, BulkTagAction, TagSummary, TagTarget};
use crate::components::context_menu::ContextMenuSurface;
use crate::components::modal::Modal;
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
/// (docs/data-model.md, Tag).
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
    format!("background: {};", tag_colour(color))
}

/// The `var()` reference to the theme token of a tag's colour, for SVG
/// fills and strokes.
pub(crate) fn tag_colour(color: i16) -> &'static str {
    TAG_VARS[tag_slot(color)]
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

/// A bulk change over more persons than this asks for confirmation first
/// (docs/ui-common.md §4.18).
const CONFIRM_ABOVE: i64 = 100;

/// The deepest generation a branch reaches (docs/ui-common.md §4.18).
const MAX_DEPTH: u8 = 30;

/// How many of a target's persons carry each tag.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct BulkCounts {
    total: i64,
    per_tag: HashMap<Uuid, i64>,
}

impl BulkCounts {
    /// How many of the persons carry `tag`.
    fn carried(&self, tag: Uuid) -> i64 {
        self.per_tag.get(&tag).copied().unwrap_or(0)
    }
}

/// Reads the counts of the target. The server counts each person of the
/// target once, whatever the target spans.
async fn read_counts(
    api: &ApiClient,
    tree_id: Uuid,
    target: &TagTarget,
) -> Result<BulkCounts, ApiError> {
    let part = api.tag_target(tree_id, target).await?;
    Ok(BulkCounts {
        total: part.total,
        per_tag: part
            .tags
            .into_iter()
            .map(|tag| (tag.tag_id, tag.count))
            .collect(),
    })
}

/// The counts a read gave, or the message that says why there are none.
fn counts_of(
    i18n: &I18n,
    read: &Option<Result<BulkCounts, ApiError>>,
) -> (Option<BulkCounts>, Option<String>) {
    match read {
        Some(Ok(counts)) => (Some(counts.clone()), None),
        Some(Err(err)) => (None, Some(error_message(i18n, err))),
        None => (None, None),
    }
}

/// The change waiting for confirmation, its action, and the number of persons
/// it writes.
type PendingChange = Option<(TagRef, BulkTagAction, i64)>;

/// The number of persons an action writes: those without the tag for Add,
/// those carrying it for Remove.
fn changed_persons(action: BulkTagAction, total: i64, carried: i64) -> i64 {
    match action {
        BulkTagAction::Add => (total - carried).max(0),
        BulkTagAction::Remove => carried.max(0),
    }
}

/// Whether a change writing `changed` persons asks for confirmation first.
fn needs_confirmation(changed: i64) -> bool {
    changed > CONFIRM_ABOVE
}

/// Starts a change: at once, or once confirmed when it writes many persons.
fn ask(
    changed: i64,
    mut pending: Signal<PendingChange>,
    apply: Callback<(TagRef, BulkTagAction)>,
    tag: TagRef,
    action: BulkTagAction,
) {
    if needs_confirmation(changed) {
        pending.set(Some((tag, action, changed)));
    } else {
        apply.call((tag, action));
    }
}

/// Props of [`BulkTagPicker`].
#[derive(Props, Clone, PartialEq)]
pub struct BulkTagPickerProps {
    pub tree_id: Uuid,
    /// The persons the picker acts on. Fixed when the picker opens.
    pub target: TagTarget,
    pub on_close: EventHandler<()>,
    /// After each change, so the views that show the tags read them again.
    pub on_changed: EventHandler<()>,
}

/// The picker in bulk mode (docs/ui-common.md §4.18). For each tag of the tree it
/// shows how many of the target's persons carry it, and two actions: Add to
/// all, Remove from all. A change over more than [`CONFIRM_ABOVE`] persons
/// asks first. Each change is one request, so it is written at once.
#[component]
pub fn BulkTagPicker(props: BulkTagPickerProps) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let tree_id = props.tree_id;
    let on_changed = props.on_changed;
    let target = use_hook(|| props.target.clone());

    // Bumped after each change, so the counts are read again.
    let mut revision = use_signal(|| 0_u32);
    let vocabulary = {
        let api = api.clone();
        use_ui_resource("tag_vocabulary", move || {
            let api = api.clone();
            async move { api.list_tags(tree_id).await }
        })
    };
    let counts = {
        let api = api.clone();
        let target = target.clone();
        use_ui_resource("tag_bulk_counts", move || {
            let api = api.clone();
            let target = target.clone();
            let _ = revision();
            async move { read_counts(&api, tree_id, &target).await }
        })
    };

    let mut query = use_signal(String::new);
    let mut error = use_signal(|| None::<String>);
    let mut pending = use_signal(|| None::<(TagRef, BulkTagAction, i64)>);
    let mut busy = use_signal(|| false);

    let (tags, load_failure) = vocabulary_of(&i18n, &vocabulary.read_unchecked());
    let (counts_now, count_failure) = counts_of(&i18n, &counts.read_unchecked());
    let loading = vocabulary.read_unchecked().is_none() || counts.read_unchecked().is_none();
    let total = counts_now.as_ref().map_or(0, |counts| counts.total);

    // The tags the target carries sort first, as the single picker does.
    let carried: Vec<TagRef> = tags
        .iter()
        .filter(|tag| {
            counts_now
                .as_ref()
                .is_some_and(|counts| counts.carried(tag.id) > 0)
        })
        .map(as_tag_ref)
        .collect();
    let typed = query().trim().to_string();
    let (options, _) = picker_options(&tags, &carried, &typed);

    // Writes the change in one request, then reads the counts again.
    let api_for_apply = api.clone();
    let apply_target = target.clone();
    let apply = use_callback(move |(tag, action): (TagRef, BulkTagAction)| {
        let api = api_for_apply.clone();
        let target = apply_target.clone();
        pending.set(None);
        busy.set(true);
        spawn(async move {
            let result = api.bulk_tag(tree_id, tag.id, action, &target).await;
            busy.set(false);
            match result {
                Ok(_) => error.set(None),
                Err(err) => error.set(Some(error_message(&i18n, &err))),
            }
            revision += 1;
            on_changed.call(());
        });
    });

    let title = i18n.t("tag.bulk.title");
    // The list says why it is empty; a failed count is shown on its own
    // line, and no row is drawn over counts that are unknown.
    let empty_text = if loading {
        i18n.t("tag.picker.loading")
    } else if let Some(message) = load_failure {
        message
    } else {
        i18n.t("tag.bulk.empty")
    };
    let total_text = i18n.t_plural("tag.bulk.total", total.max(0) as usize);
    let filter_label = i18n.t("tag.picker.filter");

    rsx! {
        Modal {
            class: "modal-card tag-bulk-card".to_string(),
            label: title.clone(),
            on_close: props.on_close,
            busy: busy(),
            h3 { "{title}" }
            p { class: "tag-bulk-total", "{total_text}" }
            input {
                r#type: "text",
                class: "tag-picker-input",
                placeholder: "{filter_label}",
                aria_label: "{filter_label}",
                autocomplete: "off",
                spellcheck: "false",
                value: "{query}",
                oninput: move |event: Event<FormData>| query.set(event.value()),
            }
            if let Some(message) = error() {
                p { class: "tag-picker-error", role: "alert", "{message}" }
            }
            if let Some(message) = count_failure.clone() {
                p { class: "tag-picker-error", role: "alert", "{message}" }
            }
            if let Some((tag, action, changed)) = pending() {
                {
                    let confirmed = tag.clone();
                    let confirm_text = i18n.t_plural("tag.bulk.confirm_message", changed.max(0) as usize);
                    rsx! {
                        div { class: "tag-bulk-confirm", role: "group",
                            p { "{confirm_text}" }
                            div { class: "tag-bulk-confirm-actions",
                                button {
                                    class: "btn btn-outline btn-sm",
                                    r#type: "button",
                                    onclick: move |_| pending.set(None),
                                    {i18n.t("common.cancel")}
                                }
                                button {
                                    class: "btn btn-primary btn-sm",
                                    r#type: "button",
                                    onclick: move |_| apply.call((confirmed.clone(), action)),
                                    {i18n.t("tag.bulk.confirm")}
                                }
                            }
                        }
                    }
                }
            }
            div {
                class: "tag-bulk-list",
                if let Some(counts) = counts_now.clone() {
                    BulkTagRows {
                        options: options.clone(),
                        counts,
                        busy: busy(),
                        pending,
                        apply,
                    }
                    if options.is_empty() {
                        div { class: "tag-picker-empty", "{empty_text}" }
                    }
                } else if count_failure.is_none() {
                    div { class: "tag-picker-empty", "{empty_text}" }
                }
            }
        }
    }
}

/// The tag as the picker's rows carry it.
fn as_tag_ref(tag: &TagSummary) -> TagRef {
    TagRef {
        id: tag.id,
        label: tag.label.clone(),
        color: tag.color,
    }
}

/// The rows of the bulk picker: one line per tag with its count and the two
/// actions. Each action is a button, so Tab and Enter reach it.
#[component]
fn BulkTagRows(
    options: Vec<TagSummary>,
    counts: BulkCounts,
    busy: bool,
    pending: Signal<PendingChange>,
    apply: Callback<(TagRef, BulkTagAction)>,
) -> Element {
    let i18n = use_i18n();
    let total = counts.total;
    rsx! {
        for tag in options.iter() {
            {
                let row = as_tag_ref(tag);
                let carried = counts.carried(tag.id);
                let add_row = row.clone();
                let remove_row = row.clone();
                let add_disabled = busy || carried >= total;
                let remove_disabled = busy || carried == 0;
                let add_changed = changed_persons(BulkTagAction::Add, total, carried);
                let remove_changed = changed_persons(BulkTagAction::Remove, total, carried);
                let carried_text = i18n.t_args(
                    "tag.bulk.carried",
                    &[("count", &carried.to_string()), ("total", &total.to_string())],
                );
                let add_label = i18n.t_args("tag.bulk.add_label", &[("label", &tag.label)]);
                let remove_label = i18n.t_args("tag.bulk.remove_label", &[("label", &tag.label)]);
                rsx! {
                    div {
                        key: "{tag.id}",
                        class: "tag-bulk-row",
                        role: "group",
                        aria_label: "{tag.label}",
                        span { class: "tag-dot", style: dot_style(tag.color), aria_hidden: "true" }
                        span { class: "tag-picker-label", "{tag.label}" }
                        span { class: "tag-bulk-count", "{carried_text}" }
                        button {
                            class: "btn btn-outline btn-sm",
                            r#type: "button",
                            disabled: add_disabled,
                            aria_label: "{add_label}",
                            onclick: move |_| ask(add_changed, pending, apply, add_row.clone(), BulkTagAction::Add),
                            {i18n.t("tag.bulk.add")}
                        }
                        button {
                            class: "btn btn-outline btn-sm",
                            r#type: "button",
                            disabled: remove_disabled,
                            aria_label: "{remove_label}",
                            onclick: move |_| ask(remove_changed, pending, apply, remove_row.clone(), BulkTagAction::Remove),
                            {i18n.t("tag.bulk.remove")}
                        }
                    }
                }
            }
        }
    }
}

/// Which persons of a branch are named (docs/ui-common.md §4.18).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BranchScope {
    Ancestors,
    Descendants,
    Both,
}

/// The target a branch names. "Both" is one target on the server, which
/// counts a person on both sides once.
fn branch_target(
    person: Uuid,
    scope: BranchScope,
    depth: Option<u8>,
    include_self: bool,
    include_spouses: bool,
) -> TagTarget {
    match scope {
        BranchScope::Ancestors => TagTarget::Ancestors {
            ancestors_of: person,
            depth,
            include_self,
        },
        BranchScope::Descendants => TagTarget::Descendants {
            descendants_of: person,
            depth,
            include_self,
            include_spouses,
        },
        BranchScope::Both => TagTarget::AncestorsAndDescendants {
            ancestors_and_descendants_of: person,
            depth,
            include_self,
            include_spouses,
        },
    }
}

/// The generations field: empty means all of them, up to 30; otherwise a
/// number from 1 to 30. `None` when the text is neither.
fn parse_depth(text: &str) -> Option<Option<u8>> {
    let text = text.trim();
    if text.is_empty() {
        return Some(None);
    }
    match text.parse::<u8>() {
        Ok(depth) if (1..=MAX_DEPTH).contains(&depth) => Some(Some(depth)),
        _ => None,
    }
}

/// The dialog that tags a branch from a person's menu (docs/ui-common.md §4.18):
/// the scope, the depth and the two inclusions, then the bulk picker on the
/// persons they name.
#[component]
pub fn TagBranchDialog(
    tree_id: Uuid,
    person_id: Uuid,
    on_close: EventHandler<()>,
    on_changed: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let mut scope = use_signal(|| BranchScope::Ancestors);
    let mut depth = use_signal(String::new);
    let mut include_self = use_signal(|| true);
    let mut include_spouses = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut chosen = use_signal(|| None::<TagTarget>);

    if let Some(target) = chosen() {
        return rsx! {
            BulkTagPicker { tree_id, target, on_close, on_changed }
        };
    }

    let title = i18n.t("tag.branch.title");
    let spouses_enabled = scope() != BranchScope::Ancestors;
    let submit = move |event: Event<FormData>| {
        event.prevent_default();
        match parse_depth(&depth()) {
            Some(depth) => {
                error.set(None);
                chosen.set(Some(branch_target(
                    person_id,
                    scope(),
                    depth,
                    include_self(),
                    include_spouses(),
                )));
            }
            None => error.set(Some(i18n.t("tag.branch.depth_invalid"))),
        }
    };

    rsx! {
        Modal {
            class: "modal-card".to_string(),
            label: title.clone(),
            on_close,
            h3 { "{title}" }
            form { class: "tag-branch-form", onsubmit: submit,
                fieldset { class: "tag-branch-scope",
                    legend { {i18n.t("tag.branch.scope")} }
                    label {
                        input {
                            r#type: "radio",
                            name: "tag-branch-scope",
                            checked: scope() == BranchScope::Ancestors,
                            onchange: move |_| scope.set(BranchScope::Ancestors),
                        }
                        span { {i18n.t("tag.branch.ancestors")} }
                    }
                    label {
                        input {
                            r#type: "radio",
                            name: "tag-branch-scope",
                            checked: scope() == BranchScope::Descendants,
                            onchange: move |_| scope.set(BranchScope::Descendants),
                        }
                        span { {i18n.t("tag.branch.descendants")} }
                    }
                    label {
                        input {
                            r#type: "radio",
                            name: "tag-branch-scope",
                            checked: scope() == BranchScope::Both,
                            onchange: move |_| scope.set(BranchScope::Both),
                        }
                        span { {i18n.t("tag.branch.both")} }
                    }
                }
                label { class: "tag-branch-field",
                    span { {i18n.t("tag.branch.depth")} }
                    input {
                        r#type: "number",
                        inputmode: "numeric",
                        min: "1",
                        max: "30",
                        placeholder: i18n.t("tag.branch.depth_all"),
                        value: "{depth}",
                        oninput: move |event: Event<FormData>| depth.set(event.value()),
                    }
                    span { class: "tag-branch-hint", {i18n.t("tag.branch.depth_hint")} }
                }
                label {
                    input {
                        r#type: "checkbox",
                        checked: include_self(),
                        onchange: move |event: Event<FormData>| include_self.set(event.checked()),
                    }
                    span { {i18n.t("tag.branch.include_self")} }
                }
                label {
                    input {
                        r#type: "checkbox",
                        disabled: !spouses_enabled,
                        checked: include_spouses(),
                        onchange: move |event: Event<FormData>| include_spouses.set(event.checked()),
                    }
                    span { {i18n.t("tag.branch.include_spouses")} }
                }
                if let Some(message) = error() {
                    p { class: "tag-picker-error", role: "alert", "{message}" }
                }
                div { class: "tag-branch-actions",
                    button {
                        class: "btn btn-outline",
                        r#type: "button",
                        onclick: move |_| on_close.call(()),
                        {i18n.t("common.cancel")}
                    }
                    button { class: "btn btn-primary", r#type: "submit", {i18n.t("tag.branch.next")} }
                }
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
/// dot follows (docs/ui-common.md §4.18).
fn tag_route(tree_id: &str, tag: &TagRef) -> Route {
    Route::DictionaryTag {
        tree_id: tree_id.to_string(),
        tag: tag.id.to_string(),
    }
}

/// The tags of a record as chips: a colour dot and the label, one after the
/// other, each opening the Tags tab on its tag. Nothing is drawn for a record
/// without tags (docs/ui-common.md §4.18). Chips carry no remove button.
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
/// name, so a dot never carries its tag by colour alone (docs/ui-common.md §4.18).
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

/// How long a touch must hold before it opens a record menu, in milliseconds.
const LONG_PRESS_MS: u32 = 500;

/// Places the menu beside the row, read from the page: the row's left edge and
/// its bottom. Sends nothing when neither the row nor one of its controls holds
/// the focus, so a key pressed outside the row does not open its menu.
fn anchor_script(row_id: &str) -> String {
    let id = serde_json::to_string(row_id).unwrap_or_default();
    format!(
        "const row = document.getElementById({id});\n\
         if (row && row.contains(document.activeElement)) {{\n\
             const rect = row.getBoundingClientRect();\n\
             dioxus.send([rect.left + 24, rect.bottom]);\n\
         }} else {{\n\
             dioxus.send(null);\n\
         }}"
    )
}

/// Gives the focus to the row with this DOM id, when a menu or picker that
/// the row opened closes without a choice that moves the focus elsewhere.
fn focus_row(row_id: &str) {
    let _ = document::eval(&format!(
        "document.getElementById({})?.focus();",
        serde_json::to_string(row_id).unwrap_or_default()
    ));
}

/// The record a menu is open for: its kind and id, the tags it carries now,
/// and the DOM id of the row that holds the focus while the menu is open.
#[derive(Clone, PartialEq)]
pub struct MenuTarget {
    pub kind: RecordKind,
    pub record_id: Uuid,
    pub tags: Vec<TagRef>,
    pub row_id: String,
}

/// The menu state of one list of records (docs/ui-common.md §4.18). The rows
/// of a list are not components, so the page holds one [`RecordMenu`] (see
/// [`use_record_menu`]), wires each row to it, and renders one
/// [`RecordTagMenu`] for the list.
#[derive(Clone, Copy, PartialEq)]
pub struct RecordMenu {
    /// Where the menu opens, in client coordinates; `None` while closed.
    at: Signal<Option<(f64, f64)>>,
    target: Signal<Option<MenuTarget>>,
    /// Counts the touches that began and ended, so a long press that was
    /// lifted before its time out does not open the menu.
    press: Signal<u32>,
}

/// The menu state of a list, created by the page component.
pub fn use_record_menu() -> RecordMenu {
    RecordMenu {
        at: use_signal(|| None),
        target: use_signal(|| None),
        press: use_signal(|| 0),
    }
}

impl RecordMenu {
    fn open(mut self, target: MenuTarget, point: (f64, f64)) {
        self.target.set(Some(target));
        self.at.set(Some(point));
    }

    /// Opens the menu of `target` where a right click landed.
    pub fn open_at_pointer(self, event: &Event<MouseData>, target: MenuTarget) {
        event.prevent_default();
        event.stop_propagation();
        let point = event.client_coordinates();
        self.open(target, (point.x, point.y));
    }

    /// Opens the menu of `target` beside its row for the context-menu key and
    /// Shift+F10, when the row or one of its controls holds the focus.
    pub fn open_from_keyboard(self, event: &Event<KeyboardData>, target: MenuTarget) {
        let opens = match event.key() {
            Key::ContextMenu => true,
            Key::F10 => event.modifiers().shift(),
            _ => false,
        };
        if !opens {
            return;
        }
        event.prevent_default();
        event.stop_propagation();
        spawn(async move {
            let mut anchor = document::eval(&anchor_script(&target.row_id));
            if let Ok(Some(point)) = anchor.recv::<Option<(f64, f64)>>().await {
                self.open(target, point);
            }
        });
    }

    /// Starts a long press when a touch begins on a row: the menu opens where
    /// the touch landed once [`LONG_PRESS_MS`] pass, unless the touch ends first.
    pub fn press_start(mut self, event: &Event<PointerData>, target: MenuTarget) {
        if event.pointer_type() != "touch" {
            return;
        }
        event.stop_propagation();
        let point = event.client_coordinates();
        let started = *self.press.peek() + 1;
        self.press.set(started);
        spawn(async move {
            crate::utils::sleep_ms(LONG_PRESS_MS).await;
            if *self.press.peek() == started {
                self.open(target, (point.x, point.y));
            }
        });
    }

    /// Ends a touch: a long press not yet due no longer opens the menu.
    pub fn press_end(mut self) {
        self.press += 1;
    }
}

/// Whether a key press is Enter or Space, the keys that activate a row.
pub(crate) fn is_enter_or_space(event: &Event<KeyboardData>) -> bool {
    match event.key() {
        Key::Enter => true,
        Key::Character(typed) => typed == " ",
        _ => false,
    }
}

/// Enter and Space on a button or tag link inside a row act on that control
/// alone: the row's own activation does not also fire.
pub(crate) fn stop_row_activation(event: Event<KeyboardData>) {
    if is_enter_or_space(&event) {
        event.stop_propagation();
    }
}

/// A touch on a button inside a row does not also start the row's long press,
/// which would open its record menu.
pub(crate) fn stop_row_press(event: Event<PointerData>) {
    event.stop_propagation();
}

/// The menu of the record a [`RecordMenu`] is open for: one entry, Tags…,
/// which closes it and opens the shared [`TagPicker`] beside it. Rendered once
/// per list, beside the rows it serves.
#[component]
pub fn RecordTagMenu(tree_id: Uuid, menu: RecordMenu, on_changed: EventHandler<()>) -> Element {
    let i18n = use_i18n();
    let mut at = menu.at;
    let mut target = menu.target;
    let mut picker_at = use_signal(|| None::<(f64, f64)>);
    let open = at();
    let picked = picker_at();
    let current = target();
    let menu_row_id = current
        .as_ref()
        .map(|t| t.row_id.clone())
        .unwrap_or_default();

    rsx! {
        if let (Some((x, y)), Some(_)) = (open, current.as_ref()) {
            ContextMenuSurface {
                x,
                y,
                menu_class: "record-tag-menu",
                on_close: move |()| {
                    // Dismissed without a choice: the row that opened the menu
                    // takes the focus back. Tags… hands it to the picker.
                    focus_row(&menu_row_id);
                    at.set(None);
                    target.set(None);
                },
                button {
                    class: "context-menu-item",
                    r#type: "button",
                    onmounted: move |event: Event<MountedData>| {
                        spawn(async move {
                            let _ = event.set_focus(true).await;
                        });
                    },
                    onclick: move |_| {
                        picker_at.set(open);
                        at.set(None);
                    },
                    {i18n.t("tag.picker.open")}
                }
            }
        }
        if let (Some((x, y)), Some(current)) = (picked, current) {
            TagPicker {
                tree_id,
                kind: current.kind,
                record_id: current.record_id,
                tags: current.tags,
                x,
                y,
                on_close: move |()| {
                    // The row that opened the menu takes the focus back.
                    focus_row(&current.row_id);
                    picker_at.set(None);
                    target.set(None);
                },
                on_changed: move |_: Vec<TagRef>| on_changed.call(()),
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

    #[test]
    fn the_generations_field_is_empty_for_all_or_one_to_thirty() {
        assert_eq!(parse_depth("  "), Some(None));
        assert_eq!(parse_depth("1"), Some(Some(1)));
        assert_eq!(parse_depth(" 30 "), Some(Some(30)));
        assert_eq!(parse_depth("0"), None);
        assert_eq!(parse_depth("31"), None);
        assert_eq!(parse_depth("many"), None);
    }

    #[test]
    fn both_sides_of_a_branch_are_one_target() {
        let person = Uuid::now_v7();
        assert_eq!(
            branch_target(person, BranchScope::Both, Some(4), true, true),
            TagTarget::AncestorsAndDescendants {
                ancestors_and_descendants_of: person,
                depth: Some(4),
                include_self: true,
                include_spouses: true,
            }
        );
        assert_eq!(
            branch_target(person, BranchScope::Ancestors, None, false, true),
            TagTarget::Ancestors {
                ancestors_of: person,
                depth: None,
                include_self: false,
            }
        );
    }

    #[test]
    fn a_branch_target_serializes_as_the_server_reads_it() {
        let person = Uuid::nil();
        let json = serde_json::to_value(TagTarget::Descendants {
            descendants_of: person,
            depth: None,
            include_self: true,
            include_spouses: false,
        })
        .expect("a target serializes");
        assert_eq!(json["descendants_of"], person.to_string());
        assert!(json.get("depth").is_none(), "an absent depth is left out");
        assert_eq!(json["include_self"], true);
    }

    #[test]
    fn the_carried_count_of_an_unlisted_tag_is_zero() {
        let tag = Uuid::now_v7();
        let counts = BulkCounts {
            total: 5,
            per_tag: HashMap::from([(tag, 2)]),
        };
        assert_eq!(counts.carried(tag), 2);
        assert_eq!(counts.carried(Uuid::now_v7()), 0);
    }

    #[test]
    fn a_change_is_gated_on_the_persons_it_writes_not_the_target() {
        // Add to all on 150 persons, 60 carrying the tag: 90 are written.
        let added = changed_persons(BulkTagAction::Add, 150, 60);
        assert_eq!(added, 90);
        assert!(!needs_confirmation(added));
        // Remove from all on the same 150, 3 carrying it: 3 are written.
        let removed = changed_persons(BulkTagAction::Remove, 150, 3);
        assert_eq!(removed, 3);
        assert!(!needs_confirmation(removed));
        // Add to all on 250 persons, 60 carrying the tag: 190 are written.
        let large = changed_persons(BulkTagAction::Add, 250, 60);
        assert_eq!(large, 190);
        assert!(needs_confirmation(large));
    }

    #[test]
    fn the_confirmation_threshold_is_more_than_one_hundred_persons() {
        assert!(!needs_confirmation(CONFIRM_ABOVE));
        assert!(needs_confirmation(CONFIRM_ABOVE + 1));
    }

    #[test]
    fn a_menu_anchors_on_its_row_while_a_control_in_the_row_has_the_focus() {
        let script = anchor_script("row-1");
        assert!(script.contains("row.contains(document.activeElement)"));
        assert!(script.contains("\"row-1\""));
    }
}
