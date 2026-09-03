//! Client-side display preferences, persisted in `localStorage`.
//!
//! These are per-viewer choices about how data is *shown*, not properties of
//! the data itself — so they live in the browser rather than on the tree. The
//! pattern mirrors [`crate::i18n::use_init_language`].

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

pub const MAX_PEDIGREE_LEVELS: usize = 10;

/// Default pedigree window for trees that have no saved view state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PedigreeDefaults {
    pub ancestor_levels: usize,
    pub descendant_levels: usize,
}

impl Default for PedigreeDefaults {
    fn default() -> Self {
        Self {
            ancestor_levels: 4,
            descendant_levels: 3,
        }
    }
}

impl PedigreeDefaults {
    #[must_use]
    pub fn normalized(self) -> Self {
        Self {
            ancestor_levels: self.ancestor_levels.min(MAX_PEDIGREE_LEVELS),
            descendant_levels: self.descendant_levels.min(MAX_PEDIGREE_LEVELS),
        }
    }
}

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

const SORT_PARTICLES_STORAGE_KEY: &str = "oxidgene-sort-particles";
const PEDIGREE_DEFAULTS_STORAGE_KEY: &str = "oxidgene-pedigree-defaults";

/// Hook: initialise the surname-sorting preference (call once in `Layout`).
pub fn use_init_sort_particles() -> Signal<SortParticles> {
    let mut pref = use_context_provider(|| Signal::new(SortParticles::default()));

    use_effect(move || {
        spawn(async move {
            let result = document::eval(&format!(
                "return localStorage.getItem('{SORT_PARTICLES_STORAGE_KEY}');"
            ));
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
        "localStorage.setItem('{SORT_PARTICLES_STORAGE_KEY}', '{include}');"
    ));
}

/// Initialise pedigree defaults, leaving `None` until localStorage resolves.
pub fn use_init_pedigree_defaults() -> Signal<Option<PedigreeDefaults>> {
    let mut pref = use_context_provider(|| Signal::new(None));

    use_effect(move || {
        spawn(async move {
            let result = document::eval(&format!(
                "return localStorage.getItem('{PEDIGREE_DEFAULTS_STORAGE_KEY}');"
            ));
            let value = result
                .await
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .and_then(|stored| serde_json::from_str::<PedigreeDefaults>(&stored).ok())
                .unwrap_or_default()
                .normalized();
            pref.set(Some(value));
        });
    });

    pref
}

/// Read the loaded pedigree defaults, falling back outside a provider.
pub fn use_pedigree_defaults() -> Option<PedigreeDefaults> {
    try_use_context::<Signal<Option<PedigreeDefaults>>>()
        .map(|pref| *pref.read())
        .unwrap_or_else(|| Some(PedigreeDefaults::default()))
}

/// Persist pedigree defaults and update every mounted settings view.
pub fn set_pedigree_defaults(mut pref: Signal<Option<PedigreeDefaults>>, value: PedigreeDefaults) {
    let value = value.normalized();
    pref.set(Some(value));
    if let Ok(stored) = serde_json::to_string(&value)
        && let Ok(js_value) = serde_json::to_string(&stored)
    {
        document::eval(&format!(
            "localStorage.setItem('{PEDIGREE_DEFAULTS_STORAGE_KEY}', {js_value});"
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pedigree_defaults_are_bounded_to_the_depth_selector() {
        assert_eq!(
            PedigreeDefaults {
                ancestor_levels: 99,
                descendant_levels: 11,
            }
            .normalized(),
            PedigreeDefaults {
                ancestor_levels: MAX_PEDIGREE_LEVELS,
                descendant_levels: MAX_PEDIGREE_LEVELS,
            }
        );
    }
}
