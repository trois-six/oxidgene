//! The navbar's bell (docs/messaging.md §2.2): the unread count, and a panel
//! of the latest notifications, newest first, each opening what it is about
//! and marked read when opened, with **Mark all as read**. Signed-in users
//! of the web build only: the account navigation draws it beside the avatar.

use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::ApiClient;
use crate::components::context_menu::{MenuButton, MenuClose};
use crate::components::history_diff::format_timestamp;
use crate::i18n::use_i18n;
use crate::notifications::{self, refresh, use_notifications};

/// The bell of the signed-in user `reader`.
#[component]
pub fn NotificationBell(reader: Uuid) -> Element {
    let state = use_notifications();
    let api = use_context::<ApiClient>();
    let i18n = use_i18n();
    let navigator = use_navigator();
    let unread = state.read().unread;
    let label = if unread > 0 {
        i18n.t_args(
            "notifications.bell_unread",
            &[("count", &unread.to_string())],
        )
    } else {
        i18n.t("notifications.title")
    };
    let mark_all = {
        let api = api.clone();
        move |menu: MenuClose| {
            let api = api.clone();
            spawn(async move {
                if api.mark_notifications_read(None).await.is_ok() {
                    let _ = refresh(&api, state).await;
                }
                menu.close.call(());
            });
        }
    };

    rsx! {
        MenuButton {
            name: i18n.t("notifications.title"),
            label: label.clone(),
            class: "notification-bell",
            menu_class: "notification-panel",
            trigger: rsx! {
                svg {
                    class: "notification-bell-icon",
                    "viewBox": "0 0 24 24",
                    "aria-hidden": "true",
                    path {
                        d: "M12 22a2.5 2.5 0 0 0 2.45-2h-4.9A2.5 2.5 0 0 0 12 22Zm7-6V11a7 7 0 0 0-5.5-6.84V3.5a1.5 1.5 0 0 0-3 0v.66A7 7 0 0 0 5 11v5l-2 2v1h18v-1Z",
                    }
                }
                if unread > 0 {
                    span { class: "notification-badge", aria_hidden: "true",
                        if unread > 99 { "99+" } else { "{unread}" }
                    }
                }
            },
            items: move |menu: MenuClose| {
                let current = state.read();
                let api = api.clone();
                let mark_all = mark_all.clone();
                rsx! {
                    div { class: "context-menu-header notification-panel-header", role: "none",
                        span { class: "notification-panel-title", {i18n.t("notifications.title")} }
                        if current.unread > 0 {
                            button {
                                role: "menuitem",
                                class: "btn btn-outline btn-sm",
                                onclick: move |_| mark_all(menu),
                                {i18n.t("notifications.mark_all")}
                            }
                        }
                    }
                    if current.loaded && current.latest.is_empty() {
                        div { class: "notification-empty", role: "none",
                            {i18n.t("notifications.empty")}
                        }
                    }
                    for notification in current.latest.clone() {
                        button {
                            key: "{notification.id}",
                            role: "menuitem",
                            class: if notification.read_at.is_none() { "context-menu-item notification-item unread" } else { "context-menu-item notification-item" },
                            onclick: {
                                let api = api.clone();
                                let notification = notification.clone();
                                move |_| {
                                    let api = api.clone();
                                    let id = notification.id;
                                    let unread = notification.read_at.is_none();
                                    navigator.push(notifications::target(&notification));
                                    menu.close.call(());
                                    spawn(async move {
                                        if unread && api.mark_notifications_read(Some(&[id])).await.is_ok() {
                                            let _ = refresh(&api, state).await;
                                        }
                                    });
                                }
                            },
                            span { class: "notification-text", {notifications::text(&i18n, &notification, reader)} }
                            span { class: "notification-time", {format_timestamp(&i18n, notification.created_at)} }
                        }
                    }
                }
            },
        }
    }
}
