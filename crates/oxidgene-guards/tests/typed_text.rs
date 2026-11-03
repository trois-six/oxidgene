//! Typed text: no field the user types into is bound to its text by
//! `value`.
//!
//! Drift it prevents: a field bound by `value` is set again by every render
//! whose text changed, the user's keystrokes included. The renderer's
//! changes reach the page after the keystroke that caused them, so when a
//! render is slow a later keystroke already in the field is undone, and
//! typing quickly loses letters (docs/ui-common.md §6.6).
//!
//! The fields are the `input { … }` and `textarea { … }` elements of the
//! markup that have both a `value:` and an `oninput:`; a range, a checkbox
//! or a radio button is not typed into. `components/text_field.rs` draws
//! the shared fields and is the one place that writes such an element.
//!
//! Fixing a failure: draw the field with `TextInput` or `TextArea`
//! (`components/text_field.rs`), which take `value` and `on_input` as props
//! and pass every other attribute through.

use oxidgene_guards::{files, line_of, read, relative, without_test_modules};

/// Input types the user does not type into.
const NOT_TYPED: &[&str] = &["range", "checkbox", "radio", "file", "color"];

/// The byte offset of each `input` or `textarea` element bound by `value`
/// and listening to `oninput` in `source`.
fn bound_fields(source: &str) -> Vec<usize> {
    let mut found = Vec::new();
    for element in ["input {", "textarea {"] {
        for (at, _) in source.match_indices(element) {
            let before = source[..at].chars().next_back();
            if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                continue;
            }
            let body = element_body(&source[at + element.len()..]);
            let typed = !NOT_TYPED
                .iter()
                .any(|kind| body.contains(&format!("r#type: \"{kind}\"")));
            if typed && has_attribute(body, "value") && has_attribute(body, "oninput") {
                found.push(at);
            }
        }
    }
    found.sort_unstable();
    found
}

/// The text of an element up to its closing brace.
fn element_body(rest: &str) -> &str {
    let mut depth = 1;
    for (at, c) in rest.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &rest[..at];
                }
            }
            _ => {}
        }
    }
    rest
}

/// Whether `body` sets the attribute `name`, as a word of its own.
fn has_attribute(body: &str, name: &str) -> bool {
    body.match_indices(&format!("{name}:")).any(|(at, _)| {
        !body[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_')
    })
}

#[test]
fn no_typed_field_is_bound_to_its_text() {
    let mut found = Vec::new();
    for path in files("crates/oxidgene-ui/src", "rs") {
        let file = relative(&path);
        if file.ends_with("components/text_field.rs") {
            continue;
        }
        let source = read(&file);
        let source = without_test_modules(&source);
        for at in bound_fields(source) {
            found.push(format!("{file}:{}", line_of(source, at)));
        }
    }
    assert!(
        found.is_empty(),
        "fields bound to their text by `value`, which fast typing undoes \
         (see this guard's header for the fix):\n{}",
        found.join("\n")
    );
}

#[test]
fn the_reader_finds_bound_fields_only() {
    let source = "rsx! {\n\
        input { value: \"{a}\", oninput: move |e| a.set(e.value()) }\n\
        textarea { class: \"x\", value: \"{b}\", oninput: move |e| { b.set(e.value()) } }\n\
        input { r#type: \"range\", value: \"{c}\", oninput: move |e| c.set(e.value()) }\n\
        input { readonly: true, value: \"{d}\" }\n\
        TextInput { value: \"{e}\", on_input: move |t| e.set(t) }\n\
        input { initial_value: \"{f}\", oninput: move |e| f.set(e.value()) }\n\
    }";
    let lines: Vec<usize> = bound_fields(source)
        .into_iter()
        .map(|at| line_of(source, at))
        .collect();
    assert_eq!(lines, [2, 3]);
}
