//! The settings page (`/settings`, docs/ui-app-settings.md): the device's
//! preferences (theme, language, pedigree, name display, date display, entry
//! options, API access) and, in the web build's `oidc` mode, the signed-in
//! user's account and an administrator's instance.

use dioxus::prelude::*;
use oxidgene_core::enums::{Calendar, DateDisplayFormat, DateInputFormat, DateQualifier};
use oxidgene_core::types::QualifiedYear;

use crate::account::{AccountState, set_me, use_account};
use crate::api::ApiClient;
use crate::api::account::ProfilePatch;
use crate::assistant::{AssistantLauncher, use_assistant_launcher};
use crate::components::breadcrumb::TreeBreadcrumb;
use crate::components::copy_field::CopyField;
use crate::components::date_input::{
    calendar_from_value, calendar_value, format_date, format_example, input_format_label,
};
use crate::components::pedigree_chart::{PedigreeViewSwatch, format_lifespan};
use crate::components::pedigree_theme::{CardFrame, LinkSpec, PedigreeThemeId, Point, link_path};
use crate::components::pedigree_view::PedigreeView;
use crate::i18n::{
    self, CustomLanguageLoader, DateStyle, Language, LanguageCatalog, reload_custom_languages,
    use_i18n,
};
use crate::pages::account_settings::{
    AccountSection, NotificationsSection, PROFILE_STYLES, ProfileSection, SessionsSection,
};
use crate::prefs::{
    EntryOptions, PedigreeDefaults, SortParticles, set_date_style, set_entry_options,
    set_pedigree_defaults, set_pedigree_theme, set_pedigree_view, set_sort_particles,
    use_entry_options,
};
use crate::router::Route;
use crate::theme::{CustomThemeLoader, Theme, ThemeState, reload_custom_themes, set_theme};
use crate::ui_observability::{UiPage, use_ui_load_trace};

/// Scrolls the settings content back to its top whenever `section`, the
/// section on screen, changes.
pub(crate) fn use_settings_scroll_reset<T: Clone + PartialEq + 'static>(section: T) {
    use_effect(use_reactive!(|section| {
        let _section = section;
        document::eval(
            r#"
            requestAnimationFrame(() => {
                const content = document.querySelector('.settings-layout')
                    ?.closest('.sub-page-content');
                if (content) content.scrollTop = 0;
            });
            "#,
        );
    }));
}

/// The sections of the left navigation, each named in the page's address by
/// its [`Section::id`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Profile,
    Notifications,
    Sessions,
    Account,
    Appearance,
    Language,
    Pedigree,
    Names,
    DateDisplay,
    EntryOptions,
    Api,
    /// The instance's users: administrators of the web build only.
    Users,
}

impl Section {
    /// The signed-in user's account: the web build's `oidc` mode only,
    /// below the preferences, which stay as the desktop has them.
    const ACCOUNT: [Self; 4] = [
        Self::Profile,
        Self::Notifications,
        Self::Sessions,
        Self::Account,
    ];
    /// The device's preferences, open to everyone.
    const PREFERENCES: [Self; 7] = [
        Self::Appearance,
        Self::Language,
        Self::Pedigree,
        Self::Names,
        Self::DateDisplay,
        Self::EntryOptions,
        Self::Api,
    ];
    /// The instance: its administrators only.
    const INSTANCE: [Self; 1] = [Self::Users];

    /// What the page's `section` query names it.
    fn id(self) -> &'static str {
        match self {
            Self::Profile => "profile",
            Self::Notifications => "notifications",
            Self::Sessions => "sessions",
            Self::Account => "account",
            Self::Appearance => "appearance",
            Self::Language => "language",
            Self::Pedigree => "pedigree",
            Self::Names => "names",
            Self::DateDisplay => "date-display",
            Self::EntryOptions => "entry-options",
            Self::Api => "api",
            Self::Users => "users",
        }
    }

    fn from_id(id: &str) -> Option<Self> {
        [
            Self::ACCOUNT.as_slice(),
            &Self::PREFERENCES,
            &Self::INSTANCE,
        ]
        .concat()
        .into_iter()
        .find(|section| section.id() == id)
    }

    fn label_key(self) -> &'static str {
        match self {
            Self::Profile => "profile.section_profile",
            Self::Notifications => "profile.section_notifications",
            Self::Sessions => "profile.section_sessions",
            Self::Account => "profile.section_account",
            Self::Appearance => "app_settings.appearance",
            Self::Language => "app_settings.language",
            Self::Pedigree => "app_settings.pedigree",
            Self::Names => "app_settings.names",
            Self::DateDisplay => "app_settings.date_display",
            Self::EntryOptions => "app_settings.entry_options",
            Self::Api => "app_settings.api",
            Self::Users => "instance.users.title",
        }
    }

    /// Whether only a signed-in user has the section.
    fn needs_account(self) -> bool {
        Self::ACCOUNT.contains(&self) || Self::INSTANCE.contains(&self)
    }
}

/// Whether the settings opened on `section` need a signed-in user: an
/// account or instance section does, the device's preferences do not
/// ([`crate::account::gate`]).
pub(crate) fn needs_account(section: &str) -> bool {
    Section::from_id(section).is_some_and(Section::needs_account)
}

/// The settings, open on their first section.
#[component]
pub fn AppSettings() -> Element {
    let _load_trace = use_ui_load_trace(UiPage::AppSettings);
    rsx! { SettingsPage { section: None } }
}

/// The settings, open on the section `section` names.
#[component]
pub fn AppSettingsSection(section: String) -> Element {
    let _load_trace = use_ui_load_trace(UiPage::AppSettings);
    rsx! { SettingsPage { section } }
}

#[component]
fn SettingsPage(section: Option<String>) -> Element {
    let i18n = use_i18n();
    let theme_state = use_context::<Signal<ThemeState>>();
    let lang_signal = use_context::<Signal<Language>>();
    let sort_particles = use_context::<Signal<SortParticles>>();
    let pedigree_defaults = use_context::<Signal<Option<PedigreeDefaults>>>();
    let nav = navigator();

    let account = use_account();
    let (me, account_url) = match &*account.read() {
        AccountState::SignedIn(signed_in) => {
            (Some(signed_in.me.clone()), signed_in.account_url.clone())
        }
        _ => (None, None),
    };
    let is_admin = account.read().is_admin();
    // A section the user does not have — an account's without one, the
    // instance's for anyone but its administrators, a name the page does
    // not know — opens the first of the preferences.
    let has = |section: Section| match section {
        Section::Users => is_admin,
        section if section.needs_account() => me.is_some(),
        _ => true,
    };
    let active = section
        .as_deref()
        .and_then(Section::from_id)
        .filter(|section| has(*section))
        .unwrap_or(Section::Appearance);
    use_settings_scroll_reset(active);

    // The section lives in the address, which a link to another section of
    // the open page changes too. Replaced rather than pushed: going back
    // leaves the settings rather than stepping through their sections.
    let item = move |section: Section| {
        rsx! {
            button {
                key: "{section.id()}",
                class: if active == section { "settings-nav-item active" } else { "settings-nav-item" },
                onclick: move |_| {
                    nav.replace(Route::AppSettingsSection {
                        section: section.id().to_string(),
                    });
                },
                {i18n.t(section.label_key())}
            }
        }
    };

    rsx! {
        style { {APP_SETTINGS_STYLES} }
        if me.is_some() {
            style { {PROFILE_STYLES} }
        }

        div { class: "sub-page",
            div { class: "td-topbar",
                TreeBreadcrumb {
                    span { class: "td-bc-current", {i18n.t("app_settings.title")} }
                }
            }

            div { class: "sub-page-content",
                div { class: "settings-layout",
                    nav { class: "settings-nav",
                        div { class: "settings-nav-group",
                            span { class: "settings-nav-group-label",
                                {i18n.t("app_settings.preferences")}
                            }
                            for section in Section::PREFERENCES {
                                {item(section)}
                            }
                        }
                        if me.is_some() {
                            div { class: "settings-nav-group",
                                span { class: "settings-nav-group-label",
                                    {i18n.t("app_settings.group_account")}
                                }
                                for section in Section::ACCOUNT {
                                    {item(section)}
                                }
                            }
                        }
                        if is_admin {
                            div { class: "settings-nav-group",
                                span { class: "settings-nav-group-label",
                                    {i18n.t("instance.title")}
                                }
                                for section in Section::INSTANCE {
                                    {item(section)}
                                }
                            }
                        }
                    }

                    div { class: "settings-content",
                        match (active, me) {
                            (Section::Profile, Some(me)) => rsx! { ProfileSection { me } },
                            (Section::Notifications, Some(_)) => rsx! { NotificationsSection {} },
                            (Section::Sessions, Some(_)) => rsx! { SessionsSection {} },
                            (Section::Account, Some(_)) => rsx! { AccountSection { account_url } },
                            (Section::Appearance, _) => rsx! { AppearanceSection { theme_state } },
                            (Section::Language, _) => rsx! { LanguageSection { lang_signal } },
                            (Section::Pedigree, _) => rsx! {
                                PedigreeDefaultsSection { pedigree_defaults }
                            },
                            (Section::Names, _) => rsx! { NamesSection { sort_particles } },
                            (Section::DateDisplay, _) => rsx! { DateDisplaySection {} },
                            (Section::EntryOptions, _) => rsx! { EntryOptionsSection {} },
                            (Section::Api, _) => rsx! { ApiSection {} },
                            (Section::Users, _) => rsx! {
                                crate::pages::instance_users::InstanceUsersSection {}
                            },
                            // `has` keeps an account's sections for a signed-in user.
                            (Section::Profile | Section::Notifications | Section::Sessions | Section::Account, None) => rsx! {},
                        }
                    }
                }
            }
        }
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

/// The interface language. Chosen while signed in, it is the account's
/// language too (docs/authentication.md §10): OxidGene writes to the user in
/// it, and a device where none was chosen opens in it.
#[component]
pub fn LanguageSection(lang_signal: Signal<Language>) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let account = use_account();
    let mut account_error = use_signal(|| None::<String>);
    let current = *lang_signal.read();
    let account_locale = account.read().me().map(|me| me.locale.clone());
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
                            onclick: {
                                let (api, account_locale) = (api.clone(), account_locale.clone());
                                move |_| {
                                    i18n::set_language(lang_signal, lang);
                                    // A language of the device's own files
                                    // is no account's.
                                    let Some(locale) = account_locale.clone() else { return };
                                    if locale.as_deref() == Some(lang.code())
                                        || !Language::builtins().contains(&lang)
                                    {
                                        return;
                                    }
                                    let api = api.clone();
                                    spawn(async move {
                                        account_error.set(None);
                                        let patch = ProfilePatch {
                                            display_name: None,
                                            locale: Some(Some(lang.code().to_string())),
                                        };
                                        match api.update_me(&patch).await {
                                            Ok(me) => set_me(account, me),
                                            Err(_) => account_error.set(Some(i18n.t("profile.save_failed"))),
                                        }
                                    });
                                }
                            },
                            span { class: "lang-option-flag", {lang.flag()} }
                            span { class: "lang-option-label", {lang.native_name()} }
                            if current == lang {
                                span { class: "lang-option-check", "\u{2713}" }
                            }
                        }
                    }
                }
                if account_locale.is_some() {
                    p { class: "app-settings-option-hint",
                        {i18n.t("app_settings.language_account_hint")}
                    }
                }
                if let Some(error) = account_error() {
                    div { class: "error-msg", role: "alert", "{error}" }
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
/// connects to it (`docs/ui-app-settings.md` §10).
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
/// same section (`docs/ui-app-settings.md` §10).
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
        // Writes are a second consent, given by editing the client's
        // configuration: no writing command is offered to copy.
        p { class: "app-settings-option-hint", {i18n.t("app_settings.assistant_writes_hint")} }
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

// ── Date display section ────────────────────────────────────────────────────

/// How every page writes dates (`docs/ui-app-settings.md` §8): the format
/// with its preview, the event symbols, « circa », and the calendar other ones
/// are also given in. A preference of the device, like the theme: each choice
/// applies at once and nothing reaches the server.
#[component]
pub fn DateDisplaySection() -> Element {
    let i18n = use_i18n();
    let pref = use_context::<Signal<DateStyle>>();
    let style = *pref.read();

    rsx! {
        div { class: "settings-section",
            span { class: "settings-section-eyebrow", {i18n.t("app_settings.date_display")} }
            h2 { class: "settings-section-title", {i18n.t("app_settings.date_display")} }
            p { class: "settings-section-subtitle", {i18n.t("app_settings.date_display_desc")} }

            div { class: "app-settings-card",
                div { class: "app-settings-option app-settings-option-stacked",
                    div { class: "app-settings-option-info",
                        span { class: "app-settings-option-label", {i18n.t("app_settings.date_format")} }
                        span { class: "app-settings-option-hint", {i18n.t("app_settings.date_format_desc")} }
                    }
                    select {
                        class: "settings-date-select",
                        aria_label: i18n.t("app_settings.date_format"),
                        onchange: move |e: Event<FormData>| {
                            let value = e.value();
                            if let Some(format) = DateDisplayFormat::ALL
                                .into_iter()
                                .find(|format| format.as_str() == value)
                            {
                                set_date_style(pref, DateStyle { format, ..style });
                            }
                        },
                        for format in DateDisplayFormat::ALL {
                            option {
                                value: format.as_str(),
                                selected: style.format == format,
                                {format_example(&i18n, format)}
                            }
                        }
                    }
                    DatePreview { style }
                }
                YesNoOption {
                    label: i18n.t("app_settings.date_symbols"),
                    hint: i18n.t("app_settings.date_symbols_desc"),
                    value: style.symbols,
                    on_change: move |symbols| set_date_style(pref, DateStyle { symbols, ..style }),
                }
                YesNoOption {
                    label: i18n.t("app_settings.date_circa"),
                    hint: i18n.t("app_settings.date_circa_desc"),
                    value: style.circa,
                    on_change: move |circa| set_date_style(pref, DateStyle { circa, ..style }),
                }
                div { class: "app-settings-option",
                    div { class: "app-settings-option-info",
                        span { class: "app-settings-option-label", {i18n.t("app_settings.date_calendar")} }
                        span { class: "app-settings-option-hint", {i18n.t("app_settings.date_calendar_desc")} }
                    }
                    CalendarSelect {
                        label: i18n.t("app_settings.date_calendar"),
                        value: style.calendar,
                        on_change: move |calendar| set_date_style(pref, DateStyle { calendar, ..style }),
                    }
                }
            }
        }
    }
}

/// A fictitious person's dates as the pages would write them in `style`: a
/// birth recorded in the Republican calendar, a marriage, a death known only
/// roughly, and the lifespan a card would draw.
#[component]
fn DatePreview(style: DateStyle) -> Element {
    let i18n = use_i18n();
    let sample = i18n.with_dates(style);
    let rows = [
        (
            i18n.t("event.type.birth"),
            format_date(
                &sample,
                Calendar::FrenchRepublican,
                DateQualifier::Exact,
                Some("18 BRUM 8"),
                None,
            ),
        ),
        (
            i18n.t("event.type.marriage"),
            format_date(
                &sample,
                Calendar::Gregorian,
                DateQualifier::Exact,
                Some("12 MAR 1822"),
                None,
            ),
        ),
        (
            i18n.t("event.type.death"),
            format_date(
                &sample,
                Calendar::Gregorian,
                DateQualifier::About,
                Some("1867"),
                None,
            ),
        ),
        (
            i18n.t("app_settings.date_preview_lifespan"),
            format_lifespan(
                style,
                Some(QualifiedYear::new(1799, DateQualifier::Exact)),
                Some(QualifiedYear::new(1867, DateQualifier::About)),
            ),
        ),
    ];
    rsx! {
        div { class: "settings-date-preview", aria_live: "polite",
            div { class: "settings-date-preview-title", {i18n.t("app_settings.date_preview")} }
            dl {
                for (label , text) in rows {
                    dt { "{label}" }
                    dd { "{text}" }
                }
            }
        }
    }
}

/// One of the calendars, picked from a dropdown named `label`.
#[component]
fn CalendarSelect(label: String, value: Calendar, on_change: EventHandler<Calendar>) -> Element {
    let i18n = use_i18n();
    rsx! {
        select {
            class: "settings-date-select",
            aria_label: "{label}",
            onchange: move |e: Event<FormData>| on_change.call(calendar_from_value(&e.value())),
            for calendar in [
                Calendar::Gregorian,
                Calendar::Julian,
                Calendar::FrenchRepublican,
                Calendar::Hebrew,
            ] {
                option {
                    value: calendar_value(calendar),
                    selected: value == calendar,
                    {i18n.t(&format!("calendar.{calendar}"))}
                }
            }
        }
    }
}

/// A preference that is on or off: its name, what it does, and Yes / No.
#[component]
fn YesNoOption(label: String, hint: String, value: bool, on_change: EventHandler<bool>) -> Element {
    let i18n = use_i18n();
    rsx! {
        div { class: "app-settings-option",
            div { class: "app-settings-option-info",
                span { class: "app-settings-option-label", "{label}" }
                span { class: "app-settings-option-hint", "{hint}" }
            }
            div { class: "theme-toggle-group", role: "group", aria_label: "{label}",
                for (choice , text) in [(true, i18n.t("common.yes")), (false, i18n.t("common.no"))] {
                    button {
                        key: "{choice}",
                        r#type: "button",
                        class: if value == choice { "theme-toggle-btn active" } else { "theme-toggle-btn" },
                        aria_pressed: if value == choice { "true" } else { "false" },
                        onclick: move |_| on_change.call(choice),
                        "{text}"
                    }
                }
            }
        }
    }
}

// ── Entry options section ───────────────────────────────────────────────────

/// How the forms help with entry (`docs/ui-app-settings.md` §9):
/// suggestions, surnames in capitals, existing persons offered, and how dates
/// are entered. A preference of the device, followed by the forms of every
/// tree.
#[component]
pub fn EntryOptionsSection() -> Element {
    let i18n = use_i18n();
    let pref = use_entry_options();
    let options = *pref.read();

    rsx! {
        div { class: "settings-section",
            span { class: "settings-section-eyebrow", {i18n.t("app_settings.entry_options")} }
            h2 { class: "settings-section-title", {i18n.t("app_settings.entry_options")} }
            p { class: "settings-section-subtitle", {i18n.t("app_settings.entry_options_desc")} }

            div { class: "app-settings-card",
                YesNoOption {
                    label: i18n.t("app_settings.entry_suggestions"),
                    hint: i18n.t("app_settings.entry_suggestions_desc"),
                    value: options.suggestions,
                    on_change: move |suggestions| set_entry_options(
                        pref,
                        EntryOptions { suggestions, ..options },
                    ),
                }
                YesNoOption {
                    label: i18n.t("app_settings.surname_uppercase"),
                    hint: i18n.t("app_settings.surname_uppercase_desc"),
                    value: options.surname_uppercase,
                    on_change: move |surname_uppercase| set_entry_options(
                        pref,
                        EntryOptions { surname_uppercase, ..options },
                    ),
                }
                YesNoOption {
                    label: i18n.t("app_settings.suggest_persons"),
                    hint: i18n.t("app_settings.suggest_persons_desc"),
                    value: options.suggest_persons,
                    on_change: move |suggest_persons| set_entry_options(
                        pref,
                        EntryOptions { suggest_persons, ..options },
                    ),
                }
                div { class: "app-settings-option",
                    div { class: "app-settings-option-info",
                        span { class: "app-settings-option-label", {i18n.t("app_settings.input_date_format")} }
                        span { class: "app-settings-option-hint", {i18n.t("app_settings.input_date_format_desc")} }
                    }
                    select {
                        class: "settings-date-select",
                        aria_label: i18n.t("app_settings.input_date_format"),
                        onchange: move |e: Event<FormData>| {
                            let value = e.value();
                            if let Some(date_format) = DateInputFormat::ALL
                                .into_iter()
                                .find(|format| format.as_str() == value)
                            {
                                set_entry_options(pref, EntryOptions { date_format, ..options });
                            }
                        },
                        for format in DateInputFormat::ALL {
                            option {
                                value: format.as_str(),
                                selected: options.date_format == format,
                                {input_format_label(&i18n, format)}
                            }
                        }
                    }
                }
                div { class: "app-settings-option",
                    div { class: "app-settings-option-info",
                        span { class: "app-settings-option-label", {i18n.t("app_settings.input_calendar")} }
                        span { class: "app-settings-option-hint", {i18n.t("app_settings.input_calendar_desc")} }
                    }
                    CalendarSelect {
                        label: i18n.t("app_settings.input_calendar"),
                        value: options.calendar,
                        on_change: move |calendar| set_entry_options(
                            pref,
                            EntryOptions { calendar, ..options },
                        ),
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
    fn every_section_has_an_address_and_a_label() {
        let english = Language::english().translations();
        for section in [
            Section::ACCOUNT.as_slice(),
            &Section::PREFERENCES,
            &Section::INSTANCE,
        ]
        .concat()
        {
            assert_eq!(Section::from_id(section.id()), Some(section));
            assert!(english.contains_key(section.label_key()), "{section:?}");
        }
        assert_eq!(Section::from_id("nowhere"), None);
    }

    #[test]
    fn only_the_account_and_the_instance_need_an_account() {
        assert!(needs_account("profile"));
        assert!(needs_account("notifications"));
        assert!(needs_account("users"));
        assert!(!needs_account("appearance"));
        assert!(!needs_account("nowhere"));
    }

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
