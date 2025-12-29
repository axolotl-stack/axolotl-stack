//! Cave carver implementation.
//!
//! Creates winding underground tunnels matching vanilla Minecraft's cave carver.

use super::{CaveCarverConfig, WorldCarver};
use crate::world::chunk::{blocks, Chunk};
use crate::world::generator::aquifer::FluidPicker;
use crate::world::generator::xoroshiro::Xoroshiro128;
use std::f64::consts::PI;

/// Cave carver that creates winding underground tunnels.
#[derive(Debug, Clone)]
pub struct CaveCarver {
    config: CaveCarverConfig,
}

impl CaveCarver {
    /// Create a new cave carver with default configuration.
    pub fn new() -> Self {
        Self {
            config: CaveCarverConfig::default(),
        }
    }

    /// Create a new cave carver with custom configuration.
    pub fn with_config(config: CaveCarverConfig) -> Self {
        Self { config }
    }

    /// Create a seeded RNG for the given chunk position.
    fn create_rng(&self, seed: i64, chunk_x: i32, chunk_z: i32) -> Xoroshiro128 {
        let pos_seed = seed
            .wrapping_add(chunk_x as i64 * 341873128712)
            .wrapping_add(chunk_z as i64 * 132897987541);
        Xoroshiro128::from_seed(pos_seed)
    }

    /// Carve a single cave branch.
    fn carve_branch(
        &self,
        chunk: &mut Chunk,
        rng: &mut Xoroshiro128,
        start_x: f64,
        start_y: f64,
        start_z: f64,
        horizontal_radius: f64,
        vertical_radius: f64,
        yaw: f64,
        pitch: f64,
        branch_count: i32,
        branch_index: i32,
        fluid_picker: Option<&dyn FluidPicker>,
    ) {
        let chunk_center_x = (chunk.x * 16) as f64 + 8.0;
        let chunk_center_z = (chunk.z * 16) as f64 + 8.0;

        let mut x = start_x;
        let mut y = start_y;
        let mut z = start_z;
        let mut current_yaw = yaw;
        let mut current_pitch = pitch;
        let mut current_h_radius = horizontal_radius;
        let mut current_v_radius = vertical_radius;

        let yaw_delta = 0.0f64;
        let pitch_delta = 0.0f64;

        // Progress through the branch
        for i in branch_index..branch_count {
            let progress = i as f64 / branch_count as f64;

            // Calculate current radius with sine wave modulation
            let radius_modifier = 1.0 + (progress * PI).sin() * horizontal_radius * 2.0;
            let h_rad =
                current_h_radius * radius_modifier * self.config.horizontal_radius_multiplier;
            let v_rad =
                current_v_radius * radius_modifier * self.config.vertical_radius_multiplier;

            // Move along the direction
            let cos_pitch = current_pitch.cos();
            let sin_pitch = current_pitch.sin();
            let cos_yaw = current_yaw.cos();
            let sin_yaw = current_yaw.sin();

            x += cos_yaw * cos_pitch;
            y += sin_pitch;
            z += sin_yaw * cos_pitch;

            // Check if we're close enough to the chunk to carve
            let dist_x = x - chunk_center_x;
            let dist_z = z - chunk_center_z;
            let dist_sq = dist_x * dist_x + dist_z * dist_z;

            // Only carve if within reasonable distance of chunk
            if dist_sq < (16.0 + h_rad * 2.0).powi(2) {
                self.carve_sphere(chunk, x, y, z, h_rad, v_rad, fluid_picker);
            }

            // Update direction with some random variation
            current_pitch = current_pitch * 0.7 + pitch_delta * 0.05;
            current_yaw += yaw_delta * 0.05;

            // Add small random perturbation
            if rng.next_float() < 0.02 {
                current_yaw += (rng.next_float() - 0.5) as f64 * PI * 0.25;
            }
            if rng.next_float() < 0.05 {
                current_pitch += (rng.next_float() - 0.5) as f64 * 0.5;
            }

            // Clamp pitch
            current_pitch = current_pitch.clamp(-PI / 4.0, PI / 4.0);

            // Random radius variations
            if rng.next_float() < 0.25 {
                current_h_radius *= 0.9 + rng.next_float() as f64 * 0.2;
                current_v_radius *= 0.9 + rng.next_float() as f64 * 0.2;
            }
        }
    }

    /// Carve a spheroid at the given position.
    fn carve_sphere(
        &self,
        chunk: &mut Chunk,
        center_x: f64,
        center_y: f64,
        center_z: f64,
        h_radius: f64,
        v_radius: f64,
        fluid_picker: Option<&dyn FluidPicker>,
    ) {
        let chunk_min_x = chunk.x * 16;
        let chunk_min_z = chunk.z * 16;

        // Calculate block bounds
        let min_x = ((center_x - h_radius).floor() as i32).max(chunk_min_x);
        let max_x = ((center_x + h_radius).ceil() as i32).min(chunk_min_x + 15);
        let min_y = ((center_y - v_radius).floor() as i32).max(self.config.min_y);
        let max_y = ((center_y + v_radius).ceil() as i32).min(self.config.max_y);
        let min_z = ((center_z - h_radius).floor() as i32).max(chunk_min_z);
        let max_z = ((center_z + h_radius).ceil() as i32).min(chunk_min_z + 15);

        // Don't carve below floor level
        let floor_y = (center_y + v_radius * self.config.floor_level) as i32;

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

                for by in (min_y.max(floor_y)..=max_y).rev() {
                    let dy = (by as f64 + 0.5 - center_y) / v_radius;
                    let dist_sq = h_dist_sq + dy * dy;

                    if dist_sq < 1.0 {
                        // Check fluid picker - don't carve into water
                        if let Some(fp) = fluid_picker {
                            let fluid_status = fp.compute_fluid(bx, by, bz);
                            if fluid_status.fluid_level > by {
                                continue; // Don't carve below water level
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

    /// Check if a block should be carved (stone-like blocks only).
    fn should_carve_block(&self, block_id: u32) -> bool {
        block_id == *blocks::STONE
            || block_id == *blocks::DEEPSLATE
            || block_id == *blocks::GRANITE
            || block_id == *blocks::DIORITE
            || block_id == *blocks::ANDESITE
            || block_id == *blocks::TUFF
            || block_id == *blocks::DIRT
            || block_id == *blocks::SAND
            || block_id == *blocks::SANDSTONE
            || block_id == *blocks::GRAVEL
    }
}

impl Default for CaveCarver {
    fn default() -> Self {
        Self::new()
    }
}

impl WorldCarver for CaveCarver {
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
        // Check neighboring chunks for cave starts that might extend into this chunk
        for dx in -8..=8 {
            for dz in -8..=8 {
                let cx = chunk_x + dx;
                let cz = chunk_z + dz;

                if self.is_start_chunk(seed, cx, cz) {
                    let mut rng = self.create_rng(seed, cx, cz);

                    // Skip the probability check we already did
                    let _ = rng.next_float();

                    // Generate cave parameters
                    let cave_count = rng.next_int(15) as i32 + 1;

                    for _ in 0..cave_count {
                        // Cave starting position within the starting chunk
                        let start_x = (cx * 16) as f64 + rng.next_float() as f64 * 16.0;
                        let start_y = self.config.min_y as f64
                            + rng.next_float() as f64
                                * (self.config.max_y - self.config.min_y) as f64;
                        let start_z = (cz * 16) as f64 + rng.next_float() as f64 * 16.0;

                        // Cave branch parameters
                        let branch_count = rng.next_int(40) as i32 + 40;
                        let h_radius = 1.0 + rng.next_float() as f64 * 4.0;
                        let v_radius = 0.5 + rng.next_float() as f64 * 2.0;
                        let yaw = rng.next_float() as f64 * PI * 2.0;
                        let pitch = (rng.next_float() as f64 - 0.5) * PI / 4.0;

                        self.carve_branch(
                            chunk,
                            &mut rng,
                            start_x,
                            start_y,
                            start_z,
                            h_radius,
                            v_radius,
                            yaw,
                            pitch,
                            branch_count,
                            0,
                            fluid_picker,
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cave_carver_creation() {
        let carver = CaveCarver::new();
        assert!(carver.config.probability > 0.0);
    }

    #[test]
    fn test_cave_carver_determinism() {
        let carver = CaveCarver::new();
        let result1 = carver.is_start_chunk(12345, 0, 0);
        let result2 = carver.is_start_chunk(12345, 0, 0);
        assert_eq!(result1, result2, "Cave carver should be deterministic");
    }

    #[test]
    fn test_should_carve_block() {
        let carver = CaveCarver::new();
        assert!(carver.should_carve_block(*blocks::STONE));
        assert!(carver.should_carve_block(*blocks::DEEPSLATE));
        assert!(!carver.should_carve_block(*blocks::AIR));
        assert!(!carver.should_carve_block(*blocks::WATER));
    }
}
