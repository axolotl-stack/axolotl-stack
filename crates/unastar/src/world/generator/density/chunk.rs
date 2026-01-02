//! NoiseChunk - the cell-based interpolation engine.
//!
//! NoiseChunk manages chunk-level state for density function evaluation.
//! Uses AOT-compiled density functions for maximum performance.
//!
//! ## Cell Structure
//!
//! A chunk (16x384x16 blocks) is divided into cells:
//! - Cell width: 4 blocks (4 cells per chunk in XZ)
//! - Cell height: 8 blocks (48 cells for 384 block height)
//!
//! Density is evaluated at cell corners and trilinearly interpolated
//! for interior block positions.

use unastar_noise::{FunctionContext, FlatCacheGrid, ColumnContext, NoiseSource, compute_final_density};
use super::lerp3;

/// Cell-based noise chunk for caching and interpolation.
///
/// This is the main orchestrator for density function evaluation within a chunk.
/// It manages cell configuration and traversal state.
pub struct NoiseChunk {
    // Cell configuration
    /// Width of each cell in blocks (typically 4).
    cell_width: i32,
    /// Height of each cell in blocks (typically 8).
    cell_height: i32,
    /// Number of cells in X/Z direction.
    cell_count_xz: i32,
    /// Number of cells in Y direction.
    cell_count_y: i32,

    // World position
    /// Minimum X block coordinate of this chunk.
    min_block_x: i32,
    /// Minimum Y block coordinate of this chunk.
    min_block_y: i32,
    /// Minimum Z block coordinate of this chunk.
    min_block_z: i32,

    // Current cell position state
    /// Current cell X index.
    cell_x: i32,
    /// Current cell Y index.
    cell_y: i32,
    /// Current cell Z index.
    cell_z: i32,

    // Position within cell
    /// Current position within cell (0 to cell_width-1).
    in_cell_x: i32,
    /// Current position within cell (0 to cell_height-1).
    in_cell_y: i32,
    /// Current position within cell (0 to cell_width-1).
    in_cell_z: i32,
}

impl NoiseChunk {
    /// Create a new NoiseChunk for the given chunk position.
    ///
    /// # Arguments
    /// * `chunk_x` - Chunk X coordinate
    /// * `chunk_z` - Chunk Z coordinate
    /// * `cell_width` - Width of each cell in blocks (typically 4)
    /// * `cell_height` - Height of each cell in blocks (typically 8)
    /// * `min_y` - Minimum Y coordinate (e.g., -64)
    /// * `height` - Total height in blocks (e.g., 384)
    pub fn new(
        chunk_x: i32,
        chunk_z: i32,
        cell_width: i32,
        cell_height: i32,
        min_y: i32,
        height: i32,
    ) -> Self {
        let cell_count_xz = 16 / cell_width;
        let cell_count_y = height / cell_height;

        Self {
            cell_width,
            cell_height,
            cell_count_xz,
            cell_count_y,
            min_block_x: chunk_x * 16,
            min_block_y: min_y,
            min_block_z: chunk_z * 16,
            cell_x: 0,
            cell_y: 0,
            cell_z: 0,
            in_cell_x: 0,
            in_cell_y: 0,
            in_cell_z: 0,
        }
    }

    /// Get cell dimensions.
    pub fn cell_width(&self) -> i32 {
        self.cell_width
    }

    pub fn cell_height(&self) -> i32 {
        self.cell_height
    }

    pub fn cell_count_xz(&self) -> i32 {
        self.cell_count_xz
    }

    pub fn cell_count_y(&self) -> i32 {
        self.cell_count_y
    }

    /// Get current block X coordinate.
    pub fn block_x(&self) -> i32 {
        self.min_block_x + self.cell_x * self.cell_width + self.in_cell_x
    }

    /// Get current block Y coordinate.
    pub fn block_y(&self) -> i32 {
        self.min_block_y + self.cell_y * self.cell_height + self.in_cell_y
    }

    /// Get current block Z coordinate.
    pub fn block_z(&self) -> i32 {
        self.min_block_z + self.cell_z * self.cell_width + self.in_cell_z
    }

    /// Get the current position as a FunctionContext.
    pub fn context(&self) -> FunctionContext {
        FunctionContext::new(self.block_x(), self.block_y(), self.block_z())
    }

    /// Set cell position.
    pub fn set_cell(&mut self, cell_x: i32, cell_y: i32, cell_z: i32) {
        self.cell_x = cell_x;
        self.cell_y = cell_y;
        self.cell_z = cell_z;
    }

    /// Set position within cell.
    pub fn set_in_cell(&mut self, x: i32, y: i32, z: i32) {
        self.in_cell_x = x;
        self.in_cell_y = y;
        self.in_cell_z = z;
    }

}

/// Interpolator that caches cell corner values for trilinear interpolation.
///
/// This is used to efficiently evaluate density functions across a cell
/// by computing values at corners once and interpolating for interior points.
pub struct CellInterpolator {
    /// Cached corner values: [x0y0z0, x1y0z0, x0y1z0, x1y1z0, x0y0z1, x1y0z1, x0y1z1, x1y1z1]
    corners: [f64; 8],
    /// Cell dimensions
    cell_width: i32,
    cell_height: i32,
}

impl CellInterpolator {
    /// Create a new interpolator with the given cell dimensions.
    pub fn new(cell_width: i32, cell_height: i32) -> Self {
        Self {
            corners: [0.0; 8],
            cell_width,
            cell_height,
        }
    }

    /// Set corner values directly (used when values are already computed).
    pub fn set_corners(&mut self, corners: [f64; 8]) {
        self.corners = corners;
    }

    /// Fill corner values for a cell using AOT compiled final_density function.
    ///
    /// This samples the 8 corners of a cell using the AOT-compiled density function,
    /// which is significantly faster than interpreting a density function tree.
    pub fn fill_cell_aot(
        &mut self,
        base_x: i32,
        base_y: i32,
        base_z: i32,
        noises: &impl NoiseSource,
        grid: &FlatCacheGrid,
    ) {
        let w = self.cell_width;
        let h = self.cell_height;

        // Create ColumnContexts for each unique (X, Z) column
        // Corners use 4 unique columns: (base_x, base_z), (base_x+w, base_z), (base_x, base_z+w), (base_x+w, base_z+w)
        let col00 = ColumnContext::new(base_x, base_z, noises, grid);
        let col10 = ColumnContext::new(base_x + w, base_z, noises, grid);
        let col01 = ColumnContext::new(base_x, base_z + w, noises, grid);
        let col11 = ColumnContext::new(base_x + w, base_z + w, noises, grid);

        self.corners[0] = compute_final_density(&FunctionContext::new(base_x, base_y, base_z), noises, grid, &col00);
        self.corners[1] = compute_final_density(&FunctionContext::new(base_x + w, base_y, base_z), noises, grid, &col10);
        self.corners[2] = compute_final_density(&FunctionContext::new(base_x, base_y + h, base_z), noises, grid, &col00);
        self.corners[3] = compute_final_density(&FunctionContext::new(base_x + w, base_y + h, base_z), noises, grid, &col10);
        self.corners[4] = compute_final_density(&FunctionContext::new(base_x, base_y, base_z + w), noises, grid, &col01);
        self.corners[5] = compute_final_density(&FunctionContext::new(base_x + w, base_y, base_z + w), noises, grid, &col11);
        self.corners[6] = compute_final_density(&FunctionContext::new(base_x, base_y + h, base_z + w), noises, grid, &col01);
        self.corners[7] = compute_final_density(&FunctionContext::new(base_x + w, base_y + h, base_z + w), noises, grid, &col11);
    }

    /// Interpolate value at the given position within the cell.
    ///
    /// # Arguments
    /// * `x` - Position within cell (0 to cell_width-1)
    /// * `y` - Position within cell (0 to cell_height-1)
    /// * `z` - Position within cell (0 to cell_width-1)
    pub fn interpolate(&self, x: i32, y: i32, z: i32) -> f64 {
        let tx = x as f64 / self.cell_width as f64;
        let ty = y as f64 / self.cell_height as f64;
        let tz = z as f64 / self.cell_width as f64;

        lerp3(
            tx, ty, tz,
            self.corners[0], self.corners[1],
            self.corners[2], self.corners[3],
            self.corners[4], self.corners[5],
            self.corners[6], self.corners[7],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_noise_chunk_creation() {
        let chunk = NoiseChunk::new(0, 0, 4, 8, -64, 384);

        assert_eq!(chunk.cell_width(), 4);
        assert_eq!(chunk.cell_height(), 8);
        assert_eq!(chunk.cell_count_xz(), 4); // 16 / 4
        assert_eq!(chunk.cell_count_y(), 48); // 384 / 8
    }

    #[test]
    fn test_noise_chunk_block_coords() {
        let mut chunk = NoiseChunk::new(1, 2, 4, 8, -64, 384);

        // Initial position should be at min coords
        assert_eq!(chunk.block_x(), 16); // chunk_x * 16 = 1 * 16
        assert_eq!(chunk.block_y(), -64);
        assert_eq!(chunk.block_z(), 32); // chunk_z * 16 = 2 * 16

        // Update position within cell
        chunk.set_cell(1, 5, 0);
        chunk.set_in_cell(2, 3, 0);
        assert_eq!(chunk.block_x(), 16 + 4 + 2); // min + cell_x*4 + in_cell
        assert_eq!(chunk.block_y(), -64 + 40 + 3); // min + cell_y*8 + in_cell
    }

    #[test]
    fn test_cell_interpolator() {
        let interp = CellInterpolator::new(4, 8);

        // Test that dimensions are stored correctly
        assert_eq!(interp.cell_width, 4);
        assert_eq!(interp.cell_height, 8);
    }
}
