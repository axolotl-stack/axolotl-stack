//! Algorithms of the block-placing JSON feature types. The compiled data calls
//! `features::<type>::place(ctx, pos, ...)` with `static` parameters emitted by
//! `unastar_bedrock_codegen` (composition types and `single_block_feature` are
//! compiled inline and have no module here).

pub mod carver;
pub mod fossil;
pub mod geode;
pub mod growing_plant;
pub mod horizontal_tree_decoration;
pub mod multiface;
pub mod ore;
pub mod partially_exposed_blob;
pub mod sculk_patch;
pub mod structure_template;
pub mod tree;
pub mod vegetation_patch;

/// An inclusive integer range (`{"range_min": a, "range_max": b}` in pack data).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntRange {
    pub min: i32,
    pub max: i32,
}

impl IntRange {
    /// Uniform draw in `[min, max]`.
    #[inline]
    pub fn sample(self, rng: &mut super::rng::BedrockRandom) -> i32 {
        rng.next_int_inclusive(self.min, self.max)
    }
}

/// A chance as written in pack data.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Chance {
    /// `numerator / denominator`.
    Fraction(i32, i32),
    /// Probability in `[0, 1]`.
    Probability(f32),
}

impl Chance {
    #[inline]
    pub fn roll(self, rng: &mut super::rng::BedrockRandom) -> bool {
        match self {
            Chance::Fraction(n, d) => rng.chance(n, d),
            Chance::Probability(p) => rng.probability(p),
        }
    }
}
