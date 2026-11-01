//! Per-player chunk loader component.
//!
//! Manages which chunks are loaded for a player, including:
//! - Load ordering: nearest first, with look-ahead in the direction the
//!   player moves
//! - Efficient eviction of out-of-range chunks when moving
//! - The player's [`SendWindow`] of columns sent but not yet acknowledged
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

use crate::world::ecs::streaming::SendWindow;
use crate::world::ecs::{ChunkManager, ChunkViewers};

/// How far ahead (in ticks of the current velocity) the load order looks.
const LOOKAHEAD_TICKS: f32 = 40.0;
/// How far ahead (in ticks of the current velocity) columns are loaded before
/// they enter the view: two seconds. Measured with eight players flying apart
/// at 30 blocks/s, three seconds removed no more holes, and a shorter horizon
/// wastes less when a player turns. Prefetched columns enter the view well
/// within the unload grace of columns nobody views (5 s).
const PREFETCH_TICKS: f32 = 40.0;
/// Weight of the newest movement sample in the velocity estimate.
const VELOCITY_SMOOTHING: f32 = 0.25;
/// A move longer than this in one tick is a teleport, not motion.
const TELEPORT_STEP: f32 = 8.0;

/// Per-player chunk loader component.
///
/// This component manages chunk loading for a single player, providing:
/// - A load queue in priority order (see [`ChunkLoader::priority`])
/// - Tracking of which chunks have been sent to this viewer
/// - The send window that paces terrain to the client
/// - Efficient updates when the player moves or radius changes
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
    /// Chunks of the view not sent yet, highest priority first.
    load_queue: Vec<(i32, i32)>,
    /// Chunks sent to this viewer.
    loaded: HashSet<(i32, i32)>,
    /// Columns sent and not yet acknowledged.
    window: SendWindow,
    /// Last observed position (blocks) and smoothed velocity (blocks/tick).
    last_block_position: Option<(f32, f32)>,
    velocity: (f32, f32),
}

impl Default for ChunkLoader {
    fn default() -> Self {
        Self::new(8)
    }
}

impl ChunkLoader {
    /// Create a new chunk loader with the given radius, centered on chunk
    /// (0, 0) with an empty queue.
    pub fn new(radius: i32) -> Self {
        Self {
            position: (0, 0),
            radius: radius.max(1),
            load_queue: Vec::new(),
            loaded: HashSet::new(),
            window: SendWindow::default(),
            last_block_position: None,
            velocity: (0.0, 0.0),
        }
    }

    /// A loader centered on `chunk` with its whole view queued.
    pub fn new_at(radius: i32, chunk: (i32, i32)) -> Self {
        let mut loader = Self::new(radius);
        loader.position = chunk;
        loader.rebuild_queue_and_evict();
        loader
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

    /// Chunks of the view not sent yet, highest priority first.
    pub fn queue(&self) -> &[(i32, i32)] {
        &self.load_queue
    }

    /// Whether `pos` is still waiting to be sent.
    pub fn wants(&self, pos: (i32, i32)) -> bool {
        self.load_queue.contains(&pos)
    }

    /// Drops queued chunks for which `keep` is false (sent, or failed to load).
    pub(super) fn retain_queue(&mut self, keep: impl FnMut(&(i32, i32)) -> bool) {
        self.load_queue.retain(keep);
    }

    /// Mark a chunk as loaded.
    pub fn mark_loaded(&mut self, x: i32, z: i32) {
        self.loaded.insert((x, z));
    }

    pub fn window(&self) -> &SendWindow {
        &self.window
    }

    pub fn window_mut(&mut self) -> &mut SendWindow {
        &mut self.window
    }

    /// Feeds the player's position (blocks) once per tick to estimate the
    /// movement the load order looks ahead along.
    pub fn observe_position(&mut self, x: f64, z: f64) {
        let now = (x as f32, z as f32);
        if let Some(last) = self.last_block_position.replace(now) {
            let step = (now.0 - last.0, now.1 - last.1);
            if step.0.hypot(step.1) > TELEPORT_STEP {
                self.velocity = (0.0, 0.0);
            } else {
                self.velocity = (
                    self.velocity.0 + (step.0 - self.velocity.0) * VELOCITY_SMOOTHING,
                    self.velocity.1 + (step.1 - self.velocity.1) * VELOCITY_SMOOTHING,
                );
            }
        }
    }

    /// Where the load order looks ahead to, in chunks from the view center:
    /// [`LOOKAHEAD_TICKS`] of the current velocity, at most half the radius.
    pub fn lookahead(&self) -> (f32, f32) {
        let ahead = (
            self.velocity.0 * LOOKAHEAD_TICKS / 16.0,
            self.velocity.1 * LOOKAHEAD_TICKS / 16.0,
        );
        let length = ahead.0.hypot(ahead.1);
        let max = self.radius as f32 / 2.0;
        if length > max {
            (ahead.0 * max / length, ahead.1 * max / length)
        } else {
            ahead
        }
    }

    /// Load priority of the chunk at `(dx, dz)` from the view center (lower
    /// loads first): the squared distance to the player, or to the point the
    /// player is heading for, whichever is nearer; ties go to the chunk nearer
    /// that point. Standing still this is plain nearest first; moving, the
    /// terrain ahead comes before the terrain behind.
    pub fn priority(dx: i32, dz: i32, lookahead: (f32, f32)) -> (f32, f32) {
        let (dx, dz) = (dx as f32, dz as f32);
        let here = dx * dx + dz * dz;
        let ahead = (dx - lookahead.0).powi(2) + (dz - lookahead.1).powi(2);
        (here.min(ahead), ahead)
    }

    /// Columns to load before they enter the view: the view around the point
    /// the player reaches in [`PREFETCH_TICKS`] at its current velocity,
    /// minus the current view, nearest to the player first (roughly the order
    /// they enter the view in). Empty while standing still. These are only
    /// loaded, never sent, until they are in the view.
    pub fn prefetch(&self) -> Vec<(i32, i32)> {
        let ahead = (
            self.velocity.0 * PREFETCH_TICKS / 16.0,
            self.velocity.1 * PREFETCH_TICKS / 16.0,
        );
        if ahead.0.hypot(ahead.1) < 0.5 {
            return Vec::new();
        }
        let (cx, cz) = self.position;
        let reach = self.radius + 1;
        let future = (cx as f32 + ahead.0, cz as f32 + ahead.1);
        let mut columns = Vec::new();
        for x in (future.0 - reach as f32).floor() as i32..=(future.0 + reach as f32).ceil() as i32
        {
            for z in
                (future.1 - reach as f32).floor() as i32..=(future.1 + reach as f32).ceil() as i32
            {
                let (dx, dz) = (x - cx, z - cz);
                if Self::is_in_view_range(dx, dz, self.radius) {
                    continue;
                }
                let (fx, fz) = (x as f32 - future.0, z as f32 - future.1);
                if fx * fx + fz * fz <= (reach * reach) as f32 {
                    columns.push((dx * dx + dz * dz, (x, z)));
                }
            }
        }
        columns.sort_unstable();
        columns.into_iter().map(|(_, column)| column).collect()
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

        // Evict chunks that left the view disk.
        let mut evicted = Vec::new();
        self.loaded.retain(|&(lx, lz)| {
            let in_range = Self::is_in_view_range(lx - cx, lz - cz, r);
            if !in_range {
                evicted.push((lx, lz));
            }
            in_range
        });
        for &pos in &evicted {
            self.window.forget(pos);
        }

        let lookahead = self.lookahead();
        let reach = r + 1;
        let mut to_load = Vec::with_capacity(((2 * reach + 1) * (2 * reach + 1)) as usize);
        for dx in -reach..=reach {
            for dz in -reach..=reach {
                let chunk = (cx + dx, cz + dz);
                if Self::is_in_view_range(dx, dz, r) && !self.loaded.contains(&chunk) {
                    to_load.push((Self::priority(dx, dz, lookahead), chunk));
                }
            }
        }
        to_load.sort_by(|a, b| a.0.0.total_cmp(&b.0.0).then(a.0.1.total_cmp(&b.0.1)));
        self.load_queue = to_load.into_iter().map(|(_, chunk)| chunk).collect();

        evicted
    }

    /// Whether the chunk at `(dx, dz)` from the view center is in a view of
    /// `radius`: the disk `dx² + dz² <= (radius + 1)²`, the columns BDS
    /// 1.26.51 streams (113 at radius 5, tools/perf BDS reference). The client
    /// renders the disk of `radius`; the extra ring gives it the neighbours
    /// it needs to mesh the edge.
    #[inline]
    fn is_in_view_range(dx: i32, dz: i32, radius: i32) -> bool {
        dx * dx + dz * dz <= (radius + 1) * (radius + 1)
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
                if let Some(mut viewers) = world.get_mut::<ChunkViewers>(chunk_entity)
                    && viewers.remove(player_entity)
                {
                    cleaned += 1;
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

    fn distance_sq(center: (i32, i32), chunk: (i32, i32)) -> i32 {
        (chunk.0 - center.0).pow(2) + (chunk.1 - center.1).pow(2)
    }

    /// Sends everything queued, like a client that took the whole view.
    fn load_all(loader: &mut ChunkLoader) {
        for (x, z) in loader.queue().to_vec() {
            loader.mark_loaded(x, z);
        }
        loader.retain_queue(|_| false);
    }

    #[test]
    fn new_at_queues_the_whole_view_disk() {
        let loader = ChunkLoader::new_at(2, (5, 5));
        assert_eq!(loader.position(), (5, 5));
        // dx² + dz² <= 9
        assert_eq!(loader.queue_len(), 29);
        assert_eq!(loader.loaded_count(), 0);
    }

    #[test]
    fn standing_still_loads_nearest_first() {
        let loader = ChunkLoader::new_at(4, (10, -3));
        let queue = loader.queue();
        assert_eq!(queue[0], (10, -3));
        for pair in queue.windows(2) {
            assert!(
                distance_sq((10, -3), pair[0]) <= distance_sq((10, -3), pair[1]),
                "{pair:?} out of nearest-first order"
            );
        }
    }

    #[test]
    fn moving_loads_the_terrain_ahead_before_the_terrain_behind() {
        let mut loader = ChunkLoader::new(8);
        // 1.5 blocks per tick east (30 blocks per second, elytra speed).
        for tick in 0..40 {
            loader.observe_position(f64::from(tick) * 1.5, 0.0);
        }
        let (ahead_x, ahead_z) = loader.lookahead();
        assert!(
            ahead_x > 3.0 && ahead_z.abs() < 1e-3,
            "{:?}",
            loader.lookahead()
        );
        loader.move_to(1, 0);
        let rank = |chunk| loader.queue().iter().position(|c| *c == chunk).unwrap();
        // The player's own chunk still comes first.
        assert_eq!(loader.queue()[0], (1, 0));
        // Same distance from the player, opposite sides.
        for d in 1..=8 {
            assert!(rank((1 + d, 0)) < rank((1 - d, 0)), "distance {d}");
        }
        // The view edge ahead (8 chunks) comes before terrain 5 chunks behind.
        assert!(rank((9, 0)) < rank((-4, 0)));
    }

    #[test]
    fn lookahead_is_capped_at_half_the_radius() {
        let mut loader = ChunkLoader::new(4);
        for tick in 0..100 {
            loader.observe_position(f64::from(tick) * 7.0, 0.0);
        }
        let (x, z) = loader.lookahead();
        assert!((x - 2.0).abs() < 1e-3 && z.abs() < 1e-3, "{x} {z}");
    }

    #[test]
    fn flying_prefetches_the_columns_about_to_enter_the_view() {
        let mut loader = ChunkLoader::new_at(12, (0, 0));
        assert!(loader.prefetch().is_empty(), "standing still");
        // 1.5 blocks per tick east; 40 ticks of it reach 3.75 chunks ahead.
        for tick in 0..40 {
            loader.observe_position(f64::from(tick) * 1.5, 0.0);
        }
        let prefetch = loader.prefetch();
        let view = |(x, z): (i32, i32)| x * x + z * z <= 13 * 13;
        assert!(
            prefetch.iter().all(|&c| !view(c)),
            "only columns outside the view"
        );
        assert!(prefetch.iter().all(|&(x, _)| x > 0), "only columns ahead");
        // The next columns straight ahead, up to the prefetch distance.
        for x in 14..=16 {
            assert!(prefetch.contains(&(x, 0)), "{x} chunks ahead");
        }
        assert!(!prefetch.contains(&(17, 0)));
        // Nearest first: the columns just outside the view's edge lead.
        let d = |(x, z): (i32, i32)| x * x + z * z;
        assert!(prefetch.windows(2).all(|w| d(w[0]) <= d(w[1])));
        assert_eq!(d(prefetch[0]), 13 * 13 + 1);
    }

    #[test]
    fn a_teleport_is_not_movement() {
        let mut loader = ChunkLoader::new(8);
        loader.observe_position(0.0, 0.0);
        loader.observe_position(500.0, 0.0);
        assert_eq!(loader.lookahead(), (0.0, 0.0));
    }

    #[test]
    fn moving_away_evicts_every_sent_chunk_and_forgets_its_window_slot() {
        let mut loader = ChunkLoader::new_at(1, (0, 0));
        let now = std::time::Instant::now();
        loader.window_mut().sent((1, 1), true, now);
        load_all(&mut loader);
        assert_eq!(loader.loaded_count(), 13);

        let evicted = loader.move_to(10, 10);
        assert_eq!(evicted.len(), 13);
        assert_eq!(loader.loaded_count(), 0);
        assert_eq!(loader.window().in_flight(), 0);
        assert_eq!(loader.queue_len(), 13);
    }

    #[test]
    fn moving_one_chunk_queues_only_the_new_edge() {
        let mut loader = ChunkLoader::new_at(1, (0, 0));
        load_all(&mut loader);
        let mut evicted = loader.move_to(1, 0);
        evicted.sort();
        assert_eq!(evicted, vec![(-2, 0), (-1, -1), (-1, 1), (0, -2), (0, 2)]);
        let mut queued = loader.queue().to_vec();
        queued.sort();
        assert_eq!(queued, vec![(1, -2), (1, 2), (2, -1), (2, 1), (3, 0)]);
    }

    #[test]
    fn shrinking_the_radius_evicts_the_outer_ring() {
        let mut loader = ChunkLoader::new_at(2, (0, 0));
        load_all(&mut loader);
        assert_eq!(loader.loaded_count(), 29);
        let evicted = loader.set_radius(1);
        assert_eq!(evicted.len(), 16);
        assert_eq!(loader.loaded_count(), 13);
    }

    /// The view is the disk of radius + 1 chunks, as many columns as BDS
    /// 1.26.51 streams (113 at radius 5).
    #[test]
    fn the_view_is_the_disk_bds_streams() {
        let count = |r: i32| {
            (-r - 1..=r + 1)
                .flat_map(|dx| (-r - 1..=r + 1).map(move |dz| (dx, dz)))
                .filter(|&(dx, dz)| ChunkLoader::is_in_view_range(dx, dz, r))
                .count()
        };
        assert_eq!(count(5), 113);
        assert_eq!(ChunkLoader::new_at(5, (0, 0)).queue_len(), 113);
        assert!(ChunkLoader::is_in_view_range(6, 0, 5));
        assert!(!ChunkLoader::is_in_view_range(5, 5, 5));
    }
}
