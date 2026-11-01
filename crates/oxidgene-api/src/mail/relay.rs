//! Handing a mail to the operator's SMTP relay (docs/messaging.md §3.2).
//! OxidGene neither delivers nor signs mail: the relay does, and the
//! operator keeps SPF, DKIM, DMARC and bounces.

use std::time::Duration;

use lettre::message::header::{HeaderName, HeaderValue};
use lettre::message::{Mailbox, MultiPart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::transport::smtp::extension::ClientId;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use super::config::{SmtpConfig, SmtpTls};

/// A mail ready to send: its recipient, subject, plain-text and HTML
/// bodies, and the one-click unsubscribe address (RFC 8058), if any.
#[derive(Clone, PartialEq, Eq)]
pub struct Outgoing {
    pub to: String,
    pub subject: String,
    pub text: String,
    pub html: String,
    pub unsubscribe: Option<String>,
}

impl std::fmt::Debug for Outgoing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the address.
        f.debug_struct("Outgoing")
            .field("subject", &self.subject)
            .finish_non_exhaustive()
    }
}

/// Why a relay did not take a mail: a stable category for the logs and the
/// outbox, and whether trying again could help.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelayFailure {
    /// `rejected`, `deferred`, `timeout`, `tls`, `connection`, `message`.
    pub category: &'static str,
    /// A permanent refusal (an SMTP 5xx reply, a mail that cannot be
    /// built): retrying sends nothing more.
    pub permanent: bool,
}

/// Where mail goes: the SMTP relay, or a test's fake.
#[async_trait::async_trait]
pub trait Relay: Send + Sync {
    async fn send(&self, mail: &Outgoing) -> Result<(), RelayFailure>;
}

/// The relay `OXIDGENE_SMTP_HOST` names.
pub struct SmtpRelay {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
}

impl std::fmt::Debug for SmtpRelay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SmtpRelay")
    }
}

/// How long one exchange with the relay may take.
const SMTP_TIMEOUT: Duration = Duration::from_secs(60);

impl SmtpRelay {
    /// A relay as `config` describes it, greeting it as `hello`, the public
    /// URL's host.
    ///
    /// # Errors
    ///
    /// When the TLS parameters cannot be built for the host.
    pub fn new(config: &SmtpConfig, hello: &str) -> Result<Self, RelayFailure> {
        let tls = match config.tls {
            SmtpTls::StartTls => Tls::Required(tls_parameters(&config.host)?),
            SmtpTls::Implicit => Tls::Wrapper(tls_parameters(&config.host)?),
            SmtpTls::None | SmtpTls::DangerInsecure => Tls::None,
        };
        let mut builder = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&config.host)
            .port(config.port)
            .tls(tls)
            .timeout(Some(SMTP_TIMEOUT))
            .hello_name(ClientId::Domain(hello.to_string()));
        if let Some((username, password)) = &config.credentials {
            builder = builder.credentials(Credentials::new(username.clone(), password.clone()));
        }
        let from = config.from.parse().map_err(|_| RelayFailure {
            category: "message",
            permanent: true,
        })?;
        Ok(Self {
            transport: builder.build(),
            from,
        })
    }
}

fn tls_parameters(host: &str) -> Result<TlsParameters, RelayFailure> {
    TlsParameters::new(host.to_string()).map_err(|_| RelayFailure {
        category: "tls",
        permanent: true,
    })
}

/// `mail` as a MIME message from `from`: plain text and HTML alternatives,
/// with the one-click unsubscribe headers of RFC 8058 when it has an
/// address for them.
pub(crate) fn message(from: &Mailbox, mail: &Outgoing) -> Result<Message, RelayFailure> {
    let malformed = |_| RelayFailure {
        category: "message",
        permanent: true,
    };
    let to: Mailbox = mail.to.parse().map_err(malformed)?;
    let mut builder = Message::builder()
        .from(from.clone())
        .to(to)
        .subject(mail.subject.clone());
    if let Some(url) = &mail.unsubscribe {
        builder = builder
            .raw_header(HeaderValue::new(
                HeaderName::new_from_ascii_str("List-Unsubscribe"),
                format!("<{url}>"),
            ))
            .raw_header(HeaderValue::new(
                HeaderName::new_from_ascii_str("List-Unsubscribe-Post"),
                "List-Unsubscribe=One-Click".to_string(),
            ));
    }
    builder
        .multipart(MultiPart::alternative_plain_html(
            mail.text.clone(),
            mail.html.clone(),
        ))
        .map_err(|_| RelayFailure {
            category: "message",
            permanent: true,
        })
}

#[async_trait::async_trait]
impl Relay for SmtpRelay {
    async fn send(&self, mail: &Outgoing) -> Result<(), RelayFailure> {
        let message = message(&self.from, mail)?;
        self.transport
            .send(message)
            .await
            .map(drop)
            .map_err(|error| classify(&error))
    }
}

/// The category of a relay's error, without its text, which may quote an
/// address.
fn classify(error: &lettre::transport::smtp::Error) -> RelayFailure {
    let (category, permanent) = if error.is_permanent() {
        ("rejected", true)
    } else if error.is_transient() {
        ("deferred", false)
    } else if error.is_timeout() {
        ("timeout", false)
    } else if error.is_tls() {
        ("tls", false)
    } else {
        ("connection", false)
    };
    RelayFailure {
        category,
        permanent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mail_carries_both_bodies_and_its_one_click_unsubscribe() {
        let from: Mailbox = "OxidGene <no-reply@genealogy.example.invalid>"
            .parse()
            .unwrap();
        let mail = Outgoing {
            to: "fictive.reader@example.invalid".into(),
            subject: "Fictive subject".into(),
            text: "Plain body".into(),
            html: "<p>HTML body</p>".into(),
            unsubscribe: Some(
                "https://genealogy.example.invalid/api/v1/mail/unsubscribe?token=t".into(),
            ),
        };
        let raw = String::from_utf8(message(&from, &mail).unwrap().formatted()).unwrap();
        assert!(raw.contains(
            "List-Unsubscribe: <https://genealogy.example.invalid/api/v1/mail/unsubscribe?token=t>"
        ));
        assert!(raw.contains("List-Unsubscribe-Post: List-Unsubscribe=One-Click"));
        assert!(raw.contains("multipart/alternative"));
        assert!(raw.contains("Plain body") && raw.contains("<p>HTML body</p>"));
        let without = Outgoing {
            unsubscribe: None,
            ..mail
        };
        let raw = String::from_utf8(message(&from, &without).unwrap().formatted()).unwrap();
        assert!(!raw.contains("List-Unsubscribe"));
        assert!(!format!("{without:?}").contains("example.invalid"));
    }
}
