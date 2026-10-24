//! In-memory [`FeatureWorld`] for unit tests of feature implementations.
//!
//! Blocks default to air (or `fill_below` for `y < fill_y`), heightmaps are
//! computed by scanning down from the top, and every position is writable
//! unless it lies outside `writable` (inclusive block bounds on x/z).

use super::blocks::{BlockPos, Heightmap, JavaState};
use super::eval::FeatureWorld;
use super::java_blocks::states;
use std::collections::HashMap;

/// Test world.
#[derive(Debug, Clone)]
pub struct TestWorld {
    pub blocks: HashMap<BlockPos, JavaState>,
    /// Blocks with `y < fill_y` that are not in `blocks` read as `fill`.
    pub fill_y: i32,
    pub fill: JavaState,
    pub seed: i64,
    pub biome: u8,
    /// `(min_x, min_z, max_x, max_z)`; writes outside are rejected.
    pub writable: Option<(i32, i32, i32, i32)>,
    pub ticks: Vec<BlockPos>,
}

impl Default for TestWorld {
    fn default() -> Self {
        Self {
            blocks: HashMap::new(),
            fill_y: -64,
            fill: states::STONE,
            seed: 0,
            biome: 0,
            writable: None,
            ticks: Vec::new(),
        }
    }
}

impl TestWorld {
    /// Stone below `surface_y`, air above.
    pub fn flat(surface_y: i32) -> Self {
        Self {
            fill_y: surface_y,
            ..Self::default()
        }
    }

    /// Count blocks equal to `state` in the explicit block map.
    pub fn count(&self, state: JavaState) -> usize {
        self.blocks.values().filter(|s| **s == state).count()
    }
}

impl FeatureWorld for TestWorld {
    fn min_y(&self) -> i32 {
        -64
    }
    fn height(&self) -> i32 {
        384
    }
    fn sea_level(&self) -> i32 {
        63
    }
    fn seed(&self) -> i64 {
        self.seed
    }
    fn get_state(&mut self, pos: BlockPos) -> JavaState {
        if let Some(s) = self.blocks.get(&pos) {
            return *s;
        }
        if pos.y < self.fill_y && pos.y >= -64 {
            self.fill
        } else {
            states::AIR
        }
    }
    fn set_state(&mut self, pos: BlockPos, state: JavaState) -> bool {
        if self.is_outside_build_height(pos.y) {
            return false;
        }
        if let Some((x0, z0, x1, z1)) = self.writable
            && (pos.x < x0 || pos.x > x1 || pos.z < z0 || pos.z > z1)
        {
            return false;
        }
        self.blocks.insert(pos, state);
        true
    }
    fn get_height(&mut self, kind: Heightmap, x: i32, z: i32) -> i32 {
        let mut y = self.max_y() - 1;
        while y >= self.min_y() {
            if kind.is_opaque(self.get_state(BlockPos::new(x, y, z))) {
                return y + 1;
            }
            y -= 1;
        }
        self.min_y()
    }
    fn biome(&mut self, _pos: BlockPos) -> u8 {
        self.biome
    }
    fn schedule_tick(&mut self, pos: BlockPos) {
        self.ticks.push(pos);
    }
}
