//! Typed IR for Java `worldgen/configured_feature` and `worldgen/placed_feature`.
//!
//! Every vanilla file is parsed into these types with `deny_unknown_fields`, and
//! every `type` discriminator is a closed enum, so an unknown feature, placement
//! modifier, provider, predicate, placer or decorator fails the build with the
//! file name and JSON path instead of being skipped.
//!
//! Field defaults follow the Java codecs' `optionalFieldOf(..., default)` values
//! (for example `ore.discard_chance_on_air_exposure` is required while
//! `matching_blocks.offset` defaults to `[0, 0, 0]`).
//!
//! Shapes that Java encodes "either" way (an int or an int provider object, a
//! placed-feature reference string or an inline placed feature, one block or a
//! list of blocks) are decoded through [`Either`], which keeps the inner error
//! message when the object branch fails.

use serde::de::{DeserializeOwned, Error as _};
use serde::{Deserialize, Deserializer};
use std::collections::BTreeMap;
use std::path::Path;

// ---------------------------------------------------------------------------
// Either-shaped values
// ---------------------------------------------------------------------------

/// A JSON value that is either a string/number shorthand or a full object.
///
/// Implemented by hand instead of `#[serde(untagged)]` so the object branch keeps
/// its precise error ("unknown variant `minecraft:foo`") rather than serde's
/// generic "did not match any variant".
macro_rules! either_enum {
    ($(#[$m:meta])* $name:ident { short: $short_ty:ty => $short:ident, full: $full_ty:ty => $full:ident, check: $check:expr }) => {
        $(#[$m])*
        #[derive(Debug, Clone, PartialEq)]
        pub enum $name {
            $short($short_ty),
            $full($full_ty),
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let v = serde_json::Value::deserialize(d)?;
                let check: fn(&serde_json::Value) -> bool = $check;
                if check(&v) {
                    serde_json::from_value(v).map($name::$short).map_err(D::Error::custom)
                } else {
                    serde_json::from_value(v).map($name::$full).map_err(D::Error::custom)
                }
            }
        }
    };
}

fn is_number(v: &serde_json::Value) -> bool {
    v.is_number()
}
fn is_string(v: &serde_json::Value) -> bool {
    v.is_string()
}

/// Generic "one or many" list (Java `ExtraCodecs.compactListCodec` / holder sets).
#[derive(Debug, Clone, PartialEq)]
pub struct OneOrMany<T>(pub Vec<T>);

impl<'de, T: DeserializeOwned> Deserialize<'de> for OneOrMany<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        if v.is_array() {
            serde_json::from_value(v).map(OneOrMany).map_err(D::Error::custom)
        } else {
            serde_json::from_value(v)
                .map(|x| OneOrMany(vec![x]))
                .map_err(D::Error::custom)
        }
    }
}

/// Holder set of blocks: `"#minecraft:tag"`, `"minecraft:block"` or a list of block ids.
#[derive(Debug, Clone, PartialEq)]
pub enum BlockHolderSet {
    Tag(String),
    Blocks(Vec<String>),
}

impl<'de> Deserialize<'de> for BlockHolderSet {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        match v {
            serde_json::Value::String(s) => Ok(match s.strip_prefix('#') {
                Some(tag) => BlockHolderSet::Tag(tag.to_string()),
                None => BlockHolderSet::Blocks(vec![s]),
            }),
            serde_json::Value::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    match item {
                        serde_json::Value::String(s) if !s.starts_with('#') => out.push(s),
                        other => {
                            return Err(D::Error::custom(format!(
                                "block holder set list entries must be block ids, got {other}"
                            )));
                        }
                    }
                }
                Ok(BlockHolderSet::Blocks(out))
            }
            other => Err(D::Error::custom(format!(
                "expected block holder set, got {other}"
            ))),
        }
    }
}

/// A `#tag` reference (fields that only accept tags, e.g. `TagKey<Block>`).
#[derive(Debug, Clone, PartialEq)]
pub struct TagRef(pub String);

impl<'de> Deserialize<'de> for TagRef {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        match s.strip_prefix('#') {
            Some(t) => Ok(TagRef(t.to_string())),
            None => Err(D::Error::custom(format!("expected #tag, got {s}"))),
        }
    }
}

// ---------------------------------------------------------------------------
// Block states
// ---------------------------------------------------------------------------

/// `{"Name": "...", "Properties": {...}}`
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockStateJson {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Properties", default)]
    pub properties: BTreeMap<String, String>,
}

// ---------------------------------------------------------------------------
// Value providers
// ---------------------------------------------------------------------------

/// Object form of an int provider.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum IntProviderObj {
    #[serde(rename = "minecraft:constant")]
    Constant { value: i32 },
    #[serde(rename = "minecraft:uniform")]
    Uniform {
        min_inclusive: i32,
        max_inclusive: i32,
    },
    #[serde(rename = "minecraft:biased_to_bottom")]
    BiasedToBottom {
        min_inclusive: i32,
        max_inclusive: i32,
    },
    #[serde(rename = "minecraft:clamped")]
    Clamped {
        source: Box<IntProvider>,
        min_inclusive: i32,
        max_inclusive: i32,
    },
    #[serde(rename = "minecraft:clamped_normal")]
    ClampedNormal {
        mean: f32,
        deviation: f32,
        min_inclusive: i32,
        max_inclusive: i32,
    },
    #[serde(rename = "minecraft:weighted_list")]
    WeightedList { distribution: Vec<WeightedInt> },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeightedInt {
    pub data: IntProvider,
    pub weight: i32,
}

either_enum! {
    /// Java `IntProvider` (an int is a constant provider).
    IntProvider { short: i32 => Constant, full: IntProviderObj => Provider, check: is_number }
}

/// Untyped `UniformInt` (some fields use `UniformInt.CODEC` directly).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UniformIntJson {
    pub min_inclusive: i32,
    pub max_inclusive: i32,
}

/// Object form of a float provider.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum FloatProviderObj {
    #[serde(rename = "minecraft:constant")]
    Constant { value: f32 },
    #[serde(rename = "minecraft:uniform")]
    Uniform {
        min_inclusive: f32,
        max_exclusive: f32,
    },
    #[serde(rename = "minecraft:clamped_normal")]
    ClampedNormal {
        mean: f32,
        deviation: f32,
        min: f32,
        max: f32,
    },
    #[serde(rename = "minecraft:trapezoid")]
    Trapezoid { min: f32, max: f32, plateau: f32 },
}

either_enum! {
    /// Java `FloatProvider` (a number is a constant provider).
    FloatProvider { short: f32 => Constant, full: FloatProviderObj => Provider, check: is_number }
}

/// Java `VerticalAnchor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum VerticalAnchor {
    Absolute(i32),
    AboveBottom(i32),
    BelowTop(i32),
}

/// Object form of a height provider.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum HeightProviderObj {
    #[serde(rename = "minecraft:constant")]
    Constant { value: VerticalAnchor },
    #[serde(rename = "minecraft:uniform")]
    Uniform {
        min_inclusive: VerticalAnchor,
        max_inclusive: VerticalAnchor,
    },
    #[serde(rename = "minecraft:biased_to_bottom")]
    BiasedToBottom {
        min_inclusive: VerticalAnchor,
        max_inclusive: VerticalAnchor,
        #[serde(default = "one")]
        inner: i32,
    },
    #[serde(rename = "minecraft:very_biased_to_bottom")]
    VeryBiasedToBottom {
        min_inclusive: VerticalAnchor,
        max_inclusive: VerticalAnchor,
        #[serde(default = "one")]
        inner: i32,
    },
    #[serde(rename = "minecraft:trapezoid")]
    Trapezoid {
        min_inclusive: VerticalAnchor,
        max_inclusive: VerticalAnchor,
        #[serde(default)]
        plateau: i32,
    },
}

fn one() -> i32 {
    1
}

/// Java `HeightProvider` (a bare anchor is a constant provider).
#[derive(Debug, Clone, PartialEq)]
pub enum HeightProvider {
    Constant(VerticalAnchor),
    Provider(HeightProviderObj),
}

impl<'de> Deserialize<'de> for HeightProvider {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        if v.get("type").is_some() {
            serde_json::from_value(v)
                .map(HeightProvider::Provider)
                .map_err(D::Error::custom)
        } else {
            serde_json::from_value(v)
                .map(HeightProvider::Constant)
                .map_err(D::Error::custom)
        }
    }
}

/// `NormalNoise.NoiseParameters` as written inline in providers.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoiseParamsJson {
    #[serde(rename = "firstOctave")]
    pub first_octave: i32,
    pub amplitudes: Vec<f64>,
}

/// Java `BlockStateProvider`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum StateProvider {
    #[serde(rename = "minecraft:simple_state_provider")]
    Simple { state: BlockStateJson },
    #[serde(rename = "minecraft:rotated_block_provider")]
    Rotated { state: BlockStateJson },
    #[serde(rename = "minecraft:weighted_state_provider")]
    Weighted { entries: Vec<WeightedState> },
    #[serde(rename = "minecraft:randomized_int_state_provider")]
    RandomizedInt {
        source: Box<StateProvider>,
        property: String,
        values: IntProvider,
    },
    #[serde(rename = "minecraft:noise_provider")]
    Noise {
        seed: i64,
        noise: NoiseParamsJson,
        scale: f32,
        states: Vec<BlockStateJson>,
    },
    #[serde(rename = "minecraft:dual_noise_provider")]
    DualNoise {
        seed: i64,
        noise: NoiseParamsJson,
        scale: f32,
        states: Vec<BlockStateJson>,
        variety: UniformIntArray,
        slow_noise: NoiseParamsJson,
        slow_scale: f32,
    },
    #[serde(rename = "minecraft:noise_threshold_provider")]
    NoiseThreshold {
        seed: i64,
        noise: NoiseParamsJson,
        scale: f32,
        threshold: f32,
        high_chance: f32,
        default_state: BlockStateJson,
        low_states: Vec<BlockStateJson>,
        high_states: Vec<BlockStateJson>,
    },
}

/// `InclusiveRange<Integer>` encoded as `[min, max]`.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct UniformIntArray(pub i32, pub i32);

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeightedState {
    pub data: BlockStateJson,
    pub weight: i32,
}

/// `RuleBasedBlockStateProvider` (used by `disk`; not type-dispatched).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleBasedStateProvider {
    pub fallback: StateProvider,
    pub rules: Vec<StateRule>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateRule {
    pub if_true: BlockPredicate,
    pub then: StateProvider,
}

// ---------------------------------------------------------------------------
// Predicates
// ---------------------------------------------------------------------------

/// Java `Direction` (serialized lowercase).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

fn zero_offset() -> [i32; 3] {
    [0, 0, 0]
}

/// Java `BlockPredicate`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum BlockPredicate {
    #[serde(rename = "minecraft:matching_blocks")]
    MatchingBlocks {
        blocks: BlockHolderSet,
        #[serde(default = "zero_offset")]
        offset: [i32; 3],
    },
    #[serde(rename = "minecraft:matching_block_tag")]
    MatchingBlockTag {
        tag: String,
        #[serde(default = "zero_offset")]
        offset: [i32; 3],
    },
    #[serde(rename = "minecraft:matching_fluids")]
    MatchingFluids {
        fluids: OneOrMany<String>,
        #[serde(default = "zero_offset")]
        offset: [i32; 3],
    },
    #[serde(rename = "minecraft:solid")]
    Solid {
        #[serde(default = "zero_offset")]
        offset: [i32; 3],
    },
    #[serde(rename = "minecraft:replaceable")]
    Replaceable {
        #[serde(default = "zero_offset")]
        offset: [i32; 3],
    },
    #[serde(rename = "minecraft:would_survive")]
    WouldSurvive {
        state: BlockStateJson,
        #[serde(default = "zero_offset")]
        offset: [i32; 3],
    },
    #[serde(rename = "minecraft:has_sturdy_face")]
    HasSturdyFace {
        direction: Direction,
        #[serde(default = "zero_offset")]
        offset: [i32; 3],
    },
    #[serde(rename = "minecraft:inside_world_bounds")]
    InsideWorldBounds {
        #[serde(default = "zero_offset")]
        offset: [i32; 3],
    },
    #[serde(rename = "minecraft:all_of")]
    AllOf { predicates: Vec<BlockPredicate> },
    #[serde(rename = "minecraft:any_of")]
    AnyOf { predicates: Vec<BlockPredicate> },
    #[serde(rename = "minecraft:not")]
    Not { predicate: Box<BlockPredicate> },
    #[serde(rename = "minecraft:true")]
    True {},
}

/// Java `RuleTest` (ore targets, structure processors).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "predicate_type", deny_unknown_fields)]
pub enum RuleTest {
    #[serde(rename = "minecraft:always_true")]
    AlwaysTrue {},
    #[serde(rename = "minecraft:block_match")]
    BlockMatch { block: String },
    #[serde(rename = "minecraft:blockstate_match")]
    BlockStateMatch { block_state: BlockStateJson },
    #[serde(rename = "minecraft:tag_match")]
    TagMatch { tag: String },
    #[serde(rename = "minecraft:random_block_match")]
    RandomBlockMatch { block: String, probability: f32 },
    #[serde(rename = "minecraft:random_blockstate_match")]
    RandomBlockStateMatch {
        block_state: BlockStateJson,
        probability: f32,
    },
}

// ---------------------------------------------------------------------------
// Placement
// ---------------------------------------------------------------------------

/// Java `Heightmap.Types` (serialized SCREAMING_SNAKE).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HeightmapKind {
    WorldSurfaceWg,
    WorldSurface,
    OceanFloorWg,
    OceanFloor,
    MotionBlocking,
    MotionBlockingNoLeaves,
}

/// Vertical search direction of `environment_scan`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VerticalDirection {
    Up,
    Down,
}

/// Java `PlacementModifier`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum PlacementModifier {
    #[serde(rename = "minecraft:biome")]
    Biome {},
    #[serde(rename = "minecraft:block_predicate_filter")]
    BlockPredicateFilter { predicate: BlockPredicate },
    #[serde(rename = "minecraft:count")]
    Count { count: IntProvider },
    #[serde(rename = "minecraft:count_on_every_layer")]
    CountOnEveryLayer { count: IntProvider },
    #[serde(rename = "minecraft:environment_scan")]
    EnvironmentScan {
        direction_of_search: VerticalDirection,
        target_condition: BlockPredicate,
        #[serde(default)]
        allowed_search_condition: Option<BlockPredicate>,
        max_steps: i32,
    },
    #[serde(rename = "minecraft:fixed_placement")]
    FixedPlacement { positions: Vec<[i32; 3]> },
    #[serde(rename = "minecraft:height_range")]
    HeightRange { height: HeightProvider },
    #[serde(rename = "minecraft:heightmap")]
    Heightmap { heightmap: HeightmapKind },
    #[serde(rename = "minecraft:in_square")]
    InSquare {},
    #[serde(rename = "minecraft:noise_based_count")]
    NoiseBasedCount {
        noise_to_count_ratio: i32,
        noise_factor: f64,
        #[serde(default)]
        noise_offset: f64,
    },
    #[serde(rename = "minecraft:noise_threshold_count")]
    NoiseThresholdCount {
        noise_level: f64,
        below_noise: i32,
        above_noise: i32,
    },
    #[serde(rename = "minecraft:random_offset")]
    RandomOffset {
        xz_spread: IntProvider,
        y_spread: IntProvider,
    },
    #[serde(rename = "minecraft:rarity_filter")]
    RarityFilter { chance: i32 },
    #[serde(rename = "minecraft:surface_relative_threshold_filter")]
    SurfaceRelativeThresholdFilter {
        heightmap: HeightmapKind,
        #[serde(default)]
        min_inclusive: Option<i32>,
        #[serde(default)]
        max_inclusive: Option<i32>,
    },
    #[serde(rename = "minecraft:surface_water_depth_filter")]
    SurfaceWaterDepthFilter { max_water_depth: i32 },
}

/// A placed feature: configured feature + placement modifiers.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacedFeatureJson {
    pub feature: ConfiguredRef,
    pub placement: Vec<PlacementModifier>,
}

either_enum! {
    /// A configured feature: registry reference or inline definition.
    ConfiguredRef { short: String => Ref, full: Box<ConfiguredFeature> => Inline, check: is_string }
}

either_enum! {
    /// A placed feature: registry reference or inline definition.
    PlacedRef { short: String => Ref, full: Box<PlacedFeatureJson> => Inline, check: is_string }
}

// ---------------------------------------------------------------------------
// Configured features
// ---------------------------------------------------------------------------

/// Empty configuration (`NoneFeatureConfiguration`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoConfig {}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OreTarget {
    pub target: RuleTest,
    pub state: BlockStateJson,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OreConfig {
    pub targets: Vec<OreTarget>,
    pub size: i32,
    pub discard_chance_on_air_exposure: f32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RandomPatchConfig {
    pub tries: i32,
    pub xz_spread: i32,
    pub y_spread: i32,
    pub feature: PlacedRef,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimpleBlockConfig {
    pub to_place: StateProvider,
    #[serde(default)]
    pub schedule_tick: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeightedPlaced {
    pub feature: PlacedRef,
    pub chance: f32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RandomSelectorConfig {
    pub features: Vec<WeightedPlaced>,
    pub default: PlacedRef,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimpleRandomSelectorConfig {
    pub features: Vec<PlacedRef>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RandomBooleanSelectorConfig {
    pub feature_true: PlacedRef,
    pub feature_false: PlacedRef,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiskConfig {
    pub state_provider: RuleBasedStateProvider,
    pub target: BlockPredicate,
    pub radius: IntProvider,
    pub half_height: i32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LakeConfig {
    pub fluid: StateProvider,
    pub barrier: StateProvider,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpringConfig {
    pub state: BlockStateJson,
    #[serde(default = "yes")]
    pub requires_block_below: bool,
    #[serde(default = "four")]
    pub rock_count: i32,
    #[serde(default = "one")]
    pub hole_count: i32,
    pub valid_blocks: BlockHolderSet,
}

fn yes() -> bool {
    true
}
fn four() -> i32 {
    4
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColumnLayer {
    pub height: IntProvider,
    pub provider: StateProvider,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockColumnConfig {
    pub layers: Vec<ColumnLayer>,
    pub direction: Direction,
    pub allowed_placement: BlockPredicate,
    pub prioritize_tip: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VegetationPatchConfig {
    pub replaceable: TagRef,
    pub ground_state: StateProvider,
    pub vegetation_feature: PlacedRef,
    pub surface: CaveSurface,
    pub depth: IntProvider,
    pub extra_bottom_block_chance: f32,
    pub vertical_range: i32,
    pub vegetation_chance: f32,
    pub xz_radius: IntProvider,
    pub extra_edge_column_chance: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CaveSurface {
    Ceiling,
    Floor,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HugeMushroomConfig {
    pub cap_provider: StateProvider,
    pub stem_provider: StateProvider,
    #[serde(default = "two")]
    pub foliage_radius: i32,
}

fn two() -> i32 {
    2
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HugeFungusConfig {
    pub valid_base_block: BlockStateJson,
    pub stem_state: BlockStateJson,
    pub hat_state: BlockStateJson,
    pub decor_state: BlockStateJson,
    pub replaceable_blocks: BlockPredicate,
    #[serde(default)]
    pub planted: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbabilityConfig {
    pub probability: f32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CountConfig {
    pub count: IntProvider,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockStateConfig {
    pub state: BlockStateJson,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockPileConfig {
    pub state_provider: StateProvider,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeodeConfig {
    pub blocks: GeodeBlocks,
    pub layers: GeodeLayers,
    pub crack: GeodeCrack,
    #[serde(default = "geode_potential")]
    pub use_potential_placements_chance: f64,
    #[serde(default = "geode_alt")]
    pub use_alternate_layer0_chance: f64,
    #[serde(default = "yes")]
    pub placements_require_layer0_alternate: bool,
    pub outer_wall_distance: IntProvider,
    pub distribution_points: IntProvider,
    pub point_offset: IntProvider,
    #[serde(default = "geode_min_off")]
    pub min_gen_offset: i32,
    #[serde(default = "geode_max_off")]
    pub max_gen_offset: i32,
    #[serde(default = "geode_noise")]
    pub noise_multiplier: f64,
    pub invalid_blocks_threshold: i32,
}

fn geode_potential() -> f64 {
    0.35
}
fn geode_alt() -> f64 {
    0.0
}
fn geode_min_off() -> i32 {
    -16
}
fn geode_max_off() -> i32 {
    16
}
fn geode_noise() -> f64 {
    0.05
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeodeBlocks {
    pub filling_provider: StateProvider,
    pub inner_layer_provider: StateProvider,
    pub alternate_inner_layer_provider: StateProvider,
    pub middle_layer_provider: StateProvider,
    pub outer_layer_provider: StateProvider,
    pub inner_placements: Vec<BlockStateJson>,
    pub cannot_replace: TagRef,
    pub invalid_blocks: TagRef,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeodeLayers {
    #[serde(default = "f1_7")]
    pub filling: f64,
    #[serde(default = "f2_2")]
    pub inner_layer: f64,
    #[serde(default = "f3_2")]
    pub middle_layer: f64,
    #[serde(default = "f4_2")]
    pub outer_layer: f64,
}

fn f1_7() -> f64 {
    1.7
}
fn f2_2() -> f64 {
    2.2
}
fn f3_2() -> f64 {
    3.2
}
fn f4_2() -> f64 {
    4.2
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeodeCrack {
    #[serde(default)]
    pub generate_crack_chance: f64,
    #[serde(default = "f2_0")]
    pub base_crack_size: f64,
    #[serde(default = "two")]
    pub crack_point_offset: i32,
}

fn f2_0() -> f64 {
    2.0
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DripstoneClusterConfig {
    pub floor_to_ceiling_search_range: i32,
    pub height: IntProvider,
    pub radius: IntProvider,
    pub max_stalagmite_stalactite_height_diff: i32,
    pub height_deviation: i32,
    pub dripstone_block_layer_thickness: IntProvider,
    pub density: FloatProvider,
    pub wetness: FloatProvider,
    pub chance_of_dripstone_column_at_max_distance_from_center: f32,
    pub max_distance_from_edge_affecting_chance_of_dripstone_column: i32,
    pub max_distance_from_center_affecting_height_bias: i32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LargeDripstoneConfig {
    #[serde(default = "thirty")]
    pub floor_to_ceiling_search_range: i32,
    pub column_radius: IntProvider,
    pub height_scale: FloatProvider,
    pub max_column_radius_to_cave_height_ratio: f32,
    pub stalactite_bluntness: FloatProvider,
    pub stalagmite_bluntness: FloatProvider,
    pub wind_speed: FloatProvider,
    pub min_radius_for_wind: i32,
    pub min_bluntness_for_wind: f32,
}

fn thirty() -> i32 {
    30
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PointedDripstoneConfig {
    #[serde(default = "f0_2")]
    pub chance_of_taller_dripstone: f32,
    #[serde(default = "f0_7")]
    pub chance_of_directional_spread: f32,
    #[serde(default = "f0_5")]
    pub chance_of_spread_radius2: f32,
    #[serde(default = "f0_5")]
    pub chance_of_spread_radius3: f32,
}

fn f0_2() -> f32 {
    0.2
}
fn f0_7() -> f32 {
    0.7
}
fn f0_5() -> f32 {
    0.5
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SculkPatchConfig {
    pub charge_count: i32,
    pub amount_per_charge: i32,
    pub spread_attempts: i32,
    pub growth_rounds: i32,
    pub spread_rounds: i32,
    pub extra_rare_growths: IntProvider,
    pub catalyst_chance: f32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MultifaceGrowthConfig {
    pub block: String,
    #[serde(default = "ten")]
    pub search_range: i32,
    #[serde(default)]
    pub can_place_on_floor: bool,
    #[serde(default)]
    pub can_place_on_ceiling: bool,
    #[serde(default)]
    pub can_place_on_wall: bool,
    #[serde(default = "f0_5")]
    pub chance_of_spreading: f32,
    pub can_be_placed_on: BlockHolderSet,
}

fn ten() -> i32 {
    10
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FossilConfig {
    pub fossil_structures: Vec<String>,
    pub overlay_structures: Vec<String>,
    pub fossil_processors: String,
    pub overlay_processors: String,
    pub max_empty_corners_allowed: i32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootSystemConfig {
    pub feature: PlacedRef,
    pub required_vertical_space_for_tree: i32,
    pub root_radius: i32,
    pub root_replaceable: TagRef,
    pub root_state_provider: StateProvider,
    pub root_placement_attempts: i32,
    pub root_column_max_height: i32,
    pub hanging_root_radius: i32,
    pub hanging_roots_vertical_span: i32,
    pub hanging_root_state_provider: StateProvider,
    pub hanging_root_placement_attempts: i32,
    pub allowed_vertical_water_for_tree: i32,
    pub allowed_tree_position: BlockPredicate,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnderwaterMagmaConfig {
    pub floor_search_range: i32,
    pub placement_radius_around_floor: i32,
    pub placement_probability_per_valid_position: f32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetherForestVegetationConfig {
    pub state_provider: StateProvider,
    pub spread_width: i32,
    pub spread_height: i32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TwistingVinesConfig {
    pub spread_width: i32,
    pub spread_height: i32,
    pub max_height: i32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplaceBlobsConfig {
    pub target: BlockStateJson,
    pub state: BlockStateJson,
    pub radius: IntProvider,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColumnConfig {
    pub reach: IntProvider,
    pub height: IntProvider,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeltaConfig {
    pub contents: BlockStateJson,
    pub rim: BlockStateJson,
    pub size: IntProvider,
    pub rim_size: IntProvider,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndGatewayConfig {
    #[serde(default)]
    pub exit: Option<[i32; 3]>,
    pub exact: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndSpikeConfig {
    #[serde(default)]
    pub crystal_invulnerable: bool,
    pub spikes: Vec<EndSpike>,
    #[serde(default)]
    pub crystal_beam_target: Option<[i32; 3]>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndSpike {
    #[serde(rename = "centerX", default)]
    pub center_x: i32,
    #[serde(rename = "centerZ", default)]
    pub center_z: i32,
    #[serde(default)]
    pub radius: i32,
    #[serde(default)]
    pub height: i32,
    #[serde(default)]
    pub guarded: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FallenTreeConfig {
    pub trunk_provider: StateProvider,
    pub log_length: IntProvider,
    pub stump_decorators: Vec<TreeDecorator>,
    pub log_decorators: Vec<TreeDecorator>,
}

// ---- trees ---------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeConfig {
    pub trunk_provider: StateProvider,
    pub trunk_placer: TrunkPlacer,
    pub foliage_provider: StateProvider,
    pub foliage_placer: FoliagePlacer,
    #[serde(default)]
    pub root_placer: Option<RootPlacer>,
    pub dirt_provider: StateProvider,
    pub minimum_size: FeatureSize,
    pub decorators: Vec<TreeDecorator>,
    #[serde(default)]
    pub ignore_vines: bool,
    #[serde(default)]
    pub force_dirt: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum TrunkPlacer {
    #[serde(rename = "minecraft:straight_trunk_placer")]
    Straight {
        base_height: i32,
        height_rand_a: i32,
        height_rand_b: i32,
    },
    #[serde(rename = "minecraft:forking_trunk_placer")]
    Forking {
        base_height: i32,
        height_rand_a: i32,
        height_rand_b: i32,
    },
    #[serde(rename = "minecraft:giant_trunk_placer")]
    Giant {
        base_height: i32,
        height_rand_a: i32,
        height_rand_b: i32,
    },
    #[serde(rename = "minecraft:mega_jungle_trunk_placer")]
    MegaJungle {
        base_height: i32,
        height_rand_a: i32,
        height_rand_b: i32,
    },
    #[serde(rename = "minecraft:dark_oak_trunk_placer")]
    DarkOak {
        base_height: i32,
        height_rand_a: i32,
        height_rand_b: i32,
    },
    #[serde(rename = "minecraft:fancy_trunk_placer")]
    Fancy {
        base_height: i32,
        height_rand_a: i32,
        height_rand_b: i32,
    },
    #[serde(rename = "minecraft:bending_trunk_placer")]
    Bending {
        base_height: i32,
        height_rand_a: i32,
        height_rand_b: i32,
        #[serde(default = "one")]
        min_height_for_leaves: i32,
        bend_length: IntProvider,
    },
    #[serde(rename = "minecraft:upwards_branching_trunk_placer")]
    UpwardsBranching {
        base_height: i32,
        height_rand_a: i32,
        height_rand_b: i32,
        extra_branch_steps: IntProvider,
        place_branch_per_log_probability: f32,
        extra_branch_length: IntProvider,
        can_grow_through: TagRef,
    },
    #[serde(rename = "minecraft:cherry_trunk_placer")]
    Cherry {
        base_height: i32,
        height_rand_a: i32,
        height_rand_b: i32,
        branch_count: IntProvider,
        branch_horizontal_length: IntProvider,
        branch_start_offset_from_top: UniformIntJson,
        branch_end_offset_from_top: IntProvider,
    },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum FoliagePlacer {
    #[serde(rename = "minecraft:blob_foliage_placer")]
    Blob {
        radius: IntProvider,
        offset: IntProvider,
        height: i32,
    },
    #[serde(rename = "minecraft:bush_foliage_placer")]
    Bush {
        radius: IntProvider,
        offset: IntProvider,
        height: i32,
    },
    #[serde(rename = "minecraft:fancy_foliage_placer")]
    Fancy {
        radius: IntProvider,
        offset: IntProvider,
        height: i32,
    },
    #[serde(rename = "minecraft:jungle_foliage_placer")]
    Jungle {
        radius: IntProvider,
        offset: IntProvider,
        height: i32,
    },
    #[serde(rename = "minecraft:spruce_foliage_placer")]
    Spruce {
        radius: IntProvider,
        offset: IntProvider,
        trunk_height: IntProvider,
    },
    #[serde(rename = "minecraft:pine_foliage_placer")]
    Pine {
        radius: IntProvider,
        offset: IntProvider,
        height: IntProvider,
    },
    #[serde(rename = "minecraft:acacia_foliage_placer")]
    Acacia {
        radius: IntProvider,
        offset: IntProvider,
    },
    #[serde(rename = "minecraft:dark_oak_foliage_placer")]
    DarkOak {
        radius: IntProvider,
        offset: IntProvider,
    },
    #[serde(rename = "minecraft:mega_pine_foliage_placer")]
    MegaPine {
        radius: IntProvider,
        offset: IntProvider,
        crown_height: IntProvider,
    },
    #[serde(rename = "minecraft:random_spread_foliage_placer")]
    RandomSpread {
        radius: IntProvider,
        offset: IntProvider,
        foliage_height: IntProvider,
        leaf_placement_attempts: i32,
    },
    #[serde(rename = "minecraft:cherry_foliage_placer")]
    Cherry {
        radius: IntProvider,
        offset: IntProvider,
        height: IntProvider,
        wide_bottom_layer_hole_chance: f32,
        corner_hole_chance: f32,
        hanging_leaves_chance: f32,
        hanging_leaves_extension_chance: f32,
    },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum RootPlacer {
    #[serde(rename = "minecraft:mangrove_root_placer")]
    Mangrove {
        trunk_offset_y: IntProvider,
        root_provider: StateProvider,
        #[serde(default)]
        above_root_placement: Option<AboveRootPlacement>,
        mangrove_root_placement: MangroveRootPlacement,
    },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AboveRootPlacement {
    pub above_root_provider: StateProvider,
    pub above_root_placement_chance: f32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MangroveRootPlacement {
    pub can_grow_through: TagRef,
    pub muddy_roots_in: BlockHolderSet,
    pub muddy_roots_provider: StateProvider,
    pub max_root_width: i32,
    pub max_root_length: i32,
    pub random_skew_chance: f32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum FeatureSize {
    #[serde(rename = "minecraft:two_layers_feature_size")]
    TwoLayers {
        #[serde(default = "one")]
        limit: i32,
        #[serde(default)]
        lower_size: i32,
        #[serde(default = "one")]
        upper_size: i32,
        #[serde(default)]
        min_clipped_height: Option<i32>,
    },
    #[serde(rename = "minecraft:three_layers_feature_size")]
    ThreeLayers {
        #[serde(default = "one")]
        limit: i32,
        #[serde(default = "one")]
        upper_limit: i32,
        #[serde(default)]
        lower_size: i32,
        #[serde(default = "one")]
        middle_size: i32,
        #[serde(default = "one")]
        upper_size: i32,
        #[serde(default)]
        min_clipped_height: Option<i32>,
    },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum TreeDecorator {
    #[serde(rename = "minecraft:trunk_vine")]
    TrunkVine {},
    #[serde(rename = "minecraft:leave_vine")]
    LeaveVine { probability: f32 },
    #[serde(rename = "minecraft:cocoa")]
    Cocoa { probability: f32 },
    #[serde(rename = "minecraft:beehive")]
    Beehive { probability: f32 },
    #[serde(rename = "minecraft:alter_ground")]
    AlterGround { provider: StateProvider },
    #[serde(rename = "minecraft:attached_to_leaves")]
    AttachedToLeaves {
        probability: f32,
        exclusion_radius_xz: i32,
        exclusion_radius_y: i32,
        block_provider: StateProvider,
        required_empty_blocks: i32,
        directions: Vec<Direction>,
    },
    #[serde(rename = "minecraft:attached_to_logs")]
    AttachedToLogs {
        probability: f32,
        block_provider: StateProvider,
        directions: Vec<Direction>,
    },
    #[serde(rename = "minecraft:place_on_ground")]
    PlaceOnGround {
        #[serde(default = "onetwentyeight")]
        tries: i32,
        #[serde(default = "two")]
        radius: i32,
        #[serde(default = "one")]
        height: i32,
        block_state_provider: StateProvider,
    },
    #[serde(rename = "minecraft:creaking_heart")]
    CreakingHeart { probability: f32 },
    #[serde(rename = "minecraft:pale_moss")]
    PaleMoss {
        leaves_probability: f32,
        trunk_probability: f32,
        ground_probability: f32,
    },
}

fn onetwentyeight() -> i32 {
    128
}

/// Java `ConfiguredFeature`: one variant per `Feature` type used by vanilla data.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", content = "config", deny_unknown_fields)]
pub enum ConfiguredFeature {
    #[serde(rename = "minecraft:bamboo")]
    Bamboo(ProbabilityConfig),
    #[serde(rename = "minecraft:basalt_columns")]
    BasaltColumns(ColumnConfig),
    #[serde(rename = "minecraft:basalt_pillar")]
    BasaltPillar(NoConfig),
    #[serde(rename = "minecraft:block_column")]
    BlockColumn(BlockColumnConfig),
    #[serde(rename = "minecraft:block_pile")]
    BlockPile(BlockPileConfig),
    #[serde(rename = "minecraft:blue_ice")]
    BlueIce(NoConfig),
    #[serde(rename = "minecraft:bonus_chest")]
    BonusChest(NoConfig),
    #[serde(rename = "minecraft:chorus_plant")]
    ChorusPlant(NoConfig),
    #[serde(rename = "minecraft:coral_claw")]
    CoralClaw(NoConfig),
    #[serde(rename = "minecraft:coral_mushroom")]
    CoralMushroom(NoConfig),
    #[serde(rename = "minecraft:coral_tree")]
    CoralTree(NoConfig),
    #[serde(rename = "minecraft:delta_feature")]
    Delta(DeltaConfig),
    #[serde(rename = "minecraft:desert_well")]
    DesertWell(NoConfig),
    #[serde(rename = "minecraft:disk")]
    Disk(DiskConfig),
    #[serde(rename = "minecraft:dripstone_cluster")]
    DripstoneCluster(DripstoneClusterConfig),
    #[serde(rename = "minecraft:end_gateway")]
    EndGateway(EndGatewayConfig),
    #[serde(rename = "minecraft:end_island")]
    EndIsland(NoConfig),
    #[serde(rename = "minecraft:end_platform")]
    EndPlatform(NoConfig),
    #[serde(rename = "minecraft:end_spike")]
    EndSpike(EndSpikeConfig),
    #[serde(rename = "minecraft:fallen_tree")]
    FallenTree(FallenTreeConfig),
    #[serde(rename = "minecraft:flower")]
    Flower(RandomPatchConfig),
    #[serde(rename = "minecraft:forest_rock")]
    ForestRock(BlockStateConfig),
    #[serde(rename = "minecraft:fossil")]
    Fossil(FossilConfig),
    #[serde(rename = "minecraft:freeze_top_layer")]
    FreezeTopLayer(NoConfig),
    #[serde(rename = "minecraft:geode")]
    Geode(Box<GeodeConfig>),
    #[serde(rename = "minecraft:glowstone_blob")]
    GlowstoneBlob(NoConfig),
    #[serde(rename = "minecraft:huge_brown_mushroom")]
    HugeBrownMushroom(HugeMushroomConfig),
    #[serde(rename = "minecraft:huge_fungus")]
    HugeFungus(HugeFungusConfig),
    #[serde(rename = "minecraft:huge_red_mushroom")]
    HugeRedMushroom(HugeMushroomConfig),
    #[serde(rename = "minecraft:ice_spike")]
    IceSpike(NoConfig),
    #[serde(rename = "minecraft:iceberg")]
    Iceberg(BlockStateConfig),
    #[serde(rename = "minecraft:kelp")]
    Kelp(NoConfig),
    #[serde(rename = "minecraft:lake")]
    Lake(LakeConfig),
    #[serde(rename = "minecraft:large_dripstone")]
    LargeDripstone(LargeDripstoneConfig),
    #[serde(rename = "minecraft:monster_room")]
    MonsterRoom(NoConfig),
    #[serde(rename = "minecraft:multiface_growth")]
    MultifaceGrowth(MultifaceGrowthConfig),
    #[serde(rename = "minecraft:nether_forest_vegetation")]
    NetherForestVegetation(NetherForestVegetationConfig),
    #[serde(rename = "minecraft:netherrack_replace_blobs")]
    NetherrackReplaceBlobs(ReplaceBlobsConfig),
    #[serde(rename = "minecraft:no_bonemeal_flower")]
    NoBonemealFlower(RandomPatchConfig),
    #[serde(rename = "minecraft:ore")]
    Ore(OreConfig),
    #[serde(rename = "minecraft:pointed_dripstone")]
    PointedDripstone(PointedDripstoneConfig),
    #[serde(rename = "minecraft:random_boolean_selector")]
    RandomBooleanSelector(RandomBooleanSelectorConfig),
    #[serde(rename = "minecraft:random_patch")]
    RandomPatch(RandomPatchConfig),
    #[serde(rename = "minecraft:random_selector")]
    RandomSelector(RandomSelectorConfig),
    #[serde(rename = "minecraft:root_system")]
    RootSystem(Box<RootSystemConfig>),
    #[serde(rename = "minecraft:scattered_ore")]
    ScatteredOre(OreConfig),
    #[serde(rename = "minecraft:sculk_patch")]
    SculkPatch(SculkPatchConfig),
    #[serde(rename = "minecraft:sea_pickle")]
    SeaPickle(CountConfig),
    #[serde(rename = "minecraft:seagrass")]
    Seagrass(ProbabilityConfig),
    #[serde(rename = "minecraft:simple_block")]
    SimpleBlock(SimpleBlockConfig),
    #[serde(rename = "minecraft:simple_random_selector")]
    SimpleRandomSelector(SimpleRandomSelectorConfig),
    #[serde(rename = "minecraft:spring_feature")]
    Spring(SpringConfig),
    #[serde(rename = "minecraft:tree")]
    Tree(Box<TreeConfig>),
    #[serde(rename = "minecraft:twisting_vines")]
    TwistingVines(TwistingVinesConfig),
    #[serde(rename = "minecraft:underwater_magma")]
    UnderwaterMagma(UnderwaterMagmaConfig),
    #[serde(rename = "minecraft:vegetation_patch")]
    VegetationPatch(Box<VegetationPatchConfig>),
    #[serde(rename = "minecraft:vines")]
    Vines(NoConfig),
    #[serde(rename = "minecraft:void_start_platform")]
    VoidStartPlatform(NoConfig),
    #[serde(rename = "minecraft:waterlogged_vegetation_patch")]
    WaterloggedVegetationPatch(Box<VegetationPatchConfig>),
    #[serde(rename = "minecraft:weeping_vines")]
    WeepingVines(NoConfig),
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

/// All configured and placed features, keyed by registry id (`minecraft:<path>`).
#[derive(Debug, Clone, Default)]
pub struct FeatureData {
    pub configured: BTreeMap<String, ConfiguredFeature>,
    pub placed: BTreeMap<String, PlacedFeatureJson>,
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

/// Parse `configured_feature/` and `placed_feature/` under `worldgen_root`.
pub fn parse_all(worldgen_root: &Path) -> Result<FeatureData, Box<dyn std::error::Error>> {
    Ok(FeatureData {
        configured: read_dir_json(&worldgen_root.join("configured_feature"))?,
        placed: read_dir_json(&worldgen_root.join("placed_feature"))?,
    })
}
