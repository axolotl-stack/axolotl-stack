//! Blocks and block sets the hand-written runtime code needs.
//!
//! Built-in features, surface builders and carvers are Rust, but their block
//! ids must still come from the canonical palette. Each entry here becomes,
//! in the generated `bb` module:
//! - [`Spec::State`]: `pub const NAME: u32` (the exact state; omitted states
//!   take the block's first canonical state);
//! - [`Spec::Blocks`] / [`Spec::Tags`]: `pub fn name(id: u32) -> bool` testing
//!   every state of the listed blocks / of blocks carrying any listed tag.
//!
//! A name or state that does not exist in the palette is a build error.

/// A state value in a declaration.
#[derive(Debug, Clone, Copy)]
pub enum Val {
    I(i32),
    S(&'static str),
    B(bool),
}

/// One declaration.
#[derive(Debug, Clone, Copy)]
pub enum Spec {
    State(&'static str, &'static [(&'static str, Val)]),
    Blocks(&'static [&'static str]),
    Tags(&'static [&'static str]),
}

use Spec::{Blocks, State, Tags};
use Val::{B, I, S};

/// Declarations, grouped by consumer. Constant names are SCREAMING_CASE for
/// states and snake_case for sets.
pub const BUILTIN_BLOCKS: &[(&str, Spec)] = &[
    // Shared
    ("AIR", State("minecraft:air", &[])),
    ("WATER", State("minecraft:water", &[])),
    ("LAVA", State("minecraft:lava", &[])),
    ("STONE", State("minecraft:stone", &[])),
    ("BEDROCK", State("minecraft:bedrock", &[])),
    ("DIRT", State("minecraft:dirt", &[])),
    ("COARSE_DIRT", State("minecraft:coarse_dirt", &[])),
    ("GRASS_BLOCK", State("minecraft:grass_block", &[])),
    ("PODZOL", State("minecraft:podzol", &[])),
    ("MYCELIUM", State("minecraft:mycelium", &[])),
    ("SAND", State("minecraft:sand", &[])),
    ("RED_SAND", State("minecraft:red_sand", &[])),
    ("SANDSTONE", State("minecraft:sandstone", &[])),
    ("RED_SANDSTONE", State("minecraft:red_sandstone", &[])),
    ("GRAVEL", State("minecraft:gravel", &[])),
    ("CLAY", State("minecraft:clay", &[])),
    ("ICE", State("minecraft:ice", &[])),
    ("PACKED_ICE", State("minecraft:packed_ice", &[])),
    ("BLUE_ICE", State("minecraft:blue_ice", &[])),
    ("SNOW_LAYER", State("minecraft:snow_layer", &[])),
    ("SNOW_BLOCK", State("minecraft:snow", &[])),
    ("POWDER_SNOW", State("minecraft:powder_snow", &[])),
    ("DEEPSLATE", State("minecraft:deepslate", &[])),
    ("TUFF", State("minecraft:tuff", &[])),
    ("CALCITE", State("minecraft:calcite", &[])),
    ("OBSIDIAN", State("minecraft:obsidian", &[])),
    ("MAGMA", State("minecraft:magma", &[])),
    ("MUD", State("minecraft:mud", &[])),
    ("HARDENED_CLAY", State("minecraft:hardened_clay", &[])),
    ("WHITE_TERRACOTTA", State("minecraft:white_terracotta", &[])),
    (
        "ORANGE_TERRACOTTA",
        State("minecraft:orange_terracotta", &[]),
    ),
    (
        "YELLOW_TERRACOTTA",
        State("minecraft:yellow_terracotta", &[]),
    ),
    ("BROWN_TERRACOTTA", State("minecraft:brown_terracotta", &[])),
    ("RED_TERRACOTTA", State("minecraft:red_terracotta", &[])),
    (
        "LIGHT_GRAY_TERRACOTTA",
        State("minecraft:light_gray_terracotta", &[]),
    ),
    ("air", Blocks(&["minecraft:air"])),
    (
        "water",
        Blocks(&["minecraft:water", "minecraft:flowing_water"]),
    ),
    (
        "lava",
        Blocks(&["minecraft:lava", "minecraft:flowing_lava"]),
    ),
    (
        "ice",
        Blocks(&[
            "minecraft:ice",
            "minecraft:packed_ice",
            "minecraft:blue_ice",
            "minecraft:frosted_ice",
        ]),
    ),
    ("dirt_like", Tags(&["dirt"])),
    ("sand_like", Tags(&["sand"])),
    ("stone_like", Tags(&["stone"])),
    ("leaves", Tags(&["minecraft:leaves"])),
    ("logs", Tags(&["log"])),
    (
        "snow_like",
        Blocks(&[
            "minecraft:snow_layer",
            "minecraft:snow",
            "minecraft:powder_snow",
        ]),
    ),
    (
        "terracotta",
        Blocks(&[
            "minecraft:hardened_clay",
            "minecraft:white_terracotta",
            "minecraft:orange_terracotta",
            "minecraft:magenta_terracotta",
            "minecraft:light_blue_terracotta",
            "minecraft:yellow_terracotta",
            "minecraft:lime_terracotta",
            "minecraft:pink_terracotta",
            "minecraft:gray_terracotta",
            "minecraft:light_gray_terracotta",
            "minecraft:cyan_terracotta",
            "minecraft:purple_terracotta",
            "minecraft:blue_terracotta",
            "minecraft:brown_terracotta",
            "minecraft:green_terracotta",
            "minecraft:red_terracotta",
            "minecraft:black_terracotta",
        ]),
    ),
    // Persistence/update bits on leaves placed by worldgen trees.
    (
        "OAK_LEAVES",
        State(
            "minecraft:oak_leaves",
            &[("persistent_bit", B(false)), ("update_bit", B(false))],
        ),
    ),
    (
        "OAK_LOG",
        State("minecraft:oak_log", &[("pillar_axis", S("y"))]),
    ),
    ("KELP_AGE_0", State("minecraft:kelp", &[("kelp_age", I(0))])),
    // Aquatic built-ins (`builtin/aquatic.rs`). Coral colours are always listed
    // tube, brain, bubble, fire, horn. Wall fans: `coral_direction` 0 west,
    // 1 east, 2 north, 3 south = the side the fan faces (away from its block).
    (
        "SEAGRASS",
        State("minecraft:seagrass", &[("sea_grass_type", S("default"))]),
    ),
    (
        "SEAGRASS_DOUBLE_BOT",
        State("minecraft:seagrass", &[("sea_grass_type", S("double_bot"))]),
    ),
    (
        "SEAGRASS_DOUBLE_TOP",
        State("minecraft:seagrass", &[("sea_grass_type", S("double_top"))]),
    ),
    (
        "KELP_AGE_20",
        State("minecraft:kelp", &[("kelp_age", I(20))]),
    ),
    (
        "KELP_AGE_21",
        State("minecraft:kelp", &[("kelp_age", I(21))]),
    ),
    (
        "KELP_AGE_22",
        State("minecraft:kelp", &[("kelp_age", I(22))]),
    ),
    (
        "KELP_AGE_23",
        State("minecraft:kelp", &[("kelp_age", I(23))]),
    ),
    ("kelp", Blocks(&["minecraft:kelp"])),
    ("WATERLILY", State("minecraft:waterlily", &[])),
    (
        "SEA_PICKLE_1",
        State(
            "minecraft:sea_pickle",
            &[("cluster_count", I(0)), ("dead_bit", B(false))],
        ),
    ),
    (
        "SEA_PICKLE_2",
        State(
            "minecraft:sea_pickle",
            &[("cluster_count", I(1)), ("dead_bit", B(false))],
        ),
    ),
    (
        "SEA_PICKLE_3",
        State(
            "minecraft:sea_pickle",
            &[("cluster_count", I(2)), ("dead_bit", B(false))],
        ),
    ),
    (
        "SEA_PICKLE_4",
        State(
            "minecraft:sea_pickle",
            &[("cluster_count", I(3)), ("dead_bit", B(false))],
        ),
    ),
    ("TUBE_CORAL_BLOCK", State("minecraft:tube_coral_block", &[])),
    (
        "BRAIN_CORAL_BLOCK",
        State("minecraft:brain_coral_block", &[]),
    ),
    (
        "BUBBLE_CORAL_BLOCK",
        State("minecraft:bubble_coral_block", &[]),
    ),
    ("FIRE_CORAL_BLOCK", State("minecraft:fire_coral_block", &[])),
    ("HORN_CORAL_BLOCK", State("minecraft:horn_coral_block", &[])),
    ("TUBE_CORAL", State("minecraft:tube_coral", &[])),
    ("BRAIN_CORAL", State("minecraft:brain_coral", &[])),
    ("BUBBLE_CORAL", State("minecraft:bubble_coral", &[])),
    ("FIRE_CORAL", State("minecraft:fire_coral", &[])),
    ("HORN_CORAL", State("minecraft:horn_coral", &[])),
    (
        "TUBE_CORAL_FAN_0",
        State("minecraft:tube_coral_fan", &[("coral_fan_direction", I(0))]),
    ),
    (
        "TUBE_CORAL_FAN_1",
        State("minecraft:tube_coral_fan", &[("coral_fan_direction", I(1))]),
    ),
    (
        "BRAIN_CORAL_FAN_0",
        State(
            "minecraft:brain_coral_fan",
            &[("coral_fan_direction", I(0))],
        ),
    ),
    (
        "BRAIN_CORAL_FAN_1",
        State(
            "minecraft:brain_coral_fan",
            &[("coral_fan_direction", I(1))],
        ),
    ),
    (
        "BUBBLE_CORAL_FAN_0",
        State(
            "minecraft:bubble_coral_fan",
            &[("coral_fan_direction", I(0))],
        ),
    ),
    (
        "BUBBLE_CORAL_FAN_1",
        State(
            "minecraft:bubble_coral_fan",
            &[("coral_fan_direction", I(1))],
        ),
    ),
    (
        "FIRE_CORAL_FAN_0",
        State("minecraft:fire_coral_fan", &[("coral_fan_direction", I(0))]),
    ),
    (
        "FIRE_CORAL_FAN_1",
        State("minecraft:fire_coral_fan", &[("coral_fan_direction", I(1))]),
    ),
    (
        "HORN_CORAL_FAN_0",
        State("minecraft:horn_coral_fan", &[("coral_fan_direction", I(0))]),
    ),
    (
        "HORN_CORAL_FAN_1",
        State("minecraft:horn_coral_fan", &[("coral_fan_direction", I(1))]),
    ),
    (
        "TUBE_CORAL_WALL_FAN_WEST",
        State(
            "minecraft:tube_coral_wall_fan",
            &[("coral_direction", I(0))],
        ),
    ),
    (
        "TUBE_CORAL_WALL_FAN_EAST",
        State(
            "minecraft:tube_coral_wall_fan",
            &[("coral_direction", I(1))],
        ),
    ),
    (
        "TUBE_CORAL_WALL_FAN_NORTH",
        State(
            "minecraft:tube_coral_wall_fan",
            &[("coral_direction", I(2))],
        ),
    ),
    (
        "TUBE_CORAL_WALL_FAN_SOUTH",
        State(
            "minecraft:tube_coral_wall_fan",
            &[("coral_direction", I(3))],
        ),
    ),
    (
        "BRAIN_CORAL_WALL_FAN_WEST",
        State(
            "minecraft:brain_coral_wall_fan",
            &[("coral_direction", I(0))],
        ),
    ),
    (
        "BRAIN_CORAL_WALL_FAN_EAST",
        State(
            "minecraft:brain_coral_wall_fan",
            &[("coral_direction", I(1))],
        ),
    ),
    (
        "BRAIN_CORAL_WALL_FAN_NORTH",
        State(
            "minecraft:brain_coral_wall_fan",
            &[("coral_direction", I(2))],
        ),
    ),
    (
        "BRAIN_CORAL_WALL_FAN_SOUTH",
        State(
            "minecraft:brain_coral_wall_fan",
            &[("coral_direction", I(3))],
        ),
    ),
    (
        "BUBBLE_CORAL_WALL_FAN_WEST",
        State(
            "minecraft:bubble_coral_wall_fan",
            &[("coral_direction", I(0))],
        ),
    ),
    (
        "BUBBLE_CORAL_WALL_FAN_EAST",
        State(
            "minecraft:bubble_coral_wall_fan",
            &[("coral_direction", I(1))],
        ),
    ),
    (
        "BUBBLE_CORAL_WALL_FAN_NORTH",
        State(
            "minecraft:bubble_coral_wall_fan",
            &[("coral_direction", I(2))],
        ),
    ),
    (
        "BUBBLE_CORAL_WALL_FAN_SOUTH",
        State(
            "minecraft:bubble_coral_wall_fan",
            &[("coral_direction", I(3))],
        ),
    ),
    (
        "FIRE_CORAL_WALL_FAN_WEST",
        State(
            "minecraft:fire_coral_wall_fan",
            &[("coral_direction", I(0))],
        ),
    ),
    (
        "FIRE_CORAL_WALL_FAN_EAST",
        State(
            "minecraft:fire_coral_wall_fan",
            &[("coral_direction", I(1))],
        ),
    ),
    (
        "FIRE_CORAL_WALL_FAN_NORTH",
        State(
            "minecraft:fire_coral_wall_fan",
            &[("coral_direction", I(2))],
        ),
    ),
    (
        "FIRE_CORAL_WALL_FAN_SOUTH",
        State(
            "minecraft:fire_coral_wall_fan",
            &[("coral_direction", I(3))],
        ),
    ),
    (
        "HORN_CORAL_WALL_FAN_WEST",
        State(
            "minecraft:horn_coral_wall_fan",
            &[("coral_direction", I(0))],
        ),
    ),
    (
        "HORN_CORAL_WALL_FAN_EAST",
        State(
            "minecraft:horn_coral_wall_fan",
            &[("coral_direction", I(1))],
        ),
    ),
    (
        "HORN_CORAL_WALL_FAN_NORTH",
        State(
            "minecraft:horn_coral_wall_fan",
            &[("coral_direction", I(2))],
        ),
    ),
    (
        "HORN_CORAL_WALL_FAN_SOUTH",
        State(
            "minecraft:horn_coral_wall_fan",
            &[("coral_direction", I(3))],
        ),
    ),
    (
        "coral_block",
        Blocks(&[
            "minecraft:tube_coral_block",
            "minecraft:brain_coral_block",
            "minecraft:bubble_coral_block",
            "minecraft:fire_coral_block",
            "minecraft:horn_coral_block",
        ]),
    ),
    (
        "coral_plant_or_fan",
        Blocks(&[
            "minecraft:tube_coral",
            "minecraft:brain_coral",
            "minecraft:bubble_coral",
            "minecraft:fire_coral",
            "minecraft:horn_coral",
            "minecraft:tube_coral_fan",
            "minecraft:brain_coral_fan",
            "minecraft:bubble_coral_fan",
            "minecraft:fire_coral_fan",
            "minecraft:horn_coral_fan",
        ]),
    ),
    // Legacy built-ins (`builtin/legacy.rs`): single flowers, the forest
    // double flowers (lower/upper halves), the forest rock and spring hosts.
    ("DANDELION", State("minecraft:dandelion", &[])),
    ("POPPY", State("minecraft:poppy", &[])),
    ("BLUE_ORCHID", State("minecraft:blue_orchid", &[])),
    ("ALLIUM", State("minecraft:allium", &[])),
    ("AZURE_BLUET", State("minecraft:azure_bluet", &[])),
    ("RED_TULIP", State("minecraft:red_tulip", &[])),
    ("ORANGE_TULIP", State("minecraft:orange_tulip", &[])),
    ("WHITE_TULIP", State("minecraft:white_tulip", &[])),
    ("PINK_TULIP", State("minecraft:pink_tulip", &[])),
    ("OXEYE_DAISY", State("minecraft:oxeye_daisy", &[])),
    ("CORNFLOWER", State("minecraft:cornflower", &[])),
    (
        "LILY_OF_THE_VALLEY",
        State("minecraft:lily_of_the_valley", &[]),
    ),
    (
        "LILAC_LOWER",
        State("minecraft:lilac", &[("upper_block_bit", B(false))]),
    ),
    (
        "LILAC_UPPER",
        State("minecraft:lilac", &[("upper_block_bit", B(true))]),
    ),
    (
        "ROSE_BUSH_LOWER",
        State("minecraft:rose_bush", &[("upper_block_bit", B(false))]),
    ),
    (
        "ROSE_BUSH_UPPER",
        State("minecraft:rose_bush", &[("upper_block_bit", B(true))]),
    ),
    (
        "PEONY_LOWER",
        State("minecraft:peony", &[("upper_block_bit", B(false))]),
    ),
    (
        "PEONY_UPPER",
        State("minecraft:peony", &[("upper_block_bit", B(true))]),
    ),
    (
        "MOSSY_COBBLESTONE",
        State("minecraft:mossy_cobblestone", &[]),
    ),
    (
        "lava_spring_host",
        Blocks(&[
            "minecraft:stone",
            "minecraft:granite",
            "minecraft:diorite",
            "minecraft:andesite",
            "minecraft:deepslate",
            "minecraft:tuff",
            "minecraft:calcite",
        ]),
    ),
    (
        "water_spring_host",
        Blocks(&[
            "minecraft:stone",
            "minecraft:granite",
            "minecraft:diorite",
            "minecraft:andesite",
            "minecraft:deepslate",
            "minecraft:tuff",
            "minecraft:calcite",
            "minecraft:dirt",
            "minecraft:snow",
            "minecraft:powder_snow",
            "minecraft:packed_ice",
        ]),
    ),
    // Terrain built-ins (`builtin/terrain.rs`). Spikes: `UP` = standing
    // (`hanging` 0), `DOWN` = hanging; thickness from the tip towards the
    // attached end is tip, frustum, middle..., base; `merge` is a tip touching
    // the opposite spike's tip. Huge mushroom bits: 1 NW, 2 N, 3 NE, 4 W,
    // 5 top, 6 E, 7 SW, 8 S, 9 SE (-x west, -z north), 10 stem sides.
    ("DRIPSTONE_BLOCK", State("minecraft:dripstone_block", &[])),
    (
        "POINTED_DRIPSTONE_UP_TIP",
        State(
            "minecraft:pointed_dripstone",
            &[("dripstone_thickness", S("tip")), ("hanging", B(false))],
        ),
    ),
    (
        "POINTED_DRIPSTONE_UP_FRUSTUM",
        State(
            "minecraft:pointed_dripstone",
            &[("dripstone_thickness", S("frustum")), ("hanging", B(false))],
        ),
    ),
    (
        "POINTED_DRIPSTONE_UP_MIDDLE",
        State(
            "minecraft:pointed_dripstone",
            &[("dripstone_thickness", S("middle")), ("hanging", B(false))],
        ),
    ),
    (
        "POINTED_DRIPSTONE_UP_BASE",
        State(
            "minecraft:pointed_dripstone",
            &[("dripstone_thickness", S("base")), ("hanging", B(false))],
        ),
    ),
    (
        "POINTED_DRIPSTONE_UP_MERGE",
        State(
            "minecraft:pointed_dripstone",
            &[("dripstone_thickness", S("merge")), ("hanging", B(false))],
        ),
    ),
    (
        "POINTED_DRIPSTONE_DOWN_TIP",
        State(
            "minecraft:pointed_dripstone",
            &[("dripstone_thickness", S("tip")), ("hanging", B(true))],
        ),
    ),
    (
        "POINTED_DRIPSTONE_DOWN_FRUSTUM",
        State(
            "minecraft:pointed_dripstone",
            &[("dripstone_thickness", S("frustum")), ("hanging", B(true))],
        ),
    ),
    (
        "POINTED_DRIPSTONE_DOWN_MIDDLE",
        State(
            "minecraft:pointed_dripstone",
            &[("dripstone_thickness", S("middle")), ("hanging", B(true))],
        ),
    ),
    (
        "POINTED_DRIPSTONE_DOWN_BASE",
        State(
            "minecraft:pointed_dripstone",
            &[("dripstone_thickness", S("base")), ("hanging", B(true))],
        ),
    ),
    (
        "POINTED_DRIPSTONE_DOWN_MERGE",
        State(
            "minecraft:pointed_dripstone",
            &[("dripstone_thickness", S("merge")), ("hanging", B(true))],
        ),
    ),
    (
        "pointed_dripstone",
        Blocks(&["minecraft:pointed_dripstone"]),
    ),
    ("SULFUR", State("minecraft:sulfur", &[])),
    ("CINNABAR", State("minecraft:cinnabar", &[])),
    (
        "sulfur_cave_stone",
        Blocks(&["minecraft:sulfur", "minecraft:cinnabar"]),
    ),
    (
        "SULFUR_SPIKE_UP_TIP",
        State(
            "minecraft:sulfur_spike",
            &[("dripstone_thickness", S("tip")), ("hanging", B(false))],
        ),
    ),
    (
        "SULFUR_SPIKE_UP_FRUSTUM",
        State(
            "minecraft:sulfur_spike",
            &[("dripstone_thickness", S("frustum")), ("hanging", B(false))],
        ),
    ),
    (
        "SULFUR_SPIKE_UP_MIDDLE",
        State(
            "minecraft:sulfur_spike",
            &[("dripstone_thickness", S("middle")), ("hanging", B(false))],
        ),
    ),
    (
        "SULFUR_SPIKE_UP_BASE",
        State(
            "minecraft:sulfur_spike",
            &[("dripstone_thickness", S("base")), ("hanging", B(false))],
        ),
    ),
    (
        "SULFUR_SPIKE_UP_MERGE",
        State(
            "minecraft:sulfur_spike",
            &[("dripstone_thickness", S("merge")), ("hanging", B(false))],
        ),
    ),
    (
        "SULFUR_SPIKE_DOWN_TIP",
        State(
            "minecraft:sulfur_spike",
            &[("dripstone_thickness", S("tip")), ("hanging", B(true))],
        ),
    ),
    (
        "SULFUR_SPIKE_DOWN_FRUSTUM",
        State(
            "minecraft:sulfur_spike",
            &[("dripstone_thickness", S("frustum")), ("hanging", B(true))],
        ),
    ),
    (
        "SULFUR_SPIKE_DOWN_MIDDLE",
        State(
            "minecraft:sulfur_spike",
            &[("dripstone_thickness", S("middle")), ("hanging", B(true))],
        ),
    ),
    (
        "SULFUR_SPIKE_DOWN_BASE",
        State(
            "minecraft:sulfur_spike",
            &[("dripstone_thickness", S("base")), ("hanging", B(true))],
        ),
    ),
    (
        "SULFUR_SPIKE_DOWN_MERGE",
        State(
            "minecraft:sulfur_spike",
            &[("dripstone_thickness", S("merge")), ("hanging", B(true))],
        ),
    ),
    // Vanilla's `base_stone_overworld` block set (what dripstone replaces).
    (
        "base_stone_overworld",
        Blocks(&[
            "minecraft:stone",
            "minecraft:granite",
            "minecraft:diorite",
            "minecraft:andesite",
            "minecraft:tuff",
            "minecraft:deepslate",
        ]),
    ),
    // Rock a sulfur spring trail may run through: the `block_allowlist` of
    // the shipped `sulfur_spring_*` structure features, minus air and its
    // dirt/sand/gravel tags (tested in code with `dirt_like`, `sand_like`
    // and `GRAVEL`).
    (
        "sulfur_trail_rock",
        Blocks(&[
            "minecraft:stone",
            "minecraft:granite",
            "minecraft:diorite",
            "minecraft:andesite",
            "minecraft:tuff",
            "minecraft:deepslate",
            "minecraft:calcite",
            "minecraft:sandstone",
            "minecraft:red_sandstone",
            "minecraft:hardened_clay",
            "minecraft:brown_terracotta",
            "minecraft:orange_terracotta",
            "minecraft:light_gray_terracotta",
            "minecraft:red_terracotta",
            "minecraft:white_terracotta",
            "minecraft:yellow_terracotta",
        ]),
    ),
    // Bottom half (the slab's first canonical state).
    ("SANDSTONE_SLAB", State("minecraft:sandstone_slab", &[])),
    ("snow_layer_any", Blocks(&["minecraft:snow_layer"])),
    (
        "huge_mushroom_soil",
        Blocks(&[
            "minecraft:dirt",
            "minecraft:grass_block",
            "minecraft:mycelium",
            "minecraft:podzol",
        ]),
    ),
    (
        "BROWN_MUSHROOM_BLOCK_1",
        State(
            "minecraft:brown_mushroom_block",
            &[("huge_mushroom_bits", I(1))],
        ),
    ),
    (
        "BROWN_MUSHROOM_BLOCK_2",
        State(
            "minecraft:brown_mushroom_block",
            &[("huge_mushroom_bits", I(2))],
        ),
    ),
    (
        "BROWN_MUSHROOM_BLOCK_3",
        State(
            "minecraft:brown_mushroom_block",
            &[("huge_mushroom_bits", I(3))],
        ),
    ),
    (
        "BROWN_MUSHROOM_BLOCK_4",
        State(
            "minecraft:brown_mushroom_block",
            &[("huge_mushroom_bits", I(4))],
        ),
    ),
    (
        "BROWN_MUSHROOM_BLOCK_5",
        State(
            "minecraft:brown_mushroom_block",
            &[("huge_mushroom_bits", I(5))],
        ),
    ),
    (
        "BROWN_MUSHROOM_BLOCK_6",
        State(
            "minecraft:brown_mushroom_block",
            &[("huge_mushroom_bits", I(6))],
        ),
    ),
    (
        "BROWN_MUSHROOM_BLOCK_7",
        State(
            "minecraft:brown_mushroom_block",
            &[("huge_mushroom_bits", I(7))],
        ),
    ),
    (
        "BROWN_MUSHROOM_BLOCK_8",
        State(
            "minecraft:brown_mushroom_block",
            &[("huge_mushroom_bits", I(8))],
        ),
    ),
    (
        "BROWN_MUSHROOM_BLOCK_9",
        State(
            "minecraft:brown_mushroom_block",
            &[("huge_mushroom_bits", I(9))],
        ),
    ),
    (
        "RED_MUSHROOM_BLOCK_1",
        State(
            "minecraft:red_mushroom_block",
            &[("huge_mushroom_bits", I(1))],
        ),
    ),
    (
        "RED_MUSHROOM_BLOCK_2",
        State(
            "minecraft:red_mushroom_block",
            &[("huge_mushroom_bits", I(2))],
        ),
    ),
    (
        "RED_MUSHROOM_BLOCK_3",
        State(
            "minecraft:red_mushroom_block",
            &[("huge_mushroom_bits", I(3))],
        ),
    ),
    (
        "RED_MUSHROOM_BLOCK_4",
        State(
            "minecraft:red_mushroom_block",
            &[("huge_mushroom_bits", I(4))],
        ),
    ),
    (
        "RED_MUSHROOM_BLOCK_5",
        State(
            "minecraft:red_mushroom_block",
            &[("huge_mushroom_bits", I(5))],
        ),
    ),
    (
        "RED_MUSHROOM_BLOCK_6",
        State(
            "minecraft:red_mushroom_block",
            &[("huge_mushroom_bits", I(6))],
        ),
    ),
    (
        "RED_MUSHROOM_BLOCK_7",
        State(
            "minecraft:red_mushroom_block",
            &[("huge_mushroom_bits", I(7))],
        ),
    ),
    (
        "RED_MUSHROOM_BLOCK_8",
        State(
            "minecraft:red_mushroom_block",
            &[("huge_mushroom_bits", I(8))],
        ),
    ),
    (
        "RED_MUSHROOM_BLOCK_9",
        State(
            "minecraft:red_mushroom_block",
            &[("huge_mushroom_bits", I(9))],
        ),
    ),
    (
        "MUSHROOM_STEM",
        State("minecraft:mushroom_stem", &[("huge_mushroom_bits", I(10))]),
    ),
    // Carvers (`features/carver.rs`): overworld ground the worm carvers
    // replace (terracotta via `terracotta`), the nether set, and the surface
    // blocks whose exposed dirt below becomes that surface.
    (
        "carver_replaceable",
        Blocks(&[
            "minecraft:stone",
            "minecraft:granite",
            "minecraft:diorite",
            "minecraft:andesite",
            "minecraft:deepslate",
            "minecraft:tuff",
            "minecraft:calcite",
            "minecraft:dirt",
            "minecraft:coarse_dirt",
            "minecraft:grass_block",
            "minecraft:podzol",
            "minecraft:mycelium",
            "minecraft:dirt_with_roots",
            "minecraft:moss_block",
            "minecraft:mud",
            "minecraft:muddy_mangrove_roots",
            "minecraft:sand",
            "minecraft:red_sand",
            "minecraft:suspicious_sand",
            "minecraft:gravel",
            "minecraft:suspicious_gravel",
            "minecraft:sandstone",
            "minecraft:red_sandstone",
            "minecraft:clay",
            "minecraft:snow",
            "minecraft:packed_ice",
            "minecraft:coal_ore",
            "minecraft:iron_ore",
            "minecraft:copper_ore",
            "minecraft:gold_ore",
            "minecraft:redstone_ore",
            "minecraft:lit_redstone_ore",
            "minecraft:lapis_ore",
            "minecraft:diamond_ore",
            "minecraft:emerald_ore",
            "minecraft:deepslate_coal_ore",
            "minecraft:deepslate_iron_ore",
            "minecraft:deepslate_copper_ore",
            "minecraft:deepslate_gold_ore",
            "minecraft:deepslate_redstone_ore",
            "minecraft:lit_deepslate_redstone_ore",
            "minecraft:deepslate_lapis_ore",
            "minecraft:deepslate_diamond_ore",
            "minecraft:deepslate_emerald_ore",
            "minecraft:raw_iron_block",
            "minecraft:raw_copper_block",
        ]),
    ),
    (
        "nether_carver_replaceable",
        Blocks(&[
            "minecraft:netherrack",
            "minecraft:soul_sand",
            "minecraft:soul_soil",
            "minecraft:basalt",
            "minecraft:blackstone",
            "minecraft:crimson_nylium",
            "minecraft:warped_nylium",
            "minecraft:nether_wart_block",
            "minecraft:warped_wart_block",
            "minecraft:magma",
            "minecraft:quartz_ore",
            "minecraft:nether_gold_ore",
            "minecraft:gravel",
            "minecraft:stone",
            "minecraft:granite",
            "minecraft:diorite",
            "minecraft:andesite",
            "minecraft:deepslate",
            "minecraft:tuff",
            "minecraft:dirt",
            "minecraft:grass_block",
            "minecraft:mycelium",
            "minecraft:podzol",
            "minecraft:coarse_dirt",
        ]),
    ),
    (
        "carver_surface_top",
        Blocks(&["minecraft:grass_block", "minecraft:mycelium"]),
    ),
    // Geodes (`features/geode.rs`): blocks that void a distribution point and
    // blocks no geode block may overwrite.
    (
        "geode_invalid",
        Blocks(&[
            "minecraft:bedrock",
            "minecraft:water",
            "minecraft:flowing_water",
            "minecraft:lava",
            "minecraft:flowing_lava",
            "minecraft:ice",
            "minecraft:packed_ice",
            "minecraft:blue_ice",
        ]),
    ),
    (
        "geode_cannot_replace",
        Blocks(&[
            "minecraft:bedrock",
            "minecraft:mob_spawner",
            "minecraft:chest",
            "minecraft:end_portal_frame",
            "minecraft:reinforced_deepslate",
            "minecraft:trial_spawner",
            "minecraft:vault",
        ]),
    ),
    // Plant built-ins (`builtin/plants.rs`). Directional rows are listed south,
    // west, north, east (`minecraft:cardinal_direction` order); segmented covers
    // use `growth` 0..=3 for 1..=4 pieces; vines name the side of the block
    // they cling to (`vine_direction_bits` 1 south, 2 west, 4 north, 8 east).
    ("CACTUS", State("minecraft:cactus", &[("age", I(0))])),
    ("CACTUS_FLOWER", State("minecraft:cactus_flower", &[])),
    ("REEDS", State("minecraft:reeds", &[("age", I(0))])),
    ("DEADBUSH", State("minecraft:deadbush", &[])),
    ("MELON_BLOCK", State("minecraft:melon_block", &[])),
    (
        "PUMPKIN_SOUTH",
        State(
            "minecraft:pumpkin",
            &[("minecraft:cardinal_direction", S("south"))],
        ),
    ),
    (
        "PUMPKIN_WEST",
        State(
            "minecraft:pumpkin",
            &[("minecraft:cardinal_direction", S("west"))],
        ),
    ),
    (
        "PUMPKIN_NORTH",
        State(
            "minecraft:pumpkin",
            &[("minecraft:cardinal_direction", S("north"))],
        ),
    ),
    (
        "PUMPKIN_EAST",
        State(
            "minecraft:pumpkin",
            &[("minecraft:cardinal_direction", S("east"))],
        ),
    ),
    ("BROWN_MUSHROOM", State("minecraft:brown_mushroom", &[])),
    ("RED_MUSHROOM", State("minecraft:red_mushroom", &[])),
    (
        "SWEET_BERRY_BUSH_MATURE",
        State("minecraft:sweet_berry_bush", &[("growth", I(3))]),
    ),
    (
        "CLOSED_EYEBLOSSOM",
        State("minecraft:closed_eyeblossom", &[]),
    ),
    ("FIREFLY_BUSH", State("minecraft:firefly_bush", &[])),
    (
        "PINK_PETALS_1_SOUTH",
        State(
            "minecraft:pink_petals",
            &[
                ("growth", I(0)),
                ("minecraft:cardinal_direction", S("south")),
            ],
        ),
    ),
    (
        "PINK_PETALS_1_WEST",
        State(
            "minecraft:pink_petals",
            &[
                ("growth", I(0)),
                ("minecraft:cardinal_direction", S("west")),
            ],
        ),
    ),
    (
        "PINK_PETALS_1_NORTH",
        State(
            "minecraft:pink_petals",
            &[
                ("growth", I(0)),
                ("minecraft:cardinal_direction", S("north")),
            ],
        ),
    ),
    (
        "PINK_PETALS_1_EAST",
        State(
            "minecraft:pink_petals",
            &[
                ("growth", I(0)),
                ("minecraft:cardinal_direction", S("east")),
            ],
        ),
    ),
    (
        "PINK_PETALS_2_SOUTH",
        State(
            "minecraft:pink_petals",
            &[
                ("growth", I(1)),
                ("minecraft:cardinal_direction", S("south")),
            ],
        ),
    ),
    (
        "PINK_PETALS_2_WEST",
        State(
            "minecraft:pink_petals",
            &[
                ("growth", I(1)),
                ("minecraft:cardinal_direction", S("west")),
            ],
        ),
    ),
    (
        "PINK_PETALS_2_NORTH",
        State(
            "minecraft:pink_petals",
            &[
                ("growth", I(1)),
                ("minecraft:cardinal_direction", S("north")),
            ],
        ),
    ),
    (
        "PINK_PETALS_2_EAST",
        State(
            "minecraft:pink_petals",
            &[
                ("growth", I(1)),
                ("minecraft:cardinal_direction", S("east")),
            ],
        ),
    ),
    (
        "PINK_PETALS_3_SOUTH",
        State(
            "minecraft:pink_petals",
            &[
                ("growth", I(2)),
                ("minecraft:cardinal_direction", S("south")),
            ],
        ),
    ),
    (
        "PINK_PETALS_3_WEST",
        State(
            "minecraft:pink_petals",
            &[
                ("growth", I(2)),
                ("minecraft:cardinal_direction", S("west")),
            ],
        ),
    ),
    (
        "PINK_PETALS_3_NORTH",
        State(
            "minecraft:pink_petals",
            &[
                ("growth", I(2)),
                ("minecraft:cardinal_direction", S("north")),
            ],
        ),
    ),
    (
        "PINK_PETALS_3_EAST",
        State(
            "minecraft:pink_petals",
            &[
                ("growth", I(2)),
                ("minecraft:cardinal_direction", S("east")),
            ],
        ),
    ),
    (
        "PINK_PETALS_4_SOUTH",
        State(
            "minecraft:pink_petals",
            &[
                ("growth", I(3)),
                ("minecraft:cardinal_direction", S("south")),
            ],
        ),
    ),
    (
        "PINK_PETALS_4_WEST",
        State(
            "minecraft:pink_petals",
            &[
                ("growth", I(3)),
                ("minecraft:cardinal_direction", S("west")),
            ],
        ),
    ),
    (
        "PINK_PETALS_4_NORTH",
        State(
            "minecraft:pink_petals",
            &[
                ("growth", I(3)),
                ("minecraft:cardinal_direction", S("north")),
            ],
        ),
    ),
    (
        "PINK_PETALS_4_EAST",
        State(
            "minecraft:pink_petals",
            &[
                ("growth", I(3)),
                ("minecraft:cardinal_direction", S("east")),
            ],
        ),
    ),
    (
        "WILDFLOWERS_1_SOUTH",
        State(
            "minecraft:wildflowers",
            &[
                ("growth", I(0)),
                ("minecraft:cardinal_direction", S("south")),
            ],
        ),
    ),
    (
        "WILDFLOWERS_1_WEST",
        State(
            "minecraft:wildflowers",
            &[
                ("growth", I(0)),
                ("minecraft:cardinal_direction", S("west")),
            ],
        ),
    ),
    (
        "WILDFLOWERS_1_NORTH",
        State(
            "minecraft:wildflowers",
            &[
                ("growth", I(0)),
                ("minecraft:cardinal_direction", S("north")),
            ],
        ),
    ),
    (
        "WILDFLOWERS_1_EAST",
        State(
            "minecraft:wildflowers",
            &[
                ("growth", I(0)),
                ("minecraft:cardinal_direction", S("east")),
            ],
        ),
    ),
    (
        "WILDFLOWERS_2_SOUTH",
        State(
            "minecraft:wildflowers",
            &[
                ("growth", I(1)),
                ("minecraft:cardinal_direction", S("south")),
            ],
        ),
    ),
    (
        "WILDFLOWERS_2_WEST",
        State(
            "minecraft:wildflowers",
            &[
                ("growth", I(1)),
                ("minecraft:cardinal_direction", S("west")),
            ],
        ),
    ),
    (
        "WILDFLOWERS_2_NORTH",
        State(
            "minecraft:wildflowers",
            &[
                ("growth", I(1)),
                ("minecraft:cardinal_direction", S("north")),
            ],
        ),
    ),
    (
        "WILDFLOWERS_2_EAST",
        State(
            "minecraft:wildflowers",
            &[
                ("growth", I(1)),
                ("minecraft:cardinal_direction", S("east")),
            ],
        ),
    ),
    (
        "WILDFLOWERS_3_SOUTH",
        State(
            "minecraft:wildflowers",
            &[
                ("growth", I(2)),
                ("minecraft:cardinal_direction", S("south")),
            ],
        ),
    ),
    (
        "WILDFLOWERS_3_WEST",
        State(
            "minecraft:wildflowers",
            &[
                ("growth", I(2)),
                ("minecraft:cardinal_direction", S("west")),
            ],
        ),
    ),
    (
        "WILDFLOWERS_3_NORTH",
        State(
            "minecraft:wildflowers",
            &[
                ("growth", I(2)),
                ("minecraft:cardinal_direction", S("north")),
            ],
        ),
    ),
    (
        "WILDFLOWERS_3_EAST",
        State(
            "minecraft:wildflowers",
            &[
                ("growth", I(2)),
                ("minecraft:cardinal_direction", S("east")),
            ],
        ),
    ),
    (
        "WILDFLOWERS_4_SOUTH",
        State(
            "minecraft:wildflowers",
            &[
                ("growth", I(3)),
                ("minecraft:cardinal_direction", S("south")),
            ],
        ),
    ),
    (
        "WILDFLOWERS_4_WEST",
        State(
            "minecraft:wildflowers",
            &[
                ("growth", I(3)),
                ("minecraft:cardinal_direction", S("west")),
            ],
        ),
    ),
    (
        "WILDFLOWERS_4_NORTH",
        State(
            "minecraft:wildflowers",
            &[
                ("growth", I(3)),
                ("minecraft:cardinal_direction", S("north")),
            ],
        ),
    ),
    (
        "WILDFLOWERS_4_EAST",
        State(
            "minecraft:wildflowers",
            &[
                ("growth", I(3)),
                ("minecraft:cardinal_direction", S("east")),
            ],
        ),
    ),
    (
        "LEAF_LITTER_1_SOUTH",
        State(
            "minecraft:leaf_litter",
            &[
                ("growth", I(0)),
                ("minecraft:cardinal_direction", S("south")),
            ],
        ),
    ),
    (
        "LEAF_LITTER_1_WEST",
        State(
            "minecraft:leaf_litter",
            &[
                ("growth", I(0)),
                ("minecraft:cardinal_direction", S("west")),
            ],
        ),
    ),
    (
        "LEAF_LITTER_1_NORTH",
        State(
            "minecraft:leaf_litter",
            &[
                ("growth", I(0)),
                ("minecraft:cardinal_direction", S("north")),
            ],
        ),
    ),
    (
        "LEAF_LITTER_1_EAST",
        State(
            "minecraft:leaf_litter",
            &[
                ("growth", I(0)),
                ("minecraft:cardinal_direction", S("east")),
            ],
        ),
    ),
    (
        "LEAF_LITTER_2_SOUTH",
        State(
            "minecraft:leaf_litter",
            &[
                ("growth", I(1)),
                ("minecraft:cardinal_direction", S("south")),
            ],
        ),
    ),
    (
        "LEAF_LITTER_2_WEST",
        State(
            "minecraft:leaf_litter",
            &[
                ("growth", I(1)),
                ("minecraft:cardinal_direction", S("west")),
            ],
        ),
    ),
    (
        "LEAF_LITTER_2_NORTH",
        State(
            "minecraft:leaf_litter",
            &[
                ("growth", I(1)),
                ("minecraft:cardinal_direction", S("north")),
            ],
        ),
    ),
    (
        "LEAF_LITTER_2_EAST",
        State(
            "minecraft:leaf_litter",
            &[
                ("growth", I(1)),
                ("minecraft:cardinal_direction", S("east")),
            ],
        ),
    ),
    (
        "LEAF_LITTER_3_SOUTH",
        State(
            "minecraft:leaf_litter",
            &[
                ("growth", I(2)),
                ("minecraft:cardinal_direction", S("south")),
            ],
        ),
    ),
    (
        "LEAF_LITTER_3_WEST",
        State(
            "minecraft:leaf_litter",
            &[
                ("growth", I(2)),
                ("minecraft:cardinal_direction", S("west")),
            ],
        ),
    ),
    (
        "LEAF_LITTER_3_NORTH",
        State(
            "minecraft:leaf_litter",
            &[
                ("growth", I(2)),
                ("minecraft:cardinal_direction", S("north")),
            ],
        ),
    ),
    (
        "LEAF_LITTER_3_EAST",
        State(
            "minecraft:leaf_litter",
            &[
                ("growth", I(2)),
                ("minecraft:cardinal_direction", S("east")),
            ],
        ),
    ),
    (
        "LEAF_LITTER_4_SOUTH",
        State(
            "minecraft:leaf_litter",
            &[
                ("growth", I(3)),
                ("minecraft:cardinal_direction", S("south")),
            ],
        ),
    ),
    (
        "LEAF_LITTER_4_WEST",
        State(
            "minecraft:leaf_litter",
            &[
                ("growth", I(3)),
                ("minecraft:cardinal_direction", S("west")),
            ],
        ),
    ),
    (
        "LEAF_LITTER_4_NORTH",
        State(
            "minecraft:leaf_litter",
            &[
                ("growth", I(3)),
                ("minecraft:cardinal_direction", S("north")),
            ],
        ),
    ),
    (
        "LEAF_LITTER_4_EAST",
        State(
            "minecraft:leaf_litter",
            &[
                ("growth", I(3)),
                ("minecraft:cardinal_direction", S("east")),
            ],
        ),
    ),
    (
        "VINE_SOUTH",
        State("minecraft:vine", &[("vine_direction_bits", I(1))]),
    ),
    (
        "VINE_WEST",
        State("minecraft:vine", &[("vine_direction_bits", I(2))]),
    ),
    (
        "VINE_NORTH",
        State("minecraft:vine", &[("vine_direction_bits", I(4))]),
    ),
    (
        "VINE_EAST",
        State("minecraft:vine", &[("vine_direction_bits", I(8))]),
    ),
    (
        "BAMBOO_TRUNK",
        State(
            "minecraft:bamboo",
            &[
                ("bamboo_stalk_thickness", S("thick")),
                ("bamboo_leaf_size", S("no_leaves")),
                ("age_bit", B(false)),
            ],
        ),
    ),
    (
        "BAMBOO_TOP_SMALL",
        State(
            "minecraft:bamboo",
            &[
                ("bamboo_stalk_thickness", S("thick")),
                ("bamboo_leaf_size", S("small_leaves")),
                ("age_bit", B(false)),
            ],
        ),
    ),
    (
        "BAMBOO_TOP_LARGE",
        State(
            "minecraft:bamboo",
            &[
                ("bamboo_stalk_thickness", S("thick")),
                ("bamboo_leaf_size", S("large_leaves")),
                ("age_bit", B(false)),
            ],
        ),
    ),
    (
        "BAMBOO_FINAL_LARGE",
        State(
            "minecraft:bamboo",
            &[
                ("bamboo_stalk_thickness", S("thick")),
                ("bamboo_leaf_size", S("large_leaves")),
                ("age_bit", B(true)),
            ],
        ),
    ),
    ("MOSS_CARPET", State("minecraft:moss_carpet", &[])),
    ("SHORT_GRASS", State("minecraft:short_grass", &[])),
    (
        "TALL_GRASS_LOWER",
        State("minecraft:tall_grass", &[("upper_block_bit", B(false))]),
    ),
    (
        "TALL_GRASS_UPPER",
        State("minecraft:tall_grass", &[("upper_block_bit", B(true))]),
    ),
    ("AZALEA", State("minecraft:azalea", &[])),
    ("FLOWERING_AZALEA", State("minecraft:flowering_azalea", &[])),
    (
        "PALE_MOSS_CARPET",
        State(
            "minecraft:pale_moss_carpet",
            &[
                ("pale_moss_carpet_side_east", S("none")),
                ("pale_moss_carpet_side_north", S("none")),
                ("pale_moss_carpet_side_south", S("none")),
                ("pale_moss_carpet_side_west", S("none")),
                ("upper_block_bit", B(false)),
            ],
        ),
    ),
    ("ROOTED_DIRT", State("minecraft:dirt_with_roots", &[])),
    ("HANGING_ROOTS", State("minecraft:hanging_roots", &[])),
    (
        "SMALL_DRIPLEAF_LOWER_SOUTH",
        State(
            "minecraft:small_dripleaf_block",
            &[
                ("upper_block_bit", B(false)),
                ("minecraft:cardinal_direction", S("south")),
            ],
        ),
    ),
    (
        "SMALL_DRIPLEAF_LOWER_WEST",
        State(
            "minecraft:small_dripleaf_block",
            &[
                ("upper_block_bit", B(false)),
                ("minecraft:cardinal_direction", S("west")),
            ],
        ),
    ),
    (
        "SMALL_DRIPLEAF_LOWER_NORTH",
        State(
            "minecraft:small_dripleaf_block",
            &[
                ("upper_block_bit", B(false)),
                ("minecraft:cardinal_direction", S("north")),
            ],
        ),
    ),
    (
        "SMALL_DRIPLEAF_LOWER_EAST",
        State(
            "minecraft:small_dripleaf_block",
            &[
                ("upper_block_bit", B(false)),
                ("minecraft:cardinal_direction", S("east")),
            ],
        ),
    ),
    (
        "SMALL_DRIPLEAF_UPPER_SOUTH",
        State(
            "minecraft:small_dripleaf_block",
            &[
                ("upper_block_bit", B(true)),
                ("minecraft:cardinal_direction", S("south")),
            ],
        ),
    ),
    (
        "SMALL_DRIPLEAF_UPPER_WEST",
        State(
            "minecraft:small_dripleaf_block",
            &[
                ("upper_block_bit", B(true)),
                ("minecraft:cardinal_direction", S("west")),
            ],
        ),
    ),
    (
        "SMALL_DRIPLEAF_UPPER_NORTH",
        State(
            "minecraft:small_dripleaf_block",
            &[
                ("upper_block_bit", B(true)),
                ("minecraft:cardinal_direction", S("north")),
            ],
        ),
    ),
    (
        "SMALL_DRIPLEAF_UPPER_EAST",
        State(
            "minecraft:small_dripleaf_block",
            &[
                ("upper_block_bit", B(true)),
                ("minecraft:cardinal_direction", S("east")),
            ],
        ),
    ),
    ("cactus", Blocks(&["minecraft:cactus"])),
    // Water a sugar cane ground block must touch.
    (
        "reeds_water",
        Blocks(&[
            "minecraft:water",
            "minecraft:flowing_water",
            "minecraft:frosted_ice",
        ]),
    ),
    // Mushrooms grow here regardless of the block below being opaque.
    (
        "mushroom_grow_block",
        Blocks(&[
            "minecraft:mycelium",
            "minecraft:podzol",
            "minecraft:crimson_nylium",
            "minecraft:warped_nylium",
        ]),
    ),
    (
        "bamboo",
        Blocks(&["minecraft:bamboo", "minecraft:bamboo_sapling"]),
    ),
    // Bamboo ground besides the `dirt` and `sand` tags.
    (
        "bamboo_plantable_extra",
        Blocks(&[
            "minecraft:gravel",
            "minecraft:suspicious_gravel",
            "minecraft:bamboo",
            "minecraft:bamboo_sapling",
        ]),
    ),
    ("vine", Blocks(&["minecraft:vine"])),
    // What a melon may replace (vanilla "replaceable" plants on the ground).
    (
        "melon_replaceable",
        Blocks(&[
            "minecraft:air",
            "minecraft:short_grass",
            "minecraft:fern",
            "minecraft:deadbush",
            "minecraft:short_dry_grass",
        ]),
    ),
    // Vanilla "replaceable by trees" besides air (leaves via `leaves`).
    (
        "replaceable_by_trees",
        Blocks(&[
            "minecraft:short_grass",
            "minecraft:fern",
            "minecraft:tall_grass",
            "minecraft:large_fern",
            "minecraft:deadbush",
            "minecraft:bush",
            "minecraft:firefly_bush",
            "minecraft:short_dry_grass",
            "minecraft:tall_dry_grass",
            "minecraft:vine",
            "minecraft:glow_lichen",
            "minecraft:sunflower",
            "minecraft:lilac",
            "minecraft:rose_bush",
            "minecraft:peony",
            "minecraft:hanging_roots",
            "minecraft:leaf_litter",
            "minecraft:water",
            "minecraft:flowing_water",
            "minecraft:seagrass",
            "minecraft:acacia_leaves",
            "minecraft:azalea_leaves",
            "minecraft:azalea_leaves_flowered",
            "minecraft:birch_leaves",
            "minecraft:cherry_leaves",
            "minecraft:dark_oak_leaves",
            "minecraft:jungle_leaves",
            "minecraft:mangrove_leaves",
            "minecraft:oak_leaves",
            "minecraft:pale_oak_leaves",
            "minecraft:spruce_leaves",
        ]),
    ),
    // Rooted dirt targets of the azalea root system besides the `dirt` tag and
    // terracotta.
    (
        "azalea_root_replaceable",
        Blocks(&[
            "minecraft:stone",
            "minecraft:granite",
            "minecraft:diorite",
            "minecraft:andesite",
            "minecraft:tuff",
            "minecraft:deepslate",
            "minecraft:sand",
            "minecraft:red_sand",
            "minecraft:gravel",
            "minecraft:clay",
            "minecraft:snow",
            "minecraft:powder_snow",
        ]),
    ),
    (
        "small_dripleaf_ground",
        Blocks(&["minecraft:clay", "minecraft:moss_block"]),
    ),
    // Surface builders (`surface/`): the_end builder's terrain material.
    ("END_STONE", State("minecraft:end_stone", &[])),
    // Sculk spreading (`features/sculk_patch.rs`; also the vein spreader of
    // `features/multiface.rs`). `SCULK_VEIN` is the faceless state (bits 0).
    ("SCULK", State("minecraft:sculk", &[])),
    (
        "SCULK_VEIN",
        State(
            "minecraft:sculk_vein",
            &[("multi_face_direction_bits", I(0))],
        ),
    ),
    ("SCULK_SENSOR", State("minecraft:sculk_sensor", &[])),
    (
        "SCULK_SHRIEKER_CAN_SUMMON",
        State("minecraft:sculk_shrieker", &[("can_summon", B(true))]),
    ),
    ("sculk_catalyst", Blocks(&["minecraft:sculk_catalyst"])),
    (
        "sculk_growth",
        Blocks(&["minecraft:sculk_sensor", "minecraft:sculk_shrieker"]),
    ),
    // Vanilla's world-generation sculk-replaceable set (Bedrock block names).
    (
        "sculk_replaceable",
        Blocks(&[
            "minecraft:stone",
            "minecraft:granite",
            "minecraft:diorite",
            "minecraft:andesite",
            "minecraft:tuff",
            "minecraft:deepslate",
            "minecraft:cobbled_deepslate",
            "minecraft:polished_deepslate",
            "minecraft:deepslate_bricks",
            "minecraft:cracked_deepslate_bricks",
            "minecraft:deepslate_tiles",
            "minecraft:cracked_deepslate_tiles",
            "minecraft:dirt",
            "minecraft:coarse_dirt",
            "minecraft:grass_block",
            "minecraft:podzol",
            "minecraft:mycelium",
            "minecraft:dirt_with_roots",
            "minecraft:moss_block",
            "minecraft:mud",
            "minecraft:muddy_mangrove_roots",
            "minecraft:sand",
            "minecraft:red_sand",
            "minecraft:gravel",
            "minecraft:clay",
            "minecraft:calcite",
            "minecraft:smooth_basalt",
            "minecraft:dripstone_block",
            "minecraft:sandstone",
            "minecraft:red_sandstone",
            "minecraft:end_stone",
            "minecraft:netherrack",
            "minecraft:basalt",
            "minecraft:blackstone",
            "minecraft:soul_sand",
            "minecraft:soul_soil",
            "minecraft:crimson_nylium",
            "minecraft:warped_nylium",
            "minecraft:hardened_clay",
            "minecraft:white_terracotta",
            "minecraft:orange_terracotta",
            "minecraft:magenta_terracotta",
            "minecraft:light_blue_terracotta",
            "minecraft:yellow_terracotta",
            "minecraft:lime_terracotta",
            "minecraft:pink_terracotta",
            "minecraft:gray_terracotta",
            "minecraft:light_gray_terracotta",
            "minecraft:cyan_terracotta",
            "minecraft:purple_terracotta",
            "minecraft:blue_terracotta",
            "minecraft:brown_terracotta",
            "minecraft:green_terracotta",
            "minecraft:red_terracotta",
            "minecraft:black_terracotta",
        ]),
    ),
];
