//! Surface rule definitions.
//!
//! Rules produce block states based on the current [`SurfaceContext`].
//! They can be combined using conditions to create complex surface patterns.

use super::condition::Condition;
use super::context::SurfaceContext;
use crate::world::generator::noise::DoublePerlinNoise;

/// Trait for surface rules that produce block states.
///
/// Rules are evaluated in sequence until one returns a block.
pub trait Rule: Send + Sync {
    /// Try to apply this rule at the current position.
    ///
    /// Returns `Some(block_id)` if the rule applies and produces a block,
    /// or `None` if the rule doesn't apply.
    fn try_apply(&self, ctx: &SurfaceContext) -> Option<u32>;
}

/// Returns a constant block.
#[derive(Debug, Clone, Copy)]
pub struct BlockRule {
    /// The block ID to return.
    pub block: u32,
}

impl Rule for BlockRule {
    fn try_apply(&self, _ctx: &SurfaceContext) -> Option<u32> {
        Some(self.block)
    }
}

impl BlockRule {
    /// Create a new block rule.
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
    fn try_apply(&self, ctx: &SurfaceContext) -> Option<u32> {
        for rule in &self.rules {
            if let Some(block) = rule.try_apply(ctx) {
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
    fn try_apply(&self, ctx: &SurfaceContext) -> Option<u32> {
        if self.condition.test(ctx) {
            self.then_run.try_apply(ctx)
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
pub struct BandlandsRule {
    /// Array of 192 terracotta colors indexed by Y.
    pub bands: [u32; 192],
    /// Noise for Y offset variation.
    pub offset_noise: DoublePerlinNoise,
}

impl Rule for BandlandsRule {
    fn try_apply(&self, ctx: &SurfaceContext) -> Option<u32> {
        let offset = (self
            .offset_noise
            .sample(ctx.block_x as f64, 0.0, ctx.block_z as f64)
            * 4.0)
            .round() as i32;
        let index = ((ctx.block_y + offset + 192).rem_euclid(192)) as usize;
        Some(self.bands[index])
    }
}

impl std::fmt::Debug for BandlandsRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BandlandsRule").finish()
    }
}

/// Helper to create a test rule more concisely.
pub fn test_rule<C: Condition + 'static, R: Rule + 'static>(condition: C, then_run: R) -> TestRule {
    TestRule {
        condition: Box::new(condition),
        then_run: Box::new(then_run),
    }
}

/// Helper to create a block rule.
pub fn block(id: u32) -> BlockRule {
    BlockRule { block: id }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::chunk::blocks;
    use crate::world::generator::constants::Biome;
    use crate::world::generator::surface::condition::{BiomeCheck, Steep};

    #[test]
    fn test_block_rule() {
        let rule = BlockRule::new(*blocks::STONE);
        let ctx = SurfaceContext::default();
        assert_eq!(rule.try_apply(&ctx), Some(*blocks::STONE));
    }

    #[test]
    fn test_sequence_rule() {
        let rule = SequenceRule::new(vec![
            Box::new(TestRule::new(
                Box::new(Steep),
                Box::new(BlockRule::new(*blocks::STONE)),
            )),
            Box::new(BlockRule::new(*blocks::GRASS_BLOCK)),
        ]);

        // Non-steep should return grass
        let mut ctx = SurfaceContext::default();
        ctx.steep = false;
        assert_eq!(rule.try_apply(&ctx), Some(*blocks::GRASS_BLOCK));

        // Steep should return stone
        ctx.steep = true;
        assert_eq!(rule.try_apply(&ctx), Some(*blocks::STONE));
    }

    #[test]
    fn test_test_rule_condition_pass() {
        let rule = TestRule::new(
            Box::new(BiomeCheck::single(Biome::Desert)),
            Box::new(BlockRule::new(*blocks::SAND)),
        );

        let mut ctx = SurfaceContext::default();
        ctx.biome = Biome::Desert;
        assert_eq!(rule.try_apply(&ctx), Some(*blocks::SAND));
    }

    #[test]
    fn test_test_rule_condition_fail() {
        let rule = TestRule::new(
            Box::new(BiomeCheck::single(Biome::Desert)),
            Box::new(BlockRule::new(*blocks::SAND)),
        );

        let mut ctx = SurfaceContext::default();
        ctx.biome = Biome::Plains;
        assert_eq!(rule.try_apply(&ctx), None);
    }

    #[test]
    fn test_nested_rules() {
        // Desert -> Sand, Beach -> Sand, else Grass
        let rule = SequenceRule::new(vec![
            Box::new(TestRule::new(
                Box::new(BiomeCheck::multiple(vec![Biome::Desert, Biome::Beach])),
                Box::new(BlockRule::new(*blocks::SAND)),
            )),
            Box::new(BlockRule::new(*blocks::GRASS_BLOCK)),
        ]);

        let mut ctx = SurfaceContext::default();

        ctx.biome = Biome::Desert;
        assert_eq!(rule.try_apply(&ctx), Some(*blocks::SAND));

        ctx.biome = Biome::Beach;
        assert_eq!(rule.try_apply(&ctx), Some(*blocks::SAND));

        ctx.biome = Biome::Forest;
        assert_eq!(rule.try_apply(&ctx), Some(*blocks::GRASS_BLOCK));
    }
}
