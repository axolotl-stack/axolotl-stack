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
    /// Default chunk radius (in chunks) used until the client requests a different value.
    /// This is the view/render distance for clients.
    pub default_chunk_radius: i32,
    /// Maximum chunk radius (in chunks) that the server will accept from clients.
    pub max_chunk_radius: i32,
    /// Default game mode for new players.
    pub default_gamemode: GameMode,
    /// Simulation distance in chunks.
    /// Chunks within this range of ANY player get random ticks and entity updates.
    /// Should typically be 1-2 larger than default_chunk_radius to avoid
    /// edge-of-view pop-in for world updates.
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
            bind_address: "0.0.0.0:19132".into(),
            motd: "Unastar Server".into(),
            max_players: 20,
            default_chunk_radius: 4,
            max_chunk_radius: 12,
            default_gamemode: GameMode::Survival,
            simulation_distance: 6,  // 2 more than default view
            chunk_unload_ticks: 100, // 5 second grace period
            world: WorldConfig::default(),
        }
    }
}
