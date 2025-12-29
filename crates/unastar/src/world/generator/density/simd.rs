use std::arch::x86_64::*;
use super::math::Clamp;
// The "Hot" Trait
pub trait SimdDensityFunction {
    // Instead of [f64; 4], we return the raw intrinsic type
    unsafe fn compute_avx2(&self, x: __m256d, y: __m256d, z: __m256d) -> __m256d;
}