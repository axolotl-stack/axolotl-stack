//! Surface rule application system.
//!
//! The [`SurfaceSystem`] applies surface rules to terrain chunks,
//! replacing stone with appropriate surface blocks based on biome
//! and position.

use super::context::SurfaceContext;
use super::rule::Rule;
use crate::world::chunk::{blocks, Chunk};
use crate::world::generator::climate::BiomeNoise;
use crate::world::generator::noise::DoublePerlinNoise;
use crate::world::generator::xoroshiro::Xoroshiro128;

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
    pub fn build_surface(&self, chunk: &mut Chunk, chunk_x: i32, chunk_z: i32) {
        let min_y = -64i32;
        let max_y = 320i32;

        let mut ctx = SurfaceContext::new(chunk_x, chunk_z, min_y, max_y);

        for local_z in 0u8..16 {
            for local_x in 0u8..16 {
                let world_x = chunk_x * 16 + local_x as i32;
                let world_z = chunk_z * 16 + local_z as i32;

                // Calculate XZ-dependent values
                let surface_depth = self.get_surface_depth(world_x, world_z);
                let surface_secondary = self.get_surface_secondary(world_x, world_z);
                let steep = self.is_steep(chunk, local_x, local_z);
                let min_surface_level = chunk.height_map().at(local_x, local_z) as i32 - surface_depth;

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

                // Get the surface Y for this column
                let surface_y = chunk.height_map().at(local_x, local_z);

                // Iterate from surface down
                for y in (min_y..=surface_y as i32).rev() {
                    let block = chunk.get_block(local_x, y as i16, local_z);

                    // Skip air
                    if block == *blocks::AIR {
                        stone_depth_above = 0;
                        in_stone = false;
                        continue;
                    }

                    // Track water level
                    if block == *blocks::WATER {
                        if water_height == i32::MIN {
                            water_height = y;
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

                    // Calculate stone depth below (scan down)
                    let mut stone_depth_below = 0;
                    for dy in 1..=5 {
                        let below_y = y - dy;
                        if below_y < min_y {
                            break;
                        }
                        let below = chunk.get_block(local_x, below_y as i16, local_z);
                        if below == *blocks::AIR || below == *blocks::WATER {
                            break;
                        }
                        stone_depth_below += 1;
                    }

                    // Only apply rules to the default block (stone)
                    if block == self.default_block {
                        let biome = self.biome_noise.get_biome(world_x, y, world_z);
                        ctx.update_y(y, stone_depth_above, stone_depth_below, water_height, biome);

                        if let Some(new_block) = self.rule.try_apply(&ctx) {
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
    use crate::world::generator::constants::Biome;
    use crate::world::generator::surface::condition::BiomeCheck;
    use crate::world::generator::surface::rule::{BlockRule, SequenceRule, TestRule};

    fn create_test_system() -> SurfaceSystem {
        let seed = 12345i64;
        let biome_noise = BiomeNoise::from_seed(seed);

        // Simple rule: Desert -> Sand, else Grass
        let rule: Box<dyn Rule> = Box::new(SequenceRule::new(vec![
            Box::new(TestRule::new(
                Box::new(BiomeCheck::single(Biome::Desert)),
                Box::new(BlockRule::new(*blocks::SAND)),
            )),
            Box::new(BlockRule::new(*blocks::GRASS_BLOCK)),
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
