//! The SMTP relay's settings (docs/messaging.md §3.2): read raw from
//! `oxidgene.toml` and the `OXIDGENE_*` environment by the server and the
//! worker, then checked once into a [`SmtpConfig`].
//!
//! A missing or invalid value fails with a stable category and the name of
//! the variable, never its value: the password must not reach a log.

use serde::Deserialize;

/// Why the settings cannot be used: `mail_config_missing` or
/// `mail_config_invalid`, and the variable at fault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MailConfigError {
    pub category: &'static str,
    pub variable: &'static str,
}

impl MailConfigError {
    fn missing(variable: &'static str) -> Self {
        Self {
            category: "mail_config_missing",
            variable,
        }
    }

    fn invalid(variable: &'static str) -> Self {
        Self {
            category: "mail_config_invalid",
            variable,
        }
    }
}

impl std::fmt::Display for MailConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.category, self.variable)
    }
}

impl std::error::Error for MailConfigError {}

/// The settings as written: every field is the `OXIDGENE_` variable of the
/// same name, lower-cased. The password may instead name a mounted file
/// through `OXIDGENE_SMTP_PASSWORD_FILE`.
#[derive(Clone, Default, Deserialize)]
pub struct MailSettings {
    #[serde(default)]
    pub smtp_host: Option<String>,
    #[serde(default)]
    pub smtp_port: Option<u16>,
    #[serde(default)]
    pub smtp_tls: Option<String>,
    #[serde(default)]
    pub smtp_username: Option<String>,
    #[serde(default)]
    pub smtp_password: Option<String>,
    #[serde(default)]
    pub smtp_password_file: Option<String>,
    #[serde(default)]
    pub mail_from: Option<String>,
}

impl std::fmt::Debug for MailSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Whether a relay is set, nothing else: the rest names hosts,
        // accounts and a password.
        f.debug_struct("MailSettings")
            .field("configured", &self.smtp_host.is_some())
            .finish_non_exhaustive()
    }
}

/// How the connection to the relay is protected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmtpTls {
    /// `STARTTLS`, required: the default, on port 587.
    StartTls,
    /// TLS from the first byte, on port 465.
    Implicit,
    /// None, towards a relay on the loopback interface only.
    None,
    /// None, towards any relay: a development relay such as Mailpit on a
    /// Compose network. Every start warns about it.
    DangerInsecure,
}

/// The checked settings of the relay.
#[derive(Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub tls: SmtpTls,
    pub credentials: Option<(String, String)>,
    /// The `From` mailbox, as written: `OxidGene <no-reply@…>`.
    pub from: String,
}

impl std::fmt::Debug for SmtpConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmtpConfig")
            .field("port", &self.port)
            .field("tls", &self.tls)
            .field("credentials", &self.credentials.is_some())
            .finish_non_exhaustive()
    }
}

/// `value`, trimmed, unless empty.
fn optional(value: Option<&String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Whether `host` is the loopback interface.
fn loopback(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

impl MailSettings {
    /// The relay, if `OXIDGENE_SMTP_HOST` names one: without, nothing is
    /// mailed (§3.5).
    ///
    /// # Errors
    ///
    /// The category and variable of the first missing or invalid value.
    pub fn resolve(&self) -> Result<Option<SmtpConfig>, MailConfigError> {
        let Some(host) = optional(self.smtp_host.as_ref()) else {
            return Ok(None);
        };
        let tls = match optional(self.smtp_tls.as_ref()).as_deref() {
            None | Some("starttls") => SmtpTls::StartTls,
            Some("implicit") => SmtpTls::Implicit,
            Some("none") if loopback(&host) => SmtpTls::None,
            Some("danger-insecure") => SmtpTls::DangerInsecure,
            Some(_) => return Err(MailConfigError::invalid("OXIDGENE_SMTP_TLS")),
        };
        let port = match (self.smtp_port, tls) {
            (Some(0), _) => return Err(MailConfigError::invalid("OXIDGENE_SMTP_PORT")),
            (Some(port), _) => port,
            (None, SmtpTls::Implicit) => 465,
            (None, _) => 587,
        };
        let from = optional(self.mail_from.as_ref())
            .ok_or(MailConfigError::missing("OXIDGENE_MAIL_FROM"))?;
        from.parse::<lettre::message::Mailbox>()
            .map_err(|_| MailConfigError::invalid("OXIDGENE_MAIL_FROM"))?;
        Ok(Some(SmtpConfig {
            host,
            port,
            tls,
            credentials: self.credentials()?,
            from,
        }))
    }

    /// The account and password, both or neither.
    fn credentials(&self) -> Result<Option<(String, String)>, MailConfigError> {
        let username = optional(self.smtp_username.as_ref());
        let password = match optional(self.smtp_password_file.as_ref()) {
            Some(path) => Some(
                std::fs::read_to_string(path)
                    .map_err(|_| MailConfigError::invalid("OXIDGENE_SMTP_PASSWORD_FILE"))?
                    .trim_end_matches(['\r', '\n'])
                    .to_string(),
            ),
            None => self.smtp_password.clone().filter(|p| !p.is_empty()),
        };
        match (username, password) {
            (Some(username), Some(password)) => Ok(Some((username, password))),
            (None, None) => Ok(None),
            (Some(_), None) => Err(MailConfigError::missing("OXIDGENE_SMTP_PASSWORD")),
            (None, Some(_)) => Err(MailConfigError::missing("OXIDGENE_SMTP_USERNAME")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(host: &str, tls: Option<&str>) -> MailSettings {
        MailSettings {
            smtp_host: Some(host.into()),
            smtp_tls: tls.map(Into::into),
            mail_from: Some("OxidGene <no-reply@genealogy.example.invalid>".into()),
            ..MailSettings::default()
        }
    }

    #[test]
    fn without_a_host_nothing_is_mailed() {
        assert!(MailSettings::default().resolve().unwrap().is_none());
        assert!(settings("  ", None).resolve().unwrap().is_none());
    }

    #[test]
    fn starttls_on_587_is_the_default_and_implicit_tls_takes_465() {
        let relay = settings("relay.example.invalid", None)
            .resolve()
            .unwrap()
            .unwrap();
        assert_eq!((relay.tls, relay.port), (SmtpTls::StartTls, 587));
        let relay = settings("relay.example.invalid", Some("implicit"))
            .resolve()
            .unwrap()
            .unwrap();
        assert_eq!((relay.tls, relay.port), (SmtpTls::Implicit, 465));
    }

    #[test]
    fn plain_smtp_is_for_a_loopback_relay_unless_called_dangerous() {
        for host in ["localhost", "127.0.0.1", "[::1]"] {
            assert_eq!(
                settings(host, Some("none")).resolve().unwrap().unwrap().tls,
                SmtpTls::None,
                "{host}"
            );
        }
        assert_eq!(
            settings("relay.example.invalid", Some("none"))
                .resolve()
                .unwrap_err(),
            MailConfigError::invalid("OXIDGENE_SMTP_TLS")
        );
        assert_eq!(
            settings("mailpit", Some("danger-insecure"))
                .resolve()
                .unwrap()
                .unwrap()
                .tls,
            SmtpTls::DangerInsecure
        );
    }

    #[test]
    fn the_sender_and_both_credentials_are_required() {
        let mut no_from = settings("relay.example.invalid", None);
        no_from.mail_from = None;
        assert_eq!(
            no_from.resolve().unwrap_err(),
            MailConfigError::missing("OXIDGENE_MAIL_FROM")
        );
        let mut bad_from = settings("relay.example.invalid", None);
        bad_from.mail_from = Some("not a mailbox".into());
        assert_eq!(
            bad_from.resolve().unwrap_err(),
            MailConfigError::invalid("OXIDGENE_MAIL_FROM")
        );
        let mut half = settings("relay.example.invalid", None);
        half.smtp_username = Some("fictive-account".into());
        assert_eq!(
            half.resolve().unwrap_err(),
            MailConfigError::missing("OXIDGENE_SMTP_PASSWORD")
        );
        half.smtp_password = Some("fixture-password".into());
        let relay = half.resolve().unwrap().unwrap();
        assert!(relay.credentials.is_some());
        assert!(!format!("{relay:?}").contains("fixture-password"));
    }
}
