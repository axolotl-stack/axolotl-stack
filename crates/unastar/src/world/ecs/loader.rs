//! Per-player chunk loader component.
//!
//! Manages which chunks are loaded for a player, including:
//! - Center-outward load ordering for better visual experience
//! - Efficient eviction of out-of-range chunks when moving
//! - Integration with chunk viewer tracking
//!
//! ## Component Hook
//!
//! `ChunkLoader` implements a custom `on_remove` hook that automatically
//! cleans up chunk viewers when the component is removed. This ensures
//! that when a player entity is despawned, they are removed from all
//! `ChunkViewers` components without needing a separate cleanup system.

use bevy_ecs::component::{Mutable, StorageType};
use bevy_ecs::lifecycle::{ComponentHook, HookContext};
use bevy_ecs::prelude::*;
use bevy_ecs::world::DeferredWorld;
use std::collections::HashSet;

use crate::world::ecs::{ChunkManager, ChunkViewers};

/// Per-player chunk loader component.
///
/// This component manages chunk loading for a single player, providing:
/// - A load queue ordered from center outward (closest chunks load first)
/// - Tracking of which chunks are currently loaded for this viewer
/// - Efficient updates when the player moves or radius changes
///
/// This replaces the simpler `SentChunks` with more sophisticated ordering.
///
/// ## Automatic Cleanup
///
/// When this component is removed (e.g., player despawn), an `on_remove` hook
/// automatically removes the player from all `ChunkViewers` they were viewing.
/// This eliminates the need for a separate cleanup system or marker component.
#[derive(Debug)]
pub struct ChunkLoader {
    /// Current chunk position (center of view).
    position: (i32, i32),
    /// View radius in chunks.
    radius: i32,
    /// Queue of chunks to load, ordered center-outward.
    /// Pop from back for next chunk to load.
    load_queue: Vec<(i32, i32)>,
    /// Set of chunks currently loaded for this viewer.
    loaded: HashSet<(i32, i32)>,
    /// Internal counter for tracking chunks sent in the current tick.
    pub(super) sent_this_tick: usize,
}

impl Default for ChunkLoader {
    fn default() -> Self {
        Self::new(8)
    }
}

impl ChunkLoader {
    /// Create a new chunk loader with the given radius.
    pub fn new(radius: i32) -> Self {
        Self {
            position: (0, 0),
            radius: radius.max(1),
            load_queue: Vec::new(),
            loaded: HashSet::new(),
            sent_this_tick: 0,
        }
    }

    /// Get current position.
    pub fn position(&self) -> (i32, i32) {
        self.position
    }

    /// Get current radius.
    pub fn radius(&self) -> i32 {
        self.radius
    }

    /// Check if a chunk is currently loaded for this viewer.
    pub fn is_loaded(&self, x: i32, z: i32) -> bool {
        self.loaded.contains(&(x, z))
    }

    /// Get the set of loaded chunks.
    pub fn loaded_chunks(&self) -> &HashSet<(i32, i32)> {
        &self.loaded
    }

    /// Get the number of loaded chunks.
    pub fn loaded_count(&self) -> usize {
        self.loaded.len()
    }

    /// Get the number of chunks in the load queue.
    pub fn queue_len(&self) -> usize {
        self.load_queue.len()
    }

    /// Check if there are chunks waiting to be loaded.
    pub fn has_pending(&self) -> bool {
        !self.load_queue.is_empty()
    }

    /// Mark a chunk as loaded.
    pub fn mark_loaded(&mut self, x: i32, z: i32) {
        self.loaded.insert((x, z));
    }

    /// Pop the next chunk to load from the queue, if any.
    /// Returns chunks in center-outward order.
    pub fn next_to_load(&mut self) -> Option<(i32, i32)> {
        self.load_queue.pop()
    }

    /// Peek at the next chunk to load without removing it from the queue.
    /// Returns chunks in center-outward order.
    pub fn peek_next_to_load(&self) -> Option<(i32, i32)> {
        self.load_queue.last().copied()
    }

    /// Re-queues a chunk at the front of the queue to be retried next.
    pub(super) fn requeue_front(&mut self, x: i32, z: i32) {
        // Avoid duplicates if it's somehow still in the queue
        if !self.load_queue.contains(&(x, z)) {
            self.load_queue.push((x, z));
        }
    }

    /// Update position, evicting out-of-range chunks and repopulating queue.
    /// Returns chunks that were evicted (no longer in range).
    pub fn move_to(&mut self, x: i32, z: i32) -> Vec<(i32, i32)> {
        if self.position == (x, z) {
            return Vec::new();
        }

        self.position = (x, z);
        self.rebuild_queue_and_evict()
    }

    /// Change radius, evicting out-of-range chunks and repopulating queue.
    /// Returns chunks that were evicted.
    pub fn set_radius(&mut self, radius: i32) -> Vec<(i32, i32)> {
        let new_radius = radius.max(1);
        if self.radius == new_radius {
            return Vec::new();
        }

        self.radius = new_radius;
        self.rebuild_queue_and_evict()
    }

    /// Rebuild the load queue and evict out-of-range chunks.
    /// Returns evicted chunk positions.
    fn rebuild_queue_and_evict(&mut self) -> Vec<(i32, i32)> {
        let (cx, cz) = self.position;
        let r = self.radius;

        // Evict chunks outside the new view
        let mut evicted = Vec::new();
        self.loaded.retain(|&(lx, lz)| {
            let in_range = Self::is_in_range_circular(cx, cz, lx, lz, r);
            if !in_range {
                evicted.push((lx, lz));
            }
            in_range
        });

        // Build list of chunks that need loading (in range but not loaded)
        let mut to_load = Vec::new();
        for dx in -r..=r {
            for dz in -r..=r {
                let dist_sq = dx * dx + dz * dz;
                let radius_sq = r * r;

                // Only load chunks within circular radius
                if dist_sq <= radius_sq {
                    let chunk_x = cx + dx;
                    let chunk_z = cz + dz;
                    if !self.loaded.contains(&(chunk_x, chunk_z)) {
                        to_load.push((dist_sq, chunk_x, chunk_z));
                    }
                }
            }
        }

        // Sort by distance (descending so pop() gives closest first)
        to_load.sort_by(|a, b| b.0.cmp(&a.0));

        // Update queue
        self.load_queue = to_load.into_iter().map(|(_, x, z)| (x, z)).collect();

        evicted
    }

    /// Check if a chunk position is within circular range of the center.
    #[inline]
    fn is_in_range_circular(cx: i32, cz: i32, x: i32, z: i32, radius: i32) -> bool {
        let dx = x - cx;
        let dz = z - cz;
        let dist_sq = dx * dx + dz * dz;
        let radius_sq = radius * radius;
        dist_sq <= radius_sq
    }

    /// Check if a chunk position is within range of the center.
    #[inline]
    fn is_in_range(cx: i32, cz: i32, x: i32, z: i32, radius: i32) -> bool {
        let dx = x - cx;
        let dz = z - cz;
        dx.abs() <= radius && dz.abs() <= radius
    }

    /// Get all chunks that should be in view for the current position/radius.
    /// Useful for initialization or debugging.
    pub fn desired_chunks(&self) -> Vec<(i32, i32)> {
        let (cx, cz) = self.position;
        let r = self.radius;
        let mut chunks = Vec::with_capacity(((r * 2 + 1) * (r * 2 + 1)) as usize);

        for dx in -r..=r {
            for dz in -r..=r {
                chunks.push((cx + dx, cz + dz));
            }
        }

        chunks
    }

    /// Force reload - clears loaded set and rebuilds queue.
    /// Returns all previously loaded chunks as evicted.
    pub fn force_reload(&mut self) -> Vec<(i32, i32)> {
        let evicted: Vec<_> = self.loaded.drain().collect();
        self.rebuild_queue_and_evict();
        evicted
    }
}

// Manual Component implementation with on_remove hook for automatic cleanup.
impl Component for ChunkLoader {
    const STORAGE_TYPE: StorageType = StorageType::Table;
    type Mutability = Mutable;

    fn on_remove() -> Option<ComponentHook> {
        Some(|mut world: DeferredWorld<'_>, context: HookContext| {
            let player_entity = context.entity;

            // Collect the loaded chunks before accessing other components
            let loaded_chunks: Vec<(i32, i32)> = world
                .get::<ChunkLoader>(player_entity)
                .map(|loader| loader.loaded.iter().copied().collect())
                .unwrap_or_default();

            if loaded_chunks.is_empty() {
                return;
            }

            // Get chunk entities from ChunkManager
            let chunk_entities: Vec<bevy_ecs::entity::Entity> = {
                let Some(chunk_manager) = world.get_resource::<ChunkManager>() else {
                    tracing::warn!(
                        player = ?player_entity,
                        "ChunkManager not available during ChunkLoader cleanup"
                    );
                    return;
                };

                loaded_chunks
                    .iter()
                    .filter_map(|(cx, cz)| chunk_manager.get_by_coords(*cx, *cz))
                    .collect()
            };

            // Remove player from each chunk's viewers
            let mut cleaned = 0;
            for chunk_entity in chunk_entities {
                if let Some(mut viewers) = world.get_mut::<ChunkViewers>(chunk_entity) {
                    if viewers.remove(player_entity) {
                        cleaned += 1;
                    }
                }
            }

            tracing::trace!(
                player = ?player_entity,
                chunks_cleaned = cleaned,
                "ChunkLoader on_remove: cleaned up chunk viewers"
            );
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_loader_new() {
        let loader = ChunkLoader::new(4);
        assert_eq!(loader.radius(), 4);
        assert_eq!(loader.position(), (0, 0));
        assert!(loader.loaded_count() == 0);
    }

    #[test]
    fn test_chunk_loader_move_to_initial() {
        let mut loader = ChunkLoader::new(2);
        // Loader starts at (0,0), so move to a different position
        let evicted = loader.move_to(5, 5);
        assert!(evicted.is_empty()); // no prior loaded chunks

        // Queue should have all chunks in radius
        // 2*2+1 = 5 per axis, 25 total
        assert_eq!(loader.queue_len(), 25);
    }

    #[test]
    fn test_chunk_loader_center_outward_ordering() {
        let mut loader = ChunkLoader::new(2);
        // Use force_reload to build initial queue at current position (0,0)
        loader.force_reload();

        // First chunk should be center (0, 0)
        let first = loader.next_to_load();
        assert_eq!(first, Some((0, 0)));

        // Next chunks should be immediate neighbors (distance 1)
        let second = loader.next_to_load();
        if let Some((x, z)) = second {
            let dist_sq = x * x + z * z;
            assert!(dist_sq <= 1, "Second chunk should be immediate neighbor");
        }
    }

    #[test]
    fn test_chunk_loader_eviction() {
        let mut loader = ChunkLoader::new(1);
        // Build queue at (0,0)
        loader.force_reload();

        // Load all chunks
        while let Some((x, z)) = loader.next_to_load() {
            loader.mark_loaded(x, z);
        }
        assert_eq!(loader.loaded_count(), 9); // 3x3

        // Move to (10, 10) - all chunks should be evicted
        let evicted = loader.move_to(10, 10);
        assert_eq!(evicted.len(), 9);
        assert_eq!(loader.loaded_count(), 0);
    }

    #[test]
    fn test_chunk_loader_partial_move() {
        let mut loader = ChunkLoader::new(1);
        // Build queue at (0,0)
        loader.force_reload();

        // Load all chunks
        while let Some((x, z)) = loader.next_to_load() {
            loader.mark_loaded(x, z);
        }

        // Move by 1 chunk - some should remain, some evicted
        let evicted = loader.move_to(1, 0);

        // Old (-1, *) chunks are now out of range
        assert!(!evicted.is_empty());
        // Some chunks should still be loaded
        assert!(loader.loaded_count() > 0);
        // New chunks should be in queue
        assert!(loader.queue_len() > 0);
    }

    #[test]
    fn test_chunk_loader_radius_change() {
        let mut loader = ChunkLoader::new(2);
        // Build queue at (0,0)
        loader.force_reload();

        // Load all chunks
        while let Some((x, z)) = loader.next_to_load() {
            loader.mark_loaded(x, z);
        }
        assert_eq!(loader.loaded_count(), 25); // 5x5

        // Reduce radius
        let evicted = loader.set_radius(1);

        // Should evict outer ring
        assert!(!evicted.is_empty());
        assert_eq!(loader.loaded_count(), 9); // 3x3
    }
}
