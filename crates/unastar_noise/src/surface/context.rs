//! Surface rule evaluation context.
//!
//! The [`SurfaceContext`] provides all the information needed for evaluating
//! surface rules at a given block position.

use crate::Biome;

/// Which surface of a cave we're checking (floor or ceiling).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaveSurface {
    /// Floor of a cave (checking stone depth above).
    Floor,
    /// Ceiling of a cave (checking stone depth below).
    Ceiling,
}

/// Vertical anchor for Y coordinate checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalAnchor {
    /// Absolute Y coordinate.
    Absolute(i32),
    /// Y coordinate above world bottom.
    AboveBottom(i32),
    /// Y coordinate below world top.
    BelowTop(i32),
}

impl VerticalAnchor {
    /// Resolve the anchor to an absolute Y coordinate.
    pub fn resolve(&self, min_y: i32, max_y: i32) -> i32 {
        match self {
            VerticalAnchor::Absolute(y) => *y,
            VerticalAnchor::AboveBottom(offset) => min_y + offset,
            VerticalAnchor::BelowTop(offset) => max_y - offset,
        }
    }
}

/// Context for surface rule evaluation.
///
/// Contains all the information needed to evaluate surface conditions
/// at a specific block position.
#[derive(Debug, Clone)]
pub struct SurfaceContext {
    // Position
    /// World X coordinate.
    pub block_x: i32,
    /// World Y coordinate.
    pub block_y: i32,
    /// World Z coordinate.
    pub block_z: i32,

    // Surface data
    /// Depth of the surface layer (from noise).
    pub surface_depth: i32,
    /// Secondary surface noise value (for variation).
    pub surface_secondary: f64,
    /// Water level at this position.
    pub water_height: i32,
    /// Number of solid blocks above this position (stone depth from floor).
    pub stone_depth_above: i32,
    /// Number of solid blocks below this position (stone depth from ceiling).
    pub stone_depth_below: i32,

    // Computed values
    /// Whether this position is on a steep slope.
    pub steep: bool,
    /// Biome at this position.
    pub biome: Biome,
    /// Minimum surface level (used for some conditions).
    pub min_surface_level: i32,

    // World bounds
    /// Minimum Y coordinate of the world.
    pub min_y: i32,
    /// Maximum Y coordinate of the world.
    pub max_y: i32,

    // Cache invalidation
    /// Counter for tracking XZ updates.
    pub last_update_xz: u64,
    /// Counter for tracking Y updates.
    pub last_update_y: u64,

    // Chunk position for local coordinate calculations
    chunk_x: i32,
    chunk_z: i32,
}

impl Default for SurfaceContext {
    fn default() -> Self {
        Self {
            block_x: 0,
            block_y: 0,
            block_z: 0,
            surface_depth: 0,
            surface_secondary: 0.0,
            water_height: i32::MIN,
            stone_depth_above: 0,
            stone_depth_below: 0,
            steep: false,
            biome: Biome::Plains,
            min_surface_level: -64,
            min_y: -64,
            max_y: 320,
            last_update_xz: 0,
            last_update_y: 0,
            chunk_x: 0,
            chunk_z: 0,
        }
    }
}

impl SurfaceContext {
    /// Create a new surface context for a chunk.
    pub fn new(chunk_x: i32, chunk_z: i32, min_y: i32, max_y: i32) -> Self {
        Self {
            chunk_x,
            chunk_z,
            min_y,
            max_y,
            ..Default::default()
        }
    }

    /// Update the context for a new XZ position.
    ///
    /// This updates the horizontal position and recalculates XZ-dependent
    /// values like surface depth and steep detection.
    pub fn update_xz(
        &mut self,
        x: i32,
        z: i32,
        surface_depth: i32,
        surface_secondary: f64,
        steep: bool,
        min_surface_level: i32,
    ) {
        self.last_update_xz += 1;
        self.last_update_y += 1;

        self.block_x = x;
        self.block_z = z;
        self.surface_depth = surface_depth;
        self.surface_secondary = surface_secondary;
        self.steep = steep;
        self.min_surface_level = min_surface_level;
    }

    /// Update the context for a new Y position.
    ///
    /// This updates the vertical position and Y-dependent values like
    /// stone depths and biome.
    pub fn update_y(
        &mut self,
        y: i32,
        stone_depth_above: i32,
        stone_depth_below: i32,
        water_height: i32,
        biome: Biome,
    ) {
        self.last_update_y += 1;

        self.block_y = y;
        self.stone_depth_above = stone_depth_above;
        self.stone_depth_below = stone_depth_below;
        self.water_height = water_height;
        self.biome = biome;
    }

    /// Create a test context with specified values.
    #[cfg(test)]
    pub fn test_context(x: i32, y: i32, z: i32, biome: Biome) -> Self {
        Self {
            block_x: x,
            block_y: y,
            block_z: z,
            biome,
            min_y: -64,
            max_y: 320,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vertical_anchor_resolve() {
        let min_y = -64;
        let max_y = 320;

        assert_eq!(VerticalAnchor::Absolute(63).resolve(min_y, max_y), 63);
        assert_eq!(VerticalAnchor::AboveBottom(10).resolve(min_y, max_y), -54);
        assert_eq!(VerticalAnchor::BelowTop(10).resolve(min_y, max_y), 310);
    }

    #[test]
    fn test_surface_context_update_xz() {
        let mut ctx = SurfaceContext::new(0, 0, -64, 320);
        let initial_xz = ctx.last_update_xz;
        let initial_y = ctx.last_update_y;

        ctx.update_xz(10, 20, 3, 0.5, true, 60);

        assert_eq!(ctx.block_x, 10);
        assert_eq!(ctx.block_z, 20);
        assert_eq!(ctx.surface_depth, 3);
        assert!((ctx.surface_secondary - 0.5).abs() < f64::EPSILON);
        assert!(ctx.steep);
        assert_eq!(ctx.min_surface_level, 60);
        assert!(ctx.last_update_xz > initial_xz);
        assert!(ctx.last_update_y > initial_y);
    }

    #[test]
    fn test_surface_context_update_y() {
        let mut ctx = SurfaceContext::new(0, 0, -64, 320);
        let initial_y = ctx.last_update_y;

        ctx.update_y(64, 2, 5, 63, Biome::Desert);

        assert_eq!(ctx.block_y, 64);
        assert_eq!(ctx.stone_depth_above, 2);
        assert_eq!(ctx.stone_depth_below, 5);
        assert_eq!(ctx.water_height, 63);
        assert_eq!(ctx.biome, Biome::Desert);
        assert!(ctx.last_update_y > initial_y);
    }
}
