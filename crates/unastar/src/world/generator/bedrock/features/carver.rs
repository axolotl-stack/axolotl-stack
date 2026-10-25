//! The worm cave carvers: `minecraft:cave_carver_feature`,
//! `minecraft:underwater_cave_carver_feature` and
//! `minecraft:nether_cave_carver_feature`.
//!
//! Vanilla rules place them once per chunk in `pregeneration_pass` at the
//! chunk's min corner (`y = 0`). Like the vanilla carvers of both editions,
//! a call carves the caves of every *source* chunk within
//! [`SOURCE_RADIUS`] chunks, clipped to the column
//! `[pos.x, pos.x + 16) x [pos.z, pos.z + 16)`: each source chunk's caves
//! are a pure function of `(world seed, feature, source chunk)`, so the
//! pieces carved by neighbouring chunks line up without any shared state.
//!
//! Per source chunk (the classic worm algorithm):
//! - the chunk starts caves with probability `1 / skip_carve_chance` (see
//!   [`source_seed`]);
//! - `n = r(r(r(bound) + 1) + 1)` cave starts (`bound` 15, nether 10), each
//!   at a random block of the source chunk, at
//!   `y = min_y + 8 + r(height_limit)`;
//! - each start samples `horizontal_radius_multiplier`,
//!   `vertical_radius_multiplier` and `floor_level` once; with chance 1/4 it
//!   opens a room (an ellipsoid of radius `2.5..8.5`, vertical radius scaled
//!   by a `y_scale` draw) and gets `r(4)` extra tunnels;
//! - a tunnel walks up to 112 steps with smoothly drifting yaw/pitch; its
//!   radius is `1.5 + sin(pi * step / steps) * thickness`; thick tunnels split
//!   once into two thinner side branches; 3 in 4 steps carve an ellipsoid.
//!
//! An ellipsoid carves block centres with `x^2 + y^2 + z^2 < 1` (coordinates
//! relative to the centre, divided by the radii) and `y > floor_level`, which
//! flattens cave floors.
//!
//! Interpretation of the pack keys (documented next to each [`Params`]
//! field): `height_limit` is the span of start heights above `min_y + 8`
//! (vanilla overworld values 236 and 103 put starts in `-56..=179` and
//! `-56..=46`, the vanilla 1.18 cave ranges); `width_modifier` is added to
//! every radius.
//!
//! The carve pass runs on the proto chunk alone (see `decorate.rs`), so reads
//! outside the chunk return `OUT_OF_BOUNDS` and the liquid checks cannot see
//! water across the chunk border.

use super::super::generated::bb;
use super::super::rng::BedrockRandom;
use super::super::world::{BlockPos, BlockWorld, Ctx};
use std::f64::consts::{FRAC_PI_2, PI, TAU};

/// Chunk radius of the source chunks whose caves may reach the carved chunk
/// (vanilla: 8, a 17 x 17 square).
pub const SOURCE_RADIUS: i32 = 8;

/// Steps of the longest tunnel: 112 blocks, the vanilla carver range
/// (`(4 * 2 - 1) * 16`). A tunnel takes `112 - r(28)` steps.
const TUNNEL_STEPS: i32 = 112;

/// Lowest tunnel start above `min_y` (vanilla 1.18 `above_bottom: 8`).
const START_ABOVE_BOTTOM: i32 = 8;

/// The dry overworld carver fills with lava at and below `min_y + 8`
/// (y = -56 in the overworld), the vanilla 1.18+ cave `lava_level`
/// (`above_bottom: 8`, inclusive).
const LAVA_ABOVE_BOTTOM: i32 = 8;

const NEIGHBOURS: [(i32, i32, i32); 6] = [
    (1, 0, 0),
    (-1, 0, 0),
    (0, 1, 0),
    (0, -1, 0),
    (0, 0, 1),
    (0, 0, -1),
];

/// Which carver type the parameters belong to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `minecraft:cave_carver_feature`: dry caves; never breaches water, lava
    /// below the lava level.
    Cave,
    /// `minecraft:underwater_cave_carver_feature`: flooded caves, carved only
    /// where they connect to existing water.
    UnderwaterCave,
    /// `minecraft:nether_cave_carver_feature`: nether replaceables, taller
    /// tunnels, never breaches a liquid.
    NetherCave,
}

impl Kind {
    /// Bound of the nested cave-start count draw (vanilla: 15, nether 10).
    fn cave_bound(self) -> i32 {
        match self {
            Kind::NetherCave => 10,
            Kind::Cave | Kind::UnderwaterCave => 15,
        }
    }

    /// Tunnel thickness: `2r + r` with a rare (1/10) up to 4x widening for
    /// overworld caves; nether tunnels are `(2r + r) * 2` (vanilla).
    fn thickness(self, rng: &mut BedrockRandom) -> f64 {
        match self {
            Kind::NetherCave => f64::from((rng.next_float() * 2.0 + rng.next_float()) * 2.0),
            Kind::Cave | Kind::UnderwaterCave => {
                let mut t = rng.next_float() * 2.0 + rng.next_float();
                if rng.next_int(10) == 0 {
                    t *= rng.next_float() * rng.next_float() * 3.0 + 1.0;
                }
                f64::from(t)
            }
        }
    }

    /// Vertical/horizontal radius ratio of main tunnels (vanilla: 1, nether
    /// 5, which makes nether caves tall). Side branches always use 1.
    fn tunnel_y_scale(self) -> f64 {
        match self {
            Kind::NetherCave => 5.0,
            Kind::Cave | Kind::UnderwaterCave => 1.0,
        }
    }
}

/// A float range as written in pack data (`[min, max]`), drawn uniformly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FloatRange {
    pub min: f32,
    pub max: f32,
}

impl FloatRange {
    /// Uniform draw in `[min, max)` (one draw even when `min == max`, so the
    /// draw sequence does not depend on the data).
    #[inline]
    pub fn sample(self, rng: &mut BedrockRandom) -> f64 {
        f64::from(self.min + (self.max - self.min) * rng.next_float())
    }
}

/// Parameters of one carver feature.
#[derive(Debug, Clone, Copy)]
pub struct Params {
    pub kind: Kind,
    /// Per-feature salt (FNV-1a of the feature identifier, computed at build
    /// time) so two carvers of the same type pick different source chunks.
    pub salt: u64,
    /// `fill_with`: what carved terrain becomes.
    pub fill_with: u32,
    /// `replace_air_with`: what air inside the carved volume becomes
    /// (`None`: air is left alone).
    pub replace_air_with: Option<u32>,
    /// `width_modifier`: blocks added to every cave radius.
    pub width_modifier: f32,
    /// `skip_carve_chance`: a source chunk starts caves with probability
    /// `1 / skip_carve_chance` (Mojang documents the key as "1 / value").
    /// Values `<= 1` carve from every source chunk.
    pub skip_carve_chance: i32,
    /// `height_limit`: cave starts are uniform in
    /// `[min_y + 8, min_y + 8 + height_limit)`.
    pub height_limit: i32,
    /// `y_scale`: vertical/horizontal radius ratio of rooms.
    pub y_scale: FloatRange,
    /// `horizontal_radius_multiplier`: scales tunnel radii (drawn per start).
    pub horizontal_radius_multiplier: FloatRange,
    /// `vertical_radius_multiplier`: scales tunnel vertical radii (per start).
    pub vertical_radius_multiplier: FloatRange,
    /// `floor_level`: relative height under which an ellipsoid carves nothing
    /// (`-1` = full ellipsoid; drawn per start).
    pub floor_level: FloatRange,
}

/// Carve the caves reaching the 16 x 16 column at `pos` (the chunk's min
/// corner); `Some(pos)` when any block changed.
pub fn place<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    pos: BlockPos,
    params: &Params,
) -> Option<BlockPos> {
    let carver = Carver {
        params,
        x0: pos.x,
        z0: pos.z,
        min_y: ctx.world.min_y(),
        max_y: ctx.world.max_y(),
    };
    let (chunk_x, chunk_z) = (pos.x >> 4, pos.z >> 4);
    let mut carved = false;
    for sx in chunk_x - SOURCE_RADIUS..=chunk_x + SOURCE_RADIUS {
        for sz in chunk_z - SOURCE_RADIUS..=chunk_z + SOURCE_RADIUS {
            let Some(seed) = source_seed(ctx.seed, params.salt, sx, sz, params.skip_carve_chance)
            else {
                continue;
            };
            let mut rng = BedrockRandom::new(seed);
            carved |= carver.carve_source(ctx, &mut rng, sx, sz);
        }
    }
    carved.then_some(pos)
}

/// SplitMix64 finaliser: a bijective 64-bit mix.
#[inline]
const fn mix64(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Whether source chunk `(chunk_x, chunk_z)` starts caves, and if so the
/// 32-bit seed of its [`BedrockRandom`].
///
/// Formula: `h = splitmix64(world_seed ^ salt ^ chunk_x * K1 ^ chunk_z * K2)`
/// (wrapping 64-bit, `K1`/`K2` odd constants); the chunk carves when
/// `(h >> 32) % skip_carve_chance == 0` and seeds its random with the low 32
/// bits. Rolling the skip chance on the hash instead of the first random draw
/// avoids seeding an MT19937 (2.5 KiB of state) for each of the 289 source
/// chunks when only ~1/33 of them carve. BDS's own source-chunk seeding is not
/// observable from data: statistical parity, exact draw order unknown.
pub fn source_seed(
    world_seed: i64,
    salt: u64,
    chunk_x: i32,
    chunk_z: i32,
    skip_carve_chance: i32,
) -> Option<u32> {
    const K1: u64 = 0x9e37_79b9_7f4a_7c15;
    const K2: u64 = 0xc2b2_ae3d_27d4_eb4f;
    let h = mix64(
        world_seed as u64
            ^ salt
            ^ (i64::from(chunk_x) as u64).wrapping_mul(K1)
            ^ (i64::from(chunk_z) as u64).wrapping_mul(K2),
    );
    if skip_carve_chance > 1 && !((h >> 32) as u32).is_multiple_of(skip_carve_chance as u32) {
        return None;
    }
    Some(h as u32)
}

/// `[lo, hi]` blocks whose centres may lie within `radius` of `center`
/// (`floor(center - radius) ..= floor(center + radius)`), clipped to
/// `[min, max]`; `None` when the span misses it.
#[inline]
pub fn clip_span(center: f64, radius: f64, min: i32, max: i32) -> Option<(i32, i32)> {
    let lo = ((center - radius).floor() as i32).max(min);
    let hi = ((center + radius).floor() as i32).min(max);
    (lo <= hi).then_some((lo, hi))
}

/// Whether a block centre at relative coordinates (`dxz_sq = dx^2 + dz^2`,
/// `dy`; each divided by its radius) is carved: inside the unit sphere and
/// strictly above `floor_level`.
#[inline]
pub fn in_carve_shape(dxz_sq: f64, dy: f64, floor_level: f64) -> bool {
    dy > floor_level && dxz_sq + dy * dy < 1.0
}

/// Whether a tunnel at `(x, z)` on `step` of `steps` can still reach the
/// column centred on `(center_x, center_z)`: it moves at most one block per
/// step, and carves at most `thickness + 2` (plus half a chunk) away. Vanilla
/// stops a tunnel once this fails.
#[inline]
pub fn can_reach(
    center_x: f64,
    center_z: f64,
    x: f64,
    z: f64,
    step: i32,
    steps: i32,
    thickness: f64,
) -> bool {
    let (dx, dz) = (x - center_x, z - center_z);
    let remaining = f64::from(steps - step);
    let reach = thickness + 2.0 + 16.0;
    dx * dx + dz * dz - remaining * remaining <= reach * reach
}

/// Ground the overworld carvers may replace.
#[inline]
fn overworld_carvable(id: u32) -> bool {
    bb::carver_replaceable(id) || bb::terracotta(id)
}

/// Per-start tunnel shape draws.
#[derive(Clone, Copy)]
struct Shape {
    horizontal: f64,
    vertical: f64,
    floor: f64,
}

/// State of one tunnel when it starts (or branches off).
#[derive(Clone, Copy)]
struct Tunnel {
    x: f64,
    y: f64,
    z: f64,
    yaw: f64,
    pitch: f64,
    thickness: f64,
    y_scale: f64,
    start: i32,
    steps: i32,
}

/// One carve call: the parameters and the target column.
struct Carver<'p> {
    params: &'p Params,
    /// Min corner of the carved column.
    x0: i32,
    z0: i32,
    min_y: i32,
    max_y: i32,
}

impl Carver<'_> {
    /// Simulate every cave of one source chunk, carving the parts inside the
    /// target column.
    fn carve_source<W: BlockWorld>(
        &self,
        ctx: &mut Ctx<'_, W>,
        rng: &mut BedrockRandom,
        sx: i32,
        sz: i32,
    ) -> bool {
        let p = self.params;
        let width = f64::from(p.width_modifier);
        let starts = {
            let a = rng.next_int(p.kind.cave_bound()) + 1;
            let b = rng.next_int(a) + 1;
            rng.next_int(b)
        };
        let mut carved = false;
        for _ in 0..starts {
            let x = f64::from(sx * 16 + rng.next_int(16));
            let y = f64::from(self.min_y + START_ABOVE_BOTTOM + rng.next_int(p.height_limit));
            let z = f64::from(sz * 16 + rng.next_int(16));
            let shape = Shape {
                horizontal: p.horizontal_radius_multiplier.sample(rng),
                vertical: p.vertical_radius_multiplier.sample(rng),
                floor: p.floor_level.sample(rng),
            };
            let mut tunnels = 1;
            if rng.next_int(4) == 0 {
                // A room: radius 1.5 + thickness (the tunnel formula at its
                // widest point), unaffected by the radius multipliers.
                let y_scale = p.y_scale.sample(rng);
                let thickness = 1.0 + f64::from(rng.next_float()) * 6.0;
                let radius = 1.5 + thickness + width;
                carved |=
                    self.carve_ellipsoid(ctx, x + 1.0, y, z, radius, radius * y_scale, shape.floor);
                tunnels += rng.next_int(4);
            }
            for _ in 0..tunnels {
                let yaw = f64::from(rng.next_float()) * TAU;
                let pitch = (f64::from(rng.next_float()) - 0.5) / 4.0;
                let thickness = p.kind.thickness(rng);
                let steps = TUNNEL_STEPS - rng.next_int(TUNNEL_STEPS / 4);
                // Each tunnel walks its own random so stopping early (out of
                // reach of this column) never shifts the source chunk's
                // remaining draws: every column sees the same caves.
                let seed = rng.next_u32();
                let tunnel = Tunnel {
                    x,
                    y,
                    z,
                    yaw,
                    pitch,
                    thickness,
                    y_scale: p.kind.tunnel_y_scale(),
                    start: 0,
                    steps,
                };
                carved |= self.tunnel(ctx, seed, &shape, tunnel);
            }
        }
        carved
    }

    /// Walk one tunnel. Branches recurse once (branch thickness is below 1,
    /// so branches never split again).
    fn tunnel<W: BlockWorld>(
        &self,
        ctx: &mut Ctx<'_, W>,
        seed: u32,
        shape: &Shape,
        t: Tunnel,
    ) -> bool {
        let mut rng = BedrockRandom::new(seed);
        let width = f64::from(self.params.width_modifier);
        let Tunnel {
            mut x,
            mut y,
            mut z,
            mut yaw,
            mut pitch,
            thickness,
            y_scale,
            start,
            steps,
        } = t;
        let (center_x, center_z) = (f64::from(self.x0) + 8.0, f64::from(self.z0) + 8.0);
        let split = rng.next_int(steps / 2) + steps / 4;
        // One tunnel in six keeps its pitch longer (steeper shafts).
        let steep = rng.next_int(6) == 0;
        let (mut yaw_change, mut pitch_change) = (0.0f64, 0.0f64);
        let mut carved = false;
        for step in start..steps {
            let radius = 1.5 + (PI * f64::from(step) / f64::from(steps)).sin() * thickness + width;
            let vertical = radius * y_scale;
            let cos_pitch = pitch.cos();
            x += yaw.cos() * cos_pitch;
            y += pitch.sin();
            z += yaw.sin() * cos_pitch;
            pitch *= if steep { 0.92 } else { 0.7 };
            pitch += pitch_change * 0.1;
            yaw += yaw_change * 0.1;
            pitch_change *= 0.9;
            yaw_change *= 0.75;
            pitch_change +=
                f64::from((rng.next_float() - rng.next_float()) * rng.next_float() * 2.0);
            yaw_change += f64::from((rng.next_float() - rng.next_float()) * rng.next_float() * 4.0);
            if step == split && thickness > 1.0 {
                for turn in [-FRAC_PI_2, FRAC_PI_2] {
                    let branch_seed = rng.next_u32();
                    let branch = Tunnel {
                        x,
                        y,
                        z,
                        yaw: yaw + turn,
                        pitch: pitch / 3.0,
                        thickness: f64::from(rng.next_float()) * 0.5 + 0.5,
                        y_scale: 1.0,
                        start: step,
                        steps,
                    };
                    carved |= self.tunnel(ctx, branch_seed, shape, branch);
                }
                return carved;
            }
            if rng.next_int(4) == 0 {
                continue;
            }
            if !can_reach(center_x, center_z, x, z, step, steps, thickness) {
                return carved;
            }
            carved |= self.carve_ellipsoid(
                ctx,
                x,
                y,
                z,
                radius * shape.horizontal,
                vertical * shape.vertical,
                shape.floor,
            );
        }
        carved
    }

    /// Carve the part of one ellipsoid inside the target column.
    #[allow(clippy::too_many_arguments)]
    fn carve_ellipsoid<W: BlockWorld>(
        &self,
        ctx: &mut Ctx<'_, W>,
        cx: f64,
        cy: f64,
        cz: f64,
        horizontal: f64,
        vertical: f64,
        floor: f64,
    ) -> bool {
        let Some((x_lo, x_hi)) = clip_span(cx, horizontal, self.x0, self.x0 + 15) else {
            return false;
        };
        let Some((z_lo, z_hi)) = clip_span(cz, horizontal, self.z0, self.z0 + 15) else {
            return false;
        };
        // Never the bottom layer (vanilla keeps `min_y` intact).
        let Some((y_lo, y_hi)) = clip_span(cy, vertical, self.min_y + 1, self.max_y - 1) else {
            return false;
        };
        if self.params.kind == Kind::UnderwaterCave
            && !touches_water_box(ctx, (x_lo, x_hi), (y_lo, y_hi), (z_lo, z_hi))
        {
            return false;
        }
        let mut carved = false;
        for x in x_lo..=x_hi {
            let dx = (f64::from(x) + 0.5 - cx) / horizontal;
            for z in z_lo..=z_hi {
                let dz = (f64::from(z) + 0.5 - cz) / horizontal;
                let dxz_sq = dx * dx + dz * dz;
                if dxz_sq >= 1.0 {
                    continue;
                }
                // Top-down so a carved surface block can be moved onto the
                // dirt it exposes (see `carve_block`).
                let mut exposed_top = None;
                for y in (y_lo..=y_hi).rev() {
                    let dy = (f64::from(y) + 0.5 - cy) / vertical;
                    if in_carve_shape(dxz_sq, dy, floor) {
                        carved |= self.carve_block(ctx, BlockPos::new(x, y, z), &mut exposed_top);
                    }
                }
            }
        }
        carved
    }

    /// Carve one block of the volume.
    fn carve_block<W: BlockWorld>(
        &self,
        ctx: &mut Ctx<'_, W>,
        at: BlockPos,
        exposed_top: &mut Option<u32>,
    ) -> bool {
        let p = self.params;
        let id = ctx.get(at);
        if bb::air(id) {
            return p.replace_air_with.is_some_and(|with| ctx.set(at, with));
        }
        let fill = match p.kind {
            Kind::Cave => {
                if !overworld_carvable(id) || touches(ctx, at, bb::water) {
                    return false;
                }
                if at.y <= self.min_y + LAVA_ABOVE_BOTTOM {
                    bb::LAVA
                } else {
                    p.fill_with
                }
            }
            Kind::UnderwaterCave => {
                if !overworld_carvable(id) {
                    return false;
                }
                p.fill_with
            }
            Kind::NetherCave => {
                // The nether lava sea must not pour into the caves.
                if !bb::nether_carver_replaceable(id)
                    || touches(ctx, at, |b| bb::lava(b) || bb::water(b))
                {
                    return false;
                }
                p.fill_with
            }
        };
        if !ctx.set(at, fill) {
            return false;
        }
        // Vanilla: when a cave opens a grass/mycelium surface, the dirt it
        // exposes below becomes that surface block.
        if p.kind == Kind::Cave && fill != bb::LAVA {
            if bb::carver_surface_top(id) {
                *exposed_top = Some(id);
            }
            if let Some(top) = *exposed_top {
                let below = at.below();
                if ctx.get(below) == bb::DIRT {
                    ctx.set(below, top);
                }
            }
        }
        true
    }
}

/// Whether any of the six neighbours of `at` matches `pred`.
#[inline]
fn touches<W: BlockWorld>(ctx: &Ctx<'_, W>, at: BlockPos, pred: impl Fn(u32) -> bool) -> bool {
    NEIGHBOURS
        .iter()
        .any(|&(dx, dy, dz)| pred(ctx.get(at.offset(dx, dy, dz))))
}

/// Whether the box (grown by one block on every side) contains water: the
/// underwater carver only extends existing water (an ocean floor or a
/// flooded cave) instead of leaving sealed water pockets in solid rock.
fn touches_water_box<W: BlockWorld>(
    ctx: &Ctx<'_, W>,
    xs: (i32, i32),
    ys: (i32, i32),
    zs: (i32, i32),
) -> bool {
    for x in xs.0 - 1..=xs.1 + 1 {
        for z in zs.0 - 1..=zs.1 + 1 {
            for y in ys.0 - 1..=ys.1 + 1 {
                if bb::water(ctx.get(BlockPos::new(x, y, z))) {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::super::super::world::{NoiseSource, OUT_OF_BOUNDS};
    use super::*;

    struct NoNoise;
    impl NoiseSource for NoNoise {
        fn noise(&self, _x: f32, _z: f32) -> f32 {
            0.0
        }
    }

    /// A box of blocks: `[x0, x0 + size) x [min_y, max_y) x [z0, z0 + size)`.
    #[derive(Clone, PartialEq)]
    struct Boxed {
        x0: i32,
        z0: i32,
        size: i32,
        min_y: i32,
        max_y: i32,
        blocks: Vec<u32>,
    }

    impl Boxed {
        fn filled(x0: i32, z0: i32, size: i32, min_y: i32, max_y: i32, id: u32) -> Self {
            let n = (size * size * (max_y - min_y)) as usize;
            Self {
                x0,
                z0,
                size,
                min_y,
                max_y,
                blocks: vec![id; n],
            }
        }

        fn index(&self, p: BlockPos) -> Option<usize> {
            let (x, y, z) = (p.x - self.x0, p.y - self.min_y, p.z - self.z0);
            let inside = (0..self.size).contains(&x)
                && (0..self.max_y - self.min_y).contains(&y)
                && (0..self.size).contains(&z);
            inside.then(|| ((y * self.size + z) * self.size + x) as usize)
        }
    }

    impl BlockWorld for Boxed {
        fn min_y(&self) -> i32 {
            self.min_y
        }
        fn max_y(&self) -> i32 {
            self.max_y
        }
        fn get(&self, pos: BlockPos) -> u32 {
            self.index(pos).map_or(OUT_OF_BOUNDS, |i| self.blocks[i])
        }
        fn set(&mut self, pos: BlockPos, id: u32) -> bool {
            match self.index(pos) {
                Some(i) => {
                    self.blocks[i] = id;
                    true
                }
                None => false,
            }
        }
        fn biome(&self, _pos: BlockPos) -> u16 {
            0
        }
        fn heightmap(&mut self, _x: i32, _z: i32) -> i32 {
            self.max_y
        }
        fn above_top_solid(&mut self, _x: i32, _z: i32) -> i32 {
            self.max_y
        }
    }

    fn cave(skip: i32) -> Params {
        Params {
            kind: Kind::Cave,
            salt: 0x1234,
            fill_with: bb::AIR,
            replace_air_with: None,
            width_modifier: 0.0,
            skip_carve_chance: skip,
            height_limit: 100,
            y_scale: FloatRange { min: 0.1, max: 0.9 },
            horizontal_radius_multiplier: FloatRange { min: 0.7, max: 1.4 },
            vertical_radius_multiplier: FloatRange { min: 0.8, max: 1.3 },
            floor_level: FloatRange {
                min: -1.0,
                max: -0.4,
            },
        }
    }

    fn run(world: &mut Boxed, pos: BlockPos, params: &Params, seed: i64) -> Option<BlockPos> {
        let mut rng = BedrockRandom::new(1);
        let mut ctx = Ctx {
            world,
            rng: &mut rng,
            noise: &NoNoise,
            seed,
        };
        place(&mut ctx, pos, params)
    }

    #[test]
    fn source_seed_is_deterministic_and_rolls_one_in_n() {
        let a = source_seed(42, 7, 3, -5, 1);
        assert_eq!(a, source_seed(42, 7, 3, -5, 1));
        assert!(a.is_some());
        // Different chunk, salt or world seed: a different random.
        assert_ne!(a, source_seed(42, 7, -5, 3, 1));
        assert_ne!(a, source_seed(42, 8, 3, -5, 1));
        assert_ne!(a, source_seed(43, 7, 3, -5, 1));
        // Over many chunks, about 1/33 start caves.
        let mut hits = 0;
        for x in -100..100 {
            for z in -50..50 {
                hits += i32::from(source_seed(-9_876_543_210, 99, x, z, 33).is_some());
            }
        }
        let expected = 20_000 / 33;
        assert!(
            (hits - expected).abs() < expected / 5,
            "{hits} vs {expected}"
        );
    }

    #[test]
    fn clip_span_bounds_block_centres() {
        // Centres 2.5..=7.5 lie within 3 of 5.2 (|c - 5.2| < 3).
        assert_eq!(clip_span(5.2, 3.0, -100, 100), Some((2, 8)));
        assert_eq!(clip_span(5.2, 3.0, 4, 6), Some((4, 6)));
        assert_eq!(clip_span(5.2, 3.0, 9, 20), None);
        assert_eq!(clip_span(5.2, 3.0, -20, 1), None);
    }

    #[test]
    fn carve_shape_floor_cut_off() {
        assert!(in_carve_shape(0.0, 0.0, -1.0));
        assert!(!in_carve_shape(0.0, -0.5, -0.5));
        assert!(in_carve_shape(0.0, -0.49, -0.5));
        assert!(!in_carve_shape(0.9, 0.4, -1.0));
        assert!(!in_carve_shape(0.0, 1.0, -1.0));
    }

    #[test]
    fn ellipsoid_respects_floor_and_column() {
        let mut world = Boxed::filled(-16, -16, 48, 0, 32, bb::STONE);
        let params = Params {
            floor_level: FloatRange {
                min: -0.5,
                max: -0.5,
            },
            ..cave(1)
        };
        let carver = Carver {
            params: &params,
            x0: 0,
            z0: 0,
            min_y: 0,
            max_y: 32,
        };
        let mut rng = BedrockRandom::new(1);
        let mut ctx = Ctx {
            world: &mut world,
            rng: &mut rng,
            noise: &NoNoise,
            seed: 0,
        };
        // Centre on the column edge: half of it lies in the next column.
        assert!(carver.carve_ellipsoid(&mut ctx, 15.5, 20.0, 8.5, 6.0, 4.0, -0.5));
        // Floor at 20 - 0.5 * 4 = 18: centre 18.5 carved, 17.5 kept.
        assert_eq!(world.get(BlockPos::new(15, 18, 8)), bb::AIR);
        assert_eq!(world.get(BlockPos::new(15, 17, 8)), bb::STONE);
        assert_eq!(world.get(BlockPos::new(15, 23, 8)), bb::AIR);
        assert_eq!(world.get(BlockPos::new(15, 24, 8)), bb::STONE);
        // Nothing outside [0, 16) x [0, 16).
        assert_eq!(world.get(BlockPos::new(16, 20, 8)), bb::STONE);
        assert_eq!(world.get(BlockPos::new(18, 20, 8)), bb::STONE);
        assert_eq!(world.get(BlockPos::new(14, 20, 8)), bb::AIR);
    }

    #[test]
    fn carving_is_deterministic_and_clipped_to_the_column() {
        let params = cave(1);
        let fresh = Boxed::filled(-16, -16, 48, -64, 64, bb::STONE);
        let pos = BlockPos::new(0, 0, 0);
        let mut a = fresh.clone();
        let mut b = fresh.clone();
        assert_eq!(run(&mut a, pos, &params, 1234), Some(pos));
        assert_eq!(run(&mut b, pos, &params, 1234), Some(pos));
        assert!(a == b, "same seed, same caves");
        let mut carved = 0;
        for (i, (&after, &before)) in a.blocks.iter().zip(&fresh.blocks).enumerate() {
            if after == before {
                continue;
            }
            carved += 1;
            let i = i as i32;
            let (x, z, y) = (i % 48 - 16, i / 48 % 48 - 16, i / (48 * 48) - 64);
            assert!(
                (0..16).contains(&x) && (0..16).contains(&z),
                "carved outside the column at {x} {y} {z}"
            );
            assert!(y > -64, "bottom layer carved");
            let expected = if y <= -56 { bb::LAVA } else { bb::AIR };
            assert_eq!(after, expected, "fill at y {y}");
        }
        assert!(carved > 0);
        let mut other = fresh.clone();
        run(&mut other, pos, &params, 99);
        assert!(other != a, "another world seed carves other caves");
    }

    #[test]
    fn neighbouring_columns_agree_at_their_border() {
        // Carving column 0 then column 1 equals column 1 then column 0, and
        // each column's result is independent of the other's.
        let params = cave(1);
        let fresh = Boxed::filled(-16, -16, 48, -64, 64, bb::STONE);
        let (p0, p1) = (BlockPos::new(0, 0, 0), BlockPos::new(16, 0, 0));
        let mut ab = fresh.clone();
        run(&mut ab, p0, &params, 5);
        run(&mut ab, p1, &params, 5);
        let mut ba = fresh.clone();
        run(&mut ba, p1, &params, 5);
        run(&mut ba, p0, &params, 5);
        assert!(ab == ba);
    }

    #[test]
    fn dry_carver_never_touches_water() {
        let params = cave(1);
        let mut world = Boxed::filled(-16, -16, 48, -64, 64, bb::STONE);
        // A water layer through the whole column.
        for x in 0..16 {
            for z in 0..16 {
                for y in 0..8 {
                    world.set(BlockPos::new(x, y, z), bb::WATER);
                }
            }
        }
        run(&mut world, BlockPos::new(0, 0, 0), &params, 1234);
        for x in 0..16 {
            for z in 0..16 {
                for y in 0..8 {
                    assert_eq!(world.get(BlockPos::new(x, y, z)), bb::WATER);
                }
                assert_ne!(world.get(BlockPos::new(x, -1, z)), bb::AIR);
                assert_ne!(world.get(BlockPos::new(x, 8, z)), bb::AIR);
            }
        }
    }

    #[test]
    fn tunnels_stop_out_of_reach() {
        assert!(can_reach(8.0, 8.0, 100.0, 8.0, 0, 112, 2.0));
        assert!(!can_reach(8.0, 8.0, 200.0, 8.0, 100, 112, 2.0));
    }
}
