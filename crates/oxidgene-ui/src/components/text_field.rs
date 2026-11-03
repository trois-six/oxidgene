//! The one text field every form and search uses: [`TextInput`] and
//! [`TextArea`] (docs/ui-common.md §6.6).
//!
//! A field bound to its text by `value` is set again by every render whose
//! text changed, the user's own keystrokes included. The renderer's changes
//! reach the page after the keystroke that caused them, and on the desktop
//! later keystrokes land meanwhile when the render is slow: typing quickly,
//! the field is set back to `ab` once `abc` is in it, and the `c` is lost.
//! These fields therefore take their text when they are created
//! (`initial_value`), and afterwards only when the application changes it
//! to something the user did not type — a suggestion picked, a surname put
//! in capitals, a form reset — and only while the field still reads what
//! the user last typed: otherwise a newer keystroke is on its way, and wins.

use dioxus::core::AttributeValue;
use dioxus::html::events;
use dioxus::prelude::*;

use crate::listeners::use_element_id;

/// A one-line text field showing `value`, reporting each keystroke's text
/// through `on_input`. Any attribute of `input` passes through, `id`
/// included; the field's listeners are the props below.
#[component]
pub fn TextInput(
    value: String,
    on_input: EventHandler<String>,
    #[props(default)] on_keydown: Option<EventHandler<KeyboardEvent>>,
    #[props(default)] on_focus: Option<EventHandler<FocusEvent>>,
    #[props(default)] on_blur: Option<EventHandler<FocusEvent>>,
    #[props(default)] on_mounted: Option<EventHandler<MountedEvent>>,
    /// Both `input` and the global attributes name it: a prop of its own
    /// says which is meant.
    #[props(default)]
    spellcheck: Option<bool>,
    #[props(extends = GlobalAttributes, extends = input)] attributes: Vec<Attribute>,
) -> Element {
    let listeners = Listeners {
        on_keydown,
        on_focus,
        on_blur,
        on_mounted,
    };
    let (field, attributes) = use_field(attributes, &value, listeners);
    rsx! {
        input {
            initial_value: value,
            spellcheck: spellcheck.map(|on| on.to_string()),
            oninput: move |e: FormEvent| {
                field.typed(&e.value());
                on_input.call(e.value());
            },
            ..attributes,
        }
    }
}

/// [`TextInput`] over several lines: any attribute of `textarea` passes
/// through.
#[component]
pub fn TextArea(
    value: String,
    on_input: EventHandler<String>,
    #[props(default)] on_keydown: Option<EventHandler<KeyboardEvent>>,
    #[props(default)] on_focus: Option<EventHandler<FocusEvent>>,
    #[props(default)] on_blur: Option<EventHandler<FocusEvent>>,
    #[props(default)] on_mounted: Option<EventHandler<MountedEvent>>,
    /// Both `input` and the global attributes name it: a prop of its own
    /// says which is meant.
    #[props(default)]
    spellcheck: Option<bool>,
    #[props(extends = GlobalAttributes, extends = textarea)] attributes: Vec<Attribute>,
) -> Element {
    let listeners = Listeners {
        on_keydown,
        on_focus,
        on_blur,
        on_mounted,
    };
    let (field, attributes) = use_field(attributes, &value, listeners);
    rsx! {
        textarea {
            initial_value: value,
            spellcheck: spellcheck.map(|on| on.to_string()),
            oninput: move |e: FormEvent| {
                field.typed(&e.value());
                on_input.call(e.value());
            },
            ..attributes,
        }
    }
}

/// The listeners a field may have besides its input.
struct Listeners {
    on_keydown: Option<EventHandler<KeyboardEvent>>,
    on_focus: Option<EventHandler<FocusEvent>>,
    on_blur: Option<EventHandler<FocusEvent>>,
    on_mounted: Option<EventHandler<MountedEvent>>,
}

impl Listeners {
    /// Those given, as attributes: a field without a key handler sends its
    /// keystrokes nowhere but its input.
    fn into_attributes(self) -> Vec<Attribute> {
        let mut attributes = Vec::new();
        if let Some(handler) = self.on_keydown {
            attributes.push(events::onkeydown(move |e| handler.call(e)));
        }
        if let Some(handler) = self.on_focus {
            attributes.push(events::onfocus(move |e| handler.call(e)));
        }
        if let Some(handler) = self.on_blur {
            attributes.push(events::onblur(move |e| handler.call(e)));
        }
        if let Some(handler) = self.on_mounted {
            attributes.push(events::onmounted(move |e| handler.call(e)));
        }
        attributes
    }
}

/// The field's text handle, and the attributes it spreads: the caller's,
/// its listeners and its id — the caller's own, or one of its own.
fn use_field(
    mut attributes: Vec<Attribute>,
    text: &str,
    listeners: Listeners,
) -> (FieldText, Vec<Attribute>) {
    let own_id = use_element_id("field");
    let given_id = attributes
        .iter()
        .position(|attribute| attribute.name == "id")
        .map(|at| attributes.remove(at))
        .and_then(|attribute| match attribute.value {
            AttributeValue::Text(id) => Some(id),
            _ => None,
        });
    let field = use_field_text(given_id.unwrap_or(own_id), text);
    attributes.extend(listeners.into_attributes());
    attributes.push(Attribute::new("id", field.id(), None, false));
    // The renderer diffs an element's spread attributes as a sorted list.
    attributes.sort_by_key(|attribute| attribute.name);
    (field, attributes)
}

/// A field's element id and the text the field reads, as far as Rust knows.
#[derive(Clone, Copy)]
struct FieldText {
    id: CopyValue<String>,
    shown: CopyValue<String>,
}

impl FieldText {
    fn id(&self) -> String {
        self.id.cloned()
    }

    /// Records `text` as what the user typed, before it goes on to the
    /// application.
    fn typed(mut self, text: &str) {
        self.shown.set(text.to_owned());
    }
}

/// The handle of the field `id` showing `text`: a render whose `text` is
/// not what the field reads writes it into the field.
fn use_field_text(id: String, text: &str) -> FieldText {
    let field = use_hook(|| FieldText {
        id: CopyValue::new(id),
        shown: CopyValue::new(text.to_owned()),
    });
    let mut shown = field.shown;
    let was = rewrite(&mut shown.write(), text);
    if let Some(was) = was {
        let _ = document::eval(&rewrite_script(&field.id(), &was, text));
    }
    field
}

/// What the field read before `text` replaced it in `shown`, or `None` when
/// it already reads `text` — the echo of a keystroke.
fn rewrite(shown: &mut String, text: &str) -> Option<String> {
    (shown != text).then(|| std::mem::replace(shown, text.to_owned()))
}

/// Sets the field `id` to `text` if it still reads `was`.
fn rewrite_script(id: &str, was: &str, text: &str) -> String {
    let [id, was, text] = [id, was, text].map(|s| serde_json::to_string(s).unwrap_or_default());
    format!(
        "const field = document.getElementById({id});\n\
         if (field && field.value === {was}) field.value = {text};"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_keystroke_echoed_back_writes_nothing() {
        let mut shown = "ab".to_owned();
        assert_eq!(rewrite(&mut shown, "ab"), None);
        assert_eq!(shown, "ab");
    }

    #[test]
    fn text_the_user_did_not_type_replaces_what_they_typed() {
        let mut shown = "name_a".to_owned();
        assert_eq!(rewrite(&mut shown, "NAME_A").as_deref(), Some("name_a"));
        assert_eq!(shown, "NAME_A");
        // A pick returning to an earlier text is still written.
        let mut shown = "Given_".to_owned();
        assert_eq!(rewrite(&mut shown, "Given_a").as_deref(), Some("Given_"));
    }

    #[test]
    fn the_field_is_written_only_while_it_reads_what_was_typed() {
        assert_eq!(
            rewrite_script("field-3", "a\"b", "A\"B"),
            "const field = document.getElementById(\"field-3\");\n\
             if (field && field.value === \"a\\\"b\") field.value = \"A\\\"B\";"
        );
    }
}
