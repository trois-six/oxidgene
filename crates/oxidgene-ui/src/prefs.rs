//! Client-side preferences, persisted in `localStorage`.
//!
//! These are per-viewer choices about how data is *shown* and *entered*, not
//! properties of the data itself — so they live in the browser rather than on
//! the tree. The pattern mirrors [`crate::i18n::use_init_language`].

use dioxus::prelude::*;
use oxidgene_core::enums::{Calendar, DateDisplayFormat, DateInputFormat};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::components::pedigree_theme::PedigreeThemeId;
use crate::components::pedigree_view::PedigreeView;
use crate::i18n::DateStyle;

pub const MAX_PEDIGREE_LEVELS: usize = 10;

/// Reads a value a page keeps in the browser under `key`, when it keeps one:
/// a page's own per-viewer choices, such as the tab it was last left on.
/// Storage that is unavailable or refused reads as nothing stored.
pub async fn stored(key: &str) -> Option<String> {
    document::eval(&format!(
        "try {{ return localStorage.getItem('{key}'); }} catch (e) {{ return null; }}"
    ))
    .await
    .ok()
    .and_then(|v| v.as_str().map(str::to_string))
}

/// Keeps `value` under `key` for [`stored`]; a refusal is ignored, the
/// choice then lasting only for the visit. Both are plain identifiers the
/// page itself chose, never user text.
pub fn store(key: &str, value: &str) {
    document::eval(&format!(
        "try {{ localStorage.setItem('{key}', '{value}'); }} catch (e) {{}}"
    ));
}

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
const PEDIGREE_THEME_STORAGE_KEY: &str = "oxidgene-pedigree-theme";
const PEDIGREE_VIEW_STORAGE_KEY: &str = "oxidgene-pedigree-view";
const DATE_STYLE_STORAGE_KEY: &str = "oxidgene-date-style";
const ENTRY_OPTIONS_STORAGE_KEY: &str = "oxidgene-entry-options";

/// Hook: initialise the pedigree theme (call once in `AppShell`).
///
/// Unlike the depth defaults, this resolves to a usable value immediately
/// rather than staying `None` until storage answers: the theme changes card
/// geometry and colors, so resolving it late would visibly redraw every chart.
pub fn use_init_pedigree_theme() -> Signal<PedigreeThemeId> {
    let mut pref = use_context_provider(|| Signal::new(PedigreeThemeId::default()));

    use_effect(move || {
        spawn(async move {
            let result = document::eval(&format!(
                "return localStorage.getItem('{PEDIGREE_THEME_STORAGE_KEY}');"
            ));
            if let Ok(value) = result.await
                && let Some(stored) = value.as_str()
                && let Ok(id) = serde_json::from_str::<PedigreeThemeId>(stored)
            {
                // A name we no longer ship leaves the default in place rather
                // than failing: themes may be renamed or withdrawn, and a
                // stale entry is not a reason to show the reader nothing.
                pref.set(id);
            }
        });
    });

    pref
}

/// Read the chosen pedigree theme, falling back outside a provider.
pub fn use_pedigree_theme() -> PedigreeThemeId {
    try_use_context::<Signal<PedigreeThemeId>>()
        .map(|pref| *pref.read())
        .unwrap_or_default()
}

/// Persist the pedigree theme and update every chart on screen.
pub fn set_pedigree_theme(mut pref: Signal<PedigreeThemeId>, id: PedigreeThemeId) {
    pref.set(id);
    if let Ok(stored) = serde_json::to_string(&id)
        && let Ok(js_value) = serde_json::to_string(&stored)
    {
        document::eval(&format!(
            "localStorage.setItem('{PEDIGREE_THEME_STORAGE_KEY}', {js_value});"
        ));
    }
}

/// Hook: initialise the tree view's drawing (call once in `AppShell`).
///
/// Resolves to the default at once, for the same reason as the theme: the
/// tree view would otherwise draw one chart and then replace it with another.
pub fn use_init_pedigree_view() -> Signal<PedigreeView> {
    let mut pref = use_context_provider(|| Signal::new(PedigreeView::default()));

    use_effect(move || {
        spawn(async move {
            // A view this build no longer ships keeps the default, the way an
            // unknown theme does.
            if let Some(view) = stored(PEDIGREE_VIEW_STORAGE_KEY)
                .await
                .as_deref()
                .and_then(PedigreeView::from_stored)
            {
                pref.set(view);
            }
        });
    });

    pref
}

/// Read the chosen tree view drawing, falling back outside a provider.
pub fn use_pedigree_view() -> PedigreeView {
    try_use_context::<Signal<PedigreeView>>()
        .map(|pref| *pref.read())
        .unwrap_or_default()
}

/// Persist the tree view drawing and redraw every tree view on screen.
pub fn set_pedigree_view(mut pref: Signal<PedigreeView>, view: PedigreeView) {
    pref.set(view);
    if let Ok(js_value) = serde_json::to_string(&view.to_stored()) {
        document::eval(&format!(
            "try {{ localStorage.setItem('{PEDIGREE_VIEW_STORAGE_KEY}', {js_value}); }} catch (e) {{}}"
        ));
    }
}

/// Hook: initialise the surname-sorting preference (call once in `AppShell`).
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

/// How the forms help with entry (`docs/ui-app-settings.md` §9): a
/// preference of the device, followed by the forms of every tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryOptions {
    /// Whether entry fields suggest values as the user types.
    pub suggestions: bool,
    /// Whether surname fields write what is typed in capitals.
    pub surname_uppercase: bool,
    /// Whether adding a relative offers the persons already in the tree.
    pub suggest_persons: bool,
    /// The order and form of a date field's parts.
    pub date_format: DateInputFormat,
    /// The calendar an empty date field starts in.
    pub calendar: Calendar,
}

impl EntryOptions {
    /// Every help on, dates entered `dd/mm/yyyy` in the Gregorian calendar.
    pub const DEFAULT: Self = Self {
        suggestions: true,
        surname_uppercase: true,
        suggest_persons: true,
        date_format: DateInputFormat::Slashes,
        calendar: Calendar::Gregorian,
    };

    /// The options a stored entry names. Each one is read on its own: one
    /// missing, unknown or unreadable keeps its default, and so does every
    /// option of an entry that is not a JSON object at all.
    fn from_stored(stored: &str) -> Self {
        let value = serde_json::from_str::<Value>(stored).unwrap_or(Value::Null);
        let default = Self::DEFAULT;
        Self {
            suggestions: field(&value, "suggestions").unwrap_or(default.suggestions),
            surname_uppercase: field(&value, "surname_uppercase")
                .unwrap_or(default.surname_uppercase),
            suggest_persons: field(&value, "suggest_persons").unwrap_or(default.suggest_persons),
            date_format: field(&value, "date_format").unwrap_or(default.date_format),
            calendar: field(&value, "calendar").unwrap_or(default.calendar),
        }
    }

    fn to_stored(self) -> String {
        json!({
            "suggestions": self.suggestions,
            "surname_uppercase": self.surname_uppercase,
            "suggest_persons": self.suggest_persons,
            "date_format": self.date_format,
            "calendar": self.calendar,
        })
        .to_string()
    }
}

impl Default for EntryOptions {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The date style a stored entry names, read one setting at a time, as
/// [`EntryOptions::from_stored`] reads its options.
fn date_style_from_stored(stored: &str) -> DateStyle {
    let value = serde_json::from_str::<Value>(stored).unwrap_or(Value::Null);
    let default = DateStyle::DEFAULT;
    DateStyle {
        format: field::<DateDisplayFormat>(&value, "format").unwrap_or(default.format),
        symbols: field(&value, "symbols").unwrap_or(default.symbols),
        circa: field(&value, "circa").unwrap_or(default.circa),
        calendar: field(&value, "calendar").unwrap_or(default.calendar),
    }
}

fn date_style_to_stored(style: DateStyle) -> String {
    json!({
        "format": style.format,
        "symbols": style.symbols,
        "circa": style.circa,
        "calendar": style.calendar,
    })
    .to_string()
}

/// The member `key` of a stored object, when it reads as a `T`.
fn field<T: DeserializeOwned>(value: &Value, key: &str) -> Option<T> {
    value.get(key).and_then(|v| T::deserialize(v).ok())
}

/// Keeps the JSON `stored` under `key`; a refusal is ignored, the choice then
/// lasting only for the visit.
fn store_json(key: &str, stored: &str) {
    if let Ok(js_value) = serde_json::to_string(stored) {
        document::eval(&format!(
            "try {{ localStorage.setItem('{key}', {js_value}); }} catch (e) {{}}"
        ));
    }
}

/// Hook: initialise the date style (call once in `AppShell`), which
/// [`use_i18n`](crate::i18n::use_i18n) hands to every page.
///
/// Resolves to the default at once, like the pedigree theme, then to the
/// stored style once storage answers.
pub fn use_init_date_style() -> Signal<DateStyle> {
    let mut pref = use_context_provider(|| Signal::new(DateStyle::DEFAULT));

    use_effect(move || {
        spawn(async move {
            if let Some(stored) = stored(DATE_STYLE_STORAGE_KEY).await {
                let style = date_style_from_stored(&stored);
                if *pref.peek() != style {
                    pref.set(style);
                }
            }
        });
    });

    pref
}

/// Persist the date style and rewrite every date on screen.
pub fn set_date_style(mut pref: Signal<DateStyle>, style: DateStyle) {
    pref.set(style);
    store_json(DATE_STYLE_STORAGE_KEY, &date_style_to_stored(style));
}

/// Hook: initialise the entry options (call once in `AppShell`).
pub fn use_init_entry_options() -> Signal<EntryOptions> {
    let mut pref = use_context_provider(|| Signal::new(EntryOptions::DEFAULT));

    use_effect(move || {
        spawn(async move {
            if let Some(stored) = stored(ENTRY_OPTIONS_STORAGE_KEY).await {
                let options = EntryOptions::from_stored(&stored);
                if *pref.peek() != options {
                    pref.set(options);
                }
            }
        });
    });

    pref
}

/// Hook: the entry options. A signal rather than a value, so that a resource
/// reading it in its closure runs again when they change.
pub fn use_entry_options() -> Signal<EntryOptions> {
    use_context()
}

/// Persist the entry options; every form follows them at once.
pub fn set_entry_options(mut pref: Signal<EntryOptions>, options: EntryOptions) {
    pref.set(options);
    store_json(ENTRY_OPTIONS_STORAGE_KEY, &options.to_stored());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_options_survive_a_round_trip() {
        let options = EntryOptions {
            suggestions: false,
            surname_uppercase: false,
            suggest_persons: true,
            date_format: DateInputFormat::MonthName,
            calendar: Calendar::FrenchRepublican,
        };
        assert_eq!(EntryOptions::from_stored(&options.to_stored()), options);
    }

    #[test]
    fn an_unreadable_entry_option_keeps_its_default_alone() {
        let options = EntryOptions::from_stored(
            r#"{"suggestions":false,"date_format":"dots","calendar":7,"extra":1}"#,
        );
        assert_eq!(
            options,
            EntryOptions {
                suggestions: false,
                ..EntryOptions::DEFAULT
            }
        );
        assert_eq!(EntryOptions::from_stored("not json"), EntryOptions::DEFAULT);
        assert_eq!(EntryOptions::from_stored("[]"), EntryOptions::DEFAULT);
    }

    #[test]
    fn the_date_style_survives_a_round_trip() {
        let style = DateStyle {
            format: DateDisplayFormat::Numeric,
            symbols: true,
            circa: true,
            calendar: Calendar::Julian,
        };
        assert_eq!(date_style_from_stored(&date_style_to_stored(style)), style);
    }

    #[test]
    fn an_unreadable_date_setting_keeps_its_default_alone() {
        assert_eq!(
            date_style_from_stored(r#"{"format":"roman","symbols":true,"calendar":"mayan"}"#),
            DateStyle {
                symbols: true,
                ..DateStyle::DEFAULT
            }
        );
        assert_eq!(date_style_from_stored(""), DateStyle::DEFAULT);
    }

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
