//! Hover tooltip for occupation-sheet / given-name reference content.
//!
//! Wraps a span of text (an occupation label, a given name) and — only when
//! a matching fiche exists in `/api/v1/reference` (backend module
//! `oxidgene-api::reference`) — shows it on hover. Resolves eagerly on
//! mount (not on hover) so a term with no fiche renders as plain, unstyled
//! text: no help cursor, no bubble, nothing. Terms are few per page and the
//! response is cached client-side, so this costs at most a couple of small,
//! cached GET requests per page visit.

use dioxus::prelude::*;

use crate::api::ApiClient;
use crate::i18n::use_i18n;

/// Which reference table to query for a given term.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceKind {
    Occupation,
    GivenName,
}

/// Delay before showing the bubble, so a quick mouse pass doesn't flash one.
const SHOW_DELAY_MS: u64 = 350;
/// Delay before hiding, so moving from the trigger onto the bubble itself
/// (e.g. to select/copy text) doesn't immediately close it.
const HIDE_DELAY_MS: u64 = 150;

/// One fetched fiche's display fields, already shaped for rendering
/// regardless of which reference table it came from.
struct FicheContent {
    label: String,
    meta: String,
    text: String,
}

/// Wraps `children` so hovering over them shows a reference tooltip for
/// `term` (the raw GEDCOM value — occupation label or given name) — but
/// only once a matching fiche has been resolved. While loading, or when the
/// backend has no fiche for this term (404), `children` render as plain
/// text with no hover affordance at all.
#[component]
pub fn ReferenceHover(kind: ReferenceKind, term: String, children: Element) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let lang_code = i18n.0.code().to_string();
    let term_for_fetch = term.clone();

    let content_resource = use_resource(move || {
        let api = api.clone();
        let lang_code = lang_code.clone();
        let term = term_for_fetch.clone();
        async move {
            match kind {
                ReferenceKind::Occupation => api
                    .reference_occupation(&lang_code, &term)
                    .await
                    .ok()
                    .flatten()
                    .map(|r| FicheContent {
                        label: r.label,
                        meta: r.summary,
                        text: r.text,
                    }),
                ReferenceKind::GivenName => api
                    .reference_given_name(&lang_code, &term)
                    .await
                    .ok()
                    .flatten()
                    .map(|r| FicheContent {
                        label: r.label,
                        meta: format!("{} — {}", r.origin, r.meaning),
                        text: r.text,
                    }),
            }
        }
    });

    let guard = content_resource.read();
    let Some(Some(fiche)) = &*guard else {
        drop(guard);
        return rsx! { {children} };
    };

    let mut visible = use_signal(|| false);
    let mut pos = use_signal(|| (0.0_f64, 0.0_f64));
    let mut hover_gen = use_signal(|| 0_u64);
    let (px, py) = pos();
    let style = format!("left: {}px; top: {}px;", px + 16.0, py + 16.0);

    rsx! {
        span {
            class: "ref-hover-target",
            onmouseenter: move |evt| {
                let c = evt.client_coordinates();
                pos.set((c.x, c.y));
                hover_gen += 1;
                let my_gen = hover_gen();
                spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(SHOW_DELAY_MS)).await;
                    if hover_gen() == my_gen {
                        visible.set(true);
                    }
                });
            },
            onmousemove: move |evt| {
                if visible() {
                    let c = evt.client_coordinates();
                    pos.set((c.x, c.y));
                }
            },
            onmouseleave: move |_| {
                hover_gen += 1;
                let leave_gen = hover_gen();
                spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(HIDE_DELAY_MS)).await;
                    if hover_gen() == leave_gen {
                        visible.set(false);
                    }
                });
            },
            {children}
        }
        if visible() {
            div { class: "ref-tooltip", style: "{style}",
                div { class: "ref-tooltip-label", "{fiche.label}" }
                div { class: "ref-tooltip-meta", "{fiche.meta}" }
                div { class: "ref-tooltip-text", "{fiche.text}" }
            }
        }
    }
}

/// Splits a given-names field into individual first-name tokens on spaces
/// and hyphens (e.g. "Louis Marie Emile Augustin" or "Jean-Baptiste"),
/// pairing each with the separator that followed it (empty for the last
/// token) so the original spelling can be reconstructed exactly.
fn split_given_name_tokens(given: &str) -> Vec<(String, String)> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for c in given.chars() {
        if c == ' ' || c == '-' {
            if !current.is_empty() {
                tokens.push((std::mem::take(&mut current), c.to_string()));
            }
        } else {
            current.push(c);
        }
    }
    if !current.is_empty() {
        tokens.push((current, String::new()));
    }
    tokens
}

/// Renders a (possibly multi-word / hyphen-compound) given-names field with
/// each individual first name wrapped in its own [`ReferenceHover`] — so
/// hovering "Marie" in "Louis Marie Emile Augustin" shows Marie's meaning,
/// not whichever name happens to be first. Original spacing/hyphenation is
/// preserved between tokens.
#[component]
pub fn GivenNamesHover(given_names: String) -> Element {
    let tokens = split_given_name_tokens(&given_names);
    rsx! {
        for (i, (word, sep)) in tokens.into_iter().enumerate() {
            ReferenceHover {
                key: "given-{i}-{word}",
                kind: ReferenceKind::GivenName,
                term: word.clone(),
                "{word}"
            }
            if !sep.is_empty() {
                "{sep}"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_space_separated_tokens() {
        assert_eq!(
            split_given_name_tokens("alpha beta gamma"),
            vec![
                ("alpha".to_string(), " ".to_string()),
                ("beta".to_string(), " ".to_string()),
                ("gamma".to_string(), String::new()),
            ]
        );
    }

    #[test]
    fn splits_hyphen_compound_tokens() {
        assert_eq!(
            split_given_name_tokens("alpha-beta"),
            vec![
                ("alpha".to_string(), "-".to_string()),
                ("beta".to_string(), String::new()),
            ]
        );
    }

    #[test]
    fn splits_mixed_space_and_hyphen_tokens() {
        assert_eq!(
            split_given_name_tokens("alpha beta-gamma delta"),
            vec![
                ("alpha".to_string(), " ".to_string()),
                ("beta".to_string(), "-".to_string()),
                ("gamma".to_string(), " ".to_string()),
                ("delta".to_string(), String::new()),
            ]
        );
    }

    #[test]
    fn single_token_has_no_trailing_separator() {
        assert_eq!(
            split_given_name_tokens("alpha"),
            vec![("alpha".to_string(), String::new())]
        );
    }
}
