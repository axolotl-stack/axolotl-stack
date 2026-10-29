//! Crops, stems, nether wart and farmland, with the growth rules and
//! numbers BDS 1.26.51 uses.
//!
//! - **Crops** (wheat, carrots, potatoes, beetroot; `growth` 0..=7) and
//!   **stems** (melon and pumpkin stems) stay only on farmland. Survival is
//!   the soil check alone: BDS re-checks a crop only when the block below
//!   changes, and darkness alone never pops one.
//! - **Growth** (random tick): the crop needs a brightness of at least 9
//!   at its own cell (sky light after night darkening; block light is not
//!   modelled by the server, so a torch does not let crops grow at night).
//!   Its growth speed `f` starts at 1 and adds, for each of the 3×3 blocks
//!   below it, 3 for moist farmland, 1 for dry farmland and 0 for anything
//!   else, the eight around the centre counting a quarter. `f` halves when
//!   the same crop grows on a diagonal, or both on the north/south and on
//!   the east/west side. A tick grows the crop one stage with chance
//!   `1 / (floor(25 / f) + 1)`. Beetroot grows like the others.
//! - **Stems** grow with the same light, speed and chance; a fully grown
//!   stem not yet pointing at a fruit picks one of the four sides at
//!   random and, when that cell is air above a `dirt`-tagged block, puts
//!   its fruit (melon block or pumpkin) there and points
//!   `facing_direction` at it. When the block it points at changes, the
//!   stem points back at nothing (core neighbour update).
//! - **Bone meal** grows a crop or stem by 2 to 4 stages (capped at 7) and
//!   shows the crop-growth particles; a fully grown one does not use it.
//! - **Nether wart** (`age` 0..=3) stays only on soul sand and grows one
//!   stage on one random tick in ten, without any light requirement. Bone
//!   meal does not grow it.
//! - **Farmland** (`moisturized_amount` 0..=7) becomes fully moist on a
//!   random tick when water is within 4 blocks horizontally, at its level
//!   or one above; otherwise it dries one step, and dry farmland without a
//!   crop or stem on it turns into dirt. Rain does not moisten it (the
//!   server has no weather), and only water in the main block layer counts
//!   (waterlogged blocks are not seen). A block of solid material above it
//!   turns it into dirt one tick after the change; a fall onto it can
//!   trample it ([`trample`]). The crop on farmland that turned into dirt
//!   then pops with its drops.
//! - **Torchflower and pitcher crops** share the farmland rule (the upper
//!   half of a pitcher crop stays on its lower half); their growth, which
//!   turns them into another block or a two-block crop, is not modelled:
//!   random ticks and bone meal leave them as they are.

use std::ops::RangeInclusive;

use bevy_ecs::prelude::*;
use glam::IVec3;
use rand::Rng;

use super::{
    Family, air, bit, centre, family, family_of, int, name_at, random, schedule_tick, with,
};
use crate::entity::systems::BlockSource;
use crate::registry::block::{BlockEntry, LiquidKind, StateValue};
use crate::server::game::blocks::set_block;
use crate::server::game::placement::Horizontal;
use crate::server::game::types::BlockRegistryResource;
use crate::world::clock::sky_brightness;
use crate::world::rules::GameRules;

/// Crop and stem growth stage.
pub const GROWTH: &str = "growth";
/// Nether wart growth stage.
pub const AGE: &str = "age";
/// Farmland moisture.
pub const MOISTURE: &str = "moisturized_amount";
/// The side a stem's fruit is on (0 none, 2 north, 3 south, 4 west, 5
/// east).
pub const FACING: &str = "facing_direction";
const UPPER: &str = "upper_block_bit";

/// Last growth stage of crops and stems.
pub const MAX_GROWTH: i32 = 7;
/// Last growth stage of nether wart.
pub const MAX_WART_AGE: i32 = 3;
/// Moisture of farmland near water.
pub const MAX_MOISTURE: i32 = 7;
/// Brightness a crop or stem needs at its cell to grow.
pub const MIN_GROWTH_LIGHT: u8 = 9;
/// Growth points a stage takes; the growth speed divides them.
const GROWTH_POINTS: f32 = 25.0;
/// Stages one bone meal adds to a crop or stem.
const BONE_MEAL_GROWTH: RangeInclusive<i32> = 2..=4;
/// Nether wart grows on one random tick in this many.
const WART_GROWTH_ONE_IN: u32 = 10;
/// Horizontal reach of the farmland water search.
const HYDRATION_RADIUS: i32 = 4;
/// Fall distance that never tramples farmland (the trample chance is the
/// fall distance beyond it).
const TRAMPLE_SAFE_FALL: f32 = 0.5;
/// `LevelEvent` id of the crop-growth (bone meal) particles (provisional:
/// public protocol documentation of the level event ids).
pub const LEVEL_EVENT_PARTICLES_CROP_GROWTH: i32 = 2005;

/// Whether `state` is a state of the block `entry`.
pub(super) fn is_state_of(entry: &BlockEntry, state: u32) -> bool {
    (entry.min_state_id..=entry.max_state_id).contains(&state)
}

/// Whether the crop `entry` grows by the farmland crop rules (see the
/// module docs for torchflower and pitcher crops).
fn grows(entry: &BlockEntry) -> bool {
    entry.string_id != "minecraft:torchflower_crop"
        && entry.state_value(entry.default_state_id, UPPER).is_none()
}

/// Moisture of the farmland at `pos`, `None` when it is not farmland.
fn farmland_moisture(blocks: &impl BlockSource, pos: IVec3) -> Option<i32> {
    let state = blocks.block_at(pos)?;
    let entry = blocks.registry().get_by_runtime_id(state)?;
    if entry.string_id != "minecraft:farmland" {
        return None;
    }
    int(entry, state, MOISTURE)
}

/// Crops and stems need farmland below; the upper half of a pitcher crop
/// needs its lower half.
pub fn crop_can_survive(
    blocks: &impl BlockSource,
    pos: IVec3,
    state: u32,
    entry: &BlockEntry,
) -> bool {
    if bit(entry, state, UPPER) {
        return blocks
            .block_at(pos - IVec3::Y)
            .is_some_and(|below| is_state_of(entry, below) && !bit(entry, below, UPPER));
    }
    farmland_moisture(blocks, pos - IVec3::Y).is_some()
}

/// Nether wart needs soul sand below.
pub fn nether_wart_can_survive(blocks: &impl BlockSource, pos: IVec3) -> bool {
    name_at(blocks, pos - IVec3::Y) == Some("minecraft:soul_sand")
}

/// Growth speed of the crop or stem `entry` at `pos` (see the module
/// docs).
pub fn growth_speed(blocks: &impl BlockSource, pos: IVec3, entry: &BlockEntry) -> f32 {
    let mut speed = 1.0;
    for dx in -1..=1 {
        for dz in -1..=1 {
            let mut score = match farmland_moisture(blocks, pos + IVec3::new(dx, -1, dz)) {
                None => 0.0,
                Some(0) => 1.0,
                Some(_) => 3.0,
            };
            if dx != 0 || dz != 0 {
                score /= 4.0;
            }
            speed += score;
        }
    }
    let same = |dx: i32, dz: i32| {
        blocks
            .block_at(pos + IVec3::new(dx, 0, dz))
            .is_some_and(|state| is_state_of(entry, state))
    };
    let north_south = same(0, -1) || same(0, 1);
    let west_east = same(-1, 0) || same(1, 0);
    let diagonal = same(-1, -1) || same(1, -1) || same(1, 1) || same(-1, 1);
    if diagonal || (north_south && west_east) {
        speed /= 2.0;
    }
    speed
}

/// One growth roll at growth speed `speed`.
fn growth_roll(world: &mut World, speed: f32) -> bool {
    let bound = (GROWTH_POINTS / speed) as u32 + 1;
    random(world).gen_range(0..bound) == 0
}

/// Whether the cell at `pos` is bright enough for crops to grow.
fn bright_enough(world: &World, pos: IVec3) -> bool {
    sky_brightness(world, pos) >= MIN_GROWTH_LIGHT
}

/// Send the viewers of `pos` the crop-growth particles.
pub fn growth_particles(world: &World, pos: IVec3) {
    crate::entity::mob::net::sound::level_event(
        world,
        centre(pos),
        LEVEL_EVENT_PARTICLES_CROP_GROWTH,
        0,
    );
}

fn registry(world: &World) -> std::sync::Arc<crate::registry::BlockRegistry> {
    world.resource::<BlockRegistryResource>().0.clone()
}

pub fn crop_random_tick(world: &mut World, pos: IVec3, state: u32) {
    let registry = registry(world);
    let Some(entry) = registry.get_by_runtime_id(state) else {
        return;
    };
    if !grows(entry) || !bright_enough(world, pos) {
        return;
    }
    let Some(growth) = int(entry, state, GROWTH) else {
        return;
    };
    if growth >= MAX_GROWTH {
        return;
    }
    let speed = growth_speed(world, pos, entry);
    if growth_roll(world, speed) {
        let grown = with(entry, state, GROWTH, StateValue::Int(growth + 1));
        set_block(world, pos, grown, false);
    }
}

/// The fruit a stem grows.
fn fruit(stem: &BlockEntry) -> Option<&'static str> {
    match stem.string_id.as_str() {
        "minecraft:melon_stem" => Some("minecraft:melon_block"),
        "minecraft:pumpkin_stem" => Some("minecraft:pumpkin"),
        _ => None,
    }
}

pub fn stem_random_tick(world: &mut World, pos: IVec3, state: u32) {
    let registry = registry(world);
    let Some(entry) = registry.get_by_runtime_id(state) else {
        return;
    };
    if !bright_enough(world, pos) {
        return;
    }
    let Some(growth) = int(entry, state, GROWTH) else {
        return;
    };
    let speed = growth_speed(world, pos, entry);
    if !growth_roll(world, speed) {
        return;
    }
    if growth < MAX_GROWTH {
        let grown = with(entry, state, GROWTH, StateValue::Int(growth + 1));
        set_block(world, pos, grown, false);
        return;
    }
    if int(entry, state, FACING) != Some(0) {
        return;
    }
    let facing = random(world).gen_range(0..4) + 2;
    let Some(side) = Horizontal::from_facing(facing) else {
        return;
    };
    let target = pos + side.offset();
    if world.block_at(target) != Some(air(&registry)) {
        return;
    }
    let on_dirt = world
        .block_at(target - IVec3::Y)
        .and_then(|below| registry.get_by_runtime_id(below))
        .is_some_and(|below| below.has_tag("dirt"));
    let Some(fruit) = fruit(entry).and_then(|name| registry.default_state_id_by_name(name)) else {
        return;
    };
    if !on_dirt {
        return;
    }
    set_block(world, target, fruit, false);
    let attached = with(entry, state, FACING, StateValue::Int(facing));
    set_block(world, pos, attached, false);
}

pub fn nether_wart_random_tick(world: &mut World, pos: IVec3, state: u32) {
    let registry = registry(world);
    let Some(entry) = registry.get_by_runtime_id(state) else {
        return;
    };
    let Some(age) = int(entry, state, AGE) else {
        return;
    };
    if age < MAX_WART_AGE && random(world).gen_range(0..WART_GROWTH_ONE_IN) == 0 {
        set_block(
            world,
            pos,
            with(entry, state, AGE, StateValue::Int(age + 1)),
            false,
        );
    }
}

/// Whether water is in the farmland hydration box around `pos`.
fn near_water(blocks: &impl BlockSource, pos: IVec3) -> bool {
    let registry = blocks.registry();
    (0..=1).any(|dy| {
        (-HYDRATION_RADIUS..=HYDRATION_RADIUS).any(|dx| {
            (-HYDRATION_RADIUS..=HYDRATION_RADIUS).any(|dz| {
                blocks
                    .block_at(pos + IVec3::new(dx, dy, dz))
                    .and_then(|state| registry.liquid_state(state))
                    .is_some_and(|liquid| liquid.kind == LiquidKind::Water)
            })
        })
    })
}

/// Turn the farmland at `pos` into dirt.
fn to_dirt(world: &mut World, pos: IVec3) {
    let Some(dirt) = registry(world).default_state_id_by_name("minecraft:dirt") else {
        return;
    };
    set_block(world, pos, dirt, false);
}

pub fn farmland_random_tick(world: &mut World, pos: IVec3, state: u32) {
    let registry = registry(world);
    let Some(entry) = registry.get_by_runtime_id(state) else {
        return;
    };
    let Some(moisture) = int(entry, state, MOISTURE) else {
        return;
    };
    if near_water(world, pos) {
        if moisture != MAX_MOISTURE {
            let moist = with(entry, state, MOISTURE, StateValue::Int(MAX_MOISTURE));
            set_block(world, pos, moist, false);
        }
        return;
    }
    if moisture > 0 {
        let drier = with(entry, state, MOISTURE, StateValue::Int(moisture - 1));
        set_block(world, pos, drier, false);
        return;
    }
    let planted = world
        .block_at(pos + IVec3::Y)
        .is_some_and(|above| matches!(family_of(&registry, above), Family::Crop | Family::Stem));
    if !planted {
        to_dirt(world, pos);
    }
}

/// Whether a block of solid material covers the farmland at `pos`.
fn covered(blocks: &impl BlockSource, pos: IVec3) -> bool {
    blocks
        .block_at(pos + IVec3::Y)
        .and_then(|above| blocks.registry().get_by_runtime_id(above))
        .is_some_and(super::plants::solid_material)
}

/// A neighbour of the farmland at `pos` changed: when a block of solid
/// material is now above it, it turns into dirt on the next tick
/// ([`farmland_tick`]).
pub fn farmland_neighbour_changed(world: &mut World, pos: IVec3, _state: u32) {
    if covered(world, pos) {
        schedule_tick(world, pos, 1);
    }
}

/// The scheduled tick of farmland: dirt if it is still covered.
pub fn farmland_tick(world: &mut World, pos: IVec3, _state: u32) {
    if covered(world, pos) {
        to_dirt(world, pos);
    }
}

/// An entity landed on the farmland at `pos` after falling
/// `fall_distance` blocks: the farmland turns into dirt when a uniform
/// draw in `[0, 1)` is below `fall_distance − 0.5` (so falls of 1.5
/// blocks and more always trample) and, for anything but a player, the
/// `mobGriefing` game rule is on. Returns whether it was trampled; the
/// crop on it then pops with its drops.
///
/// Only mobs trample, and only those whose bounding box (width × height ×
/// depth) is at least 0.512 cubic blocks; players only when their
/// abilities allow mining. The caller passes only such entities (items,
/// projectiles and small mobs never reach this).
pub fn trample(world: &mut World, pos: IVec3, fall_distance: f32, is_player: bool) -> bool {
    let Some(state) = world.block_at(pos) else {
        return false;
    };
    if family(world, state) != Family::Farmland {
        return false;
    }
    if random(world).gen_range(0.0f32..1.0) >= fall_distance - TRAMPLE_SAFE_FALL {
        return false;
    }
    if !is_player && !world.resource::<GameRules>().mob_griefing {
        return false;
    }
    to_dirt(world, pos);
    true
}

/// Bone meal on the crop or stem `state` at `pos`: 2 to 4 stages more
/// (capped at the last one) and the crop-growth particles. Returns false
/// (the bone meal is not used) when it is fully grown or does not grow by
/// these rules.
pub fn fertilize(world: &mut World, pos: IVec3, state: u32) -> bool {
    let registry = registry(world);
    let Some(entry) = registry.get_by_runtime_id(state) else {
        return false;
    };
    if !grows(entry) {
        return false;
    }
    let Some(growth) = int(entry, state, GROWTH) else {
        return false;
    };
    if growth >= MAX_GROWTH {
        return false;
    }
    let step = random(world).gen_range(BONE_MEAL_GROWTH);
    let grown = with(
        entry,
        state,
        GROWTH,
        StateValue::Int((growth + step).min(MAX_GROWTH)),
    );
    set_block(world, pos, grown, false);
    growth_particles(world, pos);
    true
}

#[cfg(test)]
mod tests {
    use glam::IVec3;
    use jolyne::valentine::McpePacketData;

    use super::super::tests::{FLOOR, Harness, state};
    use super::*;
    use crate::entity::systems::physics::tests::TestBlocks;
    use crate::registry::BlockRegistry;
    use crate::world::rules::WorldTime;

    const NOON: u64 = 6_000;
    const MIDNIGHT: u64 = 18_000;

    fn clock(h: &mut Harness, ticks: u64) {
        h.world.insert_resource(WorldTime { ticks });
        h.world.insert_resource(GameRules::default());
    }

    fn farmland(moisture: i32) -> u32 {
        state(
            "minecraft:farmland",
            &[(MOISTURE, StateValue::Int(moisture))],
        )
    }

    fn crop(name: &str, growth: i32) -> u32 {
        state(name, &[(GROWTH, StateValue::Int(growth))])
    }

    fn growth_at(h: &Harness, pos: IVec3) -> Option<StateValue> {
        h.value(pos, GROWTH)
    }

    fn entry(name: &str) -> &'static BlockEntry {
        BlockRegistry::vanilla().get_by_name(name).unwrap()
    }

    #[test]
    fn crops_and_stems_stay_only_on_farmland() {
        let mut blocks = TestBlocks::default();
        let pos = IVec3::new(0, 1, 0);
        for name in [
            "minecraft:wheat",
            "minecraft:beetroot",
            "minecraft:melon_stem",
        ] {
            let block = entry(name);
            blocks.set(IVec3::ZERO, "minecraft:farmland");
            assert!(
                crop_can_survive(&blocks, pos, block.default_state_id, block),
                "{name}"
            );
            blocks.set(IVec3::ZERO, "minecraft:dirt");
            assert!(
                !crop_can_survive(&blocks, pos, block.default_state_id, block),
                "{name}"
            );
        }
    }

    #[test]
    fn pitcher_crop_upper_half_stays_on_its_lower_half() {
        let pitcher = entry("minecraft:pitcher_crop");
        let lower = state("minecraft:pitcher_crop", &[(UPPER, StateValue::Byte(0))]);
        let upper = state("minecraft:pitcher_crop", &[(UPPER, StateValue::Byte(1))]);
        let mut blocks = TestBlocks::default();
        blocks.set_state(IVec3::new(0, 1, 0), lower);
        assert!(crop_can_survive(
            &blocks,
            IVec3::new(0, 2, 0),
            upper,
            pitcher
        ));
        blocks.set(IVec3::new(0, 1, 0), "minecraft:air");
        assert!(!crop_can_survive(
            &blocks,
            IVec3::new(0, 2, 0),
            upper,
            pitcher
        ));
    }

    #[test]
    fn nether_wart_stays_only_on_soul_sand() {
        let mut blocks = TestBlocks::floor("minecraft:soul_sand", 0);
        assert!(nether_wart_can_survive(&blocks, IVec3::new(0, 1, 0)));
        blocks.set(IVec3::ZERO, "minecraft:soul_soil");
        assert!(!nether_wart_can_survive(&blocks, IVec3::new(0, 1, 0)));
    }

    #[test]
    fn growth_speed_counts_farmland_moisture_and_halves_for_crowding() {
        let wheat = entry("minecraft:wheat");
        let pos = IVec3::new(0, 1, 0);
        let mut blocks = TestBlocks::default();
        blocks.set_state(IVec3::ZERO, farmland(0));
        assert_eq!(growth_speed(&blocks, pos, wheat), 2.0);
        blocks.set_state(IVec3::ZERO, farmland(7));
        assert_eq!(growth_speed(&blocks, pos, wheat), 4.0);
        for dx in -1..=1 {
            for dz in -1..=1 {
                blocks.set_state(IVec3::new(dx, 0, dz), farmland(7));
            }
        }
        // 1 + 3 for the centre + 8 × 3 / 4 around it.
        assert_eq!(growth_speed(&blocks, pos, wheat), 10.0);
        // A row along one axis does not slow it down...
        blocks.set_state(pos + IVec3::Z, wheat.default_state_id);
        blocks.set_state(pos - IVec3::Z, wheat.default_state_id);
        assert_eq!(growth_speed(&blocks, pos, wheat), 10.0);
        // ...a crop of another kind beside it does not either...
        blocks.set(pos + IVec3::X, "minecraft:carrots");
        assert_eq!(growth_speed(&blocks, pos, wheat), 10.0);
        // ...but the same crop on both axes or on a diagonal halves it.
        blocks.set_state(pos + IVec3::X, wheat.default_state_id);
        assert_eq!(growth_speed(&blocks, pos, wheat), 5.0);
        blocks.set(pos + IVec3::X, "minecraft:air");
        blocks.set(pos + IVec3::Z, "minecraft:air");
        blocks.set(pos - IVec3::Z, "minecraft:air");
        blocks.set_state(pos + IVec3::new(1, 0, 1), wheat.default_state_id);
        assert_eq!(growth_speed(&blocks, pos, wheat), 5.0);
    }

    /// Wheat on moist farmland next to water at (2, 64, 2).
    fn wheat_field(h: &mut Harness) -> IVec3 {
        let soil = IVec3::new(2, FLOOR, 2);
        h.put(soil, farmland(7));
        h.put(soil + IVec3::X, state("minecraft:water", &[]));
        h.put(soil + IVec3::Y, crop("minecraft:wheat", 0));
        soil + IVec3::Y
    }

    #[test]
    fn wheat_grows_one_stage_at_a_time_to_the_last_in_daylight() {
        let mut h = Harness::new();
        clock(&mut h, NOON);
        let pos = wheat_field(&mut h);
        let mut last = 0;
        for _ in 0..500 {
            let state = h.state(pos);
            super::super::random_tick(&mut h.world, pos, state);
            let Some(StateValue::Int(growth)) = growth_at(&h, pos) else {
                panic!("wheat has a growth stage");
            };
            assert!(growth == last || growth == last + 1, "{last} -> {growth}");
            last = growth;
        }
        assert_eq!(last, MAX_GROWTH);
    }

    #[test]
    fn wheat_does_not_grow_at_night() {
        let mut h = Harness::new();
        clock(&mut h, MIDNIGHT);
        let pos = wheat_field(&mut h);
        for _ in 0..500 {
            let state = h.state(pos);
            crop_random_tick(&mut h.world, pos, state);
        }
        assert_eq!(growth_at(&h, pos), Some(StateValue::Int(0)));
    }

    #[test]
    fn wheat_pops_with_drops_when_its_farmland_turns_to_dirt() {
        let mut h = Harness::new();
        clock(&mut h, NOON);
        let pos = wheat_field(&mut h);
        h.set(pos - IVec3::Y, state("minecraft:dirt", &[]));
        assert_eq!(h.name(pos), "minecraft:wheat", "breaks one tick later");
        h.tick(1);
        assert_eq!(h.name(pos), "minecraft:air");
        assert!(
            h.drops()
                .iter()
                .any(|(item, _)| item == "minecraft:wheat_seeds")
        );
    }

    #[test]
    fn bone_meal_grows_two_to_four_stages_and_is_refused_when_grown() {
        let mut h = Harness::new();
        let pos = wheat_field(&mut h);
        h.packets();
        let state = h.state(pos);
        assert!(fertilize(&mut h.world, pos, state));
        let Some(StateValue::Int(growth)) = growth_at(&h, pos) else {
            panic!("wheat has a growth stage");
        };
        assert!((2..=4).contains(&growth), "grew to {growth}");
        assert!(h.packets().iter().any(|packet| matches!(
            packet,
            McpePacketData::LevelEventPacket(event)
                if event.event_id == LEVEL_EVENT_PARTICLES_CROP_GROWTH
        )));
        h.put(pos, crop("minecraft:wheat", MAX_GROWTH));
        let grown = h.state(pos);
        assert!(!fertilize(&mut h.world, pos, grown));
        assert_eq!(h.state(pos), grown);
    }

    #[test]
    fn grown_stem_puts_its_fruit_on_dirt_beside_it_and_points_at_it() {
        let mut h = Harness::new();
        clock(&mut h, NOON);
        let soil = IVec3::new(4, FLOOR, 4);
        h.put(soil, farmland(7));
        for side in super::super::HORIZONTAL {
            h.put(soil + side, state("minecraft:dirt", &[]));
        }
        let pos = soil + IVec3::Y;
        h.put(pos, crop("minecraft:melon_stem", MAX_GROWTH));
        for _ in 0..200 {
            let state = h.state(pos);
            stem_random_tick(&mut h.world, pos, state);
        }
        let Some(StateValue::Int(facing)) = h.value(pos, FACING) else {
            panic!("stem has a facing");
        };
        let side = Horizontal::from_facing(facing).expect("the stem points at a side");
        assert_eq!(h.name(pos + side.offset()), "minecraft:melon_block");
        let fruits = super::super::HORIZONTAL
            .iter()
            .filter(|offset| h.name(pos + **offset) == "minecraft:melon_block")
            .count();
        assert_eq!(fruits, 1, "one fruit at a time");

        h.set(pos + side.offset(), state("minecraft:air", &[]));
        assert_eq!(h.value(pos, FACING), Some(StateValue::Int(0)));
    }

    #[test]
    fn grown_stem_grows_no_fruit_without_dirt_beside_it() {
        let mut h = Harness::new();
        clock(&mut h, NOON);
        let soil = IVec3::new(4, FLOOR, 4);
        h.put(soil, farmland(7));
        let pos = soil + IVec3::Y;
        h.put(pos, crop("minecraft:pumpkin_stem", MAX_GROWTH));
        for _ in 0..200 {
            let state = h.state(pos);
            stem_random_tick(&mut h.world, pos, state);
        }
        assert_eq!(h.value(pos, FACING), Some(StateValue::Int(0)));
        for side in super::super::HORIZONTAL {
            assert_eq!(h.name(pos + side), "minecraft:air");
        }
    }

    #[test]
    fn nether_wart_grows_in_darkness() {
        let mut h = Harness::new();
        let soil = IVec3::new(2, FLOOR, 2);
        h.put(soil, state("minecraft:soul_sand", &[]));
        let pos = soil + IVec3::Y;
        h.put(pos, state("minecraft:nether_wart", &[]));
        h.put(pos + IVec3::Y, state("minecraft:stone", &[]));
        for _ in 0..300 {
            let state = h.state(pos);
            nether_wart_random_tick(&mut h.world, pos, state);
        }
        assert_eq!(h.value(pos, AGE), Some(StateValue::Int(MAX_WART_AGE)));
    }

    #[test]
    fn farmland_moistens_near_water() {
        let mut h = Harness::new();
        let pos = IVec3::new(0, FLOOR, 0);
        h.put(pos, farmland(0));
        h.put(pos + IVec3::new(4, 1, -4), state("minecraft:water", &[]));
        farmland_random_tick(&mut h.world, pos, farmland(0));
        assert_eq!(h.value(pos, MOISTURE), Some(StateValue::Int(MAX_MOISTURE)));
    }

    #[test]
    fn farmland_out_of_reach_of_water_dries_then_turns_to_dirt() {
        let mut h = Harness::new();
        let pos = IVec3::new(0, FLOOR, 0);
        h.put(pos, farmland(1));
        h.put(pos + IVec3::new(5, 0, 0), state("minecraft:water", &[]));
        h.put(pos + IVec3::new(0, 2, 0), state("minecraft:water", &[]));
        farmland_random_tick(&mut h.world, pos, farmland(1));
        assert_eq!(h.value(pos, MOISTURE), Some(StateValue::Int(0)));
        farmland_random_tick(&mut h.world, pos, farmland(0));
        assert_eq!(h.name(pos), "minecraft:dirt");
    }

    #[test]
    fn dry_farmland_with_a_crop_stays_farmland() {
        let mut h = Harness::new();
        let pos = IVec3::new(0, FLOOR, 0);
        h.put(pos, farmland(0));
        h.put(pos + IVec3::Y, crop("minecraft:carrots", 3));
        farmland_random_tick(&mut h.world, pos, farmland(0));
        assert_eq!(h.name(pos), "minecraft:farmland");
    }

    #[test]
    fn solid_block_above_turns_farmland_to_dirt_a_tick_later() {
        let mut h = Harness::new();
        let pos = IVec3::new(0, FLOOR, 0);
        h.put(pos, farmland(7));
        h.set(pos + IVec3::Y, state("minecraft:white_carpet", &[]));
        h.tick(1);
        assert_eq!(h.name(pos), "minecraft:farmland", "a carpet is not solid");
        h.set(pos + IVec3::Y, state("minecraft:oak_planks", &[]));
        assert_eq!(h.name(pos), "minecraft:farmland");
        h.tick(1);
        assert_eq!(h.name(pos), "minecraft:dirt");
    }

    #[test]
    fn falls_trample_farmland_by_distance_and_pop_the_crop() {
        let mut h = Harness::new();
        clock(&mut h, NOON);
        let pos = wheat_field(&mut h) - IVec3::Y;
        for _ in 0..100 {
            assert!(!trample(&mut h.world, pos, TRAMPLE_SAFE_FALL, true));
        }
        h.world.resource_mut::<GameRules>().mob_griefing = false;
        assert!(!trample(&mut h.world, pos, 3.0, false), "mobGriefing off");
        assert_eq!(h.name(pos), "minecraft:farmland");
        assert!(trample(&mut h.world, pos, 1.5, true));
        assert_eq!(h.name(pos), "minecraft:dirt");
        h.tick(1);
        assert_eq!(h.name(pos + IVec3::Y), "minecraft:air");
    }
}
