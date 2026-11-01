//! The account surfaces every page shares, in the web build's `oidc` mode
//! only (docs/authentication.md §10): the navbar's **Sign in** or avatar
//! menu, the invitation to sign in that stands in for a page needing an
//! account, and the dialogs of a failed sign-in and of an ended session.
//!
//! Each renders nothing without the [`crate::account::Accounts`] capability
//! or in `none` mode, so the desktop and the unauthenticated web build look
//! exactly as they did.

use dioxus::prelude::*;

use crate::account::{
    AccountPrompt, AccountState, sign_in, sign_out, use_account, use_account_prompt, use_accounts,
};
use crate::api::ApiClient;
use crate::components::confirm_dialog::ConfirmDialog;
use crate::components::context_menu::{MenuButton, MenuClose};
use crate::components::empty_state::EmptyState;
use crate::components::notification_bell::NotificationBell;
use crate::components::user_avatar::UserAvatar;
use crate::i18n::{Language, use_i18n};
use crate::router::Route;
use crate::ui_observability::use_ui_resource;

/// The navbar's right zone: **Sign in** for a visitor, the user's avatar
/// opening their menu once signed in.
#[component]
pub fn AccountNav() -> Element {
    let state = use_account();
    let accounts = use_accounts();
    let api = use_context::<ApiClient>();
    let language = use_context::<Signal<Language>>();
    let route = use_route::<Route>();
    let i18n = use_i18n();
    let mut sign_out_error = use_signal(|| None::<String>);
    // Signing out leaves the page; a refusal (the provider or the session
    // store out of reach) keeps it, and says so.
    let leave = use_callback({
        let (accounts, api) = (accounts.clone(), api.clone());
        move |()| {
            let (Some(accounts), api) = (accounts.clone(), api.clone()) else {
                return;
            };
            spawn(async move {
                sign_out_error.set(None);
                if sign_out(&accounts, &api, false).await.is_err() {
                    sign_out_error.set(Some(i18n.t("account.sign_out_failed")));
                }
            });
        }
    });
    let Some(accounts) = accounts else {
        return rsx! {};
    };

    let signed_in = match &*state.read() {
        AccountState::SignedIn(signed_in) => signed_in.me.clone(),
        AccountState::Anonymous => {
            return rsx! {
                div { class: "nav-end",
                    button {
                        class: "btn btn-primary nav-sign-in",
                        r#type: "button",
                        onclick: move |_| sign_in(&accounts, &api, &route, language()),
                        {i18n.t("account.sign_in")}
                    }
                }
            };
        }
        AccountState::Absent | AccountState::Loading => return rsx! {},
    };

    let name = signed_in
        .display_name
        .clone()
        .or_else(|| signed_in.email.clone())
        .unwrap_or_default();
    let email = signed_in.email.clone().filter(|email| *email != name);
    let avatar = signed_in.avatar.as_ref().map(|avatar| avatar.etag.clone());

    rsx! {
        div { class: "nav-end",
            NotificationBell { reader: signed_in.id }
            MenuButton {
                name: name.clone(),
                label: i18n.t("account.menu"),
                class: "account-menu-trigger",
                menu_class: "account-menu",
                trigger: rsx! {
                    UserAvatar {
                        user_id: signed_in.id,
                        name: signed_in.display_name.clone(),
                        avatar: avatar.clone(),
                    }
                },
                items: move |menu: MenuClose| rsx! {
                    div { class: "context-menu-header account-menu-who", role: "none",
                        span { class: "account-menu-name", "{name}" }
                        if let Some(email) = email.clone() {
                            span { class: "account-menu-email", "{email}" }
                        }
                    }
                    Link {
                        to: Route::Profile {},
                        role: "menuitem",
                        class: "context-menu-item",
                        onclick: move |_| menu.close.call(()),
                        {i18n.t("account.profile")}
                    }
                    button {
                        role: "menuitem",
                        class: "context-menu-item",
                        onclick: move |_| {
                            menu.close.call(());
                            leave.call(());
                        },
                        {i18n.t("account.sign_out")}
                    }
                },
            }
            if let Some(error) = sign_out_error() {
                ConfirmDialog {
                    title: i18n.t("account.sign_out"),
                    message: error,
                    confirm_label: i18n.t("account.try_again"),
                    dismiss_label: i18n.t("common.close"),
                    confirm_class: "btn btn-primary",
                    on_confirm: move |_| leave.call(()),
                    on_cancel: move |_| sign_out_error.set(None),
                }
            }
        }
    }
}

/// What a page needing an account shows a visitor: why, and **Sign in**,
/// which comes back to the page.
#[component]
pub fn SignInRequired() -> Element {
    let accounts = use_accounts();
    let api = use_context::<ApiClient>();
    let language = use_context::<Signal<Language>>();
    let route = use_route::<Route>();
    let i18n = use_i18n();
    let Some(accounts) = accounts else {
        return rsx! {};
    };
    rsx! {
        div { class: "sub-page-content sign-in-required",
            EmptyState {
                title: i18n.t("account.sign_in_required_title"),
                action: rsx! {
                    button {
                        class: "btn btn-primary",
                        r#type: "button",
                        onclick: move |_| sign_in(&accounts, &api, &route, language()),
                        {i18n.t("account.sign_in")}
                    }
                },
                p { class: "text-muted", {i18n.t("account.sign_in_required_message")} }
            }
        }
    }
}

/// The banner over a tree's pages when the viewer is a Visitor of it
/// (docs/authorization.md §11): they browse it under its privacy settings;
/// an anonymous one is offered to sign in. The web build's sharing surfaces
/// only: the desktop and the `none` mode show none.
#[component]
pub fn VisitorBanner(tree_id: String) -> Element {
    let accounts = use_accounts();
    let api = use_context::<ApiClient>();
    let language = use_context::<Signal<Language>>();
    let route = use_route::<Route>();
    let account = use_account();
    let i18n = use_i18n();
    let tree = tree_id.parse::<uuid::Uuid>().ok();
    let api_access = api.clone();
    let access = use_ui_resource("viewer_access", move || {
        let api = api_access.clone();
        let sharing = crate::sharing::offered(&account.read());
        async move {
            match tree.filter(|_| sharing) {
                Some(tree) => api.tree_access(tree).await.ok(),
                None => None,
            }
        }
    });
    let visitor = matches!(
        &*access.read(),
        Some(Some(access)) if access.role == oxidgene_core::access::TreeRole::Visitor
    );
    let (true, Some(accounts)) = (visitor, accounts) else {
        return rsx! {};
    };
    let anonymous = *account.read() == AccountState::Anonymous;
    rsx! {
        div { class: "visitor-banner", role: "status",
            span { {i18n.t("sharing.visitor_banner")} }
            if anonymous {
                button {
                    class: "btn btn-outline btn-sm",
                    r#type: "button",
                    onclick: move |_| sign_in(&accounts, &api, &route, language()),
                    {i18n.t("account.sign_in")}
                }
            }
        }
    }
}

/// The dialog the account UI opened, if any: a failed sign-in, or a session
/// that ended under a signed-in page.
#[component]
pub fn AccountPrompts() -> Element {
    let mut prompt = use_account_prompt();
    let accounts = use_accounts();
    let api = use_context::<ApiClient>();
    let language = use_context::<Signal<Language>>();
    let route = use_route::<Route>();
    let nav = navigator();
    let i18n = use_i18n();

    // The failed sign-in's error leaves the address once it is shown, so a
    // reload does not show it again.
    let failed = matches!(prompt(), Some(AccountPrompt::SignInFailed(_)));
    let replace_route = route.clone();
    use_effect(use_reactive!(|failed| {
        if failed {
            nav.replace(replace_route.clone());
        }
    }));

    let (Some(accounts), Some(current)) = (accounts, prompt()) else {
        return rsx! {};
    };
    let (title, message, confirm) = match current {
        AccountPrompt::SessionEnded => (
            i18n.t("account.session_ended_title"),
            i18n.t("account.session_ended_message"),
            i18n.t("account.sign_in_again"),
        ),
        AccountPrompt::SignInFailed(error) => (
            i18n.t("account.sign_in_failed_title"),
            i18n.t(error.message_key()),
            i18n.t("account.try_again"),
        ),
    };
    rsx! {
        ConfirmDialog {
            title,
            message,
            confirm_label: confirm,
            dismiss_label: i18n.t("common.close"),
            confirm_class: "btn btn-primary",
            on_confirm: move |_| sign_in(&accounts, &api, &route, language()),
            on_cancel: move |_| prompt.set(None),
        }
    }
}
