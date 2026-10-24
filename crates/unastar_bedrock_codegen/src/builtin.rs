//! Built-in features: identifiers BDS places without a JSON definition.
//!
//! BDS ships these as engine behaviour keyed by identifier (`places_feature`
//! points at them, no `features` document exists). unastar implements each one
//! in Rust under `unastar::world::generator::bedrock::builtin`; the emitted code
//! calls `builtin::<rust_name>(ctx, pos)`. This list is the contract between
//! the two: a reachable built-in missing here is a build error, a listed one
//! missing in Rust is a compile error.
//!
//! The list covers every built-in reachable from overworld feature rules in BDS
//! 1.26.51 (nether-only and rules disabled by a `deprecated` biome tag never
//! run in an overworld chunk).

/// Built-in feature identifiers implemented in Rust.
pub const BUILTIN_FEATURES: &[&str] = &[
    "minecraft:azalea_root_system_feature",
    "minecraft:bamboo_feature",
    "minecraft:brown_mushroom_feature",
    "minecraft:cactus_feature",
    "minecraft:clay_feature",
    "minecraft:coral_crust_feature",
    "minecraft:coral_feature",
    "minecraft:coral_hang_feature",
    "minecraft:dead_bush_feature",
    "minecraft:desert_well_feature",
    "minecraft:dripleaf_feature",
    "minecraft:dripstone_cluster_feature",
    "minecraft:eyeblossom_feature",
    "minecraft:firefly_bush_cluster_feature",
    "minecraft:firefly_bush_water_cluster_feature",
    "minecraft:grass_disc_feature",
    "minecraft:gravel_feature",
    "minecraft:huge_mushroom_feature",
    "minecraft:ice_patch_feature",
    "minecraft:ice_spike_feature",
    "minecraft:kelp_feature",
    "minecraft:large_dripstone_feature",
    "minecraft:leaf_litter_feature",
    "minecraft:legacy:bamboo_jungle_tree_feature",
    "minecraft:legacy:birch_forest_mutated_tree_feature",
    "minecraft:legacy:birch_forest_tree_feature",
    "minecraft:legacy:extreme_hills_plus_trees_tree_feature",
    "minecraft:legacy:flower_forest_flower_feature",
    "minecraft:legacy:flower_forest_foliage_feature",
    "minecraft:legacy:flower_forest_tree_feature",
    "minecraft:legacy:forest_foliage_feature",
    "minecraft:legacy:forest_rock_feature",
    "minecraft:legacy:forest_tree_feature",
    "minecraft:legacy:ice_feature",
    "minecraft:legacy:ice_tree_feature",
    "minecraft:legacy:jungle_edge_tree_feature",
    "minecraft:legacy:jungle_tree_feature",
    "minecraft:legacy:meadow_trees_feature",
    "minecraft:legacy:mega_taiga_tree_feature",
    "minecraft:legacy:mesa_plateau_stone_tree_feature",
    "minecraft:legacy:overworld_flower_feature",
    "minecraft:legacy:plains_flower_feature",
    "minecraft:legacy:plains_tree_feature",
    "minecraft:legacy:redwood_taiga_mutated_tree_feature",
    "minecraft:legacy:savanna_mutated_tree_feature",
    "minecraft:legacy:savanna_tree_feature",
    "minecraft:legacy:springs_feature",
    "minecraft:legacy:swamp_flower_feature",
    "minecraft:legacy:swamp_foliage_feature",
    "minecraft:legacy:taiga_tree_feature",
    "minecraft:meadow_flower_feature",
    "minecraft:melon_feature",
    "minecraft:moss_vegetation_feature",
    "minecraft:pale_moss_vegetation_feature",
    "minecraft:pickle_feature",
    "minecraft:pink_petals_feature",
    "minecraft:podzol_area_feature",
    "minecraft:pointed_dripstone_feature",
    "minecraft:pumpkin_feature",
    "minecraft:red_mushroom_feature",
    "minecraft:reeds_feature",
    "minecraft:sand_feature",
    "minecraft:sea_anemone_feature",
    "minecraft:seagrass_feature",
    "minecraft:shorter_seagrass_feature",
    "minecraft:shortest_seagrass_feature",
    "minecraft:sulfur_pool",
    "minecraft:sulfur_spike_cluster_feature",
    "minecraft:sulfur_spike_feature",
    "minecraft:sulfur_spring_trail_to_surface_feature",
    "minecraft:sweet_berry_bush_feature",
    "minecraft:taller_seagrass_feature",
    "minecraft:vines_feature",
    "minecraft:vines_single_face_feature",
    "minecraft:waterlily_feature",
    "minecraft:wildflowers_feature",
];

/// Rust function name of a built-in: the identifier without its namespace,
/// `:` replaced by `_` (`minecraft:legacy:ice_feature` -> `legacy_ice_feature`).
pub fn rust_name(identifier: &str) -> String {
    identifier
        .strip_prefix("minecraft:")
        .unwrap_or(identifier)
        .replace(':', "_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_names_are_unique_identifiers() {
        let mut names: Vec<String> = BUILTIN_FEATURES.iter().map(|id| rust_name(id)).collect();
        assert!(names.iter().all(|n| syn::parse_str::<syn::Ident>(n).is_ok()));
        let len = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), len);
        assert_eq!(rust_name("minecraft:legacy:ice_feature"), "legacy_ice_feature");
    }
}
