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

    /// Generate a random f32 in [0.0, 1.0).
    pub fn next_float(&mut self) -> f32 {
        (self.next_long() >> 40) as f32 / (1u32 << 24) as f32
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
        self.seed = self
            .seed
            .wrapping_mul(Self::MULTIPLIER)
            .wrapping_add(Self::ADDEND)
            & Self::MASK;
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

    /// Skip n iterations of the RNG.
    ///
    /// This advances the internal state as if `next()` was called n times.
    /// Used for chunk-deterministic randomness where we need to skip to
    /// a specific position in the sequence.
    pub fn skip(&mut self, n: i64) {
        // For small n, just iterate
        // For large n, we could use fast-forward with modular exponentiation,
        // but for typical use cases (chunk offsets), iteration is fine
        for _ in 0..n.unsigned_abs() {
            self.next(1);
        }
    }
}

/// Compute a seed from block coordinates.
/// This is Java's `Mth.getSeed(x, y, z)` implementation.
#[inline]
pub fn get_seed(x: i32, y: i32, z: i32) -> i64 {
    let mut l = (x as i64)
        .wrapping_mul(3129871)
        ^ ((z as i64).wrapping_mul(116129781))
        ^ (y as i64);
    l = l.wrapping_mul(l).wrapping_mul(42317861).wrapping_add(l.wrapping_mul(11));
    l >> 16
}

/// Positional random factory for creating position-dependent RNGs.
/// This matches Java's `XoroshiroRandomSource.XoroshiroPositionalRandomFactory`.
#[derive(Debug, Clone)]
pub struct PositionalRandomFactory {
    seed_lo: i64,
    seed_hi: i64,
}

// MD5 hash of "minecraft:aquifer" - precomputed for performance.
// Java: RandomSupport.seedFromHashOf("minecraft:aquifer")
const AQUIFER_HASH_LO: i64 = 0x7bb15cc403c6ace6_u64 as i64;
const AQUIFER_HASH_HI: i64 = 0x0bdd56bc9d232691_u64 as i64;

// MD5 hash of "minecraft:ore" - precomputed for performance.
// Java: RandomSupport.seedFromHashOf("minecraft:ore")
// MD5("minecraft:ore") = 9b88124de600116d2ae68055aa4a7761
const ORE_HASH_LO: i64 = 0x9b88124de600116d_u64 as i64;
const ORE_HASH_HI: i64 = 0x2ae68055aa4a7761_u64 as i64;

impl PositionalRandomFactory {
    /// Create a new positional random factory from a world seed.
    /// This matches Java's `RandomSource.create(seed).forkPositional()`.
    pub fn new(seed: i64) -> Self {
        let mut rng = Xoroshiro128::from_seed(seed);
        let seed_lo = rng.next_long() as i64;
        let seed_hi = rng.next_long() as i64;
        Self { seed_lo, seed_hi }
    }

    /// Create from explicit low/high seeds.
    pub fn from_seeds(seed_lo: i64, seed_hi: i64) -> Self {
        Self { seed_lo, seed_hi }
    }

    /// Create a random source at the given position.
    /// This matches Java's `positionalRandomFactory.at(x, y, z)`.
    #[inline]
    pub fn at(&self, x: i32, y: i32, z: i32) -> Xoroshiro128 {
        let l = get_seed(x, y, z);
        let m = l ^ self.seed_lo;
        Xoroshiro128::from_state(m as u64, self.seed_hi as u64)
    }

    /// Create a Xoroshiro128 RNG from a hash of the given string, XOR'd with our seeds.
    /// This matches Java's `PositionalRandomFactory.fromHashOf(String)`.
    ///
    /// Java implementation:
    /// ```java
    /// public RandomSource fromHashOf(String string) {
    ///     RandomSupport.Seed128bit seed128bit = RandomSupport.seedFromHashOf(string);
    ///     return new XoroshiroRandomSource(seed128bit.xor(this.seedLo, this.seedHi));
    /// }
    /// ```
    pub fn from_hash_of_aquifer(&self) -> Xoroshiro128 {
        // XOR the precomputed MD5 hash with our seeds
        let lo = AQUIFER_HASH_LO ^ self.seed_lo;
        let hi = AQUIFER_HASH_HI ^ self.seed_hi;
        Xoroshiro128::from_state(lo as u64, hi as u64)
    }

    /// Create a Xoroshiro128 RNG from hash of "minecraft:ore", XOR'd with our seeds.
    pub fn from_hash_of_ore(&self) -> Xoroshiro128 {
        // XOR the precomputed MD5 hash with our seeds
        let lo = ORE_HASH_LO ^ self.seed_lo;
        let hi = ORE_HASH_HI ^ self.seed_hi;
        Xoroshiro128::from_state(lo as u64, hi as u64)
    }

    /// Create the aquifer positional random factory.
    /// This matches Java's `this.random.fromHashOf("minecraft:aquifer").forkPositional()`.
    ///
    /// Java: RandomState.java line 38:
    /// `this.aquiferRandom = this.random.fromHashOf(Identifier.withDefaultNamespace("aquifer")).forkPositional();`
    pub fn fork_aquifer_random(&self) -> PositionalRandomFactory {
        let mut rng = self.from_hash_of_aquifer();
        let seed_lo = rng.next_long() as i64;
        let seed_hi = rng.next_long() as i64;
        PositionalRandomFactory { seed_lo, seed_hi }
    }

    /// Create the ore positional random factory.
    /// This matches Java's `this.random.fromHashOf("minecraft:ore").forkPositional()`.
    ///
    /// Java: RandomState.java line 39:
    /// `this.oreRandom = this.random.fromHashOf(Identifier.withDefaultNamespace("ore")).forkPositional();`
    pub fn fork_ore_random(&self) -> PositionalRandomFactory {
        let mut rng = self.from_hash_of_ore();
        let seed_lo = rng.next_long() as i64;
        let seed_hi = rng.next_long() as i64;
        PositionalRandomFactory { seed_lo, seed_hi }
    }

    /// Get the seed_lo value.
    pub fn seed_lo(&self) -> i64 {
        self.seed_lo
    }

    /// Get the seed_hi value.
    pub fn seed_hi(&self) -> i64 {
        self.seed_hi
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
