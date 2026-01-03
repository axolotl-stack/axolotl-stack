//! Surface rule application system.
//!
//! The [`SurfaceSystem`] applies surface rules to terrain chunks,
//! replacing stone with appropriate surface blocks based on biome
//! and position.

use std::simd::f64x4;

use unastar_noise::noise::DoublePerlinNoise;
use unastar_noise::surface::{Rule, SurfaceContext};
use unastar_noise::xoroshiro::Xoroshiro128;

use crate::world::chunk::{blocks, Chunk};
use crate::world::generator::climate::BiomeNoise;

/// System for applying surface rules to terrain.
///
/// This system iterates over a chunk's columns and applies surface rules
/// to replace the default stone block with biome-appropriate surface blocks.
pub struct SurfaceSystem {
    /// The default block that surface rules replace.
    pub default_block: u32,
    /// Sea level (water surface).
    pub sea_level: i32,
    /// Noise for surface depth variation.
    surface_noise: DoublePerlinNoise,
    /// Secondary noise for extra variation.
    surface_secondary_noise: DoublePerlinNoise,
    /// Main surface rule.
    rule: Box<dyn Rule>,
    /// Biome noise for sampling biomes.
    biome_noise: BiomeNoise,
}

impl SurfaceSystem {
    /// Create a new surface system.
    ///
    /// # Arguments
    /// * `seed` - World seed for noise generation
    /// * `rule` - The surface rule to apply
    /// * `biome_noise` - Biome noise for sampling biomes
    pub fn new(seed: i64, rule: Box<dyn Rule>, biome_noise: BiomeNoise) -> Self {
        // Create surface noise using the seed
        let mut rng = Xoroshiro128::from_seed(seed.wrapping_add(0x1234567890ABCDEF));
        let surface_noise = DoublePerlinNoise::new(&mut rng, &[1.0, 1.0, 1.0], -6);

        let mut rng2 = Xoroshiro128::from_seed(seed.wrapping_add(0x0EDCBA0987654321));
        let surface_secondary_noise = DoublePerlinNoise::new(&mut rng2, &[1.0, 1.0], -6);

        Self {
            default_block: *blocks::STONE,
            sea_level: 63,
            surface_noise,
            surface_secondary_noise,
            rule,
            biome_noise,
        }
    }

    /// Get surface depth at a position using noise.
    pub fn get_surface_depth(&self, x: i32, z: i32) -> i32 {
        let noise = self.surface_noise.sample(x as f64, 0.0, z as f64);
        // Scale noise to reasonable surface depth (typically 3-6 blocks)
        ((noise + 1.0) * 2.75 + 3.0) as i32
    }

    /// Get secondary surface noise value.
    pub fn get_surface_secondary(&self, x: i32, z: i32) -> f64 {
        // Return 0-1 range
        (self
            .surface_secondary_noise
            .sample(x as f64, 0.0, z as f64)
            + 1.0)
            / 2.0
    }

    /// SIMD: Get surface depth for 4 positions at once.
    #[inline]
    pub fn get_surface_depth_4(&self, x: f64x4, z: f64x4) -> [i32; 4] {
        use std::simd::prelude::*;
        let noise = self.surface_noise.sample_4(x, f64x4::splat(0.0), z);
        // ((noise + 1.0) * 2.75 + 3.0) as i32
        let result = (noise + f64x4::splat(1.0)) * f64x4::splat(2.75) + f64x4::splat(3.0);
        let arr = result.to_array();
        [arr[0] as i32, arr[1] as i32, arr[2] as i32, arr[3] as i32]
    }

    /// SIMD: Get secondary surface noise for 4 positions at once.
    #[inline]
    pub fn get_surface_secondary_4(&self, x: f64x4, z: f64x4) -> f64x4 {
        use std::simd::prelude::*;
        let noise = self.surface_secondary_noise.sample_4(x, f64x4::splat(0.0), z);
        // (noise + 1.0) / 2.0
        (noise + f64x4::splat(1.0)) * f64x4::splat(0.5)
    }

    /// Calculate whether a position is on steep terrain.
    ///
    /// Compares heights at adjacent positions to detect slopes.
    fn is_steep(&self, chunk: &Chunk, local_x: u8, local_z: u8) -> bool {
        // Get height at this column
        let center_height = chunk.height_map().at(local_x, local_z);

        // Check adjacent columns (if within chunk)
        let threshold = 3i16; // Height difference to consider "steep"

        // Check all 4 cardinal directions
        let neighbors = [
            (local_x.wrapping_sub(1), local_z),
            (local_x.wrapping_add(1), local_z),
            (local_x, local_z.wrapping_sub(1)),
            (local_x, local_z.wrapping_add(1)),
        ];

        for (nx, nz) in neighbors {
            if nx < 16 && nz < 16 {
                let neighbor_height = chunk.height_map().at(nx, nz);
                if (center_height - neighbor_height).abs() >= threshold {
                    return true;
                }
            }
        }

        false
    }

    /// Build surface for a chunk.
    ///
    /// This iterates over all columns in the chunk and applies surface rules
    /// to replace stone blocks with appropriate surface blocks.
    ///
    /// Uses SIMD to batch noise sampling for 4 X columns at a time.
    pub fn build_surface(&self, chunk: &mut Chunk, chunk_x: i32, chunk_z: i32) {
        let min_y = -64i32;
        let max_y = 320i32;

        let mut ctx = SurfaceContext::new(chunk_x, chunk_z, min_y, max_y);

        // Block lookup closure - converts block names to IDs
        let get_block = |name: &str| blocks::get_block_id(name);

        let base_x = chunk_x * 16;
        let base_z = chunk_z * 16;

        for local_z in 0u8..16 {
            let world_z = base_z + local_z as i32;
            let z_simd = f64x4::splat(world_z as f64);

            // Process 4 X columns at a time using SIMD
            for local_x_base in (0u8..16).step_by(4) {
                // Build SIMD vectors for 4 consecutive X positions
                let world_x_arr = [
                    (base_x + local_x_base as i32) as f64,
                    (base_x + local_x_base as i32 + 1) as f64,
                    (base_x + local_x_base as i32 + 2) as f64,
                    (base_x + local_x_base as i32 + 3) as f64,
                ];
                let x_simd = f64x4::from_array(world_x_arr);

                // Batch sample noise for 4 columns
                let surface_depths = self.get_surface_depth_4(x_simd, z_simd);
                let surface_secondaries = self.get_surface_secondary_4(x_simd, z_simd);
                let secondary_arr = surface_secondaries.to_array();

                // Process each of the 4 columns
                for i in 0..4 {
                    let local_x = local_x_base + i as u8;
                    let world_x = base_x + local_x as i32;

                    // Get the surface Y for this column
                    let surface_y = chunk.height_map().at(local_x, local_z) as i32;

                    // Use batched noise values
                    let surface_depth = surface_depths[i];
                    let surface_secondary = secondary_arr[i];
                    let steep = self.is_steep(chunk, local_x, local_z);
                    let min_surface_level = surface_y - surface_depth;

                    // Cache biome at surface for the whole column
                    let column_biome = self.biome_noise.get_biome(world_x, surface_y, world_z);

                    ctx.update_xz(
                        world_x,
                        world_z,
                        surface_depth,
                        surface_secondary,
                        steep,
                        min_surface_level,
                    );

                    // Track stone depth as we go down from the surface
                    let mut stone_depth_above = 0;
                    let mut water_height = i32::MIN;
                    let mut in_stone = false;

                    // Iterate from surface down
                    for y in (min_y..=surface_y).rev() {
                        let block = chunk.get_block(local_x, y as i16, local_z);

                        // Skip air
                        if block == *blocks::AIR {
                            stone_depth_above = 0;
                            in_stone = false;
                            continue;
                        }

                        // Track water level (Java: r = u + 1 when first hitting fluid)
                        if block == *blocks::WATER {
                            if water_height == i32::MIN {
                                water_height = y + 1;
                            }
                            stone_depth_above = 0;
                            in_stone = false;
                            continue;
                        }

                        // Now we're in a solid block
                        if !in_stone {
                            in_stone = true;
                            stone_depth_above = 0;
                        }

                        // Only apply rules to the default block (stone)
                        if block == self.default_block {
                            ctx.update_y(y, stone_depth_above, 0, water_height, column_biome);

                            if let Some(new_block) = self.rule.try_apply(&ctx, &get_block) {
                                if new_block != block {
                                    chunk.set_block(local_x, y as i16, local_z, new_block);
                                }
                            }
                        }

                        stone_depth_above += 1;
                    }
                }
            }
        }
    }
}

impl std::fmt::Debug for SurfaceSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SurfaceSystem")
            .field("default_block", &self.default_block)
            .field("sea_level", &self.sea_level)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use unastar_noise::surface::{BiomeCheck, BlockRule, SequenceRule, TestRule};
    use unastar_noise::Biome;

    fn create_test_system() -> SurfaceSystem {
        let seed = 12345i64;
        let biome_noise = BiomeNoise::from_seed(seed);

        // Simple rule: Desert -> Sand, else Grass (using block names now)
        let rule: Box<dyn Rule> = Box::new(SequenceRule::new(vec![
            Box::new(TestRule::new(
                Box::new(BiomeCheck::single(Biome::Desert)),
                Box::new(BlockRule::new("minecraft:sand")),
            )),
            Box::new(BlockRule::new("minecraft:grass_block")),
        ]));

        SurfaceSystem::new(seed, rule, biome_noise)
    }

    #[test]
    fn test_surface_depth() {
        let system = create_test_system();
        let depth = system.get_surface_depth(0, 0);
        // Surface depth should be in reasonable range
        assert!(depth >= 0 && depth <= 10, "Depth {} out of range", depth);
    }

    #[test]
    fn test_surface_secondary() {
        let system = create_test_system();
        let secondary = system.get_surface_secondary(0, 0);
        // Should be in 0-1 range
        assert!(
            (0.0..=1.0).contains(&secondary),
            "Secondary {} out of range",
            secondary
        );
    }
}
