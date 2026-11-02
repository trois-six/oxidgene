//! The Users section of App Settings' Instance group (docs/authorization.md
//! §2, §11): the administrator's list of the instance's users, in the web
//! build's `oidc` mode only. An administrator grants or withdraws the
//! administrator role and tree creation, deactivates and reactivates,
//! binds a user to another identity at the provider, and erases a user.
//!
//! The trees, the access log and the messaging reports of the Instance
//! group join it with the phases that store them.

use dioxus::prelude::*;
use oxidgene_core::access::{InstanceRole, UserStatus};
use uuid::Uuid;

use crate::account::use_account;
use crate::api::ApiClient;
use crate::api::account::{ManagedUser, UserPatch};
use crate::components::confirm_dialog::ConfirmDialog;
use crate::components::context_menu::{MenuButton, MenuClose};
use crate::components::history_diff::format_timestamp;
use crate::components::modal::Modal;
use crate::components::paged_list::use_paged_list;
use crate::components::user_avatar::UserAvatar;
use crate::i18n::{I18n, use_i18n};

/// An action an administrator takes on a user, from the user's menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UserAction {
    MakeAdmin,
    RemoveAdmin,
    AllowTrees,
    DisallowTrees,
    Deactivate,
    Reactivate,
}

impl UserAction {
    /// The actions offered for `user`, in menu order.
    fn offered(user: &ManagedUser) -> Vec<Self> {
        if user.status == UserStatus::Erased {
            return Vec::new();
        }
        vec![
            if user.instance_role == InstanceRole::Admin {
                Self::RemoveAdmin
            } else {
                Self::MakeAdmin
            },
            if user.can_create_trees {
                Self::DisallowTrees
            } else {
                Self::AllowTrees
            },
            if user.status == UserStatus::Deactivated {
                Self::Reactivate
            } else {
                Self::Deactivate
            },
        ]
    }

    fn label_key(self) -> &'static str {
        match self {
            Self::MakeAdmin => "instance.users.make_admin",
            Self::RemoveAdmin => "instance.users.remove_admin",
            Self::AllowTrees => "instance.users.allow_trees",
            Self::DisallowTrees => "instance.users.disallow_trees",
            Self::Deactivate => "instance.users.deactivate",
            Self::Reactivate => "instance.users.reactivate",
        }
    }

    fn patch(self) -> UserPatch {
        match self {
            Self::MakeAdmin => UserPatch {
                instance_role: Some(InstanceRole::Admin),
                ..UserPatch::default()
            },
            Self::RemoveAdmin => UserPatch {
                instance_role: Some(InstanceRole::User),
                ..UserPatch::default()
            },
            Self::AllowTrees => UserPatch {
                can_create_trees: Some(true),
                ..UserPatch::default()
            },
            Self::DisallowTrees => UserPatch {
                can_create_trees: Some(false),
                ..UserPatch::default()
            },
            Self::Deactivate => UserPatch {
                status: Some(UserStatus::Deactivated),
                ..UserPatch::default()
            },
            Self::Reactivate => UserPatch {
                status: Some(UserStatus::Active),
                ..UserPatch::default()
            },
        }
    }
}

/// The name a user is listed under: an erased one is a former member.
fn listed_name(i18n: &I18n, user: &ManagedUser) -> String {
    if user.status == UserStatus::Erased {
        return i18n.t("instance.users.former_member");
    }
    user.display_name
        .clone()
        .or_else(|| user.username.clone())
        .or_else(|| user.email.clone())
        .unwrap_or_else(|| i18n.t("instance.users.unnamed"))
}

/// What a refused administrator change says.
fn failure(i18n: &I18n, error: &crate::api::ApiError) -> String {
    match error.status() {
        Some(400) => i18n.t("instance.users.claim_decides"),
        Some(409) => i18n.t("instance.users.conflict"),
        _ => i18n.t("instance.users.failed"),
    }
}

#[component]
pub fn InstanceUsersSection() -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let account = use_account();
    let me = account.read().me().map(|me| me.id);
    let mut refresh = use_signal(|| 0_u32);
    let mut error = use_signal(|| None::<String>);
    let mut rebinding = use_signal(|| None::<ManagedUser>);
    let mut erasing = use_signal(|| None::<ManagedUser>);
    let mut busy = use_signal(|| false);

    let users = use_paged_list("admin_users", {
        let api = api.clone();
        move |cursor: Option<String>| {
            let api = api.clone();
            let _ = refresh();
            async move {
                api.admin_users(cursor.as_deref())
                    .await
                    .map_err(|error| error.message(&i18n))
            }
        }
    });

    let act = {
        let api = api.clone();
        move |(user_id, action): (Uuid, UserAction)| {
            let api = api.clone();
            spawn(async move {
                error.set(None);
                match api.update_user(user_id, &action.patch()).await {
                    Ok(_) => refresh += 1,
                    Err(failed) => error.set(Some(failure(&i18n, &failed))),
                }
            });
        }
    };
    let erase = {
        let api = api.clone();
        move |_| {
            let Some(user) = erasing() else { return };
            let api = api.clone();
            spawn(async move {
                busy.set(true);
                error.set(None);
                match api.erase_user(user.id).await {
                    Ok(()) => {
                        erasing.set(None);
                        refresh += 1;
                    }
                    Err(failed) => error.set(Some(failure(&i18n, &failed))),
                }
                busy.set(false);
            });
        }
    };

    rsx! {
        style { {INSTANCE_USERS_STYLES} }
        div { class: "settings-section",
            span { class: "settings-section-eyebrow", {i18n.t("instance.title")} }
            h2 { class: "settings-section-title", {i18n.t("instance.users.title")} }
            p { class: "settings-section-subtitle", {i18n.t("instance.users.desc")} }

            if let (Some(message), None) = (error(), erasing()) {
                div { class: "error-msg", role: "alert", "{message}" }
            }
            div { class: "app-settings-card instance-users",
                match &*users.first.read() {
                    None => rsx! { p { class: "loading", {i18n.t("common.loading")} } },
                    Some(Err(_)) => rsx! {
                        div { class: "error-msg", role: "alert", {i18n.t("instance.users.load_failed")} }
                    },
                    Some(Ok(_)) => rsx! {
                        for user in users.items() {
                            UserRow {
                                key: "{user.id}",
                                own: Some(user.id) == me,
                                user: user.clone(),
                                on_action: act.clone(),
                                on_rebind: move |user| rebinding.set(Some(user)),
                                on_erase: move |user| {
                                    error.set(None);
                                    erasing.set(Some(user));
                                },
                            }
                        }
                    },
                }
            }
            if users.has_more() {
                div { class: "instance-users-more",
                    button {
                        class: "btn btn-outline",
                        r#type: "button",
                        disabled: users.loading_more(),
                        onclick: move |_| users.load_more(),
                        {i18n.t("history.load_more")}
                    }
                }
            }
        }
        if let Some(user) = rebinding() {
            RebindDialog {
                user,
                on_close: move |changed: bool| {
                    rebinding.set(None);
                    if changed {
                        refresh += 1;
                    }
                },
            }
        }
        if let Some(user) = erasing() {
            ConfirmDialog {
                title: i18n.t("instance.users.erase_title"),
                message: i18n.t_args("instance.users.erase_message", &[("name", &listed_name(&i18n, &user))]),
                confirm_label: i18n.t("instance.users.erase"),
                error: error(),
                busy: busy(),
                on_confirm: erase,
                on_cancel: move |_| erasing.set(None),
            }
        }
    }
}

/// One user of the list, with their menu.
#[component]
fn UserRow(
    user: ManagedUser,
    /// Whether the row is the administrator's own: they change their own
    /// account from their profile, not here.
    own: bool,
    on_action: EventHandler<(Uuid, UserAction)>,
    on_rebind: EventHandler<ManagedUser>,
    on_erase: EventHandler<ManagedUser>,
) -> Element {
    let i18n = use_i18n();
    let name = listed_name(&i18n, &user);
    let erased = user.status == UserStatus::Erased;
    let details: Vec<String> = [
        user.username.as_ref().map(|u| format!("@{u}")),
        user.email.clone(),
    ]
    .into_iter()
    .flatten()
    .collect();
    let signed_in = format_timestamp(&i18n, user.last_sign_in_at);
    let avatar = user.avatar.as_ref().map(|avatar| avatar.version.clone());
    let actions = if own {
        Vec::new()
    } else {
        UserAction::offered(&user)
    };
    let menu_user = user.clone();
    let user_id = user.id;

    rsx! {
        div { class: "app-settings-option instance-user",
            div { class: "instance-user-who",
                UserAvatar {
                    user_id,
                    name: (!erased).then(|| name.clone()),
                    avatar,
                }
                div { class: "app-settings-option-info",
                    span { class: "app-settings-option-label instance-user-name",
                        "{name}"
                        if user.instance_role == InstanceRole::Admin {
                            span { class: "badge", {i18n.t("instance.users.admin")} }
                        }
                        if user.can_create_trees && !erased {
                            span { class: "badge", {i18n.t("instance.users.creates_trees")} }
                        }
                        if user.status == UserStatus::Deactivated {
                            span { class: "badge", {i18n.t("instance.users.deactivated")} }
                        }
                    }
                    if !details.is_empty() {
                        span { class: "app-settings-option-hint", {details.join(" · ")} }
                    }
                    span { class: "app-settings-option-hint",
                        {i18n.t_args("instance.users.last_sign_in", &[("date", &signed_in)])}
                    }
                }
            }
            if !actions.is_empty() {
                MenuButton {
                    name: name.clone(),
                    items: move |menu: MenuClose| {
                        let user = menu_user.clone();
                        rsx! {
                            for action in actions.clone() {
                                button {
                                    key: "{action.label_key()}",
                                    role: "menuitem",
                                    class: "context-menu-item",
                                    onclick: move |_| {
                                        menu.close.call(());
                                        on_action.call((user_id, action));
                                    },
                                    {i18n.t(action.label_key())}
                                }
                            }
                            button {
                                role: "menuitem",
                                class: "context-menu-item",
                                onclick: {
                                    let user = user.clone();
                                    move |_| {
                                        menu.hand_off.call(());
                                        on_rebind.call(user.clone());
                                    }
                                },
                                {i18n.t("instance.users.rebind")}
                            }
                            button {
                                role: "menuitem",
                                class: "context-menu-item context-menu-danger",
                                onclick: move |_| {
                                    menu.hand_off.call(());
                                    on_erase.call(user.clone());
                                },
                                {i18n.t("instance.users.erase")}
                            }
                        }
                    },
                }
            }
        }
    }
}

/// Binds a user to another identity at the provider: its issuer and
/// subject (docs/authentication.md §6.4).
#[component]
fn RebindDialog(user: ManagedUser, on_close: EventHandler<bool>) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let mut issuer = use_signal(|| user.issuer.clone().unwrap_or_default());
    let mut subject = use_signal(|| user.subject.clone().unwrap_or_default());
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let user_id = user.id;

    let save = move |event: Event<FormData>| {
        event.prevent_default();
        let api = api.clone();
        spawn(async move {
            busy.set(true);
            error.set(None);
            match api
                .rebind_user_identity(user_id, issuer().trim(), subject().trim())
                .await
            {
                Ok(_) => on_close.call(true),
                Err(failed) => error.set(Some(failure(&i18n, &failed))),
            }
            busy.set(false);
        });
    };
    let ready = !issuer().trim().is_empty() && !subject().trim().is_empty();

    rsx! {
        Modal {
            label: i18n.t("instance.users.rebind_title"),
            busy: busy(),
            on_close: move |()| on_close.call(false),
            h3 { {i18n.t("instance.users.rebind_title")} }
            p { class: "confirm-message", {i18n.t("instance.users.rebind_hint")} }
            form { onsubmit: save,
                div { class: "form-group",
                    label { r#for: "rebind-issuer", {i18n.t("instance.users.issuer")} }
                    input {
                        id: "rebind-issuer",
                        r#type: "url",
                        value: "{issuer}",
                        oninput: move |event: Event<FormData>| issuer.set(event.value()),
                    }
                }
                div { class: "form-group",
                    label { r#for: "rebind-subject", {i18n.t("instance.users.subject")} }
                    input {
                        id: "rebind-subject",
                        r#type: "text",
                        value: "{subject}",
                        oninput: move |event: Event<FormData>| subject.set(event.value()),
                    }
                }
                if let Some(message) = error() {
                    div { class: "error-msg", role: "alert", "{message}" }
                }
                div { class: "modal-actions",
                    button {
                        class: "btn btn-outline",
                        r#type: "button",
                        disabled: busy(),
                        onclick: move |_| on_close.call(false),
                        {i18n.t("common.cancel")}
                    }
                    button {
                        class: "btn btn-primary",
                        r#type: "submit",
                        disabled: busy() || !ready,
                        {i18n.t("common.save")}
                    }
                }
            }
        }
    }
}

const INSTANCE_USERS_STYLES: &str = r#"
    .instance-users {
        margin-top: var(--space-8);
    }

    .instance-user-who {
        display: flex;
        align-items: center;
        gap: var(--space-6);
        min-width: 0;
    }

    .instance-user-name {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: var(--space-3);
    }

    .instance-users-more {
        display: flex;
        justify-content: center;
        margin-top: var(--space-6);
    }
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn user(role: &str, trees: bool, status: &str) -> ManagedUser {
        serde_json::from_value(serde_json::json!({
            "id": "0195c0de-0000-7000-8000-000000000001",
            "display_name": "Ada Example",
            "instance_role": role,
            "can_create_trees": trees,
            "status": status,
            "created_at": "2026-10-01T00:00:00Z",
            "last_sign_in_at": "2026-10-30T00:00:00Z",
        }))
        .unwrap()
    }

    #[test]
    fn the_menu_offers_the_opposite_of_each_state() {
        assert_eq!(
            UserAction::offered(&user("user", false, "active")),
            vec![
                UserAction::MakeAdmin,
                UserAction::AllowTrees,
                UserAction::Deactivate
            ]
        );
        assert_eq!(
            UserAction::offered(&user("admin", true, "deactivated")),
            vec![
                UserAction::RemoveAdmin,
                UserAction::DisallowTrees,
                UserAction::Reactivate
            ]
        );
        assert!(UserAction::offered(&user("user", false, "erased")).is_empty());
    }

    #[test]
    fn each_action_changes_one_field() {
        for (action, expected) in [
            (UserAction::MakeAdmin, r#"{"instance_role":"admin"}"#),
            (UserAction::RemoveAdmin, r#"{"instance_role":"user"}"#),
            (UserAction::AllowTrees, r#"{"can_create_trees":true}"#),
            (UserAction::DisallowTrees, r#"{"can_create_trees":false}"#),
            (UserAction::Deactivate, r#"{"status":"deactivated"}"#),
            (UserAction::Reactivate, r#"{"status":"active"}"#),
        ] {
            assert_eq!(serde_json::to_string(&action.patch()).unwrap(), expected);
        }
    }

    #[test]
    fn an_erased_user_is_a_former_member() {
        let english = crate::i18n::I18n::new(crate::i18n::Language::english());
        assert_eq!(
            listed_name(&english, &user("user", false, "erased")),
            english.t("instance.users.former_member")
        );
        assert_eq!(
            listed_name(&english, &user("user", false, "active")),
            "Ada Example"
        );
    }
}
