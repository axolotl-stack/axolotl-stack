//! `minecraft:tree_feature`: a trunk (one of eight kinds), an optional
//! canopy (one of eleven kinds, plus a second canopy kind at branch ends),
//! optional mangrove prop roots, ground conversion and decorations.
//!
//! # Placement
//! 1. The block below `pos` must match `may_grow_on`.
//! 2. Mangrove roots (if any) raise the trunk origin by `y_offset` and are
//!    simulated down to the ground; a root that never finds ground within
//!    `max_root_length` fails the tree (vanilla mangrove behaviour).
//! 3. The trunk and all of its branches are planned into a stack buffer and
//!    every planned log cell is checked: air, `may_replace` or
//!    `may_grow_through`, inside the build height. Any failure places
//!    nothing and returns `None`.
//! 4. Then, in order: `base_block` under the trunk footprint (kept if the
//!    ground already is one of the listed blocks, else the first entry is
//!    placed, so coarse dirt survives under a tree while grass becomes dirt),
//!    roots, logs, canopies, canopy decorations (vines, hanging blocks),
//!    trunk decorations, blocks above roots, `base_cluster` patches and
//!    finally `log_decoration_feature` on the flagged logs.
//!
//! Canopy leaves are placed only into air or `may_replace` cells and are not
//! part of the validity check: vanilla trees of both editions only require a
//! free trunk, and leaves of neighbouring trees simply wrap around each
//! other's logs (a strict canopy check would empty dense forests).
//!
//! # Data conventions (applied by the code generator)
//! - `{"range_min": a, "range_max": b}` is `[a, b)`: every vanilla tree
//!   range then reproduces the classic generator formulas exactly (oak
//!   `4..7` = 4..=6 logs, spruce modifier `-2..1` = 0..=2 logs shorter, mega
//!   spruce canopy `13..18` = 13..=17 layers, acacia lean `1..5` = 1..=4,
//!   mangrove `y_offset` `1..4` = 1..=3). `{"min", "max"}` is inclusive and a
//!   constant is a fixed value. The runtime only sees inclusive [`IntRange`]s.
//! - Bare numbers in `*_chance` keys are percentages (`75.0`, `33.33`,
//!   `100.0`), except `poplar_canopy.side_hole_chance`, which is a
//!   probability in `[0, 1]` (`0.15`).
//!
//! Geometry and random draw order are unmeasured: to confirm by BDS fixture.

use super::super::generated::{bb, props};
use super::super::rng::BedrockRandom;
use super::super::world::{BlockPos, BlockWorld, Ctx, Direction};
use super::{Chance, IntRange};
use std::f32::consts::TAU;

// ---------------------------------------------------------------------------
// Parameters (emitted as `static` literals by the code generator)
// ---------------------------------------------------------------------------

/// Pillar axis of a log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    Z,
}

/// A log block in each pillar axis (`pillar_axis` x/y/z; all three equal for
/// blocks without that state).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Log {
    /// Lying east-west.
    pub x: u32,
    /// Upright.
    pub y: u32,
    /// Lying north-south.
    pub z: u32,
}

impl Log {
    /// The state lying along `axis`.
    #[inline]
    pub const fn along(self, axis: Axis) -> u32 {
        match axis {
            Axis::X => self.x,
            Axis::Y => self.y,
            Axis::Z => self.z,
        }
    }
}

/// A block attached to the side of its support (vines), one state per
/// direction from the block to its support, indexed in [`Direction`] order
/// (south, west, north, east). Blocks without a facing repeat one state.
pub type SideStates = [u32; 4];

/// `trunk_decoration`: each free side of a trunk log gets `block` with
/// `chance` (jungle vines).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrunkDecoration {
    /// Attached block per support direction.
    pub block: SideStates,
    /// Per-side chance.
    pub chance: Chance,
}

/// `canopy_decoration`: each free side of a leaf starts a hanging column of
/// up to `num_steps` blocks with `chance`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CanopyDecoration {
    /// Attached block per support direction.
    pub block: SideStates,
    /// Per-side chance.
    pub chance: Chance,
    /// Maximum column length.
    pub num_steps: i32,
    /// Column direction: `-1` down, `1` up.
    pub step: i32,
}

/// `trunk_height` (and the other trunk length draws).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Height {
    /// Uniform in the range.
    Range(IntRange),
    /// `base + sum(next_int(interval))` (`{"base", "intervals"}`), the classic
    /// `base + rand(a) + rand(b)` tree heights.
    Intervals {
        base: i32,
        intervals: &'static [i32],
    },
    /// `base + next_int(a + 1) + next_int(b + 1)`
    /// (`{"base", "height_rand_a", "height_rand_b"}`, both bounds inclusive).
    Rand { base: i32, a: i32, b: i32 },
}

impl Height {
    /// Draw a height.
    pub fn sample(self, rng: &mut BedrockRandom) -> i32 {
        match self {
            Height::Range(r) => r.sample(rng),
            Height::Intervals { base, intervals } => {
                base + intervals.iter().map(|&n| rng.next_int(n)).sum::<i32>()
            }
            Height::Rand { base, a, b } => base + rng.next_int(a + 1) + rng.next_int(b + 1),
        }
    }
}

/// Everything one tree document compiles to.
#[derive(Debug, Clone, Copy)]
pub struct Params {
    /// Ground the tree may stand on (block below `pos`).
    pub may_grow_on: fn(u32) -> bool,
    /// Blocks logs and leaves may replace.
    pub may_replace: fn(u32) -> bool,
    /// Additional blocks logs (not leaves) may replace.
    pub may_grow_through: fn(u32) -> bool,
    /// Ground under the trunk: kept if already listed, else the first entry.
    pub base_block: &'static [u32],
    /// Patches of `base_block[0]` around the trunk (mega spruce podzol).
    pub base_cluster: Option<BaseCluster>,
    /// Mangrove prop roots.
    pub roots: Option<MangroveRoots>,
    /// Trunk shape.
    pub trunk: Trunk,
    /// Main canopy (none for fallen trees).
    pub canopy: Option<Canopy>,
    /// Whether `log_decoration_feature` exists (the closure of [`place`]
    /// is called on every flagged log).
    pub log_decoration: bool,
}

/// `base_cluster`: `num_clusters` discs of radius `radius` (corners cut)
/// around the trunk; per column the first `may_replace` block from two above
/// to three below the trunk base becomes `base_block[0]`.
#[derive(Debug, Clone, Copy)]
pub struct BaseCluster {
    /// Ground the patches convert.
    pub may_replace: fn(u32) -> bool,
    /// Number of patches.
    pub num_clusters: i32,
    /// Patch radius.
    pub radius: i32,
}

/// `mangrove_roots`.
#[derive(Debug, Clone, Copy)]
pub struct MangroveRoots {
    /// Manhattan distance from the trunk where roots stop spreading.
    pub max_width: i32,
    /// Maximum root steps (and root cells per side) before the tree fails.
    pub max_length: i32,
    /// Height of the trunk origin above `pos`.
    pub y_offset: IntRange,
    /// Root block.
    pub root: u32,
    /// Root block replacing `mud`.
    pub muddy_root: u32,
    /// The block roots turn muddy in.
    pub mud: u32,
    /// Block placed above roots and its per-root chance.
    pub above_root: Option<(u32, Chance)>,
    /// Blocks roots may replace besides air and `may_replace`.
    pub may_grow_through: fn(u32) -> bool,
}

/// Trunk kinds.
#[derive(Debug, Clone, Copy)]
pub enum Trunk {
    /// `trunk`: a straight column.
    Straight(StraightTrunk),
    /// `fancy_trunk`: fancy oak with branch limbs.
    Fancy(FancyTrunk),
    /// `mega_trunk`: a wide column with optional side branches.
    Mega(MegaTrunk),
    /// `acacia_trunk`: leaning trunk with forks or hanging stubs.
    Acacia(AcaciaTrunk),
    /// `mangrove_trunk`: column with upward diagonal branches.
    Mangrove(MangroveTrunk),
    /// `cherry_trunk`: one or two bent branches.
    Cherry(CherryTrunk),
    /// `poplar_trunk`: column with foliage support stubs.
    Poplar(PoplarTrunk),
    /// `fallen_trunk`: stump plus a log lying on the ground.
    Fallen(FallenTrunk),
}

/// `trunk`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StraightTrunk {
    /// Log count.
    pub height: Height,
    /// Added to the drawn height.
    pub height_modifier: IntRange,
    /// Log block.
    pub log: Log,
    /// Trunk vines.
    pub decoration: Option<TrunkDecoration>,
    /// `can_be_submerged.max_depth`: deepest water the base may stand in.
    pub max_water_depth: Option<i32>,
}

/// `fancy_trunk` (the classic fancy oak generator).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FancyTrunk {
    /// Tree height limit: `base + next_int(variance)`.
    pub base: i32,
    /// See `base`.
    pub variance: i32,
    /// Trunk height as a fraction of the limit.
    pub scale: f32,
    /// Trunk footprint width.
    pub width: i32,
    /// Log block.
    pub log: Log,
    /// Branch drop per horizontal block.
    pub branch_slope: f32,
    /// Branch density (branches per layer grow with it).
    pub branch_density: f32,
    /// Branches start no lower than this fraction of the limit.
    pub min_altitude_factor: f32,
    /// Horizontal reach multiplier.
    pub width_scale: f32,
    /// Foliage clusters start above this fraction of the limit.
    pub foliage_altitude_factor: f32,
    /// Height of one foliage cluster (the `fancy_canopy.height`).
    pub foliage_height: i32,
}

/// `mega_trunk`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MegaTrunk {
    /// Log layers.
    pub height: Height,
    /// Footprint width.
    pub width: i32,
    /// Log block.
    pub log: Log,
    /// Trunk vines.
    pub decoration: Option<TrunkDecoration>,
    /// Side branches.
    pub branches: Option<MegaBranches>,
}

/// `mega_trunk.branches`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MegaBranches {
    /// Logs per branch.
    pub length: i32,
    /// Rise per horizontal step.
    pub slope: f32,
    /// Vertical gap between branches.
    pub interval: IntRange,
    /// Lowest branch height as a fraction of the trunk.
    pub altitude_min: f32,
    /// Highest branch height as a fraction of the trunk (before the first gap).
    pub altitude_max: f32,
    /// Canopy at each branch end.
    pub canopy: &'static Canopy,
}

/// `acacia_trunk`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AcaciaTrunk {
    /// Log layers.
    pub height: Height,
    /// `trunk_height.min_height_for_canopy`: every log from this layer up (and
    /// every bend log) carries a canopy instead of one canopy at the top.
    pub min_height_for_canopy: Option<i32>,
    /// Footprint width.
    pub width: i32,
    /// Log block.
    pub log: Log,
    /// Lean of the upper trunk.
    pub lean: Option<Lean>,
    /// Trunk vines.
    pub decoration: Option<TrunkDecoration>,
    /// Forks (width 1) or hanging stubs around the top (wider trunks).
    pub branches: Option<AcaciaBranches>,
}

/// `acacia_trunk.trunk_lean`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lean {
    /// Diagonal (edge-only) log connections allowed.
    pub allow_diagonal: bool,
    /// Layers from the top where the lean starts.
    pub height: IntRange,
    /// Horizontal shifts (one per layer).
    pub steps: IntRange,
    /// Horizontal bend logs above the top.
    pub length: Option<IntRange>,
}

/// `acacia_trunk.branches`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AcaciaBranches {
    /// Fork chance (width 1) or chance per ring cell (wider trunks).
    pub chance: Chance,
    /// Fork steps or stub length.
    pub length: IntRange,
    /// Fork start below the lean start, or stub top below the trunk top.
    pub position: IntRange,
    /// Canopy at each branch.
    pub canopy: &'static Canopy,
}

/// `mangrove_trunk`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MangroveTrunk {
    /// Log layers.
    pub height: Height,
    /// Footprint width.
    pub width: i32,
    /// Log block.
    pub log: Log,
    /// Upward branches.
    pub branches: Option<MangroveBranches>,
}

/// `mangrove_trunk.branches`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MangroveBranches {
    /// Chance per trunk log below the top.
    pub chance: Chance,
    /// Vertical offset of the branch start above its log.
    pub length: IntRange,
    /// Diagonal steps (one log each).
    pub steps: IntRange,
}

/// `cherry_trunk`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CherryTrunk {
    /// Tree height.
    pub height: Height,
    /// Log block.
    pub log: Log,
    /// Branch layout.
    pub branches: CherryBranches,
}

/// `cherry_trunk.branches`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CherryBranches {
    /// Weights of one branch, two branches, two branches plus a trunk crown.
    pub weights: [i32; 3],
    /// Horizontal reach of a branch.
    pub horizontal_length: IntRange,
    /// Branch starts relative to the top (both bounds are used).
    pub start_offset: IntRange,
    /// Branch end height relative to the top.
    pub end_offset: IntRange,
    /// Canopy at each branch end.
    pub canopy: &'static Canopy,
}

/// `poplar_trunk`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PoplarTrunk {
    /// Log count.
    pub height: Height,
    /// Log block.
    pub log: Log,
    /// Trunk logs above the support branches.
    pub remaining_above_branches: IntRange,
    /// Number of support stubs.
    pub support_branches: IntRange,
}

/// `fallen_trunk`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FallenTrunk {
    /// Lying log length.
    pub length: IntRange,
    /// Added to the length.
    pub height_modifier: IntRange,
    /// Log block.
    pub log: Log,
    /// Stump vines.
    pub decoration: Option<TrunkDecoration>,
}

/// Canopy kinds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Canopy {
    /// `canopy`: the classic layered blob (oak, birch, jungle, swamp, bush).
    Blob(BlobCanopy),
    /// `fancy_canopy`: spherical clusters at fancy oak branch ends.
    Fancy(FancyCanopy),
    /// `mega_canopy`: discs widening downwards (mega jungle).
    Mega(MegaCanopy),
    /// `mega_pine_canopy`: tall cone (mega spruce/pine).
    MegaPine(MegaPineCanopy),
    /// `pine_canopy`: small cone at the top.
    Pine(PineCanopy),
    /// `spruce_canopy`: alternating rings.
    Spruce(SpruceCanopy),
    /// `roofed_canopy`: dark/pale oak roof.
    Roofed(RoofedCanopy),
    /// `acacia_canopy`: flat two-layer crown.
    Acacia(AcaciaCanopy),
    /// `random_spread_canopy` / `mangrove_canopy`: random leaf scatter.
    RandomSpread(RandomSpreadCanopy),
    /// `cherry_canopy`: wide crown with hanging leaves.
    Cherry(CherryCanopy),
    /// `poplar_canopy`: narrow column.
    Poplar(PoplarCanopy),
}

/// `canopy`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlobCanopy {
    /// Lowest layer relative to the attachment (one above the top log).
    pub offset_min: i32,
    /// Highest layer.
    pub offset_max: i32,
    /// Radius of the top layer.
    pub min_width: i32,
    /// Radius growth per `run` layers downwards.
    pub rise: i32,
    /// See `rise`.
    pub run: i32,
    /// Chance to drop a layer's corner leaves, bottom layer first (the last
    /// entry repeats).
    pub variation: &'static [Chance],
    /// Leaf block.
    pub leaf: u32,
    /// Hanging vines.
    pub decoration: Option<CanopyDecoration>,
}

/// `fancy_canopy`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FancyCanopy {
    /// Cluster height.
    pub height: i32,
    /// Radius of the middle layers (end layers one less).
    pub radius: i32,
    /// Leaf block.
    pub leaf: u32,
}

/// `mega_canopy`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MegaCanopy {
    /// Layer count.
    pub height: IntRange,
    /// Top-layer radius beyond the core.
    pub base_radius: i32,
    /// Core width (the trunk footprint).
    pub core_width: i32,
    /// Plain discs, without the large-radius corner trim.
    pub simplify: bool,
    /// Leaf block.
    pub leaf: u32,
}

/// `mega_pine_canopy`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MegaPineCanopy {
    /// Layers below the tip.
    pub height: IntRange,
    /// Tip radius.
    pub base_radius: i32,
    /// Radius gained over the full height.
    pub radius_step: f32,
    /// Core width.
    pub core_width: i32,
    /// Leaf block.
    pub leaf: u32,
}

/// `pine_canopy`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PineCanopy {
    /// Minimum widest radius (plus `0..=height` at random).
    pub base_radius: i32,
    /// Layers below the tip.
    pub height: IntRange,
    /// Leaf block.
    pub leaf: u32,
}

/// `spruce_canopy`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpruceCanopy {
    /// Bare trunk layers at the bottom.
    pub lower_offset: IntRange,
    /// Leaf layers above the attachment.
    pub upper_offset: IntRange,
    /// Widest ring.
    pub max_radius: IntRange,
    /// Leaf block.
    pub leaf: u32,
}

/// `roofed_canopy`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoofedCanopy {
    /// Layer count (from one below the top log).
    pub height: i32,
    /// Core width.
    pub core_width: i32,
    /// Radius of the lower and upper layers.
    pub outer_radius: i32,
    /// Radius of the main roof layer (top log level).
    pub inner_radius: i32,
    /// Leaf block.
    pub leaf: u32,
}

/// `acacia_canopy`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AcaciaCanopy {
    /// Radius of the bottom layer.
    pub size: i32,
    /// Omit the four outrigger leaves of the top layer.
    pub simplify: bool,
    /// Leaf block.
    pub leaf: u32,
}

/// `random_spread_canopy` / `mangrove_canopy`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RandomSpreadCanopy {
    /// Vertical spread: offsets `next_int(h) - next_int(h)`.
    pub height: i32,
    /// Horizontal spread: offsets `next_int(r) - next_int(r)`.
    pub radius: i32,
    /// Leaves attempted per attachment.
    pub attempts: i32,
    /// Weighted leaf blocks.
    pub leaves: &'static [(u32, i32)],
    /// Hanging vines.
    pub decoration: Option<CanopyDecoration>,
    /// Block hung under leaves and its chance per leaf.
    pub hanging: Option<(u32, Chance)>,
}

/// `cherry_canopy`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CherryCanopy {
    /// Layer count.
    pub height: i32,
    /// Radius (the widest rows use `radius - 1`).
    pub radius: i32,
    /// Probability to drop edge leaves of the wide bottom row.
    pub wide_bottom_hole: f32,
    /// Probability to drop leaves near the corners.
    pub corner_hole: f32,
    /// Probability of a hanging leaf under an edge leaf.
    pub hanging: f32,
    /// Probability the hanging leaf grows one more block.
    pub hanging_extension: f32,
    /// Leaf block.
    pub leaf: u32,
}

/// `poplar_canopy`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PoplarCanopy {
    /// Weighted `(value, weight)` cross-section sizes: a cell belongs to a
    /// full layer when `dx² + dz² <= value`.
    pub radius: &'static [(i32, i32)],
    /// Full-width layers.
    pub height: IntRange,
    /// Probability to drop an outer leaf of a full layer.
    pub side_hole: f32,
    /// Leaf block.
    pub leaf: u32,
    /// Log of the foliage support stubs.
    pub branch: Log,
}

impl Trunk {
    fn width(&self) -> i32 {
        match self {
            Trunk::Fancy(t) => t.width,
            Trunk::Mega(t) => t.width,
            Trunk::Acacia(t) => t.width,
            Trunk::Mangrove(t) => t.width,
            Trunk::Straight(_) | Trunk::Cherry(_) | Trunk::Poplar(_) | Trunk::Fallen(_) => 1,
        }
    }

    fn decoration(&self) -> Option<TrunkDecoration> {
        match self {
            Trunk::Straight(t) => t.decoration,
            Trunk::Mega(t) => t.decoration,
            Trunk::Acacia(t) => t.decoration,
            Trunk::Fallen(t) => t.decoration,
            Trunk::Fancy(_) | Trunk::Mangrove(_) | Trunk::Cherry(_) | Trunk::Poplar(_) => None,
        }
    }

    fn branch_canopy(&self) -> Option<&'static Canopy> {
        match self {
            Trunk::Mega(t) => t.branches.map(|b| b.canopy),
            Trunk::Acacia(t) => t.branches.map(|b| b.canopy),
            Trunk::Cherry(t) => Some(t.branches.canopy),
            _ => None,
        }
    }
}

impl Canopy {
    /// Whether leaves get a post pass (vines, hanging blocks).
    fn decorated(&self) -> bool {
        let (vines, hanging) = self.decorations();
        vines.is_some() || hanging.is_some()
    }

    fn decorations(&self) -> (Option<CanopyDecoration>, Option<(u32, Chance)>) {
        match self {
            Canopy::Blob(c) => (c.decoration, None),
            Canopy::RandomSpread(c) => (c.decoration, c.hanging),
            _ => (None, None),
        }
    }

    fn is_leaf(&self, b: u32) -> bool {
        match self {
            Canopy::Blob(c) => c.leaf == b,
            Canopy::Fancy(c) => c.leaf == b,
            Canopy::Mega(c) => c.leaf == b,
            Canopy::MegaPine(c) => c.leaf == b,
            Canopy::Pine(c) => c.leaf == b,
            Canopy::Spruce(c) => c.leaf == b,
            Canopy::Roofed(c) => c.leaf == b,
            Canopy::Acacia(c) => c.leaf == b,
            Canopy::RandomSpread(c) => c.leaves.iter().any(|&(l, _)| l == b),
            Canopy::Cherry(c) => c.leaf == b,
            Canopy::Poplar(c) => c.leaf == b,
        }
    }
}

// ---------------------------------------------------------------------------
// Planning buffers
// ---------------------------------------------------------------------------

/// Planned logs per tree. The largest vanilla trees (fancy oak with ~35
/// limbs, tall mangroves branching on every log, 2x2 mega jungle trunks up
/// to 31 layers) stay well below this; a plan that does not fit fails the
/// tree like an obstructed one.
const MAX_LOGS: usize = 1024;
/// Canopy attachments per tree (fancy oak nodes, azalea logs, branch ends).
const MAX_ATTACH: usize = 96;
/// Root cells: `max_root_length + 2` per side plus the column top.
const MAX_ROOTS: usize = 256;

/// Log gets `trunk_decoration`.
const DECO_VINES: u8 = 1;
/// Log gets `log_decoration_feature`.
const DECO_FEATURE: u8 = 2;

/// Vanilla mangrove roots turn sideways/diagonal with this probability (not
/// part of the pack data).
const ROOT_SKEW: f32 = 0.2;

/// Fixed-capacity stack vector.
struct Buf<T: Copy + Default, const N: usize> {
    items: [T; N],
    len: usize,
}

impl<T: Copy + Default, const N: usize> Buf<T, N> {
    fn new() -> Self {
        Self {
            items: [T::default(); N],
            len: 0,
        }
    }

    /// `None` when full.
    #[inline]
    fn push(&mut self, item: T) -> Option<()> {
        *self.items.get_mut(self.len)? = item;
        self.len += 1;
        Some(())
    }

    #[inline]
    fn len(&self) -> usize {
        self.len
    }

    #[inline]
    fn as_slice(&self) -> &[T] {
        &self.items[..self.len]
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct PlannedLog {
    pos: BlockPos,
    block: u32,
    flags: u8,
}

/// Where a canopy goes. `pos` is one above the top log for crowns (vanilla's
/// foliage attachment point), the cluster base for fancy oak nodes and the
/// log itself for azalea-style per-log canopies.
#[derive(Debug, Clone, Copy, Default)]
struct Attachment {
    pos: BlockPos,
    /// Uses the trunk's branch canopy instead of the main one.
    branch: bool,
}

struct Plan {
    logs: Buf<PlannedLog, MAX_LOGS>,
    attach: Buf<Attachment, MAX_ATTACH>,
}

impl Plan {
    fn new() -> Self {
        Self {
            logs: Buf::new(),
            attach: Buf::new(),
        }
    }

    #[inline]
    fn log(&mut self, pos: BlockPos, block: u32, flags: u8) -> Option<()> {
        self.logs.push(PlannedLog { pos, block, flags })
    }

    #[inline]
    fn attach(&mut self, pos: BlockPos, branch: bool) -> Option<()> {
        self.attach.push(Attachment { pos, branch })
    }
}

/// Places leaves into free cells and records their bounding box when the
/// canopy needs a decoration pass.
struct LeafSink {
    may_replace: fn(u32) -> bool,
    track: bool,
    bounds: Option<(BlockPos, BlockPos)>,
}

impl LeafSink {
    fn new(p: &Params, track: bool) -> Self {
        Self {
            may_replace: p.may_replace,
            track,
            bounds: None,
        }
    }

    /// Place `block` at `at` if the cell is air or `may_replace`.
    fn put<W: BlockWorld>(&mut self, ctx: &mut Ctx<'_, W>, at: BlockPos, block: u32) -> bool {
        let here = ctx.get(at);
        if !(bb::air(here) || (self.may_replace)(here)) || !ctx.set(at, block) {
            return false;
        }
        if self.track {
            self.bounds = Some(match self.bounds {
                None => (at, at),
                Some((lo, hi)) => (
                    BlockPos::new(lo.x.min(at.x), lo.y.min(at.y), lo.z.min(at.z)),
                    BlockPos::new(hi.x.max(at.x), hi.y.max(at.y), hi.z.max(at.z)),
                ),
            });
        }
        true
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Grow the tree described by `p` on the ground below `pos`. `decorate` is
/// the compiled `log_decoration_feature` (called on every flagged log when
/// `p.log_decoration` is set). Returns the trunk origin on success and places
/// nothing when the ground or a trunk cell rejects the tree.
pub fn place<W, F>(
    ctx: &mut Ctx<'_, W>,
    pos: BlockPos,
    p: &'static Params,
    mut decorate: F,
) -> Option<BlockPos>
where
    W: BlockWorld,
    F: FnMut(&mut Ctx<'_, W>, BlockPos) -> Option<BlockPos>,
{
    if !ctx.in_height(pos.y) || !(p.may_grow_on)(ctx.get(pos.below())) {
        return None;
    }
    let mut roots = Buf::<BlockPos, MAX_ROOTS>::new();
    let origin = match &p.roots {
        Some(r) => plan_roots(ctx, p, r, pos, &mut roots)?,
        None => pos,
    };
    let mut plan = Plan::new();
    plan_trunk(ctx, p, origin, &mut plan)?;
    let free = plan
        .logs
        .as_slice()
        .iter()
        .all(|l| ctx.in_height(l.pos.y) && can_log(p, ctx.get(l.pos)));
    if !free {
        return None;
    }

    match &p.roots {
        Some(_) => place_base(ctx, p, pos, 1),
        None => place_base(ctx, p, origin, p.trunk.width()),
    }
    if let Some(r) = &p.roots {
        for &at in roots.as_slice() {
            let here = ctx.get(at);
            if !can_root(p, r, here) {
                continue;
            }
            // Duplicates (two root paths meeting) keep their muddy state.
            let block = if here == r.mud || here == r.muddy_root {
                r.muddy_root
            } else {
                r.root
            };
            ctx.set(at, block);
        }
    }
    for l in plan.logs.as_slice() {
        ctx.set(l.pos, l.block);
    }

    let branch_canopy = p.trunk.branch_canopy();
    let mut main_leaves = LeafSink::new(p, p.canopy.as_ref().is_some_and(Canopy::decorated));
    let mut branch_leaves = LeafSink::new(p, branch_canopy.is_some_and(Canopy::decorated));
    for &a in plan.attach.as_slice() {
        let (canopy, sink) = if a.branch {
            (branch_canopy, &mut branch_leaves)
        } else {
            (p.canopy.as_ref(), &mut main_leaves)
        };
        if let Some(c) = canopy {
            place_canopy(ctx, sink, c, a, origin);
        }
    }
    if let (Some(c), Some(b)) = (p.canopy.as_ref(), main_leaves.bounds) {
        decorate_canopy(ctx, c, b);
    }
    if let (Some(c), Some(b)) = (branch_canopy, branch_leaves.bounds) {
        decorate_canopy(ctx, c, b);
    }

    if let Some(d) = p.trunk.decoration() {
        for l in plan.logs.as_slice() {
            if l.flags & DECO_VINES == 0 {
                continue;
            }
            for dir in Direction::ALL {
                if !d.chance.roll(ctx.rng) {
                    continue;
                }
                let side = l.pos.side(dir);
                if bb::air(ctx.get(side)) {
                    ctx.set(side, d.block[dir.opposite() as usize]);
                }
            }
        }
    }
    if let Some(MangroveRoots {
        above_root: Some((block, chance)),
        ..
    }) = &p.roots
    {
        for &at in roots.as_slice() {
            if chance.roll(ctx.rng) && bb::air(ctx.get(at.above())) {
                ctx.set(at.above(), *block);
            }
        }
    }
    if let Some(c) = &p.base_cluster {
        base_clusters(ctx, p, c, origin, p.trunk.width());
    }
    if p.log_decoration {
        for l in plan.logs.as_slice() {
            if l.flags & DECO_FEATURE != 0 {
                decorate(ctx, l.pos);
            }
        }
    }
    Some(origin)
}

#[inline]
fn can_log(p: &Params, b: u32) -> bool {
    bb::air(b) || (p.may_replace)(b) || (p.may_grow_through)(b)
}

#[inline]
fn can_root(p: &Params, r: &MangroveRoots, b: u32) -> bool {
    bb::air(b) || (p.may_replace)(b) || (r.may_grow_through)(b)
}

/// `base_block` under each footprint column of `at`.
fn place_base<W: BlockWorld>(ctx: &mut Ctx<'_, W>, p: &Params, at: BlockPos, width: i32) {
    let Some(&first) = p.base_block.first() else {
        return;
    };
    for dx in 0..width {
        for dz in 0..width {
            let below = at.offset(dx, -1, dz);
            if !p.base_block.contains(&ctx.get(below)) {
                ctx.set(below, first);
            }
        }
    }
}

/// `base_cluster` patches. Vanilla mega spruces convert four corner discs and
/// a few random edge discs within three blocks of the trunk; the pack only
/// gives the count, so the `num_clusters` centres are uniform over that same
/// area (unmeasured: to confirm by BDS fixture).
fn base_clusters<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    p: &Params,
    c: &BaseCluster,
    origin: BlockPos,
    width: i32,
) {
    let Some(&block) = p.base_block.first() else {
        return;
    };
    let r = c.radius;
    for _ in 0..c.num_clusters {
        let cx = ctx.rng.next_int_inclusive(-r - 1, width + r);
        let cz = ctx.rng.next_int_inclusive(-r - 1, width + r);
        for dx in -r..=r {
            for dz in -r..=r {
                if dx.abs() == r && dz.abs() == r {
                    continue;
                }
                let column = origin.offset(cx + dx, 0, cz + dz);
                for dy in (-3..=2).rev() {
                    let at = column.offset(0, dy, 0);
                    let here = ctx.get(at);
                    if (c.may_replace)(here) {
                        ctx.set(at, block);
                        break;
                    }
                    if dy < 0 && !bb::air(here) {
                        break;
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Roots
// ---------------------------------------------------------------------------

/// Mangrove roots: the trunk origin rises by `y_offset`; the column below it
/// must be root-passable, its top cell becomes a root, and from each side of
/// the trunk base a root walks outwards/downwards until it meets ground.
fn plan_roots<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    p: &Params,
    r: &MangroveRoots,
    pos: BlockPos,
    roots: &mut Buf<BlockPos, MAX_ROOTS>,
) -> Option<BlockPos> {
    let origin = pos.offset(0, r.y_offset.sample(ctx.rng).max(0), 0);
    for y in pos.y..origin.y {
        if !ctx.in_height(y) || !can_root(p, r, ctx.get(BlockPos::new(pos.x, y, pos.z))) {
            return None;
        }
    }
    if origin.y > pos.y {
        roots.push(origin.below())?;
    }
    for dir in Direction::ALL {
        let start = origin.side(dir);
        let begin = roots.len();
        root_walk(ctx, p, r, start, dir, origin, roots, begin, 0)?;
        roots.push(start)?;
    }
    Some(origin)
}

/// One root step: each passable candidate becomes a root and continues. A
/// root longer than `max_length` (steps or cells on this side) fails the tree.
#[allow(clippy::too_many_arguments)]
fn root_walk<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    p: &Params,
    r: &MangroveRoots,
    at: BlockPos,
    dir: Direction,
    centre: BlockPos,
    roots: &mut Buf<BlockPos, MAX_ROOTS>,
    begin: usize,
    length: i32,
) -> Option<()> {
    if length >= r.max_length || roots.len() - begin > r.max_length as usize {
        return None;
    }
    let (cands, n) = root_candidates(ctx.rng, r.max_width, at, dir, centre);
    for &c in &cands[..n] {
        if ctx.in_height(c.y) && can_root(p, r, ctx.get(c)) {
            roots.push(c)?;
            root_walk(ctx, p, r, c, dir, centre, roots, begin, length + 1)?;
        }
    }
    Some(())
}

/// Next root cells: straight down near the trunk with sideways steps, a
/// possible split near `max_width`, only down beyond it.
fn root_candidates(
    rng: &mut BedrockRandom,
    max_width: i32,
    at: BlockPos,
    dir: Direction,
    centre: BlockPos,
) -> ([BlockPos; 2], usize) {
    let below = at.below();
    let side = at.side(dir);
    let dist = manhattan(at, centre);
    if dist > max_width - 3 && dist <= max_width {
        if rng.next_float() < ROOT_SKEW {
            ([below, side.below()], 2)
        } else {
            ([below, below], 1)
        }
    } else if dist > max_width || rng.next_float() < ROOT_SKEW {
        ([below, below], 1)
    } else if rng.next_int(2) == 0 {
        ([side, side], 1)
    } else {
        ([below, below], 1)
    }
}

// ---------------------------------------------------------------------------
// Trunks
// ---------------------------------------------------------------------------

fn plan_trunk<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    p: &Params,
    origin: BlockPos,
    plan: &mut Plan,
) -> Option<()> {
    match &p.trunk {
        Trunk::Straight(t) => straight(ctx, t, origin, plan),
        Trunk::Fancy(t) => fancy(ctx, p, t, origin, plan),
        Trunk::Mega(t) => mega(ctx.rng, t, origin, plan),
        Trunk::Acacia(t) => acacia(ctx.rng, t, origin, plan),
        Trunk::Mangrove(t) => mangrove(ctx.rng, t, origin, plan),
        Trunk::Cherry(t) => cherry(ctx.rng, t, origin, plan),
        Trunk::Poplar(t) => poplar(ctx.rng, p, t, origin, plan),
        Trunk::Fallen(t) => fallen(ctx, p, t, origin, plan),
    }
}

#[inline]
fn random_dir(rng: &mut BedrockRandom) -> Direction {
    Direction::ALL[rng.next_int(4) as usize]
}

#[inline]
fn horizontal_axis(d: Direction) -> Axis {
    match d {
        Direction::East | Direction::West => Axis::X,
        Direction::North | Direction::South => Axis::Z,
    }
}

#[inline]
fn manhattan(a: BlockPos, b: BlockPos) -> i32 {
    (a.x - b.x).abs() + (a.y - b.y).abs() + (a.z - b.z).abs()
}

/// `width`×`width` logs with `at` as the min corner.
fn footprint(plan: &mut Plan, at: BlockPos, width: i32, block: u32, flags: u8) -> Option<()> {
    for dx in 0..width {
        for dz in 0..width {
            plan.log(at.offset(dx, 0, dz), block, flags)?;
        }
    }
    Some(())
}

/// Upright column of `height` layers. The bottom layer never gets vines
/// (classic jungle trunks start their vines one block up).
fn column(
    plan: &mut Plan,
    origin: BlockPos,
    height: i32,
    width: i32,
    block: u32,
    flags: u8,
) -> Option<()> {
    for i in 0..height {
        let f = if i == 0 { flags & !DECO_VINES } else { flags };
        footprint(plan, origin.offset(0, i, 0), width, block, f)?;
    }
    Some(())
}

/// Straight column; the crown sits one above the top log. With
/// `can_be_submerged` the base may stand in at most `max_depth` blocks of
/// liquid (swamp oaks).
fn straight<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    t: &StraightTrunk,
    origin: BlockPos,
    plan: &mut Plan,
) -> Option<()> {
    if let Some(max) = t.max_water_depth {
        let mut depth = 0;
        while depth <= max && props::is_liquid(ctx.get(origin.offset(0, depth, 0))) {
            depth += 1;
        }
        if depth > max {
            return None;
        }
    }
    let h = (t.height.sample(ctx.rng) + t.height_modifier.sample(ctx.rng)).max(1);
    column(plan, origin, h, 1, t.log.y, DECO_VINES)?;
    plan.attach(origin.offset(0, h, 0), false)
}

/// Fancy oak (the classic large oak generator):
/// - height limit `L = base + next_int(variance)`; if the trunk line is
///   blocked at index `i`, the tree fails for `i < 6` and shrinks to `i`;
/// - trunk height `floor(L * scale)` (at most `L - 1`);
/// - from `L - foliage_height` down, each layer above
///   `L * foliage_altitude_factor` gets `floor(1.382 + (density * L / 13)^2)`
///   cluster attempts at a random angle and a distance following a sphere
///   profile; a cluster needs a clear column and a clear line from the trunk;
/// - each accepted cluster above `L * min_altitude_factor` gets a limb
///   sloping up from the trunk by `branch_slope`, logs oriented along the
///   limb's dominant horizontal axis.
fn fancy<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    p: &Params,
    t: &FancyTrunk,
    origin: BlockPos,
    plan: &mut Plan,
) -> Option<()> {
    let fh = t.foliage_height.max(1);
    let mut limit = t.base + ctx.rng.next_int(t.variance);
    if let Some(i) = blocked_at(ctx, p, origin, origin.offset(0, limit - 1, 0)) {
        if i < 6 {
            return None;
        }
        limit = i;
    }
    let height = ((limit as f32 * t.scale) as i32).min(limit - 1).max(0);
    let trunk_top = origin.y + height;
    let per_layer = ((1.382 + (t.branch_density * limit as f32 / 13.0).powi(2)) as i32).max(1);
    plan.attach(origin.offset(0, limit - fh, 0), false)?;
    for k in (0..=limit - fh).rev() {
        let size = fancy_layer_size(limit, k, t.foliage_altitude_factor);
        if size < 0.0 {
            continue;
        }
        for _ in 0..per_layer {
            let reach = t.width_scale * size * (ctx.rng.next_float() + 0.328);
            let angle = ctx.rng.next_float() * TAU;
            let node = origin.offset(
                (reach * angle.sin() + 0.5).floor() as i32,
                k - 1,
                (reach * angle.cos() + 0.5).floor() as i32,
            );
            if blocked_at(ctx, p, node, node.offset(0, fh, 0)).is_some() {
                continue;
            }
            let (dx, dz) = (origin.x - node.x, origin.z - node.z);
            let base_f = node.y as f32 - ((dx * dx + dz * dz) as f32).sqrt() * t.branch_slope;
            let base_y = if base_f > trunk_top as f32 {
                trunk_top
            } else {
                base_f as i32
            };
            let base = BlockPos::new(origin.x, base_y, origin.z);
            if blocked_at(ctx, p, base, node).is_some() {
                continue;
            }
            plan.attach(node, false)?;
            if base != node && (base_y - origin.y) as f32 >= limit as f32 * t.min_altitude_factor {
                limb(plan, base, node, t.log)?;
            }
        }
    }
    for dx in 0..t.width {
        for dz in 0..t.width {
            let foot = origin.offset(dx, 0, dz);
            limb(plan, foot, foot.offset(0, height, 0), t.log)?;
        }
    }
    Some(())
}

/// Fancy oak cluster radius profile of layer `y`: `-1` below the foliage
/// band, else half the chord of a sphere spanning the height limit.
fn fancy_layer_size(limit: i32, y: i32, altitude: f32) -> f32 {
    if (y as f32) < limit as f32 * altitude {
        return -1.0;
    }
    let half = limit as f32 / 2.0;
    let dy = half - y as f32;
    let chord = if dy == 0.0 {
        half
    } else if dy.abs() >= half {
        return 0.0;
    } else {
        (half * half - dy * dy).sqrt()
    };
    chord * 0.5
}

/// Step count and delta of a block line from `a` to `b`.
#[inline]
fn line(a: BlockPos, b: BlockPos) -> ((i32, i32, i32), i32) {
    let d = (b.x - a.x, b.y - a.y, b.z - a.z);
    (d, d.0.abs().max(d.1.abs()).max(d.2.abs()))
}

/// Cell `j` of the line `a + d` sampled in `steps` steps (block centres
/// rounded down, as the classic fancy oak generator walks its limbs).
#[inline]
fn line_cell(a: BlockPos, d: (i32, i32, i32), steps: i32, j: i32) -> BlockPos {
    if steps == 0 {
        return a;
    }
    let at = |c: i32| (0.5 + j as f32 * (c as f32 / steps as f32)).floor() as i32;
    a.offset(at(d.0), at(d.1), at(d.2))
}

/// Index of the first cell of the line `a..=b` a log may not occupy.
fn blocked_at<W: BlockWorld>(
    ctx: &Ctx<'_, W>,
    p: &Params,
    a: BlockPos,
    b: BlockPos,
) -> Option<i32> {
    let (d, steps) = line(a, b);
    (0..=steps).find(|&j| {
        let c = line_cell(a, d, steps, j);
        !ctx.in_height(c.y) || !can_log(p, ctx.get(c))
    })
}

/// Logs along `a..=b`, each oriented by its offset from `a`: x or z when it
/// moved horizontally (the larger component), upright otherwise.
fn limb(plan: &mut Plan, a: BlockPos, b: BlockPos, log: Log) -> Option<()> {
    let (d, steps) = line(a, b);
    for j in 0..=steps {
        let c = line_cell(a, d, steps, j);
        let (ax, az) = ((c.x - a.x).abs(), (c.z - a.z).abs());
        let axis = match ax.max(az) {
            0 => Axis::Y,
            m if ax == m => Axis::X,
            _ => Axis::Z,
        };
        plan.log(c, log.along(axis), DECO_VINES)?;
    }
    Some(())
}

/// Wide column with a crown one above the top. Branches (mega jungle): from
/// `height * altitude_max - interval` down to `height * altitude_min`, every
/// `interval` layers, a `length`-log branch leaves the trunk centre at a
/// random angle, rising `slope` per block from three below its height; its
/// canopy sits at the branch end at the branch height.
fn mega(rng: &mut BedrockRandom, t: &MegaTrunk, origin: BlockPos, plan: &mut Plan) -> Option<()> {
    let h = t.height.sample(rng).max(1);
    column(plan, origin, h, t.width, t.log.y, DECO_VINES)?;
    plan.attach(origin.offset(0, h, 0), false)?;
    let Some(b) = &t.branches else {
        return Some(());
    };
    // Centre of the footprint as vanilla offsets it (1.5 for a 2x2 trunk).
    let centre = t.width as f32 - 0.5;
    let lowest = (h as f32 * b.altitude_min) as i32;
    let mut y = (h as f32 * b.altitude_max) as i32 - b.interval.sample(rng);
    while y > lowest {
        let (sin, cos) = (rng.next_float() * TAU).sin_cos();
        let (mut bx, mut bz) = (0, 0);
        for l in 0..b.length {
            bx = (centre + cos * l as f32) as i32;
            bz = (centre + sin * l as f32) as i32;
            plan.log(
                origin.offset(bx, y - 3 + (l as f32 * b.slope) as i32, bz),
                t.log.y,
                DECO_VINES,
            )?;
        }
        plan.attach(origin.offset(bx, y, bz), true)?;
        y -= b.interval.sample(rng).max(1);
    }
    Some(())
}

/// Acacia-style trunk (savanna acacia, dark/pale oak, azalea):
/// - the column shifts one block in a random direction per layer from
///   `height - lean_height` up, `lean_steps` times; without
///   `allow_diagonal_growth` a 1-wide trunk gets a connecting log at each
///   shift (wider footprints already overlap);
/// - `lean_length` continues the trunk above the top and then horizontally;
/// - with `min_height_for_canopy` every log from that layer up and every bend
///   log carries a canopy (azalea); otherwise one canopy sits above the end;
/// - width 1 branches: one fork (with `branch_chance`) in another direction,
///   starting `branch_position` below the lean start and climbing diagonally
///   for `branch_length` layers, crowned by the branch canopy (acacia);
/// - wider trunks: each cell of the ring around the top footprint grows, with
///   `branch_chance`, a stub of `branch_length` logs hanging from
///   `branch_position` below the top, crowned by the branch canopy (the log
///   stubs under dark oak roofs).
fn acacia(
    rng: &mut BedrockRandom,
    t: &AcaciaTrunk,
    origin: BlockPos,
    plan: &mut Plan,
) -> Option<()> {
    let h = t.height.sample(rng).max(1);
    let w = t.width;
    let block = t.log.y;
    let dir = random_dir(rng);
    let (dx, dz) = dir.delta();
    let (lean_start, mut steps, diagonal) = match t.lean {
        Some(l) => (
            h - l.height.sample(rng),
            l.steps.sample(rng),
            l.allow_diagonal,
        ),
        None => (h, 0, true),
    };
    let (mut x, mut z) = (0, 0);
    for i in 0..h {
        // The bottom layer never moves off its ground.
        if i >= lean_start.max(1) && steps > 0 {
            if !diagonal && w == 1 {
                plan.log(origin.offset(x, i, z), block, DECO_VINES)?;
            }
            x += dx;
            z += dz;
            steps -= 1;
        }
        let at = origin.offset(x, i, z);
        footprint(plan, at, w, block, if i == 0 { 0 } else { DECO_VINES })?;
        if t.min_height_for_canopy.is_some_and(|m| i >= m) {
            plan.attach(at, false)?;
        }
    }
    let mut end = origin.offset(x, h - 1, z);
    if let Some(len) = t.lean.and_then(|l| l.length) {
        let n = len.sample(rng);
        end = end.above();
        for k in 0..=n {
            if k > 0 {
                end = end.offset(dx, 0, dz);
            }
            footprint(plan, end, w, block, DECO_VINES)?;
            if t.min_height_for_canopy.is_some() {
                plan.attach(end, false)?;
            }
        }
    }
    if t.min_height_for_canopy.is_none() {
        plan.attach(end.above(), false)?;
    }
    let Some(b) = &t.branches else {
        return Some(());
    };
    if w == 1 {
        if !b.chance.roll(rng) {
            return Some(());
        }
        let fork = Direction::ALL[(dir as usize + 1 + rng.next_int(3) as usize) % 4];
        let (fx, fz) = fork.delta();
        let start = (lean_start - b.position.sample(rng)).max(1);
        let n = b.length.sample(rng);
        let (mut bx, mut bz) = (0, 0);
        let mut last = None;
        for k in 0..n {
            let layer = start + k;
            if layer >= h {
                break;
            }
            if !diagonal {
                plan.log(origin.offset(bx, layer, bz), block, DECO_VINES)?;
            }
            bx += fx;
            bz += fz;
            let at = origin.offset(bx, layer, bz);
            plan.log(at, block, DECO_VINES)?;
            last = Some(at);
        }
        if let Some(at) = last {
            plan.attach(at.above(), true)?;
        }
    } else {
        let top = h - 1;
        for rx in -1..=w {
            for rz in -1..=w {
                if (0..w).contains(&rx) && (0..w).contains(&rz) {
                    continue;
                }
                if !b.chance.roll(rng) {
                    continue;
                }
                let n = b.length.sample(rng);
                let stub_top = top - b.position.sample(rng);
                for k in 0..n {
                    plan.log(
                        origin.offset(x + rx, stub_top - k, z + rz),
                        block,
                        DECO_VINES,
                    )?;
                }
                plan.attach(origin.offset(x + rx, stub_top + 1, z + rz), true)?;
            }
        }
    }
    Some(())
}

/// Mangrove trunk: a column; each log below the top may (`branch_chance`)
/// start a branch `branch_length` above it that climbs diagonally for
/// `branch_steps` logs in a random direction. Every branch end and the top
/// carry the canopy.
fn mangrove(
    rng: &mut BedrockRandom,
    t: &MangroveTrunk,
    origin: BlockPos,
    plan: &mut Plan,
) -> Option<()> {
    let h = t.height.sample(rng).max(1);
    column(plan, origin, h, t.width, t.log.y, DECO_VINES)?;
    if let Some(b) = &t.branches {
        for i in 0..h - 1 {
            if !b.chance.roll(rng) {
                continue;
            }
            let (dx, dz) = random_dir(rng).delta();
            let mut at = origin.offset(0, i + b.length.sample(rng), 0);
            let steps = b.steps.sample(rng);
            for _ in 0..steps {
                at = at.offset(dx, 1, dz);
                plan.log(at, t.log.y, DECO_VINES)?;
            }
            if steps > 0 {
                plan.attach(at.above(), false)?;
            }
        }
    }
    plan.attach(origin.offset(0, h, 0), false)
}

/// Cherry trunk: branch starts at `height - 1 + start_offset.min` and
/// `.max` (at least one apart); the tree type decides one branch, two
/// opposite branches, or two branches plus the full trunk with the main
/// canopy on top. A branch leaves the trunk horizontally (one or two logs),
/// then walks towards its end, `horizontal_length` out and
/// `height - 1 + end_offset` up, choosing a vertical step with probability
/// `remaining height / remaining distance`.
fn cherry(
    rng: &mut BedrockRandom,
    t: &CherryTrunk,
    origin: BlockPos,
    plan: &mut Plan,
) -> Option<()> {
    let h = t.height.sample(rng).max(1);
    let b = &t.branches;
    let lo = (h - 1 + b.start_offset.min).max(0);
    let hi = (h - 1 + b.start_offset.max).max(0).max(lo + 1);
    let kind = pick(
        &[(0, b.weights[0]), (1, b.weights[1]), (2, b.weights[2])],
        rng,
    )
    .unwrap_or(0);
    let trunk_len = match kind {
        2 => h,
        1 => lo.max(hi) + 1,
        _ => lo + 1,
    };
    column(plan, origin, trunk_len, 1, t.log.y, DECO_VINES)?;
    if kind == 2 {
        plan.attach(origin.offset(0, trunk_len, 0), false)?;
    }
    let dir = random_dir(rng);
    cherry_branch(rng, t, origin, h, dir, lo, lo < trunk_len - 1, plan)?;
    if kind >= 1 {
        cherry_branch(
            rng,
            t,
            origin,
            h,
            dir.opposite(),
            hi,
            hi < trunk_len - 1,
            plan,
        )?;
    }
    Some(())
}

#[allow(clippy::too_many_arguments)]
fn cherry_branch(
    rng: &mut BedrockRandom,
    t: &CherryTrunk,
    origin: BlockPos,
    h: i32,
    dir: Direction,
    start: i32,
    middle_has_log: bool,
    plan: &mut Plan,
) -> Option<()> {
    let b = &t.branches;
    let end_y = h - 1 + b.end_offset.sample(rng);
    let through = middle_has_log || end_y < start;
    let reach = b.horizontal_length.sample(rng) + i32::from(through);
    let (dx, dz) = dir.delta();
    let target = origin.offset(dx * reach, end_y, dz * reach);
    let side_log = t.log.along(horizontal_axis(dir));
    let mut m = origin.offset(0, start, 0);
    for _ in 0..1 + i32::from(through) {
        m = m.side(dir);
        plan.log(m, side_log, DECO_VINES)?;
    }
    let vy = if target.y > m.y { 1 } else { -1 };
    // Each step moves one block closer; the bound only guards degenerate data
    // (a reach shorter than the initial horizontal logs).
    for _ in 0..64 {
        let dist = manhattan(m, target);
        if dist == 0 {
            return plan.attach(target.above(), true);
        }
        let f = (target.y - m.y).abs() as f32 / dist as f32;
        if rng.next_float() < f {
            m = m.offset(0, vy, 0);
            plan.log(m, t.log.y, DECO_VINES)?;
        } else {
            m = m.side(dir);
            plan.log(m, side_log, DECO_VINES)?;
        }
    }
    None
}

/// Poplar trunk: a column whose logs all receive `log_decoration_feature`,
/// with `amount_of_foliage_support_branches` one-log stubs in distinct
/// directions at `remaining_trunk_height_above_branches` below the top,
/// holding the bottom of the column canopy (stub length is not in the data;
/// one log keeps them inside the crown).
fn poplar(
    rng: &mut BedrockRandom,
    p: &Params,
    t: &PoplarTrunk,
    origin: BlockPos,
    plan: &mut Plan,
) -> Option<()> {
    let h = t.height.sample(rng).max(1);
    column(plan, origin, h, 1, t.log.y, DECO_VINES | DECO_FEATURE)?;
    let stub_log = match &p.canopy {
        Some(Canopy::Poplar(c)) => c.branch,
        _ => t.log,
    };
    let y = (h - 1 - t.remaining_above_branches.sample(rng)).max(1);
    let n = t.support_branches.sample(rng).clamp(0, 4) as usize;
    let mut dirs = Direction::ALL;
    for i in (1..dirs.len()).rev() {
        dirs.swap(i, rng.next_int(i as i32 + 1) as usize);
    }
    if y < h {
        for &d in &dirs[..n] {
            plan.log(
                origin.offset(0, y, 0).side(d),
                stub_log.along(horizontal_axis(d)),
                0,
            )?;
        }
    }
    plan.attach(origin.offset(0, h, 0), false)
}

/// Fallen tree: an upright stump at `pos` plus, if it fits, a log of
/// `log_length + height_modifier` blocks lying in a random direction starting
/// two or three blocks away. The log start settles on the first cell from one
/// above to four below that is free with solid ground under it; every log
/// cell must be free and no more than two consecutive cells may lack solid
/// ground. A log that does not fit leaves the stump alone (vanilla). Lying
/// logs receive `log_decoration_feature`; the stump gets the trunk vines.
fn fallen<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    p: &Params,
    t: &FallenTrunk,
    origin: BlockPos,
    plan: &mut Plan,
) -> Option<()> {
    plan.log(origin, t.log.y, DECO_VINES)?;
    let dir = random_dir(ctx.rng);
    let (dx, dz) = dir.delta();
    let len = t.length.sample(ctx.rng) + t.height_modifier.sample(ctx.rng);
    let gap = 2 + ctx.rng.next_int(2);
    let probe = origin.offset(dx * gap, 0, dz * gap);
    let Some(start) = (-4..=1).rev().map(|dy| probe.offset(0, dy, 0)).find(|&at| {
        ctx.in_height(at.y) && can_log(p, ctx.get(at)) && props::is_solid(ctx.get(at.below()))
    }) else {
        return Some(());
    };
    let mut unsupported = 0;
    for i in 0..len {
        let at = start.offset(dx * i, 0, dz * i);
        if !ctx.in_height(at.y) || !can_log(p, ctx.get(at)) {
            return Some(());
        }
        if props::is_solid(ctx.get(at.below())) {
            unsupported = 0;
        } else {
            unsupported += 1;
            if unsupported > 2 {
                return Some(());
            }
        }
    }
    let block = t.log.along(horizontal_axis(dir));
    for i in 0..len {
        plan.log(start.offset(dx * i, 0, dz * i), block, DECO_FEATURE)?;
    }
    Some(())
}

/// Weighted choice; zero total weight picks the first entry.
fn pick<T: Copy>(list: &[(T, i32)], rng: &mut BedrockRandom) -> Option<T> {
    let total: i32 = list.iter().map(|&(_, w)| w.max(0)).sum();
    if total <= 0 {
        return list.first().map(|&(v, _)| v);
    }
    pick_at(list, rng.next_int(total))
}

/// Entry whose cumulative weight range contains `roll`.
fn pick_at<T: Copy>(list: &[(T, i32)], mut roll: i32) -> Option<T> {
    for &(v, w) in list {
        let w = w.max(0);
        if roll < w {
            return Some(v);
        }
        roll -= w;
    }
    list.last().map(|&(v, _)| v)
}

// ---------------------------------------------------------------------------
// Canopies
// ---------------------------------------------------------------------------

fn place_canopy<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    sink: &mut LeafSink,
    c: &Canopy,
    a: Attachment,
    origin: BlockPos,
) {
    match c {
        Canopy::Blob(c) => blob(ctx, sink, c, a.pos),
        Canopy::Fancy(c) => fancy_cluster(ctx, sink, c, a.pos),
        Canopy::Mega(c) => mega_canopy(ctx, sink, c, a.pos),
        Canopy::MegaPine(c) => mega_pine(ctx, sink, c, a.pos),
        Canopy::Pine(c) => pine(ctx, sink, c, a.pos),
        Canopy::Spruce(c) => spruce(ctx, sink, c, a.pos, origin.y),
        Canopy::Roofed(c) => roofed(ctx, sink, c, a.pos),
        Canopy::Acacia(c) => acacia_canopy(ctx, sink, c, a.pos),
        Canopy::RandomSpread(c) => random_spread(ctx, sink, c, a.pos),
        Canopy::Cherry(c) => cherry_canopy(ctx, sink, c, a.pos),
        Canopy::Poplar(c) => poplar_canopy(ctx, sink, c, a.pos),
    }
}

/// Distance of offset `d` from a core of `width` cells starting at 0.
#[inline]
fn core_dist(d: i32, width: i32) -> i32 {
    if d < 0 {
        -d
    } else if d >= width {
        d - width + 1
    } else {
        0
    }
}

/// One horizontal leaf layer around a `core`×`core` column whose min corner
/// is `centre`, reaching `r` beyond the core; `keep(rng, dx, dz)` decides
/// each cell from its distances to the core.
#[allow(clippy::too_many_arguments)]
fn layer<W, K>(
    ctx: &mut Ctx<'_, W>,
    sink: &mut LeafSink,
    centre: BlockPos,
    core: i32,
    r: i32,
    block: u32,
    mut keep: K,
) where
    W: BlockWorld,
    K: FnMut(&mut BedrockRandom, i32, i32) -> bool,
{
    if r < 0 {
        return;
    }
    for dx in -r..r + core {
        for dz in -r..r + core {
            if keep(&mut *ctx.rng, core_dist(dx, core), core_dist(dz, core)) {
                sink.put(ctx, centre.offset(dx, 0, dz), block);
            }
        }
    }
}

/// Radius of the blob layer `depth` below the top.
#[inline]
fn blob_radius(min_width: i32, rise: i32, run: i32, depth: i32) -> i32 {
    min_width + depth / run.max(1) * rise
}

/// Classic blob: layers `offset_min..=offset_max` around the attachment,
/// radius `min_width` at the top growing by `rise` every `run` layers down;
/// each corner leaf is dropped with the layer's variation chance.
fn blob<W: BlockWorld>(ctx: &mut Ctx<'_, W>, sink: &mut LeafSink, c: &BlobCanopy, at: BlockPos) {
    for o in c.offset_min..=c.offset_max {
        let r = blob_radius(c.min_width, c.rise, c.run, c.offset_max - o);
        let index = (o - c.offset_min) as usize;
        let variation = c.variation.get(index).or(c.variation.last()).copied();
        layer(
            ctx,
            sink,
            at.offset(0, o, 0),
            1,
            r,
            c.leaf,
            |rng, ax, az| !(ax == r && az == r && variation.is_some_and(|v| v.roll(rng))),
        );
    }
}

/// Fancy oak cluster radius of layer `i` (end layers one smaller).
#[inline]
fn fancy_cluster_radius(radius: i32, height: i32, i: i32) -> f32 {
    if i == 0 || i == height - 1 {
        (radius - 1) as f32
    } else {
        radius as f32
    }
}

/// Fancy oak cluster: `height` round layers upwards from the node.
fn fancy_cluster<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    sink: &mut LeafSink,
    c: &FancyCanopy,
    at: BlockPos,
) {
    for i in 0..c.height {
        let r = fancy_cluster_radius(c.radius, c.height, i);
        let reach = (r + 0.618) as i32;
        for dx in -reach..=reach {
            for dz in -reach..=reach {
                let (fx, fz) = (dx.abs() as f32 + 0.5, dz.abs() as f32 + 0.5);
                if fx * fx + fz * fz <= r * r {
                    sink.put(ctx, at.offset(dx, i, dz), c.leaf);
                }
            }
        }
    }
}

/// Mega canopy radius `depth` layers below the top.
#[inline]
fn mega_radius(base: i32, core: i32, depth: i32) -> i32 {
    base + core - 1 + depth
}

/// Disc test of the mega canopies; unsimplified discs also drop cells with
/// `dx + dz >= 7` (rounding the corners of radius-5+ layers).
#[inline]
fn mega_keep(ax: i32, az: i32, r: i32, simplify: bool) -> bool {
    ax * ax + az * az <= r * r && (simplify || ax + az < 7)
}

/// Mega canopy: `height` disc layers downwards from the attachment, widening
/// by one per layer around the core.
fn mega_canopy<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    sink: &mut LeafSink,
    c: &MegaCanopy,
    at: BlockPos,
) {
    let layers = c.height.sample(ctx.rng);
    for d in 0..layers {
        let r = mega_radius(c.base_radius, c.core_width, d);
        let simplify = c.simplify;
        layer(
            ctx,
            sink,
            at.offset(0, -d, 0),
            c.core_width,
            r,
            c.leaf,
            |_, ax, az| mega_keep(ax, az, r, simplify),
        );
    }
}

/// Mega pine radius `depth` layers below the tip of a `height`-layer cone.
#[inline]
fn mega_pine_radius(base: i32, step: f32, depth: i32, height: i32) -> i32 {
    if height <= 0 {
        return base;
    }
    base + (depth as f32 / height as f32 * step).floor() as i32
}

/// Mega pine cone: from the attachment down `height` layers, radius growing
/// to `base_radius + radius_step`; a layer repeating the previous radius on
/// an even y grows by one (the ragged spruce silhouette).
fn mega_pine<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    sink: &mut LeafSink,
    c: &MegaPineCanopy,
    at: BlockPos,
) {
    let h = c.height.sample(ctx.rng);
    let mut prev = 0;
    for k in 0..=h {
        let y = at.y - k;
        let r = mega_pine_radius(c.base_radius, c.radius_step, k, h);
        let rr = if k > 0 && r == prev && (y & 1) == 0 {
            r + 1
        } else {
            r
        };
        layer(
            ctx,
            sink,
            BlockPos::new(at.x, y, at.z),
            c.core_width,
            rr,
            c.leaf,
            |_, ax, az| mega_keep(ax, az, rr, false),
        );
        prev = r;
    }
}

/// Pine ring radius after a layer at `y`: grows by one per layer up to `max`
/// and shrinks by one for the bottom layer.
#[inline]
fn pine_next(r: i32, y: i32, bottom: i32, max: i32) -> i32 {
    if r >= 1 && y == bottom + 1 {
        r - 1
    } else if r < max {
        r + 1
    } else {
        r
    }
}

/// Pine cone: a tip leaf one above the attachment, then `height` layers
/// down; widest radius `base_radius + next_int(height + 1)` (the classic pine
/// draw); square rings without corners.
fn pine<W: BlockWorld>(ctx: &mut Ctx<'_, W>, sink: &mut LeafSink, c: &PineCanopy, at: BlockPos) {
    let h = c.height.sample(ctx.rng).max(0);
    let max = c.base_radius + ctx.rng.next_int(h + 1);
    let top = at.y + 1;
    let bottom = top - h;
    let mut r = 0;
    for y in (bottom..=top).rev() {
        let rr = r;
        layer(
            ctx,
            sink,
            BlockPos::new(at.x, y, at.z),
            1,
            rr,
            c.leaf,
            |_, ax, az| rr == 0 || ax != rr || az != rr,
        );
        r = pine_next(r, y, bottom, max);
    }
}

/// Spruce ring sequence state: current radius, its cap and the radius it
/// restarts from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SpruceRing {
    r: i32,
    cap: i32,
    next: i32,
}

impl SpruceRing {
    /// Next layer down: grow until the cap, then restart smaller with a cap
    /// one wider (up to `max`).
    fn step(self, max: i32) -> Self {
        if self.r >= self.cap {
            SpruceRing {
                r: self.next,
                cap: (self.cap + 1).min(max),
                next: 1,
            }
        } else {
            SpruceRing {
                r: self.r + 1,
                ..self
            }
        }
    }
}

/// Spruce: rings from `upper_offset` above the attachment down to
/// `lower_offset` above the trunk base, alternating widths that widen down
/// to `max_radius`.
fn spruce<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    sink: &mut LeafSink,
    c: &SpruceCanopy,
    at: BlockPos,
    base_y: i32,
) {
    let top = at.y + c.upper_offset.sample(ctx.rng);
    let bottom = base_y + c.lower_offset.sample(ctx.rng);
    let max = c.max_radius.sample(ctx.rng);
    let mut ring = SpruceRing {
        r: ctx.rng.next_int(2),
        cap: 1,
        next: 0,
    };
    for y in (bottom..=top).rev() {
        let rr = ring.r;
        layer(
            ctx,
            sink,
            BlockPos::new(at.x, y, at.z),
            1,
            rr,
            c.leaf,
            |_, ax, az| rr == 0 || ax != rr || az != rr,
        );
        ring = ring.step(max);
    }
}

/// Roof layer `index` (0 = one below the top log) cell test: the lowest
/// layer is a full square, the roof layer (top log level) cuts three cells
/// per corner, the layers above are diamonds.
#[inline]
fn roofed_keep(index: i32, ax: i32, az: i32, outer: i32, inner: i32) -> bool {
    match index {
        0 => true,
        1 => ax + az <= 2 * inner - 2,
        _ => ax + az <= outer,
    }
}

/// Dark oak roof: `height` layers from one below the top log; the last
/// layer (of three or more) is the core only, placed half the time.
fn roofed<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    sink: &mut LeafSink,
    c: &RoofedCanopy,
    at: BlockPos,
) {
    let (outer, inner) = (c.outer_radius, c.inner_radius);
    for index in 0..c.height {
        let centre = at.offset(0, index - 2, 0);
        if index == c.height - 1 && c.height >= 3 {
            if ctx.rng.next_int(2) == 0 {
                layer(ctx, sink, centre, c.core_width, 0, c.leaf, |_, _, _| true);
            }
            continue;
        }
        let r = if index == 1 { inner } else { outer };
        layer(ctx, sink, centre, c.core_width, r, c.leaf, |_, ax, az| {
            roofed_keep(index, ax, az, outer, inner)
        });
    }
}

/// Acacia crown: a `size`-radius square without corners at the top log level
/// and a 3x3 above it, plus four outriggers at distance `size - 1` unless
/// simplified.
fn acacia_canopy<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    sink: &mut LeafSink,
    c: &AcaciaCanopy,
    at: BlockPos,
) {
    let s = c.size;
    layer(ctx, sink, at.below(), 1, s, c.leaf, |_, ax, az| {
        ax != s || az != s
    });
    layer(ctx, sink, at, 1, 1, c.leaf, |_, _, _| true);
    if !c.simplify {
        for d in Direction::ALL {
            let (dx, dz) = d.delta();
            sink.put(ctx, at.offset(dx * (s - 1), 0, dz * (s - 1)), c.leaf);
        }
    }
}

/// Random spread: `attempts` leaves at triangular random offsets
/// (`next_int(n) - next_int(n)`) around the attachment.
fn random_spread<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    sink: &mut LeafSink,
    c: &RandomSpreadCanopy,
    at: BlockPos,
) {
    for _ in 0..c.attempts {
        let dx = ctx.rng.next_int(c.radius) - ctx.rng.next_int(c.radius);
        let dy = ctx.rng.next_int(c.height) - ctx.rng.next_int(c.height);
        let dz = ctx.rng.next_int(c.radius) - ctx.rng.next_int(c.radius);
        if let Some(leaf) = pick(c.leaves, ctx.rng) {
            sink.put(ctx, at.offset(dx, dy, dz), leaf);
        }
    }
}

/// Whether a cherry row cell is left out: edge holes in the wide bottom row,
/// corners always (radius > 2) or by chance, and random holes near corners.
fn cherry_skip(
    rng: &mut BedrockRandom,
    c: &CherryCanopy,
    local_y: i32,
    ax: i32,
    az: i32,
    r: i32,
) -> bool {
    if local_y == -1 && (ax == r || az == r) && rng.probability(c.wide_bottom_hole) {
        return true;
    }
    let corner = ax == r && az == r;
    if r > 2 {
        corner || (ax + az > r * 2 - 2 && rng.probability(c.corner_hole))
    } else {
        corner && rng.probability(c.corner_hole)
    }
}

/// Cherry crown (rows relative to the attachment, `i = radius - 1`):
/// `height - 3` radius `i - 2`, `height - 4` radius `i - 1`, `height - 5..=0`
/// radius `i`, then `-1` radius `i` and `-2` radius `i - 1`, the last two
/// with leaves hanging from their edges.
fn cherry_canopy<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    sink: &mut LeafSink,
    c: &CherryCanopy,
    at: BlockPos,
) {
    let i = c.radius - 1;
    cherry_row(ctx, sink, c, at, c.height - 3, i - 2, false);
    cherry_row(ctx, sink, c, at, c.height - 4, i - 1, false);
    for y in (0..=c.height - 5).rev() {
        cherry_row(ctx, sink, c, at, y, i, false);
    }
    cherry_row(ctx, sink, c, at, -1, i, true);
    cherry_row(ctx, sink, c, at, -2, i - 1, true);
}

/// One cherry row; hanging rows grow a leaf under edge leaves with
/// `hanging` and one more with `hanging_extension`, within six blocks
/// (Manhattan) of the branch end.
#[allow(clippy::too_many_arguments)]
fn cherry_row<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    sink: &mut LeafSink,
    c: &CherryCanopy,
    at: BlockPos,
    local_y: i32,
    r: i32,
    hanging: bool,
) {
    if r < 0 {
        return;
    }
    let centre = at.offset(0, local_y, 0);
    layer(ctx, sink, centre, 1, r, c.leaf, |rng, ax, az| {
        !cherry_skip(rng, c, local_y, ax, az, r)
    });
    if !hanging {
        return;
    }
    let branch_end = at.below();
    for dx in -r..=r {
        for dz in -r..=r {
            if dx.abs() != r && dz.abs() != r {
                continue;
            }
            let cell = centre.offset(dx, 0, dz);
            if ctx.get(cell) != c.leaf {
                continue;
            }
            let below = cell.below();
            if manhattan(below, branch_end) >= 7
                || !ctx.rng.probability(c.hanging)
                || !sink.put(ctx, below, c.leaf)
            {
                continue;
            }
            let further = below.below();
            if manhattan(further, branch_end) < 7 && ctx.rng.probability(c.hanging_extension) {
                sink.put(ctx, further, c.leaf);
            }
        }
    }
}

/// Poplar cross-section membership: `dx² + dz² <= size`. The pack's
/// `radius` values (5..8) are read as this squared-distance bound, which
/// gives the narrow 5-wide column of a poplar crown (5: rounded, 8: full
/// square); the exact BDS shape is not observable from data.
#[inline]
fn poplar_layer(ax: i32, az: i32, size: i32) -> bool {
    ax * ax + az * az <= size
}

/// Whether a cross-section cell is on the outside (a 4-neighbour outward
/// leaves the section).
#[inline]
fn poplar_side(ax: i32, az: i32, size: i32) -> bool {
    !poplar_layer(ax + 1, az, size) || !poplar_layer(ax, az + 1, size)
}

/// Poplar column: a tip leaf above the attachment, a narrow cap at the
/// attachment, `height` full layers below it (outer leaves dropped with
/// `side_hole_chance`) and a narrow bottom layer.
fn poplar_canopy<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    sink: &mut LeafSink,
    c: &PoplarCanopy,
    at: BlockPos,
) {
    let size = pick(c.radius, ctx.rng).unwrap_or(0);
    let h = c.height.sample(ctx.rng).max(1);
    let reach = (size.max(0) as f32).sqrt() as i32;
    let narrow = size / 2;
    let side_hole = c.side_hole;
    sink.put(ctx, at.above(), c.leaf);
    layer(ctx, sink, at, 1, reach, c.leaf, |_, ax, az| {
        poplar_layer(ax, az, narrow)
    });
    for d in 1..=h {
        layer(
            ctx,
            sink,
            at.offset(0, -d, 0),
            1,
            reach,
            c.leaf,
            |rng, ax, az| {
                poplar_layer(ax, az, size)
                    && !(poplar_side(ax, az, size) && rng.probability(side_hole))
            },
        );
    }
    layer(
        ctx,
        sink,
        at.offset(0, -h - 1, 0),
        1,
        reach,
        c.leaf,
        |_, ax, az| poplar_layer(ax, az, narrow),
    );
}

/// Vines and hanging blocks on the leaves of `canopy` inside `bounds` (the
/// box of the leaves this tree placed): per leaf and free side, a vine column
/// with the decoration chance; under a leaf with air below, the hanging
/// block with its chance.
fn decorate_canopy<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    canopy: &Canopy,
    bounds: (BlockPos, BlockPos),
) {
    let (vines, hanging) = canopy.decorations();
    let (lo, hi) = bounds;
    for y in (lo.y..=hi.y).rev() {
        for x in lo.x..=hi.x {
            for z in lo.z..=hi.z {
                let at = BlockPos::new(x, y, z);
                if !canopy.is_leaf(ctx.get(at)) {
                    continue;
                }
                if let Some(d) = vines {
                    for dir in Direction::ALL {
                        if d.chance.roll(ctx.rng) {
                            hang(
                                ctx,
                                at.side(dir),
                                d.block[dir.opposite() as usize],
                                d.step,
                                d.num_steps,
                            );
                        }
                    }
                }
                if let Some((block, chance)) = hanging
                    && chance.roll(ctx.rng)
                    && bb::air(ctx.get(at.below()))
                {
                    ctx.set(at.below(), block);
                }
            }
        }
    }
}

/// Column of up to `steps` blocks from `start`, stopping at the first
/// non-air cell.
fn hang<W: BlockWorld>(ctx: &mut Ctx<'_, W>, start: BlockPos, block: u32, step: i32, steps: i32) {
    let mut at = start;
    for _ in 0..steps {
        if !bb::air(ctx.get(at)) {
            break;
        }
        ctx.set(at, block);
        at = at.offset(0, step, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::world::{NoiseSource, OUT_OF_BOUNDS};
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn heights_stay_in_declared_ranges_and_reach_both_ends() {
        let mut rng = BedrockRandom::new(11);
        let cases = [
            (Height::Range(IntRange { min: 4, max: 6 }), 4, 6),
            (
                Height::Intervals {
                    base: 6,
                    intervals: &[3, 2],
                },
                6,
                9,
            ),
            (
                Height::Intervals {
                    base: 10,
                    intervals: &[3, 20],
                },
                10,
                31,
            ),
            (
                Height::Rand {
                    base: 2,
                    a: 1,
                    b: 4,
                },
                2,
                7,
            ),
        ];
        for (h, lo, hi) in cases {
            let (mut seen_lo, mut seen_hi) = (false, false);
            for _ in 0..20_000 {
                let v = h.sample(&mut rng);
                assert!((lo..=hi).contains(&v), "{h:?} drew {v}");
                seen_lo |= v == lo;
                seen_hi |= v == hi;
            }
            assert!(seen_lo && seen_hi, "{h:?} never reached an end");
        }
    }

    #[test]
    fn blob_radius_reproduces_oak_swamp_and_bush_profiles() {
        let profile = |min, rise, run, layers| {
            (0..layers)
                .map(|d| blob_radius(min, rise, run, d))
                .collect::<Vec<_>>()
        };
        // Depth 0 is the top layer.
        assert_eq!(profile(1, 1, 2, 4), [1, 1, 2, 2]);
        assert_eq!(profile(2, 1, 2, 4), [2, 2, 3, 3]);
        assert_eq!(profile(0, 1, 1, 3), [0, 1, 2]);
    }

    #[test]
    fn core_distance_measures_from_the_trunk_footprint() {
        let one: Vec<i32> = (-2..=2).map(|d| core_dist(d, 1)).collect();
        assert_eq!(one, [2, 1, 0, 1, 2]);
        let two: Vec<i32> = (-2..=3).map(|d| core_dist(d, 2)).collect();
        assert_eq!(two, [2, 1, 0, 0, 1, 2]);
    }

    /// Cells a layer keeps, counted without a world.
    fn count(core: i32, r: i32, keep: impl Fn(i32, i32) -> bool) -> usize {
        let mut n = 0;
        for dx in -r..r + core {
            for dz in -r..r + core {
                n += usize::from(keep(core_dist(dx, core), core_dist(dz, core)));
            }
        }
        n
    }

    #[test]
    fn roof_layers_match_the_dark_oak_shape() {
        // 2x2 core, outer radius 2, inner radius 3.
        assert_eq!(count(2, 2, |ax, az| roofed_keep(0, ax, az, 2, 3)), 36);
        assert_eq!(
            count(2, 3, |ax, az| roofed_keep(1, ax, az, 2, 3)),
            64 - 4 * 3
        );
        assert_eq!(count(2, 2, |ax, az| roofed_keep(2, ax, az, 2, 3)), 24);
    }

    #[test]
    fn mega_discs_widen_downwards_and_trim_large_corners() {
        assert_eq!(
            (0..3).map(|d| mega_radius(2, 2, d)).collect::<Vec<_>>(),
            [3, 4, 5]
        );
        assert_eq!(
            (0..3).map(|d| mega_radius(1, 1, d)).collect::<Vec<_>>(),
            [1, 2, 3]
        );
        assert!(mega_keep(4, 3, 5, true));
        assert!(!mega_keep(4, 3, 5, false));
        assert!(!mega_keep(4, 4, 5, true));
    }

    #[test]
    fn mega_pine_radius_grows_from_tip_to_base() {
        let radii: Vec<i32> = (0..=13).map(|k| mega_pine_radius(0, 3.5, k, 13)).collect();
        assert_eq!(radii[0], 0);
        assert_eq!(radii[13], 3);
        assert!(radii.windows(2).all(|w| w[0] <= w[1]));
        assert_eq!(mega_pine_radius(2, 3.5, 4, 0), 2);
    }

    #[test]
    fn pine_rings_grow_then_shrink_for_the_bottom_layer() {
        // Tip at y = 4, bottom at y = 0, widest radius 2.
        let mut r = 0;
        let mut radii = Vec::new();
        for y in (0..=4).rev() {
            radii.push(r);
            r = pine_next(r, y, 0, 2);
        }
        assert_eq!(radii, [0, 1, 2, 2, 1]);
    }

    #[test]
    fn spruce_rings_alternate_and_respect_the_max_radius() {
        let mut ring = SpruceRing {
            r: 0,
            cap: 1,
            next: 0,
        };
        let mut radii = Vec::new();
        for _ in 0..10 {
            radii.push(ring.r);
            ring = ring.step(2);
        }
        assert_eq!(radii, [0, 1, 0, 1, 2, 1, 2, 1, 2, 1]);
    }

    #[test]
    fn fancy_profile_is_a_sphere_over_the_foliage_band() {
        assert_eq!(fancy_layer_size(10, 2, 0.3), -1.0);
        assert_eq!(fancy_layer_size(10, 5, 0.3), 2.5);
        assert_eq!(fancy_layer_size(10, 10, 0.3), 0.0);
        assert!(fancy_layer_size(10, 4, 0.3) < 2.5 && fancy_layer_size(10, 4, 0.3) > 0.0);
        assert_eq!(fancy_cluster_radius(3, 4, 0), 2.0);
        assert_eq!(fancy_cluster_radius(3, 4, 1), 3.0);
        assert_eq!(fancy_cluster_radius(3, 4, 3), 2.0);
    }

    #[test]
    fn cherry_rows_always_drop_large_corners_and_keep_inner_cells() {
        let c = CherryCanopy {
            height: 5,
            radius: 4,
            wide_bottom_hole: 0.0,
            corner_hole: 0.0,
            hanging: 0.0,
            hanging_extension: 0.0,
            leaf: 0,
        };
        let mut rng = BedrockRandom::new(3);
        assert!(cherry_skip(&mut rng, &c, 0, 3, 3, 3));
        assert!(!cherry_skip(&mut rng, &c, 0, 3, 2, 3));
        assert!(!cherry_skip(&mut rng, &c, -1, 3, 0, 3));
        // Small rows keep their corners unless the hole chance hits.
        assert!(!cherry_skip(&mut rng, &c, 2, 1, 1, 1));
    }

    #[test]
    fn poplar_sections_form_a_narrow_column() {
        assert_eq!(count(1, 2, |ax, az| poplar_layer(ax, az, 5)), 21);
        assert_eq!(count(1, 2, |ax, az| poplar_layer(ax, az, 8)), 25);
        assert!(
            poplar_side(2, 0, 5)
                && poplar_side(1, 2, 5)
                && !poplar_side(1, 1, 5)
                && !poplar_side(0, 0, 5)
        );
    }

    #[test]
    fn weighted_pick_uses_cumulative_weights() {
        let list = [('a', 1), ('b', 0), ('c', 3)];
        let picks: Vec<char> = (0..4).filter_map(|roll| pick_at(&list, roll)).collect();
        assert_eq!(picks, ['a', 'c', 'c', 'c']);
    }

    /// Sparse test world: air unless set, y in [-64, 320).
    #[derive(Default)]
    struct TestWorld {
        blocks: HashMap<BlockPos, u32>,
    }

    impl TestWorld {
        fn floor(r: i32, id: u32) -> Self {
            let mut w = Self::default();
            for x in -r..=r {
                for z in -r..=r {
                    w.blocks.insert(BlockPos::new(x, 0, z), id);
                }
            }
            w
        }

        fn count(&self, id: u32) -> usize {
            self.blocks.values().filter(|&&b| b == id).count()
        }
    }

    impl BlockWorld for TestWorld {
        fn min_y(&self) -> i32 {
            -64
        }
        fn max_y(&self) -> i32 {
            320
        }
        fn get(&self, pos: BlockPos) -> u32 {
            if (-64..320).contains(&pos.y) {
                self.blocks.get(&pos).copied().unwrap_or(bb::AIR)
            } else {
                OUT_OF_BOUNDS
            }
        }
        fn set(&mut self, pos: BlockPos, id: u32) -> bool {
            if !(-64..320).contains(&pos.y) {
                return false;
            }
            self.blocks.insert(pos, id);
            true
        }
        fn biome(&self, _: BlockPos) -> u16 {
            0
        }
        fn heightmap(&mut self, _: i32, _: i32) -> i32 {
            1
        }
        fn above_top_solid(&mut self, _: i32, _: i32) -> i32 {
            1
        }
    }

    struct Flat;
    impl NoiseSource for Flat {
        fn noise(&self, _: f32, _: f32) -> f32 {
            0.0
        }
    }

    fn on_dirt(b: u32) -> bool {
        b == bb::DIRT || b == bb::GRASS_BLOCK
    }
    fn leaves_only(b: u32) -> bool {
        bb::leaves(b)
    }
    fn nothing(_: u32) -> bool {
        false
    }

    const OAK: Log = Log {
        x: bb::OAK_LOG,
        y: bb::OAK_LOG,
        z: bb::OAK_LOG,
    };
    /// Distinct stand-ins for the lying states, to check log orientation.
    const MARKED: Log = Log {
        x: bb::SAND,
        y: bb::OAK_LOG,
        z: bb::GRAVEL,
    };

    static OAK_TREE: Params = Params {
        may_grow_on: on_dirt,
        may_replace: leaves_only,
        may_grow_through: nothing,
        base_block: &[bb::DIRT, bb::COARSE_DIRT],
        base_cluster: None,
        roots: None,
        trunk: Trunk::Straight(StraightTrunk {
            height: Height::Range(IntRange { min: 4, max: 6 }),
            height_modifier: IntRange { min: 0, max: 0 },
            log: OAK,
            decoration: None,
            max_water_depth: None,
        }),
        canopy: Some(Canopy::Blob(BlobCanopy {
            offset_min: -3,
            offset_max: 0,
            min_width: 1,
            rise: 1,
            run: 2,
            variation: &[
                Chance::Fraction(1, 2),
                Chance::Fraction(1, 2),
                Chance::Fraction(1, 2),
                Chance::Fraction(1, 1),
            ],
            leaf: bb::OAK_LEAVES,
            decoration: None,
        })),
        log_decoration: false,
    };

    static MEGA_TREE: Params = Params {
        may_grow_on: on_dirt,
        may_replace: leaves_only,
        may_grow_through: nothing,
        base_block: &[bb::PODZOL],
        base_cluster: None,
        roots: None,
        trunk: Trunk::Mega(MegaTrunk {
            height: Height::Intervals {
                base: 13,
                intervals: &[3, 15],
            },
            width: 2,
            log: OAK,
            decoration: None,
            branches: None,
        }),
        canopy: Some(Canopy::MegaPine(MegaPineCanopy {
            height: IntRange { min: 3, max: 7 },
            base_radius: 0,
            radius_step: 3.5,
            core_width: 2,
            leaf: bb::OAK_LEAVES,
        })),
        log_decoration: false,
    };

    static FALLEN_TREE: Params = Params {
        may_grow_on: on_dirt,
        may_replace: leaves_only,
        may_grow_through: nothing,
        base_block: &[bb::DIRT],
        base_cluster: None,
        roots: None,
        trunk: Trunk::Fallen(FallenTrunk {
            length: IntRange { min: 4, max: 6 },
            height_modifier: IntRange { min: 0, max: 0 },
            log: MARKED,
            decoration: None,
        }),
        canopy: None,
        log_decoration: true,
    };

    fn grow<F>(
        world: &mut TestWorld,
        seed: u32,
        p: &'static Params,
        decorate: F,
    ) -> Option<BlockPos>
    where
        F: FnMut(&mut Ctx<'_, TestWorld>, BlockPos) -> Option<BlockPos>,
    {
        let mut rng = BedrockRandom::new(seed);
        let mut ctx = Ctx {
            world,
            rng: &mut rng,
            noise: &Flat,
            seed: 0,
        };
        place(&mut ctx, BlockPos::new(0, 1, 0), p, decorate)
    }

    #[test]
    fn oak_grows_a_trunk_in_range_with_a_crown_on_top() {
        for seed in 0..32 {
            let mut world = TestWorld::floor(8, bb::GRASS_BLOCK);
            let base = grow(&mut world, seed, &OAK_TREE, |_, _| None);
            assert_eq!(base, Some(BlockPos::new(0, 1, 0)));
            let h = (1..20)
                .take_while(|&y| world.get(BlockPos::new(0, y, 0)) == bb::OAK_LOG)
                .count() as i32;
            assert!((4..=6).contains(&h), "trunk height {h}");
            // Grass under the trunk became the first base block.
            assert_eq!(world.get(BlockPos::new(0, 0, 0)), bb::DIRT);
            // A leaf caps the trunk and the widest layers reach radius 2.
            assert_eq!(world.get(BlockPos::new(0, 1 + h, 0)), bb::OAK_LEAVES);
            assert_eq!(world.get(BlockPos::new(2, h - 2, 0)), bb::OAK_LEAVES);
            assert!(world.blocks.keys().all(|p| p.y < 2 + h && p.x.abs() <= 8));
        }
    }

    #[test]
    fn obstructed_trunk_places_nothing() {
        let mut world = TestWorld::floor(8, bb::DIRT);
        world.set(BlockPos::new(0, 3, 0), bb::STONE);
        let before = world.blocks.len();
        assert_eq!(grow(&mut world, 5, &OAK_TREE, |_, _| None), None);
        assert_eq!(world.blocks.len(), before);
        // Wrong ground fails as well.
        let mut stone = TestWorld::floor(8, bb::STONE);
        assert_eq!(grow(&mut stone, 5, &OAK_TREE, |_, _| None), None);
        assert_eq!(stone.count(bb::OAK_LOG), 0);
    }

    #[test]
    fn mega_trunk_is_two_by_two_and_sits_on_its_base_block() {
        let mut world = TestWorld::floor(10, bb::DIRT);
        assert!(grow(&mut world, 9, &MEGA_TREE, |_, _| None).is_some());
        let h = (1..64)
            .take_while(|&y| world.get(BlockPos::new(0, y, 0)) == bb::OAK_LOG)
            .count() as i32;
        assert!((13..=29).contains(&h), "trunk height {h}");
        for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            assert_eq!(world.get(BlockPos::new(dx, 0, dz)), bb::PODZOL);
            for y in 1..=h {
                assert_eq!(world.get(BlockPos::new(dx, y, dz)), bb::OAK_LOG);
            }
        }
        assert_eq!(world.get(BlockPos::new(0, h + 1, 0)), bb::OAK_LEAVES);
    }

    #[test]
    fn fallen_log_lies_along_its_direction_and_is_decorated_per_block() {
        for seed in 0..16 {
            let mut world = TestWorld::floor(12, bb::DIRT);
            let mut decorated = Vec::new();
            let base = grow(&mut world, seed, &FALLEN_TREE, |_, p| {
                decorated.push(p);
                None
            });
            assert_eq!(base, Some(BlockPos::new(0, 1, 0)));
            assert_eq!(world.get(BlockPos::new(0, 1, 0)), bb::OAK_LOG);
            let lying: Vec<BlockPos> = world
                .blocks
                .iter()
                .filter(|&(_, &b)| b == bb::SAND || b == bb::GRAVEL)
                .map(|(&p, _)| p)
                .collect();
            assert!((4..=6).contains(&lying.len()), "log length {}", lying.len());
            assert_eq!(decorated.len(), lying.len());
            for p in &lying {
                assert_eq!(p.y, 1);
                let along_x = world.get(*p) == bb::SAND;
                assert!(if along_x {
                    p.z == 0 && p.x.abs() >= 2
                } else {
                    p.x == 0 && p.z.abs() >= 2
                });
            }
        }
    }
}
