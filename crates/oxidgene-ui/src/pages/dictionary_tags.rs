//! The Dictionary's Tags tab: the tree's vocabulary, each tag with its colour
//! and its counts per kind of record. It shares the other tabs' toolbar, rows
//! and usage lists (`pages/dictionary.rs`, `docs/ui-dictionary.md` §20). A row
//! unfolds the records carrying the tag, grouped by kind; a record's line takes
//! the record off the tag, and the row edits or deletes the tag itself.

use dioxus::prelude::*;
use oxidgene_core::search::fold_words;
use oxidgene_core::types::{RecordKind, TAG_LABEL_MAX};
use uuid::Uuid;

use crate::api::{ApiClient, TagCounts, TagSummary, TaggedEvent, TaggedRecord};
use crate::components::confirm_dialog::ConfirmDialog;
use crate::components::context_menu::{MenuButton, MenuClose, use_menu_shortcut};
use crate::components::date_input::format_date;
use crate::components::empty_state::EmptyState;
use crate::components::modal::Modal;
use crate::components::paged_list::use_paged_list;
use crate::components::tags::{dot_style, error_message, is_enter_or_space, next_color};
use crate::i18n::{I18n, use_i18n};
use crate::pages::dictionary::{
    DEFAULT_PER_PAGE, ListFilters, PageSize, available_letters, matches_filters, paginate,
    person_usage_link, person_usage_parts, render_clear_filters, render_pagination,
    render_toolbar_with, surname_first, total_pages, with_headers,
};
use crate::router::Route;
use crate::ui_observability::use_ui_resource;
use crate::utils::event_type_label_key;

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

/// The DOM id of a tag's row, the focus lands on when its last line goes.
fn tag_row_id(tag_id: Uuid) -> String {
    format!("tag-row-{tag_id}")
}

/// The DOM id of a record's remove button, the focus lands on when the line
/// before or after it goes.
fn untag_button_id(record_id: Uuid) -> String {
    format!("tag-untag-{record_id}")
}

/// The script that moves the focus to the element `id`. It runs from an
/// effect, so the frame that removed or restored the line is already drawn.
fn focus_script(id: &str) -> String {
    format!("document.getElementById('{id}')?.focus();")
}

#[component]
pub fn DictionaryTags(tree_id: Uuid, open_tag: Option<Uuid>) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let quick = use_signal(String::new);
    let letter = use_signal(|| None::<char>);
    let page_size = use_signal(|| PageSize::Fixed(DEFAULT_PER_PAGE));
    let page = use_signal(|| 1_usize);
    let filters = ListFilters {
        quick,
        letter,
        page_size,
        page,
    };
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

    let loaded = matches!(&*vocabulary.read(), Some(Ok(_)));
    let failed = matches!(&*vocabulary.read(), Some(Err(_)));
    let all_tags: Vec<TagSummary> = match &*vocabulary.read() {
        Some(Ok(list)) => list.clone(),
        _ => Vec::new(),
    };

    // Filed and searched as the other tabs' are: folded, so an accent or a
    // capital in the filter finds the label.
    let needle = fold_words(quick().trim());
    let letters = available_letters(all_tags.iter().map(|tag| tag.label.as_str()));
    let letter_filter = letter();
    let filtered: Vec<&TagSummary> = all_tags
        .iter()
        .filter(|tag| matches_filters(&tag.label, &fold_words(&tag.label), &needle, letter_filter))
        .collect();
    let total_filtered = filtered.len();
    let per_page = page_size().as_option();
    let current = page();
    let pages = total_pages(total_filtered, per_page);
    let page_items = paginate(filtered, current, per_page);
    let rows = with_headers(&page_items, |tag| tag.label.as_str());

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

    let add = rsx! {
        button {
            class: "pf-add-btn",
            r#type: "button",
            onclick: move |_| editing.set(Some(Editing::New)),
            {i18n.t("tag.dictionary.new")}
        }
    };
    rsx! {
        {render_toolbar_with(i18n, &letters, filters, total_filtered, Some(add))}
        if !loaded {
            if failed {
                div { class: "error-msg", {i18n.t("dictionary.error")} }
            } else {
                div { class: "loading", {i18n.t("dictionary.loading")} }
            }
        } else if all_tags.is_empty() {
            EmptyState { p { {i18n.t("tag.dictionary.empty")} } }
        } else if rows.is_empty() {
            {render_clear_filters(i18n, filters)}
        } else {
            div { class: "dict-list",
                for (header , tag) in rows.into_iter() {
                    div { key: "{tag.id}",
                        if let Some(c) = header {
                            div { class: "dict-group-header", "{c}" }
                        }
                        TagRow {
                            tree_id,
                            open: expanded() == Some(tag.id),
                            tag: tag.clone(),
                            on_toggle: move |id: Uuid| expanded.set((expanded() != Some(id)).then_some(id)),
                            on_edit: move |tag: TagSummary| editing.set(Some(Editing::Existing(tag))),
                            on_delete: move |tag: TagSummary| {
                                delete_error.set(None);
                                deleting.set(Some(tag));
                            },
                            on_untagged: move |()| vocabulary.restart(),
                        }
                    }
                }
            }
        }
        {render_pagination(page, current, pages)}
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

/// One tag of the vocabulary: its dot and label, its counts, its edit and
/// delete buttons, and, once unfolded, the records carrying it. The row is the
/// shared Dictionary row: a click or Enter folds it, like the other tabs.
#[component]
fn TagRow(
    tree_id: Uuid,
    tag: TagSummary,
    open: bool,
    on_toggle: EventHandler<Uuid>,
    on_edit: EventHandler<TagSummary>,
    on_delete: EventHandler<TagSummary>,
    on_untagged: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let id = tag.id;
    let counts = counts_line(&i18n, &tag.counts);
    let edited = tag.clone();
    let removed = tag.clone();
    let edit_label = i18n.t("common.edit");
    let delete_label = i18n.t("common.delete");
    let shortcut = use_menu_shortcut();
    rsx! {
        div {
            div {
                class: "dict-row",
                id: "{tag_row_id(id)}",
                tabindex: "0",
                onclick: move |_| on_toggle.call(id),
                onkeydown: move |event: Event<KeyboardData>| {
                    if is_enter_or_space(&event) {
                        event.prevent_default();
                        on_toggle.call(id);
                    } else {
                        shortcut.key(&event);
                    }
                },
                oncontextmenu: move |event: Event<MouseData>| shortcut.right_click(&event),
                onpointerdown: move |event: Event<PointerData>| shortcut.press_start(&event),
                onpointerup: move |_| shortcut.press_end(),
                onpointercancel: move |_| shortcut.press_end(),
                div { class: "dict-row-main",
                    div { class: "dict-row-line",
                        span { class: "tag-dot", style: dot_style(tag.color), aria_hidden: "true" }
                        span { class: "dict-row-value", "{tag.label}" }
                    }
                }
                span { class: "dict-row-count", {counts} }
                MenuButton {
                    name: tag.label.clone(),
                    shortcut: Some(shortcut),
                    items: {
                        let edited = edited.clone();
                        let removed = removed.clone();
                        move |menu: MenuClose| rsx! {
                            button {
                                role: "menuitem",
                                class: "context-menu-item",
                                onclick: {
                                    let edited = edited.clone();
                                    move |_| {
                                        menu.close.call(());
                                        on_edit.call(edited.clone());
                                    }
                                },
                                "{edit_label}"
                            }
                            button {
                                role: "menuitem",
                                class: "context-menu-item context-menu-danger",
                                onclick: {
                                    let removed = removed.clone();
                                    move |_| {
                                        menu.hand_off.call(());
                                        on_delete.call(removed.clone());
                                    }
                                },
                                "{delete_label}"
                            }
                        }
                    },
                }
                span {
                    class: "dict-row-chevron",
                    title: i18n.t("dictionary.view_usage"),
                    if open { "\u{25B2}" } else { "\u{25BC}" }
                }
            }
            if open {
                TagRecords {
                    tree_id,
                    tag_id: id,
                    tag_label: tag.label.clone(),
                    counts: tag.counts.clone(),
                    on_untagged,
                }
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

/// The records carrying a tag, grouped by kind, once the row is unfolded. A
/// kind whose count is spent drops out of the list.
#[component]
fn TagRecords(
    tree_id: Uuid,
    tag_id: Uuid,
    tag_label: String,
    counts: TagCounts,
    on_untagged: EventHandler<()>,
) -> Element {
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
                TaggedKind {
                    key: "{kind}",
                    tree_id,
                    tag_id,
                    tag_label: tag_label.clone(),
                    kind,
                    on_untagged,
                }
            }
        }
    }
}

/// One kind's records carrying a tag, a page at a time. Each line has a
/// button that takes its record off the tag at once; a line that fails to
/// go comes back with the error shown.
#[component]
fn TaggedKind(
    tree_id: Uuid,
    tag_id: Uuid,
    tag_label: String,
    kind: RecordKind,
    on_untagged: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let api_page = api.clone();
    let records = use_paged_list("tag_records", move |cursor: Option<String>| {
        let api = api_page.clone();
        async move {
            api.list_tagged_records(tree_id, tag_id, kind, RECORDS_PAGE, cursor.as_deref())
                .await
                .map_err(|err| err.to_string())
        }
    });
    let mut first = records.first;
    // The lines taken off the tag, hidden until the list is read again: a
    // successful removal reads the list, and a line the list no longer
    // carries leaves this set, so a record put back shows again.
    let mut taken = use_signal(Vec::<Uuid>::new);
    let mut untag_error = use_signal(|| None::<String>);
    // The element to focus once the frame that moved a line is drawn.
    let mut refocus = use_signal(|| None::<String>);
    let listed = records.clone();
    use_effect(move || {
        let present: Vec<Uuid> = listed.items().iter().map(|record| record.id).collect();
        taken.with_mut(|ids| ids.retain(|id| present.contains(id)));
    });
    use_effect(move || {
        let target = refocus();
        if let Some(id) = target {
            refocus.set(None);
            document::eval(&focus_script(&id));
        }
    });
    let loading = first.read().is_none();
    let failed = matches!(&*first.read(), Some(Err(_)));
    let items: Vec<TaggedRecord> = records
        .items()
        .into_iter()
        .filter(|record| !taken().contains(&record.id))
        .collect();
    let row_focus = tag_row_id(tag_id);
    let tree = tree_id.to_string();
    rsx! {
        div { class: "dict-accordion-group",
            div { class: "dict-group-header", {i18n.t(kind_key(kind))} }
            if loading {
                div { class: "dict-accordion-empty", {i18n.t("common.loading")} }
            } else if failed {
                div { class: "error-msg", {i18n.t("dictionary.error")} }
            }
            if let Some(err) = untag_error() {
                div { class: "error-msg", role: "alert", "{err}" }
            }
            for (index , record) in items.iter().enumerate() {
                {
                    let record_id = record.id;
                    // Focus goes to the next line's button, else the previous
                    // one's, else the row's, once this line is gone.
                    let focus_on = items
                        .get(index + 1)
                        .or_else(|| index.checked_sub(1).and_then(|before| items.get(before)))
                        .map(|next| untag_button_id(next.id))
                        .unwrap_or_else(|| row_focus.clone());
                    let record_name = record_plain(i18n, record);
                    let remove_label = i18n.t_args(
                        "tag.untag.label",
                        &[("record", &record_name), ("tag", &tag_label)],
                    );
                    let api = api.clone();
                    rsx! {
                        div { key: "{record_id}", class: "dict-accordion-line",
                            if let Some(entry) = &record.person {
                                {person_usage_link(i18n, &tree, entry)}
                            } else {
                                {record_link(i18n, &tree, record)}
                            }
                            button {
                                id: "{untag_button_id(record_id)}",
                                class: "tag-chip-remove",
                                r#type: "button",
                                title: "{remove_label}",
                                aria_label: "{remove_label}",
                                onclick: move |_| {
                                    let api = api.clone();
                                    taken.write().push(record_id);
                                    untag_error.set(None);
                                    // The line leaves at once, so the focus moves on with it; a
                                    // failure brings the line back and the focus with it.
                                    refocus.set(Some(focus_on.clone()));
                                    spawn(async move {
                                        match api.untag_record(tree_id, tag_id, kind, record_id).await {
                                            Ok(()) => {
                                                // Read again before the counts, which may drop this kind's group.
                                                first.restart();
                                                on_untagged.call(());
                                            }
                                            Err(err) => {
                                                taken.with_mut(|ids| ids.retain(|id| *id != record_id));
                                                untag_error.set(Some(error_message(&i18n, &err)));
                                                refocus.set(Some(untag_button_id(record_id)));
                                            }
                                        }
                                    });
                                },
                                "\u{00D7}"
                            }
                        }
                    }
                }
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

/// The page a record's line opens: a couple, a document, and a source, a
/// place or a repository on its own row of this dictionary. An event, a
/// citation or a note opens its owner's profile. `None` for a record with no
/// page to open, shown as plain text.
fn record_route(tree: &str, record: &TaggedRecord) -> Option<Route> {
    let tree_id = tree.to_string();
    let id = record.id.to_string();
    match record.kind {
        RecordKind::Person => Some(Route::TreeDetail {
            tree_id,
            person: Some(id),
        }),
        RecordKind::Family => Some(Route::CoupleDetail {
            tree_id,
            family_id: id,
        }),
        RecordKind::Media => Some(Route::Media {
            tree_id,
            media_id: id,
        }),
        RecordKind::Source => Some(Route::DictionarySource {
            tree_id,
            source_id: id,
        }),
        RecordKind::Place => Some(Route::DictionaryPlace {
            tree_id,
            place_id: id,
        }),
        RecordKind::Repository => Some(Route::DictionaryRepository {
            tree_id,
            repository_id: id,
        }),
        RecordKind::Event | RecordKind::Citation | RecordKind::Note => {
            if let Some(person) = record.person_id {
                Some(Route::PersonDetail {
                    tree_id,
                    person_id: person.to_string(),
                })
            } else {
                record.family_id.map(|family| Route::CoupleDetail {
                    tree_id,
                    family_id: family.to_string(),
                })
            }
        }
    }
}

/// A record's line, as the usage lists show one: the link of its page in the
/// usage list's style, or its text when it has no page.
fn record_link(i18n: I18n, tree: &str, record: &TaggedRecord) -> Element {
    match record_route(tree, record) {
        Some(to) => rsx! {
            Link { to, class: "dict-accordion-item",
                {record_line(i18n, record)}
            }
        },
        None => rsx! {
            span { class: "dict-accordion-item",
                {record_line(i18n, record)}
            }
        },
    }
}

/// The contents of a record's line: what the record is, then the persons it
/// shows, each with the lifespan a person line gives. A dash joins the
/// persons to what the record is, a dot joins one person to the next.
fn record_line(i18n: I18n, record: &TaggedRecord) -> Element {
    let text = record_text(i18n, record).or_else(|| record_fallback(i18n, record));
    let has_text = text.is_some();
    let persons = record.persons.as_deref().unwrap_or_default();
    rsx! {
        if let Some(text) = text {
            span { class: "dict-accordion-name", "{text}" }
        }
        for (index, entry) in persons.iter().enumerate() {
            if let Some(separator) = separator_before(i18n, index, has_text) {
                span { "{separator}" }
            }
            {person_usage_parts(i18n, entry)}
        }
    }
}

/// The separator written before the person at `index` of a line: a dash
/// after the record's own text, a dot between two persons.
fn separator_before(i18n: I18n, index: usize, has_text: bool) -> Option<String> {
    match (index, has_text) {
        (0, true) => Some(i18n.t("tag.record.dash")),
        (0, false) => None,
        _ => Some(i18n.t("tag.record.dot")),
    }
}

/// The text a record's line leads with, or `None` for a couple, which shows
/// its spouses instead. A record with no text of its own is named by what it
/// is, never by its id or its raw value.
fn record_text(i18n: I18n, record: &TaggedRecord) -> Option<String> {
    let text = match (record.kind, record.label.trim()) {
        (RecordKind::Family, _) => return None,
        (RecordKind::Event, _) => record.event.as_ref().map(|event| event_text(i18n, event)),
        (RecordKind::Citation, _) => Some(citation_text(i18n, record)),
        (RecordKind::Note, "") => return Some(i18n.t("tag.record.empty_note")),
        (_, label) if !label.is_empty() => Some(label.to_owned()),
        _ => None,
    };
    Some(text.unwrap_or_else(|| i18n.t("tag.record.untitled")))
}

/// The text of a record line with no text of its own and no persons either:
/// the untitled text, so that a couple with no spouse still reads.
fn record_fallback(i18n: I18n, record: &TaggedRecord) -> Option<String> {
    let has_persons = record
        .persons
        .as_ref()
        .is_some_and(|persons| !persons.is_empty());
    (record.kind == RecordKind::Family && !has_persons).then(|| i18n.t("tag.record.untitled"))
}

/// A record's name as plain text, for the remove control's label: the text the
/// record's line leads with, or for a couple, its spouses' names.
fn record_plain(i18n: I18n, record: &TaggedRecord) -> String {
    if let Some(text) = record_text(i18n, record) {
        return text;
    }
    let dot = i18n.t("tag.record.dot");
    let names = record
        .persons
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|entry| surname_first(entry, &i18n))
        .collect::<Vec<_>>()
        .join(&dot);
    if names.is_empty() {
        i18n.t("tag.record.untitled")
    } else {
        names
    }
}

/// A citation's text: its source's title and its page, or the untitled text
/// when its source is gone.
fn citation_text(i18n: I18n, record: &TaggedRecord) -> String {
    let Some(citation) = &record.citation else {
        return i18n.t("tag.record.untitled_citation");
    };
    match (&citation.source_title, &citation.page) {
        (Some(title), Some(page)) => i18n.t_args(
            "tag.record.citation_page",
            &[("title", title), ("page", page)],
        ),
        (Some(title), None) => title.clone(),
        (None, _) => i18n.t("tag.record.untitled_citation"),
    }
}

/// An event's text, as the timeline words it: its translated type, its
/// description after a dash, then its date and its place after dots, without
/// its sources.
fn event_text(i18n: I18n, event: &TaggedEvent) -> String {
    let kind = i18n.t(event_type_label_key(event.event_type));
    let head = match &event.description {
        Some(description) => format!("{kind}{}{description}", i18n.t("tag.record.dash")),
        None => kind,
    };
    let date = format_date(
        &i18n,
        event.calendar,
        event.date_qualifier,
        event.date_value.as_deref(),
        event.date_value2.as_deref(),
    );
    let dot = i18n.t("tag.record.dot");
    [date, event.place_name.clone().unwrap_or_default()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .fold(head, |text, part| format!("{text}{dot}{part}"))
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
