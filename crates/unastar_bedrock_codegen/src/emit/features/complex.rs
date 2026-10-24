//! Block-placing feature types whose algorithm lives in the runtime
//! (`unastar::world::generator::bedrock::features::<type>`). Each emitter
//! turns the strict JSON into a parameter literal and a `place` call; the
//! expression it returns evaluates to `Option<BlockPos>` with `ctx` and `pos`
//! in scope.

use super::super::Emitter;
use crate::error::{Error, Result};
use crate::ir::Obj;
use proc_macro2::TokenStream;

// The emitters live next to this file (`emit/features/<type>.rs`); a plain
// `mod x;` in this non-mod.rs file would resolve to `complex/x.rs`.
#[path = "carver.rs"]
mod carver;
#[path = "fossil.rs"]
mod fossil;
#[path = "geode.rs"]
mod geode;
#[path = "growing_plant.rs"]
mod growing_plant;
#[path = "horizontal_tree_decoration.rs"]
mod horizontal_tree_decoration;
#[path = "multiface.rs"]
mod multiface;
#[path = "partially_exposed_blob.rs"]
mod partially_exposed_blob;
#[path = "sculk_patch.rs"]
mod sculk_patch;
#[path = "structure_template.rs"]
mod structure_template;
#[path = "tree.rs"]
mod tree;
#[path = "vegetation_patch.rs"]
mod vegetation_patch;

pub fn emit(em: &mut Emitter<'_>, kind: &str, o: &Obj<'_>) -> Result<TokenStream> {
    match kind {
        "minecraft:cave_carver_feature" => carver::emit(em, o, carver::Kind::Cave),
        "minecraft:underwater_cave_carver_feature" => carver::emit(em, o, carver::Kind::UnderwaterCave),
        "minecraft:nether_cave_carver_feature" => carver::emit(em, o, carver::Kind::NetherCave),
        "minecraft:geode_feature" => geode::emit(em, o),
        "minecraft:growing_plant_feature" => growing_plant::emit(em, o),
        "minecraft:vegetation_patch_feature" => vegetation_patch::emit(em, o),
        "minecraft:multiface_feature" => multiface::emit(em, o),
        "minecraft:partially_exposed_blob_feature" => partially_exposed_blob::emit(em, o),
        "minecraft:horizontal_tree_decoration_feature" => horizontal_tree_decoration::emit(em, o),
        "minecraft:sculk_patch_feature" => sculk_patch::emit(em, o),
        "minecraft:structure_template_feature" => structure_template::emit(em, o),
        "minecraft:fossil_feature" => fossil::emit(em, o),
        "minecraft:tree_feature" => tree::emit(em, o),
        _ => Err(Error::data(o.origin, format!("feature type {kind} has no emitter"))),
    }
}
