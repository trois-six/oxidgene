//! Authentication of the web build (docs/authentication.md): the `auth`
//! feature, which the standalone server and its worker enable and the
//! desktop never does (§7).
//!
//! - [`config`] reads `OXIDGENE_AUTH_MODE` and the OpenID Connect settings.
//! - `provider` speaks to the identity provider over `reqwest`: discovery,
//!   JWKS, the token and introspection endpoints.
//! - `token` validates access, ID and logout tokens.
//! - [`store`] keeps sessions in the process or in Redis, `crypto` seals
//!   their tokens and keeps only digests of their identifiers.
//! - `layer` turns each request's cookie or bearer token into the
//!   [`Principal`] every handler and resolver receives; `bff` answers the
//!   `/auth/*` routes of the Backend-for-Frontend.
//!
//! The `none` mode gives every request the local owner of every tree, as the
//! server behaved before authentication existed.

pub(crate) mod bff;
pub mod config;
mod cookie;
pub(crate) mod crypto;
pub mod identity;
pub(crate) mod layer;
mod provider;
mod session;
pub mod store;
mod token;

use std::sync::Arc;

use oxidgene_core::error::OxidGeneError;
use uuid::Uuid;

pub use config::{AuthConfig, AuthConfigError, AuthSettings};
pub use identity::{Identity, Principal};
pub use store::{MemoryStore, RedisStore, SessionStore, StoreUnavailable};

use config::{OidcConfig, StoreKind};
use crypto::Sealer;
use provider::Provider;
use token::{IntrospectionCache, Validator};

use crate::service::account::SignupPolicy;
use crate::service::notification::Mailing;
use crate::service::sharing::{InvitationOutbox, NoOutbox, SharingEnv};

/// The authentication of one server: its mode and, in `oidc`, the provider,
/// the validator and the session store.
#[derive(Debug)]
pub struct Auth {
    mode: Mode,
    /// Where invitations are mailed: nowhere until the server, given an
    /// SMTP relay, sets the mail outbox.
    outbox: Arc<dyn InvitationOutbox>,
}

#[derive(Debug)]
enum Mode {
    None,
    Oidc(Box<Oidc>),
}

/// Everything the `oidc` mode holds.
#[derive(Debug)]
pub(crate) struct Oidc {
    pub(crate) config: OidcConfig,
    pub(crate) provider: Provider,
    pub(crate) validator: Validator,
    pub(crate) store: Arc<dyn SessionStore>,
    pub(crate) sealer: Sealer,
    pub(crate) introspection: IntrospectionCache,
}

impl Auth {
    /// The `none` mode: every request acts as the local owner of every tree.
    pub fn none() -> Self {
        Self {
            mode: Mode::None,
            outbox: Arc::new(NoOutbox),
        }
    }

    /// The authentication `config` describes, its session store connected.
    ///
    /// # Errors
    ///
    /// When the Redis session store cannot be reached. The identity
    /// provider being unreachable is no error: discovery and the JWKS are
    /// fetched when first needed, and requests needing them answer
    /// `503 auth_unavailable` meanwhile (§3.4).
    pub async fn start(config: AuthConfig) -> Result<Self, StoreUnavailable> {
        let config = match config {
            AuthConfig::None => return Ok(Self::none()),
            AuthConfig::Oidc(config) => *config,
        };
        let store: Arc<dyn SessionStore> = match &config.store {
            StoreKind::Memory => Arc::new(MemoryStore::default()),
            StoreKind::Redis(url) => Arc::new(RedisStore::connect(url).await?),
        };
        Ok(Self::with_store(config, store))
    }

    /// The `oidc` mode over `store`.
    pub fn with_store(config: OidcConfig, store: Arc<dyn SessionStore>) -> Self {
        Self {
            outbox: Arc::new(NoOutbox),
            mode: Mode::Oidc(Box::new(Oidc {
                provider: Provider::new(&config),
                validator: Validator {
                    issuer: config.issuer.clone(),
                    audience: config.audience.clone(),
                    client_id: config.client_id.clone(),
                    algorithms: config.algorithms.clone(),
                },
                sealer: Sealer::new(&config.session_keys),
                store,
                introspection: IntrospectionCache::default(),
                config,
            })),
        }
    }

    /// This authentication mailing invitations through `outbox`
    /// ([`crate::mail::MailOutbox`]); whether it delivers decides whether
    /// notifications are mailed too.
    #[must_use]
    pub fn with_invitation_outbox(mut self, outbox: Arc<dyn InvitationOutbox>) -> Self {
        self.outbox = outbox;
        self
    }

    /// Whether the deployment mails (docs/messaging.md §3.5): an SMTP relay
    /// is configured, in `oidc`. The `none` mode never mails.
    pub(crate) fn mailing(&self) -> Mailing {
        Mailing(self.oidc().is_some() && self.outbox.delivers())
    }

    /// The keys of the unsubscribe tokens, in `oidc`: derived from the
    /// session keys.
    pub(crate) fn unsubscribe_keys(&self) -> Result<crate::mail::UnsubscribeKeys, OxidGeneError> {
        self.oidc()
            .map(|oidc| crate::mail::UnsubscribeKeys::from_session_keys(&oidc.config.session_keys))
            .ok_or(OxidGeneError::NotFound {
                entity: "Notification",
                id: Uuid::nil(),
            })
    }

    /// What sharing needs of the deployment, in `oidc`: the `none` mode has
    /// no accounts to share with, and answers `404` as the desktop does.
    pub(crate) fn sharing_env(&self) -> Result<SharingEnv, OxidGeneError> {
        match self.oidc() {
            Some(oidc) => Ok(SharingEnv {
                public_url: oidc.config.public_url.trim_end_matches('/').to_string(),
                outbox: Arc::clone(&self.outbox),
            }),
            None => Err(OxidGeneError::NotFound {
                entity: "Membership",
                id: Uuid::nil(),
            }),
        }
    }

    /// `oidc` or `none`, as `/auth/session` reports it.
    pub fn mode_name(&self) -> &'static str {
        match self.mode {
            Mode::None => "none",
            Mode::Oidc(_) => "oidc",
        }
    }

    /// The application's public origin, in `oidc`: the only origin a
    /// browser may write from, and a host the server answers under.
    pub fn public_origin(&self) -> Option<&str> {
        self.oidc().map(|oidc| oidc.config.public_origin.as_str())
    }

    /// Whether the public URL is `https`, which earns every response
    /// `Strict-Transport-Security` (§9.1).
    pub fn uses_https(&self) -> bool {
        self.oidc().is_some_and(|oidc| oidc.config.uses_https())
    }

    /// Read the provider's discovery and keys in the background, so that the
    /// first sign-in does not wait for them.
    pub fn warm_up(self: &Arc<Self>) {
        let auth = Arc::clone(self);
        tokio::spawn(async move {
            if let Some(oidc) = auth.oidc()
                && oidc.provider.metadata().await.is_err()
            {
                tracing::warn!(
                    auth = "idp_unreachable",
                    "The identity provider could not be reached; retrying when needed"
                );
            }
        });
    }

    pub(crate) fn oidc(&self) -> Option<&Oidc> {
        match &self.mode {
            Mode::Oidc(oidc) => Some(oidc),
            Mode::None => None,
        }
    }

    /// The signup and role rules, in `oidc`.
    pub(crate) fn signup_policy(&self) -> Option<&SignupPolicy> {
        self.oidc().map(|oidc| &oidc.config.signup)
    }

    /// End every session of `user_id` at once (§4.7): deactivation,
    /// erasure, a role change, signing out everywhere.
    pub(crate) async fn end_user_sessions(
        &self,
        user_id: Uuid,
        cause: &'static str,
    ) -> Result<(), OxidGeneError> {
        if let Some(oidc) = self.oidc() {
            oidc.end_user_sessions(user_id, cause)
                .await
                .map_err(unavailable)?;
        }
        Ok(())
    }

    /// The live sessions of `user_id`; none in the `none` mode.
    pub(crate) async fn user_sessions(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<store::Session>, OxidGeneError> {
        match self.oidc() {
            Some(oidc) => oidc.store.user_sessions(user_id).await.map_err(unavailable),
            None => Ok(Vec::new()),
        }
    }

    /// End one session.
    pub(crate) async fn end_session(
        &self,
        session: &store::Session,
        cause: &'static str,
    ) -> Result<(), OxidGeneError> {
        if let Some(oidc) = self.oidc() {
            oidc.end_session(session, cause)
                .await
                .map_err(unavailable)?;
        }
        Ok(())
    }
}

fn unavailable(_: StoreUnavailable) -> OxidGeneError {
    OxidGeneError::Internal("session store unavailable".into())
}
