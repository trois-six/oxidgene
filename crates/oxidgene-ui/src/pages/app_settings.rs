//! Application-level settings page (theme, language, name display, API access).

use dioxus::prelude::*;

use crate::api::ApiClient;
use crate::components::layout::set_theme;
use crate::i18n::{self, Language, use_i18n};
use crate::prefs::{SortParticles, set_sort_particles};
use crate::router::Route;

/// Sidebar sections.
#[derive(Clone, Copy, PartialEq)]
enum Section {
    Appearance,
    Language,
    Names,
    Api,
}

#[component]
pub fn AppSettings() -> Element {
    let i18n = use_i18n();
    let is_dark = use_context::<Signal<bool>>();
    let lang_signal = use_context::<Signal<Language>>();
    let sort_particles = use_context::<Signal<SortParticles>>();

    let mut active_section = use_signal(|| Section::Appearance);

    rsx! {
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
                            AppearanceSection { is_dark }
                        },
                        Section::Language => rsx! {
                            LanguageSection { lang_signal }
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

#[component]
pub fn AppearanceSection(is_dark: Signal<bool>) -> Element {
    let i18n = use_i18n();
    let dark = *is_dark.read();

    rsx! {
        div { class: "settings-section",
            span { class: "settings-section-eyebrow", {i18n.t("app_settings.appearance")} }
            h2 { class: "settings-section-title", {i18n.t("app_settings.appearance_title")} }
            p { class: "settings-section-subtitle", {i18n.t("app_settings.appearance_desc")} }

            div { class: "app-settings-card",
                div { class: "app-settings-option",
                    div { class: "app-settings-option-info",
                        span { class: "app-settings-option-label", {i18n.t("app_settings.theme")} }
                        span { class: "app-settings-option-hint",
                            {i18n.t(if dark { "app_settings.theme_dark_active" } else { "app_settings.theme_light_active" })}
                        }
                    }
                    div { class: "theme-toggle-group",
                        button {
                            class: if !dark { "theme-toggle-btn active" } else { "theme-toggle-btn" },
                            onclick: move |_| set_theme(is_dark, false),
                            title: "{i18n.t(\"app_settings.theme_light\")}",
                            // Sun icon
                            svg {
                                width: "18",
                                height: "18",
                                fill: "none",
                                "viewBox": "0 0 24 24",
                                stroke: "currentColor",
                                "strokeWidth": "2",
                                circle { cx: "12", cy: "12", r: "5" }
                                line { x1: "12", y1: "1", x2: "12", y2: "3" }
                                line { x1: "12", y1: "21", x2: "12", y2: "23" }
                                line { x1: "4.22", y1: "4.22", x2: "5.64", y2: "5.64" }
                                line { x1: "18.36", y1: "18.36", x2: "19.78", y2: "19.78" }
                                line { x1: "1", y1: "12", x2: "3", y2: "12" }
                                line { x1: "21", y1: "12", x2: "23", y2: "12" }
                                line { x1: "4.22", y1: "19.78", x2: "5.64", y2: "18.36" }
                                line { x1: "18.36", y1: "5.64", x2: "19.78", y2: "4.22" }
                            }
                            span { {i18n.t("app_settings.theme_light")} }
                        }
                        button {
                            class: if dark { "theme-toggle-btn active" } else { "theme-toggle-btn" },
                            onclick: move |_| set_theme(is_dark, true),
                            title: "{i18n.t(\"app_settings.theme_dark\")}",
                            // Moon icon
                            svg {
                                width: "18",
                                height: "18",
                                fill: "none",
                                "viewBox": "0 0 24 24",
                                stroke: "currentColor",
                                "strokeWidth": "2",
                                path { d: "M21 12.79A9 9 0 1111.21 3 7 7 0 0021 12.79z" }
                            }
                            span { {i18n.t("app_settings.theme_dark")} }
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

/// Styles for the [`AppearanceSection`] / [`LanguageSection`] widgets only
/// (no layout/nav rules) — shared with the tree [`crate::pages::settings`]
/// page, which embeds these same sections under its own "Global
/// preferences" nav group.
pub(crate) const APP_SETTINGS_WIDGET_STYLES: &str = r#"
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

    .app-settings-option-info {
        display: flex;
        flex-direction: column;
        gap: 0.15rem;
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
    .settings-layout {
        display: flex;
        gap: 2rem;
    }

    .settings-nav {
        width: 200px;
        flex-shrink: 0;
    }

    .settings-nav-group {
        display: flex;
        flex-direction: column;
        gap: 2px;
    }

    .settings-nav-group-label {
        font-size: 0.7rem;
        font-weight: 700;
        text-transform: uppercase;
        letter-spacing: 0.08em;
        color: var(--orange);
        padding: 0.5rem 0.75rem 0.25rem;
    }

    .settings-nav-item {
        display: block;
        width: 100%;
        text-align: left;
        padding: 0.5rem 0.75rem;
        border: none;
        background: none;
        border-radius: 6px;
        font-size: 0.9rem;
        color: var(--text-secondary);
        cursor: pointer;
        transition: background 0.15s, color 0.15s;
    }

    .settings-nav-item:hover {
        background: var(--bg-card-hover);
        color: var(--text-primary);
    }

    .settings-nav-item.active {
        background: var(--bg-card);
        color: var(--orange);
        font-weight: 600;
    }

    .settings-content {
        flex: 1;
        min-width: 0;
    }

    .settings-section {
        margin-bottom: 2rem;
    }

    .settings-section-eyebrow {
        display: block;
        font-size: 0.7rem;
        font-weight: 700;
        text-transform: uppercase;
        letter-spacing: 0.08em;
        color: var(--orange);
        margin-bottom: 0.25rem;
    }

    .settings-section-title {
        font-family: var(--font-heading);
        font-size: 1.5rem;
        font-weight: 700;
        color: var(--text-primary);
        margin-bottom: 0.25rem;
    }

    .settings-section-subtitle {
        font-size: 0.9rem;
        color: var(--text-muted);
        margin-bottom: 1.25rem;
    }

    /* ── Card ─────────────────────────────── */

    .app-settings-card {
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: 10px;
        padding: 1.25rem;
    }

    /* ── Theme toggle ────────────────────── */

    .app-settings-option {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 1rem;
    }

    .app-settings-option-info {
        display: flex;
        flex-direction: column;
        gap: 0.15rem;
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

    /* ── Language options ─────────────────── */

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

    /* ── Responsive ───────────────────────── */

    @media (max-width: 640px) {
        .settings-layout {
            flex-direction: column;
        }
        .settings-nav {
            width: 100%;
            flex-direction: row;
            overflow-x: auto;
            padding-bottom: 4px;
        }
        .settings-nav-group {
            flex-direction: row;
            flex-wrap: nowrap;
            gap: 0.5rem;
        }
        .settings-nav-group-label {
            width: auto;
            flex: none;
            padding-right: 0.75rem;
            border-right: 1px solid var(--border);
            white-space: nowrap;
        }
        .settings-nav-item {
            width: auto;
            flex: none;
            white-space: nowrap;
        }
        .app-settings-option {
            flex-direction: column;
            align-items: flex-start;
        }
        .api-endpoint {
            align-items: flex-start;
            flex-direction: column;
        }
    }
"#;
