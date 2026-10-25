//! Typed IR for Java `worldgen/structure_set`, `worldgen/structure`,
//! `worldgen/template_pool` and `worldgen/processor_list`.
//!
//! Like the feature IR, every discriminator is a closed enum and every object
//! denies unknown fields, so new or unknown data fails the build.

use super::feature::{
    BlockHolderSet, BlockStateJson, HeightProvider, HeightmapKind, IntProvider, RuleTest, TagRef,
};
use serde::de::{DeserializeOwned, Error as _};
use serde::{Deserialize, Deserializer};
use std::collections::BTreeMap;
use std::path::Path;

// ---------------------------------------------------------------------------
// Processors
// ---------------------------------------------------------------------------

/// Java `PosRuleTest`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "predicate_type", deny_unknown_fields)]
pub enum PosRuleTest {
    #[serde(rename = "minecraft:always_true")]
    AlwaysTrue {},
    #[serde(rename = "minecraft:linear_pos")]
    LinearPos {
        #[serde(default)]
        min_chance: f32,
        #[serde(default)]
        max_chance: f32,
        #[serde(default)]
        min_dist: i32,
        #[serde(default)]
        max_dist: i32,
    },
    #[serde(rename = "minecraft:axis_aligned_linear_pos")]
    AxisAlignedLinearPos {
        #[serde(default = "axis_y")]
        axis: String,
        #[serde(default)]
        min_chance: f32,
        #[serde(default)]
        max_chance: f32,
        #[serde(default)]
        min_dist: i32,
        #[serde(default)]
        max_dist: i32,
    },
}

fn axis_y() -> String {
    "y".to_string()
}

/// Java `RuleBlockEntityModifier`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum BlockEntityModifier {
    #[serde(rename = "minecraft:append_loot")]
    AppendLoot { loot_table: String },
    #[serde(rename = "minecraft:clear")]
    Clear {},
    #[serde(rename = "minecraft:passthrough")]
    Passthrough {},
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessorRule {
    pub input_predicate: RuleTest,
    pub location_predicate: RuleTest,
    #[serde(default)]
    pub position_predicate: Option<PosRuleTest>,
    pub output_state: BlockStateJson,
    #[serde(default)]
    pub block_entity_modifier: Option<BlockEntityModifier>,
}

/// Java `StructureProcessor` (data-driven ones).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "processor_type", deny_unknown_fields)]
pub enum Processor {
    #[serde(rename = "minecraft:block_rot")]
    BlockRot {
        integrity: f32,
        #[serde(default)]
        rottable_blocks: Option<BlockHolderSet>,
    },
    #[serde(rename = "minecraft:rule")]
    Rule { rules: Vec<ProcessorRule> },
    #[serde(rename = "minecraft:capped")]
    Capped {
        delegate: Box<Processor>,
        limit: IntProvider,
    },
    #[serde(rename = "minecraft:protected_blocks")]
    ProtectedBlocks { value: TagRef },
    #[serde(rename = "minecraft:block_ignore")]
    BlockIgnore { blocks: Vec<BlockStateJson> },
    #[serde(rename = "minecraft:gravity")]
    Gravity {
        #[serde(default = "wsg")]
        heightmap: HeightmapKind,
        #[serde(default)]
        offset: i32,
    },
    #[serde(rename = "minecraft:jigsaw_replacement")]
    JigsawReplacement {},
    #[serde(rename = "minecraft:nop")]
    Nop {},
    #[serde(rename = "minecraft:blackstone_replace")]
    BlackstoneReplace {},
    #[serde(rename = "minecraft:lava_submerged_block")]
    LavaSubmergedBlock {},
    #[serde(rename = "minecraft:block_age")]
    BlockAge { mossiness: f32 },
}

fn wsg() -> HeightmapKind {
    HeightmapKind::WorldSurfaceWg
}

/// `processor_list` file body.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessorList {
    pub processors: Vec<Processor>,
}

/// A processor list: registry reference or inline.
#[derive(Debug, Clone, PartialEq)]
pub enum ProcessorListRef {
    Ref(String),
    Inline(ProcessorList),
}

impl<'de> Deserialize<'de> for ProcessorListRef {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        match v {
            serde_json::Value::String(s) => Ok(ProcessorListRef::Ref(s)),
            other => serde_json::from_value(other)
                .map(ProcessorListRef::Inline)
                .map_err(D::Error::custom),
        }
    }
}

// ---------------------------------------------------------------------------
// Template pools
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Projection {
    Rigid,
    TerrainMatching,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiquidSettings {
    IgnoreWaterlogging,
    ApplyWaterlogging,
}

/// Java `StructurePoolElement`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "element_type", deny_unknown_fields)]
pub enum PoolElement {
    #[serde(rename = "minecraft:single_pool_element")]
    Single {
        location: String,
        processors: ProcessorListRef,
        projection: Projection,
        #[serde(default)]
        override_liquid_settings: Option<LiquidSettings>,
    },
    #[serde(rename = "minecraft:legacy_single_pool_element")]
    LegacySingle {
        location: String,
        processors: ProcessorListRef,
        projection: Projection,
        #[serde(default)]
        override_liquid_settings: Option<LiquidSettings>,
    },
    #[serde(rename = "minecraft:feature_pool_element")]
    Feature {
        feature: String,
        projection: Projection,
    },
    #[serde(rename = "minecraft:list_pool_element")]
    List {
        elements: Vec<PoolElement>,
        projection: Projection,
    },
    #[serde(rename = "minecraft:empty_pool_element")]
    Empty {},
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeightedElement {
    pub element: PoolElement,
    pub weight: i32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplatePool {
    pub fallback: String,
    pub elements: Vec<WeightedElement>,
}

// ---------------------------------------------------------------------------
// Structures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerrainAdjustment {
    None,
    Bury,
    BeardThin,
    BeardBox,
    Encapsulate,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnEntry {
    #[serde(rename = "type")]
    pub entity: String,
    pub weight: i32,
    #[serde(rename = "minCount")]
    pub min_count: i32,
    #[serde(rename = "maxCount")]
    pub max_count: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpawnBox {
    Piece,
    Structure,
    Full,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnOverride {
    pub bounding_box: SpawnBox,
    pub spawns: Vec<SpawnEntry>,
}

/// Fields shared by every structure type (`Structure.StructureSettings`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
// No `deny_unknown_fields`: this struct is flattened into the typed variants,
// and `parse_all` checks every structure file's keys instead.
pub struct StructureCommon {
    pub biomes: String,
    pub step: String,
    #[serde(default)]
    pub spawn_overrides: BTreeMap<String, SpawnOverride>,
    #[serde(default = "no_adjust")]
    pub terrain_adaptation: TerrainAdjustment,
}

fn no_adjust() -> TerrainAdjustment {
    TerrainAdjustment::None
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum PoolAlias {
    #[serde(rename = "minecraft:direct")]
    Direct { alias: String, target: String },
    #[serde(rename = "minecraft:random")]
    Random {
        alias: String,
        targets: Vec<WeightedString>,
    },
    #[serde(rename = "minecraft:random_group")]
    RandomGroup { groups: Vec<WeightedAliasGroup> },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeightedString {
    pub data: String,
    pub weight: i32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeightedAliasGroup {
    pub data: Vec<PoolAlias>,
    pub weight: i32,
}

/// `DimensionPadding`: a number (both sides) or `{bottom, top}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DimensionPadding {
    pub bottom: i32,
    pub top: i32,
}

impl<'de> Deserialize<'de> for DimensionPadding {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Obj {
            #[serde(default)]
            bottom: i32,
            #[serde(default)]
            top: i32,
        }
        let v = serde_json::Value::deserialize(d)?;
        if let Some(n) = v.as_i64() {
            return Ok(DimensionPadding {
                bottom: n as i32,
                top: n as i32,
            });
        }
        let o: Obj = serde_json::from_value(v).map_err(D::Error::custom)?;
        Ok(DimensionPadding {
            bottom: o.bottom,
            top: o.top,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuinedPortalSetup {
    pub placement: String,
    pub air_pocket_probability: f32,
    pub mossiness: f32,
    pub overgrown: bool,
    pub vines: bool,
    pub can_be_cold: bool,
    pub replace_with_blackstone: bool,
    pub weight: f32,
}

/// Java `Structure` (one variant per structure type).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type")]
pub enum Structure {
    #[serde(rename = "minecraft:jigsaw")]
    Jigsaw(JigsawStructure),
    #[serde(rename = "minecraft:buried_treasure")]
    BuriedTreasure(StructureCommon),
    #[serde(rename = "minecraft:desert_pyramid")]
    DesertPyramid(StructureCommon),
    #[serde(rename = "minecraft:end_city")]
    EndCity(StructureCommon),
    #[serde(rename = "minecraft:fortress")]
    Fortress(StructureCommon),
    #[serde(rename = "minecraft:igloo")]
    Igloo(StructureCommon),
    #[serde(rename = "minecraft:jungle_temple")]
    JungleTemple(StructureCommon),
    #[serde(rename = "minecraft:mineshaft")]
    Mineshaft(MineshaftStructure),
    #[serde(rename = "minecraft:nether_fossil")]
    NetherFossil(NetherFossilStructure),
    #[serde(rename = "minecraft:ocean_monument")]
    OceanMonument(StructureCommon),
    #[serde(rename = "minecraft:ocean_ruin")]
    OceanRuin(OceanRuinStructure),
    #[serde(rename = "minecraft:ruined_portal")]
    RuinedPortal(RuinedPortalStructure),
    #[serde(rename = "minecraft:shipwreck")]
    Shipwreck(ShipwreckStructure),
    #[serde(rename = "minecraft:stronghold")]
    Stronghold(StructureCommon),
    #[serde(rename = "minecraft:swamp_hut")]
    SwampHut(StructureCommon),
    #[serde(rename = "minecraft:woodland_mansion")]
    WoodlandMansion(StructureCommon),
}

impl Structure {
    pub fn common(&self) -> &StructureCommon {
        match self {
            Structure::Jigsaw(s) => &s.common,
            Structure::Mineshaft(s) => &s.common,
            Structure::NetherFossil(s) => &s.common,
            Structure::OceanRuin(s) => &s.common,
            Structure::RuinedPortal(s) => &s.common,
            Structure::Shipwreck(s) => &s.common,
            Structure::BuriedTreasure(c)
            | Structure::DesertPyramid(c)
            | Structure::EndCity(c)
            | Structure::Fortress(c)
            | Structure::Igloo(c)
            | Structure::JungleTemple(c)
            | Structure::OceanMonument(c)
            | Structure::Stronghold(c)
            | Structure::SwampHut(c)
            | Structure::WoodlandMansion(c) => c,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
// Keys checked in `parse_all` (serde cannot deny unknown fields with `flatten`).
pub struct JigsawStructure {
    #[serde(flatten)]
    pub common: StructureCommon,
    pub start_pool: String,
    #[serde(default)]
    pub start_jigsaw_name: Option<String>,
    pub size: i32,
    pub start_height: HeightProvider,
    pub use_expansion_hack: bool,
    #[serde(default)]
    pub project_start_to_heightmap: Option<HeightmapKind>,
    #[serde(default = "eighty")]
    pub max_distance_from_center: i32,
    #[serde(default)]
    pub pool_aliases: Vec<PoolAlias>,
    #[serde(default)]
    pub dimension_padding: Option<DimensionPadding>,
    #[serde(default)]
    pub liquid_settings: Option<LiquidSettings>,
}

fn eighty() -> i32 {
    80
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
// Keys checked in `parse_all` (serde cannot deny unknown fields with `flatten`).
pub struct MineshaftStructure {
    #[serde(flatten)]
    pub common: StructureCommon,
    pub mineshaft_type: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
// Keys checked in `parse_all` (serde cannot deny unknown fields with `flatten`).
pub struct NetherFossilStructure {
    #[serde(flatten)]
    pub common: StructureCommon,
    pub height: HeightProvider,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
// Keys checked in `parse_all` (serde cannot deny unknown fields with `flatten`).
pub struct OceanRuinStructure {
    #[serde(flatten)]
    pub common: StructureCommon,
    pub biome_temp: String,
    pub large_probability: f32,
    pub cluster_probability: f32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
// Keys checked in `parse_all` (serde cannot deny unknown fields with `flatten`).
pub struct RuinedPortalStructure {
    #[serde(flatten)]
    pub common: StructureCommon,
    pub setups: Vec<RuinedPortalSetup>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
// Keys checked in `parse_all` (serde cannot deny unknown fields with `flatten`).
pub struct ShipwreckStructure {
    #[serde(flatten)]
    pub common: StructureCommon,
    pub is_beached: bool,
}

// ---------------------------------------------------------------------------
// Structure sets
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExclusionZone {
    pub other_set: String,
    pub chunk_count: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrequencyReduction {
    Default,
    #[serde(rename = "legacy_type_1")]
    LegacyType1,
    #[serde(rename = "legacy_type_2")]
    LegacyType2,
    #[serde(rename = "legacy_type_3")]
    LegacyType3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpreadType {
    Linear,
    Triangular,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum StructurePlacement {
    #[serde(rename = "minecraft:random_spread")]
    RandomSpread {
        #[serde(default)]
        locate_offset: Option<[i32; 3]>,
        #[serde(default = "default_reduction")]
        frequency_reduction_method: FrequencyReduction,
        #[serde(default = "one_f")]
        frequency: f32,
        salt: i32,
        #[serde(default)]
        exclusion_zone: Option<ExclusionZone>,
        spacing: i32,
        separation: i32,
        #[serde(default = "linear")]
        spread_type: SpreadType,
    },
    #[serde(rename = "minecraft:concentric_rings")]
    ConcentricRings {
        #[serde(default)]
        locate_offset: Option<[i32; 3]>,
        #[serde(default = "default_reduction")]
        frequency_reduction_method: FrequencyReduction,
        #[serde(default = "one_f")]
        frequency: f32,
        salt: i32,
        #[serde(default)]
        exclusion_zone: Option<ExclusionZone>,
        distance: i32,
        spread: i32,
        count: i32,
        preferred_biomes: String,
    },
}

fn default_reduction() -> FrequencyReduction {
    FrequencyReduction::Default
}
fn one_f() -> f32 {
    1.0
}
fn linear() -> SpreadType {
    SpreadType::Linear
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeightedStructure {
    pub structure: String,
    pub weight: i32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StructureSet {
    pub structures: Vec<WeightedStructure>,
    pub placement: StructurePlacement,
}

// ---------------------------------------------------------------------------
// Biome tags
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TagFile {
    pub values: Vec<TagEntry>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum TagEntry {
    Id(String),
    Obj { id: String, required: bool },
}

impl TagEntry {
    pub fn id(&self) -> &str {
        match self {
            TagEntry::Id(s) => s,
            TagEntry::Obj { id, .. } => id,
        }
    }
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

/// All structure-related registries keyed by id.
#[derive(Debug, Clone, Default)]
pub struct StructureData {
    pub sets: BTreeMap<String, StructureSet>,
    pub structures: BTreeMap<String, Structure>,
    pub pools: BTreeMap<String, TemplatePool>,
    pub processor_lists: BTreeMap<String, ProcessorList>,
    /// Biome tags (`minecraft:has_structure/...`) -> raw entries.
    pub biome_tags: BTreeMap<String, Vec<String>>,
}

fn read_dir_json<T: DeserializeOwned>(
    dir: &Path,
) -> Result<BTreeMap<String, T>, Box<dyn std::error::Error>> {
    let mut out = BTreeMap::new();
    for entry in walkdir::WalkDir::new(dir).sort_by_file_name() {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let rel = path
            .strip_prefix(dir)?
            .with_extension("")
            .to_string_lossy()
            .replace('\\', "/");
        let text = std::fs::read_to_string(path)?;
        let value: T =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        out.insert(format!("minecraft:{rel}"), value);
    }
    Ok(out)
}

/// Parse every structure registry under `worldgen_root`.
pub fn parse_all(
    worldgen_root: &Path,
    java_data: &Path,
) -> Result<StructureData, Box<dyn std::error::Error>> {
    // serde ignores `deny_unknown_fields` on structs that `flatten` the common
    // settings, so every structure file's keys are checked against the fields
    // its type accepts.
    let structures: BTreeMap<String, Structure> = read_dir_json(&worldgen_root.join("structure"))?;
    let raw: BTreeMap<String, serde_json::Map<String, serde_json::Value>> =
        read_dir_json(&worldgen_root.join("structure"))?;
    for (name, obj) in &raw {
        let ty = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let extra: &[&str] = match ty {
            "minecraft:jigsaw" => &[
                "start_pool",
                "start_jigsaw_name",
                "size",
                "start_height",
                "use_expansion_hack",
                "project_start_to_heightmap",
                "max_distance_from_center",
                "pool_aliases",
                "dimension_padding",
                "liquid_settings",
            ],
            "minecraft:mineshaft" => &["mineshaft_type"],
            "minecraft:nether_fossil" => &["height"],
            "minecraft:ocean_ruin" => &["biome_temp", "large_probability", "cluster_probability"],
            "minecraft:ruined_portal" => &["setups"],
            "minecraft:shipwreck" => &["is_beached"],
            _ => &[],
        };
        for key in obj.keys() {
            let common = matches!(
                key.as_str(),
                "type" | "biomes" | "step" | "spawn_overrides" | "terrain_adaptation"
            );
            if !common && !extra.contains(&key.as_str()) {
                return Err(format!("structure {name}: unknown field {key}").into());
            }
        }
    }
    let tags: BTreeMap<String, TagFile> =
        read_dir_json(&java_data.join("tags/worldgen/biome"))?;
    Ok(StructureData {
        sets: read_dir_json(&worldgen_root.join("structure_set"))?,
        structures,
        pools: read_dir_json(&worldgen_root.join("template_pool"))?,
        processor_lists: read_dir_json(&worldgen_root.join("processor_list"))?,
        biome_tags: tags
            .into_iter()
            .map(|(k, v)| (k, v.values.iter().map(|e| e.id().to_string()).collect()))
            .collect(),
    })
}
