//! The one text field with suggestions every form uses: free text, with a
//! list opening under it as the user types. [`SuggestInput`] draws the field
//! and its list and handles the keyboard; [`ValueInput`] feeds it the values
//! the tree already holds and the reference sheets' terms. Place fields feed
//! it their own way (see `place_input.rs`). See `docs/ui-common.md` §4.4.

use std::collections::HashMap;
use std::rc::Rc;

use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::{ApiClient, NameScope, SuggestionField, ValueSuggestion};
use crate::components::tree_cache::use_tree_cache;
use crate::i18n::{I18n, use_i18n};
use crate::ui_observability::use_ui_resource;
use crate::utils::sleep_ms;

/// Keystrokes settle for this long before the backend is asked.
pub(crate) const DEBOUNCE_MS: u32 = 300;
/// Values a field lists at once. A tree's source titles often share a long
/// head ("Parish register, …"), so they list as many as the API returns and
/// the list scrolls.
fn value_suggestions(field: SuggestionField) -> usize {
    match field {
        SuggestionField::Sources => 50,
        _ => 10,
    }
}

/// One row of the list: a name, then muted details.
#[derive(Clone, PartialEq)]
pub(crate) struct SuggestRow {
    pub name: String,
    pub detail: String,
    /// A reference sheet explains this term.
    pub sheet: bool,
}

/// A text field showing `value`, with `rows` listed under it while it has
/// the focus. Typing reports the new text through `on_input`; picking a row,
/// by pointer or with the arrow keys and Enter, reports its index through
/// `on_pick`. Escape closes the list.
#[component]
pub(crate) fn SuggestInput(
    value: String,
    rows: Vec<SuggestRow>,
    on_input: EventHandler<String>,
    on_pick: EventHandler<usize>,
    #[props(default)] placeholder: Option<String>,
    #[props(default)] disabled: bool,
) -> Element {
    let i18n = use_i18n();
    let mut open = use_signal(|| false);
    let mut highlight = use_signal(|| None::<usize>);
    // The rows as mounted, so the keyboard can scroll the one it reaches
    // into view.
    let mut mounted = use_signal(HashMap::<usize, Rc<MountedData>>::new);
    let count = rows.len();
    let list_visible = open() && count > 0;

    let mut move_to = move |index: usize| {
        highlight.set(Some(index));
        if let Some(row) = mounted.peek().get(&index).cloned() {
            spawn(async move {
                let _ = row
                    .scroll_to_with_options(ScrollToOptions {
                        behavior: ScrollBehavior::Instant,
                        vertical: ScrollLogicalPosition::Nearest,
                        horizontal: ScrollLogicalPosition::Nearest,
                    })
                    .await;
            });
        }
    };

    let mut pick = move |index: usize| {
        open.set(false);
        highlight.set(None);
        on_pick.call(index);
    };

    let on_key = move |e: Event<KeyboardData>| match e.key() {
        Key::ArrowDown if count > 0 => {
            e.prevent_default();
            open.set(true);
            move_to(highlight().map_or(0, |i| (i + 1) % count));
        }
        Key::ArrowUp if count > 0 => {
            e.prevent_default();
            open.set(true);
            move_to(highlight().map_or(count - 1, |i| (i + count - 1) % count));
        }
        Key::Enter => {
            if let Some(index) = highlight().filter(|&i| open() && i < count) {
                // Enter picks the row instead of submitting the form.
                e.prevent_default();
                pick(index);
            }
        }
        Key::Escape if open() => {
            e.stop_propagation();
            open.set(false);
            highlight.set(None);
        }
        _ => {}
    };

    rsx! {
        div { class: "suggest-input",
            input {
                r#type: "text",
                value: "{value}",
                placeholder: placeholder.unwrap_or_default(),
                disabled,
                autocomplete: "off",
                "aria-autocomplete": "list",
                "aria-expanded": "{list_visible}",
                oninput: move |e: Event<FormData>| {
                    open.set(true);
                    highlight.set(None);
                    on_input.call(e.value());
                },
                onfocus: move |_| open.set(true),
                // The list's buttons keep the focus on the field while they
                // are pressed, so leaving the field really is leaving it.
                onblur: move |_| {
                    open.set(false);
                    highlight.set(None);
                },
                onkeydown: on_key,
            }
            if list_visible {
                div {
                    class: "suggest-input-list",
                    role: "listbox",
                    onmousedown: move |e: Event<MouseData>| e.prevent_default(),
                    for (index, row) in rows.iter().enumerate() {
                        button {
                            key: "{index}",
                            r#type: "button",
                            role: "option",
                            class: if highlight() == Some(index) {
                                "context-menu-item td-suggest-row suggest-input-row is-active"
                            } else {
                                "context-menu-item td-suggest-row suggest-input-row"
                            },
                            onmounted: move |e: MountedEvent| {
                                mounted.write().insert(index, e.data());
                            },
                            onmouseenter: move |_| highlight.set(Some(index)),
                            onclick: move |_| pick(index),
                            {render_suggest_row(row, &i18n)}
                        }
                    }
                }
            }
        }
    }
}

/// The inside of one suggestion row, shared with the topbar search panel.
pub(crate) fn render_suggest_row(row: &SuggestRow, i18n: &I18n) -> Element {
    rsx! {
        span { class: "suggest-input-name", "{row.name}" }
        if !row.detail.is_empty() {
            span { class: "suggest-input-detail", "{row.detail}" }
        }
        if row.sheet {
            span {
                class: "suggest-input-sheet",
                title: i18n.t("suggest_input.sheet_title"),
                {i18n.t("suggest_input.sheet")}
            }
        }
    }
}

/// What `field` suggests for `typed`, asked once the keystrokes settle: at
/// most `limit` values, nothing for empty text or a tree whose entry
/// suggestions are off. Given names follow the word being typed. A `scope`
/// counts only the persons a search on it finds (`NameScope`).
pub(crate) fn use_value_suggestions(
    tree_id: Option<Uuid>,
    field: SuggestionField,
    typed: Signal<String>,
    limit: usize,
    scope: Memo<NameScope>,
) -> Resource<Vec<ValueSuggestion>> {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let tree_cache = use_tree_cache();
    let mut debounced = use_signal(String::new);
    let _debounce = use_ui_resource("value_input_debounce", move || {
        let text = typed();
        async move {
            sleep_ms(DEBOUNCE_MS).await;
            debounced.set(text);
        }
    });
    use_ui_resource("value_input_suggest", move || {
        let api = api.clone();
        let text = debounced();
        let scope = scope();
        let lang = i18n.0.code();
        let enabled = tree_cache.entry_suggestions();
        async move {
            let query = match field {
                SuggestionField::GivenNames => last_word(&text).1,
                _ => text.trim(),
            };
            let Some(tree_id) = tree_id.filter(|_| enabled && !query.is_empty()) else {
                return Vec::new();
            };
            api.value_suggestions(tree_id, field, lang, query, limit, &scope)
                .await
                .unwrap_or_default()
        }
    })
}

/// The rows listing `suggestions`: each value, then how many persons carry
/// it (citations for a source).
pub(crate) fn suggest_rows(
    suggestions: &[ValueSuggestion],
    field: SuggestionField,
    i18n: &I18n,
) -> Vec<SuggestRow> {
    suggestions
        .iter()
        .map(|s| SuggestRow {
            name: s.value.clone(),
            detail: match (s.count, field) {
                (0, _) => String::new(),
                (count, SuggestionField::Sources) => {
                    i18n.t_plural("dictionary.citation_count", count as usize)
                }
                (count, _) => i18n.t_plural("dictionary.person_count", count as usize),
            },
            sheet: s.reference,
        })
        .collect()
}

/// The field's text once `picked` is picked: the value itself, or for given
/// names, the text with its last word replaced.
pub(crate) fn picked_text(field: SuggestionField, current: &str, picked: &str) -> String {
    match field {
        SuggestionField::GivenNames => format!("{}{picked}", last_word(current).0),
        _ => picked.to_owned(),
    }
}

/// A free-text field suggesting what `field` holds across the tree, then,
/// for occupations and given names, the terms a reference sheet answers to.
/// Given names are completed word by word: the list follows the word being
/// typed, and picking replaces that word only.
#[component]
pub fn ValueInput(
    value: Signal<String>,
    tree_id: Uuid,
    field: SuggestionField,
    #[props(default)] placeholder: Option<String>,
    /// The field stores its text in capitals, as surname fields do.
    #[props(default)]
    uppercase: bool,
    /// Offer only what the tree holds, as search filters do: a term no
    /// record carries would find nobody.
    #[props(default)]
    tree_only: bool,
    #[props(default)] on_change: Option<EventHandler<()>>,
) -> Element {
    let i18n = use_i18n();
    let mut value = value;
    // What the user typed last; empty until they type, so a form opening
    // with a filled field asks for nothing.
    let mut typed = use_signal(String::new);
    let whole_tree = use_memo(NameScope::default);
    let found = use_value_suggestions(
        Some(tree_id),
        field,
        typed,
        value_suggestions(field),
        whole_tree,
    );

    let suggestions = suggestions_shown(
        found.read().as_deref().unwrap_or_default(),
        uppercase,
        tree_only,
    );
    let rows = suggest_rows(&suggestions, field, &i18n);

    let changed = move || {
        if let Some(handler) = on_change {
            handler.call(());
        }
    };

    rsx! {
        SuggestInput {
            value: value(),
            rows,
            placeholder,
            on_input: move |text: String| {
                let text = if uppercase { text.to_uppercase() } else { text };
                typed.set(text.clone());
                value.set(text);
                changed();
            },
            on_pick: move |index: usize| {
                let Some(picked) = suggestions.get(index) else {
                    return;
                };
                let text = picked_text(field, &value(), &picked.value);
                typed.set(String::new());
                value.set(text);
                changed();
            },
        }
    }
}

/// The suggestions a field lists: capitalized for a field in capitals, and
/// each spelling once, its counts summed; only the tree's for `tree_only`.
pub(crate) fn suggestions_shown(
    found: &[ValueSuggestion],
    uppercase: bool,
    tree_only: bool,
) -> Vec<ValueSuggestion> {
    let mut shown: Vec<ValueSuggestion> = Vec::with_capacity(found.len());
    for suggestion in found.iter().filter(|s| !tree_only || s.count > 0) {
        let value = if uppercase {
            suggestion.value.to_uppercase()
        } else {
            suggestion.value.clone()
        };
        match shown.iter_mut().find(|s| s.value == value) {
            Some(same) => {
                same.count += suggestion.count;
                same.reference |= suggestion.reference;
            }
            None => shown.push(ValueSuggestion {
                value,
                ..suggestion.clone()
            }),
        }
    }
    shown
}

/// Splits `text` before its last word: what precedes it, spaces included,
/// and the word itself — empty when the text ends with a space.
fn last_word(text: &str) -> (&str, &str) {
    let at = text
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_whitespace())
        .map_or(0, |(at, c)| at + c.len_utf8());
    text.split_at(at)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn suggestion(value: &str, count: i64, reference: bool) -> ValueSuggestion {
        ValueSuggestion {
            value: value.to_string(),
            count,
            reference,
        }
    }

    #[test]
    fn the_word_being_typed_is_the_last_one() {
        assert_eq!(last_word("Jean Ma"), ("Jean ", "Ma"));
        assert_eq!(last_word("Jean  Ma"), ("Jean  ", "Ma"));
        assert_eq!(last_word("Ma"), ("", "Ma"));
        assert_eq!(last_word("Jean "), ("Jean ", ""));
        assert_eq!(last_word("Élise Zoé"), ("Élise ", "Zoé"));
    }

    #[test]
    fn picking_a_given_name_replaces_the_word_being_typed_only() {
        let given = SuggestionField::GivenNames;
        assert_eq!(
            picked_text(given, "Given_a Gi", "Given_b"),
            "Given_a Given_b"
        );
        assert_eq!(picked_text(given, "Gi", "Given_b"), "Given_b");
        let surname = SuggestionField::FamilyNames;
        assert_eq!(picked_text(surname, "NAME NA", "NAME_B"), "NAME_B");
    }

    #[test]
    fn a_field_in_capitals_lists_each_spelling_once() {
        let shown = suggestions_shown(
            &[
                suggestion("Name_a", 2, false),
                suggestion("NAME_A", 3, false),
                suggestion("Name_b", 1, false),
            ],
            true,
            false,
        );
        assert_eq!(
            shown,
            [
                suggestion("NAME_A", 5, false),
                suggestion("NAME_B", 1, false)
            ]
        );
    }

    #[test]
    fn a_search_filter_lists_only_what_the_tree_holds() {
        let shown = suggestions_shown(
            &[
                suggestion("Given_a", 2, true),
                suggestion("Given_b", 0, true),
            ],
            false,
            true,
        );
        assert_eq!(shown, [suggestion("Given_a", 2, true)]);
    }
}
