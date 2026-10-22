use std::fmt;
use std::net::IpAddr;
use std::sync::Arc;

/// Predicate selecting local interface addresses used for host candidates.
pub type IpFilter = Arc<dyn Fn(IpAddr) -> bool + Send + Sync>;

/// Local ICE policy: which interface addresses gather host candidates.
///
/// Defaults follow pion as used by go-nethernet (`webrtc.NewAPI()`): every
/// non-loopback UDP4 and UDP6 interface address (`icegatherer.go`
/// `IncludeLoopbackCandidate` false, `networktype.go` UDP4/UDP6). One UDP
/// socket is bound per selected address so every datagram's local address is
/// exact for ICE.
#[derive(Clone)]
pub struct TransportConfig {
    /// Explicit local addresses to bind instead of enumerating interfaces
    /// (the equivalent of webrtc 0.21 `with_udp_addrs`); each must still pass
    /// the policy below. Enumeration costs milliseconds per connection on
    /// Windows (`GetAdaptersAddresses`).
    pub addresses: Option<Vec<IpAddr>>,
    /// Also gather loopback addresses (local testing policy, not a production default).
    pub include_loopback: bool,
    /// Optional additional address filter applied after the loopback/family policy.
    pub ip_filter: Option<IpFilter>,
    /// Gather IPv4 host candidates.
    pub udp4: bool,
    /// Gather IPv6 host candidates.
    pub udp6: bool,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            addresses: None,
            include_loopback: false,
            ip_filter: None,
            udp4: true,
            udp6: true,
        }
    }
}

impl fmt::Debug for TransportConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TransportConfig")
            .field("addresses", &self.addresses)
            .field("include_loopback", &self.include_loopback)
            .field("ip_filter", &self.ip_filter.as_ref().map(|_| ".."))
            .field("udp4", &self.udp4)
            .field("udp6", &self.udp6)
            .finish()
    }
}

impl TransportConfig {
    /// IPv4 loopback only: strict local tests confining the actual ICE/DTLS/SCTP sockets.
    pub fn loopback_v4() -> Self {
        Self {
            addresses: Some(vec![IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)]),
            include_loopback: true,
            ip_filter: None,
            udp4: true,
            udp6: false,
        }
    }

    pub(crate) fn allows(&self, ip: IpAddr) -> bool {
        let family = match ip {
            IpAddr::V4(_) => self.udp4,
            // Link-local IPv6 needs a scope id that cannot be signaled in candidates.
            IpAddr::V6(v6) => self.udp6 && (v6.segments()[0] & 0xffc0) != 0xfe80,
        };
        family
            && !ip.is_unspecified()
            && !ip.is_multicast()
            && (self.include_loopback || !ip.is_loopback())
            && self.ip_filter.as_ref().is_none_or(|filter| filter(ip))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_matches_pion_gathering() {
        let config = TransportConfig::default();
        assert!(config.allows("192.168.1.2".parse().unwrap()));
        assert!(config.allows("2001:db8::1".parse().unwrap()));
        assert!(!config.allows("127.0.0.1".parse().unwrap()));
        assert!(!config.allows("::1".parse().unwrap()));
        assert!(!config.allows("fe80::1".parse().unwrap()));
        assert!(!config.allows("0.0.0.0".parse().unwrap()));
        let local = TransportConfig::loopback_v4();
        assert_eq!(local.addresses, Some(vec!["127.0.0.1".parse().unwrap()]));
        assert!(local.allows("127.0.0.1".parse().unwrap()));
        assert!(!local.allows("::1".parse().unwrap()));
    }
}
