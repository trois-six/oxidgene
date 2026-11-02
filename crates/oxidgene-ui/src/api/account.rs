//! Client calls of the web build's accounts (docs/authentication.md §4.2,
//! §6.6, §6.7, docs/authorization.md §10): the browser session, the signed-in
//! user's profile, avatar and sessions, and the administrator's user list.
//!
//! None of these routes exists on the desktop or in the web build's `none`
//! mode; only the account UI calls them, once `/auth/session` has answered
//! `oidc` (`crate::account`). Nothing here is cached: every answer describes
//! a session or an account that another device may change at any time.

use chrono::{DateTime, Utc};
use oxidgene_core::access::{InstanceRole, UserStatus};
use oxidgene_core::types::Connection;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{ApiClient, ApiError};

/// What `GET /auth/session` answers, always with `200`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AuthSession {
    /// `oidc` or `none`.
    pub mode: String,
    pub authenticated: bool,
    #[serde(default)]
    pub user: Option<SessionUser>,
    #[serde(default)]
    pub csrf_token: Option<String>,
    /// The provider's account page, for the password and second factor.
    #[serde(default)]
    pub account_url: Option<String>,
}

impl AuthSession {
    /// Whether the server signs users in, as opposed to the `none` mode.
    pub fn signs_in(&self) -> bool {
        self.mode == "oidc"
    }
}

/// The signed-in user as `/auth/session` names them.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SessionUser {
    pub id: Uuid,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub email_verified: bool,
    #[serde(default)]
    pub locale: Option<String>,
    pub instance_role: InstanceRole,
    #[serde(default)]
    pub can_create_trees: bool,
}

/// An uploaded avatar's version; `None` where the avatar is generated.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AvatarInfo {
    /// The version its address carries (`docs/architecture.md` §4.3).
    pub version: String,
    pub updated_at: DateTime<Utc>,
}

/// The signed-in user's own account: `GET /api/v1/me`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Me {
    pub id: Uuid,
    #[serde(default)]
    pub display_name: Option<String>,
    /// The provider's name, which returns when the user clears their own.
    #[serde(default)]
    pub provider_name: Option<String>,
    #[serde(default)]
    pub avatar: Option<AvatarInfo>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub email_verified: bool,
    #[serde(default)]
    pub locale: Option<String>,
    pub instance_role: InstanceRole,
    #[serde(default)]
    pub can_create_trees: bool,
}

impl Me {
    /// The account as far as `/auth/session` describes it, before — or
    /// without — `/me`: no avatar and no provider's name.
    pub fn from_session(user: &SessionUser) -> Self {
        Self {
            id: user.id,
            display_name: user.display_name.clone(),
            provider_name: None,
            avatar: None,
            email: user.email.clone(),
            email_verified: user.email_verified,
            locale: user.locale.clone(),
            instance_role: user.instance_role,
            can_create_trees: user.can_create_trees,
        }
    }

    /// Whether the user is an instance administrator.
    pub fn is_admin(&self) -> bool {
        self.instance_role == InstanceRole::Admin
    }
}

/// A change to the profile: `PUT /api/v1/me`. An absent field is kept;
/// `Some(None)` returns to the provider's value.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ProfilePatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<Option<String>>,
}

/// The square of the upright image an avatar is cut from, in its pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AvatarCrop {
    pub x: u32,
    pub y: u32,
    pub size: u32,
}

/// One of the signed-in user's sessions: `GET /api/v1/me/sessions`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SessionEntry {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub last_used_at: DateTime<Utc>,
    pub client_id: String,
    /// `firefox`, `chrome`, `edge`, `safari`, `opera` or `other`.
    pub browser: String,
    pub current: bool,
}

/// What a sign-out answers: the provider's end-session address, if any.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct LogoutAnswer {
    #[serde(default)]
    pub logout_url: Option<String>,
}

/// A user as an administrator manages them: `GET /api/v1/admin/users`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ManagedUser {
    pub id: Uuid,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub email_verified: bool,
    #[serde(default)]
    pub issuer: Option<String>,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub avatar: Option<AvatarInfo>,
    pub instance_role: InstanceRole,
    #[serde(default)]
    pub can_create_trees: bool,
    pub status: UserStatus,
    pub created_at: DateTime<Utc>,
    pub last_sign_in_at: DateTime<Utc>,
}

/// What an administrator changes on a user: `PUT /api/v1/admin/users/{id}`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct UserPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_role: Option<InstanceRole>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub can_create_trees: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<UserStatus>,
}

#[derive(Serialize)]
struct IdentityBody<'a> {
    issuer: &'a str,
    subject: &'a str,
}

/// The size of the administrator's user pages.
pub const USERS_PAGE_SIZE: u64 = 50;

impl ApiClient {
    /// A GET that is never cached.
    pub(super) async fn get_fresh<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<T, ApiError> {
        let response = self
            .send_request("GET", self.client.get(self.url(path)))
            .await?;
        Self::handle_response("GET", response).await
    }

    /// The browser's session, or that the server signs nobody in.
    pub async fn auth_session(&self) -> Result<AuthSession, ApiError> {
        self.get_fresh("/auth/session").await
    }

    /// The address that starts a sign-in returning to `return_to`, the
    /// provider's page shown in `lang` (docs/authentication.md §4.2).
    pub fn sign_in_url(&self, return_to: &str, lang: &str) -> String {
        self.url(&sign_in_path(return_to, lang))
    }

    /// Ends this session, or every session of the user; the answer names the
    /// provider's end-session address to go to.
    pub async fn sign_out(&self, everywhere: bool) -> Result<LogoutAnswer, ApiError> {
        let path = if everywhere {
            "/auth/logout-everywhere"
        } else {
            "/auth/logout"
        };
        let response = self
            .send_request("POST", self.client.post(self.url(path)))
            .await?;
        Self::handle_response("POST", response).await
    }

    pub async fn me(&self) -> Result<Me, ApiError> {
        self.get_fresh("/api/v1/me").await
    }

    pub async fn update_me(&self, patch: &ProfilePatch) -> Result<Me, ApiError> {
        self.put("/api/v1/me", patch).await
    }

    /// Replaces the avatar with `bytes`, cut to `crop`.
    pub async fn upload_avatar(
        &self,
        file_name: String,
        bytes: Vec<u8>,
        crop: AvatarCrop,
    ) -> Result<Me, ApiError> {
        let path = "/api/v1/me/avatar";
        let form = reqwest::multipart::Form::new()
            .part(
                "file",
                reqwest::multipart::Part::bytes(bytes).file_name(file_name),
            )
            .text("x", crop.x.to_string())
            .text("y", crop.y.to_string())
            .text("size", crop.size.to_string());
        let response = self
            .send_write("PUT", path, self.client.put(self.url(path)).multipart(form))
            .await?;
        Self::handle_response("PUT", response).await
    }

    /// Back to the generated avatar.
    pub async fn delete_avatar(&self) -> Result<(), ApiError> {
        self.delete_no_content("/api/v1/me/avatar").await
    }

    /// The address of a user's avatar at `size` pixels (64 or 256), at
    /// `version`: a replaced avatar is a new address, which the browser
    /// fetches once and keeps.
    #[must_use]
    pub fn avatar_url(&self, user_id: Uuid, size: u32, version: &str) -> String {
        self.url(&format!(
            "/api/v1/users/{user_id}/avatar?size={size}&v={version}"
        ))
    }

    pub async fn my_sessions(&self) -> Result<Vec<SessionEntry>, ApiError> {
        self.get_fresh("/api/v1/me/sessions").await
    }

    pub async fn end_my_session(&self, session_id: Uuid) -> Result<(), ApiError> {
        self.delete_no_content(&format!("/api/v1/me/sessions/{session_id}"))
            .await
    }

    /// Erases the account; the session ends with it.
    pub async fn delete_me(&self) -> Result<(), ApiError> {
        self.delete_no_content("/api/v1/me").await
    }

    /// A page of the instance's users, for an administrator.
    pub async fn admin_users(
        &self,
        after: Option<&str>,
    ) -> Result<Connection<ManagedUser>, ApiError> {
        let mut path = format!("/api/v1/admin/users?first={USERS_PAGE_SIZE}");
        if let Some(after) = after {
            path.push_str("&after=");
            path.push_str(&query_value(after));
        }
        self.get_fresh(&path).await
    }

    pub async fn update_user(
        &self,
        user_id: Uuid,
        patch: &UserPatch,
    ) -> Result<ManagedUser, ApiError> {
        self.put(&format!("/api/v1/admin/users/{user_id}"), patch)
            .await
    }

    /// Binds a user to another identity at the provider.
    pub async fn rebind_user_identity(
        &self,
        user_id: Uuid,
        issuer: &str,
        subject: &str,
    ) -> Result<ManagedUser, ApiError> {
        self.put(
            &format!("/api/v1/admin/users/{user_id}/identity"),
            &IdentityBody { issuer, subject },
        )
        .await
    }

    pub async fn erase_user(&self, user_id: Uuid) -> Result<(), ApiError> {
        self.delete_no_content(&format!("/api/v1/admin/users/{user_id}"))
            .await
    }
}

/// `value` encoded for a query string: every byte but the unreserved ones
/// of RFC 3986 percent-encoded.
fn query_value(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

/// The path of a sign-in that returns to `return_to`, a path of the
/// application, with the provider's page in `lang`.
pub fn sign_in_path(return_to: &str, lang: &str) -> String {
    format!(
        "/auth/login?return_to={}&lang={}",
        query_value(&safe_return_to(return_to)),
        query_value(lang)
    )
}

/// `path` when the server would accept it as a place to return to after a
/// sign-in (docs/authentication.md §4.3), the home page otherwise: a single
/// leading `/`, no scheme or authority, no backslash, no control character,
/// at most 2 KiB.
pub fn safe_return_to(path: &str) -> String {
    let acceptable = path.starts_with('/')
        && !path.starts_with("//")
        && !path.contains('\\')
        && !path.chars().any(char::is_control)
        && path.len() <= 2048;
    if acceptable {
        path.to_string()
    } else {
        "/".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sign_in_returns_to_the_page_it_left() {
        assert_eq!(
            sign_in_path("/trees/t/persons/p", "fr"),
            "/auth/login?return_to=%2Ftrees%2Ft%2Fpersons%2Fp&lang=fr"
        );
        // A page's own query survives as part of the path to return to.
        assert_eq!(
            sign_in_path("/trees/t/search?last=A b&first=", "en"),
            "/auth/login?return_to=%2Ftrees%2Ft%2Fsearch%3Flast%3DA%20b%26first%3D&lang=en"
        );
    }

    #[test]
    fn a_sign_in_never_returns_outside_the_application() {
        for elsewhere in [
            "https://elsewhere.example.invalid/",
            "//elsewhere.example.invalid/",
            "/\\elsewhere.example.invalid",
            "relative/path",
            "",
            "/line\nbreak",
        ] {
            assert_eq!(safe_return_to(elsewhere), "/", "{elsewhere:?}");
        }
        assert_eq!(safe_return_to(&format!("/{}", "a".repeat(2048))), "/");
        assert_eq!(safe_return_to("/settings"), "/settings");
    }

    #[test]
    fn a_session_says_whether_the_server_signs_in() {
        let none: AuthSession =
            serde_json::from_str(r#"{"mode":"none","authenticated":false,"user":null}"#).unwrap();
        assert!(!none.signs_in());
        let signed: AuthSession = serde_json::from_str(
            r#"{"mode":"oidc","authenticated":true,"client_id":"web","csrf_token":"c",
                "user":{"id":"0195c0de-0000-7000-8000-000000000001","display_name":"Ada Example",
                "email":"ada@example.invalid","email_verified":true,"locale":null,
                "instance_role":"user","can_create_trees":true},
                "expires_at":"2026-10-31T12:00:00Z","idle_expires_at":"2026-10-31T10:00:00Z",
                "account_url":"https://idp.example.invalid/account"}"#,
        )
        .unwrap();
        assert!(signed.signs_in());
        assert_eq!(signed.csrf_token.as_deref(), Some("c"));
        let me = Me::from_session(signed.user.as_ref().unwrap());
        assert_eq!(me.display_name.as_deref(), Some("Ada Example"));
        assert!(!me.is_admin());
    }

    #[test]
    fn a_profile_patch_sends_only_what_changes() {
        let reset = ProfilePatch {
            display_name: Some(None),
            locale: None,
        };
        assert_eq!(
            serde_json::to_string(&reset).unwrap(),
            r#"{"display_name":null}"#
        );
        let renamed = ProfilePatch {
            display_name: Some(Some("Ada".into())),
            locale: Some(Some("fr".into())),
        };
        assert_eq!(
            serde_json::to_string(&renamed).unwrap(),
            r#"{"display_name":"Ada","locale":"fr"}"#
        );
    }
}
