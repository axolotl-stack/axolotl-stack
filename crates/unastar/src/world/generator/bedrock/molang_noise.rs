//! World-independent Molang `query.noise(x, z)` for the Bedrock generator.
//!
//! Authored BDS 1.26.51 controls match a fixed MT19937(2345) permutation,
//! three constructor offset draws and native float32 simplex arithmetic. The
//! permutation and final hash gradients are evaluated at compile time: no
//! world-seed mixing, RNG initialization, permutation copies or allocation at
//! runtime. Scalar/bit-pattern controls live in `bds_1_26_51_molang_noise.json`.

use super::rng::BedrockRandom;
use super::world::NoiseSource;

/// Stateless native noise intrinsic; deliberately takes no world seed.
pub struct MolangNoise;

struct Tables {
    permutation: [u8; 256],
    gradients: [[f32; 2]; 256],
}

const GRADIENTS: [[f32; 2]; 12] = [
    [1.0, 1.0],
    [-1.0, 1.0],
    [1.0, -1.0],
    [-1.0, -1.0],
    [1.0, 0.0],
    [-1.0, 0.0],
    [1.0, 0.0],
    [-1.0, 0.0],
    [0.0, 1.0],
    [0.0, -1.0],
    [0.0, 1.0],
    [0.0, -1.0],
];

static TABLES: Tables = tables();

const fn tables() -> Tables {
    let mut random = BedrockRandom::new(2345);
    // The constructor's three offsets are unused by 2D sampling, but their
    // draws precede the shuffle and must not be omitted.
    let mut i = 0;
    while i < 3 {
        random.next_u32();
        i += 1;
    }
    let mut permutation = [0; 256];
    i = 0;
    while i < 256 {
        permutation[i] = i as u8;
        i += 1;
    }
    i = 0;
    while i < 256 {
        let j = i + (random.next_u32() % (256 - i) as u32) as usize;
        let old = permutation[i];
        permutation[i] = permutation[j];
        permutation[j] = old;
        i += 1;
    }
    let mut gradients = [[0.0; 2]; 256];
    i = 0;
    while i < 256 {
        gradients[i] = GRADIENTS[(permutation[i] % 12) as usize];
        i += 1;
    }
    Tables {
        permutation,
        gradients,
    }
}

#[inline]
fn gradient(i: usize, j: usize) -> [f32; 2] {
    TABLES.gradients[(i + TABLES.permutation[j & 255] as usize) & 255]
}

#[inline]
fn corner(gradient: [f32; 2], x: f32, z: f32) -> f32 {
    let t = 0.5 - x * x - z * z;
    if t < 0.0 {
        return 0.0;
    }
    let dot = gradient[0] * x + gradient[1] * z;
    // Keep float32 and left association. Java's (t*t)*(t*t) changes observed
    // native bits even at small coordinates; f64 also diverges at large ones.
    t * t * t * t * dot
}

impl NoiseSource for MolangNoise {
    #[inline]
    fn noise(&self, x: f32, z: f32) -> f32 {
        const F2: f32 = 0.366_025_4;
        const G2: f32 = 0.211_324_87;
        let skew = (x + z) * F2;
        let i = (x + skew).floor() as i32;
        let j = (z + skew).floor() as i32;
        let unskew = (i + j) as f32 * G2;
        let x0 = x - (i as f32 - unskew);
        let z0 = z - (j as f32 - unskew);
        let (di, dj) = if x0 > z0 { (1, 0) } else { (0, 1) };
        let x1 = x0 - di as f32 + G2;
        let z1 = z0 - dj as f32 + G2;
        let x2 = x0 - 1.0 + 2.0 * G2;
        let z2 = z0 - 1.0 + 2.0 * G2;
        let ii = (i & 255) as usize;
        let jj = (j & 255) as usize;
        let a = corner(gradient(ii, jj), x0, z0);
        let b = corner(gradient(ii + di, jj + dj), x1, z1);
        let c = corner(gradient(ii + 1, jj + 1), x2, z2);
        70.0 * (a + b + c)
    }
}
