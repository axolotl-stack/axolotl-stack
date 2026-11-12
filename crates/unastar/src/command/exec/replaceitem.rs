//! `/replaceitem entity <target> <slotType> <slotId> [destroy|keep] <item>
//! [amount] [data] [components]` and `/replaceitem block <position>
//! slot.container <slotId> [destroy|keep] <item> …`.
//!
//! Measured on BDS 1.26.51:
//! - an entity answers `commands.replaceitem.success.entity [slot, id, name,
//!   amount, item]` and a container `commands.replaceitem.success [slot.container,
//!   id, amount, item]`; `keep` over an occupied slot answers
//!   `commands.replaceitem.failed [slot, id, amount, item]` (`none` as the
//!   slot of a container);
//! - slot ids are bounded for the hotbar (0-8), the inventory (0-26) and a
//!   container (its size), else `commands.replaceitem.badSlotNumber [id, 0,
//!   max]`; armour and hand slots take any id; the amount is 1 to the item's
//!   stack size (`commands.generic.num.tooSmall|tooBig`);
//! - a player's head and offhand take any item, chest, legs and feet only
//!   matching armour, yet a refused item still answers success (logged in
//!   `docs/quirks/bds.md`); `slot.armor.body` and the ender chest answer
//!   success too;
//! - a block that holds no container answers
//!   `commands.replaceitem.noContainer ["Pos(x,y,z)"]`.
//!
//! [INFERENCE] Mobs take any item in their armour and hand slots and refuse
//! the player-only slots; the other player-only slots (`slot.saddle`,
//! `slot.armor`, `slot.chest`, `slot.equippable`) answer like `slot.armor.body`.

use super::give::{given_stack, with_components};
use crate::command::names;
use crate::command::prelude::*;
use crate::entity::components::{
    ArmourInventory, EnderChestInventory, HeldSlot, MainInventory, OffhandSlot, Player,
};
use crate::entity::mob::MobKind;
use crate::entity::mob::gear::{GearSlot, MobEquipment};
use crate::item::ItemStack;

arg_enum! {
    /// Captured `EntityEquipmentSlot`.
    pub enum EntitySlot in "EntityEquipmentSlot" {
        Mainhand = "slot.weapon.mainhand",
        Offhand = "slot.weapon.offhand",
        Head = "slot.armor.head",
        Chest = "slot.armor.chest",
        Legs = "slot.armor.legs",
        Feet = "slot.armor.feet",
        Body = "slot.armor.body",
        Hotbar = "slot.hotbar",
        Inventory = "slot.inventory",
        EnderChest = "slot.enderchest",
        Saddle = "slot.saddle",
        Armor = "slot.armor",
        Chest2 = "slot.chest",
        Equippable = "slot.equippable",
    }
}

arg_enum! {
    /// Captured `ReplaceMode`.
    pub enum ReplaceMode in "ReplaceMode" {
        Destroy = "destroy",
        Keep = "keep",
    }
}

overloads! {
    pub enum ReplaceItemArgs for "replaceitem" {
        0 => Block {
            block: EnumValue,
            position: BlockPosition,
            slot_type: EnumValue = "slotType",
            slot_id: i32 = "slotId",
            item: EnumValue = "itemName",
            amount: Option<i32>,
            data: Option<i32>,
            components: Option<serde_json::Value>,
        },
        1 => Entity {
            entity: EnumValue,
            target: Target,
            slot_type: EntitySlot = "slotType",
            slot_id: i32 = "slotId",
            item: EnumValue = "itemName",
            amount: Option<i32>,
            data: Option<i32>,
            components: Option<serde_json::Value>,
        },
        2 => BlockMode {
            block: EnumValue,
            position: BlockPosition,
            slot_type: EnumValue = "slotType",
            slot_id: i32 = "slotId",
            mode: ReplaceMode = "oldItemHandling",
            item: EnumValue = "itemName",
            amount: Option<i32>,
            data: Option<i32>,
            components: Option<serde_json::Value>,
        },
        3 => EntityMode {
            entity: EnumValue,
            target: Target,
            slot_type: EntitySlot = "slotType",
            slot_id: i32 = "slotId",
            mode: ReplaceMode = "oldItemHandling",
            item: EnumValue = "itemName",
            amount: Option<i32>,
            data: Option<i32>,
            components: Option<serde_json::Value>,
        },
    }
}

/// The item a command names, with its amount checked.
struct Item {
    stack: ItemStack,
    amount: i32,
    name: String,
}

fn item(
    ctx: &CommandContext<'_>,
    name: &str,
    amount: Option<i32>,
    data: Option<i32>,
    components: Option<&serde_json::Value>,
) -> Result<Item, CommandOutput> {
    let stack = given_stack(ctx.world, name, data.unwrap_or(0))?;
    let stack = match components {
        Some(components) => with_components(ctx.world, stack, components)?,
        None => stack,
    };
    let amount = amount.unwrap_or(1);
    let max = stack.max_stack_size() as i32;
    if amount < 1 {
        return failure!("commands.generic.num.tooSmall", amount, 1);
    }
    if amount > max {
        return failure!("commands.generic.num.tooBig", amount, max);
    }
    Ok(Item {
        name: names::item_name(&stack),
        stack: stack.with_count(amount as u8),
        amount,
    })
}

pub fn run(ctx: &mut CommandContext<'_>, args: ReplaceItemArgs) -> CommandResult {
    use ReplaceItemArgs as R;
    match args {
        R::Block {
            position,
            slot_id,
            item: name,
            amount,
            data,
            components,
            ..
        } => {
            let item = item(ctx, &name, amount, data, components.as_ref())?;
            replace_block(ctx, position, slot_id, ReplaceMode::Destroy, item)
        }
        R::BlockMode {
            position,
            slot_id,
            mode,
            item: name,
            amount,
            data,
            components,
            ..
        } => {
            let item = item(ctx, &name, amount, data, components.as_ref())?;
            replace_block(ctx, position, slot_id, mode, item)
        }
        R::Entity {
            target,
            slot_type,
            slot_id,
            item: name,
            amount,
            data,
            components,
            ..
        } => {
            let item = item(ctx, &name, amount, data, components.as_ref())?;
            replace_entity(ctx, &target, slot_type, slot_id, ReplaceMode::Destroy, item)
        }
        R::EntityMode {
            target,
            slot_type,
            slot_id,
            mode,
            item: name,
            amount,
            data,
            components,
            ..
        } => {
            let item = item(ctx, &name, amount, data, components.as_ref())?;
            replace_entity(ctx, &target, slot_type, slot_id, mode, item)
        }
    }
}

fn replace_block(
    ctx: &mut CommandContext<'_>,
    position: BlockPosition,
    slot: i32,
    mode: ReplaceMode,
    item: Item,
) -> CommandResult {
    let pos = ctx.block_position(Some(position));
    let Some(slots) = crate::server::game::command_container_slots(ctx.world, pos) else {
        return failure!(
            "commands.replaceitem.noContainer",
            format!("Pos({},{},{})", pos.x, pos.y, pos.z)
        );
    };
    let max = slots.len() as i32 - 1;
    let Some(index) = usize::try_from(slot).ok().filter(|&i| i < slots.len()) else {
        return failure!("commands.replaceitem.badSlotNumber", slot, 0, max);
    };
    if mode == ReplaceMode::Keep && !slots[index].is_empty() {
        return failure!(
            "commands.replaceitem.failed",
            "none",
            slot,
            item.amount,
            item.name
        );
    }
    crate::server::game::command_set_container_slot(ctx.world, pos, index, item.stack);
    success!(
        "commands.replaceitem.success",
        "slot.container",
        slot,
        item.amount,
        item.name
    )
}

/// Where a slot lives on a player.
enum PlayerSlot {
    Main(usize),
    Armour(usize),
    Offhand,
    EnderChest(usize),
    /// A slot players do not have: nothing changes.
    None,
}

fn player_slot(
    world: &World,
    player: Entity,
    slot: EntitySlot,
    id: i32,
) -> Result<PlayerSlot, CommandOutput> {
    let bounded = |max: i32| {
        if (0..=max).contains(&id) {
            Ok(id as usize)
        } else {
            failure!("commands.replaceitem.badSlotNumber", id, 0, max)
        }
    };
    Ok(match slot {
        EntitySlot::Hotbar => PlayerSlot::Main(bounded(8)?),
        EntitySlot::Inventory => PlayerSlot::Main(9 + bounded(26)?),
        EntitySlot::EnderChest => PlayerSlot::EnderChest(bounded(26)?),
        EntitySlot::Mainhand => {
            PlayerSlot::Main(world.get::<HeldSlot>(player).map_or(0, |h| h.0 as usize))
        }
        EntitySlot::Offhand => PlayerSlot::Offhand,
        EntitySlot::Head => PlayerSlot::Armour(0),
        EntitySlot::Chest => PlayerSlot::Armour(1),
        EntitySlot::Legs => PlayerSlot::Armour(2),
        EntitySlot::Feet => PlayerSlot::Armour(3),
        EntitySlot::Body
        | EntitySlot::Saddle
        | EntitySlot::Armor
        | EntitySlot::Chest2
        | EntitySlot::Equippable => PlayerSlot::None,
    })
}

fn slot_name(slot: EntitySlot) -> &'static str {
    <EntitySlot as crate::command::ArgEnum>::SPELLINGS[slot as usize]
}

fn replace_entity(
    ctx: &mut CommandContext<'_>,
    target: &Target,
    slot: EntitySlot,
    id: i32,
    mode: ReplaceMode,
    item: Item,
) -> CommandResult {
    let targets = ctx.entities(Some(target))?;
    let slot_name = slot_name(slot);
    let mut output = CommandOutput::default();
    for target in targets {
        let name = ctx.name(target);
        let placed = if ctx.world.get::<Player>(target).is_some() {
            replace_player(ctx, target, slot, id, mode, &item)?
        } else if ctx.world.get::<MobKind>(target).is_some() {
            replace_mob(ctx.world, target, slot, mode, &item)
        } else {
            false
        };
        let params = vec![slot_name.to_owned(), id.to_string()];
        if placed {
            let mut params = params;
            params.extend([name, item.amount.to_string(), item.name.clone()]);
            output.add_success("commands.replaceitem.success.entity", params);
        } else {
            let mut params = params;
            params.extend([item.amount.to_string(), item.name.clone()]);
            output.add_failure("commands.replaceitem.failed", params);
        }
    }
    if output.success_count == 0 {
        return Err(output);
    }
    Ok(output)
}

/// Returns `false` when `keep` finds the slot occupied.
fn replace_player(
    ctx: &mut CommandContext<'_>,
    player: Entity,
    slot: EntitySlot,
    id: i32,
    mode: ReplaceMode,
    item: &Item,
) -> Result<bool, CommandOutput> {
    let slot = player_slot(ctx.world, player, slot, id)?;
    let current = match &slot {
        PlayerSlot::Main(i) => ctx
            .world
            .get::<MainInventory>(player)
            .and_then(|m| m.0.item(*i).cloned()),
        PlayerSlot::Armour(i) => ctx
            .world
            .get::<ArmourInventory>(player)
            .and_then(|a| a.slot(*i).cloned()),
        PlayerSlot::Offhand => ctx.world.get::<OffhandSlot>(player).map(|o| o.0.clone()),
        PlayerSlot::EnderChest(i) => ctx
            .world
            .get::<EnderChestInventory>(player)
            .and_then(|e| e.0.item(*i).cloned()),
        PlayerSlot::None => None,
    };
    if mode == ReplaceMode::Keep && current.is_some_and(|c| !c.is_empty()) {
        return Ok(false);
    }
    let stack = item.stack.clone();
    match slot {
        PlayerSlot::Main(i) => {
            if let Some(mut main) = ctx.world.get_mut::<MainInventory>(player) {
                let _ = main.0.set_item(i, stack);
            }
        }
        PlayerSlot::Armour(i) => {
            // Measured: head takes any item; chest, legs and feet refuse
            // anything but matching armour and still answer success.
            let accepted = i == 0 || crate::item::equipment::accepts_armour(&stack, i);
            if accepted && let Some(mut armour) = ctx.world.get_mut::<ArmourInventory>(player) {
                let _ = armour.inventory_mut().set_item(i, stack);
            }
        }
        PlayerSlot::Offhand => {
            if let Some(mut offhand) = ctx.world.get_mut::<OffhandSlot>(player) {
                offhand.0 = stack;
            }
        }
        PlayerSlot::EnderChest(i) => {
            if let Some(mut chest) = ctx.world.get_mut::<EnderChestInventory>(player) {
                let _ = chest.0.set_item(i, stack);
            }
        }
        PlayerSlot::None => {}
    }
    crate::server::game::send_inventory_contents(ctx.world, player);
    Ok(true)
}

/// Returns `false` for a slot mobs do not have or `keep` over an item.
fn replace_mob(
    world: &mut World,
    mob: Entity,
    slot: EntitySlot,
    mode: ReplaceMode,
    item: &Item,
) -> bool {
    let gear = match slot {
        EntitySlot::Mainhand => GearSlot::Mainhand,
        EntitySlot::Offhand => GearSlot::Offhand,
        EntitySlot::Head => GearSlot::Head,
        EntitySlot::Chest => GearSlot::Chest,
        EntitySlot::Legs => GearSlot::Legs,
        EntitySlot::Feet => GearSlot::Feet,
        _ => return false,
    };
    if world.get::<MobEquipment>(mob).is_none() {
        world.entity_mut(mob).insert(MobEquipment::default());
    }
    let mut equipment = world.get_mut::<MobEquipment>(mob).expect("inserted above");
    if mode == ReplaceMode::Keep && !equipment.get(gear).is_empty() {
        return false;
    }
    equipment.set(gear, item.stack.clone(), false);
    true
}

/// Replays of the committed BDS 1.26.51 command capture
/// (`crates/bds-extractor/captures/1.26.51/operator-commands`).
#[cfg(test)]
mod bds_capture {
    use crate::command::exec::evidence::Replay;

    #[test]
    fn replaceitem_matches_the_bds_capture() {
        Replay::new().assert_segment("replaceitem", &[]);
    }
}
