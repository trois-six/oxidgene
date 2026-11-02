//! Conditional hooks: no hook of the interface is called on the right of
//! `&&` or `||`.
//!
//! Drift it prevents: a Dioxus component must call the same hooks in the
//! same order on every render. `flag && use_can(..)` skips the hook whenever
//! `flag` is false — and `use_can` is two context lookups, each a hook — so
//! the hooks after it shift place the render the flag changes, a mismatch
//! Dioxus reports as a panic or answers with another hook's state. The
//! person form shipped such a mismatch once; the media gallery's tiles held
//! two more, harmless only while their props never changed.
//!
//! A hook is a function whose name starts with `use_`, called directly or
//! through a path (`crate::sharing::use_sharing_surfaces()`).
//!
//! Fixing a failure: call the hook first, on every render, into a `let`,
//! then combine: `let may_edit = use_can(Permission::Edit); let has_menu =
//! person_id.is_some() && may_edit;`.

use oxidgene_guards::{files, line_of, relative, without_test_modules};

/// The hook calls on the right of a short-circuit operator in `source`, as
/// their byte offsets.
fn conditional_hooks(source: &str) -> Vec<usize> {
    let mut found = Vec::new();
    for operator in ["&&", "||"] {
        for (at, _) in source.match_indices(operator) {
            let rest = &source[at + operator.len()..];
            let call = rest.trim_start();
            // The callee: a path of identifiers, up to its parenthesis.
            let callee_len = call
                .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':'))
                .unwrap_or(call.len());
            let callee = &call[..callee_len];
            let name = callee.rsplit("::").next().unwrap_or_default();
            if name.starts_with("use_") && call[callee_len..].starts_with('(') {
                found.push(at);
            }
        }
    }
    found.sort_unstable();
    found
}

#[test]
fn no_hook_is_called_conditionally() {
    let mut offenders = Vec::new();
    for path in files("crates/oxidgene-ui/src", "rs") {
        let source = std::fs::read_to_string(&path).unwrap();
        let code = without_test_modules(&source);
        for at in conditional_hooks(code) {
            offenders.push(format!("{}:{}", relative(&path), line_of(code, at)));
        }
    }
    assert!(
        offenders.is_empty(),
        "hooks called on the right of `&&` or `||`; call them first, into a `let`:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn the_scan_finds_a_hook_behind_a_short_circuit() {
    assert_eq!(conditional_hooks("let a = b && use_can(P::Edit);").len(), 1);
    assert_eq!(
        conditional_hooks("let a = b ||\n    crate::sharing::use_sharing_surfaces();").len(),
        1
    );
    assert!(conditional_hooks("let a = use_can(P::Edit) && b;").is_empty());
    assert!(conditional_hooks("let a = b && user_count(x);").is_empty());
    assert!(conditional_hooks("let a = b && use_this;").is_empty());
}
