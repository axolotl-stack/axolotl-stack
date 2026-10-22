//! Connection, server and client configuration (runtime-free).

#[cfg(feature = "server")]
use std::net::SocketAddr;
#[cfg(feature = "server")]
use std::sync::Arc;
use std::time::Duration;

#[cfg(feature = "server")]
use crate::login::AuthenticatedLogin;
use crate::proto::batch::{BatchCompression, BatchLimits};
use crate::proto::custom::{PacketRegistry, UnknownPacketPolicy};
use crate::proto::observe::Observer;
use crate::proto::packs::ClientPackPolicy;
#[cfg(feature = "server")]
use crate::proto::packs::ServerPacks;
#[cfg(feature = "server")]
use crate::status::StatusProvider;

/// When queued outbound packets are sealed into a batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FlushMode {
    /// Every `send_packet` is its own batch (lowest latency).
    #[default]
    Immediate,
    /// Packets accumulate until `flush()` (tick-driven servers).
    Manual,
    /// Packets accumulate and are flushed at most this long after the first
    /// queued packet, while the connection is being driven (gophertunnel's
    /// 50 ms `FlushRate`), plus on explicit `flush()`.
    Interval(Duration),
}

impl FlushMode {
    /// gophertunnel's default flush cadence.
    pub const GOPHERTUNNEL: Self = Self::Interval(Duration::from_millis(50));
}

/// Codec-level options shared by client and server connections.
#[derive(Clone)]
pub struct ConnectionOptions {
    /// Inbound decompression and packet-count bounds.
    pub limits: BatchLimits,
    /// Policy for packet IDs valentine does not know.
    pub unknown_packets: UnknownPacketPolicy,
    /// Extra packet IDs treated as known.
    pub custom_packets: PacketRegistry,
    pub flush: FlushMode,
    /// Outbound packets per batch before the pending batch is sealed
    /// automatically (gophertunnel listeners reject more than 812).
    pub max_packets_per_outbound_batch: usize,
    /// Pending (unsealed) payload bytes before the batch is sealed automatically.
    pub max_pending_bytes: usize,
    /// Sealed batches waiting for the transport, in bytes. Exceeding it fails
    /// the send with `ProtocolError::WriteBufferFull` (the async layer drains
    /// after every send, so this only trips for stalled custom drivers).
    pub max_queued_bytes: usize,
    /// Optional read-only packet hook.
    pub observer: Option<Observer>,
}

impl Default for ConnectionOptions {
    fn default() -> Self {
        Self {
            limits: BatchLimits::default(),
            unknown_packets: UnknownPacketPolicy::Reject,
            custom_packets: PacketRegistry::default(),
            flush: FlushMode::Immediate,
            max_packets_per_outbound_batch: 812,
            max_pending_bytes: 4 * 1024 * 1024,
            max_queued_bytes: 64 * 1024 * 1024,
            observer: None,
        }
    }
}

impl std::fmt::Debug for ConnectionOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectionOptions")
            .field("limits", &self.limits)
            .field("unknown_packets", &self.unknown_packets)
            .field("flush", &self.flush)
            .field(
                "max_packets_per_outbound_batch",
                &self.max_packets_per_outbound_batch,
            )
            .field("max_pending_bytes", &self.max_pending_bytes)
            .field("max_queued_bytes", &self.max_queued_bytes)
            .field("observer", &self.observer.is_some())
            .finish_non_exhaustive()
    }
}

/// Batch compression the server negotiates in `NetworkSettings`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompressionConfig {
    /// `Deflate` (vanilla default), `Snappy`, or `None`.
    pub algorithm: BatchCompression,
    /// Deflate level 1..=9 (clamped); ignored for Snappy/None.
    pub level: u32,
    /// Batches shorter than this are sent uncompressed. gophertunnel's
    /// default is 256; BDS ships `compression-threshold=1`.
    pub threshold: u16,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            algorithm: BatchCompression::Deflate,
            level: 7,
            threshold: 256,
        }
    }
}

#[cfg(feature = "server")]
/// Server-side admission check run after login verification and before the
/// encryption handshake (gophertunnel `ListenConfig.Allow`).
pub trait AdmissionHook: Send + Sync {
    /// Returns `Err(message)` to refuse the player with a `Kicked` disconnect.
    fn admit(&self, peer: SocketAddr, login: &AuthenticatedLogin) -> Result<(), String>;
}

#[cfg(feature = "server")]
impl<F> AdmissionHook for F
where
    F: Fn(SocketAddr, &AuthenticatedLogin) -> Result<(), String> + Send + Sync,
{
    fn admit(&self, peer: SocketAddr, login: &AuthenticatedLogin) -> Result<(), String> {
        self(peer, login)
    }
}

#[cfg(feature = "server")]
/// Server configuration shared by every connection a listener accepts.
#[derive(Clone)]
pub struct BedrockListenerConfig {
    /// Require Xbox Live (OpenID) authentication. Default: `true`.
    pub online_mode: bool,
    /// Accept self-signed chains when `online_mode` is off. Default: `true`.
    pub allow_legacy_auth: bool,
    /// Run the ECDH handshake and AES-CTR batch encryption. Default: `true`
    /// (NetherNet listeners default to `false`; DTLS already encrypts).
    pub encryption_enabled: bool,
    pub compression: CompressionConfig,
    /// Deadline from accept through `ClientToServerHandshake` (settings,
    /// login, authentication, encryption). Default: 30 s.
    pub login_timeout: Duration,
    /// Deadline for the resource pack phase. Default: 5 min.
    pub resource_pack_timeout: Duration,
    /// Deadline from `StartGame` to `SetLocalPlayerAsInitialized`
    /// (gophertunnel: 1 min). Default: 60 s.
    pub spawn_timeout: Duration,
    /// Packs offered to every player.
    pub resource_packs: Arc<ServerPacks>,
    /// Players allowed concurrently (0 = unlimited). When full, the login is
    /// answered with `PlayStatus(LoginFailedServerFull)`.
    pub max_players: usize,
    pub admission: Option<Arc<dyn AdmissionHook>>,
    /// Status (MOTD) advertised to RakNet pings and NetherNet LAN discovery.
    pub status: Option<Arc<dyn StatusProvider>>,
    /// How often the advertised status is refreshed (gophertunnel: 4 s).
    pub status_refresh: Duration,
    pub connection: ConnectionOptions,
}

#[cfg(feature = "server")]
impl Default for BedrockListenerConfig {
    fn default() -> Self {
        Self {
            online_mode: true,
            allow_legacy_auth: true,
            encryption_enabled: true,
            compression: CompressionConfig::default(),
            login_timeout: Duration::from_secs(30),
            resource_pack_timeout: Duration::from_secs(300),
            spawn_timeout: Duration::from_secs(60),
            resource_packs: Arc::new(ServerPacks::default()),
            max_players: 0,
            admission: None,
            status: None,
            status_refresh: Duration::from_secs(4),
            connection: ConnectionOptions::default(),
        }
    }
}

#[cfg(feature = "server")]
impl std::fmt::Debug for BedrockListenerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BedrockListenerConfig")
            .field("online_mode", &self.online_mode)
            .field("allow_legacy_auth", &self.allow_legacy_auth)
            .field("encryption_enabled", &self.encryption_enabled)
            .field("compression", &self.compression)
            .field("login_timeout", &self.login_timeout)
            .field("resource_pack_timeout", &self.resource_pack_timeout)
            .field("spawn_timeout", &self.spawn_timeout)
            .field("resource_packs", &self.resource_packs.len())
            .field("max_players", &self.max_players)
            .field("admission", &self.admission.is_some())
            .field("status", &self.status.is_some())
            .field("connection", &self.connection)
            .finish()
    }
}

/// Client configuration.
#[derive(Clone, Debug)]
pub struct ClientConfig {
    /// Codec options. The unknown-packet policy defaults to `Passthrough`
    /// (gophertunnel dialers do not disconnect on unknown packets).
    pub connection: ConnectionOptions,
    /// One deadline spanning every join phase. Default: 120 s.
    pub join_timeout: Duration,
    /// Radius sent in `RequestChunkRadius` after `StartGame`. Default: 16.
    pub chunk_radius: i32,
    /// Resource pack download policy and bounds.
    pub packs: ClientPackPolicy,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            connection: ConnectionOptions {
                unknown_packets: UnknownPacketPolicy::Passthrough,
                ..ConnectionOptions::default()
            },
            join_timeout: Duration::from_secs(120),
            chunk_radius: 16,
            packs: ClientPackPolicy::default(),
        }
    }
}
