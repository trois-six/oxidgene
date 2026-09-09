//! Hover tooltip for occupation-sheet / given-name reference content.
//!
//! Wraps a span of text (an occupation label, a given name) and — only when
//! a matching fiche exists in `/api/v1/reference` (backend module
//! `oxidgene-api::reference`) — shows it on hover. Resolves eagerly on
//! mount (not on hover) so a term with no fiche renders as plain, unstyled
//! text: no help cursor, no bubble, nothing.
//!
//! Both fields resolve every term they display through one bounded batch
//! operation. A component per term would put one request per term on the
//! wire at mount, which is what made a profile with several occupations
//! open slowly.

use std::collections::HashMap;

use dioxus::prelude::*;

use crate::api::{ApiClient, GivenNameReference, OccupationReference};
use crate::i18n::use_i18n;
use crate::ui_observability::use_ui_resource;

/// Delay before showing the bubble, so a quick mouse pass doesn't flash one.
const SHOW_DELAY_MS: u64 = 350;
/// Delay before hiding, so moving from the trigger onto the bubble itself
/// (e.g. to select/copy text) doesn't immediately close it.
const HIDE_DELAY_MS: u64 = 150;

/// One fetched fiche's display fields, already shaped for rendering
/// regardless of which reference table it came from.
#[derive(Clone, PartialEq)]
struct FicheContent {
    label: String,
    meta: String,
    text: String,
}

impl From<GivenNameReference> for FicheContent {
    fn from(reference: GivenNameReference) -> Self {
        Self {
            label: reference.label,
            meta: format!("{} — {}", reference.origin, reference.meaning),
            text: reference.text,
        }
    }
}

impl From<OccupationReference> for FicheContent {
    fn from(reference: OccupationReference) -> Self {
        Self {
            label: reference.label,
            meta: reference.summary,
            text: reference.text,
        }
    }
}

/// Renders a person's occupation labels as a comma-separated list, resolving
/// all of them in one batch. Each label with a fiche gets its own hover
/// target; the others stay plain text.
#[component]
pub fn OccupationsHover(titles: Vec<String>) -> Element {
    let api = use_context::<ApiClient>();
    let lang_code = use_i18n().0.code().to_string();
    let terms_for_fetch = titles.clone();
    let references = use_ui_resource("occupation_reference_bundle", move || {
        let api = api.clone();
        let lang_code = lang_code.clone();
        let terms = terms_for_fetch.clone();
        async move {
            api.reference_occupations(&lang_code, &terms)
                .await
                .unwrap_or_default()
        }
    });
    let fiches = references
        .read()
        .as_ref()
        .map(|matches| {
            matches
                .iter()
                .cloned()
                .map(|result| (result.term, result.reference.into()))
                .collect::<HashMap<String, FicheContent>>()
        })
        .unwrap_or_default();
    rsx! {
        for (i , title) in titles.into_iter().enumerate() {
            span { key: "occ-{title}",
                if i > 0 {
                    ", "
                }
                if let Some(fiche) = fiches.get(&title) {
                    FicheHover { fiche: fiche.clone(), "{title}" }
                } else {
                    "{title}"
                }
            }
        }
    }
}

#[component]
fn FicheHover(fiche: FicheContent, children: Element) -> Element {
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
                    crate::utils::sleep_ms(SHOW_DELAY_MS as u32).await;
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
                    crate::utils::sleep_ms(HIDE_DELAY_MS as u32).await;
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

/// Renders a (possibly multi-word / hyphen-compound) given-names field after
/// resolving all individual names in one batch. Each resolved token gets its
/// own hover target, and original spacing/hyphenation is preserved.
#[component]
pub fn GivenNamesHover(given_names: String) -> Element {
    let tokens = split_given_name_tokens(&given_names);
    let terms_for_fetch = tokens
        .iter()
        .map(|(word, _)| word.clone())
        .collect::<Vec<_>>();
    let api = use_context::<ApiClient>();
    let lang_code = use_i18n().0.code().to_string();
    let references = use_ui_resource("given_name_reference_bundle", move || {
        let api = api.clone();
        let lang_code = lang_code.clone();
        let terms = terms_for_fetch.clone();
        async move {
            api.reference_given_names(&lang_code, &terms)
                .await
                .unwrap_or_default()
        }
    });
    let fiches = references
        .read()
        .as_ref()
        .map(|matches| {
            matches
                .iter()
                .cloned()
                .map(|result| (result.term, result.reference.into()))
                .collect::<HashMap<String, FicheContent>>()
        })
        .unwrap_or_default();
    rsx! {
        for (i, (word, sep)) in tokens.into_iter().enumerate() {
            if let Some(fiche) = fiches.get(&word) {
                FicheHover {
                    key: "given-{i}-{word}",
                    fiche: fiche.clone(),
                    "{word}"
                }
            } else {
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
