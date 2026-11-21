//! The address a request comes from (docs/authentication.md §9.2): what the
//! per-address rate limits count by.
//!
//! The server hands each connection's peer address to [`client_addresses`],
//! which reads the client behind the reverse proxies the operator named in
//! `OXIDGENE_TRUSTED_PROXIES`. A forwarded header is believed only when the
//! peer that sent it is one of them: anybody can write `X-Forwarded-For`,
//! and a client believed on its word would choose the address it is counted
//! under. The address is held in a counter for its window and nowhere else:
//! never stored, never logged.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::Router;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::HeaderMap;
use axum::middleware::{self, Next};
use axum::response::Response;

/// The header reverse proxies list the addresses a request came through in.
const FORWARDED_FOR: &str = "x-forwarded-for";

/// The address of the client of a request, as far as the trusted proxies
/// tell: a request extension, absent when the server was given no peer
/// address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientAddress(pub IpAddr);

impl ClientAddress {
    /// What the address is counted under: itself for IPv4, its /64 for
    /// IPv6, where one subscriber holds a whole prefix.
    pub(crate) fn bucket(self) -> String {
        match self.0 {
            IpAddr::V4(address) => address.to_string(),
            IpAddr::V6(address) => {
                let [a, b, c, d, ..] = address.segments();
                format!("{a:x}:{b:x}:{c:x}:{d:x}::/64")
            }
        }
    }
}

/// An address range, as `OXIDGENE_TRUSTED_PROXIES` writes one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Network {
    address: IpAddr,
    prefix: u32,
}

impl Network {
    /// `address` or `address/prefix`.
    fn parse(text: &str) -> Option<Self> {
        let (address, prefix) = match text.split_once('/') {
            Some((address, prefix)) => (address, Some(prefix)),
            None => (text, None),
        };
        let address = address.parse::<IpAddr>().ok()?;
        let width = match address {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        };
        let prefix = match prefix {
            Some(prefix) => prefix.parse::<u32>().ok().filter(|bits| *bits <= width)?,
            None => width,
        };
        Some(Self { address, prefix })
    }

    fn contains(&self, address: IpAddr) -> bool {
        match (self.address, address) {
            (IpAddr::V4(network), IpAddr::V4(address)) => {
                let mask = u32::MAX.checked_shl(32 - self.prefix).unwrap_or(0);
                u32::from(network) & mask == u32::from(address) & mask
            }
            (IpAddr::V6(network), IpAddr::V6(address)) => {
                let mask = u128::MAX.checked_shl(128 - self.prefix).unwrap_or(0);
                u128::from(network) & mask == u128::from(address) & mask
            }
            _ => false,
        }
    }
}

/// `OXIDGENE_TRUSTED_PROXIES` names something that is neither an address
/// nor a range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidTrustedProxies;

/// The reverse proxies in front of the server: the only peers whose
/// `X-Forwarded-For` is believed.
#[derive(Debug, Clone, Default)]
pub struct TrustedProxies(Arc<[Network]>);

impl TrustedProxies {
    /// No proxy: every request's client is its peer.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// The comma-separated addresses and CIDR ranges of `list`
    /// (`127.0.0.1, 10.0.0.0/8, fd00::/8`); blank entries are ignored.
    ///
    /// # Errors
    ///
    /// When an entry is neither an address nor a range.
    pub fn parse(list: &str) -> Result<Self, InvalidTrustedProxies> {
        list.split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(|entry| Network::parse(entry).ok_or(InvalidTrustedProxies))
            .collect::<Result<Vec<_>, _>>()
            .map(|networks| Self(networks.into()))
    }

    fn trusts(&self, address: IpAddr) -> bool {
        self.0.iter().any(|network| network.contains(address))
    }

    /// The client of a request whose connection comes from `peer`.
    ///
    /// The peer itself unless it is a trusted proxy; then the nearest
    /// address of its `X-Forwarded-For` that is not one — each proxy
    /// appends the address it received from, so the list is read from its
    /// end, and what stands before the first untrusted address is the
    /// client's own writing. A list naming trusted proxies only gives its
    /// first address, and one that cannot be read the last address known.
    #[must_use]
    pub fn client(&self, peer: IpAddr, headers: &HeaderMap) -> IpAddr {
        let mut client = peer.to_canonical();
        if !self.trusts(client) {
            return client;
        }
        let forwarded = headers
            .get_all(FORWARDED_FOR)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(|value| value.split(','))
            .collect::<Vec<_>>();
        for entry in forwarded.into_iter().rev() {
            let Some(address) = forwarded_address(entry) else {
                break;
            };
            client = address;
            if !self.trusts(client) {
                break;
            }
        }
        client
    }
}

/// The address of one `X-Forwarded-For` entry: an address, an IPv4 address
/// with a port, or a bracketed IPv6 address with or without one.
fn forwarded_address(entry: &str) -> Option<IpAddr> {
    let entry = entry.trim();
    let address = entry
        .parse::<IpAddr>()
        .ok()
        .or_else(|| entry.parse::<SocketAddr>().ok().map(|socket| socket.ip()))
        .or_else(|| {
            entry
                .strip_prefix('[')?
                .strip_suffix(']')?
                .parse::<IpAddr>()
                .ok()
        })?;
    Some(address.to_canonical())
}

/// Give every request of `router` its [`ClientAddress`], read from the peer
/// address the server serves with (`ConnectInfo<SocketAddr>`) and
/// `proxies`.
pub fn client_addresses(router: Router, proxies: TrustedProxies) -> Router {
    router.layer(middleware::from_fn_with_state(proxies, resolve))
}

/// Whether the warning of [`warn_untrusted`] was given.
static WARNED: AtomicBool = AtomicBool::new(false);

/// Say once that a forwarded header came from a peer nobody named: either
/// a client forging one, or a reverse proxy missing from
/// `OXIDGENE_TRUSTED_PROXIES`, behind which every visitor counts as one.
/// No address is logged.
fn warn_untrusted() {
    if !WARNED.swap(true, Ordering::Relaxed) {
        tracing::warn!(
            auth = "proxy_untrusted",
            "A request carried X-Forwarded-For from a peer OXIDGENE_TRUSTED_PROXIES does not \
             name: the header is ignored and the peer is counted as the client"
        );
    }
}

async fn resolve(
    State(proxies): State<TrustedProxies>,
    mut request: Request,
    next: Next,
) -> Response {
    if let Some(ConnectInfo(peer)) = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .copied()
    {
        let peer = peer.ip();
        if request.headers().contains_key(FORWARDED_FOR) && !proxies.trusts(peer.to_canonical()) {
            warn_untrusted();
        }
        let client = proxies.client(peer, request.headers());
        request.extensions_mut().insert(ClientAddress(client));
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn forwarded(values: &[&'static str]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for value in values {
            headers.append(FORWARDED_FOR, HeaderValue::from_static(value));
        }
        headers
    }

    fn address(text: &str) -> IpAddr {
        text.parse().expect("an address")
    }

    #[test]
    fn an_untrusted_peer_is_the_client_whatever_it_forwards() {
        let headers = forwarded(&["198.51.100.7"]);
        for proxies in [
            TrustedProxies::none(),
            TrustedProxies::parse("10.0.0.0/8").expect("valid"),
        ] {
            assert_eq!(
                proxies.client(address("203.0.113.9"), &headers),
                address("203.0.113.9")
            );
        }
    }

    #[test]
    fn a_trusted_proxy_names_the_nearest_address_that_is_not_one() {
        let proxies = TrustedProxies::parse("10.0.0.0/8, 192.0.2.1").expect("valid");
        // The client wrote the first entry itself; the proxies appended the
        // two others.
        let headers = forwarded(&["198.51.100.99, 203.0.113.9", "10.1.2.3"]);
        assert_eq!(
            proxies.client(address("192.0.2.1"), &headers),
            address("203.0.113.9")
        );
        // Proxies alone: the first of them.
        assert_eq!(
            proxies.client(address("10.0.0.1"), &forwarded(&["10.9.9.9, 10.1.2.3"])),
            address("10.9.9.9")
        );
        // Nothing forwarded, or nothing readable: the last address known.
        assert_eq!(
            proxies.client(address("10.0.0.1"), &HeaderMap::new()),
            address("10.0.0.1")
        );
        assert_eq!(
            proxies.client(address("10.0.0.1"), &forwarded(&["unknown, 10.1.2.3"])),
            address("10.1.2.3")
        );
    }

    #[test]
    fn forwarded_entries_are_read_with_their_port_and_brackets() {
        let proxies = TrustedProxies::parse("::1, 127.0.0.0/8, fd00::/8").expect("valid");
        for (entry, client) in [
            ("203.0.113.9:4711", "203.0.113.9"),
            ("[2001:db8::7]:4711", "2001:db8::7"),
            ("[2001:db8::7]", "2001:db8::7"),
            ("::ffff:203.0.113.9", "203.0.113.9"),
        ] {
            let mut headers = HeaderMap::new();
            headers.insert(FORWARDED_FOR, HeaderValue::from_str(entry).expect("header"));
            assert_eq!(
                proxies.client(address("::1"), &headers),
                address(client),
                "{entry}"
            );
        }
        // An IPv4 peer reached over a dual-stack socket is its IPv4 self.
        assert!(proxies.trusts(address("::ffff:127.0.0.1").to_canonical()));
        assert!(proxies.trusts(address("fd12::1")));
        assert!(!proxies.trusts(address("2001:db8::1")));
    }

    #[test]
    fn ranges_are_addresses_with_an_optional_prefix() {
        assert!(TrustedProxies::parse("").expect("valid").0.is_empty());
        assert!(
            TrustedProxies::parse("0.0.0.0/0")
                .expect("valid")
                .trusts(address("203.0.113.9"))
        );
        for refused in [
            "proxy.example.invalid",
            "10.0.0.0/33",
            "fd00::/129",
            "10.0.0.0/",
        ] {
            assert_eq!(
                TrustedProxies::parse(refused).err(),
                Some(InvalidTrustedProxies),
                "{refused}"
            );
        }
    }

    #[test]
    fn an_ipv6_client_is_counted_by_its_prefix() {
        assert_eq!(
            ClientAddress(address("203.0.113.9")).bucket(),
            "203.0.113.9"
        );
        assert_eq!(
            ClientAddress(address("2001:db8:1:2:3:4:5:6")).bucket(),
            ClientAddress(address("2001:db8:1:2::1")).bucket()
        );
        assert_ne!(
            ClientAddress(address("2001:db8:1:2::1")).bucket(),
            ClientAddress(address("2001:db8:1:3::1")).bucket()
        );
    }
}
