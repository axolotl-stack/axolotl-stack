//! Actual native configured-feature consumer fixtures, deliberately isolated
//! from natural terrain. Every write (including overwritten states) and the
//! continuing decoration RNG are compared; this is not full-world parity.

use std::collections::HashMap;
use unastar_noise::features::blocks::{BlockPos, Heightmap, JavaBlock, JavaState, StateProvider};
use unastar_noise::features::config::{CaveSurface, VegetationPatchConfig};
use unastar_noise::features::eval::{BlockEntityData, FeatureWorld};
use unastar_noise::features::generated;
use unastar_noise::features::java_blocks::{states, tags};
use unastar_noise::features::kinds::vegetation_patch;
use unastar_noise::features::random::{FeatureRandom, RegionRandom};
use unastar_noise::features::values::IntProvider;

struct Cave {
    uneven: bool,
    blocks: HashMap<BlockPos, JavaState>,
    writes: Vec<(BlockPos, JavaState)>,
    random: RegionRandom,
}

impl FeatureWorld for Cave {
    fn min_y(&self) -> i32 { -64 }
    fn height(&self) -> i32 { 384 }
    fn sea_level(&self) -> i32 { 63 }
    fn raw_brightness(&mut self, _pos: BlockPos, sky_darken: i32) -> i32 {
        15_i32.wrapping_sub(sky_darken).max(0)
    }
    fn seed(&self) -> i64 { 12345 }
    fn gen_depth(&self) -> i32 { 384 }
    fn get_state(&mut self, p: BlockPos) -> JavaState {
        if let Some(&state) = self.blocks.get(&p) { return state; }
        let mut floor = -48;
        let mut ceiling = -40;
        if self.uneven {
            floor += (p.x * 3 + p.z).rem_euclid(5);
            ceiling += (p.x + p.z * 2).rem_euclid(3);
        }
        if p.y <= floor || p.y >= ceiling { states::DEEPSLATE } else { states::AIR }
    }
    fn set_state(&mut self, p: BlockPos, state: JavaState) -> bool {
        self.blocks.insert(p, state);
        self.writes.push((p, state));
        true
    }
    fn get_height(&mut self, _: Heightmap, _: i32, _: i32) -> i32 {
        panic!("native primitive did not request a heightmap")
    }
    fn biome(&mut self, _: BlockPos) -> u8 {
        panic!("native configured primitive did not request a biome")
    }
    fn schedule_tick(&mut self, _: BlockPos) {
        panic!("native primitive did not schedule a tick")
    }
    fn region_random(&mut self) -> &mut RegionRandom { &mut self.random }
    fn set_block_entity(&mut self, _: BlockPos, _: BlockEntityData) {
        panic!("native primitive did not create an actor")
    }
}

fn native_state(text: &str) -> JavaState {
    let (name, properties) = text.split_once('[').unwrap_or((text, ""));
    let mut state = JavaBlock::by_name(name).unwrap().default_state();
    for pair in properties.trim_end_matches(']').split(',').filter(|p| !p.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap();
        state = state.with(key, value);
    }
    state
}

#[test]
fn configured_moss_clay_and_vines_match_native_writes_and_rng() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "data/native_moss_primitives_1_21_11.json"
    )).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let seed = case["seed"].as_i64().unwrap();
        let id = case["configured_feature"].as_str().unwrap();
        let mut world = Cave {
            uneven: case["uneven"].as_bool().unwrap(),
            blocks: HashMap::new(),
            writes: Vec::new(),
            random: RegionRandom::new(12345, false, 0, 0),
        };
        let coordinates = case["origin"].as_array().unwrap();
        let origin = BlockPos::new(
            coordinates[0].as_i64().unwrap() as i32,
            coordinates[1].as_i64().unwrap() as i32,
            coordinates[2].as_i64().unwrap() as i32,
        );
        let mut rng = FeatureRandom::new(seed);
        let success = match id {
            "minecraft:moss_patch" => generated::cf_moss_patch(&mut world, &mut rng, origin),
            "minecraft:moss_patch_ceiling" => generated::cf_moss_patch_ceiling(&mut world, &mut rng, origin),
            "minecraft:lush_caves_clay" => generated::cf_lush_caves_clay(&mut world, &mut rng, origin),
            "uniform_depth_probe" => {
                // Same explicitly labeled constructor probe as the native
                // helper; not a registered vanilla feature identity.
                let cfg = VegetationPatchConfig {
                    replaceable: tags::MOSS_REPLACEABLE,
                    ground_state: StateProvider::Simple(states::MOSS_BLOCK),
                    surface: CaveSurface::Floor,
                    depth: IntProvider::Uniform { min: 1, max: 3 },
                    extra_bottom_block_chance: 0.65,
                    vertical_range: 5,
                    vegetation_chance: 0.8,
                    xz_radius: IntProvider::Uniform { min: 4, max: 7 },
                    extra_edge_column_chance: 0.3,
                };
                vegetation_patch::place(&mut world, &mut rng, &cfg, origin, |w, r, p| {
                    generated::cf_moss_vegetation(w, r, p)
                })
            }
            _ => panic!("unknown native fixture feature {id}"),
        };
        let label = format!("{id} seed={seed} uneven={}", world.uneven);
        assert_eq!(success, case["success"].as_bool().unwrap(), "{label}");
        let expected: Vec<_> = case["writes"].as_array().unwrap().iter().map(|write| {
            let p = write["pos"].as_array().unwrap();
            (BlockPos::new(p[0].as_i64().unwrap() as i32, p[1].as_i64().unwrap() as i32,
                p[2].as_i64().unwrap() as i32), native_state(write["state"].as_str().unwrap()))
        }).collect();
        assert_eq!(world.writes.len(), expected.len(), "write count: {label}");
        for (index, (actual, expected)) in world.writes.iter().zip(&expected).enumerate() {
            assert_eq!(actual, expected, "write {index}: {label}");
        }
        assert_eq!(rng.next_int(), case["next_int"].as_i64().unwrap() as i32, "RNG: {label}");
    }
}

/// Exact native pre-placed-feature blocks with a strict native biome-query
/// boundary. The tape asserts each query's coordinates and order; it does not
/// substitute a uniform biome or claim to test the runtime's biome manager.
struct NativeSubstrate {
    chunks: HashMap<(i32, i32), Vec<JavaState>>,
    min_y: i32,
    height: i32,
    origin_chunk: (i32, i32),
    write_radius: i32,
    writes: Vec<(BlockPos, JavaState, bool)>,
    biome_queries: Vec<(BlockPos, u8)>,
    next_biome_query: usize,
    random: RegionRandom,
}

impl FeatureWorld for NativeSubstrate {
    fn min_y(&self) -> i32 { self.min_y }
    fn height(&self) -> i32 { self.height }
    fn sea_level(&self) -> i32 { 63 }
    fn raw_brightness(&mut self, _pos: BlockPos, sky_darken: i32) -> i32 {
        15_i32.wrapping_sub(sky_darken).max(0)
    }
    fn seed(&self) -> i64 { 12345 }
    fn gen_depth(&self) -> i32 { 384 }
    fn get_state(&mut self, p: BlockPos) -> JavaState {
        if self.is_outside_build_height(p.y) { return states::AIR; }
        let chunk = self.chunks.get(&(p.x >> 4, p.z >> 4))
            .expect("native primitive read outside its captured 3x3 substrate");
        chunk[((p.y - self.min_y) * 256 + (p.z & 15) * 16 + (p.x & 15)) as usize]
    }
    fn set_state(&mut self, p: BlockPos, state: JavaState) -> bool {
        let accepted = !self.is_outside_build_height(p.y)
            && ((p.x >> 4) - self.origin_chunk.0).abs() <= self.write_radius
            && ((p.z >> 4) - self.origin_chunk.1).abs() <= self.write_radius;
        self.writes.push((p, state, accepted));
        if accepted {
            self.chunks.get_mut(&(p.x >> 4, p.z >> 4)).unwrap()
                [((p.y - self.min_y) * 256 + (p.z & 15) * 16 + (p.x & 15)) as usize] = state;
        }
        accepted
    }
    fn get_height(&mut self, _: Heightmap, _: i32, _: i32) -> i32 {
        panic!("native placed-clay replay did not query a heightmap")
    }
    fn biome(&mut self, p: BlockPos) -> u8 {
        let &(expected, biome) = self.biome_queries.get(self.next_biome_query)
            .expect("compiled placement requested an extra biome not queried by native");
        assert_eq!(p, expected, "native biome-query boundary {}", self.next_biome_query);
        self.next_biome_query += 1;
        biome
    }
    fn schedule_tick(&mut self, _: BlockPos) {
        panic!("native placed-clay replay did not schedule a tick")
    }
    fn region_random(&mut self) -> &mut RegionRandom { &mut self.random }
    fn set_block_entity(&mut self, _: BlockPos, _: BlockEntityData) {
        panic!("native placed-clay replay did not create an actor")
    }
}

fn captured_position(value: &serde_json::Value) -> BlockPos {
    let p = value.as_array().unwrap();
    BlockPos::new(p[0].as_i64().unwrap() as i32, p[1].as_i64().unwrap() as i32,
        p[2].as_i64().unwrap() as i32)
}

fn captured_random(value: &serde_json::Value) -> FeatureRandom {
    use unastar_noise::features::random::XoroshiroBits;
    use unastar_noise::xoroshiro::Xoroshiro128;
    FeatureRandom::from_source(XoroshiroBits(Xoroshiro128::from_state(
        value[0].as_i64().unwrap() as u64, value[1].as_i64().unwrap() as u64,
    )))
}

#[test]
fn natural_substrate_placed_clay_matches_native_ordered_writes_and_rng() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "data/native_lush_clay_natural_substrate_1_21_11.json"
    )).unwrap();
    assert_eq!(fixture["replay_ordered_writes_exact"], true);
    assert_eq!(fixture["replay_rng_exact"], true);
    assert_eq!(fixture["replay_success_exact"], true);
    let origin = captured_position(&fixture["origin"]);
    let captures = fixture["pre_feature_chunks"].as_array().unwrap();
    let min_y = captures[0]["min_y"].as_i64().unwrap() as i32;
    let height = captures[0]["height"].as_i64().unwrap() as i32;
    let mut chunks = HashMap::new();
    for capture in captures {
        assert_eq!(capture["min_y"].as_i64().unwrap(), i64::from(min_y));
        assert_eq!(capture["height"].as_i64().unwrap(), i64::from(height));
        let palette: Vec<_> = capture["palette"].as_array().unwrap().iter()
            .map(|name| native_state(name.as_str().unwrap())).collect();
        let mut blocks = Vec::with_capacity(height as usize * 256);
        for run in capture["blocks"].as_array().unwrap().chunks_exact(2) {
            blocks.extend(std::iter::repeat_n(palette[run[0].as_u64().unwrap() as usize],
                run[1].as_u64().unwrap() as usize));
        }
        assert_eq!(blocks.len(), height as usize * 256);
        chunks.insert((capture["x"].as_i64().unwrap() as i32,
            capture["z"].as_i64().unwrap() as i32), blocks);
    }
    let biome_queries = fixture["native_biome_queries"].as_array().unwrap().iter()
        .map(|query| (captured_position(&query["pos"]),
            generated::biome_index(query["biome"].as_str().unwrap()).unwrap())).collect();
    let mut world = NativeSubstrate {
        chunks, min_y, height, origin_chunk: (origin.x >> 4, origin.z >> 4),
        write_radius: fixture["write_radius"].as_i64().unwrap() as i32,
        writes: Vec::new(), biome_queries, next_biome_query: 0,
        random: RegionRandom::new(12345, false, origin.x >> 4, origin.z >> 4),
    };
    let mut rng = captured_random(&fixture["native_rng_before"]);
    let id = generated::PLACED_NAMES.iter().position(|&id|
        id == fixture["placed_feature"].as_str().unwrap()).unwrap() as u16;
    let success = generated::place_placed(id, &mut world, &mut rng, origin);
    assert_eq!(success, fixture["native_success"].as_bool().unwrap());
    assert_eq!(world.next_biome_query, world.biome_queries.len());
    let expected: Vec<_> = fixture["native_ordered_writes"].as_array().unwrap().iter()
        .map(|write| (captured_position(&write["pos"]),
            native_state(write["state"].as_str().unwrap()), write["accepted"].as_bool().unwrap())).collect();
    for (index, (actual, expected)) in world.writes.iter().zip(&expected).enumerate() {
        assert_eq!(actual, expected, "natural-substrate native write {index}");
    }
    assert_eq!(world.writes.len(), expected.len());
    let mut expected_rng = captured_random(&fixture["native_rng_after"]);
    for index in 0..16 {
        assert_eq!(rng.next_int(), expected_rng.next_int(), "native continuing RNG draw {index}");
    }
}
