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
    ("ORANGE_TERRACOTTA", State("minecraft:orange_terracotta", &[])),
    ("YELLOW_TERRACOTTA", State("minecraft:yellow_terracotta", &[])),
    ("BROWN_TERRACOTTA", State("minecraft:brown_terracotta", &[])),
    ("RED_TERRACOTTA", State("minecraft:red_terracotta", &[])),
    ("LIGHT_GRAY_TERRACOTTA", State("minecraft:light_gray_terracotta", &[])),
    ("air", Blocks(&["minecraft:air"])),
    ("water", Blocks(&["minecraft:water", "minecraft:flowing_water"])),
    ("lava", Blocks(&["minecraft:lava", "minecraft:flowing_lava"])),
    ("ice", Blocks(&["minecraft:ice", "minecraft:packed_ice", "minecraft:blue_ice", "minecraft:frosted_ice"])),
    ("dirt_like", Tags(&["dirt"])),
    ("sand_like", Tags(&["sand"])),
    ("stone_like", Tags(&["stone"])),
    ("leaves", Tags(&["minecraft:leaves"])),
    ("logs", Tags(&["log"])),
    ("snow_like", Blocks(&["minecraft:snow_layer", "minecraft:snow", "minecraft:powder_snow"])),
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
    ("OAK_LEAVES", State("minecraft:oak_leaves", &[("persistent_bit", B(false)), ("update_bit", B(false))])),
    ("OAK_LOG", State("minecraft:oak_log", &[("pillar_axis", S("y"))])),
    ("KELP_AGE_0", State("minecraft:kelp", &[("kelp_age", I(0))])),
];
