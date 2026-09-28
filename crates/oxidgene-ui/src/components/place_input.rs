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
use crate::components::suggest_input::{DEBOUNCE_MS, SuggestInput, SuggestRow};
use crate::i18n::{I18n, use_i18n};
use crate::ui_observability::use_ui_resource;
use crate::utils::sleep_ms;

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

    // What the person typed; empty while the field holds a picked place.
    let typed = use_memo(move || {
        let raw = value();
        if Uuid::parse_str(&raw).is_ok() {
            String::new()
        } else {
            raw.trim().to_string()
        }
    });
    // Only what the user types is sent to the dictionary, not the text a
    // form opens with.
    let mut asked = use_signal(String::new);
    let mut debounced = use_signal(String::new);
    let _debounce = use_ui_resource("place_input_debounce", move || {
        let text = asked();
        async move {
            sleep_ms(DEBOUNCE_MS).await;
            debounced.set(text);
        }
    });
    let dictionary = use_ui_resource("place_input_suggest", move || {
        let api = api.clone();
        let text = debounced();
        let lang = i18n.0.code();
        async move {
            if text.chars().count() < MIN_QUERY_CHARS {
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

    let key = fold(&typed());
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
    if let Some(found) = &*dictionary.read() {
        // One list: the tree's places first, then the dictionary's, a
        // dictionary label the tree already holds being offered once, as the
        // tree's place.
        choices.extend(
            found
                .iter()
                .filter(|s| !options.iter().any(|(_, name)| *name == s.label))
                .cloned()
                .map(Choice::Dictionary),
        );
    }
    let rows: Vec<SuggestRow> = choices.iter().map(|c| row(c, &i18n)).collect();

    let changed = move || {
        if let Some(handler) = on_change {
            handler.call(());
        }
    };

    rsx! {
        SuggestInput {
            value: shown,
            rows,
            placeholder: i18n.t("place_input.placeholder"),
            on_input: move |text: String| {
                asked.set(text.trim().to_string());
                value.set(text);
                changed();
            },
            on_pick: move |index: usize| {
                match choices.get(index) {
                    Some(Choice::Tree { id, .. }) => value.set(id.clone()),
                    Some(Choice::Dictionary(place)) => value.set(place.label.clone()),
                    None => return,
                }
                asked.set(String::new());
                changed();
            },
        }
    }
}

/// Tree places and dictionary places read alike: the place's own name,
/// then the rest of its label.
fn row(choice: &Choice, i18n: &I18n) -> SuggestRow {
    let (name, mut detail) = match choice {
        Choice::Tree { name, .. } => match name.split_once(", ") {
            Some((head, rest)) => (head.to_string(), rest.to_string()),
            None => (name.clone(), String::new()),
        },
        Choice::Dictionary(place) => (
            place.name.clone(),
            place
                .label
                .strip_prefix(&place.name)
                .map(|rest| rest.trim_start_matches(", ").to_string())
                .unwrap_or_default(),
        ),
    };
    let until = match choice {
        Choice::Dictionary(place) => place
            .valid_until
            .as_deref()
            .and_then(|date| date.get(..4))
            .map(|year| i18n.t_args("place_input.until", &[("year", year)])),
        Choice::Tree { .. } => None,
    };
    if let Some(until) = until {
        detail = if detail.is_empty() {
            until
        } else {
            format!("{detail} · {until}")
        };
    }
    SuggestRow {
        name,
        detail,
        sheet: false,
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
