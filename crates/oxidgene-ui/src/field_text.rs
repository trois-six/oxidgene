//! The text of a field that both the user and the application write
//! (docs/ui-common.md §6.6).
//!
//! A field bound to its text by `value` is set again by every render whose
//! text changed, the user's own keystrokes included. The renderer's changes
//! reach the page after the keystroke that caused them, and on the desktop
//! later keystrokes land meanwhile when the render is slow: typing quickly,
//! the field is set back to `ab` once `abc` is in it, and the `c` is lost.
//! Such a field therefore takes its text when it is created
//! (`initial_value`), and afterwards only when the application changes it
//! to something the user did not type — a suggestion picked, a surname put
//! in capitals — and only while the field still reads what the user last
//! typed: otherwise a newer keystroke is on its way, and wins.

use dioxus::prelude::*;

use crate::listeners::use_element_id;

/// A field's element id and the text the field reads, as far as Rust knows.
#[derive(Clone, Copy)]
pub struct FieldText {
    id: CopyValue<String>,
    shown: CopyValue<String>,
}

impl FieldText {
    /// The id the field's element carries.
    pub fn id(&self) -> String {
        self.id.cloned()
    }

    /// Records `text` as what the user typed: called by the field's
    /// `oninput`, before the text goes on to the application.
    pub fn typed(mut self, text: &str) {
        self.shown.set(text.to_owned());
    }
}

/// The handle of a field showing `text`: the field carries its `id()` and
/// `initial_value: text`, and reports each keystroke through `typed`, never
/// binding `value`. A render whose `text` is not what the field reads writes
/// it into the field.
pub fn use_field_text(text: &str) -> FieldText {
    let id = use_element_id("field");
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
        let mut shown = "pi".to_owned();
        assert_eq!(rewrite(&mut shown, "pi"), None);
        assert_eq!(shown, "pi");
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
