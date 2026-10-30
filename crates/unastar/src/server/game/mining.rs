//! Mining rules: harvestability, destroy speed and break time.
//!
//! The vanilla break-time formula (the same one dragonfly implements in
//! `block.BreakDuration` / `player.breakTime`; BDS sends no break times, so
//! the timings below are to be confirmed by the planned BDS mining sweep):
//!
//! ```text
//! speed  = item destroy speed on the block (tier speed for matching tools,
//!          + efficiency^2 + 1 when that speed is above 1)
//! speed *= 1 + 0.2 * max(haste, conduit power)        (effect levels)
//! speed *= 0.3 ^ mining_fatigue
//! speed /= 5   when riding, or airborne and not flying
//! speed /= 5   when the head is under water without aqua affinity
//! progress per tick = speed / hardness / (harvestable ? 30 : 100)
//! ```
//!
//! Hardness 0 breaks in one tick; negative hardness never breaks.

use crate::entity::components::{EffectType, Effects};
use crate::item::ItemStack;
use crate::item::enchant::{Enchantment, enchantment_level};
use crate::registry::block::BlockEntry;
use crate::registry::item::{ToolKind, ToolTier};

/// Minimum tool tier a block needs for drops, from its
/// `minecraft:<tier>_tier_destructible` tag (harvest level as in
/// [`ToolTier::harvest_level`]).
pub fn required_harvest_level(block: &BlockEntry) -> u8 {
    if block.has_tag("minecraft:diamond_tier_destructible") {
        3
    } else if block.has_tag("minecraft:iron_tier_destructible") {
        2
    } else if block.has_tag("minecraft:stone_tier_destructible") {
        1
    } else {
        0
    }
}

fn tool_matches(kind: ToolKind, block: &BlockEntry) -> bool {
    block.has_tag(kind.destructible_tag())
}

/// Whether breaking `block` with `tool` yields drops: blocks that do not
/// require the correct tool always drop; the others need a matching tool
/// (native `minecraft:is_<tool>_item_destructible` tag) of a high enough tier
/// (`minecraft:<tier>_tier_destructible`).
pub fn can_harvest(block: &BlockEntry, tool: Option<&ItemStack>) -> bool {
    if !block.data.requires_correct_tool_for_drops {
        return true;
    }
    let Some(info) = tool
        .filter(|t| !t.is_empty())
        .and_then(|t| t.item().map(|item| item.tool_info()))
    else {
        return false;
    };
    let Some(kind) = info.kind else {
        return false;
    };
    if !tool_matches(kind, block) {
        return false;
    }
    let level = info.tier.map_or(0, ToolTier::harvest_level);
    level >= required_harvest_level(block)
}

/// Speed of `tool` on `block` before effects.
pub fn item_destroy_speed(tool: Option<&ItemStack>, block: &BlockEntry) -> f32 {
    let Some(tool) = tool.filter(|t| !t.is_empty()) else {
        return 1.0;
    };
    let Some(info) = tool.item().map(|item| item.tool_info()) else {
        return 1.0;
    };
    let id = block.string_id.as_str();
    let base = match info.kind {
        // Swords: cobweb 15, plant-like blocks 1.5.
        Some(ToolKind::Sword) => {
            if id == "minecraft:web" {
                15.0
            } else if block.data.material == 7
                || block.data.material == 8
                || matches!(
                    id,
                    "minecraft:pumpkin"
                        | "minecraft:melon_block"
                        | "minecraft:vine"
                        | "minecraft:glow_lichen"
                        | "minecraft:cocoa"
                        | "minecraft:moss_carpet"
                )
            {
                1.5
            } else {
                1.0
            }
        }
        // Shears: cobweb and leaves 15, wool 5, vines and glow lichen 2.
        Some(ToolKind::Shears) => {
            if id == "minecraft:web" || block.data.material == 7 {
                15.0
            } else if id.ends_with("_wool") {
                5.0
            } else if matches!(id, "minecraft:vine" | "minecraft:glow_lichen") {
                2.0
            } else {
                1.0
            }
        }
        Some(kind) if tool_matches(kind, block) => info.tier.map_or(1.0, ToolTier::speed),
        _ => 1.0,
    };
    let efficiency = enchantment_level(tool, Enchantment::Efficiency);
    if base > 1.0 && efficiency > 0 {
        base + f32::from(efficiency) * f32::from(efficiency) + 1.0
    } else {
        base
    }
}

/// Player state that changes the break time.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MiningConditions {
    pub on_ground: bool,
    pub flying: bool,
    pub riding: bool,
    /// Head under water.
    pub underwater: bool,
    /// Haste (or conduit power) effect level, 0 when absent.
    pub haste: u8,
    /// Mining fatigue effect level, 0 when absent.
    pub mining_fatigue: u8,
}

impl MiningConditions {
    /// Effect levels from an `Effects` component (`ActiveEffect::level` is
    /// the 0-based amplifier).
    pub fn with_effects(mut self, effects: Option<&Effects>) -> Self {
        let level = |effect| {
            effects
                .and_then(|e| e.get(effect))
                .map_or(0, |e| e.level.saturating_add(1))
        };
        self.haste = level(EffectType::HASTE);
        self.mining_fatigue = level(EffectType::MINING_FATIGUE);
        self
    }
}

/// Fraction of the block destroyed per tick.
pub fn destroy_progress(
    block: &BlockEntry,
    tool: Option<&ItemStack>,
    conditions: MiningConditions,
) -> f32 {
    let hardness = block.hardness;
    if hardness < 0.0 {
        return 0.0;
    }
    if hardness == 0.0 {
        return 1.0;
    }
    let mut speed = item_destroy_speed(tool, block);
    if conditions.haste > 0 {
        speed *= f32::from(conditions.haste) * 0.2 + 1.0;
    }
    if conditions.mining_fatigue > 0 {
        speed = (f64::from(speed) * 0.3f64.powi(i32::from(conditions.mining_fatigue))) as f32;
    }
    if conditions.riding || (!conditions.on_ground && !conditions.flying) {
        speed /= 5.0;
    }
    let aqua_affinity = tool.is_some_and(|t| enchantment_level(t, Enchantment::AquaAffinity) > 0);
    if conditions.underwater && !aqua_affinity {
        speed /= 5.0;
    }
    let divisor = if can_harvest(block, tool) {
        30.0
    } else {
        100.0
    };
    speed / hardness / divisor
}

/// Ticks needed to break `block` (`u32::MAX` for unbreakable blocks).
pub fn break_ticks(
    block: &BlockEntry,
    tool: Option<&ItemStack>,
    conditions: MiningConditions,
) -> u32 {
    let progress = destroy_progress(block, tool, conditions);
    if progress <= 0.0 {
        return u32::MAX;
    }
    if progress >= 1.0 {
        return 1;
    }
    (1.0 / progress).ceil() as u32
}

/// Durability lost by `tool` when it breaks `block`: 1 for tools on blocks
/// with hardness > 0, 2 for swords, 0 for hardness-0 blocks. Shears only wear
/// on blocks they are made for.
pub fn mining_wear(block: &BlockEntry, tool: &ItemStack) -> i32 {
    let Some(info) = tool
        .item()
        .filter(|item| super::durability::max_durability(item) > 0)
        .map(|item| item.tool_info())
    else {
        return 0;
    };
    match info.kind {
        Some(ToolKind::Shears) => i32::from(
            block.string_id == "minecraft:web"
                || block.data.material == 7
                || block.has_tag(ToolKind::Shears.destructible_tag())
                || matches!(
                    block.string_id.as_str(),
                    "minecraft:vine" | "minecraft:glow_lichen"
                ),
        ),
        Some(_) if block.hardness == 0.0 => 0,
        Some(ToolKind::Sword) => 2,
        Some(_) => 1,
        None => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::enchant::with_enchantments;
    use crate::registry::BlockRegistry;

    fn block(name: &str) -> &'static BlockEntry {
        BlockRegistry::vanilla().get_by_name(name).expect(name)
    }

    fn tool(name: &str) -> ItemStack {
        crate::item::test_support::stack(name, 1)
    }

    const GROUND: MiningConditions = MiningConditions {
        on_ground: true,
        flying: false,
        riding: false,
        underwater: false,
        haste: 0,
        mining_fatigue: 0,
    };

    #[test]
    fn vanilla_break_times_by_hand_and_tool() {
        // Stone (hardness 1.5): hand 7.5 s, wooden pickaxe 1.15 s, iron 0.4 s.
        let stone = block("minecraft:stone");
        assert_eq!(break_ticks(stone, None, GROUND), 150);
        assert_eq!(
            break_ticks(stone, Some(&tool("minecraft:wooden_pickaxe")), GROUND),
            23
        );
        assert_eq!(
            break_ticks(stone, Some(&tool("minecraft:iron_pickaxe")), GROUND),
            8
        );
        assert_eq!(
            break_ticks(stone, Some(&tool("minecraft:golden_pickaxe")), GROUND),
            4
        );
        // Dirt (0.5) by hand: 0.75 s; with a shovel 0.25 s (wooden 2x).
        let dirt = block("minecraft:dirt");
        assert_eq!(break_ticks(dirt, None, GROUND), 15);
        assert_eq!(
            break_ticks(dirt, Some(&tool("minecraft:wooden_shovel")), GROUND),
            8
        );
        // Obsidian (Bedrock hardness 35) with a diamond pickaxe: 6.6 s.
        assert_eq!(
            break_ticks(
                block("minecraft:obsidian"),
                Some(&tool("minecraft:diamond_pickaxe")),
                GROUND
            ),
            132
        );
        assert_eq!(
            break_ticks(block("minecraft:bedrock"), None, GROUND),
            u32::MAX
        );
        assert_eq!(break_ticks(block("minecraft:short_grass"), None, GROUND), 1);
    }

    #[test]
    fn efficiency_haste_fatigue_water_and_air_modify_speed() {
        let stone = block("minecraft:stone");
        let pick = with_enchantments(
            &tool("minecraft:iron_pickaxe"),
            &[(Enchantment::Efficiency, 5)],
        );
        // (6 + 25 + 1) / 1.5 / 30 = 0.711 -> 2 ticks
        assert_eq!(break_ticks(stone, Some(&pick), GROUND), 2);
        let haste = MiningConditions { haste: 2, ..GROUND };
        // 6 * 1.4 / 45 = 0.1867 -> 6 ticks
        assert_eq!(
            break_ticks(stone, Some(&tool("minecraft:iron_pickaxe")), haste),
            6
        );
        let fatigue = MiningConditions {
            mining_fatigue: 1,
            ..GROUND
        };
        assert_eq!(
            break_ticks(stone, Some(&tool("minecraft:iron_pickaxe")), fatigue),
            25
        );
        let airborne = MiningConditions {
            on_ground: false,
            ..GROUND
        };
        assert_eq!(
            break_ticks(stone, Some(&tool("minecraft:iron_pickaxe")), airborne),
            38
        );
        let flying = MiningConditions {
            on_ground: false,
            flying: true,
            ..GROUND
        };
        assert_eq!(
            break_ticks(stone, Some(&tool("minecraft:iron_pickaxe")), flying),
            8
        );
        let wet = MiningConditions {
            underwater: true,
            ..GROUND
        };
        assert_eq!(
            break_ticks(stone, Some(&tool("minecraft:iron_pickaxe")), wet),
            38
        );
        let aqua = with_enchantments(
            &tool("minecraft:iron_pickaxe"),
            &[(Enchantment::AquaAffinity, 1)],
        );
        assert_eq!(break_ticks(stone, Some(&aqua), wet), 8);
    }

    #[test]
    fn harvestability_follows_native_tool_tags_and_tiers() {
        let stone = block("minecraft:stone");
        assert!(!can_harvest(stone, None));
        assert!(!can_harvest(stone, Some(&tool("minecraft:iron_axe"))));
        assert!(can_harvest(stone, Some(&tool("minecraft:wooden_pickaxe"))));
        let diamond_ore = block("minecraft:diamond_ore");
        assert!(!can_harvest(
            diamond_ore,
            Some(&tool("minecraft:stone_pickaxe"))
        ));
        assert!(!can_harvest(
            diamond_ore,
            Some(&tool("minecraft:golden_pickaxe"))
        ));
        assert!(can_harvest(
            diamond_ore,
            Some(&tool("minecraft:iron_pickaxe"))
        ));
        let obsidian = block("minecraft:obsidian");
        assert!(!can_harvest(
            obsidian,
            Some(&tool("minecraft:iron_pickaxe"))
        ));
        assert!(can_harvest(
            obsidian,
            Some(&tool("minecraft:diamond_pickaxe"))
        ));
        assert!(can_harvest(block("minecraft:dirt"), None));
        assert!(can_harvest(
            block("minecraft:iron_ore"),
            Some(&tool("minecraft:stone_pickaxe"))
        ));
        assert!(can_harvest(
            block("minecraft:iron_ore"),
            Some(&tool("minecraft:copper_pickaxe"))
        ));
    }

    #[test]
    fn tool_wear_per_block() {
        let stone = block("minecraft:stone");
        assert_eq!(mining_wear(stone, &tool("minecraft:iron_pickaxe")), 1);
        assert_eq!(mining_wear(stone, &tool("minecraft:iron_sword")), 2);
        assert_eq!(
            mining_wear(
                block("minecraft:short_grass"),
                &tool("minecraft:iron_pickaxe")
            ),
            0
        );
        assert_eq!(mining_wear(stone, &tool("minecraft:shears")), 0);
        assert_eq!(
            mining_wear(block("minecraft:oak_leaves"), &tool("minecraft:shears")),
            1
        );
        assert_eq!(mining_wear(stone, &tool("minecraft:dirt")), 0);
    }
}
