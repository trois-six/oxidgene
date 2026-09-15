//! Global colour themes — the palette every page draws from.
//!
//! A theme is a set of colours and nothing else. Fonts, spacing, radii and
//! component geometry stay in [`crate::components::layout::LAYOUT_STYLES`],
//! which is the same for every theme: switching theme must repaint the
//! application, never relayout it.
//!
//! Themes are data, not code. Each one is a JSON document; the built-in
//! themes are embedded at compile time and users may drop further ones into a
//! folder the desktop application reads (see [`CustomThemeSource`]). The
//! selected theme is turned into a `:root { … }` block at runtime by
//! [`Theme::css`] and injected ahead of the stylesheet, so every
//! `var(--token)` already in the CSS resolves against it.
//!
//! # Why the values are validated
//!
//! A theme file is user input that ends up inside a `<style>` element. An
//! unchecked value could close the declaration and append arbitrary rules, so
//! [`Color`] accepts hexadecimal notation and nothing else, and theme
//! identifiers are restricted to a slug. Anything else is rejected with the
//! file name attached rather than silently dropped.
//!
//! # Deriving a colour rather than adding a token
//!
//! Tints of a token — a hover wash, a focus halo, a shadow — belong in the
//! stylesheet as `color-mix(in srgb, var(--token) N%, transparent)`, not in
//! the theme. A theme that had to enumerate every alpha of every accent would
//! be unwritable by hand, and a custom theme that set only `--orange` would
//! still show the stock orange in a dozen places.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, LazyLock};

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

/// A colour, in hexadecimal CSS notation.
///
/// Stored normalised to lowercase and kept as a string rather than unpacked
/// into channels: the value is written straight into CSS, and round-tripping
/// it through an integer would only risk changing it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Color(String);

impl Color {
    /// Parse `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`.
    ///
    /// # Errors
    ///
    /// Returns [`ThemeError::Color`] for any other shape, including CSS
    /// functions and named colours: see the module note on validation.
    pub fn parse(raw: &str) -> Result<Self, ThemeError> {
        let value = raw.trim();
        let digits = value
            .strip_prefix('#')
            .filter(|digits| matches!(digits.len(), 3 | 4 | 6 | 8))
            .filter(|digits| digits.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(|| ThemeError::Color(raw.to_owned()))?;
        Ok(Self(format!("#{}", digits.to_ascii_lowercase())))
    }

    /// The CSS literal, `#` included.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The six opaque digits, with any alpha pair dropped and shorthand
    /// expanded.
    ///
    /// Used where a colour has to be spelled out inside a value that cannot
    /// read a custom property, such as the data URI of the select arrow.
    #[must_use]
    pub fn rgb_digits(&self) -> String {
        let digits = &self.0[1..];
        match digits.len() {
            3 | 4 => digits[..3].chars().flat_map(|c| [c, c]).collect(),
            _ => digits[..6].to_owned(),
        }
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

/// Declares the palette once: the struct, the CSS variable names, the
/// by-name setter used to apply a partial override, and the emitter.
///
/// Keeping them in one place is the point — a token added to the struct but
/// forgotten in the emitter would be a colour no stylesheet could reach, and
/// the compiler cannot catch that on its own.
macro_rules! theme_tokens {
    ($($(#[$meta:meta])* $field:ident => $css:literal,)+) => {
        /// Every colour a theme defines.
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct ThemeColors {
            $($(#[$meta])* pub $field: Color,)+
        }

        impl ThemeColors {
            /// Every token name, without the `--` prefix, in emission order.
            pub const TOKENS: &'static [&'static str] = &[$($css,)+];

            /// Build a complete palette from a token map, as a theme with no
            /// `base` must supply.
            fn from_tokens(tokens: &BTreeMap<String, Color>) -> Result<Self, ThemeError> {
                Ok(Self {
                    $($field: tokens
                        .get($css)
                        .cloned()
                        .ok_or(ThemeError::MissingToken($css))?,)+
                })
            }

            /// Apply one override, reporting whether the token exists.
            fn set(&mut self, token: &str, value: Color) -> bool {
                match token {
                    $($css => {
                        self.$field = value;
                        true
                    })+
                    _ => false,
                }
            }

            fn write_declarations(&self, out: &mut String) {
                $(
                    out.push_str("    --");
                    out.push_str($css);
                    out.push_str(": ");
                    out.push_str(self.$field.as_str());
                    out.push_str(";\n");
                )+
            }
        }
    };
}

theme_tokens! {
    // ── Surfaces ────────────────────────────────────────────────────────
    /// Page background.
    bg_deep => "bg-deep",
    /// Panels, topbars and sidebars.
    bg_panel => "bg-panel",
    /// Cards, inputs and modals.
    bg_card => "bg-card",
    /// Hovered cards and rows.
    bg_card_hover => "bg-card-hover",
    /// Opaque colour behind the translucent navigation bar.
    nav_surface => "nav-surface",
    /// Selected rows and active list entries.
    sel_bg => "sel-bg",
    /// Borders and dividers.
    border => "border",
    /// Border of a focused control.
    border_glow => "border-glow",

    // ── Text ────────────────────────────────────────────────────────────
    /// Body and heading text.
    text_primary => "text-primary",
    /// Labels, metadata and breadcrumbs.
    text_secondary => "text-secondary",
    /// Placeholders and disabled text.
    text_muted => "text-muted",
    /// Text and icons drawn on an accent fill.
    on_accent => "on-accent",

    // ── Accents ─────────────────────────────────────────────────────────
    /// Primary actions, focus and the brand.
    orange => "orange",
    /// Primary hover, and the far end of the primary gradient.
    orange_light => "orange-light",
    /// Birth events and success.
    green => "green",
    /// Text on a green wash.
    green_light => "green-light",
    /// Green fills and washes.
    green_accent => "green-accent",
    /// Death events and information.
    blue => "blue",
    /// Female indicator.
    pink => "pink",
    /// Destructive hover and emphasis.
    red => "red",
    /// Destructive fills.
    danger => "danger",
    /// Destructive text.
    danger_text => "danger-text",

    // ── Structure ───────────────────────────────────────────────────────
    /// Pedigree connectors.
    connector => "connector",
    /// Background of the miniature tree on home cards.
    tree_visual_bg => "tree-visual-bg",
    /// Branches of the miniature tree on home cards.
    tree_visual_branch => "tree-visual-branch",

    // ── Depth ───────────────────────────────────────────────────────────
    /// Base shadow colour, tinted per use.
    shadow => "shadow",
    /// Resting elevation, alpha included.
    shadow_weak => "shadow-weak",
    /// Raised elevation, alpha included.
    shadow_strong => "shadow-strong",
    /// Backdrop behind modals and over media.
    scrim => "scrim",
    /// Warm light leak on the page background; fully transparent to disable.
    page_glow_warm => "page-glow-warm",
    /// Cool light leak on the page background; fully transparent to disable.
    page_glow_cool => "page-glow-cool",

    // ── Media viewer ────────────────────────────────────────────────────
    /// Backdrop of the full-screen media viewer.
    media_bg => "media-bg",
    /// Toolbars and filmstrip of the media viewer.
    media_panel => "media-panel",
    /// Vignette frames and control glyphs drawn over media.
    media_frame => "media-frame",
    /// Captions and labels drawn over media.
    media_caption => "media-caption",
    /// Wash inside a vignette frame.
    media_tint => "media-tint",

    // ── Pedigree person cards ───────────────────────────────────────────
    /// Card background.
    pn_bg => "pn-bg",
    /// Background of the card the chart is rooted on.
    pn_root_bg => "pn-root-bg",
    /// Background of a spouse card.
    pn_spouse_bg => "pn-spouse-bg",
    /// Card outline.
    pn_border => "pn-border",
    /// Male gender rule.
    pn_male_line => "pn-male-line",
    /// Female gender rule.
    pn_female_line => "pn-female-line",
    /// Sosa badge.
    pn_sosa => "pn-sosa",
    /// Sosa badge on the root card.
    pn_sosa_root => "pn-sosa-root",
    /// Marker on the selected person.
    pn_self => "pn-self",
    /// Card text.
    pn_text => "pn-text",
    /// Card dates and secondary card text.
    pn_text_muted => "pn-text-muted",
    /// Hovered card background.
    pn_hover_bg => "pn-hover-bg",
}

/// A complete, usable theme.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    /// Stable identifier; what is persisted as the user's choice.
    pub id: String,
    /// Name shown in the picker. Built-in themes are translated instead, by
    /// [`Theme::display_name`].
    pub name: String,
    /// Whether the theme ships with the application.
    pub builtin: bool,
    /// The palette.
    pub colors: ThemeColors,
}

impl Theme {
    /// The `:root` block that makes this theme the active palette.
    #[must_use]
    pub fn css(&self) -> String {
        let mut out = String::with_capacity(ThemeColors::TOKENS.len() * 32 + 512);
        out.push_str(":root {\n");
        self.colors.write_declarations(&mut out);

        // A data URI cannot read a custom property, so the arrow of every
        // native select is spelled out here from the theme's own secondary
        // text colour. Doing it in the generated block is what lets a custom
        // theme change it at all: as a literal in the stylesheet it could
        // only ever have matched the two built-in palettes.
        out.push_str("    --select-arrow: url(\"data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 10 6'><path d='M1 1l4 4 4-4' fill='none' stroke='%23");
        out.push_str(&self.colors.text_secondary.rgb_digits());
        out.push_str(
            "' stroke-width='1.6' stroke-linecap='round' stroke-linejoin='round'/></svg>\");\n",
        );

        out.push_str("}\n");
        out
    }

    /// The name to show in the picker.
    ///
    /// A theme whose name is an ordinary word — "Light", "Dark" — is part of
    /// the interface and is translated. Everything else falls back to the
    /// `name` in the file: a theme the user wrote, and equally a built-in one
    /// named after a place or a palette, which has no translation and should
    /// read as its author spelled it.
    #[must_use]
    pub fn display_name(&self, i18n: &crate::i18n::I18n) -> String {
        if self.builtin
            && let Some(translated) = i18n.try_t(&format!("app_settings.theme_{}", self.id))
        {
            return translated;
        }
        self.name.clone()
    }
}

/// The JSON shape of a theme file.
///
/// `base` exists so a theme can state only what it changes. The dark theme is
/// written that way, and so is any user theme that only wants a different
/// accent: listing all of the tokens to alter one of them would make the
/// format tedious to write and would silently freeze the theme the next time
/// a token is added.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeFile {
    id: String,
    name: String,
    #[serde(default)]
    base: Option<String>,
    colors: BTreeMap<String, Color>,
}

/// Why a theme could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeError {
    /// The value is not hexadecimal colour notation.
    Color(String),
    /// A theme with no `base` left a token undefined.
    MissingToken(&'static str),
    /// The file names a token that does not exist, most often a typo.
    UnknownToken(String),
    /// The `base` names a theme that is not built in.
    UnknownBase(String),
    /// The identifier is not a lowercase slug.
    InvalidId(String),
    /// The identifier would shadow a built-in theme.
    ReservedId(String),
    /// The document is not valid JSON, or not this shape.
    Syntax(String),
}

impl fmt::Display for ThemeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Color(value) => write!(f, "`{value}` is not a hex colour such as #1e1a14"),
            Self::MissingToken(token) => {
                write!(f, "no value for `{token}`, and no `base` to take it from")
            }
            Self::UnknownToken(token) => write!(f, "`{token}` is not a theme colour"),
            Self::UnknownBase(base) => write!(f, "`{base}` is not a built-in theme"),
            Self::InvalidId(id) => {
                write!(f, "`{id}` is not a valid id: use lowercase, digits and -")
            }
            Self::ReservedId(id) => write!(f, "`{id}` is the id of a built-in theme"),
            Self::Syntax(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ThemeError {}

/// Parse one theme document.
///
/// `bases` are the themes a `base` may name, and the ids this one may not
/// reuse. `builtin` decides whether the name is translated.
fn parse_theme(source: &str, builtin: bool, bases: &[Theme]) -> Result<Theme, ThemeError> {
    let file: ThemeFile =
        serde_json::from_str(source).map_err(|error| ThemeError::Syntax(error.to_string()))?;

    if file.id.is_empty()
        || !file
            .id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(ThemeError::InvalidId(file.id));
    }

    if bases.iter().any(|theme| theme.id == file.id) {
        return Err(ThemeError::ReservedId(file.id));
    }

    let mut colors = match &file.base {
        Some(base) => bases
            .iter()
            .find(|theme| theme.id == *base)
            .ok_or_else(|| ThemeError::UnknownBase(base.clone()))?
            .colors
            .clone(),
        None => ThemeColors::from_tokens(&file.colors)?,
    };

    for (token, value) in &file.colors {
        if !colors.set(token, value.clone()) {
            return Err(ThemeError::UnknownToken(token.clone()));
        }
    }

    Ok(Theme {
        id: file.id,
        name: file.name,
        builtin,
        colors,
    })
}

/// Parse a theme written by the user.
///
/// # Errors
///
/// Returns the first problem found, which the settings page reports next to
/// the file it came from.
pub fn parse_custom_theme(source: &str) -> Result<Theme, ThemeError> {
    parse_theme(source, false, &BUILTIN_THEMES)
}

/// The shipped themes, in picker order.
///
/// A theme may only inherit from one listed before it — `dark` from `light`,
/// and the three palettes from `dark` — so the order here is the resolution
/// order as well as the order they appear in.
const BUILTIN_SOURCES: &[(&str, &str)] = &[
    ("light", include_str!("../assets/themes/light.json")),
    ("dark", include_str!("../assets/themes/dark.json")),
    ("geneanet", include_str!("../assets/themes/geneanet.json")),
    ("solarized", include_str!("../assets/themes/solarized.json")),
    ("nord", include_str!("../assets/themes/nord.json")),
    ("omarchy", include_str!("../assets/themes/omarchy.json")),
    ("ayu", include_str!("../assets/themes/ayu.json")),
    (
        "catppuccin",
        include_str!("../assets/themes/catppuccin.json"),
    ),
];

/// The id used when nothing is stored, and when a stored id no longer exists.
pub const DEFAULT_THEME_ID: &str = "light";

/// The shipped themes.
///
/// These are compiled into the binary, so a failure here is a build mistake
/// rather than anything the user can cause or repair at runtime; the
/// `builtin_themes_are_valid` test is what keeps it from ever shipping.
pub static BUILTIN_THEMES: LazyLock<Vec<Theme>> = LazyLock::new(|| {
    let mut themes: Vec<Theme> = Vec::with_capacity(BUILTIN_SOURCES.len());
    for (id, source) in BUILTIN_SOURCES {
        // Parsed one at a time against the themes already in the list, so
        // that a `base` can name an earlier entry. Nothing in here reads the
        // static itself, which is what keeps the initialisation acyclic.
        let theme = parse_theme(source, true, &themes)
            .unwrap_or_else(|error| panic!("built-in theme `{id}` is invalid: {error}"));
        themes.push(theme);
    }
    themes
});

/// Look a shipped theme up by id.
#[must_use]
pub fn builtin_theme(id: &str) -> Option<&'static Theme> {
    BUILTIN_THEMES.iter().find(|theme| theme.id == id)
}

// ── Custom themes ───────────────────────────────────────────────────────────

/// What the application read from the user's theme folder.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CustomThemes {
    /// The themes that loaded.
    pub themes: Vec<Theme>,
    /// The files that did not, one message each.
    pub errors: Vec<CustomThemeError>,
}

/// A theme file that could not be used, named so the user can go fix it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomThemeError {
    /// File name, without the directory.
    pub file: String,
    /// What was wrong with it, already formatted.
    pub message: String,
}

/// Reads themes the user wrote.
///
/// `oxidgene-ui` compiles to WebAssembly and has no filesystem, so it
/// declares what it needs and the desktop binary supplies it — the same seam
/// as [`crate::geneanet::GeneanetBridge`]. The web build provides none, and
/// the settings page says so instead of offering a folder that cannot exist.
pub trait CustomThemeSource: Send + Sync {
    /// Where the files are read from, shown so the user can find the folder.
    fn location(&self) -> String;

    /// Re-read the folder.
    fn load(&self) -> CustomThemes;
}

/// Context handle for a [`CustomThemeSource`].
#[derive(Clone)]
pub struct CustomThemeLoader(Arc<dyn CustomThemeSource>);

impl CustomThemeLoader {
    /// Wrap a source for injection into the Dioxus context.
    #[must_use]
    pub fn new(source: Arc<dyn CustomThemeSource>) -> Self {
        Self(source)
    }

    /// Where the files are read from.
    #[must_use]
    pub fn location(&self) -> String {
        self.0.location()
    }

    /// Re-read the folder.
    #[must_use]
    pub fn load(&self) -> CustomThemes {
        self.0.load()
    }
}

impl PartialEq for CustomThemeLoader {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl fmt::Debug for CustomThemeLoader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CustomThemeLoader")
    }
}

// ── Selection ───────────────────────────────────────────────────────────────

/// `localStorage` key holding the selected theme's id.
///
/// The key and its two historical values, `light` and `dark`, are the ones
/// the previous light/dark switch used, so an existing choice carries over
/// without a migration.
const THEME_STORAGE_KEY: &str = "oxidgene-theme";

/// The themes on offer and the one in use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeState {
    /// Id of the selected theme.
    selected: String,
    /// What was read from the user's theme folder.
    custom: CustomThemes,
}

impl Default for ThemeState {
    fn default() -> Self {
        Self {
            selected: DEFAULT_THEME_ID.to_owned(),
            custom: CustomThemes::default(),
        }
    }
}

impl ThemeState {
    /// Every selectable theme, shipped ones first.
    pub fn themes(&self) -> impl Iterator<Item = &Theme> {
        BUILTIN_THEMES.iter().chain(self.custom.themes.iter())
    }

    /// The theme in use.
    ///
    /// A selection that no longer resolves — a custom theme whose file was
    /// renamed or removed — falls back to the default rather than leaving the
    /// application unstyled. The stored id is kept as it is, so putting the
    /// file back restores the choice.
    #[must_use]
    pub fn active(&self) -> &Theme {
        self.themes()
            .find(|theme| theme.id == self.selected)
            .or_else(|| builtin_theme(DEFAULT_THEME_ID))
            .unwrap_or_else(|| &BUILTIN_THEMES[0])
    }

    /// Id of the selected theme, whether or not it resolves.
    #[must_use]
    pub fn selected_id(&self) -> &str {
        &self.selected
    }

    /// Theme files that could not be loaded.
    #[must_use]
    pub fn errors(&self) -> &[CustomThemeError] {
        &self.custom.errors
    }
}

/// Hook: initialise the theme (call once in `Layout`).
///
/// There is deliberately no `prefers-color-scheme` branch. The application
/// starts light and stays light until someone picks otherwise: a theme list
/// that anyone can extend has no meaningful "system" member, and a palette
/// that changed under the user because the hour changed was surprising in a
/// window they leave open all day.
pub fn use_init_theme() -> Signal<ThemeState> {
    let loader = try_use_context::<CustomThemeLoader>();
    let mut state = use_context_provider(|| Signal::new(ThemeState::default()));

    use_effect(move || {
        let loader = loader.clone();
        spawn(async move {
            let custom = loader.map(|loader| loader.load()).unwrap_or_default();
            let stored = document::eval(&format!(
                "return localStorage.getItem('{THEME_STORAGE_KEY}');"
            ))
            .await
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned));

            state.set(ThemeState {
                selected: stored.unwrap_or_else(|| DEFAULT_THEME_ID.to_owned()),
                custom,
            });
        });
    });

    state
}

/// Persist and apply a theme choice.
pub fn set_theme(mut state: Signal<ThemeState>, id: &str) {
    state.write().selected = id.to_owned();
    // Ids are slugs, checked at parse time, so this cannot break out of the
    // string literal.
    document::eval(&format!(
        "localStorage.setItem('{THEME_STORAGE_KEY}', '{id}');"
    ));
}

/// Re-read the user's theme folder, keeping the current selection.
///
/// A folder that has not changed is not written back, so opening the section
/// does not re-render the picker for nothing.
///
/// The comparison reads through `peek`, which does not subscribe. This is
/// called from an effect, and a tracked read of the signal the same call
/// writes to is the shape a render loop comes from.
pub fn reload_custom_themes(mut state: Signal<ThemeState>, loader: &CustomThemeLoader) {
    let custom = loader.load();
    if state.peek().custom != custom {
        state.write().custom = custom;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_themes_are_valid() {
        let themes = &*BUILTIN_THEMES;
        assert_eq!(themes.len(), BUILTIN_SOURCES.len());
        assert!(themes.iter().all(|theme| theme.builtin));
        assert_eq!(themes[0].id, DEFAULT_THEME_ID);
    }

    #[test]
    fn every_token_is_emitted() {
        let css = builtin_theme("light").expect("light theme").css();
        for token in ThemeColors::TOKENS {
            assert!(css.contains(&format!("--{token}:")), "missing --{token}");
        }
    }

    /// Each shipped palette must actually be its own palette. A theme whose
    /// `base` overrides were dropped or misspelled would still parse and
    /// would silently be a copy of what it inherits from.
    #[test]
    fn every_builtin_theme_is_distinct() {
        for (index, theme) in BUILTIN_THEMES.iter().enumerate() {
            for other in BUILTIN_THEMES.iter().skip(index + 1) {
                assert_ne!(
                    theme.colors, other.colors,
                    "`{}` and `{}` are the same palette",
                    theme.id, other.id
                );
            }
        }
    }

    /// "Light" and "Dark" are interface words and are translated; a palette
    /// named after a place or a product is not.
    #[test]
    fn only_themes_with_a_translation_are_translated() {
        let fr = crate::i18n::I18n(crate::i18n::Language::Fr);
        assert_eq!(
            builtin_theme("dark").expect("dark theme").display_name(&fr),
            "Sombre"
        );
        assert_eq!(
            builtin_theme("nord").expect("nord theme").display_name(&fr),
            "Nord"
        );
        assert_eq!(
            builtin_theme("omarchy")
                .expect("omarchy theme")
                .display_name(&fr),
            "Omarchy"
        );
    }

    #[test]
    fn dark_differs_from_light_without_changing_the_token_set() {
        let light = builtin_theme("light").expect("light theme");
        let dark = builtin_theme("dark").expect("dark theme");
        assert_ne!(light.colors, dark.colors);
        assert_eq!(light.colors.orange, dark.colors.orange);
        assert_ne!(light.colors.bg_deep, dark.colors.bg_deep);
    }

    #[test]
    fn color_accepts_hex_shapes_and_rejects_css_functions() {
        assert_eq!(Color::parse("#FFF").expect("short").as_str(), "#fff");
        assert_eq!(Color::parse(" #1E1A14 ").expect("long").as_str(), "#1e1a14");
        assert_eq!(
            Color::parse("#00000014").expect("alpha").as_str(),
            "#00000014"
        );
        for rejected in ["rgb(0,0,0)", "red", "#12345", "#gggggg", "", "#"] {
            assert!(Color::parse(rejected).is_err(), "accepted `{rejected}`");
        }
    }

    #[test]
    fn rgb_digits_drops_alpha_and_expands_shorthand() {
        assert_eq!(Color::parse("#abc").expect("short").rgb_digits(), "aabbcc");
        assert_eq!(
            Color::parse("#1e1a1480").expect("alpha").rgb_digits(),
            "1e1a14"
        );
    }

    /// A theme value is written into a `<style>` element, so a value that
    /// could close the declaration must never get that far.
    ///
    /// The rejection surfaces as [`ThemeError::Syntax`] rather than
    /// [`ThemeError::Color`] because `Color`'s own error is raised from
    /// inside `Deserialize` and serde wraps it; the message still names the
    /// offending value, which is what the author needs.
    #[test]
    fn a_value_cannot_escape_the_declaration() {
        let source = r##"{"id":"x","name":"X","base":"light","colors":{"orange":"red;} body{display:none"}}"##;
        let error = parse_custom_theme(source).expect_err("must be rejected");
        assert!(matches!(error, ThemeError::Syntax(_)));
        assert!(
            error.to_string().contains("is not a hex colour"),
            "unhelpful message: {error}"
        );
    }

    /// The generated block is the only place a theme reaches CSS, so it must
    /// contain nothing but the declarations it is meant to.
    #[test]
    fn the_generated_block_holds_only_declarations() {
        let css = builtin_theme("dark").expect("dark theme").css();
        assert_eq!(css.matches('{').count(), 1);
        assert_eq!(css.matches('}').count(), 1);
        // One per token, plus the select arrow.
        assert_eq!(css.matches(";\n").count(), ThemeColors::TOKENS.len() + 1);
    }

    #[test]
    fn a_partial_theme_inherits_the_rest_of_its_base() {
        let source =
            r##"{"id":"sepia","name":"Sepia","base":"light","colors":{"orange":"#8a5a2b"}}"##;
        let theme = parse_custom_theme(source).expect("sepia");
        let light = builtin_theme("light").expect("light theme");
        assert_eq!(theme.colors.orange.as_str(), "#8a5a2b");
        assert_eq!(theme.colors.bg_deep, light.colors.bg_deep);
        assert!(!theme.builtin);
    }

    #[test]
    fn a_theme_without_a_base_must_be_complete() {
        let source = r##"{"id":"x","name":"X","colors":{"orange":"#8a5a2b"}}"##;
        assert!(matches!(
            parse_custom_theme(source),
            Err(ThemeError::MissingToken(_))
        ));
    }

    #[test]
    fn a_misspelled_token_is_reported_rather_than_ignored() {
        let source = r##"{"id":"x","name":"X","base":"light","colors":{"orang":"#8a5a2b"}}"##;
        assert!(matches!(
            parse_custom_theme(source),
            Err(ThemeError::UnknownToken(token)) if token == "orang"
        ));
    }

    #[test]
    fn a_custom_theme_cannot_take_a_builtin_id() {
        let source = r##"{"id":"dark","name":"Mine","base":"light","colors":{}}"##;
        assert!(matches!(
            parse_custom_theme(source),
            Err(ThemeError::ReservedId(_))
        ));
    }

    #[test]
    fn an_id_is_restricted_to_a_slug() {
        for id in ["Sepia", "my theme", "../escape", ""] {
            let source = format!(r##"{{"id":"{id}","name":"X","base":"light","colors":{{}}}}"##);
            assert!(
                matches!(parse_custom_theme(&source), Err(ThemeError::InvalidId(_))),
                "accepted `{id}`"
            );
        }
    }

    /// A custom theme's file can be renamed or deleted between two runs while
    /// its id is still the stored choice. That must not leave the application
    /// with no palette at all.
    #[test]
    fn a_selection_that_no_longer_resolves_falls_back_without_being_forgotten() {
        let state = ThemeState {
            selected: "gone".to_owned(),
            custom: CustomThemes::default(),
        };
        assert_eq!(state.active().id, DEFAULT_THEME_ID);
        assert_eq!(state.selected_id(), "gone");
    }

    #[test]
    fn custom_themes_are_offered_after_the_builtin_ones() {
        let sepia = parse_custom_theme(
            r##"{"id":"sepia","name":"Sepia","base":"light","colors":{"orange":"#8a5a2b"}}"##,
        )
        .expect("sepia");
        let state = ThemeState {
            selected: "sepia".to_owned(),
            custom: CustomThemes {
                themes: vec![sepia],
                errors: Vec::new(),
            },
        };
        let ids: Vec<&str> = state.themes().map(|theme| theme.id.as_str()).collect();
        let builtin: Vec<&str> = BUILTIN_THEMES.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids[..builtin.len()], builtin[..]);
        assert_eq!(ids.last(), Some(&"sepia"));
        assert_eq!(state.active().colors.orange.as_str(), "#8a5a2b");
    }

    #[test]
    fn an_unknown_base_is_reported() {
        let source = r##"{"id":"x","name":"X","base":"no-such-theme","colors":{}}"##;
        assert!(matches!(
            parse_custom_theme(source),
            Err(ThemeError::UnknownBase(_))
        ));
    }
}
