//! Bedrock's float32 simplex primitive and compile-time permutation tables.
//!
//! Shared by the measured Molang biome-info field and frozen-ocean climate
//! octaves. Keep the operation order: native scalar controls distinguish this
//! path from Java's f64 kernel and from regrouping the fourth-power falloff.

use super::rng::BedrockRandom;

pub(super) struct SimplexNoise {
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

impl SimplexNoise {
    pub(super) const fn new(random: &mut BedrockRandom) -> Self {
        // Constructor offsets are unused in 2D, but their draws precede the
        // shuffle. Taking an existing stream also preserves octave sequencing.
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
        Self {
            permutation,
            gradients,
        }
    }

    #[inline]
    fn gradient(&self, i: usize, j: usize) -> [f32; 2] {
        self.gradients[(i + self.permutation[j & 255] as usize) & 255]
    }

    #[inline]
    pub(super) fn get_value_2d(&self, x: f32, z: f32) -> f32 {
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
        let a = corner(self.gradient(ii, jj), x0, z0);
        let b = corner(self.gradient(ii + di, jj + dj), x1, z1);
        let c = corner(self.gradient(ii + 1, jj + 1), x2, z2);
        70.0 * (a + b + c)
    }
}

#[inline]
fn corner(gradient: [f32; 2], x: f32, z: f32) -> f32 {
    let t = 0.5 - x * x - z * z;
    if t < 0.0 {
        return 0.0;
    }
    let dot = gradient[0] * x + gradient[1] * z;
    // Left association is observable; do not replace with (t*t)*(t*t).
    t * t * t * t * dot
}
