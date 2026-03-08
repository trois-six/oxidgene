//! Application-level settings page (theme, language).

use dioxus::prelude::*;

use crate::components::layout::set_theme;
use crate::i18n::{self, Language, use_i18n};
use crate::router::Route;

/// Sidebar sections.
#[derive(Clone, Copy, PartialEq)]
enum Section {
    Appearance,
    Language,
}

#[component]
pub fn AppSettings() -> Element {
    let i18n = use_i18n();
    let is_dark = use_context::<Signal<bool>>();
    let lang_signal = use_context::<Signal<Language>>();

    let mut active_section = use_signal(|| Section::Appearance);

    rsx! {
        style { {APP_SETTINGS_STYLES} }

        div { class: "sub-page",
            // ── Topbar breadcrumb ──────────────────────────────────
            div { class: "td-topbar",
                nav { class: "td-bc",
                    Link { to: Route::Home {}, class: "td-bc-link", {i18n.t("app_settings.breadcrumb_home")} }
                    span { class: "td-bc-sep", "/" }
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
                    }
                }
            }
            } // close sub-page-content
        } // close sub-page
    }
}

// ── Appearance section ──────────────────────────────────────────────────────

#[component]
fn AppearanceSection(is_dark: Signal<bool>) -> Element {
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
fn LanguageSection(lang_signal: Signal<Language>) -> Element {
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

// ── Styles ──────────────────────────────────────────────────────────────────

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
        color: #fff;
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

    /* ── Responsive ───────────────────────── */

    @media (max-width: 640px) {
        .settings-layout {
            flex-direction: column;
        }
        .settings-nav {
            width: 100%;
            flex-direction: row;
        }
        .settings-nav-group {
            flex-direction: row;
            flex-wrap: wrap;
            gap: 0.5rem;
        }
        .settings-nav-group-label {
            width: 100%;
        }
        .app-settings-option {
            flex-direction: column;
            align-items: flex-start;
        }
    }
"#;
