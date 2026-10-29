//! Native Java 1.21.11 BIOMES-stage boundaries where equal climate fitness
//! retains the previous search candidate. Measured against stopped-world quart
//! palettes and the pinned public-API section-fill oracle, not self-output hashes.

use unastar::world::ChunkPos;
use unastar::world::generator::{JavaGenerator, JavaTerrainStage};
use unastar_noise::java_biomes::JavaBiome;

#[test]
fn native_section_traversal_retains_surface_biomes_at_cave_fitness_ties() {
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(|| {
            let generator = JavaGenerator::new(12345);
            let plains =
                generator.generate_stage(ChunkPos::new(-130, 27), JavaTerrainStage::Biomes);
            for (qx, qy, qz) in [(1, 9, 0), (2, 23, 2)] {
                assert_eq!(
                    JavaBiome::from_index(plains.biomes.get(qx, qy, qz)),
                    Some(JavaBiome::Plains),
                    "native equal-fitness cave boundary at local quart ({qx},{qy},{qz})",
                );
            }
            let jungle =
                generator.generate_stage(ChunkPos::new(-120, 34), JavaTerrainStage::Biomes);
            assert_eq!(
                JavaBiome::from_index(jungle.biomes.get(1, 16, 1)),
                Some(JavaBiome::SparseJungle),
                "native humidity/depth fitness tie at absolute quart (-479,0,137)",
            );
        })
        .unwrap()
        .join()
        .unwrap();
}
