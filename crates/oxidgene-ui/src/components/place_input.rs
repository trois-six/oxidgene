//! The one place field every form uses: free text, with suggestions from the
//! tree's own places and from the place dictionary.
//!
//! The field's signal holds either the id of a tree place picked from the
//! suggestions, or text — a dictionary label or anything typed. Nothing is
//! written while the form is open: on save, [`resolve_place`] turns text into
//! a place, reusing the tree's place of that name or creating it. See
//! `docs/ui-common.md` §4.4.

use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::{ApiClient, ApiError, CreatePlaceBody, PlaceSuggestion};
use crate::i18n::{I18n, use_i18n};
use crate::ui_observability::use_ui_resource;
use crate::utils::sleep_ms;

/// Keystrokes settle for this long before the dictionary is asked.
const DEBOUNCE_MS: u32 = 300;
/// Shortest text worth asking the dictionary about.
const MIN_QUERY_CHARS: usize = 3;
/// Tree places shown at once, above the dictionary's.
const TREE_SUGGESTIONS: usize = 5;
/// Dictionary places shown at once.
const DICTIONARY_SUGGESTIONS: usize = 8;

/// A labelled place field, bound to `value` (see the module documentation).
/// `options` are the tree's places as `(id, name)`.
pub(crate) fn render_place_input(
    i18n: &I18n,
    value: Signal<String>,
    options: &[(String, String)],
    mut on_change: impl FnMut() + 'static,
) -> Element {
    rsx! {
        div { class: "form-group",
            label { {i18n.t("person_form.place")} }
            PlaceInput {
                value,
                options: options.to_vec(),
                on_change: move |()| on_change(),
            }
        }
    }
}

/// One entry of the open list.
#[derive(Clone, PartialEq)]
enum Choice {
    Tree { id: String, name: String },
    Dictionary(PlaceSuggestion),
}

#[component]
pub fn PlaceInput(
    value: Signal<String>,
    options: Vec<(String, String)>,
    #[props(default)] on_change: Option<EventHandler<()>>,
) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let mut value = value;
    let mut open = use_signal(|| false);
    let mut highlight = use_signal(|| None::<usize>);

    // What the person typed; empty while the field holds a picked place.
    let typed = use_memo(move || {
        let raw = value();
        if Uuid::parse_str(&raw).is_ok() {
            String::new()
        } else {
            raw.trim().to_string()
        }
    });
    let mut debounced = use_signal(String::new);
    let _debounce = use_ui_resource("place_input_debounce", move || {
        let text = typed();
        async move {
            sleep_ms(DEBOUNCE_MS).await;
            debounced.set(text);
        }
    });
    let dictionary = use_ui_resource("place_input_suggest", move || {
        let api = api.clone();
        let text = debounced();
        let wanted = open();
        let lang = i18n.0.code();
        async move {
            if !wanted || text.chars().count() < MIN_QUERY_CHARS {
                return Vec::new();
            }
            api.place_suggestions(lang, &text, DICTIONARY_SUGGESTIONS)
                .await
                .unwrap_or_default()
        }
    });

    let raw = value();
    let picked = options.iter().find(|(id, _)| *id == raw);
    // A picked place whose name has not loaded yet shows nothing rather than
    // its id.
    let shown = match picked {
        Some((_, name)) => name.clone(),
        None if Uuid::parse_str(&raw).is_ok() => String::new(),
        None => raw.clone(),
    };

    let text = typed();
    let key = fold(&text);
    let mut choices: Vec<Choice> = if key.is_empty() {
        Vec::new()
    } else {
        options
            .iter()
            .filter(|(_, name)| starts_a_word(&fold(name), &key))
            .take(TREE_SUGGESTIONS)
            .map(|(id, name)| Choice::Tree {
                id: id.clone(),
                name: name.clone(),
            })
            .collect()
    };
    let tree_count = choices.len();
    if let Some(found) = &*dictionary.read() {
        // A dictionary place the tree already holds is offered as the tree's.
        choices.extend(
            found
                .iter()
                .filter(|s| !options.iter().any(|(_, name)| *name == s.label))
                .cloned()
                .map(Choice::Dictionary),
        );
    }
    let list_visible = open() && !choices.is_empty();

    let mut choose = move |choice: &Choice| {
        match choice {
            Choice::Tree { id, .. } => value.set(id.clone()),
            Choice::Dictionary(place) => value.set(place.label.clone()),
        }
        open.set(false);
        highlight.set(None);
        if let Some(handler) = on_change {
            handler.call(());
        }
    };

    let count = choices.len();
    let keyboard_choices = choices.clone();
    let on_key = move |e: Event<KeyboardData>| match e.key() {
        Key::ArrowDown if count > 0 => {
            e.prevent_default();
            open.set(true);
            highlight.set(Some(highlight().map_or(0, |i| (i + 1) % count)));
        }
        Key::ArrowUp if count > 0 => {
            e.prevent_default();
            open.set(true);
            highlight.set(Some(
                highlight().map_or(count - 1, |i| (i + count - 1) % count),
            ));
        }
        Key::Enter => {
            if let Some(choice) = highlight()
                .filter(|_| open())
                .and_then(|i| keyboard_choices.get(i))
            {
                // Enter picks the place instead of submitting the form.
                e.prevent_default();
                choose(choice);
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
        div { class: "place-input",
            input {
                r#type: "text",
                value: "{shown}",
                placeholder: "{i18n.t(\"place_input.placeholder\")}",
                autocomplete: "off",
                "aria-autocomplete": "list",
                "aria-expanded": "{list_visible}",
                oninput: move |e: Event<FormData>| {
                    value.set(e.value());
                    open.set(true);
                    highlight.set(None);
                    if let Some(handler) = on_change {
                        handler.call(());
                    }
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
                    class: "place-input-list",
                    role: "listbox",
                    onmousedown: move |e: Event<MouseData>| e.prevent_default(),
                    if tree_count > 0 {
                        div { class: "context-menu-header", {i18n.t("place_input.tree_places")} }
                    }
                    for (index, choice) in choices.iter().enumerate().take(tree_count) {
                        {render_option(index, choice, highlight, choose, &i18n)}
                    }
                    if choices.len() > tree_count {
                        div { class: "context-menu-header", {i18n.t("place_input.dictionary")} }
                    }
                    for (index, choice) in choices.iter().enumerate().skip(tree_count) {
                        {render_option(index, choice, highlight, choose, &i18n)}
                    }
                }
            }
        }
    }
}

fn render_option(
    index: usize,
    choice: &Choice,
    mut highlight: Signal<Option<usize>>,
    mut choose: impl FnMut(&Choice) + 'static,
    i18n: &I18n,
) -> Element {
    let picked = choice.clone();
    rsx! {
        button {
            key: "{index}",
            r#type: "button",
            role: "option",
            class: if highlight() == Some(index) {
                "context-menu-item td-suggest-row is-active"
            } else {
                "context-menu-item td-suggest-row"
            },
            onmouseenter: move |_| highlight.set(Some(index)),
            onclick: move |_| choose(&picked),
            {render_choice(choice, i18n)}
        }
    }
}

fn render_choice(choice: &Choice, i18n: &I18n) -> Element {
    match choice {
        Choice::Tree { name, .. } => rsx! { span { "{name}" } },
        Choice::Dictionary(place) => {
            let detail = place
                .label
                .strip_prefix(&place.name)
                .map(|rest| rest.trim_start_matches(", ").to_string())
                .unwrap_or_default();
            let until = place
                .valid_until
                .as_deref()
                .and_then(|date| date.get(..4))
                .map(|year| i18n.t_args("place_input.until", &[("year", year)]));
            rsx! {
                span { class: "place-input-name", "{place.name}" }
                if !detail.is_empty() {
                    span { class: "place-input-detail", " {detail}" }
                }
                if let Some(until) = until {
                    span { class: "place-input-detail", " · {until}" }
                }
            }
        }
    }
}

/// Turns what a place field holds into a place id, for saving: nothing for
/// an empty field, the id of a picked tree place, and for text the tree's
/// place of that name, created when the tree has none. A place created from
/// a dictionary label takes the dictionary's coordinates.
pub(crate) async fn resolve_place(
    api: &ApiClient,
    tree_id: Uuid,
    value: &str,
    lang: &str,
) -> Result<Option<Uuid>, ApiError> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if let Ok(id) = Uuid::parse_str(value) {
        return Ok(Some(id));
    }
    let places = api.list_all_places(tree_id).await?;
    if let Some(place) = places
        .iter()
        .find(|p| p.name.trim().to_lowercase() == value.to_lowercase())
    {
        return Ok(Some(place.id));
    }
    // Only the coordinates are wanted: a failed lookup still creates the
    // place, without them.
    let known = api
        .place_suggestions(lang, value, DICTIONARY_SUGGESTIONS)
        .await
        .unwrap_or_default()
        .into_iter()
        .find(|s| s.label == value);
    let body = CreatePlaceBody {
        name: value.to_string(),
        latitude: known.as_ref().and_then(|s| s.latitude),
        longitude: known.as_ref().and_then(|s| s.longitude),
    };
    Ok(Some(api.create_place(tree_id, &body).await?.id))
}

/// Lowercase and without the accents of Latin scripts, so "etienne" finds
/// "Étienne". The backend folds dictionary names the same way.
fn fold(text: &str) -> String {
    let mut folded = String::with_capacity(text.len());
    for c in text.chars().flat_map(char::to_lowercase) {
        match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => folded.push('a'),
            'ç' => folded.push('c'),
            'è' | 'é' | 'ê' | 'ë' => folded.push('e'),
            'ì' | 'í' | 'î' | 'ï' => folded.push('i'),
            'ñ' => folded.push('n'),
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' => folded.push('o'),
            'ù' | 'ú' | 'û' | 'ü' => folded.push('u'),
            'ý' | 'ÿ' => folded.push('y'),
            'œ' => folded.push_str("oe"),
            'æ' => folded.push_str("ae"),
            c if c.is_alphanumeric() => folded.push(c),
            _ => folded.push(' '),
        }
    }
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn starts_a_word(text: &str, query: &str) -> bool {
    text.match_indices(query)
        .any(|(at, _)| at == 0 || text.as_bytes()[at - 1] == b' ')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folding_ignores_case_accents_and_punctuation() {
        assert_eq!(fold("Saint-Étienne-d'Œuf"), "saint etienne d oeuf");
    }

    #[test]
    fn a_tree_place_matches_from_the_start_of_a_word() {
        assert!(starts_a_word(&fold("Le Bourg-Neuf"), "neuf"));
        assert!(starts_a_word(&fold("Le Bourg-Neuf"), "le bourg"));
        assert!(!starts_a_word(&fold("Le Bourg-Neuf"), "ourg"));
    }
}
