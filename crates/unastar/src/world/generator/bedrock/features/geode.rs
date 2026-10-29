//! `minecraft:geode_feature`: the vanilla amethyst geode, a layered shell
//! around a hollow core.
//!
//! Algorithm (shared by both editions):
//! - `n` distribution points are drawn at `outer_wall_distance` offsets
//!   (each axis) from `pos`, each with a `point_offset`. Every point that
//!   lands in air or a [`bb::geode_invalid`] block counts as invalid; more
//!   than `invalid_blocks_threshold` invalid points abort the geode.
//! - For every block within `max_radius` (a cube), the shell value is
//!   `s = sum(1 / sqrt(dist^2(block, point) + offset) + noise)` over the
//!   points, where `noise` is a smooth 3D noise times `noise_multiplier`.
//!   Thresholds `1 / sqrt(layer + d)` with `d = n / max_wall_distance` and
//!   the vanilla layer constants (filling 1.7, inner 2.2, middle 3.2, outer
//!   4.2; the filling threshold has no `d`) pick, from the inside out:
//!   `filler`, the inner layer (`alternate_inner_layer` with
//!   `use_alternate_layer0_chance`), `middle_layer`, `outer_layer`; blocks
//!   with `s` below the outer threshold are untouched.
//! - With `generate_crack_chance` the shell gets a crack: three points on a
//!   vertical line at one of four corners of the geode; shell blocks (not
//!   the core) whose crack value
//!   `sum(1 / sqrt(dist^2 + crack_point_offset) + noise)` reaches
//!   `1 / sqrt(base_crack_size + r/2 + (n > 3 ? d : 0))` become air.
//! - Inner-layer blocks are kept as growth candidates with
//!   `use_potential_placements_chance` (only alternate/budding ones when
//!   `placements_require_layer0_alternate`). Afterwards each candidate grows
//!   one random `inner_placements` block into its first free neighbour (air
//!   or a water source) in the order down, up, north, south, west, east,
//!   facing away from the shell.
//!
//! Every write skips [`bb::geode_cannot_replace`] blocks (bedrock, spawners,
//! chests, ...). The world stores one block per position, so a cluster
//! growing into water replaces it (vanilla Bedrock keeps the water in a
//! second block layer; not representable here). Vanilla schedules fluid
//! updates next to the crack; decoration has no tick queue, so it does not.

use super::super::generated::bb;
use super::super::world::{BlockPos, BlockWorld, Ctx, OUT_OF_BOUNDS};
use super::IntRange;
use smallvec::SmallVec;
use unastar_noise::{PerlinNoise, Xoroshiro128};

/// Vanilla shell layer constants (not part of the pack data).
const FILLING: f64 = 1.7;
const INNER_LAYER: f64 = 2.2;
const MIDDLE_LAYER: f64 = 3.2;
const OUTER_LAYER: f64 = 4.2;

/// Growth directions of `inner_placements`, in the order of
/// [`Params::inner_placements`]' state columns (Bedrock `minecraft:block_face`
/// order): down, up, north, south, west, east.
const FACES: [(i32, i32, i32); 6] = [
    (0, -1, 0),
    (0, 1, 0),
    (0, 0, -1),
    (0, 0, 1),
    (-1, 0, 0),
    (1, 0, 0),
];

/// Parameters of one geode feature.
#[derive(Debug, Clone, Copy)]
pub struct Params {
    /// Core (`filler`).
    pub filler: u32,
    /// Layer 0 (`inner_layer`).
    pub inner_layer: u32,
    /// Layer 0 variant (`alternate_inner_layer`, budding amethyst).
    pub alternate_inner_layer: u32,
    /// `middle_layer`.
    pub middle_layer: u32,
    /// `outer_layer`.
    pub outer_layer: u32,
    /// `inner_placements`: per block, its states facing down, up, north,
    /// south, west, east (all equal for blocks without a facing).
    pub inner_placements: &'static [[u32; 6]],
    /// Per-axis offset of distribution points (inclusive; the pack's
    /// `max_outer_wall_distance` is exclusive).
    pub outer_wall_distance: IntRange,
    /// Number of distribution points (inclusive; pack max exclusive).
    pub distribution_points: IntRange,
    /// Distance offset added per point (inclusive; pack max exclusive).
    pub point_offset: IntRange,
    /// Half-size of the cube the shell is evaluated in.
    pub max_radius: i32,
    pub crack_point_offset: f32,
    pub generate_crack_chance: f32,
    pub base_crack_size: f32,
    pub noise_multiplier: f32,
    pub use_potential_placements_chance: f32,
    pub use_alternate_layer0_chance: f32,
    pub placements_require_layer0_alternate: bool,
    pub invalid_blocks_threshold: i32,
}

/// Place one geode with its distribution points offset from `pos`;
/// `Some(pos)` when any block changed.
pub fn place<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    pos: BlockPos,
    params: &Params,
) -> Option<BlockPos> {
    // The point count is the first draw: in both native seed-12345 geodes it
    // is the only one of the first eight draws giving their measured three
    // points (shell radii 3.4 / 4.8 / 5.7 / 6.9 fit n = 3 with the vanilla
    // thresholds). The order of the remaining draws is unmeasured.
    let points = params.distribution_points.sample(ctx.rng).max(1);
    let noise = ShellNoise::new(i64::from(ctx.rng.next_u32()));
    let crack_jitter = f64::from(ctx.rng.next_float());
    let t = Thresholds::new(
        points,
        params.outer_wall_distance.max,
        f64::from(params.base_crack_size),
        crack_jitter,
    );
    let generate_crack = ctx.rng.probability(params.generate_crack_chance);

    // Vanilla draws 3..=4 centres; larger custom counts retain growable storage.
    let mut centres: SmallVec<[(BlockPos, f64); 4]> = SmallVec::with_capacity(points as usize);
    let mut invalid = 0;
    for _ in 0..points {
        let dx = params.outer_wall_distance.sample(ctx.rng);
        let dy = params.outer_wall_distance.sample(ctx.rng);
        let dz = params.outer_wall_distance.sample(ctx.rng);
        let at = pos.offset(dx, dy, dz);
        let b = ctx.get(at);
        if b == OUT_OF_BOUNDS || bb::air(b) || bb::geode_invalid(b) {
            invalid += 1;
            if invalid > params.invalid_blocks_threshold {
                return None;
            }
        }
        // Invalid points still shape the geode (vanilla keeps them).
        centres.push((at, f64::from(params.point_offset.sample(ctx.rng))));
    }

    let cracks = crack_offsets(ctx.rng.next_int(4), points).map(|(x, y, z)| pos.offset(x, y, z));
    let cracks: &[BlockPos] = if generate_crack { &cracks } else { &[] };

    let r = params.max_radius;
    let noise_multiplier = f64::from(params.noise_multiplier);
    let crack_point_offset = f64::from(params.crack_point_offset);
    // Keep common growth-candidate scratch inline; larger geodes still spill.
    let mut potential: SmallVec<[BlockPos; 32]> = SmallVec::new();
    let mut placed = false;
    for dx in -r..=r {
        for dz in -r..=r {
            for dy in -r..=r {
                let at = pos.offset(dx, dy, dz);
                let n = noise.sample(at) * noise_multiplier;
                let s: f64 = centres
                    .iter()
                    .map(|&(c, offset)| inv_sqrt(dist_sq(at, c) + offset) + n)
                    .sum();
                if s < t.outer {
                    continue;
                }
                let block = if s < t.filling
                    && !cracks.is_empty()
                    && cracks
                        .iter()
                        .map(|&c| inv_sqrt(dist_sq(at, c) + crack_point_offset) + n)
                        .sum::<f64>()
                        >= t.crack
                {
                    bb::AIR
                } else if s >= t.filling {
                    params.filler
                } else if s >= t.inner {
                    let alternate = ctx.rng.probability(params.use_alternate_layer0_chance);
                    if (!params.placements_require_layer0_alternate || alternate)
                        && ctx.rng.probability(params.use_potential_placements_chance)
                    {
                        potential.push(at);
                    }
                    if alternate {
                        params.alternate_inner_layer
                    } else {
                        params.inner_layer
                    }
                } else if s >= t.middle {
                    params.middle_layer
                } else {
                    params.outer_layer
                };
                placed |= safe_set(ctx, at, block);
            }
        }
    }

    let kinds = params.inner_placements;
    if !kinds.is_empty() {
        for at in potential {
            let faces = &kinds[ctx.rng.next_int(kinds.len() as i32) as usize];
            for (face, &(dx, dy, dz)) in FACES.iter().enumerate() {
                let q = at.offset(dx, dy, dz);
                let b = ctx.get(q);
                if bb::air(b) || b == bb::WATER {
                    placed |= safe_set(ctx, q, faces[face]);
                    break;
                }
            }
        }
    }
    placed.then_some(pos)
}

/// Shell value thresholds of one geode.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Thresholds {
    filling: f64,
    inner: f64,
    middle: f64,
    outer: f64,
    crack: f64,
}

impl Thresholds {
    /// `d = points / max_wall_distance`; `crack_jitter` is a `[0, 1)` draw.
    fn new(points: i32, max_wall_distance: i32, base_crack_size: f64, crack_jitter: f64) -> Self {
        let d = f64::from(points) / f64::from(max_wall_distance.max(1));
        let crack_extra = if points > 3 { d } else { 0.0 };
        Self {
            filling: inv_sqrt(FILLING),
            inner: inv_sqrt(INNER_LAYER + d),
            middle: inv_sqrt(MIDDLE_LAYER + d),
            outer: inv_sqrt(OUTER_LAYER + d),
            crack: inv_sqrt(base_crack_size + crack_jitter / 2.0 + crack_extra),
        }
    }
}

/// The three crack points (relative to the geode origin) for corner `index`
/// (0..4): a vertical line at heights 7, 5, 1 at `x` and/or `z` offset
/// `2 * points + 1`.
fn crack_offsets(index: i32, points: i32) -> [(i32, i32, i32); 3] {
    let c = points * 2 + 1;
    let (x, z) = match index {
        0 => (c, 0),
        1 => (0, c),
        2 => (c, c),
        _ => (0, 0),
    };
    [(x, 7, z), (x, 5, z), (x, 1, z)]
}

#[inline]
fn inv_sqrt(v: f64) -> f64 {
    1.0 / v.sqrt()
}

#[inline]
fn dist_sq(a: BlockPos, b: BlockPos) -> f64 {
    let (dx, dy, dz) = (
        f64::from(a.x - b.x),
        f64::from(a.y - b.y),
        f64::from(a.z - b.z),
    );
    dx * dx + dy * dy + dz * dz
}

/// Set unless the current block is protected; `true` when it changed.
#[inline]
fn safe_set<W: BlockWorld>(ctx: &mut Ctx<'_, W>, at: BlockPos, id: u32) -> bool {
    !bb::geode_cannot_replace(ctx.get(at)) && ctx.set(at, id)
}

/// The shell noise: a vanilla-style "normal" noise (two Perlin noises, the
/// second sampled at `337/331` of the frequency, scaled by 5/6) at frequency
/// 1/16, seeded per geode. Values are roughly in `[-1, 1]`; the pack's
/// `noise_multiplier` scales them.
struct ShellNoise {
    a: PerlinNoise,
    b: PerlinNoise,
}

impl ShellNoise {
    fn new(seed: i64) -> Self {
        let mut rng = Xoroshiro128::from_seed(seed);
        let a = PerlinNoise::new(&mut rng);
        let b = PerlinNoise::new(&mut rng);
        Self { a, b }
    }

    #[inline]
    fn sample(&self, at: BlockPos) -> f64 {
        const FREQUENCY: f64 = 1.0 / 16.0;
        const SECOND: f64 = 337.0 / 331.0;
        let (x, y, z) = (
            f64::from(at.x) * FREQUENCY,
            f64::from(at.y) * FREQUENCY,
            f64::from(at.z) * FREQUENCY,
        );
        (self.a.sample(x, y, z) + self.b.sample(x * SECOND, y * SECOND, z * SECOND)) * (5.0 / 6.0)
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::rng::BedrockRandom;
    use super::super::super::world::NoiseSource;
    use super::*;

    struct NoNoise;
    impl NoiseSource for NoNoise {
        fn noise(&self, _x: f32, _z: f32) -> f32 {
            0.0
        }
    }

    /// A cube of blocks `[-half, half)` on every axis.
    #[derive(Clone, PartialEq)]
    struct Cube {
        half: i32,
        blocks: Vec<u32>,
        secondary: std::collections::HashMap<BlockPos, u32>,
        block_entities: Vec<crate::world::block_entity::BlockEntity>,
    }

    impl Cube {
        fn filled(half: i32, id: u32) -> Self {
            let side = (2 * half) as usize;
            Self {
                half,
                blocks: vec![id; side * side * side],
                secondary: Default::default(),
                block_entities: Vec::new(),
            }
        }

        fn index(&self, p: BlockPos) -> Option<usize> {
            let side = 2 * self.half;
            let (x, y, z) = (p.x + self.half, p.y + self.half, p.z + self.half);
            let inside = (0..side).contains(&x) && (0..side).contains(&y) && (0..side).contains(&z);
            inside.then(|| ((y * side + z) * side + x) as usize)
        }

        fn count(&self, id: u32) -> usize {
            self.blocks.iter().filter(|&&b| b == id).count()
        }
    }

    impl BlockWorld for Cube {
        fn min_y(&self) -> i32 {
            -self.half
        }
        fn max_y(&self) -> i32 {
            self.half
        }
        fn get(&self, pos: BlockPos) -> u32 {
            self.index(pos).map_or(OUT_OF_BOUNDS, |i| self.blocks[i])
        }
        fn get_layer(&self, pos: BlockPos, layer: u8) -> u32 {
            match (self.index(pos), layer) {
                (Some(i), 0) => self.blocks[i],
                (Some(_), 1) => self.secondary.get(&pos).copied().unwrap_or(bb::AIR),
                _ => OUT_OF_BOUNDS,
            }
        }
        fn set(&mut self, pos: BlockPos, id: u32) -> bool {
            match self.index(pos) {
                Some(i) => {
                    self.blocks[i] = id;
                    self.secondary.remove(&pos);
                    true
                }
                None => false,
            }
        }
        fn set_layers(&mut self, pos: BlockPos, primary: u32, secondary: Option<u32>) -> bool {
            if !self.set(pos, primary) {
                return false;
            }
            if let Some(id) = secondary.filter(|&id| id != bb::AIR) {
                self.secondary.insert(pos, id);
            }
            true
        }
        fn set_block_entity(&mut self, entity: crate::world::block_entity::BlockEntity) -> bool {
            let position = entity.position();
            if self
                .index(BlockPos::new(position[0], position[1], position[2]))
                .is_none()
            {
                return false;
            }
            if let Some(existing) = self
                .block_entities
                .iter_mut()
                .find(|existing| existing.position() == position)
            {
                *existing = entity;
            } else {
                self.block_entities.push(entity);
            }
            true
        }
        fn biome(&self, _pos: BlockPos) -> u16 {
            0
        }
        fn heightmap(&mut self, _x: i32, _z: i32) -> i32 {
            self.half
        }
        fn above_top_solid(&mut self, _x: i32, _z: i32) -> i32 {
            self.half
        }
    }

    /// One "placement" block per face, so the face it grew in is visible.
    static PLACEMENTS: [[u32; 6]; 1] = [[
        bb::SAND,
        bb::RED_SAND,
        bb::GRAVEL,
        bb::CLAY,
        bb::ICE,
        bb::DIRT,
    ]];

    fn params() -> Params {
        Params {
            filler: bb::AIR,
            inner_layer: bb::CALCITE,
            alternate_inner_layer: bb::MAGMA,
            middle_layer: bb::TUFF,
            outer_layer: bb::OBSIDIAN,
            inner_placements: &PLACEMENTS,
            outer_wall_distance: IntRange { min: 4, max: 6 },
            distribution_points: IntRange { min: 3, max: 4 },
            point_offset: IntRange { min: 1, max: 2 },
            max_radius: 16,
            crack_point_offset: 2.0,
            generate_crack_chance: 0.0,
            base_crack_size: 2.0,
            noise_multiplier: 0.025,
            use_potential_placements_chance: 1.0,
            use_alternate_layer0_chance: 0.083,
            placements_require_layer0_alternate: false,
            invalid_blocks_threshold: 1,
        }
    }

    fn run(world: &mut Cube, params: &Params, seed: u32) -> Option<BlockPos> {
        let mut rng = BedrockRandom::new(seed);
        let mut ctx = Ctx {
            world,
            rng: &mut rng,
            noise: &NoNoise,
            seed: 0,
        };
        place(&mut ctx, BlockPos::new(0, 0, 0), params)
    }

    #[test]
    fn thresholds_nest_from_the_core_outwards() {
        let t = Thresholds::new(4, 6, 2.0, 0.5);
        assert!(t.filling > t.inner && t.inner > t.middle && t.middle > t.outer);
        let d = 4.0 / 6.0;
        assert!((t.outer - 1.0 / (4.2f64 + d).sqrt()).abs() < 1e-12);
        assert!((t.crack - 1.0 / (2.25f64 + d).sqrt()).abs() < 1e-12);
        // Three points: no `d` in the crack threshold.
        assert!((Thresholds::new(3, 6, 2.0, 0.0).crack - 1.0 / 2.0f64.sqrt()).abs() < 1e-12);
    }

    #[test]
    fn crack_points_form_a_vertical_line() {
        assert_eq!(crack_offsets(0, 3), [(7, 7, 0), (7, 5, 0), (7, 1, 0)]);
        assert_eq!(crack_offsets(1, 4), [(0, 7, 9), (0, 5, 9), (0, 1, 9)]);
        assert_eq!(crack_offsets(2, 3), [(7, 7, 7), (7, 5, 7), (7, 1, 7)]);
        assert_eq!(crack_offsets(3, 3), [(0, 7, 0), (0, 5, 0), (0, 1, 0)]);
    }

    #[test]
    fn geode_has_layers_and_outward_facing_growth() {
        let mut world = Cube::filled(24, bb::STONE);
        assert!(run(&mut world, &params(), 7).is_some());
        for id in [bb::AIR, bb::TUFF, bb::OBSIDIAN] {
            assert!(world.count(id) > 0, "layer {id} missing");
        }
        assert!(world.count(bb::CALCITE) + world.count(bb::MAGMA) > 0);
        // Layers nest: measured from the core's centroid, the inner layer
        // lies outside the core, the middle layer outside it, the outer
        // layer outermost.
        let positions = |id: u32| {
            let mut out = Vec::new();
            for x in -24..24 {
                for y in -24..24 {
                    for z in -24..24 {
                        if world.get(BlockPos::new(x, y, z)) == id {
                            out.push((f64::from(x), f64::from(y), f64::from(z)));
                        }
                    }
                }
            }
            out
        };
        let core = positions(bb::AIR);
        let n = core.len() as f64;
        let centre = core.iter().fold((0.0, 0.0, 0.0), |a, p| {
            (a.0 + p.0 / n, a.1 + p.1 / n, a.2 + p.2 / n)
        });
        let mean_distance = |id: u32| {
            let ps = positions(id);
            let sum: f64 = ps
                .iter()
                .map(|p| {
                    ((p.0 - centre.0).powi(2) + (p.1 - centre.1).powi(2) + (p.2 - centre.2).powi(2))
                        .sqrt()
                })
                .sum();
            sum / ps.len() as f64
        };
        let (core_d, inner_d, middle_d, outer_d) = (
            mean_distance(bb::AIR),
            mean_distance(bb::CALCITE),
            mean_distance(bb::TUFF),
            mean_distance(bb::OBSIDIAN),
        );
        assert!(
            core_d < inner_d && inner_d < middle_d && middle_d < outer_d,
            "{core_d} {inner_d} {middle_d} {outer_d}"
        );
        let mut grown = 0;
        for (face, &(dx, dy, dz)) in FACES.iter().enumerate() {
            let id = PLACEMENTS[0][face];
            for x in -24..24 {
                for y in -24..24 {
                    for z in -24..24 {
                        if world.get(BlockPos::new(x, y, z)) != id {
                            continue;
                        }
                        grown += 1;
                        let support = world.get(BlockPos::new(x - dx, y - dy, z - dz));
                        assert!(
                            support == bb::CALCITE || support == bb::MAGMA,
                            "{id} at {x} {y} {z} not on the shell"
                        );
                    }
                }
            }
        }
        assert!(grown > 0);
    }

    #[test]
    fn geode_is_deterministic() {
        let mut a = Cube::filled(24, bb::STONE);
        let mut b = a.clone();
        run(&mut a, &params(), 11);
        run(&mut b, &params(), 11);
        assert!(a == b);
    }

    #[test]
    fn too_many_invalid_points_abort() {
        let mut world = Cube::filled(24, bb::AIR);
        let before = world.clone();
        assert_eq!(run(&mut world, &params(), 3), None);
        assert!(world == before);
    }

    /// The seed-12345 geode in chunk (-4,3), exported from a natural BDS
    /// 1.26.51 world with all eight neighbouring chunks: 413 smooth basalt,
    /// 310 calcite and 239 amethyst/budding blocks, a three-point shell. Its
    /// feature random starts from the rule seed `3502308776`. With the point
    /// count drawn second, this seed gave four points and about three times
    /// the native volume.
    #[test]
    fn native_geode_seed_draws_three_points_first() {
        let vanilla = Params {
            generate_crack_chance: 0.95,
            use_potential_placements_chance: 0.35,
            placements_require_layer0_alternate: true,
            ..params()
        };
        let mut world = Cube::filled(24, bb::STONE);
        assert!(run(&mut world, &vanilla, 3_502_308_776).is_some());
        let inner = world.count(bb::CALCITE) + world.count(bb::MAGMA);
        let (middle, outer) = (world.count(bb::TUFF), world.count(bb::OBSIDIAN));
        for (layer, generated, native) in [
            ("inner", inner, 239),
            ("middle", middle, 310),
            ("outer", outer, 413),
        ] {
            assert!(
                generated * 4 >= native * 3 && generated * 4 <= native * 5,
                "{layer} layer {generated} blocks, native {native}"
            );
        }
    }
}
