//! Fire and soul fire, as BDS 1.26.51 runs them.
//!
//! - **Survival**: fire never stays on a campfire or soul campfire; it
//!   stays on a block whose top face is full, next to a block that can
//!   catch fire (any of its six neighbours), or over soul sand and soul
//!   soil (where it becomes soul fire). Soul fire stays only over soul sand
//!   and soul soil. Fire that loses its place goes out without particles or
//!   drops.
//! - **Placing** fire over soul sand or soul soil turns it into soul fire at
//!   once; any other fire gets its first tick one tick later. When the
//!   block below a fire becomes soul sand or soul soil, the fire turns into
//!   soul fire three ticks later. Soul fire never ticks: it neither ages,
//!   spreads nor burns blocks.
//! - **A fire tick**, in this order:
//!   1. over soul sand or soul soil, turn into soul fire and stop;
//!   2. with `doFireTick` off, only schedule the next tick;
//!   3. unless the fire burns forever (over netherrack, magma, soul sand,
//!      soul soil, or bedrock with `infiniburn_bit`), raise `age` by
//!      `nextInt(3) / 2` (0 or 1) up to [`MAX_AGE`];
//!   4. schedule the next tick in [`TICK_DELAY`] + `nextInt(`
//!      [`TICK_DELAY_RANDOM`]`)` ticks, unless one is pending;
//!   5. unless it burns forever: with no neighbour that can catch fire, or
//!      water below, go out unless the top below is full and the age is
//!      below 4, and stop there; over a block that cannot catch fire, at
//!      age 15, go out when `nextInt(4) == 0`;
//!   6. burn the neighbours (east, west, below, above, north, south) with
//!      bounds 300 sideways and 250 above and below;
//!   7. spread into every air cell of the box x ± 1, z ± 1, y − 1 ..= y + 4
//!      (x outermost, then z, then y), with
//!      `chance = (encouragement + 40 + 7 × difficulty) / (age + 30)`
//!      (integer division; `encouragement` the highest among the cell's six
//!      neighbours, skipped at 0; `difficulty` 0 for peaceful to 3 for
//!      hard): when `chance > 0` and `nextFloat() × k <= chance`, `k` being
//!      100 up to one block above and `100 × dy` higher, the cell gets fire
//!      of age `min(age + nextInt(5) / 4, 15)` (the BDS 1.26.51 capture has
//!      spread fires one older than their source as well as equal ones).
//!      Steps 5 to 7 use the age after step 3.
//! - **Burning a neighbour** happens when `nextInt(bound) < flammability`.
//!   A campfire or soul campfire is lit. Any other block is removed (never
//!   with drops); then, when `nextInt(age + 10) < 5`, fire of age
//!   `min(age + nextInt(5) / 4, 15)` takes its place if fire can stay
//!   there. TNT is removed without fire; priming it is not modelled.
//! - **Not modelled**: rain and humid biomes (the server has no weather, so
//!   rain never puts fire out or stops it spreading, and no biome lowers the
//!   odds); the `tntExplodes` rule (there is none, so it counts as on and
//!   fire over TNT burns like fire over any block); water held by a
//!   waterlogged block below (only a water block below counts); nether
//!   portals lit by fire.
//! - **Random ticks** do nothing to fire.
//! - **Punching** the face of a block with fire or soul fire in the cell
//!   beyond that face puts it out with the `extinguish.fire` sound at the
//!   fire's centre, data −1.
//! - **Lava** ([`lava_ignites`], on its random and scheduled ticks), with
//!   `doFireTick` on and air above the lava: ten tries at the cell
//!   `lava + (nextInt(3) − 1, nextInt(3) − 1, nextInt(3) − 1)`; when that
//!   block is lava-flammable, the first air among its neighbours (east,
//!   west, above, below, south, north) gets fire of age 0 and lava stops.
//!
//! Flammability ([`flammability`]): blocks defined in the vanilla behaviour
//! pack take their `minecraft:flammable` values (catch chance =
//! encouragement, destroy chance = flammability, `lava_flammable`; a test
//! checks the table against the pack). Built-in blocks use the values BDS
//! 1.26.51 builds in for wood, logs, leaves, carpets, shelves, flowers and
//! double plants, TNT, azaleas, cave vines, campfires, bee nests, beehives
//! and coal (of those, flowers, coal, azaleas, cave vines, moss carpet and
//! campfires are not lava-flammable). The rest of the table (wool,
//! bookshelves, lecterns, grass-like plants other than short grass and
//! ferns, vines, lichen, dripleaf, moss, roots, hay, dried kelp, scaffolding,
//! bamboo, target, composter) and the order in which the neighbours burn
//! are provisional: Minecraft Wiki "Fire".

use bevy_ecs::prelude::*;
use glam::IVec3;
use rand::Rng;

use super::{
    NEIGHBOURS, ScheduledTicks, SupportKind, air, bit, fire_ticks, int, name_at, provides_support,
    random, with,
};
use crate::entity::systems::BlockSource;
use crate::registry::BlockRegistry;
use crate::registry::block::{BlockEntry, StateValue};
use crate::server::game::blocks::set_block;
use crate::server::game::types::BlockRegistryResource;

/// Fire's age state property.
pub const AGE: &str = "age";
/// The highest age.
pub const MAX_AGE: i32 = 15;
/// Fixed part of the delay between fire ticks.
pub const TICK_DELAY: u64 = 30;
/// Random extra delay between fire ticks: `0..TICK_DELAY_RANDOM`.
pub const TICK_DELAY_RANDOM: u64 = 10;
/// Delay of a placed fire's first tick.
pub const FIRST_TICK_DELAY: u64 = 1;
/// Delay before fire whose base became soul sand or soul soil turns into
/// soul fire.
pub const SOUL_DELAY: u64 = 3;
/// Tries lava makes per tick to set fire around it.
pub const LAVA_TRIES: u32 = 10;
/// `LevelSoundEvent` played where a punched fire goes out.
pub const SOUND_EXTINGUISH: &str = "extinguish.fire";

const FIRE: &str = "minecraft:fire";
const SOUL_FIRE: &str = "minecraft:soul_fire";

/// How a block burns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flammable {
    /// Encouragement: how readily fire spreads into the cells next to it.
    pub catch: i32,
    /// Flammability: how readily fire next to it burns it away.
    pub destroy: i32,
    /// Whether lava sets fire next to it.
    pub lava: bool,
}

impl Flammable {
    const fn new(catch: i32, destroy: i32, lava: bool) -> Self {
        Self {
            catch,
            destroy,
            lava,
        }
    }
}

/// Woods whose planks, slabs, stairs, fences, fence gates, shelves, logs
/// and wood burn (the nether stems do not).
const WOODS: [&str; 11] = [
    "oak", "spruce", "birch", "jungle", "acacia", "dark_oak", "mangrove", "cherry", "pale_oak",
    "poplar", "bamboo",
];

const PLANKS: Flammable = Flammable::new(5, 20, true);
const LOG: Flammable = Flammable::new(5, 5, true);
const LEAVES: Flammable = Flammable::new(30, 60, true);
const SHELF: Flammable = Flammable::new(30, 20, true);
const FLOWER: Flammable = Flammable::new(60, 100, false);
const PLANT: Flammable = Flammable::new(60, 100, true);
const AZALEA: Flammable = Flammable::new(15, 100, false);
const TNT: Flammable = Flammable::new(15, 100, true);
const WOOL: Flammable = Flammable::new(30, 60, true);
const VINE: Flammable = Flammable::new(15, 100, true);
const MOSS: Flammable = Flammable::new(5, 100, true);

/// Whether `name` is `<wood>_<suffix>` for a burning wood and one of
/// `suffixes`.
fn wooden(name: &str, suffixes: &[&str]) -> bool {
    WOODS.iter().any(|wood| {
        name.strip_prefix(wood)
            .and_then(|rest| rest.strip_prefix('_'))
            .is_some_and(|rest| suffixes.contains(&rest))
    })
}

/// How the block `entry` burns; `None` for blocks fire never burns.
pub fn flammability(entry: &BlockEntry) -> Option<Flammable> {
    let name = entry.string_id.strip_prefix("minecraft:")?;
    // Behaviour pack blocks (`minecraft:flammable`).
    if name.ends_with("_wool_slab")
        || name.ends_with("_wool_double_slab")
        || name.ends_with("_wool_stairs")
    {
        return Some(Flammable::new(30, 60, true));
    }
    if name == "red_shrub" {
        return Some(Flammable::new(60, 100, false));
    }
    // Built-in blocks.
    if name == "fence_gate"
        || wooden(
            name,
            &[
                "planks",
                "slab",
                "double_slab",
                "stairs",
                "fence",
                "fence_gate",
            ],
        )
        || matches!(
            name,
            "bamboo_mosaic"
                | "bamboo_mosaic_slab"
                | "bamboo_mosaic_double_slab"
                | "bamboo_mosaic_stairs"
                | "bamboo_block"
                | "stripped_bamboo_block"
                | "bee_nest"
                | "beehive"
        )
    {
        return Some(PLANKS);
    }
    let unstripped = name.strip_prefix("stripped_").unwrap_or(name);
    if !unstripped.starts_with("bamboo_") && wooden(unstripped, &["log", "wood"]) {
        return Some(LOG);
    }
    if wooden(name, &["shelf"]) || matches!(name, "bookshelf" | "chiseled_bookshelf" | "lectern") {
        return Some(SHELF);
    }
    if matches!(name, "azalea_leaves" | "azalea_leaves_flowered") {
        return Some(Flammable::new(15, 100, true));
    }
    if name.ends_with("_leaves") {
        return Some(LEAVES);
    }
    if name.ends_with("_wool") {
        return Some(WOOL);
    }
    if name == "moss_carpet" {
        return Some(Flammable::new(15, 100, false));
    }
    if name == "pale_moss_carpet" {
        return Some(MOSS);
    }
    if name.ends_with("_carpet") {
        return Some(SHELF);
    }
    Some(match name {
        "campfire" | "soul_campfire" => Flammable::new(5, 20, false),
        "coal_block" => Flammable::new(5, 5, false),
        "tnt" | "underwater_tnt" => TNT,
        "dandelion" | "golden_dandelion" | "poppy" | "blue_orchid" | "allium" | "azure_bluet"
        | "red_tulip" | "orange_tulip" | "white_tulip" | "pink_tulip" | "oxeye_daisy"
        | "cornflower" | "lily_of_the_valley" | "wither_rose" | "torchflower"
        | "open_eyeblossom" | "closed_eyeblossom" | "pink_petals" | "wildflowers" => FLOWER,
        "short_grass" | "fern" | "sunflower" | "lilac" | "tall_grass" | "large_fern"
        | "rose_bush" | "peony" | "deadbush" | "bush" | "firefly_bush" | "short_dry_grass"
        | "tall_dry_grass" | "sweet_berry_bush" | "leaf_litter" | "cactus_flower"
        | "spore_blossom" | "pitcher_plant" => PLANT,
        "azalea"
        | "flowering_azalea"
        | "cave_vines"
        | "cave_vines_body_with_berries"
        | "cave_vines_head_with_berries" => AZALEA,
        "vine" | "glow_lichen" | "big_dripleaf" | "small_dripleaf_block" => VINE,
        "moss_block" | "pale_moss_block" | "pale_hanging_moss" => MOSS,
        "hay_block" | "dried_kelp_block" => Flammable::new(60, 20, true),
        "target" => Flammable::new(15, 20, true),
        "hanging_roots" => Flammable::new(30, 60, true),
        "scaffolding" | "bamboo" => Flammable::new(60, 60, true),
        "composter" | "mangrove_roots" => PLANKS,
        _ => return None,
    })
}

/// How the block at `pos` burns (`None` when it does not or is unloaded).
fn flammable_at(blocks: &impl BlockSource, pos: IVec3) -> Option<Flammable> {
    let state = blocks.block_at(pos)?;
    flammability(blocks.registry().get_by_runtime_id(state)?)
}

/// Encouragement of the block at `pos` (0 for blocks that do not burn).
fn catch_at(blocks: &impl BlockSource, pos: IVec3) -> i32 {
    flammable_at(blocks, pos).map_or(0, |f| f.catch)
}

/// Whether any of the six neighbours of `pos` can catch fire.
fn next_to_flammable(blocks: &impl BlockSource, pos: IVec3) -> bool {
    NEIGHBOURS
        .iter()
        .any(|offset| catch_at(blocks, pos + *offset) > 0)
}

/// Whether the block below `pos` has a full top face.
fn on_solid_top(blocks: &impl BlockSource, pos: IVec3) -> bool {
    provides_support(blocks, pos - IVec3::Y, IVec3::Y, SupportKind::Full)
}

/// Whether the block below `pos` is soul sand or soul soil.
fn on_soul_base(blocks: &impl BlockSource, pos: IVec3) -> bool {
    matches!(
        name_at(blocks, pos - IVec3::Y),
        Some("minecraft:soul_sand" | "minecraft:soul_soil")
    )
}

/// Whether fire can stay at `pos`.
fn fire_can_stay(blocks: &impl BlockSource, pos: IVec3) -> bool {
    if matches!(
        name_at(blocks, pos - IVec3::Y),
        Some("minecraft:campfire" | "minecraft:soul_campfire")
    ) {
        return false;
    }
    on_soul_base(blocks, pos) || on_solid_top(blocks, pos) || next_to_flammable(blocks, pos)
}

pub fn can_survive(blocks: &impl BlockSource, pos: IVec3, _state: u32, entry: &BlockEntry) -> bool {
    if entry.string_id == SOUL_FIRE {
        on_soul_base(blocks, pos)
    } else {
        fire_can_stay(blocks, pos)
    }
}

/// Whether `state` is fire or soul fire.
fn is_fire(registry: &BlockRegistry, state: u32) -> bool {
    registry
        .get_by_runtime_id(state)
        .is_some_and(|entry| matches!(entry.string_id.as_str(), FIRE | SOUL_FIRE))
}

/// Fire with `age`.
fn fire_state(registry: &BlockRegistry, age: i32) -> Option<u32> {
    let entry = registry.get_by_name(FIRE)?;
    Some(with(
        entry,
        entry.default_state_id,
        AGE,
        StateValue::Int(age.min(MAX_AGE)),
    ))
}

/// Turn the fire at `pos` into soul fire.
fn to_soul_fire(world: &mut World, pos: IVec3) {
    let soul = world
        .resource::<BlockRegistryResource>()
        .0
        .default_state_id_by_name(SOUL_FIRE);
    if let Some(soul) = soul {
        set_block(world, pos, soul, false);
    }
}

/// Whether a tick is already pending at `pos`.
fn tick_pending(world: &World, pos: IVec3) -> bool {
    world
        .get_resource::<ScheduledTicks>()
        .is_some_and(|ticks| ticks.0.iter().any(|(_, at, _)| *at == pos))
}

pub fn placed(world: &mut World, pos: IVec3, state: u32) {
    let registry = world.resource::<BlockRegistryResource>().0.clone();
    let Some(entry) = registry.get_by_runtime_id(state) else {
        return;
    };
    if !can_survive(world, pos, state, entry) {
        set_block(world, pos, air(&registry), false);
        return;
    }
    if entry.string_id != FIRE {
        return;
    }
    if on_soul_base(world, pos) {
        to_soul_fire(world, pos);
    } else if !tick_pending(world, pos) {
        super::schedule_tick(world, pos, FIRST_TICK_DELAY);
    }
}

pub fn neighbour_changed(world: &mut World, pos: IVec3, _state: u32) {
    if name_at(world, pos) != Some(FIRE) || !on_soul_base(world, pos) {
        return;
    }
    // The conversion tick: bring a pending fire tick forward, or schedule
    // one.
    let due = world
        .get_resource::<crate::ecs::resources::TickCounter>()
        .map_or(0, |t| t.current)
        + SOUL_DELAY;
    let moved = world
        .get_resource_mut::<ScheduledTicks>()
        .and_then(|mut ticks| {
            let pending = ticks.0.iter_mut().find(|(_, at, _)| *at == pos)?;
            pending.0 = pending.0.min(due);
            Some(())
        })
        .is_some();
    if !moved {
        super::schedule_tick(world, pos, SOUL_DELAY);
    }
}

/// Whether fire at `pos` burns forever: over netherrack, magma, soul sand,
/// soul soil or bedrock with `infiniburn_bit`.
fn burns_forever(blocks: &impl BlockSource, pos: IVec3) -> bool {
    let below = pos - IVec3::Y;
    let Some(state) = blocks.block_at(below) else {
        return false;
    };
    let Some(entry) = blocks.registry().get_by_runtime_id(state) else {
        return false;
    };
    match entry.string_id.as_str() {
        "minecraft:netherrack"
        | "minecraft:magma"
        | "minecraft:soul_sand"
        | "minecraft:soul_soil" => true,
        "minecraft:bedrock" => bit(entry, state, "infiniburn_bit"),
        _ => false,
    }
}

/// A draw in `0..bound`.
fn roll(world: &mut World, bound: i32) -> i32 {
    random(world).gen_range(0..bound)
}

/// Schedule the next fire tick at `pos` unless one is pending.
fn schedule_next(world: &mut World, pos: IVec3) {
    if tick_pending(world, pos) {
        return;
    }
    let delay = TICK_DELAY + random(world).gen_range(0..TICK_DELAY_RANDOM);
    super::schedule_tick(world, pos, delay);
}

pub fn scheduled_tick(world: &mut World, pos: IVec3, state: u32) {
    let registry = world.resource::<BlockRegistryResource>().0.clone();
    let Some(entry) = registry.get_by_runtime_id(state) else {
        return;
    };
    if entry.string_id != FIRE {
        return;
    }
    if on_soul_base(world, pos) {
        to_soul_fire(world, pos);
        return;
    }
    if !fire_ticks(world) {
        schedule_next(world, pos);
        return;
    }
    let forever = burns_forever(world, pos);
    let mut age = int(entry, state, AGE).unwrap_or(0);
    let old_age = age;
    if age < MAX_AGE && !forever {
        age = (age + roll(world, 3) / 2).min(MAX_AGE);
    }
    schedule_next(world, pos);
    if age != old_age {
        set_block(
            world,
            pos,
            with(entry, state, AGE, StateValue::Int(age)),
            false,
        );
    }
    let air = air(&registry);
    if !forever {
        let water_below = matches!(
            name_at(world, pos - IVec3::Y),
            Some("minecraft:water" | "minecraft:flowing_water")
        );
        if !next_to_flammable(world, pos) || water_below {
            if !(on_solid_top(world, pos) && age < 4) {
                set_block(world, pos, air, false);
            }
            return;
        }
        if catch_at(world, pos - IVec3::Y) == 0 && age == MAX_AGE && roll(world, 4) == 0 {
            set_block(world, pos, air, false);
            return;
        }
    }
    for (offset, bound) in [
        (IVec3::X, 300),
        (IVec3::NEG_X, 300),
        (IVec3::NEG_Y, 250),
        (IVec3::Y, 250),
        (IVec3::NEG_Z, 300),
        (IVec3::Z, 300),
    ] {
        burn(world, pos + offset, bound, age);
    }
    spread(world, pos, age);
}

/// The fire next to `pos` (aged `age`) may burn the block there.
fn burn(world: &mut World, pos: IVec3, bound: i32, age: i32) {
    let Some(state) = world.block_at(pos) else {
        return;
    };
    let registry = world.resource::<BlockRegistryResource>().0.clone();
    let Some(entry) = registry.get_by_runtime_id(state) else {
        return;
    };
    let destroy = flammability(entry).map_or(0, |f| f.destroy);
    if roll(world, bound) >= destroy {
        return;
    }
    let name = entry.string_id.as_str();
    if matches!(name, "minecraft:campfire" | "minecraft:soul_campfire") {
        if bit(entry, state, "extinguished") {
            let lit = with(entry, state, "extinguished", StateValue::Byte(0));
            set_block(world, pos, lit, false);
        }
        return;
    }
    let catches = roll(world, age + 10) < 5;
    let new_age = if catches {
        Some((age + roll(world, 5) / 4).min(MAX_AGE))
    } else {
        None
    };
    set_block(world, pos, air(&registry), false);
    if matches!(name, "minecraft:tnt" | "minecraft:underwater_tnt") {
        return;
    }
    if let Some(new_age) = new_age
        && next_to_flammable(world, pos)
        && let Some(fire) = fire_state(&registry, new_age)
    {
        set_block(world, pos, fire, false);
    }
}

/// The fire at `pos` (aged `age`) may spread into the air around it.
fn spread(world: &mut World, pos: IVec3, age: i32) {
    let difficulty = world
        .get_resource::<crate::world::rules::Difficulty>()
        .copied()
        .unwrap_or_default() as i32;
    let registry = world.resource::<BlockRegistryResource>().0.clone();
    let air = air(&registry);
    for dx in -1..=1 {
        for dz in -1..=1 {
            for dy in -1..=4 {
                if dx == 0 && dy == 0 && dz == 0 {
                    continue;
                }
                let target = pos + IVec3::new(dx, dy, dz);
                if world.block_at(target) != Some(air) {
                    continue;
                }
                let encouragement = NEIGHBOURS
                    .iter()
                    .map(|offset| catch_at(world, target + *offset))
                    .max()
                    .unwrap_or(0);
                if encouragement <= 0 {
                    continue;
                }
                let chance = (encouragement + 40 + difficulty * 7) / (age + 30);
                let k = if dy > 1 { 100 * dy } else { 100 };
                if chance > 0 && random(world).r#gen::<f32>() * k as f32 <= chance as f32 {
                    let new_age = age + roll(world, 5) / 4;
                    if let Some(fire) = fire_state(&registry, new_age) {
                        set_block(world, target, fire, false);
                    }
                }
            }
        }
    }
}

pub fn random_tick(_world: &mut World, _pos: IVec3, _state: u32) {}

/// A player punched the face `face` of the block at `pos`: put out a fire
/// on that face. Returns whether a fire went out.
pub fn punch(world: &mut World, pos: IVec3, face: u8) -> bool {
    let Some(offset) = NEIGHBOURS.get(usize::from(face)) else {
        return false;
    };
    let target = pos + *offset;
    let Some(state) = world.block_at(target) else {
        return false;
    };
    let registry = world.resource::<BlockRegistryResource>().0.clone();
    if !is_fire(&registry, state) {
        return false;
    }
    crate::entity::mob::net::sound::play_level_sound(
        world,
        None,
        super::centre(target),
        SOUND_EXTINGUISH,
        -1,
    );
    set_block(world, target, air(&registry), false);
    true
}

/// Lava's random tick may set fire to flammable blocks around `pos`.
pub fn lava_ignites(world: &mut World, pos: IVec3) {
    if !fire_ticks(world) {
        return;
    }
    let registry = world.resource::<BlockRegistryResource>().0.clone();
    let air = air(&registry);
    let above_open = world.block_at(pos + IVec3::Y) == Some(air)
        || flammable_at(world, pos + IVec3::Y).is_some_and(|f| f.lava);
    if !above_open {
        return;
    }
    let Some(fire) = fire_state(&registry, 0) else {
        return;
    };
    for _ in 0..LAVA_TRIES {
        let (x, y, z) = (roll(world, 3) - 1, roll(world, 3) - 1, roll(world, 3) - 1);
        let cell = pos + IVec3::new(x, y, z);
        if !flammable_at(world, cell).is_some_and(|f| f.lava) {
            continue;
        }
        let open = [
            IVec3::X,
            IVec3::NEG_X,
            IVec3::Y,
            IVec3::NEG_Y,
            IVec3::Z,
            IVec3::NEG_Z,
        ]
        .into_iter()
        .map(|offset| cell + offset)
        .find(|side| world.block_at(*side) == Some(air));
        if let Some(side) = open {
            set_block(world, side, fire, false);
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use glam::IVec3;
    use jolyne::valentine::McpePacketData;

    use super::super::LEVEL_EVENT_PARTICLES_DESTROY_BLOCK;
    use super::super::tests::{FLOOR, Harness, state};
    use super::*;
    use crate::registry::BlockRegistry;

    const FIRE_POS: IVec3 = IVec3::new(0, FLOOR + 1, 0);
    const AIR: &str = "minecraft:air";

    fn id(name: &str) -> u32 {
        state(name, &[])
    }

    fn age(h: &Harness, pos: IVec3) -> Option<StateValue> {
        h.value(pos, AGE)
    }

    #[test]
    fn fire_on_netherrack_never_ages_or_burns_out() {
        let mut h = Harness::new();
        h.put(FIRE_POS - IVec3::Y, id("minecraft:netherrack"));
        h.set(FIRE_POS, id(FIRE));
        h.tick(4000);
        assert_eq!(h.name(FIRE_POS), FIRE);
        assert_eq!(age(&h, FIRE_POS), Some(StateValue::Int(0)));
    }

    #[test]
    fn fire_on_stone_with_nothing_to_burn_goes_out_without_drops() {
        let mut h = Harness::new();
        h.set(FIRE_POS, id(FIRE));
        h.tick(40);
        assert_eq!(h.name(FIRE_POS), FIRE, "young fire stays");
        h.tick(2000);
        assert_eq!(h.name(FIRE_POS), AIR);
        assert!(h.drops().is_empty());
    }

    #[test]
    fn fire_ages_by_at_most_one_per_tick() {
        let mut h = Harness::new();
        h.put(FIRE_POS + IVec3::X, id("minecraft:stone"));
        h.put(FIRE_POS + IVec3::NEG_X, id("minecraft:coal_block"));
        h.set(FIRE_POS, id(FIRE));
        let mut last = 0;
        for _ in 0..1000 {
            h.tick(1);
            let Some(StateValue::Int(now)) = age(&h, FIRE_POS) else {
                break;
            };
            assert!(now == last || now == last + 1, "{last} -> {now}");
            last = now;
        }
        assert!(last > 0, "the fire never aged");
    }

    #[test]
    fn fire_goes_out_without_particles_or_drops_when_its_support_goes() {
        let mut h = Harness::new();
        h.set(FIRE_POS, id(FIRE));
        h.packets();
        h.set(FIRE_POS - IVec3::Y, id(AIR));
        h.tick(1);
        assert_eq!(h.name(FIRE_POS), AIR);
        let particles = h.packets().into_iter().any(|packet| {
            matches!(packet, McpePacketData::LevelEventPacket(event)
                if event.event_id == LEVEL_EVENT_PARTICLES_DESTROY_BLOCK)
        });
        assert!(!particles, "fire went out with destroy particles");
        h.tick(100);
        assert!(h.drops().is_empty());
    }

    /// A plank ring around the fire and planks two above it.
    fn plank_hut(h: &mut Harness) -> Vec<IVec3> {
        let planks = id("minecraft:oak_planks");
        let mut walls = Vec::new();
        for dx in -1..=1 {
            for dz in -1..=1 {
                if dx != 0 || dz != 0 {
                    walls.push(FIRE_POS + IVec3::new(dx, 0, dz));
                }
            }
        }
        walls.push(FIRE_POS + IVec3::new(0, 2, 0));
        for pos in &walls {
            h.put(*pos, planks);
        }
        walls
    }

    #[test]
    fn fire_burns_planks_away_without_drops() {
        let mut h = Harness::new();
        let walls = plank_hut(&mut h);
        h.set(FIRE_POS, id(FIRE));
        h.tick(6000);
        let burnt = walls
            .iter()
            .filter(|pos| h.name(**pos) != "minecraft:oak_planks")
            .count();
        assert!(burnt >= 3, "only {burnt} planks burnt");
        assert!(h.drops().is_empty());
    }

    #[test]
    fn fire_spreads_into_the_air_over_planks() {
        let mut h = Harness::new();
        // On netherrack the fire never ages or goes out; the corner planks
        // are not its neighbours, so only spreading reaches above them.
        h.put(FIRE_POS - IVec3::Y, id("minecraft:netherrack"));
        let walls = plank_hut(&mut h);
        h.set(FIRE_POS, id(FIRE));
        let corners: Vec<IVec3> = walls
            .iter()
            .filter(|pos| pos.x != FIRE_POS.x && pos.z != FIRE_POS.z)
            .map(|pos| *pos + IVec3::Y)
            .collect();
        assert_eq!(corners.len(), 4);
        let mut spread = false;
        for _ in 0..600 {
            h.tick(10);
            spread = corners.iter().any(|pos| h.name(*pos) == FIRE);
            if spread {
                break;
            }
        }
        assert!(spread, "fire never spread over the corner planks");
    }

    #[test]
    fn without_fire_ticks_fire_neither_ages_burns_nor_spreads() {
        let mut h = Harness::new();
        h.world
            .resource_mut::<crate::world::rules::GameRules>()
            .set_value(
                "doFireTick",
                unastar_data::game_rules::GameRuleValue::Bool(false),
            )
            .unwrap();
        let walls = plank_hut(&mut h);
        h.set(FIRE_POS, id(FIRE));
        h.tick(6000);
        assert_eq!(h.name(FIRE_POS), FIRE);
        assert_eq!(age(&h, FIRE_POS), Some(StateValue::Int(0)));
        for pos in &walls {
            assert_eq!(h.name(*pos), "minecraft:oak_planks");
        }
        // Fire that loses its place still goes out.
        for pos in &walls {
            h.set(*pos, id(AIR));
        }
        h.set(FIRE_POS - IVec3::Y, id(AIR));
        h.tick(1);
        assert_eq!(h.name(FIRE_POS), AIR);
    }

    #[test]
    fn burning_lights_an_extinguished_campfire_without_removing_it() {
        let mut h = Harness::new();
        let out = state(
            "minecraft:campfire",
            &[("extinguished", StateValue::Byte(1))],
        );
        h.put(FIRE_POS + IVec3::X, out);
        h.set(FIRE_POS, id(FIRE));
        let mut lit = false;
        for _ in 0..400 {
            h.tick(10);
            assert_eq!(h.name(FIRE_POS + IVec3::X), "minecraft:campfire");
            lit = h.value(FIRE_POS + IVec3::X, "extinguished") == Some(StateValue::Byte(0));
            if lit || h.name(FIRE_POS) != FIRE {
                break;
            }
        }
        assert!(lit, "the campfire was never lit");
    }

    #[test]
    fn punching_the_face_below_a_fire_puts_it_out_with_the_sound() {
        let mut h = Harness::new();
        h.set(FIRE_POS, id(FIRE));
        h.packets();
        let floor = FIRE_POS - IVec3::Y;
        assert!(!punch(&mut h.world, floor, 0), "nothing below the floor");
        assert!(!punch(&mut h.world, floor, 6), "not a face");
        assert!(punch(&mut h.world, floor, 1));
        assert_eq!(h.name(FIRE_POS), AIR);
        let packets = h.packets();
        assert!(packets.iter().any(|packet| matches!(
            packet,
            McpePacketData::LevelSoundEventPacket(sound)
                if sound.sound_event == SOUND_EXTINGUISH
        )));
        assert!(!punch(&mut h.world, floor, 1), "the fire is gone");
    }

    #[test]
    fn punching_soul_fire_from_the_side_puts_it_out() {
        let mut h = Harness::new();
        h.put(FIRE_POS - IVec3::Y, id("minecraft:soul_sand"));
        h.set(FIRE_POS, id(SOUL_FIRE));
        // Face 5 (east) of the cell west of the fire.
        assert!(punch(&mut h.world, FIRE_POS - IVec3::X, 5));
        assert_eq!(h.name(FIRE_POS), AIR);
    }

    #[test]
    fn soul_fire_stays_only_on_soul_blocks() {
        let mut h = Harness::new();
        for base in ["minecraft:soul_sand", "minecraft:soul_soil"] {
            h.put(FIRE_POS - IVec3::Y, id(base));
            h.set(FIRE_POS, id(SOUL_FIRE));
            h.tick(500);
            assert_eq!(h.name(FIRE_POS), SOUL_FIRE, "{base}");
            assert_eq!(age(&h, FIRE_POS), Some(StateValue::Int(0)), "{base}");
        }
        // The base turns to stone: the soul fire goes out.
        h.set(FIRE_POS - IVec3::Y, id("minecraft:stone"));
        h.tick(1);
        assert_eq!(h.name(FIRE_POS), AIR);
        // Soul fire set on stone goes out at once.
        h.set(FIRE_POS, id(SOUL_FIRE));
        assert_eq!(h.name(FIRE_POS), AIR);
    }

    #[test]
    fn fire_over_soul_blocks_becomes_soul_fire() {
        let mut h = Harness::new();
        h.put(FIRE_POS - IVec3::Y, id("minecraft:soul_soil"));
        h.set(FIRE_POS, id(FIRE));
        assert_eq!(h.name(FIRE_POS), SOUL_FIRE, "placed over soul soil");
        // A burning fire whose base becomes soul sand turns three ticks
        // later.
        let other = FIRE_POS + IVec3::new(4, 0, 0);
        h.set(other, id(FIRE));
        h.tick(2);
        h.set(other - IVec3::Y, id("minecraft:soul_sand"));
        assert_eq!(h.name(other), FIRE);
        h.tick(SOUL_DELAY - 1);
        assert_eq!(h.name(other), FIRE);
        h.tick(1);
        assert_eq!(h.name(other), SOUL_FIRE);
    }

    #[test]
    fn fire_needs_a_full_top_or_a_flammable_neighbour() {
        let mut h = Harness::new();
        let registry = BlockRegistry::vanilla();
        let fire = registry.get_by_name(FIRE).unwrap();
        let soul = registry.get_by_name(SOUL_FIRE).unwrap();
        let high = FIRE_POS + IVec3::Y * 3;
        assert!(can_survive(&h.world, FIRE_POS, fire.default_state_id, fire));
        assert!(!can_survive(
            &h.world,
            FIRE_POS,
            soul.default_state_id,
            soul
        ));
        assert!(!can_survive(&h.world, high, fire.default_state_id, fire));
        h.put(high + IVec3::X, id("minecraft:oak_log"));
        assert!(can_survive(&h.world, high, fire.default_state_id, fire));
        h.put(high - IVec3::Y, id("minecraft:campfire"));
        assert!(!can_survive(&h.world, high, fire.default_state_id, fire));
        h.put(high - IVec3::Y, id("minecraft:soul_soil"));
        assert!(can_survive(&h.world, high, soul.default_state_id, soul));
    }

    /// Lava ticks at `lava` until fire appears; the cells that got fire.
    fn lava_fires(h: &mut Harness, lava: IVec3, ticks: usize) -> Vec<IVec3> {
        let mut lit = Vec::new();
        for _ in 0..ticks {
            lava_ignites(&mut h.world, lava);
            for dx in -2..=2 {
                for dy in -1..=2 {
                    for dz in -2..=2 {
                        let pos = lava + IVec3::new(dx, dy, dz);
                        if h.name(pos) == FIRE && !lit.contains(&pos) {
                            lit.push(pos);
                        }
                    }
                }
            }
        }
        lit
    }

    #[test]
    fn lava_sets_fire_next_to_a_log_and_planks() {
        for (block, offset) in [
            ("minecraft:oak_log", IVec3::X),
            ("minecraft:oak_planks", IVec3::new(-1, 1, 0)),
        ] {
            let mut h = Harness::new();
            let lava = FIRE_POS;
            h.put(lava, id("minecraft:lava"));
            let fuel = lava + offset;
            h.put(fuel, id(block));
            let lit = lava_fires(&mut h, lava, 300);
            assert!(!lit.is_empty(), "lava never lit next to {block}");
            assert!(
                lit.iter()
                    .all(|pos| NEIGHBOURS.iter().any(|offset| *pos + *offset == fuel)),
                "{block}: fire away from the fuel: {lit:?}"
            );
        }
    }

    #[test]
    fn lava_sets_no_fire_next_to_coal_without_fire_ticks_or_under_a_block() {
        let lava = FIRE_POS;
        // Coal is not lava-flammable.
        let mut h = Harness::new();
        h.put(lava, id("minecraft:lava"));
        h.put(lava + IVec3::X, id("minecraft:coal_block"));
        assert!(lava_fires(&mut h, lava, 300).is_empty(), "coal");
        // Without doFireTick.
        let mut h = Harness::new();
        h.world
            .resource_mut::<crate::world::rules::GameRules>()
            .set_value(
                "doFireTick",
                unastar_data::game_rules::GameRuleValue::Bool(false),
            )
            .unwrap();
        h.put(lava, id("minecraft:lava"));
        h.put(lava + IVec3::X, id("minecraft:oak_log"));
        assert!(lava_fires(&mut h, lava, 300).is_empty(), "doFireTick off");
        // Under stone.
        let mut h = Harness::new();
        h.put(lava, id("minecraft:lava"));
        h.put(lava + IVec3::X, id("minecraft:oak_log"));
        h.put(lava + IVec3::Y, id("minecraft:stone"));
        assert!(lava_fires(&mut h, lava, 300).is_empty(), "covered lava");
    }

    #[test]
    fn flammability_table_covers_the_common_blocks() {
        let registry = BlockRegistry::vanilla();
        let of = |name: &str| flammability(registry.get_by_name(name).unwrap());
        assert_eq!(of("minecraft:oak_planks"), Some(PLANKS));
        assert_eq!(of("minecraft:fence_gate"), Some(PLANKS));
        assert_eq!(of("minecraft:pale_oak_stairs"), Some(PLANKS));
        assert_eq!(of("minecraft:stripped_bamboo_block"), Some(PLANKS));
        assert_eq!(of("minecraft:stripped_cherry_log"), Some(LOG));
        assert_eq!(of("minecraft:mangrove_leaves"), Some(LEAVES));
        assert_eq!(of("minecraft:red_carpet"), Some(SHELF));
        assert_eq!(of("minecraft:spruce_shelf"), Some(SHELF));
        assert_eq!(of("minecraft:poppy"), Some(FLOWER));
        assert_eq!(of("minecraft:tall_grass"), Some(PLANT));
        assert_eq!(of("minecraft:tnt"), Some(TNT));
        for inert in [
            "minecraft:stone",
            "minecraft:crimson_planks",
            "minecraft:warped_stem",
            "minecraft:crimson_shelf",
            "minecraft:wooden_door",
            "minecraft:netherrack",
            "minecraft:air",
        ] {
            assert_eq!(of(inert), None, "{inert}");
        }
    }

    /// Every behaviour pack block with `minecraft:flammable` burns as the
    /// pack says, and pack blocks without it do not burn.
    #[test]
    fn flammability_matches_the_behaviour_pack() {
        let registry = BlockRegistry::vanilla();
        let pack_dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../unastar-data/data/vanilla_bp/behavior_pack/blocks"
        );
        let mut flammable = 0;
        for file in std::fs::read_dir(pack_dir).expect("vanilla behaviour pack blocks") {
            let path = file.unwrap().path();
            if path.extension().is_none_or(|extension| extension != "json") {
                continue;
            }
            let definition: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            let block = &definition["minecraft:block"];
            let name = block["description"]["identifier"].as_str().unwrap();
            let entry = registry.get_by_name(name).unwrap();
            let component = &block["components"]["minecraft:flammable"];
            let expected = component.as_object().map(|flammable| {
                let int = |key: &str, default: i64| {
                    flammable
                        .get(key)
                        .and_then(|v| v.as_i64())
                        .unwrap_or(default) as i32
                };
                Flammable::new(
                    int("catch_chance_modifier", 5),
                    int("destroy_chance_modifier", 20),
                    flammable.get("lava_flammable").and_then(|v| v.as_str()) == Some("always"),
                )
            });
            flammable += usize::from(expected.is_some());
            assert_eq!(flammability(entry), expected, "{name}");
        }
        assert_eq!(flammable, 49);
    }

    /// Fire against the BDS 1.26.51 capture (`testdata`): doFireTick on,
    /// peaceful unless named. Each fire scenario there is one random trial
    /// (the netherrack and bare stone fires are eight fires of one run), so
    /// these tests check the rules the capture shows every time and keep
    /// the statistical bounds loose, over a few seeded runs of ours.
    mod capture {
        use serde_json::Value;

        use super::*;
        use crate::server::game::block_behaviour::BlockRandom;
        use crate::world::rules::Difficulty;

        const CAPTURE: &str = include_str!("testdata/bds_1_26_51_redstone.json");
        /// The capture's floor is at y = −61.
        const LIFT: i32 = FLOOR + 61;
        const SEEDS: [u64; 3] = [1, 2, 3];

        fn step(name: &str) -> Value {
            let capture: Value = serde_json::from_str(CAPTURE).unwrap();
            capture["fire"]
                .as_array()
                .unwrap()
                .iter()
                .find(|step| step["step"] == name)
                .unwrap_or_else(|| panic!("capture step {name}"))
                .clone()
        }

        fn watched(step: &Value) -> u64 {
            step["watched_ticks"].as_u64().unwrap()
        }

        /// Our cell for the captured watched cell `cell`.
        fn cell(step: &Value, cell: &str) -> IVec3 {
            let p = &step["positions"][cell];
            let at = |i: usize| p[i].as_i64().unwrap() as i32;
            IVec3::new(at(0), at(1) + LIFT, at(2))
        }

        /// The captured changes of watched cell `cell`: `(tick, state)`.
        fn history(step: &Value, cell: &str) -> Vec<(u64, String)> {
            step["changes"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|change| {
                    let state = change["cells"][cell].as_str()?;
                    Some((change["tick"].as_u64().unwrap(), state.to_owned()))
                })
                .collect()
        }

        /// Fire age in block state notation (`minecraft:fire[age=3]`).
        fn notation_age(state: &str) -> Option<i32> {
            state
                .strip_prefix("minecraft:fire[age=")?
                .strip_suffix(']')?
                .parse()
                .ok()
        }

        fn seeded(seed: u64) -> Harness {
            let mut h = Harness::new();
            h.world.insert_resource(BlockRandom::seeded(seed));
            h
        }

        /// Our state of the fire at `pos` as the capture writes it.
        fn notation(h: &Harness, pos: IVec3) -> String {
            match age(h, pos) {
                Some(StateValue::Int(age)) if h.name(pos) == FIRE => {
                    format!("minecraft:fire[age={age}]")
                }
                _ => h.name(pos).to_owned(),
            }
        }

        /// Ages along a history step by one and the fire goes out from age
        /// 3; age changes after the first tick are at least a fire tick
        /// (30) apart. Returns the tick it went out.
        fn check_bare_stone_history(history: &[(u64, String)], what: &str) -> u64 {
            let (out, burning) = history.split_last().expect(what);
            assert_eq!(out.1, AIR, "{what}: {history:?}");
            let ages: Vec<i32> = burning
                .iter()
                .map(|(_, state)| notation_age(state).expect(what))
                .collect();
            assert_eq!(ages, (0..=3).collect::<Vec<_>>(), "{what}: {history:?}");
            let ticks: Vec<u64> = history.iter().map(|(tick, _)| *tick).collect();
            for pair in ticks.windows(2).filter(|pair| pair[0] > 1) {
                assert!(pair[1] - pair[0] >= 30, "{what}: {history:?}");
            }
            out.0
        }

        #[test]
        fn fire_on_netherrack_stays_at_age_zero_like_the_capture() {
            let capture = step("fires_set");
            assert_eq!(
                history(&capture, "netherrack_fire"),
                [(0, "minecraft:fire[age=0]".to_owned())]
            );
            let pos = cell(&capture, "netherrack_fire");
            for seed in SEEDS {
                let mut h = seeded(seed);
                h.put(pos - IVec3::Y, id("minecraft:netherrack"));
                h.set(pos, id(FIRE));
                for tick in 1..=watched(&capture) {
                    h.tick(1);
                    assert_eq!(notation(&h, pos), "minecraft:fire[age=0]", "tick {tick}");
                }
            }
        }

        #[test]
        fn bare_stone_fires_age_and_go_out_like_the_capture() {
            let capture = step("fires_set");
            let names: Vec<String> = (1..=8).map(|i| format!("stone_fire_{i}")).collect();
            let captured: Vec<u64> = names
                .iter()
                .map(|name| check_bare_stone_history(&history(&capture, name), name))
                .collect();
            let captured_first = names
                .iter()
                .filter(|name| history(&capture, name).iter().any(|(tick, _)| *tick == 1))
                .count();
            assert!(captured_first > 0);
            let mut ours = Vec::new();
            let mut first_tick_aged = 0;
            for seed in SEEDS {
                let mut h = seeded(seed);
                let cells: Vec<IVec3> = names.iter().map(|name| cell(&capture, name)).collect();
                let mut histories: Vec<Vec<(u64, String)>> = Vec::new();
                for pos in &cells {
                    h.set(*pos, id(FIRE));
                    histories.push(vec![(0, notation(&h, *pos))]);
                }
                for tick in 1..=watched(&capture) {
                    h.tick(1);
                    for (pos, history) in cells.iter().zip(&mut histories) {
                        let now = notation(&h, *pos);
                        if history.last().unwrap().1 != now {
                            history.push((tick, now));
                        }
                    }
                }
                for history in &histories {
                    first_tick_aged += usize::from(history.iter().any(|(tick, _)| *tick == 1));
                    ours.push(check_bare_stone_history(history, &format!("seed {seed}")));
                }
            }
            // The first fire tick comes one tick after the fire is set.
            assert!(first_tick_aged > 0);
            // The means agree within a third (eight captured trials only).
            let mean = |ticks: &[u64]| ticks.iter().sum::<u64>() as f64 / ticks.len() as f64;
            let (captured_mean, our_mean) = (mean(&captured), mean(&ours));
            assert!(
                (our_mean - captured_mean).abs() <= captured_mean / 3.0,
                "ours {ours:?} captured {captured:?}"
            );
            // Four age steps take the first tick and three more fire ticks:
            // no fire, captured or ours, goes out sooner.
            let soonest = 1 + 3 * TICK_DELAY;
            assert!(captured.iter().chain(&ours).all(|tick| *tick >= soonest));
        }

        /// One fire beside a four-high oak planks column, as the capture
        /// measures it.
        #[derive(Debug)]
        struct ColumnRun {
            /// Fire reached a cell that held neither the fire nor a plank.
            spread: bool,
            planks_left: usize,
            /// The last tick a watched cell changed.
            last_change: u64,
            /// The most fire cells seen at once.
            most_fires: usize,
            /// Fire cells at the end of the watched ticks.
            fires_at_end: usize,
        }

        fn captured_column(step: &Value) -> ColumnRun {
            let samples = step["samples"].as_array().unwrap();
            let planks: Vec<&str> = samples[0]["oak_planks"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| p.as_str().unwrap())
                .collect();
            let is_cell = |name: &str| name.split(',').count() == 3;
            let changes = step["changes"].as_array().unwrap();
            let spread = changes.iter().any(|change| {
                change["cells"]
                    .as_object()
                    .unwrap()
                    .iter()
                    .any(|(name, state)| {
                        is_cell(name)
                            && change["tick"].as_u64().unwrap() > 0
                            && !planks.contains(&name.as_str())
                            && notation_age(state.as_str().unwrap()).is_some()
                    })
            });
            let last_change = changes
                .iter()
                .filter(|change| {
                    change["cells"]
                        .as_object()
                        .unwrap()
                        .keys()
                        .any(|name| is_cell(name))
                })
                .map(|change| change["tick"].as_u64().unwrap())
                .max()
                .unwrap();
            let fires = |sample: &Value| {
                sample["fire"].as_array().map_or(0, |fires| {
                    fires
                        .iter()
                        .filter(|name| is_cell(name.as_str().unwrap()))
                        .count()
                })
            };
            let last = samples.last().unwrap();
            ColumnRun {
                spread,
                planks_left: last["oak_planks"].as_array().map_or(0, Vec::len),
                last_change,
                most_fires: samples.iter().map(fires).max().unwrap(),
                fires_at_end: fires(last),
            }
        }

        fn simulated_column(step: &Value, seed: u64, difficulty: Difficulty) -> ColumnRun {
            let mut h = seeded(seed);
            h.world.insert_resource(difficulty);
            let samples = step["samples"].as_array().unwrap();
            let at = |name: &str| {
                let p: Vec<i32> = name.split(',').map(|v| v.parse().unwrap()).collect();
                IVec3::new(p[0], p[1] + LIFT, p[2])
            };
            let planks: Vec<IVec3> = samples[0]["oak_planks"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| at(p.as_str().unwrap()))
                .collect();
            let start = at("13,-60,0");
            let cells: Vec<IVec3> = (11..=13)
                .flat_map(|x| (-60..=-55).flat_map(move |y| (-1..=1).map(move |z| (x, y, z))))
                .map(|(x, y, z)| IVec3::new(x, y + LIFT, z))
                .collect();
            for pos in &planks {
                h.put(*pos, id("minecraft:oak_planks"));
            }
            h.set(start, id(FIRE));
            let mut states: Vec<String> = cells.iter().map(|pos| notation(&h, *pos)).collect();
            let mut run = ColumnRun {
                spread: false,
                planks_left: 0,
                last_change: 0,
                most_fires: 1,
                fires_at_end: 0,
            };
            for tick in 1..=watched(step) {
                h.tick(1);
                let mut fires = 0;
                for (pos, state) in cells.iter().zip(&mut states) {
                    let now = notation(&h, *pos);
                    let burning = now.starts_with(FIRE);
                    fires += usize::from(burning);
                    run.spread |= burning && *pos != start && !planks.contains(pos);
                    if *state != now {
                        run.last_change = tick;
                        *state = now;
                    }
                }
                run.most_fires = run.most_fires.max(fires);
                run.fires_at_end = fires;
            }
            run.planks_left = planks
                .iter()
                .filter(|pos| h.name(**pos) == "minecraft:oak_planks")
                .count();
            run
        }

        #[test]
        fn fire_beside_a_planks_column_burns_like_the_capture() {
            // Peaceful: spread, one plank left, everything out by tick 557.
            // Normal: spread to up to 7 cells, every plank gone, everything
            // out by tick 909. (A second peaceful trial outside the fixture
            // did not spread and was out by about tick 400.) Next to planks
            // the spread chance is 1 on every difficulty (integer division),
            // so with the same seeds our peaceful and normal runs are the
            // same; one captured trial each cannot tell the two apart.
            for (name, difficulty) in [
                ("fires_set", Difficulty::Peaceful),
                ("planks_fire_normal_difficulty", Difficulty::Normal),
            ] {
                let capture = step(name);
                let captured = captured_column(&capture);
                assert!(captured.spread, "{name}: {captured:?}");
                assert!(captured.last_change < watched(&capture), "{name}");
                let runs: Vec<ColumnRun> = (1..=4)
                    .map(|seed| simulated_column(&capture, seed, difficulty))
                    .collect();
                // Every trial burned out within the watched ticks.
                assert_eq!(captured.fires_at_end, 0, "{name}: {captured:?}");
                for run in &runs {
                    assert_eq!(run.fires_at_end, 0, "{name}: {runs:?}");
                }
                // Fire spreads in some trials and burns planks away.
                assert!(runs.iter().any(|run| run.spread), "{name}: {runs:?}");
                assert!(
                    runs.iter()
                        .any(|run| run.planks_left <= captured.planks_left)
                );
                // A fire this size never grows past a handful of cells.
                for run in &runs {
                    assert!(run.most_fires <= 3 * captured.most_fires.max(3), "{runs:?}");
                }
            }
        }

        #[test]
        fn punching_puts_fire_out_with_the_captured_sound() {
            for name in ["punch_netherrack", "punch_stone"] {
                let capture = step(name);
                let under = &capture["before"]["under"];
                let fire = cell(&capture, "fire");
                let mut h = Harness::new();
                h.put(fire - IVec3::Y, id(under.as_str().unwrap()));
                h.set(fire, id(FIRE));
                h.packets();
                assert!(punch(&mut h.world, fire - IVec3::Y, 1));
                assert_eq!(h.name(fire), capture["after"]["fire"].as_str().unwrap());
                assert_eq!(
                    h.name(fire - IVec3::Y),
                    capture["after"]["under"].as_str().unwrap()
                );
                let sounds: Vec<(String, [f32; 3], i32)> = h
                    .packets()
                    .into_iter()
                    .filter_map(|packet| match packet {
                        McpePacketData::LevelSoundEventPacket(sound) => Some((
                            sound.sound_event.to_string(),
                            [sound.position.x, sound.position.y, sound.position.z],
                            sound.data,
                        )),
                        _ => None,
                    })
                    .collect();
                let expected: Vec<(String, [f32; 3], i32)> = capture["sounds"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|sound| {
                        let p = &sound["position"];
                        let at = |i: usize| p[i].as_f64().unwrap() as f32;
                        (
                            sound["sound"].as_str().unwrap().to_owned(),
                            [at(0), at(1) + LIFT as f32, at(2)],
                            sound["data"].as_i64().unwrap() as i32,
                        )
                    })
                    .collect();
                assert_eq!(sounds, expected, "{name}");
            }
        }
    }
}
