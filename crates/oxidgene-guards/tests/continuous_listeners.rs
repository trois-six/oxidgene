//! Continuous listeners: no element listens to pointer moves or wheel turns
//! for as long as it is on screen.
//!
//! Drift it prevents: on the desktop, Dioxus answers every bubbling event
//! through one listener on the document and hands each event to Rust
//! synchronously, the WebView waiting for the answer. A single
//! `onmousemove`, `onpointermove` or `onwheel` mounted anywhere sends every
//! move or turn in the window across — scrolling included, since the engine
//! moves the pointer over the content it scrolls — and the page lags while
//! it is shown (docs/ui-common.md §6.5).
//!
//! The listeners are the `onX:` attributes of the markup, for the events
//! named below; listeners built with `dioxus::html::events::onX(…)` and
//! spread with `crate::listeners::while_active` exist during a gesture only,
//! and are not attributes.
//!
//! Fixing a failure: attach move listeners for the duration of the drag
//! with `while_active`, read the wheel with `crate::listeners::forward_wheel`,
//! or place a tooltip where the pointer entered rather than following it.

use oxidgene_guards::{files, line_of, read, relative};

/// Bubbling events fired continuously by a pointer, a wheel or a touch.
const CONTINUOUS: &[&str] = &[
    "onmousemove",
    "onpointermove",
    "onwheel",
    "onmouseover",
    "onmouseout",
    "onpointerover",
    "onpointerout",
    "ontouchmove",
];

#[test]
fn no_element_listens_to_moves_or_wheel_turns_while_idle() {
    let mut found = Vec::new();
    for path in files("crates/oxidgene-ui/src", "rs") {
        let file = relative(&path);
        let source = read(&file);
        for event in CONTINUOUS {
            let attribute = format!("{event}:");
            for (offset, _) in source.match_indices(&attribute) {
                let before = source[..offset].chars().next_back();
                if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    continue;
                }
                found.push(format!("{file}:{}: {event}", line_of(&source, offset)));
            }
        }
    }
    assert!(
        found.is_empty(),
        "listeners that stay attached while nothing needs them \
         (see this guard's header for the fix):\n{}",
        found.join("\n")
    );
}

/// Each set of gesture listeners is spread on a wrapper of its own (see
/// `crate::listeners::while_active`): spread on an element holding other
/// listeners, its removal at the end of the gesture leaves that element deaf
/// to presses on its descendants.
///
/// Fixing a failure: wrap the element in
/// `div { class: "gesture-listeners", ..listeners, element { … } }`.
#[test]
fn gesture_listeners_are_spread_on_a_wrapper_of_their_own() {
    let mut found = Vec::new();
    for path in files("crates/oxidgene-ui/src", "rs") {
        let file = relative(&path);
        let source = read(&file);
        for (offset, _) in source.match_indices("listeners::while_active(") {
            let Some(name) = source[..offset]
                .rsplit("let ")
                .next()
                .and_then(|binding| binding.split('=').next())
                .map(str::trim)
            else {
                continue;
            };
            let spread = format!("..{name},");
            let wrapped = source.match_indices(&spread).all(|(at, _)| {
                let before: Vec<&str> = source[..at]
                    .lines()
                    .rev()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .take(2)
                    .collect();
                before == ["class: \"gesture-listeners\",", "div {"]
            });
            if !wrapped || !source.contains(&spread) {
                found.push(format!("{file}:{}: {name}", line_of(&source, offset)));
            }
        }
    }
    assert!(
        found.is_empty(),
        "gesture listeners not spread on a `gesture-listeners` wrapper \
         (see this guard's test for the fix):\n{}",
        found.join("\n")
    );
}
