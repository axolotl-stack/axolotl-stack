//! Structure positioning algorithms based on cubiomes.
//!
//! This module handles finding structure generation positions using
//! vanilla-accurate algorithms ported from cubiomes.
#![allow(dead_code)]

use super::BiomeNoise;
use super::constants::Biome;
use super::structure_registry::StructureRuntimeRegistry;
use super::xoroshiro::JavaRandom;
use serde::Deserialize;
use std::f64::consts::PI;
use std::sync::LazyLock;

/// Structure types that can be generated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureType {
    Village,
    DesertPyramid,
    JungleTemple,
    SwampHut,
    Igloo,
    PillagerOutpost,
    RuinedPortal,
    OceanRuin,
    Shipwreck,
}

/// Configuration for structure generation.
#[derive(Debug, Clone, PartialEq)]
pub struct StructureConfig {
    /// Salt value for the structure type
    pub salt: i64,
    /// Region size in chunks (structure attempts per region)
    pub region_size: i32,
    /// Range within region where structure can generate
    pub chunk_range: i32,
    /// Spread algorithm used to pick the chunk within a region
    pub spread_type: RandomSpreadType,
    /// Chance a candidate placement survives the frequency reducer
    pub frequency: f32,
    /// Java placement-specific frequency reducer
    pub frequency_reduction_method: FrequencyReductionMethod,
    /// Optional exclusion zone against another structure set
    pub exclusion_zone: Option<ExclusionZone>,
}

impl StructureConfig {
    /// Get the configuration for a specific structure type.
    pub fn get(structure: StructureType) -> Self {
        RANDOM_SPREAD_STRUCTURE_CONFIGS[structure.as_index()].clone()
    }

    fn apply_additional_chunk_restrictions(
        &self,
        level_seed: i64,
        source_x: i32,
        source_z: i32,
    ) -> bool {
        if self.frequency >= 1.0 {
            return true;
        }
        self.frequency_reduction_method.should_generate(
            level_seed,
            self.salt,
            source_x,
            source_z,
            self.frequency,
        )
    }

    fn apply_interactions_with_other_structures(
        &self,
        level_seed: i64,
        source_x: i32,
        source_z: i32,
    ) -> bool {
        match &self.exclusion_zone {
            Some(zone) => !has_structure_chunk_in_range(
                &zone.other_set,
                level_seed,
                source_x,
                source_z,
                zone.chunk_count,
            ),
            None => true,
        }
    }

    fn is_structure_chunk(&self, level_seed: i64, source_x: i32, source_z: i32) -> bool {
        let reg_x = floor_div(source_x, self.region_size);
        let reg_z = floor_div(source_z, self.region_size);
        let pos = get_structure_pos(self, level_seed, reg_x, reg_z);
        pos.chunk_x == source_x
            && pos.chunk_z == source_z
            && self.apply_additional_chunk_restrictions(level_seed, source_x, source_z)
            && self.apply_interactions_with_other_structures(level_seed, source_x, source_z)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RandomSpreadType {
    Linear,
    Triangular,
}

impl Default for RandomSpreadType {
    fn default() -> Self {
        Self::Linear
    }
}

impl RandomSpreadType {
    fn sample(self, random: &mut JavaRandom, limit: u32) -> i32 {
        match self {
            Self::Linear => random.next_int(limit),
            Self::Triangular => (random.next_int(limit) + random.next_int(limit)) / 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrequencyReductionMethod {
    Default,
    #[serde(rename = "legacy_type_1", alias = "legacy_type1")]
    LegacyType1,
    #[serde(rename = "legacy_type_2", alias = "legacy_type2")]
    LegacyType2,
    #[serde(rename = "legacy_type_3", alias = "legacy_type3")]
    LegacyType3,
}

impl Default for FrequencyReductionMethod {
    fn default() -> Self {
        Self::Default
    }
}

impl FrequencyReductionMethod {
    fn should_generate(
        self,
        level_seed: i64,
        salt: i64,
        source_x: i32,
        source_z: i32,
        probability: f32,
    ) -> bool {
        if probability <= 0.0 {
            return false;
        }

        match self {
            Self::Default => {
                let mut random = JavaRandom::from_seed(large_feature_with_salt_seed(
                    level_seed, source_x, source_z, salt,
                ));
                random.next_float() < probability
            }
            Self::LegacyType1 => {
                let cx = source_x >> 4;
                let cz = source_z >> 4;
                let mut random =
                    JavaRandom::from_seed((cx as i64) ^ ((cz as i64) << 4) ^ level_seed);
                random.next_i32();
                let bound = (1.0 / probability) as u32;
                random.next_int(bound) == 0
            }
            Self::LegacyType2 => {
                let mut random = JavaRandom::from_seed(large_feature_with_salt_seed(
                    level_seed, source_x, source_z, 10387320,
                ));
                random.next_float() < probability
            }
            Self::LegacyType3 => {
                let mut random = JavaRandom::from_seed(level_seed);
                set_large_feature_seed(&mut random, level_seed, source_x, source_z);
                random.next_double() < f64::from(probability)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExclusionZone {
    pub other_set: String,
    pub chunk_count: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructureSelectionEntry {
    pub structure: String,
    pub weight: i32,
}

#[derive(Debug, Clone, Deserialize)]
struct RandomSpreadPlacementJson {
    #[serde(rename = "type")]
    kind: String,
    salt: i64,
    spacing: i32,
    separation: i32,
    #[serde(default)]
    spread_type: RandomSpreadType,
    #[serde(default = "default_structure_frequency")]
    frequency: f32,
    #[serde(default)]
    frequency_reduction_method: FrequencyReductionMethod,
    #[serde(default)]
    exclusion_zone: Option<ExclusionZoneJson>,
}

#[derive(Debug, Clone, Deserialize)]
struct ExclusionZoneJson {
    other_set: String,
    chunk_count: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StrongholdConfig {
    count: i32,
    distance: i32,
    spread: i32,
    preferred_biomes: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ConcentricRingsPlacementJson {
    #[serde(rename = "type")]
    kind: String,
    count: i32,
    distance: i32,
    spread: i32,
    preferred_biomes: String,
}

fn default_structure_frequency() -> f32 {
    1.0
}

fn normalize_registry_name(name: &str) -> &str {
    name.strip_prefix("minecraft:").unwrap_or(name)
}

fn floor_div(value: i32, divisor: i32) -> i32 {
    value.div_euclid(divisor)
}

fn large_feature_with_salt_seed(seed: i64, x: i32, z: i32, salt: i64) -> i64 {
    (x as i64)
        .wrapping_mul(341873128712)
        .wrapping_add((z as i64).wrapping_mul(132897987541))
        .wrapping_add(seed)
        .wrapping_add(salt)
}

fn set_large_feature_seed(random: &mut JavaRandom, seed: i64, chunk_x: i32, chunk_z: i32) {
    random.set_seed(seed);
    let x_scale = random.next_long();
    let z_scale = random.next_long();
    let result =
        (chunk_x as i64).wrapping_mul(x_scale) ^ (chunk_z as i64).wrapping_mul(z_scale) ^ seed;
    random.set_seed(result);
}

fn random_spread_config_for_structure_set(structure_set_name: &str) -> StructureConfig {
    match normalize_registry_name(structure_set_name) {
        "villages" => StructureConfig::get(StructureType::Village),
        "desert_pyramids" => StructureConfig::get(StructureType::DesertPyramid),
        "jungle_temples" => StructureConfig::get(StructureType::JungleTemple),
        "swamp_huts" => StructureConfig::get(StructureType::SwampHut),
        "igloos" => StructureConfig::get(StructureType::Igloo),
        "pillager_outposts" => StructureConfig::get(StructureType::PillagerOutpost),
        "ruined_portals" => StructureConfig::get(StructureType::RuinedPortal),
        "ocean_ruins" => StructureConfig::get(StructureType::OceanRuin),
        "shipwrecks" => StructureConfig::get(StructureType::Shipwreck),
        "ocean_monuments" => monument_config(),
        "woodland_mansions" => mansion_config(),
        "mineshafts" => (*MINESHAFT_CONFIG).clone(),
        other => load_random_spread_structure_config(other),
    }
}

fn has_structure_chunk_in_range(
    structure_set_name: &str,
    level_seed: i64,
    source_x: i32,
    source_z: i32,
    chunk_count: i32,
) -> bool {
    let config = random_spread_config_for_structure_set(structure_set_name);
    for test_x in source_x - chunk_count..=source_x + chunk_count {
        for test_z in source_z - chunk_count..=source_z + chunk_count {
            if config.is_structure_chunk(level_seed, test_x, test_z) {
                return true;
            }
        }
    }
    false
}

fn structure_set_entries(structure_set_name: &str) -> Vec<StructureSelectionEntry> {
    load_structure_set_entries(normalize_registry_name(structure_set_name))
}

pub fn structure_set_attempt_order(
    structure_set_name: &str,
    level_seed: i64,
    chunk_x: i32,
    chunk_z: i32,
) -> Vec<StructureSelectionEntry> {
    let mut options = structure_set_entries(structure_set_name);
    if options.len() <= 1 {
        return options;
    }

    let mut random = JavaRandom::from_seed(level_seed);
    set_large_feature_seed(&mut random, level_seed, chunk_x, chunk_z);
    let mut total_weight: i32 = options.iter().map(|option| option.weight).sum();
    let mut ordered = Vec::with_capacity(options.len());

    while !options.is_empty() {
        let mut choice = random.next_int(total_weight as u32);
        let mut selected_index = 0;
        for (index, option) in options.iter().enumerate() {
            choice -= option.weight;
            if choice < 0 {
                selected_index = index;
                break;
            }
        }

        let selected = options.remove(selected_index);
        total_weight -= selected.weight;
        ordered.push(selected);
    }

    ordered
}

pub(crate) fn structure_set_candidate_chunk(
    structure_set_name: &str,
    level_seed: i64,
    chunk_x: i32,
    chunk_z: i32,
) -> bool {
    random_spread_config_for_structure_set(structure_set_name)
        .is_structure_chunk(level_seed, chunk_x, chunk_z)
}

fn load_random_spread_structure_config(structure_set_name: &str) -> StructureConfig {
    let parsed = StructureRuntimeRegistry::global()
        .structure_set(structure_set_name)
        .cloned()
        .unwrap_or_else(|| panic!("missing structure_set registry entry: {structure_set_name}"));
    let placement: RandomSpreadPlacementJson = serde_json::from_value(parsed.placement)
        .unwrap_or_else(|err| {
            panic!("invalid structure_set placement JSON for {structure_set_name}: {err}")
        });

    assert_eq!(
        placement.kind, "minecraft:random_spread",
        "structure_set {structure_set_name} must use random_spread placement"
    );
    assert!(
        placement.spacing > placement.separation,
        "structure_set {structure_set_name} must have spacing > separation"
    );

    StructureConfig {
        salt: placement.salt,
        region_size: placement.spacing,
        chunk_range: placement.spacing - placement.separation,
        spread_type: placement.spread_type,
        frequency: placement.frequency,
        frequency_reduction_method: placement.frequency_reduction_method,
        exclusion_zone: placement.exclusion_zone.map(|zone| ExclusionZone {
            other_set: normalize_registry_name(&zone.other_set).to_owned(),
            chunk_count: zone.chunk_count,
        }),
    }
}

fn load_structure_set_entries(structure_set_name: &str) -> Vec<StructureSelectionEntry> {
    let parsed = StructureRuntimeRegistry::global()
        .structure_set(structure_set_name)
        .cloned()
        .unwrap_or_else(|| panic!("missing structure_set registry entry: {structure_set_name}"));
    parsed
        .structures
        .into_iter()
        .map(|entry| {
            assert!(
                entry.weight > 0,
                "structure_set {structure_set_name} must have positive weights"
            );
            StructureSelectionEntry {
                structure: normalize_registry_name(&entry.structure).to_owned(),
                weight: entry.weight,
            }
        })
        .collect()
}

fn load_stronghold_config() -> StrongholdConfig {
    let parsed = StructureRuntimeRegistry::global()
        .structure_set("strongholds")
        .cloned()
        .unwrap_or_else(|| panic!("missing structure_set registry entry: strongholds"));
    let placement: ConcentricRingsPlacementJson = serde_json::from_value(parsed.placement)
        .unwrap_or_else(|err| {
            panic!("invalid structure_set placement JSON for strongholds: {err}")
        });

    assert_eq!(
        placement.kind, "minecraft:concentric_rings",
        "strongholds must use concentric_rings placement"
    );
    assert_eq!(
        placement.preferred_biomes, "#minecraft:stronghold_biased_to",
        "strongholds must use the stronghold-biased biome tag"
    );

    StrongholdConfig {
        count: placement.count,
        distance: placement.distance,
        spread: placement.spread,
        preferred_biomes: placement.preferred_biomes,
    }
}

static RANDOM_SPREAD_STRUCTURE_CONFIGS: LazyLock<[StructureConfig; 9]> = LazyLock::new(|| {
    [
        load_random_spread_structure_config("villages"),
        load_random_spread_structure_config("desert_pyramids"),
        load_random_spread_structure_config("jungle_temples"),
        load_random_spread_structure_config("swamp_huts"),
        load_random_spread_structure_config("igloos"),
        load_random_spread_structure_config("pillager_outposts"),
        load_random_spread_structure_config("ruined_portals"),
        load_random_spread_structure_config("ocean_ruins"),
        load_random_spread_structure_config("shipwrecks"),
    ]
});

static MONUMENT_CONFIG: LazyLock<StructureConfig> =
    LazyLock::new(|| load_random_spread_structure_config("ocean_monuments"));
static MANSION_CONFIG: LazyLock<StructureConfig> =
    LazyLock::new(|| load_random_spread_structure_config("woodland_mansions"));
static MINESHAFT_CONFIG: LazyLock<StructureConfig> =
    LazyLock::new(|| load_random_spread_structure_config("mineshafts"));
static STRONGHOLD_CONFIG: LazyLock<StrongholdConfig> = LazyLock::new(load_stronghold_config);

impl StructureType {
    const fn as_index(self) -> usize {
        match self {
            StructureType::Village => 0,
            StructureType::DesertPyramid => 1,
            StructureType::JungleTemple => 2,
            StructureType::SwampHut => 3,
            StructureType::Igloo => 4,
            StructureType::PillagerOutpost => 5,
            StructureType::RuinedPortal => 6,
            StructureType::OceanRuin => 7,
            StructureType::Shipwreck => 8,
        }
    }
}

/// Structure position result.
#[derive(Debug, Clone, Copy)]
pub struct StructurePos {
    /// Block X coordinate
    pub x: i32,
    /// Block Z coordinate
    pub z: i32,
    /// Chunk X coordinate
    pub chunk_x: i32,
    /// Chunk Z coordinate
    pub chunk_z: i32,
}

/// Gets the chunk position of a structure within a region.
/// This is the core algorithm from cubiomes getFeatureChunkInRegion.
pub fn get_feature_chunk_in_region(
    config: &StructureConfig,
    seed: i64,
    reg_x: i32,
    reg_z: i32,
) -> (i32, i32) {
    let mut random = JavaRandom::from_seed(large_feature_with_salt_seed(
        seed,
        reg_x,
        reg_z,
        config.salt,
    ));
    let limit = config.chunk_range as u32;
    (
        config.spread_type.sample(&mut random, limit),
        config.spread_type.sample(&mut random, limit),
    )
}

/// Gets the block position of a structure in a given region.
pub fn get_structure_pos(
    config: &StructureConfig,
    seed: i64,
    reg_x: i32,
    reg_z: i32,
) -> StructurePos {
    let (cx, cz) = get_feature_chunk_in_region(config, seed, reg_x, reg_z);

    let chunk_x = reg_x * config.region_size + cx;
    let chunk_z = reg_z * config.region_size + cz;

    StructurePos {
        x: chunk_x << 4,
        z: chunk_z << 4,
        chunk_x,
        chunk_z,
    }
}

/// Find all structure positions in a range of chunks.
pub fn find_structures_in_area(
    structure: StructureType,
    seed: i64,
    chunk_x: i32,
    chunk_z: i32,
    chunk_radius: i32,
) -> Vec<StructurePos> {
    let config = StructureConfig::get(structure);
    let mut results = Vec::new();

    // Calculate region range
    let min_reg_x = floor_div(chunk_x - chunk_radius, config.region_size) - 1;
    let max_reg_x = floor_div(chunk_x + chunk_radius, config.region_size) + 1;
    let min_reg_z = floor_div(chunk_z - chunk_radius, config.region_size) - 1;
    let max_reg_z = floor_div(chunk_z + chunk_radius, config.region_size) + 1;

    for rx in min_reg_x..=max_reg_x {
        for rz in min_reg_z..=max_reg_z {
            let pos = get_structure_pos(&config, seed, rx, rz);

            // Check if within requested chunk range
            if pos.chunk_x >= chunk_x - chunk_radius
                && pos.chunk_x <= chunk_x + chunk_radius
                && pos.chunk_z >= chunk_z - chunk_radius
                && pos.chunk_z <= chunk_z + chunk_radius
                && config.is_structure_chunk(seed, pos.chunk_x, pos.chunk_z)
            {
                results.push(pos);
            }
        }
    }

    results
}

/// Check if a chunk contains a slime chunk.
/// Ported from cubiomes isSlimeChunk.
pub fn is_slime_chunk(seed: i64, chunk_x: i32, chunk_z: i32) -> bool {
    let mut rnd = seed as u64;
    rnd = rnd.wrapping_add(chunk_x.wrapping_mul(0x5ac0db) as u64);
    rnd = rnd.wrapping_add((chunk_x.wrapping_mul(chunk_x)).wrapping_mul(0x4c1906) as u64);
    rnd = rnd.wrapping_add(chunk_z.wrapping_mul(0x5f24f) as u64);
    rnd = rnd.wrapping_add((chunk_z.wrapping_mul(chunk_z) as u64).wrapping_mul(0x4307a7));
    rnd ^= 0x3ad8025f;

    let mut java_rng = JavaRandom::from_seed(rnd as i64);
    java_rng.next_int(10) == 0
}

//==============================================================================
// Stronghold Generation (Java concentric rings with biome relocation)
//==============================================================================

/// Stronghold iterator for ring-based generation.
#[derive(Debug, Clone)]
pub struct StrongholdIter {
    positions: Vec<(i32, i32)>,
    index: usize,
}

fn is_stronghold_preferred_biome(biome: Biome) -> bool {
    matches!(
        biome,
        Biome::Plains
            | Biome::SunflowerPlains
            | Biome::SnowyPlains
            | Biome::IceSpikes
            | Biome::Desert
            | Biome::Forest
            | Biome::FlowerForest
            | Biome::BirchForest
            | Biome::DarkForest
            | Biome::PaleGarden
            | Biome::TallBirchForest
            | Biome::OldGrowthPineTaiga
            | Biome::OldGrowthSpruceTaiga
            | Biome::Taiga
            | Biome::SnowyTaiga
            | Biome::Savanna
            | Biome::SavannaPlateau
            | Biome::WindsweptHills
            | Biome::GravellyMountains
            | Biome::WindsweptForest
            | Biome::WindsweptSavanna
            | Biome::Jungle
            | Biome::SparseJungle
            | Biome::BambooJungle
            | Biome::Badlands
            | Biome::ErodedBadlands
            | Biome::WoodedBadlands
            | Biome::Meadow
            | Biome::CherryGrove
            | Biome::Grove
            | Biome::SnowySlopes
            | Biome::FrozenPeaks
            | Biome::JaggedPeaks
            | Biome::StonyPeaks
            | Biome::MushroomFields
            | Biome::DripstoneCaves
            | Biome::LushCaves
    )
}

fn quart_to_block(quart: i32) -> i32 {
    quart << 2
}

fn block_to_chunk(block: i32) -> i32 {
    block >> 4
}

fn chunk_center_block(chunk: i32) -> i32 {
    (chunk << 4) + 8
}

fn find_stronghold_biome_horizontal(
    biome_noise: &BiomeNoise,
    origin_x: i32,
    origin_z: i32,
    random: &mut JavaRandom,
) -> Option<(i32, i32, Biome)> {
    let noise_center_x = origin_x >> 2;
    let noise_center_z = origin_z >> 2;
    let noise_radius = 112 >> 2;
    let noise_y = 0;
    let mut result = None;
    let mut found = 0u32;

    for z in -noise_radius..=noise_radius {
        for x in -noise_radius..=noise_radius {
            let noise_x = noise_center_x + x;
            let noise_z = noise_center_z + z;
            let biome =
                biome_noise.get_biome(quart_to_block(noise_x), noise_y, quart_to_block(noise_z));
            if is_stronghold_preferred_biome(biome) {
                if result.is_none() || random.next_int(found + 1) == 0 {
                    result = Some((quart_to_block(noise_x), quart_to_block(noise_z), biome));
                }
                found += 1;
            }
        }
    }

    result
}

fn stronghold_candidate_specs(seed: i64) -> Vec<((i32, i32), i64)> {
    let config = &*STRONGHOLD_CONFIG;
    let mut random = JavaRandom::from_seed(seed);
    let mut angle = random.next_double() * PI * 2.0;
    let mut spread = config.spread;
    let mut position_in_circle = 0;
    let mut circle = 0;
    let mut specs = Vec::with_capacity(config.count as usize);

    for i in 0..config.count {
        let dist = (4 * config.distance + config.distance * circle * 6) as f64
            + (random.next_double() - 0.5) * (config.distance as f64 * 2.5);
        let initial_chunk_x = (angle.cos() * dist).round() as i32;
        let initial_chunk_z = (angle.sin() * dist).round() as i32;
        let biome_search_seed = random.next_long();

        specs.push((
            (
                chunk_center_block(initial_chunk_x),
                chunk_center_block(initial_chunk_z),
            ),
            biome_search_seed,
        ));

        angle += (PI * 2.0) / spread as f64;
        position_in_circle += 1;
        if position_in_circle == spread {
            circle += 1;
            position_in_circle = 0;
            spread += 2 * spread / (circle + 1);
            spread = spread.min(config.count - i);
            angle += random.next_double() * PI * 2.0;
        }
    }

    specs
}

fn approximate_stronghold_positions(seed: i64) -> Vec<(i32, i32)> {
    stronghold_candidate_specs(seed)
        .into_iter()
        .map(|(pos, _)| pos)
        .collect()
}

fn generate_stronghold_positions(seed: i64) -> Vec<(i32, i32)> {
    let biome_noise = BiomeNoise::from_seed(seed);
    stronghold_candidate_specs(seed)
        .into_iter()
        .map(|((block_x, block_z), biome_search_seed)| {
            let mut biome_search_random = JavaRandom::from_seed(biome_search_seed);
            match find_stronghold_biome_horizontal(
                &biome_noise,
                block_x,
                block_z,
                &mut biome_search_random,
            ) {
                Some((relocated_x, relocated_z, _)) => (
                    chunk_center_block(block_to_chunk(relocated_x)),
                    chunk_center_block(block_to_chunk(relocated_z)),
                ),
                None => (block_x, block_z),
            }
        })
        .collect()
}

impl StrongholdIter {
    /// Initialize stronghold iterator from Java's concentric-rings placement.
    pub fn new(seed: i64) -> Self {
        Self {
            positions: generate_stronghold_positions(seed),
            index: 0,
        }
    }

    /// Get the next relocated stronghold position.
    pub fn next(&mut self) -> Option<(i32, i32)> {
        let pos = self.positions.get(self.index).copied()?;
        self.index += 1;
        Some(pos)
    }
}

//==============================================================================
// Mineshaft Detection (ported from cubiomes getMineshafts)
//==============================================================================

/// Check if a chunk contains a mineshaft.
/// Ported from cubiomes getMineshafts for MC 1.13+.
/// Mineshafts have a 0.4% chance per chunk.
pub fn has_mineshaft(seed: i64, chunk_x: i32, chunk_z: i32) -> bool {
    MINESHAFT_CONFIG.is_structure_chunk(seed, chunk_x, chunk_z)
}

//==============================================================================
// Monument/Mansion (Large Structures with triangular distribution)
//==============================================================================

/// Get position for large structures (Monument, Mansion) with triangular distribution.
/// Ported from cubiomes getLargeStructurePos.
pub fn get_large_structure_pos(
    config: &StructureConfig,
    seed: i64,
    reg_x: i32,
    reg_z: i32,
) -> StructurePos {
    get_structure_pos(config, seed, reg_x, reg_z)
}

/// Config for Monument (1.8+)
pub fn monument_config() -> StructureConfig {
    (*MONUMENT_CONFIG).clone()
}

/// Config for Mansion (1.11+)
pub fn mansion_config() -> StructureConfig {
    (*MANSION_CONFIG).clone()
}

//==============================================================================
// Chunk Generation Random (for caves/ravines)
//==============================================================================

/// Get the random seed for chunk generation (caves, ravines, etc).
/// Ported from cubiomes chunkGenerateRnd.
pub fn chunk_generate_rnd(seed: i64, chunk_x: i32, chunk_z: i32) -> u64 {
    let mut rnd = seed as u64;
    // setSeed
    rnd = (rnd ^ 0x5DEECE66D) & ((1u64 << 48) - 1);

    // nextLong() for x multiplier
    rnd = rnd.wrapping_mul(0x5DEECE66D).wrapping_add(0xB) & ((1u64 << 48) - 1);
    let x_mult = rnd;

    // nextLong() for z multiplier
    rnd = rnd.wrapping_mul(0x5DEECE66D).wrapping_add(0xB) & ((1u64 << 48) - 1);
    let z_mult = rnd;

    // Combine
    let combined = (x_mult.wrapping_mul(chunk_x as u64))
        ^ (z_mult.wrapping_mul(chunk_z as u64))
        ^ (seed as u64);

    // setSeed on result
    (combined ^ 0x5DEECE66D) & ((1u64 << 48) - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linear_config(salt: i64, region_size: i32, chunk_range: i32) -> StructureConfig {
        StructureConfig {
            salt,
            region_size,
            chunk_range,
            spread_type: RandomSpreadType::Linear,
            frequency: 1.0,
            frequency_reduction_method: FrequencyReductionMethod::Default,
            exclusion_zone: None,
        }
    }

    fn triangular_config(salt: i64, region_size: i32, chunk_range: i32) -> StructureConfig {
        StructureConfig {
            spread_type: RandomSpreadType::Triangular,
            ..linear_config(salt, region_size, chunk_range)
        }
    }

    fn structure_names(entries: &[StructureSelectionEntry]) -> Vec<String> {
        entries
            .iter()
            .map(|entry| entry.structure.clone())
            .collect()
    }

    #[test]
    fn test_village_positions() {
        let seed = 12345i64;
        let config = StructureConfig::get(StructureType::Village);
        let pos = get_structure_pos(&config, seed, 0, 0);
        // Should get a valid position
        assert!(pos.chunk_x >= 0 && pos.chunk_x < config.region_size);
        assert!(pos.chunk_z >= 0 && pos.chunk_z < config.region_size);
    }

    #[test]
    fn test_negative_chunk_search_uses_floor_division() {
        let village = StructureConfig::get(StructureType::Village);
        let expected = get_structure_pos(&village, 1, -1, -1);

        assert_eq!((expected.chunk_x, expected.chunk_z), (-11, -28));
        assert!(village.is_structure_chunk(1, expected.chunk_x, expected.chunk_z));

        let found = find_structures_in_area(
            StructureType::Village,
            1,
            expected.chunk_x,
            expected.chunk_z,
            0,
        );
        assert_eq!(found.len(), 1);
        assert_eq!(
            (found[0].chunk_x, found[0].chunk_z),
            (expected.chunk_x, expected.chunk_z)
        );
    }

    #[test]
    fn test_random_spread_structure_configs_match_registry_values() {
        assert_eq!(
            StructureConfig::get(StructureType::Village),
            linear_config(10387312, 34, 26)
        );
        assert_eq!(
            StructureConfig::get(StructureType::DesertPyramid),
            linear_config(14357617, 32, 24)
        );
        assert_eq!(
            StructureConfig::get(StructureType::JungleTemple),
            linear_config(14357619, 32, 24)
        );
        assert_eq!(
            StructureConfig::get(StructureType::SwampHut),
            linear_config(14357620, 32, 24)
        );
        assert_eq!(
            StructureConfig::get(StructureType::Igloo),
            linear_config(14357618, 32, 24)
        );
        assert_eq!(
            StructureConfig::get(StructureType::PillagerOutpost),
            StructureConfig {
                frequency: 0.2,
                frequency_reduction_method: FrequencyReductionMethod::LegacyType1,
                exclusion_zone: Some(ExclusionZone {
                    other_set: "villages".to_owned(),
                    chunk_count: 10,
                }),
                ..linear_config(165745296, 32, 24)
            }
        );
        assert_eq!(
            StructureConfig::get(StructureType::RuinedPortal),
            linear_config(34222645, 40, 25)
        );
        assert_eq!(
            StructureConfig::get(StructureType::OceanRuin),
            linear_config(14357621, 20, 12)
        );
        assert_eq!(
            StructureConfig::get(StructureType::Shipwreck),
            linear_config(165745295, 24, 20)
        );
    }

    #[test]
    fn test_large_structure_configs_match_registry_values() {
        assert_eq!(monument_config(), triangular_config(10387313, 32, 27));
        assert_eq!(mansion_config(), triangular_config(10387319, 80, 60));
    }

    #[test]
    fn test_mineshaft_config_matches_registry_values() {
        assert_eq!(
            &*MINESHAFT_CONFIG,
            &StructureConfig {
                frequency: 0.004,
                frequency_reduction_method: FrequencyReductionMethod::LegacyType3,
                ..linear_config(0, 1, 1)
            }
        );
    }

    #[test]
    fn test_stronghold_config_matches_registry_values() {
        assert_eq!(
            &*STRONGHOLD_CONFIG,
            &StrongholdConfig {
                count: 128,
                distance: 32,
                spread: 3,
                preferred_biomes: "#minecraft:stronghold_biased_to".to_owned(),
            }
        );
    }

    #[test]
    fn test_multi_entry_structure_sets_are_loaded_from_registry() {
        assert_eq!(structure_set_entries("villages").len(), 5);
        assert_eq!(structure_set_entries("mineshafts").len(), 2);
        assert_eq!(structure_set_entries("nether_complexes").len(), 2);
        assert_eq!(structure_set_entries("ocean_ruins").len(), 2);
        assert_eq!(structure_set_entries("shipwrecks").len(), 2);
        assert_eq!(structure_set_entries("ruined_portals").len(), 7);
    }

    #[test]
    fn test_structure_set_attempt_order_matches_java_weighted_shuffle() {
        assert_eq!(
            structure_names(&structure_set_attempt_order("villages", 0, 0, 0)),
            vec![
                "village_plains".to_owned(),
                "village_taiga".to_owned(),
                "village_savanna".to_owned(),
                "village_snowy".to_owned(),
                "village_desert".to_owned(),
            ]
        );
        assert_eq!(
            structure_names(&structure_set_attempt_order("mineshafts", 0, 0, 0)),
            vec!["mineshaft_mesa".to_owned(), "mineshaft".to_owned()]
        );
        assert_eq!(
            structure_names(&structure_set_attempt_order("nether_complexes", 0, 10, 20)),
            vec!["bastion_remnant".to_owned(), "fortress".to_owned()]
        );
    }

    #[test]
    fn test_pillager_outpost_frequency_reducer_matches_java_data() {
        let outpost = StructureConfig::get(StructureType::PillagerOutpost);
        let blocked = get_structure_pos(&outpost, 0, 0, 0);

        assert_eq!((blocked.chunk_x, blocked.chunk_z), (20, 12));
        assert!(!outpost.apply_additional_chunk_restrictions(0, blocked.chunk_x, blocked.chunk_z));
        assert!(find_structures_in_area(StructureType::PillagerOutpost, 0, 20, 12, 0).is_empty());
    }

    #[test]
    fn test_pillager_outpost_exclusion_zone_matches_java_data() {
        let outpost = StructureConfig::get(StructureType::PillagerOutpost);
        let blocked = get_structure_pos(&outpost, 0, 0, 3);

        assert_eq!((blocked.chunk_x, blocked.chunk_z), (4, 116));
        assert!(outpost.apply_additional_chunk_restrictions(0, blocked.chunk_x, blocked.chunk_z));
        assert!(!outpost.apply_interactions_with_other_structures(
            0,
            blocked.chunk_x,
            blocked.chunk_z
        ));
        assert!(has_structure_chunk_in_range(
            "villages",
            0,
            blocked.chunk_x,
            blocked.chunk_z,
            10
        ));
        assert!(find_structures_in_area(StructureType::PillagerOutpost, 0, 4, 116, 0).is_empty());
    }

    #[test]
    fn test_slime_chunk() {
        // Known slime chunk at seed 0, chunk (0, 0) should be testable
        let is_slime = is_slime_chunk(0, 0, 0);
        // Just verify it runs without panic
        let _ = is_slime;
    }

    #[test]
    fn test_stronghold_iterator() {
        let mut iter = StrongholdIter::new(12345);
        let first = iter.next().unwrap();
        // First stronghold should be ~1500-2000 blocks from origin
        let dist = ((first.0 * first.0 + first.1 * first.1) as f64).sqrt();
        assert!(dist > 500.0 && dist < 3000.0);
    }

    #[test]
    fn test_stronghold_positions_relocate_to_preferred_biomes() {
        for seed in 0..128 {
            let approximate = approximate_stronghold_positions(seed);
            let relocated = generate_stronghold_positions(seed);
            let biome_noise = BiomeNoise::from_seed(seed);

            for (approximate_pos, relocated_pos) in approximate.iter().zip(relocated.iter()) {
                if approximate_pos != relocated_pos {
                    let biome = biome_noise.get_biome(relocated_pos.0, 0, relocated_pos.1);
                    assert!(is_stronghold_preferred_biome(biome));
                    return;
                }
            }
        }

        panic!("expected at least one relocated stronghold in sampled seeds");
    }

    #[test]
    fn test_mineshaft_probability() {
        // Test that mineshafts are rare (0.4%)
        let mut count = 0;
        for cx in -100..100 {
            for cz in -100..100 {
                if has_mineshaft(12345, cx, cz) {
                    count += 1;
                }
            }
        }
        // Expect roughly 160 mineshafts in 40000 chunks (0.4%)
        assert!(count > 50 && count < 300);
    }

    #[test]
    fn test_mineshaft_frequency_reducer_matches_java_data() {
        assert!(has_mineshaft(0, 4, 26));
        assert!(has_mineshaft(0, 16, 14));
        assert!(!has_mineshaft(0, 0, 0));
    }
}
