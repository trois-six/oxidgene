//! Menu buttons: every element that opens a context menu carries the shared
//! "⋮" trigger in the same element.
//!
//! A menu opened only by right-click, long press or the menu key is invisible
//! to a touch user without a long press and to anyone who does not know the
//! shortcut. The rule (docs/ui-common.md §4.8): each element with secondary
//! actions shows a `MenuButton` at its trailing edge; right-click, the menu key
//! and long press are shortcuts to the same menu.
//!
//! The openers scanned for are the markup listeners that open a menu
//! (`oncontextmenu:`), the shared shortcut calls that open it from a pointer or
//! a key (`shortcut.right_click(`, `shortcut.key(`, `opens_menu(`) and the
//! shared record-menu hooks that call it (`open_at_pointer(`,
//! `open_from_keyboard(`, `press_start(`, which also covers the long press of
//! `MenuShortcut::press_start`). The innermost
//! rsx element that holds an opener must contain a `MenuButton`.
//!
//! An element whose menu key only activates the trigger of another element
//! (a chart canvas that delegates to the focus person's ⋮) names that target
//! in a `menu-guard: delegates to <what>` comment inside the element. The
//! comment is the documented reason for the exception.
//!
//! Fixing a failure: wrap the element's actions in a `MenuButton { name, items }`
//! as the element's trailing control. Do not remove the shortcut; it opens the
//! same menu.

use oxidgene_guards::{files, line_of, read, relative, without_test_modules};

/// Calls and listeners that open a context menu from a pointer or a key.
const OPENERS: &[&str] = &[
    "oncontextmenu:",
    "shortcut.right_click(",
    "shortcut.key(",
    "opens_menu(",
    "open_at_pointer(",
    "open_from_keyboard(",
    "press_start(",
];

/// The comment that documents an element whose menu key delegates to the
/// trigger of another element.
const DELEGATION: &str = "menu-guard: delegates to";

/// The shared surface closes its menu on a right click; it opens nothing.
const SURFACE: &str = "crates/oxidgene-ui/src/components/context_menu.rs";

/// Words that precede a block without naming an element.
const NOT_ELEMENTS: &[&str] = &[
    "if", "else", "let", "match", "in", "while", "for", "impl", "struct", "enum", "fn", "mod",
    "trait", "return", "move", "loop", "unsafe", "async", "where", "as", "dyn",
];

/// Blanks the interior of comments and string literals, keeping every byte
/// offset and newline, so braces inside text do not count as code.
fn sanitize(source: &str) -> String {
    let bytes: Vec<char> = source.chars().collect();
    let mut out: Vec<char> = bytes.clone();
    let blank = |out: &mut Vec<char>, from: usize, to: usize| {
        for c in &mut out[from..to] {
            if *c != '\n' {
                *c = ' ';
            }
        }
    };
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == '/' && bytes.get(i + 1) == Some(&'/') {
            let end = bytes[i..]
                .iter()
                .position(|&c| c == '\n')
                .map_or(bytes.len(), |p| i + p);
            blank(&mut out, i, end);
            i = end;
        } else if c == '/' && bytes.get(i + 1) == Some(&'*') {
            let end = (i + 2..bytes.len().saturating_sub(1))
                .find(|&j| bytes[j] == '*' && bytes[j + 1] == '/')
                .map_or(bytes.len(), |j| j + 2);
            blank(&mut out, i, end);
            i = end;
        } else if c == 'r'
            && matches!(bytes.get(i + 1), Some('#') | Some('"'))
            && !(i > 0 && (bytes[i - 1].is_alphanumeric() || bytes[i - 1] == '_'))
        {
            let hashes = bytes[i + 1..].iter().take_while(|&&c| c == '#').count();
            let quote = i + 1 + hashes;
            if bytes.get(quote) != Some(&'"') {
                i += 1;
                continue;
            }
            let closing: String = std::iter::once('"')
                .chain(std::iter::repeat_n('#', hashes))
                .collect();
            let closing: Vec<char> = closing.chars().collect();
            let end = (quote + 1..bytes.len())
                .find(|&j| bytes[j..].starts_with(&closing))
                .map_or(bytes.len(), |j| j + closing.len());
            blank(&mut out, quote + 1, end.saturating_sub(closing.len()));
            i = end;
        } else if c == '"' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j] != '"' {
                j += if bytes[j] == '\\' { 2 } else { 1 };
            }
            blank(&mut out, i + 1, j.min(bytes.len()));
            i = j + 1;
        } else if c == '\'' && bytes.get(i + 1) == Some(&'\\') {
            let end = (i + 2..bytes.len())
                .find(|&j| bytes[j] == '\'')
                .unwrap_or(bytes.len() - 1);
            blank(&mut out, i + 1, end);
            i = end + 1;
        } else if c == '\'' && bytes.get(i + 2) == Some(&'\'') {
            blank(&mut out, i + 1, i + 2);
            i += 3;
        } else {
            i += 1;
        }
    }
    out.into_iter().collect()
}

/// Every `{ … }` pair of the sanitized source, as (open, close) offsets.
fn brace_pairs(code: &[char]) -> Vec<(usize, usize)> {
    let mut stack = Vec::new();
    let mut pairs = Vec::new();
    for (i, &c) in code.iter().enumerate() {
        match c {
            '{' => stack.push(i),
            '}' => {
                if let Some(open) = stack.pop() {
                    pairs.push((open, i));
                }
            }
            _ => {}
        }
    }
    pairs
}

/// Whether the block opened at `open` is an rsx element or component: a name
/// that is not a keyword, not a path segment, and not a closure or a
/// condition.
fn is_element_block(code: &[char], open: usize) -> bool {
    let mut end = open;
    while end > 0 && code[end - 1].is_whitespace() {
        end -= 1;
    }
    let mut start = end;
    while start > 0 && (code[start - 1].is_alphanumeric() || code[start - 1] == '_') {
        start -= 1;
    }
    if start == end || !code[start].is_alphabetic() {
        return false;
    }
    let name: String = code[start..end].iter().collect();
    if NOT_ELEMENTS.contains(&name.as_str()) {
        return false;
    }
    let mut before = start;
    while before > 0 && code[before - 1].is_whitespace() {
        before -= 1;
    }
    if before == 0 {
        return true;
    }
    let previous = code[before - 1];
    if matches!(
        previous,
        '|' | '=' | '>' | '-' | '(' | '.' | '&' | '!' | '+'
    ) {
        return false;
    }
    if previous == ':' && before >= 2 && code[before - 2] == ':' {
        return false;
    }
    let mut word_start = before;
    while word_start > 0 && (code[word_start - 1].is_alphanumeric() || code[word_start - 1] == '_')
    {
        word_start -= 1;
    }
    let word: String = code[word_start..before].iter().collect();
    !NOT_ELEMENTS.contains(&word.as_str())
}

/// The innermost element block that holds `offset`, as (open, close).
fn enclosing_element(
    code: &[char],
    pairs: &[(usize, usize)],
    offset: usize,
) -> Option<(usize, usize)> {
    let mut holding: Vec<(usize, usize)> = pairs
        .iter()
        .copied()
        .filter(|&(open, close)| open < offset && offset < close)
        .collect();
    holding.sort_by_key(|&(open, _)| std::cmp::Reverse(open));
    holding
        .into_iter()
        .find(|&(open, _)| is_element_block(code, open))
}

/// Opener offsets in `source` that are not definitions, with the element
/// holding them, returned as the element's text.
fn unguarded_openers(source: &str) -> Vec<(usize, &'static str)> {
    let source = without_test_modules(source);
    let code_text = sanitize(source);
    let code: Vec<char> = code_text.chars().collect();
    let raw: Vec<char> = source.chars().collect();
    let pairs = brace_pairs(&code);
    let mut found = Vec::new();
    for opener in OPENERS {
        for (offset, _) in code_text.match_indices(opener) {
            let line_start = code_text[..offset].rfind('\n').map_or(0, |p| p + 1);
            if code_text[line_start..offset].contains("fn ") {
                continue;
            }
            let char_offset = code_text[..offset].chars().count();
            let guarded =
                enclosing_element(&code, &pairs, char_offset).is_some_and(|(open, close)| {
                    let body: String = code[open..close].iter().collect();
                    // The delegation comment is blanked in `code`, so it is
                    // read from the source text, which has the same offsets.
                    let commented: String = raw[open..close].iter().collect();
                    body.contains("MenuButton {")
                        || body.contains("MenuButton{")
                        || commented.contains(DELEGATION)
                });
            if !guarded {
                found.push((offset, *opener));
            }
        }
    }
    found
}

#[test]
fn every_context_menu_opener_has_a_menu_button_in_its_element() {
    let mut failures = Vec::new();
    for path in files("crates/oxidgene-ui/src", "rs") {
        let file = relative(&path);
        if file == SURFACE {
            continue;
        }
        let source = read(&file);
        for (offset, opener) in unguarded_openers(&source) {
            failures.push(format!(
                "{file}:{}: `{opener}` opens a menu, but its element has no MenuButton",
                line_of(&source, offset)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "context-menu openers without the shared ⋮ trigger in the same element \
         (see this guard's header for the fix):\n{}",
        failures.join("\n")
    );
}

#[test]
fn sanitizer_hides_braces_in_text_and_keeps_offsets() {
    let source = "let a = \"{x\"; // }\nlet b = r#\"}\"#; /* { */ c { }";
    let code = sanitize(source);
    assert_eq!(code.chars().count(), source.chars().count());
    assert!(!code.contains("\"{x\""));
    assert_eq!(code.matches('{').count(), 1);
    assert_eq!(code.matches('}').count(), 1);
}

#[test]
fn an_opener_is_guarded_only_by_a_menu_button_in_its_own_element() {
    let guarded = "div { oncontextmenu: f, MenuButton { name: n, items: i } }";
    assert!(unguarded_openers(guarded).is_empty());

    // The opener sits on a child that has no trigger; the trigger in the
    // parent does not guard the child's element.
    let child = "div { span { oncontextmenu: f } MenuButton { name: n, items: i } }";
    assert_eq!(unguarded_openers(child).len(), 1);

    let definition = "pub fn open_at_pointer(self) { press_start(x) }";
    assert!(unguarded_openers(definition).is_empty());
}

#[test]
fn every_shortcut_that_opens_a_menu_is_guarded_too() {
    // The keyboard route: a menu key handled on an element with no trigger.
    let key = "div { onkeydown: move |evt| shortcut.key(&evt) }";
    assert_eq!(unguarded_openers(key).len(), 1);

    let pointer = "div { oncontextmenu: move |evt| shortcut.right_click(&evt) }";
    assert_eq!(unguarded_openers(pointer).len(), 2);

    let long_press = "div { onpointerdown: move |evt| shortcut.press_start(&evt) }";
    assert_eq!(unguarded_openers(long_press).len(), 1);

    let guarded_key =
        "div { onkeydown: move |evt| shortcut.key(&evt), MenuButton { name: n, items: i } }";
    assert!(unguarded_openers(guarded_key).is_empty());

    // A documented delegation to another element's trigger is the exception.
    let delegated = "div { // menu-guard: delegates to the focus person's trigger\n onkeydown: move |evt| { if opens_menu(&evt) { go() } } }";
    assert!(unguarded_openers(delegated).is_empty());
}
