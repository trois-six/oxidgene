//! Client calls of sharing (docs/authorization.md §6, §10): a tree's
//! members, sharing with users and invitations, share links, the tree's
//! visitor scope and the preview of a scope, and the invitations addressed
//! to the signed-in user.
//!
//! None of these routes exists on the desktop or in the web build's `none`
//! mode; only the sharing surfaces call them, which appear once the server
//! says it authenticates its viewers ([`crate::sharing`]). Nothing here is
//! cached: the owner and the people they share with change it from other
//! devices.

use chrono::{DateTime, Utc};
use oxidgene_core::access::TreeRole;
use oxidgene_core::sharing::{ScopeDefinition, TreeVisibility};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{ApiClient, ApiError};

/// A stored scope: its definition, its root's name, and whether the root is
/// deleted, which leaves the scope empty until it is restored.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ScopeView {
    pub id: Uuid,
    #[serde(flatten)]
    pub definition: ScopeDefinition,
    pub person_name: Option<String>,
    #[serde(default)]
    pub person_deleted: bool,
}

/// A member of a tree.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Member {
    pub user_id: Uuid,
    pub display_name: Option<String>,
    pub role: TreeRole,
    #[serde(default)]
    pub scope: Option<ScopeView>,
    pub self_person_id: Option<Uuid>,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A pending invitation, as the owner sees it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Invitation {
    pub id: Uuid,
    pub email: String,
    pub role: TreeRole,
    #[serde(default)]
    pub scope: Option<ScopeView>,
    pub membership_expires_at: Option<DateTime<Utc>>,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub last_sent_at: Option<DateTime<Utc>>,
    /// The link to send by hand: it carries no secret.
    pub link: String,
    /// `email` when OxidGene mails invitations, `manual` otherwise.
    pub delivery: String,
}

impl Invitation {
    /// Whether the owner sends the link themselves.
    pub fn manual(&self) -> bool {
        self.delivery != "email"
    }
}

/// An invitation addressed to the signed-in user.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct MyInvitation {
    pub id: Uuid,
    pub tree_id: Uuid,
    pub tree_name: String,
    pub inviter_name: Option<String>,
    pub role: TreeRole,
    pub scoped: bool,
    pub membership_expires_at: Option<DateTime<Utc>>,
    pub expires_at: DateTime<Utc>,
}

/// One of the people the owner already shares with.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ShareCandidate {
    pub user_id: Uuid,
    pub display_name: Option<String>,
}

/// What sharing made: a membership, or an invitation.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ShareOutcome {
    pub member: Option<Member>,
    pub invitation: Option<Invitation>,
}

/// A share: one of `email`, `username` and `user_id`, the role, and
/// optionally a scope and the membership's expiry.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ShareBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<Uuid>,
    pub role: Option<TreeRole>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<ScopeDefinition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
}

/// A member update: `None` keeps a field, `Some(None)` clears it.
#[derive(Debug, Clone, Default, Serialize)]
pub struct MemberPatchBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<TreeRole>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<Option<ScopeDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<Option<DateTime<Utc>>>,
}

/// A share link, never its token.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ShareLink {
    pub id: Uuid,
    pub label: Option<String>,
    #[serde(default)]
    pub scope: Option<ScopeView>,
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub expired: bool,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub use_count: i64,
}

/// A link just created, with its URL: shown once.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct CreatedShareLink {
    pub link: ShareLink,
    pub url: String,
}

/// A link to create.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ShareLinkBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<ScopeDefinition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
}

/// What a scope reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct ScopePreview {
    pub person_count: u64,
    pub visitor_count: u64,
    pub excluded_count: u64,
}

#[derive(Serialize)]
struct PreviewBody<'a> {
    scope: Option<&'a ScopeDefinition>,
}

#[derive(Serialize)]
struct OwnershipBody {
    user_id: Uuid,
}

/// A tree's visibility and visitor scope, as the tree update sets them.
#[derive(Debug, Clone, Default, Serialize)]
pub struct VisibilityBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<TreeVisibility>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visitor_scope: Option<Option<ScopeDefinition>>,
}

/// Pages read at once for the owner's lists, which a tree keeps short.
const PAGE: u64 = 100;

impl ApiClient {
    /// The members of tree `tree_id`: all of them for the owner.
    pub async fn tree_members(&self, tree_id: Uuid) -> Result<Vec<Member>, ApiError> {
        self.collect_fresh_pages(&format!("/api/v1/trees/{tree_id}/members"), PAGE)
            .await
    }

    pub async fn share_tree(
        &self,
        tree_id: Uuid,
        body: &ShareBody,
    ) -> Result<ShareOutcome, ApiError> {
        self.post(&format!("/api/v1/trees/{tree_id}/members"), body)
            .await
    }

    pub async fn update_member(
        &self,
        tree_id: Uuid,
        user_id: Uuid,
        body: &MemberPatchBody,
    ) -> Result<Member, ApiError> {
        self.put(&format!("/api/v1/trees/{tree_id}/members/{user_id}"), body)
            .await
    }

    /// Ends a membership: the owner removes a member, a member leaves.
    pub async fn remove_member(&self, tree_id: Uuid, user_id: Uuid) -> Result<(), ApiError> {
        self.delete_no_content(&format!("/api/v1/trees/{tree_id}/members/{user_id}"))
            .await?;
        self.invalidate_tree_list();
        Ok(())
    }

    pub async fn transfer_ownership(
        &self,
        tree_id: Uuid,
        user_id: Uuid,
    ) -> Result<Member, ApiError> {
        let member = self
            .post(
                &format!("/api/v1/trees/{tree_id}/ownership"),
                &OwnershipBody { user_id },
            )
            .await?;
        self.invalidate_tree_list();
        Ok(member)
    }

    pub async fn share_candidates(&self, tree_id: Uuid) -> Result<Vec<ShareCandidate>, ApiError> {
        self.get_fresh(&format!("/api/v1/trees/{tree_id}/share-candidates"))
            .await
    }

    pub async fn tree_invitations(&self, tree_id: Uuid) -> Result<Vec<Invitation>, ApiError> {
        self.collect_fresh_pages(&format!("/api/v1/trees/{tree_id}/invitations"), PAGE)
            .await
    }

    pub async fn resend_invitation(
        &self,
        tree_id: Uuid,
        invitation_id: Uuid,
    ) -> Result<Invitation, ApiError> {
        self.post(
            &format!("/api/v1/trees/{tree_id}/invitations/{invitation_id}/resend"),
            &serde_json::json!({}),
        )
        .await
    }

    pub async fn revoke_invitation(
        &self,
        tree_id: Uuid,
        invitation_id: Uuid,
    ) -> Result<(), ApiError> {
        self.delete_no_content(&format!(
            "/api/v1/trees/{tree_id}/invitations/{invitation_id}"
        ))
        .await
    }

    pub async fn my_invitations(&self) -> Result<Vec<MyInvitation>, ApiError> {
        self.get_fresh("/api/v1/me/invitations").await
    }

    /// Accepts an invitation: the tree joins the user's list.
    pub async fn accept_invitation(&self, invitation_id: Uuid) -> Result<(), ApiError> {
        self.post_no_content(
            &format!("/api/v1/me/invitations/{invitation_id}/accept"),
            &serde_json::json!({}),
        )
        .await?;
        self.invalidate_tree_list();
        Ok(())
    }

    pub async fn decline_invitation(&self, invitation_id: Uuid) -> Result<(), ApiError> {
        self.post_no_content(
            &format!("/api/v1/me/invitations/{invitation_id}/decline"),
            &serde_json::json!({}),
        )
        .await
    }

    pub async fn share_links(&self, tree_id: Uuid) -> Result<Vec<ShareLink>, ApiError> {
        self.collect_fresh_pages(&format!("/api/v1/trees/{tree_id}/share-links"), PAGE)
            .await
    }

    pub async fn create_share_link(
        &self,
        tree_id: Uuid,
        body: &ShareLinkBody,
    ) -> Result<CreatedShareLink, ApiError> {
        self.post(&format!("/api/v1/trees/{tree_id}/share-links"), body)
            .await
    }

    pub async fn revoke_share_link(&self, tree_id: Uuid, link_id: Uuid) -> Result<(), ApiError> {
        self.delete_no_content(&format!("/api/v1/trees/{tree_id}/share-links/{link_id}"))
            .await
    }

    /// What `scope` reaches in tree `tree_id`, the whole tree without one.
    pub async fn scope_preview(
        &self,
        tree_id: Uuid,
        scope: Option<&ScopeDefinition>,
    ) -> Result<ScopePreview, ApiError> {
        self.post(
            &format!("/api/v1/trees/{tree_id}/access-scopes/preview"),
            &PreviewBody { scope },
        )
        .await
    }

    pub async fn visitor_scope(&self, tree_id: Uuid) -> Result<Option<ScopeView>, ApiError> {
        self.get_fresh(&format!("/api/v1/trees/{tree_id}/visitor-scope"))
            .await
    }

    /// Sets who the tree is visible to.
    pub async fn update_visibility(
        &self,
        tree_id: Uuid,
        body: &VisibilityBody,
    ) -> Result<(), ApiError> {
        let _: serde_json::Value = self.put(&format!("/api/v1/trees/{tree_id}"), body).await?;
        self.cache
            .invalidate_prefix(&format!("/api/v1/trees/{tree_id}"));
        self.invalidate_tree_list();
        Ok(())
    }

    /// Every page of the cursor-paginated list at `path`, never cached.
    async fn collect_fresh_pages<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        page_size: u64,
    ) -> Result<Vec<T>, ApiError> {
        self.cache.invalidate_prefix(path);
        self.collect_pages(path, page_size, Vec::new()).await
    }
}
