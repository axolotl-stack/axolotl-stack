//! Static feature configurations emitted by the feature code generator.
//!
//! Each struct mirrors the Java configuration codec of one feature type with
//! block states, block sets and providers already resolved. Nested placed
//! features are not stored here: the generated code passes them to the
//! feature implementations as closures.

use super::blocks::{
    BlockPredicate, BlockSet, Direction, JavaBlock, JavaState, RuleBasedStateProvider, RuleTest,
    StateProvider,
};
use super::values::{FloatProvider, IntProvider};

#[derive(Debug, Clone, Copy)]
pub struct OreTarget {
    pub target: RuleTest,
    pub state: JavaState,
}

/// `ore` and `scattered_ore`.
#[derive(Debug, Clone, Copy)]
pub struct OreConfig {
    pub targets: &'static [OreTarget],
    pub size: i32,
    pub discard_chance_on_air_exposure: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct SimpleBlockConfig {
    pub to_place: StateProvider,
    pub schedule_tick: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct DiskConfig {
    pub state_provider: RuleBasedStateProvider,
    pub target: BlockPredicate,
    pub radius: IntProvider,
    pub half_height: i32,
}

#[derive(Debug, Clone, Copy)]
pub struct LakeConfig {
    pub fluid: StateProvider,
    pub barrier: StateProvider,
}

#[derive(Debug, Clone, Copy)]
pub struct SpringConfig {
    pub state: JavaState,
    pub requires_block_below: bool,
    pub rock_count: i32,
    pub hole_count: i32,
    pub valid_blocks: BlockSet,
}

#[derive(Debug, Clone, Copy)]
pub struct ColumnLayer {
    pub height: IntProvider,
    pub provider: StateProvider,
}

#[derive(Debug, Clone, Copy)]
pub struct BlockColumnConfig {
    pub layers: &'static [ColumnLayer],
    pub direction: Direction,
    pub allowed_placement: BlockPredicate,
    pub prioritize_tip: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaveSurface {
    Ceiling,
    Floor,
}

impl CaveSurface {
    /// Direction from the patch towards the surface it grows on.
    #[inline]
    pub fn direction(self) -> Direction {
        match self {
            CaveSurface::Ceiling => Direction::Up,
            CaveSurface::Floor => Direction::Down,
        }
    }
}

/// `vegetation_patch` and `waterlogged_vegetation_patch`.
#[derive(Debug, Clone, Copy)]
pub struct VegetationPatchConfig {
    pub replaceable: BlockSet,
    pub ground_state: StateProvider,
    pub surface: CaveSurface,
    pub depth: IntProvider,
    pub extra_bottom_block_chance: f32,
    pub vertical_range: i32,
    pub vegetation_chance: f32,
    pub xz_radius: IntProvider,
    pub extra_edge_column_chance: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct HugeMushroomConfig {
    pub cap_provider: StateProvider,
    pub stem_provider: StateProvider,
    pub foliage_radius: i32,
}

#[derive(Debug, Clone, Copy)]
pub struct HugeFungusConfig {
    pub valid_base_block: JavaState,
    pub stem_state: JavaState,
    pub hat_state: JavaState,
    pub decor_state: JavaState,
    pub replaceable_blocks: BlockPredicate,
    pub planted: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct BlockPileConfig {
    pub state_provider: StateProvider,
}

#[derive(Debug, Clone, Copy)]
pub struct GeodeConfig {
    pub filling_provider: StateProvider,
    pub inner_layer_provider: StateProvider,
    pub alternate_inner_layer_provider: StateProvider,
    pub middle_layer_provider: StateProvider,
    pub outer_layer_provider: StateProvider,
    pub inner_placements: &'static [JavaState],
    pub cannot_replace: BlockSet,
    pub invalid_blocks: BlockSet,
    pub filling: f64,
    pub inner_layer: f64,
    pub middle_layer: f64,
    pub outer_layer: f64,
    pub generate_crack_chance: f64,
    pub base_crack_size: f64,
    pub crack_point_offset: i32,
    pub use_potential_placements_chance: f64,
    pub use_alternate_layer0_chance: f64,
    pub placements_require_layer0_alternate: bool,
    pub outer_wall_distance: IntProvider,
    pub distribution_points: IntProvider,
    pub point_offset: IntProvider,
    pub min_gen_offset: i32,
    pub max_gen_offset: i32,
    pub noise_multiplier: f64,
    pub invalid_blocks_threshold: i32,
}

#[derive(Debug, Clone, Copy)]
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

#[derive(Debug, Clone, Copy)]
pub struct LargeDripstoneConfig {
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

#[derive(Debug, Clone, Copy)]
pub struct PointedDripstoneConfig {
    pub chance_of_taller_dripstone: f32,
    pub chance_of_directional_spread: f32,
    pub chance_of_spread_radius2: f32,
    pub chance_of_spread_radius3: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct SculkPatchConfig {
    pub charge_count: i32,
    pub amount_per_charge: i32,
    pub spread_attempts: i32,
    pub growth_rounds: i32,
    pub spread_rounds: i32,
    pub extra_rare_growths: IntProvider,
    pub catalyst_chance: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct MultifaceGrowthConfig {
    pub block: JavaBlock,
    pub search_range: i32,
    pub can_place_on_floor: bool,
    pub can_place_on_ceiling: bool,
    pub can_place_on_wall: bool,
    pub chance_of_spreading: f32,
    pub can_be_placed_on: BlockSet,
    /// Directions this block may attach to, in Java's list order before
    /// shuffling: `Up` if it may hang from ceilings, `Down` if it may sit on
    /// floors, then the horizontal plane (N, E, S, W) if it may go on walls.
    pub valid_directions: &'static [Direction],
}

/// `fossil`: template ids (looked up in the embedded template table) and the
/// processor lists applied to the fossil and its ore overlay.
#[derive(Debug, Clone, Copy)]
pub struct FossilConfig {
    pub fossil_structures: &'static [&'static str],
    pub overlay_structures: &'static [&'static str],
    pub fossil_processors: &'static [crate::structures::processor::Processor],
    pub overlay_processors: &'static [crate::structures::processor::Processor],
    pub max_empty_corners_allowed: i32,
}

#[derive(Debug, Clone, Copy)]
pub struct RootSystemConfig {
    pub required_vertical_space_for_tree: i32,
    pub root_radius: i32,
    pub root_replaceable: BlockSet,
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

#[derive(Debug, Clone, Copy)]
pub struct UnderwaterMagmaConfig {
    pub floor_search_range: i32,
    pub placement_radius_around_floor: i32,
    pub placement_probability_per_valid_position: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct NetherForestVegetationConfig {
    pub state_provider: StateProvider,
    pub spread_width: i32,
    pub spread_height: i32,
}

#[derive(Debug, Clone, Copy)]
pub struct TwistingVinesConfig {
    pub spread_width: i32,
    pub spread_height: i32,
    pub max_height: i32,
}

#[derive(Debug, Clone, Copy)]
pub struct ReplaceBlobsConfig {
    pub target: JavaState,
    pub state: JavaState,
    pub radius: IntProvider,
}

#[derive(Debug, Clone, Copy)]
pub struct ColumnConfig {
    pub reach: IntProvider,
    pub height: IntProvider,
}

#[derive(Debug, Clone, Copy)]
pub struct DeltaConfig {
    pub contents: JavaState,
    pub rim: JavaState,
    pub size: IntProvider,
    pub rim_size: IntProvider,
}

#[derive(Debug, Clone, Copy)]
pub struct EndGatewayConfig {
    pub exit: Option<[i32; 3]>,
    pub exact: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct EndSpike {
    pub center_x: i32,
    pub center_z: i32,
    pub radius: i32,
    pub height: i32,
    pub guarded: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct EndSpikeConfig {
    pub crystal_invulnerable: bool,
    pub spikes: &'static [EndSpike],
    pub crystal_beam_target: Option<[i32; 3]>,
}

#[derive(Debug, Clone, Copy)]
pub struct FallenTreeConfig {
    pub trunk_provider: StateProvider,
    pub log_length: IntProvider,
    pub stump_decorators: &'static [TreeDecorator],
    pub log_decorators: &'static [TreeDecorator],
}

// ---- trees ---------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct TreeConfig {
    pub trunk_provider: StateProvider,
    pub trunk_placer: TrunkPlacer,
    pub foliage_provider: StateProvider,
    pub foliage_placer: FoliagePlacer,
    pub root_placer: Option<RootPlacer>,
    pub dirt_provider: StateProvider,
    pub minimum_size: FeatureSize,
    pub decorators: &'static [TreeDecorator],
    pub ignore_vines: bool,
    pub force_dirt: bool,
}

/// Shared trunk placer heights (`base_height + nextInt(a + 1) + nextInt(b + 1)`).
#[derive(Debug, Clone, Copy)]
pub struct TrunkHeights {
    pub base_height: i32,
    pub height_rand_a: i32,
    pub height_rand_b: i32,
}

#[derive(Debug, Clone, Copy)]
pub enum TrunkPlacer {
    Straight(TrunkHeights),
    Forking(TrunkHeights),
    Giant(TrunkHeights),
    MegaJungle(TrunkHeights),
    DarkOak(TrunkHeights),
    Fancy(TrunkHeights),
    Bending {
        heights: TrunkHeights,
        min_height_for_leaves: i32,
        bend_length: IntProvider,
    },
    UpwardsBranching {
        heights: TrunkHeights,
        extra_branch_steps: IntProvider,
        place_branch_per_log_probability: f32,
        extra_branch_length: IntProvider,
        can_grow_through: BlockSet,
    },
    Cherry {
        heights: TrunkHeights,
        branch_count: IntProvider,
        branch_horizontal_length: IntProvider,
        branch_start_offset_from_top: (i32, i32),
        branch_end_offset_from_top: IntProvider,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum FoliagePlacer {
    Blob {
        radius: IntProvider,
        offset: IntProvider,
        height: i32,
    },
    Bush {
        radius: IntProvider,
        offset: IntProvider,
        height: i32,
    },
    Fancy {
        radius: IntProvider,
        offset: IntProvider,
        height: i32,
    },
    Jungle {
        radius: IntProvider,
        offset: IntProvider,
        height: i32,
    },
    Spruce {
        radius: IntProvider,
        offset: IntProvider,
        trunk_height: IntProvider,
    },
    Pine {
        radius: IntProvider,
        offset: IntProvider,
        height: IntProvider,
    },
    Acacia {
        radius: IntProvider,
        offset: IntProvider,
    },
    DarkOak {
        radius: IntProvider,
        offset: IntProvider,
    },
    MegaPine {
        radius: IntProvider,
        offset: IntProvider,
        crown_height: IntProvider,
    },
    RandomSpread {
        radius: IntProvider,
        offset: IntProvider,
        foliage_height: IntProvider,
        leaf_placement_attempts: i32,
    },
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

#[derive(Debug, Clone, Copy)]
pub enum RootPlacer {
    Mangrove {
        trunk_offset_y: IntProvider,
        root_provider: StateProvider,
        above_root_placement: Option<(StateProvider, f32)>,
        can_grow_through: BlockSet,
        muddy_roots_in: BlockSet,
        muddy_roots_provider: StateProvider,
        max_root_width: i32,
        max_root_length: i32,
        random_skew_chance: f32,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum FeatureSize {
    TwoLayers {
        limit: i32,
        lower_size: i32,
        upper_size: i32,
        min_clipped_height: Option<i32>,
    },
    ThreeLayers {
        limit: i32,
        upper_limit: i32,
        lower_size: i32,
        middle_size: i32,
        upper_size: i32,
        min_clipped_height: Option<i32>,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum TreeDecorator {
    TrunkVine,
    LeaveVine {
        probability: f32,
    },
    Cocoa {
        probability: f32,
    },
    Beehive {
        probability: f32,
    },
    AlterGround {
        provider: StateProvider,
    },
    AttachedToLeaves {
        probability: f32,
        exclusion_radius_xz: i32,
        exclusion_radius_y: i32,
        block_provider: StateProvider,
        required_empty_blocks: i32,
        directions: &'static [Direction],
    },
    AttachedToLogs {
        probability: f32,
        block_provider: StateProvider,
        directions: &'static [Direction],
    },
    PlaceOnGround {
        tries: i32,
        radius: i32,
        height: i32,
        block_state_provider: StateProvider,
    },
    CreakingHeart {
        probability: f32,
    },
    PaleMoss {
        leaves_probability: f32,
        trunk_probability: f32,
        ground_probability: f32,
    },
}
