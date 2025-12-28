//! Accurate Perlin and octave noise implementation based on cubiomes.
//! This implementation matches Minecraft's exact terrain generation algorithms.

use super::xoroshiro::Xoroshiro128;

/// 3D Perlin noise generator - cubiomes-accurate implementation.
#[derive(Debug, Clone)]
pub struct PerlinNoise {
    /// Permutation table (257 entries for wrapping)
    pub d: [u8; 257],
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
    /// Precomputed floor(b) mod 256
    h2: u8,
    /// Precomputed b - floor(b)
    d2: f64,
    /// Precomputed smoothstep(d2)
    t2: f64,
}

impl Default for PerlinNoise {
    fn default() -> Self {
        Self {
            d: [0; 257],
            a: 0.0,
            b: 0.0,
            c: 0.0,
            amplitude: 1.0,
            lacunarity: 1.0,
            h2: 0,
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

        let mut d = [0u8; 257];

        // Initialize with identity
        for i in 0..256 {
            d[i] = i as u8;
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
        let h2 = (i2 as i32) as u8;
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
            let h2 = (i2 as i32) as u8;
            let t2 = d2 * d2 * d2 * (d2 * (d2 * 6.0 - 15.0) + 10.0);
            (d2, h2, t2)
        };

        let d1 = x + self.a;
        let d3 = z + self.c;

        let i1 = d1.floor();
        let i3 = d3.floor();
        let d1 = d1 - i1;
        let d3 = d3 - i3;

        let h1 = (i1 as i32) as u8;
        let h3 = (i3 as i32) as u8;

        let t1 = d1 * d1 * d1 * (d1 * (d1 * 6.0 - 15.0) + 10.0);
        let t3 = d3 * d3 * d3 * (d3 * (d3 * 6.0 - 15.0) + 10.0);

        let idx = &self.d;

        // Calculate hash indices
        let a1 = idx[h1 as usize].wrapping_add(h2);
        let b1 = idx[h1.wrapping_add(1) as usize].wrapping_add(h2);

        let a2 = idx[a1 as usize].wrapping_add(h3);
        let a3 = idx[a1.wrapping_add(1) as usize].wrapping_add(h3);
        let b2 = idx[b1 as usize].wrapping_add(h3);
        let b3 = idx[b1.wrapping_add(1) as usize].wrapping_add(h3);

        // Calculate gradients and interpolate
        let l1 = indexed_lerp(idx[a2 as usize], d1, d2, d3);
        let l2 = indexed_lerp(idx[b2 as usize], d1 - 1.0, d2, d3);
        let l3 = indexed_lerp(idx[a3 as usize], d1, d2 - 1.0, d3);
        let l4 = indexed_lerp(idx[b3 as usize], d1 - 1.0, d2 - 1.0, d3);
        let l5 = indexed_lerp(idx[a2.wrapping_add(1) as usize], d1, d2, d3 - 1.0);
        let l6 = indexed_lerp(idx[b2.wrapping_add(1) as usize], d1 - 1.0, d2, d3 - 1.0);
        let l7 = indexed_lerp(idx[a3.wrapping_add(1) as usize], d1, d2 - 1.0, d3 - 1.0);
        let l8 = indexed_lerp(
            idx[b3.wrapping_add(1) as usize],
            d1 - 1.0,
            d2 - 1.0,
            d3 - 1.0,
        );

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

    /// Sample 4 noise values simultaneously using SIMD (x86_64).
    #[cfg(target_arch = "x86_64")]
    pub fn sample_4(&self, x: [f64; 4], y: f64, z: [f64; 4]) -> [f64; 4] {
        use std::arch::x86_64::*;
        
        unsafe {
            // Load X and Z coordinates into SIMD registers
            let x_simd = _mm256_loadu_pd(x.as_ptr());
            let z_simd = _mm256_loadu_pd(z.as_ptr());
            
            // Add offsets
            let a_broadcast = _mm256_set1_pd(self.a);
            let c_broadcast = _mm256_set1_pd(self.c);
            let d1 = _mm256_add_pd(x_simd, a_broadcast);
            let d3 = _mm256_add_pd(z_simd, c_broadcast);
            
            // Floor operation
            let i1 = _mm256_floor_pd(d1);
            let i3 = _mm256_floor_pd(d3);
            
            // Subtract to get fractional parts
            let d1_frac = _mm256_sub_pd(d1, i1);
            let d3_frac = _mm256_sub_pd(d3, i3);
            
            // Smoothstep calculation: t = d³(d(6d - 15) + 10)
            let six = _mm256_set1_pd(6.0);
            let fifteen = _mm256_set1_pd(15.0);
            let ten = _mm256_set1_pd(10.0);
            
            let t1 = smoothstep_simd(d1_frac, six, fifteen, ten);
            let t3 = smoothstep_simd(d3_frac, six, fifteen, ten); // Note: unused in this partial implementation but kept for structure

            // Use scalar fallback for the rest gradient calculation as it requires gathering from permutation table
            // Ideally we would gather using AVX2 but that is complex for u8 permutation table
            let mut results = [0.0; 4];
            for i in 0..4 {
                results[i] = self.sample(x[i], y, z[i]);
            }
            results
        }
    }
}

/// Helper for SIMD smoothstep
#[inline]
#[cfg(target_arch = "x86_64")]
unsafe fn smoothstep_simd(
    d: std::arch::x86_64::__m256d,
    six: std::arch::x86_64::__m256d,
    fifteen: std::arch::x86_64::__m256d,
    ten: std::arch::x86_64::__m256d
) -> std::arch::x86_64::__m256d {
    use std::arch::x86_64::*;
    
    // t = d³(d(6d - 15) + 10)
    let d_sq = _mm256_mul_pd(d, d);
    let d_cube = _mm256_mul_pd(d_sq, d);
    
    let inner = _mm256_mul_pd(six, d);
    let inner = _mm256_sub_pd(inner, fifteen);
    let inner = _mm256_mul_pd(inner, d);
    let inner = _mm256_add_pd(inner, ten);
    
    _mm256_mul_pd(d_cube, inner)
}

/// Indexed gradient function - matches cubiomes exactly.
#[inline]
fn indexed_lerp(idx: u8, a: f64, b: f64, c: f64) -> f64 {
    match idx & 0xf {
        0 => a + b,
        1 => -a + b,
        2 => a - b,
        3 => -a - b,
        4 => a + c,
        5 => -a + c,
        6 => a - c,
        7 => -a - c,
        8 => b + c,
        9 => -b + c,
        10 => b - c,
        11 => -b - c,
        12 => a + b,
        13 => -b + c,
        14 => -a + b,
        15 => -b - c,
        _ => unreachable!(),
    }
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
        let mut lacuna = if lacuna_idx < lacuna_ini.len() {
            lacuna_ini[lacuna_idx]
        } else {
            2.0_f64.powi(omin)
        };

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

    #[cfg(target_arch = "x86_64")]
    pub fn sample_simd(&self, x: f64, y: f64, z: f64) -> f64 {
        use std::arch::x86_64::*;
        
        // Process 4 octaves at a time if we have enough
        let mut value = 0.0;
        let chunks = self.octaves.chunks(4);
        
        for chunk in chunks {
            if chunk.len() == 4 {
                unsafe {
                    // Load 4 lacunarities
                    let lf = [
                        chunk[0].lacunarity,
                        chunk[1].lacunarity,
                        chunk[2].lacunarity,
                        chunk[3].lacunarity,
                    ];
                    let lf_simd = _mm256_loadu_pd(lf.as_ptr());
                    
                    // Scale coordinates
                    let x_simd = _mm256_set1_pd(x);
                    let ax = _mm256_mul_pd(x_simd, lf_simd);
                    
                    // For now, fallback to scalar sample because complete SIMD Perlin is complex
                    // to implement without full gather support for the permutation table.
                    // But we structure it for future expansion.
                     for octave in chunk {
                        let lf = octave.lacunarity;
                        let pv = octave.sample(x * lf, y * lf, z * lf);
                        value += octave.amplitude * pv;
                    }
                }
            } else {
                // Fallback for remaining octaves
                for octave in chunk {
                    let lf = octave.lacunarity;
                    let pv = octave.sample(x * lf, y * lf, z * lf);
                    value += octave.amplitude * pv;
                }
            }
        }
        
        value
    }
}

/// Double Perlin noise (two octave noises combined) - cubiomes-accurate.
#[derive(Debug, Clone, Default)]
pub struct DoublePerlinNoise {
    /// Amplitude for combination
    pub amplitude: f64,
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

        Self {
            amplitude,
            oct_a,
            oct_b,
        }
    }

    /// Sample double Perlin noise.
    pub fn sample(&self, x: f64, y: f64, z: f64) -> f64 {
        const F: f64 = 337.0 / 331.0;
        let v = self.oct_a.sample(x, y, z) + self.oct_b.sample(x * F, y * F, z * F);
        v * self.amplitude
    }
}
