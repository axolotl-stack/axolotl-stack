//! Accurate Perlin and octave noise implementation based on cubiomes.
//! This implementation matches Minecraft's exact terrain generation algorithms.
//!
//! Uses portable SIMD for cross-platform vectorization.

use super::xoroshiro::Xoroshiro128;
use std::simd::prelude::*;
use std::simd::StdFloat;

/// 3D Perlin noise generator - cubiomes-accurate implementation.
#[derive(Debug, Clone)]
pub struct PerlinNoise {
    /// Permutation table (257 entries for wrapping).
    /// Stored as i32 for efficient SIMD gather operations.
    pub d: [i32; 257],
    /// X offset
    pub a: f64,
    /// Y offset
    pub b: f64,
    /// Z offset
    pub c: f64,
    /// Amplitude multiplier
    pub amplitude: f64,
    /// Lacunarity (frequency multiplier)
    pub lacunarity: f64,
    /// Precomputed floor(b) mod 256 (as i32 for SIMD compatibility)
    h2: i32,
    /// Precomputed b - floor(b)
    d2: f64,
    /// Precomputed smoothstep(d2)
    t2: f64,
}

impl Default for PerlinNoise {
    fn default() -> Self {
        Self {
            d: [0i32; 257],
            a: 0.0,
            b: 0.0,
            c: 0.0,
            amplitude: 1.0,
            lacunarity: 1.0,
            h2: 0i32,
            d2: 0.0,
            t2: 0.0,
        }
    }
}

impl PerlinNoise {
    /// Initialize Perlin noise from Xoroshiro RNG (cubiomes xPerlinInit).
    pub fn new(rng: &mut Xoroshiro128) -> Self {
        let a = rng.next_double() * 256.0;
        let b = rng.next_double() * 256.0;
        let c = rng.next_double() * 256.0;

        // Initialize permutation table as i32 for SIMD gather operations
        let mut d = [0i32; 257];

        // Initialize with identity
        for i in 0..256 {
            d[i] = i as i32;
        }

        // Fisher-Yates shuffle
        for i in 0..256 {
            let j = rng.next_int(256 - i as u32) as usize + i;
            d.swap(i, j);
        }
        // Wrap around for easier indexing
        d[256] = d[0];

        // Precompute y-related values
        let i2 = b.floor();
        let d2 = b - i2;
        let h2 = (i2 as i32) & 255;
        let t2 = d2 * d2 * d2 * (d2 * (d2 * 6.0 - 15.0) + 10.0);

        Self {
            d,
            a,
            b,
            c,
            amplitude: 1.0,
            lacunarity: 1.0,
            h2,
            d2,
            t2,
        }
    }

    /// Sample 3D Perlin noise - cubiomes-accurate (scalar).
    #[inline]
    pub fn sample(&self, x: f64, y: f64, z: f64) -> f64 {
        // Handle y=0 special case (use precomputed values)
        let (d2, h2, t2) = if y == 0.0 {
            (self.d2, self.h2, self.t2)
        } else {
            let y = y + self.b;
            let i2 = y.floor();
            let d2 = y - i2;
            let h2 = (i2 as i32) & 255;
            let t2 = d2 * d2 * d2 * (d2 * (d2 * 6.0 - 15.0) + 10.0);
            (d2, h2, t2)
        };

        let d1 = x + self.a;
        let d3 = z + self.c;

        let i1 = d1.floor();
        let i3 = d3.floor();
        let d1 = d1 - i1;
        let d3 = d3 - i3;

        let h1 = (i1 as i32) & 255;
        let h3 = (i3 as i32) & 255;

        let t1 = d1 * d1 * d1 * (d1 * (d1 * 6.0 - 15.0) + 10.0);
        let t3 = d3 * d3 * d3 * (d3 * (d3 * 6.0 - 15.0) + 10.0);

        let idx = &self.d;

        // Calculate hash indices using i32 arithmetic with masking
        let a1 = (idx[h1 as usize] + h2) & 255;
        let b1 = (idx[((h1 + 1) & 255) as usize] + h2) & 255;

        let a2 = (idx[a1 as usize] + h3) & 255;
        let a3 = (idx[((a1 + 1) & 255) as usize] + h3) & 255;
        let b2 = (idx[b1 as usize] + h3) & 255;
        let b3 = (idx[((b1 + 1) & 255) as usize] + h3) & 255;

        // Calculate gradients and interpolate
        let l1 = indexed_lerp(idx[a2 as usize] & 15, d1, d2, d3);
        let l2 = indexed_lerp(idx[b2 as usize] & 15, d1 - 1.0, d2, d3);
        let l3 = indexed_lerp(idx[a3 as usize] & 15, d1, d2 - 1.0, d3);
        let l4 = indexed_lerp(idx[b3 as usize] & 15, d1 - 1.0, d2 - 1.0, d3);
        let l5 = indexed_lerp(idx[((a2 + 1) & 255) as usize] & 15, d1, d2, d3 - 1.0);
        let l6 = indexed_lerp(idx[((b2 + 1) & 255) as usize] & 15, d1 - 1.0, d2, d3 - 1.0);
        let l7 = indexed_lerp(idx[((a3 + 1) & 255) as usize] & 15, d1, d2 - 1.0, d3 - 1.0);
        let l8 = indexed_lerp(idx[((b3 + 1) & 255) as usize] & 15, d1 - 1.0, d2 - 1.0, d3 - 1.0);

        // Trilinear interpolation
        let l1 = lerp(t1, l1, l2);
        let l3 = lerp(t1, l3, l4);
        let l5 = lerp(t1, l5, l6);
        let l7 = lerp(t1, l7, l8);

        let l1 = lerp(t2, l1, l3);
        let l5 = lerp(t2, l5, l7);

        lerp(t3, l1, l5)
    }

    /// Sample 2D noise (y=0).
    #[inline]
    pub fn sample_2d(&self, x: f64, z: f64) -> f64 {
        self.sample(x, 0.0, z)
    }

    /// Sample noise with Y-smearing (for BlendedNoise).
    ///
    /// This matches Java's ImprovedNoise.noise(d, e, f, g, h) where:
    /// - g = yScale (smear scale)
    /// - h = y * scale (original y for clamping, NOT offset by yo)
    ///
    /// The smearing quantizes the Y fractional part to reduce noise sensitivity
    /// to Y changes, creating vertical stretching in terrain.
    ///
    /// Java algorithm (lines 37-62):
    /// ```java
    /// double j = e + this.yo;  // ONLY the y coord gets yo added
    /// int m = Mth.floor(j);
    /// double p = j - m;        // fractional y after offset
    /// double s;
    /// if (g != 0.0) {
    ///     double r = (h >= 0.0 && h < p) ? h : p;  // h is NOT offset by yo!
    ///     s = Mth.floor(r / g + 1.0E-7F) * g;
    /// } else {
    ///     s = 0.0;
    /// }
    /// return sampleAndLerp(l, m, n, o, p - s, q, p);
    /// ```
    #[inline]
    pub fn sample_smeared(&self, x: f64, y: f64, z: f64, y_scale: f64, y_orig: f64) -> f64 {
        // Add offsets (only to the sampling coordinates, NOT to y_orig)
        let d1 = x + self.a;
        let d2_raw = y + self.b;  // Java: j = e + this.yo
        let d3 = z + self.c;

        // Floor
        let i1 = d1.floor();
        let i2 = d2_raw.floor();  // Java: m = Mth.floor(j)
        let i3 = d3.floor();

        // Fractional parts
        let d1 = d1 - i1;
        let d2 = d2_raw - i2;  // Java: p = j - m (fractional y after offset)
        let d3 = d3 - i3;

        // h in Java is NOT offset by yo - it's used directly for comparison
        // y_orig is the unmodified h value (e.g., h*o or e*o from BlendedNoise)
        let h = y_orig;

        // Java smearing logic:
        // if (h >= 0.0 && h < p) r = h; else r = p;
        // s = floor(r / g + epsilon) * g
        let s = if y_scale != 0.0 {
            let r = if h >= 0.0 && h < d2 { h } else { d2 };
            (r / y_scale + 1.0e-7_f64).floor() * y_scale
        } else {
            0.0
        };

        // d2_smeared is used for gradient computation
        // d2 (original) is used for smoothstep
        let d2_smeared = d2 - s;

        let h1 = (i1 as i32) & 255;
        let h2 = (i2 as i32) & 255;
        let h3 = (i3 as i32) & 255;

        // Smoothstep uses ORIGINAL d2 (not smeared)
        let t1 = d1 * d1 * d1 * (d1 * (d1 * 6.0 - 15.0) + 10.0);
        let t2 = d2 * d2 * d2 * (d2 * (d2 * 6.0 - 15.0) + 10.0);  // original d2
        let t3 = d3 * d3 * d3 * (d3 * (d3 * 6.0 - 15.0) + 10.0);

        let idx = &self.d;

        // Calculate hash indices
        let a1 = (idx[h1 as usize] + h2) & 255;
        let b1 = (idx[((h1 + 1) & 255) as usize] + h2) & 255;

        let a2 = (idx[a1 as usize] + h3) & 255;
        let a3 = (idx[((a1 + 1) & 255) as usize] + h3) & 255;
        let b2 = (idx[b1 as usize] + h3) & 255;
        let b3 = (idx[((b1 + 1) & 255) as usize] + h3) & 255;

        // Gradients use SMEARED d2
        let l1 = indexed_lerp(idx[a2 as usize] & 15, d1, d2_smeared, d3);
        let l2 = indexed_lerp(idx[b2 as usize] & 15, d1 - 1.0, d2_smeared, d3);
        let l3 = indexed_lerp(idx[a3 as usize] & 15, d1, d2_smeared - 1.0, d3);
        let l4 = indexed_lerp(idx[b3 as usize] & 15, d1 - 1.0, d2_smeared - 1.0, d3);
        let l5 = indexed_lerp(idx[((a2 + 1) & 255) as usize] & 15, d1, d2_smeared, d3 - 1.0);
        let l6 = indexed_lerp(idx[((b2 + 1) & 255) as usize] & 15, d1 - 1.0, d2_smeared, d3 - 1.0);
        let l7 = indexed_lerp(idx[((a3 + 1) & 255) as usize] & 15, d1, d2_smeared - 1.0, d3 - 1.0);
        let l8 = indexed_lerp(idx[((b3 + 1) & 255) as usize] & 15, d1 - 1.0, d2_smeared - 1.0, d3 - 1.0);

        // Trilinear interpolation
        let l1 = lerp(t1, l1, l2);
        let l3 = lerp(t1, l3, l4);
        let l5 = lerp(t1, l5, l6);
        let l7 = lerp(t1, l7, l8);

        let l1 = lerp(t2, l1, l3);
        let l5 = lerp(t2, l5, l7);

        lerp(t3, l1, l5)
    }

    /// Sample 4 noise values simultaneously using portable SIMD.
    ///
    /// Takes `f64x4` for all three axes, allowing any combination of inputs:
    /// - 4 different X, same Y (splatted), 4 different Z
    /// - Same X, 4 different Y, same Z
    /// - Any arbitrary combination
    #[inline]
    pub fn sample_4(&self, x: f64x4, y: f64x4, z: f64x4) -> f64x4 {
        let one = f64x4::splat(1.0);
        let mask_255 = i32x4::splat(255);
        let mask_15 = i32x4::splat(15);

        // Offset coordinates
        let d1_vec = x + f64x4::splat(self.a);
        let y_vec = y + f64x4::splat(self.b);
        let d3_vec = z + f64x4::splat(self.c);

        // Floor
        let i1_vec = d1_vec.floor();
        let i2_vec = y_vec.floor();
        let i3_vec = d3_vec.floor();

        // Fractional parts
        let d1 = d1_vec - i1_vec;
        let d2 = y_vec - i2_vec;
        let d3 = d3_vec - i3_vec;

        // Hash indices (cast to i32, mask to 0-255)
        let h1 = i1_vec.cast::<i32>() & mask_255;
        let h2 = i2_vec.cast::<i32>() & mask_255;
        let h3 = i3_vec.cast::<i32>() & mask_255;

        // Compute smoothstep
        let t1 = smoothstep_simd(d1);
        let t2 = smoothstep_simd(d2);
        let t3 = smoothstep_simd(d3);

        // --- HASHING using gather ---
        let d_slice = &self.d;

        // Helper for gathering from permutation table
        let gather_d = |idx: i32x4| -> i32x4 {
            let safe_idx = idx & mask_255;
            let indices = safe_idx.cast::<usize>();
            Simd::gather_or_default(d_slice, indices)
        };

        // Layer 1: h1 lookups
        let a1_base = gather_d(h1);
        let b1_base = gather_d(h1 + i32x4::splat(1));

        // Add h2
        let a1 = a1_base + h2;
        let b1 = b1_base + h2;

        // Layer 2: resolve a1, b1
        let a2_base = gather_d(a1);
        let a3_base = gather_d(a1 + i32x4::splat(1));
        let b2_base = gather_d(b1);
        let b3_base = gather_d(b1 + i32x4::splat(1));

        // Add h3 to get final indices
        let a2 = a2_base + h3;
        let a3 = a3_base + h3;
        let b2 = b2_base + h3;
        let b3 = b3_base + h3;

        // Final gradient lookups
        let grad_a2 = gather_d(a2) & mask_15;
        let grad_b2 = gather_d(b2) & mask_15;
        let grad_a3 = gather_d(a3) & mask_15;
        let grad_b3 = gather_d(b3) & mask_15;

        let grad_a2p1 = gather_d(a2 + i32x4::splat(1)) & mask_15;
        let grad_b2p1 = gather_d(b2 + i32x4::splat(1)) & mask_15;
        let grad_a3p1 = gather_d(a3 + i32x4::splat(1)) & mask_15;
        let grad_b3p1 = gather_d(b3 + i32x4::splat(1)) & mask_15;

        // --- INTERPOLATION ---
        let d1_m1 = d1 - one;
        let d2_m1 = d2 - one;
        let d3_m1 = d3 - one;

        let l1 = indexed_lerp_simd(grad_a2, d1, d2, d3);
        let l2 = indexed_lerp_simd(grad_b2, d1_m1, d2, d3);
        let l3 = indexed_lerp_simd(grad_a3, d1, d2_m1, d3);
        let l4 = indexed_lerp_simd(grad_b3, d1_m1, d2_m1, d3);
        let l5 = indexed_lerp_simd(grad_a2p1, d1, d2, d3_m1);
        let l6 = indexed_lerp_simd(grad_b2p1, d1_m1, d2, d3_m1);
        let l7 = indexed_lerp_simd(grad_a3p1, d1, d2_m1, d3_m1);
        let l8 = indexed_lerp_simd(grad_b3p1, d1_m1, d2_m1, d3_m1);

        // Trilinear interpolation
        let l1 = lerp_simd(t1, l1, l2);
        let l3 = lerp_simd(t1, l3, l4);
        let l5 = lerp_simd(t1, l5, l6);
        let l7 = lerp_simd(t1, l7, l8);

        let l1 = lerp_simd(t2, l1, l3);
        let l5 = lerp_simd(t2, l5, l7);

        lerp_simd(t3, l1, l5)
    }

    /// Convenience: sample 4 points with arrays (converts to SIMD internally).
    #[inline]
    pub fn sample_4_arrays(&self, x: [f64; 4], y: f64, z: [f64; 4]) -> [f64; 4] {
        let result = self.sample_4(
            f64x4::from_array(x),
            f64x4::splat(y),
            f64x4::from_array(z),
        );
        result.to_array()
    }
}

// =============================================================================
// SIMD Helper Functions (Portable)
// =============================================================================

/// Smoothstep: t³(t(6t - 15) + 10)
#[inline(always)]
fn smoothstep_simd(d: f64x4) -> f64x4 {
    let six = f64x4::splat(6.0);
    let fifteen = f64x4::splat(15.0);
    let ten = f64x4::splat(10.0);

    // inner = (d * 6 - 15) * d + 10
    let inner = (d * six - fifteen).mul_add(d, ten);
    // d³ * inner
    let d2 = d * d;
    d2 * d * inner
}

/// Linear interpolation: a + t * (b - a)
#[inline(always)]
fn lerp_simd(t: f64x4, a: f64x4, b: f64x4) -> f64x4 {
    t.mul_add(b - a, a)
}

/// Branchless gradient selection (SIMD).
///
/// Logic matches standard Perlin:
/// - u = (h < 8) ? x : y
/// - v = (h < 4) ? y : ((h == 12 || h == 14) ? x : z)
/// - result = ((h & 1) ? -u : u) + ((h & 2) ? -v : v)
#[inline(always)]
fn indexed_lerp_simd(idx: i32x4, x: f64x4, y: f64x4, z: f64x4) -> f64x4 {
    // Note: idx is already masked to & 15 by caller
    // Convert to i64 since f64's mask type is i64
    let idx64: i64x4 = idx.cast();

    let one = i64x4::splat(1);
    let two = i64x4::splat(2);
    let four = i64x4::splat(4);
    let eight = i64x4::splat(8);
    let twelve = i64x4::splat(12);
    let fourteen = i64x4::splat(14);

    // u = h < 8 ? x : y
    let mask_u = idx64.simd_lt(eight);
    let u = mask_u.select(x, y);

    // v = h < 4 ? y : ((h == 12 || h == 14) ? x : z)
    let mask_less_4 = idx64.simd_lt(four);
    let mask_12 = idx64.simd_eq(twelve);
    let mask_14 = idx64.simd_eq(fourteen);
    let mask_12_or_14 = mask_12 | mask_14;

    let v_inner = mask_12_or_14.select(x, z);
    let v = mask_less_4.select(y, v_inner);

    // If (idx & 1) != 0, negate u
    let mask_neg_u = (idx64 & one).simd_ne(i64x4::splat(0));
    let u_final = mask_neg_u.select(-u, u);

    // If (idx & 2) != 0, negate v
    let mask_neg_v = (idx64 & two).simd_ne(i64x4::splat(0));
    let v_final = mask_neg_v.select(-v, v);

    u_final + v_final
}

// =============================================================================
// Scalar Helper Functions
// =============================================================================

/// Indexed gradient function (scalar).
#[inline(always)]
fn indexed_lerp(idx: i32, x: f64, y: f64, z: f64) -> f64 {
    let u = if idx < 8 { x } else { y };
    let v = if idx < 4 {
        y
    } else if idx == 12 || idx == 14 {
        x
    } else {
        z
    };
    let u_final = if (idx & 1) != 0 { -u } else { u };
    let v_final = if (idx & 2) != 0 { -v } else { v };
    u_final + v_final
}

/// Linear interpolation (scalar).
#[inline(always)]
fn lerp(t: f64, a: f64, b: f64) -> f64 {
    t.mul_add(b - a, a)
}

// =============================================================================
// Octave Noise
// =============================================================================

/// MD5 hash constants for octave initialization.
const MD5_OCTAVE_N: [(u64, u64); 13] = [
    (0xb198de63a8012672, 0x7b84cad43ef7b5a8), // octave_-12
    (0x0fd787bfbc403ec3, 0x74a4a31ca21b48b8), // octave_-11
    (0x36d326eed40efeb2, 0x5be9ce18223c636a), // octave_-10
    (0x082fe255f8be6631, 0x4e96119e22dedc81), // octave_-9
    (0x0ef68ec68504005e, 0x48b6bf93a2789640), // octave_-8
    (0xf11268128982754f, 0x257a1d670430b0aa), // octave_-7
    (0xe51c98ce7d1de664, 0x5f9478a733040c45), // octave_-6
    (0x6d7b49e7e429850a, 0x2e3063c622a24777), // octave_-5
    (0xbd90d5377ba1b762, 0xc07317d419a7548d), // octave_-4
    (0x53d39c6752dac858, 0xbcd1c5a80ab65b3e), // octave_-3
    (0xb4a24d7a84e7677b, 0x023ff9668e89b5c4), // octave_-2
    (0xdffa22b534c5f608, 0xb9b67517d3665ca9), // octave_-1
    (0xd50708086cef4d7c, 0x6e1651ecc7f43309), // octave_0
];

/// Multiple octaves of Perlin noise.
#[derive(Debug, Clone, Default)]
pub struct OctaveNoise {
    /// Individual octaves
    pub octaves: Vec<PerlinNoise>,
}

impl OctaveNoise {
    /// Create octave noise with given amplitudes (cubiomes-accurate xOctaveInit).
    pub fn new(rng: &mut Xoroshiro128, amplitudes: &[f64], omin: i32) -> Self {
        // Precomputed persistence table for len = 0..10
        let persist_ini: [f64; 11] = [
            0.0,
            1.0,
            0.6666666666666666,
            0.5714285714285714,
            0.5333333333333333,
            0.5161290322580645,
            0.5079365079365079,
            0.503937007874016,
            0.5019607843137255,
            0.5009775171065493,
            0.5004882812500000,
        ];

        let len = amplitudes.len();
        let mut lacuna = 1.0;

        let mut persist = if len < persist_ini.len() {
            persist_ini[len]
        } else {
            (1 << len) as f64 / ((1 << len) - 1) as f64
        };

        let x_lo = rng.next_long();
        let x_hi = rng.next_long();

        let mut octaves = Vec::new();

        for (i, &amp) in amplitudes.iter().enumerate() {
            if amp != 0.0 {
                let octave_idx = (12 + omin + i as i32) as usize;
                if octave_idx < MD5_OCTAVE_N.len() {
                    let md5 = MD5_OCTAVE_N[octave_idx];
                    let mut pxr = Xoroshiro128::from_state(x_lo ^ md5.0, x_hi ^ md5.1);
                    let mut noise = PerlinNoise::new(&mut pxr);
                    noise.amplitude = amp * persist;
                    noise.lacunarity = lacuna;
                    octaves.push(noise);
                }
            }
            lacuna *= 2.0;
            persist *= 0.5;
        }

        Self { octaves }
    }

    /// Sample combined octave noise (scalar).
    #[inline]
    pub fn sample(&self, x: f64, y: f64, z: f64) -> f64 {
        let mut value = 0.0;
        for octave in &self.octaves {
            let lf = octave.lacunarity;
            let ax = x * lf;
            let ay = y * lf;
            let az = z * lf;
            let pv = octave.sample(ax, ay, az);
            value += octave.amplitude * pv;
        }
        value
    }

    /// Sample 4 noise values simultaneously using portable SIMD.
    #[inline]
    pub fn sample_4(&self, x: f64x4, y: f64x4, z: f64x4) -> f64x4 {
        let mut values = f64x4::splat(0.0);

        for octave in &self.octaves {
            let lf = f64x4::splat(octave.lacunarity);
            let amp = f64x4::splat(octave.amplitude);

            // Scale coordinates
            let ax = x * lf;
            let ay = y * lf;
            let az = z * lf;

            // Accumulate with FMA
            values = amp.mul_add(octave.sample_4(ax, ay, az), values);
        }
        values
    }

    /// Convenience: sample 4 points with arrays.
    #[inline]
    pub fn sample_4_arrays(&self, x: [f64; 4], y: f64, z: [f64; 4]) -> [f64; 4] {
        let result = self.sample_4(
            f64x4::from_array(x),
            f64x4::splat(y),
            f64x4::from_array(z),
        );
        result.to_array()
    }
}

// =============================================================================
// Double Perlin Noise
// =============================================================================

/// Double Perlin noise (two octave noises combined) - cubiomes-accurate.
#[derive(Debug, Clone, Default)]
pub struct DoublePerlinNoise {
    /// Amplitude for combination
    pub amplitude: f64,
    pub frequency: f64,
    /// First octave noise
    pub oct_a: OctaveNoise,
    /// Second octave noise
    pub oct_b: OctaveNoise,
}

impl DoublePerlinNoise {
    /// Create double Perlin noise (cubiomes-accurate xDoublePerlinInit).
    pub fn new(rng: &mut Xoroshiro128, amplitudes: &[f64], omin: i32) -> Self {
        let oct_a = OctaveNoise::new(rng, amplitudes, omin);
        let oct_b = OctaveNoise::new(rng, amplitudes, omin);

        // Count effective octaves (trim zeros)
        let mut len = amplitudes.len() as i32;
        for i in (0..amplitudes.len()).rev() {
            if amplitudes[i] == 0.0 {
                len -= 1;
            } else {
                break;
            }
        }
        for amp in amplitudes {
            if *amp == 0.0 {
                len -= 1;
            } else {
                break;
            }
        }

        // Amplitude lookup: (5/3) * len / (len + 1)
        let amp_ini: [f64; 11] = [
            0.0,
            0.8333333333333334,
            1.1111111111111112,
            1.25,
            1.3333333333333333,
            1.3888888888888888,
            1.4285714285714286,
            1.4583333333333333,
            1.4814814814814814,
            1.5,
            1.5151515151515151,
        ];

        let amplitude = if len >= 0 && (len as usize) < amp_ini.len() {
            amp_ini[len as usize]
        } else if len > 0 {
            (5.0 / 3.0) * len as f64 / (len as f64 + 1.0)
        } else {
            0.0
        };
        let frequency = 2.0f64.powi(omin);

        Self {
            amplitude,
            frequency,
            oct_a,
            oct_b,
        }
    }

    /// Sample double Perlin noise (scalar).
    #[inline]
    pub fn sample(&self, x: f64, y: f64, z: f64) -> f64 {
        const F: f64 = 337.0 / 331.0;
        let nx = x * self.frequency;
        let ny = y * self.frequency;
        let nz = z * self.frequency;

        let v = self.oct_a.sample(nx, ny, nz) + self.oct_b.sample(nx * F, ny * F, nz * F);
        v * self.amplitude
    }

    /// Sample 4 double Perlin noise values simultaneously using portable SIMD.
    #[inline]
    pub fn sample_4(&self, x: f64x4, y: f64x4, z: f64x4) -> f64x4 {
        let f = f64x4::splat(337.0 / 331.0);
        let freq = f64x4::splat(self.frequency);
        let amp = f64x4::splat(self.amplitude);

        let nx = x * freq;
        let ny = y * freq;
        let nz = z * freq;

        let va = self.oct_a.sample_4(nx, ny, nz);

        // B coordinates (slightly offset frequency)
        let nx_b = nx * f;
        let ny_b = ny * f;
        let nz_b = nz * f;

        let vb = self.oct_b.sample_4(nx_b, ny_b, nz_b);

        (va + vb) * amp
    }

    /// Convenience: sample 4 points with arrays.
    #[inline]
    pub fn sample_4_arrays(&self, x: [f64; 4], y: f64, z: [f64; 4]) -> [f64; 4] {
        let result = self.sample_4(
            f64x4::from_array(x),
            f64x4::splat(y),
            f64x4::from_array(z),
        );
        result.to_array()
    }
}

// =============================================================================
// Blended Noise (OldBlendedNoise / base_3d_noise)
// =============================================================================

/// Legacy blended noise used for base_3d_noise in terrain generation.
///
/// This implements Java's BlendedNoise which blends between two limit noises
/// based on a main noise value. Uses 16 octaves for limit noises and 8 for main.
///
/// The algorithm:
/// 1. Sample mainNoise to get blend factor n (scaled to 0-1)
/// 2. If n >= 1.0, use only maxLimitNoise
/// 3. If n <= 0.0, use only minLimitNoise
/// 4. Otherwise, blend: lerp(n, minLimitNoise/512, maxLimitNoise/512)
/// 5. Final result: result / 128
///
/// Y-Smearing:
/// BlendedNoise uses a special "smearing" technique where the Y fractional part
/// is quantized to reduce sensitivity to Y changes. This creates the characteristic
/// vertical stretching in Minecraft terrain. The smear scale differs between main
/// noise (coarser, k = j/yFactor) and limit noises (finer, j = yMultiplier * smear).
#[derive(Debug, Clone)]
pub struct BlendedNoise {
    /// 16-octave noise for lower limit (octaves -15 to 0)
    min_limit: OctaveNoise,
    /// 16-octave noise for upper limit (octaves -15 to 0)
    max_limit: OctaveNoise,
    /// 8-octave noise for blending (octaves -7 to 0)
    main: OctaveNoise,
    /// Configuration
    xz_multiplier: f64,
    y_multiplier: f64,
    xz_factor: f64,
    y_factor: f64,
    /// Smear scale for limit noises (Java: j = yMultiplier * smearScaleMultiplier)
    limit_smear_scale: f64,
    /// Smear scale for main noise (Java: k = j / yFactor)
    main_smear_scale: f64,
}

impl BlendedNoise {
    /// Base multiplier for coordinates (matches Java's 684.412)
    const BASE_SCALE: f64 = 684.412;

    /// Create BlendedNoise with given parameters.
    ///
    /// This uses a simpler legacy initialization that directly creates Perlin noise
    /// for each octave without the MD5-based seeding used by DoublePerlinNoise.
    pub fn new(
        rng: &mut Xoroshiro128,
        xz_scale: f64,
        y_scale: f64,
        xz_factor: f64,
        y_factor: f64,
        smear_scale_multiplier: f64,
    ) -> Self {
        // Create 16 octaves for minLimitNoise (octaves -15 to 0)
        let min_limit = Self::create_legacy_octaves(rng, 16);
        // Create 16 octaves for maxLimitNoise (octaves -15 to 0)
        let max_limit = Self::create_legacy_octaves(rng, 16);
        // Create 8 octaves for mainNoise (octaves -7 to 0)
        let main = Self::create_legacy_octaves(rng, 8);

        // Java: j = yMultiplier * smearScaleMultiplier = 684.412 * y_scale * smear
        // Java: k = j / yFactor (smear scale for main noise)
        let limit_smear_scale = Self::BASE_SCALE * y_scale * smear_scale_multiplier;  // j
        let main_smear_scale = limit_smear_scale / y_factor;  // k

        Self {
            min_limit,
            max_limit,
            main,
            xz_multiplier: Self::BASE_SCALE * xz_scale,
            y_multiplier: Self::BASE_SCALE * y_scale,
            xz_factor,
            y_factor,
            limit_smear_scale,  // j - for limit noises
            main_smear_scale,   // k - for main noise
        }
    }

    /// Create legacy octaves for BlendedNoise (no MD5 seeding).
    ///
    /// Java's createLegacyForBlendedNoise just creates sequential Perlin instances.
    fn create_legacy_octaves(rng: &mut Xoroshiro128, count: usize) -> OctaveNoise {
        let mut octaves = Vec::with_capacity(count);

        for _ in 0..count {
            // Each octave gets its own Perlin noise from the same RNG sequence
            let noise = PerlinNoise::new(rng);
            octaves.push(noise);
        }

        OctaveNoise { octaves }
    }

    /// Sample blended noise at a position (matches Java BlendedNoise.compute).
    ///
    /// The Java algorithm:
    /// 1. Sample mainNoise at coarser scale (divided by factor) to get blend factor n
    /// 2. Blend factor q = (n/10 + 1) / 2, clamped to [0, 1]
    /// 3. If q >= 1: use only maxLimitNoise
    /// 4. If q <= 0: use only minLimitNoise
    /// 5. Otherwise: blend between them
    /// 6. Final: clampedLerp(q, l/512, m/512) / 128
    ///
    /// Y-Smearing:
    /// Both main and limit noises use smeared sampling where the Y fractional part
    /// is quantized to reduce sensitivity to Y changes.
    /// - Main noise: smear scale = k = (yMultiplier * smear) / yFactor
    /// - Limit noises: smear scale = j = yMultiplier * smear
    #[inline]
    pub fn sample(&self, x: f64, y: f64, z: f64) -> f64 {
        // Scale coordinates by base multiplier (684.412 * scale)
        // Java: d = x * xzMultiplier, e = y * yMultiplier, f = z * xzMultiplier
        let dx = x * self.xz_multiplier;
        let dy = y * self.y_multiplier;
        let dz = z * self.xz_multiplier;

        // Coarser scale for main noise (divide by factor)
        // Java: g = d / xzFactor, h = e / yFactor, i = f / xzFactor
        let gx = dx / self.xz_factor;
        let gy = dy / self.y_factor;
        let gz = dz / self.xz_factor;

        // Sample main noise (8 octaves) - determines the blend factor
        // Java: for (int p = 0; p < 8; p++) {
        //     n += noise(wrap(g*o), wrap(h*o), wrap(i*o), k*o, h*o) / o;
        //     o /= 2.0;
        // }
        let mut n = 0.0;
        let mut o = 1.0;
        for p in 0..8 {
            if p < self.main.octaves.len() {
                // Main noise uses smeared sampling with k*o as smear scale, h*o as y_orig
                let sampled = self.main.octaves[p].sample_smeared(
                    wrap_coord(gx * o),
                    wrap_coord(gy * o),
                    wrap_coord(gz * o),
                    self.main_smear_scale * o,  // k * o
                    gy * o,                      // h * o (not wrapped, used for clamping)
                );
                n += sampled / o;
            }
            o /= 2.0;
        }

        // Convert main noise to blend factor [0, 1]
        // Java: double q = (n / 10.0 + 1.0) / 2.0;
        let q = (n / 10.0 + 1.0) / 2.0;

        let use_max_only = q >= 1.0;
        let use_min_only = q <= 0.0;

        // Sample limit noises (16 octaves each)
        // Java: for (int r = 0; r < 16; r++) {
        //     l += noise(wrap(d*o), wrap(e*o), wrap(f*o), j*o, e*o) / o;
        //     o /= 2.0;
        // }
        let mut l = 0.0; // minLimitNoise sum
        let mut m = 0.0; // maxLimitNoise sum
        o = 1.0;

        for r in 0..16 {
            // Limit noises use smeared sampling with j*o as smear scale, e*o as y_orig
            if !use_max_only && r < self.min_limit.octaves.len() {
                l += self.min_limit.octaves[r].sample_smeared(
                    wrap_coord(dx * o),
                    wrap_coord(dy * o),
                    wrap_coord(dz * o),
                    self.limit_smear_scale * o,  // j * o
                    dy * o,                       // e * o (not wrapped)
                ) / o;
            }

            if !use_min_only && r < self.max_limit.octaves.len() {
                m += self.max_limit.octaves[r].sample_smeared(
                    wrap_coord(dx * o),
                    wrap_coord(dy * o),
                    wrap_coord(dz * o),
                    self.limit_smear_scale * o,  // j * o
                    dy * o,                       // e * o (not wrapped)
                ) / o;
            }

            o /= 2.0;
        }

        // Java: return Mth.clampedLerp(q, l / 512.0, m / 512.0) / 128.0;
        clamped_lerp(q, l / 512.0, m / 512.0) / 128.0
    }

    /// Sample blended noise at 4 Y positions (SIMD).
    #[inline]
    pub fn sample_4(&self, x: f64, y: f64x4, z: f64) -> f64x4 {
        // For now, use scalar fallback
        let y_arr = y.to_array();
        f64x4::from_array([
            self.sample(x, y_arr[0], z),
            self.sample(x, y_arr[1], z),
            self.sample(x, y_arr[2], z),
            self.sample(x, y_arr[3], z),
        ])
    }
}

/// Wrap coordinate to Perlin noise range [-33554432, 33554432].
/// Matches Java's PerlinNoise.wrap().
#[inline]
fn wrap_coord(value: f64) -> f64 {
    const RANGE: f64 = 33554432.0; // 2^25
    value - ((value / RANGE).floor() * RANGE)
}

/// Clamped linear interpolation.
#[inline]
fn clamped_lerp(t: f64, a: f64, b: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    a + t * (b - a)
}

// =============================================================================
// Simplex Noise
// =============================================================================

/// Simplex noise generator matching Java's SimplexNoise.
///
/// Used for End island generation and some special terrain features.
#[derive(Debug, Clone)]
pub struct SimplexNoise {
    /// Permutation table (512 entries for easy wrapping).
    p: [i32; 512],
    /// X offset
    pub xo: f64,
    /// Y offset
    pub yo: f64,
    /// Z offset
    pub zo: f64,
}

impl SimplexNoise {
    /// Gradient vectors for simplex noise (16 directions).
    const GRADIENT: [[i32; 3]; 16] = [
        [1, 1, 0],
        [-1, 1, 0],
        [1, -1, 0],
        [-1, -1, 0],
        [1, 0, 1],
        [-1, 0, 1],
        [1, 0, -1],
        [-1, 0, -1],
        [0, 1, 1],
        [0, -1, 1],
        [0, 1, -1],
        [0, -1, -1],
        [1, 1, 0],
        [0, -1, 1],
        [-1, 1, 0],
        [0, -1, -1],
    ];

    /// F2 constant: 0.5 * (sqrt(3) - 1)
    const F2: f64 = 0.3660254037844386;
    /// G2 constant: (3 - sqrt(3)) / 6
    const G2: f64 = 0.21132486540518713;
    /// F3 constant: 1/3
    const F3: f64 = 0.3333333333333333;
    /// G3 constant: 1/6
    const G3: f64 = 0.16666666666666666;

    /// Create a new SimplexNoise from a Xoroshiro RNG.
    pub fn new(rng: &mut Xoroshiro128) -> Self {
        let xo = rng.next_double() * 256.0;
        let yo = rng.next_double() * 256.0;
        let zo = rng.next_double() * 256.0;

        let mut p = [0i32; 512];

        // Initialize with identity permutation
        for i in 0..256 {
            p[i] = i as i32;
        }

        // Fisher-Yates shuffle (matching Java's exact algorithm)
        for i in 0..256 {
            let j = rng.next_int(256 - i as u32) as usize;
            let k = p[i];
            p[i] = p[j + i];
            p[j + i] = k;
        }

        Self { p, xo, yo, zo }
    }

    /// Get permutation value with wrapping.
    #[inline(always)]
    fn p(&self, i: i32) -> i32 {
        self.p[(i & 0xFF) as usize]
    }

    /// Dot product with gradient vector.
    #[inline(always)]
    fn dot(grad: &[i32; 3], x: f64, y: f64, z: f64) -> f64 {
        grad[0] as f64 * x + grad[1] as f64 * y + grad[2] as f64 * z
    }

    /// Calculate corner contribution for 3D simplex noise.
    #[inline(always)]
    fn get_corner_noise_3d(&self, grad_idx: i32, x: f64, y: f64, z: f64, falloff: f64) -> f64 {
        let h = falloff - x * x - y * y - z * z;
        if h < 0.0 {
            0.0
        } else {
            let h = h * h;
            h * h * Self::dot(&Self::GRADIENT[(grad_idx % 12) as usize], x, y, z)
        }
    }

    /// Sample 2D simplex noise.
    pub fn get_value_2d(&self, x: f64, y: f64) -> f64 {
        let f = (x + y) * Self::F2;
        let i = (x + f).floor() as i32;
        let j = (y + f).floor() as i32;

        let g = (i + j) as f64 * Self::G2;
        let h = i as f64 - g;
        let k = j as f64 - g;
        let l = x - h;
        let m = y - k;

        // Determine which simplex we're in
        let (n, o) = if l > m { (1, 0) } else { (0, 1) };

        // Offsets for middle corner
        let p = l - n as f64 + Self::G2;
        let q = m - o as f64 + Self::G2;

        // Offsets for last corner
        let r = l - 1.0 + 2.0 * Self::G2;
        let s = m - 1.0 + 2.0 * Self::G2;

        // Hash coordinates
        let t = i & 0xFF;
        let u = j & 0xFF;
        let v = (self.p(t + self.p(u)) % 12) as usize;
        let w = (self.p(t + n + self.p(u + o)) % 12) as usize;
        let x_idx = (self.p(t + 1 + self.p(u + 1)) % 12) as usize;

        // Calculate contributions from the three corners
        let corner0 = self.get_corner_noise_3d(v as i32, l, m, 0.0, 0.5);
        let corner1 = self.get_corner_noise_3d(w as i32, p, q, 0.0, 0.5);
        let corner2 = self.get_corner_noise_3d(x_idx as i32, r, s, 0.0, 0.5);

        70.0 * (corner0 + corner1 + corner2)
    }

    /// Sample 3D simplex noise.
    pub fn get_value_3d(&self, x: f64, y: f64, z: f64) -> f64 {
        let h = (x + y + z) * Self::F3;
        let i = (x + h).floor() as i32;
        let j = (y + h).floor() as i32;
        let k = (z + h).floor() as i32;

        let m = (i + j + k) as f64 * Self::G3;
        let n = i as f64 - m;
        let o = j as f64 - m;
        let p = k as f64 - m;
        let q = x - n;
        let r = y - o;
        let s = z - p;

        // Determine which simplex we're in
        let (t, u, v, w, x_off, y_off): (i32, i32, i32, i32, i32, i32);
        if q >= r {
            if r >= s {
                t = 1;
                u = 0;
                v = 0;
                w = 1;
                x_off = 1;
                y_off = 0;
            } else if q >= s {
                t = 1;
                u = 0;
                v = 0;
                w = 1;
                x_off = 0;
                y_off = 1;
            } else {
                t = 0;
                u = 0;
                v = 1;
                w = 1;
                x_off = 0;
                y_off = 1;
            }
        } else if r < s {
            t = 0;
            u = 0;
            v = 1;
            w = 0;
            x_off = 1;
            y_off = 1;
        } else if q < s {
            t = 0;
            u = 1;
            v = 0;
            w = 0;
            x_off = 1;
            y_off = 1;
        } else {
            t = 0;
            u = 1;
            v = 0;
            w = 1;
            x_off = 1;
            y_off = 0;
        }

        // Offsets for second corner
        let z_off = q - t as f64 + Self::G3;
        let aa = r - u as f64 + Self::G3;
        let ab = s - v as f64 + Self::G3;

        // Offsets for third corner
        let ac = q - w as f64 + 2.0 * Self::G3;
        let ad = r - x_off as f64 + 2.0 * Self::G3;
        let ae = s - y_off as f64 + 2.0 * Self::G3;

        // Offsets for fourth corner
        let af = q - 1.0 + 0.5;
        let ag = r - 1.0 + 0.5;
        let ah = s - 1.0 + 0.5;

        // Hash coordinates
        let ai = i & 0xFF;
        let aj = j & 0xFF;
        let ak = k & 0xFF;

        let al = self.p(ai + self.p(aj + self.p(ak))) % 12;
        let am = self.p(ai + t + self.p(aj + u + self.p(ak + v))) % 12;
        let an = self.p(ai + w + self.p(aj + x_off + self.p(ak + y_off))) % 12;
        let ao = self.p(ai + 1 + self.p(aj + 1 + self.p(ak + 1))) % 12;

        // Calculate contributions from the four corners
        let ap = self.get_corner_noise_3d(al, q, r, s, 0.6);
        let aq = self.get_corner_noise_3d(am, z_off, aa, ab, 0.6);
        let ar = self.get_corner_noise_3d(an, ac, ad, ae, 0.6);
        let as_ = self.get_corner_noise_3d(ao, af, ag, ah, 0.6);

        32.0 * (ap + aq + ar + as_)
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simplex_noise_deterministic() {
        // Test that simplex noise is deterministic with same seed
        let mut rng1 = Xoroshiro128::from_seed(12345);
        let mut rng2 = Xoroshiro128::from_seed(12345);

        let noise1 = SimplexNoise::new(&mut rng1);
        let noise2 = SimplexNoise::new(&mut rng2);

        // Same seed should produce same results
        assert_eq!(noise1.get_value_2d(0.0, 0.0), noise2.get_value_2d(0.0, 0.0));
        assert_eq!(noise1.get_value_2d(1.5, 2.5), noise2.get_value_2d(1.5, 2.5));
        assert_eq!(
            noise1.get_value_3d(1.0, 2.0, 3.0),
            noise2.get_value_3d(1.0, 2.0, 3.0)
        );
    }

    #[test]
    fn test_simplex_noise_range() {
        // Simplex noise should produce finite values
        let mut rng = Xoroshiro128::from_seed(42);
        let noise = SimplexNoise::new(&mut rng);

        for x in -10..10 {
            for y in -10..10 {
                let value_2d = noise.get_value_2d(x as f64 * 0.5, y as f64 * 0.5);
                assert!(
                    value_2d.is_finite() && value_2d.abs() <= 100.0,
                    "2D simplex value {} out of expected range at ({}, {})",
                    value_2d,
                    x,
                    y
                );

                let value_3d = noise.get_value_3d(x as f64 * 0.5, y as f64 * 0.5, 0.0);
                assert!(
                    value_3d.is_finite() && value_3d.abs() <= 100.0,
                    "3D simplex value {} out of expected range at ({}, {}, 0)",
                    value_3d,
                    x,
                    y
                );
            }
        }
    }

    #[test]
    fn test_perlin_noise_deterministic() {
        // Test that noise is deterministic with same seed
        let mut rng1 = Xoroshiro128::from_seed(12345);
        let mut rng2 = Xoroshiro128::from_seed(12345);

        let noise1 = PerlinNoise::new(&mut rng1);
        let noise2 = PerlinNoise::new(&mut rng2);

        // Same seed should produce same results
        assert_eq!(noise1.sample(0.0, 0.0, 0.0), noise2.sample(0.0, 0.0, 0.0));
        assert_eq!(noise1.sample(1.5, 2.5, 3.5), noise2.sample(1.5, 2.5, 3.5));
    }

    #[test]
    fn test_perlin_noise_range() {
        // Perlin noise should be in reasonable range
        let mut rng = Xoroshiro128::from_seed(42);
        let noise = PerlinNoise::new(&mut rng);

        for x in -10..10 {
            for z in -10..10 {
                let value = noise.sample(x as f64 * 0.5, 0.0, z as f64 * 0.5);
                assert!(
                    value >= -2.0 && value <= 2.0,
                    "Noise value {} out of expected range at ({}, {})",
                    value,
                    x,
                    z
                );
            }
        }
    }

    #[test]
    fn test_sample_4_matches_scalar() {
        // Test that SIMD sample_4 produces same results as scalar sample
        let mut rng = Xoroshiro128::from_seed(98765);
        let noise = PerlinNoise::new(&mut rng);

        // Test various coordinates
        let test_coords: [([f64; 4], f64, [f64; 4]); 5] = [
            ([0.0, 1.0, 2.0, 3.0], 0.0, [0.0, 0.0, 0.0, 0.0]),
            ([0.0, 0.0, 0.0, 0.0], 0.0, [0.0, 1.0, 2.0, 3.0]),
            ([0.5, 1.5, 2.5, 3.5], 0.0, [0.5, 1.5, 2.5, 3.5]),
            ([-1.0, 0.0, 1.0, 2.0], 0.0, [-2.0, -1.0, 0.0, 1.0]),
            ([10.0, 20.0, 30.0, 40.0], 0.0, [5.0, 15.0, 25.0, 35.0]),
        ];

        for (x, y, z) in test_coords {
            let simd_results = noise.sample_4_arrays(x, y, z);
            for i in 0..4 {
                let scalar_result = noise.sample(x[i], y, z[i]);
                let diff = (simd_results[i] - scalar_result).abs();
                assert!(
                    diff < 1e-10,
                    "SIMD and scalar mismatch at ({}, {}, {}): SIMD={}, scalar={}, diff={}",
                    x[i],
                    y,
                    z[i],
                    simd_results[i],
                    scalar_result,
                    diff
                );
            }
        }
    }

    #[test]
    fn test_sample_4_with_nonzero_y() {
        // Test sample_4 with non-zero y coordinate
        let mut rng = Xoroshiro128::from_seed(11111);
        let noise = PerlinNoise::new(&mut rng);

        let x = [0.0, 1.0, 2.0, 3.0];
        let y = 1.5;
        let z = [0.0, 1.0, 2.0, 3.0];

        let simd_results = noise.sample_4_arrays(x, y, z);
        for i in 0..4 {
            let scalar_result = noise.sample(x[i], y, z[i]);
            let diff = (simd_results[i] - scalar_result).abs();
            assert!(
                diff < 1e-10,
                "SIMD and scalar mismatch with y={}: SIMD={}, scalar={}, diff={}",
                y,
                simd_results[i],
                scalar_result,
                diff
            );
        }
    }

    #[test]
    fn test_sample_4_varying_y() {
        // Test sample_4 with different Y values for each lane
        let mut rng = Xoroshiro128::from_seed(22222);
        let noise = PerlinNoise::new(&mut rng);

        let x = f64x4::from_array([0.0, 1.0, 2.0, 3.0]);
        let y = f64x4::from_array([0.0, 0.5, 1.0, 1.5]);
        let z = f64x4::from_array([0.0, 1.0, 2.0, 3.0]);

        let simd_results = noise.sample_4(x, y, z).to_array();

        // Compare each lane against scalar
        let x_arr = x.to_array();
        let y_arr = y.to_array();
        let z_arr = z.to_array();

        for i in 0..4 {
            let scalar_result = noise.sample(x_arr[i], y_arr[i], z_arr[i]);
            let diff = (simd_results[i] - scalar_result).abs();
            assert!(
                diff < 1e-10,
                "SIMD and scalar mismatch at lane {}: SIMD={}, scalar={}, diff={}",
                i,
                simd_results[i],
                scalar_result,
                diff
            );
        }
    }

    #[test]
    fn test_permutation_table_i32() {
        // Verify the permutation table contains valid values
        let mut rng = Xoroshiro128::from_seed(54321);
        let noise = PerlinNoise::new(&mut rng);

        // All values should be in 0..256 range
        for &val in &noise.d[..256] {
            assert!(val >= 0 && val < 256, "Invalid permutation value: {}", val);
        }

        // Wrap value should match first value
        assert_eq!(noise.d[256], noise.d[0]);
    }

    #[test]
    fn test_perlin_noise_z_variation() {
        // This test ensures noise varies along Z axis
        let mut rng = Xoroshiro128::from_seed(42);
        let noise = PerlinNoise::new(&mut rng);

        // Sample at same X,Y but different Z values
        let v1 = noise.sample(5.0, 5.0, 0.0);
        let v2 = noise.sample(5.0, 5.0, 1.0);
        let v3 = noise.sample(5.0, 5.0, 2.0);

        // Values should differ
        assert!(
            (v1 - v2).abs() > 0.001 || (v2 - v3).abs() > 0.001,
            "Noise should vary along Z axis: v1={}, v2={}, v3={}",
            v1,
            v2,
            v3
        );
    }

    #[test]
    fn test_perlin_noise_all_axes_variation() {
        // More comprehensive test for variation along all axes
        let mut rng = Xoroshiro128::from_seed(12345);
        let noise = PerlinNoise::new(&mut rng);

        // Test X variation
        let x1 = noise.sample(0.0, 5.0, 5.0);
        let x2 = noise.sample(1.0, 5.0, 5.0);
        assert!(
            (x1 - x2).abs() > 0.0001,
            "Noise should vary along X axis: x1={}, x2={}",
            x1,
            x2
        );

        // Test Y variation
        let y1 = noise.sample(5.0, 0.0, 5.0);
        let y2 = noise.sample(5.0, 1.0, 5.0);
        assert!(
            (y1 - y2).abs() > 0.0001,
            "Noise should vary along Y axis: y1={}, y2={}",
            y1,
            y2
        );

        // Test Z variation
        let z1 = noise.sample(5.0, 5.0, 0.0);
        let z2 = noise.sample(5.0, 5.0, 1.0);
        assert!(
            (z1 - z2).abs() > 0.0001,
            "Noise should vary along Z axis: z1={}, z2={}",
            z1,
            z2
        );
    }

    #[test]
    fn test_octave_sample_4_matches_scalar() {
        let mut rng = Xoroshiro128::from_seed(33333);
        let noise = OctaveNoise::new(&mut rng, &[1.0, 0.5, 0.25], -3);

        let x = f64x4::from_array([0.0, 1.0, 2.0, 3.0]);
        let y = f64x4::splat(0.5);
        let z = f64x4::from_array([0.0, 1.0, 2.0, 3.0]);

        let simd_results = noise.sample_4(x, y, z).to_array();
        let x_arr = x.to_array();
        let z_arr = z.to_array();

        for i in 0..4 {
            let scalar_result = noise.sample(x_arr[i], 0.5, z_arr[i]);
            let diff = (simd_results[i] - scalar_result).abs();
            assert!(
                diff < 1e-9,
                "OctaveNoise SIMD/scalar mismatch at lane {}: SIMD={}, scalar={}, diff={}",
                i,
                simd_results[i],
                scalar_result,
                diff
            );
        }
    }

    #[test]
    fn test_double_perlin_sample_4_matches_scalar() {
        let mut rng = Xoroshiro128::from_seed(44444);
        let noise = DoublePerlinNoise::new(&mut rng, &[1.0, 1.0], -5);

        let x = f64x4::from_array([0.0, 10.0, 20.0, 30.0]);
        let y = f64x4::splat(5.0);
        let z = f64x4::from_array([0.0, 10.0, 20.0, 30.0]);

        let simd_results = noise.sample_4(x, y, z).to_array();
        let x_arr = x.to_array();
        let z_arr = z.to_array();

        for i in 0..4 {
            let scalar_result = noise.sample(x_arr[i], 5.0, z_arr[i]);
            let diff = (simd_results[i] - scalar_result).abs();
            assert!(
                diff < 1e-9,
                "DoublePerlinNoise SIMD/scalar mismatch at lane {}: SIMD={}, scalar={}, diff={}",
                i,
                simd_results[i],
                scalar_result,
                diff
            );
        }
    }
}
