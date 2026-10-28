//! Pinned Java 1.21.11 configured-carver primitives and actual applyCarvers edges.

use super::*;
use crate::world::chunk::BlockStateSpace;

/// Native NetherWorldCarver never consults the aquifer or surface callback.
struct NoNetherCallbacks;

impl CarveContext for NoNetherCallbacks {
    fn prepare_start_chunk(&mut self, _: ChunkPos) {
        panic!("configured-carver primitive entered source-chunk selection")
    }

    fn carve_substance(&mut self, _: i32, _: i32, _: i32) -> Option<u32> {
        panic!("Nether carving consulted the aquifer")
    }

    fn top_material(
        &mut self,
        _: &DenseBlocks,
        _: i32,
        _: i32,
        _: i32,
        _: bool,
    ) -> Option<u32> {
        panic!("Nether carving consulted surface rules")
    }
}

fn nether_config() -> &'static CaveConfig {
    match &carvers::NETHER_CARVERS[0] {
        ConfiguredCarver::NetherCave(config) => config,
        _ => panic!("native Nether carver kind changed"),
    }
}

fn native_state(name: &str) -> u32 {
    java_blocks::default_state(name).expect("native fixture block")
}

#[test]
fn native_thickness_and_random_post_state() {
    // Direct getThickness(RandomSource) observations with RandomSource.create(seed).
    // Pins and the separate configured-carver capture profile are in the fixture.
    let observations = [
        (0, 1074972334, -7261648964369397258, 1083360942, 4437113781045784766),
        (1, 1070069539, 7515937759503895804, 1078458147, 7564655870752979346),
        (2, 1071783468, -2680663443691639204, 1080080846, -1817970389778669801),
        (3, 1075221533, -7713215339908698230, 1082233291, 1309571695633557482),
        (12345, 1067339770, -1528963862231680626, 1075728378, -1236052134575208584),
        (-1, 1064974842, -8337289232527761815, 1073363450, 226341162490527646),
        (i64::MIN, 1074972334, -7261648964369397258, 1083360942, 4437113781045784766),
        (i64::MAX, 1064974842, -8337289232527761815, 1073363450, 226341162490527646),
    ];
    for (seed, cave_bits, cave_next, nether_bits, nether_next) in observations {
        for (kind, bits, next) in [
            (CaveKind::Cave, cave_bits, cave_next),
            (CaveKind::Nether, nether_bits, nether_next),
        ] {
            let mut random = JavaRandom::from_seed(seed);
            assert_eq!(kind.thickness(&mut random).to_bits(), bits, "seed {seed}");
            assert_eq!(random.next_long(), next, "seed {seed}");
        }
    }
}

#[test]
fn native_nether_material_boundary_and_replaceability() {
    let geometry = CarverGeometry::for_dimension(JavaDimension::Nether);
    let config = nether_config();
    // Observed directly through NetherWorldCarver.carveBlock with genuine native
    // settings, CarvingContext, ProtoChunk, Aquifer, and vanilla-bound block tags.
    // The configured above-bottom lava anchor (10) is not this subclass's cutoff.
    for (name, replaceable) in [
        ("minecraft:netherrack", true),
        ("minecraft:stone", true),
        ("minecraft:dirt", true),
        ("minecraft:soul_sand", true),
        ("minecraft:soul_soil", true),
        ("minecraft:basalt", true),
        ("minecraft:blackstone", true),
        ("minecraft:bedrock", false),
        ("minecraft:gravel", false),
        ("minecraft:water", false),
        ("minecraft:lava", false),
        ("minecraft:cave_air", false),
    ] {
        let input = native_state(name);
        let mut chunk = DenseBlocks::filled(input, BlockStateSpace::Java);
        let mut ctx = NoNetherCallbacks;
        let mut carving = Carving {
            chunk: &mut chunk,
            pos: ChunkPos::new(0, 0),
            mask: CarvingMask::new(geometry),
            geometry,
            ctx: &mut ctx,
        };
        for y in [0, 1, 9, 10, 11, 30, 31, 32, 120, 121, 127, 128, 255] {
            let mut reached_surface = false;
            carving.carve_block(
                CaveKind::Nether,
                config.replaceable,
                config.lava_level,
                8,
                y,
                8,
                &mut reached_surface,
            );
            let expected = if !replaceable {
                input
            } else if y <= 31 {
                CARVER_BLOCKS.lava
            } else {
                CARVER_BLOCKS.cave_air
            };
            assert_eq!(carving.chunk.get_block(8, y as i16, 8), expected, "{name} y={y}");
            assert!(!reached_surface);
            if y < geometry.depth {
                // The enclosing ellipsoid, not carveBlock, owns mask writes.
                assert!(!carving.mask.get(8, y, 8));
            }
        }
    }
}

#[test]
fn native_nether_lava_boundary_tracks_context_floor() {
    // Native generator with a real LevelHeightAccessor(16, 64) produced
    // CarvingContext min=16/depth=64 and lava through 47, cave_air from 48.
    let geometry = CarverGeometry { min_y: 16, depth: 64 };
    let config = nether_config();
    let mut chunk = DenseBlocks::filled(native_state("minecraft:netherrack"), BlockStateSpace::Java);
    let mut ctx = NoNetherCallbacks;
    let mut carving = Carving {
        chunk: &mut chunk,
        pos: ChunkPos::new(0, 0),
        mask: CarvingMask::new(geometry),
        geometry,
        ctx: &mut ctx,
    };
    for y in [30, 31, 32, 46, 47, 48, 63, 72] {
        let mut reached_surface = false;
        carving.carve_block(
            CaveKind::Nether,
            config.replaceable,
            config.lava_level,
            8,
            y,
            8,
            &mut reached_surface,
        );
        assert_eq!(
            carving.chunk.get_block(8, y as i16, 8),
            if y <= 47 { CARVER_BLOCKS.lava } else { CARVER_BLOCKS.cave_air },
            "shifted context y={y}",
        );
    }
}

#[test]
fn native_nether_ellipsoid_floor_ceiling_and_mask() {
    let geometry = CarverGeometry::for_dimension(JavaDimension::Nether);
    let config = nether_config();
    let netherrack = native_state("minecraft:netherrack");
    // Native carveEllipsoid: center x/z=8, horizontal radius=4, vertical
    // radius=8, native cave skip checker with floor=-0.7, solid netherrack.
    for (center_y, first, last) in [
        (0.0, 2, 8),
        (2.0, 2, 10),
        (31.0, 26, 39),
        (32.0, 27, 40),
        (64.0, 59, 72),
        (120.0, 115, 120),
        (127.0, 1, 0),
        (128.0, 1, 0),
    ] {
        let mut chunk = DenseBlocks::filled(netherrack, BlockStateSpace::Java);
        let mut ctx = NoNetherCallbacks;
        let mut carving = Carving {
            chunk: &mut chunk,
            pos: ChunkPos::new(0, 0),
            mask: CarvingMask::new(geometry),
            geometry,
            ctx: &mut ctx,
        };
        carving.ellipsoid(
            CaveKind::Nether,
            config.replaceable,
            config.lava_level,
            8.0,
            center_y,
            8.0,
            4.0,
            8.0,
            &Skip::Cave { floor_level: -0.7 },
        );
        for y in 0..256 {
            let carved = (first..=last).contains(&y);
            let expected = if !carved {
                netherrack
            } else if y <= 31 {
                CARVER_BLOCKS.lava
            } else {
                CARVER_BLOCKS.cave_air
            };
            assert_eq!(carving.chunk.get_block(8, y as i16, 8), expected, "center={center_y} y={y}");
            if y < geometry.depth {
                assert_eq!(carving.mask.get(8, y, 8), carved, "center={center_y} y={y}");
            }
        }
    }
}

#[test]
fn native_configured_nether_full_volume_and_random_post_state() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("carver_native_1_21_11.json")).expect("native capture");
    let geometry = CarverGeometry::for_dimension(JavaDimension::Nether);
    let netherrack = native_state("minecraft:netherrack");
    for capture in fixture["runs"].as_array().expect("native runs") {
        let seed = capture["seed"].as_i64().expect("native seed");
        let mut expected = vec![0u8; 16 * 16 * 256];
        for run in capture["nonzero_runs_yzx"].as_array().expect("native volume") {
            let first = run[0].as_u64().unwrap() as usize;
            let count = run[1].as_u64().unwrap() as usize;
            let code = run[2].as_u64().unwrap() as u8;
            expected[first..first + count].fill(code);
        }
        let mut chunk = DenseBlocks::filled(netherrack, BlockStateSpace::Java);
        let mut ctx = NoNetherCallbacks;
        let mut carving = Carving {
            chunk: &mut chunk,
            pos: ChunkPos::new(0, 0),
            mask: CarvingMask::new(geometry),
            geometry,
            ctx: &mut ctx,
        };
        let mut random = JavaRandom::from_seed(seed);
        carving.cave(CaveKind::Nether, nether_config(), &mut random, ChunkPos::new(0, 0));
        assert_eq!(random.next_long(), capture["next_long"].as_i64().unwrap(), "seed {seed}");
        for (index, code) in expected.into_iter().enumerate() {
            let x = (index % 16) as u8;
            let z = ((index / 16) % 16) as u8;
            let y = (index / 256) as i32;
            let state = match code & 3 {
                0 => netherrack,
                1 => CARVER_BLOCKS.lava,
                2 => CARVER_BLOCKS.cave_air,
                _ => panic!("unknown native state code"),
            };
            assert_eq!(carving.chunk.get_block(x, y as i16, z), state, "seed {seed} ({x},{y},{z})");
            if y < geometry.depth {
                assert_eq!(carving.mask.get(x as i32, y, z as i32), code & 4 != 0, "seed {seed} mask ({x},{y},{z})");
            } else {
                assert_eq!(code, 0, "native writes outside generation depth");
            }
        }
    }
}

#[test]
fn native_overworld_dry_aquifer_barrier_blocks_zero_density_carving() {
    use crate::world::generator::{JavaGenerationOptions, JavaGenerator, JavaTerrainStage};

    // Actual native fullstage-overworld-carvers-seed0-chunk-1_-1 capture
    // retains stone at both positions. Separate native computeSubstance(+0.0)
    // observations return null: the neighboring dry-water fluid status is at
    // DimensionType.WAY_BELOW_MIN_Y (-32512), and pressure between the second
    // and third nearest statuses blocks carving. This exercises the real
    // generation consumer, including the generic native-aquifer callback.
    let generator = JavaGenerator::with_options(0, JavaGenerationOptions {
        dimension: JavaDimension::Overworld,
        generate_structures: false,
        biome_features: true,
    });
    let carved = generator.generate_stage(ChunkPos::new(-1, -1), JavaTerrainStage::Carvers);
    let stone = native_state("minecraft:stone");
    assert_eq!(carved.chunk.state_space(), BlockStateSpace::Java);
    for x in [-12, -11] {
        assert_eq!(carved.chunk.get_block((x & 15) as u8, 28, 9), stone, "({x},28,-7)");
    }
}
