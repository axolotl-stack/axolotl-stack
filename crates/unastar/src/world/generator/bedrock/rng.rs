//! Bedrock's game random: a 32-bit Mersenne Twister (MT19937).
//!
//! Bedrock Edition's `Random` is MT19937 seeded with a 32-bit seed; bounded
//! integers are `next_u32() % bound` and floats scale the 32-bit output by
//! 2^-32. This is the generator decoration and built-in features draw from.
//! Byte-exact decoration also needs BDS's per-chunk seeding and each
//! feature's draw order (research W14); until then only the generator itself
//! is pinned (reference vector from the MT19937 reference implementation).

const N: usize = 624;
const M: usize = 397;
const MATRIX_A: u32 = 0x9908_b0df;
const UPPER_MASK: u32 = 0x8000_0000;
const LOWER_MASK: u32 = 0x7fff_ffff;

/// MT19937 with Bedrock's helper draws.
#[derive(Clone)]
pub struct BedrockRandom {
    state: [u32; N],
    index: usize,
}

impl std::fmt::Debug for BedrockRandom {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BedrockRandom")
            .field("index", &self.index)
            .finish_non_exhaustive()
    }
}

impl BedrockRandom {
    /// Seed exactly like the reference `init_genrand`.
    pub fn new(seed: u32) -> Self {
        let mut state = [0u32; N];
        state[0] = seed;
        for i in 1..N {
            state[i] = 1_812_433_253u32
                .wrapping_mul(state[i - 1] ^ (state[i - 1] >> 30))
                .wrapping_add(i as u32);
        }
        Self { state, index: N }
    }

    fn twist(&mut self) {
        for i in 0..N {
            let y = (self.state[i] & UPPER_MASK) | (self.state[(i + 1) % N] & LOWER_MASK);
            let mut next = self.state[(i + M) % N] ^ (y >> 1);
            if y & 1 != 0 {
                next ^= MATRIX_A;
            }
            self.state[i] = next;
        }
        self.index = 0;
    }

    /// Next tempered 32-bit output (`genrand_int32`).
    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        if self.index >= N {
            self.twist();
        }
        let mut y = self.state[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^ (y >> 18)
    }

    /// Uniform integer in `[0, bound)`; `0` when `bound <= 0`.
    #[inline]
    pub fn next_int(&mut self, bound: i32) -> i32 {
        if bound <= 0 {
            return 0;
        }
        (self.next_u32() % bound as u32) as i32
    }

    /// Uniform integer in `[min, max]` (inclusive); `min` when `max <= min`.
    #[inline]
    pub fn next_int_inclusive(&mut self, min: i32, max: i32) -> i32 {
        if max <= min {
            return min;
        }
        min + self.next_int(max - min + 1)
    }

    /// Uniform float in `[0, 1)`.
    #[inline]
    pub fn next_float(&mut self) -> f32 {
        (self.next_u32() as f64 * (1.0 / 4_294_967_296.0)) as f32
    }

    /// `true` with probability `numerator / denominator`.
    #[inline]
    pub fn chance(&mut self, numerator: i32, denominator: i32) -> bool {
        self.next_int(denominator) < numerator
    }

    /// `true` with probability `p` (`p` in `[0, 1]`).
    #[inline]
    pub fn probability(&mut self, p: f32) -> bool {
        self.next_float() < p
    }

    /// Molang `math.random(a, b)`: uniform float in `[a, b)`.
    #[inline]
    pub fn molang_random(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.next_float()
    }

    /// Molang `math.random_integer(a, b)`: uniform integer in `[a, b]`.
    #[inline]
    pub fn molang_random_integer(&mut self, a: f32, b: f32) -> i32 {
        self.next_int_inclusive(a.round() as i32, b.round() as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_mt19937_reference_vector() {
        // init_genrand(5489) reference outputs (mt19937ar.c test vector).
        let mut r = BedrockRandom::new(5489);
        let expected = [
            3_499_211_612u32,
            581_869_302,
            3_890_346_734,
            3_586_334_585,
            545_404_204,
        ];
        for e in expected {
            assert_eq!(r.next_u32(), e);
        }
        // The 10000th output of the default seed, a common MT19937 check.
        let mut r = BedrockRandom::new(5489);
        let mut last = 0;
        for _ in 0..10_000 {
            last = r.next_u32();
        }
        assert_eq!(last, 4_123_659_995);
    }

    #[test]
    fn bounded_draws_stay_in_range() {
        let mut r = BedrockRandom::new(12345);
        for _ in 0..10_000 {
            assert!((0..7).contains(&r.next_int(7)));
            assert!((-3..=3).contains(&r.next_int_inclusive(-3, 3)));
            let f = r.next_float();
            assert!((0.0..1.0).contains(&f));
        }
        assert_eq!(r.next_int(0), 0);
        assert_eq!(r.next_int_inclusive(4, 4), 4);
    }
}
