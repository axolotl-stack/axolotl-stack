#![feature(portable_simd)]

use std::cell::Cell;
use std::simd::f64x4;
use unastar_noise::{
    ColumnContext, ColumnContext4, FlatCacheGrid, FunctionContext, FunctionContext4, NoiseRef,
    NoiseSource, compute_final_density_4, compute_vein_toggle,
};

#[derive(Default)]
struct CountingNoise {
    samples: Cell<usize>,
    noodle_samples: Cell<usize>,
}

#[allow(clippy::too_many_arguments)]
impl NoiseSource for CountingNoise {
    fn sample(&self, noise: NoiseRef, _x: f64, _y: f64, _z: f64) -> f64 {
        self.samples.set(self.samples.get() + 1);
        if noise == NoiseRef::Noodle {
            self.noodle_samples.set(self.noodle_samples.get() + 1);
        }
        0.375
    }

    fn sample_4(&self, noise: NoiseRef, _x: f64x4, _y: f64x4, _z: f64x4) -> f64x4 {
        self.samples.set(self.samples.get() + 1);
        if noise == NoiseRef::Noodle {
            self.noodle_samples.set(self.noodle_samples.get() + 1);
        }
        f64x4::splat(0.375)
    }

    fn sample_blended_noise(
        &self,
        _x: f64,
        _y: f64,
        _z: f64,
        _xz_scale: f64,
        _y_scale: f64,
        _xz_factor: f64,
        _y_factor: f64,
        _smear_scale_multiplier: f64,
    ) -> f64 {
        0.0
    }

    fn sample_blended_noise_4(
        &self,
        _x: f64,
        _y: f64x4,
        _z: f64,
        _xz_scale: f64,
        _y_scale: f64,
        _xz_factor: f64,
        _y_factor: f64,
        _smear_scale_multiplier: f64,
    ) -> f64x4 {
        f64x4::splat(0.0)
    }
}

#[test]
fn range_choice_does_not_sample_the_unselected_vein_branch() {
    for y in [-64, -61, 51, 319] {
        let noise = CountingNoise::default();
        let value = compute_vein_toggle(&FunctionContext::new(-17, y, 31), &noise);
        assert_eq!(value.to_bits(), 0.0_f64.to_bits(), "y={y}");
        assert_eq!(noise.samples.get(), 0, "unselected noise branch at y={y}");
    }
}

#[test]
fn range_choice_keeps_inclusive_lower_and_exclusive_upper_boundaries() {
    for y in [-60, -59, 0, 49, 50] {
        let noise = CountingNoise::default();
        let value = compute_vein_toggle(&FunctionContext::new(-17, y, 31), &noise);
        assert_eq!(value.to_bits(), 0.375_f64.to_bits(), "y={y}");
        assert_eq!(noise.samples.get(), 1, "selected noise branch at y={y}");
    }
}

#[test]
fn uniform_simd_mask_skips_out_of_range_noodle_noise() {
    let noise = CountingNoise::default();
    let flat = FlatCacheGrid::new(-2, 1, &noise);
    let column = ColumnContext4::splat(&ColumnContext::new(-32, 16, &noise, &flat));
    for y in [-64, 321] {
        noise.noodle_samples.set(0);
        compute_final_density_4(
            &FunctionContext4::new(-32, [y; 4], 16),
            &noise,
            &flat,
            &column,
        );
        assert_eq!(noise.noodle_samples.get(), 0, "unselected noodle at y={y}");
    }
}

#[test]
fn mixed_simd_mask_matches_each_uniform_lane_at_range_boundaries() {
    let noise = CountingNoise::default();
    let flat = FlatCacheGrid::new(-2, 1, &noise);
    let column = ColumnContext4::splat(&ColumnContext::new(-32, 16, &noise, &flat));
    let heights = [-61, -60, 320, 321];
    let mixed = compute_final_density_4(
        &FunctionContext4::new(-32, heights, 16),
        &noise,
        &flat,
        &column,
    )
    .to_array();
    for (lane, y) in heights.into_iter().enumerate() {
        let uniform = compute_final_density_4(
            &FunctionContext4::new(-32, [y; 4], 16),
            &noise,
            &flat,
            &column,
        );
        assert_eq!(mixed[lane].to_bits(), uniform[0].to_bits(), "y={y}");
    }
}
