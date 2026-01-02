//! Surface rule definitions.
//!
//! Rules produce block states based on the current [`SurfaceContext`].
//! They can be combined using conditions to create complex surface patterns.
//!
//! # Block Lookup
//!
//! Block IDs are resolved at apply time via a closure. This allows:
//! - Generated code in `unastar_noise` (no dependency on block registry)
//! - Custom blocks for modded generation
//! - Late binding of block names to IDs

use super::condition::Condition;
use super::context::SurfaceContext;
use crate::noise::DoublePerlinNoise;

/// Trait for surface rules that produce block states.
///
/// Rules are evaluated in sequence until one returns a block.
/// Block lookup happens via the `get_block` closure at apply time.
pub trait Rule: Send + Sync {
    /// Try to apply this rule at the current position.
    ///
    /// Returns `Some(block_id)` if the rule applies and produces a block,
    /// or `None` if the rule doesn't apply.
    ///
    /// # Arguments
    /// * `ctx` - The surface context with position and biome info
    /// * `get_block` - Closure to resolve block name to ID (e.g., "minecraft:stone" -> 1)
    fn try_apply(&self, ctx: &SurfaceContext, get_block: &dyn Fn(&str) -> u32) -> Option<u32>;
}

/// Returns a constant block (looked up by name at apply time).
#[derive(Debug, Clone)]
pub struct BlockRule {
    /// The block name (e.g., "minecraft:grass_block").
    pub block_name: String,
}

impl Rule for BlockRule {
    fn try_apply(&self, _ctx: &SurfaceContext, get_block: &dyn Fn(&str) -> u32) -> Option<u32> {
        Some(get_block(&self.block_name))
    }
}

impl BlockRule {
    /// Create a new block rule with a block name.
    pub fn new(block_name: impl Into<String>) -> Self {
        Self {
            block_name: block_name.into(),
        }
    }
}

/// Returns a constant block by ID (for use when ID is already known).
#[derive(Debug, Clone, Copy)]
pub struct BlockIdRule {
    /// The block ID to return.
    pub block: u32,
}

impl Rule for BlockIdRule {
    fn try_apply(&self, _ctx: &SurfaceContext, _get_block: &dyn Fn(&str) -> u32) -> Option<u32> {
        Some(self.block)
    }
}

impl BlockIdRule {
    /// Create a new block ID rule.
    pub fn new(block: u32) -> Self {
        Self { block }
    }
}

/// Tries rules in sequence, returns first non-None.
pub struct SequenceRule {
    /// The rules to try in order.
    pub rules: Vec<Box<dyn Rule>>,
}

impl Rule for SequenceRule {
    fn try_apply(&self, ctx: &SurfaceContext, get_block: &dyn Fn(&str) -> u32) -> Option<u32> {
        for rule in &self.rules {
            if let Some(block) = rule.try_apply(ctx, get_block) {
                return Some(block);
            }
        }
        None
    }
}

impl SequenceRule {
    /// Create a new sequence rule.
    pub fn new(rules: Vec<Box<dyn Rule>>) -> Self {
        Self { rules }
    }

    /// Create an empty sequence rule.
    pub fn empty() -> Self {
        Self { rules: Vec::new() }
    }
}

impl std::fmt::Debug for SequenceRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SequenceRule")
            .field("rules_count", &self.rules.len())
            .finish()
    }
}

/// Conditional rule application.
///
/// If the condition passes, delegates to `then_run`.
/// Otherwise returns `None`.
pub struct TestRule {
    /// The condition to check.
    pub condition: Box<dyn Condition>,
    /// The rule to run if the condition passes.
    pub then_run: Box<dyn Rule>,
}

impl Rule for TestRule {
    fn try_apply(&self, ctx: &SurfaceContext, get_block: &dyn Fn(&str) -> u32) -> Option<u32> {
        if self.condition.test(ctx) {
            self.then_run.try_apply(ctx, get_block)
        } else {
            None
        }
    }
}

impl TestRule {
    /// Create a new test rule.
    pub fn new(condition: Box<dyn Condition>, then_run: Box<dyn Rule>) -> Self {
        Self { condition, then_run }
    }
}

impl std::fmt::Debug for TestRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TestRule").finish()
    }
}

/// Badlands terracotta banding rule.
///
/// Creates the distinctive horizontal terracotta bands found in badlands biomes.
/// Block names are resolved at apply time via the closure.
pub struct BandlandsRule {
    /// Array of 192 terracotta block names indexed by Y.
    pub bands: [String; 192],
    /// Noise for Y offset variation.
    pub offset_noise: DoublePerlinNoise,
}

impl Rule for BandlandsRule {
    fn try_apply(&self, ctx: &SurfaceContext, get_block: &dyn Fn(&str) -> u32) -> Option<u32> {
        let offset = (self
            .offset_noise
            .sample(ctx.block_x as f64, 0.0, ctx.block_z as f64)
            * 4.0)
            .round() as i32;
        let index = ((ctx.block_y + offset + 192).rem_euclid(192)) as usize;
        Some(get_block(&self.bands[index]))
    }
}

impl std::fmt::Debug for BandlandsRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BandlandsRule").finish()
    }
}

impl BandlandsRule {
    /// Create a new badlands rule with default terracotta banding.
    ///
    /// Uses the vanilla pattern of colored terracotta bands.
    pub fn new(seed: i64) -> Self {
        use crate::xoroshiro::Xoroshiro128;

        // Create the offset noise
        let mut rng = Xoroshiro128::from_seed(seed.wrapping_add(0xBADBADBAD));
        let offset_noise = DoublePerlinNoise::new(&mut rng, &[1.0], -8);

        // Build the band pattern (vanilla pattern)
        let bands = Self::generate_bands(seed);

        Self {
            bands,
            offset_noise,
        }
    }

    /// Generate the vanilla terracotta band pattern.
    fn generate_bands(seed: i64) -> [String; 192] {
        use crate::xoroshiro::Xoroshiro128;

        // Default is regular terracotta
        let mut bands: [String; 192] = std::array::from_fn(|_| "minecraft:terracotta".to_string());

        // Use random to place bands
        let mut rng = Xoroshiro128::from_seed(seed);

        // Place orange terracotta bands
        for _ in 0..4 {
            let start = (rng.next_float() * 192.0) as usize;
            for i in 0..((rng.next_float() * 3.0 + 1.0) as usize) {
                if start + i < 192 {
                    bands[start + i] = "minecraft:orange_terracotta".to_string();
                }
            }
        }

        // Place yellow terracotta bands
        for _ in 0..2 {
            let start = (rng.next_float() * 192.0) as usize;
            for i in 0..((rng.next_float() * 2.0 + 1.0) as usize) {
                if start + i < 192 {
                    bands[start + i] = "minecraft:yellow_terracotta".to_string();
                }
            }
        }

        // Place brown terracotta bands
        for _ in 0..2 {
            let start = (rng.next_float() * 192.0) as usize;
            for i in 0..((rng.next_float() * 3.0 + 1.0) as usize) {
                if start + i < 192 {
                    bands[start + i] = "minecraft:brown_terracotta".to_string();
                }
            }
        }

        // Place red terracotta bands
        for _ in 0..2 {
            let start = (rng.next_float() * 192.0) as usize;
            for i in 0..((rng.next_float() * 2.0 + 1.0) as usize) {
                if start + i < 192 {
                    bands[start + i] = "minecraft:red_terracotta".to_string();
                }
            }
        }

        // Place white terracotta bands (sparse)
        for _ in 0..2 {
            let start = (rng.next_float() * 192.0) as usize;
            if start < 192 {
                bands[start] = "minecraft:white_terracotta".to_string();
            }
        }

        // Place light gray terracotta bands
        for _ in 0..2 {
            let start = (rng.next_float() * 192.0) as usize;
            for i in 0..((rng.next_float() * 2.0 + 1.0) as usize) {
                if start + i < 192 {
                    bands[start + i] = "minecraft:light_gray_terracotta".to_string();
                }
            }
        }

        bands
    }
}

/// Helper to create a test rule more concisely.
pub fn test_rule<C: Condition + 'static, R: Rule + 'static>(condition: C, then_run: R) -> TestRule {
    TestRule {
        condition: Box::new(condition),
        then_run: Box::new(then_run),
    }
}

/// Helper to create a block rule by name.
pub fn block(name: impl Into<String>) -> BlockRule {
    BlockRule::new(name)
}

/// Helper to create a block ID rule.
pub fn block_id(id: u32) -> BlockIdRule {
    BlockIdRule::new(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Biome;
    use crate::surface::condition::{BiomeCheck, Steep};

    // Mock block lookup for tests
    fn mock_get_block(name: &str) -> u32 {
        match name {
            "minecraft:stone" => 1,
            "minecraft:grass_block" => 2,
            "minecraft:sand" => 3,
            "minecraft:terracotta" => 4,
            _ => 0, // air
        }
    }

    #[test]
    fn test_block_rule() {
        let rule = BlockRule::new("minecraft:stone");
        let ctx = SurfaceContext::default();
        assert_eq!(rule.try_apply(&ctx, &mock_get_block), Some(1));
    }

    #[test]
    fn test_block_id_rule() {
        let rule = BlockIdRule::new(42);
        let ctx = SurfaceContext::default();
        assert_eq!(rule.try_apply(&ctx, &mock_get_block), Some(42));
    }

    #[test]
    fn test_sequence_rule() {
        let rule = SequenceRule::new(vec![
            Box::new(TestRule::new(
                Box::new(Steep),
                Box::new(BlockRule::new("minecraft:stone")),
            )),
            Box::new(BlockRule::new("minecraft:grass_block")),
        ]);

        // Non-steep should return grass
        let mut ctx = SurfaceContext::default();
        ctx.steep = false;
        assert_eq!(rule.try_apply(&ctx, &mock_get_block), Some(2)); // grass

        // Steep should return stone
        ctx.steep = true;
        assert_eq!(rule.try_apply(&ctx, &mock_get_block), Some(1)); // stone
    }

    #[test]
    fn test_test_rule_condition_pass() {
        let rule = TestRule::new(
            Box::new(BiomeCheck::single(Biome::Desert)),
            Box::new(BlockRule::new("minecraft:sand")),
        );

        let mut ctx = SurfaceContext::default();
        ctx.biome = Biome::Desert;
        assert_eq!(rule.try_apply(&ctx, &mock_get_block), Some(3)); // sand
    }

    #[test]
    fn test_test_rule_condition_fail() {
        let rule = TestRule::new(
            Box::new(BiomeCheck::single(Biome::Desert)),
            Box::new(BlockRule::new("minecraft:sand")),
        );

        let mut ctx = SurfaceContext::default();
        ctx.biome = Biome::Plains;
        assert_eq!(rule.try_apply(&ctx, &mock_get_block), None);
    }

    #[test]
    fn test_nested_rules() {
        // Desert -> Sand, Beach -> Sand, else Grass
        let rule = SequenceRule::new(vec![
            Box::new(TestRule::new(
                Box::new(BiomeCheck::multiple(vec![Biome::Desert, Biome::Beach])),
                Box::new(BlockRule::new("minecraft:sand")),
            )),
            Box::new(BlockRule::new("minecraft:grass_block")),
        ]);

        let mut ctx = SurfaceContext::default();

        ctx.biome = Biome::Desert;
        assert_eq!(rule.try_apply(&ctx, &mock_get_block), Some(3)); // sand

        ctx.biome = Biome::Beach;
        assert_eq!(rule.try_apply(&ctx, &mock_get_block), Some(3)); // sand

        ctx.biome = Biome::Forest;
        assert_eq!(rule.try_apply(&ctx, &mock_get_block), Some(2)); // grass
    }
}
