//! Accurate Perlin and octave noise implementation based on cubiomes.
//! This implementation matches Minecraft's exact terrain generation algorithms.
//!
//! The permutation table is stored as i32 to enable efficient SIMD gather operations
//! on x86_64 using AVX2 instructions.

use super::xoroshiro::Xoroshiro128;

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

    /// Sample 3D Perlin noise - cubiomes-accurate.
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
        // Note: Mask with & 15 to get gradient index in 0..15 range (Perlin uses 12 gradients)
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
    pub fn sample_2d(&self, x: f64, z: f64) -> f64 {
        self.sample(x, 0.0, z)
    }

    /// Sample 4 noise values simultaneously using SIMD (x86_64 with AVX2).
    #[cfg(target_arch = "x86_64")]
    pub fn sample_4(&self, x: [f64; 4], y: f64, z: [f64; 4]) -> [f64; 4] {
        use std::arch::x86_64::*;

        // Y processing (shared scalar path for all 4 samples)
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

        unsafe {
            // Load X and Z coordinates
            let x_vec = _mm256_loadu_pd(x.as_ptr());
            let z_vec = _mm256_loadu_pd(z.as_ptr());

            // Add offsets
            let a_bc = _mm256_set1_pd(self.a);
            let c_bc = _mm256_set1_pd(self.c);
            let d1_vec = _mm256_add_pd(x_vec, a_bc);
            let d3_vec = _mm256_add_pd(z_vec, c_bc);

            // Floor
            let i1_vec = _mm256_floor_pd(d1_vec);
            let i3_vec = _mm256_floor_pd(d3_vec);

            // Fractional parts (d1, d3)
            let d1_frac = _mm256_sub_pd(d1_vec, i1_vec);
            let d3_frac = _mm256_sub_pd(d3_vec, i3_vec);

            // Convert floor to i32 for hash indices
            let i1_i32 = _mm256_cvtpd_epi32(i1_vec);
            let i3_i32 = _mm256_cvtpd_epi32(i3_vec);

            // Mask to 255
            let mask_255 = _mm_set1_epi32(255);
            let h1_vec = _mm_and_si128(i1_i32, mask_255);
            let h3_vec = _mm_and_si128(i3_i32, mask_255);

            // Smoothstep for t1 and t3
            let t1_vec = smoothstep_simd_256(d1_frac);
            let t3_vec = smoothstep_simd_256(d3_frac);

            // --- Hashing (AVX2 Gather) ---
            let perm_ptr = self.d.as_ptr(); // Requires self.d to be [i32; 257]

            // h1 lookup
            let a1_base = _mm_i32gather_epi32::<4>(perm_ptr, h1_vec);

            // h1 + 1 lookup
            let h1_plus1 = _mm_and_si128(_mm_add_epi32(h1_vec, _mm_set1_epi32(1)), mask_255);
            let b1_base = _mm_i32gather_epi32::<4>(perm_ptr, h1_plus1);

            // Add h2
            let h2_bc = _mm_set1_epi32(h2);
            let a1 = _mm_and_si128(_mm_add_epi32(a1_base, h2_bc), mask_255);
            let b1 = _mm_and_si128(_mm_add_epi32(b1_base, h2_bc), mask_255);

            // Next layer lookups
            let a1_plus1 = _mm_and_si128(_mm_add_epi32(a1, _mm_set1_epi32(1)), mask_255);
            let b1_plus1 = _mm_and_si128(_mm_add_epi32(b1, _mm_set1_epi32(1)), mask_255);

            let a2 = _mm_and_si128(
                _mm_add_epi32(_mm_i32gather_epi32::<4>(perm_ptr, a1), h3_vec),
                mask_255,
            );
            let a3 = _mm_and_si128(
                _mm_add_epi32(_mm_i32gather_epi32::<4>(perm_ptr, a1_plus1), h3_vec),
                mask_255,
            );
            let b2 = _mm_and_si128(
                _mm_add_epi32(_mm_i32gather_epi32::<4>(perm_ptr, b1), h3_vec),
                mask_255,
            );
            let b3 = _mm_and_si128(
                _mm_add_epi32(_mm_i32gather_epi32::<4>(perm_ptr, b1_plus1), h3_vec),
                mask_255,
            );

            // Final Gradient Index Lookups (The "grad" values)
            let grad_a2 = _mm_i32gather_epi32::<4>(perm_ptr, a2);
            let grad_b2 = _mm_i32gather_epi32::<4>(perm_ptr, b2);
            let grad_a3 = _mm_i32gather_epi32::<4>(perm_ptr, a3);
            let grad_b3 = _mm_i32gather_epi32::<4>(perm_ptr, b3);

            let a2_p1 = _mm_and_si128(_mm_add_epi32(a2, _mm_set1_epi32(1)), mask_255);
            let b2_p1 = _mm_and_si128(_mm_add_epi32(b2, _mm_set1_epi32(1)), mask_255);
            let a3_p1 = _mm_and_si128(_mm_add_epi32(a3, _mm_set1_epi32(1)), mask_255);
            let b3_p1 = _mm_and_si128(_mm_add_epi32(b3, _mm_set1_epi32(1)), mask_255);

            let grad_a2p1 = _mm_i32gather_epi32::<4>(perm_ptr, a2_p1);
            let grad_b2p1 = _mm_i32gather_epi32::<4>(perm_ptr, b2_p1);
            let grad_a3p1 = _mm_i32gather_epi32::<4>(perm_ptr, a3_p1);
            let grad_b3p1 = _mm_i32gather_epi32::<4>(perm_ptr, b3_p1);

            // --- Compute Gradients (Branchless SIMD) ---
            let d2_bc = _mm256_set1_pd(d2);
            let one = _mm256_set1_pd(1.0);
            let d1_m1 = _mm256_sub_pd(d1_frac, one);
            let d2_m1 = _mm256_sub_pd(d2_bc, one);
            let d3_m1 = _mm256_sub_pd(d3_frac, one);

            // 8 gradient dot products (using the new bitwise function)
            let l1 = indexed_lerp_simd(grad_a2, d1_frac, d2_bc, d3_frac);
            let l2 = indexed_lerp_simd(grad_b2, d1_m1, d2_bc, d3_frac);
            let l3 = indexed_lerp_simd(grad_a3, d1_frac, d2_m1, d3_frac);
            let l4 = indexed_lerp_simd(grad_b3, d1_m1, d2_m1, d3_frac);
            let l5 = indexed_lerp_simd(grad_a2p1, d1_frac, d2_bc, d3_m1);
            let l6 = indexed_lerp_simd(grad_b2p1, d1_m1, d2_bc, d3_m1);
            let l7 = indexed_lerp_simd(grad_a3p1, d1_frac, d2_m1, d3_m1);
            let l8 = indexed_lerp_simd(grad_b3p1, d1_m1, d2_m1, d3_m1);

            // --- Trilinear Interpolation ---
            let l1 = lerp_simd(t1_vec, l1, l2);
            let l3 = lerp_simd(t1_vec, l3, l4);
            let l5 = lerp_simd(t1_vec, l5, l6);
            let l7 = lerp_simd(t1_vec, l7, l8);

            let t2_bc = _mm256_set1_pd(t2);
            let l1 = lerp_simd(t2_bc, l1, l3);
            let l5 = lerp_simd(t2_bc, l5, l7);

            let result = lerp_simd(t3_vec, l1, l5);

            let mut out = [0.0f64; 4];
            _mm256_storeu_pd(out.as_mut_ptr(), result);
            out
        }
    }

    /// Non-SIMD fallback for sample_4 on non-x86_64 platforms.
    #[cfg(not(target_arch = "x86_64"))]
    pub fn sample_4(&self, x: [f64; 4], y: f64, z: [f64; 4]) -> [f64; 4] {
        [
            self.sample(x[0], y, z[0]),
            self.sample(x[1], y, z[1]),
            self.sample(x[2], y, z[2]),
            self.sample(x[3], y, z[3]),
        ]
    }
}

/// SIMD smoothstep: t³(t(6t - 15) + 10)
#[inline]
#[cfg(target_arch = "x86_64")]
unsafe fn smoothstep_simd_256(d: std::arch::x86_64::__m256d) -> std::arch::x86_64::__m256d {
    use std::arch::x86_64::*;

    unsafe {
        let six = _mm256_set1_pd(6.0);
        let fifteen = _mm256_set1_pd(15.0);
        let ten = _mm256_set1_pd(10.0);

        let inner = _mm256_add_pd(
            _mm256_mul_pd(_mm256_sub_pd(_mm256_mul_pd(d, six), fifteen), d),
            ten,
        );
        let d3 = _mm256_mul_pd(_mm256_mul_pd(d, d), d);
        _mm256_mul_pd(d3, inner)
    }
}

/// SIMD lerp: a + t * (b - a)
#[inline(always)]
#[cfg(target_arch = "x86_64")]
unsafe fn lerp_simd(
    t: std::arch::x86_64::__m256d,
    a: std::arch::x86_64::__m256d,
    b: std::arch::x86_64::__m256d,
) -> std::arch::x86_64::__m256d {
    use std::arch::x86_64::*;
    unsafe { _mm256_add_pd(a, _mm256_mul_pd(t, _mm256_sub_pd(b, a))) }
}

/// True SIMD gradient calculation (AVX2).
///
/// Instead of extracting to scalar, this uses bitwise logic to select
/// coordinates (u, v) and apply signs based on the hash bits.
///
/// Logic matches Standard/Improved Perlin:
/// u = (h < 8) ? x : y
/// v = (h < 4) ? y : ((h == 12 || h == 14) ? x : z)
/// result = ((h & 1) ? -u : u) + ((h & 2) ? -v : v)
#[inline(always)]
unsafe fn indexed_lerp_simd(
    idx_128: std::arch::x86_64::__m128i, // 4 integer indices from permutation table
    x: std::arch::x86_64::__m256d,
    y: std::arch::x86_64::__m256d,
    z: std::arch::x86_64::__m256d,
) -> std::arch::x86_64::__m256d {
    use std::arch::x86_64::*;
    unsafe {
        // Mask indices to 0..15 range (critical for correct gradient selection)
        // Permutation table contains 0-255, but gradient logic expects 0-15
        let mask_15 = _mm_set1_epi32(15);
        let idx_masked = _mm_and_si128(idx_128, mask_15);

        // 1. Prepare Masks
        // Convert indices to doubles for easy comparison (0.0 .. 15.0)
        let h_dbl = _mm256_cvtepi32_pd(idx_masked);
        // Convert indices to 64-bit ints for bitwise checks
        let h_long = _mm256_cvtepi32_epi64(idx_masked);

        // Constants
        let eight = _mm256_set1_pd(8.0);
        let four = _mm256_set1_pd(4.0);
        let twelve = _mm256_set1_pd(12.0);
        let fourteen = _mm256_set1_pd(14.0);

        // 2. Select U: (h < 8) ? x : y
        // CMP_LT_OQ: Less Than, Ordered, Quiet
        let mask_u = _mm256_cmp_pd(h_dbl, eight, _CMP_LT_OQ);
        let u = _mm256_blendv_pd(y, x, mask_u); // Select x if mask is 1, else y

        // 3. Select V: (h < 4) ? y : ((h == 12 || h == 14) ? x : z)
        // Mask: h < 4
        let mask_less_4 = _mm256_cmp_pd(h_dbl, four, _CMP_LT_OQ);
        // Mask: h == 12 or h == 14
        let mask_12 = _mm256_cmp_pd(h_dbl, twelve, _CMP_EQ_OQ);
        let mask_14 = _mm256_cmp_pd(h_dbl, fourteen, _CMP_EQ_OQ);
        let mask_12_14 = _mm256_or_pd(mask_12, mask_14);

        // Logic: If 12/14, pick X. Else pick Z.
        let v_base = _mm256_blendv_pd(z, x, mask_12_14);
        // Logic: If < 4, pick Y. Else pick v_base.
        let v = _mm256_blendv_pd(v_base, y, mask_less_4);

        // 4. Apply Signs
        // Term 1: If (h & 1) is true, negate u
        let one_i64 = _mm256_set1_epi64x(1);
        let mask_bit1_i = _mm256_cmpeq_epi64(_mm256_and_si256(h_long, one_i64), one_i64);
        let mask_bit1 = _mm256_castsi256_pd(mask_bit1_i);

        // Term 2: If (h & 2) is true, negate v
        let two_i64 = _mm256_set1_epi64x(2);
        let mask_bit2_i = _mm256_cmpeq_epi64(_mm256_and_si256(h_long, two_i64), two_i64);
        let mask_bit2 = _mm256_castsi256_pd(mask_bit2_i);

        // Compute both versions
        let neg_u = _mm256_sub_pd(_mm256_setzero_pd(), u);
        let neg_v = _mm256_sub_pd(_mm256_setzero_pd(), v);

        // Select based on bit masks
        let term1 = _mm256_blendv_pd(u, neg_u, mask_bit1);
        let term2 = _mm256_blendv_pd(v, neg_v, mask_bit2);

        // 5. Final Addition
        _mm256_add_pd(term1, term2)
    }
}

/// Indexed gradient function (Scalar).
/// This matches the SIMD logic but for single values.
#[inline(always)]
fn indexed_lerp(idx: i32, x: f64, y: f64, z: f64) -> f64 {
    // The bitwise logic is often faster than the 16-way match
    // because it avoids branch misprediction penalties.

    // u = h < 8 ? x : y
    let u = if idx < 8 { x } else { y };

    // v = h < 4 ? y : (h == 12 || h == 14 ? x : z)
    let v = if idx < 4 {
        y
    } else if idx == 12 || idx == 14 {
        x
    } else {
        z
    };

    // calculate (h&1 ? -u : u) + (h&2 ? -v : v)
    let u_final = if (idx & 1) != 0 { -u } else { u };
    let v_final = if (idx & 2) != 0 { -v } else { v };

    u_final + v_final
}

/// Linear interpolation.
#[inline]
fn lerp(t: f64, a: f64, b: f64) -> f64 {
    a + t * (b - a)
}

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
        /*
        // Precomputed lacunarity table for -omin = 0..12
        let lacuna_ini: [f64; 13] = [
            1.0,
            0.5,
            0.25,
            0.125,
            0.0625,
            0.03125,
            0.015625,
            0.0078125,
            0.00390625,
            0.001953125,
            0.0009765625,
            0.00048828125,
            0.000244140625,
        ];
        */
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
        let lacuna_idx = (-omin) as usize;
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

    /// Sample combined octave noise.
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

    /// Sample 4 noise values simultaneously using SIMD.
    /// Each input is an array of 4 coordinates to sample at.
    #[cfg(target_arch = "x86_64")]
    pub fn sample_4(&self, x: [f64; 4], y: f64, z: [f64; 4]) -> [f64; 4] {
        let mut values = [0.0f64; 4];

        for octave in &self.octaves {
            let lf = octave.lacunarity;

            // Scale all 4 x and z coordinates
            let ax = [x[0] * lf, x[1] * lf, x[2] * lf, x[3] * lf];
            let ay = y * lf;
            let az = [z[0] * lf, z[1] * lf, z[2] * lf, z[3] * lf];

            // Use SIMD sample_4 for all 4 points
            let pv = octave.sample_4(ax, ay, az);

            // Accumulate with amplitude
            let amp = octave.amplitude;
            values[0] += amp * pv[0];
            values[1] += amp * pv[1];
            values[2] += amp * pv[2];
            values[3] += amp * pv[3];
        }

        values
    }

    /// Non-SIMD fallback for sample_4 on non-x86_64 platforms.
    #[cfg(not(target_arch = "x86_64"))]
    pub fn sample_4(&self, x: [f64; 4], y: f64, z: [f64; 4]) -> [f64; 4] {
        [
            self.sample(x[0], y, z[0]),
            self.sample(x[1], y, z[1]),
            self.sample(x[2], y, z[2]),
            self.sample(x[3], y, z[3]),
        ]
    }
}

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

    /// Sample double Perlin noise.
    pub fn sample(&self, x: f64, y: f64, z: f64) -> f64 {
        const F: f64 = 337.0 / 331.0;
        let nx = x * self.frequency;
        let ny = y * self.frequency;
        let nz = z * self.frequency;

        let v = self.oct_a.sample(nx, ny, nz) + self.oct_b.sample(nx * F, ny * F, nz * F);
        v * self.amplitude
    }

    /// Sample 4 double Perlin noise values simultaneously using SIMD.
    pub fn sample_4(&self, x: [f64; 4], y: f64, z: [f64; 4]) -> [f64; 4] {
        const F: f64 = 337.0 / 331.0;
        let freq = self.frequency;

        // Apply frequency
        let nx = [x[0] * freq, x[1] * freq, x[2] * freq, x[3] * freq];
        let ny = y * freq;
        let nz = [z[0] * freq, z[1] * freq, z[2] * freq, z[3] * freq];

        // Sample
        let va = self.oct_a.sample_4(nx, ny, nz);
        
        // Calculate B coordinates
        let nx_b = [nx[0] * F, nx[1] * F, nx[2] * F, nx[3] * F];
        let ny_b = ny * F;
        let nz_b = [nz[0] * F, nz[1] * F, nz[2] * F, nz[3] * F];
        let vb = self.oct_b.sample_4(nx_b, ny_b, nz_b);

        let amp = self.amplitude;
        [
            (va[0] + vb[0]) * amp,
            (va[1] + vb[1]) * amp,
            (va[2] + vb[2]) * amp,
            (va[3] + vb[3]) * amp,
        ]
    }
}

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
    #[inline]
    fn p(&self, i: i32) -> i32 {
        self.p[(i & 0xFF) as usize]
    }

    /// Dot product with gradient vector.
    #[inline]
    fn dot(grad: &[i32; 3], x: f64, y: f64, z: f64) -> f64 {
        grad[0] as f64 * x + grad[1] as f64 * y + grad[2] as f64 * z
    }

    /// Calculate corner contribution for 3D simplex noise.
    #[inline]
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
                t = 1; u = 0; v = 0; w = 1; x_off = 1; y_off = 0;
            } else if q >= s {
                t = 1; u = 0; v = 0; w = 1; x_off = 0; y_off = 1;
            } else {
                t = 0; u = 0; v = 1; w = 1; x_off = 0; y_off = 1;
            }
        } else if r < s {
            t = 0; u = 0; v = 1; w = 0; x_off = 1; y_off = 1;
        } else if q < s {
            t = 0; u = 1; v = 0; w = 0; x_off = 1; y_off = 1;
        } else {
            t = 0; u = 1; v = 0; w = 1; x_off = 1; y_off = 0;
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
        assert_eq!(noise1.get_value_3d(1.0, 2.0, 3.0), noise2.get_value_3d(1.0, 2.0, 3.0));
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
        // Perlin noise should be in [-1, 1] range
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
    #[cfg(target_arch = "x86_64")]
    fn test_sample_4_matches_scalar() {
        // Test that SIMD sample_4 produces same results as scalar sample
        let mut rng = Xoroshiro128::from_seed(98765);
        let noise = PerlinNoise::new(&mut rng);

        // Test various coordinates
        let test_coords = [
            ([0.0, 1.0, 2.0, 3.0], 0.0, [0.0, 0.0, 0.0, 0.0]),
            ([0.0, 0.0, 0.0, 0.0], 0.0, [0.0, 1.0, 2.0, 3.0]),
            ([0.5, 1.5, 2.5, 3.5], 0.0, [0.5, 1.5, 2.5, 3.5]),
            ([-1.0, 0.0, 1.0, 2.0], 0.0, [-2.0, -1.0, 0.0, 1.0]),
            ([10.0, 20.0, 30.0, 40.0], 0.0, [5.0, 15.0, 25.0, 35.0]),
        ];

        for (x, y, z) in test_coords {
            let simd_results = noise.sample_4(x, y, z);
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
    #[cfg(target_arch = "x86_64")]
    fn test_sample_4_with_nonzero_y() {
        // Test sample_4 with non-zero y coordinate
        let mut rng = Xoroshiro128::from_seed(11111);
        let noise = PerlinNoise::new(&mut rng);

        let x = [0.0, 1.0, 2.0, 3.0];
        let y = 1.5;
        let z = [0.0, 1.0, 2.0, 3.0];

        let simd_results = noise.sample_4(x, y, z);
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
        // This test ensures noise varies along Z axis (catches the & 15 mask bug)
        // Without proper masking, noise would create "lines" along certain axes
        let mut rng = Xoroshiro128::from_seed(42);
        let noise = PerlinNoise::new(&mut rng);

        // Sample at same X,Y but different Z values
        let v1 = noise.sample(5.0, 5.0, 0.0);
        let v2 = noise.sample(5.0, 5.0, 1.0);
        let v3 = noise.sample(5.0, 5.0, 2.0);

        // Values should differ (not be identical "lines")
        assert!(
            (v1 - v2).abs() > 0.001 || (v2 - v3).abs() > 0.001,
            "Noise should vary along Z axis: v1={}, v2={}, v3={}",
            v1, v2, v3
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
            x1, x2
        );

        // Test Y variation
        let y1 = noise.sample(5.0, 0.0, 5.0);
        let y2 = noise.sample(5.0, 1.0, 5.0);
        assert!(
            (y1 - y2).abs() > 0.0001,
            "Noise should vary along Y axis: y1={}, y2={}",
            y1, y2
        );

        // Test Z variation
        let z1 = noise.sample(5.0, 5.0, 0.0);
        let z2 = noise.sample(5.0, 5.0, 1.0);
        assert!(
            (z1 - z2).abs() > 0.0001,
            "Noise should vary along Z axis: z1={}, z2={}",
            z1, z2
        );
    }
}
