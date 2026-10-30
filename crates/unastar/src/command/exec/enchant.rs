//! `/enchant <player> <enchantmentName|enchantmentId> [level]`.
//!
//! Measured (BDS 1.26.51): the held item of each target gets the
//! enchantment (`commands.enchant.success` with the player's name); an empty
//! hand is `commands.enchant.noItem`, an enchantment the item cannot take
//! `commands.enchant.cantEnchant`, both with the player's name. A level above
//! the enchantment's maximum answers `commands.generic.num.tooBig [level,
//! max]` followed by `commands.enchant.invalidLevel [%<enchantment key>,
//! level]`. Which items take which enchantments, maximum levels and the
//! add-to-existing rule are the enchanting rules of `unastar_rules`.

use unastar_rules::enchanting::{EnchantmentInstance, add_enchantment, max_level};

use crate::command::prelude::*;
use crate::entity::components::{HeldSlot, MainInventory};
use crate::item::enchant::{enchantment_ids, with_enchantment_ids};
use crate::item::enchanting::item_enchanting;

arg_enum! {
    /// Enchantments by name, declared in Bedrock enchantment id order (the
    /// variant's ordinal is its id, as the captured enum lists them).
    pub enum Enchant in "Enchant" {
        Protection = "protection",
        FireProtection = "fire_protection",
        FeatherFalling = "feather_falling",
        BlastProtection = "blast_protection",
        ProjectileProtection = "projectile_protection",
        Thorns = "thorns",
        Respiration = "respiration",
        DepthStrider = "depth_strider",
        AquaAffinity = "aqua_affinity",
        Sharpness = "sharpness",
        Smite = "smite",
        BaneOfArthropods = "bane_of_arthropods",
        Knockback = "knockback",
        FireAspect = "fire_aspect",
        Looting = "looting",
        Efficiency = "efficiency",
        SilkTouch = "silk_touch",
        Unbreaking = "unbreaking",
        Fortune = "fortune",
        Power = "power",
        Punch = "punch",
        Flame = "flame",
        Infinity = "infinity",
        LuckOfTheSea = "luck_of_the_sea",
        Lure = "lure",
        FrostWalker = "frost_walker",
        Mending = "mending",
        Binding = "binding",
        Vanishing = "vanishing",
        Impaling = "impaling",
        Riptide = "riptide",
        Loyalty = "loyalty",
        Channeling = "channeling",
        Multishot = "multishot",
        Piercing = "piercing",
        QuickCharge = "quick_charge",
        SoulSpeed = "soul_speed",
        SwiftSneak = "swift_sneak",
        WindBurst = "wind_burst",
        Density = "density",
        Breach = "breach",
        Lunge = "lunge",
    }
}

/// Resource-pack name key of each enchantment, by id (`en_US.lang`).
const NAME_KEYS: [&str; 42] = [
    "enchantment.protect.all",
    "enchantment.protect.fire",
    "enchantment.protect.fall",
    "enchantment.protect.explosion",
    "enchantment.protect.projectile",
    "enchantment.thorns",
    "enchantment.oxygen",
    "enchantment.waterWalker",
    "enchantment.waterWorker",
    "enchantment.damage.all",
    "enchantment.damage.undead",
    "enchantment.damage.arthropods",
    "enchantment.knockback",
    "enchantment.fire",
    "enchantment.lootBonus",
    "enchantment.digging",
    "enchantment.untouching",
    "enchantment.durability",
    "enchantment.lootBonusDigger",
    "enchantment.arrowDamage",
    "enchantment.arrowKnockback",
    "enchantment.arrowFire",
    "enchantment.arrowInfinite",
    "enchantment.lootBonusFishing",
    "enchantment.fishingSpeed",
    "enchantment.frostwalker",
    "enchantment.mending",
    "enchantment.curse.binding",
    "enchantment.curse.vanishing",
    "enchantment.tridentImpaling",
    "enchantment.tridentRiptide",
    "enchantment.tridentLoyalty",
    "enchantment.tridentChanneling",
    "enchantment.crossbowMultishot",
    "enchantment.crossbowPiercing",
    "enchantment.crossbowQuickCharge",
    "enchantment.soul_speed",
    "enchantment.swift_sneak",
    "enchantment.heavy_weapon.windburst",
    "enchantment.heavy_weapon.density",
    "enchantment.heavy_weapon.breach",
    "enchantment.lunge",
];

overloads! {
    pub enum EnchantArgs for "enchant" {
        0 => ByName { player: Target, enchantment_name: Enchant = "enchantmentName", level: Option<i32> },
        1 => ById { player: Target, enchantment_id: i32 = "enchantmentId", level: Option<i32> },
    }
}

pub fn run(ctx: &mut CommandContext<'_>, args: EnchantArgs) -> CommandResult {
    let (player, id, level) = match args {
        EnchantArgs::ByName {
            player,
            enchantment_name,
            level,
        } => (player, enchantment_name as u8, level),
        EnchantArgs::ById {
            player,
            enchantment_id,
            level,
        } => match u8::try_from(enchantment_id)
            .ok()
            .filter(|id| max_level(*id).is_some())
        {
            Some(id) => (player, id, level),
            None => return failure!("commands.enchant.notFound", enchantment_id),
        },
    };
    let max = max_level(id).expect("enchantment ids come from the enchantment table");
    let level = level.unwrap_or(1);
    let name = format!("%{}", NAME_KEYS[usize::from(id)]);
    if level > i32::from(max) {
        let mut output = CommandOutput::failure(
            "commands.generic.num.tooBig",
            vec![level.to_string(), max.to_string()],
        );
        output.add_failure(
            "commands.enchant.invalidLevel",
            vec![name, level.to_string()],
        );
        return Err(output);
    }
    // [INFERENCE] the lower bound mirrors the measured upper bound.
    if level < 1 {
        let mut output = CommandOutput::failure(
            "commands.generic.num.tooSmall",
            vec![level.to_string(), "1".to_owned()],
        );
        output.add_failure(
            "commands.enchant.invalidLevel",
            vec![name, level.to_string()],
        );
        return Err(output);
    }
    let enchantment = EnchantmentInstance {
        id,
        level: level as u8,
    };

    let targets = ctx.players(Some(&player))?;
    let mut output = CommandOutput::default();
    for target in targets {
        let name = ctx.name(target);
        match enchant_held(ctx.world, target, enchantment) {
            Ok(()) => {
                crate::server::game::send_inventory_contents(ctx.world, target);
                output.add_success("commands.enchant.success", vec![name]);
            }
            Err(key) => output.add_failure(key, vec![name]),
        }
    }
    if output.is_success() {
        Ok(output)
    } else {
        Err(output)
    }
}

/// Add `enchantment` to the item `player` holds; `Err` is the failure key.
fn enchant_held(
    world: &mut World,
    player: Entity,
    enchantment: EnchantmentInstance,
) -> Result<(), &'static str> {
    let held = world.get::<HeldSlot>(player).map_or(0, |slot| slot.0);
    let Some(stack) = world
        .get::<MainInventory>(player)
        .and_then(|main| main.hotbar(held))
        .filter(|stack| !stack.is_empty())
        .cloned()
    else {
        return Err("commands.enchant.noItem");
    };
    let slots = stack
        .item()
        .and_then(item_enchanting)
        .map_or(0, |facts| facts.slots);
    let mut existing: Vec<EnchantmentInstance> = enchantment_ids(&stack)
        .into_iter()
        .filter_map(|(id, level)| {
            Some(EnchantmentInstance {
                id: u8::try_from(id).ok()?,
                level,
            })
        })
        .collect();
    if !add_enchantment(&mut existing, slots, enchantment) {
        return Err("commands.enchant.cantEnchant");
    }
    let enchanted =
        with_enchantment_ids(&stack, existing.iter().map(|e| (i16::from(e.id), e.level)));
    let mut main = world
        .get_mut::<MainInventory>(player)
        .expect("the held item came from the main inventory");
    *main.hotbar_mut(held).expect("held slot is a hotbar slot") = enchanted;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::args::assert_covers;
    use crate::command::exec::test_support::Harness;
    use crate::item::enchant::enchantment_ids;
    use jolyne::valentine::McpePacketData;

    fn held(h: &mut Harness, player: Entity) -> crate::item::ItemStack {
        h.world().get::<MainInventory>(player).unwrap().0.slots()[0].clone()
    }

    #[test]
    fn enchant_covers_the_captured_enum_in_id_order() {
        assert_covers::<Enchant>();
        let captured = unastar_data::commands::vanilla()
            .enum_named("Enchant")
            .unwrap();
        let ids: Vec<&str> = captured.values.iter().map(String::as_str).collect();
        assert_eq!(<Enchant as ArgEnum>::SPELLINGS, ids.as_slice());
        assert_eq!(Enchant::Sharpness as u8, 9);
        assert_eq!(NAME_KEYS.len(), ids.len());
    }

    #[test]
    fn enchanting_the_held_item_matches_bds() {
        let mut h = Harness::new();
        let op = h.player("BDSExtractor", true);
        assert_eq!(
            h.run(op, "/enchant @s sharpness 2"),
            CommandOutput::failure("commands.enchant.noItem", vec!["BDSExtractor".into()])
        );

        h.run(op, "/give @s diamond_sword");
        h.sent(op);
        assert_eq!(
            h.run(op, "/enchant @s sharpness 2"),
            CommandOutput::success("commands.enchant.success", vec!["BDSExtractor".into()])
        );
        assert_eq!(enchantment_ids(&held(&mut h, op)), [(9, 2)]);
        assert!(
            h.sent(op)
                .iter()
                .any(|p| matches!(p, McpePacketData::InventoryContentPacket(_))),
            "the client gets the enchanted item"
        );

        let mut too_big =
            CommandOutput::failure("commands.generic.num.tooBig", vec!["9".into(), "5".into()]);
        too_big.add_failure(
            "commands.enchant.invalidLevel",
            vec!["%enchantment.damage.all".into(), "9".into()],
        );
        assert_eq!(h.run(op, "/enchant @s sharpness 9"), too_big);

        assert_eq!(
            h.run(op, "/enchant @s protection 1"),
            CommandOutput::failure("commands.enchant.cantEnchant", vec!["BDSExtractor".into()])
        );
        // Smite shares sharpness's exclusive group.
        assert_eq!(
            h.run(op, "/enchant @s smite 1"),
            CommandOutput::failure("commands.enchant.cantEnchant", vec!["BDSExtractor".into()])
        );
        // By id: 17 is unbreaking.
        assert!(h.run(op, "/enchant @s 17 3").is_success());
        assert_eq!(enchantment_ids(&held(&mut h, op)), [(9, 2), (17, 3)]);
        assert_eq!(
            h.run(op, "/enchant @s 42"),
            CommandOutput::failure("commands.enchant.notFound", vec!["42".into()])
        );
    }
}
