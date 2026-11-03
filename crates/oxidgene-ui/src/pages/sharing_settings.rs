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
use oxidgene_core::access::{Permission, TreeRole};
use oxidgene_core::sharing::{ScopeDefinition, ScopeKind, TreeVisibility};
use uuid::Uuid;

use super::home_sharing::role_key;
use super::settings::{SettingsCheck, parse_depth, root_person_row};
use crate::api::sharing::{
    Invitation, Member, MemberPatchBody, ScopeView, ShareBody, ShareLink, ShareLinkBody,
    VisibilityBody,
};
use crate::api::{ApiClient, ApiError};
use crate::components::confirm_dialog::ConfirmDialog;
use crate::components::copy_field::CopyField;
use crate::components::date_input::format_day;
use crate::components::person_picker::PersonPicker;
use crate::components::search_person::PersonSearchSummary;
use crate::components::text_field::TextInput;
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

/// What `lookup` answers for the root person of `draft`, looked up again
/// each time the owner picks another root. The root is read inside the
/// resource: a value captured at the first render would name that root
/// for good, while the draft saved another.
fn use_draft_root<T, F>(
    draft: Signal<ScopeDraft>,
    mut lookup: impl FnMut(Uuid) -> F + 'static,
) -> Resource<Option<T>>
where
    T: 'static,
    F: Future<Output = Option<T>> + 'static,
{
    let root = use_memo(move || draft().root);
    use_ui_resource("scope_root", move || {
        let pending = root().map(&mut lookup);
        async move {
            match pending {
                Some(pending) => pending.await,
                None => None,
            }
        }
    })
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
    let root_person = use_draft_root(draft, move |root| {
        let api = root_api.clone();
        async move {
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

/// A list as read for one of the owner's cards.
#[derive(Debug, PartialEq)]
pub(crate) struct Listed<T> {
    pub rows: Vec<T>,
    /// Why the read failed, translated.
    pub error: Option<String>,
    /// The read succeeded and found nothing: the only case for the card's
    /// "none" line.
    pub empty: bool,
}

impl<T: Clone> Listed<T> {
    /// The list of `read`. A failed read must not pass for an empty list:
    /// "no share link" or "no exclusion" would tell the owner that nothing
    /// is shared, or nobody protected, when the request only failed.
    pub(crate) fn of(read: &Option<Result<Vec<T>, ApiError>>, i18n: &I18n) -> Self {
        match read {
            Some(Ok(rows)) => Self {
                rows: rows.clone(),
                error: None,
                empty: rows.is_empty(),
            },
            Some(Err(e)) => Self {
                rows: Vec::new(),
                error: Some(e.message(i18n)),
                empty: false,
            },
            None => Self {
                rows: Vec::new(),
                error: None,
                empty: false,
            },
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
    let member_list = Listed::of(&members.read(), &i18n);
    let invitation_list = Listed::of(&invitations.read(), &i18n);
    rsx! {
        div { class: "settings-section",
            div { class: "settings-section-eyebrow", {i18n.t("settings.breadcrumb")} }
            h2 { class: "settings-section-title", {i18n.t("sharing.members.title")} }
            p { class: "settings-section-subtitle", {i18n.t("sharing.members.subtitle")} }

            ShareForm { tree_id, on_shared: changed }

            div { class: "card settings-card",
                h3 { class: "settings-card-title", {i18n.t("sharing.members.list")} }
                for member in member_list.rows {
                    MemberRow { key: "{member.user_id}", tree_id, member, error, on_change: changed }
                }
                if let Some(err) = member_list.error {
                    div { class: "error-msg settings-feedback", role: "alert", "{err}" }
                }
            }

            div { class: "card settings-card",
                h3 { class: "settings-card-title", {i18n.t("sharing.invitations.pending")} }
                if invitation_list.empty {
                    p { class: "settings-check-desc", {i18n.t("sharing.invitations.none")} }
                }
                for invitation in invitation_list.rows {
                    InvitationRow { key: "{invitation.id}", tree_id, invitation, error, on_change: changed }
                }
                if let Some(err) = invitation_list.error {
                    div { class: "error-msg settings-feedback", role: "alert", "{err}" }
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
        async move { api.share_candidates(tree_id).await }
    });
    let Listed {
        rows: candidate_list,
        error: candidates_error,
        ..
    } = Listed::of(&candidates.read(), &i18n);
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
            TextInput {
                id: "share-target",
                class: "settings-date-select",
                r#type: "text",
                autocomplete: "off",
                placeholder: i18n.t("sharing.share.target_placeholder"),
                value: "{target}",
                disabled: candidate().is_some(),
                on_input: move |text: String| target.set(text),
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
            if let Some(err) = candidates_error {
                div { class: "error-msg settings-feedback", role: "alert", "{err}" }
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
            TextInput {
                id: "share-expiry",
                class: "settings-date-select",
                r#type: "date",
                value: "{expiry}",
                on_input: move |text: String| expiry.set(text),
            }
            div { class: "modal-actions",
                button { class: "btn btn-primary", r#type: "button", onclick: submit,
                    {i18n.t("sharing.share.submit")}
                }
            }
            if let Some(invitation) = invited() {
                div { class: "sharing-invited",
                    if invitation.manual() {
                        p { {i18n.t_args("sharing.share.invited", &[("email", &invitation.email)])} }
                    } else {
                        p { {i18n.t_args("sharing.share.invited_mailed", &[("email", &invitation.email)])} }
                    }
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
    // A transfer makes the viewer an Editor: what the pages offer follows.
    let refresh_access = crate::permissions::use_refresh_tree_permissions();
    let act = move |_| {
        let api = api_act.clone();
        let Some(action) = confirm() else { return };
        confirm.set(None);
        spawn(async move {
            let transfer = action == "transfer";
            let result = if transfer {
                api.transfer_ownership(tree_id, user_id).await.map(drop)
            } else {
                api.remove_member(tree_id, user_id).await
            };
            match result {
                Ok(()) => {
                    on_change.call(());
                    if transfer {
                        refresh_access.call(());
                    }
                }
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
/// and the number of persons Visitors will see, and the share links. The
/// visibility card waits for the tree's stored visibility: drawn before it,
/// it would offer **Private** as the current choice.
#[component]
pub fn SharingSection(tree_id: Uuid, visibility: Option<TreeVisibility>) -> Element {
    let i18n = use_i18n();
    rsx! {
        div { class: "settings-section",
            div { class: "settings-section-eyebrow", {i18n.t("settings.breadcrumb")} }
            h2 { class: "settings-section-title", {i18n.t("sharing.section.title")} }
            p { class: "settings-section-subtitle", {i18n.t("sharing.section.subtitle")} }
            if let Some(visibility) = visibility {
                VisibilityCard { key: "{tree_id}", tree_id, visibility }
            }
            ShareLinksCard { tree_id }
        }
    }
}

/// The update **Save** sends from the visibility card. `None` until the
/// stored visitor scope is read: the draft would then stand for a scope
/// nobody read, and could widen it to the whole tree. The scope is sent
/// only when the owner changed it, so the visibility alone leaves the
/// stored scope as it is. `Err` carries the translation key of what the
/// changed scope lacks.
fn visibility_update(
    chosen: TreeVisibility,
    stored: Option<&ScopeDraft>,
    draft: &ScopeDraft,
) -> Option<Result<VisibilityBody, &'static str>> {
    let stored = stored?;
    let visitor_scope = if draft == stored {
        None
    } else {
        match draft.definition() {
            Ok(scope) => Some(scope),
            Err(problem) => return Some(Err(problem)),
        }
    };
    Some(Ok(VisibilityBody {
        visibility: Some(chosen),
        visitor_scope,
    }))
}

/// The tree's visibility and its visitor scope, saved together once the
/// owner has seen how many persons Visitors will see.
#[component]
fn VisibilityCard(tree_id: Uuid, visibility: TreeVisibility) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let mut chosen = use_signal(|| visibility);
    let mut draft = use_signal(ScopeDraft::default);
    // The stored visitor scope as read, or why it could not be: Save waits
    // for it, and the draft is compared with it.
    let mut stored = use_signal(|| None::<Result<ScopeDraft, String>>);
    let mut error = use_signal(|| None::<String>);
    let mut saved = use_signal(|| false);
    let api_scope = api.clone();
    use_future(move || {
        let api = api_scope.clone();
        async move {
            let read = api
                .visitor_scope(tree_id)
                .await
                .map(|scope| ScopeDraft::of(scope.as_ref()))
                .map_err(|e| e.message(&i18n));
            if let Ok(scope) = &read {
                draft.set(scope.clone());
            }
            stored.set(Some(read));
        }
    });
    let save = move |_| {
        let api = api.clone();
        let current = draft();
        let read = stored();
        let read = read.as_ref().and_then(|read| read.as_ref().ok());
        let body = match visibility_update(chosen(), read, &current) {
            None => return,
            Some(Ok(body)) => body,
            Some(Err(problem)) => {
                error.set(Some(i18n.t(problem)));
                return;
            }
        };
        spawn(async move {
            match api.update_visibility(tree_id, &body).await {
                Ok(()) => {
                    error.set(None);
                    saved.set(true);
                    // What is stored now is the draft just sent.
                    stored.set(Some(Ok(current)));
                }
                Err(e) => error.set(Some(e.message(&i18n))),
            }
        });
    };
    let (readable, load_error) = match stored() {
        Some(Ok(_)) => (true, None),
        Some(Err(e)) => (false, Some(e)),
        None => (false, None),
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
            if chosen() != TreeVisibility::Private && readable {
                label { class: "settings-check-label", {i18n.t("sharing.visibility.scope")} }
                ScopeEditor { tree_id, draft, id_prefix: "visitor-scope" }
                p { class: "settings-check-desc", {i18n.t("sharing.visibility.redaction")} }
            }
            div { class: "modal-actions",
                button {
                    class: "btn btn-primary",
                    r#type: "button",
                    disabled: !readable,
                    onclick: save,
                    {i18n.t("common.save")}
                }
            }
            if saved() {
                p { class: "settings-check-desc", role: "status", {i18n.t("sharing.visibility.saved")} }
            }
            if let Some(err) = load_error {
                div { class: "error-msg settings-feedback", role: "alert", "{err}" }
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
    let listed = Listed::of(&links.read(), &i18n);
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
            if listed.empty {
                p { class: "settings-check-desc", {i18n.t("sharing.links.none")} }
            }
            for link in listed.rows {
                ShareLinkRow { key: "{link.id}", link, on_revoke: revoke.clone() }
            }
            if let Some(err) = listed.error {
                div { class: "error-msg settings-feedback", role: "alert", "{err}" }
            }
            h3 { class: "settings-card-title", {i18n.t("sharing.links.new")} }
            label { class: "settings-check-label", r#for: "link-label", {i18n.t("sharing.links.label")} }
            TextInput {
                id: "link-label",
                class: "settings-date-select",
                r#type: "text",
                maxlength: "100",
                placeholder: i18n.t("sharing.links.label_placeholder"),
                value: "{label}",
                on_input: move |text: String| label.set(text),
            }
            label { class: "settings-check-label", {i18n.t("sharing.scope.title")} }
            ScopeEditor { tree_id, draft, id_prefix: "link-scope" }
            label { class: "settings-check-label", r#for: "link-expiry", {i18n.t("sharing.share.expiry")} }
            TextInput {
                id: "link-expiry",
                class: "settings-date-select",
                r#type: "date",
                value: "{expiry}",
                on_input: move |text: String| expiry.set(text),
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
    let account = crate::account::use_account();
    let permissions = crate::permissions::use_tree_permissions();
    if !permissions.shares() {
        return TreeSharing::default();
    }
    let me = account.read().me().map(|me| me.id);
    TreeSharing {
        tree_id,
        manages: permissions.can(Permission::MembersManage),
        leaver: me.filter(|_| permissions.can(Permission::Leave)),
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
pub fn OwnerSection(
    section: String,
    sharing: TreeSharing,
    visibility: Option<TreeVisibility>,
) -> Element {
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

    fn branch_of(root: Uuid) -> ScopeDraft {
        ScopeDraft {
            branch: true,
            root: Some(root),
            ..ScopeDraft::default()
        }
    }

    /// The visibility card used to send its draft — the whole tree until the
    /// stored scope was read — with every save: a failed or slow read then
    /// widened a branch scope to the whole tree for every anonymous visitor.
    #[test]
    fn saving_the_visibility_never_sends_a_visitor_scope_nobody_read() {
        let stored = branch_of(Uuid::from_u128(7));

        // Not read yet, or the read failed: nothing may be sent.
        assert!(visibility_update(TreeVisibility::Public, None, &ScopeDraft::default()).is_none());

        // Read and untouched: the visibility alone, the scope left out.
        let body = visibility_update(TreeVisibility::Public, Some(&stored), &stored)
            .unwrap()
            .unwrap();
        assert_eq!(
            serde_json::to_value(&body).unwrap(),
            serde_json::json!({"visibility": "public"})
        );

        // The owner chose the whole tree: sent as such, on purpose.
        let body = visibility_update(TreeVisibility::Site, Some(&stored), &ScopeDraft::default())
            .unwrap()
            .unwrap();
        assert_eq!(
            serde_json::to_value(&body).unwrap(),
            serde_json::json!({"visibility": "site", "visitor_scope": null})
        );

        // Another root: the new branch.
        let other = Uuid::from_u128(8);
        let body = visibility_update(TreeVisibility::Public, Some(&stored), &branch_of(other))
            .unwrap()
            .unwrap();
        assert_eq!(
            body.visitor_scope.flatten().map(|scope| scope.person_id),
            Some(other)
        );

        // A branch without its root is refused, not sent as the whole tree.
        let incomplete = ScopeDraft {
            root: None,
            ..stored.clone()
        };
        assert_eq!(
            visibility_update(TreeVisibility::Public, Some(&stored), &incomplete)
                .unwrap()
                .unwrap_err(),
            "sharing.scope.incomplete"
        );
    }

    /// Probe of the root the editor's row names after each render.
    #[derive(Clone, Default)]
    struct RootProbe(std::rc::Rc<std::cell::RefCell<Vec<Option<Option<Uuid>>>>>);

    impl PartialEq for RootProbe {
        fn eq(&self, other: &Self) -> bool {
            std::rc::Rc::ptr_eq(&self.0, &other.0)
        }
    }

    fn root_host(probe: RootProbe) -> Element {
        let mut draft = use_signal(|| branch_of(Uuid::from_u128(1)));
        let named = use_draft_root(draft, |root| async move { Some(root) });
        // The owner picks another root once the first one is named.
        use_effect(move || {
            if *named.read() == Some(Some(Uuid::from_u128(1))) {
                draft.write().root = Some(Uuid::from_u128(2));
            }
        });
        probe.0.borrow_mut().push(*named.read());
        rsx! {}
    }

    /// The row once named the first root for good, while the draft saved
    /// the one picked after it.
    #[tokio::test]
    async fn the_scope_root_row_follows_the_root_picked() {
        let probe = RootProbe::default();
        let mut dom = VirtualDom::new_with_props(root_host, probe.clone());
        dom.rebuild_in_place();
        for _ in 0..8 {
            let _ = tokio::time::timeout(std::time::Duration::from_millis(50), dom.wait_for_work())
                .await;
            dom.render_immediate(&mut dioxus::core::NoOpMutations);
        }
        let seen = probe.0.borrow();
        assert!(seen.contains(&Some(Some(Uuid::from_u128(1)))));
        assert_eq!(seen.last(), Some(&Some(Some(Uuid::from_u128(2)))));
    }

    /// A failed read of the owner's lists used to show "No share link" or
    /// "No exclusion" while live links or exclusions existed.
    #[test]
    fn a_failed_list_read_is_an_error_not_an_empty_list() {
        let i18n = I18n::new(crate::i18n::Language::english());
        let failed: Option<Result<Vec<u8>, ApiError>> = Some(Err(ApiError::Api {
            status: 500,
            body: String::new(),
        }));
        let listed = Listed::of(&failed, &i18n);
        assert!(!listed.empty);
        assert!(listed.rows.is_empty());
        assert!(listed.error.is_some_and(|error| !error.is_empty()));

        let pending: Option<Result<Vec<u8>, ApiError>> = None;
        let listed = Listed::of(&pending, &i18n);
        assert!(!listed.empty);
        assert_eq!(listed.error, None);

        let listed = Listed::of(&Some(Ok(Vec::<u8>::new())), &i18n);
        assert!(listed.empty);
        assert_eq!(listed.error, None);

        let listed = Listed::of(&Some(Ok(vec![1u8])), &i18n);
        assert!(!listed.empty);
        assert_eq!(listed.rows, [1]);
    }

    #[test]
    fn an_expiry_is_the_end_of_the_day_picked() {
        let at = expiry_of("2031-05-17").unwrap();
        assert_eq!(at.to_rfc3339(), "2031-05-17T23:59:59+00:00");
        assert_eq!(expiry_of(""), None);
    }
}
