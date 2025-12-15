use std::marker::PhantomData;
use std::sync::Arc;

use crate::config::BedrockListenerConfig;
use crate::protocol::types::{
    block::BlockCoordinates,
    game::GameMode,
    vec::{Vec2F, Vec3F},
};
use transport::BedrockTransport;

pub mod client;
pub mod server;
pub mod transport;

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

/// Configuration for the StartGame packet.
#[derive(Debug, Clone)]
pub struct StartGameConfig {
    pub entity_id: i64,
    pub runtime_entity_id: i64,
    pub spawn_position: BlockCoordinates,
    pub player_position: Vec3F,
    pub rotation: Vec2F,
    pub world_name: String,
    pub level_id: String,
    pub world_identifier: String,
    pub game_version: String,
    pub seed: u64,
    pub generator: i32,
    pub dimension: crate::protocol::packets::start::PacketStartGameDimension,
    pub player_gamemode: GameMode,
    pub world_gamemode: GameMode,
    pub difficulty: i32,
    pub server_authoritative_inventory: bool,
    pub server_authoritative_block_breaking: bool,
    pub block_network_ids_are_hashes: bool,
    pub block_palette_checksum: u64,
}

impl Default for StartGameConfig {
    fn default() -> Self {
        Self {
            entity_id: 1,
            runtime_entity_id: 1,
            spawn_position: BlockCoordinates { x: 0, y: 64, z: 0 },
            player_position: Vec3F {
                x: 0.5,
                y: 65.0,
                z: 0.5,
            },
            rotation: Vec2F { x: 0.0, z: 0.0 },
            world_name: "world".to_string(),
            level_id: "world".to_string(),
            world_identifier: "world".to_string(),
            game_version: crate::protocol::GAME_VERSION.to_string(),
            seed: 0,
            generator: 1,
            dimension: crate::protocol::packets::start::PacketStartGameDimension::Overworld,
            player_gamemode: GameMode::Survival,
            world_gamemode: GameMode::Survival,
            difficulty: 1,
            server_authoritative_inventory: false,
            server_authoritative_block_breaking: false,
            block_network_ids_are_hashes: false,
            block_palette_checksum: 0,
        }
    }
}

impl<S: State, R: Role> BedrockStream<S, R> {
    pub fn peer_addr(&self) -> std::net::SocketAddr {
        self.transport.peer_addr()
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
pub type ServerPlay = BedrockStream<Play, Server>;

/// Entry point for Client connection.
pub type ClientLogin = BedrockStream<Handshake, Client>;
pub type ClientPlay = BedrockStream<Play, Client>;
