//! What structure start generation needs from the chunk generator, before any
//! chunk exists (Java `Structure.GenerationContext`): the biome source, the
//! noise-only terrain height and column, and the level's vertical bounds.

use crate::features::{Heightmap, JavaState};

/// Chunk-generator queries used while generating structure starts.
///
/// Implemented by the Java-style generator runtime. Results must depend only on
/// the seed and the arguments.
pub trait StructureContext: Sync {
    /// Level seed.
    fn seed(&self) -> i64;
    /// Lowest buildable Y.
    /// Lowest Y of the `WorldGenerationContext`: `max(level min_y, generator min_y)`.
    fn min_y(&self) -> i32;
    /// Height of the `WorldGenerationContext`: `min(level height, generator gen depth)`
    /// (384 in the overworld, 128 in the nether). Height providers resolve
    /// anchors against this and [`Self::min_y`].
    fn height(&self) -> i32;
    /// Lowest buildable Y of the level, before the generator's height clamp.
    /// Jigsaw dimension padding and assembly space use level bounds rather
    /// than the height-provider `WorldGenerationContext` bounds.
    fn level_min_y(&self) -> i32 {
        self.min_y()
    }
    /// Full buildable level height (256 in the Nether, whose generator
    /// depth and height-provider context are only 128 blocks).
    fn level_height(&self) -> i32 {
        self.height()
    }
    fn sea_level(&self) -> i32;
    /// `BiomeSource.getNoiseBiome(qx, qy, qz, sampler)`: biome index (into
    /// `features::generated::BIOME_NAMES`) at a quart position, no zoom.
    fn noise_biome(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> u8;
    /// `ChunkGenerator.getBaseHeight(x, z, type, level, randomState)`: one
    /// above the highest block of the noise-only terrain counted by `kind`.
    fn base_height(&self, x: i32, z: i32, kind: Heightmap) -> i32;
    /// `ChunkGenerator.getBaseColumn(x, z, level, randomState)`: noise-only
    /// block states from `min_y` upwards (`out.len() == height`).
    fn base_column(&self, x: i32, z: i32, out: &mut [JavaState]);

    /// `getFirstFreeHeight` (`getBaseHeight(x, z, kind) `).
    #[inline]
    fn first_free_height(&self, x: i32, z: i32, kind: Heightmap) -> i32 {
        self.base_height(x, z, kind)
    }

    /// `getFirstOccupiedHeight`: `base_height - 1`.
    #[inline]
    fn first_occupied_height(&self, x: i32, z: i32, kind: Heightmap) -> i32 {
        self.base_height(x, z, kind) - 1
    }
}
