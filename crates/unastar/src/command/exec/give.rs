//! `/give <player> <itemName> [amount] [data] [components]`.
//!
//! Measured (BDS 1.26.51): the items fill matching stacks, then empty slots
//! of the main inventory (`/give @s stone 600` makes nine stacks of 64 and
//! one of 24); the answer is `commands.give.success [item name, amount,
//! player names]`, each target reads `commands.give.successRecipient [item
//! name, amount]`, gets its whole inventory resent and hears `pop` at its eye
//! position. Legacy spellings resolve through the item registry's measured
//! identities; `components` carries the vanilla item components
//! (`minecraft:can_place_on`, `minecraft:can_destroy`, `minecraft:item_lock`
//! as NBT byte 1 = lock_in_slot / 2 = lock_in_inventory,
//! `minecraft:keep_on_death` as NBT byte 1).
//!
//! Not in unastar: recipe unlocking (BDS follows with `UnlockedRecipes`),
//! and legacy data values of flattened families (`/give @s wool 3 14` is
//! red wool on BDS) for which unastar has no measured remapping.

use zuri_nbt::{NBTTag, tag};

use crate::command::names;
use crate::command::prelude::*;
use crate::entity::components::MainInventory;
use crate::entity::spawn_item_drop;
use crate::item::ItemStack;
use crate::item::enchant::{item_tag, with_block_lists, with_durability_damage, with_item_tag};
use crate::server::game::types::{BlockRegistryResource, ItemRegistryResource};

overloads! {
    pub enum GiveArgs for "give" {
        0 => Give {
            player: Target,
            item_name: EnumValue = "itemName",
            amount: Option<i32>,
            data: Option<i32>,
            components: Option<serde_json::Value>,
        },
    }
}

/// Largest `amount` accepted. [INFERENCE] BDS's item count bound (`i16`).
const MAX_AMOUNT: i32 = i16::MAX as i32;

pub fn run(ctx: &mut CommandContext<'_>, args: GiveArgs) -> CommandResult {
    let GiveArgs::Give {
        player,
        item_name,
        amount,
        data,
        components,
    } = args;
    let amount = amount.unwrap_or(1);
    // [INFERENCE] amount bounds answer the generic number messages.
    if amount < 1 {
        return failure!("commands.generic.num.tooSmall", amount, 1);
    }
    if amount > MAX_AMOUNT {
        return failure!("commands.generic.num.tooBig", amount, MAX_AMOUNT);
    }
    let stack = given_stack(ctx.world, &item_name, data.unwrap_or(0))?;
    let stack = match &components {
        Some(components) => with_components(ctx.world, stack, components)?,
        None => stack,
    };

    let targets = ctx.players(Some(&player))?;
    let name = names::item_name(&stack);
    for &target in &targets {
        give(ctx.world, target, &stack, amount as u32);
        crate::server::game::send_inventory_contents(ctx.world, target);
        ctx.notify(
            target,
            "commands.give.successRecipient",
            vec![name.clone(), amount.to_string()],
        );
        if let Some(eye) = ctx.eye(target) {
            crate::entity::mob::net::sound::play_level_sound(ctx.world, None, eye, "pop", -1);
        }
    }
    Ok(CommandOutput::success(
        "commands.give.success",
        vec![name, amount.to_string(), ctx.names(&targets)],
    )
    .with_count(targets.len()))
}

fn given_stack(world: &World, name: &str, data: i32) -> Result<ItemStack, CommandOutput> {
    let invalid = || CommandOutput::failure("commands.give.item.invalid", vec![name.to_owned()]);
    let items = &world.resource::<ItemRegistryResource>().0;
    let blocks = &world.resource::<BlockRegistryResource>().0;
    let spelled = if data == 0 {
        name.to_owned()
    } else {
        format!("{name}:{data}")
    };
    let identity = items.resolve_identity(&spelled).ok_or_else(invalid)?;
    let entry = items.get(identity.id).ok_or_else(invalid)?;
    let stack = items
        .stack_with_block(entry.identifier(), 1, blocks)
        .ok_or_else(invalid)?;
    let qualified = unastar_data::item_identity::qualify(name);
    let aliased = entry.identifier() != qualified;
    let aux = identity.data.unwrap_or(0);
    if data == 0 || aliased {
        // No data value, or a measured legacy alias consumed it.
        return Ok(stack.with_damage(aux));
    }
    if entry.tool_info().max_damage > 0 {
        return Ok(with_durability_damage(&stack, data));
    }
    if entry.stacked_by_data() {
        return Ok(stack.with_damage(aux));
    }
    Err(CommandOutput::failure(
        format!(
            "Legacy data value {data} of {} is not implemented in unastar yet",
            entry.identifier()
        ),
        vec![],
    ))
}

/// `stack` carrying the vanilla item components of `/give`'s JSON.
fn with_components(
    world: &World,
    stack: ItemStack,
    components: &serde_json::Value,
) -> Result<ItemStack, CommandOutput> {
    // [INFERENCE] malformed components answer `commands.give.tagError`
    // with the offending component.
    let tag_error =
        |what: &str| CommandOutput::failure("commands.give.tagError", vec![what.to_owned()]);
    let serde_json::Value::Object(components) = components else {
        return Err(tag_error(&components.to_string()));
    };
    let blocks = &world.resource::<BlockRegistryResource>().0;
    let block_list = |key: &str, value: &serde_json::Value| -> Result<Vec<String>, CommandOutput> {
        let names = value
            .get("blocks")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| tag_error(key))?;
        names
            .iter()
            .map(|name| {
                let name = name.as_str().ok_or_else(|| tag_error(key))?;
                let name = unastar_data::item_identity::qualify(name).into_owned();
                blocks
                    .get_by_name(&name)
                    .map(|_| name)
                    .ok_or_else(|| tag_error(key))
            })
            .collect()
    };
    let mut compound = item_tag(&stack).unwrap_or_default();
    let mut tagged = false;
    let mut can_place_on = Vec::new();
    let mut can_destroy = Vec::new();
    for (key, value) in components {
        match key.as_str() {
            "minecraft:can_place_on" => can_place_on = block_list(key, value)?,
            "minecraft:can_destroy" => can_destroy = block_list(key, value)?,
            "minecraft:item_lock" => {
                let mode = match value.get("mode").and_then(serde_json::Value::as_str) {
                    Some("lock_in_slot") => 1,
                    Some("lock_in_inventory") => 2,
                    _ => return Err(tag_error(key)),
                };
                compound
                    .0
                    .insert(key.clone(), NBTTag::Byte(tag::Byte(mode)));
                tagged = true;
            }
            "minecraft:keep_on_death" => {
                compound.0.insert(key.clone(), NBTTag::Byte(tag::Byte(1)));
                tagged = true;
            }
            // [INFERENCE] other keys are not item components; ignored.
            _ => {}
        }
    }
    let stack = if tagged {
        with_item_tag(&stack, compound)
    } else {
        stack
    };
    Ok(if can_place_on.is_empty() && can_destroy.is_empty() {
        stack
    } else {
        with_block_lists(&stack, &can_place_on, &can_destroy)
    })
}

/// Put `amount` of `stack` into `player`'s main inventory, matching stacks
/// first; what does not fit drops at the player as if it threw it.
fn give(world: &mut World, player: Entity, stack: &ItemStack, amount: u32) {
    let per_stack = u32::from(stack.max_stack_size().max(1));
    let mut overflow = Vec::new();
    if let Some(mut main) = world.get_mut::<MainInventory>(player) {
        let mut remaining = amount;
        while remaining > 0 {
            let count = remaining.min(per_stack);
            remaining -= count;
            let (_, leftover) = main.0.add_item(stack.clone().with_count(count as u8));
            if !leftover.is_empty() {
                overflow.push(leftover);
            }
        }
    }
    let Some(eye) = world
        .get::<crate::entity::components::Position>(player)
        .map(|p| p.0)
    else {
        return;
    };
    for leftover in overflow {
        spawn_item_drop(world, eye, leftover, Some(player));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::exec::test_support::Harness;
    use crate::command::output::translation_text;
    use crate::entity::components::{DroppedItem, ItemEntityStack};
    use crate::item::enchant::block_lists;
    use jolyne::valentine::McpePacketData;

    fn main_slots(h: &mut Harness, player: Entity) -> Vec<(String, u8)> {
        h.world()
            .get::<MainInventory>(player)
            .unwrap()
            .0
            .slots()
            .iter()
            .map(|s| (s.item_id().to_owned(), s.count))
            .collect()
    }

    fn slot(h: &mut Harness, player: Entity, index: usize) -> ItemStack {
        h.world().get::<MainInventory>(player).unwrap().0.slots()[index].clone()
    }

    /// Makes `player` a viewer of the harness chunk so level sounds reach it.
    fn view_chunk(h: &mut Harness, player: Entity) {
        let world = h.world();
        let chunk = world
            .resource::<crate::world::ecs::ChunkManager>()
            .get_by_coords(0, 0)
            .unwrap();
        let mut viewers = crate::world::ecs::ChunkViewers::default();
        viewers.insert(player);
        world.entity_mut(chunk).insert(viewers);
    }

    #[test]
    fn giving_wheat_matches_bds() {
        let mut h = Harness::new();
        let op = h.player("BDSExtractor", true);
        view_chunk(&mut h, op);
        assert_eq!(
            h.run(op, "/give @s wheat 64"),
            CommandOutput::success(
                "commands.give.success",
                vec!["Wheat".into(), "64".into(), "BDSExtractor".into()]
            )
        );
        assert_eq!(main_slots(&mut h, op)[0], ("minecraft:wheat".into(), 64));

        let sent = h.sent(op);
        let contents: Vec<_> = sent
            .iter()
            .filter_map(|p| match p {
                McpePacketData::InventoryContentPacket(c) => Some(c.slots.len()),
                _ => None,
            })
            .collect();
        assert_eq!(contents, [36, 5, 54, 1], "main, armour, UI, offhand");
        assert!(
            sent.iter()
                .any(|p| matches!(p, McpePacketData::PlayerHotbarPacket(_)))
        );
        let recipient = format!(
            "{:?}",
            translation_text(
                "commands.give.successRecipient",
                &["Wheat".into(), "64".into()]
            )
        );
        assert!(
            sent.iter().any(
                |p| matches!(p, McpePacketData::TextPacket(t) if format!("{t:?}") == recipient)
            )
        );
        let pop = sent
            .iter()
            .find_map(|p| match p {
                McpePacketData::LevelSoundEventPacket(s) if &*s.sound_event == "pop" => Some(s),
                _ => None,
            })
            .expect("pop at the player");
        assert_eq!(
            (pop.position.x, pop.position.y, pop.position.z),
            (3.5, 65.62, 3.5)
        );
        assert_eq!((pop.data, pop.actor_unique_id), (-1, -1));
        assert_eq!(&*pop.actor_identifier, "");
    }

    #[test]
    fn amounts_fill_stacks_then_empty_slots_and_drop_the_rest() {
        let mut h = Harness::new();
        let op = h.player("BDSExtractor", true);
        h.run(op, "/give @s wheat 64");
        h.run(op, "/give @s minecraft:diamond_sword");
        assert_eq!(
            h.run(op, "/give @s stone 600"),
            CommandOutput::success(
                "commands.give.success",
                vec!["Stone".into(), "600".into(), "BDSExtractor".into()]
            )
        );
        let slots = main_slots(&mut h, op);
        assert_eq!(slots[0], ("minecraft:wheat".into(), 64));
        assert_eq!(slots[1], ("minecraft:diamond_sword".into(), 1));
        for slot in &slots[2..11] {
            assert_eq!(*slot, ("minecraft:stone".into(), 64));
        }
        assert_eq!(slots[11], ("minecraft:stone".into(), 24));

        // Stone merges into the partial stack; the rest of 36 slots fill and
        // what is left drops at the player.
        h.run(op, "/give @s stone 2000");
        let slots = main_slots(&mut h, op);
        assert!(
            slots[11..]
                .iter()
                .all(|s| *s == ("minecraft:stone".into(), 64))
        );
        let stored: u32 = slots[2..].iter().map(|s| u32::from(s.1)).sum();
        let mut drops = h
            .world()
            .query_filtered::<&ItemEntityStack, bevy_ecs::query::With<DroppedItem>>();
        let dropped: u32 = drops.iter(h.world()).map(|s| u32::from(s.0.count)).sum();
        assert_eq!(stored + dropped, 2600);
        assert!(dropped > 0);
    }

    #[test]
    fn data_values_and_invalid_items() {
        let mut h = Harness::new();
        let op = h.player("BDSExtractor", true);
        assert!(h.run(op, "/give @s bucket 1 1").is_success());
        assert_eq!(slot(&mut h, op, 0).item_id(), "minecraft:milk_bucket");
        assert!(h.run(op, "/give @s diamond_sword 1 17").is_success());
        assert_eq!(
            crate::item::enchant::durability_damage(&slot(&mut h, op, 1)),
            17
        );
        assert_eq!(
            h.run(op, "/give @s bucket 1 2"),
            CommandOutput::failure("commands.give.item.invalid", vec!["bucket".into()])
        );
        let legacy = h.run(op, "/give @s wool 3 14");
        assert!(!legacy.is_success());
        assert!(
            legacy.messages[0]
                .key
                .contains("not implemented in unastar yet")
        );
        assert_eq!(main_slots(&mut h, op)[2].1, 0, "nothing given");
    }

    #[test]
    fn components_become_item_data() {
        let mut h = Harness::new();
        let op = h.player("BDSExtractor", true);
        h.run(op, "/give @s wheat 32");
        assert_eq!(
            h.run(op, r#"/give @s wheat 64 0 {"minecraft:keep_on_death":{}}"#),
            CommandOutput::success(
                "commands.give.success",
                vec!["Wheat".into(), "64".into(), "BDSExtractor".into()]
            )
        );
        // Different item data: a stack of its own.
        let kept = slot(&mut h, op, 1);
        assert_eq!((kept.item_id(), kept.count), ("minecraft:wheat", 64));
        assert!(crate::entity::player_life::death::keeps_on_death(&kept));

        assert_eq!(
            h.run(
                op,
                r#"/give @s minecraft:diamond_sword 1 0 {"minecraft:item_lock":{"mode":"lock_in_inventory"}}"#
            ),
            CommandOutput::success(
                "commands.give.success",
                vec!["Diamond Sword".into(), "1".into(), "BDSExtractor".into()]
            )
        );
        let locked = item_tag(&slot(&mut h, op, 2)).unwrap();
        assert!(matches!(
            locked.0.get("minecraft:item_lock"),
            Some(NBTTag::Byte(b)) if b.0 == 2
        ));

        h.run(
            op,
            r#"/give @s diamond_pickaxe 1 0 {"minecraft:can_destroy":{"blocks":["stone"]},"minecraft:can_place_on":{"blocks":["minecraft:dirt","grass_block"]}}"#,
        );
        assert_eq!(
            block_lists(&slot(&mut h, op, 3)),
            (
                vec![
                    "minecraft:dirt".to_owned(),
                    "minecraft:grass_block".to_owned()
                ],
                vec!["minecraft:stone".to_owned()]
            )
        );
        assert_eq!(
            h.run(
                op,
                r#"/give @s stone 1 0 {"minecraft:item_lock":{"mode":"nope"}}"#
            ),
            CommandOutput::failure("commands.give.tagError", vec!["minecraft:item_lock".into()])
        );
    }
}
