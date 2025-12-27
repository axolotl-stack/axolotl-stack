//! Chunk ECS components.

use crate::world::Chunk;
use bevy_ecs::prelude::*;

/// Chunk position component (mirrors world::ChunkPos but as ECS component).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChunkPosition {
    pub x: i32,
    pub z: i32,
}

impl ChunkPosition {
    pub fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }

    pub fn from_block(block_x: i32, block_z: i32) -> Self {
        Self {
            x: block_x.div_euclid(16),
            z: block_z.div_euclid(16),
        }
    }

    /// Convert to world::ChunkPos.
    pub fn to_chunk_pos(&self) -> crate::world::ChunkPos {
        crate::world::ChunkPos::new(self.x, self.z)
    }

    /// Get as tuple key for HashMap lookups.
    pub fn as_tuple(&self) -> (i32, i32) {
        (self.x, self.z)
    }
}

impl From<crate::world::ChunkPos> for ChunkPosition {
    fn from(pos: crate::world::ChunkPos) -> Self {
        Self { x: pos.x, z: pos.z }
    }
}

impl From<ChunkPosition> for crate::world::ChunkPos {
    fn from(pos: ChunkPosition) -> Self {
        Self::new(pos.x, pos.z)
    }
}

impl From<(i32, i32)> for ChunkPosition {
    fn from((x, z): (i32, i32)) -> Self {
        Self { x, z }
    }
}

/// Chunk block data component.
/// Contains the actual chunk data (subchunks, biomes, etc.).
#[derive(Component)]
pub struct ChunkData {
    /// The underlying chunk from the world module.
    pub inner: Chunk,
}

impl ChunkData {
    pub fn new(chunk: Chunk) -> Self {
        Self { inner: chunk }
    }
}

/// Chunk loading state.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChunkState {
    /// Chunk is not loaded (shouldn't exist as an entity in this state).
    #[default]
    Unloaded,
    /// Chunk is queued for loading/generation.
    Pending,
    /// Chunk is currently being generated (async).
    Generating,
    /// Chunk is fully loaded and ready.
    Loaded,
}

impl ChunkState {
    pub fn is_loaded(&self) -> bool {
        matches!(self, ChunkState::Loaded)
    }

    pub fn is_ready(&self) -> bool {
        matches!(self, ChunkState::Loaded)
    }
}

/// Marker component for chunks modified since last save.
/// Separate from ChunkState to allow Loaded + Modified simultaneously.
#[derive(Component, Debug)]
pub struct ChunkModified;

/// Marker component for chunks that need network re-encoding.
/// Add this when blocks are modified to trigger broadcast to viewers.
#[derive(Component, Debug)]
pub struct ChunkDirty;

/// Marker component for chunks within simulation distance.
/// These chunks receive random ticks and entity update systems.
#[derive(Component, Debug)]
pub struct ChunkTicking;

/// Marker component for chunks pending unload.
/// Set a grace period before actually despawning.
#[derive(Component, Debug)]
pub struct ChunkPendingUnload {
    pub ticks_remaining: u32,
}

impl Default for ChunkPendingUnload {
    fn default() -> Self {
        Self {
            ticks_remaining: 100, // 5 seconds grace period at 20 TPS
        }
    }
}

impl ChunkPendingUnload {
    pub fn new(ticks: u32) -> Self {
        Self {
            ticks_remaining: ticks,
        }
    }

    /// Tick down the grace period. Returns true if expired.
    pub fn tick(&mut self) -> bool {
        self.ticks_remaining = self.ticks_remaining.saturating_sub(1);
        self.ticks_remaining == 0
    }
}

/// Component tracking player sessions viewing this chunk.
///
/// A viewer is any player whose view radius includes this chunk.
/// This is different from players physically inside the chunk.
/// Used for:
/// - Broadcasting block updates
/// - Entity spawn/despawn visibility
/// - Determining when chunks can unload (no viewers = can unload)
#[derive(Component, Debug, Default)]
pub struct ChunkViewers {
    /// Player entities with this chunk in their view radius.
    pub entities: Vec<Entity>,
}

impl ChunkViewers {
    pub fn insert(&mut self, entity: Entity) {
        if !self.entities.contains(&entity) {
            self.entities.push(entity);
        }
    }

    pub fn remove(&mut self, entity: Entity) {
        self.entities.retain(|&e| e != entity);
    }

    pub fn contains(&self, entity: Entity) -> bool {
        self.entities.contains(&entity)
    }

    pub fn iter(&self) -> impl Iterator<Item = Entity> + '_ {
        self.entities.iter().copied()
    }

    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    pub fn clear(&mut self) {
        self.entities.clear();
    }
}

/// Component tracking entities currently inside this chunk.
///
/// Updated when entities cross chunk boundaries.
/// Used for:
/// - Entity save/freeze on chunk unload
/// - Spatial queries (entities within a chunk)
/// - Entity visibility broadcasts to chunk viewers
///
/// Note: This tracks entities by their physical position, not view radius.
#[derive(Component, Debug, Default)]
pub struct ChunkEntities {
    /// Non-player entities in this chunk (mobs, items, projectiles, etc.)
    pub entities: Vec<Entity>,
}

impl ChunkEntities {
    pub fn insert(&mut self, entity: Entity) {
        if !self.entities.contains(&entity) {
            self.entities.push(entity);
        }
    }

    pub fn remove(&mut self, entity: Entity) {
        self.entities.retain(|&e| e != entity);
    }

    pub fn contains(&self, entity: Entity) -> bool {
        self.entities.contains(&entity)
    }

    pub fn iter(&self) -> impl Iterator<Item = Entity> + '_ {
        self.entities.iter().copied()
    }

    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    pub fn clear(&mut self) {
        self.entities.clear();
    }

    /// Take all entities, leaving the list empty.
    pub fn take(&mut self) -> Vec<Entity> {
        std::mem::take(&mut self.entities)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_position_from_block() {
        assert_eq!(ChunkPosition::from_block(0, 0), ChunkPosition::new(0, 0));
        assert_eq!(ChunkPosition::from_block(15, 15), ChunkPosition::new(0, 0));
        assert_eq!(ChunkPosition::from_block(16, 16), ChunkPosition::new(1, 1));
        assert_eq!(
            ChunkPosition::from_block(-1, -1),
            ChunkPosition::new(-1, -1)
        );
        assert_eq!(
            ChunkPosition::from_block(-16, -16),
            ChunkPosition::new(-1, -1)
        );
        assert_eq!(
            ChunkPosition::from_block(-17, -17),
            ChunkPosition::new(-2, -2)
        );
    }

    #[test]
    fn test_chunk_pending_unload_tick() {
        let mut pending = ChunkPendingUnload::new(3);
        assert!(!pending.tick()); // 2 remaining
        assert!(!pending.tick()); // 1 remaining
        assert!(pending.tick()); // 0 remaining, expired
        assert!(pending.tick()); // stays at 0 (saturating)
    }

    #[test]
    fn test_chunk_viewers() {
        use bevy_ecs::world::World;

        let mut world = World::new();
        let e1 = world.spawn_empty().id();
        let e2 = world.spawn_empty().id();

        let mut viewers = ChunkViewers::default();

        assert!(viewers.is_empty());
        viewers.insert(e1);
        assert_eq!(viewers.len(), 1);
        viewers.insert(e1); // duplicate, no-op
        assert_eq!(viewers.len(), 1);
        viewers.insert(e2);
        assert_eq!(viewers.len(), 2);
        assert!(viewers.contains(e1));
        assert!(viewers.contains(e2));

        viewers.remove(e1);
        assert_eq!(viewers.len(), 1);
        assert!(!viewers.contains(e1));
        assert!(viewers.contains(e2));
    }

    #[test]
    fn test_chunk_entities() {
        use bevy_ecs::world::World;

        let mut world = World::new();
        let e1 = world.spawn_empty().id();
        let e2 = world.spawn_empty().id();

        let mut entities = ChunkEntities::default();

        entities.insert(e1);
        entities.insert(e2);
        assert_eq!(entities.len(), 2);

        let taken = entities.take();
        assert_eq!(taken.len(), 2);
        assert!(entities.is_empty());
    }
}
