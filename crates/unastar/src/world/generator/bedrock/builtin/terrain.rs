//! Terrain-shaping built-ins: desert wells, ice patches and spikes, huge
//! mushrooms, the dripstone cave set (clusters, large dripstone, small
//! pointed dripstone) and the 1.26 sulfur cave set (spikes, spike clusters,
//! pools and the spring trail to the surface).
//!
//! The overworld algorithms follow vanilla behaviour (both editions share
//! them); the sulfur cave features have no published algorithm, so their
//! shapes are inferred from the shipped data (the `sulfur_spike` block shares
//! pointed dripstone's `dripstone_thickness`/`hanging` states, pools are
//! followed by the potent sulfur search, the trail is snapped to a cave
//! ceiling) and documented as such next to the code.
//!
//! Decoration is statistical parity with BDS: the shapes, sizes and
//! probabilities are vanilla's, the exact draw order is unknown.
//!
//! Bedrock keeps waterlogging in a separate liquid layer that [`BlockWorld`]
//! does not expose, so a spike grown into water replaces the water block.

use super::super::generated::{bb, props};
use super::super::rng::BedrockRandom;
use super::super::world::{BlockPos, BlockWorld, Ctx, Direction, OUT_OF_BOUNDS};
use std::f32::consts::PI;

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Vanilla `isEmptyOrWater`: the space spikes grow through.
#[inline]
fn is_empty_or_water(id: u32) -> bool {
    bb::air(id) || bb::water(id)
}

/// Vanilla `isEmptyOrWaterOrLava`.
#[inline]
fn is_empty_or_fluid(id: u32) -> bool {
    is_empty_or_water(id) || bb::lava(id)
}

/// Any block that ends a column scan (not air/water, inside the region).
#[inline]
fn is_column_edge(id: u32) -> bool {
    id != OUT_OF_BOUNDS && !is_empty_or_water(id)
}

/// A block that keeps water in: solid, not lava, inside the region.
#[inline]
fn holds_water(id: u32) -> bool {
    id != OUT_OF_BOUNDS && !bb::lava(id) && props::is_solid(id)
}

/// Uniform float in `[lo, hi)`.
#[inline]
fn uniform(rng: &mut BedrockRandom, lo: f32, hi: f32) -> f32 {
    lo + (hi - lo) * rng.next_float()
}

/// Standard normal sample (Box-Muller). Bedrock's random has no gaussian
/// helper exposed here; the distribution is what matters for parity.
fn gaussian(rng: &mut BedrockRandom) -> f32 {
    // `next_float` can round up to exactly 1.0 in f32, keep `u1` > 0.
    let u1 = (1.0 - rng.next_float()).max(f32::MIN_POSITIVE);
    let u2 = rng.next_float();
    (-2.0 * u1.ln()).sqrt() * (2.0 * PI * u2).cos()
}

/// Vanilla `clamped_normal` float provider.
#[inline]
fn clamped_normal(rng: &mut BedrockRandom, mean: f32, deviation: f32, lo: f32, hi: f32) -> f32 {
    (mean + deviation * gaussian(rng)).clamp(lo, hi)
}

/// Linear map of `v` from `[in_lo, in_hi]` to `[out_lo, out_hi]`, clamped.
#[inline]
fn clamped_map(v: f32, in_lo: f32, in_hi: f32, out_lo: f32, out_hi: f32) -> f32 {
    let t = ((v - in_lo) / (in_hi - in_lo)).clamp(0.0, 1.0);
    out_lo + t * (out_hi - out_lo)
}

/// Walks from `pos` along `dy` (+1 up, -1 down) through blocks matching
/// `inside`, at most `range` steps; returns the y of the first other block
/// when `edge` accepts it (vanilla `Column.scan` for one direction).
fn scan_edge<W: BlockWorld>(
    ctx: &Ctx<'_, W>,
    pos: BlockPos,
    dy: i32,
    range: i32,
    inside: fn(u32) -> bool,
    edge: fn(u32) -> bool,
) -> Option<i32> {
    for k in 1..=range {
        let y = pos.y + dy * k;
        let id = ctx.get(BlockPos::new(pos.x, y, pos.z));
        if inside(id) {
            continue;
        }
        return (id != OUT_OF_BOUNDS && edge(id)).then_some(y);
    }
    None
}

/// Lowers `pos` through air (and loose snow layers) to the first other
/// block, stopping two blocks above the world floor (vanilla's legacy
/// "find the ground" loop).
fn descend_to_ground<W: BlockWorld>(ctx: &Ctx<'_, W>, pos: BlockPos) -> BlockPos {
    let floor = ctx.world.min_y() + 2;
    let mut p = pos;
    while p.y > floor {
        let id = ctx.get(p);
        if !(bb::air(id) || bb::snow_layer_any(id)) {
            break;
        }
        p = p.below();
    }
    p
}

const NEIGHBOURS_6: [(i32, i32, i32); 6] = [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)];

// ---------------------------------------------------------------------------
// Spikes (pointed dripstone and sulfur spikes)
// ---------------------------------------------------------------------------

/// `dripstone_thickness` values; the discriminant indexes [`SpikeStates`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Thickness {
    Tip = 0,
    Frustum = 1,
    Middle = 2,
    Base = 3,
    /// A tip touching the opposite spike's tip (vanilla `tip_merge`).
    Merge = 4,
}

/// Thickness of segment `i` (0 = the segment touching the attached block) of
/// a spike `len` long: base, middle..., frustum, tip. Vanilla builds the
/// column this way, so a 1-long spike is a lone tip, 2 is frustum + tip and
/// only spikes of 3+ have a base.
fn thickness_at(i: i32, len: i32, merge: bool) -> Thickness {
    match len - 1 - i {
        0 if merge => Thickness::Merge,
        0 => Thickness::Tip,
        1 => Thickness::Frustum,
        _ if i == 0 => Thickness::Base,
        _ => Thickness::Middle,
    }
}

/// Runtime ids of one spike block, indexed by [`Thickness`].
struct SpikeStates {
    standing: [u32; 5],
    hanging: [u32; 5],
}

const POINTED_DRIPSTONE: SpikeStates = SpikeStates {
    standing: [
        bb::POINTED_DRIPSTONE_UP_TIP,
        bb::POINTED_DRIPSTONE_UP_FRUSTUM,
        bb::POINTED_DRIPSTONE_UP_MIDDLE,
        bb::POINTED_DRIPSTONE_UP_BASE,
        bb::POINTED_DRIPSTONE_UP_MERGE,
    ],
    hanging: [
        bb::POINTED_DRIPSTONE_DOWN_TIP,
        bb::POINTED_DRIPSTONE_DOWN_FRUSTUM,
        bb::POINTED_DRIPSTONE_DOWN_MIDDLE,
        bb::POINTED_DRIPSTONE_DOWN_BASE,
        bb::POINTED_DRIPSTONE_DOWN_MERGE,
    ],
};

const SULFUR_SPIKE: SpikeStates = SpikeStates {
    standing: [
        bb::SULFUR_SPIKE_UP_TIP,
        bb::SULFUR_SPIKE_UP_FRUSTUM,
        bb::SULFUR_SPIKE_UP_MIDDLE,
        bb::SULFUR_SPIKE_UP_BASE,
        bb::SULFUR_SPIKE_UP_MERGE,
    ],
    hanging: [
        bb::SULFUR_SPIKE_DOWN_TIP,
        bb::SULFUR_SPIKE_DOWN_FRUSTUM,
        bb::SULFUR_SPIKE_DOWN_MIDDLE,
        bb::SULFUR_SPIKE_DOWN_BASE,
        bb::SULFUR_SPIKE_DOWN_MERGE,
    ],
};

/// Block palette of a spike family: the spike block, the solid block it
/// grows from, what that block may replace and what a spike may hang from or
/// stand on.
struct SpikeKind {
    states: &'static SpikeStates,
    block: u32,
    replaceable: fn(u32) -> bool,
    attach: fn(u32) -> bool,
}

/// Vanilla `isDripstoneBase`: dripstone block or overworld base stone.
fn is_dripstone_base(id: u32) -> bool {
    id == bb::DRIPSTONE_BLOCK || bb::base_stone_overworld(id)
}

/// Sulfur spikes grow from the sulfur cave's own blocks or from base stone
/// (inferred: the cave walls are sulfur/cinnabar bands over stone).
fn is_sulfur_spike_base(id: u32) -> bool {
    bb::sulfur_cave_stone(id) || bb::base_stone_overworld(id)
}

const DRIPSTONE_KIND: SpikeKind = SpikeKind {
    states: &POINTED_DRIPSTONE,
    block: bb::DRIPSTONE_BLOCK,
    replaceable: bb::base_stone_overworld,
    attach: is_dripstone_base,
};

/// Sulfur spikes turn the stone they grow from into sulfur (inferred, the
/// counterpart of dripstone blocks under pointed dripstone); existing
/// cinnabar bands are left alone.
const SULFUR_KIND: SpikeKind = SpikeKind {
    states: &SULFUR_SPIKE,
    block: bb::SULFUR,
    replaceable: bb::base_stone_overworld,
    attach: is_sulfur_spike_base,
};

/// Grows a spike from `start` (the open block touching the attached block)
/// along `dy` (+1 standing, -1 hanging), `len` segments. The length is first
/// shortened to the open space so every spike ends in a tip (vanilla grows
/// segment by segment and can leave a tipless stub against an obstacle; the
/// shortened form is what such spikes look like after a block update).
/// A merged tip is only kept when the full length fits. Returns the length
/// placed.
fn grow_spike<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    start: BlockPos,
    dy: i32,
    len: i32,
    merge: bool,
    states: &SpikeStates,
) -> i32 {
    let mut room = 0;
    while room < len && is_empty_or_water(ctx.get(start.offset(0, dy * room, 0))) {
        room += 1;
    }
    let merge = merge && room == len;
    let row = if dy < 0 { &states.hanging } else { &states.standing };
    for i in 0..room {
        ctx.set(start.offset(0, dy * i, 0), row[thickness_at(i, room, merge) as usize]);
    }
    room
}

/// Turns `at` into the spike family's block when it may be replaced.
#[inline]
fn place_base_block<W: BlockWorld>(ctx: &mut Ctx<'_, W>, at: BlockPos, kind: &SpikeKind) -> bool {
    (kind.replaceable)(ctx.get(at)) && ctx.set(at, kind.block)
}

/// Small spike: how far a start position looks up and down for a ceiling or
/// floor (the `vertical_search_range` of `small_dripstone_snap_to_surface_feature`).
const SMALL_SPIKE_SEARCH: i32 = 12;
/// Chance of each extra segment of a small spike.
const SMALL_SPIKE_TALLER: f32 = 0.2;
/// Longest small spike.
const SMALL_SPIKE_MAX: i32 = 4;
/// Vanilla patch spread under a small spike: chance per horizontal side, then
/// of a second and third random step.
const PATCH_SPREAD: f32 = 0.7;
const PATCH_RADIUS_2: f32 = 0.5;
const PATCH_RADIUS_3: f32 = 0.5;

/// One small spike near `pos` (vanilla `pointed_dripstone` feature): picks a
/// ceiling or floor reachable through open blocks (random when both are), turns
/// the attached block and a random patch around it into the family's block,
/// then grows a 1 to 4 long spike. Vanilla's small dripstone is 1 or 2 long
/// (20 % taller); here every further segment has the same 20 % chance, capped
/// at 4, which keeps vanilla's 1/2 ratio and lets the rare longer ones BDS
/// shows appear.
fn place_small_spike<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos, kind: &SpikeKind) -> Option<BlockPos> {
    if !is_empty_or_water(ctx.get(pos)) {
        return None;
    }
    // The feature receives a point in open space (the snap moves it beside a
    // wall); it attaches to the nearest ceiling/floor within the snap range.
    let ceiling = scan_edge(ctx, pos, 1, SMALL_SPIKE_SEARCH, is_empty_or_water, kind.attach);
    let floor = scan_edge(ctx, pos, -1, SMALL_SPIKE_SEARCH, is_empty_or_water, kind.attach);
    let (root_y, dy) = match (ceiling, floor) {
        (Some(c), Some(f)) => {
            if ctx.rng.next_int(2) == 0 {
                (c, -1)
            } else {
                (f, 1)
            }
        }
        (Some(c), None) => (c, -1),
        (None, Some(f)) => (f, 1),
        (None, None) => return None,
    };
    let root = BlockPos::new(pos.x, root_y, pos.z);
    place_base_block(ctx, root, kind);
    for dir in Direction::ALL {
        if ctx.rng.next_float() > PATCH_SPREAD {
            continue;
        }
        let first = root.side(dir);
        place_base_block(ctx, first, kind);
        if ctx.rng.next_float() > PATCH_RADIUS_2 {
            continue;
        }
        let (x, y, z) = NEIGHBOURS_6[ctx.rng.next_int(6) as usize];
        let second = first.offset(x, y, z);
        place_base_block(ctx, second, kind);
        if ctx.rng.next_float() > PATCH_RADIUS_3 {
            continue;
        }
        let (x, y, z) = NEIGHBOURS_6[ctx.rng.next_int(6) as usize];
        place_base_block(ctx, second.offset(x, y, z), kind);
    }
    let mut len = 1;
    while len < SMALL_SPIKE_MAX && ctx.rng.probability(SMALL_SPIKE_TALLER) {
        len += 1;
    }
    let start = root.offset(0, dy, 0);
    (grow_spike(ctx, start, dy, len, false, kind.states) > 0).then_some(start)
}

// ---------------------------------------------------------------------------
// Spike clusters (dripstone_cluster and its sulfur counterpart)
// ---------------------------------------------------------------------------

/// Vanilla dripstone cluster parameters.
const CLUSTER_SEARCH: i32 = 12;
const CLUSTER_HEIGHT: (i32, i32) = (2, 6);
const CLUSTER_RADIUS: (i32, i32) = (2, 8);
const CLUSTER_MAX_HEIGHT_DIFF: i32 = 1;
const CLUSTER_HEIGHT_DEVIATION: f32 = 3.0;
const CLUSTER_LAYER_THICKNESS: (i32, i32) = (2, 4);
const CLUSTER_DENSITY: (f32, f32) = (0.3, 0.7);
/// Wetness: clamped normal (mean, deviation, min, max).
const CLUSTER_WETNESS: (f32, f32, f32, f32) = (0.1, 0.3, 0.1, 0.9);
const CLUSTER_EDGE_CHANCE: f32 = 0.1;
const CLUSTER_EDGE_DISTANCE: f32 = 3.0;
const CLUSTER_BIAS_DISTANCE: f32 = 8.0;

/// Chance that a column of a `x_radius` by `z_radius` cluster grows spikes:
/// 0.1 on the rim rising to 1 three blocks inside.
fn column_chance(x_radius: i32, z_radius: i32, dx: i32, dz: i32) -> f32 {
    let to_edge = (x_radius - dx.abs()).min(z_radius - dz.abs());
    clamped_map(to_edge as f32, 0.0, CLUSTER_EDGE_DISTANCE, CLUSTER_EDGE_CHANCE, 1.0)
}

/// Spike length in a cluster column: 0 with probability `1 - density`, else a
/// clamped normal around a bias that is `max_height / 2` at the centre and 0
/// eight blocks (taxicab) out.
fn cluster_spike_height(rng: &mut BedrockRandom, dx: i32, dz: i32, density: f32, max_height: i32) -> i32 {
    if max_height <= 0 || rng.next_float() > density {
        return 0;
    }
    let distance = (dx.abs() + dz.abs()) as f32;
    let bias = clamped_map(distance, 0.0, CLUSTER_BIAS_DISTANCE, max_height as f32 / 2.0, 0.0);
    clamped_normal(rng, bias, CLUSTER_HEIGHT_DEVIATION, 0.0, max_height as f32) as i32
}

/// Vanilla `canPlacePool`: the floor block is not already water/dripstone and
/// is enclosed by water-holding blocks on all sides and below.
fn can_place_pool<W: BlockWorld>(ctx: &Ctx<'_, W>, at: BlockPos) -> bool {
    let id = ctx.get(at);
    if bb::water(id) || id == bb::DRIPSTONE_BLOCK || bb::pointed_dripstone(id) || bb::water(ctx.get(at.above())) {
        return false;
    }
    Direction::ALL.iter().all(|&d| holds_water(ctx.get(at.side(d)))) && holds_water(ctx.get(at.below()))
}

/// Replaces up to `thickness` blocks from `start` along `dy` with the family
/// block, stopping at the first block it may not replace.
fn replace_layer<W: BlockWorld>(ctx: &mut Ctx<'_, W>, start: BlockPos, dy: i32, thickness: i32, kind: &SpikeKind) -> bool {
    let mut placed = false;
    for i in 0..thickness {
        if !place_base_block(ctx, start.offset(0, dy * i, 0), kind) {
            break;
        }
        placed = true;
    }
    placed
}

/// Shared cluster parameters drawn once per cluster.
struct ClusterRoll {
    height: i32,
    wetness: f32,
    density: f32,
}

/// One column of a cluster at `col` (offset `dx, dz` from the centre).
fn cluster_column<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    kind: &SpikeKind,
    col: BlockPos,
    dx: i32,
    dz: i32,
    chance: f32,
    roll: &ClusterRoll,
) -> bool {
    if !is_empty_or_water(ctx.get(col)) {
        return false;
    }
    let ceiling = scan_edge(ctx, col, 1, CLUSTER_SEARCH, is_empty_or_water, is_column_edge);
    let mut floor = scan_edge(ctx, col, -1, CLUSTER_SEARCH, is_empty_or_water, is_column_edge);
    if ceiling.is_none() && floor.is_none() {
        return false;
    }
    let mut placed = false;
    // Wet columns sink their floor block into a one-block pool; the floor
    // moves down to the block under the water.
    if ctx.rng.probability(roll.wetness)
        && let Some(f) = floor
    {
        let at = BlockPos::new(col.x, f, col.z);
        if can_place_pool(ctx, at) {
            placed |= ctx.set(at, bb::WATER);
            floor = Some(f - 1);
        }
    }
    let grow_down = ctx.rng.next_float() < chance;
    let grow_up = ctx.rng.next_float() < chance;
    let gap = match (ceiling, floor) {
        (Some(c), Some(f)) => Some(c - f - 1),
        _ => None,
    };
    let max_height = gap.map_or(roll.height, |g| roll.height.min(g));

    let mut down = 0;
    if let Some(c) = ceiling {
        let at = BlockPos::new(col.x, c, col.z);
        if grow_down && !bb::lava(ctx.get(at)) {
            let thickness = ctx.rng.next_int_inclusive(CLUSTER_LAYER_THICKNESS.0, CLUSTER_LAYER_THICKNESS.1);
            placed |= replace_layer(ctx, at, 1, thickness, kind);
            down = cluster_spike_height(ctx.rng, dx, dz, roll.density, max_height);
            if !(kind.attach)(ctx.get(at)) {
                down = 0;
            }
        }
    }
    let mut up = 0;
    if let Some(f) = floor {
        let at = BlockPos::new(col.x, f, col.z);
        if grow_up && !bb::lava(ctx.get(at)) {
            let thickness = ctx.rng.next_int_inclusive(CLUSTER_LAYER_THICKNESS.0, CLUSTER_LAYER_THICKNESS.1);
            placed |= replace_layer(ctx, at, -1, thickness, kind);
            // A stalagmite under a stalactite mirrors its length within the
            // allowed difference; a lone one draws its own.
            up = if down > 0 {
                let diff = ctx.rng.next_int_inclusive(-CLUSTER_MAX_HEIGHT_DIFF, CLUSTER_MAX_HEIGHT_DIFF);
                (down + diff).clamp(0, max_height)
            } else {
                cluster_spike_height(ctx.rng, dx, dz, roll.density, max_height)
            };
            if !(kind.attach)(ctx.get(at)) {
                up = 0;
            }
        }
    }
    // Spikes that would fill the gap meet tip to tip as merged tips.
    let mut merge = false;
    if let Some(g) = gap
        && down > 0
        && up > 0
        && down + up >= g
    {
        if g >= 2 {
            while down + up > g {
                if down >= up {
                    down -= 1;
                } else {
                    up -= 1;
                }
            }
            merge = true;
        } else {
            down = down.min(g);
            up = 0;
        }
    }
    if let Some(c) = ceiling
        && down > 0
    {
        placed |= grow_spike(ctx, BlockPos::new(col.x, c - 1, col.z), -1, down, merge, kind.states) > 0;
    }
    if let Some(f) = floor
        && up > 0
    {
        placed |= grow_spike(ctx, BlockPos::new(col.x, f + 1, col.z), 1, up, merge, kind.states) > 0;
    }
    placed
}

/// Vanilla dripstone cluster algorithm for a spike family: a rectangle of
/// columns, each scanning for its floor and ceiling, coating them with the
/// family block and growing paired spikes (pools only when `pools`).
fn place_cluster<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos, kind: &SpikeKind, pools: bool) -> Option<BlockPos> {
    if !is_empty_or_water(ctx.get(pos)) {
        return None;
    }
    let height = ctx.rng.next_int_inclusive(CLUSTER_HEIGHT.0, CLUSTER_HEIGHT.1);
    let wetness = if pools {
        let (mean, deviation, lo, hi) = CLUSTER_WETNESS;
        clamped_normal(ctx.rng, mean, deviation, lo, hi)
    } else {
        0.0
    };
    let density = uniform(ctx.rng, CLUSTER_DENSITY.0, CLUSTER_DENSITY.1);
    let roll = ClusterRoll { height, wetness, density };
    let x_radius = ctx.rng.next_int_inclusive(CLUSTER_RADIUS.0, CLUSTER_RADIUS.1);
    let z_radius = ctx.rng.next_int_inclusive(CLUSTER_RADIUS.0, CLUSTER_RADIUS.1);
    let mut placed = false;
    for dx in -x_radius..=x_radius {
        for dz in -z_radius..=z_radius {
            let chance = column_chance(x_radius, z_radius, dx, dz);
            placed |= cluster_column(ctx, kind, pos.offset(dx, 0, dz), dx, dz, chance, &roll);
        }
    }
    placed.then_some(pos)
}

// ---------------------------------------------------------------------------
// Large dripstone
// ---------------------------------------------------------------------------

/// Vanilla large dripstone parameters.
const LARGE_SEARCH: i32 = 30;
const LARGE_RADIUS: (i32, i32) = (3, 19);
const LARGE_HEIGHT_SCALE: (f32, f32) = (0.4, 2.0);
const LARGE_MAX_RADIUS_TO_HEIGHT: f32 = 0.33;
const LARGE_STALACTITE_BLUNTNESS: (f32, f32) = (0.3, 0.9);
const LARGE_STALAGMITE_BLUNTNESS: (f32, f32) = (0.4, 1.0);
const LARGE_WIND_SPEED: (f32, f32) = (0.0, 0.3);
const LARGE_MIN_RADIUS_FOR_WIND: i32 = 4;

/// Height of a large dripstone cone at distance `r` from its axis, for a
/// cone of base radius `max_r`. Vanilla's profile: with `d = r / max_r *
/// 0.384`, height `= scale * (0.75 d^(4/3) - d^(2/3) - ln(d) / 3) / 0.384 *
/// max_r`, clamped at 0 (it reaches 0 at the rim); `r` is raised to
/// `bluntness` first so the tip is flattened.
fn cone_height(r: f32, max_r: f32, scale: f32, bluntness: f32) -> f32 {
    const CUTOFF: f64 = 0.384;
    let r = f64::from(r.max(bluntness));
    let d = r / f64::from(max_r) * CUTOFF;
    let profile = 0.75 * d.powf(4.0 / 3.0) - d.powf(2.0 / 3.0) - d.ln() / 3.0;
    let h = (f64::from(scale) * profile).max(0.0);
    (h / CUTOFF * f64::from(max_r)) as f32
}

/// Horizontal drift of a large dripstone, growing with the distance from the
/// feature's origin height.
#[derive(Clone, Copy)]
struct Wind {
    origin_y: i32,
    vx: f32,
    vz: f32,
}

impl Wind {
    const NONE: Wind = Wind { origin_y: 0, vx: 0.0, vz: 0.0 };

    fn new(origin_y: i32, rng: &mut BedrockRandom) -> Wind {
        let speed = uniform(rng, LARGE_WIND_SPEED.0, LARGE_WIND_SPEED.1);
        let angle = uniform(rng, 0.0, PI);
        Wind { origin_y, vx: angle.cos() * speed, vz: angle.sin() * speed }
    }

    fn offset(self, p: BlockPos) -> BlockPos {
        if self.vx == 0.0 && self.vz == 0.0 {
            return p;
        }
        let k = (self.origin_y - p.y) as f32;
        p.offset((self.vx * k).floor() as i32, 0, (self.vz * k).floor() as i32)
    }
}

/// One cone of a large dripstone.
struct LargeDripstone {
    root: BlockPos,
    pointing_up: bool,
    radius: i32,
    bluntness: f32,
    scale: f32,
}

impl LargeDripstone {
    fn height_at(&self, r: f32) -> i32 {
        cone_height(r, self.radius as f32, self.scale, self.bluntness) as i32
    }

    /// `true` when a ring of `radius` around `center` is all rock (vanilla
    /// samples the circle every `6 / radius` radians). Unreadable blocks count
    /// as open so cones near the region edge fail cleanly.
    fn circle_embedded<W: BlockWorld>(ctx: &Ctx<'_, W>, center: BlockPos, radius: i32) -> bool {
        let solid = |id: u32| id != OUT_OF_BOUNDS && !is_empty_or_fluid(id);
        if !solid(ctx.get(center)) {
            return false;
        }
        let step = 6.0 / radius as f32;
        let mut angle = 0.0f32;
        while angle < 2.0 * PI {
            let x = (angle.cos() * radius as f32) as i32;
            let z = (angle.sin() * radius as f32) as i32;
            if !solid(ctx.get(center.offset(x, 0, z))) {
                return false;
            }
            angle += step;
        }
        true
    }

    /// Moves the root into the rock (up to 10 blocks) until its base circle is
    /// embedded, halving the radius on failure; `false` when it never fits or
    /// lava is hit.
    fn anchor<W: BlockWorld>(&mut self, ctx: &Ctx<'_, W>, wind: Wind) -> bool {
        let step = if self.pointing_up { -1 } else { 1 };
        while self.radius > 1 {
            let mut at = self.root;
            let tries = self.height_at(0.0).min(10);
            for _ in 0..tries {
                if bb::lava(ctx.get(at)) {
                    return false;
                }
                if Self::circle_embedded(ctx, wind.offset(at), self.radius) {
                    self.root = at;
                    return true;
                }
                at = at.offset(0, step, 0);
            }
            self.radius /= 2;
        }
        false
    }

    /// Fills the cone with dripstone blocks through open space; a column stops
    /// at the first base stone after it placed something, and stalagmites
    /// never rise above the terrain surface.
    fn place<W: BlockWorld>(&self, ctx: &mut Ctx<'_, W>, wind: Wind) -> bool {
        let r = self.radius;
        let dy = if self.pointing_up { 1 } else { -1 };
        let mut placed = false;
        for dx in -r..=r {
            for dz in -r..=r {
                let distance = ((dx * dx + dz * dz) as f32).sqrt();
                if distance > r as f32 {
                    continue;
                }
                let mut h = self.height_at(distance);
                if h <= 0 {
                    continue;
                }
                if ctx.rng.next_float() < 0.2 {
                    h = (h as f32 * uniform(ctx.rng, 0.8, 1.0)) as i32;
                }
                let mut at = self.root.offset(dx, 0, dz);
                let max_y = if self.pointing_up { ctx.heightmap(at.x, at.z) } else { i32::MAX };
                let mut started = false;
                for _ in 0..h {
                    if at.y >= max_y {
                        break;
                    }
                    let target = wind.offset(at);
                    let id = ctx.get(target);
                    if is_empty_or_fluid(id) {
                        started = true;
                        placed |= ctx.set(target, bb::DRIPSTONE_BLOCK);
                    } else if started && bb::base_stone_overworld(id) {
                        break;
                    }
                    at = at.offset(0, dy, 0);
                }
            }
        }
        placed
    }
}

// ---------------------------------------------------------------------------
// Huge mushroom
// ---------------------------------------------------------------------------

/// Cap block ids by `huge_mushroom_bits - 1` (bits 1..=9).
const BROWN_CAP: [u32; 9] = [
    bb::BROWN_MUSHROOM_BLOCK_1,
    bb::BROWN_MUSHROOM_BLOCK_2,
    bb::BROWN_MUSHROOM_BLOCK_3,
    bb::BROWN_MUSHROOM_BLOCK_4,
    bb::BROWN_MUSHROOM_BLOCK_5,
    bb::BROWN_MUSHROOM_BLOCK_6,
    bb::BROWN_MUSHROOM_BLOCK_7,
    bb::BROWN_MUSHROOM_BLOCK_8,
    bb::BROWN_MUSHROOM_BLOCK_9,
];
const RED_CAP: [u32; 9] = [
    bb::RED_MUSHROOM_BLOCK_1,
    bb::RED_MUSHROOM_BLOCK_2,
    bb::RED_MUSHROOM_BLOCK_3,
    bb::RED_MUSHROOM_BLOCK_4,
    bb::RED_MUSHROOM_BLOCK_5,
    bb::RED_MUSHROOM_BLOCK_6,
    bb::RED_MUSHROOM_BLOCK_7,
    bb::RED_MUSHROOM_BLOCK_8,
    bb::RED_MUSHROOM_BLOCK_9,
];

/// `huge_mushroom_bits` of the cap block at `(dx, dz)` in a square layer of
/// radius `r`, or `None` when nothing goes there.
///
/// Bedrock's bits name the outside faces: 1 north-west, 2 north, 3
/// north-east, 4 west, 5 top only, 6 east, 7 south-west, 8 south, 9
/// south-east (10 is the stem). -x is west, -z north. With `cut_corners`
/// the four corners are left out and the blocks beside them take the
/// diagonal corner bits; the hidden interior of a side layer (`!top`) stays
/// empty, which makes red caps hollow.
fn cap_bits(dx: i32, dz: i32, r: i32, cut_corners: bool, top: bool) -> Option<u8> {
    let (west, east, north, south) = (dx == -r, dx == r, dz == -r, dz == r);
    if cut_corners && (west || east) && (north || south) {
        return None;
    }
    let mut bits = 5 - i32::from(west) + i32::from(east) - 3 * i32::from(north) + 3 * i32::from(south);
    if cut_corners {
        let (near_w, near_e, near_n, near_s) = (dx == 1 - r, dx == r - 1, dz == 1 - r, dz == r - 1);
        if (near_w && north) || (west && near_n) {
            bits = 1;
        } else if (near_e && north) || (east && near_n) {
            bits = 3;
        } else if (near_w && south) || (west && near_s) {
            bits = 7;
        } else if (near_e && south) || (east && near_s) {
            bits = 9;
        }
    }
    if bits == 5 && !top {
        return None;
    }
    Some(bits as u8)
}

// ---------------------------------------------------------------------------
// Desert well
// ---------------------------------------------------------------------------

/// A block of the desert well.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WellPart {
    Sandstone,
    Slab,
    Water,
}

/// The classic well at offset `(dx, dy, dz)` from the sand block it sits in
/// (`dx`, `dz` in -2..=2, `dy` in -1..=4): a 5x5 sandstone base two blocks
/// deep with a plus of water in its top layer, a sandstone rim with a slab in
/// the middle of each side, four corner pillars and a slab roof around a
/// sandstone centre. `None` leaves the block as it is.
fn well_part(dx: i32, dy: i32, dz: i32) -> Option<WellPart> {
    let (ax, az) = (dx.abs(), dz.abs());
    match dy {
        -1 => Some(WellPart::Sandstone),
        0 if ax + az <= 1 => Some(WellPart::Water),
        0 => Some(WellPart::Sandstone),
        1 if (ax == 2 && dz == 0) || (az == 2 && dx == 0) => Some(WellPart::Slab),
        1 if ax == 2 || az == 2 => Some(WellPart::Sandstone),
        1..=3 if ax == 1 && az == 1 => Some(WellPart::Sandstone),
        4 if ax == 0 && az == 0 => Some(WellPart::Sandstone),
        4 if ax <= 1 && az <= 1 => Some(WellPart::Slab),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Ice spikes
// ---------------------------------------------------------------------------

/// Blocks ice spikes and patches may overwrite (vanilla: air in spikes only,
/// dirt, snow block, ice).
#[inline]
fn ice_spike_replaceable(id: u32) -> bool {
    bb::air(id) || bb::dirt_like(id) || id == bb::SNOW_BLOCK || id == bb::ICE
}

/// Radius of layer `k` of an ice spike `height` tall and `width` wide: shrinks
/// linearly from `width` at the base to 0 at the top.
#[inline]
fn spike_layer_radius(k: i32, height: i32, width: i32) -> f32 {
    (1.0 - k as f32 / height as f32) * width as f32
}

/// The packed ice root of an ice spike goes down to this y (vanilla's fixed
/// floor, unchanged by the 1.18 height extension).
const ICE_SPIKE_ROOT_FLOOR: i32 = 50;

// ---------------------------------------------------------------------------
// Sulfur pool
// ---------------------------------------------------------------------------

/// Largest pool radius; the cell grid is `2 * POOL_MAX_RADIUS + 1` square.
const POOL_MAX_RADIUS: i32 = 4;
const POOL_GRID: usize = (2 * POOL_MAX_RADIUS + 1) as usize;

/// Water depth of a pool cell at squared distance `d2` from the centre of a
/// radius `r` pool (0 = outside): 1 on the rim, 2 inside, 3 in the middle of
/// the largest pools.
fn pool_depth(d2: i32, r: i32) -> i32 {
    if d2 > r * r {
        return 0;
    }
    1 + i32::from(d2 <= (r - 1) * (r - 1)) + i32::from(r >= POOL_MAX_RADIUS && d2 <= 2)
}

/// Blocks a pool may line with sulfur: plain cave rock and cinnabar.
#[inline]
fn pool_liner(id: u32) -> bool {
    bb::base_stone_overworld(id) || id == bb::CINNABAR
}

/// Blocks a sulfur spring trail may turn into sulfur.
#[inline]
fn trail_replaceable(id: u32) -> bool {
    bb::sulfur_trail_rock(id) || bb::dirt_like(id) || bb::sand_like(id) || id == bb::GRAVEL
}

// ---------------------------------------------------------------------------
// Features
// ---------------------------------------------------------------------------

/// `minecraft:desert_well_feature`: the classic sandstone well (see
/// [`well_part`]) sunk into flat sand. `pos` is the air above the ground; the
/// feature lowers it to the sand and refuses ground with a 2-deep hole under
/// any of the 25 base blocks. The archaeology suspicious sand of current
/// wells carries a loot block entity, which this generator does not place.
pub fn desert_well_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    let ground = descend_to_ground(ctx, pos);
    if ctx.get(ground) != bb::SAND {
        return None;
    }
    for dx in -2..=2 {
        for dz in -2..=2 {
            if bb::air(ctx.get(ground.offset(dx, -1, dz))) && bb::air(ctx.get(ground.offset(dx, -2, dz))) {
                return None;
            }
        }
    }
    for dy in -1..=4 {
        for dx in -2..=2 {
            for dz in -2..=2 {
                let id = match well_part(dx, dy, dz) {
                    Some(WellPart::Sandstone) => bb::SANDSTONE,
                    Some(WellPart::Slab) => bb::SANDSTONE_SLAB,
                    Some(WellPart::Water) => bb::WATER,
                    None => continue,
                };
                ctx.set(ground.offset(dx, dy, dz), id);
            }
        }
    }
    Some(ground)
}

/// `minecraft:dripstone_cluster_feature` (dripstone caves, 48-96 attempts per
/// chunk at random cave heights): vanilla dripstone cluster. Columns within a
/// random 2-8 by 2-8 rectangle find their floor and ceiling (12 blocks), coat
/// them with 2-4 dripstone blocks, sometimes sink a one-block pool, and grow
/// stalactite/stalagmite pairs (2-6 high, longest near the centre, merged
/// when they meet).
pub fn dripstone_cluster_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    place_cluster(ctx, pos, &DRIPSTONE_KIND, true)
}

/// `minecraft:huge_mushroom_feature` (mushroom fields, roofed forest): a red
/// or brown huge mushroom (even odds) 4-6 tall, doubled with 1/12 chance.
/// Needs dirt, grass, mycelium or podzol below and air or leaves in its
/// volume. Brown caps are one flat radius-3 layer without corners; red caps
/// are a 3x3 top over three hollow radius-2 side layers. Faces carry the
/// [`cap_bits`] values, the stem is `mushroom_stem` with bits 10 (stem sides).
/// Blocks are only written where the current block is not opaque.
pub fn huge_mushroom_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    let red = ctx.rng.next_int(2) == 1;
    let mut height = ctx.rng.next_int(3) + 4;
    if ctx.rng.next_int(12) == 0 {
        height *= 2;
    }
    if pos.y <= ctx.world.min_y() || pos.y + height + 1 >= ctx.world.max_y() {
        return None;
    }
    if !bb::huge_mushroom_soil(ctx.get(pos.below())) {
        return None;
    }
    for y in pos.y..=pos.y + height + 1 {
        let r = if y <= pos.y + 3 { 0 } else { 3 };
        for dx in -r..=r {
            for dz in -r..=r {
                let id = ctx.get(BlockPos::new(pos.x + dx, y, pos.z + dz));
                if !(bb::air(id) || bb::leaves(id)) {
                    return None;
                }
            }
        }
    }
    let top = pos.y + height;
    let (cap, first) = if red { (&RED_CAP, top - 3) } else { (&BROWN_CAP, top) };
    for y in first..=top {
        let is_top = y == top;
        let r = match (red, is_top) {
            (false, _) => 3,
            (true, true) => 1,
            (true, false) => 2,
        };
        let cut_corners = !red || !is_top;
        for dx in -r..=r {
            for dz in -r..=r {
                let Some(bits) = cap_bits(dx, dz, r, cut_corners, is_top) else {
                    continue;
                };
                let at = BlockPos::new(pos.x + dx, y, pos.z + dz);
                if !props::is_opaque(ctx.get(at)) {
                    ctx.set(at, cap[usize::from(bits) - 1]);
                }
            }
        }
    }
    for i in 0..height {
        let at = pos.offset(0, i, 0);
        if !props::is_opaque(ctx.get(at)) {
            ctx.set(at, bb::MUSHROOM_STEM);
        }
    }
    Some(pos)
}

/// `minecraft:ice_patch_feature` (ice spikes, first pass): a packed ice disc
/// of radius 2-3 on snow ground, replacing dirt, grass, snow blocks and ice
/// from one block below to one block above the ground. Grass is included
/// because Bedrock grass carries the `dirt` tag in other features; the ground
/// itself must be a snow block, as in vanilla.
pub fn ice_patch_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    let ground = descend_to_ground(ctx, pos);
    if ctx.get(ground) != bb::SNOW_BLOCK {
        return None;
    }
    let r = ctx.rng.next_int(2) + 2;
    let mut placed = false;
    for dx in -r..=r {
        for dz in -r..=r {
            if dx * dx + dz * dz > r * r {
                continue;
            }
            for dy in -1..=1 {
                let at = ground.offset(dx, dy, dz);
                let id = ctx.get(at);
                if bb::dirt_like(id) || id == bb::GRASS_BLOCK || id == bb::SNOW_BLOCK || id == bb::ICE {
                    placed |= ctx.set(at, bb::PACKED_ICE);
                }
            }
        }
    }
    placed.then_some(ground)
}

/// `minecraft:ice_spike_feature` (ice spikes, first pass): vanilla ice spike.
/// On a snow block, a packed ice cone 7-10 tall and `height / 4` (+0/1) wide,
/// mirrored below ground for wide spikes, with ragged edges (25 % of rim
/// blocks dropped). One in 60 wide spikes is lifted 10-39 blocks, and the
/// packed ice root under every spike runs down to y 50 (corner roots in short
/// dashes), which makes those the 50+ block tall spikes.
pub fn ice_spike_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    let ground = descend_to_ground(ctx, pos);
    if ctx.get(ground) != bb::SNOW_BLOCK {
        return None;
    }
    let lift = ctx.rng.next_int(4);
    let mut base = ground.offset(0, lift, 0);
    let height = ctx.rng.next_int(4) + 7;
    let width = height / 4 + ctx.rng.next_int(2);
    if width > 1 && ctx.rng.next_int(60) == 0 {
        let extra = 10 + ctx.rng.next_int(30);
        base = base.offset(0, extra, 0);
    }
    for k in 0..height {
        let f = spike_layer_radius(k, height, width);
        let l = f.ceil() as i32;
        for dx in -l..=l {
            let fx = dx.abs() as f32 - 0.25;
            for dz in -l..=l {
                let fz = dz.abs() as f32 - 0.25;
                if !((dx == 0 && dz == 0) || fx * fx + fz * fz <= f * f) {
                    continue;
                }
                let rim = dx.abs() == l || dz.abs() == l;
                if rim && ctx.rng.next_float() > 0.75 {
                    continue;
                }
                let at = base.offset(dx, k, dz);
                if ice_spike_replaceable(ctx.get(at)) {
                    ctx.set(at, bb::PACKED_ICE);
                }
                if k != 0 && l > 1 {
                    let at = base.offset(dx, -k, dz);
                    if ice_spike_replaceable(ctx.get(at)) {
                        ctx.set(at, bb::PACKED_ICE);
                    }
                }
            }
        }
    }
    let root = (width - 1).clamp(0, 1);
    for dx in -root..=root {
        for dz in -root..=root {
            let mut at = base.offset(dx, -1, dz);
            let mut run = if dx.abs() == 1 && dz.abs() == 1 { ctx.rng.next_int(5) } else { 50 };
            while at.y > ICE_SPIKE_ROOT_FLOOR {
                let id = ctx.get(at);
                if !(ice_spike_replaceable(id) || id == bb::PACKED_ICE) {
                    break;
                }
                ctx.set(at, bb::PACKED_ICE);
                at = at.below();
                run -= 1;
                if run <= 0 {
                    let skip = ctx.rng.next_int(5) + 1;
                    at = at.offset(0, -skip, 0);
                    run = ctx.rng.next_int(5);
                }
            }
        }
    }
    Some(base)
}

/// `minecraft:large_dripstone_feature` (dripstone caves, 10-48 attempts per
/// chunk): vanilla large dripstone. From an open start it finds floor and
/// ceiling (30 blocks, both must be base stone, dripstone or lava, gap >= 4),
/// picks a radius 3..=clamp(gap * 0.33, 3, 19), and builds a stalactite and a
/// stalagmite cone (see [`cone_height`]) of dripstone blocks, each first sunk
/// into the rock until its base circle is embedded, leaning with a random wind
/// when the radius is at least 4.
pub fn large_dripstone_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    if !is_empty_or_fluid(ctx.get(pos)) {
        return None;
    }
    let edge = |id: u32| is_dripstone_base(id) || bb::lava(id);
    let ceiling = scan_edge(ctx, pos, 1, LARGE_SEARCH, is_empty_or_water, edge)?;
    let floor = scan_edge(ctx, pos, -1, LARGE_SEARCH, is_empty_or_water, edge)?;
    let gap = ceiling - floor - 1;
    if gap < 4 {
        return None;
    }
    let radius_max = ((gap as f32 * LARGE_MAX_RADIUS_TO_HEIGHT) as i32).clamp(LARGE_RADIUS.0, LARGE_RADIUS.1);
    let radius = ctx.rng.next_int_inclusive(LARGE_RADIUS.0, radius_max);
    let mut stalactite = LargeDripstone {
        root: BlockPos::new(pos.x, ceiling - 1, pos.z),
        pointing_up: false,
        radius,
        bluntness: uniform(ctx.rng, LARGE_STALACTITE_BLUNTNESS.0, LARGE_STALACTITE_BLUNTNESS.1),
        scale: uniform(ctx.rng, LARGE_HEIGHT_SCALE.0, LARGE_HEIGHT_SCALE.1),
    };
    let mut stalagmite = LargeDripstone {
        root: BlockPos::new(pos.x, floor + 1, pos.z),
        pointing_up: true,
        radius,
        bluntness: uniform(ctx.rng, LARGE_STALAGMITE_BLUNTNESS.0, LARGE_STALAGMITE_BLUNTNESS.1),
        scale: uniform(ctx.rng, LARGE_HEIGHT_SCALE.0, LARGE_HEIGHT_SCALE.1),
    };
    // Vanilla's minimum bluntness for wind is 0, so only the radius gates it.
    let wind = if radius >= LARGE_MIN_RADIUS_FOR_WIND { Wind::new(pos.y, ctx.rng) } else { Wind::NONE };
    let mut placed = false;
    if stalactite.anchor(ctx, wind) {
        placed |= stalactite.place(ctx, wind);
    }
    if stalagmite.anchor(ctx, wind) {
        placed |= stalagmite.place(ctx, wind);
    }
    placed.then_some(pos)
}

/// `minecraft:pointed_dripstone_feature` (reached through
/// `small_dripstone_snap_to_surface_feature`): one small pointed dripstone on
/// the nearest floor or ceiling within 12 blocks, over a random patch of
/// dripstone blocks (see `place_small_spike`).
pub fn pointed_dripstone_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    place_small_spike(ctx, pos, &DRIPSTONE_KIND)
}

/// `minecraft:sulfur_pool` (first step of `sulfur_pool_with_potent_sulfur`,
/// snapped and embedded into a sulfur floor; the potent sulfur search that
/// follows needs sulfur under water within 8 blocks below `pos`).
///
/// Shape inferred from that data (no published algorithm): a round pool of
/// radius 2-4 sunk into the floor with `pos` as its centre, 1 deep on a
/// ragged rim (rim cells kept with 1/2 chance), 2 inside and 3 in the middle
/// of radius-4 pools. A cell only holds water when every water block is
/// carved out of solid ground and is enclosed by solid blocks or other pool
/// water on the sides and below; cells that are not are dropped until the
/// pool is stable. Base stone and cinnabar touching the water become sulfur,
/// so the pool is always lined with the block potent sulfur replaces.
pub fn sulfur_pool<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    if ctx.get(pos) != bb::SULFUR || !bb::air(ctx.get(pos.above())) {
        return None;
    }
    let r = ctx.rng.next_int_inclusive(2, POOL_MAX_RADIUS);
    let c = POOL_MAX_RADIUS;
    let mut depth = [[0i32; POOL_GRID]; POOL_GRID];
    for dx in -r..=r {
        for dz in -r..=r {
            let d2 = dx * dx + dz * dz;
            let d = pool_depth(d2, r);
            if d == 0 || (d2 > (r - 1) * (r - 1) && ctx.rng.next_int(2) == 0) {
                continue;
            }
            depth[(dx + c) as usize][(dz + c) as usize] = d;
        }
    }
    // Depth of the cell at offset (dx, dz), 0 outside the grid.
    let cell = |depth: &[[i32; POOL_GRID]; POOL_GRID], dx: i32, dz: i32| -> i32 {
        if dx.abs() > c || dz.abs() > c { 0 } else { depth[(dx + c) as usize][(dz + c) as usize] }
    };
    // Drop cells that cannot hold water until nothing changes; each pass
    // removes at least one of the at most 81 cells.
    loop {
        let mut changed = false;
        for dx in -c..=c {
            for dz in -c..=c {
                let d = cell(&depth, dx, dz);
                if d == 0 {
                    continue;
                }
                let mut ok = holds_water(ctx.get(pos.offset(dx, -d, dz)));
                for i in 0..d {
                    if !ok {
                        break;
                    }
                    let at = pos.offset(dx, -i, dz);
                    let id = ctx.get(at);
                    ok = holds_water(id) && id != bb::BEDROCK;
                    for dir in Direction::ALL {
                        if !ok {
                            break;
                        }
                        let (sx, sz) = dir.delta();
                        ok = cell(&depth, dx + sx, dz + sz) > i || holds_water(ctx.get(at.side(dir)));
                    }
                }
                if !ok {
                    depth[(dx + c) as usize][(dz + c) as usize] = 0;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    if cell(&depth, 0, 0) == 0 {
        return None;
    }
    for dx in -c..=c {
        for dz in -c..=c {
            let d = cell(&depth, dx, dz);
            if d == 0 {
                continue;
            }
            for i in 0..d {
                let at = pos.offset(dx, -i, dz);
                ctx.set(at, bb::WATER);
                for dir in Direction::ALL {
                    let (sx, sz) = dir.delta();
                    let side = at.side(dir);
                    if cell(&depth, dx + sx, dz + sz) <= i && pool_liner(ctx.get(side)) {
                        ctx.set(side, bb::SULFUR);
                    }
                }
            }
            let bottom = pos.offset(dx, -d, dz);
            if pool_liner(ctx.get(bottom)) {
                ctx.set(bottom, bb::SULFUR);
            }
        }
    }
    Some(pos)
}

/// `minecraft:sulfur_spike_cluster_feature` (sulfur caves, 48-96 attempts per
/// chunk): inferred as the sulfur counterpart of the dripstone cluster (same
/// rule shape as `dripstone_cluster_feature`, and `sulfur_spike` carries the
/// pointed dripstone states): the vanilla cluster algorithm with sulfur as
/// the coating block and sulfur spikes, without pools (sulfur caves get their
/// water from `sulfur_pool`).
pub fn sulfur_spike_cluster_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    place_cluster(ctx, pos, &SULFUR_KIND, false)
}

/// `minecraft:sulfur_spike_feature` (sulfur caves, 192-256 attempts per chunk,
/// the counterpart of the small dripstone rule): inferred as a small spike
/// (see `place_small_spike`) of sulfur spikes growing from sulfur, cinnabar or
/// base stone, with a sulfur patch around its base.
pub fn sulfur_spike_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    place_small_spike(ctx, pos, &SULFUR_KIND)
}

/// `minecraft:sulfur_spring_trail_to_surface_feature` (sulfur caves, before
/// the surface pass, 1-2 per chunk; snapped to a cave ceiling, so `pos` is
/// the open block under it).
///
/// Shape inferred from the name and data (no published algorithm): a
/// one-block, face-connected trail of sulfur from the ceiling up through the
/// rock to the terrain surface, wandering sideways on a third of its steps.
/// It only converts the blocks the sulfur spring structures may sit in
/// (stone types, soil, sand, gravel, sandstone, terracotta) and passes
/// through other blocks and open pockets unchanged; the topmost surface block
/// it reaches is sulfur too, marking the cave below.
///
/// `minecraft:random_sulfur_spring_with_tuff_feature` (the weighted surface
/// spring structures) is referenced by nothing in the 1.26.51 data, so whether
/// BDS places it from this built-in is unknown; it is not placed here.
pub fn sulfur_spring_trail_to_surface_feature<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    pos: BlockPos,
) -> Option<BlockPos> {
    let mut at = pos.above();
    if ctx.heightmap(at.x, at.z) - 1 <= at.y {
        return None;
    }
    let mut last = None;
    // Every step either climbs or moves sideways; the bound covers a trail
    // that climbs the whole world height with a sideways step for each climb.
    let steps = 2 * (ctx.world.max_y() - ctx.world.min_y());
    for _ in 0..steps {
        let top = ctx.heightmap(at.x, at.z) - 1;
        if at.y > top {
            break;
        }
        if trail_replaceable(ctx.get(at)) && ctx.set(at, bb::SULFUR) {
            last = Some(at);
        }
        if at.y == top {
            break;
        }
        at = if ctx.rng.next_int(3) == 0 {
            at.side(Direction::ALL[ctx.rng.next_int(4) as usize])
        } else {
            at.above()
        };
    }
    last
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order(len: i32, merge: bool) -> Vec<Thickness> {
        (0..len).map(|i| thickness_at(i, len, merge)).collect()
    }

    #[test]
    fn dripstone_thickness_runs_base_to_tip() {
        use super::Thickness::*;
        assert_eq!(order(1, false), [Tip]);
        assert_eq!(order(2, false), [Frustum, Tip]);
        assert_eq!(order(3, false), [Base, Frustum, Tip]);
        assert_eq!(order(4, false), [Base, Middle, Frustum, Tip]);
        assert_eq!(order(6, false), [Base, Middle, Middle, Middle, Frustum, Tip]);
        assert_eq!(order(3, true), [Base, Frustum, Merge]);
        assert_eq!(order(1, true), [Merge]);
    }

    #[test]
    fn brown_cap_layer_bits() {
        // Corners cut, the blocks next to them take the diagonal bits.
        for (dx, dz) in [(-3, -3), (3, -3), (-3, 3), (3, 3)] {
            assert_eq!(cap_bits(dx, dz, 3, true, true), None);
        }
        assert_eq!(cap_bits(-2, -3, 3, true, true), Some(1));
        assert_eq!(cap_bits(-3, -2, 3, true, true), Some(1));
        assert_eq!(cap_bits(2, -3, 3, true, true), Some(3));
        assert_eq!(cap_bits(-3, 2, 3, true, true), Some(7));
        assert_eq!(cap_bits(3, 2, 3, true, true), Some(9));
        assert_eq!(cap_bits(0, -3, 3, true, true), Some(2));
        assert_eq!(cap_bits(-3, 0, 3, true, true), Some(4));
        assert_eq!(cap_bits(3, 0, 3, true, true), Some(6));
        assert_eq!(cap_bits(0, 3, 3, true, true), Some(8));
        assert_eq!(cap_bits(1, 1, 3, true, true), Some(5));
        let placed = (-3..=3).flat_map(|x| (-3..=3).map(move |z| (x, z))).filter(|&(x, z)| cap_bits(x, z, 3, true, true).is_some()).count();
        assert_eq!(placed, 49 - 4);
    }

    #[test]
    fn red_cap_layer_bits() {
        // Top: full 3x3 with corner bits and a centre top.
        assert_eq!(cap_bits(-1, -1, 1, false, true), Some(1));
        assert_eq!(cap_bits(1, -1, 1, false, true), Some(3));
        assert_eq!(cap_bits(-1, 1, 1, false, true), Some(7));
        assert_eq!(cap_bits(1, 1, 1, false, true), Some(9));
        assert_eq!(cap_bits(0, 0, 1, false, true), Some(5));
        // Side layer: hollow ring of radius 2 without corners.
        assert_eq!(cap_bits(0, 0, 2, true, false), None);
        assert_eq!(cap_bits(1, 1, 2, true, false), None);
        assert_eq!(cap_bits(2, 2, 2, true, false), None);
        assert_eq!(cap_bits(-1, -2, 2, true, false), Some(1));
        assert_eq!(cap_bits(0, -2, 2, true, false), Some(2));
        assert_eq!(cap_bits(2, 1, 2, true, false), Some(9));
        let ring = (-2..=2).flat_map(|x| (-2..=2).map(move |z| (x, z))).filter(|&(x, z)| cap_bits(x, z, 2, true, false).is_some()).count();
        assert_eq!(ring, 12);
        // Every placed value is a valid cap face (1..=9).
        for r in 1..=3 {
            for x in -r..=r {
                for z in -r..=r {
                    for (cut, top) in [(true, true), (true, false), (false, true)] {
                        if let Some(b) = cap_bits(x, z, r, cut, top) {
                            assert!((1..=9).contains(&b));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn cluster_chance_and_mapping() {
        assert_eq!(clamped_map(-1.0, 0.0, 3.0, 0.1, 1.0), 0.1);
        assert_eq!(clamped_map(5.0, 0.0, 3.0, 0.1, 1.0), 1.0);
        assert!((column_chance(5, 5, 5, 0) - 0.1).abs() < 1e-6);
        assert!((column_chance(5, 5, 0, 0) - 1.0).abs() < 1e-6);
        assert!(column_chance(5, 5, 4, 0) < column_chance(5, 5, 3, 0));
        // A zero density never grows, a full one stays in range.
        let mut rng = BedrockRandom::new(7);
        assert_eq!(cluster_spike_height(&mut rng, 0, 0, 0.0, 6), 0);
        for _ in 0..1000 {
            let h = cluster_spike_height(&mut rng, 1, 2, 1.0, 6);
            assert!((0..=6).contains(&h));
        }
    }

    #[test]
    fn large_dripstone_cone_profile() {
        let (max_r, scale, blunt) = (8.0, 1.0, 0.5);
        assert!(cone_height(max_r, max_r, scale, blunt) < 0.5, "cone reaches ~0 at the rim");
        let mut last = f32::INFINITY;
        for i in 0..=16 {
            let h = cone_height(i as f32 * 0.5, max_r, scale, blunt);
            assert!(h <= last + 1e-4, "height never grows outwards");
            last = h;
        }
        // Bluntness flattens the tip: below it the height is constant.
        assert_eq!(cone_height(0.0, max_r, scale, blunt), cone_height(0.4, max_r, scale, blunt));
        assert!(cone_height(0.0, max_r, 2.0, blunt) > cone_height(0.0, max_r, 1.0, blunt));
    }

    #[test]
    fn desert_well_layout() {
        let mut counts = [0usize; 3];
        for dy in -2..=5 {
            for dx in -2..=2 {
                for dz in -2..=2 {
                    match well_part(dx, dy, dz) {
                        Some(p) => {
                            assert!((-1..=4).contains(&dy), "well block outside its 6 layers at dy {dy}");
                            counts[p as usize] += 1;
                        }
                        None => assert!(dy != -1 && dy != 0, "base layers are solid"),
                    }
                }
            }
        }
        // 25 + 20 base, 12 rim + 4*3 pillars + 1 roof centre.
        assert_eq!(counts[WellPart::Sandstone as usize], 25 + 20 + 12 + 12 + 1);
        assert_eq!(counts[WellPart::Slab as usize], 4 + 8);
        assert_eq!(counts[WellPart::Water as usize], 5);
        assert_eq!(well_part(0, 0, 0), Some(WellPart::Water));
        assert_eq!(well_part(0, 2, 0), None);
        assert_eq!(well_part(2, 1, 0), Some(WellPart::Slab));
        assert_eq!(well_part(1, 3, -1), Some(WellPart::Sandstone));
    }

    #[test]
    fn ice_spike_layers_taper() {
        assert_eq!(spike_layer_radius(0, 8, 2), 2.0);
        let mut last = f32::INFINITY;
        for k in 0..10 {
            let r = spike_layer_radius(k, 10, 3);
            assert!(r < last && r > 0.0);
            last = r;
        }
    }

    #[test]
    fn pool_depth_profile() {
        assert_eq!(pool_depth(17, 4), 0);
        assert_eq!(pool_depth(16, 4), 1);
        assert_eq!(pool_depth(9, 4), 2);
        assert_eq!(pool_depth(0, 4), 3);
        assert_eq!(pool_depth(0, 2), 2);
        assert_eq!(pool_depth(4, 2), 1);
        assert_eq!(pool_depth(5, 2), 0);
    }

    #[test]
    fn gaussian_is_standard_normal() {
        let mut rng = BedrockRandom::new(12345);
        let n = 20_000;
        let samples: Vec<f32> = (0..n).map(|_| gaussian(&mut rng)).collect();
        let mean = samples.iter().sum::<f32>() / n as f32;
        let var = samples.iter().map(|s| (s - mean) * (s - mean)).sum::<f32>() / n as f32;
        assert!(mean.abs() < 0.05, "mean {mean}");
        assert!((var - 1.0).abs() < 0.05, "variance {var}");
        assert!(samples.iter().all(|s| s.is_finite()));
    }
}
