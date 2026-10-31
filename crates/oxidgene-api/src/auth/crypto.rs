//! The small cryptography of sessions: random identifiers, digests that let
//! the store hold no usable secret, and AES-256-GCM sealing of the tokens a
//! session keeps (docs/authentication.md §4.4, §4.5).

use aws_lc_rs::aead::{AES_256_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

/// 256 random bits from the operating system, base64url-encoded: a session
/// id, a `state`, a `nonce`, a PKCE verifier (43 characters, within RFC
/// 7636's 43–128).
pub(crate) fn random_token() -> String {
    let mut bytes = [0u8; 32];
    aws_lc_rs::rand::fill(&mut bytes).expect("the system's random source");
    URL_SAFE_NO_PAD.encode(bytes)
}

/// The base64url SHA-256 of `value`: what the store keeps in place of a
/// session id, a CSRF token, a `state` or a login binding.
pub(crate) fn digest(value: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(value.as_bytes()))
}

/// The PKCE `S256` challenge of `verifier`.
pub(crate) fn pkce_challenge(verifier: &str) -> String {
    digest(verifier)
}

/// The CSRF token of the session whose cookie value is `cookie`.
///
/// Derived rather than stored: the store keeps only its digest, like the
/// session id's, and `/auth/session` recomputes it from the cookie the
/// request carries. A page that cannot read the cookie cannot compute it.
pub(crate) fn csrf_token(cookie: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"oxidgene-csrf\0");
    hasher.update(cookie.as_bytes());
    URL_SAFE_NO_PAD.encode(hasher.finalize())
}

/// Seals a session's tokens under the first key and opens them under any,
/// so that keys rotate without signing anyone out.
pub(crate) struct Sealer {
    keys: Vec<LessSafeKey>,
}

impl std::fmt::Debug for Sealer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Sealer({} keys)", self.keys.len())
    }
}

impl Sealer {
    pub(crate) fn new(keys: &[[u8; 32]]) -> Self {
        Self {
            keys: keys
                .iter()
                .map(|key| {
                    LessSafeKey::new(UnboundKey::new(&AES_256_GCM, key).expect("a 256-bit AES key"))
                })
                .collect(),
        }
    }

    /// `plaintext` sealed and bound to `context` (the session's id digest),
    /// as base64url of the nonce followed by the ciphertext and its tag.
    pub(crate) fn seal(&self, plaintext: &str, context: &str) -> String {
        let mut nonce = [0u8; NONCE_LEN];
        aws_lc_rs::rand::fill(&mut nonce).expect("the system's random source");
        let mut sealed = plaintext.as_bytes().to_vec();
        self.keys[0]
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(context.as_bytes()),
                &mut sealed,
            )
            .expect("AES-GCM sealing");
        let mut out = nonce.to_vec();
        out.extend_from_slice(&sealed);
        URL_SAFE_NO_PAD.encode(out)
    }

    /// The plaintext of `sealed`, if one of the keys opens it under
    /// `context`.
    pub(crate) fn open(&self, sealed: &str, context: &str) -> Option<String> {
        let bytes = URL_SAFE_NO_PAD.decode(sealed).ok()?;
        if bytes.len() < NONCE_LEN {
            return None;
        }
        let (nonce, ciphertext) = bytes.split_at(NONCE_LEN);
        self.keys.iter().find_map(|key| {
            let mut buffer = ciphertext.to_vec();
            let nonce = Nonce::try_assume_unique_for_key(nonce).ok()?;
            let plain = key
                .open_in_place(nonce, Aad::from(context.as_bytes()), &mut buffer)
                .ok()?;
            String::from_utf8(plain.to_vec()).ok()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_256_random_bits() {
        let a = random_token();
        assert_eq!(a.len(), 43);
        assert_ne!(a, random_token());
    }

    #[test]
    fn the_pkce_challenge_is_rfc_7636s() {
        // RFC 7636, appendix B.
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn sealed_tokens_open_under_any_key_and_only_in_their_session() {
        let old = Sealer::new(&[[1; 32]]);
        let rotated = Sealer::new(&[[2; 32], [1; 32]]);
        let sealed = old.seal("token-value", "session-a");
        assert!(!sealed.contains("token-value"));
        assert_eq!(
            rotated.open(&sealed, "session-a").as_deref(),
            Some("token-value")
        );
        assert_eq!(rotated.open(&sealed, "session-b"), None);
        assert_eq!(Sealer::new(&[[3; 32]]).open(&sealed, "session-a"), None);
        assert_eq!(rotated.open("not-sealed", "session-a"), None);
    }

    #[test]
    fn the_csrf_token_is_neither_the_cookie_nor_its_digest() {
        let cookie = random_token();
        let csrf = csrf_token(&cookie);
        assert_ne!(csrf, cookie);
        assert_ne!(csrf, digest(&cookie));
        assert_eq!(csrf, csrf_token(&cookie));
    }
}
