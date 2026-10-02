//! The one place field every form uses: free text, with suggestions from the
//! tree's own places and from the place dictionary.
//!
//! The field's signal holds either the id of the place the record already
//! sits on, or text — a tree place or a dictionary label picked from the
//! suggestions, or anything typed. Nothing is written while the form is open:
//! on save, [`resolve_place`] turns text into a place, reusing the tree's
//! place of that name or creating it. The server answers both the
//! suggestions and the lookup, so no form reads the tree's whole place list.
//! See `docs/ui-common.md` §4.4.

use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::{
    ApiClient, ApiError, CreatePlaceBody, NameScope, PlaceSuggestion, SuggestionField,
    ValueSuggestion,
};
use crate::components::suggest_input::{DEBOUNCE_MS, SuggestInput, SuggestRow};
use crate::components::tree_cache::use_tree_cache;
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
/// `known` names the places the form's record already sits on, as
/// `(id, name)`.
pub(crate) fn render_place_input(
    i18n: &I18n,
    tree_id: Uuid,
    value: Signal<String>,
    known: &[(String, String)],
    mut on_change: impl FnMut() + 'static,
) -> Element {
    rsx! {
        div { class: "form-group",
            label { {i18n.t("person_form.place")} }
            PlaceInput {
                tree_id,
                value,
                known: known.to_vec(),
                on_change: move |()| on_change(),
            }
        }
    }
}

/// One entry of the open list.
#[derive(Clone, PartialEq)]
enum Choice {
    Tree(String),
    Dictionary(PlaceSuggestion),
}

/// The tree's place names (with `tree_places`, from the first character)
/// and the dictionary's places (from [`MIN_QUERY_CHARS`]) for `text`, asked
/// together.
async fn suggest(
    api: &ApiClient,
    tree_id: Uuid,
    lang: &'static str,
    text: String,
    tree_places: bool,
) -> (Vec<ValueSuggestion>, Vec<PlaceSuggestion>) {
    let scope = NameScope::default();
    let tree = async {
        if !tree_places || text.is_empty() {
            return Vec::new();
        }
        api.value_suggestions(
            tree_id,
            SuggestionField::Places,
            lang,
            &text,
            TREE_SUGGESTIONS,
            &scope,
        )
        .await
        .unwrap_or_default()
    };
    let dictionary = async {
        if text.chars().count() < MIN_QUERY_CHARS {
            return Vec::new();
        }
        api.place_suggestions(lang, &text, DICTIONARY_SUGGESTIONS)
            .await
            .unwrap_or_default()
    };
    futures_util::future::join(tree, dictionary).await
}

#[component]
pub fn PlaceInput(
    tree_id: Uuid,
    value: Signal<String>,
    known: Vec<(String, String)>,
    /// Offer the tree's own places beside the dictionary's.
    #[props(default = true)]
    tree_places: bool,
    #[props(default)] on_change: Option<EventHandler<()>>,
) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let tree_cache = use_tree_cache();
    let mut value = value;

    // Only what the user types is asked about, not the text a form opens
    // with.
    let mut asked = use_signal(String::new);
    let mut debounced = use_signal(String::new);
    let _debounce = use_ui_resource("place_input_debounce", move || {
        let text = asked();
        async move {
            sleep_ms(DEBOUNCE_MS).await;
            debounced.set(text);
        }
    });
    let suggestions = use_ui_resource("place_input_suggest", move || {
        let api = api.clone();
        let text = debounced();
        let lang = i18n.0.reference_code();
        let enabled = tree_cache.entry_suggestions();
        async move {
            if !enabled {
                return (Vec::new(), Vec::new());
            }
            suggest(&api, tree_id, lang, text, tree_places).await
        }
    });

    let raw = value();
    let picked = known.iter().find(|(id, _)| *id == raw);
    let shown = match picked {
        Some((_, name)) => name.clone(),
        None if Uuid::parse_str(&raw).is_ok() => String::new(),
        None => raw.clone(),
    };

    let mut choices: Vec<Choice> = Vec::new();
    if let Some((tree, dictionary)) = &*suggestions.read() {
        choices.extend(tree.iter().map(|s| Choice::Tree(s.value.clone())));
        choices.extend(
            dictionary
                .iter()
                .filter(|s| !tree.iter().any(|t| t.value == s.label))
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
                    Some(Choice::Tree(name)) => value.set(name.clone()),
                    Some(Choice::Dictionary(place)) => value.set(place.label.clone()),
                    None => return,
                }
                asked.set(String::new());
                changed();
            },
        }
    }
}

fn row(choice: &Choice, i18n: &I18n) -> SuggestRow {
    let (name, mut detail) = match choice {
        Choice::Tree(name) => match name.split_once(", ") {
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
        Choice::Tree(_) => None,
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
    // The server finds the tree's place of that name, trimmed and ignoring
    // case, rather than the form reading every place of the tree.
    if let Some(place) = api.find_place(tree_id, value).await? {
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
