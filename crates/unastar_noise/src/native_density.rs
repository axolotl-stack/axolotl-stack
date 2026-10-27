//! Numeric primitives for native Java density caches. Calibrated terrain keeps
//! its existing cache and arithmetic; these entrypoints never change that profile.

use crate::{FunctionContext, FunctionContext4};
use std::simd::f64x4;

/// Geometry baked into a compiled dimension router.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoiseGeometry {
    pub min_y: i32,
    pub height: i32,
    pub cell_width: i32,
    pub cell_height: i32,
}

/// Native CubicSpline multipoint segment, retaining each float operation and its
/// order. Polynomial coefficient folding or f64 intermediates change native bits.
#[inline]
pub fn spline_segment(
    coordinate: f32,
    low: f32,
    high: f32,
    value_low: f32,
    value_high: f32,
    derivative_low: f32,
    derivative_high: f32,
) -> f32 {
    let width = high - low;
    let t = (coordinate - low) / width;
    let delta = value_high - value_low;
    let a = derivative_low * width - delta;
    let b = -derivative_high * width + delta;
    let linear = value_low + t * (value_high - value_low);
    linear + t * (1.0 - t) * (a + t * (b - a))
}

#[inline]
fn lerp(t: f64, a: f64, b: f64) -> f64 {
    a + t * (b - a)
}

/// Native CacheAllInCell bulk sampling uses X, then Y, then Z interpolation.
/// Corners are indexed by the three XYZ bits, with Z the low bit.
#[inline]
pub fn interpolate_bulk(c: &[f64; 8], x: f64, y: f64, z: f64) -> f64 {
    let low = lerp(y, lerp(x, c[0], c[4]), lerp(x, c[2], c[6]));
    let high = lerp(y, lerp(x, c[1], c[5]), lerp(x, c[3], c[7]));
    lerp(z, low, high)
}

/// Active NoiseInterpolator updates outside a cell fill use Y, then X, then Z.
/// Keeping it separate is observable in raw native material/vein density bits.
#[inline]
pub fn interpolate_active(c: &[f64; 8], x: f64, y: f64, z: f64) -> f64 {
    let low = lerp(x, lerp(y, c[0], c[2]), lerp(y, c[4], c[6]));
    let high = lerp(x, lerp(y, c[1], c[3]), lerp(y, c[5], c[7]));
    lerp(z, low, high)
}

/// Fixed, generated-size planes for schema-marked interpolated density nodes.
/// S is the source count; Z/Y include their terminal cell-corner positions.
/// No heap storage or per-block node lookup is needed.
pub struct InterpolationCache<const S: usize, const Z: usize, const Y: usize> {
    planes: [[[[f64; Y]; S]; Z]; 2],
    left: usize,
    corners: [[f64; 8]; S],
}

impl<const S: usize, const Z: usize, const Y: usize> Default for InterpolationCache<S, Z, Y> {
    fn default() -> Self {
        Self {
            planes: [[[[0.0; Y]; S]; Z]; 2],
            left: 0,
            corners: [[0.0; 8]; S],
        }
    }
}

impl<const S: usize, const Z: usize, const Y: usize> InterpolationCache<S, Z, Y> {
    /// Fill one plane, preserving native scalar bits in the SIMD batches and tail.
    pub fn fill(
        &mut self,
        right: bool,
        block_x: i32,
        first_block_z: i32,
        geometry: NoiseGeometry,
        mut scalar: impl FnMut(&FunctionContext) -> [f64; S],
        mut vector: impl FnMut(&FunctionContext4) -> [f64x4; S],
    ) {
        let plane = &mut self.planes[self.left ^ usize::from(right)];
        for (z, columns) in plane.iter_mut().enumerate() {
            let block_z = first_block_z + z as i32 * geometry.cell_width;
            let mut y = 0;
            while y + 4 <= Y {
                let ys = std::array::from_fn(|lane| {
                    geometry.min_y + (y + lane) as i32 * geometry.cell_height
                });
                let values = vector(&FunctionContext4::new(block_x, ys, block_z));
                for (column, value) in columns.iter_mut().zip(values) {
                    column[y..y + 4].copy_from_slice(&value.to_array());
                }
                y += 4;
            }
            while y < Y {
                let block_y = geometry.min_y + y as i32 * geometry.cell_height;
                let values = scalar(&FunctionContext::new(block_x, block_y, block_z));
                for (column, value) in columns.iter_mut().zip(values) {
                    column[y] = value;
                }
                y += 1;
            }
        }
    }

    #[inline]
    pub fn swap_planes(&mut self) {
        self.left ^= 1;
    }

    /// Select all source corners once before filling/evaluating the material cell.
    pub fn select_cell(&mut self, cell_y: usize, cell_z: usize) {
        assert!(
            cell_y + 1 < Y && cell_z + 1 < Z,
            "cell outside interpolation planes"
        );
        let a = &self.planes[self.left];
        let b = &self.planes[self.left ^ 1];
        for (source, corners) in self.corners.iter_mut().enumerate() {
            *corners = [
                a[cell_z][source][cell_y],
                a[cell_z + 1][source][cell_y],
                a[cell_z][source][cell_y + 1],
                a[cell_z + 1][source][cell_y + 1],
                b[cell_z][source][cell_y],
                b[cell_z + 1][source][cell_y],
                b[cell_z][source][cell_y + 1],
                b[cell_z + 1][source][cell_y + 1],
            ];
        }
    }

    #[inline]
    pub fn bulk_values(&self, x: f64, y: f64, z: f64) -> [f64; S] {
        std::array::from_fn(|source| interpolate_bulk(&self.corners[source], x, y, z))
    }

    #[inline]
    pub fn active_values(&self, x: f64, y: f64, z: f64) -> [f64; S] {
        std::array::from_fn(|source| interpolate_active(&self.corners[source], x, y, z))
    }
}

#[cfg(test)]
mod tests {
    use super::{interpolate_active, interpolate_bulk, spline_segment};

    #[test]
    fn native_material_cache_uses_bulk_not_active_interpolation_order() {
        // Passive captures of real Java 1.21.11 fillFromNoise, seed 0.
        // The second point distinguishes both valid native interpolation modes.
        for (corners, xyz, expected, active_expected) in [
            (
                [
                    0x3fdcf8a4317f034a,
                    0x3fecbed396149110,
                    0xbfcabc3a419d598a,
                    0xbfbdbc2f9a54f02a,
                    0x3fe19c4833930431,
                    0x3fe95df89c0df039,
                    0xbfc3fb90732eb81c,
                    0xbfbc8fa778392e4e,
                ],
                [0.0, 0.75, 0.25],
                0x3f426065381826f9_u64,
                0x3f426065381826f9_u64,
            ),
            (
                [
                    0x3fe19c4833930431,
                    0x3fe95df89c0df039,
                    0xbfc3fb90732eb81c,
                    0xbfbc8fa778392e4e,
                    0x3fe6d7e2e136ed5f,
                    0x3fe72b3e603a09ba,
                    0xbfb92f27ba330706,
                    0xbfb4a2099bd0766e,
                ],
                [0.75, 0.875, 0.5],
                0x3f200bd1dded487e_u64,
                0x3f200bd1dded45ef_u64,
            ),
        ] {
            let corners = corners.map(f64::from_bits);
            let [x, y, z] = xyz;
            let material = |density: f64| {
                let value = (density * 0.64).clamp(-1.0, 1.0);
                value / 2.0 - value * value * value / 24.0
            };
            assert_eq!(
                material(interpolate_bulk(&corners, x, y, z)).to_bits(),
                expected
            );
            assert_eq!(
                material(interpolate_active(&corners, x, y, z)).to_bits(),
                active_expected
            );
        }
    }

    #[test]
    fn native_float_spline_segments_preserve_measured_bits() {
        let rows: Vec<[u32; 8]> = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/java_spline_segments.json",
        )))
        .unwrap();
        for row in rows {
            let [x, low, high, a, b, da, db, expected] = row;
            assert_eq!(
                spline_segment(
                    f32::from_bits(x),
                    f32::from_bits(low),
                    f32::from_bits(high),
                    f32::from_bits(a),
                    f32::from_bits(b),
                    f32::from_bits(da),
                    f32::from_bits(db),
                )
                .to_bits(),
                expected,
                "native f32 spline coordinate={x:08x}"
            );
        }
    }
}
