//! Overworld surface rules builder.
//!
//! This module builds the surface rules for the overworld dimension,
//! matching vanilla Minecraft's surface generation.

use super::condition::{AbovePreliminarySurface, BiomeCheck, Hole, Steep, StoneDepthCheck, VerticalGradient, WaterCheck};
use super::context::CaveSurface;
use super::rule::{BlockRule, Rule, SequenceRule, TestRule};
use crate::world::chunk::blocks;
use crate::world::generator::constants::Biome;

/// Build the overworld surface rule matching vanilla.
///
/// This creates a hierarchical rule structure that:
/// 1. Places bedrock at the world bottom
/// 2. Applies biome-specific surface blocks (only above preliminary surface)
/// 3. Handles underwater surfaces
/// 4. Places deepslate in deep regions
///
/// CRITICAL: Main surface rules are wrapped in AbovePreliminarySurface to prevent
/// grass/dirt from appearing on cave ceilings and floors. This matches Java's
/// behavior where surface rules only apply above the computed preliminary surface level.
pub fn build_overworld_surface_rule(seed: i64) -> Box<dyn Rule> {
    Box::new(SequenceRule::new(vec![
        // Bedrock floor (Y=-64 to Y=-60 with gradient)
        build_bedrock_rule(seed),
        // Main surface rules - WRAPPED in AbovePreliminarySurface condition
        // This is critical: prevents grass appearing on cave floors/ceilings
        Box::new(TestRule::new(
            Box::new(AbovePreliminarySurface),
            build_surface_rules(),
        )),
        // Deepslate layer (Y=0 to Y=8, replacing stone) - applied everywhere underground
        build_deepslate_rule(seed),
    ]))
}

/// Build bedrock rule with randomized top.
///
/// Java's bedrock floor uses a simple vertical_gradient:
/// - true_at_and_below: above_bottom(0) = Y=-64 (guaranteed bedrock)
/// - false_at_and_above: above_bottom(5) = Y=-59 (no bedrock)
/// - Between Y=-63 and Y=-60: random gradient from 100% to 0%
fn build_bedrock_rule(seed: i64) -> Box<dyn Rule> {
    // Simple vertical gradient: 100% at Y=-64, 0% at Y=-59
    Box::new(TestRule::new(
        Box::new(VerticalGradient::new(-64, -59, seed)),
        Box::new(BlockRule::new(*blocks::BEDROCK)),
    ))
}

/// Build deepslate transition rule.
///
/// Java's deepslate uses a simple vertical_gradient:
/// - true_at_and_below: absolute Y=0 (guaranteed deepslate)
/// - false_at_and_above: absolute Y=8 (no deepslate)
/// - Between Y=1 and Y=7: random gradient from 100% to 0%
fn build_deepslate_rule(seed: i64) -> Box<dyn Rule> {
    // Simple vertical gradient: 100% at Y=0, 0% at Y=8
    Box::new(TestRule::new(
        Box::new(VerticalGradient::new(0, 8, seed)),
        Box::new(BlockRule::new(*blocks::DEEPSLATE)),
    ))
}

/// Build the main surface rules.
fn build_surface_rules() -> Box<dyn Rule> {
    Box::new(TestRule::new(
        // Only apply at surface (stone depth check)
        Box::new(StoneDepthCheck {
            offset: 0,
            add_surface_depth: true,
            secondary_depth_range: 0,
            surface_type: CaveSurface::Floor,
        }),
        Box::new(SequenceRule::new(vec![
            // Above water surface
            Box::new(TestRule::new(
                Box::new(WaterCheck::new(-1)),
                build_above_water_rules(),
            )),
            // Underwater surface
            build_underwater_rules(),
        ])),
    ))
}

/// Build rules for surfaces above water.
fn build_above_water_rules() -> Box<dyn Rule> {
    Box::new(SequenceRule::new(vec![
        // Frozen biomes: snow/ice surface
        build_frozen_surface_rules(),
        // Sandy biomes: sand surface
        build_sandy_surface_rules(),
        // Mountain biomes: stone/gravel
        build_mountain_surface_rules(),
        // Swamp: grass with occasional clay
        build_swamp_surface_rules(),
        // Savanna: grass with occasional coarse dirt
        build_savanna_surface_rules(),
        // Default: grass with dirt underneath
        build_default_surface_rules(),
    ]))
}

/// Build frozen biome surface rules.
fn build_frozen_surface_rules() -> Box<dyn Rule> {
    let frozen_biomes = vec![
        Biome::SnowyPlains,
        Biome::SnowyTaiga,
        Biome::SnowyMountains,
        Biome::SnowySlopes,
        Biome::FrozenPeaks,
        Biome::IceSpikes,
        Biome::Grove,
    ];

    Box::new(TestRule::new(
        Box::new(BiomeCheck::multiple(frozen_biomes)),
        Box::new(SequenceRule::new(vec![
            // Top layer: snow block
            Box::new(TestRule::new(
                Box::new(StoneDepthCheck::floor(0)),
                Box::new(BlockRule::new(*blocks::SNOW_BLOCK)),
            )),
            // Below: packed ice or dirt
            Box::new(TestRule::new(
                Box::new(StoneDepthCheck::floor_with_depth(1, true)),
                Box::new(BlockRule::new(*blocks::DIRT)),
            )),
        ])),
    ))
}

/// Build sandy biome surface rules (desert, beach).
fn build_sandy_surface_rules() -> Box<dyn Rule> {
    let sandy_biomes = vec![Biome::Desert, Biome::Beach];

    Box::new(TestRule::new(
        Box::new(BiomeCheck::multiple(sandy_biomes)),
        Box::new(SequenceRule::new(vec![
            // Top layers: sand
            Box::new(TestRule::new(
                Box::new(StoneDepthCheck::floor_with_depth(0, true)),
                Box::new(BlockRule::new(*blocks::SAND)),
            )),
            // Deeper: sandstone (in desert)
            Box::new(TestRule::new(
                Box::new(BiomeCheck::single(Biome::Desert)),
                Box::new(BlockRule::new(*blocks::SANDSTONE)),
            )),
        ])),
    ))
}

/// Build mountain surface rules.
fn build_mountain_surface_rules() -> Box<dyn Rule> {
    let mountain_biomes = vec![
        Biome::WindsweptHills,
        Biome::StonyPeaks,
        Biome::JaggedPeaks,
    ];

    Box::new(TestRule::new(
        Box::new(BiomeCheck::multiple(mountain_biomes)),
        Box::new(SequenceRule::new(vec![
            // Steep slopes: exposed stone
            Box::new(TestRule::new(
                Box::new(Steep),
                Box::new(BlockRule::new(*blocks::STONE)),
            )),
            // Flat areas: grass or gravel
            Box::new(TestRule::new(
                Box::new(StoneDepthCheck::floor(0)),
                Box::new(BlockRule::new(*blocks::GRASS_BLOCK)),
            )),
        ])),
    ))
}

/// Build swamp surface rules.
fn build_swamp_surface_rules() -> Box<dyn Rule> {
    Box::new(TestRule::new(
        Box::new(BiomeCheck::multiple(vec![Biome::Swamp, Biome::MangroveSwamp])),
        Box::new(SequenceRule::new(vec![
            // Top: grass
            Box::new(TestRule::new(
                Box::new(StoneDepthCheck::floor(0)),
                Box::new(BlockRule::new(*blocks::GRASS_BLOCK)),
            )),
            // Below: dirt or clay
            Box::new(TestRule::new(
                Box::new(StoneDepthCheck::floor_with_depth(1, true)),
                Box::new(BlockRule::new(*blocks::DIRT)),
            )),
        ])),
    ))
}

/// Build savanna surface rules.
fn build_savanna_surface_rules() -> Box<dyn Rule> {
    Box::new(TestRule::new(
        Box::new(BiomeCheck::multiple(vec![
            Biome::Savanna,
            Biome::SavannaPlateau,
            Biome::WindsweptSavanna,
        ])),
        Box::new(SequenceRule::new(vec![
            // Top: grass (occasionally coarse dirt handled elsewhere)
            Box::new(TestRule::new(
                Box::new(StoneDepthCheck::floor(0)),
                Box::new(BlockRule::new(*blocks::GRASS_BLOCK)),
            )),
            // Below: dirt
            Box::new(TestRule::new(
                Box::new(StoneDepthCheck::floor_with_depth(1, true)),
                Box::new(BlockRule::new(*blocks::DIRT)),
            )),
        ])),
    ))
}

/// Build default surface rules (most biomes).
fn build_default_surface_rules() -> Box<dyn Rule> {
    Box::new(SequenceRule::new(vec![
        // Check for hole (no surface)
        Box::new(TestRule::new(
            Box::new(Hole),
            Box::new(BlockRule::new(*blocks::STONE)),
        )),
        // Top layer: grass
        Box::new(TestRule::new(
            Box::new(StoneDepthCheck::floor(0)),
            Box::new(BlockRule::new(*blocks::GRASS_BLOCK)),
        )),
        // Below grass: dirt
        Box::new(TestRule::new(
            Box::new(StoneDepthCheck::floor_with_depth(1, true)),
            Box::new(BlockRule::new(*blocks::DIRT)),
        )),
    ]))
}

/// Build underwater surface rules.
fn build_underwater_rules() -> Box<dyn Rule> {
    Box::new(SequenceRule::new(vec![
        // Ocean floor
        Box::new(TestRule::new(
            Box::new(BiomeCheck::multiple(vec![
                Biome::Ocean,
                Biome::DeepOcean,
                Biome::WarmOcean,
                Biome::LukewarmOcean,
                Biome::ColdOcean,
                Biome::DeepColdOcean,
                Biome::DeepLukewarmOcean,
                Biome::DeepFrozenOcean,
            ])),
            Box::new(SequenceRule::new(vec![
                // Top: sand or gravel depending on depth
                Box::new(TestRule::new(
                    Box::new(StoneDepthCheck::floor(0)),
                    Box::new(BlockRule::new(*blocks::SAND)),
                )),
                // Below: sand
                Box::new(TestRule::new(
                    Box::new(StoneDepthCheck::floor_with_depth(1, true)),
                    Box::new(BlockRule::new(*blocks::SAND)),
                )),
            ])),
        )),
        // River bed
        Box::new(TestRule::new(
            Box::new(BiomeCheck::multiple(vec![Biome::River, Biome::FrozenRiver])),
            Box::new(SequenceRule::new(vec![
                Box::new(TestRule::new(
                    Box::new(StoneDepthCheck::floor(0)),
                    Box::new(BlockRule::new(*blocks::SAND)),
                )),
                Box::new(TestRule::new(
                    Box::new(StoneDepthCheck::floor_with_depth(1, true)),
                    Box::new(BlockRule::new(*blocks::CLAY)),
                )),
            ])),
        )),
        // Default underwater: gravel
        Box::new(TestRule::new(
            Box::new(StoneDepthCheck::floor_with_depth(0, true)),
            Box::new(BlockRule::new(*blocks::GRAVEL)),
        )),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::generator::surface::SurfaceContext;

    #[test]
    fn test_build_overworld_surface_rule() {
        let rule = build_overworld_surface_rule(12345);
        let ctx = SurfaceContext::default();
        // Should not panic
        let _ = rule.try_apply(&ctx);
    }

    #[test]
    fn test_desert_surface() {
        let rule = build_overworld_surface_rule(12345);
        let mut ctx = SurfaceContext::default();
        ctx.biome = Biome::Desert;
        ctx.block_y = 64;
        ctx.stone_depth_above = 0;
        ctx.surface_depth = 3;
        ctx.water_height = i32::MIN; // No water
        ctx.min_surface_level = 60; // Above preliminary surface

        let result = rule.try_apply(&ctx);
        // Desert should get sand at surface
        assert_eq!(result, Some(*blocks::SAND), "Desert surface should be sand");
    }

    #[test]
    fn test_plains_surface() {
        let rule = build_overworld_surface_rule(12345);
        let mut ctx = SurfaceContext::default();
        ctx.biome = Biome::Plains;
        ctx.block_y = 64;
        ctx.stone_depth_above = 0;
        ctx.surface_depth = 3;
        ctx.water_height = i32::MIN;
        ctx.min_surface_level = 60; // Above preliminary surface

        let result = rule.try_apply(&ctx);
        // Plains should get grass at surface
        assert_eq!(result, Some(*blocks::GRASS_BLOCK), "Plains surface should be grass");
    }

    #[test]
    fn test_snowy_surface() {
        let rule = build_overworld_surface_rule(12345);
        let mut ctx = SurfaceContext::default();
        ctx.biome = Biome::SnowyPlains;
        ctx.block_y = 64;
        ctx.stone_depth_above = 0;
        ctx.surface_depth = 3;
        ctx.water_height = i32::MIN;
        ctx.min_surface_level = 60; // Above preliminary surface

        let result = rule.try_apply(&ctx);
        // Snowy biomes should get snow
        assert_eq!(result, Some(*blocks::SNOW_BLOCK), "Snowy surface should be snow");
    }

    #[test]
    fn test_ocean_floor() {
        let rule = build_overworld_surface_rule(12345);
        let mut ctx = SurfaceContext::default();
        ctx.biome = Biome::Ocean;
        ctx.block_y = 50;
        ctx.stone_depth_above = 0;
        ctx.surface_depth = 3;
        ctx.water_height = 63; // Underwater
        ctx.min_surface_level = 45; // Above preliminary surface

        let result = rule.try_apply(&ctx);
        // Ocean floor should be sand
        assert_eq!(result, Some(*blocks::SAND), "Ocean floor should be sand");
    }

    #[test]
    fn test_steep_mountain() {
        let rule = build_overworld_surface_rule(12345);
        let mut ctx = SurfaceContext::default();
        ctx.biome = Biome::WindsweptHills;
        ctx.block_y = 100;
        ctx.stone_depth_above = 0;
        ctx.surface_depth = 3;
        ctx.water_height = i32::MIN;
        ctx.steep = true;
        ctx.min_surface_level = 95; // Above preliminary surface

        let result = rule.try_apply(&ctx);
        // Steep mountain should be stone
        assert_eq!(result, Some(*blocks::STONE), "Steep mountain should be stone");
    }

    #[test]
    fn test_cave_floor_no_grass() {
        // This is the critical test - cave floors should NOT get grass
        let rule = build_overworld_surface_rule(12345);
        let mut ctx = SurfaceContext::default();
        ctx.biome = Biome::Plains;
        ctx.block_y = 30; // Deep underground
        ctx.stone_depth_above = 0; // First solid block after cave air
        ctx.surface_depth = 3;
        ctx.water_height = i32::MIN;
        ctx.min_surface_level = 60; // Surface is at Y=60, we're below it

        let result = rule.try_apply(&ctx);
        // Cave floor should NOT get grass - AbovePreliminarySurface should block it
        assert_eq!(result, None, "Cave floor should not get grass");
    }
}
