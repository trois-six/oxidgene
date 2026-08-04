//! Client-side display preferences, persisted in `localStorage`.
//!
//! These are per-viewer choices about how data is *shown*, not properties of
//! the data itself — so they live in the browser rather than on the tree. The
//! pattern mirrors [`crate::i18n::use_init_language`].

use dioxus::prelude::*;

/// Whether surname particles count when filing surnames alphabetically.
///
/// `true` (the default) files "de la Cruz" under D, as written; `false` files
/// it under C, on its root. Conventions differ by country and by researcher,
/// so this is a preference rather than a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortParticles(pub bool);

impl Default for SortParticles {
    fn default() -> Self {
        Self(true)
    }
}

const STORAGE_KEY: &str = "oxidgene-sort-particles";

/// Hook: initialise the surname-sorting preference (call once in `Layout`).
pub fn use_init_sort_particles() -> Signal<SortParticles> {
    let mut pref = use_context_provider(|| Signal::new(SortParticles::default()));

    use_effect(move || {
        spawn(async move {
            let result = document::eval(&format!("return localStorage.getItem('{STORAGE_KEY}');"));
            if let Ok(val) = result.await
                && let Some(stored) = val.as_str()
            {
                // Anything other than an explicit "false" keeps the default,
                // so a missing or corrupted entry degrades gracefully.
                pref.set(SortParticles(stored != "false"));
            }
        });
    });

    pref
}

/// Read the preference from context, falling back to the default outside a
/// provider (which is what tests and isolated component previews get).
pub fn use_sort_particles() -> SortParticles {
    try_use_context::<Signal<SortParticles>>()
        .map(|s| *s.read())
        .unwrap_or_default()
}

/// Persist the preference and update the signal.
pub fn set_sort_particles(mut pref: Signal<SortParticles>, include: bool) {
    pref.set(SortParticles(include));
    document::eval(&format!(
        "localStorage.setItem('{STORAGE_KEY}', '{include}');"
    ));
}
