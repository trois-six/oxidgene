//! The one avatar of a user of the instance (docs/authentication.md §6.7):
//! the image they uploaded, else their initials on a colour chosen from
//! their id. Every place a user's avatar appears draws it with
//! [`UserAvatar`].

use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::ApiClient;

/// How many avatar colours the palette has: the `--avatar-N-bg` and
/// `--avatar-N-fg` tokens of `LAYOUT_STYLES`, N from 0.
pub const AVATAR_TONES: usize = 8;

/// The size an avatar is drawn at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AvatarSize {
    /// 32 pixels: the navbar, a list row.
    #[default]
    Small,
    /// 96 pixels: the profile page.
    Large,
}

impl AvatarSize {
    fn class(self) -> &'static str {
        match self {
            Self::Small => "user-avatar user-avatar-small",
            Self::Large => "user-avatar user-avatar-large",
        }
    }

    /// The stored rendition drawn at this size, sharp on a dense screen.
    fn rendition(self) -> u32 {
        match self {
            Self::Small => 64,
            Self::Large => 256,
        }
    }
}

/// The initials of `name`: the first letter of each of its first two words,
/// upper-cased; `?` without a name.
pub fn initials(name: Option<&str>) -> String {
    let letters: String = name
        .unwrap_or_default()
        .split_whitespace()
        .take(2)
        .filter_map(|word| word.chars().find(|c| c.is_alphanumeric()))
        .flat_map(char::to_uppercase)
        .collect();
    if letters.is_empty() {
        "?".to_string()
    } else {
        letters
    }
}

/// The palette entry of the user `id`: the first byte of the SHA-256 of the
/// id's sixteen bytes, modulo the palette's size. The same user has the same
/// colour on every device, with nothing to store or fetch.
pub fn avatar_tone(id: Uuid) -> usize {
    usize::from(sha256(id.as_bytes())[0]) % AVATAR_TONES
}

/// A user's avatar.
///
/// `avatar` is the uploaded avatar's version, `None` for the generated one.
/// The uploaded image is drawn from its versioned address, which the
/// browser fetches once and keeps (`docs/architecture.md` §4.3).
#[component]
pub fn UserAvatar(
    user_id: Uuid,
    /// The name the initials come from, and the avatar's accessible name.
    name: Option<String>,
    avatar: Option<String>,
    #[props(default)] size: AvatarSize,
) -> Element {
    let api = use_context::<ApiClient>();
    let image = avatar
        .as_deref()
        .map(|version| api.avatar_url(user_id, size.rendition(), version));
    let tone = avatar_tone(user_id);
    let label = name.clone().unwrap_or_default();

    rsx! {
        span {
            class: "{size.class()} avatar-tone-{tone}",
            role: "img",
            "aria-label": "{label}",
            match image {
                Some(url) => rsx! {
                    img { class: "user-avatar-image", src: "{url}", alt: "", draggable: "false" }
                },
                None => rsx! {
                    span { class: "user-avatar-initials", "aria-hidden": "true",
                        {initials(name.as_deref())}
                    }
                },
            }
        }
    }
}

/// The SHA-256 digest of `data` (FIPS 180-4).
///
/// Written out rather than linked: one tone per user is all the frontend
/// hashes, and the specification's choice of digest is what makes the
/// colour the same on every device; a crate would add to the WebAssembly
/// bundle what these few lines do.
fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a_2f98,
        0x7137_4491,
        0xb5c0_fbcf,
        0xe9b5_dba5,
        0x3956_c25b,
        0x59f1_11f1,
        0x923f_82a4,
        0xab1c_5ed5,
        0xd807_aa98,
        0x1283_5b01,
        0x2431_85be,
        0x550c_7dc3,
        0x72be_5d74,
        0x80de_b1fe,
        0x9bdc_06a7,
        0xc19b_f174,
        0xe49b_69c1,
        0xefbe_4786,
        0x0fc1_9dc6,
        0x240c_a1cc,
        0x2de9_2c6f,
        0x4a74_84aa,
        0x5cb0_a9dc,
        0x76f9_88da,
        0x983e_5152,
        0xa831_c66d,
        0xb003_27c8,
        0xbf59_7fc7,
        0xc6e0_0bf3,
        0xd5a7_9147,
        0x06ca_6351,
        0x1429_2967,
        0x27b7_0a85,
        0x2e1b_2138,
        0x4d2c_6dfc,
        0x5338_0d13,
        0x650a_7354,
        0x766a_0abb,
        0x81c2_c92e,
        0x9272_2c85,
        0xa2bf_e8a1,
        0xa81a_664b,
        0xc24b_8b70,
        0xc76c_51a3,
        0xd192_e819,
        0xd699_0624,
        0xf40e_3585,
        0x106a_a070,
        0x19a4_c116,
        0x1e37_6c08,
        0x2748_774c,
        0x34b0_bcb5,
        0x391c_0cb3,
        0x4ed8_aa4a,
        0x5b9c_ca4f,
        0x682e_6ff3,
        0x748f_82ee,
        0x78a5_636f,
        0x84c8_7814,
        0x8cc7_0208,
        0x90be_fffa,
        0xa450_6ceb,
        0xbef9_a3f7,
        0xc671_78f2,
    ];
    let mut state: [u32; 8] = [
        0x6a09_e667,
        0xbb67_ae85,
        0x3c6e_f372,
        0xa54f_f53a,
        0x510e_527f,
        0x9b05_688c,
        0x1f83_d9ab,
        0x5be0_cd19,
    ];
    let mut message = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bits.to_be_bytes());
    for block in message.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (word, bytes) in w.iter_mut().zip(block.as_chunks::<4>().0) {
            *word = u32::from_be_bytes(*bytes);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
        for (k, w) in K.iter().zip(w) {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choice = (e & f) ^ (!e & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(choice)
                .wrapping_add(*k)
                .wrapping_add(w);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(majority);
            (h, g, f, e, d, c, b, a) = (g, f, e, d.wrapping_add(t1), c, b, a, t1.wrapping_add(t2));
        }
        for (word, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *word = word.wrapping_add(value);
        }
    }
    let mut digest = [0u8; 32];
    for (bytes, word) in digest.as_chunks_mut::<4>().0.iter_mut().zip(state) {
        *bytes = word.to_be_bytes();
    }
    digest
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(digest: [u8; 32]) -> String {
        digest.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn the_digest_is_sha_256() {
        // FIPS 180-4's examples, and a message spanning two blocks.
        assert_eq!(
            hex(sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex(sha256(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn the_initials_are_those_of_the_first_two_words() {
        assert_eq!(initials(Some("Ada Example")), "AE");
        assert_eq!(initials(Some("  jean-paul  de la Forge ")), "JD");
        assert_eq!(initials(Some("Admin")), "A");
        assert_eq!(initials(Some("élise ørsted")), "ÉØ");
        assert_eq!(initials(Some("(Ada) Example")), "AE");
        assert_eq!(initials(Some("   ")), "?");
        assert_eq!(initials(None), "?");
    }

    /// The colour `--name` takes in the shared stylesheet, as RGB.
    fn token(name: &str) -> [f64; 3] {
        let styles = crate::components::layout::LAYOUT_STYLES;
        let start = styles
            .find(&format!("--{name}:"))
            .unwrap_or_else(|| panic!("--{name} is defined"));
        let value = styles[start..].split(':').nth(1).unwrap();
        let hex = value.trim().trim_start_matches('#');
        let channel = |i: usize| f64::from(u8::from_str_radix(&hex[i..i + 2], 16).unwrap());
        [channel(0), channel(2), channel(4)]
    }

    /// WCAG 2's relative luminance.
    fn luminance([r, g, b]: [f64; 3]) -> f64 {
        let linear = |c: f64| {
            let c = c / 255.0;
            if c <= 0.039_28 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
    }

    #[test]
    fn every_avatar_colour_keeps_its_initials_legible() {
        for tone in 0..AVATAR_TONES {
            let background = luminance(token(&format!("avatar-{tone}-bg")));
            let text = luminance(token(&format!("avatar-{tone}-fg")));
            let (light, dark) = if background > text {
                (background, text)
            } else {
                (text, background)
            };
            let contrast = (light + 0.05) / (dark + 0.05);
            assert!(contrast >= 4.5, "avatar {tone}: {contrast:.2}:1");
        }
        // No tone beyond the palette.
        assert!(
            !crate::components::layout::LAYOUT_STYLES
                .contains(&format!("--avatar-{AVATAR_TONES}-bg"))
        );
    }

    #[test]
    fn a_user_keeps_one_colour_chosen_from_the_id() {
        let id = Uuid::parse_str("0195c0de-0000-7000-8000-000000000001").unwrap();
        assert_eq!(
            avatar_tone(id),
            usize::from(sha256(id.as_bytes())[0]) % AVATAR_TONES
        );
        assert_eq!(avatar_tone(id), avatar_tone(id));
        // The id's bytes, not its text, are hashed: the tone of the nil id
        // is that of sixteen zero bytes.
        assert_eq!(
            avatar_tone(Uuid::nil()),
            usize::from(sha256(&[0; 16])[0]) % AVATAR_TONES
        );
        // Ids spread over the palette.
        let tones: std::collections::BTreeSet<usize> = (1..=64u128)
            .map(|n| avatar_tone(Uuid::from_u128(n)))
            .collect();
        assert_eq!(tones.len(), AVATAR_TONES);
    }
}
