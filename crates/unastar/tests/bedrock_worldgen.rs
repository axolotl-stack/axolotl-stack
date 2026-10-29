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
            // Open-space snapping then raised the terrain floors again.
            // Per-rule feature seeding (coordinator-approved, 2026-10-28) is
            // proven against native probe worlds and cuts histogram error by a
            // quarter, but re-baselines terrain within 2, terrain error and the
            // surface floors while feature algorithms still differ: see
            // "Per-rule seeding re-baseline" in that README. Measuring the
            // surface-relative threshold below water then raised the terrain
            // floors. Native scatter distributions (coordinator-approved)
            // raise the surface floors and lower terrain within 2 and terrain
            // error by one column: see "Native scatter distributions". Upper-
            // exclusive vegetation-patch ranges then raised the terrain floors.
            // Native-proven draw-order fixes (each pinned by a regression
            // against the BDS world) re-baseline these floors and the
            // ceilings below; each is tabled under "Roofed forest draw order"
            // in the codegen README. Native carver source seeding moves
            // tunnels onto BDS's caves but carves less volume than BDS so
            // far: see "Carver source seeding".
            for (name, value, floor) in [
                ("terrain_exact", m.terrain_exact, 0.8850),
                ("terrain_within_2", m.terrain_within_2, 0.9240),
                ("surface_exact", m.surface_exact, 0.8513),
                ("surface_block", m.surface_block, 0.8472),
                ("surface_biome", m.surface_biome, 0.9950),
            ] {
                assert!(value >= floor, "{name} {value:.6} fell below {floor:.6}");
            }
            assert!(
                m.terrain_mean_abs_error <= 1.2059,
                "terrain error rose: {}",
                m.terrain_mean_abs_error
            );
            // Native-sized geodes leave deepslate their oversized cores used to
            // hollow out, and magma no longer covers over-generated sea-floor
            // stone, unmasking existing excesses (D8, one ceiling each time):
            // see "Geode point count" and "Magma depth below solid ground" in
            // that README. Block-exact geodes then lowered the error again.
            // Upper-exclusive vegetation-patch ranges stop overgrown moss
            // hiding a moss deficit (D8): see "Vegetation patch ranges". The
            // native dripleaf draws then cost the terrain error a fraction of
            // a column (D8): see "Dripleaf draws".
            assert!(
                m.histogram_l1 <= 0.03696,
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

/// Generate one chunk of the seed-12345 Bedrock-style world on a big stack.
fn generate_chunk(x: i32, z: i32) -> unastar::world::chunk::Chunk {
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(move || {
            use std::sync::Arc;
            use unastar::world::generator::ChunkPipeline;
            ChunkPipeline::new(Arc::new(BedrockGenerator::new(12345)))
                .generate(ChunkPos::new(x, z))
                .unwrap()
        })
        .unwrap()
        .join()
        .unwrap()
}

/// Blocks named `name` in the chunk-local box `x0..=x1, y0..=y1, z0..=z1`.
fn blocks_named(
    chunk: &unastar::world::chunk::Chunk,
    name: &str,
    (x0, x1): (u8, u8),
    (y0, y1): (i16, i16),
    (z0, z1): (u8, u8),
) -> BTreeSet<(u8, i16, u8)> {
    let registry = unastar::registry::BlockRegistry::vanilla();
    let mut out = BTreeSet::new();
    for x in x0..=x1 {
        for z in z0..=z1 {
            for y in y0..=y1 {
                let id = chunk.get_block(x, y, z);
                if registry
                    .get_by_runtime_id(id)
                    .is_some_and(|e| e.string_id == name)
                {
                    out.insert((x, y, z));
                }
            }
        }
    }
    out
}

/// Natural seed-12345 BDS world, roofed forest chunk (4,-5): the first
/// iteration of `roofed_forest_surface_roofed_tree_feature_rules` grows a
/// fancy oak whose trunk starts at y 124 in column (77,-66).
#[test]
fn roofed_forest_chunk_starts_with_the_native_trees() {
    let chunk = generate_chunk(4, -5);
    let logs = blocks_named(&chunk, "minecraft:oak_log", (13, 13), (-63, 319), (14, 14));
    assert_eq!(logs.first().map(|&(_, y, _)| y), Some(124));
}

/// Natural seed-12345 BDS world, roofed forest chunk (4,-3): the first dark
/// oak (base (76,127,-36)) block for block: a straight 2x2 trunk of eight
/// logs and five hanging branch stubs. Its rule iteration draws the tree pick,
/// the 1-in-6 vine roll and one extra draw before the trunk, so this fails
/// when any of those draws is missing or misplaced.
#[test]
fn roofed_forest_first_dark_oak_is_block_exact() {
    let chunk = generate_chunk(4, -3);
    // Chunk-local x 11..=14, z 11..=14 is block x 75..=78, z -37..=-34.
    let actual = blocks_named(
        &chunk,
        "minecraft:dark_oak_log",
        (11, 14),
        (120, 140),
        (11, 14),
    );
    let mut expected = BTreeSet::new();
    // (local x, local z, lowest y, highest y) per column, from the BDS world.
    for (x, z, lo, hi) in [
        (12u8, 12u8, 127i16, 134i16),
        (12, 13, 127, 134),
        (13, 12, 127, 134),
        (13, 13, 127, 134),
        (11, 12, 131, 133),
        (11, 14, 131, 133),
        (12, 14, 131, 133),
        (13, 14, 132, 133),
        (14, 11, 132, 133),
    ] {
        for y in lo..=hi {
            expected.insert((x, y, z));
        }
    }
    assert_eq!(actual, expected);
}

/// Natural seed-12345 BDS world, roofed forest chunk (4,-4): after the first
/// dark oak, its 96-iteration leaf-litter scatter and its 20-iteration grass
/// scatter, the second rule iteration grows an oak whose trunk is exactly the
/// four logs y 124..=127 of column (74,-63). The tree pick lands on that oak
/// only when placing leaf litter draws nothing from the feature random.
#[test]
fn roofed_forest_second_tree_follows_the_litter_scatter() {
    let chunk = generate_chunk(4, -4);
    // Chunk-local (10, 1) is block (74, -63).
    let logs = blocks_named(&chunk, "minecraft:oak_log", (10, 10), (-63, 319), (1, 1));
    let expected: BTreeSet<_> = (124..=127).map(|y| (10u8, y, 1u8)).collect();
    assert_eq!(logs, expected);
}

/// Natural seed-12345 BDS world, roofed forest chunk (4,-4): the first dark
/// oak (base (75,122,-54)) draws two lean steps yet grows straight: its two
/// west trunk columns hold logs y 122..=128 and its only west branch hangs at
/// y 124..=127, with no log further west. (The east trunk columns stand on
/// higher BDS terrain and are left out.)
#[test]
fn roofed_forest_wide_trunk_grows_straight() {
    let chunk = generate_chunk(4, -4);
    // Chunk-local x 9..=11, z 8..=12 is block x 73..=75, z -56..=-52.
    let actual = blocks_named(
        &chunk,
        "minecraft:dark_oak_log",
        (9, 11),
        (118, 135),
        (8, 12),
    );
    let mut expected = BTreeSet::new();
    for (x, z, lo, hi) in [
        (11u8, 10u8, 122i16, 128i16),
        (11, 11, 122, 128),
        (10, 10, 124, 127),
    ] {
        for y in lo..=hi {
            expected.insert((x, y, z));
        }
    }
    assert_eq!(actual, expected);
}

/// Natural seed-12345 BDS world, roofed forest chunks x 3..=4, z -5..=-2:
/// how many of the 1828 native tree blocks (logs, leaves, vines, huge-mushroom
/// blocks) we reproduce exactly. Every tree of a rule iteration shares one
/// random with its decorations, so draw-order fixes move this count; it only
/// drops for a native-proven change elsewhere (native carver seeding cost one
/// voxel), recorded in "Roofed forest draw order" in the codegen README.
#[test]
fn roofed_forest_tree_voxels_ratchet_against_bds() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/bds_1_26_51_seed_12345_roofed_trees.json"
    ))
    .unwrap();
    let registry = unastar::registry::BlockRegistry::vanilla();
    let chunks: Vec<((i32, i32), unastar::world::chunk::Chunk)> = fixture["chunks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            let (x, z) = (c[0].as_i64().unwrap() as i32, c[1].as_i64().unwrap() as i32);
            ((x, z), generate_chunk(x, z))
        })
        .collect();
    let voxels = fixture["voxels"].as_array().unwrap();
    let mut exact = 0;
    for v in voxels {
        let (x, y, z) = (
            v[0].as_i64().unwrap() as i32,
            v[1].as_i64().unwrap() as i16,
            v[2].as_i64().unwrap() as i32,
        );
        let (_, chunk) = chunks
            .iter()
            .find(|(pos, _)| *pos == (x >> 4, z >> 4))
            .expect("voxel inside a listed chunk");
        let id = chunk.get_block((x & 15) as u8, y, (z & 15) as u8);
        if registry
            .get_by_runtime_id(id)
            .is_some_and(|e| e.string_id == v[3].as_str().unwrap())
        {
            exact += 1;
        }
    }
    println!(
        "roofed forest tree voxels: {exact} of {} exact",
        voxels.len()
    );
    assert!(exact >= 787, "roofed tree voxels regressed to {exact}");
}

/// Natural seed-12345 BDS world, 13 chunks around x -5..=-2, z 1..=4: how
/// many of the 10221 cells BDS leaves open below y 0 (air, water or lava:
/// noise caves, aquifers and carver tunnels) are open in ours too. Carver
/// tunnels only land there with the native source-chunk seeding; it may only
/// rise (see "Carver source seeding" in the codegen README).
#[test]
fn underground_open_cells_ratchet_against_bds() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/bds_1_26_51_seed_12345_underground_open.json"
    ))
    .unwrap();
    let registry = unastar::registry::BlockRegistry::vanilla();
    let open_name = |id: u32| {
        registry.get_by_runtime_id(id).is_some_and(|e| {
            matches!(
                e.string_id.as_str(),
                "minecraft:air" | "minecraft:cave_air" | "minecraft:water" | "minecraft:lava"
            )
        })
    };
    let mut shared = 0;
    let mut total = 0;
    for c in fixture["chunks"].as_array().unwrap() {
        let (cx, cz) = (c[0].as_i64().unwrap() as i32, c[1].as_i64().unwrap() as i32);
        let chunk = generate_chunk(cx, cz);
        for run in fixture["runs"].as_array().unwrap() {
            let (x, z) = (
                run[0].as_i64().unwrap() as i32,
                run[1].as_i64().unwrap() as i32,
            );
            if (x >> 4, z >> 4) != (cx, cz) {
                continue;
            }
            for y in run[2].as_i64().unwrap()..=run[3].as_i64().unwrap() {
                total += 1;
                if open_name(chunk.get_block((x & 15) as u8, y as i16, (z & 15) as u8)) {
                    shared += 1;
                }
            }
        }
    }
    println!("underground open cells: {shared} of {total} shared with BDS");
    assert!(
        shared >= 4135,
        "underground open cells regressed to {shared}"
    );
}
