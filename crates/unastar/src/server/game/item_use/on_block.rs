//! Using the held item on a block: hoes, shovels, axes, shears, flint and
//! steel, fire charges, bone meal and buckets.
//!
//! These are built-in item classes in BDS (`HoeItem`, `ShovelItem`,
//! `HatchetItem`, `ShearsItem`, `FlintAndSteelItem`, `FireChargeItem`,
//! `BoneMealItem`, `BucketItem`); the pack has no data for them. Block
//! identities come from the block registry (stripped logs are the
//! registered `stripped_*` blocks, copper ageing follows the registered
//! `exposed_`/`weathered_`/`oxidized_`/`waxed_` names) so no conversion
//! table is retyped. Which blocks a hoe tills and a shovel flattens, the
//! bone meal growth amounts and the sound names follow public references
//! and are recorded in `bds-quirks.md` until a BDS capture of block clicks
//! confirms them.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use rand::Rng;

use super::{give_or_drop, held, set_slot};
use crate::entity::EntityRandom;
use crate::entity::components::GameMode;
use crate::item::ItemStack;
use crate::registry::BlockRegistry;
use crate::registry::block::{BlockEntry, LiquidKind, StateValue};
use crate::server::game::blocks::{block_at, set_block};
use crate::server::game::durability::wear_held;
use crate::server::game::types::BlockRegistryResource;

/// Faces of a block click, in protocol order.
const DOWN: u8 = 0;
const UP: u8 = 1;

/// Use the held item on the clicked block. Returns whether the item did
/// something (then nothing is placed).
pub fn use_on_block(world: &mut World, player: Entity, clicked: IVec3, face: u8) -> bool {
    let Some((slot, stack)) = held(world, player) else {
        return false;
    };
    let Some(entry) = stack.item().cloned() else {
        return false;
    };
    let Some(registry) = world
        .get_resource::<BlockRegistryResource>()
        .map(|r| r.0.clone())
    else {
        return false;
    };
    let Some(state) = block_at(world, (clicked.x, clicked.y, clicked.z)) else {
        return false;
    };
    let Some(block) = registry.get_by_runtime_id(state).cloned() else {
        return false;
    };
    let ctx = Use {
        player,
        slot,
        stack: &stack,
        registry: &registry,
        clicked,
        face,
        state,
        block: &block,
    };
    let id = entry.identifier();
    if entry.has_tag("minecraft:is_hoe") {
        return till(world, &ctx);
    }
    if entry.has_tag("minecraft:is_shovel") {
        return flatten(world, &ctx);
    }
    if entry.has_tag("minecraft:is_axe") {
        return strip_or_scrape(world, &ctx);
    }
    match id {
        "minecraft:shears" => carve(world, &ctx),
        "minecraft:flint_and_steel" | "minecraft:fire_charge" => ignite(world, &ctx),
        "minecraft:bone_meal" => fertilize(world, &ctx),
        "minecraft:bucket" => fill_bucket(world, &ctx),
        "minecraft:water_bucket" | "minecraft:lava_bucket" | "minecraft:powder_snow_bucket" => {
            empty_bucket(world, &ctx)
        }
        _ => false,
    }
}

struct Use<'a> {
    player: Entity,
    slot: u8,
    stack: &'a ItemStack,
    registry: &'a BlockRegistry,
    clicked: IVec3,
    face: u8,
    state: u32,
    block: &'a BlockEntry,
}

impl Use<'_> {
    fn block_name(&self) -> &str {
        &self.block.string_id
    }

    fn target(&self) -> IVec3 {
        self.clicked + face_offset(self.face)
    }

    fn is_air(&self, world: &World, pos: IVec3) -> bool {
        block_at(world, (pos.x, pos.y, pos.z))
            .and_then(|id| self.registry.get_by_runtime_id(id))
            .is_some_and(|b| b.string_id == "minecraft:air")
    }

    fn default_state(&self, name: &str) -> Option<u32> {
        self.registry.default_state_id_by_name(name)
    }
}

fn face_offset(face: u8) -> IVec3 {
    match face {
        0 => IVec3::NEG_Y,
        1 => IVec3::Y,
        2 => IVec3::NEG_Z,
        3 => IVec3::Z,
        4 => IVec3::NEG_X,
        _ => IVec3::X,
    }
}

fn centre(pos: IVec3) -> DVec3 {
    pos.as_dvec3() + DVec3::splat(0.5)
}

/// Replace the clicked block and play the item-use-on sound with the new
/// block, as tilling, flattening and stripping do.
fn convert(world: &mut World, ctx: &Use, new_state: u32) {
    set_block(world, ctx.clicked, new_state, false);
    let wire = crate::server::game::types::block_network_ids(world).to_wire(new_state);
    sound(
        world,
        ctx.player,
        centre(ctx.clicked),
        "item.use.on",
        wire as i32,
    );
}

fn sound(world: &World, player: Entity, at: DVec3, name: &str, data: i32) {
    crate::entity::mob::net::sound::play_level_sound(world, Some(player), at, name, data);
}

/// Use up one of the held item outside creative.
fn use_one(world: &mut World, ctx: &Use) {
    if world.get::<GameMode>(ctx.player) != Some(&GameMode::Creative) {
        set_slot(world, ctx.player, ctx.slot, ctx.stack.grow(-1));
    }
}

/// The state of block `to` that keeps every property `from_state` (of
/// `from`) shares with it, such as a log's `pillar_axis` or a copper
/// stair's facing.
fn carry_states(
    registry: &BlockRegistry,
    from: &BlockEntry,
    from_state: u32,
    to: &str,
) -> Option<u32> {
    let target = registry.get_by_name(to)?;
    let mut state = target.default_state_id;
    for property in from.data.properties {
        if let Some(value) = from.state_value(from_state, property.name)
            && let Some(next) = target.with_state_value(state, property.name, value)
        {
            state = next;
        }
    }
    Some(state)
}

/// Hoe: grass, dirt and dirt paths become farmland, coarse dirt and rooted
/// dirt become dirt (rooted dirt drops hanging roots), from any face but
/// the bottom and only with air above.
fn till(world: &mut World, ctx: &Use) -> bool {
    if ctx.face == DOWN || !ctx.is_air(world, ctx.clicked + IVec3::Y) {
        return false;
    }
    let (into, drop) = match ctx.block_name() {
        "minecraft:grass_block" | "minecraft:dirt" | "minecraft:grass_path" => {
            ("minecraft:farmland", None)
        }
        "minecraft:coarse_dirt" => ("minecraft:dirt", None),
        "minecraft:dirt_with_roots" => ("minecraft:dirt", Some("minecraft:hanging_roots")),
        _ => return false,
    };
    let Some(state) = ctx.default_state(into) else {
        return false;
    };
    convert(world, ctx, state);
    if let Some(drop) = drop.and_then(|name| crate::server::game::world_stack(world, name, 1)) {
        crate::server::game::block_drops::pop_resource(world, ctx.clicked + IVec3::Y, drop);
    }
    wear_held(world, ctx.player, 1);
    true
}

/// Shovel: grass, dirt, coarse dirt, podzol, mycelium and rooted dirt
/// become a dirt path (not from below, only with air above); a lit
/// campfire is put out.
fn flatten(world: &mut World, ctx: &Use) -> bool {
    if matches!(
        ctx.block_name(),
        "minecraft:campfire" | "minecraft:soul_campfire"
    ) {
        let Some(out) = ctx
            .block
            .with_state_value(ctx.state, "extinguished", StateValue::Byte(1))
            .filter(|out| *out != ctx.state)
        else {
            return false;
        };
        set_block(world, ctx.clicked, out, false);
        sound(
            world,
            ctx.player,
            centre(ctx.clicked),
            "extinguish.fire",
            -1,
        );
        wear_held(world, ctx.player, 1);
        return true;
    }
    if ctx.face == DOWN
        || !ctx.is_air(world, ctx.clicked + IVec3::Y)
        || !matches!(
            ctx.block_name(),
            "minecraft:grass_block"
                | "minecraft:dirt"
                | "minecraft:coarse_dirt"
                | "minecraft:podzol"
                | "minecraft:mycelium"
                | "minecraft:dirt_with_roots"
        )
    {
        return false;
    }
    let Some(state) = ctx.default_state("minecraft:grass_path") else {
        return false;
    };
    convert(world, ctx, state);
    wear_held(world, ctx.player, 1);
    true
}

/// The copper block one oxidation stage younger than `name`.
fn younger_copper(name: &str) -> Option<String> {
    let base = name.strip_prefix("minecraft:")?;
    let (stage, rest) = base.split_once('_')?;
    let younger = match stage {
        "exposed" if rest == "copper" => "copper_block".to_owned(),
        "exposed" => rest.to_owned(),
        "weathered" => format!("exposed_{rest}"),
        "oxidized" => format!("weathered_{rest}"),
        _ => return None,
    };
    Some(format!("minecraft:{younger}"))
}

/// Axe: strip a log or wood, remove wax, or scrape one oxidation stage.
fn strip_or_scrape(world: &mut World, ctx: &Use) -> bool {
    let name = ctx.block_name().to_owned();
    let stripped = name
        .strip_prefix("minecraft:")
        .map(|base| format!("minecraft:stripped_{base}"))
        .filter(|stripped| ctx.registry.get_by_name(stripped).is_some());
    if let Some(stripped) = stripped {
        let Some(state) = carry_states(ctx.registry, ctx.block, ctx.state, &stripped) else {
            return false;
        };
        convert(world, ctx, state);
        wear_held(world, ctx.player, 1);
        return true;
    }
    let (into, sound_name) = if let Some(unwaxed) = name
        .strip_prefix("minecraft:waxed_")
        .map(|rest| format!("minecraft:{rest}"))
        .filter(|n| ctx.registry.get_by_name(n).is_some())
    {
        (unwaxed, "wax.off")
    } else if let Some(younger) =
        younger_copper(&name).filter(|n| ctx.registry.get_by_name(n).is_some())
    {
        (younger, "scrape")
    } else {
        return false;
    };
    let Some(state) = carry_states(ctx.registry, ctx.block, ctx.state, &into) else {
        return false;
    };
    set_block(world, ctx.clicked, state, false);
    sound(world, ctx.player, centre(ctx.clicked), sound_name, -1);
    wear_held(world, ctx.player, 1);
    true
}

/// Shears on a pumpkin carve it facing the clicked side and drop four
/// pumpkin seeds there.
fn carve(world: &mut World, ctx: &Use) -> bool {
    if ctx.block_name() != "minecraft:pumpkin" || ctx.face <= UP {
        return false;
    }
    let direction = match ctx.face {
        2 => "north",
        3 => "south",
        4 => "west",
        _ => "east",
    };
    let Some(carved) = ctx
        .registry
        .get_by_name("minecraft:carved_pumpkin")
        .and_then(|b| {
            b.with_state_value(
                b.default_state_id,
                "minecraft:cardinal_direction",
                StateValue::Str(direction),
            )
        })
    else {
        return false;
    };
    set_block(world, ctx.clicked, carved, false);
    if let Some(seeds) = crate::server::game::world_stack(world, "minecraft:pumpkin_seeds", 4) {
        crate::server::game::block_drops::pop_resource(world, ctx.target(), seeds);
    }
    sound(world, ctx.player, centre(ctx.clicked), "pumpkin.carve", -1);
    wear_held(world, ctx.player, 1);
    true
}

/// Flint and steel or a fire charge: light a campfire or candle, else set
/// fire to the air next to the clicked face (soul fire on soul sand and
/// soul soil). Flint and steel wears, a fire charge is used up.
fn ignite(world: &mut World, ctx: &Use) -> bool {
    let flint = ctx.stack.item_id() == "minecraft:flint_and_steel";
    let lit = match ctx.block_name() {
        "minecraft:campfire" | "minecraft:soul_campfire" => {
            ctx.block
                .with_state_value(ctx.state, "extinguished", StateValue::Byte(0))
        }
        name if name.ends_with("candle") || name.ends_with("candle_cake") => ctx
            .block
            .with_state_value(ctx.state, "lit", StateValue::Byte(1)),
        _ => None,
    }
    .filter(|lit| *lit != ctx.state);
    let (pos, state) = match lit {
        Some(lit) => (ctx.clicked, lit),
        None => {
            let target = ctx.target();
            if !ctx.is_air(world, target) {
                return false;
            }
            let below = block_at(world, (target.x, target.y - 1, target.z))
                .and_then(|id| ctx.registry.get_by_runtime_id(id))
                .map(|b| b.string_id.as_str());
            let fire = if matches!(below, Some("minecraft:soul_sand" | "minecraft:soul_soil")) {
                "minecraft:soul_fire"
            } else {
                "minecraft:fire"
            };
            let Some(state) = ctx.default_state(fire) else {
                return false;
            };
            (target, state)
        }
    };
    set_block(world, pos, state, false);
    let sound_name = if flint {
        "ignite"
    } else {
        "mob.ghast.fireball"
    };
    sound(world, ctx.player, centre(pos), sound_name, -1);
    if flint {
        wear_held(world, ctx.player, 1);
    } else {
        use_one(world, ctx);
    }
    true
}

/// Growth stages a crop with `growth` 0..=7 advances by from bone meal.
fn bone_meal_growth(rng: &mut impl Rng) -> i32 {
    rng.gen_range(2..=5)
}

/// Bone meal: grow a crop or stem by 2-5 stages, a sweet berry bush by one,
/// or plant grass and flowers around a grass block.
fn fertilize(world: &mut World, ctx: &Use) -> bool {
    let grown = if ctx.block.has_tag("minecraft:crop")
        || matches!(
            ctx.block_name(),
            "minecraft:melon_stem" | "minecraft:pumpkin_stem"
        ) {
        let Some(StateValue::Int(growth)) = ctx.block.state_value(ctx.state, "growth") else {
            return false;
        };
        if growth >= 7 {
            return false;
        }
        let step = bone_meal_growth(&mut world.get_resource_or_init::<EntityRandom>().0);
        let next = (growth + step).min(7);
        let Some(state) = ctx
            .block
            .with_state_value(ctx.state, "growth", StateValue::Int(next))
        else {
            return false;
        };
        set_block(world, ctx.clicked, state, false);
        true
    } else if ctx.block_name() == "minecraft:sweet_berry_bush" {
        let Some(StateValue::Int(age)) = ctx.block.state_value(ctx.state, "growth") else {
            return false;
        };
        let Some(state) = (age < 3)
            .then(|| {
                ctx.block
                    .with_state_value(ctx.state, "growth", StateValue::Int(age + 1))
            })
            .flatten()
        else {
            return false;
        };
        set_block(world, ctx.clicked, state, false);
        true
    } else if ctx.block_name() == "minecraft:grass_block"
        && ctx.is_air(world, ctx.clicked + IVec3::Y)
    {
        grow_grass(world, ctx);
        true
    } else {
        false
    };
    if grown {
        use_one(world, ctx);
    }
    grown
}

/// Grass block bone meal: 128 random walks from the block above, each up
/// to `i / 16` steps over grass, planting short grass (one in eight a
/// dandelion or poppy) on air above grass.
fn grow_grass(world: &mut World, ctx: &Use) {
    let (Some(grass), Some(short), Some(dandelion), Some(poppy)) = (
        ctx.default_state("minecraft:grass_block"),
        ctx.default_state("minecraft:short_grass"),
        ctx.default_state("minecraft:dandelion"),
        ctx.default_state("minecraft:poppy"),
    ) else {
        return;
    };
    let start = ctx.clicked + IVec3::Y;
    'walks: for i in 0..128 {
        let mut pos = start;
        for _ in 0..i / 16 {
            let step = {
                let rng = &mut world.get_resource_or_init::<EntityRandom>().0;
                IVec3::new(
                    rng.gen_range(-1..=1),
                    rng.gen_range(-1..=1) * rng.gen_range(0..=2) / 2,
                    rng.gen_range(-1..=1),
                )
            };
            pos += step;
            if block_at(world, (pos.x, pos.y - 1, pos.z)) != Some(grass) {
                continue 'walks;
            }
        }
        if !ctx.is_air(world, pos) || block_at(world, (pos.x, pos.y - 1, pos.z)) != Some(grass) {
            continue;
        }
        let plant = {
            let rng = &mut world.get_resource_or_init::<EntityRandom>().0;
            match rng.gen_range(0..8) {
                0 if rng.r#gen::<bool>() => dandelion,
                0 => poppy,
                _ => short,
            }
        };
        set_block(world, pos, plant, false);
    }
}

/// An empty bucket on a still water or lava source, or on powder snow,
/// takes it.
fn fill_bucket(world: &mut World, ctx: &Use) -> bool {
    let filled = match ctx.registry.liquid_state(ctx.state) {
        Some(liquid) if liquid.depth == 0 => match liquid.kind {
            LiquidKind::Water => ("minecraft:water_bucket", "bucket.fill.water"),
            LiquidKind::Lava => ("minecraft:lava_bucket", "bucket.fill.lava"),
        },
        Some(_) => return false,
        None if ctx.block_name() == "minecraft:powder_snow" => {
            ("minecraft:powder_snow_bucket", "bucket.fill.powder_snow")
        }
        None => return false,
    };
    let Some(air) = ctx.default_state("minecraft:air") else {
        return false;
    };
    let Some(bucket) = crate::server::game::world_stack(world, filled.0, 1) else {
        return false;
    };
    set_block(world, ctx.clicked, air, false);
    sound(world, ctx.player, centre(ctx.clicked), filled.1, -1);
    if world.get::<GameMode>(ctx.player) != Some(&GameMode::Creative) {
        let rest = ctx.stack.grow(-1);
        if rest.is_empty() {
            set_slot(world, ctx.player, ctx.slot, bucket);
        } else {
            set_slot(world, ctx.player, ctx.slot, rest);
            give_or_drop(world, ctx.player, bucket);
        }
    }
    true
}

/// A filled bucket places its source block in the clicked block when that
/// is a liquid or replaceable, else next to the clicked face, and becomes
/// an empty bucket.
fn empty_bucket(world: &mut World, ctx: &Use) -> bool {
    let (block, sound_name) = match ctx.stack.item_id() {
        "minecraft:water_bucket" => ("minecraft:water", "bucket.empty.water"),
        "minecraft:lava_bucket" => ("minecraft:lava", "bucket.empty.lava"),
        _ => ("minecraft:powder_snow", "bucket.empty.powder_snow"),
    };
    let replaceable = |pos: IVec3| {
        block_at(world, (pos.x, pos.y, pos.z))
            .and_then(|id| Some((id, ctx.registry.get_by_runtime_id(id)?)))
            .is_some_and(|(id, b)| {
                b.string_id == "minecraft:air" || ctx.registry.liquid_state(id).is_some()
            })
    };
    let pos = if replaceable(ctx.clicked) {
        ctx.clicked
    } else if replaceable(ctx.target()) {
        ctx.target()
    } else {
        return false;
    };
    let Some(state) = ctx.default_state(block) else {
        return false;
    };
    let Some(empty) = crate::server::game::world_stack(world, "minecraft:bucket", 1) else {
        return false;
    };
    set_block(world, pos, state, false);
    sound(world, ctx.player, centre(pos), sound_name, -1);
    if world.get::<GameMode>(ctx.player) != Some(&GameMode::Creative) {
        set_slot(world, ctx.player, ctx.slot, empty);
    }
    true
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::entity::components::{HeldSlot, MainInventory, Position};
    use crate::item::enchant::durability_damage;
    use crate::item::test_support::stack;
    use crate::world::ChunkManager;
    use crate::world::ecs::{ChunkData, ChunkPosition};

    /// A chunk with `block` at (1, 64, 1) and air above, and a survival
    /// player holding `item`.
    fn world_with(block: &str, item: &str, count: u8) -> (World, Entity, Entity) {
        let mut world = World::new();
        world.insert_resource(ChunkManager::new(
            crate::world::WorldConfig::void_spawn_platform(1),
        ));
        world.insert_resource(crate::ecs::events::EventBuffer::default());
        world.insert_resource(crate::server::game::types::ServerConfigResource(
            Default::default(),
        ));
        world.insert_resource(BlockRegistryResource(Arc::new(
            BlockRegistry::vanilla().clone(),
        )));
        let mut items = crate::registry::ItemRegistry::new();
        items.load_vanilla();
        world.insert_resource(crate::server::game::types::ItemRegistryResource(Arc::new(
            items,
        )));
        world.insert_resource(EntityRandom::seeded(3));
        world
            .init_resource::<bevy_ecs::message::Messages<crate::world::ecs::BlockBroadcastEvent>>();
        let mut chunk = crate::world::chunk::Chunk::new(0, 0);
        chunk.set_block(1, 64, 1, state(block));
        let chunk = world.spawn(ChunkData::new(chunk)).id();
        world
            .resource_mut::<ChunkManager>()
            .insert(ChunkPosition::new(0, 0), chunk);
        let mut main = MainInventory::default();
        main.0.set_item(0, stack(item, count)).unwrap();
        let player = world
            .spawn((
                GameMode::Survival,
                Position(DVec3::new(1.5, 66.62, 3.5)),
                main,
                HeldSlot(0),
            ))
            .id();
        (world, player, chunk)
    }

    fn state(name: &str) -> u32 {
        BlockRegistry::vanilla()
            .default_state_id_by_name(name)
            .expect(name)
    }

    fn block(world: &World, chunk: Entity, y: i16) -> String {
        let id = world
            .get::<ChunkData>(chunk)
            .unwrap()
            .inner
            .get_block(1, y, 1);
        BlockRegistry::vanilla()
            .get_by_runtime_id(id)
            .unwrap()
            .string_id
            .clone()
    }

    fn held_stack(world: &World, player: Entity) -> ItemStack {
        world
            .get::<MainInventory>(player)
            .unwrap()
            .0
            .item(0)
            .unwrap()
            .clone()
    }

    #[test]
    fn hoes_till_grass_from_above_and_wear() {
        let (mut world, player, chunk) =
            world_with("minecraft:grass_block", "minecraft:iron_hoe", 1);
        assert!(!use_on_block(
            &mut world,
            player,
            IVec3::new(1, 64, 1),
            DOWN
        ));
        assert!(use_on_block(&mut world, player, IVec3::new(1, 64, 1), UP));
        assert_eq!(block(&world, chunk, 64), "minecraft:farmland");
        assert_eq!(durability_damage(&held_stack(&world, player)), 1);
    }

    #[test]
    fn hoes_need_air_above() {
        let (mut world, player, chunk) = world_with("minecraft:dirt", "minecraft:iron_hoe", 1);
        world.get_mut::<ChunkData>(chunk).unwrap().inner.set_block(
            1,
            65,
            1,
            state("minecraft:stone"),
        );
        assert!(!use_on_block(&mut world, player, IVec3::new(1, 64, 1), 2));
        assert_eq!(block(&world, chunk, 64), "minecraft:dirt");
    }

    #[test]
    fn shovels_make_paths() {
        let (mut world, player, chunk) =
            world_with("minecraft:podzol", "minecraft:wooden_shovel", 1);
        assert!(use_on_block(&mut world, player, IVec3::new(1, 64, 1), 3));
        assert_eq!(block(&world, chunk, 64), "minecraft:grass_path");
    }

    #[test]
    fn axes_strip_wax_and_scrape() {
        for (from, to) in [
            ("minecraft:oak_log", "minecraft:stripped_oak_log"),
            ("minecraft:waxed_exposed_copper", "minecraft:exposed_copper"),
            ("minecraft:weathered_copper", "minecraft:exposed_copper"),
        ] {
            let (mut world, player, chunk) = world_with(from, "minecraft:stone_axe", 1);
            assert!(
                use_on_block(&mut world, player, IVec3::new(1, 64, 1), UP),
                "{from}"
            );
            assert_eq!(block(&world, chunk, 64), to);
        }
        let (mut world, player, _) = world_with("minecraft:copper_block", "minecraft:stone_axe", 1);
        assert!(!use_on_block(&mut world, player, IVec3::new(1, 64, 1), UP));
        assert_eq!(durability_damage(&held_stack(&world, player)), 0);
    }

    #[test]
    fn flint_and_steel_sets_fire_next_to_the_face() {
        let (mut world, player, chunk) =
            world_with("minecraft:stone", "minecraft:flint_and_steel", 1);
        assert!(use_on_block(&mut world, player, IVec3::new(1, 64, 1), UP));
        assert_eq!(block(&world, chunk, 65), "minecraft:fire");
        let (mut world, player, chunk) =
            world_with("minecraft:soul_sand", "minecraft:fire_charge", 2);
        assert!(use_on_block(&mut world, player, IVec3::new(1, 64, 1), UP));
        assert_eq!(block(&world, chunk, 65), "minecraft:soul_fire");
        assert_eq!(held_stack(&world, player).count, 1);
    }

    #[test]
    fn bone_meal_grows_crops_two_to_five_stages() {
        let (mut world, player, chunk) = world_with("minecraft:wheat", "minecraft:bone_meal", 3);
        assert!(use_on_block(&mut world, player, IVec3::new(1, 64, 1), UP));
        let id = world
            .get::<ChunkData>(chunk)
            .unwrap()
            .inner
            .get_block(1, 64, 1);
        let wheat = BlockRegistry::vanilla().get_by_runtime_id(id).unwrap();
        let Some(StateValue::Int(growth)) = wheat.state_value(id, "growth") else {
            panic!("wheat growth");
        };
        assert!((2..=5).contains(&growth), "{growth}");
        assert_eq!(held_stack(&world, player).count, 2);
    }

    #[test]
    fn buckets_take_and_place_sources() {
        let (mut world, player, chunk) = world_with("minecraft:water", "minecraft:bucket", 2);
        assert!(use_on_block(&mut world, player, IVec3::new(1, 64, 1), UP));
        assert_eq!(block(&world, chunk, 64), "minecraft:air");
        assert_eq!(held_stack(&world, player).count, 1);
        let filled = world
            .get::<MainInventory>(player)
            .unwrap()
            .0
            .item(1)
            .unwrap()
            .clone();
        assert_eq!(filled.item_id(), "minecraft:water_bucket");

        let (mut world, player, chunk) = world_with("minecraft:stone", "minecraft:lava_bucket", 1);
        assert!(use_on_block(&mut world, player, IVec3::new(1, 64, 1), UP));
        assert_eq!(block(&world, chunk, 65), "minecraft:lava");
        assert_eq!(held_stack(&world, player).item_id(), "minecraft:bucket");
    }

    #[test]
    fn copper_scrapes_one_stage_younger() {
        assert_eq!(
            younger_copper("minecraft:exposed_copper").as_deref(),
            Some("minecraft:copper_block")
        );
        assert_eq!(
            younger_copper("minecraft:oxidized_cut_copper_stairs").as_deref(),
            Some("minecraft:weathered_cut_copper_stairs")
        );
        assert_eq!(
            younger_copper("minecraft:exposed_cut_copper").as_deref(),
            Some("minecraft:cut_copper")
        );
        assert_eq!(younger_copper("minecraft:copper_block"), None);
    }

    #[test]
    fn every_copper_stage_and_waxed_block_resolves_in_the_registry() {
        let registry = BlockRegistry::vanilla();
        let mut checked = 0;
        for entry in registry.iter() {
            if let Some(younger) = younger_copper(&entry.string_id) {
                assert!(
                    registry.get_by_name(&younger).is_some(),
                    "{} -> {younger}",
                    entry.string_id
                );
                checked += 1;
            }
        }
        assert!(checked > 20, "{checked}");
    }

    #[test]
    fn stripping_keeps_the_log_axis() {
        let registry = BlockRegistry::vanilla();
        let log = registry.get_by_name("minecraft:oak_log").unwrap();
        let sideways = log
            .with_state_value(log.default_state_id, "pillar_axis", StateValue::Str("x"))
            .unwrap();
        let stripped = carry_states(registry, log, sideways, "minecraft:stripped_oak_log").unwrap();
        let entry = registry.get_by_runtime_id(stripped).unwrap();
        assert_eq!(entry.string_id, "minecraft:stripped_oak_log");
        assert_eq!(
            entry.state_value(stripped, "pillar_axis"),
            Some(StateValue::Str("x"))
        );
    }
}
