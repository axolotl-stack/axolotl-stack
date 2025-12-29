//! Cave and canyon carvers for world generation.
//!
//! Carvers are applied after basic terrain generation to create caves and
//! canyons. They operate on chunks and carve out air spaces based on noise
//! and geometric algorithms.
//!
//! ## Carver Types
//!
//! - **Cave Carver**: Creates winding cave tunnels underground
//! - **Canyon Carver**: Creates deep ravines from the surface
//!
//! ## Generation Process
//!
//! Carvers are applied in the `CARVERS` step of world generation:
//! 1. Determine if chunk is a carver start chunk
//! 2. Generate carver positions/parameters from seed
//! 3. Carve air pockets respecting aquifer boundaries
//!
//! ## Java Parity
//!
//! This implementation aims to match vanilla Java Edition's carver behavior
//! for deterministic world generation.

mod cave;
mod canyon;

pub use cave::CaveCarver;
pub use canyon::CanyonCarver;

use crate::world::chunk::Chunk;
use crate::world::generator::aquifer::FluidPicker;

/// Trait for world carvers.
///
/// Carvers modify chunks by carving out air spaces for caves and canyons.
pub trait WorldCarver: Send + Sync {
    /// Check if this chunk is a starting point for the carver.
    ///
    /// Starting chunks initiate carving that may extend into neighboring chunks.
    fn is_start_chunk(&self, seed: i64, chunk_x: i32, chunk_z: i32) -> bool;

    /// Carve the chunk.
    ///
    /// This may be called even if `is_start_chunk` is false, as carving from
    /// neighboring chunks can extend into this chunk.
    fn carve(
        &self,
        chunk: &mut Chunk,
        seed: i64,
        chunk_x: i32,
        chunk_z: i32,
        fluid_picker: Option<&dyn FluidPicker>,
    );
}

/// Configuration for cave carvers.
#[derive(Debug, Clone)]
pub struct CaveCarverConfig {
    /// Probability of a chunk being a cave start point (0.0 to 1.0).
    pub probability: f32,
    /// Minimum Y level for caves.
    pub min_y: i32,
    /// Maximum Y level for caves.
    pub max_y: i32,
    /// Horizontal radius multiplier.
    pub horizontal_radius_multiplier: f64,
    /// Vertical radius multiplier.
    pub vertical_radius_multiplier: f64,
    /// Y scale for the floor cutoff.
    pub floor_level: f64,
}

impl Default for CaveCarverConfig {
    fn default() -> Self {
        Self {
            probability: 0.14285715,
            min_y: -64,
            max_y: 180,
            horizontal_radius_multiplier: 1.0,
            vertical_radius_multiplier: 1.0,
            floor_level: -0.7,
        }
    }
}

/// Configuration for canyon carvers.
#[derive(Debug, Clone)]
pub struct CanyonCarverConfig {
    /// Probability of a chunk being a canyon start point.
    pub probability: f32,
    /// Minimum Y level for canyons.
    pub min_y: i32,
    /// Maximum Y level for canyons.
    pub max_y: i32,
    /// Vertical rotation range.
    pub vertical_rotation_range: f64,
    /// Shape configuration.
    pub shape: CanyonShape,
}

/// Canyon shape configuration.
#[derive(Debug, Clone)]
pub struct CanyonShape {
    /// Distance factor.
    pub distance_factor: f64,
    /// Thickness.
    pub thickness: f64,
    /// Width factor.
    pub width_smoothness: i32,
    /// Horizontal radius multiplier.
    pub horizontal_radius_factor: f64,
    /// Vertical radius default factor.
    pub vertical_radius_default_factor: f64,
    /// Vertical radius center factor.
    pub vertical_radius_center_factor: f64,
}

impl Default for CanyonCarverConfig {
    fn default() -> Self {
        Self {
            probability: 0.02,
            min_y: 10,
            max_y: 67,
            vertical_rotation_range: 3.0,
            shape: CanyonShape {
                distance_factor: 0.0,
                thickness: 3.0,
                width_smoothness: 3,
                horizontal_radius_factor: 1.0,
                vertical_radius_default_factor: 1.0,
                vertical_radius_center_factor: 0.0,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cave_config_default() {
        let config = CaveCarverConfig::default();
        assert!(config.probability > 0.0 && config.probability < 1.0);
        assert!(config.min_y < config.max_y);
    }

    #[test]
    fn test_canyon_config_default() {
        let config = CanyonCarverConfig::default();
        assert!(config.probability > 0.0 && config.probability < 1.0);
        assert!(config.min_y < config.max_y);
    }
}
