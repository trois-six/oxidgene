//! Whether a picture held only as somebody else's address may be loaded
//! (`docs/cross-cutting.md` §7.2).
//!
//! A medium does not have to be a file we hold: a GEDCOM `OBJE.FILE` is
//! routinely a URL, and the reader's engine is what fetches it. A file
//! received from a third party would therefore make every reader's engine
//! call addresses its author chose — a beacon recording who looks at the tree
//! and when, or a blind request to a device of the reader's own network.
//! Nothing here fetches anything: this module only says, for an address,
//! whether the engine may be handed it.
//!
//! The decision is [`remote_load`]: an address of a host the tree trusts is
//! drawn, over `https`; an address of any other public host is drawn once
//! the reader asks; an address a browser would resolve to the reader's own
//! machine or network is never drawn, whatever the trust list says.
//!
//! What it cannot see is a public name that resolves to a private address:
//! resolution is the engine's, after the decision. The classification covers
//! every literal address and the names reserved for local use.

use std::net::{Ipv4Addr, Ipv6Addr};

/// How many hosts a tree's trust list holds at most.
pub const MAX_TRUSTED_HOSTS: usize = 256;

/// The longest host name the DNS allows.
const MAX_HOST_LENGTH: usize = 253;

/// The names reserved for a machine or a network of the reader's own, as
/// whole names or as the last labels of one: `localhost` (RFC 6761),
/// `local` (mDNS, RFC 6762), `internal` (ICANN's reserved private-use name),
/// `lan` (the default domain of home routers) and `home.arpa` (RFC 8375).
const LOCAL_NAMES: [&str; 5] = ["localhost", "local", "internal", "lan", "home.arpa"];

/// An `http(s)` address, split into what the load decision reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteAddress<'a> {
    /// Whether it is `https`.
    pub secure: bool,
    /// The host as a browser resolves it: lowercase, without port or
    /// trailing dot, an IPv4 literal in dotted decimal whatever its
    /// spelling, an IPv6 literal in its canonical form between brackets.
    pub host: String,
    /// What precedes the host in the authority: `user:password@`, or
    /// nothing.
    credentials: &'a str,
    /// The port as written, without its colon.
    port: Option<&'a str>,
    /// Everything after the authority: path, query and fragment.
    rest: &'a str,
}

impl<'a> RemoteAddress<'a> {
    /// Read `address`, or `None` when it is not an `http(s)` address a
    /// browser would request: another scheme, no host, a host no name or
    /// literal address is spelled like.
    ///
    /// A host outside ASCII or holding a percent escape is refused rather
    /// than normalised: a browser maps both before resolving, full-width
    /// digits to an IPv4 literal among other things, and a mapping not
    /// reproduced here must not be a way around the classification. A name
    /// in another script is given in its `xn--` form.
    #[must_use]
    pub fn parse(address: &'a str) -> Option<Self> {
        let address = address.trim();
        let (secure, after_scheme) = match address.strip_prefix("https://") {
            Some(rest) => (true, rest),
            None => (false, address.strip_prefix("http://")?),
        };
        // A browser ends the authority at a backslash as at a slash.
        let end = after_scheme
            .find(['/', '\\', '?', '#'])
            .unwrap_or(after_scheme.len());
        let (authority, rest) = after_scheme.split_at(end);
        let (credentials, host_port) = match authority.rfind('@') {
            Some(at) => authority.split_at(at + 1),
            None => ("", authority),
        };
        let (host, port) = split_port(host_port)?;
        Some(Self {
            secure,
            host: canonical_host(host)?,
            credentials,
            port,
            rest,
        })
    }

    /// Whether the host is one of the public network: not a literal address
    /// of a private, loopback, link-local or otherwise reserved range, and
    /// not a name reserved for local use or without a dot.
    #[must_use]
    pub fn is_public(&self) -> bool {
        is_public_host(&self.host)
    }

    /// The address over `https`: itself when it is, else the same host,
    /// path and query under the other scheme, a port `80` left out.
    #[must_use]
    pub fn over_https(&self) -> String {
        let port = match self.port {
            Some(port) if self.secure || port != "80" => format!(":{port}"),
            _ => String::new(),
        };
        let host = self.host.as_str();
        format!("https://{}{host}{port}{}", self.credentials, self.rest)
    }

    /// The address under its own scheme.
    fn as_written(&self) -> String {
        let scheme = if self.secure { "https" } else { "http" };
        let port = self.port.map(|port| format!(":{port}")).unwrap_or_default();
        let host = self.host.as_str();
        format!("{scheme}://{}{host}{port}{}", self.credentials, self.rest)
    }
}

/// `host[:port]` split in two; `None` for a port that is not digits or an
/// IPv6 literal that does not close.
fn split_port(host_port: &str) -> Option<(&str, Option<&str>)> {
    let (host, port) = if host_port.starts_with('[') {
        let close = host_port.find(']')?;
        let (host, after) = host_port.split_at(close + 1);
        match after.strip_prefix(':') {
            Some(port) => (host, Some(port)),
            None if after.is_empty() => (host, None),
            None => return None,
        }
    } else {
        match host_port.rsplit_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (host_port, None),
        }
    };
    let port = port.filter(|port| !port.is_empty());
    port.is_none_or(|port| port.bytes().all(|b| b.is_ascii_digit()))
        .then_some((host, port))
}

/// The host as a browser resolves it, or `None` when it is no host.
fn canonical_host(host: &str) -> Option<String> {
    if let Some(literal) = host.strip_prefix('[') {
        let address: Ipv6Addr = literal.strip_suffix(']')?.parse().ok()?;
        return Some(format!("[{address}]"));
    }
    let host = host.strip_suffix('.').unwrap_or(host).to_ascii_lowercase();
    let spelled = !host.is_empty()
        && host.len() <= MAX_HOST_LENGTH
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_'))
        && host.split('.').all(|label| !label.is_empty());
    if !spelled {
        return None;
    }
    match ipv4_literal(&host) {
        Some(Ok(address)) => Some(address.to_string()),
        // It ends in a number without being an address: no browser
        // requests it.
        Some(Err(())) => None,
        None => Some(host),
    }
}

/// The IPv4 address `host` spells, as a browser reads it: one to four
/// numbers, each decimal, octal (`0177`) or hexadecimal (`0x7f`), the last
/// one filling the bytes the others leave (`127.1`, `2130706433`).
///
/// `None` for a host that does not end in a number, which is a name;
/// `Some(Err)` for one that does and spells no address.
fn ipv4_literal(host: &str) -> Option<Result<Ipv4Addr, ()>> {
    let parts: Vec<&str> = host.split('.').collect();
    let last = parts.last()?;
    let ends_in_number = last.bytes().all(|b| b.is_ascii_digit()) || number(last).is_some();
    if !ends_in_number {
        return None;
    }
    Some(ipv4_of(&parts).ok_or(()))
}

fn ipv4_of(parts: &[&str]) -> Option<Ipv4Addr> {
    let (last, leading) = parts.split_last()?;
    if leading.len() > 3 {
        return None;
    }
    let mut address: u64 = 0;
    for part in leading {
        let byte = number(part).filter(|byte| *byte <= 0xff)?;
        address = (address << 8) | byte;
    }
    let free_bytes = 4 - u32::try_from(leading.len()).ok()?;
    let last = number(last).filter(|last| *last < 1_u64 << (8 * free_bytes))?;
    let address = (address << (8 * free_bytes)) | last;
    u32::try_from(address).ok().map(Ipv4Addr::from)
}

/// One number of an IPv4 literal: `0x` hexadecimal, a leading `0` octal,
/// decimal otherwise; `None` for anything else. A number too large to hold
/// is still one — it ends a host in a number — and reads as the largest.
fn number(text: &str) -> Option<u64> {
    let (digits, radix) = match text.strip_prefix("0x") {
        Some(hexadecimal) => (hexadecimal, 16),
        None if text.len() > 1 && text.starts_with('0') => (&text[1..], 8),
        None => (text, 10),
    };
    if digits.is_empty() {
        // `0x` alone is zero; an empty number is none.
        return (radix == 16).then_some(0);
    }
    digits
        .chars()
        .all(|digit| digit.is_digit(radix))
        .then(|| u64::from_str_radix(digits, radix).unwrap_or(u64::MAX))
}

/// Whether `host`, as [`RemoteAddress::parse`] canonicalises it, is one of
/// the public network.
fn is_public_host(host: &str) -> bool {
    if let Some(literal) = host.strip_prefix('[') {
        return literal
            .strip_suffix(']')
            .and_then(|literal| literal.parse().ok())
            .is_some_and(is_public_v6);
    }
    if let Ok(address) = host.parse::<Ipv4Addr>() {
        return is_public_v4(address);
    }
    let local = LOCAL_NAMES.iter().any(|name| {
        host == *name
            || host
                .strip_suffix(name)
                .is_some_and(|rest| rest.ends_with('.'))
    });
    !local && host.contains('.')
}

/// Whether an IPv4 address is one of the public network: outside "this
/// network" (0/8), the private ranges (10/8, 172.16/12, 192.168/16), the
/// carrier-grade NAT range (100.64/10), loopback (127/8), link-local
/// (169.254/16), the protocol and benchmarking assignments (192.0.0/24,
/// 198.18/15), multicast and the reserved top of the space (224/3).
fn is_public_v4(address: Ipv4Addr) -> bool {
    let [a, b, c, _] = address.octets();
    !(a == 0
        || a == 10
        || a == 127
        || (a == 100 && (64..128).contains(&b))
        || (a == 169 && b == 254)
        || (a == 172 && (16..32).contains(&b))
        || (a == 192 && b == 168)
        || (a == 192 && b == 0 && c == 0)
        || (a == 198 && (18..20).contains(&b))
        || a >= 224)
}

/// Whether an IPv6 address is one of the public network: not unspecified
/// or loopback, not unique-local (fc00::/7), link-local (fe80::/10),
/// site-local (fec0::/10) or multicast (ff00::/8), and, for the forms that
/// carry an IPv4 address — mapped, compatible, NAT64 (64:ff9b::/96), 6to4
/// (2002::/16) — not one whose IPv4 address is not public. The NAT64 prefix
/// for local use (64:ff9b:1::/48) is not public either, nor is the rest of
/// the reserved ::/16, the IPv4-translated form (::ffff:0:0/96) among it.
fn is_public_v6(address: Ipv6Addr) -> bool {
    let segments = address.segments();
    let embedded = |high: u16, low: u16| {
        let [a, b] = high.to_be_bytes();
        let [c, d] = low.to_be_bytes();
        is_public_v4(Ipv4Addr::new(a, b, c, d))
    };
    match segments {
        // Unspecified, loopback, and the IPv4-compatible form.
        [0, 0, 0, 0, 0, 0, high, low] => (high, low) > (0, 1) && embedded(high, low),
        [0, 0, 0, 0, 0, 0xffff, high, low] | [0x64, 0xff9b, 0, 0, 0, 0, high, low] => {
            embedded(high, low)
        }
        [0, ..] | [0x64, 0xff9b, 1, ..] => false,
        [0x2002, high, low, ..] => embedded(high, low),
        [first, ..] => {
            !((first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
                || (first & 0xffc0) == 0xfec0)
                && (first & 0xff00) != 0xff00
        }
    }
}

/// What a tree's trust says of a host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostTrust {
    /// Nothing: the reader decides.
    Unknown,
    /// Its pictures are loaded, over `https`.
    Trusted,
    /// Its pictures are loaded under the scheme their address states: an
    /// archive portal the catalogue lists as answering over plain `http`
    /// only (`docs/archives.md` §3.1).
    TrustedPlainHttp,
}

/// Whether the reader's engine is handed a remote address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteLoad {
    /// Yes, as this address: the record's own, or its `https` spelling.
    Load(String),
    /// Not until the reader asks for the pictures of this host.
    Ask(String),
    /// Never: not an address of the public network. The host, when the
    /// address names one.
    Refuse(Option<String>),
}

/// Whether `address` is loaded, `trust` saying what the tree and the
/// catalogue make of its host.
///
/// A private, loopback or link-local host is refused before `trust` is
/// asked: no list makes the reader's own network a place to send requests
/// to. A plain `http` address is loaded over `https` unless its host is
/// trusted for plain `http`, so that the request and what it names are not
/// readable on the way.
#[must_use]
pub fn remote_load(address: &str, trust: impl FnOnce(&str) -> HostTrust) -> RemoteLoad {
    let Some(parsed) = RemoteAddress::parse(address) else {
        return RemoteLoad::Refuse(None);
    };
    if !parsed.is_public() {
        return RemoteLoad::Refuse(Some(parsed.host));
    }
    match trust(&parsed.host) {
        HostTrust::Unknown => RemoteLoad::Ask(parsed.host),
        HostTrust::Trusted => RemoteLoad::Load(parsed.over_https()),
        HostTrust::TrustedPlainHttp => RemoteLoad::Load(parsed.as_written()),
    }
}

/// The host of `address` when it is one a tree may trust: a public one.
#[must_use]
pub fn trustable_host(address: &str) -> Option<String> {
    RemoteAddress::parse(address)
        .filter(RemoteAddress::is_public)
        .map(|address| address.host)
}

/// The host a trust-list entry names: a host, or an address whose host is
/// taken. Refused when it names none, or one that is never loaded.
pub fn trusted_host(entry: &str) -> Result<String, String> {
    let entry = entry.trim();
    let address = if crate::types::is_remote_url(entry) {
        entry.to_string()
    } else {
        format!("https://{entry}")
    };
    let Some(parsed) = RemoteAddress::parse(&address) else {
        return Err(format!("`{entry}` is not a host name"));
    };
    if !parsed.is_public() {
        return Err(format!(
            "`{}` is a private or local host, which is never loaded",
            parsed.host
        ));
    }
    Ok(parsed.host)
}

/// A trust list as it is stored: each entry's host ([`trusted_host`]),
/// sorted, each once. Refused beyond [`MAX_TRUSTED_HOSTS`].
pub fn trusted_hosts(entries: &[String]) -> Result<Vec<String>, String> {
    let mut hosts = entries
        .iter()
        .map(|entry| trusted_host(entry))
        .collect::<Result<Vec<_>, _>>()?;
    hosts.sort();
    hosts.dedup();
    if hosts.len() > MAX_TRUSTED_HOSTS {
        return Err(format!(
            "a tree trusts at most {MAX_TRUSTED_HOSTS} media hosts"
        ));
    }
    Ok(hosts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(address: &str) -> Option<String> {
        RemoteAddress::parse(address).map(|address| address.host)
    }

    fn public(address: &str) -> bool {
        RemoteAddress::parse(address).is_some_and(|address| address.is_public())
    }

    fn untrusted(_: &str) -> HostTrust {
        HostTrust::Unknown
    }

    fn trusted(_: &str) -> HostTrust {
        HostTrust::Trusted
    }

    #[test]
    fn the_host_is_what_a_browser_resolves() {
        for (address, expected) in [
            (
                "https://archives.example.org/scan/42.jpg",
                "archives.example.org",
            ),
            ("  http://Archives.Example.ORG/x  ", "archives.example.org"),
            ("https://archives.example.org", "archives.example.org"),
            ("https://archives.example.org.", "archives.example.org"),
            (
                "https://archives.example.org:8443/x",
                "archives.example.org",
            ),
            ("https://archives.example.org?x=1", "archives.example.org"),
            ("https://archives.example.org#top", "archives.example.org"),
            // The authority ends at a backslash, and starts after the
            // credentials: the host is not the one the eye reads first.
            (
                "https://archives.example.org\\@127.0.0.1/x",
                "archives.example.org",
            ),
            ("https://archives.example.org@127.0.0.1/x", "127.0.0.1"),
            (
                "https://user:secret@archives.example.org/x",
                "archives.example.org",
            ),
            ("https://a@b@archives.example.org/x", "archives.example.org"),
            (
                "https://xn--archives-dpartement-e2b.example/x",
                "xn--archives-dpartement-e2b.example",
            ),
            ("https://[2001:DB8:0:0::1]/x", "[2001:db8::1]"),
            ("https://[2001:db8::1]:8443/x", "[2001:db8::1]"),
        ] {
            assert_eq!(host(address).as_deref(), Some(expected), "{address}");
        }
    }

    #[test]
    fn what_no_browser_would_request_has_no_host() {
        for address in [
            "",
            "media/photo.jpg",
            "D:\\Photos\\portrait.jpg",
            "ftp://archives.example.org/x.jpg",
            "HTTPS://archives.example.org/x.jpg",
            "//archives.example.org/x.jpg",
            "https://",
            "https:///x.jpg",
            "https://:443/x.jpg",
            "https://archives.example.org:port/x",
            "https://archives example.org/x",
            "https://archives..example.org/x",
            "https://.example.org/x",
            "https://archives.example.org../x",
            // Mapped by a browser before it resolves: not reproduced here,
            // so not accepted.
            "https://%31%32%37.0.0.1/x",
            "https://１２７.０.０.１/x",
            "https://archives。example。org/x",
            "https://département.example/x",
            "https://[::1/x",
            "https://[::1]x/y",
            "https://[not-an-address]/x",
            // Ends in a number without spelling an address.
            "https://1.2.3.4.5/x",
            "https://256.1.1.1/x",
            "https://1.2.3.256/x",
            "https://example.0x100000000/x",
            "https://1.2.3.08/x",
            "https://4294967296/x",
        ] {
            assert_eq!(host(address), None, "{address}");
        }
    }

    #[test]
    fn every_spelling_of_an_ipv4_address_is_read_as_that_address() {
        for (address, expected) in [
            ("https://127.0.0.1/x", "127.0.0.1"),
            ("https://127.1/x", "127.0.0.1"),
            ("https://127.0.1/x", "127.0.0.1"),
            ("https://2130706433/x", "127.0.0.1"),
            ("https://0x7f000001/x", "127.0.0.1"),
            ("https://0x7f.0x0.0x0.0x1/x", "127.0.0.1"),
            ("https://0X7F.1/x", "127.0.0.1"),
            ("https://0177.0.0.01/x", "127.0.0.1"),
            ("https://017700000001/x", "127.0.0.1"),
            ("https://127.0.0.1./x", "127.0.0.1"),
            ("https://0xc0.0250.513/x", "192.168.2.1"),
            ("https://3232235777/x", "192.168.1.1"),
            ("https://0/x", "0.0.0.0"),
            ("https://0x/x", "0.0.0.0"),
            ("https://192.0x/x", "192.0.0.0"),
            ("https://203.0.113.7/x", "203.0.113.7"),
        ] {
            assert_eq!(host(address).as_deref(), Some(expected), "{address}");
        }
    }

    #[test]
    fn private_loopback_and_link_local_ipv4_ranges_are_not_public() {
        for address in [
            "0.0.0.0",
            "0.255.255.255",
            "10.0.0.1",
            "10.255.255.255",
            "100.64.0.1",
            "100.127.255.255",
            "127.0.0.1",
            "127.255.255.254",
            "169.254.0.1",
            "169.254.169.254",
            "172.16.0.1",
            "172.31.255.255",
            "192.0.0.1",
            "192.168.0.1",
            "192.168.255.255",
            "198.18.0.1",
            "198.19.255.255",
            "224.0.0.1",
            "239.255.255.250",
            "240.0.0.1",
            "255.255.255.255",
            // The same ranges under their other spellings.
            "2130706433",
            "0x7f.1",
            "0300.0250.1",
            "10.1",
            "0xa9fea9fe",
        ] {
            assert!(!public(&format!("https://{address}/x")), "{address}");
        }
    }

    #[test]
    fn the_addresses_beside_the_reserved_ipv4_ranges_are_public() {
        for address in [
            "1.1.1.1",
            "9.255.255.255",
            "11.0.0.1",
            "100.63.255.255",
            "100.128.0.1",
            "126.255.255.255",
            "128.0.0.1",
            "169.253.255.255",
            "169.255.0.1",
            "172.15.255.255",
            "172.32.0.1",
            "192.0.1.1",
            "192.167.255.255",
            "192.169.0.1",
            "198.17.255.255",
            "198.20.0.1",
            "203.0.113.7",
            "223.255.255.255",
        ] {
            assert!(public(&format!("https://{address}/x")), "{address}");
        }
    }

    #[test]
    fn local_ipv6_addresses_and_those_carrying_a_local_ipv4_are_not_public() {
        for address in [
            "::",
            "::1",
            "0:0:0:0:0:0:0:1",
            "fc00::1",
            "fd12:3456:789a::1",
            "fdff:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
            "fe80::1",
            "febf::1",
            "fec0::1",
            "ff02::1",
            "ff0e::1",
            // IPv4-mapped, in both spellings.
            "::ffff:127.0.0.1",
            "::ffff:7f00:1",
            "::ffff:10.0.0.1",
            "::ffff:192.168.1.1",
            "::ffff:169.254.169.254",
            // IPv4-compatible.
            "::127.0.0.1",
            "::10.0.0.1",
            // IPv4-translated, and the rest of the reserved ::/16.
            "::ffff:0:7f00:1",
            "::ffff:0:203.0.113.7",
            "::1:0:0:1",
            "0:1::1",
            // NAT64 and 6to4 of a private address.
            "64:ff9b::10.0.0.1",
            "64:ff9b::7f00:1",
            "64:ff9b:1::1",
            "2002:7f00:1::1",
            "2002:c0a8:101::1",
        ] {
            assert!(!public(&format!("https://[{address}]/x")), "{address}");
        }
    }

    #[test]
    fn global_ipv6_addresses_are_public() {
        for address in [
            "2001:db8::1",
            "2606:4700:4700::1111",
            "fbff::1",
            "fe00::1",
            "::ffff:203.0.113.7",
            "::203.0.113.7",
            "64:ff9b::203.0.113.7",
            "2002:cb00:7107::1",
        ] {
            assert!(public(&format!("https://[{address}]/x")), "{address}");
        }
    }

    #[test]
    fn names_reserved_for_local_use_and_bare_labels_are_not_public() {
        for name in [
            "localhost",
            "LOCALHOST",
            "localhost.",
            "printer.localhost",
            "nas.local",
            "a.b.local",
            "local",
            "wiki.internal",
            "internal",
            "router.lan",
            "lan",
            "nas.home.arpa",
            "home.arpa",
            "intranet",
            "nas",
            "my_nas",
        ] {
            assert!(!public(&format!("https://{name}/x")), "{name}");
        }
    }

    #[test]
    fn names_that_only_look_local_are_public() {
        for name in [
            "archives.example.org",
            "localhost.example.org",
            "notlocal.example",
            "mylocal.example",
            "example.locale",
            "xlan.example",
            "home.arpa.example.org",
            "internal.example.org",
        ] {
            assert!(public(&format!("https://{name}/x")), "{name}");
        }
    }

    #[test]
    fn an_unknown_public_host_is_asked_about_and_named() {
        assert_eq!(
            remote_load("https://Tracker.Example.org/pixel.gif?id=7", untrusted),
            RemoteLoad::Ask("tracker.example.org".into())
        );
        assert_eq!(
            remote_load("http://tracker.example.org/pixel.gif", untrusted),
            RemoteLoad::Ask("tracker.example.org".into())
        );
    }

    #[test]
    fn a_trusted_host_is_loaded_over_https() {
        assert_eq!(
            remote_load(
                " https://archives.example.org/scan/42.jpg?size=full#p ",
                trusted
            ),
            RemoteLoad::Load("https://archives.example.org/scan/42.jpg?size=full#p".into())
        );
        for (address, loaded) in [
            (
                "http://archives.example.org/scan/42.jpg",
                "https://archives.example.org/scan/42.jpg",
            ),
            (
                "http://archives.example.org:80/42.jpg",
                "https://archives.example.org/42.jpg",
            ),
            (
                "http://archives.example.org:8080/42.jpg",
                "https://archives.example.org:8080/42.jpg",
            ),
            (
                "http://archives.example.org",
                "https://archives.example.org",
            ),
            (
                "http://reader@archives.example.org/42.jpg",
                "https://reader@archives.example.org/42.jpg",
            ),
            (
                "https://archives.example.org:80/42.jpg",
                "https://archives.example.org:80/42.jpg",
            ),
        ] {
            assert_eq!(
                remote_load(address, trusted),
                RemoteLoad::Load(loaded.into()),
                "{address}"
            );
        }
    }

    #[test]
    fn only_a_host_trusted_for_plain_http_keeps_its_scheme() {
        let plain = |_: &str| HostTrust::TrustedPlainHttp;
        assert_eq!(
            remote_load("http://portal.example.org/view/7.jpg", plain),
            RemoteLoad::Load("http://portal.example.org/view/7.jpg".into())
        );
        assert_eq!(
            remote_load("https://portal.example.org/view/7.jpg", plain),
            RemoteLoad::Load("https://portal.example.org/view/7.jpg".into())
        );
    }

    #[test]
    fn the_trust_is_asked_for_the_host_the_browser_resolves() {
        let mut asked = None;
        let _ = remote_load(
            "https://archives.example.org@Other.Example.net:8443/x",
            |host| {
                asked = Some(host.to_string());
                HostTrust::Unknown
            },
        );
        assert_eq!(asked.as_deref(), Some("other.example.net"));
    }

    #[test]
    fn a_local_host_is_refused_whatever_the_trust_says() {
        for address in [
            "https://127.0.0.1/x.jpg",
            "http://localhost:8080/x.jpg",
            "https://192.168.1.10/camera.jpg",
            "https://10.0.0.5/x.jpg",
            "https://172.20.1.1/x.jpg",
            "https://169.254.169.254/latest/meta-data",
            "https://100.64.0.9/x.jpg",
            "https://2130706433/x.jpg",
            "https://0x7f.1/x.jpg",
            "https://[::1]/x.jpg",
            "https://[fd00::5]/x.jpg",
            "https://[fe80::1]/x.jpg",
            "https://[::ffff:192.168.1.10]/x.jpg",
            "https://nas.local/x.jpg",
            "https://printer.lan/x.jpg",
            "https://wiki.internal/x.jpg",
            "https://nas.home.arpa/x.jpg",
            "https://nas/x.jpg",
            "https://archives.example.org@192.168.1.10/x.jpg",
        ] {
            let mut asked = false;
            let load = remote_load(address, |_| {
                asked = true;
                HostTrust::TrustedPlainHttp
            });
            assert!(
                matches!(load, RemoteLoad::Refuse(Some(_))),
                "{address}: {load:?}"
            );
            assert!(!asked, "{address}: the trust was asked");
        }
    }

    #[test]
    fn what_is_no_address_is_refused_without_a_host() {
        for address in [
            "",
            "media/photo.jpg",
            "ftp://archives.example.org/x",
            "https://１２７.０.０.１/x",
        ] {
            assert_eq!(
                remote_load(address, trusted),
                RemoteLoad::Refuse(None),
                "{address}"
            );
        }
    }

    #[test]
    fn a_trust_entry_is_a_public_host_or_an_address_of_one() {
        for (entry, expected) in [
            ("archives.example.org", "archives.example.org"),
            ("  Archives.Example.ORG. ", "archives.example.org"),
            ("archives.example.org:8443", "archives.example.org"),
            (
                "https://archives.example.org/scan/42.jpg",
                "archives.example.org",
            ),
            ("http://archives.example.org", "archives.example.org"),
            ("archives.example.org/scan", "archives.example.org"),
            ("203.0.113.7", "203.0.113.7"),
            ("[2001:db8::1]", "[2001:db8::1]"),
        ] {
            assert_eq!(trusted_host(entry).as_deref(), Ok(expected), "{entry}");
        }
        for entry in [
            "",
            "   ",
            "archives example.org",
            "ftp://archives.example.org",
            "localhost",
            "nas.local",
            "192.168.1.10",
            "0x7f.1",
            "[::1]",
            "nas",
            "*.example.org",
            "département.example",
        ] {
            assert!(trusted_host(entry).is_err(), "{entry}");
        }
    }

    #[test]
    fn a_trust_list_is_stored_sorted_without_repeats_and_bounded() {
        let entries = [
            "media.example.org",
            "https://archives.example.org/x.jpg",
            "ARCHIVES.example.org",
        ]
        .map(str::to_string);
        assert_eq!(
            trusted_hosts(&entries),
            Ok(vec![
                "archives.example.org".to_string(),
                "media.example.org".to_string()
            ])
        );
        assert!(trusted_hosts(&["localhost".to_string()]).is_err());
        let hosts = |count: usize| -> Vec<String> {
            (0..count)
                .map(|index| format!("host{index}.example.org"))
                .collect()
        };
        assert!(trusted_hosts(&hosts(MAX_TRUSTED_HOSTS)).is_ok());
        assert!(trusted_hosts(&hosts(MAX_TRUSTED_HOSTS + 1)).is_err());
    }

    #[test]
    fn only_a_public_host_is_worth_trusting() {
        assert_eq!(
            trustable_host("http://archives.example.org/view/7.jpg").as_deref(),
            Some("archives.example.org")
        );
        for address in [
            "https://192.168.1.10/x.jpg",
            "https://nas.local/x.jpg",
            "scans/7.jpg",
            "",
        ] {
            assert_eq!(trustable_host(address), None, "{address}");
        }
    }
}
