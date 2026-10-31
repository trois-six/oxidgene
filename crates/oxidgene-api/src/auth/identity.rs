//! The identity a request carries (docs/authentication.md §5.4): the seam
//! the `ViewerContext` of [Authorization §4] is built on. The authentication
//! layer puts one [`Principal`] in every request's extensions; GraphQL
//! resolvers find it in their context.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use oxidgene_db::repo::InstanceRole;
use uuid::Uuid;

/// Who a request acts for.
#[derive(Debug, Clone)]
pub enum Principal {
    /// The `none` mode's single user, owner of every tree: issuer and
    /// subject `local`.
    Local,
    /// No credential.
    Anonymous,
    /// A signed-in user, through a session or a trusted client's token.
    User(Identity),
}

impl Principal {
    /// The signed-in user, if any.
    pub fn user(&self) -> Option<&Identity> {
        match self {
            Self::User(identity) => Some(identity),
            Self::Local | Self::Anonymous => None,
        }
    }

    /// The client the request came through, for logs, spans and the
    /// history: configuration, not personal data.
    pub fn client_id(&self) -> Option<&str> {
        self.user()
            .and_then(|identity| identity.client_id.as_deref())
    }
}

/// The identity of a signed-in request, from a token validated for it.
#[derive(Clone, PartialEq, Eq)]
pub struct Identity {
    /// The access token's `iss`.
    pub issuer: String,
    /// The access token's `sub`: never logged.
    pub subject: String,
    /// The user row the issuer and subject key.
    pub user_id: Uuid,
    /// The token's `client_id`, else `azp`.
    pub client_id: Option<String>,
    /// The token's scopes.
    pub scopes: Vec<String>,
    /// The digest of the session id, for a cookie session.
    pub session_id: Option<String>,
    /// The API client the `client_id` names, once API clients ship.
    pub api_client_id: Option<Uuid>,
    /// The user's instance role, read with the user row.
    pub instance_role: InstanceRole,
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The subject identifies a person at the provider: never printed.
        f.debug_struct("Identity")
            .field("user_id", &self.user_id)
            .field("client_id", &self.client_id)
            .field("scopes", &self.scopes)
            .field("instance_role", &self.instance_role)
            .finish_non_exhaustive()
    }
}

/// A handler's principal. A request the authentication layer did not see
/// is anonymous: absent means no credential, never full access.
impl<S: Send + Sync> FromRequestParts<S> for Principal {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(parts
            .extensions
            .get::<Self>()
            .cloned()
            .unwrap_or(Self::Anonymous))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_subject_is_never_printed() {
        let identity = Identity {
            issuer: "https://id.example.invalid".into(),
            subject: "subject-value".into(),
            user_id: Uuid::nil(),
            client_id: Some("oxidgene-web".into()),
            scopes: vec![],
            session_id: Some("session-digest".into()),
            api_client_id: None,
            instance_role: InstanceRole::User,
        };
        let printed = format!("{:?}", Principal::User(identity));
        assert!(!printed.contains("subject-value"), "{printed}");
        assert!(!printed.contains("session-digest"), "{printed}");
        assert!(printed.contains("oxidgene-web"), "{printed}");
    }
}
