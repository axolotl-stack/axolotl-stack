//! Java `WorldgenRandom`: a `java.util.Random`-style generator ("bit random
//! source") on top of either a Xoroshiro128++ or a 48-bit LCG source.
//!
//! Java decoration and structure code always draws through `next(bits)`:
//! - with the Xoroshiro source (decoration), `next(bits)` is the top `bits` of
//!   the source's next 64-bit output, so `nextInt(bound)`, `nextFloat()` and
//!   friends use the `java.util.Random` formulas on those bits, not the
//!   Xoroshiro-native ones;
//! - with the legacy source (structure placement, templates, noise providers)
//!   it is the classic LCG.
//!
//! Seeding helpers mirror `WorldgenRandom.setDecorationSeed`, `setFeatureSeed`,
//! `setLargeFeatureSeed` and `setLargeFeatureWithSalt`.

use crate::xoroshiro::Xoroshiro128;

/// A source of raw random bits with Java reseeding semantics.
pub trait BitSource: Clone {
    /// Create a source seeded like `RandomSource.setSeed(seed)`.
    fn from_seed(seed: i64) -> Self;
    /// `next(bits)` for `1 <= bits <= 32`.
    fn next_bits(&mut self, bits: u32) -> i32;
    /// Native 64-bit output (used by `fork`, which bypasses `next(bits)`).
    fn native_long(&mut self) -> i64;
}

/// Xoroshiro128++ source (`XoroshiroRandomSource`).
#[derive(Debug, Clone)]
pub struct XoroshiroBits(pub Xoroshiro128);

impl BitSource for XoroshiroBits {
    #[inline]
    fn from_seed(seed: i64) -> Self {
        XoroshiroBits(Xoroshiro128::from_seed(seed))
    }
    #[inline]
    fn next_bits(&mut self, bits: u32) -> i32 {
        (self.0.next_long() >> (64 - bits)) as i32
    }
    #[inline]
    fn native_long(&mut self) -> i64 {
        self.0.next_long() as i64
    }
}

/// 48-bit LCG source (`LegacyRandomSource`).
#[derive(Debug, Clone)]
pub struct LegacyBits(u64);

impl LegacyBits {
    const MULTIPLIER: u64 = 0x5_DEEC_E66D;
    const ADDEND: u64 = 0xB;
    const MASK: u64 = (1 << 48) - 1;
}

impl BitSource for LegacyBits {
    #[inline]
    fn from_seed(seed: i64) -> Self {
        LegacyBits((seed as u64 ^ Self::MULTIPLIER) & Self::MASK)
    }
    #[inline]
    fn next_bits(&mut self, bits: u32) -> i32 {
        self.0 = self
            .0
            .wrapping_mul(Self::MULTIPLIER)
            .wrapping_add(Self::ADDEND)
            & Self::MASK;
        (self.0 >> (48 - bits)) as i64 as i32
    }
    #[inline]
    fn native_long(&mut self) -> i64 {
        let hi = self.next_bits(32) as i64;
        let lo = self.next_bits(32) as i64;
        (hi << 32).wrapping_add(lo)
    }
}

/// Java `WorldgenRandom` over a bit source.
#[derive(Debug, Clone)]
pub struct WorldgenRandom<S: BitSource, const RETAIN_GAUSSIAN: bool = true> {
    source: S,
    next_gaussian: Option<f64>,
}

/// Decoration-time random (`WorldgenRandom(XoroshiroRandomSource)`).
pub type FeatureRandom = WorldgenRandom<XoroshiroBits>;
/// Bare `LegacyRandomSource` / `RandomSource.create(seed)`: reseeding clears
/// its Gaussian cache. A wrapped legacy source uses `WorldgenRandom<LegacyBits>`.
pub type LegacyRandom = WorldgenRandom<LegacyBits, false>;

/// Native `WorldGenRegion.getRandom()`, not a `WorldgenRandom` bit wrapper.
/// The preset selects the source once when the region is constructed.
#[derive(Debug, Clone)]
pub enum RegionRandom {
    Xoroshiro(Xoroshiro128),
    Legacy(LegacyRandom),
}

impl RegionRandom {
    /// `RandomState.getOrCreateRandomFactory(worldgen_region_random).at(origin)`.
    /// Both positional forks consume native longs, including the legacy branch.
    pub fn new(seed: i64, legacy: bool, chunk_x: i32, chunk_z: i32) -> Self {
        if legacy {
            let mut root = LegacyRandom::new(seed);
            let positional_seed = root.next_long();
            let mut named = LegacyRandom::new(
                positional_seed ^ i64::from(java_string_hash("minecraft:worldgen_region_random")),
            );
            let region_seed = named.next_long();
            Self::Legacy(LegacyRandom::new(
                region_seed ^ crate::xoroshiro::get_seed(chunk_x * 16, 0, chunk_z * 16),
            ))
        } else {
            Self::Xoroshiro(
                crate::xoroshiro::PositionalRandomFactory::new(seed)
                    .fork_from_hash(crate::random_hashes::WORLDGEN_REGION_RANDOM)
                    .at(chunk_x * 16, 0, chunk_z * 16),
            )
        }
    }

    /// Native `RandomSource.nextBoolean`: each algorithm has different draws.
    #[inline]
    pub fn next_bool(&mut self) -> bool {
        match self {
            Self::Xoroshiro(random) => random.next_boolean(),
            Self::Legacy(random) => random.next_bool(),
        }
    }

    #[inline]
    pub fn next_float(&mut self) -> f32 {
        match self {
            Self::Xoroshiro(random) => random.next_float(),
            Self::Legacy(random) => random.next_float(),
        }
    }

    #[inline]
    pub fn next_int(&mut self, bound: u32) -> u32 {
        match self {
            Self::Xoroshiro(random) => random.next_int(bound),
            Self::Legacy(random) => random.next_int_bounded(bound as i32) as u32,
        }
    }
}

impl<S: BitSource, const RETAIN_GAUSSIAN: bool> WorldgenRandom<S, RETAIN_GAUSSIAN> {
    /// A generator seeded with `seed` (`setSeed`).
    #[inline]
    pub fn new(seed: i64) -> Self {
        Self {
            source: S::from_seed(seed),
            next_gaussian: None,
        }
    }

    /// Wrap an existing source.
    #[inline]
    pub fn from_source(source: S) -> Self {
        Self {
            source,
            next_gaussian: None,
        }
    }

    /// Reseed the bit source. WorldgenRandom's inherited Gaussian cache survives
    /// feature reseeds; a bare LegacyRandomSource clears its own cache.
    #[inline]
    pub fn set_seed(&mut self, seed: i64) {
        self.source = S::from_seed(seed);
        if !RETAIN_GAUSSIAN {
            self.next_gaussian = None;
        }
    }

    /// `next(bits)`.
    #[inline]
    pub fn next(&mut self, bits: u32) -> i32 {
        self.source.next_bits(bits)
    }

    /// `nextInt()`.
    #[inline]
    pub fn next_int(&mut self) -> i32 {
        self.next(32)
    }

    /// `nextInt(bound)`; `bound` must be positive.
    #[inline]
    pub fn next_int_bounded(&mut self, bound: i32) -> i32 {
        debug_assert!(bound > 0, "bound must be positive");
        if bound & (bound - 1) == 0 {
            return ((bound as i64 * self.next(31) as i64) >> 31) as i32;
        }
        loop {
            let bits = self.next(31);
            let val = bits % bound;
            if bits.wrapping_sub(val).wrapping_add(bound - 1) >= 0 {
                return val;
            }
        }
    }

    /// `nextInt(origin, bound)`: `origin + nextInt(bound - origin)`.
    #[inline]
    pub fn next_int_range(&mut self, origin: i32, bound: i32) -> i32 {
        origin + self.next_int_bounded(bound - origin)
    }

    /// `nextIntBetweenInclusive(min, max)` / `Mth.randomBetweenInclusive`.
    #[inline]
    pub fn next_int_between_inclusive(&mut self, min: i32, max: i32) -> i32 {
        self.next_int_bounded(max - min + 1) + min
    }

    /// `nextLong()`.
    #[inline]
    pub fn next_long(&mut self) -> i64 {
        let hi = self.next(32) as i64;
        let lo = self.next(32) as i64;
        (hi << 32).wrapping_add(lo)
    }

    /// `nextBoolean()`.
    #[inline]
    pub fn next_bool(&mut self) -> bool {
        self.next(1) != 0
    }

    /// `nextFloat()`.
    #[inline]
    pub fn next_float(&mut self) -> f32 {
        self.next(24) as f32 * 5.960_464_5e-8
    }

    /// `nextDouble()`.
    #[inline]
    pub fn next_double(&mut self) -> f64 {
        let hi = self.next(26) as i64;
        let lo = self.next(27) as i64;
        ((hi << 27) + lo) as f64 * 1.110_223_024_625_156_5e-16
    }

    /// `nextGaussian()` (Marsaglia polar method with one cached value).
    pub fn next_gaussian(&mut self) -> f64 {
        if let Some(g) = self.next_gaussian.take() {
            return g;
        }
        loop {
            let a = 2.0 * self.next_double() - 1.0;
            let b = 2.0 * self.next_double() - 1.0;
            let s = a * a + b * b;
            if s < 1.0 && s != 0.0 {
                let m = (-2.0 * s.ln() / s).sqrt();
                self.next_gaussian = Some(b * m);
                return a * m;
            }
        }
    }

    /// `triangle(mode, deviation)`: `mode + deviation * (nextDouble() - nextDouble())`.
    #[inline]
    pub fn triangle(&mut self, mode: f64, deviation: f64) -> f64 {
        mode + deviation * (self.next_double() - self.next_double())
    }

    /// `consumeCount(n)`: draws and discards `n` ints.
    #[inline]
    pub fn consume_count(&mut self, n: i32) {
        for _ in 0..n {
            self.next_int();
        }
    }

    /// `fork()`: a new generator seeded from two native longs of the source.
    pub fn fork(&mut self) -> Self
    where
        S: ForkSource,
    {
        Self::from_source(S::fork(&mut self.source))
    }

    /// `setDecorationSeed(levelSeed, minBlockX, minBlockZ)`; returns the seed.
    pub fn set_decoration_seed(&mut self, level_seed: i64, x: i32, z: i32) -> i64 {
        self.set_seed(level_seed);
        let a = self.next_long() | 1;
        let b = self.next_long() | 1;
        let seed =
            ((x as i64).wrapping_mul(a)).wrapping_add((z as i64).wrapping_mul(b)) ^ level_seed;
        self.set_seed(seed);
        seed
    }

    /// `setFeatureSeed(decorationSeed, index, step)`.
    #[inline]
    pub fn set_feature_seed(&mut self, decoration_seed: i64, index: i32, step: i32) {
        let seed = decoration_seed
            .wrapping_add(index as i64)
            .wrapping_add(10_000i64 * step as i64);
        self.set_seed(seed);
    }

    /// `setLargeFeatureSeed(levelSeed, chunkX, chunkZ)`.
    pub fn set_large_feature_seed(&mut self, level_seed: i64, chunk_x: i32, chunk_z: i32) {
        self.set_seed(level_seed);
        let a = self.next_long();
        let b = self.next_long();
        let seed =
            ((chunk_x as i64).wrapping_mul(a)) ^ ((chunk_z as i64).wrapping_mul(b)) ^ level_seed;
        self.set_seed(seed);
    }

    /// `setLargeFeatureWithSalt(levelSeed, regionX, regionZ, salt)`.
    pub fn set_large_feature_with_salt(&mut self, level_seed: i64, x: i32, z: i32, salt: i32) {
        let seed = (x as i64)
            .wrapping_mul(341_873_128_712)
            .wrapping_add((z as i64).wrapping_mul(132_897_987_541))
            .wrapping_add(level_seed)
            .wrapping_add(salt as i64);
        self.set_seed(seed);
    }
}

/// Sources that implement `RandomSource.fork()`.
pub trait ForkSource: BitSource {
    fn fork(&mut self) -> Self;
}

impl ForkSource for XoroshiroBits {
    fn fork(&mut self) -> Self {
        let lo = self.0.next_long();
        let hi = self.0.next_long();
        XoroshiroBits(Xoroshiro128::from_state(lo, hi))
    }
}

impl ForkSource for LegacyBits {
    fn fork(&mut self) -> Self {
        let seed = self.native_long();
        LegacyBits::from_seed(seed)
    }
}

impl<S: BitSource, const RETAIN_GAUSSIAN: bool> crate::noise::NoiseRandom
    for WorldgenRandom<S, RETAIN_GAUSSIAN>
{
    #[inline]
    fn noise_next_double(&mut self) -> f64 {
        self.next_double()
    }
    #[inline]
    fn noise_next_int(&mut self, bound: u32) -> u32 {
        self.next_int_bounded(bound as i32) as u32
    }
}

/// Java `String.hashCode()` (used by `fromHashOf` on legacy positional factories).
pub fn java_string_hash(s: &str) -> i32 {
    let mut h: i32 = 0;
    for unit in s.encode_utf16() {
        h = h.wrapping_mul(31).wrapping_add(unit as i32);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_gaussian_reseed_preserves_wrapper_cache_and_continuation() {
        fn check<S: BitSource, const RETAIN_GAUSSIAN: bool>(case: &serde_json::Value) {
            let mut random = WorldgenRandom::<S, RETAIN_GAUSSIAN>::new(
                case["first_seed"].as_i64().unwrap(),
            );
            for expected in case["initial_gaussian_bits"].as_array().unwrap() {
                assert_eq!(
                    random.next_gaussian().to_bits(),
                    expected.as_str().unwrap().parse::<u64>().unwrap(),
                    "initial Gaussian: {case}",
                );
            }
            random.set_seed(case["second_seed"].as_i64().unwrap());
            assert_eq!(
                random.next_gaussian().to_bits(),
                case["gaussian_after_reseed_bits"].as_str().unwrap().parse::<u64>().unwrap(),
                "Gaussian after reseed: {case}",
            );
            for expected in case["next_ints"].as_array().unwrap() {
                assert_eq!(i64::from(random.next_int()), expected.as_i64().unwrap(),
                    "continuation after reseed: {case}");
            }
        }
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/data/native_gaussian_reseed_1_21_11.json"
        )).unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            match case["algorithm"].as_str().unwrap() {
                "worldgen_xoroshiro" => check::<XoroshiroBits, true>(case),
                "worldgen_legacy" => check::<LegacyBits, true>(case),
                "legacy" => check::<LegacyBits, false>(case),
                other => panic!("unmeasured native random algorithm {other}"),
            }
        }
    }

    /// Native 1.21.11 RandomState positional region stream observations:
    /// native-column-corpus-20261027/columns-{dimension}-seed{seed}.json.
    /// Each tuple is nextBoolean, nextFloat raw f32 bits, nextInt(3).
    #[test]
    fn region_stream_matches_native_dimension_sources() {
        let cases = [
            (0_i64, false, 0, 0, [(true, 1044136244, 1), (true, 1062540261, 0), (false, 1059860540, 1), (false, 1050665236, 1)]),
            (0_i64, false, -1, -1, [(true, 1046372152, 0), (true, 1062444189, 2), (true, 1059300077, 0), (true, 1045930116, 2)]),
            (0_i64, true, 0, 0, [(true, 1059351337, 0), (true, 1044964440, 0), (false, 1057639355, 0), (false, 1063198417, 1)]),
            (0_i64, true, -1, -1, [(false, 1057640546, 0), (true, 1056643492, 2), (false, 1052133062, 0), (true, 1044943764, 1)]),
            (12345_i64, false, 0, 0, [(true, 1055095394, 2), (true, 1060382677, 0), (false, 1058479631, 0), (true, 1064573554, 0)]),
            (12345_i64, false, -1, -1, [(true, 1057253039, 0), (false, 1032799560, 1), (true, 1064303708, 2), (true, 1063928557, 2)]),
            (12345_i64, true, 0, 0, [(false, 1060887882, 0), (true, 1048116300, 0), (false, 1062769422, 1), (false, 1039774104, 0)]),
            (12345_i64, true, -1, -1, [(false, 1064150167, 2), (false, 1061115056, 2), (true, 1047642276, 0), (true, 1050082240, 2)]),
            (987654321_i64, false, 0, 0, [(false, 1055886994, 1), (false, 1059101805, 2), (false, 1046739660, 1), (false, 1058270747, 1)]),
            (987654321_i64, false, -1, -1, [(false, 1062297281, 1), (true, 1064267042, 0), (false, 1040087160, 2), (true, 1045794568, 2)]),
            (987654321_i64, true, 0, 0, [(true, 1036552104, 2), (true, 1056253428, 0), (false, 1054179556, 2), (true, 1058612099, 2)]),
            (987654321_i64, true, -1, -1, [(false, 1057201008, 2), (true, 1051809232, 1), (true, 1057335793, 2), (false, 1053390844, 0)]),
        ];
        for (seed, legacy, cx, cz, expected) in cases {
            let mut random = RegionRandom::new(seed, legacy, cx, cz);
            for (boolean, float_bits, bounded) in expected {
                assert_eq!(random.next_bool(), boolean, "{seed}/{legacy}/{cx}/{cz}");
                assert_eq!(random.next_float().to_bits(), float_bits, "{seed}/{legacy}/{cx}/{cz}");
                assert_eq!(random.next_int(3), bounded, "{seed}/{legacy}/{cx}/{cz}");
            }
        }
    }

    /// `new java.util.Random(42)`: nextInt() = -1170105035 then 234785527, so
    /// nextLong() = (-1170105035 << 32) + 234785527; nextInt(10) = 0.
    #[test]
    fn legacy_matches_java_util_random() {
        let mut r = LegacyRandom::new(42);
        assert_eq!(r.next_int(), -1_170_105_035);
        let mut r = LegacyRandom::new(42);
        assert_eq!(r.next_int_bounded(10), 0);
        let mut r = LegacyRandom::new(42);
        assert_eq!(r.next_long(), (-1_170_105_035i64 << 32) + 234_785_527);
    }

    #[test]
    fn feature_seed_formula() {
        let mut a = FeatureRandom::new(0);
        a.set_feature_seed(1234, 5, 6);
        let mut b = FeatureRandom::new(1234 + 5 + 60_000);
        assert_eq!(a.next_long(), b.next_long());
    }

    #[test]
    fn java_hash() {
        assert_eq!(java_string_hash("hello"), 99_162_322);
    }
}
