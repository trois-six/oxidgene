//! The text of OxidGene's mail (docs/messaging.md §3.4): each kind in plain
//! text and simple HTML, written in the recipient's language from the
//! translation tables `assets/mail/<language>.json`, one per interface
//! language at key parity.
//!
//! A mail is rendered when the worker sends it, from the ids its outbox row
//! holds, so it says what is true then: the tree's current name, and no
//! name at all for a tree the recipient no longer sees. A mail that no
//! longer applies — a revoked invitation, a notification read meanwhile — is
//! not sent. It names the actor and the tree and links to the application;
//! nothing else of the genealogy.

use std::collections::HashMap;
use std::sync::LazyLock;

use chrono::{DateTime, Utc};
use oxidgene_core::OxidGeneError;
use oxidgene_db::repo::{
    AppUserRepo, BackgroundJobRepo, InvitationRepo, MembershipRepo, Notification, NotificationRepo,
    OutboxMail, TreeRepo, UserStatus,
};
use oxidgene_db::sea_orm::ConnectionTrait;
use serde_json::Value;
use uuid::Uuid;

use super::relay::Outgoing;
use super::unsubscribe::UnsubscribeKeys;
use crate::service::notification::{self as notice, EmailPreference, NotificationKind};

/// The interface languages and their mail tables.
const TABLES: [(&str, &str); 8] = [
    ("en", include_str!("../../../../assets/mail/en.json")),
    ("fr", include_str!("../../../../assets/mail/fr.json")),
    ("de", include_str!("../../../../assets/mail/de.json")),
    ("es", include_str!("../../../../assets/mail/es.json")),
    ("it", include_str!("../../../../assets/mail/it.json")),
    ("nl", include_str!("../../../../assets/mail/nl.json")),
    ("pl", include_str!("../../../../assets/mail/pl.json")),
    ("pt", include_str!("../../../../assets/mail/pt.json")),
];

/// The language of a mail to someone without an account, or whose account
/// names none: the instance's default (§3.4).
pub const DEFAULT_LANGUAGE: &str = "en";

static PARSED: LazyLock<HashMap<&'static str, HashMap<String, String>>> = LazyLock::new(|| {
    TABLES
        .iter()
        .map(|(code, json)| {
            let table: HashMap<String, String> =
                serde_json::from_str(json).expect("a mail table is a JSON object of strings");
            (*code, table)
        })
        .collect()
});

/// The mail language of an account's locale: one of the interface
/// languages, else the default.
pub fn language_of(locale: Option<&str>) -> String {
    locale
        .and_then(crate::service::account::interface_language)
        .filter(|code| PARSED.contains_key(code))
        .unwrap_or(DEFAULT_LANGUAGE)
        .to_string()
}

/// The text of `key` in `language`, its `{name}`s replaced by `args`;
/// English when the language lacks it, the key itself when both do.
fn t(language: &str, key: &str, args: &[(&str, &str)]) -> String {
    let text = PARSED
        .get(language)
        .and_then(|table| table.get(key))
        .or_else(|| PARSED[DEFAULT_LANGUAGE].get(key))
        .map_or(key, String::as_str);
    args.iter().fold(text.to_string(), |text, (name, value)| {
        text.replace(&format!("{{{name}}}"), value)
    })
}

/// Text from a user or a tree, made safe for one line: no control
/// character, which could split a header.
fn one_line(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

/// `text` escaped for HTML.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// What the worker does with a queued mail.
#[derive(Debug)]
pub enum Rendered {
    Send(Box<Outgoing>),
    /// It no longer applies; why, as a stable code.
    Drop(&'static str),
}

/// What rendering needs of the deployment: where links point, and the keys
/// of the unsubscribe tokens.
#[derive(Debug, Clone)]
pub struct MailContext {
    /// The application's public URL, without a trailing slash.
    pub public_url: String,
    pub unsubscribe: UnsubscribeKeys,
}

/// A mail before it is laid out: a subject, its paragraphs, the button and
/// its link, and the unsubscribe address.
struct Composed {
    language: String,
    subject: String,
    paragraphs: Vec<String>,
    action: (String, String),
    /// The one-click address a mail client posts to, and the page a reader
    /// opens from the body.
    unsubscribe: Option<(String, String)>,
}

impl Composed {
    fn into_outgoing(self, to: &str) -> Outgoing {
        let reason = self
            .unsubscribe
            .is_some()
            .then(|| t(&self.language, "common.reason", &[]));
        let mut text = self.paragraphs.join("\n\n");
        text.push_str(&format!("\n\n{}: {}\n", self.action.0, self.action.1));
        let mut html = String::from(
            "<!DOCTYPE html><html><body style=\"font-family:sans-serif;line-height:1.5\">",
        );
        for paragraph in &self.paragraphs {
            html.push_str(&format!("<p>{}</p>", escape(paragraph)));
        }
        html.push_str(&format!(
            "<p><a href=\"{}\">{}</a></p>",
            escape(&self.action.1),
            escape(&self.action.0)
        ));
        if let (Some(reason), Some((_, url))) = (reason, &self.unsubscribe) {
            let stop = t(&self.language, "common.unsubscribe", &[]);
            text.push_str(&format!("\n--\n{reason}\n{stop}: {url}\n"));
            html.push_str(&format!(
                "<hr><p style=\"font-size:small;color:#555\">{} <a href=\"{}\">{}</a></p>",
                escape(&reason),
                escape(url),
                escape(&stop)
            ));
        }
        html.push_str("</body></html>");
        Outgoing {
            to: to.to_string(),
            subject: one_line(&self.subject),
            text,
            html,
            unsubscribe: self.unsubscribe.map(|(one_click, _)| one_click),
        }
    }
}

/// Render `mail` as it stands now.
///
/// # Errors
///
/// A database error; the worker tries again later.
pub async fn render(
    db: &impl ConnectionTrait,
    context: &MailContext,
    mail: &OutboxMail,
) -> Result<Rendered, OxidGeneError> {
    let params: Value = serde_json::from_str(&mail.params).unwrap_or(Value::Null);
    let id = |name: &str| params[name].as_str().and_then(|v| v.parse::<Uuid>().ok());
    if mail.kind == "invitation" {
        let Some(invitation_id) = id("invitation_id") else {
            return Ok(Rendered::Drop("malformed"));
        };
        return invitation(db, context, mail, invitation_id).await;
    }
    let Some(user_id) = mail.user_id else {
        return Ok(Rendered::Drop("malformed"));
    };
    if !active(db, user_id).await? {
        return Ok(Rendered::Drop("recipient_unavailable"));
    }
    if mail.kind == "digest" {
        let since = params["since"]
            .as_str()
            .and_then(|v| DateTime::parse_from_rfc3339(v).ok())
            .map(|at| at.with_timezone(&Utc));
        let Some(since) = since else {
            return Ok(Rendered::Drop("malformed"));
        };
        return digest(db, context, mail, user_id, since).await;
    }
    let (Some(kind), Some(notification_id)) =
        (NotificationKind::parse(&mail.kind), id("notification_id"))
    else {
        return Ok(Rendered::Drop("malformed"));
    };
    let Some(notification) = NotificationRepo::get(db, user_id, notification_id).await? else {
        return Ok(Rendered::Drop("obsolete"));
    };
    if notification.read_at.is_some() {
        return Ok(Rendered::Drop("already_read"));
    }
    if notice::email_preference(db, user_id, kind).await? != EmailPreference::Immediate {
        return Ok(Rendered::Drop("unsubscribed"));
    }
    let language = &mail.language;
    let line = sentence(db, language, &notification).await?;
    let link = line.link(context);
    let composed = Composed {
        language: language.clone(),
        subject: line.subject,
        paragraphs: vec![line.body],
        action: (t(language, "common.open", &[]), link),
        unsubscribe: Some(unsubscribe_urls(context, user_id, kind.as_str())),
    };
    Ok(Rendered::Send(Box::new(
        composed.into_outgoing(&mail.to_address),
    )))
}

/// `{public_url}/api/v1/mail/unsubscribe?token=…`, which a mail client
/// posts to (RFC 8058), and `{public_url}/unsubscribe?token=…`, the page
/// the mail's body links to, which asks before it posts the same token.
fn unsubscribe_urls(context: &MailContext, user_id: Uuid, kind: &str) -> (String, String) {
    let token = context.unsubscribe.sign(user_id, kind);
    (
        format!(
            "{}/api/v1/mail/unsubscribe?token={token}",
            context.public_url
        ),
        format!("{}/unsubscribe?token={token}", context.public_url),
    )
}

/// The invitation `invitation_id` mailed to its address, if it still waits
/// for an answer.
async fn invitation(
    db: &impl ConnectionTrait,
    context: &MailContext,
    mail: &OutboxMail,
    invitation_id: Uuid,
) -> Result<Rendered, OxidGeneError> {
    let now = Utc::now();
    let Some(invitation) = InvitationRepo::get(db, invitation_id)
        .await?
        .filter(|invitation| invitation.is_pending(now))
    else {
        return Ok(Rendered::Drop("obsolete"));
    };
    let Ok(tree) = TreeRepo::get(db, invitation.tree_id).await else {
        return Ok(Rendered::Drop("obsolete"));
    };
    let inviter = match invitation.created_by {
        Some(id) => AppUserRepo::shown_names(db, &[id])
            .await?
            .remove(&id)
            .flatten(),
        None => None,
    };
    let language = &mail.language;
    let tree_name = one_line(&tree.name);
    let role = t(language, &format!("role.{}", invitation.role.as_str()), &[]);
    let (subject, body) = match &inviter {
        Some(inviter) => {
            let inviter = one_line(inviter);
            let args = [("inviter", inviter.as_str()), ("tree", tree_name.as_str())];
            (
                t(language, "invitation.subject", &args),
                t(
                    language,
                    "invitation.body",
                    &[args[0], args[1], ("role", &role)],
                ),
            )
        }
        None => (
            t(
                language,
                "invitation.subject_anonymous",
                &[("tree", &tree_name)],
            ),
            t(
                language,
                "invitation.body_anonymous",
                &[("tree", &tree_name), ("role", &role)],
            ),
        ),
    };
    let date = invitation.expires_at.format("%Y-%m-%d").to_string();
    let composed = Composed {
        language: language.clone(),
        subject,
        paragraphs: vec![
            body,
            t(language, "invitation.how", &[("date", &date)]),
            t(language, "invitation.ignore", &[]),
        ],
        action: (
            t(language, "invitation.action", &[]),
            format!("{}/invitations", context.public_url),
        ),
        // The address has no account to keep a preference in: an
        // invitation is mailed once per send, at the owner's request.
        unsubscribe: None,
    };
    Ok(Rendered::Send(Box::new(
        composed.into_outgoing(&mail.to_address),
    )))
}

/// The daily digest of `user_id`: the unread notifications of the kinds
/// they want daily, since `since`.
async fn digest(
    db: &impl ConnectionTrait,
    context: &MailContext,
    mail: &OutboxMail,
    user_id: Uuid,
    since: DateTime<Utc>,
) -> Result<Rendered, OxidGeneError> {
    let mut kinds = Vec::new();
    for kind in NotificationKind::ALL {
        if notice::email_preference(db, user_id, kind).await? == EmailPreference::Daily {
            kinds.push(kind.as_str());
        }
    }
    if kinds.is_empty() {
        return Ok(Rendered::Drop("unsubscribed"));
    }
    let notifications = NotificationRepo::unread_since(db, user_id, &kinds, since).await?;
    if notifications.is_empty() {
        return Ok(Rendered::Drop("already_read"));
    }
    let language = &mail.language;
    let mut paragraphs = vec![t(language, "digest.intro", &[])];
    for notification in &notifications {
        paragraphs.push(format!(
            "• {}",
            sentence(db, language, notification).await?.body
        ));
    }
    let composed = Composed {
        language: language.clone(),
        subject: t(language, "digest.subject", &[]),
        paragraphs,
        action: (
            t(language, "common.open", &[]),
            format!("{}/", context.public_url),
        ),
        unsubscribe: Some(unsubscribe_urls(context, user_id, "digest")),
    };
    Ok(Rendered::Send(Box::new(
        composed.into_outgoing(&mail.to_address),
    )))
}

/// One notification in words: its subject line, its sentence, and the
/// tree it opens when its reader still sees it.
struct Sentence {
    subject: String,
    body: String,
    tree: Option<Uuid>,
}

impl Sentence {
    fn link(&self, context: &MailContext) -> String {
        match self.tree {
            Some(tree_id) => format!("{}/trees/{tree_id}", context.public_url),
            None => format!("{}/", context.public_url),
        }
    }
}

/// Whether `user_id` still sees tree `tree_id`: a live membership of a live
/// tree; its name then.
async fn tree_seen(
    db: &impl ConnectionTrait,
    user_id: Uuid,
    tree_id: Uuid,
) -> Result<Option<String>, OxidGeneError> {
    let member = MembershipRepo::get(db, tree_id, user_id)
        .await?
        .is_some_and(|membership| membership.is_live(Utc::now()));
    if !member {
        return Ok(None);
    }
    Ok(TreeRepo::get(db, tree_id)
        .await
        .ok()
        .map(|tree| one_line(&tree.name)))
}

/// `notification` in `language`, under its reader's access now.
async fn sentence(
    db: &impl ConnectionTrait,
    language: &str,
    notification: &Notification,
) -> Result<Sentence, OxidGeneError> {
    let unseen = || Sentence {
        subject: t(language, "membership_ended.subject", &[]),
        body: t(language, "membership_ended.body", &[]),
        tree: None,
    };
    let Some(kind) = NotificationKind::parse(&notification.kind) else {
        return Ok(unseen());
    };
    let tree = match notification.tree_id {
        Some(tree_id) => tree_seen(db, notification.user_id, tree_id)
            .await?
            .map(|name| (tree_id, name)),
        None => None,
    };
    let Some((tree_id, tree_name)) = tree.filter(|_| kind != NotificationKind::MembershipEnded)
    else {
        return Ok(unseen());
    };
    let mut people: Vec<Uuid> = notification.actor_user_id.into_iter().collect();
    people.extend(notification.subject_id);
    let names = AppUserRepo::shown_names(db, &people).await?;
    let name = |id: Option<Uuid>| {
        id.and_then(|id| names.get(&id).cloned().flatten())
            .map_or_else(|| t(language, "common.someone", &[]), |n| one_line(&n))
    };
    let actor = name(notification.actor_user_id);
    let args = [("actor", actor.as_str()), ("tree", tree_name.as_str())];
    let key = kind.as_str();
    let (subject, body) = match kind {
        NotificationKind::OwnershipTransferred => {
            let you = notification.subject_id == Some(notification.user_id);
            let owner = name(notification.subject_id);
            (
                t(language, "ownership_transferred.subject", &args),
                if you {
                    t(language, "ownership_transferred.body_you", &args)
                } else {
                    t(
                        language,
                        "ownership_transferred.body_other",
                        &[("owner", &owner), args[1]],
                    )
                },
            )
        }
        NotificationKind::JobFinished => {
            let outcome = job_outcome(db, notification.subject_id).await?;
            (
                t(language, "job_finished.subject", &args),
                t(language, &format!("job_finished.{outcome}"), &args),
            )
        }
        _ => (
            t(language, &format!("{key}.subject"), &args),
            t(language, &format!("{key}.body"), &args),
        ),
    };
    Ok(Sentence {
        subject,
        body,
        tree: Some(tree_id),
    })
}

/// How job `job_id` ended, as a key of `job_finished`: `ended` once its row
/// is gone.
pub(crate) async fn job_outcome(
    db: &impl ConnectionTrait,
    job_id: Option<Uuid>,
) -> Result<&'static str, OxidGeneError> {
    let Some(job_id) = job_id else {
        return Ok("ended");
    };
    let Some(job) = BackgroundJobRepo::find(db, job_id).await? else {
        return Ok("ended");
    };
    Ok(match (job.kind.as_str(), job.status.as_str()) {
        ("import", "completed") => "import_completed",
        ("import", "failed") => "import_failed",
        ("export", "completed") => "export_completed",
        ("export", "failed") => "export_failed",
        _ => "ended",
    })
}

/// Whether `user_id` is someone to mail: an active account.
pub(crate) async fn active(
    db: &impl ConnectionTrait,
    user_id: Uuid,
) -> Result<bool, OxidGeneError> {
    Ok(AppUserRepo::get(db, user_id)
        .await
        .is_ok_and(|user| user.status == UserStatus::Active))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn placeholders(text: &str) -> BTreeSet<String> {
        text.split('{')
            .skip(1)
            .filter_map(|rest| rest.split_once('}').map(|(name, _)| name.to_string()))
            .collect()
    }

    /// Every table carries the keys of the English one, no others, and the
    /// same `{names}` in each: a missing key renders as the key itself in a
    /// mail nobody can recall.
    #[test]
    fn the_mail_tables_are_at_key_parity() {
        let english = &PARSED[DEFAULT_LANGUAGE];
        for (code, table) in PARSED.iter() {
            let missing: Vec<_> = english.keys().filter(|k| !table.contains_key(*k)).collect();
            let extra: Vec<_> = table.keys().filter(|k| !english.contains_key(*k)).collect();
            assert!(missing.is_empty(), "{code} lacks {missing:?}");
            assert!(extra.is_empty(), "{code} has keys English lacks: {extra:?}");
            for (key, text) in table {
                assert_eq!(
                    placeholders(text),
                    placeholders(&english[key]),
                    "{code} {key}"
                );
                assert!(!text.trim().is_empty(), "{code} {key}");
            }
        }
        assert_eq!(PARSED.len(), 8);
    }

    /// Every key the templates build exists: one per kind, outcome and
    /// role.
    #[test]
    fn every_key_the_templates_build_exists() {
        let english = &PARSED[DEFAULT_LANGUAGE];
        let mut keys: Vec<String> = Vec::new();
        for kind in NotificationKind::ALL {
            keys.push(format!("{}.subject", kind.as_str()));
            match kind {
                NotificationKind::OwnershipTransferred => {
                    keys.push("ownership_transferred.body_you".into());
                    keys.push("ownership_transferred.body_other".into());
                }
                NotificationKind::JobFinished => {
                    for outcome in [
                        "import_completed",
                        "import_failed",
                        "export_completed",
                        "export_failed",
                        "ended",
                    ] {
                        keys.push(format!("job_finished.{outcome}"));
                    }
                }
                _ => keys.push(format!("{}.body", kind.as_str())),
            }
        }
        keys.extend(["role.editor", "role.guest"].map(String::from));
        for key in keys {
            assert!(english.contains_key(&key), "{key}");
        }
    }

    #[test]
    fn user_text_cannot_break_the_html_or_a_header() {
        let composed = Composed {
            language: "en".into(),
            subject: "A <b>tree</b>\r\nBcc: someone".into(),
            paragraphs: vec!["Fictive & <script>tree</script>".into()],
            action: ("Open".into(), "https://genealogy.example.invalid/".into()),
            unsubscribe: Some((
                "https://genealogy.example.invalid/api/v1/mail/unsubscribe?token=a".into(),
                "https://genealogy.example.invalid/unsubscribe?token=a&b".into(),
            )),
        };
        let mail = composed.into_outgoing("reader@example.invalid");
        assert!(!mail.subject.contains('\n') && !mail.subject.contains('\r'));
        assert!(
            mail.html
                .contains("Fictive &amp; &lt;script&gt;tree&lt;/script&gt;")
        );
        assert!(!mail.html.contains("<script>"));
        assert!(mail.html.contains("token=a&amp;b"));
        assert!(mail.text.contains("Fictive & <script>tree</script>"));
        assert!(mail.text.contains("Stop these emails"));
    }

    #[test]
    fn an_account_locale_picks_the_mail_language() {
        assert_eq!(language_of(Some("fr-FR")), "fr");
        assert_eq!(language_of(Some("pt_BR")), "pt");
        assert_eq!(language_of(Some("ja")), "en");
        assert_eq!(language_of(None), "en");
        assert_eq!(
            t(
                "fr",
                "tree_shared.subject",
                &[("actor", "Ada"), ("tree", "Fictive")]
            ),
            "Ada a partagé l’arbre généalogique Fictive avec vous"
        );
    }
}
