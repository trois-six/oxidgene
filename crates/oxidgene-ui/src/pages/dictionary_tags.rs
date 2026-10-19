//! The Dictionary's Tags tab: the tree's vocabulary, each tag with its colour
//! and its counts per kind of record. A row unfolds the records carrying the
//! tag, grouped by kind and paged a kind at a time, and offers to edit,
//! delete or merge the tag. See `docs/ui-dictionary.md` §20.

use dioxus::prelude::*;
use oxidgene_core::search::fold_words;
use oxidgene_core::types::{RecordKind, TAG_LABEL_MAX};
use uuid::Uuid;

use crate::api::{ApiClient, TagCounts, TagSummary, TaggedRecord};
use crate::components::confirm_dialog::ConfirmDialog;
use crate::components::empty_state::EmptyState;
use crate::components::modal::Modal;
use crate::components::paged_list::use_paged_list;
use crate::components::tags::{dot_style, error_message, next_color};
use crate::i18n::{I18n, use_i18n};
use crate::router::Route;
use crate::ui_observability::use_ui_resource;

/// Records a kind shows per page, in the unfolded row.
const RECORDS_PAGE: u64 = 25;

/// Which editor is open: a tag's, or the one creating a tag.
#[derive(Clone, PartialEq)]
enum Editing {
    New,
    Existing(TagSummary),
}

/// The kinds in the order the counts show them, each with its count.
fn kind_counts(counts: &TagCounts) -> [(RecordKind, i64); 9] {
    [
        (RecordKind::Person, counts.persons),
        (RecordKind::Family, counts.families),
        (RecordKind::Event, counts.events),
        (RecordKind::Place, counts.places),
        (RecordKind::Source, counts.sources),
        (RecordKind::Citation, counts.citations),
        (RecordKind::Repository, counts.repositories),
        (RecordKind::Media, counts.media),
        (RecordKind::Note, counts.notes),
    ]
}

/// The translation key of a kind's count, pluralised by the caller.
fn count_key(kind: RecordKind) -> &'static str {
    match kind {
        RecordKind::Person => "tag.count.persons",
        RecordKind::Family => "tag.count.families",
        RecordKind::Event => "tag.count.events",
        RecordKind::Place => "tag.count.places",
        RecordKind::Source => "tag.count.sources",
        RecordKind::Citation => "tag.count.citations",
        RecordKind::Repository => "tag.count.repositories",
        RecordKind::Media => "tag.count.media",
        RecordKind::Note => "tag.count.notes",
    }
}

/// The translation key of a kind's heading in the unfolded row.
fn kind_key(kind: RecordKind) -> &'static str {
    match kind {
        RecordKind::Person => "tag.kind.persons",
        RecordKind::Family => "tag.kind.families",
        RecordKind::Event => "tag.kind.events",
        RecordKind::Place => "tag.kind.places",
        RecordKind::Source => "tag.kind.sources",
        RecordKind::Citation => "tag.kind.citations",
        RecordKind::Repository => "tag.kind.repositories",
        RecordKind::Media => "tag.kind.media",
        RecordKind::Note => "tag.kind.notes",
    }
}

/// Every tag's total of records, over all kinds.
fn total_records(tag: &TagSummary) -> i64 {
    kind_counts(&tag.counts)
        .iter()
        .map(|(_, count)| count)
        .sum()
}

/// The counts of a tag on one line: "12 persons · 3 sources", or nothing
/// said when no record carries it.
fn counts_line(i18n: &I18n, counts: &TagCounts) -> String {
    let parts: Vec<String> = kind_counts(counts)
        .into_iter()
        .filter(|(_, count)| *count > 0)
        .map(|(kind, count)| i18n.t_plural(count_key(kind), count as usize))
        .collect();
    if parts.is_empty() {
        i18n.t("tag.count.none")
    } else {
        parts.join(" \u{00B7} ")
    }
}

#[component]
pub fn DictionaryTags(tree_id: Uuid, open_tag: Option<Uuid>) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let mut quick = use_signal(String::new);
    let mut expanded = use_signal(|| open_tag);
    let mut editing = use_signal(|| None::<Editing>);
    let mut deleting = use_signal(|| None::<TagSummary>);
    let mut delete_error = use_signal(|| None::<String>);
    let mut deleting_busy = use_signal(|| false);

    let api_list = api.clone();
    let mut vocabulary = use_ui_resource("tags_vocabulary", move || {
        let api = api_list.clone();
        async move { api.list_tags(tree_id).await }
    });

    let filter = fold_words(quick().trim());
    let listed: Option<Vec<TagSummary>> = match &*vocabulary.read() {
        Some(Ok(list)) => Some(
            list.iter()
                .filter(|tag| filter.is_empty() || fold_words(&tag.label).contains(&filter))
                .cloned()
                .collect(),
        ),
        _ => None,
    };
    let failed = matches!(&*vocabulary.read(), Some(Err(_)));
    let all_tags: Vec<TagSummary> = match &*vocabulary.read() {
        Some(Ok(list)) => list.clone(),
        _ => Vec::new(),
    };

    let api_delete = api.clone();
    let confirm_delete = move |()| {
        let Some(tag) = deleting() else { return };
        let api = api_delete.clone();
        spawn(async move {
            deleting_busy.set(true);
            match api.delete_tag(tree_id, tag.id).await {
                Ok(()) => {
                    deleting.set(None);
                    delete_error.set(None);
                    if expanded() == Some(tag.id) {
                        expanded.set(None);
                    }
                    vocabulary.restart();
                }
                Err(err) => delete_error.set(Some(error_message(&i18n, &err))),
            }
            deleting_busy.set(false);
        });
    };

    rsx! {
        div { class: "dict-filter-row",
            input {
                r#type: "text",
                class: "dict-filter-input",
                placeholder: "{i18n.t(\"dictionary.filter_placeholder\")}",
                value: "{quick}",
                oninput: move |e: Event<FormData>| quick.set(e.value()),
            }
            button {
                class: "pf-add-btn",
                r#type: "button",
                onclick: move |_| editing.set(Some(Editing::New)),
                {i18n.t("tag.dictionary.new")}
            }
        }
        match listed {
            None if failed => rsx! { div { class: "error-msg", {i18n.t("dictionary.error")} } },
            None => rsx! { div { class: "loading", {i18n.t("dictionary.loading")} } },
            Some(list) if list.is_empty() && all_tags.is_empty() => rsx! {
                EmptyState { p { {i18n.t("tag.dictionary.empty")} } }
            },
            Some(list) if list.is_empty() => rsx! {
                div { class: "dict-src-summary", {i18n.t_plural("dictionary.count", 0)} }
            },
            Some(list) => rsx! {
                div { class: "dict-src-summary", {i18n.t_plural("dictionary.count", list.len())} }
                div { class: "dict-list",
                    for tag in list.into_iter() {
                        TagRow {
                            key: "{tag.id}",
                            tree_id,
                            open: expanded() == Some(tag.id),
                            tag,
                            on_toggle: move |id: Uuid| expanded.set((expanded() != Some(id)).then_some(id)),
                            on_edit: move |tag: TagSummary| editing.set(Some(Editing::Existing(tag))),
                            on_delete: move |tag: TagSummary| {
                                delete_error.set(None);
                                deleting.set(Some(tag));
                            },
                        }
                    }
                }
            },
        }
        if let Some(edit) = editing() {
            TagEditor {
                key: "{edit_key(&edit)}",
                tree_id,
                existing: match &edit {
                    Editing::New => None,
                    Editing::Existing(tag) => Some(tag.clone()),
                },
                vocabulary: all_tags.clone(),
                on_close: move |()| editing.set(None),
                on_saved: move |()| {
                    editing.set(None);
                    vocabulary.restart();
                },
            }
        }
        if let Some(tag) = deleting() {
            ConfirmDialog {
                title: i18n.t("tag.delete.title"),
                message: i18n.t_args(
                    &i18n.plural_key("tag.delete.message", total_records(&tag) as usize),
                    &[("label", &tag.label), ("count", &total_records(&tag).to_string())],
                ),
                confirm_label: i18n.t("common.delete"),
                confirm_class: "btn btn-danger",
                error: delete_error(),
                busy: deleting_busy(),
                on_confirm: confirm_delete,
                on_cancel: move |()| {
                    deleting.set(None);
                    delete_error.set(None);
                },
            }
        }
    }
}

/// One tag of the vocabulary: its dot, label and counts, its edit and delete
/// buttons, and, once unfolded, the records carrying it.
#[component]
fn TagRow(
    tree_id: Uuid,
    tag: TagSummary,
    open: bool,
    on_toggle: EventHandler<Uuid>,
    on_edit: EventHandler<TagSummary>,
    on_delete: EventHandler<TagSummary>,
) -> Element {
    let i18n = use_i18n();
    let id = tag.id;
    let counts = counts_line(&i18n, &tag.counts);
    let counts_for_records = tag.counts.clone();
    let edited = tag.clone();
    let removed = tag.clone();
    rsx! {
        div {
            div {
                class: "dict-row",
                onclick: move |_| on_toggle.call(id),
                div { class: "dict-row-main",
                    span { class: "tag-dot", style: dot_style(tag.color), aria_hidden: "true" }
                    span { class: "dict-row-value", "{tag.label}" }
                    span { class: "dict-row-meta", "{counts}" }
                }
                button {
                    class: "dict-row-action",
                    r#type: "button",
                    title: "{i18n.t(\"common.edit\")}",
                    aria_label: "{i18n.t(\"common.edit\")}",
                    onclick: move |e: Event<MouseData>| {
                        e.stop_propagation();
                        on_edit.call(edited.clone());
                    },
                    "\u{270E}"
                }
                button {
                    class: "dict-row-action",
                    r#type: "button",
                    title: "{i18n.t(\"common.delete\")}",
                    aria_label: "{i18n.t(\"common.delete\")}",
                    onclick: move |e: Event<MouseData>| {
                        e.stop_propagation();
                        on_delete.call(removed.clone());
                    },
                    "\u{00D7}"
                }
                button {
                    class: "dict-row-action",
                    r#type: "button",
                    if open { "\u{25B2}" } else { "\u{25BC}" }
                }
            }
            if open {
                TagRecords { tree_id, tag_id: id, counts: counts_for_records }
            }
        }
    }
}

/// The key the editor is mounted under, so another tag's opens fresh.
fn edit_key(edit: &Editing) -> String {
    match edit {
        Editing::New => "new".to_string(),
        Editing::Existing(tag) => tag.id.to_string(),
    }
}

/// The records carrying a tag, grouped by kind, once the row is unfolded.
#[component]
fn TagRecords(tree_id: Uuid, tag_id: Uuid, counts: TagCounts) -> Element {
    let i18n = use_i18n();
    let kinds: Vec<RecordKind> = kind_counts(&counts)
        .into_iter()
        .filter(|(_, count)| *count > 0)
        .map(|(kind, _)| kind)
        .collect();
    if kinds.is_empty() {
        return rsx! {
            div { class: "dict-accordion",
                div { class: "dict-accordion-empty", {i18n.t("tag.count.none")} }
            }
        };
    }
    rsx! {
        div { class: "dict-accordion",
            for kind in kinds {
                TaggedKind { key: "{kind}", tree_id, tag_id, kind }
            }
        }
    }
}

/// One kind's records carrying a tag, a page at a time.
#[component]
fn TaggedKind(tree_id: Uuid, tag_id: Uuid, kind: RecordKind) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let records = use_paged_list("tag_records", move |cursor: Option<String>| {
        let api = api.clone();
        async move {
            api.list_tagged_records(tree_id, tag_id, kind, RECORDS_PAGE, cursor.as_deref())
                .await
                .map_err(|err| err.to_string())
        }
    });
    let loading = records.first.read().is_none();
    let failed = matches!(&*records.first.read(), Some(Err(_)));
    let items = records.items();
    rsx! {
        div { class: "dict-accordion-group",
            div { class: "dict-accordion-name", {i18n.t(kind_key(kind))} }
            if loading {
                div { class: "dict-accordion-empty", {i18n.t("common.loading")} }
            } else if failed {
                div { class: "error-msg", {i18n.t("dictionary.error")} }
            }
            for record in items.into_iter() {
                TaggedRecordRow { key: "{record.id}", tree_id, record }
            }
            if records.has_more() {
                button {
                    class: "pf-add-btn",
                    r#type: "button",
                    disabled: records.loading_more(),
                    onclick: move |_| records.load_more(),
                    {i18n.t("history.load_more")}
                }
            }
        }
    }
}

/// A record carrying a tag: a link to its page, the media viewer for a
/// document, and plain text for the kinds that have no page yet.
#[component]
fn TaggedRecordRow(tree_id: Uuid, record: TaggedRecord) -> Element {
    let tree = tree_id.to_string();
    match record.kind {
        RecordKind::Person => rsx! {
            div { class: "dict-accordion-item",
                Link {
                    to: Route::PersonDetail { tree_id: tree, person_id: record.id.to_string() },
                    class: "dict-accordion-name",
                    "{record.label}"
                }
            }
        },
        RecordKind::Family => rsx! {
            div { class: "dict-accordion-item",
                Link {
                    to: Route::CoupleDetail { tree_id: tree, family_id: record.id.to_string() },
                    class: "dict-accordion-name",
                    "{record.label}"
                }
            }
        },
        RecordKind::Media => rsx! {
            div { class: "dict-accordion-item",
                Link {
                    to: Route::Media { tree_id: tree, media_id: record.id.to_string() },
                    class: "dict-accordion-name",
                    "{record.label}"
                }
            }
        },
        // An event or a citation is shown on its owner's profile.
        RecordKind::Event | RecordKind::Citation if record.person_id.is_some() => rsx! {
            div { class: "dict-accordion-item",
                Link {
                    to: Route::PersonDetail {
                        tree_id: tree,
                        person_id: record.person_id.map(|id| id.to_string()).unwrap_or_default(),
                    },
                    class: "dict-accordion-name",
                    "{record.label}"
                }
            }
        },
        RecordKind::Event | RecordKind::Citation if record.family_id.is_some() => rsx! {
            div { class: "dict-accordion-item",
                Link {
                    to: Route::CoupleDetail {
                        tree_id: tree,
                        family_id: record.family_id.map(|id| id.to_string()).unwrap_or_default(),
                    },
                    class: "dict-accordion-name",
                    "{record.label}"
                }
            }
        },
        // A source is edited in the Sources tab of this dictionary.
        RecordKind::Source => rsx! {
            div { class: "dict-accordion-item",
                Link {
                    to: Route::DictionarySources { tree_id: tree },
                    class: "dict-accordion-name",
                    "{record.label}"
                }
            }
        },
        _ => rsx! {
            div { class: "dict-accordion-item",
                span { class: "dict-accordion-name", "{record.label}" }
            }
        },
    }
}

/// The tag's editor: its label and its colour. A label folding to another
/// tag's offers to merge the two rather than saving a clash.
#[component]
fn TagEditor(
    tree_id: Uuid,
    existing: Option<TagSummary>,
    vocabulary: Vec<TagSummary>,
    on_close: EventHandler<()>,
    on_saved: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let own_id = existing.as_ref().map(|tag| tag.id);
    let mut label = use_signal(|| {
        existing
            .as_ref()
            .map(|tag| tag.label.clone())
            .unwrap_or_default()
    });
    let mut color = use_signal(|| {
        existing
            .as_ref()
            .map(|tag| tag.color)
            .unwrap_or_else(|| next_color(&vocabulary))
    });
    let mut saving = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut merge_into = use_signal(|| None::<TagSummary>);

    let title = if existing.is_some() {
        i18n.t("tag.edit.title")
    } else {
        i18n.t("tag.dictionary.new")
    };

    let save = {
        let api = api.clone();
        let vocabulary = vocabulary.clone();
        move |_| {
            let api = api.clone();
            let typed = label().trim().to_string();
            if typed.is_empty() || typed.chars().count() > TAG_LABEL_MAX {
                error.set(Some(i18n.t("tag.error.validation")));
                return;
            }
            let folded = fold_words(&typed);
            let clash = vocabulary
                .iter()
                .find(|tag| Some(tag.id) != own_id && fold_words(&tag.label) == folded)
                .cloned();
            if let Some(other) = clash {
                if own_id.is_some() {
                    merge_into.set(Some(other));
                } else {
                    error.set(Some(i18n.t("tag.error.conflict")));
                }
                return;
            }
            let chosen = color();
            spawn(async move {
                saving.set(true);
                error.set(None);
                let written = match own_id {
                    None => api
                        .create_tag(tree_id, &typed, Some(chosen))
                        .await
                        .map(|_| ()),
                    Some(id) => api
                        .update_tag(tree_id, id, Some(&typed), Some(chosen))
                        .await
                        .map(|_| ()),
                };
                saving.set(false);
                match written {
                    Ok(()) => on_saved.call(()),
                    Err(err) => error.set(Some(error_message(&i18n, &err))),
                }
            });
        }
    };

    let merge = {
        let api = api.clone();
        move |_| {
            let (Some(id), Some(other)) = (own_id, merge_into()) else {
                return;
            };
            let api = api.clone();
            spawn(async move {
                saving.set(true);
                error.set(None);
                let merged = api.merge_tag(tree_id, id, other.id).await;
                saving.set(false);
                match merged {
                    Ok(_) => on_saved.call(()),
                    Err(err) => error.set(Some(error_message(&i18n, &err))),
                }
            });
        }
    };

    let current_color = color();
    rsx! {
        Modal {
            class: "dict-edit-modal",
            label: Some(title.clone()),
            busy: saving(),
            on_close,
            div { class: "dict-edit-header",
                h2 { {title.clone()} }
                button { class: "person-form-close", r#type: "button", onclick: move |_| on_close.call(()), "\u{2715}" }
            }
            div { class: "form-group",
                label { {i18n.t("tag.label")} }
                input {
                    r#type: "text",
                    value: "{label}",
                    oninput: move |e: Event<FormData>| {
                        merge_into.set(None);
                        label.set(e.value());
                    },
                }
            }
            div { class: "form-group",
                label { {i18n.t("tag.color")} }
                div { class: "tag-swatches", role: "radiogroup", aria_label: i18n.t("tag.color"),
                    for slot in 0..12_i16 {
                        button {
                            key: "{slot}",
                            class: if slot == current_color { "tag-swatch is-selected" } else { "tag-swatch" },
                            r#type: "button",
                            role: "radio",
                            aria_checked: if slot == current_color { "true" } else { "false" },
                            aria_label: i18n.t_args("tag.color_slot", &[("number", &(slot + 1).to_string())]),
                            title: i18n.t_args("tag.color_slot", &[("number", &(slot + 1).to_string())]),
                            style: dot_style(slot),
                            onclick: move |_| color.set(slot),
                        }
                    }
                }
            }
            if let Some(other) = merge_into() {
                div { class: "tag-merge",
                    p { class: "tag-merge-message", {i18n.t_args("tag.merge.message", &[("label", &other.label)])} }
                    button {
                        class: "td-btn td-btn-primary",
                        r#type: "button",
                        disabled: saving(),
                        onclick: merge,
                        {i18n.t_args("tag.merge.action", &[("label", &other.label)])}
                    }
                }
            }
            if let Some(err) = error() {
                div { class: "error-msg", "{err}" }
            }
            div { class: "modal-actions",
                button { class: "td-btn", r#type: "button", onclick: move |_| on_close.call(()), {i18n.t("common.cancel")} }
                button {
                    class: "td-btn td-btn-primary",
                    r#type: "button",
                    disabled: saving(),
                    onclick: save,
                    if saving() { {i18n.t("common.saving")} } else { {i18n.t("common.save")} }
                }
            }
        }
    }
}
