//! Canyon (ravine) carver implementation.
//!
//! Creates deep ravines/canyons from the surface matching vanilla Minecraft.

use super::{CanyonCarverConfig, WorldCarver};
use crate::world::chunk::{blocks, Chunk};
use crate::world::generator::aquifer::FluidPicker;
use crate::world::generator::xoroshiro::Xoroshiro128;
use std::f64::consts::PI;

/// Canyon carver that creates deep ravines from the surface.
#[derive(Debug, Clone)]
pub struct CanyonCarver {
    config: CanyonCarverConfig,
}

impl CanyonCarver {
    /// Create a new canyon carver with default configuration.
    pub fn new() -> Self {
        Self {
            config: CanyonCarverConfig::default(),
        }
    }

    /// Create a new canyon carver with custom configuration.
    pub fn with_config(config: CanyonCarverConfig) -> Self {
        Self { config }
    }

    /// Create a seeded RNG for the given chunk position.
    fn create_rng(&self, seed: i64, chunk_x: i32, chunk_z: i32) -> Xoroshiro128 {
        let pos_seed = seed
            .wrapping_add(chunk_x as i64 * 341873128712)
            .wrapping_add(chunk_z as i64 * 132897987541)
            .wrapping_add(1); // Different salt from cave carver
        Xoroshiro128::from_seed(pos_seed)
    }

    /// Carve the canyon path.
    fn carve_canyon(
        &self,
        chunk: &mut Chunk,
        rng: &mut Xoroshiro128,
        start_x: f64,
        start_y: f64,
        start_z: f64,
        length: i32,
        fluid_picker: Option<&dyn FluidPicker>,
    ) {
        let chunk_center_x = (chunk.x * 16) as f64 + 8.0;
        let chunk_center_z = (chunk.z * 16) as f64 + 8.0;

        let mut x = start_x;
        let mut y = start_y;
        let mut z = start_z;

        let mut yaw = rng.next_float() as f64 * PI * 2.0;
        let mut pitch = (rng.next_float() as f64 - 0.5) * 0.25;

        let base_width = (rng.next_float() as f64 * 2.0 + 2.0) * 2.0;
        let height = base_width * self.config.shape.thickness;

        for i in 0..length {
            let progress = i as f64 / length as f64;

            // Width varies along length - wider in middle
            let width_factor = 1.0 - (progress * 2.0 - 1.0).abs();
            let width = base_width * width_factor * self.config.shape.horizontal_radius_factor;
            let current_height = height * self.config.shape.vertical_radius_default_factor;

            // Move along the canyon
            let cos_pitch = pitch.cos();
            let sin_pitch = pitch.sin();
            let cos_yaw = yaw.cos();
            let sin_yaw = yaw.sin();

            x += cos_yaw * cos_pitch;
            y += sin_pitch * self.config.vertical_rotation_range;
            z += sin_yaw * cos_pitch;

            // Slight direction changes
            yaw += (rng.next_float() as f64 - 0.5) * 0.2;
            pitch = pitch * 0.9 + (rng.next_float() as f64 - 0.5) * 0.1;
            pitch = pitch.clamp(-0.5, 0.5);

            // Check if we're close enough to the chunk to carve
            let dist_x = x - chunk_center_x;
            let dist_z = z - chunk_center_z;
            let dist_sq = dist_x * dist_x + dist_z * dist_z;

            if dist_sq < (16.0 + width * 2.0).powi(2) {
                self.carve_section(chunk, x, y, z, width, current_height, fluid_picker);
            }
        }
    }

    /// Carve a vertical section of the canyon.
    fn carve_section(
        &self,
        chunk: &mut Chunk,
        center_x: f64,
        center_y: f64,
        center_z: f64,
        width: f64,
        height: f64,
        fluid_picker: Option<&dyn FluidPicker>,
    ) {
        let chunk_min_x = chunk.x * 16;
        let chunk_min_z = chunk.z * 16;

        // Canyons are taller than wide
        let v_radius = height;
        let h_radius = width;

        // Calculate block bounds
        let min_x = ((center_x - h_radius).floor() as i32).max(chunk_min_x);
        let max_x = ((center_x + h_radius).ceil() as i32).min(chunk_min_x + 15);
        let min_y = ((center_y - v_radius).floor() as i32).max(self.config.min_y);
        let max_y = ((center_y + v_radius).ceil() as i32).min(self.config.max_y);
        let min_z = ((center_z - h_radius).floor() as i32).max(chunk_min_z);
        let max_z = ((center_z + h_radius).ceil() as i32).min(chunk_min_z + 15);

        for bx in min_x..=max_x {
            let local_x = (bx - chunk_min_x) as u8;
            let dx = (bx as f64 + 0.5 - center_x) / h_radius;

            for bz in min_z..=max_z {
                let local_z = (bz - chunk_min_z) as u8;
                let dz = (bz as f64 + 0.5 - center_z) / h_radius;

                let h_dist_sq = dx * dx + dz * dz;
                if h_dist_sq >= 1.0 {
                    continue;
                }

                // Canyons carve from top to bottom
                for by in (min_y..=max_y).rev() {
                    let dy = (by as f64 + 0.5 - center_y) / v_radius;

                    // More lenient horizontal check for canyons
                    if h_dist_sq < 0.8 && dy.abs() < 1.0 {
                        // Check fluid picker
                        if let Some(fp) = fluid_picker {
                            let fluid_status = fp.compute_fluid(bx, by, bz);
                            if fluid_status.fluid_level > by {
                                continue;
                            }
                        }

                        // Only carve stone-like blocks
                        let current = chunk.get_block(local_x, by as i16, local_z);
                        if self.should_carve_block(current) {
                            chunk.set_block(local_x, by as i16, local_z, *blocks::AIR);
                        }
                    }
                }
            }
        }
    }

    /// Check if a block should be carved.
    fn should_carve_block(&self, block_id: u32) -> bool {
        block_id == *blocks::STONE
            || block_id == *blocks::DEEPSLATE
            || block_id == *blocks::GRANITE
            || block_id == *blocks::DIORITE
            || block_id == *blocks::ANDESITE
            || block_id == *blocks::TUFF
            || block_id == *blocks::DIRT
            || block_id == *blocks::GRASS_BLOCK
            || block_id == *blocks::SAND
            || block_id == *blocks::SANDSTONE
            || block_id == *blocks::GRAVEL
    }
}

impl Default for CanyonCarver {
    fn default() -> Self {
        Self::new()
    }
}

impl WorldCarver for CanyonCarver {
    fn is_start_chunk(&self, seed: i64, chunk_x: i32, chunk_z: i32) -> bool {
        let mut rng = self.create_rng(seed, chunk_x, chunk_z);
        rng.next_float() < self.config.probability
    }

    fn carve(
        &self,
        chunk: &mut Chunk,
        seed: i64,
        chunk_x: i32,
        chunk_z: i32,
        fluid_picker: Option<&dyn FluidPicker>,
    ) {
        // Check neighboring chunks for canyon starts
        for dx in -8..=8 {
            for dz in -8..=8 {
                let cx = chunk_x + dx;
                let cz = chunk_z + dz;

                if self.is_start_chunk(seed, cx, cz) {
                    let mut rng = self.create_rng(seed, cx, cz);

                    // Skip probability check
                    let _ = rng.next_float();

                    // Canyon starting position
                    let start_x = (cx * 16) as f64 + rng.next_float() as f64 * 16.0;
                    let start_y = self.config.min_y as f64
                        + rng.next_float() as f64
                            * (self.config.max_y - self.config.min_y) as f64;
                    let start_z = (cz * 16) as f64 + rng.next_float() as f64 * 16.0;

                    // Canyon length
                    let length = rng.next_int(40) as i32 + 50;

                    self.carve_canyon(chunk, &mut rng, start_x, start_y, start_z, length, fluid_picker);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canyon_carver_creation() {
        let carver = CanyonCarver::new();
        assert!(carver.config.probability > 0.0);
        assert!(carver.config.probability < 0.1); // Canyons are rare
    }

    #[test]
    fn test_canyon_carver_determinism() {
        let carver = CanyonCarver::new();
        let result1 = carver.is_start_chunk(12345, 0, 0);
        let result2 = carver.is_start_chunk(12345, 0, 0);
        assert_eq!(result1, result2, "Canyon carver should be deterministic");
    }
}
