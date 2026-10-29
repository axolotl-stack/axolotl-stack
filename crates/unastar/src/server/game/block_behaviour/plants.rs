//! Plants: flowers, grass, saplings, double plants, sugar cane, cacti and
//! carpets.
//!
//! - **Single plants** ([`is_plant`]): blocks of the native plant material
//!   that stand on soil, each with a soil rule ([`Soil`]). Flowers (the
//!   `fertilize_area`-tagged ones plus torchflower, wither rose, eyeblossoms
//!   and the golden dandelion), saplings, short grass, ferns, bushes,
//!   firefly bushes, pink petals, wildflowers and sweet berry bushes need a
//!   `dirt`-tagged block below (grass, dirt, coarse and rooted dirt,
//!   podzol, mycelium, farmland, moss, pale moss, mud, muddy mangrove
//!   roots). Mangrove propagules, hanging and underwater plants, vines,
//!   dripleaves and blocks whose rules are not known are left out (no
//!   survival rule).
//! - **Double plants** ([`is_double_plant`]): `plant`-tagged blocks with
//!   `upper_block_bit` that are not crops (tall grass, large fern,
//!   sunflower, lilac, rose bush, peony, pitcher plant). The lower half
//!   needs a `dirt`-tagged block below and its upper half above; the upper
//!   half needs its lower half. A lone half breaks one tick after its
//!   partner goes; a player breaking either half removes both and gets the
//!   lower half's drops.
//! - **Sugar cane** (`reeds`, `age` 0..=15) stands on sugar cane, or on a
//!   `dirt`-tagged block other than farmland or a `sand`-tagged block with
//!   water (or frosted ice) beside that block, and no liquid above it. A
//!   random tick first re-checks this (breaking the cane with its drops),
//!   then, with air above and a column under 3 blocks, ages the cane by one
//!   on every tick; at age 15 it grows a new cane above and starts over.
//!   Bone meal fills the column up to 3 blocks at once.
//! - **Cacti** (`age` 0..=15) stand on cactus or a `sand`-tagged block,
//!   with no block of solid material and no lava beside them and no lava
//!   above. With air above, a random tick ages the cactus; at age 15 a
//!   column under 3 blocks grows a new cactus above. At age 7 a cactus
//!   flower may open above instead (10 % in a column under 3, 25 % in a
//!   full one), starting the age over; a full column stops ageing at 8.
//! - **Carpets** need any block but air below.
//!
//! Soil rules other than the `dirt`-tagged default, sugar cane and cacti
//! are provisional (Minecraft Wiki): dead bushes and dry grass also take
//! `sand`-tagged blocks and terracotta; nether roots, sprouts and fungi
//! take nylium and soul soil; wither roses netherrack, soul sand and soul
//! soil; bamboo saplings `sand`- and `gravel`-tagged blocks; mushrooms
//! mycelium, podzol and nylium, or any opaque full block (the light limit
//! on other blocks is not applied: the server does not model block
//! light); cactus flowers cactus, farmland or a block whose top centre
//! holds; leaf litter a block with a full top face.
//!
//! Neighbours in unloaded chunks never break a plant: an unloaded cell
//! beside sugar cane counts as water, beside a cactus as empty.

use bevy_ecs::prelude::*;
use glam::IVec3;
use rand::Rng;

use super::crops::{growth_particles, is_state_of};
use super::{
    SupportKind, air, bit, destroy_block, int, provides_support, random, schedule_tick, with,
};
use crate::entity::systems::BlockSource;
use crate::registry::block::{BlockEntry, StateValue};
use crate::server::game::blocks::set_block;
use crate::server::game::types::BlockRegistryResource;

const UPPER: &str = "upper_block_bit";
/// Sugar cane and cactus growth stage.
pub const AGE: &str = "age";
/// Last sugar cane and cactus growth stage.
pub const MAX_AGE: i32 = 15;
/// Tallest sugar cane or cactus column that grows by itself.
pub const MAX_HEIGHT: i32 = 3;
/// Age at which a cactus may open a flower.
const CACTUS_FLOWER_AGE: i32 = 7;
/// Age a full cactus column stops at.
const FULL_CACTUS_MAX_AGE: i32 = 8;
/// Chance of a cactus flower on a column under [`MAX_HEIGHT`].
const CACTUS_FLOWER_CHANCE: f32 = 0.1;
/// Chance of a cactus flower on a full column.
const FULL_CACTUS_FLOWER_CHANCE: f32 = 0.25;

/// Native material type ordinals.
const MATERIAL_PLANT: u8 = 8;
const MATERIAL_LAVA: u8 = 6;

/// Whether the block's native material is solid: everything but air and
/// light blocks (0), water (5), lava (6), plants (8), fire (10), powder
/// snow (14), portals (16), bubble columns (18) and decorations (24:
/// torches, carpets, rails, snow layers, ...), as the native material
/// table marks them.
pub fn solid_material(entry: &BlockEntry) -> bool {
    !matches!(entry.data.material, 0 | 5 | 6 | 8 | 10 | 14 | 16 | 18 | 24)
}

/// What a single plant may stand on (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Soil {
    /// A `dirt`-tagged block.
    Dirt,
    /// `dirt`- or `sand`-tagged blocks and terracotta.
    Dry,
    /// Mycelium, podzol, nylium or any opaque full block.
    Mushroom,
    /// `dirt`-tagged blocks, nylium and soul soil.
    Nether,
    /// `dirt`-tagged blocks, netherrack, soul sand and soul soil.
    WitherRose,
    /// `dirt`-, `sand`- and `gravel`-tagged blocks.
    Bamboo,
    /// Cactus, farmland or a block whose top centre holds.
    CactusFlower,
    /// A block with a full top face.
    FullTop,
}

/// The soil rule of a single plant, `None` for blocks that are not one.
pub fn soil(entry: &BlockEntry) -> Option<Soil> {
    if entry.data.material != MATERIAL_PLANT
        || entry.state_value(entry.default_state_id, UPPER).is_some()
    {
        return None;
    }
    let name = entry.string_id.strip_prefix("minecraft:")?;
    Some(match name {
        "deadbush" | "short_dry_grass" | "tall_dry_grass" => Soil::Dry,
        "brown_mushroom" | "red_mushroom" => Soil::Mushroom,
        "crimson_roots" | "warped_roots" | "nether_sprouts" | "crimson_fungus"
        | "warped_fungus" => Soil::Nether,
        "wither_rose" => Soil::WitherRose,
        "bamboo_sapling" => Soil::Bamboo,
        "cactus_flower" => Soil::CactusFlower,
        "leaf_litter" => Soil::FullTop,
        "short_grass" | "fern" | "bush" | "firefly_bush" | "torchflower" | "open_eyeblossom"
        | "closed_eyeblossom" | "golden_dandelion" | "pink_petals" | "wildflowers"
        | "sweet_berry_bush" => Soil::Dirt,
        _ if entry.has_tag("fertilize_area") || name.ends_with("_sapling") => Soil::Dirt,
        _ => return None,
    })
}

/// Two-block plants (see the module docs).
pub fn is_double_plant(entry: &BlockEntry) -> bool {
    entry.has_tag("plant")
        && !entry.has_tag("minecraft:crop")
        && entry.state_value(entry.default_state_id, UPPER).is_some()
}

/// Single-block plants that need soil (see the module docs).
pub fn is_plant(entry: &BlockEntry) -> bool {
    soil(entry).is_some()
}

/// The block at `pos`, `None` when unloaded.
fn entry_at(blocks: &impl BlockSource, pos: IVec3) -> Option<&BlockEntry> {
    let state = blocks.block_at(pos)?;
    blocks.registry().get_by_runtime_id(state)
}

fn is_terracotta(entry: &BlockEntry) -> bool {
    let id = entry.string_id.as_str();
    id == "minecraft:hardened_clay" || (id.ends_with("_terracotta") && !id.contains("glazed"))
}

fn named(entry: &BlockEntry, names: &[&str]) -> bool {
    names
        .iter()
        .any(|name| entry.string_id.strip_prefix("minecraft:") == Some(*name))
}

/// Whether the block at `pos` is soil of the kind `soil`.
fn is_soil(blocks: &impl BlockSource, pos: IVec3, soil: Soil) -> bool {
    let Some(below) = entry_at(blocks, pos) else {
        return false;
    };
    let dirt = below.has_tag("dirt");
    match soil {
        Soil::Dirt => dirt,
        Soil::Dry => dirt || below.has_tag("sand") || is_terracotta(below),
        Soil::Mushroom => {
            named(
                below,
                &["mycelium", "podzol", "crimson_nylium", "warped_nylium"],
            ) || !below.is_transparent
        }
        Soil::Nether => dirt || named(below, &["crimson_nylium", "warped_nylium", "soul_soil"]),
        Soil::WitherRose => dirt || named(below, &["netherrack", "soul_sand", "soul_soil"]),
        Soil::Bamboo => dirt || below.has_tag("sand") || below.has_tag("gravel"),
        Soil::CactusFlower => {
            named(below, &["cactus", "farmland"])
                || provides_support(blocks, pos, IVec3::Y, SupportKind::Centre)
        }
        Soil::FullTop => provides_support(blocks, pos, IVec3::Y, SupportKind::Full),
    }
}

pub fn plant_can_survive(blocks: &impl BlockSource, pos: IVec3, entry: &BlockEntry) -> bool {
    soil(entry).is_some_and(|soil| is_soil(blocks, pos - IVec3::Y, soil))
}

/// Whether the cell at `pos` is water (`water`-tagged) or frosted ice;
/// unloaded cells count.
fn wet(blocks: &impl BlockSource, pos: IVec3) -> bool {
    entry_at(blocks, pos)
        .is_none_or(|entry| entry.has_tag("water") || entry.string_id == "minecraft:frosted_ice")
}

pub fn sugar_cane_can_survive(blocks: &impl BlockSource, pos: IVec3) -> bool {
    let soil_pos = pos - IVec3::Y;
    let Some(soil) = entry_at(blocks, soil_pos) else {
        return false;
    };
    if soil.string_id == "minecraft:reeds" {
        return true;
    }
    let soil_ok =
        (soil.has_tag("dirt") && soil.string_id != "minecraft:farmland") || soil.has_tag("sand");
    let liquid_above = blocks
        .block_at(pos + IVec3::Y)
        .is_some_and(|above| blocks.registry().liquid_state(above).is_some());
    soil_ok
        && !liquid_above
        && super::HORIZONTAL
            .iter()
            .any(|side| wet(blocks, soil_pos + *side))
}

pub fn cactus_can_survive(blocks: &impl BlockSource, pos: IVec3) -> bool {
    let sides_clear = super::HORIZONTAL.iter().all(|side| {
        entry_at(blocks, pos + *side)
            .is_none_or(|entry| !solid_material(entry) && entry.data.material != MATERIAL_LAVA)
    });
    let lava_above =
        entry_at(blocks, pos + IVec3::Y).is_some_and(|entry| entry.data.material == MATERIAL_LAVA);
    let soil_ok = entry_at(blocks, pos - IVec3::Y)
        .is_some_and(|below| below.string_id == "minecraft:cactus" || below.has_tag("sand"));
    sides_clear && !lava_above && soil_ok
}

/// Whether the cell at `pos` holds the `upper` (or lower) half of `entry`.
fn half_at(blocks: &impl BlockSource, pos: IVec3, entry: &BlockEntry, upper: bool) -> bool {
    blocks
        .block_at(pos)
        .is_some_and(|state| is_state_of(entry, state) && bit(entry, state, UPPER) == upper)
}

pub fn double_plant_can_survive(
    blocks: &impl BlockSource,
    pos: IVec3,
    state: u32,
    entry: &BlockEntry,
) -> bool {
    if bit(entry, state, UPPER) {
        return half_at(blocks, pos - IVec3::Y, entry, false);
    }
    half_at(blocks, pos + IVec3::Y, entry, true) && is_soil(blocks, pos - IVec3::Y, Soil::Dirt)
}

pub fn carpet_can_survive(blocks: &impl BlockSource, pos: IVec3) -> bool {
    blocks
        .block_at(pos - IVec3::Y)
        .is_some_and(|below| below != air(blocks.registry()))
}

/// The upper half placed with the lower half `state` at `pos`.
pub fn double_plant_parts(entry: &BlockEntry, pos: IVec3, state: u32) -> Vec<(IVec3, u32)> {
    if bit(entry, state, UPPER) {
        return Vec::new();
    }
    vec![(
        pos + IVec3::Y,
        with(entry, state, UPPER, StateValue::Byte(1)),
    )]
}

/// The other half of the double plant half `state` at `pos`, and the
/// state whose drops a player's break yields (the lower half).
pub fn double_plant_structure(entry: &BlockEntry, pos: IVec3, state: u32) -> (Vec<IVec3>, u32) {
    let partner = if bit(entry, state, UPPER) {
        pos - IVec3::Y
    } else {
        pos + IVec3::Y
    };
    (
        vec![partner],
        with(entry, state, UPPER, StateValue::Byte(0)),
    )
}

/// Height of the column of `entry` blocks ending at `pos` (counted down,
/// `pos` included).
fn column_height(blocks: &impl BlockSource, pos: IVec3, entry: &BlockEntry) -> i32 {
    let mut height = 1;
    while blocks
        .block_at(pos - IVec3::Y * height)
        .is_some_and(|state| is_state_of(entry, state))
    {
        height += 1;
    }
    height
}

fn registry(world: &World) -> std::sync::Arc<crate::registry::BlockRegistry> {
    world.resource::<BlockRegistryResource>().0.clone()
}

pub fn sugar_cane_random_tick(world: &mut World, pos: IVec3, state: u32) {
    if !sugar_cane_can_survive(world, pos) {
        destroy_block(world, pos, true);
        return;
    }
    let registry = registry(world);
    let Some(entry) = registry.get_by_runtime_id(state) else {
        return;
    };
    let above = pos + IVec3::Y;
    if world.block_at(above) != Some(air(&registry))
        || column_height(world, pos, entry) >= MAX_HEIGHT
    {
        return;
    }
    let Some(age) = int(entry, state, AGE) else {
        return;
    };
    if age >= MAX_AGE {
        let young = with(entry, entry.default_state_id, AGE, StateValue::Int(0));
        set_block(world, above, young, false);
        set_block(
            world,
            pos,
            with(entry, state, AGE, StateValue::Int(0)),
            false,
        );
    } else {
        set_block(
            world,
            pos,
            with(entry, state, AGE, StateValue::Int(age + 1)),
            false,
        );
    }
}

/// Bone meal on the sugar cane at `pos`: new cane fills the air above the
/// column until it is [`MAX_HEIGHT`] tall. Returns whether any grew (the
/// bone meal is then used).
pub fn sugar_cane_fertilize(world: &mut World, pos: IVec3, state: u32) -> bool {
    let registry = registry(world);
    let Some(entry) = registry.get_by_runtime_id(state) else {
        return false;
    };
    let air = air(&registry);
    let young = with(entry, entry.default_state_id, AGE, StateValue::Int(0));
    let mut top = pos;
    while world
        .block_at(top + IVec3::Y)
        .is_some_and(|above| is_state_of(entry, above))
    {
        top += IVec3::Y;
    }
    let mut height = column_height(world, top, entry);
    let mut grew = false;
    while height < MAX_HEIGHT && world.block_at(top + IVec3::Y) == Some(air) {
        top += IVec3::Y;
        set_block(world, top, young, false);
        height += 1;
        grew = true;
    }
    if grew {
        growth_particles(world, pos);
    }
    grew
}

pub fn cactus_random_tick(world: &mut World, pos: IVec3, state: u32) {
    let registry = registry(world);
    let Some(entry) = registry.get_by_runtime_id(state) else {
        return;
    };
    let above = pos + IVec3::Y;
    if world.block_at(above) != Some(air(&registry)) {
        return;
    }
    let Some(age) = int(entry, state, AGE) else {
        return;
    };
    let height = column_height(world, pos, entry);
    if height >= MAX_HEIGHT && age >= FULL_CACTUS_MAX_AGE {
        return;
    }
    let restart = with(entry, state, AGE, StateValue::Int(0));
    if height < MAX_HEIGHT && age >= MAX_AGE {
        let young = with(entry, entry.default_state_id, AGE, StateValue::Int(0));
        set_block(world, above, young, false);
        set_block(world, pos, restart, false);
        // The new cactus checks its own sides as a neighbour update would.
        if !super::can_survive(world, above, young) {
            schedule_tick(world, above, 1);
        }
        return;
    }
    if age == CACTUS_FLOWER_AGE
        && let Some(flower) = registry.get_by_name("minecraft:cactus_flower")
        && plant_can_survive(world, above, flower)
    {
        let chance = if height < MAX_HEIGHT {
            CACTUS_FLOWER_CHANCE
        } else {
            FULL_CACTUS_FLOWER_CHANCE
        };
        if random(world).gen_range(0.0f32..1.0) < chance {
            set_block(world, above, flower.default_state_id, false);
            set_block(world, pos, restart, false);
            return;
        }
    }
    set_block(
        world,
        pos,
        with(entry, state, AGE, StateValue::Int(age + 1)),
        false,
    );
}

#[cfg(test)]
mod tests {
    use glam::IVec3;

    use super::super::tests::{FLOOR, Harness, state};
    use super::super::{Family, random_tick};
    use super::*;
    use crate::entity::systems::physics::tests::TestBlocks;
    use crate::registry::BlockRegistry;

    fn entry(name: &str) -> &'static BlockEntry {
        BlockRegistry::vanilla().get_by_name(name).unwrap()
    }

    fn family(name: &str) -> Family {
        Family::of(entry(name))
    }

    #[test]
    fn plant_families_come_from_material_tags_and_names() {
        for name in [
            "minecraft:poppy",
            "minecraft:dandelion",
            "minecraft:torchflower",
            "minecraft:short_grass",
            "minecraft:fern",
            "minecraft:oak_sapling",
            "minecraft:cherry_sapling",
            "minecraft:deadbush",
            "minecraft:brown_mushroom",
            "minecraft:crimson_roots",
            "minecraft:sweet_berry_bush",
        ] {
            assert_eq!(family(name), Family::Plant, "{name}");
        }
        for name in [
            "minecraft:tall_grass",
            "minecraft:large_fern",
            "minecraft:sunflower",
            "minecraft:pitcher_plant",
        ] {
            assert_eq!(family(name), Family::DoublePlant, "{name}");
        }
        assert_eq!(family("minecraft:pitcher_crop"), Family::Crop);
        assert_eq!(family("minecraft:pale_moss_carpet"), Family::Carpet);
        assert_eq!(family("minecraft:moss_carpet"), Family::Carpet);
        for name in [
            "minecraft:grass_block",
            "minecraft:moss_block",
            "minecraft:seagrass",
            "minecraft:vine",
            "minecraft:small_dripleaf_block",
            "minecraft:mangrove_propagule",
        ] {
            assert!(
                !is_plant(entry(name)) && !is_double_plant(entry(name)),
                "{name}"
            );
        }
    }

    #[test]
    fn plants_need_their_soil_below() {
        let pos = IVec3::new(0, 1, 0);
        let cases = [
            ("minecraft:poppy", "minecraft:grass_block", true),
            ("minecraft:poppy", "minecraft:farmland", true),
            ("minecraft:poppy", "minecraft:sand", false),
            ("minecraft:poppy", "minecraft:stone", false),
            ("minecraft:deadbush", "minecraft:sand", true),
            ("minecraft:deadbush", "minecraft:red_terracotta", true),
            ("minecraft:deadbush", "minecraft:stone", false),
            ("minecraft:crimson_roots", "minecraft:soul_soil", true),
            ("minecraft:crimson_roots", "minecraft:netherrack", false),
            ("minecraft:wither_rose", "minecraft:netherrack", true),
            ("minecraft:brown_mushroom", "minecraft:mycelium", true),
            ("minecraft:brown_mushroom", "minecraft:stone", true),
            ("minecraft:brown_mushroom", "minecraft:glass", false),
        ];
        for (plant, below, survives) in cases {
            let blocks = TestBlocks::floor(below, 0);
            assert_eq!(
                plant_can_survive(&blocks, pos, entry(plant)),
                survives,
                "{plant} on {below}"
            );
        }
    }

    #[test]
    fn carpet_needs_any_block_below() {
        let mut blocks = TestBlocks::floor("minecraft:oak_fence", 0);
        assert!(carpet_can_survive(&blocks, IVec3::new(0, 1, 0)));
        blocks.set(IVec3::ZERO, "minecraft:air");
        assert!(!carpet_can_survive(&blocks, IVec3::new(0, 1, 0)));
    }

    #[test]
    fn sugar_cane_needs_water_beside_its_soil() {
        let pos = IVec3::new(0, 1, 0);
        let mut blocks = TestBlocks::floor("minecraft:sand", 0);
        assert!(!sugar_cane_can_survive(&blocks, pos));
        blocks.set(IVec3::new(1, 1, 0), "minecraft:water");
        assert!(
            !sugar_cane_can_survive(&blocks, pos),
            "water beside the cane"
        );
        blocks.set(IVec3::new(1, 0, 0), "minecraft:water");
        assert!(sugar_cane_can_survive(&blocks, pos));
        blocks.set(pos, "minecraft:reeds");
        blocks.set(pos + IVec3::Y, "minecraft:reeds");
        assert!(sugar_cane_can_survive(&blocks, pos + IVec3::Y), "on cane");
        blocks.set(IVec3::ZERO, "minecraft:farmland");
        assert!(
            !sugar_cane_can_survive(&blocks, pos),
            "farmland is not cane soil"
        );
        blocks.set(IVec3::ZERO, "minecraft:grass_block");
        assert!(sugar_cane_can_survive(&blocks, pos));
    }

    /// Sand at the floor with water beside it and sugar cane on it.
    fn cane(h: &mut Harness) -> IVec3 {
        let soil = IVec3::new(4, FLOOR, 4);
        h.put(soil, state("minecraft:sand", &[]));
        h.put(soil + IVec3::X, state("minecraft:water", &[]));
        h.put(soil + IVec3::Y, state("minecraft:reeds", &[]));
        soil + IVec3::Y
    }

    #[test]
    fn sugar_cane_grows_a_block_every_sixteen_ticks_up_to_three() {
        let mut h = Harness::new();
        let pos = cane(&mut h);
        let mut ticks = 0;
        while h.name(pos + IVec3::Y) == "minecraft:air" {
            let state = h.state(pos);
            random_tick(&mut h.world, pos, state);
            ticks += 1;
        }
        assert_eq!(ticks, 16);
        for _ in 0..100 {
            for y in 0..4 {
                let cell = pos + IVec3::Y * y;
                let state = h.state(cell);
                random_tick(&mut h.world, cell, state);
            }
        }
        for y in 0..3 {
            assert_eq!(h.name(pos + IVec3::Y * y), "minecraft:reeds");
        }
        assert_eq!(h.name(pos + IVec3::Y * 3), "minecraft:air");
    }

    #[test]
    fn sugar_cane_without_water_breaks_on_its_next_random_tick() {
        let mut h = Harness::new();
        let pos = cane(&mut h);
        h.set(pos - IVec3::Y + IVec3::X, state("minecraft:stone", &[]));
        let state = h.state(pos);
        random_tick(&mut h.world, pos, state);
        assert_eq!(h.name(pos), "minecraft:air");
        assert_eq!(h.drops(), vec![("minecraft:sugar_cane".to_string(), 1)]);
    }

    #[test]
    fn bone_meal_fills_the_sugar_cane_column_to_three() {
        let mut h = Harness::new();
        let pos = cane(&mut h);
        let state = h.state(pos);
        assert!(sugar_cane_fertilize(&mut h.world, pos, state));
        assert_eq!(h.name(pos + IVec3::Y * 2), "minecraft:reeds");
        assert_eq!(h.name(pos + IVec3::Y * 3), "minecraft:air");
        assert!(!sugar_cane_fertilize(&mut h.world, pos, state));
    }

    /// A cactus on sand at (6, 64, 6).
    fn cactus(h: &mut Harness, age: i32) -> IVec3 {
        let soil = IVec3::new(6, FLOOR, 6);
        h.put(soil, state("minecraft:sand", &[]));
        h.put(
            soil + IVec3::Y,
            state("minecraft:cactus", &[(AGE, StateValue::Int(age))]),
        );
        soil + IVec3::Y
    }

    #[test]
    fn cactus_breaks_with_drops_next_to_a_solid_block() {
        let mut h = Harness::new();
        let pos = cactus(&mut h, 0);
        h.set(pos + IVec3::X, state("minecraft:white_carpet", &[]));
        h.tick(1);
        assert_eq!(h.name(pos), "minecraft:cactus", "carpet is not solid");
        h.set(pos + IVec3::X, state("minecraft:air", &[]));
        h.set(pos + IVec3::Z, state("minecraft:oak_planks", &[]));
        assert_eq!(h.name(pos), "minecraft:cactus", "breaks one tick later");
        h.tick(1);
        assert_eq!(h.name(pos), "minecraft:air");
        assert_eq!(h.drops(), vec![("minecraft:cactus".to_string(), 1)]);
    }

    #[test]
    fn cactus_at_age_fifteen_grows_a_new_cactus_until_three_tall() {
        let mut h = Harness::new();
        let pos = cactus(&mut h, MAX_AGE);
        let grown = h.state(pos);
        random_tick(&mut h.world, pos, grown);
        assert_eq!(h.value(pos, AGE), Some(StateValue::Int(0)));
        assert_eq!(h.name(pos + IVec3::Y), "minecraft:cactus");
        assert_eq!(h.value(pos + IVec3::Y, AGE), Some(StateValue::Int(0)));

        let top = pos + IVec3::Y * 2;
        h.put(
            top,
            state("minecraft:cactus", &[(AGE, StateValue::Int(MAX_AGE))]),
        );
        let full = h.state(top);
        random_tick(&mut h.world, top, full);
        assert_eq!(
            h.name(top + IVec3::Y),
            "minecraft:air",
            "three is the limit"
        );
        assert_eq!(h.value(top, AGE), Some(StateValue::Int(MAX_AGE)));
    }

    #[test]
    fn double_plant_halves_break_together() {
        let mut h = Harness::new();
        let soil = IVec3::new(8, FLOOR, 8);
        let lower = soil + IVec3::Y;
        let upper = lower + IVec3::Y;
        let grass = |upper: u8| state("minecraft:tall_grass", &[(UPPER, StateValue::Byte(upper))]);
        h.put(soil, state("minecraft:dirt", &[]));
        h.put(lower, grass(0));
        h.put(upper, grass(1));
        h.set(soil, state("minecraft:stone", &[]));
        h.tick(2);
        assert_eq!(h.name(lower), "minecraft:air");
        assert_eq!(h.name(upper), "minecraft:air");

        h.put(soil, state("minecraft:dirt", &[]));
        h.put(lower, grass(0));
        h.put(upper, grass(1));
        h.set(upper, state("minecraft:air", &[]));
        h.tick(1);
        assert_eq!(h.name(lower), "minecraft:air");
    }

    #[test]
    fn double_plant_parts_and_structure_pair_the_halves() {
        let sunflower = entry("minecraft:sunflower");
        let lower = state("minecraft:sunflower", &[(UPPER, StateValue::Byte(0))]);
        let upper = state("minecraft:sunflower", &[(UPPER, StateValue::Byte(1))]);
        let pos = IVec3::new(0, 1, 0);
        assert_eq!(
            double_plant_parts(sunflower, pos, lower),
            vec![(pos + IVec3::Y, upper)]
        );
        assert_eq!(
            double_plant_structure(sunflower, pos + IVec3::Y, upper),
            (vec![pos], lower)
        );
    }
}
