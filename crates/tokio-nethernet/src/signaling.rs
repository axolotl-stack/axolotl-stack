use async_trait::async_trait;
use std::fmt;
use thiserror::Error;

/// Signal types defined by the NetherNet protocol.
pub mod signal_type {
    pub const OFFER: &str = "CONNECTREQUEST";
    pub const ANSWER: &str = "CONNECTRESPONSE";
    pub const CANDIDATE: &str = "CANDIDATEADD";
    pub const ERROR: &str = "CONNECTERROR";
}

/// Error codes used in `SignalType::ERROR`.
/// These match the upstream Bedrock implementation exactly.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalErrorCode {
    None = 0,
    DestinationNotLoggedIn = 1,
    NegotiationTimeout = 2,
    WrongTransportVersion = 3,
    FailedToCreatePeerConnection = 4,
    Ice = 5,
    ConnectRequest = 6,
    ConnectResponse = 7,
    CandidateAdd = 8,
    InactivityTimeout = 9,
    FailedToCreateOffer = 10,
    FailedToCreateAnswer = 11,
    FailedToSetLocalDescription = 12,
    FailedToSetRemoteDescription = 13,
    NegotiationTimeoutWaitingForResponse = 14,
    NegotiationTimeoutWaitingForAccept = 15,
    IncomingConnectionIgnored = 16,
    SignalingParsingFailure = 17,
    SignalingUnknownError = 18,
    SignalingUnicastMessageDeliveryFailed = 19,
    SignalingBroadcastDeliveryFailed = 20,
    SignalingMessageDeliveryFailed = 21,
    SignalingTurnAuthFailed = 22,
    SignalingFallbackToBestEffortDelivery = 23,
    NoSignalingChannel = 24,
    NotLoggedIn = 25,
    SignalingFailedToSend = 26,
}

impl fmt::Display for SignalErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", *self as i32)
    }
}

/// Connection type for NetherNet signaling.
///
/// Indicates which transport/signaling method is used for the connection.
/// These values are used in discovery packets and session properties.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConnectionType {
    /// Legacy RakNet over UDP (version 1).
    RakNetV1 = 0,
    /// RakNet with encryption (version 2).
    RakNetV2 = 1,
    /// WebRTC via Xbox Live signaling (signal.franchise.minecraft-services.net).
    WebRTC = 3,
    /// LAN discovery signaling (UDP broadcast on port 7551).
    #[default]
    Lan = 4,
}

impl ConnectionType {
    /// Convert from raw u8 value.
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::RakNetV1),
            1 => Some(Self::RakNetV2),
            3 => Some(Self::WebRTC),
            4 => Some(Self::Lan),
            _ => None,
        }
    }
}

impl From<ConnectionType> for u8 {
    fn from(ct: ConnectionType) -> u8 {
        ct as u8
    }
}

/// ICE server credentials for TURN authentication.
/// Matches the upstream Bedrock JSON format.
#[derive(Debug, Clone, Default)]
pub struct Credentials {
    /// Expiration time in seconds.
    pub expiration_seconds: u32,
    /// List of ICE servers with authentication.
    pub ice_servers: Vec<IceServer>,
}

/// A single ICE server configuration.
#[derive(Debug, Clone)]
pub struct IceServer {
    pub username: String,
    pub password: String,
    pub urls: Vec<String>,
}

/// A signal sent or received to negotiate a connection in a NetherNet network.
///
/// Wire format: `[TYPE] [CONNECTION_ID] [DATA]`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signal {
    /// The type of signal (e.g. `CONNECTREQUEST`).
    pub typ: String,
    /// Unique ID of the connection sending the signal.
    pub connection_id: u64,
    /// The payload data (SDP, Candidate, Error Code, etc.).
    pub data: String,
    /// Internal ID used by signaling to reference a remote network.
    /// Not part of the wire format string.
    pub network_id: String,
}

impl fmt::Display for Signal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} {}", self.typ, self.connection_id, self.data)
    }
}

impl Signal {
    /// Parses a signal string into a `Signal` struct.
    ///
    /// Expected format: `TYPE CONNECTION_ID DATA`
    pub fn parse(s: &str, network_id: String) -> Result<Self, SignalParseError> {
        let parts: Vec<&str> = s.splitn(3, ' ').collect();
        if parts.len() != 3 {
            return Err(SignalParseError::InvalidFormat);
        }

        let typ = parts[0].to_string();
        let connection_id = parts[1]
            .parse::<u64>()
            .map_err(|_| SignalParseError::InvalidConnectionId)?;
        let data = parts[2].to_string();

        Ok(Self {
            typ,
            connection_id,
            data,
            network_id,
        })
    }

    /// Creates an error signal to send back to the remote.
    #[inline]
    pub fn error(connection_id: u64, network_id: String, code: SignalErrorCode) -> Self {
        Self {
            typ: signal_type::ERROR.to_string(),
            connection_id,
            data: code.to_string(),
            network_id,
        }
    }
}

#[derive(Debug, Error)]
pub enum SignalParseError {
    #[error("invalid signal format")]
    InvalidFormat,
    #[error("invalid connection id")]
    InvalidConnectionId,
    #[error("invalid IP address")]
    InvalidIpAddress,
    #[error("invalid port number")]
    InvalidPort,
    #[error("field exceeds maximum length")]
    FieldTooLong,
    #[error("invalid protocol")]
    InvalidProtocol,
}

/// Maximum allowed lengths for ICE candidate fields (security limits).
const MAX_FOUNDATION_LEN: usize = 32;
const MAX_UFRAG_LEN: usize = 256;
const MAX_CANDIDATE_LEN: usize = 1024;

/// Formats an ICE candidate in the C++ WebRTC format expected by Bedrock.
///
/// Format: `candidate:<foundation> 1 <proto> <priority> <addr> <port> typ <type> [raddr <raddr> rport <rport>] generation 0 ufrag <ufrag> network-id <id> network-cost 0`
///
/// This matches the format used by `go-nethernet` and upstream Bedrock clients.
#[inline]
#[allow(clippy::too_many_arguments)]
pub fn format_ice_candidate(
    id: u32,
    foundation: &str,
    protocol: &str,
    priority: u32,
    address: &str,
    port: u16,
    candidate_type: &str,
    related_address: Option<&str>,
    related_port: Option<u16>,
    ufrag: &str,
) -> String {
    let mut s = format!(
        "candidate:{} 1 {} {} {} {} typ {}",
        foundation, protocol, priority, address, port, candidate_type
    );

    // Add related address/port for relay and srflx candidates
    if let (Some(raddr), Some(rport)) = (related_address, related_port) {
        s.push_str(&format!(" raddr {} rport {}", raddr, rport));
    }

    s.push_str(&format!(
        " generation 0 ufrag {} network-id {} network-cost 0",
        ufrag, id
    ));

    s
}

/// Parses a C++ WebRTC format ICE candidate string with validation.
///
/// Returns (foundation, protocol, priority, address, port, type, related_addr, related_port, ufrag).
///
/// # Security
/// - Validates IP addresses (IPv4 or IPv6 format)
/// - Validates port range (1-65535)
/// - Limits string field lengths to prevent memory exhaustion
/// - Validates protocol is "udp" or "tcp"
pub fn parse_ice_candidate(s: &str) -> Result<IceCandidateInfo, SignalParseError> {
    // Check total length limit
    if s.len() > MAX_CANDIDATE_LEN {
        return Err(SignalParseError::FieldTooLong);
    }

    // Format: candidate:<foundation> 1 <proto> <priority> <addr> <port> typ <type> ...
    let s = s.strip_prefix("candidate:").unwrap_or(s);
    let parts: Vec<&str> = s.split_whitespace().collect();

    if parts.len() < 8 {
        return Err(SignalParseError::InvalidFormat);
    }

    // Validate and parse foundation
    let foundation = parts[0];
    if foundation.len() > MAX_FOUNDATION_LEN {
        return Err(SignalParseError::FieldTooLong);
    }
    let foundation = foundation.to_string();

    // parts[1] is component (always 1)

    // Validate protocol (must be udp or tcp)
    let protocol = parts[2].to_lowercase();
    if protocol != "udp" && protocol != "tcp" {
        return Err(SignalParseError::InvalidProtocol);
    }

    let priority = parts[3]
        .parse()
        .map_err(|_| SignalParseError::InvalidFormat)?;

    // Validate IP address format
    let address = parts[4];
    if address.parse::<std::net::IpAddr>().is_err() {
        return Err(SignalParseError::InvalidIpAddress);
    }
    let address = address.to_string();

    // Validate port range (1-65535)
    let port: u16 = parts[5]
        .parse()
        .map_err(|_| SignalParseError::InvalidPort)?;
    if port == 0 {
        return Err(SignalParseError::InvalidPort);
    }

    // parts[6] is "typ"
    let candidate_type = parts[7].to_string();

    // Parse optional fields
    let mut related_address = None;
    let mut related_port = None;
    let mut ufrag = None;

    let mut i = 8;
    while i < parts.len() {
        match parts[i] {
            "raddr" if i + 1 < parts.len() => {
                let addr = parts[i + 1];
                // Validate related address if present
                if addr.parse::<std::net::IpAddr>().is_ok() {
                    related_address = Some(addr.to_string());
                }
                i += 2;
            }
            "rport" if i + 1 < parts.len() => {
                related_port = parts[i + 1].parse().ok();
                i += 2;
            }
            "ufrag" if i + 1 < parts.len() => {
                let u = parts[i + 1];
                // Validate ufrag length
                if u.len() <= MAX_UFRAG_LEN {
                    ufrag = Some(u.to_string());
                }
                i += 2;
            }
            _ => i += 1,
        }
    }

    Ok(IceCandidateInfo {
        foundation,
        protocol,
        priority,
        address,
        port,
        candidate_type,
        related_address,
        related_port,
        ufrag,
    })
}

/// Parsed ICE candidate information.
#[derive(Debug, Clone)]
pub struct IceCandidateInfo {
    pub foundation: String,
    pub protocol: String,
    pub priority: u32,
    pub address: String,
    pub port: u16,
    pub candidate_type: String,
    pub related_address: Option<String>,
    pub related_port: Option<u16>,
    pub ufrag: Option<String>,
}

/// Interface for sending and receiving Signals over a network.
///
/// Implementations should be async-first and avoid blocking.
#[async_trait]
pub trait Signaling: Send + Sync {
    /// Sends a signal to the remote network referenced by `signal.network_id`.
    async fn signal(&self, signal: Signal) -> anyhow::Result<()>;

    /// Returns TURN server credentials if available.
    /// Returns `None` if no credentials are needed (e.g., STUN-only).
    async fn credentials(&self) -> Option<Credentials> {
        None
    }

    /// Sets pong data for RakNet-style ping responses.
    /// This is used by the listener to respond to discovery pings.
    fn set_pong_data(&self, _data: &[u8]) {}

    /// Returns the local network ID.
    fn network_id(&self) -> String;
}

// Blanket implementation for Arc<T> where T: Signaling
#[async_trait]
impl<T: Signaling + ?Sized> Signaling for std::sync::Arc<T> {
    async fn signal(&self, signal: Signal) -> anyhow::Result<()> {
        (**self).signal(signal).await
    }

    async fn credentials(&self) -> Option<Credentials> {
        (**self).credentials().await
    }

    fn set_pong_data(&self, data: &[u8]) {
        (**self).set_pong_data(data)
    }

    fn network_id(&self) -> String {
        (**self).network_id()
    }
}

/// Extended signaling interface that provides a signal receiver.
///
/// This trait is used by `NetherNetListener::bind_with_signaling` and
/// `NetherNetDialer::connect_with_signaling` to internalize the signal pump.
///
/// Types implementing this trait (like `DiscoveryListener`) can be passed
/// directly to the `bind_with_signaling` constructor without manual wiring.
#[async_trait]
pub trait SignalingChannel: Signaling {
    /// Takes the signal receiver from this channel.
    ///
    /// This should only be called once - subsequent calls return `None`.
    async fn take_signal_receiver(&self) -> Option<tokio::sync::mpsc::Receiver<Signal>>;
}

// Blanket implementation for Arc<T> where T: SignalingChannel
#[async_trait]
impl<T: SignalingChannel + ?Sized> SignalingChannel for std::sync::Arc<T> {
    async fn take_signal_receiver(&self) -> Option<tokio::sync::mpsc::Receiver<Signal>> {
        (**self).take_signal_receiver().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ice_candidate_valid_host() {
        let candidate = "candidate:1 1 udp 2130706431 192.168.1.100 54321 typ host generation 0 ufrag abc123 network-id 1 network-cost 0";
        let result = parse_ice_candidate(candidate).unwrap();

        assert_eq!(result.foundation, "1");
        assert_eq!(result.protocol, "udp");
        assert_eq!(result.priority, 2130706431);
        assert_eq!(result.address, "192.168.1.100");
        assert_eq!(result.port, 54321);
        assert_eq!(result.candidate_type, "host");
        assert_eq!(result.ufrag, Some("abc123".to_string()));
    }

    #[test]
    fn test_parse_ice_candidate_valid_relay() {
        let candidate = "candidate:2 1 udp 1677721855 203.0.113.50 19302 typ relay raddr 192.168.1.100 rport 54321 generation 0 ufrag xyz";
        let result = parse_ice_candidate(candidate).unwrap();

        assert_eq!(result.candidate_type, "relay");
        assert_eq!(result.address, "203.0.113.50");
        assert_eq!(result.port, 19302);
        assert_eq!(result.related_address, Some("192.168.1.100".to_string()));
        assert_eq!(result.related_port, Some(54321));
        assert_eq!(result.ufrag, Some("xyz".to_string()));
    }

    #[test]
    fn test_parse_ice_candidate_ipv6() {
        let candidate = "candidate:1 1 udp 2130706431 2001:db8::1 54321 typ host";
        let result = parse_ice_candidate(candidate).unwrap();

        assert_eq!(result.address, "2001:db8::1");
    }

    #[test]
    fn test_parse_ice_candidate_invalid_ip() {
        let candidate = "candidate:1 1 udp 2130706431 999.999.999.999 54321 typ host";
        let result = parse_ice_candidate(candidate);

        assert!(matches!(result, Err(SignalParseError::InvalidIpAddress)));
    }

    #[test]
    fn test_parse_ice_candidate_invalid_port_zero() {
        let candidate = "candidate:1 1 udp 2130706431 192.168.1.100 0 typ host";
        let result = parse_ice_candidate(candidate);

        assert!(matches!(result, Err(SignalParseError::InvalidPort)));
    }

    #[test]
    fn test_parse_ice_candidate_invalid_port_overflow() {
        let candidate = "candidate:1 1 udp 2130706431 192.168.1.100 70000 typ host";
        let result = parse_ice_candidate(candidate);

        assert!(matches!(result, Err(SignalParseError::InvalidPort)));
    }

    #[test]
    fn test_parse_ice_candidate_invalid_protocol() {
        let candidate = "candidate:1 1 sctp 2130706431 192.168.1.100 54321 typ host";
        let result = parse_ice_candidate(candidate);

        assert!(matches!(result, Err(SignalParseError::InvalidProtocol)));
    }

    #[test]
    fn test_parse_ice_candidate_too_long() {
        let long_candidate = format!(
            "candidate:1 1 udp 2130706431 192.168.1.100 54321 typ host {}",
            "x".repeat(2000)
        );
        let result = parse_ice_candidate(&long_candidate);

        assert!(matches!(result, Err(SignalParseError::FieldTooLong)));
    }

    #[test]
    fn test_parse_ice_candidate_foundation_too_long() {
        let long_foundation = "a".repeat(64);
        let candidate = format!(
            "candidate:{} 1 udp 2130706431 192.168.1.100 54321 typ host",
            long_foundation
        );
        let result = parse_ice_candidate(&candidate);

        assert!(matches!(result, Err(SignalParseError::FieldTooLong)));
    }

    #[test]
    fn test_parse_ice_candidate_invalid_format() {
        let candidate = "candidate:1 1 udp";
        let result = parse_ice_candidate(candidate);

        assert!(matches!(result, Err(SignalParseError::InvalidFormat)));
    }

    #[test]
    fn test_signal_parse_valid() {
        let signal = Signal::parse(
            "CONNECTREQUEST 12345 some sdp data here",
            "net123".to_string(),
        )
        .unwrap();

        assert_eq!(signal.typ, "CONNECTREQUEST");
        assert_eq!(signal.connection_id, 12345);
        assert_eq!(signal.data, "some sdp data here");
        assert_eq!(signal.network_id, "net123");
    }

    #[test]
    fn test_signal_parse_invalid_connection_id() {
        let result = Signal::parse("CONNECTREQUEST notanumber some data", "net123".to_string());

        assert!(matches!(result, Err(SignalParseError::InvalidConnectionId)));
    }

    #[test]
    fn test_signal_error_code_display() {
        assert_eq!(SignalErrorCode::None.to_string(), "0");
        assert_eq!(SignalErrorCode::DestinationNotLoggedIn.to_string(), "1");
        assert_eq!(SignalErrorCode::NegotiationTimeout.to_string(), "2");
    }

    #[test]
    fn test_connection_type_from_u8() {
        assert_eq!(ConnectionType::from_u8(0), Some(ConnectionType::RakNetV1));
        assert_eq!(ConnectionType::from_u8(1), Some(ConnectionType::RakNetV2));
        assert_eq!(ConnectionType::from_u8(3), Some(ConnectionType::WebRTC));
        assert_eq!(ConnectionType::from_u8(4), Some(ConnectionType::Lan));
        assert_eq!(ConnectionType::from_u8(2), None);
        assert_eq!(ConnectionType::from_u8(5), None);
    }

    #[test]
    fn test_format_ice_candidate() {
        let formatted = format_ice_candidate(
            1,
            "abc123",
            "udp",
            2130706431,
            "192.168.1.100",
            54321,
            "host",
            None,
            None,
            "ufrag123",
        );

        assert!(
            formatted.starts_with("candidate:abc123 1 udp 2130706431 192.168.1.100 54321 typ host")
        );
        assert!(formatted.contains("ufrag ufrag123"));
        assert!(formatted.contains("network-id 1"));
    }

    #[test]
    fn test_format_ice_candidate_with_relay() {
        let formatted = format_ice_candidate(
            1,
            "abc123",
            "udp",
            1677721855,
            "203.0.113.50",
            19302,
            "relay",
            Some("192.168.1.100"),
            Some(54321),
            "ufrag123",
        );

        assert!(formatted.contains("raddr 192.168.1.100 rport 54321"));
    }
}
