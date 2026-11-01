//! OxidGene's own mail (docs/messaging.md §3), in the web build's `auth`
//! feature: the invitations by email and the notifications each recipient
//! wants by email, single or as a daily digest. The identity provider sends
//! its own — address verification, password reset — configured in the
//! provider, never through this module.
//!
//! - [`config`] reads `OXIDGENE_SMTP_*` and `OXIDGENE_MAIL_FROM`; without a
//!   host nothing is mailed, and nothing requires a relay.
//! - A mail is a row of the outbox (`oxidgene_db::repo::MailOutboxRepo`),
//!   written in the transaction of what causes it — [`MailOutbox`] for the
//!   invitations, `service::notification` for the notifications.
//! - The worker's [`sender::MailSender`] renders each row with
//!   [`templates`] in the recipient's language and hands it to the
//!   [`relay`]; every notification mail carries a one-click
//!   [`unsubscribe`].

pub mod config;
pub mod relay;
pub mod sender;
pub mod templates;
pub mod unsubscribe;

use std::sync::Arc;

use chrono::Utc;
use oxidgene_core::OxidGeneError;
use oxidgene_db::repo::{MailOutboxRepo, NewMail};
use oxidgene_db::sea_orm::{DatabaseConnection, DatabaseTransaction};

pub use config::{MailConfigError, MailSettings, SmtpConfig, SmtpTls};
pub use relay::{Outgoing, Relay, RelayFailure, SmtpRelay};
pub use sender::{MailSender, wake};
pub use templates::MailContext;
pub use unsubscribe::UnsubscribeKeys;

use crate::service::sharing::{InvitationMail, InvitationOutbox};

/// The outbox invitations are mailed through when the deployment has a
/// relay: a row in the invitation's transaction, which the worker sends.
#[derive(Debug, Clone, Copy)]
pub struct MailOutbox {
    delivers: bool,
}

impl MailOutbox {
    /// The outbox of a deployment that has a relay (`delivers`) or not.
    pub fn new(delivers: bool) -> Self {
        Self { delivers }
    }
}

#[async_trait::async_trait]
impl InvitationOutbox for MailOutbox {
    fn delivers(&self) -> bool {
        self.delivers
    }

    async fn enqueue(
        &self,
        txn: &DatabaseTransaction,
        mail: InvitationMail,
    ) -> Result<(), OxidGeneError> {
        if !self.delivers {
            return Ok(());
        }
        MailOutboxRepo::enqueue(
            txn,
            NewMail {
                user_id: None,
                to_address: mail.email,
                kind: "invitation",
                // The address has no account, hence no language of its own.
                language: templates::DEFAULT_LANGUAGE.to_string(),
                params: serde_json::json!({
                    "invitation_id": mail.invitation_id,
                    "tree_id": mail.tree_id,
                    "inviter_id": mail.inviter_id,
                }),
                due: Utc::now(),
            },
        )
        .await
        .map(drop)
    }

    fn committed(&self) {
        if self.delivers {
            wake();
        }
    }
}

/// The context mail is rendered in: the deployment's public URL and the
/// unsubscribe keys derived from its session keys.
pub fn context(public_url: &str, session_keys: &[[u8; 32]]) -> MailContext {
    MailContext {
        public_url: public_url.trim_end_matches('/').to_string(),
        unsubscribe: UnsubscribeKeys::from_session_keys(session_keys),
    }
}

/// The sender of a worker over `db`, relaying through `smtp` and greeting
/// it with the public URL's host.
///
/// # Errors
///
/// When the relay's TLS parameters cannot be built.
pub fn sender(
    db: DatabaseConnection,
    smtp: &SmtpConfig,
    context: MailContext,
) -> Result<MailSender, RelayFailure> {
    let hello = reqwest::Url::parse(&context.public_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .unwrap_or_else(|| "localhost".to_string());
    let relay = SmtpRelay::new(smtp, &hello)?;
    Ok(MailSender::new(db, Arc::new(relay), context))
}
