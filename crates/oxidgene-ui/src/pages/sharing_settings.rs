//! The owner's sharing sections of the tree settings
//! (docs/authorization.md §11, docs/ui-settings.md §8c and §8d): **Members**
//! — who holds which role, sharing with someone by email, username or among
//! the people already shared with, the invitations waiting for an answer,
//! handing the tree over — and **Sharing** — the tree's visibility with its
//! visitor scope, and the share links. Both edit scopes with the one
//! [`ScopeEditor`], which counts what a scope reaches as it changes.
//!
//! The sections exist in the web build only, for the tree's owner: the
//! settings page offers them when the server signs users in and the
//! viewer holds `members_manage` ([`crate::sharing`]).

use chrono::{DateTime, NaiveDate, Utc};
use dioxus::prelude::*;
use oxidgene_core::access::TreeRole;
use oxidgene_core::sharing::{ScopeDefinition, ScopeKind, TreeVisibility};
use uuid::Uuid;

use super::home_sharing::role_key;
use super::settings::{SettingsCheck, parse_depth, root_person_row};
use crate::api::ApiClient;
use crate::api::sharing::{
    Invitation, Member, MemberPatchBody, ScopeView, ShareBody, ShareLink, ShareLinkBody,
    VisibilityBody,
};
use crate::components::confirm_dialog::ConfirmDialog;
use crate::components::copy_field::CopyField;
use crate::components::date_input::format_day;
use crate::components::person_picker::PersonPicker;
use crate::components::search_person::PersonSearchSummary;
use crate::i18n::{I18n, use_i18n};
use crate::ui_observability::use_ui_resource;

// ── Scopes ──────────────────────────────────────────────────────────────

/// A scope being edited: the whole tree, or a branch of a root person.
#[derive(Debug, Clone, PartialEq)]
pub struct ScopeDraft {
    /// A branch rather than the whole tree.
    pub branch: bool,
    pub root: Option<Uuid>,
    pub kind: ScopeKind,
    /// The generations as typed: empty for all of them.
    pub depth_text: String,
    pub include_self: bool,
    pub include_spouses: bool,
    pub collateral: bool,
}

impl Default for ScopeDraft {
    fn default() -> Self {
        Self {
            branch: false,
            root: None,
            kind: ScopeKind::DescendantsOf,
            depth_text: String::new(),
            include_self: true,
            include_spouses: false,
            collateral: false,
        }
    }
}

impl ScopeDraft {
    /// The draft of a stored scope, the whole tree without one.
    pub fn of(scope: Option<&ScopeView>) -> Self {
        let Some(scope) = scope else {
            return Self::default();
        };
        let definition = scope.definition;
        Self {
            branch: true,
            root: Some(definition.person_id),
            kind: definition.kind,
            depth_text: definition
                .depth
                .map(|depth| depth.to_string())
                .unwrap_or_default(),
            include_self: definition.include_self,
            include_spouses: definition.include_spouses,
            collateral: definition.collateral,
        }
    }

    /// What the draft asks for: `Ok(None)` for the whole tree, the
    /// definition of a branch, or the translation key of what is missing.
    pub fn definition(&self) -> Result<Option<ScopeDefinition>, &'static str> {
        if !self.branch {
            return Ok(None);
        }
        let root = self.root.ok_or("sharing.scope.incomplete")?;
        let (depth, invalid) = parse_depth(self.depth_text.trim());
        if invalid {
            return Err("settings.export_scope_depth_invalid");
        }
        Ok(Some(ScopeDefinition {
            kind: self.kind,
            person_id: root,
            depth: depth.and_then(|depth| u32::try_from(depth).ok()),
            include_self: self.include_self,
            include_spouses: self.include_spouses,
            collateral: self.collateral,
        }))
    }
}

/// What a stored scope reaches, in a line: its side, its root, its depth
/// and what it adds; a deleted root says the scope shares nobody.
pub fn scope_summary(i18n: &I18n, scope: Option<&ScopeView>) -> String {
    let Some(scope) = scope else {
        return i18n.t("sharing.scope.whole");
    };
    let definition = scope.definition;
    let root = scope
        .person_name
        .clone()
        .unwrap_or_else(|| i18n.t("common.unnamed"));
    let mut parts = vec![i18n.t_args(kind_key(definition.kind), &[("name", &root)])];
    if let Some(depth) = definition.depth {
        parts.push(i18n.t_args("settings.exclusion_depth", &[("count", &depth.to_string())]));
    }
    if definition.include_spouses {
        parts.push(i18n.t("sharing.scope.with_spouses"));
    }
    if definition.collateral {
        parts.push(i18n.t("sharing.scope.with_collateral"));
    }
    if scope.person_deleted {
        parts.push(i18n.t("sharing.scope.root_deleted"));
    }
    parts.join(" · ")
}

fn kind_key(kind: ScopeKind) -> &'static str {
    match kind {
        ScopeKind::AncestorsOf => "sharing.scope.ancestors_of",
        ScopeKind::DescendantsOf => "sharing.scope.descendants_of",
        ScopeKind::AncestorsAndDescendantsOf => "sharing.scope.both_of",
    }
}

/// The scope editor: the whole tree or a branch — its root person, its side,
/// its generations, the root itself, the spouses and the collateral — with
/// the number of persons it reaches and how many of them a Visitor sees,
/// counted by the server as the draft changes.
#[component]
pub fn ScopeEditor(tree_id: Uuid, draft: Signal<ScopeDraft>, id_prefix: String) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let current = draft();
    let root_api = api.clone();
    let root = current.root;
    let root_person = use_ui_resource("scope_root", move || {
        let api = root_api.clone();
        async move {
            let root = root?;
            api.get_person_profile(tree_id, root)
                .await
                .ok()
                .map(PersonSearchSummary::from)
        }
    });
    let preview = use_ui_resource("scope_preview", move || {
        let api = api.clone();
        let definition = draft().definition();
        async move {
            let definition = definition.ok()?;
            api.scope_preview(tree_id, definition.as_ref()).await.ok()
        }
    });
    let selected = current
        .root
        .map(|root| root_person_row(root, &root_person.read(), &i18n));
    let problem = current.definition().err();
    let mode_id = format!("{id_prefix}-mode");
    let depth_id = format!("{id_prefix}-depth");
    rsx! {
        div { class: "sharing-scope",
            div { class: "pf-gender-group settings-choices",
                for (branch, label) in [(false, "sharing.scope.whole"), (true, "sharing.scope.branch")] {
                    button {
                        key: "{branch}",
                        class: if current.branch == branch { "pf-gender-btn active" } else { "pf-gender-btn" },
                        r#type: "button",
                        onclick: move |_| draft.write().branch = branch,
                        {i18n.t(label)}
                    }
                }
            }
            if current.branch {
                PersonPicker {
                    tree_id,
                    selected,
                    search_placeholder: i18n.t("settings.export_scope_search"),
                    change_label: i18n.t("settings.export_scope_change"),
                    on_change: move |person: Option<Uuid>| draft.write().root = person,
                }
                label { class: "settings-check-label", r#for: "{mode_id}",
                    {i18n.t("settings.export_scope_mode")}
                }
                select {
                    id: "{mode_id}",
                    class: "settings-date-select",
                    onchange: move |e: Event<FormData>| {
                        draft.write().kind = match e.value().as_str() {
                            "ancestors" => ScopeKind::AncestorsOf,
                            "both" => ScopeKind::AncestorsAndDescendantsOf,
                            _ => ScopeKind::DescendantsOf,
                        };
                    },
                    option { value: "descendants", selected: current.kind == ScopeKind::DescendantsOf, {i18n.t("settings.export_scope_descendants")} }
                    option { value: "ancestors", selected: current.kind == ScopeKind::AncestorsOf, {i18n.t("settings.export_scope_ancestors")} }
                    option { value: "both", selected: current.kind == ScopeKind::AncestorsAndDescendantsOf, {i18n.t("settings.export_scope_both")} }
                }
                label { class: "settings-check-label", r#for: "{depth_id}",
                    {i18n.t("settings.export_scope_depth")}
                }
                input {
                    id: "{depth_id}",
                    class: "settings-date-select",
                    r#type: "number",
                    min: "1",
                    max: "30",
                    placeholder: i18n.t("settings.export_scope_depth_all"),
                    value: "{current.depth_text}",
                    onchange: move |e: Event<FormData>| draft.write().depth_text = e.value(),
                }
                SettingsCheck {
                    checked: current.include_self,
                    label: i18n.t("settings.export_scope_include_self"),
                    on_change: move |checked: bool| draft.write().include_self = checked,
                }
                SettingsCheck {
                    checked: current.include_spouses,
                    label: i18n.t("sharing.scope.include_spouses"),
                    on_change: move |checked: bool| draft.write().include_spouses = checked,
                }
                SettingsCheck {
                    checked: current.collateral,
                    label: i18n.t("sharing.scope.collateral"),
                    on_change: move |checked: bool| draft.write().collateral = checked,
                }
            }
            match (problem, &*preview.read()) {
                (Some(problem), _) => rsx! {
                    p { class: "field-hint field-hint-warn", {i18n.t(problem)} }
                },
                (None, Some(Some(preview))) => rsx! {
                    p { class: "settings-check-desc",
                        {i18n.t_plural("history.persons_count", preview.person_count as usize)}
                        " · "
                        {i18n.t_args("sharing.scope.visitor_count", &[("count", &preview.visitor_count.to_string())])}
                        if preview.excluded_count > 0 {
                            " · "
                            {i18n.t_args("sharing.scope.excluded_count", &[("count", &preview.excluded_count.to_string())])}
                        }
                    }
                },
                _ => rsx! {},
            }
        }
    }
}

/// The end of a day picked in a date field, as the server's expiry: none
/// for an empty field.
fn expiry_of(text: &str) -> Option<DateTime<Utc>> {
    let day = NaiveDate::parse_from_str(text.trim(), "%Y-%m-%d").ok()?;
    Some(day.and_hms_opt(23, 59, 59)?.and_utc())
}

/// An expiry as a line, or that there is none.
fn expiry_text(i18n: &I18n, expires_at: Option<DateTime<Utc>>) -> String {
    match expires_at {
        Some(at) => i18n.t_args(
            "sharing.expires_on",
            &[("date", &format_day(i18n, at.date_naive()))],
        ),
        None => i18n.t("sharing.no_expiry"),
    }
}

// ── Members ─────────────────────────────────────────────────────────────

/// The Members section: the members with their role, scope and expiry, the
/// form sharing the tree, the pending invitations, and the transfer of
/// ownership.
#[component]
pub fn MembersSection(tree_id: Uuid) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let mut refresh = use_signal(|| 0u32);
    let error = use_signal(|| None::<String>);
    let api_members = api.clone();
    let members = use_ui_resource("members", move || {
        let api = api_members.clone();
        let _tick = refresh();
        async move { api.tree_members(tree_id).await }
    });
    let api_invitations = api.clone();
    let invitations = use_ui_resource("invitations", move || {
        let api = api_invitations.clone();
        let _tick = refresh();
        async move { api.tree_invitations(tree_id).await }
    });
    let changed = move |()| refresh += 1;
    let member_rows = match &*members.read() {
        Some(Ok(rows)) => rows.clone(),
        _ => Vec::new(),
    };
    let invitation_rows = match &*invitations.read() {
        Some(Ok(rows)) => rows.clone(),
        _ => Vec::new(),
    };
    rsx! {
        div { class: "settings-section",
            div { class: "settings-section-eyebrow", {i18n.t("settings.breadcrumb")} }
            h2 { class: "settings-section-title", {i18n.t("sharing.members.title")} }
            p { class: "settings-section-subtitle", {i18n.t("sharing.members.subtitle")} }

            ShareForm { tree_id, on_shared: changed }

            div { class: "card settings-card",
                h3 { class: "settings-card-title", {i18n.t("sharing.members.list")} }
                for member in member_rows {
                    MemberRow { key: "{member.user_id}", tree_id, member, error, on_change: changed }
                }
            }

            div { class: "card settings-card",
                h3 { class: "settings-card-title", {i18n.t("sharing.invitations.pending")} }
                if invitation_rows.is_empty() {
                    p { class: "settings-check-desc", {i18n.t("sharing.invitations.none")} }
                }
                for invitation in invitation_rows {
                    InvitationRow { key: "{invitation.id}", tree_id, invitation, error, on_change: changed }
                }
            }
            if let Some(err) = error() {
                div { class: "error-msg settings-feedback", role: "alert", "{err}" }
            }
        }
    }
}

/// Share the tree: by exact email or username, or among the people the
/// owner already shares with; a role, an optional scope and expiry. An
/// address without an account becomes an invitation, whose link the owner
/// sends themselves while OxidGene mails nothing.
#[component]
fn ShareForm(tree_id: Uuid, on_shared: EventHandler<()>) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let mut target = use_signal(String::new);
    let mut candidate = use_signal(|| None::<Uuid>);
    let mut role = use_signal(|| TreeRole::Guest);
    let mut expiry = use_signal(String::new);
    let draft = use_signal(ScopeDraft::default);
    let mut error = use_signal(|| None::<String>);
    let mut invited = use_signal(|| None::<Invitation>);
    let api_candidates = api.clone();
    let candidates = use_ui_resource("share_candidates", move || {
        let api = api_candidates.clone();
        async move { api.share_candidates(tree_id).await.unwrap_or_default() }
    });
    let candidate_list = candidates.read().clone().unwrap_or_default();
    let submit = move |_| {
        let api = api.clone();
        let scope = match draft().definition() {
            Ok(scope) => scope,
            Err(problem) => {
                error.set(Some(i18n.t(problem)));
                return;
            }
        };
        let typed = target().trim().to_string();
        let mut body = ShareBody {
            role: Some(role()),
            scope,
            expires_at: expiry_of(&expiry()),
            ..ShareBody::default()
        };
        match (candidate(), typed.contains('@')) {
            (Some(user_id), _) => body.user_id = Some(user_id),
            (None, _) if typed.is_empty() => {
                error.set(Some(i18n.t("sharing.share.target_required")));
                return;
            }
            (None, true) => body.email = Some(typed),
            (None, false) => body.username = Some(typed),
        }
        spawn(async move {
            match api.share_tree(tree_id, &body).await {
                Ok(outcome) => {
                    error.set(None);
                    target.set(String::new());
                    candidate.set(None);
                    invited.set(outcome.invitation);
                    on_shared.call(());
                }
                Err(e) => error.set(Some(e.message(&i18n))),
            }
        });
    };
    rsx! {
        div { class: "card settings-card",
            h3 { class: "settings-card-title", {i18n.t("sharing.share.title")} }
            p { class: "settings-check-desc", {i18n.t("sharing.share.desc")} }
            label { class: "settings-check-label", r#for: "share-target", {i18n.t("sharing.share.target")} }
            input {
                id: "share-target",
                class: "settings-date-select",
                r#type: "text",
                autocomplete: "off",
                placeholder: i18n.t("sharing.share.target_placeholder"),
                value: "{target}",
                disabled: candidate().is_some(),
                oninput: move |e: Event<FormData>| target.set(e.value()),
            }
            if !candidate_list.is_empty() {
                label { class: "settings-check-label", r#for: "share-candidate", {i18n.t("sharing.share.candidate")} }
                select {
                    id: "share-candidate",
                    class: "settings-date-select",
                    onchange: move |e: Event<FormData>| candidate.set(e.value().parse::<Uuid>().ok()),
                    option { value: "", selected: candidate().is_none(), {i18n.t("sharing.share.candidate_none")} }
                    for person in candidate_list {
                        option {
                            key: "{person.user_id}",
                            value: "{person.user_id}",
                            selected: candidate() == Some(person.user_id),
                            {person.display_name.clone().unwrap_or_default()}
                        }
                    }
                }
            }
            label { class: "settings-check-label", r#for: "share-role", {i18n.t("sharing.share.role")} }
            select {
                id: "share-role",
                class: "settings-date-select",
                onchange: move |e: Event<FormData>| {
                    role.set(if e.value() == "editor" { TreeRole::Editor } else { TreeRole::Guest });
                },
                option { value: "guest", selected: role() == TreeRole::Guest, {i18n.t("sharing.role.guest")} }
                option { value: "editor", selected: role() == TreeRole::Editor, {i18n.t("sharing.role.editor")} }
            }
            p { class: "settings-check-desc", {i18n.t(role_hint(role()))} }
            label { class: "settings-check-label", {i18n.t("sharing.scope.title")} }
            ScopeEditor { tree_id, draft, id_prefix: "share-scope" }
            label { class: "settings-check-label", r#for: "share-expiry", {i18n.t("sharing.share.expiry")} }
            input {
                id: "share-expiry",
                class: "settings-date-select",
                r#type: "date",
                value: "{expiry}",
                oninput: move |e: Event<FormData>| expiry.set(e.value()),
            }
            div { class: "modal-actions",
                button { class: "btn btn-primary", r#type: "button", onclick: submit,
                    {i18n.t("sharing.share.submit")}
                }
            }
            if let Some(invitation) = invited() {
                div { class: "sharing-invited",
                    p { {i18n.t_args("sharing.share.invited", &[("email", &invitation.email)])} }
                    if invitation.manual() {
                        p { class: "settings-check-desc", {i18n.t("sharing.invitations.manual")} }
                        CopyField { label: i18n.t("sharing.invitations.link"), value: invitation.link.clone(), multiline: false }
                    }
                }
            }
            if let Some(err) = error() {
                div { class: "error-msg settings-feedback", role: "alert", "{err}" }
            }
        }
    }
}

/// What a role lets a member do, in a sentence.
fn role_hint(role: TreeRole) -> &'static str {
    match role {
        TreeRole::Editor => "sharing.role.editor_hint",
        _ => "sharing.role.guest_hint",
    }
}

/// One member: their name, role, scope and expiry; the owner changes the
/// role of another member, removes them, or hands them the tree.
#[component]
fn MemberRow(
    tree_id: Uuid,
    member: Member,
    error: Signal<Option<String>>,
    on_change: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let mut confirm = use_signal(|| None::<&'static str>);
    let owner = member.role == TreeRole::Owner;
    let user_id = member.user_id;
    let name = member
        .display_name
        .clone()
        .unwrap_or_else(|| i18n.t("sharing.former_member"));
    let details = [
        scope_summary(&i18n, member.scope.as_ref()),
        expiry_text(&i18n, member.expires_at),
    ]
    .join(" · ");
    let api_role = api.clone();
    let set_role = move |e: Event<FormData>| {
        let api = api_role.clone();
        let role = if e.value() == "editor" {
            TreeRole::Editor
        } else {
            TreeRole::Guest
        };
        let body = MemberPatchBody {
            role: Some(role),
            ..MemberPatchBody::default()
        };
        spawn(async move {
            match api.update_member(tree_id, user_id, &body).await {
                Ok(_) => on_change.call(()),
                Err(e) => error.set(Some(e.message(&i18n))),
            }
        });
    };
    let api_act = api.clone();
    let act = move |_| {
        let api = api_act.clone();
        let Some(action) = confirm() else { return };
        confirm.set(None);
        spawn(async move {
            let result = if action == "transfer" {
                api.transfer_ownership(tree_id, user_id).await.map(drop)
            } else {
                api.remove_member(tree_id, user_id).await
            };
            match result {
                Ok(()) => on_change.call(()),
                Err(e) => error.set(Some(e.message(&i18n))),
            }
        });
    };
    rsx! {
        div { class: "settings-export-row",
            div { class: "settings-export-info",
                div { class: "settings-check-label", "{name}" }
                p { class: "settings-check-desc", "{details}" }
            }
            if owner {
                span { class: "badge", {i18n.t("sharing.role.owner")} }
            } else {
                div { class: "sharing-row-actions",
                    select {
                        class: "settings-date-select",
                        "aria-label": i18n.t("sharing.share.role"),
                        onchange: set_role,
                        option { value: "guest", selected: member.role == TreeRole::Guest, {i18n.t("sharing.role.guest")} }
                        option { value: "editor", selected: member.role == TreeRole::Editor, {i18n.t("sharing.role.editor")} }
                    }
                    button {
                        class: "btn btn-outline btn-sm",
                        r#type: "button",
                        onclick: move |_| confirm.set(Some("transfer")),
                        {i18n.t("sharing.members.transfer")}
                    }
                    button {
                        class: "btn btn-outline btn-sm",
                        r#type: "button",
                        onclick: move |_| confirm.set(Some("remove")),
                        {i18n.t("sharing.members.remove")}
                    }
                }
            }
        }
        if let Some(action) = confirm() {
            ConfirmDialog {
                title: i18n.t(if action == "transfer" { "sharing.members.transfer_title" } else { "sharing.members.remove_title" }),
                message: i18n.t_args(
                    if action == "transfer" { "sharing.members.transfer_message" } else { "sharing.members.remove_message" },
                    &[("name", &name)],
                ),
                confirm_label: i18n.t(if action == "transfer" { "sharing.members.transfer" } else { "sharing.members.remove" }),
                confirm_class: "btn btn-danger".to_string(),
                on_confirm: act,
                on_cancel: move |_| confirm.set(None),
            }
        }
    }
}

/// One pending invitation: its address, role, scope and expiry, the link
/// to send by hand, **Resend** and **Revoke**.
#[component]
fn InvitationRow(
    tree_id: Uuid,
    invitation: Invitation,
    error: Signal<Option<String>>,
    on_change: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let id = invitation.id;
    let details = [
        i18n.t(role_key(invitation.role)),
        scope_summary(&i18n, invitation.scope.as_ref()),
        i18n.t_args(
            "sharing.invitations.waits_until",
            &[(
                "date",
                &format_day(&i18n, invitation.expires_at.date_naive()),
            )],
        ),
    ]
    .join(" · ");
    let api_resend = api.clone();
    rsx! {
        div { class: "settings-export-row",
            div { class: "settings-export-info",
                div { class: "settings-check-label", "{invitation.email}" }
                p { class: "settings-check-desc", "{details}" }
                if invitation.manual() {
                    CopyField { label: i18n.t("sharing.invitations.link"), value: invitation.link.clone(), multiline: false }
                }
            }
            div { class: "sharing-row-actions",
                button {
                    class: "btn btn-outline btn-sm",
                    r#type: "button",
                    onclick: move |_| {
                        let api = api_resend.clone();
                        spawn(async move {
                            match api.resend_invitation(tree_id, id).await {
                                Ok(_) => on_change.call(()),
                                Err(e) => error.set(Some(e.message(&i18n))),
                            }
                        });
                    },
                    {i18n.t("sharing.invitations.resend")}
                }
                button {
                    class: "btn btn-outline btn-sm",
                    r#type: "button",
                    onclick: move |_| {
                        let api = api.clone();
                        spawn(async move {
                            match api.revoke_invitation(tree_id, id).await {
                                Ok(()) => on_change.call(()),
                                Err(e) => error.set(Some(e.message(&i18n))),
                            }
                        });
                    },
                    {i18n.t("sharing.invitations.revoke")}
                }
            }
        }
    }
}

// ── Sharing ─────────────────────────────────────────────────────────────

/// The Sharing section: who the tree is visible to, with the visitor scope
/// and the number of persons Visitors will see, and the share links.
#[component]
pub fn SharingSection(tree_id: Uuid, visibility: TreeVisibility) -> Element {
    let i18n = use_i18n();
    rsx! {
        div { class: "settings-section",
            div { class: "settings-section-eyebrow", {i18n.t("settings.breadcrumb")} }
            h2 { class: "settings-section-title", {i18n.t("sharing.section.title")} }
            p { class: "settings-section-subtitle", {i18n.t("sharing.section.subtitle")} }
            VisibilityCard { key: "{tree_id}", tree_id, visibility }
            ShareLinksCard { tree_id }
        }
    }
}

/// The tree's visibility and its visitor scope, saved together once the
/// owner has seen how many persons Visitors will see.
#[component]
fn VisibilityCard(tree_id: Uuid, visibility: TreeVisibility) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let mut chosen = use_signal(|| visibility);
    let mut draft = use_signal(ScopeDraft::default);
    let mut error = use_signal(|| None::<String>);
    let mut saved = use_signal(|| false);
    let api_scope = api.clone();
    // The stored visitor scope fills the draft once it is read.
    use_future(move || {
        let api = api_scope.clone();
        async move {
            if let Ok(scope) = api.visitor_scope(tree_id).await {
                draft.set(ScopeDraft::of(scope.as_ref()));
            }
        }
    });
    let save = move |_| {
        let api = api.clone();
        let scope = match draft().definition() {
            Ok(scope) => scope,
            Err(problem) => {
                error.set(Some(i18n.t(problem)));
                return;
            }
        };
        let body = VisibilityBody {
            visibility: Some(chosen()),
            visitor_scope: Some(scope),
        };
        spawn(async move {
            match api.update_visibility(tree_id, &body).await {
                Ok(()) => {
                    error.set(None);
                    saved.set(true);
                }
                Err(e) => error.set(Some(e.message(&i18n))),
            }
        });
    };
    rsx! {
        div { class: "card settings-card",
            h3 { class: "settings-card-title", {i18n.t("sharing.visibility.title")} }
            div { class: "pf-gender-group settings-choices",
                for (value, label) in [
                    (TreeVisibility::Private, "sharing.visibility.private"),
                    (TreeVisibility::Site, "sharing.visibility.site"),
                    (TreeVisibility::Public, "sharing.visibility.public"),
                ] {
                    button {
                        key: "{value}",
                        class: if chosen() == value { "pf-gender-btn active" } else { "pf-gender-btn" },
                        r#type: "button",
                        onclick: move |_| {
                            chosen.set(value);
                            saved.set(false);
                        },
                        {i18n.t(label)}
                    }
                }
            }
            p { class: "settings-check-desc", {i18n.t(visibility_hint(chosen()))} }
            if chosen() != TreeVisibility::Private {
                label { class: "settings-check-label", {i18n.t("sharing.visibility.scope")} }
                ScopeEditor { tree_id, draft, id_prefix: "visitor-scope" }
                p { class: "settings-check-desc", {i18n.t("sharing.visibility.redaction")} }
            }
            div { class: "modal-actions",
                button { class: "btn btn-primary", r#type: "button", onclick: save,
                    {i18n.t("common.save")}
                }
            }
            if saved() {
                p { class: "settings-check-desc", role: "status", {i18n.t("sharing.visibility.saved")} }
            }
            if let Some(err) = error() {
                div { class: "error-msg settings-feedback", role: "alert", "{err}" }
            }
        }
    }
}

fn visibility_hint(visibility: TreeVisibility) -> &'static str {
    match visibility {
        TreeVisibility::Private => "sharing.visibility.private_hint",
        TreeVisibility::Site => "sharing.visibility.site_hint",
        TreeVisibility::Public => "sharing.visibility.public_hint",
    }
}

/// The share links: each with its label, scope, expiry and use, and
/// **Revoke**; a new one is created with a label, a scope and an expiry, its
/// address shown this once.
#[component]
fn ShareLinksCard(tree_id: Uuid) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let mut refresh = use_signal(|| 0u32);
    let mut label = use_signal(String::new);
    let mut expiry = use_signal(String::new);
    let draft = use_signal(ScopeDraft::default);
    let mut created = use_signal(|| None::<String>);
    let mut error = use_signal(|| None::<String>);
    let api_links = api.clone();
    let links = use_ui_resource("share_links", move || {
        let api = api_links.clone();
        let _tick = refresh();
        async move { api.share_links(tree_id).await }
    });
    let rows = match &*links.read() {
        Some(Ok(rows)) => rows.clone(),
        _ => Vec::new(),
    };
    let api_create = api.clone();
    let create = move |_| {
        let api = api_create.clone();
        let scope = match draft().definition() {
            Ok(scope) => scope,
            Err(problem) => {
                error.set(Some(i18n.t(problem)));
                return;
            }
        };
        let body = ShareLinkBody {
            label: Some(label().trim().to_string()).filter(|label| !label.is_empty()),
            scope,
            expires_at: expiry_of(&expiry()),
        };
        spawn(async move {
            match api.create_share_link(tree_id, &body).await {
                Ok(link) => {
                    error.set(None);
                    label.set(String::new());
                    created.set(Some(link.url));
                    refresh += 1;
                }
                Err(e) => error.set(Some(e.message(&i18n))),
            }
        });
    };
    let revoke = move |id: Uuid| {
        let api = api.clone();
        spawn(async move {
            match api.revoke_share_link(tree_id, id).await {
                Ok(()) => refresh += 1,
                Err(e) => error.set(Some(e.message(&i18n))),
            }
        });
    };
    rsx! {
        div { class: "card settings-card",
            h3 { class: "settings-card-title", {i18n.t("sharing.links.title")} }
            p { class: "settings-check-desc", {i18n.t("sharing.links.desc")} }
            if rows.is_empty() {
                p { class: "settings-check-desc", {i18n.t("sharing.links.none")} }
            }
            for link in rows {
                ShareLinkRow { key: "{link.id}", link, on_revoke: revoke.clone() }
            }
            h3 { class: "settings-card-title", {i18n.t("sharing.links.new")} }
            label { class: "settings-check-label", r#for: "link-label", {i18n.t("sharing.links.label")} }
            input {
                id: "link-label",
                class: "settings-date-select",
                r#type: "text",
                maxlength: "100",
                placeholder: i18n.t("sharing.links.label_placeholder"),
                value: "{label}",
                oninput: move |e: Event<FormData>| label.set(e.value()),
            }
            label { class: "settings-check-label", {i18n.t("sharing.scope.title")} }
            ScopeEditor { tree_id, draft, id_prefix: "link-scope" }
            label { class: "settings-check-label", r#for: "link-expiry", {i18n.t("sharing.share.expiry")} }
            input {
                id: "link-expiry",
                class: "settings-date-select",
                r#type: "date",
                value: "{expiry}",
                oninput: move |e: Event<FormData>| expiry.set(e.value()),
            }
            div { class: "modal-actions",
                button { class: "btn btn-primary", r#type: "button", onclick: create,
                    {i18n.t("sharing.links.create")}
                }
            }
            if let Some(url) = created() {
                div { class: "sharing-invited",
                    p { class: "settings-check-desc", {i18n.t("sharing.links.shown_once")} }
                    CopyField { label: i18n.t("sharing.links.address"), value: url, multiline: false }
                }
            }
            if let Some(err) = error() {
                div { class: "error-msg settings-feedback", role: "alert", "{err}" }
            }
        }
    }
}

#[component]
fn ShareLinkRow(link: ShareLink, on_revoke: EventHandler<Uuid>) -> Element {
    let i18n = use_i18n();
    let title = link
        .label
        .clone()
        .unwrap_or_else(|| i18n.t("sharing.links.unlabelled"));
    let mut details = vec![
        scope_summary(&i18n, link.scope.as_ref()),
        expiry_text(&i18n, link.expires_at),
        i18n.t_args(
            "sharing.links.uses",
            &[("count", &link.use_count.to_string())],
        ),
    ];
    if link.expired {
        details.push(i18n.t("sharing.links.expired"));
    }
    let details = details.join(" · ");
    let id = link.id;
    rsx! {
        div { class: "settings-export-row",
            div { class: "settings-export-info",
                div { class: "settings-check-label", "{title}" }
                p { class: "settings-check-desc", "{details}" }
            }
            button {
                class: "btn btn-outline btn-sm",
                r#type: "button",
                onclick: move |_| on_revoke.call(id),
                {i18n.t("sharing.links.revoke")}
            }
        }
    }
}

// ── The settings page's part ────────────────────────────────────────────

/// What the settings page offers of sharing on a tree: the owner's sections
/// to a viewer who manages the members, **Leave this tree** to another
/// member. Nothing without the sharing surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TreeSharing {
    pub tree_id: Option<Uuid>,
    /// The viewer manages the members: the owner.
    pub manages: bool,
    /// The signed-in member who may leave the tree, when the viewer is one.
    pub leaver: Option<Uuid>,
}

/// What the settings page of tree `tree_id` offers of sharing, from the
/// viewer's access to the tree.
pub fn use_tree_sharing(tree_id: Option<Uuid>) -> TreeSharing {
    let api = use_context::<ApiClient>();
    let account = crate::account::use_account();
    let access = use_ui_resource("viewer_access", move || {
        let api = api.clone();
        let surfaces = crate::sharing::offered(&account.read());
        async move {
            match tree_id.filter(|_| surfaces) {
                Some(tree) => api.tree_access(tree).await.ok(),
                None => None,
            }
        }
    });
    let Some(Some(access)) = &*access.read() else {
        return TreeSharing::default();
    };
    let me = account.read().me().map(|me| me.id);
    TreeSharing {
        tree_id,
        manages: access.allows(oxidgene_core::access::Permission::MembersManage),
        leaver: me.filter(|_| access.allows(oxidgene_core::access::Permission::Leave)),
    }
}

/// The owner's sections of the web build, after the tree's settings
/// (docs/ui-settings.md §4).
const SHARING_NAV: [(&str, &str); 2] = [
    ("members", "sharing.members.title"),
    ("sharing", "sharing.section.title"),
];

/// The owner's entries of the settings' left menu, at the end of its first
/// group.
#[component]
pub fn SharingNav(group: String, manages: bool, active_section: Signal<String>) -> Element {
    let i18n = use_i18n();
    if !manages || group != "settings.breadcrumb" {
        return rsx! {};
    }
    let current = active_section();
    rsx! {
        for (section, label) in SHARING_NAV {
            button {
                class: if current == section { "settings-nav-item active" } else { "settings-nav-item" },
                onclick: move |_| active_section.set(section.to_string()),
                {i18n.t(label)}
            }
        }
    }
}

/// The owner's section `section`: **Members** or **Sharing**; nothing for
/// anyone else.
#[component]
pub fn OwnerSection(section: String, sharing: TreeSharing, visibility: TreeVisibility) -> Element {
    let (true, Some(tree_id)) = (sharing.manages, sharing.tree_id) else {
        return rsx! {};
    };
    if section == "members" {
        rsx! { MembersSection { tree_id } }
    } else {
        rsx! { SharingSection { tree_id, visibility } }
    }
}

// ── Leaving ─────────────────────────────────────────────────────────────

/// **Leave this tree**, for a member who is not its owner, in Tree & Roots:
/// once confirmed, the membership ends and the homepage opens.
#[component]
pub fn LeaveTree(sharing: TreeSharing) -> Element {
    let (Some(tree_id), Some(user_id)) = (sharing.tree_id, sharing.leaver) else {
        return rsx! {};
    };
    rsx! { LeaveTreeCard { tree_id, user_id } }
}

#[component]
fn LeaveTreeCard(tree_id: Uuid, user_id: Uuid) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let nav = navigator();
    let mut confirming = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let leave = move |_| {
        let api = api.clone();
        confirming.set(false);
        spawn(async move {
            match api.remove_member(tree_id, user_id).await {
                Ok(()) => {
                    nav.push(crate::router::Route::Home {});
                }
                Err(e) => error.set(Some(e.message(&i18n))),
            }
        });
    };
    rsx! {
        div { class: "card settings-card",
            h3 { class: "settings-card-title", {i18n.t("sharing.leave.title")} }
            p { class: "settings-check-desc", {i18n.t("sharing.leave.desc")} }
            div { class: "modal-actions",
                button {
                    class: "btn btn-danger",
                    r#type: "button",
                    onclick: move |_| confirming.set(true),
                    {i18n.t("sharing.leave.button")}
                }
            }
            if let Some(err) = error() {
                div { class: "error-msg settings-feedback", role: "alert", "{err}" }
            }
        }
        if confirming() {
            ConfirmDialog {
                title: i18n.t("sharing.leave.title"),
                message: i18n.t("sharing.leave.confirm"),
                confirm_label: i18n.t("sharing.leave.button"),
                confirm_class: "btn btn-danger".to_string(),
                on_confirm: leave,
                on_cancel: move |_| confirming.set(false),
            }
        }
    }
}

/// The styles of the sharing sections.
pub const SHARING_SETTINGS_STYLES: &str = r#"
    .sharing-scope {
        display: flex;
        flex-direction: column;
        gap: var(--space-4);
        margin-bottom: var(--space-6);
    }

    .sharing-row-actions {
        display: flex;
        align-items: center;
        gap: var(--space-4);
        flex-shrink: 0;
    }

    .sharing-invited {
        margin-top: var(--space-6);
        display: flex;
        flex-direction: column;
        gap: var(--space-3);
    }
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_draft_is_the_whole_tree_or_a_complete_branch() {
        let mut draft = ScopeDraft::default();
        assert_eq!(draft.definition(), Ok(None));
        draft.branch = true;
        assert_eq!(draft.definition(), Err("sharing.scope.incomplete"));
        let root = Uuid::now_v7();
        draft.root = Some(root);
        draft.depth_text = "3".into();
        let definition = draft.definition().unwrap().unwrap();
        assert_eq!(definition.person_id, root);
        assert_eq!(definition.depth, Some(3));
        draft.depth_text = "99".into();
        assert!(draft.definition().is_err());
    }

    #[test]
    fn an_expiry_is_the_end_of_the_day_picked() {
        let at = expiry_of("2031-05-17").unwrap();
        assert_eq!(at.to_rfc3339(), "2031-05-17T23:59:59+00:00");
        assert_eq!(expiry_of(""), None);
    }
}
