//! Chunk ECS components.

use crate::world::Chunk;
use bevy_ecs::prelude::*;
use bitflags::bitflags;
use std::collections::HashSet;
use tokio::sync::oneshot;

bitflags! {
    /// Chunk state flags for frequently-toggled state.
    /// Using bitflags instead of marker components avoids archetype thrashing.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct ChunkFlags: u8 {
        /// Chunk is within simulation distance of at least one player.
        /// Receives random ticks and entity updates.
        const TICKING = 1 << 0;

        /// Chunk has been modified since last save.
        /// Used by persistence system to know what needs saving.
        const DIRTY = 1 << 1;

        /// Chunk block data has changed and needs network re-encoding.
        /// Used by broadcast system to re-encode and send to viewers.
        const NEEDS_REBROADCAST = 1 << 2;
    }
}

/// Component holding chunk state flags.
/// Replaces marker components ChunkTicking, ChunkModified, ChunkDirty
/// to avoid archetype thrashing from frequent insert/remove.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct ChunkStateFlags {
    pub flags: ChunkFlags,
}

impl ChunkStateFlags {
    pub fn is_ticking(&self) -> bool {
        self.flags.contains(ChunkFlags::TICKING)
    }

    pub fn set_ticking(&mut self, ticking: bool) {
        self.flags.set(ChunkFlags::TICKING, ticking);
    }

    pub fn is_dirty(&self) -> bool {
        self.flags.contains(ChunkFlags::DIRTY)
    }

    pub fn mark_dirty(&mut self) {
        self.flags.insert(ChunkFlags::DIRTY);
    }

    pub fn clear_dirty(&mut self) {
        self.flags.remove(ChunkFlags::DIRTY);
    }

    pub fn needs_rebroadcast(&self) -> bool {
        self.flags.contains(ChunkFlags::NEEDS_REBROADCAST)
    }

    pub fn mark_needs_rebroadcast(&mut self) {
        self.flags.insert(ChunkFlags::NEEDS_REBROADCAST);
    }

    pub fn clear_needs_rebroadcast(&mut self) {
        self.flags.remove(ChunkFlags::NEEDS_REBROADCAST);
    }
}

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

// NOTE: ChunkModified, ChunkDirty, and ChunkTicking marker components have been
// replaced by ChunkStateFlags bitflags to avoid archetype thrashing.
// See ChunkStateFlags at the top of this file.

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
///
/// Uses HashSet for O(1) insert/remove/contains operations.
#[derive(Component, Debug, Default)]
pub struct ChunkViewers {
    /// Player entities with this chunk in their view radius.
    entities: HashSet<Entity>,
}

impl ChunkViewers {
    /// Insert a viewer. Returns true if the entity was newly inserted.
    pub fn insert(&mut self, entity: Entity) -> bool {
        self.entities.insert(entity)
    }

    /// Remove a viewer. Returns true if the entity was present.
    pub fn remove(&mut self, entity: Entity) -> bool {
        self.entities.remove(&entity)
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

// =============================================================================
// Async Chunk Generation Resources (Phase 1 performance optimization)
// =============================================================================

/// A single pending chunk generation request.
pub struct PendingGeneration {
    /// Chunk X coordinate.
    pub x: i32,
    /// Chunk Z coordinate.
    pub z: i32,
    /// Receiver for the generated chunk data.
    pub receiver: oneshot::Receiver<Chunk>,
}

/// Resource tracking pending async chunk generation results.
///
/// The async generation worker sends chunks via oneshot channels.
/// This resource collects those receivers so systems can poll for
/// completed generations without blocking.
#[derive(Resource, Default)]
pub struct PendingChunkGenerations {
    /// List of pending generation receivers to poll.
    pub pending: Vec<PendingGeneration>,
}

impl PendingChunkGenerations {
    /// Add a new pending generation.
    pub fn add(&mut self, x: i32, z: i32, receiver: oneshot::Receiver<Chunk>) {
        self.pending.push(PendingGeneration { x, z, receiver });
    }

    /// Get the count of pending generations.
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// Check if there are no pending generations.
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

/// Helper struct to create ChunkStateFlags with the appropriate initial state.
impl ChunkStateFlags {
    /// Create state flags for a newly generated chunk (dirty, not ticking).
    pub fn new_generated() -> Self {
        let mut flags = Self::default();
        flags.mark_dirty();
        flags
    }
}

// =============================================================================
// Chunk Ticking State (Phase 2 performance optimization)
// =============================================================================

/// Resource for reusable chunk ticking calculation.
///
/// Eliminates per-tick HashSet allocation (~57KB per tick with many players)
/// by reusing a persistent HashSet that is cleared and refilled each tick.
///
/// ## Performance Note
/// - Old: `let should_tick = HashSet::new()` every tick → allocates ~57KB
/// - New: `ticking_state.should_tick.clear()` → zero allocation
#[derive(Resource, Default)]
pub struct ChunkTickingState {
    /// Reusable set of chunk coordinates that should be ticking.
    /// Cleared at start of each tick, filled with chunks in simulation distance.
    pub should_tick: HashSet<(i32, i32)>,
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
        assert!(viewers.insert(e1)); // returns true for new insert
        assert_eq!(viewers.len(), 1);
        assert!(!viewers.insert(e1)); // returns false for duplicate
        assert_eq!(viewers.len(), 1);
        assert!(viewers.insert(e2));
        assert_eq!(viewers.len(), 2);
        assert!(viewers.contains(e1));
        assert!(viewers.contains(e2));

        assert!(viewers.remove(e1)); // returns true when present
        assert_eq!(viewers.len(), 1);
        assert!(!viewers.contains(e1));
        assert!(viewers.contains(e2));
        assert!(!viewers.remove(e1)); // returns false when not present
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
