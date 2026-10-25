//! Structure type implementations (`Structure.findGenerationPoint` and their
//! pieces), one module per structure family, and the dispatch over
//! [`StructureKind`].

pub mod end_city;
pub mod fortress;
pub mod jigsaw_place;
pub mod mineshaft;
pub mod ocean_monument;
pub mod scattered;
pub mod stronghold;
pub mod template_structures;
pub mod woodland_mansion;

use super::context::StructureContext;
use super::piece::StructurePiece;
use super::start::{GenContext, GenerationStub, StructureDef, StructureKind};
use super::transform::BoundingBox;
use crate::features::{FeatureRandom, FeatureWorld};

/// `Structure.findGenerationPoint` for the structure of `g.def`.
pub fn find_generation_point<'a>(g: &mut GenContext<'a>) -> Option<GenerationStub<'a>> {
    match g.def.kind {
        StructureKind::Jigsaw(_) => jigsaw_place::find_generation_point(g),
        StructureKind::BuriedTreasure => scattered::find_buried_treasure_point(g),
        StructureKind::DesertPyramid => scattered::find_desert_pyramid_point(g),
        StructureKind::EndCity => end_city::find_generation_point(g),
        StructureKind::Fortress => fortress::find_generation_point(g),
        StructureKind::Igloo => scattered::find_igloo_point(g),
        StructureKind::JungleTemple => scattered::find_jungle_temple_point(g),
        StructureKind::Mineshaft { .. } => mineshaft::find_generation_point(g),
        StructureKind::NetherFossil { .. } => template_structures::find_nether_fossil_point(g),
        StructureKind::OceanMonument => ocean_monument::find_generation_point(g),
        StructureKind::OceanRuin { .. } => template_structures::find_ocean_ruin_point(g),
        StructureKind::RuinedPortal { .. } => template_structures::find_ruined_portal_point(g),
        StructureKind::Shipwreck { .. } => template_structures::find_shipwreck_point(g),
        StructureKind::Stronghold => stronghold::find_generation_point(g),
        StructureKind::SwampHut => scattered::find_swamp_hut_point(g),
        StructureKind::WoodlandMansion => woodland_mansion::find_generation_point(g),
    }
}

/// `Structure.afterPlace` (desert pyramids and woodland mansions).
pub fn after_place(
    def: &StructureDef,
    world: &mut dyn FeatureWorld,
    ctx: &dyn StructureContext,
    rng: &mut FeatureRandom,
    chunk_box: &BoundingBox,
    chunk: (i32, i32),
    pieces: &[Box<dyn StructurePiece>],
) {
    match def.kind {
        StructureKind::DesertPyramid => {
            scattered::after_place_desert_pyramid(world, ctx, rng, chunk_box, chunk, pieces)
        }
        StructureKind::WoodlandMansion => {
            woodland_mansion::after_place(world, ctx, rng, chunk_box, chunk, pieces)
        }
        _ => {}
    }
}
