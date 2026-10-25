//! Java-style structures for the [`super::JavaGenerator`]: the
//! [`StructureContext`] the AOT-compiled structure code queries (biome source,
//! noise-only column heights) and the per-world [`StructureManager`].
//!
//! Noise-only heights follow Java's `ChunkGenerator.getBaseHeight`: the top of
//! the density field, with water below sea level (and lava below Y -54) where
//! the density is not positive. Two documented approximations: aquifers are
//! reduced to that default fluid rule (Java evaluates the full aquifer while
//! iterating the column), and the density is sampled per block without Java's
//! per-column cell interpolation.

use std::collections::HashMap;
use std::sync::Mutex;

use unastar_noise::features::generated::OVERWORLD_BIOMES;
use unastar_noise::features::java_blocks::states;
use unastar_noise::features::{Heightmap, JavaState};
use unastar_noise::overworld_settings;
use unastar_noise::structures::context::StructureContext;
use unastar_noise::structures::manager::StructureManager;

use super::biome_source::sample_biome;
use super::density::{
    ColumnContext, FlatCacheGrid, FunctionContext, NoiseRegistry, compute_final_density,
};

/// Noise-only top of one column: first Y above the highest non-air block and
/// above the highest solid block.
#[derive(Clone, Copy)]
struct ColumnTop {
    surface: i32,
    floor: i32,
}

/// Column heights are cached by position; the cache is cleared when it grows
/// past this many columns.
const COLUMN_CACHE_LIMIT: usize = 1 << 18;

/// Structure generation state of one Java-style overworld.
pub struct JavaStructures {
    pub manager: StructureManager,
    columns: Mutex<HashMap<(i32, i32), ColumnTop>>,
}

impl Default for JavaStructures {
    fn default() -> Self {
        Self::new()
    }
}

impl JavaStructures {
    pub fn new() -> Self {
        Self {
            manager: StructureManager::new(OVERWORLD_BIOMES),
            columns: Mutex::new(HashMap::new()),
        }
    }

    /// A [`StructureContext`] over the generator's noises.
    pub fn context<'a>(&'a self, seed: i64, noises: &'a NoiseRegistry) -> JavaStructureContext<'a> {
        JavaStructureContext {
            seed,
            noises,
            columns: &self.columns,
        }
    }
}

/// [`StructureContext`] of the Java-style generator.
pub struct JavaStructureContext<'a> {
    seed: i64,
    noises: &'a NoiseRegistry,
    columns: &'a Mutex<HashMap<(i32, i32), ColumnTop>>,
}

impl JavaStructureContext<'_> {
    fn density(&self, grid: &FlatCacheGrid, col: &ColumnContext, x: i32, y: i32, z: i32) -> f64 {
        compute_final_density(&FunctionContext::new(x, y, z), self.noises, grid, col)
    }

    /// Noise-only block at `y` of a column (stone, water, lava or air).
    fn fluid_or_air(y: i32) -> JavaState {
        if y < -54 {
            states::LAVA
        } else if y < overworld_settings::SEA_LEVEL {
            states::WATER
        } else {
            states::AIR
        }
    }

    fn column_top(&self, x: i32, z: i32) -> ColumnTop {
        if let Some(t) = self.columns.lock().expect("column cache").get(&(x, z)) {
            return *t;
        }
        let grid = FlatCacheGrid::new(x >> 4, z >> 4, self.noises);
        let col = ColumnContext::new(x, z, self.noises, &grid);
        let min_y = overworld_settings::MIN_Y;
        let max_y = min_y + overworld_settings::HEIGHT - 1;
        // Step down a cell (8 blocks) at a time to the first solid sample, then
        // walk back up block by block to the exact top.
        let mut y = max_y;
        while y > min_y && self.density(&grid, &col, x, y, z) <= 0.0 {
            y -= 8;
        }
        let mut solid_top = None;
        if y > min_y || self.density(&grid, &col, x, y.max(min_y), z) > 0.0 {
            let mut t = y.max(min_y);
            while t < max_y && self.density(&grid, &col, x, t + 1, z) > 0.0 {
                t += 1;
            }
            solid_top = Some(t);
        }
        let floor = solid_top.map_or(min_y, |t| t + 1);
        let surface = floor.max(if overworld_settings::SEA_LEVEL > min_y {
            // Water fills every non-solid block below sea level.
            overworld_settings::SEA_LEVEL
        } else {
            min_y
        });
        let top = ColumnTop { surface, floor };
        let mut cache = self.columns.lock().expect("column cache");
        if cache.len() >= COLUMN_CACHE_LIMIT {
            cache.clear();
        }
        cache.insert((x, z), top);
        top
    }
}

impl StructureContext for JavaStructureContext<'_> {
    fn seed(&self) -> i64 {
        self.seed
    }
    fn min_y(&self) -> i32 {
        overworld_settings::MIN_Y
    }
    fn height(&self) -> i32 {
        overworld_settings::HEIGHT
    }
    fn sea_level(&self) -> i32 {
        overworld_settings::SEA_LEVEL
    }
    fn noise_biome(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> u8 {
        sample_biome(self.noises, quart_x, quart_y, quart_z) as u8
    }
    fn base_height(&self, x: i32, z: i32, kind: Heightmap) -> i32 {
        let top = self.column_top(x, z);
        match kind {
            Heightmap::OceanFloorWg | Heightmap::OceanFloor => top.floor,
            Heightmap::WorldSurfaceWg
            | Heightmap::WorldSurface
            | Heightmap::MotionBlocking
            | Heightmap::MotionBlockingNoLeaves => top.surface,
        }
    }
    fn base_column(&self, x: i32, z: i32, out: &mut [JavaState]) {
        let grid = FlatCacheGrid::new(x >> 4, z >> 4, self.noises);
        let col = ColumnContext::new(x, z, self.noises, &grid);
        let min_y = overworld_settings::MIN_Y;
        for (i, s) in out.iter_mut().enumerate() {
            let y = min_y + i as i32;
            *s = if self.density(&grid, &col, x, y, z) > 0.0 {
                states::STONE
            } else {
                Self::fluid_or_air(y)
            };
        }
    }
}
