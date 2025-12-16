use std::marker::PhantomData;
use std::sync::Arc;

use crate::config::BedrockListenerConfig;
use crate::error::JolyneError;
use crate::protocol::McpePacket;
use tokio_raknet::protocol::reliability::Reliability;
use transport::BedrockTransport;

pub mod transport;

#[cfg(feature = "client")]
pub mod client;
#[cfg(feature = "server")]
pub mod server;

/// A strongly-typed, state-aware Bedrock protocol stream.
///
/// This struct enforces protocol correctness at compile time using the Typestate pattern.
/// The `S` parameter represents the current protocol state (e.g., `Login`, `Play`),
/// and the `R` parameter represents the connection role (`Client` or `Server`).
pub struct BedrockStream<S: State, R: Role> {
    pub(crate) transport: BedrockTransport,
    pub(crate) state: S,
    pub(crate) _role: PhantomData<R>,
}

impl<S: State, R: Role> BedrockStream<S, R> {
    pub fn peer_addr(&self) -> std::net::SocketAddr {
        self.transport.peer_addr()
    }

    /// Consumes the stream and returns the underlying transport.
    /// This allows bypassing the state machine for proxying or raw access.
    pub fn into_transport(self) -> BedrockTransport {
        self.transport
    }

    /// Configures the flushing strategy.
    ///
    /// - `true` (Default): `send()` sends packets immediately (low latency, high overhead).
    /// - `false`: `send()` queues packets. You MUST call `flush()` to send them (high throughput).
    pub fn set_auto_flush(&mut self, auto: bool) {
        self.transport.set_auto_flush(auto);
    }

    /// Flushes all buffered packets as a single batch (ReliableOrdered).
    /// Does nothing if the buffer is empty.
    pub async fn flush(&mut self) -> Result<(), JolyneError> {
        self.transport.flush().await
    }

    /// Sends a list of packets as a single batch with specified reliability.
    ///
    /// This bypasses the internal `write_buffer` and sends immediately.
    /// Useful for streaming data (e.g. video/maps) that should use `Unreliable` or `ReliableSequenced`.
    pub async fn send_batch_with_reliability(
        &mut self,
        packets: &[McpePacket],
        reliability: Reliability,
    ) -> Result<(), JolyneError> {
        self.transport
            .send_batch_with_reliability(packets, reliability)
            .await
    }
}

/// Marker trait for protocol states.
pub trait State {}

// --- Granular Handshake States ---

/// Initial state: Connected, waiting for RequestNetworkSettings.
pub struct Handshake {
    pub config: Option<Arc<BedrockListenerConfig>>,
}
impl State for Handshake {}

/// State: Network settings agreed, waiting for Login packet.
pub struct Login {
    pub config: Option<Arc<BedrockListenerConfig>>,
}
impl State for Login {}

/// State: Authenticated, negotiating encryption (ServerToClient/ClientToServer).
pub struct SecurePending {
    pub config: Option<Arc<BedrockListenerConfig>>,
}
impl State for SecurePending {}

/// State: Negotiating resource packs (ResourcePacksInfo, ResourcePackStack).
pub struct ResourcePacks;
impl State for ResourcePacks {}

/// State: Connection is initialized, waiting for StartGame packet/processing.
pub struct StartGame;
impl State for StartGame {}

// --- Main Game State ---

/// Final State: Fully authenticated, in-game. Ready to exchange game packets.
pub struct Play;
impl State for Play {}

// --- Roles ---

/// Marker trait for connection roles.
pub trait Role {}

/// Marker for a Server connection.
pub struct Server;
impl Role for Server {}

/// Marker for a Client connection.
pub struct Client;
impl Role for Client {}

// --- User-Friendly Type Aliases ---

/// Entry point for Server connection.
pub type ServerLogin = BedrockStream<Handshake, Server>;
pub type ServerSecurePending = BedrockStream<SecurePending, Server>;
pub type ServerResourcePacks = BedrockStream<ResourcePacks, Server>;
pub type ServerStartGame = BedrockStream<StartGame, Server>;
pub type ServerPlay = BedrockStream<Play, Server>;

/// Entry point for Client connection.
pub type ClientLogin = BedrockStream<Handshake, Client>;
pub type ClientSecurePending = BedrockStream<SecurePending, Client>;
pub type ClientResourcePacks = BedrockStream<ResourcePacks, Client>;
pub type ClientStartGame = BedrockStream<StartGame, Client>;
pub type ClientPlay = BedrockStream<Play, Client>;
