//! Entity/block facts supplied to the shared loot evaluator.
//! Evaluation and typed loot schemas live in the bevy-free `unastar-rules`
//! crate; this adapter alone knows about ItemStack and the block registry.

use crate::entity::mob::constants::{LEGACY_ITEM_NAMES, LEGACY_WOOL, WOOL_ITEMS};
use crate::item::ItemStack;
use crate::item::enchant::{Enchantment, enchantment_level};
use crate::registry::block::{BlockEntry, StateValue};
use unastar_rules::loot::LootStateValue;

#[derive(Debug, Clone, Copy, Default)]
pub struct LootContext<'a> {
    pub tool: Option<&'a ItemStack>,
    pub block: Option<(&'a BlockEntry, u32)>,
    pub killed_by_player: bool,
    pub looting: u8,
    /// None means this evaluation has no THIS actor.
    pub on_fire: Option<bool>,
    pub recipes: Option<&'a unastar_data::recipes::RecipeRegistry>,
    /// Palette colour of the THIS actor, when it has one.
    pub color_index: Option<u8>,
}

impl unastar_rules::loot::LootContext for LootContext<'_> {
    fn tool_item(&self) -> Option<&str> {
        self.tool.filter(|t| !t.is_empty()).map(|t| t.item_id())
    }

    fn enchantment_level(&self, id: i16) -> u8 {
        let enchantment = match id {
            8 => Enchantment::AquaAffinity,
            14 => Enchantment::Looting,
            15 => Enchantment::Efficiency,
            16 => Enchantment::SilkTouch,
            17 => Enchantment::Unbreaking,
            18 => Enchantment::Fortune,
            _ => return 0,
        };
        self.tool
            .map_or(0, |tool| enchantment_level(tool, enchantment))
    }

    fn block_state_matches(&self, property: &str, expected: &LootStateValue) -> bool {
        let Some((block, state)) = self.block else {
            return false;
        };
        match (block.state_value(state, property), expected) {
            (Some(StateValue::Int(v)), LootStateValue::Int(e)) => v == *e,
            (Some(StateValue::Byte(v)), LootStateValue::Int(e)) => i32::from(v) == *e,
            (Some(StateValue::Str(v)), LootStateValue::Str(e)) => v == e,
            _ => false,
        }
    }

    fn killed_by_player(&self) -> bool {
        self.killed_by_player
    }
    fn looting(&self) -> u8 {
        self.looting
    }

    fn on_fire(&self) -> Option<bool> {
        self.on_fire
    }

    fn furnace(&self) -> Option<&dyn unastar_rules::loot::FurnaceSmelt> {
        self.recipes
            .map(|recipes| recipes as &dyn unastar_rules::loot::FurnaceSmelt)
    }

    fn this_color_index(&self) -> Option<u8> {
        self.color_index
    }

    fn item_alias(&self, name: &str) -> Option<&str> {
        LEGACY_ITEM_NAMES
            .iter()
            .find(|(legacy, _)| legacy.eq_ignore_ascii_case(name))
            .map(|(_, modern)| *modern)
    }

    fn data_alias(&self, name: &str, data: i16) -> Option<&str> {
        if !LEGACY_WOOL.eq_ignore_ascii_case(name) {
            return None;
        }
        usize::try_from(data)
            .ok()
            .and_then(|colour| WOOL_ITEMS.get(colour))
            .copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::enchant::with_enchantments;
    use crate::registry::BlockRegistry;
    use rand::SeedableRng;
    use rand::rngs::StdRng;
    use unastar_rules::loot::{LootDrop, LootError, LootTable};

    fn roll(
        table: &LootTable,
        ctx: &LootContext<'_>,
        rng: &mut impl rand::Rng,
    ) -> Result<Vec<LootDrop>, LootError> {
        unastar_rules::loot::roll(unastar_data::loot::vanilla(), table, ctx, rng)
    }

    fn block_ctx<'a>(name: &str, tool: Option<&'a ItemStack>) -> LootContext<'a> {
        let block = BlockRegistry::vanilla().get_by_name(name).expect(name);
        LootContext {
            tool,
            block: Some((block, block.default_state_id)),
            ..LootContext::default()
        }
    }

    fn table(name: &str) -> &'static LootTable {
        unastar_data::loot::get(name).expect(name)
    }

    fn counts(name: &str, ctx: &LootContext<'_>, seeds: u64) -> Vec<Vec<LootDrop>> {
        (0..seeds)
            .map(|seed| roll(table(name), ctx, &mut StdRng::seed_from_u64(seed)).unwrap())
            .collect()
    }

    #[test]
    fn stone_drops_cobblestone_unless_silk_touched() {
        let pick = crate::item::test_support::stack("minecraft:iron_pickaxe", 1);
        let silk = with_enchantments(&pick, &[(Enchantment::SilkTouch, 1)]);
        let mut rng = StdRng::seed_from_u64(1);
        let plain = roll(
            table("blocks/stone"),
            &block_ctx("minecraft:stone", Some(&pick)),
            &mut rng,
        )
        .unwrap();
        assert_eq!(
            plain,
            vec![LootDrop {
                item_id: "minecraft:cobblestone".into(),
                count: 1,
                data: 0,
                nbt: None,
            }]
        );
        let silky = roll(
            table("blocks/stone"),
            &block_ctx("minecraft:stone", Some(&silk)),
            &mut rng,
        )
        .unwrap();
        assert_eq!(silky[0].item_id, "minecraft:stone");
    }

    #[test]
    fn fortune_multiplies_ore_drops_within_vanilla_bounds() {
        let pick = crate::item::test_support::stack("minecraft:iron_pickaxe", 1);
        let fortune = with_enchantments(&pick, &[(Enchantment::Fortune, 3)]);
        let plain = counts(
            "blocks/diamond_ore",
            &block_ctx("minecraft:diamond_ore", Some(&pick)),
            50,
        );
        assert!(plain.iter().all(|d| d
            == &vec![LootDrop {
                item_id: "minecraft:diamond".into(),
                count: 1,
                data: 0,
                nbt: None,
            }]));
        let lucky = counts(
            "blocks/diamond_ore",
            &block_ctx("minecraft:diamond_ore", Some(&fortune)),
            200,
        );
        let amounts: Vec<u32> = lucky.iter().map(|d| d[0].count).collect();
        assert!(amounts.iter().all(|c| (1..=4).contains(c)));
        assert!(amounts.contains(&4) && amounts.contains(&1));
        let lapis = counts(
            "blocks/lapis_ore",
            &block_ctx("minecraft:lapis_ore", Some(&pick)),
            100,
        );
        assert!(lapis.iter().all(|d| (4..=9).contains(&d[0].count)));
    }

    #[test]
    fn gravel_sometimes_drops_flint_and_leaves_need_shears() {
        let gravel = counts("blocks/gravel", &block_ctx("minecraft:gravel", None), 300);
        let flint = gravel
            .iter()
            .filter(|d| d[0].item_id == "minecraft:flint")
            .count();
        assert!(
            (10..=60).contains(&flint),
            "about 10% flint, got {flint}/300"
        );
        let shears = crate::item::test_support::stack("minecraft:shears", 1);
        let sheared = roll(
            table("blocks/oak_leaves"),
            &block_ctx("minecraft:oak_leaves", Some(&shears)),
            &mut StdRng::seed_from_u64(2),
        )
        .unwrap();
        assert_eq!(
            sheared,
            vec![LootDrop {
                item_id: "minecraft:oak_leaves".into(),
                count: 1,
                data: 0,
                nbt: None,
            }]
        );
        let by_hand = counts(
            "blocks/oak_leaves",
            &block_ctx("minecraft:oak_leaves", None),
            400,
        );
        assert!(
            by_hand
                .iter()
                .flatten()
                .all(|d| d.item_id != "minecraft:oak_leaves")
        );
        assert!(
            by_hand
                .iter()
                .flatten()
                .any(|d| d.item_id == "minecraft:oak_sapling")
        );
    }

    #[test]
    fn crops_drop_produce_only_when_mature() {
        let block = BlockRegistry::vanilla()
            .get_by_name("minecraft:wheat")
            .unwrap();
        let young = LootContext {
            block: Some((block, block.default_state_id)),
            ..LootContext::default()
        };
        let mature_state = block
            .with_state_value(block.default_state_id, "growth", StateValue::Int(7))
            .unwrap();
        let mature = LootContext {
            block: Some((block, mature_state)),
            ..LootContext::default()
        };
        let mut rng = StdRng::seed_from_u64(5);
        assert_eq!(
            roll(table("blocks/wheat"), &young, &mut rng).unwrap(),
            vec![LootDrop {
                item_id: "minecraft:wheat_seeds".into(),
                count: 1,
                data: 0,
                nbt: None,
            }]
        );
        let ripe = roll(table("blocks/wheat"), &mature, &mut rng).unwrap();
        assert_eq!(
            ripe[0],
            LootDrop {
                item_id: "minecraft:wheat".into(),
                count: 1,
                data: 0,
                nbt: None,
            }
        );
        assert!(ripe[1].item_id == "minecraft:wheat_seeds" && (1..=4).contains(&ripe[1].count));
    }

    /// Every block table must evaluate without unsupported features, with and
    /// without tools and enchantments. Vanilla also ships `blocks/*` tables
    /// that no block shares a name with (e.g. `shelf_mushroom_growth_0`,
    /// referenced from block JSON); those roll without a block.
    #[test]
    fn block_tables_only_use_supported_features() {
        let pick = crate::item::test_support::stack("minecraft:diamond_pickaxe", 1);
        let silk = with_enchantments(&pick, &[(Enchantment::SilkTouch, 1)]);
        let fortune = with_enchantments(&pick, &[(Enchantment::Fortune, 3)]);
        let shears = crate::item::test_support::stack("minecraft:shears", 1);
        let registry = BlockRegistry::vanilla();
        let mut items = crate::registry::ItemRegistry::new();
        items.load_vanilla();
        for table in unastar_data::loot::vanilla()
            .iter()
            .filter(|t| t.name.starts_with("blocks/"))
        {
            let states: Vec<_> = registry
                .get_by_name(&format!("minecraft:{}", &table.name["blocks/".len()..]))
                .map_or_else(
                    || vec![None],
                    |block| {
                        (block.min_state_id..=block.max_state_id)
                            .map(|state| Some((block, state)))
                            .collect()
                    },
                );
            for tool in [
                None,
                Some(&pick),
                Some(&silk),
                Some(&fortune),
                Some(&shears),
            ] {
                for (seed, &block) in states.iter().enumerate() {
                    let ctx = LootContext {
                        tool,
                        block,
                        ..LootContext::default()
                    };
                    let drops = roll(table, &ctx, &mut StdRng::seed_from_u64(seed as u64))
                        .unwrap_or_else(|e| panic!("{}: {e}", table.name));
                    for drop in drops {
                        assert!(
                            items.get_by_name(&drop.item_id).is_some(),
                            "{} drops unknown item {}",
                            table.name,
                            drop.item_id
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn vanilla_entity_tables_roll_with_looting() {
        let ctx = LootContext {
            killed_by_player: true,
            looting: 3,
            ..LootContext::default()
        };
        let rolls: Vec<Vec<LootDrop>> = (0..50)
            .map(|seed| {
                roll(
                    table("entities/spider"),
                    &ctx,
                    &mut StdRng::seed_from_u64(seed),
                )
                .unwrap()
            })
            .collect();
        let strings: Vec<u32> = rolls
            .iter()
            .flatten()
            .filter(|d| d.item_id == "minecraft:string")
            .map(|d| d.count)
            .collect();
        assert!(
            strings.iter().all(|c| *c <= 5),
            "0-2 plus up to 3 from looting"
        );
        assert!(strings.iter().any(|c| *c > 2), "looting adds string");
    }

    #[test]
    fn unsupported_vanilla_features_are_errors_not_skipped() {
        let ctx = LootContext {
            killed_by_player: true,
            ..LootContext::default()
        };
        assert_eq!(
            roll(
                table("entities/zombie"),
                &ctx,
                &mut StdRng::seed_from_u64(3)
            ),
            Err(LootError::Unsupported("passenger_of_entity".into()))
        );
    }
}
