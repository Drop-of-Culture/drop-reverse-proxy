//! Client IP used for IP bans. Behind a reverse proxy (Traefik) every connection comes
//! from the proxy, so the client IP is read from `X-Forwarded-For`, but only when the
//! connection comes from a trusted proxy (`trusted_proxies` in app.toml).

use crate::AppState;
use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::HeaderMap;
use axum::http::request::Parts;
use serde::{Deserialize, Deserializer};
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;

/// An IP network in CIDR notation (`172.16.0.0/12`), or a single IP.
#[derive(Clone, Debug, PartialEq)]
pub struct IpNet {
    addr: IpAddr,
    prefix_len: u8,
}

impl IpNet {
    pub fn contains(&self, ip: &IpAddr) -> bool {
        match (self.addr, ip.to_canonical()) {
            (IpAddr::V4(net), IpAddr::V4(ip)) => {
                let mask = u32::MAX.checked_shl(32 - u32::from(self.prefix_len)).unwrap_or(0);
                u32::from(net) & mask == u32::from(ip) & mask
            }
            (IpAddr::V6(net), IpAddr::V6(ip)) => {
                let mask = u128::MAX.checked_shl(128 - u32::from(self.prefix_len)).unwrap_or(0);
                u128::from(net) & mask == u128::from(ip) & mask
            }
            _ => false,
        }
    }
}

impl FromStr for IpNet {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (addr_str, prefix_str) = match s.split_once('/') {
            Some((addr, prefix)) => (addr, Some(prefix)),
            None => (s, None),
        };
        let addr = IpAddr::from_str(addr_str.trim()).map_err(|e| format!("invalid IP in '{s}': {e}"))?;
        let max_len = if addr.is_ipv4() { 32 } else { 128 };
        let prefix_len = match prefix_str {
            Some(prefix) => prefix.trim().parse::<u8>()
                .ok()
                .filter(|len| *len <= max_len)
                .ok_or_else(|| format!("invalid prefix length in '{s}'"))?,
            None => max_len,
        };
        Ok(IpNet { addr: addr.to_canonical(), prefix_len })
    }
}

impl<'de> Deserialize<'de> for IpNet {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        IpNet::from_str(&s).map_err(serde::de::Error::custom)
    }
}

/// Client IP of the request: the peer address, or, when the peer is a trusted proxy,
/// the rightmost `X-Forwarded-For` entry that isn't a trusted proxy itself.
/// Entries left of it are set by the client and can't be trusted.
pub fn resolve_client_ip(peer: IpAddr, headers: &HeaderMap, trusted_proxies: &[IpNet]) -> IpAddr {
    let is_trusted = |ip: &IpAddr| trusted_proxies.iter().any(|net| net.contains(ip));
    if !is_trusted(&peer) {
        return peer;
    }
    let mut client_ip = peer;
    // several X-Forwarded-For headers are one list, in order
    let forwarded = headers.get_all("x-forwarded-for")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .collect::<Vec<_>>();
    for entry in forwarded.iter().rev() {
        match IpAddr::from_str(entry.trim()) {
            Ok(ip) => {
                client_ip = ip.to_canonical();
                if !is_trusted(&client_ip) {
                    break;
                }
            }
            // garbage: keep the last valid hop rather than trusting anything further left
            Err(_) => break,
        }
    }
    client_ip
}

/// Extractor giving the client IP, see [`resolve_client_ip`].
pub struct ClientIp(pub IpAddr);

impl FromRequestParts<AppState> for ClientIp {
    type Rejection = <ConnectInfo<SocketAddr> as FromRequestParts<AppState>>::Rejection;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let ConnectInfo(peer) = ConnectInfo::<SocketAddr>::from_request_parts(parts, state).await?;
        Ok(ClientIp(resolve_client_ip(peer.ip(), &parts.headers, state.conf.trusted_proxies())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn nets(list: &[&str]) -> Vec<IpNet> {
        list.iter().map(|s| s.parse().unwrap()).collect()
    }

    fn headers(xff: &[&str]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for v in xff {
            headers.append("x-forwarded-for", HeaderValue::from_str(v).unwrap());
        }
        headers
    }

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn ip_net_contains() {
        let net: IpNet = "172.16.0.0/12".parse().unwrap();
        assert!(net.contains(&ip("172.18.0.5")));
        assert!(!net.contains(&ip("172.32.0.1")));
        assert!(net.contains(&ip("::ffff:172.18.0.5")));
        assert!(!net.contains(&ip("fd00::1")));
        assert!("0.0.0.0/0".parse::<IpNet>().unwrap().contains(&ip("8.8.8.8")));
        assert!("10.0.0.1".parse::<IpNet>().unwrap().contains(&ip("10.0.0.1")));
        assert!(!"10.0.0.1".parse::<IpNet>().unwrap().contains(&ip("10.0.0.2")));
        assert!("fd00::/8".parse::<IpNet>().unwrap().contains(&ip("fd12::1")));
    }

    #[test]
    fn ip_net_parse_errors() {
        assert!("10.0.0.0/33".parse::<IpNet>().is_err());
        assert!("nope/8".parse::<IpNet>().is_err());
        assert!("10.0.0.0/x".parse::<IpNet>().is_err());
    }

    #[test]
    fn untrusted_peer_ignores_header() {
        let ip = resolve_client_ip(ip("203.0.113.9"), &headers(&["1.2.3.4"]), &nets(&["172.16.0.0/12"]));
        assert_eq!(ip, "203.0.113.9".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn no_trusted_proxies_ignores_header() {
        let ip = resolve_client_ip(ip("172.18.0.2"), &headers(&["1.2.3.4"]), &[]);
        assert_eq!(ip, "172.18.0.2".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn trusted_peer_uses_rightmost_untrusted_entry() {
        // the client sent a fake X-Forwarded-For, Traefik appended the real IP
        let ip = resolve_client_ip(ip("172.18.0.2"), &headers(&["6.6.6.6, 203.0.113.9"]), &nets(&["172.16.0.0/12"]));
        assert_eq!(ip, "203.0.113.9".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn skips_trusted_hops_across_headers() {
        let ip = resolve_client_ip(ip("172.18.0.2"), &headers(&["203.0.113.9", "10.0.0.7"]), &nets(&["172.16.0.0/12", "10.0.0.0/8"]));
        assert_eq!(ip, "203.0.113.9".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn trusted_peer_without_header_is_the_client() {
        let ip = resolve_client_ip(ip("172.18.0.2"), &HeaderMap::new(), &nets(&["172.16.0.0/12"]));
        assert_eq!(ip, "172.18.0.2".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn garbage_entry_stops_at_last_valid_hop() {
        let ip = resolve_client_ip(ip("172.18.0.2"), &headers(&["6.6.6.6, garbage"]), &nets(&["172.16.0.0/12"]));
        assert_eq!(ip, "172.18.0.2".parse::<IpAddr>().unwrap());
    }
}
