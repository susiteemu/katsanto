//! The client's IP address for rate limiting, also behind a reverse proxy.
//!
//! `X-Forwarded-For` is only believed when the connection comes from a configured
//! trusted proxy; anyone else could put any address in it.

use std::{
    net::{IpAddr, Ipv6Addr, SocketAddr},
    sync::Arc,
};

use axum::{extract::ConnectInfo, http::Request};
use tower_governor::{GovernorError, key_extractor::KeyExtractor};

/// An IP network like `10.0.0.0/8`, or a single address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IpNet {
    addr: IpAddr,
    prefix: u8,
}

impl IpNet {
    pub fn parse(s: &str) -> Result<Self, String> {
        let s = s.trim();
        let invalid = || format!("invalid IP address or network: {s:?}");
        let (addr, prefix) = match s.split_once('/') {
            Some((a, p)) => (a, Some(p.parse::<u8>().map_err(|_| invalid())?)),
            None => (s, None),
        };
        let addr: IpAddr = addr.parse().map_err(|_| invalid())?;
        let max = if addr.is_ipv4() { 32 } else { 128 };
        let prefix = prefix.unwrap_or(max);
        if prefix > max {
            return Err(invalid());
        }
        Ok(Self {
            addr: addr.to_canonical(),
            prefix,
        })
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        match (self.addr, ip.to_canonical()) {
            (IpAddr::V4(net), IpAddr::V4(ip)) => {
                prefix_eq(&net.octets(), &ip.octets(), self.prefix)
            }
            (IpAddr::V6(net), IpAddr::V6(ip)) => {
                prefix_eq(&net.octets(), &ip.octets(), self.prefix)
            }
            _ => false,
        }
    }
}

fn prefix_eq(a: &[u8], b: &[u8], prefix: u8) -> bool {
    let (bytes, bits) = ((prefix / 8) as usize, prefix % 8);
    if a[..bytes] != b[..bytes] {
        return false;
    }
    bits == 0 || {
        let mask = 0xffu8 << (8 - bits);
        a[bytes] & mask == b[bytes] & mask
    }
}

/// Rate-limit key: the peer address, or the client address a trusted proxy reports.
#[derive(Clone, Debug)]
pub struct ClientIpKey {
    trusted: Arc<[IpNet]>,
}

impl ClientIpKey {
    pub fn new(trusted: Vec<IpNet>) -> Self {
        Self {
            trusted: trusted.into(),
        }
    }

    fn is_trusted(&self, ip: IpAddr) -> bool {
        self.trusted.iter().any(|n| n.contains(ip))
    }

    /// Walk `X-Forwarded-For` from the right (the entry our proxy added) past
    /// trusted proxies; the first other address is the client.
    fn client_ip(&self, peer: IpAddr, forwarded_for: Option<&str>) -> IpAddr {
        let mut client = peer.to_canonical();
        if !self.is_trusted(client) {
            return client;
        }
        let entries = forwarded_for.into_iter().flat_map(|h| h.rsplit(','));
        for entry in entries {
            let Ok(ip) = entry.trim().parse::<IpAddr>() else {
                break;
            };
            client = ip.to_canonical();
            if !self.is_trusted(client) {
                break;
            }
        }
        client
    }
}

/// IPv6 users typically get a whole /64; keying on single addresses would let
/// one client rotate through billions of them.
fn rate_limit_key(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => {
            let s = v6.segments();
            IpAddr::V6(Ipv6Addr::new(s[0], s[1], s[2], s[3], 0, 0, 0, 0))
        }
        v4 => v4,
    }
}

impl KeyExtractor for ClientIpKey {
    type Key = IpAddr;

    fn extract<T>(&self, req: &Request<T>) -> Result<IpAddr, GovernorError> {
        let peer = req
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ConnectInfo(addr)| addr.ip())
            .ok_or(GovernorError::UnableToExtractKey)?;
        let forwarded_for = req
            .headers()
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok());
        Ok(rate_limit_key(self.client_ip(peer, forwarded_for)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn networks() {
        let net = IpNet::parse("172.16.0.0/12").unwrap();
        assert!(net.contains(ip("172.16.0.1")));
        assert!(net.contains(ip("172.31.255.255")));
        assert!(!net.contains(ip("172.32.0.1")));
        assert!(net.contains(ip("::ffff:172.20.0.3")), "IPv4-mapped");
        let single = IpNet::parse("10.1.2.3").unwrap();
        assert!(single.contains(ip("10.1.2.3")));
        assert!(!single.contains(ip("10.1.2.4")));
        assert!(IpNet::parse("fd00::/8").unwrap().contains(ip("fd12::1")));
        assert!(IpNet::parse("0.0.0.0/0").unwrap().contains(ip("8.8.8.8")));
        assert!(IpNet::parse("10.0.0.0/33").is_err());
        assert!(IpNet::parse("nonsense").is_err());
    }

    #[test]
    fn forwarded_for_only_from_trusted_proxies() {
        let key = ClientIpKey::new(vec![IpNet::parse("172.16.0.0/12").unwrap()]);
        let proxy = ip("172.18.0.2");
        // Untrusted peers can't choose their key.
        assert_eq!(
            key.client_ip(ip("203.0.113.9"), Some("198.51.100.1")),
            ip("203.0.113.9")
        );
        assert_eq!(
            key.client_ip(proxy, Some("198.51.100.1")),
            ip("198.51.100.1")
        );
        // A spoofed entry on the left is ignored: the proxy appended the real one.
        assert_eq!(
            key.client_ip(proxy, Some("1.2.3.4, 198.51.100.1")),
            ip("198.51.100.1")
        );
        // Chained trusted proxies are skipped.
        assert_eq!(
            key.client_ip(proxy, Some("198.51.100.1, 172.18.0.5")),
            ip("198.51.100.1")
        );
        assert_eq!(key.client_ip(proxy, None), proxy);
        assert_eq!(key.client_ip(proxy, Some("garbage")), proxy);
    }

    #[test]
    fn ipv6_is_keyed_by_64() {
        assert_eq!(
            rate_limit_key(ip("2001:db8:1:2:aaaa::1")),
            rate_limit_key(ip("2001:db8:1:2:bbbb::9"))
        );
        assert_ne!(
            rate_limit_key(ip("2001:db8:1:2::1")),
            rate_limit_key(ip("2001:db8:1:3::1"))
        );
        assert_eq!(rate_limit_key(ip("192.0.2.1")), ip("192.0.2.1"));
    }
}
