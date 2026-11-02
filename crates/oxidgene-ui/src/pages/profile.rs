//! The signed-in user's profile (`/profile`, docs/authentication.md §10):
//! web build only, in `oidc` mode. Its sections:
//! the display name and avatar, the account's language, the notification
//! preferences, the sessions, and the account's data and deletion; API
//! clients, messaging security and blocked users join it with their phases.
//!
//! On the desktop and in `none` mode there is no account, and the address
//! is not found.

use dioxus::prelude::*;
use oxidgene_core::notification::NotificationKind;

use crate::account::{AccountState, set_me, sign_out, use_account, use_accounts};
use crate::api::account::{AvatarCrop, Me, ProfilePatch, SessionEntry};
use crate::api::notification::{KindPreference, NotificationPreferences, PreferencesPatch};
use crate::api::{ApiClient, ApiError};
use crate::components::avatar_cropper::AvatarCropper;
use crate::components::breadcrumb::TreeBreadcrumb;
use crate::components::confirm_dialog::ConfirmDialog;
use crate::components::history_diff::format_timestamp;
use crate::components::user_avatar::{AvatarSize, UserAvatar};
use crate::i18n::{self, I18n, Language, use_i18n};
use crate::pages::app_settings::use_settings_scroll_reset;
use crate::pages::not_found::NotFound;
use crate::ui_observability::{UiPage, use_ui_load_trace, use_ui_resource};

/// The largest avatar file the server takes (docs/authentication.md §6.7).
const MAX_AVATAR_BYTES: usize = 5 * 1024 * 1024;

/// The sections of the left navigation.
#[derive(Clone, Copy, PartialEq)]
enum Section {
    Profile,
    Language,
    Notifications,
    Sessions,
    Account,
}

impl Section {
    const ALL: [Self; 5] = [
        Self::Profile,
        Self::Language,
        Self::Notifications,
        Self::Sessions,
        Self::Account,
    ];

    fn label_key(self) -> &'static str {
        match self {
            Self::Profile => "profile.section_profile",
            Self::Language => "profile.section_language",
            Self::Notifications => "profile.section_notifications",
            Self::Sessions => "profile.section_sessions",
            Self::Account => "profile.section_account",
        }
    }
}

#[component]
pub fn Profile() -> Element {
    let _load_trace = use_ui_load_trace(UiPage::Profile);
    let account = use_account();
    let i18n = use_i18n();
    let mut active = use_signal(|| Section::Profile);
    use_settings_scroll_reset(active);

    let (me, account_url) = match &*account.read() {
        AccountState::SignedIn(signed_in) => (signed_in.me.clone(), signed_in.account_url.clone()),
        // The server has not said yet whether anyone is signed in.
        AccountState::Loading => return rsx! {},
        // No account here: the desktop, or the web build in `none` mode.
        AccountState::Absent | AccountState::Anonymous => {
            return rsx! {
                NotFound { segments: vec!["profile".to_string()] }
            };
        }
    };

    rsx! {
        style { {PROFILE_STYLES} }
        div { class: "sub-page",
            div { class: "td-topbar",
                TreeBreadcrumb {
                    span { class: "td-bc-current", {i18n.t("profile.title")} }
                }
            }
            div { class: "sub-page-content",
                div { class: "settings-layout",
                    nav { class: "settings-nav",
                        div { class: "settings-nav-group",
                            span { class: "settings-nav-group-label", {i18n.t("profile.title")} }
                            for section in Section::ALL {
                                button {
                                    key: "{section.label_key()}",
                                    class: if active() == section { "settings-nav-item active" } else { "settings-nav-item" },
                                    onclick: move |_| active.set(section),
                                    {i18n.t(section.label_key())}
                                }
                            }
                        }
                    }
                    div { class: "settings-content",
                        match active() {
                            Section::Profile => rsx! { ProfileSection { me } },
                            Section::Language => rsx! { AccountLanguageSection { me } },
                            Section::Notifications => rsx! { NotificationsSection {} },
                            Section::Sessions => rsx! { SessionsSection {} },
                            Section::Account => rsx! { AccountSection { account_url } },
                        }
                    }
                }
            }
        }
    }
}

/// The section's eyebrow, title and subtitle.
#[component]
fn SectionHeader(eyebrow: String, title: String, subtitle: String) -> Element {
    rsx! {
        span { class: "settings-section-eyebrow", "{eyebrow}" }
        h2 { class: "settings-section-title", "{title}" }
        p { class: "settings-section-subtitle", "{subtitle}" }
    }
}

/// What a failed account request says to the user.
fn failure(i18n: &I18n, error: &ApiError) -> String {
    match error.status() {
        Some(400) => i18n.t("profile.invalid_name"),
        Some(409) => i18n.t("profile.delete_owns_trees"),
        _ => i18n.t("profile.save_failed"),
    }
}

// ── Profile: avatar, display name, email ───────────────────────────────────

/// A picture chosen for the avatar, before it is cut and sent.
#[derive(Clone, PartialEq)]
struct PickedImage {
    file_name: String,
    bytes: Vec<u8>,
    data_url: String,
}

/// The picture's type, read from its first bytes as the server reads it:
/// JPEG, PNG or WebP, the three an avatar may be.
fn sniff_image(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// The picture the user chose, or why it cannot be an avatar; `Ok(None)`
/// when they chose nothing.
async fn pick_image(i18n: I18n) -> Result<Option<PickedImage>, String> {
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_title(i18n.t("profile.avatar_upload"))
        .add_filter(
            i18n.t("profile.avatar_file_kind"),
            &["jpg", "jpeg", "png", "webp"],
        )
        .pick_file()
        .await
    else {
        return Ok(None);
    };
    let bytes = file.read().await;
    if bytes.len() > MAX_AVATAR_BYTES {
        return Err(i18n.t("profile.avatar_too_large"));
    }
    let Some(mime) = sniff_image(&bytes) else {
        return Err(i18n.t("profile.avatar_wrong_type"));
    };
    use base64::Engine as _;
    let data_url = format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    );
    Ok(Some(PickedImage {
        file_name: file.file_name(),
        bytes,
        data_url,
    }))
}

#[component]
fn ProfileSection(me: Me) -> Element {
    let i18n = use_i18n();
    rsx! {
        div { class: "settings-section",
            SectionHeader {
                eyebrow: i18n.t("profile.section_profile"),
                title: i18n.t("profile.profile_title"),
                subtitle: i18n.t("profile.profile_desc"),
            }
            div { class: "app-settings-card",
                AvatarOption { me: me.clone() }
                DisplayNameOption { me: me.clone() }
                EmailOption { me }
            }
        }
    }
}

/// The avatar with Upload, which opens the crop dialog, and Remove.
#[component]
fn AvatarOption(me: Me) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let account = use_account();
    let mut picked = use_signal(|| None::<PickedImage>);
    let mut uploading = use_signal(|| false);
    let mut avatar_error = use_signal(|| None::<String>);

    let choose = move |_| {
        spawn(async move {
            avatar_error.set(None);
            match pick_image(i18n).await {
                Ok(image) => picked.set(image),
                Err(error) => avatar_error.set(Some(error)),
            }
        });
    };
    let upload = {
        let api = api.clone();
        move |crop: AvatarCrop| {
            let Some(image) = picked() else { return };
            let api = api.clone();
            spawn(async move {
                uploading.set(true);
                avatar_error.set(None);
                match api.upload_avatar(image.file_name, image.bytes, crop).await {
                    Ok(me) => {
                        set_me(account, me);
                        picked.set(None);
                    }
                    Err(_) => avatar_error.set(Some(i18n.t("profile.avatar_failed"))),
                }
                uploading.set(false);
            });
        }
    };
    let remove = move |_| {
        let api = api.clone();
        spawn(async move {
            uploading.set(true);
            avatar_error.set(None);
            let removed = match api.delete_avatar().await {
                Ok(()) => api.me().await,
                Err(error) => Err(error),
            };
            match removed {
                Ok(me) => set_me(account, me),
                Err(_) => avatar_error.set(Some(i18n.t("profile.avatar_failed"))),
            }
            uploading.set(false);
        });
    };

    let avatar = me.avatar.as_ref().map(|avatar| avatar.etag.clone());
    let has_avatar = avatar.is_some();

    rsx! {
        div { class: "app-settings-option",
            div { class: "profile-avatar-row",
                UserAvatar {
                    user_id: me.id,
                    name: me.display_name.clone(),
                    avatar,
                    size: AvatarSize::Large,
                }
                div { class: "app-settings-option-info",
                    span { class: "app-settings-option-label", {i18n.t("profile.avatar")} }
                    span { class: "app-settings-option-hint", {i18n.t("profile.avatar_hint")} }
                }
            }
            div { class: "profile-actions",
                button {
                    class: "btn btn-outline",
                    r#type: "button",
                    disabled: uploading(),
                    onclick: choose,
                    {i18n.t("profile.avatar_upload")}
                }
                if has_avatar {
                    button {
                        class: "btn btn-outline",
                        r#type: "button",
                        disabled: uploading(),
                        onclick: remove,
                        {i18n.t("profile.avatar_remove")}
                    }
                }
            }
        }
        if let (Some(error), None) = (avatar_error(), picked()) {
            div { class: "error-msg", role: "alert", "{error}" }
        }
        if let Some(image) = picked() {
            AvatarCropper {
                image: image.data_url.clone(),
                busy: uploading(),
                error: avatar_error(),
                on_confirm: upload,
                on_cancel: move |()| {
                    picked.set(None);
                    avatar_error.set(None);
                },
            }
        }
    }
}

/// The display name with Save, and the way back to the provider's name.
#[component]
fn DisplayNameOption(me: Me) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let account = use_account();
    // The field's own: what is typed stays until it is saved.
    let mut name = use_signal(|| me.display_name.clone().unwrap_or_default());
    let mut saving = use_signal(|| false);
    let mut message = use_signal(|| None::<Result<String, String>>);

    let save_name = move |display_name: Option<String>| {
        let api = api.clone();
        spawn(async move {
            saving.set(true);
            message.set(None);
            let patch = ProfilePatch {
                display_name: Some(display_name),
                locale: None,
            };
            match api.update_me(&patch).await {
                Ok(me) => {
                    name.set(me.display_name.clone().unwrap_or_default());
                    set_me(account, me);
                    message.set(Some(Ok(i18n.t("profile.saved"))));
                }
                Err(error) => message.set(Some(Err(failure(&i18n, &error)))),
            }
            saving.set(false);
        });
    };
    let save_typed = {
        let save_name = save_name.clone();
        move || save_name(Some(name().trim().to_string()))
    };

    let typed = name().trim().to_string();
    let unchanged = Some(typed.as_str()) == me.display_name.as_deref();
    let provider_name = me
        .provider_name
        .clone()
        .filter(|provider| Some(provider.as_str()) != me.display_name.as_deref());

    rsx! {
        div { class: "app-settings-option app-settings-option-stacked",
            label { class: "app-settings-option-info", r#for: "profile-display-name",
                span { class: "app-settings-option-label", {i18n.t("profile.display_name")} }
                span { class: "app-settings-option-hint", {i18n.t("profile.display_name_hint")} }
            }
            form {
                class: "profile-name-form",
                onsubmit: move |event: Event<FormData>| {
                    event.prevent_default();
                    save_typed();
                },
                input {
                    id: "profile-display-name",
                    r#type: "text",
                    maxlength: "80",
                    value: "{name}",
                    oninput: move |event: Event<FormData>| name.set(event.value()),
                }
                button {
                    class: "btn btn-primary",
                    r#type: "submit",
                    disabled: saving() || typed.is_empty() || unchanged,
                    {i18n.t("common.save")}
                }
            }
            if let Some(provider) = provider_name {
                div {
                    button {
                        class: "btn btn-outline btn-sm",
                        r#type: "button",
                        disabled: saving(),
                        onclick: move |_| save_name(None),
                        {i18n.t_args("profile.use_provider_name", &[("name", &provider)])}
                    }
                }
            }
            match message() {
                Some(Ok(text)) => rsx! { p { class: "profile-saved", role: "status", "{text}" } },
                Some(Err(text)) => rsx! { div { class: "error-msg", role: "alert", "{text}" } },
                None => rsx! {},
            }
        }
    }
}

/// The email as the provider gives it, read-only, with its verification.
#[component]
fn EmailOption(me: Me) -> Element {
    let i18n = use_i18n();
    let verification = me.email.as_ref().map(|_| {
        if me.email_verified {
            i18n.t("profile.email_verified")
        } else {
            i18n.t("profile.email_unverified")
        }
    });
    rsx! {
        div { class: "app-settings-option",
            div { class: "app-settings-option-info",
                span { class: "app-settings-option-label", {i18n.t("profile.email")} }
                span { class: "app-settings-option-hint", {i18n.t("profile.email_hint")} }
            }
            div { class: "profile-email",
                span { {me.email.clone().unwrap_or_else(|| i18n.t("profile.email_none"))} }
                if let Some(verification) = verification {
                    span { class: "badge", "{verification}" }
                }
            }
        }
    }
}

// ── Language ────────────────────────────────────────────────────────────────

#[component]
fn AccountLanguageSection(me: Me) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let account = use_account();
    let language = use_context::<Signal<Language>>();
    let mut error = use_signal(|| None::<String>);
    let mut saving = use_signal(|| false);
    let chosen = me.locale.as_deref().and_then(Language::try_from_code);

    rsx! {
        div { class: "settings-section",
            SectionHeader {
                eyebrow: i18n.t("profile.section_language"),
                title: i18n.t("profile.language_title"),
                subtitle: i18n.t("profile.language_desc"),
            }
            div { class: "app-settings-card app-settings-option app-settings-option-stacked",
                div { class: "lang-options",
                    for lang in Language::builtins() {
                        button {
                            key: "{lang.code()}",
                            class: if chosen == Some(lang) { "lang-option active" } else { "lang-option" },
                            aria_pressed: if chosen == Some(lang) { "true" } else { "false" },
                            disabled: saving(),
                            onclick: {
                                let api = api.clone();
                                move |_| {
                                    let api = api.clone();
                                    spawn(async move {
                                        saving.set(true);
                                        error.set(None);
                                        let patch = ProfilePatch {
                                            display_name: None,
                                            locale: Some(Some(lang.code().to_string())),
                                        };
                                        match api.update_me(&patch).await {
                                            Ok(me) => {
                                                set_me(account, me);
                                                // Chosen here, it is this device's choice too.
                                                i18n::set_language(language, lang);
                                            }
                                            Err(failed) => error.set(Some(failure(&i18n, &failed))),
                                        }
                                        saving.set(false);
                                    });
                                }
                            },
                            span { class: "lang-option-flag", {lang.flag()} }
                            span { class: "lang-option-label", {lang.native_name()} }
                            if chosen == Some(lang) {
                                span { class: "lang-option-check", "\u{2713}" }
                            }
                        }
                    }
                }
                if let Some(error) = error() {
                    div { class: "error-msg", role: "alert", "{error}" }
                }
            }
        }
    }
}

// ── Notifications ───────────────────────────────────────────────────────────

/// The preferences of docs/messaging.md §2.3: in the application always;
/// by email off, at once or in a daily summary, per kind, with one switch
/// for all email. A server that sends no mail hides the email choices.
#[component]
fn NotificationsSection() -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let mut error = use_signal(|| None::<String>);
    let mut current = use_signal(|| None::<NotificationPreferences>);
    let loaded = use_ui_resource("notification_preferences", {
        let api = api.clone();
        move || {
            let api = api.clone();
            async move { api.notification_preferences().await }
        }
    });
    if current.peek().is_none()
        && let Some(Ok(preferences)) = &*loaded.read()
    {
        current.set(Some(preferences.clone()));
    }
    let save = move |patch: PreferencesPatch| {
        let api = api.clone();
        spawn(async move {
            error.set(None);
            match api.update_notification_preferences(&patch).await {
                Ok(preferences) => current.set(Some(preferences)),
                Err(_) => error.set(Some(i18n.t("profile.save_failed"))),
            }
        });
    };

    rsx! {
        div { class: "settings-section",
            SectionHeader {
                eyebrow: i18n.t("profile.section_notifications"),
                title: i18n.t("notifications.settings_title"),
                subtitle: i18n.t("notifications.settings_desc"),
            }
            div { class: "app-settings-card",
                match (current(), &*loaded.read()) {
                    (Some(preferences), _) if !preferences.email_available => rsx! {
                        p { class: "app-settings-option-hint",
                            {i18n.t("notifications.settings_no_email")}
                        }
                    },
                    (Some(preferences), _) => rsx! {
                        label { class: "settings-check settings-check-first",
                            input {
                                r#type: "checkbox",
                                checked: preferences.email_enabled,
                                onchange: {
                                    let save = save.clone();
                                    move |e: Event<FormData>| save(PreferencesPatch {
                                        email_enabled: Some(e.checked()),
                                        kinds: Vec::new(),
                                    })
                                },
                            }
                            div {
                                div { class: "settings-check-label", {i18n.t("notifications.settings_email")} }
                                p { class: "settings-check-desc", {i18n.t("notifications.settings_email_desc")} }
                            }
                        }
                        for kind in NotificationKind::ALL {
                            div { key: "{kind.as_str()}", class: "app-settings-option",
                                div { class: "app-settings-option-info",
                                    span { class: "app-settings-option-label",
                                        {i18n.t(&format!("notifications.kind.{}", kind.as_str()))}
                                    }
                                }
                                select {
                                    class: "settings-date-select",
                                    aria_label: i18n.t(&format!("notifications.kind.{}", kind.as_str())),
                                    disabled: !preferences.email_enabled,
                                    onchange: {
                                        let save = save.clone();
                                        move |e: Event<FormData>| save(PreferencesPatch {
                                            email_enabled: None,
                                            kinds: vec![KindPreference { kind, email: e.value() }],
                                        })
                                    },
                                    for email in ["off", "immediate", "daily"] {
                                        option {
                                            key: "{email}",
                                            value: email,
                                            selected: preferences.kinds.iter().any(|p| p.kind == kind && p.email == email),
                                            {i18n.t(&format!("notifications.email.{email}"))}
                                        }
                                    }
                                }
                            }
                        }
                    },
                    (None, Some(Err(_))) => rsx! {
                        div { class: "error-msg", role: "alert", {i18n.t("notifications.settings_failed")} }
                    },
                    (None, _) => rsx! { p { class: "loading", {i18n.t("common.loading")} } },
                }
                if let Some(error) = error() {
                    div { class: "error-msg", role: "alert", "{error}" }
                }
            }
        }
    }
}

// ── Sessions ────────────────────────────────────────────────────────────────

/// The translation key of the browser family a session was opened in.
fn browser_key(browser: &str) -> &'static str {
    match browser {
        "firefox" => "profile.browser_firefox",
        "chrome" => "profile.browser_chrome",
        "edge" => "profile.browser_edge",
        "safari" => "profile.browser_safari",
        "opera" => "profile.browser_opera",
        _ => "profile.browser_other",
    }
}

#[component]
fn SessionsSection() -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let accounts = use_accounts();
    let mut refresh = use_signal(|| 0_u32);
    let mut error = use_signal(|| None::<String>);
    let sessions = use_ui_resource("my_sessions", {
        let api = api.clone();
        move || {
            let api = api.clone();
            let _ = refresh();
            async move { api.my_sessions().await }
        }
    });

    let end = {
        let (api, accounts) = (api.clone(), accounts.clone());
        move |session: SessionEntry| {
            let (api, accounts) = (api.clone(), accounts.clone());
            spawn(async move {
                error.set(None);
                match api.end_my_session(session.id).await {
                    // Ending this device's session signs it out.
                    Ok(()) if session.current => {
                        if let Some(accounts) = accounts {
                            accounts.navigate("/");
                        }
                    }
                    Ok(()) => refresh += 1,
                    Err(_) => error.set(Some(i18n.t("profile.session_end_failed"))),
                }
            });
        }
    };
    let everywhere = move |_| {
        let (api, accounts) = (api.clone(), accounts.clone());
        spawn(async move {
            error.set(None);
            let Some(accounts) = accounts else { return };
            if sign_out(&accounts, &api, true).await.is_err() {
                error.set(Some(i18n.t("profile.session_end_failed")));
            }
        });
    };

    rsx! {
        div { class: "settings-section",
            SectionHeader {
                eyebrow: i18n.t("profile.section_sessions"),
                title: i18n.t("profile.sessions_title"),
                subtitle: i18n.t("profile.sessions_desc"),
            }
            div { class: "app-settings-card profile-sessions",
                match &*sessions.read() {
                    None => rsx! { p { class: "loading", {i18n.t("common.loading")} } },
                    Some(Err(_)) => rsx! {
                        div { class: "error-msg", role: "alert", {i18n.t("profile.sessions_failed")} }
                    },
                    Some(Ok(list)) => rsx! {
                        for session in list.clone() {
                            SessionRow { key: "{session.id}", session, on_end: end.clone() }
                        }
                    },
                }
                if let Some(error) = error() {
                    div { class: "error-msg", role: "alert", "{error}" }
                }
            }
            div { class: "profile-actions profile-actions-end",
                button {
                    class: "btn btn-outline",
                    r#type: "button",
                    onclick: everywhere,
                    {i18n.t("profile.sign_out_everywhere")}
                }
            }
        }
    }
}

#[component]
fn SessionRow(session: SessionEntry, on_end: EventHandler<SessionEntry>) -> Element {
    let i18n = use_i18n();
    let created = format_timestamp(&i18n, session.created_at);
    let used = format_timestamp(&i18n, session.last_used_at);
    let current = session.current;
    rsx! {
        div { class: "app-settings-option profile-session",
            div { class: "app-settings-option-info",
                span { class: "app-settings-option-label",
                    {i18n.t(browser_key(&session.browser))}
                    if current {
                        span { class: "badge profile-session-current", {i18n.t("profile.session_current")} }
                    }
                }
                span { class: "app-settings-option-hint",
                    {i18n.t_args("profile.session_times", &[("created", &created), ("used", &used)])}
                }
                span { class: "app-settings-option-hint", "{session.client_id}" }
            }
            button {
                class: "btn btn-outline btn-sm",
                r#type: "button",
                onclick: move |_| on_end.call(session.clone()),
                if current {
                    {i18n.t("account.sign_out")}
                } else {
                    {i18n.t("profile.session_end")}
                }
            }
        }
    }
}

// ── Account: the provider's page, account data, deletion ────────────────────

/// Saves the account data the server exports.
#[cfg(target_arch = "wasm32")]
async fn download_account_data(api: &ApiClient) -> Result<(), ApiError> {
    let mut download = crate::api::BrowserDownload::new("oxidgene-account.json");
    if !download.ready().await? {
        return Ok(());
    }
    api.download_in_browser(download, "/api/v1/me/export").await
}

/// Saves the account data the server exports. The desktop has no account;
/// this keeps the page building for it.
#[cfg(not(target_arch = "wasm32"))]
async fn download_account_data(api: &ApiClient) -> Result<(), ApiError> {
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_file_name("oxidgene-account.json")
        .save_file()
        .await
    else {
        return Ok(());
    };
    api.download_to_file("/api/v1/me/export", file.path()).await
}

#[component]
fn AccountSection(account_url: Option<String>) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let accounts = use_accounts();
    let mut confirm_delete = use_signal(|| false);
    let mut deleting = use_signal(|| false);
    let mut delete_error = use_signal(|| None::<String>);
    let mut downloading = use_signal(|| false);
    let mut download_error = use_signal(|| None::<String>);

    let download = {
        let api = api.clone();
        move |_| {
            let api = api.clone();
            spawn(async move {
                downloading.set(true);
                download_error.set(None);
                if download_account_data(&api).await.is_err() {
                    download_error.set(Some(i18n.t("profile.data_failed")));
                }
                downloading.set(false);
            });
        }
    };
    let delete = move |_| {
        let (api, accounts) = (api.clone(), accounts.clone());
        spawn(async move {
            deleting.set(true);
            delete_error.set(None);
            match api.delete_me().await {
                Ok(()) => {
                    if let Some(accounts) = accounts {
                        accounts.navigate("/");
                    }
                }
                Err(error) => delete_error.set(Some(failure(&i18n, &error))),
            }
            deleting.set(false);
        });
    };

    rsx! {
        div { class: "settings-section",
            SectionHeader {
                eyebrow: i18n.t("profile.section_account"),
                title: i18n.t("profile.account_title"),
                subtitle: i18n.t("profile.account_desc"),
            }
            div { class: "app-settings-card",
                if let Some(url) = account_url {
                    div { class: "app-settings-option",
                        div { class: "app-settings-option-info",
                            span { class: "app-settings-option-label", {i18n.t("profile.provider_account")} }
                            span { class: "app-settings-option-hint", {i18n.t("profile.provider_account_hint")} }
                        }
                        a {
                            class: "btn btn-outline",
                            href: "{url}",
                            target: "_blank",
                            rel: "noopener noreferrer",
                            {i18n.t("profile.provider_account_open")}
                        }
                    }
                }
                div { class: "app-settings-option",
                    div { class: "app-settings-option-info",
                        span { class: "app-settings-option-label", {i18n.t("profile.data")} }
                        span { class: "app-settings-option-hint", {i18n.t("profile.data_hint")} }
                    }
                    button {
                        class: "btn btn-outline",
                        r#type: "button",
                        disabled: downloading(),
                        onclick: download,
                        {i18n.t("profile.data_download")}
                    }
                }
                if let Some(error) = download_error() {
                    div { class: "error-msg", role: "alert", "{error}" }
                }
                div { class: "app-settings-option",
                    div { class: "app-settings-option-info",
                        span { class: "app-settings-option-label", {i18n.t("profile.delete")} }
                        span { class: "app-settings-option-hint", {i18n.t("profile.delete_hint")} }
                    }
                    button {
                        class: "btn btn-danger",
                        r#type: "button",
                        onclick: move |_| {
                            delete_error.set(None);
                            confirm_delete.set(true);
                        },
                        {i18n.t("profile.delete")}
                    }
                }
            }
        }
        if confirm_delete() {
            ConfirmDialog {
                title: i18n.t("profile.delete_confirm_title"),
                message: i18n.t("profile.delete_confirm_message"),
                confirm_label: i18n.t("profile.delete_confirm"),
                error: delete_error(),
                busy: deleting(),
                on_confirm: delete,
                on_cancel: move |_| confirm_delete.set(false),
            }
        }
    }
}

const PROFILE_STYLES: &str = r#"
    .profile-avatar-row {
        display: flex;
        align-items: center;
        gap: var(--space-8);
        min-width: 0;
    }

    .profile-actions {
        display: flex;
        flex-wrap: wrap;
        gap: var(--space-4);
    }

    .profile-actions-end {
        justify-content: flex-end;
        margin-top: var(--space-6);
    }

    .profile-name-form {
        display: flex;
        gap: var(--space-4);
    }

    .profile-name-form input {
        flex: 1;
        min-width: 0;
    }

    .profile-saved {
        font-size: var(--text-85);
        color: var(--text-secondary);
    }

    .profile-email {
        display: flex;
        align-items: center;
        gap: var(--space-4);
        overflow-wrap: anywhere;
    }

    .profile-session-current {
        margin-left: var(--space-4);
    }

    @media (max-width: 640px) {
        .profile-avatar-row,
        .app-settings-option.profile-session {
            align-items: flex-start;
        }
    }
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_avatar_is_a_jpeg_a_png_or_a_webp() {
        assert_eq!(sniff_image(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(sniff_image(b"\x89PNG\r\n\x1a\n...."), Some("image/png"));
        assert_eq!(sniff_image(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(sniff_image(b"GIF89a"), None);
        assert_eq!(sniff_image(b""), None);
    }

    #[test]
    fn every_browser_family_has_a_name() {
        let english = Language::english().translations();
        for browser in [
            "firefox", "chrome", "edge", "safari", "opera", "other", "lynx",
        ] {
            assert!(english.contains_key(browser_key(browser)), "{browser}");
        }
        for section in Section::ALL {
            assert!(english.contains_key(section.label_key()));
        }
    }
}
