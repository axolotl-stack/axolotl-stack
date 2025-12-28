//! Xoroshiro128++ random number generator.
//!
//! This is the RNG used by Minecraft for terrain generation.

/// Xoroshiro128++ random number generator.
#[derive(Debug, Clone)]
pub struct Xoroshiro128 {
    low: u64,
    high: u64,
}

impl Xoroshiro128 {
    /// Create a new RNG from a seed using splitmix64 initialization.
    pub fn from_seed(seed: i64) -> Self {
        const XL: u64 = 0x9e3779b97f4a7c15;
        const XH: u64 = 0x6a09e667f3bcc909;
        const A: u64 = 0xbf58476d1ce4e5b9;
        const B: u64 = 0x94d049bb133111eb;

        let seed = seed as u64;

        let mut l = seed ^ XH;
        let mut h = l.wrapping_add(XL);

        l = (l ^ (l >> 30)).wrapping_mul(A);
        h = (h ^ (h >> 30)).wrapping_mul(A);
        l = (l ^ (l >> 27)).wrapping_mul(B);
        h = (h ^ (h >> 27)).wrapping_mul(B);
        l ^= l >> 31;
        h ^= h >> 31;

        Self { low: l, high: h }
    }

    /// Create from explicit low/high state.
    pub fn from_state(low: u64, high: u64) -> Self {
        Self { low, high }
    }

    /// Generate next u64 value.
    pub fn next_long(&mut self) -> u64 {
        let l = self.low;
        let h = self.high;
        let n = l.wrapping_add(h).rotate_left(17).wrapping_add(l);
        let xor = h ^ l;

        self.low = l.rotate_left(49) ^ xor ^ (xor << 21);
        self.high = xor.rotate_left(28);

        n
    }

    /// Generate a random integer in [0, bound).
    pub fn next_int(&mut self, bound: u32) -> u32 {
        let mut r = ((self.next_long() as u32 as u64) * (bound as u64)) as u64;

        if (r as u32) < bound {
            let threshold = ((!bound).wrapping_add(1)) % bound;
            while (r as u32) < threshold {
                r = ((self.next_long() as u32 as u64) * (bound as u64)) as u64;
            }
        }

        (r >> 32) as u32
    }

    /// Generate a random f64 in [0.0, 1.0).
    pub fn next_double(&mut self) -> f64 {
        (self.next_long() >> 11) as f64 * 1.1102230246251565e-16
    }

    /// Fork this RNG into two independent streams.
    pub fn fork(&mut self) -> Self {
        let low = self.next_long();
        let high = self.next_long();
        Self::from_state(low, high)
    }

    /// Fork with MD5 mixing for octave noise.
    pub fn fork_with_md5(&mut self, md5_low: u64, md5_high: u64) -> Self {
        let low = self.next_long();
        let high = self.next_long();
        Self::from_state(low ^ md5_low, high ^ md5_high)
    }
}

/// Linear congruential generator for structure positions.
#[derive(Debug, Clone)]
pub struct JavaRandom {
    seed: u64,
}

impl JavaRandom {
    const MULTIPLIER: u64 = 0x5deece66d;
    const ADDEND: u64 = 0xb;
    const MASK: u64 = (1 << 48) - 1;

    /// Create from seed.
    pub fn from_seed(seed: i64) -> Self {
        Self {
            seed: (seed as u64 ^ Self::MULTIPLIER) & Self::MASK,
        }
    }

    /// Set seed with XOR.
    pub fn set_seed(&mut self, seed: i64) {
        self.seed = (seed as u64 ^ Self::MULTIPLIER) & Self::MASK;
    }

    /// Generate next bits.
    fn next(&mut self, bits: u32) -> i32 {
        self.seed = self.seed.wrapping_mul(Self::MULTIPLIER).wrapping_add(Self::ADDEND) & Self::MASK;
        (self.seed >> (48 - bits)) as i32
    }

    /// Generate random integer in [0, bound).
    pub fn next_int(&mut self, bound: u32) -> i32 {
        let m = bound.wrapping_sub(1);

        if (m & bound) == 0 {
            // Power of 2
            let x = (bound as i64) * (self.next(31) as i64);
            return (x >> 31) as i32;
        }

        loop {
            let bits = self.next(31);
            let val = bits % (bound as i32);
            if bits.wrapping_sub(val).wrapping_add(m as i32) >= 0 {
                return val;
            }
        }
    }

    /// Generate random float in [0.0, 1.0).
    pub fn next_float(&mut self) -> f32 {
        self.next(24) as f32 / (1 << 24) as f32
    }

    /// Generate random double in [0.0, 1.0).
    pub fn next_double(&mut self) -> f64 {
        let high = (self.next(26) as i64) << 27;
        let low = self.next(27) as i64;
        (high + low) as f64 / ((1i64 << 53) as f64)
    }

    /// Generate random long (i64).
    pub fn next_long(&mut self) -> i64 {
        let high = (self.next(32) as i64) << 32;
        let low = self.next(32) as i64 & 0xffffffff;
        high + low
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xoroshiro_from_seed() {
        let mut rng = Xoroshiro128::from_seed(0);
        let v1 = rng.next_long();
        let v2 = rng.next_long();
        // Values should be deterministic
        assert_ne!(v1, v2);
    }

    #[test]
    fn test_java_random() {
        let mut rng = JavaRandom::from_seed(12345);
        let v1 = rng.next_int(100);
        let v2 = rng.next_int(100);
        assert!(v1 >= 0 && v1 < 100);
        assert!(v2 >= 0 && v2 < 100);
    }
}
