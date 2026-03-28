use super::BiomeNoise;
use super::beardifier::{
    BeardJunction, BeardRigid, IntBoundingBox, StructureBeardifier, TerrainAdjustment,
};
use super::constants::Biome;
use super::structure_registry::StructureRuntimeRegistry;
use super::structures::{structure_set_attempt_order, structure_set_candidate_chunk};
use super::xoroshiro::{JavaRandom, PositionalRandomFactory, Xoroshiro128};
use crate::world::chunk::{Chunk, MIN_Y, blocks};
use flate2::read::GzDecoder;
use parking_lot::RwLock;
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::sync::{Arc, LazyLock};

const OVERWORLD_JIGSAW_STRUCTURE_SETS: [&str; 5] = [
    "villages",
    "pillager_outposts",
    "ancient_cities",
    "trail_ruins",
    "trial_chambers",
];

const OVERWORLD_DIRECT_STRUCTURE_SETS: [&str; 4] =
    ["igloos", "shipwrecks", "ocean_ruins", "ruined_portals"];

const SHIPWRECK_BEACHED_TEMPLATES: &[&str] = &[
    "shipwreck/with_mast",
    "shipwreck/sideways_full",
    "shipwreck/sideways_fronthalf",
    "shipwreck/sideways_backhalf",
    "shipwreck/rightsideup_full",
    "shipwreck/rightsideup_fronthalf",
    "shipwreck/rightsideup_backhalf",
    "shipwreck/with_mast_degraded",
    "shipwreck/rightsideup_full_degraded",
    "shipwreck/rightsideup_fronthalf_degraded",
    "shipwreck/rightsideup_backhalf_degraded",
];

const SHIPWRECK_OCEAN_TEMPLATES: &[&str] = &[
    "shipwreck/with_mast",
    "shipwreck/upsidedown_full",
    "shipwreck/upsidedown_fronthalf",
    "shipwreck/upsidedown_backhalf",
    "shipwreck/sideways_full",
    "shipwreck/sideways_fronthalf",
    "shipwreck/sideways_backhalf",
    "shipwreck/rightsideup_full",
    "shipwreck/rightsideup_fronthalf",
    "shipwreck/rightsideup_backhalf",
    "shipwreck/with_mast_degraded",
    "shipwreck/upsidedown_full_degraded",
    "shipwreck/upsidedown_fronthalf_degraded",
    "shipwreck/upsidedown_backhalf_degraded",
    "shipwreck/sideways_full_degraded",
    "shipwreck/sideways_fronthalf_degraded",
    "shipwreck/sideways_backhalf_degraded",
    "shipwreck/rightsideup_full_degraded",
    "shipwreck/rightsideup_fronthalf_degraded",
    "shipwreck/rightsideup_backhalf_degraded",
];

const OCEAN_RUIN_WARM_TEMPLATES: &[&str] = &[
    "underwater_ruin/warm_1",
    "underwater_ruin/warm_2",
    "underwater_ruin/warm_3",
    "underwater_ruin/warm_4",
    "underwater_ruin/warm_5",
    "underwater_ruin/warm_6",
    "underwater_ruin/warm_7",
    "underwater_ruin/warm_8",
];

const OCEAN_RUIN_WARM_LARGE_TEMPLATES: &[&str] = &[
    "underwater_ruin/big_warm_4",
    "underwater_ruin/big_warm_5",
    "underwater_ruin/big_warm_6",
    "underwater_ruin/big_warm_7",
];

const OCEAN_RUIN_COLD_BRICK_TEMPLATES: &[&str] = &[
    "underwater_ruin/brick_1",
    "underwater_ruin/brick_2",
    "underwater_ruin/brick_3",
    "underwater_ruin/brick_4",
    "underwater_ruin/brick_5",
    "underwater_ruin/brick_6",
    "underwater_ruin/brick_7",
    "underwater_ruin/brick_8",
];

const OCEAN_RUIN_COLD_BRICK_LARGE_TEMPLATES: &[&str] = &[
    "underwater_ruin/big_brick_1",
    "underwater_ruin/big_brick_2",
    "underwater_ruin/big_brick_3",
    "underwater_ruin/big_brick_8",
];

const RUINED_PORTAL_TEMPLATES: &[&str] = &[
    "ruined_portal/portal_1",
    "ruined_portal/portal_2",
    "ruined_portal/portal_3",
    "ruined_portal/portal_4",
    "ruined_portal/portal_5",
    "ruined_portal/portal_6",
    "ruined_portal/portal_7",
    "ruined_portal/portal_8",
    "ruined_portal/portal_9",
    "ruined_portal/portal_10",
];

const RUINED_PORTAL_GIANT_TEMPLATES: &[&str] = &[
    "ruined_portal/giant_portal_1",
    "ruined_portal/giant_portal_2",
    "ruined_portal/giant_portal_3",
];

pub(crate) fn place_overworld_jigsaw_structure_starts(
    chunk: &mut Chunk,
    level_seed: i64,
    chunk_x: i32,
    chunk_z: i32,
    biome_noise: &BiomeNoise,
    first_free_height: &mut dyn FnMut(i32, i32) -> i32,
) {
    for plan in planned_overworld_jigsaw_structures_for_chunk(
        level_seed,
        chunk_x,
        chunk_z,
        biome_noise,
        first_free_height,
        0,
    ) {
        place_planned_structure(chunk, &plan, level_seed, first_free_height);
    }
}

pub(crate) fn place_overworld_direct_structure_starts(
    chunk: &mut Chunk,
    level_seed: i64,
    chunk_x: i32,
    chunk_z: i32,
    biome_noise: &BiomeNoise,
    solid_first_free_height: &mut dyn FnMut(i32, i32) -> i32,
) {
    for plan in planned_overworld_direct_structures_for_chunk(
        level_seed,
        chunk_x,
        chunk_z,
        biome_noise,
        solid_first_free_height,
    ) {
        place_pool_element(
            chunk,
            &plan.element,
            plan.position,
            plan.rotation,
            level_seed,
            &plan.structure_name,
            solid_first_free_height,
        );
    }
}

pub(crate) fn build_overworld_jigsaw_beardifier(
    level_seed: i64,
    chunk_x: i32,
    chunk_z: i32,
    biome_noise: &BiomeNoise,
    first_free_height: &mut dyn FnMut(i32, i32) -> i32,
) -> StructureBeardifier {
    let mut beardifier = StructureBeardifier::empty();
    for plan in planned_overworld_jigsaw_structures_for_chunk(
        level_seed,
        chunk_x,
        chunk_z,
        biome_noise,
        first_free_height,
        12,
    ) {
        let Some(adaptation_name) = plan.terrain_adaptation.as_deref() else {
            continue;
        };
        let Some(terrain_adjustment) = TerrainAdjustment::from_str(adaptation_name) else {
            continue;
        };
        if matches!(terrain_adjustment, TerrainAdjustment::None) {
            continue;
        }

        for piece in &plan.pieces {
            let piece_bounds = IntBoundingBox {
                min: piece.bounds.min,
                max: piece.bounds.max,
            };
            if piece.projection == Projection::Rigid
                && piece_bounds.close_to_chunk(chunk_x, chunk_z, 12)
            {
                beardifier.add_rigid(BeardRigid {
                    box_bounds: piece_bounds,
                    terrain_adjustment,
                    ground_level_delta: piece.ground_level_delta,
                });
            }
        }
        for junction in &plan.junctions {
            if IntBoundingBox::from_point(
                junction.source_x,
                junction.source_ground_y,
                junction.source_z,
            )
            .close_to_chunk(chunk_x, chunk_z, 12)
            {
                beardifier.add_junction(*junction);
            }
        }
    }

    beardifier.finish()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JigsawRotation {
    None,
    Clockwise90,
    Clockwise180,
    CounterClockwise90,
}

impl JigsawRotation {
    fn random(random: &mut JavaRandom) -> Self {
        match random.next_int(4) {
            1 => Self::Clockwise90,
            2 => Self::Clockwise180,
            3 => Self::CounterClockwise90,
            _ => Self::None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum JigsawFacing {
    North,
    South,
    East,
    West,
    Up,
    Down,
}

impl JigsawFacing {
    fn from_str(value: &str) -> Option<Self> {
        match value {
            "north" => Some(Self::North),
            "south" => Some(Self::South),
            "east" => Some(Self::East),
            "west" => Some(Self::West),
            "up" => Some(Self::Up),
            "down" => Some(Self::Down),
            _ => None,
        }
    }

    fn rotate(self, rotation: JigsawRotation) -> Self {
        if matches!(self, Self::Up | Self::Down) {
            return self;
        }

        match rotation {
            JigsawRotation::Clockwise90 => match self {
                Self::North => Self::East,
                Self::East => Self::South,
                Self::South => Self::West,
                Self::West => Self::North,
                other => other,
            },
            JigsawRotation::Clockwise180 => match self {
                Self::North => Self::South,
                Self::East => Self::West,
                Self::South => Self::North,
                Self::West => Self::East,
                other => other,
            },
            JigsawRotation::CounterClockwise90 => match self {
                Self::North => Self::West,
                Self::West => Self::South,
                Self::South => Self::East,
                Self::East => Self::North,
                other => other,
            },
            JigsawRotation::None => self,
        }
    }

    fn opposite(self) -> Self {
        match self {
            Self::North => Self::South,
            Self::South => Self::North,
            Self::East => Self::West,
            Self::West => Self::East,
            Self::Up => Self::Down,
            Self::Down => Self::Up,
        }
    }

    fn step(self) -> [i32; 3] {
        match self {
            Self::North => [0, 0, -1],
            Self::South => [0, 0, 1],
            Self::East => [1, 0, 0],
            Self::West => [-1, 0, 0],
            Self::Up => [0, 1, 0],
            Self::Down => [0, -1, 0],
        }
    }

    fn is_horizontal(self) -> bool {
        matches!(self, Self::North | Self::South | Self::East | Self::West)
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::North => "north",
            Self::South => "south",
            Self::East => "east",
            Self::West => "west",
            Self::Up => "up",
            Self::Down => "down",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JigsawJointType {
    Aligned,
    Rollable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct JigsawMaxDistance {
    horizontal: i32,
    vertical: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StartHeightProvider {
    Absolute(i32),
    Uniform {
        min_inclusive: i32,
        max_inclusive: i32,
    },
}

impl StartHeightProvider {
    fn sample(&self, random: &mut JavaRandom) -> i32 {
        match self {
            Self::Absolute(value) => *value,
            Self::Uniform {
                min_inclusive,
                max_inclusive,
            } => {
                let span = (max_inclusive - min_inclusive + 1) as u32;
                min_inclusive + random.next_int(span)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct JigsawStructureDefinition {
    start_pool: String,
    start_jigsaw_name: Option<String>,
    max_depth: i32,
    start_height: StartHeightProvider,
    use_expansion_hack: bool,
    project_start_to_heightmap: Option<String>,
    max_distance_from_center: JigsawMaxDistance,
    terrain_adaptation: Option<String>,
    pool_aliases: Vec<Value>,
    dimension_padding: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Projection {
    Rigid,
    TerrainMatching,
}

#[derive(Debug, Clone, PartialEq)]
enum RuntimePoolElement {
    Empty,
    Feature {
        feature: String,
        projection: Projection,
    },
    Single {
        template: String,
        processors: Vec<RuntimeProcessor>,
        projection: Projection,
        legacy: bool,
    },
    List {
        elements: Vec<RuntimePoolElement>,
        projection: Projection,
    },
}

#[derive(Debug, Clone, PartialEq)]
struct WeightedPoolElement {
    element: RuntimePoolElement,
    weight: i32,
}

#[derive(Debug, Clone, PartialEq)]
struct ParsedTemplatePool {
    fallback: String,
    elements: Vec<WeightedPoolElement>,
}

#[derive(Debug, Clone, PartialEq)]
enum RuntimeProcessor {
    Rule(Vec<RuleTransform>),
    BlockRot(f32),
    Noop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuleTransform {
    input_block: String,
    probability: ProbabilityKey,
    output_state: JavaBlockState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ProbabilityKey(u32);

impl ProbabilityKey {
    fn from_f32(value: f32) -> Self {
        Self(value.to_bits())
    }

    fn as_f32(self) -> f32 {
        f32::from_bits(self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct JavaBlockState {
    name: String,
    properties: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedStructureBlock {
    pos: [i32; 3],
    state: JavaBlockState,
    nbt: BTreeMap<String, JavaNbtValue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedStructureTemplate {
    size: [i32; 3],
    blocks: Vec<ParsedStructureBlock>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeJigsawBlock {
    pos: [i32; 3],
    local_pos: [i32; 3],
    front: JigsawFacing,
    top: JigsawFacing,
    joint_type: JigsawJointType,
    name: String,
    pool: String,
    target: String,
    placement_priority: i32,
    selection_priority: i32,
}

#[derive(Debug, Clone, PartialEq)]
struct PlannedPiece {
    element: RuntimePoolElement,
    projection: Projection,
    rotation: JigsawRotation,
    position: [i32; 3],
    ground_level_delta: i32,
    bounds: BoundingBox3,
}

#[derive(Debug, Clone, PartialEq)]
struct PlannedStructure {
    structure_name: String,
    start_position: [i32; 3],
    terrain_adaptation: Option<String>,
    center_position: [i32; 3],
    junctions: Vec<BeardJunction>,
    pieces: Vec<PlannedPiece>,
    bounds: BoundingBox3,
}

impl PlannedStructure {
    fn intersects_chunk(&self, chunk_x: i32, chunk_z: i32) -> bool {
        let chunk_bounds = BoundingBox3 {
            min: [chunk_x * 16, MIN_Y, chunk_z * 16],
            max: [chunk_x * 16 + 15, 319, chunk_z * 16 + 15],
        };
        self.bounds.intersects(chunk_bounds)
    }

    fn is_close_to_chunk(&self, chunk_x: i32, chunk_z: i32, padding: i32) -> bool {
        self.bounds.close_to_chunk(chunk_x, chunk_z, padding)
    }
}

#[derive(Debug, Clone, PartialEq)]
struct DirectPlannedStructure {
    structure_name: String,
    element: RuntimePoolElement,
    rotation: JigsawRotation,
    position: [i32; 3],
    bounds: BoundingBox3,
}

impl DirectPlannedStructure {
    fn intersects_chunk(&self, chunk_x: i32, chunk_z: i32) -> bool {
        let chunk_bounds = BoundingBox3 {
            min: [chunk_x * 16, MIN_Y, chunk_z * 16],
            max: [chunk_x * 16 + 15, 319, chunk_z * 16 + 15],
        };
        self.bounds.intersects(chunk_bounds)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum JavaNbtValue {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(u32),
    Double(u64),
    String(String),
    ByteArray(Vec<u8>),
    List(Vec<JavaNbtValue>),
    Compound(BTreeMap<String, JavaNbtValue>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl JavaNbtValue {
    fn as_i32(&self) -> Option<i32> {
        match self {
            Self::Byte(value) => Some(*value as i32),
            Self::Short(value) => Some(*value as i32),
            Self::Int(value) => Some(*value),
            _ => None,
        }
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    fn as_list(&self) -> Option<&[JavaNbtValue]> {
        match self {
            Self::List(values) => Some(values),
            _ => None,
        }
    }

    fn as_compound(&self) -> Option<&BTreeMap<String, JavaNbtValue>> {
        match self {
            Self::Compound(values) => Some(values),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BedrockStateScalar {
    Byte(i8),
    Int(i32),
    String(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BedrockPaletteEntry {
    runtime_id: u32,
    name: String,
    states: BTreeMap<String, BedrockStateScalar>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawJigsawStructureDefinition {
    #[serde(rename = "type")]
    kind: String,
    start_pool: String,
    #[serde(default)]
    start_jigsaw_name: Option<String>,
    size: i32,
    start_height: RawHeightProvider,
    use_expansion_hack: bool,
    #[serde(default)]
    project_start_to_heightmap: Option<String>,
    max_distance_from_center: RawMaxDistance,
    #[serde(default)]
    terrain_adaptation: Option<String>,
    #[serde(default)]
    pool_aliases: Vec<Value>,
    #[serde(default)]
    dimension_padding: Option<i32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum RawMaxDistance {
    Horizontal(i32),
    Full {
        horizontal: i32,
        vertical: Option<i32>,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum RawHeightProvider {
    Absolute {
        absolute: i32,
    },
    Uniform {
        #[serde(rename = "type")]
        kind: String,
        min_inclusive: RawVerticalAnchor,
        max_inclusive: RawVerticalAnchor,
    },
}

#[derive(Debug, Clone, Deserialize)]
struct RawVerticalAnchor {
    absolute: i32,
}

#[derive(Debug, Clone, Deserialize)]
struct RawTemplatePool {
    fallback: String,
    elements: Vec<RawTemplatePoolEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawTemplatePoolEntry {
    element: RawPoolElement,
    weight: i32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "element_type")]
enum RawPoolElement {
    #[serde(rename = "minecraft:empty_pool_element")]
    Empty,
    #[serde(rename = "minecraft:feature_pool_element")]
    Feature {
        feature: String,
        projection: Projection,
    },
    #[serde(rename = "minecraft:single_pool_element")]
    Single {
        location: String,
        processors: Value,
        projection: Projection,
    },
    #[serde(rename = "minecraft:legacy_single_pool_element")]
    LegacySingle {
        location: String,
        processors: Value,
        projection: Projection,
    },
    #[serde(rename = "minecraft:list_pool_element")]
    List {
        elements: Vec<RawPoolElement>,
        projection: Projection,
    },
}

#[derive(Debug, Clone, Deserialize)]
struct RawProcessorList {
    processors: Vec<Value>,
}

static TEMPLATE_POOL_CACHE: LazyLock<RwLock<HashMap<String, Arc<ParsedTemplatePool>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));
static STRUCTURE_TEMPLATE_CACHE: LazyLock<RwLock<HashMap<String, Arc<ParsedStructureTemplate>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));
static JAVA_STATE_CACHE: LazyLock<RwLock<HashMap<String, u32>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));
static TEMPLATE_POOL_MAX_SIZE_CACHE: LazyLock<RwLock<HashMap<String, i32>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));
static BEDROCK_BLOCK_PALETTE: LazyLock<HashMap<String, Vec<BedrockPaletteEntry>>> =
    LazyLock::new(load_bedrock_block_palette);

fn load_jigsaw_structure_definition(name: &str) -> Option<JigsawStructureDefinition> {
    let raw = StructureRuntimeRegistry::global().structure(name)?;
    if raw.kind != "minecraft:jigsaw" {
        return None;
    }
    let parsed: RawJigsawStructureDefinition =
        serde_json::from_value(structure_json_value(raw)).ok()?;
    if parsed.kind != "minecraft:jigsaw" {
        return None;
    }
    let definition = JigsawStructureDefinition {
        start_pool: normalize_registry_name(&parsed.start_pool).to_owned(),
        start_jigsaw_name: parsed
            .start_jigsaw_name
            .map(|name| normalize_registry_name(&name).to_owned()),
        max_depth: parsed.size,
        start_height: match parsed.start_height {
            RawHeightProvider::Absolute { absolute } => StartHeightProvider::Absolute(absolute),
            RawHeightProvider::Uniform {
                kind,
                min_inclusive,
                max_inclusive,
            } => {
                if kind != "minecraft:uniform" {
                    return None;
                }
                StartHeightProvider::Uniform {
                    min_inclusive: min_inclusive.absolute,
                    max_inclusive: max_inclusive.absolute,
                }
            }
        },
        use_expansion_hack: parsed.use_expansion_hack,
        project_start_to_heightmap: parsed.project_start_to_heightmap,
        max_distance_from_center: match parsed.max_distance_from_center {
            RawMaxDistance::Horizontal(horizontal) => JigsawMaxDistance {
                horizontal,
                vertical: horizontal,
            },
            RawMaxDistance::Full {
                horizontal,
                vertical,
            } => JigsawMaxDistance {
                horizontal,
                vertical: vertical.unwrap_or(horizontal),
            },
        },
        terrain_adaptation: parsed.terrain_adaptation,
        pool_aliases: parsed.pool_aliases,
        dimension_padding: parsed.dimension_padding.unwrap_or(0),
    };
    let edge_needed = match definition.terrain_adaptation.as_deref() {
        Some("bury" | "beard_thin" | "beard_box" | "encapsulate") => 12,
        _ => 0,
    };
    if definition.max_distance_from_center.horizontal + edge_needed > 128 {
        return None;
    }

    Some(definition)
}

fn structure_json_value(raw: &super::structure_registry::RuntimeStructureDefinition) -> Value {
    let mut value = serde_json::Map::new();
    value.insert("type".to_owned(), Value::String(raw.kind.clone()));
    value.insert("biomes".to_owned(), raw.biomes.clone());
    value.insert("spawn_overrides".to_owned(), raw.spawn_overrides.clone());
    value.insert("step".to_owned(), Value::String(raw.step.clone()));
    if let Some(adaptation) = &raw.terrain_adaptation {
        value.insert(
            "terrain_adaptation".to_owned(),
            Value::String(adaptation.clone()),
        );
    }
    for (key, value_entry) in &raw.extra {
        value.insert(key.clone(), value_entry.clone());
    }
    Value::Object(value)
}

fn planned_overworld_jigsaw_structures_for_chunk(
    level_seed: i64,
    chunk_x: i32,
    chunk_z: i32,
    biome_noise: &BiomeNoise,
    first_free_height: &mut dyn FnMut(i32, i32) -> i32,
    extra_margin_blocks: i32,
) -> Vec<PlannedStructure> {
    let mut plans = Vec::new();
    for structure_set in OVERWORLD_JIGSAW_STRUCTURE_SETS {
        let search_radius = structure_set_search_radius_chunks(structure_set, extra_margin_blocks);
        for start_chunk_z in chunk_z - search_radius..=chunk_z + search_radius {
            for start_chunk_x in chunk_x - search_radius..=chunk_x + search_radius {
                if !structure_set_candidate_chunk(
                    structure_set,
                    level_seed,
                    start_chunk_x,
                    start_chunk_z,
                ) {
                    continue;
                }

                let Some(plan) = plan_structure_start_for_set(
                    structure_set,
                    level_seed,
                    start_chunk_x,
                    start_chunk_z,
                    biome_noise,
                    first_free_height,
                ) else {
                    continue;
                };

                if plan.is_close_to_chunk(chunk_x, chunk_z, extra_margin_blocks) {
                    plans.push(plan);
                }
            }
        }
    }
    plans
}

fn planned_overworld_direct_structures_for_chunk(
    level_seed: i64,
    chunk_x: i32,
    chunk_z: i32,
    biome_noise: &BiomeNoise,
    solid_first_free_height: &mut dyn FnMut(i32, i32) -> i32,
) -> Vec<DirectPlannedStructure> {
    let mut plans = Vec::new();
    for structure_set in OVERWORLD_DIRECT_STRUCTURE_SETS {
        let search_radius = 4;
        for start_chunk_z in chunk_z - search_radius..=chunk_z + search_radius {
            for start_chunk_x in chunk_x - search_radius..=chunk_x + search_radius {
                if !structure_set_candidate_chunk(
                    structure_set,
                    level_seed,
                    start_chunk_x,
                    start_chunk_z,
                ) {
                    continue;
                }
                let Some(plan) = plan_direct_structure_start_for_set(
                    structure_set,
                    level_seed,
                    start_chunk_x,
                    start_chunk_z,
                    biome_noise,
                    solid_first_free_height,
                ) else {
                    continue;
                };
                if plan.intersects_chunk(chunk_x, chunk_z) {
                    plans.push(plan);
                }
            }
        }
    }
    plans
}

fn plan_direct_structure_start_for_set(
    structure_set: &str,
    level_seed: i64,
    start_chunk_x: i32,
    start_chunk_z: i32,
    biome_noise: &BiomeNoise,
    solid_first_free_height: &mut dyn FnMut(i32, i32) -> i32,
) -> Option<DirectPlannedStructure> {
    for selected in
        structure_set_attempt_order(structure_set, level_seed, start_chunk_x, start_chunk_z)
    {
        let biome_probe = [
            start_chunk_x * 16 + 8,
            solid_first_free_height(start_chunk_x * 16 + 8, start_chunk_z * 16 + 8),
            start_chunk_z * 16 + 8,
        ];
        if !structure_biome_matches(&selected.structure, biome_probe, biome_noise) {
            continue;
        }

        let mut random = JavaRandom::from_seed(level_seed);
        set_large_feature_seed(&mut random, level_seed, start_chunk_x, start_chunk_z);
        let plan = match selected.structure.as_str() {
            "igloo" => plan_igloo_direct_structure(
                start_chunk_x,
                start_chunk_z,
                &mut random,
                solid_first_free_height,
            ),
            "shipwreck" | "shipwreck_beached" => plan_shipwreck_direct_structure(
                &selected.structure,
                start_chunk_x,
                start_chunk_z,
                &mut random,
                solid_first_free_height,
            ),
            "ocean_ruin_warm" | "ocean_ruin_cold" => plan_ocean_ruin_direct_structure(
                &selected.structure,
                start_chunk_x,
                start_chunk_z,
                &mut random,
                solid_first_free_height,
            ),
            "ruined_portal"
            | "ruined_portal_desert"
            | "ruined_portal_jungle"
            | "ruined_portal_swamp"
            | "ruined_portal_mountain"
            | "ruined_portal_ocean"
            | "ruined_portal_nether" => plan_ruined_portal_direct_structure(
                &selected.structure,
                start_chunk_x,
                start_chunk_z,
                &mut random,
                solid_first_free_height,
            ),
            _ => None,
        };
        if plan.is_some() {
            return plan;
        }
    }
    None
}

fn plan_igloo_direct_structure(
    start_chunk_x: i32,
    start_chunk_z: i32,
    random: &mut JavaRandom,
    solid_first_free_height: &mut dyn FnMut(i32, i32) -> i32,
) -> Option<DirectPlannedStructure> {
    let element = direct_single_template_element("igloo/top", true);
    let rotation = JigsawRotation::random(random);
    let mut position = [start_chunk_x * 16, 90, start_chunk_z * 16];
    let initial_bounds = element_bounding_box(&element, position, rotation)?;
    let center_x = (initial_bounds.min[0] + initial_bounds.max[0]) / 2;
    let center_z = (initial_bounds.min[2] + initial_bounds.max[2]) / 2;
    position[1] = solid_first_free_height(center_x, center_z) - 1;
    let bounds = element_bounding_box(&element, position, rotation)?;
    Some(DirectPlannedStructure {
        structure_name: "igloo".to_owned(),
        element,
        rotation,
        position,
        bounds,
    })
}

fn plan_shipwreck_direct_structure(
    structure_name: &str,
    start_chunk_x: i32,
    start_chunk_z: i32,
    random: &mut JavaRandom,
    solid_first_free_height: &mut dyn FnMut(i32, i32) -> i32,
) -> Option<DirectPlannedStructure> {
    let beached = StructureRuntimeRegistry::global()
        .structure(structure_name)
        .and_then(|structure| structure.extra.get("is_beached"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let templates = if beached {
        SHIPWRECK_BEACHED_TEMPLATES
    } else {
        SHIPWRECK_OCEAN_TEMPLATES
    };
    let template = choose_direct_template(templates, random)?;
    let element = direct_single_template_element(template, true);
    let rotation = JigsawRotation::random(random);
    let mut position = [start_chunk_x * 16, 90, start_chunk_z * 16];
    let initial_bounds = element_bounding_box(&element, position, rotation)?;
    position[1] = if beached {
        template_min_surface_y(initial_bounds, solid_first_free_height)
            - (initial_bounds.max[1] - initial_bounds.min[1] + 1) / 2
            - random.next_int(3)
    } else {
        template_mean_surface_y(initial_bounds, solid_first_free_height)
    };
    let bounds = element_bounding_box(&element, position, rotation)?;
    Some(DirectPlannedStructure {
        structure_name: structure_name.to_owned(),
        element,
        rotation,
        position,
        bounds,
    })
}

fn plan_ocean_ruin_direct_structure(
    structure_name: &str,
    start_chunk_x: i32,
    start_chunk_z: i32,
    random: &mut JavaRandom,
    solid_first_free_height: &mut dyn FnMut(i32, i32) -> i32,
) -> Option<DirectPlannedStructure> {
    let large_probability = StructureRuntimeRegistry::global()
        .structure(structure_name)
        .and_then(|structure| structure.extra.get("large_probability"))
        .and_then(Value::as_f64)
        .unwrap_or(0.3);
    let is_large = random.next_float() < large_probability as f32;
    let templates = match structure_name {
        "ocean_ruin_warm" if is_large => OCEAN_RUIN_WARM_LARGE_TEMPLATES,
        "ocean_ruin_warm" => OCEAN_RUIN_WARM_TEMPLATES,
        "ocean_ruin_cold" if is_large => OCEAN_RUIN_COLD_BRICK_LARGE_TEMPLATES,
        "ocean_ruin_cold" => OCEAN_RUIN_COLD_BRICK_TEMPLATES,
        _ => return None,
    };
    let template = choose_direct_template(templates, random)?;
    let element = direct_single_template_element(template, true);
    let rotation = JigsawRotation::random(random);
    let mut position = [start_chunk_x * 16 + 8, 90, start_chunk_z * 16 + 8];
    let initial_bounds = element_bounding_box(&element, position, rotation)?;
    position[1] = template_floor_y(initial_bounds, solid_first_free_height);
    let bounds = element_bounding_box(&element, position, rotation)?;
    Some(DirectPlannedStructure {
        structure_name: structure_name.to_owned(),
        element,
        rotation,
        position,
        bounds,
    })
}

fn plan_ruined_portal_direct_structure(
    structure_name: &str,
    start_chunk_x: i32,
    start_chunk_z: i32,
    random: &mut JavaRandom,
    solid_first_free_height: &mut dyn FnMut(i32, i32) -> i32,
) -> Option<DirectPlannedStructure> {
    let structure = StructureRuntimeRegistry::global().structure(structure_name)?;
    let placement = choose_ruined_portal_placement(structure.extra.get("setups")?, random)?;
    let templates = if random.next_float() < 0.05 {
        RUINED_PORTAL_GIANT_TEMPLATES
    } else {
        RUINED_PORTAL_TEMPLATES
    };
    let template = choose_direct_template(templates, random)?;
    let element = direct_single_template_element(template, true);
    let rotation = JigsawRotation::random(random);
    let mut position = [start_chunk_x * 16, 0, start_chunk_z * 16];
    let initial_bounds = element_bounding_box(&element, position, rotation)?;
    let center_x = (initial_bounds.min[0] + initial_bounds.max[0]) / 2;
    let center_z = (initial_bounds.min[2] + initial_bounds.max[2]) / 2;
    let surface_y = solid_first_free_height(center_x, center_z) - 1;
    let y_span = initial_bounds.max[1] - initial_bounds.min[1] + 1;
    position[1] = ruined_portal_target_y(&placement, surface_y, y_span, random);
    let bounds = element_bounding_box(&element, position, rotation)?;
    Some(DirectPlannedStructure {
        structure_name: structure_name.to_owned(),
        element,
        rotation,
        position,
        bounds,
    })
}

fn direct_single_template_element(template: &str, legacy: bool) -> RuntimePoolElement {
    RuntimePoolElement::Single {
        template: normalize_registry_name(template).to_owned(),
        processors: Vec::new(),
        projection: Projection::Rigid,
        legacy,
    }
}

fn choose_direct_template<'a>(
    templates: &'a [&'a str],
    random: &mut JavaRandom,
) -> Option<&'a str> {
    if templates.is_empty() {
        return None;
    }
    Some(templates[random.next_int(templates.len() as u32) as usize])
}

fn choose_ruined_portal_placement(setups: &Value, random: &mut JavaRandom) -> Option<String> {
    let setups = setups.as_array()?;
    let total_weight: f64 = setups
        .iter()
        .map(|setup| {
            setup
                .get("weight")
                .and_then(Value::as_f64)
                .unwrap_or(0.0)
                .max(0.0)
        })
        .sum();
    if total_weight <= 0.0 {
        return setups
            .first()
            .and_then(|setup| setup.get("placement"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
    }
    let mut choice = random.next_float() as f64 * total_weight;
    for setup in setups {
        let weight = setup
            .get("weight")
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
            .max(0.0);
        if choice < weight {
            return setup
                .get("placement")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
        }
        choice -= weight;
    }
    setups
        .first()
        .and_then(|setup| setup.get("placement"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn ruined_portal_target_y(
    placement: &str,
    surface_y: i32,
    y_span: i32,
    random: &mut JavaRandom,
) -> i32 {
    match placement {
        "underground" => random_between_inclusive(MIN_Y + 15, surface_y - y_span, random),
        "partly_buried" => surface_y - y_span + random_between_inclusive(2, 8, random),
        "in_mountain" => random_between_inclusive(70, surface_y - y_span, random),
        _ => surface_y,
    }
}

fn random_between_inclusive(min_value: i32, max_value: i32, random: &mut JavaRandom) -> i32 {
    if max_value <= min_value {
        return min_value;
    }
    min_value + random.next_int((max_value - min_value + 1) as u32)
}

fn template_mean_surface_y(
    bounds: BoundingBox3,
    solid_first_free_height: &mut dyn FnMut(i32, i32) -> i32,
) -> i32 {
    let mut total = 0;
    let mut count = 0;
    for x in bounds.min[0]..=bounds.max[0] {
        for z in bounds.min[2]..=bounds.max[2] {
            total += solid_first_free_height(x, z) - 1;
            count += 1;
        }
    }
    if count == 0 { MIN_Y } else { total / count }
}

fn template_min_surface_y(
    bounds: BoundingBox3,
    solid_first_free_height: &mut dyn FnMut(i32, i32) -> i32,
) -> i32 {
    let mut value = 319;
    for x in bounds.min[0]..=bounds.max[0] {
        for z in bounds.min[2]..=bounds.max[2] {
            value = value.min(solid_first_free_height(x, z) - 1);
        }
    }
    value
}

fn template_floor_y(
    bounds: BoundingBox3,
    solid_first_free_height: &mut dyn FnMut(i32, i32) -> i32,
) -> i32 {
    template_min_surface_y(bounds, solid_first_free_height) + 1
}

fn structure_set_search_radius_chunks(structure_set: &str, extra_margin_blocks: i32) -> i32 {
    let Some(set) = StructureRuntimeRegistry::global().structure_set(structure_set) else {
        return 0;
    };

    let max_distance = set
        .structures
        .iter()
        .filter_map(|entry| load_jigsaw_structure_definition(&entry.structure))
        .map(|definition| definition.max_distance_from_center.horizontal)
        .max()
        .unwrap_or(0);
    (max_distance + extra_margin_blocks + 31).div_euclid(16) + 2
}

fn plan_structure_start_for_set(
    structure_set: &str,
    level_seed: i64,
    start_chunk_x: i32,
    start_chunk_z: i32,
    biome_noise: &BiomeNoise,
    first_free_height: &mut dyn FnMut(i32, i32) -> i32,
) -> Option<PlannedStructure> {
    for selected in
        structure_set_attempt_order(structure_set, level_seed, start_chunk_x, start_chunk_z)
    {
        let Some(definition) = load_jigsaw_structure_definition(&selected.structure) else {
            continue;
        };

        let mut random = JavaRandom::from_seed(level_seed);
        set_large_feature_seed(&mut random, level_seed, start_chunk_x, start_chunk_z);
        let Some(mut plan) = plan_root_piece(
            &selected.structure,
            &definition,
            start_chunk_x,
            start_chunk_z,
            &mut random,
            first_free_height,
        ) else {
            continue;
        };

        if !structure_biome_matches(&selected.structure, plan.center_position, biome_noise) {
            continue;
        }

        let aliases =
            resolve_pool_aliases(&definition.pool_aliases, plan.start_position, level_seed);
        expand_structure_pieces(
            &mut plan,
            &definition,
            &aliases,
            &mut random,
            first_free_height,
        );
        if structure_within_dimension_padding(&plan, definition.dimension_padding) {
            return Some(plan);
        }
    }

    None
}

fn plan_root_piece(
    structure_name: &str,
    definition: &JigsawStructureDefinition,
    chunk_x: i32,
    chunk_z: i32,
    random: &mut JavaRandom,
    first_free_height: &mut dyn FnMut(i32, i32) -> i32,
) -> Option<PlannedStructure> {
    let rotation = JigsawRotation::random(random);
    let root_element = choose_weighted_pool_element(&definition.start_pool, random)?;

    let start_y = definition.start_height.sample(random);
    let position = [chunk_x * 16, start_y, chunk_z * 16];
    let anchor_offset = if let Some(anchor_name) = &definition.start_jigsaw_name {
        element_anchor_offset(&root_element, anchor_name, rotation, random)?
    } else {
        [0, 0, 0]
    };
    let mut adjusted_position = [
        position[0] - anchor_offset[0],
        position[1] - anchor_offset[1],
        position[2] - anchor_offset[2],
    ];

    let initial_bounds = element_bounding_box(&root_element, adjusted_position, rotation)?;
    let bottom_y = match definition.project_start_to_heightmap.as_deref() {
        Some("WORLD_SURFACE_WG") => {
            let center_x = (initial_bounds.min[0] + initial_bounds.max[0]) / 2;
            let center_z = (initial_bounds.min[2] + initial_bounds.max[2]) / 2;
            position[1] + first_free_height(center_x, center_z)
        }
        _ => adjusted_position[1],
    };
    let old_absolute_ground_y = initial_bounds.min[1] + 1;
    adjusted_position[1] += bottom_y - old_absolute_ground_y;
    let bounds = element_bounding_box(&root_element, adjusted_position, rotation)?;
    let center_x = (bounds.min[0] + bounds.max[0]) / 2;
    let center_z = (bounds.min[2] + bounds.max[2]) / 2;
    let center_y = bottom_y + anchor_offset[1];
    let projection = element_projection(&root_element);

    Some(PlannedStructure {
        structure_name: structure_name.to_owned(),
        start_position: position,
        terrain_adaptation: definition.terrain_adaptation.clone(),
        center_position: [center_x, center_y, center_z],
        junctions: Vec::new(),
        bounds,
        pieces: vec![PlannedPiece {
            element: root_element,
            projection,
            rotation,
            position: adjusted_position,
            ground_level_delta: 1,
            bounds,
        }],
    })
}

fn place_planned_structure(
    chunk: &mut Chunk,
    plan: &PlannedStructure,
    level_seed: i64,
    first_free_height: &mut dyn FnMut(i32, i32) -> i32,
) {
    for piece in &plan.pieces {
        place_pool_element(
            chunk,
            &piece.element,
            piece.position,
            piece.rotation,
            level_seed,
            &plan.structure_name,
            first_free_height,
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PendingPlacement {
    piece_index: usize,
    depth: i32,
    priority: i32,
    sequence: usize,
}

fn structure_biome_matches(
    structure_name: &str,
    position: [i32; 3],
    biome_noise: &BiomeNoise,
) -> bool {
    let Some(structure) = StructureRuntimeRegistry::global().structure(structure_name) else {
        return false;
    };
    let biome = biome_noise.get_biome(position[0], position[1], position[2]);
    biome_selector_matches(&structure.biomes, biome)
}

fn biome_selector_matches(selector: &Value, biome: Biome) -> bool {
    match selector {
        Value::String(name_or_tag) => biome_name_or_tag_matches(name_or_tag, biome),
        Value::Array(values) => values
            .iter()
            .any(|value| biome_selector_matches(value, biome)),
        _ => true,
    }
}

fn biome_name_or_tag_matches(name_or_tag: &str, biome: Biome) -> bool {
    if let Some(tag_name) = name_or_tag.strip_prefix('#') {
        return match tag_name.strip_prefix("minecraft:").unwrap_or(tag_name) {
            "has_structure/village_desert" => matches!(biome, Biome::Desert),
            "has_structure/village_plains" => matches!(biome, Biome::Plains | Biome::Meadow),
            "has_structure/village_savanna" => matches!(biome, Biome::Savanna),
            "has_structure/village_snowy" => matches!(biome, Biome::SnowyPlains),
            "has_structure/village_taiga" => matches!(biome, Biome::Taiga),
            "has_structure/pillager_outpost" => {
                matches!(
                    biome,
                    Biome::Desert
                        | Biome::Plains
                        | Biome::Savanna
                        | Biome::SnowyPlains
                        | Biome::Taiga
                        | Biome::Grove
                ) || biome_name_or_tag_matches("#minecraft:is_mountain", biome)
            }
            "has_structure/ancient_city" => matches!(biome, Biome::DeepDark),
            "has_structure/trial_chambers" => matches!(
                biome,
                Biome::MushroomFields
                    | Biome::DeepFrozenOcean
                    | Biome::FrozenOcean
                    | Biome::DeepColdOcean
                    | Biome::ColdOcean
                    | Biome::DeepOcean
                    | Biome::Ocean
                    | Biome::DeepLukewarmOcean
                    | Biome::LukewarmOcean
                    | Biome::WarmOcean
                    | Biome::StonyShore
                    | Biome::Swamp
                    | Biome::MangroveSwamp
                    | Biome::SnowySlopes
                    | Biome::SnowyPlains
                    | Biome::SnowyBeach
                    | Biome::GravellyMountains
                    | Biome::Grove
                    | Biome::WindsweptHills
                    | Biome::SnowyTaiga
                    | Biome::WindsweptForest
                    | Biome::Taiga
                    | Biome::Plains
                    | Biome::Meadow
                    | Biome::Beach
                    | Biome::Forest
                    | Biome::OldGrowthSpruceTaiga
                    | Biome::FlowerForest
                    | Biome::BirchForest
                    | Biome::DarkForest
                    | Biome::PaleGarden
                    | Biome::SavannaPlateau
                    | Biome::Savanna
                    | Biome::Jungle
                    | Biome::Badlands
                    | Biome::Desert
                    | Biome::WoodedBadlands
                    | Biome::JaggedPeaks
                    | Biome::StonyPeaks
                    | Biome::FrozenRiver
                    | Biome::River
                    | Biome::IceSpikes
                    | Biome::OldGrowthPineTaiga
                    | Biome::SunflowerPlains
                    | Biome::TallBirchForest
                    | Biome::SparseJungle
                    | Biome::BambooJungle
                    | Biome::ErodedBadlands
                    | Biome::WindsweptSavanna
                    | Biome::CherryGrove
                    | Biome::FrozenPeaks
                    | Biome::DripstoneCaves
                    | Biome::LushCaves
            ),
            "has_structure/trail_ruins" => matches!(
                biome,
                Biome::Taiga
                    | Biome::SnowyTaiga
                    | Biome::OldGrowthPineTaiga
                    | Biome::OldGrowthSpruceTaiga
                    | Biome::TallBirchForest
                    | Biome::Jungle
            ),
            "is_mountain" => matches!(
                biome,
                Biome::Meadow
                    | Biome::FrozenPeaks
                    | Biome::JaggedPeaks
                    | Biome::StonyPeaks
                    | Biome::SnowySlopes
                    | Biome::CherryGrove
            ),
            _ => false,
        };
    }

    Biome::from_name(name_or_tag) == Some(biome)
}

fn resolve_pool_aliases(
    bindings: &[Value],
    start_position: [i32; 3],
    level_seed: i64,
) -> HashMap<String, String> {
    if bindings.is_empty() {
        return HashMap::new();
    }

    let factory = PositionalRandomFactory::new(level_seed);
    let mut random = factory.at(start_position[0], start_position[1], start_position[2]);
    let mut resolved = HashMap::new();
    for binding in bindings {
        resolve_pool_alias_binding(binding, &mut random, &mut resolved);
    }
    resolved
}

fn resolve_pool_alias_binding(
    binding: &Value,
    random: &mut Xoroshiro128,
    resolved: &mut HashMap<String, String>,
) {
    let Some(kind) = binding.get("type").and_then(Value::as_str) else {
        return;
    };
    match kind {
        "minecraft:direct" => {
            let Some(alias) = binding.get("alias").and_then(Value::as_str) else {
                return;
            };
            let Some(target) = binding.get("target").and_then(Value::as_str) else {
                return;
            };
            resolved.insert(
                normalize_registry_name(alias).to_owned(),
                normalize_registry_name(target).to_owned(),
            );
        }
        "minecraft:random" => {
            let Some(alias) = binding.get("alias").and_then(Value::as_str) else {
                return;
            };
            let Some(targets) = binding.get("targets").and_then(Value::as_array) else {
                return;
            };
            if let Some(target) = choose_weighted_value_entry(targets, random) {
                resolved.insert(
                    normalize_registry_name(alias).to_owned(),
                    normalize_registry_name(target).to_owned(),
                );
            }
        }
        "minecraft:random_group" => {
            let Some(groups) = binding.get("groups").and_then(Value::as_array) else {
                return;
            };
            let Some(group) = choose_weighted_group(groups, random) else {
                return;
            };
            if let Some(entries) = group.get("data").and_then(Value::as_array) {
                for entry in entries {
                    resolve_pool_alias_binding(entry, random, resolved);
                }
            }
        }
        _ => {}
    }
}

fn choose_weighted_value_entry<'a>(
    entries: &'a [Value],
    random: &mut Xoroshiro128,
) -> Option<&'a str> {
    let total_weight: i32 = entries
        .iter()
        .map(|entry| entry.get("weight").and_then(Value::as_i64).unwrap_or(1) as i32)
        .sum();
    if total_weight <= 0 {
        return None;
    }

    let mut choice = random.next_int(total_weight as u32) as i32;
    for entry in entries {
        choice -= entry.get("weight").and_then(Value::as_i64).unwrap_or(1) as i32;
        if choice < 0 {
            return entry.get("data").and_then(Value::as_str);
        }
    }

    entries.last()?.get("data").and_then(Value::as_str)
}

fn choose_weighted_group<'a>(groups: &'a [Value], random: &mut Xoroshiro128) -> Option<&'a Value> {
    let total_weight: i32 = groups
        .iter()
        .map(|group| group.get("weight").and_then(Value::as_i64).unwrap_or(1) as i32)
        .sum();
    if total_weight <= 0 {
        return None;
    }

    let mut choice = random.next_int(total_weight as u32) as i32;
    for group in groups {
        choice -= group.get("weight").and_then(Value::as_i64).unwrap_or(1) as i32;
        if choice < 0 {
            return Some(group);
        }
    }

    groups.last()
}

fn expand_structure_pieces(
    plan: &mut PlannedStructure,
    definition: &JigsawStructureDefinition,
    pool_aliases: &HashMap<String, String>,
    random: &mut JavaRandom,
    first_free_height: &mut dyn FnMut(i32, i32) -> i32,
) {
    if definition.max_depth <= 0 {
        return;
    }

    let mut pending = vec![PendingPlacement {
        piece_index: 0,
        depth: 0,
        priority: 0,
        sequence: 0,
    }];
    let mut next_sequence = 1usize;
    let allowed_bounds = BoundingBox3 {
        min: [
            plan.center_position[0] - definition.max_distance_from_center.horizontal,
            (plan.center_position[1] - definition.max_distance_from_center.vertical)
                .max(MIN_Y + definition.dimension_padding),
            plan.center_position[2] - definition.max_distance_from_center.horizontal,
        ],
        max: [
            plan.center_position[0] + definition.max_distance_from_center.horizontal + 1,
            (plan.center_position[1] + definition.max_distance_from_center.vertical + 1)
                .min(319 + 1 - definition.dimension_padding),
            plan.center_position[2] + definition.max_distance_from_center.horizontal + 1,
        ],
    };

    while let Some(state) = pop_next_pending(&mut pending) {
        let source_piece = plan.pieces[state.piece_index].clone();
        let source_jigsaws = element_jigsaw_blocks(
            &source_piece.element,
            source_piece.position,
            source_piece.rotation,
            random,
        );
        let source_box_y = source_piece.bounds.min[1];

        'source_jigsaws: for source_jigsaw in source_jigsaws {
            let source_direction = source_jigsaw.front;
            let target_jigsaw_pos = add_pos(source_jigsaw.pos, source_direction.step());
            let attach_inside_source = source_piece.bounds.contains(target_jigsaw_pos);
            let target_pool_name = resolve_pool_name(pool_aliases, &source_jigsaw.pool);
            let Some(target_pool) = load_template_pool(&target_pool_name) else {
                continue;
            };
            let Some(fallback_pool) = load_template_pool(&target_pool.fallback) else {
                continue;
            };

            let mut target_elements = Vec::new();
            if state.depth != definition.max_depth {
                target_elements.extend(expanded_shuffled_pool_elements(&target_pool, random));
            }
            target_elements.extend(expanded_shuffled_pool_elements(&fallback_pool, random));

            for target_element in target_elements {
                if matches!(target_element, RuntimePoolElement::Empty) {
                    break;
                }

                for target_rotation in shuffled_rotations(random) {
                    let target_jigsaws =
                        element_jigsaw_blocks(&target_element, [0, 0, 0], target_rotation, random);
                    let expand_to = expansion_hack_height(
                        definition,
                        pool_aliases,
                        &target_element,
                        target_rotation,
                        &target_jigsaws,
                    );

                    for target_jigsaw in target_jigsaws {
                        if !jigsaw_blocks_can_attach(&source_jigsaw, &target_jigsaw) {
                            continue;
                        }

                        let raw_target_box_pos = sub_pos(target_jigsaw_pos, target_jigsaw.pos);
                        let Some(raw_target_bounds) = element_bounding_box(
                            &target_element,
                            raw_target_box_pos,
                            target_rotation,
                        ) else {
                            continue;
                        };
                        let raw_target_y = raw_target_bounds.min[1];
                        let source_jigsaw_local_y = source_jigsaw.pos[1] - source_box_y;
                        let target_jigsaw_local_y = target_jigsaw.local_pos[1];
                        let delta_y = source_jigsaw_local_y - target_jigsaw_local_y
                            + source_direction.step()[1];
                        let target_projection = element_projection(&target_element);
                        let source_rigid = source_piece.projection == Projection::Rigid;
                        let target_rigid = target_projection == Projection::Rigid;
                        let target_box_y = if source_rigid && target_rigid {
                            source_box_y + delta_y
                        } else {
                            first_free_height(source_jigsaw.pos[0], source_jigsaw.pos[2])
                                - target_jigsaw_local_y
                        };

                        let y_offset = target_box_y - raw_target_y;
                        let target_position = [
                            raw_target_box_pos[0],
                            raw_target_box_pos[1] + y_offset,
                            raw_target_box_pos[2],
                        ];
                        let Some(mut target_bounds) =
                            element_bounding_box(&target_element, target_position, target_rotation)
                        else {
                            continue;
                        };

                        if expand_to > 0 {
                            let height_delta = target_bounds.max[1] - target_bounds.min[1];
                            let new_size = (expand_to + 1).max(height_delta);
                            target_bounds.max[1] = target_bounds.min[1] + new_size;
                        }

                        if !allowed_bounds.contains_box(target_bounds) {
                            continue;
                        }
                        if planned_piece_intersects_existing(
                            &plan.pieces,
                            target_bounds,
                            attach_inside_source.then_some(state.piece_index),
                        ) {
                            continue;
                        }

                        let ground_level_delta = if target_rigid {
                            source_piece.ground_level_delta - delta_y
                        } else {
                            1
                        };
                        let source_ground_level_delta = source_piece.ground_level_delta;
                        let junction_y = if source_rigid {
                            source_box_y + source_jigsaw_local_y
                        } else if target_rigid {
                            target_box_y + target_jigsaw_local_y
                        } else {
                            first_free_height(source_jigsaw.pos[0], source_jigsaw.pos[2])
                                + delta_y / 2
                        };
                        let piece = PlannedPiece {
                            element: target_element.clone(),
                            projection: target_projection,
                            rotation: target_rotation,
                            position: target_position,
                            ground_level_delta,
                            bounds: target_bounds,
                        };
                        plan.bounds = plan.bounds.union(piece.bounds);
                        plan.junctions.push(BeardJunction {
                            source_x: target_jigsaw_pos[0],
                            source_ground_y: junction_y - source_jigsaw_local_y
                                + source_ground_level_delta,
                            source_z: target_jigsaw_pos[2],
                        });
                        plan.junctions.push(BeardJunction {
                            source_x: source_jigsaw.pos[0],
                            source_ground_y: junction_y - target_jigsaw_local_y
                                + ground_level_delta,
                            source_z: source_jigsaw.pos[2],
                        });
                        plan.pieces.push(piece);
                        if state.depth + 1 <= definition.max_depth {
                            pending.push(PendingPlacement {
                                piece_index: plan.pieces.len() - 1,
                                depth: state.depth + 1,
                                priority: source_jigsaw.placement_priority,
                                sequence: next_sequence,
                            });
                            next_sequence += 1;
                        }
                        continue 'source_jigsaws;
                    }
                }
            }
        }
    }
}

fn pop_next_pending(pending: &mut Vec<PendingPlacement>) -> Option<PendingPlacement> {
    let mut best_index: Option<usize> = None;
    for (index, entry) in pending.iter().enumerate() {
        let is_better = match best_index {
            None => true,
            Some(current) => {
                let best: PendingPlacement = pending[current];
                entry.priority > best.priority
                    || (entry.priority == best.priority && entry.sequence < best.sequence)
            }
        };
        if is_better {
            best_index = Some(index);
        }
    }

    best_index.map(|index| pending.remove(index))
}

fn expansion_hack_height(
    definition: &JigsawStructureDefinition,
    pool_aliases: &HashMap<String, String>,
    target_element: &RuntimePoolElement,
    target_rotation: JigsawRotation,
    target_jigsaws: &[RuntimeJigsawBlock],
) -> i32 {
    if !definition.use_expansion_hack {
        return 0;
    }

    let Some(hack_box) = element_bounding_box(target_element, [0, 0, 0], target_rotation) else {
        return 0;
    };
    if hack_box.max[1] - hack_box.min[1] + 1 > 16 {
        return 0;
    }

    let mut expand_to = 0;
    for target_jigsaw in target_jigsaws {
        let child_pos = add_pos(target_jigsaw.pos, target_jigsaw.front.step());
        if !hack_box.contains(child_pos) {
            continue;
        }

        let child_pool_name = resolve_pool_name(pool_aliases, &target_jigsaw.pool);
        let child_pool_size = template_pool_max_size(&child_pool_name);
        let fallback_pool_size = load_template_pool(&child_pool_name)
            .map(|pool| template_pool_max_size(&pool.fallback))
            .unwrap_or(0);
        expand_to = expand_to.max(child_pool_size.max(fallback_pool_size));
    }

    expand_to
}

fn planned_piece_intersects_existing(
    pieces: &[PlannedPiece],
    target_bounds: BoundingBox3,
    ignore_index: Option<usize>,
) -> bool {
    pieces.iter().enumerate().any(|(index, piece)| {
        if ignore_index == Some(index) {
            return false;
        }
        piece.bounds.intersects(target_bounds)
    })
}

fn structure_within_dimension_padding(plan: &PlannedStructure, padding: i32) -> bool {
    plan.bounds.min[1] >= MIN_Y + padding && plan.bounds.max[1] <= 319 - padding
}

fn resolve_pool_name(pool_aliases: &HashMap<String, String>, pool_name: &str) -> String {
    pool_aliases
        .get(pool_name)
        .cloned()
        .unwrap_or_else(|| pool_name.to_owned())
}

fn expanded_shuffled_pool_elements(
    pool: &ParsedTemplatePool,
    random: &mut JavaRandom,
) -> Vec<RuntimePoolElement> {
    let mut elements = Vec::new();
    for entry in &pool.elements {
        for _ in 0..entry.weight {
            elements.push(entry.element.clone());
        }
    }
    shuffle_slice(&mut elements, random);
    elements
}

fn shuffled_rotations(random: &mut JavaRandom) -> [JigsawRotation; 4] {
    let mut rotations = [
        JigsawRotation::None,
        JigsawRotation::Clockwise90,
        JigsawRotation::Clockwise180,
        JigsawRotation::CounterClockwise90,
    ];
    shuffle_slice(&mut rotations, random);
    rotations
}

fn shuffle_slice<T>(values: &mut [T], random: &mut JavaRandom) {
    let mut index = values.len();
    while index > 1 {
        index -= 1;
        let swap_index = random.next_int((index + 1) as u32) as usize;
        values.swap(index, swap_index);
    }
}

fn element_projection(element: &RuntimePoolElement) -> Projection {
    match element {
        RuntimePoolElement::Empty => Projection::TerrainMatching,
        RuntimePoolElement::Feature { projection, .. }
        | RuntimePoolElement::Single { projection, .. }
        | RuntimePoolElement::List { projection, .. } => projection.clone(),
    }
}

fn element_jigsaw_blocks(
    element: &RuntimePoolElement,
    position: [i32; 3],
    rotation: JigsawRotation,
    random: &mut JavaRandom,
) -> Vec<RuntimeJigsawBlock> {
    match element {
        RuntimePoolElement::Empty => Vec::new(),
        RuntimePoolElement::Feature { .. } => vec![RuntimeJigsawBlock {
            pos: position,
            local_pos: [0, 0, 0],
            front: JigsawFacing::Down,
            top: JigsawFacing::South,
            joint_type: JigsawJointType::Rollable,
            name: "bottom".to_owned(),
            pool: "empty".to_owned(),
            target: "empty".to_owned(),
            placement_priority: 0,
            selection_priority: 0,
        }],
        RuntimePoolElement::Single { template, .. } => load_structure_template(template)
            .map(|template| template_jigsaw_blocks(&template, position, rotation, random))
            .unwrap_or_default(),
        RuntimePoolElement::List { elements, .. } => elements
            .first()
            .map(|first| element_jigsaw_blocks(first, position, rotation, random))
            .unwrap_or_default(),
    }
}

fn template_jigsaw_blocks(
    template: &ParsedStructureTemplate,
    position: [i32; 3],
    rotation: JigsawRotation,
    random: &mut JavaRandom,
) -> Vec<RuntimeJigsawBlock> {
    let mut jigsaws = template
        .blocks
        .iter()
        .filter_map(|block| parse_runtime_jigsaw_block(block, position, rotation))
        .collect::<Vec<_>>();
    shuffle_slice(&mut jigsaws, random);
    jigsaws.sort_by(|left, right| right.selection_priority.cmp(&left.selection_priority));
    jigsaws
}

fn parse_runtime_jigsaw_block(
    block: &ParsedStructureBlock,
    position: [i32; 3],
    rotation: JigsawRotation,
) -> Option<RuntimeJigsawBlock> {
    if normalize_registry_name(&block.state.name) != "jigsaw" {
        return None;
    }

    let orientation = block
        .state
        .properties
        .get("orientation")
        .and_then(|value| parse_front_and_top(value))
        .unwrap_or((JigsawFacing::North, JigsawFacing::Up));
    let front = orientation.0.rotate(rotation);
    let top = orientation.1.rotate(rotation);
    let joint_type = match nbt_string(&block.nbt, "joint") {
        Some("aligned") => JigsawJointType::Aligned,
        Some("rollable") => JigsawJointType::Rollable,
        _ if front.is_horizontal() => JigsawJointType::Aligned,
        _ => JigsawJointType::Rollable,
    };
    let local_pos = block.pos;
    let rotated_pos = rotate_local_pos(local_pos, rotation);

    Some(RuntimeJigsawBlock {
        pos: add_pos(position, rotated_pos),
        local_pos,
        front,
        top,
        joint_type,
        name: nbt_string(&block.nbt, "name").unwrap_or("empty").to_owned(),
        pool: nbt_string(&block.nbt, "pool").unwrap_or("empty").to_owned(),
        target: nbt_string(&block.nbt, "target")
            .unwrap_or("empty")
            .to_owned(),
        placement_priority: block
            .nbt
            .get("placement_priority")
            .and_then(JavaNbtValue::as_i32)
            .unwrap_or(0),
        selection_priority: block
            .nbt
            .get("selection_priority")
            .and_then(JavaNbtValue::as_i32)
            .unwrap_or(0),
    })
}

fn parse_front_and_top(value: &str) -> Option<(JigsawFacing, JigsawFacing)> {
    let (front, top) = value.split_once('_')?;
    Some((JigsawFacing::from_str(front)?, JigsawFacing::from_str(top)?))
}

fn jigsaw_blocks_can_attach(source: &RuntimeJigsawBlock, target: &RuntimeJigsawBlock) -> bool {
    source.front == target.front.opposite()
        && (matches!(source.joint_type, JigsawJointType::Rollable) || source.top == target.top)
        && source.target == target.name
}

fn choose_weighted_pool_element(
    pool_name: &str,
    random: &mut JavaRandom,
) -> Option<RuntimePoolElement> {
    let pool = load_template_pool(pool_name)?;
    if pool.elements.is_empty() {
        return choose_weighted_pool_element(&pool.fallback, random);
    }

    let total_weight: i32 = pool.elements.iter().map(|entry| entry.weight).sum();
    let mut choice = random.next_int(total_weight as u32);
    for entry in &pool.elements {
        choice -= entry.weight;
        if choice < 0 {
            return Some(entry.element.clone());
        }
    }
    pool.elements.last().map(|entry| entry.element.clone())
}

fn add_pos(left: [i32; 3], right: [i32; 3]) -> [i32; 3] {
    [left[0] + right[0], left[1] + right[1], left[2] + right[2]]
}

fn sub_pos(left: [i32; 3], right: [i32; 3]) -> [i32; 3] {
    [left[0] - right[0], left[1] - right[1], left[2] - right[2]]
}

fn template_pool_max_size(name: &str) -> i32 {
    if let Some(cached) = TEMPLATE_POOL_MAX_SIZE_CACHE.read().get(name).copied() {
        return cached;
    }

    let size = load_template_pool(name)
        .map(|pool| {
            pool.elements
                .iter()
                .filter(|entry| !matches!(entry.element, RuntimePoolElement::Empty))
                .filter_map(|entry| {
                    element_bounding_box(&entry.element, [0, 0, 0], JigsawRotation::None)
                })
                .map(|bounds| bounds.max[1] - bounds.min[1] + 1)
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0);
    TEMPLATE_POOL_MAX_SIZE_CACHE
        .write()
        .insert(name.to_owned(), size);
    size
}

fn element_anchor_offset(
    element: &RuntimePoolElement,
    anchor_name: &str,
    rotation: JigsawRotation,
    random: &mut JavaRandom,
) -> Option<[i32; 3]> {
    element_jigsaw_blocks(element, [0, 0, 0], rotation, random)
        .into_iter()
        .find(|jigsaw| jigsaw.name == anchor_name)
        .map(|jigsaw| jigsaw.pos)
}

fn element_bounding_box(
    element: &RuntimePoolElement,
    position: [i32; 3],
    rotation: JigsawRotation,
) -> Option<BoundingBox3> {
    match element {
        RuntimePoolElement::Empty | RuntimePoolElement::Feature { .. } => Some(BoundingBox3 {
            min: position,
            max: position,
        }),
        RuntimePoolElement::Single { template, .. } => {
            let template = load_structure_template(template)?;
            Some(template_bounding_box(template.size, position, rotation))
        }
        RuntimePoolElement::List { elements, .. } => {
            let mut bounds: Option<BoundingBox3> = None;
            for element_entry in elements {
                let entry_bounds = element_bounding_box(element_entry, position, rotation)?;
                bounds = Some(match bounds {
                    Some(current) => current.union(entry_bounds),
                    None => entry_bounds,
                });
            }
            bounds
        }
    }
}

fn place_pool_element(
    chunk: &mut Chunk,
    element: &RuntimePoolElement,
    position: [i32; 3],
    rotation: JigsawRotation,
    level_seed: i64,
    structure_name: &str,
    first_free_height: &mut dyn FnMut(i32, i32) -> i32,
) {
    match element {
        RuntimePoolElement::Empty | RuntimePoolElement::Feature { .. } => {}
        RuntimePoolElement::List { elements, .. } => {
            for entry in elements {
                place_pool_element(
                    chunk,
                    entry,
                    position,
                    rotation,
                    level_seed,
                    structure_name,
                    first_free_height,
                );
            }
        }
        RuntimePoolElement::Single {
            template,
            processors,
            legacy,
            projection,
            ..
        } => {
            let Some(template) = load_structure_template(template) else {
                return;
            };
            for block in &template.blocks {
                let world_offset = rotate_local_pos(block.pos, rotation);
                let world_x = position[0] + world_offset[0];
                let world_z = position[2] + world_offset[2];
                let world_y = match projection {
                    Projection::Rigid => position[1] + world_offset[1],
                    Projection::TerrainMatching => {
                        first_free_height(world_x, world_z) - 1 + block.pos[1]
                    }
                };
                if world_y < MIN_Y || world_y > 319 {
                    continue;
                }

                let local_x = world_x - chunk.x * 16;
                let local_z = world_z - chunk.z * 16;
                if !(0..16).contains(&local_x) || !(0..16).contains(&local_z) {
                    continue;
                }

                let rotated_state = if normalize_registry_name(&block.state.name) == "jigsaw" {
                    block.state.clone()
                } else {
                    rotate_java_block_state(&block.state, rotation)
                };
                let Some(mut state) = apply_single_element_processors(
                    &rotated_state,
                    &block.nbt,
                    processors,
                    *legacy,
                    level_seed,
                    structure_name,
                    world_x,
                    world_y,
                    world_z,
                ) else {
                    continue;
                };
                if *legacy && normalize_registry_name(&state.name) == "air" {
                    continue;
                }

                let runtime_id = resolve_java_block_state(&state);
                let _ = chunk.set_block(local_x as u8, world_y as i16, local_z as u8, runtime_id);
                state.properties.clear();
            }
        }
    }
}

fn apply_single_element_processors(
    block_state: &JavaBlockState,
    block_nbt: &BTreeMap<String, JavaNbtValue>,
    processors: &[RuntimeProcessor],
    legacy: bool,
    level_seed: i64,
    structure_name: &str,
    world_x: i32,
    world_y: i32,
    world_z: i32,
) -> Option<JavaBlockState> {
    let mut state = if normalize_registry_name(&block_state.name) == "jigsaw" {
        let final_state = nbt_string(block_nbt, "final_state").unwrap_or("air");
        parse_java_block_state_string(final_state)
    } else {
        block_state.clone()
    };

    let normalized_state_name = normalize_registry_name(&state.name);
    if normalized_state_name == "structure_void" || normalized_state_name == "structure_block" {
        return None;
    }
    if legacy && normalized_state_name == "air" {
        return None;
    }

    for processor in processors {
        match processor {
            RuntimeProcessor::Noop => {}
            RuntimeProcessor::BlockRot(integrity) => {
                let mut random = JavaRandom::from_seed(block_processing_seed(
                    level_seed,
                    structure_name,
                    world_x,
                    world_y,
                    world_z,
                ));
                if random.next_float() > *integrity {
                    state = JavaBlockState {
                        name: "air".to_owned(),
                        properties: BTreeMap::new(),
                    };
                }
            }
            RuntimeProcessor::Rule(rules) => {
                for rule in rules {
                    if state.name != rule.input_block {
                        continue;
                    }
                    let mut random = JavaRandom::from_seed(block_processing_seed(
                        level_seed,
                        structure_name,
                        world_x,
                        world_y,
                        world_z,
                    ));
                    if random.next_float() <= rule.probability.as_f32() {
                        state = rule.output_state.clone();
                        break;
                    }
                }
            }
        }
    }

    Some(state)
}

fn block_processing_seed(level_seed: i64, structure_name: &str, x: i32, y: i32, z: i32) -> i64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in structure_name.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash ^= x as u64;
    hash = hash.wrapping_mul(0x100000001b3);
    hash ^= y as u64;
    hash = hash.wrapping_mul(0x100000001b3);
    hash ^= z as u64;
    hash = hash.wrapping_mul(0x100000001b3);
    hash ^= level_seed as u64;
    hash as i64
}

fn template_bounding_box(
    size: [i32; 3],
    position: [i32; 3],
    rotation: JigsawRotation,
) -> BoundingBox3 {
    let max_x = size[0] - 1;
    let max_y = size[1] - 1;
    let max_z = size[2] - 1;

    let mut min_corner = [i32::MAX; 3];
    let mut max_corner = [i32::MIN; 3];
    for x in [0, max_x] {
        for y in [0, max_y] {
            for z in [0, max_z] {
                let rotated = rotate_local_pos([x, y, z], rotation);
                let world = [
                    position[0] + rotated[0],
                    position[1] + rotated[1],
                    position[2] + rotated[2],
                ];
                for axis in 0..3 {
                    min_corner[axis] = min_corner[axis].min(world[axis]);
                    max_corner[axis] = max_corner[axis].max(world[axis]);
                }
            }
        }
    }

    BoundingBox3 {
        min: min_corner,
        max: max_corner,
    }
}

fn rotate_local_pos(pos: [i32; 3], rotation: JigsawRotation) -> [i32; 3] {
    match rotation {
        JigsawRotation::None => pos,
        JigsawRotation::Clockwise90 => [-pos[2], pos[1], pos[0]],
        JigsawRotation::Clockwise180 => [-pos[0], pos[1], -pos[2]],
        JigsawRotation::CounterClockwise90 => [pos[2], pos[1], -pos[0]],
    }
}

fn rotate_java_block_state(state: &JavaBlockState, rotation: JigsawRotation) -> JavaBlockState {
    if matches!(rotation, JigsawRotation::None) {
        return state.clone();
    }

    let mut rotated = state.clone();
    if let Some(facing) = state.properties.get("facing") {
        if let Some(direction) = JigsawFacing::from_str(facing) {
            rotated.properties.insert(
                "facing".to_owned(),
                direction.rotate(rotation).as_str().to_owned(),
            );
        }
    }

    if let Some(axis) = state.properties.get("axis") {
        let rotated_axis = match (axis.as_str(), rotation) {
            ("x", JigsawRotation::Clockwise90 | JigsawRotation::CounterClockwise90) => "z",
            ("z", JigsawRotation::Clockwise90 | JigsawRotation::CounterClockwise90) => "x",
            _ => axis,
        };
        rotated
            .properties
            .insert("axis".to_owned(), rotated_axis.to_owned());
    }

    if let Some(orientation) = state.properties.get("orientation") {
        if let Some((front, top)) = parse_front_and_top(orientation) {
            rotated.properties.insert(
                "orientation".to_owned(),
                format!(
                    "{}_{}",
                    front.rotate(rotation).as_str(),
                    top.rotate(rotation).as_str()
                ),
            );
        }
    }

    if let Some(rotation_value) = state.properties.get("rotation") {
        if let Ok(parsed) = rotation_value.parse::<i32>() {
            let delta = match rotation {
                JigsawRotation::None => 0,
                JigsawRotation::Clockwise90 => 4,
                JigsawRotation::Clockwise180 => 8,
                JigsawRotation::CounterClockwise90 => 12,
            };
            rotated.properties.insert(
                "rotation".to_owned(),
                ((parsed + delta).rem_euclid(16)).to_string(),
            );
        }
    }

    let mut wall_sides = None::<BTreeMap<String, String>>;
    for key in ["north", "east", "south", "west"] {
        if state.properties.contains_key(key) {
            let map = wall_sides.get_or_insert_with(BTreeMap::new);
            let source_direction = JigsawFacing::from_str(key).unwrap_or(JigsawFacing::North);
            let rotated_direction = source_direction.rotate(rotation);
            if let Some(value) = state.properties.get(key) {
                map.insert(rotated_direction.as_str().to_owned(), value.clone());
            }
        }
    }
    if let Some(wall_sides) = wall_sides {
        for key in ["north", "east", "south", "west"] {
            rotated.properties.remove(key);
        }
        for (key, value) in wall_sides {
            rotated.properties.insert(key, value);
        }
    }

    rotated
}

fn resolve_java_block_state(state: &JavaBlockState) -> u32 {
    let cache_key = format!("{}{:?}", state.name, state.properties);
    if let Some(runtime_id) = JAVA_STATE_CACHE.read().get(&cache_key).copied() {
        return runtime_id;
    }

    let desired = translate_java_properties(state);
    for candidate_name in candidate_bedrock_names(&state.name) {
        let Some(candidates) = BEDROCK_BLOCK_PALETTE.get(candidate_name.as_str()) else {
            continue;
        };

        let mut best = None::<(&BedrockPaletteEntry, i32)>;
        for candidate in candidates {
            let mut score = 0;
            for (key, desired_value) in &desired {
                match candidate.states.get(key) {
                    Some(actual) if actual == desired_value => score += 8,
                    Some(_) => score -= 32,
                    None => {}
                }
            }
            best = Some(match best {
                Some((best_entry, best_score)) if best_score >= score => (best_entry, best_score),
                _ => (candidate, score),
            });
        }

        if let Some((best_entry, _)) = best {
            let runtime_id = best_entry.runtime_id;
            JAVA_STATE_CACHE.write().insert(cache_key, runtime_id);
            return runtime_id;
        }
    }

    let candidate_names = candidate_bedrock_names(&state.name);
    let fallback_name = format!("minecraft:{}", candidate_names[0]);
    let runtime_id = blocks::get_block_id(&fallback_name);
    JAVA_STATE_CACHE.write().insert(cache_key, runtime_id);
    runtime_id
}

fn translate_java_properties(state: &JavaBlockState) -> BTreeMap<String, BedrockStateScalar> {
    let mut desired = BTreeMap::new();
    for (key, value) in &state.properties {
        match key.as_str() {
            "axis" => {
                desired.insert(
                    "pillar_axis".to_owned(),
                    BedrockStateScalar::String(value.clone()),
                );
            }
            "facing" => {
                desired.insert(
                    "minecraft:cardinal_direction".to_owned(),
                    BedrockStateScalar::String(value.clone()),
                );
                desired.insert(
                    "cardinal_direction".to_owned(),
                    BedrockStateScalar::String(value.clone()),
                );
                if let Some(direction_index) = cardinal_direction_index(value) {
                    desired.insert(
                        "direction".to_owned(),
                        BedrockStateScalar::Int(direction_index),
                    );
                }
            }
            "half" => {
                let upper = matches!(value.as_str(), "top" | "upper");
                desired.insert(
                    "minecraft:vertical_half".to_owned(),
                    BedrockStateScalar::String(if upper { "top" } else { "bottom" }.to_owned()),
                );
                desired.insert(
                    "upside_down_bit".to_owned(),
                    BedrockStateScalar::Byte(upper as i8),
                );
                desired.insert(
                    "upper_block_bit".to_owned(),
                    BedrockStateScalar::Byte(upper as i8),
                );
            }
            "hinge" => {
                desired.insert(
                    "door_hinge_bit".to_owned(),
                    BedrockStateScalar::Byte((value == "right") as i8),
                );
            }
            "open" => {
                desired.insert(
                    "open_bit".to_owned(),
                    BedrockStateScalar::Byte((value == "true") as i8),
                );
            }
            "part" => {
                desired.insert(
                    "head_piece_bit".to_owned(),
                    BedrockStateScalar::Byte((value == "head") as i8),
                );
            }
            "type" => {
                if matches!(value.as_str(), "top" | "bottom") {
                    desired.insert(
                        "minecraft:vertical_half".to_owned(),
                        BedrockStateScalar::String(value.clone()),
                    );
                }
            }
            "east" | "north" | "south" | "west" => {
                desired.insert(
                    format!("wall_connection_type_{key}"),
                    BedrockStateScalar::String(match value.as_str() {
                        "true" | "low" => "short".to_owned(),
                        "tall" => "tall".to_owned(),
                        _ => "none".to_owned(),
                    }),
                );
            }
            "up" => {
                desired.insert(
                    "wall_post_bit".to_owned(),
                    BedrockStateScalar::Byte((value == "true") as i8),
                );
            }
            "attachment" => {
                desired.insert(
                    "attachment".to_owned(),
                    BedrockStateScalar::String(value.clone()),
                );
            }
            "level" => {
                if let Ok(parsed) = value.parse::<i32>() {
                    desired.insert("liquid_depth".to_owned(), BedrockStateScalar::Int(parsed));
                }
            }
            "persistent" => {
                desired.insert(
                    "persistent_bit".to_owned(),
                    BedrockStateScalar::Byte((value == "true") as i8),
                );
            }
            _ => {}
        }
    }
    desired
}

fn candidate_bedrock_names(java_name: &str) -> Vec<String> {
    let normalized = normalize_registry_name(java_name);
    let mut names = vec![normalized.to_owned()];

    if normalized == "wall_torch" {
        names.push("torch".to_owned());
    } else if normalized.ends_with("_bed") {
        names.push("bed".to_owned());
    } else if normalized.ends_with("_wall_banner") {
        names.push("wall_banner".to_owned());
    } else if normalized.ends_with("_banner") {
        names.push("standing_banner".to_owned());
    } else if normalized == "oak_door" {
        names.push("wooden_door".to_owned());
    } else if normalized == "oak_fence" {
        names.push("fence".to_owned());
    } else if normalized == "oak_fence_gate" {
        names.push("fence_gate".to_owned());
    }

    names.into_iter().map(|name| name.to_owned()).collect()
}

fn cardinal_direction_index(direction: &str) -> Option<i32> {
    match direction {
        "south" => Some(0),
        "west" => Some(1),
        "north" => Some(2),
        "east" => Some(3),
        _ => None,
    }
}

fn load_bedrock_block_palette() -> HashMap<String, Vec<BedrockPaletteEntry>> {
    let mut by_name: HashMap<String, Vec<BedrockPaletteEntry>> = HashMap::new();
    let data = jolyne::valentine::block_palette::BLOCK_PALETTE_NBT;
    let mut reader = BedrockNbtReader::new(data);
    let mut runtime_id = 0u32;

    while !reader.is_eof() {
        let Some(entry) = reader.read_root_compound() else {
            break;
        };
        let name = entry
            .get("name")
            .and_then(JavaNbtValue::as_str)
            .unwrap_or("minecraft:air");
        let states = entry
            .get("states")
            .and_then(JavaNbtValue::as_compound)
            .map(|compound| {
                compound
                    .iter()
                    .filter_map(|(key, value)| match value {
                        JavaNbtValue::Byte(v) => Some((key.clone(), BedrockStateScalar::Byte(*v))),
                        JavaNbtValue::Int(v) => Some((key.clone(), BedrockStateScalar::Int(*v))),
                        JavaNbtValue::String(v) => {
                            Some((key.clone(), BedrockStateScalar::String(v.clone())))
                        }
                        _ => None,
                    })
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default();

        by_name
            .entry(normalize_registry_name(name).to_owned())
            .or_default()
            .push(BedrockPaletteEntry {
                runtime_id,
                name: normalize_registry_name(name).to_owned(),
                states,
            });
        runtime_id += 1;
    }

    by_name
}

fn load_template_pool(name: &str) -> Option<Arc<ParsedTemplatePool>> {
    if let Some(cached) = TEMPLATE_POOL_CACHE.read().get(name).cloned() {
        return Some(cached);
    }

    let raw = StructureRuntimeRegistry::global().template_pool(name)?;
    let mut raw_pool_value = serde_json::Map::new();
    raw_pool_value.insert("fallback".to_owned(), Value::String(raw.fallback.clone()));
    raw_pool_value.insert("elements".to_owned(), Value::Array(raw.elements.clone()));
    let raw_pool: RawTemplatePool = serde_json::from_value(Value::Object(raw_pool_value)).ok()?;

    let parsed = Arc::new(ParsedTemplatePool {
        fallback: normalize_registry_name(&raw_pool.fallback).to_owned(),
        elements: raw_pool
            .elements
            .into_iter()
            .map(|entry| WeightedPoolElement {
                element: parse_pool_element(entry.element),
                weight: entry.weight,
            })
            .collect(),
    });
    TEMPLATE_POOL_CACHE
        .write()
        .insert(name.to_owned(), parsed.clone());
    Some(parsed)
}

fn parse_pool_element(element: RawPoolElement) -> RuntimePoolElement {
    match element {
        RawPoolElement::Empty => RuntimePoolElement::Empty,
        RawPoolElement::Feature {
            feature,
            projection,
        } => RuntimePoolElement::Feature {
            feature: normalize_registry_name(&feature).to_owned(),
            projection,
        },
        RawPoolElement::Single {
            location,
            processors,
            projection,
        } => RuntimePoolElement::Single {
            template: normalize_registry_name(&location).to_owned(),
            processors: parse_processors(processors),
            projection,
            legacy: false,
        },
        RawPoolElement::LegacySingle {
            location,
            processors,
            projection,
        } => RuntimePoolElement::Single {
            template: normalize_registry_name(&location).to_owned(),
            processors: parse_processors(processors),
            projection,
            legacy: true,
        },
        RawPoolElement::List {
            elements,
            projection,
        } => RuntimePoolElement::List {
            elements: elements.into_iter().map(parse_pool_element).collect(),
            projection,
        },
    }
}

fn parse_processors(value: Value) -> Vec<RuntimeProcessor> {
    let list_value = match value {
        Value::String(name) => StructureRuntimeRegistry::global()
            .processor_list(normalize_registry_name(&name))
            .cloned()
            .unwrap_or_else(|| {
                let mut empty = serde_json::Map::new();
                empty.insert("processors".to_owned(), Value::Array(Vec::new()));
                Value::Object(empty)
            }),
        inline => inline,
    };

    let Ok(parsed) = serde_json::from_value::<RawProcessorList>(list_value) else {
        return Vec::new();
    };

    parsed
        .processors
        .into_iter()
        .map(|processor| {
            let kind = processor
                .get("processor_type")
                .and_then(Value::as_str)
                .unwrap_or_default();
            match kind {
                "minecraft:block_rot" => processor
                    .get("integrity")
                    .and_then(Value::as_f64)
                    .map(|value| RuntimeProcessor::BlockRot(value as f32))
                    .unwrap_or(RuntimeProcessor::Noop),
                "minecraft:rule" => RuntimeProcessor::Rule(parse_rule_processor(processor)),
                _ => RuntimeProcessor::Noop,
            }
        })
        .collect()
}

fn parse_rule_processor(processor: Value) -> Vec<RuleTransform> {
    processor
        .get("rules")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|rule| {
            let input_block = rule
                .get("input_predicate")
                .and_then(Value::as_object)
                .and_then(|predicate| predicate.get("block"))
                .and_then(Value::as_str)?
                .to_owned();
            let probability = rule
                .get("input_predicate")
                .and_then(Value::as_object)
                .and_then(|predicate| predicate.get("probability"))
                .and_then(Value::as_f64)
                .unwrap_or(1.0) as f32;
            let output_state = rule
                .get("output_state")
                .and_then(Value::as_object)
                .and_then(|state| {
                    let name = state.get("Name").and_then(Value::as_str)?;
                    let properties = state
                        .get("Properties")
                        .and_then(Value::as_object)
                        .map(|properties| {
                            properties
                                .iter()
                                .filter_map(|(key, value)| {
                                    value
                                        .as_str()
                                        .map(|value_str| (key.clone(), value_str.to_owned()))
                                })
                                .collect::<BTreeMap<_, _>>()
                        })
                        .unwrap_or_default();
                    Some(JavaBlockState {
                        name: normalize_registry_name(name).to_owned(),
                        properties,
                    })
                })?;

            Some(RuleTransform {
                input_block: normalize_registry_name(&input_block).to_owned(),
                probability: ProbabilityKey::from_f32(probability),
                output_state,
            })
        })
        .collect()
}

fn load_structure_template(name: &str) -> Option<Arc<ParsedStructureTemplate>> {
    if let Some(cached) = STRUCTURE_TEMPLATE_CACHE.read().get(name).cloned() {
        return Some(cached);
    }

    let bytes = StructureRuntimeRegistry::global().structure_template(name)?;
    let parsed = Arc::new(parse_structure_template(bytes)?);
    STRUCTURE_TEMPLATE_CACHE
        .write()
        .insert(name.to_owned(), parsed.clone());
    Some(parsed)
}

fn parse_structure_template(compressed: &[u8]) -> Option<ParsedStructureTemplate> {
    let mut decoder = GzDecoder::new(compressed);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed).ok()?;

    let mut reader = JavaNbtReader::new(&decompressed);
    let root = reader.read_root_compound()?;
    let size = parse_size(root.get("size")?)?;

    let palette = if let Some(values) = root.get("palette").and_then(JavaNbtValue::as_list) {
        parse_java_palette(values)?
    } else {
        let first_palette = root
            .get("palettes")
            .and_then(JavaNbtValue::as_list)?
            .first()?
            .as_list()?;
        parse_java_palette(first_palette)?
    };

    let blocks = root
        .get("blocks")
        .and_then(JavaNbtValue::as_list)?
        .iter()
        .filter_map(|value| parse_structure_block(value, &palette))
        .collect();

    Some(ParsedStructureTemplate { size, blocks })
}

fn parse_size(value: &JavaNbtValue) -> Option<[i32; 3]> {
    let list = value.as_list()?;
    Some([
        list.first()?.as_i32()?,
        list.get(1)?.as_i32()?,
        list.get(2)?.as_i32()?,
    ])
}

fn parse_java_palette(values: &[JavaNbtValue]) -> Option<Vec<JavaBlockState>> {
    values
        .iter()
        .map(|value| {
            let compound = value.as_compound()?;
            let name = compound.get("Name")?.as_str()?.to_owned();
            let properties = compound
                .get("Properties")
                .and_then(JavaNbtValue::as_compound)
                .map(|properties| {
                    properties
                        .iter()
                        .filter_map(|(key, value)| {
                            value
                                .as_str()
                                .map(|value_str| (key.clone(), value_str.to_owned()))
                        })
                        .collect::<BTreeMap<_, _>>()
                })
                .unwrap_or_default();
            Some(JavaBlockState {
                name: normalize_registry_name(&name).to_owned(),
                properties,
            })
        })
        .collect()
}

fn parse_structure_block(
    value: &JavaNbtValue,
    palette: &[JavaBlockState],
) -> Option<ParsedStructureBlock> {
    let compound = value.as_compound()?;
    let pos_value = compound.get("pos")?.as_list()?;
    let state_index = compound.get("state")?.as_i32()? as usize;
    let state = palette.get(state_index)?.clone();
    let nbt = compound
        .get("nbt")
        .and_then(JavaNbtValue::as_compound)
        .cloned()
        .unwrap_or_default();

    Some(ParsedStructureBlock {
        pos: [
            pos_value.first()?.as_i32()?,
            pos_value.get(1)?.as_i32()?,
            pos_value.get(2)?.as_i32()?,
        ],
        state,
        nbt,
    })
}

fn parse_java_block_state_string(value: &str) -> JavaBlockState {
    let (name, property_string) = match value.split_once('[') {
        Some((name, rest)) => (name, Some(rest.trim_end_matches(']'))),
        None => (value, None),
    };

    let mut properties = BTreeMap::new();
    if let Some(property_string) = property_string {
        for entry in property_string.split(',') {
            let Some((key, entry_value)) = entry.split_once('=') else {
                continue;
            };
            properties.insert(key.trim().to_owned(), entry_value.trim().to_owned());
        }
    }

    JavaBlockState {
        name: normalize_registry_name(name).to_owned(),
        properties,
    }
}

fn nbt_string<'a>(nbt: &'a BTreeMap<String, JavaNbtValue>, key: &str) -> Option<&'a str> {
    nbt.get(key)
        .and_then(JavaNbtValue::as_str)
        .map(normalize_registry_name)
}

fn normalize_registry_name(name: &str) -> &str {
    name.strip_prefix("minecraft:").unwrap_or(name)
}

fn set_large_feature_seed(random: &mut JavaRandom, seed: i64, chunk_x: i32, chunk_z: i32) {
    random.set_seed(seed);
    let x_scale = random.next_long();
    let z_scale = random.next_long();
    let result =
        (chunk_x as i64).wrapping_mul(x_scale) ^ (chunk_z as i64).wrapping_mul(z_scale) ^ seed;
    random.set_seed(result);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BoundingBox3 {
    min: [i32; 3],
    max: [i32; 3],
}

impl BoundingBox3 {
    fn union(self, other: Self) -> Self {
        let mut min = [0; 3];
        let mut max = [0; 3];
        for axis in 0..3 {
            min[axis] = self.min[axis].min(other.min[axis]);
            max[axis] = self.max[axis].max(other.max[axis]);
        }
        Self { min, max }
    }

    fn contains(self, pos: [i32; 3]) -> bool {
        (0..3).all(|axis| pos[axis] >= self.min[axis] && pos[axis] <= self.max[axis])
    }

    fn contains_box(self, other: Self) -> bool {
        (0..3).all(|axis| other.min[axis] >= self.min[axis] && other.max[axis] <= self.max[axis])
    }

    fn intersects(self, other: Self) -> bool {
        (0..3).all(|axis| self.min[axis] <= other.max[axis] && self.max[axis] >= other.min[axis])
    }

    fn close_to_chunk(self, chunk_x: i32, chunk_z: i32, padding: i32) -> bool {
        let chunk_min_x = chunk_x * 16 - padding;
        let chunk_max_x = chunk_x * 16 + 15 + padding;
        let chunk_min_z = chunk_z * 16 - padding;
        let chunk_max_z = chunk_z * 16 + 15 + padding;
        self.max[0] >= chunk_min_x
            && self.min[0] <= chunk_max_x
            && self.max[2] >= chunk_min_z
            && self.min[2] <= chunk_max_z
    }
}

struct JavaNbtReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> JavaNbtReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn read_root_compound(&mut self) -> Option<BTreeMap<String, JavaNbtValue>> {
        if self.read_u8()? != 10 {
            return None;
        }
        let _ = self.read_string()?;
        match self.read_payload(10)? {
            JavaNbtValue::Compound(compound) => Some(compound),
            _ => None,
        }
    }

    fn read_payload(&mut self, tag: u8) -> Option<JavaNbtValue> {
        match tag {
            1 => Some(JavaNbtValue::Byte(self.read_i8()?)),
            2 => Some(JavaNbtValue::Short(self.read_i16()?)),
            3 => Some(JavaNbtValue::Int(self.read_i32()?)),
            4 => Some(JavaNbtValue::Long(self.read_i64()?)),
            5 => Some(JavaNbtValue::Float(self.read_u32()?)),
            6 => Some(JavaNbtValue::Double(self.read_u64()?)),
            7 => {
                let length = self.read_i32()? as usize;
                let bytes = self.data.get(self.pos..self.pos + length)?.to_vec();
                self.pos += length;
                Some(JavaNbtValue::ByteArray(bytes))
            }
            8 => Some(JavaNbtValue::String(self.read_string()?)),
            9 => {
                let inner = self.read_u8()?;
                let count = self.read_i32()? as usize;
                let mut values = Vec::with_capacity(count);
                for _ in 0..count {
                    values.push(self.read_payload(inner)?);
                }
                Some(JavaNbtValue::List(values))
            }
            10 => {
                let mut values = BTreeMap::new();
                loop {
                    let inner = self.read_u8()?;
                    if inner == 0 {
                        break;
                    }
                    let name = self.read_string()?;
                    let value = self.read_payload(inner)?;
                    values.insert(name, value);
                }
                Some(JavaNbtValue::Compound(values))
            }
            11 => {
                let count = self.read_i32()? as usize;
                let mut values = Vec::with_capacity(count);
                for _ in 0..count {
                    values.push(self.read_i32()?);
                }
                Some(JavaNbtValue::IntArray(values))
            }
            12 => {
                let count = self.read_i32()? as usize;
                let mut values = Vec::with_capacity(count);
                for _ in 0..count {
                    values.push(self.read_i64()?);
                }
                Some(JavaNbtValue::LongArray(values))
            }
            _ => None,
        }
    }

    fn read_u8(&mut self) -> Option<u8> {
        let value = *self.data.get(self.pos)?;
        self.pos += 1;
        Some(value)
    }

    fn read_i8(&mut self) -> Option<i8> {
        Some(self.read_u8()? as i8)
    }

    fn read_i32(&mut self) -> Option<i32> {
        let bytes = self.data.get(self.pos..self.pos + 4)?;
        self.pos += 4;
        Some(i32::from_be_bytes(bytes.try_into().ok()?))
    }

    fn read_i16(&mut self) -> Option<i16> {
        let bytes = self.data.get(self.pos..self.pos + 2)?;
        self.pos += 2;
        Some(i16::from_be_bytes(bytes.try_into().ok()?))
    }

    fn read_u32(&mut self) -> Option<u32> {
        let bytes = self.data.get(self.pos..self.pos + 4)?;
        self.pos += 4;
        Some(u32::from_be_bytes(bytes.try_into().ok()?))
    }

    fn read_i64(&mut self) -> Option<i64> {
        let bytes = self.data.get(self.pos..self.pos + 8)?;
        self.pos += 8;
        Some(i64::from_be_bytes(bytes.try_into().ok()?))
    }

    fn read_u64(&mut self) -> Option<u64> {
        let bytes = self.data.get(self.pos..self.pos + 8)?;
        self.pos += 8;
        Some(u64::from_be_bytes(bytes.try_into().ok()?))
    }

    fn read_string(&mut self) -> Option<String> {
        let length_bytes = self.data.get(self.pos..self.pos + 2)?;
        self.pos += 2;
        let length = u16::from_be_bytes(length_bytes.try_into().ok()?) as usize;
        let bytes = self.data.get(self.pos..self.pos + length)?;
        self.pos += length;
        Some(String::from_utf8_lossy(bytes).into_owned())
    }
}

struct BedrockNbtReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> BedrockNbtReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn is_eof(&self) -> bool {
        self.pos >= self.data.len()
    }

    fn read_root_compound(&mut self) -> Option<BTreeMap<String, JavaNbtValue>> {
        if self.is_eof() || self.read_u8()? != 10 {
            return None;
        }
        let _ = self.read_var_string()?;
        match self.read_payload(10)? {
            JavaNbtValue::Compound(compound) => Some(compound),
            _ => None,
        }
    }

    fn read_payload(&mut self, tag: u8) -> Option<JavaNbtValue> {
        match tag {
            1 => Some(JavaNbtValue::Byte(self.read_i8()?)),
            3 => Some(JavaNbtValue::Int(self.read_zigzag_i32()?)),
            8 => Some(JavaNbtValue::String(self.read_var_string()?)),
            10 => {
                let mut values = BTreeMap::new();
                loop {
                    let inner = self.read_u8()?;
                    if inner == 0 {
                        break;
                    }
                    let name = self.read_var_string()?;
                    let value = self.read_payload(inner)?;
                    values.insert(name, value);
                }
                Some(JavaNbtValue::Compound(values))
            }
            _ => None,
        }
    }

    fn read_u8(&mut self) -> Option<u8> {
        let value = *self.data.get(self.pos)?;
        self.pos += 1;
        Some(value)
    }

    fn read_i8(&mut self) -> Option<i8> {
        Some(self.read_u8()? as i8)
    }

    fn read_var_u32(&mut self) -> Option<u32> {
        let mut result = 0u32;
        let mut shift = 0;
        loop {
            let byte = self.read_u8()?;
            result |= u32::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                break;
            }
            shift += 7;
        }
        Some(result)
    }

    fn read_zigzag_i32(&mut self) -> Option<i32> {
        let value = self.read_var_u32()?;
        Some(((value >> 1) as i32) ^ (-((value & 1) as i32)))
    }

    fn read_var_string(&mut self) -> Option<String> {
        let length = self.read_var_u32()? as usize;
        let bytes = self.data.get(self.pos..self.pos + length)?;
        self.pos += length;
        Some(String::from_utf8_lossy(bytes).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn loads_jigsaw_structure_definitions_from_registry() {
        let village = load_jigsaw_structure_definition("village_plains").expect("village_plains");
        assert_eq!(village.start_pool, "village/plains/town_centers");
        assert_eq!(village.max_depth, 6);
        assert_eq!(
            village.project_start_to_heightmap.as_deref(),
            Some("WORLD_SURFACE_WG")
        );
        assert_eq!(village.terrain_adaptation.as_deref(), Some("beard_thin"));
        assert_eq!(village.dimension_padding, 0);
    }

    #[test]
    fn parses_structure_templates_and_exposes_jigsaw_blocks() {
        let template =
            load_structure_template("village/plains/town_centers/plains_meeting_point_1")
                .expect("missing template");
        assert_eq!(template.size, [10, 7, 10]);
        assert!(template.blocks.iter().any(|block| {
            normalize_registry_name(&block.state.name) == "jigsaw"
                && nbt_string(&block.nbt, "pool") == Some("village/plains/streets")
        }));
    }

    #[test]
    fn resolves_non_default_bedrock_states_for_java_templates() {
        let runtime_id = resolve_java_block_state(&JavaBlockState {
            name: "oak_log".to_owned(),
            properties: BTreeMap::from([("axis".to_owned(), "x".to_owned())]),
        });
        assert_ne!(runtime_id, blocks::get_block_id("minecraft:oak_log"));
    }

    #[test]
    fn rotate_local_pos_matches_java_transform() {
        assert_eq!(
            rotate_local_pos([3, 5, 7], JigsawRotation::Clockwise90),
            [-7, 5, 3]
        );
        assert_eq!(
            rotate_local_pos([3, 5, 7], JigsawRotation::CounterClockwise90),
            [7, 5, -3]
        );
    }

    #[test]
    fn village_plan_expands_beyond_the_root_piece() {
        let definition =
            load_jigsaw_structure_definition("village_plains").expect("village_plains");
        let mut random = JavaRandom::from_seed(1);
        set_large_feature_seed(&mut random, 1, -11, -28);
        let mut surface_height = |_x: i32, _z: i32| 80;
        let aliases = HashMap::new();
        let mut plan = plan_root_piece(
            "village_plains",
            &definition,
            -11,
            -28,
            &mut random,
            &mut surface_height,
        )
        .expect("village root");
        expand_structure_pieces(
            &mut plan,
            &definition,
            &aliases,
            &mut random,
            &mut surface_height,
        );

        assert!(
            plan.pieces.len() > 1,
            "expected recursive village child pieces"
        );
        let min_chunk_x = plan.bounds.min[0].div_euclid(16);
        let max_chunk_x = plan.bounds.max[0].div_euclid(16);
        let min_chunk_z = plan.bounds.min[2].div_euclid(16);
        let max_chunk_z = plan.bounds.max[2].div_euclid(16);
        assert!(
            min_chunk_x != -11 || max_chunk_x != -11 || min_chunk_z != -28 || max_chunk_z != -28,
            "expected the expanded village to span neighboring chunks"
        );
    }

    #[test]
    fn village_beardifier_collects_rigid_piece_data() {
        let (chunk_x, chunk_z, _) = find_test_village_chunk(1);
        let biome_noise = BiomeNoise::from_seed(1);
        let mut surface_height = |_x: i32, _z: i32| 80;
        let beardifier = build_overworld_jigsaw_beardifier(
            1,
            chunk_x,
            chunk_z,
            &biome_noise,
            &mut surface_height,
        );
        assert!(
            !beardifier.is_empty(),
            "expected terrain adaptation data for villages"
        );
    }

    #[test]
    fn places_village_structure_blocks_into_candidate_chunk() {
        let (chunk_x, chunk_z, plan) = find_test_village_chunk(1);
        let mut chunk = Chunk::new(chunk_x, chunk_z);
        chunk.fill_floor(80, *blocks::STONE);
        let biome_noise = BiomeNoise::from_seed(1);
        let mut surface_height = |_x: i32, _z: i32| 80;
        place_overworld_jigsaw_structure_starts(
            &mut chunk,
            1,
            chunk_x,
            chunk_z,
            &biome_noise,
            &mut surface_height,
        );

        let mut found_structure_block = false;
        let min_y = plan.bounds.min[1].max(MIN_Y);
        let max_y = plan.bounds.max[1].min(319);
        for y in min_y..=max_y {
            for z in 0u8..16 {
                for x in 0u8..16 {
                    let block = chunk.get_block(x, y as i16, z);
                    if block != *blocks::AIR && block != *blocks::STONE {
                        found_structure_block = true;
                        break;
                    }
                }
            }
        }
        assert!(
            found_structure_block,
            "expected village start blocks above the filled floor"
        );
    }

    fn find_test_village_chunk(seed: i64) -> (i32, i32, PlannedStructure) {
        let biome_noise = BiomeNoise::from_seed(seed);
        let mut surface_height = |_x: i32, _z: i32| 80;

        for radius in 0..=64 {
            for chunk_z in -radius..=radius {
                for chunk_x in -radius..=radius {
                    if !structure_set_candidate_chunk("villages", seed, chunk_x, chunk_z) {
                        continue;
                    }
                    let Some(plan) = plan_structure_start_for_set(
                        "villages",
                        seed,
                        chunk_x,
                        chunk_z,
                        &biome_noise,
                        &mut surface_height,
                    ) else {
                        continue;
                    };
                    let target_piece = plan
                        .pieces
                        .iter()
                        .find(|piece| piece.projection == Projection::Rigid)
                        .unwrap_or(&plan.pieces[0]);
                    let target_chunk_x = target_piece.position[0].div_euclid(16);
                    let target_chunk_z = target_piece.position[2].div_euclid(16);
                    return (target_chunk_x, target_chunk_z, plan);
                }
            }
        }

        panic!("expected to find a village start for test seed {seed}");
    }
}
