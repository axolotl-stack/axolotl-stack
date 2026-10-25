//! BDS built-in features: identifiers that `places_feature` names without a
//! JSON definition. Each is a Rust function registered under its identifier
//! (`unastar_bedrock_codegen::builtin::BUILTIN_FEATURES`): the compiled rules
//! call `builtin::<name>(ctx, pos)` where `<name>` is the identifier without
//! `minecraft:` and with `:` replaced by `_`.
//!
//! Every function has the feature signature
//! `fn<W: BlockWorld>(&mut Ctx<'_, W>, BlockPos) -> Option<BlockPos>`:
//! `Some(position)` when something was placed.
//!
//! The behaviour is engine code in BDS, reproduced from observed vanilla
//! behaviour; numbers that BDS does not ship are measured against BDS
//! fixtures (owner decision D4) and odd behaviour is logged in
//! `thoughts/shared/research/bds-quirks.md`.

mod aquatic;
mod legacy;
mod plants;
mod terrain;

pub use aquatic::{
    clay_feature, coral_crust_feature, coral_feature, coral_hang_feature, grass_disc_feature,
    gravel_feature, kelp_feature, pickle_feature, sand_feature, sea_anemone_feature,
    seagrass_feature, shorter_seagrass_feature, shortest_seagrass_feature, taller_seagrass_feature,
    waterlily_feature,
};
pub use legacy::{
    legacy_bamboo_jungle_tree_feature, legacy_birch_forest_mutated_tree_feature,
    legacy_birch_forest_tree_feature, legacy_extreme_hills_plus_trees_tree_feature,
    legacy_flower_forest_flower_feature, legacy_flower_forest_foliage_feature,
    legacy_flower_forest_tree_feature, legacy_forest_foliage_feature, legacy_forest_rock_feature,
    legacy_forest_tree_feature, legacy_ice_feature, legacy_ice_tree_feature,
    legacy_jungle_edge_tree_feature, legacy_jungle_tree_feature, legacy_meadow_trees_feature,
    legacy_mega_taiga_tree_feature, legacy_mesa_plateau_stone_tree_feature,
    legacy_overworld_flower_feature, legacy_plains_flower_feature, legacy_plains_tree_feature,
    legacy_redwood_taiga_mutated_tree_feature, legacy_savanna_mutated_tree_feature,
    legacy_savanna_tree_feature, legacy_springs_feature, legacy_swamp_flower_feature,
    legacy_swamp_foliage_feature, legacy_taiga_tree_feature,
};
pub use plants::{
    azalea_root_system_feature, bamboo_feature, brown_mushroom_feature, cactus_feature,
    dead_bush_feature, dripleaf_feature, eyeblossom_feature, firefly_bush_cluster_feature,
    firefly_bush_water_cluster_feature, leaf_litter_feature, meadow_flower_feature, melon_feature,
    moss_vegetation_feature, pale_moss_vegetation_feature, pink_petals_feature,
    podzol_area_feature, pumpkin_feature, red_mushroom_feature, reeds_feature,
    sweet_berry_bush_feature, vines_feature, vines_single_face_feature, wildflowers_feature,
};
pub use terrain::{
    desert_well_feature, dripstone_cluster_feature, huge_mushroom_feature, ice_patch_feature,
    ice_spike_feature, large_dripstone_feature, pointed_dripstone_feature, sulfur_pool,
    sulfur_spike_cluster_feature, sulfur_spike_feature, sulfur_spring_trail_to_surface_feature,
};
