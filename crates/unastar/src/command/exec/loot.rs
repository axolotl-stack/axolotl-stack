//! `/loot <spawn|give|insert|replace> … <loot|kill|mine> … [tool]`: rolls a
//! loot table into the world, inventories or slots.
//!
//! Measured on BDS 1.26.51 (command capture, `loot` segment):
//!
//! - the table names a path under `loot_tables/` (quoted: `entities/x`
//!   unquoted is a syntax error at the `/`); a missing one answers
//!   `commands.loot.failure.invalidLootTable [loot_tables/<name>.json]`;
//! - `kill <entity>` rolls the entity's own table and `mine <position>` the
//!   block's drops with the tool (`mainhand`/`offhand` of the source, an
//!   item, or an empty hand: stone without a pickaxe drops nothing, air
//!   nothing);
//! - `spawn`, `give` and `insert` answer `commands.loot.success [items]`
//!   (0 included); `insert` into a block without a container answers
//!   `commands.loot.failure.noContainer [Pos(x,y,z)]`;
//! - `replace entity` answers `commands.loot.replaceSuccess [slot type,
//!   slot, name, items, slots]`, `replace block` `replaceSuccess.block
//!   [slot type, slot, items, slots]`; the drops fill consecutive slots from
//!   the one named, at most `count` of them.

use super::replaceitem::{EntitySlot, PlayerSlot, player_slot};
use crate::command::prelude::*;
use crate::entity::components::{
    ArmourInventory, EnderChestInventory, HeldSlot, MainInventory, OffhandSlot, Player,
};
use crate::entity::item_drop::EntityRandom;
use crate::item::ItemStack;
use crate::server::game::{LootContext, LootLevel};
use rand::Rng;
use rand::SeedableRng;

/// Where the loot comes from.
enum Source {
    Table(String),
    Kill(Target),
    Mine(CommandPosition),
}

/// Where the loot goes.
enum Destination {
    Spawn(CommandPosition),
    Give(Target),
    Insert(CommandPosition),
    ReplaceEntity {
        target: Target,
        slot: EntitySlot,
        id: i32,
        count: Option<i32>,
    },
    ReplaceBlock {
        position: CommandPosition,
        id: i32,
        count: Option<i32>,
    },
}

/// `/loot`'s 18 overloads combine a destination and a source; they read by
/// parameter name.
pub fn run(ctx: &mut CommandContext<'_>, args: &crate::command::Args) -> CommandResult {
    let source = if args.has("loot_table") {
        Source::Table(args.get::<Text>("loot_table").expect("loot_table").0)
    } else if args.has("TargetBlockPosition") {
        Source::Mine(args.get("TargetBlockPosition").expect("position"))
    } else {
        // The kill source's entity is the last `entity` target parameter.
        let overload = &args.command().overloads[args.overload()];
        let index = overload
            .params
            .iter()
            .rposition(|p| p.name == "entity" && p.kind.to_string() == "target")
            .expect("kill names an entity");
        let target = args
            .at(index)
            .and_then(<Target as crate::command::FromArg>::from_arg)
            .expect("kill entity");
        Source::Kill(target)
    };
    let tool = args.get::<SoftEnumValue>("<tool>|mainhand|offhand");
    let destination = if args.raw("target").is_some_and(is("spawn")) {
        Destination::Spawn(args.get("position").expect("position"))
    } else if args.raw("target").is_some_and(is("give")) {
        Destination::Give(args.get("players").expect("players"))
    } else if args.raw("target").is_some_and(is("insert")) {
        Destination::Insert(args.get("position").expect("position"))
    } else if args.has("block") {
        Destination::ReplaceBlock {
            position: args.get("position").expect("position"),
            id: args.get("slotId").expect("slotId"),
            count: args.get("count"),
        }
    } else {
        // `replace entity`: the first `entity` parameter is the literal.
        let overload = &args.command().overloads[args.overload()];
        let index = overload
            .params
            .iter()
            .position(|p| p.name == "entity" && p.kind.to_string() == "target")
            .expect("replace entity names a target");
        Destination::ReplaceEntity {
            target: args
                .at(index)
                .and_then(<Target as crate::command::FromArg>::from_arg)
                .expect("replace entity target"),
            slot: args.get("slotType").expect("slotType"),
            id: args.get("slotId").expect("slotId"),
            count: args.get("count"),
        }
    };
    let stacks = roll(ctx, &source, tool.as_deref())?;
    deliver(ctx, destination, stacks)
}

fn is(literal: &'static str) -> impl Fn(&crate::command::parser::ArgValue) -> bool {
    move |value| matches!(value, crate::command::parser::ArgValue::Enum(v) if *v == literal)
}

/// The tool a loot source uses: the source's held items, an item, or none.
fn tool_stack(ctx: &CommandContext<'_>, tool: Option<&str>) -> Option<ItemStack> {
    let entity = ctx.source.entity();
    match tool? {
        "mainhand" => {
            let entity = entity?;
            let slot = ctx
                .world
                .get::<HeldSlot>(entity)
                .map_or(0, |h| h.0 as usize);
            ctx.world
                .get::<MainInventory>(entity)
                .and_then(|main| main.0.item(slot).cloned())
        }
        "offhand" => ctx
            .world
            .get::<OffhandSlot>(entity?)
            .map(|offhand| offhand.0.clone()),
        item => super::give::given_stack(ctx.world, item, 0).ok(),
    }
}

fn roll(
    ctx: &mut CommandContext<'_>,
    source: &Source,
    tool: Option<&str>,
) -> Result<Vec<ItemStack>, CommandOutput> {
    let tool = tool_stack(ctx, tool);
    let table_name = match source {
        Source::Table(name) => {
            let name = name
                .trim_start_matches("loot_tables/")
                .trim_end_matches(".json")
                .to_owned();
            if unastar_data::loot::get(&name).is_none() {
                return Err(CommandOutput::failure(
                    "commands.loot.failure.invalidLootTable",
                    vec![format!("loot_tables/{name}.json")],
                ));
            }
            name
        }
        Source::Kill(target) => {
            let entity = ctx.entity(Some(target))?;
            match ctx
                .world
                .get::<crate::entity::mob::components::LootConfig>(entity)
            {
                Some(loot) => loot.table.to_string(),
                // [INFERENCE] No table (players, unsupported actors): nothing.
                None => return Ok(Vec::new()),
            }
        }
        Source::Mine(position) => {
            let block = ctx.position(Some(*position)).floor().as_ivec3();
            // Measured: air (and an unloaded block) drops nothing.
            let Some(state) =
                crate::server::game::blocks::block_at(ctx.world, (block.x, block.y, block.z))
                    .filter(|state| *state != *crate::world::chunk::blocks::AIR)
            else {
                return Ok(Vec::new());
            };
            return Ok(crate::server::game::block_drops(
                ctx.world,
                state,
                tool.as_ref(),
            ));
        }
    };
    let Some(table) = unastar_data::loot::get(&table_name) else {
        return Ok(Vec::new());
    };
    let seed = ctx
        .world
        .get_resource_or_insert_with(EntityRandom::default)
        .0
        .r#gen::<u64>();
    let items = ctx
        .world
        .get_resource::<crate::server::game::types::ItemRegistryResource>()
        .map(|items| items.0.clone());
    let loot_ctx = LootContext {
        tool: tool.as_ref(),
        level: LootLevel::of(ctx.world),
        items: items.as_deref(),
        ..LootContext::default()
    };
    let drops = unastar_rules::loot::roll(
        unastar_data::loot::vanilla(),
        table,
        &loot_ctx,
        &mut rand::rngs::StdRng::seed_from_u64(seed),
    )
    .unwrap_or_default();
    Ok(crate::server::game::loot_stacks(ctx.world, drops))
}

fn item_count(stacks: &[ItemStack]) -> usize {
    stacks.iter().map(|s| usize::from(s.count)).sum()
}

fn deliver(
    ctx: &mut CommandContext<'_>,
    destination: Destination,
    stacks: Vec<ItemStack>,
) -> CommandResult {
    match destination {
        Destination::Spawn(position) => {
            let at = ctx.position(Some(position));
            let count = item_count(&stacks);
            for stack in stacks {
                crate::entity::spawn_item_drop(ctx.world, at, stack, None);
            }
            success!("commands.loot.success", count)
        }
        Destination::Give(target) => {
            let players = ctx.players(Some(&target))?;
            let count = item_count(&stacks);
            for &player in &players {
                for stack in &stacks {
                    super::give::give(ctx.world, player, stack, u32::from(stack.count));
                }
                crate::server::game::send_inventory_contents(ctx.world, player);
            }
            success!("commands.loot.success", count)
        }
        Destination::Insert(position) => {
            let pos = ctx.block_position(Some(BlockPosition(position)));
            let Some(mut slots) = crate::server::game::command_container_slots(ctx.world, pos)
            else {
                return failure!(
                    "commands.loot.failure.noContainer",
                    format!("Pos({},{},{})", pos.x, pos.y, pos.z)
                );
            };
            let count = item_count(&stacks);
            for stack in stacks {
                if let Some(index) = slots.iter().position(ItemStack::is_empty) {
                    slots[index] = stack.clone();
                    crate::server::game::command_set_container_slot(ctx.world, pos, index, stack);
                }
            }
            success!("commands.loot.success", count)
        }
        Destination::ReplaceBlock {
            position,
            id,
            count,
        } => {
            let pos = ctx.block_position(Some(BlockPosition(position)));
            let Some(slots) = crate::server::game::command_container_slots(ctx.world, pos) else {
                return failure!(
                    "commands.loot.failure.noContainer",
                    format!("Pos({},{},{})", pos.x, pos.y, pos.z)
                );
            };
            let max = slots.len() as i32 - 1;
            if !(0..=max).contains(&id) {
                return failure!(
                    "commands.loot.failure.invalidSlotIdForSlotType",
                    id,
                    0,
                    max,
                    "slot.container"
                );
            }
            let used = fill(stacks, count, |offset, stack| {
                let index = id as usize + offset;
                (index < slots.len()).then(|| {
                    crate::server::game::command_set_container_slot(ctx.world, pos, index, stack);
                })
            });
            success!(
                "commands.loot.replaceSuccess.block",
                "slot.container",
                id,
                used.0,
                used.1
            )
        }
        Destination::ReplaceEntity {
            target,
            slot,
            id,
            count,
        } => {
            let targets = ctx.entities(Some(&target))?;
            let slot_name = super::replaceitem::slot_name(slot);
            let mut output = CommandOutput::default();
            for target in targets {
                let name = ctx.name(target);
                if ctx.world.get::<Player>(target).is_none() {
                    continue;
                }
                let first = player_slot(ctx.world, target, slot, id)?;
                let used = fill(stacks.clone(), count, |offset, stack| {
                    let at = match &first {
                        PlayerSlot::Main(i) => PlayerSlot::Main(i + offset),
                        PlayerSlot::Armour(i) => PlayerSlot::Armour(i + offset),
                        PlayerSlot::EnderChest(i) => PlayerSlot::EnderChest(i + offset),
                        PlayerSlot::Offhand if offset == 0 => PlayerSlot::Offhand,
                        _ => return None,
                    };
                    set_player_slot(ctx.world, target, at, stack)
                });
                crate::server::game::send_inventory_contents(ctx.world, target);
                output.add_success(
                    "commands.loot.replaceSuccess",
                    vec![
                        slot_name.to_owned(),
                        id.to_string(),
                        name,
                        used.0.to_string(),
                        used.1.to_string(),
                    ],
                );
            }
            if output.messages.is_empty() {
                return Err(CommandOutput::no_targets());
            }
            Ok(output)
        }
    }
}

/// Puts the stacks into consecutive slots (at most `count`, one without)
/// through `place(offset, stack)`, which answers `None` past the last slot;
/// returns (items, slots) placed.
fn fill(
    stacks: Vec<ItemStack>,
    count: Option<i32>,
    mut place: impl FnMut(usize, ItemStack) -> Option<()>,
) -> (usize, usize) {
    let limit = count.map_or(1, |c| c.max(0) as usize);
    let (mut items, mut slots) = (0, 0);
    for (offset, stack) in stacks.into_iter().take(limit).enumerate() {
        let n = usize::from(stack.count);
        if place(offset, stack).is_none() {
            break;
        }
        items += n;
        slots += 1;
    }
    (items, slots)
}

fn set_player_slot(
    world: &mut World,
    player: Entity,
    slot: PlayerSlot,
    stack: ItemStack,
) -> Option<()> {
    match slot {
        PlayerSlot::Main(i) if i < 36 => {
            let _ = world.get_mut::<MainInventory>(player)?.0.set_item(i, stack);
        }
        PlayerSlot::Armour(i) if i < 4 => {
            let _ = world
                .get_mut::<ArmourInventory>(player)?
                .inventory_mut()
                .set_item(i, stack);
        }
        PlayerSlot::Offhand => world.get_mut::<OffhandSlot>(player)?.0 = stack,
        PlayerSlot::EnderChest(i) if i < 27 => {
            let _ = world
                .get_mut::<EnderChestInventory>(player)?
                .0
                .set_item(i, stack);
        }
        _ => return None,
    }
    Some(())
}

/// Replays of the committed BDS 1.26.51 command capture
/// (`crates/bds-extractor/captures/1.26.51/operator-commands`).
#[cfg(test)]
mod bds_capture {
    use crate::command::exec::evidence::Replay;

    #[test]
    fn loot_matches_the_bds_capture() {
        let differ = [
            // unastar's mob runtime cannot spawn armor stands yet (the
            // summon row's gap); `kill` is tested on a pig below.
            "/summon armor_stand 4 200 4",
            "/loot give @s kill @e[type=armor_stand,r=16]",
            "/kill @e[type=armor_stand,r=16]",
        ];
        Replay::new().assert_segment("loot", &differ);
    }

    #[test]
    fn kill_rolls_the_entitys_own_table() {
        let mut replay = Replay::new();
        assert!(replay.run("/summon pig 4 200 4").0.is_success());
        let (output, _) = replay.run("/loot give @s kill @e[type=pig,r=16]");
        let count: usize = output.messages[0].params[0].parse().unwrap();
        assert_eq!(output.messages[0].key, "commands.loot.success");
        // `entities/pig`: one to three porkchops.
        assert!((1..=3).contains(&count), "{output:?}");
        let main = replay
            .h
            .world()
            .get::<crate::entity::components::MainInventory>(replay.player)
            .unwrap()
            .0
            .slots()
            .iter()
            .map(|s| (s.item_id().to_owned(), usize::from(s.count)))
            .filter(|(_, n)| *n > 0)
            .collect::<Vec<_>>();
        assert_eq!(main, [("minecraft:porkchop".to_owned(), count)]);
    }
}
