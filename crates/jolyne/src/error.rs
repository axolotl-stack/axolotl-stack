//! Error types shared by the sans-IO core ([`crate::proto`]) and the async layer.
//!
//! Everything here is runtime-free. Transport errors are carried as boxed
//! sources (never stringified) so callers can downcast to the concrete
//! RakNet/NetherNet error.

use std::fmt;

use thiserror::Error;

use crate::valentine::{EnumsConnectionDisconnectFailReason, PlayStatusPacketStatus};

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AuthError {
    #[error("Empty login chain")]
    EmptyChain,
    #[error("Missing login chain")]
    MissingChain,
    #[error("Legacy/self-signed authentication is disabled on this server")]
    LegacyAuthDisabled,
    #[error("Missing authentication type")]
    MissingAuthType,
    #[error("Missing authentication token")]
    MissingToken,
    #[error("Missing authentication certificate")]
    MissingCertificate,
    #[error("Unsupported authentication type: {0}")]
    UnsupportedAuthType(u32),
    #[error("Login identity payload too large (max {0} bytes)")]
    PayloadTooLarge(usize),
    #[error("Invalid JWT header: {0}")]
    InvalidHeader(String),
    #[error("Unsupported JWT algorithm: {0}")]
    UnsupportedAlg(String),
    #[error("JWT validation failed: {0}")]
    BadSignature(String),
    #[error("JWT issuer not accepted: {0}")]
    InvalidIssuer(String),
    #[error("JWT audience not accepted: {0}")]
    InvalidAudience(String),
    #[error("Login chain has too many entries (max {0})")]
    ChainTooLong(usize),
    #[error("JWT token too large (max {0} bytes)")]
    TokenTooLarge(usize),
    #[error("Missing identityPublicKey")]
    MissingIdentityKey,
    #[error("Missing extraData")]
    MissingExtraData,
    #[error("Invalid UTF-8 in identity payload")]
    InvalidUtf8,
    #[error("Invalid JSON in identity payload")]
    InvalidJson,
    #[error("Token expired or not yet valid")]
    TemporalValidation,
    /// The provider-verified token carries no XUID, so it does not prove an
    /// Xbox Live account (gophertunnel `AuthResult.XBOXLiveAuthenticated`).
    #[error("Login token does not identify an Xbox Live account")]
    NotAuthenticated,
    /// Identity claims failed gophertunnel `IdentityData.Validate` rules. The
    /// payload names the rule only; client values are never echoed.
    #[error("Invalid identity data: {0}")]
    InvalidIdentityData(&'static str),
    /// Client data failed gophertunnel `ClientData.Validate` rules. The payload
    /// names the rule only; client values are never echoed.
    #[error("Invalid client data: {0}")]
    InvalidClientData(&'static str),
    /// Discovery, OpenID configuration or JWKS could not be obtained.
    #[error("Authentication provider unavailable: {0}")]
    ProviderUnavailable(String),
    /// A client-side login token could not be produced or acquired.
    #[error("Login token acquisition failed: {0}")]
    TokenAcquisition(String),
    /// Online mode with Bedrock encryption disabled on a transport that cannot
    /// prove the client's key (RakNet): a captured `Login` could be replayed.
    #[error("online mode requires encryption on this transport")]
    OnlineModeRequiresEncryption,
}

/// The handshake stage an error or timeout belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum HandshakePhase {
    /// Waiting for `RequestNetworkSettings` (server) or `NetworkSettings` (client).
    NetworkSettings,
    /// Waiting for `Login` (server) or for login tokens to be supplied (client).
    Login,
    /// Running the online/offline login verification.
    Authentication,
    /// ECDH handshake: waiting for `ClientToServerHandshake` (server) or
    /// `ServerToClientHandshake`/`PlayStatus(LoginSuccess)` (client).
    Encryption,
    /// Resource pack negotiation and transfer.
    ResourcePacks,
    /// `StartGame` through `SetLocalPlayerAsInitialized`.
    Spawn,
}

impl fmt::Display for HandshakePhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NetworkSettings => "network settings",
            Self::Login => "login",
            Self::Authentication => "authentication",
            Self::Encryption => "encryption handshake",
            Self::ResourcePacks => "resource packs",
            Self::Spawn => "spawn",
        })
    }
}

/// Why a resource pack exchange failed.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PackError {
    #[error("client refused required resource packs")]
    Refused,
    #[error("client requested unknown resource pack {0}")]
    UnknownPack(String),
    #[error("chunk request for pack {pack} index {index} is out of order (expected {expected})")]
    ChunkOutOfOrder {
        pack: String,
        index: u32,
        expected: u32,
    },
    #[error("chunk request for pack {pack} index {index} exceeds {count} chunks")]
    ChunkOutOfRange {
        pack: String,
        index: u32,
        count: u32,
    },
    #[error("server offered {count} resource packs (max {max})")]
    TooManyPacks { count: usize, max: usize },
    #[error("resource pack {pack} is {size} bytes (max {max})")]
    PackTooLarge { pack: String, size: u64, max: u64 },
    #[error("resource pack transfer exceeds {max} total bytes")]
    TotalTooLarge { max: u64 },
    #[error("invalid chunk size {size} for pack {pack}")]
    InvalidChunkSize { pack: String, size: u32 },
    #[error("unexpected chunk for pack {pack}: {reason}")]
    UnexpectedChunk { pack: String, reason: &'static str },
    #[error("SHA-256 mismatch for pack {0}")]
    HashMismatch(String),
    #[error("server stack references pack {0} that was neither downloaded nor built in")]
    MissingStackPack(String),
    #[error("resource pack {0} is invalid: {1}")]
    Invalid(String, &'static str),
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProtocolError {
    #[error("expected batch marker 0xfe, got 0x{0:02x}")]
    InvalidBatchId(u8),
    #[error("unknown batch compression algorithm 0x{0:02x}")]
    UnknownCompression(u8),
    #[error("compressed batch has no algorithm byte")]
    EmptyCompressedBatch,
    #[error("decompression failed: {0}")]
    DecompressionFailed(String),
    #[error("incompatible protocol version: client {client_protocol}, server {server_protocol}")]
    IncompatibleProtocol {
        client_protocol: i32,
        server_protocol: i32,
    },
    #[error("batch payload exceeds max decompressed size ({actual} > {max})")]
    BatchTooLarge { actual: usize, max: usize },
    #[error("batch contains more than {max} packets")]
    TooManyPackets { max: usize },
    #[error("declared packet length {declared} exceeds available {available}")]
    PacketLength { declared: usize, available: usize },
    #[error("unknown packet id {0}")]
    UnknownPacket(u16),
    #[error("unexpected {got} during {phase}")]
    UnexpectedPacket { phase: HandshakePhase, got: String },
    #[error("timed out during {0}")]
    Timeout(HandshakePhase),
    #[error("timed out draining output while closing")]
    CloseTimeout,
    #[error("invalid encryption handshake: {0}")]
    InvalidHandshake(String),
    #[error("encrypted frame of {len} bytes is shorter than the {min}-byte minimum")]
    ShortEncryptedFrame { len: usize, min: usize },
    #[error("batch checksum mismatch on encrypted batch {counter}")]
    ChecksumMismatch { counter: u64 },
    #[error("cipher invalidated by an earlier checksum failure")]
    CipherInvalidated,
    #[error("unreliable sends are not allowed while encryption is active")]
    UnreliableWhileEncrypted,
    #[error("unsupported compression algorithm {0} in NetworkSettings")]
    UnsupportedNetworkCompression(u16),
    #[error("invalid DEFLATE compression level {0}")]
    InvalidCompressionLevel(u32),
    #[error("invalid chunk radius {0}")]
    InvalidChunkRadius(i32),
    #[error("SetLocalPlayerAsInitialized runtime id {got} does not match StartGame {expected}")]
    RuntimeIdMismatch { expected: u64, got: u64 },
    #[error("conflicting StartGame runtime ids: first {first}, then {second}")]
    ConflictingStartGame { first: u64, second: u64 },
    #[error("resource packs: {0}")]
    Pack(#[from] PackError),
    #[error(
        "more than {max_packets} packets / {max_bytes} bytes arrived before the phase that handles them"
    )]
    DeferredOverflow {
        max_packets: usize,
        max_bytes: usize,
    },
    #[error("write buffer full: {pending} bytes pending (max {max})")]
    WriteBufferFull { pending: usize, max: usize },
    #[error("packet of {0} bytes exceeds the u32 length prefix")]
    PacketTooLarge(usize),
    #[error("packet id {0} does not match the custom packet type's ID")]
    NotCustomPacket(u16),
    #[error("packet id {0} is not a valid custom packet ID (ten-bit, outside valentine)")]
    InvalidPacketId(u16),
    #[error("handshake token: {0}")]
    HandshakeToken(String),
}

/// A `Disconnect` packet received from the peer.
#[derive(Debug, Clone)]
pub struct PeerDisconnect {
    pub reason: EnumsConnectionDisconnectFailReason,
    /// The message, if the packet carried one.
    pub message: Option<String>,
    /// The original packet, so proxies can forward it verbatim.
    pub packet: crate::proto::raw::RawPacket,
}

/// Why the server refused a login during admission.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LoginRefusal {
    /// `max_players` reached; `PlayStatus(LoginFailedServerFull)` was queued.
    ServerFull,
    /// The admission hook refused the player with this disconnect message.
    Denied(String),
}

impl fmt::Display for LoginRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ServerFull => f.write_str("server full"),
            Self::Denied(message) => write!(f, "denied: {message}"),
        }
    }
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum JolyneError {
    #[error("Decode error: {0}")]
    Decode(#[from] valentine::bedrock::error::DecodeError),

    #[error(
        "decode packet {packet_id} (body_len={body_len}, body_preview={body_preview:02x?}): {source}"
    )]
    PacketDecode {
        packet_id: u16,
        body_len: usize,
        body_preview: Vec<u8>,
        #[source]
        source: valentine::bedrock::error::DecodeError,
    },

    #[error("decode packet {packet_id} (body_len={body_len}) left {remaining} trailing body bytes")]
    PacketTrailingBytes {
        packet_id: u16,
        body_len: usize,
        remaining: usize,
    },

    /// A registered custom packet or a passed-through unknown packet reached a
    /// typed (`McpePacket`) receive API. The packet was consumed; the
    /// connection remains usable. Use the raw receive APIs to handle such IDs.
    #[error("packet id {} is not a valentine packet", .0.id())]
    NonValentinePacket(Box<crate::proto::raw::RawPacket>),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("transport error: {0}")]
    Transport(#[source] Box<dyn std::error::Error + Send + Sync>),

    #[error("Connection closed")]
    ConnectionClosed,

    /// The peer sent a `Disconnect` packet.
    #[error("peer disconnected ({:?}): {}", .0.reason, .0.message.as_deref().unwrap_or(""))]
    Disconnected(Box<PeerDisconnect>),

    /// The server answered the login with a failure `PlayStatus`.
    #[error("login refused by server: {0:?}")]
    LoginRefused(PlayStatusPacketStatus),

    /// The server refused the player during admission.
    #[error("login refused: {0}")]
    Refused(LoginRefusal),

    #[error("Authentication error: {0}")]
    Auth(#[from] AuthError),

    #[error("Protocol error: {0}")]
    Protocol(#[from] ProtocolError),
}

impl From<PackError> for JolyneError {
    fn from(error: PackError) -> Self {
        Self::Protocol(ProtocolError::Pack(error))
    }
}

impl JolyneError {
    /// Wraps a transport-specific error without stringifying it.
    pub fn transport(error: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::Transport(Box::new(error))
    }

    /// Whether the connection can keep being used after this error.
    ///
    /// Only [`JolyneError::NonValentinePacket`] and per-packet decode errors
    /// leave the connection state intact; everything else is terminal.
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            Self::NonValentinePacket(_)
                | Self::PacketDecode { .. }
                | Self::PacketTrailingBytes { .. }
        )
    }

    /// The disconnect reason a server should report to the client for this
    /// failure, or `None` when no packet should be sent (transport failure,
    /// peer already gone, refusal already answered with `PlayStatus`).
    pub fn disconnect_reason(&self) -> Option<EnumsConnectionDisconnectFailReason> {
        use EnumsConnectionDisconnectFailReason as R;
        Some(match self {
            Self::Transport(_)
            | Self::Io(_)
            | Self::ConnectionClosed
            | Self::Disconnected(_)
            | Self::LoginRefused(_)
            // Refusals are answered by the phase itself (PlayStatus or Kicked).
            | Self::Refused(_) => return None,
            Self::Auth(_) => R::Notauthenticated,
            Self::Protocol(ProtocolError::IncompatibleProtocol { .. }) => return None,
            Self::Protocol(ProtocolError::Timeout(HandshakePhase::Spawn)) => {
                R::Loadingstatetimeout
            }
            Self::Protocol(ProtocolError::Timeout(_)) => R::Timeout,
            Self::Protocol(ProtocolError::Pack(_)) => R::Resourcepackproblem,
            Self::Protocol(ProtocolError::UnknownPacket(_)) => R::Unknownpacket,
            Self::Protocol(_)
            | Self::Decode(_)
            | Self::PacketDecode { .. }
            | Self::PacketTrailingBytes { .. }
            | Self::NonValentinePacket(_) => R::Badpacket,
        })
    }

    /// The message sent with [`Self::disconnect_reason`]: one fixed sentence
    /// per reason, never the error's own text, so decode previews, token and
    /// signature errors and other internals stay in the server's logs.
    pub fn disconnect_message(&self) -> &'static str {
        use EnumsConnectionDisconnectFailReason as R;
        match self.disconnect_reason() {
            Some(R::Notauthenticated) => "Authentication failed.",
            Some(R::Loadingstatetimeout) => "Timed out while joining the world.",
            Some(R::Timeout) => "Connection timed out.",
            Some(R::Resourcepackproblem) => "Resource pack negotiation failed.",
            Some(R::Unknownpacket) => "Received an unknown packet.",
            Some(R::Badpacket) => "Received an invalid packet.",
            _ => "Disconnected.",
        }
    }
}
