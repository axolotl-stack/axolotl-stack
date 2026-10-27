//! Core types for worldgen density functions.
//!
//! These types are used by the generated AOT-compiled density functions.

use std::simd::prelude::*;

/// Context for density function evaluation at a single point.
#[derive(Debug, Clone, Copy)]
pub struct FunctionContext {
    pub block_x: i32,
    pub block_y: i32,
    pub block_z: i32,
}

impl FunctionContext {
    #[inline]
    pub fn new(x: i32, y: i32, z: i32) -> Self {
        Self {
            block_x: x,
            block_y: y,
            block_z: z,
        }
    }
}

/// Context for 4 Y positions at once (for SIMD evaluation).
#[derive(Debug, Clone, Copy)]
pub struct FunctionContext4 {
    pub block_x: i32,
    pub block_y: [i32; 4],
    pub block_z: i32,
}

impl FunctionContext4 {
    #[inline]
    pub fn new(x: i32, y: [i32; 4], z: i32) -> Self {
        Self {
            block_x: x,
            block_y: y,
            block_z: z,
        }
    }
}

/// Rarity type for weird scaled sampler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RarityType {
    Type1,
    Type2,
}

/// Trait for noise registry - allows the caller to provide their own implementation.
///
/// The generated code uses this trait to sample noise values. The actual implementation
/// (`NoiseRegistry`) lives in the `unastar` crate since it depends on noise generation code.
pub trait NoiseSource {
    /// Select native Java root/cache arithmetic statically, never per-block dispatch.
    const NATIVE_JAVA: bool;

    /// Actual structure influence at this block. Plain/unstructured sources return
    /// zero explicitly; structured native sources must provide their real engine.
    fn sample_beardifier(&self, block_x: i32, block_y: i32, block_z: i32) -> f64;

    /// Sample noise at a 3D position.
    fn sample(&self, noise_ref: super::NoiseRef, x: f64, y: f64, z: f64) -> f64;

    /// Sample noise at 4 Y positions (SIMD).
    fn sample_4(&self, noise_ref: super::NoiseRef, x: f64x4, y: f64x4, z: f64x4) -> f64x4;

    /// Seeded End-islands density at block X/Z coordinates; independent of Y.
    fn sample_end_islands(&self, block_x: i32, block_z: i32) -> f64;

    /// Sample old blended noise (base_3d_noise).
    ///
    /// This implements the legacy blended noise algorithm used for base terrain variation.
    #[allow(clippy::too_many_arguments)]
    fn sample_blended_noise(
        &self,
        x: f64,
        y: f64,
        z: f64,
        xz_scale: f64,
        y_scale: f64,
        xz_factor: f64,
        y_factor: f64,
        smear_scale_multiplier: f64,
    ) -> f64;

    /// Sample old blended noise (SIMD version for 4 Y positions).
    #[allow(clippy::too_many_arguments)]
    fn sample_blended_noise_4(
        &self,
        x: f64,
        y: f64x4,
        z: f64,
        xz_scale: f64,
        y_scale: f64,
        xz_factor: f64,
        y_factor: f64,
        smear_scale_multiplier: f64,
    ) -> f64x4;
}

/// Find the Y level where density becomes positive (first solid block from top).
///
/// This implements the FindTopSurface density function which searches for the first
/// Y level (from top to bottom) where the density becomes positive, indicating solid terrain.
///
/// # Arguments
/// * `_block_x` - Block X coordinate (for debugging)
/// * `_block_z` - Block Z coordinate (for debugging)
/// * `lower_bound` - Minimum Y to search (inclusive)
/// * `upper_bound` - Upper bound of the search (density function value)
/// * `cell_height` - Y step size for the search (typically 8)
/// * `density_fn` - Function that computes density at a given Y level
///
/// # Returns
/// The Y coordinate where density first becomes positive (> 0.0), or lower_bound if not found.
#[inline]
pub fn find_top_surface<F>(
    _block_x: i32,
    _block_z: i32,
    lower_bound: i32,
    upper_bound: f64,
    cell_height: i32,
    mut density_fn: F,
) -> f64
where
    F: FnMut(i32) -> f64,
{
    // Round the upper bound down to a multiple of the cell height (floor, also for negatives).
    let mut y = (upper_bound / cell_height as f64).floor() as i32 * cell_height;

    if y <= lower_bound {
        return lower_bound as f64;
    }

    // Search from top to bottom, stepping by cell_height
    while y >= lower_bound {
        let density = density_fn(y);

        // Found the surface: first Y where density becomes positive
        if density > 0.0 {
            return y as f64;
        }

        y -= cell_height;
    }

    // No surface found in range, return lower bound
    lower_bound as f64
}

#[cfg(test)]
mod tests {
    use super::find_top_surface;

    #[test]
    fn negative_upper_bound_rounds_down_to_the_cell_below() {
        // floor(-3.5 / 8) * 8 = -8; truncating toward zero would start at 0.
        let top = find_top_surface(0, 0, -64, -3.5, 8, |_| 1.0);
        assert_eq!(top, -8.0);
    }
}
