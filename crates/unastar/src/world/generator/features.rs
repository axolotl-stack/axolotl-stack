use super::climate::BiomeNoise;
use super::feature_registry::{FeatureRuntimeRegistry, RuntimePlacedFeatureDefinition};
use crate::world::chunk::{Chunk, MIN_Y, SUBCHUNK_COUNT, blocks};
use jolyne::valentine::blocks::BLOCKS;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::f64::consts::PI;
use std::sync::LazyLock;
use unastar_noise::{ALL_BIOME_FEATURES, Biome, BiomeFeatures, GenerationStep, Xoroshiro128};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct FeatureDataKey {
    step: usize,
    feature_index: usize,
    feature_name: String,
}

#[derive(Debug, Default)]
struct StepFeatureOrder {
    features: Vec<String>,
    index_by_name: HashMap<String, usize>,
}

static STEP_FEATURE_ORDER: LazyLock<Vec<StepFeatureOrder>> =
    LazyLock::new(build_step_feature_order);
static RUNTIME_BLOCK_NAMES: LazyLock<Vec<&'static str>> = LazyLock::new(build_runtime_block_names);

struct FeaturePlacementContext<'a> {
    level_seed: i64,
    target_chunk: &'a mut Chunk,
    target_chunk_x: i32,
    target_chunk_z: i32,
    source_chunk_x: i32,
    source_chunk_z: i32,
    step: GenerationStep,
    top_feature_name: &'a str,
    biome_noise: &'a BiomeNoise,
    sample_neighbor_block: &'a mut dyn FnMut(i32, i32, i32) -> u32,
}

pub fn apply_overworld_biome_decoration(
    chunk: &mut Chunk,
    level_seed: i64,
    target_chunk_x: i32,
    target_chunk_z: i32,
    biome_noise: &BiomeNoise,
    sample_neighbor_block: &mut dyn FnMut(i32, i32, i32) -> u32,
) {
    let mut random = DecorationRandom::new(0);

    for source_chunk_x in (target_chunk_x - 1)..=(target_chunk_x + 1) {
        for source_chunk_z in (target_chunk_z - 1)..=(target_chunk_z + 1) {
            let possible_biomes =
                collect_possible_biomes(biome_noise, source_chunk_x, source_chunk_z);
            if possible_biomes.is_empty() {
                continue;
            }

            let ordered_features = ordered_step_features_for_biomes(&possible_biomes);
            let decoration_seed =
                random.set_decoration_seed(level_seed, source_chunk_x * 16, source_chunk_z * 16);

            for (step_index, features) in ordered_features.iter().enumerate() {
                let step = GenerationStep::ALL
                    .get(step_index)
                    .copied()
                    .unwrap_or(GenerationStep::VegetalDecoration);
                for (feature_index, feature_name) in features.iter().enumerate() {
                    random.set_feature_seed(
                        decoration_seed,
                        feature_index as i32,
                        step_index as i32,
                    );
                    let mut context = FeaturePlacementContext {
                        level_seed,
                        target_chunk: &mut *chunk,
                        target_chunk_x,
                        target_chunk_z,
                        source_chunk_x,
                        source_chunk_z,
                        step,
                        top_feature_name: feature_name,
                        biome_noise,
                        sample_neighbor_block: &mut *sample_neighbor_block,
                    };
                    let _ = place_placed_feature(&mut context, feature_name, &mut random);
                }
            }
        }
    }
}

impl<'a> FeaturePlacementContext<'a> {
    fn source_origin(&self) -> [i32; 3] {
        [self.source_chunk_x * 16, MIN_Y, self.source_chunk_z * 16]
    }

    fn max_y(&self) -> i32 {
        MIN_Y + SUBCHUNK_COUNT as i32 * 16
    }

    fn is_inside_world_bounds(&self, block_y: i32) -> bool {
        (MIN_Y..self.max_y()).contains(&block_y)
    }

    fn get_block(&mut self, block_x: i32, block_y: i32, block_z: i32) -> u32 {
        if block_x.div_euclid(16) == self.target_chunk_x
            && block_z.div_euclid(16) == self.target_chunk_z
        {
            let local_x = block_x.rem_euclid(16) as u8;
            let local_z = block_z.rem_euclid(16) as u8;
            self.target_chunk
                .get_block(local_x, block_y as i16, local_z)
        } else {
            (self.sample_neighbor_block)(block_x, block_y, block_z)
        }
    }

    fn set_block(&mut self, block_x: i32, block_y: i32, block_z: i32, block_id: u32) {
        if block_x.div_euclid(16) != self.target_chunk_x
            || block_z.div_euclid(16) != self.target_chunk_z
        {
            return;
        }

        let local_x = block_x.rem_euclid(16) as u8;
        let local_z = block_z.rem_euclid(16) as u8;
        let _ = self
            .target_chunk
            .set_block(local_x, block_y as i16, local_z, block_id);
    }

    fn height_at(&mut self, kind: HeightmapKind, block_x: i32, block_z: i32) -> i32 {
        let max_y = MIN_Y + SUBCHUNK_COUNT as i32 * 16;
        for block_y in (MIN_Y..max_y).rev() {
            let block_id = self.get_block(block_x, block_y, block_z);
            if heightmap_includes_block(kind, block_id) {
                return block_y + 1;
            }
        }
        MIN_Y
    }

    fn biome_has_top_feature(&self, block_x: i32, block_y: i32, block_z: i32) -> bool {
        let biome = self.biome_noise.get_biome(block_x, block_y, block_z);
        let Some(feature_source) = BiomeFeatures::from_name(biome.name()) else {
            return false;
        };
        feature_source
            .get_features(self.step)
            .iter()
            .any(|feature_name| *feature_name == self.top_feature_name)
    }
}

#[derive(Debug, Clone, Copy)]
enum HeightmapKind {
    WorldSurface,
    OceanFloor,
    MotionBlocking,
    MotionBlockingNoLeaves,
}

fn place_placed_feature(
    context: &mut FeaturePlacementContext<'_>,
    feature_name: &str,
    random: &mut DecorationRandom,
) -> bool {
    let Some(placed) = FeatureRuntimeRegistry::global().placed_feature(feature_name) else {
        return false;
    };
    let origin = context.source_origin();
    place_placed_feature_definition(context, placed, origin, random)
}

fn place_placed_feature_definition(
    context: &mut FeaturePlacementContext<'_>,
    placed: &RuntimePlacedFeatureDefinition,
    origin: [i32; 3],
    random: &mut DecorationRandom,
) -> bool {
    let Some(configured_feature) = resolve_feature_reference(&placed.feature) else {
        return false;
    };

    let mut positions = vec![origin];
    for modifier in &placed.placement {
        positions = apply_placement_modifier(context, random, positions, modifier);
        if positions.is_empty() {
            return false;
        }
    }

    let mut placed_any = false;
    for position in positions {
        placed_any |=
            place_configured_feature_by_name(context, &configured_feature, position, random);
    }
    placed_any
}

fn place_placed_feature_value(
    context: &mut FeaturePlacementContext<'_>,
    value: &Value,
    origin: [i32; 3],
    random: &mut DecorationRandom,
) -> bool {
    match value {
        Value::String(name) => {
            let Some(placed) =
                FeatureRuntimeRegistry::global().placed_feature(normalize_registry_name(name))
            else {
                return false;
            };
            place_placed_feature_definition(context, placed, origin, random)
        }
        Value::Object(_) => {
            let Ok(inline_feature) =
                serde_json::from_value::<RuntimePlacedFeatureDefinition>(value.clone())
            else {
                return false;
            };
            place_placed_feature_definition(context, &inline_feature, origin, random)
        }
        _ => false,
    }
}

fn place_configured_feature_by_name(
    context: &mut FeaturePlacementContext<'_>,
    feature_name: &str,
    origin: [i32; 3],
    random: &mut DecorationRandom,
) -> bool {
    let Some(feature) = FeatureRuntimeRegistry::global().configured_feature(feature_name) else {
        return false;
    };

    match normalize_registry_name(&feature.kind) {
        "random_selector" => {
            place_random_selector_feature(context, &feature.config, origin, random)
        }
        "simple_random_selector" => {
            place_simple_random_selector_feature(context, &feature.config, origin, random)
        }
        "random_boolean_selector" => {
            place_random_boolean_selector_feature(context, &feature.config, origin, random)
        }
        "tree" => place_basic_tree_feature(context, &feature.config, origin, random),
        "simple_block" => place_simple_block_feature(context, &feature.config, origin, random),
        "random_patch" | "flower" => {
            place_random_patch_feature(context, &feature.config, origin, random)
        }
        "block_column" => place_block_column_feature(context, &feature.config, origin, random),
        "seagrass" => place_seagrass_feature(context, &feature.config, origin, random),
        "kelp" => place_kelp_feature(context, origin, random),
        "ore" | "scattered_ore" => place_ore_feature(
            context,
            &feature.config,
            origin,
            random,
            normalize_registry_name(&feature.kind) == "scattered_ore",
        ),
        "disk" => place_disk_feature(context, &feature.config, origin, random),
        "vegetation_patch" | "waterlogged_vegetation_patch" => {
            place_vegetation_patch_feature(context, &feature.config, origin, random)
        }
        "spring_feature" => place_spring_feature(context, &feature.config, origin),
        "forest_rock" => place_forest_rock_feature(context, &feature.config, origin),
        "huge_brown_mushroom" => {
            place_huge_mushroom_feature(context, &feature.config, origin, false)
        }
        "huge_red_mushroom" => place_huge_mushroom_feature(context, &feature.config, origin, true),
        "lake" => place_lake_feature(context, &feature.config, origin, random),
        "freeze_top_layer" => place_freeze_top_layer_feature(context, origin),
        "underwater_magma" => {
            place_underwater_magma_feature(context, &feature.config, origin, random)
        }
        "multiface_growth" => {
            place_multiface_growth_feature(context, &feature.config, origin, random)
        }
        "monster_room" => place_monster_room_feature(context, origin, random),
        "geode" => place_geode_feature(context, &feature.config, origin, random),
        "fossil" => place_fossil_feature(context, &feature.config, origin, random),
        "iceberg" => place_iceberg_feature(context, &feature.config, origin, random),
        "vines" => place_vines_feature(context, origin),
        "blue_ice" => place_blue_ice_feature(context, origin, random),
        "bamboo" => place_bamboo_feature(context, &feature.config, origin, random),
        "ice_spike" => place_ice_spike_feature(context, origin, random),
        "sea_pickle" => place_sea_pickle_feature(context, &feature.config, origin, random),
        "desert_well" => place_desert_well_feature(context, origin),
        "sculk_patch" => place_sculk_patch_feature(context, &feature.config, origin, random),
        "root_system" => place_root_system_feature(context, &feature.config, origin, random),
        "large_dripstone" => {
            place_large_dripstone_feature(context, &feature.config, origin, random)
        }
        "dripstone_cluster" => {
            place_dripstone_cluster_feature(context, &feature.config, origin, random)
        }
        "void_start_platform" => place_void_start_platform_feature(context, origin),
        _ => false,
    }
}

fn apply_placement_modifier(
    context: &mut FeaturePlacementContext<'_>,
    random: &mut DecorationRandom,
    positions: Vec<[i32; 3]>,
    modifier: &Value,
) -> Vec<[i32; 3]> {
    let Some(kind) = modifier.get("type").and_then(Value::as_str) else {
        return positions;
    };

    match normalize_registry_name(kind) {
        "count" => {
            let count = modifier
                .get("count")
                .and_then(|value| sample_int_provider(value, random))
                .unwrap_or(0)
                .max(0) as usize;
            positions
                .into_iter()
                .flat_map(|position| std::iter::repeat(position).take(count))
                .collect()
        }
        "rarity_filter" => {
            let chance = modifier
                .get("chance")
                .and_then(Value::as_i64)
                .unwrap_or(1)
                .max(1) as u32;
            positions
                .into_iter()
                .filter(|_| random.next_int(chance) == 0)
                .collect()
        }
        "in_square" => positions
            .into_iter()
            .map(|mut position| {
                position[0] += random.next_int(16) as i32;
                position[2] += random.next_int(16) as i32;
                position
            })
            .collect(),
        "surface_water_depth_filter" => {
            let max_depth = modifier
                .get("max_water_depth")
                .and_then(Value::as_i64)
                .unwrap_or(0) as i32;
            positions
                .into_iter()
                .filter(|position| {
                    let ocean_floor =
                        context.height_at(HeightmapKind::OceanFloor, position[0], position[2]);
                    let surface =
                        context.height_at(HeightmapKind::WorldSurface, position[0], position[2]);
                    surface - ocean_floor <= max_depth
                })
                .collect()
        }
        "heightmap" => {
            let heightmap = modifier
                .get("heightmap")
                .and_then(Value::as_str)
                .and_then(HeightmapKind::from_name)
                .unwrap_or(HeightmapKind::WorldSurface);
            positions
                .into_iter()
                .filter_map(|mut position| {
                    let height = context.height_at(heightmap, position[0], position[2]);
                    (height > MIN_Y).then(|| {
                        position[1] = height;
                        position
                    })
                })
                .collect()
        }
        "block_predicate_filter" => positions
            .into_iter()
            .filter(|position| {
                modifier
                    .get("predicate")
                    .map(|predicate| block_predicate_matches(context, predicate, *position))
                    .unwrap_or(true)
            })
            .collect(),
        "biome" => positions
            .into_iter()
            .filter(|position| context.biome_has_top_feature(position[0], position[1], position[2]))
            .collect(),
        "height_range" => positions
            .into_iter()
            .filter_map(|mut position| {
                let height = modifier
                    .get("height")
                    .and_then(|value| sample_height_provider(value, context, random));
                height
                    .filter(|height| context.is_inside_world_bounds(*height))
                    .map(|height| {
                        position[1] = height;
                        position
                    })
            })
            .collect(),
        "random_offset" => positions
            .into_iter()
            .map(|mut position| {
                let xz_spread = modifier
                    .get("xz_spread")
                    .and_then(|value| sample_int_provider(value, random))
                    .unwrap_or(0);
                let y_spread = modifier
                    .get("y_spread")
                    .and_then(|value| sample_int_provider(value, random))
                    .unwrap_or(0);
                position[0] += sample_symmetric_offset(random, xz_spread);
                position[1] += sample_symmetric_offset(random, y_spread);
                position[2] += sample_symmetric_offset(random, xz_spread);
                position
            })
            .filter(|position| context.is_inside_world_bounds(position[1]))
            .collect(),
        "environment_scan" => positions
            .into_iter()
            .filter_map(|position| scan_environment_position(context, modifier, position))
            .collect(),
        "surface_relative_threshold_filter" => {
            let heightmap = modifier
                .get("heightmap")
                .and_then(Value::as_str)
                .and_then(HeightmapKind::from_name)
                .unwrap_or(HeightmapKind::WorldSurface);
            let min_inclusive = modifier
                .get("min_inclusive")
                .and_then(Value::as_i64)
                .unwrap_or(i64::from(i32::MIN)) as i32;
            let max_inclusive = modifier
                .get("max_inclusive")
                .and_then(Value::as_i64)
                .unwrap_or(i64::from(i32::MAX)) as i32;
            positions
                .into_iter()
                .filter(|position| {
                    let surface = context.height_at(heightmap, position[0], position[2]);
                    let min_y = surface + min_inclusive;
                    let max_y = surface + max_inclusive;
                    (min_y..=max_y).contains(&position[1])
                })
                .collect()
        }
        "count_on_every_layer" => {
            let count = modifier
                .get("count")
                .and_then(|value| sample_int_provider(value, random))
                .unwrap_or(0)
                .max(0) as usize;
            positions
                .into_iter()
                .flat_map(|position| {
                    find_positions_on_every_layer(context, position, count, random)
                })
                .collect()
        }
        "noise_based_count" => {
            let noise_factor = modifier
                .get("noise_factor")
                .and_then(Value::as_f64)
                .unwrap_or(1.0)
                .max(0.000001);
            let noise_offset = modifier
                .get("noise_offset")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let ratio = modifier
                .get("noise_to_count_ratio")
                .and_then(Value::as_i64)
                .unwrap_or(0)
                .max(0) as usize;
            positions
                .into_iter()
                .flat_map(|position| {
                    let noise = biome_info_noise(context, position, noise_factor);
                    let count = ((noise + noise_offset) * ratio as f64).ceil().max(0.0) as usize;
                    std::iter::repeat(position).take(count)
                })
                .collect()
        }
        "noise_threshold_count" => {
            let noise_level = modifier
                .get("noise_level")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let below_noise = modifier
                .get("below_noise")
                .and_then(Value::as_i64)
                .unwrap_or(0)
                .max(0) as usize;
            let above_noise = modifier
                .get("above_noise")
                .and_then(Value::as_i64)
                .unwrap_or(0)
                .max(0) as usize;
            positions
                .into_iter()
                .flat_map(|position| {
                    let count = if biome_info_noise(context, position, 200.0) < noise_level {
                        below_noise
                    } else {
                        above_noise
                    };
                    std::iter::repeat(position).take(count)
                })
                .collect()
        }
        "fixed_placement" => modifier
            .get("positions")
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|entry| entry.as_array())
                    .filter_map(|entry| {
                        let block_x = entry.first().and_then(Value::as_i64)? as i32;
                        let block_y = entry.get(1).and_then(Value::as_i64)? as i32;
                        let block_z = entry.get(2).and_then(Value::as_i64)? as i32;
                        (block_x.div_euclid(16) == context.source_chunk_x
                            && block_z.div_euclid(16) == context.source_chunk_z
                            && context.is_inside_world_bounds(block_y))
                        .then_some([block_x, block_y, block_z])
                    })
                    .collect()
            })
            .unwrap_or_default(),
        _ => positions,
    }
}

fn build_step_feature_order() -> Vec<StepFeatureOrder> {
    let mut feature_index_by_name = HashMap::<String, usize>::new();
    let mut next_feature_index = 0usize;
    let mut edges = BTreeMap::<FeatureDataKey, BTreeSet<FeatureDataKey>>::new();
    let mut max_step = 0usize;

    for biome in ALL_BIOME_FEATURES {
        let mut chain = Vec::new();
        for step in GenerationStep::ALL {
            let step_index = step as usize;
            max_step = max_step.max(step_index + 1);
            for feature_name in biome.get_features(step) {
                let feature_index = *feature_index_by_name
                    .entry((*feature_name).to_owned())
                    .or_insert_with(|| {
                        let index = next_feature_index;
                        next_feature_index += 1;
                        index
                    });
                chain.push(FeatureDataKey {
                    step: step_index,
                    feature_index,
                    feature_name: (*feature_name).to_owned(),
                });
            }
        }

        for feature in &chain {
            edges.entry(feature.clone()).or_default();
        }
        for pair in chain.windows(2) {
            if let [current, next] = pair {
                edges
                    .entry(current.clone())
                    .or_default()
                    .insert(next.clone());
            }
        }
    }

    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    let mut sorted = Vec::new();
    for key in edges.keys().cloned().collect::<Vec<_>>() {
        dfs_feature_order(&edges, &key, &mut visiting, &mut visited, &mut sorted);
    }
    sorted.reverse();

    let mut steps = (0..max_step)
        .map(|_| StepFeatureOrder::default())
        .collect::<Vec<_>>();
    for feature in sorted {
        let step = &mut steps[feature.step];
        if step.index_by_name.contains_key(&feature.feature_name) {
            continue;
        }
        let index = step.features.len();
        step.index_by_name
            .insert(feature.feature_name.clone(), index);
        step.features.push(feature.feature_name);
    }
    steps
}

fn dfs_feature_order(
    edges: &BTreeMap<FeatureDataKey, BTreeSet<FeatureDataKey>>,
    key: &FeatureDataKey,
    visiting: &mut BTreeSet<FeatureDataKey>,
    visited: &mut BTreeSet<FeatureDataKey>,
    sorted: &mut Vec<FeatureDataKey>,
) {
    if visited.contains(key) {
        return;
    }
    if !visiting.insert(key.clone()) {
        panic!("feature order cycle found at {}", key.feature_name);
    }
    if let Some(children) = edges.get(key) {
        for child in children {
            dfs_feature_order(edges, child, visiting, visited, sorted);
        }
    }
    visiting.remove(key);
    visited.insert(key.clone());
    sorted.push(key.clone());
}

fn collect_possible_biomes(
    biome_noise: &BiomeNoise,
    center_chunk_x: i32,
    center_chunk_z: i32,
) -> HashSet<Biome> {
    let mut possible_biomes = HashSet::new();
    let max_y = MIN_Y + SUBCHUNK_COUNT as i32 * 16;

    for sample_chunk_x in (center_chunk_x - 1)..=(center_chunk_x + 1) {
        for sample_chunk_z in (center_chunk_z - 1)..=(center_chunk_z + 1) {
            let base_x = sample_chunk_x * 16;
            let base_z = sample_chunk_z * 16;

            for section_y in (MIN_Y..max_y).step_by(16) {
                for local_quart_y in 0..4 {
                    let sample_y = section_y + local_quart_y * 4 + 2;
                    for local_quart_x in 0..4 {
                        let sample_x = base_x + local_quart_x * 4 + 2;
                        for local_quart_z in 0..4 {
                            let sample_z = base_z + local_quart_z * 4 + 2;
                            possible_biomes
                                .insert(biome_noise.get_biome(sample_x, sample_y, sample_z));
                        }
                    }
                }
            }
        }
    }

    possible_biomes
}

fn ordered_step_features_for_biomes(possible_biomes: &HashSet<Biome>) -> Vec<Vec<String>> {
    GenerationStep::ALL
        .iter()
        .enumerate()
        .map(|(step_index, step)| {
            let mut indices = BTreeSet::new();
            for biome in possible_biomes {
                let Some(feature_source) = BiomeFeatures::from_name(biome.name()) else {
                    continue;
                };
                for feature_name in feature_source.get_features(*step) {
                    if let Some(index) = STEP_FEATURE_ORDER[step_index]
                        .index_by_name
                        .get(*feature_name)
                        .copied()
                    {
                        indices.insert(index);
                    }
                }
            }

            indices
                .into_iter()
                .map(|index| STEP_FEATURE_ORDER[step_index].features[index].clone())
                .collect()
        })
        .collect()
}

fn resolve_placed_feature(name: &str) -> Option<(&RuntimePlacedFeatureDefinition, String)> {
    let placed = FeatureRuntimeRegistry::global().placed_feature(name)?;
    let configured_feature = resolve_feature_reference(&placed.feature)?;
    FeatureRuntimeRegistry::global().configured_feature(&configured_feature)?;
    Some((placed, configured_feature))
}

fn resolve_feature_reference(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(name) => Some(normalize_registry_name(name).to_owned()),
        serde_json::Value::Object(object) => object
            .get("feature")
            .and_then(resolve_feature_reference)
            .or_else(|| {
                object
                    .get("type")
                    .and_then(serde_json::Value::as_str)
                    .map(normalize_registry_name)
                    .map(ToOwned::to_owned)
            }),
        _ => None,
    }
}

impl HeightmapKind {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "WORLD_SURFACE" | "WORLD_SURFACE_WG" => Some(Self::WorldSurface),
            "OCEAN_FLOOR" | "OCEAN_FLOOR_WG" => Some(Self::OceanFloor),
            "MOTION_BLOCKING" => Some(Self::MotionBlocking),
            "MOTION_BLOCKING_NO_LEAVES" => Some(Self::MotionBlockingNoLeaves),
            _ => None,
        }
    }
}

fn place_random_selector_feature(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    origin: [i32; 3],
    random: &mut DecorationRandom,
) -> bool {
    let Some(features) = config.get("features").and_then(Value::as_array) else {
        return config
            .get("default")
            .map(|default_feature| {
                place_placed_feature_value(context, default_feature, origin, random)
            })
            .unwrap_or(false);
    };

    for feature in features {
        let chance = feature.get("chance").and_then(Value::as_f64).unwrap_or(0.0) as f32;
        if random.next_float() < chance {
            return feature
                .get("feature")
                .map(|selected| place_placed_feature_value(context, selected, origin, random))
                .unwrap_or(false);
        }
    }
    config
        .get("default")
        .map(|default_feature| place_placed_feature_value(context, default_feature, origin, random))
        .unwrap_or(false)
}

fn place_simple_random_selector_feature(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    _origin: [i32; 3],
    random: &mut DecorationRandom,
) -> bool {
    let Some(features) = config.get("features").and_then(Value::as_array) else {
        return false;
    };
    if features.is_empty() {
        return false;
    }

    let index = random.next_int(features.len() as u32) as usize;
    place_placed_feature_value(context, &features[index], _origin, random)
}

fn place_random_boolean_selector_feature(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    _origin: [i32; 3],
    random: &mut DecorationRandom,
) -> bool {
    let key = if random.next_bool() {
        "feature_true"
    } else {
        "feature_false"
    };
    config
        .get(key)
        .map(|selected| place_placed_feature_value(context, selected, _origin, random))
        .unwrap_or(false)
}

fn place_simple_block_feature(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    origin: [i32; 3],
    random: &mut DecorationRandom,
) -> bool {
    let Some(block_id) = select_provider_block_id(config.get("to_place"), random, origin) else {
        return false;
    };
    let current = block_at(context, origin);
    if !(is_air_block(current)
        || is_liquid_block(current)
        || is_replaceable_by_trees_block(current)
        || current == block_id)
    {
        return false;
    }
    context.set_block(origin[0], origin[1], origin[2], block_id);
    true
}

fn place_random_patch_feature(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    origin: [i32; 3],
    random: &mut DecorationRandom,
) -> bool {
    let Some(feature) = config.get("feature") else {
        return false;
    };
    let tries = config
        .get("tries")
        .and_then(Value::as_i64)
        .unwrap_or(0)
        .max(0) as usize;
    let xz_spread = config.get("xz_spread").and_then(Value::as_i64).unwrap_or(0) as i32;
    let y_spread = config.get("y_spread").and_then(Value::as_i64).unwrap_or(0) as i32;

    let mut placed_any = false;
    for _ in 0..tries {
        let candidate = [
            origin[0] + sample_patch_offset(random, xz_spread),
            origin[1] + sample_patch_offset(random, y_spread),
            origin[2] + sample_patch_offset(random, xz_spread),
        ];
        if !context.is_inside_world_bounds(candidate[1]) {
            continue;
        }
        placed_any |= place_placed_feature_value(context, feature, candidate, random);
    }
    placed_any
}

fn place_block_column_feature(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    origin: [i32; 3],
    random: &mut DecorationRandom,
) -> bool {
    let Some(direction) = config
        .get("direction")
        .and_then(Value::as_str)
        .and_then(direction_offset_from_name)
    else {
        return false;
    };
    let Some(layers) = config.get("layers").and_then(Value::as_array) else {
        return false;
    };
    let prioritize_tip = config
        .get("prioritize_tip")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut planned = Vec::new();
    for layer in layers {
        let height = layer
            .get("height")
            .and_then(|value| sample_int_provider(value, random))
            .unwrap_or(0)
            .max(0);
        planned.push((height, layer.get("provider")));
    }
    if prioritize_tip {
        planned.reverse();
    }

    let mut cursor = origin;
    let mut placed_any = false;
    for (height, provider) in planned {
        for _ in 0..height {
            if !context.is_inside_world_bounds(cursor[1]) {
                return placed_any;
            }
            if !config
                .get("allowed_placement")
                .map(|predicate| block_predicate_matches(context, predicate, cursor))
                .unwrap_or(true)
            {
                return placed_any;
            }
            let Some(block_id) = select_provider_block_id(provider, random, cursor) else {
                return placed_any;
            };
            context.set_block(cursor[0], cursor[1], cursor[2], block_id);
            placed_any = true;
            cursor = offset_pos(cursor, direction);
        }
    }
    placed_any
}

fn place_seagrass_feature(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    origin: [i32; 3],
    random: &mut DecorationRandom,
) -> bool {
    if !is_water_block(block_at(context, origin)) {
        return false;
    }
    let probability = config
        .get("probability")
        .and_then(Value::as_f64)
        .unwrap_or(0.0) as f32;
    let tall = random.next_float() < probability;
    let upper = [origin[0], origin[1] + 1, origin[2]];
    if tall && context.is_inside_world_bounds(upper[1]) && is_water_block(block_at(context, upper))
    {
        context.set_block(
            origin[0],
            origin[1],
            origin[2],
            blocks::get_block_id("minecraft:tall_seagrass"),
        );
        context.set_block(
            upper[0],
            upper[1],
            upper[2],
            blocks::get_block_id("minecraft:tall_seagrass"),
        );
        return true;
    }

    context.set_block(
        origin[0],
        origin[1],
        origin[2],
        blocks::get_block_id("minecraft:seagrass"),
    );
    true
}

fn place_kelp_feature(
    context: &mut FeaturePlacementContext<'_>,
    origin: [i32; 3],
    random: &mut DecorationRandom,
) -> bool {
    if !is_water_block(block_at(context, origin)) {
        return false;
    }
    let height = 1 + random.next_int(10) as i32;
    let mut placed = 0;
    for dy in 0..height {
        let pos = [origin[0], origin[1] + dy, origin[2]];
        if !context.is_inside_world_bounds(pos[1]) || !is_water_block(block_at(context, pos)) {
            break;
        }
        context.set_block(
            pos[0],
            pos[1],
            pos[2],
            blocks::get_block_id("minecraft:kelp"),
        );
        placed += 1;
    }
    placed > 0
}

fn place_ore_feature(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    origin: [i32; 3],
    random: &mut DecorationRandom,
    scattered: bool,
) -> bool {
    let Some(targets) = config.get("targets").and_then(Value::as_array) else {
        return false;
    };
    let size = config
        .get("size")
        .and_then(Value::as_i64)
        .unwrap_or(0)
        .max(0) as i32;
    if size <= 0 {
        return false;
    }

    let radius = (size as f64).cbrt().ceil() as i32;
    let mut placed_any = false;
    let attempts = if scattered { size } else { size * 2 };
    for _ in 0..attempts {
        let pos = if scattered {
            [
                origin[0] + sample_patch_offset(random, radius),
                origin[1] + sample_patch_offset(random, radius / 2 + 1),
                origin[2] + sample_patch_offset(random, radius),
            ]
        } else {
            [
                origin[0] + sample_patch_offset(random, radius),
                origin[1] + sample_patch_offset(random, radius),
                origin[2] + sample_patch_offset(random, radius),
            ]
        };
        if !context.is_inside_world_bounds(pos[1]) {
            continue;
        }
        let block_id = block_at(context, pos);
        if let Some(replacement) = select_ore_replacement(targets, block_id) {
            context.set_block(pos[0], pos[1], pos[2], replacement);
            placed_any = true;
        }
    }
    placed_any
}

fn place_disk_feature(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    origin: [i32; 3],
    random: &mut DecorationRandom,
) -> bool {
    let half_height = config
        .get("half_height")
        .and_then(Value::as_i64)
        .unwrap_or(0)
        .max(0) as i32;
    let radius = config
        .get("radius")
        .and_then(|value| sample_int_provider(value, random))
        .unwrap_or(0)
        .max(0);
    let Some(target) = config.get("target") else {
        return false;
    };

    let mut placed_any = false;
    for dx in -radius..=radius {
        for dz in -radius..=radius {
            if dx * dx + dz * dz > radius * radius {
                continue;
            }
            for dy in -half_height..=half_height {
                let pos = [origin[0] + dx, origin[1] + dy, origin[2] + dz];
                if !context.is_inside_world_bounds(pos[1]) {
                    continue;
                }
                if !block_predicate_matches(context, target, pos) {
                    continue;
                }
                let Some(block_id) =
                    select_provider_block_id(config.get("state_provider"), random, pos)
                else {
                    continue;
                };
                context.set_block(pos[0], pos[1], pos[2], block_id);
                placed_any = true;
            }
        }
    }
    placed_any
}

fn place_vegetation_patch_feature(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    origin: [i32; 3],
    random: &mut DecorationRandom,
) -> bool {
    let radius = config
        .get("xz_radius")
        .and_then(|value| sample_int_provider(value, random))
        .unwrap_or(0)
        .max(0);
    let depth = config
        .get("depth")
        .and_then(|value| sample_int_provider(value, random))
        .unwrap_or(1)
        .max(1);
    let vertical_range = config
        .get("vertical_range")
        .and_then(Value::as_i64)
        .unwrap_or(1) as i32;
    let vegetation_chance = config
        .get("vegetation_chance")
        .and_then(Value::as_f64)
        .unwrap_or(0.0) as f32;
    let replaceable = config.get("replaceable");
    let Some(vegetation_feature) = config.get("vegetation_feature") else {
        return false;
    };
    let mut placed_any = false;

    for dx in -radius..=radius {
        for dz in -radius..=radius {
            if dx * dx + dz * dz > radius * radius {
                continue;
            }
            let x = origin[0] + dx;
            let z = origin[2] + dz;
            let top_y = context.height_at(HeightmapKind::MotionBlockingNoLeaves, x, z) - 1;
            let base = [x, top_y, z];
            if !context.is_inside_world_bounds(base[1]) {
                continue;
            }
            let base_block = block_at(context, base);
            if let Some(tag) = replaceable.and_then(Value::as_str) {
                if !block_matches_tag(base_block, tag) {
                    continue;
                }
            } else if !is_tree_ground_block(base_block) && !is_solid_render_block(base_block) {
                continue;
            }

            for layer in 0..depth {
                let pos = [x, top_y - layer, z];
                if !context.is_inside_world_bounds(pos[1]) {
                    break;
                }
                let Some(block_id) =
                    select_provider_block_id(config.get("ground_state"), random, pos)
                else {
                    break;
                };
                context.set_block(pos[0], pos[1], pos[2], block_id);
                placed_any = true;
            }

            if random.next_float() < vegetation_chance {
                let vegetation_origin = [
                    x,
                    top_y + 1 + random.next_int((vertical_range + 1).max(1) as u32) as i32,
                    z,
                ];
                if context.is_inside_world_bounds(vegetation_origin[1]) {
                    placed_any |= place_placed_feature_value(
                        context,
                        vegetation_feature,
                        vegetation_origin,
                        random,
                    );
                }
            }
        }
    }
    placed_any
}

fn place_spring_feature(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    origin: [i32; 3],
) -> bool {
    if !context.is_inside_world_bounds(origin[1]) {
        return false;
    }
    let Some(state) = config.get("state").and_then(Value::as_object) else {
        return false;
    };
    let valid_blocks = config.get("valid_blocks");
    let requires_block_below = config
        .get("requires_block_below")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let rock_count = config
        .get("rock_count")
        .and_then(Value::as_i64)
        .unwrap_or(0) as usize;
    let hole_count = config
        .get("hole_count")
        .and_then(Value::as_i64)
        .unwrap_or(0) as usize;

    if requires_block_below
        && !valid_blocks_match_block(
            valid_blocks,
            block_at(context, [origin[0], origin[1] - 1, origin[2]]),
        )
    {
        return false;
    }

    let mut rock_neighbors = 0usize;
    let mut hole_neighbors = 0usize;
    for offset in [[1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1], [0, -1, 0]] {
        let pos = offset_pos(origin, offset);
        if !context.is_inside_world_bounds(pos[1]) {
            continue;
        }
        let block = block_at(context, pos);
        if valid_blocks_match_block(valid_blocks, block) {
            rock_neighbors += 1;
        } else if is_air_block(block) {
            hole_neighbors += 1;
        }
    }
    if rock_neighbors < rock_count || hole_neighbors < hole_count {
        return false;
    }

    let block_id = feature_state_runtime_id(&parse_feature_state_object(state));
    context.set_block(origin[0], origin[1], origin[2], block_id);
    true
}

fn place_forest_rock_feature(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    origin: [i32; 3],
) -> bool {
    let Some(state) = config.get("state").and_then(Value::as_object) else {
        return false;
    };
    let block_id = feature_state_runtime_id(&parse_feature_state_object(state));
    let radius = 1;
    let ground_y = context.height_at(HeightmapKind::MotionBlocking, origin[0], origin[2]) - 1;
    let mut placed = false;
    for dx in -radius..=radius {
        for dz in -radius..=radius {
            if dx * dx + dz * dz > radius * radius + 1 {
                continue;
            }
            let pos = [origin[0] + dx, ground_y + 1, origin[2] + dz];
            if !context.is_inside_world_bounds(pos[1]) {
                continue;
            }
            if !is_air_block(block_at(context, pos)) {
                continue;
            }
            context.set_block(pos[0], pos[1], pos[2], block_id);
            placed = true;
        }
    }
    placed
}

fn place_huge_mushroom_feature(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    origin: [i32; 3],
    red_cap: bool,
) -> bool {
    let Some(stem_provider) = config.get("stem_provider") else {
        return false;
    };
    let Some(cap_provider) = config.get("cap_provider") else {
        return false;
    };
    let height = 4;
    let base = [origin[0], origin[1], origin[2]];
    let below = [base[0], base[1] - 1, base[2]];
    if !is_tree_ground_block(block_at(context, below)) {
        return false;
    }

    let mut provider_random = DecorationRandom::new(
        ((base[0] as i64) << 32) ^ (base[1] as i64) ^ ((base[2] as i64) << 16),
    );
    let stem_id = select_provider_block_id(Some(stem_provider), &mut provider_random, base)
        .unwrap_or(blocks::get_block_id("minecraft:mushroom_stem"));
    for dy in 0..height {
        let pos = [base[0], base[1] + dy, base[2]];
        if !context.is_inside_world_bounds(pos[1]) {
            return false;
        }
        context.set_block(pos[0], pos[1], pos[2], stem_id);
    }

    let radius: i32 = if red_cap { 2 } else { 3 };
    for dx in -radius..=radius {
        for dz in -radius..=radius {
            if !red_cap && dx.abs() == radius && dz.abs() == radius {
                continue;
            }
            let pos = [base[0] + dx, base[1] + height, base[2] + dz];
            if !context.is_inside_world_bounds(pos[1]) {
                continue;
            }
            let Some(block_id) =
                select_provider_block_id(Some(cap_provider), &mut provider_random, pos)
            else {
                continue;
            };
            context.set_block(pos[0], pos[1], pos[2], block_id);
        }
    }
    true
}

fn place_lake_feature(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    origin: [i32; 3],
    random: &mut DecorationRandom,
) -> bool {
    let Some(fluid) = config.get("fluid").and_then(Value::as_object) else {
        return false;
    };
    let fluid_id = feature_state_runtime_id(&parse_feature_state_object(fluid));
    let cavity_radius = 2 + random.next_int(3) as i32;
    let center = [origin[0], origin[1] - 4, origin[2]];
    let mut placed_any = false;
    for dx in -cavity_radius..=cavity_radius {
        for dy in -2..=2 {
            for dz in -cavity_radius..=cavity_radius {
                let dist = (dx * dx + dz * dz) as f64 / (cavity_radius * cavity_radius) as f64
                    + (dy * dy) as f64 / 4.0;
                if dist > 1.0 {
                    continue;
                }
                let pos = [center[0] + dx, center[1] + dy, center[2] + dz];
                if !context.is_inside_world_bounds(pos[1]) {
                    continue;
                }
                if dy <= 0 {
                    context.set_block(pos[0], pos[1], pos[2], fluid_id);
                } else {
                    context.set_block(
                        pos[0],
                        pos[1],
                        pos[2],
                        blocks::get_block_id("minecraft:air"),
                    );
                }
                placed_any = true;
            }
        }
    }
    placed_any
}

fn place_freeze_top_layer_feature(
    context: &mut FeaturePlacementContext<'_>,
    origin: [i32; 3],
) -> bool {
    let x = origin[0];
    let z = origin[2];
    let surface_y = context.height_at(HeightmapKind::MotionBlocking, x, z);
    if !context.is_inside_world_bounds(surface_y) {
        return false;
    }
    let top = [x, surface_y, z];
    let below = [x, surface_y - 1, z];
    let mut placed = false;
    if is_water_block(block_at(context, below)) {
        context.set_block(
            below[0],
            below[1],
            below[2],
            blocks::get_block_id("minecraft:ice"),
        );
        placed = true;
    }
    if is_air_block(block_at(context, top)) && is_solid_render_block(block_at(context, below)) {
        context.set_block(
            top[0],
            top[1],
            top[2],
            blocks::get_block_id("minecraft:snow_layer"),
        );
        placed = true;
    }
    placed
}

fn place_underwater_magma_feature(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    origin: [i32; 3],
    random: &mut DecorationRandom,
) -> bool {
    let floor_y = context.height_at(HeightmapKind::OceanFloor, origin[0], origin[2]) - 1;
    let surface_y = context.height_at(HeightmapKind::WorldSurface, origin[0], origin[2]) - 1;
    if surface_y - floor_y < 2 {
        return false;
    }
    let floor = [origin[0], floor_y, origin[2]];
    if !is_water_block(block_at(context, [floor[0], floor[1] + 1, floor[2]])) {
        return false;
    }
    let placement_radius = config
        .get("placement_radius_around_floor")
        .and_then(Value::as_i64)
        .unwrap_or(1) as i32;
    let count = config
        .get("placement_probability_per_valid_position")
        .and_then(Value::as_f64)
        .map(|value| (value * 16.0).round() as i32)
        .unwrap_or(4)
        .max(1);
    let magma = blocks::get_block_id("minecraft:magma");
    let mut placed = false;
    for _ in 0..count {
        let pos = [
            floor[0] + sample_patch_offset(random, placement_radius),
            floor[1],
            floor[2] + sample_patch_offset(random, placement_radius),
        ];
        if !context.is_inside_world_bounds(pos[1]) || !is_solid_render_block(block_at(context, pos))
        {
            continue;
        }
        if !is_water_block(block_at(context, [pos[0], pos[1] + 1, pos[2]])) {
            continue;
        }
        context.set_block(pos[0], pos[1], pos[2], magma);
        placed = true;
    }
    placed
}

fn place_multiface_growth_feature(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    origin: [i32; 3],
    random: &mut DecorationRandom,
) -> bool {
    let growth_block_name = config
        .get("block")
        .and_then(Value::as_str)
        .unwrap_or("minecraft:glow_lichen");
    if !is_air_block(block_at(context, origin)) {
        return false;
    }

    let mut directions = Vec::new();
    if config
        .get("can_place_on_wall")
        .and_then(Value::as_bool)
        .unwrap_or(true)
    {
        directions.extend([[1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1]]);
    }
    if config
        .get("can_place_on_ceiling")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        directions.push([0, 1, 0]);
    }
    if config
        .get("can_place_on_floor")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        directions.push([0, -1, 0]);
    }
    shuffle_random(&mut directions, random);

    for direction in directions {
        let support = offset_pos(origin, direction);
        let support_name = block_name(block_at(context, support)).to_owned();
        let allowed = config
            .get("can_be_placed_on")
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(Value::as_str)
                    .map(normalize_feature_state_name)
                    .any(|name| name == support_name)
            })
            .unwrap_or(true);
        if allowed && is_solid_render_block(block_at(context, support)) {
            context.set_block(
                origin[0],
                origin[1],
                origin[2],
                blocks::get_block_id(growth_block_name),
            );
            return true;
        }
    }
    false
}

fn place_monster_room_feature(
    context: &mut FeaturePlacementContext<'_>,
    origin: [i32; 3],
    random: &mut DecorationRandom,
) -> bool {
    let x_radius = 2 + random.next_int(2) as i32;
    let z_radius = 2 + random.next_int(2) as i32;
    let floor_y = origin[1] - 1;
    let ceiling_y = origin[1] + 3;
    if !context.is_inside_world_bounds(floor_y) || !context.is_inside_world_bounds(ceiling_y) {
        return false;
    }
    let cobble = blocks::get_block_id("minecraft:cobblestone");
    let mossy = blocks::get_block_id("minecraft:mossy_cobblestone");

    for x in origin[0] - x_radius - 1..=origin[0] + x_radius + 1 {
        for z in origin[2] - z_radius - 1..=origin[2] + z_radius + 1 {
            if !is_solid_render_block(block_at(context, [x, floor_y, z]))
                || !is_solid_render_block(block_at(context, [x, ceiling_y, z]))
            {
                return false;
            }
        }
    }

    let mut placed = false;
    for x in origin[0] - x_radius - 1..=origin[0] + x_radius + 1 {
        for y in floor_y..=ceiling_y {
            for z in origin[2] - z_radius - 1..=origin[2] + z_radius + 1 {
                if !context.is_inside_world_bounds(y) {
                    continue;
                }
                let boundary = x == origin[0] - x_radius - 1
                    || x == origin[0] + x_radius + 1
                    || y == floor_y
                    || y == ceiling_y
                    || z == origin[2] - z_radius - 1
                    || z == origin[2] + z_radius + 1;
                let block_id = if boundary {
                    if y == floor_y && random.next_float() < 0.75 {
                        mossy
                    } else {
                        cobble
                    }
                } else {
                    blocks::get_block_id("minecraft:air")
                };
                context.set_block(x, y, z, block_id);
                placed = true;
            }
        }
    }
    placed
}

fn place_geode_feature(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    origin: [i32; 3],
    random: &mut DecorationRandom,
) -> bool {
    let Some(blocks_config) = config.get("blocks").and_then(Value::as_object) else {
        return false;
    };
    let outer_radius = config
        .get("outer_wall_distance")
        .and_then(|value| sample_int_provider(value, random))
        .unwrap_or(4)
        .max(2);
    let outer_id =
        select_provider_block_id(blocks_config.get("outer_layer_provider"), random, origin)
            .unwrap_or_else(|| blocks::get_block_id("minecraft:smooth_basalt"));
    let middle_id =
        select_provider_block_id(blocks_config.get("middle_layer_provider"), random, origin)
            .unwrap_or_else(|| blocks::get_block_id("minecraft:calcite"));
    let inner_id =
        select_provider_block_id(blocks_config.get("inner_layer_provider"), random, origin)
            .unwrap_or_else(|| blocks::get_block_id("minecraft:amethyst_block"));
    let alt_inner_id = select_provider_block_id(
        blocks_config.get("alternate_inner_layer_provider"),
        random,
        origin,
    )
    .unwrap_or(inner_id);
    let filling_id =
        select_provider_block_id(blocks_config.get("filling_provider"), random, origin)
            .unwrap_or_else(|| blocks::get_block_id("minecraft:air"));
    let cannot_replace_tag = blocks_config
        .get("cannot_replace")
        .and_then(Value::as_str)
        .unwrap_or("#minecraft:features_cannot_replace");

    let outer_sq = outer_radius * outer_radius;
    let middle_sq = (outer_radius - 1).max(1).pow(2);
    let inner_sq = (outer_radius - 2).max(1).pow(2);
    let filling_sq = (outer_radius - 3).max(1).pow(2);
    let mut placed = false;

    for dx in -outer_radius..=outer_radius {
        for dy in -outer_radius..=outer_radius {
            for dz in -outer_radius..=outer_radius {
                let pos = [origin[0] + dx, origin[1] + dy, origin[2] + dz];
                if !context.is_inside_world_bounds(pos[1]) {
                    continue;
                }
                let dist_sq = dx * dx + dy * dy + dz * dz;
                if dist_sq > outer_sq {
                    continue;
                }
                let current = block_at(context, pos);
                if is_air_block(current) || block_matches_tag(current, cannot_replace_tag) {
                    continue;
                }
                let block_id = if dist_sq <= filling_sq {
                    filling_id
                } else if dist_sq <= inner_sq {
                    if random.next_float()
                        < config
                            .get("use_alternate_layer0_chance")
                            .and_then(Value::as_f64)
                            .unwrap_or(0.083) as f32
                    {
                        alt_inner_id
                    } else {
                        inner_id
                    }
                } else if dist_sq <= middle_sq {
                    middle_id
                } else {
                    outer_id
                };
                context.set_block(pos[0], pos[1], pos[2], block_id);
                placed = true;
            }
        }
    }

    placed
}

fn place_fossil_feature(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    origin: [i32; 3],
    random: &mut DecorationRandom,
) -> bool {
    let fossil_id = blocks::get_block_id("minecraft:bone_block");
    let overlay_id = match config
        .get("overlay_processors")
        .and_then(Value::as_str)
        .map(normalize_registry_name)
    {
        Some("fossil_diamonds") => blocks::get_block_id("minecraft:deepslate_diamond_ore"),
        _ => blocks::get_block_id("minecraft:coal_ore"),
    };
    let radius_x = 2 + random.next_int(3) as i32;
    let radius_y = 1 + random.next_int(2) as i32;
    let radius_z = 3 + random.next_int(3) as i32;
    let mut placed = false;

    for dx in -radius_x..=radius_x {
        for dy in -radius_y..=radius_y {
            for dz in -radius_z..=radius_z {
                let nx = dx as f64 / radius_x.max(1) as f64;
                let ny = dy as f64 / radius_y.max(1) as f64;
                let nz = dz as f64 / radius_z.max(1) as f64;
                if nx * nx + ny * ny + nz * nz > 1.0 {
                    continue;
                }
                let pos = [origin[0] + dx, origin[1] + dy, origin[2] + dz];
                if !context.is_inside_world_bounds(pos[1]) {
                    continue;
                }
                let current = block_at(context, pos);
                if !(block_matches_tag(current, "#minecraft:stone_ore_replaceables")
                    || block_matches_tag(current, "#minecraft:deepslate_ore_replaceables")
                    || is_solid_render_block(current))
                {
                    continue;
                }
                let block_id = if random.next_float() < 0.15 {
                    overlay_id
                } else {
                    fossil_id
                };
                context.set_block(pos[0], pos[1], pos[2], block_id);
                placed = true;
            }
        }
    }

    placed
}

fn place_iceberg_feature(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    origin: [i32; 3],
    random: &mut DecorationRandom,
) -> bool {
    let state_id = config
        .get("state")
        .and_then(Value::as_object)
        .map(parse_feature_state_object)
        .map(|state| feature_state_runtime_id(&state))
        .unwrap_or_else(|| blocks::get_block_id("minecraft:packed_ice"));
    let height = 6 + random.next_int(10) as i32;
    let radius = 2 + random.next_int(4) as i32;
    let surface_y = context.height_at(HeightmapKind::WorldSurface, origin[0], origin[2]) - 1;
    let mut placed = false;

    for dy in -height / 3..=height {
        let taper = (radius - (dy.abs() / 3)).max(1);
        for dx in -taper..=taper {
            for dz in -taper..=taper {
                if dx * dx + dz * dz > taper * taper {
                    continue;
                }
                let pos = [origin[0] + dx, surface_y + dy, origin[2] + dz];
                if !context.is_inside_world_bounds(pos[1]) {
                    continue;
                }
                let current = block_at(context, pos);
                if !(is_air_block(current)
                    || is_water_block(current)
                    || block_name(current) == "minecraft:packed_ice"
                    || block_name(current) == "minecraft:snow"
                    || block_name(current) == "minecraft:ice")
                {
                    continue;
                }
                context.set_block(pos[0], pos[1], pos[2], state_id);
                placed = true;
            }
        }
    }

    placed
}

fn place_vines_feature(context: &mut FeaturePlacementContext<'_>, origin: [i32; 3]) -> bool {
    if !is_air_block(block_at(context, origin)) {
        return false;
    }
    for (offset, direction_name) in [
        ([-1, 0, 0], "west"),
        ([1, 0, 0], "east"),
        ([0, 0, -1], "north"),
        ([0, 0, 1], "south"),
    ] {
        let support = offset_pos(origin, offset);
        if is_solid_render_block(block_at(context, support)) {
            place_vine_block(context, origin, direction_name);
            return true;
        }
    }
    false
}

fn place_blue_ice_feature(
    context: &mut FeaturePlacementContext<'_>,
    origin: [i32; 3],
    random: &mut DecorationRandom,
) -> bool {
    let blue_ice = blocks::get_block_id("minecraft:blue_ice");
    let mut placed = false;
    for _ in 0..32 {
        let pos = [
            origin[0] + sample_patch_offset(random, 2),
            origin[1] + sample_patch_offset(random, 2),
            origin[2] + sample_patch_offset(random, 2),
        ];
        if !context.is_inside_world_bounds(pos[1]) {
            continue;
        }
        let current = block_at(context, pos);
        if matches!(
            block_name(current),
            "minecraft:water" | "minecraft:packed_ice" | "minecraft:ice"
        ) {
            context.set_block(pos[0], pos[1], pos[2], blue_ice);
            placed = true;
        }
    }
    placed
}

fn place_bamboo_feature(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    origin: [i32; 3],
    random: &mut DecorationRandom,
) -> bool {
    let ground = [origin[0], origin[1] - 1, origin[2]];
    if !is_tree_ground_block(block_at(context, ground)) {
        return false;
    }
    let height = 5 + random.next_int(12) as i32;
    let bamboo_id = blocks::get_block_id("minecraft:bamboo");
    let mut placed = false;
    for dy in 0..height {
        let pos = [origin[0], origin[1] + dy, origin[2]];
        if !context.is_inside_world_bounds(pos[1]) || !is_air_block(block_at(context, pos)) {
            break;
        }
        context.set_block(pos[0], pos[1], pos[2], bamboo_id);
        placed = true;
    }
    if placed
        && random.next_float()
            < config
                .get("probability")
                .and_then(Value::as_f64)
                .unwrap_or(0.0) as f32
    {
        let podzol = blocks::get_block_id("minecraft:podzol");
        for dx in -2..=2 {
            for dz in -2..=2 {
                if dx * dx + dz * dz > 4 {
                    continue;
                }
                let pos = [origin[0] + dx, origin[1] - 1, origin[2] + dz];
                if block_name(block_at(context, pos)) == "minecraft:dirt" {
                    context.set_block(pos[0], pos[1], pos[2], podzol);
                }
            }
        }
    }
    placed
}

fn place_ice_spike_feature(
    context: &mut FeaturePlacementContext<'_>,
    origin: [i32; 3],
    random: &mut DecorationRandom,
) -> bool {
    let base_y = context.height_at(HeightmapKind::MotionBlocking, origin[0], origin[2]) - 1;
    let base = [origin[0], base_y, origin[2]];
    if !matches!(
        block_name(block_at(context, base)),
        "minecraft:snow_block" | "minecraft:snow" | "minecraft:ice" | "minecraft:packed_ice"
    ) {
        return false;
    }
    let packed_ice = blocks::get_block_id("minecraft:packed_ice");
    let height = 7 + random.next_int(5) as i32;
    let radius = 1 + random.next_int(2) as i32;
    let mut placed = false;

    for dy in 0..=height {
        let taper = (((height - dy) as f32 / height.max(1) as f32) * radius as f32).ceil() as i32;
        for dx in -taper..=taper {
            for dz in -taper..=taper {
                if dx * dx + dz * dz > taper.max(1) * taper.max(1) {
                    continue;
                }
                let pos = [origin[0] + dx, base_y + dy, origin[2] + dz];
                if context.is_inside_world_bounds(pos[1]) {
                    context.set_block(pos[0], pos[1], pos[2], packed_ice);
                    placed = true;
                }
            }
        }
    }
    placed
}

fn place_sea_pickle_feature(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    origin: [i32; 3],
    random: &mut DecorationRandom,
) -> bool {
    if !is_water_block(block_at(context, origin))
        || !is_solid_render_block(block_at(context, [origin[0], origin[1] - 1, origin[2]]))
    {
        return false;
    }
    let cluster_count = 1 + random.next_int(
        config
            .get("count")
            .and_then(Value::as_i64)
            .unwrap_or(4)
            .clamp(1, 4) as u32,
    ) as i32;
    let mut state = FeatureStateSelection {
        name: "minecraft:sea_pickle".to_owned(),
        properties: HashMap::new(),
    };
    state
        .properties
        .insert("cluster_count".to_owned(), cluster_count.to_string());
    normalize_feature_state(&mut state);
    context.set_block(
        origin[0],
        origin[1],
        origin[2],
        feature_state_runtime_id(&state),
    );
    true
}

fn place_desert_well_feature(context: &mut FeaturePlacementContext<'_>, origin: [i32; 3]) -> bool {
    let base_y = context.height_at(HeightmapKind::MotionBlocking, origin[0], origin[2]) - 1;
    if !context.is_inside_world_bounds(base_y - 1) {
        return false;
    }
    let base = [origin[0], base_y, origin[2]];
    if block_name(block_at(context, base)) != "minecraft:sand" {
        return false;
    }

    let sandstone = blocks::get_block_id("minecraft:sandstone");
    let slab = blocks::get_block_id("minecraft:sandstone_slab");
    let water = blocks::get_block_id("minecraft:water");
    for dx in -2_i32..=2 {
        for dz in -2_i32..=2 {
            context.set_block(origin[0] + dx, base_y, origin[2] + dz, sandstone);
            context.set_block(origin[0] + dx, base_y - 1, origin[2] + dz, sandstone);
        }
    }
    context.set_block(origin[0], base_y + 1, origin[2], water);
    for offset in [[1, 0], [-1, 0], [0, 1], [0, -1]] {
        context.set_block(
            origin[0] + offset[0],
            base_y + 1,
            origin[2] + offset[1],
            water,
        );
    }
    for dx in -2_i32..=2 {
        for dz in -2_i32..=2 {
            if dx.abs() == 2 || dz.abs() == 2 {
                context.set_block(origin[0] + dx, base_y + 1, origin[2] + dz, sandstone);
                context.set_block(origin[0] + dx, base_y + 4, origin[2] + dz, sandstone);
            }
        }
    }
    for (dx, dz) in [(-1, -1), (-1, 1), (1, -1), (1, 1)] {
        context.set_block(origin[0] + dx, base_y + 4, origin[2] + dz, slab);
    }
    for (dx, dz) in [(-1, -1), (-1, 1), (1, -1), (1, 1)] {
        for dy in 2..=3 {
            context.set_block(origin[0] + dx, base_y + dy, origin[2] + dz, sandstone);
        }
    }
    true
}

fn place_sculk_patch_feature(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    origin: [i32; 3],
    random: &mut DecorationRandom,
) -> bool {
    let radius = 2 + config
        .get("spread_rounds")
        .and_then(Value::as_i64)
        .unwrap_or(1) as i32;
    let attempts = config
        .get("spread_attempts")
        .and_then(Value::as_i64)
        .unwrap_or(32)
        .clamp(1, 128) as usize;
    let sculk = blocks::get_block_id("minecraft:sculk");
    let catalyst = blocks::get_block_id("minecraft:sculk_catalyst");
    let mut placed = false;
    for _ in 0..attempts {
        let pos = [
            origin[0] + sample_patch_offset(random, radius),
            origin[1] + sample_patch_offset(random, 1),
            origin[2] + sample_patch_offset(random, radius),
        ];
        if !context.is_inside_world_bounds(pos[1]) {
            continue;
        }
        let current = block_at(context, pos);
        if !is_solid_render_block(current) || block_name(current) == "minecraft:bedrock" {
            continue;
        }
        context.set_block(pos[0], pos[1], pos[2], sculk);
        placed = true;
    }
    if placed
        && random.next_float()
            < config
                .get("catalyst_chance")
                .and_then(Value::as_f64)
                .unwrap_or(0.0) as f32
        && context.is_inside_world_bounds(origin[1])
    {
        context.set_block(origin[0], origin[1], origin[2], catalyst);
    }
    placed
}

fn place_root_system_feature(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    origin: [i32; 3],
    random: &mut DecorationRandom,
) -> bool {
    let root_radius = config
        .get("root_radius")
        .and_then(Value::as_i64)
        .unwrap_or(2) as i32;
    let root_attempts = config
        .get("root_placement_attempts")
        .and_then(Value::as_i64)
        .unwrap_or(8) as usize;
    let hanging_attempts = config
        .get("hanging_root_placement_attempts")
        .and_then(Value::as_i64)
        .unwrap_or(8) as usize;
    let root_id = select_provider_block_id(config.get("root_state_provider"), random, origin)
        .unwrap_or_else(|| blocks::get_block_id("minecraft:rooted_dirt"));
    let hanging_id =
        select_provider_block_id(config.get("hanging_root_state_provider"), random, origin)
            .unwrap_or_else(|| blocks::get_block_id("minecraft:hanging_roots"));

    let mut placed = false;
    for _ in 0..root_attempts {
        let pos = [
            origin[0] + sample_patch_offset(random, root_radius),
            origin[1] - 1 + sample_patch_offset(random, 1),
            origin[2] + sample_patch_offset(random, root_radius),
        ];
        if !context.is_inside_world_bounds(pos[1]) {
            continue;
        }
        let current = block_at(context, pos);
        if is_air_block(current)
            || is_liquid_block(current)
            || !valid_blocks_match_block(config.get("root_replaceable"), current)
        {
            continue;
        }
        context.set_block(pos[0], pos[1], pos[2], root_id);
        placed = true;
    }
    for _ in 0..hanging_attempts {
        let pos = [
            origin[0] + sample_patch_offset(random, root_radius),
            origin[1] + sample_patch_offset(random, 2),
            origin[2] + sample_patch_offset(random, root_radius),
        ];
        let above = [pos[0], pos[1] + 1, pos[2]];
        if !context.is_inside_world_bounds(pos[1])
            || !context.is_inside_world_bounds(above[1])
            || !is_air_block(block_at(context, pos))
            || !is_solid_render_block(block_at(context, above))
        {
            continue;
        }
        context.set_block(pos[0], pos[1], pos[2], hanging_id);
        placed = true;
    }

    if let Some(feature) = config.get("feature") {
        let required_space = config
            .get("required_vertical_space_for_tree")
            .and_then(Value::as_i64)
            .unwrap_or(1) as i32;
        for y in origin[1]
            ..=(origin[1]
                + config
                    .get("root_column_max_height")
                    .and_then(Value::as_i64)
                    .unwrap_or(12) as i32)
        {
            let candidate = [origin[0], y, origin[2]];
            if !context.is_inside_world_bounds(y) {
                break;
            }
            let allowed = if let Some(predicate) = config.get("allowed_tree_position") {
                block_predicate_matches(context, predicate, candidate)
            } else {
                is_air_block(block_at(context, candidate))
            };
            if !allowed {
                continue;
            }
            let mut has_space = true;
            for dy in 0..required_space {
                let pos = [candidate[0], candidate[1] + dy, candidate[2]];
                if !context.is_inside_world_bounds(pos[1]) || !is_air_block(block_at(context, pos))
                {
                    has_space = false;
                    break;
                }
            }
            if has_space {
                placed |= place_placed_feature_value(context, feature, candidate, random);
                break;
            }
        }
    }

    placed
}

fn place_large_dripstone_feature(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    origin: [i32; 3],
    random: &mut DecorationRandom,
) -> bool {
    let search_range = config
        .get("floor_to_ceiling_search_range")
        .and_then(Value::as_i64)
        .unwrap_or(30) as i32;
    let Some((floor_y, ceiling_y)) =
        find_floor_and_ceiling(context, origin[0], origin[1], origin[2], search_range)
    else {
        return false;
    };
    if ceiling_y - floor_y < 4 {
        return false;
    }
    let radius = config
        .get("column_radius")
        .and_then(|value| sample_int_provider(value, random))
        .unwrap_or(3)
        .max(1);
    let height = ((ceiling_y - floor_y) / 2).max(2);
    place_dripstone_cone(
        context,
        [origin[0], floor_y + 1, origin[2]],
        height,
        radius,
        false,
    );
    place_dripstone_cone(
        context,
        [origin[0], ceiling_y - 1, origin[2]],
        height,
        radius,
        true,
    );
    true
}

fn place_dripstone_cluster_feature(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    origin: [i32; 3],
    random: &mut DecorationRandom,
) -> bool {
    let search_range = config
        .get("floor_to_ceiling_search_range")
        .and_then(Value::as_i64)
        .unwrap_or(12) as i32;
    let radius = config
        .get("radius")
        .and_then(|value| sample_int_provider(value, random))
        .unwrap_or(4)
        .max(1);
    let height = config
        .get("height")
        .and_then(|value| sample_int_provider(value, random))
        .unwrap_or(4)
        .max(1);
    let mut placed = false;
    for dx in -radius..=radius {
        for dz in -radius..=radius {
            if dx * dx + dz * dz > radius * radius {
                continue;
            }
            let x = origin[0] + dx;
            let z = origin[2] + dz;
            let Some((floor_y, ceiling_y)) =
                find_floor_and_ceiling(context, x, origin[1], z, search_range)
            else {
                continue;
            };
            if floor_y + 1 < ceiling_y && random.next_float() < 0.6 {
                let floor_height = height.min((ceiling_y - floor_y - 1).max(1));
                place_dripstone_column(context, [x, floor_y + 1, z], floor_height, false);
                placed = true;
            }
            if floor_y + 1 < ceiling_y && random.next_float() < 0.6 {
                let ceiling_height = height.min((ceiling_y - floor_y - 1).max(1));
                place_dripstone_column(context, [x, ceiling_y - 1, z], ceiling_height, true);
                placed = true;
            }
        }
    }
    placed
}

fn place_void_start_platform_feature(
    context: &mut FeaturePlacementContext<'_>,
    origin: [i32; 3],
) -> bool {
    let obsidian = blocks::get_block_id("minecraft:obsidian");
    for dx in -2..=2 {
        for dz in -2..=2 {
            if context.is_inside_world_bounds(origin[1]) {
                context.set_block(origin[0] + dx, origin[1], origin[2] + dz, obsidian);
            }
        }
    }
    true
}

fn find_floor_and_ceiling(
    context: &mut FeaturePlacementContext<'_>,
    block_x: i32,
    origin_y: i32,
    block_z: i32,
    search_range: i32,
) -> Option<(i32, i32)> {
    let mut floor_y = None;
    for y in (origin_y - search_range..origin_y).rev() {
        if !context.is_inside_world_bounds(y) {
            continue;
        }
        if is_solid_render_block(block_at(context, [block_x, y, block_z])) {
            floor_y = Some(y);
            break;
        }
    }
    let mut ceiling_y = None;
    for y in origin_y + 1..=origin_y + search_range {
        if !context.is_inside_world_bounds(y) {
            break;
        }
        if is_solid_render_block(block_at(context, [block_x, y, block_z])) {
            ceiling_y = Some(y);
            break;
        }
    }
    Some((floor_y?, ceiling_y?))
}

fn place_dripstone_cone(
    context: &mut FeaturePlacementContext<'_>,
    origin: [i32; 3],
    height: i32,
    radius: i32,
    hanging: bool,
) {
    for dy in 0..height {
        let y = if hanging {
            origin[1] - dy
        } else {
            origin[1] + dy
        };
        if !context.is_inside_world_bounds(y) {
            break;
        }
        let taper = (((height - dy) as f32 / height.max(1) as f32) * radius as f32).ceil() as i32;
        for dx in -taper..=taper {
            for dz in -taper..=taper {
                if dx * dx + dz * dz > taper.max(1) * taper.max(1) {
                    continue;
                }
                context.set_block(
                    origin[0] + dx,
                    y,
                    origin[2] + dz,
                    blocks::get_block_id("minecraft:dripstone_block"),
                );
            }
        }
    }
}

fn place_dripstone_column(
    context: &mut FeaturePlacementContext<'_>,
    origin: [i32; 3],
    height: i32,
    hanging: bool,
) {
    let dripstone = blocks::get_block_id("minecraft:dripstone_block");
    for dy in 0..height {
        let y = if hanging {
            origin[1] - dy
        } else {
            origin[1] + dy
        };
        if !context.is_inside_world_bounds(y) {
            break;
        }
        context.set_block(origin[0], y, origin[2], dripstone);
    }
}

fn sample_patch_offset(random: &mut DecorationRandom, spread: i32) -> i32 {
    if spread <= 0 {
        return 0;
    }
    random.next_int((spread + 1) as u32) as i32 - random.next_int((spread + 1) as u32) as i32
}

fn sample_symmetric_offset(random: &mut DecorationRandom, spread: i32) -> i32 {
    if spread == 0 {
        return 0;
    }
    if spread > 0 {
        sample_patch_offset(random, spread)
    } else {
        -sample_patch_offset(random, -spread)
    }
}

fn select_ore_replacement(targets: &[Value], current_block: u32) -> Option<u32> {
    for target in targets {
        let predicate = target.get("target")?;
        if !ore_target_matches(current_block, predicate) {
            continue;
        }
        let state = target.get("state")?.as_object()?;
        return Some(feature_state_runtime_id(&parse_feature_state_object(state)));
    }
    None
}

fn ore_target_matches(current_block: u32, predicate: &Value) -> bool {
    let Some(kind) = predicate.get("predicate_type").and_then(Value::as_str) else {
        return false;
    };
    match normalize_registry_name(kind) {
        "tag_match" => predicate
            .get("tag")
            .and_then(Value::as_str)
            .map(|tag| block_matches_tag(current_block, tag))
            .unwrap_or(false),
        "block_match" => predicate
            .get("block")
            .and_then(Value::as_str)
            .map(|block| normalize_feature_state_name(block) == block_name(current_block))
            .unwrap_or(false),
        _ => false,
    }
}

#[derive(Debug, Clone, Copy)]
struct FancyFoliageCoord {
    attachment: [i32; 3],
    branch_base_y: i32,
}

fn place_basic_tree_feature(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    origin: [i32; 3],
    random: &mut DecorationRandom,
) -> bool {
    let trunk_type = config
        .get("trunk_placer")
        .and_then(Value::as_object)
        .and_then(|placer| placer.get("type"))
        .and_then(Value::as_str)
        .map(normalize_registry_name);
    let foliage_type = config
        .get("foliage_placer")
        .and_then(Value::as_object)
        .and_then(|placer| placer.get("type"))
        .and_then(Value::as_str)
        .map(normalize_registry_name);
    let Some(trunk_state) = state_provider_block_name(config.get("trunk_provider")) else {
        return false;
    };
    let Some(foliage_state) = state_provider_block_name(config.get("foliage_provider")) else {
        return false;
    };
    let below_trunk_state = config
        .get("below_trunk_provider")
        .or_else(|| config.get("dirt_provider"))
        .and_then(|provider| state_provider_block_name(Some(provider)))
        .unwrap_or("minecraft:dirt")
        .to_owned();
    let force_dirt = config
        .get("force_dirt")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let tree_height = tree_height_from_trunk_placer(config.get("trunk_placer"), random);
    if tree_height <= 0 {
        return false;
    }
    if trunk_type == Some("dark_oak_trunk_placer")
        && foliage_type == Some("dark_oak_foliage_placer")
    {
        let placed = place_dark_oak_tree_feature(
            context,
            config,
            origin,
            random,
            trunk_state,
            foliage_state,
            &below_trunk_state,
            force_dirt,
            tree_height,
        );
        return finalize_tree_feature(
            context,
            config,
            origin,
            random,
            tree_height,
            trunk_state,
            placed,
        );
    }
    if trunk_type == Some("forking_trunk_placer") && foliage_type == Some("acacia_foliage_placer") {
        let placed = place_acacia_tree_feature(
            context,
            config,
            origin,
            random,
            trunk_state,
            foliage_state,
            &below_trunk_state,
            force_dirt,
            tree_height,
        );
        return finalize_tree_feature(
            context,
            config,
            origin,
            random,
            tree_height,
            trunk_state,
            placed,
        );
    }
    if trunk_type == Some("fancy_trunk_placer") && foliage_type == Some("fancy_foliage_placer") {
        let placed = place_fancy_tree_feature(
            context,
            config,
            origin,
            random,
            trunk_state,
            foliage_state,
            &below_trunk_state,
            force_dirt,
            tree_height,
        );
        return finalize_tree_feature(
            context,
            config,
            origin,
            random,
            tree_height,
            trunk_state,
            placed,
        );
    }
    if trunk_type == Some("cherry_trunk_placer") && foliage_type == Some("cherry_foliage_placer") {
        let placed = place_cherry_tree_feature(
            context,
            config,
            origin,
            random,
            trunk_state,
            foliage_state,
            &below_trunk_state,
            force_dirt,
            tree_height,
        );
        return finalize_tree_feature(
            context,
            config,
            origin,
            random,
            tree_height,
            trunk_state,
            placed,
        );
    }
    if trunk_type == Some("bending_trunk_placer")
        && foliage_type == Some("random_spread_foliage_placer")
    {
        let placed = place_bending_tree_feature(
            context,
            config,
            origin,
            random,
            trunk_state,
            foliage_state,
            &below_trunk_state,
            force_dirt,
            tree_height,
        );
        return finalize_tree_feature(
            context,
            config,
            origin,
            random,
            tree_height,
            trunk_state,
            placed,
        );
    }
    if trunk_type == Some("upwards_branching_trunk_placer")
        && foliage_type == Some("random_spread_foliage_placer")
    {
        let placed = place_upwards_branching_tree_feature(
            context,
            config,
            origin,
            random,
            trunk_state,
            foliage_state,
            tree_height,
        );
        return finalize_tree_feature(
            context,
            config,
            origin,
            random,
            tree_height,
            trunk_state,
            placed,
        );
    }
    if trunk_type == Some("giant_trunk_placer") && foliage_type == Some("mega_pine_foliage_placer")
    {
        let placed = place_mega_pine_tree_feature(
            context,
            config,
            origin,
            random,
            trunk_state,
            foliage_state,
            &below_trunk_state,
            force_dirt,
            tree_height,
        );
        return finalize_tree_feature(
            context,
            config,
            origin,
            random,
            tree_height,
            trunk_state,
            placed,
        );
    }
    if trunk_type == Some("mega_jungle_trunk_placer")
        && foliage_type == Some("jungle_foliage_placer")
    {
        let placed = place_mega_jungle_tree_feature(
            context,
            config,
            origin,
            random,
            trunk_state,
            foliage_state,
            &below_trunk_state,
            force_dirt,
            tree_height,
        );
        return finalize_tree_feature(
            context,
            config,
            origin,
            random,
            tree_height,
            trunk_state,
            placed,
        );
    }
    if trunk_type != Some("straight_trunk_placer") {
        return false;
    }

    let max_free_height = max_free_tree_height(context, config, origin, tree_height);
    let min_clipped_height = minimum_clipped_height(config.get("minimum_size"));
    if max_free_height < tree_height
        && min_clipped_height
            .map(|min_height| max_free_height < min_height)
            .unwrap_or(true)
    {
        return false;
    }
    let actual_height = max_free_height.min(tree_height);

    if force_dirt
        || is_replaceable_below_trunk(context.get_block(origin[0], origin[1] - 1, origin[2]))
    {
        context.set_block(
            origin[0],
            origin[1] - 1,
            origin[2],
            blocks::get_block_id(&below_trunk_state),
        );
    }

    let trunk_id = blocks::get_block_id(&trunk_state);
    let foliage_id = blocks::get_block_id(&foliage_state);
    for y in 0..actual_height {
        context.set_block(origin[0], origin[1] + y, origin[2], trunk_id);
    }

    let foliage_height = match foliage_type {
        Some("spruce_foliage_placer") => {
            let trunk_height = config
                .get("foliage_placer")
                .and_then(Value::as_object)
                .and_then(|placer| placer.get("trunk_height"))
                .and_then(|value| sample_int_provider(value, random))
                .unwrap_or(0);
            (actual_height - trunk_height).max(4)
        }
        _ => config
            .get("foliage_placer")
            .and_then(Value::as_object)
            .and_then(|placer| placer.get("height"))
            .and_then(|value| sample_int_provider(value, random))
            .unwrap_or(3)
            .max(0),
    };
    let foliage_offset = config
        .get("foliage_placer")
        .and_then(Value::as_object)
        .and_then(|placer| placer.get("offset"))
        .and_then(|value| sample_int_provider(value, random))
        .unwrap_or(0);
    let base_leaf_radius = config
        .get("foliage_placer")
        .and_then(Value::as_object)
        .and_then(|placer| placer.get("radius"))
        .and_then(|value| sample_int_provider(value, random))
        .unwrap_or(2)
        .max(0);
    let leaf_radius = match foliage_type {
        Some("pine_foliage_placer") => {
            base_leaf_radius + random.next_int((actual_height + 1).max(1) as u32) as i32
        }
        _ => base_leaf_radius,
    };
    let foliage_attachment = [origin[0], origin[1] + actual_height, origin[2]];
    match foliage_type {
        Some("blob_foliage_placer") => place_blob_foliage(
            context,
            [
                foliage_attachment[0],
                foliage_attachment[1] + foliage_offset,
                foliage_attachment[2],
            ],
            foliage_id,
            foliage_height,
            leaf_radius,
            random,
        ),
        Some("pine_foliage_placer") => place_pine_foliage(
            context,
            foliage_attachment,
            foliage_id,
            foliage_height,
            leaf_radius,
            foliage_offset,
        ),
        Some("spruce_foliage_placer") => place_spruce_foliage(
            context,
            foliage_attachment,
            foliage_id,
            foliage_height,
            leaf_radius,
            foliage_offset,
            random,
        ),
        Some("bush_foliage_placer") => place_bush_foliage(
            context,
            [
                foliage_attachment[0],
                foliage_attachment[1] + foliage_offset,
                foliage_attachment[2],
            ],
            foliage_id,
            foliage_height,
            leaf_radius,
            random,
        ),
        _ => return false,
    }

    finalize_tree_feature(
        context,
        config,
        origin,
        random,
        actual_height,
        trunk_state,
        true,
    )
}

fn finalize_tree_feature(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    origin: [i32; 3],
    random: &mut DecorationRandom,
    tree_height: i32,
    trunk_state: &str,
    placed: bool,
) -> bool {
    if !placed {
        return false;
    }
    apply_tree_post_processing(context, config, origin, random, tree_height, trunk_state);
    true
}

fn apply_tree_post_processing(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    origin: [i32; 3],
    random: &mut DecorationRandom,
    tree_height: i32,
    trunk_state: &str,
) {
    let mut leaf_names = HashSet::new();
    collect_provider_block_names(config.get("foliage_provider"), &mut leaf_names);
    let (trunks, leaves) =
        collect_tree_positions(context, origin, tree_height, trunk_state, &leaf_names);
    if trunks.is_empty() && leaves.is_empty() {
        return;
    }

    if let Some(foliage_provider) = config.get("foliage_provider") {
        for pos in &leaves {
            if let Some(block_id) = select_provider_block_id(Some(foliage_provider), random, *pos) {
                context.set_block(pos[0], pos[1], pos[2], block_id);
            }
        }
    }

    if let Some(decorators) = config.get("decorators").and_then(Value::as_array) {
        apply_tree_decorators(
            context, config, origin, &trunks, &leaves, decorators, random,
        );
    }
}

fn collect_tree_positions(
    context: &mut FeaturePlacementContext<'_>,
    origin: [i32; 3],
    tree_height: i32,
    trunk_state: &str,
    leaf_names: &HashSet<String>,
) -> (Vec<[i32; 3]>, Vec<[i32; 3]>) {
    let target_min_x = context.target_chunk_x * 16;
    let target_max_x = target_min_x + 15;
    let target_min_z = context.target_chunk_z * 16;
    let target_max_z = target_min_z + 15;
    let radius = (tree_height / 2).max(8);
    let min_x = (origin[0] - radius).max(target_min_x);
    let max_x = (origin[0] + radius).min(target_max_x);
    let min_z = (origin[2] - radius).max(target_min_z);
    let max_z = (origin[2] + radius).min(target_max_z);
    let min_y = origin[1] - 2;
    let max_y = origin[1] + tree_height + 8;
    let mut trunks = Vec::new();
    let mut leaves = Vec::new();

    for block_x in min_x..=max_x {
        for block_z in min_z..=max_z {
            for block_y in min_y..=max_y {
                let name = block_name(context.get_block(block_x, block_y, block_z));
                if name == trunk_state {
                    trunks.push([block_x, block_y, block_z]);
                } else if leaf_names.contains(name) {
                    leaves.push([block_x, block_y, block_z]);
                }
            }
        }
    }

    trunks.sort_by_key(|pos| (pos[1], pos[0], pos[2]));
    leaves.sort_by_key(|pos| (pos[1], pos[0], pos[2]));
    (trunks, leaves)
}

fn apply_tree_decorators(
    context: &mut FeaturePlacementContext<'_>,
    _config: &Value,
    origin: [i32; 3],
    trunks: &[[i32; 3]],
    leaves: &[[i32; 3]],
    decorators: &[Value],
    random: &mut DecorationRandom,
) {
    for decorator in decorators {
        let Some(kind) = decorator.get("type").and_then(Value::as_str) else {
            continue;
        };
        match normalize_registry_name(kind) {
            "beehive" => place_beehive_decorator(context, trunks, leaves, decorator, random),
            "trunk_vine" => place_trunk_vine_decorator(context, trunks, random),
            "leave_vine" => place_leaf_vine_decorator(context, leaves, decorator, random),
            "attached_to_leaves" => {
                place_attached_to_leaves_decorator(context, leaves, decorator, random)
            }
            "cocoa" => place_cocoa_decorator(context, trunks, decorator, random),
            "place_on_ground" => {
                place_on_ground_decorator(context, origin, trunks, decorator, random)
            }
            "alter_ground" => {
                place_alter_ground_decorator(context, origin, trunks, decorator, random)
            }
            "pale_moss" => {
                place_pale_moss_decorator(context, origin, trunks, leaves, decorator, random)
            }
            "creaking_heart" => place_creaking_heart_decorator(context, trunks, decorator, random),
            _ => {}
        }
    }
}

fn place_beehive_decorator(
    context: &mut FeaturePlacementContext<'_>,
    trunks: &[[i32; 3]],
    leaves: &[[i32; 3]],
    decorator: &Value,
    random: &mut DecorationRandom,
) {
    if trunks.is_empty() {
        return;
    }
    let probability = decorator
        .get("probability")
        .and_then(Value::as_f64)
        .unwrap_or(0.0) as f32;
    if probability <= 0.0 || random.next_float() >= probability {
        return;
    }

    let trunk_first = trunks[0];
    let trunk_last = trunks[trunks.len() - 1];
    let hive_y = if !leaves.is_empty() {
        (leaves[0][1] - 1).max(trunk_first[1] + 1)
    } else {
        (trunk_first[1] + 1 + random.next_int(3) as i32).min(trunk_last[1])
    };

    let mut hive_placements = Vec::new();
    for pos in trunks.iter().copied().filter(|pos| pos[1] == hive_y) {
        for offset in [[1, 0, 0], [-1, 0, 0], [0, 0, 1]] {
            hive_placements.push(offset_pos(pos, offset));
        }
    }
    shuffle_random(&mut hive_placements, random);

    let hive_id = feature_state_block_id(
        "minecraft:bee_nest",
        &[("direction", "2"), ("honey_level", "0")],
    );
    for hive_pos in hive_placements {
        let front_pos = offset_pos(hive_pos, [0, 0, 1]);
        if is_air_block(block_at(context, hive_pos)) && is_air_block(block_at(context, front_pos)) {
            context.set_block(hive_pos[0], hive_pos[1], hive_pos[2], hive_id);
            return;
        }
    }
}

fn place_trunk_vine_decorator(
    context: &mut FeaturePlacementContext<'_>,
    trunks: &[[i32; 3]],
    random: &mut DecorationRandom,
) {
    for pos in trunks {
        for (offset, support_name) in [
            ([-1, 0, 0], "east"),
            ([1, 0, 0], "west"),
            ([0, 0, -1], "south"),
            ([0, 0, 1], "north"),
        ] {
            if random.next_int(3) == 0 {
                continue;
            }
            let vine_pos = offset_pos(*pos, offset);
            if is_air_block(block_at(context, vine_pos)) {
                place_vine_block(context, vine_pos, support_name);
            }
        }
    }
}

fn place_leaf_vine_decorator(
    context: &mut FeaturePlacementContext<'_>,
    leaves: &[[i32; 3]],
    decorator: &Value,
    random: &mut DecorationRandom,
) {
    let probability = decorator
        .get("probability")
        .and_then(Value::as_f64)
        .unwrap_or(0.0) as f32;
    if probability <= 0.0 {
        return;
    }

    for pos in leaves {
        for (offset, support_name) in [
            ([-1, 0, 0], "east"),
            ([1, 0, 0], "west"),
            ([0, 0, -1], "south"),
            ([0, 0, 1], "north"),
        ] {
            if random.next_float() >= probability {
                continue;
            }
            let vine_pos = offset_pos(*pos, offset);
            if is_air_block(block_at(context, vine_pos)) {
                place_hanging_vine(context, vine_pos, support_name);
            }
        }
    }
}

fn place_attached_to_leaves_decorator(
    context: &mut FeaturePlacementContext<'_>,
    leaves: &[[i32; 3]],
    decorator: &Value,
    random: &mut DecorationRandom,
) {
    let probability = decorator
        .get("probability")
        .and_then(Value::as_f64)
        .unwrap_or(0.0) as f32;
    let exclusion_radius_xz = decorator
        .get("exclusion_radius_xz")
        .and_then(Value::as_i64)
        .unwrap_or(0) as i32;
    let exclusion_radius_y = decorator
        .get("exclusion_radius_y")
        .and_then(Value::as_i64)
        .unwrap_or(0) as i32;
    let required_empty_blocks = decorator
        .get("required_empty_blocks")
        .and_then(Value::as_i64)
        .unwrap_or(1)
        .max(1) as i32;
    let directions = decorator
        .get("directions")
        .and_then(Value::as_array)
        .map(|directions| {
            directions
                .iter()
                .filter_map(Value::as_str)
                .filter_map(direction_offset_from_name)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if directions.is_empty() {
        return;
    }

    let mut shuffled_leaves = leaves.to_vec();
    shuffle_random(&mut shuffled_leaves, random);
    let mut blacklist = HashSet::new();

    for leaf_pos in shuffled_leaves {
        let direction = directions[random.next_int(directions.len() as u32) as usize];
        let placement_pos = offset_pos(leaf_pos, direction);
        if blacklist.contains(&placement_pos) || random.next_float() >= probability {
            continue;
        }
        if !has_required_empty_blocks(context, leaf_pos, direction, required_empty_blocks) {
            continue;
        }
        let Some(block_id) =
            select_provider_block_id(decorator.get("block_provider"), random, placement_pos)
        else {
            continue;
        };

        for dx in -exclusion_radius_xz..=exclusion_radius_xz {
            for dy in -exclusion_radius_y..=exclusion_radius_y {
                for dz in -exclusion_radius_xz..=exclusion_radius_xz {
                    blacklist.insert([
                        placement_pos[0] + dx,
                        placement_pos[1] + dy,
                        placement_pos[2] + dz,
                    ]);
                }
            }
        }
        context.set_block(
            placement_pos[0],
            placement_pos[1],
            placement_pos[2],
            block_id,
        );
    }
}

fn place_cocoa_decorator(
    context: &mut FeaturePlacementContext<'_>,
    trunks: &[[i32; 3]],
    decorator: &Value,
    random: &mut DecorationRandom,
) {
    if trunks.is_empty() {
        return;
    }
    let probability = decorator
        .get("probability")
        .and_then(Value::as_f64)
        .unwrap_or(0.0) as f32;
    if probability <= 0.0 || random.next_float() >= probability {
        return;
    }

    let base_y = trunks[0][1];
    for pos in trunks.iter().copied().filter(|pos| pos[1] - base_y <= 2) {
        for (support_offset, candidate_offset) in [
            ([0, 0, 1], [0, 0, -1]),
            ([-1, 0, 0], [1, 0, 0]),
            ([0, 0, -1], [0, 0, 1]),
            ([1, 0, 0], [-1, 0, 0]),
        ] {
            if random.next_float() > 0.25 {
                continue;
            }
            let cocoa_pos = offset_pos(pos, candidate_offset);
            if !is_air_block(block_at(context, cocoa_pos)) {
                continue;
            }
            let direction = horizontal_direction_state_value(support_offset);
            let age = random.next_int(3).to_string();
            let direction = direction.to_string();
            let cocoa_id = feature_state_block_id(
                "minecraft:cocoa",
                &[("age", &age), ("direction", &direction)],
            );
            context.set_block(cocoa_pos[0], cocoa_pos[1], cocoa_pos[2], cocoa_id);
        }
    }
}

fn place_on_ground_decorator(
    context: &mut FeaturePlacementContext<'_>,
    origin: [i32; 3],
    trunks: &[[i32; 3]],
    decorator: &Value,
    random: &mut DecorationRandom,
) {
    let base_positions = lowest_trunk_positions_or_origin(trunks, origin);
    if base_positions.is_empty() {
        return;
    }

    let min_y = base_positions[0][1];
    let min_x = base_positions
        .iter()
        .map(|pos| pos[0])
        .min()
        .unwrap_or(origin[0]);
    let max_x = base_positions
        .iter()
        .map(|pos| pos[0])
        .max()
        .unwrap_or(origin[0]);
    let min_z = base_positions
        .iter()
        .map(|pos| pos[2])
        .min()
        .unwrap_or(origin[2]);
    let max_z = base_positions
        .iter()
        .map(|pos| pos[2])
        .max()
        .unwrap_or(origin[2]);
    let tries = decorator
        .get("tries")
        .and_then(Value::as_i64)
        .unwrap_or(128)
        .max(1) as i32;
    let radius = decorator
        .get("radius")
        .and_then(Value::as_i64)
        .unwrap_or(2)
        .max(0) as i32;
    let height = decorator
        .get("height")
        .and_then(Value::as_i64)
        .unwrap_or(1)
        .max(0) as i32;

    for _ in 0..tries {
        let ground_pos = [
            random_between_inclusive(random, min_x - radius, max_x + radius),
            random_between_inclusive(random, min_y - height, min_y + height),
            random_between_inclusive(random, min_z - radius, max_z + radius),
        ];
        attempt_to_place_block_above(
            context,
            ground_pos,
            decorator.get("block_state_provider"),
            random,
        );
    }
}

fn place_alter_ground_decorator(
    context: &mut FeaturePlacementContext<'_>,
    origin: [i32; 3],
    trunks: &[[i32; 3]],
    decorator: &Value,
    random: &mut DecorationRandom,
) {
    let base_positions = lowest_trunk_positions_or_origin(trunks, origin);
    if base_positions.is_empty() {
        return;
    }

    for pos in base_positions {
        for center in [
            [pos[0] - 1, pos[1], pos[2] - 1],
            [pos[0] + 2, pos[1], pos[2] - 1],
            [pos[0] - 1, pos[1], pos[2] + 2],
            [pos[0] + 2, pos[1], pos[2] + 2],
        ] {
            place_alter_ground_circle(context, center, decorator.get("provider"), random);
        }

        for _ in 0..5 {
            let placement = random.next_int(64) as i32;
            let xx = placement % 8;
            let zz = placement / 8;
            if xx == 0 || xx == 7 || zz == 0 || zz == 7 {
                place_alter_ground_circle(
                    context,
                    [pos[0] - 3 + xx, pos[1], pos[2] - 3 + zz],
                    decorator.get("provider"),
                    random,
                );
            }
        }
    }
}

fn place_pale_moss_decorator(
    context: &mut FeaturePlacementContext<'_>,
    origin: [i32; 3],
    trunks: &[[i32; 3]],
    leaves: &[[i32; 3]],
    decorator: &Value,
    random: &mut DecorationRandom,
) {
    let trunk_probability = decorator
        .get("trunk_probability")
        .and_then(Value::as_f64)
        .unwrap_or(0.0) as f32;
    let leaves_probability = decorator
        .get("leaves_probability")
        .and_then(Value::as_f64)
        .unwrap_or(0.0) as f32;
    let ground_probability = decorator
        .get("ground_probability")
        .and_then(Value::as_f64)
        .unwrap_or(0.0) as f32;

    let base = trunks
        .iter()
        .copied()
        .min_by_key(|pos| pos[1])
        .unwrap_or(origin);
    if ground_probability > 0.0 && random.next_float() < ground_probability {
        place_pale_moss_patch(context, [base[0], base[1] + 1, base[2]], random);
    }

    for pos in trunks {
        if trunk_probability <= 0.0 || random.next_float() >= trunk_probability {
            continue;
        }
        let hanging_pos = [pos[0], pos[1] - 1, pos[2]];
        if is_air_block(block_at(context, hanging_pos)) {
            add_pale_moss_hanger(context, hanging_pos, random);
        }
    }
    for pos in leaves {
        if leaves_probability <= 0.0 || random.next_float() >= leaves_probability {
            continue;
        }
        let hanging_pos = [pos[0], pos[1] - 1, pos[2]];
        if is_air_block(block_at(context, hanging_pos)) {
            add_pale_moss_hanger(context, hanging_pos, random);
        }
    }
}

fn place_creaking_heart_decorator(
    context: &mut FeaturePlacementContext<'_>,
    trunks: &[[i32; 3]],
    decorator: &Value,
    random: &mut DecorationRandom,
) {
    if trunks.is_empty() {
        return;
    }
    let probability = decorator
        .get("probability")
        .and_then(Value::as_f64)
        .unwrap_or(0.0) as f32;
    if probability <= 0.0 || random.next_float() >= probability {
        return;
    }

    let mut candidates = trunks.to_vec();
    shuffle_random(&mut candidates, random);
    let heart_id = feature_state_block_id(
        "minecraft:creaking_heart",
        &[
            ("creaking_heart_state", "dormant"),
            ("natural", "true"),
            ("pillar_axis", "y"),
        ],
    );

    for pos in candidates {
        if [
            [1, 0, 0],
            [-1, 0, 0],
            [0, 1, 0],
            [0, -1, 0],
            [0, 0, 1],
            [0, 0, -1],
        ]
        .into_iter()
        .all(|offset| is_log_block(block_at(context, offset_pos(pos, offset))))
        {
            context.set_block(pos[0], pos[1], pos[2], heart_id);
            return;
        }
    }
}

fn shuffle_random<T>(values: &mut [T], random: &mut DecorationRandom) {
    for index in (1..values.len()).rev() {
        let swap_index = random.next_int((index + 1) as u32) as usize;
        values.swap(index, swap_index);
    }
}

fn offset_pos(pos: [i32; 3], offset: [i32; 3]) -> [i32; 3] {
    [pos[0] + offset[0], pos[1] + offset[1], pos[2] + offset[2]]
}

fn block_at(context: &mut FeaturePlacementContext<'_>, pos: [i32; 3]) -> u32 {
    context.get_block(pos[0], pos[1], pos[2])
}

fn feature_state_block_id(name: &str, properties: &[(&str, &str)]) -> u32 {
    let mut selection = FeatureStateSelection {
        name: name.to_owned(),
        properties: HashMap::new(),
    };
    for (key, value) in properties {
        selection
            .properties
            .insert((*key).to_owned(), (*value).to_owned());
    }
    normalize_feature_state(&mut selection);
    feature_state_runtime_id(&selection)
}

fn direction_offset_from_name(direction: &str) -> Option<[i32; 3]> {
    match normalize_registry_name(direction) {
        "down" => Some([0, -1, 0]),
        "up" => Some([0, 1, 0]),
        "north" => Some([0, 0, -1]),
        "south" => Some([0, 0, 1]),
        "west" => Some([-1, 0, 0]),
        "east" => Some([1, 0, 0]),
        _ => None,
    }
}

fn has_required_empty_blocks(
    context: &mut FeaturePlacementContext<'_>,
    leaf_pos: [i32; 3],
    direction: [i32; 3],
    required_empty_blocks: i32,
) -> bool {
    for step in 1..=required_empty_blocks {
        let pos = [
            leaf_pos[0] + direction[0] * step,
            leaf_pos[1] + direction[1] * step,
            leaf_pos[2] + direction[2] * step,
        ];
        if !is_air_block(block_at(context, pos)) {
            return false;
        }
    }
    true
}

fn horizontal_direction_state_value(direction: [i32; 3]) -> u8 {
    match direction {
        [0, 0, 1] => 2,
        [-1, 0, 0] => 0,
        [0, 0, -1] => 1,
        [1, 0, 0] => 3,
        _ => 2,
    }
}

fn vine_attachment_bits(support_name: &str) -> u8 {
    match normalize_registry_name(support_name) {
        "south" => 1,
        "west" => 2,
        "north" => 4,
        "east" => 8,
        _ => 0,
    }
}

fn place_vine_block(context: &mut FeaturePlacementContext<'_>, pos: [i32; 3], support_name: &str) {
    let bits = vine_attachment_bits(support_name).to_string();
    let vine_id = feature_state_block_id("minecraft:vine", &[("vine_direction_bits", &bits)]);
    context.set_block(pos[0], pos[1], pos[2], vine_id);
}

fn place_hanging_vine(
    context: &mut FeaturePlacementContext<'_>,
    pos: [i32; 3],
    support_name: &str,
) {
    place_vine_block(context, pos, support_name);
    let mut cursor = [pos[0], pos[1] - 1, pos[2]];
    let mut remaining = 4;
    while remaining > 0 && is_air_block(block_at(context, cursor)) {
        place_vine_block(context, cursor, support_name);
        cursor[1] -= 1;
        remaining -= 1;
    }
}

fn lowest_trunk_positions_or_origin(trunks: &[[i32; 3]], origin: [i32; 3]) -> Vec<[i32; 3]> {
    if trunks.is_empty() {
        return vec![origin];
    }
    let min_y = trunks[0][1];
    trunks
        .iter()
        .copied()
        .filter(|pos| pos[1] == min_y)
        .collect()
}

fn random_between_inclusive(random: &mut DecorationRandom, min: i32, max: i32) -> i32 {
    if max <= min {
        min
    } else {
        min + random.next_int((max - min + 1) as u32) as i32
    }
}

fn attempt_to_place_block_above(
    context: &mut FeaturePlacementContext<'_>,
    ground_pos: [i32; 3],
    provider: Option<&Value>,
    random: &mut DecorationRandom,
) {
    let above_pos = [ground_pos[0], ground_pos[1] + 1, ground_pos[2]];
    let above = block_at(context, above_pos);
    if !(is_air_block(above) || is_vine_block(above)) {
        return;
    }
    if !is_solid_render_block(block_at(context, ground_pos)) {
        return;
    }
    if context.height_at(
        HeightmapKind::MotionBlockingNoLeaves,
        ground_pos[0],
        ground_pos[2],
    ) > above_pos[1]
    {
        return;
    }
    let Some(block_id) = select_provider_block_id(provider, random, above_pos) else {
        return;
    };
    context.set_block(above_pos[0], above_pos[1], above_pos[2], block_id);
}

fn place_alter_ground_circle(
    context: &mut FeaturePlacementContext<'_>,
    pos: [i32; 3],
    provider: Option<&Value>,
    random: &mut DecorationRandom,
) {
    for xx in -2_i32..=2 {
        for zz in -2_i32..=2 {
            if xx.abs() == 2 && zz.abs() == 2 {
                continue;
            }
            place_alter_ground_block_at(
                context,
                [pos[0] + xx, pos[1], pos[2] + zz],
                provider,
                random,
            );
        }
    }
}

fn place_alter_ground_block_at(
    context: &mut FeaturePlacementContext<'_>,
    pos: [i32; 3],
    provider: Option<&Value>,
    random: &mut DecorationRandom,
) {
    let Some(block_id) = select_provider_block_id(provider, random, pos) else {
        return;
    };

    for dy in (-3..=2).rev() {
        let cursor = [pos[0], pos[1] + dy, pos[2]];
        let current = block_at(context, cursor);
        if is_tree_ground_block(current) {
            context.set_block(cursor[0], cursor[1], cursor[2], block_id);
            break;
        }
        if !is_air_block(current) && dy < 0 {
            break;
        }
    }
}

fn place_pale_moss_patch(
    context: &mut FeaturePlacementContext<'_>,
    origin: [i32; 3],
    random: &mut DecorationRandom,
) {
    let radius = 2 + random.next_int(3) as i32;
    let pale_moss_id = blocks::get_block_id("minecraft:pale_moss_block");
    let pale_moss_carpet_id = feature_state_block_id(
        "minecraft:pale_moss_carpet",
        &[
            ("bottom", "true"),
            ("east", "none"),
            ("north", "none"),
            ("south", "none"),
            ("west", "none"),
        ],
    );
    let tall_grass_lower_id = feature_state_block_id("minecraft:tall_grass", &[("half", "lower")]);
    let tall_grass_upper_id = feature_state_block_id("minecraft:tall_grass", &[("half", "upper")]);
    let short_grass_id = blocks::get_block_id("minecraft:short_grass");
    for dx in -radius..=radius {
        for dz in -radius..=radius {
            if dx * dx + dz * dz > radius * radius + radius {
                continue;
            }
            let block_x = origin[0] + dx;
            let block_z = origin[2] + dz;
            let block_y =
                context.height_at(HeightmapKind::MotionBlockingNoLeaves, block_x, block_z) - 1;
            if block_y < MIN_Y {
                continue;
            }
            let current = context.get_block(block_x, block_y, block_z);
            if is_pale_moss_replaceable_block(current) {
                context.set_block(block_x, block_y, block_z, pale_moss_id);
                let above_y = block_y + 1;
                if !is_air_block(context.get_block(block_x, above_y, block_z))
                    || random.next_float() >= 0.3
                {
                    continue;
                }
                match random.next_int(60) {
                    0..=24 => {
                        context.set_block(block_x, above_y, block_z, pale_moss_carpet_id);
                    }
                    25..=49 => {
                        context.set_block(block_x, above_y, block_z, short_grass_id);
                    }
                    _ => {
                        if is_air_block(context.get_block(block_x, above_y + 1, block_z)) {
                            context.set_block(block_x, above_y, block_z, tall_grass_lower_id);
                            context.set_block(block_x, above_y + 1, block_z, tall_grass_upper_id);
                        } else {
                            context.set_block(block_x, above_y, block_z, short_grass_id);
                        }
                    }
                }
            }
        }
    }
}

fn add_pale_moss_hanger(
    context: &mut FeaturePlacementContext<'_>,
    mut pos: [i32; 3],
    random: &mut DecorationRandom,
) {
    let body_id = feature_state_block_id("minecraft:pale_hanging_moss", &[("tip", "false")]);
    let tip_id = feature_state_block_id("minecraft:pale_hanging_moss", &[("tip", "true")]);
    while is_air_block(block_at(context, [pos[0], pos[1] - 1, pos[2]]))
        && random.next_float() >= 0.5
    {
        context.set_block(pos[0], pos[1], pos[2], body_id);
        pos[1] -= 1;
    }
    context.set_block(pos[0], pos[1], pos[2], tip_id);
}

fn place_pine_foliage(
    context: &mut FeaturePlacementContext<'_>,
    attachment: [i32; 3],
    foliage_id: u32,
    foliage_height: i32,
    leaf_radius: i32,
    foliage_offset: i32,
) {
    let mut current_radius = 0;
    for y_offset in ((foliage_offset - foliage_height)..=foliage_offset).rev() {
        place_conical_leaves_row(
            context,
            attachment,
            foliage_id,
            current_radius,
            y_offset,
            |dx, dz, radius| dx.abs() == radius && dz.abs() == radius && radius > 0,
        );
        if current_radius >= 1 && y_offset == foliage_offset - foliage_height + 1 {
            current_radius -= 1;
        } else if current_radius < leaf_radius {
            current_radius += 1;
        }
    }
}

fn place_spruce_foliage(
    context: &mut FeaturePlacementContext<'_>,
    attachment: [i32; 3],
    foliage_id: u32,
    foliage_height: i32,
    leaf_radius: i32,
    foliage_offset: i32,
    random: &mut DecorationRandom,
) {
    let mut current_radius = random.next_int(2) as i32;
    let mut max_radius = 1;
    let mut min_radius = 0;
    for y_offset in ((-(foliage_height))..=foliage_offset).rev() {
        place_conical_leaves_row(
            context,
            attachment,
            foliage_id,
            current_radius,
            y_offset,
            |dx, dz, radius| dx.abs() == radius && dz.abs() == radius && radius > 0,
        );
        if current_radius >= max_radius {
            current_radius = min_radius;
            min_radius = 1;
            max_radius = (max_radius + 1).min(leaf_radius);
        } else {
            current_radius += 1;
        }
    }
}

fn place_conical_leaves_row<F>(
    context: &mut FeaturePlacementContext<'_>,
    attachment: [i32; 3],
    foliage_id: u32,
    current_radius: i32,
    y_offset: i32,
    mut should_skip: F,
) where
    F: FnMut(i32, i32, i32) -> bool,
{
    for dx in -current_radius..=current_radius {
        for dz in -current_radius..=current_radius {
            if should_skip(dx, dz, current_radius) {
                continue;
            }
            let block_x = attachment[0] + dx;
            let block_y = attachment[1] + y_offset;
            let block_z = attachment[2] + dz;
            let existing = context.get_block(block_x, block_y, block_z);
            if is_air_block(existing)
                || is_leaves_block(existing)
                || is_replaceable_by_trees_block(existing)
            {
                context.set_block(block_x, block_y, block_z, foliage_id);
            }
        }
    }
}

fn place_bush_foliage(
    context: &mut FeaturePlacementContext<'_>,
    attachment: [i32; 3],
    foliage_id: u32,
    foliage_height: i32,
    leaf_radius: i32,
    random: &mut DecorationRandom,
) {
    for y_offset in ((-foliage_height)..=0).rev() {
        let current_radius = leaf_radius - 1 - y_offset;
        place_conical_leaves_row(
            context,
            attachment,
            foliage_id,
            current_radius.max(0),
            y_offset,
            |dx, dz, radius| dx.abs() == radius && dz.abs() == radius && random.next_int(2) == 0,
        );
    }
}

fn place_random_spread_foliage(
    context: &mut FeaturePlacementContext<'_>,
    attachment: [i32; 3],
    foliage_id: u32,
    foliage_height: i32,
    leaf_radius: i32,
    leaf_attempts: i32,
    random: &mut DecorationRandom,
) {
    let x_bound = leaf_radius.max(1) as u32;
    let y_bound = foliage_height.max(1) as u32;
    for _ in 0..leaf_attempts.max(0) {
        let block_x =
            attachment[0] + random.next_int(x_bound) as i32 - random.next_int(x_bound) as i32;
        let block_y =
            attachment[1] + random.next_int(y_bound) as i32 - random.next_int(y_bound) as i32;
        let block_z =
            attachment[2] + random.next_int(x_bound) as i32 - random.next_int(x_bound) as i32;
        let existing = context.get_block(block_x, block_y, block_z);
        if is_air_block(existing)
            || is_leaves_block(existing)
            || is_replaceable_by_trees_block(existing)
        {
            context.set_block(block_x, block_y, block_z, foliage_id);
        }
    }
}

fn place_mega_pine_foliage(
    context: &mut FeaturePlacementContext<'_>,
    attachment: [i32; 3],
    foliage_id: u32,
    foliage_height: i32,
    leaf_radius: i32,
) {
    let mut previous_radius = 0;
    for block_y in (attachment[1] - foliage_height)..=attachment[1] {
        let y_offset = attachment[1] - block_y;
        let smooth_radius =
            leaf_radius + ((y_offset as f32 / foliage_height.max(1) as f32) * 3.5).floor() as i32;
        let current_radius =
            if y_offset > 0 && smooth_radius == previous_radius && (block_y & 1) == 0 {
                smooth_radius + 1
            } else {
                smooth_radius
            };
        place_leaves_row_signed(
            context,
            [attachment[0], block_y, attachment[2]],
            foliage_id,
            current_radius,
            0,
            true,
            |dx, _, dz, radius, _| dx + dz >= 7 || dx * dx + dz * dz > radius * radius,
        );
        previous_radius = smooth_radius;
    }
}

fn place_mega_jungle_foliage(
    context: &mut FeaturePlacementContext<'_>,
    attachment: [i32; 3],
    foliage_id: u32,
    foliage_height: i32,
    leaf_radius: i32,
    radius_offset: i32,
    double_trunk: bool,
    random: &mut DecorationRandom,
) {
    let leaf_height = if double_trunk {
        foliage_height
    } else {
        1 + random.next_int(2) as i32
    };
    for y_offset in ((-leaf_height)..=0).rev() {
        let current_radius = leaf_radius + radius_offset + 1 - y_offset;
        place_leaves_row_signed(
            context,
            attachment,
            foliage_id,
            current_radius.max(0),
            y_offset,
            double_trunk,
            |dx, _, dz, radius, _| dx + dz >= 7 || dx * dx + dz * dz > radius * radius,
        );
    }
}

fn place_fancy_foliage_attachment(
    context: &mut FeaturePlacementContext<'_>,
    attachment: [i32; 3],
    foliage_id: u32,
    foliage_height: i32,
    leaf_radius: i32,
) {
    for y_offset in ((-foliage_height)..=0).rev() {
        let current_radius = leaf_radius
            + if y_offset != 0 && y_offset != -foliage_height {
                1
            } else {
                0
            };
        place_conical_leaves_row(
            context,
            attachment,
            foliage_id,
            current_radius,
            y_offset,
            |dx, dz, radius| {
                let dx = dx as f64 + 0.5;
                let dz = dz as f64 + 0.5;
                dx * dx + dz * dz > (radius * radius) as f64
            },
        );
    }
}

fn place_cherry_foliage_attachment(
    context: &mut FeaturePlacementContext<'_>,
    attachment: [i32; 3],
    foliage_id: u32,
    foliage_height: i32,
    leaf_radius: i32,
    radius_offset: i32,
    wide_bottom_layer_hole_chance: f32,
    corner_hole_chance: f32,
    hanging_leaves_chance: f32,
    hanging_leaves_extension_chance: f32,
    random: &mut DecorationRandom,
) {
    let current_radius = leaf_radius + radius_offset - 1;
    place_leaves_row_signed(
        context,
        attachment,
        foliage_id,
        (current_radius - 2).max(0),
        foliage_height - 3,
        false,
        |dx, y, dz, radius, _| {
            should_skip_cherry_leaf(
                random,
                dx,
                y,
                dz,
                radius,
                wide_bottom_layer_hole_chance,
                corner_hole_chance,
            )
        },
    );
    place_leaves_row_signed(
        context,
        attachment,
        foliage_id,
        (current_radius - 1).max(0),
        foliage_height - 4,
        false,
        |dx, y, dz, radius, _| {
            should_skip_cherry_leaf(
                random,
                dx,
                y,
                dz,
                radius,
                wide_bottom_layer_hole_chance,
                corner_hole_chance,
            )
        },
    );
    for y_offset in (0..=(foliage_height - 5).max(0)).rev() {
        place_leaves_row_signed(
            context,
            attachment,
            foliage_id,
            current_radius.max(0),
            y_offset,
            false,
            |dx, y, dz, radius, _| {
                should_skip_cherry_leaf(
                    random,
                    dx,
                    y,
                    dz,
                    radius,
                    wide_bottom_layer_hole_chance,
                    corner_hole_chance,
                )
            },
        );
    }
    place_hanging_leaves_row(
        context,
        attachment,
        foliage_id,
        current_radius.max(0),
        -1,
        false,
        wide_bottom_layer_hole_chance,
        corner_hole_chance,
        hanging_leaves_chance,
        hanging_leaves_extension_chance,
        random,
    );
    place_hanging_leaves_row(
        context,
        attachment,
        foliage_id,
        (current_radius - 1).max(0),
        -2,
        false,
        wide_bottom_layer_hole_chance,
        corner_hole_chance,
        hanging_leaves_chance,
        hanging_leaves_extension_chance,
        random,
    );
}

fn should_skip_cherry_leaf(
    random: &mut DecorationRandom,
    dx: i32,
    y: i32,
    dz: i32,
    current_radius: i32,
    wide_bottom_layer_hole_chance: f32,
    corner_hole_chance: f32,
) -> bool {
    if y == -1
        && (dx == current_radius || dz == current_radius)
        && random.next_float() < wide_bottom_layer_hole_chance
    {
        return true;
    }
    let corner = dx == current_radius && dz == current_radius;
    if current_radius > 2 {
        corner || (dx + dz > current_radius * 2 - 2 && random.next_float() < corner_hole_chance)
    } else {
        corner && random.next_float() < corner_hole_chance
    }
}

fn place_hanging_leaves_row(
    context: &mut FeaturePlacementContext<'_>,
    origin: [i32; 3],
    foliage_id: u32,
    current_radius: i32,
    y_offset: i32,
    double_trunk: bool,
    wide_bottom_layer_hole_chance: f32,
    corner_hole_chance: f32,
    hanging_leaves_chance: f32,
    hanging_leaves_extension_chance: f32,
    random: &mut DecorationRandom,
) {
    place_leaves_row_signed(
        context,
        origin,
        foliage_id,
        current_radius,
        y_offset,
        double_trunk,
        |dx, y, dz, radius, _| {
            should_skip_cherry_leaf(
                random,
                dx,
                y,
                dz,
                radius,
                wide_bottom_layer_hole_chance,
                corner_hole_chance,
            )
        },
    );
    let extra = if double_trunk { 1 } else { 0 };
    let min_x = -current_radius;
    let max_x = current_radius + extra;
    let min_z = -current_radius;
    let max_z = current_radius + extra;
    let log_y = origin[1] - 1;
    for dx in min_x..=max_x {
        for dz in min_z..=max_z {
            if dx != min_x && dx != max_x && dz != min_z && dz != max_z {
                continue;
            }
            let block_x = origin[0] + dx;
            let block_y = origin[1] + y_offset;
            let block_z = origin[2] + dz;
            if !is_leaves_block(context.get_block(block_x, block_y, block_z)) {
                continue;
            }
            if (block_x - origin[0]).abs() + (block_y - log_y).abs() + (block_z - origin[2]).abs()
                >= 7
            {
                continue;
            }
            let below_y = block_y - 1;
            if !is_air_block(context.get_block(block_x, below_y, block_z))
                || random.next_float() > hanging_leaves_chance
            {
                continue;
            }
            context.set_block(block_x, below_y, block_z, foliage_id);
            if is_air_block(context.get_block(block_x, below_y - 1, block_z))
                && random.next_float() <= hanging_leaves_extension_chance
            {
                context.set_block(block_x, below_y - 1, block_z, foliage_id);
            }
        }
    }
}

fn place_leaves_row_signed<F>(
    context: &mut FeaturePlacementContext<'_>,
    origin: [i32; 3],
    foliage_id: u32,
    current_radius: i32,
    y_offset: i32,
    double_trunk: bool,
    mut should_skip: F,
) where
    F: FnMut(i32, i32, i32, i32, bool) -> bool,
{
    let extra = if double_trunk { 1 } else { 0 };
    for dx in -current_radius..=current_radius + extra {
        for dz in -current_radius..=current_radius + extra {
            let min_dx = if double_trunk {
                dx.abs().min((dx - 1).abs())
            } else {
                dx.abs()
            };
            let min_dz = if double_trunk {
                dz.abs().min((dz - 1).abs())
            } else {
                dz.abs()
            };
            if should_skip(min_dx, y_offset, min_dz, current_radius, double_trunk) {
                continue;
            }
            let block_x = origin[0] + dx;
            let block_y = origin[1] + y_offset;
            let block_z = origin[2] + dz;
            let existing = context.get_block(block_x, block_y, block_z);
            if is_air_block(existing)
                || is_leaves_block(existing)
                || is_replaceable_by_trees_block(existing)
            {
                context.set_block(block_x, block_y, block_z, foliage_id);
            }
        }
    }
}

fn choose_horizontal_direction(random: &mut DecorationRandom) -> (i32, i32) {
    const DIRECTIONS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
    DIRECTIONS[random.next_int(DIRECTIONS.len() as u32) as usize]
}

fn can_place_tree_block(
    context: &mut FeaturePlacementContext<'_>,
    block_x: i32,
    block_y: i32,
    block_z: i32,
) -> bool {
    let block_id = context.get_block(block_x, block_y, block_z);
    is_air_block(block_id) || is_leaves_block(block_id) || is_replaceable_by_trees_block(block_id)
}

fn is_free_tree_space(
    context: &mut FeaturePlacementContext<'_>,
    block_x: i32,
    block_y: i32,
    block_z: i32,
) -> bool {
    let block_id = context.get_block(block_x, block_y, block_z);
    can_place_tree_block(context, block_x, block_y, block_z) || is_log_block(block_id)
}

fn pillar_block_id(name: &str, axis: char) -> u32 {
    let Some(block) = BLOCKS.iter().find(|block| block.string_id() == name) else {
        return blocks::get_block_id(name);
    };
    let axis_offset = match axis {
        'x' | 'X' => 1,
        'z' | 'Z' => 2,
        _ => 0,
    };
    let state_id = block.min_state_id() + axis_offset;
    if state_id <= block.max_state_id() {
        state_id
    } else {
        block.default_state_id()
    }
}

fn place_axis_log(
    context: &mut FeaturePlacementContext<'_>,
    block_x: i32,
    block_y: i32,
    block_z: i32,
    trunk_state: &str,
    axis: char,
) -> bool {
    if !can_place_tree_block(context, block_x, block_y, block_z) {
        return false;
    }
    context.set_block(
        block_x,
        block_y,
        block_z,
        pillar_block_id(trunk_state, axis),
    );
    true
}

fn place_fancy_tree_feature(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    origin: [i32; 3],
    random: &mut DecorationRandom,
    trunk_state: &str,
    foliage_state: &str,
    below_trunk_state: &str,
    force_dirt: bool,
    tree_height: i32,
) -> bool {
    let max_free_height = max_free_tree_height(context, config, origin, tree_height + 2);
    let min_clipped_height = minimum_clipped_height(config.get("minimum_size"));
    if max_free_height < tree_height
        && min_clipped_height
            .map(|min_height| max_free_height < min_height)
            .unwrap_or(true)
    {
        return false;
    }
    let actual_height = max_free_height.min(tree_height);

    if force_dirt
        || is_replaceable_below_trunk(context.get_block(origin[0], origin[1] - 1, origin[2]))
    {
        context.set_block(
            origin[0],
            origin[1] - 1,
            origin[2],
            blocks::get_block_id(below_trunk_state),
        );
    }

    let height = actual_height + 2;
    let trunk_height = (height as f64 * 0.618).floor() as i32;
    let trunk_top_y = origin[1] + trunk_height;
    let clusters_per_y = 1.min((1.382 + (height as f64 / 13.0).powi(2)).floor() as i32);
    let mut relative_y = height - 5;
    let mut foliage_coords = vec![FancyFoliageCoord {
        attachment: [origin[0], origin[1] + relative_y, origin[2]],
        branch_base_y: trunk_top_y,
    }];

    while relative_y >= 0 {
        let tree_shape = fancy_tree_shape(height, relative_y);
        if tree_shape >= 0.0 {
            for _ in 0..clusters_per_y {
                let radius = tree_shape * (random.next_float() as f64 + 0.328);
                let angle = random.next_float() as f64 * 2.0 * PI;
                let x = radius * angle.sin() + 0.5;
                let z = radius * angle.cos() + 0.5;
                let check_start = [
                    origin[0] + x.floor() as i32,
                    origin[1] + relative_y - 1,
                    origin[2] + z.floor() as i32,
                ];
                let check_end = [check_start[0], check_start[1] + 5, check_start[2]];
                if make_fancy_limb(context, random, check_start, check_end, trunk_state, false) {
                    let dx = origin[0] - check_start[0];
                    let dz = origin[2] - check_start[2];
                    let branch_height =
                        check_start[1] as f64 - ((dx * dx + dz * dz) as f64).sqrt() * 0.381;
                    let branch_top_y = branch_height.floor() as i32;
                    let branch_base_y = branch_top_y.min(trunk_top_y);
                    let branch_base = [origin[0], branch_base_y, origin[2]];
                    if make_fancy_limb(
                        context,
                        random,
                        branch_base,
                        check_start,
                        trunk_state,
                        false,
                    ) {
                        foliage_coords.push(FancyFoliageCoord {
                            attachment: check_start,
                            branch_base_y,
                        });
                    }
                }
            }
        }
        relative_y -= 1;
    }

    let _ = make_fancy_limb(
        context,
        random,
        origin,
        [origin[0], origin[1] + trunk_height, origin[2]],
        trunk_state,
        true,
    );

    for coord in &foliage_coords {
        let branch_base = [origin[0], coord.branch_base_y, origin[2]];
        if branch_base != coord.attachment
            && fancy_trim_branches(height, coord.branch_base_y - origin[1])
        {
            let _ = make_fancy_limb(
                context,
                random,
                branch_base,
                coord.attachment,
                trunk_state,
                true,
            );
        }
    }

    let foliage_height = config
        .get("foliage_placer")
        .and_then(Value::as_object)
        .and_then(|placer| placer.get("height"))
        .and_then(|value| sample_int_provider(value, random))
        .unwrap_or(4)
        .max(0);
    let leaf_radius = config
        .get("foliage_placer")
        .and_then(Value::as_object)
        .and_then(|placer| placer.get("radius"))
        .and_then(|value| sample_int_provider(value, random))
        .unwrap_or(2)
        .max(0);
    let foliage_id = blocks::get_block_id(foliage_state);
    for coord in foliage_coords {
        if fancy_trim_branches(height, coord.branch_base_y - origin[1]) {
            place_fancy_foliage_attachment(
                context,
                coord.attachment,
                foliage_id,
                foliage_height,
                leaf_radius,
            );
        }
    }

    true
}

fn fancy_tree_shape(height: i32, y: i32) -> f64 {
    if (y as f32) < height as f32 * 0.3 {
        return -1.0;
    }
    let radius = height as f64 / 2.0;
    let adjacent = radius - y as f64;
    if adjacent.abs() >= radius {
        return 0.0;
    }
    let mut distance = (radius * radius - adjacent * adjacent).sqrt();
    if adjacent == 0.0 {
        distance = radius;
    }
    distance * 0.5
}

fn fancy_trim_branches(height: i32, local_y: i32) -> bool {
    local_y as f64 >= height as f64 * 0.2
}

fn make_fancy_limb(
    context: &mut FeaturePlacementContext<'_>,
    _random: &mut DecorationRandom,
    start_pos: [i32; 3],
    end_pos: [i32; 3],
    trunk_state: &str,
    do_place: bool,
) -> bool {
    if !do_place && start_pos == end_pos {
        return true;
    }

    let delta = [
        end_pos[0] - start_pos[0],
        end_pos[1] - start_pos[1],
        end_pos[2] - start_pos[2],
    ];
    let steps = delta[0].abs().max(delta[1].abs()).max(delta[2].abs());
    if steps == 0 {
        return true;
    }
    let dx = delta[0] as f64 / steps as f64;
    let dy = delta[1] as f64 / steps as f64;
    let dz = delta[2] as f64 / steps as f64;

    for step in 0..=steps {
        let block_x = start_pos[0] + (0.5 + step as f64 * dx).floor() as i32;
        let block_y = start_pos[1] + (0.5 + step as f64 * dy).floor() as i32;
        let block_z = start_pos[2] + (0.5 + step as f64 * dz).floor() as i32;
        if do_place {
            let axis = if (block_x - start_pos[0]).abs() >= (block_z - start_pos[2]).abs()
                && block_x != start_pos[0]
            {
                'x'
            } else if block_z != start_pos[2] {
                'z'
            } else {
                'y'
            };
            let _ = place_axis_log(context, block_x, block_y, block_z, trunk_state, axis);
        } else if !is_free_tree_space(context, block_x, block_y, block_z) {
            return false;
        }
    }

    true
}

fn place_cherry_tree_feature(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    origin: [i32; 3],
    random: &mut DecorationRandom,
    trunk_state: &str,
    foliage_state: &str,
    below_trunk_state: &str,
    force_dirt: bool,
    tree_height: i32,
) -> bool {
    let max_free_height = max_free_tree_height(context, config, origin, tree_height);
    let min_clipped_height = minimum_clipped_height(config.get("minimum_size"));
    if max_free_height < tree_height
        && min_clipped_height
            .map(|min_height| max_free_height < min_height)
            .unwrap_or(true)
    {
        return false;
    }
    let actual_height = max_free_height.min(tree_height);

    if force_dirt
        || is_replaceable_below_trunk(context.get_block(origin[0], origin[1] - 1, origin[2]))
    {
        context.set_block(
            origin[0],
            origin[1] - 1,
            origin[2],
            blocks::get_block_id(below_trunk_state),
        );
    }

    let trunk_placer = config.get("trunk_placer").and_then(Value::as_object);
    let first_branch_offset = (actual_height - 1
        + trunk_placer
            .and_then(|placer| placer.get("branch_start_offset_from_top"))
            .and_then(|value| sample_int_provider(value, random))
            .unwrap_or(-4))
    .max(0);
    let branch_start_range = trunk_placer
        .and_then(|placer| placer.get("branch_start_offset_from_top"))
        .and_then(Value::as_object);
    let second_branch_start_min = branch_start_range
        .and_then(|range| range.get("min_inclusive"))
        .and_then(Value::as_i64)
        .unwrap_or(-4) as i32;
    let second_branch_start_max = branch_start_range
        .and_then(|range| range.get("max_inclusive"))
        .and_then(Value::as_i64)
        .unwrap_or(-3) as i32
        - 1;
    let second_branch_start_offset = if second_branch_start_max >= second_branch_start_min {
        second_branch_start_min
            + random.next_int((second_branch_start_max - second_branch_start_min + 1) as u32) as i32
    } else {
        second_branch_start_min
    };
    let mut second_branch_offset = (actual_height - 1 + second_branch_start_offset).max(0);
    if second_branch_offset >= first_branch_offset {
        second_branch_offset += 1;
    }

    let branch_count = trunk_placer
        .and_then(|placer| placer.get("branch_count"))
        .and_then(|value| sample_int_provider(value, random))
        .unwrap_or(2);
    let has_middle_branch = branch_count == 3;
    let has_both_side_branches = branch_count >= 2;
    let trunk_height = if has_middle_branch {
        actual_height
    } else if has_both_side_branches {
        first_branch_offset.max(second_branch_offset) + 1
    } else {
        first_branch_offset + 1
    };

    let trunk_id = blocks::get_block_id(trunk_state);
    for y in 0..trunk_height {
        context.set_block(origin[0], origin[1] + y, origin[2], trunk_id);
    }

    let mut attachments = Vec::new();
    if has_middle_branch {
        attachments.push([origin[0], origin[1] + trunk_height, origin[2]]);
    }
    let (dir_x, dir_z) = choose_horizontal_direction(random);
    attachments.push(generate_cherry_branch(
        context,
        config,
        origin,
        random,
        trunk_state,
        actual_height,
        (dir_x, dir_z),
        first_branch_offset,
        first_branch_offset < trunk_height - 1,
    ));
    if has_both_side_branches {
        attachments.push(generate_cherry_branch(
            context,
            config,
            origin,
            random,
            trunk_state,
            actual_height,
            (-dir_x, -dir_z),
            second_branch_offset,
            second_branch_offset < trunk_height - 1,
        ));
    }

    let foliage_placer = config.get("foliage_placer").and_then(Value::as_object);
    let foliage_id = blocks::get_block_id(foliage_state);
    let foliage_height = foliage_placer
        .and_then(|placer| placer.get("height"))
        .and_then(|value| sample_int_provider(value, random))
        .unwrap_or(5)
        .max(0);
    let leaf_radius = foliage_placer
        .and_then(|placer| placer.get("radius"))
        .and_then(|value| sample_int_provider(value, random))
        .unwrap_or(4)
        .max(0);
    let wide_bottom_layer_hole_chance = foliage_placer
        .and_then(|placer| placer.get("wide_bottom_layer_hole_chance"))
        .and_then(Value::as_f64)
        .unwrap_or(0.25) as f32;
    let corner_hole_chance = foliage_placer
        .and_then(|placer| placer.get("corner_hole_chance"))
        .and_then(Value::as_f64)
        .unwrap_or(0.25) as f32;
    let hanging_leaves_chance = foliage_placer
        .and_then(|placer| placer.get("hanging_leaves_chance"))
        .and_then(Value::as_f64)
        .unwrap_or(1.0 / 6.0) as f32;
    let hanging_leaves_extension_chance = foliage_placer
        .and_then(|placer| placer.get("hanging_leaves_extension_chance"))
        .and_then(Value::as_f64)
        .unwrap_or(1.0 / 3.0) as f32;

    for attachment in attachments {
        place_cherry_foliage_attachment(
            context,
            attachment,
            foliage_id,
            foliage_height,
            leaf_radius,
            0,
            wide_bottom_layer_hole_chance,
            corner_hole_chance,
            hanging_leaves_chance,
            hanging_leaves_extension_chance,
            random,
        );
    }

    true
}

fn generate_cherry_branch(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    origin: [i32; 3],
    random: &mut DecorationRandom,
    trunk_state: &str,
    tree_height: i32,
    branch_direction: (i32, i32),
    offset_from_origin: i32,
    middle_continues_upwards: bool,
) -> [i32; 3] {
    let trunk_placer = config.get("trunk_placer").and_then(Value::as_object);
    let branch_end_offset = tree_height - 1
        + trunk_placer
            .and_then(|placer| placer.get("branch_end_offset_from_top"))
            .and_then(|value| sample_int_provider(value, random))
            .unwrap_or(0);
    let extend_branch = middle_continues_upwards || branch_end_offset < offset_from_origin;
    let distance_to_trunk = trunk_placer
        .and_then(|placer| placer.get("branch_horizontal_length"))
        .and_then(|value| sample_int_provider(value, random))
        .unwrap_or(2)
        + if extend_branch { 1 } else { 0 };
    let branch_end_pos = [
        origin[0] + branch_direction.0 * distance_to_trunk,
        origin[1] + branch_end_offset,
        origin[2] + branch_direction.1 * distance_to_trunk,
    ];

    let axis = if branch_direction.0 != 0 { 'x' } else { 'z' };
    let mut log_pos = [origin[0], origin[1] + offset_from_origin, origin[2]];
    for _ in 0..if extend_branch { 2 } else { 1 } {
        log_pos[0] += branch_direction.0;
        log_pos[2] += branch_direction.1;
        let _ = place_axis_log(
            context,
            log_pos[0],
            log_pos[1],
            log_pos[2],
            trunk_state,
            axis,
        );
    }

    while log_pos != branch_end_pos {
        let distance = (log_pos[0] - branch_end_pos[0]).abs()
            + (log_pos[1] - branch_end_pos[1]).abs()
            + (log_pos[2] - branch_end_pos[2]).abs();
        if distance == 0 {
            break;
        }
        let vertical_distance = (branch_end_pos[1] - log_pos[1]).abs() as f32;
        let grow_vertically = random.next_float() < vertical_distance / distance.max(1) as f32;
        if grow_vertically {
            log_pos[1] += if branch_end_pos[1] > log_pos[1] {
                1
            } else {
                -1
            };
            let _ = place_axis_log(
                context,
                log_pos[0],
                log_pos[1],
                log_pos[2],
                trunk_state,
                'y',
            );
        } else {
            log_pos[0] += branch_direction.0;
            log_pos[2] += branch_direction.1;
            let _ = place_axis_log(
                context,
                log_pos[0],
                log_pos[1],
                log_pos[2],
                trunk_state,
                axis,
            );
        }
    }

    [branch_end_pos[0], branch_end_pos[1] + 1, branch_end_pos[2]]
}

fn place_bending_tree_feature(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    origin: [i32; 3],
    random: &mut DecorationRandom,
    trunk_state: &str,
    foliage_state: &str,
    below_trunk_state: &str,
    force_dirt: bool,
    tree_height: i32,
) -> bool {
    let max_free_height = max_free_tree_height(context, config, origin, tree_height);
    let min_clipped_height = minimum_clipped_height(config.get("minimum_size"));
    if max_free_height < tree_height
        && min_clipped_height
            .map(|min_height| max_free_height < min_height)
            .unwrap_or(true)
    {
        return false;
    }
    let actual_height = max_free_height.min(tree_height);

    if force_dirt
        || is_replaceable_below_trunk(context.get_block(origin[0], origin[1] - 1, origin[2]))
    {
        context.set_block(
            origin[0],
            origin[1] - 1,
            origin[2],
            blocks::get_block_id(below_trunk_state),
        );
    }

    let trunk_placer = config.get("trunk_placer").and_then(Value::as_object);
    let min_height_for_leaves = trunk_placer
        .and_then(|placer| placer.get("min_height_for_leaves"))
        .and_then(Value::as_i64)
        .unwrap_or(1) as i32;
    let bend_length = trunk_placer
        .and_then(|placer| placer.get("bend_length"))
        .and_then(|value| sample_int_provider(value, random))
        .unwrap_or(1)
        .max(0);
    let mut pos = origin;
    let log_height = actual_height - 1;
    let direction = choose_horizontal_direction(random);
    let trunk_id = blocks::get_block_id(trunk_state);
    let mut attachments = Vec::new();

    for index in 0..=log_height {
        if index + 1 >= log_height + random.next_int(2) as i32 {
            pos[0] += direction.0;
            pos[2] += direction.1;
        }
        if can_place_tree_block(context, pos[0], pos[1], pos[2]) {
            context.set_block(pos[0], pos[1], pos[2], trunk_id);
        }
        if index >= min_height_for_leaves {
            attachments.push(pos);
        }
        pos[1] += 1;
    }

    for _ in 0..=bend_length {
        if can_place_tree_block(context, pos[0], pos[1], pos[2]) {
            context.set_block(pos[0], pos[1], pos[2], trunk_id);
        }
        attachments.push(pos);
        pos[0] += direction.0;
        pos[2] += direction.1;
    }

    let foliage_placer = config.get("foliage_placer").and_then(Value::as_object);
    let foliage_height = foliage_placer
        .and_then(|placer| placer.get("foliage_height"))
        .and_then(|value| sample_int_provider(value, random))
        .unwrap_or(2)
        .max(0);
    let leaf_radius = foliage_placer
        .and_then(|placer| placer.get("radius"))
        .and_then(|value| sample_int_provider(value, random))
        .unwrap_or(3)
        .max(0);
    let leaf_attempts = foliage_placer
        .and_then(|placer| placer.get("leaf_placement_attempts"))
        .and_then(Value::as_i64)
        .unwrap_or(32) as i32;
    let foliage_id = blocks::get_block_id(foliage_state);
    for attachment in attachments {
        place_random_spread_foliage(
            context,
            attachment,
            foliage_id,
            foliage_height,
            leaf_radius,
            leaf_attempts,
            random,
        );
    }

    true
}

fn place_upwards_branching_tree_feature(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    origin: [i32; 3],
    random: &mut DecorationRandom,
    trunk_state: &str,
    foliage_state: &str,
    tree_height: i32,
) -> bool {
    let trunk_origin = trunk_origin_from_root_placer(config.get("root_placer"), origin, random);
    let max_free_height = max_free_tree_height(context, config, trunk_origin, tree_height);
    let min_clipped_height = minimum_clipped_height(config.get("minimum_size"));
    if max_free_height < tree_height
        && min_clipped_height
            .map(|min_height| max_free_height < min_height)
            .unwrap_or(true)
    {
        return false;
    }
    let actual_height = max_free_height.min(tree_height);
    if !place_root_placer(
        context,
        config.get("root_placer"),
        origin,
        trunk_origin,
        random,
    ) {
        return false;
    }

    let trunk_placer = config.get("trunk_placer").and_then(Value::as_object);
    let branch_probability = trunk_placer
        .and_then(|placer| placer.get("place_branch_per_log_probability"))
        .and_then(Value::as_f64)
        .unwrap_or(0.0) as f32;
    let trunk_id = blocks::get_block_id(trunk_state);
    let mut attachments = Vec::new();

    for height_pos in 0..actual_height {
        let current_y = trunk_origin[1] + height_pos;
        if can_place_upwards_branching_log(
            context,
            trunk_origin[0],
            current_y,
            trunk_origin[2],
            trunk_placer,
        ) {
            context.set_block(trunk_origin[0], current_y, trunk_origin[2], trunk_id);
            if height_pos < actual_height - 1 && random.next_float() < branch_probability {
                let branch_direction = choose_horizontal_direction(random);
                let branch_len = trunk_placer
                    .and_then(|placer| placer.get("extra_branch_length"))
                    .and_then(|value| sample_int_provider(value, random))
                    .unwrap_or(0)
                    .max(0);
                let branch_pos = (branch_len
                    - trunk_placer
                        .and_then(|placer| placer.get("extra_branch_length"))
                        .and_then(|value| sample_int_provider(value, random))
                        .unwrap_or(0)
                    - 1)
                .max(0);
                let branch_steps = trunk_placer
                    .and_then(|placer| placer.get("extra_branch_steps"))
                    .and_then(|value| sample_int_provider(value, random))
                    .unwrap_or(1)
                    .max(0);
                place_upwards_branch(
                    context,
                    trunk_placer,
                    trunk_state,
                    random,
                    actual_height,
                    trunk_origin,
                    current_y,
                    branch_direction,
                    branch_pos,
                    branch_steps,
                    &mut attachments,
                );
            }
        }

        if height_pos == actual_height - 1 {
            attachments.push([trunk_origin[0], current_y + 1, trunk_origin[2]]);
        }
    }

    let foliage_placer = config.get("foliage_placer").and_then(Value::as_object);
    let foliage_height = foliage_placer
        .and_then(|placer| placer.get("foliage_height"))
        .and_then(|value| sample_int_provider(value, random))
        .unwrap_or(2)
        .max(0);
    let leaf_radius = foliage_placer
        .and_then(|placer| placer.get("radius"))
        .and_then(|value| sample_int_provider(value, random))
        .unwrap_or(3)
        .max(0);
    let leaf_attempts = foliage_placer
        .and_then(|placer| placer.get("leaf_placement_attempts"))
        .and_then(Value::as_i64)
        .unwrap_or(70) as i32;
    let foliage_id = blocks::get_block_id(foliage_state);
    for attachment in attachments {
        place_random_spread_foliage(
            context,
            attachment,
            foliage_id,
            foliage_height,
            leaf_radius,
            leaf_attempts,
            random,
        );
    }

    true
}

fn can_place_upwards_branching_log(
    context: &mut FeaturePlacementContext<'_>,
    block_x: i32,
    block_y: i32,
    block_z: i32,
    trunk_placer: Option<&serde_json::Map<String, Value>>,
) -> bool {
    let block_id = context.get_block(block_x, block_y, block_z);
    can_place_tree_block(context, block_x, block_y, block_z)
        || trunk_placer
            .and_then(|placer| placer.get("can_grow_through"))
            .map(|selector| block_matches_selector(block_id, selector))
            .unwrap_or(false)
}

fn place_upwards_branch(
    context: &mut FeaturePlacementContext<'_>,
    trunk_placer: Option<&serde_json::Map<String, Value>>,
    trunk_state: &str,
    _random: &mut DecorationRandom,
    tree_height: i32,
    trunk_origin: [i32; 3],
    current_height: i32,
    branch_direction: (i32, i32),
    branch_pos: i32,
    mut branch_steps: i32,
    attachments: &mut Vec<[i32; 3]>,
) {
    let mut height_along_branch = current_height + branch_pos;
    let mut log_x = trunk_origin[0];
    let mut log_z = trunk_origin[2];
    let mut placement_index = branch_pos;
    let trunk_id = blocks::get_block_id(trunk_state);

    while placement_index < tree_height && branch_steps > 0 {
        if placement_index >= 1 {
            let placement_height = current_height + placement_index;
            log_x += branch_direction.0;
            log_z += branch_direction.1;
            height_along_branch = placement_height;
            if can_place_upwards_branching_log(
                context,
                log_x,
                placement_height,
                log_z,
                trunk_placer,
            ) {
                context.set_block(log_x, placement_height, log_z, trunk_id);
                height_along_branch = placement_height + 1;
            }
            attachments.push([log_x, placement_height, log_z]);
        }

        placement_index += 1;
        branch_steps -= 1;
    }

    if height_along_branch - current_height > 1 {
        attachments.push([log_x, height_along_branch, log_z]);
        attachments.push([log_x, height_along_branch - 2, log_z]);
    }
}

fn place_mega_pine_tree_feature(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    origin: [i32; 3],
    random: &mut DecorationRandom,
    trunk_state: &str,
    foliage_state: &str,
    below_trunk_state: &str,
    force_dirt: bool,
    tree_height: i32,
) -> bool {
    let max_free_height = max_free_tree_height_for_offsets(
        context,
        config,
        origin,
        tree_height,
        &[(0, 0), (1, 0), (0, 1), (1, 1)],
    );
    let min_clipped_height = minimum_clipped_height(config.get("minimum_size"));
    if max_free_height < tree_height
        && min_clipped_height
            .map(|min_height| max_free_height < min_height)
            .unwrap_or(true)
    {
        return false;
    }
    let actual_height = max_free_height.min(tree_height);

    let trunk_id = blocks::get_block_id(trunk_state);
    place_giant_trunk(context, origin, trunk_id, below_trunk_state, force_dirt);
    for height in 0..actual_height {
        context.set_block(origin[0], origin[1] + height, origin[2], trunk_id);
        if height < actual_height - 1 {
            context.set_block(origin[0] + 1, origin[1] + height, origin[2], trunk_id);
            context.set_block(origin[0] + 1, origin[1] + height, origin[2] + 1, trunk_id);
            context.set_block(origin[0], origin[1] + height, origin[2] + 1, trunk_id);
        }
    }

    let foliage_placer = config.get("foliage_placer").and_then(Value::as_object);
    let foliage_height = foliage_placer
        .and_then(|placer| placer.get("crown_height"))
        .and_then(|value| sample_int_provider(value, random))
        .unwrap_or(4)
        .max(0);
    let leaf_radius = foliage_placer
        .and_then(|placer| placer.get("radius"))
        .and_then(|value| sample_int_provider(value, random))
        .unwrap_or(0)
        .max(0);
    place_mega_pine_foliage(
        context,
        [origin[0], origin[1] + actual_height, origin[2]],
        blocks::get_block_id(foliage_state),
        foliage_height,
        leaf_radius,
    );

    true
}

fn place_mega_jungle_tree_feature(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    origin: [i32; 3],
    random: &mut DecorationRandom,
    trunk_state: &str,
    foliage_state: &str,
    below_trunk_state: &str,
    force_dirt: bool,
    tree_height: i32,
) -> bool {
    let max_free_height = max_free_tree_height_for_offsets(
        context,
        config,
        origin,
        tree_height,
        &[(0, 0), (1, 0), (0, 1), (1, 1)],
    );
    let min_clipped_height = minimum_clipped_height(config.get("minimum_size"));
    if max_free_height < tree_height
        && min_clipped_height
            .map(|min_height| max_free_height < min_height)
            .unwrap_or(true)
    {
        return false;
    }
    let actual_height = max_free_height.min(tree_height);

    let trunk_id = blocks::get_block_id(trunk_state);
    place_giant_trunk(context, origin, trunk_id, below_trunk_state, force_dirt);
    for height in 0..actual_height {
        context.set_block(origin[0], origin[1] + height, origin[2], trunk_id);
        if height < actual_height - 1 {
            context.set_block(origin[0] + 1, origin[1] + height, origin[2], trunk_id);
            context.set_block(origin[0] + 1, origin[1] + height, origin[2] + 1, trunk_id);
            context.set_block(origin[0], origin[1] + height, origin[2] + 1, trunk_id);
        }
    }

    let foliage_id = blocks::get_block_id(foliage_state);
    let foliage_placer = config.get("foliage_placer").and_then(Value::as_object);
    let foliage_height = foliage_placer
        .and_then(|placer| placer.get("height"))
        .and_then(|value| sample_int_provider(value, random))
        .unwrap_or(2)
        .max(0);
    let leaf_radius = foliage_placer
        .and_then(|placer| placer.get("radius"))
        .and_then(|value| sample_int_provider(value, random))
        .unwrap_or(2)
        .max(0);

    let mut branch_height = actual_height - 2 - random.next_int(4) as i32;
    while branch_height > actual_height / 2 {
        let angle = random.next_float() as f64 * 2.0 * PI;
        let mut branch_x = 0;
        let mut branch_z = 0;
        for branch_index in 0..5 {
            branch_x = (1.5 + angle.cos() * branch_index as f64) as i32;
            branch_z = (1.5 + angle.sin() * branch_index as f64) as i32;
            context.set_block(
                origin[0] + branch_x,
                origin[1] + branch_height - 3 + branch_index / 2,
                origin[2] + branch_z,
                trunk_id,
            );
        }
        place_mega_jungle_foliage(
            context,
            [
                origin[0] + branch_x,
                origin[1] + branch_height,
                origin[2] + branch_z,
            ],
            foliage_id,
            foliage_height,
            leaf_radius,
            -2,
            false,
            random,
        );
        branch_height -= 2 + random.next_int(4) as i32;
    }

    place_mega_jungle_foliage(
        context,
        [origin[0], origin[1] + actual_height, origin[2]],
        foliage_id,
        foliage_height,
        leaf_radius,
        0,
        true,
        random,
    );

    true
}

fn trunk_origin_from_root_placer(
    root_placer: Option<&Value>,
    origin: [i32; 3],
    random: &mut DecorationRandom,
) -> [i32; 3] {
    let Some(root_placer) = root_placer.and_then(Value::as_object) else {
        return origin;
    };
    let offset_y = root_placer
        .get("trunk_offset_y")
        .and_then(|value| sample_int_provider(value, random))
        .unwrap_or(0);
    [origin[0], origin[1] + offset_y, origin[2]]
}

fn place_root_placer(
    context: &mut FeaturePlacementContext<'_>,
    root_placer: Option<&Value>,
    origin: [i32; 3],
    trunk_origin: [i32; 3],
    random: &mut DecorationRandom,
) -> bool {
    let Some(root_placer) = root_placer.and_then(Value::as_object) else {
        return true;
    };
    match root_placer
        .get("type")
        .and_then(Value::as_str)
        .map(normalize_registry_name)
    {
        Some("mangrove_root_placer") => {
            place_mangrove_root_placer(context, root_placer, origin, trunk_origin, random)
        }
        _ => true,
    }
}

fn place_mangrove_root_placer(
    context: &mut FeaturePlacementContext<'_>,
    root_placer: &serde_json::Map<String, Value>,
    origin: [i32; 3],
    trunk_origin: [i32; 3],
    random: &mut DecorationRandom,
) -> bool {
    let mut column_y = origin[1];
    while column_y < trunk_origin[1] {
        if !can_place_root(context, root_placer, [origin[0], column_y, origin[2]]) {
            return false;
        }
        column_y += 1;
    }

    let mut root_positions = vec![[trunk_origin[0], trunk_origin[1] - 1, trunk_origin[2]]];
    for direction in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        let root_pos = [
            trunk_origin[0] + direction.0,
            trunk_origin[1],
            trunk_origin[2] + direction.1,
        ];
        let mut direction_positions = Vec::new();
        if !simulate_mangrove_roots(
            context,
            root_placer,
            random,
            root_pos,
            direction,
            trunk_origin,
            &mut direction_positions,
            0,
        ) {
            return false;
        }
        root_positions.extend(direction_positions);
        root_positions.push(root_pos);
    }

    for root_pos in root_positions {
        place_single_root(context, root_placer, random, root_pos);
    }
    true
}

fn simulate_mangrove_roots(
    context: &mut FeaturePlacementContext<'_>,
    root_placer: &serde_json::Map<String, Value>,
    random: &mut DecorationRandom,
    root_pos: [i32; 3],
    direction: (i32, i32),
    root_origin: [i32; 3],
    root_positions: &mut Vec<[i32; 3]>,
    layer: i32,
) -> bool {
    let placement = root_placer
        .get("mangrove_root_placement")
        .and_then(Value::as_object);
    let max_root_length = placement
        .and_then(|placement| placement.get("max_root_length"))
        .and_then(Value::as_i64)
        .unwrap_or(15) as i32;
    if layer == max_root_length || root_positions.len() > max_root_length as usize {
        return false;
    }

    for pos in
        potential_mangrove_root_positions(root_placer, root_pos, direction, random, root_origin)
    {
        if can_place_root(context, root_placer, pos) {
            root_positions.push(pos);
            if !simulate_mangrove_roots(
                context,
                root_placer,
                random,
                pos,
                direction,
                root_origin,
                root_positions,
                layer + 1,
            ) {
                return false;
            }
        }
    }

    true
}

fn potential_mangrove_root_positions(
    root_placer: &serde_json::Map<String, Value>,
    pos: [i32; 3],
    prev_dir: (i32, i32),
    random: &mut DecorationRandom,
    root_origin: [i32; 3],
) -> Vec<[i32; 3]> {
    let placement = root_placer
        .get("mangrove_root_placement")
        .and_then(Value::as_object);
    let below = [pos[0], pos[1] - 1, pos[2]];
    let next_to = [pos[0] + prev_dir.0, pos[1], pos[2] + prev_dir.1];
    let width = (pos[0] - root_origin[0]).abs()
        + (pos[1] - root_origin[1]).abs()
        + (pos[2] - root_origin[2]).abs();
    let max_root_width = placement
        .and_then(|placement| placement.get("max_root_width"))
        .and_then(Value::as_i64)
        .unwrap_or(8) as i32;
    let random_skew_chance = placement
        .and_then(|placement| placement.get("random_skew_chance"))
        .and_then(Value::as_f64)
        .unwrap_or(0.2) as f32;
    if width > max_root_width - 3 && width <= max_root_width {
        if random.next_float() < random_skew_chance {
            vec![below, [next_to[0], next_to[1] - 1, next_to[2]]]
        } else {
            vec![below]
        }
    } else if width > max_root_width || random.next_float() < random_skew_chance {
        vec![below]
    } else if random.next_bool() {
        vec![next_to]
    } else {
        vec![below]
    }
}

fn can_place_root(
    context: &mut FeaturePlacementContext<'_>,
    root_placer: &serde_json::Map<String, Value>,
    pos: [i32; 3],
) -> bool {
    let block_id = context.get_block(pos[0], pos[1], pos[2]);
    can_place_tree_block(context, pos[0], pos[1], pos[2])
        || root_placer
            .get("mangrove_root_placement")
            .and_then(Value::as_object)
            .and_then(|placement| placement.get("can_grow_through"))
            .map(|selector| block_matches_selector(block_id, selector))
            .unwrap_or(false)
}

fn place_single_root(
    context: &mut FeaturePlacementContext<'_>,
    root_placer: &serde_json::Map<String, Value>,
    random: &mut DecorationRandom,
    pos: [i32; 3],
) {
    let placement = root_placer
        .get("mangrove_root_placement")
        .and_then(Value::as_object);
    let muddy_roots_in = placement.and_then(|placement| placement.get("muddy_roots_in"));
    let current = context.get_block(pos[0], pos[1], pos[2]);
    let root_state = if muddy_roots_in
        .map(|selector| block_matches_selector(current, selector))
        .unwrap_or(false)
    {
        placement.and_then(|placement| placement.get("muddy_roots_provider"))
    } else {
        root_placer.get("root_provider")
    };
    let root_name = state_provider_block_name(root_state).unwrap_or("minecraft:mangrove_roots");
    context.set_block(pos[0], pos[1], pos[2], blocks::get_block_id(root_name));

    if let Some(above_root_placement) = root_placer
        .get("above_root_placement")
        .and_then(Value::as_object)
    {
        let chance = above_root_placement
            .get("above_root_placement_chance")
            .and_then(Value::as_f64)
            .unwrap_or(0.0) as f32;
        let above = [pos[0], pos[1] + 1, pos[2]];
        if random.next_float() < chance
            && is_air_block(context.get_block(above[0], above[1], above[2]))
        {
            let above_name =
                state_provider_block_name(above_root_placement.get("above_root_provider"))
                    .unwrap_or("minecraft:moss_carpet");
            context.set_block(
                above[0],
                above[1],
                above[2],
                blocks::get_block_id(above_name),
            );
        }
    }
}

fn block_matches_selector(block_id: u32, selector: &Value) -> bool {
    if let Some(name) = selector.as_str() {
        return block_matches_name(block_id, name);
    }
    selector
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .any(|entry| block_matches_selector(block_id, entry))
        })
        .unwrap_or(false)
}

fn block_matches_name(block_id: u32, name: &str) -> bool {
    if let Some(tag) = name.strip_prefix('#') {
        return match tag {
            "minecraft:mangrove_logs_can_grow_through" => matches!(
                block_name(block_id),
                "minecraft:mud"
                    | "minecraft:muddy_mangrove_roots"
                    | "minecraft:mangrove_roots"
                    | "minecraft:mangrove_leaves"
                    | "minecraft:mangrove_log"
                    | "minecraft:mangrove_propagule"
                    | "minecraft:moss_carpet"
                    | "minecraft:vine"
            ),
            "minecraft:mangrove_roots_can_grow_through" => matches!(
                block_name(block_id),
                "minecraft:mud"
                    | "minecraft:muddy_mangrove_roots"
                    | "minecraft:mangrove_roots"
                    | "minecraft:moss_carpet"
                    | "minecraft:vine"
                    | "minecraft:mangrove_propagule"
                    | "minecraft:snow"
            ),
            _ => false,
        };
    }
    block_name(block_id) == name
}

fn place_giant_trunk(
    context: &mut FeaturePlacementContext<'_>,
    origin: [i32; 3],
    _trunk_id: u32,
    below_trunk_state: &str,
    force_dirt: bool,
) {
    let below_positions = [
        [origin[0], origin[1] - 1, origin[2]],
        [origin[0] + 1, origin[1] - 1, origin[2]],
        [origin[0], origin[1] - 1, origin[2] + 1],
        [origin[0] + 1, origin[1] - 1, origin[2] + 1],
    ];
    if force_dirt
        || below_positions
            .iter()
            .any(|pos| is_replaceable_below_trunk(context.get_block(pos[0], pos[1], pos[2])))
    {
        let below_id = blocks::get_block_id(below_trunk_state);
        for pos in below_positions {
            context.set_block(pos[0], pos[1], pos[2], below_id);
        }
    }
}

fn place_dark_oak_tree_feature(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    origin: [i32; 3],
    random: &mut DecorationRandom,
    trunk_state: &str,
    foliage_state: &str,
    below_trunk_state: &str,
    force_dirt: bool,
    tree_height: i32,
) -> bool {
    let max_free_height = max_free_tree_height_for_offsets(
        context,
        config,
        origin,
        tree_height,
        &[(0, 0), (1, 0), (0, 1), (1, 1)],
    );
    let min_clipped_height = minimum_clipped_height(config.get("minimum_size"));
    if max_free_height < tree_height
        && min_clipped_height
            .map(|min_height| max_free_height < min_height)
            .unwrap_or(true)
    {
        return false;
    }
    let actual_height = max_free_height.min(tree_height);

    let base_positions = [
        [origin[0], origin[1] - 1, origin[2]],
        [origin[0] + 1, origin[1] - 1, origin[2]],
        [origin[0], origin[1] - 1, origin[2] + 1],
        [origin[0] + 1, origin[1] - 1, origin[2] + 1],
    ];
    if force_dirt
        || base_positions.iter().any(|position| {
            is_replaceable_below_trunk(context.get_block(position[0], position[1], position[2]))
        })
    {
        let below_trunk_id = blocks::get_block_id(below_trunk_state);
        for position in base_positions {
            context.set_block(position[0], position[1], position[2], below_trunk_id);
        }
    }

    let directions = [(1, 0), (-1, 0), (0, 1), (0, -1)];
    let (step_x, step_z) = directions[random.next_int(directions.len() as u32) as usize];
    let lean_height = actual_height - random.next_int(4) as i32;
    let mut lean_steps = 2 - random.next_int(3) as i32;
    let mut trunk_x = origin[0];
    let mut trunk_z = origin[2];
    let trunk_top_y = origin[1] + actual_height - 1;
    let trunk_id = blocks::get_block_id(trunk_state);
    let foliage_id = blocks::get_block_id(foliage_state);

    for dy in 0..actual_height {
        if dy >= lean_height && lean_steps > 0 {
            trunk_x += step_x;
            trunk_z += step_z;
            lean_steps -= 1;
        }

        let block_y = origin[1] + dy;
        for (offset_x, offset_z) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let block_x = trunk_x + offset_x;
            let block_z = trunk_z + offset_z;
            let existing = context.get_block(block_x, block_y, block_z);
            if is_air_block(existing)
                || is_leaves_block(existing)
                || is_replaceable_by_trees_block(existing)
            {
                context.set_block(block_x, block_y, block_z, trunk_id);
            }
        }
    }

    let leaf_radius = config
        .get("foliage_placer")
        .and_then(Value::as_object)
        .and_then(|placer| placer.get("radius"))
        .and_then(|value| sample_int_provider(value, random))
        .unwrap_or(0)
        .max(0);
    let foliage_offset = config
        .get("foliage_placer")
        .and_then(Value::as_object)
        .and_then(|placer| placer.get("offset"))
        .and_then(|value| sample_int_provider(value, random))
        .unwrap_or(0);

    place_dark_oak_foliage_attachment(
        context,
        [trunk_x, trunk_top_y + foliage_offset, trunk_z],
        foliage_id,
        leaf_radius,
        true,
        random,
    );

    for offset_x in -1..=2 {
        for offset_z in -1..=2 {
            if (0..=1).contains(&offset_x) && (0..=1).contains(&offset_z) {
                continue;
            }
            if random.next_int(3) != 0 {
                continue;
            }

            let branch_length = random.next_int(3) as i32 + 2;
            for branch_y in 0..branch_length {
                let block_x = origin[0] + offset_x;
                let block_y = trunk_top_y - branch_y - 1;
                let block_z = origin[2] + offset_z;
                if can_place_tree_block(context, block_x, block_y, block_z) {
                    context.set_block(block_x, block_y, block_z, trunk_id);
                }
            }
            place_dark_oak_foliage_attachment(
                context,
                [
                    origin[0] + offset_x,
                    trunk_top_y + foliage_offset,
                    origin[2] + offset_z,
                ],
                foliage_id,
                leaf_radius,
                false,
                random,
            );
        }
    }

    true
}

fn place_acacia_tree_feature(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    origin: [i32; 3],
    random: &mut DecorationRandom,
    trunk_state: &str,
    foliage_state: &str,
    below_trunk_state: &str,
    force_dirt: bool,
    tree_height: i32,
) -> bool {
    let max_free_height = max_free_tree_height(context, config, origin, tree_height);
    let min_clipped_height = minimum_clipped_height(config.get("minimum_size"));
    if max_free_height < tree_height
        && min_clipped_height
            .map(|min_height| max_free_height < min_height)
            .unwrap_or(true)
    {
        return false;
    }
    let actual_height = max_free_height.min(tree_height);

    if force_dirt
        || is_replaceable_below_trunk(context.get_block(origin[0], origin[1] - 1, origin[2]))
    {
        context.set_block(
            origin[0],
            origin[1] - 1,
            origin[2],
            blocks::get_block_id(below_trunk_state),
        );
    }

    let directions = [(1, 0), (-1, 0), (0, 1), (0, -1)];
    let (lean_x, lean_z) = directions[random.next_int(directions.len() as u32) as usize];
    let lean_height = actual_height - random.next_int(4) as i32 - 1;
    let mut lean_steps = 3 - random.next_int(3) as i32;
    let trunk_id = blocks::get_block_id(trunk_state);
    let foliage_id = blocks::get_block_id(foliage_state);
    let mut attachments = Vec::new();
    let mut trunk_x = origin[0];
    let mut trunk_z = origin[2];
    let mut main_attachment_y = None;

    for yo in 0..actual_height {
        if yo >= lean_height && lean_steps > 0 {
            trunk_x += lean_x;
            trunk_z += lean_z;
            lean_steps -= 1;
        }

        let block_y = origin[1] + yo;
        let existing = context.get_block(trunk_x, block_y, trunk_z);
        if is_air_block(existing)
            || is_leaves_block(existing)
            || is_replaceable_by_trees_block(existing)
        {
            context.set_block(trunk_x, block_y, trunk_z, trunk_id);
            main_attachment_y = Some(block_y + 1);
        }
    }

    if let Some(attachment_y) = main_attachment_y {
        attachments.push(([trunk_x, attachment_y, trunk_z], 1));
    }

    let (branch_x, branch_z) = directions[random.next_int(directions.len() as u32) as usize];
    if (branch_x, branch_z) != (lean_x, lean_z) {
        let mut branch_pos = lean_height - random.next_int(2) as i32 - 1;
        let mut branch_steps = 1 + random.next_int(3) as i32;
        let mut attachment_y = None;
        let mut branch_trunk_x = origin[0];
        let mut branch_trunk_z = origin[2];

        while branch_pos < actual_height && branch_steps > 0 {
            if branch_pos >= 1 {
                let block_y = origin[1] + branch_pos;
                branch_trunk_x += branch_x;
                branch_trunk_z += branch_z;
                let existing = context.get_block(branch_trunk_x, block_y, branch_trunk_z);
                if is_air_block(existing)
                    || is_leaves_block(existing)
                    || is_replaceable_by_trees_block(existing)
                {
                    context.set_block(branch_trunk_x, block_y, branch_trunk_z, trunk_id);
                    attachment_y = Some(block_y + 1);
                }
            }
            branch_pos += 2;
            branch_steps -= 1;
        }

        if let Some(attachment_y) = attachment_y {
            attachments.push(([branch_trunk_x, attachment_y, branch_trunk_z], 0));
        }
    }

    let foliage_offset = config
        .get("foliage_placer")
        .and_then(Value::as_object)
        .and_then(|placer| placer.get("offset"))
        .and_then(|value| sample_int_provider(value, random))
        .unwrap_or(0);
    let base_leaf_radius = config
        .get("foliage_placer")
        .and_then(Value::as_object)
        .and_then(|placer| placer.get("radius"))
        .and_then(|value| sample_int_provider(value, random))
        .unwrap_or(2)
        .max(0);

    for (attachment, radius_offset) in attachments {
        place_acacia_foliage_attachment(
            context,
            [attachment[0], attachment[1] + foliage_offset, attachment[2]],
            foliage_id,
            base_leaf_radius,
            radius_offset,
        );
    }

    true
}

fn place_acacia_foliage_attachment(
    context: &mut FeaturePlacementContext<'_>,
    attachment: [i32; 3],
    foliage_id: u32,
    base_leaf_radius: i32,
    radius_offset: i32,
) {
    place_conical_leaves_row(
        context,
        attachment,
        foliage_id,
        base_leaf_radius + radius_offset,
        -1,
        |dx, dz, _| (dx.abs() > 1 || dz.abs() > 1) && dx != 0 && dz != 0,
    );
    place_conical_leaves_row(
        context,
        attachment,
        foliage_id,
        (base_leaf_radius - 1).max(0),
        0,
        |dx, dz, radius| dx.abs() == radius && dz.abs() == radius && radius > 0,
    );
    place_conical_leaves_row(
        context,
        attachment,
        foliage_id,
        (base_leaf_radius + radius_offset - 1).max(0),
        0,
        |dx, dz, radius| dx.abs() == radius && dz.abs() == radius && radius > 0,
    );
}

fn place_dark_oak_foliage_attachment(
    context: &mut FeaturePlacementContext<'_>,
    origin: [i32; 3],
    foliage_id: u32,
    leaf_radius: i32,
    double_trunk: bool,
    random: &mut DecorationRandom,
) {
    if double_trunk {
        place_dark_oak_leaves_row(context, origin, foliage_id, leaf_radius + 2, -1, true);
        place_dark_oak_leaves_row(context, origin, foliage_id, leaf_radius + 3, 0, true);
        place_dark_oak_leaves_row(context, origin, foliage_id, leaf_radius + 2, 1, true);
        if random.next_bool() {
            place_dark_oak_leaves_row(context, origin, foliage_id, leaf_radius, 2, true);
        }
    } else {
        place_dark_oak_leaves_row(context, origin, foliage_id, leaf_radius + 2, -1, false);
        place_dark_oak_leaves_row(context, origin, foliage_id, leaf_radius + 1, 0, false);
    }
}

fn place_dark_oak_leaves_row(
    context: &mut FeaturePlacementContext<'_>,
    origin: [i32; 3],
    foliage_id: u32,
    current_radius: i32,
    y_offset: i32,
    double_trunk: bool,
) {
    let extra = if double_trunk { 1 } else { 0 };
    for dx in -current_radius..=current_radius + extra {
        for dz in -current_radius..=current_radius + extra {
            if should_skip_dark_oak_leaf(dx, dz, y_offset, current_radius, double_trunk) {
                continue;
            }

            let block_x = origin[0] + dx;
            let block_y = origin[1] + y_offset;
            let block_z = origin[2] + dz;
            let existing = context.get_block(block_x, block_y, block_z);
            if is_air_block(existing)
                || is_leaves_block(existing)
                || is_replaceable_by_trees_block(existing)
            {
                context.set_block(block_x, block_y, block_z, foliage_id);
            }
        }
    }
}

fn should_skip_dark_oak_leaf(
    dx: i32,
    dz: i32,
    y_offset: i32,
    current_radius: i32,
    double_trunk: bool,
) -> bool {
    if y_offset == 0 && double_trunk {
        if (dx == -current_radius || dx >= current_radius)
            && (dz == -current_radius || dz >= current_radius)
        {
            return true;
        }
    }

    let min_dx = if double_trunk {
        dx.abs().min((dx - 1).abs())
    } else {
        dx.abs()
    };
    let min_dz = if double_trunk {
        dz.abs().min((dz - 1).abs())
    } else {
        dz.abs()
    };
    if y_offset == -1 && !double_trunk {
        min_dx == current_radius && min_dz == current_radius
    } else if y_offset == 1 {
        min_dx + min_dz > current_radius * 2 - 2
    } else {
        false
    }
}

fn place_blob_foliage(
    context: &mut FeaturePlacementContext<'_>,
    origin: [i32; 3],
    foliage_id: u32,
    foliage_height: i32,
    leaf_radius: i32,
    random: &mut DecorationRandom,
) {
    for yo in (origin[1] - foliage_height)..=origin[1] {
        let y_offset = yo - origin[1];
        let current_radius = (leaf_radius - 1 - y_offset / 2).max(0);
        for dx in -current_radius..=current_radius {
            for dz in -current_radius..=current_radius {
                if dx.abs() == current_radius
                    && dz.abs() == current_radius
                    && (random.next_int(2) == 0 || y_offset == 0)
                {
                    continue;
                }
                let block_x = origin[0] + dx;
                let block_z = origin[2] + dz;
                let block_id = context.get_block(block_x, yo, block_z);
                if is_air_block(block_id)
                    || is_leaves_block(block_id)
                    || is_replaceable_by_trees_block(block_id)
                {
                    context.set_block(block_x, yo, block_z, foliage_id);
                }
            }
        }
    }
}

fn max_free_tree_height(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    origin: [i32; 3],
    max_tree_height: i32,
) -> i32 {
    max_free_tree_height_for_offsets(context, config, origin, max_tree_height, &[(0, 0)])
}

fn max_free_tree_height_for_offsets(
    context: &mut FeaturePlacementContext<'_>,
    config: &Value,
    origin: [i32; 3],
    max_tree_height: i32,
    footprint: &[(i32, i32)],
) -> i32 {
    let ignore_vines = config
        .get("ignore_vines")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    for y in 0..=max_tree_height + 1 {
        let radius = feature_size_at_height(config.get("minimum_size"), max_tree_height, y);
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                for (footprint_x, footprint_z) in footprint {
                    let block_id = context.get_block(
                        origin[0] + footprint_x + dx,
                        origin[1] + y,
                        origin[2] + footprint_z + dz,
                    );
                    if (!ignore_vines && is_vine_block(block_id))
                        || !(is_air_block(block_id)
                            || is_leaves_block(block_id)
                            || is_replaceable_by_trees_block(block_id))
                    {
                        return y - 2;
                    }
                }
            }
        }
    }

    max_tree_height
}

fn feature_size_at_height(minimum_size: Option<&Value>, tree_height: i32, y: i32) -> i32 {
    let Some(size) = minimum_size.and_then(Value::as_object) else {
        return 0;
    };
    match size
        .get("type")
        .and_then(Value::as_str)
        .map(normalize_registry_name)
    {
        Some("three_layers_feature_size") => {
            let limit = size.get("limit").and_then(Value::as_i64).unwrap_or(1) as i32;
            let upper_limit = size.get("upper_limit").and_then(Value::as_i64).unwrap_or(1) as i32;
            let lower_size = size.get("lower_size").and_then(Value::as_i64).unwrap_or(0) as i32;
            let middle_size = size.get("middle_size").and_then(Value::as_i64).unwrap_or(1) as i32;
            let upper_size = size.get("upper_size").and_then(Value::as_i64).unwrap_or(1) as i32;
            if y < limit {
                lower_size
            } else if y >= tree_height - upper_limit {
                upper_size
            } else {
                middle_size
            }
        }
        _ => {
            let limit = size.get("limit").and_then(Value::as_i64).unwrap_or(1) as i32;
            let lower_size = size.get("lower_size").and_then(Value::as_i64).unwrap_or(0) as i32;
            let upper_size = size.get("upper_size").and_then(Value::as_i64).unwrap_or(1) as i32;
            if y < limit { lower_size } else { upper_size }
        }
    }
}

fn minimum_clipped_height(minimum_size: Option<&Value>) -> Option<i32> {
    minimum_size
        .and_then(Value::as_object)
        .and_then(|size| size.get("min_clipped_height"))
        .and_then(Value::as_i64)
        .map(|value| value as i32)
}

fn tree_height_from_trunk_placer(
    trunk_placer: Option<&Value>,
    random: &mut DecorationRandom,
) -> i32 {
    let Some(placer) = trunk_placer.and_then(Value::as_object) else {
        return 0;
    };
    let base_height = placer
        .get("base_height")
        .and_then(Value::as_i64)
        .unwrap_or(0) as i32;
    let height_rand_a = placer
        .get("height_rand_a")
        .and_then(Value::as_i64)
        .unwrap_or(0) as i32;
    let height_rand_b = placer
        .get("height_rand_b")
        .and_then(Value::as_i64)
        .unwrap_or(0) as i32;
    base_height
        + random.next_int((height_rand_a + 1).max(1) as u32) as i32
        + random.next_int((height_rand_b + 1).max(1) as u32) as i32
}

fn state_provider_block_name(provider: Option<&Value>) -> Option<&str> {
    let provider = provider?.as_object()?;
    match provider
        .get("type")
        .and_then(Value::as_str)
        .map(normalize_registry_name)
    {
        Some("simple_state_provider") => provider
            .get("state")
            .and_then(Value::as_object)
            .and_then(|state| state.get("Name"))
            .and_then(Value::as_str),
        Some("weighted_state_provider") => provider
            .get("entries")
            .and_then(Value::as_array)
            .and_then(|entries| entries.first())
            .and_then(Value::as_object)
            .and_then(|entry| entry.get("data"))
            .and_then(Value::as_object)
            .and_then(|state| state.get("Name"))
            .and_then(Value::as_str),
        Some("rule_based_state_provider") => provider
            .get("fallback")
            .and_then(|fallback| state_provider_block_name(Some(fallback)))
            .or_else(|| {
                provider
                    .get("rules")
                    .and_then(Value::as_array)
                    .and_then(|rules| rules.first())
                    .and_then(Value::as_object)
                    .and_then(|rule| rule.get("then"))
                    .and_then(|then_provider| state_provider_block_name(Some(then_provider)))
            }),
        _ => provider
            .get("state")
            .and_then(Value::as_object)
            .and_then(|state| state.get("Name"))
            .and_then(Value::as_str),
    }
}

#[derive(Debug, Clone, Default)]
struct FeatureStateSelection {
    name: String,
    properties: HashMap<String, String>,
}

fn collect_provider_block_names(provider: Option<&Value>, names: &mut HashSet<String>) {
    let Some(provider) = provider.and_then(Value::as_object) else {
        return;
    };
    match provider
        .get("type")
        .and_then(Value::as_str)
        .map(normalize_registry_name)
    {
        Some("simple_state_provider") => {
            if let Some(name) = provider
                .get("state")
                .and_then(Value::as_object)
                .and_then(|state| state.get("Name"))
                .and_then(Value::as_str)
            {
                names.insert(normalize_feature_state_name(name));
            }
        }
        Some("weighted_state_provider") => {
            if let Some(entries) = provider.get("entries").and_then(Value::as_array) {
                for entry in entries {
                    collect_state_object_name(entry.get("data").and_then(Value::as_object), names);
                }
            }
        }
        Some("randomized_int_state_provider") => {
            collect_provider_block_names(provider.get("source"), names);
        }
        Some("noise_threshold_provider") => {
            collect_state_object_name(
                provider.get("default_state").and_then(Value::as_object),
                names,
            );
            if let Some(states) = provider.get("low_states").and_then(Value::as_array) {
                for state in states {
                    collect_state_object_name(state.as_object(), names);
                }
            }
            if let Some(states) = provider.get("high_states").and_then(Value::as_array) {
                for state in states {
                    collect_state_object_name(state.as_object(), names);
                }
            }
        }
        Some("noise_provider") | Some("dual_noise_provider") => {
            if let Some(states) = provider.get("states").and_then(Value::as_array) {
                for state in states {
                    collect_state_object_name(state.as_object(), names);
                }
            }
        }
        Some("rotated_block_provider") => {
            collect_state_object_name(provider.get("state").and_then(Value::as_object), names);
        }
        Some("rule_based_state_provider") => {
            if let Some(rules) = provider.get("rules").and_then(Value::as_array) {
                for rule in rules {
                    collect_provider_block_names(rule.get("then"), names);
                }
            }
            collect_provider_block_names(provider.get("fallback"), names);
        }
        _ => {
            collect_state_object_name(provider.get("state").and_then(Value::as_object), names);
        }
    }
}

fn collect_state_object_name(
    state: Option<&serde_json::Map<String, Value>>,
    names: &mut HashSet<String>,
) {
    if let Some(name) = state
        .and_then(|state| state.get("Name"))
        .and_then(Value::as_str)
    {
        names.insert(normalize_feature_state_name(name));
    }
}

fn select_provider_block_id(
    provider: Option<&Value>,
    random: &mut DecorationRandom,
    origin: [i32; 3],
) -> Option<u32> {
    let selection = select_provider_state(provider, random, origin)?;
    Some(feature_state_runtime_id(&selection))
}

fn select_provider_state(
    provider: Option<&Value>,
    random: &mut DecorationRandom,
    origin: [i32; 3],
) -> Option<FeatureStateSelection> {
    let provider = provider?.as_object()?;
    match provider
        .get("type")
        .and_then(Value::as_str)
        .map(normalize_registry_name)
    {
        Some("simple_state_provider") => provider
            .get("state")
            .and_then(Value::as_object)
            .map(parse_feature_state_object),
        Some("weighted_state_provider") => {
            let entries = provider.get("entries").and_then(Value::as_array)?;
            let total_weight: i32 = entries
                .iter()
                .map(|entry| entry.get("weight").and_then(Value::as_i64).unwrap_or(0) as i32)
                .sum();
            if total_weight <= 0 {
                return None;
            }
            let mut choice = random.next_int(total_weight as u32) as i32;
            for entry in entries {
                choice -= entry.get("weight").and_then(Value::as_i64).unwrap_or(0) as i32;
                if choice < 0 {
                    return entry
                        .get("data")
                        .and_then(Value::as_object)
                        .map(parse_feature_state_object);
                }
            }
            None
        }
        Some("randomized_int_state_provider") => {
            let mut state = select_provider_state(provider.get("source"), random, origin)?;
            let property = provider.get("property").and_then(Value::as_str)?;
            let value = provider
                .get("values")
                .and_then(|value| sample_int_provider(value, random))
                .unwrap_or(0);
            state
                .properties
                .insert(property.to_owned(), value.to_string());
            normalize_feature_state(&mut state);
            Some(state)
        }
        Some("noise_threshold_provider") => {
            let noise = provider_noise_value(provider, origin);
            let threshold = provider
                .get("threshold")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let default_state = provider
                .get("default_state")
                .and_then(Value::as_object)
                .map(parse_feature_state_object)?;
            let pool_key = if noise < threshold {
                "low_states"
            } else if random.next_float()
                < provider
                    .get("high_chance")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0) as f32
            {
                "high_states"
            } else {
                return Some(default_state);
            };
            select_state_from_state_array(provider.get(pool_key), random).or(Some(default_state))
        }
        Some("noise_provider") => select_state_from_state_array(provider.get("states"), random),
        Some("dual_noise_provider") => {
            let Some(states) = provider.get("states").and_then(Value::as_array) else {
                return None;
            };
            if states.is_empty() {
                return None;
            }
            let slow_noise = provider_slow_noise_value(provider, origin);
            let variety = provider
                .get("variety")
                .and_then(Value::as_array)
                .and_then(|values| {
                    Some([
                        values.first()?.as_i64()? as i32,
                        values.get(1)?.as_i64()? as i32,
                    ])
                })
                .unwrap_or([1, 1]);
            let count = ((slow_noise.abs() * f64::from(variety[1] - variety[0] + 1)).floor() as i32
                + variety[0])
                .clamp(1, states.len() as i32) as usize;
            let noise = provider_noise_value(provider, origin);
            let index = ((noise.abs() * count as f64).floor() as usize).min(count - 1);
            states
                .get(index)
                .and_then(Value::as_object)
                .map(parse_feature_state_object)
        }
        Some("rotated_block_provider") => {
            let mut state = provider
                .get("state")
                .and_then(Value::as_object)
                .map(parse_feature_state_object)?;
            let axis = match random.next_int(3) {
                1 => "x",
                2 => "z",
                _ => "y",
            };
            state.properties.insert("axis".to_owned(), axis.to_owned());
            normalize_feature_state(&mut state);
            Some(state)
        }
        Some("rule_based_state_provider") => {
            if let Some(rules) = provider.get("rules").and_then(Value::as_array) {
                for rule in rules {
                    if let Some(selected) = select_provider_state(rule.get("then"), random, origin)
                    {
                        return Some(selected);
                    }
                }
            }
            select_provider_state(provider.get("fallback"), random, origin)
        }
        _ => provider
            .get("state")
            .and_then(Value::as_object)
            .map(parse_feature_state_object),
    }
}

fn select_state_from_state_array(
    states: Option<&Value>,
    random: &mut DecorationRandom,
) -> Option<FeatureStateSelection> {
    let states = states?.as_array()?;
    if states.is_empty() {
        return None;
    }
    let index = random.next_int(states.len() as u32) as usize;
    states
        .get(index)
        .and_then(Value::as_object)
        .map(parse_feature_state_object)
}

fn parse_feature_state_object(state: &serde_json::Map<String, Value>) -> FeatureStateSelection {
    let mut parsed = FeatureStateSelection {
        name: state
            .get("Name")
            .and_then(Value::as_str)
            .map(normalize_feature_state_name)
            .unwrap_or_else(|| "minecraft:air".to_owned()),
        properties: HashMap::new(),
    };
    if let Some(properties) = state.get("Properties").and_then(Value::as_object) {
        for (key, value) in properties {
            let value = value
                .as_str()
                .map(str::to_owned)
                .or_else(|| value.as_i64().map(|value| value.to_string()))
                .or_else(|| value.as_bool().map(|value| value.to_string()))
                .unwrap_or_else(|| value.to_string());
            parsed.properties.insert(key.to_owned(), value);
        }
    }
    normalize_feature_state(&mut parsed);
    parsed
}

fn normalize_feature_state(state: &mut FeatureStateSelection) {
    let normalized_name = normalize_feature_state_name(&state.name);
    state.name = normalized_name.clone();
    if normalized_name.ends_with("_log")
        || normalized_name.ends_with("_wood")
        || normalized_name.ends_with("_stem")
        || normalized_name.ends_with("_hyphae")
        || normalized_name == "minecraft:muddy_mangrove_roots"
        || normalized_name == "minecraft:basalt"
        || normalized_name == "minecraft:deepslate"
    {
        rename_feature_property(&mut state.properties, "axis", "pillar_axis");
    }
    if normalized_name.ends_with("_leaves")
        || matches!(
            normalize_registry_name(&normalized_name),
            "azalea_leaves" | "azalea_leaves_flowered"
        )
    {
        rename_feature_property(&mut state.properties, "persistent", "persistent_bit");
        state.properties.remove("distance");
        state.properties.remove("waterlogged");
        state
            .properties
            .entry("update_bit".to_owned())
            .or_insert_with(|| "false".to_owned());
    }
    if normalized_name == "minecraft:hanging_roots" {
        state.properties.remove("waterlogged");
    }
    if normalized_name == "minecraft:mangrove_propagule" {
        rename_feature_property(&mut state.properties, "age", "propagule_stage");
    }
}

fn normalize_feature_state_name(name: &str) -> String {
    match normalize_registry_name(name) {
        "lily_pad" => "minecraft:waterlily".to_owned(),
        "snow_block" => "minecraft:snow".to_owned(),
        "nether_quartz_ore" => "minecraft:quartz_ore".to_owned(),
        "flowering_azalea_leaves" => "minecraft:azalea_leaves_flowered".to_owned(),
        stripped => {
            if stripped == name {
                name.to_owned()
            } else {
                format!("minecraft:{stripped}")
            }
        }
    }
}

fn rename_feature_property(properties: &mut HashMap<String, String>, from: &str, to: &str) {
    let Some(value) = properties.remove(from) else {
        return;
    };
    properties.entry(to.to_owned()).or_insert(value);
}

fn feature_state_runtime_id(state: &FeatureStateSelection) -> u32 {
    let Some(block) = BLOCKS.iter().find(|block| block.string_id() == state.name) else {
        return blocks::get_block_id(&state.name);
    };
    let stripped = normalize_registry_name(&state.name);
    let runtime_id = if state.properties.contains_key("pillar_axis") {
        let axis_offset = match state.properties.get("pillar_axis").map(String::as_str) {
            Some("x") | Some("X") => 1,
            Some("z") | Some("Z") => 2,
            _ => 0,
        };
        block.min_state_id() + axis_offset
    } else if stripped.ends_with("_leaves")
        || matches!(stripped, "azalea_leaves" | "azalea_leaves_flowered")
    {
        let persistent = feature_prop_bool(&state.properties, "persistent_bit");
        let update = feature_prop_bool(&state.properties, "update_bit");
        block.min_state_id() + persistent as u32 + (update as u32) * 2
    } else if stripped == "vine" {
        block.min_state_id()
            + feature_prop_int(&state.properties, "vine_direction_bits").unwrap_or(0) as u32
    } else if matches!(stripped, "leaf_litter" | "wildflowers" | "pink_petals") {
        let growth = feature_prop_int(&state.properties, "growth")
            .or_else(|| {
                feature_prop_int(&state.properties, "segment_amount")
                    .map(|value| (value - 1).clamp(0, 7))
            })
            .or_else(|| {
                feature_prop_int(&state.properties, "flower_amount")
                    .map(|value| (value - 1).clamp(0, 7))
            })
            .unwrap_or(0) as u32;
        let cardinal_direction = feature_prop_str(&state.properties, "cardinal_direction")
            .or_else(|| feature_prop_str(&state.properties, "facing"))
            .map(cardinal_direction_state_value)
            .unwrap_or(0);
        block.min_state_id() + growth + cardinal_direction * 8
    } else if stripped == "tall_grass" {
        let upper = feature_prop_str(&state.properties, "half")
            .map(|half| normalize_registry_name(half) == "upper")
            .unwrap_or_else(|| feature_prop_bool(&state.properties, "upper_block_bit"));
        block.min_state_id() + upper as u32
    } else if stripped == "pale_moss_carpet" {
        let east = feature_prop_str(&state.properties, "pale_moss_carpet_side_east")
            .or_else(|| feature_prop_str(&state.properties, "east"))
            .map(pale_moss_carpet_side_value)
            .unwrap_or(0);
        let north = feature_prop_str(&state.properties, "pale_moss_carpet_side_north")
            .or_else(|| feature_prop_str(&state.properties, "north"))
            .map(pale_moss_carpet_side_value)
            .unwrap_or(0);
        let south = feature_prop_str(&state.properties, "pale_moss_carpet_side_south")
            .or_else(|| feature_prop_str(&state.properties, "south"))
            .map(pale_moss_carpet_side_value)
            .unwrap_or(0);
        let west = feature_prop_str(&state.properties, "pale_moss_carpet_side_west")
            .or_else(|| feature_prop_str(&state.properties, "west"))
            .map(pale_moss_carpet_side_value)
            .unwrap_or(0);
        let upper = feature_prop_bool(&state.properties, "upper_block_bit")
            || !feature_prop_bool(&state.properties, "bottom")
                && state.properties.contains_key("bottom");
        block.min_state_id() + east + north * 3 + south * 9 + west * 27 + upper as u32 * 81
    } else if matches!(stripped, "bee_nest" | "beehive") {
        let direction = feature_prop_int(&state.properties, "direction").unwrap_or(0);
        let honey = feature_prop_int(&state.properties, "honey_level").unwrap_or(0);
        block.min_state_id() + direction as u32 + honey as u32 * 4
    } else if stripped == "cocoa" {
        let age = feature_prop_int(&state.properties, "age").unwrap_or(0);
        let direction = feature_prop_int(&state.properties, "direction").unwrap_or(0);
        block.min_state_id() + age as u32 + direction as u32 * 3
    } else if stripped == "mangrove_propagule" {
        let hanging = feature_prop_bool(&state.properties, "hanging");
        let stage = feature_prop_int(&state.properties, "propagule_stage").unwrap_or(0);
        block.min_state_id() + hanging as u32 + stage as u32 * 2
    } else if stripped == "pale_hanging_moss" {
        block.min_state_id() + feature_prop_bool(&state.properties, "tip") as u32
    } else if stripped == "creaking_heart" {
        let state_offset = match state
            .properties
            .get("creaking_heart_state")
            .map(String::as_str)
            .unwrap_or("dormant")
        {
            "uprooted" => 0,
            "awake" => 2,
            _ => 1,
        };
        let natural = feature_prop_bool(&state.properties, "natural") as u32;
        let axis_offset = match state.properties.get("pillar_axis").map(String::as_str) {
            Some("x") | Some("X") => 1,
            Some("z") | Some("Z") => 2,
            _ => 0,
        };
        block.min_state_id() + state_offset + natural * 3 + axis_offset * 6
    } else {
        block.default_state_id()
    };
    runtime_id.min(block.max_state_id())
}

fn feature_prop_bool(properties: &HashMap<String, String>, key: &str) -> bool {
    properties
        .get(key)
        .map(|value| value == "true" || value == "1")
        .unwrap_or(false)
}

fn feature_prop_str<'a>(properties: &'a HashMap<String, String>, key: &str) -> Option<&'a str> {
    properties.get(key).map(String::as_str)
}

fn feature_prop_int(properties: &HashMap<String, String>, key: &str) -> Option<i32> {
    properties
        .get(key)
        .and_then(|value| value.parse::<i32>().ok())
}

fn cardinal_direction_state_value(direction: &str) -> u32 {
    match normalize_registry_name(direction) {
        "south" => 0,
        "west" => 1,
        "north" => 2,
        "east" => 3,
        _ => 0,
    }
}

fn pale_moss_carpet_side_value(side: &str) -> u32 {
    match normalize_registry_name(side) {
        "short" => 1,
        "tall" => 2,
        _ => 0,
    }
}

fn block_predicate_matches(
    context: &mut FeaturePlacementContext<'_>,
    predicate: &Value,
    origin: [i32; 3],
) -> bool {
    let Some(kind) = predicate.get("type").and_then(Value::as_str) else {
        return true;
    };

    match normalize_registry_name(kind) {
        "all_of" => predicate
            .get("predicates")
            .and_then(Value::as_array)
            .map(|predicates| {
                predicates
                    .iter()
                    .all(|predicate| block_predicate_matches(context, predicate, origin))
            })
            .unwrap_or(true),
        "any_of" => predicate
            .get("predicates")
            .and_then(Value::as_array)
            .map(|predicates| {
                predicates
                    .iter()
                    .any(|predicate| block_predicate_matches(context, predicate, origin))
            })
            .unwrap_or(false),
        "not" => predicate
            .get("predicate")
            .map(|predicate| !block_predicate_matches(context, predicate, origin))
            .unwrap_or(true),
        "matching_blocks" => {
            let target = offset_origin_by_predicate(origin, predicate);
            if !context.is_inside_world_bounds(target[1]) {
                return false;
            }
            let block = block_at(context, target);
            block_matches_blocks_value(block, predicate.get("blocks"))
        }
        "matching_block_tag" => {
            let target = offset_origin_by_predicate(origin, predicate);
            if !context.is_inside_world_bounds(target[1]) {
                return false;
            }
            let block = block_at(context, target);
            predicate
                .get("tag")
                .and_then(Value::as_str)
                .map(|tag| block_matches_tag(block, tag))
                .unwrap_or(false)
        }
        "matching_fluids" => {
            let target = offset_origin_by_predicate(origin, predicate);
            if !context.is_inside_world_bounds(target[1]) {
                return false;
            }
            let block = block_at(context, target);
            fluids_value_matches_block(block, predicate.get("fluids"))
        }
        "solid" => {
            let target = offset_origin_by_predicate(origin, predicate);
            context.is_inside_world_bounds(target[1])
                && is_solid_render_block(block_at(context, target))
        }
        "inside_world_bounds" => {
            let target = offset_origin_by_predicate(origin, predicate);
            context.is_inside_world_bounds(target[1])
        }
        "has_sturdy_face" => {
            let target = offset_origin_by_predicate(origin, predicate);
            context.is_inside_world_bounds(target[1])
                && predicate
                    .get("direction")
                    .and_then(Value::as_str)
                    .and_then(direction_offset_from_name)
                    .map(|direction| {
                        let support = offset_pos(target, direction);
                        context.is_inside_world_bounds(support[1])
                            && is_solid_render_block(block_at(context, support))
                    })
                    .unwrap_or(false)
        }
        "replaceable" => {
            let target = offset_origin_by_predicate(origin, predicate);
            context.is_inside_world_bounds(target[1])
                && is_replaceable_by_trees_block(block_at(context, target))
        }
        "would_survive" => {
            let state_name = predicate
                .get("state")
                .and_then(Value::as_object)
                .and_then(|state| state.get("Name"))
                .and_then(Value::as_str)
                .unwrap_or("minecraft:oak_sapling");
            would_survive_state(
                context,
                state_name,
                offset_origin_by_predicate(origin, predicate),
            )
        }
        _ => true,
    }
}

fn offset_origin_by_predicate(origin: [i32; 3], predicate: &Value) -> [i32; 3] {
    let offset = predicate
        .get("offset")
        .and_then(Value::as_array)
        .map(|offset| {
            [
                offset.first().and_then(Value::as_i64).unwrap_or(0) as i32,
                offset.get(1).and_then(Value::as_i64).unwrap_or(0) as i32,
                offset.get(2).and_then(Value::as_i64).unwrap_or(0) as i32,
            ]
        })
        .unwrap_or([0, 0, 0]);
    [
        origin[0] + offset[0],
        origin[1] + offset[1],
        origin[2] + offset[2],
    ]
}

fn would_survive_state(
    context: &mut FeaturePlacementContext<'_>,
    state_name: &str,
    origin: [i32; 3],
) -> bool {
    let state_name = normalize_registry_name(state_name);
    let current = context.get_block(origin[0], origin[1], origin[2]);
    if !(is_air_block(current)
        || is_replaceable_by_trees_block(current)
        || is_liquid_block(current))
    {
        return false;
    }

    let below = context.get_block(origin[0], origin[1] - 1, origin[2]);
    if state_name.ends_with("_sapling")
        || matches!(
            state_name,
            "mangrove_propagule" | "azalea" | "flowering_azalea"
        )
    {
        return is_tree_ground_block(below);
    }
    if state_name == "cactus" {
        return matches!(
            block_name(below),
            "minecraft:sand" | "minecraft:red_sand" | "minecraft:cactus"
        ) && [[1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1]]
            .into_iter()
            .all(|offset| is_air_block(block_at(context, offset_pos(origin, offset))));
    }
    if matches!(state_name, "seagrass" | "tall_seagrass" | "kelp") {
        return is_water_block(current) && (is_solid_render_block(below) || is_water_block(below));
    }

    true
}

fn sample_int_provider(value: &Value, random: &mut DecorationRandom) -> Option<i32> {
    if let Some(raw) = value.as_i64() {
        return Some(raw as i32);
    }

    let object = value.as_object()?;
    if object.get("type").is_none()
        && object.get("min_inclusive").is_some()
        && object.get("max_inclusive").is_some()
    {
        let min = object
            .get("min_inclusive")
            .and_then(|entry| sample_int_provider(entry, random))
            .unwrap_or(0);
        let max = object
            .get("max_inclusive")
            .and_then(|entry| sample_int_provider(entry, random))
            .unwrap_or(min);
        let span = (max - min + 1).max(1) as u32;
        return Some(min + random.next_int(span) as i32);
    }
    match object
        .get("type")
        .and_then(Value::as_str)
        .map(normalize_registry_name)
    {
        Some("uniform") => {
            let min = object
                .get("min_inclusive")
                .and_then(|entry| sample_int_provider(entry, random))
                .unwrap_or(0);
            let max = object
                .get("max_inclusive")
                .and_then(|entry| sample_int_provider(entry, random))
                .unwrap_or(min);
            let span = (max - min + 1).max(1) as u32;
            Some(min + random.next_int(span) as i32)
        }
        Some("clamped") => {
            let source = object
                .get("source")
                .and_then(|entry| sample_int_provider(entry, random))?;
            let min = object
                .get("min_inclusive")
                .and_then(|entry| sample_int_provider(entry, random))
                .unwrap_or(source);
            let max = object
                .get("max_inclusive")
                .and_then(|entry| sample_int_provider(entry, random))
                .unwrap_or(source);
            Some(source.clamp(min, max))
        }
        Some("biased_to_bottom") => {
            let min = object
                .get("min_inclusive")
                .and_then(|entry| sample_int_provider(entry, random))
                .unwrap_or(0);
            let max = object
                .get("max_inclusive")
                .and_then(|entry| sample_int_provider(entry, random))
                .unwrap_or(min);
            let inner = random.next_int((max - min + 1).max(1) as u32) as i32 + min;
            Some(min + random.next_int((inner - min + 1).max(1) as u32) as i32)
        }
        Some("weighted_list") => {
            let distribution = object.get("distribution").and_then(Value::as_array)?;
            let total_weight: i32 = distribution
                .iter()
                .map(|entry| entry.get("weight").and_then(Value::as_i64).unwrap_or(0) as i32)
                .sum();
            if total_weight <= 0 {
                return None;
            }
            let mut choice = random.next_int(total_weight as u32) as i32;
            for entry in distribution {
                choice -= entry.get("weight").and_then(Value::as_i64).unwrap_or(0) as i32;
                if choice < 0 {
                    return entry
                        .get("data")
                        .and_then(|entry| sample_int_provider(entry, random));
                }
            }
            None
        }
        _ => object
            .get("value")
            .and_then(|entry| sample_int_provider(entry, random)),
    }
}

fn build_runtime_block_names() -> Vec<&'static str> {
    let max_runtime_id = BLOCKS
        .iter()
        .map(|block| block.max_state_id())
        .max()
        .unwrap_or(0) as usize;
    let mut names = vec!["minecraft:air"; max_runtime_id + 1];
    for block in BLOCKS.iter() {
        for runtime_id in block.min_state_id()..=block.max_state_id() {
            names[runtime_id as usize] = block.string_id();
        }
    }
    names
}

fn block_name(block_id: u32) -> &'static str {
    RUNTIME_BLOCK_NAMES
        .get(block_id as usize)
        .copied()
        .unwrap_or("minecraft:air")
}

fn block_matches_blocks_value(block_id: u32, value: Option<&Value>) -> bool {
    match value {
        Some(Value::String(name)) => normalize_feature_state_name(name) == block_name(block_id),
        Some(Value::Array(entries)) => entries
            .iter()
            .filter_map(Value::as_str)
            .map(normalize_feature_state_name)
            .any(|name| name == block_name(block_id)),
        _ => false,
    }
}

fn block_matches_tag(block_id: u32, tag: &str) -> bool {
    let tag = tag.strip_prefix('#').unwrap_or(tag);
    match normalize_registry_name(tag) {
        "air" => is_air_block(block_id),
        "features_cannot_replace" | "geode_invalid_blocks" => matches!(
            block_name(block_id),
            "minecraft:bedrock"
                | "minecraft:water"
                | "minecraft:lava"
                | "minecraft:obsidian"
                | "minecraft:crying_obsidian"
                | "minecraft:end_portal"
                | "minecraft:end_portal_frame"
                | "minecraft:spawner"
        ),
        "stone_ore_replaceables" => matches!(
            block_name(block_id),
            "minecraft:stone"
                | "minecraft:granite"
                | "minecraft:diorite"
                | "minecraft:andesite"
                | "minecraft:tuff"
        ),
        "deepslate_ore_replaceables" => matches!(
            block_name(block_id),
            "minecraft:deepslate" | "minecraft:tuff"
        ),
        "moss_replaceable" => matches!(
            block_name(block_id),
            "minecraft:stone"
                | "minecraft:granite"
                | "minecraft:diorite"
                | "minecraft:andesite"
                | "minecraft:deepslate"
                | "minecraft:tuff"
                | "minecraft:dirt"
                | "minecraft:grass_block"
                | "minecraft:coarse_dirt"
                | "minecraft:podzol"
                | "minecraft:rooted_dirt"
                | "minecraft:clay"
        ),
        "azalea_root_replaceable" => matches!(
            block_name(block_id),
            "minecraft:stone"
                | "minecraft:granite"
                | "minecraft:diorite"
                | "minecraft:andesite"
                | "minecraft:deepslate"
                | "minecraft:tuff"
                | "minecraft:calcite"
                | "minecraft:dirt"
                | "minecraft:grass_block"
                | "minecraft:coarse_dirt"
                | "minecraft:moss_block"
                | "minecraft:clay"
        ),
        _ => false,
    }
}

fn valid_blocks_match_block(valid_blocks: Option<&Value>, block_id: u32) -> bool {
    match valid_blocks {
        Some(Value::String(entry)) if entry.starts_with('#') => block_matches_tag(block_id, entry),
        Some(value) => block_matches_blocks_value(block_id, Some(value)),
        None => false,
    }
}

fn fluids_value_matches_block(block_id: u32, value: Option<&Value>) -> bool {
    match value {
        Some(Value::String(name)) => fluid_name_matches_block(block_id, name),
        Some(Value::Array(entries)) => entries
            .iter()
            .filter_map(Value::as_str)
            .any(|entry| fluid_name_matches_block(block_id, entry)),
        _ => false,
    }
}

fn fluid_name_matches_block(block_id: u32, fluid: &str) -> bool {
    match normalize_registry_name(fluid) {
        "water" => is_water_block(block_id),
        "lava" => block_name(block_id) == "minecraft:lava",
        _ => false,
    }
}

fn sample_height_provider(
    value: &Value,
    context: &FeaturePlacementContext<'_>,
    random: &mut DecorationRandom,
) -> Option<i32> {
    let object = value.as_object()?;
    match object
        .get("type")
        .and_then(Value::as_str)
        .map(normalize_registry_name)
    {
        Some("uniform") => {
            let min = object
                .get("min_inclusive")
                .and_then(|entry| resolve_vertical_anchor(entry, context))
                .unwrap_or(MIN_Y);
            let max = object
                .get("max_inclusive")
                .and_then(|entry| resolve_vertical_anchor(entry, context))
                .unwrap_or(min);
            let span = (max - min + 1).max(1) as u32;
            Some(min + random.next_int(span) as i32)
        }
        Some("trapezoid") => {
            let min = object
                .get("min_inclusive")
                .and_then(|entry| resolve_vertical_anchor(entry, context))
                .unwrap_or(MIN_Y);
            let max = object
                .get("max_inclusive")
                .and_then(|entry| resolve_vertical_anchor(entry, context))
                .unwrap_or(min);
            let plateau = object.get("plateau").and_then(Value::as_i64).unwrap_or(0) as i32;
            sample_trapezoid_height(min, max, plateau, random)
        }
        _ => None,
    }
}

fn resolve_vertical_anchor(value: &Value, context: &FeaturePlacementContext<'_>) -> Option<i32> {
    let object = value.as_object()?;
    if let Some(y) = object.get("absolute").and_then(Value::as_i64) {
        return Some(y as i32);
    }
    if let Some(y) = object.get("above_bottom").and_then(Value::as_i64) {
        return Some(MIN_Y + y as i32);
    }
    if let Some(y) = object.get("below_top").and_then(Value::as_i64) {
        return Some(context.max_y() - y as i32);
    }
    None
}

fn sample_trapezoid_height(
    min: i32,
    max: i32,
    plateau: i32,
    random: &mut DecorationRandom,
) -> Option<i32> {
    if min > max {
        return None;
    }
    if plateau <= 0 || min == max {
        let span = (max - min + 1).max(1) as u32;
        return Some(min + random.next_int(span) as i32);
    }
    let inner_min = min + ((max - min - plateau).max(0) / 2);
    let inner_max = max - ((max - min - plateau).max(0) / 2);
    let a = min + random.next_int((inner_max - min + 1).max(1) as u32) as i32;
    let b = min + random.next_int((inner_max - min + 1).max(1) as u32) as i32;
    Some(
        ((a + b) / 2)
            .clamp(min, max)
            .clamp(inner_min.min(max), inner_max.max(min)),
    )
}

fn scan_environment_position(
    context: &mut FeaturePlacementContext<'_>,
    modifier: &Value,
    origin: [i32; 3],
) -> Option<[i32; 3]> {
    let direction = modifier
        .get("direction_of_search")
        .and_then(Value::as_str)
        .and_then(direction_offset_from_name)?;
    let max_steps = modifier
        .get("max_steps")
        .and_then(Value::as_i64)
        .unwrap_or(1)
        .max(1) as usize;
    let allowed = modifier.get("allowed_search_condition");
    let target = modifier.get("target_condition")?;
    let mut cursor = origin;
    if !allowed
        .map(|predicate| block_predicate_matches(context, predicate, cursor))
        .unwrap_or(true)
    {
        return None;
    }
    for _ in 0..max_steps {
        if block_predicate_matches(context, target, cursor) {
            return Some(cursor);
        }
        cursor = offset_pos(cursor, direction);
        if !context.is_inside_world_bounds(cursor[1]) {
            return None;
        }
        if !allowed
            .map(|predicate| block_predicate_matches(context, predicate, cursor))
            .unwrap_or(true)
        {
            break;
        }
    }
    block_predicate_matches(context, target, cursor).then_some(cursor)
}

fn find_positions_on_every_layer(
    context: &mut FeaturePlacementContext<'_>,
    origin: [i32; 3],
    count: usize,
    random: &mut DecorationRandom,
) -> Vec<[i32; 3]> {
    let mut positions = Vec::new();
    let mut layer = 0;
    loop {
        let mut found_any = false;
        for _ in 0..count {
            let x = origin[0] + random.next_int(16) as i32;
            let z = origin[2] + random.next_int(16) as i32;
            let start_y = context.height_at(HeightmapKind::MotionBlocking, x, z);
            if let Some(y) = find_on_ground_y_position(context, x, start_y, z, layer) {
                positions.push([x, y, z]);
                found_any = true;
            }
        }
        if !found_any {
            break;
        }
        layer += 1;
    }
    positions
}

fn find_on_ground_y_position(
    context: &mut FeaturePlacementContext<'_>,
    block_x: i32,
    start_y: i32,
    block_z: i32,
    layer_to_place_on: usize,
) -> Option<i32> {
    let mut current_layer = 0usize;
    let mut current = context.get_block(block_x, start_y, block_z);
    for y in ((MIN_Y + 1)..=start_y).rev() {
        let below = context.get_block(block_x, y - 1, block_z);
        if is_empty_layer_block(current)
            && !is_empty_layer_block(below)
            && block_name(below) != "minecraft:bedrock"
        {
            if current_layer == layer_to_place_on {
                return Some(y);
            }
            current_layer += 1;
        }
        current = below;
    }
    None
}

fn is_empty_layer_block(block_id: u32) -> bool {
    is_air_block(block_id) || is_liquid_block(block_id)
}

fn biome_info_noise(context: &FeaturePlacementContext<'_>, origin: [i32; 3], factor: f64) -> f64 {
    sample_value_noise_2d(
        context.level_seed ^ 2345,
        origin[0] as f64 / factor.max(0.000001),
        origin[2] as f64 / factor.max(0.000001),
    )
}

fn provider_noise_value(provider: &serde_json::Map<String, Value>, origin: [i32; 3]) -> f64 {
    let seed = provider.get("seed").and_then(Value::as_i64).unwrap_or(0);
    let scale = provider.get("scale").and_then(Value::as_f64).unwrap_or(1.0);
    sample_value_noise_2d(
        seed,
        origin[0] as f64 * scale.max(0.000001),
        origin[2] as f64 * scale.max(0.000001),
    )
}

fn provider_slow_noise_value(provider: &serde_json::Map<String, Value>, origin: [i32; 3]) -> f64 {
    let seed = provider.get("seed").and_then(Value::as_i64).unwrap_or(0) ^ 0x5bf0_3635;
    let scale = provider
        .get("slow_scale")
        .and_then(Value::as_f64)
        .unwrap_or(1.0);
    sample_value_noise_2d(
        seed,
        origin[0] as f64 * scale.max(0.000001),
        origin[2] as f64 * scale.max(0.000001),
    )
}

fn sample_value_noise_2d(seed: i64, x: f64, z: f64) -> f64 {
    let x0 = x.floor();
    let z0 = z.floor();
    let xf = x - x0;
    let zf = z - z0;
    let v00 = hashed_noise(seed, x0 as i64, z0 as i64);
    let v10 = hashed_noise(seed, x0 as i64 + 1, z0 as i64);
    let v01 = hashed_noise(seed, x0 as i64, z0 as i64 + 1);
    let v11 = hashed_noise(seed, x0 as i64 + 1, z0 as i64 + 1);
    let sx = smoothstep(xf);
    let sz = smoothstep(zf);
    let ix0 = lerp_f64(v00, v10, sx);
    let ix1 = lerp_f64(v01, v11, sx);
    lerp_f64(ix0, ix1, sz)
}

fn hashed_noise(seed: i64, x: i64, z: i64) -> f64 {
    let mut value = seed
        .wrapping_add(x.wrapping_mul(341_873_128_712))
        .wrapping_add(z.wrapping_mul(132_897_987_541));
    value ^= value >> 33;
    value = value.wrapping_mul(0xff51afd7ed558ccd_u64 as i64);
    value ^= value >> 33;
    value = value.wrapping_mul(0xc4ceb9fe1a85ec53_u64 as i64);
    value ^= value >> 33;
    (value as f64 / i64::MAX as f64).clamp(-1.0, 1.0)
}

fn smoothstep(value: f64) -> f64 {
    value * value * (3.0 - 2.0 * value)
}

fn lerp_f64(a: f64, b: f64, delta: f64) -> f64 {
    a + (b - a) * delta
}

fn heightmap_includes_block(kind: HeightmapKind, block_id: u32) -> bool {
    if is_air_block(block_id) {
        return false;
    }
    match kind {
        HeightmapKind::WorldSurface => true,
        HeightmapKind::OceanFloor => !is_liquid_block(block_id),
        HeightmapKind::MotionBlocking => !is_liquid_block(block_id),
        HeightmapKind::MotionBlockingNoLeaves => {
            !is_liquid_block(block_id) && !is_leaves_block(block_id)
        }
    }
}

fn is_air_block(block_id: u32) -> bool {
    matches!(
        block_name(block_id),
        "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
    )
}

fn is_liquid_block(block_id: u32) -> bool {
    matches!(block_name(block_id), "minecraft:water" | "minecraft:lava")
}

fn is_water_block(block_id: u32) -> bool {
    block_name(block_id) == "minecraft:water"
}

fn is_leaves_block(block_id: u32) -> bool {
    block_name(block_id).contains("leaves")
        || matches!(
            block_name(block_id),
            "minecraft:azalea_leaves" | "minecraft:flowering_azalea_leaves"
        )
}

fn is_vine_block(block_id: u32) -> bool {
    block_name(block_id) == "minecraft:vine"
}

fn is_log_block(block_id: u32) -> bool {
    let name = block_name(block_id);
    name.contains("_log")
        || name.contains("_wood")
        || name.contains("_stem")
        || name.contains("hyphae")
        || name == "minecraft:mangrove_roots"
        || name == "minecraft:muddy_mangrove_roots"
}

fn is_replaceable_by_trees_block(block_id: u32) -> bool {
    let name = block_name(block_id);
    name.contains("grass")
        || name.contains("flower")
        || name.contains("sapling")
        || name.contains("mushroom")
        || name.contains("fern")
        || matches!(
            name,
            "minecraft:dead_bush"
                | "minecraft:vine"
                | "minecraft:snow"
                | "minecraft:water"
                | "minecraft:waterlily"
                | "minecraft:lily_pad"
                | "minecraft:seagrass"
                | "minecraft:tall_seagrass"
                | "minecraft:hanging_roots"
                | "minecraft:glow_lichen"
                | "minecraft:pale_moss_carpet"
                | "minecraft:small_dripleaf"
                | "minecraft:big_dripleaf"
                | "minecraft:bush"
                | "minecraft:firefly_bush"
                | "minecraft:warped_roots"
                | "minecraft:nether_sprouts"
                | "minecraft:crimson_roots"
                | "minecraft:leaf_litter"
                | "minecraft:pitcher_plant"
                | "minecraft:pink_petals"
                | "minecraft:wildflowers"
                | "minecraft:short_dry_grass"
                | "minecraft:tall_dry_grass"
        )
}

fn is_solid_render_block(block_id: u32) -> bool {
    !is_air_block(block_id)
        && !is_liquid_block(block_id)
        && !is_leaves_block(block_id)
        && !is_vine_block(block_id)
        && !is_replaceable_by_trees_block(block_id)
        && !matches!(block_name(block_id), "minecraft:pale_hanging_moss")
}

fn is_tree_ground_block(block_id: u32) -> bool {
    matches!(
        block_name(block_id),
        "minecraft:dirt"
            | "minecraft:grass_block"
            | "minecraft:coarse_dirt"
            | "minecraft:podzol"
            | "minecraft:mycelium"
            | "minecraft:rooted_dirt"
            | "minecraft:mud"
            | "minecraft:clay"
            | "minecraft:moss_block"
            | "minecraft:sand"
            | "minecraft:red_sand"
    )
}

fn is_pale_moss_replaceable_block(block_id: u32) -> bool {
    is_tree_ground_block(block_id)
        || matches!(
            block_name(block_id),
            "minecraft:stone"
                | "minecraft:granite"
                | "minecraft:diorite"
                | "minecraft:andesite"
                | "minecraft:deepslate"
                | "minecraft:tuff"
                | "minecraft:calcite"
                | "minecraft:moss_block"
                | "minecraft:pale_moss_block"
        )
}

fn is_replaceable_below_trunk(block_id: u32) -> bool {
    is_tree_ground_block(block_id)
        || is_replaceable_by_trees_block(block_id)
        || is_air_block(block_id)
}

fn normalize_registry_name(name: &str) -> &str {
    name.strip_prefix("minecraft:").unwrap_or(name)
}

#[derive(Debug, Clone)]
struct DecorationRandom {
    rng: Xoroshiro128,
}

impl DecorationRandom {
    fn new(seed: i64) -> Self {
        Self {
            rng: Xoroshiro128::from_seed(seed),
        }
    }

    fn set_seed(&mut self, seed: i64) {
        self.rng = Xoroshiro128::from_seed(seed);
    }

    fn next_long(&mut self) -> i64 {
        self.rng.next_long() as i64
    }

    fn next_int(&mut self, bound: u32) -> u32 {
        self.rng.next_int(bound.max(1))
    }

    fn next_float(&mut self) -> f32 {
        self.rng.next_float()
    }

    fn next_bool(&mut self) -> bool {
        self.next_int(2) == 0
    }

    fn set_decoration_seed(&mut self, seed: i64, block_x: i32, block_z: i32) -> i64 {
        self.set_seed(seed);
        let x_scale = self.next_long() | 1;
        let z_scale = self.next_long() | 1;
        let result =
            (block_x as i64).wrapping_mul(x_scale) + (block_z as i64).wrapping_mul(z_scale) ^ seed;
        self.set_seed(result);
        result
    }

    fn set_feature_seed(&mut self, seed: i64, index: i32, step: i32) {
        self.set_seed(seed + index as i64 + 10000 * step as i64);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feature_order_includes_plains_tree_features() {
        let plains = BiomeFeatures::from_name("plains").expect("missing plains biome features");
        let step_index = GenerationStep::VegetalDecoration as usize;
        let order = &STEP_FEATURE_ORDER[step_index];

        for feature_name in plains.get_features(GenerationStep::VegetalDecoration) {
            assert!(order.index_by_name.contains_key(*feature_name));
        }
    }

    #[test]
    fn resolve_placed_feature_finds_configured_feature() {
        let (_, configured_feature) =
            resolve_placed_feature("trees_plains").expect("missing trees_plains");
        assert_eq!(configured_feature, "trees_plains");
    }

    #[test]
    fn parses_heightmap_wg_aliases() {
        assert!(matches!(
            HeightmapKind::from_name("WORLD_SURFACE_WG"),
            Some(HeightmapKind::WorldSurface)
        ));
        assert!(matches!(
            HeightmapKind::from_name("OCEAN_FLOOR_WG"),
            Some(HeightmapKind::OceanFloor)
        ));
    }

    #[test]
    fn reads_legacy_tree_below_trunk_provider_fields() {
        let oak = FeatureRuntimeRegistry::global()
            .configured_feature("oak")
            .expect("missing oak configured feature");
        assert_eq!(
            state_provider_block_name(oak.config.get("dirt_provider")),
            Some("minecraft:dirt")
        );
    }

    #[test]
    fn sample_int_provider_supports_uniform_without_explicit_type() {
        let cherry = FeatureRuntimeRegistry::global()
            .configured_feature("cherry")
            .expect("missing cherry configured feature");
        let provider = cherry
            .config
            .get("trunk_placer")
            .and_then(Value::as_object)
            .and_then(|placer| placer.get("branch_start_offset_from_top"))
            .expect("missing cherry branch start provider");
        let mut random = DecorationRandom::new(12345);
        for _ in 0..16 {
            let sampled =
                sample_int_provider(provider, &mut random).expect("provider should sample");
            assert!((-4..=-3).contains(&sampled));
        }
    }

    #[test]
    fn supports_all_vanilla_tree_placer_pairs() {
        let supported_pairs = HashSet::from([
            ("straight_trunk_placer", "blob_foliage_placer"),
            ("straight_trunk_placer", "pine_foliage_placer"),
            ("straight_trunk_placer", "spruce_foliage_placer"),
            ("straight_trunk_placer", "bush_foliage_placer"),
            ("dark_oak_trunk_placer", "dark_oak_foliage_placer"),
            ("forking_trunk_placer", "acacia_foliage_placer"),
            ("fancy_trunk_placer", "fancy_foliage_placer"),
            ("cherry_trunk_placer", "cherry_foliage_placer"),
            ("bending_trunk_placer", "random_spread_foliage_placer"),
            (
                "upwards_branching_trunk_placer",
                "random_spread_foliage_placer",
            ),
            ("giant_trunk_placer", "mega_pine_foliage_placer"),
            ("mega_jungle_trunk_placer", "jungle_foliage_placer"),
        ]);

        for feature_name in unastar_noise::CONFIGURED_FEATURE_NAMES {
            let Some(feature) = FeatureRuntimeRegistry::global().configured_feature(feature_name)
            else {
                continue;
            };
            if normalize_registry_name(&feature.kind) != "tree" {
                continue;
            }
            let trunk = feature
                .config
                .get("trunk_placer")
                .and_then(Value::as_object)
                .and_then(|placer| placer.get("type"))
                .and_then(Value::as_str)
                .map(normalize_registry_name)
                .expect("missing trunk placer");
            let foliage = feature
                .config
                .get("foliage_placer")
                .and_then(Value::as_object)
                .and_then(|placer| placer.get("type"))
                .and_then(Value::as_str)
                .map(normalize_registry_name)
                .expect("missing foliage placer");
            assert!(
                supported_pairs.contains(&(trunk, foliage)),
                "unsupported tree pair in {feature_name}: {trunk}/{foliage}"
            );
        }
    }

    #[test]
    fn supports_all_vanilla_tree_decorator_types() {
        let supported = HashSet::from([
            "alter_ground",
            "attached_to_leaves",
            "beehive",
            "cocoa",
            "creaking_heart",
            "leave_vine",
            "pale_moss",
            "place_on_ground",
            "trunk_vine",
        ]);

        for feature_name in unastar_noise::CONFIGURED_FEATURE_NAMES {
            let Some(feature) = FeatureRuntimeRegistry::global().configured_feature(feature_name)
            else {
                continue;
            };
            if normalize_registry_name(&feature.kind) != "tree" {
                continue;
            }
            let decorators = feature
                .config
                .get("decorators")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for decorator in decorators {
                let kind = decorator
                    .get("type")
                    .and_then(Value::as_str)
                    .map(normalize_registry_name)
                    .expect("decorator type");
                assert!(
                    supported.contains(kind),
                    "unsupported tree decorator in {feature_name}: {kind}"
                );
            }
        }
    }

    #[test]
    fn feature_state_runtime_id_supports_leaf_litter_segments() {
        let north_one = feature_state_block_id(
            "minecraft:leaf_litter",
            &[("facing", "north"), ("segment_amount", "1")],
        );
        let north_four = feature_state_block_id(
            "minecraft:leaf_litter",
            &[("facing", "north"), ("segment_amount", "4")],
        );
        let east_one = feature_state_block_id(
            "minecraft:leaf_litter",
            &[("facing", "east"), ("segment_amount", "1")],
        );

        assert_ne!(north_one, north_four);
        assert_ne!(north_one, east_one);

        let block = BLOCKS
            .iter()
            .find(|block| block.string_id() == "minecraft:leaf_litter")
            .expect("missing leaf litter block");
        assert!((block.min_state_id()..=block.max_state_id()).contains(&north_one));
        assert!((block.min_state_id()..=block.max_state_id()).contains(&north_four));
        assert!((block.min_state_id()..=block.max_state_id()).contains(&east_one));
    }

    #[test]
    fn feature_state_runtime_id_supports_pale_moss_and_tall_grass_states() {
        let carpet = feature_state_block_id(
            "minecraft:pale_moss_carpet",
            &[
                ("bottom", "true"),
                ("east", "none"),
                ("north", "none"),
                ("south", "none"),
                ("west", "none"),
            ],
        );
        let tall_grass_lower = feature_state_block_id("minecraft:tall_grass", &[("half", "lower")]);
        let tall_grass_upper = feature_state_block_id("minecraft:tall_grass", &[("half", "upper")]);

        assert_ne!(tall_grass_lower, tall_grass_upper);

        let carpet_block = BLOCKS
            .iter()
            .find(|block| block.string_id() == "minecraft:pale_moss_carpet")
            .expect("missing pale moss carpet block");
        assert!((carpet_block.min_state_id()..=carpet_block.max_state_id()).contains(&carpet));

        let tall_grass_block = BLOCKS
            .iter()
            .find(|block| block.string_id() == "minecraft:tall_grass")
            .expect("missing tall grass block");
        assert!(
            (tall_grass_block.min_state_id()..=tall_grass_block.max_state_id())
                .contains(&tall_grass_lower)
        );
        assert!(
            (tall_grass_block.min_state_id()..=tall_grass_block.max_state_id())
                .contains(&tall_grass_upper)
        );
    }
}
