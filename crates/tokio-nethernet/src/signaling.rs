use async_trait::async_trait;
use std::fmt;
use thiserror::Error;

/// Signal kinds and their wire names.
///
/// Source: go-nethernet signal.go:SignalTypeOffer constant block / Signal.validate.
/// Only these four names are valid; `Signal::parse` rejects every other name,
/// so an unknown type is unrepresentable rather than validated later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SignalType {
    /// `CONNECTREQUEST`: SDP offer from the dialer.
    Offer,
    /// `CONNECTRESPONSE`: SDP answer from the listener.
    Answer,
    /// `CANDIDATEADD`: trickled ICE candidate.
    Candidate,
    /// `CONNECTERROR`: signed 32-bit decimal error code.
    Error,
}

impl SignalType {
    /// Exact (case-sensitive) wire name.
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Offer => "CONNECTREQUEST",
            Self::Answer => "CONNECTRESPONSE",
            Self::Candidate => "CANDIDATEADD",
            Self::Error => "CONNECTERROR",
        }
    }

    /// Parses an exact wire name; go-nethernet compares names case-sensitively.
    #[inline]
    pub fn from_wire(name: &[u8]) -> Option<Self> {
        match name {
            b"CONNECTREQUEST" => Some(Self::Offer),
            b"CONNECTRESPONSE" => Some(Self::Answer),
            b"CANDIDATEADD" => Some(Self::Candidate),
            b"CONNECTERROR" => Some(Self::Error),
            _ => None,
        }
    }
}

impl fmt::Display for SignalType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for SignalType {
    type Err = SignalParseError;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        Self::from_wire(name.as_bytes()).ok_or(SignalParseError::InvalidSignalType)
    }
}

/// Error codes used in [`SignalType::Error`] signals.
/// Source: go-nethernet signal.go:ErrorCodeNone constant block / Signal.validate.
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
    RelayServerConfigurationResultFailure = 27,
    RelayServerConfigurationResultParsingErrorNoURLs = 28,
    RelayServerConfigurationResultParsingErrorNoCreds = 29,
    RelayServerConfigurationResultParsingErrorNoServers = 30,
    RelayServerConfigurationResultParsingErrorNoExpiration = 31,
    DataChannelClosed = 32,
    InternalErrorJSONSerialization = 33,
    InvalidArgument = 34,
    GenericFailure = 35,
    FailedToCreateIdentityAssertion = 36,
    IdentityNotAllowed = 37,
}

impl fmt::Display for SignalErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(itoa::Buffer::new().format(*self as i32))
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
    /// The type of signal (e.g. [`SignalType::Offer`] for `CONNECTREQUEST`).
    pub typ: SignalType,
    /// Unique ID of the connection sending the signal.
    pub connection_id: u64,
    /// The payload data (SDP, Candidate, Error Code, etc.).
    pub data: String,
    /// Internal ID used by signaling to reference a remote network.
    /// Not part of the wire format string.
    pub network_id: String,
}

/// Signals up to this wire length are assembled on the stack and handed to the
/// formatter in one `write_str`, so `to_string()` allocates exactly once.
const INLINE_DISPLAY_LEN: usize = 256;

/// Longest `TYPE CONNECTION_ID ` head: 16-byte prefix slot + 20 digits + space.
const HEAD_CAPACITY: usize = 40;

/// ASCII `TYPE CONNECTION_ID ` prefix of the wire text, built with fixed-size
/// stores (no per-piece `memcpy` calls) so formatting costs a few nanoseconds.
struct WireHead {
    bytes: [u8; HEAD_CAPACITY],
    len: usize,
}

impl WireHead {
    #[inline]
    fn new(typ: SignalType, connection_id: u64) -> Self {
        // Each wire name plus its separating space fits a 16-byte slot.
        let (slot, prefix_len): (&[u8; 16], usize) = match typ {
            SignalType::Offer => (b"CONNECTREQUEST  ", 15),
            SignalType::Answer => (b"CONNECTRESPONSE ", 16),
            SignalType::Candidate => (b"CANDIDATEADD    ", 13),
            SignalType::Error => (b"CONNECTERROR    ", 13),
        };
        let mut bytes = [0u8; HEAD_CAPACITY];
        bytes[..16].copy_from_slice(slot);
        let digits = connection_id
            .checked_ilog10()
            .map_or(1, |log| log as usize + 1);
        // Two digits per step from the right, as itoa does.
        let mut value = connection_id;
        let mut at = prefix_len + digits;
        while value >= 10 {
            let pair = (value % 100) as usize * 2;
            value /= 100;
            at -= 2;
            bytes[at..at + 2].copy_from_slice(&DIGIT_PAIRS[pair..pair + 2]);
        }
        if at > prefix_len {
            bytes[at - 1] = b'0' + value as u8;
        }
        bytes[prefix_len + digits] = b' ';
        Self {
            bytes,
            len: prefix_len + digits + 1,
        }
    }

    #[inline]
    fn as_str(&self) -> &str {
        // SAFETY: `bytes[..len]` holds only ASCII: a wire name, decimal digits
        // and spaces, all written by `new`.
        unsafe { std::str::from_utf8_unchecked(&self.bytes[..self.len]) }
    }
}

/// `"00" ..= "99"`, two ASCII digits per index pair.
const DIGIT_PAIRS: [u8; 200] = {
    let mut table = [0u8; 200];
    let mut n = 0;
    while n < 100 {
        table[n * 2] = b'0' + (n / 10) as u8;
        table[n * 2 + 1] = b'0' + (n % 10) as u8;
        n += 1;
    }
    table
};

impl fmt::Display for Signal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let head = WireHead::new(self.typ, self.connection_id);
        if let Some(result) = with_inline_wire(&head, &self.data, |text| f.write_str(text)) {
            return result;
        }
        f.write_str(head.as_str())?;
        f.write_str(&self.data)
    }
}

/// Runs `use_text` on the complete wire text assembled in a stack buffer, or
/// returns `None` when it exceeds [`INLINE_DISPLAY_LEN`].
#[inline]
fn with_inline_wire<R>(head: &WireHead, data: &str, use_text: impl FnOnce(&str) -> R) -> Option<R> {
    let len = head.len + data.len();
    if len > INLINE_DISPLAY_LEN {
        return None;
    }
    let mut buf = [0u8; INLINE_DISPLAY_LEN + HEAD_CAPACITY];
    buf[..HEAD_CAPACITY].copy_from_slice(&head.bytes);
    buf[head.len..len].copy_from_slice(data.as_bytes());
    // SAFETY: `buf[..len]` is the ASCII head followed by the complete `data`
    // string, so it is valid UTF-8 ending on a char boundary.
    Some(use_text(unsafe {
        std::str::from_utf8_unchecked(&buf[..len])
    }))
}

impl Signal {
    /// Parses a signal string into a `Signal` struct.
    ///
    /// Expected format: `TYPE CONNECTION_ID DATA`, split on the first two
    /// spaces as go-nethernet signal.go:Signal.UnmarshalText does. Error
    /// precedence matches upstream: format, connection ID, type, error code.
    /// The only allocation is the owned copy of `DATA`.
    pub fn parse(s: &str, network_id: String) -> Result<Self, SignalParseError> {
        let (typ, connection_id, data_start) = split_wire(s)?;
        Ok(Self {
            typ,
            connection_id,
            data: s[data_start..].to_owned(),
            network_id,
        })
    }

    /// [`Signal::parse`] that takes ownership of the wire text and reuses its
    /// buffer for `data` (the `TYPE CONNECTION_ID ` head is shifted out in
    /// place), so a received text becomes a signal without allocating.
    pub fn parse_owned(mut text: String, network_id: String) -> Result<Self, SignalParseError> {
        let (typ, connection_id, data_start) = split_wire(&text)?;
        text.drain(..data_start);
        Ok(Self {
            typ,
            connection_id,
            data: text,
            network_id,
        })
    }

    /// Validates constructed signals too, as go-nethernet signal.go:Signal.validate does.
    pub fn validate(&self) -> Result<(), SignalParseError> {
        match self.typ {
            SignalType::Offer | SignalType::Answer | SignalType::Candidate => Ok(()),
            SignalType::Error => parse_signal_error_code(&self.data).map(|_| ()),
        }
    }

    /// Creates an error signal to send back to the remote.
    #[inline]
    pub fn error(connection_id: u64, network_id: String, code: SignalErrorCode) -> Self {
        Self {
            typ: SignalType::Error,
            connection_id,
            data: itoa::Buffer::new().format(code as i32).to_owned(),
            network_id,
        }
    }

    /// Appends the wire text (`TYPE CONNECTION_ID DATA`) to `out` with one exact
    /// reservation; a reused `out` makes formatting allocation-free.
    pub fn write_wire(&self, out: &mut String) {
        let head = WireHead::new(self.typ, self.connection_id);
        out.reserve(head.len + self.data.len());
        out.push_str(head.as_str());
        out.push_str(&self.data);
    }

    /// Returns the wire text in a fresh `String` allocated once at its exact
    /// length. Same text as `to_string()`, which cannot pre-size through
    /// `fmt` and so grows its buffer, notably for SDP-sized payloads.
    pub fn to_wire_string(&self) -> String {
        let head = WireHead::new(self.typ, self.connection_id);
        with_inline_wire(&head, &self.data, str::to_owned).unwrap_or_else(|| {
            let mut out = String::with_capacity(head.len + self.data.len());
            out.push_str(head.as_str());
            out.push_str(&self.data);
            out
        })
    }
}

/// Validates `TYPE CONNECTION_ID DATA` and returns the type, ID and the byte
/// offset of `DATA` (go-nethernet signal.go:Signal.UnmarshalText).
#[inline]
fn split_wire(s: &str) -> Result<(SignalType, u64, usize), SignalParseError> {
    let bytes = s.as_bytes();
    let first = find_space(bytes, 0).ok_or(SignalParseError::InvalidFormat)?;
    let second = find_space(bytes, first + 1).ok_or(SignalParseError::InvalidFormat)?;
    let connection_id = decimal_prefix_u64(&bytes[first + 1..second])
        .ok_or(SignalParseError::InvalidConnectionId)?;
    let typ = SignalType::from_wire(&bytes[..first]).ok_or(SignalParseError::InvalidSignalType)?;
    // `second` indexes an ASCII space, so `second + 1` is a char boundary.
    let data_start = second + 1;
    if typ == SignalType::Error {
        parse_signal_error_code(&s[data_start..])?;
    }
    Ok((typ, connection_id, data_start))
}

#[inline]
fn find_space(bytes: &[u8], from: usize) -> Option<usize> {
    bytes[from..]
        .iter()
        .position(|&b| b == b' ')
        .map(|offset| from + offset)
}

/// Value of the leading ASCII digits; `None` when there are none or they
/// overflow, as go-nethernet signal.go:decimalPrefix + strconv.ParseUint do.
#[inline]
fn decimal_prefix_u64(bytes: &[u8]) -> Option<u64> {
    let mut value = 0u64;
    let mut digits = 0;
    for &byte in bytes {
        let digit = byte.wrapping_sub(b'0');
        if digit > 9 {
            break;
        }
        value = value.checked_mul(10)?.checked_add(u64::from(digit))?;
        digits += 1;
    }
    (digits > 0).then_some(value)
}

/// Parses signed decimal prefixes, matching go-nethernet signal.go:parseSignalErrorCode
/// (an optional `-`, at least one digit, trailing text ignored, `i32` range).
pub fn parse_signal_error_code(data: &str) -> Result<i32, SignalParseError> {
    let bytes = data.as_bytes();
    let (negative, digits) = match bytes.split_first() {
        Some((b'-', rest)) => (true, rest),
        _ => (false, bytes),
    };
    // Magnitude of i32::MIN; anything larger is out of range for either sign.
    const LIMIT: i64 = 1 << 31;
    let mut magnitude = 0i64;
    let mut count = 0;
    for &byte in digits {
        let digit = byte.wrapping_sub(b'0');
        if digit > 9 {
            break;
        }
        magnitude = magnitude * 10 + i64::from(digit);
        if magnitude > LIMIT {
            return Err(SignalParseError::InvalidErrorCode);
        }
        count += 1;
    }
    if count == 0 {
        return Err(SignalParseError::InvalidErrorCode);
    }
    i32::try_from(if negative { -magnitude } else { magnitude })
        .map_err(|_| SignalParseError::InvalidErrorCode)
}

#[derive(Debug, Error)]
pub enum SignalParseError {
    #[error("unknown signal type")]
    InvalidSignalType,
    #[error("invalid signed 32-bit signal error code")]
    InvalidErrorCode,
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
/// Source: go-nethernet signal.go:formatICECandidate (component, generation and cost constants).
/// Allocates the result once, at its exact length.
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
    let (mut priority_buf, mut port_buf, mut id_buf, mut rport_buf) = (
        itoa::Buffer::new(),
        itoa::Buffer::new(),
        itoa::Buffer::new(),
        itoa::Buffer::new(),
    );
    // Upstream emits related fields only for relay and server-reflexive candidates.
    let related = matches!(candidate_type, "relay" | "srflx");
    let (raddr_key, raddr, rport_key, rport) = if related {
        (
            " raddr ",
            related_address.unwrap_or(""),
            " rport ",
            rport_buf.format(related_port.unwrap_or(0)),
        )
    } else {
        ("", "", "", "")
    };
    let parts = [
        "candidate:",
        foundation,
        " 1 ",
        protocol,
        " ",
        priority_buf.format(priority),
        " ",
        address,
        " ",
        port_buf.format(port),
        " typ ",
        candidate_type,
        raddr_key,
        raddr,
        rport_key,
        rport,
        " generation 0 ufrag ",
        ufrag,
        " network-id ",
        id_buf.format(id),
        " network-cost 0",
    ];
    let mut s = String::with_capacity(parts.iter().map(|part| part.len()).sum());
    for part in parts {
        s.push_str(part);
    }
    s
}

/// Parses a standard ICE candidate, accepting SDP attribute and candidate prefixes.
///
/// Source: go-nethernet conn.go:parseRemoteCandidate. The legacy return type
/// exposes only the fields needed by existing callers; use the native WebRTC
/// candidate parser when component/TCP type information must be preserved.
/// The result borrows from `s` and parsing does not allocate.
///
/// # Security
/// - Limits string field lengths to prevent memory exhaustion
/// - Validates protocol is "udp" or "tcp"
pub fn parse_ice_candidate(s: &str) -> Result<IceCandidateInfo<'_>, SignalParseError> {
    // Check total length limit
    if s.len() > MAX_CANDIDATE_LEN {
        return Err(SignalParseError::FieldTooLong);
    }

    // Source: go-nethernet conn.go:parseRemoteCandidate.
    let s = s.trim();
    let s = s.strip_prefix("a=").unwrap_or(s);
    let s = s.strip_prefix("candidate:").unwrap_or(s);
    // Tokens split on Unicode whitespace. For ASCII text that is exactly
    // `split_ascii_whitespace` plus U+000B, so the byte splitter is used unless
    // the text holds a vertical tab or any non-ASCII byte.
    if s.is_ascii() && !s.as_bytes().contains(&0x0b) {
        parse_candidate_tokens(s.split_ascii_whitespace())
    } else {
        parse_candidate_tokens(s.split_whitespace())
    }
}

fn parse_candidate_tokens<'a>(
    mut parts: impl Iterator<Item = &'a str>,
) -> Result<IceCandidateInfo<'a>, SignalParseError> {
    // Count is checked before any field, preserving upstream error precedence.
    let mut head = [""; 8];
    for slot in &mut head {
        *slot = parts.next().ok_or(SignalParseError::InvalidFormat)?;
    }
    let [
        foundation,
        component,
        protocol,
        priority,
        address,
        port,
        typ,
        candidate_type,
    ] = head;

    if foundation.len() > MAX_FOUNDATION_LEN {
        return Err(SignalParseError::FieldTooLong);
    }

    component
        .parse::<u16>()
        .map_err(|_| SignalParseError::InvalidFormat)?;

    // Validate protocol (must be udp or tcp). No non-ASCII character
    // lowercases to these ASCII letters, so ASCII case folding is exact.
    let protocol = if protocol.eq_ignore_ascii_case("udp") {
        "udp"
    } else if protocol.eq_ignore_ascii_case("tcp") {
        "tcp"
    } else {
        return Err(SignalParseError::InvalidProtocol);
    };

    let priority = priority
        .parse()
        .map_err(|_| SignalParseError::InvalidFormat)?;

    // Validate IP address format
    if address.parse::<std::net::IpAddr>().is_err() && !address.ends_with(".local") {
        return Err(SignalParseError::InvalidIpAddress);
    }

    // Standard ICE accepts the full unsigned 16-bit port range.
    let port: u16 = port.parse().map_err(|_| SignalParseError::InvalidPort)?;

    let candidate_type = match (typ, candidate_type) {
        ("typ", "host") => "host",
        ("typ", "srflx") => "srflx",
        ("typ", "prflx") => "prflx",
        ("typ", "relay") => "relay",
        _ => return Err(SignalParseError::InvalidFormat),
    };

    // Parse optional fields; a trailing key without a value is ignored.
    let mut related_address = None;
    let mut related_port = None;
    let mut ufrag = None;

    while let Some(key) = parts.next() {
        if !matches!(key, "raddr" | "rport" | "ufrag") {
            continue;
        }
        let Some(value) = parts.next() else {
            break;
        };
        match key {
            "raddr" => {
                if value.parse::<std::net::IpAddr>().is_err() {
                    return Err(SignalParseError::InvalidIpAddress);
                }
                related_address = Some(value);
            }
            "rport" => {
                related_port = Some(value.parse().map_err(|_| SignalParseError::InvalidPort)?);
            }
            _ => {
                if value.len() > MAX_UFRAG_LEN {
                    return Err(SignalParseError::FieldTooLong);
                }
                ufrag = Some(value);
            }
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

/// Parsed ICE candidate information, borrowing from the parsed text.
/// `protocol` is lowercased; `candidate_type` is one of `host`, `srflx`, `prflx`, `relay`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IceCandidateInfo<'a> {
    pub foundation: &'a str,
    pub protocol: &'static str,
    pub priority: u32,
    pub address: &'a str,
    pub port: u16,
    pub candidate_type: &'static str,
    pub related_address: Option<&'a str>,
    pub related_port: Option<u16>,
    pub ufrag: Option<&'a str>,
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
    async fn credentials(&self) -> anyhow::Result<Option<Credentials>> {
        Ok(None)
    }

    /// Whether candidates must be embedded in SDP rather than trickled.
    /// Source: go-nethernet signal.go:shouldDisableTrickleICE.
    fn disable_trickle_ice(&self) -> bool {
        false
    }

    /// Whether a listener answers with every candidate inline instead of
    /// trickling them. Defaults to [`Signaling::disable_trickle_ice`]; the real
    /// client's LAN host always answers non-trickle (`ServerNegotiatorNoTrickleIce`
    /// for every LAN-channel offer), which `DiscoveryListener` reproduces.
    fn disable_trickle_ice_for_answer(&self) -> bool {
        self.disable_trickle_ice()
    }

    /// Whether this signaling transport requires the current identity assertion.
    /// The retained legacy franchise service predates this upstream requirement.
    fn requires_identity(&self) -> bool {
        true
    }

    /// Completes on a fatal signaling shutdown; unbound clients remain open.
    /// Source: go-nethernet signal.go:Signaling.Context.
    async fn closed(&self) {
        std::future::pending::<()>().await;
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

    async fn credentials(&self) -> anyhow::Result<Option<Credentials>> {
        (**self).credentials().await
    }

    fn disable_trickle_ice(&self) -> bool {
        (**self).disable_trickle_ice()
    }

    fn disable_trickle_ice_for_answer(&self) -> bool {
        (**self).disable_trickle_ice_for_answer()
    }

    fn requires_identity(&self) -> bool {
        (**self).requires_identity()
    }

    async fn closed(&self) {
        (**self).closed().await;
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
    /// Servers permit one receiver; broadcast-capable clients create independent subscriptions.
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
    #[test]
    fn upstream_candidate_accepts_sdp_attribute_prefix() {
        let parsed =
            parse_ice_candidate(" \ta=candidate:1 1 udp 123 127.0.0.1 9 typ host\r\n").unwrap();
        assert_eq!(parsed.foundation, "1");
    }

    #[test]
    fn standard_candidates_accept_mdns_and_validate_component_and_type() {
        assert!(parse_ice_candidate("candidate:1 1 udp 1 host.local 9 typ host").is_ok());
        for text in [
            "candidate:1 nope udp 1 127.0.0.1 9 typ host",
            "candidate:1 1 udp 1 127.0.0.1 9 typ unknown",
            "candidate:1 1 udp 1 127.0.0.1 9 typ relay raddr 127.0.0.1 rport nope",
        ] {
            assert!(parse_ice_candidate(text).is_err(), "{text}");
        }
    }
    use super::*;

    #[test]
    fn wire_head_formats_every_digit_count() {
        let mut id = 0u64;
        for typ in [
            SignalType::Offer,
            SignalType::Answer,
            SignalType::Candidate,
            SignalType::Error,
        ] {
            for value in [
                0,
                9,
                10,
                99,
                100,
                101,
                12345,
                1 << 32,
                u64::MAX - 1,
                u64::MAX,
            ] {
                let head = WireHead::new(typ, value);
                assert_eq!(head.as_str(), format!("{} {value} ", typ.as_str()));
                id = id.wrapping_add(value);
            }
        }
        for _ in 0..1000 {
            id = id
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let value = id >> (id % 64);
            assert_eq!(
                WireHead::new(SignalType::Error, value).as_str(),
                format!("CONNECTERROR {value} ")
            );
        }
    }

    /// Fixtures from go-nethernet signal_test.go:TestSignalUnmarshalText.
    #[test]
    fn upstream_signal_decimal_prefix_fixtures() {
        for (text, id) in [
            ("CONNECTERROR 42junk -12suffix", 42),
            ("CONNECTERROR 0x10 0x10", 0),
            ("CONNECTERROR 18446744073709551615 2147483647", u64::MAX),
            ("CONNECTERROR 42 -2147483648", 42),
            ("CONNECTREQUEST 42junk offer data", 42),
            ("CONNECTRESPONSE 42junk answer data", 42),
            ("CANDIDATEADD 42junk candidate data", 42),
        ] {
            let signal = Signal::parse(text, "peer".into()).unwrap();
            assert_eq!(signal.connection_id, id, "{text}");
            assert_eq!(
                Signal::parse(&signal.to_string(), "peer".into()).unwrap(),
                signal
            );
        }
        for text in [
            "CONNECTERROR 42 2147483648",
            "CONNECTERROR 42 -2147483649",
            "CONNECTERROR 42 invalid",
            "CONNECTERROR 42 ",
            "CONNECTERROR 42 -",
            "CONNECTERROR 42 +12",
            "CONNECTERROR 42  12",
            "CONNECTERROR 42\t12",
            "CONNECTERROR 18446744073709551616 12",
            "CONNECTERROR -42 12",
            "CONNECTERROR +42 12",
            "CONNECTERROR  42 12",
            "CONNECTERROR invalid 12",
            "UNKNOWN 42 12",
            "connecterror 42 12",
        ] {
            assert!(Signal::parse(text, "peer".into()).is_err(), "{text}");
            assert!(
                Signal::parse_owned(text.into(), "peer".into()).is_err(),
                "{text}"
            );
        }
    }

    /// Display assembles short signals on the stack (one `write_str`) and
    /// streams longer ones; both, and `write_wire`, must emit identical text,
    /// including multi-byte UTF-8 straddling the inline limit.
    #[test]
    fn wire_text_is_identical_across_inline_boundary() {
        // 36 bytes of `CONNECTREQUEST <u64::MAX> ` + data; the inline limit is 256.
        for data_len in [0, 1, 200, 219, 220, 221, 4096] {
            let data: String = "é".repeat(data_len / 2) + &"x".repeat(data_len % 2);
            let signal = Signal {
                typ: SignalType::Offer,
                connection_id: u64::MAX,
                data,
                network_id: "peer".into(),
            };
            let expected = format!("CONNECTREQUEST {} {}", u64::MAX, signal.data);
            assert_eq!(signal.to_string(), expected);
            assert_eq!(signal.to_wire_string(), expected);
            let mut reused = String::from("prefix:");
            signal.write_wire(&mut reused);
            assert_eq!(reused, format!("prefix:{expected}"));
            assert_eq!(Signal::parse(&expected, "peer".into()).unwrap(), signal);
            assert_eq!(
                Signal::parse_owned(expected, "peer".into()).unwrap(),
                signal
            );
        }
    }

    #[test]
    fn signal_type_names_roundtrip_case_sensitively() {
        for typ in [
            SignalType::Offer,
            SignalType::Answer,
            SignalType::Candidate,
            SignalType::Error,
        ] {
            assert_eq!(typ.as_str().parse::<SignalType>().unwrap(), typ);
            assert!(typ.as_str().to_lowercase().parse::<SignalType>().is_err());
        }
    }

    /// Candidate tokens split on Unicode whitespace (as `split_whitespace`),
    /// including U+000B and non-ASCII spaces that the ASCII fast path omits.
    #[test]
    fn candidate_tokens_split_on_unicode_whitespace() {
        for text in [
            "candidate:1\u{0b}1 udp 1 127.0.0.1 9 typ host",
            "candidate:1\u{a0}1 UDP 1 127.0.0.1 9 typ host ufrag\u{3000}u",
            "candidate:1\t1\r\nudp\x0c1 127.0.0.1 9 typ host ufrag u",
        ] {
            let parsed = parse_ice_candidate(text).unwrap();
            assert_eq!(
                (parsed.foundation, parsed.protocol, parsed.port),
                ("1", "udp", 9),
                "{text:?}"
            );
        }
        assert_eq!(
            parse_ice_candidate("candidate:1\u{a0}1 udp 1 127.0.0.1 9 typ host ufrag\u{3000}u")
                .unwrap()
                .ufrag,
            Some("u")
        );
        // A key without a value is ignored; a key consumes the next token.
        let parsed = parse_ice_candidate(
            "candidate:1 1 udp 1 127.0.0.1 9 typ relay rport raddr 10.0.0.1 ufrag",
        );
        assert!(matches!(parsed, Err(SignalParseError::InvalidPort)));
        let parsed =
            parse_ice_candidate("candidate:1 1 udp 1 127.0.0.1 9 typ host x ufrag").unwrap();
        assert_eq!(parsed.ufrag, None);
    }

    #[test]
    fn upstream_host_candidate_omits_related_address() {
        assert_eq!(
            format_ice_candidate(
                2,
                "abc",
                "udp",
                1,
                "127.0.0.1",
                9,
                "host",
                Some("10.0.0.1"),
                Some(123),
                "xyz"
            ),
            "candidate:abc 1 udp 1 127.0.0.1 9 typ host generation 0 ufrag xyz network-id 2 network-cost 0"
        );
    }

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
        assert_eq!(result.ufrag, Some("abc123"));
    }

    #[test]
    fn test_parse_ice_candidate_valid_relay() {
        let candidate = "candidate:2 1 udp 1677721855 203.0.113.50 19302 typ relay raddr 192.168.1.100 rport 54321 generation 0 ufrag xyz";
        let result = parse_ice_candidate(candidate).unwrap();

        assert_eq!(result.candidate_type, "relay");
        assert_eq!(result.address, "203.0.113.50");
        assert_eq!(result.port, 19302);
        assert_eq!(result.related_address, Some("192.168.1.100"));
        assert_eq!(result.related_port, Some(54321));
        assert_eq!(result.ufrag, Some("xyz"));
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
    fn test_parse_ice_candidate_standard_port_zero() {
        let candidate = "candidate:1 1 udp 2130706431 192.168.1.100 0 typ host";
        let result = parse_ice_candidate(candidate);

        assert_eq!(result.unwrap().port, 0);
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

        assert_eq!(signal.typ, SignalType::Offer);
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
