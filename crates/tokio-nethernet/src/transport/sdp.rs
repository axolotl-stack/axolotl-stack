//! NetherNet session descriptions with go-nethernet `conn.go` parity.
//!
//! `encode` writes exactly the lines and order of `description.encode` (pion
//! `sdp.Marshal`); `parse` applies the requirements of `parseDescription`.

use std::fmt::Write as _;
use std::net::IpAddr;

use rtc_sdp::SessionDescription;

pub(crate) use super::certificate::FingerprintAlgorithm;
use crate::error::NetherNetError;
use crate::framing::MAX_SEGMENT_PAYLOAD;

/// Advertised SCTP message ceiling: `conn.go:maxMessageSize` + 1 header byte.
pub(crate) const MAX_MESSAGE_SIZE: u32 = MAX_SEGMENT_PAYLOAD as u32 + 1;

/// Remote ceiling when `a=max-message-size` is absent: RFC 8841 §6 "64K",
/// which libwebrtc (the real client's stack) applies. go-nethernet's
/// `parseDescription` rejects such descriptions instead.
const DEFAULT_REMOTE_MAX_MESSAGE_SIZE: u32 = 65_536;

/// DTLS connection role from `a=setup`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Setup {
    ActPass,
    Active,
    Passive,
}

impl Setup {
    fn as_str(self) -> &'static str {
        match self {
            Self::ActPass => "actpass",
            Self::Active => "active",
            Self::Passive => "passive",
        }
    }

    /// Answer role for a remote offer, `listener.go:listenerNegotiator.answererRole`.
    pub(crate) fn answer_to(remote: Setup) -> Setup {
        match remote {
            Setup::Passive | Setup::ActPass => Setup::Active,
            Setup::Active => Setup::Passive,
        }
    }

    /// Whether this side is the DTLS client given its own and the remote role.
    /// An `actpass` offerer becomes the server when the answer is `active`.
    pub(crate) fn is_dtls_client(local: Setup, remote: Setup) -> bool {
        match local {
            Setup::Active => true,
            Setup::Passive => false,
            Setup::ActPass => remote == Setup::Passive,
        }
    }
}

/// One local candidate rendered with `signal.go:formatICECandidate`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LocalCandidate {
    pub(crate) foundation: String,
    pub(crate) priority: u32,
    pub(crate) address: IpAddr,
    pub(crate) port: u16,
    pub(crate) typ: &'static str,
    pub(crate) related: Option<(IpAddr, u16)>,
}

impl LocalCandidate {
    /// `candidate:` text for SDP lines and `CANDIDATEADD` signals.
    pub(crate) fn format(&self, network_id: u32, ufrag: &str) -> String {
        let related_address = self.related.map(|(ip, _)| ip.to_string());
        crate::signaling::format_ice_candidate(
            network_id,
            &self.foundation,
            "udp",
            self.priority,
            &self.address.to_string(),
            self.port,
            self.typ,
            related_address.as_deref(),
            self.related.map(|(_, port)| port),
            ufrag,
        )
    }

    /// libwebrtc default-candidate preference (`conn.go:typePreference`).
    fn type_preference(&self) -> u8 {
        match self.typ {
            "host" => 1,
            "srflx" => 2,
            "relay" => 3,
            _ => 0,
        }
    }
}

pub(crate) struct LocalDescription<'a> {
    pub(crate) ufrag: &'a str,
    pub(crate) pwd: &'a str,
    pub(crate) fingerprint: &'a str,
    pub(crate) setup: Setup,
    /// Inline candidates (non-trickle signaling only).
    pub(crate) candidates: &'a [LocalCandidate],
}

/// `description.defaultCandidate`: highest preference per family; IPv4 is
/// never replaced by IPv6.
fn default_candidate(candidates: &[LocalCandidate]) -> Option<&LocalCandidate> {
    let mut selected: Option<&LocalCandidate> = None;
    for candidate in candidates {
        let replace = match selected {
            None => true,
            Some(current) => {
                let same_family = current.address.is_ipv4() == candidate.address.is_ipv4();
                if same_family {
                    candidate.type_preference() > current.type_preference()
                } else {
                    current.address.is_ipv6()
                }
            }
        };
        if replace {
            selected = Some(candidate);
        }
    }
    selected
}

pub(crate) fn encode(local: &LocalDescription<'_>) -> String {
    let mut sdp = String::with_capacity(512 + local.candidates.len() * 128);
    let session_id: u64 = rand::random::<u64>() >> 1;
    let (port, family, address) = match default_candidate(local.candidates) {
        Some(c) => (
            c.port,
            if c.address.is_ipv4() { "IP4" } else { "IP6" },
            c.address.to_string(),
        ),
        None => (9, "IP4", "0.0.0.0".to_owned()),
    };
    let _ = write!(
        sdp,
        "v=0\r\no=- {session_id} 2 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\n\
         a=group:BUNDLE 0\r\na=extmap-allow-mixed\r\na=msid-semantic: WMS\r\n\
         m=application {port} UDP/DTLS/SCTP webrtc-datachannel\r\nc=IN {family} {address}\r\n"
    );
    for (index, candidate) in local.candidates.iter().enumerate() {
        let _ = write!(sdp, "a={}\r\n", candidate.format(index as u32, local.ufrag));
    }
    let _ = write!(
        sdp,
        "a=ice-ufrag:{}\r\na=ice-pwd:{}\r\na=ice-options:trickle\r\n\
         a=fingerprint:sha-256 {}\r\na=setup:{}\r\na=mid:0\r\na=sctp-port:5000\r\n\
         a=max-message-size:{MAX_MESSAGE_SIZE}\r\n",
        local.ufrag,
        local.pwd,
        local.fingerprint,
        local.setup.as_str(),
    );
    sdp
}

/// Remote description fields NetherNet uses (`conn.go:parseDescription`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RemoteDescription {
    pub(crate) ufrag: String,
    pub(crate) pwd: String,
    pub(crate) fingerprint_algorithm: FingerprintAlgorithm,
    pub(crate) fingerprint: String,
    pub(crate) setup: Setup,
    pub(crate) max_message_size: u32,
    /// Raw candidate lines, each starting with `candidate:`.
    pub(crate) candidates: Vec<String>,
}

fn invalid(reason: &'static str) -> NetherNetError {
    NetherNetError::InvalidDescription(reason)
}

pub(crate) fn parse(sdp: &str) -> Result<RemoteDescription, NetherNetError> {
    let description = SessionDescription::unmarshal(&mut std::io::Cursor::new(sdp.as_bytes()))
        .map_err(|_| invalid("malformed SDP"))?;
    let [media] = description.media_descriptions.as_slice() else {
        return Err(invalid("exactly one media section required"));
    };
    let media_value = |key: &str| media.attribute(key).flatten();
    let ufrag = media_value("ice-ufrag").ok_or(invalid("missing ice-ufrag"))?;
    let pwd = media_value("ice-pwd").ok_or(invalid("missing ice-pwd"))?;
    let fingerprint = media_value("fingerprint")
        .or_else(|| description.attribute("fingerprint").map(String::as_str))
        .ok_or(invalid("missing fingerprint"))?;
    let mut parts = fingerprint.split(' ');
    let (Some(algorithm), Some(value), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err(invalid("malformed fingerprint"));
    };
    let algorithm = FingerprintAlgorithm::parse(algorithm)
        .ok_or(invalid("unsupported fingerprint algorithm"))?;
    let setup = match media_value("setup") {
        Some("active") => Setup::Active,
        Some("passive") => Setup::Passive,
        Some("actpass") => Setup::ActPass,
        Some(_) => return Err(invalid("unsupported setup role")),
        None => return Err(invalid("missing setup")),
    };
    // An absent attribute takes the RFC 8841 default like libwebrtc; a present
    // one must still be a decimal size above the one-byte segment header.
    let max_message_size = match media_value("max-message-size") {
        None => DEFAULT_REMOTE_MAX_MESSAGE_SIZE,
        Some(value) => Some(value)
            .filter(|value| !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|&size| size > 1)
            .ok_or(NetherNetError::InvalidMaxMessageSize)?,
    };
    let candidates = description
        .attributes
        .iter()
        .chain(&media.attributes)
        .filter(|attribute| attribute.key == "candidate")
        .filter_map(|attribute| attribute.value.as_deref())
        .map(|value| format!("candidate:{value}"))
        .collect();
    Ok(RemoteDescription {
        ufrag: ufrag.to_owned(),
        pwd: pwd.to_owned(),
        fingerprint_algorithm: algorithm,
        fingerprint: value.to_owned(),
        setup,
        max_message_size,
        candidates,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(address: &str, port: u16) -> LocalCandidate {
        LocalCandidate {
            foundation: "1".into(),
            priority: 2130706431,
            address: address.parse().unwrap(),
            port,
            typ: "host",
            related: None,
        }
    }

    #[test]
    fn encodes_go_nethernet_line_order_and_roundtrips() {
        let candidates = [
            host("192.168.0.2", 50000),
            LocalCandidate {
                typ: "srflx",
                related: Some(("192.168.0.2".parse().unwrap(), 50000)),
                ..host("203.0.113.9", 60000)
            },
        ];
        let sdp = encode(&LocalDescription {
            ufrag: "abcd",
            pwd: "0123456789abcdef0123",
            fingerprint: "AA:BB",
            setup: Setup::ActPass,
            candidates: &candidates,
        });
        let keys: Vec<&str> = sdp
            .lines()
            .map(|line| line.split([':', '=']).take(2).collect::<Vec<_>>()[1])
            .collect();
        assert_eq!(
            sdp.lines().map(|l| &l[..2]).collect::<String>(),
            "v=o=s=t=a=a=a=m=c=a=a=a=a=a=a=a=a=a=a="
        );
        assert_eq!(
            &keys[4..],
            [
                "group",
                "extmap-allow-mixed",
                "msid-semantic",
                "application 60000 UDP/DTLS/SCTP webrtc-datachannel",
                "IN IP4 203.0.113.9",
                "candidate",
                "candidate",
                "ice-ufrag",
                "ice-pwd",
                "ice-options",
                "fingerprint",
                "setup",
                "mid",
                "sctp-port",
                "max-message-size"
            ]
        );
        assert!(sdp.contains("a=msid-semantic: WMS\r\n"));
        assert!(sdp.contains("a=max-message-size:262144\r\n"));
        assert!(sdp.contains(
            "a=candidate:1 1 udp 2130706431 203.0.113.9 60000 typ srflx raddr 192.168.0.2 rport 50000 generation 0 ufrag abcd network-id 1 network-cost 0\r\n"
        ));
        let parsed = parse(&sdp).unwrap();
        assert_eq!(parsed.ufrag, "abcd");
        assert_eq!(parsed.pwd, "0123456789abcdef0123");
        assert_eq!(parsed.fingerprint_algorithm, FingerprintAlgorithm::Sha256);
        assert_eq!(parsed.fingerprint, "AA:BB");
        assert_eq!(parsed.setup, Setup::ActPass);
        assert_eq!(parsed.max_message_size, 262144);
        assert_eq!(parsed.candidates.len(), 2);
        assert!(
            parsed.candidates[0]
                .starts_with("candidate:1 1 udp 2130706431 192.168.0.2 50000 typ host")
        );
    }

    #[test]
    fn default_candidate_prefers_reachability_and_never_ipv6_over_ipv4() {
        let v6_relay = LocalCandidate {
            typ: "relay",
            ..host("2001:db8::1", 1)
        };
        let v4_host = host("192.168.0.2", 2);
        let v4_relay = LocalCandidate {
            typ: "relay",
            ..host("198.51.100.1", 3)
        };
        assert_eq!(
            default_candidate(&[v6_relay.clone(), v4_host.clone()])
                .unwrap()
                .port,
            2
        );
        assert_eq!(
            default_candidate(&[v4_host.clone(), v6_relay.clone()])
                .unwrap()
                .port,
            2
        );
        assert_eq!(
            default_candidate(&[v4_host, v4_relay, v6_relay])
                .unwrap()
                .port,
            3
        );
        let sdp = encode(&LocalDescription {
            ufrag: "abcd",
            pwd: "0123456789abcdef0123",
            fingerprint: "AA",
            setup: Setup::Active,
            candidates: &[],
        });
        assert!(
            sdp.contains(
                "m=application 9 UDP/DTLS/SCTP webrtc-datachannel\r\nc=IN IP4 0.0.0.0\r\n"
            )
        );
    }

    #[test]
    fn parse_enforces_parse_description_requirements() {
        let valid = encode(&LocalDescription {
            ufrag: "abcd",
            pwd: "0123456789abcdef0123",
            fingerprint: "AA:BB",
            setup: Setup::Passive,
            candidates: &[],
        });
        assert_eq!(parse(&valid).unwrap().setup, Setup::Passive);
        for (from, to) in [
            ("a=ice-ufrag:abcd\r\n", ""),
            ("a=ice-pwd:0123456789abcdef0123\r\n", ""),
            ("a=fingerprint:sha-256 AA:BB\r\n", ""),
            (
                "a=fingerprint:sha-256 AA:BB\r\n",
                "a=fingerprint:sha-256\r\n",
            ),
            ("a=fingerprint:sha-256", "a=fingerprint:sha-1"),
            ("a=setup:passive", "a=setup:holdconn"),
            ("a=setup:passive\r\n", ""),
            ("a=max-message-size:262144", "a=max-message-size:1"),
            ("a=max-message-size:262144", "a=max-message-size:+5"),
            ("a=max-message-size:262144", "a=max-message-size:"),
        ] {
            assert!(parse(&valid.replace(from, to)).is_err(), "{from} -> {to}");
        }
        let two_sections = format!("{valid}m=application 9 UDP/DTLS/SCTP webrtc-datachannel\r\n");
        assert!(parse(&two_sections).is_err());
        // Session-level fingerprint is the fallback, as in parseDescription.
        let session = valid
            .replace("a=fingerprint:sha-256 AA:BB\r\n", "")
            .replace(
                "a=msid-semantic: WMS\r\n",
                "a=msid-semantic: WMS\r\na=fingerprint:sha-256 CC:DD\r\n",
            );
        assert_eq!(parse(&session).unwrap().fingerprint, "CC:DD");
    }

    /// libwebrtc (the client) and RFC 8841 §6 read an absent
    /// `a=max-message-size` as 65536, so segments become 65535 bytes.
    #[test]
    fn missing_max_message_size_defaults_to_libwebrtc_65536() {
        let valid = encode(&LocalDescription {
            ufrag: "abcd",
            pwd: "0123456789abcdef0123",
            fingerprint: "AA:BB",
            setup: Setup::ActPass,
            candidates: &[],
        });
        let without = valid.replace("a=max-message-size:262144\r\n", "");
        assert_ne!(without, valid);
        assert_eq!(parse(&without).unwrap().max_message_size, 65_536);
    }

    #[test]
    fn dtls_roles_follow_answerer_role() {
        assert_eq!(Setup::answer_to(Setup::ActPass), Setup::Active);
        assert_eq!(Setup::answer_to(Setup::Passive), Setup::Active);
        assert_eq!(Setup::answer_to(Setup::Active), Setup::Passive);
        assert!(!Setup::is_dtls_client(Setup::ActPass, Setup::Active));
        assert!(Setup::is_dtls_client(Setup::ActPass, Setup::Passive));
        assert!(Setup::is_dtls_client(Setup::Active, Setup::ActPass));
        assert!(!Setup::is_dtls_client(Setup::Passive, Setup::Active));
    }
}
