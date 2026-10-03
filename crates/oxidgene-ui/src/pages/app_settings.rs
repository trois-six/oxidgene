//! Application-level settings page (theme, language, name display, API access).

use dioxus::prelude::*;

use crate::api::ApiClient;
use crate::assistant::{AssistantLauncher, use_assistant_launcher};
use crate::components::breadcrumb::TreeBreadcrumb;
use crate::components::copy_field::CopyField;
use crate::components::pedigree_chart::PedigreeViewSwatch;
use crate::components::pedigree_theme::{CardFrame, LinkSpec, PedigreeThemeId, Point, link_path};
use crate::components::pedigree_view::PedigreeView;
use crate::i18n::{
    self, CustomLanguageLoader, Language, LanguageCatalog, reload_custom_languages, use_i18n,
};
use crate::prefs::{
    PedigreeDefaults, SortParticles, set_pedigree_defaults, set_pedigree_theme, set_pedigree_view,
    set_sort_particles,
};
use crate::theme::{CustomThemeLoader, Theme, ThemeState, reload_custom_themes, set_theme};
use crate::ui_observability::{UiPage, use_ui_load_trace};

pub(crate) fn use_settings_scroll_reset<T: Clone + 'static>(section: Signal<T>) {
    use_effect(move || {
        let _section = section();
        document::eval(
            r#"
            requestAnimationFrame(() => {
                const content = document.querySelector('.settings-layout')
                    ?.closest('.sub-page-content');
                if (content) content.scrollTop = 0;
            });
            "#,
        );
    });
}

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
    use_settings_scroll_reset(active_section);

    rsx! {
        style { {APP_SETTINGS_STYLES} }

        div { class: "sub-page",
            // ── Topbar breadcrumb ──────────────────────────────────
            div { class: "td-topbar",
                TreeBreadcrumb {
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

/// A miniature of the theme in its own colours, typefaces, corners and
/// shadows.
///
/// Every swatch on the page shows a *different* theme from the one applied,
/// so the swatch declares that theme's custom properties on itself: its
/// stylesheet rules then resolve `var(--…)` against the theme shown. A theme
/// the user wrote previews correctly without this page knowing anything
/// about it.
#[component]
fn ThemeSwatch(theme: ReadSignal<Theme>) -> Element {
    let properties = theme.read().declarations();

    rsx! {
        div { class: "app-theme-swatch", style: "{properties}",
            div { class: "app-theme-swatch-bar",
                span { class: "app-theme-swatch-dot" }
            }
            div { class: "app-theme-swatch-card",
                // A specimen of the heading typeface, not text to read.
                span { class: "app-theme-swatch-type", aria_hidden: "true", "Aa" }
                span { class: "app-theme-swatch-line" }
                span { class: "app-theme-swatch-line is-short" }
                span { class: "app-theme-swatch-accent" }
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
    let catalog: Signal<LanguageCatalog> = use_context();
    let loader = try_use_context::<CustomLanguageLoader>();
    use_effect(move || {
        if let Some(loader) = &loader {
            reload_custom_languages(lang_signal, catalog, loader);
        }
    });
    let options = catalog.read().clone();

    rsx! {
        div { class: "settings-section",
            span { class: "settings-section-eyebrow", {i18n.t("app_settings.language")} }
            h2 { class: "settings-section-title", {i18n.t("app_settings.language_title")} }
            p { class: "settings-section-subtitle", {i18n.t("app_settings.language_desc")} }

            div { class: "app-settings-card app-settings-option app-settings-option-stacked",
                div { class: "lang-options",
                    for lang in options.languages {
                        button {
                            key: "{lang.code()}",
                            class: if current == lang { "lang-option active" } else { "lang-option" },
                            aria_pressed: if current == lang { "true" } else { "false" },
                            onclick: move |_| i18n::set_language(lang_signal, lang),
                            span { class: "lang-option-flag", {lang.flag()} }
                            span { class: "lang-option-label", {lang.native_name()} }
                            if current == lang {
                                span { class: "lang-option-check", "\u{2713}" }
                            }
                        }
                    }
                }
                if let Some(location) = options.location {
                    div { class: "app-theme-source",
                        p { class: "app-settings-option-hint",
                            {i18n.t("app_settings.language_custom_hint")}
                        }
                        code { class: "app-theme-folder", {location} }
                    }
                }
                if !options.errors.is_empty() {
                    ul { class: "app-theme-errors",
                        for error in options.errors {
                            li { key: "{error.file}",
                                strong { "{error.file}: " }
                                {i18n.t("app_settings.language_file_error")}
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
    let view_pref = use_context::<Signal<PedigreeView>>();
    let current_view = *view_pref.read();

    rsx! {
        div { class: "settings-section",
            span { class: "settings-section-eyebrow", {i18n.t("app_settings.pedigree")} }
            h2 { class: "settings-section-title", {i18n.t("app_settings.pedigree_title")} }
            p { class: "settings-section-subtitle", {i18n.t("app_settings.pedigree_desc")} }

            div { class: "app-settings-card",
                div { class: "app-settings-option app-settings-option-stacked",
                    div { class: "app-settings-option-info",
                        span { class: "app-settings-option-label", {i18n.t("app_settings.pedigree_view")} }
                        span { class: "app-settings-option-hint", {i18n.t("app_settings.pedigree_view_hint")} }
                    }
                    div { class: "theme-picker {current_theme.theme().viewport_class}",
                        for view in PedigreeView::ALL {
                            button {
                                key: "{view:?}",
                                class: if current_view == view { "theme-picker-option active" } else { "theme-picker-option" },
                                aria_pressed: if current_view == view { "true" } else { "false" },
                                onclick: move |_| set_pedigree_view(view_pref, view),
                                PedigreeViewSwatch { view }
                                span { class: "theme-picker-label", {i18n.t(view.label_key())} }
                                span { class: "theme-picker-hint", {i18n.t(view.hint_key())} }
                            }
                        }
                    }
                }
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
            }
        }
        div { class: "settings-section",
            h2 { class: "settings-section-title", {i18n.t("app_settings.pedigree_depth")} }
            div { class: "app-settings-card",
                div { class: "app-settings-option",
                    div { class: "app-settings-option-info",
                        span { class: "app-settings-option-label", {i18n.t("app_settings.ancestor_levels")} }
                        span { class: "app-settings-option-hint", {i18n.t("app_settings.ancestor_levels_hint")} }
                    }
                    div { class: "pedigree-depth-stepper",
                        button {
                            class: "pedigree-depth-step",
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
                            class: "pedigree-depth-step",
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
                            class: "pedigree-depth-step",
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
                            class: "pedigree-depth-step",
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

/// The API section: where each endpoint is, and how an external client
/// connects to it (`docs/ui-app-settings.md` §8).
///
/// Both builds serve REST. Only the web server serves GraphQL: the desktop
/// compiles the API without it, so its entries show in the web build alone.
/// Only the desktop's embedded backend requires a bearer token; the client
/// knows it, and its absence means the backend asks for none.
#[component]
fn ApiSection() -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let rest_url = api.rest_url();
    let openapi_url = api.openapi_url();
    let graphql_url = api.graphql_url();
    let token = api.auth_token();
    let rest_example = openapi_curl_example(&openapi_url);
    let graphql_example = graphql_curl_example(&graphql_url);
    let graphql = cfg!(target_arch = "wasm32");

    rsx! {
        div { class: "settings-section",
            span { class: "settings-section-eyebrow", {i18n.t("app_settings.api")} }
            h2 { class: "settings-section-title", {i18n.t("app_settings.api_title")} }
            p { class: "settings-section-subtitle", {i18n.t("app_settings.api_desc")} }

            div { class: "app-settings-card api-endpoints",
                ApiEndpointLink {
                    href: openapi_url,
                    label: i18n.t("app_settings.openapi_label"),
                    hint: i18n.t("app_settings.openapi_hint"),
                    title: i18n.t("app_settings.openapi_open"),
                }
                if graphql {
                    ApiEndpointLink {
                        href: graphql_url.clone(),
                        label: i18n.t("app_settings.graphql_label"),
                        hint: i18n.t("app_settings.graphql_hint"),
                        title: i18n.t("app_settings.graphql_open"),
                    }
                }
            }

            div { class: "app-settings-card copy-card",
                span { class: "app-settings-option-label", {i18n.t("app_settings.connect_title")} }
                span { class: "app-settings-option-hint", {i18n.t("app_settings.connect_hint")} }

                if let Some(token) = token {
                    div { class: "warning-msg", {i18n.t("app_settings.token_warning")} }
                    CopyField {
                        label: i18n.t("app_settings.token_label"),
                        value: token,
                        multiline: false,
                    }
                    span { class: "app-settings-option-hint", {i18n.t("app_settings.token_hint")} }
                }

                CopyField {
                    label: i18n.t("app_settings.rest_base_label"),
                    value: rest_url,
                    multiline: false,
                }
                CopyField {
                    label: i18n.t("app_settings.rest_example_label"),
                    value: rest_example,
                    multiline: false,
                }
                if graphql {
                    CopyField {
                        label: i18n.t("app_settings.graphql_endpoint_label"),
                        value: graphql_url,
                        multiline: false,
                    }
                    CopyField {
                        label: i18n.t("app_settings.graphql_example_label"),
                        value: graphql_example,
                        multiline: true,
                    }
                }
            }

            AssistantCard {}
        }
    }
}

/// One row of the endpoint list: opens `href` in the system browser.
#[component]
fn ApiEndpointLink(href: String, label: String, hint: String, title: String) -> Element {
    rsx! {
        a {
            class: "api-endpoint",
            href: href.clone(),
            target: "_blank",
            rel: "noopener noreferrer",
            title: "{title}",
            div { class: "api-endpoint-info",
                span { class: "app-settings-option-label", "{label}" }
                span { class: "app-settings-option-hint", "{hint}" }
                code { class: "api-endpoint-url", "{href}" }
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

/// The minimal GraphQL operation the connection example sends: the first
/// tree's identifier and name, valid against the executable schema.
const GRAPHQL_EXAMPLE_QUERY: &str = "{ trees(first: 1) { edges { node { id name } } } }";

/// A `curl` command fetching the OpenAPI document, which every backend serves
/// without a token.
fn openapi_curl_example(openapi_url: &str) -> String {
    format!("curl '{openapi_url}'")
}

/// A `curl` command posting [`GRAPHQL_EXAMPLE_QUERY`] to `graphql_url`. Only
/// the web server serves GraphQL, and it asks for no token.
///
/// POSIX shell quoting: the body is single-quoted JSON, so the query must hold
/// no single quote, which [`GRAPHQL_EXAMPLE_QUERY`] does not.
fn graphql_curl_example(graphql_url: &str) -> String {
    let body = serde_json::json!({ "query": GRAPHQL_EXAMPLE_QUERY });
    [
        format!("curl -X POST '{graphql_url}'"),
        "  -H 'Content-Type: application/json'".to_string(),
        format!("  -d '{body}'"),
    ]
    .join(" \\\n")
}

// ── AI assistant (MCP) ──────────────────────────────────────────────────────

/// The "AI assistant (MCP)" card, following the API endpoints card in the
/// same section (`docs/ui-app-settings.md` §8).
///
/// Desktop-only: an MCP client needs the absolute path of the running
/// executable, which only exists once there is a running executable to
/// report. The desktop binary injects it as an [`AssistantLauncher`]; the web
/// build provides none and this shows a note instead of a command that could
/// not run.
#[component]
fn AssistantCard() -> Element {
    let i18n = use_i18n();
    let launcher = use_assistant_launcher();

    rsx! {
        div { class: "app-settings-card copy-card",
            span { class: "app-settings-option-label", {i18n.t("app_settings.assistant_title")} }

            match launcher {
                Some(launcher) => rsx! {
                    AssistantLauncherPanel { launcher }
                },
                None => rsx! {
                    p { class: "app-settings-option-hint",
                        {i18n.t("app_settings.assistant_desktop_only")}
                    }
                },
            }
        }
    }
}

/// The warning, the command, and the JSON client configuration, once an
/// [`AssistantLauncher`] is available.
///
/// The warning is always visible above the command, not behind a disclosure:
/// configuring the client is the consent, so the cost of that consent has to
/// be read before the command that grants it, not discovered after.
#[component]
fn AssistantLauncherPanel(launcher: AssistantLauncher) -> Element {
    let i18n = use_i18n();

    rsx! {
        div { class: "warning-msg", {i18n.t("app_settings.assistant_warning")} }

        CopyField {
            label: i18n.t("app_settings.assistant_command_label"),
            value: launcher.command_line(),
            multiline: false,
        }
        CopyField {
            label: i18n.t("app_settings.assistant_config_label"),
            value: launcher.client_config_json(),
            multiline: true,
        }
    }
}

// ── Styles ──────────────────────────────────────────────────────────────────

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
        gap: var(--space-8);
        padding: var(--space-8) var(--space-10);
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
        gap: var(--space-2);
    }

    .api-endpoint-url {
        margin-top: var(--space-2);
        color: var(--orange);
        font-size: var(--text-80);
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

    /* ── Connection details and AI assistant (MCP) ─────────────────
       Cards below the endpoint list rather than rows inside it: the
       endpoints are one link each, while connecting a client or the
       assistant takes a warning plus several whole fields, and folding
       them into the same list would make its shortest rows the tallest. */
    .copy-card {
        margin-top: var(--space-8);
        display: flex;
        flex-direction: column;
        gap: var(--space-7);
    }

"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_openapi_example_needs_no_token() {
        assert_eq!(
            openapi_curl_example("http://127.0.0.1:8080/api/v1/openapi.json"),
            "curl 'http://127.0.0.1:8080/api/v1/openapi.json'"
        );
    }

    #[test]
    fn the_graphql_example_posts_the_query_as_json() {
        let example = graphql_curl_example("http://127.0.0.1:8080/graphql");
        assert_eq!(
            example,
            "curl -X POST 'http://127.0.0.1:8080/graphql' \\\n  \
             -H 'Content-Type: application/json' \\\n  \
             -d '{\"query\":\"{ trees(first: 1) { edges { node { id name } } } }\"}'"
        );
        assert!(!GRAPHQL_EXAMPLE_QUERY.contains('\''));
    }
}
