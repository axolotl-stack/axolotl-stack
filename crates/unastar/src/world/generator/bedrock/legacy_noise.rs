//! Bedrock's single-precision port of Java's legacy normal noise, built from
//! a feature's own MT19937 stream (the geode shell noise).
//!
//! Construction follows Java's legacy octave layout: for `first_octave = -k`,
//! one improved-noise octave is drawn for index `k` (kept only when an
//! amplitude exists there), then indices `k-1 ..= 0` are drawn or skipped.
//! An octave costs 259 draws (three `next_double`s scaled by 256, then a
//! 256-entry Fisher-Yates shuffle); a skipped octave discards 262, Java's
//! count, although Bedrock's doubles use one 32-bit draw each. A normal
//! noise is two such stacks, the second sampled at `1.0181268882175227`
//! times the input. All arithmetic is `f32`, in the native operation order:
//! the measured seed-12345 geodes are reproduced block for block.

use super::rng::BedrockRandom;

/// Java's 16 improved-noise gradients.
const GRADIENTS: [[f32; 3]; 16] = [
    [1.0, 1.0, 0.0],
    [-1.0, 1.0, 0.0],
    [1.0, -1.0, 0.0],
    [-1.0, -1.0, 0.0],
    [1.0, 0.0, 1.0],
    [-1.0, 0.0, 1.0],
    [1.0, 0.0, -1.0],
    [-1.0, 0.0, -1.0],
    [0.0, 1.0, 1.0],
    [0.0, -1.0, 1.0],
    [0.0, 1.0, -1.0],
    [0.0, -1.0, -1.0],
    [1.0, 1.0, 0.0],
    [0.0, -1.0, 1.0],
    [-1.0, 1.0, 0.0],
    [0.0, -1.0, -1.0],
];

/// Draws a skipped octave discards.
const SKIPPED_OCTAVE_DRAWS: u32 = 262;

/// Input scale of a normal noise's second stack.
const SECOND_STACK_SCALE: f32 = 1.018_126_9;

/// One improved-noise octave: origin offset, permutation, frequency and
/// amplitude.
#[derive(Clone)]
struct Octave {
    origin: [f32; 3],
    permutation: [u8; 256],
    frequency: f32,
    amplitude: f32,
}

impl Octave {
    fn new(rng: &mut BedrockRandom) -> Self {
        let origin = [
            (rng.next_double() * 256.0) as f32,
            (rng.next_double() * 256.0) as f32,
            (rng.next_double() * 256.0) as f32,
        ];
        let mut permutation = [0u8; 256];
        for (i, p) in permutation.iter_mut().enumerate() {
            *p = i as u8;
        }
        for i in 0..256 {
            let j = i + rng.next_int(256 - i as i32) as usize;
            permutation.swap(i, j);
        }
        Self {
            origin,
            permutation,
            frequency: 0.0,
            amplitude: 0.0,
        }
    }

    #[inline]
    fn p(&self, i: i32) -> i32 {
        i32::from(self.permutation[(i & 0xff) as usize])
    }

    #[inline]
    fn grad(&self, hash: i32, x: f32, y: f32, z: f32) -> f32 {
        let g = GRADIENTS[(self.p(hash) & 15) as usize];
        g[0] * x + g[1] * y + g[2] * z
    }

    /// Improved noise at an already-scaled and offset position.
    fn sample(&self, x: f32, y: f32, z: f32) -> f32 {
        let (fx, fy, fz) = (x.floor(), y.floor(), z.floor());
        let (ix, iy, iz) = (fx as i32, fy as i32, fz as i32);
        let (dx, dy, dz) = (x - ix as f32, y - iy as f32, z - iz as f32);
        let i = self.p(ix);
        let j = self.p(ix + 1);
        let k = self.p(i + iy);
        let l = self.p(i + iy + 1);
        let m = self.p(j + iy);
        let n = self.p(j + iy + 1);
        let d = self.grad(k + iz, dx, dy, dz);
        let e = self.grad(m + iz, dx - 1.0, dy, dz);
        let f = self.grad(l + iz, dx, dy - 1.0, dz);
        let g = self.grad(n + iz, dx - 1.0, dy - 1.0, dz);
        let h = self.grad(k + iz + 1, dx, dy, dz - 1.0);
        let o = self.grad(m + iz + 1, dx - 1.0, dy, dz - 1.0);
        let p = self.grad(l + iz + 1, dx, dy - 1.0, dz - 1.0);
        let q = self.grad(n + iz + 1, dx - 1.0, dy - 1.0, dz - 1.0);
        let (u, v, w) = (smoothstep(dx), smoothstep(dy), smoothstep(dz));
        lerp(
            w,
            lerp(v, lerp(u, d, e), lerp(u, f, g)),
            lerp(v, lerp(u, h, o), lerp(u, p, q)),
        )
    }
}

#[inline]
fn smoothstep(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

#[inline]
fn lerp(t: f32, a: f32, b: f32) -> f32 {
    a + t * (b - a)
}

/// One legacy octave stack.
#[derive(Clone)]
struct Stack {
    octaves: Vec<Octave>,
}

impl Stack {
    fn new(rng: &mut BedrockRandom, first_octave: i32, amplitudes: &[f32]) -> Self {
        let top = -first_octave;
        let size = amplitudes.len() as i32;
        let mut slots: Vec<Option<Octave>> = vec![None; amplitudes.len()];
        let first = Octave::new(rng);
        if (0..size).contains(&top) && amplitudes[top as usize] != 0.0 {
            slots[top as usize] = Some(first);
        }
        for index in (0..top).rev() {
            if index < size && amplitudes[index as usize] != 0.0 {
                slots[index as usize] = Some(Octave::new(rng));
            } else {
                rng.skip(SKIPPED_OCTAVE_DRAWS);
            }
        }
        let value_factor = 2f32.powi(size - 1) / (2f32.powi(size) - 1.0);
        let input_factor = 2f32.powi(first_octave);
        let octaves = slots
            .into_iter()
            .enumerate()
            .filter_map(|(i, octave)| {
                octave.map(|mut o| {
                    o.frequency = input_factor * 2f32.powi(i as i32);
                    o.amplitude = amplitudes[i] * value_factor / 2f32.powi(i as i32);
                    o
                })
            })
            .collect();
        Self { octaves }
    }

    fn sample(&self, x: f32, y: f32, z: f32) -> f32 {
        let mut total = 0.0f32;
        for o in &self.octaves {
            total += o.sample(
                o.frequency * x + o.origin[0],
                o.frequency * y + o.origin[1],
                o.frequency * z + o.origin[2],
            ) * o.amplitude;
        }
        total
    }
}

/// Java's normal noise: two stacks, the value scaled to roughly `[-1, 1]`.
#[derive(Clone)]
pub struct NormalNoise {
    first: Stack,
    second: Stack,
    value_factor: f32,
}

impl NormalNoise {
    /// Build from `rng`, consuming the draws Bedrock consumes.
    pub fn new(rng: &mut BedrockRandom, first_octave: i32, amplitudes: &[f32]) -> Self {
        let first = Stack::new(rng, first_octave, amplitudes);
        let second = Stack::new(rng, first_octave, amplitudes);
        let used: Vec<i32> = (0..amplitudes.len() as i32)
            .filter(|&i| amplitudes[i as usize] != 0.0)
            .collect();
        let span = match (used.first(), used.last()) {
            (Some(lo), Some(hi)) => (hi - lo) as f32 + 1.0,
            _ => 2.0,
        };
        Self {
            first,
            second,
            value_factor: 0.166_666_67 / ((1.0 / span + 1.0) * 0.1),
        }
    }

    pub fn sample(&self, x: f32, y: f32, z: f32) -> f32 {
        let a = self.first.sample(x, y, z);
        let b = self.second.sample(
            x * SECOND_STACK_SCALE,
            y * SECOND_STACK_SCALE,
            z * SECOND_STACK_SCALE,
        );
        (a + b) * self.value_factor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_octave_noise_consumes_bedrocks_draw_count() {
        // First octave -4 with one amplitude: per stack an unused octave, three
        // skipped octaves and the used one.
        let mut rng = BedrockRandom::new(1);
        let _ = NormalNoise::new(&mut rng, -4, &[1.0]);
        let mut reference = BedrockRandom::new(1);
        reference.skip(2 * (259 + 3 * 262 + 259));
        assert_eq!(rng.next_u32(), reference.next_u32());
    }
}
