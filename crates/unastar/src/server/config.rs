//! Server configuration.

use crate::entity::components::GameMode;
use crate::world::WorldConfig;
use serde::{Deserialize, Serialize};

/// How survival block breaking treats a client's inputs (`[gameplay]
/// block_breaking`). BDS 1.26.51 adds one input's worth of progress for
/// every PlayerAuthInput, however many arrive in a tick, and keeps the
/// progress built at a position while the block there changes; both let a
/// modified client break blocks faster than a vanilla one
/// (docs/decisions/0056-guarded-block-breaking.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BlockBreaking {
    /// BDS's per-input breaking, but at most one input of progress per
    /// server tick since the progress began, and the progress begins again
    /// when the block at the position is no longer the one being broken.
    #[default]
    Guarded,
    /// Exactly what BDS 1.26.51 does.
    Bds,
}

impl BlockBreaking {
    /// Whether the anticheat guard is on.
    pub fn is_guarded(self) -> bool {
        self == Self::Guarded
    }
}

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
    /// World difficulty at startup.
    pub difficulty: crate::world::rules::Difficulty,
    /// Block state numbering on the wire (`StartGame.block_network_ids_are_hashes`).
    pub block_network_ids: crate::registry::block::BlockNetworkIds,
    /// Survival block breaking: guarded (default) or exactly BDS.
    pub block_breaking: BlockBreaking,
    /// Players given operator permission on join: display names
    /// (ASCII case-insensitive) or XUIDs.
    pub operators: Vec<String>,
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
            bind_address: "127.0.0.1:19132".into(),
            motd: "Unastar Server".into(),
            max_players: 20,
            default_chunk_radius: 4,
            max_chunk_radius: 12,
            default_gamemode: GameMode::Survival,
            difficulty: crate::world::rules::Difficulty::Easy,
            block_network_ids: crate::registry::block::BlockNetworkIds::Hashed,
            block_breaking: BlockBreaking::Guarded,
            operators: Vec::new(),
            simulation_distance: 6,  // 2 more than default view
            chunk_unload_ticks: 100, // 5 second grace period
            world: WorldConfig::default(),
        }
    }
}
