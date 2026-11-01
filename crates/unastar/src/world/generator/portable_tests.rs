use super::*;
use crate::world::chunk::BlockStateSpace;
use crate::world::{ChunkPos, WorldGenerator};
use structures::NativeStructureSource;

#[test]
fn configured_portable_presets_match_direct_native_terrain_in_all_three_dimensions() {
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(|| {
            for dimension in [
                JavaDimension::Overworld,
                JavaDimension::Nether,
                JavaDimension::End,
            ] {
                let configured = WorldGenerator::JavaTerrain {
                    seed: 7,
                    preset: dimension,
                }
                .build();
                let direct = JavaGenerator::with_options(
                    7,
                    JavaGenerationOptions {
                        dimension,
                        generate_structures: false,
                        biome_features: true,
                    },
                );
                let portable = JavaGenerator::portable(7, dimension);
                assert!(!portable.template_features_enabled());
                assert!(direct.template_features_enabled());
                assert!(configured.decorates());
                assert!(portable.structure_starts(-1, 2).is_empty());
                let position = ChunkPos::new(-1, 2);
                let expected = direct.generate_proto(position);
                let actual = configured.generate_proto(position);
                let repeated = portable.generate_proto(position);
                assert_eq!(actual.chunk.state_space(), BlockStateSpace::Java);
                assert_eq!(actual.chunk.block_bounds(), dimension.block_bounds());
                for y in dimension.block_bounds() {
                    for x in 0..16 {
                        for z in 0..16 {
                            assert_eq!(
                                actual.chunk.get_block(x, y as i16, z),
                                expected.chunk.get_block(x, y as i16, z)
                            );
                            assert_eq!(
                                actual.chunk.get_block(x, y as i16, z),
                                repeated.chunk.get_block(x, y as i16, z)
                            );
                        }
                    }
                }
                let protos = region::SLOT_OFFSETS.map(|(dx, dz)| {
                    ProtoChunk::empty_in_space(
                        ChunkPos::new(position.x + dx, position.z + dz),
                        0,
                        BlockStateSpace::Java,
                    )
                });
                let refs = std::array::from_fn(|slot| &protos[slot]);
                let mut region = DecorationRegion::new(
                    7,
                    position,
                    refs,
                    &unastar_noise::java_blocks::JAVA_STATE_CLASSES,
                );
                let world = features::RegionWorld::new(&mut region, dimension, &portable);
                assert!(!unastar_noise::features::FeatureWorld::template_features_enabled(&world));
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
