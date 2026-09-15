//! Application-level settings page (theme, language, name display, API access).

use dioxus::prelude::*;

use crate::api::ApiClient;
use crate::components::pedigree_theme::{CardFrame, LinkSpec, PedigreeThemeId, Point, link_path};
use crate::i18n::{self, Language, use_i18n};
use crate::prefs::{
    PedigreeDefaults, SortParticles, set_pedigree_defaults, set_pedigree_theme, set_sort_particles,
};
use crate::router::Route;
use crate::theme::{CustomThemeLoader, Theme, ThemeState, reload_custom_themes, set_theme};
use crate::ui_observability::{UiPage, use_ui_load_trace};

/// Sidebar sections.
#[derive(Clone, Copy, PartialEq)]
enum Section {
    Appearance,
    Language,
    Pedigree,
    Names,
    Api,
}

#[component]
pub fn AppSettings() -> Element {
    let _load_trace = use_ui_load_trace(UiPage::AppSettings);
    let i18n = use_i18n();
    let theme_state = use_context::<Signal<ThemeState>>();
    let lang_signal = use_context::<Signal<Language>>();
    let sort_particles = use_context::<Signal<SortParticles>>();
    let pedigree_defaults = use_context::<Signal<Option<PedigreeDefaults>>>();

    let mut active_section = use_signal(|| Section::Appearance);

    rsx! {
        style { {SHARED_SETTINGS_STYLES} }
        style { {APP_SETTINGS_STYLES} }

        div { class: "sub-page",
            // ── Topbar breadcrumb ──────────────────────────────────
            div { class: "td-topbar",
                nav { class: "td-bc",
                    Link { to: Route::Home {}, class: "td-bc-logo",
                        img {
                            src: crate::components::layout::LOGO_PNG_B64,
                            alt: "OxidGene",
                            class: "td-bc-logo-img",
                        }
                    }
                    span { class: "td-bc-current", {i18n.t("app_settings.title")} }
                }
            }

            div { class: "sub-page-content",
                div { class: "settings-layout",
                // ── Left sidebar ────────────────────────────
                nav { class: "settings-nav",
                    div { class: "settings-nav-group",
                        span { class: "settings-nav-group-label",
                            {i18n.t("app_settings.preferences")}
                        }
                        button {
                            class: if *active_section.read() == Section::Appearance { "settings-nav-item active" } else { "settings-nav-item" },
                            onclick: move |_| active_section.set(Section::Appearance),
                            {i18n.t("app_settings.appearance")}
                        }
                        button {
                            class: if *active_section.read() == Section::Language { "settings-nav-item active" } else { "settings-nav-item" },
                            onclick: move |_| active_section.set(Section::Language),
                            {i18n.t("app_settings.language")}
                        }
                        button {
                            class: if *active_section.read() == Section::Pedigree { "settings-nav-item active" } else { "settings-nav-item" },
                            onclick: move |_| active_section.set(Section::Pedigree),
                            {i18n.t("app_settings.pedigree")}
                        }
                        button {
                            class: if *active_section.read() == Section::Names { "settings-nav-item active" } else { "settings-nav-item" },
                            onclick: move |_| active_section.set(Section::Names),
                            {i18n.t("app_settings.names")}
                        }
                        button {
                            class: if *active_section.read() == Section::Api { "settings-nav-item active" } else { "settings-nav-item" },
                            onclick: move |_| active_section.set(Section::Api),
                            {i18n.t("app_settings.api")}
                        }
                    }
                }

                // ── Content area ────────────────────────────
                div { class: "settings-content",
                    match *active_section.read() {
                        Section::Appearance => rsx! {
                            AppearanceSection { theme_state }
                        },
                        Section::Language => rsx! {
                            LanguageSection { lang_signal }
                        },
                        Section::Pedigree => rsx! {
                            PedigreeDefaultsSection { pedigree_defaults }
                        },
                        Section::Names => rsx! {
                            NamesSection { sort_particles }
                        },
                        Section::Api => rsx! {
                            ApiSection {}
                        },
                    }
                }
            }
            } // close sub-page-content
        } // close sub-page
    }
}

// ── Appearance section ──────────────────────────────────────────────────────

/// A miniature of the theme, painted in that theme's own colours.
///
/// The swatch takes its values from the theme rather than from `var(--…)`,
/// because every swatch on the page shows a *different* theme from the one
/// currently applied — reading the cascade would paint them all alike. It
/// also means a theme the user wrote previews correctly without this page
/// knowing anything about it.
#[component]
fn ThemeSwatch(theme: ReadSignal<Theme>) -> Element {
    let theme = theme.read();
    let colors = &theme.colors;

    rsx! {
        div {
            class: "app-theme-swatch",
            style: "background: {colors.bg_deep}; border-color: {colors.border};",
            div {
                class: "app-theme-swatch-bar",
                style: "background: {colors.nav_surface}; border-color: {colors.border};",
                span {
                    class: "app-theme-swatch-dot",
                    style: "background: {colors.orange};",
                }
            }
            div {
                class: "app-theme-swatch-card",
                style: "background: {colors.bg_card}; border-color: {colors.border};",
                span {
                    class: "app-theme-swatch-line",
                    style: "background: {colors.text_primary};",
                }
                span {
                    class: "app-theme-swatch-line is-short",
                    style: "background: {colors.text_secondary};",
                }
                span {
                    class: "app-theme-swatch-accent",
                    style: "background: {colors.green_accent};",
                }
            }
        }
    }
}

#[component]
pub fn AppearanceSection(theme_state: Signal<ThemeState>) -> Element {
    let i18n = use_i18n();
    let loader = try_use_context::<CustomThemeLoader>();

    // Re-read the folder on arrival. This section is mounted only while it is
    // the one on screen, and someone opening it has usually just finished
    // editing a theme file — so the list is refreshed when it is about to be
    // looked at, and there is nothing to press.
    use_effect({
        let loader = loader.clone();
        move || {
            if let Some(loader) = &loader {
                reload_custom_themes(theme_state, loader);
            }
        }
    });

    let state = theme_state.read();
    let selected = state.selected_id().to_owned();
    let themes: Vec<Theme> = state.themes().cloned().collect();
    let errors = state.errors().to_vec();
    drop(state);

    rsx! {
        div { class: "settings-section",
            span { class: "settings-section-eyebrow", {i18n.t("app_settings.appearance")} }
            h2 { class: "settings-section-title", {i18n.t("app_settings.appearance_title")} }
            p { class: "settings-section-subtitle", {i18n.t("app_settings.appearance_desc")} }

            div { class: "app-settings-card",
                div { class: "app-settings-option app-settings-option-stacked",
                    div { class: "app-settings-option-info",
                        span { class: "app-settings-option-label", {i18n.t("app_settings.theme")} }
                        span { class: "app-settings-option-hint", {i18n.t("app_settings.theme_hint")} }
                    }

                    div { class: "theme-picker",
                        for theme in themes {
                            {
                                let id = theme.id.clone();
                                let active = id == selected;
                                rsx! {
                                    button {
                                        key: "{theme.id}",
                                        class: if active { "theme-picker-option active" } else { "theme-picker-option" },
                                        aria_pressed: if active { "true" } else { "false" },
                                        onclick: move |_| set_theme(theme_state, &id),
                                        ThemeSwatch { theme: theme.clone() }
                                        span { class: "theme-picker-label",
                                            {theme.display_name(&i18n)}
                                        }
                                        if !theme.builtin {
                                            span { class: "theme-picker-tag",
                                                {i18n.t("app_settings.theme_custom_tag")}
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Where custom themes come from. On the web there is no
                    // folder to point at, so the note says so rather than
                    // offering a path that cannot exist.
                    match loader {
                        Some(loader) => rsx! {
                            div { class: "app-theme-source",
                                p { class: "app-settings-option-hint",
                                    {i18n.t("app_settings.theme_custom_hint")}
                                }
                                code { class: "app-theme-folder", {loader.location()} }
                            }
                        },
                        None => rsx! {
                            p { class: "app-settings-option-hint app-theme-source",
                                {i18n.t("app_settings.theme_custom_desktop_only")}
                            }
                        },
                    }

                    if !errors.is_empty() {
                        ul { class: "app-theme-errors",
                            for error in errors {
                                li { key: "{error.file}",
                                    strong { "{error.file}" }
                                    " — "
                                    "{error.message}"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

// ── Language section ────────────────────────────────────────────────────────

#[component]
pub fn LanguageSection(lang_signal: Signal<Language>) -> Element {
    let i18n = use_i18n();
    let current = *lang_signal.read();

    rsx! {
        div { class: "settings-section",
            span { class: "settings-section-eyebrow", {i18n.t("app_settings.language")} }
            h2 { class: "settings-section-title", {i18n.t("app_settings.language_title")} }
            p { class: "settings-section-subtitle", {i18n.t("app_settings.language_desc")} }

            div { class: "app-settings-card",
                div { class: "lang-options",
                    for lang in [Language::En, Language::Fr] {
                        button {
                            key: "{lang.code()}",
                            class: if current == lang { "lang-option active" } else { "lang-option" },
                            onclick: move |_| i18n::set_language(lang_signal, lang),
                            span { class: "lang-option-flag",
                                {match lang {
                                    Language::En => "\u{1F1EC}\u{1F1E7}",
                                    Language::Fr => "\u{1F1EB}\u{1F1F7}",
                                }}
                            }
                            span { class: "lang-option-label", {lang.label()} }
                            if current == lang {
                                span { class: "lang-option-check", "\u{2713}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

// ── Pedigree section ────────────────────────────────────────────────────────

/// A parent card, a child card and the line between them, drawn from the
/// theme's own values.
///
/// Deliberately schematic — no names, no portrait. What a reader is choosing
/// between is the frame, the line and the ground, and those are exactly what
/// this shows. Taking them from the theme rather than drawing a picture of
/// each one means the swatch cannot go stale when a theme is adjusted.
#[component]
fn PedigreeThemeSwatch(id: PedigreeThemeId) -> Element {
    let theme = id.theme();
    let m = theme.metrics;
    let (rw, rh) = m.rect(false);

    // The child is set aside rather than straight below, because a connector
    // that runs vertically is a straight line in every style — the swatch
    // would show the frame and hide the one other thing it is meant to
    // compare. Stepping sideways is what separates a curve from a right
    // angle, and `is_edge` is what asks the classic style for its curve.
    let child_dx = m.card_w * 0.5;
    let link = link_path(
        &LinkSpec::SimpleChild {
            from: Point::new(0.0, 0.0),
            to: Point::new(child_dx, m.card_h),
            is_edge: true,
        },
        theme.link_style,
        &m,
    );
    let vb_w = m.card_w + child_dx;
    let vb_h = m.card_h + rh + 2.0 * m.padding;
    let inner = match theme.card.frame {
        CardFrame::Plain => None,
        CardFrame::Cartouche { inner_inset } => Some((
            m.padding + inner_inset,
            rw - 2.0 * inner_inset,
            rh - 2.0 * inner_inset,
        )),
    };
    let frame_w = theme.card.frame_width;

    rsx! {
        svg {
            class: "ped-theme-swatch {theme.viewport_class}",
            "viewBox": "0 0 {vb_w} {vb_h}",
            "preserveAspectRatio": "xMidYMid meet",
            "aria-hidden": "true",
            rect {
                x: "0", y: "0", width: "{vb_w}", height: "{vb_h}",
                style: "fill:var(--pn-swatch-bg,transparent)",
            }
            path { d: "{link}", class: "pedigree-connector-path" }
            for (i, (x, y)) in [(0.0f64, 0.0f64), (child_dx, m.card_h)].into_iter().enumerate() {
                g { key: "{i}", transform: "translate({x},{y})",
                    rect {
                        class: "ped-card-rect",
                        x: "{m.padding}", y: "{m.padding}",
                        rx: "{m.border_radius}", ry: "{m.border_radius}",
                        width: "{rw}", height: "{rh}",
                        style: "fill:var(--pn-bg);stroke:var(--pn-border);stroke-width:{frame_w}",
                    }
                    if let Some((inset, iw, ih)) = inner {
                        rect {
                            class: "ped-card-inner-rule",
                            x: "{inset}", y: "{inset}", width: "{iw}", height: "{ih}",
                            style: "fill:none;stroke:var(--pn-border);stroke-width:1",
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub fn PedigreeDefaultsSection(pedigree_defaults: Signal<Option<PedigreeDefaults>>) -> Element {
    let i18n = use_i18n();
    let current = (*pedigree_defaults.read()).unwrap_or_default();
    let theme_pref = use_context::<Signal<PedigreeThemeId>>();
    let current_theme = *theme_pref.read();

    rsx! {
        div { class: "settings-section",
            span { class: "settings-section-eyebrow", {i18n.t("app_settings.pedigree")} }
            h2 { class: "settings-section-title", {i18n.t("app_settings.pedigree_title")} }
            p { class: "settings-section-subtitle", {i18n.t("app_settings.pedigree_desc")} }

            div { class: "app-settings-card",
                div { class: "app-settings-option app-settings-option-stacked",
                    div { class: "app-settings-option-info",
                        span { class: "app-settings-option-label", {i18n.t("app_settings.pedigree_theme")} }
                        span { class: "app-settings-option-hint", {i18n.t("app_settings.pedigree_theme_hint")} }
                    }
                    div { class: "theme-picker",
                        for id in PedigreeThemeId::ALL {
                            button {
                                key: "{id:?}",
                                class: if current_theme == id { "theme-picker-option active" } else { "theme-picker-option" },
                                aria_pressed: if current_theme == id { "true" } else { "false" },
                                onclick: move |_| set_pedigree_theme(theme_pref, id),
                                PedigreeThemeSwatch { id }
                                span { class: "theme-picker-label", {i18n.t(id.label_key())} }
                                span { class: "theme-picker-hint", {i18n.t(id.hint_key())} }
                            }
                        }
                    }
                }
                div { class: "app-settings-option",
                    div { class: "app-settings-option-info",
                        span { class: "app-settings-option-label", {i18n.t("app_settings.ancestor_levels")} }
                        span { class: "app-settings-option-hint", {i18n.t("app_settings.ancestor_levels_hint")} }
                    }
                    div { class: "pedigree-depth-stepper",
                        button {
                            class: "pedigree-depth-btn",
                            disabled: current.ancestor_levels == 0,
                            title: i18n.t("app_settings.decrease_ancestor_levels"),
                            aria_label: i18n.t("app_settings.decrease_ancestor_levels"),
                            onclick: move |_| set_pedigree_defaults(
                                pedigree_defaults,
                                PedigreeDefaults {
                                    ancestor_levels: current.ancestor_levels.saturating_sub(1),
                                    ..current
                                },
                            ),
                            "-"
                        }
                        span { class: "pedigree-depth-value", "{current.ancestor_levels}" }
                        button {
                            class: "pedigree-depth-btn",
                            disabled: current.ancestor_levels >= crate::prefs::MAX_PEDIGREE_LEVELS,
                            title: i18n.t("app_settings.increase_ancestor_levels"),
                            aria_label: i18n.t("app_settings.increase_ancestor_levels"),
                            onclick: move |_| set_pedigree_defaults(
                                pedigree_defaults,
                                PedigreeDefaults {
                                    ancestor_levels: current.ancestor_levels + 1,
                                    ..current
                                },
                            ),
                            "+"
                        }
                    }
                }
                div { class: "app-settings-option",
                    div { class: "app-settings-option-info",
                        span { class: "app-settings-option-label", {i18n.t("app_settings.descendant_levels")} }
                        span { class: "app-settings-option-hint", {i18n.t("app_settings.descendant_levels_hint")} }
                    }
                    div { class: "pedigree-depth-stepper",
                        button {
                            class: "pedigree-depth-btn",
                            disabled: current.descendant_levels == 0,
                            title: i18n.t("app_settings.decrease_descendant_levels"),
                            aria_label: i18n.t("app_settings.decrease_descendant_levels"),
                            onclick: move |_| set_pedigree_defaults(
                                pedigree_defaults,
                                PedigreeDefaults {
                                    descendant_levels: current.descendant_levels.saturating_sub(1),
                                    ..current
                                },
                            ),
                            "-"
                        }
                        span { class: "pedigree-depth-value", "{current.descendant_levels}" }
                        button {
                            class: "pedigree-depth-btn",
                            disabled: current.descendant_levels >= crate::prefs::MAX_PEDIGREE_LEVELS,
                            title: i18n.t("app_settings.increase_descendant_levels"),
                            aria_label: i18n.t("app_settings.increase_descendant_levels"),
                            onclick: move |_| set_pedigree_defaults(
                                pedigree_defaults,
                                PedigreeDefaults {
                                    descendant_levels: current.descendant_levels + 1,
                                    ..current
                                },
                            ),
                            "+"
                        }
                    }
                }
            }
        }
    }
}

// ── API section ─────────────────────────────────────────────────────────────

#[component]
fn ApiSection() -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let openapi_url = api.openapi_url();
    let graphql_url = api.graphql_url();

    rsx! {
        div { class: "settings-section",
            span { class: "settings-section-eyebrow", {i18n.t("app_settings.api")} }
            h2 { class: "settings-section-title", {i18n.t("app_settings.api_title")} }
            p { class: "settings-section-subtitle", {i18n.t("app_settings.api_desc")} }

            div { class: "app-settings-card api-endpoints",
                a {
                    class: "api-endpoint",
                    href: openapi_url.clone(),
                    target: "_blank",
                    rel: "noopener noreferrer",
                    title: i18n.t("app_settings.openapi_open"),
                    div { class: "api-endpoint-info",
                        span { class: "app-settings-option-label",
                            {i18n.t("app_settings.openapi_label")}
                        }
                        span { class: "app-settings-option-hint",
                            {i18n.t("app_settings.openapi_hint")}
                        }
                        code { class: "api-endpoint-url", "{openapi_url}" }
                    }
                    svg {
                        class: "api-external-icon",
                        width: "18",
                        height: "18",
                        fill: "none",
                        "viewBox": "0 0 24 24",
                        stroke: "currentColor",
                        "strokeWidth": "2",
                        path { d: "M15 3h6v6" }
                        path { d: "M10 14 21 3" }
                        path { d: "M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" }
                    }
                }
                if cfg!(target_arch = "wasm32") {
                    a {
                        class: "api-endpoint",
                        href: graphql_url.clone(),
                        target: "_blank",
                        rel: "noopener noreferrer",
                        title: i18n.t("app_settings.graphql_open"),
                        div { class: "api-endpoint-info",
                            span { class: "app-settings-option-label",
                                {i18n.t("app_settings.graphql_label")}
                            }
                            span { class: "app-settings-option-hint",
                                {i18n.t("app_settings.graphql_hint")}
                            }
                            code { class: "api-endpoint-url", "{graphql_url}" }
                        }
                        svg {
                            class: "api-external-icon",
                            width: "18",
                            height: "18",
                            fill: "none",
                            "viewBox": "0 0 24 24",
                            stroke: "currentColor",
                            "strokeWidth": "2",
                            path { d: "M15 3h6v6" }
                            path { d: "M10 14 21 3" }
                            path { d: "M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" }
                        }
                    }
                }
            }
        }
    }
}

// ── Styles ──────────────────────────────────────────────────────────────────

/// Shared settings layout and application-preference widget styles.
///
/// The tree [`crate::pages::settings`] page embeds the same sections under its
/// own "Global preferences" nav group and uses this layout as the canonical
/// visual treatment for both settings surfaces.
pub(crate) const SHARED_SETTINGS_STYLES: &str = r#"
    /* ── Theme pickers ──────────────────────────────────────────────
       One grid for both the application palette and the pedigree chart
       style: they are the same control over different things, and a
       reader moving between the two sections should not have to work
       out that they are. Only the swatch inside differs — a painted
       miniature for a palette, an SVG card for a chart style. */
    /* `auto-fill`, not `auto-fit`: fit collapses the empty tracks and lets the
       few items that exist stretch across the row, which would give the two
       pedigree styles tiles three times the width of the five palettes. Fill
       keeps the tracks, so a tile is the same size in both pickers. */
    .theme-picker {
        display: grid;
        grid-template-columns: repeat(auto-fill, minmax(132px, 1fr));
        gap: 0.75rem;
    }

    .theme-picker-option {
        display: grid;
        justify-items: center;
        align-content: start;
        gap: 0.4rem;
        padding: 0.7rem;
        border: 1px solid var(--border);
        border-radius: 8px;
        background: none;
        cursor: pointer;
        transition: border-color 0.15s, background 0.15s;
        text-align: center;
        color: var(--text-primary);
        font-family: var(--font-sans);
    }

    .theme-picker-option:hover { border-color: var(--orange); }

    .theme-picker-option.active {
        border-color: var(--orange);
        background: var(--sel-bg);
    }

    .theme-picker-label {
        font-size: 0.9rem;
        font-weight: 600;
    }

    .theme-picker-hint {
        font-size: 0.78rem;
        color: var(--text-secondary);
        line-height: 1.35;
    }

    .theme-picker-tag {
        font-size: 0.68rem;
        letter-spacing: 0.04em;
        text-transform: uppercase;
        color: var(--text-secondary);
    }

    /* Both swatches occupy the same box, so the two pickers line up. */
    .app-theme-swatch,
    .ped-theme-swatch {
        width: 100%;
        height: 68px;
    }

    /* The miniature is painted from inline values, so it carries no colour
       of its own: everything here is layout. */
    .app-theme-swatch {
        border: 1px solid;
        border-radius: 5px;
        overflow: hidden;
        display: flex;
        flex-direction: column;
    }

    .app-theme-swatch-bar {
        height: 13px;
        border-bottom: 1px solid;
        display: flex;
        align-items: center;
        padding: 0 4px;
        flex: none;
    }

    .app-theme-swatch-dot {
        width: 6px;
        height: 6px;
        border-radius: 50%;
    }

    .app-theme-swatch-card {
        margin: 6px;
        padding: 5px;
        border: 1px solid;
        border-radius: 3px;
        flex: 1;
        display: flex;
        flex-direction: column;
        justify-content: center;
        gap: 4px;
    }

    .app-theme-swatch-line {
        height: 3px;
        border-radius: 2px;
        width: 100%;
        opacity: 0.85;
    }

    .app-theme-swatch-line.is-short { width: 60%; }

    .app-theme-swatch-accent {
        height: 5px;
        width: 34%;
        border-radius: 2px;
    }

    .app-theme-source {
        display: flex;
        align-items: center;
        flex-wrap: wrap;
        gap: 0.6rem;
    }

    .app-theme-folder {
        font-size: 0.78rem;
        padding: 3px 7px;
        border: 1px solid var(--border);
        border-radius: 4px;
        background: var(--bg-deep);
        color: var(--text-secondary);
        word-break: break-all;
    }

    /* A theme file that failed to load is named here rather than only in the
       log: the person who wrote it is the one who can fix it. */
    .app-theme-errors {
        margin: 0;
        padding-left: 1.1rem;
        font-size: 0.8rem;
        line-height: 1.45;
        color: var(--danger-text);
    }

    /* The swatch carries the theme's own variables and uses the same ground
       as the pedigree canvas. */
    .ped-theme-swatch {
        border-radius: 5px;
        border: 1px solid var(--border);
        /* The ground is painted by the rect inside. */
        --pn-swatch-bg: var(--bg-deep);
    }

    .settings-layout {
        display: flex;
        gap: 24px;
        min-height: 0;
    }

    .settings-nav {
        width: 200px;
        min-width: 200px;
        flex-shrink: 0;
    }

    .settings-nav-group {
        margin-bottom: 20px;
    }

    .settings-nav-group-label {
        font-size: 0.68rem;
        font-weight: 700;
        color: var(--orange);
        text-transform: uppercase;
        letter-spacing: 0.5px;
        margin-bottom: 6px;
        padding: 0 8px;
    }

    .settings-nav-item {
        display: block;
        width: 100%;
        padding: 6px 8px;
        text-align: left;
        background: none;
        border: none;
        border-radius: 5px;
        font-size: 0.85rem;
        color: var(--text-secondary);
        cursor: pointer;
        transition: background 0.12s, color 0.12s;
        font-family: var(--font-sans);
    }

    .settings-nav-item:hover {
        background: var(--bg-card-hover);
        color: var(--text-primary);
    }

    .settings-nav-item.active {
        background: var(--sel-bg);
        color: var(--text-primary);
        font-weight: 600;
    }

    .settings-content {
        flex: 1;
        min-width: 0;
        max-width: 860px;
    }

    .settings-section-eyebrow {
        font-size: 0.68rem;
        font-weight: 700;
        color: var(--orange);
        text-transform: uppercase;
        letter-spacing: 0.5px;
        margin-bottom: 4px;
    }

    .settings-section-title {
        font-family: var(--font-heading);
        font-size: 1.2rem;
        font-weight: 600;
        color: var(--text-primary);
        margin-bottom: 4px;
    }

    .settings-section-subtitle {
        font-size: 0.85rem;
        color: var(--text-secondary);
    }

    .app-settings-card {
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: 10px;
        padding: 1.25rem;
    }

    .app-settings-option {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 1rem;
    }

    /* Must stay after the rule above: both are single-class selectors, so the
       later one wins the tie. Declared first, `align-items: center` took it
       back and every picker grid collapsed to one centred column. */
    .app-settings-option-stacked {
        flex-direction: column;
        align-items: stretch;
        gap: 0.9rem;
    }

    .app-settings-option-info {
        display: flex;
        flex-direction: column;
        gap: 0.15rem;
    }

    .app-settings-option + .app-settings-option {
        margin-top: 1rem;
        padding-top: 1rem;
        border-top: 1px solid var(--border);
    }

    .pedigree-depth-stepper {
        display: grid;
        grid-template-columns: 2rem 2.5rem 2rem;
        align-items: center;
        border: 1px solid var(--border);
        border-radius: 8px;
        overflow: hidden;
        flex-shrink: 0;
    }

    .pedigree-depth-btn {
        width: 2rem;
        height: 2rem;
        border: none;
        background: none;
        color: var(--text-primary);
        cursor: pointer;
        font-size: 1rem;
    }

    .pedigree-depth-btn:hover:not(:disabled) {
        background: var(--bg-card-hover);
    }

    .pedigree-depth-btn:disabled {
        color: var(--text-muted);
        cursor: default;
        opacity: 0.5;
    }

    .pedigree-depth-value {
        line-height: 2rem;
        text-align: center;
        border-right: 1px solid var(--border);
        border-left: 1px solid var(--border);
        color: var(--text-primary);
        font-variant-numeric: tabular-nums;
        font-weight: 600;
    }

    .app-settings-option-label {
        font-size: 0.95rem;
        font-weight: 600;
        color: var(--text-primary);
    }

    .app-settings-option-hint {
        font-size: 0.8rem;
        color: var(--text-muted);
    }

    .theme-toggle-group {
        display: flex;
        gap: 0;
        border: 1px solid var(--border);
        border-radius: 8px;
        overflow: hidden;
    }

    .theme-toggle-btn {
        display: flex;
        align-items: center;
        gap: 0.4rem;
        padding: 0.45rem 0.85rem;
        border: none;
        background: none;
        font-size: 0.85rem;
        color: var(--text-muted);
        cursor: pointer;
        transition: background 0.15s, color 0.15s;
    }

    .theme-toggle-btn:first-child {
        border-right: 1px solid var(--border);
    }

    .theme-toggle-btn:hover {
        background: var(--bg-card-hover);
        color: var(--text-primary);
    }

    .theme-toggle-btn.active {
        background: var(--orange);
        color: var(--white);
    }

    .lang-options {
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
    }

    .lang-option {
        display: flex;
        align-items: center;
        gap: 0.75rem;
        padding: 0.75rem 1rem;
        border: 1px solid var(--border);
        border-radius: 8px;
        background: none;
        cursor: pointer;
        transition: border-color 0.15s, background 0.15s;
        width: 100%;
        text-align: left;
        font-size: 0.95rem;
        color: var(--text-primary);
    }

    .lang-option:hover {
        border-color: var(--orange);
        background: var(--bg-card-hover);
    }

    .lang-option.active {
        border-color: var(--orange);
        background: color-mix(in srgb, var(--orange) 8%, transparent);
    }

    .lang-option-flag {
        font-size: 1.3rem;
    }

    .lang-option-label {
        flex: 1;
        font-weight: 500;
    }

    .lang-option-check {
        color: var(--orange);
        font-weight: 700;
        font-size: 1rem;
    }

    @media (max-width: 768px) {
        .settings-layout {
            flex-direction: column;
        }
        .settings-nav {
            width: 100%;
            min-width: 0;
            display: flex;
            flex-wrap: nowrap;
            align-items: center;
            gap: 12px;
            overflow-x: auto;
            padding-bottom: 4px;
        }
        .settings-nav-group {
            display: flex;
            flex: none;
            align-items: center;
            flex-wrap: nowrap;
            gap: 4px;
            margin-bottom: 0;
        }
        .settings-nav-group-label {
            width: auto;
            margin: 0 4px 0 0;
            padding: 0 8px 0 0;
            border-right: 1px solid var(--border);
            white-space: nowrap;
        }
        .settings-nav-item {
            width: auto;
            flex: none;
            white-space: nowrap;
        }
    }

    @media (max-width: 640px) {
        .app-settings-option {
            flex-direction: column;
            align-items: flex-start;
        }
    }
"#;

// ── Names section ───────────────────────────────────────────────────────────

/// How surnames carrying a particle ("de la Cruz") are filed alphabetically.
///
/// Both conventions are in real use — French genealogy usually files under the
/// particle, many catalogues file under the root — so this is a preference,
/// not a correctness question. It only affects ordering: names always *display*
/// with their particle.
#[component]
pub fn NamesSection(sort_particles: Signal<SortParticles>) -> Element {
    let i18n = use_i18n();
    let include = sort_particles.read().0;

    rsx! {
        div { class: "settings-section",
            span { class: "settings-section-eyebrow", {i18n.t("app_settings.names")} }
            h2 { class: "settings-section-title", {i18n.t("app_settings.names_title")} }
            p { class: "settings-section-subtitle", {i18n.t("app_settings.names_desc")} }

            div { class: "app-settings-card",
                div { class: "app-settings-option",
                    div { class: "app-settings-option-info",
                        span { class: "app-settings-option-label",
                            {i18n.t("app_settings.sort_particles")}
                        }
                        span { class: "app-settings-option-hint",
                            {i18n.t(if include {
                                "app_settings.sort_particles_included_hint"
                            } else {
                                "app_settings.sort_particles_ignored_hint"
                            })}
                        }
                    }
                    div { class: "theme-toggle-group",
                        button {
                            class: if include { "theme-toggle-btn active" } else { "theme-toggle-btn" },
                            onclick: move |_| set_sort_particles(sort_particles, true),
                            {i18n.t("app_settings.sort_particles_included")}
                        }
                        button {
                            class: if include { "theme-toggle-btn" } else { "theme-toggle-btn active" },
                            onclick: move |_| set_sort_particles(sort_particles, false),
                            {i18n.t("app_settings.sort_particles_ignored")}
                        }
                    }
                }
            }
        }
    }
}

const APP_SETTINGS_STYLES: &str = r#"
    .api-endpoints {
        padding: 0;
        overflow: hidden;
    }

    .api-endpoint {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 1rem;
        padding: 1rem 1.25rem;
        border-bottom: 1px solid var(--border);
        color: inherit;
        text-decoration: none;
        transition: background 0.15s;
    }

    .api-endpoint:last-child {
        border-bottom: none;
    }

    .api-endpoint:hover {
        background: var(--bg-card-hover);
    }

    .api-endpoint-info {
        display: flex;
        min-width: 0;
        flex-direction: column;
        gap: 0.2rem;
    }

    .api-endpoint-url {
        margin-top: 0.2rem;
        color: var(--orange);
        font-size: 0.78rem;
        overflow-wrap: anywhere;
    }

    .api-external-icon {
        flex: none;
        color: var(--text-muted);
    }

    @media (max-width: 640px) {
        .api-endpoint {
            align-items: flex-start;
            flex-direction: column;
        }
    }
"#;
