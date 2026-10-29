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

/// One saved actor: its unique ID and its Bedrock actor NBT (a rooted,
/// fixed-width little-endian compound, as Bedrock stores actors).
#[derive(Debug, Clone, PartialEq)]
pub struct StoredActor {
    pub unique_id: i64,
    pub nbt: Vec<u8>,
}

/// Data stored for a chunk column.
#[derive(Debug, Clone)]
pub struct ChunkColumn {
    /// The authoritative chunk block data and sparse block-entity metadata.
    pub chunk: Chunk,
    /// Actors whose feet are in this chunk. Saving replaces the chunk's
    /// previous actor list.
    pub actors: Vec<StoredActor>,
    /// Save only: unique IDs of actors this chunk listed earlier that no longer
    /// exist anywhere (killed or despawned). Providers that store actor records
    /// apart from the chunk delete these records.
    pub removed_actors: Vec<i64>,
}

impl ChunkColumn {
    /// Create a new column with just chunk data (no actors).
    pub fn new(chunk: Chunk) -> Self {
        Self {
            chunk,
            actors: Vec::new(),
            removed_actors: Vec::new(),
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
    /// Inventory slots in hotbar/main order. Missing on pre-inventory records.
    #[serde(default)]
    pub inventory: Vec<StoredItemStack>,
    #[serde(default)]
    pub armour: [StoredItemStack; 4],
    #[serde(default)]
    pub offhand: StoredItemStack,
    #[serde(default)]
    pub cursor: StoredItemStack,
    #[serde(default)]
    pub held_slot: u8,
    /// Ender chest slots in order. Missing on records saved before ender chests.
    #[serde(default)]
    pub ender_chest: Vec<StoredItemStack>,
    #[serde(default = "default_max_health")]
    pub max_health: f32,
    #[serde(default = "default_saturation")]
    pub saturation: f32,
    #[serde(default)]
    pub exhaustion: f32,
    #[serde(default)]
    pub experience_progress: f32,
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
            inventory: Vec::new(),
            armour: Default::default(),
            offhand: Default::default(),
            cursor: Default::default(),
            held_slot: 0,
            ender_chest: Vec::new(),
            max_health: default_max_health(),
            saturation: default_saturation(),
            exhaustion: 0.0,
            experience_progress: 0.0,
        }
    }
}

fn default_max_health() -> f32 {
    20.0
}
fn default_saturation() -> f32 {
    5.0
}

/// Stable persistence representation, independent of ECS inventory internals.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct StoredItemStack {
    pub item_id: String,
    pub runtime_id: i32,
    pub count: u8,
    pub damage: i16,
    pub nbt: Option<Vec<u8>>,
}

impl From<&crate::item::ItemStack> for StoredItemStack {
    fn from(item: &crate::item::ItemStack) -> Self {
        Self {
            item_id: item.item_id().to_owned(),
            runtime_id: item.runtime_id,
            count: item.count,
            damage: item.damage,
            nbt: item.nbt.as_ref().map(|data| data.to_vec()),
        }
    }
}

impl StoredItemStack {
    /// Resolve against the frozen snapshot before creating any player state.
    /// Consumes the saved user-data buffer without copying its contents.
    pub fn restore(
        self,
        items: &crate::registry::ItemRegistry,
    ) -> StorageResult<crate::item::ItemStack> {
        if self.count == 0 || self.item_id.is_empty() {
            return Ok(crate::item::ItemStack::empty());
        }
        let item = items.item_type(&self.item_id).ok_or_else(|| {
            StorageError::Deserialization(format!("saved item {} is not registered", self.item_id))
        })?;
        if self.count > item.max_stack_size() || self.damage < 0 || self.runtime_id < 0 {
            return Err(StorageError::Deserialization(format!(
                "saved item {} has invalid stack fields",
                self.item_id
            )));
        }
        let mut stack = crate::item::ItemStack::new(item, self.count)
            .with_runtime_id(self.runtime_id)
            .with_damage(self.damage);
        if let Some(data) = self.nbt {
            stack = stack.with_nbt(data);
        }
        Ok(stack)
    }
}

impl PlayerData {
    /// Validate structural state; typed item resolution occurs atomically at spawn.
    pub fn validate(&self, uuid: Uuid) -> StorageResult<()> {
        if self.uuid != uuid.to_string()
            || !self.position.iter().all(|n| n.is_finite())
            || !self.rotation.iter().all(|n| n.is_finite())
            || !matches!(self.game_mode, 0 | 1 | 2 | 6)
            || self.inventory.len() > 36
            || self.ender_chest.len() > crate::entity::components::ENDER_CHEST_SIZE
            || self.held_slot > 8
            || !self.health.is_finite()
            || !self.max_health.is_finite()
            || self.max_health <= 0.0
            || !(0.0..=self.max_health).contains(&self.health)
            || !(0..=20).contains(&self.food)
            || self.experience < 0
            || !self.saturation.is_finite()
            || !self.exhaustion.is_finite()
            || !(0.0..=1.0).contains(&self.experience_progress)
        {
            return Err(StorageError::Deserialization("invalid player state".into()));
        }
        Ok(())
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
