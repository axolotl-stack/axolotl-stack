//! Error types shared by the sans-IO core ([`crate::proto`]) and the async layer.
//!
//! Everything here is runtime-free. Transport errors are carried as boxed
//! sources (never stringified) so callers can downcast to the concrete
//! RakNet/NetherNet error.

use std::fmt;

use thiserror::Error;

use crate::valentine::{EnumsConnectionDisconnectFailReason, PlayStatusPacketStatus};

/// Why login authentication or login token handling failed.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AuthError {
    /// The self-signed login certificate chain has no entries.
    #[error("Empty login chain")]
    EmptyChain,
    /// A self-signed or guest login arrived while online mode is on or legacy
    /// authentication is not allowed.
    #[error("Legacy/self-signed authentication is disabled on this server")]
    LegacyAuthDisabled,
    /// The login's auth info JSON has no `AuthenticationType`.
    #[error("Missing authentication type")]
    MissingAuthType,
    /// A full (`AuthenticationType` 0) login has no or an empty `Token`.
    #[error("Missing authentication token")]
    MissingToken,
    /// A self-signed login without a token has no extractable certificate chain.
    #[error("Missing authentication certificate")]
    MissingCertificate,
    /// The login's `AuthenticationType` is not 0 (full), 1 (guest) or 2
    /// (self-signed); holds the received value.
    #[error("Unsupported authentication type: {0}")]
    UnsupportedAuthType(u32),
    /// A login payload section is too long for its length prefix; holds the
    /// maximum in bytes.
    #[error("Login identity payload too large (max {0} bytes)")]
    PayloadTooLarge(usize),
    /// A JWT header cannot be decoded; holds the decoder's message.
    #[error("Invalid JWT header: {0}")]
    InvalidHeader(String),
    /// A JWT is malformed, has a bad signature or no trusted key verifies it;
    /// holds the reason.
    #[error("JWT validation failed: {0}")]
    BadSignature(String),
    /// A provider token's `iss` claim is not the trusted issuer; holds the
    /// validator's message.
    #[error("JWT issuer not accepted: {0}")]
    InvalidIssuer(String),
    /// A provider token's `aud` claim is not accepted; holds the validator's
    /// message.
    #[error("JWT audience not accepted: {0}")]
    InvalidAudience(String),
    /// The self-signed certificate chain has more tokens than allowed; holds the
    /// maximum.
    #[error("Login chain has too many entries (max {0})")]
    ChainTooLong(usize),
    /// The last self-signed chain token is larger than allowed; holds the
    /// maximum in bytes.
    #[error("JWT token too large (max {0} bytes)")]
    TokenTooLarge(usize),
    /// The identity claims (or a provider JWK) lack the public key needed to
    /// verify the login.
    #[error("Missing identityPublicKey")]
    MissingIdentityKey,
    /// The self-signed chain claims have no `extraData` identity object.
    #[error("Missing extraData")]
    MissingExtraData,
    /// A section of the `Login` connection request is not valid UTF-8.
    #[error("Invalid UTF-8 in identity payload")]
    InvalidUtf8,
    /// The `Login` connection request framing, auth info, token claims or key
    /// set JSON is malformed.
    #[error("Invalid JSON in identity payload")]
    InvalidJson,
    /// A JWT's `exp`/`nbf` window, after clock-skew leeway, excludes the
    /// current time.
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
    /// The client cancelled the exchange while the server requires its packs.
    #[error("client refused required resource packs")]
    Refused,
    /// The peer named a pack (`uuid_version`) that is not part of the exchange.
    #[error("client requested unknown resource pack {0}")]
    UnknownPack(String),
    /// The client requested a chunk other than the next one in sequence.
    #[error("chunk request for pack {pack} index {index} is out of order (expected {expected})")]
    ChunkOutOfOrder {
        /// The pack's `uuid_version` name.
        pack: String,
        /// The requested chunk index.
        index: u32,
        /// The chunk index the server expected next.
        expected: u32,
    },
    /// The client requested a chunk index past the pack's last chunk.
    #[error("chunk request for pack {pack} index {index} exceeds {count} chunks")]
    ChunkOutOfRange {
        /// The pack's `uuid_version` name.
        pack: String,
        /// The requested chunk index (0 if it did not fit in a `u32`).
        index: u32,
        /// The pack's chunk count.
        count: u32,
    },
    /// `ResourcePacksInfo` offers more packs than the client accepts.
    #[error("server offered {count} resource packs (max {max})")]
    TooManyPacks {
        /// The number of packs offered.
        count: usize,
        /// The client's pack limit.
        max: usize,
    },
    /// An offered pack is larger than the client accepts.
    #[error("resource pack {pack} is {size} bytes (max {max})")]
    PackTooLarge {
        /// The pack's `uuid_version` name.
        pack: String,
        /// The offered pack size in bytes.
        size: u64,
        /// The client's per-pack limit in bytes.
        max: u64,
    },
    /// The offered packs together exceed the client's total download limit.
    #[error("resource pack transfer exceeds {max} total bytes")]
    TotalTooLarge {
        /// The client's total limit in bytes.
        max: u64,
    },
    /// `ResourcePackDataInfo` declares a chunk size of zero or one yielding
    /// more than `u32::MAX` chunks.
    #[error("invalid chunk size {size} for pack {pack}")]
    InvalidChunkSize {
        /// The pack's `uuid_version` name.
        pack: String,
        /// The declared chunk size in bytes.
        size: u32,
    },
    /// A pack data packet arrived that does not fit the current transfer.
    #[error("unexpected chunk for pack {pack}: {reason}")]
    UnexpectedChunk {
        /// The pack's `uuid_version` name.
        pack: String,
        /// What was wrong with the packet.
        reason: &'static str,
    },
    /// A downloaded pack's SHA-256 does not match `ResourcePackDataInfo`; holds
    /// the pack name.
    #[error("SHA-256 mismatch for pack {0}")]
    HashMismatch(String),
    /// `ResourcePackStack` lists a pack the client neither downloaded nor has
    /// built in; holds its `uuid_version` name.
    #[error("server stack references pack {0} that was neither downloaded nor built in")]
    MissingStackPack(String),
    /// A pack or pack message is inconsistent; holds the pack name (or the
    /// exchange step) and the reason.
    #[error("resource pack {0} is invalid: {1}")]
    Invalid(String, &'static str),
}

/// A violation of the Bedrock wire protocol or of a local protocol limit.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProtocolError {
    /// A RakNet batch does not start with the `0xFE` game packet marker; holds
    /// the byte found.
    #[error("expected batch marker 0xfe, got 0x{0:02x}")]
    InvalidBatchId(u8),
    /// A compressed batch names an unknown compression algorithm; holds the byte.
    #[error("unknown batch compression algorithm 0x{0:02x}")]
    UnknownCompression(u8),
    /// Compression is enabled but the batch has no algorithm byte.
    #[error("compressed batch has no algorithm byte")]
    EmptyCompressedBatch,
    /// The batch payload cannot be decompressed or inflates past the size
    /// limit; holds the reason.
    #[error("decompression failed: {0}")]
    DecompressionFailed(String),
    /// The client's protocol version differs from the server's.
    #[error("incompatible protocol version: client {client_protocol}, server {server_protocol}")]
    IncompatibleProtocol {
        /// The protocol version from `RequestNetworkSettings`.
        client_protocol: i32,
        /// The protocol version this server speaks.
        server_protocol: i32,
    },
    /// A batch payload exceeds the maximum decompressed size.
    #[error("batch payload exceeds max decompressed size ({actual} > {max})")]
    BatchTooLarge {
        /// The payload size in bytes.
        actual: usize,
        /// The size limit in bytes.
        max: usize,
    },
    /// A batch holds more packets than allowed.
    #[error("batch contains more than {max} packets")]
    TooManyPackets {
        /// The per-batch packet limit.
        max: usize,
    },
    /// A packet's length prefix runs past the end of the batch.
    #[error("declared packet length {declared} exceeds available {available}")]
    PacketLength {
        /// The length prefix in bytes.
        declared: usize,
        /// The bytes left in the batch.
        available: usize,
    },
    /// A packet ID is neither a valentine packet nor a registered custom one
    /// and the unknown packet policy rejects it; holds the ID.
    #[error("unknown packet id {0}")]
    UnknownPacket(u16),
    /// A handshake phase received a packet it does not handle.
    #[error("unexpected {got} during {phase}")]
    UnexpectedPacket {
        /// The phase that received the packet.
        phase: HandshakePhase,
        /// The packet's name, or `packet id N` for non-valentine IDs.
        got: String,
    },
    /// A handshake phase hit its deadline; holds the phase.
    #[error("timed out during {0}")]
    Timeout(HandshakePhase),
    /// Closing could not drain pending output and close the transport in time.
    #[error("timed out draining output while closing")]
    CloseTimeout,
    /// A handshake public key or the `ServerToClientHandshake` token is
    /// malformed, fails verification or is duplicated; holds the reason.
    #[error("invalid encryption handshake: {0}")]
    InvalidHandshake(String),
    /// An encrypted frame is too short to hold its header and checksum.
    #[error("encrypted frame of {len} bytes is shorter than the {min}-byte minimum")]
    ShortEncryptedFrame {
        /// The frame length in bytes.
        len: usize,
        /// The minimum length in bytes.
        min: usize,
    },
    /// A decrypted batch fails its SHA-256 checksum; the cipher is invalidated.
    #[error("batch checksum mismatch on encrypted batch {counter}")]
    ChecksumMismatch {
        /// The receive counter of the rejected batch.
        counter: u64,
    },
    /// The cipher is used again after a checksum failure invalidated it.
    #[error("cipher invalidated by an earlier checksum failure")]
    CipherInvalidated,
    /// An unreliable send was attempted once encryption is enabled, which needs
    /// in-order delivery for its counter.
    #[error("unreliable sends are not allowed while encryption is active")]
    UnreliableWhileEncrypted,
    /// `NetworkSettings` names an unknown compression algorithm; holds its value.
    #[error("unsupported compression algorithm {0} in NetworkSettings")]
    UnsupportedNetworkCompression(u16),
    /// The compressor rejects the configured DEFLATE level; holds the level.
    #[error("invalid DEFLATE compression level {0}")]
    InvalidCompressionLevel(u32),
    /// A requested or updated chunk radius is below 1; holds the radius.
    #[error("invalid chunk radius {0}")]
    InvalidChunkRadius(i32),
    /// `SetLocalPlayerAsInitialized` names a runtime ID other than the player's.
    #[error("SetLocalPlayerAsInitialized runtime id {got} does not match StartGame {expected}")]
    RuntimeIdMismatch {
        /// The player's runtime ID from `StartGame`.
        expected: u64,
        /// The runtime ID the client sent.
        got: u64,
    },
    /// The client received a second `StartGame` with a different runtime ID.
    #[error("conflicting StartGame runtime ids: first {first}, then {second}")]
    ConflictingStartGame {
        /// The runtime ID from the first `StartGame`.
        first: u64,
        /// The runtime ID from the later `StartGame`.
        second: u64,
    },
    /// The resource pack exchange failed.
    #[error("resource packs: {0}")]
    Pack(#[from] PackError),
    /// Too many packets arrived ahead of the phase that handles them.
    #[error(
        "more than {max_packets} packets / {max_bytes} bytes arrived before the phase that handles them"
    )]
    DeferredOverflow {
        /// The deferred packet limit.
        max_packets: usize,
        /// The deferred byte limit.
        max_bytes: usize,
    },
    /// Queued output would exceed `max_queued_bytes`; the batch is dropped.
    #[error("write buffer full: {pending} bytes pending (max {max})")]
    WriteBufferFull {
        /// The bytes that would be queued.
        pending: usize,
        /// The queue limit in bytes.
        max: usize,
    },
    /// An encoded packet is too long for its `u32` length prefix; holds its size.
    #[error("packet of {0} bytes exceeds the u32 length prefix")]
    PacketTooLarge(usize),
    /// A custom packet decode was attempted on another packet ID; holds the
    /// packet's actual ID.
    #[error("packet id {0} does not match the custom packet type's ID")]
    NotCustomPacket(u16),
    /// A custom packet ID exceeds the ten-bit maximum or is a valentine packet
    /// ID; holds the ID.
    #[error("packet id {0} is not a valid custom packet ID (ten-bit, outside valentine)")]
    InvalidPacketId(u16),
    /// The server's `ServerToClientHandshake` token cannot be encoded; holds the
    /// reason.
    #[error("handshake token: {0}")]
    HandshakeToken(String),
}

/// A `Disconnect` packet received from the peer.
#[derive(Debug, Clone)]
pub struct PeerDisconnect {
    /// The disconnect reason the peer reported.
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

/// Every error a Jolyne connection, handshake or listener reports.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum JolyneError {
    /// A valentine codec error without per-packet context; received packets
    /// report [`Self::PacketDecode`] instead.
    #[error("Decode error: {0}")]
    Decode(#[from] valentine::bedrock::error::DecodeError),

    /// A received packet's body fails to decode. The connection remains usable.
    #[error(
        "decode packet {packet_id} (body_len={body_len}, body_preview={body_preview:02x?}): {source}"
    )]
    PacketDecode {
        /// The packet ID.
        packet_id: u16,
        /// The body length in bytes.
        body_len: usize,
        /// The first 32 body bytes, at most.
        body_preview: Vec<u8>,
        /// The valentine decode error.
        #[source]
        source: valentine::bedrock::error::DecodeError,
    },

    /// A received packet decodes but leaves unread body bytes. The connection
    /// remains usable.
    #[error("decode packet {packet_id} (body_len={body_len}) left {remaining} trailing body bytes")]
    PacketTrailingBytes {
        /// The packet ID.
        packet_id: u16,
        /// The body length in bytes.
        body_len: usize,
        /// The unread bytes left after decoding.
        remaining: usize,
    },

    /// A registered custom packet or a passed-through unknown packet reached a
    /// typed (`McpePacket`) receive API. The packet was consumed; the
    /// connection remains usable. Use the raw receive APIs to handle such IDs.
    #[error("packet id {} is not a valentine packet", .0.id())]
    NonValentinePacket(Box<crate::proto::raw::RawPacket>),

    /// An I/O error, such as an invalid listen address.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// The underlying transport (RakNet/NetherNet) failed; downcast the source
    /// for the concrete error.
    #[error("transport error: {0}")]
    Transport(#[source] Box<dyn std::error::Error + Send + Sync>),

    /// The connection is closed: the peer's `Disconnect` was already reported,
    /// the transport ended or the listener stopped accepting.
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

    /// Login authentication failed.
    #[error("Authentication error: {0}")]
    Auth(#[from] AuthError),

    /// The peer violated the protocol or a protocol limit was exceeded.
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
