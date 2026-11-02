//! What the homepage shows of sharing in the web build
//! (docs/authorization.md §11, docs/ui-home.md §6): the viewer's role on
//! each tree, the *My trees* and *Shared with me* filters, and the
//! invitations waiting for an answer, each accepted or declined in place.
//!
//! Nothing here renders without the sharing surfaces ([`crate::sharing`]):
//! the desktop and the web build's `none` mode keep the page as it was.

use dioxus::prelude::*;
use oxidgene_core::access::TreeRole;

use super::sharing_settings::Listed;
use crate::account::use_account;
use crate::api::ApiClient;
use crate::api::sharing::MyInvitation;
use crate::components::date_input::format_day;
use crate::i18n::use_i18n;
use crate::sharing::use_sharing_surfaces;
use crate::ui_observability::use_ui_resource;

/// Which trees the homepage lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ownership {
    All,
    /// The trees the viewer owns.
    Mine,
    /// The trees others share with the viewer.
    Shared,
}

impl Ownership {
    /// The filter a select's value names.
    pub fn parse(value: &str) -> Self {
        match value {
            "mine" => Self::Mine,
            "shared" => Self::Shared,
            _ => Self::All,
        }
    }

    /// Whether a tree on which the viewer holds `role` is listed.
    pub fn keeps(self, role: TreeRole) -> bool {
        match self {
            Self::All => true,
            Self::Mine => role == TreeRole::Owner,
            Self::Shared => role != TreeRole::Owner,
        }
    }
}

/// The translation key naming `role`.
pub fn role_key(role: TreeRole) -> &'static str {
    match role {
        TreeRole::Owner => "sharing.role.owner",
        TreeRole::Editor => "sharing.role.editor",
        TreeRole::Guest => "sharing.role.guest",
        TreeRole::Visitor => "sharing.role.visitor",
    }
}

/// The *My trees* / *Shared with me* filter of the toolbar.
#[component]
pub fn OwnershipFilter(ownership: Signal<Ownership>) -> Element {
    let i18n = use_i18n();
    if !use_sharing_surfaces() {
        return rsx! {};
    }
    let value = match ownership() {
        Ownership::All => "all",
        Ownership::Mine => "mine",
        Ownership::Shared => "shared",
    };
    rsx! {
        select {
            class: "home-sort-select",
            "aria-label": i18n.t("home.filter_label"),
            value,
            onchange: move |e: Event<FormData>| ownership.set(Ownership::parse(&e.value())),
            option { value: "all", {i18n.t("home.filter_all")} }
            option { value: "mine", {i18n.t("home.filter_mine")} }
            option { value: "shared", {i18n.t("home.filter_shared")} }
        }
    }
}

/// The viewer's role on a tree, on its card.
#[component]
pub fn RoleBadge(role: TreeRole) -> Element {
    let i18n = use_i18n();
    if !use_sharing_surfaces() {
        return rsx! {};
    }
    rsx! {
        span { class: "badge tree-card-role", {i18n.t(role_key(role))} }
    }
}

/// The invitations addressed to the signed-in user, above the cards, each
/// with **Accept** and **Decline**. `on_accepted` reads the tree list
/// again: an accepted invitation is a tree of the user's.
#[component]
pub fn HomeInvitations(on_accepted: EventHandler<()>) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let account = use_account();
    let mut refresh = use_signal(|| 0u32);
    let mut error = use_signal(|| None::<String>);
    let api_list = api.clone();
    let invitations = use_ui_resource("my_invitations", move || {
        let api = api_list.clone();
        let _tick = refresh();
        // Read here, so the list is asked once the server has answered who
        // is signed in and that it shares.
        let ready = matches!(*account.read(), crate::account::AccountState::SignedIn(_));
        async move {
            if ready {
                api.my_invitations().await
            } else {
                Ok(Vec::new())
            }
        }
    });
    // A failed read says so: hidden, it would pass for "no invitation".
    let listed = Listed::of(&invitations.read(), &i18n);
    if listed.rows.is_empty() && listed.error.is_none() {
        return rsx! {};
    }
    let answer = move |(id, accept): (uuid::Uuid, bool)| {
        let api = api.clone();
        spawn(async move {
            let result = if accept {
                api.accept_invitation(id).await
            } else {
                api.decline_invitation(id).await
            };
            match result {
                Ok(()) => {
                    error.set(None);
                    refresh += 1;
                    if accept {
                        on_accepted.call(());
                    }
                }
                Err(e) => error.set(Some(e.message(&i18n))),
            }
        });
    };
    rsx! {
        section { class: "home-invitations", "aria-label": i18n.t("sharing.invitations.title"),
            h2 { class: "home-invitations-title", {i18n.t("sharing.invitations.title")} }
            for invitation in listed.rows {
                InvitationRow { key: "{invitation.id}", invitation, on_answer: answer.clone() }
            }
            if let Some(error) = listed.error {
                div { class: "error-msg", role: "alert", "{error}" }
            }
            if let Some(error) = error() {
                div { class: "error-msg", role: "alert", "{error}" }
            }
        }
    }
}

#[component]
fn InvitationRow(invitation: MyInvitation, on_answer: EventHandler<(uuid::Uuid, bool)>) -> Element {
    let i18n = use_i18n();
    let role = i18n.t(role_key(invitation.role));
    let text = match &invitation.inviter_name {
        Some(inviter) => i18n.t_args(
            "sharing.invitations.from",
            &[
                ("inviter", inviter),
                ("tree", &invitation.tree_name),
                ("role", &role),
            ],
        ),
        None => i18n.t_args(
            "sharing.invitations.from_unknown",
            &[("tree", &invitation.tree_name), ("role", &role)],
        ),
    };
    let expires = format_day(&i18n, invitation.expires_at.date_naive());
    let id = invitation.id;
    rsx! {
        div { class: "home-invitation",
            div { class: "home-invitation-text",
                span { "{text}" }
                span { class: "home-invitation-hint",
                    if invitation.scoped {
                        {i18n.t("sharing.invitations.scoped")}
                        " · "
                    }
                    {i18n.t_args("sharing.invitations.expires", &[("date", &expires)])}
                }
            }
            div { class: "home-invitation-actions",
                button {
                    class: "btn btn-outline btn-sm",
                    r#type: "button",
                    onclick: move |_| on_answer.call((id, false)),
                    {i18n.t("sharing.invitations.decline")}
                }
                button {
                    class: "btn btn-primary btn-sm",
                    r#type: "button",
                    onclick: move |_| on_answer.call((id, true)),
                    {i18n.t("sharing.invitations.accept")}
                }
            }
        }
    }
}

/// The styles of the homepage's sharing parts.
pub const HOME_SHARING_STYLES: &str = r#"
    .tree-card-role {
        font-size: var(--text-65);
        text-transform: uppercase;
        letter-spacing: 0.04em;
    }

    .home-invitations {
        display: flex;
        flex-direction: column;
        gap: var(--space-4);
        margin-bottom: var(--space-12);
    }

    .home-invitations-title {
        font-family: var(--font-heading);
        font-size: var(--text-100);
        margin: 0;
    }

    .home-invitation {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--space-8);
        padding: var(--space-6) var(--space-8);
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: var(--radius-lg);
    }

    .home-invitation-text {
        display: flex;
        flex-direction: column;
        gap: var(--space-2);
    }

    .home-invitation-hint {
        font-size: var(--text-75);
        color: var(--text-muted);
    }

    .home-invitation-actions {
        display: flex;
        gap: var(--space-4);
        flex-shrink: 0;
    }
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_filters_keep_the_owners_or_the_others_trees() {
        assert!(Ownership::All.keeps(TreeRole::Visitor));
        assert!(Ownership::Mine.keeps(TreeRole::Owner));
        assert!(!Ownership::Mine.keeps(TreeRole::Guest));
        assert!(Ownership::Shared.keeps(TreeRole::Editor));
        assert!(!Ownership::Shared.keeps(TreeRole::Owner));
        assert_eq!(Ownership::parse("shared"), Ownership::Shared);
        assert_eq!(Ownership::parse("unknown"), Ownership::All);
    }
}
