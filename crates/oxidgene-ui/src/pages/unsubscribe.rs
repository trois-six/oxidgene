//! Where a mail's unsubscribe link leads (`/unsubscribe?token=`,
//! docs/messaging.md §2.3): the web build in `oidc` mode only, without
//! signing in. The page asks before it posts the token, so that a mail
//! scanner following links unsubscribes nobody; mail clients post it
//! themselves through the mail's one-click header (RFC 8058).

use dioxus::prelude::*;

use crate::account::{AccountState, use_account};
use crate::api::ApiClient;
use crate::i18n::use_i18n;
use crate::pages::not_found::NotFound;
use crate::router::Route;
use crate::ui_observability::{UiPage, use_ui_load_trace};

/// Where the page is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Asking,
    Sending,
    Done,
    Invalid,
}

#[component]
pub fn Unsubscribe(token: String) -> Element {
    let _load_trace = use_ui_load_trace(UiPage::Unsubscribe);
    let account = use_account();
    let api = use_context::<ApiClient>();
    let i18n = use_i18n();
    let mut step = use_signal(|| Step::Asking);
    if matches!(*account.read(), AccountState::Absent) {
        return rsx! {
            NotFound { segments: vec!["unsubscribe".to_string()] }
        };
    }
    let confirm = move |_| {
        let (api, token) = (api.clone(), token.clone());
        spawn(async move {
            step.set(Step::Sending);
            step.set(match api.unsubscribe(&token).await {
                Ok(_) => Step::Done,
                Err(_) => Step::Invalid,
            });
        });
    };
    rsx! {
        div { class: "sub-page-content unsubscribe-page",
            h1 { {i18n.t("unsubscribe.title")} }
            match step() {
                Step::Asking | Step::Sending => rsx! {
                    p { {i18n.t("unsubscribe.question")} }
                    button {
                        class: "btn btn-primary",
                        r#type: "button",
                        disabled: step() == Step::Sending,
                        onclick: confirm,
                        {i18n.t("unsubscribe.confirm")}
                    }
                },
                Step::Done => rsx! {
                    p { role: "status", {i18n.t("unsubscribe.done")} }
                    Link { to: Route::Profile {}, {i18n.t("unsubscribe.settings")} }
                },
                Step::Invalid => rsx! {
                    div { class: "error-msg", role: "alert", {i18n.t("unsubscribe.invalid")} }
                },
            }
        }
    }
}
