//! Density function system for 3D terrain generation.
//!
//! This module provides AOT-compiled density functions generated from Minecraft's
//! worldgen JSON files. The system evaluates 3D density values where positive
//! values indicate solid blocks and non-positive values indicate air/fluid.
//!
//! ## AOT Compiled Functions
//!
//! For maximum performance, the `unastar_noise` crate provides ahead-of-time
//! compiled density functions as flat Rust code. This eliminates tree traversal
//! overhead and enables better LLVM optimization.

mod caching;
mod chunk;
mod context;
mod types;

// Re-export core types (static, hand-written code)
pub use types::NoiseRegistry;

// Re-export types from unastar_noise
pub use unastar_noise::{FunctionContext, FunctionContext4, RarityType, NoiseSource};

// Re-export generated types (from unastar_noise)
pub use unastar_noise::{NoiseParamsData, NoiseRef, NOISE_PARAMS};

// Re-export AOT compiled functions and types from unastar_noise
pub use unastar_noise::{
    FlatCacheGrid, ColumnContext, ColumnContextGrid,
    compute_barrier, compute_continents, compute_depth, compute_erosion,
    compute_final_density, compute_final_density_4,
    compute_fluid_level_floodedness, compute_fluid_level_spread,
    compute_preliminary_surface_level, compute_lava,
    compute_ridges, compute_temperature, compute_vegetation,
    compute_vein_gap, compute_vein_ridged, compute_vein_toggle,
};

// Re-export context types
pub use context::{ContextProvider, SinglePointContext};

// Re-export NoiseChunk and CellInterpolator
pub use chunk::{CellInterpolator, NoiseChunk};

// Re-export caching types
pub use caching::{CachingNoiseChunk, ChunkCache};

// Helper function for linear interpolation
#[inline]
pub fn lerp(t: f64, a: f64, b: f64) -> f64 {
    a + t * (b - a)
}

// Helper for trilinear interpolation
#[inline]
pub fn lerp3(tx: f64, ty: f64, tz: f64, v000: f64, v100: f64, v010: f64, v110: f64, v001: f64, v101: f64, v011: f64, v111: f64) -> f64 {
    lerp(tz,
        lerp(ty, lerp(tx, v000, v100), lerp(tx, v010, v110)),
        lerp(ty, lerp(tx, v001, v101), lerp(tx, v011, v111))
    )
}

/// Find the top surface Y level by searching downward from upper_bound to lower_bound.
///
/// This implements the FindTopSurface density function which searches for the first
/// Y level (from top to bottom) where the density becomes positive.
///
/// # Arguments
/// * `block_x` - Block X coordinate
/// * `block_z` - Block Z coordinate
/// * `lower_bound` - Minimum Y to search (inclusive)
/// * `upper_bound` - Maximum Y to search (inclusive)
/// * `cell_height` - Y step size for the search (typically 8)
/// * `density_fn` - Function that computes density at a given Y level
///
/// # Returns
/// The Y coordinate where density first becomes positive (>= 0.0), or lower_bound if not found.
#[inline]
pub fn find_top_surface<F>(
    _block_x: i32,
    _block_z: i32,
    lower_bound: i32,
    upper_bound: i32,
    cell_height: i32,
    mut density_fn: F,
) -> f64
where
    F: FnMut(i32) -> f64,
{
    // Java: Mth.floor(upperBound / cellHeight) * cellHeight
    // Round upper_bound down to nearest multiple of cell_height
    let mut y = (upper_bound / cell_height) * cell_height;

    // Java: if (i <= this.lowerBound) return this.lowerBound
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
