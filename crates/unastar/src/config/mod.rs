//! End-user configuration loading for the Unastar server binary.
//!
//! The server reads a TOML config file (default: `unastar.toml`) and applies:
//! - Network/server settings
//! - World generator/bounds settings
//! - Spawn rules (including optional previous-position spawning)

use crate::server::ServerConfig;
use crate::world::WorldConfig;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

/// A friendly, commented default configuration file.
pub const DEFAULT_CONFIG_TOML: &str = r#"# Unastar Server Configuration (TOML)
#
# This file is intentionally verbose and friendly: most fields are optional and
# have safe defaults. Start by changing `server.bind_address` and `spawn_rules`.
#
# Notes:
# - `[[spawn_rules]]` rules are evaluated top-to-bottom. The first rule that produces
#   a spawn location wins.
# - `previous_position = true` uses `players.data_dir` to load a per-player file.
#   If a player has never joined before (or has no UUID), it falls back.

[server]
# Address to bind the Bedrock listener to.
bind_address = "0.0.0.0:19132"

# Shown in server list (if/when advertisement is implemented).
motd = "Unastar Server"

# Soft maximum: connection acceptance still depends on upstream layers.
max_players = 20

# Chunk radius we will *advertise* to clients and stream.
# (Clients may request a different radius; we clamp it by `max_chunk_radius`.)
default_chunk_radius = 4
max_chunk_radius = 12

# Authentication settings
# Xbox Live authentication. Set to false for offline/cracked clients.
online_mode = true
# Allow legacy JWT chains (self-signed/guest). Required for stress testing.
allow_legacy_auth = true
# Enable encryption handshake. Set to false for debugging.
encryption_enabled = true

[world]
# Dimension ID: 0 = Overworld, 1 = Nether, 2 = End
dimension = 0

  [world.bounds]
  # Bounds are applied to *terrain generation* (non-air). Chunks outside bounds
  # still exist as all-air so the Bedrock client can mesh borders correctly.
  #
  # modes: "infinite" | "radius" | "rect"
  mode = "infinite"
  # radius_chunks = 256
  # min_x = -64
  # max_x = 64
  # min_z = -64
  # max_z = 64

  [world.generator]
  # kinds: "flat_stone" | "void_spawn_platform"
  kind = "void_spawn_platform"
  # For void worlds, generate a stone platform at chunk coords [-r..r].
  platform_radius_chunks = 1

  [world.storage]
  # Directory for world data (relative to working dir).
  data_dir = "worlds"
  # Enable LevelDB-based chunk persistence.
  leveldb_enabled = true
  # Save modified chunks on shutdown.
  save_on_shutdown = true

[players]
# Where Unastar stores per-player files (e.g. last known position).
data_dir = \"playerdata\"

# If true, Unastar writes a player's last known position on disconnect (legacy TOML format).
save_previous_position = true

# Enable LevelDB-based player persistence (recommended).
leveldb_enabled = true

# Save player data on disconnect (requires leveldb_enabled).
save_on_disconnect = true

# Spawn rules.
#
# Common patterns:
# - Always spawn at a fixed location:
#     always_at_location = true
#     location = { x = 0.5, y = 17.0, z = 0.5, yaw = 0.0, pitch = 0.0 }
#
# - Prefer previous position, fallback to fixed spawn:
#     previous_position = true
#     always_at_location = true
#     location = { ... }

[[spawn_rules]]
name = "main"

# If true, try to place returning players at their last saved position.
previous_position = true

# If true, force a fixed spawn location (or act as a fallback if previous position is missing).
always_at_location = true

# Spawn location in *block coordinates* (floats allowed for smoother centering).
# `yaw` and `pitch` are optional.
location = { x = 0.5, y = 17.0, z = 0.5, yaw = 0.0, pitch = 0.0 }
"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UnastarConfig {
    pub server: ServerConfigFile,
    pub world: WorldConfig,
    pub players: PlayerStorageConfig,
    #[serde(alias = "spawn")]
    pub spawn_rules: Vec<SpawnRule>,
}

impl Default for UnastarConfig {
    fn default() -> Self {
        Self {
            server: ServerConfigFile::default(),
            world: WorldConfig::default(),
            players: PlayerStorageConfig::default(),
            spawn_rules: vec![SpawnRule::default()],
        }
    }
}

impl UnastarConfig {
    pub fn load_or_create(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref().to_path_buf();

        let contents = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                std::fs::write(&path, DEFAULT_CONFIG_TOML).map_err(|source| {
                    ConfigError::WriteDefault {
                        path: path.clone(),
                        source,
                    }
                })?;
                DEFAULT_CONFIG_TOML.to_string()
            }
            Err(source) => return Err(ConfigError::Read { path, source }),
        };

        let cfg: Self =
            toml_edit::de::from_str(&contents).map_err(|source| ConfigError::Parse {
                path: path.clone(),
                source,
            })?;

        cfg.validate().map_err(ConfigError::Validation)?;
        Ok(cfg)
    }

    pub fn server_config(&self) -> ServerConfig {
        let defaults = ServerConfig::default();

        // Convert gamemode string to enum
        let gamemode = match self.server.default_gamemode.to_lowercase().as_str() {
            "creative" => crate::entity::components::GameMode::Creative,
            "adventure" => crate::entity::components::GameMode::Adventure,
            "spectator" => crate::entity::components::GameMode::Spectator,
            _ => crate::entity::components::GameMode::Survival, // default
        };

        ServerConfig {
            bind_address: self.server.bind_address.clone(),
            motd: self.server.motd.clone(),
            max_players: self.server.max_players,
            default_chunk_radius: self.server.default_chunk_radius,
            max_chunk_radius: self.server.max_chunk_radius,
            default_gamemode: gamemode,
            // Use defaults for new fields not in config file yet
            simulation_distance: defaults.simulation_distance,
            chunk_unload_ticks: defaults.chunk_unload_ticks,
            world: self.world,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.spawn_rules.is_empty() {
            return Err("config requires at least one `[[spawn_rules]]` rule".into());
        }
        if self.server.default_chunk_radius < 1 {
            return Err("`server.default_chunk_radius` must be >= 1".into());
        }
        if self.server.max_chunk_radius < 1 {
            return Err("`server.max_chunk_radius` must be >= 1".into());
        }
        if self.server.default_chunk_radius > self.server.max_chunk_radius {
            return Err(
                "`server.default_chunk_radius` cannot exceed `server.max_chunk_radius`".into(),
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfigFile {
    pub bind_address: String,
    pub motd: String,
    pub max_players: u32,
    pub default_chunk_radius: i32,
    pub max_chunk_radius: i32,
    /// Default game mode for new players: "survival", "creative", "adventure", "spectator"
    pub default_gamemode: String,
    /// Enable Xbox Live authentication (online mode).
    /// Set to false for offline/cracked clients.
    pub online_mode: bool,
    /// Allow legacy/self-signed JWT chains (guest login).
    pub allow_legacy_auth: bool,
    /// Enable encryption handshake.
    pub encryption_enabled: bool,
}

impl Default for ServerConfigFile {
    fn default() -> Self {
        Self {
            bind_address: "0.0.0.0:19132".into(),
            motd: "Unastar Server".into(),
            max_players: 20,
            default_chunk_radius: 4,
            max_chunk_radius: 12,
            default_gamemode: "survival".into(),
            online_mode: true,
            allow_legacy_auth: true,
            encryption_enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayerStorageConfig {
    /// Directory for player data files.
    pub data_dir: PathBuf,
    /// Save player position on disconnect (legacy TOML files).
    pub save_previous_position: bool,
    /// Enable LevelDB-based player persistence.
    pub leveldb_enabled: bool,
    /// Save player data on disconnect (via LevelDB).
    pub save_on_disconnect: bool,
}

impl Default for PlayerStorageConfig {
    fn default() -> Self {
        Self {
            data_dir: PathBuf::from("playerdata"),
            save_previous_position: true,
            leveldb_enabled: true,
            save_on_disconnect: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpawnLocation {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub yaw: f32,
    pub pitch: f32,
}

impl Default for SpawnLocation {
    fn default() -> Self {
        Self {
            x: 0.5,
            y: 17.0,
            z: 0.5,
            yaw: 0.0,
            pitch: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SpawnRule {
    pub name: String,
    pub always_at_location: bool,
    pub previous_position: bool,
    pub location: Option<SpawnLocation>,
}

impl Default for SpawnRule {
    fn default() -> Self {
        Self {
            name: "main".into(),
            always_at_location: true,
            previous_position: true,
            location: Some(SpawnLocation::default()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlayerDataStore {
    root: PathBuf,
}

impl PlayerDataStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn file_path(&self, uuid: &str) -> PathBuf {
        self.root.join(format!("{uuid}.toml"))
    }

    pub async fn load_last_position(
        &self,
        uuid: &str,
    ) -> Result<Option<PlayerLastPosition>, PlayerDataError> {
        let path = self.file_path(uuid);

        let contents = match tokio::fs::read_to_string(&path).await {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(PlayerDataError::Read { path, source }),
        };

        let state: PlayerLastPosition =
            toml_edit::de::from_str(&contents).map_err(|source| PlayerDataError::Parse {
                path: path.clone(),
                source,
            })?;

        Ok(Some(state))
    }

    pub async fn save_last_position(
        &self,
        uuid: &str,
        state: &PlayerLastPosition,
    ) -> Result<(), PlayerDataError> {
        tokio::fs::create_dir_all(&self.root)
            .await
            .map_err(|source| PlayerDataError::CreateDir {
                path: self.root.clone(),
                source,
            })?;

        let path = self.file_path(uuid);
        let contents =
            toml_edit::ser::to_string_pretty(state).map_err(PlayerDataError::Serialize)?;

        tokio::fs::write(&path, contents)
            .await
            .map_err(|source| PlayerDataError::Write { path, source })?;

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerLastPosition {
    pub dimension: i32,
    pub location: SpawnLocation,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read config file `{path}`: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write default config file `{path}`: {source}")]
    WriteDefault {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse config file `{path}`: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml_edit::de::Error,
    },
    #[error("{0}")]
    Validation(String),
}

#[derive(Debug, Error)]
pub enum PlayerDataError {
    #[error("failed to create directory `{path}`: {source}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read `{path}`: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse `{path}`: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml_edit::de::Error,
    },
    #[error("failed to serialize player data: {0}")]
    Serialize(#[from] toml_edit::ser::Error),
    #[error("failed to write `{path}`: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}
