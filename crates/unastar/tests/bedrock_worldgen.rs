//! Bedrock generator invariants measured from the local BDS reference world.
#![cfg(feature = "bedrock-worldgen")]

use std::collections::BTreeSet;
use unastar::world::ChunkPos;
use unastar::world::generator::{
    Generator,
    bedrock::{BedrockGenerator, generated::bb},
};

#[test]
fn foundation_matches_observed_bds_layer_boundaries() {
    let reference: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/bds_1_26_51_seed_12345_geology.json")).unwrap();
    let allowed: BTreeSet<u8> = reference["chunks"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|c| c["bedrock_masks"].as_array().unwrap().iter())
        .map(|m| m.as_u64().unwrap() as u8)
        .collect();
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(move || {
            let generator = BedrockGenerator::new(12345);
            let proto = generator.generate_proto(ChunkPos::new(6, -4));
            for z in 0..16 {
                for x in 0..16 {
                    let mask = (0..6).fold(0u8, |mask, dy| {
                        mask | (u8::from(proto.chunk.get_block(x, -64 + dy, z) == bb::BEDROCK)
                            << dy)
                    });
                    assert!(
                        allowed.contains(&mask),
                        "unobserved bedrock topology {mask:#08b} at ({x},{z})"
                    );
                    for y in -64..=0 {
                        assert_ne!(
                            proto.chunk.get_block(x, y, z),
                            bb::STONE,
                            "stone below transition at ({x},{y},{z})"
                        );
                    }
                    for y in 8..=16 {
                        assert_ne!(
                            proto.chunk.get_block(x, y, z),
                            bb::DEEPSLATE,
                            "deepslate above transition at ({x},{y},{z})"
                        );
                    }
                }
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn bedrock_style_vs_bds_reference_ratchets() {
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(|| {
            use unastar::world::generator::{
                ChunkPipeline,
                parity::{Fixture, measure},
            };
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/bds_1_26_51_seed_12345.json");
            let fixture = Fixture::load(&path).unwrap();
            let pipeline = ChunkPipeline::new(
                unastar::world::WorldGenerator::Bedrock { seed: fixture.seed }.build(),
            );
            let m = measure(&fixture, &pipeline).metrics;
            println!("Bedrock BDS seed12345: {m:#?}");
            // D8 (coordinator-delegated, 2026-10-28): the native-calibrated ore
            // frame alone lowers surface height exact (0.857479 -> 0.851333);
            // see the primitive-correction ablation in
            // unastar_bedrock_codegen/README.md. That floor was re-baselined;
            // per-position rule biome checks then raised every metric. The
            // single-block deep sea floor (coordinator-approved, 2026-10-28)
            // halves histogram error but re-baselines terrain within 2 and
            // terrain error: see "Single-block deep sea floor" in that README.
            for (name, value, floor) in [
                ("terrain_exact", m.terrain_exact, 0.8741),
                ("terrain_within_2", m.terrain_within_2, 0.9227),
                ("surface_exact", m.surface_exact, 0.8526),
                ("surface_block", m.surface_block, 0.8432),
                ("surface_biome", m.surface_biome, 0.9950),
            ] {
                assert!(value >= floor, "{name} {value:.6} fell below {floor:.6}");
            }
            assert!(
                m.terrain_mean_abs_error <= 1.2328,
                "terrain error rose: {}",
                m.terrain_mean_abs_error
            );
            assert!(
                m.histogram_l1 <= 0.0405,
                "histogram error rose: {}",
                m.histogram_l1
            );
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn bedrock_decoration_is_independent_of_chunk_request_order() {
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(|| {
            use std::sync::Arc;
            use unastar::world::generator::ChunkPipeline;
            let forward = ChunkPipeline::new(Arc::new(BedrockGenerator::new(12345)));
            let reverse = ChunkPipeline::new(Arc::new(BedrockGenerator::new(12345)));
            let positions = [ChunkPos::new(6, -4), ChunkPos::new(7, -4)];
            let a = positions.map(|p| forward.generate(p).unwrap());
            let b = [
                reverse.generate(positions[1]).unwrap(),
                reverse.generate(positions[0]).unwrap(),
            ];
            for i in 0..2 {
                for y in -64..320 {
                    for z in 0..16 {
                        for x in 0..16 {
                            assert_eq!(
                                a[i].get_block(x, y, z),
                                b[1 - i].get_block(x, y, z),
                                "request order changed ({x},{y},{z}) in {:?}",
                                positions[i]
                            );
                        }
                    }
                }
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn frozen_ocean_temperature_patches_ratchet_against_bds() {
    use unastar::world::generator::{
        bedrock::{
            generated,
            surface::{self, Builder, SurfaceNoises},
        },
        parity::Fixture,
    };
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/bds_1_26_51_seed_12345.json");
    let fixture = Fixture::load(&path).unwrap();
    let noises = SurfaceNoises::new(fixture.seed);
    let java = unastar_noise::java_biomes::JavaBiome::from_name("minecraft:frozen_ocean").unwrap();
    let (mut ice, mut water, mut java_ice, mut java_water) = (0, 0, 0, 0);
    for chunk in &fixture.chunks {
        for i in 0..256 {
            let Some(biome) = generated::biome(chunk.surface_biome[i] as u16) else {
                continue;
            };
            if !matches!(biome.surface, Some(Builder::FrozenOcean(_))) {
                continue;
            }
            let actual = match chunk.surface_block[i].as_str() {
                "minecraft:ice" => true,
                "minecraft:water" => false,
                _ => continue,
            };
            let x = chunk.pos.x * 16 + (i % 16) as i32;
            let z = chunk.pos.z * 16 + (i / 16) as i32;
            let ours = surface::freezes(&noises, biome, x, z);
            let shared =
                unastar_noise::biome_climate::cold_enough_to_snow(java, x, chunk.surface[i], z, 63);
            ice += i32::from(actual && ours);
            water += i32::from(!actual && !ours);
            java_ice += i32::from(actual && shared);
            java_water += i32::from(!actual && !shared);
        }
    }
    println!("frozen-ocean correct ice/water: Bedrock {ice}/{water}, Java {java_ice}/{java_water}");
    // Separate class floors prevent an always-water predicate from passing.
    assert!(ice >= 111, "reference ice matches regressed to {ice}");
    assert!(water >= 561, "reference water matches regressed to {water}");
}

/// BDS fixture chunk (-4,-4) is ocean with 517 kelp and 65 seagrass blocks.
/// The vanilla rules pass `query.heightmap` (the first air above the water)
/// as the input height, so the built-ins must find the sea floor themselves.
/// Feature RNG seeding is unmeasured: the counts are a density band, and the
/// anchoring is checked structurally (plants stand on a solid floor block).
#[test]
fn ocean_kelp_and_seagrass_grow_from_the_floor_like_bds() {
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(|| {
            use std::sync::Arc;
            use unastar::registry::BlockRegistry;
            use unastar::world::generator::ChunkPipeline;
            let registry = BlockRegistry::vanilla();
            let pipeline = ChunkPipeline::new(Arc::new(BedrockGenerator::new(12345)));
            let chunk = pipeline.generate(ChunkPos::new(-4, -4)).unwrap();
            let name = |id: u32| {
                registry
                    .get_by_runtime_id(id)
                    .map_or("?", |entry| entry.string_id.as_str())
            };
            let (mut kelp, mut seagrass) = (0, 0);
            for z in 0..16 {
                for x in 0..16 {
                    for y in -63..320 {
                        let block = name(chunk.get_block(x, y, z));
                        let below = name(chunk.get_block(x, y - 1, z));
                        let plant = match block {
                            "minecraft:kelp" => &mut kelp,
                            "minecraft:seagrass" => &mut seagrass,
                            _ => continue,
                        };
                        *plant += 1;
                        // A plant may sit on another plant: seagrass scattered
                        // from a neighbouring chunk's decoration region can
                        // replace a kelp base when the regions are merged.
                        // It must never hang in water or air.
                        assert!(
                            !["minecraft:air", "minecraft:water"].contains(&below),
                            "{block} at ({x},{y},{z}) floats above {below}"
                        );
                    }
                }
            }
            println!("chunk (-4,-4): kelp {kelp} (BDS 517), seagrass {seagrass} (BDS 65)");
            assert!(
                (259..=1034).contains(&kelp),
                "kelp {kelp} outside 0.5x..2x of BDS 517"
            );
            assert!(
                (22..=195).contains(&seagrass),
                "seagrass {seagrass} outside 1/3x..3x of BDS 65"
            );
        })
        .unwrap()
        .join()
        .unwrap();
}
