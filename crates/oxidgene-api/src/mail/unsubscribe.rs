//! The one-click unsubscribe of every notification mail (docs/messaging.md
//! §2.3, RFC 8058): a token naming a user and a kind, signed with a key
//! derived from the session keys, valid without signing in.
//!
//! The token is `{payload}.{signature}`, both base64url: the payload is the
//! user's sixteen-byte id followed by the kind, the signature its
//! HMAC-SHA256. The first key signs, any opens it, so that rotating the
//! session keys keeps the links of the mail already sent working until the
//! old key goes. It has no expiry: an unsubscribe link must work whenever
//! the mail is read.

use aws_lc_rs::hmac;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use uuid::Uuid;

/// The keys signing and checking unsubscribe tokens.
#[derive(Clone)]
pub struct UnsubscribeKeys(Vec<hmac::Key>);

impl std::fmt::Debug for UnsubscribeKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "UnsubscribeKeys({} keys)", self.0.len())
    }
}

/// What a token unsubscribes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unsubscription {
    pub user_id: Uuid,
    /// A notification kind, or `digest`.
    pub kind: String,
}

impl UnsubscribeKeys {
    /// Keys derived from the session keys, so that no other secret needs
    /// configuring: each is the HMAC of a fixed label under a session key.
    pub fn from_session_keys(session_keys: &[[u8; 32]]) -> Self {
        Self(
            session_keys
                .iter()
                .map(|key| {
                    let derived = hmac::sign(
                        &hmac::Key::new(hmac::HMAC_SHA256, key),
                        b"oxidgene-mail-unsubscribe",
                    );
                    hmac::Key::new(hmac::HMAC_SHA256, derived.as_ref())
                })
                .collect(),
        )
    }

    /// The token unsubscribing `user_id` from `kind`.
    pub fn sign(&self, user_id: Uuid, kind: &str) -> String {
        let mut payload = user_id.as_bytes().to_vec();
        payload.extend_from_slice(kind.as_bytes());
        let tag = hmac::sign(&self.0[0], &payload);
        format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(&payload),
            URL_SAFE_NO_PAD.encode(tag.as_ref())
        )
    }

    /// What `token` unsubscribes from, if one of the keys signed it.
    pub fn open(&self, token: &str) -> Option<Unsubscription> {
        let (payload, tag) = token.trim().split_once('.')?;
        let payload = URL_SAFE_NO_PAD.decode(payload).ok()?;
        let tag = URL_SAFE_NO_PAD.decode(tag).ok()?;
        self.0
            .iter()
            .any(|key| hmac::verify(key, &payload, &tag).is_ok())
            .then_some(())?;
        let (id, kind) = payload.split_at_checked(16)?;
        Some(Unsubscription {
            user_id: Uuid::from_slice(id).ok()?,
            kind: String::from_utf8(kind.to_vec()).ok()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_opens_under_any_key_and_nothing_else_does() {
        let old = UnsubscribeKeys::from_session_keys(&[[1; 32]]);
        let rotated = UnsubscribeKeys::from_session_keys(&[[2; 32], [1; 32]]);
        let user = Uuid::now_v7();
        let token = old.sign(user, "tree_shared");
        let opened = rotated.open(&token).expect("the old key still opens it");
        assert_eq!(opened.user_id, user);
        assert_eq!(opened.kind, "tree_shared");

        let (payload, _) = token.split_once('.').unwrap();
        let forged = format!("{payload}.{}", URL_SAFE_NO_PAD.encode([0u8; 32]));
        assert_eq!(rotated.open(&forged), None);
        let other = UnsubscribeKeys::from_session_keys(&[[3; 32]]);
        assert_eq!(other.open(&token), None);
        for malformed in ["", "x", "a.b", "..", &format!("{token}x")] {
            assert_eq!(rotated.open(malformed), None, "{malformed}");
        }
        // The kind is signed with the user: changing it breaks the token.
        let other_kind = old.sign(user, "digest");
        assert_ne!(token, other_kind);
    }
}
