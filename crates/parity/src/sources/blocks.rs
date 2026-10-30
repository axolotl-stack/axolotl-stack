//! Block rows: every block in the protocolgen 1.26.51 native palette
//! (`unastar_data::blocks::ALL_BLOCKS`), grouped into behaviour families.
//!
//! The family rules below only classify; they never add or drop a block. The
//! first matching rule wins, and a block that matches none is a
//! `building_block` (placed, broken and dropped, with no behaviour of its own).

use unastar_data::blocks::{ALL_BLOCKS, BlockData};

use crate::catalog::Row;

pub fn rows() -> Vec<Row> {
    ALL_BLOCKS
        .iter()
        .map(|block| Row::new(block.identifier, family(block)))
        .collect()
}

/// Blocks whose behaviour is a redstone power source, consumer or transmitter.
const REDSTONE: &[&str] = &[
    "redstone_wire",
    "redstone_block",
    "redstone_torch",
    "unlit_redstone_torch",
    "redstone_lamp",
    "lit_redstone_lamp",
    "unpowered_repeater",
    "powered_repeater",
    "unpowered_comparator",
    "powered_comparator",
    "observer",
    "piston",
    "sticky_piston",
    "piston_arm_collision",
    "sticky_piston_arm_collision",
    "moving_block",
    "lever",
    "daylight_detector",
    "daylight_detector_inverted",
    "target",
    "trip_wire",
    "tripwire_hook",
    "noteblock",
    "dispenser",
    "dropper",
    "hopper",
    "crafter",
    "tnt",
    "sculk_sensor",
    "calibrated_sculk_sensor",
    "lightning_rod",
];

/// Blocks with an inventory, a menu or another block-entity driven function.
const FUNCTIONAL: &[&str] = &[
    "chest",
    "trapped_chest",
    "ender_chest",
    "barrel",
    "furnace",
    "lit_furnace",
    "blast_furnace",
    "lit_blast_furnace",
    "smoker",
    "lit_smoker",
    "brewing_stand",
    "beacon",
    "enchanting_table",
    "anvil",
    "chipped_anvil",
    "damaged_anvil",
    "grindstone",
    "stonecutter_block",
    "loom",
    "cartography_table",
    "smithing_table",
    "crafting_table",
    "fletching_table",
    "lectern",
    "jukebox",
    "composter",
    "cauldron",
    "campfire",
    "soul_campfire",
    "chiseled_bookshelf",
    "decorated_pot",
    "vault",
    "trial_spawner",
    "mob_spawner",
    "bell",
    "respawn_anchor",
    "lodestone",
    "conduit",
    "beehive",
    "bee_nest",
    "suspicious_sand",
    "suspicious_gravel",
    "flower_pot",
    "frame",
    "glow_frame",
    "skull",
    "end_portal_frame",
    "brewing_stand",
    "enchanting_table",
];

/// Operator and technical blocks.
const TECHNICAL: &[&str] = &[
    "barrier",
    "command_block",
    "chain_command_block",
    "repeating_command_block",
    "jigsaw",
    "light_block",
    "allow",
    "deny",
    "border_block",
    "bedrock",
    "invisible_bedrock",
    "info_update",
    "info_update2",
    "reserved6",
    "unknown",
    "client_request_placeholder_block",
];

/// Crops, stems and other plants that grow on random ticks.
const GROWING: &[&str] = &[
    "wheat",
    "carrots",
    "potatoes",
    "beetroot",
    "melon_stem",
    "pumpkin_stem",
    "torchflower_crop",
    "pitcher_crop",
    "nether_wart",
    "sweet_berry_bush",
    "cocoa",
    "bamboo",
    "bamboo_sapling",
    "reeds",
    "cactus",
    "kelp",
    "cave_vines",
    "cave_vines_body_with_berries",
    "cave_vines_head_with_berries",
    "chorus_flower",
    "chorus_plant",
    "twisting_vines",
    "weeping_vines",
    "vine",
    "small_amethyst_bud",
    "medium_amethyst_bud",
    "large_amethyst_bud",
    "amethyst_cluster",
    "budding_amethyst",
    "pointed_dripstone",
    "big_dripleaf",
    "small_dripleaf_block",
];

/// Blocks that spread, decay, melt or otherwise change on their own.
const SURFACE: &[&str] = &[
    "grass_block",
    "mycelium",
    "podzol",
    "dirt",
    "coarse_dirt",
    "rooted_dirt",
    "farmland",
    "grass_path",
    "crimson_nylium",
    "warped_nylium",
    "snow_layer",
    "snow",
    "ice",
    "frosted_ice",
    "packed_ice",
    "blue_ice",
    "powder_snow",
    "sponge",
    "wet_sponge",
    "mud",
    "muddy_mangrove_roots",
    "moss_block",
    "pale_moss_block",
    "magma",
    "soul_sand",
    "soul_soil",
    "slime",
    "honey_block",
    "web",
    "scaffolding",
    "ladder",
    "turtle_egg",
    "sniffer_egg",
    "frog_spawn",
    "dragon_egg",
    "sculk",
    "sculk_vein",
    "sculk_catalyst",
    "sculk_shrieker",
    "bubble_column",
    "cake",
    "fire",
    "soul_fire",
    "portal",
    "end_portal",
    "end_gateway",
    "obsidian",
    "crying_obsidian",
];

/// Plants and ground cover without growth stages.
const PLANT_MARKERS: &[&str] = &[
    "flower",
    "tulip",
    "orchid",
    "allium",
    "bluet",
    "daisy",
    "poppy",
    "dandelion",
    "cornflower",
    "lily",
    "rose",
    "peony",
    "lilac",
    "sunflower",
    "fern",
    "short_grass",
    "tall_grass",
    "deadbush",
    "mushroom",
    "fungus",
    "roots",
    "sprouts",
    "azalea",
    "spore_blossom",
    "hanging_moss",
    "glow_lichen",
    "seagrass",
    "sea_pickle",
    "waterlily",
    "bush",
    "petals",
    "leaf_litter",
    "eyeblossom",
    "wildflowers",
    "dry_grass",
];

/// Education Edition content (chemistry, elements, boards).
fn is_education(name: &str) -> bool {
    name.starts_with("element_")
        || name.contains("chemistry")
        || name.contains("chemical")
        || name.contains("compound_creator")
        || name.contains("material_reducer")
        || name.contains("lab_table")
        || name.contains("colored_torch")
        || name.starts_with("hard_")
        || name == "chalkboard"
        || name == "underwater_torch"
        || name == "underwater_tnt"
        || name == "camera"
}

fn family(block: &BlockData) -> &'static str {
    let name = block
        .identifier
        .strip_prefix("minecraft:")
        .unwrap_or(block.identifier);
    let ends = |suffix: &str| name.ends_with(suffix);
    let has = |part: &str| name.contains(part);
    let listed = |list: &[&str]| list.contains(&name);

    if name == "air" || name.ends_with("_air") {
        "air"
    } else if is_education(name) {
        "education"
    } else if block.liquid || matches!(name, "water" | "flowing_water" | "lava" | "flowing_lava") {
        "liquids"
    } else if listed(TECHNICAL) || name.starts_with("light_block") {
        "technical"
    } else if ends("trapdoor") {
        "trapdoors"
    } else if ends("_door") {
        "doors"
    } else if ends("fence_gate") {
        "fence_gates"
    } else if ends("_fence") {
        "fences"
    } else if ends("_stairs") {
        "stairs"
    } else if ends("_slab") {
        "slabs"
    } else if ends("_button") {
        "buttons"
    } else if ends("pressure_plate") {
        "pressure_plates"
    } else if has("sign") {
        "signs"
    } else if has("banner") {
        "banners"
    } else if name == "bed" || ends("_bed") {
        "beds"
    } else if has("candle") {
        "candles"
    } else if ends("rail") {
        "rails"
    } else if listed(REDSTONE) || has("copper_bulb") {
        "redstone"
    } else if listed(FUNCTIONAL) || ends("shulker_box") || ends("_shelf") || has("cauldron") {
        "functional"
    } else if listed(GROWING) {
        "crops_growing"
    } else if has("sapling") || has("propagule") {
        "saplings"
    } else if has("leaves") {
        "leaves"
    } else if has("coral") {
        "coral"
    } else if ends("_wall") {
        "walls"
    } else if ends("pane") || ends("_bars") {
        "panes_bars"
    } else if has("torch") || has("lantern") || ends("_chain") || name == "chain" || has("end_rod")
    {
        "attached_lights"
    } else if listed(SURFACE) || has("concrete_powder") {
        "surface_dynamic"
    } else if has("copper") {
        "copper_oxidising"
    } else if ends("_ore") || name == "ancient_debris" {
        "ores"
    } else if ends("_log")
        || ends("_wood")
        || ends("_stem")
        || ends("_hyphae")
        || ends("bamboo_block")
    {
        "logs_stems"
    } else if ends("carpet") {
        "carpets"
    } else if PLANT_MARKERS.iter().any(|marker| has(marker)) {
        "plants"
    } else if block.falling {
        "falling"
    } else if has("head") || has("skull") {
        "functional"
    } else {
        "building_blocks"
    }
}
