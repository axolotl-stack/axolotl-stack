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

use unastar_rules::enchanting::{EnchantmentInstance, add_enchantment};
use unastar_rules::ids::Enchantment;

use crate::command::args::FromArg;
use crate::command::prelude::*;
use crate::entity::components::{HeldSlot, MainInventory};
use crate::item::enchant::{enchantment_list, with_enchantments};
use crate::item::enchanting::item_enchanting;

/// The captured `Enchant` command enum spells enchantments by their bare
/// names in id order, which is [`Enchantment::NAMES`].
impl ArgEnum for Enchantment {
    const ENUM: &'static str = "Enchant";
    const SPELLINGS: &'static [&'static str] = &Enchantment::NAMES;

    fn from_value(value: &str) -> Option<Self> {
        Enchantment::ALL.into_iter().find(|e| e.name() == value)
    }
}

impl FromArg for Enchantment {
    fn accepts(kind: &unastar_data::commands::ParamKind) -> bool {
        matches!(kind, unastar_data::commands::ParamKind::Enum(e) if e == <Self as ArgEnum>::ENUM)
    }

    fn from_arg(value: &crate::command::parser::ArgValue) -> Option<Self> {
        match value {
            crate::command::parser::ArgValue::Enum(v) => <Self as ArgEnum>::from_value(v),
            _ => None,
        }
    }
}

overloads! {
    pub enum EnchantArgs for "enchant" {
        0 => ByName { player: Target, enchantment_name: Enchantment = "enchantmentName", level: Option<i32> },
        1 => ById { player: Target, enchantment_id: i32 = "enchantmentId", level: Option<i32> },
    }
}

pub fn run(ctx: &mut CommandContext<'_>, args: EnchantArgs) -> CommandResult {
    let (player, enchantment, level) = match args {
        EnchantArgs::ByName {
            player,
            enchantment_name,
            level,
        } => (player, enchantment_name, level),
        EnchantArgs::ById {
            player,
            enchantment_id,
            level,
        } => match i16::try_from(enchantment_id)
            .ok()
            .and_then(|id| Enchantment::try_from(id).ok())
        {
            Some(enchantment) => (player, enchantment, level),
            None => return failure!("commands.enchant.notFound", enchantment_id),
        },
    };
    let max = enchantment.max_level();
    let level = level.unwrap_or(1);
    let name = format!("%{}", enchantment.lang_key());
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
        enchantment,
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
    let mut existing = enchantment_list(&stack);
    if !add_enchantment(&mut existing, slots, enchantment) {
        return Err("commands.enchant.cantEnchant");
    }
    let enchanted = with_enchantments(&stack, existing);
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
    use crate::item::enchant::enchantment_list;
    use jolyne::valentine::McpePacketData;

    fn held(h: &mut Harness, player: Entity) -> crate::item::ItemStack {
        h.world().get::<MainInventory>(player).unwrap().0.slots()[0].clone()
    }

    /// Ids by name, as the enchantment list of an item.
    fn ids(stack: &crate::item::ItemStack) -> Vec<(i16, u8)> {
        enchantment_list(stack)
            .into_iter()
            .map(|e| (e.enchantment.id(), e.level))
            .collect()
    }

    #[test]
    fn enchant_covers_the_captured_enum_in_id_order() {
        assert_covers::<Enchantment>();
        let captured = unastar_data::commands::vanilla()
            .enum_named("Enchant")
            .unwrap();
        let names: Vec<&str> = captured.values.iter().map(String::as_str).collect();
        assert_eq!(<Enchantment as ArgEnum>::SPELLINGS, names.as_slice());
        for (id, name) in names.iter().enumerate() {
            let enchantment = <Enchantment as ArgEnum>::from_value(name).unwrap();
            assert_eq!(usize::try_from(enchantment.id()).unwrap(), id);
        }
        assert_eq!(Enchantment::Sharpness.id(), 9);
        assert_eq!(
            <Enchantment as ArgEnum>::from_value("minecraft:sharpness"),
            None
        );
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
        assert_eq!(ids(&held(&mut h, op)), [(9, 2)]);
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
        assert_eq!(ids(&held(&mut h, op)), [(9, 2), (17, 3)]);
        assert_eq!(
            h.run(op, "/enchant @s 42"),
            CommandOutput::failure("commands.enchant.notFound", vec!["42".into()])
        );
    }
}

/// Replays of the committed BDS 1.26.51 command capture
/// (`crates/bds-extractor/captures/1.26.51/operator-commands`).
#[cfg(test)]
mod bds_capture {
    use crate::command::exec::evidence::Replay;

    #[test]
    fn enchant_matches_the_bds_capture() {
        Replay::new().assert_segment("enchant", &[]);
    }
}
