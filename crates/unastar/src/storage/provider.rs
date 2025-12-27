//! Provider traits for world and player persistence.

use async_trait::async_trait;
use std::io;
use uuid::Uuid;

use crate::world::{Chunk, ChunkPos};

/// Result type for storage operations.
pub type StorageResult<T> = Result<T, StorageError>;

/// Storage operation errors.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Deserialization error: {0}")]
    Deserialization(String),

    #[error("Not found")]
    NotFound,
}

/// Data stored for a chunk column.
#[derive(Debug, Clone)]
pub struct ChunkColumn {
    /// The chunk block data.
    pub chunk: Chunk,
    /// Entities in this chunk (serialized NBT).
    pub entities: Vec<Vec<u8>>,
    /// Block entities (serialized NBT).
    pub block_entities: Vec<Vec<u8>>,
}

impl ChunkColumn {
    /// Create a new column with just chunk data (no entities).
    pub fn new(chunk: Chunk) -> Self {
        Self {
            chunk,
            entities: Vec::new(),
            block_entities: Vec::new(),
        }
    }
}

/// Provider for chunk/column persistence.
///
/// Each world should have its own provider instance.
#[async_trait]
pub trait WorldProvider: Send + Sync + 'static {
    /// Load a chunk column from storage.
    ///
    /// Returns `Ok(None)` if the chunk doesn't exist (needs generation).
    async fn load_column(&self, pos: ChunkPos, dim: i32) -> StorageResult<Option<ChunkColumn>>;

    /// Save a chunk column to storage.
    async fn save_column(&self, pos: ChunkPos, dim: i32, col: &ChunkColumn) -> StorageResult<()>;

    /// Flush any pending writes to disk.
    async fn flush(&self) -> StorageResult<()>;

    /// Close the provider, flushing all data.
    async fn close(&self) -> StorageResult<()>;
}

/// Player data for persistence.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlayerData {
    /// Player UUID.
    pub uuid: String,
    /// Last position.
    pub position: [f64; 3],
    /// Last rotation (yaw, pitch).
    pub rotation: [f32; 2],
    /// Dimension ID.
    pub dimension: i32,
    /// Game mode.
    pub game_mode: u8,
    /// Health.
    pub health: f32,
    /// Hunger.
    pub food: i32,
    /// Experience level.
    pub experience: i32,
    // TODO: Inventory, effects, etc.
}

impl Default for PlayerData {
    fn default() -> Self {
        Self {
            uuid: String::new(),
            position: [0.5, 17.0, 0.5],
            rotation: [0.0, 0.0],
            dimension: 0,
            game_mode: 1, // Creative
            health: 20.0,
            food: 20,
            experience: 0,
        }
    }
}

/// Provider for player data persistence.
///
/// Shared across all worlds (player data is global).
#[async_trait]
pub trait PlayerProvider: Send + Sync + 'static {
    /// Load player data.
    ///
    /// Returns `Ok(None)` if the player doesn't exist.
    async fn load(&self, uuid: Uuid) -> StorageResult<Option<PlayerData>>;

    /// Save player data.
    async fn save(&self, uuid: Uuid, data: &PlayerData) -> StorageResult<()>;

    /// Delete player data.
    async fn delete(&self, uuid: Uuid) -> StorageResult<()>;

    /// Close the provider.
    async fn close(&self) -> StorageResult<()>;
}
