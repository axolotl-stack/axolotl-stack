//! Real Java 1.21.11 NOISE-stage material boundaries, observed through native
//! fillFromNoise with structures disabled and Blender.empty. Full reference:
//! native-dimension-corpus-20261027/grid-overworld-noise-seed0.json, SHA256
//! bf53c7e117cb9cdf64947492edff22880ed9d8efa2e4c8d11151b7815e56ddfc.

use unastar::world::ChunkPos;
use unastar::world::generator::{JavaDimension, JavaGenerationOptions, JavaGenerator, JavaTerrainStage};
use unastar_noise::features::java_blocks::states;

#[test]
fn native_interpolation_precedes_surface_material_selection_at_sea_level() {
    std::thread::Builder::new().stack_size(64 << 20).spawn(|| {
        let generator = JavaGenerator::with_options(0, JavaGenerationOptions {
            dimension: JavaDimension::Overworld,
            generate_structures: false,
            biome_features: true,
        });
        let proto = generator.generate_stage(ChunkPos::new(0, 0), JavaTerrainStage::Noise);
        // Both native material densities are positive. Interpolating the already
        // squeezed final function incorrectly chooses source water below sea level
        // and air at sea level, respectively.
        assert_eq!(proto.chunk.get_block(0, 62, 13), u32::from(states::STONE.0),
            "native stone must not become source water below sea level");
        assert_eq!(proto.chunk.get_block(7, 63, 14), u32::from(states::STONE.0),
            "native stone must not become air at sea level");
    }).unwrap().join().unwrap();
}

#[test]
fn native_vein_filler_keeps_java_state_identity() {
    std::thread::Builder::new().stack_size(64 << 20).spawn(|| {
        let generator = JavaGenerator::with_options(0, JavaGenerationOptions {
            dimension: JavaDimension::Overworld,
            generate_structures: false,
            biome_features: true,
        });
        let proto = generator.generate_stage(ChunkPos::new(-100001, 99999), JavaTerrainStage::Noise);
        // Native block (-1600016,-25,1599989). A Bedrock tuff ID interpreted
        // as a Java ID became a property-bearing chiseled bookshelf.
        assert_eq!(proto.chunk.get_block(0, -25, 5), u32::from(states::TUFF.0));
    }).unwrap().join().unwrap();
}
