#![feature(portable_simd)]

use std::cell::Cell;
use std::simd::f64x4;
use unastar_noise::{
    ColumnContext, ColumnContext4, FinalDensityCache4, FlatCacheGrid, FunctionContext,
    FunctionContext4, NoiseRef, NoiseSource, compute_final_density_4, compute_vein_toggle,
};

#[derive(Default)]
struct CountingNoise {
    samples: Cell<usize>,
    noodle_samples: Cell<usize>,
    jagged_simd_samples: Cell<usize>,
    jagged_negative_y_lanes: Cell<u8>,
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

    fn sample_4(&self, noise: NoiseRef, _x: f64x4, y: f64x4, _z: f64x4) -> f64x4 {
        self.samples.set(self.samples.get() + 1);
        if noise == NoiseRef::Noodle {
            self.noodle_samples.set(self.noodle_samples.get() + 1);
        }
        if noise == NoiseRef::Jagged {
            self.jagged_simd_samples
                .set(self.jagged_simd_samples.get() + 1);
            self.jagged_negative_y_lanes.set(
                y.to_array()
                    .into_iter()
                    .enumerate()
                    .fold(0, |bits, (lane, value)| {
                        bits | ((value.is_sign_negative() as u8) << lane)
                    }),
            );
        }
        f64x4::splat(0.375)
    }

    fn sample_end_islands(&self, _block_x: i32, _block_z: i32) -> f64 {
        panic!("Overworld density fixture must not sample End islands")
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
    let column_noise = FinalDensityCache4::default();
    for y in [-64, 321] {
        noise.noodle_samples.set(0);
        compute_final_density_4(
            &FunctionContext4::new(-32, [y; 4], 16),
            &noise,
            &flat,
            &column,
            &column_noise,
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
        &FinalDensityCache4::default(),
    )
    .to_array();
    for (lane, y) in heights.into_iter().enumerate() {
        let uniform = compute_final_density_4(
            &FunctionContext4::new(-32, [y; 4], 16),
            &noise,
            &flat,
            &column,
            &FinalDensityCache4::default(),
        );
        assert_eq!(mixed[lane].to_bits(), uniform[0].to_bits(), "y={y}");
    }
}

#[test]
fn zero_y_noise_is_reused_across_simd_density_batches() {
    let noise = CountingNoise::default();
    let flat = FlatCacheGrid::new(-2, 1, &noise);
    let column = ColumnContext4::splat(&ColumnContext::new(-32, 16, &noise, &flat));
    let column_noise = FinalDensityCache4::default();
    noise.jagged_simd_samples.set(0);
    for heights in [[0, 8, 16, 24], [32, 40, 48, 56], [64, 72, 80, 88]] {
        compute_final_density_4(
            &FunctionContext4::new(-32, heights, 16),
            &noise,
            &flat,
            &column,
            &column_noise,
        );
    }
    assert_eq!(noise.jagged_simd_samples.get(), 1);
}

#[test]
fn column_memo_distinguishes_signed_zero_lanes_and_coordinates() {
    let noise = CountingNoise::default();
    let column_noise = FinalDensityCache4::default();
    let inputs = [
        (-32, 16, [-32, -24, -16, -8], 1, 0b1111),
        (-32, 16, [-16, -8, -2, -1], 1, 0b1111),
        (-32, 16, [0, 8, 16, 24], 2, 0),
        (-32, 16, [32, 40, 48, 56], 2, 0),
        (-32, 16, [-1, 0, -2, 1], 3, 0b0101),
        (-32, 16, [-2, 0, -1, 2], 3, 0b0101),
        (-32, 16, [0, -1, 2, -2], 4, 0b1010),
        (-32, 16, [1, -2, 0, -1], 4, 0b1010),
        (-16, 16, [0, 8, 16, 24], 5, 0),
        (-16, 16, [16, 24, 32, 40], 5, 0),
        (-16, -16, [0, 8, 16, 24], 6, 0),
        (-16, -16, [16, 24, 32, 40], 6, 0),
    ];
    for (x, z, heights, samples, negative_y_lanes) in inputs {
        let flat = FlatCacheGrid::new(x >> 4, z >> 4, &noise);
        let column = ColumnContext4::splat(&ColumnContext::new(x, z, &noise, &flat));
        compute_final_density_4(
            &FunctionContext4::new(x, heights, z),
            &noise,
            &flat,
            &column,
            &column_noise,
        );
        assert_eq!(noise.jagged_simd_samples.get(), samples);
        assert_eq!(noise.jagged_negative_y_lanes.get(), negative_y_lanes);
    }
}
