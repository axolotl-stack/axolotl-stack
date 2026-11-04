//! Server configuration.

use crate::entity::components::GameMode;
use crate::world::WorldConfig;

/// Server configuration options.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Bind address (e.g., "0.0.0.0:19132").
    pub bind_address: String,
    /// Server name shown in server list.
    pub motd: String,
    /// Maximum players allowed.
    pub max_players: u32,
    /// Largest chunk radius granted: a client receives the view distance it
    /// requests, up to this.
    pub max_chunk_radius: i32,
    /// Default game mode for new players.
    pub default_gamemode: GameMode,
    /// World difficulty at startup.
    pub difficulty: crate::world::rules::Difficulty,
    /// Block state numbering on the wire (`StartGame.block_network_ids_are_hashes`).
    pub block_network_ids: crate::registry::block::BlockNetworkIds,
    /// Players given operator permission on join: display names
    /// (ASCII case-insensitive) or XUIDs.
    pub operators: Vec<String>,
    /// Game rules set at startup (name, value), on top of BDS's defaults;
    /// validated by `UnastarConfig::validate`.
    pub game_rules: Vec<(String, unastar_data::game_rules::GameRuleValue)>,
    /// Simulation distance in chunks.
    /// Chunks within this range of ANY player get random ticks and entity updates.
    /// Independent of the view distance a client is granted.
    pub simulation_distance: i32,
    /// Number of ticks to wait before unloading a chunk with no viewers.
    /// Default is 100 (5 seconds at 20 TPS).
    pub chunk_unload_ticks: u32,
    /// World configuration (generator, bounds, dimension).
    pub world: WorldConfig,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_address: "127.0.0.1:19132".into(),
            motd: "Unastar Server".into(),
            max_players: 20,
            max_chunk_radius: 16,
            default_gamemode: GameMode::Survival,
            difficulty: crate::world::rules::Difficulty::Easy,
            block_network_ids: crate::registry::block::BlockNetworkIds::Hashed,
            operators: Vec::new(),
            game_rules: Vec::new(),
            simulation_distance: crate::world::ecs::systems::DEFAULT_SIMULATION_DISTANCE,
            chunk_unload_ticks: 100, // 5 second grace period
            world: WorldConfig::default(),
        }
    }
}
